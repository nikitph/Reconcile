"""WebSocket real-time projection subscription tests.

Tests that clients subscribed to a resource receive updated
InterfaceProjections when the resource transitions.
"""

import pytest
import pytest_asyncio
from httpx import AsyncClient, ASGITransport
from reconcile._native import ReconcileSystem
from reconcile.api import create_app


@pytest.fixture
def system_and_app():
    sys = ReconcileSystem()
    sys.register_type("loan", ["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED"],
                      [("APPLIED", "UNDERWRITING"), ("UNDERWRITING", "APPROVED"),
                       ("APPROVED", "DISBURSED")],
                      "APPLIED", ["DISBURSED"])
    sys.register_role("officer", ["view", "transition:*"])
    app = create_app(sys)
    return sys, app


@pytest_asyncio.fixture
async def client(system_and_app):
    _, app = system_and_app
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c


@pytest.mark.asyncio
async def test_websocket_initial_projection(system_and_app):
    """WebSocket sends initial projection on connect."""
    sys, app = system_and_app

    # Create a resource first
    r = sys.create("loan", {"amount": 100_000}, "sys", "SYSTEM")
    rid = r.resource.id

    from starlette.testclient import TestClient
    with TestClient(app) as tc:
        with tc.websocket_connect(f"/ws/interface/loan/{rid}") as ws:
            # Send role
            ws.send_json({"role": "officer"})

            # Receive initial projection
            data = ws.receive_json()
            assert data["resource"]["state"] == "APPLIED"
            assert data["resource"]["id"] == rid
            assert len(data["valid_actions"]) > 0


@pytest.mark.asyncio
async def test_websocket_receives_update_on_transition(system_and_app):
    """WebSocket pushes new projection when resource transitions via REST."""
    sys, app = system_and_app

    r = sys.create("loan", {"amount": 100_000}, "sys", "SYSTEM")
    rid = r.resource.id

    from starlette.testclient import TestClient
    with TestClient(app) as tc:
        with tc.websocket_connect(f"/ws/interface/loan/{rid}") as ws:
            ws.send_json({"role": "officer"})

            # Consume initial projection
            initial = ws.receive_json()
            assert initial["resource"]["state"] == "APPLIED"

            # Transition via REST
            resp = tc.post(f"/api/loan/{rid}/transition", json={
                "to_state": "UNDERWRITING", "actor": "u1", "role": "officer",
            })
            assert resp.status_code == 200

            # WebSocket should receive updated projection
            updated = ws.receive_json()
            assert updated["resource"]["state"] == "UNDERWRITING"


@pytest.mark.asyncio
async def test_websocket_ping_pong(system_and_app):
    """WebSocket responds to ping with pong."""
    sys, app = system_and_app

    r = sys.create("loan", {"amount": 100}, "sys", "SYSTEM")
    rid = r.resource.id

    from starlette.testclient import TestClient
    with TestClient(app) as tc:
        with tc.websocket_connect(f"/ws/interface/loan/{rid}") as ws:
            ws.send_json({"role": "officer"})
            ws.receive_json()  # initial projection

            ws.send_text("ping")
            pong = ws.receive_text()
            assert pong == "pong"
