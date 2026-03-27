"""Procurement operating system example."""

from reconcile import InvariantResult, PolicyResult, define_system


def create_procurement_system():
    """Create a procurement workflow with approval thresholds and controls."""

    def budget_threshold(resource, ctx, query):
        amount = resource.data.get("amount", 0)
        if ctx.get("to_state") == "APPROVED" and amount > 1_000_000:
            return PolicyResult.deny("Purchases above ₹10L require CFO_REVIEW before approval.")
        return PolicyResult.allow()

    def vendor_due_diligence(resource, ctx, query):
        if ctx.get("to_state") == "PO_ISSUED" and not resource.data.get("vendor_approved"):
            return PolicyResult.deny("Vendor due diligence must complete before PO issuance.")
        return PolicyResult.allow()

    def positive_amount(resource, query):
        if resource.data.get("amount", 0) <= 0:
            return InvariantResult.violated("Procurement amount must be positive.")
        return InvariantResult.ok()

    return define_system(
        name="purchase_request",
        states=[
            "DRAFT", "SUBMITTED", "BUDGET_REVIEW", "VENDOR_REVIEW", "LEGAL_REVIEW",
            "CFO_REVIEW", "APPROVED", "PO_ISSUED", "RECEIVED", "CLOSED", "REJECTED",
        ],
        transitions=[
            ("DRAFT", "SUBMITTED"),
            ("SUBMITTED", "BUDGET_REVIEW"),
            ("BUDGET_REVIEW", "VENDOR_REVIEW"),
            ("VENDOR_REVIEW", "LEGAL_REVIEW"),
            ("LEGAL_REVIEW", "APPROVED"),
            ("LEGAL_REVIEW", "CFO_REVIEW"),
            ("CFO_REVIEW", "APPROVED"),
            ("BUDGET_REVIEW", "REJECTED"),
            ("VENDOR_REVIEW", "REJECTED"),
            ("LEGAL_REVIEW", "REJECTED"),
            ("CFO_REVIEW", "REJECTED"),
            ("APPROVED", "PO_ISSUED"),
            ("PO_ISSUED", "RECEIVED"),
            ("RECEIVED", "CLOSED"),
        ],
        terminal_states=["CLOSED", "REJECTED"],
        roles={
            "requester": ["view", "transition:SUBMITTED"],
            "budget_owner": ["view", "transition:BUDGET_REVIEW", "transition:REJECTED"],
            "procurement": ["view", "transition:VENDOR_REVIEW", "transition:PO_ISSUED"],
            "legal": ["view", "transition:LEGAL_REVIEW", "transition:REJECTED"],
            "cfo": ["view", "transition:CFO_REVIEW", "transition:APPROVED", "transition:REJECTED"],
            "warehouse": ["view", "transition:RECEIVED", "transition:CLOSED"],
        },
        policies=[
            {
                "name": "budget_threshold",
                "description": "Large purchases need CFO review",
                "evaluate": budget_threshold,
                "resource_types": ["purchase_request"],
                "priority": 90,
            },
            {
                "name": "vendor_due_diligence",
                "description": "Only approved vendors may receive POs",
                "evaluate": vendor_due_diligence,
                "resource_types": ["purchase_request"],
                "priority": 85,
            },
        ],
        invariants=[
            {
                "name": "positive_amount",
                "description": "Purchase amount must stay positive",
                "mode": "strong",
                "scope": "resource",
                "check": positive_amount,
                "resource_types": ["purchase_request"],
            }
        ],
    )
