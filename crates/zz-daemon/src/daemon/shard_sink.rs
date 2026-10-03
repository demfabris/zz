use std::any::Any;
#[cfg(unix)]
use std::cell::RefCell;

use zz_terminal::{OutputWake, TerminalFrameSink, ViewFrame};

use super::*;

pub(super) struct PaneSink {
    pane: PaneId,
    frames: Arc<TerminalFrames>,
    state: Mutex<PaneSinkState>,
    takes: AtomicBool,
    wake: OutputWake,
}

struct PaneSinkState {
    views: Vec<SinkView>,
    fanout: PaneFrameFanout,
    placements: bool,
    observer: Weak<dyn Any + Send + Sync>,
    controls: Vec<Arc<OutboundMailbox>>,
    pipe: Option<Arc<PipeFeed>>,
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
    pub(super) fn new(pane: PaneId, frames: Arc<TerminalFrames>, wake: OutputWake) -> Self {
        Self {
            pane,
            frames,
            state: Mutex::new(PaneSinkState {
                views: Vec::new(),
                fanout: PaneFrameFanout::new(),
                placements: false,
                observer: Weak::<()>::new(),
                controls: Vec::new(),
                pipe: None,
            }),
            takes: AtomicBool::new(false),
            wake,
        }
    }

    pub(super) fn set_control(&self, mailbox: &Arc<OutboundMailbox>, routed: bool) {
        let mut state = self.state.lock();
        state
            .controls
            .retain(|known| !Arc::ptr_eq(known, mailbox) && !known.is_closed());
        if routed && !mailbox.is_closed() {
            state.controls.push(Arc::clone(mailbox));
        }
        self.retake(&state);
    }

    pub(super) fn set_pipe(&self, pipe: Option<Arc<PipeFeed>>) {
        let mut state = self.state.lock();
        state.pipe = pipe;
        self.retake(&state);
    }

    fn retake(&self, state: &PaneSinkState) {
        self.takes.store(
            !state.controls.is_empty() || state.pipe.is_some(),
            Ordering::Release,
        );
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
        let mut state = self.state.lock();
        state.views.clear();
        state.controls.clear();
        state.pipe = None;
        self.retake(&state);
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
            ..
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

    fn takes_output(&self) -> bool {
        self.takes.load(Ordering::Acquire)
    }

    fn output_room(&self) -> usize {
        let state = self.state.lock();
        let controls = state.controls.iter().filter_map(|mailbox| {
            mailbox
                .control_feed()
                .map(|feed| feed.room(self.pane, &self.wake))
        });
        let pipe = state.pipe.iter().map(|pipe| pipe.room(&self.wake));
        controls.chain(pipe).min().unwrap_or(usize::MAX)
    }

    fn output(&self, bytes: &Arc<[u8]>) {
        let mut state = self.state.lock();
        let mut closed = false;
        for mailbox in &state.controls {
            if let Some(feed) = mailbox.control_feed() {
                closed |= !feed.push(self.pane, bytes, Some(&self.wake));
            }
        }
        if closed {
            state.controls.retain(|mailbox| !mailbox.is_closed());
        }
        if state
            .pipe
            .as_ref()
            .is_some_and(|pipe| !pipe.push(bytes, &self.wake))
        {
            state.pipe = None;
        }
        if closed || state.pipe.is_none() {
            self.retake(&state);
        }
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

pub(super) struct ControlWake {
    pending: AtomicBool,
    wake: AcceptWake,
    #[cfg(windows)]
    pump: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>>,
}

impl ControlWake {
    pub(super) fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
            wake: AcceptWake::new(),
            #[cfg(windows)]
            pump: std::sync::OnceLock::new(),
        }
    }

    #[cfg(unix)]
    pub(super) fn install(&self, waker: Arc<mio::Waker>) {
        self.wake.install(waker);
    }

    #[cfg(windows)]
    pub(super) fn install_pump(&self, pump: impl Fn() + Send + Sync + 'static) {
        let _ = self.pump.set(Box::new(pump));
    }

    fn notify(&self) {
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        self.wake.wake();
        #[cfg(windows)]
        if let Some(pump) = self.pump.get() {
            pump();
        }
    }

    pub(super) fn take(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }

    pub(super) fn again(&self, progressed: bool) {
        self.pending.store(true, Ordering::Release);
        if progressed {
            self.wake.wake();
        }
    }
}

pub(super) struct ControlFeed {
    state: Mutex<FeedState>,
    delivery: Mutex<()>,
    wake: Arc<ControlWake>,
}

#[derive(Default)]
struct FeedState {
    panes: BTreeMap<PaneId, FeedPane>,
    muted: BTreeSet<PaneId>,
    no_output: bool,
    pause_after_ms: Option<u64>,
    closed: bool,
}

#[derive(Default)]
struct FeedPane {
    pending: VecDeque<PendingControlOutput>,
    waiter: Option<OutputWake>,
}

impl FeedPane {
    fn release(&mut self) {
        if self.pending.len() < CONTROL_PENDING_CHUNKS_PER_PANE
            && let Some(waiter) = self.waiter.take()
        {
            waiter.wake();
        }
    }

    fn clear(&mut self) {
        self.pending.clear();
        self.release();
    }
}

impl FeedState {
    fn clear(&mut self) {
        for pane in self.panes.values_mut() {
            pane.clear();
        }
    }

    fn pending(&self) -> bool {
        self.panes.values().any(|pane| !pane.pending.is_empty())
    }
}

#[derive(Default)]
pub(super) struct FeedPump {
    pub(super) paused: Vec<PaneId>,
    pub(super) kill: bool,
    pub(super) pending: bool,
    pub(super) progressed: bool,
}

pub(super) fn passes_control_barrier(message: &ProtocolMessage) -> bool {
    matches!(
        message,
        ProtocolMessage::TreeSync
            | ProtocolMessage::Event(Event {
                payload: EventPayload::PaneOutput { .. }
                    | EventPayload::PaneOutputAged { .. }
                    | EventPayload::PaneOutputState { .. }
                    | EventPayload::TreeDelta(_)
                    | EventPayload::Snapshot(_),
                ..
            })
    )
}

impl ControlFeed {
    pub(super) fn new(wake: Arc<ControlWake>) -> Self {
        Self {
            state: Mutex::new(FeedState::default()),
            delivery: Mutex::new(()),
            wake,
        }
    }

    pub(super) fn push(
        &self,
        pane: PaneId,
        bytes: &Arc<[u8]>,
        waiter: Option<&OutputWake>,
    ) -> bool {
        let mut state = self.state.lock();
        if state.closed {
            return false;
        }
        if state.no_output || state.muted.contains(&pane) {
            return true;
        }
        let idle = !state.pending();
        let entry = state.panes.entry(pane).or_default();
        entry.pending.push_back(PendingControlOutput {
            bytes: Arc::clone(bytes),
            offset: 0,
            enqueued_at: Instant::now(),
        });
        if entry.pending.len() >= CONTROL_PENDING_CHUNKS_PER_PANE
            && let Some(waiter) = waiter
        {
            entry.waiter = Some(waiter.clone());
        }
        drop(state);
        if idle {
            self.wake.notify();
        }
        true
    }

    fn room(&self, pane: PaneId, waiter: &OutputWake) -> usize {
        let mut state = self.state.lock();
        if state.closed || state.no_output || state.muted.contains(&pane) {
            return usize::MAX;
        }
        let Some(entry) = state.panes.get_mut(&pane) else {
            return CONTROL_PENDING_CHUNKS_PER_PANE;
        };
        let room = CONTROL_PENDING_CHUNKS_PER_PANE.saturating_sub(entry.pending.len());
        if room == 0 {
            entry.waiter = Some(waiter.clone());
        }
        room
    }

    pub(super) fn configure(
        &self,
        no_output: bool,
        pause_after_ms: Option<u64>,
        muted: BTreeSet<PaneId>,
    ) {
        let mut state = self.state.lock();
        state.no_output = no_output;
        state.pause_after_ms = pause_after_ms;
        if no_output {
            state.clear();
        }
        for pane in &muted {
            if let Some(entry) = state.panes.get_mut(pane) {
                entry.clear();
            }
        }
        state.muted = muted;
    }

    pub(super) fn clear(&self) {
        self.state.lock().clear();
    }

    pub(super) fn clear_pane(&self, pane: PaneId) {
        if let Some(entry) = self.state.lock().panes.get_mut(&pane) {
            entry.clear();
        }
    }

    pub(super) fn close(&self) {
        let mut state = self.state.lock();
        state.closed = true;
        state.clear();
    }

    pub(super) fn pending_panes(&self) -> Vec<PaneId> {
        self.state
            .lock()
            .panes
            .iter()
            .filter_map(|(pane, entry)| (!entry.pending.is_empty()).then_some(*pane))
            .collect()
    }

    #[cfg(unix)]
    pub(super) fn oldest(&self) -> Option<Instant> {
        self.state
            .lock()
            .panes
            .values()
            .filter_map(|pane| pane.pending.front().map(|chunk| chunk.enqueued_at))
            .min()
    }

    #[cfg(test)]
    pub(super) fn queued(&self, pane: PaneId) -> usize {
        self.state
            .lock()
            .panes
            .get(&pane)
            .map_or(0, |entry| entry.pending.len())
    }

    #[cfg(test)]
    pub(super) fn queued_bytes(&self, pane: PaneId) -> Vec<u8> {
        self.state
            .lock()
            .panes
            .get(&pane)
            .map_or_else(Vec::new, |entry| {
                entry
                    .pending
                    .iter()
                    .flat_map(|chunk| chunk.bytes[chunk.offset..].iter().copied())
                    .collect()
            })
    }

    #[cfg(test)]
    pub(super) fn queue_at(&self, pane: PaneId, bytes: &[u8], enqueued_at: Instant) {
        self.state
            .lock()
            .panes
            .entry(pane)
            .or_default()
            .pending
            .push_back(PendingControlOutput {
                bytes: Arc::from(bytes),
                offset: 0,
                enqueued_at,
            });
    }

    pub(super) fn flush(&self, mailbox: &OutboundMailbox) {
        if !self.state.lock().pending() {
            return;
        }
        let _delivery = self.delivery.lock();
        let deliveries = {
            let mut state = self.state.lock();
            let now = Instant::now();
            let pause_after_ms = state.pause_after_ms;
            let mut deliveries = Vec::new();
            for (pane, entry) in &mut state.panes {
                while !entry.pending.is_empty() {
                    let (age_ms, bytes) =
                        drain_control_pane_output(&mut entry.pending, CONTROL_BUFFER_HIGH, now);
                    deliveries.push(control_output_payload(*pane, pause_after_ms, age_ms, bytes));
                }
                entry.release();
            }
            deliveries
        };
        for payload in deliveries {
            if !mailbox.enqueue_reliable(&Shared::event(payload)) {
                self.state.lock().clear();
                return;
            }
        }
    }

    pub(super) fn pump(
        &self,
        mailbox: &OutboundMailbox,
        pause_after_ms: Option<u64>,
        now: Instant,
    ) -> FeedPump {
        let _delivery = self.delivery.lock();
        let mut outcome = FeedPump::default();
        loop {
            let Some((queued_bytes, queued_messages)) = mailbox.queued_reliable() else {
                let mut state = self.state.lock();
                state.closed = true;
                state.clear();
                return outcome;
            };
            let mut deliveries = Vec::new();
            {
                let mut state = self.state.lock();
                let pending = state
                    .panes
                    .iter()
                    .filter_map(|(pane, entry)| (!entry.pending.is_empty()).then_some(*pane))
                    .collect::<Vec<_>>();
                for pane in pending {
                    let entry = state.panes.get_mut(&pane).expect("pending pane");
                    let enqueued_at = entry.pending.front().expect("pending chunk").enqueued_at;
                    match control_output_age_action(pause_after_ms, enqueued_at, now) {
                        ControlOutputAgeAction::Output(_) => {}
                        ControlOutputAgeAction::Pause => {
                            entry.clear();
                            state.muted.insert(pane);
                            outcome.paused.push(pane);
                            deliveries.push(EventPayload::PaneOutputState { pane, paused: true });
                        }
                        ControlOutputAgeAction::Kill => {
                            outcome.kill = true;
                            break;
                        }
                    }
                }
                if outcome.kill {
                    state.clear();
                    return outcome;
                }
                let pending = state
                    .panes
                    .iter()
                    .filter_map(|(pane, entry)| (!entry.pending.is_empty()).then_some(*pane))
                    .collect::<Vec<_>>();
                if !pending.is_empty()
                    && queued_bytes < CONTROL_BUFFER_HIGH
                    && queued_messages < CONTROL_PENDING_MESSAGE_LIMIT
                {
                    let limit = ((CONTROL_BUFFER_HIGH - queued_bytes) / pending.len() / 3)
                        .max(CONTROL_WRITE_MINIMUM);
                    for pane in pending {
                        let entry = state.panes.get_mut(&pane).expect("pending pane");
                        let (age_ms, bytes) =
                            drain_control_pane_output(&mut entry.pending, limit, now);
                        entry.release();
                        deliveries.push(control_output_payload(
                            pane,
                            pause_after_ms,
                            age_ms,
                            bytes,
                        ));
                    }
                }
                outcome.pending = state.pending();
            }
            if deliveries.is_empty() {
                break;
            }
            outcome.progressed = true;
            for payload in deliveries {
                if !mailbox.enqueue_reliable(&Shared::event(payload)) {
                    self.state.lock().clear();
                    outcome.pending = false;
                    return outcome;
                }
            }
        }
        outcome
    }
}

fn control_output_payload(
    pane: PaneId,
    pause_after_ms: Option<u64>,
    age_ms: u64,
    bytes: Vec<u8>,
) -> EventPayload {
    if pause_after_ms.is_some() {
        EventPayload::PaneOutputAged {
            pane,
            age_ms,
            bytes,
        }
    } else {
        EventPayload::PaneOutput { pane, bytes }
    }
}

pub(super) struct PipeFeed {
    state: Mutex<PipeState>,
    ready: Condvar,
    notify: Box<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct PipeState {
    chunks: VecDeque<Arc<[u8]>>,
    waiter: Option<OutputWake>,
    closed: bool,
    ended: bool,
}

impl PipeState {
    fn release(&mut self) {
        if self.chunks.len() < CONTROL_PENDING_CHUNKS_PER_PANE
            && let Some(waiter) = self.waiter.take()
        {
            waiter.wake();
        }
    }
}

impl PipeFeed {
    pub(super) fn new(notify: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(PipeState::default()),
            ready: Condvar::new(),
            notify: Box::new(notify),
        })
    }

    pub(super) fn reader(self: &Arc<Self>) -> PipeReader {
        PipeReader(Arc::clone(self))
    }

    fn push(&self, bytes: &Arc<[u8]>, waiter: &OutputWake) -> bool {
        let mut state = self.state.lock();
        if state.closed || state.ended {
            return false;
        }
        state.chunks.push_back(Arc::clone(bytes));
        if state.chunks.len() >= CONTROL_PENDING_CHUNKS_PER_PANE {
            state.waiter = Some(waiter.clone());
        }
        drop(state);
        self.ready.notify_one();
        (self.notify)();
        true
    }

    fn room(&self, waiter: &OutputWake) -> usize {
        let mut state = self.state.lock();
        if state.closed || state.ended {
            return usize::MAX;
        }
        let room = CONTROL_PENDING_CHUNKS_PER_PANE.saturating_sub(state.chunks.len());
        if room == 0 {
            state.waiter = Some(waiter.clone());
        }
        room
    }

    pub(super) fn end(&self) {
        let mut state = self.state.lock();
        state.ended = true;
        state.release();
        drop(state);
        self.ready.notify_all();
        (self.notify)();
    }
}

pub(super) struct PipeReader(Arc<PipeFeed>);

impl PipeReader {
    #[cfg(unix)]
    pub(super) fn try_recv(&self) -> Result<Arc<[u8]>, crossbeam_channel::TryRecvError> {
        let mut state = self.0.state.lock();
        if let Some(bytes) = state.chunks.pop_front() {
            state.release();
            return Ok(bytes);
        }
        Err(if state.ended {
            crossbeam_channel::TryRecvError::Disconnected
        } else {
            crossbeam_channel::TryRecvError::Empty
        })
    }

    #[cfg(windows)]
    pub(super) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Arc<[u8]>, crossbeam_channel::RecvTimeoutError> {
        let mut state = self.0.state.lock();
        if state.chunks.is_empty() && !state.ended {
            let _ = self.0.ready.wait_for(&mut state, timeout);
        }
        if let Some(bytes) = state.chunks.pop_front() {
            state.release();
            return Ok(bytes);
        }
        Err(if state.ended {
            crossbeam_channel::RecvTimeoutError::Disconnected
        } else {
            crossbeam_channel::RecvTimeoutError::Timeout
        })
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        let mut state = self.0.state.lock();
        state.closed = true;
        state.chunks.clear();
        state.release();
    }
}
