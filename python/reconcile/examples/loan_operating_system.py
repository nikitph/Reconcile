"""Loan Operating System reference implementation."""

from reconcile import PolicyResult, InvariantResult, define_system


def create_loan_operating_system(database_url: str | None = None):
    """Build a realistic lending operating system on top of Reconcile."""

    def senior_review_threshold(resource, ctx, query):
        if ctx.get("to_state") == "APPROVED" and ctx.get("from_state") == "UNDERWRITING":
            amount = resource.data.get("amount", 0)
            if amount > 2_500_000:
                return PolicyResult.deny(
                    f"Loans above ₹25L require SENIOR_REVIEW, got ₹{amount:,.0f}."
                )
        return PolicyResult.allow()

    def committee_threshold(resource, ctx, query):
        if ctx.get("to_state") == "APPROVED" and ctx.get("from_state") == "SENIOR_REVIEW":
            amount = resource.data.get("amount", 0)
            if amount > 10_000_000:
                return PolicyResult.deny(
                    f"Loans above ₹1Cr require COMMITTEE_REVIEW, got ₹{amount:,.0f}."
                )
        return PolicyResult.allow()

    def collateral_required(resource, ctx, query):
        if ctx.get("to_state") != "APPROVED":
            return PolicyResult.allow()
        amount = resource.data.get("amount", 0)
        if amount > 1_000_000 and not resource.data.get("collateral_id"):
            return PolicyResult.deny("Loans above ₹10L require pledged collateral.")
        return PolicyResult.allow()

    def exposure_limit(resource, ctx, query):
        if ctx.get("to_state") not in {"APPROVED", "DISBURSED"}:
            return PolicyResult.allow()
        applicant_id = resource.data.get("applicant_id")
        if not applicant_id:
            return PolicyResult.allow()
        existing = query.graph_aggregate(applicant_id, "belongs_to", "amount", "SUM") or 0
        total = existing + resource.data.get("amount", 0)
        if total > 50_000_000:
            return PolicyResult.deny(
                f"Borrower exposure ₹{total:,.0f} exceeds ₹5Cr prudential limit."
            )
        return PolicyResult.allow()

    def npa_classification(resource, ctx, query):
        if ctx.get("to_state") == "NPA" and resource.data.get("days_overdue", 0) < 90:
            return PolicyResult.deny("NPA classification requires 90+ days overdue.")
        return PolicyResult.allow()

    def bureau_watchlist(resource, ctx, query):
        if ctx.get("to_state") in {"APPROVED", "DISBURSED"} and resource.data.get("watchlist_hit"):
            return PolicyResult.deny("Watchlist-hit applicants cannot be sanctioned.")
        return PolicyResult.allow()

    def positive_amount(resource, query):
        if resource.resource_type == "loan" and resource.data.get("amount", 0) <= 0:
            return InvariantResult.violated("Loan amount must be positive.")
        return InvariantResult.ok()

    def valid_interest_rate(resource, query):
        if resource.resource_type != "loan":
            return InvariantResult.ok()
        rate = resource.data.get("interest_rate")
        if rate is not None and (rate < 0 or rate > 36):
            return InvariantResult.violated("Interest rate must remain within 0-36%.")
        return InvariantResult.ok()

    def applicant_has_name(resource, query):
        if resource.resource_type == "applicant" and not resource.data.get("name"):
            return InvariantResult.violated("Applicant name is required.")
        return InvariantResult.ok()

    def risk_scorer(resource, query):
        if resource.resource_type != "loan" or resource.state not in {"UNDERWRITING", "SENIOR_REVIEW"}:
            return None
        amount = resource.data.get("amount", 0)
        bureau_score = resource.data.get("bureau_score", 700)
        leverage = resource.data.get("debt_to_income", 0.35)
        confidence = 0.95
        reasons = []
        if amount > 5_000_000:
            confidence -= 0.25
            reasons.append("large ticket size")
        if bureau_score < 680:
            confidence -= 0.20
            reasons.append("sub-prime bureau score")
        if leverage > 0.55:
            confidence -= 0.20
            reasons.append("high leverage")
        confidence = max(0.2, round(confidence, 2))
        if confidence < 0.5:
            return {
                "flag": "Escalated credit risk",
                "confidence": confidence,
                "reasoning": ", ".join(reasons) or "risk model escalation",
            }
        return {
            "transition": "APPROVED",
            "confidence": confidence,
            "reasoning": ", ".join(reasons) or "acceptable risk profile",
        }

    def fraud_detector(resource, query):
        if resource.resource_type != "loan" or resource.state not in {
            "UNDERWRITING", "SENIOR_REVIEW", "COMMITTEE_REVIEW",
        }:
            return None
        suspicious = resource.data.get("suspicious_indicators", [])
        if suspicious:
            return {
                "flag": f"Fraud indicators present: {', '.join(suspicious)}",
                "confidence": 0.2,
                "reasoning": f"{len(suspicious)} suspicious indicator(s) found",
            }
        return {
            "transition": "APPROVED",
            "confidence": 0.9,
            "reasoning": "No fraud or document anomalies detected",
        }

    system = define_system(
        name="loan",
        states=[
            "DRAFT",
            "APPLIED",
            "KYC_REVIEW",
            "DOCUMENT_VERIFICATION",
            "CREDIT_BUREAU_CHECK",
            "UNDERWRITING",
            "SENIOR_REVIEW",
            "COMMITTEE_REVIEW",
            "APPROVED",
            "DISBURSED",
            "REPAYING",
            "CLOSED",
            "REJECTED",
            "NPA",
            "WRITTEN_OFF",
        ],
        transitions=[
            ("DRAFT", "APPLIED"),
            ("APPLIED", "KYC_REVIEW"),
            ("KYC_REVIEW", "DOCUMENT_VERIFICATION"),
            ("DOCUMENT_VERIFICATION", "CREDIT_BUREAU_CHECK"),
            ("CREDIT_BUREAU_CHECK", "UNDERWRITING"),
            ("UNDERWRITING", "APPROVED"),
            ("UNDERWRITING", "SENIOR_REVIEW"),
            ("UNDERWRITING", "REJECTED"),
            ("SENIOR_REVIEW", "APPROVED"),
            ("SENIOR_REVIEW", "COMMITTEE_REVIEW"),
            ("SENIOR_REVIEW", "REJECTED"),
            ("COMMITTEE_REVIEW", "APPROVED"),
            ("COMMITTEE_REVIEW", "REJECTED"),
            ("APPROVED", "DISBURSED"),
            ("DISBURSED", "REPAYING"),
            ("REPAYING", "CLOSED"),
            ("REPAYING", "NPA"),
            ("NPA", "REPAYING"),
            ("NPA", "WRITTEN_OFF"),
            ("DOCUMENT_VERIFICATION", "DRAFT"),
            ("UNDERWRITING", "DOCUMENT_VERIFICATION"),
        ],
        terminal_states=["CLOSED", "REJECTED", "WRITTEN_OFF"],
        roles={
            "data_entry": ["view", "transition:APPLIED"],
            "kyc_officer": ["view", "transition:KYC_REVIEW", "transition:DOCUMENT_VERIFICATION"],
            "document_officer": [
                "view",
                "transition:DRAFT",
                "transition:CREDIT_BUREAU_CHECK",
                "transition:UNDERWRITING",
            ],
            "underwriter": [
                "view", "transition:APPROVED", "transition:SENIOR_REVIEW",
                "transition:REJECTED", "transition:DOCUMENT_VERIFICATION",
            ],
            "senior_underwriter": [
                "view", "transition:APPROVED", "transition:COMMITTEE_REVIEW", "transition:REJECTED",
            ],
            "credit_committee": ["view", "transition:APPROVED", "transition:REJECTED"],
            "branch_manager": ["view", "transition:DISBURSED", "transition:REPAYING", "transition:CLOSED"],
            "collections": ["view", "transition:NPA", "transition:REPAYING", "transition:WRITTEN_OFF"],
        },
        policies=[
            {
                "name": "senior_review_threshold",
                "description": "Loans above ₹25L require senior credit approval",
                "evaluate": senior_review_threshold,
                "applicable_states": ["UNDERWRITING"],
                "resource_types": ["loan"],
                "priority": 95,
            },
            {
                "name": "committee_threshold",
                "description": "Loans above ₹1Cr require committee review",
                "evaluate": committee_threshold,
                "applicable_states": ["SENIOR_REVIEW"],
                "resource_types": ["loan"],
                "priority": 92,
            },
            {
                "name": "collateral_required",
                "description": "Secured lending threshold at ₹10L",
                "evaluate": collateral_required,
                "applicable_states": ["UNDERWRITING", "SENIOR_REVIEW", "COMMITTEE_REVIEW"],
                "resource_types": ["loan"],
                "priority": 88,
            },
            {
                "name": "borrower_exposure_limit",
                "description": "Total borrower exposure capped at ₹5Cr",
                "evaluate": exposure_limit,
                "resource_types": ["loan"],
                "priority": 98,
            },
            {
                "name": "npa_90_day_rule",
                "description": "RBI NPA classification starts at 90 days overdue",
                "evaluate": npa_classification,
                "applicable_states": ["REPAYING"],
                "resource_types": ["loan"],
                "priority": 85,
            },
            {
                "name": "bureau_watchlist",
                "description": "Watchlist applicants cannot be sanctioned",
                "evaluate": bureau_watchlist,
                "resource_types": ["loan"],
                "priority": 99,
            },
        ],
        invariants=[
            {
                "name": "positive_amount",
                "description": "Loan principal must be positive",
                "mode": "strong",
                "scope": "resource",
                "check": positive_amount,
                "resource_types": ["loan"],
            },
            {
                "name": "interest_rate_limit",
                "description": "Rates must remain inside the allowed band",
                "mode": "strong",
                "scope": "resource",
                "check": valid_interest_rate,
                "resource_types": ["loan"],
            },
            {
                "name": "applicant_name_required",
                "description": "Applicants must be identified",
                "mode": "strong",
                "scope": "resource",
                "check": applicant_has_name,
                "resource_types": ["applicant"],
            },
        ],
        relationships=[
            {"to_type": "applicant", "relation": "belongs_to", "required": True, "foreign_key": "applicant_id"},
            {"to_type": "collateral", "relation": "secured_by", "required": False, "foreign_key": "collateral_id"},
        ],
        agents=[
            {"name": "risk_scorer", "observe": risk_scorer, "on_events": ["loan.transitioned"], "priority": 80},
            {"name": "fraud_detector", "observe": fraud_detector, "on_events": ["loan.transitioned"], "priority": 70},
        ],
        decision_nodes=[
            {
                "name": "auto_underwriting",
                "agents": ["risk_scorer", "fraud_detector"],
                "aggregation": "weighted_avg",
                "auto_accept": 0.9,
                "auto_reject": 0.3,
            }
        ],
        database_url=database_url,
    )

    system.register_type(
        "applicant",
        ["ACTIVE", "UNDER_REVIEW", "SUSPENDED", "BLACKLISTED"],
        [
            ("ACTIVE", "UNDER_REVIEW"),
            ("UNDER_REVIEW", "ACTIVE"),
            ("UNDER_REVIEW", "SUSPENDED"),
            ("SUSPENDED", "ACTIVE"),
            ("ACTIVE", "BLACKLISTED"),
            ("SUSPENDED", "BLACKLISTED"),
        ],
        "ACTIVE",
        ["BLACKLISTED"],
    )
    system.register_type(
        "collateral",
        ["PENDING_APPRAISAL", "APPRAISED", "PLEDGED", "RELEASED", "SEIZED"],
        [
            ("PENDING_APPRAISAL", "APPRAISED"),
            ("APPRAISED", "PLEDGED"),
            ("PLEDGED", "RELEASED"),
            ("PLEDGED", "SEIZED"),
        ],
        "PENDING_APPRAISAL",
        ["RELEASED", "SEIZED"],
    )

    return system
