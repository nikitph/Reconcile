"""Reconcile Platform — multi-app isolation on a single instance.

Each app gets its own Kernel (own types, roles, policies, state).
The Platform manages app lifecycle and routes requests to the correct Kernel.

Usage:
    platform = ReconcilePlatform()

    # Register apps
    platform.register_app("bank_a", define_system(name="loan", ...))
    platform.register_app("bank_b", define_system(name="loan", ...))

    # Route operations to the correct app
    platform.create("bank_a", "loan", data, actor="u1")
    platform.project("bank_a", resource_id, role="officer")

    # Apps are completely isolated — bank_a can't see bank_b
"""

from typing import Any
from reconcile._native import ReconcileSystem
from reconcile.dsl import SystemWrapper
from reconcile.llm import GovernedLLM


class AppNotFoundError(Exception):
    pass


class ReconcilePlatform:
    """Multi-app platform. Each app is a fully isolated Kernel instance."""

    def __init__(self):
        self._apps: dict[str, AppInstance] = {}

    def register_app(self, app_id: str, system: SystemWrapper | ReconcileSystem) -> "AppInstance":
        """Register a new app with its system definition."""
        if app_id in self._apps:
            raise ValueError(f"App '{app_id}' already registered")

        native = system.native if isinstance(system, SystemWrapper) else system
        resource_type = system._resource_type if isinstance(system, SystemWrapper) else None

        instance = AppInstance(
            app_id=app_id,
            system=native,
            resource_type=resource_type,
        )
        self._apps[app_id] = instance
        return instance

    def get_app(self, app_id: str) -> "AppInstance":
        """Get an app instance by ID."""
        if app_id not in self._apps:
            raise AppNotFoundError(f"App '{app_id}' not found")
        return self._apps[app_id]

    def list_apps(self) -> list[str]:
        """List all registered app IDs."""
        return list(self._apps.keys())

    def remove_app(self, app_id: str):
        """Remove an app. All data in that app's kernel is lost."""
        if app_id not in self._apps:
            raise AppNotFoundError(f"App '{app_id}' not found")
        del self._apps[app_id]

    # --- Convenience methods that route to the correct app ---

    def create(self, app_id: str, resource_type: str, data: dict, *,
               actor: str = "system", authority_level: str = "HUMAN"):
        return self.get_app(app_id).system.create(
            resource_type, data, actor, authority_level,
        )

    def transition(self, app_id: str, resource_id: str, to_state: str, *,
                   actor: str = "system", role: str = "system",
                   authority_level: str = "HUMAN"):
        return self.get_app(app_id).system.transition(
            resource_id, to_state, actor, role, authority_level,
        )

    def get(self, app_id: str, resource_id: str):
        return self.get_app(app_id).system.get(resource_id)

    def project(self, app_id: str, resource_id: str, role: str):
        return self.get_app(app_id).system.project(resource_id, role)

    def project_list(self, app_id: str, resource_type: str, role: str):
        return self.get_app(app_id).system.project_list(resource_type, role)

    def export_spec(self, app_id: str):
        return self.get_app(app_id).system.export_spec()

    def governed_llm(self, app_id: str, actor: str, role: str,
                     provider=None) -> GovernedLLM:
        """Create a GovernedLLM bound to a specific app."""
        return GovernedLLM(
            self.get_app(app_id).system,
            actor=actor, role=role, provider=provider,
        )


class AppInstance:
    """A single app running on the platform."""

    def __init__(self, app_id: str, system: ReconcileSystem,
                 resource_type: str | None = None):
        self.app_id = app_id
        self.system = system
        self.resource_type = resource_type

    def __repr__(self):
        return f"AppInstance(app_id={self.app_id!r})"
