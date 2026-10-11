# Developer setup

This takes you from a fresh Windows 11 machine to a passing test suite and a running `eileen doctor`. The external pieces (Cua Driver, Playwright MCP, the agent harnesses) are optional until you work on the milestone that needs them.

Third-party facts here are summarized from [RESEARCH.md](RESEARCH.md). When the two disagree, RESEARCH.md wins.

## Prerequisites

| Tool | Version | Needed for |
|---|---|---|
| Windows 11 | current | Everything. The desktop layer uses Win32, DWM and UI Automation and does not run elsewhere. |
| Python | 3.12 or newer | The core package |
| git | any recent | Cloning and pull requests |
| Node | 24 | Playwright MCP (milestone 2) |
| uv | latest | Recommended. Faster installs and a managed venv. Plain `pip` works too. |
| Rust toolchain or .NET SDK | latest stable | Only for the DesktopState sidecar (milestone 1). Not installed on the owner's machine today. |

Install Python from python.org and tick "Add to PATH". Install Node 24 from nodejs.org.

## Clone and install

```powershell
git clone https://github.com/Sixteen1-6/eileen.git
cd eileen
python -m venv .venv
.venv\Scripts\activate
pip install -e ".[dev]"
```

With uv instead:

```powershell
uv venv
.venv\Scripts\activate
uv pip install -e ".[dev]"
```

Then check the install:

```powershell
pytest
python -m eileen --version
python -m eileen doctor
```

`doctor` reports which external pieces it can find (Cua Driver, Playwright MCP, `claude`, `codex`, a microphone) and which environment variables are set. Missing pieces are warnings, not errors, until the milestone that needs them.

## External pieces

None of these are needed to run the tests. Install them when you reach the milestone that uses them.

### Cua Driver (milestone 2, desktop actions)

Cua Driver is the Windows host daemon that exposes UI Automation and input as MCP tools. It is MIT licensed, written in Rust, and ships Python and TypeScript SDKs. Install it from the official docs at cua.ai. Do not build it from source unless you are changing it. After install, `python -m eileen doctor` should list its named pipe.

### Playwright MCP and the browser extension (milestone 2, browser)

Playwright MCP drives your real, logged-in Chrome or Edge tabs through a browser extension. Two steps:

1. Install the Playwright Extension from the Chrome Web Store. It works in Chrome and Edge.
2. Run the server with the extension flag:

```powershell
npx @playwright/mcp@latest --extension
```

The extension shows a tab picker the first time a session attaches. Chrome DevTools MCP is a Chrome-only alternative; see RESEARCH.md section 2.3.

### Agent harnesses (milestone 4)

Install Claude Code and Codex CLI from their vendors, then sign in yourself:

```powershell
claude auth login
codex login
```

Eileen spawns the unmodified binaries in your environment. It never reads their auth files, never runs an OAuth flow for you, and never proxies tokens. If you prefer API keys, set them in `.env` and the router uses them directly. The rules behind this are in RESEARCH.md section 3.

## Environment variables

Copy `.env.example` to `.env` and fill in what you have. `.env` is gitignored. Never commit it.

```ini
# Model access. Optional: the signed-in claude and codex binaries work without these.
ANTHROPIC_API_KEY=
OPENAI_API_KEY=

# Phone bridge (milestone 5). Token from BotFather; ntfy topic name.
TELEGRAM_BOT_TOKEN=
NTFY_TOPIC=

# Set to 1 to keep everything on this machine: no cloud model calls, no subscription-backed workers.
EILEEN_LOCAL_ONLY=0
```

## Running the tests

```powershell
pytest                # full suite
pytest tests/core     # one subpackage
pytest -k desktop     # by keyword
ruff check .
ruff format --check .
```

Unit tests run without a microphone, a display or a signed-in harness. Tests that need real hardware or a real harness are marked `integration` and skipped by default. Run them with `pytest -m integration` on a machine that has the pieces installed.

## Troubleshooting

**PyAudio will not install.** `pip` tries to compile it when no prebuilt wheel matches your Python version, and the build fails without PortAudio headers. Check that your Python version has a wheel on PyPI, or install the Microsoft C++ Build Tools and retry. RealtimeSTT and RealtimeTTS, which the voice loop uses, are known to work on Windows.

**Window rects are off by a scale factor.** Every process that reads window geometry must be per-monitor-V2 DPI aware, so rects come back in physical pixels. If you write a script that talks to Win32 directly, set DPI awareness before the first window call. The sidecar does this for you.

**A UI Automation call hangs.** UIA calls into a frozen app block until that app answers. Every UIA call in Eileen goes through a timeout wrapper; use it instead of calling the library directly. If the hang persists, the app is the problem, not the tree.

**Chrome refuses a CDP connection.** Since Chrome 136, raw CDP against the default profile is refused unless you opt in via `chrome://inspect`. That is why Eileen uses the Playwright extension path for browser work. Chrome DevTools MCP needs Chrome 144 or newer and shows a consent dialog per request.

**`python -m eileen` cannot find the module.** The venv is not active, or you installed without `-e`. Run `.venv\Scripts\activate` and then `pip install -e ".[dev]"` again.

**`claude` or `codex` not found.** They must be on PATH in the shell where you run Eileen. Open a new terminal after installing them.
