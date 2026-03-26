"""PostgreSQL integration tests.

These tests require a running PostgreSQL instance with a `reconcile_test` database.
Skip automatically if connection fails.

Run with: pytest tests/python/test_postgres.py -v
"""

import pytest
from reconcile import define_system, PolicyResult, InvariantResult

PG_URL = "host=localhost dbname=reconcile_test"


def pg_available():
    """Check if we can connect to the test PG database."""
    try:
        from reconcile._native import ReconcileSystem
        sys = ReconcileSystem(database_url=PG_URL)
        return True
    except Exception:
        return False


# Skip all tests in this module if PG is not available
pytestmark = pytest.mark.skipif(
    not pg_available(),
    reason="PostgreSQL not available at reconcile_test"
)


@pytest.fixture(autouse=True)
def clean_pg():
    """Truncate all tables before each test."""
    try:
        import subprocess
        subprocess.run(
            ["psql", "reconcile_test", "-c",
             "TRUNCATE snapshots, audit_log, events, resources CASCADE"],
            capture_output=True, check=True,
        )
    except Exception:
        pass
    yield


@pytest.fixture
def pg_loan_system():
    """Loan system backed by PostgreSQL."""
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
        roles={"manager": ["view", "transition:*"]},
        database_url=PG_URL,
    )


class TestPgBasicLifecycle:
    """Full lifecycle through PostgreSQL."""

    def test_create_and_get(self, pg_loan_system):
        sys = pg_loan_system
        result = sys.create({"amount": 500_000, "applicant": "Acme"}, actor="u1")
        assert result.success
        assert result.resource.state == "APPLIED"

        # Fetch back from PG
        fetched = sys.get(result.resource.id)
        assert fetched is not None
        assert fetched.state == "APPLIED"
        assert fetched.data["amount"] == 500_000
        assert fetched.data["applicant"] == "Acme"

    def test_full_lifecycle(self, pg_loan_system):
        sys = pg_loan_system
        result = sys.create({"amount": 100_000}, actor="u1")
        rid = result.resource.id

        sys.transition(rid, "UNDERWRITING", actor="u1", role="manager")
        sys.transition(rid, "APPROVED", actor="u1", role="manager")
        sys.transition(rid, "DISBURSED", actor="u1", role="manager")

        final = sys.get(rid)
        assert final.state == "DISBURSED"
        assert final.version == 4

    def test_rejection_path(self, pg_loan_system):
        sys = pg_loan_system
        result = sys.create({"amount": 100_000}, actor="u1")
        rid = result.resource.id

        sys.transition(rid, "UNDERWRITING", actor="u1", role="manager")
        r = sys.transition(rid, "REJECTED", actor="u1", role="manager")
        assert r.success
        assert sys.get(rid).state == "REJECTED"


class TestPgAuditAndEvents:
    """Verify audit and events are persisted in PG."""

    def test_audit_persisted(self, pg_loan_system):
        sys = pg_loan_system
        result = sys.create({"amount": 100_000}, actor="u1")
        rid = result.resource.id

        sys.transition(rid, "UNDERWRITING", actor="alice", role="manager")
        sys.transition(rid, "APPROVED", actor="bob", role="manager")

        audit = sys.audit(rid)
        assert len(audit) == 2
        assert audit[0].actor == "alice"
        assert audit[0].new_state == "UNDERWRITING"
        assert audit[1].actor == "bob"
        assert audit[1].new_state == "APPROVED"

    def test_events_persisted(self, pg_loan_system):
        sys = pg_loan_system
        result = sys.create({"amount": 100_000}, actor="u1")
        rid = result.resource.id

        sys.transition(rid, "UNDERWRITING", actor="u1", role="manager")

        events = sys.events(rid)
        assert len(events) >= 2
        assert events[0].event_type == "loan.created"
        assert events[1].event_type == "loan.transitioned"

    def test_events_ordered(self, pg_loan_system):
        sys = pg_loan_system
        result = sys.create({"amount": 100_000}, actor="u1")
        rid = result.resource.id

        sys.transition(rid, "UNDERWRITING", actor="u1", role="manager")
        sys.transition(rid, "APPROVED", actor="u1", role="manager")
        sys.transition(rid, "DISBURSED", actor="u1", role="manager")

        events = sys.events(rid)
        offsets = [e.offset for e in events]
        assert offsets == sorted(offsets)


class TestPgPersistence:
    """Verify data survives across separate system instances (simulating restart)."""

    def test_data_survives_reconnect(self):
        """Create with one system, read with another (simulating restart)."""
        # System 1: create and transition
        sys1 = define_system(
            name="loan",
            states=["APPLIED", "UNDERWRITING", "DONE"],
            transitions=[("APPLIED", "UNDERWRITING"), ("UNDERWRITING", "DONE")],
            terminal_states=["DONE"],
            database_url=PG_URL,
        )

        result = sys1.create({"amount": 999}, actor="u1")
        rid = result.resource.id
        sys1.transition(
            rid, "UNDERWRITING", actor="u1", role="x",
            authority_level="CONTROLLER",
        )

        # System 2: reconnect and verify state persisted
        sys2 = define_system(
            name="loan",
            states=["APPLIED", "UNDERWRITING", "DONE"],
            transitions=[("APPLIED", "UNDERWRITING"), ("UNDERWRITING", "DONE")],
            terminal_states=["DONE"],
            database_url=PG_URL,
        )

        resource = sys2.get(rid)
        assert resource is not None
        assert resource.state == "UNDERWRITING"
        assert resource.data["amount"] == 999

        # Continue from where we left off
        r = sys2.transition(
            rid, "DONE", actor="u2", role="x",
            authority_level="CONTROLLER",
        )
        assert r.success
        assert sys2.get(rid).state == "DONE"

        # Events span both sessions
        events = sys2.events(rid)
        assert len(events) >= 3


class TestPgPoliciesAndInvariants:
    """Policies and invariants work with PG backend."""

    def test_policy_with_pg(self):
        def amount_limit(resource, ctx):
            if resource.data.get("amount", 0) > 1_000_000:
                return PolicyResult.deny("Too much")
            return PolicyResult.allow()

        sys = define_system(
            name="loan",
            states=["APPLIED", "APPROVED"],
            transitions=[("APPLIED", "APPROVED")],
            terminal_states=["APPROVED"],
            policies=[{"name": "limit", "evaluate": amount_limit}],
            database_url=PG_URL,
        )

        # Under limit
        r1 = sys.create({"amount": 500_000}, actor="u1")
        result = sys.transition(
            r1.resource.id, "APPROVED", actor="u1", role="x",
            authority_level="CONTROLLER",
        )
        assert result.success

        # Over limit
        r2 = sys.create({"amount": 5_000_000}, actor="u1")
        result = sys.transition(
            r2.resource.id, "APPROVED", actor="u1", role="x",
            authority_level="CONTROLLER",
        )
        assert not result.success
        assert "limit" in result.rejected_reason.lower()

    def test_invariant_with_pg(self):
        def positive(resource):
            if resource.data.get("amount", 0) <= 0:
                return InvariantResult.violated("Must be positive")
            return InvariantResult.ok()

        sys = define_system(
            name="loan",
            states=["APPLIED", "DONE"],
            transitions=[("APPLIED", "DONE")],
            terminal_states=["DONE"],
            invariants=[{
                "name": "positive",
                "mode": "strong",
                "scope": "resource",
                "check": positive,
            }],
            database_url=PG_URL,
        )

        # Positive - allowed
        r1 = sys.create({"amount": 100}, actor="u1")
        assert r1.success

        # Negative - blocked
        r2 = sys.create({"amount": -50}, actor="u1")
        assert not r2.success


class TestPgDesiredState:
    """Desired state reconciliation through PG."""

    def test_desired_state_reconciles(self, pg_loan_system):
        sys = pg_loan_system
        result = sys.create({"amount": 100_000}, actor="u1")
        rid = result.resource.id

        sys.set_desired(rid, "DISBURSED", requested_by="mgr")
        assert sys.get(rid).state == "DISBURSED"


class TestPgSnapshots:
    """Snapshot creation with PG backend."""

    def test_auto_snapshots(self):
        sys = define_system(
            name="item",
            states=["A", "B", "C", "D"],
            transitions=[("A", "B"), ("B", "C"), ("C", "D")],
            terminal_states=["D"],
            database_url=PG_URL,
            snapshot_interval=2,
        )

        result = sys.create({}, actor="u1")
        rid = result.resource.id

        sys.transition(rid, "B", actor="u1", role="x", authority_level="CONTROLLER")
        sys.transition(rid, "C", actor="u1", role="x", authority_level="CONTROLLER")
        sys.transition(rid, "D", actor="u1", role="x", authority_level="CONTROLLER")

        assert sys.get(rid).state == "D"
        assert sys.get(rid).version == 4


class TestPgMultipleResources:
    """Multiple resources in PG."""

    def test_independent_resources(self, pg_loan_system):
        sys = pg_loan_system

        ids = []
        for i in range(5):
            r = sys.create({"amount": (i + 1) * 10_000}, actor=f"u{i}")
            ids.append(r.resource.id)

        # Advance only first 3
        for rid in ids[:3]:
            sys.transition(rid, "UNDERWRITING", actor="u1", role="manager")

        # Verify states
        for i, rid in enumerate(ids):
            resource = sys.get(rid)
            if i < 3:
                assert resource.state == "UNDERWRITING"
            else:
                assert resource.state == "APPLIED"

    def test_list_resources(self, pg_loan_system):
        sys = pg_loan_system
        for _ in range(3):
            sys.create({"amount": 100}, actor="u1")

        resources = sys.list_resources()
        assert len(resources) == 3
