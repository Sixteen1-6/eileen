//! Writing a recording's input lane: one [`RecordLine`] a line, long strings moved into the blob
//! store. The recorder in `eileen-runtime` decides what reaches it (unaddressed speech already
//! replaced by its placeholder); the writer keeps what it is given exactly, so the reader gives
//! back the line that was written.

use std::io::Write;

use eileen_contracts::record::RecordLine;

use crate::blob::Blobs;
use crate::line::{self, LineError, MAX_INLINE_BYTES};

/// Why a line could not be written.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// The line could not be encoded or its blobs stored.
    #[error("could not encode the line")]
    Line(#[from] LineError),
    /// The output could not be written.
    #[error("could not write the recording")]
    Io(#[source] std::io::Error),
}

/// Writes a recording line by line.
#[derive(Debug)]
pub struct Writer<W, B> {
    out: W,
    blobs: B,
    max_inline: usize,
    lines: u64,
}

impl<W: Write, B: Blobs> Writer<W, B> {
    /// A writer to `out` that keeps strings over [`MAX_INLINE_BYTES`] in `blobs`.
    pub const fn new(out: W, blobs: B) -> Self {
        Self { out, blobs, max_inline: MAX_INLINE_BYTES, lines: 0 }
    }

    /// The same writer with another inline limit, in bytes.
    #[must_use]
    pub const fn with_max_inline(mut self, max_inline: usize) -> Self {
        self.max_inline = max_inline;
        self
    }

    /// Writes one line and its newline. The line's blobs are stored first, so a crash part way
    /// leaves a line whose blobs are all there, or a cut line the reader reports.
    pub fn write(&mut self, record: &RecordLine) -> Result<(), WriteError> {
        let mut text = line::encode(record, self.max_inline, &mut self.blobs)?;
        text.push('\n');
        self.out.write_all(text.as_bytes()).map_err(WriteError::Io)?;
        self.lines = self.lines.saturating_add(1);
        Ok(())
    }

    /// The number of lines written.
    #[must_use]
    pub const fn lines(&self) -> u64 {
        self.lines
    }

    /// Flushes the output.
    pub fn flush(&mut self) -> Result<(), WriteError> {
        self.out.flush().map_err(WriteError::Io)
    }

    /// Flushes the output and gives back the output and the store.
    pub fn finish(mut self) -> Result<(W, B), WriteError> {
        self.flush()?;
        Ok((self.out, self.blobs))
    }
}
