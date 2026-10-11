"""Schema round-trips and policy resolution."""

from datetime import UTC, datetime
from pathlib import Path

from eileen.schemas.desktop import DesktopState
from eileen.schemas.jobs import Job, JobEvent, JobEventType, JobStatus, Salience
from eileen.schemas.policy import Override, RoutingPolicy, default_policy_path, resolve

REPO_ROOT = Path(__file__).resolve().parent.parent
POLICY_FILE = REPO_ROOT / "src" / "eileen" / "policy" / "default.yaml"


def _desktop_state_dict() -> dict:
    """A small but complete DesktopState as the grounding service would emit it."""
    return {
        "monitors": [
            {
                "ordinal": 1,
                "gdi_name": r"\\.\DISPLAY1",
                "friendly_name": "DELL U2720Q",
                "rect": {"left": 0, "top": 0, "right": 3840, "bottom": 2160},
                "work_rect": {"left": 0, "top": 0, "right": 3840, "bottom": 2100},
                "dpi": 144,
                "is_primary": True,
            }
        ],
        "windows": [
            {
                "hwnd": 66082,
                "pid": 4321,
                "process": "chrome.exe",
                "title": "Eileen research",
                "monitor_ordinal": 1,
                "rect": {"left": 10, "top": 10, "right": 1900, "bottom": 1200},
                "is_cloaked": False,
                "z_order": 0,
                "is_foreground": True,
            }
        ],
        "foreground_hwnd": 66082,
        "focused_element": {
            "control_type": "Edit",
            "name": "Address and search bar",
            "automation_id": "omnibox",
            "rect": {"left": 200, "top": 40, "right": 1500, "bottom": 80},
            "patterns": ["Value", "Text"],
            "window_hwnd": 66082,
        },
        "selection": {"text": "second paragraph", "source": "uia_textpattern"},
        "caret_rect": {"left": 300, "top": 45, "right": 302, "bottom": 75},
        "virtual_desktop_id": "{A1B2C3D4-0000-0000-0000-000000000001}",
        "recent_references": ["chrome", "terminal"],
        "captured_at": "2026-09-19T15:00:00+00:00",
    }


def test_desktop_state_round_trip() -> None:
    state = DesktopState.model_validate(_desktop_state_dict())

    assert state.foreground_window is not None
    assert state.foreground_window.process == "chrome.exe"
    assert state.monitors[0].rect.width == 3840
    assert state.selection.source == "uia_textpattern"

    again = DesktopState.model_validate(state.model_dump())
    assert again == state

    from_json = DesktopState.model_validate_json(state.model_dump_json())
    assert from_json == state


def test_desktop_state_defaults() -> None:
    state = DesktopState(
        monitors=[],
        windows=[],
        foreground_hwnd=0,
        captured_at=datetime.now(UTC),
    )
    assert state.focused_element is None
    assert state.selection.text is None
    assert state.selection.source == "none"
    assert state.recent_references == []
    assert state.foreground_window is None


def test_job_and_event() -> None:
    job = Job(
        id="job_0001",
        name="sim refactor",
        kind="coding",
        harness="codex",
        model="gpt-5.6-sol",
        origin="voice",
        cwd=Path("C:/work/sim"),
    )
    assert job.status is JobStatus.queued
    assert job.salience is Salience.when_idle
    assert job.transcript_path is None
    assert not job.is_terminal
    assert job.created_at.tzinfo is not None

    done = job.model_copy(update={"status": JobStatus.completed})
    assert done.is_terminal

    event = JobEvent(job_id=job.id, type=JobEventType.TaskStarted, message="spawned codex")
    assert event.type == "TaskStarted"
    dumped = event.model_dump(mode="json")
    assert dumped["type"] == "TaskStarted"
    assert JobEvent.model_validate(dumped) == event


def test_policy_loads_and_override_wins() -> None:
    policy = RoutingPolicy.from_yaml(POLICY_FILE)

    assert policy.default.kind == "default"
    assert policy.default.harness == "claude"
    assert {rule.kind for rule in policy.rules} >= {
        "coding",
        "architecture",
        "research",
        "trivial",
        "simulation",
    }

    coding = resolve(policy, "coding")
    assert coding.harness == "codex"
    assert coding.model == "gpt-5.6-sol"

    routed = resolve(policy, "coding", Override(harness="claude", reason="user said so"))
    assert routed.harness == "claude"
    assert routed.model == "gpt-5.6-sol"
    assert routed.kind == "coding"

    unknown = resolve(policy, "poetry")
    assert unknown == policy.default

    trivial = resolve(policy, "trivial")
    assert trivial.local_only is True
    assert trivial.cost_tier == "cheap"

    forced_local = resolve(policy, "research", Override(local_only=True, model="qwen3-8b"))
    assert forced_local.local_only is True
    assert forced_local.model == "qwen3-8b"
    assert forced_local.harness == "claude"


def test_default_policy_path_matches_source_tree() -> None:
    assert default_policy_path().resolve() == POLICY_FILE.resolve()
    assert default_policy_path().is_file()
