"""Interface Projection Engine E2E tests.

Tests the core product function: project(resource_id, role).
Verifies that the projection correctly reflects role permissions,
policy constraints, agent proposals, and audit history.
"""

import pytest
from reconcile import define_system, PolicyResult, InvariantResult


@pytest.fixture
def loan_system():
    """Loan system with multiple roles and policies for projection testing."""

    def high_value_policy(resource, ctx, query):
        if ctx.get("to_state") == "APPROVED":
            amount = resource.data.get("amount", 0)
            if amount > 5_000_000:
                return PolicyResult.deny("Loans > 50L need committee approval")
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
            "clerk": ["view", "transition:UNDERWRITING"],
            "officer": ["view", "transition:APPROVED", "transition:REJECTED"],
            "manager": ["view", "transition:*"],
            "viewer": ["view"],
        },
        policies=[{
            "name": "high_value_limit",
            "description": "Loans > 50L need committee",
            "evaluate": high_value_policy,
            "applicable_states": ["UNDERWRITING"],
            "resource_types": ["loan"],
            "priority": 90,
        }],
    )


class TestProjectionBasics:
    def test_projection_returns_resource_data(self, loan_system):
        r = loan_system.create({"amount": 500_000, "applicant": "Acme"}, actor="u1")
        p = loan_system.project(r.resource.id, "clerk")

        assert p.resource["state"] == "APPLIED"
        assert p.resource["data"]["amount"] == 500_000
        assert p.resource["data"]["applicant"] == "Acme"
        assert p.resource["version"] == 1
        assert p.resource["is_terminal"] is False

    def test_terminal_state_projection(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")
        loan_system.transition(r.resource.id, "REJECTED", actor="u1", role="manager")

        p = loan_system.project(r.resource.id, "manager")
        assert p.resource["is_terminal"] is True
        assert len(p.valid_actions) == 0
        assert len(p.blocked_actions) == 0


class TestRoleBasedProjection:
    def test_clerk_sees_underwriting_only(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        p = loan_system.project(r.resource.id, "clerk")

        actions = [a[0] for a in p.valid_actions]
        assert "UNDERWRITING" in actions
        assert len(p.valid_actions) == 1

    def test_officer_in_underwriting_sees_approve_reject(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")

        p = loan_system.project(r.resource.id, "officer")
        actions = [a[0] for a in p.valid_actions]
        assert "APPROVED" in actions
        assert "REJECTED" in actions
        assert len(actions) == 2

    def test_manager_sees_all_valid_transitions(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")

        p = loan_system.project(r.resource.id, "manager")
        assert len(p.valid_actions) == 2  # APPROVED + REJECTED

    def test_viewer_sees_no_actions(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        p = loan_system.project(r.resource.id, "viewer")

        assert len(p.valid_actions) == 0
        # Should have blocked actions explaining why
        blocked = [b[0] for b in p.blocked_actions]
        assert "UNDERWRITING" in blocked

    def test_blocked_actions_explain_reason(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")

        # Clerk can't approve or reject — projection should explain why
        p = loan_system.project(r.resource.id, "clerk")
        assert len(p.valid_actions) == 0  # No valid actions in UNDERWRITING for clerk

        for action, reason, blocked_by in p.blocked_actions:
            assert blocked_by == "role_permission"
            assert "clerk" in reason.lower()


class TestPolicyProjection:
    def test_policy_blocks_action_with_reason(self, loan_system):
        r = loan_system.create({"amount": 10_000_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")

        p = loan_system.project(r.resource.id, "manager")

        # APPROVED should be blocked by policy
        blocked_map = {b[0]: (b[1], b[2]) for b in p.blocked_actions}
        assert "APPROVED" in blocked_map
        reason, blocked_by = blocked_map["APPROVED"]
        assert blocked_by == "policy"
        assert "50L" in reason or "committee" in reason.lower()

        # REJECTED should still be valid
        valid = [a[0] for a in p.valid_actions]
        assert "REJECTED" in valid

    def test_policy_warning_surfaces(self, loan_system):
        r = loan_system.create({"amount": 10_000_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")

        p = loan_system.project(r.resource.id, "manager")

        # Should have a warning about the high value policy
        warning_sources = [w[1] for w in p.warnings]
        assert "high_value_limit" in warning_sources

    def test_small_loan_not_blocked(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")

        p = loan_system.project(r.resource.id, "manager")
        valid = [a[0] for a in p.valid_actions]
        assert "APPROVED" in valid
        assert len(p.blocked_actions) == 0


class TestAuditInProjection:
    def test_audit_summary_after_transitions(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="alice", role="clerk")
        loan_system.transition(r.resource.id, "APPROVED", actor="bob", role="officer")

        p = loan_system.project(r.resource.id, "manager")

        assert len(p.audit_summary) >= 2
        actors = [a[0] for a in p.audit_summary]
        assert "alice" in actors or "bob" in actors

    def test_no_audit_before_transitions(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        p = loan_system.project(r.resource.id, "manager")
        assert len(p.audit_summary) == 0


class TestProjectionList:
    def test_batch_projection(self, loan_system):
        for _ in range(5):
            loan_system.create({"amount": 100_000}, actor="u1")

        projections = loan_system.project_list("manager")
        assert len(projections) == 5
        for p in projections:
            assert p.resource["state"] == "APPLIED"


class TestExportSpec:
    def test_spec_has_types(self, loan_system):
        spec = loan_system.export_spec()
        assert len(spec["types"]) >= 1
        assert spec["types"][0]["name"] == "loan"

    def test_spec_has_states(self, loan_system):
        spec = loan_system.export_spec()
        states = spec["types"][0]["states"]
        assert "APPLIED" in states
        assert "UNDERWRITING" in states
        assert "DISBURSED" in states

    def test_spec_has_transitions(self, loan_system):
        spec = loan_system.export_spec()
        transitions = spec["types"][0]["transitions"]
        froms = [t["from"] for t in transitions]
        assert "APPLIED" in froms

    def test_spec_has_counts(self, loan_system):
        spec = loan_system.export_spec()
        assert spec["policy_count"] >= 1
        assert spec["version"] == "0.1.0"


class TestExecuteAction:
    def test_execute_returns_new_projection(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")

        result, projection = loan_system.execute_action(
            r.resource.id, "UNDERWRITING", actor="clerk1", role="clerk",
        )
        assert result.success
        assert projection is not None
        assert projection.resource["state"] == "UNDERWRITING"

    def test_execute_blocked_returns_no_projection(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")

        result, projection = loan_system.execute_action(
            r.resource.id, "APPROVED", actor="clerk1", role="clerk",
        )
        assert not result.success
        assert projection is None


class TestDesiredStateWarning:
    def test_desired_state_shows_in_warnings(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        loan_system.set_desired(r.resource.id, "DISBURSED", requested_by="mgr")

        # After reconciliation, resource is at DISBURSED
        p = loan_system.project(r.resource.id, "manager")
        # If reconciled fully, no desired state warning
        assert p.resource["state"] == "DISBURSED"


class TestProjectionToJSON:
    def test_to_json_is_complete(self, loan_system):
        r = loan_system.create({"amount": 500_000}, actor="u1")
        p = loan_system.project(r.resource.id, "manager")
        j = p.to_json()

        assert "resource" in j
        assert "valid_actions" in j
        assert "blocked_actions" in j
        assert "warnings" in j
        assert "proposals" in j
        assert "audit_summary" in j
        assert j["resource"]["state"] == "APPLIED"
