//! Helpers for the tests of the test sources and sinks.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::stream::{CaptureCallback, CaptureInfo, StreamFormat};

/// 48 kHz mono, the format most tests use.
pub(crate) const MONO_48K: StreamFormat = StreamFormat { sample_rate_hz: 48_000, channels: 1 };

/// Samples in a 10 ms buffer at 48 kHz mono.
pub(crate) const BUFFER: usize = 480;

/// A directory of its own for one test, removed when it drops.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("eileen-audio-{test}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn join(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// What a recording callback heard: each buffer's time and samples.
pub(crate) type Heard = Arc<Mutex<Vec<(Duration, Vec<f32>)>>>;

/// A capture callback that records every buffer.
pub(crate) fn recorder() -> (CaptureCallback, Heard) {
    let heard: Heard = Arc::default();
    let sink = Arc::clone(&heard);
    let callback: CaptureCallback = Box::new(move |buffer: &[f32], info: CaptureInfo| {
        sink.lock().unwrap().push((info.captured_at, buffer.to_vec()));
    });
    (callback, heard)
}

/// The times of the buffers heard so far.
pub(crate) fn times(heard: &Heard) -> Vec<Duration> {
    heard.lock().unwrap().iter().map(|(at, _)| *at).collect()
}

/// The samples heard so far, end to end.
pub(crate) fn samples(heard: &Heard) -> Vec<f32> {
    heard.lock().unwrap().iter().flat_map(|(_, buffer)| buffer.iter().copied()).collect()
}

/// `n` milliseconds.
pub(crate) const fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The bit patterns of samples, so tests compare them exactly.
pub(crate) fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().copied().map(f32::to_bits).collect()
}

/// A sample of a test ramp that repeats every 4096 samples, exact in `f32`.
pub(crate) fn ramp(n: usize) -> f32 {
    f32::from(u16::try_from(n % 4096).unwrap()) / 4096.0
}
