"""Shared fixtures for Reconcile tests."""

import pytest
from reconcile import define_system, PolicyResult, InvariantResult


@pytest.fixture
def loan_system():
    """A loan lifecycle system with roles, policies, and invariants."""
    return define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED", "REJECTED"],
        transitions=[
            ("APPLIED", "UNDERWRITING"),
            ("UNDERWRITING", "APPROVED"),
            ("UNDERWRITING", "REJECTED"),
            ("APPROVED", "DISBURSED"),
        ],
        terminal_states=["DISBURSED", "REJECTED"],
        roles={
            "officer": ["view", "transition:UNDERWRITING"],
            "manager": ["view", "transition:*"],
            "viewer": ["view"],
        },
    )


@pytest.fixture
def loan_system_with_policies():
    """Loan system with a high-value policy."""

    def high_value_check(resource, ctx):
        amount = resource.data.get("amount", 0)
        if amount > 5_000_000:
            return PolicyResult.deny(f"Loan amount {amount} exceeds 50L limit")
        return PolicyResult.allow()

    return define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED", "REJECTED"],
        transitions=[
            ("APPLIED", "UNDERWRITING"),
            ("UNDERWRITING", "APPROVED"),
            ("UNDERWRITING", "REJECTED"),
            ("APPROVED", "DISBURSED"),
        ],
        terminal_states=["DISBURSED", "REJECTED"],
        roles={
            "officer": ["view", "transition:UNDERWRITING"],
            "manager": ["view", "transition:*"],
        },
        policies=[
            {
                "name": "high_value_limit",
                "description": "Loans > 50L need committee approval",
                "evaluate": high_value_check,
                "applicable_states": ["APPLIED"],
                "resource_types": ["loan"],
                "priority": 50,
            }
        ],
    )


@pytest.fixture
def loan_system_with_invariants():
    """Loan system with a strong invariant."""

    def positive_amount(resource):
        amount = resource.data.get("amount", 0)
        if amount > 0:
            return InvariantResult.ok()
        return InvariantResult.violated("Amount must be positive")

    return define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED", "REJECTED"],
        transitions=[
            ("APPLIED", "UNDERWRITING"),
            ("UNDERWRITING", "APPROVED"),
            ("UNDERWRITING", "REJECTED"),
            ("APPROVED", "DISBURSED"),
        ],
        terminal_states=["DISBURSED", "REJECTED"],
        roles={"manager": ["view", "transition:*"]},
        invariants=[
            {
                "name": "positive_amount",
                "description": "Loan amount must be positive",
                "mode": "strong",
                "scope": "resource",
                "check": positive_amount,
                "resource_types": ["loan"],
            }
        ],
    )
