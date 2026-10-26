//! The `eileen-sim` binary; its guidance is the crate doc of the library.

use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let code =
        eileen_sim::cli::main(std::env::args_os().skip(1), &mut io::stdout(), &mut io::stderr());
    ExitCode::from(code)
}
