# WP6: App adapters

The living plan for work package 6 of [BUILD.md](../BUILD.md) section 6.8: the app adapters and
the correction-route spike. The agent doing the work keeps it current.

## Goal

Step 0, in wave W1, is the correction-route spike on the owner's PC. With the owner's agreement,
Resolve is launched with one dedicated test project, under the exclusive build slot, and no Rust
is compiled. Through the Lua bridge it tests importing a generated Fusion `.setting`, a DRX that
carries a window, undo per route, render progress, exporting stills across a clip, and whether
`ApplyGradeFromDRX` replaces existing nodes. The results and the chosen route are recorded here,
with an amendment to section 3.3.9 if `ResolveOp` needs to change.

The rest, in wave W4:

- `eileen-apps`, neutral and tested on Linux: the tool handlers and `ToolSpec`s, `routes_lookup`
  with the fuzzed `registry_reply` parser, the installer for approved MCP servers, render
  discovery, the ResolveMCP client path, timecode math, the bridge protocol, media history and
  SMTC position, face picking and tracking, the Fusion generator and recent-work ranking.
- `eileen-apps-win`: `AppHost` for Word, Excel, Explorer, Photoshop and the Resolve gate, one SMTC
  manager, launches through `eileen_supervisor::user_launch`, late-bound `IDispatch` on per-target
  STA threads, and the recent-work index.
- The Resolve bridge in `adapters/resolve/`, YuNet through `ort`, and
  `eileen-apps doctor resolve`.

## Branch, dependencies and files

- Branch: `wp6/<topic>`.
- Depends on: WP0 phase A, WP1 and WP2; step 0 depends on nothing.
- Wave: step 0 in W1, the rest in W4. Desktop: yes.
- Owns: `crates/eileen-apps` (including `models.toml`), `crates/eileen-apps-win`,
  `adapters/resolve/`, `scenarios/apps/`, the fuzz targets `resolve_reply` and `registry_reply`
  with their corpora and regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.8, items 1 to 8, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. On the dedicated test project: `goto` across frame rates, still export, the chosen route's edit
   and undo, latencies and render progress; later, the same through ResolveMCP.
2. With Resolve closed no call stalls: the gate answers in under 50 ms, and after a launch `hello`
   is polled until ready.
3. YuNet finds, picks, crops and tracks faces in committed synthetic images, and sharpness rises
   after a known sharpen filter.
4. SMTC pause, play and seek are confirmed by events, YouTube play-at and seek work on the owner's
   PC, and `reuse_playing` opens no second tab or window.
5. Word selection and Excel's active cell read from test documents, `apps_open_path` opens a
   fixture, and `apps_open_uri` refuses a scheme outside the allowlist.
6. The fuzz target `resolve_reply` runs clean for 10 minutes (`long.yml`).
7. `scenarios/apps/` covers the listed cases, and B1's deterministic phases run end to end with a
   stub worker.
8. `routes_lookup` parses registry replies as `Untrusted` entries with their namespaces,
   `registry_reply` fuzzes clean, and the installer refuses scripts, OCI and remote-only entries.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

Not started.

## Surprises

None yet.

## Decisions

None yet.

## Outcome

Open.
