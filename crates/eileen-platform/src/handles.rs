//! Handles and small values the trait signatures share.

use std::fmt;
use std::marker::PhantomData;
use std::time::Duration;

use crate::PlatformError;

/// How long a call may take. Implementations abandon a call past it and answer
/// [`PlatformError::DeadlineExceeded`] (section 2.1 rule 2: a UIA call defaults to 5 s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Deadline(Duration);

impl Deadline {
    /// The default for a UI Automation call.
    pub const UIA: Self = Self(Duration::from_secs(5));

    /// A deadline `budget` from when the call starts.
    #[must_use]
    pub const fn after(budget: Duration) -> Self {
        Self(budget)
    }

    /// The time the call may take.
    #[must_use]
    pub const fn budget(self) -> Duration {
        self.0
    }
}

/// Runs its release action once, when dropped.
struct OnDrop(Option<Box<dyn FnOnce() + Send>>);

impl OnDrop {
    fn new(release: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(release)))
    }

    fn take(&mut self) {
        if let Some(release) = self.0.take() {
            release();
        }
    }
}

/// A subscription to a stream of changes; dropping it unsubscribes.
pub struct Subscription(OnDrop);

impl Subscription {
    /// A subscription whose drop runs `unsubscribe`.
    #[must_use]
    pub fn new(unsubscribe: impl FnOnce() + Send + 'static) -> Self {
        Self(OnDrop::new(unsubscribe))
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.0.take();
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription").field("active", &self.0.0.is_some()).finish()
    }
}

/// An audio thread's MMCSS registration; dropping it reverts the thread.
///
/// Not `Send` on any OS: MMCSS must be reverted on the thread that joined it, so a ticket
/// moved to another thread would revert there. The marker keeps Linux to the Windows rule, as
/// `eileen_supervisor::qos::AudioBoost` does, and `revert` need not be `Send` either, so a
/// Windows implementation can capture the thread's MMCSS handle as it is (or wrap the
/// supervisor's `qos::boost_audio_thread`).
///
/// ```compile_fail
/// let ticket = eileen_platform::ThreadTicket::new("Pro Audio", || {});
/// std::thread::spawn(move || drop(ticket));
/// ```
pub struct ThreadTicket {
    task: &'static str,
    revert: Option<Box<dyn FnOnce()>>,
    _thread_bound: PhantomData<*const ()>,
}

impl ThreadTicket {
    /// A registration for the MMCSS task `task` (for example `"Pro Audio"`), reverted by
    /// `revert` on drop, on the thread that holds the ticket.
    #[must_use]
    pub fn new(task: &'static str, revert: impl FnOnce() + 'static) -> Self {
        Self { task, revert: Some(Box::new(revert)), _thread_bound: PhantomData }
    }

    /// The MMCSS task the thread joined.
    #[must_use]
    pub const fn task(&self) -> &'static str {
        self.task
    }
}

impl Drop for ThreadTicket {
    fn drop(&mut self) {
        if let Some(revert) = self.revert.take() {
            revert();
        }
    }
}

impl fmt::Debug for ThreadTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThreadTicket").field("task", &self.task).finish_non_exhaustive()
    }
}

/// A display-required and system-required power request for an armed run; dropping it
/// releases the request.
pub struct PowerRequest(OnDrop);

impl PowerRequest {
    /// A request released by `release` on drop.
    #[must_use]
    pub fn new(release: impl FnOnce() + Send + 'static) -> Self {
        Self(OnDrop::new(release))
    }
}

impl Drop for PowerRequest {
    fn drop(&mut self) {
        self.0.take();
    }
}

impl fmt::Debug for PowerRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PowerRequest").field("held", &self.0.0.is_some()).finish()
    }
}

/// An image, 8 bits per channel, red, green, blue and alpha, rows top to bottom.
#[derive(Clone, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaImage {
    /// An image from its pixels; the buffer must hold exactly `width * height * 4` bytes.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, PlatformError> {
        let expected = u64::from(width) * u64::from(height) * 4;
        if u64::try_from(pixels.len()).ok() == Some(expected) {
            Ok(Self { width, height, pixels })
        } else {
            Err(PlatformError::Invalid {
                what: "image buffer",
                detail: format!("{} bytes for {width}x{height}", pixels.len()),
            })
        }
    }

    /// Its width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Its height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The pixel bytes.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

impl fmt::Debug for RgbaImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RgbaImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.pixels.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::{PowerRequest, RgbaImage, Subscription, ThreadTicket};

    #[test]
    fn handles_release_exactly_once_on_drop() {
        let released = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&released);
        drop(Subscription::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        // The revert need not be Send: an `Rc` stands in for a thread-bound MMCSS handle.
        let counter = Arc::clone(&released);
        let thread_bound = Rc::new(());
        let ticket = ThreadTicket::new("Pro Audio", move || {
            drop(thread_bound);
            counter.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(ticket.task(), "Pro Audio", "task kept");
        drop(ticket);
        let counter = Arc::clone(&released);
        drop(PowerRequest::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(released.load(Ordering::SeqCst), 3, "each released once");
    }

    #[test]
    fn an_image_checks_its_buffer() {
        assert!(RgbaImage::new(2, 2, vec![0; 16]).is_ok(), "exact size");
        assert!(RgbaImage::new(2, 2, vec![0; 15]).is_err(), "short buffer");
    }
}
