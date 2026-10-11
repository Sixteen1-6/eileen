"""Pydantic models shared across Eileen: desktop state, jobs and routing policy."""

from eileen.schemas.desktop import (
    DesktopState,
    FocusedElement,
    Monitor,
    Rect,
    Selection,
    Window,
)
from eileen.schemas.jobs import Job, JobEvent, JobEventType, JobStatus, Salience
from eileen.schemas.policy import (
    Override,
    PermissionTier,
    RouteRule,
    RoutingPolicy,
    resolve,
)

__all__ = [
    "DesktopState",
    "FocusedElement",
    "Job",
    "JobEvent",
    "JobEventType",
    "JobStatus",
    "Monitor",
    "Override",
    "PermissionTier",
    "Rect",
    "RouteRule",
    "RoutingPolicy",
    "Salience",
    "Selection",
    "Window",
    "resolve",
]
