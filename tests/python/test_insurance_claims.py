"""Real-world insurance claims processing scenario tests.

Models: claims filing, investigation, assessment, fraud detection,
approval caps, payment, and cross-claim exposure limits.
"""

import pytest
from reconcile import define_system, PolicyResult, InvariantResult


@pytest.fixture
def claims_system():
    """Insurance claims processing with fraud detection and exposure limits."""

    all_claims_total = {"value": 0.0}  # Closure-captured mutable state

    def auto_approval_cap(resource, ctx, query):
        """Claims > 500K cannot be auto-approved."""
        if ctx.get("to_state") == "APPROVED":
            amount = resource.data.get("amount", 0)
            if amount > 500_000:
                return PolicyResult.deny(
                    f"Claim {amount} exceeds auto-approval cap of 500K"
                )
        return PolicyResult.allow()

    def positive_claim_amount(resource, query):
        amount = resource.data.get("amount", 0)
        if amount <= 0:
            return InvariantResult.violated("Claim amount must be positive")
        return InvariantResult.ok()

    return define_system(
        name="claim",
        states=[
            "FILED", "UNDER_INVESTIGATION", "ASSESSED",
            "APPROVED", "PAID", "DENIED", "FRAUD_FLAGGED",
        ],
        transitions=[
            ("FILED", "UNDER_INVESTIGATION"),
            ("UNDER_INVESTIGATION", "ASSESSED"),
            ("UNDER_INVESTIGATION", "FRAUD_FLAGGED"),
            ("ASSESSED", "APPROVED"),
            ("ASSESSED", "DENIED"),
            ("APPROVED", "PAID"),
        ],
        terminal_states=["PAID", "DENIED", "FRAUD_FLAGGED"],
        roles={
            "adjuster": [
                "view", "transition:UNDER_INVESTIGATION",
                "transition:ASSESSED", "transition:FRAUD_FLAGGED",
            ],
            "supervisor": ["view", "transition:*"],
        },
        policies=[
            {
                "name": "auto_approval_cap",
                "description": "Claims > 500K need manual review",
                "evaluate": auto_approval_cap,
                "resource_types": ["claim"],
                "priority": 80,
            },
        ],
        invariants=[
            {
                "name": "positive_amount",
                "mode": "strong",
                "scope": "resource",
                "check": positive_claim_amount,
                "resource_types": ["claim"],
            },
        ],
    )


class TestClaimHappyPaths:
    def test_small_claim_full_lifecycle(self, claims_system):
        sys = claims_system
        result = sys.create(
            {"amount": 25_000, "type": "auto", "policy_number": "POL-001"},
            actor="claimant",
        )
        assert result.success
        rid = result.resource.id

        sys.transition(rid, "UNDER_INVESTIGATION", actor="adj1", role="adjuster")
        sys.transition(rid, "ASSESSED", actor="adj1", role="adjuster")
        sys.transition(rid, "APPROVED", actor="sup1", role="supervisor")
        r = sys.transition(rid, "PAID", actor="sup1", role="supervisor")
        assert r.success
        assert sys.get(rid).state == "PAID"

    def test_claim_denial(self, claims_system):
        sys = claims_system
        result = sys.create(
            {"amount": 100_000, "type": "home", "policy_number": "POL-002"},
            actor="claimant",
        )
        rid = result.resource.id
        sys.transition(rid, "UNDER_INVESTIGATION", actor="adj1", role="adjuster")
        sys.transition(rid, "ASSESSED", actor="adj1", role="adjuster")
        r = sys.transition(rid, "DENIED", actor="sup1", role="supervisor")
        assert r.success
        assert sys.get(rid).state == "DENIED"


class TestFraudDetection:
    def test_fraud_flagging_terminates(self, claims_system):
        sys = claims_system
        result = sys.create(
            {"amount": 1_000_000, "type": "life", "suspicious": True},
            actor="claimant",
        )
        rid = result.resource.id
        sys.transition(rid, "UNDER_INVESTIGATION", actor="adj1", role="adjuster")
        r = sys.transition(rid, "FRAUD_FLAGGED", actor="adj1", role="adjuster")
        assert r.success
        assert sys.get(rid).state == "FRAUD_FLAGGED"

    def test_fraud_flagged_is_truly_terminal(self, claims_system):
        sys = claims_system
        result = sys.create({"amount": 100_000}, actor="claimant")
        rid = result.resource.id
        sys.transition(rid, "UNDER_INVESTIGATION", actor="adj1", role="adjuster")
        sys.transition(rid, "FRAUD_FLAGGED", actor="adj1", role="adjuster")

        for target in ["FILED", "UNDER_INVESTIGATION", "ASSESSED", "APPROVED", "PAID"]:
            r = sys.transition(rid, target, actor="sup1", role="supervisor")
            assert not r.success, f"FRAUD_FLAGGED should not transition to {target}"

    def test_fraud_controller_auto_flags(self):
        """Controller that auto-flags suspicious claims."""

        def fraud_detector(resource, query):
            if resource.data.get("suspicious") and resource.state == "UNDER_INVESTIGATION":
                return {"transition": "FRAUD_FLAGGED"}
            return None

        sys = define_system(
            name="claim",
            states=["FILED", "UNDER_INVESTIGATION", "ASSESSED", "FRAUD_FLAGGED"],
            transitions=[
                ("FILED", "UNDER_INVESTIGATION"),
                ("UNDER_INVESTIGATION", "ASSESSED"),
                ("UNDER_INVESTIGATION", "FRAUD_FLAGGED"),
            ],
            terminal_states=["ASSESSED", "FRAUD_FLAGGED"],
            controllers=[{
                "name": "fraud-detector",
                "reconcile": fraud_detector,
                "on_events": ["claim.transitioned"],
                "priority": 90,
            }],
        )

        # Suspicious claim -> auto-flagged
        r1 = sys.create({"amount": 999_999, "suspicious": True}, actor="claimant")
        sys.transition(
            r1.resource.id, "UNDER_INVESTIGATION",
            actor="adj", role="adj", authority_level="CONTROLLER",
        )
        assert sys.get(r1.resource.id).state == "FRAUD_FLAGGED"

        # Clean claim -> stays in UNDER_INVESTIGATION
        r2 = sys.create({"amount": 5_000, "suspicious": False}, actor="claimant")
        sys.transition(
            r2.resource.id, "UNDER_INVESTIGATION",
            actor="adj", role="adj", authority_level="CONTROLLER",
        )
        assert sys.get(r2.resource.id).state == "UNDER_INVESTIGATION"


class TestClaimPolicies:
    def test_high_value_claim_blocked(self, claims_system):
        sys = claims_system
        result = sys.create({"amount": 750_000}, actor="claimant")
        rid = result.resource.id
        sys.transition(rid, "UNDER_INVESTIGATION", actor="adj1", role="adjuster")
        sys.transition(rid, "ASSESSED", actor="adj1", role="adjuster")

        r = sys.transition(rid, "APPROVED", actor="sup1", role="supervisor")
        assert not r.success
        assert "500K" in r.rejected_reason

    def test_small_claim_not_blocked(self, claims_system):
        sys = claims_system
        result = sys.create({"amount": 100_000}, actor="claimant")
        rid = result.resource.id
        sys.transition(rid, "UNDER_INVESTIGATION", actor="adj1", role="adjuster")
        sys.transition(rid, "ASSESSED", actor="adj1", role="adjuster")

        r = sys.transition(rid, "APPROVED", actor="sup1", role="supervisor")
        assert r.success


class TestClaimAudit:
    def test_complete_audit_trail(self, claims_system):
        sys = claims_system
        result = sys.create({"amount": 50_000}, actor="claimant")
        rid = result.resource.id

        sys.transition(rid, "UNDER_INVESTIGATION", actor="adj1", role="adjuster")
        sys.transition(rid, "ASSESSED", actor="adj1", role="adjuster")
        sys.transition(rid, "APPROVED", actor="sup1", role="supervisor")
        sys.transition(rid, "PAID", actor="sup1", role="supervisor")

        audit = sys.audit(rid)
        assert len(audit) == 4
        states = [(a.previous_state, a.new_state) for a in audit]
        assert states == [
            ("FILED", "UNDER_INVESTIGATION"),
            ("UNDER_INVESTIGATION", "ASSESSED"),
            ("ASSESSED", "APPROVED"),
            ("APPROVED", "PAID"),
        ]

    def test_audit_captures_different_actors(self, claims_system):
        sys = claims_system
        result = sys.create({"amount": 50_000}, actor="claimant")
        rid = result.resource.id

        sys.transition(rid, "UNDER_INVESTIGATION", actor="alice", role="adjuster")
        sys.transition(rid, "ASSESSED", actor="bob", role="adjuster")
        sys.transition(rid, "APPROVED", actor="charlie", role="supervisor")

        audit = sys.audit(rid)
        actors = [a.actor for a in audit]
        assert actors == ["alice", "bob", "charlie"]


class TestMultipleClaims:
    def test_independent_claim_processing(self, claims_system):
        sys = claims_system

        # Create 5 claims
        claim_ids = []
        for i in range(5):
            r = sys.create(
                {"amount": (i + 1) * 10_000, "policy_number": f"POL-{i:03d}"},
                actor=f"claimant_{i}",
            )
            claim_ids.append(r.resource.id)

        # Process them independently
        for rid in claim_ids:
            sys.transition(rid, "UNDER_INVESTIGATION", actor="adj", role="adjuster")

        # Approve only even-indexed claims
        for i, rid in enumerate(claim_ids):
            sys.transition(rid, "ASSESSED", actor="adj", role="adjuster")
            if i % 2 == 0:
                sys.transition(rid, "APPROVED", actor="sup", role="supervisor")
            else:
                sys.transition(rid, "DENIED", actor="sup", role="supervisor")

        # Verify states
        for i, rid in enumerate(claim_ids):
            resource = sys.get(rid)
            if i % 2 == 0:
                assert resource.state == "APPROVED"
            else:
                assert resource.state == "DENIED"

    def test_batch_audit_isolation(self, claims_system):
        sys = claims_system
        r1 = sys.create({"amount": 10_000}, actor="c1")
        r2 = sys.create({"amount": 20_000}, actor="c2")

        sys.transition(r1.resource.id, "UNDER_INVESTIGATION", actor="adj", role="adjuster")
        sys.transition(r1.resource.id, "ASSESSED", actor="adj", role="adjuster")

        assert len(sys.audit(r1.resource.id)) == 2
        assert len(sys.audit(r2.resource.id)) == 0
