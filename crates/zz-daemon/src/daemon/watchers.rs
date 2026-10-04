use super::*;
use zz_terminal::{ScrollbarState, SearchStatus};

pub(super) type Effect = Box<dyn FnOnce(&Arc<Shared>) + Send>;

#[derive(Clone)]
pub(super) struct Sender {
    sender: crossbeam_channel::Sender<Input>,
    pub(super) wake: Arc<AcceptWake>,
    next: Arc<AtomicU64>,
    pending_wake: Arc<AtomicBool>,
}

pub(super) enum Input {
    Started(Box<Watcher>),
    Ready(u64),
    Completed(u64),
    ImagesCompleted(u64, bool),
}

impl Sender {
    pub(super) fn new() -> (Self, crossbeam_channel::Receiver<Input>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        (
            Self {
                sender,
                wake: Arc::new(AcceptWake::new()),
                next: Arc::new(AtomicU64::new(1)),
                pending_wake: Arc::new(AtomicBool::new(false)),
            },
            receiver,
        )
    }

    fn send(&self, input: Input) -> bool {
        if self.sender.send(input).is_err() {
            return false;
        }
        self.notify_loop();
        true
    }

    fn notify_loop(&self) {
        if !self.pending_wake.swap(true, Ordering::AcqRel) {
            self.wake.wake();
        }
    }

    pub(super) fn register(
        &self,
        mut watcher: Watcher,
        events: TerminalEvents,
        prefetched: VecDeque<DeferredTerminalEvent>,
    ) -> Result<(), DaemonError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        watcher.id = id;
        watcher.pending = prefetched;
        let notifications = events.clone();
        watcher.events = Some(events);
        let notified = Arc::clone(&watcher.notified);
        let terminal = watcher.terminal.clone();
        if !self.send(Input::Started(Box::new(watcher))) {
            return Err(DaemonError::Thread("watcher loop stopped".to_owned()));
        }
        let sender = self.clone();
        notifications.install_notification_sink(move || {
            if notified.swap(true, Ordering::AcqRel) {
                return;
            }
            if terminal.strong_count() == 0 {
                let _ = sender.sender.send(Input::Ready(id));
            } else {
                sender.send(Input::Ready(id));
            }
        });
        Ok(())
    }
}

pub(super) struct Watcher {
    id: u64,
    terminal: Weak<TerminalSession>,
    surface: Surface,
    pending: VecDeque<DeferredTerminalEvent>,
    events: Option<TerminalEvents>,
    notified: Arc<AtomicBool>,
    drained: bool,
    busy: bool,
    closed: bool,
    stopped: bool,
    frame: Option<FrameSnapshot>,
    admitted: Option<TerminalEvent>,
}

struct FrameSnapshot {
    runtime: Arc<TerminalViewport>,
    views: Vec<(TerminalViewId, Arc<TerminalViewport>, Option<u64>)>,
    sunk: Vec<TerminalViewId>,
}

impl FrameSnapshot {
    fn capture(terminal: &TerminalSession) -> Self {
        let runtime = terminal.latest_viewport();
        let (views, sunk) = terminal.latest_frames();
        Self {
            runtime,
            views,
            sunk,
        }
    }

    fn viewport_for(&self, view: TerminalViewId) -> Option<Arc<TerminalViewport>> {
        self.views
            .iter()
            .find(|(current, _, _)| *current == view)
            .map(|(_, viewport, _)| Arc::clone(viewport))
    }
}

enum Surface {
    Terminal(Box<TerminalWatcher>),
    Command(CommandWatcher),
    Popup(Box<PopupWatcher>),
}

struct TerminalWatcher {
    pane: PaneId,
    previous: BTreeMap<TerminalViewId, (u64, Arc<TerminalViewport>)>,
    previous_title: Option<String>,
    previous_title_writes: u64,
    projects_agent: bool,
    previous_bar_state: ProgressBarState,
    fanout: PaneFrameFanout,
    mode_memo: BTreeMap<TerminalViewId, (u8, ScrollbarState, Option<SearchStatus>)>,
    completion_handled: bool,
    previous_reported_path: Option<String>,
}

struct CommandWatcher {
    client: ClientId,
    pane: PaneId,
    admitted_generation: Option<TerminalGeneration>,
    presented: Option<(u8, ScrollbarState, Option<SearchStatus>)>,
}

struct PopupWatcher {
    client: ClientId,
    previous: Option<Arc<TerminalViewport>>,
    fanout: PaneFrameFanout,
}

impl Watcher {
    fn new(terminal: &Arc<TerminalSession>, surface: Surface) -> Self {
        Self {
            id: 0,
            terminal: Arc::downgrade(terminal),
            surface,
            pending: VecDeque::new(),
            events: None,
            notified: Arc::new(AtomicBool::new(false)),
            drained: false,
            busy: false,
            closed: false,
            stopped: false,
            frame: None,
            admitted: None,
        }
    }

    pub(super) fn terminal(
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
        projects_agent: bool,
    ) -> Self {
        Self::new(
            terminal,
            Surface::Terminal(Box::new(TerminalWatcher {
                pane,
                previous: BTreeMap::new(),
                previous_title: None,
                previous_title_writes: 0,
                projects_agent,
                previous_bar_state: ProgressBarState::Hidden,
                fanout: PaneFrameFanout::new(),
                mode_memo: BTreeMap::new(),
                completion_handled: false,
                previous_reported_path: None,
            })),
        )
    }

    pub(super) fn command_output(
        client: ClientId,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
        admitted_generation: Option<TerminalGeneration>,
    ) -> Self {
        Self::new(
            terminal,
            Surface::Command(CommandWatcher {
                client,
                pane,
                admitted_generation,
                presented: None,
            }),
        )
    }

    pub(super) fn popup(client: ClientId, terminal: &Arc<TerminalSession>) -> Self {
        Self::new(
            terminal,
            Surface::Popup(Box::new(PopupWatcher {
                client,
                previous: None,
                fanout: PaneFrameFanout::new(),
            })),
        )
    }

    fn current(&self, shared: &Shared, terminal: &Arc<TerminalSession>) -> bool {
        match &self.surface {
            Surface::Terminal(surface) => shared.is_current_terminal(surface.pane, terminal),
            Surface::Command(surface) => shared.is_current_command_output(surface.client, terminal),
            Surface::Popup(surface) => shared.is_current_popup(surface.client, terminal),
        }
    }

    fn handle(
        &mut self,
        shared: &Arc<Shared>,
        terminal: &Arc<TerminalSession>,
        event: TerminalEvent,
    ) -> bool {
        let frame = self.frame.take();
        match &mut self.surface {
            Surface::Terminal(surface) => surface.handle(shared, terminal, event, frame),
            Surface::Command(surface) => surface.handle(shared, terminal, event, frame),
            Surface::Popup(surface) => surface.handle(shared, terminal, event, frame),
        }
    }
}

impl CommandWatcher {
    fn handle(
        &mut self,
        shared: &Arc<Shared>,
        terminal: &Arc<TerminalSession>,
        event: TerminalEvent,
        frame: Option<FrameSnapshot>,
    ) -> bool {
        let client = self.client;
        let pane = self.pane;
        match event {
            TerminalEvent::ViewportReady { .. } => {
                let frame = frame.expect("viewport notification frame");
                if let Some(viewport) = frame.viewport_for(TerminalViewId(client.0)) {
                    let key = (
                        mode_kind(viewport.mode),
                        viewport.scrollbar,
                        viewport.search,
                    );
                    if self.presented != Some(key) {
                        self.presented = Some(key);
                        shared
                            .status
                            .lock()
                            .request_mode_refresh(BTreeSet::from([client]));
                    }
                    if self.admitted_generation == Some(viewport_generation(&viewport)) {
                        return true;
                    }
                    self.admitted_generation = None;
                    shared.publish_command_output(client, pane, terminal, &viewport);
                }
            }
            TerminalEvent::CopyReady { view, copy } if view == TerminalViewId(client.0) => {
                let copy = *copy;
                if let Some(buffer) = copy.buffer {
                    shared.store_copy_buffer(copy.text.clone(), buffer);
                }
                if let Some(command) = copy.pipe {
                    let text = copy.text.clone();
                    shared.defer_watcher_effect(move |shared| {
                        shared.spawn_copy_pipe(pane, client, command, text);
                    });
                }
                if let Some(target) = copy.clipboard {
                    shared.publish_to_client(
                        client,
                        EventPayload::Clipboard {
                            pane,
                            request_id: copy.request_id,
                            target,
                            text: copy.text,
                            producer: ClipboardProducer::Server,
                        },
                    );
                    shared.raise_copy_mode_set_clipboard(pane);
                }
            }
            TerminalEvent::OpenUri(open) if open.view == TerminalViewId(client.0) => {
                shared.publish_to_client(
                    client,
                    EventPayload::OpenUri {
                        pane,
                        uri: open.uri,
                    },
                );
            }
            TerminalEvent::ViewClosed(view) if view == TerminalViewId(client.0) => {
                shared.close_command_output(client, terminal);
                return false;
            }
            TerminalEvent::CopyReady { .. }
            | TerminalEvent::OpenUri(_)
            | TerminalEvent::ViewClosed(_)
            | TerminalEvent::ClipboardSet { .. }
            | TerminalEvent::Bell
            | TerminalEvent::RenameWindow(_)
            | TerminalEvent::PlaceholderBound { .. }
            | TerminalEvent::PendingPasteExpired { .. } => {}
        }
        true
    }
}

impl PopupWatcher {
    fn handle(
        &mut self,
        shared: &Arc<Shared>,
        terminal: &Arc<TerminalSession>,
        event: TerminalEvent,
        frame: Option<FrameSnapshot>,
    ) -> bool {
        let client = self.client;
        match event {
            TerminalEvent::ViewportReady { .. } => {
                let frame = frame.expect("viewport notification frame");
                let viewport = frame
                    .viewport_for(TerminalViewId(client.0))
                    .unwrap_or(frame.runtime);
                shared.publish_popup_terminal(
                    client,
                    terminal,
                    self.previous.as_deref(),
                    &viewport,
                    &mut self.fanout,
                );
                self.fanout.release();
                self.previous = Some(viewport);
                if let Some(exit_code) = popup_exit_code(terminal) {
                    shared.finish_popup(client, terminal, exit_code);
                    return false;
                }
            }
            TerminalEvent::ClipboardSet { target, text } => {
                let pane = shared.read_client(client, |c| {
                    c.and_then(|c| c.popup.as_ref())
                        .map(|popup| popup.state.pane)
                });
                if let Some(pane) = pane {
                    shared.publish_to_client(
                        client,
                        EventPayload::Clipboard {
                            pane,
                            request_id: 0,
                            target,
                            text,
                            producer: ClipboardProducer::Application,
                        },
                    );
                }
            }
            TerminalEvent::OpenUri(open) if open.view == TerminalViewId(client.0) => {
                let pane = shared.read_client(client, |c| {
                    c.and_then(|c| c.popup.as_ref())
                        .map(|popup| popup.state.pane)
                });
                if let Some(pane) = pane {
                    shared.publish_to_client(
                        client,
                        EventPayload::OpenUri {
                            pane,
                            uri: open.uri,
                        },
                    );
                }
            }
            TerminalEvent::CopyReady { view, copy } if view == TerminalViewId(client.0) => {
                let copy = *copy;
                if let Some(buffer) = copy.buffer {
                    shared.store_copy_buffer(copy.text.clone(), buffer);
                }
                if let Some(target) = copy.clipboard
                    && let Some(pane) = shared.read_client(client, |c| {
                        c.and_then(|c| c.popup.as_ref())
                            .map(|popup| popup.state.pane)
                    })
                {
                    shared.publish_to_client(
                        client,
                        EventPayload::Clipboard {
                            pane,
                            request_id: copy.request_id,
                            target,
                            text: copy.text,
                            producer: ClipboardProducer::Server,
                        },
                    );
                }
            }
            TerminalEvent::ViewClosed(_)
            | TerminalEvent::CopyReady { .. }
            | TerminalEvent::OpenUri(_)
            | TerminalEvent::Bell
            | TerminalEvent::RenameWindow(_)
            | TerminalEvent::PlaceholderBound { .. }
            | TerminalEvent::PendingPasteExpired { .. } => {}
        }
        true
    }
}

impl TerminalWatcher {
    fn handle(
        &mut self,
        shared: &Arc<Shared>,
        terminal: &Arc<TerminalSession>,
        event: TerminalEvent,
        frame: Option<FrameSnapshot>,
    ) -> bool {
        let pane = self.pane;
        match event {
            TerminalEvent::ViewportReady { output_activity } => {
                let frame = frame.expect("viewport notification frame");
                let current = frame.views;
                let sunk = frame.sunk;
                let runtime_viewport = frame.runtime;
                let active = current
                    .iter()
                    .map(|(view, _, _)| *view)
                    .collect::<BTreeSet<_>>();
                let mut finished = terminal_status_should_close(&runtime_viewport.status);
                let mut mode_clients = BTreeSet::new();
                for frame in current {
                    finished |= terminal_status_should_close(&frame.1.status);
                    if !sunk.contains(&frame.0) {
                        shard_sink::publish_loop_view(
                            shared,
                            terminal,
                            pane,
                            &frame,
                            self.previous.get(&frame.0),
                            &mut self.fanout,
                        );
                    }
                    let (view, viewport, epoch) = frame;
                    let key = (
                        mode_kind(viewport.mode),
                        viewport.scrollbar,
                        viewport.search,
                    );
                    let before = self.mode_memo.insert(view, key);
                    if before != Some(key)
                        && (key.0 != 0 || before.is_some_and(|before| before.0 != 0))
                    {
                        mode_clients.insert(ClientId(view.0));
                    }
                    match epoch {
                        Some(epoch) => {
                            self.previous.insert(view, (epoch, viewport));
                        }
                        None => {
                            self.previous.remove(&view);
                        }
                    }
                }
                self.fanout.release();
                self.mode_memo.retain(|view, _| active.contains(view));
                self.previous.retain(|view, _| active.contains(view));
                if !terminal_status_should_close(&runtime_viewport.status) {
                    if output_activity {
                        shared.note_pane_output(pane, terminal, Instant::now());
                    } else {
                        shared.note_pane_frame(pane, terminal, Instant::now());
                    }
                    let reported_path = runtime_viewport.working_directory().unwrap_or_default();
                    if self.previous_reported_path.as_deref() != Some(reported_path) {
                        shared.synchronize_pane_reported_path(pane, terminal, reported_path);
                        self.previous_reported_path = Some(reported_path.to_owned());
                    }
                    let bar_state = terminal.progress_bar().state;
                    if !self.projects_agent && self.previous_bar_state != bar_state {
                        self.previous_bar_state = bar_state;
                        let terminal = Arc::clone(terminal);
                        shared.defer_watcher_effect(move |shared| {
                            let current_command = terminal_current_command(&terminal);
                            shared.synchronize_pane_progress(
                                pane,
                                &terminal,
                                &current_command,
                                bar_state,
                            );
                        });
                    }
                }
                let title_writes = terminal.facts().program_title_writes;
                if !self.projects_agent
                    && (self.previous_title_writes != title_writes
                        || self
                            .previous_title
                            .as_deref()
                            .is_none_or(|previous| previous != runtime_viewport.title()))
                {
                    shared.synchronize_pane_title(
                        pane,
                        terminal,
                        runtime_viewport.title(),
                        self.previous_title_writes != title_writes,
                    );
                    self.previous_title = Some(runtime_viewport.title().to_owned());
                    self.previous_title_writes = title_writes;
                }
                if terminal.take_preview_ready() {
                    shared.refresh_chooser_previews();
                }
                if !mode_clients.is_empty() {
                    shared.status.lock().request_mode_refresh(mode_clients);
                }
                if finished && !self.completion_handled {
                    let terminal = Arc::clone(terminal);
                    shared.defer_watcher_effect(move |shared| {
                        shared.close_exited_terminal(pane, &terminal);
                    });
                    self.completion_handled = true;
                }
            }
            TerminalEvent::CopyReady { view, copy } => {
                let client = ClientId(view.0);
                let copy = *copy;
                if let Some(buffer) = copy.buffer {
                    shared.store_copy_buffer(copy.text.clone(), buffer);
                }
                if let Some(command) = copy.pipe {
                    let text = copy.text.clone();
                    shared.defer_watcher_effect(move |shared| {
                        shared.spawn_copy_pipe(pane, client, command, text);
                    });
                }
                if let Some(target) = copy.clipboard {
                    shared.publish_to_client(
                        client,
                        EventPayload::Clipboard {
                            pane,
                            request_id: copy.request_id,
                            target,
                            text: copy.text,
                            producer: ClipboardProducer::Server,
                        },
                    );
                    shared.raise_copy_mode_set_clipboard(pane);
                }
            }
            TerminalEvent::OpenUri(open) => {
                shared.publish_to_client(
                    ClientId(open.view.0),
                    EventPayload::OpenUri {
                        pane,
                        uri: open.uri,
                    },
                );
            }
            TerminalEvent::PlaceholderBound { token, number } => {
                let terminal = Arc::clone(terminal);
                shared.defer_watcher_effect(move |shared| {
                    shared.bind_pasted_image(pane, &terminal, token, number);
                });
            }
            TerminalEvent::PendingPasteExpired { token } => {
                shared.expire_pending_pasted_image(pane, terminal, token);
            }
            TerminalEvent::ClipboardSet { target, text } => {
                shared.deliver_clipboard_write(pane, target, text);
            }
            TerminalEvent::Bell => shared.raise_pane_bell(pane),
            TerminalEvent::RenameWindow(name) => {
                let terminal = Arc::clone(terminal);
                shared.defer_watcher_effect(move |shared| {
                    shared.rename_window_from_pane(pane, &terminal, &name);
                });
            }
            TerminalEvent::ViewClosed(_) => {}
        }
        true
    }
}

pub(super) struct LoopWatchers {
    inputs: Option<crossbeam_channel::Receiver<Input>>,
    surfaces: BTreeMap<u64, Box<Watcher>>,
}

impl LoopWatchers {
    pub(super) fn new(shared: &Shared) -> Self {
        Self {
            inputs: shared.watcher_rx.lock().take(),
            surfaces: BTreeMap::new(),
        }
    }

    pub(super) fn turn(&mut self, shared: &Arc<Shared>) {
        shared
            .watcher_tx
            .pending_wake
            .store(false, Ordering::Release);
        for _ in 0..256 {
            let Some(input) = self
                .inputs
                .as_ref()
                .and_then(|inputs| inputs.try_recv().ok())
            else {
                return;
            };
            self.input(shared, input);
        }
        if self
            .inputs
            .as_ref()
            .is_some_and(|inputs| !inputs.is_empty())
        {
            shared.watcher_tx.notify_loop();
        }
    }

    fn schedule(shared: &Shared, watcher: &Watcher) {
        if !watcher.notified.swap(true, Ordering::AcqRel) {
            shared.watcher_tx.send(Input::Ready(watcher.id));
        }
    }

    fn input(&mut self, shared: &Arc<Shared>, input: Input) {
        let id = match input {
            Input::Started(watcher) => {
                self.surfaces.insert(watcher.id, watcher);
                return;
            }
            Input::ImagesCompleted(id, success) => {
                if let Some(watcher) = self.surfaces.get_mut(&id) {
                    watcher.busy = false;
                    if !success {
                        watcher.frame = None;
                        watcher.admitted = None;
                    }
                    Self::schedule(shared, watcher);
                }
                return;
            }
            Input::Ready(id) => id,
            Input::Completed(id) => {
                if let Some(watcher) = self.surfaces.get_mut(&id) {
                    watcher.busy = false;
                    Self::schedule(shared, watcher);
                }
                return;
            }
        };
        let Some(watcher) = self.surfaces.get_mut(&id) else {
            return;
        };
        watcher.notified.store(false, Ordering::Release);
        if watcher.busy {
            return;
        }
        watcher.drained = false;
        Self::advance(shared, watcher);
        if watcher.closed && !watcher.busy {
            self.surfaces.remove(&id);
        } else if !watcher.busy && !watcher.closed && !watcher.drained {
            Self::schedule(shared, watcher);
        }
    }

    fn execute(shared: &Arc<Shared>, watcher: &mut Watcher, effects: Vec<Effect>) {
        watcher.busy = true;
        let id = watcher.id;
        let sender = shared.watcher_tx.clone();
        let owner = shared.server_owner();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for effect in effects {
                effect(&owner);
            }
        }));
        if result.is_err() {
            log::error!("watcher effect panicked: {id}");
        }
        sender.send(Input::Completed(id));
    }

    fn drain_stopped(watcher: &mut Watcher) {
        watcher.pending.clear();
        watcher.admitted = None;
        watcher.frame = None;
        for _ in 0..8 {
            match watcher
                .events
                .as_ref()
                .map(TerminalEvents::try_recv_deferred)
            {
                Some(Ok(event)) => drop(event),
                Some(Err(error)) => {
                    watcher.closed = error.is_closed();
                    watcher.drained = true;
                    break;
                }
                None => {
                    watcher.drained = true;
                    break;
                }
            }
        }
    }

    fn advance(shared: &Arc<Shared>, watcher: &mut Watcher) {
        if watcher.busy {
            return;
        }
        let Some(terminal) = watcher.terminal.upgrade() else {
            watcher.stopped = true;
            Self::drain_stopped(watcher);
            return;
        };
        if !watcher.current(shared, &terminal) {
            watcher.stopped = true;
        }
        if watcher.stopped {
            Self::drain_stopped(watcher);
            return;
        }
        for _ in 0..8 {
            if watcher.admitted.is_none() && watcher.pending.is_empty() {
                match watcher
                    .events
                    .as_ref()
                    .map(TerminalEvents::try_recv_deferred)
                {
                    Some(Ok(event)) => watcher.pending.push_back(event),
                    Some(Err(error)) if error.is_closed() => watcher.closed = true,
                    _ => {}
                }
                if watcher.pending.is_empty() {
                    watcher.drained = true;
                    break;
                }
            }
            if !watcher.current(shared, &terminal) {
                watcher.stopped = true;
                Self::drain_stopped(watcher);
                return;
            }
            if watcher.admitted.is_none() {
                watcher.admitted = watcher
                    .pending
                    .pop_front()
                    .map(DeferredTerminalEvent::into_event);
            }
            if watcher.frame.is_none()
                && matches!(watcher.admitted, Some(TerminalEvent::ViewportReady { .. }))
            {
                let frame = FrameSnapshot::capture(&terminal);
                let pane = match &watcher.surface {
                    Surface::Terminal(surface) => Some(surface.pane),
                    Surface::Command(surface) => Some(surface.pane),
                    Surface::Popup(surface) => shared.read_client(surface.client, |c| {
                        c.and_then(|c| c.popup.as_ref())
                            .map(|popup| popup.state.pane)
                    }),
                };
                if let Some(pane) = pane {
                    let images = frame
                        .views
                        .iter()
                        .flat_map(|(_, viewport, _)| {
                            viewport
                                .kitty_placements
                                .iter()
                                .map(|placement| (placement.image_id, placement.image_generation))
                        })
                        .collect::<BTreeSet<_>>();
                    if !images.is_empty()
                        || shared
                            .kitty_image_frames
                            .lock()
                            .keys()
                            .any(|key| key.pane == pane)
                    {
                        shared.evict_absent_kitty_images(pane, &terminal, &images);
                        let id = watcher.id;
                        let sender = shared.watcher_tx.clone();
                        if !shared.request_kitty_images(
                            pane,
                            &terminal,
                            &images,
                            move |_, success| {
                                sender.send(Input::ImagesCompleted(id, success));
                            },
                        ) {
                            watcher.frame = Some(frame);
                            watcher.busy = true;
                            return;
                        }
                    }
                }
                watcher.frame = Some(frame);
            }
            let event = watcher.admitted.take().unwrap();
            let context = shared.watcher_context();
            let _round_trips = zz_terminal::forbid_actor_round_trips();
            watcher.stopped = !watcher.handle(&context, &terminal, event);
            let effects = context
                .watcher_effects
                .as_ref()
                .unwrap()
                .lock()
                .drain(..)
                .collect::<Vec<_>>();
            if watcher.stopped {
                watcher.pending.clear();
            }
            if !effects.is_empty() {
                Self::execute(shared, watcher, effects);
                return;
            }
            if watcher.stopped {
                Self::drain_stopped(watcher);
                return;
            }
        }
        if watcher.closed {
            let effect: Option<Effect> = match &watcher.surface {
                Surface::Terminal(surface)
                    if !surface.completion_handled
                        && terminal_status_should_close(&terminal.latest_viewport().status) =>
                {
                    let pane = surface.pane;
                    Some(Box::new(move |shared| {
                        shared.close_exited_terminal(pane, &terminal);
                    }))
                }
                Surface::Popup(surface) => {
                    if let Some(code) = popup_exit_code(&terminal) {
                        shared.finish_popup(surface.client, &terminal, code);
                    }
                    None
                }
                _ => None,
            };
            watcher.stopped = true;
            if let Some(effect) = effect {
                Self::execute(shared, watcher, vec![effect]);
            }
        }
    }
}

impl Shared {
    fn watcher_context(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self {
            server: Arc::clone(&self.server),
            command_item: None,
            owner: Some(self.server_owner()),
            watcher_effects: Some(Mutex::new(Vec::new())),
        })
    }

    fn defer_watcher_effect(&self, effect: impl FnOnce(&Arc<Shared>) + Send + 'static) {
        self.watcher_effects
            .as_ref()
            .expect("loop watcher context")
            .lock()
            .push(Box::new(effect));
    }

    #[cfg(any(test, windows))]
    pub(super) fn start_watcher_consumer(self: &Arc<Self>) -> Result<(), DaemonError> {
        #[cfg(any(test, windows))]
        {
            let mut watchers = LoopWatchers::new(self);
            if watchers.inputs.is_none() {
                return Ok(());
            }
            #[cfg(feature = "agent")]
            let mut agents = agent_inbox::AgentInbox::new(self);
            let owner = Arc::downgrade(&self.server_owner());
            thread::Builder::new()
                .name("zz-watchers".to_owned())
                .spawn(move || {
                    loop {
                        #[cfg(feature = "agent")]
                        let input = {
                            let never = crossbeam_channel::never();
                            let agent_rx = agents.receiver.as_ref().unwrap_or(&never);
                            crossbeam_channel::select! {
                                recv(watchers.inputs.as_ref().unwrap()) -> input => {
                                    let Ok(input) = input else { return; };
                                    Some(input)
                                }
                                default(Duration::from_millis(5)) => { None }
                                recv(agent_rx) -> message => {
                                    let Ok(message) = message else { return; };
                                    let Some(owner) = owner.upgrade() else { return; };
                                    agents.apply(&owner, message);
                                    agents.turn(&owner);
                                    None
                                }
                            }
                        };
                        #[cfg(not(feature = "agent"))]
                        let input = {
                            match watchers
                                .inputs
                                .as_ref()
                                .unwrap()
                                .recv_timeout(Duration::from_millis(5))
                            {
                                Ok(input) => Some(input),
                                Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
                                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                            }
                        };
                        let Some(owner) = owner.upgrade() else {
                            return;
                        };
                        owner.terminal_requests.turn(&owner);
                        if let Some(input) = input {
                            watchers.input(&owner, input);
                        }
                        watchers.turn(&owner);
                    }
                })
                .map_err(|error| DaemonError::Thread(error.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "watcher_tests.rs"]
mod tests;
