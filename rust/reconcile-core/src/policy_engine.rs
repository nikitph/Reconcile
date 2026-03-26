use crate::invariant_checker::SystemQuery;
use crate::types::{PolicyEvaluation, PolicyResult, Resource, TransitionContext};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Policy evaluator trait (bridged from Python)
// ---------------------------------------------------------------------------

pub trait PolicyEvaluator: Send + Sync {
    fn evaluate(&self, resource: &Resource, context: &TransitionContext, query: &dyn SystemQuery) -> PolicyResult;
}

// ---------------------------------------------------------------------------
// Policy definition
// ---------------------------------------------------------------------------

pub struct PolicyDefinition {
    pub name: String,
    pub description: String,
    pub evaluator: Box<dyn PolicyEvaluator>,
    pub applicable_states: Vec<String>,
    pub resource_types: Vec<String>,
    pub priority: u32,
}

impl std::fmt::Debug for PolicyDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyDefinition")
            .field("name", &self.name)
            .field("priority", &self.priority)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Policy engine with indexing
// ---------------------------------------------------------------------------

pub struct PolicyEngine {
    policies: Vec<PolicyDefinition>,
    /// Index: (resource_type, state) -> [policy indices], sorted by priority desc
    index: HashMap<(String, String), Vec<usize>>,
}

impl PolicyEngine {
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn register(&mut self, policy: PolicyDefinition) {
        self.policies.push(policy);
        self.rebuild_index();
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, policy) in self.policies.iter().enumerate() {
            let types: Vec<String> = if policy.resource_types.is_empty() {
                vec!["*".to_string()]
            } else {
                policy.resource_types.clone()
            };

            let states: Vec<String> = if policy.applicable_states.is_empty() {
                vec!["*".to_string()]
            } else {
                policy.applicable_states.clone()
            };

            for t in &types {
                for s in &states {
                    self.index
                        .entry((t.clone(), s.clone()))
                        .or_default()
                        .push(i);
                }
            }
        }

        // Sort each index bucket by priority descending
        for bucket in self.index.values_mut() {
            bucket.sort_by(|a, b| self.policies[*b].priority.cmp(&self.policies[*a].priority));
        }
    }

    /// Get applicable policies for a given resource type and state.
    fn get_applicable(&self, resource_type: &str, state: &str) -> Vec<&PolicyDefinition> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        // Check exact (type, state), (type, *), (*, state), (*, *)
        let keys = [
            (resource_type.to_string(), state.to_string()),
            (resource_type.to_string(), "*".to_string()),
            ("*".to_string(), state.to_string()),
            ("*".to_string(), "*".to_string()),
        ];

        for key in &keys {
            if let Some(indices) = self.index.get(key) {
                for &i in indices {
                    if seen.insert(i) {
                        result.push(&self.policies[i]);
                    }
                }
            }
        }

        // Sort by priority descending
        result.sort_by(|a, b| b.priority.cmp(&a.priority));
        result
    }

    /// Evaluate all applicable policies. Returns evaluations for every policy.
    pub fn evaluate_all(
        &self,
        resource: &Resource,
        context: &TransitionContext,
        query: &dyn SystemQuery,
    ) -> Vec<PolicyEvaluation> {
        let applicable = self.get_applicable(&resource.resource_type, &resource.state);
        applicable
            .iter()
            .map(|policy| {
                let result = policy.evaluator.evaluate(resource, context, query);
                PolicyEvaluation {
                    name: policy.name.clone(),
                    passed: result.passed,
                    message: result.message,
                }
            })
            .collect()
    }

    /// Returns the first denied policy evaluation, or None if all pass.
    pub fn first_denied(
        &self,
        resource: &Resource,
        context: &TransitionContext,
        query: &dyn SystemQuery,
    ) -> Option<PolicyEvaluation> {
        let applicable = self.get_applicable(&resource.resource_type, &resource.state);
        for policy in applicable {
            let result = policy.evaluator.evaluate(resource, context, query);
            if !result.passed {
                return Some(PolicyEvaluation {
                    name: policy.name.clone(),
                    passed: false,
                    message: result.message,
                });
            }
        }
        None
    }

    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AuthorityLevel, Resource, ResourceId};
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

    struct DenyAll(String);
    impl PolicyEvaluator for DenyAll {
        fn evaluate(&self, _r: &Resource, _c: &TransitionContext, _q: &dyn SystemQuery) -> PolicyResult {
            PolicyResult::deny(&self.0)
        }
    }

    struct AmountLimit(f64);
    impl PolicyEvaluator for AmountLimit {
        fn evaluate(&self, r: &Resource, _c: &TransitionContext, _q: &dyn SystemQuery) -> PolicyResult {
            let amount = r.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if amount > self.0 {
                PolicyResult::deny(format!("Amount {} exceeds limit {}", amount, self.0))
            } else {
                PolicyResult::allow()
            }
        }
    }

    fn make_resource(data: serde_json::Value) -> Resource {
        Resource {
            id: ResourceId::new(),
            resource_type: "loan".into(),
            state: "UNDERWRITING".into(),
            desired_state: None,
            data,
            version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_context(resource: &Resource) -> TransitionContext {
        TransitionContext {
            resource_id: resource.id.clone(),
            resource_type: "loan".into(),
            from_state: "UNDERWRITING".into(),
            to_state: "APPROVED".into(),
            actor: "user1".into(),
            role: "officer".into(),
            authority_level: AuthorityLevel::Human,
        }
    }

    #[test]
    fn test_all_policies_pass() {
        let mut engine = PolicyEngine::new();
        engine.register(PolicyDefinition {
            name: "always_allow".into(),
            description: "test".into(),
            evaluator: Box::new(AllowAll),
            applicable_states: vec![],
            resource_types: vec![],
            priority: 10,
        });

        let r = make_resource(serde_json::json!({}));
        let ctx = make_context(&r);
        let evals = engine.evaluate_all(&r, &ctx, &EmptyQuery);
        assert_eq!(evals.len(), 1);
        assert!(evals[0].passed);
    }

    #[test]
    fn test_policy_denied() {
        let mut engine = PolicyEngine::new();
        engine.register(PolicyDefinition {
            name: "deny_all".into(),
            description: "test".into(),
            evaluator: Box::new(DenyAll("nope".into())),
            applicable_states: vec![],
            resource_types: vec![],
            priority: 10,
        });

        let r = make_resource(serde_json::json!({}));
        let ctx = make_context(&r);
        let denied = engine.first_denied(&r, &ctx, &EmptyQuery);
        assert!(denied.is_some());
        assert_eq!(denied.unwrap().name, "deny_all");
    }

    #[test]
    fn test_amount_limit_policy() {
        let mut engine = PolicyEngine::new();
        engine.register(PolicyDefinition {
            name: "amount_limit".into(),
            description: "Loans > 50L need committee".into(),
            evaluator: Box::new(AmountLimit(5_000_000.0)),
            applicable_states: vec!["UNDERWRITING".into()],
            resource_types: vec!["loan".into()],
            priority: 50,
        });

        let small = make_resource(serde_json::json!({"amount": 1_000_000}));
        let ctx_small = make_context(&small);
        assert!(engine.first_denied(&small, &ctx_small, &EmptyQuery).is_none());

        let big = make_resource(serde_json::json!({"amount": 10_000_000}));
        let ctx_big = make_context(&big);
        assert!(engine.first_denied(&big, &ctx_big, &EmptyQuery).is_some());
    }

    #[test]
    fn test_state_scoped_policy() {
        let mut engine = PolicyEngine::new();
        engine.register(PolicyDefinition {
            name: "uw_only".into(),
            description: "test".into(),
            evaluator: Box::new(DenyAll("blocked".into())),
            applicable_states: vec!["UNDERWRITING".into()],
            resource_types: vec!["loan".into()],
            priority: 10,
        });

        // Resource in UNDERWRITING -> policy applies
        let r1 = make_resource(serde_json::json!({}));
        let ctx1 = make_context(&r1);
        assert!(engine.first_denied(&r1, &ctx1, &EmptyQuery).is_some());

        // Resource in APPLIED -> policy does not apply
        let mut r2 = make_resource(serde_json::json!({}));
        r2.state = "APPLIED".into();
        let ctx2 = TransitionContext {
            resource_id: r2.id.clone(),
            resource_type: "loan".into(),
            from_state: "APPLIED".into(),
            to_state: "UNDERWRITING".into(),
            actor: "user1".into(),
            role: "officer".into(),
            authority_level: AuthorityLevel::Human,
        };
        assert!(engine.first_denied(&r2, &ctx2, &EmptyQuery).is_none());
    }

    #[test]
    fn test_priority_ordering() {
        let mut engine = PolicyEngine::new();
        engine.register(PolicyDefinition {
            name: "low_priority".into(),
            description: "test".into(),
            evaluator: Box::new(AllowAll),
            applicable_states: vec![],
            resource_types: vec![],
            priority: 10,
        });
        engine.register(PolicyDefinition {
            name: "high_priority".into(),
            description: "test".into(),
            evaluator: Box::new(DenyAll("high prio deny".into())),
            applicable_states: vec![],
            resource_types: vec![],
            priority: 90,
        });

        let r = make_resource(serde_json::json!({}));
        let ctx = make_context(&r);
        let evals = engine.evaluate_all(&r, &ctx, &EmptyQuery);
        // High priority should be first
        assert_eq!(evals[0].name, "high_priority");
        assert_eq!(evals[1].name, "low_priority");
    }
}
