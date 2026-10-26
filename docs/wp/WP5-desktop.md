# WP5: Desktop control

The living plan for work package 5 of [BUILD.md](../BUILD.md) section 6.7: the desktop server
and its Windows backend. The agent doing the work keeps it current.

## Goal

The neutral desktop server, `eileen-desktop`: the 18 tool handlers with their `ToolSpec`s, tiers,
`observes` rules and output schemas, the safety rules of section 2.6 with the target-category
tiers, selectors and evidence-based self-healing, DPI mapping, set-of-mark drawing, the app-name
scorer and the insertion ladder.

The Windows backend, `eileen-desktop-win`: UIA on dedicated threads with deadlines, window
management and launch, event hooks feeding `DesktopDelta`s and the DesktopState cache, capture,
guarded `SendInput` with leases, app launch outside Eileen's jobs, the address-bar URL, the web
content tree as the `Dom` rung, text insertion including the port of Handy's clipboard
transaction, and the `test-physical` injector, never enabled in a shipped binary.

It starts from the computer-use proof on the owner's PC, with rmcp bumped to the workspace's
3.6.0. It also fixes the testpad fixture so its controls stay inside its window at any DPI, and
builds the medium-integrity helper the hosted desktop lane needs.

## Branch, dependencies and files

- Branch: `wp5/<topic>`.
- Depends on: WP0 phase A, WP1 and WP2, and so on slice S1 having passed. Wave: W3. Desktop: yes.
- Owns: `crates/eileen-desktop`, `crates/eileen-desktop-win` (including the `testpad` fixture as
  an example binary), `scenarios/desktop/`, and this file.

## Done when

The acceptance list of BUILD.md section 6.7, items 1 to 13, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. On owned fixtures: background Invoke and SetValue, the unsaved-state check, the refusals,
   leases with the spoken notice and hand-back, and physical input stopping a sequence.
2. `desktop_state` p95 under 100 ms from the cache, and `desktop_tree` for Notepad and Explorer
   within the proof's numbers, recorded.
3. A hung UIA provider gives a typed error within its deadline, its thread is replaced, and other
   actions keep working while the core keeps listening.
4. A real Claude worker opens Notepad and writes and reads back a sentence after the per-app
   consent question, with every step of the trajectory recorded.
5. Dictation insertion is exact in RichEdit, Notepad and Edge, the user's clipboard is restored,
   and an excluded clipboard entry is never read.
6. The Chromium probe is recorded: web content over UIA, page password detection, and Chrome's
   cost with Eileen running against not running.
7. `scenarios/desktop/` covers the listed cases and replays in simulation and in the desktop lane.
8. DPI mapping unit tests at 100, 125, 150 and 200 percent and for offset monitors; the testpad
   keeps its controls inside its window at 144 DPI.
9. The hosted lane's medium-integrity helper, and the nightly canary that keeps the hosted desktop
   answer true.
10. Neutral table tests of the tier decisions: send-like actions, `Shift+Delete`, Eileen's own
    hotkeys, card numbers and secrets in typed text, and `Show` while the user types.
11. The web content tree serves `desktop_tree` with rung `Dom`, classed `Web` by
    `observed_source`, with its cost recorded.
12. Apps Eileen launches leave its jobs, carry none of its environment and survive sidecar
    restarts, and the session-role shim rule holds.
13. The address bar gives `WindowInfo.url_host` and the focus-change URL in normal and private
    windows, and an unknown host when the bar cannot be read.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

Not started.

## Surprises

None yet.

## Decisions

None yet.

## Outcome

Open.
