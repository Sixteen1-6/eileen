# WP8: HUD, first run and Claude integrations

The living plan for work package 8 of [BUILD.md](../BUILD.md) section 6.10: the HUD, the first
run's surface and the Claude plugin. The agent doing the work keeps it current.

## Goal

The first pull request probes the renderer on the owner's PC (DirectWrite into Direct2D on a
DirectComposition surface, and the colour-keyed GDI fallback) with `WDA_EXCLUDEFROMCAPTURE` on
exactly that window type, measures its idle cost with `cargo xtask measure-idle`, and records the
choice here.

Then:

- `eileen-hud`: the platform-neutral layout from `HudState` to a draw list, with insta snapshots,
  the HUD protocol with its `physical` flag, and the accessibility test that maps every clickable
  input to a grammar entry or a hotkey.
- `eileen-hud-win`: the overlay and the strip, Raw Input so only physical clicks count, per-monitor
  DPI, the DWM thumbnail, dropped voice clips, and hiding in presentation and full-screen states.
- The Claude plugin of the claude-mods report: the plugin `eileen`, the handoff skill, the session
  shim, the `Notification` and `Stop` command hooks, a display-only mod, and the root marketplace
  file. The mod never answers `tool.check`, never approves, never submits prompts and never calls
  the model.
- From D77: the notes, held-note review and receipt panels, the first-run privacy line and the
  "tidy my notes" offer.

`integrations/claude-plugin/` does not exist yet. Section 4b.4 and `xtask/tidy/guidance.toml`
list it for a nested `AGENTS.md` (TypeScript, the never-gating rule, plugin validation), which
comes with the folder.

## Branch, dependencies and files

- Branch: `wp8/<topic>`.
- Depends on: WP0 phase A, WP1, WP2, WP3, WP4 and WP7. Wave: W5. Desktop: yes.
- Owns: `crates/eileen-hud`, `crates/eileen-hud-win`, `integrations/claude-plugin/`,
  `.claude-plugin/marketplace.json`, `scenarios/hud/`, `scenarios/claude-plugin/`, the fuzz target
  `hud_line` with its corpus and regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.10, items 1 to 12, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. Draw-list snapshots for every `HudState` variant, and the HUD survives being killed and
   restarted with the current state.
2. On the owner's PC: the HUD never takes focus, clicks pass through, only physical clicks on the
   strip approve, screenshots exclude it, and the thumbnail works.
3. The idle footprint of `eileen-hud.exe`, idle and while updating ten times a second, recorded.
4. The HUD mic button stops capture, the settings panel's changes undo, and a mute key another
   process owns is refused.
5. The first run end to end on the owner's PC: tour, skip and the surprise path by click and by
   voice, every ending deleting what it should, and no demo phrasing kept.
6. `claude plugin validate --strict` and `claude plugin test` pass, with no gating hook and only
   the reviewed hooks and host calls.
7. On the owner's PC: `/eileen:handoff`, `session_say`, the permission-prompt notice that never
   answers or delays the prompt, `session_ask`, and `--notify` always exiting 0.
8. The fuzz target `hud_line` runs clean for 10 minutes (`long.yml`).
9. The accessibility test passes, and draft review, route proposals and an error's Fix button work
   from their panels on the owner's PC.
10. The about panel lists every component and model of `NOTICE` and `models.toml` with its
    licence, and a test fails if `NOTICE` gains an entry the credits lack.
11. The machine panel and chip: snapshots for WP0's three fixture machines, controls through
    `SetSetting`, and endpoint names only on the panel.
12. The notes and receipt panels (D77): snapshots, keep and drop by click and by voice, and no held
    note spoken unprompted.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

Not started.

## Surprises

None yet.

## Decisions

None yet.

## Outcome

Open.
