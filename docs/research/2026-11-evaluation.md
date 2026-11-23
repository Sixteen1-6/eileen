# Evaluation methods for Eileen

Research pass, 2026-11-07, for the owner direction of the same day: an opt-in pipeline that spends tokens on purpose and returns an evaluation, cross-evaluating Eileen across routes, models, modes and skills, built from the best of every known way of evaluating and automated. It is written for WP9, which builds the framework (BUILD.md section 4.9), and for anyone who adds a suite.

## How to read this

Every claim carries a tag saying how it was checked:

- **[primary]**: read on the maintainer's or vendor's own page during this pass.
- **[secondary]**: from a search summary, an aggregator or a paper abstract seen through a search, not read in full. Treat numbers as soft.
- **[recalled]**: the author's knowledge of well-known work, not re-read for this pass.
- **[inferred]**: our reasoning from the above.

## 1. Verdict

No existing framework fits Eileen as it is. Eileen is a voice-first desktop agent that routes work to vendor CLIs signed in on the user's own plan; the agent-evaluation frameworks assume model calls they can proxy with API keys and a Docker sandbox, and each benchmark measures one capability. So Eileen keeps its own runner, in Rust, on the scenario format it already has (BUILD.md sections 3.3.14 and 4.8), and takes the best method from each field into it. Thin adapters run the external benchmarks in their own harnesses for comparability, where Eileen's rules allow [inferred].

| Method | From | What Eileen takes | BUILD.md 4.9 |
|---|---|---|---|
| Ground truth from the world's state | OSWorld, Windows Agent Arena, AgentDojo | A case passes only on state expectations; graded scores sit on top | Cases |
| Reliability over repeated trials | tau-bench's pass^k | k repeats per live cell; pass^1, pass^k and pass@k | Repeats and statistics |
| A declared matrix of providers and prompts | promptfoo | A matrix of routes, models, modes and skills, priced before it runs | The matrix |
| Harness as the agent under test | Harbor, inspect_swe | The serving route is an axis; transcripts kept per run | The matrix, Reports |
| Rubric judges, pairwise, bias controls | MT-Bench and Chatbot Arena studies | Pairwise against a baseline, both orders, a judge from another vendor, calibrated on the owner's labels | Judges |
| Blind human preference | Chatbot Arena | An arena for the owner and friends, Bradley-Terry ranking | The arena |
| Simulated users | tau-bench | A user simulator for multi-turn cases | Simulated users |
| Turn-taking metrics | Full-Duplex-Bench | Pause handling, backchannels, turn-taking and interruption in the voice suite | Suites |
| Perturbations and faults | ReliabilityBench, CheckList | Metamorphic variants and injected faults | Robustness and faults |
| Utility and security under injection | AgentDojo | Attack cases with deterministic security checks, plus a canary credential | Security |
| Error bars and paired tests | Standard statistics | Intervals, McNemar, paired bootstrap, Holm correction | Repeats and statistics |
| Contamination controls | BIG-bench | A holdout split and a canary string in eval files | Contamination and overfitting |

## 2. What the plan already has, and what it lacks

BUILD.md section 4.7 has unit, property, snapshot, simulation, exhaustive, concurrency, fuzzing, end-to-end, soak and mutation layers; benchmarks B1 to B10 and B8-curve, timed from event spans and judged by A/B with a Mann-Whitney test inside admission strata; results as JSON lines on the `devhub` branch with `RESULTS.md` and a Pages page. Section 4.8 adds the scenario corpus from real use and the test fleet, with a recorded mode (`--replay-models`) and a live mode on each contributor's own plan.

What it lacks against the owner's ask [inferred]:

1. No cross-configuration matrix. B10 compares brain backends; nothing compares routes, models, depth levels, serving layers, skills or speech stages against each other in one run.
2. Open-ended output is not graded. A spoken answer, narration or a deep answer passes on timing and a few checks; its quality is not scored.
3. Reliability is not measured. A live case runs once; an agent that succeeds two times in three looks the same as one that always does.
4. No live tier in CI. Real-model runs happen only on the owner's PC at night, so a pull request cannot ask for a live evaluation and get a report.
5. No human preference signal and no check that any automatic grader agrees with the owner.
6. Injection resistance is checked as simulator invariants, not as live attack cases with a measured success rate.

## 3. Methods surveyed

### 3.1 Execution-based checks

- **OSWorld-Verified** is the OSWorld team's corrected release: 369 tasks, 361 without the Google Drive tasks that need manual setup, with maintainer-run results under consistent settings [secondary]. Leaderboard numbers differ between aggregators and many rows are self-reported, so they are not comparable across reports [secondary].
- **Windows Agent Arena** (Microsoft, 2024) adapts the OSWorld framework to 150+ Windows tasks across planning, screen understanding and tool use; its Navi agent reached 19.5 percent against 74.5 percent for unassisted humans at release [secondary]. Later systems reportedly narrowed the gap; that claim was not verified against a primary source [secondary].
- **AgentDojo** (ETH Zurich, NeurIPS 2024) has 97 tasks and 629 security cases across workspace, chat, travel and banking settings; each task has formal utility and security functions that inspect environment state, so grading does not depend on a model that could itself be injected [secondary].

Eileen already checks state (scenario expectations such as `window_snapped`, `field_text`, `resolve_playhead`). The lesson is to keep that the ground truth for every case, graded parts included.

### 3.2 Reliability over repeats

tau-bench defines pass^k as the chance that all k independent trials of a task succeed, averaged over tasks, against pass@k (at least one succeeds), with the unbiased estimate C(c, k) / C(n, k) for c successes in n trials; GPT-4o's retail pass^1 of about 61 percent fell below 25 percent at pass^8 [secondary]. ReliabilityBench (2026) argues consistency alone is not enough and adds perturbations and fault injection, and notes pass^k is not simply (pass^1)^k because trials are correlated [secondary]. For an assistant people rely on every day, pass^k is the number that matters [inferred].

### 3.3 Agent harness frameworks

- **Inspect** (UK AI Security Institute) splits an evaluation into a dataset, a solver and a scorer, runs agents in sandboxes and keeps full transcripts with a log viewer [recalled].
- **inspect_swe** (Meridian Labs) provides Claude Code, Codex CLI, Gemini CLI, Kimi Code, OpenCode and Mini SWE Agent as Inspect agents; the agent runs in the sample's sandbox and its model API calls are proxied back to Inspect through `sandbox_agent_bridge()`, so token limits and transcripts work as usual [primary]. Release 0.2.52 is dated 2026-05-09 [secondary].
- **Harbor**, from the creators of Terminal-Bench, is the official harness for Terminal-Bench 2.0; it runs Claude Code, OpenHands, Codex CLI and other agents against datasets including SWE-Bench and Aider Polyglot, locally in Docker or on cloud sandboxes, under Apache-2.0 [primary].

What Eileen takes: the dataset, solver and scorer split (Eileen's case, cell and expectations), the serving harness as an axis of the matrix, and one transcript per run. What it does not take: proxying the agent's model calls through the framework's providers, which needs API keys. Eileen's rule is subscriptions through each vendor's own signed-in CLI, with DeepSeek's key the only one allowed (README principle 4), so an inspect_swe run would break it. Eileen borrows the method and may write Inspect-compatible logs later; it does not run through it [inferred].

### 3.4 Declarative matrices and assertions

promptfoo runs a matrix of providers, prompts and test cases with assertions (exact, regex, JSON, model-graded rubrics), red-team suites and pull request comments from CI [recalled]. OpenAI announced on 2026-03-09 that it would acquire promptfoo; the project says it stays open source under MIT [secondary]. Eileen takes the declared matrix, the cost estimate before a run and the pull request report, not the tool itself (Node, provider keys).

### 3.5 Model judges

The MT-Bench and Chatbot Arena study (Zheng and others, 2023) found strong model judges agree with human preferences about as often as humans agree with each other, and named three biases: position (favouring the first answer), verbosity (favouring longer answers) and self-enhancement (favouring their own model's answers) [recalled]. The usual mitigations are judging both orders, controlling for length, using a reference answer and using a judge from another model family [recalled]. Eileen adds a trust rule: a judge counts toward a gate only while it agrees with the owner's own labels (section 4.9).

### 3.6 Blind human preference

Chatbot Arena collects blind pairwise votes between two anonymous models and ranks them with a Bradley-Terry model and confidence intervals [recalled]. The same shape works for configurations of Eileen, rated by the owner and by friends who opt in.

### 3.7 Simulated users

tau-bench has a language model play the user, with a goal and constraints, so multi-turn tool use can be graded at scale [secondary]. Eileen's conversations ("no, the other one", interruptions, changes of mind) need the same.

### 3.8 Spoken dialogue

Full-Duplex-Bench (Lin and others, 2025) evaluates full-duplex spoken dialogue models on four areas with automatic metrics: pause handling, backchanneling, turn-taking and interruption management [secondary]. Eileen's voice loop is not one end-to-end model, but the four areas map onto what its voice suite should measure, with its WAV source, room mixer and capture sink (BUILD.md section 4.7) [inferred].

### 3.9 Statistics

Rates get Wilson score intervals; paired pass or fail between two configurations on the same cases gets McNemar's exact test; scores get paired bootstrap intervals; many comparisons in one matrix get the Holm correction [recalled]. "Adding Error Bars to Evals" (Miller, 2024) recommends standard errors from the central limit theorem, clustered errors when cases share a source, and paired differences between models on the same questions [recalled]. The existing Mann-Whitney A/B for timings stays.

### 3.10 Robustness

CheckList (Ribeiro and others, 2020) tests invariance: an output should not change under a perturbation that should not matter, such as a paraphrase [recalled]. ReliabilityBench adds injected faults [secondary]. Eileen already injects faults in simulation (`buggify!`) and plays audio through a room mixer.

### 3.11 Contamination

BIG-bench marks its files with a canary string so that a model trained on them can be detected, and asks that they stay out of training data [recalled]. A holdout split that is never used to tune prompts or rules catches overfitting to the suites [recalled].

### 3.12 Evaluating the evaluator

A scorer that cannot fail tells nothing. Mutation testing of scorers (feeding outputs that must fail) is the same idea as `cargo mutants`, which the plan already runs on code [inferred].

## 4. Live evaluations on the owner's plan

Facts from the vendor's documentation [primary]:

- `claude setup-token` makes a one-year OAuth token for a Pro, Max, Ultra, Team or Enterprise plan. It prints the token and saves it nowhere; it is used as `CLAUDE_CODE_OAUTH_TOKEN`, "for CI pipelines and scripts where browser login isn't available". It "can only make model requests", so it cannot start Remote Control sessions or fetch claude.ai connectors.
- On public repositories GitHub withholds secrets from runs triggered by fork pull requests.
- Deleting a GitHub secret does not invalidate the credential it held.
- `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` outrank `CLAUDE_CODE_OAUTH_TOKEN`, and in non-interactive mode an `ANTHROPIC_API_KEY` that is present is always used. Bare mode does not read the OAuth token.
- Workload identity federation (no stored secret) exists for Claude API access through a Console service account, not for a subscription.

Risks:

- The code under test runs beside the credential, and the thing under test is an agent that runs commands, so a hostile file or page could ask it to print its environment [inferred].
- A compromised third-party action can read a job's secrets: in March 2025 `tj-actions/changed-files` was altered to dump runner memory, secrets included, into public build logs (CVE-2025-30066) [recalled].
- GitHub masks a secret in logs only where its exact text appears, so an encoded copy passes [recalled].

The guards in BUILD.md section 4.9 answer these: a protected environment the owner approves per run, the workflow always taken from `main`, refusal of pull requests from forks, the secret in one step only, an outbound allowlist, a canary token and a scan of every log and artifact before upload, separate jobs for posting results, and a rotation clock. The owner's PC stays out of GitHub's runner pool (BUILD.md section 4.6); local runs pull CI's artifacts instead [inferred].

## 5. External benchmarks worth running

| Benchmark | Measures | Harness | Fit for Eileen | Caveat |
|---|---|---|---|---|
| Windows Agent Arena | Windows desktop tasks | its own, a Windows VM | WP5 desktop control and WP6 apps | Needs a Windows VM image; runs are long |
| OSWorld-Verified | Desktop tasks, mostly Linux | its own | Method only, plus any Windows subset | Settings differ between reported rows |
| tau2-bench | Conversational tool use with a simulated user, pass^k | its own | The brain and the conversation suite | Customer-service domains, not desktop |
| AgentDojo | Prompt injection, utility under attack | its own | The security suite's method and a comparable number | Text agents, not multimodal |
| Full-Duplex-Bench | Turn-taking in spoken dialogue | its own scripts | The voice suite's metrics | Built for end-to-end speech models |
| Terminal-Bench 2.0 | Terminal tasks for coding agents | Harbor | The worker route | Measures the CLI as much as Eileen |

## 6. Open questions

1. How the owner revokes a `claude setup-token` token early was not in the pages read; WP9 confirms the steps before the live lane first runs and records them in `docs/EVAL.md`.
2. Whether a DeepSeek judge reaches the agreement bar with the owner's labels is unknown until the gold set exists.
3. Windows Agent Arena's VM may not fit a hosted runner's time and disk limits; phase C measures it before committing to a nightly.
4. Hosted runners have no audio device, so the live voice suite runs on WAV input and capture there, and the owner's PC runs the strict audio cases.

## Sources

- [Claude Code GitHub Actions](https://code.claude.com/docs/en/github-actions) and [Authentication](https://code.claude.com/docs/en/authentication), read 2026-11-07
- [Inspect SWE](https://meridianlabs-ai.github.io/inspect_swe) and its [changelog](https://meridianlabs-ai.github.io/inspect_swe/CHANGELOG.html)
- [Harbor](https://github.com/laude-institute/harbor)
- [Windows Agent Arena](https://microsoft.github.io/WindowsAgentArena) and [arXiv 2409.08264](https://arxiv.org/abs/2409.08264)
- [OSWorld-Verified listing](https://benchmarklist.com/benchmarks/osworld_verified/)
- [tau-bench, arXiv 2406.12045](https://arxiv.org/pdf/2406.12045) and [ReliabilityBench, arXiv 2601.06112](https://arxiv.org/html/2601.06112v1)
- [AgentDojo, NeurIPS 2024](https://proceedings.neurips.cc/paper_files/paper/2024/hash/97091a5177d8dc64b1da8bf3e1f6fb54-Abstract.html)
- [Full-Duplex-Bench, arXiv 2503.04721](https://arxiv.org/abs/2503.04721v1)
- [OpenAI to acquire Promptfoo](https://openai.com/hy-AM/index/openai-to-acquire-promptfoo/)
