"""Command line entry point: `eileen version` and `eileen doctor`."""

from __future__ import annotations

import argparse
import platform
import shutil
import sys
from collections.abc import Sequence

from eileen import __version__

REQUIRED_PYTHON: tuple[int, int] = (3, 12)

# Executables the v0.1 nucleus expects on PATH, with a one-line hint for each.
# The doctor only asks the OS whether these exist. It never runs them, never
# touches the network, and never reads any vendor auth file.
EXECUTABLES: list[tuple[str, str]] = [
    ("claude", "install the official claude CLI from Anthropic, then sign in yourself"),
    ("codex", "install the official Codex CLI from OpenAI, then sign in yourself"),
    ("gemini", "optional harness; install Gemini CLI and use an API key"),
    ("node", "install Node 24; Playwright MCP runs on it"),
    ("npx", "ships with Node 24; install Node"),
    ("cua-driver", "install Cua Driver (trycua/cua), the Windows action layer"),
    ("git", "install Git for Windows"),
]


def _python_row() -> tuple[str, str, str]:
    """Report whether the running interpreter is new enough."""
    have = sys.version_info[:3]
    text = ".".join(str(n) for n in have)
    if have[:2] >= REQUIRED_PYTHON:
        return ("python", "ok", text)
    want = ".".join(str(n) for n in REQUIRED_PYTHON)
    return ("python", "missing", f"{text} found, {want} or newer needed")


def _windows_row() -> tuple[str, str, str]:
    """Report whether we are on Windows, the only supported host today."""
    system = platform.system()
    if system == "Windows":
        return ("windows", "ok", f"{system} {platform.release()} ({platform.version()})")
    return ("windows", "missing", f"running on {system or 'unknown'}; Eileen targets Windows 11")


def _executable_row(name: str, hint: str) -> tuple[str, str, str]:
    """Report whether one executable is on PATH."""
    path = shutil.which(name)
    if path:
        return (name, "found", path)
    return (name, "missing", hint)


def _print_table(rows: Sequence[tuple[str, str, str]]) -> None:
    """Print a plain three-column table with aligned headers."""
    header = ("check", "status", "detail")
    widths = [max(len(r[i]) for r in (header, *rows)) for i in range(2)]
    fmt = f"{{:<{widths[0]}}}  {{:<{widths[1]}}}  {{}}"
    print(fmt.format(*header))
    print(fmt.format("-" * widths[0], "-" * widths[1], "-" * 6))
    for row in rows:
        print(fmt.format(*row))


def run_doctor() -> int:
    """Check the host for what Eileen needs and print a plain table. Always exits 0."""
    rows: list[tuple[str, str, str]] = [_windows_row(), _python_row()]
    rows.extend(_executable_row(name, hint) for name, hint in EXECUTABLES)

    print(f"eileen {__version__} doctor")
    print()
    _print_table(rows)
    print()
    missing = [r[0] for r in rows if r[1] == "missing"]
    if missing:
        print("missing: " + ", ".join(missing))
    else:
        print("everything the v0.1 nucleus expects is present")
    return 0


def build_parser() -> argparse.ArgumentParser:
    """Build the argparse parser with the version and doctor subcommands."""
    parser = argparse.ArgumentParser(
        prog="eileen",
        description="Voice-first AI operating layer for a Windows 11 PC.",
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"eileen {__version__}",
        help="print the version and exit (same as the version subcommand)",
    )
    sub = parser.add_subparsers(dest="command", metavar="command")
    sub.add_parser("version", help="print the version")
    sub.add_parser("doctor", help="check this machine for the tools Eileen expects")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """Run the CLI and return an exit code."""
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.command == "version":
        print(f"eileen {__version__}")
        return 0
    if args.command == "doctor":
        return run_doctor()
    parser.print_help()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
