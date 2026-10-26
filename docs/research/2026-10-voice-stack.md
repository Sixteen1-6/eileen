# Always-on local voice stack on this PC, October 2026

This report picks the v0.1 voice pipeline for Eileen's Rust core: capture, echo cancellation, mute, voice activity, streaming speech recognition, end of turn, addressee detection, speech synthesis and barge-in, all local, open and free.

**Where the numbers come from.** Almost every measurement here was made by two earlier research attempts on the owner's PC whose sessions ended before they could write up: attempt `af0133077ffea93d8` on **2026-10-11** (stopped by a usage limit) and attempt `a4973575530370a19` on **2026-10-20** (stopped when the PC ran out of memory). Their commands and results are condensed in `%USERPROFILE%\Projects\eileen-workfiles\predecessor-notes\voice-stack.md`, and their scripts and result files are in `%USERPROFILE%\Projects\eileen-workfiles\2026-10-scratch\voice\`. This write-up (2026-10-20) added one light measurement, the idle cost of listening, and re-ran the read-only Windows effects probe. Each number below carries the date it was measured.

**Read every number as pessimistic.** The PC is a Core Ultra 7 155H (6 P-cores, 8 E-cores, 2 LP E-cores, 22 logical processors), 16 GB RAM, Arc iGPU, Intel AI Boost NPU, no NVIDIA GPU, on the Balanced power plan. During nearly every run, other research agents and Rust builds held the CPU at 55 to 75 percent and left 0.5 to 2 GB of RAM free; one run saw 9,000 pages per second of paging and the CPU clock at 1.4 GHz of 3.8 GHz. Latencies on a quiet machine will be lower, and some results (TTS especially) moved by a factor of two between runs.

**Test audio is synthetic.** The speech corpus is ten Eileen-style commands (41.3 s at 16 kHz, `wavs/session.json`) synthesized with Kokoro voices, not recorded from a person. Accuracy figures are therefore a smoke test, not a benchmark, and the Smart Turn result in particular needs a re-test on real speech.

## 1. Recommendation: the v0.1 pipeline

One Rust process owns the microphone and speaker. Everything below runs on the CPU through a single `onnxruntime.dll` (1.28.2) shipped with sherpa-onnx.

| Stage | Choice | Crate or library | Model file | Measured on this PC |
|---|---|---|---|---|
| Audio I/O | WASAPI shared mode, 48 kHz, 10 ms buffers | `cpal` 0.18.2, `windows` 0.62 for endpoint and category control | none | Output callback every 10.1 ms of audio; a stop flag reached the callback 1.5 ms after it was set (2026-10-20) |
| Mute | Global hotkey and HUD button that stop the capture stream itself, so nothing is read while muted | `windows` (RegisterHotKey) or `global-hotkey` | none | not measured |
| Resample | 48 kHz stereo to 16 kHz mono | sherpa-onnx resampler or `rubato` | none | not measured |
| Echo cancellation | WebRTC AEC3 + noise suppression + high-pass, reference is the exact PCM Eileen plays | `sonora` 0.2.0 (pure Rust, BSD-3-Clause) | none | ERLE 33.3 dB on the synthetic echo test, 0 leaked words, user command fully recovered; 592 us per 10 ms frame p50, 4.4 ms p99 (2026-10-20) |
| Voice activity | Silero VAD v6.2, 32 ms windows, one thread, 300 ms pre-roll | `sherpa-onnx` 1.13.8 `VoiceActivityDetector` | `silero_vad.onnx` v6.2, 2.3 MB | 0.3 to 1.0 ms per window p50; 10 of 10 utterances found; idle listening 5.8 percent of one logical core, +31 MB (2026-10-20) |
| Streaming ASR (partials and finals) | Nemotron speech streaming EN 0.6B, 560 ms chunks, int8, four threads, fed only while VAD reports speech | `sherpa-onnx` `OnlineRecognizer` | `sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25`, 464 MB archive | WER 2.35 percent on the corpus, first partial after 1.3 s of speech, final 0.4 s after speech ends, compute 0.47 s per second of audio, about 740 MB resident (2026-10-11) |
| End of turn | Silero silence of 200 ms or more triggers Smart Turn v3.2 on the last 8 s; combine with transcript cues and a silence cap | `ort` 2.0.0-rc.13 linked to the same `onnxruntime.dll`; Whisper log-mel ported to Rust with `realfft` | `smart-turn-v3.2-cpu.onnx`, 8.7 MB | Model 52 to 71 ms at four threads, features 18 to 30 ms (Python, 2026-10-11); Rust port matches the reference features within 1.1e-5 (2026-10-20); accuracy on synthetic cuts weak, see section 5 |
| Addressee gate | Local rules first: the name "Eileen" in the transcript, a follow-up window after Eileen speaks, the deterministic command grammar; ambiguous speech goes to the HUD, not to action | Eileen code | none (speaker check and keyword spotter later) | Nemotron transcribed "Eileen" correctly; Parakeet heard "Clean" (2026-10-11) |
| TTS | Kokoro v1.0 fp32, voice `af_heart`, four threads, text split at clause boundaries, short first clause, a PCM cache for stock phrases | `sherpa-onnx` `OfflineTts` | `kokoro-multi-lang-v1_0` (fp32, 350 MB archive) | Real-time factor 0.54 on the quietest run, 1.1 to 1.3 under typical load, so a one-second clause takes 0.5 to 1.3 s (2026-10-11 and 2026-10-20) |
| Barge-in | While Eileen speaks, VAD on the echo-cancelled mic with a stricter threshold; duck on the first speech window, stop on the third; cancel synthesis through the generation callback; log the last sample played | Eileen code over the above | none | Detection 64 to 96 ms plus at most one 10 ms buffer, so about 70 to 110 ms (derived from the measured parts) |
| Process setup | Opt out of EcoQoS power throttling at start, raise the audio threads with MMCSS | `windows` (SetProcessInformation, AvSetMmThreadCharacteristicsW) | none | Without the opt-out one Kokoro run took 27.8 s instead of 2.4 s (2026-10-11) |

Resident memory with models loaded is about 1.25 GB (VAD 31 MB, Nemotron about 740 MB, Kokoro fp32 about 450 MB, plus Smart Turn, whose footprint was not recorded). Idle CPU while listening is roughly 6 percent of one logical core for VAD, plus up to 6 percent of one core for AEC if it runs continuously, which is under 1 percent of the whole CPU.

Not in the v0.1 path, but ready to add:

- **Dictation mode** (owner direction of 2026-10-20): Parakeet TDT 0.6B v3 int8, loaded on demand as a final pass over each VAD segment. It punctuates, covers 25 European languages, is CC-BY-4.0, and measured a real-time factor of 0.18 to 0.51. It costs about 750 MB while loaded, so unload it after dictation ends. The sherpa-onnx Rust API exposes `create_stream_with_hotwords` for user vocabulary; that path was not tested with Parakeet.
- **Speaker check** to ignore the TV and other people: WeSpeaker CAM++ (29 MB) was downloaded on 2026-10-20 but not evaluated.
- **Name keyword spotter**: the sherpa-onnx GigaSpeech zipformer KWS (3.3M parameters, 17.6 MB archive) was downloaded on 2026-10-20 but not evaluated.
- **Any voice** (owner direction of 2026-10-20): Kyutai Pocket TTS clones a voice from a reference clip and was measured (section 6). Chatterbox was not evaluated by either attempt.

What changed from ROADMAP M3: M3 planned a Pipecat spike and a Python loop on RealtimeSTT, RealtimeTTS and pywebrtc-audio, with Claude Haiku through LiteLLM. The core is now Rust, so Pipecat and the Python libraries are reference designs only; `sonora` replaces pywebrtc-audio; and the brain is the signed-in Claude CLI per the owner's 2026-10-20 direction (see `2026-10-brain-latency.md`). M3's model choices (Silero, Smart Turn, Nemotron, Parakeet, Kokoro) hold up, except that running Nemotron and Parakeet together is too heavy for this PC (section 4).

## 2. What was run

All scripts are in `%USERPROFILE%\Projects\eileen-workfiles\2026-10-scratch\voice\scripts\` and the Rust spike in `...\voice\rust-bench\` and `...\voice\aecprobe\`. Python ran in uv venvs in the scratch directory (`.venv313` with sherpa-onnx 1.13.8 and onnxruntime 1.31.0, `.venv-ov` with OpenVINO 2026.4.1, `.venv-dml` with onnxruntime-directml 1.24.4). Models live in `%LOCALAPPDATA%\eileen\models\{vad,stt,tts,turn}`.

| Script | What it measures | Date |
|---|---|---|
| `tts_corpus.py` | Builds the ten-command corpus with Kokoro, then times Kokoro through sherpa-onnx (time to first audio, real-time factor, RSS) | 2026-10-11, callback fix 2026-10-20 |
| `stt_bench.py` | Silero VAD segmentation, Parakeet finals per segment, a streaming model in 100 ms chunks (WER, per-chunk time, tail flush, first-partial time, RSS) | 2026-10-11 and 2026-10-20 |
| `alt_stt_bench.py` | faster-whisper base.en and Moonshine medium streaming on the same corpus | 2026-10-11 |
| `smart_turn_bench.py`, `whisper_mel.py` | Smart Turn v3.2 on complete utterances and on cuts at 55 percent; a numpy Whisper log-mel port checked against `transformers` (max diff 5e-6) | 2026-10-11 |
| `tts_alt_bench.py` | kokoro-onnx 0.6.1 and Pocket TTS through sherpa-onnx | 2026-10-11, Pocket re-run 2026-10-20 |
| `make_echo_test.py`, `aec_bench.py` | Synthetic echo scenario and five echo cancellers, scored by transcribing the output | 2026-10-11, scoring 2026-10-20 |
| `accel_probe.py`, `kokoro_resize4d.py`, `kokoro_gpu.py`, `kokoro_gpu_static.py` | ORT CPU, DirectML, and OpenVINO CPU, GPU and NPU on Kokoro, Smart Turn and the Parakeet encoder; a Kokoro graph rewrite for the iGPU | 2026-10-11 and 2026-10-20 |
| `qos_probe.py`, `kokoro_threads.py` | Kokoro fp32 with and without EcoQoS and HIGH priority; intra-op threads and spinning | 2026-10-11 and 2026-10-20 |
| `idle_listen.py` | Silero VAD alone, fed 32 ms windows paced at real time from a WAV (no microphone opened), process CPU and RSS | 2026-10-20, this write-up |
| `rust-bench` (`stt`, `tts`, `aec`, `audio`, `mel-check`, `turn`) | The same pipeline from Rust: sherpa-onnx crate, sonora, cpal, ort | 2026-10-20 |
| `aecprobe` | Which capture effects Windows applies to the default communications mic in the Other and Communications categories; initializes but never starts capture, so nothing is recorded | 2026-10-20, re-run in this write-up |

Result files are in `...\voice\wavs\*.json`. The `downloads\` folder with the archives and `manifest.txt` exists only in the original scratch directory (`%USERPROFILE%\AppData\Local\Temp\claude\C--Users-USER-Projects--claude\8b2a7b12-9d74-4cce-a7ac-9d5f3ba7299c\scratchpad\voice\`).

## 3. Audio I/O, echo cancellation and barge-in

### Devices and callbacks (2026-10-20, Rust, cpal 0.18.2)

- Host WASAPI. Output "Speakers (Realtek(R) Audio)": 48 kHz, 2 channels, f32, buffer fixed at 480 frames (10 ms). Input "Microphone Array (Intel Smart Sound Technology for Digital Microphones)": 48 kHz, 2 channels, f32, 480 frames.
- The output callback delivered 10.1 ms of audio per call, and a stop flag set from another thread was seen inside the callback 1.5 ms later. Stopping playback is therefore bounded by one buffer, not by the OS.

### Echo cancellation quality (scenario 2026-10-11, scored 2026-10-20)

`make_echo_test.py` plays three long Kokoro sentences as the far end, passes them through a simulated speaker and room (60 ms system delay, RT60 about 0.3 s, soft clipping, echo at about -6 dB, noise floor at -55 dBFS), and has the user say "Move this window to the left monitor." over the tail at 6.0 s. Each canceller's output was run through VAD and Parakeet.

| Canceller | ERLE in far-only span | Far-end words leaked into the transcript | User words recovered | Cost per 10 ms frame, p50 / p99 |
|---|---|---|---|---|
| none (raw mic) | 0 dB | 22 | 43 percent | n/a |
| `sonora` 0.2.0 AEC3 + NS + HPF (Rust) | 33.3 dB | 0 | 100 percent | 592 us / 4.4 ms |
| livekit 1.1.20 APM (Python) | 36.5 dB | 0 | 100 percent | 357 us / 2.0 ms |
| aec-audio-processing 1.0.1 (Python) | 33.7 dB | 0 | 100 percent | 252 us / 3.4 ms |
| pywebrtc-audio 0.2.0, AEC + NS + HPF + AGC | 25.9 dB | 0 | 86 percent | 281 us / 8.8 ms |
| pywebrtc-audio 0.2.0, AEC only | 20.8 dB | 0 | 86 percent | 282 us / 5.0 ms |

sonora's own statistics reported a 64 ms delay estimate, close to the simulated 60 ms. The Python timings are from 2026-10-11 and the sonora timing from 2026-10-20 under heavier load, so the cost difference is not a fair comparison; all of them use under 6 percent of one core at 16 kHz mono.

Why sonora: it is pure Rust (no C++ toolchain, no FFI), BSD-3-Clause, ports WebRTC M145 with fixes up to M156, and matched the best Python wrappers here. The other Rust options were `webrtc-audio-processing` 2.1.0 (C++ bundle, license field "non-standard", which cargo-deny would flag) and `aec3` 0.4.0 (MIT OR BSD-3-Clause, not tested). sonora does not port upstream's experimental neural residual echo estimator.

### Windows' own echo cancellation (built and run 2026-10-20, re-run for this write-up)

`aecprobe` opened the default communications mic with IAudioClient2 in two stream categories and asked IAudioEffectsManager for its effects:

- `AudioCategory_Other`: noise suppression on, beamforming off, acoustic echo cancellation off; none can be toggled by the app.
- `AudioCategory_Communications`: noise suppression, acoustic echo cancellation and beamforming all on.
- In both, `IAcousticEchoCancellationControl` is available, so an app can choose which render endpoint is the echo reference.

So this PC offers a system AEC that uses the real speaker output as reference, which also covers music and video, not only Eileen's voice. Its quality was not measured (that needs real playback and recording in the room). v0.1 should default to sonora and offer the Communications category as a setting, then compare the two with real audio.

### Barge-in design

1. While Eileen is speaking, run VAD on the echo-cancelled signal with a higher threshold than when idle.
2. On the first speech window (32 ms), duck playback by about 20 dB in the next output callback. On the third consecutive window (96 ms), stop playback and call the TTS generation callback's stop path (in the Rust crate the callback returns `false` to stop; in the Python API it returns 0, which caught the first attempt out, see Pitfalls).
3. Keep a counter of samples actually handed to the device, and map it back to the text chunk being played, so the brain knows what the user heard.

Measured parts give about 70 to 110 ms from speech onset to silence. The VAD window cost had a p99 of 15.7 ms in one throttled run on 2026-10-11, so the EcoQoS opt-out (section 7) is part of meeting the 150 ms target.

## 4. Voice activity and speech recognition

### Silero VAD

- Model: Silero VAD v6.2 from the upstream repository (2,327,524 bytes). The sherpa-onnx release asset `silero_vad.onnx` is the older v4 (643,854 bytes); both work with sherpa-onnx.
- Per 32 ms window: p50 0.32 to 1.0 ms, p99 0.65 to 2.5 ms in boosted runs (2026-10-11 and 2026-10-20); 10 of 10 utterances segmented in every run.
- **Idle listening (2026-10-20, this write-up):** VAD alone, fed real-time-paced windows for 40 s, used 2.31 s of CPU, which is **5.8 percent of one logical core** (about 0.26 percent of the whole CPU), with the system at 73 percent load. RSS went from 40 MB (Python plus sherpa-onnx) to 71 MB after loading the VAD and 75 MB at the end, so the VAD itself costs about **31 MB**. In Rust the process baseline is about 14 MB instead of 40 MB.

### Recognizers compared (corpus WER; latency under the load noted)

| Model | Mode | Corpus WER | Latency | Resident memory | License | Date |
|---|---|---|---|---|---|---|
| Nemotron speech streaming EN 0.6B, 560 ms, int8 | streaming, 4 threads | 2.35 percent | first partial after 1.3 s; per-100 ms chunk p95 280 ms, max 419 ms; tail flush p50 397 ms; compute 0.47 s per audio second | about 740 MB | NVIDIA Open Model License | 2026-10-11 |
| Parakeet TDT 0.6B v3, int8 | per VAD segment, 4 threads | 3.5 percent | 428 to 1,323 ms per segment, RTF 0.18 to 0.35 (boosted, 2026-10-11); 845 to 1,779 ms (2026-10-20, Python); 618 to 2,304 ms, RTF 0.35 to 0.51 (2026-10-20, Rust); 1.9 to 7.1 s unboosted | about 750 MB | CC-BY-4.0 | 2026-10-11, 2026-10-20 |
| Kroko streaming zipformer EN (2025-08-06) | streaming, 2 threads | 8.2 percent | first partial after 1.5 s; chunk p95 104 ms; tail 114 ms; compute 0.075 s per audio second | about 155 MB | see the Banafo Kroko-ASR page; not verified | 2026-10-11 |
| faster-whisper base.en | per segment, 4 threads | 5.9 percent | p50 1,367 ms, max 1,556 ms, nearly flat with length ("Stop." took 1.3 s) | +51 MB | MIT | 2026-10-11 |
| Moonshine medium streaming (moonshine-voice 0.1.5) | per segment and streaming | 3.5 percent | p50 4,142 ms; streaming compute 1.81 s per audio second, slower than real time under that load; no thread setting in 0.1.5 | +54 MB reported (the 269 MB of model files appear to be memory-mapped) | MIT for English models | 2026-10-11 |

Running Nemotron and Parakeet together in one process (2026-10-20, Python) pushed RSS to 1.04 GB after loading and 1.37 GB at the end, and slowed Nemotron's chunk p95 to 577 ms and its tail to 738 ms. In Rust the same pair reached 1.48 GB. Model load times were 9 to 21 s for Parakeet and 10 to 19 s for Nemotron, so the live recognizer must stay resident.

The only Parakeet error that mattered was the name: it transcribed "Eileen, open..." as "Clean, open...". Nemotron and Moonshine both heard "Eileen" (faster-whisper's transcript of that line was not preserved). The rest of the WER difference is number formatting ("1530" against "fifteen thirty").

**Choice for v0.1:** Nemotron 560 ms alone gives partials for the HUD and the brain and a final transcript about 0.4 s after speech ends, which beats running a separate Parakeet final pass. Parakeet stays as the dictation and multilingual model, loaded on demand. Kroko is the fallback if memory is short, subject to its license.

Untested variants worth one run each: Nemotron at 160 ms and 80 ms chunks (lower partial latency; NVIDIA's card lists average WER 7.07 percent at 560 ms, 7.67 percent at 160 ms and 8.43 percent at 80 ms against 6.93 percent at 1.12 s), the multilingual Nemotron 3.5 ASR streaming 0.6B (sherpa-onnx, 2026-06-11), and Parakeet unified EN 0.6B (one model with streaming and offline modes, sherpa-onnx 2026-05).

## 5. End of turn and addressee detection

### Smart Turn v3.2 (2026-10-11 Python, 2026-10-20 Rust)

- Input: the last 8 s of 16 kHz audio as an 80 x 800 Whisper log-mel. Features p50 29 ms (min 18 ms) in numpy; model p50 62 ms, p95 71 ms, min 52 ms at four threads; 110 ms p50 at two threads (ORT 1.31 CPU).
- The Rust port of the log-mel matched the Python reference within 1.09e-5 over 64,000 values. Smart Turn ran through `ort` 2.0.0-rc.13 against the same `onnxruntime.dll` as sherpa-onnx and gave probabilities close to Python's on the same cases (for example 0.080 against 0.10, and 0.166 against 0.17, on two cuts). Its Rust timings were not captured before the session ended.
- Accuracy: 12 of 19 correct. All ten complete utterances scored 0.95 to 0.99 (correct), but only two of nine cuts at 55 percent scored low (0.08 and 0.17); the other seven scored 0.96 to 0.98 and would have ended the turn early. The cuts are abrupt truncations of synthetic speech, which is not what a real mid-sentence pause sounds like, so this needs a re-test with recorded human speech before it is trusted.

**Rule for v0.1:** treat Smart Turn as one vote. End the turn when Smart Turn says complete and the streaming partial does not end on an obvious continuation (a conjunction, an article, a preposition, "and", "the", "to"); wait longer when the partial looks unfinished; end regardless after a silence cap (start at 1.2 s and tune). Start the Nemotron tail flush as soon as silence begins so it overlaps the decision.

### Addressed or not

The owner wants Eileen to know when it is being spoken to without a wake word. The v0.1 gate, in order, all local:

1. The name "Eileen" anywhere in the utterance (Nemotron transcribes it reliably on this corpus).
2. A follow-up window: speech within a few seconds after Eileen spoke or asked a question.
3. A match against the deterministic command grammar (mute, pause, open an app, move a window).
4. Anything else is shown on the HUD as heard but not acted on, and can be passed to the brain for a cheap "is this for me" check.

The CAM++ speaker embedder and the KWS model downloaded on 2026-10-20 are the next step (ignore other voices; catch the name even when ASR misses it).

## 6. Speech synthesis

| Engine | Setup | Time to first audio | Real-time factor | Memory | Date |
|---|---|---|---|---|---|
| Kokoro v1.0 fp32, sherpa-onnx, Python | 4 threads, EcoQoS off, HIGH priority, system 73 percent busy | p50 2.88 s, max 6.32 s per sentence; "Stop." 1.35 s | mean 1.25 | 480 MB loaded, 620 MB after | 2026-10-11 |
| kokoro-onnx 0.6.1 (same fp32 model) | 4 threads, system 56 percent busy | p50 2.23 s; "Stop." 1.29 s | mean 1.13 | +514 MB | 2026-10-11 |
| Kokoro v1.0 fp32 through onnxruntime directly, fixed 60-token input (3.42 s of audio) | 4 threads, spinning on / off | 1.85 s / 2.30 s on the quietest run; 4.18 s / 4.41 s on another | 0.54 to 1.29 | n/a | 2026-10-20 |
| Kokoro v1.0 fp32, sherpa-onnx, Rust | 4 threads, 1 GB free RAM, paging | 4.5 to 13.3 s | 2.47 to 3.17 | 450 MB | 2026-10-20 |
| Kokoro v1.0 int8, sherpa-onnx | 2 threads, EcoQoS not disabled | 20 to 171 s | 13.5 to 42.6 | 281 to 300 MB | 2026-10-11 |
| Pocket TTS int8 (2026-01-26 export), sherpa-onnx | 4 threads, system 9.5 percent busy | p50 2.20 s; streams within a sentence | mean 1.28 | +605 MB | 2026-10-20 |

Notes:

- sherpa-onnx's Kokoro called back once per sentence (one callback per request in these runs), so time to first audio equals the synthesis time of the whole first sentence. Eileen must split replies at clause boundaries, make the first clause short (three to six words), and synthesize the next clause while the current one plays.
- A one-word reply still took about 1.3 s under load. Stock narration ("On it.", "Done.", "Opening Resolve.") should be synthesized once and cached as PCM, which gives zero synthesis latency and is still a real voice, not a filler noise.
- The int8 Kokoro spent 42 percent of its time in `ConvInteger` in an ORT profile (2026-10-11). The attempt switched to fp32; no clean int8 against fp32 comparison under equal load was recorded.
- Spinning threads (ORT's default) cut wall time slightly at four threads but used more CPU (7.1 s against 6.5 s); turn spinning off between utterances.
- Pocket TTS numbers from 2026-10-11 are invalid: the benchmark's callback returned 0, which stops sherpa-onnx after the first chunk, so every clip came out 1.20 s long. Fixed and re-run on 2026-10-20. Kyutai released Pocket TTS v4.0.0 on 2026-10-20 with 18M, 7M and 2M parameter English models and a Rust CLI; those were not tested and are the obvious next TTS experiment.
- Piper VITS en_US lessac medium int8 (21 MB) and high int8 (35 MB) are listed in the sherpa-onnx release but were not benchmarked.

**Choice for v0.1:** Kokoro v1.0 fp32 from the official sherpa-onnx package `kokoro-multi-lang-v1_0` (the attempts patched sherpa-onnx metadata onto kokoro-onnx's `kokoro-v1.0.onnx` with `patch_kokoro_meta.py`; the official package makes that unnecessary), voice `af_heart` (speaker id 3), 54 voices available. Kokoro does not produce filler sounds.

## 7. NPU, iGPU and Windows scheduling

### Acceleration probes

| Model | ORT CPU | OpenVINO CPU | OpenVINO GPU (Arc) | OpenVINO NPU | DirectML |
|---|---|---|---|---|---|
| Smart Turn v3.2 | p50 50.8 ms (2026-10-11) | 138 ms | 39.9 ms with a 59 s first compile (2026-10-11); 18.4 ms with 10.7 s load from cache (2026-10-20) | 20.3 ms, almost no host CPU (2026-10-11); 40.0 ms (2026-10-20) | fails at a Reshape node |
| Kokoro fp32, fixed input | p50 2.84 s (2026-10-11), 3.99 s (2026-10-20) | 5.29 to 6.81 s, worse | compile fails: `linear_onnx` Resize supports only 2-D, 4-D and 5-D tensors | load fails | fails at a ConvTranspose node |

The 2026-10-20 attempt rewrote Kokoro's six 3-D Resize nodes as Unsqueeze, 4-D Resize, Squeeze (`derived\kokoro-v1.0-fp32-resize4d.onnx`). Output correlation with the original was 0.998 (the original is deterministic run to run), and OpenVINO CPU ran the rewritten graph at a static 60-token shape (best 6.1 s, RTF 1.59). The iGPU still failed, with both dynamic and static shapes, on a MatMul shape check (`dimension 9 doesn't match 1`). No iGPU or NPU path for Kokoro exists today without deeper graph surgery. A Parakeet encoder probe was started on 2026-10-11 but its result was not recorded.

Conclusion: v0.1 is CPU only. Moving Smart Turn to the NPU through OpenVINO (or the onnxruntime OpenVINO execution provider) is a later saving of about 50 ms of CPU per end-of-turn check; it is not worth a second runtime in v0.1.

### EcoQoS and priority (Kokoro fp32, four threads, 3.42 s of audio)

| Setting | 2026-10-11, run 2 | 2026-10-20, 4 threads | 2026-10-20, 8 threads |
|---|---|---|---|
| Default | 27.8 s (0.7 cores used) | 5.4 s | 4.9 s |
| EcoQoS off (`SetProcessInformation`, ProcessPowerThrottling, EXECUTION_SPEED) | 2.4 s (3.9 cores) | 4.9 s | 4.7 s |
| EcoQoS off and HIGH priority class | 3.2 s | 5.2 s | 4.3 s |

When Windows treats the process as background it can apply EcoQoS (efficient cores, low clocks), and inference became more than ten times slower. Eileen's voice process must opt out of execution-speed throttling at start. HIGH priority did not help consistently; use MMCSS for the audio threads instead and leave the process at normal or above-normal priority. A script to map logical processors to P-cores and E-cores (`cpu_sets.py`) was written on 2026-10-20 but never run; pinning inference to P-cores is an open item.

## 8. onnxruntime: how it is linked and shipped

- **sherpa-onnx crate 1.13.8** (Apache-2.0, published by the sherpa-onnx author, Rust API covering VAD, online and offline ASR, TTS, KWS, speaker embedding and a resampler). Its `sherpa-onnx-sys` build script downloads a prebuilt archive from the sherpa-onnx GitHub release at build time and caches it under `target\sherpa-onnx-prebuilt\`. Two environment variables avoid the download: `SHERPA_ONNX_ARCHIVE_DIR` (a folder holding the expected archive) and `SHERPA_ONNX_LIB_DIR` (an extracted lib folder).
- **Static linking failed** (2026-10-20): the default `static` feature fetches `sherpa-onnx-v1.13.8-win-x64-static-MT-Release-lib.tar.bz2` (123 MB, with a 944 MB `onnxruntime.lib`). With VS 2022 17.12 (MSVC 14.42.34433) the link failed on unresolved STL symbols such as `__std_find_last_of_trivial_pos_1`, `__std_remove_8` and `__std_search_2`, which the prebuilt expects from a newer MSVC STL.
- **Shared linking works** (2026-10-20): `sherpa-onnx = { version = "1.13.8", default-features = false, features = ["shared"] }` fetches `sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib.tar.bz2` (8,032,957 bytes, SHA256 `b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7`). Release build took 6 min 8 s under load.
- **One onnxruntime for both crates:** `ort` 2.0.0-rc.13 (built for ONNX Runtime 1.28) built with `ORT_LIB_LOCATION` pointing at the sherpa-onnx shared lib folder and `ORT_PREFER_DYNAMIC_LINK=1` links the same `onnxruntime.dll`. `dumpbin /dependents` showed the exe importing only system DLLs plus `onnxruntime.dll` and `sherpa-onnx-c-api.dll`, and `sherpa-onnx-c-api.dll` importing only `onnxruntime.dll`, KERNEL32 and ADVAPI32.
- **No VC++ redistributable:** the sherpa-onnx "MT" builds use the static CRT, and `.cargo\config.toml` sets `rustflags = ["-C", "target-feature=+crt-static"]` for `x86_64-pc-windows-msvc`, so nothing depends on vcruntime or msvcp.
- **What ships next to `eileen.exe`:** `onnxruntime.dll` 1.28.2 (17,799,168 bytes), `onnxruntime_providers_shared.dll` (104,960 bytes), `sherpa-onnx-c-api.dll` (4,605,952 bytes). `sherpa-onnx-cxx-api.dll` (259,584 bytes) is in the archive but not imported by the Rust build. The spike exe was 2.56 MB with LTO thin and stripped symbols.
- **Recommended setup:** commit a pinned archive name and SHA256, have CI fetch it into a cache directory, verify the hash, and point `SHERPA_ONNX_ARCHIVE_DIR` at it, so builds never download unverified binaries. Pin `ort` to `=2.0.0-rc.13`; rc.14 (2026-10-12) and rc.15 (2026-10-19) are newer than the two-week rule allows. Updating VS 2022 to a current 17.x and retrying the static archive is an option if a single-file exe matters later.
- **ort detail:** in rc.13, errors from `Session::builder()` calls carry the builder and are not `Send + Sync`, so `?` into `anyhow` fails; map them to strings.

## 9. Latency budget from the end of the user's speech

| Step | Measured | Source |
|---|---|---|
| Silence before the end-of-turn check | 200 ms (setting) | design |
| Smart Turn features and model | about 70 to 100 ms at four threads | 2026-10-11 |
| Nemotron final (tail flush, overlaps the two rows above) | about 400 ms alone, 740 ms with Parakeet also loaded | 2026-10-11, 2026-10-20 |
| Brain, warm turn, first text | 1.0 to 1.5 s quiet, 2.2 to 4.2 s busy | `2026-10-brain-latency.md`, 2026-10-20 |
| TTS first clause (about one second of audio) | 0.5 to 1.3 s; 0 for cached phrases | 2026-10-11, 2026-10-20 |
| Playback start | one 10 ms buffer | 2026-10-20 |

That is roughly 0.5 s to the first word for a deterministic command with a cached reply, about 1.5 s for a deterministic command with a synthesized reply, and 2 to 3.5 s for a brain turn on a quiet machine. TTS is the largest local cost, which is why clause chunking, the phrase cache and the next TTS experiments matter most.

## 10. Model manifest

SHA256 values are from the attempts' `downloads\manifest.txt` (computed after download) or from the GitHub and Hugging Face APIs. Where only a prefix was recorded, fetch the full digest from the release API when pinning.

| File | URL | Size | SHA256 | License |
|---|---|---|---|---|
| Silero VAD v6.2 `silero_vad.onnx` | https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx | 2,327,524 B | `1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3` | MIT |
| Silero VAD v4 (sherpa-onnx asset) | https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx | 643,854 B | `9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6` | MIT |
| Smart Turn v3.2 CPU | https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/main/smart-turn-v3.2-cpu.onnx | 8,679,182 B | `2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f` | BSD-2-Clause |
| Nemotron streaming EN 0.6B 560 ms int8 | https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25.tar.bz2 | 463,945,051 B (633 MB extracted) | `78e2b79fcf7271553a74402a76b771b09ea40117a39566a79f52235b23db6358` | NVIDIA Open Model License |
| Nemotron streaming EN 0.6B 160 ms int8 (untested) | same release, `...-160ms-int8-2026-04-25.tar.bz2` | 463.9 MB | prefix `0ae73a41cd515` | NVIDIA Open Model License |
| Parakeet TDT 0.6B v3 int8 | https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2 | 487,170,055 B (641 MB extracted) | `5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf` | CC-BY-4.0 |
| Kroko streaming zipformer EN | https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06.tar.bz2 | 57,267,600 B | `c8676e5ff9ac2a85296e53ee0fd4d5fb1db6770e7a7647166eeafe349ade6834` | see https://huggingface.co/Banafo/Kroko-ASR |
| Kokoro v1.0 fp32 (recommended package) | https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2 | 349.9 MB | prefix `c5f7e2d2caf08` | Apache-2.0 (model) |
| Kokoro v1.0 int8 | https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-int8-multi-lang-v1_0.tar.bz2 | 132,303,094 B | `4c3052abaa60943a341f193888cf6abd68787dae6ab8ae5c925a706caa247e4e` | Apache-2.0 (model) |
| Kokoro v1.0 fp32 (kokoro-onnx, used by the attempts) | https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx | 325,532,387 B | `7d5df8ecf7d4b1878015a32686053fd0eebe2bc377234608764cc0ef3636a6c5` | Apache-2.0 |
| Kokoro voices (kokoro-onnx) | https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin | 28,214,398 B | `bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d` | Apache-2.0 |
| Pocket TTS int8 | https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/sherpa-onnx-pocket-tts-int8-2026-01-26.tar.bz2 | 98,336,520 B | `2f3b88823cbbb9bf0b2477ec8ae7b3fec417b3a87b6bb5f256dba66f2ad967cb` | CC-BY-4.0 |
| WeSpeaker CAM++ speaker embedder (not evaluated) | https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_CAM%2B%2B.onnx | 29,292,684 B | `c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef` | see the WeSpeaker repository |
| GigaSpeech KWS zipformer 3.3M (not evaluated) | https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2 | 17,626,723 B | `f170013b4716e41b62b9bfd809687c207cef798ef9bc6534d524e17af9b6561a` | Apache-2.0 (k2-fsa) |
| sherpa-onnx shared prebuilt (build input) | https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib.tar.bz2 | 8,032,957 B | `b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7` | Apache-2.0, bundles third-party code (see risks) |

Eileen should download models on first run into `%LOCALAPPDATA%\eileen\models`, verify each SHA256, and show each model's license before the first download, rather than putting model files in the repository or installer.

## 11. Pitfalls

- **EcoQoS throttling.** A background Windows process can be slowed more than ten times. Opt out at startup and check it in a self-test.
- **The TTS stop callback is inverted between APIs.** In the sherpa-onnx Rust crate the generation callback returns `true` to continue; in Python, returning 0 stops after the first chunk. The first attempt's Pocket TTS numbers were wrong because of this.
- **Static sherpa-onnx on an older MSVC fails to link.** Use the `shared` feature, or update Visual Studio first.
- **Two onnxruntimes.** If `ort` downloads its own runtime while sherpa-onnx ships another, the process loads two different versions. Point `ort` at the sherpa-onnx DLL with `ORT_LIB_LOCATION` and `ORT_PREFER_DYNAMIC_LINK=1`, and keep the DLLs beside the exe. This PC has a different `onnxruntime.dll` in `C:\Windows\System32` (10,675,424 bytes, dated 2026-10-14, checked during this write-up), and a PATH or working-directory mix-up can load that one.
- **Memory.** Nemotron plus Parakeet plus Kokoro is about 2 GB, and the earlier attempt froze the PC by running several model processes and Rust builds together. Keep one ASR model resident and load the dictation model on demand.
- **Load time.** The 0.6B recognizers take 10 to 20 s to load, so they load at startup, never per utterance.
- **Kokoro latency per request.** sherpa-onnx returns a whole sentence at once; without clause chunking the first word of a long sentence arrives several seconds late.
- **Smart Turn alone ends turns early** on unfinished synthetic speech. Combine it with the transcript and a silence cap.
- **Mute must stop capture**, not discard samples, so the Windows mic indicator goes off and nothing is processed while muted.
- **Ducking.** Opening a capture stream in the Communications category can make Windows lower other apps' volume; if that mode is offered, opt the session out of ducking.
- **DirectML is not an accelerator here** for these models: it failed on both Kokoro and Smart Turn.
- **Two-week rule for crates.** `ort` rc.14 and rc.15 appeared on 2026-10-12 and 2026-10-19; stay on rc.13 until they age.

## 12. Open risks and follow-ups

1. **Real-speech evaluation.** Every accuracy figure comes from synthetic speech. Record a short consented set of the owner's own commands, half-sentences, pauses and side conversations, and re-test VAD, Nemotron, Smart Turn and the addressee gate.
2. **Room echo test.** sonora and the Windows Communications AEC have only been compared on simulated echo. Test both with the laptop speakers at normal volume, plus music or a video playing, and decide whether a WASAPI loopback reference is needed so system audio is cancelled too.
3. **License of the sherpa-onnx binaries.** The prebuilt sherpa-onnx libraries list `espeak-ng` and `piper_phonemize` among their components, and Kokoro's package ships `espeak-ng-data`. espeak-ng is GPL-3.0-or-later. If `sherpa-onnx-c-api.dll` contains it, Eileen's release binaries would carry GPL obligations even though Eileen's source is MIT. This was inferred from the archive's library list and has not been verified. Check before the first binary release, and decide between accepting GPL for release binaries or a TTS front end without espeak-ng.
4. **Nemotron's license** is NVIDIA's Open Model License, which allows commercial use but is not an OSI license. Downloading at first run with the license shown is the mitigation; Parakeet (CC-BY-4.0) or Kroko are the alternatives if the owner wants only open licenses.
5. **TTS speed and voice choice.** Test Pocket TTS v4 small models, Piper, Kokoro int8 under equal load, and Chatterbox for the owner's "any voice" direction, with voice cloning limited to voices the user has the right to use and watermarking left on.
6. **P-core placement.** Run `cpu_sets.py` and test whether pinning inference threads to P-cores with CPU sets reduces the run-to-run swings seen here.
7. **Smart Turn in Rust and on the NPU.** Capture Rust Smart Turn timings; decide later whether the NPU's roughly 20 to 40 ms with no host CPU justifies OpenVINO.
8. **Speaker check and keyword spotting.** Evaluate the downloaded CAM++ and KWS models for ignoring other voices and catching "Eileen".
9. **Pipecat and the Python stacks** (pipecat-ai 1.13.0, RealtimeSTT, RealtimeTTS) were not run as pipelines. Pipecat's audio filters page lists only noise filters (AIC, Koala, Krisp, RNNoise), no echo cancellation. With a Rust core they are design references, not dependencies.
10. **Future microphones.** Phone, earbuds and room devices (owner direction of 2026-10-20) should enter as connectors that deliver 16 kHz frames into the same VAD, ASR and gate. Each needs its own echo reference or must be treated as a headset, and room-sensing devices stay opt-in per device.

## Sources

- sherpa-onnx: https://github.com/k2-fsa/sherpa-onnx (v1.13.8, 2026-09-10); Rust crate https://crates.io/crates/sherpa-onnx; model releases https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models and https://github.com/k2-fsa/sherpa-onnx/releases/tag/tts-models
- Silero VAD: https://github.com/snakers4/silero-vad (v6.2.3, 2026-09-23)
- Smart Turn: https://huggingface.co/pipecat-ai/smart-turn-v3, https://github.com/pipecat-ai/smart-turn, https://www.daily.co/blog/announcing-smart-turn-v3-with-cpu-inference-in-just-12ms/
- Nemotron speech streaming: https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b
- Parakeet TDT 0.6B v3: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3
- Kokoro-82M: https://huggingface.co/hexgrad/Kokoro-82M; kokoro-onnx: https://github.com/thewh1teagle/kokoro-onnx
- Pocket TTS: https://github.com/kyutai-labs/pocket-tts (v4.0.0, 2026-10-20), https://huggingface.co/kyutai/pocket-tts
- Moonshine: https://github.com/moonshine-ai/moonshine, https://arxiv.org/abs/2602.12241
- faster-whisper: https://github.com/SYSTRAN/faster-whisper
- sonora: https://github.com/dignifiedquire/sonora (0.2.0, 2026-07-29); webrtc-audio-processing: https://github.com/tonarino/webrtc-audio-processing; aec3: https://github.com/RubyBit/aec3-rs
- pywebrtc-audio: https://github.com/strands-labs/pywebrtc-audio; LiveKit Python SDK: https://github.com/livekit/python-sdks
- ort: https://github.com/pykeio/ort; cpal: https://github.com/RustAudio/cpal; wasapi: https://github.com/HEnquist/wasapi-rs
- OpenVINO: https://github.com/openvinotoolkit/openvino
- Pipecat: https://github.com/pipecat-ai/pipecat (v1.13.0, 2026-10-19); audio filters: https://reference-server.pipecat.ai/en/latest/api/pipecat.audio.filters.html
- Windows audio effects and AEC control: https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioeffectsmanager, https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iacousticechocancellationcontrol (effect GUIDs from Windows SDK 10.0.22621 `ksmedia.h`)
- Power throttling: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation
- Related Eileen reports: `2026-10-brain-latency.md`, `2026-10-prior-art.md`, `2026-10-subscription-harnesses.md`, `2026-10-app-hooks.md`
