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
4. `wp4/s1-audio-resample` (#148, merged 2026-11-01): the resamplers, crate-private in `eileen-audio`
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
5. `wp4/s1-audio-sources` (#153, merged 2026-11-01): the test audio of section 4.7, "End to end
   without a sound card", public in `eileen-audio` for `eileen run --audio-source wav:<file>
   --audio-sink capture:<file>` and other crates' tests. `WavClip` holds a WAV file's samples
   (integer up to 32 bits or 32-bit float, read through `hound` 3.5.1, section 4.1's pin, at
   most 20 minutes, 8 channels, 192 kHz and 20 minutes of 48 kHz stereo in samples, so at most
   461 MB);
   `WavSource` captures one as a microphone would, then silence; `CaptureSink` writes what Eileen
   plays to a WAV of 32-bit floats, up to the 1.07 billion samples hound can count; `RoomMixer`
   joins a speaker, a microphone and the speaker's loopback. The speaker plays Eileen's playback
   plus a bed the PC plays (`BedSource::Speakers`, the default), and the loopback delivers
   exactly that, as the system loopback section 2.8 makes AEC3's far-end reference; the
   microphone hears the near-end talker, a bed something else in the room plays
   (`BedSource::Room`), and the speaker's mix delayed and attenuated. All run on `Pace`d virtual
   time: one task per device calls its callbacks once per 10 ms buffer when the tokio clock
   reaches it, and each buffer's time comes from its sample position. Evidence: the unit tests
   in `eileen_audio::wav::tests` (a float WAV round trip through the sink and the source, 8, 16,
   24 and 32-bit integer WAVs at full scale, the 20-minute and sample caps, refused channel
   counts, rates and 64-bit floats, the pacing and a faster pace on paused tokio time, a stop and
   restart, silence after the clip, a pause kept in the capture, silence written after a stop,
   and the capture's budget) and `eileen_audio::room::tests` (an impulse's echo exact to the
   sample at 37 ms and 12 dB down, in mono and in one stereo channel, the loopback in the same
   buffer at full gain, a bed from the speakers in the loopback and its echo, a zero delay, the
   talker and a bed in the room, each stream stopping alone with the last echo arriving after
   the speaker stopped, a room stopped and started again with an echo still due, one past due
   and the bed the speakers played meanwhile, and the setup checks). The tests make their audio;
   no recorded fixture is checked in.
6. `wp4/s1-audio-playback` (#158, merged 2026-11-01): playback, crate-private in `eileen-audio`
   (`playback`) until the component takes it. The `Player`, the component's half, takes PCM
   frames straight from the speech connection, never through `Core::step` (section 3.3.7),
   converts them from 24 kHz with part 4's interpolator (a fresh one per utterance), and queues
   them with a marker per utterance and clause in a bounded queue (30 s of device samples by
   default, which holds the largest PCM frame), refusing and logging a frame that does not fit.
   It serves `Duck`, `Unduck`, `Pause`, `Resume` and `Stop`. The `Renderer`, the render
   callback's half, hands the samples to the device on every channel, moves to a new gain over
   the first millisecond of the next callback, holds a paused utterance at a sample, drops a
   stopped one, and reports `Played` (at most every 50 ms of audio), `Paused`, `Stopped` and
   `Drained` (each time the utterance's queued audio runs out). Evidence: the unit tests in
   `eileen_audio::playback::tests` (each command over a `CaptureSink` on paused tokio time,
   compared bit for bit with the interpolator over the whole clause; a duck exact from the next
   callback; a resume from exactly the paused sample; a paused utterance holding the next; a
   reply in two batches playing both and draining after each; the clause counts of `Played`;
   the refusals and the logged drops, each queue with its own count; and the render callback
   inside `allocation_counter::measure`).
7. `wp4/s1-audio-aec` (#162, merged 2026-11-02): echo cancellation and the energy barge-in fallback,
   crate-private in `eileen-audio` (`aec`, `energy`) until the component takes them.
   `EchoCanceller` wraps sonora 0.2.0 at the speech pipe's 16 kHz mono in 10 ms chunks: AEC3
   with noise suppression and the high-pass filter against the loopback while it sounds,
   against Eileen's own PCM once she has sounded for 50 ms without the loopback or none runs,
   and bypassed (a second pipeline with only the high-pass filter and noise suppression) once
   the reference and her PCM have both been silent for 500 ms. Each chunk yields the `playback`
   and `other_audio` capture flags, whether AEC3 is still learning the room, and whether a NaN
   or infinite input sample was replaced. `WindowAssembler` gathers the chunks into 32 ms
   windows for `EnergyDetector`, which judges them against an adaptive floor, counts nothing
   while AEC3 learns, and reports an onset at the third speech window in a row. Evidence: the
   unit tests in `eileen_audio::aec::tests`, over a 16 kHz `RoomMixer` with synthetic voices,
   music and a speech video made bit for bit the same on every platform, the loopback two
   chunks behind her stream (an ERLE floor of 20 dB against the loopback, against own PCM,
   after the loopback stops or falls silent under her voice once AEC3 has learned her PCM's
   delay afresh, after a NaN and after an idle spell; a near-end command's energy kept alone, over Eileen and over Eileen with each
   bed, with the detector's onset within 250 ms of it; the reference switch exact to the chunk;
   the learning time; the flags; and no onset in ten minutes of Eileen over each bed, in four
   runs of 153 s from a fresh canceller, nor in a minute of each bed alone), and in
   `eileen_audio::energy::tests` (the window assembly, the detector's rules, and both inside
   `allocation_counter::measure`). Acceptance 1's transcript-level check needs the recognizer
   and comes after both series (W2).
8. `wp4/s1-audio-link` (#164, merged 2026-11-07): the connection half of the component.
   `eileen_audio::component` now returns the audio component, which takes the audio and speech
   effect families, captures
   from `AudioConfig::capture` (a `SharedSource`, new) into the ring from its start, and serves
   each connection `AudioConfig::speech` gives: the latest `SetMode` and `SetHotwords` first,
   then the commands held while none was up, then the ring's unconfirmed frames with their flags,
   then live frames. It sends a `Ping` every second and answers the speech process's, reports
   each `Pong` as `Input::Sidecar(SidecarId::Speech, Heartbeat)`, confirms through the ring from
   each `Pong` and `EndOfTurn`, forwards every event as `Input::Voice`, and hands a connection
   back when it breaks or the codec refuses a frame. `SetCapture` stops and starts the capture
   stream, and a mute empties the ring. While no connection is up it answers `Speak` with
   `SpeakDone` and `Check` with `CheckResult` `Unavailable`, and holds other commands (at most
   64). The capture callback (`capture::Framer`) downmixes, converts to 16 kHz with part 4's
   resamplers and cuts 512-sample frames with their capture times, allocating nothing. Evidence:
   the integration tests in `tests/it/speech_pipe.rs` (acceptance 15's late connect, kill,
   reload, latest mode and hotwords, overrun and mute pieces, the heartbeat and confirmation, a
   refused frame, the `Speak` answer while no connection is up, the held commands' maximum of
   64 with the drop logged, and a clean stop logging no warning) and the unit tests in
   `eileen_audio::capture::tests`. The review's follow-up (#173) serves a mute while a write or
   a closing connection's last read waits on the pipe, answers a read's `Ping`s with one `Pong`
   and drains the handoff before every write, logs the link's `channel.full` as running totals
   with the inputs' capacity, and keeps a capture buffer's whole frames when it ends inside a
   device frame; its tests add a write that misses its deadline, a command whose own write
   fails and the transport's shutdown, and #174 amends section 4.6 so CI lints the shipped
   configuration, which tests compile with `fake` on. Still to come in later parts: PCM into
   playback and the playback commands, echo cancellation and the capture flags, the energy
   fallback while no connection is up, `PlayPhrase` and `Record` as `never!`, `Bench` and
   `Providers` to the machine link, which waits for those events in `eileen_contracts::speech`,
   and the core's span id in each speech command frame, whose `span` stays 0 until the plan
   says how the id reaches the component (planned for `amend/wp4-audio-settled`).

The speech series has not opened yet. When it does, `tts.synth` cannot take its remote `speak`
parent from the command frame, since the audio component sends `span` 0 until the plan settles
how the core's span id reaches it; so the series builds no span against that parent before then,
which would fail the span-parent check in every simulation run.

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

- sonora 0.2.0 allocates and frees as it runs AEC3, converged too: about 33 allocations and
  13 KiB a chunk on average on Windows CI (`allocation_counter::measure` in `aec::tests`, over
  converged chunks against the loopback, own PCM and in the bypass), at most 6 KiB live at
  once. So the echo canceller cannot run inside the capture
  callback, which allocates nothing (D82). The callbacks hand their 16 kHz chunks (microphone,
  loopback, own PCM) on, and the component part runs the canceller beside the ring's reader,
  numbering frames after it; the ring's `CaptureWriter` then takes cancelled audio from that
  side rather than from the callback.
- With a -45 dBFS minimum, the energy detector fired on a syllable peak of Eileen's echo 4.2 s
  into a run, at -45 to -40 dBFS for three windows, so the minimum is -40 dBFS, still under a
  voice near the PC. It also fired 2.2 s into Eileen over music, where AEC3 had not converged:
  -26 to -27.5 dBFS at the microphone, -31 to -33.5 dBFS after the canceller. No threshold
  meant for a voice tells that apart, and the fallback runs mostly while the core starts and the
  speech process loads its models, when AEC3 is fresh, so the canceller reports when it is
  still learning the room (until 3 s of sounding reference since it started or since its
  reference changed between the loopback and her PCM), and the detector counts no speech then.
  The false-onset bar is counted from a fresh canceller's first chunk. The cost: while the
  fallback runs and AEC3 learns, a command over sound the PC plays is not heard as barge-in;
  over silence it is, since AEC3 is bypassed there.
- Once the music bed looped every 47 s instead of every 60 s, the ten minutes over it showed one
  more event on Windows CI only (the test audio then came from the platform's `sin` and `exp`,
  so the two runners heard slightly different signals; it is now made from IEEE 754's exactly
  rounded operations alone, a Taylor series for the sine, just-intonation ratios for the scale
  and a per-sample multiplier for the decay, the same bits everywhere): 78.7 s in, AEC3 took only 5.8 to 15 dB off
  for about 200 ms (-22 to -25 dBFS at the microphone, -30 to -40 dBFS after it), and the
  detector fired twice. Two changes answer it. The speakers' mix of Eileen and a bed peaked at
  0.99 of full scale, hotter than a device's mix gets, so the test voices and beds are 6 dB
  quieter (Eileen at -26 dBFS) and the bed tests assert the mix never reaches full scale. And
  the detector counts a window as speech only if the canceller took at most 6 dB from it while
  AEC3 ran, which leaves neither event a run of three windows and which a voice over the echo
  passes. Applied with AEC3 bypassed too, the test held back a command spoken into a silent
  room by 400 ms, as noise suppression took more than 6 dB from its first syllables.

- With the loopback two chunks behind her stream, a handover to her PCM moves AEC3's reference
  20 ms, and AEC3 has to find the new delay: from 500 ms after the switch until it had heard
  3 s of her PCM, the ERLE was 14.9 dB when the loopback stopped and 11.3 dB when it fell
  silent under her voice, under the 20 dB floor, and 61.3 to 62.6 dB after. So a change of
  reference restarts the learning time, in which the fallback counts no speech, and the tests
  hold the floor from its end. Holding her PCM back by the loopback's lag would shorten the
  relearning, but the lag is the endpoint's; that is for the component part to measure on the
  owner's PC.
- The margins each room test had in CI run 41395786735, the same on both runners to 0.01 dB
  but where noted, since the test audio is now the same bits everywhere and only sonora's
  SIMD kernels, chosen by the runner's CPU, can differ. ERLE (floor 20 dB): 32.1 dB for Eileen
  against the loopback over 3 to 12 s, 44.0 (Windows) and 44.1 (Linux) for the music bed,
  61.5 for the video bed, 58.4 against her own PCM, 59.3 in the first second after an idle
  spell, 60.8 after a NaN, and under a command without it 32.0 over Eileen, 48.3 over her and
  the music, 70.0 over her and the video. A command keeps -2.70 dB alone (bound -3 dB, the
  tightest margin), and -0.50, -0.85 and -1.24 dB over Eileen, with the music and with the
  video (bound -6 dB); its onset comes 120, 150, 180 and 210 ms after it starts (bound
  250 ms). The detector's closest approach to a false onset (over every three windows in a
  row, the most any fell short of speech, then the least of that) was 2.6 dB in the music run
  from 450 s, 26.1 to 42.7 dB in the other music runs, 42.5 to 53.2 dB in the video runs,
  11.8 (Windows) and 11.7 dB (Linux) for the music bed alone and 49.7 dB for the video bed
  alone. AEC3 learned the room at chunk 431 of Eileen over the loopback. A runner whose
  arithmetic moved AEC3 would show first in the command alone and the music run from 450 s.
- Each 153 s bed run took 5.3 to 5.9 s on Windows CI and 2.1 to 5.2 s on Linux, inside the
  default profile's 10 s slow mark.
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

- The test sources and sinks pace on the tokio clock, not on `eileen_stdx::clock::ManualClock`.
  The component's speech connections take their deadlines from tokio's timer, so a source on
  another clock would let paused tokio time jump past those deadlines while the source waited;
  on one clock, a test's audio and its deadlines move together, and paused tokio time is the
  manual clock the tests move. A buffer's time on the device's clock comes from its sample
  position, so a busy runtime that runs a buffer late changes nothing a test sees.
- A device's timeline runs on while its stream is stopped, as a room does: a `WavSource`
  started again skips the clip's stretch that played meanwhile, as a muted microphone hears
  none of it, and a `CaptureSink` writes the stretch as silence, so a sample's place in the file
  is its time.
- `RoomMixer` runs its speaker, loopback and microphone in one task, in that order in each
  buffer, so the echo is exact to the sample at any delay, zero included, the three streams
  carry the same times, and every run gives the same samples. All three share one format, the
  room's; a clip in another format is refused rather than converted. A bed defaults to playing
  from the speakers, so it is in the loopback and the echo canceller can remove it, as WP4
  acceptance 1's video bed needs; `BedSource::Room` stands for a television the PC does not
  play. When every stream was stopped and the room starts again, the echo line moves on by what
  the speakers played meanwhile: silence from Eileen and the bed from the speakers.
- `CaptureSink`'s task runs from its first stream until the file is completed and writes
  silence while no stream runs, instead of writing the stopped stretch at the next start: each
  step then writes one buffer, so a restart after a long idle never blocks the runtime. A
  writer thread was weighed and not taken: under paused tokio time the runtime would advance the
  clock while that thread wrote, so its queue would either drop audio or have to block the task
  anyway.
- `AudioError` gains `Setup(SetupError)`, a typed reason: a second stream while one runs, a test
  stream outside a tokio runtime, a clip of partial frames, a clip or capture over its limit, a
  mixer gain that is not a finite non-negative number, or an echo delay over 1 s.
- `eileen-rt`, `eileen-stdx` and `hound` are ordinary dependencies of `eileen-audio` now, since
  the test audio ships for `eileen run`; the `fake` feature no longer needs to turn `eileen-rt` on.

- Playback's `samples_played` counts the clause's own samples, at its `PcmHeader::sample_rate`
  (24 kHz), as `ClauseReady.samples` does, so the core divides one by the other to map the
  samples heard to characters (section 2.8, "What was heard"). The renderer counts device
  samples and scales them by 24 kHz over the device's rate; the interpolator's delay (16 input
  samples, under 1 ms) is not subtracted.
- Commands reach the renderer through atomics, not a command queue: the target gain, and one
  hold (playing, paused or stopped) per live utterance in slot `ordinal % 8`. The renderer reads
  the gain at the start of each callback and a hold at each step through the queue, so a duck
  or a pause given before a callback is in force from its start (one given during a callback
  may take effect within it), and there is no queue of commands to fill. The player gives a
  new utterance an ordinal only within eight of the one the renderer last reached, and writes
  a hold only for an utterance the renderer has not passed, so no write reaches a slot another
  live utterance holds; a new utterance past the limit is refused and logged, and so is an
  early pause past the 16 the player remembers before their audio.
- Each utterance starts the interpolator from silence, and the previous one's last 16 input
  samples (under 1 ms) stay unplayed in the old interpolator's history: speech ends in silence,
  and flushing them would add samples `ClauseReady.samples` does not count.
- Utterances play in the order their audio arrived, each whole: audio of an utterance after a
  later one's began is refused, so a stopped or overtaken utterance never sounds again, and a
  paused utterance with audio left holds those behind it, so Eileen never talks over herself.
  A new utterance past the live limit is refused as `PlayError::Utterances`, not `Full`, since
  room for it comes when the renderer moves on, not as samples play.
- `Drained` keeps the core's meaning (`eileen-core`'s crate doc, and the simulator's device):
  every sample queued so far for the utterance has played. The renderer reports it once each
  time the queue runs dry while the utterance plays, and when a later utterance's audio begins
  if that dry spell was not already reported, and keeps the utterance current, so its next
  batch continues it with the same interpolator. A reply is one `SpeakId` sent as several
  `SpeechCmd::Speak` batches as its text streams, and the speech process answers each batch
  with a `SpeakDone`, so `SpeakDone` ends nothing in playback: the core alone knows when a reply
  is closed, and ends it on the `Drained` after that. The player has no call that marks an
  utterance's last audio. It drops a `Drained` for the newest utterance while samples queued
  after it wait: the renderer ran dry before that audio came, and the core, reading the event
  after the audio's `ClauseReady`, would end a closed reply with its last clause unplayed. The
  next dry spell is reported instead.
- A pause cuts at a sample, with no fade: the duck at the first barge-in window (about 20 dB,
  section 2.8) comes two windows earlier, so the cut is quiet, and "resume from the paused
  sample" stays exact. A duck moves the gain over the first millisecond of frames from the next
  callback, on into later buffers when that one is shorter, so the step does not click, and a
  positive `db` counts as 0, since a duck only lowers.
- A frame that does not fit the queue is dropped whole, never cut, so each marker covers the
  samples it names; the drop is logged as `channel.full`, an existing name, so no name was
  added to `eileen_trace::names`. Each bounded queue or list logs under its own `channel` with
  its own `times` and capacity (`audio.playback` for the samples, `.markers`, `.utterances`,
  `.waiting` and `.reported`). Events the render side cannot queue are counted in an atomic and
  logged by the player when it next reads (`audio.playback.events`).
- `Earcon` stays for a later part: `Player::apply` serves only duck, unduck, pause, resume and
  stop, and reports any other command as not its own.

- Echo cancellation runs at 16 kHz mono, after the capture resampler, as section 2.2 step 1
  orders it: a third of the cost of 48 kHz, and the band the recognizer hears. The loopback
  and Eileen's own PCM take the same conversion to reach it.
- The reference: the loopback while it carries her; Eileen's own PCM once her PCM has sounded
  in five chunks (50 ms) since the loopback last sounded, or while no loopback stream runs;
  bypassed once the reference and her PCM have both been silent for 500 ms. Section 2.2 step 1
  bypasses on 500 ms of loopback silence alone, which, read literally, would bypass AEC3 under
  her voice when the loopback does not carry it (a broken loopback, or the default endpoint
  changed under it); while the loopback works the readings agree, since her voice is in it.
  An earlier rule waited for 500 ms of loopback silence before switching, which left AEC3
  cancelling against silence for half a second while her echo passed whole; 50 ms covers a
  loopback a buffer or two behind her stream, which the room tests now model. A loopback more
  than 40 ms behind her PCM, once placed on the microphone's clock, would hand the reference to
  her PCM and back at each reply after a pause, and each change restarts AEC3's learning time;
  the device part measures the real lag on the owner's PC. The pull request
  proposes a sentence for section 2.2 under "Suggested guidance changes". A chunk is silent
  under -60 dBFS.
- The devices' samples are untrusted: a NaN or infinite sample in the microphone, the loopback
  or her PCM, or one past 24 dB over full scale, becomes silence before sonora or the flags see it, and the report says so, for the
  component part to count and log. AEC3 is never reset, so one bad sample in its filter would
  last for the process.
- The bypass runs a second sonora pipeline (high-pass filter and noise suppression) and leaves
  the AEC3 pipeline untouched, rather than switching AEC3 off in one pipeline with
  `apply_config`: sonora's `Config` warns that changing it during processing resets
  submodules, so AEC3 would learn the room again at the start of every reply and leak echo
  meanwhile. The cost is the second pipeline's
  memory and a noise estimate in each that goes stale while the other runs.
- The `playback` flag is true of its own chunk, as D81 and the contract say ("Eileen's own
  playback was sounding at capture"): her PCM for the chunk is at or over -60 dBFS. Sounding
  means her PCM's energy, not the player having a reply in hand, so the flag drops in her
  pauses. Her echo tail is the canceller's: her PCM stays its reference for 500 ms after she
  falls silent. The `other_audio` flag compares energies summed over 100 ms, not
  samples: on a real endpoint the loopback lags her stream by a buffer or more and may be
  scaled by the stream's volume, so a sample difference would leave her own voice behind. It
  needs more than twice the energy she played (3 dB), turns on after 200 ms and off after 1 s,
  so other audio quieter than her voice shows in her pauses and the hold bridges them.
- The energy fallback works on the 32 ms windows the VAD and the speech frames use. A window
  is speech at 12 dB over an adaptive floor, at least -40 dBFS and, while a converged AEC3
  runs, with at most 6 dB taken by the canceller (a double-talk test: a voice in the room passes AEC3 nearly
  whole, while a window it took more from was mostly echo; bypassed, noise suppression alone
  takes more than that from the first syllables after a silence, so the test is off), with an
  18 dB margin while
  other audio plays (section 2.8 asks barge-in for two VAD windows over its threshold then; the
  fallback's onset already takes three, so it raises its margin instead); the floor starts at the first window, drops to any
  quieter window and rises 5 dB a second, so a steady noise stops counting within seconds. An
  onset is the third speech window in a row, where barge-in pauses playback, once per run. In a
  quiet room the floor sits low under noise suppression, so the -40 dBFS minimum decides; the
  margin decides over a noisy one. The component makes a fresh detector each time the speech
  process disconnects, so no stale floor carries over. `WindowAssembler` builds the windows
  from the canceller's chunks: each pairs the cancelled samples with the microphone's, and
  carries the flags of every chunk it spans, a chunk split between two windows counting in
  both, so the component wires the same code the room tests run. It returns windows and onsets; turning them into the core's barge-in input is the component
  part's, since no contract carries the fallback yet.
- The tests run the room at 16 kHz, so each buffer is one chunk; the resamplers are measured on
  their own. Ten minutes of a bed in one test took 19.5 s and 20.1 s on Windows CI and 14 s on
  Linux, near the default nextest profile's 30 s limit on a loaded PC, so each bed runs as four
  tests of 153 s, starting 150 s apart into Eileen and the bed, each from a fresh canceller. Their audio is
  synthetic speech (harmonic syllables on vowel formants with word gaps), music (plucked
  chords and hi-hat bursts) and a video (a man talking over quiet music), since AEC3's delay
  estimator and noise suppression treat steady tones differently from speech. Eileen's voice
  loops every 60 s, the music every 47 s and the video every 53 s, so ten minutes of her over a
  bed never repeat a stretch. The room delivers the loopback in the microphone's buffer; the
  tests hold it back two chunks, as a real endpoint's loopback lags her stream, so the switch to
  her PCM moves AEC3's reference by 20 ms. A loopback that carries her at another volume is
  tested on the canceller directly. Lining the loopback's buffers up with the microphone's is
  the component's: the canceller takes `None` only while no loopback stream runs.

- The speech link runs in the component's one task. Its pending accept, read and effect are
  owned futures (each takes its half, source or transport handle and gives it back), so they
  live across the loop's turns and none is dropped part way; this also lets a read in flight be
  awaited when a write fails, so the halves rejoin before the connection goes back. A reader
  task per connection was weighed: it would read while a long replay writes, but it needs its
  own channel and a way to hand its half back. Writes go a second of audio at a time instead,
  with the loop reading between batches and sending a due `Ping` before the next, so a speech
  process that reads a long replay slowly still answers a heartbeat each second and the core
  does not count it dead. When a write fails, the read in flight is awaited and the frames it
  brought are still taken, so an `EndOfTurn` or `SpeakDone` sent just before the break reaches
  the core; a command whose write failed is served as one that came with no connection up.
- A mute never waits on the pipe (follow-up #173). Each write, and a closing connection's last
  read, races the pending effect: a mute stops capture at once and drops any `SetCapture` parked
  before it, and any other effect is parked, at most 64, for the loop to serve in order after
  the wait. Before, a slow speech process could hold a mute for a write's 2 s and a read's 1 s,
  with the capture stream and the mic indicator still on, past section 4.7's one-buffer budget
  and simulator invariant 9. Audio goes a frame at a time under one deadline for the batch, so
  after a mute only the frame already begun is finished and the rest of the batch, captured
  before the mute, is dropped with the ring. Handing the connection back once a write outlived
  one buffer was weighed: it would also stop the frames, but the speech process exits when its
  connection closes, so a mute during a slow replay would cost a 10 to 20 s model reload. A
  writer task per connection was weighed too; it frees the loop from writes, but frames already
  queued to it go out after the mute unless the queue is cleared, which is the same race again.
- The ring's handoff holds 128 frames (about 4 s), up from part 3's 64: a write that waits out
  its 2 s deadline and the read awaited after it (up to 1 s) stall the loop that drains the
  handoff, and 2 s of room would lose the very audio the next connection should replay. The
  component drains the handoff before every write and before that read, and one `Pong`
  answers all the `Ping`s of a read (follow-up #173; before, each `Ping` had its own write and
  deadline), so no stretch between two drains is longer than one write's deadline and one
  read's. A test stalls a connection until its write misses the deadline and finds no frame
  lost or flagged on the next.
- A read that misses its one-second deadline is started again, never a failure: silence is the
  core's business, counted as missed heartbeats, and during a reload it counts none until the
  120 s load deadline. A write that misses its two-second deadline ends the connection, as
  the writer can no longer tell where the stream is.
- While no connection is up the ring is drained from the handoff every 10 ms, so the handoff
  never fills; on a connection the same tick writes what the ring holds.
- Commands held while no connection is up go to the next connection after the latest `SetMode`
  and `SetHotwords` and before any audio, and leave the queue only once that write succeeds; a
  full queue refuses the new command, logged as `channel.full` on `audio.speech.held`. The
  link's other `channel.full` events, the ring's drops and the inputs the core's queue refused,
  go in the once-a-second report when they changed, each with its running total as `times`
  and the inputs' with the capacity the queue's last refusal gave, as every other emitter does
  (follow-up #173; before, each gave the count since the last report, and the inputs' had no
  capacity). `Speak`
  and `Check` are never held, since they are answered at once. With no transport, or after it
  has shut down, nothing is held, since no connection will come.
- While capture is off the 10 ms drain is off too: a mute empties the ring and nothing new
  arrives, and a component wired with no microphone (as `eileen-cli` still is) never gets a
  frame. So a muted component, connected or not, wakes once a second for its `Ping`, not a
  hundred times; a new connection's replay runs on the loop's own flag, not on the tick.
- A connection handed back logs at warn, except when the component itself is stopping, which
  is an ordinary end and logs at info, so a clean run and stop logs no warning.
- Inputs leave the component with `seq`, `at` and `mono` zero: only the runtime's input queue
  can give an input its place in the core-wide replay order, and the component reads no clock.
  `eileen-runtime` has no `InputSink` yet; WP2's queue fills them when it takes an input.
- The capture callback takes a buffer in chunks of 480 device frames (10 ms at 48 kHz) into
  scratch buffers allocated once, so a device buffer of any length allocates nothing. It takes
  whole device frames before it chunks, so a buffer that ends inside a device frame loses that
  partial frame only (follow-up #173; before, the downmix refused the last chunk whole). A frame's
  capture time is the buffer's plus its first sample's place in it at 16 kHz; the resampler's
  own delay (under 3 ms) is not subtracted. Every frame carries no capture flag until echo
  cancellation joins.
- The integration tests take the `fake` feature through a dev-dependency of `eileen-audio` on
  itself, so `cargo nextest run --workspace` builds them without a feature flag and no shipped
  binary carries the memory transport. Under resolver 3 that turns `fake` on in every build of
  the workspace that compiles test targets, clippy over `--all-targets` included, so #174
  amends section 4.6 with a clippy run over libraries and binaries alone, the one check of the
  configuration that ships.

## Outcome

Open.
