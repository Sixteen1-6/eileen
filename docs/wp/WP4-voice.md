# WP4: Voice

The living plan for work package 4 of [BUILD.md](../BUILD.md) section 6.6: audio, speech,
dictation, voices and the first-run recording. The agent doing the work keeps it current.

## Goal

Audio capture, render and loopback with echo cancellation; the speech process (VAD, Nemotron,
end of turn, Kokoro with its espeak-free front end, barge-in windows); the cloning voices (Pocket
TTS and Chatterbox); tier-0 dictation cleanup; consent enrollment and the speaker check; the
first-run calibration; the embedding model; the machine profile's speech part; and `models.toml`
with the speech part of `eileen setup`.

Its first work is slice S1's and comes before everything else, as two series of small pull
requests (BUILD.md section 6.6), each part under section 6.1's size and merged in order. The last
part of each series is the branch named here:

- `wp4/s1-audio` (parts `wp4/s1-audio-<part>`): capture, render and loopback, AEC, mute, the ring
  buffer, playback accounting, the energy barge-in fallback and the core end of the speech pipe
  (decision D81).
- `wp4/s1-speech` (parts `wp4/s1-speech-<part>`): the speech process's end of the pipe, VAD,
  Nemotron, end of turn, Kokoro with its front end, barge-in windows and the transcript-injection
  stub.

## Branch, dependencies and files

- Branch: `wp4/<topic>`; the slice's series end in `wp4/s1-audio` and `wp4/s1-speech`.
- Depends on: WP0 phase A.
- Wave: W1 for the two slice series, W2 for the rest, which starts when both have merged.
- Desktop: yes (audio devices). Device tests take `desktop.lock` and never run during WP6's
  spike; speech-model tests and benchmarks take both build slots (`--exclusive`).
- Owns: `crates/eileen-audio`, `crates/eileen-audio-win`, `crates/eileen-speech` (including
  `models.toml`), `crates/eileen-dictation`, `scenarios/voice/`, `scenarios/dictation/`, the fuzz
  targets `speech_frame` and `dictation_cleanup` with their corpora and regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.6, items 1 to 15, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. Synthetic echo through `RoomMixer`: no far-end words leak, the command is recovered, and false
   barge-ins stay under 1 per 10 minutes of video.
2. Barge-in on the owner's PC with speakers: onset to silence at most 150 ms at p95 over 20
   trials, with the right heard prefix; a backchannel resumes.
3. Idle listening: the speech process under 8 percent of one core and the full stack about
   1.25 GB resident, recorded with the machine state.
4. End of turn: half-finished sentences do not end early on the synthetic set, nor on the
   owner's consented recordings once they exist.
5. Speech timings measured with the exclusive slot: Kokoro, int8 against fp32, Parakeet against
   Nemotron, the embedding round trip and the first command after 30 idle minutes.
6. The owner's head-to-head of Pocket TTS, Chatterbox-Nano and Chatterbox-Turbo, and the engine
   per voice through the settings flow with the `VoiceRealtime` check.
7. Enrollment refuses another speaker's consent clip and an unconsented dropped clip, and
   deleting a voice or declining the reveal leaves no file.
8. Dictation cleanup passes all 79 SpeakoFlow cases and Eileen's negatives, and
   `dictation_cleanup` and `speech_frame` fuzz clean for 10 minutes each.
9. Mute stops capture within one buffer and the microphone indicator goes off; a wrong
   `onnxruntime.dll` stops `eileen-speech` with a clear error.
10. The transcript-injection stub runs end-to-end scenarios with no models or audio device, and
    the neutral speech and audio crates pass on `ubuntu-latest`.
11. No watermark by default, AudioSeal finds the mark only when exports are watermarked, and no
    GPL component or espeak-ng file ships.
12. The first-run calibration on the owner's PC records its measurements, and declining or a
    missed render limit leaves `calibration.toml` and no audio.
13. The speaker check against section 2.8's bar is recorded as a `SpeakerBar`; a check that does
    not pass, or lacks the people or utterances, gates nothing on voice.
14. The machine profile's speech part: setup benchmarks, thread counts, command accuracy,
    recognizer swaps, endpoints and execution providers.
15. The core end of the speech pipe, over an in-memory transport with a scripted stand-in: a late
    connect or a restart recognises each turn once, the latest `SetMode` and `SetHotwords` arrive
    before any audio, a gap sets the break flag, muted frames are never written, and `Bench` and
    `Providers` reach the machine link.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

No code yet. The amendments before the first part:

- #127 (`amend/wp4-slice-plan`, merged 2026-10-31): the slice as two series, and one owner for
  each part of the speech pipe (decision D81).
- #132 (`amend/wp4-pins`, draft): pins `sha2` 0.11.1 for the model downloads' SHA256 and
  `allocation-counter` 0.8.1, a dev-dependency of `eileen-audio` only, for the callbacks'
  allocation check in test builds (decision D82), and sets the bar the resampler is measured
  against, with `rubato` not pinned unless the own filter misses it.

The slice parts are listed here as each opens. None has opened yet.

## Surprises

- `assert_no_alloc`, the observability report's reference design, is BSD-1-Clause, which
  `deny.toml`'s allow list lacks, and the allocator the report proposed writing instead needs the
  unsafe code `eileen-audio` forbids. `allocation-counter` 0.8.1 (MIT/Apache-2.0) does the same
  job with no license exception (D82).
- A dev-dependency that declares its own `#[global_allocator]` reaches only the test binaries whose
  code names it: rustc links a crate passed with `--extern` only when code uses it. So the check
  cannot run in product debug builds without a feature in `eileen.exe`; D82 checks the real
  callbacks in an `owner_pc::` device test instead.

## Decisions

- D81: the audio component owns every frame of the speech pipe, WP2 the transport, and the core
  the speech process's life.
- D82: the callbacks' code runs inside `allocation_counter::measure` in tests, which assert an
  all-zero `AllocationInfo`, and an `owner_pc::` device test does the same on the real callbacks,
  passing the counts out through atomics. Product builds carry no check. `deny.toml` lets only
  `eileen-audio` take the crate, and check 15 keeps it under `[dev-dependencies]` once WP0's
  phase B builds it; until then the `bans` entry holds it to `eileen-audio`, whose manifest WP4
  owns.
- Resampling: the bar of BUILD.md section 6.6 (the decimator flat within 0.1 dB to 7 kHz and
  60 dB down from 9 kHz, the interpolator flat to 10 kHz with its images 60 dB down from 14 kHz,
  each at most 25 us per 10 ms buffer). At the 48 kHz endpoint rate both ratios are integers (3:1
  and 1:2), so the own polyphase filter is measured first, a 44.1 kHz endpoint included; if it
  misses, an amendment pins `rubato` so that it can be measured against the same bar.

## Outcome

Open.
