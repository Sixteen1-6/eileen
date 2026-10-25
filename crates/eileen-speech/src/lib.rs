//! Eileen's speech process (BUILD.md sections 2.8, 3.3.7 and 4.2): voice activity, streaming
//! recognition, end of turn, synthesis with the phrase cache, voices and cloning, the speaker
//! check, the embedding model, and the speech pipe to the core. A lib and the `eileen-speech`
//! binary. A stub until WP4 builds it; the stub binary says it is not built yet.
//!
//! # Owns
//!
//! - The speech pipe of section 3.3.7 (length checked before any allocation), the plug-in
//!   traits `VoiceActivity`, `Recognizer`, `TurnDetector` and `Synth`, and every engine behind
//!   them, through `sherpa-onnx` and `ort` under the Windows target table.
//! - `models.toml`, `eileen-speech setup`, `bench`, `voice ...` and the transcript-injection
//!   stub mode ([`cli`]).
//! - The `speech_frame` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Audio devices and echo cancellation (`eileen-audio`), the core's turn decisions
//!   (`eileen-core`), or dictation cleanup (`eileen-dictation`).
//!
//! # Invariants
//!
//! - The shipped `eileen` binary never links ONNX Runtime: `eileen voice` and `eileen setup`
//!   run this binary through the supervisor (section 3.1).
//! - At start it checks that the loaded `onnxruntime.dll` sits beside the executable and reports
//!   1.28.2, and exits with a clear error otherwise.
//! - No watermark stage runs with default settings.
//! - No GPL component in a release build (owner answer 12).
//!
//! # Boundaries
//!
//! - Called by the core process through the pipe; started by `eileen-runtime` through the
//!   supervisor.
//! - May depend on `eileen-supervisor`, `eileen-rt`, `eileen-contracts`, `eileen-stdx` and
//!   `eileen-trace`; `eileen-audio-win`, `sherpa-onnx` and `ort` under the Windows target table.
//! - Wire types: the speech pipe's frames and JSON bodies (`eileen-contracts::speech`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-speech` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-speech`).
//! - Layers: neutral unit tests on Linux (pipe protocol, turn fusion, log-mel, clause splitter,
//!   phrase cache), the `speech_frame` fuzz target, and model tests under the exclusive build
//!   slot.
//! - A bug here starts as a turn that ended early, a misheard name, or a frame over its maximum.
//!
//! # Traps
//!
//! - `System32` holds an older `onnxruntime.dll` the loader searches first; `ort`'s
//!   `copy-dylibs` puts the right one beside every test binary.
//! - Model tests and benchmarks take the exclusive build slot (section 6.1 rule 2).
//!
//! # Logs and tracing
//!
//! - None yet. WP4's spans are `speech.segment`, `turn.detect` and `tts.synth` from
//!   `eileen_trace::names`, each taking its remote parent from the pipe's `TraceCtx`.
//!
//! # Do not change without a decision record
//!
//! - The pipe's frame header, the model pins in `models.toml`, and the espeak-free build.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;
