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
    Status(SessionId),
    Subscriptions,
    Monitors,
    Labels,
    PeerProbe,
    Diagnostics,
    #[cfg(unix)]
    Shutdown,
}

#[derive(Clone, Copy)]
enum Expiry {
    DisplayPanes(DisplayPanesDeadline),
    KeyTable(ClientId),
    Silence(SilenceDeadline),
    ClientMessage(ClientMessageDeadline),
    Rename,
    PublishFlush,
    Status(SessionId),
    Subscriptions,
    Monitors,
    Labels,
    PeerProbe,
    Diagnostics,
    #[cfg(unix)]
    Shutdown,
}

pub(super) enum TimerInput {
    DisplayPanes(DisplayPanesDeadlineCommand),
    KeyTable(KeyTableDeadlineCommand),
    Silence(SilenceDeadlineCommand),
    ClientMessage(ClientMessageDeadlineCommand),
    Timer(TimerCommand),
    ClientTimersChanged,
    StatusJobs,
}

#[derive(Clone)]
pub(super) struct TimerSender {
    sender: crossbeam_channel::Sender<TimerInput>,
    wake: Arc<AcceptWake>,
}

impl std::fmt::Debug for TimerSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimerSender")
            .field("sender", &self.sender)
            .finish_non_exhaustive()
    }
}

impl TimerSender {
    pub(super) fn new(sender: crossbeam_channel::Sender<TimerInput>) -> Self {
        Self {
            sender,
            wake: Arc::new(AcceptWake::new()),
        }
    }

    pub(super) fn send(
        &self,
        input: TimerInput,
    ) -> Result<(), crossbeam_channel::SendError<TimerInput>> {
        self.sender.send(input)?;
        self.wake.wake();
        Ok(())
    }
}

impl std::task::Wake for TimerSender {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.send(TimerInput::StatusJobs);
    }
}

#[derive(Default)]
struct ClientTimers {
    intervals: BTreeMap<SessionId, Duration>,
    probe: PeerProbe,
    monitor_running: bool,
    peer_running: bool,
}

impl ClientTimers {
    fn sync(&mut self, shared: &Shared, deadlines: &mut Deadlines, now: Instant) {
        let inner = shared.inner.lock();
        let intervals = Shared::status_timer_sessions(&inner)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|session| {
                let formats = inner.engine.status_formats_for_session(Some(session));
                (formats.enabled && !formats.interval.is_zero())
                    .then_some((session, formats.interval))
            })
            .collect::<BTreeMap<_, _>>();
        for session in self.intervals.keys() {
            if !intervals.contains_key(session) {
                deadlines.remove(TimerKey::Status(*session));
            }
        }
        for (&session, &interval) in &intervals {
            if self.intervals.get(&session) != Some(&interval)
                || deadlines.get(TimerKey::Status(session)).is_none()
            {
                deadlines.insert(
                    TimerKey::Status(session),
                    now + interval,
                    Expiry::Status(session),
                );
            }
        }
        self.intervals = intervals;
        for (key, expiry, needed, interval) in [
            (
                TimerKey::Subscriptions,
                Expiry::Subscriptions,
                Shared::control_subscriptions_needed(&inner),
                CONTROL_SUBSCRIPTION_INTERVAL,
            ),
            (
                TimerKey::Monitors,
                Expiry::Monitors,
                inner.engine.has_format_monitors() && !self.monitor_running,
                CONTROL_SUBSCRIPTION_INTERVAL,
            ),
            (
                TimerKey::Labels,
                Expiry::Labels,
                Shared::clock_labels_needed(&inner),
                CONTROL_SUBSCRIPTION_INTERVAL,
            ),
            (
                TimerKey::Diagnostics,
                Expiry::Diagnostics,
                log::log_enabled!(target: "zz_daemon::diagnostics::state", log::Level::Trace),
                DIAGNOSTIC_STATE_INTERVAL,
            ),
        ] {
            if !needed {
                deadlines.remove(key);
            } else if deadlines.get(key).is_none() {
                deadlines.insert(key, now + interval, expiry);
            }
        }
        let armed = Shared::peer_scan_armed(&inner);
        drop(inner);
        let requested = shared.peer_probe_requested();
        if !self.peer_running && (armed || requested || self.probe.follow_up) {
            let next = self
                .probe
                .last
                .map_or(now, |last| (last + PEER_PROBE_INTERVAL).max(now));
            deadlines.insert_earliest(TimerKey::PeerProbe, next, Expiry::PeerProbe);
        } else {
            deadlines.remove(TimerKey::PeerProbe);
        }
    }
}

#[cfg(unix)]
enum TimerCompletion {
    Expiries,
    Monitor,
    Peer,
}

#[cfg(unix)]
pub(super) const TIMER_INPUT_BURST: usize = 64;
#[cfg(unix)]
pub(super) const TIMER_EXPIRY_BURST: usize = 32;

#[cfg(unix)]
pub(super) struct LoopTimers {
    inputs: Option<crossbeam_channel::Receiver<TimerInput>>,
    deadlines: Deadlines,
    pending: VecDeque<(Expiry, Instant)>,
    worker_running: bool,
    completed: crossbeam_channel::Receiver<TimerCompletion>,
    completion_sender: crossbeam_channel::Sender<TimerCompletion>,
    clients: ClientTimers,
    shutdown_due: bool,
}

#[cfg(unix)]
impl LoopTimers {
    pub(super) fn new(shared: &Shared, waker: &Arc<mio::Waker>) -> Self {
        let inputs = shared.timer_rx.lock().take();
        if inputs.is_some() {
            shared.timer_tx.wake.install(Arc::clone(waker));
        }
        let (completion_sender, completed) = crossbeam_channel::unbounded();
        let mut deadlines = Deadlines::default();
        let mut clients = ClientTimers::default();
        clients.sync(shared, &mut deadlines, Instant::now());
        Self {
            inputs,
            deadlines,
            pending: VecDeque::new(),
            worker_running: false,
            completed,
            completion_sender,
            shutdown_due: false,
            clients,
        }
    }

    pub(super) fn shutdown_deadline(&mut self, deadline: Option<Instant>) {
        self.shutdown_due = false;
        self.deadlines.remove(TimerKey::Shutdown);
        if let Some(deadline) = deadline {
            self.deadlines
                .insert(TimerKey::Shutdown, deadline, Expiry::Shutdown);
        }
    }

    pub(super) fn take_shutdown_due(&mut self) -> bool {
        std::mem::take(&mut self.shutdown_due)
    }

    pub(super) fn next(&self, now: Instant) -> Option<Instant> {
        if self
            .inputs
            .as_ref()
            .is_some_and(|inputs| !inputs.is_empty())
            || (!self.worker_running && !self.pending.is_empty())
        {
            return Some(now);
        }
        self.deadlines.next()
    }

    pub(super) fn turn(
        &mut self,
        shared: &Arc<Shared>,
        waker: &Arc<mio::Waker>,
    ) -> Result<(), DaemonError> {
        if self.completed.is_empty()
            && self
                .inputs
                .as_ref()
                .is_none_or(crossbeam_channel::Receiver::is_empty)
            && (self.pending.is_empty() || self.worker_running)
            && self
                .deadlines
                .next()
                .is_none_or(|deadline| deadline > Instant::now())
        {
            return Ok(());
        }
        self.turn_ready(shared, waker)
    }

    fn turn_ready(
        &mut self,
        shared: &Arc<Shared>,
        waker: &Arc<mio::Waker>,
    ) -> Result<(), DaemonError> {
        let mut changed = false;
        for completion in self.completed.try_iter() {
            match completion {
                TimerCompletion::Expiries => self.worker_running = false,
                TimerCompletion::Monitor => {
                    self.clients.monitor_running = false;
                    changed = true;
                }
                TimerCompletion::Peer => {
                    self.clients.peer_running = false;
                    changed = true;
                }
            }
        }
        let mut status_jobs = false;
        if let Some(inputs) = &self.inputs {
            for input in inputs.try_iter().take(TIMER_INPUT_BURST) {
                match input {
                    TimerInput::ClientTimersChanged => changed = true,
                    TimerInput::StatusJobs => status_jobs = true,
                    _ => shared.schedule_timer(&mut self.deadlines, &input),
                }
            }
        }
        if status_jobs {
            shared.refresh_status_notifications();
        }
        if changed {
            self.clients
                .sync(shared, &mut self.deadlines, Instant::now());
        }
        let now = Instant::now();
        let mut status_sessions = BTreeSet::new();
        let mut recurring_due = false;
        for _ in 0..TIMER_EXPIRY_BURST {
            let Some(expiry) = self.deadlines.pop_due(now) else {
                break;
            };
            match expiry {
                Expiry::Shutdown => self.shutdown_due = true,
                Expiry::Status(session) => {
                    recurring_due = true;
                    status_sessions.insert(session);
                }
                Expiry::Subscriptions => {
                    recurring_due = true;
                    shared.refresh_control_subscriptions();
                }
                Expiry::Labels => {
                    recurring_due = true;
                    shared.publish_mux_labels();
                }
                Expiry::Diagnostics => {
                    recurring_due = true;
                    shared.log_diagnostic_snapshot("periodic");
                }
                Expiry::Monitors | Expiry::PeerProbe => {
                    recurring_due = true;
                    let peer = matches!(expiry, Expiry::PeerProbe);
                    if peer {
                        self.clients.peer_running = true;
                        shared.prepare_peer_probe(&mut self.clients.probe, now);
                    } else {
                        self.clients.monitor_running = true;
                    }
                    let owner = shared.server_owner();
                    let completed = self.completion_sender.clone();
                    let wake = Arc::clone(waker);
                    let job: Box<dyn FnOnce() + Send> = Box::new(move || {
                        owner.run_timer_expiry(expiry, now);
                        let _ = completed.send(if peer {
                            TimerCompletion::Peer
                        } else {
                            TimerCompletion::Monitor
                        });
                        let _ = wake.wake();
                    });
                    if peer {
                        thread::Builder::new()
                            .name("zz-peer-probe".to_owned())
                            .spawn(job)?;
                    } else {
                        shared.connection_threads.run(job)?;
                    }
                }
                Expiry::DisplayPanes(_) | Expiry::KeyTable(_) => {
                    shared.run_timer_expiry(expiry, now);
                }
                _ => self.pending.push_back((expiry, now)),
            }
        }
        if !status_sessions.is_empty() {
            shared.refresh_status_for_sessions(Some(&status_sessions));
        }
        if recurring_due {
            self.clients
                .sync(shared, &mut self.deadlines, Instant::now());
        }
        if !self.worker_running && !self.pending.is_empty() {
            let expiries = self
                .pending
                .drain(..self.pending.len().min(TIMER_EXPIRY_BURST))
                .collect::<Vec<_>>();
            let shared = shared.server_owner();
            let threads = Arc::clone(&shared.connection_threads);
            let completed = self.completion_sender.clone();
            let waker = Arc::clone(waker);
            threads.run(Box::new(move || {
                for (expiry, now) in expiries {
                    shared.run_timer_expiry(expiry, now);
                }
                let _ = completed.send(TimerCompletion::Expiries);
                let _ = waker.wake();
            }))?;
            self.worker_running = true;
        }
        Ok(())
    }
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
    #[cfg(all(test, unix))]
    pub(super) fn start_timers(self: &Arc<Self>) -> Result<(), DaemonError> {
        if self.timer_rx.lock().is_none() {
            return Ok(());
        }
        super::event_loop::start_timer_fixture(&self.server_owner())
    }

    #[cfg(windows)]
    pub(super) fn start_timers(self: &Arc<Self>) -> Result<(), DaemonError> {
        let Some(inputs) = self.timer_rx.lock().take() else {
            return Ok(());
        };
        let shared = Arc::downgrade(&self.server_owner());
        thread::Builder::new()
            .name("zz-deadlines".to_owned())
            .spawn(move || {
                let mut deadlines = Deadlines::default();
                let mut clients = ClientTimers::default();
                if let Some(shared) = shared.upgrade() {
                    clients.sync(&shared, &mut deadlines, Instant::now());
                }
                loop {
                    let now = Instant::now();
                    while let Some(expiry) = deadlines.pop_due(now) {
                        let Some(shared) = shared.upgrade() else {
                            return;
                        };
                        if matches!(expiry, Expiry::PeerProbe) {
                            shared.prepare_peer_probe(&mut clients.probe, now);
                        }
                        shared.run_timer_expiry(expiry, now);
                        clients.sync(&shared, &mut deadlines, Instant::now());
                    }
                    let input = match deadlines.next() {
                        Some(deadline) => match inputs.recv_deadline(deadline) {
                            Ok(input) => input,
                            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                        },
                        None => match inputs.recv() {
                            Ok(input) => input,
                            Err(_) => return,
                        },
                    };
                    let Some(shared) = shared.upgrade() else {
                        return;
                    };
                    shared.schedule_timer(&mut deadlines, &input);
                    if matches!(input, TimerInput::ClientTimersChanged) {
                        clients.sync(&shared, &mut deadlines, Instant::now());
                    }
                }
            })
            .map(drop)
            .map_err(|error| DaemonError::Thread(error.to_string()))
    }

    fn schedule_timer(&self, deadlines: &mut Deadlines, input: &TimerInput) {
        match *input {
            TimerInput::ClientTimersChanged => {}
            TimerInput::StatusJobs => self.refresh_status_notifications(),
            TimerInput::DisplayPanes(DisplayPanesDeadlineCommand::Schedule(deadline)) => {
                if self.read_client(deadline.client, |c| {
                    c.and_then(|c| c.display_panes.as_ref())
                        .is_some_and(|overlay| {
                            overlay.token == deadline.token
                                && overlay.deadline == Some(deadline.deadline)
                        })
                }) {
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
            TimerInput::KeyTable(KeyTableDeadlineCommand::Schedule(client, deadline)) => {
                if self.read_client(client, |c| c.and_then(|c| c.key_table_deadline)) != deadline {
                    return;
                }
                if let Some(deadline) = deadline {
                    deadlines.insert(
                        TimerKey::KeyTable(client),
                        deadline,
                        Expiry::KeyTable(client),
                    );
                } else {
                    deadlines.remove(TimerKey::KeyTable(client));
                }
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
                if self.read_client(deadline.client, |c| {
                    c.and_then(|c| c.message.as_ref()).is_some_and(|current| {
                        current.token == deadline.token
                            && current.deadline == Some(deadline.deadline)
                    })
                }) {
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

    fn run_timer_expiry(self: &Arc<Self>, expiry: Expiry, now: Instant) {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.expire_timer(expiry, now);
        }))
        .is_err()
        {
            log::error!(target: "zz_daemon::timers", "a timer expiry panicked");
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
            Expiry::Status(session) => {
                self.refresh_status_for_sessions(Some(&BTreeSet::from([session])));
            }
            Expiry::Subscriptions => self.refresh_control_subscriptions(),
            Expiry::Monitors => self.run_format_monitors(),
            Expiry::Labels => self.publish_mux_labels(),
            Expiry::Diagnostics => self.log_diagnostic_snapshot("periodic"),
            Expiry::PeerProbe => {
                #[cfg(all(feature = "agent", unix))]
                self.sync_claude_peer_states();
            }
            #[cfg(unix)]
            Expiry::Shutdown => unreachable!(),
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
                        .send(TimerInput::Timer(TimerCommand::PublishFlush(
                            last + PUBLISH_FLUSH_INTERVAL,
                        )));
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
            let choosers = !inner.clients.values().all(|c| c.choose_tree.is_none());
            let presentation = (!inner.clients.values().all(|c| c.subscriber.is_none())
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
        let _ = self
            .timer_tx
            .send(TimerInput::Timer(TimerCommand::Rename(deadline)));
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
        let shared = self.server_owner();
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

    pub(super) fn control_subscriptions_needed(inner: &ServerState) -> bool {
        inner.clients.iter().any(|(client, state)| {
            client_attached_session(inner, *client).is_some()
                && state
                    .control_output
                    .as_ref()
                    .is_some_and(|output| !output.subscriptions.is_empty())
        })
    }

    pub(super) fn clock_labels_needed(inner: &ServerState) -> bool {
        inner.clients.iter().any(|(client, state)| {
            state.subscriber.is_some()
                && client_attached_session(inner, *client).is_some()
                && state
                    .ctrl_subscriptions
                    .as_ref()
                    .is_none_or(|subscriptions| {
                        subscriptions.tree != zz_protocol::TreeSubscription::None
                    })
        }) && inner.engine.window_labels_follow_the_clock()
    }

    pub(super) fn status_timer_sessions(
        inner: &ServerState,
    ) -> impl Iterator<Item = SessionId> + '_ {
        inner
            .clients
            .iter()
            .filter(|(_, client)| {
                client.subscriber.is_some()
                    && client
                        .ctrl_subscriptions
                        .as_ref()
                        .is_none_or(|subscription| subscription.status)
            })
            .filter_map(|(client, _)| client_attached_session(inner, *client))
    }

    #[cfg(test)]
    pub(super) fn client_timers_have_work(inner: &ServerState) -> bool {
        Self::control_subscriptions_needed(inner)
            || inner.engine.has_format_monitors()
            || Self::clock_labels_needed(inner)
            || Self::peer_scan_armed(inner)
            || Self::status_timer_sessions(inner).any(|session| {
                let formats = inner.engine.status_formats_for_session(Some(session));
                formats.enabled && !formats.interval.is_zero()
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
        self.nudge_client_timers();
    }

    #[cfg(not(all(feature = "agent", unix)))]
    pub(super) fn request_peer_probe(&self) {}

    fn peer_probe_requested(&self) -> bool {
        #[cfg(all(feature = "agent", unix))]
        {
            self.peer_probe.load(Ordering::Acquire)
        }
        #[cfg(not(all(feature = "agent", unix)))]
        {
            false
        }
    }

    fn prepare_peer_probe(&self, probe: &mut PeerProbe, now: Instant) {
        #[cfg(all(feature = "agent", unix))]
        {
            probe.follow_up = self.peer_probe.swap(false, Ordering::AcqRel);
        }
        probe.last = Some(now);
    }

    fn refresh_status_notifications(&self) {
        let (modes, jobs) = {
            let mut status = self.status.lock();
            (status.take_pending_modes(), status.poll_jobs())
        };
        if !modes.is_empty() {
            self.refresh_modes(&modes);
        }
        if !jobs.is_empty() {
            self.refresh_status_filtered(None, Some(&jobs));
        }
    }

    pub(super) fn nudge_client_timers(&self) {
        let _ = self.timer_tx.send(TimerInput::ClientTimersChanged);
    }
}

impl Drop for SharedServer {
    fn drop(&mut self) {
        self.timer_tx.wake.wake();
    }
}

#[cfg(all(test, unix))]
#[path = "timer_loop_tests.rs"]
mod loop_tests;

#[cfg(all(test, unix))]
#[path = "status_loop_tests.rs"]
mod client_tests;

#[cfg(all(test, unix))]
#[path = "status_loop_b6fix_tests.rs"]
mod b6fix_tests;
