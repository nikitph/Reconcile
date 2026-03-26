"""LLM Adapter / GovernedLLM tests.

Tests the governance constraint protocol: system prompt construction,
tool binding, tool call execution through the kernel, and the full
interaction loop.
"""

import pytest
from reconcile import define_system, PolicyResult
from reconcile.llm import GovernedLLM, build_system_prompt, GOVERNED_TOOLS


@pytest.fixture
def loan_system():
    def high_value(resource, ctx, query):
        if ctx.get("to_state") == "APPROVED" and resource.data.get("amount", 0) > 5_000_000:
            return PolicyResult.deny("Needs committee approval")
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
            "officer": ["view", "transition:UNDERWRITING", "transition:APPROVED", "transition:REJECTED"],
            "manager": ["view", "transition:*"],
            "viewer": ["view"],
        },
        policies=[{
            "name": "high_value", "evaluate": high_value,
            "applicable_states": ["UNDERWRITING"], "resource_types": ["loan"],
        }],
    )


class TestSystemPromptConstruction:
    def test_prompt_contains_state(self, loan_system):
        r = loan_system.create({"amount": 500_000, "applicant": "Acme"}, actor="u1")
        p = loan_system.project(r.resource.id, "officer")
        prompt = build_system_prompt(p)

        assert "APPLIED" in prompt
        assert "loan" in prompt

    def test_prompt_contains_valid_actions(self, loan_system):
        r = loan_system.create({"amount": 500_000}, actor="u1")
        p = loan_system.project(r.resource.id, "officer")
        prompt = build_system_prompt(p)

        assert "UNDERWRITING" in prompt
        assert "VALID ACTIONS" in prompt

    def test_prompt_contains_blocked_actions(self, loan_system):
        r = loan_system.create({"amount": 500_000}, actor="u1")
        p = loan_system.project(r.resource.id, "viewer")
        prompt = build_system_prompt(p)

        assert "BLOCKED ACTIONS" in prompt
        # Viewer can't transition — should explain why
        assert "viewer" in prompt.lower() or "role" in prompt.lower()

    def test_prompt_contains_warnings(self, loan_system):
        r = loan_system.create({"amount": 10_000_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")
        p = loan_system.project(r.resource.id, "officer")
        prompt = build_system_prompt(p)

        assert "WARNINGS" in prompt
        assert "committee" in prompt.lower() or "high_value" in prompt.lower()

    def test_prompt_contains_resource_data(self, loan_system):
        r = loan_system.create({"amount": 500_000, "applicant": "Acme Corp"}, actor="u1")
        p = loan_system.project(r.resource.id, "officer")
        prompt = build_system_prompt(p)

        assert "500000" in prompt or "500_000" in prompt
        assert "Acme Corp" in prompt

    def test_prompt_contains_rules(self, loan_system):
        r = loan_system.create({"amount": 100_000}, actor="u1")
        p = loan_system.project(r.resource.id, "officer")
        prompt = build_system_prompt(p)

        assert "RULES" in prompt
        assert "VALID ACTIONS" in prompt
        assert "execute_action" in prompt


class TestGovernedLLMToolCalls:
    def test_execute_action_succeeds(self, loan_system):
        governed = GovernedLLM(loan_system.native, actor="officer1", role="officer")
        r = loan_system.create({"amount": 100_000}, actor="u1")

        result = governed.handle_tool_call("execute_action", {
            "resource_id": r.resource.id,
            "action": "UNDERWRITING",
        })

        assert result["success"] is True
        assert result["new_state"] == "UNDERWRITING"

    def test_execute_action_blocked_by_rbac(self, loan_system):
        governed = GovernedLLM(loan_system.native, actor="viewer1", role="viewer")
        r = loan_system.create({"amount": 100_000}, actor="u1")

        result = governed.handle_tool_call("execute_action", {
            "resource_id": r.resource.id,
            "action": "UNDERWRITING",
        })

        assert result["success"] is False
        assert "role" in result["rejected_reason"].lower() or "permission" in result["rejected_reason"].lower()

    def test_execute_action_blocked_by_policy(self, loan_system):
        governed = GovernedLLM(loan_system.native, actor="officer1", role="officer")
        r = loan_system.create({"amount": 10_000_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")

        result = governed.handle_tool_call("execute_action", {
            "resource_id": r.resource.id,
            "action": "APPROVED",
        })

        assert result["success"] is False
        assert "committee" in result["rejected_reason"].lower()

    def test_get_resource(self, loan_system):
        governed = GovernedLLM(loan_system.native, actor="u1", role="officer")
        r = loan_system.create({"amount": 100_000}, actor="u1")

        result = governed.handle_tool_call("get_resource", {
            "resource_id": r.resource.id,
        })

        assert result["state"] == "APPLIED"
        assert result["data"]["amount"] == 100_000

    def test_get_audit_trail(self, loan_system):
        governed = GovernedLLM(loan_system.native, actor="u1", role="officer")
        r = loan_system.create({"amount": 100_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="alice", role="officer")

        result = governed.handle_tool_call("get_audit_trail", {
            "resource_id": r.resource.id,
        })

        assert len(result["entries"]) == 1
        assert result["entries"][0]["actor"] == "alice"

    def test_list_resources(self, loan_system):
        governed = GovernedLLM(loan_system.native, actor="u1", role="officer")
        for _ in range(3):
            loan_system.create({"amount": 100_000}, actor="u1")

        result = governed.handle_tool_call("list_resources", {
            "resource_type": "loan",
        })

        assert len(result["resources"]) == 3

    def test_unknown_tool_returns_error(self, loan_system):
        governed = GovernedLLM(loan_system.native, actor="u1", role="officer")
        result = governed.handle_tool_call("nonexistent_tool", {})
        assert "error" in result

    def test_execute_action_uses_interface_authority(self, loan_system):
        """Actions through GovernedLLM should use INTERFACE authority level."""
        governed = GovernedLLM(loan_system.native, actor="officer1", role="officer")
        r = loan_system.create({"amount": 100_000}, actor="u1")

        governed.handle_tool_call("execute_action", {
            "resource_id": r.resource.id,
            "action": "UNDERWRITING",
        })

        audit = loan_system.audit(r.resource.id)
        assert len(audit) == 1
        assert audit[0].authority_level == "INTERFACE"


class TestGovernedLLMInteraction:
    def test_interact_without_provider_returns_context(self, loan_system):
        """Without an LLM provider, interact() returns the raw context."""
        governed = GovernedLLM(loan_system.native, actor="u1", role="officer")
        r = loan_system.create({"amount": 100_000}, actor="u1")

        response = governed.interact(r.resource.id, "Show me this loan")

        assert "system_prompt" in response
        assert "tools" in response
        assert "projection" in response
        assert "user_message" in response
        assert response["user_message"] == "Show me this loan"

    def test_context_has_projection_data(self, loan_system):
        governed = GovernedLLM(loan_system.native, actor="u1", role="officer")
        r = loan_system.create({"amount": 500_000, "applicant": "Acme"}, actor="u1")

        context = governed.build_context(r.resource.id)

        assert "system_prompt" in context
        assert "tools" in context
        assert "spec" in context
        assert context["actor"] == "u1"
        assert context["role"] == "officer"
        assert "Acme" in context["system_prompt"] or "500000" in context["system_prompt"]

    def test_tool_definitions_are_complete(self, loan_system):
        assert len(GOVERNED_TOOLS) == 4
        names = [t["name"] for t in GOVERNED_TOOLS]
        assert "execute_action" in names
        assert "get_resource" in names
        assert "get_audit_trail" in names
        assert "list_resources" in names


class TestGovernanceGuarantee:
    """The core safety property: no tool call can bypass governance."""

    def test_tool_call_respects_rbac(self, loan_system):
        """Even if the LLM tries to approve, RBAC blocks it for viewer."""
        governed = GovernedLLM(loan_system.native, actor="viewer1", role="viewer")
        r = loan_system.create({"amount": 100_000}, actor="u1")

        # Try every possible action through tool calls
        for action in ["UNDERWRITING", "APPROVED", "REJECTED", "DISBURSED"]:
            result = governed.handle_tool_call("execute_action", {
                "resource_id": r.resource.id,
                "action": action,
            })
            assert result["success"] is False, f"Viewer should not be able to {action}"

        # Resource should be unchanged
        resource = loan_system.get(r.resource.id)
        assert resource.state == "APPLIED"

    def test_tool_call_respects_policies(self, loan_system):
        """Policy blocks high-value approval even through tool call."""
        governed = GovernedLLM(loan_system.native, actor="officer1", role="officer")
        r = loan_system.create({"amount": 10_000_000}, actor="u1")
        loan_system.transition(r.resource.id, "UNDERWRITING", actor="u1", role="manager")

        result = governed.handle_tool_call("execute_action", {
            "resource_id": r.resource.id,
            "action": "APPROVED",
        })

        assert result["success"] is False
        assert "committee" in result["rejected_reason"].lower()

    def test_tool_call_respects_state_machine(self, loan_system):
        """Can't skip states even through tool calls."""
        governed = GovernedLLM(loan_system.native, actor="mgr", role="manager")
        r = loan_system.create({"amount": 100_000}, actor="u1")

        # Try to skip directly to APPROVED (invalid: must go through UNDERWRITING)
        result = governed.handle_tool_call("execute_action", {
            "resource_id": r.resource.id,
            "action": "APPROVED",
        })

        assert result["success"] is False
