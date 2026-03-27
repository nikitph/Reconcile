#!/usr/bin/env python3
"""Executable script for the packaged loan operating system example."""

from reconcile.examples.loan_operating_system import create_loan_operating_system


def demo():
    system = create_loan_operating_system()
    native = system.native

    applicant = native.create("applicant", {"name": "Acme Manufacturing Pvt Ltd"}, "system", "SYSTEM")
    collateral = native.create(
        "collateral",
        {"type": "commercial_property", "estimated_value": 25_000_000},
        "system",
        "SYSTEM",
    )
    loan = system.create({
        "amount": 800_000,
        "purpose": "working_capital",
        "interest_rate": 12.5,
        "bureau_score": 742,
        "debt_to_income": 0.28,
        "applicant_id": applicant.resource.id,
        "collateral_id": collateral.resource.id,
    }, actor="maker-1")

    for state, actor, role in [
        ("APPLIED", "maker-1", "data_entry"),
        ("KYC_REVIEW", "kyc-1", "kyc_officer"),
        ("DOCUMENT_VERIFICATION", "kyc-1", "kyc_officer"),
        ("CREDIT_BUREAU_CHECK", "doc-1", "document_officer"),
        ("UNDERWRITING", "doc-1", "document_officer"),
        ("APPROVED", "uw-1", "underwriter"),
        ("DISBURSED", "bm-1", "branch_manager"),
    ]:
        system.transition(loan.resource.id, state, actor=actor, role=role)

    print(system.project(loan.resource.id, "branch_manager").to_json())


if __name__ == "__main__":
    demo()
