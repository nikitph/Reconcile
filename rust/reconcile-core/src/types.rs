use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Identity types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId(pub Uuid);

impl ResourceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for ResourceId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Authority levels
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityLevel {
    Human,
    Controller,
    Agent,
    System,
}

impl fmt::Display for AuthorityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Human => write!(f, "HUMAN"),
            Self::Controller => write!(f, "CONTROLLER"),
            Self::Agent => write!(f, "AGENT"),
            Self::System => write!(f, "SYSTEM"),
        }
    }
}

impl AuthorityLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "HUMAN" => Some(Self::Human),
            "CONTROLLER" => Some(Self::Controller),
            "AGENT" => Some(Self::Agent),
            "SYSTEM" => Some(Self::System),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: ResourceId,
    pub resource_type: String,
    pub state: String,
    pub desired_state: Option<String>,
    pub data: serde_json::Value,
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub offset: u64,
    pub event_type: String,
    pub resource_id: ResourceId,
    pub payload: serde_json::Value,
    pub actor: String,
    pub authority_level: AuthorityLevel,
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Transition context (threaded through the 8-step boundary)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TransitionContext {
    pub resource_id: ResourceId,
    pub resource_type: String,
    pub from_state: String,
    pub to_state: String,
    pub actor: String,
    pub role: String,
    pub authority_level: AuthorityLevel,
}

// ---------------------------------------------------------------------------
// Policy result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyResult {
    pub passed: bool,
    pub message: String,
    pub details: serde_json::Value,
}

impl PolicyResult {
    pub fn allow() -> Self {
        Self {
            passed: true,
            message: String::new(),
            details: serde_json::Value::Null,
        }
    }

    pub fn deny(message: impl Into<String>) -> Self {
        Self {
            passed: false,
            message: message.into(),
            details: serde_json::Value::Null,
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantResult {
    pub holds: bool,
    pub violation: Option<String>,
    pub details: serde_json::Value,
}

impl InvariantResult {
    pub fn ok() -> Self {
        Self {
            holds: true,
            violation: None,
            details: serde_json::Value::Null,
        }
    }

    pub fn violated(message: impl Into<String>) -> Self {
        Self {
            holds: false,
            violation: Some(message.into()),
            details: serde_json::Value::Null,
        }
    }
}

// ---------------------------------------------------------------------------
// Transition outcome (returned from the 8-step boundary)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum TransitionOutcome {
    Success {
        resource: Resource,
        events: Vec<Event>,
    },
    Rejected {
        step: String,
        reason: String,
        details: serde_json::Value,
    },
}

// ---------------------------------------------------------------------------
// Audit record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEvaluation {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantEvaluation {
    pub name: String,
    pub holds: bool,
    pub violation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: Uuid,
    pub resource_type: String,
    pub resource_id: ResourceId,
    pub actor: String,
    pub role: String,
    pub authority_level: AuthorityLevel,
    pub previous_state: String,
    pub new_state: String,
    pub policies_evaluated: Vec<PolicyEvaluation>,
    pub invariants_checked: Vec<InvariantEvaluation>,
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: Uuid,
    pub resource_id: ResourceId,
    pub state: String,
    pub data: serde_json::Value,
    pub version: u64,
    pub event_offset: u64,
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Controller action (what a controller wants to do after reconcile)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ControllerAction {
    NoOp,
    Transition { to_state: String },
    SetDesiredState { state: String },
}
