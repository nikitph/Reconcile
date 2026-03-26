"""Tests for role-based access control."""


def test_officer_can_transition_to_underwriting(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    r = loan_system.transition(rid, "UNDERWRITING", actor="u1", role="officer")
    assert r.success


def test_officer_cannot_transition_to_approved(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="u1", role="officer")
    r = loan_system.transition(rid, "APPROVED", actor="u1", role="officer")
    assert not r.success
    assert r.rejected_step == "check_role_permissions"


def test_manager_can_transition_anywhere(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    r = loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    assert r.success

    r = loan_system.transition(rid, "APPROVED", actor="u1", role="manager")
    assert r.success

    r = loan_system.transition(rid, "DISBURSED", actor="u1", role="manager")
    assert r.success


def test_viewer_cannot_transition(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    r = loan_system.transition(rid, "UNDERWRITING", actor="u1", role="viewer")
    assert not r.success
    assert r.rejected_step == "check_role_permissions"


def test_controller_bypasses_rbac(loan_system):
    """CONTROLLER authority level bypasses role checks."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    r = loan_system.transition(rid, "UNDERWRITING", actor="ctrl", role="none",
                               authority_level="CONTROLLER")
    assert r.success
