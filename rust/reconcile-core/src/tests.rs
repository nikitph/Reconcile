//! Comprehensive integration tests for the Reconcile kernel.
//! Tests real-world scenarios across all subsystems working together.

#[cfg(test)]
mod loan_origination {
    //! Full banking loan origination workflow with multi-level approvals,
    //! policies, invariants, and controllers.

    use crate::invariant_checker::SystemQuery;
    use crate::policy_engine::{PolicyDefinition, PolicyEvaluator};
    use crate::resource_registry::ResourceTypeDefinition;
    use crate::roles::{Permission, RoleDefinition};
    use crate::state_machine::*;
    use crate::transaction::Kernel;
    use crate::types::*;

    fn build_loan_kernel() -> Kernel {
        let mut kernel = Kernel::new();

        // 7-state loan lifecycle
        let states = vec![
            StateDefinition { name: "APPLIED".into(), status: StateStatus::Active },
            StateDefinition { name: "DOCUMENT_CHECK".into(), status: StateStatus::Active },
            StateDefinition { name: "UNDERWRITING".into(), status: StateStatus::Active },
            StateDefinition { name: "SENIOR_REVIEW".into(), status: StateStatus::Active },
            StateDefinition { name: "APPROVED".into(), status: StateStatus::Active },
            StateDefinition { name: "DISBURSED".into(), status: StateStatus::Terminal },
            StateDefinition { name: "REJECTED".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "APPLIED".into(), to_state: "DOCUMENT_CHECK".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "DOCUMENT_CHECK".into(), to_state: "UNDERWRITING".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "DOCUMENT_CHECK".into(), to_state: "REJECTED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "APPROVED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "SENIOR_REVIEW".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "REJECTED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "SENIOR_REVIEW".into(), to_state: "APPROVED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "SENIOR_REVIEW".into(), to_state: "REJECTED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "APPROVED".into(), to_state: "DISBURSED".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "APPLIED".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "loan".into(),
            schema: serde_json::json!({}),
            state_machine: sm,
        }).unwrap();

        // Roles: clerk, officer, senior_officer, manager
        kernel.role_registry.register(RoleDefinition {
            name: "clerk".into(),
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:DOCUMENT_CHECK"),
            ],
        });
        kernel.role_registry.register(RoleDefinition {
            name: "officer".into(),
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:UNDERWRITING"),
                Permission::from_shorthand("transition:REJECTED"),
            ],
        });
        kernel.role_registry.register(RoleDefinition {
            name: "senior_officer".into(),
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:APPROVED"),
                Permission::from_shorthand("transition:SENIOR_REVIEW"),
                Permission::from_shorthand("transition:REJECTED"),
            ],
        });
        kernel.role_registry.register(RoleDefinition {
            name: "manager".into(),
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:*"),
            ],
        });

        kernel
    }

    #[test]
    fn test_full_loan_happy_path() {
        let mut kernel = build_loan_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({"amount": 500_000, "applicant": "Acme Corp"}), "system", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Clerk moves to document check
        assert!(matches!(
            kernel.transition(&resource.id, "DOCUMENT_CHECK", "clerk1", "clerk", AuthorityLevel::Human).unwrap(),
            TransitionOutcome::Success { .. }
        ));

        // Officer moves to underwriting
        assert!(matches!(
            kernel.transition(&resource.id, "UNDERWRITING", "officer1", "officer", AuthorityLevel::Human).unwrap(),
            TransitionOutcome::Success { .. }
        ));

        // Senior officer approves
        assert!(matches!(
            kernel.transition(&resource.id, "APPROVED", "sr_officer", "senior_officer", AuthorityLevel::Human).unwrap(),
            TransitionOutcome::Success { .. }
        ));

        // Manager disburses
        assert!(matches!(
            kernel.transition(&resource.id, "DISBURSED", "mgr1", "manager", AuthorityLevel::Human).unwrap(),
            TransitionOutcome::Success { .. }
        ));

        let final_r = kernel.get_resource(&resource.id).unwrap();
        assert_eq!(final_r.state, "DISBURSED");
        assert_eq!(final_r.version, 5);

        // Verify complete audit trail
        let audit = kernel.get_audit(&resource.id);
        assert_eq!(audit.len(), 4);
        assert_eq!(audit[0].actor, "clerk1");
        assert_eq!(audit[0].role, "clerk");
        assert_eq!(audit[1].actor, "officer1");
        assert_eq!(audit[2].actor, "sr_officer");
        assert_eq!(audit[3].actor, "mgr1");
    }

    #[test]
    fn test_high_value_loan_requires_senior_review() {
        let mut kernel = build_loan_kernel();

        // Policy: loans > 1M need senior review
        struct SeniorReviewPolicy;
        impl PolicyEvaluator for SeniorReviewPolicy {
            fn evaluate(&self, r: &Resource, ctx: &TransitionContext, _q: &dyn SystemQuery) -> PolicyResult {
                let amount = r.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                // Block direct UNDERWRITING -> APPROVED for high-value loans
                if ctx.from_state == "UNDERWRITING" && ctx.to_state == "APPROVED" && amount > 1_000_000.0 {
                    return PolicyResult::deny("Loans > 1M require senior review");
                }
                PolicyResult::allow()
            }
        }

        kernel.policy_engine.register(PolicyDefinition {
            name: "senior_review_required".into(),
            description: "High-value loans need senior review".into(),
            evaluator: Box::new(SeniorReviewPolicy),
            applicable_states: vec!["UNDERWRITING".into()],
            resource_types: vec!["loan".into()],
            priority: 90,
        });

        // High-value loan
        let resource = match kernel.create_resource("loan", serde_json::json!({"amount": 5_000_000}), "system", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        kernel.transition(&resource.id, "DOCUMENT_CHECK", "c1", "clerk", AuthorityLevel::Human).unwrap();
        kernel.transition(&resource.id, "UNDERWRITING", "o1", "officer", AuthorityLevel::Human).unwrap();

        // Direct approval should be blocked
        let result = kernel.transition(&resource.id, "APPROVED", "sr1", "senior_officer", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "evaluate_policies"));

        // Must go through senior review first
        assert!(matches!(
            kernel.transition(&resource.id, "SENIOR_REVIEW", "sr1", "senior_officer", AuthorityLevel::Human).unwrap(),
            TransitionOutcome::Success { .. }
        ));

        // Now can approve from senior review
        assert!(matches!(
            kernel.transition(&resource.id, "APPROVED", "sr1", "senior_officer", AuthorityLevel::Human).unwrap(),
            TransitionOutcome::Success { .. }
        ));
    }

    #[test]
    fn test_clerk_cannot_approve() {
        let mut kernel = build_loan_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({}), "system", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        kernel.transition(&resource.id, "DOCUMENT_CHECK", "c1", "clerk", AuthorityLevel::Human).unwrap();

        // Clerk cannot move to underwriting
        let result = kernel.transition(&resource.id, "UNDERWRITING", "c1", "clerk", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "check_role_permissions"));
    }

    #[test]
    fn test_rejection_at_any_review_stage() {
        let mut kernel = build_loan_kernel();

        // Test rejection at document check
        let r1 = match kernel.create_resource("loan", serde_json::json!({"applicant": "Bad Docs"}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };
        kernel.transition(&r1.id, "DOCUMENT_CHECK", "c1", "clerk", AuthorityLevel::Human).unwrap();
        let result = kernel.transition(&r1.id, "REJECTED", "o1", "officer", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Success { .. }));
        assert_eq!(kernel.get_resource(&r1.id).unwrap().state, "REJECTED");

        // Test rejection at senior review
        let r2 = match kernel.create_resource("loan", serde_json::json!({"applicant": "High Risk"}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };
        kernel.transition(&r2.id, "DOCUMENT_CHECK", "c1", "clerk", AuthorityLevel::Human).unwrap();
        kernel.transition(&r2.id, "UNDERWRITING", "o1", "officer", AuthorityLevel::Human).unwrap();
        kernel.transition(&r2.id, "SENIOR_REVIEW", "sr1", "senior_officer", AuthorityLevel::Human).unwrap();
        let result = kernel.transition(&r2.id, "REJECTED", "sr1", "senior_officer", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Success { .. }));
        assert_eq!(kernel.get_resource(&r2.id).unwrap().state, "REJECTED");
    }

    #[test]
    fn test_multiple_loans_independent() {
        let mut kernel = build_loan_kernel();

        let r1 = match kernel.create_resource("loan", serde_json::json!({"amount": 100_000}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };
        let r2 = match kernel.create_resource("loan", serde_json::json!({"amount": 200_000}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Advance r1 to DISBURSED
        kernel.transition(&r1.id, "DOCUMENT_CHECK", "c1", "manager", AuthorityLevel::Human).unwrap();
        kernel.transition(&r1.id, "UNDERWRITING", "o1", "manager", AuthorityLevel::Human).unwrap();
        kernel.transition(&r1.id, "APPROVED", "sr1", "manager", AuthorityLevel::Human).unwrap();
        kernel.transition(&r1.id, "DISBURSED", "m1", "manager", AuthorityLevel::Human).unwrap();

        // r2 should still be APPLIED
        assert_eq!(kernel.get_resource(&r1.id).unwrap().state, "DISBURSED");
        assert_eq!(kernel.get_resource(&r2.id).unwrap().state, "APPLIED");

        // Reject r2
        kernel.transition(&r2.id, "DOCUMENT_CHECK", "c1", "manager", AuthorityLevel::Human).unwrap();
        kernel.transition(&r2.id, "REJECTED", "o1", "manager", AuthorityLevel::Human).unwrap();
        assert_eq!(kernel.get_resource(&r2.id).unwrap().state, "REJECTED");

        // Verify audit isolation
        let audit1 = kernel.get_audit(&r1.id);
        let audit2 = kernel.get_audit(&r2.id);
        assert_eq!(audit1.len(), 4);
        assert_eq!(audit2.len(), 2);
    }

    #[test]
    fn test_desired_state_through_multi_step_approval() {
        let mut kernel = build_loan_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({"amount": 100}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Set desired to DISBURSED, should traverse:
        // APPLIED -> DOCUMENT_CHECK -> UNDERWRITING -> APPROVED -> DISBURSED
        kernel.set_desired_state(&resource.id, "DISBURSED", "manager", AuthorityLevel::System).unwrap();

        let final_r = kernel.get_resource(&resource.id).unwrap();
        assert_eq!(final_r.state, "DISBURSED");

        // Verify it took the shortest path
        let audit = kernel.get_audit(&resource.id);
        let states: Vec<&str> = audit.iter().map(|a| a.new_state.as_str()).collect();
        assert_eq!(states, vec!["DOCUMENT_CHECK", "UNDERWRITING", "APPROVED", "DISBURSED"]);
    }
}

#[cfg(test)]
mod insurance_claims {
    //! Insurance claims processing workflow.

    use crate::invariant_checker::*;
    use crate::policy_engine::{PolicyDefinition, PolicyEvaluator};
    use crate::resource_registry::ResourceTypeDefinition;
    use crate::roles::{Permission, RoleDefinition};
    use crate::state_machine::*;
    use crate::transaction::Kernel;
    use crate::types::*;

    fn build_claims_kernel() -> Kernel {
        let mut kernel = Kernel::new();

        let states = vec![
            StateDefinition { name: "FILED".into(), status: StateStatus::Active },
            StateDefinition { name: "UNDER_INVESTIGATION".into(), status: StateStatus::Active },
            StateDefinition { name: "ASSESSED".into(), status: StateStatus::Active },
            StateDefinition { name: "APPROVED".into(), status: StateStatus::Active },
            StateDefinition { name: "PAID".into(), status: StateStatus::Terminal },
            StateDefinition { name: "DENIED".into(), status: StateStatus::Terminal },
            StateDefinition { name: "FRAUD_FLAGGED".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "FILED".into(), to_state: "UNDER_INVESTIGATION".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDER_INVESTIGATION".into(), to_state: "ASSESSED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDER_INVESTIGATION".into(), to_state: "FRAUD_FLAGGED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "ASSESSED".into(), to_state: "APPROVED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "ASSESSED".into(), to_state: "DENIED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "APPROVED".into(), to_state: "PAID".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "FILED".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "claim".into(),
            schema: serde_json::json!({}),
            state_machine: sm,
        }).unwrap();

        kernel.role_registry.register(RoleDefinition {
            name: "adjuster".into(),
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:UNDER_INVESTIGATION"),
                Permission::from_shorthand("transition:ASSESSED"),
                Permission::from_shorthand("transition:FRAUD_FLAGGED"),
            ],
        });
        kernel.role_registry.register(RoleDefinition {
            name: "supervisor".into(),
            permissions: vec![
                Permission::from_shorthand("transition:*"),
            ],
        });

        kernel
    }

    #[test]
    fn test_claim_approval_and_payment() {
        let mut kernel = build_claims_kernel();
        let claim = match kernel.create_resource("claim", serde_json::json!({"amount": 25_000, "type": "auto"}), "claimant", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        kernel.transition(&claim.id, "UNDER_INVESTIGATION", "adj1", "adjuster", AuthorityLevel::Human).unwrap();
        kernel.transition(&claim.id, "ASSESSED", "adj1", "adjuster", AuthorityLevel::Human).unwrap();
        kernel.transition(&claim.id, "APPROVED", "sup1", "supervisor", AuthorityLevel::Human).unwrap();
        kernel.transition(&claim.id, "PAID", "sup1", "supervisor", AuthorityLevel::Human).unwrap();

        assert_eq!(kernel.get_resource(&claim.id).unwrap().state, "PAID");
        assert_eq!(kernel.get_audit(&claim.id).len(), 4);
    }

    #[test]
    fn test_fraud_flagging_is_terminal() {
        let mut kernel = build_claims_kernel();
        let claim = match kernel.create_resource("claim", serde_json::json!({"amount": 1_000_000}), "claimant", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        kernel.transition(&claim.id, "UNDER_INVESTIGATION", "adj1", "adjuster", AuthorityLevel::Human).unwrap();
        kernel.transition(&claim.id, "FRAUD_FLAGGED", "adj1", "adjuster", AuthorityLevel::Human).unwrap();

        // Cannot transition from FRAUD_FLAGGED
        let result = kernel.transition(&claim.id, "ASSESSED", "sup1", "supervisor", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "validate_state_machine"));
    }

    #[test]
    fn test_claim_amount_cap_policy() {
        let mut kernel = build_claims_kernel();

        struct ClaimCap;
        impl PolicyEvaluator for ClaimCap {
            fn evaluate(&self, r: &Resource, ctx: &TransitionContext, _q: &dyn SystemQuery) -> PolicyResult {
                if ctx.to_state == "APPROVED" {
                    let amount = r.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    if amount > 500_000.0 {
                        return PolicyResult::deny(format!("Claim {} exceeds auto-approval cap of 500K", amount));
                    }
                }
                PolicyResult::allow()
            }
        }

        kernel.policy_engine.register(PolicyDefinition {
            name: "claim_cap".into(),
            description: "Auto-approval cap".into(),
            evaluator: Box::new(ClaimCap),
            applicable_states: vec![],
            resource_types: vec!["claim".into()],
            priority: 80,
        });

        // Small claim - auto-approved
        let c1 = match kernel.create_resource("claim", serde_json::json!({"amount": 10_000}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!(""),
        };
        kernel.transition(&c1.id, "UNDER_INVESTIGATION", "a", "adjuster", AuthorityLevel::Human).unwrap();
        kernel.transition(&c1.id, "ASSESSED", "a", "adjuster", AuthorityLevel::Human).unwrap();
        assert!(matches!(
            kernel.transition(&c1.id, "APPROVED", "s", "supervisor", AuthorityLevel::Human).unwrap(),
            TransitionOutcome::Success { .. }
        ));

        // Large claim - blocked
        let c2 = match kernel.create_resource("claim", serde_json::json!({"amount": 750_000}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!(""),
        };
        kernel.transition(&c2.id, "UNDER_INVESTIGATION", "a", "adjuster", AuthorityLevel::Human).unwrap();
        kernel.transition(&c2.id, "ASSESSED", "a", "adjuster", AuthorityLevel::Human).unwrap();
        let result = kernel.transition(&c2.id, "APPROVED", "s", "supervisor", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "evaluate_policies"));
    }

    #[test]
    fn test_cross_resource_exposure_invariant() {
        let mut kernel = build_claims_kernel();

        // Eventual invariant: total approved claims per policy cannot exceed coverage
        struct ExposureLimit;
        impl InvariantCheck for ExposureLimit {
            fn check(&self, resource: &Resource, query: &dyn SystemQuery) -> InvariantResult {
                if resource.state != "APPROVED" {
                    return InvariantResult::ok();
                }
                // Sum all approved claims
                let all_claims = query.list_by_type("claim");
                let total: f64 = all_claims.iter()
                    .filter(|c| c.state == "APPROVED" || c.state == "PAID")
                    .filter_map(|c| c.data.get("amount").and_then(|v| v.as_f64()))
                    .sum();

                let this_amount = resource.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if total + this_amount > 2_000_000.0 {
                    InvariantResult::violated(format!("Total exposure {} + {} exceeds 2M limit", total, this_amount))
                } else {
                    InvariantResult::ok()
                }
            }
        }

        kernel.invariant_checker.register(InvariantDefinition {
            name: "exposure_limit".into(),
            description: "Total exposure cap".into(),
            mode: InvariantMode::Strong,
            scope: InvariantScope::CrossResource,
            resource_types: vec!["claim".into()],
            checker: Box::new(ExposureLimit),
        });

        // Approve claims until we hit the limit
        for i in 0..3 {
            let claim = match kernel.create_resource("claim", serde_json::json!({"amount": 600_000}), "sys", AuthorityLevel::System).unwrap() {
                TransitionOutcome::Success { resource, .. } => resource,
                _ => panic!("Failed to create claim {}", i),
            };
            kernel.transition(&claim.id, "UNDER_INVESTIGATION", "a", "adjuster", AuthorityLevel::Human).unwrap();
            kernel.transition(&claim.id, "ASSESSED", "a", "adjuster", AuthorityLevel::Human).unwrap();

            let result = kernel.transition(&claim.id, "APPROVED", "s", "supervisor", AuthorityLevel::Human).unwrap();
            if i < 3 {
                // First few should succeed (total <= 2M)
                if let TransitionOutcome::Rejected { .. } = &result {
                    // Third one (1.8M total) should fail on 4th approval attempt at invariant
                    if i == 2 {
                        // 600K * 3 = 1.8M > 2M with the proposed addition. This depends on timing.
                        // Actually 600K + 600K = 1.2M for first two. Third = 1.8M still under 2M.
                        // So 3 should all pass. Would need a 4th to fail.
                    }
                }
            }
        }

        // Fourth claim should be blocked by exposure limit
        let c4 = match kernel.create_resource("claim", serde_json::json!({"amount": 600_000}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Failed to create claim 4"),
        };
        kernel.transition(&c4.id, "UNDER_INVESTIGATION", "a", "adjuster", AuthorityLevel::Human).unwrap();
        kernel.transition(&c4.id, "ASSESSED", "a", "adjuster", AuthorityLevel::Human).unwrap();

        let result = kernel.transition(&c4.id, "APPROVED", "s", "supervisor", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "verify_invariants"),
            "Fourth claim approval should be blocked by exposure limit invariant");
    }
}

#[cfg(test)]
mod cascade_and_controllers {
    //! Tests for controller cascading, depth limits, and interaction patterns.

    use crate::controller_scheduler::{ControllerHandler, ControllerRegistration};
    use crate::errors::KernelError;
    use crate::event_log::EventPattern;
    use crate::invariant_checker::SystemQuery;
    use crate::resource_registry::ResourceTypeDefinition;
    use crate::state_machine::*;
    use crate::transaction::Kernel;
    use crate::types::*;

    fn simple_kernel() -> Kernel {
        let mut kernel = Kernel::new();
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Active },
            StateDefinition { name: "C".into(), status: StateStatus::Active },
            StateDefinition { name: "D".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "B".into(), to_state: "C".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "C".into(), to_state: "D".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "item".into(),
            schema: serde_json::json!({}),
            state_machine: sm,
        }).unwrap();
        kernel
    }

    #[test]
    fn test_chained_controllers() {
        // Controller 1: on item.created -> transition to B
        // Controller 2: on item.transitioned -> if state == B, transition to C
        // Controller 3: on item.transitioned -> if state == C, transition to D

        let mut kernel = simple_kernel();

        struct AutoAdvance { target_from: String, target_to: String }
        impl ControllerHandler for AutoAdvance {
            fn reconcile(&self, r: &Resource, _q: &dyn SystemQuery) -> Result<ControllerAction, KernelError> {
                if r.state == self.target_from {
                    Ok(ControllerAction::Transition { to_state: self.target_to.clone() })
                } else {
                    Ok(ControllerAction::NoOp)
                }
            }
        }

        kernel.controller_scheduler.register(ControllerRegistration {
            name: "auto-a-to-b".into(),
            priority: 90,
            enforces: vec![],
            on_events: vec![EventPattern::parse("item.created")],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(AutoAdvance { target_from: "A".into(), target_to: "B".into() }),
        });
        kernel.controller_scheduler.register(ControllerRegistration {
            name: "auto-b-to-c".into(),
            priority: 80,
            enforces: vec![],
            on_events: vec![EventPattern::parse("item.transitioned")],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(AutoAdvance { target_from: "B".into(), target_to: "C".into() }),
        });
        kernel.controller_scheduler.register(ControllerRegistration {
            name: "auto-c-to-d".into(),
            priority: 70,
            enforces: vec![],
            on_events: vec![EventPattern::parse("item.transitioned")],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(AutoAdvance { target_from: "C".into(), target_to: "D".into() }),
        });

        let result = kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap();
        match result {
            TransitionOutcome::Success { resource, .. } => {
                // Should have cascaded all the way to D
                let final_r = kernel.get_resource(&resource.id).unwrap();
                assert_eq!(final_r.state, "D");
            }
            _ => panic!("Expected success"),
        }
    }

    #[test]
    fn test_controller_noop_stops_cascade() {
        let mut kernel = simple_kernel();

        struct ConditionalAdvance;
        impl ControllerHandler for ConditionalAdvance {
            fn reconcile(&self, r: &Resource, _q: &dyn SystemQuery) -> Result<ControllerAction, KernelError> {
                if r.state == "A" {
                    Ok(ControllerAction::Transition { to_state: "B".into() })
                } else {
                    Ok(ControllerAction::NoOp) // Stop cascading
                }
            }
        }

        kernel.controller_scheduler.register(ControllerRegistration {
            name: "conditional".into(),
            priority: 50,
            enforces: vec![],
            on_events: vec![EventPattern::Wildcard],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(ConditionalAdvance),
        });

        let result = kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap();
        match result {
            TransitionOutcome::Success { resource, .. } => {
                let final_r = kernel.get_resource(&resource.id).unwrap();
                assert_eq!(final_r.state, "B"); // Stopped at B, didn't go to C or D
            }
            _ => panic!("Expected success"),
        }
    }

    #[test]
    fn test_cascade_depth_limit() {
        // Create a long chain state machine that exceeds cascade depth
        let states: Vec<StateDefinition> = (0..15).map(|i| {
            StateDefinition {
                name: format!("S{}", i),
                status: if i == 14 { StateStatus::Terminal } else { StateStatus::Active },
            }
        }).collect();

        let transitions: Vec<TransitionDefinition> = (0..14).map(|i| {
            TransitionDefinition {
                from_state: format!("S{}", i),
                to_state: format!("S{}", i + 1),
                guard: None,
                required_role: None,
            }
        }).collect();

        let mut kernel = Kernel::new();
        let sm = StateMachine::new(states, transitions, "S0".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "deep".into(),
            schema: serde_json::json!({}),
            state_machine: sm,
        }).unwrap();

        // Set max cascade depth to 5
        kernel.controller_scheduler.max_cascade_depth = 5;

        let resource = match kernel.create_resource("deep", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Try to reconcile to S14 - should fail with cascade depth exceeded
        let result = kernel.set_desired_state(&resource.id, "S14", "sys", AuthorityLevel::System);
        assert!(result.is_err());
        match result {
            Err(KernelError::CascadeDepthExceeded { depth: _, max }) => {
                assert_eq!(max, 5);
            }
            other => panic!("Expected CascadeDepthExceeded, got {:?}", other),
        }
    }

    #[test]
    fn test_cascade_depth_tracked_through_controller_transitions() {
        // Verifies fix for the bug where cascade depth was always 0.
        // Build a 15-state chain with auto-advance controllers.
        // With max_cascade_depth=5, controllers should stop advancing.
        let states: Vec<StateDefinition> = (0..15).map(|i| StateDefinition {
            name: format!("S{}", i),
            status: if i == 14 { StateStatus::Terminal } else { StateStatus::Active },
        }).collect();
        let transitions: Vec<TransitionDefinition> = (0..14).map(|i| TransitionDefinition {
            from_state: format!("S{}", i),
            to_state: format!("S{}", i + 1),
            guard: None, required_role: None,
        }).collect();

        let mut kernel = Kernel::new();
        let sm = StateMachine::new(states, transitions, "S0".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "chain".into(), schema: serde_json::json!({}), state_machine: sm,
        }).unwrap();

        // Auto-advance controller for each state
        for i in 0..14 {
            let from_s = format!("S{}", i);
            let to_s = format!("S{}", i + 1);
            struct Advance { from: String, to: String }
            impl ControllerHandler for Advance {
                fn reconcile(&self, r: &Resource, _q: &dyn SystemQuery) -> Result<ControllerAction, KernelError> {
                    if r.state == self.from {
                        Ok(ControllerAction::Transition { to_state: self.to.clone() })
                    } else {
                        Ok(ControllerAction::NoOp)
                    }
                }
            }
            kernel.controller_scheduler.register(ControllerRegistration {
                name: format!("auto-{}", i),
                priority: (90 - i) as u32,
                enforces: vec![],
                on_events: vec![EventPattern::Wildcard],
                authority_level: AuthorityLevel::Controller,
                handler: Box::new(Advance { from: from_s, to: to_s }),
            });
        }

        kernel.controller_scheduler.max_cascade_depth = 5;

        let resource = match kernel.create_resource("chain", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Should NOT reach S14 — cascade depth limit should stop it
        let final_r = kernel.get_resource(&resource.id).unwrap();
        let state_idx: usize = final_r.state[1..].parse().unwrap();
        assert!(state_idx < 14,
            "With max_cascade_depth=5, should not reach S14. Got {}", final_r.state);
        assert!(state_idx > 0,
            "Should have advanced at least one step. Got {}", final_r.state);
    }

    #[test]
    fn test_multiple_controllers_priority_ordering() {
        let mut kernel = Kernel::new();
        let states = vec![
            StateDefinition { name: "START".into(), status: StateStatus::Active },
            StateDefinition { name: "PATH_A".into(), status: StateStatus::Terminal },
            StateDefinition { name: "PATH_B".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "START".into(), to_state: "PATH_A".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "START".into(), to_state: "PATH_B".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "START".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "item".into(),
            schema: serde_json::json!({}),
            state_machine: sm,
        }).unwrap();

        // High priority controller goes to PATH_A
        struct GoA;
        impl ControllerHandler for GoA {
            fn reconcile(&self, r: &Resource, _q: &dyn SystemQuery) -> Result<ControllerAction, KernelError> {
                if r.state == "START" {
                    Ok(ControllerAction::Transition { to_state: "PATH_A".into() })
                } else {
                    Ok(ControllerAction::NoOp)
                }
            }
        }

        // Low priority controller goes to PATH_B (should not win)
        struct GoB;
        impl ControllerHandler for GoB {
            fn reconcile(&self, r: &Resource, _q: &dyn SystemQuery) -> Result<ControllerAction, KernelError> {
                if r.state == "START" {
                    Ok(ControllerAction::Transition { to_state: "PATH_B".into() })
                } else {
                    Ok(ControllerAction::NoOp)
                }
            }
        }

        kernel.controller_scheduler.register(ControllerRegistration {
            name: "go-a".into(),
            priority: 90,
            enforces: vec![],
            on_events: vec![EventPattern::parse("item.created")],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(GoA),
        });
        kernel.controller_scheduler.register(ControllerRegistration {
            name: "go-b".into(),
            priority: 10,
            enforces: vec![],
            on_events: vec![EventPattern::parse("item.created")],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(GoB),
        });

        let result = kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap();
        let resource = match result {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        let final_r = kernel.get_resource(&resource.id).unwrap();
        assert_eq!(final_r.state, "PATH_A", "Higher priority controller should win");
    }
}

#[cfg(test)]
mod state_machine_edge_cases {
    use crate::state_machine::*;
    use crate::errors::KernelError;
    use crate::types::*;
    use chrono::Utc;

    #[test]
    fn test_self_loop_state() {
        // Self-loops should be valid (e.g., PROCESSING -> PROCESSING for retries)
        let states = vec![
            StateDefinition { name: "PROCESSING".into(), status: StateStatus::Active },
            StateDefinition { name: "DONE".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "PROCESSING".into(), to_state: "PROCESSING".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "PROCESSING".into(), to_state: "DONE".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "PROCESSING".into()).unwrap();
        assert!(sm.validate_transition("PROCESSING", "PROCESSING"));
        assert!(sm.validate_transition("PROCESSING", "DONE"));
    }

    #[test]
    fn test_cycle_detection() {
        // Cycles are allowed (unlike DAGs) - the convergence guarantee handles them
        let states = vec![
            StateDefinition { name: "REVIEW".into(), status: StateStatus::Active },
            StateDefinition { name: "REVISION".into(), status: StateStatus::Active },
            StateDefinition { name: "APPROVED".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "REVIEW".into(), to_state: "REVISION".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "REVISION".into(), to_state: "REVIEW".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "REVIEW".into(), to_state: "APPROVED".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "REVIEW".into()).unwrap();
        assert!(sm.validate_transition("REVIEW", "REVISION"));
        assert!(sm.validate_transition("REVISION", "REVIEW"));
        assert!(sm.validate_transition("REVIEW", "APPROVED"));
        // No dead ends
        assert!(sm.detect_dead_ends().is_empty());
    }

    #[test]
    fn test_single_state_terminal() {
        let states = vec![
            StateDefinition { name: "ONLY".into(), status: StateStatus::Terminal },
        ];
        let sm = StateMachine::new(states, vec![], "ONLY".into()).unwrap();
        assert!(sm.is_terminal("ONLY"));
        assert!(sm.get_valid_transitions("ONLY").is_empty());
    }

    #[test]
    fn test_large_state_machine() {
        // 50 states in a chain
        let states: Vec<StateDefinition> = (0..50).map(|i| StateDefinition {
            name: format!("S{}", i),
            status: if i == 49 { StateStatus::Terminal } else { StateStatus::Active },
        }).collect();
        let transitions: Vec<TransitionDefinition> = (0..49).map(|i| TransitionDefinition {
            from_state: format!("S{}", i),
            to_state: format!("S{}", i + 1),
            guard: None,
            required_role: None,
        }).collect();

        let sm = StateMachine::new(states, transitions, "S0".into()).unwrap();
        assert!(sm.validate_transition("S0", "S1"));
        assert!(sm.validate_transition("S48", "S49"));
        assert!(!sm.validate_transition("S0", "S49")); // Can't skip
        assert!(sm.detect_dead_ends().is_empty());
        assert!(sm.detect_unreachable().is_empty());

        let dist = sm.distance_to("S49");
        assert_eq!(dist["S0"], 49);
        assert_eq!(dist["S48"], 1);
    }

    #[test]
    fn test_diamond_state_machine() {
        //    A
        //   / \
        //  B   C
        //   \ /
        //    D (terminal)
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Active },
            StateDefinition { name: "C".into(), status: StateStatus::Active },
            StateDefinition { name: "D".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "A".into(), to_state: "C".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "B".into(), to_state: "D".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "C".into(), to_state: "D".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();

        // Both paths lead to D
        let dist = sm.distance_to("D");
        assert_eq!(dist["A"], 2);
        assert_eq!(dist["B"], 1);
        assert_eq!(dist["C"], 1);

        assert!(sm.detect_dead_ends().is_empty());
        assert!(sm.detect_unreachable().is_empty());
    }

    #[test]
    fn test_guard_with_data_check() {
        struct MinAmountGuard(f64);
        impl GuardFn for MinAmountGuard {
            fn evaluate(&self, resource: &Resource) -> Result<bool, KernelError> {
                let amount = resource.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                Ok(amount >= self.0)
            }
        }

        let states = vec![
            StateDefinition { name: "DRAFT".into(), status: StateStatus::Active },
            StateDefinition { name: "SUBMITTED".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition {
                from_state: "DRAFT".into(),
                to_state: "SUBMITTED".into(),
                guard: Some(Box::new(MinAmountGuard(100.0))),
                required_role: None,
            },
        ];
        let sm = StateMachine::new(states, transitions, "DRAFT".into()).unwrap();

        let low = Resource {
            id: ResourceId::new(), resource_type: "test".into(), state: "DRAFT".into(),
            desired_state: None, data: serde_json::json!({"amount": 50}),
            version: 1, created_at: Utc::now(), updated_at: Utc::now(),
        };
        assert_eq!(sm.evaluate_guard("DRAFT", "SUBMITTED", &low).unwrap(), false);

        let high = Resource {
            id: ResourceId::new(), resource_type: "test".into(), state: "DRAFT".into(),
            desired_state: None, data: serde_json::json!({"amount": 200}),
            version: 1, created_at: Utc::now(), updated_at: Utc::now(),
        };
        assert_eq!(sm.evaluate_guard("DRAFT", "SUBMITTED", &high).unwrap(), true);
    }

    #[test]
    fn test_disconnected_subgraph_detection() {
        // States A->B and C->D are disconnected; C and D unreachable from A
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Terminal },
            StateDefinition { name: "C".into(), status: StateStatus::Active },
            StateDefinition { name: "D".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "C".into(), to_state: "D".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();

        let unreachable = sm.detect_unreachable();
        assert!(unreachable.contains(&"C".to_string()));
        assert!(unreachable.contains(&"D".to_string()));
        assert_eq!(unreachable.len(), 2);
    }
}

#[cfg(test)]
mod event_pattern_edge_cases {
    use crate::event_log::EventPattern;

    #[test]
    fn test_exact_match() {
        let p = EventPattern::Exact("loan.created".into());
        assert!(p.matches("loan.created"));
        assert!(!p.matches("loan.created.extra"));
        assert!(!p.matches("loan."));
        assert!(!p.matches(""));
    }

    #[test]
    fn test_prefix_match() {
        let p = EventPattern::parse("loan.*");
        assert!(p.matches("loan.created"));
        assert!(p.matches("loan.transitioned"));
        assert!(p.matches("loan.desired_state_set"));
        assert!(!p.matches("application.created"));
        assert!(!p.matches("loanx.created")); // Prefix includes the dot
    }

    #[test]
    fn test_wildcard_match() {
        let p = EventPattern::Wildcard;
        assert!(p.matches("anything"));
        assert!(p.matches(""));
        assert!(p.matches("deeply.nested.event.type"));
    }

    #[test]
    fn test_parse_patterns() {
        assert!(matches!(EventPattern::parse("*"), EventPattern::Wildcard));
        assert!(matches!(EventPattern::parse("loan.created"), EventPattern::Exact(_)));
        assert!(matches!(EventPattern::parse("loan.*"), EventPattern::Prefix(_)));
    }

    #[test]
    fn test_empty_event_type() {
        assert!(EventPattern::Exact("".into()).matches(""));
        assert!(!EventPattern::Exact("".into()).matches("something"));
    }

    #[test]
    fn test_nested_prefix() {
        let p = EventPattern::parse("system.audit.*");
        assert!(p.matches("system.audit.created"));
        assert!(p.matches("system.audit.updated"));
        assert!(!p.matches("system.event.created"));
    }
}

#[cfg(test)]
mod policy_engine_edge_cases {
    use crate::invariant_checker::SystemQuery;
    use crate::policy_engine::*;
    use crate::types::*;
    use chrono::Utc;

    struct EmptyQuery;
    impl SystemQuery for EmptyQuery {
        fn get_resource(&self, _id: &ResourceId) -> Option<Resource> { None }
        fn list_by_type(&self, _t: &str) -> Vec<Resource> { vec![] }
    }

    struct AllowAll;
    impl PolicyEvaluator for AllowAll {
        fn evaluate(&self, _r: &Resource, _c: &TransitionContext, _q: &dyn SystemQuery) -> PolicyResult {
            PolicyResult::allow()
        }
    }

    struct DenyIf { field: String, threshold: f64 }
    impl PolicyEvaluator for DenyIf {
        fn evaluate(&self, r: &Resource, _c: &TransitionContext, _q: &dyn SystemQuery) -> PolicyResult {
            let val = r.data.get(&self.field).and_then(|v| v.as_f64()).unwrap_or(0.0);
            if val > self.threshold {
                PolicyResult::deny(format!("{} = {} exceeds {}", self.field, val, self.threshold))
            } else {
                PolicyResult::allow()
            }
        }
    }

    fn make_resource(rtype: &str, state: &str, data: serde_json::Value) -> Resource {
        Resource {
            id: ResourceId::new(), resource_type: rtype.into(), state: state.into(),
            desired_state: None, data, version: 1,
            created_at: Utc::now(), updated_at: Utc::now(),
        }
    }

    fn make_ctx(r: &Resource, to: &str) -> TransitionContext {
        TransitionContext {
            resource_id: r.id.clone(), resource_type: r.resource_type.clone(),
            from_state: r.state.clone(), to_state: to.into(),
            actor: "test".into(), role: "test".into(),
            authority_level: AuthorityLevel::Human,
        }
    }

    #[test]
    fn test_no_policies_allows_all() {
        let engine = PolicyEngine::new();
        let r = make_resource("loan", "APPLIED", serde_json::json!({}));
        let ctx = make_ctx(&r, "UW");
        assert!(engine.first_denied(&r, &ctx, &EmptyQuery).is_none());
        assert!(engine.evaluate_all(&r, &ctx, &EmptyQuery).is_empty());
    }

    #[test]
    fn test_wildcard_resource_type_applies_to_all() {
        let mut engine = PolicyEngine::new();
        engine.register(PolicyDefinition {
            name: "global".into(),
            description: "".into(),
            evaluator: Box::new(AllowAll),
            applicable_states: vec![],
            resource_types: vec![], // Empty = applies to all
            priority: 10,
        });

        let loan = make_resource("loan", "APPLIED", serde_json::json!({}));
        let ctx1 = make_ctx(&loan, "UW");
        assert_eq!(engine.evaluate_all(&loan, &ctx1, &EmptyQuery).len(), 1);

        let claim = make_resource("claim", "FILED", serde_json::json!({}));
        let ctx2 = make_ctx(&claim, "REVIEW");
        assert_eq!(engine.evaluate_all(&claim, &ctx2, &EmptyQuery).len(), 1);
    }

    #[test]
    fn test_many_policies_all_evaluated() {
        let mut engine = PolicyEngine::new();
        for i in 0..20 {
            engine.register(PolicyDefinition {
                name: format!("policy_{}", i),
                description: "".into(),
                evaluator: Box::new(AllowAll),
                applicable_states: vec![],
                resource_types: vec![],
                priority: i as u32,
            });
        }

        let r = make_resource("loan", "APPLIED", serde_json::json!({}));
        let ctx = make_ctx(&r, "UW");
        let evals = engine.evaluate_all(&r, &ctx, &EmptyQuery);
        assert_eq!(evals.len(), 20);
        // Verify priority ordering (highest first)
        assert_eq!(evals[0].name, "policy_19");
        assert_eq!(evals[19].name, "policy_0");
    }

    #[test]
    fn test_first_denied_short_circuits() {
        let mut engine = PolicyEngine::new();
        engine.register(PolicyDefinition {
            name: "blocker".into(),
            description: "".into(),
            evaluator: Box::new(DenyIf { field: "amount".into(), threshold: 0.0 }),
            applicable_states: vec![],
            resource_types: vec![],
            priority: 100, // Highest priority
        });
        engine.register(PolicyDefinition {
            name: "also_blocks".into(),
            description: "".into(),
            evaluator: Box::new(DenyIf { field: "amount".into(), threshold: 0.0 }),
            applicable_states: vec![],
            resource_types: vec![],
            priority: 50,
        });

        let r = make_resource("loan", "APPLIED", serde_json::json!({"amount": 100}));
        let ctx = make_ctx(&r, "UW");
        let denied = engine.first_denied(&r, &ctx, &EmptyQuery).unwrap();
        assert_eq!(denied.name, "blocker"); // Highest priority reported
    }
}

#[cfg(test)]
mod error_handling {
    //! Tests for error conditions and callback failures.

    use crate::resource_registry::ResourceTypeDefinition;
    use crate::state_machine::*;
    use crate::transaction::Kernel;
    use crate::types::*;
    use crate::errors::KernelError;

    fn simple_kernel() -> Kernel {
        let mut kernel = Kernel::new();
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "item".into(), schema: serde_json::json!({}), state_machine: sm,
        }).unwrap();
        kernel
    }

    #[test]
    fn test_transition_nonexistent_resource() {
        let mut kernel = simple_kernel();
        let fake_id = ResourceId::new();
        let result = kernel.transition(&fake_id, "B", "user", "role", AuthorityLevel::Human);
        assert!(matches!(result, Err(KernelError::ResourceNotFound(_))));
    }

    #[test]
    fn test_create_unregistered_type() {
        let mut kernel = simple_kernel();
        let result = kernel.create_resource("nonexistent", serde_json::json!({}), "user", AuthorityLevel::Human);
        assert!(matches!(result, Err(KernelError::TypeNotRegistered(_))));
    }

    #[test]
    fn test_set_desired_nonexistent_resource() {
        let mut kernel = simple_kernel();
        let fake_id = ResourceId::new();
        let result = kernel.set_desired_state(&fake_id, "B", "user", AuthorityLevel::Human);
        assert!(matches!(result, Err(KernelError::ResourceNotFound(_))));
    }

    #[test]
    fn test_set_desired_undefined_state() {
        let mut kernel = simple_kernel();
        let r = match kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };
        let result = kernel.set_desired_state(&r.id, "NONEXISTENT", "user", AuthorityLevel::Human);
        assert!(matches!(result, Err(KernelError::StateNotDefined(_))));
    }

    #[test]
    fn test_double_registration_fails() {
        let mut kernel = simple_kernel();
        let states = vec![
            StateDefinition { name: "X".into(), status: StateStatus::Terminal },
        ];
        let sm = StateMachine::new(states, vec![], "X".into()).unwrap();
        let result = kernel.register_type(ResourceTypeDefinition {
            name: "item".into(), schema: serde_json::json!({}), state_machine: sm,
        });
        assert!(matches!(result, Err(KernelError::TypeAlreadyRegistered(_))));
    }

    #[test]
    fn test_transition_from_terminal_is_rejected() {
        let mut kernel = simple_kernel();
        let r = match kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };
        kernel.transition(&r.id, "B", "sys", "sys", AuthorityLevel::Controller).unwrap();

        // Now at terminal B, try to go anywhere
        let result = kernel.transition(&r.id, "A", "sys", "sys", AuthorityLevel::Controller).unwrap();
        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "validate_state_machine"));
    }

    #[test]
    fn test_rejected_transition_no_side_effects() {
        let mut kernel = simple_kernel();
        let r = match kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        let events_before = kernel.get_events(&r.id).len();
        let audit_before = kernel.get_audit(&r.id).len();
        let version_before = kernel.get_resource(&r.id).unwrap().version;

        // Invalid transition
        kernel.transition(&r.id, "NONEXISTENT", "sys", "sys", AuthorityLevel::Controller).unwrap();

        // No side effects
        assert_eq!(kernel.get_events(&r.id).len(), events_before);
        assert_eq!(kernel.get_audit(&r.id).len(), audit_before);
        assert_eq!(kernel.get_resource(&r.id).unwrap().version, version_before);
        assert_eq!(kernel.get_resource(&r.id).unwrap().state, "A");
    }
}

#[cfg(test)]
mod snapshot_store_tests {
    use crate::snapshot_store::*;
    use crate::types::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_snapshot(rid: &ResourceId, state: &str, offset: u64, version: u64) -> Snapshot {
        Snapshot {
            id: Uuid::new_v4(),
            resource_id: rid.clone(),
            state: state.into(),
            data: serde_json::json!({"amount": 100}),
            version,
            event_offset: offset,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_snapshot_ordering() {
        let mut store = InMemorySnapshotStore::new();
        let rid = ResourceId::new();

        store.create(make_snapshot(&rid, "A", 0, 1));
        store.create(make_snapshot(&rid, "B", 5, 2));
        store.create(make_snapshot(&rid, "C", 10, 3));

        let latest = store.get_latest(&rid).unwrap();
        assert_eq!(latest.state, "C");
        assert_eq!(latest.event_offset, 10);
        assert_eq!(latest.version, 3);
    }

    #[test]
    fn test_snapshots_per_resource_isolation() {
        let mut store = InMemorySnapshotStore::new();
        let r1 = ResourceId::new();
        let r2 = ResourceId::new();

        store.create(make_snapshot(&r1, "A", 0, 1));
        store.create(make_snapshot(&r2, "B", 0, 1));
        store.create(make_snapshot(&r1, "C", 5, 2));

        assert_eq!(store.get_latest(&r1).unwrap().state, "C");
        assert_eq!(store.get_latest(&r2).unwrap().state, "B");
    }

    #[test]
    fn test_snapshot_data_preserved() {
        let mut store = InMemorySnapshotStore::new();
        let rid = ResourceId::new();
        let snapshot = Snapshot {
            id: Uuid::new_v4(),
            resource_id: rid.clone(),
            state: "APPROVED".into(),
            data: serde_json::json!({
                "amount": 500_000,
                "applicant": "Acme Corp",
                "approved_by": "mgr1",
                "nested": {"key": "value"}
            }),
            version: 5,
            event_offset: 42,
            timestamp: Utc::now(),
        };
        store.create(snapshot);

        let retrieved = store.get_latest(&rid).unwrap();
        assert_eq!(retrieved.data["amount"], 500_000);
        assert_eq!(retrieved.data["applicant"], "Acme Corp");
        assert_eq!(retrieved.data["nested"]["key"], "value");
    }
}

#[cfg(test)]
mod data_integrity {
    //! Tests ensuring data stays consistent through transitions.

    use crate::resource_registry::ResourceTypeDefinition;
    use crate::state_machine::*;
    use crate::transaction::Kernel;
    use crate::types::*;

    #[test]
    fn test_data_preserved_across_transitions() {
        let mut kernel = Kernel::new();
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Active },
            StateDefinition { name: "C".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "B".into(), to_state: "C".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "item".into(), schema: serde_json::json!({}), state_machine: sm,
        }).unwrap();

        let data = serde_json::json!({
            "name": "Test Item",
            "amount": 42,
            "tags": ["important", "urgent"],
            "metadata": {"created_by": "test", "priority": 1}
        });

        let r = match kernel.create_resource("item", data.clone(), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Transition through states
        kernel.transition(&r.id, "B", "sys", "sys", AuthorityLevel::Controller).unwrap();
        kernel.transition(&r.id, "C", "sys", "sys", AuthorityLevel::Controller).unwrap();

        // Verify data integrity
        let final_r = kernel.get_resource(&r.id).unwrap();
        assert_eq!(final_r.data["name"], "Test Item");
        assert_eq!(final_r.data["amount"], 42);
        assert_eq!(final_r.data["tags"][0], "important");
        assert_eq!(final_r.data["metadata"]["priority"], 1);
    }

    #[test]
    fn test_resource_type_preserved() {
        let mut kernel = Kernel::new();
        let states = vec![
            StateDefinition { name: "X".into(), status: StateStatus::Terminal },
        ];
        let sm = StateMachine::new(states, vec![], "X".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "special_type".into(), schema: serde_json::json!({}), state_machine: sm,
        }).unwrap();

        let r = match kernel.create_resource("special_type", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        assert_eq!(kernel.get_resource(&r.id).unwrap().resource_type, "special_type");
    }

    #[test]
    fn test_version_monotonically_increases() {
        let mut kernel = Kernel::new();
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Active },
            StateDefinition { name: "C".into(), status: StateStatus::Active },
            StateDefinition { name: "D".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "B".into(), to_state: "C".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "C".into(), to_state: "D".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "item".into(), schema: serde_json::json!({}), state_machine: sm,
        }).unwrap();

        let r = match kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        let mut prev_version = r.version;
        for state in ["B", "C", "D"] {
            kernel.transition(&r.id, state, "sys", "sys", AuthorityLevel::Controller).unwrap();
            let current = kernel.get_resource(&r.id).unwrap().version;
            assert!(current > prev_version, "Version should increase: {} > {}", current, prev_version);
            prev_version = current;
        }
    }

    #[test]
    fn test_timestamps_advance() {
        let mut kernel = Kernel::new();
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "item".into(), schema: serde_json::json!({}), state_machine: sm,
        }).unwrap();

        let r = match kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        let created_at = kernel.get_resource(&r.id).unwrap().created_at;
        kernel.transition(&r.id, "B", "sys", "sys", AuthorityLevel::Controller).unwrap();
        let updated_at = kernel.get_resource(&r.id).unwrap().updated_at;

        assert!(updated_at >= created_at);
    }
}
