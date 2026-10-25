@AGENTS.md

## Claude only

- Project skills (`sim-debug`, `add-fuzz-target`, `e2e-scenario`, `work-package` and
  `save-scenario`) arrive in `.claude/skills/` with WP0 phase B. Until then, follow AGENTS.md.
- `.claude/settings.json` denies direct `cargo build`, `test`, `nextest`, `clippy`, `check`, `run`,
  `doc`, `bench`, `fuzz`, `insta` and `mutants`, release builds, `cargo install`, `cargo clean`,
  and reads of vendor credential files. Use `cargo xtask` instead. If a denial blocks you, stop and
  ask; never work around it, because working around it skips the build guard.
- `.claude/rules/ffi.md` loads when you open an `ffi` module. The per-crate rules generated from
  the crate docs arrive with `cargo xtask codegen` in WP0 phase B.
- Final review: a fresh subagent that gets only the diff and the goal, not your own conclusions.
- No language server during build work: rust-analyzer runs its own `cargo check` outside the
  build slots, on a PC that is often short of memory.
- You never sign in for the user, and never read the claude CLI's credential files or
  `~/.claude.json`. The user signs in to each CLI themselves.
