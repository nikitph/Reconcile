"""Coverage for the richer DSL surface."""

from reconcile import define_system


def test_define_system_registers_relationships_agents_and_decision_nodes():
    def agent(resource, query):
        if resource.state == "REVIEW":
            return {"transition": "APPROVED", "confidence": 0.95, "reasoning": "looks safe"}
        return None

    system = define_system(
        name="loan",
        states=["NEW", "REVIEW", "APPROVED"],
        transitions=[("NEW", "REVIEW"), ("REVIEW", "APPROVED")],
        relationships=[{"to_type": "applicant", "relation": "belongs_to", "required": True}],
        agents=[{"name": "risk", "observe": agent, "on_events": ["loan.transitioned"], "priority": 80}],
        decision_nodes=[{"name": "auto_review", "agents": ["risk"], "auto_accept": 0.9}],
    )
    system.register_type("applicant", ["ACTIVE"], [], "ACTIVE", ["ACTIVE"])

    applicant = system.create_resource("applicant", {"name": "Acme"}, actor="system", authority_level="SYSTEM")
    loan = system.create({"applicant_id": applicant.resource.id}, actor="u1")
    system.transition(loan.resource.id, "REVIEW", actor="u1", role="system", authority_level="SYSTEM")

    spec = system.export_spec()
    assert spec["relationships"][0]["relation"] == "belongs_to"
    assert spec["agent_count"] == 1
    assert spec["decision_node_count"] == 1


def test_system_wrapper_multi_type_helpers_work():
    system = define_system(
        name="loan",
        states=["NEW", "DONE"],
        transitions=[("NEW", "DONE")],
    )
    system.register_type("applicant", ["ACTIVE"], [], "ACTIVE", ["ACTIVE"])
    system.register_role("operator", ["transition:*"])

    created = system.create_resource("applicant", {"name": "Borrower"}, actor="u1")
    listed = system.list_resources_of("applicant")

    assert created.success is True
    assert len(listed) == 1
    assert listed[0].resource_type == "applicant"
