use crate::agent::AgentScheduler;
use crate::circuit_breaker::CircuitBreaker;
use crate::controller_scheduler::ControllerScheduler;
use crate::decision::DecisionNode;
use crate::errors::KernelError;
use crate::instance_graph::{AggFn, GraphEdge, GraphNode, InMemoryInstanceGraph, InstanceGraph};
use crate::invariant_checker::{InvariantChecker, SystemQuery};
use crate::policy_engine::PolicyEngine;
use crate::resource_registry::{ResourceRegistry, ResourceTypeDefinition};
use crate::roles::RoleRegistry;
use crate::schema_graph::SchemaGraph;
use crate::storage::{InMemoryBackend, StorageBackend};
use crate::temporal_graph::{CausalEdge, InMemoryTemporalGraph, TemporalGraph};
use crate::types::*;
use chrono::Utc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Kernel: assembles all 8 subsystems
// ---------------------------------------------------------------------------

pub struct Kernel {
    pub registry: ResourceRegistry,
    pub storage: Box<dyn StorageBackend>,
    pub policy_engine: PolicyEngine,
    pub invariant_checker: InvariantChecker,
    pub role_registry: RoleRegistry,
    pub controller_scheduler: ControllerScheduler,
    pub agent_scheduler: AgentScheduler,
    pub circuit_breaker: CircuitBreaker,
    pub decision_nodes: Vec<DecisionNode>,
    pub schema_graph: SchemaGraph,
    pub instance_graph: Box<dyn InstanceGraph>,
    pub temporal_graph: Box<dyn TemporalGraph>,
    cascade_depth: u32,
    pub snapshot_interval: u32,
    /// Proposals collected during the current event processing cycle.
    pending_proposals: Vec<Proposal>,
}

/// Read-only view of the kernel for invariant checks and controllers.
struct KernelQuery<'a> {
    storage: &'a dyn StorageBackend,
    instance_graph: &'a dyn InstanceGraph,
}

impl<'a> SystemQuery for KernelQuery<'a> {
    fn get_resource(&self, id: &ResourceId) -> Option<Resource> {
        self.storage.state_store().get(id)
    }

    fn list_by_type(&self, resource_type: &str) -> Vec<Resource> {
        self.storage.state_store().list_by_type(resource_type)
    }

    fn graph_neighbors(&self, id: &ResourceId, edge_type: Option<&str>) -> Vec<Resource> {
        self.instance_graph.neighbors(id, edge_type)
            .into_iter()
            .map(|n| Resource {
                id: n.id, resource_type: n.resource_type, state: n.state,
                desired_state: None, data: n.data, version: n.version,
                tenant_id: None,
                created_at: Utc::now(), updated_at: Utc::now(),
            })
            .collect()
    }

    fn graph_aggregate(&self, id: &ResourceId, edge_type: &str, field: &str, agg_fn: &str) -> serde_json::Value {
        AggFn::from_str(agg_fn)
            .map(|f| self.instance_graph.aggregate(id, edge_type, field, f))
            .unwrap_or(serde_json::Value::Null)
    }

    fn graph_degree(&self, id: &ResourceId, edge_type: Option<&str>) -> usize {
        self.instance_graph.degree(id, edge_type)
    }

    fn graph_has_cycle(&self, id: &ResourceId) -> bool {
        self.instance_graph.has_cycle(id)
    }
}

impl Kernel {
    pub fn new() -> Self {
        Self {
            registry: ResourceRegistry::new(),
            storage: Box::new(InMemoryBackend::new()),
            policy_engine: PolicyEngine::new(),
            invariant_checker: InvariantChecker::new(),
            role_registry: RoleRegistry::new(),
            controller_scheduler: ControllerScheduler::new(),
            agent_scheduler: AgentScheduler::new(),
            circuit_breaker: CircuitBreaker::default(),
            decision_nodes: Vec::new(),
            schema_graph: SchemaGraph::new(),
            instance_graph: Box::new(InMemoryInstanceGraph::new()),
            temporal_graph: Box::new(InMemoryTemporalGraph::new()),
            cascade_depth: 0,
            snapshot_interval: 0,
            pending_proposals: Vec::new(),
        }
    }

    /// Create a Kernel with a custom storage backend.
    pub fn with_storage(storage: Box<dyn StorageBackend>) -> Self {
        Self {
            registry: ResourceRegistry::new(),
            storage,
            policy_engine: PolicyEngine::new(),
            invariant_checker: InvariantChecker::new(),
            role_registry: RoleRegistry::new(),
            controller_scheduler: ControllerScheduler::new(),
            agent_scheduler: AgentScheduler::new(),
            circuit_breaker: CircuitBreaker::default(),
            decision_nodes: Vec::new(),
            schema_graph: SchemaGraph::new(),
            instance_graph: Box::new(InMemoryInstanceGraph::new()),
            temporal_graph: Box::new(InMemoryTemporalGraph::new()),
            cascade_depth: 0,
            snapshot_interval: 0,
            pending_proposals: Vec::new(),
        }
    }

    /// Register a resource type with its state machine.
    pub fn register_type(&mut self, def: ResourceTypeDefinition) -> Result<(), KernelError> {
        self.registry.register(def)
    }

    /// Create a new resource in its initial state.
    pub fn create_resource(
        &mut self,
        resource_type: &str,
        data: serde_json::Value,
        actor: &str,
        authority_level: AuthorityLevel,
    ) -> Result<TransitionOutcome, KernelError> {
        let type_def = self
            .registry
            .get(resource_type)
            .ok_or_else(|| KernelError::TypeNotRegistered(resource_type.to_string()))?;

        let initial_state = type_def.state_machine.initial_state().to_string();
        let id = ResourceId::new();
        let now = Utc::now();

        let resource = Resource {
            id: id.clone(),
            resource_type: resource_type.to_string(),
            state: initial_state.clone(),
            desired_state: None,
            data,
            version: 1,
            tenant_id: None,
            created_at: now,
            updated_at: now,
        };

        // Check strong invariants on the new resource
        let query = KernelQuery {
            storage: self.storage.as_ref(),
            instance_graph: self.instance_graph.as_ref(),
        };
        if let Some(violation) = self.invariant_checker.first_strong_violation(&resource, &query) {
            return Ok(TransitionOutcome::Rejected {
                step: "verify_invariants".into(),
                reason: format!(
                    "Invariant '{}' violated: {}",
                    violation.name,
                    violation.violation.unwrap_or_default()
                ),
                details: serde_json::Value::Null,
            });
        }

        // Create the resource
        self.storage.state_store_mut().create(resource.clone())?;

        // Post-commit: add to instance graph
        self.instance_graph.add_node(GraphNode {
            id: id.clone(),
            resource_type: resource_type.to_string(),
            state: initial_state.clone(),
            data: resource.data.clone(),
            version: 1,
        });
        // Add edges based on foreign keys in schema graph
        for rel in self.schema_graph.outbound_relations(resource_type) {
            if let Some(ref_id_str) = resource.data.get(&rel.foreign_key).and_then(|v| v.as_str()) {
                if let Ok(ref_uuid) = uuid::Uuid::parse_str(ref_id_str) {
                    self.instance_graph.add_edge(GraphEdge {
                        from_id: id.clone(),
                        to_id: ResourceId(ref_uuid),
                        relation: rel.relation.clone(),
                        metadata: serde_json::json!({}),
                    });
                }
            }
        }

        // Emit creation event
        let event = Event {
            id: Uuid::new_v4(),
            offset: 0,
            event_type: format!("{}.created", resource_type),
            resource_id: id.clone(),
            payload: serde_json::json!({
                "state": initial_state,
            }),
            actor: actor.to_string(),
            authority_level,
            timestamp: now,
        };
        self.storage.event_log_mut().append(event.clone());

        // Run post-commit cascade.
        // Cascade errors should not invalidate resource creation.
        let cascade_events = self.run_cascade(&event).unwrap_or_default();
        let mut all_events = vec![event.clone()];
        all_events.extend(cascade_events);

        // Run agents on creation event
        self.run_agents(&event);
        self.evaluate_decision_nodes(&id);

        let final_resource = self
            .storage
            .state_store()
            .get(&id)
            .ok_or_else(|| KernelError::ResourceNotFound(id.clone()))?;

        Ok(TransitionOutcome::Success {
            resource: final_resource,
            events: all_events,
        })
    }

    /// The 8-step atomic transition boundary.
    pub fn transition(
        &mut self,
        resource_id: &ResourceId,
        to_state: &str,
        actor: &str,
        role: &str,
        authority_level: AuthorityLevel,
    ) -> Result<TransitionOutcome, KernelError> {
        // Get current resource
        let resource = self
            .storage
            .state_store()
            .get(resource_id)
            .ok_or_else(|| KernelError::ResourceNotFound(resource_id.clone()))?;

        let from_state = resource.state.clone();
        let resource_type = resource.resource_type.clone();

        // Build transition context
        let context = TransitionContext {
            resource_id: resource_id.clone(),
            resource_type: resource_type.clone(),
            from_state: from_state.clone(),
            to_state: to_state.to_string(),
            actor: actor.to_string(),
            role: role.to_string(),
            authority_level,
        };

        // Step 1: validate_state_machine
        let sm = self.registry.get_state_machine(&resource_type)?;
        if !sm.validate_transition(&from_state, to_state) {
            return Ok(TransitionOutcome::Rejected {
                step: "validate_state_machine".into(),
                reason: format!("Transition {} -> {} is not defined", from_state, to_state),
                details: serde_json::Value::Null,
            });
        }

        // Step 2: check_role_permissions
        if authority_level == AuthorityLevel::Human {
            if !self
                .role_registry
                .check_permission(role, "transition", &resource_type, to_state)
            {
                return Ok(TransitionOutcome::Rejected {
                    step: "check_role_permissions".into(),
                    reason: format!(
                        "Role '{}' cannot transition {} to state '{}'",
                        role, resource_type, to_state
                    ),
                    details: serde_json::Value::Null,
                });
            }
        }

        // Step 3: evaluate_guards
        let sm = self.registry.get_state_machine(&resource_type)?;
        match sm.evaluate_guard(&from_state, to_state, &resource) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(TransitionOutcome::Rejected {
                    step: "evaluate_guards".into(),
                    reason: format!("Guard failed for {} -> {}", from_state, to_state),
                    details: serde_json::Value::Null,
                });
            }
            Err(e) => return Err(e),
        }

        // Build read-only query context for policies and invariants.
        // This borrows storage + graph immutably — safe while &mut self is held.
        let query = KernelQuery {
            storage: self.storage.as_ref(),
            instance_graph: self.instance_graph.as_ref(),
        };

        // Step 4: evaluate_policies
        if let Some(denied) = self.policy_engine.first_denied(&resource, &context, &query) {
            return Ok(TransitionOutcome::Rejected {
                step: "evaluate_policies".into(),
                reason: format!("Policy '{}' denied: {}", denied.name, denied.message),
                details: serde_json::Value::Null,
            });
        }

        // Collect all policy evaluations for audit
        let policy_evals = self.policy_engine.evaluate_all(&resource, &context, &query);

        // Step 5: verify_invariants (strong only)
        // Check what the resource would look like AFTER the transition
        let mut projected = resource.clone();
        projected.state = to_state.to_string();
        let invariant_evals = self.invariant_checker.check_strong(&projected, &query);
        if let Some(violated) = invariant_evals.iter().find(|e| !e.holds) {
            return Ok(TransitionOutcome::Rejected {
                step: "verify_invariants".into(),
                reason: format!(
                    "Invariant '{}' violated: {}",
                    violated.name,
                    violated.violation.as_deref().unwrap_or("unknown")
                ),
                details: serde_json::Value::Null,
            });
        }

        // === All validation passed. Begin transaction, then mutate. ===

        let now = Utc::now();
        self.storage.begin()?;

        // Step 6: apply_state_change (read-modify-write)
        let mut updated = resource.clone();
        updated.state = to_state.to_string();
        updated.version += 1;
        updated.updated_at = now;
        if let Err(e) = self.storage.state_store_mut().update(&updated) {
            let _ = self.storage.rollback();
            return Err(e);
        }

        // Step 7: write_audit_record
        let audit_record = AuditRecord {
            id: Uuid::new_v4(),
            resource_type: resource_type.clone(),
            resource_id: resource_id.clone(),
            actor: actor.to_string(),
            role: role.to_string(),
            authority_level,
            previous_state: from_state.clone(),
            new_state: to_state.to_string(),
            policies_evaluated: policy_evals,
            invariants_checked: invariant_evals,
            timestamp: now,
        };
        self.storage.audit_store_mut().write(audit_record);

        // Step 8: emit_events
        let event = Event {
            id: Uuid::new_v4(),
            offset: 0,
            event_type: format!("{}.transitioned", resource_type),
            resource_id: resource_id.clone(),
            payload: serde_json::json!({
                "from": from_state,
                "to": to_state,
                "actor": actor,
                "role": role,
                "authority_level": authority_level.to_string(),
            }),
            actor: actor.to_string(),
            authority_level,
            timestamp: now,
        };
        self.storage.event_log_mut().append(event.clone());

        // Commit transaction (steps 6-8 are atomic)
        if let Err(e) = self.storage.commit() {
            let _ = self.storage.rollback();
            return Err(e);
        }

        // Post-commit: update instance graph node
        self.instance_graph.update_node(
            resource_id, to_state, &updated.data, updated.version,
        );

        // Post-commit: maybe create snapshot
        self.maybe_snapshot(resource_id, updated.version);

        // Post-commit: run cascade (outside transaction).
        // Cascade errors should not invalidate the committed transition.
        let cascade_events = self.run_cascade(&event).unwrap_or_default();
        let mut all_events = vec![event.clone()];
        all_events.extend(cascade_events);

        // Post-commit: run agents (observe the event, collect proposals)
        self.run_agents(&event);

        // Post-commit: evaluate decision nodes against collected proposals
        self.evaluate_decision_nodes(resource_id);

        let final_resource = self
            .storage
            .state_store()
            .get(resource_id)
            .ok_or_else(|| KernelError::ResourceNotFound(resource_id.clone()))?;

        Ok(TransitionOutcome::Success {
            resource: final_resource,
            events: all_events,
        })
    }

    /// Set desired state, triggering reconciliation.
    pub fn set_desired_state(
        &mut self,
        resource_id: &ResourceId,
        desired_state: &str,
        requested_by: &str,
        authority: AuthorityLevel,
    ) -> Result<(), KernelError> {
        let resource = self
            .storage
            .state_store()
            .get(resource_id)
            .ok_or_else(|| KernelError::ResourceNotFound(resource_id.clone()))?;

        let resource_type = resource.resource_type.clone();

        // Validate the desired state exists
        let sm = self.registry.get_state_machine(&resource_type)?;
        if !sm.has_state(desired_state) {
            return Err(KernelError::StateNotDefined(desired_state.to_string()));
        }

        // Set desired state (read-modify-write, bumps version for optimistic locking)
        let mut updated = resource;
        updated.desired_state = Some(desired_state.to_string());
        updated.version += 1;
        updated.updated_at = Utc::now();
        self.storage.state_store_mut().update(&updated)?;

        // Emit event
        let event = Event {
            id: Uuid::new_v4(),
            offset: 0,
            event_type: format!("{}.desired_state_set", resource_type),
            resource_id: resource_id.clone(),
            payload: serde_json::json!({
                "desired_state": desired_state,
                "requested_by": requested_by,
            }),
            actor: requested_by.to_string(),
            authority_level: authority,
            timestamp: Utc::now(),
        };
        self.storage.event_log_mut().append(event.clone());

        // Try to reconcile toward desired state
        self.reconcile_toward_desired(resource_id)?;

        Ok(())
    }

    /// Run the reconciliation loop toward desired state.
    fn reconcile_toward_desired(&mut self, resource_id: &ResourceId) -> Result<(), KernelError> {
        let max_depth = self.controller_scheduler.max_cascade_depth;

        for _depth in 0..max_depth {
            let resource = match self.storage.state_store().get(resource_id) {
                Some(r) => r,
                None => return Err(KernelError::ResourceNotFound(resource_id.clone())),
            };

            let desired = match &resource.desired_state {
                Some(d) => d.clone(),
                None => return Ok(()), // No desired state, nothing to do
            };

            if resource.state == desired {
                return Ok(()); // Already at desired state
            }

            // Find the next step toward desired state
            let sm = self.registry.get_state_machine(&resource.resource_type)?;
            let distances = sm.distance_to(&desired);

            // Pick the transition that gets us closest to desired
            let valid = sm.get_valid_transitions(&resource.state);
            let best = valid
                .iter()
                .filter_map(|t| {
                    distances
                        .get(&t.to_state)
                        .map(|&dist| (t, dist))
                })
                .min_by_key(|&(_, dist)| dist);

            match best {
                Some((transition, _)) => {
                    let to_state = transition.to_state.clone();
                    let result = self.transition(
                        resource_id,
                        &to_state,
                        "reconciler",
                        "system",
                        AuthorityLevel::Controller,
                    )?;

                    match result {
                        TransitionOutcome::Success { .. } => {
                            // Continue loop
                        }
                        TransitionOutcome::Rejected { .. } => {
                            // Can't make progress, stop
                            return Err(KernelError::ConvergenceFailure {
                                resource_id: resource_id.clone(),
                            });
                        }
                    }
                }
                None => {
                    // No path to desired state
                    return Err(KernelError::ConvergenceFailure {
                        resource_id: resource_id.clone(),
                    });
                }
            }
        }

        Err(KernelError::CascadeDepthExceeded {
            depth: max_depth,
            max: max_depth,
        })
    }

    /// Run the event cascade: dispatch event to matching controllers.
    /// Depth is tracked via self.cascade_depth across recursive transition() calls.
    fn run_cascade(&mut self, trigger: &Event) -> Result<Vec<Event>, KernelError> {
        self.cascade_depth += 1;
        let result = self.run_cascade_inner(trigger);
        self.cascade_depth -= 1;
        result
    }

    fn run_cascade_inner(&mut self, trigger: &Event) -> Result<Vec<Event>, KernelError> {
        if self.cascade_depth > self.controller_scheduler.max_cascade_depth {
            return Err(KernelError::CascadeDepthExceeded {
                depth: self.cascade_depth,
                max: self.controller_scheduler.max_cascade_depth,
            });
        }

        // Collect matching controller info (names, indices) to avoid borrow conflicts
        let matching_info: Vec<(String, u32, AuthorityLevel)> = self.controller_scheduler
            .get_matching_controllers(trigger)
            .iter()
            .map(|ctrl| (ctrl.name.clone(), ctrl.priority, ctrl.authority_level))
            .collect();

        if matching_info.is_empty() {
            return Ok(vec![]);
        }

        let resource = match self.storage.state_store().get(&trigger.resource_id) {
            Some(r) => r,
            None => return Ok(vec![]),
        };

        // Collect actions — skip controllers with open circuits
        let mut actions: Vec<(String, ControllerAction, AuthorityLevel)> = Vec::new();
        for (ctrl_name, _, authority) in &matching_info {
            if self.circuit_breaker.is_open(ctrl_name) {
                continue;
            }
            let query = KernelQuery {
                storage: self.storage.as_ref(),
                instance_graph: self.instance_graph.as_ref(),
            };
            // Find the controller handler and call reconcile
            let matching_ctrls = self.controller_scheduler.get_matching_controllers(trigger);
            if let Some(ctrl) = matching_ctrls.iter().find(|c| c.name == *ctrl_name) {
                match ctrl.handler.reconcile(&resource, &query) {
                    Ok(action) => {
                        self.circuit_breaker.record_success(ctrl_name);
                        actions.push((ctrl_name.clone(), action, *authority));
                    }
                    Err(e) => {
                        self.circuit_breaker.record_failure(ctrl_name);
                        if self.circuit_breaker.is_open(ctrl_name) {
                            self.circuit_breaker.add_dead_letter(DeadLetter {
                                event: trigger.clone(),
                                controller: ctrl_name.clone(),
                                error: e.to_string(),
                                attempts: self.circuit_breaker.get_failure_count(ctrl_name),
                                timestamp: Utc::now(),
                            });
                        }
                    }
                }
            }
        }

        let mut all_events = Vec::new();

        for (ctrl_name, action, authority) in actions {
            match action {
                ControllerAction::NoOp => {}
                ControllerAction::Transition { to_state } => {
                    let result = self.transition(
                        &trigger.resource_id,
                        &to_state,
                        &ctrl_name,
                        "controller",
                        authority,
                    )?;
                    if let TransitionOutcome::Success { events, .. } = result {
                        all_events.extend(events);
                    }
                }
                ControllerAction::SetDesiredState { state } => {
                    self.set_desired_state(
                        &trigger.resource_id,
                        &state,
                        &ctrl_name,
                        authority,
                    )?;
                }
            }
        }

        Ok(all_events)
    }

    // --- Query methods ---

    pub fn get_resource(&self, id: &ResourceId) -> Option<Resource> {
        self.storage.state_store().get(id)
    }

    pub fn get_events(&self, resource_id: &ResourceId) -> Vec<Event> {
        self.storage.event_log().get_by_resource(resource_id)
    }

    pub fn get_audit(&self, resource_id: &ResourceId) -> Vec<AuditRecord> {
        self.storage.audit_store().get_by_resource(resource_id)
    }

    pub fn list_resources(&self, resource_type: &str) -> Vec<Resource> {
        self.storage.state_store().list_by_type(resource_type)
    }

    // --- Snapshot methods ---

    /// Create a snapshot for a resource at its current state.
    pub fn create_snapshot(&mut self, resource_id: &ResourceId) -> Result<(), KernelError> {
        let resource = self
            .storage
            .state_store()
            .get(resource_id)
            .ok_or_else(|| KernelError::ResourceNotFound(resource_id.clone()))?;
        let offset = self.storage.event_log().get_latest_offset();

        self.storage.snapshot_store_mut().create(Snapshot {
            id: Uuid::new_v4(),
            resource_id: resource_id.clone(),
            state: resource.state,
            data: resource.data,
            version: resource.version,
            event_offset: offset,
            timestamp: Utc::now(),
        });
        Ok(())
    }

    /// Maybe create a snapshot if the resource version is at a snapshot interval.
    fn maybe_snapshot(&mut self, resource_id: &ResourceId, version: u64) {
        if self.snapshot_interval > 0 && version % (self.snapshot_interval as u64) == 0 {
            let _ = self.create_snapshot(resource_id);
        }
    }

    /// Replay a resource from its latest snapshot + subsequent events.
    /// Returns the reconstructed resource state, or None if the resource never existed.
    pub fn replay_resource(&self, resource_id: &ResourceId) -> Option<Resource> {
        let snapshot = self.storage.snapshot_store().get_latest(resource_id);

        match snapshot {
            Some(snap) => {
                // Start from snapshot state
                let mut resource = Resource {
                    id: resource_id.clone(),
                    resource_type: String::new(),
                    state: snap.state,
                    desired_state: None,
                    data: snap.data,
                    version: snap.version,
                    tenant_id: None,
                    created_at: snap.timestamp,
                    updated_at: snap.timestamp,
                };

                // Replay events after the snapshot offset
                let events = self.storage.event_log().get_by_resource_since(
                    resource_id, snap.event_offset,
                );
                for event in events {
                    if let Some(to) = event.payload.get("to").and_then(|v| v.as_str()) {
                        resource.state = to.to_string();
                        resource.version += 1;
                        resource.updated_at = event.timestamp;
                    }
                }

                Some(resource)
            }
            None => {
                // No snapshot — try to get current state directly
                self.storage.state_store().get(resource_id)
            }
        }
    }

    // --- Agent dispatch ---

    /// Run agents that match the event, collect proposals, log them.
    fn run_agents(&mut self, event: &Event) {
        let resource = match self.storage.state_store().get(&event.resource_id) {
            Some(r) => r,
            None => return,
        };

        let query = KernelQuery {
            storage: self.storage.as_ref(),
            instance_graph: self.instance_graph.as_ref(),
        };

        let proposals = self.agent_scheduler.collect_proposals(event, &resource, &query);

        // Log each proposal as an event
        for proposal in &proposals {
            let proposal_event = Event {
                id: Uuid::new_v4(),
                offset: 0,
                event_type: "agent.proposal".into(),
                resource_id: proposal.resource_id.clone(),
                payload: serde_json::to_value(proposal).unwrap_or_default(),
                actor: proposal.agent.clone(),
                authority_level: AuthorityLevel::Agent,
                timestamp: Utc::now(),
            };
            self.storage.event_log_mut().append(proposal_event);
        }

        self.pending_proposals.extend(proposals);
    }

    /// Evaluate decision nodes against pending proposals for a resource.
    fn evaluate_decision_nodes(&mut self, resource_id: &ResourceId) {
        let relevant: Vec<Proposal> = self.pending_proposals
            .iter()
            .filter(|p| &p.resource_id == resource_id)
            .cloned()
            .collect();

        if relevant.is_empty() {
            return;
        }

        for node in &self.decision_nodes.clone() {
            let outcome = node.evaluate(&relevant);
            match outcome {
                crate::decision::DecisionOutcome::AutoAccept { action, confidence } => {
                    // Execute the accepted action as AuthorityLevel::Agent
                    match action {
                        ProposedAction::Transition { to_state } => {
                            let _ = self.transition(
                                resource_id, &to_state,
                                &node.name, "agent", AuthorityLevel::Agent,
                            );
                        }
                        ProposedAction::SetDesiredState { state } => {
                            let _ = self.set_desired_state(
                                resource_id, &state, &node.name, AuthorityLevel::Agent,
                            );
                        }
                        ProposedAction::Flag { reason } => {
                            // Log flag event
                            let flag_event = Event {
                                id: Uuid::new_v4(),
                                offset: 0,
                                event_type: "agent.flag".into(),
                                resource_id: resource_id.clone(),
                                payload: serde_json::json!({
                                    "reason": reason,
                                    "decision_node": node.name,
                                    "confidence": confidence,
                                }),
                                actor: node.name.clone(),
                                authority_level: AuthorityLevel::Agent,
                                timestamp: Utc::now(),
                            };
                            self.storage.event_log_mut().append(flag_event);
                        }
                    }
                }
                _ => {
                    // NeedsReview, AutoReject, NoProposals — log but don't act
                }
            }
        }

        // Clear processed proposals for this resource
        self.pending_proposals.retain(|p| &p.resource_id != resource_id);
    }

    /// Get pending proposals (for inspection/testing).
    pub fn pending_proposals(&self) -> &[Proposal] {
        &self.pending_proposals
    }
}

impl Default for Kernel {
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
    use crate::invariant_checker::{InvariantCheck, InvariantDefinition, InvariantMode, InvariantScope};
    use crate::policy_engine::{PolicyDefinition, PolicyEvaluator};
    use crate::roles::{Permission, RoleDefinition};
    use crate::state_machine::{StateDefinition, StateStatus, TransitionDefinition};

    fn register_loan_type(kernel: &mut Kernel) {
        let states = vec![
            StateDefinition { name: "APPLIED".into(), status: StateStatus::Active },
            StateDefinition { name: "UNDERWRITING".into(), status: StateStatus::Active },
            StateDefinition { name: "APPROVED".into(), status: StateStatus::Active },
            StateDefinition { name: "DISBURSED".into(), status: StateStatus::Terminal },
            StateDefinition { name: "REJECTED".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "APPLIED".into(), to_state: "UNDERWRITING".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "APPROVED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "REJECTED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "APPROVED".into(), to_state: "DISBURSED".into(), guard: None, required_role: None },
        ];
        let sm = crate::state_machine::StateMachine::new(states, transitions, "APPLIED".into()).unwrap();
        kernel
            .register_type(ResourceTypeDefinition {
                name: "loan".into(),
                schema: serde_json::json!({}),
                state_machine: sm,
            })
            .unwrap();
    }

    fn register_roles(kernel: &mut Kernel) {
        kernel.role_registry.register(RoleDefinition {
            name: "officer".into(),
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:UNDERWRITING"),
            ],
        });
        kernel.role_registry.register(RoleDefinition {
            name: "manager".into(),
            permissions: vec![
                Permission::from_shorthand("view"),
                Permission::from_shorthand("transition:*"),
            ],
        });
    }

    fn setup_kernel() -> Kernel {
        let mut kernel = Kernel::new();
        register_loan_type(&mut kernel);
        register_roles(&mut kernel);
        kernel
    }

    #[test]
    fn test_create_resource() {
        let mut kernel = setup_kernel();
        let result = kernel
            .create_resource("loan", serde_json::json!({"amount": 100000}), "user1", AuthorityLevel::Human)
            .unwrap();

        match result {
            TransitionOutcome::Success { resource, events } => {
                assert_eq!(resource.state, "APPLIED");
                assert_eq!(resource.version, 1);
                assert!(!events.is_empty());
                assert_eq!(events[0].event_type, "loan.created");
            }
            TransitionOutcome::Rejected { .. } => panic!("Expected success"),
        }
    }

    #[test]
    fn test_create_unknown_type() {
        let mut kernel = setup_kernel();
        let result = kernel.create_resource("bogus", serde_json::json!({}), "user1", AuthorityLevel::Human);
        assert!(matches!(result, Err(KernelError::TypeNotRegistered(_))));
    }

    #[test]
    fn test_basic_transition() {
        let mut kernel = setup_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        let result = kernel
            .transition(&resource.id, "UNDERWRITING", "user1", "officer", AuthorityLevel::Human)
            .unwrap();

        match result {
            TransitionOutcome::Success { resource, events } => {
                assert_eq!(resource.state, "UNDERWRITING");
                assert_eq!(resource.version, 2);
                assert!(events.iter().any(|e| e.event_type == "loan.transitioned"));
            }
            TransitionOutcome::Rejected { step, reason, .. } => {
                panic!("Rejected at {}: {}", step, reason);
            }
        }
    }

    #[test]
    fn test_invalid_transition_rejected() {
        let mut kernel = setup_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        let result = kernel
            .transition(&resource.id, "APPROVED", "user1", "manager", AuthorityLevel::Human)
            .unwrap();

        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "validate_state_machine"));
    }

    #[test]
    fn test_permission_denied() {
        let mut kernel = setup_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Officer can transition to UNDERWRITING
        let result = kernel
            .transition(&resource.id, "UNDERWRITING", "user1", "officer", AuthorityLevel::Human)
            .unwrap();
        assert!(matches!(result, TransitionOutcome::Success { .. }));

        // Officer cannot transition to APPROVED
        let result = kernel
            .transition(&resource.id, "APPROVED", "user1", "officer", AuthorityLevel::Human)
            .unwrap();
        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "check_role_permissions"));
    }

    #[test]
    fn test_policy_blocks_transition() {
        let mut kernel = setup_kernel();

        struct HighValueBlock;
        impl PolicyEvaluator for HighValueBlock {
            fn evaluate(&self, r: &Resource, _c: &TransitionContext, _q: &dyn SystemQuery) -> PolicyResult {
                let amount = r.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if amount > 5_000_000.0 {
                    PolicyResult::deny("High value loans need committee approval")
                } else {
                    PolicyResult::allow()
                }
            }
        }

        kernel.policy_engine.register(PolicyDefinition {
            name: "high_value".into(),
            description: "Block high value loans".into(),
            evaluator: Box::new(HighValueBlock),
            applicable_states: vec!["APPLIED".into()],
            resource_types: vec!["loan".into()],
            priority: 50,
        });

        let resource = match kernel.create_resource("loan", serde_json::json!({"amount": 10_000_000}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        let result = kernel
            .transition(&resource.id, "UNDERWRITING", "user1", "manager", AuthorityLevel::Human)
            .unwrap();

        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "evaluate_policies"));
    }

    #[test]
    fn test_strong_invariant_blocks_transition() {
        let mut kernel = setup_kernel();

        struct PositiveAmount;
        impl InvariantCheck for PositiveAmount {
            fn check(&self, r: &Resource, _q: &dyn SystemQuery) -> InvariantResult {
                let amount = r.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if amount > 0.0 {
                    InvariantResult::ok()
                } else {
                    InvariantResult::violated("Amount must be positive")
                }
            }
        }

        kernel.invariant_checker.register(InvariantDefinition {
            name: "positive_amount".into(),
            description: "test".into(),
            mode: InvariantMode::Strong,
            scope: InvariantScope::Resource,
            resource_types: vec!["loan".into()],
            checker: Box::new(PositiveAmount),
        });

        // Creating a resource with negative amount should be blocked
        let result = kernel
            .create_resource("loan", serde_json::json!({"amount": -100}), "user1", AuthorityLevel::Human)
            .unwrap();

        assert!(matches!(result, TransitionOutcome::Rejected { step, .. } if step == "verify_invariants"));
    }

    #[test]
    fn test_audit_trail() {
        let mut kernel = setup_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        kernel
            .transition(&resource.id, "UNDERWRITING", "user1", "officer", AuthorityLevel::Human)
            .unwrap();

        let audit = kernel.get_audit(&resource.id);
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].previous_state, "APPLIED");
        assert_eq!(audit[0].new_state, "UNDERWRITING");
        assert_eq!(audit[0].actor, "user1");
        assert_eq!(audit[0].role, "officer");
        assert_eq!(audit[0].authority_level, AuthorityLevel::Human);
    }

    #[test]
    fn test_event_trail() {
        let mut kernel = setup_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        kernel
            .transition(&resource.id, "UNDERWRITING", "user1", "officer", AuthorityLevel::Human)
            .unwrap();

        let events = kernel.get_events(&resource.id);
        assert!(events.len() >= 2); // created + transitioned
        assert_eq!(events[0].event_type, "loan.created");
        assert_eq!(events[1].event_type, "loan.transitioned");
    }

    #[test]
    fn test_full_lifecycle() {
        let mut kernel = setup_kernel();

        // Create
        let resource = match kernel.create_resource("loan", serde_json::json!({"amount": 100000}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };
        assert_eq!(resource.state, "APPLIED");

        // APPLIED -> UNDERWRITING
        let result = kernel.transition(&resource.id, "UNDERWRITING", "user1", "officer", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Success { .. }));

        // UNDERWRITING -> APPROVED
        let result = kernel.transition(&resource.id, "APPROVED", "user1", "manager", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Success { .. }));

        // APPROVED -> DISBURSED
        let result = kernel.transition(&resource.id, "DISBURSED", "user1", "manager", AuthorityLevel::Human).unwrap();
        assert!(matches!(result, TransitionOutcome::Success { .. }));

        let final_resource = kernel.get_resource(&resource.id).unwrap();
        assert_eq!(final_resource.state, "DISBURSED");
        assert_eq!(final_resource.version, 4); // 1 create + 3 transitions

        // Verify audit trail has 3 entries (only transitions, not creation)
        let audit = kernel.get_audit(&resource.id);
        assert_eq!(audit.len(), 3);

        // Verify event trail
        let events = kernel.get_events(&resource.id);
        assert!(events.len() >= 4); // created + 3 transitioned
    }

    #[test]
    fn test_desired_state_reconciliation() {
        let mut kernel = setup_kernel();

        let resource = match kernel.create_resource("loan", serde_json::json!({}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Set desired state to DISBURSED
        kernel
            .set_desired_state(&resource.id, "DISBURSED", "manager1", AuthorityLevel::Human)
            .unwrap();

        // Resource should now be at DISBURSED (reconciliation loop ran)
        let final_resource = kernel.get_resource(&resource.id).unwrap();
        assert_eq!(final_resource.state, "DISBURSED");
    }

    #[test]
    fn test_controller_authority_level_in_audit() {
        let mut kernel = setup_kernel();

        let resource = match kernel.create_resource("loan", serde_json::json!({}), "user1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Transition as CONTROLLER
        kernel
            .transition(&resource.id, "UNDERWRITING", "auto-ctrl", "controller", AuthorityLevel::Controller)
            .unwrap();

        let audit = kernel.get_audit(&resource.id);
        assert_eq!(audit[0].authority_level, AuthorityLevel::Controller);
    }

    #[test]
    fn test_snapshot_creation_manual() {
        let mut kernel = setup_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({"amount": 100}), "u1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        kernel.transition(&resource.id, "UNDERWRITING", "u1", "manager", AuthorityLevel::Human).unwrap();
        kernel.create_snapshot(&resource.id).unwrap();

        let snap = kernel.storage.snapshot_store().get_latest(&resource.id);
        assert!(snap.is_some());
        let snap = snap.unwrap();
        assert_eq!(snap.state, "UNDERWRITING");
        assert_eq!(snap.version, 2);
    }

    #[test]
    fn test_snapshot_auto_creation() {
        let mut kernel = setup_kernel();
        kernel.snapshot_interval = 2; // Snapshot every 2 transitions

        let resource = match kernel.create_resource("loan", serde_json::json!({}), "u1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Version 1 (create) — no snapshot (1 % 2 != 0)
        assert!(kernel.storage.snapshot_store().get_latest(&resource.id).is_none());

        // Version 2 (transition) — snapshot! (2 % 2 == 0)
        kernel.transition(&resource.id, "UNDERWRITING", "u1", "manager", AuthorityLevel::Human).unwrap();
        let snap = kernel.storage.snapshot_store().get_latest(&resource.id);
        assert!(snap.is_some());
        assert_eq!(snap.unwrap().state, "UNDERWRITING");

        // Version 3 — no snapshot
        kernel.transition(&resource.id, "APPROVED", "u1", "manager", AuthorityLevel::Human).unwrap();

        // Version 4 — snapshot again
        kernel.transition(&resource.id, "DISBURSED", "u1", "manager", AuthorityLevel::Human).unwrap();
        let snap = kernel.storage.snapshot_store().get_latest(&resource.id).unwrap();
        assert_eq!(snap.state, "DISBURSED");
        assert_eq!(snap.version, 4);
    }

    #[test]
    fn test_replay_resource() {
        let mut kernel = setup_kernel();
        let resource = match kernel.create_resource("loan", serde_json::json!({"amount": 500}), "u1", AuthorityLevel::Human).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        kernel.transition(&resource.id, "UNDERWRITING", "u1", "manager", AuthorityLevel::Human).unwrap();
        kernel.create_snapshot(&resource.id).unwrap();
        kernel.transition(&resource.id, "APPROVED", "u1", "manager", AuthorityLevel::Human).unwrap();
        kernel.transition(&resource.id, "DISBURSED", "u1", "manager", AuthorityLevel::Human).unwrap();

        // Replay from snapshot should reconstruct final state
        let replayed = kernel.replay_resource(&resource.id).unwrap();
        assert_eq!(replayed.state, "DISBURSED");
        assert_eq!(replayed.version, 4); // snap at v2 + 2 events replayed
    }
}
