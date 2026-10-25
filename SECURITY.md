# Security

If you find a vulnerability, report it privately. Use GitHub's private vulnerability reporting on
this repository, or send a direct message to the owner (Sixteen1-6 on GitHub). Please do not open a
public issue for security problems. You will get a reply, and a fix or a plan, before anything is
disclosed.

## What counts as a vulnerability

- **Credentials.** Eileen never stores, reads or proxies vendor credentials. It spawns the
  unmodified official CLI of each route (the claude CLI in v0.1) in your environment, and you sign
  in to it yourself. Any code that reads a vendor credential file, offers a vendor login flow, or
  sends a token anywhere is a bug: report it the same way.
- **Untrusted input steering an action.** Everything on screen is untrusted: window titles, page
  contents, selected text, file names, transcripts, tool results and anything a model or a worker
  reads back are data, not instructions. An action above the reversible tier needs an approval
  from the person at the keyboard (a physical click, an approval hotkey on a real keyboard, or the
  spoken action word in the approval window). A path where such text can approve an action, raise
  a permission tier, start a job from an ambient turn or become a learned skill without the user is
  a vulnerability.
- **Containment.** Workers run in a kill-on-close Job Object, contained by their folder, a path
  guard and a shell policy. A way for a worker to write outside its folder without asking, reach
  Eileen's own data folder, or run a child that outlives the job is a vulnerability.
- **Privacy.** What Eileen learns stays on your PC, and nothing leaves it without a preview and
  your approval. A path where personal data reaches a log line, an incident, a shared scenario or a
  fleet result unredacted, or where speech not addressed to Eileen is written to disk, is a
  vulnerability.

## Supply chain

Dependencies pass cargo-deny's license, advisory, ban and source policy on every pull request
(`deny.toml`); a new crate version waits 14 days unless an advisory's patched release needs it
sooner. Native libraries and models are pinned by size and SHA256 and verified after download.
