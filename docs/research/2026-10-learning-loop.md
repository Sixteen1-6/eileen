# The learning loop: prior art, design and v0.1 slice

Research and design pass, 2026-10-20. Question: how does Eileen learn from every request, so that the second time someone asks for something hard it is faster, cheaper and more reliable, and eventually needs no model at all? This is the owner's main point (see "Eileen learns from every request" in [OWNER-DIRECTION.md](../OWNER-DIRECTION.md), 2026-10-20). Every design choice below has to satisfy that entry.

This report is written for contributors who will build the `eileen-learn` crate and its hooks. It covers what others have tried, what was verified on the owner's PC, the design in Rust, three worked examples, and the exact v0.1 slice with acceptance tests.

## How to read this

Tags say how each claim was checked:

- **[ran 10-20]**: run on the owner's PC (Windows 11 Home 26200, Core Ultra 7 155H, 16 GB RAM, claude CLI 2.1.304 signed in to the owner's plan) by an **earlier attempt at this report** on 2026-10-20, between 12:33 and 13:14 local time (16:33 to 17:14 UTC in the debug logs). That attempt finished its research and on-PC checks and was cut off before writing. Its scripts, skill folders, logs and matcher source are kept in `%USERPROFILE%\Projects\eileen-workfiles\2026-10-scratch\learning\`.
- **[re-ran 10-20]**: repeated during this write-up, same day. Only two things were repeated: one run of the already-built matcher binary (it runs in under a second), and a re-read of the skills documentation page whose earlier fetch was not saved.
- **[primary]**: the vendor's or authors' own docs, paper abstract or repository.
- **[secondary]**: press, blogs, search-result snippets. Treat details as soft.
- **[inferred]**: our reasoning. Every threshold and budget number in the design is a starting value we chose, not a measurement.

Nothing in this report ran DaVinci Resolve, Photoshop or a browser. Those hooks are covered by [2026-10-app-hooks.md](2026-10-app-hooks.md). Here, the PC checks are about how the signed-in claude CLI finds, loads and uses learned skills, and how fast a local Rust matcher is.

## 1. Verdict

1. **Learning a skill library from an agent's own work is well studied and partly shipped.** Voyager, Agent Workflow Memory, SkillWeaver, ASI, UFO2, Hermes and Letta all turn trajectories into reusable procedures, and the papers report double-digit relative gains in success rate on web and desktop benchmarks (section 2). Eileen must not claim the idea is new.
2. **What nobody ships, as of 2026-10-20, is the whole loop on a Windows desktop for one person.** That means capture from every job, distillation into the open SKILL.md format so the vendor's own harness loads it, a promotion ladder that ends in replay with no model and a check after every step, handoff to a worker the moment a check fails, local version history with rollback, and voice plus HUD control ("what have you learned", "forget that"). Desktop-specific work (CUA-Skill, Work2Skill, EvoSkill-GUI) is research code on benchmarks, not a product the user controls.
3. **The native route works on this PC [ran 10-20].** The signed-in claude CLI discovered a learned skill supplied from outside its working directory, both as a local plugin (`--plugin-dir`) and as an additional directory (`--add-dir`). The model invoked the skill by name with the spoken argument, read a supporting file from the skill's folder, and answered in the format the skill prescribed. So Eileen's Claude worker can use what Eileen learned with no custom prompt plumbing.
4. **The fast local path is cheap [ran 10-20, re-ran 10-20].** A std-only Rust matcher (typed slot templates, then BM25 hints) took 4 to 6 microseconds per utterance at the median and under 1 ms at p99 with 5,000 skills, and extracted the right slots for all seven positive test utterances.
5. **Safety has to be built in, not added later.** The 2026 skill-marketplace incidents (over 230 malicious skills on ClawHub in one week, and 26.1 percent of 31,132 marketplace skills with at least one vulnerability) show what happens when skills are treated as trusted code. Eileen's learned skills are data written through a validating tool, linted, provenance-tagged per line, and never able to raise a permission.

**Recommendation in one paragraph.** Record a redacted trajectory for every job. Let a low-priority background Claude worker, the librarian, distill successful trajectories into native SKILL.md skills, each optionally paired with a typed recipe of Eileen MCP calls with pre and postcondition checks. Store each skill as its own small git repository inside one local plugin folder, pass that folder to every worker with `--plugin-dir`, and match utterances locally in Rust. Promote skills on evidence: draft, trusted, compiled recipe, spoken fast path. Demote on failure. Replay recipes through the same policy gate a worker uses, checking every step and handing off to a worker on the first failed check. Give the user voice and HUD control over all of it. Keep everything on the PC.

## 2. Prior art, checked 2026-10-20

### 2.1 Research on skill libraries and workflow memory

| Work | What it does | Reported result | What Eileen takes | What Eileen avoids |
|---|---|---|---|---|
| [Voyager](https://arxiv.org/abs/2305.16291) (May 2023) | Minecraft agent with "an ever-growing skill library of executable code"; iterative prompting with environment feedback, execution errors and self-verification | 3.3x more unique items, 2.3x longer distances than prior state of the art [primary, abstract] | Skills as executable programs; verify before a skill is admitted | A sandboxed game with no cost to a wrong action; no permission model |
| [Agent Workflow Memory](https://arxiv.org/abs/2409.07429) (AWM, Sep 2024) | Induces commonly reused routines ("workflows") from trajectories and supplies them to later tasks, offline or online | +24.6% relative on Mind2Web, +51.1% relative on WebArena; fewer steps [primary, abstract] | Online induction from real requests as they arrive; abstract workflows with variables | Text-only workflows that never become deterministic |
| [SkillWeaver](https://arxiv.org/abs/2504.07079) (Apr 2025) | Discovery, practice, distillation of website skills into APIs | +31.8% on WebArena, +39.8% on real sites; APIs written by stronger agents raise weaker agents up to 54.3% [primary, abstract] | Compile skills into typed APIs; a strong model writes, a cheap path runs | Autonomous "practice" on live sites; on a user's PC that means unrequested actions |
| [ASI, Inducing Programmatic Skills](https://arxiv.org/abs/2504.06821) (Apr 2025, rev. Aug 2025) | Induces program skills during operation and verifies them programmatically before use | +23.5% success over a static agent, +11.3% over text skills, 10.7 to 15.3% fewer steps, WebArena [primary, abstract] | Programs beat text for reuse; verification at induction time | Nothing specific; the representation lesson is the main one |
| [ICAL](https://arxiv.org/abs/2406.14596) (Jun 2024) | A vision-language model turns imperfect demonstrations into generalized strategies and annotations, refined by human feedback | +17.5% goal-condition success on TEACh over raw demonstrations; gains on VisualWebArena [primary, abstract] | Clean up a messy first trajectory before saving it; human feedback lowers the cost of later learning | Fine-tuning; Eileen never trains weights |
| [AgentRR, Record and Replay](https://arxiv.org/abs/2505.17716) (May 2025) | Records interaction traces and internal decisions, summarizes them at several abstraction levels, replays with a "check function" as "trust anchor to ensure completeness and safety during replay" | No numbers in the abstract [primary, abstract] | The shape of Eileen's replay: abstraction levels plus a check that guards replay | The abstract does not say what the check tests; Eileen defines its own (section 3.6) |
| [Cradle](https://arxiv.org/abs/2403.03186) (Mar 2024) | General computer control from screenshots to keyboard and mouse code, with six modules including skill curation and memory | Ran four commercial games and five software apps [primary, abstract; module names from the paper body, secondary] | Skill curation as a named module | Pixel-only control; Eileen prefers deterministic hooks first |
| [CUA-Skill](https://arxiv.org/abs/2601.21123) (Jan 2026, Microsoft authors) | Large library of Windows-app skills "coupled with parameterized execution and composition graphs"; retrieval, argument instantiation, failure recovery | No counts on the abstract page [primary, abstract] | Parameterized skills over Windows apps with recovery | A hand-built library; Eileen's grows from the user's own work |
| [From Interaction Traces to Persistent Skills](https://arxiv.org/abs/2609.04869) (Sep 2026) | Online evolution of a versioned skill library from trajectories and evaluator feedback; each iteration runs on a frozen snapshot | Beat a matched empty library in all four OSWorld app domains, +5.7 to +18.6 points [primary, abstract] | Versioned library; changes apply to later runs, never mid-run | Benchmark evaluator as the success signal; Eileen uses the user's reaction |
| [EvoSkill-GUI, Reflect, Revise, Reuse](https://arxiv.org/abs/2609.17653) (Sep 2026) | Multi-file skill packages (retrieval metadata, plans, backup localization, recovery rules, failure cases); an isolated critic diagnoses failures; edits go through a restricted tool | Improves several base models on MobileWorld, AndroidWorld, OSWorld [primary, abstract] | Restricted edit tool for the skill writer; keep failure cases with the skill; isolate the critic | None noted |
| [Work2Skill](https://arxiv.org/abs/2610.12984) (Oct 9, 2026) | Records human desktop work (screen video, mouse and keyboard, accessibility tree aligned) and induces skills with workflow rules, step dependencies and completion checks | WorkDemo-350 dataset [primary, abstract] | Completion checks as part of a skill; accessibility tree as the capture backbone | Continuous screen recording; Eileen records its own tool calls, not the user's screen, by default |
| [A Systematic Survey of Agentic Skills](https://arxiv.org/abs/2608.29596) (Aug 2026) | Nine lifecycle stages from discovery to security governance | Survey [primary, abstract] | A checklist: Eileen's design covers every stage | None |
| [Sleep-time compute](https://arxiv.org/abs/2504.13171) (Apr 2025) | Think offline about a context before the query arrives | About 5x less test-time compute for the same accuracy [primary, abstract] | Run the librarian in idle time, not during the user's turn | None |
| [Agentic Context Engineering](https://arxiv.org/abs/2510.04618) (Oct 2025) | Evolving "playbooks" with itemized, incremental updates instead of whole rewrites | [secondary; numbers not rechecked] | Delta edits for the user model, so detail does not erode with each rewrite | Whole-document rewrites |
| [ReasoningBank](https://arxiv.org/abs/2509.25140) (Sep 2025) | Memory distilled from both successes and failures | [secondary; not rechecked] | Keep failure lessons as "pitfalls" in a skill | None |

### 2.2 Shipped products and open-source agents

| System | How it learns | Status and evidence | Lesson for Eileen |
|---|---|---|---|
| **Agent Skills and the claude CLI** ([docs](https://code.claude.com/docs/en/skills), [spec](https://agentskills.io/specification)) | A skill is a folder with `SKILL.md` (YAML frontmatter, then Markdown) plus optional `scripts/`, `references/`, `assets/`. Descriptions are listed in context; the full body loads only when invoked. Skills come from managed, user, project, nested, `--add-dir` and plugin locations | Name max 64 chars, lowercase, digits and hyphens [primary, spec]. The combined `description` and `when_to_use` is truncated at 1,536 characters in the listing. A skill body stays in context after invocation; after compaction the most recent invocation of each skill is re-attached, first 5,000 tokens each, 25,000 combined. `allowed-tools` grants tools "without asking permission" for that turn. `` !`cmd` `` runs shell commands before the body reaches the model; `"disableSkillShellExecution": true` replaces them with a placeholder for user, project, plugin and additional-directory skills [primary, re-ran 10-20]. Announced as an open standard in December 2025 [secondary] | Write learned skills in this format so the Claude worker, and later Codex, load them natively. Strip `allowed-tools`, `hooks`, `shell` and similar fields from learned skills. Keep bodies short and push detail into reference files |
| **Skill-creator** ([anthropics/skills](https://github.com/anthropics/skills/blob/main/skills/skill-creator/SKILL.md)) | Meta-skill: capture intent, interview, draft, test, review, revise; can extract a workflow from the conversation history | [primary] | The librarian's prompt follows the same steps, with the trajectory standing in for the conversation |
| **`claude plugin eval`** ([docs](https://code.claude.com/docs/en/plugin-evals)) | Runs a plugin against cases with graders (regex over the reply, whether a tool was called, a rubric judged by a second model) and compares with a no-plugin baseline; can gate CI | [primary] | A ready-made way to regression-test text skills at night, within a usage cap |
| **Claude CLI memory** ([docs](https://code.claude.com/docs/en/memory)) | CLAUDE.md files plus auto memory: Claude saves `user`, `feedback`, `project` and `reference` notes; the first 200 lines or 25 KB of `MEMORY.md` load each session | `autoMemoryEnabled`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, `autoMemoryDirectory` [primary]. Auto memory was on in our `-p` worker sessions (`memory_paths.auto` in the init event) [ran 10-20] | Turn auto memory off in every session Eileen starts, so all learning lives in Eileen's store where the user can see and forget it. Borrow the four note types for the user model |
| **Hermes Agent** ([skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills), [curator](https://hermes-agent.nousresearch.com/docs/user-guide/features/curator)) | The system prompt tells the agent to save a skill after a multi-step workflow worth repeating, after finding the working path past dead ends, or after a user correction. Skills capture "lessons, not logs" (no incident narration, dates, issue numbers or quoted chat), in SKILL.md with `metadata.hermes.*` | The curator runs when idle (every 168 h by default, after 2 h idle) in a forked agent. Phase 1 has no model: unused 14 days becomes stale, 30 days archived; pinned and cron-referenced skills skipped. Phase 2 (off by default) lets a model keep, patch, merge or archive, after a tar.gz snapshot with rollback. It never deletes; it tracks `view_count`, `use_count`, `patch_count`; every mutation goes into an append-only ledger [primary]. An independent test saw one skill created unprompted in a 14-tool-call run, but did not test reuse [secondary, codecut.ai, 2026-07-17] | Closest shipped design. Copy: the three save triggers, "lessons, not logs", usage counters, archive instead of delete, snapshot before any model edit, pinning. Improve on: no reuse evidence, no deterministic replay, no per-line provenance, no permission story |
| **Letta skill learning** ([blog](https://www.letta.com/blog/skill-learning/), 2025-12-02) | Reflection on a trajectory (solved? sound reasoning? repeated actions to abstract?), then a learning agent uses skill-creator to write the skill; skills are `.md` files managed with git | [primary] | Reflection before writing; git for history |
| **UFO2 / UFO3** ([repo](https://github.com/microsoft/UFO), [UFO2 paper](https://arxiv.org/abs/2504.14603)) | UFO2's knowledge substrate: help documents as JSON records (request, guidance) in a vector store; a summarization module "distills successful trajectories into reusable Example records" (task signature plus step plan), validated by a task evaluator; up to three past logs retrieved per task. The README lists "docs, demos, execution traces". UFO3 Galaxy (Nov 2025) adds multi-device orchestration with a task DAG | [primary]. The earlier attempt's paper fetch was cut off at the demonstration section, so how demonstrations are captured was not checked | Validate before archiving; retrieve few, relevant examples. UFO3 is the closest prior art for the later "everything around the user" connectors, but it does not describe learning |
| **OpenClaw and ClawHub** | Personal agent with a public skills registry | Over 230 malicious skills found on ClawHub and GitHub between 2026-01-27 and 02-01, posing as crypto trading and social media tools; their docs told victims to install a fake "AuthTool" that dropped stealers (NovaStealer on macOS; password-protected ZIPs on Windows) to take API keys, wallet keys, browser passwords, SSH keys and cloud credentials. The developers said they could not review submissions in time [secondary, ThaiCERT 2026-02-04]. Search results name the campaign ClawHavoc and report that VirusTotal scanning was added to ClawHub afterwards [secondary, titles only] | A public registry with weak review becomes a malware channel within weeks. Eileen ships no registry in v0.1; later packs are signed, reviewed and untrusted until verified locally |
| **Skills in the wild** ([arXiv 2601.10338](https://arxiv.org/abs/2601.10338), Jan 2026) | Scan of 31,132 skills from two marketplaces | 26.1% had at least one vulnerability across 14 patterns (prompt injection, data exfiltration 13.3%, privilege escalation 11.8%, supply chain); 5.2% high-severity patterns suggesting malice; skills with executable scripts 2.12x more likely to be vulnerable [primary, abstract] | Learned skills carry no scripts in v0.1. Recipes are data run by Eileen's own engine |
| **Paseo** ([skills](https://paseo.sh/docs/skills)) | Four orchestration skills: reference, handoff (task plus briefing on files, progress, decisions, constraints, acceptance criteria), committee, advisor | No learning feature described [primary] | The handoff briefing is the right shape for mid-recipe handoff (section 3.6) |
| **mem0** ([arXiv 2504.19413](https://arxiv.org/abs/2504.19413)) | Extracts, consolidates and retrieves salient facts from conversations; a graph variant | +26% relative over OpenAI's memory on LOCOMO (judge-scored), 91% lower p95 latency and over 90% fewer tokens than full context [primary, abstract] | A compact, retrieved user model beats stuffing history into the prompt. Eileen does this locally with no extra model in v0.1 |
| **Behavior caches** (muscle-mem, Butter) | Cache an agent's tool-call sequence and replay it for repeat tasks | [secondary] | Same idea as the recipe tier; neither has per-step policy checks or user control |
| **browser-use workflow-use** | Record a browser workflow once, run it deterministically, fall back to an agent when it breaks | [secondary] | Deterministic first, agent as fallback |
| **Power Automate Desktop self-healing** ([docs](https://learn.microsoft.com/en-us/power-automate/desktop-flows/self-healing), preview, 2026-06-30) | AI repair of UI or web actions that fail at runtime "allowing flows to continue instead of stopping on errors" | [primary] | Microsoft also treats recorded flows as brittle and adds model repair at the failure point |
| **Programming by demonstration and macro recorders** (SUGILITE, CHI 2017; AutoHotkey and recorders such as Pulover's Macro Creator) | Learn a script from one demonstration plus a spoken description that names the parameters; or record raw input | [secondary] | SUGILITE's lesson is that natural language tells you which parts of a demonstration are parameters. Raw recorders record coordinates and break when a window moves; Eileen records semantic tool calls |

### 2.3 What worked, what failed, what is safe

- **Worked:** programs or typed APIs over text alone (ASI, SkillWeaver); verifying before admitting a skill (Voyager, ASI, UFO2); online induction from real requests (AWM); versioned libraries applied on the next run (Interaction Traces); a curator that archives instead of deleting (Hermes); a stronger model writing skills that a cheaper path runs (SkillWeaver).
- **Failed or unproven:** coordinate macro recorders, which break on layout changes; whole-document rewrites of memory, which lose detail (ACE); agent-written skills with no reuse evidence (the Hermes test); open registries with weak review (ClawHub).
- **Safe:** skills as data with no scripts; per-step checks against the live state; every replayed action re-checked by the same policy as a live one; signatures for origin plus local verification for behavior; local storage by default.

## 3. Design

### 3.1 Terms

- **Job**: one unit of work in the job registry, run by a worker (route Claude in v0.1), by replay, or by the fast path.
- **Trajectory**: the redacted record of one job: the request, context, every tool call, the result, cost and the user's reaction.
- **Skill**: a native `SKILL.md` folder the Claude worker can load, with Eileen's bookkeeping under `metadata.eileen`.
- **Recipe**: an optional `recipe.json` beside a skill: a typed list of Eileen MCP calls with slots and pre and postcondition checks, run by Eileen's own engine with no model.
- **Librarian**: a low-priority background Claude worker that turns trajectories into skills and repairs broken ones.

### 3.2 Capture

**What is recorded per job.** The owner's direction lists the request, what was on screen, every tool call, the result, and whether the user was happy, corrected it or undid it. Eileen records:

| Field | Source | Notes |
|---|---|---|
| Request | Final transcript of the utterance that started the job, plus the brain's one-line task statement | Redacted |
| Context | Focused app and its version, active project or document identifier from the app hooks, active connectors | "What was on screen" is captured as structured app state (UIA and app-hook reads), not screenshots. Window titles are redacted |
| Tool calls | Every Eileen MCP call (logged at Eileen's dispatcher, which is authoritative) and every harness tool call from the worker's stream (`tool_use` and `tool_result` in stream-json) | Arguments and a truncated result summary; tier; approval asked and answer |
| Cost | The `result` event: `duration_ms`, `duration_api_ms`, `num_turns`, token usage including cache reads | These fields were present in every turn we drove [ran 10-20] |
| Outcome signals | Job status; then a regret window (120 s by default [inferred]) that listens for "no", "undo that", "that's wrong", a retry of the same request, an app-level undo seen by the hooks, or praise | Silence counts as weak acceptance, not success |
| Provenance | Each step is tagged `user`, `worker`, `tool`, `observed` (screen, web, file, notification text) or `librarian` | Drives the safety rules in section 3.11 |

**Redaction at write time.** Nothing reaches disk before the `Redactor` runs. It replaces API-key and token patterns, JWTs, private-key blocks, card numbers that pass a Luhn check, and values from UIA fields marked as passwords. Paths to vendor credential files are on a deny list for every session Eileen starts, so they are never read in the first place. Each trajectory stores a count of redactions, never the originals. Screenshots are not stored by default; when a skill depends on a frame (the Resolve face box), the trajectory keeps the derived numbers (box, sharpness score), not the image.

**Storage budget and retention [inferred].** Trajectories live in SQLite with zstd-compressed JSON bodies. A job of about 30 tool calls should compress to tens of kilobytes, so 100 jobs a day is a few megabytes. Defaults: 200 MB cap, 30-day retention for trajectories that no skill cites as evidence, evidence trajectories kept as long as the skill exists, the episodic ledger kept 14 days. When the cap is reached, the oldest uncited trajectories go first.

### 3.3 Distillation by the librarian

**When it runs.** Only when no foreground job is running and the user has been idle for a few minutes, or in a nightly batch, with a daily cap on librarian sessions to protect the owner's plan usage. This follows the sleep-time result and the Hermes curator's idle trigger.

**How it runs.** A Claude worker session started like the brain (warm stream-json, `--setting-sources ""`, `--strict-mcp-config`, `ENABLE_CLAUDEAI_MCP_SERVERS=false`, `--no-session-persistence`), plus:

- `--tools ""` and only Eileen's librarian MCP server: `trajectory_read`, `skill_list`, `skill_read`, `skill_propose`. No Bash, no file writes. The librarian can only write through `skill_propose`, which validates the schema, runs the linter (section 3.11) and rejects anything that fails. This is EvoSkill-GUI's restricted edit tool.
- `--settings '{"disableSkillShellExecution": true}'` and `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`.
- A mid-tier model alias by default and `haiku` for cheap pre-passes (is this job worth a skill?), both configurable.

**What it writes.**

1. A native `SKILL.md`: a short description that leads with the trigger phrases (the listing truncates at 1,536 characters; Eileen caps learned descriptions at 300), then When to use, Procedure (in terms of Eileen MCP tool names), Pitfalls (from failed attempts in the trajectory), and Checks. "Lessons, not logs": the linter rejects dates, job ids, quoted chat and file paths specific to one run. Provenance goes in `metadata.eileen`, not in the body. (The test skill in section 4 put "Learned by Eileen from job 01TEST" in the body; that is the mistake to avoid.)
2. Optional `reference/*.md` for facts the procedure needs (timeline start and frame rate conventions, a repo's test command). The worker reads these on demand; this was verified [ran 10-20].
3. Optional `recipe.json` when every step in the successful path is an Eileen MCP call with structured results.

**Parameter extraction.** The librarian marks spans of the request that became tool arguments ("15:30" became the `offset` argument; "the girl on the right" became `position: right`). With one trajectory it proposes slots from the request text alone; with two or more it confirms them by diffing the arguments. Slots are typed, and each type has a deterministic Rust resolver: `Offset` (spoken time to seconds, then to an absolute timecode in Rust, never in the model, per the app-hooks finding), `Count`, `Text`, `App`, `File` (resolved through the recent-work index), `Url`, `Position` (left, right, center, largest), `Choice`.

### 3.4 Retrieval

Two paths run side by side.

1. **Native.** Every worker session gets the learned-skills plugin. The model sees each skill's description in its listing and decides to invoke it, exactly as verified in section 4. No Eileen code is needed for this path.
2. **Local Rust matcher**, on every final utterance (end-of-turn event from the voice loop):
   - Stage A, templates with typed slots (`go to {t:time} in? resolve?`), matched by backtracking over normalized tokens with fillers dropped. A full match gives a skill id and filled slots.
   - Stage B, BM25 over each skill's name, description and triggers, used when no template matches, giving up to three hints above an absolute score floor.
   - Template matches can trigger replay (levels 3 and 4 below). BM25 hints never trigger anything; they only go into the brain's context as one line ("Known skills that may apply: ..."). The probe showed why: "export the render in premiere" drew three hints that shared the noun and app but had the wrong verb [re-ran 10-20].
   - An embedding stage is deferred past v0.1: it needs a model download and memory this PC does not have to spare.

### 3.5 The promotion ladder

| Level | Name | How a skill gets here | What it does | Default thresholds [inferred] |
|---|---|---|---|---|
| 0 | Trajectory | Every job | Evidence only | n/a |
| 1 | Draft | The librarian distilled it from a successful trajectory, and the linter passed | Loaded into every worker as a native skill; never replayed | 1 success with a clear accept signal or a passed postcondition |
| 2 | Trusted | Used successfully by workers | Listed first in hints; named to the brain; its recipe (if any) runs in shadow mode | 3 successes with at least 2 distinct slot values, no failure in the last 5 uses, no undo |
| 3 | Compiled recipe | Has a recipe whose shadow runs agreed with what the worker actually did, and the user approved once ("I can do this without asking Claude from now on. OK?") | After a template match and one warm brain turn to confirm intent, Eileen runs the recipe itself with checks after every step | 3 agreeing shadow runs; explicit approval |
| 4 | Spoken fast path | Proven recipe whose slots all resolve deterministically | Template match runs the recipe with no model call at all, the deterministic fast path the brain-latency report calls for | 10 clean replays, at least 95% clean over the last 20; announced to the user when it happens |

**Demotion.** A failed step check during replay hands off to a worker and counts as a failure. Two failures in the last five uses drop the skill one level. An undo or correction in the regret window counts as a failure. A version change in an app the recipe uses (Resolve 21.0.1 to 21.1, say) puts the recipe on probation: one shadow run before the next replay. A skill unused for 30 days is archived (Hermes' rule), never deleted automatically.

### 3.6 Replay with step checks and handoff

A recipe is a list of steps. Each step names an Eileen MCP tool, its arguments with `{"$slot": "t"}` references, preconditions, postconditions, a timeout and what to do on failure. Checks may only call read-only tools (tier 0), such as `resolve.timeline_info`, `smtc.session`, `uia.focused`, `fs.exists` or `measure.sharpness`. The replay engine:

1. Resolves slots in Rust. Any slot that cannot be resolved deterministically sends the job to a worker before any action.
2. For each step: run the preconditions; call the tool through the same dispatcher and policy gate a worker uses, with origin `Replay { skill, version }`; run the postconditions; publish a progress event (the HUD shows "replaying resolve-sharpen-face v3, step 4 of 7").
3. The user can interrupt between any two steps, and "stop" cancels at once.
4. On the first failed check, stop, publish `ReplayHandoff`, and start a Claude worker job with a briefing in the Paseo handoff shape: the request, the skill and version, steps done with their results, the failed check with the observed state, the steps left, and the instruction to continue from the current state, not restart. The worker loads the same skill natively, so it knows the plan.
5. When the worker recovers, its trajectory goes to the librarian as a repair candidate.

### 3.7 Repair and versioning

- Each skill folder is its own small git repository inside the plugin folder (`skills/<name>/.git`). A repo per skill makes "forget" complete (delete the folder and the history goes with it) and makes exporting a single skill trivial. Commits are written with `gix` (pure Rust, MIT or Apache-2.0); if its commit API proves too thin on Windows, fall back to the git CLI when installed (open risk, section 7).
- Every commit message records the reason (`distill`, `repair`, `user-edit`, `rollback`, `promote`, `demote`) and the trajectory ids used as evidence.
- Repair: the librarian gets the current skill, the failed replay or worker trajectory, the successful recovery and the skill's history, and proposes a new version through `skill_propose`. The new version starts one level below the old one and climbs again on evidence. The old version stays in history.
- Rollback: "use the old version" or a HUD click checks out the previous version as a new commit, so history only grows.
- Before any model-driven consolidation pass, snapshot the plugin folder (Hermes' rule).

### 3.8 The user model and the episodic desktop ledger

**User model.** Small itemized facts, each with provenance, a source job id, created and last-confirmed times, and a confidence, in four kinds borrowed from the claude CLI's auto memory: `user` (role, preferences), `feedback` (corrections and confirmed approaches), `project` (Resolve projects start at 01:00:00:00 at 24 fps; this repo's tests run with `cargo test`), `reference` (where things live). Updates are deltas (add, confirm, retire), never whole rewrites (ACE). Phrasing maps are facts too: "the video editor" means Resolve.

**Episodic desktop ledger.** A timeline built from Eileen's own events: jobs, app-hook events and sampled focus changes ("14:02 opened beach.psd", "14:20 render started", "14:31 tried the sharpen node, undone"). It answers "go back to what we tried an hour ago" and resolves "that" in "forget that". It is built from structured events, not screen text, and kept 14 days by default.

**Per-turn token budget [inferred].** The brain's turn gets at most about 600 tokens from learning: up to 3 skill hints (about 40 tokens each), up to 8 user-model facts relevant to the active app (about 25 each) and up to 6 ledger lines from the last hour. The block goes in the user message after the cached prefix, never in the system prompt, so it does not break prompt caching. (A near-empty worker turn already read about 20,000 cached tokens [ran 10-20], so the learning block is small next to it.)

### 3.9 User control by voice and HUD

| Say | Eileen does |
|---|---|
| "What have you learned?" / "...today?" / "...about Resolve?" | Speaks the newest two or three items in one sentence each; the HUD shows the full list with level, success rate, time saved and last use |
| "Show me how you do that" | HUD shows the skill's steps and checks; for a recipe, each step with its tool and tier; each line colored by provenance |
| "Forget that" | Resolves "that" through the ledger, confirms once ("Forget the Resolve sharpen skill and its four recordings?"), then deletes the skill folder with its history, purges its evidence trajectories and writes a tombstone so the librarian does not relearn it from the same jobs |
| "Don't learn from this" / "Stop learning" / "Start learning again" | Marks the current job no-learn; pauses or resumes capture; the HUD shows a learning indicator next to the mic indicator |
| "That's wrong" / "Undo that" | Failure signal on the current or last job |
| "Yes, keep that" / "Approve" | Approves a pending draft or a promotion to level 3 |
| "Use the old version" | Rollback |
| "How much time have you saved me?" | Sums time saved across skills this week |

The HUD Learning panel adds a queue of drafts and promotions waiting for approval, a diff between versions, a per-line provenance view, pin, forget and (later) export.

### 3.10 Measurement

Per skill, from trajectories: uses, successes, failures, undos, handoffs; median duration, model turns and tokens for the first runs of that request kind before the skill existed (its evidence trajectories) against the median of the last 10 uses. Time saved is the difference times the number of uses. These numbers drive the ladder, the HUD and the spoken summaries. Text skills also get nightly regression cases built from their evidence, run with `claude plugin eval` against a no-plugin baseline, within the librarian's daily cap.

### 3.11 Safety

1. **No tier escalation.** A recipe step carries no permission of its own. At replay, the policy engine checks every call exactly as if a worker made it. "Always allow" grants are never written into skills.
2. **No skipped confirmations.** A replayed step that needs confirmation raises the same `ApprovalRequested` event and waits. Denial ends the replay with no call made.
3. **Screen text never becomes an instruction without approval.** Lines with `observed` provenance can enter a skill only as quoted data in a reference file, and only after the user approves that line in the HUD. The librarian receives observed content wrapped and labeled as data. The linter rejects any procedure line traced to observed content without an approval record.
4. **Frontmatter allowlist.** Learned skills may use only `name`, `description`, `when_to_use`, `arguments`, `argument-hint`, `disable-model-invocation`, `user-invocable` and `metadata`. The linter strips `allowed-tools` (it grants tool use without asking), `hooks`, `shell`, `model`, `effort`, `context`, `agent` and `paths`.
5. **No shell, no scripts.** Every session Eileen starts sets `disableSkillShellExecution`. The linter also rejects any `` !` `` sequence and any `scripts/` folder in learned skills, since the setting's placeholder behavior was not confirmed on this PC (section 4).
6. **No credentials.** Redaction at write time, password fields never captured, vendor credential paths denied, and any skill text matching a secret pattern rejected.
7. **Auto memory and session transcripts off** for all Eileen sessions (`CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, `--no-session-persistence`), so nothing Eileen learns hides in `~/.claude/projects`.
8. **Provenance per line**, kept in `metadata.eileen.provenance` as a map from line hash to source, and shown in the HUD.

### 3.12 Connectors beyond the PC

The loop does not know about apps. Trajectories record connector-qualified tool names (`pc.resolve.goto_offset`, later `phone.notify`, `home.light.set`), recipes call them, and checks use each connector's read-only tools. A phone or Home Assistant connector joins the loop by exposing tools with tiers and read-only checks; no learning code changes. Room-sensing connectors (cameras, extra microphones) contribute only derived events to trajectories, never raw audio or images, and only on devices the user opted in.

### 3.13 Community skill packs, later

Not in v0.1. When added: a pack is SKILL.md folders plus recipes plus a manifest naming the Eileen MCP tools and tiers it needs, signed with ed25519. Installed packs land at level 1 marked untrusted: never auto-loaded until the user reviews them, never replayed until local shadow runs pass, scripts rejected. Export strips personal data (paths become placeholders; user-model facts and evidence trajectories are never exported). ClawHub showed that signatures prove origin, not safety.

## 4. What was verified on this PC

All rows except the last two come from the earlier attempt on 2026-10-20. Driver scripts: `drive.py` (two model sessions), `probe.py` (discovery with no model call: start a session, send nothing, read the debug log, kill it), `probe_reload.py` (hot reload). Logs in `logs/`. All sessions ran with `--strict-mcp-config` and an empty MCP config, `ENABLE_CLAUDEAI_MCP_SERVERS=false`, `--no-session-persistence`, `--tools Skill,Read` and `--model haiku` (which resolved to `claude-haiku-5-5`). The PC was loaded with other Claude processes, so the startup times are pessimistic, in line with the 39 to 48 s cold starts in [2026-10-brain-latency.md](2026-10-brain-latency.md).

| # | Check | Result |
|---|---|---|
| 1 | `claude plugin validate` on a skills folder and on a local plugin | Both passed (21.2 s and 5.3 s; one warning for missing author) [ran 10-20] |
| 2 | Skills in the cwd's `.claude/skills`, `--setting-sources project` or default (p1, p2) | 1 project skill loaded; watcher on the folder; skill list ready 22 to 24 s after spawn [ran 10-20] |
| 3 | `--add-dir <store>` with `--setting-sources ""` (p3) | **0 skills loaded**, although the folder was watched [ran 10-20]. Matches the docs: `--add-dir` loads "depend on the `project` setting source" [primary] |
| 4 | `--add-dir <store>` with `project` source or default (p6, p7) | 1 skill loaded as "additional" [ran 10-20] |
| 5 | `--plugin-dir <plugin>` with `--setting-sources ""` (p4) | 1 plugin skill loaded [ran 10-20] |
| 6 | `--plugin-dir <parent folder>` (p8) | The CLI treated it as "a folder of plugins" and loaded the one inside; 1 plugin skill; 10.8 s [ran 10-20] |
| 7 | Worker-style model session: empty cwd, `--setting-sources ""`, `--plugin-dir` and `--add-dir` on the plugin, `disableSkillShellExecution: true`, `--permission-prompts none`. Asked "Go to 15:30 in my Resolve timeline. What absolute timecode do I pass?" | Init 34.5 s after spawn listed `eileen-learned:resolve-goto-offset`. The model called `Skill` with args `"15:30"`, the body arrived with "Base directory for this skill", the model read `reference/rates.md` from the skill folder, then answered `TC=01:15:30:00` with the skill's marker. Turn 16.9 s (API 9.8 s), 4 model turns, 20,582 cache-read tokens, 907 output [ran 10-20] |
| 8 | Same warm process: a new skill (`launch-agents-fanout`) written into the plugin's `skills/` 3 s before turn 2 | **Not picked up.** Turn 2's skill list did not include it and the model said it could not plan. The session's watcher covered only `~/.claude/skills`, not the plugin folder [ran 10-20] |
| 9 | Project-source session with `--add-dir` (no plugin). Asked for 1:02:10 | Skill listed without namespace (`resolve-goto-offset`); invoked with `"1:02:10"`; read the reference; answered `TC=02:02:10:00`. Turn 13.9 s [ran 10-20] |
| 10 | Arguments with no placeholder in the body | Appended as `ARGUMENTS: 15:30`, as documented [ran 10-20, primary] |
| 11 | A line ``PROBE=!`echo SHELL_RAN_7Q2` `` in the skill body, with shell execution disabled (row 7) and at its default (row 9) | **Inconclusive.** In both sessions the line reached the model unchanged: neither the command output nor the documented placeholder. Bash was not in the tool list and prompts were auto-denied in both, and the `!` was glued to `PROBE=`, so we cannot tell which of these stopped the expansion. Eileen does not rely on it (section 3.11) [ran 10-20] |
| 12 | Hot reload of `--add-dir` skills (`--setting-sources project`) | Watcher up 111 s after spawn on the loaded PC. A new skill folder was detected ("Detected skill change") and the set reloaded to 2 skills within a few seconds. Right after, the log showed "idle, switching poll interval to 30000ms", so later changes may take up to 30 s to appear [ran 10-20] |
| 13 | What a worker inherits | The init event showed `entrypoint: claude-desktop` and 38 bundled skills (inherited from the parent process environment), and `memory_paths.auto` set, so auto memory was on [ran 10-20] |
| 14 | Rust matcher, std only, release build (first build 1 min 28 s, incremental 21 s) | Earlier attempt, 50 skills: p50 3.9 to 7.3 us, p99 62 to 86 us across two versions [ran 10-20] |
| 15 | Same binary, run once during this write-up | 50 skills: index 1.3 ms, p50 4.9 us, p99 58.8 us. 500: index 4.6 ms, p50 3.7 us, p99 86.2 us. 5,000: index 37.5 ms, p50 5.6 us, p99 748 us, max 816 us. All 7 positive utterances matched with correct slots: "Open Resolve and go to 15:30" and "uh go to fifteen thirty in resolve" gave `t = 930 s`; "jump to 1:02:10 in Resolve" gave 3,730 s; "play lofi hip hop radio on YouTube at 2:15" gave video text plus 135 s; "play the cat video at 2 minutes 15 seconds on youtube" gave 135 s; "launch three agents to fix the flaky login tests" and "have 3 agents add input validation to the parser" gave `n = 3` and the task text. "make the face of the girl on the right less blurry" and "what's the weather tomorrow" gave nothing (correct, no such skill). "export the render in premiere" gave three wrong-verb hints [re-ran 10-20] |
| 16 | Skills docs page re-read | Frontmatter fields, 1,536-character listing cap, shell setting, `--add-dir` dependence on the project source, live detection rules, compaction re-attach budget (section 2.2) [primary, re-ran 10-20] |

**Not verified anywhere yet:** a recipe replay, the git-per-skill store, the librarian writing a skill from a real trajectory, the regret-window signals, and the Resolve face fix end to end. The "skill store prototype" from the earlier attempt is the on-disk layout used above (a skills folder and a local plugin); no Rust store code was written.

**What this decides.** Workers get learned skills through `--plugin-dir` with `--setting-sources ""`, which loads them without the user's or project's settings and namespaces them as `eileen-learned:<name>`, making skill use easy to spot in the stream. Since plugin skills are fixed at session start, Eileen recycles its hot spare session after a skill changes (start a fresh spare, retire the old one) rather than relying on reload. `--add-dir` with the project source is the alternative when mid-session reload matters, at the cost of loading the cwd's project settings.

## 5. Worked examples

Durations below are expectations, not measurements [inferred]. The first-run versus second-run numbers are exactly what section 3.10 will measure.

### 5.1 "Open Resolve, go to 15:30 and make the face of the girl on the right less blurry"

**First run.** No template matches and BM25 finds nothing (row 15). The brain starts a Claude worker with Eileen's MCP tools. Using the hooks in the app-hooks report, it ensures Resolve is running, switches to the Color page, calls the bridge's `goto_offset` with 930 s (Rust turns that into `01:15:30:00` from the timeline's start and frame rate), exports the current frame as a still, runs the local face detector and takes the right-most face. Face Refinement needs an Analyze click with no default shortcut, so it falls back to applying a DRX grade with a circular window plus sharpening at the face box, tracks forward with the Color page shortcut, and compares sharpness inside the box before and after. This means many model turns and minutes of work, with narration along the way. The user says "nice", so the trajectory closes as a success.

**Librarian, at idle.** It writes `resolve-sharpen-face` with slots `t: Offset`, `which: Position`, `strength: Choice` (default medium). Pitfalls: Resolve rejects a timecode before the timeline start with a bare `False`; Face Refinement's Analyze cannot be scripted; Fusion writes may not render. The draft recipe is: `resolve.ensure_running`, `resolve.open_page(color)`, `resolve.goto_offset(t)` (post: playhead equals start plus t), `resolve.export_still` then `face.detect` (post: at least one face), `face.pick(which)`, `resolve.apply_grade_drx(sharpen_window, box)` (tier 1, adds a node, reversible; post: node count up by one), `resolve.track_forward` (post: tracker data present), `measure.sharpness(box)` (post: at least 15% higher [inferred]). Level 1.

**Second run**, a day later: "go to 42:10 and sharpen the guy on the left". The phrasing differs, so the template may miss, but BM25 hints the skill and the worker's listing has `eileen-learned:resolve-sharpen-face`. The worker invokes it and follows the procedure with no exploration: fewer turns, no dead ends. After three such successes with different times and faces, the skill becomes trusted and its recipe runs in shadow. Once the user approves, a later request is confirmed in one brain turn and replayed by Eileen. If the face leaves the frame and `track_forward`'s check fails at step 7, Eileen hands off to a worker with the briefing, and the worker continues from the graded frame.

**Verified so far:** only the timecode piece. A worker used a learned skill with a reference file to turn "15:30" into `01:15:30:00` (row 7). Resolve scripting itself is proven in the app-hooks report.

### 5.2 "Play <video> at <time>"

**First run.** The worker resolves the video: from the conversation or the recent-work index if it was played before, otherwise a YouTube search through the browser extension. It opens `watch?v=<id>&t=<seconds>s` with `ShellExecute`, waits for the SMTC session to show the expected title (handling a preroll ad), and checks the position.

**Learned.** `youtube-play-at` with slots `video: Text` and `t: Offset`, plus a recipe: `video.resolve(video)` (deterministic when the ledger or index knows the URL), `shell.open_url`, `smtc.wait_title(contains)` (post: title matches), `smtc.position` (post: within 3 s of t after any ad).

**Second run.** The template `play {video} on youtube at {t:time}` matches with both slots filled (row 15). If the video is known, this reaches the spoken fast path quickly: no model call, just the URL launch and SMTC checks. If the video is new, the recipe's `video.resolve` step cannot resolve deterministically, so that single step goes to a worker (or one brain turn for the search), and the recipe continues with the URL.

### 5.3 "Launch three agents to <coding task>"

**First run.** The brain plans the split (three Claude workers competing on the same task, or partitioned by area), creates three git worktrees and branches, opens Windows Terminal panes (`wt -w 0 split-pane`) so the user can watch, starts three jobs with the task and acceptance criteria, narrates progress ("two of three agents pass the tests"), and helps the user pick or merge. Corrections become user-model facts ("make them compete, don't split the task"; "this repo tests with `cargo test`").

**Learned.** `launch-agents-fanout` with slots `n: Count` and `task: Text`, and a recipe for the orchestration only: create n worktrees with the user's naming, open panes, and start n worker jobs with a briefing the skill templates. The coding itself is always a worker; it is never compiled.

**Second run.** The template matches with `n = 3` and the task text (row 15). The scaffolding runs as a recipe in seconds with no planning turn, and the three workers each load the skill and the repo's facts. One lesson from row 8 applies here: a skill learned during the session reaches workers only in sessions started after it was written, which is fine because each agent is a new session.

## 6. The v0.1 implementation slice

### 6.1 Crates

The scratch Rust workspace (October 11 snapshot and its successor) has `eileen-contracts`, `eileen-core` (bus, SQLite `JobStore`, supervisor), `eileen-mcp` (rmcp server with `jobs_list`), `eileen-harness`, `eileen-shell` and `eileen-cli`. Memory on this PC is tight, so the slice adds one crate, not five:

- `eileen-contracts::learn`, a new module: all serde and schemars types below, so the MCP schema and the HUD share one source.
- `eileen-learn`, a new crate with modules `capture`, `redact`, `store` (rusqlite, already in the workspace), `skills` (plugin folder, per-skill git through `gix`), `lint`, `matcher` (std only, as probed), `ladder` (pure functions), `replay`, `librarian`, `usermodel`, `ledger`.

Two notes for whoever merges this. The scratch `Route` enum still has a `Gemini` variant, which the 2026-10-20 direction removes; the learning code must not reference it. And `gix` and `zstd` are new dependencies that need to clear `cargo-deny`.

### 6.2 Types

```rust
// eileen-contracts/src/learn.rs
use crate::{JobId, JobStatus, Route};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Agent Skills name: 1 to 64 chars, lowercase letters, digits, hyphens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SkillId(pub String);

/// Connector-qualified MCP tool name, e.g. "pc.resolve.goto_offset".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ToolName(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Level { Draft, Trusted, Recipe, FastPath }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provenance {
    User,
    Worker { route: Route },
    Tool { tool: ToolName },
    Observed { source: ObservedSource },
    Librarian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObservedSource { Screen, Web, File, Notification }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobOrigin { Worker, Replay, FastPath }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Trajectory {
    pub schema: u16,
    pub job_id: JobId,
    pub request: String,              // redacted final transcript
    pub task: String,                 // brain's one-line statement
    pub context: DesktopContext,
    pub route: Route,
    pub origin: JobOrigin,
    pub skill: Option<SkillRef>,
    pub steps: Vec<TrajectoryStep>,
    pub cost: Cost,
    pub outcome: Option<Outcome>,
    pub redactions: u32,
    pub no_learn: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopContext {
    pub focused_app: Option<String>,
    pub app_versions: Vec<(String, String)>,
    pub document: Option<String>,     // app-hook id, redacted
    pub connectors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrajectoryStep {
    pub at_ms: u64,
    pub tool: ToolName,
    pub args: serde_json::Value,      // redacted
    pub ok: bool,
    pub result: serde_json::Value,    // redacted, truncated summary
    pub tier: u8,
    pub approval: Option<bool>,       // asked and answered yes or no
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Cost { pub duration_ms: u64, pub api_ms: u64, pub model_turns: u32, pub tokens_in: u64, pub tokens_out: u64, pub tokens_cache_read: u64 }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Signal {
    Accepted, Praised, Corrected { text: String }, Undone { after_ms: u64 },
    Retried, Cancelled, PostcheckPassed, PostcheckFailed { step: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict { Success, Failure, Unclear }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Outcome { pub status: JobStatus, pub signals: Vec<Signal>, pub verdict: Verdict }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkillRef { pub id: SkillId, pub version: u32 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotKind { Offset, Count, Text, App, File, Url, Position, Choice { options: Vec<String> } }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SlotSpec { pub name: String, pub kind: SlotKind, pub required: bool, pub default: Option<serde_json::Value> }

/// Lives in SKILL.md frontmatter under metadata.eileen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillMeta {
    pub id: SkillId,
    pub version: u32,
    pub level: Level,
    pub description: String,
    pub triggers: Vec<String>,        // "go to {t:time} in? resolve?"
    pub slots: Vec<SlotSpec>,
    pub apps: Vec<(String, String)>,  // app, version range the recipe was proven on
    pub has_recipe: bool,
    pub evidence: Vec<JobId>,
    pub pinned: bool,
    pub untrusted_origin: bool,       // community pack, later
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Recipe { pub skill: SkillRef, pub pre: Vec<Check>, pub steps: Vec<RecipeStep> }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecipeStep {
    pub id: String,
    pub tool: ToolName,
    pub args: serde_json::Value,      // {"$slot": "t"} references
    pub pre: Vec<Check>,
    pub post: Vec<Check>,
    pub timeout_ms: u32,
    pub on_fail: OnFail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnFail { Handoff, RetryOnce, Abort }

/// Checks may only call tier-0 (read-only) tools; the linter enforces it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Check { pub tool: ToolName, pub args: serde_json::Value, pub path: String, pub expect: Expect }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", content = "value", rename_all = "snake_case")]
pub enum Expect { Equals(serde_json::Value), Contains(String), AtLeast(f64), Within(f64), Exists, Truthy }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UserFact {
    pub id: String,
    pub kind: FactKind,
    pub text: String,
    pub provenance: Provenance,
    pub source_job: Option<JobId>,
    pub confidence_pct: u8,
    pub created_ms: u64,
    pub confirmed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FactKind { User, Feedback, Project, Reference }
```

### 6.3 Traits

```rust
// eileen-learn/src/lib.rs (signatures only)
pub trait Redactor: Send + Sync {
    /// Rewrites secrets in place; returns how many were replaced.
    fn redact(&self, value: &mut serde_json::Value) -> u32;
}

pub trait Recorder: Send + Sync {
    fn begin(&self, job: &Job, request: &str, task: &str, ctx: DesktopContext) -> Result<(), LearnError>;
    fn step(&self, job: JobId, step: TrajectoryStep) -> Result<(), LearnError>;
    fn signal(&self, job: JobId, signal: Signal) -> Result<(), LearnError>;
    fn finish(&self, job: JobId, status: JobStatus, cost: Cost) -> Result<(), LearnError>;
}

pub trait SkillRepo: Send + Sync {
    fn list(&self) -> Result<Vec<SkillMeta>, LearnError>;
    fn read(&self, id: &SkillId) -> Result<SkillDoc, LearnError>;          // SKILL.md, references, recipe
    fn commit(&self, doc: SkillDoc, reason: CommitReason) -> Result<SkillRef, LearnError>; // lint runs here
    fn history(&self, id: &SkillId) -> Result<Vec<SkillVersion>, LearnError>;
    fn rollback(&self, id: &SkillId, to: u32) -> Result<SkillRef, LearnError>;
    fn forget(&self, id: &SkillId) -> Result<ForgetReport, LearnError>;    // folder, history, evidence, tombstone
}

pub trait Matcher: Send + Sync {
    fn rebuild(&mut self, skills: &[SkillMeta]);
    fn best(&self, utterance: &str) -> MatchResult;
}
pub enum MatchResult { Template { skill: SkillRef, slots: SlotValues }, Hints(Vec<(SkillRef, f32)>), Nothing }

/// The same dispatcher and policy gate workers use. Replay never bypasses it.
pub trait ToolDispatch: Send + Sync {
    fn call(&self, origin: CallOrigin, tool: &ToolName, args: serde_json::Value)
        -> impl Future<Output = Result<serde_json::Value, ToolError>> + Send;
}
pub enum CallOrigin { Brain, Worker { job: JobId, route: Route }, Replay { job: JobId, skill: SkillRef } }

pub enum ReplayOutcome { Completed, Handoff(HandoffBrief), Cancelled, Denied { step: String } }

/// Pure function, table-tested: what the evidence says the level should be now.
pub fn next_level(meta: &SkillMeta, recent: &[UseRecord], policy: &LadderPolicy) -> Transition;
```

`LadderPolicy` holds the thresholds from section 3.5 with those defaults, so they can be tuned from measurement without code changes.

### 6.4 Hook points

| Where | What hooks in |
|---|---|
| Job registry, `JobStore::upsert` on a new job | `Recorder::begin` with the utterance, task line, desktop context and any matched skill. `Job` gains `origin: JobOrigin` and `skill: Option<SkillRef>` |
| Eileen MCP dispatcher | Every call, from any origin, becomes a `TrajectoryStep` after redaction. This is the authoritative log for Eileen's own tools |
| Harness adapter (Claude route) | Parses stream-json `tool_use` and `tool_result` into steps for harness-native tools, and the `result` event into `Cost`; spawns workers with `--plugin-dir <store>\eileen-learned`, `disableSkillShellExecution`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, `--no-session-persistence` and a constructed environment, not the parent's |
| Event bus | New `JobEvent::ToolCalled { job_id, step }` and `JobEvent::UserSignal { job_id, signal }`; new `BusEvent::Learn(LearnEvent)` with `SkillDrafted`, `SkillPromoted`, `SkillDemoted`, `ReplayStep`, `ReplayHandoff`, `Forgot`, consumed by the HUD and the voice loop |
| `TaskFinished` | Opens the regret window; the voice loop and app hooks feed `UserSignal`s; then `Recorder::finish` sets the verdict |
| Voice loop end-of-turn | `Matcher::best` on the final transcript; template match at level 3 or 4 goes to the replay engine, hints go into the brain's turn block (section 3.8) |
| Idle detector | Schedules the librarian within its daily cap |
| Skill change | Rebuilds the matcher index and recycles the hot spare worker session (row 8) |

On-disk layout, under `%LOCALAPPDATA%\eileen\learning\`: `learn.sqlite` (trajectories, uses, stats, ledger, user facts, tombstones), `eileen-learned\.claude-plugin\plugin.json`, `eileen-learned\skills\<name>\{SKILL.md, recipe.json, reference\, .git\}`, and `pending\` for drafts waiting on approval (not loaded by workers).

### 6.5 Acceptance tests

CI-runnable, no model needed:

1. **Capture.** A fake harness emits five tool calls and a `result` event; the trajectory has five ordered steps, the cost fields and, after a simulated regret window with no signal, a verdict of `Unclear`. With "that's wrong" inside the window, the verdict is `Failure`.
2. **Redaction at write time.** A tool result containing an `sk-ant-` style key, a JWT, a Luhn-valid card number and a UIA password value is recorded; none of the raw strings appears anywhere in the bytes of `learn.sqlite`.
3. **Storage budget.** Inserting synthetic trajectories past 200 MB prunes the oldest uncited ones and keeps every trajectory cited as evidence.
4. **Linter.** Drafts are rejected, each with a specific error, for: `allowed-tools`, `hooks` or `shell` in the frontmatter; a `` !` `` sequence; a `scripts/` folder; a procedure line with `observed` provenance and no approval record; a secret-like string; a description over 300 characters; a date or job id in the body; a check that calls a tool above tier 0.
5. **Versioning.** Commit v1 and v2, roll back to v1 (creates v3 equal to v1), history lists three; `forget` removes the folder with its repository and evidence; the librarian then refuses those trajectory ids because of the tombstone.
6. **Matcher golden file.** The ten utterances of row 15 give exactly those results; p99 under 1 ms with 5,000 skills (loose bound for CI runners).
7. **Ladder.** Table-driven sequences of uses give the expected promotions and demotions, including probation after an app version change and archive after 30 idle days. A pinned skill is never archived.
8. **Replay checks and handoff.** With a mock dispatcher whose step-3 postcondition fails: steps 1 and 2 ran, step 4 never ran, a `ReplayHandoff` event carries a brief with steps 1 and 2 results and the failed check, and one worker job is queued with that brief.
9. **No escalation, no skipped confirmation.** A recipe step on a tier-3 tool raises `ApprovalRequested` and waits; a denial gives `ReplayOutcome::Denied` and the dispatcher records no call.
10. **User control.** "forget that" resolves to the last-used skill through the ledger and asks once; "what have you learned" returns at most three items for speech and the full list for the HUD.
11. **Turn budget.** With 5,000 skills and 500 facts, the learning block for one turn stays under 600 tokens.

Manual, on the owner's PC, with the signed-in CLI (not in CI, since CI has no subscription):

12. **Native load through Eileen.** Repeat row 7 with a worker spawned by `eileen-harness` against the real store: the init event lists `eileen-learned:<skill>` and the model invokes it. Repeat row 11 with a correctly spaced `` !`echo x` `` line to confirm the documented placeholder appears.
13. **First run against second run** for each worked example in section 5: record duration, model turns and tokens for both runs, and check that the second is lower on all three. This is the owner-facing proof that Eileen learns.

## 7. Pitfalls

- `--add-dir` skills do not load with `--setting-sources ""`; they need the project source (row 3).
- Skills under `--plugin-dir` are fixed when the session starts; recycle the session after a change (row 8).
- `--add-dir` hot reload works, but the watcher drops to a 30 s poll when idle (row 12).
- Startup took 11 to 111 s on the loaded PC; never start a session per utterance (rows 2 to 12, and the brain-latency report).
- Plugin skills are namespaced (`eileen-learned:x`), add-dir skills are not; the matcher and stats must accept both forms (rows 7, 9).
- With no placeholder, arguments are appended as `ARGUMENTS: <value>`; declare named `arguments` instead (row 10).
- Sessions inherit the parent's environment (here `entrypoint: claude-desktop` and 38 bundled skills); build the worker environment explicitly (row 13).
- Auto memory is on by default in `-p` sessions; turn it off (row 13).
- The skill listing truncates descriptions at 1,536 characters, and an invoked body stays in context for the session; keep bodies short and push detail into reference files.
- `allowed-tools` in frontmatter grants tools without asking; it must never appear in a learned skill.
- Lexical matching gives near-miss hints with the wrong verb; hints inform, never act (row 15).
- Agent-written skills drift toward logs (dates, job ids, quoted chat); lint for "lessons, not logs".
- `claude plugin validate` takes 5 to 21 s; run it in the librarian, never on the hot path (row 1).
- The scratch `Route` enum still lists Gemini.

## 8. Open risks

- **Plan usage.** How much of the owner's plan the librarian and nightly evals use is unmeasured. Start with a small daily cap and measure.
- **CLI behavior is not a stable API.** Skill discovery flags, namespacing and watcher behavior were observed on 2.1.304. Re-run the probe scripts after every CLI update.
- **Codex compatibility.** Secondary sources say Codex reads SKILL.md; unverified here because Codex is not installed. Check when the owner installs it.
- **Recipes for pro-app edits.** Whether the Resolve face fix compiles into a reliable recipe is untested. Tracking and Face Refinement's Analyze have no API, so that recipe may stay at level 2 with a worker in the loop.
- **Success signals.** Silence is weak evidence; app-level undo detection exists only where the hooks can see it. Wrong signals could promote a bad skill. Mitigation: promotion to level 3 needs explicit approval.
- **Prompt injection through trajectories.** Observed text inside tool results reaches the librarian. Provenance, wrapping and the linter reduce the risk but do not remove it.
- **`gix` commit support on Windows** is not yet exercised; the fallback is the git CLI, which a user may not have.
- **Vendor-side retention.** Eileen can delete everything it stores locally, but not what a vendor retains from model calls. The HUD's privacy note must say so plainly.
- **Matcher scope.** Templates are English-only and hand-shaped by the librarian; paraphrase coverage depends on the native path and BM25 until an embedding stage is affordable.

## Sources

Papers (arXiv abstract pages): [Voyager 2305.16291](https://arxiv.org/abs/2305.16291), [AWM 2409.07429](https://arxiv.org/abs/2409.07429), [SkillWeaver 2504.07079](https://arxiv.org/abs/2504.07079), [ASI 2504.06821](https://arxiv.org/abs/2504.06821), [ICAL 2406.14596](https://arxiv.org/abs/2406.14596), [AgentRR 2505.17716](https://arxiv.org/abs/2505.17716), [Cradle 2403.03186](https://arxiv.org/abs/2403.03186), [UFO2 2504.14603](https://arxiv.org/abs/2504.14603), [CUA-Skill 2601.21123](https://arxiv.org/abs/2601.21123), [Interaction Traces to Persistent Skills 2609.04869](https://arxiv.org/abs/2609.04869), [EvoSkill-GUI 2609.17653](https://arxiv.org/abs/2609.17653), [Work2Skill 2610.12984](https://arxiv.org/abs/2610.12984), [Agentic Skills survey 2608.29596](https://arxiv.org/abs/2608.29596), [Skills in the Wild 2601.10338](https://arxiv.org/abs/2601.10338), [Sleep-time Compute 2504.13171](https://arxiv.org/abs/2504.13171), [ACE 2510.04618](https://arxiv.org/abs/2510.04618), [ReasoningBank 2509.25140](https://arxiv.org/abs/2509.25140), [mem0 2504.19413](https://arxiv.org/abs/2504.19413).

Docs and repositories: [claude CLI skills](https://code.claude.com/docs/en/skills), [claude CLI memory](https://code.claude.com/docs/en/memory), [plugin evals](https://code.claude.com/docs/en/plugin-evals), [Agent Skills specification](https://agentskills.io/specification), [skill-creator](https://github.com/anthropics/skills/blob/main/skills/skill-creator/SKILL.md), [Hermes skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills), [Hermes curator](https://hermes-agent.nousresearch.com/docs/user-guide/features/curator), [Letta skill learning](https://www.letta.com/blog/skill-learning/), [microsoft/UFO](https://github.com/microsoft/UFO), [Paseo skills](https://paseo.sh/docs/skills), [Power Automate self-healing](https://learn.microsoft.com/en-us/power-automate/desktop-flows/self-healing).

Secondary: [ThaiCERT on malicious OpenClaw skills, 2026-02-04](https://www.thaicert.or.th/en/2026/02/04/researchers-discover-over-200-malicious-skills-on-openclaw-distributing-password-stealing-malware/), [codecut.ai test of Hermes skill writing, 2026-07-17](https://codecut.ai/hermes-autonomous-skills-curator/), [muscle-mem on PyPI](https://pypi.org/project/muscle-mem/), [browser-use workflow-use](https://github.com/browser-use/workflow-use), [SUGILITE development repository](https://github.com/tobyli/Sugilite_development). Search-result titles only, not fetched: the ClawHavoc analysis, the Snyk ToxicSkills report, and VirusTotal scanning on ClawHub.

On-PC artifacts: `%USERPROFILE%\Projects\eileen-workfiles\2026-10-scratch\learning\` (drive.py, probe.py, probe_reload.py, the skill folders, logs, the matcher source). The condensed notes of the earlier attempt are in `%USERPROFILE%\Projects\eileen-workfiles\predecessor-notes\learning-loop.md`.
