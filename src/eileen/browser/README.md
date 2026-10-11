# browser

Grounding and control inside the user's real, logged-in browser tabs. DOM-level state comes from here; UI Automation is reserved for browser chrome.

## Builds on

| Piece | Choice |
|---|---|
| Primary | Playwright MCP with its browser extension (Chrome and Edge, tab picker, reuses your sessions) |
| Alternative | Chrome DevTools MCP, Chrome only |
| Runtime | Node 24, launched through `npx` |

## Constraints to design around

- Since Chrome 136, raw CDP against the default profile is refused unless the user opts in.
- Chrome enables web accessibility trees only after it sees an assistive-technology client, so DOM grounding must come through the extension path.
- Selection is not a first-class field anywhere. "Copy exactly that paragraph" is a small `getSelection` snippet plus a copy keystroke.

## First task

Start Playwright MCP, attach through the extension, and expose three things to the core: the tab list, an accessibility snapshot with refs, and the current selection as text.

Must never: launch a separate browser profile as the default path, or store cookies, tokens or session data.

Background: [docs/RESEARCH.md](../../../docs/RESEARCH.md), section 2.3.
