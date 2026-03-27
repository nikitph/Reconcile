"""FastAPI application for Reconcile."""

from fastapi import FastAPI, HTTPException, WebSocket, WebSocketDisconnect
from pydantic import BaseModel
from typing import Any
from reconcile._native import ReconcileSystem
from reconcile.subscriptions import SubscriptionManager


class TransitionRequest(BaseModel):
    to_state: str
    actor: str = "system"
    role: str = "system"
    authority_level: str = "HUMAN"


class CreateRequest(BaseModel):
    data: dict[str, Any] = {}
    actor: str = "system"
    authority_level: str = "HUMAN"


class DesiredStateRequest(BaseModel):
    desired_state: str
    requested_by: str = "system"
    authority_level: str = "HUMAN"


def create_app(system: ReconcileSystem) -> FastAPI:
    """Create a FastAPI app wrapping a ReconcileSystem."""
    app = FastAPI(title="Reconcile API", version="0.1.0")
    subs = SubscriptionManager()

    async def _notify(resource_id: str):
        """Push updated projections to all subscribers of this resource."""
        await subs.broadcast(resource_id, system)

    @app.get("/api/spec")
    def get_spec():
        return system.export_spec()

    # --- Interface Projection ---

    @app.get("/api/interface/{resource_type}/{resource_id}")
    def get_projection(resource_type: str, resource_id: str, role: str):
        try:
            projection = system.project(resource_id, role)
            return projection.to_json()
        except Exception as e:
            raise HTTPException(400, detail=str(e))

    @app.get("/api/interface/{resource_type}")
    def get_projections(resource_type: str, role: str):
        projections = system.project_list(resource_type, role)
        return [p.to_json() for p in projections]

    class ActionRequest(BaseModel):
        action: str
        actor: str
        role: str
        authority_level: str = "INTERFACE"

    @app.post("/api/interface/{resource_type}/{resource_id}/action")
    async def execute_interface_action(resource_type: str, resource_id: str, req: ActionRequest):
        result, projection = system.execute_action(
            resource_id, req.action, req.actor, req.role, req.authority_level,
        )
        if not result.success:
            raise HTTPException(400, detail={
                "step": result.rejected_step,
                "reason": result.rejected_reason,
            })
        await _notify(resource_id)
        return {
            "success": True,
            "projection": projection.to_json() if projection else None,
        }

    # --- Resource CRUD ---

    @app.post("/api/{resource_type}")
    async def create_resource(resource_type: str, req: CreateRequest):
        result = system.create(resource_type, req.data, req.actor, req.authority_level)
        if not result.success:
            raise HTTPException(400, detail={
                "step": result.rejected_step,
                "reason": result.rejected_reason,
            })
        r = result.resource
        await _notify(r.id)
        return {"id": r.id, "state": r.state, "version": r.version, "data": r.data}

    @app.get("/api/{resource_type}/{resource_id}")
    def get_resource(resource_type: str, resource_id: str):
        r = system.get(resource_id)
        if r is None:
            raise HTTPException(404, detail="Resource not found")
        return {"id": r.id, "state": r.state, "version": r.version,
                "data": r.data, "desired_state": r.desired_state}

    @app.get("/api/{resource_type}")
    def list_resources(resource_type: str):
        resources = system.list_resources(resource_type)
        return [{"id": r.id, "state": r.state, "version": r.version} for r in resources]

    @app.post("/api/{resource_type}/{resource_id}/transition")
    async def transition(resource_type: str, resource_id: str, req: TransitionRequest):
        result = system.transition(resource_id, req.to_state, req.actor, req.role, req.authority_level)
        if not result.success:
            raise HTTPException(400, detail={
                "step": result.rejected_step,
                "reason": result.rejected_reason,
            })
        r = result.resource
        await _notify(resource_id)
        return {"id": r.id, "state": r.state, "version": r.version}

    @app.post("/api/{resource_type}/{resource_id}/desired")
    async def set_desired(resource_type: str, resource_id: str, req: DesiredStateRequest):
        try:
            system.set_desired(resource_id, req.desired_state, req.requested_by, req.authority_level)
        except Exception as e:
            raise HTTPException(400, detail=str(e))
        r = system.get(resource_id)
        await _notify(resource_id)
        return {"id": r.id, "state": r.state, "desired_state": r.desired_state}

    # --- Events + Audit ---

    @app.get("/api/{resource_type}/{resource_id}/events")
    def get_events(resource_type: str, resource_id: str):
        events = system.events(resource_id)
        return [{"id": e.id, "offset": e.offset, "event_type": e.event_type,
                 "actor": e.actor, "authority_level": e.authority_level,
                 "payload": e.payload} for e in events]

    @app.get("/api/{resource_type}/{resource_id}/audit")
    def get_audit(resource_type: str, resource_id: str):
        audit = system.audit(resource_id)
        return [{"id": a.id, "actor": a.actor, "role": a.role,
                 "authority_level": a.authority_level,
                 "previous_state": a.previous_state,
                 "new_state": a.new_state} for a in audit]

    # --- Graph queries ---

    @app.get("/api/graph/{resource_id}/neighbors")
    def graph_neighbors(resource_id: str, edge_type: str | None = None):
        neighbors = system.graph_neighbors(resource_id, edge_type)
        return [{"id": n.id, "resource_type": n.resource_type,
                 "state": n.state, "data": n.data} for n in neighbors]

    @app.get("/api/graph/{resource_id}/aggregate")
    def graph_aggregate(resource_id: str, edge_type: str, field: str, fn: str = "SUM"):
        result = system.graph_aggregate(resource_id, edge_type, field, fn)
        return {"value": result}

    @app.get("/api/graph/{resource_id}/degree")
    def graph_degree(resource_id: str, edge_type: str | None = None):
        return {"degree": system.graph_degree(resource_id, edge_type)}

    # --- WebSocket: real-time projection subscriptions ---

    @app.websocket("/ws/interface/{resource_type}/{resource_id}")
    async def ws_projection(websocket: WebSocket, resource_type: str, resource_id: str):
        """Subscribe to real-time projection updates for a resource.

        Client sends: {"role": "officer"} on connect.
        Server pushes: InterfaceProjection JSON on every state change.
        """
        await websocket.accept()

        try:
            # First message must specify role
            init = await websocket.receive_json()
            role = init.get("role", "viewer")

            await subs.subscribe(websocket, resource_id, role)

            # Send initial projection immediately
            try:
                projection = system.project(resource_id, role)
                await websocket.send_json(projection.to_json())
            except Exception as e:
                await websocket.send_json({"error": str(e)})

            # Keep connection alive, listen for client messages
            while True:
                msg = await websocket.receive_text()
                # Client can send "ping" to keep alive
                if msg == "ping":
                    await websocket.send_text("pong")

        except WebSocketDisconnect:
            pass
        finally:
            await subs.unsubscribe(websocket)

    return app
