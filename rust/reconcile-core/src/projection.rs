//! Interface Projection Engine — the core product feature.
//!
//! Computes what a given role can see, do, and not do for a resource
//! in its current state. Pure read-only function over all kernel subsystems.
//! Every consumer — React, LLM, API, agent — calls this one function.

use crate::errors::KernelError;
use crate::types::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// InterfaceProjection — the output type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceProjection {
    pub resource: ProjectedResource,
    pub valid_actions: Vec<ValidAction>,
    pub blocked_actions: Vec<BlockedAction>,
    pub warnings: Vec<Warning>,
    pub proposals: Vec<ProjectedProposal>,
    pub audit_summary: Vec<AuditEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedResource {
    pub id: String,
    pub resource_type: String,
    pub state: String,
    pub desired_state: Option<String>,
    pub data: serde_json::Value,
    pub version: u64,
    pub is_terminal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidAction {
    pub action: String,
    pub action_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedAction {
    pub action: String,
    pub reason: String,
    pub blocked_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warning {
    pub message: String,
    pub source: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedProposal {
    pub agent: String,
    pub action: String,
    pub confidence: f64,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub actor: String,
    pub from_state: String,
    pub to_state: String,
    pub authority_level: String,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Projection computation — implemented on Kernel in transaction.rs
// ---------------------------------------------------------------------------

/// Compute the projection for a resource viewed by a role.
/// This is a pure read-only function. It queries every subsystem but mutates nothing.
pub fn compute_projection(
    resource: &Resource,
    role: &str,
    state_machine: &crate::state_machine::StateMachine,
    role_registry: &crate::roles::RoleRegistry,
    policy_engine: &crate::policy_engine::PolicyEngine,
    invariant_checker: &crate::invariant_checker::InvariantChecker,
    query: &dyn crate::invariant_checker::SystemQuery,
    proposals: &[Proposal],
    audit_records: &[AuditRecord],
) -> InterfaceProjection {
    let resource_type = &resource.resource_type;
    let current_state = &resource.state;
    let is_terminal = state_machine.is_terminal(current_state);

    // Step 2: Get all valid transitions from state machine
    let all_transitions = state_machine.get_valid_transitions(current_state);

    let mut valid_actions = Vec::new();
    let mut blocked_actions = Vec::new();
    let mut warnings = Vec::new();

    for transition in &all_transitions {
        let to_state = &transition.to_state;

        // Step 3: Check role permission
        let has_permission = role_registry.check_permission(
            role, "transition", resource_type, to_state,
        );

        if !has_permission {
            blocked_actions.push(BlockedAction {
                action: to_state.clone(),
                reason: format!("Role '{}' does not have permission to transition to '{}'", role, to_state),
                blocked_by: "role_permission".into(),
            });
            continue;
        }

        // Step 4: Evaluate policies for this specific transition
        let context = TransitionContext {
            resource_id: resource.id.clone(),
            resource_type: resource_type.clone(),
            from_state: current_state.clone(),
            to_state: to_state.clone(),
            actor: String::new(), // Projection doesn't have actor context
            role: role.to_string(),
            authority_level: AuthorityLevel::Human,
        };

        let policy_result = policy_engine.first_denied(resource, &context, query);

        match policy_result {
            Some(denied) => {
                blocked_actions.push(BlockedAction {
                    action: to_state.clone(),
                    reason: format!("Policy '{}': {}", denied.name, denied.message),
                    blocked_by: "policy".into(),
                });
                // Surface as warning too — the user should know why
                warnings.push(Warning {
                    message: format!("{}: {}", denied.name, denied.message),
                    source: denied.name,
                    severity: "warning".into(),
                });
            }
            None => {
                valid_actions.push(ValidAction {
                    action: to_state.clone(),
                    action_type: "transition".into(),
                });
            }
        }
    }

    // Check strong invariants on current state — surface violations as warnings
    let invariant_evals = invariant_checker.check_strong(resource, query);
    for eval in &invariant_evals {
        if !eval.holds {
            warnings.push(Warning {
                message: format!("Invariant '{}' violated: {}",
                    eval.name,
                    eval.violation.as_deref().unwrap_or("unknown")),
                source: eval.name.clone(),
                severity: "critical".into(),
            });
        }
    }

    // If resource has desired_state != current state, surface as info
    if let Some(ref desired) = resource.desired_state {
        if desired != current_state {
            warnings.push(Warning {
                message: format!("Desired state is '{}', currently '{}'", desired, current_state),
                source: "desired_state".into(),
                severity: "info".into(),
            });
        }
    }

    // Step 5: Fetch agent proposals
    let projected_proposals: Vec<ProjectedProposal> = proposals
        .iter()
        .filter(|p| p.resource_id == resource.id)
        .map(|p| {
            let action_str = match &p.action {
                ProposedAction::Transition { to_state } => format!("transition:{}", to_state),
                ProposedAction::SetDesiredState { state } => format!("set_desired:{}", state),
                ProposedAction::Flag { reason } => format!("flag:{}", reason),
            };
            ProjectedProposal {
                agent: p.agent.clone(),
                action: action_str,
                confidence: p.confidence,
                reasoning: p.reasoning.clone(),
            }
        })
        .collect();

    // Step 6: Build audit summary (last 10 entries)
    let audit_summary: Vec<AuditEntry> = audit_records
        .iter()
        .rev()
        .take(10)
        .map(|a| AuditEntry {
            actor: a.actor.clone(),
            from_state: a.previous_state.clone(),
            to_state: a.new_state.clone(),
            authority_level: a.authority_level.to_string(),
            timestamp: a.timestamp.to_rfc3339(),
        })
        .collect();

    // Step 7: Assemble — filter data fields by role visibility
    let visible_data = role_registry.filter_visible_fields(role, &resource.data);

    InterfaceProjection {
        resource: ProjectedResource {
            id: resource.id.to_string(),
            resource_type: resource_type.clone(),
            state: current_state.clone(),
            desired_state: resource.desired_state.clone(),
            data: visible_data,
            version: resource.version,
            is_terminal,
        },
        valid_actions,
        blocked_actions,
        warnings,
        proposals: projected_proposals,
        audit_summary,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invariant_checker::SystemQuery;
    use crate::policy_engine::{PolicyDefinition, PolicyEvaluator, PolicyEngine};
    use crate::invariant_checker::{InvariantChecker, InvariantDefinition, InvariantMode, InvariantScope, InvariantCheck};
    use crate::roles::{RoleRegistry, RoleDefinition, Permission};
    use crate::state_machine::*;
    use chrono::Utc;
    use uuid::Uuid;

    struct EmptyQuery;
    impl SystemQuery for EmptyQuery {
        fn get_resource(&self, _id: &ResourceId) -> Option<Resource> { None }
        fn list_by_type(&self, _t: &str) -> Vec<Resource> { vec![] }
    }

    fn loan_state_machine() -> StateMachine {
        StateMachine::new(
            vec![
                StateDefinition { name: "APPLIED".into(), status: StateStatus::Active },
                StateDefinition { name: "UNDERWRITING".into(), status: StateStatus::Active },
                StateDefinition { name: "APPROVED".into(), status: StateStatus::Active },
                StateDefinition { name: "DISBURSED".into(), status: StateStatus::Terminal },
                StateDefinition { name: "REJECTED".into(), status: StateStatus::Terminal },
            ],
            vec![
                TransitionDefinition { from_state: "APPLIED".into(), to_state: "UNDERWRITING".into(), guard: None, required_role: None },
                TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "APPROVED".into(), guard: None, required_role: None },
                TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "REJECTED".into(), guard: None, required_role: None },
                TransitionDefinition { from_state: "APPROVED".into(), to_state: "DISBURSED".into(), guard: None, required_role: None },
            ],
            "APPLIED".into(),
        ).unwrap()
    }

    fn loan_roles() -> RoleRegistry {
        let mut reg = RoleRegistry::new();
        reg.register(RoleDefinition {
            name: "clerk".into(),
            visible_fields: vec![], permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:UNDERWRITING"),
            ],
        });
        reg.register(RoleDefinition {
            name: "officer".into(),
            visible_fields: vec![], permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:APPROVED"),
                Permission::from_shorthand("transition:REJECTED"),
            ],
        });
        reg.register(RoleDefinition {
            name: "manager".into(),
            visible_fields: vec![], permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:*"),
            ],
        });
        reg.register(RoleDefinition {
            name: "viewer".into(),
            visible_fields: vec![], permissions: vec![Permission::from_shorthand("view")],
        });
        reg
    }

    fn make_resource(state: &str, amount: i64) -> Resource {
        Resource {
            id: ResourceId::new(),
            resource_type: "loan".into(),
            state: state.into(),
            desired_state: None,
            data: serde_json::json!({"amount": amount, "applicant": "Acme Corp"}),
            version: 3,
            tenant_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_clerk_sees_underwriting_only() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let resource = make_resource("APPLIED", 100_000);

        let projection = compute_projection(
            &resource, "clerk", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &[],
        );

        assert_eq!(projection.valid_actions.len(), 1);
        assert_eq!(projection.valid_actions[0].action, "UNDERWRITING");
        // APPLIED only has one outbound transition (UNDERWRITING), and clerk can do it
        // So no blocked_actions in this state for clerk
    }

    #[test]
    fn test_officer_in_underwriting() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let resource = make_resource("UNDERWRITING", 500_000);

        let projection = compute_projection(
            &resource, "officer", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &[],
        );

        let action_names: Vec<&str> = projection.valid_actions.iter().map(|a| a.action.as_str()).collect();
        assert!(action_names.contains(&"APPROVED"));
        assert!(action_names.contains(&"REJECTED"));
        assert_eq!(projection.valid_actions.len(), 2);
    }

    #[test]
    fn test_manager_sees_all_transitions() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let resource = make_resource("UNDERWRITING", 100_000);

        let projection = compute_projection(
            &resource, "manager", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &[],
        );

        assert_eq!(projection.valid_actions.len(), 2); // APPROVED + REJECTED
        assert!(projection.blocked_actions.is_empty());
    }

    #[test]
    fn test_viewer_sees_no_actions() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let resource = make_resource("APPLIED", 100_000);

        let projection = compute_projection(
            &resource, "viewer", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &[],
        );

        assert!(projection.valid_actions.is_empty());
        assert!(!projection.blocked_actions.is_empty());
        assert!(projection.blocked_actions.iter().all(|b| b.blocked_by == "role_permission"));
    }

    #[test]
    fn test_terminal_state_no_actions() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let resource = make_resource("DISBURSED", 100_000);

        let projection = compute_projection(
            &resource, "manager", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &[],
        );

        assert!(projection.valid_actions.is_empty());
        assert!(projection.blocked_actions.is_empty());
        assert!(projection.resource.is_terminal);
    }

    #[test]
    fn test_policy_blocks_action() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let mut policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();

        // Policy: block APPROVED for loans > 500K
        struct HighValueBlock;
        impl PolicyEvaluator for HighValueBlock {
            fn evaluate(&self, r: &Resource, ctx: &TransitionContext, _q: &dyn SystemQuery) -> PolicyResult {
                if ctx.to_state == "APPROVED" {
                    let amount = r.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    if amount > 500_000.0 {
                        return PolicyResult::deny("Loans > 500K need committee approval");
                    }
                }
                PolicyResult::allow()
            }
        }
        policies.register(PolicyDefinition {
            name: "high_value".into(),
            description: "Block high value".into(),
            evaluator: Box::new(HighValueBlock),
            applicable_states: vec!["UNDERWRITING".into()],
            resource_types: vec!["loan".into()],
            priority: 80,
        });

        let resource = make_resource("UNDERWRITING", 1_000_000);
        let projection = compute_projection(
            &resource, "manager", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &[],
        );

        // APPROVED should be blocked by policy, REJECTED should be valid
        let valid: Vec<&str> = projection.valid_actions.iter().map(|a| a.action.as_str()).collect();
        let blocked: Vec<&str> = projection.blocked_actions.iter().map(|a| a.action.as_str()).collect();

        assert!(valid.contains(&"REJECTED"));
        assert!(!valid.contains(&"APPROVED"));
        assert!(blocked.contains(&"APPROVED"));

        // Should have a warning about the policy
        assert!(projection.warnings.iter().any(|w| w.source == "high_value"));
    }

    #[test]
    fn test_proposals_in_projection() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let resource = make_resource("UNDERWRITING", 100_000);

        let proposals = vec![
            Proposal {
                id: Uuid::new_v4(),
                agent: "risk".into(),
                action: ProposedAction::Transition { to_state: "APPROVED".into() },
                resource_id: resource.id.clone(),
                confidence: 0.87,
                reasoning: "Low risk score".into(),
                timestamp: Utc::now(),
            },
        ];

        let projection = compute_projection(
            &resource, "officer", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &proposals, &[],
        );

        assert_eq!(projection.proposals.len(), 1);
        assert_eq!(projection.proposals[0].agent, "risk");
        assert_eq!(projection.proposals[0].confidence, 0.87);
        assert!(projection.proposals[0].action.contains("APPROVED"));
    }

    #[test]
    fn test_audit_summary() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let resource = make_resource("UNDERWRITING", 100_000);

        let audit = vec![
            AuditRecord {
                id: Uuid::new_v4(),
                resource_type: "loan".into(),
                resource_id: resource.id.clone(),
                actor: "clerk1".into(),
                role: "clerk".into(),
                authority_level: AuthorityLevel::Human,
                previous_state: "APPLIED".into(),
                new_state: "UNDERWRITING".into(),
                policies_evaluated: vec![],
                invariants_checked: vec![],
                timestamp: Utc::now(),
            },
        ];

        let projection = compute_projection(
            &resource, "officer", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &audit,
        );

        assert_eq!(projection.audit_summary.len(), 1);
        assert_eq!(projection.audit_summary[0].actor, "clerk1");
        assert_eq!(projection.audit_summary[0].to_state, "UNDERWRITING");
    }

    #[test]
    fn test_desired_state_warning() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let mut resource = make_resource("APPLIED", 100_000);
        resource.desired_state = Some("DISBURSED".into());

        let projection = compute_projection(
            &resource, "manager", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &[],
        );

        assert!(projection.warnings.iter().any(|w|
            w.source == "desired_state" && w.message.contains("DISBURSED")
        ));
    }

    #[test]
    fn test_resource_data_preserved() {
        let sm = loan_state_machine();
        let roles = loan_roles();
        let policies = PolicyEngine::new();
        let invariants = InvariantChecker::new();
        let resource = make_resource("APPLIED", 500_000);

        let projection = compute_projection(
            &resource, "manager", &sm, &roles, &policies, &invariants,
            &EmptyQuery, &[], &[],
        );

        assert_eq!(projection.resource.data["amount"], 500_000);
        assert_eq!(projection.resource.data["applicant"], "Acme Corp");
        assert_eq!(projection.resource.version, 3);
    }
}
