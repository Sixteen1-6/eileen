//! `cargo xtask slots --pressure <GB>`: commits and touches memory for a while, to produce one
//! refused admission on purpose (acceptance 4, only with the owner's agreement).

use std::io;
use std::time::Duration;

use crate::clock::Clock;

/// The size of each allocation, bytes.
const CHUNK: usize = 64 * 1024 * 1024;
/// One write per page keeps every page resident.
const PAGE: usize = 4096;

/// Commits `gb` gigabytes, writes to every page so it is resident, holds it for `hold`, then
/// frees it. Fails cleanly, before touching anything, when the allocation is refused.
pub fn pressure(gb: u32, hold: Duration, clock: &dyn Clock) -> io::Result<()> {
    let total = usize::try_from(u64::from(gb) * 1024 * 1024 * 1024)
        .map_err(|_| io::Error::other("more memory than this platform can address"))?;
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut left = total;
    while left > 0 {
        let size = left.min(CHUNK);
        let mut chunk: Vec<u8> = Vec::new();
        chunk.try_reserve_exact(size).map_err(|e| io::Error::new(io::ErrorKind::OutOfMemory, e))?;
        chunk.resize(size, 0);
        for byte in chunk.iter_mut().step_by(PAGE) {
            *byte = 1;
        }
        chunks.push(chunk);
        left -= size;
    }
    clock.sleep(hold);
    drop(std::hint::black_box(chunks));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::pressure;
    use crate::clock::{Clock, ManualClock};

    #[test]
    fn zero_gigabytes_only_holds() {
        let clock = ManualClock::new(0);
        pressure(0, Duration::from_secs(60), &clock).unwrap();
        assert_eq!(clock.mono_ms(), 60_000, "held for the time asked");
    }
}
