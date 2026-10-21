"""Builds the redacted fixture files for docs/research/fixtures/wp3-approval/ from the spike runs.

Usage: python -I make_fixtures.py <out_dir>

Every string goes through one redactor: the user profile folder, the spike folder, the host
session's id and the host name become placeholders, transcript paths become a fixed pattern, and
the run's bearer token never appears (the listener never logged it). The init event keeps only the
fields an approval parser reads, and rate-limit events are left out (they describe the account's
plan usage). The script refuses to write if the user name, the host name or any value named in
REDACT_FORBID_ENV (names of environment variables whose values must not appear) survives.
"""

import json
import os
import re
import socket
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNS = ROOT / "runs"
PROFILE = os.environ["USERPROFILE"]
USER = os.path.basename(PROFILE)
HOST = socket.gethostname()
HOST_SESSION = os.environ.get("CLAUDE_CODE_SESSION_ID", "")


def variants(path):
    fwd = path.replace("\\", "/")
    out = {path, path.replace("\\", "\\\\"), fwd}
    out |= {v.lower() for v in list(out)}
    return sorted(out, key=len, reverse=True)


def red_str(s):
    if "\\.claude\\projects\\" in s or "/.claude/projects/" in s:
        # A transcript path: keep its shape, drop the encoded working folder (it holds the user name).
        tail = re.split(r"[\\/]", s)[-1]
        middle = "<session_id>\\subagents\\" if "subagents" in s else ""
        return "<home>\\.claude\\projects\\<encoded cwd>\\" + middle + tail
    for v in variants(str(RUNS)):
        s = s.replace(v, "<runs>")
    for v in variants(str(ROOT)):
        s = s.replace(v, "<spike>")
    for v in variants(PROFILE):
        s = s.replace(v, "<home>")
    if HOST_SESSION:
        s = s.replace(HOST_SESSION, "<host-session>")
    s = re.sub(rf"(?i)\b{re.escape(USER)}\b", "<user>", s)
    s = re.sub(rf"(?i)\b{re.escape(HOST)}\b", "<host>", s)
    return s


def red(obj):
    if isinstance(obj, str):
        return red_str(obj)
    if isinstance(obj, list):
        return [red(x) for x in obj]
    if isinstance(obj, dict):
        return {k: red(v) for k, v in obj.items()}
    return obj


def jl(p):
    return [json.loads(x) for x in p.read_text(encoding="utf-8").splitlines() if x.strip()]


INIT_KEEP = ("type", "subtype", "session_id", "tools", "mcp_servers", "model", "permissionMode",
             "apiKeySource", "claude_code_version", "capabilities", "agents", "uuid")


def trim_event(ev):
    if ev.get("type") == "system" and ev.get("subtype") == "init":
        out = {k: ev.get(k) for k in INIT_KEEP if k in ev}
        out["$omitted"] = sorted(k for k in ev if k not in INIT_KEEP)
        return out
    return ev


def stream(case, keep_partial=False):
    out = []
    t0 = None
    for s in jl(RUNS / case / "stream.jsonl"):
        ev = s.get("event")
        if ev is not None and ev.get("type") == "rate_limit_event":
            continue
        if t0 is None:
            t0 = s["t_ns"]
        line = {"t_ms": round((s["t_ns"] - t0) / 1e6, 1)}
        if ev is not None:
            line["event"] = trim_event(ev)
        if "sent" in s:
            line["sent_to_cli"] = s["sent"]
        if "sent_user_turn" in s:
            line["sent_user_turn"] = s["sent_user_turn"]
        out.append(line)
    return out


def events(case, pred):
    return [trim_event(s["event"]) for s in jl(RUNS / case / "stream.jsonl")
            if s.get("event") and pred(s["event"])]


def listener(case, path_suffix=None):
    out = []
    for x in jl(RUNS / case / "listener.jsonl"):
        if path_suffix and not x["path"].endswith(path_suffix):
            continue
        out.append({"path": x["path"], "headers_seen": {"X-Eileen-Session": x.get("session_header"),
                                                       "User-Agent": x.get("user_agent"),
                                                       "Authorization": "Bearer <token> (matched)" if x["auth_ok"] else "mismatch"},
                    "body": x["body"], "reply_status": x["status"],
                    "reply": json.loads(x["reply"]) if x["reply"] and x["reply"].startswith("{") else x["reply"]})
    return out


def approve_log(case):
    return [{"dir": a["kind"], "msg": a["msg"]} for a in jl(RUNS / case / "approve.jsonl")
            if a["kind"] in ("recv", "send")]


def tool_results(case):
    out = []
    for s in jl(RUNS / case / "stream.jsonl"):
        ev = s.get("event") or {}
        msg = ev.get("message") if isinstance(ev.get("message"), dict) else {}
        for b in msg.get("content") if isinstance(msg.get("content"), list) else []:
            if isinstance(b, dict) and b.get("type") == "tool_result":
                out.append(b)
    return out


def result(case):
    rs = [s["event"] for s in jl(RUNS / case / "stream.jsonl") if (s.get("event") or {}).get("type") == "result"]
    keep = ("type", "subtype", "is_error", "num_turns", "duration_ms", "duration_api_ms", "permission_denials",
            "terminal_reason", "stop_reason", "result")
    return [{k: r.get(k) for k in keep} for r in rs]


def summary(case):
    return json.loads((RUNS / case / "summary.json").read_text(encoding="utf-8"))


def write(out_dir, name, obj, forbid):
    text = json.dumps(red(obj), indent=2, ensure_ascii=False) + "\n"
    low = text.lower()
    for w in [USER.lower(), HOST.lower(), *forbid]:
        if w and w.lower() in low:
            raise SystemExit(f"{name}: a personal value survived redaction; not written")
    (out_dir / name).write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {name} ({len(text)} bytes)")


CASE_ROWS = [
    # case, route, mode, what the guard answered, what the route answered
    ("guard-deny", "none", "dontAsk", "deny", "-"),
    ("pt-allow", "prompt tool", "dontAsk", "ask", "allow"),
    ("pt-deny", "prompt tool", "dontAsk", "ask", "deny"),
    ("pt-allow-noinput", "prompt tool", "dontAsk", "ask", "allow without updatedInput"),
    ("pt-allow-rewrite", "prompt tool", "dontAsk", "ask", "allow with a rewritten command"),
    ("pt-garbage", "prompt tool", "dontAsk", "ask", "text that is not JSON"),
    ("pt-wrongshape", "prompt tool", "dontAsk", "ask", "JSON without behavior"),
    ("pt-iserror", "prompt tool", "dontAsk", "ask", "isError: true"),
    ("pt-exit", "prompt tool", "dontAsk", "ask", "server process exits mid-call"),
    ("pt-missing-server", "prompt tool", "dontAsk", "ask", "no MCP server configured"),
    ("pt-hang", "prompt tool", "dontAsk", "ask", "no answer (runner killed the CLI at 200 s)"),
    ("pt-hang-mcptimeout", "prompt tool", "dontAsk", "ask", "no answer, MCP_TOOL_TIMEOUT=15000"),
    ("pt-hold-30s", "prompt tool", "dontAsk", "ask", "allow after 30 s"),
    ("pt-hold-125s", "prompt tool", "dontAsk", "ask", "allow after 125 s, MCP_TOOL_TIMEOUT=150000"),
    ("pt-none-prompts", "none (--permission-prompts none)", "dontAsk", "ask", "-"),
    ("pt-no-tool-host", "none (default host, no prompt tool)", "dontAsk", "ask", "-"),
    ("pr-allow", "PermissionRequest hook", "dontAsk", "ask", "hook would answer allow"),
    ("pr-allow-manual", "PermissionRequest hook", "manual", "ask", "hook would answer allow"),
    ("pr-unlisted-manual", "PermissionRequest hook", "manual", "no decision, command not allowlisted", "allow"),
    ("pr-unlisted-dontAsk", "PermissionRequest hook", "dontAsk", "no decision, command not allowlisted", "hook would answer allow"),
    ("stdio-allow", "--permission-prompt-tool stdio", "dontAsk", "ask", "control_response allow"),
    ("stdio-deny", "--permission-prompt-tool stdio", "dontAsk", "ask", "control_response deny"),
    ("steer", "deny then steer", "dontAsk", "deny 'awaiting approval', then none after a grant", "-"),
    ("guard-dead-block", "-", "dontAsk", "nothing listening, onFailure block", "-"),
    ("guard-dead-open", "-", "dontAsk", "nothing listening, no onFailure", "-"),
    ("guard-hang-block", "-", "dontAsk", "no reply within the 5 s timeout, onFailure block", "-"),
    ("guard-garbage-block", "-", "dontAsk", "200 with a non-JSON body, onFailure block", "-"),
    ("guard-500-block", "-", "dontAsk", "HTTP 500, onFailure block", "-"),
    ("guard-baddecision-block", "-", "dontAsk", "permissionDecision \"maybe\", onFailure block", "-"),
    ("mode-manual", "prompt tool", "manual", "tiers", "allow"),
    ("mode-dontAsk", "prompt tool", "dontAsk", "tiers", "allow"),
    ("mode-acceptEdits", "prompt tool", "acceptEdits", "tiers", "allow"),
    ("mode-auto", "prompt tool", "auto", "tiers", "allow"),
    ("mode-bypass", "prompt tool", "bypassPermissions", "tiers", "allow"),
    ("mode-plan", "prompt tool", "plan", "tiers", "allow"),
    ("latency-5", "prompt tool", "dontAsk", "tiers", "allow"),
    ("tools-fence", "-", "dontAsk", "tiers", "-"),
    ("subagent", "prompt tool", "dontAsk", "tiers (Agent allowed)", "allow"),
]


def main():
    out_dir = Path(sys.argv[1])
    out_dir.mkdir(parents=True, exist_ok=True)
    forbid = [os.environ[k] for k in os.environ.get("REDACT_FORBID_ENV", "").split(",") if k and os.environ.get(k)]

    rows = []
    for case, route, mode, guard, answer in CASE_ROWS:
        s = summary(case)
        cmd = json.loads((RUNS / case / "command.json").read_text(encoding="utf-8"))
        rows.append({
            "case": case, "route": route, "permission_mode": mode, "guard": guard, "route_answer": answer,
            "commands_asked": [t["prompt"].split("\n")[1:] if "\n" in t["prompt"] else [t["prompt"]] for t in cmd["turns"]],
            "files_created": s["files"], "exit_code": s["exit_code"], "killed_by_runner": s["killed_by_runner"],
            "approve_calls": [a["msg"]["params"]["arguments"]["input"].get("command") for a in s["approve"]
                              if a["method"] == "tools/call"],
            "permission_denials": [d["tool_input"].get("command") for r in s["results"]
                                   for d in (r.get("permission_denials") or [])],
            "model_saw": [t["text"] for t in s["tool_results"]],
            "init_tools": s["init"].get("tools"), "init_permission_mode": s["init"].get("permissionMode"),
            "wall_s": s["wall_s"],
        })
    write(out_dir, "cases.json", {"cli": "2.1.304", "model": "claude-haiku-5-5", "cases": rows}, forbid)

    pt = summary("pt-allow")
    write(out_dir, "worker-command.json", {
        "argv_after_exe": json.loads((RUNS / "pt-allow" / "command.json").read_text(encoding="utf-8"))["args"],
        "settings_file": json.loads((RUNS / "pt-allow" / "settings.json").read_text(encoding="utf-8")),
        "mcp_config": json.loads((RUNS / "pt-allow" / "mcp.json").read_text(encoding="utf-8")),
        "first_user_line": {"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": "<prompt>"}]},
                            "parent_tool_use_id": None, "session_id": ""},
    }, forbid)

    write(out_dir, "hooks.json", {
        "pre_tool_use_ask": listener("pt-allow", "pre-tool-use")[0],
        "pre_tool_use_deny": listener("guard-deny", "pre-tool-use")[0],
        "pre_tool_use_no_decision": listener("mode-dontAsk", "pre-tool-use")[0],
        "pre_tool_use_in_subagent": listener("subagent", "pre-tool-use")[1],
        "permission_request": listener("pr-unlisted-manual", "permission-request")[0],
        "post_tool_use": listener("pt-allow", "post-tool-use")[0],
        "stop": listener("pt-allow", "/stop")[0],
        "subagent_start": listener("subagent", "subagent-start")[0],
        "subagent_stop": listener("subagent", "subagent-stop")[0],
    }, forbid)

    write(out_dir, "mcp-approve.json", {
        "allow": approve_log("pt-allow"),
        "deny_reply": [m for m in approve_log("pt-deny") if m["dir"] == "send" and m["msg"].get("id") == 2],
    }, forbid)

    write(out_dir, "stdio-control.json", {
        "allow": [x for x in stream("stdio-allow") if (x.get("event") or {}).get("type") == "control_request" or "sent_to_cli" in x],
        "deny": [x for x in stream("stdio-deny") if (x.get("event") or {}).get("type") == "control_request" or "sent_to_cli" in x],
    }, forbid)

    write(out_dir, "stream-events.json", {
        "init": events("pt-allow", lambda e: e.get("subtype") == "init")[0],
        "hook_started": events("pt-allow", lambda e: e.get("subtype") == "hook_started")[0],
        "hook_response": events("pt-allow", lambda e: e.get("subtype") == "hook_response")[0],
        "hook_response_failed_block": events("guard-dead-block", lambda e: e.get("subtype") == "hook_response")[:1],
        "permission_denied_hook_ask": events("pr-allow", lambda e: e.get("subtype") == "permission_denied"),
        "permission_denied_mode": events("pr-unlisted-dontAsk", lambda e: e.get("subtype") == "permission_denied"),
        "tool_results": {c: tool_results(c) for c in ("guard-deny", "pt-deny", "pt-garbage", "pt-iserror", "pt-exit",
                                                       "pt-missing-server", "pt-hang-mcptimeout", "pt-none-prompts",
                                                       "pt-no-tool-host", "guard-dead-block", "guard-hang-block",
                                                       "guard-garbage-block", "guard-500-block",
                                                       "guard-baddecision-block", "pr-unlisted-dontAsk", "steer")},
        "results": {c: result(c) for c in ("guard-deny", "pt-allow", "pt-iserror", "steer", "subagent")},
    }, forbid)

    for case in ("pt-allow", "steer", "stdio-allow"):
        write(out_dir, f"stream-{case}.json", stream(case), forbid)


if __name__ == "__main__":
    main()
