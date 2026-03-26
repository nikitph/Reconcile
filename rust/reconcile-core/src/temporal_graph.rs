//! Temporal Graph — causal event chains for audit traversal.
//!
//! Built from the event log. When controller C processes event A and
//! produces event B, a causal edge A→B via C is added. This enables
//! "why" queries: "Why was this loan disbursed?" → backward traversal.

use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Causal edge
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CausalEdge {
    pub cause_event_id: Uuid,
    pub effect_event_id: Uuid,
    pub via: String, // controller or agent name
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalDirection {
    Forward,  // "What resulted from this event?"
    Backward, // "Why did this event happen?"
}

// ---------------------------------------------------------------------------
// Temporal graph trait
// ---------------------------------------------------------------------------

pub trait TemporalGraph: Send + Sync {
    fn add_causal_edge(&mut self, edge: CausalEdge);

    /// Traverse the causal chain from an event in the given direction.
    /// Returns event IDs in traversal order.
    fn causal_chain(&self, event_id: &Uuid, direction: CausalDirection) -> Vec<Uuid>;

    /// Get all causal edges in the subgraph reachable from event_id.
    fn event_subgraph(&self, event_id: &Uuid, direction: CausalDirection) -> Vec<CausalEdge>;

    /// Filter causal chain by the "via" field (controller/agent name).
    fn causal_chain_via(&self, event_id: &Uuid, direction: CausalDirection, via: &str) -> Vec<Uuid>;
}

// ---------------------------------------------------------------------------
// In-memory implementation
// ---------------------------------------------------------------------------

pub struct InMemoryTemporalGraph {
    /// Forward edges: cause_id -> [effects]
    forward: HashMap<Uuid, Vec<CausalEdge>>,
    /// Backward edges: effect_id -> [causes]
    backward: HashMap<Uuid, Vec<CausalEdge>>,
}

impl InMemoryTemporalGraph {
    pub fn new() -> Self {
        Self {
            forward: HashMap::new(),
            backward: HashMap::new(),
        }
    }
}

impl Default for InMemoryTemporalGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl TemporalGraph for InMemoryTemporalGraph {
    fn add_causal_edge(&mut self, edge: CausalEdge) {
        self.forward.entry(edge.cause_event_id).or_default().push(edge.clone());
        self.backward.entry(edge.effect_event_id).or_default().push(edge);
    }

    fn causal_chain(&self, event_id: &Uuid, direction: CausalDirection) -> Vec<Uuid> {
        let adjacency = match direction {
            CausalDirection::Forward => &self.forward,
            CausalDirection::Backward => &self.backward,
        };

        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        visited.insert(*event_id);
        queue.push_back(*event_id);

        while let Some(current) = queue.pop_front() {
            if let Some(edges) = adjacency.get(&current) {
                for edge in edges {
                    let next = match direction {
                        CausalDirection::Forward => edge.effect_event_id,
                        CausalDirection::Backward => edge.cause_event_id,
                    };
                    if visited.insert(next) {
                        result.push(next);
                        queue.push_back(next);
                    }
                }
            }
        }
        result
    }

    fn event_subgraph(&self, event_id: &Uuid, direction: CausalDirection) -> Vec<CausalEdge> {
        let adjacency = match direction {
            CausalDirection::Forward => &self.forward,
            CausalDirection::Backward => &self.backward,
        };

        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        visited.insert(*event_id);
        queue.push_back(*event_id);

        while let Some(current) = queue.pop_front() {
            if let Some(edges) = adjacency.get(&current) {
                for edge in edges {
                    result.push(edge.clone());
                    let next = match direction {
                        CausalDirection::Forward => edge.effect_event_id,
                        CausalDirection::Backward => edge.cause_event_id,
                    };
                    if visited.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
        }
        result
    }

    fn causal_chain_via(&self, event_id: &Uuid, direction: CausalDirection, via: &str) -> Vec<Uuid> {
        let adjacency = match direction {
            CausalDirection::Forward => &self.forward,
            CausalDirection::Backward => &self.backward,
        };

        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        visited.insert(*event_id);
        queue.push_back(*event_id);

        while let Some(current) = queue.pop_front() {
            if let Some(edges) = adjacency.get(&current) {
                for edge in edges {
                    if edge.via == via {
                        let next = match direction {
                            CausalDirection::Forward => edge.effect_event_id,
                            CausalDirection::Backward => edge.cause_event_id,
                        };
                        if visited.insert(next) {
                            result.push(next);
                            queue.push_back(next);
                        }
                    }
                }
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn build_chain() -> (InMemoryTemporalGraph, Uuid, Uuid, Uuid, Uuid) {
        // Event chain: e1 --[ctrl_a]--> e2 --[ctrl_b]--> e3 --[ctrl_b]--> e4
        let mut tg = InMemoryTemporalGraph::new();
        let e1 = Uuid::new_v4();
        let e2 = Uuid::new_v4();
        let e3 = Uuid::new_v4();
        let e4 = Uuid::new_v4();

        tg.add_causal_edge(CausalEdge { cause_event_id: e1, effect_event_id: e2, via: "ctrl_a".into() });
        tg.add_causal_edge(CausalEdge { cause_event_id: e2, effect_event_id: e3, via: "ctrl_b".into() });
        tg.add_causal_edge(CausalEdge { cause_event_id: e3, effect_event_id: e4, via: "ctrl_b".into() });

        (tg, e1, e2, e3, e4)
    }

    #[test]
    fn test_forward_chain() {
        let (tg, e1, e2, e3, e4) = build_chain();
        let chain = tg.causal_chain(&e1, CausalDirection::Forward);
        assert_eq!(chain.len(), 3);
        assert!(chain.contains(&e2));
        assert!(chain.contains(&e3));
        assert!(chain.contains(&e4));
    }

    #[test]
    fn test_backward_chain() {
        let (tg, e1, e2, e3, e4) = build_chain();
        let chain = tg.causal_chain(&e4, CausalDirection::Backward);
        assert_eq!(chain.len(), 3);
        assert!(chain.contains(&e1));
        assert!(chain.contains(&e2));
        assert!(chain.contains(&e3));
    }

    #[test]
    fn test_partial_chain() {
        let (tg, _, e2, e3, e4) = build_chain();
        // Forward from e2
        let chain = tg.causal_chain(&e2, CausalDirection::Forward);
        assert_eq!(chain.len(), 2);
        assert!(chain.contains(&e3));
        assert!(chain.contains(&e4));
    }

    #[test]
    fn test_no_chain() {
        let (tg, _, _, _, e4) = build_chain();
        // Forward from e4 (leaf)
        let chain = tg.causal_chain(&e4, CausalDirection::Forward);
        assert!(chain.is_empty());
    }

    #[test]
    fn test_chain_via_filter() {
        let (tg, e1, _, e3, e4) = build_chain();
        // Only follow ctrl_b edges forward from e1
        // e1 --[ctrl_a]--> e2, so ctrl_b filter stops at e1
        let chain = tg.causal_chain_via(&e1, CausalDirection::Forward, "ctrl_b");
        assert!(chain.is_empty());

        // From e2 forward via ctrl_b
        let (tg2, _, e2, e3, e4) = build_chain();
        let chain = tg2.causal_chain_via(&e2, CausalDirection::Forward, "ctrl_b");
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn test_event_subgraph() {
        let (tg, e1, _, _, _) = build_chain();
        let edges = tg.event_subgraph(&e1, CausalDirection::Forward);
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn test_branching_causality() {
        let mut tg = InMemoryTemporalGraph::new();
        let root = Uuid::new_v4();
        let b1 = Uuid::new_v4();
        let b2 = Uuid::new_v4();
        let b3 = Uuid::new_v4();

        // root causes b1 and b2 (branching)
        tg.add_causal_edge(CausalEdge { cause_event_id: root, effect_event_id: b1, via: "c1".into() });
        tg.add_causal_edge(CausalEdge { cause_event_id: root, effect_event_id: b2, via: "c2".into() });
        // b1 causes b3
        tg.add_causal_edge(CausalEdge { cause_event_id: b1, effect_event_id: b3, via: "c1".into() });

        let chain = tg.causal_chain(&root, CausalDirection::Forward);
        assert_eq!(chain.len(), 3);

        // Backward from b3 should find root
        let chain = tg.causal_chain(&b3, CausalDirection::Backward);
        assert!(chain.contains(&root));
    }
}
