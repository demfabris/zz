//! Shared leased pulse clock for repeating indicator animations.
//!
//! A repeating [`gpui::Animation`] asks for a redraw on every display frame for
//! as long as its element stays mounted, and in gpui a notify repaints the
//! whole window — one mounted spinner pins the window to the display refresh
//! rate. This clock replaces that with a single ~30fps tick shared by every
//! indicator: reading a phase takes a short lease on the reading entity, the
//! tick notifies the current leaseholders, and the loop parks as soon as the
//! last lease lapses, so a window with nothing pulsing schedules nothing.
//!
//! One epoch backs every lease, so indicators that mount at different times
//! stay phase-locked instead of each running its own animation timeline.

use std::collections::HashMap;

use gpui::{App, EntityId, Global};
use instant::{Duration, Instant};

/// Tick interval of the shared clock, ~30fps.
const TICK: Duration = Duration::from_millis(33);

/// How long one read keeps its entity on the tick list. This has to outlive a
/// handful of skipped ticks: a leaseholder that falls off the list stops being
/// notified, so it can only come back once something else redraws it.
const LEASE: Duration = Duration::from_millis(300);

struct PulseClock {
    epoch: Instant,
    leases: HashMap<EntityId, Instant>,
    running: bool,
}

impl Global for PulseClock {}

impl Default for PulseClock {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            leases: HashMap::new(),
            running: false,
        }
    }
}

/// Position of this frame within `period`, in `0.0..1.0`, leasing `view` on the
/// shared clock so it keeps being redrawn while it keeps reading. Pass the
/// entity whose notify repaints the indicator — a view, or the store the view
/// reads during its render.
///
/// Under reduced motion the phase freezes at 0 and nothing is scheduled.
pub fn pulse_phase(period: Duration, view: EntityId, cx: &mut App) -> f32 {
    if cx.reduce_motion() {
        return 0.0;
    }

    let phase = phase_at(cx.default_global::<PulseClock>().epoch.elapsed(), period);
    pulse_lease(view, cx);
    phase
}

pub(crate) fn pulse_lease(view: EntityId, cx: &mut App) {
    if cx.reduce_motion() {
        return;
    }

    let clock = cx.default_global::<PulseClock>();
    clock.leases.insert(view, Instant::now() + LEASE);
    let start = !clock.running;
    clock.running = true;

    if start {
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if cx.update(tick) {
                    break;
                }
            }
        })
        .detach();
    }
}

/// Position within `period`, in `0.0..1.0`. Exact modular arithmetic, so the
/// phase stays drift-free for the life of the process — dividing elapsed
/// seconds as `f32` loses sub-frame resolution within a day of uptime.
pub fn phase_at(elapsed: Duration, period: Duration) -> f32 {
    let period = period.as_nanos();
    if period == 0 {
        return 0.0;
    }
    // The last nanoseconds of a period round up to exactly 1.0 in `f32`, which
    // would let `phase * len` index one past the end of a wave.
    let phase = (elapsed.as_nanos() % period) as f32 / period as f32;
    phase.min(1.0f32.next_down())
}

fn tick(cx: &mut App) -> bool {
    let clock = cx.default_global::<PulseClock>();
    if sweep(&mut clock.leases, Instant::now()) {
        clock.running = false;
        return true;
    }

    let views: Vec<EntityId> = clock.leases.keys().copied().collect();
    for view in views {
        cx.notify(view);
    }
    false
}

/// Drop the lapsed leases, reporting whether the clock has nothing left to
/// drive and should park.
fn sweep(leases: &mut HashMap<EntityId, Instant>, now: Instant) -> bool {
    leases.retain(|_, until| *until > now);
    leases.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_millis(800);

    #[test]
    fn phase_starts_at_zero_and_climbs_within_the_period() {
        assert_eq!(phase_at(Duration::ZERO, PERIOD), 0.0);
        assert_eq!(phase_at(Duration::from_millis(200), PERIOD), 0.25);
        assert_eq!(phase_at(Duration::from_millis(400), PERIOD), 0.5);
    }

    #[test]
    fn phase_wraps_instead_of_reaching_one() {
        assert_eq!(phase_at(PERIOD, PERIOD), 0.0);
        assert_eq!(phase_at(Duration::from_millis(2200), PERIOD), 0.75);
        assert!(phase_at(Duration::from_nanos(799_999_999), PERIOD) < 1.0);
    }

    #[test]
    fn phase_is_drift_free_after_a_day() {
        let day = PERIOD * 108_000;
        assert_eq!(phase_at(day + Duration::from_millis(200), PERIOD), 0.25);
    }

    #[test]
    fn a_zero_period_freezes_rather_than_dividing_by_zero() {
        assert_eq!(phase_at(Duration::from_millis(200), Duration::ZERO), 0.0);
    }

    #[test]
    fn sweep_drops_lapsed_leases_and_parks_once_empty() {
        let now = Instant::now();
        let mut leases = HashMap::from([
            (EntityId::from(1), now + Duration::from_millis(100)),
            (EntityId::from(2), now + Duration::from_millis(300)),
        ]);

        assert!(!sweep(&mut leases, now + Duration::from_millis(200)));
        assert_eq!(leases.len(), 1);
        assert!(leases.contains_key(&EntityId::from(2)));

        assert!(sweep(&mut leases, now + Duration::from_millis(400)));
        assert!(leases.is_empty());
    }
}
