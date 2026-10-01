use std::sync::LazyLock;

use super::*;

pub(super) const PUBLISH_FLUSH_INTERVAL: Duration = Duration::from_millis(16);

pub(super) const PEER_PROBE_INTERVAL: Duration = Duration::from_secs(1);

pub(super) static EAGER_PUBLISH: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_EAGER_PUBLISH").is_some_and(|value| value == "1"));

pub(super) static RENAME_THROTTLE: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_RENAME_THROTTLE").is_none_or(|value| value != "0"));

pub(super) static KEY_TABLE_DELTA: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_KEY_TABLE_DELTA").is_none_or(|value| value != "0"));

pub(super) static PEER_SCAN_ALWAYS: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_PEER_SCAN").is_some_and(|value| value == "always"));

thread_local! {
    static KEY_TABLE_PUBLISH_HOLD: Cell<u32> = const { Cell::new(0) };
}

pub(super) struct KeyTablePublishHold;

impl KeyTablePublishHold {
    pub(super) fn enter() -> Self {
        KEY_TABLE_PUBLISH_HOLD.with(|hold| hold.set(hold.get() + 1));
        Self
    }

    pub(super) fn active() -> bool {
        KEY_TABLE_PUBLISH_HOLD.with(|hold| hold.get() != 0)
    }
}

impl Drop for KeyTablePublishHold {
    fn drop(&mut self) {
        KEY_TABLE_PUBLISH_HOLD.with(|hold| hold.set(hold.get().saturating_sub(1)));
    }
}

pub(super) enum TimerCommand {
    Rename(Instant),
    PublishFlush(Instant),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PublishReason {
    Tree,
    RuntimeFacts,
}

#[derive(Default)]
pub(super) struct PublishFlush {
    tree: bool,
    runtime_facts: bool,
    scheduled: bool,
    last: Option<Instant>,
}

impl PublishFlush {
    fn take(&mut self) -> Option<PublishReason> {
        let reason = if self.tree {
            Some(PublishReason::Tree)
        } else {
            self.runtime_facts.then_some(PublishReason::RuntimeFacts)
        };
        self.tree = false;
        self.runtime_facts = false;
        reason
    }
}

#[derive(Default)]
pub(super) struct PeerProbe {
    last: Option<Instant>,
    follow_up: bool,
}

#[derive(Default)]
pub(super) struct HookWorker {
    jobs: VecDeque<Vec<PendingHookEvent>>,
    running: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TimerKey {
    DisplayPanes(ClientId),
    KeyTable(ClientId),
    Silence(WindowId),
    ClientMessage(ClientId),
    Rename,
    PublishFlush,
}

#[derive(Clone, Copy)]
enum Expiry {
    DisplayPanes(DisplayPanesDeadline),
    KeyTable(ClientId),
    Silence(SilenceDeadline),
    ClientMessage(ClientMessageDeadline),
    Rename,
    PublishFlush,
}

enum TimerInput {
    DisplayPanes(DisplayPanesDeadlineCommand),
    KeyTable(KeyTableDeadlineCommand),
    Silence(SilenceDeadlineCommand),
    ClientMessage(ClientMessageDeadlineCommand),
    Timer(TimerCommand),
}

#[derive(Default)]
struct Deadlines {
    order: BTreeSet<(Instant, TimerKey)>,
    entries: BTreeMap<TimerKey, (Instant, Expiry)>,
}

impl Deadlines {
    fn insert(&mut self, key: TimerKey, deadline: Instant, expiry: Expiry) {
        if let Some((previous, _)) = self.entries.insert(key, (deadline, expiry)) {
            self.order.remove(&(previous, key));
        }
        self.order.insert((deadline, key));
    }

    fn insert_earliest(&mut self, key: TimerKey, deadline: Instant, expiry: Expiry) {
        if self
            .entries
            .get(&key)
            .is_none_or(|(current, _)| deadline < *current)
        {
            self.insert(key, deadline, expiry);
        }
    }

    fn remove(&mut self, key: TimerKey) {
        if let Some((deadline, _)) = self.entries.remove(&key) {
            self.order.remove(&(deadline, key));
        }
    }

    fn get(&self, key: TimerKey) -> Option<Expiry> {
        self.entries.get(&key).map(|(_, expiry)| *expiry)
    }

    fn next(&self) -> Option<Instant> {
        self.order.first().map(|(deadline, _)| *deadline)
    }

    fn pop_due(&mut self, now: Instant) -> Option<Expiry> {
        let (deadline, key) = *self.order.first()?;
        if deadline > now {
            return None;
        }
        self.order.remove(&(deadline, key));
        self.entries.remove(&key).map(|(_, expiry)| expiry)
    }
}

impl Shared {
    pub(super) fn start_timers(self: &Arc<Self>) -> Result<(), DaemonError> {
        let receivers = (
            self.display_panes_deadline_rx.lock().take(),
            self.key_table_deadline_rx.lock().take(),
            self.silence_deadline_rx.lock().take(),
            self.client_message_deadline_rx.lock().take(),
            self.timer_rx.lock().take(),
        );
        let (
            Some(display_panes),
            Some(key_table),
            Some(silence),
            Some(client_message),
            Some(timers),
        ) = receivers
        else {
            return Ok(());
        };
        let shared = Arc::downgrade(self);
        let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
        thread::Builder::new()
            .name("zz-daemon-timers".to_owned())
            .spawn(move || {
                if ready_tx.send(()).is_err() {
                    return;
                }
                let mut select = crossbeam_channel::Select::new();
                let display_panes_index = select.recv(&display_panes);
                let key_table_index = select.recv(&key_table);
                let silence_index = select.recv(&silence);
                let client_message_index = select.recv(&client_message);
                let timers_index = select.recv(&timers);
                let mut deadlines = Deadlines::default();
                loop {
                    let now = Instant::now();
                    while let Some(expiry) = deadlines.pop_due(now) {
                        let Some(shared) = shared.upgrade() else {
                            return;
                        };
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            shared.expire_timer(expiry, now);
                        }))
                        .is_err()
                        {
                            log::error!(target: "zz_daemon::timers", "a timer expiry panicked");
                        }
                    }
                    let operation = match deadlines.next() {
                        Some(deadline) => match select.select_deadline(deadline) {
                            Ok(operation) => operation,
                            Err(_) => continue,
                        },
                        None => select.select(),
                    };
                    let index = operation.index();
                    let input = if index == display_panes_index {
                        operation.recv(&display_panes).map(TimerInput::DisplayPanes)
                    } else if index == key_table_index {
                        operation.recv(&key_table).map(TimerInput::KeyTable)
                    } else if index == silence_index {
                        operation.recv(&silence).map(TimerInput::Silence)
                    } else if index == client_message_index {
                        operation
                            .recv(&client_message)
                            .map(TimerInput::ClientMessage)
                    } else {
                        debug_assert_eq!(index, timers_index);
                        operation.recv(&timers).map(TimerInput::Timer)
                    };
                    let Ok(input) = input else {
                        return;
                    };
                    let Some(shared) = shared.upgrade() else {
                        return;
                    };
                    shared.schedule_timer(&mut deadlines, &input);
                }
            })
            .map_err(|error| DaemonError::Thread(error.to_string()))?;
        ready_rx
            .recv()
            .map_err(|error| DaemonError::Thread(error.to_string()))
    }

    fn schedule_timer(&self, deadlines: &mut Deadlines, input: &TimerInput) {
        match *input {
            TimerInput::DisplayPanes(DisplayPanesDeadlineCommand::Schedule(deadline)) => {
                if self
                    .inner
                    .lock()
                    .display_panes
                    .get(&deadline.client)
                    .is_some_and(|overlay| {
                        overlay.token == deadline.token
                            && overlay.deadline == Some(deadline.deadline)
                    })
                {
                    deadlines.insert(
                        TimerKey::DisplayPanes(deadline.client),
                        deadline.deadline,
                        Expiry::DisplayPanes(deadline),
                    );
                }
            }
            TimerInput::DisplayPanes(DisplayPanesDeadlineCommand::Cancel { client, token }) => {
                let key = TimerKey::DisplayPanes(client);
                if matches!(
                    deadlines.get(key),
                    Some(Expiry::DisplayPanes(deadline)) if deadline.token == token
                ) {
                    deadlines.remove(key);
                }
            }
            TimerInput::KeyTable(KeyTableDeadlineCommand::Schedule(client, Some(deadline))) => {
                deadlines.insert(
                    TimerKey::KeyTable(client),
                    deadline,
                    Expiry::KeyTable(client),
                );
            }
            TimerInput::KeyTable(KeyTableDeadlineCommand::Schedule(client, None)) => {
                deadlines.remove(TimerKey::KeyTable(client));
            }
            TimerInput::Silence(SilenceDeadlineCommand::Schedule(deadline)) => {
                if self
                    .inner
                    .lock()
                    .silence_deadlines
                    .get(&deadline.window)
                    .is_some_and(|current| *current == deadline)
                {
                    deadlines.insert(
                        TimerKey::Silence(deadline.window),
                        deadline.deadline,
                        Expiry::Silence(deadline),
                    );
                }
            }
            TimerInput::Silence(SilenceDeadlineCommand::Cancel { window, token }) => {
                let key = TimerKey::Silence(window);
                if matches!(
                    deadlines.get(key),
                    Some(Expiry::Silence(deadline)) if deadline.token == token
                ) {
                    deadlines.remove(key);
                }
            }
            TimerInput::ClientMessage(ClientMessageDeadlineCommand::Schedule(deadline)) => {
                if self
                    .inner
                    .lock()
                    .client_messages
                    .get(&deadline.client)
                    .is_some_and(|current| {
                        current.token == deadline.token
                            && current.deadline == Some(deadline.deadline)
                    })
                {
                    deadlines.insert(
                        TimerKey::ClientMessage(deadline.client),
                        deadline.deadline,
                        Expiry::ClientMessage(deadline),
                    );
                }
            }
            TimerInput::ClientMessage(ClientMessageDeadlineCommand::Cancel { client, token }) => {
                let key = TimerKey::ClientMessage(client);
                if matches!(
                    deadlines.get(key),
                    Some(Expiry::ClientMessage(deadline)) if deadline.token == token
                ) {
                    deadlines.remove(key);
                }
            }
            TimerInput::Timer(TimerCommand::Rename(deadline)) => {
                deadlines.insert_earliest(TimerKey::Rename, deadline, Expiry::Rename);
            }
            TimerInput::Timer(TimerCommand::PublishFlush(deadline)) => {
                deadlines.insert_earliest(TimerKey::PublishFlush, deadline, Expiry::PublishFlush);
            }
        }
    }

    fn expire_timer(self: &Arc<Self>, expiry: Expiry, now: Instant) {
        match expiry {
            Expiry::DisplayPanes(deadline) => {
                self.expire_display_panes(deadline, now);
            }
            Expiry::KeyTable(client) => self.sync_key_table(client, false),
            Expiry::Silence(deadline) => self.expire_window_silence(deadline, now),
            Expiry::ClientMessage(deadline) => self.expire_client_message(deadline, now),
            Expiry::Rename => self.apply_due_window_renames(now),
            Expiry::PublishFlush => self.flush_publish(),
        }
    }

    pub(super) fn request_publish(self: &Arc<Self>, reason: PublishReason) {
        if *EAGER_PUBLISH {
            self.publish_snapshot();
            return;
        }
        let now = Instant::now();
        let flush_now = {
            let mut flush = self.publish_flush.lock();
            match reason {
                PublishReason::Tree => flush.tree = true,
                PublishReason::RuntimeFacts => flush.runtime_facts = true,
            }
            match flush.last {
                _ if flush.scheduled => None,
                Some(last) if now < last + PUBLISH_FLUSH_INTERVAL => {
                    flush.scheduled = true;
                    let _ = self
                        .timer_tx
                        .send(TimerCommand::PublishFlush(last + PUBLISH_FLUSH_INTERVAL));
                    None
                }
                _ => {
                    flush.last = Some(now);
                    flush.take()
                }
            }
        };
        self.publish_for(flush_now);
    }

    pub(super) fn note_published(&self) {
        let mut flush = self.publish_flush.lock();
        flush.take();
        flush.scheduled = false;
        flush.last = Some(Instant::now());
    }

    fn flush_publish(self: &Arc<Self>) {
        let pending = {
            let mut flush = self.publish_flush.lock();
            flush.scheduled = false;
            flush.take()
        };
        self.publish_for(pending);
    }

    fn publish_for(self: &Arc<Self>, reason: Option<PublishReason>) {
        match reason {
            Some(PublishReason::Tree) => self.publish_snapshot(),
            Some(PublishReason::RuntimeFacts) => self.publish_runtime_facts(),
            None => {}
        }
    }

    pub(super) fn publish_runtime_facts(&self) {
        let (presentation, choosers) = {
            let inner = self.inner.lock();
            let choosers = !inner.choose_trees.is_empty();
            let presentation = (!inner.subscribers.is_empty()
                || inner.engine.has_window_style_settings()
                || *EAGER_PUBLISH)
                && inner.engine.runtime_facts_reach_presentation();
            (presentation, choosers)
        };
        if presentation {
            self.publish_mux_snapshots_as(false, true);
        }
        if choosers {
            self.refresh_choose_trees();
        }
    }

    pub(super) fn schedule_window_renames(&self, inner: &mut ServerState) {
        let Some(deadline) = inner.engine.next_window_rename_deadline() else {
            return;
        };
        if inner
            .scheduled_window_rename
            .is_some_and(|scheduled| scheduled <= deadline)
        {
            return;
        }
        inner.scheduled_window_rename = Some(deadline);
        let _ = self.timer_tx.send(TimerCommand::Rename(deadline));
    }

    pub(super) fn apply_due_window_renames(self: &Arc<Self>, now: Instant) {
        let due = {
            let inner = self.inner.lock();
            inner
                .engine
                .due_window_rename_panes(now)
                .into_iter()
                .filter_map(|pane| Some((pane, Arc::clone(inner.terminals.get(&pane)?))))
                .collect::<Vec<_>>()
        };
        let commands = due
            .into_iter()
            .map(|(pane, terminal)| (pane, terminal_current_command(&terminal)))
            .filter(|(_, command)| !command.is_empty())
            .collect::<Vec<_>>();
        let (renamed, events) = {
            let mut inner = self.inner.lock();
            inner.scheduled_window_rename = None;
            let facts = format_hook_facts(&inner);
            let mut hooks = DaemonFormatHooks::command(&facts);
            let scope = hook_events::HookScope::open(&mut inner.engine);
            let mut renamed = false;
            for (pane, command) in commands {
                let Some(mut runtime) = inner.engine.pane_runtime_facts(pane).cloned() else {
                    continue;
                };
                if runtime.current_command != command {
                    runtime.current_command = command;
                    let generation = inner.engine.state.generation();
                    inner
                        .engine
                        .set_pane_runtime_facts_at(pane, runtime, &mut hooks, now);
                    renamed |= inner.engine.state.generation() != generation;
                }
            }
            renamed |= inner.engine.apply_due_window_renames(now, &mut hooks);
            let events = if renamed {
                scope.finish(&inner.engine, "").events
            } else {
                Vec::new()
            };
            self.schedule_window_renames(&mut inner);
            (renamed, events)
        };
        if renamed {
            self.request_publish(PublishReason::Tree);
        }
        self.run_event_hooks_on_worker(events);
    }

    pub(super) fn run_event_hooks_on_worker(self: &Arc<Self>, events: Vec<PendingHookEvent>) {
        if events.is_empty() {
            return;
        }
        let mut worker = self.hook_worker.lock();
        if !worker.running && !self.event_hooks_have_commands(&events) {
            drop(worker);
            self.run_event_hooks(events);
            return;
        }
        worker.jobs.push_back(events);
        if worker.running {
            return;
        }
        worker.running = true;
        drop(worker);
        let shared = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("zz-daemon-hooks".to_owned())
            .spawn(move || {
                loop {
                    let events = {
                        let mut worker = shared.hook_worker.lock();
                        let Some(events) = worker.jobs.pop_front() else {
                            worker.running = false;
                            return;
                        };
                        events
                    };
                    shared.run_event_hooks(events);
                }
            });
        if spawned.is_err() {
            let jobs = {
                let mut worker = self.hook_worker.lock();
                worker.running = false;
                std::mem::take(&mut worker.jobs)
            };
            for events in jobs {
                self.run_event_hooks(events);
            }
        }
    }

    fn event_hooks_have_commands(&self, events: &[PendingHookEvent]) -> bool {
        let inner = self.inner.lock();
        events.iter().any(|event| {
            let mut context = event.context.clone();
            inner.engine.repair_event_context(&mut context);
            inner
                .engine
                .event_hook_commands(&context, event.name)
                .is_some()
        })
    }

    pub(super) fn status_tick_needed(inner: &ServerState) -> bool {
        inner
            .control_outputs
            .values()
            .any(|output| !output.subscriptions.is_empty())
            || inner.engine.has_format_monitors()
            || Self::peer_scan_armed(inner)
    }

    pub(super) fn status_sampler_sessions(
        inner: &ServerState,
    ) -> impl Iterator<Item = SessionId> + '_ {
        inner
            .subscribers
            .keys()
            .filter(|client| {
                inner
                    .ctrl_subscriptions
                    .get(*client)
                    .is_none_or(|subscription| subscription.status)
            })
            .filter_map(|client| client_attached_session(inner, *client))
    }

    pub(super) fn status_sampler_has_work(inner: &ServerState) -> bool {
        Self::status_tick_needed(inner)
            || Self::status_sampler_sessions(inner).any(|session| {
                !inner
                    .engine
                    .status_formats_for_session(Some(session))
                    .interval
                    .is_zero()
            })
    }

    #[cfg(all(feature = "agent", unix))]
    pub(super) fn peer_scan_armed(inner: &ServerState) -> bool {
        *PEER_SCAN_ALWAYS || !inner.claude_peer_states.is_empty()
    }

    #[cfg(not(all(feature = "agent", unix)))]
    pub(super) fn peer_scan_armed(_inner: &ServerState) -> bool {
        false
    }

    #[cfg(all(feature = "agent", unix))]
    pub(super) fn request_peer_probe(&self) {
        if self.peer_probe.load(Ordering::Relaxed) || self.peer_probe.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(sampler) = self.status_sampler.lock().as_ref() {
            sampler.unpark();
        }
    }

    #[cfg(not(all(feature = "agent", unix)))]
    pub(super) fn request_peer_probe(&self) {}

    #[cfg(all(feature = "agent", unix))]
    pub(super) fn run_peer_probe(
        self: &Arc<Self>,
        armed: bool,
        probe: &mut PeerProbe,
    ) -> Option<Instant> {
        if armed {
            return None;
        }
        let requested = self.peer_probe.load(Ordering::Acquire);
        if !requested && !probe.follow_up {
            return None;
        }
        let now = Instant::now();
        if let Some(next) = probe
            .last
            .map(|last| last + PEER_PROBE_INTERVAL)
            .filter(|next| now < *next)
        {
            return Some(next);
        }
        if requested {
            self.peer_probe.store(false, Ordering::Release);
        }
        probe.follow_up = requested;
        probe.last = Some(now);
        self.sync_claude_peer_states();
        probe.follow_up.then(|| now + PEER_PROBE_INTERVAL)
    }

    #[cfg(not(all(feature = "agent", unix)))]
    pub(super) fn run_peer_probe(
        self: &Arc<Self>,
        _armed: bool,
        _probe: &mut PeerProbe,
    ) -> Option<Instant> {
        None
    }

    pub(super) fn nudge_status_sampler(&self) {
        if !self.status_sampler_idle.load(Ordering::SeqCst) {
            return;
        }
        if !Self::status_sampler_has_work(&self.inner.lock()) {
            return;
        }
        if let Some(sampler) = self.status_sampler.lock().as_ref() {
            sampler.unpark();
        }
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        if let Some(sampler) = self.status_sampler.get_mut().take() {
            sampler.unpark();
        }
    }
}
