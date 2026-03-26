use crate::types::ResourceId;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("Invalid transition: {from} -> {to} is not defined")]
    InvalidTransition { from: String, to: String },

    #[error("Permission denied: role '{role}' cannot {action} on {resource_type} in state {state}")]
    PermissionDenied {
        role: String,
        action: String,
        resource_type: String,
        state: String,
    },

    #[error("Guard failed for transition {from} -> {to}: {reason}")]
    GuardFailed {
        from: String,
        to: String,
        reason: String,
    },

    #[error("Policy denied: {policy_name}: {message}")]
    PolicyDenied {
        policy_name: String,
        message: String,
    },

    #[error("Invariant violated: {invariant_name}: {violation}")]
    InvariantViolated {
        invariant_name: String,
        violation: String,
    },

    #[error("Resource not found: {0}")]
    ResourceNotFound(ResourceId),

    #[error("Resource type not registered: {0}")]
    TypeNotRegistered(String),

    #[error("Resource type already registered: {0}")]
    TypeAlreadyRegistered(String),

    #[error("Version conflict on {resource_id}: expected {expected}, found {found}")]
    VersionConflict {
        resource_id: ResourceId,
        expected: u64,
        found: u64,
    },

    #[error("Cascade depth exceeded: {depth} > {max}")]
    CascadeDepthExceeded { depth: u32, max: u32 },

    #[error("Convergence failure: resource {resource_id} not closer to desired state after cascade step")]
    ConvergenceFailure { resource_id: ResourceId },

    #[error("Terminal state: {state} has no outbound transitions")]
    TerminalState { state: String },

    #[error("State not defined: {0}")]
    StateNotDefined(String),

    #[error("Role not defined: {0}")]
    RoleNotDefined(String),

    #[error("No initial state defined")]
    NoInitialState,

    #[error("Callback error: {0}")]
    CallbackError(String),
}
