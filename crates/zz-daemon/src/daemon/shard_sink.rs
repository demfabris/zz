use std::any::Any;
#[cfg(unix)]
use std::cell::RefCell;

use zz_terminal::{TerminalFrameSink, ViewFrame};

use super::*;

pub(super) struct PaneSink {
    pane: PaneId,
    frames: Arc<TerminalFrames>,
    state: Mutex<PaneSinkState>,
}

struct PaneSinkState {
    views: Vec<SinkView>,
    fanout: PaneFrameFanout,
    placements: bool,
    observer: Weak<dyn Any + Send + Sync>,
}

struct SinkView {
    view: TerminalViewId,
    mailbox: Arc<OutboundMailbox>,
    live: bool,
    previous: Option<(u64, Arc<TerminalViewport>)>,
}

type TrackedFrame = Option<(u64, Arc<TerminalViewport>)>;

#[cfg(unix)]
thread_local! {
    static HELD_WAKES: RefCell<Option<Vec<Arc<mio::Waker>>>> = const { RefCell::new(None) };
}

#[cfg(unix)]
pub(super) fn hold_loop_wake(waker: &Arc<mio::Waker>) -> bool {
    HELD_WAKES.with_borrow_mut(|held| {
        let Some(held) = held else {
            return false;
        };
        if !held.iter().any(|seen| Arc::ptr_eq(seen, waker)) {
            held.push(Arc::clone(waker));
        }
        true
    })
}

#[cfg(unix)]
fn hold_wakes() {
    HELD_WAKES.with_borrow_mut(|held| {
        held.get_or_insert_with(Vec::new);
    });
}

#[cfg(unix)]
fn release_wakes(notified: bool) {
    let Some(held) = HELD_WAKES.with_borrow_mut(Option::take) else {
        return;
    };
    if !notified {
        for waker in held {
            let _ = waker.wake();
        }
    }
}

#[cfg(not(unix))]
fn hold_wakes() {}

#[cfg(not(unix))]
fn release_wakes(_: bool) {}

impl PaneSink {
    pub(super) fn new(pane: PaneId, frames: Arc<TerminalFrames>) -> Self {
        Self {
            pane,
            frames,
            state: Mutex::new(PaneSinkState {
                views: Vec::new(),
                fanout: PaneFrameFanout::new(),
                placements: false,
                observer: Weak::<()>::new(),
            }),
        }
    }

    pub(super) fn of(terminal: &TerminalSession) -> Option<&Self> {
        terminal.frame_sink()?.as_any().downcast_ref::<Self>()
    }

    pub(super) fn set_view(
        &self,
        view: TerminalViewId,
        record: Option<(Arc<OutboundMailbox>, bool)>,
    ) {
        let mut state = self.state.lock();
        let Some((mailbox, live)) = record else {
            state.views.retain(|sink| sink.view != view);
            return;
        };
        if let Some(sink) = state.views.iter_mut().find(|sink| sink.view == view) {
            if !Arc::ptr_eq(&sink.mailbox, &mailbox) {
                sink.mailbox = mailbox;
                sink.previous = None;
            }
            sink.live = live;
            return;
        }
        state.views.push(SinkView {
            view,
            mailbox,
            live,
            previous: None,
        });
    }

    pub(super) fn clear(&self) {
        self.state.lock().views.clear();
    }

    pub(super) fn observe<T: Any + Send + Sync>(&self, observer: &Arc<T>) {
        let observer: Weak<dyn Any + Send + Sync> = Arc::<T>::downgrade(observer);
        self.state.lock().observer = observer;
    }

    fn tracked(&self, view: TerminalViewId) -> Option<TrackedFrame> {
        self.state
            .lock()
            .views
            .iter()
            .find(|sink| sink.view == view)
            .map(|sink| sink.previous.clone())
    }

    fn track(
        &self,
        view: TerminalViewId,
        seen: Option<&Arc<TerminalViewport>>,
        next: TrackedFrame,
    ) {
        let mut state = self.state.lock();
        let Some(sink) = state.views.iter_mut().find(|sink| sink.view == view) else {
            return;
        };
        let unchanged = match (sink.previous.as_ref(), seen) {
            (None, None) => true,
            (Some((_, previous)), Some(seen)) => Arc::ptr_eq(previous, seen),
            _ => false,
        };
        if unchanged {
            sink.previous = next;
        }
    }

    #[cfg(test)]
    pub(super) fn views(&self) -> Vec<TerminalViewId> {
        self.state
            .lock()
            .views
            .iter()
            .filter(|sink| sink.live)
            .map(|sink| sink.view)
            .collect()
    }

    #[cfg(test)]
    pub(super) fn previous(&self, view: TerminalViewId) -> Option<Arc<TerminalViewport>> {
        self.tracked(view).flatten().map(|(_, previous)| previous)
    }
}

impl TerminalFrameSink for PaneSink {
    fn deliver(&self, frames: &[ViewFrame], sunk: &mut Vec<TerminalViewId>) -> bool {
        let mut state = self.state.lock();
        let PaneSinkState {
            views,
            fanout,
            placements,
            observer,
        } = &mut *state;
        let urgent = observer.strong_count() != 0;
        if views.is_empty() {
            return urgent;
        }
        let placed = frames
            .iter()
            .any(|(_, viewport, _)| !viewport.kitty_placements.is_empty());
        if std::mem::replace(placements, placed) || placed {
            return urgent;
        }
        let pane = self.pane;
        let mut closed = false;
        hold_wakes();
        for (view, viewport, epoch) in frames {
            let Some(target) = views
                .iter_mut()
                .find(|sink| sink.view == *view && sink.live)
            else {
                continue;
            };
            let Some(epoch) = *epoch else {
                continue;
            };
            if viewport.mode != TerminalMode::Live
                || target.mailbox.terminals_frozen()
                || target.mailbox.terminal_pending(pane)
            {
                continue;
            }
            let base = target
                .previous
                .as_ref()
                .filter(|(seen, _)| *seen == epoch)
                .map(|(_, previous)| previous.as_ref());
            let delivery = TerminalDelivery::Foreground;
            let frames = &self.frames;
            let result = fanout.enqueue(&target.mailbox, frames, pane, base, viewport, delivery);
            if result == TerminalEnqueue::NeedsFull {
                let _ = target
                    .mailbox
                    .replace_terminal_viewport(pane, viewport, frames);
            }
            closed |= result == TerminalEnqueue::Closed;
            target.previous = Some((epoch, Arc::clone(viewport)));
            sunk.push(*view);
        }
        fanout.release();
        if closed {
            views.retain(|sink| !sink.mailbox.is_closed());
        }
        urgent
    }

    fn published(&self, notified: bool) {
        release_wakes(notified);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(super) fn publish_loop_view(
    shared: &Arc<Shared>,
    terminal: &Arc<TerminalSession>,
    pane: PaneId,
    frame: &ViewFrame,
    previous: Option<&(u64, Arc<TerminalViewport>)>,
    fanout: &mut PaneFrameFanout,
) {
    let (view, viewport, epoch) = frame;
    let sink = PaneSink::of(terminal);
    let tracked = sink.and_then(|sink| sink.tracked(*view));
    let seen = tracked.clone().flatten();
    if seen.as_ref().is_some_and(|(_, newer)| {
        viewport_generation(viewport).precedes(viewport_generation(newer))
    }) {
        return;
    }
    let base = epoch.and_then(|epoch| {
        seen.as_ref()
            .or(previous)
            .filter(|(known, _)| *known == epoch)
            .map(|(_, base)| base.as_ref())
    });
    shared.publish_terminal_for_pane(pane, ClientId(view.0), base, viewport, terminal, fanout);
    if let Some(sink) = sink
        && tracked.is_some()
    {
        sink.track(
            *view,
            seen.as_ref().map(|(_, seen)| seen),
            epoch.map(|epoch| (epoch, Arc::clone(viewport))),
        );
    }
}
