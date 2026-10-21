//! W3C `traceparent` values, without OpenTelemetry.
//!
//! The trace id is the 32 hex digits of the turn ULID (or of the job ULID for a job with no
//! turn), the parent id a 64-bit span id, the flags `01`
//! (`eileen_contracts::ids::TraceCtx::traceparent` builds them). Workers get one in
//! `TRACEPARENT`, MCP requests in `_meta.traceparent`.

use eileen_contracts::ids::TraceCtx;

/// The variable `claude -p` reads its parent from at start.
pub const ENV: &str = "TRACEPARENT";

/// A parsed `traceparent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceParent {
    /// The trace id, never zero.
    pub trace_id: u128,
    /// The parent span id, never zero.
    pub parent_id: u64,
    /// The sampled flag.
    pub sampled: bool,
}

impl TraceParent {
    /// The header form, version `00`.
    #[must_use]
    pub fn to_header(self) -> String {
        format!("00-{:032x}-{:016x}-{:02x}", self.trace_id, self.parent_id, u8::from(self.sampled))
    }
}

fn hex<const N: usize>(text: &str) -> Option<u128> {
    (text.len() == N && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| u128::from_str_radix(text, 16).ok())
        .flatten()
}

/// Parses version `00` exactly as W3C Trace Context writes it: lower-case hex, non-zero ids.
#[must_use]
pub fn parse(text: &str) -> Option<TraceParent> {
    let mut parts = text.split('-');
    let version = parts.next()?;
    let trace = parts.next()?;
    let parent = parts.next()?;
    let flags = parts.next()?;
    if version != "00" || parts.next().is_some() {
        return None;
    }
    let trace_id = hex::<32>(trace)?;
    let parent_id = u64::try_from(hex::<16>(parent)?).ok()?;
    let flags = hex::<2>(flags)?;
    (trace_id != 0 && parent_id != 0).then_some(TraceParent {
        trace_id,
        parent_id,
        sampled: flags & 1 == 1,
    })
}

/// The id of a span, for a `traceparent` and for span records.
#[must_use]
pub fn span_id(span: &tracing::Span) -> Option<u64> {
    span.id().map(|id| id.into_u64())
}

/// The `traceparent` for `ctx` with the current span as the parent.
#[must_use]
pub fn current(ctx: &TraceCtx) -> Option<String> {
    ctx.traceparent(span_id(&tracing::Span::current())?)
}

#[cfg(test)]
mod tests {
    use super::{TraceParent, parse};

    #[test]
    fn parses_what_it_writes_and_refuses_the_rest() {
        let tp = TraceParent {
            trace_id: 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
            parent_id: 0xabc,
            sampled: true,
        };
        let header = tp.to_header();
        assert_eq!(header, "00-0123456789abcdef0123456789abcdef-0000000000000abc-01", "header");
        assert_eq!(parse(&header), Some(tp), "round trip");
        for bad in [
            "",
            "00-0123456789abcdef0123456789abcdef-0000000000000abc",
            "01-0123456789abcdef0123456789abcdef-0000000000000abc-01",
            "00-0123456789ABCDEF0123456789abcdef-0000000000000abc-01",
            "00-00000000000000000000000000000000-0000000000000abc-01",
            "00-0123456789abcdef0123456789abcdef-0000000000000000-01",
            "00-0123456789abcdef0123456789abcdef-0000000000000abc-01-x",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }
}
