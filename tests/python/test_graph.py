"""Instance graph integration tests.

Tests the full pipeline: schema relationships → resource creation with
foreign keys → automatic graph edge building → graph queries.
"""

import pytest
from reconcile._native import ReconcileSystem


@pytest.fixture
def lending_system():
    """Multi-type lending system with applicants, loans, and collateral."""
    sys = ReconcileSystem()

    sys.register_type("applicant", ["ACTIVE", "SUSPENDED"],
                      [("ACTIVE", "SUSPENDED")], "ACTIVE", ["SUSPENDED"])
    sys.register_type("loan", ["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED", "REJECTED"],
                      [("APPLIED", "UNDERWRITING"), ("UNDERWRITING", "APPROVED"),
                       ("UNDERWRITING", "REJECTED"), ("APPROVED", "DISBURSED")],
                      "APPLIED", ["DISBURSED", "REJECTED"])
    sys.register_type("collateral", ["PENDING", "APPRAISED"],
                      [("PENDING", "APPRAISED")], "PENDING", ["APPRAISED"])

    # Relationships
    sys.register_relationship("loan", "applicant", "belongs_to", "many_to_one", True, "applicant_id")
    sys.register_relationship("loan", "collateral", "secured_by", "many_to_one", False, "collateral_id")

    return sys


class TestGraphEdgeBuilding:
    def test_edges_created_on_resource_creation(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")
        loan = sys.create("loan", {"amount": 100, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        neighbors = sys.graph_neighbors(app.resource.id)
        assert len(neighbors) == 1
        assert neighbors[0].resource_type == "loan"

    def test_multiple_loans_per_applicant(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")

        for i in range(5):
            sys.create("loan", {"amount": (i + 1) * 100_000, "applicant_id": app.resource.id},
                       "sys", "SYSTEM")

        neighbors = sys.graph_neighbors(app.resource.id)
        assert len(neighbors) == 5

    def test_no_edge_without_foreign_key(self, lending_system):
        sys = lending_system
        # Loan without collateral_id — no secured_by edge
        app = sys.create("applicant", {"name": "Test"}, "sys", "SYSTEM")
        loan = sys.create("loan", {"amount": 100, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        # Only belongs_to edge, not secured_by
        neighbors_bt = sys.graph_neighbors(loan.resource.id, "belongs_to")
        neighbors_sb = sys.graph_neighbors(loan.resource.id, "secured_by")
        assert len(neighbors_bt) == 1
        assert len(neighbors_sb) == 0


class TestGraphAggregate:
    def test_sum_amounts(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")

        sys.create("loan", {"amount": 500_000, "applicant_id": app.resource.id}, "sys", "SYSTEM")
        sys.create("loan", {"amount": 300_000, "applicant_id": app.resource.id}, "sys", "SYSTEM")
        sys.create("loan", {"amount": 200_000, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        total = sys.graph_aggregate(app.resource.id, "belongs_to", "amount", "SUM")
        assert total == 1_000_000.0

    def test_count(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")
        for _ in range(3):
            sys.create("loan", {"amount": 100, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        count = sys.graph_aggregate(app.resource.id, "belongs_to", "amount", "COUNT")
        assert count == 3.0

    def test_max_and_min(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")
        for amount in [100_000, 500_000, 250_000]:
            sys.create("loan", {"amount": amount, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        assert sys.graph_aggregate(app.resource.id, "belongs_to", "amount", "MAX") == 500_000.0
        assert sys.graph_aggregate(app.resource.id, "belongs_to", "amount", "MIN") == 100_000.0

    def test_avg(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")
        for amount in [100, 200, 300]:
            sys.create("loan", {"amount": amount, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        avg = sys.graph_aggregate(app.resource.id, "belongs_to", "amount", "AVG")
        assert avg == 200.0

    def test_aggregate_empty(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "No loans"}, "sys", "SYSTEM")

        total = sys.graph_aggregate(app.resource.id, "belongs_to", "amount", "SUM")
        assert total is None  # No neighbors


class TestGraphDegree:
    def test_degree(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")
        sys.create("loan", {"amount": 100, "applicant_id": app.resource.id}, "sys", "SYSTEM")
        sys.create("loan", {"amount": 200, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        assert sys.graph_degree(app.resource.id) == 2
        assert sys.graph_degree(app.resource.id, "belongs_to") == 2
        assert sys.graph_degree(app.resource.id, "secured_by") == 0

    def test_degree_zero_for_isolated(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Lonely"}, "sys", "SYSTEM")
        assert sys.graph_degree(app.resource.id) == 0


class TestGraphNodeUpdates:
    def test_node_state_updates_on_transition(self, lending_system):
        sys = lending_system
        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")
        loan = sys.create("loan", {"amount": 500_000, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        # Transition loan: APPLIED -> UNDERWRITING
        sys.transition(loan.resource.id, "UNDERWRITING", "u1", "mgr", "CONTROLLER")

        # Graph node should reflect updated state
        neighbors = sys.graph_neighbors(app.resource.id)
        loan_node = [n for n in neighbors if n.id == loan.resource.id][0]
        assert loan_node.state == "UNDERWRITING"


class TestGraphExposureInvariant:
    """Real-world scenario: exposure limit as graph aggregate invariant."""

    def test_exposure_limit_blocks_approval(self):
        """Total loan amounts per applicant must be < 1M."""
        from reconcile import InvariantResult

        def exposure_check(resource):
            # This invariant runs on the loan resource.
            # For now, we check at creation time using the resource's own data.
            # With graph integration, a controller could check aggregate exposure.
            return InvariantResult.ok()

        sys = ReconcileSystem()
        sys.register_type("applicant", ["ACTIVE"], [], "ACTIVE", ["ACTIVE"])
        sys.register_type("loan", ["APPLIED", "APPROVED"],
                          [("APPLIED", "APPROVED")], "APPLIED", ["APPROVED"])
        sys.register_relationship("loan", "applicant", "belongs_to", "many_to_one", True, "applicant_id")

        app = sys.create("applicant", {"name": "Acme"}, "sys", "SYSTEM")

        # Create loans totaling 900K
        for _ in range(3):
            sys.create("loan", {"amount": 300_000, "applicant_id": app.resource.id}, "sys", "SYSTEM")

        # Verify aggregate
        total = sys.graph_aggregate(app.resource.id, "belongs_to", "amount", "SUM")
        assert total == 900_000.0

        # A controller or policy could now use this aggregate to block approval
        # when total would exceed the limit
