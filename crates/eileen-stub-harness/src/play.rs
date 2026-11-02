//! Playing a script: stream-json on stdout as the claude CLI writes it, user and control lines
//! read from stdin on a thread of their own, and the control requests the driver sends
//! (`initialize`, `mcp_status` and the interrupt) answered whenever they arrive.

use std::io::{self, BufRead, Read as _, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::time::Duration;

use eileen_contracts::record::StubScriptLine;
use eileen_stdx::clock::{self, Clock as _};
use serde_json::{Value, json};

use crate::{RunError, Script};

/// The longest stdin line read, the same cap the harness puts on the CLI's stdout lines; a
/// longer one ends the run.
pub const MAX_INPUT_LINE_BYTES: usize = 16 * 1024 * 1024;

/// The longest `request_id` a control request may carry, as the harness's parser allows.
pub(crate) const MAX_REQUEST_ID_BYTES: usize = 128;

/// What starts a narration line in assistant text (BUILD.md section 2.5, "Narration").
const NARRATION_PREFIX: &str = "> ";

/// The text of the error the stub answers a control request it does not know with.
const NOT_ANSWERED: &str =
    "the stub harness answers only the initialize, mcp_status and interrupt control requests";

/// How a played script ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// Every line played, and stdin closed after it.
    Played,
    /// Stdin closed while the script waited for a user line, which ends the CLI too.
    InputClosed,
    /// A `Crash` line: exit with this code.
    Crash(u8),
}

/// One line read on stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Input {
    /// A `user` line, which starts a turn.
    User,
    /// A `control_request`.
    Control {
        /// Its id, echoed in the reply.
        request_id: String,
        /// What it asks.
        request: Request,
    },
    /// Another line of the protocol (a `control_response` or a cancel), which the stub ignores.
    Other,
    /// A line that is not the protocol, or a failed read; it ends the run.
    Broken(InputError),
}

/// What a `control_request` asks, by its subtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Request {
    /// `interrupt`.
    Interrupt,
    /// `initialize`, which the driver sends first.
    Initialize,
    /// `mcp_status`, which the driver asks until `eileen` is connected.
    McpStatus,
    /// Any other subtype.
    Other,
}

/// Why a stdin line is not the protocol.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    /// The line is not JSON.
    #[error("a stdin line that is not JSON")]
    NotJson,
    /// The line has no string `type`.
    #[error("a stdin line without a string type")]
    NoType,
    /// A `control_request` whose `request_id` is missing, empty or longer than 128 bytes.
    #[error("a control_request without a request_id of 1 to 128 bytes")]
    BadRequestId,
    /// The line is longer than [`MAX_INPUT_LINE_BYTES`].
    #[error("a stdin line longer than MAX_INPUT_LINE_BYTES")]
    TooLong,
    /// Stdin could not be read. The kind, not the `io::Error`, so the line stays comparable.
    #[error("stdin could not be read: {0}")]
    Read(io::ErrorKind),
}

/// What a stdin line is, or `None` for a blank one.
pub(crate) fn classify(line: &[u8]) -> Option<Input> {
    let line = line.trim_ascii();
    if line.is_empty() {
        return None;
    }
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Some(Input::Broken(InputError::NotJson));
    };
    let input = match value.get("type").and_then(Value::as_str) {
        Some("user") => Input::User,
        Some("control_request") => match value.get("request_id").and_then(Value::as_str) {
            Some(id) if !id.is_empty() && id.len() <= MAX_REQUEST_ID_BYTES => Input::Control {
                request_id: id.to_owned(),
                request: match value.pointer("/request/subtype").and_then(Value::as_str) {
                    Some("interrupt") => Request::Interrupt,
                    Some("initialize") => Request::Initialize,
                    Some("mcp_status") => Request::McpStatus,
                    _ => Request::Other,
                },
            },
            _ => Input::Broken(InputError::BadRequestId),
        },
        Some(_) => Input::Other,
        None => Input::Broken(InputError::NoType),
    };
    Some(input)
}

/// Reads stdin line by line into `queue` until it closes, a line is broken (sent, then the
/// reading stops), or the player is gone. The queue is bounded, so a reader ahead of the player
/// waits for it.
pub(crate) fn read_input(mut reader: impl BufRead, queue: &SyncSender<Input>) {
    let limit = u64::try_from(MAX_INPUT_LINE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    let mut line = Vec::new();
    loop {
        line.clear();
        let input = match reader.by_ref().take(limit).read_until(b'\n', &mut line) {
            Ok(0) => return,
            Ok(_) => {
                let body = line.strip_suffix(b"\n").unwrap_or(&line);
                let body = body.strip_suffix(b"\r").unwrap_or(body);
                if body.len() > MAX_INPUT_LINE_BYTES {
                    Input::Broken(InputError::TooLong)
                } else {
                    match classify(body) {
                        Some(input) => input,
                        None => continue,
                    }
                }
            }
            Err(error) => Input::Broken(InputError::Read(error.kind())),
        };
        let broken = matches!(input, Input::Broken(_));
        if queue.send(input).is_err() || broken {
            return;
        }
    }
}

/// Plays `script` to `out`, serving `input` as it plays: an interrupt is answered between any
/// two lines and during a delay or a wait, and a user line is kept for the next `AwaitUser`.
pub(crate) fn play(
    script: &Script,
    input: &Receiver<Input>,
    out: &mut impl Write,
) -> Result<Ending, RunError> {
    let servers = script
        .steps()
        .find_map(|(_, step, _)| match step {
            StubScriptLine::Event { json } if is_init(json) => json.get("mcp_servers").cloned(),
            _ => None,
        })
        .unwrap_or_else(|| json!([]));
    let mut player = Player { input, out, users: 0, closed: false, session_id: None, servers };
    for (line, step, event) in script.steps() {
        player.drain()?;
        match step {
            StubScriptLine::Event { json } => {
                player.note_session(json);
                // `load` keeps every event's text, so this writes the script's own bytes; the
                // serialised value is only a fallback.
                match event {
                    Some(text) => player.write(text.as_bytes())?,
                    None => player.write(json.to_string().as_bytes())?,
                }
            }
            StubScriptLine::Delay { ms } => player.delay(*ms)?,
            // `load` refused a code outside 0 to 255.
            StubScriptLine::Crash { code } => {
                return Ok(Ending::Crash(u8::try_from(*code).unwrap_or(u8::MAX)));
            }
            StubScriptLine::Malformed { bytes } => player.write(bytes.as_bytes())?,
            StubScriptLine::Narration { line: said } => player.narrate(said)?,
            StubScriptLine::AwaitUser => {
                if !player.await_user()? {
                    return Ok(Ending::InputClosed);
                }
            }
            StubScriptLine::PermissionRequest { .. } => {
                return Err(RunError::NotPlayed { line });
            }
        }
    }
    player.finish()?;
    Ok(Ending::Played)
}

/// The player's state between lines.
struct Player<'a, W> {
    input: &'a Receiver<Input>,
    out: &'a mut W,
    /// User lines read and not yet taken by an `AwaitUser`: the CLI queues a message sent
    /// during a turn for the next one.
    users: usize,
    /// Whether stdin has closed.
    closed: bool,
    /// The session id of the last `system/init` played, for the narration lines.
    session_id: Option<String>,
    /// The `mcp_servers` of the script's first `system/init`, which `mcp_status` reports
    /// before that init is played, as the CLI reports its servers before its first turn.
    servers: Value,
}

/// Whether `event` is a `system/init`.
fn is_init(event: &Value) -> bool {
    event.get("type").and_then(Value::as_str) == Some("system")
        && event.get("subtype").and_then(Value::as_str) == Some("init")
}

impl<W: Write> Player<'_, W> {
    /// Writes one line and flushes it, so the driver sees it at once.
    fn write(&mut self, line: &[u8]) -> Result<(), RunError> {
        self.out
            .write_all(line)
            .and_then(|()| self.out.write_all(b"\n"))
            .and_then(|()| self.out.flush())
            .map_err(RunError::Output)
    }

    /// Keeps the session id of a `system/init` event.
    fn note_session(&mut self, event: &Value) {
        if is_init(event) {
            self.session_id = event.get("session_id").and_then(Value::as_str).map(str::to_owned);
        }
    }

    /// Writes a narration line as the CLI carries one: an assistant message whose text starts
    /// with the narration prefix. A line break inside it starts another narration line.
    fn narrate(&mut self, said: &str) -> Result<(), RunError> {
        let text = said
            .split('\n')
            .map(|part| format!("{NARRATION_PREFIX}{}", part.strip_suffix('\r').unwrap_or(part)))
            .collect::<Vec<_>>()
            .join("\n");
        let event = json!({
            "type": "assistant",
            "message": {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "text", "text": text }],
            },
            "parent_tool_use_id": null,
            "session_id": self.session_id,
        });
        self.write(event.to_string().as_bytes())
    }

    /// Acts on one stdin line. Each reply is the `control_response` envelope Eileen sends the
    /// CLI (`docs/research/fixtures/wp3-approval/stdio-control.json`, `sent_to_cli`): the
    /// interrupt's without an inner `response` object, as no reply the CLI wrote to an
    /// interrupt has been recorded yet; `initialize`'s with an empty one, since the CLI's
    /// holds account details no test needs; `mcp_status`'s with `{"mcpServers": [...]}`, the
    /// servers of the script's first init.
    // FIXME(#5): WP3 acceptance 3's owner-run check records the CLI's own replies to an
    // interrupt and to `mcp_status`; that recording replaces these shapes.
    fn serve(&mut self, input: Input) -> Result<(), RunError> {
        match input {
            Input::User => self.users = self.users.saturating_add(1),
            Input::Control { request_id, request } => {
                let response = match request {
                    Request::Interrupt => json!({ "subtype": "success", "request_id": request_id }),
                    Request::Initialize => {
                        json!({ "subtype": "success", "request_id": request_id, "response": {} })
                    }
                    Request::McpStatus => json!({
                        "subtype": "success",
                        "request_id": request_id,
                        "response": { "mcpServers": self.servers },
                    }),
                    Request::Other => {
                        json!({ "subtype": "error", "request_id": request_id, "error": NOT_ANSWERED })
                    }
                };
                let reply = json!({ "type": "control_response", "response": response });
                self.write(reply.to_string().as_bytes())?;
            }
            Input::Other => {}
            Input::Broken(error) => return Err(error.into()),
        }
        Ok(())
    }

    /// Serves every stdin line already read.
    fn drain(&mut self) -> Result<(), RunError> {
        while !self.closed {
            match self.input.try_recv() {
                Ok(input) => self.serve(input)?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.closed = true,
            }
        }
        Ok(())
    }

    /// Waits for a user line, serving the others; false when stdin closed first.
    fn await_user(&mut self) -> Result<bool, RunError> {
        loop {
            if self.users > 0 {
                self.users -= 1;
                return Ok(true);
            }
            if self.closed {
                return Ok(false);
            }
            match self.input.recv() {
                Ok(input) => self.serve(input)?,
                Err(_) => self.closed = true,
            }
        }
    }

    /// Waits `ms`, serving stdin meanwhile.
    fn delay(&mut self, ms: u32) -> Result<(), RunError> {
        let clock = clock::system();
        let wait = Duration::from_millis(u64::from(ms));
        let start = clock.mono();
        loop {
            let elapsed = Duration::from_nanos(clock.mono().0.saturating_sub(start.0));
            let Some(left) = wait.checked_sub(elapsed).filter(|left| !left.is_zero()) else {
                return Ok(());
            };
            if self.closed {
                eileen_stdx::thread::sleep(left);
                return Ok(());
            }
            match self.input.recv_timeout(left) {
                Ok(input) => self.serve(input)?,
                Err(RecvTimeoutError::Timeout) => return Ok(()),
                Err(RecvTimeoutError::Disconnected) => self.closed = true,
            }
        }
    }

    /// After the last line: serves stdin until it closes, as the CLI waits for its next turn.
    /// A user line now asks for a turn the script does not have, which ends the run.
    fn finish(&mut self) -> Result<(), RunError> {
        loop {
            if self.users > 0 {
                return Err(RunError::ScriptEnded);
            }
            if self.closed {
                return Ok(());
            }
            match self.input.recv() {
                Ok(input) => self.serve(input)?,
                Err(_) => self.closed = true,
            }
        }
    }
}
