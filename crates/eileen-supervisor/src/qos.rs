//! Scheduling quality of service (section 2.1 rule 6): every Eileen process opts out of
//! `EcoQoS` execution-speed throttling at start (without it one Kokoro run took 27.8 s instead
//! of 2.4 s), and audio threads register with MMCSS.

use std::fmt;
use std::marker::PhantomData;

/// A quality-of-service call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{call} failed: {detail}")]
pub struct QosError {
    /// The Win32 call.
    pub call: &'static str,
    /// The OS's message.
    pub detail: String,
}

/// Opts this process out of `EcoQoS` execution-speed throttling. Off Windows it does nothing.
#[cfg_attr(
    not(windows),
    expect(
        clippy::missing_const_for_fn,
        clippy::unnecessary_wraps,
        reason = "the signature is the Windows one; off Windows there is nothing to opt out of"
    )
)]
pub fn opt_out_of_ecoqos() -> Result<(), QosError> {
    #[cfg(windows)]
    {
        crate::ffi::opt_out_of_ecoqos()
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// The calling thread's MMCSS registration; dropping it reverts the thread.
///
/// Not `Send` on any OS: MMCSS must be reverted on the thread that joined it, and the marker
/// keeps Linux to the Windows rule, so code that moves a boost to another thread fails on the
/// Linux lane as it would on Windows.
pub struct AudioBoost {
    #[cfg(windows)]
    _handle: crate::ffi::MmcssHandle,
    task: &'static str,
    _thread_bound: PhantomData<*const ()>,
}

impl fmt::Debug for AudioBoost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioBoost").field("task", &self.task).finish_non_exhaustive()
    }
}

impl AudioBoost {
    /// The MMCSS task the thread joined.
    #[must_use]
    pub const fn task(&self) -> &'static str {
        self.task
    }
}

/// Registers the calling thread with MMCSS as `Pro Audio`. Off Windows it returns a boost
/// that does nothing.
#[cfg_attr(
    not(windows),
    expect(
        clippy::missing_const_for_fn,
        clippy::unnecessary_wraps,
        reason = "the signature is the Windows one; off Windows the boost does nothing"
    )
)]
pub fn boost_audio_thread() -> Result<AudioBoost, QosError> {
    const TASK: &str = "Pro Audio";
    #[cfg(windows)]
    {
        Ok(AudioBoost {
            _handle: crate::ffi::MmcssHandle::join(TASK)?,
            task: TASK,
            _thread_bound: PhantomData,
        })
    }
    #[cfg(not(windows))]
    {
        Ok(AudioBoost { task: TASK, _thread_bound: PhantomData })
    }
}

#[cfg(test)]
mod tests {
    use super::{boost_audio_thread, opt_out_of_ecoqos};

    #[test]
    fn this_process_opts_out_of_ecoqos() {
        opt_out_of_ecoqos().unwrap();
    }

    #[test]
    fn an_audio_boost_reverts_on_drop() {
        // Windows Server runners may have no audio service; a refusal must come back as a
        // typed error, and a boost must revert cleanly.
        match boost_audio_thread() {
            Ok(boost) => {
                assert_eq!(boost.task(), "Pro Audio", "task");
                drop(boost);
            }
            Err(e) => assert_eq!(e.call, "AvSetMmThreadCharacteristicsW", "typed error: {e}"),
        }
    }
}
