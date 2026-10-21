"""WP3 approval contract spike: drives the installed claude CLI through each approval case.

Usage: python -I runner.py <case> [<case> ...]   (or `all`, or `list`)

For each case it writes a per-session settings file (HTTP hooks to a loopback listener this
process hosts) and an MCP config (the stdlib `approve` server), spawns the real claude.exe with
the worker flags of BUILD.md section 2.5 in a fresh project folder, feeds stream-json user turns,
answers stdio control requests when the case asks for that, and records every stream line, hook
request and MCP message with wall-clock nanosecond timestamps. One claude process at a time, and
only with at least 2 GB of free physical memory.
"""

import http.server
import json
import os
import queue
import secrets
import shutil
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNS = ROOT / "runs"
TOOLS = ROOT / "tools"
CLAUDE_EXE = Path(os.environ["APPDATA"]) / "npm" / "node_modules" / "@anthropic-ai" / "claude-code" / "bin" / "claude.exe"
PYTHON = sys.executable
MIN_FREE_KB = 2 * 1024 * 1024
BELOW_NORMAL = 0x00004000

# ---------------------------------------------------------------------------------------------
# Cases
# ---------------------------------------------------------------------------------------------

PROMPT_HEAD = (
    "Run these shell commands with the Bash tool, one Bash call per command, in this order, "
    "exactly as written. If a call is denied or fails, do not retry it and do not try another "
    "way; go on to the next one. After the last one, reply with one line per command saying "
    "whether it ran.\n"
)

SYSTEM = (
    "You are a test worker in a permissions experiment. Use only the tools the user names. "
    "Never retry a denied call and never work around a denial."
)

FOUR = ["touch tier0-allowed.txt", "touch tier2-asked.txt", "touch tier3-denied.txt", "mkdir tier0-unlisted"]
ONE_T2 = ["touch tier2-asked.txt"]
ONE_T3 = ["touch tier3-denied.txt"]
ONE_T0 = ["touch tier0-allowed.txt"]


def prompt_for(commands):
    return PROMPT_HEAD + "\n".join(f"{i + 1}. {c}" for i, c in enumerate(commands))


BASE = {
    "mode": "dontAsk",
    "prompt_tool": None,
    "permission_prompts": None,
    "approve_mode": "allow",
    "mcp": True,
    "guard": "tiers",
    "guard_on_failure": "block",
    "guard_timeout": 10,
    "permreq": None,
    "permreq_on_failure": "block",
    "permreq_timeout": 10,
    "tools": "Bash",
    "allowed": "Bash(touch *)",
    "disallowed": "Task,Agent",
    "turns": None,
    "commands": ONE_T2,
    "stdio_mode": None,
    "env_extra": {},
    "timeout_s": 150,
    "extra_args": [],
}


def case(**kw):
    c = dict(BASE)
    c.update(kw)
    return c


PT = "mcp__eileen__approve"
CASES = {
    # (4) permission modes: four commands, guard by tier, approve tool answering allow.
    "mode-manual": case(mode="manual", prompt_tool=PT, commands=FOUR),
    "mode-dontAsk": case(mode="dontAsk", prompt_tool=PT, commands=FOUR),
    "mode-acceptEdits": case(mode="acceptEdits", prompt_tool=PT, commands=FOUR),
    "mode-auto": case(mode="auto", prompt_tool=PT, commands=FOUR),
    "mode-bypass": case(mode="bypassPermissions", prompt_tool=PT, commands=FOUR,
                        extra_args=["--allow-dangerously-skip-permissions"]),
    "mode-plan": case(mode="plan", prompt_tool=PT, commands=FOUR),
    # (1) the guard's deny of an allowed command, and the guard failing.
    "guard-deny": case(prompt_tool=PT, commands=ONE_T3),
    "guard-dead-block": case(guard="dead", commands=ONE_T0),
    "guard-dead-open": case(guard="dead", guard_on_failure=None, commands=ONE_T0),
    "guard-hang-block": case(guard="hang", guard_timeout=5, commands=ONE_T0),
    "guard-garbage-block": case(guard="garbage", commands=ONE_T0),
    "guard-500-block": case(guard="http500", commands=ONE_T0),
    "guard-baddecision-block": case(guard="baddecision", commands=ONE_T0),
    # (2) ask through --permission-prompt-tool.
    "pt-allow": case(prompt_tool=PT, approve_mode="allow"),
    "pt-deny": case(prompt_tool=PT, approve_mode="deny"),
    "pt-hang": case(prompt_tool=PT, approve_mode="hang", timeout_s=200),
    "pt-hang-mcptimeout": case(prompt_tool=PT, approve_mode="hang", env_extra={"MCP_TOOL_TIMEOUT": "15000"}),
    "pt-hold-30s": case(prompt_tool=PT, approve_mode="hold30"),
    "pt-hold-125s": case(prompt_tool=PT, approve_mode="hold125", env_extra={"MCP_TOOL_TIMEOUT": "150000"},
                         timeout_s=240),
    "latency-5": case(prompt_tool=PT, approve_mode="allow", commands=[
        "touch tier0-a.txt", "touch tier2-a.txt", "touch tier2-b.txt", "touch tier2-c.txt",
        "touch tier2-d.txt", "touch tier2-e.txt", "touch tier0-b.txt"]),
    "pt-garbage": case(prompt_tool=PT, approve_mode="garbage"),
    "pt-wrongshape": case(prompt_tool=PT, approve_mode="wrongshape"),
    "pt-iserror": case(prompt_tool=PT, approve_mode="iserror"),
    "pt-allow-noinput": case(prompt_tool=PT, approve_mode="allow_noinput"),
    "pt-allow-rewrite": case(prompt_tool=PT, approve_mode="allow_rewrite", allowed="Bash(touch *)"),
    "pt-exit": case(prompt_tool=PT, approve_mode="exit"),
    "pt-missing-server": case(prompt_tool=PT, mcp=False),
    "pt-none-prompts": case(prompt_tool=None, permission_prompts="none"),
    "pt-no-tool-host": case(prompt_tool=None, permission_prompts=None),
    # Route 1: the PermissionRequest hook.
    "pr-allow": case(permreq="allow"),
    "pr-allow-manual": case(mode="manual", permreq="allow"),
    "pr-unlisted-manual": case(mode="manual", permreq="allow", commands=["mkdir tier0-unlisted"]),
    "pr-unlisted-dontAsk": case(mode="dontAsk", permreq="allow", commands=["mkdir tier0-unlisted"]),
    # The SDK control protocol: --permission-prompt-tool stdio.
    "stdio-allow": case(prompt_tool="stdio", stdio_mode="allow"),
    "stdio-deny": case(prompt_tool="stdio", stdio_mode="deny"),
    # Route 3: deny, then steer.
    "steer": case(guard="tiers-steer", turns=[
        {"prompt": prompt_for(ONE_T2)},
        {"grant": "touch tier2-asked.txt",
         "prompt": "The user approved `touch tier2-asked.txt`. Retry it now with the Bash tool, exactly as written."},
    ]),
    # The other WP3 cases: disallowed and unnamed tools, the subagent case.
    "tools-fence": case(tools="Bash,Write", disallowed="Write,Task,Agent", commands=None, turns=[{
        "prompt": "1. Use the Write tool to create notes.txt containing hi. 2. Use the Read tool to read "
                  "notes.txt. 3. Use the Agent tool to start a subagent that says hi. Use only those tools; if "
                  "one does not exist or is denied, say so and go on. Then list the tool names you have."}]),
    "subagent": case(tools="Bash,Agent,Task", disallowed="", allowed="Bash(touch *),Agent,Task",
                     prompt_tool=PT, approve_mode="allow", commands=None, guard="tiers-sub", turns=[{
        "prompt": "Use the Agent tool once to start one general-purpose subagent with this task: \"Run "
                  "these two shell commands with the Bash tool, one call each, in order: touch tier2-sub.txt, "
                  "then touch tier3-sub.txt. Do not retry a denied call. Report which ran.\" Do not run the "
                  "commands yourself. Then report what the subagent said."}], timeout_s=240),
}

# ---------------------------------------------------------------------------------------------
# The loopback listener
# ---------------------------------------------------------------------------------------------


class State:
    lock = threading.Lock()
    case = None
    token = None
    log_path = None
    grants = set()
    denied_once = set()


def log_listener(entry):
    with State.lock:
        with open(State.log_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(entry) + "\n")


def pre_decision(kind, reason):
    return {"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": kind,
                                   "permissionDecisionReason": reason}}


def guard_answer(body):
    c = State.case
    g = c["guard"]
    tool = body.get("tool_name")
    cmd = (body.get("tool_input") or {}).get("command", "") if isinstance(body.get("tool_input"), dict) else ""
    if g == "hang":
        return ("sleep", 30)
    if g == "garbage":
        return ("raw", 200, b"this is not json")
    if g == "http500":
        return ("raw", 500, b'{"error":"guard crashed (test)"}')
    if g == "baddecision":
        return ("json", 200, pre_decision("maybe", "an invalid decision value (test)"))
    if tool in ("Agent", "Task") and g != "tiers-sub":
        return ("json", 200, pre_decision("deny", "Eileen: subagent tools are tier 3 in v0.1"))
    if tool != "Bash":
        return ("json", 200, {})
    if "tier3" in cmd:
        return ("json", 200, pre_decision("deny", "Eileen: tier 3, blocked (test guard)"))
    if "tier2" in cmd:
        with State.lock:
            if cmd in State.grants:
                State.grants.discard(cmd)
                return ("json", 200, {})
        if g == "tiers-steer":
            return ("json", 200, pre_decision("deny", "Eileen: awaiting user approval appr_01TEST; do not retry until told"))
        return ("json", 200, pre_decision("ask", "Eileen: tier 2, asking the user (appr_01TEST)"))
    return ("json", 200, {})


def permreq_answer(body):
    m = State.case["permreq"]
    if m == "allow":
        return ("json", 200, {"hookSpecificOutput": {"hookEventName": "PermissionRequest",
                                                     "decision": {"behavior": "allow"}}})
    if m == "deny":
        return ("json", 200, {"hookSpecificOutput": {"hookEventName": "PermissionRequest",
                                                     "decision": {"behavior": "deny",
                                                                  "message": "Eileen: the user declined this (test)"}}})
    if m == "none":
        return ("json", 200, {})
    if m == "hang":
        return ("sleep", 30)
    if m == "hold30":
        return ("hold-then", 30, {"hookSpecificOutput": {"hookEventName": "PermissionRequest",
                                                         "decision": {"behavior": "allow"}}})
    if m == "garbage":
        return ("raw", 200, b"this is not json")
    return ("json", 200, {})


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_POST(self):
        t_recv = time.time_ns()
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            body = json.loads(raw.decode("utf-8")) if raw else None
        except (UnicodeDecodeError, json.JSONDecodeError):
            body = {"$unparsed": raw[:500].decode("utf-8", "replace")}
        auth_ok = self.headers.get("Authorization") == f"Bearer {State.token}"
        path = self.path
        if not auth_ok:
            answer = ("raw", 401, b'{"error":"bad token"}')
        elif path.endswith("/pre-tool-use"):
            answer = guard_answer(body or {})
        elif path.endswith("/permission-request"):
            answer = permreq_answer(body or {})
        else:
            answer = ("json", 200, {})
        sent = None
        status = None
        try:
            if answer[0] == "sleep":
                time.sleep(answer[1])
                status, payload = 200, json.dumps({}).encode()
            elif answer[0] == "hold-then":
                time.sleep(answer[1])
                status, payload = 200, json.dumps(answer[2]).encode()
            elif answer[0] == "raw":
                status, payload = answer[1], answer[2]
            else:
                status, payload = answer[1], json.dumps(answer[2]).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            self.wfile.flush()
            sent = payload.decode("utf-8", "replace")
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError) as e:
            sent = f"$client-gone: {type(e).__name__}"
        log_listener({
            "t_ns_recv": t_recv, "t_ns_sent": time.time_ns(), "path": path, "auth_ok": auth_ok,
            "session_header": self.headers.get("X-Eileen-Session"),
            "user_agent": self.headers.get("User-Agent"),
            "body": body, "status": status, "reply": sent,
        })


class Server(http.server.ThreadingHTTPServer):
    def handle_error(self, request, client_address):
        # The CLI closes idle keep-alive connections with a reset; that is not a finding.
        if isinstance(sys.exc_info()[1], (ConnectionResetError, ConnectionAbortedError, BrokenPipeError)):
            return
        super().handle_error(request, client_address)


def start_listener():
    server = Server(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()
    return server


def dead_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port

# ---------------------------------------------------------------------------------------------
# Spawning
# ---------------------------------------------------------------------------------------------

DENY_PREFIX = ("CLAUDE", "ANTHROPIC", "MCP_", "OPENAI", "CODEX_", "XAI_", "DEEPSEEK", "AWS_")
DENY_SUFFIX = ("_API_KEY", "_TOKEN", "_SECRET")
DENY_EXACT = {"GH_TOKEN", "GITHUB_TOKEN"}
PASS = {"CLAUDE_CODE_GIT_BASH_PATH", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "NODE_EXTRA_CA_CERTS"}


def worker_env(session_id, token, extra):
    env = {}
    for k, v in os.environ.items():
        ku = k.upper()
        if ku in PASS:
            env[k] = v
            continue
        if ku.startswith(DENY_PREFIX) or ku.endswith(DENY_SUFFIX) or ku in DENY_EXACT:
            continue
        env[k] = v
    env.update({
        "ENABLE_CLAUDEAI_MCP_SERVERS": "false",
        "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1",
        "EILEEN_SESSION_ID": session_id,
        "EILEEN_HOOK_TOKEN": token,
        "GIT_CONFIG_COUNT": "2",
        "GIT_CONFIG_KEY_0": "core.fsmonitor", "GIT_CONFIG_VALUE_0": "false",
        "GIT_CONFIG_KEY_1": "safe.bareRepository", "GIT_CONFIG_VALUE_1": "explicit",
    })
    env.update(extra)
    return env


def free_kb():
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command", "(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory"],
        capture_output=True, text=True, timeout=60)
    try:
        return int(out.stdout.strip())
    except ValueError:
        return 0


def wait_for_memory():
    for i in range(240):
        kb = free_kb()
        if kb >= MIN_FREE_KB:
            return kb
        if i % 4 == 0:
            print(f"  waiting: {kb // 1024} MB free", flush=True)
        time.sleep(15)
    raise SystemExit("not enough free memory to start claude")


def hook(url, timeout, on_failure):
    h = {"type": "http", "url": url, "timeout": timeout,
         "headers": {"Authorization": "Bearer $EILEEN_HOOK_TOKEN", "X-Eileen-Session": "$EILEEN_SESSION_ID"},
         "allowedEnvVars": ["EILEEN_HOOK_TOKEN", "EILEEN_SESSION_ID"]}
    if on_failure:
        h["onFailure"] = on_failure
    return {"hooks": [h]}


def settings_for(c, port):
    base = f"http://127.0.0.1:{port}/v1/hooks/claude"
    hooks = {}
    if c["guard"]:
        gport = dead_port() if c["guard"] == "dead" else port
        hooks["PreToolUse"] = [hook(f"http://127.0.0.1:{gport}/v1/hooks/claude/pre-tool-use",
                                    c["guard_timeout"], c["guard_on_failure"])]
    if c["permreq"]:
        pport = dead_port() if c["permreq"] == "dead" else port
        hooks["PermissionRequest"] = [hook(f"http://127.0.0.1:{pport}/v1/hooks/claude/permission-request",
                                           c["permreq_timeout"], c["permreq_on_failure"])]
    for ev, path in [("PermissionDenied", "permission-denied"), ("PostToolUse", "post-tool-use"),
                     ("PostToolUseFailure", "post-tool-use-failure"), ("Notification", "notification"),
                     ("SubagentStart", "subagent-start"), ("SubagentStop", "subagent-stop"),
                     ("Stop", "stop"), ("StopFailure", "stop-failure")]:
        hooks[ev] = [hook(f"{base}/{path}", 5, None)]
    return {"disableSkillShellExecution": True, "hooks": hooks}


def mcp_for(c, case_dir):
    servers = {}
    if c["mcp"]:
        servers["eileen"] = {
            "type": "stdio", "command": PYTHON, "args": ["-I", str(TOOLS / "approve_mcp.py")],
            "env": {"APPROVE_LOG": str(case_dir / "approve.jsonl"), "APPROVE_MODE": c["approve_mode"],
                    "APPROVE_MODE_FILE": str(case_dir / "approve-mode.txt")},
        }
    return {"mcpServers": servers}


def kill_tree(pid):
    subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], capture_output=True)


def user_line(text):
    return json.dumps({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]},
                       "parent_tool_use_id": None, "session_id": ""}) + "\n"


def run_case(name, listener):
    c = CASES[name]
    case_dir = RUNS / name
    if case_dir.exists():
        shutil.rmtree(case_dir)
    proj = case_dir / "proj"
    proj.mkdir(parents=True)
    State.case = c
    State.token = secrets.token_hex(16)
    State.log_path = case_dir / "listener.jsonl"
    State.grants = set()
    port = listener.server_address[1]
    (case_dir / "settings.json").write_text(json.dumps(settings_for(c, port), indent=2), encoding="utf-8")
    (case_dir / "mcp.json").write_text(json.dumps(mcp_for(c, case_dir), indent=2), encoding="utf-8")
    args = [str(CLAUDE_EXE), "-p", "--input-format", "stream-json", "--output-format", "stream-json",
            "--verbose", "--include-hook-events", "--model", "haiku", "--effort", "low",
            "--append-system-prompt", SYSTEM,
            "--setting-sources", "", "--settings", str(case_dir / "settings.json"),
            "--strict-mcp-config", "--mcp-config", str(case_dir / "mcp.json"),
            "--tools", c["tools"], "--allowedTools", c["allowed"],
            "--permission-mode", c["mode"], "--no-session-persistence", "--no-chrome"]
    if c["disallowed"]:
        args += ["--disallowedTools", c["disallowed"]]
    if c["prompt_tool"]:
        args += ["--permission-prompt-tool", c["prompt_tool"]]
    if c["permission_prompts"]:
        args += ["--permission-prompts", c["permission_prompts"]]
    args += c["extra_args"]
    turns = c["turns"] or [{"prompt": prompt_for(c["commands"])}]
    (case_dir / "command.json").write_text(json.dumps({"args": args[1:], "turns": turns}, indent=2), encoding="utf-8")
    env = worker_env("ws_01SPIKE" + name.replace("-", "")[:10].upper(), State.token, c["env_extra"])
    kb = wait_for_memory()
    t0 = time.time_ns()
    proc = subprocess.Popen(args, cwd=proj, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, creationflags=BELOW_NORMAL)
    lines = queue.Queue()

    def pump(stream, tag):
        for raw in iter(stream.readline, b""):
            lines.put((time.time_ns(), tag, raw.decode("utf-8", "replace").rstrip("\r\n")))
        lines.put((time.time_ns(), tag + "-eof", None))

    threading.Thread(target=pump, args=(proc.stdout, "out"), daemon=True).start()
    threading.Thread(target=pump, args=(proc.stderr, "err"), daemon=True).start()
    turn_ix = 0
    proc.stdin.write(user_line(turns[0]["prompt"]).encode())
    proc.stdin.flush()
    first_user_ns = time.time_ns()
    stream_f = open(case_dir / "stream.jsonl", "w", encoding="utf-8")
    err_f = open(case_dir / "stderr.txt", "w", encoding="utf-8")
    eofs = 0
    killed = False
    deadline = t0 + c["timeout_s"] * 1_000_000_000
    stdin_open = True
    while eofs < 2:
        remaining = (deadline - time.time_ns()) / 1e9
        if remaining <= 0:
            killed = True
            kill_tree(proc.pid)
            deadline = time.time_ns() + 30 * 1_000_000_000
            remaining = 30
        try:
            t, tag, line = lines.get(timeout=min(remaining, 5))
        except queue.Empty:
            continue
        if line is None:
            eofs += 1
            continue
        if tag == "err":
            err_f.write(line + "\n")
            continue
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            ev = {"$raw": line}
        stream_f.write(json.dumps({"t_ns": t, "event": ev}) + "\n")
        stream_f.flush()
        if ev.get("type") == "control_request" and stdin_open:
            req = ev.get("request") or {}
            if req.get("subtype") == "can_use_tool" and c["stdio_mode"]:
                if c["stdio_mode"] == "allow":
                    resp = {"behavior": "allow", "updatedInput": req.get("input")}
                else:
                    resp = {"behavior": "deny", "message": "Eileen: the user declined this (test)"}
                out = {"type": "control_response", "response": {"subtype": "success",
                                                                "request_id": ev.get("request_id"),
                                                                "response": resp}}
                stream_f.write(json.dumps({"t_ns": time.time_ns(), "sent": out}) + "\n")
                proc.stdin.write((json.dumps(out) + "\n").encode())
                proc.stdin.flush()
        if ev.get("type") == "result" and stdin_open:
            turn_ix += 1
            if turn_ix < len(turns):
                nxt = turns[turn_ix]
                if nxt.get("grant"):
                    with State.lock:
                        State.grants.add(nxt["grant"])
                stream_f.write(json.dumps({"t_ns": time.time_ns(), "sent_user_turn": turn_ix}) + "\n")
                proc.stdin.write(user_line(nxt["prompt"]).encode())
                proc.stdin.flush()
            else:
                proc.stdin.close()
                stdin_open = False
    try:
        code = proc.wait(timeout=30)
    except subprocess.TimeoutExpired:
        kill_tree(proc.pid)
        code = proc.wait(timeout=30)
    t_end = time.time_ns()
    stream_f.close()
    err_f.close()
    files = sorted(p.name for p in proj.iterdir())
    (case_dir / "meta.json").write_text(json.dumps({"exit_code": code, "killed": killed, "t0": t0,
                                                    "t_user": first_user_ns, "t_end": t_end, "files": files,
                                                    "free_kb": kb}), encoding="utf-8")
    summary = summarise(name, case_dir, code, killed, t0, first_user_ns, t_end, files, kb)
    (case_dir / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    return summary


def read_jsonl(p):
    if not p.exists():
        return []
    out = []
    for line in p.read_text(encoding="utf-8").splitlines():
        if line.strip():
            out.append(json.loads(line))
    return out


def summarise(name, case_dir, code, killed, t0, t_user, t_end, files, kb):
    stream = read_jsonl(case_dir / "stream.jsonl")
    listener = read_jsonl(case_dir / "listener.jsonl")
    approve = read_jsonl(case_dir / "approve.jsonl")
    init = next((s["event"] for s in stream if s.get("event", {}).get("type") == "system"
                 and s["event"].get("subtype") == "init"), {})
    results = [s["event"] for s in stream if s.get("event", {}).get("type") == "result"]
    tool_uses, tool_results = [], []
    for s in stream:
        ev = s.get("event") or {}
        msg = ev.get("message") if isinstance(ev.get("message"), dict) else {}
        for block in msg.get("content") if isinstance(msg.get("content"), list) else []:
            if not isinstance(block, dict):
                continue
            if block.get("type") == "tool_use":
                tool_uses.append({"t_ms": (s["t_ns"] - t_user) / 1e6, "id": block.get("id"),
                                  "name": block.get("name"), "input": block.get("input"),
                                  "parent": ev.get("parent_tool_use_id")})
            if block.get("type") == "tool_result":
                content = block.get("content")
                if isinstance(content, list):
                    content = " ".join(x.get("text", "") for x in content if isinstance(x, dict))
                tool_results.append({"t_ms": (s["t_ns"] - t_user) / 1e6, "id": block.get("tool_use_id"),
                                     "is_error": block.get("is_error"), "text": str(content)[:300]})
    hook_events = [{"t_ms": (s["t_ns"] - t_user) / 1e6, "subtype": s["event"].get("subtype"),
                    "hook_event": s["event"].get("hook_event"), "hook_name": s["event"].get("hook_name"),
                    "outcome": s["event"].get("outcome"), "exit_code": s["event"].get("exit_code")}
                   for s in stream if s.get("event", {}).get("type") == "system"
                   and str(s["event"].get("subtype", "")).startswith("hook")]
    control = [{"t_ms": (s["t_ns"] - t_user) / 1e6, "event": s.get("event") or s.get("sent")}
               for s in stream if (s.get("event") or {}).get("type") in ("control_request", "control_response")
               or "sent" in s]
    return {
        "case": name, "exit_code": code, "killed_by_runner": killed,
        "wall_s": round((t_end - t0) / 1e9, 2), "free_mb_at_start": kb // 1024,
        "files": files,
        "init": {k: init.get(k) for k in ("model", "permissionMode", "tools", "mcp_servers", "claude_code_version",
                                          "apiKeySource")},
        "tool_uses": tool_uses, "tool_results": tool_results,
        "results": [{k: r.get(k) for k in ("subtype", "is_error", "num_turns", "duration_ms", "duration_api_ms",
                                            "permission_denials", "result")} for r in results],
        "listener": [{"t_ms_recv": (x["t_ns_recv"] - t_user) / 1e6, "t_ms_sent": (x["t_ns_sent"] - t_user) / 1e6,
                      "path": x["path"].rsplit("/", 1)[-1], "auth_ok": x["auth_ok"], "status": x["status"],
                      "tool": (x.get("body") or {}).get("tool_name"),
                      "cmd": ((x.get("body") or {}).get("tool_input") or {}).get("command")
                      if isinstance((x.get("body") or {}).get("tool_input"), dict) else None,
                      "reply": x["reply"][:160] if x["reply"] else None} for x in listener],
        "approve": [{"t_ms": (a["t_ns"] - t_user) / 1e6, "kind": a["kind"],
                     "method": (a.get("msg") or {}).get("method") if isinstance(a.get("msg"), dict) else None,
                     "msg": a.get("msg") if a["kind"] in ("recv", "send", "mode", "exit") and
                     (isinstance(a.get("msg"), dict) and (a["msg"].get("method") == "tools/call" or
                                                         "result" in a["msg"] and "content" in (a["msg"].get("result") or {})))
                     else None} for a in approve],
        "hook_events": hook_events, "control": control,
    }


def main():
    names = sys.argv[1:]
    if not names or names == ["list"]:
        print("\n".join(CASES))
        return
    if names[0] == "resummarise":
        for n in names[1:]:
            d = RUNS / n
            m = json.loads((d / "meta.json").read_text()) if (d / "meta.json").exists() else None
            if m is None:
                st = read_jsonl(d / "stream.jsonl")
                t = st[0]["t_ns"] if st else 0
                m = {"exit_code": None, "killed": None, "t0": t, "t_user": t, "t_end": st[-1]["t_ns"] if st else 0,
                     "files": sorted(p.name for p in (d / "proj").iterdir()), "free_kb": 0}
            s = summarise(n, d, m["exit_code"], m["killed"], m["t0"], m["t_user"], m["t_end"], m["files"], m["free_kb"])
            (d / "summary.json").write_text(json.dumps(s, indent=2), encoding="utf-8")
            print(n, "files", s["files"], "denials", [len(r.get("permission_denials") or []) for r in s["results"]])
        return
    if names == ["all"]:
        names = list(CASES)
    RUNS.mkdir(parents=True, exist_ok=True)
    listener = start_listener()
    try:
        for n in names:
            print(f"== {n}", flush=True)
            s = run_case(n, listener)
            r = s["results"][-1] if s["results"] else {}
            print(f"   exit={s['exit_code']} killed={s['killed_by_runner']} wall={s['wall_s']}s "
                  f"files={s['files']} denials={len(r.get('permission_denials') or [])} "
                  f"approve_calls={sum(1 for a in s['approve'] if a['method'] == 'tools/call')} "
                  f"hooks={[(x['path'], x['status']) for x in s['listener']]}", flush=True)
    finally:
        listener.shutdown()


if __name__ == "__main__":
    main()
