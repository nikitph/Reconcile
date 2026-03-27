# Reconcile

Reconcile is a governance runtime for enterprise workflows.

You define a stateful domain model once, then execute every transition through a
single kernel that enforces:

- state machines
- role-based permissions
- policies and compliance rules
- invariants
- audit trails
- graph relationships
- controller automation
- agent recommendations

It ships as a Python package backed by a Rust core.

## Install

```bash
pip install reconcile-framework
```

For the HTTP adapters:

```bash
pip install "reconcile-framework[api]"
```

## A 50-Line Enterprise System

```python
from reconcile import define_system, PolicyResult, InvariantResult


def high_value_requires_review(resource, ctx, query):
    if ctx.get("to_state") == "APPROVED" and resource.data.get("amount", 0) > 2_500_000:
        return PolicyResult.deny("Loans above ₹25L must go through SENIOR_REVIEW")
    return PolicyResult.allow()


def positive_amount(resource, query):
    if resource.data.get("amount", 0) <= 0:
        return InvariantResult.violated("amount must be positive")
    return InvariantResult.ok()


loan_os = define_system(
    name="loan",
    states=[
        "DRAFT", "APPLIED", "DOCS", "UNDERWRITING", "SENIOR_REVIEW",
        "APPROVED", "DISBURSED", "REPAYING", "CLOSED", "REJECTED",
    ],
    transitions=[
        ("DRAFT", "APPLIED"),
        ("APPLIED", "DOCS"),
        ("DOCS", "UNDERWRITING"),
        ("UNDERWRITING", "APPROVED"),
        ("UNDERWRITING", "SENIOR_REVIEW"),
        ("UNDERWRITING", "REJECTED"),
        ("SENIOR_REVIEW", "APPROVED"),
        ("SENIOR_REVIEW", "REJECTED"),
        ("APPROVED", "DISBURSED"),
        ("DISBURSED", "REPAYING"),
        ("REPAYING", "CLOSED"),
    ],
    terminal_states=["CLOSED", "REJECTED"],
    roles={
        "data_entry": ["view", "transition:APPLIED"],
        "doc_officer": ["view", "transition:DOCS", "transition:UNDERWRITING"],
        "underwriter": ["view", "transition:APPROVED", "transition:SENIOR_REVIEW", "transition:REJECTED"],
        "senior_underwriter": ["view", "transition:*"],
        "branch_manager": ["view", "transition:DISBURSED", "transition:REPAYING", "transition:CLOSED"],
    },
    policies=[{
        "name": "high_value_requires_review",
        "description": "RBI-style maker-checker threshold",
        "evaluate": high_value_requires_review,
        "applicable_states": ["UNDERWRITING"],
        "resource_types": ["loan"],
        "priority": 90,
    }],
    invariants=[{
        "name": "positive_amount",
        "description": "loan principal must be positive",
        "mode": "strong",
        "scope": "resource",
        "check": positive_amount,
        "resource_types": ["loan"],
    }],
)
```

That system is immediately usable:

```python
loan = loan_os.create({"amount": 800_000, "purpose": "working_capital"}, actor="maker-1")
loan_os.transition(loan.resource.id, "APPLIED", actor="maker-1", role="data_entry")
projection = loan_os.project(loan.resource.id, "underwriter")
print(projection.to_json())
```

## Reference Implementations

- Lending: `reconcile.examples.create_loan_operating_system()`
- Procurement: `reconcile.examples.create_procurement_system()`
- Clinical trials: `reconcile.examples.create_clinical_trials_system()`

The lending example is the full reference implementation:

- 13 loan states
- 7 roles
- RBI-style policies
- graph-enforced borrower exposure limits
- agent-based risk and fraud recommendations
- decision-node auto-underwriting
- supporting applicant and collateral resource types

## FastAPI Adapters

Single-system API:

```python
from reconcile.api import create_app
from reconcile.examples import create_loan_operating_system

system = create_loan_operating_system().native
app = create_app(system)
```

Multi-app platform:

```python
from reconcile import ReconcilePlatform
from reconcile.api import create_platform_app
from reconcile.examples import (
    create_loan_operating_system,
    create_procurement_system,
)

platform = ReconcilePlatform()
platform.register_app("lending", create_loan_operating_system())
platform.register_app("procurement", create_procurement_system())
app = create_platform_app(platform)
```

## Packaging Notes

- Core runtime: Rust + PyO3 via `maturin`
- Python package source: `python/reconcile`
- Native extension module: `reconcile._native`
- Tests: `./.venv/bin/python -m pytest tests/python -q`
