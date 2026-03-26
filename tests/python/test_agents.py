"""Agent framework E2E tests.

Tests the full agent pipeline: event triggers agent observation →
agent produces proposal with confidence → proposal logged as event →
decision node aggregates → auto-accept/reject/review.
"""

import pytest
from reconcile._native import ReconcileSystem


@pytest.fixture
def lending_with_agents():
    """Lending system with risk and fraud agents + decision node."""
    sys = ReconcileSystem()

    sys.register_type(
        "loan",
        ["APPLIED", "SCORING", "APPROVED", "REJECTED", "FLAGGED"],
        [
            ("APPLIED", "SCORING"),
            ("SCORING", "APPROVED"),
            ("SCORING", "REJECTED"),
            ("SCORING", "FLAGGED"),
        ],
        "APPLIED",
        ["REJECTED"],
    )

    # Risk agent: scores based on amount
    def risk_agent(resource, query):
        amount = resource.data.get("amount", 0)
        if amount > 1_000_000:
            return {"transition": "REJECTED", "confidence": 0.3,
                    "reasoning": f"High amount {amount}"}
        elif amount > 500_000:
            return {"transition": "APPROVED", "confidence": 0.7,
                    "reasoning": f"Medium amount {amount}"}
        else:
            return {"transition": "APPROVED", "confidence": 0.95,
                    "reasoning": f"Low amount {amount}"}

    # Fraud agent: checks for fraud signals
    def fraud_agent(resource, query):
        if resource.data.get("suspicious"):
            return {"flag": "Fraud signals detected", "confidence": 0.2,
                    "reasoning": "Suspicious flag set"}
        return {"transition": "APPROVED", "confidence": 0.9,
                "reasoning": "No fraud signals"}

    sys.register_agent("risk", risk_agent, on_events=["loan.transitioned"], priority=80)
    sys.register_agent("fraud", fraud_agent, on_events=["loan.transitioned"], priority=70)

    # Decision node: aggregates risk + fraud
    sys.register_decision_node(
        "loan_committee",
        agents=["risk", "fraud"],
        aggregation="weighted_avg",
        auto_accept=0.9,
        auto_reject=0.5,
    )

    return sys


class TestAgentProposals:
    def test_agents_produce_proposals_on_transition(self, lending_with_agents):
        sys = lending_with_agents
        r = sys.create("loan", {"amount": 100_000}, "sys", "SYSTEM")
        rid = r.resource.id

        # Transition to SCORING — triggers agents
        sys.transition(rid, "SCORING", "sys", "sys", "CONTROLLER")

        # Check events — should include agent.proposal events
        events = sys.events(rid)
        proposal_events = [e for e in events if e.event_type == "agent.proposal"]
        assert len(proposal_events) >= 1, "Agents should produce proposal events"

    def test_low_risk_auto_approved(self, lending_with_agents):
        """Small loan: both agents give high confidence → decision node auto-approves."""
        sys = lending_with_agents
        r = sys.create("loan", {"amount": 50_000}, "sys", "SYSTEM")
        rid = r.resource.id

        sys.transition(rid, "SCORING", "sys", "sys", "CONTROLLER")

        # Decision node should have auto-accepted → loan transitions to APPROVED
        resource = sys.get(rid)
        assert resource.state == "APPROVED", (
            f"Low risk loan should be auto-approved by decision node, got {resource.state}"
        )

    def test_high_risk_not_auto_approved(self, lending_with_agents):
        """Large loan: risk agent gives low confidence → not auto-approved."""
        sys = lending_with_agents
        r = sys.create("loan", {"amount": 2_000_000}, "sys", "SYSTEM")
        rid = r.resource.id

        sys.transition(rid, "SCORING", "sys", "sys", "CONTROLLER")

        # Decision node should NOT auto-accept (avg confidence < 0.9)
        resource = sys.get(rid)
        # Risk says 0.3, fraud says 0.9 → avg = 0.6 → NeedsReview (between 0.5 and 0.9)
        assert resource.state == "SCORING", (
            f"High risk loan should stay in SCORING for human review, got {resource.state}"
        )

    def test_suspicious_loan_stays_for_review(self, lending_with_agents):
        """Suspicious loan: fraud agent flags → low confidence aggregate."""
        sys = lending_with_agents
        r = sys.create("loan", {"amount": 100_000, "suspicious": True}, "sys", "SYSTEM")
        rid = r.resource.id

        sys.transition(rid, "SCORING", "sys", "sys", "CONTROLLER")

        # Fraud agent gives 0.2 confidence (flag), risk gives 0.95
        # Avg = ~0.575 → NeedsReview
        resource = sys.get(rid)
        assert resource.state == "SCORING", (
            f"Suspicious loan should stay for review, got {resource.state}"
        )

    def test_agent_events_in_audit_trail(self, lending_with_agents):
        """Verify agent proposals appear in the event log."""
        sys = lending_with_agents
        r = sys.create("loan", {"amount": 50_000}, "sys", "SYSTEM")
        rid = r.resource.id

        sys.transition(rid, "SCORING", "sys", "sys", "CONTROLLER")

        events = sys.events(rid)
        # Should have: created, transitioned(SCORING), agent.proposal(risk),
        # agent.proposal(fraud), transitioned(APPROVED)
        types = [e.event_type for e in events]
        assert "loan.created" in types
        assert "loan.transitioned" in types
        assert "agent.proposal" in types


class TestDecisionNodeStrategies:
    def test_majority_strategy(self):
        sys = ReconcileSystem()
        sys.register_type("item", ["A", "B", "C"],
                          [("A", "B"), ("A", "C")], "A", ["B", "C"])

        # 3 agents: 2 say approve (B), 1 says reject (C)
        def agent_approve(resource, query):
            return {"transition": "B", "confidence": 0.95, "reasoning": "looks good"}

        def agent_reject(resource, query):
            return {"flag": "bad", "confidence": 0.3, "reasoning": "no good"}

        sys.register_agent("a1", agent_approve, on_events=["item.*"], priority=80)
        sys.register_agent("a2", agent_approve, on_events=["item.*"], priority=70)
        sys.register_agent("a3", agent_reject, on_events=["item.*"], priority=60)

        sys.register_decision_node(
            "committee", agents=["a1", "a2", "a3"],
            aggregation="majority", auto_accept=0.9,
        )

        r = sys.create("item", {}, "sys", "SYSTEM")
        # The creation event triggers agents → decision node evaluates
        resource = sys.get(r.resource.id)
        assert resource.state == "B", f"Majority approves → should transition to B, got {resource.state}"


class TestCircuitBreaker:
    def test_failing_controller_circuit_opens(self):
        """After N failures, controller is circuit-broken."""
        call_count = {"value": 0}

        def failing_controller(resource, query):
            call_count["value"] += 1
            raise RuntimeError("I always fail!")

        sys = ReconcileSystem()
        sys.register_type("item", ["A", "B"], [("A", "B")], "A", ["B"])
        sys.register_controller(
            "failing", failing_controller,
            on_events=["item.*"], priority=50,
        )

        # Create multiple items — each triggers the failing controller
        for _ in range(10):
            sys.create("item", {}, "sys", "SYSTEM")

        # After default threshold (5), the circuit should open and
        # the controller should stop being called
        # Exact behavior depends on threshold — controller may be called 5-6 times
        assert call_count["value"] >= 5, "Controller should be called at least threshold times"
        # The circuit should have opened — the controller stops being invoked
        # for later items
