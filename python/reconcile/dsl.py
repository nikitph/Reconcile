"""Declarative DSL for defining Reconcile systems."""

from reconcile._native import ReconcileSystem


def define_system(
    *,
    name: str,
    states: list[str],
    transitions: list[tuple[str, str]],
    initial_state: str | None = None,
    terminal_states: list[str] | None = None,
    roles: dict[str, list[str]] | None = None,
    policies: list[dict] | None = None,
    invariants: list[dict] | None = None,
    controllers: list | None = None,
    relationships: list[dict] | None = None,
    agents: list[dict] | None = None,
    decision_nodes: list[dict] | None = None,
    database_url: str | None = None,
    snapshot_interval: int = 0,
) -> "SystemWrapper":
    """Define a complete Reconcile system declaratively.

    Args:
        name: Resource type name (e.g., "loan")
        states: List of state names
        transitions: List of (from_state, to_state) tuples
        initial_state: Starting state (defaults to first in list)
        terminal_states: States with no outbound transitions
        roles: Dict of role_name -> [permission shorthands]
        policies: List of policy dicts with keys: name, description, evaluate, applicable_states, resource_types, priority
        invariants: List of invariant dicts with keys: name, description, mode, scope, check, resource_types
        controllers: List of Controller instances or dicts
        relationships: List of relationship dicts with keys: to_type, relation, cardinality, required, foreign_key
        agents: List of agent dicts with keys: name, observe, on_events, priority
        decision_nodes: List of decision node dicts with keys: name, agents, aggregation, auto_accept, auto_reject
        database_url: PostgreSQL connection string (None = in-memory)
        snapshot_interval: Create snapshot every N transitions (0 = disabled)

    Returns:
        SystemWrapper around the native ReconcileSystem
    """
    system = ReconcileSystem(
        database_url=database_url,
        snapshot_interval=snapshot_interval,
    )

    init = initial_state or states[0]
    terms = terminal_states or []

    system.register_type(name, states, transitions, init, terms)

    for role_name, perms in (roles or {}).items():
        system.register_role(role_name, perms)

    for policy in policies or []:
        system.register_policy(
            name=policy["name"],
            description=policy.get("description", ""),
            evaluator=policy["evaluate"],
            applicable_states=policy.get("applicable_states", []),
            resource_types=policy.get("resource_types", []),
            priority=policy.get("priority", 50),
        )

    for inv in invariants or []:
        system.register_invariant(
            name=inv["name"],
            description=inv.get("description", ""),
            mode=inv.get("mode", "strong"),
            scope=inv.get("scope", "resource"),
            checker=inv["check"],
            resource_types=inv.get("resource_types", []),
        )

    for ctrl in controllers or []:
        if isinstance(ctrl, dict):
            system.register_controller(
                name=ctrl["name"],
                handler=ctrl["reconcile"],
                on_events=ctrl.get("on_events", []),
                priority=ctrl.get("priority", 50),
                enforces=ctrl.get("enforces", []),
                authority_level=ctrl.get("authority_level", "CONTROLLER"),
            )
        else:
            # Controller instance
            system.register_controller(
                name=ctrl.name,
                handler=ctrl.reconcile,
                on_events=ctrl.on_events,
                priority=ctrl.priority,
                enforces=ctrl.enforces,
                authority_level=ctrl.authority_level,
            )

    for relationship in relationships or []:
        system.register_relationship(
            name,
            relationship["to_type"],
            relationship["relation"],
            relationship.get("cardinality", "many_to_one"),
            relationship.get("required", False),
            relationship.get("foreign_key", ""),
        )

    for agent in agents or []:
        system.register_agent(
            agent["name"],
            agent["observe"],
            agent.get("on_events", []),
            agent.get("priority", 50),
        )

    for node in decision_nodes or []:
        system.register_decision_node(
            node["name"],
            node["agents"],
            node.get("aggregation", "weighted_avg"),
            node.get("auto_accept", 0.9),
            node.get("auto_reject", 0.5),
        )

    return SystemWrapper(system, name)


class SystemWrapper:
    """High-level wrapper around ReconcileSystem for a specific resource type."""

    def __init__(self, system: ReconcileSystem, resource_type: str):
        self._system = system
        self._resource_type = resource_type

    @property
    def native(self) -> ReconcileSystem:
        """Access the underlying native system."""
        return self._system

    def register_type(self, name: str, states: list[str], transitions: list[tuple[str, str]],
                      initial_state: str | None = None, terminal_states: list[str] | None = None):
        """Register an additional resource type on the underlying system."""
        init = initial_state or states[0]
        terms = terminal_states or []
        self._system.register_type(name, states, transitions, init, terms)
        return self

    def register_role(self, name: str, permissions: list[str]):
        """Register an additional role on the underlying system."""
        self._system.register_role(name, permissions)
        return self

    def create(self, data: dict, *, actor: str = "system", authority_level: str = "HUMAN"):
        """Create a new resource."""
        return self._system.create(self._resource_type, data, actor, authority_level)

    def create_resource(self, resource_type: str, data: dict, *, actor: str = "system",
                        authority_level: str = "HUMAN"):
        """Create a resource of any registered type."""
        return self._system.create(resource_type, data, actor, authority_level)

    def transition(self, resource_id: str, to_state: str, *, actor: str = "system",
                   role: str = "system", authority_level: str = "HUMAN"):
        """Request a state transition."""
        return self._system.transition(resource_id, to_state, actor, role, authority_level)

    def set_desired(self, resource_id: str, desired_state: str, *,
                    requested_by: str = "system", authority_level: str = "HUMAN"):
        """Set desired state (triggers reconciliation)."""
        return self._system.set_desired(resource_id, desired_state, requested_by, authority_level)

    def get(self, resource_id: str):
        """Get a resource by ID."""
        return self._system.get(resource_id)

    def events(self, resource_id: str):
        """Get events for a resource."""
        return self._system.events(resource_id)

    def audit(self, resource_id: str):
        """Get audit trail for a resource."""
        return self._system.audit(resource_id)

    def list_resources(self):
        """List all resources of this type."""
        return self._system.list_resources(self._resource_type)

    def list_resources_of(self, resource_type: str):
        """List all resources of a registered type."""
        return self._system.list_resources(resource_type)

    # --- Graph methods ---

    def register_relationship(self, to_type: str, relation: str, *,
                               cardinality: str = "many_to_one",
                               required: bool = False,
                               foreign_key: str = ""):
        """Declare a relationship from this resource type to another."""
        self._system.register_relationship(
            self._resource_type, to_type, relation,
            cardinality, required, foreign_key,
        )
        return self

    def register_agent(self, name: str, observe, *, on_events: list[str] | None = None,
                       priority: int = 50):
        """Register an agent on the underlying system."""
        self._system.register_agent(name, observe, on_events or [], priority)
        return self

    def register_decision_node(self, name: str, agents: list[str], *,
                               aggregation: str = "weighted_avg",
                               auto_accept: float = 0.9,
                               auto_reject: float = 0.5):
        """Register a decision node on the underlying system."""
        self._system.register_decision_node(
            name, agents, aggregation, auto_accept, auto_reject,
        )
        return self

    def graph_neighbors(self, resource_id: str, edge_type: str | None = None):
        """Get neighbor resources via graph edges."""
        return self._system.graph_neighbors(resource_id, edge_type)

    def graph_aggregate(self, resource_id: str, edge_type: str, field: str,
                        agg_fn: str = "SUM"):
        """Aggregate a field across graph neighbors."""
        return self._system.graph_aggregate(resource_id, edge_type, field, agg_fn)

    def graph_degree(self, resource_id: str, edge_type: str | None = None):
        """Get connection count."""
        return self._system.graph_degree(resource_id, edge_type)

    # --- Interface Projection ---

    def project(self, resource_id: str, role: str):
        """Compute the interface projection for a resource viewed by a role."""
        return self._system.project(resource_id, role)

    def project_list(self, role: str):
        """Batch projection for all resources of this type."""
        return self._system.project_list(self._resource_type, role)

    def export_spec(self):
        """Export the system definition as machine-readable JSON."""
        return self._system.export_spec()

    def execute_action(self, resource_id: str, action: str, *, actor: str,
                       role: str, authority_level: str = "INTERFACE"):
        """Execute an action and return the new projection."""
        return self._system.execute_action(resource_id, action, actor, role, authority_level)
