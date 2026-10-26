//! The `eileen-desktop` binary, the desktop sidecar; its guidance is the crate doc of the library.

fn main() -> std::process::ExitCode {
    eileen_desktop::cli::main()
}
