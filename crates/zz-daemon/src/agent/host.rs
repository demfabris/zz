#[cfg(test)]
use std::time::{Duration, Instant};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, atomic::AtomicU64},
    thread::{self, JoinHandle},
};

use async_channel::{Receiver, Sender};
use parking_lot::Mutex;
use serde_json::Value;
#[cfg(test)]
use zz_protocol::ClientInstanceId;
use zz_protocol::{
    AgentGitSummary, AgentProvider, ClientId, MAX_AGENT_PROMPT_BYTES, MAX_AGENT_QUEUED_PROMPTS,
    PaneId,
};

use crate::agent::{
    environment::{AgentWorkspaceEnvironment, warm_adapter_cache},
    git_summary,
    journal::AgentJournal,
    runtime::{AgentSpawnConfig, RuntimeCommand, RuntimeControl, run_agent_runtime},
    stream::{
        AgentAuthMethod, AgentPrompt, AgentPromptOutcome, AgentSessionCapabilities,
        AgentSessionSummary, AgentStreamItem, AgentStreamPayload,
    },
};

const PANE_INBOX_CAPACITY: usize = 64;
const RUNTIME_COMMAND_CAPACITY: usize = 32;
const RUNTIME_CONTROL_CAPACITY: usize = 32;
const RUNTIME_EVENT_CAPACITY: usize = 64;

/// Where every item a pane produces goes, with the pane state it left behind.
/// A call carrying no item is a state-only change — a queued prompt, say —
/// which a badge still has to see.
pub(crate) type AgentStreamSink =
    Box<dyn Fn(PaneId, u64, AgentPaneState, Option<AgentStreamItem>) + Send + Sync>;

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
        client: ClientId,
        cwd: Option<PathBuf>,
        cursor: Option<String>,
        replace: bool,
    },
    NewSession {
        cwd: PathBuf,
    },
    SwitchSession {
        session: AgentSessionSummary,
    },
    DeleteSession {
        client: ClientId,
        session_id: String,
    },
}

/// What an agent pane is opened against.
#[derive(Clone, Debug)]
pub(crate) struct AgentPaneSpec {
    pub(crate) provider: AgentProvider,
    pub(crate) cwd: PathBuf,
    pub(crate) resume_session: Option<String>,
    pub(crate) workspace: AgentWorkspaceEnvironment,
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
    pub(crate) git: Option<AgentGitSummary>,
}

impl AgentPaneState {
    fn new(spec: &AgentPaneSpec) -> Self {
        Self {
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
            git: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::new(&AgentPaneSpec {
            provider: AgentProvider::Codex,
            cwd: PathBuf::from("/"),
            resume_session: None,
            workspace: AgentWorkspaceEnvironment::default(),
        })
    }
}

enum PaneInput {
    Command(HostCommand),
    Event(AgentStreamPayload),
    GitSummary {
        generation: u64,
        refresh: u64,
        cwd: PathBuf,
        summary: Option<AgentGitSummary>,
    },
    Finished(Result<(), String>),
}

struct PaneHandle {
    inbox: Sender<PaneInput>,
    control: Sender<HostCommand>,
    prompts: Sender<AgentPrompt>,
    close: Sender<()>,
    state: Arc<Mutex<AgentPaneState>>,
    thread: JoinHandle<()>,
}

#[derive(Default)]
struct PaneRegistry {
    panes: Mutex<BTreeMap<PaneId, PaneHandle>>,
}

pub(crate) struct AgentHost {
    config: Mutex<AgentSpawnConfig>,
    sink: Arc<AgentStreamSink>,
    journal: Option<Arc<AgentJournal>>,
    registry: Arc<PaneRegistry>,
    permission_ids: Arc<AtomicU64>,
    #[cfg(test)]
    runner_factory: Mutex<Option<PaneRunnerFactory>>,
}

/// What a pane's runtime is handed when its thread starts. Boxed so tests can
/// swap the adapter child for an in-process fixture.
pub(crate) struct RuntimeChannels {
    pub(crate) permission_ids: Arc<AtomicU64>,
    pub(crate) journal: Option<Arc<AgentJournal>>,
    pub(crate) commands: Receiver<RuntimeCommand>,
    pub(crate) controls: Receiver<RuntimeControl>,
    pub(crate) events: Sender<AgentStreamPayload>,
}

pub(crate) type PaneRunner = Box<
    dyn FnOnce(RuntimeChannels) -> Pin<Box<dyn Future<Output = Result<(), String>>>>
        + Send
        + 'static,
>;

/// Builds the runner a pane's thread drives. Tests swap the adapter child for
/// an in-process fixture through it; the daemon never sets one.
#[cfg(test)]
pub(crate) type PaneRunnerFactory = Box<dyn Fn(&AgentPaneSpec) -> PaneRunner + Send + Sync>;

impl AgentHost {
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
            #[cfg(test)]
            runner_factory: Mutex::new(None),
        }
    }

    /// Open every pane against an in-process fixture instead of an adapter
    /// child.
    #[cfg(test)]
    pub(crate) fn set_runner_factory(&self, factory: PaneRunnerFactory) {
        *self.runner_factory.lock() = Some(factory);
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

    #[cfg(test)]
    pub(crate) fn config(&self) -> AgentSpawnConfig {
        self.config.lock().clone()
    }

    #[cfg(test)]
    pub(crate) fn pane_count(&self) -> usize {
        self.registry.panes.lock().len()
    }

    pub(crate) fn contains(&self, pane: PaneId) -> bool {
        self.registry.panes.lock().contains_key(&pane)
    }

    pub(crate) fn open(&self, pane: PaneId, generation: u64, spec: AgentPaneSpec) -> bool {
        #[cfg(test)]
        if let Some(runner) = self
            .runner_factory
            .lock()
            .as_ref()
            .map(|factory| factory(&spec))
        {
            return self.open_with(pane, generation, spec, runner);
        }
        let mut config = self.config.lock().clone();
        config.workspace.adopt_pane_identity(&spec.workspace);
        let provider = spec.provider;
        let runner: PaneRunner = Box::new(move |channels: RuntimeChannels| {
            Box::pin(run_agent_runtime(
                config,
                provider,
                channels.permission_ids,
                channels.journal,
                channels.commands,
                channels.controls,
                channels.events,
            ))
        });
        self.open_with(pane, generation, spec, runner)
    }

    pub(crate) fn open_with(
        &self,
        pane: PaneId,
        generation: u64,
        spec: AgentPaneSpec,
        runner: PaneRunner,
    ) -> bool {
        let mut panes = self.registry.panes.lock();
        if panes.contains_key(&pane) {
            return false;
        }
        let state = Arc::new(Mutex::new(AgentPaneState::new(&spec)));
        let pane_state = Arc::clone(&state);
        let sink = Arc::clone(&self.sink);
        let permission_ids = Arc::clone(&self.permission_ids);
        let journal = self.journal.clone();
        let (inbox_tx, inbox_rx) = async_channel::bounded(PANE_INBOX_CAPACITY);
        let (control_tx, control_rx) = async_channel::bounded(RUNTIME_CONTROL_CAPACITY);
        let (prompt_tx, prompt_rx) = async_channel::bounded(MAX_AGENT_QUEUED_PROMPTS);
        let (close_tx, close_rx) = async_channel::bounded::<()>(1);
        let inbox = inbox_tx.clone();
        let pane_close = close_tx.clone();
        let thread = std::thread::Builder::new()
            .name(format!("zz-agent-{}", pane.0))
            .spawn(move || {
                futures_lite::future::block_on(run_pane(
                    pane,
                    generation,
                    spec,
                    pane_state,
                    sink,
                    runner,
                    permission_ids,
                    journal,
                    inbox,
                    inbox_rx,
                    control_rx,
                    prompt_rx,
                    pane_close,
                    close_rx,
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
                control: control_tx,
                prompts: prompt_tx,
                close: close_tx,
                state,
                thread,
            },
        );
        true
    }

    pub(crate) fn command(&self, pane: PaneId, command: HostCommand) -> Result<(), HostCommand> {
        let panes = self.registry.panes.lock();
        let Some(handle) = panes.get(&pane) else {
            return Err(command);
        };
        if let HostCommand::Prompt(prompt) = command {
            return handle
                .prompts
                .try_send(prompt)
                .map_err(|error| HostCommand::Prompt(error.into_inner()));
        }
        if matches!(
            &command,
            HostCommand::Cancel | HostCommand::RespondPermission { .. }
        ) {
            return handle
                .control
                .try_send(command)
                .map_err(async_channel::TrySendError::into_inner);
        }
        handle
            .inbox
            .try_send(PaneInput::Command(command))
            .map_err(|error| match error.into_inner() {
                PaneInput::Command(command) => command,
                PaneInput::Event(_) | PaneInput::GitSummary { .. } | PaneInput::Finished(_) => {
                    unreachable!()
                }
            })
    }

    /// Stop a pane's runtime. The returned handle is the pane's thread, which
    /// settles once the adapter has been told to close; callers that care join
    /// it, the daemon's own teardown does not.
    pub(crate) fn close(&self, pane: PaneId) -> Option<JoinHandle<()>> {
        let handle = self.registry.panes.lock().remove(&pane)?;
        handle.close.close();
        Some(handle.thread)
    }

    pub(crate) fn snapshot_state(&self, pane: PaneId) -> Option<AgentPaneState> {
        let panes = self.registry.panes.lock();
        panes.get(&pane).map(|handle| handle.state.lock().clone())
    }

    pub(crate) fn shutdown(&self) {
        let handles = std::mem::take(&mut *self.registry.panes.lock());
        for handle in handles.values() {
            handle.close.close();
        }
        for handle in handles.into_values() {
            let _ = handle.thread.join();
        }
    }
}

impl Drop for AgentHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A pane's whole life on one thread: the ACP connection, the forwarder that
/// hands its payloads to the pump, and the pump itself.
async fn run_pane(
    pane: PaneId,
    generation: u64,
    spec: AgentPaneSpec,
    state: Arc<Mutex<AgentPaneState>>,
    sink: Arc<AgentStreamSink>,
    runner: PaneRunner,
    permission_ids: Arc<AtomicU64>,
    journal: Option<Arc<AgentJournal>>,
    inbox_tx: Sender<PaneInput>,
    inbox_rx: Receiver<PaneInput>,
    host_control_rx: Receiver<HostCommand>,
    prompt_rx: Receiver<AgentPrompt>,
    close_tx: Sender<()>,
    close_rx: Receiver<()>,
) {
    let (command_tx, command_rx) = async_channel::bounded(RUNTIME_COMMAND_CAPACITY);
    let (runtime_control_tx, runtime_control_rx) = async_channel::bounded(RUNTIME_CONTROL_CAPACITY);
    let (event_tx, event_rx) = async_channel::bounded(RUNTIME_EVENT_CAPACITY);
    let runtime_close = close_rx.clone();
    let closer = event_tx.clone();
    let outcome = Arc::new(Mutex::new(None));

    let runtime_outcome = Arc::clone(&outcome);
    let runtime = async move {
        let result = futures_lite::future::race(
            runner(RuntimeChannels {
                permission_ids,
                journal,
                commands: command_rx,
                controls: runtime_control_rx,
                events: event_tx,
            }),
            async move {
                let _ = runtime_close.recv().await;
                Ok(())
            },
        )
        .await;
        *runtime_outcome.lock() = Some(result);
        // The runtime is done, so nothing may keep the payload channel open:
        // the forwarder below ends on its close, and only then is the pane's
        // outcome final.
        closer.close();
    };

    let pump_inbox = inbox_tx.clone();
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
        generation,
        spec,
        state,
        sink,
        inbox: pump_inbox,
        commands: command_tx,
        controls: runtime_control_tx,
        close: close_tx,
        seq: 0,
        queue: VecDeque::new(),
        git_refresh: 0,
        next_turn_id: 0,
        active_turn: None,
        dispatched_prompt: None,
        closing: false,
    };

    futures_lite::future::zip(
        futures_lite::future::zip(runtime, forward),
        pump.run(inbox_rx, host_control_rx, prompt_rx, close_rx),
    )
    .await;
}

/// The host-side half of a pane: what the desktop controller did between its
/// runtime and its reducer.
struct PanePump {
    pane: PaneId,
    generation: u64,
    spec: AgentPaneSpec,
    state: Arc<Mutex<AgentPaneState>>,
    sink: Arc<AgentStreamSink>,
    inbox: Sender<PaneInput>,
    commands: Sender<RuntimeCommand>,
    controls: Sender<RuntimeControl>,
    close: Sender<()>,
    seq: u64,
    queue: VecDeque<AgentPrompt>,
    git_refresh: u64,
    next_turn_id: u64,
    active_turn: Option<u64>,
    dispatched_prompt: Option<(u64, AgentPrompt)>,
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
    async fn run(
        mut self,
        inbox: Receiver<PaneInput>,
        control: Receiver<HostCommand>,
        prompts: Receiver<AgentPrompt>,
        close: Receiver<()>,
    ) {
        self.send(RuntimeCommand::Open {
            cwd: self.spec.cwd.clone(),
            resume_session: self.spec.resume_session.clone(),
        });
        loop {
            let input = futures_lite::future::race(
                async {
                    let _ = close.recv().await;
                    None
                },
                futures_lite::future::race(
                    async { control.recv().await.ok().map(PaneInput::Command) },
                    futures_lite::future::race(
                        async {
                            prompts
                                .recv()
                                .await
                                .ok()
                                .map(HostCommand::Prompt)
                                .map(PaneInput::Command)
                        },
                        async { inbox.recv().await.ok() },
                    ),
                ),
            )
            .await;
            let Some(input) = input else {
                self.close();
                self.reclaim_accepted_inputs(&prompts, &inbox);
                break;
            };
            match input {
                PaneInput::Event(payload) => self.observe(payload),
                PaneInput::Command(command) => self.command(command),
                PaneInput::GitSummary {
                    generation,
                    refresh,
                    cwd,
                    summary,
                } => self.apply_git_summary(generation, refresh, &cwd, summary),
                PaneInput::Finished(result) => {
                    self.finish(&result);
                    self.reclaim_accepted_inputs(&prompts, &inbox);
                    break;
                }
            }
        }
    }

    fn reclaim_accepted_inputs(
        &mut self,
        prompts: &Receiver<AgentPrompt>,
        inbox: &Receiver<PaneInput>,
    ) {
        while let Ok(prompt) = prompts.try_recv() {
            self.emit(AgentStreamPayload::PromptsReclaimed {
                prompts: vec![prompt],
            });
        }
        while let Ok(input) = inbox.try_recv() {
            if let PaneInput::Command(HostCommand::Prompt(prompt)) = input {
                self.emit(AgentStreamPayload::PromptsReclaimed {
                    prompts: vec![prompt],
                });
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
            } => {
                if !self.send_control(RuntimeControl::RespondPermission {
                    request_id,
                    option_id,
                }) {
                    self.fail_control();
                }
            }
            HostCommand::Authenticate { method_id } => {
                if !self.send(RuntimeCommand::Authenticate { method_id }) {
                    self.observe(AgentStreamPayload::AuthenticationFailed {
                        message: "agent runtime is busy".to_owned(),
                    });
                }
            }
            HostCommand::SetConfigOption { option_id, value } => {
                if !self.send(RuntimeCommand::SetConfigOption {
                    option_id: option_id.clone(),
                    value,
                }) {
                    self.observe(AgentStreamPayload::SettingFailed {
                        option_id,
                        message: "agent runtime is busy".to_owned(),
                    });
                }
            }
            HostCommand::SetMode { mode_id } => {
                if !self.send(RuntimeCommand::SetMode { mode_id }) {
                    self.observe(AgentStreamPayload::SettingFailed {
                        option_id: "legacy-session-mode".to_owned(),
                        message: "agent runtime is busy".to_owned(),
                    });
                }
            }
            HostCommand::ListSessions {
                client,
                cwd,
                cursor,
                replace,
            } => {
                if !self.send(RuntimeCommand::ListSessions {
                    client,
                    cwd,
                    cursor,
                    replace,
                }) {
                    self.observe(AgentStreamPayload::SessionListFailed {
                        client,
                        message: "agent runtime is busy".to_owned(),
                    });
                }
            }
            HostCommand::NewSession { cwd } => self.begin_session_change(
                RuntimeCommand::NewSession { cwd },
                AgentConnectionPhase::Starting,
            ),
            HostCommand::SwitchSession { session } => {
                self.begin_session_change(
                    RuntimeCommand::SwitchSession { session },
                    AgentConnectionPhase::Restoring,
                );
            }
            HostCommand::DeleteSession { client, session_id } => {
                if !self.send(RuntimeCommand::DeleteSession { client, session_id }) {
                    self.observe(AgentStreamPayload::SessionDeleteFailed {
                        client,
                        message: "agent runtime is busy".to_owned(),
                    });
                }
            }
        }
    }

    fn begin_session_change(&mut self, command: RuntimeCommand, phase: AgentConnectionPhase) {
        let accepted = {
            let mut state = self.state.lock();
            if state.phase != AgentConnectionPhase::Ready
                || !state.pending_permissions.is_empty()
                || self.active_turn.is_some()
            {
                false
            } else {
                state.phase = phase;
                state.error = None;
                true
            }
        };
        if !accepted {
            self.emit(AgentStreamPayload::SessionSwitchFailed {
                message: "finish or cancel the current turn before changing sessions".to_owned(),
            });
            return;
        }
        let state = self.state.lock().clone();
        (self.sink)(self.pane, self.generation, state, None);
        if !self.send(command) {
            self.observe(AgentStreamPayload::SessionSwitchFailed {
                message: "agent runtime is unavailable".to_owned(),
            });
        }
    }

    fn observe(&mut self, payload: AgentStreamPayload) {
        if matches!(&payload, AgentStreamPayload::PromptAccepted { .. }) {
            return;
        }
        if let AgentStreamPayload::PromptFinished { turn_id, .. } = &payload
            && self.active_turn != Some(*turn_id)
        {
            return;
        }
        if matches!(
            &payload,
            AgentStreamPayload::AuthenticationFailed { .. } | AgentStreamPayload::PaneFailed { .. }
        ) {
            self.reclaim_dispatched_prompt();
        }
        let mut follow = FollowUp::None;
        if matches!(
            &payload,
            AgentStreamPayload::SessionReset { .. } | AgentStreamPayload::SessionSwitched { .. }
        ) {
            self.active_turn = None;
            self.reclaim_dispatched_prompt();
        }
        if matches!(&payload, AgentStreamPayload::SessionReset { .. }) {
            self.git_refresh = self.git_refresh.saturating_add(1);
        }
        let refresh_git = matches!(
            &payload,
            AgentStreamPayload::SessionReady { .. }
                | AgentStreamPayload::SessionSwitched { .. }
                | AgentStreamPayload::PromptFinished { .. }
        );
        {
            let mut state = self.state.lock();
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
                    state.git = None;
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
                AgentStreamPayload::PromptAccepted { .. } => unreachable!(),
                AgentStreamPayload::SessionSwitchFailed { message } => {
                    state.phase = AgentConnectionPhase::Ready;
                    state.error = Some(message.clone());
                    follow = FollowUp::DrainQueue;
                }
                AgentStreamPayload::SettingFailed { message, .. } => {
                    state.error = Some(message.clone());
                }
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
                AgentStreamPayload::PromptFinished { outcome, .. } => {
                    self.active_turn = None;
                    self.dispatched_prompt = None;
                    match outcome {
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
                    }
                }
                AgentStreamPayload::Authenticated => {
                    state.error = None;
                    follow = FollowUp::Reopen;
                }
                AgentStreamPayload::AuthenticationFailed { message }
                | AgentStreamPayload::PaneFailed { message } => {
                    self.active_turn = None;
                    state.phase = AgentConnectionPhase::Failed;
                    state.error = Some(message.clone());
                    settle_turn(&mut state);
                    follow = FollowUp::ReclaimQueue;
                }
                AgentStreamPayload::ConfigOptionsChanged { .. }
                | AgentStreamPayload::ModeChanged { .. } => state.error = None,
                AgentStreamPayload::Update { .. }
                | AgentStreamPayload::StateSynced { .. }
                | AgentStreamPayload::TurnStarted { .. }
                | AgentStreamPayload::SessionsListed { .. }
                | AgentStreamPayload::SessionListFailed { .. }
                | AgentStreamPayload::SessionDeleted { .. }
                | AgentStreamPayload::SessionDeleteFailed { .. }
                | AgentStreamPayload::PromptsReclaimed { .. }
                | AgentStreamPayload::PromptsRestored { .. } => {}
            }
        }
        self.emit(payload);
        if refresh_git {
            self.start_git_refresh();
        }
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
        let queued_bytes = self.queue.iter().fold(0usize, |total, queued| {
            total.saturating_add(queued.byte_len())
        });
        if self.queue.len() >= MAX_AGENT_QUEUED_PROMPTS
            || queued_bytes.saturating_add(prompt.byte_len()) > MAX_AGENT_PROMPT_BYTES
        {
            self.emit(AgentStreamPayload::PromptsReclaimed {
                prompts: vec![prompt],
            });
            return;
        }
        self.queue.push_back(prompt);
        self.publish_queue_depth();
    }

    fn dispatch(&mut self, prompt: AgentPrompt) {
        self.next_turn_id = self.next_turn_id.saturating_add(1).max(1);
        let turn_id = self.next_turn_id;
        self.active_turn = Some(turn_id);
        {
            let mut state = self.state.lock();
            state.phase = AgentConnectionPhase::Running;
            state.error = None;
        }
        let retained = prompt.clone();
        match self
            .commands
            .try_send(RuntimeCommand::Prompt { turn_id, prompt })
        {
            Ok(()) => {
                self.dispatched_prompt = Some((turn_id, retained));
                self.emit(AgentStreamPayload::TurnStarted { turn_id });
            }
            Err(error) => {
                self.active_turn = None;
                {
                    let mut state = self.state.lock();
                    state.phase = AgentConnectionPhase::Ready;
                    settle_turn(&mut state);
                }
                let RuntimeCommand::Prompt { prompt, .. } = error.into_inner() else {
                    return;
                };
                self.queue.push_front(prompt);
                self.reclaim_queue();
            }
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
        let session_id = self.state.lock().session_id.clone();
        let Some(turn_id) = self.active_turn else {
            return;
        };
        if !self.send_control(RuntimeControl::Cancel {
            turn_id,
            session_id,
        }) {
            self.fail_control();
            return;
        }
        self.state.lock().phase = AgentConnectionPhase::Cancelling;
        self.reclaim_queue();
    }

    fn start_git_refresh(&mut self) {
        self.git_refresh = self.git_refresh.saturating_add(1);
        let generation = self.generation;
        let refresh = self.git_refresh;
        let cwd = self.state.lock().cwd.clone();
        let capture_cwd = cwd.clone();
        let inbox = self.inbox.clone();
        let pane = self.pane;
        if let Err(error) = thread::Builder::new()
            .name(format!("zz-agent-git-{}-{refresh}", pane.0))
            .spawn(move || {
                let summary = match git_summary::capture_git_summary(&capture_cwd) {
                    Ok(summary) => Some(summary),
                    Err(error) => {
                        log::debug!(target: "zz::agent", "no Git summary for pane {pane}: {error}");
                        None
                    }
                };
                let _ = inbox.send_blocking(PaneInput::GitSummary {
                    generation,
                    refresh,
                    cwd,
                    summary,
                });
            })
        {
            log::warn!(target: "zz::agent", "could not start Git summary capture for pane {pane}: {error}");
        }
    }

    fn apply_git_summary(
        &mut self,
        generation: u64,
        refresh: u64,
        cwd: &Path,
        summary: Option<AgentGitSummary>,
    ) {
        if generation != self.generation || refresh != self.git_refresh {
            return;
        }
        let state = {
            let mut state = self.state.lock();
            if state.cwd != cwd || state.git == summary {
                return;
            }
            state.git = summary;
            state.clone()
        };
        (self.sink)(self.pane, self.generation, state, None);
    }

    fn close(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.resolve_pending_permissions();
        self.reclaim_dispatched_prompt();
        self.reclaim_queue();
        self.send(RuntimeCommand::Shutdown);
        self.close.close();
    }

    fn resolve_pending_permissions(&mut self) {
        let pending = std::mem::take(&mut self.state.lock().pending_permissions);
        for permission in pending {
            self.emit(AgentStreamPayload::PermissionResolved {
                request_id: permission.request_id,
                canceled: true,
            });
        }
    }

    fn reclaim_dispatched_prompt(&mut self) {
        let Some((_, prompt)) = self.dispatched_prompt.take() else {
            return;
        };
        self.emit(AgentStreamPayload::PromptsReclaimed {
            prompts: vec![prompt],
        });
    }

    fn finish(&mut self, result: &Result<(), String>) {
        self.resolve_pending_permissions();
        self.reclaim_dispatched_prompt();
        self.reclaim_queue();
        if self.closing || self.close.is_closed() {
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

    fn send(&self, command: RuntimeCommand) -> bool {
        if self.commands.try_send(command).is_err() {
            log::debug!(
                target: "zz::agent",
                "dropping a command for pane {}: its runtime is gone",
                self.pane
            );
            return false;
        }
        true
    }

    fn send_control(&self, control: RuntimeControl) -> bool {
        self.controls.try_send(control).is_ok()
    }

    fn fail_control(&mut self) {
        let message = "agent runtime control queue is unavailable".to_owned();
        {
            let mut state = self.state.lock();
            state.phase = AgentConnectionPhase::Failed;
            state.error = Some(message.clone());
            settle_turn(&mut state);
        }
        self.active_turn = None;
        self.emit(AgentStreamPayload::PaneFailed { message });
        self.close.close();
    }

    fn publish_queue_depth(&self) {
        let state = {
            let mut state = self.state.lock();
            state.queued_prompts = self.queue.len();
            state.clone()
        };
        (self.sink)(self.pane, self.generation, state, None);
    }

    fn emit(&mut self, payload: AgentStreamPayload) {
        let state = {
            let mut state = self.state.lock();
            self.seq = self.seq.saturating_add(1);
            state.last_seq = self.seq;
            state.clone()
        };
        (self.sink)(
            self.pane,
            self.generation,
            state,
            Some(AgentStreamItem {
                seq: self.seq,
                payload,
            }),
        );
    }
}

/// A turn is over: nothing the agent still owes an answer for survives it.
fn settle_turn(state: &mut AgentPaneState) {
    state.pending_permissions.clear();
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::{
        ContentBlock, ContentChunk, MessageId, SessionUpdate, TextContent,
    };

    use super::*;
    use crate::agent::{
        fixture::{Behavior, fixture_runner},
        journal::AgentJournal,
        stream::{AgentImage, AgentStreamPayload},
    };
    use std::{fs, process::Command};

    const DEADLINE: Duration = Duration::from_secs(10);

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn seeded_git_repo() -> Option<(tempfile::TempDir, PathBuf)> {
        if !git_available() {
            return None;
        }
        let scratch = tempfile::tempdir().expect("temporary directory");
        let root = scratch.path().canonicalize().expect("resolved path");
        git(&root, &["init", "--quiet", "--initial-branch=main"]);
        git(&root, &["config", "user.email", "git-summary@zz.test"]);
        git(&root, &["config", "user.name", "zz git summary"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("tracked.txt"), "one\ntwo\n").expect("seed file");
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "seed"]);
        Some((scratch, root))
    }

    #[derive(Clone, Default)]
    struct Recorder {
        items: Arc<Mutex<Vec<AgentStreamItem>>>,
        states: Arc<Mutex<Vec<(AgentPaneState, bool)>>>,
    }

    impl Recorder {
        fn sink(&self) -> AgentStreamSink {
            let items = Arc::clone(&self.items);
            let states = Arc::clone(&self.states);
            Box::new(move |_pane, _generation, state, item| {
                states.lock().push((state, item.is_some()));
                if let Some(item) = item {
                    items.lock().push(item);
                }
            })
        }

        fn items(&self) -> Vec<AgentStreamItem> {
            self.items.lock().clone()
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

    struct Fixture {
        host: AgentHost,
        recorder: Recorder,
        pane: PaneId,
    }

    impl Fixture {
        fn open(behavior: Behavior, auto_approve: bool) -> Self {
            Self::build(behavior, auto_approve, true, None, None, None)
        }

        fn open_with_runner(runner: PaneRunner) -> Self {
            let recorder = Recorder::default();
            let host = AgentHost::with_journal(AgentSpawnConfig::default(), recorder.sink(), None);
            let pane = PaneId(8);
            assert!(host.open_with(
                pane,
                1,
                AgentPaneSpec {
                    provider: AgentProvider::Codex,
                    cwd: PathBuf::from("/"),
                    resume_session: None,
                    workspace: AgentWorkspaceEnvironment::default(),
                },
                runner,
            ));
            Self {
                host,
                recorder,
                pane,
            }
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
                workspace: AgentWorkspaceEnvironment::default(),
            };
            let runner = fixture_runner(AgentProvider::Codex, behavior, auto_approve, load);
            assert!(host.open_with(pane, 1, spec, runner));
            Self {
                host,
                recorder,
                pane,
            }
        }

        fn command(&self, command: HostCommand) {
            assert!(self.host.command(self.pane, command).is_ok());
        }

        fn prompt(&self, text: &str) {
            self.command(HostCommand::Prompt(AgentPrompt {
                owner: ClientInstanceId::default(),
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
            .filter_map(|payload| chunk_text(payload).map(ToOwned::to_owned))
            .collect()
    }

    fn chunk_text(payload: &AgentStreamPayload) -> Option<&str> {
        let AgentStreamPayload::Update { update } = payload else {
            return None;
        };
        update
            .get("content")
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
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
    fn dispatch_publishes_running_with_a_turn_boundary_before_the_agent_speaks() {
        let fixture = Fixture::open(Behavior::Hang, false);
        fixture.wait_for_session();
        fixture.prompt("think quietly");
        let deadline = Instant::now() + DEADLINE;
        while !fixture
            .recorder
            .states
            .lock()
            .iter()
            .any(|(state, has_item)| state.phase == AgentConnectionPhase::Running && *has_item)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            fixture
                .recorder
                .states
                .lock()
                .iter()
                .any(|(state, has_item)| {
                    state.phase == AgentConnectionPhase::Running && *has_item
                })
        );
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
    fn a_session_change_cannot_cross_an_active_turn() {
        let fixture = Fixture::open(Behavior::Hang, false);
        fixture.wait_for_session();
        fixture.prompt("running");
        fixture
            .recorder
            .wait("the turn", |payload| chunk_text(payload) == Some("turn 0"));
        fixture.command(HostCommand::NewSession {
            cwd: PathBuf::from("/other"),
        });
        fixture.command(HostCommand::SwitchSession {
            session: AgentSessionSummary {
                session_id: "other".to_owned(),
                cwd: PathBuf::from("/other"),
                additional_directories: Vec::new(),
                title: None,
                updated_at: None,
            },
        });
        let deadline = Instant::now() + DEADLINE;
        loop {
            let payloads = fixture.recorder.payloads();
            if payloads
                .iter()
                .filter(|payload| matches!(payload, AgentStreamPayload::SessionSwitchFailed { .. }))
                .count()
                == 2
            {
                assert!(!payloads.iter().any(|payload| {
                    matches!(payload, AgentStreamPayload::SessionSwitched { .. })
                }));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "session commands were not rejected"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(fixture.state().phase, AgentConnectionPhase::Running);
        fixture.close();
    }

    #[test]
    fn close_preempts_a_runner_that_never_reads_shutdown() {
        let recorder = Recorder::default();
        let host = AgentHost::with_journal(AgentSpawnConfig::default(), recorder.sink(), None);
        let pane = PaneId(30);
        let runner: PaneRunner = Box::new(|_| Box::pin(std::future::pending()));
        assert!(host.open_with(
            pane,
            1,
            AgentPaneSpec {
                provider: AgentProvider::Codex,
                cwd: PathBuf::from("/"),
                resume_session: None,
                workspace: AgentWorkspaceEnvironment::default(),
            },
            runner,
        ));
        let thread = host.close(pane).expect("pane thread");
        let (finished, wait) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = finished.send(thread.join().is_ok());
        });
        assert_eq!(wait.recv_timeout(Duration::from_secs(1)), Ok(true));
    }

    #[test]
    fn close_reclaims_a_prompt_the_runtime_has_not_accepted() {
        let runner: PaneRunner = Box::new(|channels| {
            Box::pin(async move {
                channels
                    .events
                    .send(AgentStreamPayload::Ready {
                        agent_name: "fixture".to_owned(),
                        agent_key: "fixture".to_owned(),
                        auth_methods: Vec::new(),
                        capabilities: AgentSessionCapabilities::default(),
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                channels
                    .events
                    .send(AgentStreamPayload::SessionReady {
                        session_id: "fixture-session".to_owned(),
                        modes: None,
                        config_options: None,
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                std::future::pending::<Result<(), String>>().await
            })
        });
        let fixture = Fixture::open_with_runner(runner);
        fixture.wait_for_session();
        fixture.prompt("keep me");
        let deadline = Instant::now() + DEADLINE;
        while fixture.state().phase != AgentConnectionPhase::Running && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }

        let recorder = fixture.recorder.clone();
        let thread = fixture.host.close(fixture.pane).expect("the pane thread");
        thread.join().expect("the pane thread should settle");
        assert_eq!(queued_prompt_texts(&recorder.payloads()), ["keep me"]);
    }

    #[test]
    fn a_pane_rejects_commands_beyond_its_bounded_backlog() {
        let recorder = Recorder::default();
        let host = AgentHost::with_journal(AgentSpawnConfig::default(), recorder.sink(), None);
        let pane = PaneId(31);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let runner: PaneRunner = Box::new(move |_| {
            Box::pin(async move {
                let _ = entered_tx.send(());
                let _ = release_rx.recv();
                std::future::pending().await
            })
        });
        assert!(host.open_with(
            pane,
            1,
            AgentPaneSpec {
                provider: AgentProvider::Codex,
                cwd: PathBuf::from("/"),
                resume_session: None,
                workspace: AgentWorkspaceEnvironment::default(),
            },
            runner,
        ));
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("runner entered");
        for text in ["first", "second"] {
            assert!(
                host.command(
                    pane,
                    HostCommand::Prompt(AgentPrompt {
                        owner: ClientInstanceId::default(),
                        text: text.to_owned(),
                        images: Vec::new(),
                    })
                )
                .is_ok()
            );
        }
        for request_id in 0..PANE_INBOX_CAPACITY {
            assert!(
                host.command(
                    pane,
                    HostCommand::SetMode {
                        mode_id: request_id.to_string(),
                    }
                )
                .is_ok()
            );
        }
        assert!(
            host.command(
                pane,
                HostCommand::SetMode {
                    mode_id: PANE_INBOX_CAPACITY.to_string(),
                }
            )
            .is_err()
        );
        assert!(host.command(pane, HostCommand::Cancel).is_ok());
        assert!(
            host.command(
                pane,
                HostCommand::RespondPermission {
                    request_id: 9,
                    option_id: None,
                },
            )
            .is_ok()
        );

        let thread = host.close(pane).expect("pane thread");
        release_tx.send(()).expect("release runner");
        let (finished, wait) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = finished.send(thread.join().is_ok());
        });
        assert_eq!(wait.recv_timeout(Duration::from_secs(1)), Ok(true));
        assert_eq!(
            queued_prompt_texts(&recorder.payloads()),
            ["first", "second"]
        );
    }

    #[test]
    fn unqueueing_hands_the_queued_prompts_back_with_their_images() {
        let fixture = Fixture::open(Behavior::Hang, false);
        fixture.wait_for_session();
        fixture.prompt("running");
        fixture.command(HostCommand::Prompt(AgentPrompt {
            owner: ClientInstanceId::default(),
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
    fn queued_prompts_stay_inside_one_reclaim_frame() {
        let fixture = Fixture::open(Behavior::Hang, false);
        fixture.wait_for_session();
        fixture.prompt("running");
        fixture.command(HostCommand::Prompt(AgentPrompt {
            owner: ClientInstanceId::default(),
            text: "queued".to_owned(),
            images: vec![AgentImage {
                format: "image/png".to_owned(),
                data: vec![0; MAX_AGENT_PROMPT_BYTES - 1024],
            }],
        }));
        let deadline = Instant::now() + DEADLINE;
        while fixture.state().queued_prompts == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }

        fixture.command(HostCommand::Prompt(AgentPrompt {
            owner: ClientInstanceId::default(),
            text: "returned".to_owned(),
            images: vec![AgentImage {
                format: "image/png".to_owned(),
                data: vec![0; 2048],
            }],
        }));
        let payloads = fixture.recorder.wait("the overflow prompt", |payload| {
            matches!(payload, AgentStreamPayload::PromptsReclaimed { .. })
        });
        assert_eq!(queued_prompt_texts(&payloads), ["returned"]);
        assert_eq!(fixture.state().queued_prompts, 1);
        fixture.command(HostCommand::Unqueue);
        fixture.close();
    }

    #[test]
    fn a_session_the_agent_cannot_load_is_replayed_out_of_the_journal() {
        let directory = tempfile::tempdir().expect("journal directory");
        let journal = Arc::new(AgentJournal::open(directory.path()).expect("open journal"));
        for (index, text) in ["first restored", "second restored"]
            .into_iter()
            .enumerate()
        {
            let mut chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
            chunk.message_id = Some(MessageId::new(format!("restored-{index}")));
            let update = serde_json::to_value(SessionUpdate::AgentMessageChunk(chunk))
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
            matches!(payload, AgentStreamPayload::SessionReady { .. })
        });

        assert!(matches!(
            payloads[1],
            AgentStreamPayload::SessionReset { restoring: true }
        ));
        let AgentStreamPayload::SessionReady { session_id, .. } = payloads
            .iter()
            .find(|payload| matches!(payload, AgentStreamPayload::SessionReady { .. }))
            .expect("restored session")
            .clone()
        else {
            unreachable!()
        };
        assert_eq!(session_id, "fixture-session");
        assert_eq!(
            chunk_texts(&payloads),
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
    fn pane_outside_git_has_no_summary_on_session_ready() {
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

        assert_eq!(fixture.state().git, None);
        fixture.close();
    }

    #[test]
    fn git_summary_refreshes_on_session_ready_and_prompt_finished() {
        let Some((_scratch, root)) = seeded_git_repo() else {
            return;
        };
        let fixture = Fixture::build(
            Behavior::AskPermission,
            false,
            true,
            None,
            None,
            Some(root.clone()),
        );
        fixture.wait_for_session();
        let deadline = Instant::now() + DEADLINE;
        while fixture.state().git.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            fixture.state().git,
            Some(AgentGitSummary {
                branch: Some("main".to_owned()),
                changed_files: 0,
                additions: 0,
                deletions: 0,
            })
        );
        let states = fixture.recorder.states.lock();
        let ready = states
            .iter()
            .position(|(state, has_item)| {
                *has_item && state.phase == AgentConnectionPhase::Ready && state.git.is_none()
            })
            .expect("session readiness was published before Git capture");
        let summarized = states
            .iter()
            .position(|(state, has_item)| {
                !*has_item
                    && state
                        .git
                        .as_ref()
                        .is_some_and(|git| git.branch.as_deref() == Some("main"))
            })
            .expect("Git summary was published as a state-only update");
        assert!(ready < summarized);
        drop(states);

        fs::write(root.join("tracked.txt"), "one\nchanged\n").expect("tracked edit");
        fs::write(root.join("fresh.txt"), "new\nfile\n").expect("untracked file");
        let state_marker = fixture.recorder.states.lock().len();
        fixture.prompt("inspect the changes");
        let payloads = fixture.recorder.wait("the permission request", |payload| {
            matches!(payload, AgentStreamPayload::PermissionRequested { .. })
        });
        let request_id = payloads
            .iter()
            .find_map(|payload| match payload {
                AgentStreamPayload::PermissionRequested { request_id, .. } => Some(*request_id),
                _ => None,
            })
            .expect("permission request");
        fixture.command(HostCommand::RespondPermission {
            request_id,
            option_id: Some("allow".to_owned()),
        });
        fixture.recorder.wait("the turn to finish", |payload| {
            matches!(payload, AgentStreamPayload::PromptFinished { .. })
        });
        let deadline = Instant::now() + DEADLINE;
        while fixture
            .state()
            .git
            .as_ref()
            .is_none_or(|git| git.changed_files != 2)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(2));
        }

        assert_eq!(
            fixture.state().git,
            Some(AgentGitSummary {
                branch: Some("main".to_owned()),
                changed_files: 2,
                additions: 3,
                deletions: 1,
            })
        );
        let states = fixture.recorder.states.lock();
        let settled = states[state_marker..]
            .iter()
            .position(|(state, has_item)| {
                *has_item
                    && state.phase == AgentConnectionPhase::Ready
                    && state.git.as_ref().is_some_and(|git| git.changed_files == 0)
            })
            .expect("turn settlement was published with the previous summary");
        let refreshed = states[state_marker..]
            .iter()
            .position(|(state, has_item)| {
                !*has_item && state.git.as_ref().is_some_and(|git| git.changed_files == 2)
            })
            .expect("Git refresh was published as a state-only update");
        assert!(settled < refreshed);
        drop(states);
        fixture.close();
    }

    #[test]
    fn stale_git_summary_results_are_dropped_by_generation_token_and_cwd() {
        let Some((_scratch, root)) = seeded_git_repo() else {
            return;
        };
        let fixture = Fixture::build(Behavior::Hang, false, true, None, None, Some(root.clone()));
        fixture.wait_for_session();
        let deadline = Instant::now() + DEADLINE;
        while fixture.state().git.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        let marker = fixture.recorder.states.lock().len();
        let inbox = fixture
            .host
            .registry
            .panes
            .lock()
            .get(&fixture.pane)
            .expect("pane")
            .inbox
            .clone();
        for (generation, refresh, cwd, branch) in [
            (0, 2, root.clone(), "wrong-generation"),
            (1, 1, root.clone(), "wrong-token"),
            (1, 2, root.join("elsewhere"), "wrong-cwd"),
            (1, 2, root.clone(), "accepted"),
        ] {
            inbox
                .send_blocking(PaneInput::GitSummary {
                    generation,
                    refresh,
                    cwd,
                    summary: Some(AgentGitSummary {
                        branch: Some(branch.to_owned()),
                        changed_files: 0,
                        additions: 0,
                        deletions: 0,
                    }),
                })
                .expect("Git result reaches pane");
        }
        let deadline = Instant::now() + DEADLINE;
        while fixture
            .state()
            .git
            .as_ref()
            .and_then(|git| git.branch.as_deref())
            != Some("accepted")
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(2));
        }

        let states = fixture.recorder.states.lock();
        let branches = states[marker..]
            .iter()
            .filter_map(|(state, _)| state.git.as_ref()?.branch.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(branches, ["accepted"]);
        drop(states);
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
            owner: ClientInstanceId::default(),
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
        assert_eq!(queued_prompt_texts(&payloads), ["go", "queued"]);
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
}
