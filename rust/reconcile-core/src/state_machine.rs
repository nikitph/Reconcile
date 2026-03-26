use crate::errors::KernelError;
use crate::types::Resource;
use std::collections::{HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// Guard function trait
// ---------------------------------------------------------------------------

pub trait GuardFn: Send + Sync {
    fn evaluate(&self, resource: &Resource) -> Result<bool, KernelError>;
}

// ---------------------------------------------------------------------------
// State and transition definitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateStatus {
    Active,
    Terminal,
}

#[derive(Debug, Clone)]
pub struct StateDefinition {
    pub name: String,
    pub status: StateStatus,
}

pub struct TransitionDefinition {
    pub from_state: String,
    pub to_state: String,
    pub guard: Option<Box<dyn GuardFn>>,
    pub required_role: Option<String>,
}

impl std::fmt::Debug for TransitionDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransitionDefinition")
            .field("from_state", &self.from_state)
            .field("to_state", &self.to_state)
            .field("has_guard", &self.guard.is_some())
            .field("required_role", &self.required_role)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

pub struct StateMachine {
    states: HashMap<String, StateDefinition>,
    transitions: Vec<TransitionDefinition>,
    initial_state: String,
    /// Pre-computed index: from_state -> [indices into self.transitions]
    transition_index: HashMap<String, Vec<usize>>,
}

impl StateMachine {
    pub fn new(
        states: Vec<StateDefinition>,
        transitions: Vec<TransitionDefinition>,
        initial_state: String,
    ) -> Result<Self, KernelError> {
        // Build state map
        let state_map: HashMap<String, StateDefinition> = states
            .into_iter()
            .map(|s| (s.name.clone(), s))
            .collect();

        // Validate initial state exists
        if !state_map.contains_key(&initial_state) {
            return Err(KernelError::StateNotDefined(initial_state));
        }

        // Validate all transition endpoints reference defined states
        for t in &transitions {
            if !state_map.contains_key(&t.from_state) {
                return Err(KernelError::StateNotDefined(t.from_state.clone()));
            }
            if !state_map.contains_key(&t.to_state) {
                return Err(KernelError::StateNotDefined(t.to_state.clone()));
            }
            // Terminal states must not have outbound transitions
            if let Some(s) = state_map.get(&t.from_state) {
                if s.status == StateStatus::Terminal {
                    return Err(KernelError::TerminalState {
                        state: t.from_state.clone(),
                    });
                }
            }
        }

        // Build transition index
        let mut transition_index: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, t) in transitions.iter().enumerate() {
            transition_index
                .entry(t.from_state.clone())
                .or_default()
                .push(i);
        }

        Ok(Self {
            states: state_map,
            transitions,
            initial_state,
            transition_index,
        })
    }

    pub fn initial_state(&self) -> &str {
        &self.initial_state
    }

    pub fn has_state(&self, name: &str) -> bool {
        self.states.contains_key(name)
    }

    pub fn is_terminal(&self, state: &str) -> bool {
        self.states
            .get(state)
            .map(|s| s.status == StateStatus::Terminal)
            .unwrap_or(false)
    }

    pub fn validate_transition(&self, from: &str, to: &str) -> bool {
        self.transition_index
            .get(from)
            .map(|indices| {
                indices
                    .iter()
                    .any(|&i| self.transitions[i].to_state == to)
            })
            .unwrap_or(false)
    }

    pub fn get_transition(&self, from: &str, to: &str) -> Option<&TransitionDefinition> {
        self.transition_index.get(from).and_then(|indices| {
            indices
                .iter()
                .find(|&&i| self.transitions[i].to_state == to)
                .map(|&i| &self.transitions[i])
        })
    }

    pub fn get_valid_transitions(&self, from: &str) -> Vec<&TransitionDefinition> {
        self.transition_index
            .get(from)
            .map(|indices| indices.iter().map(|&i| &self.transitions[i]).collect())
            .unwrap_or_default()
    }

    pub fn evaluate_guard(&self, from: &str, to: &str, resource: &Resource) -> Result<bool, KernelError> {
        match self.get_transition(from, to) {
            Some(t) => match &t.guard {
                Some(guard) => guard.evaluate(resource),
                None => Ok(true), // No guard = always passes
            },
            None => Err(KernelError::InvalidTransition {
                from: from.to_string(),
                to: to.to_string(),
            }),
        }
    }

    /// Find non-terminal states with no outbound transitions (dead ends).
    pub fn detect_dead_ends(&self) -> Vec<String> {
        self.states
            .iter()
            .filter(|(_, def)| def.status != StateStatus::Terminal)
            .filter(|(name, _)| !self.transition_index.contains_key(name.as_str()))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Find states not reachable from the initial state via BFS.
    pub fn detect_unreachable(&self) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        visited.insert(self.initial_state.clone());
        queue.push_back(self.initial_state.clone());

        while let Some(state) = queue.pop_front() {
            if let Some(indices) = self.transition_index.get(&state) {
                for &i in indices {
                    let to = &self.transitions[i].to_state;
                    if visited.insert(to.clone()) {
                        queue.push_back(to.clone());
                    }
                }
            }
        }

        self.states
            .keys()
            .filter(|name| !visited.contains(name.as_str()))
            .cloned()
            .collect()
    }

    /// Compute the shortest distance (in transitions) from each state to a target state.
    /// Returns None for states that can't reach the target.
    pub fn distance_to(&self, target: &str) -> HashMap<String, u32> {
        // BFS backward from target
        let mut distances = HashMap::new();
        let mut queue = VecDeque::new();

        distances.insert(target.to_string(), 0u32);
        queue.push_back(target.to_string());

        // Build reverse adjacency
        let mut reverse_adj: HashMap<String, Vec<String>> = HashMap::new();
        for t in &self.transitions {
            reverse_adj
                .entry(t.to_state.clone())
                .or_default()
                .push(t.from_state.clone());
        }

        while let Some(state) = queue.pop_front() {
            let dist = distances[&state];
            if let Some(predecessors) = reverse_adj.get(&state) {
                for pred in predecessors {
                    if !distances.contains_key(pred) {
                        distances.insert(pred.clone(), dist + 1);
                        queue.push_back(pred.clone());
                    }
                }
            }
        }

        distances
    }

    pub fn states(&self) -> impl Iterator<Item = &StateDefinition> {
        self.states.values()
    }

    pub fn state_names(&self) -> Vec<&str> {
        self.states.keys().map(|s| s.as_str()).collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn loan_states() -> Vec<StateDefinition> {
        vec![
            StateDefinition { name: "APPLIED".into(), status: StateStatus::Active },
            StateDefinition { name: "UNDERWRITING".into(), status: StateStatus::Active },
            StateDefinition { name: "APPROVED".into(), status: StateStatus::Active },
            StateDefinition { name: "DISBURSED".into(), status: StateStatus::Terminal },
            StateDefinition { name: "REJECTED".into(), status: StateStatus::Terminal },
        ]
    }

    fn loan_transitions() -> Vec<TransitionDefinition> {
        vec![
            TransitionDefinition { from_state: "APPLIED".into(), to_state: "UNDERWRITING".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "APPROVED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "REJECTED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "APPROVED".into(), to_state: "DISBURSED".into(), guard: None, required_role: None },
        ]
    }

    fn make_loan_sm() -> StateMachine {
        StateMachine::new(loan_states(), loan_transitions(), "APPLIED".into()).unwrap()
    }

    #[test]
    fn test_valid_transitions() {
        let sm = make_loan_sm();
        assert!(sm.validate_transition("APPLIED", "UNDERWRITING"));
        assert!(sm.validate_transition("UNDERWRITING", "APPROVED"));
        assert!(sm.validate_transition("UNDERWRITING", "REJECTED"));
        assert!(sm.validate_transition("APPROVED", "DISBURSED"));
    }

    #[test]
    fn test_invalid_transitions() {
        let sm = make_loan_sm();
        assert!(!sm.validate_transition("APPLIED", "APPROVED")); // skip
        assert!(!sm.validate_transition("DISBURSED", "APPLIED")); // from terminal
        assert!(!sm.validate_transition("APPLIED", "NONEXISTENT"));
    }

    #[test]
    fn test_terminal_states() {
        let sm = make_loan_sm();
        assert!(sm.is_terminal("DISBURSED"));
        assert!(sm.is_terminal("REJECTED"));
        assert!(!sm.is_terminal("APPLIED"));
        assert!(!sm.is_terminal("APPROVED"));
    }

    #[test]
    fn test_initial_state() {
        let sm = make_loan_sm();
        assert_eq!(sm.initial_state(), "APPLIED");
    }

    #[test]
    fn test_get_valid_transitions() {
        let sm = make_loan_sm();
        let from_uw: Vec<&str> = sm.get_valid_transitions("UNDERWRITING")
            .iter()
            .map(|t| t.to_state.as_str())
            .collect();
        assert!(from_uw.contains(&"APPROVED"));
        assert!(from_uw.contains(&"REJECTED"));
        assert_eq!(from_uw.len(), 2);
    }

    #[test]
    fn test_terminal_no_transitions() {
        let sm = make_loan_sm();
        assert!(sm.get_valid_transitions("DISBURSED").is_empty());
        assert!(sm.get_valid_transitions("REJECTED").is_empty());
    }

    #[test]
    fn test_no_dead_ends() {
        let sm = make_loan_sm();
        assert!(sm.detect_dead_ends().is_empty());
    }

    #[test]
    fn test_detect_dead_ends() {
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Active }, // No outbound!
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();
        let dead = sm.detect_dead_ends();
        assert_eq!(dead, vec!["B"]);
    }

    #[test]
    fn test_no_unreachable() {
        let sm = make_loan_sm();
        assert!(sm.detect_unreachable().is_empty());
    }

    #[test]
    fn test_detect_unreachable() {
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Terminal },
            StateDefinition { name: "C".into(), status: StateStatus::Terminal }, // Unreachable
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();
        let unreachable = sm.detect_unreachable();
        assert_eq!(unreachable, vec!["C"]);
    }

    #[test]
    fn test_invalid_initial_state() {
        let result = StateMachine::new(loan_states(), loan_transitions(), "BOGUS".into());
        assert!(result.is_err());
    }

    #[test]
    fn test_terminal_outbound_rejected() {
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Terminal },
            StateDefinition { name: "B".into(), status: StateStatus::Active },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
        ];
        let result = StateMachine::new(states, transitions, "A".into());
        assert!(matches!(result, Err(KernelError::TerminalState { .. })));
    }

    #[test]
    fn test_undefined_state_in_transition() {
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "NOWHERE".into(), guard: None, required_role: None },
        ];
        let result = StateMachine::new(states, transitions, "A".into());
        assert!(matches!(result, Err(KernelError::StateNotDefined(_))));
    }

    #[test]
    fn test_guard_evaluation() {
        struct AlwaysFail;
        impl GuardFn for AlwaysFail {
            fn evaluate(&self, _resource: &Resource) -> Result<bool, KernelError> {
                Ok(false)
            }
        }

        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition {
                from_state: "A".into(),
                to_state: "B".into(),
                guard: Some(Box::new(AlwaysFail)),
                required_role: None,
            },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();

        let resource = Resource {
            id: crate::types::ResourceId::new(),
            resource_type: "test".into(),
            state: "A".into(),
            desired_state: None,
            data: serde_json::json!({}),
            version: 1,
            tenant_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        assert_eq!(sm.evaluate_guard("A", "B", &resource).unwrap(), false);
    }

    #[test]
    fn test_distance_to() {
        let sm = make_loan_sm();
        let dist = sm.distance_to("DISBURSED");
        assert_eq!(dist["DISBURSED"], 0);
        assert_eq!(dist["APPROVED"], 1);
        assert_eq!(dist["UNDERWRITING"], 2);
        assert_eq!(dist["APPLIED"], 3);
        // REJECTED can't reach DISBURSED, so not in map
        assert!(!dist.contains_key("REJECTED"));
    }
}
