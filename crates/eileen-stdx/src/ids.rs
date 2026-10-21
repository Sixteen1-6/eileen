//! The IDs port and its one real implementation.
//!
//! The core mints its IDs from `MonoTime` and its injected [`crate::rng::CoreRng`]; drivers,
//! sidecars and tools ask an [`Ids`] so replay and tests can substitute [`SeqIds`].

use std::sync::atomic::{AtomicU64, Ordering};

use eileen_contracts::ids::{
    ApprovalId, CallId, IncidentId, JobId, ProposalId, RecordingId, RouteProposalId, SessionId,
    SpeakId, TurnId, WorkerSessionId,
};
use ulid::Ulid;

/// Where new IDs come from.
pub trait Ids: Send + Sync {
    /// A new ULID.
    fn ulid(&self) -> Ulid;

    /// A new `sess_` ID.
    fn session(&self) -> SessionId {
        SessionId::from_ulid(self.ulid())
    }

    /// A new `turn_` ID.
    fn turn(&self) -> TurnId {
        TurnId::from_ulid(self.ulid())
    }

    /// A new `job_` ID.
    fn job(&self) -> JobId {
        JobId::from_ulid(self.ulid())
    }

    /// A new `ws_` ID.
    fn worker_session(&self) -> WorkerSessionId {
        WorkerSessionId::from_ulid(self.ulid())
    }

    /// A new `call_` ID.
    fn call(&self) -> CallId {
        CallId::from_ulid(self.ulid())
    }

    /// A new `speak_` ID.
    fn speak(&self) -> SpeakId {
        SpeakId::from_ulid(self.ulid())
    }

    /// A new `appr_` ID.
    fn approval(&self) -> ApprovalId {
        ApprovalId::from_ulid(self.ulid())
    }

    /// A new `inc_` ID.
    fn incident(&self) -> IncidentId {
        IncidentId::from_ulid(self.ulid())
    }

    /// A new `rec_` ID.
    fn recording(&self) -> RecordingId {
        RecordingId::from_ulid(self.ulid())
    }

    /// A new `prop_` ID.
    fn proposal(&self) -> ProposalId {
        ProposalId::from_ulid(self.ulid())
    }

    /// A new `route_` ID.
    fn route_proposal(&self) -> RouteProposalId {
        RouteProposalId::from_ulid(self.ulid())
    }
}

/// The real ID generator: the time now and 80 random bits from the OS.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsIds;

impl Ids for OsIds {
    #[expect(clippy::disallowed_methods, reason = "the one ID generator (BUILD.md section 4b.2)")]
    fn ulid(&self) -> Ulid {
        Ulid::generate()
    }
}

/// Deterministic IDs for tests and replay: a fixed timestamp, and a counter from 1 as the
/// random part, so the n-th ID is the same in every run.
#[derive(Debug)]
pub struct SeqIds {
    timestamp_ms: u64,
    issued: AtomicU64,
}

impl SeqIds {
    /// IDs stamped with `timestamp_ms`.
    #[must_use]
    pub const fn new(timestamp_ms: u64) -> Self {
        Self { timestamp_ms, issued: AtomicU64::new(0) }
    }
}

impl Ids for SeqIds {
    fn ulid(&self) -> Ulid {
        let n = self.issued.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        Ulid::from_parts(self.timestamp_ms, u128::from(n))
    }
}

#[cfg(test)]
mod tests {
    use super::{Ids, OsIds, SeqIds};

    #[test]
    fn seq_ids_repeat_across_runs() {
        let first = SeqIds::new(1_700_000_000_000);
        let second = SeqIds::new(1_700_000_000_000);
        assert_eq!(first.job(), second.job(), "same first id");
        assert_eq!(first.turn(), second.turn(), "same second id");
        assert_ne!(first.call().ulid(), SeqIds::new(1).call().ulid(), "timestamp matters");
    }

    #[test]
    fn os_ids_carry_their_prefix_and_differ() {
        let ids = OsIds;
        let a = ids.job();
        let b = ids.job();
        assert_ne!(a, b, "two fresh ids differ");
        assert!(a.to_string().starts_with("job_"), "prefix");
        assert!(ids.session().to_string().starts_with("sess_"), "session prefix");
    }
}
