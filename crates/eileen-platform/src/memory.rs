//! The machine's memory, for admission (BUILD.md section 2.5).

use eileen_contracts::jobs::MemoryPressure;

use crate::PlatformError;

/// Memory figures (`eileen-shell-win`: `GlobalMemoryStatusEx`, the commit figures and the PDH
/// paging counter). The runtime's sampler turns each sample into `Input::Memory` every 5 s.
pub trait SystemMemory: Send + Sync {
    /// One sample.
    fn sample(&self) -> Result<MemoryPressure, PlatformError>;
}

#[cfg(any(test, feature = "fake"))]
pub use fake::FakeSystemMemory;

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use eileen_contracts::jobs::MemoryPressure;

    use super::SystemMemory;
    use crate::PlatformError;
    use crate::fake_util::lock;

    /// Samples a test queues; the last one repeats once the queue runs dry.
    #[derive(Debug, Default)]
    pub struct FakeSystemMemory {
        queued: Mutex<VecDeque<MemoryPressure>>,
        last: Mutex<Option<MemoryPressure>>,
    }

    impl FakeSystemMemory {
        /// Queues `sample` behind the others.
        pub fn push(&self, sample: MemoryPressure) {
            lock(&self.queued).push_back(sample);
        }
    }

    impl SystemMemory for FakeSystemMemory {
        fn sample(&self) -> Result<MemoryPressure, PlatformError> {
            let next = lock(&self.queued).pop_front();
            let mut last = lock(&self.last);
            if let Some(sample) = next {
                *last = Some(sample);
            }
            last.clone().ok_or(PlatformError::Unavailable { what: "memory sample" })
        }
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::jobs::MemoryPressure;

    use super::{FakeSystemMemory, SystemMemory};

    fn pressure(available_mb: u32) -> MemoryPressure {
        MemoryPressure {
            available_mb,
            commit_headroom_mb: 4_000,
            paging_kb_s: 100,
            paging_limit_kb_s: 5_000,
            largest_process: None,
        }
    }

    #[test]
    fn samples_come_in_order_then_the_last_repeats() {
        let fake = FakeSystemMemory::default();
        assert!(fake.sample().is_err(), "nothing queued");
        fake.push(pressure(1_000));
        fake.push(pressure(2_000));
        assert_eq!(fake.sample().unwrap().available_mb, 1_000, "first");
        assert_eq!(fake.sample().unwrap().available_mb, 2_000, "second");
        assert_eq!(fake.sample().unwrap().available_mb, 2_000, "repeats");
    }
}
