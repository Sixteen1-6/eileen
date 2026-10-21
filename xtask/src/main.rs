//! The `cargo xtask` binary. Its guidance is the crate doc in `lib.rs`.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    xtask::run(std::env::args_os().skip(1))
}
