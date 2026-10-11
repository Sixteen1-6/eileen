"""Jobs are agent sessions the voice loop tracks without ever blocking on them."""

from datetime import UTC, datetime
from enum import StrEnum
from pathlib import Path

from pydantic import BaseModel, Field


def _now() -> datetime:
    """Timezone-aware UTC now, so timestamps compare cleanly across processes."""
    return datetime.now(UTC)


class JobStatus(StrEnum):
    """Lifecycle of a job from queue to terminal state."""

    queued = "queued"
    running = "running"
    blocked = "blocked"
    awaiting_approval = "awaiting_approval"
    completed = "completed"
    failed = "failed"
    cancelled = "cancelled"


class Salience(StrEnum):
    """How loudly the voice loop should announce this job's events."""

    silent = "silent"
    when_idle = "when_idle"
    interrupt = "interrupt"


class Job(BaseModel):
    """One unit of delegated work, usually a harness session."""

    id: str
    name: str = Field(description="Short generated name the user can refer to by voice.")
    kind: str = Field(description="Routing kind, for example coding, research or simulation.")
    harness: str = Field(description="Worker that runs it, for example claude, codex or local.")
    model: str
    status: JobStatus = JobStatus.queued
    origin: str = Field(description="Where the request came from: voice, telegram, cli.")
    created_at: datetime = Field(default_factory=_now)
    updated_at: datetime = Field(default_factory=_now)
    last_progress: str = ""
    salience: Salience = Salience.when_idle
    transcript_path: Path | None = None
    cwd: Path | None = None

    @property
    def is_terminal(self) -> bool:
        """True once the job can no longer change state."""
        return self.status in {JobStatus.completed, JobStatus.failed, JobStatus.cancelled}


class JobEventType(StrEnum):
    """Events a harness adapter publishes on the local event bus."""

    TaskAccepted = "TaskAccepted"
    TaskStarted = "TaskStarted"
    TaskProgressed = "TaskProgressed"
    TaskBlocked = "TaskBlocked"
    ApprovalRequested = "ApprovalRequested"
    TaskCompleted = "TaskCompleted"
    TaskFailed = "TaskFailed"
    TaskCancelled = "TaskCancelled"


class JobEvent(BaseModel):
    """One event about one job, consumed by the voice loop and the phone bridge."""

    job_id: str
    type: JobEventType
    message: str = ""
    at: datetime = Field(default_factory=_now)
