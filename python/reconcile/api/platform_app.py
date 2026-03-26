"""Platform-aware FastAPI application.

Routes are scoped by app_id: /api/{app_id}/{resource_type}/{id}
Each app is a fully isolated Kernel instance.
"""

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import Any
from reconcile.platform import ReconcilePlatform, AppNotFoundError


class CreateRequest(BaseModel):
    data: dict[str, Any] = {}
    actor: str = "system"
    authority_level: str = "HUMAN"


class TransitionRequest(BaseModel):
    to_state: str
    actor: str = "system"
    role: str = "system"
    authority_level: str = "HUMAN"


class ActionRequest(BaseModel):
    action: str
    actor: str
    role: str
    authority_level: str = "INTERFACE"


def create_platform_app(platform: ReconcilePlatform) -> FastAPI:
    """Create a FastAPI app that routes to apps by app_id."""
    app = FastAPI(title="Reconcile Platform", version="0.1.0")

    def get_system(app_id: str):
        try:
            return platform.get_app(app_id).system
        except AppNotFoundError:
            raise HTTPException(404, detail=f"App '{app_id}' not found")

    # --- Platform management ---

    @app.get("/api/apps")
    def list_apps():
        return {"apps": platform.list_apps()}

    @app.get("/api/{app_id}/spec")
    def get_spec(app_id: str):
        return get_system(app_id).export_spec()

    # --- Resource CRUD (scoped by app_id) ---

    @app.post("/api/{app_id}/{resource_type}")
    def create_resource(app_id: str, resource_type: str, req: CreateRequest):
        sys = get_system(app_id)
        result = sys.create(resource_type, req.data, req.actor, req.authority_level)
        if not result.success:
            raise HTTPException(400, detail={
                "step": result.rejected_step, "reason": result.rejected_reason,
            })
        r = result.resource
        return {"id": r.id, "state": r.state, "version": r.version, "data": r.data}

    @app.get("/api/{app_id}/{resource_type}/{resource_id}")
    def get_resource(app_id: str, resource_type: str, resource_id: str):
        r = get_system(app_id).get(resource_id)
        if r is None:
            raise HTTPException(404, detail="Resource not found")
        return {"id": r.id, "state": r.state, "version": r.version,
                "data": r.data, "desired_state": r.desired_state}

    @app.get("/api/{app_id}/{resource_type}")
    def list_resources(app_id: str, resource_type: str):
        resources = get_system(app_id).list_resources(resource_type)
        return [{"id": r.id, "state": r.state, "version": r.version} for r in resources]

    @app.post("/api/{app_id}/{resource_type}/{resource_id}/transition")
    def transition(app_id: str, resource_type: str, resource_id: str, req: TransitionRequest):
        result = get_system(app_id).transition(
            resource_id, req.to_state, req.actor, req.role, req.authority_level,
        )
        if not result.success:
            raise HTTPException(400, detail={
                "step": result.rejected_step, "reason": result.rejected_reason,
            })
        r = result.resource
        return {"id": r.id, "state": r.state, "version": r.version}

    # --- Events + Audit ---

    @app.get("/api/{app_id}/{resource_type}/{resource_id}/events")
    def get_events(app_id: str, resource_type: str, resource_id: str):
        events = get_system(app_id).events(resource_id)
        return [{"id": e.id, "event_type": e.event_type, "actor": e.actor,
                 "authority_level": e.authority_level, "payload": e.payload} for e in events]

    @app.get("/api/{app_id}/{resource_type}/{resource_id}/audit")
    def get_audit(app_id: str, resource_type: str, resource_id: str):
        audit = get_system(app_id).audit(resource_id)
        return [{"actor": a.actor, "role": a.role, "authority_level": a.authority_level,
                 "previous_state": a.previous_state, "new_state": a.new_state} for a in audit]

    # --- Interface Projection ---

    @app.get("/api/{app_id}/interface/{resource_type}/{resource_id}")
    def get_projection(app_id: str, resource_type: str, resource_id: str, role: str):
        try:
            projection = get_system(app_id).project(resource_id, role)
            return projection.to_json()
        except Exception as e:
            raise HTTPException(400, detail=str(e))

    @app.post("/api/{app_id}/interface/{resource_type}/{resource_id}/action")
    def execute_action(app_id: str, resource_type: str, resource_id: str, req: ActionRequest):
        result, projection = get_system(app_id).execute_action(
            resource_id, req.action, req.actor, req.role, req.authority_level,
        )
        if not result.success:
            raise HTTPException(400, detail={
                "step": result.rejected_step, "reason": result.rejected_reason,
            })
        return {
            "success": True,
            "projection": projection.to_json() if projection else None,
        }

    return app
