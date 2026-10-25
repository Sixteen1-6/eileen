# eileen-speech

The speech process: voice activity, recognition, end of turn, synthesis, voices and cloning, the
speaker check and the speech pipe (BUILD.md sections 2.8, 3.3.7 and 4.2). WP4 owns it. Read the
crate doc in `src/lib.rs` first; this file adds the rules only this folder has.

## Why this folder has its own rules

This crate links native libraries (sherpa-onnx and ONNX Runtime) and loads model files of hundreds
of megabytes, so its builds and tests need the right DLLs, the pinned models and, for anything
that loads a model, the whole PC.

## Rules

- `sherpa-onnx` and `ort` sit only under the Windows target table. `cargo xtask fetch-native`
  fetches and verifies the pinned archive, and every cargo run through `cargo xtask` then points
  `SHERPA_ONNX_ARCHIVE_DIR` and `ORT_LIB_LOCATION` at it, so nothing downloads unchecked binaries.
- `System32` holds an older `onnxruntime.dll` that the loader searches first; `ort`'s
  `copy-dylibs` puts the right one beside every test binary, and the binary checks at start that
  the loaded one is the shipped version.
- Models are listed in `models.toml` with URL, size, SHA256, license and residency, downloaded by
  `eileen-speech setup` into the data folder and verified. Never commit a model file.
- Any test or benchmark that loads a speech model takes both build slots (`--exclusive`), in a
  `owner_pc::` module when it needs the owner's PC.
- Consented recordings of the owner's or anyone's voice are never committed; test fixtures are
  synthetic or stock audio with a recorded license.
- No GPL component in a release build (owner answer 12): releases use WP4's build of sherpa-onnx
  without espeak-ng. No watermark stage runs with default settings.
- The neutral logic (the pipe protocol, turn fusion, the log-mel, the clause splitter, the phrase
  cache) is tested on Linux; the pipe checks a frame's length before it allocates.
