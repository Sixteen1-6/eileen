# WP4: Voice

The living plan for work package 4 of [BUILD.md](../BUILD.md) section 6.6: audio, speech,
dictation, voices and the first-run recording. The agent doing the work keeps it current.

## Goal

Audio capture, render and loopback with echo cancellation; the speech process (VAD, Nemotron,
end of turn, Kokoro with its espeak-free front end, barge-in windows); the cloning voices (Pocket
TTS and Chatterbox); tier-0 dictation cleanup; consent enrollment and the speaker check; the
first-run calibration; the embedding model; the machine profile's speech part; and `models.toml`
with the speech part of `eileen setup`.

Its first work is slice S1's and comes before everything else, as two short series of small pull
requests (BUILD.md section 6.6, "The slice as two series"), each ending in the branch it names:

- `wp4/s1-audio`: capture, render and loopback, AEC, mute, the ring buffer, playback accounting,
  the energy barge-in fallback, and the core end of the speech pipe (every frame the core sends
  or receives, over the transport WP2 builds behind `SpeechTransport`).
- `wp4/s1-speech`: the speech process's end of the pipe, VAD, Nemotron, end of turn, Kokoro with
  its front end, barge-in windows and the transcript-injection stub.

`PlayPhrase` with the phrase cache, and `Record` for the first run and enrollment, wait for W2.

## Branch, dependencies and files

- Branch: `wp4/<topic>`; the slice's series are `wp4/s1-audio-<part>` and
  `wp4/s1-speech-<part>`, and the last part of each is `wp4/s1-audio` or `wp4/s1-speech`, the
  branches slice S1 waits on.
- Depends on: WP0 phase A.
- Wave: W1 for the two slice series, W2 for the rest, which starts when both have merged.
- Desktop: yes (audio devices). Device tests take `desktop.lock` and never run during WP6's
  spike; speech-model tests and benchmarks take both build slots (`--exclusive`). Model tests
  live in `owner_pc::` modules until WP0 phase B gives the nightly lane's model run a module of
  its own.
- Owns: `crates/eileen-audio`, `crates/eileen-audio-win`, `crates/eileen-speech` (including
  `models.toml`), `crates/eileen-dictation`, `scenarios/voice/`, `scenarios/dictation/`, the fuzz
  targets `speech_frame` and `dictation_cleanup` with their corpora and regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.6, items 1 to 14, each recorded under Progress with its
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

Every pull request also meets the rules section 6.1 sets for every package.

## The slice series

The current split, in merge order. Parts may merge or split as the code shows, every part stays
under section 6.1's size, and the last of each series keeps the named branch. A part that changes
`eileen-contracts` is an amendment to WP0's crate.

1. `wp4/s1-speech-codec` (amendment to `eileen_contracts::speech` and `core_io`): the per-frame
   flags (replayed, a break before it, Eileen's playback sounding, other audio playing), how the
   speech process presents its token, how a PCM frame's JSON header ends, the cap on command and
   event bodies, and the doc comments of `FrameHeader::flags` and `AudioCmd::Duck` (Eileen's own
   playback), with their schema snapshots and the `speech_frame` fuzz target over the codec.
2. `wp4/s1-audio-ring`: the SPSC ring under `cfg(loom)`, and `SpeechTransport` with an in-memory
   transport for tests, so WP2 can build its side from here.
3. `wp4/s1-audio-pipe`: the component's frame I/O: frames from the ring with their flags, the
   replay at start-up and after a restart, PCM into playback, the heartbeat, `Input::Voice`, and
   `EffectFamily::Speech` beside `Audio`, tested over the in-memory transport with `WavSource`.
4. `wp4/s1-speech-stub`: the speech process's end of the pipe and the transcript-injection stub
   (`eileen-speech --stub <file>`), so the slice's CI flow runs with no models.
5. `wp4/s1-audio-dsp`: resampling (`rubato` or an own polyphase filter, by measurement), sonora's
   AEC3 with the loopback reference and the PCM fallback, and the other-audio signal.
6. `wp4/s1-audio-devices`: cpal capture, render and loopback with `eileen-audio-win`'s
   endpoints, MMCSS and the Communications option, and mute that stops the stream.
7. `wp4/s1-speech-vad`, `wp4/s1-speech-asr`, `wp4/s1-speech-turn` and `wp4/s1-speech-tts`:
   Silero VAD with turn IDs and the speech level, Nemotron fed only during speech, Smart Turn
   fused with the continuation cues and the stretched cap, and Kokoro with its espeak-free front
   end, clause chunking, synthesis ahead of playback, pause and resume. Model tests in
   `owner_pc::`.
8. `wp4/s1-audio`: playback accounting, duck, pause, resume and stop, earcons, the energy barge-in
   fallback, `RoomMixer` and `CaptureSink`, and the audio component wired whole.
9. `wp4/s1-speech`: barge-in windows (the stricter threshold under other audio, none on a replayed
   frame) and the slice's end-to-end stub scenario.

## Progress

- 2026-10-31: the amendment `amend/wp4-slice-plan` lands the slice as the two series above and
  gives the core end of the speech pipe to `eileen-audio` (BUILD.md sections 0, 3.3.3, 6.1, 6.4
  and 6.6). No code yet.

## Surprises

None yet.

## Decisions

- The core end of the speech pipe is this package's, in `eileen-audio`'s component; WP2 owns only
  the transport (the pipe, its ACL, the token, the client PID check, the supervised spawn and
  restart), behind a std-only `SpeechTransport` trait this package defines (BUILD.md 6.6).
- The speech process connects only after its models load, so the ring's replay covers start-up as
  it covers a restart; the ring holds at least one model load's worth of audio.
- `AudioCmd::Duck` lowers Eileen's own playback, never other apps' audio (invariant 2).

## Outcome

Open.
