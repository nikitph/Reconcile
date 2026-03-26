"""Tests for event log."""


def test_creation_event(loan_system):
    result = loan_system.create({"amount": 100}, actor="alice")
    rid = result.resource.id
    events = loan_system.events(rid)
    assert len(events) >= 1
    assert events[0].event_type == "loan.created"
    assert events[0].actor == "alice"


def test_transition_event(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="bob", role="manager")

    events = loan_system.events(rid)
    transition_events = [e for e in events if e.event_type == "loan.transitioned"]
    assert len(transition_events) == 1
    assert transition_events[0].actor == "bob"


def test_events_are_ordered(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")
    loan_system.transition(rid, "APPROVED", actor="u1", role="manager")

    events = loan_system.events(rid)
    offsets = [e.offset for e in events]
    assert offsets == sorted(offsets), "Events should be in offset order"


def test_event_payload(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="u1", role="manager")

    events = loan_system.events(rid)
    transition_event = [e for e in events if e.event_type == "loan.transitioned"][0]
    payload = transition_event.payload
    assert payload["from"] == "APPLIED"
    assert payload["to"] == "UNDERWRITING"


def test_events_per_resource_isolation(loan_system):
    r1 = loan_system.create({}, actor="u1")
    r2 = loan_system.create({}, actor="u2")
    loan_system.transition(r1.resource.id, "UNDERWRITING", actor="u1", role="manager")

    events1 = loan_system.events(r1.resource.id)
    events2 = loan_system.events(r2.resource.id)

    assert len(events1) > len(events2)


def test_event_authority_level(loan_system):
    result = loan_system.create({}, actor="u1")
    rid = result.resource.id
    loan_system.transition(rid, "UNDERWRITING", actor="ctrl", role="x",
                           authority_level="CONTROLLER")

    events = loan_system.events(rid)
    transition_event = [e for e in events if e.event_type == "loan.transitioned"][0]
    assert transition_event.authority_level == "CONTROLLER"
