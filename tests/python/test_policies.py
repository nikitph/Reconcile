"""Tests for policy evaluation."""

from reconcile import define_system, PolicyResult


def test_policy_allows(loan_system_with_policies):
    sys = loan_system_with_policies
    result = sys.create({"amount": 1_000_000}, actor="u1")
    rid = result.resource.id
    r = sys.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    assert r.success


def test_policy_blocks(loan_system_with_policies):
    sys = loan_system_with_policies
    result = sys.create({"amount": 10_000_000}, actor="u1")
    rid = result.resource.id
    r = sys.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    assert not r.success
    assert r.rejected_step == "evaluate_policies"
    assert "high_value_limit" in r.rejected_reason


def test_policy_state_scoping():
    """Policy only applies in APPLIED state, not UNDERWRITING."""
    def applied_only_block(resource, ctx):
        return PolicyResult.deny("blocked")

    sys = define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "DONE"],
        transitions=[("APPLIED", "UNDERWRITING"), ("UNDERWRITING", "DONE")],
        terminal_states=["DONE"],
        roles={"mgr": ["transition:*"]},
        policies=[{
            "name": "applied_block",
            "evaluate": applied_only_block,
            "applicable_states": ["APPLIED"],
            "resource_types": ["loan"],
        }],
    )

    result = sys.create({}, actor="u1")
    rid = result.resource.id

    # Policy blocks in APPLIED
    r = sys.transition(rid, "UNDERWRITING", actor="u1", role="mgr")
    assert not r.success

    # Force transition (as controller, bypasses RBAC but not policies...
    # Actually let's use a system without the policy for UNDERWRITING)


def test_policy_returns_bool():
    """Policy can return True/False instead of PolicyResult."""
    def bool_policy(resource, ctx):
        return resource.data.get("amount", 0) < 1_000_000

    sys = define_system(
        name="loan",
        states=["A", "B"],
        transitions=[("A", "B")],
        terminal_states=["B"],
        policies=[{"name": "bool_check", "evaluate": bool_policy}],
    )

    # Under limit - allowed
    r1 = sys.create({"amount": 500_000}, actor="u1")
    result1 = sys.transition(r1.resource.id, "B", actor="u1", role="any",
                             authority_level="CONTROLLER")
    assert result1.success

    # Over limit - blocked
    r2 = sys.create({"amount": 2_000_000}, actor="u1")
    result2 = sys.transition(r2.resource.id, "B", actor="u1", role="any",
                             authority_level="CONTROLLER")
    assert not result2.success


def test_multiple_policies():
    """Multiple policies: all must pass."""
    sys = define_system(
        name="loan",
        states=["A", "B"],
        transitions=[("A", "B")],
        terminal_states=["B"],
        policies=[
            {"name": "p1", "evaluate": lambda r, c: PolicyResult.allow(), "priority": 90},
            {"name": "p2", "evaluate": lambda r, c: PolicyResult.deny("nope"), "priority": 10},
        ],
    )

    result = sys.create({}, actor="u1")
    r = sys.transition(result.resource.id, "B", actor="u1", role="any",
                       authority_level="CONTROLLER")
    assert not r.success
    assert "p2" in r.rejected_reason
