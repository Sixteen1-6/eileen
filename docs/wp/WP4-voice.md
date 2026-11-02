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

The amendments before the first part:

- #127 (`amend/wp4-slice-plan`, merged 2026-10-31): the slice as two series, and one owner for
  each part of the speech pipe (decision D81).
- #132 (`amend/wp4-pins`, merged 2026-11-01): pins `sha2` 0.11.1 for the model downloads' SHA256
  and `allocation-counter` 0.8.1, a dev-dependency of `eileen-audio` only, for the callbacks'
  allocation check in test builds (decision D82), and sets the bar the resampler is measured
  against, with `rubato` not pinned unless the own filter misses it.

The parts of the audio series, in merge order, as each opens:

1. `amend/speech-pipe-codec` (#136, merged 2026-11-01): the first part, an amendment to
   WP0's `eileen_contracts::speech`. It writes the frame codec both ends of the pipe use and
   D81's meanings: `FrameKind::Hello` (kind 7, at most 256 bytes), `AudioFlags` (replayed,
   break, playback, other audio, bits 0 to 3), the `seq` and `turn` meanings on `FrameHeader`,
   `SpeechStarted.replayed`, and the doc comments of `AudioCmd::Duck` and
   `AudioCmd::SetCapture`. It settles the two framing details WP0 left open: a PCM frame's JSON
   header ends at its first newline, within 256 bytes, and command and event bodies are at most
   256 KiB. `Frame` and `FrameReader` read and write every kind, and the reader reserves a body
   only after its header passed. A reader is made for one end (`for_core`,
   `for_core_after_hello`, `for_speech`) and refuses a kind its peer may not send, any frame
   before the speech process's `Hello` and a second `Hello`. `Frame::encode_audio_into` writes
   an audio frame from a borrowed window, so the ring needs no copy. The `Hello`'s token is a
   `SessionToken`, like the dispatcher pipe's. Evidence: the unit and property tests in
   `eileen_contracts::speech::codec`.
   The `speech_frame` fuzz entry stays a stub in this part, since the target is `eileen-speech`'s
   and this part changes only the contracts. Whichever WP4 part lands first after it, from either
   series, points `eileen_speech::fuzz_entry::speech_frame` at `FrameReader` (all three readers,
   reporting `Err` when one takes more than it was given, needs more than a PCM body, or returns
   a different error after its first), with a seed corpus. The entry stays
   in `eileen-speech`, as BUILD.md section 4.7's table gives the target to WP4, though the parser
   is in `eileen-contracts`; `fuzz/AGENTS.md` says "the neutral crate that owns the parser", and
   pull request #136 proposes the wording under "Suggested guidance changes".
2. `wp4/s1-audio-transport` (#140, merged 2026-11-01): defines what WP2's
   `wp2/speech-transport` builds against, so it merges early. In `eileen-audio`: the `SpeechTransport` trait (`accept` a verified connection
   within a deadline, `close` one the component is done with), with std types only, as section
   3.3.15's ports use; `SpeechConnection`, a byte duplex over two std-only byte traits (`ByteRead`,
   `ByteWrite`) whose read and write halves give every read and write a deadline, and whose writer
   refuses every write after one fails part way; the in-memory transport under the `fake` feature
   (`fake::memory_transport`), whose `StandIns` side connects a stand-in late, kills it by dropping
   its end, connects a new one and sees each connection handed back, with every queue bounded;
   `AudioSource`, `AudioSink` and `AudioStream`, callback-driven as cpal's streams are, each
   callback told when its buffer was captured or will play on the source's or sink's own clock; and
   `AudioConfig::speech`, the transport the later parts read. Evidence: the unit tests in
   `eileen_audio::fake::tests` and `eileen_audio::transport::tests`.
3. `wp4/s1-audio-ring` (#145, merged 2026-11-01): the speech ring, crate-private in `eileen-audio`
   until the component takes it. `CaptureWriter`, the capture callback's end, numbers each
   512-sample frame (`seq` from 1, counted from the core's start and never restarting) and hands it,
   with its capture time and capture flags, over an `rtrb` SPSC ring; it allocates nothing.
   `SpeechRing`, the reader's end, moves frames into a replay store of at most 180 s (5,625 frames,
   allocated once) that keeps every frame the speech process has not confirmed, confirms through the
   `seq` of a `Pong` with no turn open or of the frame that carried an `EndOfTurn`, replays from the
   oldest unconfirmed frame on each new connection, drops the oldest on overrun, sets the break and
   replayed flags, and empties on a mute. Evidence: the unit tests in `eileen_audio::ring::tests`
   (each rule, WP4 acceptance 15's overrun and mute pieces at the unit layer, and the push inside
   `allocation_counter::measure`) and the loom tests in `eileen_audio::ring::concurrency`, which run
   once on a real thread without `--cfg loom`.
4. `wp4/s1-audio-resample` (draft): the resamplers, crate-private in `eileen-audio`
   (`resample`) until the component takes them. One polyphase engine, a Kaiser-windowed sinc
   split into phases, serves four conversions: capture from 48 kHz to 16 kHz (the 3:1
   decimator, 128 taps) and from 44.1 kHz (up 160, down 441, 120 taps per phase), and 24 kHz
   synthesis to 48 kHz (the 1:2 interpolator, 32 taps per phase) and to 44.1 kHz (up 147, down
   80, 32 taps per phase); and the stereo-to-mono capture downmix. Each filter is designed for
   80 dB when the resampler is made, and the callbacks' path allocates nothing. Evidence: the
   unit tests in `eileen_audio::resample::tests`, which measure every conversion through
   `realfft` against the bar (on Linux and Windows CI), check that buffers of any size give the
   same samples as one call and that every 10 ms buffer gives 10 ms, and run the downmix and
   all four conversions inside `allocation_counter::measure`. The cost bar is the `owner_pc::`
   benchmark in the same module, still to be run on the owner's PC; its result goes on the pull
   request's `owner-run:` line and here.

The speech series has not opened yet.

## Surprises

- `assert_no_alloc`, the observability report's reference design, is BSD-1-Clause, which
  `deny.toml`'s allow list lacks, and the allocator the report proposed writing instead needs the
  unsafe code `eileen-audio` forbids. `allocation-counter` 0.8.1 (MIT/Apache-2.0) does the same
  job with no license exception (D82).
- A dev-dependency that declares its own `#[global_allocator]` reaches only the test binaries whose
  code names it: rustc links a crate passed with `--extern` only when code uses it. So the check
  cannot run in product debug builds without a feature in `eileen.exe`; D82 checks the real
  callbacks in an `owner_pc::` device test instead.

- rtrb's newest release, 0.4.1, came out on 2026-10-25, inside the 14-day cooling-off period
  when the ring took the crate, so the lock holds 0.4.0, the release section 4.1 named.
- No CI lane builds with `--cfg loom` yet (section 4.6 plans one for pull requests that change
  `eileen-audio`), so the ring's loom tests are written to run once on a real thread in every
  other build: CI compiles and runs their bodies, and the loom exploration runs where the flag is
  set.

- realfft's newest release, 3.5.1, came out on 2026-10-26, inside the cooling-off period, so the
  lock holds 3.5.0, the release section 4.1 names.

- `AudioConfig` was a unit struct, which `eileen-cli`'s wiring named as a value. Giving it its
  first field changed that one call to `AudioConfig::default()`, a line in WP2's crate.

## Decisions

- D81: the audio component owns every frame of the speech pipe, WP2 the transport, and the core
  the speech process's life.
- D82: the callbacks' code runs inside `allocation_counter::measure` in tests, which assert an
  all-zero `AllocationInfo`, and an `owner_pc::` device test does the same on the real callbacks,
  passing the counts out through atomics. Product builds carry no check. `deny.toml` lets only
  `eileen-audio` take the crate, and check 15 keeps it under `[dev-dependencies]` once WP0's
  phase B builds it; until then the `bans` entry holds it to `eileen-audio`, whose manifest WP4
  owns.
- The speech connection: the transport supplies two std-only byte traits and `eileen-audio`
  wraps them in `SpeechConnection`, whose halves add the deadlines with tokio's timer. So every
  transport gets the same deadlines, the bytes go straight from the ring to the pipe with no pump
  task between them, and the trait never names a tokio type. A read must be cancel safe, so a
  read that misses its deadline loses nothing; a write that fails can leave half a frame, so the
  writer then refuses every write until the connection goes back.
- `SpeechTransport::accept` takes a deadline, as rule 7 asks of every wait on another process,
  and returns `AcceptError::TimedOut` when it passes, with the transport still usable. The core's
  120 s load deadline lives in another process's state machine, so it cannot stand in for one at
  this layer; the component loops on the timeout and reports there that speech is still not
  connected.
- The capture and render callbacks receive `CaptureInfo { captured_at }` and
  `RenderInfo { plays_at }` from the source's or sink's own clock, as cpal's callback infos do.
  The replayed flag (D81) and playback accounting need those times, and a callback that read a
  clock itself would read the real one on the audio thread, which no test can pace. A test source
  derives them from its sample position, so WP4 acceptance 15 runs in virtual time.
- The ring's break flag follows from the numbers: a frame carries it when its `seq` is not one
  more than the frame written before it on the connection, or, first on a connection, than the
  newest confirmed frame. A frame lost at the full handoff, one dropped from the full store before
  it was written and one written to a connection that ended before confirming it all show as such
  a gap, so one rule covers them. A mute skips no number, so it sets the flag on the next frame by
  hand.
- A confirmation never reaches past the newest frame written on the connection: the speech
  process can only have read what it was sent, and a `seq` beyond that would drop frames it never
  heard.
- The handoff is `rtrb`, whose producer cannot drop the oldest frame, so a full handoff loses the
  newest one. Its number is still used, so the gap shows, and the reader counts the loss. The
  store, on the reader's side, drops its oldest, as section 6.6 asks. A new capture stream gets a
  new handoff, since a stopped stream drops its callback and the producer with it; the next `seq`
  lives in an atomic both sides share, so numbering goes on across streams.
- A frame counts as written once the ring hands it out. A write that fails ends the connection,
  and the next connection replays from the oldest unconfirmed frame, so the ring needs no undo.
- Resampling: Eileen's own polyphase filter, measured against the bar of BUILD.md section 6.6
  (capture flat within 0.1 dB to 7 kHz and 60 dB down from 9 kHz, synthesis flat to 10 kHz with
  its images 60 dB down from 14 kHz, each at most 25 us per 10 ms buffer). Every filter is
  designed for an 80 dB stopband, 20 dB past the bar, and the tests measure each conversion
  against the bar itself, so `rubato` stays unpinned while they pass. One engine serves the
  integer ratios of the 48 kHz endpoint and the rational ratios of a 44.1 kHz one, so the
  44.1 kHz case costs no second implementation. The cost bar is the `owner_pc::` benchmark; if
  it misses, the rubato pin amendment follows, as section 6.6 says.
- A 44.1 kHz endpoint: cpal 0.18.2 opens WASAPI capture streams only at the device's own rates
  (only render streams take `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`), so capture from such an
  endpoint needs Eileen's 160:441 conversion. A render stream could be opened at 48 kHz through
  Windows' converter at its default quality, which CI cannot measure, so the synthesis path
  keeps Eileen's own 147:80 conversion as well; the render part chooses between them.
- The loopback is a capture stream too: cpal opens it as an input stream on the render endpoint
  with `AUDCLNT_STREAMFLAGS_LOOPBACK` and no converter, so a 44.1 kHz endpoint's echo reference
  also arrives at 44.1 kHz and takes the 160:441 conversion, even when playback runs at 48 kHz
  through Windows' converter. BUILD.md sections 2.2 step 1, 2.8 and 6.6 still say capture and
  loopback run at 48 kHz; the pull request for this part proposes the wording under "Suggested
  guidance changes".
- An endpoint mixing at any other rate (32, 88.2 or 96 kHz) gets `UnsupportedRates` from the
  resampler. Until the part of this series that first opens a cpal stream decides what then
  happens (refuse the endpoint with a logged reason, or add the conversions, measured against
  the same bar), such an endpoint is unsupported.

## Outcome

Open.
