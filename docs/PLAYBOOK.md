# Engineering playbook

How this repository is built, written so that another project can adopt the same practices. Eileen is the worked example throughout. The owner asked for this document on 2026-10-20 ("A reusable, fully cited engineering playbook" in [OWNER-DIRECTION.md](OWNER-DIRECTION.md#2026-10-20)); [D50](DECISIONS.md#d50-a-reusable-fully-cited-engineering-playbook) records the decision and [BUILD §4b.6](BUILD.md#4b6-the-engineering-playbook-and-reusable-templates) lists what it must cover.

Written 2026-10-21, while WP0 phase A, the first code, is being built ([BUILD §6.2](BUILD.md#62-wp0-foundation-workspace-contracts-and-quality-gates)). The Eileen parts therefore describe the verified build plan and the measurements the research took on the owner's PC, not results from a finished product. WP9 adds the measured results (benchmarks, the fleet, the learning curve) at the end of v0.1, and every pull request that changes the decision log updates this file, or says why it is unaffected ([D50](DECISIONS.md#d50-a-reusable-fully-cited-engineering-playbook) as amended by [D75](DECISIONS.md#d75-the-playbook-check-accepts-a-stated-reason), check 30 in [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)).

## How to use this

- **Who it is for.** A developer or an AI coding agent starting a new project in this style: research first, a written specification, parallel agents building against contracts, and conventions that CI enforces.
- **The shape of each practice.** *Practice* says what to do. *Why it works* gives the evidence. *Adopt it* says how to carry it to another project, and what to change when your constraints differ. *In Eileen* says how this repository does it. *Sources* lists where the claims come from.
- **Citations.** Every claim links to its source: a research report in [docs/research/](research/) with its section, a decision record by D-number in [DECISIONS.md](DECISIONS.md), a section of [BUILD.md](BUILD.md), or a primary source (a repository, documentation page, paper or post) with the date it was read. Most primary sources were read by the research passes on 2026-10-11 or 2026-10-20 and are cited as those reports cite them. A few were checked for this document on 2026-10-21; [References](#references) lists all of them. Where a report marked a claim as inferred or secondary, this document says so.
- **What is specific to Eileen.** Eileen is Windows-first, runs on one 16 GB PC that is shared with several coding agents, and drives vendor CLIs on the user's own subscription. Several practices exist because of those constraints. The *Adopt it* paragraphs say which parts are general.
- **Templates.** A `templates/` folder of ready-to-copy files (the root guidance files, the crate-doc template, the work-package file, the CI workflows, the `xtask` check registry as a small standalone crate, the pull request and issue templates, the scenario schema and an example scenario) will be produced from WP0's tested files, with `{{project}}` placeholders generated from the live files, and linked here when it lands ([BUILD §4b.6](BUILD.md#4b6-the-engineering-playbook-and-reusable-templates)).

## Contents

1. [How the work was done](#1-how-the-work-was-done)
2. [Repository architecture](#2-repository-architecture)
3. [What we pull in and why](#3-what-we-pull-in-and-why)
4. [CI and CD](#4-ci-and-cd)
5. [Testing tactics](#5-testing-tactics)
6. [Code maps and code graphs](#6-code-maps-and-code-graphs)
7. [Guidance for people and AI agents](#7-guidance-for-people-and-ai-agents)
8. [Observability](#8-observability)
9. [Improvement loops](#9-improvement-loops)
10. [Updatability and swappability](#10-updatability-and-swappability)
11. [Safety and credentials](#11-safety-and-credentials)
12. [Adopt this in your project: a checklist, in order](#12-adopt-this-in-your-project-a-checklist-in-order)
13. [References](#references)

## 1. How the work was done

How Eileen's plan was produced, from research to a verified specification, and the habits that keep a multi-agent effort on one shared PC from losing work. The revision log in [BUILD §8](BUILD.md#8-revision-log) is the record of the process; [BUILD §4b.6](BUILD.md#4b6-the-engineering-playbook-and-reusable-templates) names these working methods.

### 1.1 Research before design, measured on the target machine

**Practice.** Before anyone writes a design, run several research passes in parallel, one question each. Each pass measures on the machine the product will run on, under the load that machine really carries, and tags every claim by how it was checked: run on the machine, read from a primary source, read from a secondary source, or inferred.

**Why it works.** The numbers that decided Eileen's design were only visible on the target machine, and each one changed a decision. One UI Automation call blocked for 38.7 minutes, far past UI Automation's own 20 s timeout, which is why every risky dependency runs in a child process that can be killed ([rust-architecture §1](research/2026-10-rust-architecture.md#1-verdict), [D17](DECISIONS.md#d17-rust-core-with-supervised-sidecars)). A one-tool Python adapter handshook in 37 to 69 s with the Python MCP SDK (whose import alone took 35 to 42 s) against 1.7 to 2.9 s stdlib-only, which, with the owner's ask for a Rust runtime, retired the Python core ([rust-architecture §1](research/2026-10-rust-architecture.md#1-verdict), item 6, and [§5](research/2026-10-rust-architecture.md#5-measurements), [D38](DECISIONS.md#d38-windows-first-not-windows-only-the-python-scaffold-is-retired)). Without opting out of Windows' EcoQoS throttling, one speech synthesis run took 27.8 s instead of 2.4 s ([voice-stack §1](research/2026-10-voice-stack.md#1-recommendation-the-v01-pipeline), [D24](DECISIONS.md#d24-a-local-voice-pipeline-in-rust-cpu-only)). Loading the account's connectors made warm model turns two to four times slower ([brain-latency](research/2026-10-brain-latency.md#results), [D20](DECISIONS.md#d20-the-brain-is-a-warm-claude-session-with-a-hot-spare-behind-a-deterministic-fast-path)). A proof desktop server listed windows in 30 to 128 ms against 152 to 5,729 ms for the third-party server it replaced ([computer-use §1](research/2026-10-computer-use.md#1-verdict), [D21](DECISIONS.md#d21-eileens-own-rust-desktop-server-is-the-windows-action-layer)). None of these appear in a vendor's documentation.

**Adopt it.** Write one report per question, each with a "How to read this" legend for its tags, the machine and the load it measured under, what it did not run, and a sources section with dates. Run the passes in parallel, but give each one its own question so they do not duplicate work. Read every number taken under contention as pessimistic, and compare rows taken under the same conditions only. If you cannot measure (no memory, no sign-in), say so and mark the claim inferred rather than guess.

**In Eileen.** The September brief was written from five parallel research passes ([RESEARCH.md](RESEARCH.md), opening paragraph). The October round produced thirteen reports in [docs/research/](research/), twelve of which BUILD.md is built from and one of which (Colab) informs only a v0.2 connector ([BUILD](BUILD.md), opening paragraphs). The reports tag their claims by how each was checked (run on the PC with a date, primary, secondary, inferred) or date every measurement; see for example [rust-architecture, How to read this](research/2026-10-rust-architecture.md#how-to-read-this). Most measurements were taken while other agents compiled on the same PC. The rust-architecture report's 2026-10-20 timings were taken with CPU load averaging 66 percent and 0.46 to 2.6 GB free; the other reports record similar or heavier contention (voice-stack: 55 to 75 percent and 0.5 to 2 GB; app-hooks on 2026-10-20: 90 to 100 percent and 1.1 to 1.9 GB; subscription-harnesses on 2026-10-11: 56 to 100 percent and about 1 GB). So BUILD.md tells readers to treat the numbers as pessimistic ([rust-architecture, How to read this](research/2026-10-rust-architecture.md#how-to-read-this); [voice-stack](research/2026-10-voice-stack.md), opening; [app-hooks, How to read this](research/2026-10-app-hooks.md#how-to-read-this); [subscription-harnesses, How to read this](research/2026-10-subscription-harnesses.md#how-to-read-this), item 2; [BUILD](BUILD.md), "How to use it"). When memory was too low to build anything, a report says so and limits itself to reading ([engineering-practice, How to read this](research/2026-10-engineering-practice.md#how-to-read-this)).

**Sources.** The research reports named above; [RESEARCH.md](RESEARCH.md), opening paragraph; [D17](DECISIONS.md#d17-rust-core-with-supervised-sidecars), [D20](DECISIONS.md#d20-the-brain-is-a-warm-claude-session-with-a-hot-spare-behind-a-deterministic-fast-path), [D21](DECISIONS.md#d21-eileens-own-rust-desktop-server-is-the-windows-action-layer), [D24](DECISIONS.md#d24-a-local-voice-pipeline-in-rust-cpu-only), [D38](DECISIONS.md#d38-windows-first-not-windows-only-the-python-scaffold-is-retired).

### 1.2 An architect pass that turns the reports into one specification

**Practice.** One writer turns the reports and the owner's direction into a single specification: the target experience as scripts a tester can follow, one architecture, one set of contracts, and work packages with owners, dependencies and acceptance tests. The contracts are the only interface between packages. A package that needs a contract changed proposes the change in a separate small pull request instead of diverging quietly.

**Why it works.** Parallel builders need one source of truth. When a contract change is its own reviewed pull request, two packages cannot drift apart without someone noticing, and the specification stays the thing every agent reads first.

**Adopt it.** Keep the specification in the repository, numbered so that work packages and decisions can point at sections. Mark which documents no package owns (the specification, the decision log, the owner's direction) and route changes to them through a labelled amendment pull request. Order the work as waves with a thin end-to-end slice first.

**In Eileen.** [BUILD.md](BUILD.md) was written on 2026-10-20 by the architect pass of the design workflow and turns OWNER-DIRECTION.md and the reports into one Cargo workspace, one set of contracts and ten work packages built in waves W0 to W6, with live slice S1 gating everything from W3 on ([BUILD §0](BUILD.md#0-summary)). Its opening section says that the contracts in [§3](BUILD.md#3-workspace-and-contracts) are the only interface between packages, and that changes to files no package owns go through a pull request labelled `amendment` that the owner reviews.

**Sources.** [BUILD](BUILD.md), opening paragraphs and [§0](BUILD.md#0-summary); [BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it) (the amendment rule in every package's brief).

### 1.3 Independent critics with distinct lenses

**Practice.** After the architect pass, several critics review the specification independently, each through one lens, and each returns blocking issues with a concrete fix plus non-blocking improvements. The revision records, per lens, what was adopted and what was declined, with the reason.

**Why it works.** A single reviewer finds the class of problem they are looking for. Separate lenses find separate classes. Eileen's critics found problems the architect had no way to see from inside the design: the learned-recipe format could not express the worked examples it was meant to run ([D44](DECISIONS.md#d44-the-learning-loop-revised)); the first build guard admitted builds on a single 2 GB reading on a PC whose free memory sits near 1 to 2 GB, and the native dependencies would have broken the Linux lane ([D46](DECISIONS.md#d46-the-build-guard-and-the-gates-revised)); a worker with web tools and file reads could have sent private data out with nothing in the way ([D40](DECISIONS.md#d40-what-contains-a-worker-in-v01-and-what-waits-for-v02)).

**Adopt it.** Choose lenses that match your risks: feasibility on the real machine, whether the owner's asks are met, safety, engineering quality, and one lens for the subsystem that matters most to the product. Give every critic the binding requirements and the reports, not the architect's reasoning. Ask for blocking issues as a list with ids, so later rounds can track each one. Record declined suggestions with reasons, so the next reader does not reopen them.

**In Eileen.** Five reviews ran on 2026-10-20: feasibility on the owner's PC, the owner's asks and the feel, safety and best-in-class, engineering quality, and the learning loop ([BUILD](BUILD.md), opening paragraph; [BUILD §8](BUILD.md#8-revision-log)). Together they raised 49 blocking issues (8, 13, 11, 9 and 8 by lens, in that order), and the owner's entries added after the architect started became 8 more items; these counts come from the verification workflow's item list, which is kept with the owner's work files rather than in this repository. The revision log in [BUILD §8](BUILD.md#8-revision-log) lists, review by review, what was adopted and what was declined and why (for example, splitting three work packages was declined in favour of phases).

**Sources.** [BUILD §8](BUILD.md#8-revision-log); [D40](DECISIONS.md#d40-what-contains-a-worker-in-v01-and-what-waits-for-v02), [D44](DECISIONS.md#d44-the-learning-loop-revised), [D46](DECISIONS.md#d46-the-build-guard-and-the-gates-revised).

### 1.4 A verification loop per blocking issue

**Practice.** After the revision, independent checkers re-examine every blocking issue and every owner addition against the revised text and mark each one resolved, partial or unaddressed. The architect then fixes only the open items, and only those are checked again, for a bounded number of rounds. A question that only code or a measurement can settle is not argued on paper: it moves into the owning work package as an acceptance test.

**Why it works.** A revision's own claim to have fixed something is cheap. Checking each item against the text, by someone who did not write the fix, shows whether the design is converging, and it finds what the revision itself got wrong. Eileen's first round found that the revised memory rules would have admitted no worker and no build at all on a PC with 1 to 2 GB usually available ([D54](DECISIONS.md#d54-memory-admission-and-the-build-guard-read-available-memory-commit-headroom-and-paging)), and that a worker could have pressed Eileen's own approve hotkey with a synthetic keystroke, because the Windows hotkey API also fires on injected input ([D51](DECISIONS.md#d51-approvals-and-the-conversation-window-revised)). Carrying measurement-only questions into packages stops the paper revisions from going on forever, and the test guarantees the question is answered before the package merges.

**Adopt it.** Keep the blocking issues in a machine-readable list with stable ids. Run one checker per original lens plus one for owner additions. Record each round's changes as decision records, so the reasons survive. Stop when nothing is open, or when what remains can only be settled by running code, and write those as named acceptance tests.

**In Eileen.** The verification workflow tracked 57 items (the 49 blocking issues plus the 8 owner additions). Its tally of items still open after each round was 43, then 24, 16 and 8; the fourth round closed the last 8, and carried three questions into WP2, WP5 and WP7 as acceptance tests ([BUILD §7](BUILD.md#7-owner-decisions-settled-questions-and-risks), "Carried into packages"). As with the critics' counts, the per-round tally comes from the workflow's own records, kept with the owner's work files. The published revision log names the items each revision addressed, by id, and counts them as 37, 22, 15 and 8 ([BUILD §8](BUILD.md#8-revision-log)). Each round produced its own decision records: D51 to D59, D60 to D64, D65 to D69 and D70 to D73 ([BUILD §7](BUILD.md#7-owner-decisions-settled-questions-and-risks), answer 1). The log says of every round that nothing was declined, and from the second round on that every remaining gap was right on the facts ([BUILD §8](BUILD.md#8-revision-log)).

**Sources.** [BUILD §7](BUILD.md#7-owner-decisions-settled-questions-and-risks) and [§8](BUILD.md#8-revision-log); [D51](DECISIONS.md#d51-approvals-and-the-conversation-window-revised) to [D73](DECISIONS.md#d73-the-machine-wide-memory-files-have-one-module), whose Source lines name the item ids each one settles.

### 1.5 Evidence first, then the change

**Practice.** Every proposed change leads with its evidence and only then offers the code: the failing test or scenario that now passes, numbers before and after, a replay of the bug, or benchmark results. CI checks that the evidence is there and that the tests it names exist, and for a fix, that the named test fails on the base commit and passes on the head.

**Why it works.** A reviewer, human or agent, checks a claim instead of trusting it. The best-run repositories in the survey require the same: Turso requires every change to have a test that was seen failing and then passing, and Biome asks for no claim about the code without a file and line or a command ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project); [Turso AGENTS.md (read 2026-10-20)](https://github.com/tursodatabase/turso/blob/main/AGENTS.md); [Biome AGENTS.md (read 2026-10-20)](https://github.com/biomejs/biome/blob/main/AGENTS.md)).

**Adopt it.** Put an Evidence section first in the pull request template, with a small grammar for the ids a machine reads (test paths, scenario ids) and free prose for everything else. Check the template and the ids without compiling. For fixes, build a tree of the base commit with the head's tests copied in and require the tests to fail there. Provide an override that needs an owner's label for cases the CI platform cannot show, and void the label when new commits arrive.

**In Eileen.** The owner's entry "Evidence first, then the change" applies during the v0.1 build too ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20)). The [pull request template](../.github/PULL_REQUEST_TEMPLATE.md) opens with `## Evidence` and field lines (`tests:`, `scenarios:`, `numbers:`, `replay:`, `incident:`, `owner-run:`, `override:`). Check 8 (`cargo xtask pr-meta`) parses it, requires an id for feature, fix and refactor pull requests, resolves every test by `git grep` and checks that the declared type fits the changed files; check 19 (`cargo xtask evidence`, its own required workflow) shows a fix failing before and passing after, and a Windows-only or uncompilable case needs the owner's `evidence-reviewed` label applied after the latest push ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [D57](DECISIONS.md#d57-the-thin-slice-is-gated-on-named-pull-requests-evidence-checks-from-the-first-pull-request), [D63](DECISIONS.md#d63-evidence-has-a-grammar-and-its-own-required-check-dependabot-updates-cargo-scenario-keys-are-stable), [D68](DECISIONS.md#d68-evidence-scenario-replay-and-the-slice-revised)).

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci), checks 8 and 19; [engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project).

### 1.6 A binding, dated owner-direction file

**Practice.** The owner's asks live in one file of dated entries. It is binding: every design document must satisfy every entry. When an entry conflicts with an older decision, the newer entry wins and a decision record says so. Unanswered questions are covered by a standing rule the owner sets.

**Why it works.** Agents and sessions rotate; intent written down once, with a date, survives them. Conflicts are settled by date, not by whoever remembers the conversation. A standing rule ("the recommendation applies unless the owner says otherwise") keeps work moving without guessing.

**Adopt it.** Write entries in the owner's own terms, concrete enough to test (Eileen's include literal benchmark requests). Keep proposals that the owner has not confirmed in a separate, clearly labelled section. When an entry is corrected, correct it in place with the date and the reason, and let a decision record carry the consequences.

**In Eileen.** [OWNER-DIRECTION.md](OWNER-DIRECTION.md) states in its first paragraph that BUILD.md, ARCHITECTURE.md and ROADMAP.md must satisfy every entry and that a newer entry beats an older decision. Its entries are dated 2026-10-11 and 2026-10-20; the owner's answers to BUILD.md's 23 open questions carry the standing rule that the assistant's recommendation applies unless the owner explicitly says otherwise; proposals sit under ["Proposed 2026-10-20 (by the assistant, not yet confirmed by the owner)"](OWNER-DIRECTION.md#proposed-2026-10-20-by-the-assistant-not-yet-confirmed-by-the-owner); and the Colab entry records its own correction ("Corrected 2026-10-20 ...") ([OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20)). [D58](DECISIONS.md#d58-the-owners-answers-of-2026-10-20-applied) and [D74](DECISIONS.md#d74-the-owners-answers-applied-in-full) apply the answers to the design.

**Sources.** [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [D58](DECISIONS.md#d58-the-owners-answers-of-2026-10-20-applied), [D74](DECISIONS.md#d74-the-owners-answers-applied-in-full); [BUILD §7](BUILD.md#7-owner-decisions-settled-questions-and-risks).

### 1.7 Decision records that supersede rather than rewrite

**Practice.** Decisions are an append-only log. To change one, add a new record that names what it supersedes or amends, and update the old record's status line to point forward. Each record has a date, a status, the decision, why, its consequences and its source. CI checks the format.

**Why it works.** The reasons for the current design stay readable, including the reasons for designs that were dropped. A reader who lands on an old record follows its status line to the current rule instead of trusting stale text.

**Adopt it.** Use one file with a fixed grammar (`## D<n>. <title>`, then Date, Status, optional Supersedes and Amends, Decision, Why, Consequences, Source). Number records sequentially and check that every referenced record exists. Require every decision to cite the research or the owner's entry behind it. Make a change to the log update any document that summarises it, in the same pull request.

**In Eileen.** [DECISIONS.md](DECISIONS.md) opens with "To change a decision, add a new entry that supersedes the old one; do not edit history." It runs from D1 to D75: for example the Python core of [D9](DECISIONS.md#d9-python-core-native-sidecar-mcp-between-processes) was superseded by the Rust core of [D17](DECISIONS.md#d17-rust-core-with-supervised-sidecars), and the third-party desktop driver of [D5](DECISIONS.md#d5-cua-driver-is-the-windows-action-layer) by Eileen's own server in [D21](DECISIONS.md#d21-eileens-own-rust-desktop-server-is-the-windows-action-layer). Check 12 parses the grammar and check 30 requires a pull request that changes DECISIONS.md to change this playbook too, or say why not ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). There is one sanctioned exception to "do not edit history": the owner asked for an earlier working name and franchise references to be scrubbed from the dated records, and [D74](DECISIONS.md#d74-the-owners-answers-applied-in-full) records that it was done ([OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20), answer 15).

**Sources.** [DECISIONS.md](DECISIONS.md); [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci), checks 12 and 30; [D74](DECISIONS.md#d74-the-owners-answers-applied-in-full).

### 1.8 A handoff file kept current, and living work-package files

**Practice.** Keep one handoff document for the whole effort whose top section is rewritten after every run, merge, batch of work or owner decision, so that a new session knows the state in one read. Give each work package a living plan file: goal, done-when list, dated progress, surprises with evidence, decisions, and outcome.

**Why it works.** Multi-session, multi-agent work loses its state between sessions. A plan that someone with no context can continue from is the pattern OpenAI's cookbook describes as an execution plan; the engineering-practice report adopted its shape because it matches how Eileen's runs already hand work between sessions ([engineering-practice §4.8](research/2026-10-engineering-practice.md#48-work-package-files); [OpenAI cookbook, execution plans, secondary (read 2026-10-20)](https://developers.openai.com/cookbook/articles/codex_exec_plans)).

**Adopt it.** Make rewriting the handoff's first section part of finishing any step, not a separate chore. Keep the per-package files in the repository so that reviewers see them, and keep the handoff where every session will look, even if that is outside the repository.

**In Eileen.** Work-package files live at `docs/wp/WP<n>-<topic>.md` in the living-plan shape and are updated by the agent doing the work ([BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents)); every package keeps its file current as part of its definition of done ([BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it)). Handoff upkeep is one of the working methods the specification names ([BUILD §4b.6](BUILD.md#4b6-the-engineering-playbook-and-reusable-templates), item 10). The project's handoff file itself sits with the owner's local work files, outside this public repository.

**Sources.** [BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents) and [§4b.6](BUILD.md#4b6-the-engineering-playbook-and-reusable-templates); [engineering-practice §4.8](research/2026-10-engineering-practice.md#48-work-package-files).

### 1.9 Durable copies of scratch work

**Practice.** Before a session ends, copy every research attempt's scripts, result files and condensed notes out of the temporary scratch area into a durable work-files folder, without build outputs. Reports cite that folder for anything they did not re-run.

**Why it works.** Sessions end without warning. In Eileen's research, two attempts at the Rust-architecture question were cut off before writing their report, one by a usage limit on 2026-10-11 and one when the PC ran out of memory on 2026-10-20; the report that exists is their write-up, built from what they saved ([rust-architecture, How to read this](research/2026-10-rust-architecture.md#how-to-read-this) and [§2](research/2026-10-rust-architecture.md#2-what-the-earlier-attempts-built-and-ran)). The voice-stack, computer-use and learning-loop reports were written the same way from earlier attempts' saved outputs ([voice-stack](research/2026-10-voice-stack.md), opening; [computer-use, How to read this](research/2026-10-computer-use.md#how-to-read-this); [learning-loop, How to read this](research/2026-10-learning-loop.md#how-to-read-this)).

**Adopt it.** Keep one durable folder per effort, beside the repository and outside any temp directory. For each topic keep the raw outputs and a short condensed note of what was run and what it showed. Leave out target directories and other build output: they are large and can be rebuilt. Delete scratch build directories once the durable copy exists.

**In Eileen.** The rust-architecture report lists a durable copy of its scratch workspaces without target directories, plus condensed notes of both earlier attempts, and points at the larger originals in the temp scratch area separately ([rust-architecture §2](research/2026-10-rust-architecture.md#2-what-the-earlier-attempts-built-and-ran)); its last section says which scratch target directories to delete once nobody needs them ([rust-architecture §13](research/2026-10-rust-architecture.md#13-reproducing)).

**Sources.** [rust-architecture §2](research/2026-10-rust-architecture.md#2-what-the-earlier-attempts-built-and-ran) and [§13](research/2026-10-rust-architecture.md#13-reproducing); [voice-stack](research/2026-10-voice-stack.md); [computer-use, How to read this](research/2026-10-computer-use.md#how-to-read-this); [learning-loop, How to read this](research/2026-10-learning-loop.md#how-to-read-this).

### 1.10 Memory limits on parallel agents

**Practice.** When several agents share one machine, measure what the machine really has free, then make every build go through one guarded command that admits builds by memory and refuses or waits otherwise. Deny agents the direct build commands in their harness settings, so the guard cannot be bypassed. Run long builds in the background and never start one at the end of a session.

**Why it works.** On Eileen's 16 GB PC, one clean development build under contention took 1,278 s (about 21 minutes; the report's own parenthesis, 19 min 41 s, is a slip in that dated record) and drove free memory to 462 MB; a fat-LTO release build beside other agents took free memory to 0 MB and ended the session; one compiler process was seen at 417 MB and a clean build ran about 9.4 compile units at once; four tools built from source took 59 minutes ([rust-architecture §5](research/2026-10-rust-architecture.md#5-measurements) and [§9](research/2026-10-rust-architecture.md#9-parallel-builds-on-this-16-gb-pc); [BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it)). A build started in the background dies with the session that started it, which is how the 2026-10-20 release build and its measurements were lost ([rust-architecture §9](research/2026-10-rust-architecture.md#9-parallel-builds-on-this-16-gb-pc), item 10).

**Adopt it.** Read available memory (free plus standby), commit headroom and the paging rate, not "free RAM" alone. Measure a baseline working day before trusting any threshold, and leave the guard's own builds out of the baseline. Hold build slots as OS file locks, so a dead holder releases them, and record the holder. Scale the compiler's job count to available memory, never downward as memory rises. Exit with a distinct code when no slot is free (Eileen uses 75), so a background agent can tell waiting from failing. Install tools prebuilt, never from source.

**In Eileen.** `cargo xtask` takes one of two slot locks through `eileen-buildguard`: slot 0 is admitted by commit headroom and a paging limit learned from the PC's own baseline, with `CARGO_BUILD_JOBS` set to `min(6, max(2, floor((available_gb - 1.5) / 0.5)))` and, from WP0 phase B, never above the logical core count; slot 1 needs real room and waits until the first build has passed its peak; cargo runs in a kill-on-close Job Object at below-normal priority; release builds, speech-model tests and benchmarks take both slots ([BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it); [D46](DECISIONS.md#d46-the-build-guard-and-the-gates-revised), [D54](DECISIONS.md#d54-memory-admission-and-the-build-guard-read-available-memory-commit-headroom-and-paging), [D64](DECISIONS.md#d64-the-slice-in-w1-and-memory-thresholds-from-this-pcs-own-measurements), [D69](DECISIONS.md#d69-memory-thresholds-are-machine-wide), [D73](DECISIONS.md#d73-the-machine-wide-memory-files-have-one-module)). The agents' `.claude/settings.json` denies direct `cargo build`, `test`, `nextest`, `clippy`, `check`, `run`, `doc`, `bench`, `fuzz`, `insta` and `mutants`, plus `cargo install`, `cargo clean` and release builds, and build agents run without a language server, whose background checks would compile outside the slots ([BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents) and [§6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it), rules 3 and 4). The same memory rules admit Eileen's own Claude jobs at run time ([BUILD §2.5](BUILD.md#25-workers-routing-and-jobs), "Memory admission"), and the machine profile decides what Eileen keeps resident in the first place ([§2.7](#27-fit-the-machine-it-runs-on)).

**Sources.** [rust-architecture §5](research/2026-10-rust-architecture.md#5-measurements) and [§9](research/2026-10-rust-architecture.md#9-parallel-builds-on-this-16-gb-pc); [BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it); [D46](DECISIONS.md#d46-the-build-guard-and-the-gates-revised), [D54](DECISIONS.md#d54-memory-admission-and-the-build-guard-read-available-memory-commit-headroom-and-paging), [D69](DECISIONS.md#d69-memory-thresholds-are-machine-wide).

### 1.11 Resuming interrupted multi-agent runs from cached results

**Practice.** Design long multi-agent runs so that every finished step leaves its result on disk, and checkpoint the shared document as a commit after each round. When a run is interrupted, resume from the cached results of the finished steps and re-run only what was lost, instead of starting over.

**Why it works.** Research and verification runs last hours on a shared machine, and usage limits, memory exhaustion and sleep all stop them. Work that was saved survives; work that was only in a session does not.

**Adopt it.** Give every step a deterministic output path and have the orchestrator skip steps whose output exists. Commit the document after each round with a message naming the round. When writing up an interrupted attempt, re-run only what is cheap and say exactly what was re-run. Keep the machine awake for the length of an unattended run.

**In Eileen.** The rust-architecture, voice-stack, computer-use and learning-loop reports are write-ups of interrupted attempts' cached results; the learning-loop report re-ran exactly two cheap things and says which ([learning-loop, How to read this](research/2026-10-learning-loop.md#how-to-read-this)). The verification workflow checkpointed its fix rounds as commits and resumed its third round after a usage-limit stop, according to the run's own log (kept with the owner's work files). The owner's unattended nightly lane holds a display-required and system-required power request for its duration so the session neither sleeps nor locks mid-run ([BUILD §4.6](BUILD.md#46-what-runs-where)).

**Sources.** [learning-loop, How to read this](research/2026-10-learning-loop.md#how-to-read-this); [rust-architecture, How to read this](research/2026-10-rust-architecture.md#how-to-read-this); [BUILD §4.6](BUILD.md#46-what-runs-where).

## 2. Repository architecture

The shape of the code, as [BUILD §2](BUILD.md#2-the-architecture-as-it-will-be-built-now) and [BUILD §3](BUILD.md#3-workspace-and-contracts) specify it, and why each part has that shape.

### 2.1 A sans-IO core driven by one step function

**Practice.** Put every decision the system makes in a pure state machine with one entry point. Everything that can happen to it is one closed `Input` enum; everything it can ask the world to do is one closed `Effect` enum. Time, randomness and IDs are passed in. Drivers turn the outside world into inputs and carry out effects; they make no decisions.

**Why it works.** The best-tested systems in the survey make concurrency bugs reproducible this way: TigerBeetle's VOPR, FoundationDB and Turso stub out every source of nondeterminism and replay any failure from a seed and a commit ([engineering-practice §1](research/2026-10-engineering-practice.md#1-verdict) and [§2.4](research/2026-10-engineering-practice.md#24-simulation-and-concurrency-testing); [TigerBeetle VOPR (read 2026-10-20)](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/vopr.md); [FoundationDB testing (read 2026-10-20)](https://apple.github.io/foundationdb/testing.html)). rust-analyzer's main loop accepts a closed enum of events so that everything that can trigger work is visible in one place ([observability §5.1](research/2026-10-observability.md#51-the-principle-a-closed-set-of-inputs); [rust-analyzer architecture (read 2026-10-20)](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)). The same closed input set makes recording, replay, simulation and learning one mechanism ([§8](#8-observability)).

**Adopt it.** Design the core this way from the first commit; retrofitting it is much harder. Ban the real clock, OS randomness and unordered maps in the core with lints, and allow the single real implementation through an expected-lint exception. Bound every queue and report overflow as an event. Put fault-injection points in the core as a runtime switch, not a build feature, because a feature on a shared crate is unified into every binary built in the same invocation ([engineering-practice §3.2](research/2026-10-engineering-practice.md#32-what-the-core-must-look-like-wp0-constraints)).

**In Eileen.** `Core::step(&mut self, now, rng, input, effects)` over closed `Input` and `Effect` enums, with the resolver, dictation cleaner, settings validator and skill engine reached through ports ([BUILD §3.3.3](BUILD.md#333-inputs-and-effects-of-the-core), [§3.3.15](BUILD.md#3315-ports); [D31](DECISIONS.md#d31-a-sans-io-core-tested-by-deterministic-simulation)). `clippy.toml` bans `Instant::now`, `SystemTime::now`, `Ulid::generate`, unbounded channels and bare `tokio::spawn`, and the core's own copy also bans `HashMap` ([BUILD §4b.2](BUILD.md#4b2-lints-clippy-configuration-and-unsafe-policy)). Check 24 fails if `eileen-core` depends on anything but the contracts, `eileen-stdx` and `eileen-trace` ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)).

**Sources.** [engineering-practice §2.4](research/2026-10-engineering-practice.md#24-simulation-and-concurrency-testing) and [§3.2](research/2026-10-engineering-practice.md#32-what-the-core-must-look-like-wp0-constraints); [observability §5.1](research/2026-10-observability.md#51-the-principle-a-closed-set-of-inputs); [D31](DECISIONS.md#d31-a-sans-io-core-tested-by-deterministic-simulation); [BUILD §3.3.3](BUILD.md#333-inputs-and-effects-of-the-core).

### 2.2 Plug-in traits for everything AI progress improves

**Practice.** Every component that better models or engines will improve sits behind a trait, with a factory that names the implementation from configuration. Implementations not built yet still exist as designed slots that return a typed "unavailable" error. A better component is then a new implementation plus a configuration line.

**Why it works.** The parts move fast: rmcp, the official Rust MCP SDK, shipped six releases in six weeks (3.3.0 on 10 September to 3.6.0 on 20 October, dates from the crates.io registry, checked 2026-10-21), and Kyutai released new, smaller Pocket TTS models on the day the voice research was written ([rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20); [voice-stack §6](research/2026-10-voice-stack.md#6-speech-synthesis)). A trait boundary keeps those changes out of the core and lets a benchmark compare the old and new implementation through the same interface before anything switches ([§9.2](#92-evidence-gated-upgrades)).

**Adopt it.** List the components your field moves fastest on and give each one a trait from the start. Make each implementation report its own capabilities (languages, cost, latency class) instead of hard-coding them in the caller. Reserve enum variants for features you have scheduled, so adding them later changes no pinned wire format.

**In Eileen.** The brain is behind `BrainBackend` with `brain_for(route, cfg)`, workers behind `WorkerDriver`; only Claude is built in v0.1 and Codex, DeepSeek, Grok and a local model are slots that return `RouteUnavailable` ([BUILD §2.3](BUILD.md#23-the-brain), [§2.5](BUILD.md#25-workers-routing-and-jobs); [D19](DECISIONS.md#d19-workers-are-named-by-route-native-claude-driver-acp-as-a-slot), [D53](DECISIONS.md#d53-model-names-are-aliases-in-config-the-brain-and-the-speech-stages-are-plug-ins)). Voice activity, recognition, turn detection and voices sit behind traits, each reporting its languages and cost; the voice engine is a setting per voice ([D53](DECISIONS.md#d53-model-names-are-aliases-in-config-the-brain-and-the-speech-stages-are-plug-ins), [D39](DECISIONS.md#d39-voices-the-engine-is-chosen-per-voice-and-chatterbox-is-in-v01)). Connectors publish manifests of tools, tiers and events, so the phone or a home hub is a new connector, not a redesign ([BUILD §2.14](BUILD.md#214-connectors-and-platforms); [D37](DECISIONS.md#d37-the-pc-first-then-outward-through-connectors)). The `ml_project` job kind is reserved in the contracts and refused with one sentence until its v0.2 connector exists ([D74](DECISIONS.md#d74-the-owners-answers-applied-in-full)). The owner's entry asks for exactly this ("Everything swappable", [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20)).

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [BUILD §2.3](BUILD.md#23-the-brain), [§2.5](BUILD.md#25-workers-routing-and-jobs), [§2.14](BUILD.md#214-connectors-and-platforms); [D53](DECISIONS.md#d53-model-names-are-aliases-in-config-the-brain-and-the-speech-stages-are-plug-ins).

### 2.3 Supervised child processes for anything that can hang

**Practice.** Anything that can block, crash or leak (native libraries, OS automation, model runtimes, vendor CLIs) runs in its own child process, started by one supervisor that puts it in a kill-on-close job, gives every call a deadline and every child a heartbeat, and restarts with bounded backoff. The process that owns the user's experience links none of it.

**Why it works.** A blocked cross-process call often cannot be cancelled in-process; only a process boundary and a kill recover from it. The 38.7-minute UI Automation hang is the measured case ([rust-architecture §10](research/2026-10-rust-architecture.md#10-pitfalls), pitfall 1; [UI Automation transaction timeout (read 2026-10-20)](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_transactiontimeout)). Separate binaries also mean an agent working on one area never links another area's heavy dependencies ([BUILD §2.1](BUILD.md#21-processes), rule 5).

**Adopt it.** Make the supervisor the only place that starts a process, and ban the standard spawn call everywhere else with a lint, listing the few exceptions by name. Keep children long-lived, because process start is slow on a loaded machine (a bare Python interpreter took 4.5 to 6.9 s, a cold Claude CLI 11 to 111 s, on Eileen's PC under load; [BUILD §2.1](BUILD.md#21-processes), rule 3). Use standard output for the protocol and standard error for logs. Before killing a hung child, try the cheaper recovery (abandon a thread and start a fresh one), and capture a dump before the kill.

**In Eileen.** The core (`eileen.exe`) links no UI Automation, app COM, ONNX Runtime or web view; speech, desktop control, app scripting, the HUD and every Claude session are supervised children, started through `eileen_supervisor` with `process-wrap` 10.0.1 Job Objects; a UI Automation call past its 5 s deadline first has its thread abandoned, and only three abandoned threads in ten minutes or a missed heartbeat get the sidecar hang-dumped and restarted ([BUILD §2.1](BUILD.md#21-processes); [D17](DECISIONS.md#d17-rust-core-with-supervised-sidecars)). Clippy bans `Command::new` outside the supervisor with three listed exemptions ([BUILD §4b.2](BUILD.md#4b2-lints-clippy-configuration-and-unsafe-policy)). Apps Eileen opens for the user are started by the Windows shell, not as Eileen's children, so they survive Eileen's restarts ([BUILD §2.1](BUILD.md#21-processes), rule 1; [D62](DECISIONS.md#d62-containment-calls-and-the-brains-slots-revised)).

**Sources.** [rust-architecture §1](research/2026-10-rust-architecture.md#1-verdict), [§3.2](research/2026-10-rust-architecture.md#32-process-model), [§10](research/2026-10-rust-architecture.md#10-pitfalls); [BUILD §2.1](BUILD.md#21-processes); [D17](DECISIONS.md#d17-rust-core-with-supervised-sidecars).

### 2.4 Platform crates behind traits, Windows first and Linux next

**Practice.** Everything that touches the operating system sits behind traits in one neutral crate and is implemented in per-area platform crates. Native libraries are declared only for the platform that needs them. The platform-neutral crates build and pass their tests on a second operating system in CI from the first commit, even while only one platform is a full target.

**Why it works.** A port becomes "write new platform crates", not "rework the core". Testing the neutral crates on Linux from day one keeps them honest: a Windows type leaking into logic breaks the Linux job at once ([engineering-practice §6.4](research/2026-10-engineering-practice.md#64-keeping-windows-under-about-15-minutes), item 1). The same item makes moving everything platform-neutral to Linux the first lever for keeping the Windows CI jobs short.

**Adopt it.** Name the neutral trait crate and the platform crates early. Gate platform crates with `#![cfg(...)]` so they compile to empty libraries elsewhere. Put fakes for each trait next to it, behind a test feature. Enforce the dependency direction with a check.

**In Eileen.** `eileen-platform` holds the traits (`AudioEndpoints`, `SystemVolume`, `SystemMemory`, `MachineIdentity`, `SecretStore`, `AppHost`, `Grounding`, `Automation`, `Input`, `Capture`, `Apps`, `TextInsertion`, `MediaSessions`, `RecentSources`, `ShellHost`, `HudSurface`), with Windows implementations in the `-win` crates and a Linux port planned as `-linux` crates (AT-SPI, PipeWire, MPRIS, Secret Service) ([BUILD §3.4](BUILD.md#34-platform-traits), [§2.14](BUILD.md#214-connectors-and-platforms); [D38](DECISIONS.md#d38-windows-first-not-windows-only-the-python-scaffold-is-retired), [D55](DECISIONS.md#d55-the-crate-graph-and-the-lint-bans-agree-secrets-volume-and-app-hooks-are-platform-traits)). Native dependencies (cpal, sherpa-onnx, ort, the keyring store, tray-icon, crash-handler) appear only under `[target.'cfg(windows)'.dependencies]`, and check 15 enforces the table ([BUILD §4.1](BUILD.md#41-dependency-pins), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). The owner asked for this in "Windows first, not Windows only" ([OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20)).

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [BUILD §3.4](BUILD.md#34-platform-traits), [§4.1](BUILD.md#41-dependency-pins); [D38](DECISIONS.md#d38-windows-first-not-windows-only-the-python-scaffold-is-retired), [D55](DECISIONS.md#d55-the-crate-graph-and-the-lint-bans-agree-secrets-volume-and-app-hooks-are-platform-traits).

### 2.5 A contracts crate with snapshot-pinned types

**Practice.** Every type that crosses a process, pipe, file or socket lives in one contracts crate, derives serialisation and a JSON Schema, and has its wire shape pinned by a snapshot test, so any change is a reviewed diff. Free text from outside is wrapped in types that mark it as untrusted or personal.

**Why it works.** Parallel packages need a stable interface, and a snapshot makes an accidental change impossible to miss. rust-analyzer's lesson is that a format becomes stable the moment its first user appears, so versioned formats and snapshot tests belong in the first commit ([engineering-practice §5.2](research/2026-10-engineering-practice.md#52-skeleton)). The snapshots also catch surprises from the build itself: in Eileen's skeleton a dependency switched on `serde_json/preserve_order` for the whole build and silently reordered a schema snapshot ([rust-architecture §10](research/2026-10-rust-architecture.md#10-pitfalls), pitfall 7).

**Adopt it.** Split the contracts crate into one module per producing area with its own snapshot file, so parallel packages rarely touch the same file. Pin the modules on your first slice's path at once, and mark the rest provisional until their owner's first pull request. Use ordered maps wherever order can reach a snapshot. Treat a new top-level variant as an architectural change.

**In Eileen.** `eileen-contracts` holds every serialised type with serde and schemars, insta snapshots per module, `BTreeMap` or `IndexMap` wherever a map can reach an effect or a snapshot, and `Untrusted<T>` and `Personal<T>` wrappers checked by a test (check 26) ([BUILD §3.3](BUILD.md#33-contracts), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). Only the contracts types are serialised across a boundary, only `eileen-mcp` knows the MCP SDK and only `eileen-harness` knows vendor CLI flags ([BUILD §3.1](BUILD.md#31-the-cargo-workspace)).

**Sources.** [BUILD §3.1](BUILD.md#31-the-cargo-workspace), [§3.3](BUILD.md#33-contracts); [rust-architecture §10](research/2026-10-rust-architecture.md#10-pitfalls); [engineering-practice §5.2](research/2026-10-engineering-practice.md#52-skeleton).

### 2.6 Model aliases in configuration

**Practice.** Code never names a model. Configuration maps the project's own names (fast, mid, strong) to a vendor alias or a full id; the harness resolves each alias to a full id whenever the vendor tool's version changes, passes only full ids to the vendor, and records the resolved id beside every timing.

**Why it works.** Vendor aliases drift between tool versions: on 2026-10-11 the `haiku` alias resolved to `claude-haiku-4-5-20251001` on one installed build of the Claude CLI and to `claude-haiku-5-5` on another ([subscription-harnesses §2](research/2026-10-subscription-harnesses.md#2-versions-on-2026-10-11)). Recording the resolved id turns a silent change into a visible line in the benchmark results.

**Adopt it.** Keep the alias table in the user-editable configuration, resolve with a cheap probe per alias, cache the result per tool version, and keep every flag you pass to a vendor tool in one constant that a nightly job checks against the tool's own help output.

**In Eileen.** `routing.models` maps `fast`, `mid` and `strong` to `haiku`, `sonnet` and `opus` by default; the harness resolves them per CLI version into `run\models.json`, and every flag Eileen passes is one constant in `eileen-harness` that the nightly CLI drift job checks against `claude --help` of the pinned and the latest CLI ([BUILD §2.3](BUILD.md#23-the-brain), [§4.6](BUILD.md#46-what-runs-where); [D53](DECISIONS.md#d53-model-names-are-aliases-in-config-the-brain-and-the-speech-stages-are-plug-ins), which amends [D20](DECISIONS.md#d20-the-brain-is-a-warm-claude-session-with-a-hot-spare-behind-a-deterministic-fast-path)).

**Sources.** [BUILD §2.3](BUILD.md#23-the-brain); [D53](DECISIONS.md#d53-model-names-are-aliases-in-config-the-brain-and-the-speech-stages-are-plug-ins); [subscription-harnesses §2](research/2026-10-subscription-harnesses.md#2-versions-on-2026-10-11).

### 2.7 Fit the machine it runs on

**Practice.** Let the product read the machine it runs on (memory and what is usually free, cores by type, GPU and NPU, disk, the OS build, power, audio devices), measure the few things that matter with short built-in benchmarks, and choose its own settings through one pure function that gives a reason for every choice. Re-check while running: let optional extras go when the machine gets busy and bring them back slowly. Explain the choices in plain words, and let the user override any of them without the product undoing it.

**Why it works.** The settings that matter most differ by machine and by hour. On Eileen's 16 GB PC a live read under the owner's ordinary load found 1.27 GB free, and a Claude worker starts in 1 to 2 s from a warm session but in 30 to 111 s cold, so whether one warm session fits is the difference between seconds and a minute ([BUILD §4.3](BUILD.md#43-latency-budget-per-stage), [§4.4](BUILD.md#44-memory-budget)). A specification sheet is not a measurement: the PC's Arc GPU and NPU gave no usable path for the voice model through DirectML or OpenVINO, and one Kokoro run took 27.8 s instead of 2.4 s until the process opted out of Windows' power throttling ([voice-stack §7](research/2026-10-voice-stack.md#7-npu-igpu-and-windows-scheduling); [BUILD §2.1](BUILD.md#21-processes), rule 6). Rules written for one machine give a 32 GB desktop the same cold starts, or a small laptop a warm pool it cannot hold. Eileen's design first kept its memory rules in four places, none of which could say why it chose what it did; one plan with a reason per setting can ([D76](DECISIONS.md#d76-fit-the-machine-and-gate-on-the-speaker-only-above-a-measured-bar)).

**Adopt it.** Read the facts through a platform trait whose fake holds a few fixture machines, so the chooser is table-tested against them. Keep a long, cheap history of free memory (hourly summaries are enough) and decide from the usual figure, with the product's own extras added back, never from one busy minute at start. Fill optional extras in a fixed order of value from a budget, and keep per-job admission as a separate gate that does not change. Benchmark briefly, only at setup and when a fact the result depends on changes, record the conditions beside every number, and never spend the user's paid quota on a benchmark. Swap a heavy component only after the new choice has held for a while, and only while the user is idle. Build every explanation from templates and numbers, never from a model. Make every fitted setting `auto` by default and an override an ordinary setting, with a check that says what it costs when it does not fit.

**In Eileen.** The machine profile ([BUILD §2.16](BUILD.md#216-fitting-the-machine-one-machine-profile); [D76](DECISIONS.md#d76-fit-the-machine-and-gate-on-the-speaker-only-above-a-measured-bar)): `MachineFacts` read through the `MachineProbe` trait at setup, at every start and on change; hourly memory summaries in the same `samples` module as the build guard's machine-wide files ([BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it); [D73](DECISIONS.md#d73-the-machine-wide-memory-files-have-one-module)); short speech benchmarks while setup loads the models anyway; `eileen_core::fit::plan`, which fills the speech models, the deep pool session, the hot spare and the computer-use pool session from a memory budget and sets the threads, the worker cap, disk caps and idle work from the facts; a live re-check with a 5-minute hold before anything comes back; a HUD panel, a chip and "about this PC"; and `auto` keys with a `MachineFit` check. Memory admission's four rules stay the per-job gate ([BUILD §2.5](BUILD.md#25-workers-routing-and-jobs); [D54](DECISIONS.md#d54-memory-admission-and-the-build-guard-read-available-memory-commit-headroom-and-paging)), and the types are in [BUILD §3.3.17](BUILD.md#3317-the-machine-profile).

**Sources.** [OWNER-DIRECTION, 2026-10-21](OWNER-DIRECTION.md#2026-10-21); [BUILD §2.16](BUILD.md#216-fitting-the-machine-one-machine-profile), [§4.3](BUILD.md#43-latency-budget-per-stage), [§4.4](BUILD.md#44-memory-budget), [§6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it); [D54](DECISIONS.md#d54-memory-admission-and-the-build-guard-read-available-memory-commit-headroom-and-paging), [D73](DECISIONS.md#d73-the-machine-wide-memory-files-have-one-module), [D76](DECISIONS.md#d76-fit-the-machine-and-gate-on-the-speaker-only-above-a-measured-bar); [voice-stack §7](research/2026-10-voice-stack.md#7-npu-igpu-and-windows-scheduling).

## 3. What we pull in and why

**Practice.** Every tool, crate, model and service is listed in one place with its exact version, its licence and the alternative it beat, with the measurement or rule that decided it. New versions wait out a cooling-off period, protocol crates are pinned exactly, licences are checked by a tool on every pull request, and the licences that tool cannot see (downloaded native libraries and models) are listed by the release lane.

**Why it works.** A choice written down with its reason is not reopened by the next agent, and a licence problem is found when the dependency is added, not at release. Protocol crates change weekly (rmcp, the official Rust MCP SDK, shipped six releases in six weeks, 3.3.0 on 10 September to 3.6.0 on 20 October by the registry's dates), which is why they are pinned exactly and updated by hand ([rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20)).

**Adopt it.** Declare every third-party dependency once at the workspace level; let members only reference it. Keep a "deliberately not used" list with reasons. Allow only permissive licences in linked code, and treat GPL tools you run but never link separately. Install development tools prebuilt.

**In Eileen.** The tables below are [BUILD §4.1](BUILD.md#41-dependency-pins) and [§4.2](BUILD.md#42-models-and-eileen-setup) with the reason for each choice from the research. Licences are as the cited report records them; where a report records none, the licence comes from the registry (marked "registry": the crates.io API or the GitHub REST API, checked 2026-10-21; see [References](#new-primary-sources-checked-for-this-document-on-2026-10-21)). The models in section 3.3 take their licences from the reports and [BUILD §4.2](BUILD.md#42-models-and-eileen-setup), and the Rust row from the rust-lang/rust README. Allowed licences for linked code are MIT, MIT-0, Apache-2.0 (with or without the LLVM exception), BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD, Unicode-3.0, Unlicense, CC0-1.0, BSL-1.0 and IJG, plus one scoped exception, CDLA-Permissive-2.0 for `webpki-roots` only (it arrives through `sherpa-onnx-sys`'s build-time download client); `openssl-sys` and `native-tls` are banned, and unknown registries, git sources and yanked versions are denied ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). No GPL component may ship in a release binary (owner answer 12, [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20)).

**Sources.** [BUILD §4.1](BUILD.md#41-dependency-pins), [§4.2](BUILD.md#42-models-and-eileen-setup), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20); the per-row sources below.

### 3.1 Language, toolchain and development tools

| What | Version | Licence | Chosen over, and why | Source |
|---|---|---|---|---|
| Rust (edition 2024, resolver 3) | 1.99.0, pinned in `rust-toolchain.toml` | MIT and Apache-2.0 (README of rust-lang/rust, checked 2026-10-21) | The Python core of D9: a stdlib-only Python shim handshook in 1.7 to 2.9 s against 37 to 69 s with the Python MCP SDK, and a bare interpreter took 4.5 to 6.9 s to start; the Rust skeleton was ready in 90 ms and idled at 26.6 MB | [rust-architecture §1](research/2026-10-rust-architecture.md#1-verdict), [§5](research/2026-10-rust-architecture.md#5-measurements); [D17](DECISIONS.md#d17-rust-core-with-supervised-sidecars), [D38](DECISIONS.md#d38-windows-first-not-windows-only-the-python-scaffold-is-retired) |
| cargo-nextest | 0.9.148 | Apache-2.0 OR MIT (registry) | `cargo test`: nextest adds a test group with one thread for desktop tests, a leak timeout that fails tests leaving children running, flaky-result reporting, partitioning and JUnit | [engineering-practice §2.5](research/2026-10-engineering-practice.md#25-testing-tools) |
| cargo-deny | 0.20.2 | MIT OR Apache-2.0 (registry) | Licence, advisory, ban and source policy in one tool; it cannot see native libraries or models, so the release lane lists those | [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci) |
| cargo-machete | 0.9.2 | MIT (registry) | cargo-udeps, which needs nightly to run | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20) |
| cargo-insta | 1.49.0 | Apache-2.0 (registry) | Snapshot review with `--unreferenced reject`, which ruff uses to reject orphaned snapshots | [engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project), [§2.5](research/2026-10-engineering-practice.md#25-testing-tools) |
| typos | 1.51.1 | MIT OR Apache-2.0 (registry) | No alternative recorded; the spelling half of check 5 | [BUILD §4.1](BUILD.md#41-dependency-pins) |
| sccache | 0.18.0, installed prebuilt by the owner | Apache-2.0 (registry) | A shared `CARGO_TARGET_DIR`, which Cargo locks for a whole build and so serialises agents; sccache gives reuse across worktrees without the lock | [rust-architecture §9](research/2026-10-rust-architecture.md#9-parallel-builds-on-this-16-gb-pc) |
| cargo-binstall | 1.26.0 | GPL-3.0-only (a tool run, never linked) | `cargo install` from source, which took 59 minutes for four tools | [BUILD §4.1](BUILD.md#41-dependency-pins); [rust-architecture §5](research/2026-10-rust-architecture.md#5-measurements) |
| zizmor, and its action | 1.31.1, action 0.7.1 | MIT (registry) | No workflow analysis at all; zizmor catches dangerous triggers, persisted credentials and template injection, as uv runs it | [engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project), [§2.5](research/2026-10-engineering-practice.md#25-testing-tools) |
| cargo-fuzz | 0.13.2, nightly pinned by date | MIT OR Apache-2.0 (registry) | bolero (an alternative harness, kept in reserve); cargo-fuzz runs only on Unix-like systems, so fuzzing runs on Linux | [engineering-practice §2.5](research/2026-10-engineering-practice.md#25-testing-tools) |
| cargo-mutants | 27.1.0 | MIT (registry) | Coverage alone, which shows what ran but not what is checked; `--in-diff` mutates only changed lines | [engineering-practice §2.5](research/2026-10-engineering-practice.md#25-testing-tools), [§3.9](research/2026-10-engineering-practice.md#39-mutation-testing-and-coverage) |
| cargo-llvm-cov | 0.9.1 | Apache-2.0 OR MIT (registry) | Works on the MSVC target and can measure coverage of an end-to-end run of real binaries | [engineering-practice §2.5](research/2026-10-engineering-practice.md#25-testing-tools) |
| cargo-modules | 0.28.0, CI only | MPL-2.0 (a tool, not linked) | No prebuilt release and it builds rust-analyzer libraries from source, so it runs only in nightly CI, never locally | [observability §6.3](research/2026-10-observability.md#63-tools-evaluated) |
| dump_syms, minidump-stackwalk | 2.3.9, 0.27.0 | Apache-2.0, MIT | Symbols and stack walks for the minidumps the guard writes | [BUILD §4.1](BUILD.md#41-dependency-pins); [observability §4.7](research/2026-10-observability.md#47-from-a-minidump-to-a-stack) |
| rumdl, lychee | WP0 pins the newest release at least 14 days old | MIT; Apache-2.0 OR MIT (registry) | Markdown lint and link checks in Rust, run offline on pull requests and online nightly | [BUILD §4.1](BUILD.md#41-dependency-pins), check 5 in [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci) |

### 3.2 Crates

| What | Version | Licence | Chosen over, and why | Source |
|---|---|---|---|---|
| tokio, tokio-util | 1.54.0, 0.7.20 | MIT | madsim (needs git patches of crates.io crates, which `deny.toml` forbids) and tokio's unstable `rng_seed` (it needs `--cfg tokio_unstable` for the whole build, a second target directory on a 16 GB PC); determinism comes from the sans-IO core instead | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20); [engineering-practice §2.4](research/2026-10-engineering-practice.md#24-simulation-and-concurrency-testing) |
| serde, serde_json, schemars | 1.0.229, 1.0.154, 1.2.3 | MIT OR Apache-2.0; MIT for schemars | schemars matches the major version the MCP SDK uses | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20) |
| ulid | 3.0.0 | MIT | No alternative recorded; ULIDs sort by time, and a type prefix makes every ID greppable | [observability §1](research/2026-10-observability.md#1-verdict), [§2.1](research/2026-10-observability.md#21-identifiers) |
| toml | 1.1.8 | MIT OR Apache-2.0 | YAML: `serde_yaml` is published as deprecated and `serde_yml` is unsound and unmaintained (RUSTSEC-2025-0068) | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20); [D29](DECISIONS.md#d29-settings-by-voice-in-versioned-toml); [RustSec RUSTSEC-2025-0068 (read 2026-10-20)](https://rustsec.org/advisories/RUSTSEC-2025-0068.html) |
| thiserror, anyhow | 2.0.21, 1.0.104 | MIT OR Apache-2.0 | No alternative recorded; typed error enums in libraries, `anyhow` in binaries only | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20) |
| tracing, tracing-subscriber, tracing-appender, tracing-error | 0.1.44, 0.3.23, 0.2.5, 0.2.1 | MIT | OpenTelemetry crates in the default build; those are an opt-in feature for v0.2 | [observability §1](research/2026-10-observability.md#1-verdict); [BUILD §4.1](BUILD.md#41-dependency-pins) |
| secrecy | 0.10.3 | Apache-2.0 OR MIT (registry) | Plain strings for secrets; `SecretString` prints a redacted marker in `Debug` | [observability §3.9](research/2026-10-observability.md#39-redaction-in-every-sink) |
| await-tree | 0.3.1 | Apache-2.0 (registry) | No async task dump; await-tree shows what each live task is waiting on in an incident | [observability §3.7](research/2026-10-observability.md#37-async-debugging-what-each-task-is-waiting-on); [await-tree (read 2026-10-20)](https://github.com/risingwavelabs/await-tree) |
| rusqlite (bundled SQLite) | 0.40.2 | MIT | sqlx 0.9.0 (async and macro-heavy for one local file); skill history in SQLite instead of a git repository per skill, which avoids `gix`'s compile cost and makes "forget" one transaction | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20); [D27](DECISIONS.md#d27-the-learning-loop) |
| blake3, zstd, hound | 1.8.7, 0.14.3, 3.5.1 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception; BSD-3-Clause; Apache-2.0 (registry) | A content-addressed blob store shared by recordings, incidents and trajectories, so nothing is stored twice | [observability §1](research/2026-10-observability.md#1-verdict), item 10 |
| rmcp, the official Rust MCP SDK | =3.6.0 | Apache-2.0 | The Python MCP SDK (import alone 35 to 42 s under load); a Rust server was ready 40 to 77 ms after spawn and idled at 4.9 MB | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20); [subscription-harnesses §5](research/2026-10-subscription-harnesses.md#5-driving-the-harnesses-from-rust) |
| agent-client-protocol | =3.4.0, declared for the ACP slot only | Apache-2.0 | The ACP adapters for the main routes, which are Node wrappers (about 99 MB idle and a second per session) around richer native protocols | [subscription-harnesses §1](research/2026-10-subscription-harnesses.md#1-verdict); [D19](DECISIONS.md#d19-workers-are-named-by-route-native-claude-driver-acp-as-a-slot) |
| process-wrap | 10.0.1 | Apache-2.0 OR MIT (registry) | The ACP crate's `process` helper, which does not job-wrap a child on Windows; process-wrap is what rmcp's child transport uses | [rust-architecture §1](research/2026-10-rust-architecture.md#1-verdict), [§10](research/2026-10-rust-architecture.md#10-pitfalls) |
| windows, windows-sys | 0.62.2, 0.61.2 | MIT OR Apache-2.0 | The focused `windows-core` 0.100 crates, which do not mix with the 0.62 umbrella crate; and the `uiautomation` crate, which initialises COM on every construction while Eileen owns one apartment thread | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20), [§10](research/2026-10-rust-architecture.md#10-pitfalls); [BUILD §4.1](BUILD.md#41-dependency-pins) |
| tray-icon, global-hotkey, tauri-winrt-notification | 0.26.1, 0.9.0, 0.8.1 | MIT OR Apache-2.0 | notify-rust (the cross-platform alternative) for toasts; tray-icon with default features off drops the Linux `libappindicator` dependency | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20), [§10](research/2026-10-rust-architecture.md#10-pitfalls) |
| keyring-core, windows-native-keyring-store | 1.0.1, 1.1.0 | MIT OR Apache-2.0 | `keyring` 4, which is now glue for its command-line tool | [rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20), [§10](research/2026-10-rust-architecture.md#10-pitfalls) |
| crash-handler, minidumper | 0.8.1, 0.11.0 | MIT OR Apache-2.0 | Writing dumps from inside the crashing process; a guard process writes them from outside, the pattern Zed ships (Zed's GPL code is a design reference only) | [observability §1](research/2026-10-observability.md#1-verdict), [§4.4](research/2026-10-observability.md#44-eileen-guardexe); [Embark crash-handling (read 2026-10-20)](https://github.com/EmbarkStudios/crash-handling) |
| cpal | 0.18.2 | Apache-2.0 (registry) | Measured on the PC: output callback every 10.1 ms of audio, a stop flag seen 1.5 ms after it was set | [voice-stack §1](research/2026-10-voice-stack.md#1-recommendation-the-v01-pipeline) |
| sonora (WebRTC AEC3 in Rust) | 0.2.0 | BSD-3-Clause | pywebrtc-audio (Python); Windows' own Communications-category echo cancellation stays a setting to compare with real audio | [voice-stack §1](research/2026-10-voice-stack.md#1-recommendation-the-v01-pipeline), [§3](research/2026-10-voice-stack.md#3-audio-io-echo-cancellation-and-barge-in) |
| sherpa-onnx (shared build) | =1.13.8 | Apache-2.0 | The static build, which fails to link on MSVC 14.42; Eileen's release build of it drops espeak-ng so no GPL ships | [BUILD §4.1](BUILD.md#41-dependency-pins); [D24](DECISIONS.md#d24-a-local-voice-pipeline-in-rust-cpu-only), [D58](DECISIONS.md#d58-the-owners-answers-of-2026-10-20-applied) |
| ort | =2.0.0-rc.13 | MIT OR Apache-2.0 (registry) | rc.14 and rc.15, which the pins call too new; one shipped `onnxruntime.dll` 1.28.2 for every model | [BUILD §4.1](BUILD.md#41-dependency-pins); [voice-stack §8](research/2026-10-voice-stack.md#8-onnxruntime-how-it-is-linked-and-shipped) |
| realfft, rtrb | 3.5.0; newest non-yanked release, pinned by WP0 | MIT; MIT OR Apache-2.0 (registry) | The log-mel features for turn detection, ported to Rust and matching the reference within 1.1e-5; a real-time-safe ring between the audio thread and the core | [voice-stack §1](research/2026-10-voice-stack.md#1-recommendation-the-v01-pipeline); [BUILD §4.1](BUILD.md#41-dependency-pins) |
| image, fast_image_resize, jpeg-encoder, base64 | 0.25.10, 6.1.0, 0.7.1, 0.23.1 | MIT OR Apache-2.0; jpeg-encoder is (MIT OR Apache-2.0) AND IJG, allowed in `deny.toml` with attribution in `NOTICE` (registry) | `xcap`, which gave no gain over Eileen's own capture code | [BUILD §4.1](BUILD.md#41-dependency-pins) |
| clap, syn (tools only) | 4.6.7; newest 2.x at least 14 days old | MIT OR Apache-2.0 | `syn` parses the tree for repository checks without a build, and sees `cfg(windows)` modules on Linux | [BUILD §4.1](BUILD.md#41-dependency-pins); [engineering-practice §5.3](research/2026-10-engineering-practice.md#53-generated-code-map-and-event-map) |
| insta, proptest, proptest-state-machine, arbitrary | 1.49.0, 1.12.0, 0.9.0, 1.5.0 | Apache-2.0; MIT OR Apache-2.0 (registry for the last two) | expect-test (insta was already chosen); `arbitrary` lets one generator be a seeded test, a fuzz target and a minimiser | [engineering-practice §1](research/2026-10-engineering-practice.md#1-verdict), [§2.5](research/2026-10-engineering-practice.md#25-testing-tools) |
| loom, shuttle, shuttle-tokio | 0.7.2; 0.9.7 and 1.0.0 (0.9.8 was still inside its cooling-off period on 2026-10-20) | MIT; Apache-2.0 (registry) | turmoil (simulates TCP and UDP hosts, while Eileen talks over stdio and pipes; kept for later network connectors) | [engineering-practice §2.4](research/2026-10-engineering-practice.md#24-simulation-and-concurrency-testing); [BUILD §4.1](BUILD.md#41-dependency-pins) |

### 3.3 Models

| What | Version or file | Licence | Chosen over, and why | Source |
|---|---|---|---|---|
| Silero VAD | v6.2, 2.3 MB | MIT | The older v4 asset; idle listening cost 5.8 percent of one logical core | [voice-stack §4](research/2026-10-voice-stack.md#4-voice-activity-and-speech-recognition) |
| Nemotron speech streaming EN 0.6B | 560 ms chunks, int8 | NVIDIA Open Model License (accepted by the owner, with an open model selectable) | Parakeet TDT (heard "Eileen" as "Clean"), Kroko (8.2 percent WER), faster-whisper (1.3 s per segment whatever its length), Moonshine (slower than real time under load); Nemotron gave 2.35 percent WER and a final 0.4 s after speech ended | [voice-stack §4](research/2026-10-voice-stack.md#4-voice-activity-and-speech-recognition); owner answer 13 in [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20) |
| Smart Turn | v3.2 CPU, 8.7 MB | BSD-2-Clause | Silence timeouts alone; fused with transcript cues and a silence cap, because its accuracy on synthetic cuts was weak | [voice-stack §1](research/2026-10-voice-stack.md#1-recommendation-the-v01-pipeline), [§5](research/2026-10-voice-stack.md#5-end-of-turn-and-addressee-detection) |
| Kokoro | v1.0 fp32 | Apache-2.0 | The int8 export (13.5 to 42.6 real-time factor in one run, with EcoQoS on and two threads) and Piper (GPL-3.0, no cloning) | [voice-stack §6](research/2026-10-voice-stack.md#6-speech-synthesis); [dictation-and-voices §10](research/2026-10-dictation-and-voices.md#10-voices-the-field-on-2026-10-20) |
| Pocket TTS (cloning default) | int8 export of 2026-01-26 | MIT code, CC-BY-4.0 weights; the archive's README line about non-commercial use is confirmed with the maintainers before a downloader ships | Chatterbox-Turbo as the default cloner (7.5 GB peak and 13.4 CPU core-hours per hour of audio in a vendor-run benchmark, against 610 MB and 0.37 for Pocket TTS) | [dictation-and-voices §1](research/2026-10-dictation-and-voices.md#1-recommendations-in-brief), [§10](research/2026-10-dictation-and-voices.md#10-voices-the-field-on-2026-10-20); [D25](DECISIONS.md#d25-voices-kokoro-default-pocket-tts-for-cloning-no-watermark-by-default); [Picovoice benchmark, secondary (read 2026-10-20)](https://picovoice.ai/blog/on-device-tts/) |
| Chatterbox-Turbo and Chatterbox-Nano | official ONNX graphs; a community ONNX export | MIT for both models; the Nano community export's own licence is checked by WP4 and confirmed with its maintainer before a downloader ships (owner answer 14) | Selectable per voice, live only when it measures fast enough on the PC; a head-to-head against Pocket TTS is part of WP4's acceptance | [BUILD §4.2](BUILD.md#42-models-and-eileen-setup); [D39](DECISIONS.md#d39-voices-the-engine-is-chosen-per-voice-and-chatterbox-is-in-v01); [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20) |
| Parakeet TDT 0.6B v3 | int8 | CC-BY-4.0 | Running it beside Nemotron all the time (1.48 GB together); loaded on demand for multilingual dictation | [voice-stack §4](research/2026-10-voice-stack.md#4-voice-activity-and-speech-recognition) |
| WeSpeaker CAM++, sentence and static embedding candidates | per BUILD §4.2 | verified by WP4 before adoption | The embedding candidates are adopted only if each meets its latency budget on the PC; CAM++ gates a command only if it clears the speaker check's error bar ([§11.5](#115-a-voice-check-gates-nothing-until-it-clears-a-measured-bar)), and otherwise serves only voice enrollment's same-speaker check | [BUILD §4.2](BUILD.md#42-models-and-eileen-setup), [§2.8](BUILD.md#28-voice); [D28](DECISIONS.md#d28-a-layered-intent-resolver), [D76](DECISIONS.md#d76-fit-the-machine-and-gate-on-the-speaker-only-above-a-measured-bar) |
| YuNet face detector | OpenCV Zoo ONNX | MIT | A model call for finding faces in a still; local and well under 1 MB | [BUILD §4.2](BUILD.md#42-models-and-eileen-setup) |
| AudioSeal (opt-in watermark) | ONNX export published as a release asset | MIT | No watermark by default on any engine (owner decision); AudioSeal for users who turn it on for exported audio | [D58](DECISIONS.md#d58-the-owners-answers-of-2026-10-20-applied); [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20) |

Every model is listed with URL, size, SHA256, licence and residency in `models.toml`; `eileen setup` shows every licence and the total size and asks once, then verifies each hash ([BUILD §4.2](BUILD.md#42-models-and-eileen-setup)).

### 3.4 Programs and services

| What | Version | Licence or terms | Chosen over, and why | Source |
|---|---|---|---|---|
| The Claude CLI (`claude -p`, stream-json), unmodified, signed in by the user | tested on 2.1.304; pinned and latest both run nightly | Anthropic's terms: the end user signs in to the unmodified binary with their own plan | The Python Agent SDK (57 MB idle, 1.6 to 6.1 s to connect) and the ACP adapter (99 MB, its own bundled binary); a warm turn reached first text in 1.0 to 1.5 s in the better of two connectors-off runs (2.2 to 4.2 s in the other) | [subscription-harnesses §5](research/2026-10-subscription-harnesses.md#5-driving-the-harnesses-from-rust); [brain-latency](research/2026-10-brain-latency.md#results); [D18](DECISIONS.md#d18-subscriptions-only-claude-is-the-v01-route); [legal and compliance (read 2026-10-20)](https://code.claude.com/docs/en/legal-and-compliance) |
| Codex app-server, Grok over ACP, DeepSeek's Anthropic-compatible API | slots in v0.1, built in v0.2 | each vendor's terms; DeepSeek's key is the only API key | Gemini, excluded: Google's consumer terms bar third-party agents and the owner allows no Gemini key | [subscription-harnesses §1](research/2026-10-subscription-harnesses.md#1-verdict), [§3](research/2026-10-subscription-harnesses.md#3-policy-vendor-by-vendor); [D18](DECISIONS.md#d18-subscriptions-only-claude-is-the-v01-route), [D19](DECISIONS.md#d19-workers-are-named-by-route-native-claude-driver-acp-as-a-slot) |
| Eileen's own desktop server | ported from a proof of 1,720 library lines plus a 389-line server | MIT (this repository) | Cua Driver 0.37.0: 63 tools in a 216 KB list against 12 tools in 8.8 KB, about 95 MB of binaries, telemetry on by default, and no password-field or blocked-combo refusals | [computer-use §1](research/2026-10-computer-use.md#1-verdict); [D21](DECISIONS.md#d21-eileens-own-rust-desktop-server-is-the-windows-action-layer); [Cua release (read 2026-10-20)](https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.37.0) |
| DaVinci Resolve: its own MCP server on 21.1 and later (secondary in app-hooks §0 and §1), a Lua bridge under Resolve's `fuscript.exe` as fallback | owner upgrading to 21.1 | Blackmagic's software; the bridge is MIT | A Python fusionscript sidecar; the Lua bridge answered in 0.51 to 0.58 ms p50 from a Rust supervisor under load and needs no Python | [app-hooks §0](research/2026-10-app-hooks.md#0-what-changed-between-2026-10-11-and-2026-10-20), [§1](research/2026-10-app-hooks.md#1-summary); [D23](DECISIONS.md#d23-app-hooks-are-resident-rust-resolve-through-a-lua-bridge), [D74](DECISIONS.md#d74-the-owners-answers-applied-in-full) |
| Windows system media controls, Office and Explorer COM, URL launch | Windows 11 APIs | Windows | Screenshots and clicks for media and documents; a `&t=930s` URL landed at 15:30, and SMTC seek from Rust took effect on a test session; on YouTube an ad can follow a seek, so success is checked afterwards | [app-hooks §0](research/2026-10-app-hooks.md#0-what-changed-between-2026-10-11-and-2026-10-20), [§1](research/2026-10-app-hooks.md#1-summary), [§5](research/2026-10-app-hooks.md#go-to-a-specific-video) ("Go to a specific video"); [D22](DECISIONS.md#d22-browser-and-media-url-launch-and-smtc-first) |
| Handy and SpeakoFlow's cleanup rules (ported, with attribution) | Handy 0.9.8; rules with a 79-case conformance suite | MIT | Embedding another dictation app, and the GPL and AGPL dictation projects, which are design references only | [dictation-and-voices §1](research/2026-10-dictation-and-voices.md#1-recommendations-in-brief); [D26](DECISIONS.md#d26-dictation-is-built-into-eileen); [Handy (read 2026-10-20)](https://github.com/cjpais/Handy) |
| The official MCP registry, queried by Eileen's own code | n/a | public registry | Letting a worker search the web for MCP servers; a proposal must match a registry entry and is installed once at its exact version after the user's approval | [BUILD §2.5](BUILD.md#25-workers-routing-and-jobs); [D59](DECISIONS.md#d59-app-routes-discovery-by-eileens-own-tool-remembered-per-app-and-shown-without-hijacking), [D66](DECISIONS.md#d66-containment-and-route-proposals-third-revision) |
| GitHub: Actions (hosted runners), Pages, Dependabot, the `gh` CLI on the contributor's own sign-in | n/a | GitHub's terms | A self-hosted runner on the owner's PC, which a fork's pull request could use to run code on it | [BUILD §4.6](BUILD.md#46-what-runs-where), [§4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet); [engineering-practice §6.7](research/2026-10-engineering-practice.md#67-the-owners-pc-lane) |
| Google Colab, through Google's Colab CLI or Colab MCP server (v0.2 only) | CLI 0.7.5; MCP server 1.0.1 (in its `pyproject.toml`) | Apache-2.0 | Chosen at v0.2 design time with a live test; the CLI is Linux and macOS only and the MCP server has been quiet since April | [colab-ml-projects §1](research/2026-10-colab-ml-projects.md#1-verdict), [§2.2](research/2026-10-colab-ml-projects.md#22-the-colab-mcp-server-colab-mcp); [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20) |

### 3.5 CI actions

All pinned by commit SHA with the version in a comment; Dependabot keeps the SHAs current ([BUILD §4.1](BUILD.md#41-dependency-pins), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). Licences are as GitHub's API detects them (registry).

| Action | Version | Licence | Use |
|---|---|---|---|
| `actions/checkout` | v7.0.1 | MIT | Checkout with `persist-credentials: false` |
| `Swatinem/rust-cache` | v2.9.3 | LGPL-3.0 (an action run in CI, never linked) | Dependency caches per job type, saved only from `main` |
| `taiki-e/install-action` | v2.87.29 | Apache-2.0 | Prebuilt tools at exact versions |
| `actions/upload-artifact` | v7.0.2 | MIT | JUnit, pending snapshots, fuzz corpora |
| `dorny/paths-filter` | v4.0.1 | MIT | The `plan` job's path classes |
| `EmbarkStudios/cargo-deny-action` | v2.1.1 | Apache-2.0 | Licence and advisory policy |
| `crate-ci/typos` | v1.51.1 | Apache-2.0 | Spelling |
| `zizmorcore/zizmor-action` | v0.7.1 | MIT | Workflow analysis |

### 3.6 Deliberately not used

`chacha20poly1305` and DPAPI wrapping (encryption at rest waits for v0.2, [D40](DECISIONS.md#d40-what-contains-a-worker-in-v01-and-what-waits-for-v02)); `uiautomation` (COM per construction); `nucleo-matcher` (MPL-2.0; Eileen writes its own scorer); `xcap`; `gix` (skill history is in SQLite); `serde_yaml` and `serde_yml`; `hyper` (the loopback endpoint is a small hand-written HTTP/1.1 server with strict limits and a fuzz target); the OpenTelemetry crates (v0.2, behind a feature) ([BUILD §4.1](BUILD.md#41-dependency-pins)). Code-navigation tools rejected for agents: Serena (GPL-3.0-or-later), GitNexus (PolyForm Noncommercial) and Sourcegraph (needs a server) ([observability §6.3](research/2026-10-observability.md#63-tools-evaluated)). Any new dependency is an architectural change that the pull request names ([BUILD §4.1](BUILD.md#41-dependency-pins)).

## 4. CI and CD

The design is in [BUILD §4.6](BUILD.md#46-what-runs-where) and [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci), started from the skeleton in [engineering-practice §6](research/2026-10-engineering-practice.md#6-ci-on-github-actions).

### 4.1 Lanes

**Practice.** Split checks by what they cost and what they prove. The pull request lane is deterministic and fast and is the only gate. The main branch adds randomness and catches flakes. Nightly and weekly lanes get the long fuzzing, simulation, soak and mutation runs, and their failures become issues, not red pull requests. Releases rerun everything on the tagged commit.

**Why it works.** Across rust-analyzer, ruff, uv, Turso, tokio, TigerBeetle and Firecracker the survey found a common pattern, each project showing part of it: a fast deterministic gate, a main-branch lane that adds randomness, and dedicated time for long runs whose failures are tracked as issues ([engineering-practice §2.6](research/2026-10-engineering-practice.md#26-how-the-best-projects-split-pr-checks-from-slow-suites)). Running the full suite on `main` and keeping a list of its failures turns flakes into a tracked list instead of noise ([Catch Flakes On Main (read 2026-10-20)](https://matklad.github.io/2026/05/14/catch-flakes-on-main.html)).

**Adopt it.** Start with a `plan` job that classifies the pull request by the paths it touches and lets untouched jobs skip cleanly. Have any change to the manifests, lockfile, toolchain or workflows select every class. Give each lane a time budget and a job that opens an issue when the budget is exceeded. Provide an on-demand lane (manual dispatch or labels) for long runs a package wants to link as evidence.

**In Eileen.** The `plan` job's classes are `docs-only`, `scenarios-only`, `neutral-rust`, `windows-rust`, `plugin` and `workflow` ([BUILD §4.6](BUILD.md#46-what-runs-where)). The lanes:

| Lane | What runs | Budget |
|---|---|---|
| Every push to a pull request | Linux: formatting, typos, cargo-deny, cargo-machete, zizmor, the repository checks, generated-file freshness, Markdown lint and internal links, clippy and tests of neutral crates, simulator seeds and stored regressions, exhaustive turn-taking to depth 6, fuzz corpus replay, scenario validation and replay; Windows: clippy with warnings as errors, tests of Windows crates, doc tests, end-to-end smoke with a stub harness; plus `pr-meta`, `evidence` and an advisory `cargo mutants --in-diff` | under about 15 minutes with a warm cache |
| Merge to `main` | Full Windows tests, random simulator seeds for 10 minutes, a thin-LTO release smoke build, cache saves | 20 minutes |
| Nightly | Simulator seeds for 30 minutes per profile, fuzzing 10 minutes per target, exhaustive depth 8, the full end-to-end set, a 30-minute soak, a flake hunt, coverage, external links, CLI drift, fleet ingest, the Pages site; one issue per failing job | about 1 hour per job |
| On demand (`long.yml`) | Chosen fuzz targets, simulator profiles and soak hours on a ref, or labels `fuzz-10m` and `sim-30m` on a pull request | as requested |
| Weekly | Full mutation run in four shards, a 4-hour soak, an hour per hot fuzz target, Dependabot's grouped updates, and an issue if the pull request lane's p50 passes 15 minutes | hours |
| Release (tag `v*`) | Everything on the tagged commit, a fat-LTO build in CI, symbols, checksums, a licence listing that fails on any GPL component, end-to-end smoke from the built artifacts, a draft release the owner publishes | owner signs off |

The source for the table is [BUILD §4.6](BUILD.md#46-what-runs-where). Unlike the engineering-practice report's first proposal, the merge to `main` does not repeat the pull request lane, because branches must be up to date before they merge ([§4.4](#44-branch-protection-without-a-merge-queue); [engineering-practice §3.10](research/2026-10-engineering-practice.md#310-what-runs-when)).

**Sources.** [BUILD §4.6](BUILD.md#46-what-runs-where), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [engineering-practice §2.6](research/2026-10-engineering-practice.md#26-how-the-best-projects-split-pr-checks-from-slow-suites), [§6.5](research/2026-10-engineering-practice.md#65-nightly-and-weekly-lanes).

### 4.2 One aggregate required check

**Practice.** Make one job that needs every blocking job, runs even when they fail or skip, and fails unless each succeeded or was skipped. Require only that job (plus checks that must re-run on their own events) in branch protection.

**Why it works.** GitHub counts a skipped job as success, so a required job that was skipped because something it needs failed still passes; an aggregate that runs always and inspects every result is correct either way, and adding or removing a job never touches the branch settings. rust-analyzer calls this job `conclusion` and uv calls it `required-checks-passed` ([engineering-practice §6.2](research/2026-10-engineering-practice.md#62-pr-lane); [rust-analyzer CI (read 2026-10-20)](https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/ci.yaml); [uv CI (read 2026-10-20)](https://github.com/astral-sh/uv/blob/main/.github/workflows/ci.yml)).

**Adopt it.** Write the aggregate with `if: always()` and pass `toJson(needs)` through an environment variable so the script contains no template expression, then check it with `jq --exit-status 'all(.result == "success" or .result == "skipped")'` ([engineering-practice §6.2](research/2026-10-engineering-practice.md#62-pr-lane)). Put checks that read the pull request description in their own workflows triggered on `edited`, `labeled` and `unlabeled`, so editing the description re-runs them without re-running the heavy jobs. When you replace an old CI, switch the required checks in the same pull request, after the new check has reported, or every later pull request waits forever.

**In Eileen.** `all-green` in `ci.yml`, plus `pr-meta` and `evidence` in their own workflows, are the required checks on `main` ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). The WP0 bootstrap pull request lets `all-green` and `pr-meta` report while the old Python jobs still run, then switches the required checks with a recorded `gh api` command, then deletes the old scaffold, in that order ([BUILD §6.2](BUILD.md#62-wp0-foundation-workspace-contracts-and-quality-gates); [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20), "Repository settings").

**Sources.** [engineering-practice §6.2](research/2026-10-engineering-practice.md#62-pr-lane); [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci), [§6.2](BUILD.md#62-wp0-foundation-workspace-contracts-and-quality-gates); [D32](DECISIONS.md#d32-conventions-are-enforced-by-ci-with-one-local-command).

### 4.3 Caching and supply chain

**Practice.** Save caches only from the main branch and restore them everywhere else, with one key per job type. Never cache what must be verified (downloaded models). Pin every action by commit SHA, run workflows with read-only permissions by default, do not persist checkout credentials, analyse the workflows, and let a bot propose updates after a cooling-off period.

**Why it works.** Pull requests that only read `main`'s caches do not spend the repository's cache allowance, and clippy and test artifacts differ, so they cannot share a key ([engineering-practice §6.3](research/2026-10-engineering-practice.md#63-caching-and-supply-chain)). A cooling-off period on new versions is how wezterm avoids adopting a bad release on day one ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project); [wezterm cooldown workflow (read 2026-10-20)](https://github.com/wezterm/wezterm/blob/main/.github/workflows/cargo-cooldown.yml)); Dependabot now has a `cooldown` option that delays version updates by a configurable number of days ([Dependabot options reference (checked 2026-10-21)](https://docs.github.com/en/code-security/dependabot/working-with-dependabot/dependabot-options-reference)).

**Adopt it.** Budget the cache explicitly (GitHub gives a repository 10 GB). Let nightly jobs restore and never save. Install tools prebuilt at exact versions. Do not let any workflow open pull requests with the workflow token: such a pull request starts no workflows and can never pass required checks.

**In Eileen.** Windows keeps two cache keys (`win-clippy`, `win-test`), Linux one, fuzzing one; models are downloaded from their pinned URLs and verified, never cached ([BUILD §4.6](BUILD.md#46-what-runs-where), "Cache budget"). Every workflow starts with `permissions: contents: read`, a job that writes names only what it needs, actions are pinned by SHA, and zizmor runs on every change (check 20) ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). Dependabot opens grouped weekly `cargo` and `github-actions` updates with a 14-day cooldown; the protocol crates (`rmcp`, the ACP crate, `sherpa-onnx`, `ort`) are ignored and updated by hand with probe subcommands as smoke tests ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [D63](DECISIONS.md#d63-evidence-has-a-grammar-and-its-own-required-check-dependabot-updates-cargo-scenario-keys-are-stable)).

**Sources.** [BUILD §4.6](BUILD.md#46-what-runs-where), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [engineering-practice §6.3](research/2026-10-engineering-practice.md#63-caching-and-supply-chain).

### 4.4 Branch protection without a merge queue

**Practice.** Protect the main branch: changes only through pull requests, required checks on a branch that is up to date with `main`, the rule applied to admins, linear history, no force pushes or deletion, review conversations resolved. Without a merge queue, every open pull request is brought up to date after each merge. Push continuously: a draft pull request from the first push, small pull requests, merge only on green.

**Why it works.** GitHub's merge queue is only available to organisation-owned repositories, and without it two pull requests can each pass alone and break `main` together ([engineering-practice §6.8](research/2026-10-engineering-practice.md#68-branch-protection-and-continuous-pushing)). GitHub's page on managing a merge queue says queues are "available in any public repository owned by an organization", and in private ones only on GitHub Enterprise Cloud ([GitHub merge queue documentation (read 2026-10-20; availability note checked 2026-10-21)](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)). "Strict" required status checks require the branch to be up to date with the base branch before merging, at the cost of more builds ([GitHub, about protected branches (checked 2026-10-21)](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)). That is always correct, and the cost is predictable.

**Adopt it.** If you can use an organisation, enable the merge queue and add `merge_group` to CI. If not, use strict checks, budget one extra pull request lane run per merge for each open pull request, and enable auto-merge so an agent can open a pull request and move on. Keep logic pull requests under about 500 changed lines.

**In Eileen.** The owner approved and applied these settings on 2026-10-20, staying on a personal account with no merge queue ([OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20), "Repository settings"). Every package agent updates its open pull request after each merge to `main` and expects the lane to run again (about 15 minutes); auto-merge waits for the update but does not perform it ([BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it), rule 14). Packages open a draft pull request from the first push, keep logic changes under about 500 lines and merge only with `all-green` and `pr-meta` ([BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it); [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20), "Work is pushed continuously").

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [BUILD §6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it); [engineering-practice §6.8](research/2026-10-engineering-practice.md#68-branch-protection-and-continuous-pushing).

### 4.5 Release channels

**Practice.** Publish three channels from the start: nightly prebuilt builds for testers, betas as pre-releases, and stable releases that a person publishes. Build releases only in CI. Ship symbols and checksums, and fail the release on a licence the project does not allow.

**Why it works.** Testers without a toolchain can run the nightly build, which turns friends into a test fleet early ([§5.5](#55-the-community-test-fleet)). Building releases in CI keeps fat-LTO builds off a machine that cannot afford them ([rust-architecture §9](research/2026-10-rust-architecture.md#9-parallel-builds-on-this-16-gb-pc), item 8). Publishing stays a consequential action that a person takes ([engineering-practice §6.6](research/2026-10-engineering-practice.md#66-release-lane)).

**Adopt it.** Tag-driven releases, `v*-beta.*` tags as pre-releases, a nightly job that publishes an unsigned zip as a "nightly" pre-release, and a draft stable release the owner publishes. Add signed automatic updates with one-step rollback once you have a signing certificate.

**In Eileen.** Exactly the three channels above in v0.1, with the nightly prebuilt from owner answer 5; signed automatic updates with one-step rollback wait for the certificate in v0.2 ([BUILD §4.6](BUILD.md#46-what-runs-where); [ROADMAP](ROADMAP.md#v02); [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20), "Releases" and answer 5). The release lane refuses to package the upstream sherpa-onnx archive, which contains espeak-ng, and uses Eileen's own build without it ([BUILD §4.1](BUILD.md#41-dependency-pins)).

**Sources.** [BUILD §4.6](BUILD.md#46-what-runs-where); [ROADMAP](ROADMAP.md#v02); [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20).

### 4.6 One local command, and the build guard

**Practice.** One local command runs exactly what the pull request lane runs, through the build guard, and prints the CI-only steps it skipped and why. A faster command runs only the repository checks in seconds, and a pre-push hook runs the changed crates' checks.

**Why it works.** TigerBeetle has one CI entry point, `zig build ci`, that people and CI both run ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project)); its hacking guide names that command as the CI entry point ([TigerBeetle hacking guide (checked 2026-10-21)](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/HACKING.md)). TigerBeetle retired its GitHub `ci.yml` on 2026-07-08 and runs the command through its merge queue ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project), [§2.6](research/2026-10-engineering-practice.md#26-how-the-best-projects-split-pr-checks-from-slow-suites)). When the local command and CI agree, a failure is reproducible before pushing, and a memory-limited machine is protected because the one command goes through the guard ([§1.10](#110-memory-limits-on-parallel-agents)).

**Adopt it.** Implement the command as an `xtask` binary with few dependencies so it compiles in seconds. Make every subcommand that runs a compiler go through the guard, and make the checks print the check id, the file and line, the fix and the reproducing command.

**In Eileen.** `cargo xtask ci`, `cargo xtask lint-repo`, `cargo xtask pr-meta --body-file <file>`, `cargo xtask evidence` and `cargo xtask pre-push`, with the hook installed by `cargo xtask setup` ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci), "One command"; [BUILD §4b.5](BUILD.md#4b5-cargo-xtask); [D32](DECISIONS.md#d32-conventions-are-enforced-by-ci-with-one-local-command)). The owner's PC is never a self-hosted runner for this public repository ([BUILD §4.6](BUILD.md#46-what-runs-where)).

**Sources.** [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci), [§4b.5](BUILD.md#4b5-cargo-xtask); [D32](DECISIONS.md#d32-conventions-are-enforced-by-ci-with-one-local-command).

## 5. Testing tactics

The layers, from [BUILD §4.7](BUILD.md#47-test-strategy) (after [engineering-practice §3.1](research/2026-10-engineering-practice.md#31-layers)):

| Layer | Proves | Tool | Runs on |
|---|---|---|---|
| Unit and property | One module, including invalid input | `#[test]`, proptest | every OS |
| Snapshot | Wire formats, tool schemas, HUD draw lists, user-facing text | insta | every OS |
| Integration per crate | A crate through its public API, one test binary at `tests/it/main.rs` | nextest | every OS for neutral crates |
| Exhaustive | Turn-taking and barge-in for every input sequence up to a length | own enumerator | Linux |
| Deterministic simulation | The whole core under random inputs and faults, checked by invariants, replayable from a seed | `eileen-sim` | Linux on pull requests, both OSes nightly |
| Concurrency | The async glue and the audio ring buffer | paused tokio time, shuttle, loom | Linux |
| Fuzzing | Untrusted parsers never panic, stay bounded, round-trip | cargo-fuzz, corpus replay | fuzzing on Linux, replay everywhere |
| End to end | Real processes, real desktop, recorded audio in, event trace out | scenario runner | hosted Windows with stubs; real PCs with real apps |
| Timed scenarios | The owner's requests stay fast | A/B against the base binary | the owner's PC |
| Soak | Memory, handles, threads, idle CPU, orphans, store sizes | `cargo xtask soak` | Windows nightly and weekly |
| Mutation and coverage | Tests check what they cover | cargo-mutants, cargo-llvm-cov | Linux |

One integration test binary per crate is matklad's advice, because each file in `tests/` links separately and linking is slow on MSVC ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project), [§6.4](research/2026-10-engineering-practice.md#64-keeping-windows-under-about-15-minutes); [Delete Cargo Integration Tests (read 2026-10-20)](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html)).

### 5.1 Deterministic simulation from seeds

**Practice.** A development-only simulator owns a world of fake parts around the real core, seeds a random generator, generates inputs, steps the core, applies its effects to the world and checks every invariant after every step. Fault points in the core fire only under the simulator. Pull requests run a fixed seed set; `main` and nightly run fresh seeds. A failure is minimised and stored as a decoded input script, not as a seed.

**Why it works.** This is how TigerBeetle, FoundationDB and Turso make concurrency bugs reproducible ([engineering-practice §2.4](research/2026-10-engineering-practice.md#24-simulation-and-concurrency-testing)). FoundationDB activates each fault site for a whole run with probability 0.25 and then fires it with probability 0.25, which gives swarm testing for free ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project); [Buggify.h (read 2026-10-20)](https://github.com/apple/foundationdb/blob/main/flow/include/flow/Buggify.h)). Turso's CI keeps seeds fixed on pull requests, so a green pull request stays green and a red one reproduces exactly, while new seeds are leads to investigate ([engineering-practice §1](research/2026-10-engineering-practice.md#1-verdict); [Turso CI (read 2026-10-20)](https://github.com/tursodatabase/turso/blob/main/.github/workflows/rust.yml)). Seeds change meaning when the generator changes, which is why only decoded scripts are durable regressions ([engineering-practice §1](research/2026-10-engineering-practice.md#1-verdict), item 2). Generating inputs from a byte buffer makes the same generator a seeded test, a coverage-guided fuzz target and a minimiser ([Test Case Minimization (read 2026-10-20)](https://matklad.github.io/2026/04/20/test-case-minimization.html)).

**Adopt it.** Name every invariant as a function so a failure says what broke. Print the replay command, with the commit, on failure. Add a `--doublecheck` mode that runs a seed twice and compares trace hashes, which catches clock, randomness or map-order leaks into the core. Enumerate small state machines exhaustively to a fixed depth. Run the simulator on a thread with an explicit large stack: Turso's Windows simulator job is commented out beside a note about runs blowing the stack, and the Windows main thread's default stack is smaller ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project), [§3.3](research/2026-10-engineering-practice.md#33-the-simulator-eileen-sim); inferred).

**In Eileen.** `eileen-sim` simulates speech with several sources, playback in 10 ms callbacks, physical input, the brain with delay distributions and failures, workers from stub scripts with faults, sidecars that hang past their deadline, Job Object semantics, memory pressure and the stores; it checks fifteen invariants after every step, among them that the voice path is never starved, that barge-in takes no more than 150 ms of virtual time from speech onset to silence, that a tool call above tier 1 is always preceded by an approval built from user input, and that the user never hears two answers to one turn; regressions live in `crates/eileen-sim/regressions/` and run on every pull request; exhaustive tests cover turn-taking to depth 6 on pull requests and 8 nightly ([BUILD §4.7](BUILD.md#47-test-strategy); [D31](DECISIONS.md#d31-a-sans-io-core-tested-by-deterministic-simulation)). madsim and turmoil were judged and not used as the main tool ([engineering-practice §2.4](research/2026-10-engineering-practice.md#24-simulation-and-concurrency-testing)).

**Sources.** [engineering-practice §1](research/2026-10-engineering-practice.md#1-verdict), [§2.4](research/2026-10-engineering-practice.md#24-simulation-and-concurrency-testing), [§3.3](research/2026-10-engineering-practice.md#33-the-simulator-eileen-sim); [BUILD §4.7](BUILD.md#47-test-strategy); [D31](DECISIONS.md#d31-a-sans-io-core-tested-by-deterministic-simulation).

### 5.2 Replay modes

**Practice.** Record what entered the core and replay it two ways: strictly, on the commit that recorded it, reporting the first divergent event (for incidents); and in scenario mode, against a fresh core with model and tool responses served reactively and IDs remapped, so a recording survives code changes (for the test corpus). Add a level that replays recorded audio into the real speech process, and a manual live re-run for the rest.

**Why it works.** A strict replay tells a logic bug (it diverges or reproduces) from a model, desktop or timing problem (it replays clean), because the model's choices are replayed as recorded ([observability §5.3](research/2026-10-observability.md#53-replay-levels)). Open-loop replay broke on any change that minted one more ID, which is why scenario mode serves responses reactively ([D45](DECISIONS.md#d45-replay-and-scenarios-two-modes-formats-as-contracts-shipped-crates)). Temporal's replay tests rest on the same idea [secondary] ([observability §5.1](research/2026-10-observability.md#51-the-principle-a-closed-set-of-inputs)).

**Adopt it.** Make the recording, checkpoint, stub-script, regression and expected-trace formats contracts with fuzz targets. Record the answers the core gave to gating hooks beside the hook inputs, so a changed decision fails the replay. Ship the replayer in the product, not only in tests, so users can replay an incident.

**In Eileen.** `eileen replay inc_... --level core` and `--mode strict|scenario`, with formats in `eileen-contracts::{record, scenario, fleet}` and hook answers recorded as their own lines ([BUILD §2.13](BUILD.md#213-observability), [§3.3.16](BUILD.md#3316-recording-replay-and-fleet-formats); [D45](DECISIONS.md#d45-replay-and-scenarios-two-modes-formats-as-contracts-shipped-crates), [D72](DECISIONS.md#d72-hook-answers-are-part-of-a-recording)).

**Sources.** [observability §5](research/2026-10-observability.md#5-record-and-replay); [BUILD §3.3.16](BUILD.md#3316-recording-replay-and-fleet-formats); [D45](DECISIONS.md#d45-replay-and-scenarios-two-modes-formats-as-contracts-shipped-crates), [D72](DECISIONS.md#d72-hook-answers-are-part-of-a-recording).

### 5.3 Scenarios from real use

**Practice.** Let any user turn the request they just made, success or failure, into a scenario file: the words as text, the starting state as generic fixtures, every step taken, and expectations inferred from what changed. Before anything leaves the machine, generalise and redact it, replay the redacted copy locally to prove it still tests the same thing, and show the contributor exactly what will be shared. The contributor submits it from their own account; CI validates it and replays it in simulation on every pull request from then on.

**Why it works.** Tests then grow from what people actually do, not only from what developers think of ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "Real use becomes automated tests"). Deno's spec tests show the shape: one directory per test with a schema-validated manifest and an expected-output file with wildcard markers ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project)).

**Adopt it.** Version the scenario schema from the first commit and generate a JSON Schema for editors and issue forms. Give scenario-only pull requests their own path class so they run on Linux alone. Require a maintainer's label before a contributed scenario runs on real desktops, because a merged scenario is code that real machines execute. Make a fix pull request leave a regression behind.

**In Eileen.** "Save that as a test" or `eileen scenario save` writes a folder in the user's data directory, `%LOCALAPPDATA%\eileen\scenarios\saved\<id>\`, never in the repository; only an approved, redacted copy reaches the repository's `scenarios/`. `share` replaces every value wrapped as personal or untrusted with placeholders, runs the redactor and a PII scan, replays locally and previews; `submit` opens a pull request through the contributor's own `gh` sign-in, or an issue form whose fields map one to one to the schema ([BUILD §4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet), [§3.3.14](BUILD.md#3314-scenarios); [D35](DECISIONS.md#d35-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet)). Checks 18 and 28 enforce privacy and the scenario-only rules, the `scenario-reviewed` label gates the desktop and fleet lanes, and a `fix` pull request must add a scenario, fuzz regression, simulator regression or test ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). Contributed scenarios are MIT and consented audio is CC-BY-4.0 with a consent record (owner answer 18 in [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20); [BUILD §4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet), "Licenses").

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [BUILD §4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet); [D35](DECISIONS.md#d35-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet), [D45](DECISIONS.md#d45-replay-and-scenarios-two-modes-formats-as-contracts-shipped-crates).

### 5.4 The owner's requests as timed scenarios

**Practice.** Turn the owner's own example requests, in the owner's own words, into timed scenarios with named phases measured from event-log spans, and run them on the owner's machine on a schedule.

**Why it works.** The requests are the product's definition of done, and timing them by phase shows where the time goes (launch, navigation, worker start, the edit) instead of one total that hides regressions.

**Adopt it.** Give each request an id, phases, a pass condition that a machine can check, and a "requires" list for the apps it needs, so it skips cleanly on machines without them.

**In Eileen.** Benchmarks B1 to B10 cover the owner's Resolve request (passing only with a sharpness gain of at least 15 percent and the owner's yes), the Photoshop request (skipped until Photoshop is present), three parallel agents, coding by voice, interruption, the best answer at conversational speed, dictation, learning off against learning on (B8 and B8-curve), paraphrases through the brain, and the brain benchmark run before any backend switch ([BUILD §4.7](BUILD.md#47-test-strategy); [OWNER-DIRECTION, 2026-10-11](OWNER-DIRECTION.md#2026-10-11) for B1 to B4; [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20) for interruption, the best answer at conversational speed, dictation, learning off against on, and the brain benchmark).

**Sources.** [BUILD §4.7](BUILD.md#47-test-strategy); [engineering-practice §3.6](research/2026-10-engineering-practice.md#36-the-owners-benchmark-requests-as-timed-scenarios).

### 5.5 The community test fleet

**Practice.** Let contributors opt in to run the real-desktop scenario suite on their own machines and upload a results file with no personal data. Identify each machine by a stable, salted, one-way hash, never by hardware serials, user names or accounts.

**Why it works.** Different machines, monitors, DPI settings, app versions and languages catch what one PC never would ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20)). A stable hash lets results from one machine be grouped and its flaky scenarios tracked without knowing whose machine it is.

**Adopt it.** Offer a recorded mode that serves recorded model responses while the real desktop acts, so the fleet tests what only a fleet can (DPI, monitors, app versions, locale) without spending anyone's model plan, and a live mode as an opt-in. Run in a throwaway data directory with owned fixtures, stop at physical input, and deny every consequential approval. Mix a random install id into the hash so cloned virtual machines differ, and length-prefix the hash inputs so the encoding is unambiguous.

**In Eileen.** `eileen test --fleet` runs attended, or armed for the night, when it holds a display-required and system-required power request and locks the workstation at the end; either way it auto-denies tier-2 approvals; results arrive on an orphan `devhub` branch as pull requests from contributors' own accounts, as commits by contributors with write access, or relayed by the owner; failures are clustered by signature in CI and triaged by a Claude worker on the owner's PC ([BUILD §4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet)). The machine hash is BLAKE3 over a fixed salt, the normalised OS machine id (Windows `MachineGuid`, Linux `/etc/machine-id`) and a random install id, read through the `MachineIdentity` platform trait ([BUILD §3.4](BUILD.md#34-platform-traits); [D60](DECISIONS.md#d60-the-fleets-machine-hash-mixes-the-os-machine-id-with-a-random-install-id), [D74](DECISIONS.md#d74-the-owners-answers-applied-in-full); owner answer 6 in [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20)).

**Sources.** [BUILD §3.4](BUILD.md#34-platform-traits), [§4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet); [D60](DECISIONS.md#d60-the-fleets-machine-hash-mixes-the-os-machine-id-with-a-random-install-id), [D74](DECISIONS.md#d74-the-owners-answers-applied-in-full).

### 5.6 Fuzzing on Linux, corpus replay everywhere

**Practice.** Every parser of untrusted input gets a fuzz target that calls a function in a platform-neutral crate, with a small seed corpus. Fuzzing runs on Linux; every target's corpus and regressions replay as ordinary tests on every OS. A crash becomes a minimised regression file with a focused test before its fix counts.

**Why it works.** cargo-fuzz runs only on Unix-like systems ([engineering-practice §2.5](research/2026-10-engineering-practice.md#25-testing-tools); [cargo-fuzz (read 2026-10-20)](https://github.com/rust-fuzz/cargo-fuzz)), so keeping parsers out of FFI code lets Windows-only products fuzz them anyway. Building the fuzz targets on every pull request keeps them from rotting, as ruff does, with a daily fuzz job that files an issue ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project); [ruff daily fuzz (read 2026-10-20)](https://github.com/astral-sh/ruff/blob/main/.github/workflows/daily_fuzz.yaml)). A bug that dodged the fuzzer is first a bug in the fuzzer ([Finding Bugs (read 2026-10-20)](https://matklad.github.io/2026/09/19/finding-bugs.html)).

**Adopt it.** List the untrusted inputs (lines from other processes, tool arguments, hook bodies, configuration files, contributed files, network replies) and give each a target and a property: no panic, bounded memory, round-trip. Cap line length before parsing. Keep the corpus that nightly fuzzing grows as an artifact, not in the repository.

**In Eileen.** Twenty-three targets, from `stream_json_line` (lines from the Claude CLI) and `shell_command` (worker commands into the tokenizer) to `scenario_file`, `skill_record` and `sim` (simulator scenarios from bytes, checked by every invariant), each owned by the package that owns the parser; `cargo fuzz build` on every pull request that touches a parser crate, 10 minutes per target nightly, an hour per hot target weekly ([BUILD §4.7](BUILD.md#47-test-strategy), [§4.6](BUILD.md#46-what-runs-where)). Check 17 enforces the corpus, the entry function and the replay test ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)).

**Sources.** [engineering-practice §3.7](research/2026-10-engineering-practice.md#37-fuzz-targets); [BUILD §4.7](BUILD.md#47-test-strategy).

### 5.7 Soak tests

**Practice.** Run the real processes for a long time with recorded input and scripted requests, sample resource counters every second, and judge slopes after a warm-up, not absolute numbers.

**Why it works.** An always-on process fails slowly: a handle leaked per request, a cache that never shrinks, latency that drifts upward. Absolute limits on a loaded machine only produce noise; a flat line after warm-up is a clear pass ([engineering-practice §3.8](research/2026-10-engineering-practice.md#38-soak-tests)).

**Adopt it.** Measure working set and private bytes, handles, GUI objects, threads, idle CPU, live children (exactly the expected set), log and database sizes, and the scripted request's latency. Run 30 minutes nightly and several hours weekly.

**In Eileen.** `cargo xtask soak` with stub workers and WAV input, a slope under 1 MB per hour for the core, 30 minutes nightly and 4 hours weekly on hosted Windows, and an overnight soak on the owner's PC weekly ([BUILD §4.7](BUILD.md#47-test-strategy), [§4.5](BUILD.md#45-idle-budget), [§4.6](BUILD.md#46-what-runs-where)).

**Sources.** [engineering-practice §3.8](research/2026-10-engineering-practice.md#38-soak-tests); [BUILD §4.7](BUILD.md#47-test-strategy).

### 5.8 Mutation testing

**Practice.** Run mutation testing on the lines a pull request changed, as advice at first, and a full sharded run weekly. Make it gating per crate only once that crate's full run has no missed mutants. Use coverage as a map for where to aim fuzzing and simulation, not as a gate.

**Why it works.** Coverage shows what ran; mutation shows what is checked. Restricting it to the diff keeps it affordable on pull requests ([engineering-practice §3.9](research/2026-10-engineering-practice.md#39-mutation-testing-and-coverage); [cargo-mutants book (read 2026-10-20)](https://github.com/sourcefrog/cargo-mutants/tree/main/book/src)).

**Adopt it.** `cargo mutants --in-diff` on the pure logic crates, `continue-on-error` in CI, `--shard k/n` weekly, reports uploaded as artifacts.

**In Eileen.** `mutants-diff` is an advisory job in `ci.yml`; the weekly lane runs four shards; it is never run locally because it is heavy on the owner's PC ([BUILD §4.6](BUILD.md#46-what-runs-where), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). The owner also asked for nightly jobs that target weak tests found by mutation testing, after v0.1 ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "Eileen improves Eileen").

**Sources.** [engineering-practice §3.9](research/2026-10-engineering-practice.md#39-mutation-testing-and-coverage); [BUILD §4.6](BUILD.md#46-what-runs-where).

### 5.9 Benchmarks as A/B tests with a results store

**Practice.** Judge performance by A/B: build the base and the head, alternate runs, compare each phase with a non-parametric test, and report only differences that are both significant and larger than about 10 percent. Append one JSON line per run to a results store outside the main branch, and render a public results table from it.

**Why it works.** Fixed time thresholds on a loaded machine produce noise. Firecracker's performance tests emit data series, assert nothing, and are judged by an A/B tool with a non-parametric test; TigerBeetle appends one JSON object per benchmark run to a separate store that a dashboard reads ([engineering-practice §3.6](research/2026-10-engineering-practice.md#36-the-owners-benchmark-requests-as-timed-scenarios); [Firecracker tests README (read 2026-10-20)](https://github.com/firecracker-microvm/firecracker/blob/main/tests/README.md); [TigerBeetle devhub (read 2026-10-20)](https://github.com/tigerbeetle/tigerbeetle/blob/main/src/scripts/devhub.zig)). Keeping results off the main branch keeps pull request checks hermetic.

**Adopt it.** Record with every run what could explain a difference (commit, binary hash, tool and model versions, available memory, CPU load, whether the user was active). Compare runs only within a stratum of similar conditions, and record every exclusion with its reason.

**In Eileen.** A/B with alternating runs and a Mann-Whitney test; runs are stratified by the memory-admission rule that admitted their jobs ("roomy" or "tight") rather than excluded below a fixed free-memory figure; one JSON line per run goes to the `devhub` orphan branch, where nightly renders `RESULTS.md` and the Pages benchmark page ([BUILD §4.7](BUILD.md#47-test-strategy); [D64](DECISIONS.md#d64-the-slice-in-w1-and-memory-thresholds-from-this-pcs-own-measurements); owner answer 3 in [OWNER-DIRECTION](OWNER-DIRECTION.md#2026-10-20)).

**Sources.** [engineering-practice §3.6](research/2026-10-engineering-practice.md#36-the-owners-benchmark-requests-as-timed-scenarios); [BUILD §4.7](BUILD.md#47-test-strategy); [D64](DECISIONS.md#d64-the-slice-in-w1-and-memory-thresholds-from-this-pcs-own-measurements).

### 5.10 Tests that need a special machine, and no silent skips

**Practice.** Tests that need the owner's machine, real apps or a real vendor sign-in live in named modules that the default test filters exclude and a dedicated profile includes, and the runner prints how many it excluded. Ignored tests are banned. CI never holds vendor credentials; a stub speaks the vendor tool's protocol from recorded scripts, and a contract test on the owner's machine compares the real tool's event shapes with the scripts.

**Why it works.** A skipped test that looks like a pass hides breakage. A stub plus a shape-comparing contract test keeps CI hermetic and still catches vendor changes ([engineering-practice §3.5](research/2026-10-engineering-practice.md#35-windows-end-to-end-harness-eileen-e2e)).

**Adopt it.** Use the test runner's filters, not `#[ignore]`. Record the stub scripts from real sessions and redact them. Compare types and fields, not text. Run desktop tests one at a time in their own group, on windows the test created.

**In Eileen.** Modules `owner_pc::` and `real_claude::`, nextest's `default-filter` and an `owner` profile; check 16 bans `#[ignore]`, `#[should_panic]` and real-time sleeps in tests; `eileen-stub-harness` plays `tests/harness-scripts/*.ndjson`; the leak timeout fails a test that leaves a child process running ([BUILD §4.7](BUILD.md#47-test-strategy), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)).

**Sources.** [BUILD §4.7](BUILD.md#47-test-strategy), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [engineering-practice §3.5](research/2026-10-engineering-practice.md#35-windows-end-to-end-harness-eileen-e2e).

## 6. Code maps and code graphs

How a person or an agent finds the code behind a symptom quickly ([BUILD §2.13](BUILD.md#213-observability); [observability §6](research/2026-10-observability.md#6-code-maps-for-people-and-agents)).

### 6.1 A generated code map, checked fresh by CI

**Practice.** Generate a code map from the build metadata and the source: the crate graph grouped by process, the processes and their transports, module trees, and a table of what each crate owns. Commit it, and fail CI when it is stale. Generate reference pages (events, telemetry names, tools, configuration keys, checks) the same way.

**Why it works.** A generated map cannot drift from the code, and committed Markdown with Mermaid diagrams renders on GitHub and is readable by an agent without running anything. rust-analyzer commits its generated files, updates them from tests and runs its tidy checks as ordinary tests ([observability §6.1](research/2026-10-observability.md#61-what-ci-generates)).

**Adopt it.** Give each crate's manifest a small metadata table (which process it runs in, what it owns, its unsafe policy) and generate from that plus `cargo metadata`. Parse module trees with a parser (`syn`) rather than a build, so platform-gated modules show up on any OS. Keep generated output hermetic: sorted, derived only from the tree, no timestamps or line counts.

**In Eileen.** `cargo xtask codegen` writes `docs/CODEMAP.md`, `docs/reference/*` and `.claude/rules/crate-*.md`; check 9 runs it with `--check`; check 22 requires `[package.metadata.eileen]` with `process`, `owns` and `unsafe` in every crate ([BUILD §2.13](BUILD.md#213-observability), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci), [§6.2](BUILD.md#62-wp0-foundation-workspace-contracts-and-quality-gates)). Module trees from cargo-modules and build timings are nightly artifacts only ([observability §6.1](research/2026-10-observability.md#61-what-ci-generates)).

**Sources.** [observability §6.1](research/2026-10-observability.md#61-what-ci-generates), [§6.2](research/2026-10-observability.md#62-cargo-xtask-codemap); [engineering-practice §5.3](research/2026-10-engineering-practice.md#53-generated-code-map-and-event-map); [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci).

### 6.2 The event map

**Practice.** For an event-driven core, generate an event map: for every input and event variant, the functions that construct it and the match arms that handle it.

**Why it works.** In a core driven by closed enums, this table is the call graph that matters. It answers "who sends this, and who reacts to it" in one place, for people and for agents ([engineering-practice §5.3](research/2026-10-engineering-practice.md#53-generated-code-map-and-event-map)).

**Adopt it.** A `syn` visitor over the workspace that finds constructors and match arms per variant, written into the code map between markers.

**In Eileen.** `docs/CODEMAP.md` includes the event map of every `BusEvent` and `Input` variant ([BUILD §2.13](BUILD.md#213-observability)).

**Sources.** [engineering-practice §5.3](research/2026-10-engineering-practice.md#53-generated-code-map-and-event-map); [BUILD §2.13](BUILD.md#213-observability).

### 6.3 Crate rules

**Practice.** One workspace with a flat `crates/*` layout, every third-party dependency declared once at the workspace level, and a table that says which crates may depend on which. The rules the table encodes are repository checks.

**Why it works.** A flat layout scales better than nested workspaces ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project); [Large Rust Workspaces (read 2026-10-20)](https://matklad.github.io/2021/08/22/large-rust-workspaces.html)). Rules expressed as an absence ("the core does no IO", "only one crate knows the protocol") cannot be learned by reading code, so they must be written and checked ([engineering-practice §2.2](research/2026-10-engineering-practice.md#22-architecture-documents)).

**Adopt it.** Declare the feature sets of the heaviest shared crates once, as the union every package needs, so building one crate and building the workspace compile them once instead of alternating. Keep unsafe code in one private `ffi` module per crate. After a lockfile conflict, take `main`'s lockfile and re-run a command that fails if any third-party version changed.

**In Eileen.** [BUILD §3.1](BUILD.md#31-the-cargo-workspace) lists every crate with its kind, platform, process, unsafe policy, owner package and allowed internal dependencies. Checks enforce the rules: core purity (24), platform and native boundaries and the feature unions (15), unsafe only in `ffi` modules with one operation per block and a safety comment (10), the credential boundary and the spawn exemptions (14), the code map listing exactly the workspace crates (3) ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [D55](DECISIONS.md#d55-the-crate-graph-and-the-lint-bans-agree-secrets-volume-and-app-hooks-are-platform-traits)).

**Sources.** [BUILD §3.1](BUILD.md#31-the-cargo-workspace), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [engineering-practice §2.2](research/2026-10-engineering-practice.md#22-architecture-documents).

### 6.4 Code navigation for agents

**Practice.** Give coding agents read-only tools over the project's own diagnostics (the code map, crate docs, the event map, traces, logs, incidents, replay), redacted the same way as anything shared, and a written path from symptom to code. Add a language server only when the machine can afford it.

**Why it works.** An agent that can fetch the trace of a reported request, find the producer and handler of the first wrong event and replay the incident before and after its fix goes from symptom to code quickly ([observability §7](research/2026-10-observability.md#7-debugging-eileen-guide-outline); [engineering-practice §5.4](research/2026-10-engineering-practice.md#54-tracing-a-bug-from-symptom-to-code)). A language server runs build scripts and procedural macros outside the build guard and can need gigabytes; rust-analyzer's default `checkOnSave` runs `cargo check` after every save ([observability §6.4](research/2026-10-observability.md#64-what-eileens-own-claude-workers-get-on-eileens-codebase)).

**Adopt it.** Expose the tools through MCP from the product itself, read-only. If you add a language server, turn off check-on-save, give it its own target directory and limit how many run at once.

**In Eileen.** The `dev` toolset on the `eileen` MCP server (`dev_codemap`, `dev_crate_doc`, `dev_event_map`, `dev_trace_get`, `dev_logs_query`, `dev_incident_list`, `dev_incident_read`, `dev_replay_core`, `dev_scenario_list`) applies the share-pack redaction to everything it returns, and the repo-local `eileen-dev` plugin carries skills and these tools but no language server in v0.1, because rust-analyzer would need about 1.5 GB the PC does not have ([BUILD §2.13](BUILD.md#213-observability); [D54](DECISIONS.md#d54-memory-admission-and-the-build-guard-read-available-memory-commit-headroom-and-paging)). Serena (GPL-3.0-or-later), GitNexus (PolyForm Noncommercial), Sourcegraph (needs a server) and SCIP (no Windows build of the `scip` CLI) were evaluated and not adopted ([observability §6.3](research/2026-10-observability.md#63-tools-evaluated)).

**Sources.** [observability §6.3](research/2026-10-observability.md#63-tools-evaluated), [§6.4](research/2026-10-observability.md#64-what-eileens-own-claude-workers-get-on-eileens-codebase), [§7](research/2026-10-observability.md#7-debugging-eileen-guide-outline); [BUILD §2.13](BUILD.md#213-observability).

## 7. Guidance for people and AI agents

Where the rules live so that people and coding agents reliably find them, decided in [D33](DECISIONS.md#d33-guidance-form-one-agentsmd-crate-docs-in-the-code-nested-files-only-where-rules-differ) from the survey in [engineering-practice §2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026).

### 7.1 One canonical AGENTS.md, with CLAUDE.md importing it

**Practice.** One root `AGENTS.md` is the guide for every coding tool and for people: the commands, the rules that do not bend, how to make a change, testing and review rules, and pointers to the architecture, the specification and the owner's direction. The root `CLAUDE.md` is one import line, `@AGENTS.md`, plus a short Claude-only section.

**Why it works.** Claude's harness expands `@` imports; Codex has no import syntax and reads only `AGENTS.md`; on Windows, Git checks a committed symlink out as a one-line text file unless symlinks are enabled, so a symlink gives one tool nothing ([engineering-practice §2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026); [Claude memory documentation (read 2026-10-20)](https://code.claude.com/docs/en/memory); [Codex AGENTS.md guide (read 2026-10-20)](https://learn.chatgpt.com/docs/agent-configuration/agents-md)). ruff's `CLAUDE.md` is that one import line ([engineering-practice §2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026); [ruff CLAUDE.md (checked 2026-10-21)](https://github.com/astral-sh/ruff/blob/main/CLAUDE.md)). Shorter files are followed better: Claude's memory documentation says longer files "consume more context and reduce adherence" ([engineering-practice §4.1](research/2026-10-engineering-practice.md#41-the-layout-and-why); [Claude memory documentation (checked 2026-10-21)](https://code.claude.com/docs/en/memory)). The tools also set limits: Claude's documentation recommends under 200 lines per file, and Codex combines files up to 32 KiB by default ([engineering-practice §2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026)).

**Adopt it.** Keep the root guide under 150 lines and terse, in the style of uv's ALWAYS, NEVER and PREFER rules ([uv AGENTS.md (read 2026-10-20)](https://github.com/astral-sh/uv/blob/main/AGENTS.md)). Write for a reader who never saw the session. Instruction files are context, not enforcement, so put hard rules that must not be bypassed in a hook (the documented mechanism: Claude's memory documentation says to use a `PreToolUse` hook to block an action whatever the model decides) and in the harness's permission settings (inferred to be enforced; verify against the settings reference) ([engineering-practice §4.7](research/2026-10-engineering-practice.md#47-claude-only-extras); [Claude memory documentation (checked 2026-10-21)](https://code.claude.com/docs/en/memory)).

**In Eileen.** Root `AGENTS.md` (at most 150 lines) and `CLAUDE.md` (`@AGENTS.md` plus project skills, the settings denials, a fresh-reviewer rule and no language server during build work), with `.claude/settings.json` denying direct cargo commands and reads of vendor credential files ([BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents); [D33](DECISIONS.md#d33-guidance-form-one-agentsmd-crate-docs-in-the-code-nested-files-only-where-rules-differ)). Check 2 enforces the import line and the length ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)).

**Sources.** [engineering-practice §2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026), [§4](research/2026-10-engineering-practice.md#4-claudemd-and-agentsmd); [BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents); [D33](DECISIONS.md#d33-guidance-form-one-agentsmd-crate-docs-in-the-code-nested-files-only-where-rules-differ).

### 7.2 Crate docs in the code

**Practice.** Each crate's guidance is its crate documentation at the top of `src/lib.rs`, with required headings: what it owns, what it does not own, invariants with how each is enforced, boundaries, testing, traps, logs and tracing, and what must not change without a decision record. Generate path-scoped copies for tools that load rules when a file is touched.

**Why it works.** No repository in the survey keeps a guidance file in every crate; per-directory files appear only where a subsystem has its own contract ([engineering-practice §2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026)). Documentation in the code is rendered by rustdoc and read by every agent that opens the crate, and a generated, path-scoped copy reaches an agent that edits a file without opening `lib.rs` first. Structure goes stale fastest, so ownership gets at most three bullets; invariants, traps and test commands change less and are what code cannot show ([engineering-practice §4.1](research/2026-10-engineering-practice.md#41-the-layout-and-why), and [§2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026) for what code cannot show).

**Adopt it.** Fix the headings and a line limit, and check both in CI. State ownership in at most three bullets and spend the rest on invariants and traps.

**In Eileen.** The engineering-practice report first proposed an `AGENTS.md` in every crate; the decision chose crate docs in the code instead, at most 80 lines, extracted by `cargo xtask codegen` into `.claude/rules/crate-<name>.md` with `paths: ["crates/<name>/**"]` and into `docs/reference/crates.md` ([D33](DECISIONS.md#d33-guidance-form-one-agentsmd-crate-docs-in-the-code-nested-files-only-where-rules-differ), [D46](DECISIONS.md#d46-the-build-guard-and-the-gates-revised); [BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents)). Check 1 parses the headings and the limit; check 23 warns when a pull request changes a crate's public API without touching its crate doc ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)).

**Sources.** [D33](DECISIONS.md#d33-guidance-form-one-agentsmd-crate-docs-in-the-code-nested-files-only-where-rules-differ); [BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents); [engineering-practice §2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026).

### 7.3 Nested guidance only where the rules differ

**Practice.** A nested `AGENTS.md` (with its own one-line `CLAUDE.md`) exists only where a folder's rules genuinely differ from the root's, and adding one needs a stated reason.

**Why it works.** codex keeps per-directory files only where one subsystem has rules of its own ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project)); the one in its terminal UI's bottom pane, for example, requires changes to two input state machines to keep their module docs accurate ([codex per-directory AGENTS.md (checked 2026-10-21)](https://github.com/openai/codex/blob/main/codex-rs/tui/src/bottom_pane/AGENTS.md)). Every extra file is one more thing to keep true.

**Adopt it.** Typical candidates: FFI crates (threading and deadlines), crates with native libraries or models, the fuzz workspace (its own toolchain), a public test corpus (privacy rules), and a plugin in another language.

**In Eileen.** Six nested pairs: the two Windows automation crates, the speech crate, `fuzz/`, `scenarios/` and the Claude plugin; each has a "Why this folder has its own rules" heading, which check 2 requires ([BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)).

**Sources.** [BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents); [engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project).

### 7.4 A short ARCHITECTURE.md, and rules hygiene

**Practice.** A short `ARCHITECTURE.md` at the root in rust-analyzer's shape: a bird's eye view, entry points, a code map with one entry per crate naming files and types (not links) and marking invariants and API boundaries, then cross-cutting concerns. Guidance files are not edited during feature work; proposed rule changes go in the pull request description.

**Why it works.** The document is read by every recurring contributor, so it holds only what changes rarely; invariants that are an absence must be called out because code cannot show them ([engineering-practice §2.2](research/2026-10-engineering-practice.md#22-architecture-documents); [matklad, ARCHITECTURE.md (read 2026-10-20)](https://matklad.github.io/2021/02/06/ARCHITECTURE.md.html)). Zed's rules hygiene treats rules as traps to avoid, not maps to follow, and admits a new rule only if it is non-obvious, came up more than once and is specific ([engineering-practice §2.1](research/2026-10-engineering-practice.md#21-project-by-project); [Zed .rules (read 2026-10-20)](https://github.com/zed-industries/zed/blob/main/.rules)).

**Adopt it.** Check that the code map lists exactly the crates that exist and that every path or type it names resolves. Add a "Suggested guidance changes" heading to the pull request template.

**In Eileen.** WP0 moves `docs/ARCHITECTURE.md` to the root in that shape, check 3 matches its code map to the workspace both ways, and the pull request template carries "Suggested guidance changes" ([BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [pull request template](../.github/PULL_REQUEST_TEMPLATE.md)).

**Sources.** [engineering-practice §2.2](research/2026-10-engineering-practice.md#22-architecture-documents), [§5](research/2026-10-engineering-practice.md#5-architecturemd); [BUILD §4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents).

### 7.5 Conventions enforced by CI

**Practice.** Every convention the project adopts has an automated check that fails the pull request, runs the same way locally with one command, and is itself documented, so contributors know the rules before CI tells them. A registry of checks generates the documentation.

**Why it works.** Guidance files rot; checks do not. codex deleted its root guidance file without explanation, a reminder that the tests, lints and repository checks are the real guardrail and the files should mostly point at them ([engineering-practice §2.3](research/2026-10-engineering-practice.md#23-agent-guidance-files-in-2026)). The owner asked for exactly this ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "The conventions are enforced by CI").

**Adopt it.** Keep each check's id, rule, fix, local command and CI job in one registry; render it into `CONTRIBUTING.md` and a reference page; make every failure print the id, file, line, fix and command. Give checks that delegate to another tool a configuration assertion as their test, plus one failing fixture. Store a house-style denylist of names as salted hashes so the names never enter the repository.

**In Eileen.** Thirty checks ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [D32](DECISIONS.md#d32-conventions-are-enforced-by-ci-with-one-local-command), [D46](DECISIONS.md#d46-the-build-guard-and-the-gates-revised)):

| # | Convention |
|---|---|
| 1 | Every crate's guidance is its crate doc, with the required headings, within 80 lines |
| 2 | Nested `AGENTS.md` only where listed, each with a one-line importing `CLAUDE.md` and a reason heading; root guide at most 150 lines |
| 3 | `ARCHITECTURE.md` lists exactly the workspace crates, and every path or type in it resolves |
| 4 | Every `cargo xtask` command named in guidance exists |
| 5 | Markdown lint, internal links (external links nightly), spelling |
| 6 | House style: no em dash in Markdown, doc comments or user-facing strings; the product only ever "Eileen"; no old working names; workers named by route; no vendor product name inside an Eileen name; no film or trademark names (hashed denylist) |
| 7 | The pull request title is a valid squash subject |
| 8 | The pull request description follows the template, Evidence first, with resolvable ids and a type that fits the files |
| 9 | Generated files are current and hermetic |
| 10 | Every unsafe block has a safety comment, one operation per block, only in `ffi` modules |
| 11 | Public items are documented |
| 12 | Decision records follow their grammar |
| 13 | Dependencies pass the licence, advisory, ban and source policy; no unused dependencies; cooling-off on updates |
| 14 | The credential boundary and the process-spawn exemptions |
| 15 | Platform and native boundaries, and no member adds features to the shared feature unions |
| 16 | No ignored tests, no `should_panic`, no real-time sleeps; special-machine tests only in their modules; long modules listed with a reason |
| 17 | Fuzz targets have a capped corpus, an entry function and a replay test; simulator regressions parse |
| 18 | Scenarios are valid and private; no expired stale status or quarantine |
| 19 | A fix adds a regression, shown failing before and passing after |
| 20 | Workflows pass zizmor, actions are pinned by SHA, permissions are read-only by default |
| 21 | Every span and event name is declared and covered by the span-parent table |
| 22 | Each crate's manifest has `process`, `owns` and `unsafe` metadata |
| 23 | Docs drift (advisory): a public API change without a crate-doc change gets a warning |
| 24 | Core purity |
| 25 | Input privacy: the raw-input modules forward no key codes beyond the allowed ones and never a pointer position |
| 26 | Every free-text field reachable from the core's inputs, tool results and desktop state is wrapped as untrusted or personal, or allowlisted with a reason |
| 27 | The settings registry covers every configuration key exactly once |
| 28 | Scenario-only pull requests touch only their scenario folder; contributed scenarios need the review label for desktop lanes |
| 29 | This playbook cites a source in every section, and its links resolve |
| 30 | A change to the decision log changes this playbook too, or says why not |

**Sources.** [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [D32](DECISIONS.md#d32-conventions-are-enforced-by-ci-with-one-local-command), [D46](DECISIONS.md#d46-the-build-guard-and-the-gates-revised); [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20).

## 8. Observability

Following one request from symptom to span, log line, recording and code, as [D34](DECISIONS.md#d34-observability-request-ids-a-flight-recorder-and-replay) records and [observability §1](research/2026-10-observability.md#1-verdict) argues.

### 8.1 Request IDs end to end

**Practice.** Every request gets a typed, prefixed, time-sortable ID the moment it starts, and that ID is on every log line, span, event and stored record the request causes, across every process. A request the system decided to ignore still gets one.

**Why it works.** A prefix makes every ID greppable and self-describing in logs, in the user interface and in an agent's context ([observability §1](research/2026-10-observability.md#1-verdict), [§2.1](research/2026-10-observability.md#21-identifiers)). Giving an ignored request an ID is what makes "it ignored me" traceable.

**Adopt it.** Use ULIDs in newtypes whose `Display` adds the prefix and whose parser checks it. Mint them from the injected clock and RNG in the core, so replay is deterministic. Check in an end-to-end test that every log line written during a request carries its ID or comes from a short allowlist of background targets.

**In Eileen.** `sess_`, `turn_` (the request ID), `job_`, `ws_`, `call_`, `speak_`, `appr_`, `inc_` and `rec_` ([BUILD §2.13](BUILD.md#213-observability), [§3.3.1](BUILD.md#331-identifiers-time-correlation-and-text); [D34](DECISIONS.md#d34-observability-request-ids-a-flight-recorder-and-replay)).

**Sources.** [observability §2.1](research/2026-10-observability.md#21-identifiers); [BUILD §2.13](BUILD.md#213-observability); [D34](DECISIONS.md#d34-observability-request-ids-a-flight-recorder-and-replay).

### 8.2 Trace context across process and MCP boundaries

**Practice.** Carry correlation as data, not only in the tracing library's span context: a small context struct (request, job, call) on every bus envelope and IPC frame header, W3C `traceparent` in every MCP request's `_meta`, and `TRACEPARENT` in the environment of every child process that reads it. Span and event names are constants in one module, user content travels only in fields whose names mark it as personal, and a table of allowed span parents is checked in tests.

**Why it works.** Span context does not cross a broadcast channel, a pipe or a process ([observability §1](research/2026-10-observability.md#1-verdict), item 2). MCP's SEP-414 made `traceparent` in `_meta` the convention, and rmcp 3.6.0 has accessors for it ([observability §2.2](research/2026-10-observability.md#22-tracectx-correlation-that-crosses-channels-and-processes); [MCP SEP-414 (read 2026-10-20)](https://modelcontextprotocol.io/seps/414-request-meta); [rmcp trace context (read 2026-10-20)](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/model/meta.rs)). The Claude CLI in `-p` mode reads `TRACEPARENT` from its environment so its spans become children of the caller's trace ([observability §2.6](research/2026-10-observability.md#26-workers-joining-the-claude-clis-own-trace); [Claude CLI monitoring (read 2026-10-20)](https://code.claude.com/docs/en/monitoring-usage)). The report's design reasoning, which it marks inferred, is that using the request ULID's 128 bits as the trace id lets any process attach to the right trace without asking the core ([observability §1](research/2026-10-observability.md#1-verdict), item 3, and [§2.2](research/2026-10-observability.md#22-tracectx-correlation-that-crosses-channels-and-processes)). W3C Trace Context rules out an all-zero trace id as invalid ([W3C Trace Context (checked 2026-10-21)](https://www.w3.org/TR/trace-context/)). A ULID minted from a real clock is never all zeros, but one minted from a simulated clock at the epoch with a zero or counter random part could be; so start the simulator's clock after the epoch, or have the traceparent builder reject a zero id (a recommendation of this document; BUILD.md does not yet say either). LiveKit marks personal attributes with a `pii` segment and checks span parents against a table, which is where Eileen took both ([observability §2.4](research/2026-10-observability.md#24-span-design-rules), [§2.5](research/2026-10-observability.md#25-the-span-schema-and-its-checker)); the [LiveKit trace schema test (read 2026-10-20)](https://github.com/livekit/agents/blob/main/tests/trace_schema.py) holds the parent table.

**Adopt it.** Ban bare task spawns in favour of a wrapper that names the task, keeps the current span and registers it for task dumps; otherwise work started inside a request logs lines without its ID. Keep the audio or other real-time callback free of logging; bump a counter instead.

**In Eileen.** `TraceCtx { turn, job, call }` on every envelope and frame, `_meta.traceparent` plus an `eileen/` key on MCP requests, `TRACEPARENT` and `EILEEN_SESSION_ID` in worker environments, names in `eileen_trace::names` (check 21), `pii.` fields, and `eileen_rt::task::spawn` instead of `tokio::spawn` ([BUILD §2.13](BUILD.md#213-observability), [§4b.2](BUILD.md#4b2-lints-clippy-configuration-and-unsafe-policy)).

**Sources.** [observability §2](research/2026-10-observability.md#2-one-request-id-end-to-end); [BUILD §2.13](BUILD.md#213-observability).

### 8.3 The flight recorder

**Practice.** Keep the last few minutes of diagnostic events and of the core's inputs in memory, bounded by age and size, and write them out only when something goes wrong. Because the input lane is in every dump, every incident is replayable.

**Why it works.** Go 1.25's flight recorder buffers the last seconds of the trace in memory and lets a program snapshot it when it detects a problem ([observability §4.1](research/2026-10-observability.md#41-the-rings); [Go flight recorder (read 2026-10-20)](https://go.dev/blog/flight-recorder)). Nothing personal reaches disk unless there is a reason.

**Adopt it.** A diagnostic lane in every process, an input lane with a checkpoint every minute in the core, and audio as a separate opt-in lane ([observability §4.1](research/2026-10-observability.md#41-the-rings)), and the running job's input lane pinned for its whole duration so a long first run can still be saved as a test.

**In Eileen.** 10 minutes or 32 MB in the core, 8 MB per sidecar, a checkpoint every minute, a running job's lane pinned up to 30 minutes ([BUILD §2.13](BUILD.md#213-observability), [§2.10](BUILD.md#210-the-learning-loop) item 1).

**Sources.** [observability §4.1](research/2026-10-observability.md#41-the-rings); [BUILD §2.13](BUILD.md#213-observability).

### 8.4 Incidents

**Practice.** Write an incident bundle on a panic, a crash, a watchdog trip, a supervisor kill, a spoken or keyboard report, or a tray click: a readable manifest, the merged event lanes, the replayable input lane, a timeline for a trace viewer, the async task tree, the last known state, redacted configuration, log tails and minidumps. A small guard process outside the main process's job writes the dumps from outside and restarts a hung main process under a rate limit.

**Why it works.** Dumping a crashed process from outside is the reliable way, and a hang dump taken before the kill shows the stuck thread ([observability §1](research/2026-10-observability.md#1-verdict), item 8, and [§4.2](research/2026-10-observability.md#42-triggers)). Rate-limiting each trigger kind keeps a crash loop from filling the disk.

**Adopt it.** Embark's `crash-handler` and `minidumper` crates implement the guard protocol ([Embark crash-handling (read 2026-10-20)](https://github.com/EmbarkStudios/crash-handling)). Export the timeline in Chrome's JSON trace format so Perfetto opens it ([observability §3.3](research/2026-10-observability.md#33-timelines-in-perfetto); [Perfetto, other formats (read 2026-10-20)](https://perfetto.dev/docs/getting-started/other-formats)).

**In Eileen.** `%LOCALAPPDATA%\eileen\incidents\<time>_inc_<id>\` with `manifest.toml`; `eileen-guard.exe` restarts a hung core at most once per 10 minutes ([BUILD §2.13](BUILD.md#213-observability)).

**Sources.** [observability §4](research/2026-10-observability.md#4-flight-recorder-crash-and-hang-handling); [BUILD §2.13](BUILD.md#213-observability).

### 8.5 Replay

See [§5.2](#52-replay-modes) for the practice. In Eileen, `eileen replay inc_... --level core` is the step that separates logic bugs from model, desktop and timing problems, after the incident is captured and its trace read; the debugging guide maps each symptom ("it did not hear me", "it clicked the wrong button", "a job hung") to the spans, the recording and the code to look at ([observability §7](research/2026-10-observability.md#7-debugging-eileen-guide-outline); [BUILD §2.13](BUILD.md#213-observability)).

### 8.6 Redaction

**Practice.** Enforce "nothing personal or secret leaves a sink it should not" in layers: secret types that print redacted, personal content only in marked fields, a per-sink policy, one scanner for every line written to a file, and a canary test that injects fake secrets and searches every sink's bytes.

**Why it works.** Each layer catches what the others miss, and the canary test proves the whole policy rather than one function ([observability §3.9](research/2026-10-observability.md#39-redaction-in-every-sink)). Rust's string-slicing panics include the user's string, so the panic hook replaces the quoted part, a rule taken from Zed ([observability §3.9](research/2026-10-observability.md#39-redaction-in-every-sink); [Zed crash handling, GPL, design reference only (read 2026-10-20)](https://github.com/zed-industries/zed/blob/main/crates/crashes/src/crashes.rs)).

**Adopt it.** A literal prefilter (`sk-`, `eyJ`, `-----BEGIN`, `ghp_`, `xox`) keeps the scanner cheap per line. Never write speech or text the user did not address to the system; write a placeholder that no rule matches, so replay still reaches the same decisions.

**In Eileen.** The `Redactor` lives in `eileen-trace`; speech not addressed to Eileen is stored as "[not addressed, n words]"; the redaction canary covers fake secrets, a spoken card number and a password-manager clipboard; a forget is checked by grepping the whole data directory for a canary string, with compressed blobs decompressed and databases dumped ([BUILD §2.12](BUILD.md#212-permission-tiers-and-security), "Privacy", [§2.13](BUILD.md#213-observability), [§2.10](BUILD.md#210-the-learning-loop) item 12).

**Sources.** [observability §3.9](research/2026-10-observability.md#39-redaction-in-every-sink), [§5.5](research/2026-10-observability.md#55-privacy-rules-for-recording); [BUILD §2.12](BUILD.md#212-permission-tiers-and-security), [§2.13](BUILD.md#213-observability).

### 8.7 Errors that explain themselves

**Practice.** Every error a user can meet has a stable kind, one plain sentence built from a template in the project's own code, and, when one exists, a fix the user can trigger. Unexpected errors offer a short explanation produced from the redacted incident and the code map.

**Why it works.** The sentence is deterministic and testable; the model is used only where rules are brittle, which is the owner's principle for AI in the engineering ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "Use AI wherever it makes the engineering better").

**Adopt it.** Each package writes the templates for its own errors; one package owns the kinds and the policy.

**In Eileen.** `UserError` with a `FixAction` (open a settings page, run a doctor check, change a setting, retry); "fix it" runs the fix through the normal tools and tiers; "explain that" captures an incident and starts a short research job with the `dev` toolset; WP1 owns the error kinds and the policy ([BUILD §2.13](BUILD.md#213-observability)).

**Sources.** [BUILD §2.13](BUILD.md#213-observability); [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20).

## 9. Improvement loops

How the product gets better from its own use and how the repository improves itself, per the owner's entries "Eileen learns from every request" and "Stay at the frontier" ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20)).

### 9.1 The learning loop: capture, distil, promote, replay with checks, repair

**Practice.** Record a redacted trajectory of every job at the one place every tool call passes through. Let a capped, low-priority background worker distil successful trajectories into reusable skills, writing only through a validating tool. Promote skills on evidence, from draft to trusted to a compiled recipe that runs with no model and a check after every step, and demote them on failure. Replay through the same permission gate as a worker, hand off to a worker the moment a check fails, and turn the recovery into a repair with version history and rollback. Give the user control: what have you learned, correct it, forget it.

**Why it works.** Learning skill libraries from an agent's own work is well studied: Voyager, Agent Workflow Memory, SkillWeaver and ASI report gains such as +24.6 and +51.1 percent relative success on Mind2Web and WebArena for Agent Workflow Memory, and +31.8 percent relative on WebArena for SkillWeaver ([learning-loop §1](research/2026-10-learning-loop.md#1-verdict), [§2.1](research/2026-10-learning-loop.md#21-research-on-skill-libraries-and-workflow-memory); [Agent Workflow Memory (read 2026-10-20)](https://arxiv.org/abs/2409.07429); [SkillWeaver (read 2026-10-20; "relative" checked in the abstract 2026-10-21)](https://arxiv.org/abs/2504.07079); [Voyager (read 2026-10-20)](https://arxiv.org/abs/2305.16291); [ASI (read 2026-10-20)](https://arxiv.org/abs/2504.06821)). Programs beat text for reuse, and verification at induction time matters ([learning-loop §2.1](research/2026-10-learning-loop.md#21-research-on-skill-libraries-and-workflow-memory)). Treating skills as trusted code is dangerous: in 2026, over 230 malicious skills were reported on ClawHub and GitHub in under a week (secondary, ThaiCERT), and 26.1 percent of 31,132 marketplace skills had at least one vulnerability ([learning-loop §1](research/2026-10-learning-loop.md#1-verdict), item 5, and [§2.2](research/2026-10-learning-loop.md#22-shipped-products-and-open-source-agents)).

**Adopt it.** Store skills in the open format the vendor's own harness loads, so workers use them with no custom prompt plumbing. Compute provenance for every literal in a skill rather than accepting it from the model that wrote it, and hold anything that traces to text read from outside for the user's approval. Make thresholds data and weight them by risk. Never let a learned skill skip a confirmation or raise a permission. Turn trusted skills and failed jobs into regression tests automatically.

**In Eileen.** The loop's decisions live in a pure crate the core calls through a port, so learning runs in the simulator; the native route was verified on the owner's PC (the signed-in CLI found a learned skill through `--plugin-dir` and used it); the local matcher took 4 to 6 microseconds per utterance at the median and under 1 ms at p99 with 5,000 skills ([learning-loop §1](research/2026-10-learning-loop.md#1-verdict); [BUILD §2.10](BUILD.md#210-the-learning-loop); [D27](DECISIONS.md#d27-the-learning-loop), [D44](DECISIONS.md#d44-the-learning-loop-revised)). The ladder: level 0 trajectory, 1 draft, 2 trusted, 3 compiled recipe after agreeing shadow runs and one explicit approval, 4 spoken fast path; a light skill needs 3 clean replays for level 4 and a standard one 10 with at least 95 percent clean over the last 20 ([BUILD §2.10](BUILD.md#210-the-learning-loop), item 5). After the first record, D27, the loop's design was revised five times: D44 after the critics, then D52, D61, D65 and D70 after each verification round ([§1.4](#14-a-verification-loop-per-blocking-issue)). The owner calls learning from every request a main point of the product ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20)).

**Sources.** [learning-loop §1](research/2026-10-learning-loop.md#1-verdict), [§2](research/2026-10-learning-loop.md#2-prior-art-checked-2026-10-20), [§3](research/2026-10-learning-loop.md#3-design); [BUILD §2.10](BUILD.md#210-the-learning-loop); [D27](DECISIONS.md#d27-the-learning-loop), [D44](DECISIONS.md#d44-the-learning-loop-revised), [D52](DECISIONS.md#d52-the-learning-loop-second-revision), [D61](DECISIONS.md#d61-the-learning-loop-third-revision), [D65](DECISIONS.md#d65-the-learning-loop-fourth-revision), [D70](DECISIONS.md#d70-the-learning-loop-fifth-revision).

### 9.2 Evidence-gated upgrades

**Practice.** A candidate component (a model, an engine, a backend) runs the same benchmark suite as what it would replace, is adopted only if it wins, and its results go into the public results table. The table, not opinion, updates the routing.

**Why it works.** "Newer" is not "better" for a given product: the multilingual recogniser, which also punctuates, transcribed "Eileen" as "Clean" on the owner's PC ([voice-stack §4](research/2026-10-voice-stack.md#4-voice-activity-and-speech-recognition)). Published numbers do not settle it either: a secondary benchmark put the newer fast model at 9.5 s to the first answer token at low effort, because it thinks by default ([subscription-harnesses §1](research/2026-10-subscription-harnesses.md#1-verdict), item 5, secondary), while the measurement on the owner's PC got 1.0 to 1.5 s to first text in its better run ([brain-latency](research/2026-10-brain-latency.md#results)); WP3 measures thinking off before the brain depends on it ([BUILD §2.3](BUILD.md#23-the-brain), "Thinking off, measured").

**Adopt it.** Make the benchmark runnable by one command for any candidate, and run it before a switch, warning when the candidate is slower or less reliable by a stated margin.

**In Eileen.** Before a brain backend switch, the settings flow runs benchmark B10 for the candidate and the current backend and warns when the candidate is more than 25 percent slower to the first word or more than 5 points worse on the right action ([BUILD §2.3](BUILD.md#23-the-brain)); the candidate embedding model and Kokoro int8 are adopted only on measurement, and Chatterbox speaks live only when its measured real-time factor allows ([BUILD §7](BUILD.md#7-owner-decisions-settled-questions-and-risks), risks; [D39](DECISIONS.md#d39-voices-the-engine-is-chosen-per-voice-and-chatterbox-is-in-v01)). The owner's rule is "Benchmarks decide" ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20)).

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [BUILD §2.3](BUILD.md#23-the-brain), [§4.7](BUILD.md#47-test-strategy), [§7](BUILD.md#7-owner-decisions-settled-questions-and-risks); [subscription-harnesses §1](research/2026-10-subscription-harnesses.md#1-verdict); [brain-latency](research/2026-10-brain-latency.md#results); [D39](DECISIONS.md#d39-voices-the-engine-is-chosen-per-voice-and-chatterbox-is-in-v01).

### 9.3 The frontier scout, after v0.1

**Practice.** Once the product ships, a weekly routine checks new models, vendor tool changelogs, new open-source engines, new MCP servers for the user's apps, crate updates and research; writes a short digest; opens an issue per worthwhile upgrade; and lists a few bold ideas, ranked by fit, that become small measured experiments in branches.

**Why it works.** Staying current becomes a loop instead of occasional cleanup, and the benchmark gate of [§9.2](#92-evidence-gated-upgrades) decides what graduates.

**Adopt it.** Build only what keeps the scout possible before your first release (plug-in interfaces, aliases, the benchmark suite and its results table, a nightly run against the latest vendor tools), and build the scout itself after.

**In Eileen.** Scheduled after v0.1 by the owner; v0.1 keeps it possible with the things it needs anyway ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "Stay at the frontier"; [ROADMAP](ROADMAP.md#after-v01-ships-staying-at-the-frontier)).

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [ROADMAP](ROADMAP.md#after-v01-ships-staying-at-the-frontier).

### 9.4 The system improving itself through reviewed pull requests

**Practice.** Failing scenarios, from the owner or the test fleet, are clustered deterministically in CI and triaged by a model worker that reads the redacted cluster, the replay and the code map. The worker drafts a fix pull request that must pass every gate and merges only with the owner's approval. A check flags code that changed without its guidance and drafts the update.

**Why it works.** Models fill the places where rules are brittle, while deterministic checks stay the gate ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "Use AI wherever it makes the engineering better" and "Eileen improves Eileen"). A fixed bug always leaves its scenario behind, so the suite only grows.

**Adopt it.** Keep the clustering deterministic and in CI, the triage on a machine with the owner's sign-in, and the merge decision with a person.

**In Eileen.** `cargo xtask triage <issue>` on the owner's PC; check 23's `--draft` mode asks a worker to draft a crate-doc update; flaky scenarios are tracked in a `flake` issue, and a quarantine needs an issue link and an expiry date ([BUILD §4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)). Optional model code review on pull requests is connected by the owner on his own account ([OWNER-DIRECTION, owner answers](OWNER-DIRECTION.md#2026-10-20), answer 17).

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [BUILD §4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci).

## 10. Updatability and swappability

How parts stay current as AI progresses; v0.1 builds the pieces this needs and leaves the scout for later ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [ROADMAP](ROADMAP.md#after-v01-ships-staying-at-the-frontier)).

### 10.1 Aliases and plug-ins

Covered in [§2.6](#26-model-aliases-in-configuration) and [§2.2](#22-plug-in-traits-for-everything-ai-progress-improves). The practical test of both: a better model is a configuration line, and a better backend is a new implementation behind an existing trait plus a configuration line ([BUILD §2.3](BUILD.md#23-the-brain); [D53](DECISIONS.md#d53-model-names-are-aliases-in-config-the-brain-and-the-speech-stages-are-plug-ins)).

### 10.2 Nightly runs against the latest vendor tools

**Practice.** Every night, install both the pinned and the latest version of each vendor tool the product drives, check every flag the product passes against the tool's help output, check that the configuration files the product generates are still accepted, and, on a machine with a real sign-in, run the behavioural contract tests against both versions.

**Why it works.** A breaking change shows up as a failing nightly run before users meet it ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "Early warning on vendor changes"). The vendor's streaming control protocol Eileen relies on is not a documented public API, so the contract test is the only defence against drift ([engineering-practice §9](research/2026-10-engineering-practice.md#9-open-risks); [subscription-harnesses §5](research/2026-10-subscription-harnesses.md#5-driving-the-harnesses-from-rust)).

**Adopt it.** Split the job: a credential-free part in CI (flags, configuration acceptance, plugin validation) and a signed-in part on the owner's machine. Open an issue on a mismatch.

**In Eileen.** The credential-free CLI drift job in the nightly lane, and the stream-json and approval contract tests in the owner's nightly lane against the installed and the latest CLI, installed into a separate prefix ([BUILD §4.6](BUILD.md#46-what-runs-where)).

**Sources.** [BUILD §4.6](BUILD.md#46-what-runs-where); [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20).

### 10.3 Dependency update pull requests

**Practice.** A bot opens grouped update pull requests on a schedule, after a cooling-off period, and they run every lane like any pull request. Crates that change weekly are updated by hand with smoke tests. No workflow opens pull requests itself.

**Why it works.** Updates arrive continuously and are proven by the full suite. Protocol crates moved fast enough during the research (six releases of rmcp, the Rust MCP SDK, in six weeks) that automatic updates would mean constant churn ([rust-architecture §4](research/2026-10-rust-architecture.md#4-crate-versions-verified-2026-10-20)).

**Adopt it.** Recognise the bot's pull requests by their author, skip the human-template checks for them, and check instead that they touch only manifests, the lockfile and action pins.

**In Eileen.** Dependabot for `cargo` and `github-actions`, weekly, grouped, 14-day cooldown, protocol crates ignored; `pr-meta` treats `dependabot[bot]` pull requests as type `deps`; `cargo xtask deps-age` enforces the cooling-off period (check 13) ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [D63](DECISIONS.md#d63-evidence-has-a-grammar-and-its-own-required-check-dependabot-updates-cargo-scenario-keys-are-stable), [D68](DECISIONS.md#d68-evidence-scenario-replay-and-the-slice-revised)).

**Sources.** [BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci); [D63](DECISIONS.md#d63-evidence-has-a-grammar-and-its-own-required-check-dependabot-updates-cargo-scenario-keys-are-stable).

### 10.4 Route discovery: an app's own API or MCP server first

**Practice.** For every app, use the most direct, reliable route before touching the screen: the app's own API, MCP server or scripting, then the platform's app actions and registered connectors, then accessibility automation, then page structure, and screenshots with clicks last. Record which route each action used, remember the best route per app and task, and look up an official MCP server for apps you do not know, installing it only with the user's approval.

**Why it works.** Deterministic hooks are fast, testable and usually reversible, while multi-step vision is slow and unreliable ([D8](DECISIONS.md#d8-deterministic-first-vision-last)). Measured on Eileen's PC, warm handles in a resident process answered in microseconds while anything that started a process per request became slow and unpredictable under load ([app-hooks §1](research/2026-10-app-hooks.md#1-summary), finding 4). An app's own MCP server can arrive with an update, as Resolve's did in 21.1 ([app-hooks §1](research/2026-10-app-hooks.md#1-summary), finding 2; [Resolve 21.1 readme (read 2026-10-20)](https://www.blackmagicdesign.com/support/content/readme/baf7c071c0524fbf8ccc961925c9f443)).

**Adopt it.** Have your own code, not the model, query the registry, and build every proposal from the registry's fields alone. Show who published a server by the namespace the registry verifies, and never call anything "official" unless that verified publisher is the app's maker. Install at an exact version once, never through a package runner at session start, and treat a version change as a new proposal.

**In Eileen.** `routes_lookup` and `routes_propose`, a vendor table that maps apps to their makers' namespaces, a tier-2 Safety setting for each approved server, `route_stats` per app, version and task, and a linter that refuses to compile a vision or automation step for an app with a higher declared route ([BUILD §2.5](BUILD.md#25-workers-routing-and-jobs); [D59](DECISIONS.md#d59-app-routes-discovery-by-eileens-own-tool-remembered-per-app-and-shown-without-hijacking), [D66](DECISIONS.md#d66-containment-and-route-proposals-third-revision); [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "The app's own AI route first").

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [BUILD §2.5](BUILD.md#25-workers-routing-and-jobs); [D8](DECISIONS.md#d8-deterministic-first-vision-last), [D59](DECISIONS.md#d59-app-routes-discovery-by-eileens-own-tool-remembered-per-app-and-shown-without-hijacking), [D66](DECISIONS.md#d66-containment-and-route-proposals-third-revision); [app-hooks §1](research/2026-10-app-hooks.md#1-summary).

### 10.5 Readable, generated configuration

**Practice.** User configuration is a commented file generated from the configuration types' documentation, with a JSON Schema for editor completion, a command that explains where each effective value came from, and every change versioned so it can be undone.

**Why it works.** The same types produce the defaults, the schema and the documentation, so they cannot disagree; a registry test makes every key documented exactly once ([BUILD §2.11](BUILD.md#211-settings-by-voice-and-configuration); [D29](DECISIONS.md#d29-settings-by-voice-in-versioned-toml)).

**Adopt it.** Generate the default file and the schema with the codegen command and check them fresh in CI. Mark safety-relevant keys so only the user's own action can change them.

**In Eileen.** `eileen.toml`, `profiles\<name>.toml` and `eileen.schema.json`, `eileen config explain`, `settings_versions`, and a Safety list of key prefixes that a registry test enforces ([BUILD §2.11](BUILD.md#211-settings-by-voice-and-configuration); [D29](DECISIONS.md#d29-settings-by-voice-in-versioned-toml)).

**Sources.** [BUILD §2.11](BUILD.md#211-settings-by-voice-and-configuration); [D29](DECISIONS.md#d29-settings-by-voice-in-versioned-toml).

## 11. Safety and credentials

The rules that no setting, skill or model output can override, from [BUILD §2.12](BUILD.md#212-permission-tiers-and-security) and the decisions it cites.

### 11.1 Never handle vendor credentials

**Practice.** Spawn the vendor's own, unmodified tool, which the user signs in to themselves. Never run a login flow, never read, copy, log or refresh vendor credential files, and check sign-in only through the tool's own status command. Strip credential variables from every child's environment, and make CI fail if any source names a credential path.

**Why it works.** It keeps the product inside the vendors' terms: Anthropic's terms permit an end user to sign in to the unmodified binary with their own subscription when another product runs it, while forbidding products from routing usage through a plan on behalf of users ([claude-mods §5](research/2026-10-claude-mods.md#5-policy-and-naming); [subscription-harnesses §3](research/2026-10-subscription-harnesses.md#3-policy-vendor-by-vendor); [legal and compliance (read 2026-10-20)](https://code.claude.com/docs/en/legal-and-compliance)). Vendor policy changed often in 2026, which is why a kill switch for subscription-backed workers stays in the configuration ([D4](DECISIONS.md#d4-never-handle-vendor-credentials), [D18](DECISIONS.md#d18-subscriptions-only-claude-is-the-v01-route)).

**Adopt it.** Resolve the vendor tool to its real executable, never a copy bundled inside another app. Build a constructed environment allowlist for children that need no shell, and a denylist (vendor prefixes, `*_API_KEY`, `*_TOKEN`, `*_SECRET`, cloud keys) for children that run the user's builds. Keep the one permitted secret, if any, in the platform's credential store behind a trait.

**In Eileen.** The credential boundary of [BUILD §2.12](BUILD.md#212-permission-tiers-and-security) (doctor checks `claude auth status` by exit code; the npm shim is resolved to the real `.exe`), the two environment policies, `SecretStore` with Windows Credential Manager and `secrecy::SecretString`, and check 14 ([BUILD §4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci)); the owner's rule "Eileen never runs a login flow and never reads vendor credential files" ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "Signing in"). The same rule covers GitHub: scenario submission uses the contributor's own `gh` sign-in ([BUILD §4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet)).

**Sources.** [D4](DECISIONS.md#d4-never-handle-vendor-credentials), [D18](DECISIONS.md#d18-subscriptions-only-claude-is-the-v01-route); [BUILD §2.12](BUILD.md#212-permission-tiers-and-security); [claude-mods §5](research/2026-10-claude-mods.md#5-policy-and-naming).

### 11.2 Permission tiers

**Practice.** Every tool declares a tier: observe (silent), reversible (at once, journaled with its inverse), consequential (confirm first) and blocked (never; hand the step to the user). An undeclared tool is consequential. Approvals come only from the user's own input, bound to the exact call they described, and a tier-2 grant covers that exact call, never a pattern (tier-1 grants may name one; [BUILD §2.5](BUILD.md#25-workers-routing-and-jobs), "What a grant covers").

**Why it works.** Reversible things can happen without a prompt because they can be undone; sending, deleting and spending cannot ([D14](DECISIONS.md#d14-permission-tiers)). Building the approval type only from user input means no screen text, model output or learned skill can approve anything, whatever it says ([D43](DECISIONS.md#d43-approvals-come-only-from-the-user-and-the-worker-route-is-proven-against-the-real-cli), [D51](DECISIONS.md#d51-approvals-and-the-conversation-window-revised)).

**Adopt it.** Make the hook that guards a worker's tool calls fail closed, and never let it answer "allow"; let it answer nothing for the safe tiers, "ask" or "deny" otherwise. Prove the approval path against the real vendor tool before you depend on it, and re-run that test whenever the tool updates.

**In Eileen.** The tier table and the path classes of [BUILD §2.12](BUILD.md#212-permission-tiers-and-security); approvals from a physical click, the approve hotkey seen arriving from a keyboard, or speech with the action word inside a voice window that opens only after the question has finished playing ([BUILD §2.5](BUILD.md#25-workers-routing-and-jobs), "Approvals"); a `PreToolUse` HTTP hook with `onFailure: "block"`, verified to fire and block on the owner's PC ([claude-mods, Verdict](research/2026-10-claude-mods.md#verdict); [hooks reference (read 2026-10-20)](https://code.claude.com/docs/en/hooks)); simulator invariant 7 ([BUILD §4.7](BUILD.md#47-test-strategy)).

**Sources.** [D14](DECISIONS.md#d14-permission-tiers), [D43](DECISIONS.md#d43-approvals-come-only-from-the-user-and-the-worker-route-is-proven-against-the-real-cli), [D51](DECISIONS.md#d51-approvals-and-the-conversation-window-revised); [BUILD §2.5](BUILD.md#25-workers-routing-and-jobs), [§2.12](BUILD.md#212-permission-tiers-and-security).

### 11.3 Untrusted screen content

**Practice.** Everything read from outside (window titles, accessibility text, page text, file contents, other people's speech, tool results, model output) is data. Wrap it in a type that marks it untrusted, fence it in model prompts with an instruction never to follow it, and track per job whether it has read anything from outside and anything private; once both are true, every call that can send data out asks first. Requests that began from system events rather than the user get read-only tools.

**Why it works.** The safety review showed a worker with web tools and file reads could exfiltrate private data with nothing in the way ([D40](DECISIONS.md#d40-what-contains-a-worker-in-v01-and-what-waits-for-v02)). Types make the rule checkable: a test fails if any free-text field reachable from the core's inputs is left unwrapped and unlisted (check 26). Safety rules belong in the server that acts, not in the prompt: Eileen's proof desktop server refused password fields and blocked key combos, while the third-party server it replaced posted `Win+R` without objection ([computer-use §1](research/2026-10-computer-use.md#1-verdict), item 8).

**Adopt it.** One function decides which reads are observed and of which kind, table-tested; the same set drives job taint and what may enter a learned skill. One function vouches for URLs, used everywhere, failing closed when the context is incomplete. Say plainly what the design does not yet prevent.

**In Eileen.** `Untrusted<T>` and `Personal<T>` ([BUILD §3.3.1](BUILD.md#331-identifiers-time-correlation-and-text)); turn authority ([BUILD §2.3](BUILD.md#23-the-brain)); taint, `private_read`, the observed-read table and the URL rule ([BUILD §2.12](BUILD.md#212-permission-tiers-and-security); [D62](DECISIONS.md#d62-containment-calls-and-the-brains-slots-revised), [D66](DECISIONS.md#d66-containment-and-route-proposals-third-revision), [D71](DECISIONS.md#d71-git-internals-environment-assignments-and-the-url-rules-inputs)); provenance and held drafts for learned skills ([BUILD §2.10](BUILD.md#210-the-learning-loop), item 9). The known v0.1 limits, such as a repository's own build scripts running with the user's credentials and same-user tampering with a skill snapshot being detected rather than prevented, are listed in [BUILD §7](BUILD.md#7-owner-decisions-settled-questions-and-risks), with a real sandbox scheduled for v0.2 ([D40](DECISIONS.md#d40-what-contains-a-worker-in-v01-and-what-waits-for-v02)).

**Sources.** [BUILD §2.3](BUILD.md#23-the-brain), [§2.12](BUILD.md#212-permission-tiers-and-security), [§7](BUILD.md#7-owner-decisions-settled-questions-and-risks); [D40](DECISIONS.md#d40-what-contains-a-worker-in-v01-and-what-waits-for-v02); [computer-use §1](research/2026-10-computer-use.md#1-verdict), [§7](research/2026-10-computer-use.md#7-safety-rules-for-the-control-layer).

### 11.4 Consent for voices and recordings

**Practice.** Clone a voice only with its owner's recorded consent, checked to be the same speaker and stored with the voice on the user's machine. Word every consent prompt truthfully. Record to disk only on an explicit opt-in, keep audio a separate opt-in, and delete at once anything the user declines. Keep the owner's voice and screen out of the public repository by a check, and accept contributed audio only with a consent record and a stated licence.

**Why it works.** It protects the people whose voices are cloned, which is why the owner kept the consent enrollment even while turning watermarks off by default ([OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20), "Voice cloning engine and watermarks"). The cloning weights' own terms prohibit cloning without explicit and lawful consent, and the ungated export does not show them, so the product shows them itself ([dictation-and-voices §10](research/2026-10-dictation-and-voices.md#10-voices-the-field-on-2026-10-20)).

**Adopt it.** Make the consent sentence part of the enrollment flow, not a checkbox elsewhere. Make "delete" remove the reference clip, the consent clip, caches and profile. Offer watermarking for exported audio as an opt-in.

**In Eileen.** "Learn a new voice" records the voice owner reading a consent sentence and a passage, compares the two with a speaker-embedding check, and stores the profile on the PC only ([BUILD §1](BUILD.md#e13-any-voice-including-a-friends), E13; [D25](DECISIONS.md#d25-voices-kokoro-default-pocket-tts-for-cloning-no-watermark-by-default)); the first-run surprise path asks with a truthful tick box and deletes the clip and the clone at once if they are not kept ([BUILD §1](BUILD.md#e18-first-run-with-a-surprise-path), E18; [D49](DECISIONS.md#d49-the-first-run-has-a-surprise-path), [D56](DECISIONS.md#d56-the-first-run-revised)); recordings are opt-in with audio separate ([observability §5.5](research/2026-10-observability.md#55-privacy-rules-for-recording)); contributed audio is CC-BY-4.0 with the speaker's consent record ([OWNER-DIRECTION, owner answers](OWNER-DIRECTION.md#2026-10-20), answer 18; [BUILD §4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet)).

**Sources.** [OWNER-DIRECTION, 2026-10-20](OWNER-DIRECTION.md#2026-10-20); [D25](DECISIONS.md#d25-voices-kokoro-default-pocket-tts-for-cloning-no-watermark-by-default), [D39](DECISIONS.md#d39-voices-the-engine-is-chosen-per-voice-and-chatterbox-is-in-v01), [D56](DECISIONS.md#d56-the-first-run-revised); [dictation-and-voices §11](research/2026-10-dictation-and-voices.md#11-recommended-voice-architecture); [observability §5.5](research/2026-10-observability.md#55-privacy-rules-for-recording).

### 11.5 A voice check gates nothing until it clears a measured bar

**Practice.** If the product could recognise who is speaking, fix in advance the error rates it must show before it may accept or refuse anything on that basis, measure them on the real user's consented speech, at the length of real commands and in real rooms, with confidence bounds rather than point estimates, and keep the check out of every decision until it passes. Make sure nothing depends on it: the protections that keep other voices from doing harm must hold without it.

**Why it works.** A check that refuses its own user breaks the first promise of a voice product, that it does what it is told; one that lets others through gives a false sense of safety. The owner set the bar in exactly those terms: well under one percent of his own commands refused and as few false accepts, or not used at all, with the command respected and the existing protections kept ([OWNER-DIRECTION, 2026-10-21](OWNER-DIRECTION.md#2026-10-21)). None of Eileen's research measured speaker verification on command-length speech, so a bar is the only honest basis for using one ([BUILD §7](BUILD.md#7-owner-decisions-settled-questions-and-risks), "Carried into packages"). An upper confidence bound puts the sample size into the rule: with no misses at all, 300 trials are about the fewest that bring the one-sided 95 percent bound under 1 percent, and a single miss needs about 500 ([BUILD §2.8](BUILD.md#28-voice)).

**Adopt it.** Write the bar as numbers before measuring: both error rates, the confidence bound, the minimum held-out trials, how the threshold is chosen apart from the trials it is judged on, the room conditions and how many other people. Score synthetic voices separately and never count them toward the bar. Start each new user in a shadow mode that scores without refusing, and have the check switch itself off when live evidence shows it ignoring the user. Make every refusal visible and one phrase away from being undone.

**In Eileen.** Both one-sided 95 percent Clopper-Pearson bounds must be under 1 percent on at least 300 held-out command-length utterances each, by the owner and by at least five other consenting people, in four room conditions; each user gets a shadow period; the gate turns off after 3 false rejects in 300 gated turns; below the bar, echo cancellation, no approval from Eileen's own audio, physical clicks and the hotkey, and confirmation for every consequential action stay the protection ([BUILD §2.8](BUILD.md#28-voice), "The speaker check, used only above its bar", and [§2.5](BUILD.md#25-workers-routing-and-jobs), "Approvals"; [D51](DECISIONS.md#d51-approvals-and-the-conversation-window-revised), [D76](DECISIONS.md#d76-fit-the-machine-and-gate-on-the-speaker-only-above-a-measured-bar)). WP4 acceptance 13 is the measurement ([BUILD §6.6](BUILD.md#66-wp4-voice-audio-speech-dictation-voices-the-first-run-recording)).

**Sources.** [OWNER-DIRECTION, 2026-10-21](OWNER-DIRECTION.md#2026-10-21); [BUILD §2.8](BUILD.md#28-voice), [§6.6](BUILD.md#66-wp4-voice-audio-speech-dictation-voices-the-first-run-recording), [§7](BUILD.md#7-owner-decisions-settled-questions-and-risks); [D76](DECISIONS.md#d76-fit-the-machine-and-gate-on-the-speaker-only-above-a-measured-bar).

## 12. Adopt this in your project: a checklist, in order

Each step links to the section that explains it. The order matters: the early steps are cheap only at the start.

1. Write the owner's asks as dated, binding entries in one file, with a standing rule for unanswered questions ([§1.6](#16-a-binding-dated-owner-direction-file)).
2. Start a decision log with a fixed grammar, append-only, each record citing its source ([§1.7](#17-decision-records-that-supersede-rather-than-rewrite)).
3. Run parallel research passes, one question each, measuring on the target machine with tagged claims; copy their outputs to a durable folder as they go ([§1.1](#11-research-before-design-measured-on-the-target-machine), [§1.9](#19-durable-copies-of-scratch-work)).
4. Write one specification with contracts and work packages; route changes to it through amendment pull requests ([§1.2](#12-an-architect-pass-that-turns-the-reports-into-one-specification)).
5. Review it with independent critics, one lens each, then verify each blocking issue in bounded rounds, carrying measurement-only questions into acceptance tests ([§1.3](#13-independent-critics-with-distinct-lenses), [§1.4](#14-a-verification-loop-per-blocking-issue)).
6. Set up a handoff file and living work-package files before the first build session ([§1.8](#18-a-handoff-file-kept-current-and-living-work-package-files)).
7. If agents share a machine, measure its memory and put a build guard in front of every build before any agent compiles; deny direct builds in the harness settings ([§1.10](#110-memory-limits-on-parallel-agents)).
8. Create the workspace: flat crates, dependencies declared once with versions, licences and reasons, a cooling-off rule, exact pins for protocol crates ([§3](#3-what-we-pull-in-and-why), [§6.3](#63-crate-rules)).
9. Create the contracts crate with snapshot-pinned wire types and untrusted and personal wrappers ([§2.5](#25-a-contracts-crate-with-snapshot-pinned-types)).
10. Write the core as a sans-IO state machine with closed input and effect enums, injected time and randomness, bounded queues, and lint bans that keep it that way ([§2.1](#21-a-sans-io-core-driven-by-one-step-function)).
11. Put every OS-touching piece behind platform traits and test the neutral crates on a second OS from the first commit ([§2.4](#24-platform-crates-behind-traits-windows-first-and-linux-next)); read the machine through them too, and let the product fit its settings to it, with reasons the user can read and override ([§2.7](#27-fit-the-machine-it-runs-on)).
12. Put everything that can hang behind one supervisor with kill-on-close jobs, deadlines and heartbeats ([§2.3](#23-supervised-child-processes-for-anything-that-can-hang)).
13. Give every component that AI progress improves a trait, and name models only through aliases in configuration ([§2.2](#22-plug-in-traits-for-everything-ai-progress-improves), [§2.6](#26-model-aliases-in-configuration)).
14. Add request IDs, trace context across every boundary, named spans with a checked parent table, and a flight recorder with a replayable input lane ([§8](#8-observability)).
15. Build the simulator skeleton with a few invariants, a fixed pull request seed set and a regressions folder ([§5.1](#51-deterministic-simulation-from-seeds)).
16. Write the guidance: a root `AGENTS.md`, a `CLAUDE.md` that imports it, crate docs with required headings, nested files only where rules differ, and a short `ARCHITECTURE.md` ([§7](#7-guidance-for-people-and-ai-agents)).
17. Set up CI: a `plan` job, Linux jobs for neutral code, platform jobs in parallel, one aggregate required check, separate workflows for checks that read the description, SHA-pinned actions, read-only permissions, caches saved only from `main` ([§4.1](#41-lanes) to [§4.3](#43-caching-and-supply-chain)).
18. Add the evidence-first pull request template and its checks, and switch the required checks in the same pull request that replaces old CI ([§1.5](#15-evidence-first-then-the-change), [§4.2](#42-one-aggregate-required-check)).
19. Turn every convention into a registered check with one local command that runs what CI runs ([§4.6](#46-one-local-command-and-the-build-guard), [§7.5](#75-conventions-enforced-by-ci)).
20. Protect the main branch; without a merge queue, require up-to-date branches and update open pull requests after each merge ([§4.4](#44-branch-protection-without-a-merge-queue)).
21. Generate the code map, the event map and the reference pages, and check them fresh ([§6](#6-code-maps-and-code-graphs)).
22. Add fuzz targets for every parser of untrusted input, with corpus replay on every OS ([§5.6](#56-fuzzing-on-linux-corpus-replay-everywhere)).
23. Define the scenario schema, the capture and share flow, and the scenario-only CI lane; turn the owner's requests into timed scenarios ([§5.3](#53-scenarios-from-real-use), [§5.4](#54-the-owners-requests-as-timed-scenarios)).
24. Add the nightly, weekly, long and release lanes: soak, mutation, A/B benchmarks with a results store, CLI drift against the latest vendor tools ([§5.7](#57-soak-tests) to [§5.9](#59-benchmarks-as-ab-tests-with-a-results-store), [§10.2](#102-nightly-runs-against-the-latest-vendor-tools)).
25. Publish nightly, beta and stable channels from CI ([§4.5](#45-release-channels)).
26. Open the test fleet to friends with a machine-hash identity and a recorded mode ([§5.5](#55-the-community-test-fleet)).
27. Settle safety before features that act: the credential boundary, permission tiers with approvals only from the user, untrusted-content wrappers and taint, consent for voices and recordings, and a voice check that gates nothing until it clears a measured bar ([§11](#11-safety-and-credentials), [§11.5](#115-a-voice-check-gates-nothing-until-it-clears-a-measured-bar)).
28. Add the learning loop and the self-improvement loops once capture, replay and the permission gate exist ([§9](#9-improvement-loops)).
29. Keep this kind of playbook for your own project, cited, and update it whenever a decision changes ([D50](DECISIONS.md#d50-a-reusable-fully-cited-engineering-playbook)).

## References

### Eileen documents

- [OWNER-DIRECTION.md](OWNER-DIRECTION.md): the owner's binding direction, entries of [2026-10-11](OWNER-DIRECTION.md#2026-10-11), [2026-10-20](OWNER-DIRECTION.md#2026-10-20) and [2026-10-21](OWNER-DIRECTION.md#2026-10-21), including the answers to BUILD.md's open questions.
- [BUILD.md](BUILD.md): the v0.1 specification; sections cited: [§0](BUILD.md#0-summary), [§1](BUILD.md#1-the-target-experience), [§2](BUILD.md#2-the-architecture-as-it-will-be-built-now), [§2.1](BUILD.md#21-processes), [§2.3](BUILD.md#23-the-brain), [§2.5](BUILD.md#25-workers-routing-and-jobs), [§2.10](BUILD.md#210-the-learning-loop), [§2.11](BUILD.md#211-settings-by-voice-and-configuration), [§2.12](BUILD.md#212-permission-tiers-and-security), [§2.13](BUILD.md#213-observability), [§2.14](BUILD.md#214-connectors-and-platforms), [§2.16](BUILD.md#216-fitting-the-machine-one-machine-profile), [§3](BUILD.md#3-workspace-and-contracts), [§3.1](BUILD.md#31-the-cargo-workspace), [§3.3](BUILD.md#33-contracts), [§3.3.17](BUILD.md#3317-the-machine-profile), [§3.4](BUILD.md#34-platform-traits), [§4.1](BUILD.md#41-dependency-pins), [§4.2](BUILD.md#42-models-and-eileen-setup), [§4.3](BUILD.md#43-latency-budget-per-stage), [§4.4](BUILD.md#44-memory-budget), [§4.5](BUILD.md#45-idle-budget), [§4.6](BUILD.md#46-what-runs-where), [§4.7](BUILD.md#47-test-strategy), [§4.8](BUILD.md#48-real-use-becomes-tests-the-community-scenario-corpus-and-the-test-fleet), [§4b.2](BUILD.md#4b2-lints-clippy-configuration-and-unsafe-policy), [§4b.3](BUILD.md#4b3-cargo-deny-nextest-and-ci), [§4b.4](BUILD.md#4b4-guidance-for-people-and-coding-agents), [§4b.5](BUILD.md#4b5-cargo-xtask), [§4b.6](BUILD.md#4b6-the-engineering-playbook-and-reusable-templates), [§6.1](BUILD.md#61-building-on-the-owners-pc-without-thrashing-it), [§6.2](BUILD.md#62-wp0-foundation-workspace-contracts-and-quality-gates), [§6.6](BUILD.md#66-wp4-voice-audio-speech-dictation-voices-the-first-run-recording), [§7](BUILD.md#7-owner-decisions-settled-questions-and-risks), [§8](BUILD.md#8-revision-log).
- [DECISIONS.md](DECISIONS.md): decision records D1 to D76; cited here: D1, D4, D5, D8, D9, D14, D17 to D29, D31 to D35, D37 to D40, D43 to D46, D49 to D66, D68 to D76.
- [RESEARCH.md](RESEARCH.md): the September brief from five parallel research passes.
- [ROADMAP.md](ROADMAP.md): milestones, [v0.2](ROADMAP.md#v02) and [after v0.1](ROADMAP.md#after-v01-ships-staying-at-the-frontier).
- [Pull request template](../.github/PULL_REQUEST_TEMPLATE.md).

### Research reports (docs/research/, October 2026)

- [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) (2026-10-20): repository practice, tests, guidance files, CI.
- [2026-10-observability.md](research/2026-10-observability.md) (2026-10-20): request IDs, flight recorder, replay, code maps.
- [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) (2026-10-20): workspace, versions, measurements, parallel builds.
- [2026-10-learning-loop.md](research/2026-10-learning-loop.md) (2026-10-20): prior art and design of the learning loop.
- [2026-10-voice-stack.md](research/2026-10-voice-stack.md) (2026-10-20): the local voice pipeline.
- [2026-10-dictation-and-voices.md](research/2026-10-dictation-and-voices.md) (2026-10-20): dictation and configurable voices.
- [2026-10-subscription-harnesses.md](research/2026-10-subscription-harnesses.md) (2026-10-11): vendor harnesses and policy.
- [2026-10-brain-latency.md](research/2026-10-brain-latency.md) (2026-10-20): warm model turns on the owner's plan.
- [2026-10-computer-use.md](research/2026-10-computer-use.md) (2026-10-11 and 2026-10-20): the desktop control layer.
- [2026-10-app-hooks.md](research/2026-10-app-hooks.md) (2026-10-11, refreshed 2026-10-20): deterministic hooks into the owner's apps.
- [2026-10-claude-mods.md](research/2026-10-claude-mods.md) (2026-10-20): Claude's extension points.
- [2026-10-prior-art.md](research/2026-10-prior-art.md) (2026-10-11): prior art and novelty.
- [2026-10-colab-ml-projects.md](research/2026-10-colab-ml-projects.md) (2026-10-20): the v0.2 Colab connector.

### Primary sources, read by the research passes

Dates are when the citing report read them.

Architecture, style and testing (read 2026-10-20):

- rust-analyzer, architecture document: <https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md>
- rust-analyzer, CI workflow: <https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/ci.yaml>
- matklad, ARCHITECTURE.md: <https://matklad.github.io/2021/02/06/ARCHITECTURE.md.html>
- matklad, Delete Cargo Integration Tests: <https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html>
- matklad, Large Rust Workspaces: <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>
- matklad, Test Case Minimization: <https://matklad.github.io/2026/04/20/test-case-minimization.html>
- matklad, Catch Flakes On Main: <https://matklad.github.io/2026/05/14/catch-flakes-on-main.html>
- matklad, Finding Bugs: <https://matklad.github.io/2026/09/19/finding-bugs.html>
- TigerBeetle, VOPR: <https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/vopr.md>
- TigerBeetle, devhub: <https://github.com/tigerbeetle/tigerbeetle/blob/main/src/scripts/devhub.zig>
- FoundationDB, testing: <https://apple.github.io/foundationdb/testing.html>
- FoundationDB, `Buggify.h`: <https://github.com/apple/foundationdb/blob/main/flow/include/flow/Buggify.h>
- Turso, CI workflow: <https://github.com/tursodatabase/turso/blob/main/.github/workflows/rust.yml>
- Turso, `AGENTS.md`: <https://github.com/tursodatabase/turso/blob/main/AGENTS.md>
- Biome, `AGENTS.md`: <https://github.com/biomejs/biome/blob/main/AGENTS.md>
- uv, `AGENTS.md`: <https://github.com/astral-sh/uv/blob/main/AGENTS.md>
- uv, CI workflow: <https://github.com/astral-sh/uv/blob/main/.github/workflows/ci.yml>
- ruff, daily fuzz workflow: <https://github.com/astral-sh/ruff/blob/main/.github/workflows/daily_fuzz.yaml>
- Zed, `.rules`: <https://github.com/zed-industries/zed/blob/main/.rules>
- Claude, memory and `AGENTS.md` documentation: <https://code.claude.com/docs/en/memory>
- Codex, `AGENTS.md` guide: <https://learn.chatgpt.com/docs/agent-configuration/agents-md>
- OpenAI cookbook, execution plans (secondary): <https://developers.openai.com/cookbook/articles/codex_exec_plans>
- cargo-fuzz: <https://github.com/rust-fuzz/cargo-fuzz>
- cargo-mutants book: <https://github.com/sourcefrog/cargo-mutants/tree/main/book/src>
- Firecracker, integration tests and A/B testing: <https://github.com/firecracker-microvm/firecracker/blob/main/tests/README.md>
- wezterm, cargo cooldown workflow: <https://github.com/wezterm/wezterm/blob/main/.github/workflows/cargo-cooldown.yml>
- GitHub, managing a merge queue: <https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue>

Observability (read 2026-10-20):

- MCP SEP-414, trace context in `_meta`: <https://modelcontextprotocol.io/seps/414-request-meta>
- rmcp, trace context accessors: <https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/model/meta.rs>
- Claude CLI, monitoring and OpenTelemetry: <https://code.claude.com/docs/en/monitoring-usage>
- LiveKit Agents, trace schema test: <https://github.com/livekit/agents/blob/main/tests/trace_schema.py>
- Go, flight recorder: <https://go.dev/blog/flight-recorder>
- Embark, crash handling: <https://github.com/EmbarkStudios/crash-handling>
- Zed, crash handling (GPL, design reference only): <https://github.com/zed-industries/zed/blob/main/crates/crashes/src/crashes.rs>
- Perfetto, other trace formats: <https://perfetto.dev/docs/getting-started/other-formats>
- await-tree: <https://github.com/risingwavelabs/await-tree>

Learning loop (read 2026-10-20):

- Voyager: <https://arxiv.org/abs/2305.16291>
- Agent Workflow Memory: <https://arxiv.org/abs/2409.07429>
- SkillWeaver: <https://arxiv.org/abs/2504.07079>
- ASI, inducing programmatic skills: <https://arxiv.org/abs/2504.06821>

Vendors, apps and engines:

- Claude, legal and compliance (read 2026-10-20): <https://code.claude.com/docs/en/legal-and-compliance>
- Claude, hooks reference (read 2026-10-20): <https://code.claude.com/docs/en/hooks>
- Cua Driver 0.37.0 release (read 2026-10-20): <https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.37.0>
- DaVinci Resolve 21.1 readme (read 2026-10-20): <https://www.blackmagicdesign.com/support/content/readme/baf7c071c0524fbf8ccc961925c9f443>
- Handy (read 2026-10-20): <https://github.com/cjpais/Handy>
- Picovoice on-device TTS benchmark, vendor-run (secondary, read 2026-10-20): <https://picovoice.ai/blog/on-device-tts/>
- UI Automation transaction timeout (read 2026-10-20): <https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_transactiontimeout>
- RustSec advisory RUSTSEC-2025-0068, `serde_yml` (read 2026-10-20): <https://rustsec.org/advisories/RUSTSEC-2025-0068.html>

### New primary sources, checked for this document on 2026-10-21

- GitHub, Dependabot options reference (the `cooldown` option): <https://docs.github.com/en/code-security/dependabot/working-with-dependabot/dependabot-options-reference>
- GitHub, about protected branches (strict and loose required status checks): <https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches>
- W3C Trace Context, Recommendation of 2021-11-23 (an all-zero trace id is invalid): <https://www.w3.org/TR/trace-context/>
- GitHub, managing a merge queue (its availability note: organisation-owned public repositories, or private ones on GitHub Enterprise Cloud): <https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue>
- crates.io API, licence and latest version per crate (the rows marked "registry" in section 3), and rmcp's release dates: <https://crates.io/api/v1/crates/cargo-nextest> and the same path for each crate named
- GitHub REST API, licence per repository (the CI actions in section 3.5): <https://api.github.com/repos/actions/checkout> and the same path for each action named
- Rust, licence statement in the repository README (the Rust row of section 3.1): <https://github.com/rust-lang/rust>
- TigerBeetle, hacking guide (`zig build ci` as the CI entry point): <https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/HACKING.md>
- ruff, `CLAUDE.md` (the single line `@AGENTS.md`): <https://github.com/astral-sh/ruff/blob/main/CLAUDE.md>
- codex, a per-directory `AGENTS.md` (docs-sync rules for the terminal UI's bottom-pane state machines): <https://github.com/openai/codex/blob/main/codex-rs/tui/src/bottom_pane/AGENTS.md>
- Claude, memory documentation, re-checked for "Longer files consume more context and reduce adherence" and for a `PreToolUse` hook as the way to block an action: <https://code.claude.com/docs/en/memory>
- SkillWeaver abstract, re-checked for "relative" success rate improvements: <https://arxiv.org/abs/2504.07079>
