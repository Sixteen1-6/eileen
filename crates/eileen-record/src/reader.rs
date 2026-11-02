//! Reading a recording's input lane (`inputs.jsonl` and the flight recorder's input lane): one
//! [`RecordLine`] a line, its stubs resolved from the blob store. A line is read with a bound,
//! so a file that is not a recording cannot make the reader hold more than [`MAX_LINE_BYTES`].

use std::io::{BufRead, Read};

use eileen_contracts::record::RecordLine;

use crate::blob::{BlobStoreError, Blobs, Released};
use crate::line::{self, LineError, MAX_LINE_BYTES};

/// Why a line of a recording could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The file could not be read.
    #[error("could not read line {line} of the recording")]
    Io {
        /// The line, from 1.
        line: u64,
        /// The error.
        #[source]
        source: std::io::Error,
    },
    /// A line over [`MAX_LINE_BYTES`]; the reader stops there.
    #[error("line {line} of the recording is over the line limit")]
    TooLong {
        /// The line, from 1.
        line: u64,
    },
    /// The last line ends with no newline and does not read: a writer stopped part way.
    #[error("the recording's last line, {line}, was cut off")]
    Truncated {
        /// The line, from 1.
        line: u64,
    },
    /// A line that does not read as a [`RecordLine`].
    #[error("line {line} of the recording does not read")]
    Line {
        /// The line, from 1.
        line: u64,
        /// Why.
        #[source]
        source: LineError,
    },
}

/// One raw line and whether it ended with a newline.
struct Raw {
    bytes: Vec<u8>,
    ended: bool,
}

/// Reads a recording line by line.
#[derive(Debug)]
pub struct Reader<R> {
    input: R,
    line: u64,
    stopped: bool,
}

impl<R: BufRead> Reader<R> {
    /// A reader at the start of `input`.
    #[must_use]
    pub const fn new(input: R) -> Self {
        Self { input, line: 0, stopped: false }
    }

    /// The number of the last line read, from 1.
    #[must_use]
    pub const fn line(&self) -> u64 {
        self.line
    }

    /// The next raw line, its newline (and a carriage return before it) removed, skipping empty
    /// lines; `None` at the end, or after an error that stops the reader.
    fn next_raw(&mut self) -> Option<Result<Raw, ReadError>> {
        loop {
            if self.stopped {
                return None;
            }
            let mut bytes = Vec::new();
            let limit = u64::try_from(MAX_LINE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
            let read = (&mut self.input).take(limit).read_until(b'\n', &mut bytes);
            self.line = self.line.saturating_add(1);
            match read {
                Ok(0) => {
                    self.stopped = true;
                    return None;
                }
                Ok(_) => {}
                Err(source) => {
                    self.stopped = true;
                    return Some(Err(ReadError::Io { line: self.line, source }));
                }
            }
            let ended = bytes.last() == Some(&b'\n');
            if ended {
                bytes.pop();
            }
            if bytes.len() > MAX_LINE_BYTES {
                self.stopped = true;
                return Some(Err(ReadError::TooLong { line: self.line }));
            }
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            if !bytes.is_empty() {
                return Some(Ok(Raw { bytes, ended }));
            }
        }
    }

    /// The next line, its stubs resolved from `blobs`; `None` at the end. A line that does not
    /// read is an error and the reader goes on to the next one, except after an IO error or a
    /// line over the limit, which stop it.
    pub fn next_line<B: Blobs + ?Sized>(
        &mut self,
        blobs: &B,
    ) -> Option<Result<RecordLine, ReadError>> {
        let raw = match self.next_raw()? {
            Ok(raw) => raw,
            Err(error) => return Some(Err(error)),
        };
        Some(line::decode(&raw.bytes, blobs).map_err(|source| {
            if raw.ended {
                ReadError::Line { line: self.line, source }
            } else {
                ReadError::Truncated { line: self.line }
            }
        }))
    }

    /// Every line to the end, or the first error.
    pub fn read_all<B: Blobs + ?Sized>(mut self, blobs: &B) -> Result<Vec<RecordLine>, ReadError> {
        let mut lines = Vec::new();
        while let Some(line) = self.next_line(blobs) {
            lines.push(line?);
        }
        Ok(lines)
    }
}

/// What releasing a recording's blobs did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReleaseReport {
    /// References released.
    pub released: u64,
    /// Blobs deleted with their last reference.
    pub deleted: u64,
    /// Stubs naming a blob the store no longer held.
    pub absent: u64,
}

/// Why a recording's blobs could not be released.
#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    /// A line could not be read.
    #[error("could not read the recording")]
    Read(#[from] ReadError),
    /// The store failed.
    #[error("the blob store failed")]
    Store(#[from] BlobStoreError),
}

/// Releases every reference a recording's lines hold, once for each stub, as deleting the
/// recording does: a blob no other recording, incident or trajectory refers to is deleted. The
/// lines are not parsed, so a recording written by an older version is released too.
pub fn release_blobs<R, B>(input: R, blobs: &mut B) -> Result<ReleaseReport, ReleaseError>
where
    R: BufRead,
    B: Blobs + ?Sized,
{
    let mut reader = Reader::new(input);
    let mut report = ReleaseReport::default();
    while let Some(raw) = reader.next_raw() {
        let raw = raw?;
        let refs = match line::stub_refs(&raw.bytes) {
            Ok(refs) => refs,
            // A cut-off last line holds no whole stub to release.
            Err(_) if !raw.ended => break,
            Err(source) => return Err(ReadError::Line { line: reader.line(), source }.into()),
        };
        for blob in refs {
            report.released = report.released.saturating_add(1);
            match blobs.release(&blob)? {
                Released::Kept(_) => {}
                Released::Deleted => report.deleted = report.deleted.saturating_add(1),
                Released::Absent => report.absent = report.absent.saturating_add(1),
            }
        }
    }
    Ok(report)
}
