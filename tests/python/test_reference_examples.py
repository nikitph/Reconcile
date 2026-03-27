"""Smoke tests for the reference domain examples."""

from reconcile.examples import (
    create_clinical_trials_system,
    create_loan_operating_system,
    create_procurement_system,
)


def test_loan_operating_system_reference_spec():
    system = create_loan_operating_system()

    spec = system.export_spec()
    loan_type = next(item for item in spec["types"] if item["name"] == "loan")

    assert len(loan_type["states"]) >= 10
    assert spec["policy_count"] >= 5
    assert spec["invariant_count"] >= 3
    assert spec["agent_count"] >= 2
    assert spec["decision_node_count"] >= 1


def test_procurement_example_builds():
    system = create_procurement_system()
    spec = system.export_spec()

    assert any(item["name"] == "purchase_request" for item in spec["types"])
    assert spec["policy_count"] >= 2


def test_clinical_trials_example_builds():
    system = create_clinical_trials_system()
    spec = system.export_spec()

    assert any(item["name"] == "trial" for item in spec["types"])
    assert spec["invariant_count"] >= 1
