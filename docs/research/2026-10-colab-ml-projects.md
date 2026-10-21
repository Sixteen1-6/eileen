# ML projects end to end on the owner's Google Colab

Research and design pass, 2026-10-20. Question: how should Eileen run a machine learning project for the owner, from "I want a model that finds a person's clothing colour" to a model running locally as one of Eileen's own skills, using the owner's own Google Colab account for training? This answers the owner entry "ML projects end to end, with the owner's Google Colab" in [OWNER-DIRECTION.md](../OWNER-DIRECTION.md) (2026-10-20). The connector is v0.2; this report also says what v0.1 must keep possible.

It is written for contributors who will build the v0.2 ML project job and its Colab connector, and for the owner, who decides the route, the Colab tier and the one-time setup.

## How to read this

Tags say how each claim was checked:

- **[primary]**: Google's, the vendor's or the authors' own page, repository, source code, release or paper. Repository facts (versions, dates, issue threads, source lines) were read through the public GitHub, PyPI and Hugging Face APIs on 2026-10-20.
- **[secondary]**: press, blogs, third-party measurements, search-result summaries. Treat details as soft.
- **[inferred]**: our reasoning from the above. Every threshold, budget and time estimate in the design is a starting value, not a measurement.

Nothing was installed, nothing was signed in to, and the owner's Colab, Drive and Google account were not opened. No command ran against Google's services except anonymous reads of public pages. So no Colab route was exercised end to end; section 6.6 lists the spike that does that, with the owner signing in himself.

## 1. Verdict

1. **Google now ships three official ways for software to drive Colab, and all three are meant for agents.** The [Colab CLI](https://github.com/googlecolab/google-colab-cli) (`google-colab-cli` 0.7.5 on PyPI, released 2026-10-20, Apache-2.0) provisions CPU, GPU and TPU runtimes, runs local scripts and notebooks on them, moves files and reports compute-unit usage; its README says it is "designed to support seamless developer productivity, headless automation, and AI agent integrations", and it ships an agent skill. The [Colab MCP server](https://github.com/googlecolab/colab-mcp) (announced 2026-03-17) bridges a local agent to a Colab notebook open in the user's browser. The [Colab extension for VS Code](https://github.com/googlecolab/colab-vscode) (0.9.7) turns Colab runtimes into Jupyter kernels inside VS Code [primary]. A fourth, the [Colab API](https://developers.google.com/colab/api/reference/rest) (`colaboratory.googleapis.com`), exists but is a beta behind an allowlist [primary].
2. **None of them is a clean fit for Windows today.** The CLI says it supports Linux and macOS only and crashes on Windows at import (an unguarded `termios` import; issue #100, reopened 2026-10-04, "no released version fixes the Windows crash yet"); a user reports it works under WSL [primary]. The MCP server's notebook tools are announced dynamically and several clients, Claude's among them in some versions, never see them; its last commit was 2026-04-01 and an issue from 2026-09-16 asks whether the project is dead [primary]. The VS Code extension cannot be driven headlessly, and Claude's VS Code integration asks the user to approve every notebook cell it runs [primary].
3. **Recommended primary route: the official Colab CLI, run inside WSL2 by a small Eileen connector that calls a fixed list of subcommands.** It is Google's own client, built for agents, covers the whole lifecycle (allocate, upload, run, stream, download, stop, check the balance), and keeps every Google credential inside Google's tool, where the owner signs in once. Eileen never sees a token. **Fallback: the Colab MCP bridge**, re-implemented in Rust on Eileen's side (a loopback WebSocket that the owner's own signed-in Colab tab connects to), which drives the notebook UI itself and needs no Google credential at all. Section 6 gives the full order and the reasons.
4. **Colab's terms do not forbid this, but they set real limits.** Colab "prioritizes interactive compute", runtimes time out when idle, notebooks run at most about 12 hours (24 hours of continuous execution on Pro+ with enough compute units), limits are deliberately unpublished, and on the free tier "remote control such as SSH shells" and "running distributed computing workers" may be terminated without warning [primary, FAQ read 2026-10-20]. Eileen must keep its Colab use looking like what it is, one person training one model while present: no fake activity to defeat idle timeouts, no second accounts, no serving web UIs or demos from the runtime, no SSH or console on the free tier, always stop the runtime when done.
5. **For the owner's example, training is probably not the first answer.** Garment segmentation models already exist (an upper-clothes IoU of 0.78 and a pants IoU of 0.84 for the most downloaded one [primary, model card]), colour can be measured from the segmented pixels without a model, and permissively licensed zero-shot models can double-check the colour name. The honest unknown is accuracy on the owner's own photos, so the job's first deliverable is that measurement on 50 to 100 of his images, not a training run. If training is needed, the cheapest useful version (a small colour head on frozen image embeddings) trains on the owner's CPU in minutes and needs no Colab at all; Colab is for fine-tuning a segmentation network [inferred].
6. **Licences decide more than accuracy here.** Most human-parsing datasets and the weights trained on them are non-commercial or unlicensed (ATR has no licence, ModaNet is CC BY-NC 4.0, the SegFormer backbone is "for research or evaluation purposes only") [primary]. That is fine for the owner's personal model and wrong for anything Eileen ships or shares. The job must carry a licence chain from every dataset and base model to the exported model, and refuse to mark a model shareable when any link is non-commercial.

## 2. The routes to Colab, one by one

Read this table first; each row has its own subsection.

| Route | Who makes it | Drives what | Headless | Auth (who holds the credential) | Windows | State on 2026-10-20 |
|---|---|---|---|---|---|---|
| Colab CLI | Google | Runtimes, kernel execution, files, usage | Yes | Google OAuth in the CLI; token in the CLI's config folder | No (WSL works) | Active, 0.7.5 released today, "still very much experimental" |
| Colab MCP server | Google | Cells of a notebook open in the user's browser | No, needs the tab | The user's browser session; a random local token | Yes (Python 3.13 and uv) | Dormant since 2026-04-01; dynamic-tools bug |
| Colab extension for VS Code | Google | Colab runtimes as VS Code kernels | No | Google OAuth in VS Code | Yes | Active, 0.9.7 |
| Colab API (`colaboratory.googleapis.com`) | Google | Runtime assignments | Yes | A Google Cloud project, allowlisted | n/a (REST) | Beta, allowlist only |
| Browser automation of the Colab page | Eileen | The notebook UI by clicks and typing | No | The user's browser session | Yes | Possible, brittle |
| Notebooks in Drive or GitHub | Google | Where the notebook lives | n/a | Drive or GitHub | Yes | Stable; not a control channel |
| Local runtime | Google | Colab UI on the user's own machine | n/a | Local token | Yes (Docker or Jupyter) | Stable; no NVIDIA GPU on the owner's PC |
| Colab Enterprise, Vertex AI | Google Cloud | Managed runtimes, scheduled executions | Yes | Google Cloud IAM, billing account | n/a (REST, gcloud) | Stable; needs billing |

### 2.1 The Colab CLI (`google-colab-cli`)

**What it is.** A Python CLI from the `googlecolab` organisation, repository created 2026-04-17, 1,640 stars, Apache-2.0, 12 releases on PyPI between 0.5.4 (2026-05-22) and 0.7.5 (2026-10-20) [primary]. Commands, from its README [primary]:

- Sessions: `colab new [-s NAME] [--gpu T4|L4|G4|H100|A100] [--tpu v5e1|v6e1] [--high-mem]`, `colab sessions`, `colab status`, `colab restart-kernel`, `colab stop`, `colab url [--open]` (prints or opens a browser URL that attaches the Colab web UI to the same runtime).
- Execution: `colab exec -f file.py` or `-f notebook.ipynb` (the file is read locally and sent to the kernel; a notebook's outputs are written to `<name>_output.ipynb`), `colab run SCRIPT` (allocate, run, tear down in one command, exit code propagated), `colab repl`, `colab console` (a tmux shell), `colab ssh` (SSH over WebSocket, added in 0.7.0).
- Files: `colab ls`, `colab upload LOCAL REMOTE`, `colab download REMOTE LOCAL`, `colab rm`, `colab edit`.
- Utilities: `colab install` (uv, falling back to pip), `colab drivemount`, `colab auth` (credentials for Google Cloud services inside the VM, not for the CLI), `colab log -o file.ipynb|.md|.jsonl` (exports session history), `colab usage` (compute-unit balance and current burn rate), `colab pay`.

Kernel state persists between `colab exec` calls in the same session, so an agent can build up state step by step; the default working directory is `/content` [primary, the bundled `SKILL.md`].

**Agent use is the stated purpose.** The README pitches "headless automation, and AI agent integrations"; the package bundles `skills/colab-operator/SKILL.md`, an agent skill with rules such as "Always `colab stop -s <name>` when done" and "Never run `colab repl`, `colab console`, `colab auth`, or `colab drivemount` interactively from an agent" [primary]. This is the strongest signal on 2026-10-20 that Google sanctions software driving Colab on a user's behalf through this tool.

**Authentication.** Two providers [primary, `docs/04_automation_and_utility.md` and `auth.py`]:

- `oauth2`: Google's installed-app flow with a copy-paste step (since 0.6.0, 2026-06-16): the CLI prints a Google URL, the user signs in in their browser and pastes the code back. It uses the Cloud SDK's public OAuth client and requests six scopes: `openid`, `userinfo.profile`, `userinfo.email`, `cloud-platform`, `colaboratory` and `drive.file`. The token is cached at `~/.config/colab-cli/token.json`.
- `adc`: Application Default Credentials from `gcloud auth application-default login` with four scopes.

The documentation disagrees about the default: the README and `SKILL.md` say `adc`, while `cli.py` defaults to `AuthProvider.OAUTH2` [primary]. Eileen should pass `--auth=oauth2` explicitly so the owner needs no Google Cloud SDK.

Either way the user signs in himself, in Google's own page, and the credential stays in the CLI's folder. That fits the owner's rule that Eileen never runs a login flow and never reads credential files, exactly as with the Claude CLI.

**Known problems that matter to Eileen** [primary, issue threads]:

- **Windows.** "the Colab CLI currently supports Linux and macOS only" (README). Issue #100 (Windows crashes on `termios`) was reopened on 2026-10-04 with "no released version fixes the Windows crash yet"; a commenter's workaround was WSL. A project member (who moved the question to Discussions) wrote on 2026-07-08 that there is no public roadmap, Windows work is ongoing without a dedicated plan, and the CLI is "still very much experimental" (discussion #77).
- **Tokens in a plain-text log.** Issue #177 (opened 2026-10-20, open): the CLI sets DEBUG logging and always writes `~/.config/colab-cli/colab.log`, which then contains the refresh token and `Authorization: Bearer` headers; the token and log files take default permissions. The source confirms the DEBUG file handler [primary, `common.py`]. Eileen must treat that whole folder as a credential path and never read it, never pass `--logtostderr`, and redact Google token shapes from anything it records.
- **Partial consent.** Issue #116 (open): approving only some scopes on Google's granular consent screen breaks the login and forces a browser login every hour on `--auth=oauth2`. The owner should tick all scopes when he signs in.
- **Broad scope.** `cloud-platform` is a wide Google Cloud scope; issue #71 asks for MCP-style per-notebook consent instead. On an account with no Cloud projects the practical exposure is small, but it is worth stating to the owner [inferred].
- **Free-tier GPUs.** Issue #73 (2026-06-30, free account): `colab new --gpu T4` returned HTTP 412 (a usage-limit or capacity refusal) while a CPU runtime allocated at once. The bundled skill warns "Accelerator availability is tier-gated; most accounts can only get CPU." Whether the owner's account gets a GPU through the CLI is the first thing the spike checks.
- **Liveness.** The CLI dropped its keep-alive pings on 2026-09-25: "VM liveness is automatically maintained by the Colab backend based on active kernel activity" [primary, changelog]. A Colab team member added on 2026-10-01 that "Colab mostly depends on checking for live connections to the VM", so a long job should keep its connection open (issue #160). A Pro+ user reported sessions ending after about 1.5 hours (issue #115, unanswered).
- **Execution timeout.** `colab exec` has an inactivity `--timeout` (default 30 s, raised from 10 s on 2026-06-04); a long silent step needs either regular output or a larger value, and a long timeout can spin a local CPU core (issue #82, open) [primary]. Training scripts should print a progress line at least every 10 s.

**How Eileen would run it on Windows.** `wsl.exe --exec <absolute path to colab> --auth=oauth2 <subcommand> ...` from a supervised process, with arguments passed as an argument vector (no shell), Windows paths translated to `/mnt/c/...`, and stdout and stderr streamed back [inferred]. The owner already runs a WSL virtual machine (BUILD.md section 4.4 measured it at 4.1 GB private on 2026-10-20), so the extra cost is the CLI process, not a new VM.

### 2.2 The Colab MCP server (`colab-mcp`)

**What it is.** Announced on the Google Developers Blog on 2026-03-17 as a way to "Connect Any AI Agent to Google Colab"; repository Apache-2.0, 1,305 stars, version 1.0.1 in `pyproject.toml`, run with `uvx git+https://github.com/googlecolab/colab-mcp`; it names Gemini CLI and Claude's CLI as clients [primary].

**How it actually works** (read from `session.py` and `websocket_server.py` on 2026-10-20) [primary]:

1. The local server opens a WebSocket on `localhost` at a random port with a random 16-byte token, subprotocol `mcp`, and accepts connections only from the origins `https://colab.research.google.com` and `https://colab.google.com`, one client at a time.
2. Its only tool at start is `open_colab_browser_connection`, which opens the user's default browser at `https://colab.research.google.com/notebooks/empty.ipynb#mcpProxyToken=<token>&mcpProxyPort=<port>` and waits up to 60 s.
3. The Colab page, signed in as the user, connects back to the local socket and acts as an MCP server itself. The local server proxies the page's tools to the agent and sends `notifications/tools/list_changed`.
4. The page's tools, per community reports, are `add_code_cell`, `add_text_cell`, `execute_cell` and `update_cell` (issue #83). An existing notebook can be connected too: Colab's command palette has "Connect to a local Colab MCP server", which takes the token (issue #75).

So the browser does the Google side and the local process holds only a random loopback token. No Google credential ever reaches the agent [primary, source].

**Problems.** Clients that ignore `tools/list_changed` never see the notebook tools; reports name the Claude desktop app on Windows 11, Codex, Cursor and some versions of Claude's CLI (issues #50, #71, #83). Community fixes that pre-register the tools were closed by the maintainers, who want clients to implement the spec (pull requests #60 and #66) [primary]. There are no tools to choose the runtime type, restart a runtime, or move files (issue #74). The notebook must already be connected to a runtime (issue #75). The last commit on `main` is 2026-04-01; dependency pull requests have gone unmerged since June; issue #108 (2026-09-16) asks "is this project dead?" [primary].

**Why it still matters to Eileen.** The protocol is plain MCP over a WebSocket, and the security model is a loopback port, an origin check and a random token. Eileen can implement the local side in Rust (rmcp over a custom WebSocket transport), list the page's tools itself after the connection, and so sidestep the dynamic-tools bug and the Python dependency entirely [inferred]. The route works through the notebook UI the user sees, which is the use Colab says it prioritises. Its weakness is that it depends on an undocumented contract between Google's open-source server and Colab's front end, which Google could change.

### 2.3 The Colab extension for VS Code

Released 2025-11-13 on the Visual Studio Marketplace and Open VSX ([Google Developers Blog](https://developers.googleblog.com/google-colab-is-coming-to-vs-code/)); version 0.9.7 on 2026-10-01; depends on Microsoft's Jupyter extension [primary]. The user opens any `.ipynb`, picks Colab as the kernel, signs in through a browser OAuth page, and chooses Auto Connect or a new server of a given machine type; the status bar shows the compute-unit burn rate; experimental features add a terminal, file upload and mounting the server's `/content` as a workspace folder ([user guide](https://github.com/googlecolab/colab-vscode/wiki/User-Guide), edited 2026-04-21) [primary].

**Can it be scripted?** Not headlessly. Its contributed commands (remove server, mount Drive, upload, open terminal and similar) need a running VS Code window, and it exposes no MCP server or language-model tools; issue #433 ("Expose Colab runtimes to Chat", open since 2026-02-21) asks for exactly that [primary]. The one agent path is Claude's own VS Code integration: its built-in IDE server offers `mcp__ide__executeCode`, which "Runs Python code in the active Jupyter notebook's kernel", but every call inserts a cell and shows a VS Code prompt to Execute or Cancel; "`mcp__ide__executeCode` can't run anything silently" ([docs](https://code.claude.com/docs/en/vs-code)) [primary]. With a Colab kernel selected, that is a supervised, cell-by-cell way for a Claude worker to use Colab, good for working sessions with the owner at the keyboard and wrong for a training job that should run while he does something else.

The extension is also evidence of where Colab is going: it migrated to a new public API (pull request #681, "Post public API migration rollout") with generated clients for runtimes, runtime specs, subscriptions and an `ExecuteCodeResult` type [primary].

### 2.4 The Colab API (`colaboratory.googleapis.com`)

"The Colab API lets you programmatically manage Colab runtimes." It "is currently in beta and available on an allowlist basis"; access is by a request form, then `gcloud services enable colaboratory.googleapis.com` on a Google Cloud project; methods in v1beta are `runtimes.create`, `delete`, `get`, `list`, `runtimespecs.list` and `getSubscription`, plus long-running operations in v1 (page last updated 2026-09-08) [primary]. No code-execution method is published. It is not usable by the owner today, needs a Cloud project, and its discovery documents take an API key. The frontier scout (OWNER-DIRECTION.md, "Stay at the frontier") should watch it: if it goes public with execution and per-user OAuth, it becomes the cleanest route and needs no WSL.

### 2.5 Browser automation of the Colab page

Eileen already has a browser route through the user's logged-in browser: UI Automation reads the page's accessibility tree on demand, with screenshots and clicks last (BUILD.md section 2.6). Colab's editor cells are code editors inside a heavy web app; typing code into them and reading outputs back through the accessibility tree is possible but brittle, slow and fights the user for the screen [inferred]. The MCP bridge of section 2.2 is the structured version of the same idea, offered by Colab's own front end, so clicking should be kept for the few things neither the CLI nor the bridge can do (for example "Runtime, Change runtime type" when the bridge is the active route), with the user's screen lease as usual.

On terms: the Google Terms of Service bar "using automated means to access content from any of our services in violation of the machine-readable instructions on our web pages (for example, robots.txt files...)" (effective 2026-07-30) [primary]. `colab.research.google.com/robots.txt` answered HTTP 403 to scripted requests on 2026-10-20, so its contents could not be checked [primary]. A user-directed session in the user's own signed-in tab, with the user present, is not crawling; but the official clients carry far less doubt, which is another reason to prefer them [inferred].

### 2.6 Notebooks in Google Drive or GitHub

Colab notebooks are stored in Google Drive or loaded from GitHub and saved as `.ipynb` [primary, FAQ]. That is where a notebook lives, not how Eileen controls a run. Two uses fall out [inferred]:

- With Google Drive for desktop installed, Eileen can write the generated notebook and the exported model into the synced folder, so the owner finds his project in Drive and Eileen picks up files the notebook saved to Drive without any Drive API or credential.
- A notebook pushed to one of the owner's GitHub repositories opens at `https://colab.research.google.com/github/<owner>/<repo>/blob/<branch>/<path>.ipynb`; a private repository needs the owner to authorise Colab's GitHub access in Colab himself.

Mounting Drive inside a runtime grants "any code in your notebook" access to "any files in your Google Drive", and Colab normally asks for that grant on every new runtime [primary, FAQ]; the CLI's `drivemount` is interactive and its own skill says agents must not run it. So Drive mounting stays a step the owner does himself when he wants it.

### 2.7 Local runtime and custom runtimes

A local runtime keeps Colab's browser UI but runs code on the user's machine, either from Google's Docker image (`us-docker.pkg.dev/colab-images/public/runtime`, "for demo purposes", linux/amd64, GPU only with NVIDIA drivers and the container toolkit) or from a local Jupyter server that trusts Colab's origin ([local runtimes page](https://research.google.com/colaboratory/local-runtimes.html)) [primary]. The owner's PC has an Intel integrated GPU and 16 GB of RAM that is mostly in use, so a local runtime adds nothing for training [inferred]. The same page shows connecting to a Google Compute Engine VM through SSH port forwarding; the FAQ points to "a dedicated VM at GCP Marketplace" to "fully relax any runtime limits and idle timeouts" [primary]. Both are paid Google Cloud resources.

### 2.8 Colab Enterprise and Vertex AI

Colab Enterprise is "a collaborative, managed notebook environment with the security and compliance capabilities of Google Cloud", part of Google's agent platform (formerly Vertex AI), with IAM access control, runtime templates (machine type, accelerators, disk), idle shutdown that can be changed or turned off, and scheduled notebook executions with results in Cloud Storage ([introduction](https://docs.cloud.google.com/colab/docs/introduction), updated 2026-10-13) [primary]. Its quickstart starts with "Verify that billing is enabled for your Google Cloud project" and enabling three APIs [primary]. Why billing: it runs on Compute Engine resources priced pay as you go; a Google Cloud free trial account cannot add GPUs to VMs and gets no GPU quota by default ([free program](https://docs.cloud.google.com/free/docs/free-cloud-features)) [primary]. It is the most API-friendly Google option and the wrong one for the owner: it needs a billing account, project setup and IAM, and spends money outside his Colab plan. Eileen should support it only if the owner later has a Cloud project.

### 2.9 Alternatives with official APIs

If Colab misbehaves, other services have official, scriptable training compute. What each needs from the owner:

| Service | Official tool | How the user signs in | Free allowance | Runs unattended | Fit with the owner's rules |
|---|---|---|---|---|---|
| Kaggle notebooks | `kaggle` CLI 2.2.4 (2026-07-23): `kernels push` runs a notebook or script as a batch job, `kernels output` fetches results; accelerators listed "as of Sep 2026" include T4 x2, L4, A100, H100, RTX Pro 6000 and TPU v5e-8 and v6e-8 [primary] | `kaggle auth login`, a browser OAuth flow, or a token from Kaggle settings [primary] | About 30 GPU hours a week, varying with demand [secondary] | Yes, by design | Good: browser sign-in in Kaggle's own CLI, no key handled by Eileen |
| Hugging Face Jobs | `hf jobs run`, `hf jobs uv run`, Python `run_job`; logs, metrics, `wait`, scheduled jobs, volumes [primary] | `hf auth login` with a user access token [primary] | None; "available to any user or organization with a positive credit balance", PRO credits count; per-minute billing, T4 $0.40, L4 $0.80, A10G $1.00, A100 $2.50 an hour; default timeout 30 minutes [primary] | Yes | Acceptable only if the owner pastes his own token into `hf` himself; it is a key in all but name |
| Modal | `modal` CLI and SDK | `modal setup` opens the browser and saves a token to `~/.modal.toml` [secondary] | About $30 of credits a month on the free plan [secondary] | Yes | Acceptable (browser sign-in), costs beyond credits |
| Lightning AI | `lightning_sdk` 2026.10.13 with `Job.run(machine=...)` [primary] | `lightning login` or `LIGHTNING_API_KEY` [primary] | About 15 credits a month [secondary] | Yes | Acceptable with `lightning login` |
| RunPod | API, CLI, SDKs | API key from the console only, "authenticate requests to the Runpod API, CLI, and SDKs" [primary] | None, per-second billing [primary] | Yes | Poor: an API key is the only route |

Kaggle stands out: it is free, its official CLI has a browser sign-in, and its batch execution is meant to run without anyone watching, which matches "start training and get on with my day" better than free Colab does [inferred]. The design below puts every compute service behind one training-backend interface so Kaggle can be added as the second backend without touching the job.

## 3. Google's terms and limits, precisely

### 3.1 The documents

| Document | Date | What it settles |
|---|---|---|
| [Google Terms of Service](https://policies.google.com/terms) | Effective 2026-07-30 | General conduct: no "using automated means to access content ... in violation of the machine-readable instructions on our web pages", no "bypassing our systems or protective measures", no "using AI-generated content from our services to develop machine learning models or related AI technology" [primary] |
| [Colab Additional Terms of Service](https://research.google.com/colaboratory/tos_v6.html) (v6) | Last modified 2026-08-13 | No account sharing; no access "other than as authorized by Google"; no reselling access; incorporates the FAQ's restricted activities and resource limits by reference; paid services terms [primary] |
| [Colab FAQ](https://research.google.com/colaboratory/faq.html) | Undated; read 2026-10-20 | Restricted activities, timeouts, lifetimes, tiers, Google AI plans [primary] |

The Colab terms say nothing directly about bots, scripts or software acting for the user; the FAQ's restrictions and the official agent tools fill that gap [primary].

### 3.2 Restricted activities (FAQ, quoted)

"The following are disallowed from all managed Colab runtimes": file hosting, media serving, or other web service offerings not related to interactive compute with Colab; downloading torrents or engaging in peer-to-peer file-sharing; connecting to remote proxies; mining cryptocurrency; running denial-of-service attacks; password cracking; using multiple accounts to work around access or resource usage restrictions; creating deepfakes; employing techniques such as containerization to circumvent anti-abuse policies [primary].

On top of those, "Colab prioritizes users who are actively programming in a notebook. The following are disallowed from managed Colab runtimes running free of charge, without a positive Colab compute unit balance, and may be terminated at any time without warning": remote control such as SSH shells, remote desktops; bypassing the notebook UI to interact primarily via a web UI; chess training; running distributed computing workers. "You can remove these types of restrictions by purchasing one of our paid plans here and maintaining a positive compute unit balance" [primary].

The FAQ explains the web UI rule: free-tier users "commonly experience runtime terminations when attempting to bypass the notebook UI and using a web UI on a Colab managed runtime for content generation" [primary]. It is aimed at hosting a generation web app on the runtime, not at automation of the notebook itself [inferred].

### 3.3 Timeouts and lifetimes per tier

| Tier | Maximum lifetime | Idle behaviour | Background execution | Source |
|---|---|---|---|---|
| Free | "at most 12 hours, depending on availability and your usage patterns" | "Runtimes will time out if you are idle"; idle periods are unpublished and vary | No | FAQ [primary] |
| Pro, Pay As You Go | "In general, notebooks can run for at most 12 hours"; "backend termination if you exhaust your available compute units" | Same | No | FAQ [primary] |
| Pro+ | "continuous code execution for up to 24 hours if you have sufficient compute units" | "Idle timeouts only apply if code execution terminates" | Yes (the 2024 Workspace announcement: "background execution for our longest-running sessions") | FAQ [primary]; [Workspace blog](https://workspaceupdates.googleblog.com/2024/06/google-workspace-colab-pro-and-colab-pro-plus.html) [primary] |
| Google AI Ultra | Not stated | Not stated | "uninterrupted background execution and Premium GPU access" | [Developers Blog, 2026-09-22](https://developers.googleblog.com/colab-is-now-part-of-your-google-ai-plan/) [primary] |
| Dedicated VM (GCP Marketplace) | "fully relax any runtime limits and idle timeouts" | None | n/a | FAQ [primary] |

"Colab does not publish these limits, in part because they can vary over time" [primary]. Eileen must therefore never promise a run length; it checkpoints and expects to be cut off.

### 3.4 Compute units, plans and Google AI plans

- Paid tiers "offer you increased compute availability based on your compute unit balance"; when the balance is exhausted, paid users "revert to the free-of-charge tier" [primary, FAQ].
- Standalone prices: Colab Pro $9.99 a month for 100 compute units; Pro+ $49.99 a month (500 units in Google's 2024 Workspace announcement; some 2026 sources say 600); Pay As You Go $9.99 per 100 units; units reported to expire after 90 days [secondary; the sign-up page did not render for a scripted fetch].
- Since 2026-09-22, Google AI plans include Colab benefits: "priority access to faster accelerators and more powerful machines" for subscribers, and for Ultra "uninterrupted background execution and Premium GPU access"; Google AI Pro members get 200 units ([Google One help](https://support.google.com/googleone/answer/14534406?hl=en)) [primary] and Google AI Plus members 50 ([Google One help for Plus](https://support.google.com/googleone/answer/16882689?hl=en), seen only in a search summary) [secondary]; the benefits are not given during free trials, and a Colab subscription and a Google AI plan stack into one balance [primary, FAQ]. Buying a Google AI plan for Colab does not change the owner's rule that Gemini is not a route in Eileen.
- Measured burn rates (units per hour, March 2026): T4 1.19, L4 1.71, A100 40 GB 5.40, A100 80 GB 7.52, G4 (RTX PRO 6000, about 96 GB) 8.71; H100 unmeasured ([McCormick](https://mccormickml.com/2024/04/23/colab-gpus-features-and-pricing/)) [secondary]. Eileen should read the live rate from `colab usage` rather than use a table.

### 3.5 GPU availability

GPU and TPU types "vary over time" and are not guaranteed; premium GPUs need a paid plan "subject to availability"; Colab asks users to switch a runtime to no accelerator when the GPU is not being used [primary, FAQ]. The CLI's accelerator flags cover T4, L4, G4, H100 and A100 GPUs and v5e1 and v6e1 TPUs [primary]. A free account was refused a T4 through the CLI with HTTP 412 while a CPU runtime allocated (issue #73) [primary].

### 3.6 What would make an approach a violation, applied to Eileen

| Approach | Verdict | Why |
|---|---|---|
| Driving Colab through the official CLI, MCP server or VS Code extension, user signed in himself | Permitted | Google built and documents these for agents; the CLI says so in its README [primary] |
| Sending fake activity (pings, synthetic keystrokes, a "keep alive" cell) to defeat idle timeouts | Do not | "Runtimes will time out if you are idle" is a stated policy; defeating it is "bypassing our systems or protective measures" in the Google Terms [primary, inferred]; the CLI itself removed its keep-alive daemon |
| Keeping a real connection open while a real training cell runs, with the user aware | Permitted | That is the activity Colab measures; a Colab team member recommends keeping the connection alive for long processes (issue #160) [primary] |
| Rotating several Google accounts to get more GPU time | Do not | Explicitly disallowed [primary] |
| `colab ssh` or `colab console` on the free tier | Avoid | "remote control such as SSH shells" may be terminated on free runtimes [primary]; use `colab exec`, which talks to the kernel |
| Serving a Gradio or other web demo of the trained model from the runtime | Do not | "web service offerings not related to interactive compute" and, on the free tier, "bypassing the notebook UI to interact primarily via a web UI" [primary]; run demos locally |
| Using Colab as a pool of workers for Eileen's other jobs | Do not | "running distributed computing workers" on the free tier, and not interactive compute [primary] |
| Using Gemini in Colab, or the Data Science Agent, to produce labels for training | Do not | Google Terms bar "using AI-generated content from our services to develop machine learning models" [primary]; the owner excludes Gemini anyway |
| Clicking through the Colab page in the user's own browser while he is present | Grey but defensible | User-directed use of the notebook UI; prefer the MCP bridge, which is Google's structured version of it [inferred] |
| Leaving a runtime allocated after the job | Do not | Burns units and capacity; the CLI's skill says always stop; FAQ asks users to close tabs when done [primary] |

### 3.7 Other terms that touch an ML project

- **Anthropic's consumer terms** (effective 2025-10-08) bar using the services "To develop any products or services that compete with our Services, including to develop or train any artificial intelligence or machine learning algorithms or models" [primary]. Our reading is that the training clause is an example of competing development, so a personal clothing-colour classifier is not covered, but the wording is broad [inferred]. To stay clearly inside it, Eileen uses Claude for research, planning, code and spot-check assistance, and produces bulk training labels with local open-licence models (section 5.2, labelling). The owner may want to confirm this reading.
- **Dataset and model licences** (section 4.5) bind the trained model, whatever the compute.
- **People in photos** are personal data; section 5.6 covers consent, minimisation and what leaves the PC.

## 4. Prior art for the owner's example: a person's clothing colour

### 4.1 How the problem breaks down

"Find a person's clothing colour" is three smaller problems [inferred]:

1. **Where is the person** (skip when the photo has one person): a person detector.
2. **Which pixels are which garment**: human parsing or clothes segmentation, giving masks for upper clothes, trousers, skirt, dress, shoes and so on.
3. **What colour name those pixels have**: robust colour measurement and naming, which is not really a learning problem once the pixels are known, plus handling of lighting, shadows, patterns and multi-coloured garments.

A vision-language model can answer the whole question in one step ("what colour is her top?") at a higher cost and with known weaknesses on colour (section 4.4).

### 4.2 Garment segmentation models

| Model | Licence | Size | What it segments | Reported accuracy | ONNX | Fit for local CPU [inferred] |
|---|---|---|---|---|---|---|
| [mattmdjaga/segformer_b2_clothes](https://huggingface.co/mattmdjaga/segformer_b2_clothes) (SegFormer-B2 fine-tuned on ATR) | Card says `other`; the SegFormer code and weights carry NVIDIA's licence, use "non-commercially ... for research or evaluation purposes only" ([NVlabs LICENSE](https://github.com/NVlabs/SegFormer/blob/master/LICENSE)); ATR has no licence | About 27 M parameters | 18 classes incl. Upper-clothes, Skirt, Pants, Dress, Belt, shoes, Bag, Scarf | IoU: upper clothes 0.78, pants 0.84, skirt 0.65, dress 0.55, scarf 0.29; mean IoU 0.69, mean accuracy 0.80 (card) | Tagged; 428 k downloads | Good; fine for a personal trial, not shippable |
| [SCHP](https://github.com/GoGoDuck912/Self-Correction-Human-Parsing) (Self-Correction for Human Parsing) | Code MIT; weights trained on LIP, ATR and Pascal-Person-Part, whose terms are unclear | ResNet-101 class | LIP 20 classes, ATR 18 classes | ATR test mIoU 82.29, LIP val 59.36 [secondary, README mirrors] | Community exports | Moderate |
| [Sapiens2 segmentation](https://github.com/facebookresearch/sapiens2) (Meta, released 2026-04-24) | Sapiens2 License (Meta's own: use, modify and redistribute with its terms and use restrictions) | 0.4 B to 5 B | 29 classes incl. Upper_Clothing, Lower_Clothing, Apparel | Not checked | No official export | Too heavy for this PC's CPU at its 1K resolution |
| [SAM 3](https://huggingface.co/facebook/sam3) (Meta, text-prompted "concept" segmentation) | SAM License (2025-11-19), gated download | Large (not checked) | Anything named in text ("red shirt") | Not checked for clothing | Community split exports [secondary] | Heavy; better as a labelling tool on Colab than at run time |
| [Grounding DINO tiny](https://huggingface.co/IDEA-Research/grounding-dino-tiny) plus [SAM 2.1 tiny](https://huggingface.co/facebook/sam2.1-hiera-tiny) | Apache-2.0, Apache-2.0 | Small | Text-prompted boxes, then masks | Not checked for clothing | Yes (community) | Moderate; a permissive labelling and fallback pair |
| [Florence-2 base](https://huggingface.co/microsoft/Florence-2-base) | MIT | 0.23 B | Captions, grounding, referring-expression segmentation | Not checked for clothing | [onnx-community export](https://huggingface.co/onnx-community/Florence-2-base-ft) | Moderate |
| [RF-DETR-Seg](https://github.com/roboflow/rf-detr) | Apache-2.0 for the open models (Roboflow README) [secondary for the seg checkpoints] | Small to medium | Instance segmentation, trainable on custom classes | COCO only out of the box | Official exporter | Good; a permissive base to fine-tune on clothing masks |

The most downloaded off-the-shelf clothes segmenter is good on the classes that matter for colour (upper clothes and trousers) and weak on small or ambiguous ones (belts, scarves, dresses) [primary, card]. Its licence chain makes it a personal-use model only.

### 4.3 Colour naming without a model

Once the garment pixels are known, colour is measured, not learned [inferred]:

- Drop pixels that are not cloth (skin and hair from the parser's own classes), very dark shadows and specular highlights.
- Correct the white balance roughly (a gray-world or white-patch estimate over the whole image), since lighting shifts colour names more than any model error.
- Cluster the remaining pixels in CIELAB (k-means with k of 2 or 3) and take the dominant cluster, reporting a second colour when it covers a large share (stripes, prints).
- Map the cluster centre to a name. Two open vocabularies exist: the 11 basic colour terms with names learned from real-world images ([van de Weijer et al., "Learning Color Names for Real-World Applications", 2009](https://lear.inrialpes.fr/people/vandeweijer/color_names.html), which found names learned from real images beat names learned from colour chips) [primary], and the xkcd colour survey's 954 named colours from about 220,000 participants, released CC0 [secondary]. The user's vocabulary ("navy" versus "blue") becomes a setting the learning loop refines from corrections.
- Report a confidence from the distance between the two nearest names, so "navy or black" can be said honestly.

This kind of pipeline is not new: a 2023 paper uses Mask R-CNN masks plus k-means for pedestrian dress colour ([Information 14(2):125](https://doi.org/10.3390/info14020125)), and a garment-attributes classifier on Hugging Face says colour is "measured deterministically in the pipeline, not classified" [secondary].

### 4.4 Zero-shot vision-language models, and how accurate they are on colour

- **CLIP-style models.** [SigLIP 2](https://huggingface.co/google/siglip2-base-patch16-224) (Apache-2.0) can score a garment crop against prompts such as "a photo of a navy shirt". For a fixed vocabulary the text embeddings are computed once, so only the image tower runs on the PC [inferred]. On pedestrian attribute recognition (all attributes, colours among them), CLIP-based methods reach about 75 to 81 percent mean accuracy on the zero-shot splits PETA-ZS and RAP-ZS, where test identities are unseen: PromptPAR's baseline 75.13 and method 80.08 on PETA-ZS ([arXiv 2312.10692](https://arxiv.org/abs/2312.10692)), and UAPAR (2026 preprint) 80.77 and 81.09 ([arXiv 2604.26873](https://arxiv.org/abs/2604.26873)) [primary]. Those methods are trained on the other identities; they are not prompt-only results. Apple's MobileCLIP2 carries Apple's research licence and is not suitable [primary, licence tag].
- **General VLMs.** [ColorBench](https://arxiv.org/abs/2504.10514) (NeurIPS 2025 Datasets and Benchmarks) asked models for "the color of a specific object" (Color Recognition) and for colour codes (Color Extraction): Gemini 2 Flash 80.3 and 31.3 percent, GPT-4o 73.7 and 29.2, Qwen2.5-VL-7B 76.3 and 49.0, Qwen2.5-VL-3B 72.4 and 38.5, and the tiny LLaVA-OV-0.5B 26.3 and 44.8 [primary]. The authors conclude colour understanding "has been largely neglected by existing VLMs". Small local VLMs with permissive licences exist ([Qwen3-VL-2B](https://huggingface.co/Qwen/Qwen3-VL-2B-Instruct) and [Moondream 2](https://huggingface.co/vikhyatk/moondream2), both Apache-2.0) but at this size colour answers are a tie-breaker, not the primary signal [inferred].
- **Label noise is real.** The UPAR dataset annotators gave 11 colours (plus other and mixture) to upper and lower clothing for 100,000 images and note that labels for the same person change with lighting, shadows and the visible area ([arXiv 2209.02522](https://arxiv.org/abs/2209.02522)) [secondary, search summary of the paper]. No method should be expected to agree with a human on every borderline navy, black or dark grey.

### 4.5 Datasets and their licences

| Dataset | What it gives | Licence | Usable for |
|---|---|---|---|
| [Open Images V7](https://storage.googleapis.com/openimages/web/index.html) | Segmentation masks for 350 classes, including Clothing, Shirt, Jeans, Trousers, Dress, Skirt, Miniskirt, Shorts, Suit, Swimwear, Hat, Sun hat, Scarf, Tie, Sock, Boot and Glove (checked against the class files on 2026-10-20) [primary] | Annotations CC BY 4.0; images CC BY 2.0 from Flickr, to be verified per image [secondary] | Training a permissively licensed garment segmenter; no colour labels |
| [Fashionpedia](https://huggingface.co/datasets/detection-datasets/fashionpedia) | 27 apparel categories and 19 parts with masks, 294 fine-grained attributes | CC BY 4.0 per the dataset card [secondary] | Garment segmentation; it has no colour attributes (the ECCV 2020 paper's text has no mention of colour; checked by text search on 2026-10-20) [primary] |
| [ModaNet](https://github.com/eBay/modanet) | 55,176 street photos with polygons, 13 categories | CC BY-NC 4.0 [primary] | Personal or research only |
| [ATR](https://github.com/lemondan/HumanParsing-Dataset) | 18-class human parsing | No licence file; README asks for citation "for acadamic and commercial research" [primary] | Unclear; treat as research only |
| LIP, CIHP | 20-class human parsing, multi-person | Not found [secondary] | Treat as research only until checked |
| DeepFashion, DeepFashion2 | Large fashion sets | Non-commercial research terms [secondary] | Personal or research only |
| PETA, RAP v2, UPAR | Pedestrian attributes with upper and lower clothing colours | Research terms [secondary] | Evaluation of colour naming; personal use |
| Colorful Fashion Parsing (CFPD) | 2,682 images with pixel-level colour and category labels (13 colours, 23 categories) | Not found [secondary] | Evaluation, if the terms allow |
| The owner's own photos | The real target distribution | His own, with the consent of the people in them | Evaluation first, training if needed |

Papers with Code, the usual index for "what exists", was shut down by Meta in July 2025 and redirected to Hugging Face's Trending Papers [secondary]. The research phase should use the Hugging Face Hub API (models and datasets with licence tags), GitHub, arXiv and Hugging Face's paper pages instead.

### 4.6 What Eileen should conclude for this example

A credible plan, stated as the research phase should state it [inferred]:

1. **No training first.** Segment with a clothes parser, measure colour as in section 4.3, double-check the name with SigLIP 2 on the crop, and say "unsure" when they disagree.
2. **Measure on the owner's photos.** Label 50 to 100 of his photos through the HUD (upper and lower colour each; about ten minutes of his time) and report accuracy per colour, with the confusions.
3. **Decide.** If the pipeline meets his target (say 90 percent on his photos for 11 colours), ship it as a skill with no training. If it misses mostly on naming (navy against black), train a tiny colour head on frozen SigLIP 2 image embeddings of his labelled crops, on his CPU, in minutes. If it misses mostly on segmentation (wrong pixels), fine-tune a permissive segmenter (RF-DETR-Seg) on Open Images and Fashionpedia clothing masks plus his own corrected masks, on Colab. A rough size for that last case: a few hours on a T4 or L4, a handful of compute units at the measured rates, to be replaced by the live `colab usage` rate.
4. **Licence.** If he chooses the SegFormer clothes model for its accuracy, the result is marked personal use only and never offered for sharing.

## 5. Design: the v0.2 "ML project" job

### 5.1 Shape

An ML project is a long-lived parent job (`JobKind::MlProject`) with phases, gates and child jobs. It reuses what v0.1 builds: Research and DeepAnswer jobs for the fan-out, Coding jobs for the notebook and export code, the AppTask pattern (BUILD.md section 2.5, the Resolve render) for a run that executes outside Eileen and reports progress, the HUD for spot checks and charts, approvals for spending and uploading, and the learning loop for turning the result into a skill [inferred].

```
intake -> research (parallel) -> evidence (baseline on the user's data) -> GO / NO-GO gate (user)
      -> data + licences -> labelling (AI pre-labels, user spot checks) -> notebook + local smoke test
      -> training run (Colab connector; progress, steering) -> evaluation -> export to ONNX -> install as a skill
```

Each phase writes its outputs to the project folder (`%LOCALAPPDATA%\Eileen\projects\<slug>\` or a folder the user names), so the project survives restarts and the user can read everything: `brief.md`, `evidence/` (one card per source), `decision.md`, `data/manifest.jsonl`, `LICENCES.md`, `labels/`, `notebook/train.ipynb` and `train.py`, `runs/<run-id>/` (metrics, logs, checkpoints), `eval/`, `export/` (ONNX, model card) [inferred].

### 5.2 Phase by phase

**Intake (brain, seconds).** The brain turns the request into a brief: task type (classification, segmentation, detection, regression), input (photos, screen captures, video frames, a camera), output (for example `{upper: colour, lower: colour, confidence}`), target accuracy and speed, the sources the user named, the intended use (personal, or shareable), and the hardware budget. It asks at most one clarifying question ("Just photos, or live from a camera?"), per the owner's "one short clarifying question" rule.

**Research (parallel).** Four questions run in parallel, as separate Research jobs when memory admission allows, otherwise as parallel web searches inside one deep worker (BUILD.md section 2.3, "Research fans out"): prior-art models and their licences; datasets and their licences; no-training and zero-shot options; how others built it and what compute it took. Each finding is an `EvidenceCard`: claim, metric, on what data, source URL, date read, licence, tag (primary, secondary, inferred). A deep worker merges them into a ranked options table like section 4. The HUD shows the table; Eileen says two sentences ("Three models already do this; the best open one gets about 78 percent overlap on tops. I'd test it on your photos before training anything.").

**Evidence and the go or no-go.** Before any training, Eileen runs the top one or two no-training candidates on a sample of the user's own data and shows the result: accuracy against a small evaluation set the user labels on the HUD, plus a grid of the worst cases. Small models run locally through Eileen's ONNX runtime when they fit the memory budget; larger ones run on a Colab CPU or GPU runtime through the connector. The gate is the user's decision, with Eileen's recommendation and its numbers: "Your photos: 86 percent with no training. A small colour head would likely add 5 to 8 points and takes ten minutes on your PC. Want that?" A no-go that ships the baseline as a skill is a success, and the log says so.

**Data and licences.** Collection uses only sources the user named or approved in the research table: public dataset downloads (preferably straight onto the Colab runtime, which is faster and spares the owner's upload), the user's own folders, and pages whose terms and robots.txt allow it (never scraping sites that forbid it). Every source gets a licence record (SPDX identifier or the terms text, the URL and the date read) in `LICENCES.md`; a source with no licence is marked "unknown, personal use only". Cleaning removes duplicates by perceptual hash, corrupt files and off-task images. Splits are fixed before any training and grouped by person and by source, so the same person never appears in both training and test (the PETA-ZS lesson: unseen identities give the honest number).

**Labelling.** Pre-labels come from local open-licence models (the segmenter's masks, the measured colour, SigLIP 2's vote), each with a confidence. The user spot-checks on the HUD: a grid of 16 to 25 crops with proposed labels, lowest confidence first plus a random audit sample; corrections by voice ("third one is navy") or click. Eileen estimates the label error rate from the audit sample and stops asking when it falls below a threshold the user can change (default 3 percent, a starting value). Corrections also teach the colour vocabulary. For heavy labelling work the user can open the set in Label Studio (Apache-2.0) or CVAT (MIT), which Eileen can prepare but does not require. Claude is used to explain disagreements and suggest rules, not to mass-produce training labels (section 3.7). Faces are blurred before upload when the task does not need them, using the YuNet detector Eileen already ships.

**Notebook and smoke test.** A Coding worker writes `train.py` (and a matching `train.ipynb` for the owner to open in Colab or VS Code) from Eileen's template for the chosen task type, with: pinned dependencies; a fixed seed; a progress line at least every 10 seconds in the format of section 5.4; a checkpoint at the end of every epoch; resume from the latest checkpoint; a control file it reads between steps; final evaluation, ONNX export and a parity check in the last cells. Before any GPU time it runs the script on a tiny subset (ten images, one epoch) on a CPU runtime, and the job continues only if that run produces a checkpoint, a metrics line and a loadable ONNX file. This is "evidence first" applied to Eileen's own code.

**Training.** Through the Colab connector (section 5.3): allocate the runtime the plan chose, upload the prepared data, install requirements, run `train.py` with its output streamed, narrate and show progress, honour steering, download checkpoints periodically and artifacts at the end, and always stop the runtime. On a disconnect or a runtime cut-off, Eileen says so, re-attaches if the runtime is still assigned (catching up from the metrics file on the runtime), otherwise offers to resume from the last downloaded checkpoint on a new runtime.

**Evaluation.** On the held-out test set and, separately, on the user's own evaluation set: per-class precision and recall, a confusion matrix, calibration (what confidence means "unsure"), speed on the owner's CPU after export, and a gallery of failures on the HUD. Always against the no-training baseline on the same images. The summary is honest and short: "92 percent on your photos against 86 without training. It still mixes up navy and black in dim light; here are five."

**Export and install.** ONNX export with the opset pinned to what Eileen's shipped ONNX Runtime supports, optional int8 quantization, and a parity check twice: in the notebook with `onnxruntime` pinned to the version Eileen ships (1.28.2 per BUILD.md; the current upstream release is 1.31.0 from 2026-10-09 [primary]), and again on the PC through Eileen's own runtime on the same 20 images, within a tolerance. Then a model card (task, labels, preprocessing, metrics, data sources, licence chain, intended use, limits), a SHA256, and registration in a local user-model registry. The learning loop creates the skill ("what colour is that person wearing?") that calls the model through a generic run-model tool in a sidecar, and records the whole project as a reusable recipe, so the next "train me a classifier from my photos" starts from it.

### 5.3 The Colab connector

A v0.2 connector (`ml.colab`) with a `ConnectorManifest` like any other (BUILD.md section 2.14), running as a supervised sidecar [inferred]:

| Op | Calls | Tier | Notes |
|---|---|---|---|
| `doctor` | `wsl.exe -l`, `colab version`, `colab sessions` | 0 | Reports installed, signed in, versions; never reads the CLI's folder |
| `usage` | `colab usage` | 0 | Balance and burn rate for budgets |
| `allocate` | `colab new -s eileen-<project>-<run> [--gpu X]` | 2 | Spends compute units; approval shows the runtime type, the live rate and the cap |
| `upload` | `colab upload` | 2 when the data includes personal photos, else 1 | Approval names what leaves the PC and where it goes |
| `install` | `colab install -r requirements.txt` | 1 | |
| `run` | `colab exec -s ... -f train.py` (streamed) | 1 within an approved run | Output lines are `Untrusted`; only the progress prefix is parsed |
| `download` | `colab download` | 1 | Into the project folder only |
| `stop` | `colab stop -s ...` | 0 | Idempotent; also run by the janitor |
| `attach_view` | `colab url -s ... --open` | 1 | Opens the same runtime in the owner's browser to watch or take over |

Rules [inferred]:

- **Fixed argument vectors only.** The connector builds every command itself from typed op arguments; a worker can never pass a free-form command line to `wsl.exe`. No `ssh`, `console`, `repl`, `auth`, `drivemount` or `--logtostderr`, ever.
- **Credentials are off limits.** Paths under `\\wsl.localhost\<distro>\home\<user>\.config\colab-cli\` and `\\wsl$\...\.config\colab-cli\` are classified as credential paths (tier 3 for every tool and worker), as are the VS Code extension's secret storage and any `colab-mcp` log folder. Captured output is passed through a redactor for Google OAuth token shapes before it reaches logs, the flight recorder or trajectories.
- **Isolation.** The connector passes `--config` with a per-project session file so parallel projects and the owner's own CLI use never collide (the CLI's skill recommends this for agents).
- **Janitor.** Every 10 minutes while a project is active, and on Eileen's start, the connector runs `colab sessions` and stops any `eileen-*` runtime that no active run owns, after saying so. A forgotten runtime is the costliest failure here.
- **Keep-awake.** While a run is live on a tier without background execution, the job holds a keep-awake lease (the owner allowed keep-awake for overnight work, owner answer 10), because Colab ends runtimes whose connection drops. On Pro+ or Ultra the lease is optional.

The fallback route (the MCP bridge) implements the same `TrainingBackend` trait with fewer ops: no `allocate` or `stop` (the user chooses the runtime in the tab), no `upload` (data is fetched from the runtime itself, or the owner mounts Drive), `run` as `add_code_cell` plus `execute_cell`, and `download` through files the notebook writes to Drive, which Drive for desktop syncs. Kaggle (`kernels push`, `kernels status`, `kernels output`) would be a third implementation.

### 5.4 Progress, narration, the HUD and steering

The training template prints one line per update:

```
EILEEN_PROGRESS {"run":"r3","epoch":12,"epochs":30,"step":5400,"steps":13500,"loss":0.214,"val_acc":0.912,"lr":0.0003,"eta_s":1820,"gpu_mem_gb":9.8}
```

The connector parses only lines with that prefix through a fuzzed parser with size limits, and turns them into `JobProgressed` with structured metrics (a percent from step over steps, plus the named numbers). Everything else on stdout is kept as untrusted log text. The narration table (BUILD.md, announcements per event and job kind) gets rows for this job: by default one spoken line per epoch at most every two minutes ("Epoch 12 of 30, validation accuracy 91 percent"), immediate lines for a new best, a stall (no improvement for N epochs), divergence (loss jumps or NaN), a runtime cut-off and completion; the HUD shows the loss and accuracy curves, the ETA, the runtime type and units used. "How's the training?" answers from the latest numbers. "Stop announcing epochs" changes the row, as for renders.

Steering uses a small control file the script reads between steps (`/content/eileen_control.json`, uploaded by the connector): `{"action":"stop"}` (checkpoint and finish cleanly), `{"action":"pause_eval"}` (evaluate now), `{"lr": 0.0001}`, `{"epochs": 40}`. "Lower the learning rate", "train ten more epochs" and "stop it there" become control-file writes; "try again with the bigger model" becomes a new run from the last checkpoint with an edited config, recorded as a new run so runs can be compared. Interrupting the kernel is the last resort (`colab restart-kernel` loses in-memory state), which is why checkpoints come every epoch [inferred].

### 5.5 Costs, budgets and approvals

- Before allocation: the live burn rate from `colab usage` times the estimated hours from the smoke test's step time, shown as units and as a share of the balance, with a cap (default: the estimate plus 50 percent, a starting value). Allocation is tier 2 with that number in the approval.
- During the run: a warning at 80 percent of the cap, a clean stop at 100 percent unless the user extends it by voice.
- Research and labelling help use the Claude plan; the plan-limit awareness idea (OWNER-DIRECTION.md, proposed) applies, and the brief shows both budgets.

### 5.6 Privacy and licences

- **What leaves the PC.** Photos of people are personal data. Uploads to the owner's Colab runtime are tier 2 and say how many images, of what, and that they go to Google's servers under the owner's account. Faces are blurred and EXIF location stripped by default when the task does not need them. Nothing goes to any other service without its own approval.
- **Consent.** The owner's own photos of other people need their consent for this use; Eileen reminds once per project when the data contains other people's faces, the same spirit as the voice-cloning consent rule.
- **Cameras.** If the model is later used on a live camera, that camera is a room-sensing connector: opt-in per device, processed locally, only derived events kept (OWNER-DIRECTION.md, "Connected fully to the computer").
- **Licence chain.** The model card lists the licence of every base model and dataset; the most restrictive term wins. A model with any non-commercial or unknown link is marked "personal use only" and the learning loop's sharing step refuses it.
- **Colab's AI features.** When generative AI features are used in Colab, "Google collects prompts, related code, generated output" [primary, FAQ]. Eileen's notebooks do not use them.

### 5.7 Learning from every project

Following the learning-loop report ([2026-10-learning-loop.md](2026-10-learning-loop.md)): the trajectory of each phase is recorded; a successful project becomes a skill for using the model and a recipe for building the next one (the brief template, which research queries found good sources, the notebook template and hyperparameters that worked, the step time per runtime type, the label vocabulary the user prefers); a failed one leaves a scenario behind. The second "make me a model that ..." project should skip straight to the questions that differ.

### 5.8 Failure modes and what Eileen does

| Failure | Detection | Response |
|---|---|---|
| GPU refused (HTTP 412 or 503) | `colab new` exit and message | Say so; offer a CPU smoke test now, a retry later, a smaller GPU, or the browser route |
| Runtime cut off (idle, lifetime, units) | Stream ends; `colab sessions` no longer lists it | Say so; resume from the last downloaded checkpoint on approval |
| Sign-in expired | 401 or 403 from the CLI | Tell the owner to run one command in a WSL terminal; never attempt it |
| CLI update breaks something | `doctor` after updates; nightly check against the latest CLI | Pin the last good version; open an issue |
| Loss diverges | NaN or a jump in the progress lines | Stop by control file; suggest a lower learning rate; ask |
| Export parity fails | Tolerance check | Do not install; report the largest differences; try another opset or skip quantization |
| A runtime left running | Janitor | Stop it, then say so |

## 6. Recommendation

### 6.1 Primary route

**The official Colab CLI, version-pinned, run inside WSL2 by Eileen's `ml.colab` connector, with training as `colab exec` of a generated `train.py` (and the same code as a notebook for the owner).**

Why this one:

1. It is Google's own client, published for "headless automation, and AI agent integrations", with an agent skill in the package. Of all routes, it is the one Google has said in writing is for this [primary].
2. It covers the whole job: choosing the runtime type, files both ways, streamed execution, compute-unit balance, stopping. No other Colab route does all of that [primary].
3. Credentials stay in Google's tool. The owner signs in once in Google's own page; Eileen calls the tool and never reads its folder, which is how Eileen already treats the Claude CLI.
4. It runs from Rust as a plain supervised process; nothing in Eileen becomes Python (decision D38).

### 6.2 Fallbacks, in order

1. **The Colab MCP bridge, implemented in Rust on Eileen's side**, for when WSL is unavailable, the CLI breaks after an update, or the free tier refuses the CLI a GPU that the browser gets. The owner's signed-in Colab tab connects to Eileen's loopback socket; Eileen adds and runs cells the owner can watch. No Google credential involved. Weaker on control (no runtime type, no file transfer) and depends on an undocumented front-end contract, so it is the fallback, not the primary.
2. **VS Code with the Colab extension**, for sessions where the owner wants to work alongside: Eileen writes the notebook, the owner picks the Colab kernel, and a Claude worker runs cells through the IDE integration with the owner approving each cell.
3. **Hand-off.** Eileen writes the notebook into the owner's Drive for desktop folder or a GitHub repository, opens it in Colab in his browser, and he runs it; Eileen watches the progress lines the notebook writes to Drive.
4. **Kaggle**, as a second compute backend behind the same interface, for unattended runs on a free allowance, if the owner wants one.

Not recommended: driving the Colab page by clicks as a main route (brittle; section 2.5), Colab Enterprise or Vertex AI (billing account and Cloud setup; section 2.8), RunPod (API key only; section 2.9). The Colab API is one to watch, not to use (section 2.4).

### 6.3 What the owner does once

1. **Choose a Colab tier.** The free tier is enough to try the job with CPU smoke tests and, when capacity allows, a T4, but GPUs through the CLI may be refused and runs stop when the connection drops. For dependable GPUs: Colab Pro ($9.99 a month, 100 units) or a Google AI plan with Colab units (Pro: 200 units). For runs that keep going with the PC asleep: Pro+ or Google AI Ultra, which add background execution. Eileen recommends Pro or Google AI Pro to start, and Pro+ only if overnight runs become common.
2. **Install the CLI in WSL.** In his existing WSL distribution: `uv tool install google-colab-cli==<the version Eileen pins>` (the CLI needs Python 3.12 or later; uv supplies it).
3. **Sign in, once, himself.** In a WSL terminal: `colab --auth=oauth2 sessions`, open the printed Google link, sign in, tick every requested permission (partial consent breaks refresh; issue #116), paste the code back. Eileen never runs this step.
4. **Allow keep-awake for training runs** (already allowed for overnight work).
5. **Optional, for the fallbacks:** Google Drive for desktop (so notebooks and models sync without any API), VS Code with the Colab extension, a Kaggle account with `kaggle auth login`.

Then `eileen doctor colab` checks it: WSL present, CLI version, signed in (through `colab sessions`, not by reading files), balance, and, with the owner's approval, one 60-second CPU allocation that is stopped at once.

### 6.4 What v0.1 must keep possible

These are cheap now and expensive to retrofit [inferred, mapped to BUILD.md section 3.3 contracts]:

1. **Job kinds are extensible.** `JobKind` is a closed enum in the contracts (`Coding, ComputerUse, General, Research, DeepAnswer, Librarian, Replay, ContinueSession, AppTask, Act`). Mark it `#[non_exhaustive]`, make stored jobs and events tolerate an unknown kind when an older build reads them, and reserve `MlProject`.
2. **Parent and child jobs.** A job can start child jobs and wait on them; events carry `parent: Option<JobId>`; the HUD and "how's it going?" can summarise a parent. The E5 "launch three agents" scenario already needs most of this.
3. **External long-running tasks generalise beyond Resolve.** The AppTask pattern (an outside process with a handle, polled or streamed progress, survives an Eileen restart and re-attaches) should not be Resolve-specific: a connector-defined handle and a persisted re-attach step.
4. **Progress carries metrics.** `JobProgressed` should hold an optional percent plus named numeric metrics and an ETA, not only a percent, and the announcement table should accept per-metric rules ("only tell me about a new best").
5. **Connectors can wrap external CLIs, including through WSL.** The connector registry and supervisor should allow a sidecar that runs a vendor CLI with fixed argument vectors, typed ops and per-op tiers, without a code change to the core.
6. **Credential path classes are data.** The path classifier's credential list should be configurable and should already include `\\wsl.localhost\*\home\*\.config\colab-cli\**` and `\\wsl$\*\...` equivalents, VS Code secret storage and the Google Cloud SDK's credential folder, and the log redactor should know Google OAuth token shapes.
7. **Approvals can carry a cost.** The approval record should have optional fields for spend (units, money) and for data egress (what, how much, to where), so v0.2 adds no new approval type.
8. **A generic run-model tool and a user-model registry.** The ONNX Runtime host (a sidecar, since `eileen.exe` links no ONNX Runtime) should load any registered model by manifest (input shape, preprocessing, labels, postprocessing, licence chain, SHA256), not only the bundled ones; `models.toml` stays for bundled models and a `user-models.toml` (or table) holds trained ones.
9. **Skills can reference a model artifact** by hash, so a learned skill can call "clothing-colour@1".
10. **HUD panel types that are not hard-coded to one feature:** an image grid with per-item corrections (spot checks, failure galleries) and a small line chart (loss curves, render progress).
11. **Recorded external tools are replayable.** The scenario and replay formats should record a connector's process outputs so a Colab session can be replayed in simulation without Google, as the scenario corpus does for the desktop.

### 6.5 Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The Colab CLI changes or breaks (experimental, no roadmap, 12 releases in 5 months, internal endpoints) | High | Training route down | Pin a version; nightly check against the latest; the MCP bridge as fallback; Kaggle as an alternative backend |
| Google never ships Windows support | Medium | WSL stays required | WSL is already on the owner's PC; watch issues #85 and #100; the scout watches the Colab API |
| Free-tier GPUs refused through the CLI | High on free | No GPU training on free | Recommend a paid tier; browser bridge for free-tier GPUs; local CPU for small heads |
| Runs cut short (unpublished limits, idle rules, a Pro+ user reported 1.5 hours) | Medium | Lost time and units | Checkpoint every epoch, download periodically, resume; never promise run length |
| Terms tighten on agent use of Colab | Low to medium | Route closes | Use only official clients; no limit-evasion; keep the use interactive and attended; the job degrades to hand-off |
| Token leakage through the CLI's own log (issue #177) | Medium | Account exposure | Treat the folder as a credential path; never read or upload it; redact; tell the owner; watch for the fix |
| The MCP bridge contract changes or is abandoned | Medium | Fallback down | Version-check the page's tools at connect; fall back to hand-off |
| Licence contamination (non-commercial data in a shared model) | Medium | Legal and trust | Licence chain in the model card; sharing refuses any non-commercial or unknown link |
| Reading of Anthropic's terms on training models is wrong | Low | Policy | Labels from local open models; Claude for research and code; owner confirms the reading |
| Memory on the owner's PC (WSL plus Chrome plus workers) | Medium | Thrashing during projects | The CLI is light; training happens remotely; memory admission already queues workers |
| Accuracy on the owner's photos lower than published numbers | High | Disappointment | Measure on his photos before any training; report per-colour confusions; say "unsure" |

### 6.6 The first spike for v0.2

With the owner present and signed in himself, in this order, each step leaving evidence in the spike's notes:

1. `colab --auth=oauth2 sessions` from WSL works after the owner's sign-in; `colab usage` prints the balance and rate.
2. `colab new -s spike --gpu T4`, record success or the refusal, then `colab stop`. Repeat with a CPU runtime.
3. From Rust: `wsl.exe --exec` streaming a 20-minute dummy training script that prints a progress line every 5 seconds; measure end-to-end latency of a line, kill the Rust side for two minutes, check whether the runtime survives and whether re-attaching works.
4. Control-file steering round trip (`colab upload` of the file, the script reacts within one step).
5. `colab download` of a 100 MB checkpoint; time it.
6. ONNX parity: export a small model in the runtime with `onnxruntime==1.28.2`, download, load in Eileen's runtime, compare outputs.
7. MCP bridge: a 200-line Rust loopback server with the origin and token checks, opened against the owner's signed-in Colab tab; list the page's tools; add and execute a cell.
8. For the clothing-colour example: run SegFormer clothes and SigLIP 2 on 50 of the owner's photos locally; record per-photo CPU time and memory, and the accuracy against his labels.

## Sources

Google, Colab (all read 2026-10-20):

- [Colab FAQ](https://research.google.com/colaboratory/faq.html) (undated page)
- [Colab Additional Terms of Service v6](https://research.google.com/colaboratory/tos_v6.html), last modified 2026-08-13
- [Google Terms of Service](https://policies.google.com/terms), effective 2026-07-30
- [Colab local runtimes](https://research.google.com/colaboratory/local-runtimes.html)
- [Colab API reference](https://developers.google.com/colab/api/reference/rest), updated 2026-09-08
- [Colab CLI repository](https://github.com/googlecolab/google-colab-cli): README, CHANGELOG (0.7.1, 2026-09-14), `skills/colab-operator/SKILL.md`, `docs/04_automation_and_utility.md`, `docs/01_session_management.md`, `docs/06_ssh_access.md`, `src/colab_cli/auth.py`, `cli.py`, `common.py`; issues #71, #73, #82, #96, #100, #115, #116, #140, #148, #160, #177; discussion #77; [PyPI](https://pypi.org/project/google-colab-cli/) 0.7.5, 2026-10-20
- [Colab MCP server repository](https://github.com/googlecolab/colab-mcp): README, `src/colab_mcp/session.py`, `websocket_server.py`, `pyproject.toml`; issues #50, #71, #74, #75, #83, #108; pull requests #60, #66; [announcement, 2026-03-17](https://developers.googleblog.com/en/announcing-the-colab-mcp-server-connect-any-ai-agent-to-google-colab/)
- [Colab VS Code extension repository](https://github.com/googlecolab/colab-vscode): README, `package.json` 0.9.7, [user guide](https://github.com/googlecolab/colab-vscode/wiki/User-Guide) (2026-04-21), issues #433, #681, #692; [launch post, 2025-11-13](https://developers.googleblog.com/google-colab-is-coming-to-vs-code/)
- [Colab is now part of your Google AI plan](https://developers.googleblog.com/colab-is-now-part-of-your-google-ai-plan/), 2026-09-22; [Google AI Pro benefits](https://support.google.com/googleone/answer/14534406?hl=en); [Workspace Colab Pro and Pro+](https://workspaceupdates.googleblog.com/2024/06/google-workspace-colab-pro-and-colab-pro-plus.html), 2024-06
- [Colab Enterprise introduction](https://docs.cloud.google.com/colab/docs/introduction) (2026-10-13), [quickstart](https://docs.cloud.google.com/vertex-ai/docs/colab/create-console-quickstart), [release notes](https://docs.cloud.google.com/colab/docs/release-notes), [Google Cloud free program](https://docs.cloud.google.com/free/docs/free-cloud-features)

Other compute (read 2026-10-20): [Kaggle CLI docs](https://github.com/Kaggle/kaggle-cli/blob/main/docs/README.md) and [kernels](https://github.com/Kaggle/kaggle-cli/blob/main/docs/kernels.md); [Hugging Face Jobs pricing](https://huggingface.co/docs/hub/jobs-pricing) and [guide](https://huggingface.co/docs/huggingface_hub/guides/jobs); [Modal token CLI](https://modal.com/docs/reference/cli/token); [lightning-sdk on PyPI](https://pypi.org/project/lightning-sdk/) 2026.10.13; [RunPod API keys](https://docs.runpod.io/get-started/api-keys).

Claude: [VS Code integration and the built-in IDE server](https://code.claude.com/docs/en/vs-code); [Anthropic consumer terms](https://www.anthropic.com/legal/consumer-terms), effective 2025-10-08.

Models and licences (Hugging Face API and repositories, 2026-10-20): [segformer_b2_clothes](https://huggingface.co/mattmdjaga/segformer_b2_clothes), [NVlabs SegFormer licence](https://github.com/NVlabs/SegFormer/blob/master/LICENSE), [SCHP](https://github.com/GoGoDuck912/Self-Correction-Human-Parsing), [Sapiens2](https://github.com/facebookresearch/sapiens2) and its [segmentation docs](https://github.com/facebookresearch/sapiens2/blob/main/docs/SEG.md), [SAM 3 licence](https://github.com/facebookresearch/sam3/blob/main/LICENSE), [Florence-2 base](https://huggingface.co/microsoft/Florence-2-base), [SigLIP 2](https://huggingface.co/google/siglip2-base-patch16-224), [Grounding DINO tiny](https://huggingface.co/IDEA-Research/grounding-dino-tiny), [SAM 2.1 tiny](https://huggingface.co/facebook/sam2.1-hiera-tiny), [Qwen3-VL-2B](https://huggingface.co/Qwen/Qwen3-VL-2B-Instruct), [Moondream 2](https://huggingface.co/vikhyatk/moondream2), [RF-DETR](https://github.com/roboflow/rf-detr), [ONNX Runtime releases](https://github.com/microsoft/onnxruntime/releases), [Label Studio](https://github.com/HumanSignal/label-studio), [CVAT](https://github.com/cvat-ai/cvat).

Datasets: [Open Images V7](https://storage.googleapis.com/openimages/web/index.html) (class files checked 2026-10-20), [Fashionpedia paper, arXiv 2004.12276](https://arxiv.org/abs/2004.12276) and [dataset card](https://huggingface.co/datasets/detection-datasets/fashionpedia), [ModaNet](https://github.com/eBay/modanet), [ATR](https://github.com/lemondan/HumanParsing-Dataset), [UPAR, arXiv 2209.02522](https://arxiv.org/abs/2209.02522), [Colorful Fashion Parsing (Liu et al., IEEE TMM 2014)](https://research.buaa.edu.cn/en/publications/fashion-parsing-with-weak-color-category-labels/), [van de Weijer colour names](https://lear.inrialpes.fr/people/vandeweijer/color_names.html), [xkcd colour survey](https://blog.xkcd.com/2010/05/03/color-survey-results/).

Papers: [ColorBench, arXiv 2504.10514](https://arxiv.org/abs/2504.10514); [PromptPAR, arXiv 2312.10692](https://arxiv.org/abs/2312.10692); [UAPAR, arXiv 2604.26873](https://arxiv.org/abs/2604.26873); [Rethinking of Pedestrian Attribute Recognition, arXiv 2107.03576](https://arxiv.org/abs/2107.03576); [Pedestrian dress recognition with Mask R-CNN and k-means, Information 14(2):125](https://doi.org/10.3390/info14020125).

Secondary: [McCormick, Colab GPU features and pricing](https://mccormickml.com/2024/04/23/colab-gpus-features-and-pricing/) (burn rates, March 2026 update); [Android Authority on Google AI plans and Colab](https://www.androidauthority.com/google-ai-plans-premium-colab-benefits-3715703/); [hyper.ai on the Papers with Code shutdown](https://hyper.ai/en/news/42900); [xkcd colour data licence credits](https://chuck.stanford.edu/chai/data/xkcd/license-credits.txt); [resoa/garment-attributes model card](https://huggingface.co/resoa/garment-attributes); [InfoQ on the Colab MCP server](https://infoq.com/news/2026/04/colab-mcp-server).
