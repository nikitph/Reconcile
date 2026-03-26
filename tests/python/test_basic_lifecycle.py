"""Tests for basic resource lifecycle."""

from reconcile import define_system


def test_import():
    import reconcile
    assert reconcile.__version__ == "0.1.0"


def test_create_resource(loan_system):
    result = loan_system.create({"amount": 100_000}, actor="user1")
    assert result.success
    assert result.resource.state == "APPLIED"
    assert result.resource.version == 1
    assert result.resource.resource_type == "loan"


def test_create_resource_data(loan_system):
    result = loan_system.create({"amount": 500_000, "applicant": "Acme Corp"}, actor="user1")
    assert result.success
    data = result.resource.data
    assert data["amount"] == 500_000
    assert data["applicant"] == "Acme Corp"


def test_resource_id_is_uuid(loan_system):
    result = loan_system.create({}, actor="user1")
    rid = result.resource.id
    # UUID format: 8-4-4-4-12 hex chars
    parts = rid.split("-")
    assert len(parts) == 5
    assert len(parts[0]) == 8


def test_get_resource(loan_system):
    result = loan_system.create({"amount": 100}, actor="user1")
    rid = result.resource.id
    resource = loan_system.get(rid)
    assert resource is not None
    assert resource.id == rid
    assert resource.state == "APPLIED"


def test_get_nonexistent_resource(loan_system):
    resource = loan_system.get("00000000-0000-0000-0000-000000000000")
    assert resource is None


def test_list_resources(loan_system):
    loan_system.create({}, actor="user1")
    loan_system.create({}, actor="user2")
    resources = loan_system.list_resources()
    assert len(resources) == 2


def test_full_lifecycle(loan_system):
    """APPLIED -> UNDERWRITING -> APPROVED -> DISBURSED"""
    result = loan_system.create({"amount": 100_000}, actor="user1")
    rid = result.resource.id

    r = loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    assert r.success
    assert r.resource.state == "UNDERWRITING"

    r = loan_system.transition(rid, "APPROVED", actor="u1", role="manager")
    assert r.success
    assert r.resource.state == "APPROVED"

    r = loan_system.transition(rid, "DISBURSED", actor="u1", role="manager")
    assert r.success
    assert r.resource.state == "DISBURSED"

    final = loan_system.get(rid)
    assert final.state == "DISBURSED"
    assert final.version == 4  # 1 create + 3 transitions


def test_rejection_path(loan_system):
    """APPLIED -> UNDERWRITING -> REJECTED"""
    result = loan_system.create({}, actor="user1")
    rid = result.resource.id

    loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    r = loan_system.transition(rid, "REJECTED", actor="u1", role="manager")
    assert r.success
    assert r.resource.state == "REJECTED"


def test_define_system_minimal():
    """Minimal system with 2 states."""
    sys = define_system(
        name="ticket",
        states=["OPEN", "CLOSED"],
        transitions=[("OPEN", "CLOSED")],
        terminal_states=["CLOSED"],
    )
    result = sys.create({}, actor="bot")
    assert result.success
    assert result.resource.state == "OPEN"
