"""Platform multi-app isolation tests.

Proves that multiple apps on one Reconcile instance are completely
isolated — different types, different roles, different data,
no cross-app visibility.
"""

import pytest
from reconcile import define_system, PolicyResult, ReconcilePlatform
from reconcile.platform import AppNotFoundError


@pytest.fixture
def platform():
    p = ReconcilePlatform()

    # App A: Banking LOS
    bank = define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED", "REJECTED"],
        transitions=[
            ("APPLIED", "UNDERWRITING"), ("UNDERWRITING", "APPROVED"),
            ("UNDERWRITING", "REJECTED"), ("APPROVED", "DISBURSED"),
        ],
        terminal_states=["DISBURSED", "REJECTED"],
        roles={"officer": ["view", "transition:*"]},
    )
    p.register_app("bank_a", bank)

    # App B: Insurance claims
    insurance = define_system(
        name="claim",
        states=["FILED", "INVESTIGATING", "APPROVED", "PAID", "DENIED"],
        transitions=[
            ("FILED", "INVESTIGATING"), ("INVESTIGATING", "APPROVED"),
            ("INVESTIGATING", "DENIED"), ("APPROVED", "PAID"),
        ],
        terminal_states=["PAID", "DENIED"],
        roles={"adjuster": ["view", "transition:*"]},
    )
    p.register_app("insurance_b", insurance)

    return p


class TestAppIsolation:
    def test_apps_have_separate_data(self, platform):
        # Create resources in each app
        loan = platform.create("bank_a", "loan", {"amount": 500_000}, actor="u1")
        claim = platform.create("insurance_b", "claim", {"amount": 25_000}, actor="u2")

        assert loan.success
        assert claim.success

        # Each app sees only its own resources
        bank_resource = platform.get("bank_a", loan.resource.id)
        assert bank_resource is not None
        assert bank_resource.resource_type == "loan"

        insurance_resource = platform.get("insurance_b", claim.resource.id)
        assert insurance_resource is not None
        assert insurance_resource.resource_type == "claim"

        # Cross-app: bank can't see insurance claim
        assert platform.get("bank_a", claim.resource.id) is None
        # Cross-app: insurance can't see bank loan
        assert platform.get("insurance_b", loan.resource.id) is None

    def test_transitions_scoped_to_app(self, platform):
        loan = platform.create("bank_a", "loan", {"amount": 100_000}, actor="u1")
        rid = loan.resource.id

        # Transition in bank app
        r = platform.transition("bank_a", rid, "UNDERWRITING",
                                actor="u1", role="officer")
        assert r.success

        # Can't transition the same ID in insurance app (doesn't exist there)
        with pytest.raises(Exception):
            platform.transition("insurance_b", rid, "INVESTIGATING",
                                actor="u1", role="adjuster")

    def test_projections_scoped_to_app(self, platform):
        loan = platform.create("bank_a", "loan", {"amount": 100_000}, actor="u1")

        # Bank projection works
        p = platform.project("bank_a", loan.resource.id, "officer")
        assert p.resource["resource_type"] == "loan"
        assert len(p.valid_actions) > 0

        # Insurance projection for same ID fails (doesn't exist there)
        with pytest.raises(Exception):
            platform.project("insurance_b", loan.resource.id, "adjuster")

    def test_specs_are_independent(self, platform):
        bank_spec = platform.export_spec("bank_a")
        insurance_spec = platform.export_spec("insurance_b")

        bank_types = [t["name"] for t in bank_spec["types"]]
        insurance_types = [t["name"] for t in insurance_spec["types"]]

        assert "loan" in bank_types
        assert "claim" not in bank_types

        assert "claim" in insurance_types
        assert "loan" not in insurance_types


class TestAppLifecycle:
    def test_list_apps(self, platform):
        apps = platform.list_apps()
        assert "bank_a" in apps
        assert "insurance_b" in apps

    def test_register_duplicate_fails(self, platform):
        sys = define_system(name="x", states=["A"], transitions=[], terminal_states=["A"])
        with pytest.raises(ValueError, match="already registered"):
            platform.register_app("bank_a", sys)

    def test_remove_app(self, platform):
        platform.remove_app("bank_a")
        assert "bank_a" not in platform.list_apps()

        with pytest.raises(AppNotFoundError):
            platform.get("bank_a", "any-id")

    def test_nonexistent_app(self, platform):
        with pytest.raises(AppNotFoundError):
            platform.get("nonexistent", "any-id")


class TestGovernedLLMPerApp:
    def test_governed_llm_scoped_to_app(self, platform):
        loan = platform.create("bank_a", "loan", {"amount": 100_000}, actor="u1")

        # GovernedLLM for bank app
        governed = platform.governed_llm("bank_a", actor="officer1", role="officer")
        result = governed.handle_tool_call("execute_action", {
            "resource_id": loan.resource.id,
            "action": "UNDERWRITING",
        })
        assert result["success"] is True

    def test_governed_llm_cant_cross_apps(self, platform):
        loan = platform.create("bank_a", "loan", {"amount": 100_000}, actor="u1")

        # GovernedLLM for insurance app can't see bank's loan
        governed = platform.governed_llm("insurance_b", actor="adj1", role="adjuster")
        result = governed.handle_tool_call("get_resource", {
            "resource_id": loan.resource.id,
        })
        assert "error" in result or result.get("state") is None


class TestMultiAppAtScale:
    def test_10_apps_independent(self):
        platform = ReconcilePlatform()

        # Register 10 apps
        for i in range(10):
            sys = define_system(
                name=f"item_{i}",
                states=["NEW", "DONE"],
                transitions=[("NEW", "DONE")],
                terminal_states=["DONE"],
            )
            platform.register_app(f"app_{i}", sys)

        assert len(platform.list_apps()) == 10

        # Create resource in each app
        ids = {}
        for i in range(10):
            r = platform.create(f"app_{i}", f"item_{i}", {"index": i}, actor="u")
            ids[i] = r.resource.id

        # Each app sees only its own resource
        for i in range(10):
            r = platform.get(f"app_{i}", ids[i])
            assert r is not None
            assert r.data["index"] == i

            # Can't see other apps' resources
            for j in range(10):
                if j != i:
                    assert platform.get(f"app_{i}", ids[j]) is None
