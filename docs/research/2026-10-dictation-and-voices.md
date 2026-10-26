# Dictation mode and configurable voices, October 2026

This report answers two owner directions of 2026-10-20 (OWNER-DIRECTION.md): **dictation better than the paid tools** (speak, and clean punctuated text lands in whatever field has focus, in any app, local and free) and **any voice** (Kokoro stays the fast default; Chatterbox, or something better that runs well on this PC, lets the user pick or clone a voice, with consent and watermarking).

It builds on, and does not repeat, four sibling reports written the same day: the always-on voice pipeline (`2026-10-voice-stack.md`), desktop control (`2026-10-computer-use.md`), brain latency on the owner's plan (`2026-10-brain-latency.md`) and Claude extension points (`2026-10-claude-mods.md`).

**How claims are tagged.** [primary] means the project's own repository, model card, documentation or source code, read on 2026-10-20. [secondary] means a third-party article; several are written by competitors and say so. [ran 10-20] means measured for this report on the owner's PC. [voice-stack], [computer-use] and [brain-latency] mean measured by those sibling runs. [inferred] means reasoning, not evidence.

**Conditions on the PC.** Core Ultra 7 155H, 16 GB RAM, Arc iGPU, NPU, no NVIDIA GPU. Other research agents ran throughout. Free physical memory read 1.43, 0.54, 2.05, 2.48, 1.33, 1.69 and 1.05 GB at checks between 15:04 and 15:48, and CPU load read 88, 100, 67, 9 and 34 percent. The rule for this run was at least 6 GB free before any heavy measurement, so **no model was loaded for this report**: Parakeet timing and the cloning TTS test were not run, and published or sibling numbers are cited instead (section 2).

## 1. Recommendations in brief

**Dictation**

1. **Build dictation inside Eileen; do not embed another app.** The best open-source Wispr Flow alternative on Windows today is **Handy** (`cjpais/Handy`, MIT, Rust and Tauri, 34,208 stars, v0.9.8 on 2026-10-03) [primary]. **OpenWhispr** (MIT, Electron, 9,642 stars) is the most feature-complete second [primary]. Neither knows the focused field, its app or what is on screen, and neither runs on a subscription brain. Eileen already has all three, so it should reuse their code and designs, not wrap them.
2. **Reuse, with attribution:** Handy's Windows clipboard transaction (delayed rendering with a read receipt, guarded restore, clipboard-history opt-out), its custom-word matcher and filler tables; OpenWhispr's terminal detection and its read-only "did the user correct what we pasted" monitor as a design; SpeakoFlow's `dictation-cleanup-rules` (MIT) with its 79-case conformance suite. AGPL and GPL projects (Whispering in Epicenter, VoiceTypr, VoiceInk, FluidVoice, WhisperWriter) are design references only.
3. **Speech model:** dictation uses the streaming recognizer that is already loaded for the always-on loop, so it costs no extra memory and shows words in the HUD while the user speaks. Run one measurement to decide whether **Parakeet Unified EN 0.6B** (NVIDIA, April 2026, one model for streaming and offline, sherpa-onnx int8 exports at 240, 560 and 1,120 ms) should replace Nemotron streaming as that model: NVIDIA reports 6.52 percent average WER at 0.56 s against Nemotron's 7.07 percent [primary]. Parakeet TDT 0.6B v3 stays the multilingual dictation model, loaded on demand.
4. **Insertion ladder:** native Win32 edit controls get `EM_REPLACESEL` (targeted, no focus change, undoable, 5.8 ms for a sentence [ran 10-20]); everything else gets Handy's clipboard transaction plus Ctrl+V (Ctrl+Shift+V in terminals), after checking that the target is still the foreground window; paced Unicode `SendInput` is the last resort with a read-back check. `ValuePattern.SetValue` is never used to insert, because it replaces the whole field.
5. **Cleanup in two tiers:** local rules always (fillers, stutters, "scratch that", typed-slot self-corrections such as "at 3, no, 4 pm", context formatting), measured at about a millisecond; a **Claude "scribe" session** on the owner's plan only when the user turns polish on, for retractions the rules will not touch, tone and app-specific formatting. A warm Claude turn takes 2.3 to 3.1 s [brain-latency], so the scribe works sentence by sentence while the user is still talking, and an edit-distance guard rejects any output that changes meaning.
6. **Vocabulary that learns, in three layers:** decoder biasing through sherpa-onnx hotwords (supported for NeMo transducers, Parakeet TDT included, under `modified_beam_search`) [primary, source]; a Soundex plus Levenshtein replacement pass as in Handy; and the list passed to the scribe. New words come from explicit requests, from watching the user's corrections through read-only UI Automation, and, per utterance only, from names visible in the focused window.

**Voices**

7. **Kokoro stays the instant default** (voice-stack choice: fp32, sherpa-onnx, `af_heart`, 54 stock voices).
8. **For "any voice we want", use Kyutai Pocket TTS as the cloning engine, not Chatterbox.** Pocket TTS is 100M parameters, clones from a plain WAV, streams, is MIT code with CC-BY-4.0 weights, and already runs from Eileen's own `sherpa-onnx` Rust crate with a `reference_audio` field [primary]. Kyutai also shipped an official Rust CLI with a Windows binary and 18M, 7M and 2M English models on 2026-10-20 [primary]. A CPU benchmark run by Picovoice, which sells a competing engine, put it at 0.37 CPU core-hours per hour of audio and 610 MB peak on one machine, against Chatterbox-Turbo's 13.4 core-hours and 7.5 GB [secondary].
9. **Chatterbox stays in the plan as an optional expressive engine** for pre-rendered speech (narration, cached stock phrases, read-aloud of long text), starting with **Chatterbox-Nano** (110M, MIT, about 3x real time on 8 CPU threads per Resemble [primary]), with its Perth watermark always on.
10. **Live or not:** on this PC, under today's load, even Kokoro ran at a real-time factor of 1.1 to 1.3 and Pocket TTS at 1.28 [voice-stack], so a cloned voice can be live only when the PC has headroom. A per-reply watchdog falls back to Kokoro for the next reply when it does not.
11. **Configuration:** a voices folder (`%LOCALAPPDATA%\eileen\voices\<name>\voice.toml` plus reference clip and consent record), a `[voice]` section in Eileen's config naming the live, narration and fallback voices, and spoken commands in the deterministic grammar ("use my voice", "use the British voice", "go back to the default voice").
12. **Consent and watermarking:** a cloned voice is made only from a live enrollment in which the speaker reads a consent sentence and a passage, checked to be the same speaker with the sherpa-onnx speaker-embedding model already downloaded; built-in watermarks (Chatterbox Perth) cannot be turned off; cloned audio that is saved or leaves the PC gets a watermark that Eileen applies (AudioSeal or Perth, both MIT). Whether live speaker playback of a cloned voice is also watermarked is an owner decision (section 12.4).

## 2. What was run, and what was not

| Item | Result | Tag |
|---|---|---|
| Insertion timing on a RichEdit 5.0 window this run created in its own process (the RichEdit family Windows 11 Notepad builds on), three runs | Targeted methods measured; foreground methods refused by the safety guard; clipboard methods skipped (section 5.2) | [ran 10-20] |
| Local cleanup rules prototype, 30 hand-written cases | 30 of 30, about 1.4 ms per case in Python including regex compilation (section 6.1) | [ran 10-20] |
| Source inspection: Handy (paste transaction, settings, text utilities, model catalog), OpenWhispr (Windows paste helper, UIA text monitor, tone prompts), sherpa-onnx (NeMo hotwords, Rust Pocket TTS example), Pocket TTS v4 release, Chatterbox issues and PRs | Section 3, 4, 10 | [primary] |
| GitHub API and Hugging Face API metadata (licenses, stars, releases, gating, file sizes) for 19 dictation repos and 30 TTS models | Tables 3 and 10 | [primary] |
| Parakeet timing on a WAV | **Not run**: about 750 MB to load and the rule required 6 GB free; cited from [voice-stack] | |
| One cloning TTS test (Pocket TTS v4 or Chatterbox-Nano) | **Not run** for the same reason; cited from [voice-stack] and published numbers | |
| Insertion in real Notepad | **Not run here.** Windows 11 Notepad restores unsaved tabs between sessions, so test text would persist in the owner's Notepad; the computer-use run's 2026-10-11 Notepad measurements are cited instead | [computer-use] |

Scripts and result files: `%USERPROFILE%\AppData\Local\Temp\claude\C--Users-USER-Projects--claude\8b2a7b12-9d74-4cce-a7ac-9d5f3ba7299c\scratchpad\dictation-voices\` (`scripts\target.py`, `scripts\insert_bench.py`, `scripts\cleanup_rules.py`, `results\*.json`). The venv there holds only `comtypes` 1.4.17. All processes started were closed; a final check found none left.

## 3. Dictation: the field on 2026-10-20

### 3.1 Open-source tools

| Project | License | Windows | Stack | Speech models | Insertion on Windows | Cleanup | Vocabulary | Activity |
|---|---|---|---|---|---|---|---|---|
| **Handy** `cjpais/Handy` | MIT | x64 and ARM64 | Rust, Tauri; `transcribe-cpp` 0.4.0 (MIT, ggml, Vulkan) | 70+ in a benchmarked catalog: Parakeet v2, v3, Unified, Nemotron, Canary, Cohere, Whisper, Moonshine, Qwen3-ASR, Granite | Default Ctrl+V; also direct typing (enigo), Shift+Insert, Ctrl+Shift+V, external script. A Windows "reliable paste" publishes the transcript as a delayed-render clipboard format and restores the old clipboard only after the target has read it | Filler and stutter removal by language; optional LLM post-processing through OpenAI, Anthropic, Groq, Cerebras, OpenRouter, Z.AI, Bedrock or a custom OpenAI-style URL (Ollama default) | Custom words matched by Soundex and Levenshtein after transcription | 34,208 stars, v0.9.8 2026-10-03, pushed 2026-10-20 |
| **OpenWhispr** | MIT (open core: sync, agent and SSO in paid tiers) | yes | Electron 41, whisper.cpp, sherpa-onnx, llama.cpp | Whisper, Parakeet, Cohere Transcribe, "Orukeet"; optional cloud | A small C helper sends Ctrl+V with `SendInput`, or Ctrl+Shift+V for terminals detected by window class and exe; captures the target window at record start and re-activates it before pasting | Prompted cleanup with tones (formal, casual, very casual, excited) and app categories (personal chat, work chat, email, other); local or cloud model | Dictionary plus **auto-learn**: a UIA helper watches the focused field after the paste and reports changes | 9,642 stars, v1.10.3 2026-10-12 |
| **SpeakoFlow** | MIT | x64 | Rust, Tauri (Handy-style; not a GitHub fork) | 69-model catalog, Parakeet default | Types at the cursor | Rules first, then "SpeakoFlow Mini", a 795 MB cleanup model trained on rule-cleaned text (not yet published on Hugging Face) | Dictionary; "learn words I correct" on Windows | 344 stars, created 2026-06-17 |
| **Amical** | MIT | x64 and ARM64 | Electron, whisper.cpp, Ollama | Whisper | Not documented | Context-aware formatting by active app via local or cloud LLM | Custom vocabulary | 1,562 stars |
| **VoiceTypr** | AGPL-3.0, paid binaries | x64 | Tauri, Rust | Whisper (Parakeet on Mac only) | Rust insertion pipeline (not inspected) | Cloud LLM formatting (OpenAI, Anthropic, Gemini, custom) | Not documented | 738 stars |
| **Whispering** (in `EpicenterHQ/epicenter`) | AGPL-3.0-or-later for every package | yes | Svelte, Tauri | Local and cloud | Not inspected | LLM transformations | `vocab` app in the monorepo | Whispering listed as "development milestone" |
| **VoiceInk** | GPL-3.0 | macOS only | Swift | whisper.cpp, Parakeet via FluidAudio | n/a | AI modes, screen context | Personal dictionary | 6,867 stars |
| **FluidVoice** | GPL-3.0 (its "Fluid Intelligence" runtime is closed) | macOS only | Swift | Nemotron 3.5, Parakeet Flash, v3, v2, Cohere, Whisper | n/a | Local 3.5 GB enhancement model or cloud | Not documented | 12,322 stars |
| **Buzz** | MIT | yes | Python, Qt | Whisper family | No insertion into other apps; a file and live transcriber | none | none | 22,102 stars |
| **Vibe** | MIT | yes | Tauri | Whisper, Nemotron 3.5, Parakeet v3 | File transcriber | none | none | 7,846 stars |
| **WhisperWriter** | GPL-3.0 | yes | Python, faster-whisper | Whisper | Types into the active window | none | none | last push 2024-08-24, stale |

Sources: each repository's README, LICENSE and GitHub API record [primary]; Handy and OpenWhispr code paths named in the table [primary, source]; SpeakoFlow's own comparison page, written by its maintainer [secondary].

### 3.2 The paid bar

- **Wispr Flow** [primary, docs.wisprflow.ai]. *Smart Formatting* removes fillers and fixes grammar, punctuation and capitalization, turns "one, two" into lists, honours spoken punctuation and "new line"; it "does not correct misheard words", and "if formatting fails, Flow pastes the unformatted transcript." *Backtrack* removes "clear self-corrections" cued by "actually", "scratch that", "never mind" or a restatement ("coffee at 2 actually 3" becomes "coffee at 3"), and leaves "I actually enjoyed the movie" alone. The *dictionary* takes manual entries and misspelling corrections, and on Mac and Windows can **auto-learn** from the user's edits, skipping file names, URLs, numbers, code, emoji, everyday words, very common names and common acronyms; entries are capped at 60 characters and sync through the account. There are also Styles, Snippets, Command Mode and an MCP server. Processing is in the cloud; one guide reports audio going to Baseten and text to OpenAI, Anthropic or Cerebras, and a May 2026 controversy over screenshots for "Context Awareness" [secondary, competitor-written]. An earlier review measured the Windows app at about 800 MB RAM idle (prior-art report) [secondary].
- **Superwhisper** runs on Windows with local Whisper and Parakeet and custom AI "modes" [secondary]. **Aqua Voice** uses its own cloud model, Avalon [secondary, vendor claims]. **Willow** is cloud-first and claims about 200 ms latency [secondary, vendor claims]. **Windows Voice Typing** (Win+H) is online; **Voice access** is on-device on Windows 11 22H2 and later [secondary].

### 3.3 Verdict

Handy is the open-source leader for Windows: permissive license, Rust, the broadest benchmarked model catalog, and the most careful Windows paste code found. It still pastes the same way into every field, cleans up only through API-key providers or a local endpoint, learns no vocabulary by itself, and has no voice editing. OpenWhispr has the better product ideas (auto-learn, app-category tones, terminal handling) on a heavier Electron base. The paid tools win on polish because a server-side LLM rewrites every utterance, at the cost of privacy and a subscription. Eileen can beat both by doing locally what the paid tools do in the cloud, and by using what it already knows about the focused field.

## 4. Speech models for dictation

Handy's catalog (`src-tauri/src/catalog/catalog.json`, schema 2) benchmarks every model the same way: FLEURS test set, ggml Q8_0 through `transcribe.cpp`, speed as multiples of real time on a Ryzen 7 4750U laptop, CPU and Vulkan [primary, source]. Selected English rows:

| Model | License | FLEURS en WER | CPU speed (x real time) | Vulkan iGPU | Q8 size | Streaming |
|---|---|---|---|---|---|---|
| Qwen3-ASR 1.7B | Apache-2.0 | 3.23 | 2.5 | 4.1 | 2.2 GB | no |
| Canary-Qwen 2.5B | CC-BY-4.0 | 3.58 | 2.5 | 4.0 | 2.8 GB | no |
| **Parakeet Unified EN 0.6B** | NVIDIA Open Model License | **3.99** | **14.0** | 25.3 | 731 MB | **yes** |
| Parakeet TDT 0.6B v2 | CC-BY-4.0 | 4.11 | 14.3 | 27.4 | 730 MB | no |
| Qwen3-ASR 0.6B | Apache-2.0 | 4.23 | 5.3 | 9.7 | 850 MB | no |
| Whisper large-v3 turbo | Apache-2.0 (model card) | 4.38 | 1.4 | 3.1 | 886 MB | no |
| Granite Speech 5.0 470M TurboCTC | Apache-2.0 | 4.61 | 17.2 | 22.9 | 506 MB | no |
| **Parakeet TDT 0.6B v3** (multilingual, 25 languages) | CC-BY-4.0 | 4.83 | 13.7 | 25.1 | 740 MB | no |
| **Nemotron speech streaming EN 0.6B** | NVIDIA Open Model License | 6.43 | 13.6 | 27.1 | 730 MB | yes |
| Moonshine streaming medium | MIT | 7.87 | 9.7 | 14.3 | 296 MB | yes |

On this PC, under load, Parakeet v3 int8 took 428 to 2,304 ms per VAD segment (real-time factor 0.18 to 0.51) and about 750 MB, and took 9 to 21 s to load; Nemotron's final arrived about 0.4 s after speech ended (0.74 s with Parakeet also loaded) [voice-stack]. On the voice-stack's synthetic command corpus Nemotron scored better than Parakeet (2.35 against 3.5 percent), mostly from number formatting; on FLEURS Parakeet is better.

What this means for dictation:

- **Use the already-loaded streaming model for dictation by default.** It costs nothing extra, gives live partials for the HUD, and its final lands about 0.4 s after the key is released. Loading a second 750 MB model on demand costs 9 to 21 s, which no dictation user will wait for.
- **Measure Parakeet Unified as the single always-on English model.** It is one network for streaming and offline (shared weights, 160 ms to 2.08 s latency settings), with punctuation and capitalization, commercial use allowed, and sherpa-onnx int8 exports in streaming 240, 560 and 1,120 ms and non-streaming variants [primary]. Handy's catalog calls it the "best all-around choice for English dictation" [primary, source]. If it holds up on the owner's real speech, it replaces Nemotron for both conversation and dictation.
- **Parakeet v3 is the multilingual dictation model**, loaded when the user dictates in another language or asks for it, and unloaded after an idle timeout (Handy exposes the same setting).
- **For long dictations, run the final pass per VAD segment while the user is still speaking**, so at release only the last segment is outstanding [inferred].
- Later, `transcribe-cpp` (MIT, ggml) is a route to the Arc iGPU through Vulkan, roughly 1.8x faster than CPU on Handy's Vega iGPU numbers. The voice-stack report found OpenVINO not worth a second runtime in v0.1; this is a different backend and needs its own measurement [inferred].

## 5. Inserting text into any Windows app

### 5.1 The methods

| Method | Needs foreground | Where it works | Failure modes |
|---|---|---|---|
| Clipboard then Ctrl+V via `SendInput` | yes | Almost everywhere: Win32, WinUI, Chromium and Electron, Office, RDP sessions with clipboard redirection | Overwrites the user's clipboard unless snapshotted and restored after the target has read it; lands in Win+V history unless marked; terminals need Ctrl+Shift+V; a few apps block paste |
| Clipboard then `WM_PASTE` message | no | Win32 Edit and RichEdit only | Same clipboard issues; ignored by modern UI stacks |
| `EM_REPLACESEL` message | no | Win32 Edit and RichEdit (classic apps, many dialogs) | Ignored by WinUI, Chromium, Electron, Office canvases |
| `SendInput` with `KEYEVENTF_UNICODE` | yes | Everywhere in principle | Keystrokes go to whatever window is foreground when each event is processed, so a focus change mid-payload types into the wrong app; autocomplete and auto-pairing in editors alter the text; garbled in WinUI Notepad unless paced (below) |
| `PostMessage(WM_CHAR)` per character | no | Win32 controls | Asynchronous; under load characters arrive after the call returns (seen here) |
| UIA `ValuePattern.SetValue` | no | Controls that expose Value (many Win32 and WinUI fields, not Chromium page content) | **Replaces the whole value**, so it is wrong for inserting at a caret; may bypass undo and the app's own change events [inferred] |
| UIA `TextPattern` | n/a | Read-only: no insert API | Useful to read the text around the caret and to verify |
| TSF text input processor (the IME route Windows speech uses) | no | Every app that uses the Text Services Framework | A COM DLL loaded into other processes, with install and per-app quirks; the only route to live, underlined in-field text while speaking. A later experiment [inferred] |

### 5.2 Measurements

**This run, 2026-10-20** [ran 10-20]. A Python driver in one process timed insertion into a RichEdit 5.0 control in a window created by a second process, from the start of the call until `WM_GETTEXTLENGTH` reached the expected length, then checked the text exactly. Payloads: a 58-character sentence, a 51-character string with accents, a check mark and an emoji (surrogate pair), and 1,135 characters. Every foreground method checked that the test window was still in the foreground before each chunk of at most 32 characters and skipped otherwise; the clipboard was to be snapshotted and restored, and only if it held nothing but text.

| Method | Run 1, CPU about 88 percent, p50 (max) | Run 2, CPU 100 percent, p50 (max) | Notes |
|---|---|---|---|
| `EM_REPLACESEL` | 5.8 ms (19.4) / 6.4 ms (346) / 45.6 ms (55.8) | 98.6 ms / 35.9 ms / 168 ms | Exact every time, including the emoji |
| UIA `ValuePattern.SetValue` | 12.4 ms (one outlier 31.0 s) / 13.9 ms / 14.9 ms | 90.0 ms / 37.8 ms / 43.9 ms | First run of run 2 threw `UIA_E_TIMEOUT` (0x80131505) under 100 percent CPU; UIA setup took 135 to 197 ms |
| `PostMessage(WM_CHAR)` | 130 ms / 127 ms / 1,135 chars: 4 of 5 timed out at 10 s | 1,262 ms / 908 ms / skipped | A late character from this method appeared in the next method's field: posted input is not done when the call returns |
| `SendInput` Unicode | refused: the test window was not foreground | refused | Run 3: one 58-character sample took 2,651 ms while focus was contested; then refused again |
| Clipboard + Ctrl+V, clipboard + `WM_PASTE` | skipped | skipped | The owner's clipboard held non-text formats, which this test would have destroyed |

Times in the cells are short / Unicode / long payloads. Load made run 2 about 5 to 17 times slower than run 1, so read these as relative, not absolute.

**Computer-use run, 2026-10-11, owner's Windows 11 Notepad** [computer-use]: unpaced Unicode `SendInput` arrived garbled in every variant; only Unicode events paced 30 ms apart arrived intact, at 2,074 ms for about 65 characters; virtual-key typing lost Shift (`;` for `:`); `ValuePattern.SetValue` wrote the text in 110 ms in the background. On 2026-10-20 every `SendInput` path in that run was refused because Windows would not give the foreground to a background process while the owner was active.

What the numbers say:

1. **Targeted messages are fast and safe where they work.** `EM_REPLACESEL` inserts at the caret, needs no focus and no clipboard, and handled every payload exactly.
2. **Synthetic typing is the worst default.** It is slow when paced and wrong when not, and it is the only method that can type into the wrong window. Handy also defaults to Ctrl+V on Windows, not typing [primary, source].
3. **The clipboard path must respect what is on the clipboard.** This run refused to touch a clipboard holding images or rich formats. Handy solves this properly: it snapshots every format with full fidelity, publishes the transcript as a delayed-render promise, treats the `WM_RENDERFORMAT` callback after the keystroke as a read receipt, waits for a 200 ms quiet period (Chromium reads twice), restores only while it still owns the clipboard (sequence number unchanged), and gives up after 8 s, so "the failure mode is always the transcript stays on the clipboard a bit longer, never stale content gets pasted" [primary, `paste_tx/mod.rs`]. It also sets `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0` and `CanUploadToCloudClipboard = 0`, the formats Chrome uses, so dictated text stays out of Win+V history and cloud sync [primary, `paste_tx/windows.rs`].
4. **Foreground is not Eileen's to take.** In normal dictation the target is already the foreground window (the user is typing there), so this is fine. If focus moves during dictation, Eileen must not steal it back to paste; it keeps the text in the HUD and pastes when the user says "paste it" or clicks.

### 5.3 Eileen's insertion ladder

At record start, the desktop service records the focused element: window handle, process, class, UIA control type, `IsPassword`, read-only state, and for browsers the URL host. At insertion:

1. **Refuse** password fields and secure desktops. Report elevated targets that Windows' integrity rules block, instead of failing silently.
2. **Check the target is unchanged** (same window handle, still foreground for steps 4 and 5). If not, hold the text in the HUD.
3. **Win32 Edit or RichEdit class:** `EM_REPLACESEL` with undo enabled.
4. **Everything else:** clipboard transaction (port of Handy's `paste_tx`), then Ctrl+V; Ctrl+Shift+V for terminal classes and exes (OpenWhispr's list: `ConsoleWindowClass`, `CASCADIA_HOSTING_WINDOW_CLASS`, mintty, PuTTY, Alacritty, WezTerm, kitty and others).
5. **Last resort** (paste blocked, or the user disabled clipboard use): Unicode `SendInput` paced at 30 ms per character, foreground check before every character.
6. **Verify** where UIA can read the field (Value or TextPattern): the inserted text is present at the caret. In Chromium content, where UIA cannot see the page [computer-use], rely on the paste receipt.
7. **Record** the insertion (text, target, method, time) in dictation history for "undo that", "paste it again" and correction learning.

All of this belongs in the desktop service next to computer use, behind the same input lock, so a dictation paste and an agent's click never interleave [inferred, consistent with computer-use section 4].

## 6. Cleanup: fillers, false starts, self-corrections and formatting

### 6.1 Tier 0: local rules, always on

Order, following SpeakoFlow's spec (vocabulary and fillers before any model, user rules after it) [primary]:

1. Vocabulary replacement (section 7).
2. "Scratch that", "delete that", "strike that": drop the clause back to the previous sentence end.
3. Fillers from per-language tables (SpeakoFlow ships 16 languages; "um" is a filler in English and an article in Portuguese, so one global list is wrong) [primary].
4. Stutters: runs of three or more identical words always collapse; exact doubles collapse only for function words that are almost never doubled on purpose ("the the"), so "that that" and "had had" survive.
5. **Typed-slot self-corrections.** `<value> <cue> <value>` where both values are the same type (time or number, weekday, month) and the cue is one of "no", "no wait", "wait", "sorry", "actually", "I mean", "or rather", "make that", "correction". The second value wins. Anything else, notably noun phrases ("the red room, no, the blue room"), is left for the scribe.
6. Spoken punctuation and layout ("new line", "new paragraph") and spoken formatting for code ("camel case user id"), Talon-style.
7. Context formatting by app category (section 6.3).
8. The user's own find-and-replace rules, last, so they always win.

Parakeet and Nemotron already punctuate, capitalize and often write numbers as digits; Eileen should not rebuild that [primary, SpeakoFlow notes; NVIDIA cards].

**Prototype result** [ran 10-20]: `cleanup_rules.py` passed 30 of 30 cases written for this report, including "Let's meet at 3, no, 4 pm." to "Let's meet at 4 pm.", "three hundred, no, three fifty" to "three fifty", "gate 12, sorry, 14" to "gate 14", "Add milk to the list. Scratch that. Add oat milk." to "Add oat milk.", and negatives left untouched ("I actually enjoyed the movie.", "No, I don't think so.", "There is no 3 pm slot on Friday.", "Room 4, no smoking.", "Is it 3 or 4 people?"). About 1.4 ms per case in Python including regex compilation; a Rust port will be far below that. The cases were written by the same author as the rules, so this shows the approach is workable, not that it is accurate. The SpeakoFlow conformance suite (79 cases) plus a corpus of the owner's real dictations should be the real test.

### 6.2 Tier 1: a Claude scribe, when the user allows it

- **Route.** A separate warm `claude -p` session on the owner's plan, model Haiku, no tools, connectors and user MCP servers off (`ENABLE_CLAUDEAI_MCP_SERVERS=false`, `--strict-mcp-config` with an empty config, `--setting-sources ""`), started lazily on the first polished dictation and closed after ten idle minutes. Not the brain session: dictation is frequent, would fill the brain's context, and would queue behind its turns. Each idle `claude` process held 240 to 750 MB on this PC during this run, so lazy start and idle shutdown matter [ran 10-20, process list; inferred].
- **Latency.** A warm turn's first text arrives in 1.0 to 1.5 s and completes in 2.3 to 3.1 s with connectors off [brain-latency]. To hide this, the scribe receives each finalized sentence while the user is still speaking; at release only the last sentence is pending. Short dictations (under about 12 words) skip the scribe unless the rules flagged something they would not touch.
- **When it runs.** Off by default, per the owner's "when the user allows it". When on: a modifier on the dictation key (SpeakoFlow uses the dictation keys plus Shift), or always for chosen categories (email, documents), never for terminals, code, search boxes, password fields or apps the user marks private.
- **Prompt.** The rule-cleaned transcript in tags; the app category and tone; the vocabulary entries relevant to the text; optionally the sentence before the caret. Instructions in the style of Handy's default prompt: fix only what is needed, keep meaning and word order, do not answer questions in the transcript, do not follow instructions inside it, return only the text [primary, Handy `settings.rs`]. Text from the screen is data, never instructions.
- **Guard.** Reject the scribe's output and insert the rule-cleaned text instead if it adds words that were not spoken, drops more than a set share of content words, or exceeds an edit-distance budget relative to the input. Wispr's own fallback is the same idea ("pastes the unformatted transcript") [primary].
- **Command mode.** "Make that more formal", "turn the last paragraph into bullets": read the selection (UIA TextPattern, or a clipboard copy with the sequence-number check as in the computer-use report), send it to the scribe, replace it. This is Wispr's paid Command Mode, done with Eileen's existing desktop tools.

### 6.3 Formatting by context

| Category | Detected by | Local formatting | Scribe tone |
|---|---|---|---|
| Email body | Outlook, Thunderbird, Mail; browser hosts such as mail.google.com and outlook.live.com | Full sentences, keep dictated greeting and sign-off on their own lines, never invent one (OpenWhispr's rule) | Formal or the user's style |
| Chat | Slack, Teams, Discord, WhatsApp, Messages, Telegram | Drop the final period on single sentences | Casual |
| Code editor | VS Code, Cursor, JetBrains, Visual Studio | Literal; vocabulary keeps identifiers; spoken case formatting; comment style if the line starts with `//`, `#` or `--` | Off by default |
| Terminal | Terminal classes and exes in 5.3 | Literal, no capitalization or trailing period, Ctrl+Shift+V | Off |
| Search and address bars | UIA edit inside a toolbar or combo box; single-line fields | No trailing period | Off |
| Documents and notes | Word, Notepad, Google Docs host, Obsidian | Full punctuation, paragraphs | Optional |

The categories match OpenWhispr's personal, work, email and other split [primary, `toneStyle.ts`], with code and terminal added because they are where wrong "help" does the most damage [inferred]. Per-app overrides are learned like any other preference.

### 6.4 Latency budget after the key is released

| Step | Estimate on this PC | Source |
|---|---|---|
| Streaming recognizer final (tail flush) | about 0.4 s, 0.74 s with Parakeet loaded | [voice-stack] |
| Tier 0 rules | under 1 ms in Rust | [ran 10-20, Python 1.4 ms; inferred for Rust] |
| Insertion | 6 to 46 ms (`EM_REPLACESEL`); clipboard paste not measured here | [ran 10-20] |
| Total, local path | **about 0.4 to 0.8 s** | sum |
| Scribe on the last sentence, if on | plus 1 to 3 s | [brain-latency] |

## 7. Personal vocabulary that learns

- **Where it lives.** The learning loop's user model, as `vocabulary` facts with provenance (said, corrected, imported), counts, first and last use, and the app where they were learned (2026-10-learning-loop.md). The user can ask "what words have you learned?", correct one, or say "forget the word X".
- **Three uses of one list.** (1) Decoder biasing: sherpa-onnx accepts hotwords per stream for NeMo transducers, TDT included, with `modified_beam_search` and a `hotwords_score` [primary, `offline-recognizer-transducer-nemo-impl.h`]; the CPU cost of beam search over greedy on this PC is unmeasured. (2) A post-pass replacement: Handy's matcher compares n-grams with Soundex and normalized Levenshtein and boosts phonetic matches [primary, `audio_toolkit/text.rs`], and SpeakoFlow joins multi-token spellings ("Charge B" to "ChargeBee") [primary]. (3) The scribe prompt.
- **Learning from corrections.** After an insertion, watch the field read-only through UIA (Value or TextPattern; never write accessibility attributes, which broke focus in Chromium apps for OpenWhispr on macOS [primary, `textEditMonitor.js`]) for about 30 s. A word-level substitution inside the inserted span, phonetically close to what was heard and not an ordinary dictionary word, becomes a candidate. Apply Wispr's exclusions (file names, URLs, numbers, code, emoji, everyday words, very common names and acronyms) [primary]. Two occurrences, or one with a HUD confirmation, make it an entry. In Chromium content, where UIA cannot read the page, corrections are not watched in v0.1.
- **Context words, per utterance only.** Names visible in the focused window (an email's To line, a chat's channel members) can be passed as hotwords for that one utterance and are never stored unless the user accepts them. This stays within the owner's rule that text seen on screen does not become learned content without the user's say-so [OWNER-DIRECTION; inferred application].
- **Never** learn from password fields, apps the user marks private, or dictation the user deleted.

## 8. Starting and stopping dictation in an always-listening system

- **Hold to talk** on a configurable key (default a single modifier such as Right Ctrl, which needs no key suppression), through a low-level keyboard hook on its own thread; **tap to toggle** on the same key; and **spoken**: "Eileen, take dictation" until "Eileen, stop dictating" or a long pause, or a one-shot "Eileen, type: see you at 4" [inferred design; Handy and SpeakoFlow offer hold and toggle per shortcut, primary].
- **While dictation is active, speech goes to the field, not the brain.** The addressee gate stops treating it as a request, except for a short escape grammar: "Eileen, stop", "scratch that", "new line", "undo that", "paste it".
- **The HUD** shows the live partial transcript, the target app and field, and whether polish is on; after insertion it shows what was inserted, with undo.
- **Echo and barge-in** come from the voice loop as they are: if Eileen is speaking when dictation starts, it stops speaking.
- **Mute wins.** Muted means the capture stream is stopped [voice-stack], so the dictation key does nothing until unmuted, and the HUD says so.

## 9. What Eileen reuses, and where it does better

| From | Take | License | How |
|---|---|---|---|
| Handy | `paste_tx` (Windows delayed-render clipboard transaction, receipts, guarded restore, history opt-out), custom-word matcher, filler and stutter logic, model catalog as a benchmark reference | MIT | Port into Eileen's Rust crates with the copyright notice kept |
| SpeakoFlow `dictation-cleanup-rules` | Rule spec, per-language filler tables, 79 conformance cases, vendored Rust | MIT | Port; run the conformance cases in CI |
| OpenWhispr | Terminal list, target capture at record start, read-only field monitor for auto-learn, tone and app-category prompts | MIT | Design and data; the C helpers are small enough to reimplement in Rust |
| Wispr Flow docs | Backtrack semantics, auto-learn exclusions, "paste raw if formatting fails" | n/a | Design |
| Whispering, VoiceTypr, VoiceInk, FluidVoice, WhisperWriter | Ideas only | AGPL-3.0 or GPL-3.0 | No code |

Where Eileen is better by construction:

1. **Local, free and offline by default**; the paid leaders send audio to the cloud.
2. **It knows the field**: app, control type, password state and the text around the caret come from desktop grounding, so formatting fits the field without screenshots.
3. **Vocabulary at three levels**, including the decoder, learned from real corrections, private and forgettable.
4. **Words appear in the HUD while you speak** (streaming recognizer), not after release.
5. **Verified insertion** that never types into the wrong window and never loses the user's clipboard.
6. **Voice editing and command mode** reuse Eileen's desktop tools, with no extra subscription.
7. **Polish through the owner's Claude plan**, opt-in, with a guard that rejects rewrites.
8. **Footprint**: dictation adds no model when it reuses the always-on recognizer; Parakeet v3 adds about 750 MB only while dictating in another language.

## 10. Voices: the field on 2026-10-20

| Model (maker) | License, code / weights | Size | Cloning | Streaming | CPU evidence | Emotion or style | Watermark | Rust or ONNX path | Fit for this PC |
|---|---|---|---|---|---|---|---|---|---|
| **Kokoro v1.0** (hexgrad) | Apache-2.0 | 82M | no (54 stock voices; style vectors can be blended) | by clause | RTF 0.54 quiet, 1.1 to 1.3 loaded [voice-stack]; 1.28 core-hours per hour, 2.0 GB peak [secondary, Picovoice] | voice choice, speed | none | sherpa-onnx crate | **default** |
| **Pocket TTS** (Kyutai) | MIT / CC-BY-4.0; cloning repo gated (accept terms), a no-cloning repo is open | 100M; v4 adds 18M, 7M, 2M English | yes, from a WAV prompt; voice state exportable to safetensors | yes, about 200 ms first chunk | about 6x real time on 2 cores of an M4 [primary]; 0.37 core-hours per hour, 610 MB peak [secondary, Picovoice]; TTFA p50 2.20 s, RTF 1.28 here under contention [voice-stack] | via the prompt clip | none | sherpa-onnx crate (Pocket config with `reference_audio`); official Rust CLI on `xn` (Apache-2.0) with a Windows binary | **cloning engine** |
| Kyutai TTS 1.6B | CC-BY-4.0 | 1.6B | only precomputed voice embeddings | yes, text streaming | GPU server (CUDA) | n/a | "not applied" | Rust server needs CUDA | no |
| **Chatterbox-Nano** (Resemble) | MIT | 110M | yes, 5 s clip | not in the official API yet | about 3x real time on 8 CPU threads [primary, vendor] | paralinguistic tags | Perth, on by default | community ONNX exports only | **first Chatterbox to test** |
| **Chatterbox-Turbo** | MIT | 350M | yes | streaming PR #528 open, not merged | first audio 48.3 s, 13.4 core-hours per hour, 7.5 GB peak (PyTorch) [secondary, Picovoice] | `[laugh]`, `[sigh]` and 17 more tags | Perth (the official ONNX example defaults it off) | official ONNX (fp32, fp16, q8, q4, q4f16); GGUF via CrispASR (MIT), q8 628 + 350 MB | pre-rendered only |
| Chatterbox and Multilingual V3 | MIT | 500M | yes, 23+ languages | no | slower than Turbo | exaggeration, CFG | Perth | community ONNX, GGUF | pre-rendered only |
| F5-TTS | MIT / **CC-BY-NC-4.0** | about 336M [inferred] | yes | no | GPU-oriented | no | none | community | license rules it out |
| OpenVoice v2 (MyShell) | MIT / MIT | small converter on MeloTTS | yes (tone color conversion) | no | CPU-capable [inferred] | some style control | none | none official | dated, last push 2025-04 |
| Fish Speech S2-Pro, Drama 3 | Fish Audio Community and Research licenses | 4B; 35B-A3B | yes | server | GPU | rich | none stated | none | no |
| IndexTTS2 (bilibili) | bilibili Model Use License (separate license above 100M MAU or RMB 1B revenue) | not checked | yes | no | GPU | emotion by clip, vector or text; duration | none stated | none | no |
| Orpheus 3B (Canopy) | Apache-2.0, gated; Llama 3.2 base [inferred] | 3B | yes | yes on GPU | GGUF exists; too slow on CPU [inferred] | tags | none | llama.cpp + SNAC | no |
| Sesame CSM-1B | Apache-2.0, gated | 1B + Mimi | yes, conversational context | partial | GPU | context | none | none | no |
| Dia 1.6B, Dia2 2B (Nari) | Apache-2.0 | 1.6B, 2B | audio prompt | Dia2 streams | GPU | non-verbal tags | none | none | no |
| VibeVoice (Microsoft) | MIT | 1.5B (code disabled after misuse); Realtime 0.5B (about 1.0B params, 2 GB) | Realtime: preset embedded voices only, "to mitigate deepfake risks" | Realtime, about 200 to 300 ms first audio | GPU | n/a | n/a | none | no |
| Higgs Audio v2, v3 (Boson) | Apache-2.0 code / community and **non-commercial** model licenses | 3B base; v3 4B | yes | server | GPU | rich | none stated | none | no |
| Zonos v0.1 (Zyphra) | Apache-2.0 | 1.6B | yes | no | about 2x real time on an RTX 4090 [secondary] | emotion sliders | none | none | no |
| XTTS-v2 (Coqui) | MPL-2.0 code (idiap fork) / **Coqui Public Model License, non-commercial** | about 0.5B [inferred] | yes, 6 s | yes | slow on CPU | some | none | none | license rules it out |
| Piper | **GPL-3.0** (piper1-gpl) | 20 to 60 MB | no | yes | 0.35 core-hours per hour [secondary, Picovoice] | voice choice | none | sherpa-onnx | GPL; no cloning |
| NeuTTS Air / Nano (Neuphonic) | Apache-2.0 / NeuTTS Open License 1.0 | 552M / 229M total params | yes, 3 s | GGUF only | Nano Q4: first audio 2.95 s, 7.3 core-hours per hour, 2.1 GB [secondary, Picovoice] | NeuTTS-2E: 4 fixed speakers, 6 emotions | **watermarked outputs** | GGUF via llama.cpp | possible, slower than Pocket |
| Qwen3-TTS (Alibaba) | Apache-2.0 | 0.6B, 1.7B | yes, 3 s; voice design from text | yes, 97 ms first packet (GPU) [primary] | not published | instructions | none | none official | later test |
| VoxCPM2 (OpenBMB) | Apache-2.0 | 2.29B (4.96 GB files) | yes; voice design | yes | GPU | yes | none | none | no |
| OmniVoice (k2-fsa) | Apache-2.0 | 612M + 806 MB tokenizer | yes, 3 to 10 s with transcript | no | "RTF as low as 0.025" (hardware not stated); Intel Arc supported through PyTorch XPU [primary] | voice design | none | not yet in sherpa-onnx | later test on the Arc iGPU |
| LuxTTS / ZipVoice | Apache-2.0 (ZipVoice weights trained on Emilia, license unclear) | small | yes, 3 s | no | "faster than real time on CPU" [primary, vendor] | no | none | ZipVoice in sherpa-onnx (zh-en); LuxTTS community ONNX | check license |
| Supertonic 3 (Supertone) | MIT / OpenRAIL; repo archived 2026-09 | 99M | custom voices only through a hosted Voice Builder | yes | 0.52 core-hours per hour for v2 [secondary, Picovoice] | no | none | sherpa-onnx, Rust example | no local cloning |
| Kitten TTS, Soprano | Apache-2.0 | tiny to 80M | no | yes | fast | no | none | sherpa-onnx (Kitten) | no cloning |
| Voxtral TTS (Mistral) | **CC-BY-NC-4.0** | 4B | yes, 3 s | yes | GPU | yes | none | none | license rules it out |

Sources: each project's repository and model card, and the Hugging Face API for license, gating and sizes [primary], except where tagged. The CPU benchmark is Picovoice's own (AMD Ryzen 7 5700X, 64 GB, about 200 simulated assistant turns, published 2026-07-14, updated 2026-08-18); Picovoice sells a competing engine, so the figures are useful for ranking open models against each other on one machine, not as absolute numbers [secondary].

Two license details to carry forward:

- The sherpa-onnx Pocket TTS archive on this PC contains a CC-BY-4.0 `LICENSE`, and its README says "It is for non-commercial." The upstream weights and the ONNX export it was made from (`KevinAHM/pocket-tts-onnx`) are both CC-BY-4.0, with Apache-2.0 export code [primary]. The README line looks wrong, but it should be confirmed with the sherpa-onnx maintainer before Eileen ships a downloader for that archive.
- Kyutai gates the cloning-capable weights behind an "accept the terms" step on Hugging Face, whose terms prohibit "voice impersonation or cloning without explicit and lawful consent" [primary]. The ungated ONNX export does not show those terms, so Eileen should show them itself before the first clone (section 11.4).

## 11. Recommended voice architecture

### 11.1 Engines and roles

| Role | Engine | Why |
|---|---|---|
| Live replies, default | Kokoro v1.0 fp32 via `sherpa-onnx` | Already chosen and measured; fastest good voice here |
| Live replies in a cloned voice | Pocket TTS via `sherpa-onnx` (`OfflineTtsPocketModelConfig`, `GenerationConfig.reference_audio`, voice-embedding cache) | Same crate and runtime as the rest of the voice stack, 100M, streams, clones from a clip, permissive licenses |
| Smaller cloned voice, if quality holds | Pocket TTS v4 18M or 7M English through Kyutai's Rust CLI as a sidecar until sherpa-onnx exports them | Lower CPU; cloning support in the small models is not yet verified |
| Expressive pre-rendered speech in a cloned voice | Chatterbox-Nano, then Turbo, as an optional download and sidecar | Best expressiveness in the family (tags, exaggeration), built-in watermark; too heavy for live use here |
| Stock phrases ("On it.", "Done.") | PCM cache rendered once per voice by whichever engine the voice uses | Zero latency; the voice-stack report already plans the cache |
| A cloned voice at Kokoro speed (later) | Offline style-vector search for Kokoro in the manner of KVoiceWalk (Apache-2.0; 10,000 steps reached 93 percent Resemblyzer similarity in its example, on a GPU) | Clone once overnight, then speak at Kokoro speed; unverified on this CPU |

### 11.2 Can a cloned voice stream live on this PC?

**Sometimes.** Live conversation needs the first clause within about half a second and a real-time factor comfortably under 1. Measured here, Pocket TTS ran at 1.28 with 2 GB free and Kokoro at 0.54 quiet and 1.1 to 1.3 loaded [voice-stack]. Published numbers put Pocket TTS at about twice Kokoro's speed on the same machine [secondary, Picovoice] and about 6x real time on two M4 cores [primary]. So when the PC is not saturated, Pocket TTS should keep up with live replies, and when other agents and builds are running, nothing local does reliably.

Therefore:

- Split replies at clause boundaries with a short first clause (voice-stack rule), and synthesize ahead while playing.
- A **watchdog** keeps the measured real-time factor of the last few clauses per engine. Above 0.8 it switches the *next reply* (never mid-reply) to Kokoro and notes it in the HUD; it switches back when headroom returns.
- **Chatterbox is not live on this PC.** Its role is pre-rendering, where waiting is acceptable: narration of long text rendered ahead of playback, stock phrases, notifications.
- Run Pocket TTS at two threads first (Kyutai's design point), then measure four.

### 11.3 How voices are configured

```
%LOCALAPPDATA%\eileen\voices\
  owner\
    voice.toml
    reference.wav        # 10 to 15 s, 24 kHz mono, clean (Pocket uses at most 10 s)
    consent.wav          # the consent sentence, read by the same speaker
    pocket.safetensors   # cached voice state, rebuilt from reference.wav if missing
```

```toml
# voice.toml
name = "owner"
engine = "pocket"            # kokoro | pocket | chatterbox
reference = "reference.wav"
language = "en"
speed = 1.0
[consent]
kind = "self"                # self | consented-other
recorded = "2026-10-20T15:40:00Z"
speaker_match = 0.82         # cosine similarity, consent.wav vs reference.wav
terms_shown = "kyutai-pocket-tts-2026-10"
watermark_exports = true     # cannot be set false when engine = "chatterbox"
```

```toml
# Eileen's main config
[voice]
live = "kokoro:af_heart"     # or "owner"
narration = "owner"          # long-form and pre-rendered speech
fallback = "kokoro:af_heart" # used by the watchdog
# Kokoro blends: "kokoro:af_heart*0.7+bf_emma*0.3"
```

Spoken commands in the deterministic grammar (no model call): "use my voice" (starts enrollment if there is no voice for the owner), "use the British voice", "use the default voice", "talk slower", "which voice is this?", "delete my voice". "Clone this voice" with a file is accepted only with a consent recording from the same speaker (11.4).

### 11.4 Consent and watermark policy

1. **Enrollment, not upload.** A cloned voice is created from a live recording through Eileen: the speaker reads a consent sentence naming the date and that the voice is for this Eileen install, then a short passage. The passage becomes the reference.
2. **Same-speaker check.** sherpa-onnx's speaker-embedding extractor (the WeSpeaker CAM++ model, already downloaded on this PC [voice-stack]) must score the consent recording and the reference as the same person above a threshold. For a consented other person, they record the consent sentence themselves; a file of someone else with only the owner's consent is refused.
3. **Show the model's terms** (Kyutai's prohibited uses, Resemble's) before the first clone, and record that they were shown.
4. **Watermarks.** Chatterbox's Perth watermark is always applied, including in any ONNX or GGUF port Eileen uses, whose examples default it off [primary]; the config cannot disable it. Pocket TTS and Kokoro have no built-in watermark, so any cloned-voice audio that is saved to a file, exported, or sent off the PC (phone connector, messages) gets a watermark applied by Eileen (AudioSeal or Perth, both MIT), plus provenance metadata in the file.
5. **Live playback** through the PC's own speakers is not watermarked in v0.1 unless the owner decides otherwise (open risk 7).
6. **Use limits.** Speaking as the user to third parties (calls, voice messages) is a consequential action that needs per-use confirmation under the permission tiers; a learned skill can never skip that.
7. **Forget.** "Delete my voice" removes the reference, the consent clip, cached states and the PCM cache, and records the deletion in the learning store.

### 11.5 Crates and sidecars for the Rust core

| Need | Choice | License | Notes |
|---|---|---|---|
| Kokoro, Pocket TTS, speaker embedding | `sherpa-onnx` 1.13.8 crate (with `sherpa-onnx-sys`) | Apache-2.0 | Already the voice stack's runtime; Rust examples cover `pocket_tts.rs` (zero-shot cloning), `kokoro_tts_*`, `supertonic_tts.rs`, `zipvoice_tts_lexicon.rs` and the speaker-embedding manager. The espeak-ng GPL question (voice-stack risk 3) applies to the DLL whatever the model; Pocket TTS itself needs no phonemizer, since it uses a tokenizer (`vocab.json`, `token_scores.json`) |
| Pocket TTS small models before sherpa-onnx has them | Kyutai's `pocket-tts` Rust CLI as a sidecar (`serve` on localhost, `/tts` with `voice_wav`) | MIT, on `xn` and `ptts` (Apache-2.0) | Windows x86 binary about 8 MB; not on crates.io (`publish = false`), pinned to a git revision, so a sidecar is simpler than linking |
| Chatterbox-Nano or Turbo | Phase 1: Python sidecar in a `uv` venv with CPU PyTorch, started on demand, killed when idle. Phase 2: the official ONNX graphs (`speech_encoder`, `embed_tokens`, `language_model`, `conditional_decoder`) through `ort`, or GGUF through CrispASR (MIT, ggml, Vulkan possible on Arc) | MIT | Several GB of RAM in PyTorch; download only when the user enables it |
| Watermark | AudioSeal or Perth in the same sidecar, or an ONNX export later | MIT | Applied on save or export |
| Audio I/O, resampling | as in the voice stack (`cpal`, `rubato` or the sherpa-onnx resampler) | | |

A single `Synth` trait (stream clauses in, PCM chunks out, cancel through the callback) with three implementations: sherpa-onnx Kokoro, sherpa-onnx Pocket, and a localhost sidecar client with a random per-launch token [inferred design].

## 12. Claude extension points that touch dictation and voice

The claude-mods report covers the mechanisms. Two small links for this topic:

- **Dictating into Claude itself.** The Claude desktop app is Chromium (`Chrome_WidgetWin_1`) [computer-use], so dictation reaches it through the clipboard rung like any browser. No mod is needed. OpenWhispr found on macOS that writing an accessibility attribute to force the tree blurred the focused editor in Claude Desktop and claude.ai, so later dictations missed the field; that is the reason Eileen's correction monitor stays read-only on Windows too.
- **Claude speaking in Eileen's voice.** A mod hooked on `audio.speak` can route Claude's spoken output to Eileen's TTS, so the chosen or cloned voice is used there too (claude-mods report, item H, marked later).

## 13. Design changes for BUILD.md and the work packages

See the structured summary that accompanies this report; the same list is the basis for the work packages:

1. A dictation module in the Rust core: triggers (hold, toggle, phrase), session state, live partials to the HUD, final, Tier 0 rules, optional scribe, insertion ladder, verification, history and undo.
2. The insertion ladder in the desktop service behind the shared input lock, including a port of Handy's `paste_tx` (MIT, attribution kept) with full-format clipboard snapshots and the three clipboard-history opt-out formats.
3. Tier 0 cleanup as a pure Rust crate with SpeakoFlow's 79 conformance cases and Eileen's typed-slot retraction cases (positives and negatives) in CI.
4. A "scribe" route: a lazily started Haiku `claude -p` session with no tools and connectors off, sentence-by-sentence while speaking, idle shutdown, an edit-distance guard, and per-category opt-in.
5. Vocabulary facts in the learning store; sherpa-onnx hotwords; a Soundex plus Levenshtein post-pass; a read-only UIA correction watcher; per-utterance context hotwords never persisted without consent.
6. A measurement package: Parakeet Unified EN 0.6B streaming 560 ms against Nemotron 560 ms on the owner's real speech (WER, partial latency, tail, RSS), and the CPU cost of `modified_beam_search` with hotwords.
7. A `Synth` trait with Kokoro and Pocket TTS through `sherpa-onnx`, the per-reply watchdog, and an optional sidecar client.
8. A voices folder, `voice.toml` schema, a `[voice]` config section, and the spoken voice commands in the deterministic grammar.
9. Voice enrollment with the consent sentence, a same-speaker check (CAM++), terms shown, and deletion.
10. Watermarking on save and export (AudioSeal or Perth), always-on Perth for Chatterbox.
11. A TTS measurement package, run only with at least 6 GB free: Pocket TTS 100M and v4 small models cloning the owner's voice (TTFA, RTF at 2 and 4 threads, RSS, listening test), Chatterbox-Nano on CPU, Chatterbox-Turbo q4 ONNX.
12. An insertion measurement package on owned fixtures while the owner is idle: clipboard paste into owned Notepad (launched by AUMID, then its tab closed without saving), an isolated Edge profile, VS Code with a scratch folder, Windows Terminal; and paced `SendInput` timing.
13. Updates to ARCHITECTURE.md (TTS: Pocket TTS as the cloned-voice engine, Chatterbox optional), RESEARCH.md (Pocket TTS license note, Chatterbox CPU numbers) and a DECISIONS.md record for "Pocket TTS over Chatterbox for live cloning on this PC".

## 14. Open risks and follow-ups

1. **Nothing heavy was measured today.** Cloning quality and speed on the owner's own voice, Chatterbox on this CPU, and Parakeet Unified are all unmeasured here; free RAM never exceeded 2.5 GB.
2. **Clipboard paste and `SendInput` were not measured here.** The clipboard held non-text data and the foreground could not be held. The design leans on Handy's code and the computer-use run's Notepad results.
3. **Pocket TTS licensing and gating.** The sherpa-onnx archive's README says non-commercial while its LICENSE says CC-BY-4.0; Kyutai gates cloning weights behind terms. Confirm before shipping a downloader.
4. **The scribe costs plan usage and sends dictated text to Anthropic**, like the brain does. It stays opt-in, never runs on private apps, and treats screen text as data.
5. **Hotword decoding** needs beam search; its CPU cost on top of the always-on recognizer is unknown and could push partial latency up.
6. **Correction learning does not work in Chromium page content** without the browser rung, which covers many of the apps people dictate into most (mail and chat in the browser, Slack, Teams).
7. **Watermarking live playback** of a cloned voice is undecided. Watermarking every live clause costs CPU this PC does not have to spare; not watermarking means a recording of the room could carry an unmarked clone. Owner decision.
8. **NVIDIA Open Model License** covers Nemotron and Parakeet Unified (commercial use allowed, not an OSI license); Parakeet v3 is CC-BY-4.0. Same mitigation as the voice-stack report: show the license at download.
9. **The rule prototype was tested on cases its author wrote.** It needs the SpeakoFlow suite and a consented set of the owner's real dictations, including false-positive checks.
10. **TSF** (in-field live text with composition) is the one route to "words appear in the field as you speak" and is unexplored.
11. **Paid-tool claims are mostly secondary**, apart from Wispr's own documentation; latency comparisons with Wispr Flow were not made.
12. **Memory.** Nemotron or Parakeet Unified, Kokoro and Pocket TTS together are about 1.8 to 2 GB resident, before Parakeet v3 or a Chatterbox sidecar; with about 5 GB free on a normal day, Eileen needs explicit load and unload rules per model.

## Sources

Dictation, primary (read 2026-10-20):
- Handy: https://github.com/cjpais/Handy (files `src-tauri/src/paste_tx/mod.rs`, `paste_tx/windows.rs`, `clipboard.rs`, `settings.rs`, `audio_toolkit/text.rs`, `catalog/catalog.json`, `Cargo.toml`); transcribe.cpp: https://github.com/handy-computer/transcribe.cpp
- OpenWhispr: https://github.com/OpenWhispr/openwhispr (files `resources/windows-fast-paste.c`, `resources/windows-text-monitor.c`, `src/helpers/textEditMonitor.js`, `src/config/prompts/toneStyle.ts`)
- SpeakoFlow: https://github.com/AbhishekBarali/SpeakoFlow ; cleanup rules: https://github.com/AbhishekBarali/dictation-cleanup-rules
- Amical: https://github.com/amicalhq/amical ; VoiceTypr: https://github.com/ideaplexa/voicetypr ; Epicenter (Whispering): https://github.com/EpicenterHQ/epicenter ; VoiceInk: https://github.com/Beingpax/VoiceInk ; FluidVoice: https://github.com/altic-dev/FluidVoice ; Buzz: https://github.com/chidiwilliams/buzz ; Vibe: https://github.com/thewh1teagle/vibe ; WhisperWriter: https://github.com/savbell/whisper-writer
- Wispr Flow docs: https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack ; https://docs.wisprflow.ai/articles/4052411709-teach-flow-your-words-with-the-dictionary ; https://docs.wisprflow.ai/collections/7303194563-using_wispr_flow
- NVIDIA model cards: https://huggingface.co/nvidia/parakeet-unified-en-0.6b ; https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b
- sherpa-onnx: https://github.com/k2-fsa/sherpa-onnx (release tags `asr-models`, `tts-models`; `sherpa-onnx/csrc/offline-recognizer-transducer-nemo-impl.h`; `rust-api-examples/examples/`)
- crates.io records: `sherpa-onnx` 1.13.8, `windows` 0.62.2, `uiautomation` 0.25.1, `enigo` 0.6.1, `arboard` 3.7.0, `transcribe-cpp` 0.4.0, `transcribe-rs` 0.3.12, `ort` 2.0.0-rc.15

Dictation, secondary:
- https://www.speakoflow.com/blog/wispr-flow-alternatives-windows (SpeakoFlow maintainer, updated 2026-10-17)
- https://openalternative.co/alternatives/wisprflow
- https://www.getvoibe.com/resources/wispr-flow-review/ and https://www.getvoibe.com/resources/aqua-voice-vs-superwhisper/ (a competitor)
- https://willowvoice.com/blog/superwhisper-vs-otter-ai-comparison (a competitor)

Voices, primary:
- Pocket TTS: https://github.com/kyutai-labs/pocket-tts (v4.0.0 release notes, `rust/Cargo.toml`); https://huggingface.co/kyutai/pocket-tts ; https://huggingface.co/kyutai/pocket-tts-without-voice-cloning ; https://huggingface.co/KevinAHM/pocket-tts-onnx ; https://github.com/gradium-ai/xn-ptts ; serve docs: https://kyutai-labs.github.io/pocket-tts/CLI%20Commands/serve/
- Kyutai TTS: https://huggingface.co/kyutai/tts-1.6b-en_fr ; https://github.com/kyutai-labs/delayed-streams-modeling
- Chatterbox: https://github.com/resemble-ai/chatterbox (issues 56, 127; PR 528); https://huggingface.co/ResembleAI/chatterbox-turbo-ONNX ; https://www.resemble.ai/resources/chatterbox-nano-and-flash-speed-at-the-edge-throughput-at-scale ; Perth: https://github.com/resemble-ai/Perth ; GGUF: https://huggingface.co/cstr/chatterbox-turbo-GGUF ; CrispASR: https://github.com/CrispStrobe/CrispASR ; NVIDIA NVIGI Chatterbox card: https://docs.nvidia.com/ace-for-games/chatterbox-tts/nvigi-chatterbox-model-card.html
- Kokoro: https://huggingface.co/hexgrad/Kokoro-82M ; https://github.com/thewh1teagle/kokoro-onnx ; KVoiceWalk: https://github.com/RobViren/kvoicewalk
- Others: https://huggingface.co/SWivid/F5-TTS ; https://github.com/myshell-ai/OpenVoice ; https://github.com/fishaudio/fish-speech ; https://github.com/index-tts/index-tts ; https://github.com/canopyai/Orpheus-TTS ; https://huggingface.co/sesame/csm-1b ; https://github.com/nari-labs/dia ; https://github.com/nari-labs/dia2 ; https://github.com/microsoft/VibeVoice (`docs/vibevoice-realtime-0.5b.md`) ; https://github.com/boson-ai/higgs-audio ; https://github.com/Zyphra/Zonos ; https://huggingface.co/coqui/XTTS-v2 ; https://github.com/idiap/coqui-ai-TTS ; https://github.com/OHF-Voice/piper1-gpl ; https://github.com/neuphonic/neutts ; https://github.com/QwenLM/Qwen3-TTS ; https://github.com/OpenBMB/VoxCPM ; https://huggingface.co/openbmb/VoxCPM2 ; https://github.com/k2-fsa/OmniVoice ; https://github.com/ysharma3501/LuxTTS ; https://github.com/k2-fsa/ZipVoice ; https://github.com/supertone-oss-archive/supertonic ; https://github.com/KittenML/KittenTTS ; https://github.com/ekwek1/soprano ; https://huggingface.co/mistralai/Voxtral-4B-TTS-2603 ; AudioSeal: https://github.com/facebookresearch/audioseal

Voices, secondary:
- Picovoice on-device TTS benchmark (vendor-run, 2026-07-14, updated 2026-08-18): https://picovoice.ai/blog/on-device-tts/

Sibling reports in this repository: `2026-10-voice-stack.md`, `2026-10-computer-use.md`, `2026-10-brain-latency.md`, `2026-10-claude-mods.md`, `2026-10-learning-loop.md`, `2026-10-prior-art.md`.
