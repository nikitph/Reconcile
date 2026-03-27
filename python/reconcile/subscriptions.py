"""Real-time projection subscriptions.

Manages WebSocket connections that receive InterfaceProjection updates
when a resource transitions. Each subscription is scoped by
(resource_id, role) — different roles see different projections.
"""

import asyncio
import json
from dataclasses import dataclass, field
from typing import Any
from fastapi import WebSocket


@dataclass
class Subscription:
    """A single WebSocket subscription to a resource's projection."""
    websocket: WebSocket
    resource_id: str
    role: str


class SubscriptionManager:
    """Manages WebSocket subscriptions and broadcasts projection updates.

    Thread-safe: uses asyncio locks for concurrent access.
    """

    def __init__(self):
        self._subscriptions: list[Subscription] = []
        self._lock = asyncio.Lock()

    async def subscribe(self, websocket: WebSocket, resource_id: str, role: str):
        """Add a subscription. Call after websocket.accept()."""
        async with self._lock:
            self._subscriptions.append(Subscription(
                websocket=websocket,
                resource_id=resource_id,
                role=role,
            ))

    async def unsubscribe(self, websocket: WebSocket):
        """Remove all subscriptions for a websocket."""
        async with self._lock:
            self._subscriptions = [
                s for s in self._subscriptions if s.websocket is not websocket
            ]

    async def broadcast(self, resource_id: str, system):
        """Broadcast updated projections to all subscribers of a resource.

        Each subscriber gets a projection computed for their role.
        """
        async with self._lock:
            matching = [s for s in self._subscriptions if s.resource_id == resource_id]

        dead = []
        for sub in matching:
            try:
                projection = system.project(sub.resource_id, sub.role)
                await sub.websocket.send_json(projection.to_json())
            except Exception:
                dead.append(sub.websocket)

        # Clean up dead connections
        if dead:
            async with self._lock:
                self._subscriptions = [
                    s for s in self._subscriptions if s.websocket not in dead
                ]

    async def broadcast_event(self, event_data: dict):
        """Broadcast a raw event to all subscribers of the resource.

        Used for event stream subscriptions (pattern-based).
        """
        resource_id = event_data.get("resource_id")
        if not resource_id:
            return

        async with self._lock:
            matching = [s for s in self._subscriptions if s.resource_id == resource_id]

        dead = []
        for sub in matching:
            try:
                await sub.websocket.send_json({
                    "type": "event",
                    "event": event_data,
                })
            except Exception:
                dead.append(sub.websocket)

        if dead:
            async with self._lock:
                self._subscriptions = [
                    s for s in self._subscriptions if s.websocket not in dead
                ]

    @property
    def count(self) -> int:
        return len(self._subscriptions)
