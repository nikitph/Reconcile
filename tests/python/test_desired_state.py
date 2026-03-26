"""Tests for desired state reconciliation."""


def test_set_desired_state(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    loan_system.set_desired(rid, "DISBURSED", requested_by="manager1")

    resource = loan_system.get(rid)
    assert resource.state == "DISBURSED"


def test_desired_state_multi_step(loan_system):
    """Reconciliation traverses multiple steps: APPLIED -> UW -> APPROVED -> DISBURSED."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    loan_system.set_desired(rid, "DISBURSED", requested_by="mgr")

    resource = loan_system.get(rid)
    assert resource.state == "DISBURSED"

    # Should have events for each step
    events = loan_system.events(rid)
    transition_events = [e for e in events if e.event_type == "loan.transitioned"]
    assert len(transition_events) == 3  # UW, APPROVED, DISBURSED


def test_desired_state_already_there(loan_system):
    """Setting desired state to current state is a no-op."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    loan_system.set_desired(rid, "APPLIED", requested_by="mgr")
    resource = loan_system.get(rid)
    assert resource.state == "APPLIED"


def test_desired_state_partial_path(loan_system):
    """Set desired to APPROVED (not terminal)."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    loan_system.set_desired(rid, "APPROVED", requested_by="mgr")

    resource = loan_system.get(rid)
    assert resource.state == "APPROVED"


def test_desired_state_events_include_desired_set(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    loan_system.set_desired(rid, "UNDERWRITING", requested_by="mgr")

    events = loan_system.events(rid)
    desired_events = [e for e in events if "desired_state_set" in e.event_type]
    assert len(desired_events) == 1
