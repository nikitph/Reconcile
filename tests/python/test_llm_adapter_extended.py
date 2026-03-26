"""Additional provider-loop and edge-case coverage for GovernedLLM."""

from reconcile import define_system
from reconcile.llm import GovernedLLM, build_system_prompt


class RecordingProvider:
    def __init__(self, response):
        self.response = response
        self.calls = []

    def chat(self, messages, tools=None):
        self.calls.append({"messages": messages, "tools": tools})
        return dict(self.response)


def test_build_system_prompt_handles_empty_sections():
    system = define_system(
        name="ticket",
        states=["OPEN"],
        transitions=[],
        terminal_states=["OPEN"],
        roles={"viewer": ["view"]},
    )
    result = system.create({}, actor="u1")
    projection = system.project(result.resource.id, "viewer")

    prompt = build_system_prompt(projection)

    assert "RESOURCE DATA:\n  (empty)" in prompt
    assert "VALID ACTIONS the user can take:\n  (none" in prompt
    assert "BLOCKED ACTIONS (explain why if asked):\n  (none)" in prompt
    assert "WARNINGS to surface:\n  (none)" in prompt
    assert "AI RECOMMENDATIONS:\n  (none)" in prompt


def test_get_resource_returns_error_for_missing_resource(loan_system):
    governed = GovernedLLM(loan_system.native, actor="u1", role="officer")

    result = governed.handle_tool_call("get_resource", {
        "resource_id": "00000000-0000-0000-0000-000000000000",
    })

    assert result == {"error": "Resource not found"}


def test_interact_with_provider_passes_context_and_tracks_messages(loan_system):
    resource = loan_system.create({"amount": 100_000}, actor="u1")
    provider = RecordingProvider({"content": "Loan is ready for review."})
    governed = GovernedLLM(
        loan_system.native,
        actor="officer-1",
        role="officer",
        provider=provider,
    )

    response = governed.interact(resource.resource.id, "Summarize this loan")

    assert response["content"] == "Loan is ready for review."
    assert len(provider.calls) == 1
    call = provider.calls[0]
    assert call["messages"][0]["role"] == "system"
    assert "VALID ACTIONS" in call["messages"][0]["content"]
    assert call["messages"][-1] == {"role": "user", "content": "Summarize this loan"}
    assert call["tools"]
    assert governed._messages == [
        {"role": "user", "content": "Summarize this loan"},
        {"role": "assistant", "content": "Loan is ready for review."},
    ]


def test_interact_executes_tool_calls_and_reuses_conversation_history(loan_system):
    resource = loan_system.create({"amount": 100_000}, actor="u1")
    provider = RecordingProvider({
        "content": "Moved the loan to underwriting.",
        "tool_calls": [{
            "name": "execute_action",
            "arguments": {
                "resource_id": resource.resource.id,
                "action": "UNDERWRITING",
            },
        }],
    })
    governed = GovernedLLM(
        loan_system.native,
        actor="officer-1",
        role="officer",
        provider=provider,
    )

    first_response = governed.interact(resource.resource.id, "Advance this loan")
    second_response = governed.interact(resource.resource.id, "What changed?")

    assert first_response["tool_results"][0]["success"] is True
    assert first_response["tool_results"][0]["new_state"] == "UNDERWRITING"
    assert second_response["content"] == "Moved the loan to underwriting."
    assert len(provider.calls) == 2
    second_messages = provider.calls[1]["messages"]
    assert {"role": "assistant", "content": "Moved the loan to underwriting."} in second_messages
    assert {"role": "user", "content": "Advance this loan"} in second_messages


def test_build_context_includes_exported_spec(loan_system):
    resource = loan_system.create({"amount": 123}, actor="u1")
    governed = GovernedLLM(loan_system.native, actor="u1", role="officer")

    context = governed.build_context(resource.resource.id)

    assert context["spec"] == loan_system.export_spec()
    assert context["projection"].resource["id"] == resource.resource.id
