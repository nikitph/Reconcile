"""Additional coverage for the single-system FastAPI adapter."""

import pytest
import pytest_asyncio
from httpx import ASGITransport, AsyncClient

from reconcile import define_system
from reconcile.api import create_app


@pytest.fixture
def loan_app():
    system = define_system(
        name="loan",
        states=["APPLIED", "UNDERWRITING", "APPROVED", "DISBURSED", "REJECTED"],
        transitions=[
            ("APPLIED", "UNDERWRITING"),
            ("UNDERWRITING", "APPROVED"),
            ("UNDERWRITING", "REJECTED"),
            ("APPROVED", "DISBURSED"),
        ],
        terminal_states=["DISBURSED", "REJECTED"],
        roles={
            "officer": ["view", "transition:UNDERWRITING"],
            "manager": ["view", "transition:*"],
            "viewer": ["view"],
        },
    )
    return create_app(system.native)


@pytest_asyncio.fixture
async def client(loan_app):
    transport = ASGITransport(app=loan_app)
    async with AsyncClient(transport=transport, base_url="http://test") as async_client:
        yield async_client


async def _create_loan(client: AsyncClient, **data) -> str:
    response = await client.post("/api/loan", json={"data": data or {"amount": 100}})
    assert response.status_code == 200
    return response.json()["id"]


@pytest.mark.asyncio
async def test_set_desired_endpoint_returns_updated_desired_state(client):
    resource_id = await _create_loan(client, amount=250_000)

    response = await client.post(
        f"/api/loan/{resource_id}/desired",
        json={"desired_state": "DISBURSED", "requested_by": "manager-1"},
    )

    assert response.status_code == 200
    body = response.json()
    assert body["id"] == resource_id
    assert body["state"] == "DISBURSED"
    assert body["desired_state"] == "DISBURSED"


@pytest.mark.asyncio
async def test_set_desired_endpoint_returns_400_for_invalid_state(client):
    resource_id = await _create_loan(client)

    response = await client.post(
        f"/api/loan/{resource_id}/desired",
        json={"desired_state": "NOT_A_REAL_STATE", "requested_by": "manager-1"},
    )

    assert response.status_code == 400
    assert "NOT_A_REAL_STATE" in response.json()["detail"]


@pytest.mark.asyncio
async def test_projection_endpoint_returns_projection_json(client):
    resource_id = await _create_loan(client, amount=1_000_000)

    response = await client.get(f"/api/interface/loan/{resource_id}", params={"role": "officer"})

    assert response.status_code == 200
    body = response.json()
    assert body["resource"]["id"] == resource_id
    assert body["resource"]["state"] == "APPLIED"
    assert any(action["action"] == "UNDERWRITING" for action in body["valid_actions"])


@pytest.mark.asyncio
async def test_projection_endpoint_returns_400_when_projection_fails(client):
    response = await client.get(
        "/api/interface/loan/00000000-0000-0000-0000-000000000000",
        params={"role": "officer"},
    )

    assert response.status_code == 400
    assert "not found" in response.json()["detail"].lower()


@pytest.mark.asyncio
async def test_projection_list_endpoint_returns_all_resources(client):
    first_id = await _create_loan(client, amount=100)
    second_id = await _create_loan(client, amount=200)

    response = await client.get("/api/interface/loan", params={"role": "viewer"})

    assert response.status_code == 200
    ids = {item["resource"]["id"] for item in response.json()}
    assert {first_id, second_id}.issubset(ids)


@pytest.mark.asyncio
async def test_execute_interface_action_returns_projection(client):
    resource_id = await _create_loan(client, amount=300_000)

    response = await client.post(
        f"/api/interface/loan/{resource_id}/action",
        json={"action": "UNDERWRITING", "actor": "officer-1", "role": "officer"},
    )

    assert response.status_code == 200
    body = response.json()
    assert body["success"] is True
    assert body["projection"]["resource"]["state"] == "UNDERWRITING"


@pytest.mark.asyncio
async def test_execute_interface_action_returns_400_for_blocked_action(client):
    resource_id = await _create_loan(client, amount=300_000)

    response = await client.post(
        f"/api/interface/loan/{resource_id}/action",
        json={"action": "UNDERWRITING", "actor": "viewer-1", "role": "viewer"},
    )

    assert response.status_code == 400
    detail = response.json()["detail"]
    assert detail["step"] == "check_role_permissions"
    assert "permission" in detail["reason"].lower() or "role" in detail["reason"].lower()


@pytest.mark.asyncio
async def test_spec_endpoint_exposes_registered_role_and_type(client):
    response = await client.get("/api/spec")

    assert response.status_code == 200
    spec = response.json()
    assert any(item["name"] == "loan" for item in spec["types"])
    assert spec["version"] == "0.1.0"
