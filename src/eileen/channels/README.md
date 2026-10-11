# channels

Reach the user when they are not at the PC, and accept redirection from there. The same `JobEvent` that the voice loop would announce goes to the phone when presence says the user is away.

## Builds on

| Need | Choice |
|---|---|
| Two-way text and voice notes | Telegram Bot API, long polling (works behind home NAT) |
| Push with action buttons | ntfy (Apache-2.0, self-hostable) |
| Private access to the PC | Tailscale |
| Live two-way voice | Discord bot |
| Off-PC work | Claude Code cloud sessions and Codex cloud before any self-hosted infrastructure |

## First task

A Telegram bot and an ntfy publisher, both subscribed to the event bus, plus presence detection (a presence file or no voice activity for N minutes) that decides whether an announcement is spoken or sent.

## Must never

Never open a public webhook by default. Never send transcripts, file contents or screenshots to a channel without the permission tier saying so. Never accept a routing override from a channel without confirming the sender is the owner.

Background: [docs/RESEARCH.md](../../../docs/RESEARCH.md), section 5.
