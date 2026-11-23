//! Writing a recording's input lane: one [`RecordLine`] a line, long strings moved into the blob
//! store. The recorder in `eileen-runtime` decides what reaches it (unaddressed speech already
//! replaced by its placeholder); the writer keeps what it is given exactly, so the reader gives
//! back the line that was written.

use std::io::{ErrorKind, Write};

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
    /// The output took part of a line and then failed, so the next line starts on a line of
    /// its own.
    cut: bool,
}

impl<W: Write, B: Blobs> Writer<W, B> {
    /// A writer to `out` that keeps strings over [`MAX_INLINE_BYTES`] in `blobs`.
    #[must_use]
    pub const fn new(out: W, blobs: B) -> Self {
        Self { out, blobs, max_inline: MAX_INLINE_BYTES, lines: 0, cut: false }
    }

    /// The same writer with another inline limit, in bytes.
    #[must_use]
    pub const fn with_max_inline(mut self, max_inline: usize) -> Self {
        self.max_inline = max_inline;
        self
    }

    /// Writes one line and its newline. The line's blobs are stored first, so a crash part way
    /// leaves a line whose blobs are all there, or a cut line whose complete stubs
    /// [`release_blobs`](crate::reader::release_blobs) finds and releases. When the output
    /// fails part way, the references of the stubs it did not take are released here, and
    /// those it took are left for `release_blobs`, so none is released twice.
    pub fn write(&mut self, record: &RecordLine) -> Result<(), WriteError> {
        let mut text = line::encode(record, self.max_inline, &mut self.blobs)?;
        text.push('\n');
        if self.cut {
            // Ends the part of a line a failed write left, so the reader reports that line and
            // reads this one whole.
            text.insert(0, '\n');
        }
        let bytes = text.as_bytes();
        let mut taken = 0;
        while let Some(rest) = bytes.get(taken..).filter(|rest| !rest.is_empty()) {
            match self.out.write(rest) {
                Ok(0) => return Err(self.failed(bytes, taken, ErrorKind::WriteZero.into())),
                Ok(n) => taken += n,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(self.failed(bytes, taken, error)),
            }
        }
        self.cut = false;
        self.lines = self.lines.saturating_add(1);
        Ok(())
    }

    /// Releases the references of the stubs past the first `taken` bytes of a line the output
    /// failed to take whole: those are in no file, and the ones before are left to whoever
    /// releases the recording.
    fn failed(&mut self, line: &[u8], taken: usize, error: std::io::Error) -> WriteError {
        let whole = line.strip_suffix(b"\n").unwrap_or(line);
        let mut unwritten = line::stub_refs(whole).unwrap_or_default();
        for blob in line::scan_refs(line.get(..taken).unwrap_or_default()) {
            if let Some(at) = unwritten.iter().position(|left| *left == blob) {
                unwritten.swap_remove(at);
            }
        }
        line::release_all(&mut self.blobs, &unwritten);
        // Cut when the output took bytes of this line past the newline that ends an earlier one.
        let lead = usize::from(self.cut);
        self.cut = taken > lead || (self.cut && taken == 0);
        WriteError::Io(error)
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
