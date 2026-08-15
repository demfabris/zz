//! The daemon's agent runtimes: one thread per open pane, a shared park
//! ticker, and a single sink every stream item leaves through.
//!
//! The host owns what the desktop's controller owned below its reducer —
//! prompt queueing, the quiesce park, turn snapshots, permission bookkeeping —
//! and nothing above it. It never renders, so it keeps no transcript: the raw
//! items are journalled on the way past and handed to the sink, and whoever
//! attached rebuilds the conversation from them.

use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use async_channel::{Receiver, Sender};
use parking_lot::{Condvar, Mutex};
use serde_json::Value;
use zz_protocol::{AgentProvider, PaneId};

use crate::agent::{
    environment::warm_adapter_cache,
    journal::AgentJournal,
    profile::SdkTaskEvent,
    runtime::{
        AgentSpawnConfig, RuntimeCommand, load_persistent_journal, quiesce_window,
        run_agent_runtime, should_park_turn,
    },
    stream::{
        AgentAuthMethod, AgentPrompt, AgentPromptOutcome, AgentSessionCapabilities,
        AgentSessionSummary, AgentStreamItem, AgentStreamPayload, AgentTurnDiffOutcome,
    },
    turn_snapshot::{self, TurnTree},
};

/// How often the shared ticker looks at the open panes. The quiesce window is
/// measured in minutes, so second resolution is plenty and the thread is
/// parked outright whenever no pane is open.
const PARK_TICK: Duration = Duration::from_secs(1);

/// Where every item a pane produces goes. The wiring phase fans out from here;
/// the host only guarantees order and the per-pane `seq`.
pub(crate) type AgentStreamSink = Box<dyn Fn(PaneId, AgentStreamItem) + Send + Sync>;

/// What the host is asked to do with an open pane. Everything a client can
/// send lands here, plus the park the ticker injects.
#[derive(Debug)]
pub(crate) enum HostCommand {
    Prompt(AgentPrompt),
    Cancel,
    /// Hand the queued prompts back so the composer can refill.
    Unqueue,
    RespondPermission {
        request_id: u64,
        option_id: Option<String>,
    },
    Authenticate {
        method_id: String,
    },
    SetConfigOption {
        option_id: String,
        value: String,
    },
    SetMode {
        mode_id: String,
    },
    ListSessions {
        cwd: Option<PathBuf>,
        cursor: Option<String>,
        replace: bool,
    },
    NewSession,
    SwitchSession {
        session: AgentSessionSummary,
    },
    DeleteSession {
        session_id: String,
    },
    TurnDiff {
        request_id: u64,
    },
    Park,
}

/// What an agent pane is opened against.
#[derive(Clone, Debug)]
pub(crate) struct AgentPaneSpec {
    pub(crate) provider: AgentProvider,
    pub(crate) cwd: PathBuf,
    pub(crate) resume_session: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentConnectionPhase {
    Starting,
    Restoring,
    Ready,
    Running,
    Cancelling,
    Failed,
    Disconnected,
}

impl AgentConnectionPhase {
    const fn accepts_prompt(self) -> bool {
        matches!(self, Self::Ready)
    }

    const fn has_active_turn(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AgentPendingPermission {
    pub(crate) request_id: u64,
    pub(crate) tool_call: Value,
    pub(crate) options: Value,
}

/// The pane state a client needs without replaying the stream: enough for a
/// badge, a status line, and a permission prompt.
#[derive(Clone, Debug)]
pub(crate) struct AgentPaneState {
    pub(crate) provider: AgentProvider,
    pub(crate) phase: AgentConnectionPhase,
    pub(crate) agent_name: Option<String>,
    pub(crate) agent_key: Option<String>,
    pub(crate) capabilities: AgentSessionCapabilities,
    pub(crate) auth_methods: Vec<AgentAuthMethod>,
    pub(crate) session_id: Option<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) queued_prompts: usize,
    pub(crate) pending_permissions: Vec<AgentPendingPermission>,
    pub(crate) error: Option<String>,
    pub(crate) last_seq: u64,
    last_activity: Instant,
    live_tools: HashSet<String>,
    live_tasks: HashSet<String>,
}

impl AgentPaneState {
    fn new(spec: &AgentPaneSpec) -> Self {
        Self {
            provider: spec.provider,
            phase: AgentConnectionPhase::Starting,
            agent_name: None,
            agent_key: None,
            capabilities: AgentSessionCapabilities::default(),
            auth_methods: Vec::new(),
            session_id: spec.resume_session.clone(),
            cwd: spec.cwd.clone(),
            queued_prompts: 0,
            pending_permissions: Vec::new(),
            error: None,
            last_seq: 0,
            last_activity: Instant::now(),
            live_tools: HashSet::new(),
            live_tasks: HashSet::new(),
        }
    }

    /// What the quiesce watchdog refuses to park through: a permission the user
    /// still owes an answer to, a subagent task still reporting, or a tool call
    /// the agent has not resolved.
    fn turn_in_flight(&self) -> bool {
        !self.pending_permissions.is_empty()
            || !self.live_tasks.is_empty()
            || !self.live_tools.is_empty()
    }

    fn park_due(&self, window: Option<Duration>) -> bool {
        self.phase == AgentConnectionPhase::Running
            && should_park_turn(window, self.last_activity.elapsed(), self.turn_in_flight())
    }
}

enum PaneInput {
    Command(HostCommand),
    Event(AgentStreamPayload),
    Close,
    Finished(Result<(), String>),
}

struct PaneHandle {
    inbox: Sender<PaneInput>,
    state: Arc<Mutex<AgentPaneState>>,
    thread: JoinHandle<()>,
}

/// The open panes, and the gate the ticker parks on. Nothing polls while the
/// map is empty.
#[derive(Default)]
struct PaneRegistry {
    panes: Mutex<BTreeMap<PaneId, PaneHandle>>,
    wake: Condvar,
    stopped: AtomicBool,
}

pub(crate) struct AgentHost {
    config: Mutex<AgentSpawnConfig>,
    sink: Arc<AgentStreamSink>,
    journal: Option<Arc<AgentJournal>>,
    registry: Arc<PaneRegistry>,
    permission_ids: Arc<AtomicU64>,
    /// Shared with every pane and with the ticker, so both sides of a park
    /// decision always read the same window.
    park_window: Arc<Mutex<Option<Duration>>>,
    ticker: Mutex<Option<JoinHandle<()>>>,
}

/// What a pane's runtime is handed when its thread starts. Boxed so tests can
/// swap the adapter child for an in-process fixture.
pub(crate) struct RuntimeChannels {
    pub(crate) permission_ids: Arc<AtomicU64>,
    pub(crate) journal: Option<Arc<AgentJournal>>,
    pub(crate) commands: Receiver<RuntimeCommand>,
    pub(crate) events: Sender<AgentStreamPayload>,
}

type PaneRunner = Box<
    dyn FnOnce(RuntimeChannels) -> Pin<Box<dyn Future<Output = Result<(), String>>>>
        + Send
        + 'static,
>;

impl AgentHost {
    pub(crate) fn new(config: AgentSpawnConfig, sink: AgentStreamSink) -> Self {
        Self::with_journal(config, sink, load_persistent_journal())
    }

    /// A host with an explicit journal, so tests never reach the daemon's data
    /// directory.
    pub(crate) fn with_journal(
        config: AgentSpawnConfig,
        sink: AgentStreamSink,
        journal: Option<Arc<AgentJournal>>,
    ) -> Self {
        Self {
            config: Mutex::new(config),
            sink: Arc::new(sink),
            journal,
            registry: Arc::new(PaneRegistry::default()),
            permission_ids: Arc::new(AtomicU64::new(1)),
            park_window: Arc::new(Mutex::new(quiesce_window())),
            ticker: Mutex::new(None),
        }
    }

    /// Snapshot the login shell's PATH and warm the adapter package cache off
    /// the spawn path, so the first pane doesn't pay for either.
    pub(crate) fn prewarm(&self) {
        warm_adapter_cache(&self.config.lock().commands());
    }

    /// Adopt new adapter commands. Panes already running keep the child they
    /// have; the next one to open uses the new configuration.
    pub(crate) fn reconfigure(&self, config: AgentSpawnConfig) {
        *self.config.lock() = config;
    }

    pub(crate) fn open(&self, pane: PaneId, spec: AgentPaneSpec) -> bool {
        let config = self.config.lock().clone();
        let provider = spec.provider;
        let runner: PaneRunner = Box::new(move |channels: RuntimeChannels| {
            Box::pin(run_agent_runtime(
                config,
                provider,
                channels.permission_ids,
                channels.journal,
                channels.commands,
                channels.events,
            ))
        });
        self.open_with(pane, spec, runner)
    }

    fn open_with(&self, pane: PaneId, spec: AgentPaneSpec, runner: PaneRunner) -> bool {
        let mut panes = self.registry.panes.lock();
        if panes.contains_key(&pane) {
            return false;
        }
        let state = Arc::new(Mutex::new(AgentPaneState::new(&spec)));
        let pane_state = Arc::clone(&state);
        let sink = Arc::clone(&self.sink);
        let permission_ids = Arc::clone(&self.permission_ids);
        let journal = self.journal.clone();
        let park_window = Arc::clone(&self.park_window);
        let (inbox_tx, inbox_rx) = async_channel::unbounded();
        let inbox = inbox_tx.clone();
        let thread = std::thread::Builder::new()
            .name(format!("zz-agent-{}", pane.0))
            .spawn(move || {
                futures_lite::future::block_on(run_pane(
                    pane,
                    spec,
                    pane_state,
                    sink,
                    park_window,
                    runner,
                    permission_ids,
                    journal,
                    inbox,
                    inbox_rx,
                ));
            });
        let thread = match thread {
            Ok(thread) => thread,
            Err(error) => {
                log::error!(target: "zz::agent", "could not start the agent thread for pane {pane}: {error}");
                return false;
            }
        };
        panes.insert(
            pane,
            PaneHandle {
                inbox: inbox_tx,
                state,
                thread,
            },
        );
        drop(panes);
        self.registry.wake.notify_all();
        self.ensure_ticker();
        true
    }

    pub(crate) fn command(&self, pane: PaneId, command: HostCommand) -> bool {
        let panes = self.registry.panes.lock();
        panes
            .get(&pane)
            .is_some_and(|handle| handle.inbox.try_send(PaneInput::Command(command)).is_ok())
    }

    /// Stop a pane's runtime. The returned handle is the pane's thread, which
    /// settles once the adapter has been told to close; callers that care join
    /// it, the daemon's own teardown does not.
    pub(crate) fn close(&self, pane: PaneId) -> Option<JoinHandle<()>> {
        let handle = self.registry.panes.lock().remove(&pane)?;
        let _ = handle.inbox.try_send(PaneInput::Close);
        Some(handle.thread)
    }

    pub(crate) fn snapshot_state(&self, pane: PaneId) -> Option<AgentPaneState> {
        let panes = self.registry.panes.lock();
        panes.get(&pane).map(|handle| handle.state.lock().clone())
    }

    pub(crate) fn open_panes(&self) -> Vec<PaneId> {
        self.registry.panes.lock().keys().copied().collect()
    }

    pub(crate) fn shutdown(&self) {
        let handles = std::mem::take(&mut *self.registry.panes.lock());
        for handle in handles.into_values() {
            let _ = handle.inbox.try_send(PaneInput::Close);
            let _ = handle.thread.join();
        }
        self.registry.stopped.store(true, Ordering::Release);
        self.registry.wake.notify_all();
        if let Some(ticker) = self.ticker.lock().take() {
            let _ = ticker.join();
        }
    }

    /// Narrow the quiesce window a pane runs under. Tests drive the park
    /// decision directly rather than waiting out the process-wide window.
    #[cfg(test)]
    pub(crate) fn set_park_window(&self, window: Option<Duration>) {
        *self.park_window.lock() = window;
    }

    fn ensure_ticker(&self) {
        if self.park_window.lock().is_none() {
            return;
        }
        let mut ticker = self.ticker.lock();
        if ticker.is_some() {
            return;
        }
        let registry = Arc::clone(&self.registry);
        let window = Arc::clone(&self.park_window);
        *ticker = std::thread::Builder::new()
            .name("zz-agent-park".to_owned())
            .spawn(move || run_park_ticker(&registry, &window))
            .map_err(|error| {
                log::error!(target: "zz::agent", "could not start the agent park ticker: {error}");
            })
            .ok();
    }
}

impl Drop for AgentHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The one clock the agent host runs, shared by every pane and parked outright
/// while none is open.
fn run_park_ticker(registry: &Arc<PaneRegistry>, window: &Mutex<Option<Duration>>) {
    loop {
        let mut panes = registry.panes.lock();
        while panes.is_empty() && !registry.stopped.load(Ordering::Acquire) {
            registry.wake.wait(&mut panes);
        }
        if registry.stopped.load(Ordering::Acquire) {
            return;
        }
        let window = *window.lock();
        let due = panes
            .values()
            .filter(|handle| handle.state.lock().park_due(window))
            .map(|handle| handle.inbox.clone())
            .collect::<Vec<_>>();
        drop(panes);
        for inbox in due {
            let _ = inbox.try_send(PaneInput::Command(HostCommand::Park));
        }
        let mut panes = registry.panes.lock();
        registry.wake.wait_for(&mut panes, PARK_TICK);
        if registry.stopped.load(Ordering::Acquire) {
            return;
        }
    }
}

/// A pane's whole life on one thread: the ACP connection, the forwarder that
/// hands its payloads to the pump, and the pump itself.
async fn run_pane(
    pane: PaneId,
    spec: AgentPaneSpec,
    state: Arc<Mutex<AgentPaneState>>,
    sink: Arc<AgentStreamSink>,
    park_window: Arc<Mutex<Option<Duration>>>,
    runner: PaneRunner,
    permission_ids: Arc<AtomicU64>,
    journal: Option<Arc<AgentJournal>>,
    inbox_tx: Sender<PaneInput>,
    inbox_rx: Receiver<PaneInput>,
) {
    let (command_tx, command_rx) = async_channel::unbounded();
    let (event_tx, event_rx) = async_channel::unbounded();
    let closer = event_tx.clone();
    let outcome = Arc::new(Mutex::new(None));

    let runtime_outcome = Arc::clone(&outcome);
    let runtime = async move {
        let result = runner(RuntimeChannels {
            permission_ids,
            journal,
            commands: command_rx,
            events: event_tx,
        })
        .await;
        *runtime_outcome.lock() = Some(result);
        // The runtime is done, so nothing may keep the payload channel open:
        // the forwarder below ends on its close, and only then is the pane's
        // outcome final.
        closer.close();
    };

    let forward = async move {
        while let Ok(payload) = event_rx.recv().await {
            if inbox_tx.send(PaneInput::Event(payload)).await.is_err() {
                break;
            }
        }
        let result = outcome.lock().take().unwrap_or(Ok(()));
        let _ = inbox_tx.send(PaneInput::Finished(result)).await;
        inbox_tx.close();
    };

    let pump = PanePump {
        pane,
        spec,
        state,
        sink,
        commands: command_tx,
        park_window,
        seq: 0,
        queue: VecDeque::new(),
        turn_base: None,
        closing: false,
    };

    futures_lite::future::zip(
        futures_lite::future::zip(runtime, forward),
        pump.run(inbox_rx),
    )
    .await;
}

/// The host-side half of a pane: what the desktop controller did between its
/// runtime and its reducer.
struct PanePump {
    pane: PaneId,
    spec: AgentPaneSpec,
    state: Arc<Mutex<AgentPaneState>>,
    sink: Arc<AgentStreamSink>,
    commands: Sender<RuntimeCommand>,
    park_window: Arc<Mutex<Option<Duration>>>,
    seq: u64,
    queue: VecDeque<AgentPrompt>,
    turn_base: Option<TurnTree>,
    closing: bool,
}

/// What an observed payload asks the pump to do once it has been emitted.
enum FollowUp {
    None,
    DrainQueue,
    ReclaimQueue,
    Reopen,
}

impl PanePump {
    async fn run(mut self, inbox: Receiver<PaneInput>) {
        self.send(RuntimeCommand::Open {
            cwd: self.spec.cwd.clone(),
            resume_session: self.spec.resume_session.clone(),
        });
        while let Ok(input) = inbox.recv().await {
            match input {
                PaneInput::Event(payload) => self.observe(payload),
                PaneInput::Command(command) => self.command(command),
                PaneInput::Close => self.close(),
                PaneInput::Finished(result) => {
                    self.finish(&result);
                    break;
                }
            }
        }
    }

    fn command(&mut self, command: HostCommand) {
        match command {
            HostCommand::Prompt(prompt) => self.prompt(prompt),
            HostCommand::Cancel => self.cancel(),
            HostCommand::Unqueue => self.reclaim_queue(),
            HostCommand::RespondPermission {
                request_id,
                option_id,
            } => self.send(RuntimeCommand::RespondPermission {
                request_id,
                option_id,
            }),
            HostCommand::Authenticate { method_id } => {
                self.send(RuntimeCommand::Authenticate { method_id });
            }
            HostCommand::SetConfigOption { option_id, value } => {
                self.send(RuntimeCommand::SetConfigOption { option_id, value });
            }
            HostCommand::SetMode { mode_id } => self.send(RuntimeCommand::SetMode { mode_id }),
            HostCommand::ListSessions {
                cwd,
                cursor,
                replace,
            } => self.send(RuntimeCommand::ListSessions {
                cwd,
                cursor,
                replace,
            }),
            HostCommand::NewSession => {
                let cwd = self.state.lock().cwd.clone();
                self.send(RuntimeCommand::NewSession { cwd });
            }
            HostCommand::SwitchSession { session } => {
                self.send(RuntimeCommand::SwitchSession { session });
            }
            HostCommand::DeleteSession { session_id } => {
                self.send(RuntimeCommand::DeleteSession { session_id });
            }
            HostCommand::TurnDiff { request_id } => self.turn_diff(request_id),
            HostCommand::Park => self.park(),
        }
    }

    fn observe(&mut self, payload: AgentStreamPayload) {
        let mut follow = FollowUp::None;
        {
            let mut state = self.state.lock();
            state.last_activity = Instant::now();
            match &payload {
                AgentStreamPayload::Ready {
                    agent_name,
                    agent_key,
                    auth_methods,
                    capabilities,
                } => {
                    state.agent_name = Some(agent_name.clone());
                    state.agent_key = Some(agent_key.clone());
                    state.auth_methods.clone_from(auth_methods);
                    state.capabilities = *capabilities;
                }
                AgentStreamPayload::SessionReset { restoring } => {
                    state.phase = if *restoring {
                        AgentConnectionPhase::Restoring
                    } else {
                        AgentConnectionPhase::Starting
                    };
                    state.error = None;
                    settle_turn(&mut state);
                }
                AgentStreamPayload::SessionReady { session_id, .. } => {
                    state.session_id = Some(session_id.clone());
                    state.phase = AgentConnectionPhase::Ready;
                    state.error = None;
                    follow = FollowUp::DrainQueue;
                }
                AgentStreamPayload::SessionSwitched {
                    session_id, cwd, ..
                } => {
                    state.session_id = Some(session_id.clone());
                    state.cwd.clone_from(cwd);
                    state.phase = AgentConnectionPhase::Ready;
                    state.error = None;
                    settle_turn(&mut state);
                    follow = FollowUp::DrainQueue;
                }
                AgentStreamPayload::SessionListFailed { message }
                | AgentStreamPayload::SessionSwitchFailed { message }
                | AgentStreamPayload::SessionDeleteFailed { message }
                | AgentStreamPayload::SettingFailed { message, .. } => {
                    state.error = Some(message.clone());
                }
                AgentStreamPayload::Update { update } => track_tool_call(&mut state, update),
                AgentStreamPayload::TaskEvent { event } => track_task(&mut state, event),
                AgentStreamPayload::PermissionRequested {
                    request_id,
                    tool_call,
                    options,
                } => state.pending_permissions.push(AgentPendingPermission {
                    request_id: *request_id,
                    tool_call: tool_call.clone(),
                    options: options.clone(),
                }),
                AgentStreamPayload::PermissionResolved { request_id, .. } => state
                    .pending_permissions
                    .retain(|pending| pending.request_id != *request_id),
                AgentStreamPayload::PromptFinished { outcome } => match outcome {
                    AgentPromptOutcome::Finished { stop_reason } => {
                        state.phase = AgentConnectionPhase::Ready;
                        settle_turn(&mut state);
                        follow = if stop_reason.as_str() == Some("cancelled") {
                            FollowUp::ReclaimQueue
                        } else {
                            FollowUp::DrainQueue
                        };
                    }
                    AgentPromptOutcome::Failed { message } => {
                        state.phase = AgentConnectionPhase::Failed;
                        state.error = Some(message.clone());
                        settle_turn(&mut state);
                        follow = FollowUp::ReclaimQueue;
                    }
                },
                AgentStreamPayload::Authenticated => {
                    state.error = None;
                    follow = FollowUp::Reopen;
                }
                AgentStreamPayload::AuthenticationFailed { message }
                | AgentStreamPayload::PaneFailed { message } => {
                    state.phase = AgentConnectionPhase::Failed;
                    state.error = Some(message.clone());
                    settle_turn(&mut state);
                    follow = FollowUp::ReclaimQueue;
                }
                AgentStreamPayload::ConfigOptionsChanged { .. }
                | AgentStreamPayload::ModeChanged { .. } => state.error = None,
                AgentStreamPayload::SessionsListed { .. }
                | AgentStreamPayload::SessionDeleted { .. }
                | AgentStreamPayload::Parked
                | AgentStreamPayload::PromptsReclaimed { .. }
                | AgentStreamPayload::TurnDiff { .. } => {}
            }
        }
        self.emit(payload);
        match follow {
            FollowUp::None => {}
            FollowUp::DrainQueue => self.dispatch_next(),
            FollowUp::ReclaimQueue => self.reclaim_queue(),
            FollowUp::Reopen => {
                let (cwd, session_id) = {
                    let state = self.state.lock();
                    (state.cwd.clone(), state.session_id.clone())
                };
                self.send(RuntimeCommand::Open {
                    cwd,
                    resume_session: session_id,
                });
            }
        }
    }

    /// Send the prompt, or queue it behind the turn already running. A queued
    /// prompt is dispatched when that turn settles on its own; a turn the user
    /// stops, or one the runtime loses, hands the queue back to the composer.
    fn prompt(&mut self, prompt: AgentPrompt) {
        if prompt.is_empty() {
            return;
        }
        if self.state.lock().phase.accepts_prompt() {
            self.dispatch(prompt);
            return;
        }
        self.queue.push_back(prompt);
        self.publish_queue_depth();
    }

    fn dispatch(&mut self, prompt: AgentPrompt) {
        let cwd = self.state.lock().cwd.clone();
        // Blocking git is fine here: the pane's own thread is the only thing
        // it stalls, and a pane outside a worktree simply keeps no base.
        self.turn_base = match turn_snapshot::snapshot_tree(&cwd) {
            Ok(base) => Some(base),
            Err(error) => {
                log::debug!(
                    target: "zz::agent",
                    "no turn snapshot for pane {}: {error}",
                    self.pane
                );
                None
            }
        };
        {
            let mut state = self.state.lock();
            state.phase = AgentConnectionPhase::Running;
            state.error = None;
            state.last_activity = Instant::now();
        }
        if let Err(error) = self.commands.try_send(RuntimeCommand::Prompt { prompt }) {
            let RuntimeCommand::Prompt { prompt } = error.into_inner() else {
                return;
            };
            self.queue.push_front(prompt);
            self.reclaim_queue();
        }
    }

    /// Start the next queued prompt now that the pane accepts prompts again.
    fn dispatch_next(&mut self) {
        let Some(prompt) = self.queue.pop_front() else {
            return;
        };
        self.publish_queue_depth();
        self.dispatch(prompt);
    }

    /// Hand every queued prompt back. The at-least-once rule: a prompt the
    /// user typed is either sent or offered back to the composer, never
    /// silently dropped — images included.
    fn reclaim_queue(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let prompts = self.queue.drain(..).collect::<Vec<_>>();
        self.publish_queue_depth();
        self.emit(AgentStreamPayload::PromptsReclaimed { prompts });
    }

    fn cancel(&mut self) {
        if !self.state.lock().phase.has_active_turn() {
            return;
        }
        self.state.lock().phase = AgentConnectionPhase::Cancelling;
        self.send(RuntimeCommand::Cancel);
        self.reclaim_queue();
    }

    /// Finalize a turn the agent went quiet on without touching the child: the
    /// pane accepts prompts again and the queue moves on.
    fn park(&mut self) {
        let window = *self.park_window.lock();
        if !self.state.lock().park_due(window) {
            return;
        }
        log::info!(
            target: "zz::agent",
            "parking quiet turn for pane {}; agent process left alone",
            self.pane
        );
        {
            let mut state = self.state.lock();
            state.phase = AgentConnectionPhase::Ready;
            settle_turn(&mut state);
        }
        self.emit(AgentStreamPayload::Parked);
        self.dispatch_next();
    }

    fn turn_diff(&mut self, request_id: u64) {
        let cwd = self.state.lock().cwd.clone();
        let outcome = match self.turn_base.as_ref() {
            Some(base) => match turn_snapshot::capture_turn_diff(&cwd, base) {
                Ok(diff) => AgentTurnDiffOutcome::Captured { diff },
                Err(message) => AgentTurnDiffOutcome::Failed { message },
            },
            None => AgentTurnDiffOutcome::Failed {
                message: "this pane has no turn to diff".to_owned(),
            },
        };
        self.emit(AgentStreamPayload::TurnDiff {
            request_id,
            outcome,
        });
    }

    fn close(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.reclaim_queue();
        self.send(RuntimeCommand::Shutdown);
    }

    /// The adapter is gone. Outstanding permissions resolve cancelled, the
    /// queue comes back, and an exit nobody asked for is reported as a failure.
    fn finish(&mut self, result: &Result<(), String>) {
        let pending = std::mem::take(&mut self.state.lock().pending_permissions);
        for permission in pending {
            self.emit(AgentStreamPayload::PermissionResolved {
                request_id: permission.request_id,
                canceled: true,
            });
        }
        self.reclaim_queue();
        if self.closing {
            self.state.lock().phase = AgentConnectionPhase::Disconnected;
            return;
        }
        let message = result.as_ref().err().map_or_else(
            || "agent process disconnected unexpectedly".to_owned(),
            String::clone,
        );
        {
            let mut state = self.state.lock();
            state.phase = AgentConnectionPhase::Failed;
            state.error = Some(message.clone());
            settle_turn(&mut state);
        }
        self.emit(AgentStreamPayload::PaneFailed { message });
    }

    fn send(&self, command: RuntimeCommand) {
        if self.commands.try_send(command).is_err() {
            log::debug!(
                target: "zz::agent",
                "dropping a command for pane {}: its runtime is gone",
                self.pane
            );
        }
    }

    fn publish_queue_depth(&self) {
        self.state.lock().queued_prompts = self.queue.len();
    }

    fn emit(&mut self, payload: AgentStreamPayload) {
        self.seq = self.seq.saturating_add(1);
        self.state.lock().last_seq = self.seq;
        (self.sink)(
            self.pane,
            AgentStreamItem {
                seq: self.seq,
                payload,
            },
        );
    }
}

/// A turn is over: nothing the agent still owes an answer for survives it.
fn settle_turn(state: &mut AgentPaneState) {
    state.pending_permissions.clear();
    state.live_tools.clear();
    state.live_tasks.clear();
}

fn track_tool_call(state: &mut AgentPaneState, update: &Value) {
    let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
        return;
    };
    if kind != "tool_call" && kind != "tool_call_update" {
        return;
    }
    let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
        return;
    };
    match update.get("status").and_then(Value::as_str) {
        Some("pending" | "in_progress") => {
            state.live_tools.insert(id.to_owned());
        }
        Some(_) => {
            state.live_tools.remove(id);
        }
        // A tool call announced without a status is pending by definition; an
        // update without one only carries output.
        None if kind == "tool_call" => {
            state.live_tools.insert(id.to_owned());
        }
        None => {}
    }
}

fn track_task(state: &mut AgentPaneState, event: &SdkTaskEvent) {
    match event {
        SdkTaskEvent::Started { task_id, .. } => {
            state.live_tasks.insert(task_id.clone());
        }
        SdkTaskEvent::Notification(notification) => {
            state.live_tasks.remove(&notification.task_id);
        }
        SdkTaskEvent::Settled { task_id, .. } => {
            state.live_tasks.remove(task_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use agent_client_protocol::{
        Agent, Client as AcpClientRole, ConnectTo, ConnectionTo,
        schema::v1::{
            AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
            LoadSessionRequest, NewSessionRequest, NewSessionResponse, PermissionOption,
            PermissionOptionId, PermissionOptionKind, PromptRequest, PromptResponse,
            RequestPermissionRequest, SessionNotification, SessionUpdate, StopReason, TextContent,
            ToolCall, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
        },
    };

    use super::*;
    use crate::agent::{
        journal::AgentJournal,
        runtime::run_agent_connection,
        stream::{AgentImage, AgentStreamPayload},
    };

    const DEADLINE: Duration = Duration::from_secs(10);

    /// What the fixture agent does when it is prompted.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Behavior {
        /// One message chunk, then the turn ends.
        Chunk,
        /// A tool call that asks permission before it settles.
        AskPermission,
        /// A turn that never answers, so the pane stays RUNNING.
        Hang,
    }

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<AgentStreamItem>>>);

    impl Recorder {
        fn sink(&self) -> AgentStreamSink {
            let items = Arc::clone(&self.0);
            Box::new(move |_pane, item| items.lock().push(item))
        }

        fn items(&self) -> Vec<AgentStreamItem> {
            self.0.lock().clone()
        }

        fn payloads(&self) -> Vec<AgentStreamPayload> {
            self.items().into_iter().map(|item| item.payload).collect()
        }

        /// Wait for a payload the predicate accepts, returning everything
        /// recorded up to that point.
        fn wait<F>(&self, what: &str, accept: F) -> Vec<AgentStreamPayload>
        where
            F: Fn(&AgentStreamPayload) -> bool,
        {
            let deadline = Instant::now() + DEADLINE;
            while Instant::now() < deadline {
                let payloads = self.payloads();
                if payloads.iter().any(&accept) {
                    return payloads;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            panic!("timed out waiting for {what}: {:?}", self.payloads());
        }
    }

    fn fixture_agent(behavior: Behavior, load: bool) -> impl ConnectTo<AcpClientRole> {
        let prompts = Arc::new(AtomicUsize::new(0));
        Agent
            .builder()
            .on_receive_request(
                async move |initialize: InitializeRequest, responder, _| {
                    responder.respond(
                        InitializeResponse::new(initialize.protocol_version)
                            .agent_capabilities(AgentCapabilities::new().load_session(load)),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async |_: LoadSessionRequest, responder, _| {
                    responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params()
                            .data("fixture session cannot be loaded"),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async |_: NewSessionRequest, responder, _| {
                    responder.respond(NewSessionResponse::new("fixture-session"))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |prompt: PromptRequest,
                            responder,
                            connection: ConnectionTo<AcpClientRole>| {
                    let turn = prompts.fetch_add(1, Ordering::Relaxed);
                    let session_id = prompt.session_id.clone();
                    connection.send_notification(SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new(format!("turn {turn}")),
                        ))),
                    ))?;
                    match behavior {
                        Behavior::Chunk => {
                            responder.respond(PromptResponse::new(StopReason::EndTurn))
                        }
                        Behavior::Hang => Ok(()),
                        // A real adapter answers the prompt from a task of its
                        // own; awaiting the permission inline would wedge the
                        // fixture's own dispatch loop.
                        Behavior::AskPermission => {
                            let tool = ToolCallUpdate::new(
                                "tool-1",
                                ToolCallUpdateFields::new()
                                    .status(ToolCallStatus::Pending)
                                    .title("run it".to_owned()),
                            );
                            let permission =
                                connection.send_request(RequestPermissionRequest::new(
                                    session_id,
                                    tool,
                                    vec![
                                        PermissionOption::new(
                                            PermissionOptionId::new("allow"),
                                            "Allow",
                                            PermissionOptionKind::AllowOnce,
                                        ),
                                        PermissionOption::new(
                                            PermissionOptionId::new("deny"),
                                            "Deny",
                                            PermissionOptionKind::RejectOnce,
                                        ),
                                    ],
                                ));
                            connection.spawn(async move {
                                permission.block_task().await?;
                                responder.respond(PromptResponse::new(StopReason::EndTurn))?;
                                Ok(())
                            })
                        }
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
    }

    struct Fixture {
        host: AgentHost,
        recorder: Recorder,
        pane: PaneId,
    }

    impl Fixture {
        fn open(behavior: Behavior, auto_approve: bool) -> Self {
            Self::build(behavior, auto_approve, true, None, None, None)
        }

        fn build(
            behavior: Behavior,
            auto_approve: bool,
            load: bool,
            journal: Option<&Arc<AgentJournal>>,
            resume_session: Option<String>,
            cwd: Option<PathBuf>,
        ) -> Self {
            let recorder = Recorder::default();
            let host = AgentHost::with_journal(
                AgentSpawnConfig::default(),
                recorder.sink(),
                journal.cloned(),
            );
            let pane = PaneId(7);
            let spec = AgentPaneSpec {
                provider: AgentProvider::Codex,
                cwd: cwd.unwrap_or_else(|| PathBuf::from("/")),
                resume_session,
            };
            let runner: PaneRunner = Box::new(move |channels: RuntimeChannels| {
                Box::pin(run_agent_connection(
                    AgentProvider::Codex,
                    auto_approve,
                    fixture_agent(behavior, load),
                    channels.permission_ids,
                    channels.journal,
                    channels.commands,
                    channels.events,
                ))
            });
            assert!(host.open_with(pane, spec, runner));
            Self {
                host,
                recorder,
                pane,
            }
        }

        fn command(&self, command: HostCommand) {
            assert!(self.host.command(self.pane, command));
        }

        fn prompt(&self, text: &str) {
            self.command(HostCommand::Prompt(AgentPrompt {
                text: text.to_owned(),
                images: Vec::new(),
            }));
        }

        fn state(&self) -> AgentPaneState {
            self.host.snapshot_state(self.pane).expect("pane state")
        }

        fn wait_for_session(&self) {
            self.recorder.wait("the session to be ready", |payload| {
                matches!(payload, AgentStreamPayload::SessionReady { .. })
            });
        }

        fn close(self) {
            if let Some(thread) = self.host.close(self.pane) {
                let _ = thread.join();
            }
        }
    }

    fn chunk_texts(payloads: &[AgentStreamPayload]) -> Vec<String> {
        payloads
            .iter()
            .filter_map(|payload| match payload {
                AgentStreamPayload::Update { update } => update
                    .get("content")
                    .and_then(|content| content.get("text"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                _ => None,
            })
            .collect()
    }

    fn queued_prompt_texts(payloads: &[AgentStreamPayload]) -> Vec<String> {
        payloads
            .iter()
            .filter_map(|payload| match payload {
                AgentStreamPayload::PromptsReclaimed { prompts } => Some(prompts),
                _ => None,
            })
            .flatten()
            .map(|prompt| prompt.text.clone())
            .collect()
    }

    #[test]
    fn a_pane_streams_from_spawn_through_a_prompt_to_a_finished_turn() {
        let fixture = Fixture::open(Behavior::Chunk, false);
        fixture.wait_for_session();
        fixture.prompt("go");
        let payloads = fixture.recorder.wait("the turn to finish", |payload| {
            matches!(payload, AgentStreamPayload::PromptFinished { .. })
        });

        assert!(matches!(payloads[0], AgentStreamPayload::Ready { .. }));
        assert!(matches!(
            payloads[1],
            AgentStreamPayload::SessionReset { restoring: false }
        ));
        assert!(
            matches!(&payloads[2], AgentStreamPayload::SessionReady { session_id, .. } if session_id == "fixture-session")
        );
        assert_eq!(chunk_texts(&payloads), ["turn 0"]);

        let seqs = fixture
            .recorder
            .items()
            .into_iter()
            .map(|item| item.seq)
            .collect::<Vec<_>>();
        assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<_>>());

        let state = fixture.state();
        assert_eq!(state.phase, AgentConnectionPhase::Ready);
        assert_eq!(state.session_id.as_deref(), Some("fixture-session"));
        assert_eq!(state.last_seq, seqs.len() as u64);
        assert_eq!(state.queued_prompts, 0);
        fixture.close();
    }

    #[test]
    fn an_auto_approved_tool_never_reaches_a_client() {
        let fixture = Fixture::open(Behavior::AskPermission, true);
        fixture.wait_for_session();
        fixture.prompt("go");
        let payloads = fixture.recorder.wait("the turn to finish", |payload| {
            matches!(payload, AgentStreamPayload::PromptFinished { .. })
        });

        assert!(
            !payloads
                .iter()
                .any(|payload| matches!(payload, AgentStreamPayload::PermissionRequested { .. })),
            "an auto-approved tool is answered daemon-side: {payloads:?}"
        );
        assert!(
            payloads.iter().any(|payload| matches!(
                payload,
                AgentStreamPayload::Update { update } if update.get("sessionUpdate")
                    .and_then(Value::as_str) == Some("tool_call_update")
            )),
            "the approved tool call still reaches the transcript: {payloads:?}"
        );
        assert!(fixture.state().pending_permissions.is_empty());
        fixture.close();
    }

    #[test]
    fn a_surfaced_permission_waits_for_an_answer_and_then_resolves() {
        let fixture = Fixture::open(Behavior::AskPermission, false);
        fixture.wait_for_session();
        fixture.prompt("go");
        let payloads = fixture.recorder.wait("the permission request", |payload| {
            matches!(payload, AgentStreamPayload::PermissionRequested { .. })
        });

        let AgentStreamPayload::PermissionRequested {
            request_id,
            options,
            ..
        } = payloads
            .iter()
            .find(|payload| matches!(payload, AgentStreamPayload::PermissionRequested { .. }))
            .expect("permission request")
            .clone()
        else {
            unreachable!()
        };
        assert_eq!(options.as_array().map(Vec::len), Some(2));
        let pending = fixture.state().pending_permissions;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, request_id);
        assert!(
            !fixture
                .recorder
                .payloads()
                .iter()
                .any(|payload| matches!(payload, AgentStreamPayload::PromptFinished { .. })),
            "the turn waits for the human"
        );

        fixture.command(HostCommand::RespondPermission {
            request_id,
            option_id: Some("allow".to_owned()),
        });
        let payloads = fixture.recorder.wait("the turn to finish", |payload| {
            matches!(payload, AgentStreamPayload::PromptFinished { .. })
        });
        assert!(payloads.iter().any(|payload| matches!(
            payload,
            AgentStreamPayload::PermissionResolved { request_id: resolved, canceled: false }
                if *resolved == request_id
        )));
        assert!(fixture.state().pending_permissions.is_empty());
        fixture.close();
    }

    #[test]
    fn a_prompt_typed_during_a_turn_queues_and_dispatches_when_it_settles() {
        let fixture = Fixture::open(Behavior::AskPermission, false);
        fixture.wait_for_session();
        fixture.prompt("first");
        let payloads = fixture.recorder.wait("the permission request", |payload| {
            matches!(payload, AgentStreamPayload::PermissionRequested { .. })
        });
        fixture.prompt("second");

        let deadline = Instant::now() + DEADLINE;
        while fixture.state().queued_prompts == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(fixture.state().queued_prompts, 1);

        let AgentStreamPayload::PermissionRequested { request_id, .. } = payloads
            .iter()
            .find(|payload| matches!(payload, AgentStreamPayload::PermissionRequested { .. }))
            .expect("permission request")
            .clone()
        else {
            unreachable!()
        };
        fixture.command(HostCommand::RespondPermission {
            request_id,
            option_id: Some("allow".to_owned()),
        });

        let payloads = fixture.recorder.wait("the queued turn", |payload| {
            matches!(
                payload,
                AgentStreamPayload::Update { update }
                    if update.get("content").and_then(|content| content.get("text"))
                        .and_then(Value::as_str) == Some("turn 1")
            )
        });
        assert_eq!(chunk_texts(&payloads), ["turn 0", "turn 1"]);
        assert_eq!(fixture.state().queued_prompts, 0);
        fixture.close();
    }

    #[test]
    fn unqueueing_hands_the_queued_prompts_back_with_their_images() {
        let fixture = Fixture::open(Behavior::Hang, false);
        fixture.wait_for_session();
        fixture.prompt("running");
        fixture.command(HostCommand::Prompt(AgentPrompt {
            text: "queued".to_owned(),
            images: vec![AgentImage {
                format: "image/png".to_owned(),
                data: b"zz".to_vec(),
            }],
        }));
        let deadline = Instant::now() + DEADLINE;
        while fixture.state().queued_prompts == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }

        fixture.command(HostCommand::Unqueue);
        let payloads = fixture.recorder.wait("the reclaimed prompts", |payload| {
            matches!(payload, AgentStreamPayload::PromptsReclaimed { .. })
        });
        assert_eq!(queued_prompt_texts(&payloads), ["queued"]);
        let AgentStreamPayload::PromptsReclaimed { prompts } = payloads
            .iter()
            .find(|payload| matches!(payload, AgentStreamPayload::PromptsReclaimed { .. }))
            .expect("reclaimed prompts")
            .clone()
        else {
            unreachable!()
        };
        assert_eq!(prompts[0].images[0].data, b"zz");
        assert_eq!(fixture.state().queued_prompts, 0);
        fixture.close();
    }

    #[test]
    fn a_quiet_turn_parks_and_lets_the_queue_move_on() {
        let fixture = Fixture::open(Behavior::Hang, false);
        fixture.wait_for_session();
        fixture.prompt("running");
        fixture.recorder.wait("the turn to start", |payload| {
            matches!(
                payload,
                AgentStreamPayload::Update { update }
                    if update.get("content").and_then(|content| content.get("text"))
                        .and_then(Value::as_str) == Some("turn 0")
            )
        });
        fixture.prompt("queued");

        // The window the ticker would use is process-wide; the park decision
        // itself is what this exercises, so it is driven directly.
        let deadline = Instant::now() + DEADLINE;
        while fixture.state().phase != AgentConnectionPhase::Running && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(20));
        assert!(fixture.state().park_due(Some(Duration::from_millis(10))));
        fixture
            .host
            .set_park_window(Some(Duration::from_millis(10)));
        fixture.command(HostCommand::Park);

        let payloads = fixture.recorder.wait("the parked turn", |payload| {
            matches!(payload, AgentStreamPayload::Parked)
        });
        assert_eq!(chunk_texts(&payloads), ["turn 0"]);
        let payloads = fixture.recorder.wait("the queued turn", |payload| {
            matches!(
                payload,
                AgentStreamPayload::Update { update }
                    if update.get("content").and_then(|content| content.get("text"))
                        .and_then(Value::as_str) == Some("turn 1")
            )
        });
        assert_eq!(chunk_texts(&payloads), ["turn 0", "turn 1"]);
        fixture.close();
    }

    #[test]
    fn a_park_is_refused_while_the_agent_still_owes_an_answer() {
        let fixture = Fixture::open(Behavior::AskPermission, false);
        fixture.wait_for_session();
        fixture.prompt("go");
        fixture.recorder.wait("the permission request", |payload| {
            matches!(payload, AgentStreamPayload::PermissionRequested { .. })
        });
        std::thread::sleep(Duration::from_millis(20));

        assert!(!fixture.state().park_due(Some(Duration::from_millis(10))));
        fixture
            .host
            .set_park_window(Some(Duration::from_millis(10)));
        fixture.command(HostCommand::Park);
        std::thread::sleep(Duration::from_millis(20));
        assert!(
            !fixture
                .recorder
                .payloads()
                .iter()
                .any(|payload| matches!(payload, AgentStreamPayload::Parked)),
            "a pending permission holds the turn open"
        );
        fixture.close();
    }

    #[test]
    fn a_session_the_agent_cannot_load_is_replayed_out_of_the_journal() {
        let directory = tempfile::tempdir().expect("journal directory");
        let journal = Arc::new(AgentJournal::open(directory.path()).expect("open journal"));
        for text in ["first restored", "second restored"] {
            let update = serde_json::to_value(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new(text)),
            )))
            .expect("encode update");
            journal.append("stale-session", &update).expect("append");
        }

        let fixture = Fixture::build(
            Behavior::Chunk,
            false,
            false,
            Some(&journal),
            Some("stale-session".to_owned()),
            None,
        );
        let payloads = fixture.recorder.wait("the restored session", |payload| {
            matches!(payload, AgentStreamPayload::SessionSwitched { .. })
        });

        assert!(matches!(
            payloads[1],
            AgentStreamPayload::SessionReset { restoring: true }
        ));
        let AgentStreamPayload::SessionSwitched {
            session_id, replay, ..
        } = payloads
            .iter()
            .find(|payload| matches!(payload, AgentStreamPayload::SessionSwitched { .. }))
            .expect("restored session")
            .clone()
        else {
            unreachable!()
        };
        assert_eq!(session_id, "fixture-session");
        assert_eq!(
            replay
                .iter()
                .filter_map(|update| update
                    .get("content")
                    .and_then(|content| content.get("text"))
                    .and_then(Value::as_str))
                .collect::<Vec<_>>(),
            ["first restored", "second restored"]
        );
        assert!(
            journal.replay("stale-session").expect("replay").is_empty(),
            "the superseded journal is not left behind to be restored twice"
        );
        assert_eq!(fixture.state().phase, AgentConnectionPhase::Ready);
        fixture.close();
    }

    #[test]
    fn a_pane_outside_a_worktree_reports_that_it_has_no_turn_to_diff() {
        let scratch = tempfile::tempdir().expect("temporary directory");
        let fixture = Fixture::build(
            Behavior::Chunk,
            false,
            true,
            None,
            None,
            Some(scratch.path().to_path_buf()),
        );
        fixture.wait_for_session();
        fixture.prompt("go");
        fixture.recorder.wait("the turn to finish", |payload| {
            matches!(payload, AgentStreamPayload::PromptFinished { .. })
        });

        fixture.command(HostCommand::TurnDiff { request_id: 3 });
        let payloads = fixture.recorder.wait("the turn diff", |payload| {
            matches!(payload, AgentStreamPayload::TurnDiff { .. })
        });
        assert!(payloads.iter().any(|payload| matches!(
            payload,
            AgentStreamPayload::TurnDiff {
                request_id: 3,
                outcome: AgentTurnDiffOutcome::Failed { .. }
            }
        )));
        fixture.close();
    }

    #[test]
    fn closing_a_pane_cancels_what_it_still_owed() {
        let fixture = Fixture::open(Behavior::AskPermission, false);
        fixture.wait_for_session();
        fixture.prompt("go");
        fixture.recorder.wait("the permission request", |payload| {
            matches!(payload, AgentStreamPayload::PermissionRequested { .. })
        });
        fixture.command(HostCommand::Prompt(AgentPrompt {
            text: "queued".to_owned(),
            images: Vec::new(),
        }));
        let deadline = Instant::now() + DEADLINE;
        while fixture.state().queued_prompts == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }

        let recorder = fixture.recorder.clone();
        let pane = fixture.pane;
        let thread = fixture.host.close(pane).expect("the pane thread");
        thread.join().expect("the pane thread should settle");

        let payloads = recorder.payloads();
        assert_eq!(queued_prompt_texts(&payloads), ["queued"]);
        assert!(
            payloads.iter().any(|payload| matches!(
                payload,
                AgentStreamPayload::PermissionResolved { canceled: true, .. }
            )),
            "a closed pane owes no permission answer: {payloads:?}"
        );
        assert!(
            !payloads
                .iter()
                .any(|payload| matches!(payload, AgentStreamPayload::PaneFailed { .. })),
            "closing on purpose is not a failure: {payloads:?}"
        );
        assert!(fixture.host.snapshot_state(pane).is_none());
    }

    #[test]
    fn tool_calls_and_tasks_hold_a_turn_open_until_they_settle() {
        let mut state = AgentPaneState::new(&AgentPaneSpec {
            provider: AgentProvider::Codex,
            cwd: PathBuf::from("/"),
            resume_session: None,
        });
        let call = serde_json::to_value(SessionUpdate::ToolCall(
            ToolCall::new("tool-1", "run it").status(ToolCallStatus::Pending),
        ))
        .expect("encode tool call");
        let done = serde_json::to_value(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tool-1",
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        )))
        .expect("encode tool call update");

        track_tool_call(&mut state, &call);
        assert!(state.turn_in_flight(), "an unresolved tool call: {call}");
        track_tool_call(&mut state, &done);
        assert!(!state.turn_in_flight(), "a settled tool call: {done}");

        track_task(
            &mut state,
            &SdkTaskEvent::Started {
                task_id: "t-1".to_owned(),
                tool_use_id: "u-1".to_owned(),
                is_agent: true,
            },
        );
        assert!(state.turn_in_flight());
        track_task(
            &mut state,
            &SdkTaskEvent::Settled {
                task_id: "t-1".to_owned(),
                status: "completed".to_owned(),
            },
        );
        assert!(!state.turn_in_flight());
    }
}
