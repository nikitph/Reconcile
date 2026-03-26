//! Decision Nodes — aggregate agent proposals and decide actions.
//!
//! Multiple agents observe a resource and emit proposals with confidence scores.
//! Decision nodes aggregate these proposals using configurable strategies and
//! decide whether to auto-accept, require human review, or reject.

use crate::types::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Aggregation strategies
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregationStrategy {
    /// Accept if majority of agents propose the same action
    Majority,
    /// Weighted average of confidence scores
    WeightedAvg,
    /// All agents must agree (lowest confidence wins)
    Unanimous,
    /// Use the minimum confidence across all proposals
    MinConfidence,
}

// ---------------------------------------------------------------------------
// Decision outcome
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum DecisionOutcome {
    /// Aggregated confidence exceeds auto_accept_threshold
    AutoAccept {
        action: ProposedAction,
        confidence: f64,
    },
    /// Confidence is between reject and accept — needs human review
    NeedsReview {
        proposals: Vec<Proposal>,
        confidence: f64,
    },
    /// Aggregated confidence below auto_reject_threshold
    AutoReject {
        confidence: f64,
        reason: String,
    },
    /// No proposals received from any agent
    NoProposals,
}

// ---------------------------------------------------------------------------
// Decision node
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DecisionNode {
    pub name: String,
    pub agent_names: Vec<String>,
    pub aggregation: AggregationStrategy,
    pub auto_accept_threshold: f64,
    pub auto_reject_threshold: f64,
}

impl DecisionNode {
    pub fn new(name: impl Into<String>, agents: Vec<String>) -> Self {
        Self {
            name: name.into(),
            agent_names: agents,
            aggregation: AggregationStrategy::WeightedAvg,
            auto_accept_threshold: 0.9,
            auto_reject_threshold: 0.5,
        }
    }

    /// Evaluate proposals from matching agents and produce a decision.
    pub fn evaluate(&self, proposals: &[Proposal]) -> DecisionOutcome {
        // Filter to proposals from this node's agents
        let relevant: Vec<&Proposal> = proposals
            .iter()
            .filter(|p| self.agent_names.contains(&p.agent))
            .collect();

        if relevant.is_empty() {
            return DecisionOutcome::NoProposals;
        }

        let confidence = self.aggregate_confidence(&relevant);

        if confidence >= self.auto_accept_threshold {
            // Pick the action from the highest-confidence proposal
            let best = relevant
                .iter()
                .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
                .unwrap();
            DecisionOutcome::AutoAccept {
                action: best.action.clone(),
                confidence,
            }
        } else if confidence < self.auto_reject_threshold {
            DecisionOutcome::AutoReject {
                confidence,
                reason: format!(
                    "Aggregated confidence {:.2} below reject threshold {:.2}",
                    confidence, self.auto_reject_threshold
                ),
            }
        } else {
            DecisionOutcome::NeedsReview {
                proposals: relevant.into_iter().cloned().collect(),
                confidence,
            }
        }
    }

    fn aggregate_confidence(&self, proposals: &[&Proposal]) -> f64 {
        match self.aggregation {
            AggregationStrategy::WeightedAvg => {
                let sum: f64 = proposals.iter().map(|p| p.confidence).sum();
                sum / proposals.len() as f64
            }
            AggregationStrategy::MinConfidence => {
                proposals.iter().map(|p| p.confidence).fold(f64::INFINITY, f64::min)
            }
            AggregationStrategy::Unanimous => {
                // All must be above threshold; use minimum as aggregate
                proposals.iter().map(|p| p.confidence).fold(f64::INFINITY, f64::min)
            }
            AggregationStrategy::Majority => {
                // Count proposals that agree on the same action type
                // Use average confidence of the majority group
                let total = proposals.len() as f64;
                let approve_count = proposals.iter()
                    .filter(|p| matches!(p.action, ProposedAction::Transition { .. } | ProposedAction::SetDesiredState { .. }))
                    .count() as f64;
                let approve_conf: f64 = proposals.iter()
                    .filter(|p| matches!(p.action, ProposedAction::Transition { .. } | ProposedAction::SetDesiredState { .. }))
                    .map(|p| p.confidence)
                    .sum::<f64>();

                if approve_count > total / 2.0 {
                    approve_conf / approve_count
                } else {
                    // Majority rejects/flags
                    let reject_conf: f64 = proposals.iter()
                        .filter(|p| matches!(p.action, ProposedAction::Flag { .. }))
                        .map(|p| 1.0 - p.confidence) // Invert: low confidence in approval = high confidence in rejection
                        .sum::<f64>();
                    reject_conf / (total - approve_count).max(1.0)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_proposal(agent: &str, confidence: f64, approve: bool) -> Proposal {
        Proposal {
            id: Uuid::new_v4(),
            agent: agent.into(),
            action: if approve {
                ProposedAction::Transition { to_state: "APPROVED".into() }
            } else {
                ProposedAction::Flag { reason: "Risky".into() }
            },
            resource_id: ResourceId::new(),
            confidence,
            reasoning: format!("{} says {}", agent, if approve { "yes" } else { "no" }),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_auto_accept_high_confidence() {
        let node = DecisionNode::new("committee", vec!["risk".into(), "fraud".into()]);
        let proposals = vec![
            make_proposal("risk", 0.95, true),
            make_proposal("fraud", 0.92, true),
        ];
        match node.evaluate(&proposals) {
            DecisionOutcome::AutoAccept { confidence, .. } => {
                assert!(confidence > 0.9);
            }
            other => panic!("Expected AutoAccept, got {:?}", other),
        }
    }

    #[test]
    fn test_auto_reject_low_confidence() {
        let node = DecisionNode::new("committee", vec!["risk".into(), "fraud".into()]);
        let proposals = vec![
            make_proposal("risk", 0.2, false),
            make_proposal("fraud", 0.3, false),
        ];
        match node.evaluate(&proposals) {
            DecisionOutcome::AutoReject { confidence, .. } => {
                assert!(confidence < 0.5);
            }
            other => panic!("Expected AutoReject, got {:?}", other),
        }
    }

    #[test]
    fn test_needs_review_middle_confidence() {
        let node = DecisionNode::new("committee", vec!["risk".into(), "fraud".into()]);
        let proposals = vec![
            make_proposal("risk", 0.7, true),
            make_proposal("fraud", 0.8, true),
        ];
        match node.evaluate(&proposals) {
            DecisionOutcome::NeedsReview { confidence, proposals } => {
                assert!(confidence >= 0.5 && confidence < 0.9);
                assert_eq!(proposals.len(), 2);
            }
            other => panic!("Expected NeedsReview, got {:?}", other),
        }
    }

    #[test]
    fn test_no_proposals() {
        let node = DecisionNode::new("committee", vec!["risk".into()]);
        let proposals: Vec<Proposal> = vec![];
        assert!(matches!(node.evaluate(&proposals), DecisionOutcome::NoProposals));
    }

    #[test]
    fn test_filters_by_agent_name() {
        let node = DecisionNode::new("committee", vec!["risk".into()]);
        let proposals = vec![
            make_proposal("risk", 0.95, true),
            make_proposal("unrelated", 0.1, false), // Not in this node
        ];
        match node.evaluate(&proposals) {
            DecisionOutcome::AutoAccept { confidence, .. } => {
                assert!(confidence > 0.9);
            }
            other => panic!("Expected AutoAccept, got {:?}", other),
        }
    }

    #[test]
    fn test_min_confidence_strategy() {
        let mut node = DecisionNode::new("committee", vec!["a".into(), "b".into()]);
        node.aggregation = AggregationStrategy::MinConfidence;
        let proposals = vec![
            make_proposal("a", 0.95, true),
            make_proposal("b", 0.6, true),
        ];
        match node.evaluate(&proposals) {
            DecisionOutcome::NeedsReview { confidence, .. } => {
                assert!((confidence - 0.6).abs() < 0.01); // Min = 0.6
            }
            other => panic!("Expected NeedsReview (min=0.6), got {:?}", other),
        }
    }

    #[test]
    fn test_majority_strategy() {
        let mut node = DecisionNode::new("committee", vec!["a".into(), "b".into(), "c".into()]);
        node.aggregation = AggregationStrategy::Majority;
        // 2 out of 3 approve with high confidence
        let proposals = vec![
            make_proposal("a", 0.95, true),
            make_proposal("b", 0.92, true),
            make_proposal("c", 0.3, false),
        ];
        match node.evaluate(&proposals) {
            DecisionOutcome::AutoAccept { confidence, .. } => {
                assert!(confidence > 0.9); // Average of 0.95 and 0.92
            }
            other => panic!("Expected AutoAccept, got {:?}", other),
        }
    }
}
