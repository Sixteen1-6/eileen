//! The CLI's stdio control protocol (sections 2.5 and 3.3.8): `control_request`,
//! `control_response` and `control_cancel_request` lines, which travel on the same stdio as
//! stream-json. Eileen sends one request, the interrupt, through [`Requests`], which numbers it
//! and matches the CLI's reply to it by `request_id`. A `can_use_tool` request is the approval
//! route WP3's contract test recorded as the fallback (`--permission-prompt-tool stdio`); v0.1
//! never answers one, and parsing it lets a session that sends one be noticed. The protocol is
//! the Agent SDK's own, not a documented interface, so the interrupt is feature-detected from
//! `system/init` before it is sent.

use eileen_contracts::ids::Untrusted;
use eileen_trace::Redactor;
use serde_json::Value;

use super::{
    Init, MAX_KIND_BYTES, MAX_LINE_BYTES, cut, redact_settled, redacted_input, str_at, text,
};

/// The `system/init` capability Eileen requires before it sends an interrupt, taken to mean that
/// the CLI acknowledges an interrupt with a `control_response`, which [`Requests::settle`]
/// matches. The name is chosen from the three interrupt capabilities CLI 2.1.304's `system/init`
/// lists and is not yet verified: acceptance 3's owner-run check (BUILD.md section 6.5) confirms
/// both the name and the reply.
pub const INTERRUPT_CAPABILITY: &str = "interrupt_receipt_v1";

/// The most control requests Eileen keeps open on one session. A session runs one turn at a
/// time, so more than a few open interrupts means the CLI stopped answering; the driver's
/// deadline closes each one ([`Requests::abandon`]), or the next turn's start closes them all
/// ([`Requests::abandon_all`]).
pub const MAX_OPEN_REQUESTS: usize = 4;

/// The longest `request_id` a control line may carry. The CLI's are UUIDs and Eileen's are
/// `eileen_<n>`; a longer one is malformed.
pub const MAX_REQUEST_ID_BYTES: usize = 128;

/// One control line.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A request to stop the turn in progress (what Eileen sends).
    Interrupt {
        /// The sender's id for the request.
        request_id: String,
    },
    /// The CLI asks whether a tool may run.
    CanUseTool(Box<CanUseTool>),
    /// A request of another subtype.
    Request {
        /// The sender's id for the request.
        request_id: String,
        /// Its subtype, at most [`MAX_KIND_BYTES`].
        subtype: String,
    },
    /// The answer to a request.
    Response {
        /// The request it answers.
        request_id: String,
        /// The error, when the request failed.
        error: Option<Untrusted<String>>,
    },
    /// The sender withdrew a request it had made.
    Cancel {
        /// The request withdrawn.
        request_id: String,
    },
}

/// A `can_use_tool` request.
#[derive(Debug, Clone, PartialEq)]
pub struct CanUseTool {
    /// The CLI's id for the request.
    pub request_id: String,
    /// The tool.
    pub tool_name: String,
    /// The vendor's id for the call, the one the guard saw.
    pub tool_use_id: Option<String>,
    /// The call's input, redacted, at most [`MAX_TOOL_INPUT_BYTES`](super::MAX_TOOL_INPUT_BYTES).
    pub input: Untrusted<Value>,
    /// Why the CLI asks: the guard's reason, for a guard's `ask`.
    pub reason: Option<Untrusted<String>>,
}

/// Parses one control line.
pub fn parse(line: &[u8], redactor: &Redactor) -> Result<Message, &'static str> {
    if line.len() > MAX_LINE_BYTES {
        return Err("the line is longer than MAX_LINE_BYTES");
    }
    let value = serde_json::from_slice::<Value>(line).map_err(|_| "the line is not JSON")?;
    from_value(&value, redactor)
}

/// The interrupt request line for `request_id`, without its line ending. It checks neither the
/// session's capabilities nor [`MAX_OPEN_REQUESTS`] and records nothing as open, so it stays
/// inside the crate: [`Requests::interrupt`] is the only public way to write one.
#[must_use]
pub(crate) fn interrupt(request_id: &str) -> String {
    serde_json::json!({
        "type": "control_request",
        "request_id": request_id,
        "request": { "subtype": "interrupt" },
    })
    .to_string()
}

/// A control line already parsed as JSON.
pub(crate) fn from_value(value: &Value, redactor: &Redactor) -> Result<Message, &'static str> {
    match str_at(value, "type") {
        "control_request" => request(value, redactor),
        "control_response" => response(value, redactor),
        "control_cancel_request" => Ok(Message::Cancel { request_id: request_id(value)? }),
        _ => Err("the line is not a control message"),
    }
}

/// A `control_request`.
fn request(value: &Value, redactor: &Redactor) -> Result<Message, &'static str> {
    let request_id = request_id(value)?;
    let request = value.get("request").ok_or("a control_request without a request")?;
    let subtype = request
        .get("subtype")
        .and_then(Value::as_str)
        .ok_or("a control_request without a subtype")?;
    match subtype {
        "interrupt" => Ok(Message::Interrupt { request_id }),
        "can_use_tool" => {
            let tool_name =
                text(request, "tool_name").ok_or("a can_use_tool request without a tool_name")?;
            let input =
                request.get("input").map_or(Value::Null, |input| redacted_input(input, redactor));
            Ok(Message::CanUseTool(Box::new(CanUseTool {
                request_id,
                tool_name,
                tool_use_id: text(request, "tool_use_id"),
                input: Untrusted::new(input),
                reason: text(request, "decision_reason")
                    .map(|reason| Untrusted::new(redact_settled(&reason, redactor))),
            })))
        }
        other => {
            Ok(Message::Request { request_id, subtype: cut(other, MAX_KIND_BYTES).to_owned() })
        }
    }
}

/// A `control_response`: `success` or `error`, with the id of the request it answers.
fn response(value: &Value, redactor: &Redactor) -> Result<Message, &'static str> {
    let response = value.get("response").ok_or("a control_response without a response")?;
    let request_id = request_id(response)?;
    match str_at(response, "subtype") {
        "success" => Ok(Message::Response { request_id, error: None }),
        "error" => {
            let error = redact_settled(&text(response, "error").unwrap_or_default(), redactor);
            Ok(Message::Response { request_id, error: Some(Untrusted::new(error)) })
        }
        _ => Err("a control_response that is neither success nor error"),
    }
}

/// The `request_id`, which must be a string that is not empty and at most
/// [`MAX_REQUEST_ID_BYTES`].
fn request_id(value: &Value) -> Result<String, &'static str> {
    let id = value
        .get("request_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or("a control message without a request_id")?;
    if id.len() > MAX_REQUEST_ID_BYTES {
        return Err("a request_id longer than MAX_REQUEST_ID_BYTES");
    }
    Ok(id.to_owned())
}

/// Why an interrupt was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// The session's `system/init` does not list [`INTERRUPT_CAPABILITY`].
    #[error("the CLI's system/init does not offer {}", INTERRUPT_CAPABILITY)]
    NotOffered,
    /// [`MAX_OPEN_REQUESTS`] requests are open and unanswered.
    #[error("{} control requests are open and unanswered", MAX_OPEN_REQUESTS)]
    Full,
}

/// Why a `control_response` settled nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettleError {
    /// The response names a request that is not open: one Eileen never sent, one already
    /// answered, or one the driver abandoned at its deadline. It is a protocol fault, not an
    /// ordinary line.
    #[error("a control_response to a request that is not open")]
    NotOpen,
}

/// The CLI's answer to a request Eileen sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The request it answers.
    pub request_id: String,
    /// The error, when the request failed.
    pub error: Option<Untrusted<String>>,
}

/// The control requests Eileen has written to one session and not yet seen answered. Ids are
/// `eileen_<n>`, numbered from 1 for each session, so no clock or randomness is needed and a
/// replay writes the same lines.
#[derive(Debug, Default)]
pub struct Requests {
    /// The number of the last request written.
    sent: u64,
    /// The ids written and not yet answered or abandoned, oldest first.
    open: Vec<String>,
}

impl Requests {
    /// The interrupt line to write to the session's stdin, without its line ending, when the
    /// session's `init` offers [`INTERRUPT_CAPABILITY`] and fewer than [`MAX_OPEN_REQUESTS`]
    /// requests are open. Its id stays open until [`Self::settle`] or [`Self::abandon`].
    pub fn interrupt(&mut self, init: &Init) -> Result<String, SendError> {
        if !init.offers(INTERRUPT_CAPABILITY) {
            return Err(SendError::NotOffered);
        }
        if self.open.len() >= MAX_OPEN_REQUESTS {
            return Err(SendError::Full);
        }
        self.sent = self.sent.saturating_add(1);
        let request_id = format!("eileen_{}", self.sent);
        let line = interrupt(&request_id);
        self.open.push(request_id);
        Ok(line)
    }

    /// Settles the open request a [`Message::Response`] answers, given that response's
    /// `request_id` and `error`, and closes it. A response to a request that is not open
    /// matches nothing and leaves every open request open.
    pub fn settle(
        &mut self,
        request_id: &str,
        error: Option<Untrusted<String>>,
    ) -> Result<Reply, SettleError> {
        let index =
            self.open.iter().position(|open| open == request_id).ok_or(SettleError::NotOpen)?;
        let request_id = self.open.remove(index);
        Ok(Reply { request_id, error })
    }

    /// Closes a request the CLI did not answer before the driver's deadline; true when it was
    /// open.
    pub fn abandon(&mut self, request_id: &str) -> bool {
        let before = self.open.len();
        self.open.retain(|open| open != request_id);
        self.open.len() < before
    }

    /// Closes every open request, when their replies are no longer coming (the next turn has
    /// started), and returns how many there were. Numbering goes on, so a late reply to one of
    /// them settles nothing.
    pub fn abandon_all(&mut self) -> usize {
        let open = self.open.len();
        self.open.clear();
        open
    }

    /// The number of requests open.
    #[must_use]
    pub const fn open(&self) -> usize {
        self.open.len()
    }
}

#[cfg(test)]
mod tests;
