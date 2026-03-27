"""Clinical trials operating system example."""

from reconcile import InvariantResult, PolicyResult, define_system


def create_clinical_trials_system():
    """Create a clinical-trials workflow with compliance guardrails."""

    def consent_required(resource, ctx, query):
        if ctx.get("to_state") == "ENROLLMENT" and not resource.data.get("irb_approved"):
            return PolicyResult.deny("IRB approval is required before participant enrollment.")
        return PolicyResult.allow()

    def safety_hold(resource, ctx, query):
        if ctx.get("to_state") == "ACTIVE" and resource.data.get("serious_adverse_event"):
            return PolicyResult.deny("Trials with unresolved serious adverse events remain on hold.")
        return PolicyResult.allow()

    def protocol_identifier(resource, query):
        if not resource.data.get("protocol_id"):
            return InvariantResult.violated("protocol_id is required for every trial.")
        return InvariantResult.ok()

    return define_system(
        name="trial",
        states=[
            "DRAFT", "SPONSOR_REVIEW", "IRB_REVIEW", "SITE_SELECTION", "CONTRACTING",
            "ENROLLMENT", "ACTIVE", "DATA_REVIEW", "LOCKED", "COMPLETED", "HALTED",
        ],
        transitions=[
            ("DRAFT", "SPONSOR_REVIEW"),
            ("SPONSOR_REVIEW", "IRB_REVIEW"),
            ("IRB_REVIEW", "SITE_SELECTION"),
            ("SITE_SELECTION", "CONTRACTING"),
            ("CONTRACTING", "ENROLLMENT"),
            ("ENROLLMENT", "ACTIVE"),
            ("ACTIVE", "DATA_REVIEW"),
            ("DATA_REVIEW", "LOCKED"),
            ("LOCKED", "COMPLETED"),
            ("ACTIVE", "HALTED"),
            ("ENROLLMENT", "HALTED"),
            ("HALTED", "ACTIVE"),
        ],
        terminal_states=["COMPLETED"],
        roles={
            "coordinator": ["view", "transition:SPONSOR_REVIEW", "transition:ENROLLMENT"],
            "sponsor": ["view", "transition:IRB_REVIEW", "transition:SITE_SELECTION"],
            "irb": ["view", "transition:SITE_SELECTION", "transition:HALTED"],
            "operations": ["view", "transition:CONTRACTING", "transition:ACTIVE"],
            "data_manager": ["view", "transition:DATA_REVIEW", "transition:LOCKED"],
            "qa": ["view", "transition:COMPLETED", "transition:HALTED"],
        },
        policies=[
            {
                "name": "consent_required",
                "description": "Enrollment requires IRB approval",
                "evaluate": consent_required,
                "resource_types": ["trial"],
                "priority": 95,
            },
            {
                "name": "safety_hold",
                "description": "Active trial blocked on unresolved SAE",
                "evaluate": safety_hold,
                "resource_types": ["trial"],
                "priority": 90,
            },
        ],
        invariants=[
            {
                "name": "protocol_identifier",
                "description": "Every trial must have a protocol identifier",
                "mode": "strong",
                "scope": "resource",
                "check": protocol_identifier,
                "resource_types": ["trial"],
            }
        ],
    )
