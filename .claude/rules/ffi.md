---
paths:
  - "**/ffi.rs"
  - "**/ffi/**"
---

# FFI modules

The unsafe policy of BUILD.md section 4b.2, for the private `ffi` module of a crate whose
`[package.metadata.eileen]` says `unsafe = "ffi"`.

- The module is declared `#[expect(unsafe_code, reason = "...")] mod ffi;`, private, and is the
  only place in the crate with unsafe code.
- One unsafe operation per block, each with a `// SAFETY:` comment saying which precondition holds
  and why. Clippy denies `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block`.
- Expose only safe functions. Convert every `HRESULT` and `BOOL` into the crate's error type here.
- COM calls run on the crate's own apartment thread with a deadline, never on a tokio worker.
- Prefer a crate that owns its unsafe code (process-wrap, tray-icon, cpal) to new unsafe code.
- In prose docs write "safety comment" in words: the word with a colon in a doc comment trips
  `unnecessary_safety_comment`. Never link a private `ffi` module from public docs.
