use crate::types::{InvariantEvaluation, InvariantResult, Resource};

// ---------------------------------------------------------------------------
// Invariant modes and scopes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvariantMode {
    Strong,   // Checked synchronously inside transaction
    Eventual, // Checked asynchronously by controllers
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvariantScope {
    Resource,
    Transition,
    CrossResource,
    System,
}

// ---------------------------------------------------------------------------
// Invariant check trait (bridged from Python)
// ---------------------------------------------------------------------------

/// Read-only query interface passed to invariant checks for cross-resource access.
pub trait SystemQuery: Send + Sync {
    fn get_resource(&self, id: &crate::types::ResourceId) -> Option<Resource>;
    fn list_by_type(&self, resource_type: &str) -> Vec<Resource>;

    // Graph queries — default impls return empty for backends without graph support.
    fn graph_neighbors(&self, _id: &crate::types::ResourceId, _edge_type: Option<&str>) -> Vec<Resource> { vec![] }
    fn graph_aggregate(&self, _id: &crate::types::ResourceId, _edge_type: &str, _field: &str, _agg_fn: &str) -> serde_json::Value { serde_json::Value::Null }
    fn graph_degree(&self, _id: &crate::types::ResourceId, _edge_type: Option<&str>) -> usize { 0 }
    fn graph_has_cycle(&self, _id: &crate::types::ResourceId) -> bool { false }
}

pub trait InvariantCheck: Send + Sync {
    fn check(&self, resource: &Resource, query: &dyn SystemQuery) -> InvariantResult;
}

// ---------------------------------------------------------------------------
// Invariant definition
// ---------------------------------------------------------------------------

pub struct InvariantDefinition {
    pub name: String,
    pub description: String,
    pub mode: InvariantMode,
    pub scope: InvariantScope,
    pub resource_types: Vec<String>,
    pub checker: Box<dyn InvariantCheck>,
}

impl std::fmt::Debug for InvariantDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvariantDefinition")
            .field("name", &self.name)
            .field("mode", &self.mode)
            .field("scope", &self.scope)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Invariant checker
// ---------------------------------------------------------------------------

pub struct InvariantChecker {
    invariants: Vec<InvariantDefinition>,
}

impl InvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: Vec::new(),
        }
    }

    pub fn register(&mut self, invariant: InvariantDefinition) {
        self.invariants.push(invariant);
    }

    fn applicable(&self, resource: &Resource, mode: InvariantMode) -> Vec<&InvariantDefinition> {
        self.invariants
            .iter()
            .filter(|inv| inv.mode == mode)
            .filter(|inv| {
                inv.resource_types.is_empty()
                    || inv.resource_types.iter().any(|t| t == &resource.resource_type)
            })
            .collect()
    }

    /// Check all strong invariants. Returns evaluations.
    pub fn check_strong(
        &self,
        resource: &Resource,
        query: &dyn SystemQuery,
    ) -> Vec<InvariantEvaluation> {
        self.applicable(resource, InvariantMode::Strong)
            .iter()
            .map(|inv| {
                let result = inv.checker.check(resource, query);
                InvariantEvaluation {
                    name: inv.name.clone(),
                    holds: result.holds,
                    violation: result.violation,
                }
            })
            .collect()
    }

    /// Check all eventual invariants. Returns evaluations.
    pub fn check_eventual(
        &self,
        resource: &Resource,
        query: &dyn SystemQuery,
    ) -> Vec<InvariantEvaluation> {
        self.applicable(resource, InvariantMode::Eventual)
            .iter()
            .map(|inv| {
                let result = inv.checker.check(resource, query);
                InvariantEvaluation {
                    name: inv.name.clone(),
                    holds: result.holds,
                    violation: result.violation,
                }
            })
            .collect()
    }

    /// Returns first violated strong invariant, or None.
    pub fn first_strong_violation(
        &self,
        resource: &Resource,
        query: &dyn SystemQuery,
    ) -> Option<InvariantEvaluation> {
        self.check_strong(resource, query)
            .into_iter()
            .find(|e| !e.holds)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&InvariantDefinition> {
        self.invariants.iter().find(|inv| inv.name == name)
    }

    pub fn invariant_count(&self) -> usize {
        self.invariants.len()
    }
}

impl Default for InvariantChecker {
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
    use crate::types::ResourceId;
    use chrono::Utc;

    /// Stub query that returns nothing
    struct EmptyQuery;
    impl SystemQuery for EmptyQuery {
        fn get_resource(&self, _id: &ResourceId) -> Option<Resource> {
            None
        }
        fn list_by_type(&self, _resource_type: &str) -> Vec<Resource> {
            vec![]
        }
    }

    struct PositiveAmount;
    impl InvariantCheck for PositiveAmount {
        fn check(&self, resource: &Resource, _q: &dyn SystemQuery) -> InvariantResult {
            let amount = resource.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if amount > 0.0 {
                InvariantResult::ok()
            } else {
                InvariantResult::violated("Amount must be positive")
            }
        }
    }

    struct AlwaysHolds;
    impl InvariantCheck for AlwaysHolds {
        fn check(&self, _r: &Resource, _q: &dyn SystemQuery) -> InvariantResult {
            InvariantResult::ok()
        }
    }

    fn make_resource(data: serde_json::Value) -> Resource {
        Resource {
            id: ResourceId::new(),
            resource_type: "loan".into(),
            state: "APPLIED".into(),
            desired_state: None,
            data,
            version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_strong_invariant_holds() {
        let mut checker = InvariantChecker::new();
        checker.register(InvariantDefinition {
            name: "positive_amount".into(),
            description: "Amount must be > 0".into(),
            mode: InvariantMode::Strong,
            scope: InvariantScope::Resource,
            resource_types: vec!["loan".into()],
            checker: Box::new(PositiveAmount),
        });

        let r = make_resource(serde_json::json!({"amount": 100}));
        let evals = checker.check_strong(&r, &EmptyQuery);
        assert_eq!(evals.len(), 1);
        assert!(evals[0].holds);
    }

    #[test]
    fn test_strong_invariant_violated() {
        let mut checker = InvariantChecker::new();
        checker.register(InvariantDefinition {
            name: "positive_amount".into(),
            description: "Amount must be > 0".into(),
            mode: InvariantMode::Strong,
            scope: InvariantScope::Resource,
            resource_types: vec!["loan".into()],
            checker: Box::new(PositiveAmount),
        });

        let r = make_resource(serde_json::json!({"amount": -5}));
        let violation = checker.first_strong_violation(&r, &EmptyQuery);
        assert!(violation.is_some());
        assert_eq!(violation.unwrap().name, "positive_amount");
    }

    #[test]
    fn test_eventual_invariant_separate() {
        let mut checker = InvariantChecker::new();
        checker.register(InvariantDefinition {
            name: "strong_one".into(),
            description: "test".into(),
            mode: InvariantMode::Strong,
            scope: InvariantScope::Resource,
            resource_types: vec![],
            checker: Box::new(AlwaysHolds),
        });
        checker.register(InvariantDefinition {
            name: "eventual_one".into(),
            description: "test".into(),
            mode: InvariantMode::Eventual,
            scope: InvariantScope::CrossResource,
            resource_types: vec![],
            checker: Box::new(AlwaysHolds),
        });

        let r = make_resource(serde_json::json!({}));
        assert_eq!(checker.check_strong(&r, &EmptyQuery).len(), 1);
        assert_eq!(checker.check_eventual(&r, &EmptyQuery).len(), 1);
    }

    #[test]
    fn test_resource_type_filtering() {
        let mut checker = InvariantChecker::new();
        checker.register(InvariantDefinition {
            name: "loan_only".into(),
            description: "test".into(),
            mode: InvariantMode::Strong,
            scope: InvariantScope::Resource,
            resource_types: vec!["loan".into()],
            checker: Box::new(AlwaysHolds),
        });

        let loan = make_resource(serde_json::json!({}));
        assert_eq!(checker.check_strong(&loan, &EmptyQuery).len(), 1);

        let mut app = make_resource(serde_json::json!({}));
        app.resource_type = "application".into();
        assert_eq!(checker.check_strong(&app, &EmptyQuery).len(), 0);
    }

    #[test]
    fn test_get_by_name() {
        let mut checker = InvariantChecker::new();
        checker.register(InvariantDefinition {
            name: "test_inv".into(),
            description: "test".into(),
            mode: InvariantMode::Strong,
            scope: InvariantScope::Resource,
            resource_types: vec![],
            checker: Box::new(AlwaysHolds),
        });

        assert!(checker.get_by_name("test_inv").is_some());
        assert!(checker.get_by_name("bogus").is_none());
    }
}
