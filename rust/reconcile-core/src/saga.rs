//! Saga framework — cross-resource compensating transactions.
//!
//! A saga is a sequence of steps across resources. If step N fails,
//! compensating actions for steps N-1..0 are executed to undo changes.
//! Each step is a separate transition through the full kernel (with
//! policies, invariants, etc.).

use crate::types::*;

// ---------------------------------------------------------------------------
// Saga definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SagaStep {
    pub name: String,
    pub resource_id: ResourceId,
    pub action: SagaAction,
    pub compensate: SagaAction,
}

#[derive(Debug, Clone)]
pub enum SagaAction {
    Transition { to_state: String },
    SetDesiredState { state: String },
    NoOp,
}

#[derive(Debug, Clone)]
pub struct Saga {
    pub name: String,
    pub steps: Vec<SagaStep>,
}

#[derive(Debug, Clone)]
pub enum SagaOutcome {
    /// All steps completed successfully.
    Completed {
        steps_executed: usize,
    },
    /// A step failed; compensating actions were run.
    Compensated {
        failed_step: usize,
        failed_reason: String,
        compensated_steps: usize,
    },
    /// Compensation itself failed (manual intervention needed).
    CompensationFailed {
        failed_step: usize,
        compensation_error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_saga_definition() {
        let saga = Saga {
            name: "disburse_loan".into(),
            steps: vec![
                SagaStep {
                    name: "approve_loan".into(),
                    resource_id: ResourceId::new(),
                    action: SagaAction::Transition { to_state: "APPROVED".into() },
                    compensate: SagaAction::Transition { to_state: "UNDERWRITING".into() },
                },
                SagaStep {
                    name: "release_collateral".into(),
                    resource_id: ResourceId::new(),
                    action: SagaAction::Transition { to_state: "RELEASED".into() },
                    compensate: SagaAction::Transition { to_state: "HELD".into() },
                },
            ],
        };
        assert_eq!(saga.steps.len(), 2);
        assert_eq!(saga.name, "disburse_loan");
    }
}
