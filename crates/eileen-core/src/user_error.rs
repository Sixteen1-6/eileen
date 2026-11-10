//! The `UserError`s the core builds (BUILD.md section 2.13): a stable kind, one plain sentence
//! from a template here, and a fix when one exists. WP1 adds the other kinds with their
//! sentences and fixes as the core meets them.

use eileen_contracts::bus::{UserError, UserErrorKind};

/// A `jobs_dispatch` of `ml_project`, the one kind v0.1 does not run (section 2.5): the
/// sentence says ML projects on the user's Colab come in the next version and offers what runs
/// today, a research job on the models and datasets that exist and their licences. No fix
/// action starts a job, so the brain offers it in words.
pub(crate) fn job_kind_unavailable() -> UserError {
    UserError {
        kind: UserErrorKind::JobKindUnavailable,
        sentence: "ML projects on your Colab come in the next version of Eileen. I can start a \
                   research job now on the models and datasets that already exist, and their \
                   licences."
            .to_owned(),
        fix: None,
    }
}
