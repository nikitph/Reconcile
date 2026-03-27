"""
Government Building Permit System — Complex Regulatory Domain
=============================================================

20 states, 55 transitions, 50 policies, 8 roles, 5 invariants.
Models a real municipal building permit lifecycle with:
- Multi-department review (zoning, environmental, structural, fire, accessibility)
- Inspection stages (foundation, framing, electrical, plumbing, final)
- Appeal process for rejections
- Conditional approvals with restrictions
- Fee-based policies (permit fees, inspection fees)
- Time-based policies (permit expiry, inspection windows)
- Cross-resource relationships (permit → property, permit → contractor)
"""

from reconcile._native import ReconcileSystem, PolicyResult, InvariantResult


def create_building_permit_system(database_url: str | None = None) -> "SystemWrapper":
    from reconcile.dsl import SystemWrapper

    sys = ReconcileSystem(database_url=database_url)

    # =========================================================================
    # 20 STATES
    # =========================================================================
    sys.register_type(
        "permit",
        [
            # Application phase
            "DRAFT",                    # 1  Applicant filling out form
            "SUBMITTED",                # 2  Application submitted for review
            "FEE_PENDING",              # 3  Waiting for application fee payment
            "UNDER_REVIEW",             # 4  Assigned to review queue

            # Department reviews (parallel in real life, sequential here)
            "ZONING_REVIEW",            # 5  Zoning compliance check
            "ENVIRONMENTAL_REVIEW",     # 6  Environmental impact assessment
            "STRUCTURAL_REVIEW",        # 7  Structural engineering review
            "FIRE_SAFETY_REVIEW",       # 8  Fire code compliance
            "ACCESSIBILITY_REVIEW",     # 9  ADA/accessibility compliance

            # Decision
            "COMMITTEE_REVIEW",         # 10 Planning committee (complex projects)
            "CONDITIONAL_APPROVAL",     # 11 Approved with conditions/restrictions
            "APPROVED",                 # 12 Fully approved, ready for construction

            # Construction & inspection
            "CONSTRUCTION_STARTED",     # 13 Construction begun
            "FOUNDATION_INSPECTION",    # 14 Foundation inspection
            "FRAMING_INSPECTION",       # 15 Framing/structural inspection
            "ELECTRICAL_INSPECTION",    # 16 Electrical systems inspection
            "PLUMBING_INSPECTION",      # 17 Plumbing inspection
            "FINAL_INSPECTION",         # 18 Final comprehensive inspection

            # Terminal states
            "OCCUPANCY_GRANTED",        # 19 Certificate of occupancy issued
            "REJECTED",                 # 20 Application denied
            "EXPIRED",                  # 21 Permit expired (not terminal - can renew)
            "REVOKED",                  # 22 Permit revoked for violations
            "APPEALED",                 # 23 Rejection under appeal
        ],
        [
            # Application flow (7 transitions)
            ("DRAFT", "SUBMITTED"),
            ("SUBMITTED", "FEE_PENDING"),
            ("FEE_PENDING", "UNDER_REVIEW"),
            ("UNDER_REVIEW", "ZONING_REVIEW"),
            ("SUBMITTED", "REJECTED"),          # Quick reject for incomplete apps

            # Department reviews — sequential chain (10 transitions)
            ("ZONING_REVIEW", "ENVIRONMENTAL_REVIEW"),
            ("ZONING_REVIEW", "REJECTED"),
            ("ENVIRONMENTAL_REVIEW", "STRUCTURAL_REVIEW"),
            ("ENVIRONMENTAL_REVIEW", "REJECTED"),
            ("STRUCTURAL_REVIEW", "FIRE_SAFETY_REVIEW"),
            ("STRUCTURAL_REVIEW", "REJECTED"),
            ("FIRE_SAFETY_REVIEW", "ACCESSIBILITY_REVIEW"),
            ("FIRE_SAFETY_REVIEW", "REJECTED"),
            ("ACCESSIBILITY_REVIEW", "APPROVED"),
            ("ACCESSIBILITY_REVIEW", "CONDITIONAL_APPROVAL"),
            ("ACCESSIBILITY_REVIEW", "COMMITTEE_REVIEW"),
            ("ACCESSIBILITY_REVIEW", "REJECTED"),

            # Committee and conditional (6 transitions)
            ("COMMITTEE_REVIEW", "APPROVED"),
            ("COMMITTEE_REVIEW", "CONDITIONAL_APPROVAL"),
            ("COMMITTEE_REVIEW", "REJECTED"),
            ("CONDITIONAL_APPROVAL", "APPROVED"),
            ("CONDITIONAL_APPROVAL", "REJECTED"),

            # Construction & inspections (10 transitions)
            ("APPROVED", "CONSTRUCTION_STARTED"),
            ("CONSTRUCTION_STARTED", "FOUNDATION_INSPECTION"),
            ("FOUNDATION_INSPECTION", "FRAMING_INSPECTION"),
            ("FOUNDATION_INSPECTION", "CONSTRUCTION_STARTED"),     # Failed → redo
            ("FRAMING_INSPECTION", "ELECTRICAL_INSPECTION"),
            ("FRAMING_INSPECTION", "CONSTRUCTION_STARTED"),        # Failed → redo
            ("ELECTRICAL_INSPECTION", "PLUMBING_INSPECTION"),
            ("ELECTRICAL_INSPECTION", "CONSTRUCTION_STARTED"),     # Failed → redo
            ("PLUMBING_INSPECTION", "FINAL_INSPECTION"),
            ("PLUMBING_INSPECTION", "CONSTRUCTION_STARTED"),       # Failed → redo

            # Final outcomes (8 transitions)
            ("FINAL_INSPECTION", "OCCUPANCY_GRANTED"),
            ("FINAL_INSPECTION", "CONSTRUCTION_STARTED"),          # Failed → redo
            ("APPROVED", "EXPIRED"),
            ("CONDITIONAL_APPROVAL", "EXPIRED"),
            ("EXPIRED", "UNDER_REVIEW"),            # Renewal
            ("REJECTED", "APPEALED"),
            ("APPEALED", "UNDER_REVIEW"),            # Appeal granted → re-review
            ("APPEALED", "REJECTED"),                # Appeal denied

            # Revocation (4 transitions)
            ("CONSTRUCTION_STARTED", "REVOKED"),
            ("APPROVED", "REVOKED"),
            ("CONDITIONAL_APPROVAL", "REVOKED"),
            ("OCCUPANCY_GRANTED", "REVOKED"),

            # Rework paths (5 transitions)
            ("ZONING_REVIEW", "DRAFT"),              # Send back for corrections
            ("ENVIRONMENTAL_REVIEW", "DRAFT"),
            ("STRUCTURAL_REVIEW", "DRAFT"),
            ("UNDER_REVIEW", "DRAFT"),
            ("FEE_PENDING", "DRAFT"),
        ],
        "DRAFT",
        ["REVOKED"],  # Only REVOKED is truly terminal
    )

    # Supporting types
    sys.register_type("property", ["REGISTERED", "FLAGGED"],
                      [("REGISTERED", "FLAGGED")], "REGISTERED", ["FLAGGED"])
    sys.register_type("contractor", ["ACTIVE", "SUSPENDED", "REVOKED"],
                      [("ACTIVE", "SUSPENDED"), ("SUSPENDED", "ACTIVE"), ("ACTIVE", "REVOKED")],
                      "ACTIVE", ["REVOKED"])

    # Relationships
    sys.register_relationship("permit", "property", "for_property", "many_to_one", True, "property_id")
    sys.register_relationship("permit", "contractor", "assigned_to", "many_to_one", False, "contractor_id")

    # =========================================================================
    # 8 ROLES
    # =========================================================================

    sys.register_role("applicant", [
        "view", "transition:SUBMITTED",
    ], ["project_name", "project_type", "estimated_cost", "status"])

    sys.register_role("intake_clerk", [
        "view", "transition:FEE_PENDING", "transition:REJECTED", "transition:DRAFT",
    ])

    sys.register_role("zoning_officer", [
        "view", "transition:ZONING_REVIEW", "transition:ENVIRONMENTAL_REVIEW",
        "transition:REJECTED", "transition:DRAFT",
    ])

    sys.register_role("env_officer", [
        "view", "transition:STRUCTURAL_REVIEW", "transition:REJECTED", "transition:DRAFT",
    ])

    sys.register_role("structural_engineer", [
        "view", "transition:FIRE_SAFETY_REVIEW", "transition:REJECTED", "transition:DRAFT",
    ])

    sys.register_role("fire_marshal", [
        "view", "transition:ACCESSIBILITY_REVIEW", "transition:REJECTED",
    ])

    sys.register_role("building_inspector", [
        "view",
        "transition:FOUNDATION_INSPECTION", "transition:FRAMING_INSPECTION",
        "transition:ELECTRICAL_INSPECTION", "transition:PLUMBING_INSPECTION",
        "transition:FINAL_INSPECTION", "transition:OCCUPANCY_GRANTED",
        "transition:CONSTRUCTION_STARTED",  # Failed inspection → back to construction
    ])

    sys.register_role("planning_director", [
        "view", "transition:*",
    ])

    # =========================================================================
    # 50 POLICIES
    # =========================================================================

    policies = []

    # --- Fee policies (5) ---
    def p_fee_required(r, ctx, q):
        if ctx.get("to_state") == "UNDER_REVIEW":
            if not r.data.get("fee_paid"):
                return PolicyResult.deny("Application fee must be paid before review")
        return PolicyResult.allow()
    policies.append({"name": "fee_required", "evaluate": p_fee_required, "applicable_states": ["FEE_PENDING"], "priority": 95})

    def p_inspection_fee(r, ctx, q):
        if ctx.get("to_state") == "CONSTRUCTION_STARTED":
            if not r.data.get("inspection_fee_paid"):
                return PolicyResult.deny("Inspection fee required before construction")
        return PolicyResult.allow()
    policies.append({"name": "inspection_fee", "evaluate": p_inspection_fee, "applicable_states": ["APPROVED"], "priority": 90})

    # --- Zoning policies (8) ---
    for zone_type, allowed in [
        ("residential", ["single_family", "duplex", "townhouse"]),
        ("commercial", ["office", "retail", "restaurant"]),
        ("industrial", ["warehouse", "factory", "workshop"]),
        ("mixed_use", ["live_work", "retail_residential"]),
    ]:
        def make_zoning_policy(zt, al):
            def p(r, ctx, q):
                if ctx.get("to_state") == "ENVIRONMENTAL_REVIEW":
                    if r.data.get("zone") == zt and r.data.get("project_type") not in al:
                        return PolicyResult.deny(f"Project type '{r.data.get('project_type')}' not allowed in {zt} zone")
                return PolicyResult.allow()
            return p
        policies.append({
            "name": f"zoning_{zone_type}",
            "evaluate": make_zoning_policy(zone_type, allowed),
            "applicable_states": ["ZONING_REVIEW"],
            "priority": 85,
        })

    # Additional zoning: setback requirements
    for direction in ["front", "rear", "side_left", "side_right"]:
        def make_setback_policy(d):
            def p(r, ctx, q):
                if ctx.get("to_state") == "ENVIRONMENTAL_REVIEW":
                    setback = r.data.get(f"setback_{d}", 0)
                    min_setback = {"front": 25, "rear": 20, "side_left": 10, "side_right": 10}.get(d, 10)
                    if setback < min_setback:
                        return PolicyResult.deny(f"{d} setback {setback}ft below minimum {min_setback}ft")
                return PolicyResult.allow()
            return p
        policies.append({
            "name": f"setback_{direction}",
            "evaluate": make_setback_policy(direction),
            "applicable_states": ["ZONING_REVIEW"],
            "priority": 80,
        })

    # --- Environmental policies (6) ---
    def p_env_impact(r, ctx, q):
        if ctx.get("to_state") == "STRUCTURAL_REVIEW":
            cost = r.data.get("estimated_cost", 0)
            if cost > 5_000_000 and not r.data.get("environmental_impact_study"):
                return PolicyResult.deny("Projects > $5M require environmental impact study")
        return PolicyResult.allow()
    policies.append({"name": "env_impact_study", "evaluate": p_env_impact, "applicable_states": ["ENVIRONMENTAL_REVIEW"], "priority": 85})

    def p_wetland(r, ctx, q):
        if ctx.get("to_state") == "STRUCTURAL_REVIEW":
            if r.data.get("near_wetland") and not r.data.get("wetland_mitigation_plan"):
                return PolicyResult.deny("Wetland mitigation plan required")
        return PolicyResult.allow()
    policies.append({"name": "wetland_protection", "evaluate": p_wetland, "applicable_states": ["ENVIRONMENTAL_REVIEW"], "priority": 88})

    def p_tree_preservation(r, ctx, q):
        if ctx.get("to_state") == "STRUCTURAL_REVIEW":
            trees = r.data.get("trees_removed", 0)
            if trees > 5 and not r.data.get("tree_replacement_plan"):
                return PolicyResult.deny(f"Removing {trees} trees requires replacement plan")
        return PolicyResult.allow()
    policies.append({"name": "tree_preservation", "evaluate": p_tree_preservation, "applicable_states": ["ENVIRONMENTAL_REVIEW"], "priority": 82})

    def p_stormwater(r, ctx, q):
        if ctx.get("to_state") == "STRUCTURAL_REVIEW":
            sqft = r.data.get("lot_size_sqft", 0)
            if sqft > 10000 and not r.data.get("stormwater_plan"):
                return PolicyResult.deny("Stormwater management plan required for lots > 10,000 sqft")
        return PolicyResult.allow()
    policies.append({"name": "stormwater_mgmt", "evaluate": p_stormwater, "applicable_states": ["ENVIRONMENTAL_REVIEW"], "priority": 80})

    def p_noise(r, ctx, q):
        if ctx.get("to_state") == "STRUCTURAL_REVIEW":
            if r.data.get("zone") == "residential" and r.data.get("project_type") in ["factory", "workshop"]:
                return PolicyResult.deny("Industrial projects not permitted in residential zones")
        return PolicyResult.allow()
    policies.append({"name": "noise_ordinance", "evaluate": p_noise, "applicable_states": ["ENVIRONMENTAL_REVIEW"], "priority": 85})

    def p_historic_district(r, ctx, q):
        if ctx.get("to_state") == "STRUCTURAL_REVIEW":
            if r.data.get("historic_district") and not r.data.get("historic_review_complete"):
                return PolicyResult.deny("Historic district review required")
        return PolicyResult.allow()
    policies.append({"name": "historic_preservation", "evaluate": p_historic_district, "applicable_states": ["ENVIRONMENTAL_REVIEW"], "priority": 90})

    # --- Structural policies (5) ---
    def p_max_height(r, ctx, q):
        if ctx.get("to_state") == "FIRE_SAFETY_REVIEW":
            height = r.data.get("building_height_ft", 0)
            zone = r.data.get("zone", "residential")
            max_h = {"residential": 35, "commercial": 75, "industrial": 60, "mixed_use": 55}.get(zone, 35)
            if height > max_h:
                return PolicyResult.deny(f"Height {height}ft exceeds {zone} zone max of {max_h}ft")
        return PolicyResult.allow()
    policies.append({"name": "max_height", "evaluate": p_max_height, "applicable_states": ["STRUCTURAL_REVIEW"], "priority": 85})

    def p_lot_coverage(r, ctx, q):
        if ctx.get("to_state") == "FIRE_SAFETY_REVIEW":
            coverage = r.data.get("lot_coverage_pct", 0)
            if coverage > 60:
                return PolicyResult.deny(f"Lot coverage {coverage}% exceeds 60% maximum")
        return PolicyResult.allow()
    policies.append({"name": "lot_coverage", "evaluate": p_lot_coverage, "applicable_states": ["STRUCTURAL_REVIEW"], "priority": 80})

    def p_foundation_type(r, ctx, q):
        if ctx.get("to_state") == "FIRE_SAFETY_REVIEW":
            stories = r.data.get("stories", 1)
            if stories > 3 and r.data.get("foundation_type") != "deep":
                return PolicyResult.deny("Buildings > 3 stories require deep foundation")
        return PolicyResult.allow()
    policies.append({"name": "foundation_req", "evaluate": p_foundation_type, "applicable_states": ["STRUCTURAL_REVIEW"], "priority": 82})

    def p_seismic(r, ctx, q):
        if ctx.get("to_state") == "FIRE_SAFETY_REVIEW":
            if r.data.get("seismic_zone") == "high" and not r.data.get("seismic_engineering"):
                return PolicyResult.deny("Seismic engineering report required in high seismic zones")
        return PolicyResult.allow()
    policies.append({"name": "seismic_compliance", "evaluate": p_seismic, "applicable_states": ["STRUCTURAL_REVIEW"], "priority": 88})

    def p_wind_load(r, ctx, q):
        if ctx.get("to_state") == "FIRE_SAFETY_REVIEW":
            height = r.data.get("building_height_ft", 0)
            if height > 50 and not r.data.get("wind_load_analysis"):
                return PolicyResult.deny("Wind load analysis required for buildings > 50ft")
        return PolicyResult.allow()
    policies.append({"name": "wind_load", "evaluate": p_wind_load, "applicable_states": ["STRUCTURAL_REVIEW"], "priority": 80})

    # --- Fire safety policies (5) ---
    def p_sprinklers(r, ctx, q):
        if ctx.get("to_state") == "ACCESSIBILITY_REVIEW":
            sqft = r.data.get("building_sqft", 0)
            if sqft > 5000 and not r.data.get("sprinkler_system"):
                return PolicyResult.deny("Sprinkler system required for buildings > 5,000 sqft")
        return PolicyResult.allow()
    policies.append({"name": "sprinkler_req", "evaluate": p_sprinklers, "applicable_states": ["FIRE_SAFETY_REVIEW"], "priority": 90})

    def p_fire_exits(r, ctx, q):
        if ctx.get("to_state") == "ACCESSIBILITY_REVIEW":
            stories = r.data.get("stories", 1)
            exits = r.data.get("fire_exits", 0)
            required = max(2, stories)
            if exits < required:
                return PolicyResult.deny(f"Need {required} fire exits, have {exits}")
        return PolicyResult.allow()
    policies.append({"name": "fire_exits", "evaluate": p_fire_exits, "applicable_states": ["FIRE_SAFETY_REVIEW"], "priority": 88})

    def p_fire_lane(r, ctx, q):
        if ctx.get("to_state") == "ACCESSIBILITY_REVIEW":
            if r.data.get("project_type") in ["commercial", "office", "retail"] and not r.data.get("fire_lane"):
                return PolicyResult.deny("Fire lane access required for commercial buildings")
        return PolicyResult.allow()
    policies.append({"name": "fire_lane", "evaluate": p_fire_lane, "applicable_states": ["FIRE_SAFETY_REVIEW"], "priority": 85})

    def p_smoke_detectors(r, ctx, q):
        if ctx.get("to_state") == "ACCESSIBILITY_REVIEW":
            if not r.data.get("smoke_detectors"):
                return PolicyResult.deny("Smoke detector plan required")
        return PolicyResult.allow()
    policies.append({"name": "smoke_detectors", "evaluate": p_smoke_detectors, "applicable_states": ["FIRE_SAFETY_REVIEW"], "priority": 82})

    def p_fire_rated_walls(r, ctx, q):
        if ctx.get("to_state") == "ACCESSIBILITY_REVIEW":
            if r.data.get("multi_tenant") and not r.data.get("fire_rated_walls"):
                return PolicyResult.deny("Fire-rated walls required for multi-tenant buildings")
        return PolicyResult.allow()
    policies.append({"name": "fire_rated_walls", "evaluate": p_fire_rated_walls, "applicable_states": ["FIRE_SAFETY_REVIEW"], "priority": 80})

    # --- Accessibility policies (4) ---
    def p_ada_entrance(r, ctx, q):
        if ctx.get("to_state") in ("APPROVED", "CONDITIONAL_APPROVAL", "COMMITTEE_REVIEW"):
            if r.data.get("public_building") and not r.data.get("ada_entrance"):
                return PolicyResult.deny("ADA-compliant entrance required for public buildings")
        return PolicyResult.allow()
    policies.append({"name": "ada_entrance", "evaluate": p_ada_entrance, "applicable_states": ["ACCESSIBILITY_REVIEW"], "priority": 88})

    def p_ada_parking(r, ctx, q):
        if ctx.get("to_state") in ("APPROVED", "CONDITIONAL_APPROVAL", "COMMITTEE_REVIEW"):
            parking = r.data.get("total_parking", 0)
            ada_spots = r.data.get("ada_parking", 0)
            if parking > 25 and ada_spots < max(1, parking // 25):
                return PolicyResult.deny(f"Need {max(1, parking // 25)} ADA parking spots, have {ada_spots}")
        return PolicyResult.allow()
    policies.append({"name": "ada_parking", "evaluate": p_ada_parking, "applicable_states": ["ACCESSIBILITY_REVIEW"], "priority": 85})

    def p_ada_elevator(r, ctx, q):
        if ctx.get("to_state") in ("APPROVED", "CONDITIONAL_APPROVAL", "COMMITTEE_REVIEW"):
            stories = r.data.get("stories", 1)
            if stories > 1 and r.data.get("public_building") and not r.data.get("elevator"):
                return PolicyResult.deny("Elevator required for multi-story public buildings")
        return PolicyResult.allow()
    policies.append({"name": "ada_elevator", "evaluate": p_ada_elevator, "applicable_states": ["ACCESSIBILITY_REVIEW"], "priority": 85})

    def p_ada_restroom(r, ctx, q):
        if ctx.get("to_state") in ("APPROVED", "CONDITIONAL_APPROVAL", "COMMITTEE_REVIEW"):
            if r.data.get("public_building") and not r.data.get("ada_restroom"):
                return PolicyResult.deny("ADA-compliant restroom required for public buildings")
        return PolicyResult.allow()
    policies.append({"name": "ada_restroom", "evaluate": p_ada_restroom, "applicable_states": ["ACCESSIBILITY_REVIEW"], "priority": 82})

    # --- Committee policies (3) ---
    def p_committee_threshold(r, ctx, q):
        if ctx.get("to_state") == "APPROVED" and ctx.get("from_state") == "ACCESSIBILITY_REVIEW":
            cost = r.data.get("estimated_cost", 0)
            if cost > 10_000_000:
                return PolicyResult.deny("Projects > $10M require committee review")
        return PolicyResult.allow()
    policies.append({"name": "committee_threshold", "evaluate": p_committee_threshold, "applicable_states": ["ACCESSIBILITY_REVIEW"], "priority": 95})

    def p_variance_required(r, ctx, q):
        if ctx.get("to_state") == "APPROVED":
            if r.data.get("requires_variance") and not r.data.get("variance_granted"):
                return PolicyResult.deny("Zoning variance must be granted before approval")
        return PolicyResult.allow()
    policies.append({"name": "variance_required", "evaluate": p_variance_required, "priority": 90})

    def p_neighbor_notification(r, ctx, q):
        if ctx.get("to_state") in ("APPROVED", "CONDITIONAL_APPROVAL"):
            if r.data.get("estimated_cost", 0) > 1_000_000 and not r.data.get("neighbors_notified"):
                return PolicyResult.deny("Adjacent property owners must be notified for projects > $1M")
        return PolicyResult.allow()
    policies.append({"name": "neighbor_notification", "evaluate": p_neighbor_notification, "applicable_states": ["ACCESSIBILITY_REVIEW", "COMMITTEE_REVIEW"], "priority": 80})

    # --- Inspection policies (5) ---
    def p_licensed_contractor(r, ctx, q):
        if ctx.get("to_state") == "CONSTRUCTION_STARTED":
            if not r.data.get("contractor_id"):
                return PolicyResult.deny("Licensed contractor must be assigned before construction")
        return PolicyResult.allow()
    policies.append({"name": "licensed_contractor", "evaluate": p_licensed_contractor, "applicable_states": ["APPROVED"], "priority": 90})

    def p_insurance_required(r, ctx, q):
        if ctx.get("to_state") == "CONSTRUCTION_STARTED":
            if not r.data.get("liability_insurance"):
                return PolicyResult.deny("Liability insurance certificate required")
        return PolicyResult.allow()
    policies.append({"name": "insurance_required", "evaluate": p_insurance_required, "applicable_states": ["APPROVED"], "priority": 88})

    def p_utility_clearance(r, ctx, q):
        if ctx.get("to_state") == "CONSTRUCTION_STARTED":
            if not r.data.get("utility_clearance"):
                return PolicyResult.deny("Utility company clearance required")
        return PolicyResult.allow()
    policies.append({"name": "utility_clearance", "evaluate": p_utility_clearance, "applicable_states": ["APPROVED"], "priority": 85})

    def p_erosion_control(r, ctx, q):
        if ctx.get("to_state") == "CONSTRUCTION_STARTED":
            sqft = r.data.get("lot_size_sqft", 0)
            if sqft > 5000 and not r.data.get("erosion_control_plan"):
                return PolicyResult.deny("Erosion control plan required for sites > 5,000 sqft")
        return PolicyResult.allow()
    policies.append({"name": "erosion_control", "evaluate": p_erosion_control, "applicable_states": ["APPROVED"], "priority": 80})

    def p_final_docs(r, ctx, q):
        if ctx.get("to_state") == "OCCUPANCY_GRANTED":
            if not r.data.get("as_built_drawings"):
                return PolicyResult.deny("As-built drawings required for occupancy certificate")
        return PolicyResult.allow()
    policies.append({"name": "as_built_drawings", "evaluate": p_final_docs, "applicable_states": ["FINAL_INSPECTION"], "priority": 85})

    # --- General policies (4) ---
    def p_complete_application(r, ctx, q):
        if ctx.get("to_state") == "FEE_PENDING":
            required = ["project_name", "project_type", "estimated_cost"]
            missing = [f for f in required if not r.data.get(f)]
            if missing:
                return PolicyResult.deny(f"Incomplete application: missing {', '.join(missing)}")
        return PolicyResult.allow()
    policies.append({"name": "complete_application", "evaluate": p_complete_application, "applicable_states": ["SUBMITTED"], "priority": 95})

    def p_valid_address(r, ctx, q):
        if ctx.get("to_state") == "FEE_PENDING":
            if not r.data.get("property_id"):
                return PolicyResult.deny("Valid property address/ID required")
        return PolicyResult.allow()
    policies.append({"name": "valid_address", "evaluate": p_valid_address, "applicable_states": ["SUBMITTED"], "priority": 90})

    def p_max_active_permits(r, ctx, q):
        if ctx.get("to_state") == "UNDER_REVIEW":
            property_id = r.data.get("property_id")
            if property_id:
                count = q.graph_degree(property_id, "for_property")
                if count > 3:
                    return PolicyResult.deny(f"Property already has {count} active permits (max 3)")
        return PolicyResult.allow()
    policies.append({"name": "max_active_permits", "evaluate": p_max_active_permits, "applicable_states": ["FEE_PENDING"], "priority": 85})

    def p_no_outstanding_violations(r, ctx, q):
        if ctx.get("to_state") == "UNDER_REVIEW":
            property_id = r.data.get("property_id")
            if property_id:
                prop = q.get_resource(property_id)
                if prop and prop.state == "FLAGGED":
                    return PolicyResult.deny("Property has outstanding violations")
        return PolicyResult.allow()
    policies.append({"name": "no_violations", "evaluate": p_no_outstanding_violations, "applicable_states": ["FEE_PENDING"], "priority": 92})

    # --- Additional construction policies (8) ---
    def p_soil_test(r, ctx, q):
        if ctx.get("to_state") == "FOUNDATION_INSPECTION":
            if not r.data.get("soil_test_report"):
                return PolicyResult.deny("Soil test report required before foundation inspection")
        return PolicyResult.allow()
    policies.append({"name": "soil_test", "evaluate": p_soil_test, "applicable_states": ["CONSTRUCTION_STARTED"], "priority": 85})

    def p_survey_required(r, ctx, q):
        if ctx.get("to_state") == "CONSTRUCTION_STARTED":
            if not r.data.get("boundary_survey"):
                return PolicyResult.deny("Boundary survey required before construction")
        return PolicyResult.allow()
    policies.append({"name": "boundary_survey", "evaluate": p_survey_required, "applicable_states": ["APPROVED"], "priority": 78})

    def p_grading_plan(r, ctx, q):
        if ctx.get("to_state") == "CONSTRUCTION_STARTED":
            sqft = r.data.get("lot_size_sqft", 0)
            if sqft > 20000 and not r.data.get("grading_plan"):
                return PolicyResult.deny("Grading plan required for lots > 20,000 sqft")
        return PolicyResult.allow()
    policies.append({"name": "grading_plan", "evaluate": p_grading_plan, "applicable_states": ["APPROVED"], "priority": 75})

    def p_electrical_panel(r, ctx, q):
        if ctx.get("to_state") == "PLUMBING_INSPECTION":
            if not r.data.get("electrical_panel_approved"):
                return PolicyResult.deny("Electrical panel must be approved before plumbing")
        return PolicyResult.allow()
    policies.append({"name": "electrical_panel", "evaluate": p_electrical_panel, "applicable_states": ["ELECTRICAL_INSPECTION"], "priority": 82})

    def p_plumbing_pressure(r, ctx, q):
        if ctx.get("to_state") == "FINAL_INSPECTION":
            if not r.data.get("pressure_test_passed"):
                return PolicyResult.deny("Plumbing pressure test must pass before final")
        return PolicyResult.allow()
    policies.append({"name": "pressure_test", "evaluate": p_plumbing_pressure, "applicable_states": ["PLUMBING_INSPECTION"], "priority": 80})

    def p_occupancy_max(r, ctx, q):
        if ctx.get("to_state") == "OCCUPANCY_GRANTED":
            if not r.data.get("max_occupancy_posted"):
                return PolicyResult.deny("Maximum occupancy must be posted")
        return PolicyResult.allow()
    policies.append({"name": "occupancy_posting", "evaluate": p_occupancy_max, "applicable_states": ["FINAL_INSPECTION"], "priority": 78})

    def p_landscaping(r, ctx, q):
        if ctx.get("to_state") == "OCCUPANCY_GRANTED":
            if r.data.get("zone") == "commercial" and not r.data.get("landscaping_complete"):
                return PolicyResult.deny("Landscaping must be complete for commercial occupancy")
        return PolicyResult.allow()
    policies.append({"name": "landscaping", "evaluate": p_landscaping, "applicable_states": ["FINAL_INSPECTION"], "priority": 70})

    def p_driveway_permit(r, ctx, q):
        if ctx.get("to_state") == "CONSTRUCTION_STARTED":
            if r.data.get("new_driveway") and not r.data.get("driveway_permit"):
                return PolicyResult.deny("Separate driveway permit required for new driveways")
        return PolicyResult.allow()
    policies.append({"name": "driveway_permit", "evaluate": p_driveway_permit, "applicable_states": ["APPROVED"], "priority": 72})

    # Register all policies
    for p in policies:
        sys.register_policy(
            p["name"], p.get("description", p["name"]),
            p["evaluate"],
            p.get("applicable_states", []),
            p.get("resource_types", ["permit"]),
            p.get("priority", 50),
        )

    # =========================================================================
    # 5 INVARIANTS
    # =========================================================================

    def inv_positive_cost(r, q):
        if r.resource_type == "permit":
            cost = r.data.get("estimated_cost", 0)
            if cost <= 0:
                return InvariantResult.violated(f"Estimated cost must be positive, got {cost}")
        return InvariantResult.ok()
    sys.register_invariant("positive_cost", "Cost > 0", "strong", "resource", inv_positive_cost, ["permit"])

    def inv_project_name(r, q):
        if r.resource_type == "permit" and not r.data.get("project_name"):
            return InvariantResult.violated("Project name required")
        return InvariantResult.ok()
    sys.register_invariant("project_name", "Name required", "strong", "resource", inv_project_name, ["permit"])

    def inv_valid_zone(r, q):
        if r.resource_type == "permit":
            zone = r.data.get("zone", "")
            if zone and zone not in ("residential", "commercial", "industrial", "mixed_use", "agricultural"):
                return InvariantResult.violated(f"Invalid zone: {zone}")
        return InvariantResult.ok()
    sys.register_invariant("valid_zone", "Zone validation", "strong", "resource", inv_valid_zone, ["permit"])

    def inv_max_cost(r, q):
        if r.resource_type == "permit":
            cost = r.data.get("estimated_cost", 0)
            if cost > 500_000_000:
                return InvariantResult.violated("Cost exceeds $500M maximum for municipal permits")
        return InvariantResult.ok()
    sys.register_invariant("max_cost", "Cost < $500M", "strong", "resource", inv_max_cost, ["permit"])

    def inv_stories_limit(r, q):
        if r.resource_type == "permit":
            stories = r.data.get("stories", 0)
            if stories > 100:
                return InvariantResult.violated(f"Building height {stories} stories exceeds maximum")
        return InvariantResult.ok()
    sys.register_invariant("stories_limit", "Max 100 stories", "strong", "resource", inv_stories_limit, ["permit"])

    # =========================================================================
    # 2 AGENTS
    # =========================================================================

    def risk_assessor(r, q):
        if r.resource_type != "permit" or r.state != "UNDER_REVIEW":
            return None
        cost = r.data.get("estimated_cost", 0)
        stories = r.data.get("stories", 1)
        risk = 0.1
        if cost > 5_000_000: risk += 0.3
        if stories > 3: risk += 0.2
        if r.data.get("near_wetland"): risk += 0.15
        if r.data.get("historic_district"): risk += 0.1
        conf = max(0.1, 1.0 - risk)
        return {
            "transition": "ZONING_REVIEW",
            "confidence": conf,
            "reasoning": f"Risk score {risk:.0%}: cost=${cost:,}, {stories} stories",
        }

    sys.register_agent("risk_assessor", risk_assessor, ["permit.transitioned"], 80)

    def compliance_checker(r, q):
        if r.resource_type != "permit" or r.state not in ("ZONING_REVIEW", "ENVIRONMENTAL_REVIEW"):
            return None
        issues = []
        if not r.data.get("zone"): issues.append("missing zone")
        if not r.data.get("project_type"): issues.append("missing type")
        if r.data.get("estimated_cost", 0) > 10_000_000 and not r.data.get("environmental_impact_study"):
            issues.append("needs EIS")
        if issues:
            return {"flag": f"Compliance issues: {', '.join(issues)}", "confidence": 0.3,
                    "reasoning": f"{len(issues)} issue(s) found"}
        return {"transition": "APPROVED", "confidence": 0.85, "reasoning": "All checks pass"}

    sys.register_agent("compliance_checker", compliance_checker, ["permit.transitioned"], 70)

    sys.register_decision_node("auto_review", ["risk_assessor", "compliance_checker"],
                               auto_accept=0.9, auto_reject=0.3)

    return SystemWrapper(sys, "permit")
