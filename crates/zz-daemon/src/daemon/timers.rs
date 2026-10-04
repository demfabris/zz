use super::*;

pub(super) const PUBLISH_FLUSH_INTERVAL: Duration = Duration::from_millis(16);

pub(super) const PEER_PROBE_INTERVAL: Duration = Duration::from_secs(1);

pub(super) const NAME_INTERVAL: Duration = Duration::from_millis(500);

pub(super) struct NameCheck {
    terminal: usize,
    last: Instant,
    due: Option<Instant>,
}

pub(super) struct KeyTablePublishHold(Arc<Shared>);

impl KeyTablePublishHold {
    pub(super) fn enter(shared: &Arc<Shared>) -> Self {
        shared
            .command_item
            .as_ref()
            .expect("command item")
            .lock()
            .key_table_publish_hold += 1;
        Self(Arc::clone(shared))
    }

    pub(super) fn active(shared: &Shared) -> bool {
        shared
            .command_item
            .as_ref()
            .is_some_and(|item| item.lock().key_table_publish_hold != 0)
    }
}

impl Drop for KeyTablePublishHold {
    fn drop(&mut self) {
        let mut item = self.0.command_item.as_ref().expect("command item").lock();
        item.key_table_publish_hold = item.key_table_publish_hold.saturating_sub(1);
    }
}

pub(super) enum TimerCommand {
    Rename(Instant),
    NameCheck(Instant),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TimerKey {
    DisplayPanes(ClientId),
    KeyTable(ClientId),
    Silence(WindowId),
    ClientMessage(ClientId),
    Rename,
    NameCheck,
    PublishFlush,
    Status(SessionId),
    Subscriptions,
    Monitors,
    Labels,
    PeerProbe,
    Diagnostics,
    CopyRefresh,
    ClockMode,
    Callback(u64),
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
    NameCheck,
    PublishFlush,
    Status(SessionId),
    Subscriptions,
    Monitors,
    Labels,
    PeerProbe,
    Diagnostics,
    CopyRefresh,
    ClockMode,
    Callback(u64),
    #[cfg(unix)]
    Shutdown,
}

pub(super) enum TimerInput {
    Callback {
        deadline: Option<Instant>,
        callback: Box<dyn FnOnce() + Send>,
    },
    DisplayPanes(DisplayPanesDeadlineCommand),
    KeyTable(KeyTableDeadlineCommand),
    Silence(SilenceDeadlineCommand),
    ClientMessage(ClientMessageDeadlineCommand),
    Timer(TimerCommand),
    ClientTimersChanged,
    StatusJobs,
    HookReady,
    Hooks(Vec<PendingHookEvent>),
    #[cfg(all(feature = "agent", unix))]
    PeerSample {
        pane: PaneId,
        value: String,
    },
    MonitorHook {
        context: Box<ExecutionContext>,
        commands: Vec<Vec<CommandInvocation>>,
        variables: BTreeMap<String, String>,
    },
}

#[derive(Clone)]
pub(super) struct TimerSender {
    sender: crossbeam_channel::Sender<TimerInput>,
    wake: Arc<AcceptWake>,
    status_jobs_pending: Arc<AtomicBool>,
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
            status_jobs_pending: Arc::new(AtomicBool::new(false)),
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
        if !self.status_jobs_pending.swap(true, Ordering::AcqRel) {
            let _ = self.send(TimerInput::StatusJobs);
        }
    }
}

#[derive(Default)]
struct ClientTimers {
    intervals: BTreeMap<SessionId, Duration>,
    probe: PeerProbe,
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
                inner.engine.has_format_monitors(),
                CONTROL_SUBSCRIPTION_INTERVAL,
            ),
            (
                TimerKey::Labels,
                Expiry::Labels,
                Shared::clock_labels_needed(&inner),
                CONTROL_SUBSCRIPTION_INTERVAL,
            ),
            (
                TimerKey::CopyRefresh,
                Expiry::CopyRefresh,
                copy_mode_refresh_needed(&inner),
                COPY_MODE_REFRESH_INTERVAL,
            ),
            (
                TimerKey::ClockMode,
                Expiry::ClockMode,
                clock_mode_timer_needed(&inner),
                duration_to_next_second(),
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
pub(super) enum TimerCompletion {
    #[cfg_attr(not(feature = "agent"), allow(dead_code))]
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
    pub(super) hooks: hook_queue::LoopHooks,
    completed: crossbeam_channel::Receiver<TimerCompletion>,
    #[cfg_attr(not(feature = "agent"), allow(dead_code))]
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
            hooks: hook_queue::LoopHooks::new(),
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
        if self.hooks_pending() {
            return Some(now);
        }
        self.deadlines.next()
    }

    pub(super) fn hooks_pending(&self) -> bool {
        self.inputs
            .as_ref()
            .is_some_and(|inputs| !inputs.is_empty())
            || self.hooks.ready()
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
            && !self.hooks.ready()
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
                    TimerInput::Callback { deadline, callback } => {
                        self.deadlines.callback(deadline, callback);
                    }
                    TimerInput::ClientTimersChanged => changed = true,
                    TimerInput::StatusJobs => {
                        shared
                            .timer_tx
                            .status_jobs_pending
                            .store(false, Ordering::Release);
                        status_jobs = true;
                    }
                    TimerInput::HookReady => {}
                    TimerInput::Hooks(events) => self.hooks.events(shared, events),
                    #[cfg(feature = "agent")]
                    TimerInput::PeerSample { pane, value } => self.hooks.command(
                        shared,
                        ExecutionContext::default(),
                        CommandInvocation::new(
                            "set-option",
                            ["-p", "-t", &pane.to_string(), "@agent_state", &value],
                        ),
                    ),
                    TimerInput::MonitorHook {
                        context,
                        commands,
                        variables,
                    } => {
                        self.hooks.monitor(shared, *context, commands, variables);
                    }
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
                Expiry::Callback(id) => self.deadlines.run_callback(id),
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
                Expiry::PeerProbe => {
                    recurring_due = true;
                    shared.prepare_peer_probe(&mut self.clients.probe, now);
                    #[cfg(all(feature = "agent", unix))]
                    {
                        self.clients.peer_running = shared.start_peer_scan(&self.completion_sender);
                    }
                }
                Expiry::Monitors | Expiry::CopyRefresh | Expiry::ClockMode => {
                    recurring_due = true;
                    shared.run_timer_expiry(expiry, now);
                }
                _ => shared.run_timer_expiry(expiry, now),
            }
        }
        if !status_sessions.is_empty() {
            shared.refresh_status_for_sessions(Some(&status_sessions));
        }
        if recurring_due {
            self.clients
                .sync(shared, &mut self.deadlines, Instant::now());
        }
        self.hooks.turn(shared, waker)?;
        Ok(())
    }
}

#[derive(Default)]
struct Deadlines {
    next_callback: u64,
    callbacks: BTreeMap<u64, Box<dyn FnOnce() + Send>>,
    ready: VecDeque<(Instant, u64)>,
    order: BTreeSet<(Instant, TimerKey)>,
    entries: BTreeMap<TimerKey, (Instant, Expiry)>,
}

impl Deadlines {
    fn callback(&mut self, deadline: Option<Instant>, callback: Box<dyn FnOnce() + Send>) {
        self.next_callback = self
            .next_callback
            .checked_add(1)
            .expect("timer callback id");
        let id = self.next_callback;
        self.callbacks.insert(id, callback);
        if let Some(deadline) = deadline {
            self.insert(TimerKey::Callback(id), deadline, Expiry::Callback(id));
        } else {
            self.ready.push_back((Instant::now(), id));
        }
    }

    #[cfg(unix)]
    fn run_callback(&mut self, id: u64) {
        if let Some(callback) = self.callbacks.remove(&id)
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)).is_err()
        {
            log::error!(target: "zz_daemon::timers", "a timer callback panicked");
        }
    }

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
        self.ready
            .front()
            .map(|(ready, _)| *ready)
            .into_iter()
            .chain(self.order.first().map(|(deadline, _)| *deadline))
            .min()
    }

    fn pop_due(&mut self, now: Instant) -> Option<Expiry> {
        if self.ready.front().is_some_and(|(ready, _)| *ready <= now) {
            let (_, id) = self.ready.pop_front().expect("ready callback");
            return Some(Expiry::Callback(id));
        }
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
        let mut status_jobs = status_jobs::WindowsRegistry::new(self.status.lock().job_client());
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
                    if shared
                        .upgrade()
                        .is_none_or(|shared| shared.stopping.load(Ordering::Acquire))
                    {
                        return;
                    }
                    status_jobs.turn();
                    let now = Instant::now();
                    while let Some(expiry) = deadlines.pop_due(now) {
                        let Some(shared) = shared.upgrade() else {
                            return;
                        };
                        if matches!(expiry, Expiry::PeerProbe) {
                            shared.prepare_peer_probe(&mut clients.probe, now);
                        }
                        if let Expiry::Callback(id) = expiry {
                            if let Some(callback) = deadlines.callbacks.remove(&id)
                                && let Err(error) = shared.connection_threads.run(callback)
                            {
                                log::error!("failed to run timer callback: {error}");
                            }
                        } else {
                            shared.run_timer_expiry(expiry, now);
                        }
                        clients.sync(&shared, &mut deadlines, Instant::now());
                    }
                    let deadline = deadlines.next().into_iter().chain(status_jobs.next()).min();
                    let input = match deadline {
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
                    if let TimerInput::Callback { deadline, callback } = input {
                        deadlines.callback(deadline, callback);
                        continue;
                    }
                    match &input {
                        TimerInput::Hooks(events) => shared.run_event_hooks(events.clone()),
                        TimerInput::MonitorHook {
                            context,
                            commands,
                            variables,
                        } => {
                            shared.run_hook_commands_with_policy(
                                ClientId(u64::MAX),
                                ClientKind::Command,
                                context,
                                commands.clone(),
                                variables,
                                true,
                                None,
                                false,
                            );
                        }
                        _ => shared.schedule_timer(&mut deadlines, &input),
                    }
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
            TimerInput::Callback { .. } => unreachable!(),
            TimerInput::HookReady => {}
            #[cfg(all(feature = "agent", unix))]
            TimerInput::PeerSample { .. } => unreachable!(),
            TimerInput::Hooks(_) | TimerInput::MonitorHook { .. } => unreachable!(),
            TimerInput::ClientTimersChanged => {}
            TimerInput::StatusJobs => {
                self.timer_tx
                    .status_jobs_pending
                    .store(false, Ordering::Release);
                self.refresh_status_notifications();
            }
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
            TimerInput::Timer(TimerCommand::NameCheck(deadline)) => {
                deadlines.insert_earliest(TimerKey::NameCheck, deadline, Expiry::NameCheck);
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
            Expiry::Callback(_) => unreachable!(),
            Expiry::DisplayPanes(deadline) => {
                self.expire_display_panes(deadline, now);
            }
            Expiry::KeyTable(client) => self.sync_key_table(client, false),
            Expiry::Silence(deadline) => self.expire_window_silence(deadline, now),
            Expiry::ClientMessage(deadline) => self.expire_client_message(deadline, now),
            Expiry::Rename => self.apply_due_window_renames(now),
            Expiry::NameCheck => self.run_due_name_checks(now),
            Expiry::PublishFlush => self.flush_publish(),
            Expiry::Status(session) => {
                self.refresh_status_for_sessions(Some(&BTreeSet::from([session])));
            }
            Expiry::Subscriptions => self.refresh_control_subscriptions(),
            Expiry::Monitors => self.run_format_monitors(),
            Expiry::Labels => self.publish_mux_labels(),
            Expiry::Diagnostics => self.log_diagnostic_snapshot("periodic"),
            Expiry::CopyRefresh => {
                for (client, terminal) in copy_mode_refresh_ticks(&self.inner.lock()) {
                    terminal.view_action(
                        TerminalViewId(client.0),
                        zz_terminal::TerminalViewAction::CopyMode(
                            zz_terminal::CopyModeAction::RefreshRevision,
                        ),
                    );
                }
            }
            Expiry::ClockMode => {
                if clock_mode_timer_needed(&self.inner.lock()) {
                    self.publish_mux_snapshots();
                }
            }
            Expiry::PeerProbe => {
                #[cfg(all(feature = "agent", unix))]
                self.sync_claude_peer_states();
            }
            #[cfg(unix)]
            Expiry::Shutdown => unreachable!(),
        }
    }

    pub(super) fn request_publish(self: &Arc<Self>, reason: PublishReason) {
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
                || inner.engine.has_window_style_settings())
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

    pub(super) fn admit_name_check(
        &self,
        inner: &mut ServerState,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
        now: Instant,
    ) -> bool {
        let identity = Arc::as_ptr(terminal) as usize;
        let due = match inner.name_checks.get_mut(&pane) {
            Some(check)
                if check.terminal == identity
                    && now.saturating_duration_since(check.last) < NAME_INTERVAL =>
            {
                if check.due.is_some() {
                    return false;
                }
                let due = check.last + NAME_INTERVAL;
                check.due = Some(due);
                due
            }
            _ => {
                inner.name_checks.insert(
                    pane,
                    NameCheck {
                        terminal: identity,
                        last: now,
                        due: None,
                    },
                );
                return true;
            }
        };
        if inner
            .scheduled_name_check
            .is_none_or(|scheduled| scheduled > due)
        {
            inner.scheduled_name_check = Some(due);
            let _ = self
                .timer_tx
                .send(TimerInput::Timer(TimerCommand::NameCheck(due)));
        }
        false
    }

    pub(super) fn run_due_name_checks(self: &Arc<Self>, now: Instant) {
        let due = {
            let mut inner = self.inner.lock();
            let inner = &mut *inner;
            inner.scheduled_name_check = None;
            let mut due = Vec::new();
            let mut next: Option<Instant> = None;
            for (pane, check) in &mut inner.name_checks {
                let Some(deadline) = check.due else {
                    continue;
                };
                if deadline > now {
                    next = Some(next.map_or(deadline, |next| next.min(deadline)));
                    continue;
                }
                check.due = None;
                if let Some(terminal) = inner.terminals.get(pane)
                    && Arc::as_ptr(terminal) as usize == check.terminal
                    && terminal.take_output_since_check()
                {
                    check.last = now;
                    due.push((*pane, Arc::clone(terminal)));
                }
            }
            if let Some(next) = next {
                inner.scheduled_name_check = Some(next);
                let _ = self
                    .timer_tx
                    .send(TimerInput::Timer(TimerCommand::NameCheck(next)));
            }
            due
        };
        for (pane, terminal) in due {
            let (current_command, live_path) = terminal_foreground_facts(&terminal);
            let events =
                self.apply_pane_runtime(pane, &terminal, &current_command, live_path, true, now);
            self.enqueue_event_hooks(events);
        }
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
        self.enqueue_event_hooks(events);
    }

    pub(super) fn enqueue_event_hooks(self: &Arc<Self>, events: Vec<PendingHookEvent>) {
        if events.is_empty() {
            return;
        }
        if self.timer_rx.lock().is_none() || self.watcher_effects.is_some() {
            let _ = self.timer_tx.send(TimerInput::Hooks(events));
            return;
        }
        self.run_event_hooks(events);
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
        !inner.claude_peer_states.is_empty()
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

    #[cfg(all(feature = "agent", unix))]
    fn start_peer_scan(&self, completed: &crossbeam_channel::Sender<TimerCompletion>) -> bool {
        let armed = Self::peer_scan_armed(&self.inner.lock());
        let panes = self.peer_scan_inputs();
        if !armed && self.helpers.peer_scan_settled(&panes) {
            return false;
        }
        if let Err(error) = self.helpers.submit(helpers::Task::Peers {
            panes,
            reply: None,
            completed: Some(completed.clone()),
        }) {
            log::warn!("could not start peer scan: {error}");
            return false;
        }
        true
    }

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

#[cfg(all(test, unix))]
#[path = "timers_e20_tests.rs"]
mod e20_tests;

#[cfg(all(test, unix))]
#[path = "delay_timer_tests.rs"]
mod delay_tests;

#[cfg(all(test, feature = "agent", unix))]
#[path = "timers_peerskip_tests.rs"]
mod peerskip_tests;
