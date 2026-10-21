"""Latency per approval round trip across the spike runs, from the shared wall clock.

Usage: python -I latency.py

The runner, the hook listener and the approve server all stamp time.time_ns(), so their logs line
up. It prints the median, the 90th percentile and the range of:
  - the guard's round trip as the CLI sees it (the stream's PreToolUse hook_started to
    hook_response lines; the CLI runs PreToolUse hooks one at a time, so the n-th pair belongs to
    the n-th request the listener logged), leaving out the timeout and dead-port cases;
  - the guard's own time (receive to reply);
  - the guard's `ask` reply to the approve server receiving its tools/call, for the first ask in
    a process and for later asks in one warm process (case latency-5);
  - the guard's `ask` reply to the stdio control_request reaching the runner;
  - the approve server's own time (receive to reply), leaving out the hold and hang cases.
"""

import json
import statistics as st
from pathlib import Path

RUNS = Path(__file__).resolve().parent.parent / "runs"
SKIP_GUARD = {"guard-hang-block", "guard-dead-block", "guard-dead-open"}


def jl(p):
    return [json.loads(x) for x in p.read_text(encoding="utf-8").splitlines() if x.strip()] if p.exists() else []


def show(name, v):
    v = sorted(x for x in v if x is not None)
    if not v:
        print(f"{name}: none")
        return
    p90 = v[int(0.9 * (len(v) - 1))]
    print(f"{name}: n={len(v)} median={st.median(v):.1f} ms p90={p90:.1f} min={v[0]:.1f} max={v[-1]:.1f}")


def main():
    guard_cli, guard_own, ask_first, ask_warm, ask_stdio, route_own = [], [], [], [], [], []
    for d in sorted(RUNS.iterdir()):
        stream = jl(d / "stream.jsonl")
        if not stream:
            continue
        t0 = stream[0]["t_ns"]
        # A guard reply that slept past its case lands in the next case's log; drop it.
        pre = [x for x in jl(d / "listener.jsonl") if x["path"].endswith("pre-tool-use") and x["t_ns_recv"] >= t0]
        started = [s["t_ns"] for s in stream if (s.get("event") or {}).get("subtype") == "hook_started"
                   and s["event"].get("hook_event") == "PreToolUse"]
        answered = [s["t_ns"] for s in stream if (s.get("event") or {}).get("subtype") == "hook_response"
                    and s["event"].get("hook_event") == "PreToolUse"]
        if d.name not in SKIP_GUARD:
            guard_cli += [(b - a) / 1e6 for a, b in zip(started, answered)]
            guard_own += [(x["t_ns_sent"] - x["t_ns_recv"]) / 1e6 for x in pre]
        by_id = {(x.get("body") or {}).get("tool_use_id"): x for x in pre}
        pending, first = {}, True
        for a in jl(d / "approve.jsonl"):
            m = a.get("msg")
            if a["kind"] == "recv" and isinstance(m, dict) and m.get("method") == "tools/call":
                tid = m["params"]["arguments"].get("tool_use_id")
                pending[m["id"]] = a["t_ns"]
                if tid in by_id:
                    gap = (a["t_ns"] - by_id[tid]["t_ns_sent"]) / 1e6
                    (ask_warm if d.name == "latency-5" and not first else ask_first).append(gap)
                    first = False
            if (a["kind"] == "send" and isinstance(m, dict) and m.get("id") in pending
                    and "hold" not in d.name and "hang" not in d.name):
                route_own.append((a["t_ns"] - pending[m["id"]]) / 1e6)
        for s in stream:
            ev = s.get("event") or {}
            if ev.get("type") == "control_request":
                tid = (ev.get("request") or {}).get("tool_use_id")
                if tid in by_id:
                    ask_stdio.append((s["t_ns"] - by_id[tid]["t_ns_sent"]) / 1e6)
    show("guard round trip, CLI view", guard_cli)
    show("guard own time", guard_own)
    show("ask to approve tools/call, first ask in a process", ask_first)
    show("ask to approve tools/call, warm process (latency-5, asks 2 to 5)", ask_warm)
    show("ask to stdio control_request", ask_stdio)
    show("approve server own time", route_own)


if __name__ == "__main__":
    main()
