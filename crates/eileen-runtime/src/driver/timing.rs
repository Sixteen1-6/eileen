//! The driver's two clocks of its own: the core's timers, and the pace of `Hud` snapshots.

use std::collections::BTreeMap;

use eileen_contracts::core_io::{Effect, TimerId};
use eileen_contracts::ids::{Envelope, MonoTime};
use tokio::time::Instant;

use super::HUD_EVERY;

/// The most timers the driver keeps at once. The core keeps at most one per kind, so passing it
/// is a bug in the core: the new timer is refused with a soft assertion.
const MAX_TIMERS: usize = 1_024;

/// The timers the core has set and not cancelled.
#[derive(Debug, Default)]
pub(super) struct Timers(BTreeMap<TimerId, MonoTime>);

impl Timers {
    /// Sets or cancels a timer.
    pub(super) fn apply(&mut self, effect: &Effect) {
        if let Effect::SetTimer { id, at } = effect {
            let full = self.0.len() >= MAX_TIMERS && !self.0.contains_key(id);
            if !eileen_stdx::never!(full, "the core holds {MAX_TIMERS} timers; {id:?} is refused") {
                self.0.insert(*id, *at);
            }
        } else if let Effect::CancelTimer(id) = effect {
            self.0.remove(id);
        }
    }

    /// The timer due first, the lower id first among timers due at once.
    pub(super) fn next(&self) -> Option<(TimerId, MonoTime)> {
        self.0.iter().map(|(id, at)| (*id, *at)).min_by_key(|&(id, at)| (at, id))
    }
}

/// Holds `Hud` snapshots to one every [`HUD_EVERY`]. Each is the whole HUD state, so the newest
/// replaces one still waiting, and only states the HUD would have shown for less than that are
/// lost.
#[derive(Debug, Default)]
pub(super) struct HudPace {
    /// When the next snapshot may go; `None` before the first.
    next: Option<Instant>,
    /// The newest snapshot not yet sent.
    waiting: Option<Envelope<Effect>>,
}

impl HudPace {
    /// The snapshot to send now, or `None` when it must wait its turn (kept as the newest).
    pub(super) fn offer(
        &mut self,
        effect: Envelope<Effect>,
        now: Instant,
    ) -> Option<Envelope<Effect>> {
        if self.next.is_some_and(|next| now < next) {
            self.waiting = Some(effect);
            return None;
        }
        self.waiting = None;
        self.next = now.checked_add(HUD_EVERY);
        Some(effect)
    }

    /// When the waiting snapshot may go, if one waits.
    pub(super) const fn release_at(&self) -> Option<Instant> {
        if self.waiting.is_some() { self.next } else { None }
    }

    /// The waiting snapshot, now that its turn has come.
    pub(super) fn release(&mut self, now: Instant) -> Option<Envelope<Effect>> {
        let effect = self.waiting.take()?;
        self.next = now.checked_add(HUD_EVERY);
        Some(effect)
    }
}
