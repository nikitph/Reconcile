"""Tests for invariant checking."""

from reconcile import define_system, InvariantResult


def test_strong_invariant_blocks_creation(loan_system_with_invariants):
    sys = loan_system_with_invariants
    result = sys.create({"amount": -100}, actor="u1")
    assert not result.success
    assert result.rejected_step == "verify_invariants"
    assert "positive_amount" in result.rejected_reason


def test_strong_invariant_allows_valid(loan_system_with_invariants):
    sys = loan_system_with_invariants
    result = sys.create({"amount": 50_000}, actor="u1")
    assert result.success


def test_invariant_checked_on_transition():
    """Strong invariant checked on the post-transition state."""

    def no_approved_without_score(resource, query):
        if resource.state == "APPROVED":
            if "score" not in resource.data or resource.data["score"] is None:
                return InvariantResult.violated("Approved loans must have a score")
        return InvariantResult.ok()

    sys = define_system(
        name="loan",
        states=["APPLIED", "APPROVED", "DONE"],
        transitions=[("APPLIED", "APPROVED"), ("APPROVED", "DONE")],
        terminal_states=["DONE"],
        invariants=[{
            "name": "score_required",
            "mode": "strong",
            "scope": "transition",
            "check": no_approved_without_score,
            "resource_types": ["loan"],
        }],
    )

    # Loan without score can't be approved
    result = sys.create({"amount": 100}, actor="u1")
    rid = result.resource.id
    r = sys.transition(rid, "APPROVED", actor="u1", role="mgr",
                       authority_level="CONTROLLER")
    assert not r.success
    assert r.rejected_step == "verify_invariants"

    # Loan with score can be approved
    result2 = sys.create({"amount": 100, "score": 0.85}, actor="u1")
    r2 = sys.transition(result2.resource.id, "APPROVED", actor="u1", role="mgr",
                        authority_level="CONTROLLER")
    assert r2.success


def test_invariant_returns_bool():
    """Invariant check can return True/False instead of InvariantResult."""
    sys = define_system(
        name="item",
        states=["ACTIVE", "DONE"],
        transitions=[("ACTIVE", "DONE")],
        terminal_states=["DONE"],
        invariants=[{
            "name": "bool_inv",
            "mode": "strong",
            "scope": "resource",
            "check": lambda r, q: r.data.get("valid", False),
        }],
    )

    # Invalid data
    r1 = sys.create({"valid": False}, actor="u1")
    assert not r1.success

    # Valid data
    r2 = sys.create({"valid": True}, actor="u1")
    assert r2.success


def test_multiple_invariants():
    """Multiple strong invariants: all must hold."""
    sys = define_system(
        name="loan",
        states=["A", "B"],
        transitions=[("A", "B")],
        terminal_states=["B"],
        invariants=[
            {"name": "inv1", "mode": "strong", "scope": "resource",
             "check": lambda r, q: InvariantResult.ok()},
            {"name": "inv2", "mode": "strong", "scope": "resource",
             "check": lambda r, q: InvariantResult.violated("always fails")},
        ],
    )

    result = sys.create({}, actor="u1")
    assert not result.success
    assert "inv2" in result.rejected_reason
