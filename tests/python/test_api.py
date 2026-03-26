"""FastAPI REST API integration tests."""

import pytest
import pytest_asyncio
from httpx import AsyncClient, ASGITransport
from reconcile._native import ReconcileSystem
from reconcile.api import create_app


@pytest.fixture
def app():
    sys = ReconcileSystem()
    sys.register_type("loan", ["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED"],
                      [("APPLIED", "UNDERWRITING"), ("UNDERWRITING", "APPROVED"),
                       ("APPROVED", "DISBURSED")],
                      "APPLIED", ["DISBURSED"])
    sys.register_type("applicant", ["ACTIVE"], [], "ACTIVE", ["ACTIVE"])
    sys.register_relationship("loan", "applicant", "belongs_to", "many_to_one", True, "applicant_id")
    sys.register_role("manager", ["transition:*"])
    return create_app(sys)


@pytest_asyncio.fixture
async def client(app):
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c


@pytest.mark.asyncio
async def test_create_resource(client):
    resp = await client.post("/api/loan", json={"data": {"amount": 100_000}, "actor": "u1"})
    assert resp.status_code == 200
    body = resp.json()
    assert body["state"] == "APPLIED"
    assert body["version"] == 1
    assert "id" in body


@pytest.mark.asyncio
async def test_get_resource(client):
    resp = await client.post("/api/loan", json={"data": {"amount": 50_000}})
    rid = resp.json()["id"]

    resp = await client.get(f"/api/loan/{rid}")
    assert resp.status_code == 200
    assert resp.json()["state"] == "APPLIED"
    assert resp.json()["data"]["amount"] == 50_000


@pytest.mark.asyncio
async def test_get_nonexistent(client):
    resp = await client.get("/api/loan/00000000-0000-0000-0000-000000000000")
    assert resp.status_code == 404


@pytest.mark.asyncio
async def test_transition(client):
    resp = await client.post("/api/loan", json={"data": {"amount": 100_000}})
    rid = resp.json()["id"]

    resp = await client.post(f"/api/loan/{rid}/transition", json={
        "to_state": "UNDERWRITING", "actor": "u1", "role": "manager",
    })
    assert resp.status_code == 200
    assert resp.json()["state"] == "UNDERWRITING"


@pytest.mark.asyncio
async def test_invalid_transition(client):
    resp = await client.post("/api/loan", json={"data": {}})
    rid = resp.json()["id"]

    resp = await client.post(f"/api/loan/{rid}/transition", json={
        "to_state": "DISBURSED", "actor": "u1", "role": "manager",
    })
    assert resp.status_code == 400
    assert "validate_state_machine" in resp.json()["detail"]["step"]


@pytest.mark.asyncio
async def test_full_lifecycle(client):
    resp = await client.post("/api/loan", json={"data": {"amount": 100}})
    rid = resp.json()["id"]

    for state in ["UNDERWRITING", "APPROVED", "DISBURSED"]:
        resp = await client.post(f"/api/loan/{rid}/transition", json={
            "to_state": state, "actor": "u1", "role": "manager",
        })
        assert resp.status_code == 200

    resp = await client.get(f"/api/loan/{rid}")
    assert resp.json()["state"] == "DISBURSED"
    assert resp.json()["version"] == 4


@pytest.mark.asyncio
async def test_events_endpoint(client):
    resp = await client.post("/api/loan", json={"data": {}})
    rid = resp.json()["id"]
    await client.post(f"/api/loan/{rid}/transition", json={
        "to_state": "UNDERWRITING", "actor": "u1", "role": "manager",
    })

    resp = await client.get(f"/api/loan/{rid}/events")
    assert resp.status_code == 200
    events = resp.json()
    assert len(events) >= 2
    assert events[0]["event_type"] == "loan.created"


@pytest.mark.asyncio
async def test_audit_endpoint(client):
    resp = await client.post("/api/loan", json={"data": {}})
    rid = resp.json()["id"]
    await client.post(f"/api/loan/{rid}/transition", json={
        "to_state": "UNDERWRITING", "actor": "alice", "role": "manager",
    })

    resp = await client.get(f"/api/loan/{rid}/audit")
    assert resp.status_code == 200
    audit = resp.json()
    assert len(audit) == 1
    assert audit[0]["actor"] == "alice"
    assert audit[0]["new_state"] == "UNDERWRITING"


@pytest.mark.asyncio
async def test_list_resources(client):
    for _ in range(3):
        await client.post("/api/loan", json={"data": {"amount": 100}})

    resp = await client.get("/api/loan")
    assert resp.status_code == 200
    assert len(resp.json()) == 3


@pytest.mark.asyncio
async def test_graph_neighbors(client):
    # Create applicant
    resp = await client.post("/api/applicant", json={"data": {"name": "Acme"}, "authority_level": "SYSTEM"})
    app_id = resp.json()["id"]

    # Create loan referencing applicant
    resp = await client.post("/api/loan", json={
        "data": {"amount": 500_000, "applicant_id": app_id},
        "authority_level": "SYSTEM",
    })
    assert resp.status_code == 200

    # Query graph neighbors
    resp = await client.get(f"/api/graph/{app_id}/neighbors")
    assert resp.status_code == 200
    assert len(resp.json()) == 1
    assert resp.json()[0]["resource_type"] == "loan"


@pytest.mark.asyncio
async def test_graph_aggregate(client):
    resp = await client.post("/api/applicant", json={"data": {"name": "Acme"}, "authority_level": "SYSTEM"})
    app_id = resp.json()["id"]

    for amount in [100_000, 200_000, 300_000]:
        await client.post("/api/loan", json={
            "data": {"amount": amount, "applicant_id": app_id},
            "authority_level": "SYSTEM",
        })

    resp = await client.get(f"/api/graph/{app_id}/aggregate",
                            params={"edge_type": "belongs_to", "field": "amount", "fn": "SUM"})
    assert resp.status_code == 200
    assert resp.json()["value"] == 600_000.0


@pytest.mark.asyncio
async def test_graph_degree(client):
    resp = await client.post("/api/applicant", json={"data": {"name": "Acme"}, "authority_level": "SYSTEM"})
    app_id = resp.json()["id"]

    await client.post("/api/loan", json={
        "data": {"amount": 100, "applicant_id": app_id}, "authority_level": "SYSTEM",
    })

    resp = await client.get(f"/api/graph/{app_id}/degree")
    assert resp.status_code == 200
    assert resp.json()["degree"] == 1
