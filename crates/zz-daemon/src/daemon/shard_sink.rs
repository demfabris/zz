use std::any::Any;

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
}

struct SinkView {
    view: TerminalViewId,
    mailbox: Arc<OutboundMailbox>,
    previous: Option<(u64, Arc<TerminalViewport>)>,
}

impl PaneSink {
    pub(super) fn new(pane: PaneId, frames: Arc<TerminalFrames>) -> Self {
        Self {
            pane,
            frames,
            state: Mutex::new(PaneSinkState {
                views: Vec::new(),
                fanout: PaneFrameFanout::new(),
                placements: false,
            }),
        }
    }

    pub(super) fn of(terminal: &TerminalSession) -> Option<&Self> {
        terminal.frame_sink()?.as_any().downcast_ref::<Self>()
    }

    pub(super) fn set_view(&self, view: TerminalViewId, mailbox: Option<Arc<OutboundMailbox>>) {
        let mut state = self.state.lock();
        let Some(mailbox) = mailbox else {
            state.views.retain(|sink| sink.view != view);
            return;
        };
        if let Some(sink) = state.views.iter_mut().find(|sink| sink.view == view) {
            if !Arc::ptr_eq(&sink.mailbox, &mailbox) {
                sink.mailbox = mailbox;
                sink.previous = None;
            }
            return;
        }
        state.views.push(SinkView {
            view,
            mailbox,
            previous: None,
        });
    }

    pub(super) fn clear(&self) {
        self.state.lock().views.clear();
    }

    #[cfg(test)]
    pub(super) fn views(&self) -> Vec<TerminalViewId> {
        self.state
            .lock()
            .views
            .iter()
            .map(|sink| sink.view)
            .collect()
    }
}

impl TerminalFrameSink for PaneSink {
    fn deliver(&self, frames: &[ViewFrame], sunk: &mut Vec<TerminalViewId>) {
        let mut state = self.state.lock();
        let PaneSinkState {
            views,
            fanout,
            placements,
        } = &mut *state;
        if views.is_empty() {
            return;
        }
        let placed = frames
            .iter()
            .any(|(_, viewport, _)| !viewport.kitty_placements.is_empty());
        if std::mem::replace(placements, placed) || placed {
            for sink in views.iter_mut() {
                sink.previous = None;
            }
            return;
        }
        let pane = self.pane;
        let mut closed = false;
        for (view, viewport, epoch) in frames {
            let Some(target) = views.iter_mut().find(|sink| sink.view == *view) else {
                continue;
            };
            let Some(epoch) = *epoch else {
                target.previous = None;
                continue;
            };
            if viewport.mode != TerminalMode::Live
                || target.mailbox.terminals_frozen()
                || target.mailbox.terminal_pending(pane)
            {
                target.previous = None;
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
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
