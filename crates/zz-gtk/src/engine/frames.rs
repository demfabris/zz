use std::{collections::HashMap, mem, sync::Mutex};

use zz_client::ViewportDamage;
use zz_protocol::PaneId;
use zz_terminal::TerminalViewport;

/// One pane's newest frame plus every row touched since the UI last drained.
pub struct FrameUpdate {
    pub pane: PaneId,
    pub viewport: TerminalViewport,
    pub damage: ViewportDamage,
}

#[derive(Default)]
struct FrameState {
    pending: HashMap<PaneId, FrameUpdate>,
    wake_pending: bool,
}

/// Frames arrive faster than a compositor can paint them, so the reader thread
/// keeps only the newest viewport per pane and unions the damage. One wake is
/// outstanding at a time: everything published behind it rides the same drain.
#[derive(Default)]
pub struct FrameInbox(Mutex<FrameState>);

impl FrameInbox {
    /// True when this publish opened a new wake the caller must forward.
    pub fn publish(
        &self,
        pane: PaneId,
        viewport: TerminalViewport,
        damage: ViewportDamage,
    ) -> bool {
        let mut state = self.lock();
        state
            .pending
            .entry(pane)
            .and_modify(|pending| {
                pending.viewport.clone_from(&viewport);
                merge_damage(&mut pending.damage, damage.clone());
            })
            .or_insert(FrameUpdate {
                pane,
                viewport,
                damage,
            });
        if state.wake_pending {
            return false;
        }
        state.wake_pending = true;
        true
    }

    pub fn take(&self) -> Vec<FrameUpdate> {
        let mut state = self.lock();
        state.wake_pending = false;
        mem::take(&mut state.pending).into_values().collect()
    }

    pub fn forget(&self, pane: PaneId) {
        self.lock().pending.remove(&pane);
    }

    pub fn clear(&self) {
        let mut state = self.lock();
        state.pending.clear();
        state.wake_pending = false;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FrameState> {
        self.0.lock().expect("frame inbox poisoned")
    }
}

pub fn merge_damage(current: &mut ViewportDamage, next: ViewportDamage) {
    match (&mut *current, next) {
        (ViewportDamage::All, _) => {}
        (_, ViewportDamage::All) => *current = ViewportDamage::All,
        (ViewportDamage::Rows(rows), ViewportDamage::Rows(extra)) => {
            rows.extend(extra);
            rows.sort_unstable();
            rows.dedup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_terminal::SessionStatus;

    fn viewport(columns: u16) -> TerminalViewport {
        TerminalViewport::blank(columns, 24, SessionStatus::Running)
    }

    #[test]
    fn publishing_twice_keeps_one_wake_and_unions_the_damage() {
        let inbox = FrameInbox::default();

        assert!(inbox.publish(PaneId(1), viewport(80), ViewportDamage::Rows(vec![3, 1])));
        assert!(!inbox.publish(PaneId(1), viewport(120), ViewportDamage::Rows(vec![1, 2])));

        let drained = inbox.take();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].viewport.columns, 120);
        assert_eq!(drained[0].damage, ViewportDamage::Rows(vec![1, 2, 3]));
        assert!(inbox.publish(PaneId(1), viewport(80), ViewportDamage::All));
    }

    #[test]
    fn a_full_frame_swallows_row_damage_in_both_directions() {
        let mut damage = ViewportDamage::Rows(vec![2]);
        merge_damage(&mut damage, ViewportDamage::All);
        assert_eq!(damage, ViewportDamage::All);
        merge_damage(&mut damage, ViewportDamage::Rows(vec![5]));
        assert_eq!(damage, ViewportDamage::All);
    }
}
