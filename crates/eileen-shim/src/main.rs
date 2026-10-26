//! The `eileen-shim` binary; its guidance is the crate doc of the library.

use std::io;

fn main() -> io::Result<()> {
    eileen_shim::serve(io::stdin().lock(), &mut io::stdout().lock())
}
