"""Tests for audit log."""


def test_audit_on_transition(loan_system):
    result = loan_system.create({}, actor="alice")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="alice", role="officer")

    audit = loan_system.audit(rid)
    assert len(audit) == 1
    assert audit[0].previous_state == "APPLIED"
    assert audit[0].new_state == "UNDERWRITING"
    assert audit[0].actor == "alice"
    assert audit[0].role == "officer"


def test_audit_authority_level(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="u1", role="officer")

    audit = loan_system.audit(rid)
    assert audit[0].authority_level == "HUMAN"


def test_audit_controller_authority(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="auto-ctrl", role="system",
                           authority_level="CONTROLLER")

    audit = loan_system.audit(rid)
    assert audit[0].authority_level == "CONTROLLER"


def test_audit_records_full_lifecycle(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    loan_system.transition(rid, "APPROVED", actor="u2", role="manager")
    loan_system.transition(rid, "DISBURSED", actor="u3", role="manager")

    audit = loan_system.audit(rid)
    assert len(audit) == 3

    assert audit[0].previous_state == "APPLIED"
    assert audit[0].new_state == "UNDERWRITING"
    assert audit[0].actor == "u1"

    assert audit[1].previous_state == "UNDERWRITING"
    assert audit[1].new_state == "APPROVED"
    assert audit[1].actor == "u2"

    assert audit[2].previous_state == "APPROVED"
    assert audit[2].new_state == "DISBURSED"
    assert audit[2].actor == "u3"


def test_no_audit_for_rejected_transition(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    # Try invalid transition
    loan_system.transition(rid, "APPROVED", actor="u1", role="manager")

    audit = loan_system.audit(rid)
    assert len(audit) == 0  # No audit for rejected transitions


def test_audit_resource_isolation(loan_system):
    r1 = loan_system.create({}, actor="u1")
    r2 = loan_system.create({}, actor="u2")
    loan_system.transition(r1.resource.id, "UNDERWRITING", actor="u1", role="manager")

    assert len(loan_system.audit(r1.resource.id)) == 1
    assert len(loan_system.audit(r2.resource.id)) == 0


def test_audit_includes_policy_evaluations(loan_system_with_policies):
    sys = loan_system_with_policies
    result = sys.create({"amount": 1_000_000}, actor="u1")
    rid = result.resource.id
    sys.transition(rid, "UNDERWRITING", actor="u1", role="manager")

    audit = sys.audit(rid)
    assert len(audit) == 1
    policies = audit[0].policies_evaluated
    assert len(policies) >= 1
    # Each policy eval is (name, passed, message)
    assert policies[0][0] == "high_value_limit"
    assert policies[0][1] is True  # passed
