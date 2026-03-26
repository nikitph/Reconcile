"""LLM Adapter — governance-constrained LLM interactions.

The GovernedLLM wraps any LLM provider (Claude, GPT, open-source) with
the governance constraint protocol. Every action the LLM takes routes
through the Reconcile kernel. The LLM reads InterfaceProjection, generates
contextual responses, and uses tools that map to kernel operations.

The governance guarantee: the LLM cannot bypass policies, invariants,
or role permissions because every action is a tool call that routes
through the kernel's 8-step transition pipeline.
"""

from typing import Any, Protocol, runtime_checkable
from reconcile._native import ReconcileSystem


# ---------------------------------------------------------------------------
# Provider abstraction
# ---------------------------------------------------------------------------

@runtime_checkable
class LLMProvider(Protocol):
    """Any LLM that supports messages + tools."""

    def chat(self, messages: list[dict], tools: list[dict] | None = None) -> dict:
        """Send messages and get a response. May include tool_calls."""
        ...


# ---------------------------------------------------------------------------
# Tool definitions — the kernel's API surface for the LLM
# ---------------------------------------------------------------------------

GOVERNED_TOOLS = [
    {
        "name": "execute_action",
        "description": "Execute a governance-validated action on a resource. Routes through the full kernel transition pipeline (state machine, RBAC, policies, invariants).",
        "parameters": {
            "type": "object",
            "properties": {
                "resource_id": {"type": "string", "description": "UUID of the resource"},
                "action": {"type": "string", "description": "The transition target state"},
            },
            "required": ["resource_id", "action"],
        },
    },
    {
        "name": "get_resource",
        "description": "Get the current state and data of a resource.",
        "parameters": {
            "type": "object",
            "properties": {
                "resource_id": {"type": "string"},
            },
            "required": ["resource_id"],
        },
    },
    {
        "name": "get_audit_trail",
        "description": "Get the audit trail for a resource — who did what, when, under which policies.",
        "parameters": {
            "type": "object",
            "properties": {
                "resource_id": {"type": "string"},
            },
            "required": ["resource_id"],
        },
    },
    {
        "name": "list_resources",
        "description": "List all resources of a given type.",
        "parameters": {
            "type": "object",
            "properties": {
                "resource_type": {"type": "string"},
            },
            "required": ["resource_type"],
        },
    },
]


# ---------------------------------------------------------------------------
# System prompt construction from InterfaceProjection
# ---------------------------------------------------------------------------

def build_system_prompt(projection, spec: dict | None = None) -> str:
    """Construct the LLM system prompt from an InterfaceProjection.

    The prompt contains everything the LLM needs to generate a correct,
    governance-compliant interface. The LLM doesn't need domain knowledge —
    it needs to understand the projection schema.
    """
    resource = projection.resource

    valid_actions = "\n".join(
        f"  - {action} (type: {atype})"
        for action, atype in projection.valid_actions
    ) or "  (none — terminal state or no permissions)"

    blocked_actions = "\n".join(
        f"  - {action}: {reason} (blocked by: {blocked_by})"
        for action, reason, blocked_by in projection.blocked_actions
    ) or "  (none)"

    warnings = "\n".join(
        f"  - [{severity}] {message} (source: {source})"
        for message, source, severity in projection.warnings
    ) or "  (none)"

    proposals = "\n".join(
        f"  - Agent '{agent}': {action} (confidence: {conf:.0%}) — {reasoning}"
        for agent, action, conf, reasoning in projection.proposals
    ) or "  (none)"

    return f"""You are an interface for a governance system.
The resource is a {resource['resource_type']} in state {resource['state']}.

RESOURCE DATA:
{_format_data(resource['data'])}

VALID ACTIONS the user can take:
{valid_actions}

BLOCKED ACTIONS (explain why if asked):
{blocked_actions}

WARNINGS to surface:
{warnings}

AI RECOMMENDATIONS:
{proposals}

RULES:
- Only offer actions from the VALID ACTIONS list
- If the user asks for a blocked action, explain the reason
- When the user confirms an action, call execute_action
- Never fabricate data not present in the resource
- Present warnings proactively when relevant"""


def _format_data(data: dict) -> str:
    if not data:
        return "  (empty)"
    return "\n".join(f"  {k}: {v}" for k, v in data.items())


# ---------------------------------------------------------------------------
# GovernedLLM — the constrained LLM runtime
# ---------------------------------------------------------------------------

class GovernedLLM:
    """Wraps any LLM with the governance constraint protocol.

    Every action routes through the kernel. The LLM cannot bypass
    governance because its only execution surface is tool calls that
    map to kernel operations.

    Usage:
        governed = GovernedLLM(system, actor="officer-7", role="loan_officer")
        response = governed.interact(resource_id, "Show me this loan")
        response = governed.interact(resource_id, "Approve it")
    """

    def __init__(
        self,
        system: ReconcileSystem,
        actor: str,
        role: str,
        provider: LLMProvider | None = None,
    ):
        self.system = system
        self.actor = actor
        self.role = role
        self.provider = provider
        self._messages: list[dict] = []

    def get_projection(self, resource_id: str):
        """Get the current InterfaceProjection for a resource."""
        return self.system.project(resource_id, self.role)

    def build_context(self, resource_id: str) -> dict:
        """Build the full LLM context for a resource.

        Returns a dict with projection, system_prompt, and tools.
        This is everything the LLM needs.
        """
        projection = self.get_projection(resource_id)
        system_prompt = build_system_prompt(projection)
        spec = self.system.export_spec()

        return {
            "projection": projection,
            "system_prompt": system_prompt,
            "tools": GOVERNED_TOOLS,
            "spec": spec,
            "actor": self.actor,
            "role": self.role,
        }

    def handle_tool_call(self, tool_name: str, args: dict) -> dict:
        """Execute a tool call through the kernel.

        This is the governance boundary — every LLM action routes here.
        """
        if tool_name == "execute_action":
            result, projection = self.system.execute_action(
                args["resource_id"],
                args["action"],
                self.actor,
                self.role,
                "INTERFACE",  # Authority level distinguishes LLM-mediated actions
            )
            if result.success:
                return {
                    "success": True,
                    "new_state": projection.resource["state"] if projection else None,
                    "projection": projection.to_json() if projection else None,
                }
            else:
                return {
                    "success": False,
                    "rejected_step": result.rejected_step,
                    "rejected_reason": result.rejected_reason,
                }

        elif tool_name == "get_resource":
            resource = self.system.get(args["resource_id"])
            if resource:
                return {
                    "id": resource.id,
                    "state": resource.state,
                    "data": resource.data,
                    "version": resource.version,
                }
            return {"error": "Resource not found"}

        elif tool_name == "get_audit_trail":
            audit = self.system.audit(args["resource_id"])
            return {
                "entries": [
                    {
                        "actor": a.actor,
                        "role": a.role,
                        "previous_state": a.previous_state,
                        "new_state": a.new_state,
                        "authority_level": a.authority_level,
                    }
                    for a in audit
                ]
            }

        elif tool_name == "list_resources":
            resources = self.system.list_resources(args["resource_type"])
            return {
                "resources": [
                    {"id": r.id, "state": r.state, "data": r.data}
                    for r in resources
                ]
            }

        return {"error": f"Unknown tool: {tool_name}"}

    def interact(self, resource_id: str, user_message: str) -> dict:
        """Full interaction loop: user message → projection → LLM → tool calls → response.

        If no LLM provider is configured, returns the raw context for the caller
        to pass to their own LLM.
        """
        context = self.build_context(resource_id)

        if self.provider is None:
            # No LLM configured — return context for manual LLM integration
            return {
                "system_prompt": context["system_prompt"],
                "tools": context["tools"],
                "user_message": user_message,
                "projection": context["projection"].to_json(),
            }

        # Full LLM loop with provider
        messages = [
            {"role": "system", "content": context["system_prompt"]},
            *self._messages,
            {"role": "user", "content": user_message},
        ]

        response = self.provider.chat(messages, tools=context["tools"])

        # Handle tool calls if present
        if "tool_calls" in response:
            tool_results = []
            for tc in response["tool_calls"]:
                result = self.handle_tool_call(tc["name"], tc["arguments"])
                tool_results.append(result)
            response["tool_results"] = tool_results

        # Track conversation
        self._messages.append({"role": "user", "content": user_message})
        if "content" in response:
            self._messages.append({"role": "assistant", "content": response["content"]})

        return response
