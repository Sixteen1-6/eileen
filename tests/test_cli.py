"""The CLI runs as `python -m eileen` and never fails the doctor."""

import subprocess
import sys

from eileen import __version__
from eileen.cli import main


def _run(*args: str) -> subprocess.CompletedProcess[str]:
    """Run the module CLI in a child interpreter and capture its output."""
    return subprocess.run(
        [sys.executable, "-m", "eileen", *args],
        capture_output=True,
        text=True,
        check=False,
        timeout=60,
    )


def test_version_subcommand() -> None:
    result = _run("version")
    assert result.returncode == 0
    assert __version__ in result.stdout


def test_version_flag_matches_subcommand() -> None:
    result = _run("--version")
    assert result.returncode == 0
    assert __version__ in result.stdout


def test_doctor_exits_zero() -> None:
    result = _run("doctor")
    assert result.returncode == 0
    assert "doctor" in result.stdout
    assert "python" in result.stdout
    assert "git" in result.stdout


def test_main_in_process(capsys) -> None:
    assert main(["version"]) == 0
    assert __version__ in capsys.readouterr().out

    assert main(["doctor"]) == 0
    out = capsys.readouterr().out
    assert "status" in out
    assert "cua-driver" in out

    assert main([]) == 0
    assert "usage" in capsys.readouterr().out
