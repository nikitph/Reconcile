"""Tests for convergence guarantees."""

import pytest
from reconcile import define_system, InvariantResult


def test_convergence_reduces_distance(loan_system):
    """Each reconciliation step should get closer to desired state."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    loan_system.set_desired(rid, "DISBURSED", requested_by="mgr")

    resource = loan_system.get(rid)
    assert resource.state == "DISBURSED"

    # Verify monotonic progression through audit
    audit = loan_system.audit(rid)
    states = [a.new_state for a in audit]
    assert states == ["UNDERWRITING", "APPROVED", "DISBURSED"]


def test_unreachable_desired_state_fails():
    """Setting desired state to unreachable state should fail."""
    sys = define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED", "REJECTED"],
        transitions=[
            ("APPLIED", "UNDERWRITING"),
            ("UNDERWRITING", "APPROVED"),
            ("UNDERWRITING", "REJECTED"),
            ("APPROVED", "DISBURSED"),
        ],
        terminal_states=["DISBURSED", "REJECTED"],
    )

    result = sys.create({}, actor="u1")
    rid = result.resource.id

    # Move to REJECTED (terminal)
    sys.transition(rid, "UNDERWRITING", actor="u1", role="x", authority_level="CONTROLLER")
    sys.transition(rid, "REJECTED", actor="u1", role="x", authority_level="CONTROLLER")

    # Try to set desired to DISBURSED from REJECTED (terminal, no outbound)
    with pytest.raises(Exception):
        sys.set_desired(rid, "DISBURSED", requested_by="mgr")


def test_desired_state_invalid_state_fails(loan_system):
    """Setting desired state to non-existent state should fail."""
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id

    with pytest.raises(Exception):
        loan_system.set_desired(rid, "NONEXISTENT", requested_by="mgr")


def test_system_test_harness():
    """Test the SystemTestHarness helper."""
    from reconcile.testing import SystemTestHarness

    sys = define_system(
        name="ticket",
        states=["OPEN", "IN_PROGRESS", "CLOSED"],
        transitions=[("OPEN", "IN_PROGRESS"), ("IN_PROGRESS", "CLOSED")],
        terminal_states=["CLOSED"],
        roles={"agent": ["transition:*"]},
    )

    harness = SystemTestHarness(sys)

    result = sys.create({"title": "Bug"}, actor="u1")
    rid = result.resource.id

    harness.assert_state(rid, "OPEN")
    harness.assert_transition_succeeds(rid, "IN_PROGRESS", actor="u1", role="agent")
    harness.assert_state(rid, "IN_PROGRESS")
    harness.assert_transition_blocked(rid, "OPEN", actor="u1", role="agent",
                                      step="validate_state_machine")
    harness.assert_transition_succeeds(rid, "CLOSED", actor="u1", role="agent")
    harness.assert_state(rid, "CLOSED")
