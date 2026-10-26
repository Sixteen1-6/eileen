# WP4: Voice

The living plan for work package 4 of [BUILD.md](../BUILD.md) section 6.6: audio, speech,
dictation, voices and the first-run recording. The agent doing the work keeps it current.

## Goal

Audio capture, render and loopback with echo cancellation; the speech process (VAD, Nemotron,
end of turn, Kokoro with its espeak-free front end, barge-in windows); the cloning voices (Pocket
TTS and Chatterbox); tier-0 dictation cleanup; consent enrollment and the speaker check; the
first-run calibration; the embedding model; the machine profile's speech part; and `models.toml`
with the speech part of `eileen setup`.

Its first two pull requests are slice S1's and come before everything else:

- `wp4/s1-audio`: capture, render and loopback, AEC, mute, the ring buffer, playback accounting
  and the energy barge-in fallback.
- `wp4/s1-speech`: the pipe, VAD, Nemotron, end of turn, Kokoro with its front end, barge-in
  windows and the transcript-injection stub.

## Branch, dependencies and files

- Branch: `wp4/<topic>`; the slice's pull requests are `wp4/s1-audio` and `wp4/s1-speech`.
- Depends on: WP0 phase A.
- Wave: W1 for the two slice pull requests, W2 for the rest, which starts when both have merged.
- Desktop: yes (audio devices). Device tests take `desktop.lock` and never run during WP6's
  spike; speech-model tests and benchmarks take both build slots (`--exclusive`).
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

## Progress

Not started.

## Surprises

None yet.

## Decisions

None yet.

## Outcome

Open.
