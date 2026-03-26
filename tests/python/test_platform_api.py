"""HTTP coverage for the platform-aware FastAPI adapter."""

import pytest
import pytest_asyncio
from httpx import ASGITransport, AsyncClient

from reconcile import ReconcilePlatform, define_system
from reconcile.api.platform_app import create_platform_app


@pytest.fixture
def platform():
    platform = ReconcilePlatform()
    bank = define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED"],
        transitions=[
            ("APPLIED", "UNDERWRITING"),
            ("UNDERWRITING", "APPROVED"),
            ("APPROVED", "DISBURSED"),
        ],
        terminal_states=["DISBURSED"],
        roles={
            "manager": ["view", "transition:*"],
            "viewer": ["view"],
        },
    )
    insurance = define_system(
        name="claim",
        states=["FILED", "INVESTIGATING", "APPROVED", "PAID"],
        transitions=[
            ("FILED", "INVESTIGATING"),
            ("INVESTIGATING", "APPROVED"),
            ("APPROVED", "PAID"),
        ],
        terminal_states=["PAID"],
        roles={"adjuster": ["view", "transition:*"]},
    )
    platform.register_app("bank_a", bank)
    platform.register_app("insurance_b", insurance)
    return platform


@pytest_asyncio.fixture
async def client(platform):
    transport = ASGITransport(app=create_platform_app(platform))
    async with AsyncClient(transport=transport, base_url="http://test") as async_client:
        yield async_client


async def _create_bank_loan(client: AsyncClient, **data) -> str:
    response = await client.post("/api/bank_a/loan", json={"data": data or {"amount": 100}})
    assert response.status_code == 200
    return response.json()["id"]


@pytest.mark.asyncio
async def test_list_apps_endpoint_returns_registered_app_ids(client):
    response = await client.get("/api/apps")

    assert response.status_code == 200
    assert set(response.json()["apps"]) == {"bank_a", "insurance_b"}


@pytest.mark.asyncio
async def test_platform_create_and_get_resource(client):
    resource_id = await _create_bank_loan(client, amount=750_000)

    response = await client.get(f"/api/bank_a/loan/{resource_id}")

    assert response.status_code == 200
    body = response.json()
    assert body["id"] == resource_id
    assert body["state"] == "APPLIED"
    assert body["data"]["amount"] == 750_000


@pytest.mark.asyncio
async def test_platform_transition_and_audit_endpoints(client):
    resource_id = await _create_bank_loan(client)

    transition = await client.post(
        f"/api/bank_a/loan/{resource_id}/transition",
        json={"to_state": "UNDERWRITING", "actor": "alice", "role": "manager"},
    )
    assert transition.status_code == 200
    assert transition.json()["state"] == "UNDERWRITING"

    audit = await client.get(f"/api/bank_a/loan/{resource_id}/audit")
    assert audit.status_code == 200
    entries = audit.json()
    assert len(entries) == 1
    assert entries[0]["actor"] == "alice"
    assert entries[0]["new_state"] == "UNDERWRITING"


@pytest.mark.asyncio
async def test_platform_events_endpoint_returns_transition_history(client):
    resource_id = await _create_bank_loan(client)
    await client.post(
        f"/api/bank_a/loan/{resource_id}/transition",
        json={"to_state": "UNDERWRITING", "actor": "alice", "role": "manager"},
    )

    response = await client.get(f"/api/bank_a/loan/{resource_id}/events")

    assert response.status_code == 200
    event_types = [event["event_type"] for event in response.json()]
    assert "loan.created" in event_types
    assert "loan.transitioned" in event_types


@pytest.mark.asyncio
async def test_platform_projection_endpoint_returns_projection(client):
    resource_id = await _create_bank_loan(client)

    response = await client.get(
        f"/api/bank_a/interface/loan/{resource_id}",
        params={"role": "manager"},
    )

    assert response.status_code == 200
    body = response.json()
    assert body["resource"]["id"] == resource_id
    assert any(action["action"] == "UNDERWRITING" for action in body["valid_actions"])


@pytest.mark.asyncio
async def test_platform_execute_action_endpoint_returns_projection(client):
    resource_id = await _create_bank_loan(client)

    response = await client.post(
        f"/api/bank_a/interface/loan/{resource_id}/action",
        json={"action": "UNDERWRITING", "actor": "alice", "role": "manager"},
    )

    assert response.status_code == 200
    body = response.json()
    assert body["success"] is True
    assert body["projection"]["resource"]["state"] == "UNDERWRITING"


@pytest.mark.asyncio
async def test_platform_execute_action_returns_400_for_blocked_action(client):
    resource_id = await _create_bank_loan(client)

    response = await client.post(
        f"/api/bank_a/interface/loan/{resource_id}/action",
        json={"action": "UNDERWRITING", "actor": "viewer-1", "role": "viewer"},
    )

    assert response.status_code == 400
    detail = response.json()["detail"]
    assert detail["step"] == "check_role_permissions"


@pytest.mark.asyncio
async def test_platform_spec_endpoint_is_scoped_per_app(client):
    response = await client.get("/api/insurance_b/spec")

    assert response.status_code == 200
    spec = response.json()
    assert any(item["name"] == "claim" for item in spec["types"])
    assert not any(item["name"] == "loan" for item in spec["types"])


@pytest.mark.asyncio
async def test_platform_returns_404_for_unknown_app(client):
    response = await client.get("/api/missing/spec")

    assert response.status_code == 404
    assert "not found" in response.json()["detail"].lower()
