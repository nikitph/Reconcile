"""Real-world loan origination scenario tests.

Models a complete banking loan lifecycle:
- Multi-level approval chain with role hierarchy
- Policies based on loan amount thresholds
- Invariants on data integrity
- Controllers for auto-escalation
- Full audit trail verification
"""

import pytest
from reconcile import define_system, PolicyResult, InvariantResult
from reconcile.testing import SystemTestHarness


@pytest.fixture
def loan_origination():
    """Full loan origination system with 7 states, 4 roles, policies, invariants."""

    def high_value_escalation(resource, ctx):
        """Loans > 50L require senior review before approval."""
        amount = resource.data.get("amount", 0)
        if ctx.get("to_state") == "APPROVED" and ctx.get("from_state") == "UNDERWRITING":
            if amount > 5_000_000:
                return PolicyResult.deny(
                    f"Loan amount {amount} exceeds 50L auto-approval limit. Route through SENIOR_REVIEW."
                )
        return PolicyResult.allow()

    def minimum_amount(resource):
        """Loan amount must be positive."""
        amount = resource.data.get("amount", 0)
        if amount <= 0:
            return InvariantResult.violated(f"Loan amount must be positive, got {amount}")
        return InvariantResult.ok()

    def docs_required_for_uw(resource):
        """Underwriting requires document list."""
        if resource.state == "UNDERWRITING":
            docs = resource.data.get("documents", [])
            if not docs:
                return InvariantResult.violated("Underwriting requires at least one document")
        return InvariantResult.ok()

    return define_system(
        name="loan",
        states=[
            "APPLIED", "DOCUMENT_CHECK", "UNDERWRITING",
            "SENIOR_REVIEW", "APPROVED", "DISBURSED", "REJECTED",
        ],
        transitions=[
            ("APPLIED", "DOCUMENT_CHECK"),
            ("DOCUMENT_CHECK", "UNDERWRITING"),
            ("DOCUMENT_CHECK", "REJECTED"),
            ("UNDERWRITING", "APPROVED"),
            ("UNDERWRITING", "SENIOR_REVIEW"),
            ("UNDERWRITING", "REJECTED"),
            ("SENIOR_REVIEW", "APPROVED"),
            ("SENIOR_REVIEW", "REJECTED"),
            ("APPROVED", "DISBURSED"),
        ],
        terminal_states=["DISBURSED", "REJECTED"],
        roles={
            "clerk": ["view", "transition:DOCUMENT_CHECK"],
            "officer": ["view", "transition:UNDERWRITING", "transition:REJECTED"],
            "senior_officer": [
                "view", "transition:APPROVED", "transition:SENIOR_REVIEW", "transition:REJECTED",
            ],
            "manager": ["view", "transition:*"],
        },
        policies=[
            {
                "name": "high_value_escalation",
                "description": "Loans > 50L need senior review",
                "evaluate": high_value_escalation,
                "applicable_states": ["UNDERWRITING"],
                "resource_types": ["loan"],
                "priority": 90,
            },
        ],
        invariants=[
            {
                "name": "minimum_amount",
                "description": "Loan amount must be positive",
                "mode": "strong",
                "scope": "resource",
                "check": minimum_amount,
                "resource_types": ["loan"],
            },
            {
                "name": "docs_required_for_uw",
                "description": "Docs needed for underwriting",
                "mode": "strong",
                "scope": "transition",
                "check": docs_required_for_uw,
                "resource_types": ["loan"],
            },
        ],
    )


class TestLoanHappyPaths:
    """Standard loan lifecycle paths."""

    def test_small_loan_direct_approval(self, loan_origination):
        """Small loan: APPLIED -> DOC_CHECK -> UW -> APPROVED -> DISBURSED"""
        sys = loan_origination
        result = sys.create(
            {"amount": 500_000, "applicant": "Small Corp", "documents": ["id_proof", "income"]},
            actor="applicant1",
        )
        assert result.success
        rid = result.resource.id

        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        sys.transition(rid, "UNDERWRITING", actor="o1", role="officer")
        sys.transition(rid, "APPROVED", actor="sr1", role="senior_officer")
        r = sys.transition(rid, "DISBURSED", actor="mgr1", role="manager")
        assert r.success

        final = sys.get(rid)
        assert final.state == "DISBURSED"
        assert final.version == 5
        assert final.data["applicant"] == "Small Corp"

    def test_high_value_loan_with_senior_review(self, loan_origination):
        """High-value loan must route through SENIOR_REVIEW."""
        sys = loan_origination
        result = sys.create(
            {"amount": 10_000_000, "applicant": "Big Corp", "documents": ["full_audit"]},
            actor="app1",
        )
        rid = result.resource.id

        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        sys.transition(rid, "UNDERWRITING", actor="o1", role="officer")

        # Direct approval should be BLOCKED by policy
        r = sys.transition(rid, "APPROVED", actor="sr1", role="senior_officer")
        assert not r.success
        assert r.rejected_step == "evaluate_policies"
        assert "50L" in r.rejected_reason

        # Must go through senior review
        r = sys.transition(rid, "SENIOR_REVIEW", actor="sr1", role="senior_officer")
        assert r.success

        # Now approve from senior review
        r = sys.transition(rid, "APPROVED", actor="sr1", role="senior_officer")
        assert r.success
        assert sys.get(rid).state == "APPROVED"


class TestLoanRejections:
    """Rejection paths at various stages."""

    def test_rejection_at_document_check(self, loan_origination):
        sys = loan_origination
        result = sys.create(
            {"amount": 100_000, "applicant": "Bad Docs LLC", "documents": ["expired"]},
            actor="app1",
        )
        rid = result.resource.id
        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        r = sys.transition(rid, "REJECTED", actor="o1", role="officer")
        assert r.success
        assert sys.get(rid).state == "REJECTED"

    def test_rejection_at_underwriting(self, loan_origination):
        sys = loan_origination
        result = sys.create(
            {"amount": 100_000, "applicant": "Risky Inc", "documents": ["valid"]},
            actor="app1",
        )
        rid = result.resource.id
        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        sys.transition(rid, "UNDERWRITING", actor="o1", role="officer")
        r = sys.transition(rid, "REJECTED", actor="o1", role="officer")
        assert r.success

    def test_rejection_at_senior_review(self, loan_origination):
        sys = loan_origination
        result = sys.create(
            {"amount": 10_000_000, "applicant": "Too Risky Corp", "documents": ["full"]},
            actor="app1",
        )
        rid = result.resource.id
        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        sys.transition(rid, "UNDERWRITING", actor="o1", role="officer")
        sys.transition(rid, "SENIOR_REVIEW", actor="sr1", role="senior_officer")
        r = sys.transition(rid, "REJECTED", actor="sr1", role="senior_officer")
        assert r.success
        assert sys.get(rid).state == "REJECTED"

    def test_cannot_transition_from_rejected(self, loan_origination):
        sys = loan_origination
        result = sys.create(
            {"amount": 100_000, "applicant": "Test", "documents": ["doc"]},
            actor="app1",
        )
        rid = result.resource.id
        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        sys.transition(rid, "REJECTED", actor="o1", role="officer")

        # Terminal - cannot transition anywhere
        for target in ["APPLIED", "DOCUMENT_CHECK", "UNDERWRITING", "APPROVED"]:
            r = sys.transition(rid, target, actor="mgr", role="manager")
            assert not r.success, f"Should not transition from REJECTED to {target}"


class TestLoanRBAC:
    """Role-based access control enforcement."""

    def test_clerk_only_doc_check(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        # Clerk CAN move to DOCUMENT_CHECK
        r = sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        assert r.success

        # Clerk CANNOT move to UNDERWRITING
        r = sys.transition(rid, "UNDERWRITING", actor="c1", role="clerk")
        assert not r.success
        assert r.rejected_step == "check_role_permissions"

    def test_officer_cannot_approve(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id
        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        sys.transition(rid, "UNDERWRITING", actor="o1", role="officer")

        r = sys.transition(rid, "APPROVED", actor="o1", role="officer")
        assert not r.success
        assert r.rejected_step == "check_role_permissions"

    def test_manager_can_do_everything(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        for target in ["DOCUMENT_CHECK", "UNDERWRITING", "APPROVED", "DISBURSED"]:
            r = sys.transition(rid, target, actor="mgr", role="manager")
            assert r.success, f"Manager should transition to {target}"

    def test_controller_authority_bypasses_rbac(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        # Controller can transition without role check
        r = sys.transition(
            rid, "DOCUMENT_CHECK", actor="auto-ctrl", role="none",
            authority_level="CONTROLLER",
        )
        assert r.success


class TestLoanInvariants:
    """Invariant enforcement."""

    def test_negative_amount_blocked(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": -500, "documents": ["doc"]}, actor="app")
        assert not result.success
        assert result.rejected_step == "verify_invariants"
        assert "positive" in result.rejected_reason.lower()

    def test_zero_amount_blocked(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 0, "documents": ["doc"]}, actor="app")
        assert not result.success

    def test_missing_docs_blocks_underwriting(self, loan_origination):
        """Underwriting state requires documents."""
        sys = loan_origination
        result = sys.create({"amount": 100_000}, actor="app")  # No documents
        rid = result.resource.id
        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")

        r = sys.transition(rid, "UNDERWRITING", actor="o1", role="officer")
        assert not r.success
        assert r.rejected_step == "verify_invariants"
        assert "document" in r.rejected_reason.lower()


class TestLoanAuditTrail:
    """Audit trail completeness and accuracy."""

    def test_audit_captures_all_transitions(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        actors = [("c1", "clerk"), ("o1", "officer"), ("sr1", "senior_officer"), ("mgr", "manager")]
        targets = ["DOCUMENT_CHECK", "UNDERWRITING", "APPROVED", "DISBURSED"]

        for (actor, role), target in zip(actors, targets):
            sys.transition(rid, target, actor=actor, role=role)

        audit = sys.audit(rid)
        assert len(audit) == 4

        for i, ((actor, role), target) in enumerate(zip(actors, targets)):
            assert audit[i].actor == actor
            assert audit[i].role == role
            assert audit[i].new_state == target

    def test_audit_authority_levels(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        # Human transition
        sys.transition(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        # Controller transition
        sys.transition(
            rid, "UNDERWRITING", actor="auto-ctrl", role="sys",
            authority_level="CONTROLLER",
        )

        audit = sys.audit(rid)
        assert audit[0].authority_level == "HUMAN"
        assert audit[1].authority_level == "CONTROLLER"

    def test_rejected_transitions_leave_no_audit(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        # Attempt invalid transition
        sys.transition(rid, "APPROVED", actor="c1", role="clerk")

        # No audit entry
        assert len(sys.audit(rid)) == 0


class TestLoanDesiredState:
    """Desired state reconciliation for loans."""

    def test_reconcile_to_disbursed(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        sys.set_desired(rid, "DISBURSED", requested_by="mgr1")
        assert sys.get(rid).state == "DISBURSED"

    def test_reconcile_partial_path(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        sys.set_desired(rid, "UNDERWRITING", requested_by="mgr1")
        assert sys.get(rid).state == "UNDERWRITING"

    def test_reconcile_generates_audit(self, loan_origination):
        sys = loan_origination
        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        sys.set_desired(rid, "APPROVED", requested_by="mgr1")

        audit = sys.audit(rid)
        # Should have audit for each intermediate transition
        assert len(audit) >= 3  # DOC_CHECK, UW, APPROVED


class TestLoanTestHarness:
    """SystemTestHarness on loan origination."""

    def test_harness_workflow(self, loan_origination):
        sys = loan_origination
        harness = SystemTestHarness(sys)

        result = sys.create({"amount": 100_000, "documents": ["doc"]}, actor="app")
        rid = result.resource.id

        harness.assert_state(rid, "APPLIED")
        harness.assert_transition_succeeds(rid, "DOCUMENT_CHECK", actor="c1", role="clerk")
        harness.assert_state(rid, "DOCUMENT_CHECK")
        # Clerk can't move to UNDERWRITING (only officer/manager can)
        harness.assert_transition_blocked(
            rid, "UNDERWRITING", step="check_role_permissions", actor="c1", role="clerk",
        )
        harness.assert_transition_succeeds(rid, "UNDERWRITING", actor="o1", role="officer")
        harness.assert_transition_succeeds(rid, "APPROVED", actor="sr1", role="senior_officer")
        harness.assert_transition_succeeds(rid, "DISBURSED", actor="mgr", role="manager")
        harness.assert_state(rid, "DISBURSED")
