//! Workflow graphs — declarative process orchestration.
//!
//! Workflows define DAGs of steps: sequence (ordered), parallel (concurrent),
//! join (convergence), and branch (conditional routing). They compile to
//! state machine transitions and controllers.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Workflow definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum WorkflowStep {
    /// Execute states in order.
    Sequence(Vec<String>),
    /// Execute states concurrently (tracked via data flags).
    Parallel(Vec<String>),
    /// Wait for all parallel branches to complete before proceeding.
    Join(String),
    /// Route based on a data field value.
    Branch {
        on: String,
        paths: HashMap<String, Vec<WorkflowStep>>,
    },
}

#[derive(Debug, Clone)]
pub struct Workflow {
    pub name: String,
    pub resource_type: String,
    pub steps: Vec<WorkflowStep>,
}

impl Workflow {
    /// Extract all state names referenced in this workflow.
    pub fn all_states(&self) -> Vec<String> {
        let mut states = Vec::new();
        Self::collect_states(&self.steps, &mut states);
        states.sort();
        states.dedup();
        states
    }

    fn collect_states(steps: &[WorkflowStep], out: &mut Vec<String>) {
        for step in steps {
            match step {
                WorkflowStep::Sequence(states) => out.extend(states.clone()),
                WorkflowStep::Parallel(states) => out.extend(states.clone()),
                WorkflowStep::Join(state) => out.push(state.clone()),
                WorkflowStep::Branch { paths, .. } => {
                    for path_steps in paths.values() {
                        Self::collect_states(path_steps, out);
                    }
                }
            }
        }
    }

    /// Extract transitions implied by sequence steps.
    pub fn implied_transitions(&self) -> Vec<(String, String)> {
        let mut transitions = Vec::new();
        Self::collect_transitions(&self.steps, &mut transitions);
        transitions
    }

    fn collect_transitions(steps: &[WorkflowStep], out: &mut Vec<(String, String)>) {
        for step in steps {
            match step {
                WorkflowStep::Sequence(states) => {
                    for pair in states.windows(2) {
                        out.push((pair[0].clone(), pair[1].clone()));
                    }
                }
                WorkflowStep::Parallel(states) => {
                    // Parallel states are all reachable from the preceding state
                    // The join state is reachable from each parallel state
                }
                WorkflowStep::Join(_) => {}
                WorkflowStep::Branch { paths, .. } => {
                    for path_steps in paths.values() {
                        Self::collect_transitions(path_steps, out);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_states() {
        let wf = Workflow {
            name: "loan".into(),
            resource_type: "loan".into(),
            steps: vec![
                WorkflowStep::Sequence(vec!["APPLIED".into(), "UNDERWRITING".into()]),
                WorkflowStep::Parallel(vec!["DOC_CHECK".into(), "RISK_SCORE".into()]),
                WorkflowStep::Join("REVIEW".into()),
            ],
        };
        let states = wf.all_states();
        assert!(states.contains(&"APPLIED".to_string()));
        assert!(states.contains(&"UNDERWRITING".to_string()));
        assert!(states.contains(&"DOC_CHECK".to_string()));
        assert!(states.contains(&"RISK_SCORE".to_string()));
        assert!(states.contains(&"REVIEW".to_string()));
    }

    #[test]
    fn test_implied_transitions() {
        let wf = Workflow {
            name: "loan".into(),
            resource_type: "loan".into(),
            steps: vec![
                WorkflowStep::Sequence(vec![
                    "APPLIED".into(), "UNDERWRITING".into(), "APPROVED".into(),
                ]),
            ],
        };
        let transitions = wf.implied_transitions();
        assert_eq!(transitions, vec![
            ("APPLIED".to_string(), "UNDERWRITING".to_string()),
            ("UNDERWRITING".to_string(), "APPROVED".to_string()),
        ]);
    }

    #[test]
    fn test_branch_workflow() {
        let mut paths = HashMap::new();
        paths.insert("low".into(), vec![
            WorkflowStep::Sequence(vec!["APPROVED".into(), "DISBURSED".into()]),
        ]);
        paths.insert("high".into(), vec![
            WorkflowStep::Sequence(vec!["COMMITTEE".into(), "APPROVED".into()]),
        ]);

        let wf = Workflow {
            name: "loan".into(),
            resource_type: "loan".into(),
            steps: vec![
                WorkflowStep::Sequence(vec!["APPLIED".into(), "SCORED".into()]),
                WorkflowStep::Branch { on: "risk_level".into(), paths },
            ],
        };

        let states = wf.all_states();
        assert!(states.contains(&"COMMITTEE".to_string()));
        assert!(states.contains(&"DISBURSED".to_string()));
    }
}
