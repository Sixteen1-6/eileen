# Security

If you find a vulnerability, report it privately. Use GitHub's private vulnerability reporting on this repository, or send a direct message to the owner (Sixteen1-6 on GitHub). Please do not open a public issue for security problems. You will get a reply, and a fix or a plan, before anything is disclosed.

Eileen never stores, reads or proxies vendor credentials. It spawns the unmodified official harness binaries (Claude Agent, Codex, Gemini CLI, OpenCode) in your environment, and you sign in to those yourself. Any code here that reads `~/.claude`, `~/.codex`, `auth.json` or similar, or that offers a vendor login flow, is a bug. Report it the same way you would report a vulnerability.

Everything on screen is untrusted input. Window titles, page contents, selected text, file names and anything an agent reads back are data, not instructions. Consequential actions (sending, deleting, spending money) always require confirmation from the person at the keyboard, whatever the screen says. If you find a path where on-screen text can steer Eileen into such an action without that confirmation, treat it as a vulnerability.
