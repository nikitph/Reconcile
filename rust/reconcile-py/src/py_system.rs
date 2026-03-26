use pyo3::prelude::*;
use reconcile_core::controller_scheduler::{ControllerHandler, ControllerRegistration};
use reconcile_core::errors::KernelError;
use reconcile_core::event_log::EventPattern;
use reconcile_core::invariant_checker::{
    InvariantCheck, InvariantDefinition, InvariantMode, InvariantScope, SystemQuery,
};
use reconcile_core::policy_engine::{PolicyDefinition, PolicyEvaluator};
use reconcile_core::resource_registry::ResourceTypeDefinition;
use reconcile_core::roles::{Permission, RoleDefinition};
use reconcile_core::state_machine::{StateDefinition, StateStatus, StateMachine, TransitionDefinition};
use reconcile_core::transaction::Kernel;
use reconcile_core::types::*;

use crate::py_types::*;

// ---------------------------------------------------------------------------
// Python callable bridges
// ---------------------------------------------------------------------------

struct PyPolicyBridge {
    callable: PyObject,
}

unsafe impl Send for PyPolicyBridge {}
unsafe impl Sync for PyPolicyBridge {}

impl PolicyEvaluator for PyPolicyBridge {
    fn evaluate(&self, resource: &Resource, context: &TransitionContext, query: &dyn SystemQuery) -> PolicyResult {
        Python::with_gil(|py| {
            let py_resource = PyResource {
                inner: resource.clone(),
            };
            let py_ctx = pyo3::types::PyDict::new(py);
            let _ = py_ctx.set_item("from_state", &context.from_state);
            let _ = py_ctx.set_item("to_state", &context.to_state);
            let _ = py_ctx.set_item("actor", &context.actor);
            let _ = py_ctx.set_item("role", &context.role);
            let _ = py_ctx.set_item("authority_level", context.authority_level.to_string());
            let py_query = PyQueryContext::new(query);

            match self.callable.call1(py, (py_resource, py_ctx.as_any(), py_query)) {
                Ok(result) => {
                    if result.is_none(py) {
                        // None return = no opinion = allow (explicit choice)
                        PolicyResult::allow()
                    } else if let Ok(pr) = result.extract::<PyPolicyResult>(py) {
                        pr.inner
                    } else if let Ok(b) = result.extract::<bool>(py) {
                        if b {
                            PolicyResult::allow()
                        } else {
                            PolicyResult::deny("Policy returned False")
                        }
                    } else {
                        // Unrecognized return type: fail-closed for safety
                        PolicyResult::deny(format!(
                            "Policy returned unrecognized type: {}. Return PolicyResult, bool, or None.",
                            result.bind(py).get_type().name().map(|n| n.to_string()).unwrap_or_else(|_| "unknown".to_string())
                        ))
                    }
                }
                Err(e) => PolicyResult::deny(format!("Policy callback error: {}", e)),
            }
        })
    }
}

struct PyInvariantBridge {
    callable: PyObject,
}

unsafe impl Send for PyInvariantBridge {}
unsafe impl Sync for PyInvariantBridge {}

impl InvariantCheck for PyInvariantBridge {
    fn check(&self, resource: &Resource, query: &dyn SystemQuery) -> InvariantResult {
        Python::with_gil(|py| {
            let py_resource = PyResource {
                inner: resource.clone(),
            };
            let py_query = PyQueryContext::new(query);

            match self.callable.call1(py, (py_resource, py_query)) {
                Ok(result) => {
                    if result.is_none(py) {
                        // None return = no opinion = holds
                        InvariantResult::ok()
                    } else if let Ok(ir) = result.extract::<PyInvariantResult>(py) {
                        ir.inner
                    } else if let Ok(b) = result.extract::<bool>(py) {
                        if b {
                            InvariantResult::ok()
                        } else {
                            InvariantResult::violated("Invariant returned False")
                        }
                    } else {
                        // Unrecognized return type: fail-closed for safety
                        InvariantResult::violated(format!(
                            "Invariant returned unrecognized type: {}. Return InvariantResult, bool, or None.",
                            result.bind(py).get_type().name().map(|n| n.to_string()).unwrap_or_else(|_| "unknown".to_string())
                        ))
                    }
                }
                Err(e) => InvariantResult::violated(format!("Invariant callback error: {}", e)),
            }
        })
    }
}

struct PyControllerBridge {
    callable: PyObject,
}

unsafe impl Send for PyControllerBridge {}
unsafe impl Sync for PyControllerBridge {}

impl ControllerHandler for PyControllerBridge {
    fn reconcile(
        &self,
        resource: &Resource,
        query: &dyn SystemQuery,
    ) -> Result<ControllerAction, KernelError> {
        Python::with_gil(|py| {
            let py_resource = PyResource {
                inner: resource.clone(),
            };
            let py_query = PyQueryContext::new(query);

            match self.callable.call1(py, (py_resource, py_query)) {
                Ok(result) => {
                    // Controller can return:
                    // - None / "noop" -> NoOp
                    // - A string -> Transition to that state
                    // - A dict {"transition": "STATE"} or {"set_desired": "STATE"}
                    if result.is_none(py) {
                        return Ok(ControllerAction::NoOp);
                    }
                    if let Ok(s) = result.extract::<String>(py) {
                        if s == "noop" || s.is_empty() {
                            return Ok(ControllerAction::NoOp);
                        }
                        return Ok(ControllerAction::Transition { to_state: s });
                    }
                    if let Ok(dict) = result.downcast_bound::<pyo3::types::PyDict>(py) {
                        if let Ok(Some(t)) = dict.get_item("transition") {
                            if let Ok(to) = t.extract::<String>() {
                                return Ok(ControllerAction::Transition { to_state: to });
                            }
                        }
                        if let Ok(Some(d)) = dict.get_item("set_desired") {
                            if let Ok(state) = d.extract::<String>() {
                                return Ok(ControllerAction::SetDesiredState { state });
                            }
                        }
                    }
                    Ok(ControllerAction::NoOp)
                }
                Err(e) => Err(KernelError::CallbackError(format!(
                    "Controller callback error: {}",
                    e
                ))),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// ReconcileSystem Python class
// ---------------------------------------------------------------------------

#[pyclass(name = "ReconcileSystem")]
pub struct PyReconcileSystem {
    kernel: Kernel,
}

#[pymethods]
impl PyReconcileSystem {
    #[new]
    #[pyo3(signature = (database_url=None, snapshot_interval=0))]
    fn new(database_url: Option<String>, snapshot_interval: u32) -> PyResult<Self> {
        let mut kernel = match database_url {
            Some(url) => {
                let backend = reconcile_postgres::PostgresBackend::connect(&url)
                    .map_err(|e| pyo3::exceptions::PyConnectionError::new_err(
                        format!("Failed to connect to PostgreSQL: {}", e)
                    ))?;
                Kernel::with_storage(Box::new(backend))
            }
            None => Kernel::new(),
        };
        kernel.snapshot_interval = snapshot_interval;
        Ok(Self { kernel })
    }

    /// Register a resource type with states and transitions.
    /// terminal_states: list of state names that are terminal (optional, default: empty)
    #[pyo3(signature = (name, states, transitions, initial_state, terminal_states=vec![]))]
    fn register_type(
        &mut self,
        name: String,
        states: Vec<String>,
        transitions: Vec<(String, String)>,
        initial_state: String,
        terminal_states: Vec<String>,
    ) -> PyResult<()> {
        let state_defs: Vec<StateDefinition> = states
            .iter()
            .map(|s| StateDefinition {
                name: s.clone(),
                status: if terminal_states.contains(s) {
                    StateStatus::Terminal
                } else {
                    StateStatus::Active
                },
            })
            .collect();

        let transition_defs: Vec<TransitionDefinition> = transitions
            .into_iter()
            .map(|(from, to)| TransitionDefinition {
                from_state: from,
                to_state: to,
                guard: None,
                required_role: None,
            })
            .collect();

        let sm = StateMachine::new(state_defs, transition_defs, initial_state)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;

        self.kernel
            .register_type(ResourceTypeDefinition {
                name,
                schema: serde_json::json!({}),
                state_machine: sm,
            })
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    /// Register a role with permissions (shorthand strings).
    fn register_role(&mut self, name: String, permissions: Vec<String>) -> PyResult<()> {
        let perms: Vec<Permission> = permissions
            .iter()
            .map(|p| Permission::from_shorthand(p))
            .collect();
        self.kernel.role_registry.register(RoleDefinition {
            name,
            permissions: perms,
        });
        Ok(())
    }

    /// Register a policy with a Python callable evaluator.
    #[pyo3(signature = (name, description, evaluator, applicable_states=vec![], resource_types=vec![], priority=50))]
    fn register_policy(
        &mut self,
        name: String,
        description: String,
        evaluator: PyObject,
        applicable_states: Vec<String>,
        resource_types: Vec<String>,
        priority: u32,
    ) -> PyResult<()> {
        self.kernel.policy_engine.register(PolicyDefinition {
            name,
            description,
            evaluator: Box::new(PyPolicyBridge { callable: evaluator }),
            applicable_states,
            resource_types,
            priority,
        });
        Ok(())
    }

    /// Register an invariant with a Python callable checker.
    #[pyo3(signature = (name, description, mode, scope, checker, resource_types=vec![]))]
    fn register_invariant(
        &mut self,
        name: String,
        description: String,
        mode: String,
        scope: String,
        checker: PyObject,
        resource_types: Vec<String>,
    ) -> PyResult<()> {
        let mode = match mode.to_lowercase().as_str() {
            "strong" => InvariantMode::Strong,
            "eventual" => InvariantMode::Eventual,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid invariant mode: {}. Use 'strong' or 'eventual'",
                    mode
                )))
            }
        };

        let scope = match scope.to_lowercase().as_str() {
            "resource" => InvariantScope::Resource,
            "transition" => InvariantScope::Transition,
            "cross_resource" => InvariantScope::CrossResource,
            "system" => InvariantScope::System,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid invariant scope: {}",
                    scope
                )))
            }
        };

        self.kernel
            .invariant_checker
            .register(InvariantDefinition {
                name,
                description,
                mode,
                scope,
                resource_types,
                checker: Box::new(PyInvariantBridge { callable: checker }),
            });
        Ok(())
    }

    /// Register a reactive controller with a Python callable handler.
    #[pyo3(signature = (name, handler, on_events=vec![], priority=50, enforces=vec![], authority_level="CONTROLLER".to_string()))]
    fn register_controller(
        &mut self,
        name: String,
        handler: PyObject,
        on_events: Vec<String>,
        priority: u32,
        enforces: Vec<String>,
        authority_level: String,
    ) -> PyResult<()> {
        let authority = AuthorityLevel::from_str(&authority_level).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid authority level: {}",
                authority_level
            ))
        })?;

        let patterns: Vec<EventPattern> = on_events
            .iter()
            .map(|e| EventPattern::parse(e))
            .collect();

        self.kernel
            .controller_scheduler
            .register(ControllerRegistration {
                name,
                priority,
                enforces,
                on_events: patterns,
                authority_level: authority,
                handler: Box::new(PyControllerBridge { callable: handler }),
            });
        Ok(())
    }

    /// Create a resource.
    #[pyo3(signature = (resource_type, data, actor="system".to_string(), authority_level="HUMAN".to_string()))]
    fn create(
        &mut self,
        py: Python<'_>,
        resource_type: String,
        data: &Bound<'_, pyo3::PyAny>,
        actor: String,
        authority_level: String,
    ) -> PyResult<PyTransitionResult> {
        let json_data = py_to_json(py, data)?;
        let authority = AuthorityLevel::from_str(&authority_level).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid authority level: {}",
                authority_level
            ))
        })?;

        let outcome = self
            .kernel
            .create_resource(&resource_type, json_data, &actor, authority)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        Ok(PyTransitionResult::from_outcome(outcome))
    }

    /// Request a state transition.
    #[pyo3(signature = (resource_id, to_state, actor="system".to_string(), role="system".to_string(), authority_level="HUMAN".to_string()))]
    fn transition(
        &mut self,
        resource_id: String,
        to_state: String,
        actor: String,
        role: String,
        authority_level: String,
    ) -> PyResult<PyTransitionResult> {
        let rid = parse_resource_id(&resource_id)?;
        let authority = AuthorityLevel::from_str(&authority_level).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid authority level: {}",
                authority_level
            ))
        })?;

        let outcome = self
            .kernel
            .transition(&rid, &to_state, &actor, &role, authority)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        Ok(PyTransitionResult::from_outcome(outcome))
    }

    /// Set desired state (triggers reconciliation).
    #[pyo3(signature = (resource_id, desired_state, requested_by="system".to_string(), authority_level="HUMAN".to_string()))]
    fn set_desired(
        &mut self,
        resource_id: String,
        desired_state: String,
        requested_by: String,
        authority_level: String,
    ) -> PyResult<()> {
        let rid = parse_resource_id(&resource_id)?;
        let authority = AuthorityLevel::from_str(&authority_level).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid authority level: {}",
                authority_level
            ))
        })?;

        self.kernel
            .set_desired_state(&rid, &desired_state, &requested_by, authority)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Get a resource by ID.
    fn get(&self, resource_id: String) -> PyResult<Option<PyResource>> {
        let rid = parse_resource_id(&resource_id)?;
        Ok(self
            .kernel
            .get_resource(&rid)
            .map(|r| PyResource { inner: r }))
    }

    /// Get events for a resource.
    fn events(&self, resource_id: String) -> PyResult<Vec<PyEvent>> {
        let rid = parse_resource_id(&resource_id)?;
        Ok(self
            .kernel
            .get_events(&rid)
            .into_iter()
            .map(|e| PyEvent { inner: e })
            .collect())
    }

    /// Get audit trail for a resource.
    fn audit(&self, resource_id: String) -> PyResult<Vec<PyAuditRecord>> {
        let rid = parse_resource_id(&resource_id)?;
        Ok(self
            .kernel
            .get_audit(&rid)
            .into_iter()
            .map(|a| PyAuditRecord { inner: a })
            .collect())
    }

    /// List all resources of a type.
    fn list_resources(&self, resource_type: String) -> Vec<PyResource> {
        self.kernel
            .list_resources(&resource_type)
            .into_iter()
            .map(|r| PyResource { inner: r })
            .collect()
    }

    // --- Schema graph ---

    /// Register a relationship between resource types.
    #[pyo3(signature = (from_type, to_type, relation, cardinality="many_to_one".to_string(), required=false, foreign_key="".to_string()))]
    fn register_relationship(
        &mut self,
        from_type: String,
        to_type: String,
        relation: String,
        cardinality: String,
        required: bool,
        foreign_key: String,
    ) -> PyResult<()> {
        use reconcile_core::schema_graph::{Cardinality, RelationshipDeclaration};
        let card = match cardinality.as_str() {
            "one_to_one" => Cardinality::OneToOne,
            "one_to_many" => Cardinality::OneToMany,
            "many_to_one" => Cardinality::ManyToOne,
            "many_to_many" => Cardinality::ManyToMany,
            _ => return Err(pyo3::exceptions::PyValueError::new_err(
                format!("Invalid cardinality: {}. Use one_to_one, one_to_many, many_to_one, many_to_many", cardinality)
            )),
        };
        let fk = if foreign_key.is_empty() {
            format!("{}_id", to_type)
        } else {
            foreign_key
        };
        self.kernel.schema_graph.add_relationship(RelationshipDeclaration {
            from_type, to_type, relation, cardinality: card, required, foreign_key: fk,
        });
        Ok(())
    }

    // --- Instance graph queries ---

    /// Get neighbor resources via graph edges.
    #[pyo3(signature = (resource_id, edge_type=None))]
    fn graph_neighbors(&self, resource_id: String, edge_type: Option<String>) -> PyResult<Vec<PyResource>> {
        let rid = parse_resource_id(&resource_id)?;
        let neighbors = self.kernel.instance_graph.neighbors(&rid, edge_type.as_deref());
        Ok(neighbors.into_iter().map(|n| PyResource {
            inner: Resource {
                id: n.id, resource_type: n.resource_type, state: n.state,
                desired_state: None, data: n.data, version: n.version,
                tenant_id: None,
                created_at: chrono::Utc::now(), updated_at: chrono::Utc::now(),
            }
        }).collect())
    }

    /// Aggregate a field across graph neighbors.
    #[pyo3(signature = (resource_id, edge_type, field, agg_fn="SUM".to_string()))]
    fn graph_aggregate(
        &self, resource_id: String, edge_type: String, field: String, agg_fn: String,
    ) -> PyResult<PyObject> {
        let rid = parse_resource_id(&resource_id)?;
        let agg = reconcile_core::instance_graph::AggFn::from_str(&agg_fn)
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(
                format!("Invalid agg function: {}. Use SUM, AVG, MIN, MAX, COUNT", agg_fn)
            ))?;
        let result = self.kernel.instance_graph.aggregate(&rid, &edge_type, &field, agg);
        Python::with_gil(|py| crate::py_types::json_to_py(py, &result))
    }

    /// Get the degree (connection count) of a resource in the instance graph.
    #[pyo3(signature = (resource_id, edge_type=None))]
    fn graph_degree(&self, resource_id: String, edge_type: Option<String>) -> PyResult<usize> {
        let rid = parse_resource_id(&resource_id)?;
        Ok(self.kernel.instance_graph.degree(&rid, edge_type.as_deref()))
    }

    // --- Agents ---

    /// Register an agent with a Python callable handler.
    #[pyo3(signature = (name, handler, on_events=vec![], priority=50))]
    fn register_agent(
        &mut self,
        name: String,
        handler: PyObject,
        on_events: Vec<String>,
        priority: u32,
    ) -> PyResult<()> {
        use reconcile_core::agent::{AgentRegistration, AgentHandler};
        use reconcile_core::event_log::EventPattern;

        struct PyAgentBridge { callable: PyObject }
        unsafe impl Send for PyAgentBridge {}
        unsafe impl Sync for PyAgentBridge {}

        impl AgentHandler for PyAgentBridge {
            fn observe(&self, resource: &Resource, query: &dyn SystemQuery) -> Option<Proposal> {
                Python::with_gil(|py| {
                    let py_resource = PyResource { inner: resource.clone() };
                    let py_query = PyQueryContext::new(query);

                    match self.callable.call1(py, (py_resource, py_query)) {
                        Ok(result) => {
                            if result.is_none(py) {
                                return None;
                            }
                            // Expect a dict with: action, confidence, reasoning
                            if let Ok(dict) = result.downcast_bound::<pyo3::types::PyDict>(py) {
                                let confidence = dict.get_item("confidence").ok()
                                    .flatten()
                                    .and_then(|v| v.extract::<f64>().ok())
                                    .unwrap_or(0.5);
                                let reasoning = dict.get_item("reasoning").ok()
                                    .flatten()
                                    .and_then(|v| v.extract::<String>().ok())
                                    .unwrap_or_default();

                                let action = if let Ok(Some(t)) = dict.get_item("transition") {
                                    if let Ok(s) = t.extract::<String>() {
                                        ProposedAction::Transition { to_state: s }
                                    } else {
                                        return None;
                                    }
                                } else if let Ok(Some(f)) = dict.get_item("flag") {
                                    if let Ok(s) = f.extract::<String>() {
                                        ProposedAction::Flag { reason: s }
                                    } else {
                                        return None;
                                    }
                                } else {
                                    return None;
                                };

                                Some(Proposal {
                                    id: uuid::Uuid::new_v4(),
                                    agent: String::new(), // Filled by scheduler
                                    action,
                                    resource_id: resource.id.clone(),
                                    confidence,
                                    reasoning,
                                    timestamp: chrono::Utc::now(),
                                })
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    }
                })
            }
        }

        let patterns: Vec<EventPattern> = on_events.iter().map(|e| EventPattern::parse(e)).collect();

        self.kernel.agent_scheduler.register(AgentRegistration {
            name,
            priority,
            on_events: patterns,
            handler: Box::new(PyAgentBridge { callable: handler }),
        });
        Ok(())
    }

    // --- Decision Nodes ---

    /// Register a decision node.
    #[pyo3(signature = (name, agents, aggregation="weighted_avg".to_string(), auto_accept=0.9, auto_reject=0.5))]
    fn register_decision_node(
        &mut self,
        name: String,
        agents: Vec<String>,
        aggregation: String,
        auto_accept: f64,
        auto_reject: f64,
    ) -> PyResult<()> {
        use reconcile_core::decision::{AggregationStrategy, DecisionNode};

        let strategy = match aggregation.as_str() {
            "majority" => AggregationStrategy::Majority,
            "weighted_avg" => AggregationStrategy::WeightedAvg,
            "unanimous" => AggregationStrategy::Unanimous,
            "min_confidence" => AggregationStrategy::MinConfidence,
            _ => return Err(pyo3::exceptions::PyValueError::new_err(
                format!("Invalid aggregation: {}. Use majority, weighted_avg, unanimous, min_confidence", aggregation)
            )),
        };

        let mut node = DecisionNode::new(name, agents);
        node.aggregation = strategy;
        node.auto_accept_threshold = auto_accept;
        node.auto_reject_threshold = auto_reject;
        self.kernel.decision_nodes.push(node);
        Ok(())
    }
}

fn parse_resource_id(s: &str) -> PyResult<ResourceId> {
    let uuid = uuid::Uuid::parse_str(s)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid UUID: {}", e)))?;
    Ok(ResourceId(uuid))
}
