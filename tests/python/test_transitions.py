"""Tests for state transition validation."""


def test_valid_transition(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    r = loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    assert r.success


def test_invalid_transition_skip_state(loan_system):
    """Cannot skip UNDERWRITING: APPLIED -> APPROVED is not defined."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    r = loan_system.transition(rid, "APPROVED", actor="u1", role="manager")
    assert not r.success
    assert r.rejected_step == "validate_state_machine"


def test_invalid_transition_from_terminal(loan_system):
    """Cannot transition from terminal state DISBURSED."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    loan_system.transition(rid, "APPROVED", actor="u1", role="manager")
    loan_system.transition(rid, "DISBURSED", actor="u1", role="manager")

    r = loan_system.transition(rid, "APPLIED", actor="u1", role="manager")
    assert not r.success
    assert r.rejected_step == "validate_state_machine"


def test_invalid_transition_nonexistent_state(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    r = loan_system.transition(rid, "NONEXISTENT", actor="u1", role="manager")
    assert not r.success


def test_transition_bumps_version(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    assert result.resource.version == 1

    r = loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    assert r.resource.version == 2


def test_transition_returns_events(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    r = loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    assert r.success
    assert len(r.events) >= 1
    assert any(e.event_type == "loan.transitioned" for e in r.events)


def test_multiple_transitions_same_resource(loan_system):
    """Each transition should advance version and state correctly."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    states = ["UNDERWRITING", "APPROVED", "DISBURSED"]
    for i, state in enumerate(states, start=2):
        r = loan_system.transition(rid, state, actor="u1", role="manager")
        assert r.success
        assert r.resource.state == state
        assert r.resource.version == i
