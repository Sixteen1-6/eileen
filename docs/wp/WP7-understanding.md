# WP7: Intent, settings and learning

The living plan for work package 7 of [BUILD.md](../BUILD.md) section 6.9: the resolver, the
settings, skills and learning. The agent doing the work keeps it current.

## Goal

Phase A, in wave W3:

- `eileen-intent`, the resolver of section 2.4. Its first pull request wires
  `eileen_intent::Resolver` into the composition root in place of WP2's `SliceResolver` and
  deletes it (an amendment to WP2's files), with slice S1's stub variant still green. It brings
  the fact phrases, `follow_up`, learned templates and routing by level.
- `eileen-config`: the `SettingSpec` registry with risk classes, comment-preserving apply, the
  `SettingsValidator`, secrets through `SecretStore`, and the machine profile's keys.
- `eileen-skills`: the template grammar, the linter, computed provenance and the deny-scan.
- `eileen-learn`: the recorder, `learn.sqlite`, the snapshot verifier, draft review, the
  librarian scheduler, the user model, the phrasing store, route and source statistics, the
  ledger and forget.

Phase B, in wave W4: recipes with operands and checks, shadow agreement, the `ReplayMachine`,
self-healing, probation, L2 semantic matching and repair.

The D77 notes store and ideas 3, 8 and 9 come in both phases. `eileen_learn::vendor_notes` waits
for the owner's decision on D77's narrowing of D4.

## Branch, dependencies and files

- Branch: `wp7/<topic>`.
- Depends on: WP0 phase A, WP1, WP2 and WP3. Desktop: no.
- Wave: phase A in W3 (with WP5), phase B in W4 (with WP6).
- Owns: `crates/eileen-intent`, `crates/eileen-skills`, `crates/eileen-learn` (including
  `tests/scripts/` for the librarian and skill-invocation stub scripts), `crates/eileen-config`,
  `scenarios/learning/`, `scenarios/settings/`, the fuzz targets `config_toml`, `skill_record` and
  `command_grammar` with their corpora and regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.9, items 1 to 14, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. The learning-loop report's eleven CI acceptance tests pass, with versions in SQLite and the
   changes of section 2.10.
2. The matcher golden file: the report's ten row-15 utterances give exactly those results, with
   p99 under 1 ms at 5,000 skills.
3. The resolver golden file: every grammar phrase with ten non-acting paraphrases, the ambiguity,
   fact and follow-up cases, and `command_grammar` fuzzing clean.
4. Settings: a rename runs its checks and undoes; hotkey conflicts, tier relaxations, Safety keys,
   syntax errors and comments behave as specified.
5. Poisoning: injected instructions never become a skill line, recipe step, fact or note, and
   tainted drafts are held or rejected by provenance.
6. Privacy: after a forget, the canary test finds no trace anywhere, dictation never reaches a
   trajectory as words, and the librarian never gets fast-path or replay jobs.
7. On the owner's PC: the librarian distills a real coding trajectory into a draft, and a second
   run that follows the skill uses fewer model turns.
8. Phase B: recipes replay with every check, hand off, fall back and never skip `confirm` or
   `ask`, and the shadow-agreement and linter tables pass.
9. `scenarios/learning/` and `scenarios/settings/` replay green, and `config_toml` and
   `skill_record` fuzz clean for 10 minutes.
10. A snapshot changed after spawn, or a skill outside the manifest, marks the trajectory
    tampered, and forgetting a held skill recycles its session.
11. Phase B: the positive half of the phrasing loop and the route memory, in simulation and on the
    owner's PC, with the measured threshold recorded here.
12. The machine profile's settings: every fitted key accepts `auto`, none is a Safety key, and the
    new phrases are in the resolver golden file.
13. The notes store (D77): notes the CLI reads, concurrent writers, reconciliation, held changes,
    and vendor memory off until this item passes.
14. Ideas 3, 8 and 9 (D77): induced templates, fault injection before level 3, and re-proving
    skills after a change.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

Not started.

## Surprises

None yet.

## Decisions

None yet.

## Outcome

Open.
