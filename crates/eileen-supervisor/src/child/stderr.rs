//! Forwarding a child's stderr into `child.stderr` events, one per line.
//!
//! On Linux and other Unix systems tokio reads a child's pipe through its I/O driver, so an
//! async task forwards it. On Windows tokio reads one with a blocking `ReadFile` on the
//! runtime's blocking pool, which holds a pool thread until bytes come or the pipe closes, so a
//! quiet child would hold one for its whole life, and the store's SQLite calls wait on that
//! pool (BUILD.md section 2.1 rule 2). There a named thread of its own reads the pipe instead,
//! and an async task waits for it, so the handle is the same on every OS.

use std::io;

use eileen_trace::names;
use tokio::process::ChildStderr;
use tokio::task::JoinHandle;

use super::MAX_STDERR_LINE;

/// Forwards `stderr` of the child named `child` until the pipe closes or a read fails. The
/// handle ends once every line is logged.
#[cfg(not(windows))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature of Windows' forward, whose thread can fail to start"
)]
pub(super) fn forward(child: &'static str, stderr: ChildStderr) -> io::Result<JoinHandle<()>> {
    Ok(eileen_rt::task::spawn("supervisor.stderr", async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut line = Vec::with_capacity(256);
        while matches!(read_line_capped(&mut reader, &mut line, MAX_STDERR_LINE).await, Ok(true)) {
            log_line(child, &line);
        }
    }))
}

/// Forwards `stderr` of the child named `child` from a thread of its own (see the module doc).
#[cfg(windows)]
pub(super) fn forward(child: &'static str, stderr: ChildStderr) -> io::Result<JoinHandle<()>> {
    forward_on_thread(child, std::fs::File::from(stderr.into_owned_handle()?))
}

/// Forwards `pipe` with blocking reads on a named thread of its own, so no runtime thread waits
/// on it, and returns a task that ends once the thread has logged every line. The thread logs
/// in the caller's span and subscriber. An error starts nothing, and `pipe` is closed.
#[cfg_attr(
    all(not(windows), not(test)),
    expect(dead_code, reason = "Windows' way to forward, which the unit tests run on every OS")
)]
pub(super) fn forward_on_thread<R>(child: &'static str, pipe: R) -> io::Result<JoinHandle<()>>
where
    R: io::Read + Send + 'static,
{
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let (done, finished) = tokio::sync::oneshot::channel::<()>();
    // The thread ends when the pipe closes, so nothing joins it; `finished` says it ended.
    let _reader = eileen_stdx::thread::spawn_named("supervisor.err", move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let _entered = span.enter();
            let mut reader = io::BufReader::new(pipe);
            let mut line = Vec::with_capacity(256);
            while matches!(
                read_line_capped_blocking(&mut reader, &mut line, MAX_STDERR_LINE),
                Ok(true)
            ) {
                log_line(child, &line);
            }
        });
        let _ignored = done.send(());
    })?;
    Ok(eileen_rt::task::spawn("supervisor.stderr", async move {
        let _ignored = finished.await;
    }))
}

/// Logs one forwarded line. The line goes in the `pii.line` field (see `Child::forward_stderr`).
fn log_line(child: &'static str, line: &[u8]) {
    let text = decode_line(line);
    tracing::info!(name: names::CHILD_STDERR, child, "pii.line" = %text);
}

/// Reads one line into `line` (cleared first), keeping at most `cap` bytes of it and
/// discarding the rest up to the newline. False at the end of the stream with nothing read.
#[cfg(not(windows))]
async fn read_line_capped<R>(reader: &mut R, line: &mut Vec<u8>, cap: usize) -> io::Result<bool>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::AsyncBufReadExt as _;

    line.clear();
    let mut read_any = false;
    loop {
        let buffer = match reader.fill_buf().await {
            Ok(buffer) => buffer,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if buffer.is_empty() {
            return Ok(read_any);
        }
        read_any = true;
        let (used, ended) = take_line(buffer, line, cap);
        reader.consume(used);
        if ended {
            return Ok(true);
        }
    }
}

/// [`read_line_capped`] with blocking reads.
#[cfg_attr(
    all(not(windows), not(test)),
    expect(dead_code, reason = "Windows' way to forward, which the unit tests run on every OS")
)]
fn read_line_capped_blocking<R>(reader: &mut R, line: &mut Vec<u8>, cap: usize) -> io::Result<bool>
where
    R: io::BufRead,
{
    line.clear();
    let mut read_any = false;
    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if buffer.is_empty() {
            return Ok(read_any);
        }
        read_any = true;
        let (used, ended) = take_line(buffer, line, cap);
        reader.consume(used);
        if ended {
            return Ok(true);
        }
    }
}

/// Moves the bytes of `buffer` before its first newline into `line`, keeping at most `cap`
/// bytes of the line. Returns how many bytes of `buffer` it used (the newline too) and whether
/// the line ended.
fn take_line(buffer: &[u8], line: &mut Vec<u8>, cap: usize) -> (usize, bool) {
    let (chunk, used, ended) = match buffer.iter().position(|b| *b == b'\n') {
        Some(at) => (buffer.get(..at).unwrap_or_default(), at + 1, true),
        None => (buffer, buffer.len(), false),
    };
    let room = cap.saturating_sub(line.len());
    line.extend_from_slice(chunk.get(..room.min(chunk.len())).unwrap_or_default());
    (used, ended)
}

/// A forwarded line as text: invalid UTF-8 replaced, a trailing `\r` dropped, and cut to
/// [`MAX_STDERR_LINE`] bytes at a character boundary.
fn decode_line(bytes: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(bytes.strip_suffix(b"\r").unwrap_or(bytes)).into_owned();
    if text.len() > MAX_STDERR_LINE {
        let cut = (0..=MAX_STDERR_LINE).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        text.truncate(cut);
    }
    text
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use eileen_trace::test_support;
    use tokio::time::timeout;

    use super::forward_on_thread;
    use crate::tests::{
        EXITS_WITHIN, assert_hostile_lines_forwarded, current_thread, hostile_stderr,
    };

    #[test]
    fn the_thread_forwards_every_line_with_lines_cut() {
        let ((), captured) = test_support::capture(|| {
            current_thread().block_on(async {
                let pipe = std::io::Cursor::new(hostile_stderr());
                let forwarding = forward_on_thread("test.helper", pipe).unwrap();
                timeout(EXITS_WITHIN, forwarding)
                    .await
                    .expect("the thread ended at the end of the stream")
                    .unwrap();
            });
        });
        assert_hostile_lines_forwarded(&captured);
    }

    #[test]
    fn a_quiet_pipe_holds_no_blocking_thread() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let ((), captured) = test_support::capture(|| {
            runtime.block_on(async {
                let (pipe, mut writer) = std::io::pipe().unwrap();
                let forwarding = forward_on_thread("test.helper", pipe).unwrap();
                // Lets the forwarding start before the blocking call queues.
                tokio::task::yield_now().await;
                let ran = timeout(EXITS_WITHIN, eileen_rt::task::spawn_blocking("test.pool", || 7))
                    .await
                    .expect("the quiet pipe held the pool's only thread")
                    .unwrap();
                assert_eq!(ran, 7, "the blocking call ran");
                writeln!(writer, "the last line").unwrap();
                drop(writer);
                timeout(EXITS_WITHIN, forwarding)
                    .await
                    .expect("the thread ended once the pipe closed")
                    .unwrap();
            });
        });
        let lines = crate::tests::forwarded_lines(&captured);
        assert_eq!(lines, ["the last line"], "logged in the caller's subscriber");
    }
}
