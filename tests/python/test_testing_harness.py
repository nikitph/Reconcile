"""Coverage for the Python test harness helpers."""

import pytest

from reconcile import define_system
from reconcile.controller import Controller
from reconcile.testing import SystemTestHarness


@pytest.fixture
def harness_system():
    return define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "APPROVED"],
        transitions=[
            ("APPLIED", "UNDERWRITING"),
            ("UNDERWRITING", "APPROVED"),
        ],
        roles={
            "officer": ["view", "transition:UNDERWRITING"],
            "manager": ["view", "transition:*"],
            "viewer": ["view"],
        },
    )


def test_assert_transition_succeeds_returns_result(harness_system):
    created = harness_system.create({"amount": 100}, actor="u1")
    harness = SystemTestHarness(harness_system)

    result = harness.assert_transition_succeeds(
        created.resource.id,
        "UNDERWRITING",
        actor="u1",
        role="officer",
    )

    assert result.success is True
    assert harness_system.get(created.resource.id).state == "UNDERWRITING"


def test_assert_transition_succeeds_raises_with_rejection_details(harness_system):
    created = harness_system.create({"amount": 100}, actor="u1")
    harness = SystemTestHarness(harness_system)

    with pytest.raises(AssertionError, match="rejected at step 'check_role_permissions'"):
        harness.assert_transition_succeeds(
            created.resource.id,
            "UNDERWRITING",
            actor="u1",
            role="viewer",
        )


def test_assert_transition_blocked_validates_step(harness_system):
    created = harness_system.create({"amount": 100}, actor="u1")
    harness = SystemTestHarness(harness_system)

    result = harness.assert_transition_blocked(
        created.resource.id,
        "APPROVED",
        step="validate_state_machine",
        actor="u1",
        role="manager",
    )

    assert result.success is False


def test_assert_transition_blocked_raises_if_transition_succeeds(harness_system):
    created = harness_system.create({"amount": 100}, actor="u1")
    harness = SystemTestHarness(harness_system)

    with pytest.raises(AssertionError, match="to be blocked, but it succeeded"):
        harness.assert_transition_blocked(
            created.resource.id,
            "UNDERWRITING",
            actor="u1",
            role="officer",
        )


def test_assert_invariant_holds_returns_resource(harness_system):
    created = harness_system.create({"amount": 100}, actor="u1")
    harness = SystemTestHarness(harness_system)

    resource = harness.assert_invariant_holds(created.resource.id)

    assert resource.id == created.resource.id


def test_assert_state_checks_expected_state(harness_system):
    created = harness_system.create({"amount": 100}, actor="u1")
    harness = SystemTestHarness(harness_system)

    resource = harness.assert_state(created.resource.id, "APPLIED")

    assert resource.state == "APPLIED"


def test_assert_state_raises_for_wrong_state(harness_system):
    created = harness_system.create({"amount": 100}, actor="u1")
    harness = SystemTestHarness(harness_system)

    with pytest.raises(AssertionError, match="Expected state 'UNDERWRITING', got 'APPLIED'"):
        harness.assert_state(created.resource.id, "UNDERWRITING")


def test_controller_base_class_requires_reconcile():
    controller = Controller()

    with pytest.raises(NotImplementedError):
        controller.reconcile(None, None)
