# voice

The always-on voice loop. It owns the microphone, runs outside every agent, and keeps talking while jobs run. It reads a jobs block and a DesktopState block from its context so "how's that simulation doing?" resolves without a tool call.

## Builds on

| Stage | Choice |
|---|---|
| Framework | Pipecat if it runs natively on Windows (unverified), otherwise a custom asyncio loop on RealtimeSTT and RealtimeTTS |
| Echo cancellation | pywebrtc-audio, with played PCM as the far-end reference |
| Voice activity | Silero VAD |
| End of turn | Smart Turn v3 |
| Speech to text | sherpa-onnx (Nemotron streaming for partials, Parakeet for finals) or Moonshine v2 |
| Text to speech | Kokoro |
| Conversational model | Claude Haiku 4.5 to start; a local 4B to 8B model as the free option |

## First task

Test Pipecat on native Windows. If it fails, build the asyncio loop. Either way, barge-in stops output within about 150 ms. The announcement policy ships as data. Approval needed or blocked interrupts; completed waits for idle; progress stays silent unless asked; away routes to the phone (see `channels/`).

Must never: run inside an agent as a tool, or wait on a job before answering the user.

Background: [docs/RESEARCH.md](../../../docs/RESEARCH.md), section 4.
