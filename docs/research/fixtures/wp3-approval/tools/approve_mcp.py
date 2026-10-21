"""A stdlib-only stdio MCP server with one tool, `approve`, for the WP3 approval contract spike.

Newline-delimited JSON-RPC 2.0 on stdin and stdout. Every message it receives and every reply it
sends is appended to the file named by APPROVE_LOG as one JSON line with a wall-clock timestamp in
nanoseconds, so the runner can line its timings up with the CLI's stream and the hook listener.

The reply to `tools/call` follows APPROVE_MODE (or the first line of APPROVE_MODE_FILE when that
file exists, so a runner can change it between turns):

  allow          {"behavior": "allow", "updatedInput": <the input it was given>}
  allow_noinput  {"behavior": "allow"}
  allow_rewrite  {"behavior": "allow", "updatedInput": <input with "tier2" replaced by "rewritten">}
  deny           {"behavior": "deny", "message": "..."}
  hang           never answers (sleeps)
  garbage        the text "this is not json"
  wrongshape     {"decision": "approve"}
  iserror        isError true with a text block
  exit           the process exits without answering
  hold<N>        waits N seconds, then answers like allow
"""

import json
import os
import sys
import time

LOG = os.environ.get("APPROVE_LOG")
MODE_FILE = os.environ.get("APPROVE_MODE_FILE")


def log(kind, payload):
    if not LOG:
        return
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(json.dumps({"t_ns": time.time_ns(), "kind": kind, "msg": payload}) + "\n")


def mode():
    if MODE_FILE and os.path.exists(MODE_FILE):
        with open(MODE_FILE, encoding="utf-8") as f:
            line = f.readline().strip()
            if line:
                return line
    return os.environ.get("APPROVE_MODE", "deny")


def send(obj):
    log("send", obj)
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


TOOL = {
    "name": "approve",
    "description": "Eileen's approval tool (test double). Answers a permission prompt.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "tool_name": {"type": "string"},
            "input": {"type": "object"},
            "tool_use_id": {"type": "string"},
        },
        "required": ["tool_name", "input"],
    },
}


def text_result(text, is_error=False):
    result = {"content": [{"type": "text", "text": text}]}
    if is_error:
        result["isError"] = True
    return result


def answer_call(params):
    args = params.get("arguments") or {}
    tool_input = args.get("input") or {}
    m = mode()
    log("mode", m)
    if m == "allow":
        return text_result(json.dumps({"behavior": "allow", "updatedInput": tool_input}))
    if m == "allow_noinput":
        return text_result(json.dumps({"behavior": "allow"}))
    if m == "allow_rewrite":
        rewritten = dict(tool_input)
        if isinstance(rewritten.get("command"), str):
            rewritten["command"] = rewritten["command"].replace("tier2", "rewritten")
        return text_result(json.dumps({"behavior": "allow", "updatedInput": rewritten}))
    if m == "deny":
        return text_result(
            json.dumps({"behavior": "deny", "message": "Eileen: the user declined this (test)"})
        )
    if m.startswith("hold") and m[4:].isdigit():
        time.sleep(int(m[4:]))
        log("held", int(m[4:]))
        return text_result(json.dumps({"behavior": "allow", "updatedInput": tool_input}))
    if m == "hang":
        time.sleep(3600)
        return text_result(json.dumps({"behavior": "deny", "message": "late"}))
    if m == "garbage":
        return text_result("this is not json")
    if m == "wrongshape":
        return text_result(json.dumps({"decision": "approve"}))
    if m == "iserror":
        return text_result("Eileen: approval service failed (test)", is_error=True)
    if m == "exit":
        log("exit", None)
        sys.exit(3)
    return text_result(json.dumps({"behavior": "deny", "message": f"unknown mode {m}"}))


def main():
    log("start", {"pid": os.getpid(), "mode": mode()})
    for raw in sys.stdin:
        raw = raw.strip()
        if not raw:
            continue
        try:
            msg = json.loads(raw)
        except json.JSONDecodeError:
            log("bad-line", raw[:200])
            continue
        log("recv", msg)
        method = msg.get("method")
        mid = msg.get("id")
        if mid is None:
            continue  # a notification, such as notifications/initialized
        if method == "initialize":
            requested = (msg.get("params") or {}).get("protocolVersion", "2025-06-18")
            send({
                "jsonrpc": "2.0",
                "id": mid,
                "result": {
                    "protocolVersion": requested,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "eileen", "version": "0.0.0-spike"},
                },
            })
        elif method == "tools/list":
            send({"jsonrpc": "2.0", "id": mid, "result": {"tools": [TOOL]}})
        elif method == "tools/call":
            send({"jsonrpc": "2.0", "id": mid, "result": answer_call(msg.get("params") or {})})
        elif method == "ping":
            send({"jsonrpc": "2.0", "id": mid, "result": {}})
        else:
            send({
                "jsonrpc": "2.0",
                "id": mid,
                "error": {"code": -32601, "message": f"method not found: {method}"},
            })
    log("eof", None)


if __name__ == "__main__":
    main()
