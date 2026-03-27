"""Packaged reference systems for common domains."""

from reconcile.examples.loan_operating_system import create_loan_operating_system
from reconcile.examples.procurement_system import create_procurement_system
from reconcile.examples.clinical_trials_system import create_clinical_trials_system

__all__ = [
    "create_loan_operating_system",
    "create_procurement_system",
    "create_clinical_trials_system",
]
