"""Tests for reactive controllers."""

from reconcile import define_system


def test_reactive_controller():
    """Controller that auto-transitions on creation."""
    def auto_uw(resource):
        if resource.state == "APPLIED":
            return {"transition": "UNDERWRITING"}
        return None

    sys = define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "DONE"],
        transitions=[("APPLIED", "UNDERWRITING"), ("UNDERWRITING", "DONE")],
        terminal_states=["DONE"],
        controllers=[{
            "name": "auto-uw",
            "reconcile": auto_uw,
            "on_events": ["loan.created"],
            "priority": 50,
        }],
    )

    result = sys.create({"amount": 100}, actor="u1")
    assert result.success

    # The controller should have auto-transitioned to UNDERWRITING
    resource = sys.get(result.resource.id)
    assert resource.state == "UNDERWRITING"


def test_controller_noop():
    """Controller that returns None does nothing."""
    def noop_ctrl(resource):
        return None

    sys = define_system(
        name="loan",
        states=["APPLIED", "DONE"],
        transitions=[("APPLIED", "DONE")],
        terminal_states=["DONE"],
        controllers=[{
            "name": "noop",
            "reconcile": noop_ctrl,
            "on_events": ["loan.created"],
        }],
    )

    result = sys.create({}, actor="u1")
    assert result.success
    resource = sys.get(result.resource.id)
    assert resource.state == "APPLIED"  # No auto-transition


def test_controller_event_filtering():
    """Controller only fires on matching events."""
    calls = []

    def tracking_ctrl(resource):
        calls.append(resource.state)
        return None

    sys = define_system(
        name="loan",
        states=["APPLIED", "UW", "DONE"],
        transitions=[("APPLIED", "UW"), ("UW", "DONE")],
        terminal_states=["DONE"],
        controllers=[{
            "name": "tracker",
            "reconcile": tracking_ctrl,
            "on_events": ["loan.transitioned"],  # Only on transitions, not creation
        }],
    )

    result = sys.create({}, actor="u1")
    assert len(calls) == 0  # Not called on creation

    sys.transition(result.resource.id, "UW", actor="u1", role="x",
                   authority_level="CONTROLLER")
    assert len(calls) == 1  # Called on transition


def test_controller_with_class():
    """Using Controller base class."""
    from reconcile.controller import Controller

    class AutoReject(Controller):
        name = "auto-reject"
        priority = 80
        on_events = ["loan.created"]

        def reconcile(self, resource, ctx=None):
            if resource.data.get("risk", 0) > 0.9:
                return {"transition": "REJECTED"}
            return None

    sys = define_system(
        name="loan",
        states=["APPLIED", "REJECTED"],
        transitions=[("APPLIED", "REJECTED")],
        terminal_states=["REJECTED"],
        controllers=[AutoReject()],
    )

    # High risk -> auto-rejected
    r1 = sys.create({"risk": 0.95}, actor="u1")
    resource1 = sys.get(r1.resource.id)
    assert resource1.state == "REJECTED"

    # Low risk -> stays applied
    r2 = sys.create({"risk": 0.3}, actor="u1")
    resource2 = sys.get(r2.resource.id)
    assert resource2.state == "APPLIED"
