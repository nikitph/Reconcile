"""Edge case and stress tests.

Tests boundary conditions, unusual state machines, error handling,
data integrity, and defensive behavior.
"""

import pytest
from reconcile import define_system, PolicyResult, InvariantResult


class TestStateMachineEdgeCases:
    def test_self_loop_transition(self):
        """Resource can transition to its own state (retry pattern)."""
        sys = define_system(
            name="job",
            states=["PROCESSING", "DONE"],
            transitions=[("PROCESSING", "PROCESSING"), ("PROCESSING", "DONE")],
            terminal_states=["DONE"],
        )
        result = sys.create({}, actor="sys")
        rid = result.resource.id

        # Self-loop 3 times
        for _ in range(3):
            r = sys.transition(rid, "PROCESSING", actor="worker", role="x",
                               authority_level="CONTROLLER")
            assert r.success

        assert sys.get(rid).state == "PROCESSING"
        assert sys.get(rid).version == 4  # 1 create + 3 self-loops

        # Then complete
        r = sys.transition(rid, "DONE", actor="worker", role="x",
                           authority_level="CONTROLLER")
        assert r.success

    def test_diamond_state_machine(self):
        """Two paths converge: A -> (B or C) -> D."""
        sys = define_system(
            name="item",
            states=["A", "B", "C", "D"],
            transitions=[("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")],
            terminal_states=["D"],
        )

        # Path 1: A -> B -> D
        r1 = sys.create({}, actor="sys")
        sys.transition(r1.resource.id, "B", actor="x", role="x",
                       authority_level="CONTROLLER")
        sys.transition(r1.resource.id, "D", actor="x", role="x",
                       authority_level="CONTROLLER")
        assert sys.get(r1.resource.id).state == "D"

        # Path 2: A -> C -> D
        r2 = sys.create({}, actor="sys")
        sys.transition(r2.resource.id, "C", actor="x", role="x",
                       authority_level="CONTROLLER")
        sys.transition(r2.resource.id, "D", actor="x", role="x",
                       authority_level="CONTROLLER")
        assert sys.get(r2.resource.id).state == "D"

    def test_review_revision_cycle(self):
        """Cycle: REVIEW <-> REVISION, exit via APPROVED."""
        sys = define_system(
            name="document",
            states=["DRAFT", "REVIEW", "REVISION", "APPROVED"],
            transitions=[
                ("DRAFT", "REVIEW"),
                ("REVIEW", "REVISION"),
                ("REVISION", "REVIEW"),
                ("REVIEW", "APPROVED"),
            ],
            terminal_states=["APPROVED"],
        )

        result = sys.create({"title": "RFC"}, actor="author")
        rid = result.resource.id

        sys.transition(rid, "REVIEW", actor="author", role="x",
                       authority_level="CONTROLLER")
        # Bounce between review and revision
        sys.transition(rid, "REVISION", actor="reviewer", role="x",
                       authority_level="CONTROLLER")
        sys.transition(rid, "REVIEW", actor="author", role="x",
                       authority_level="CONTROLLER")
        sys.transition(rid, "REVISION", actor="reviewer", role="x",
                       authority_level="CONTROLLER")
        sys.transition(rid, "REVIEW", actor="author", role="x",
                       authority_level="CONTROLLER")
        # Finally approve
        r = sys.transition(rid, "APPROVED", actor="reviewer", role="x",
                           authority_level="CONTROLLER")
        assert r.success
        assert sys.get(rid).state == "APPROVED"
        assert sys.get(rid).version == 7  # create + 6 transitions

    def test_single_terminal_state(self):
        """Simplest possible system: one terminal state."""
        sys = define_system(
            name="flag",
            states=["SET"],
            transitions=[],
            terminal_states=["SET"],
        )
        result = sys.create({}, actor="sys")
        assert result.success
        assert result.resource.state == "SET"

    def test_many_outbound_transitions(self):
        """State with 10 outbound transitions."""
        targets = [f"T{i}" for i in range(10)]
        all_states = ["START"] + targets
        all_transitions = [("START", t) for t in targets]

        sys = define_system(
            name="router",
            states=all_states,
            transitions=all_transitions,
            terminal_states=targets,
        )

        for target in targets:
            r = sys.create({}, actor="sys")
            result = sys.transition(
                r.resource.id, target, actor="x", role="x",
                authority_level="CONTROLLER",
            )
            assert result.success
            assert sys.get(r.resource.id).state == target


class TestDataIntegrity:
    def test_complex_nested_data(self):
        sys = define_system(
            name="order",
            states=["DRAFT", "SUBMITTED"],
            transitions=[("DRAFT", "SUBMITTED")],
            terminal_states=["SUBMITTED"],
        )

        data = {
            "customer": {"name": "John", "address": {"city": "Mumbai", "zip": "400001"}},
            "items": [
                {"sku": "A001", "qty": 2, "price": 499.99},
                {"sku": "B002", "qty": 1, "price": 1299.00},
            ],
            "total": 2298.98,
            "tags": ["rush", "premium"],
            "notes": None,
        }

        result = sys.create(data, actor="customer")
        rid = result.resource.id
        sys.transition(rid, "SUBMITTED", actor="sys", role="x",
                       authority_level="CONTROLLER")

        final = sys.get(rid)
        assert final.data["customer"]["address"]["city"] == "Mumbai"
        assert len(final.data["items"]) == 2
        assert final.data["items"][0]["price"] == 499.99
        assert final.data["tags"] == ["rush", "premium"]
        assert final.data["notes"] is None

    def test_empty_data(self):
        sys = define_system(
            name="item", states=["A", "B"],
            transitions=[("A", "B")], terminal_states=["B"],
        )
        result = sys.create({}, actor="sys")
        assert result.success
        assert result.resource.data == {}

    def test_large_data_payload(self):
        sys = define_system(
            name="item", states=["A", "B"],
            transitions=[("A", "B")], terminal_states=["B"],
        )
        # 1000-element list
        data = {"items": list(range(1000)), "matrix": [[i * j for j in range(10)] for i in range(10)]}
        result = sys.create(data, actor="sys")
        assert result.success
        assert len(result.resource.data["items"]) == 1000

    def test_unicode_data(self):
        sys = define_system(
            name="item", states=["A"], transitions=[], terminal_states=["A"],
        )
        data = {
            "name": "Acme Corp",
            "description": "Test with special chars: <>&\"'",
            "emoji": "Test data",
        }
        result = sys.create(data, actor="sys")
        assert result.success
        assert result.resource.data["name"] == "Acme Corp"


class TestErrorHandling:
    def test_invalid_resource_id(self):
        sys = define_system(
            name="item", states=["A"], transitions=[], terminal_states=["A"],
        )
        with pytest.raises(Exception):
            sys.transition("not-a-uuid", "A", actor="x", role="x")

    def test_nonexistent_resource_id(self):
        sys = define_system(
            name="item", states=["A"], transitions=[], terminal_states=["A"],
        )
        r = sys.get("00000000-0000-0000-0000-000000000000")
        assert r is None

    def test_invalid_authority_level(self):
        sys = define_system(
            name="item", states=["A", "B"],
            transitions=[("A", "B")], terminal_states=["B"],
        )
        result = sys.create({}, actor="sys")
        with pytest.raises(Exception):
            sys.transition(
                result.resource.id, "B",
                actor="x", role="x", authority_level="INVALID",
            )

    def test_transition_after_rejection_state_unchanged(self):
        sys = define_system(
            name="item", states=["A", "B"],
            transitions=[("A", "B")], terminal_states=["B"],
            roles={"reader": ["view"]},
        )
        result = sys.create({}, actor="sys")
        rid = result.resource.id

        # Rejected transition
        r = sys.transition(rid, "B", actor="x", role="reader")
        assert not r.success

        # State unchanged
        assert sys.get(rid).state == "A"
        assert sys.get(rid).version == 1

    def test_policy_exception_treated_as_deny(self):
        """If a policy callback raises, it should deny."""
        def buggy_policy(resource, ctx, query):
            raise ValueError("Unexpected error in policy")

        sys = define_system(
            name="item", states=["A", "B"],
            transitions=[("A", "B")], terminal_states=["B"],
            policies=[{"name": "buggy", "evaluate": buggy_policy}],
        )
        result = sys.create({}, actor="sys")
        r = sys.transition(
            result.resource.id, "B", actor="x", role="x",
            authority_level="CONTROLLER",
        )
        assert not r.success
        assert r.rejected_step == "evaluate_policies"

    def test_invariant_exception_treated_as_violation(self):
        """If an invariant callback raises, it should be treated as violated."""
        def buggy_invariant(resource, query):
            raise RuntimeError("Unexpected error in invariant")

        sys = define_system(
            name="item", states=["A", "B"],
            transitions=[("A", "B")], terminal_states=["B"],
            invariants=[{
                "name": "buggy",
                "mode": "strong",
                "scope": "resource",
                "check": buggy_invariant,
            }],
        )
        result = sys.create({}, actor="sys")
        assert not result.success
        assert result.rejected_step == "verify_invariants"


class TestMultiResourceType:
    """Test systems with multiple resource types."""

    def test_two_resource_types_independent(self):
        from reconcile._native import ReconcileSystem

        sys = ReconcileSystem()
        sys.register_type(
            "loan", ["APPLIED", "APPROVED"],
            [("APPLIED", "APPROVED")], "APPLIED", ["APPROVED"],
        )
        sys.register_type(
            "claim", ["FILED", "PAID"],
            [("FILED", "PAID")], "FILED", ["PAID"],
        )

        loan = sys.create("loan", {"amount": 100}, "sys", "SYSTEM")
        claim = sys.create("claim", {"amount": 50}, "sys", "SYSTEM")

        assert loan.resource.resource_type == "loan"
        assert claim.resource.resource_type == "claim"

        r1 = sys.transition(loan.resource.id, "APPROVED", "sys", "sys", "CONTROLLER")
        r2 = sys.transition(claim.resource.id, "PAID", "sys", "sys", "CONTROLLER")

        assert r1.success
        assert r2.success

        assert sys.get(loan.resource.id).state == "APPROVED"
        assert sys.get(claim.resource.id).state == "PAID"


class TestControllerEdgeCases:
    def test_controller_on_creation_and_transition(self):
        """Controller subscribes to both creation and transition events."""
        log = []

        def logging_ctrl(resource, query):
            log.append(f"{resource.state}")
            return None

        sys = define_system(
            name="item",
            states=["A", "B", "C"],
            transitions=[("A", "B"), ("B", "C")],
            terminal_states=["C"],
            controllers=[{
                "name": "logger",
                "reconcile": logging_ctrl,
                "on_events": ["item.*"],
            }],
        )

        result = sys.create({}, actor="sys")
        rid = result.resource.id
        sys.transition(rid, "B", actor="x", role="x", authority_level="CONTROLLER")
        sys.transition(rid, "C", actor="x", role="x", authority_level="CONTROLLER")

        assert len(log) >= 3  # created + 2 transitions

    def test_controller_conditional_on_data(self):
        """Controller that acts based on resource data."""
        def score_based(resource, query):
            score = resource.data.get("risk_score", 0)
            if resource.state == "SCORING" and score > 0.8:
                return {"transition": "HIGH_RISK"}
            elif resource.state == "SCORING":
                return {"transition": "LOW_RISK"}
            return None

        sys = define_system(
            name="app",
            states=["NEW", "SCORING", "HIGH_RISK", "LOW_RISK"],
            transitions=[
                ("NEW", "SCORING"),
                ("SCORING", "HIGH_RISK"),
                ("SCORING", "LOW_RISK"),
            ],
            terminal_states=["HIGH_RISK", "LOW_RISK"],
            controllers=[{
                "name": "risk-router",
                "reconcile": score_based,
                "on_events": ["app.transitioned"],
                "priority": 80,
            }],
        )

        # High risk
        r1 = sys.create({"risk_score": 0.95}, actor="sys")
        sys.transition(r1.resource.id, "SCORING", actor="x", role="x",
                       authority_level="CONTROLLER")
        assert sys.get(r1.resource.id).state == "HIGH_RISK"

        # Low risk
        r2 = sys.create({"risk_score": 0.3}, actor="sys")
        sys.transition(r2.resource.id, "SCORING", actor="x", role="x",
                       authority_level="CONTROLLER")
        assert sys.get(r2.resource.id).state == "LOW_RISK"

    def test_desired_state_unreachable_from_terminal(self):
        """Setting desired state from terminal should fail."""
        sys = define_system(
            name="item",
            states=["A", "B"],
            transitions=[("A", "B")],
            terminal_states=["B"],
        )
        result = sys.create({}, actor="sys")
        rid = result.resource.id
        sys.transition(rid, "B", actor="x", role="x", authority_level="CONTROLLER")

        with pytest.raises(Exception):
            sys.set_desired(rid, "A", requested_by="sys")


class TestEventIntegrity:
    def test_events_globally_ordered(self):
        """Events across resources have globally increasing offsets."""
        sys = define_system(
            name="item", states=["A", "B"],
            transitions=[("A", "B")], terminal_states=["B"],
        )

        r1 = sys.create({}, actor="sys")
        r2 = sys.create({}, actor="sys")
        sys.transition(r1.resource.id, "B", actor="x", role="x",
                       authority_level="CONTROLLER")

        e1 = sys.events(r1.resource.id)
        e2 = sys.events(r2.resource.id)
        all_offsets = sorted([e.offset for e in e1] + [e.offset for e in e2])
        assert all_offsets == list(range(len(all_offsets)))

    def test_event_resource_id_matches(self):
        sys = define_system(
            name="item", states=["A", "B"],
            transitions=[("A", "B")], terminal_states=["B"],
        )
        result = sys.create({}, actor="sys")
        rid = result.resource.id

        for event in sys.events(rid):
            assert event.resource_id == rid
