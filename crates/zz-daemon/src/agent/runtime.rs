//! One ACP connection, driven by commands and answering in stream payloads.
//!
//! This is the runtime half of the desktop's agent controller, moved into the
//! daemon: it owns the adapter child, the ACP session, the permission
//! responders, and the journal, and it never knows what renders the result.
//! One connection serves exactly one pane, so nothing here routes by pane —
//! the host stamps that on the way out.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::PathBuf,
    str::FromStr as _,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use agent_client_protocol::{
    AcpAgent, Agent, Client as AcpClientRole, ConnectTo, ConnectionTo, LineDirection, Responder,
    schema::{
        ProtocolVersion,
        v1::{
            AgentNotification, AuthMethod, AuthenticateRequest, CancelNotification,
            ClientCapabilities, ClientSessionCapabilities, CloseSessionRequest, ContentBlock,
            DeleteSessionRequest, ImageContent, Implementation, InitializeRequest,
            ListSessionsRequest, LoadSessionRequest, NewSessionRequest, PermissionOption,
            PermissionOptionKind, PromptRequest, RequestPermissionOutcome,
            RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
            SessionConfigOptionValue, SessionConfigOptionsCapabilities, SessionId as AcpSessionId,
            SessionUpdate, SetSessionConfigOptionRequest, SetSessionModeRequest, TextContent,
        },
    },
};
use async_channel::{Receiver, Sender};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use parking_lot::Mutex;
use serde_json::Value;
#[cfg(test)]
use zz_protocol::ClientInstanceId;
use zz_protocol::{
    AgentProvider, ClientId, MAX_AGENT_AUTH_METHODS, MAX_AGENT_PERMISSION_BYTES,
    MAX_AGENT_RESULT_BYTES, MAX_AGENT_SESSION_DIRECTORIES, MAX_AGENT_UPDATES_BYTES,
    MAX_GUI_TEXT_BYTES,
};

use crate::agent::{
    environment::{
        AgentWorkspaceEnvironment, with_platform_environment, with_workspace_environment,
    },
    journal::AgentJournal,
    profile::{client_meta_caps, is_sdk_message_method, parse_sdk_task_event, session_meta},
    stream::{
        AgentAuthMethod, AgentPrompt, AgentPromptOutcome, AgentSessionCapabilities,
        AgentSessionSummary, AgentStreamPayload,
    },
};

const MAX_SESSION_ID_BYTES: usize = 16 * 1024;
const MAX_SESSION_TITLE_BYTES: usize = 4 * 1024;
const MAX_SESSION_TIMESTAMP_BYTES: usize = 256;
const MAX_SESSION_CURSOR_BYTES: usize = 16 * 1024;
const MAX_RUNTIME_ERROR_BYTES: usize = 300;
const TRUNCATION_MARKER: &str = "… [truncated]";
const INITIALIZE_TIMEOUT: Duration = Duration::from_mins(1);
const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_STAGED_UPDATES: usize = 4096;
const MAX_STAGED_UPDATE_BYTES: usize = 2 * MAX_AGENT_UPDATES_BYTES;
const MAX_PENDING_PERMISSIONS: usize = 64;
const MAX_PENDING_PERMISSION_BYTES: usize = MAX_PENDING_PERMISSIONS * MAX_AGENT_PERMISSION_BYTES;
/// How long a turn may go silent before the watchdog parks it. Agents think
/// for minutes on end, so this is deliberately generous and parking never
/// touches the child; `ZZ_AGENT_QUIESCE_MS=0` disables the watchdog.
const DEFAULT_QUIESCE_MS: u64 = 120_000;
const JOURNAL_RETENTION_DAYS: u64 = 30;

/// What a pane's runtime is asked to do. Everything a client sends arrives
/// here, plus the follow-ups the host derives (queued prompts, shutdown).
#[derive(Debug)]
pub(crate) enum RuntimeCommand {
    Open {
        cwd: PathBuf,
        resume_session: Option<String>,
    },
    ListSessions {
        client: ClientId,
        cwd: Option<PathBuf>,
        cursor: Option<String>,
        replace: bool,
    },
    SwitchSession {
        session: AgentSessionSummary,
    },
    NewSession {
        cwd: PathBuf,
    },
    DeleteSession {
        client: ClientId,
        session_id: String,
    },
    Prompt {
        turn_id: u64,
        prompt: AgentPrompt,
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
    Shutdown,
}

#[derive(Debug)]
pub(crate) enum RuntimeControl {
    AbandonTurn {
        turn_id: u64,
    },
    Cancel {
        turn_id: u64,
        session_id: Option<String>,
    },
    RespondPermission {
        request_id: u64,
        option_id: Option<String>,
    },
}

struct PendingPermissionResponder {
    responder: Responder<RequestPermissionResponse>,
    bytes: usize,
}

struct PendingPromptCancellation {
    cancel: Sender<()>,
    done: Receiver<()>,
}

#[derive(Clone, Copy)]
enum DeferredPromptControl {
    Abandon,
    Cancel,
}

#[derive(Default)]
struct PromptControls {
    pending: HashMap<u64, PendingPromptCancellation>,
    deferred: HashMap<u64, DeferredPromptControl>,
    last_consumed: u64,
}

/// Which sessions this connection still speaks for. A session leaves the moment
/// it is superseded, so the late updates of a switched-away session are dropped
/// instead of landing in the pane that moved on.
#[derive(Default)]
struct RuntimeRouting {
    live_sessions: HashSet<String>,
    staged_updates: HashMap<String, Vec<SessionUpdate>>,
    staged_update_count: usize,
    staged_update_bytes: usize,
    staged_overflowed: bool,
    pending_new_session: bool,
    permissions: HashMap<u64, PendingPermissionResponder>,
    pending_permission_bytes: usize,
    /// Sessions whose updates are recorded. A session only enters once its
    /// transcript has settled in the pane, so the burst an agent replays out of
    /// `session/load` is never journalled on top of what it already replays.
    journaled: HashSet<String>,
}

impl RuntimeRouting {
    fn take_permission(&mut self, request_id: u64) -> Option<PendingPermissionResponder> {
        let pending = self.permissions.remove(&request_id)?;
        self.pending_permission_bytes = self.pending_permission_bytes.saturating_sub(pending.bytes);
        Some(pending)
    }

    fn take_permissions(&mut self) -> HashMap<u64, PendingPermissionResponder> {
        self.pending_permission_bytes = 0;
        std::mem::take(&mut self.permissions)
    }

    fn begin_staging(&mut self, session_id: &str) {
        self.clear_staging();
        self.staged_updates
            .insert(session_id.to_owned(), Vec::new());
    }

    fn begin_new_session(&mut self) {
        self.clear_staging();
        self.pending_new_session = true;
    }

    fn claim_new_session(&mut self, session_id: &str) {
        self.pending_new_session = false;
        self.staged_updates.retain(|id, _| id == session_id);
        self.staged_updates
            .entry(session_id.to_owned())
            .or_default();
    }

    fn clear_staging(&mut self) {
        self.staged_updates.clear();
        self.staged_update_count = 0;
        self.staged_update_bytes = 0;
        self.staged_overflowed = false;
        self.pending_new_session = false;
    }

    fn discard_staging(&mut self, _session_id: &str) {
        self.clear_staging();
    }

    fn stage(&mut self, session_id: &str, update: SessionUpdate) -> Result<(), SessionUpdate> {
        let should_stage = self.staged_updates.contains_key(session_id)
            || (self.pending_new_session && !self.live_sessions.contains(session_id));
        if !should_stage {
            return Err(update);
        }
        let bytes =
            serde_json::to_vec(&update).map_or(MAX_STAGED_UPDATE_BYTES + 1, |value| value.len());
        if self.staged_update_count >= MAX_STAGED_UPDATES
            || self.staged_update_bytes.saturating_add(bytes) > MAX_STAGED_UPDATE_BYTES
        {
            self.staged_overflowed = true;
            return Ok(());
        }
        self.staged_updates
            .entry(session_id.to_owned())
            .or_default()
            .push(update);
        self.staged_update_count += 1;
        self.staged_update_bytes += bytes;
        Ok(())
    }

    fn drain_staging(&mut self, session_id: &str) -> Vec<SessionUpdate> {
        self.staged_updates
            .get_mut(session_id)
            .map(std::mem::take)
            .unwrap_or_default()
    }
}

/// Everything a pane's adapter child needs to exist: what to run, whether it
/// may approve itself, and the workspace identity it is told about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentSpawnConfig {
    pub(crate) command: String,
    pub(crate) claude_code_command: String,
    pub(crate) auto_approve: bool,
    pub(crate) workspace: AgentWorkspaceEnvironment,
}

impl AgentSpawnConfig {
    pub(crate) fn command_for(&self, provider: AgentProvider) -> &str {
        match provider {
            AgentProvider::Codex => &self.command,
            AgentProvider::ClaudeCode => &self.claude_code_command,
        }
    }

    pub(crate) const fn key_for(provider: AgentProvider) -> &'static str {
        match provider {
            AgentProvider::Codex => "agent-command",
            AgentProvider::ClaudeCode => "agent-claude-code-command",
        }
    }

    pub(crate) fn commands(&self) -> [String; 2] {
        [self.command.clone(), self.claude_code_command.clone()]
    }
}

pub(crate) async fn run_agent_runtime(
    config: AgentSpawnConfig,
    provider: AgentProvider,
    permission_ids: Arc<AtomicU64>,
    journal: Option<Arc<AgentJournal>>,
    command_rx: Receiver<RuntimeCommand>,
    control_rx: Receiver<RuntimeControl>,
    event_tx: Sender<AgentStreamPayload>,
) -> Result<(), String> {
    let agent = AcpAgent::from_str(config.command_for(provider))
        .map_err(|error| format!("invalid {}: {error}", AgentSpawnConfig::key_for(provider)))?;
    let agent = with_platform_environment(agent);
    let stderr = StderrTail::default();
    let debug_stderr = stderr.clone();
    let agent =
        with_workspace_environment(agent, &config.workspace).with_debug(move |line, direction| {
            if matches!(direction, LineDirection::Stderr) {
                log::warn!(target: "zz::agent::stderr", "{line}");
                debug_stderr.push(line);
            }
        });

    run_agent_connection(
        provider,
        config.auto_approve,
        agent,
        permission_ids,
        journal,
        command_rx,
        control_rx,
        event_tx,
    )
    .await
    .map_err(|error| runtime_failure_message(provider.label(), &error, stderr.snapshot()))
}

/// Rolling tail of the adapter's stderr, kept so an unexpected exit can say
/// what the child complained about instead of shrugging.
#[derive(Clone, Default)]
struct StderrTail(Arc<Mutex<std::collections::VecDeque<String>>>);

impl StderrTail {
    const KEEP_LINES: usize = 6;
    const KEEP_BYTES: usize = 700;

    fn push(&self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let mut tail = self.0.lock();
        tail.push_back(capped(line.to_owned(), Self::KEEP_BYTES));
        while tail.len() > Self::KEEP_LINES {
            tail.pop_front();
        }
    }

    fn snapshot(&self) -> Option<String> {
        let tail = self.0.lock();
        (!tail.is_empty()).then(|| {
            capped(
                tail.iter().cloned().collect::<Vec<_>>().join("\n"),
                Self::KEEP_BYTES,
            )
        })
    }
}

fn runtime_failure_message(adapter: &str, error: &str, tail: Option<String>) -> String {
    let status = exit_status_detail(error).unwrap_or_else(|| {
        capped(
            error.lines().next().unwrap_or(error).trim().to_owned(),
            MAX_RUNTIME_ERROR_BYTES,
        )
    });
    match tail {
        Some(tail) => format!("{adapter} exited unexpectedly ({status}): {tail}"),
        None => format!("{adapter} exited unexpectedly ({status})"),
    }
}

/// Recover the child's exit status from the ACP crate's process error, which
/// reads `Process exited with <status>[: <stderr>]`. The crate owns the child,
/// so this string is the only handle on the status we get.
fn exit_status_detail(error: &str) -> Option<String> {
    let status = error.split_once("exited with ")?.1;
    let status = status
        .split(": ")
        .take(2)
        .collect::<Vec<_>>()
        .join(": ")
        .trim()
        .to_owned();
    (!status.is_empty()).then_some(status)
}

fn new_session_request(provider: AgentProvider, cwd: PathBuf) -> NewSessionRequest {
    let mut request = NewSessionRequest::new(cwd);
    request.meta = session_meta(provider);
    request
}

fn load_session_request(
    provider: AgentProvider,
    session_id: AcpSessionId,
    cwd: PathBuf,
) -> LoadSessionRequest {
    let mut request = LoadSessionRequest::new(session_id, cwd);
    request.meta = session_meta(provider);
    request
}

/// The daemon's journal, opened once and pruned on the way in.
pub(crate) fn load_persistent_journal() -> Option<Arc<AgentJournal>> {
    let journal = crate::agent::paths::journal_directory().and_then(|directory| {
        AgentJournal::open(&directory).map_err(|error| std::io::Error::other(error.to_string()))
    });
    let journal = match journal {
        Ok(journal) => journal,
        Err(error) => {
            log::warn!(
                target: "zz::agent::journal",
                "agent transcripts are not journalled: {error}"
            );
            return None;
        }
    };
    match journal.prune(JOURNAL_RETENTION_DAYS) {
        Ok(0) => {}
        Ok(removed) => log::info!(
            target: "zz::agent::journal",
            "pruned {removed} agent journals older than {JOURNAL_RETENTION_DAYS} days"
        ),
        Err(error) => log::warn!(
            target: "zz::agent::journal",
            "could not prune agent journals: {error}"
        ),
    }
    Some(Arc::new(journal))
}

/// A journal failure never interrupts a turn: the transcript keeps streaming,
/// it just stops being replayable. The first one is loud, the rest are not.
fn report_journal_error(session_id: &str, error: &str) {
    static REPORTED: AtomicBool = AtomicBool::new(false);

    if REPORTED.swap(true, Ordering::Relaxed) {
        log::debug!(target: "zz::agent::journal", "session {session_id}: {error}");
    } else {
        log::warn!(
            target: "zz::agent::journal",
            "agent transcripts are no longer journalled for session {session_id}: {error}"
        );
    }
}

fn record_update(
    journal: Option<&AgentJournal>,
    provider: AgentProvider,
    session_id: &str,
    update: &SessionUpdate,
) {
    let Some(journal) = journal else {
        return;
    };
    let value = match serde_json::to_value(update) {
        Ok(value) => value,
        Err(error) => {
            report_journal_error(session_id, &error.to_string());
            return;
        }
    };
    if let Err(error) = journal.append_for(provider, session_id, &value) {
        report_journal_error(session_id, &error.to_string());
    }
}

/// The journalled transcript of `session_id`, as updates the reducer replays
/// exactly like the ones an agent sends out of `session/load`.
fn journal_replay(
    journal: Option<&AgentJournal>,
    provider: AgentProvider,
    session_id: Option<&str>,
) -> Vec<SessionUpdate> {
    let (Some(journal), Some(session_id)) = (journal, session_id) else {
        return Vec::new();
    };
    let records = match journal.replay_for(provider, session_id) {
        Ok(records) => records,
        Err(error) => {
            report_journal_error(session_id, &error.to_string());
            return Vec::new();
        }
    };
    records
        .into_iter()
        .filter_map(
            |(seq, update)| match serde_json::from_value::<SessionUpdate>(update) {
                Ok(update) => match update_payload(&update) {
                    Ok(_) => Some(update),
                    Err(error) => {
                        log::warn!(
                            target: "zz::agent::journal",
                            "skipping oversized journalled update {seq} of session {session_id}: {error}"
                        );
                        None
                    }
                },
                Err(error) => {
                    log::warn!(
                        target: "zz::agent::journal",
                        "skipping journalled update {seq} of session {session_id}: {error}"
                    );
                    None
                }
            },
        )
        .collect()
}

async fn complete_staged_session(
    routing: &Arc<Mutex<RuntimeRouting>>,
    event_tx: &Sender<AgentStreamPayload>,
    journal: Option<&AgentJournal>,
    provider: AgentProvider,
    session_id: &str,
    previous_session: Option<&str>,
    replay: Vec<SessionUpdate>,
    final_payload: AgentStreamPayload,
    record_replay: bool,
    remove_previous_journal: bool,
) -> Result<(), agent_client_protocol::Error> {
    validate_payload(&final_payload)?;
    for update in replay {
        let payload = update_payload(&update)?;
        if record_replay {
            record_update(journal, provider, session_id, &update);
        }
        send_payload(event_tx, payload).await?;
    }

    let mut final_payload = Some(final_payload);
    loop {
        let staged = {
            let mut routes = routing.lock();
            if routes.staged_overflowed {
                routes.clear_staging();
                return Err(agent_client_protocol::Error::internal_error().data(format!(
                    "agent session replay exceeded {MAX_STAGED_UPDATES} updates or {MAX_STAGED_UPDATE_BYTES} bytes"
                )));
            }
            routes.drain_staging(session_id)
        };
        for update in staged {
            let payload = update_payload(&update)?;
            if record_replay {
                record_update(journal, provider, session_id, &update);
            }
            send_payload(event_tx, payload).await?;
        }

        let retry_payload = {
            let mut routes = routing.lock();
            if routes.staged_overflowed {
                routes.clear_staging();
                return Err(agent_client_protocol::Error::internal_error().data(format!(
                    "agent session replay exceeded {MAX_STAGED_UPDATES} updates or {MAX_STAGED_UPDATE_BYTES} bytes"
                )));
            }
            if routes
                .staged_updates
                .get(session_id)
                .is_some_and(|updates| !updates.is_empty())
            {
                continue;
            }
            let payload = final_payload.take().expect("session boundary payload");
            match event_tx.try_send(payload) {
                Ok(()) => {
                    routes.clear_staging();
                    routes.live_sessions.insert(session_id.to_owned());
                    routes.journaled.insert(session_id.to_owned());
                    if let Some(previous) =
                        previous_session.filter(|previous| *previous != session_id)
                    {
                        routes.live_sessions.remove(previous);
                        routes.journaled.remove(previous);
                    }
                    None
                }
                Err(async_channel::TrySendError::Full(payload)) => Some(payload),
                Err(async_channel::TrySendError::Closed(_)) => {
                    return Err(agent_client_protocol::Error::internal_error()
                        .data("agent stream is unavailable"));
                }
            }
        };
        let Some(payload) = retry_payload else {
            break;
        };
        final_payload = Some(payload);
        smol::Timer::after(Duration::from_millis(1)).await;
    }

    if remove_previous_journal
        && let (Some(journal), Some(previous)) = (journal, previous_session)
        && previous != session_id
        && let Err(error) = journal.remove_for(provider, previous)
    {
        report_journal_error(previous, &error.to_string());
    }
    Ok(())
}

pub(crate) async fn run_agent_connection(
    provider: AgentProvider,
    auto_approve: bool,
    agent: impl ConnectTo<AcpClientRole>,
    permission_ids: Arc<AtomicU64>,
    journal: Option<Arc<AgentJournal>>,
    command_rx: Receiver<RuntimeCommand>,
    control_rx: Receiver<RuntimeControl>,
    event_tx: Sender<AgentStreamPayload>,
) -> Result<(), String> {
    let routing = Arc::new(Mutex::new(RuntimeRouting::default()));
    let prompt_controls = Arc::new(Mutex::new(PromptControls::default()));

    let notification_journal = journal.clone();
    let notification_routing = Arc::clone(&routing);
    let notification_events = event_tx.clone();
    let ext_routing = Arc::clone(&routing);
    let ext_events = event_tx.clone();
    let permission_routing = Arc::clone(&routing);
    let permission_events = event_tx.clone();

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |notification: AgentNotification, _| {
                match notification {
                    AgentNotification::SessionNotification(notification) => {
                        let session_id = notification.session_id.0.to_string();
                        let (update, live, journaled) = {
                            let mut routing = notification_routing.lock();
                            let update = match routing.stage(&session_id, notification.update) {
                                Ok(()) => return Ok(()),
                                Err(update) => update,
                            };
                            (
                                update,
                                routing.live_sessions.contains(&session_id),
                                routing.journaled.contains(&session_id),
                            )
                        };
                        if live {
                            let payload = update_payload(&update)?;
                            if journaled {
                                record_update(
                                    notification_journal.as_deref(),
                                    provider,
                                    &session_id,
                                    &update,
                                );
                            }
                            send_payload(&notification_events, payload).await?;
                        }
                        Ok(())
                    }
                    AgentNotification::ExtNotification(notification) => {
                        if !is_sdk_message_method(notification.method.as_ref()) {
                            return Ok(());
                        }
                        let Ok(params) =
                            serde_json::from_str::<Value>(notification.params.get())
                        else {
                            return Ok(());
                        };
                        let Some((session_id, event)) = parse_sdk_task_event(&params) else {
                            return Ok(());
                        };
                        let live = {
                            let routing = ext_routing.lock();
                            if routing.staged_updates.contains_key(&session_id) {
                                return Ok(());
                            }
                            routing.live_sessions.contains(&session_id)
                        };
                        if live {
                            send_payload(&ext_events, AgentStreamPayload::TaskEvent { event })
                                .await?;
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _| {
                let session_id = request.session_id.0.to_string();
                let live = {
                    let routing = permission_routing.lock();
                    routing.live_sessions.contains(&session_id)
                        && !routing.staged_updates.contains_key(&session_id)
                };
                if !live {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }
                if auto_approve
                    && !is_user_question(&request.options)
                    && let Some(option_id) = preferred_allow_option(&request.options)
                {
                    log::debug!(
                        target: "zz::agent",
                        "auto-approving tool permission with option {option_id}"
                    );
                    let update = encode_update(&SessionUpdate::ToolCallUpdate(request.tool_call))?;
                    if let Err(error) = send_payload(
                        &permission_events,
                        AgentStreamPayload::Update { update },
                    )
                    .await
                    {
                        responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ))?;
                        return Err(error);
                    }
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                            option_id,
                        )),
                    ));
                }
                let request_id = permission_ids.fetch_add(1, Ordering::Relaxed);
                let Ok((requested, bytes)) = json_of(&request.tool_call)
                    .and_then(|tool_call| Ok((tool_call, json_of(&request.options)?)))
                    .and_then(|(tool_call, options)| {
                        let payload = AgentStreamPayload::PermissionRequested {
                            request_id,
                            tool_call,
                            options,
                        };
                        let bytes = validate_payload(&payload)?;
                        Ok((payload, bytes))
                    })
                else {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                };
                {
                    let mut routing = permission_routing.lock();
                    if !routing.live_sessions.contains(&session_id)
                        || routing.staged_updates.contains_key(&session_id)
                        || routing.permissions.len() >= MAX_PENDING_PERMISSIONS
                        || routing.pending_permission_bytes.saturating_add(bytes)
                            > MAX_PENDING_PERMISSION_BYTES
                    {
                        drop(routing);
                        return responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ));
                    }
                    routing.pending_permission_bytes += bytes;
                    routing.permissions.insert(
                        request_id,
                        PendingPermissionResponder { responder, bytes },
                    );
                }
                let requested = send_payload(&permission_events, requested).await;
                if let Err(error) = requested {
                    if let Some(pending) = permission_routing.lock().take_permission(request_id)
                    {
                        let _ = pending.responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ));
                    }
                    return Err(error);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            let initialize = InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(
                    ClientCapabilities::new()
                        .session(
                            ClientSessionCapabilities::new()
                                .config_options(SessionConfigOptionsCapabilities::new()),
                        )
                        .meta(client_meta_caps(provider)),
                )
                .client_info(Implementation::new("zz", env!("CARGO_PKG_VERSION")).title("zz"));
            let response = await_rpc(
                connection.send_request(initialize).block_task(),
                INITIALIZE_TIMEOUT,
                "ACP initialize",
            )
            .await?;
            if response.protocol_version != ProtocolVersion::V1 {
                return Err(agent_client_protocol::Error::internal_error().data(format!(
                    "agent selected unsupported protocol version {:?}",
                    response.protocol_version
                )));
            }
            let capabilities = AgentSessionCapabilities {
                load: response.agent_capabilities.load_session,
                list: response.agent_capabilities.session_capabilities.list.is_some(),
                close: response
                    .agent_capabilities
                    .session_capabilities
                    .close
                    .is_some(),
                delete: response
                    .agent_capabilities
                    .session_capabilities
                    .delete
                    .is_some(),
                additional_directories: response
                    .agent_capabilities
                    .session_capabilities
                    .additional_directories
                    .is_some(),
                images: response.agent_capabilities.prompt_capabilities.image,
            };
            let (agent_name, agent_key) = response.agent_info.map_or_else(
                || ("ACP agent".to_owned(), "acp-agent".to_owned()),
                |info| {
                    let agent_key = info.name.clone();
                    (info.title.unwrap_or(info.name), agent_key)
                },
            );
            let auth_methods = response
                .auth_methods
                .iter()
                .take(MAX_AGENT_AUTH_METHODS)
                .map(auth_method_model)
                .collect::<Vec<_>>();
            send_payload(
                &event_tx,
                AgentStreamPayload::Ready {
                    agent_name,
                    agent_key,
                    auth_methods,
                    capabilities,
                },
            )
            .await?;

            let control_connection = connection.clone();
            let control_routing = Arc::clone(&routing);
            let control_prompts = Arc::clone(&prompt_controls);
            let control_events = event_tx.clone();
            connection.spawn(async move {
                while let Ok(control) = control_rx.recv().await {
                    match control {
                        RuntimeControl::AbandonTurn { turn_id } => {
                            let (pending, deferred) = {
                                let mut prompts = control_prompts.lock();
                                let pending = prompts.pending.remove(&turn_id);
                                let deferred = pending.is_none() && turn_id > prompts.last_consumed;
                                if deferred {
                                    prompts
                                        .deferred
                                        .insert(turn_id, DeferredPromptControl::Abandon);
                                }
                                (pending, deferred)
                            };
                            if let Some(pending) = pending {
                                pending.cancel.close();
                                let _ = pending.done.recv().await;
                            }
                            if deferred {
                                continue;
                            }
                            send_payload(
                                &control_events,
                                AgentStreamPayload::TurnAbandoned { turn_id },
                            )
                            .await?;
                        }
                        RuntimeControl::Cancel {
                            turn_id,
                            session_id,
                        } => {
                            let pending = {
                                let mut prompts = control_prompts.lock();
                                let pending = prompts.pending.remove(&turn_id);
                                if pending.is_none() && turn_id > prompts.last_consumed {
                                    prompts
                                        .deferred
                                        .insert(turn_id, DeferredPromptControl::Cancel);
                                }
                                pending
                            };
                            if let Some(pending) = pending {
                                pending.cancel.close();
                                let _ = pending.done.recv().await;
                            }
                            if let Some(session_id) = session_id {
                                control_connection.send_notification(CancelNotification::new(
                                    AcpSessionId::new(session_id),
                                ))?;
                            }
                            cancel_pending_permissions(&control_routing, &control_events).await?;
                            send_payload(
                                &control_events,
                                AgentStreamPayload::PromptFinished {
                                    turn_id,
                                    outcome: AgentPromptOutcome::Finished {
                                        stop_reason: Value::String("cancelled".to_owned()),
                                    },
                                },
                            )
                            .await?;
                        }
                        RuntimeControl::RespondPermission {
                            request_id,
                            option_id,
                        } => {
                            let pending = control_routing.lock().take_permission(request_id);
                            if let Some(pending) = pending {
                                let canceled = option_id.is_none();
                                let outcome = option_id.map_or(
                                    RequestPermissionOutcome::Cancelled,
                                    |option_id| {
                                        RequestPermissionOutcome::Selected(
                                            SelectedPermissionOutcome::new(option_id),
                                        )
                                    },
                                );
                                pending
                                    .responder
                                    .respond(RequestPermissionResponse::new(outcome))?;
                                send_payload(
                                    &control_events,
                                    AgentStreamPayload::PermissionResolved {
                                        request_id,
                                        canceled,
                                    },
                                )
                                .await?;
                            }
                        }
                    }
                }
                Ok(())
            })?;

            let mut session: Option<AcpSessionId> = None;
            while let Ok(command) = command_rx.recv().await {
                match command {
                    RuntimeCommand::Open {
                        cwd,
                        resume_session,
                    } => {
                        let mut restored = if capabilities.load {
                            Vec::new()
                        } else {
                            journal_replay(journal.as_deref(), provider, resume_session.as_deref())
                        };
                        let restoring =
                            (capabilities.load && resume_session.is_some()) || !restored.is_empty();
                        send_payload(&event_tx, AgentStreamPayload::SessionReset { restoring })
                            .await?;
                        let session_result: Result<_, agent_client_protocol::Error> = if let Some(resume) =
                            resume_session.clone().filter(|_| capabilities.load)
                        {
                            let session_id = AcpSessionId::new(resume);
                            routing.lock().begin_staging(session_id.0.as_ref());
                            match await_rpc(
                                connection.send_request(load_session_request(
                                    provider,
                                    session_id.clone(),
                                    cwd.clone(),
                                ))
                                .block_task(),
                                RPC_TIMEOUT,
                                "ACP session load",
                            )
                            .await
                            {
                                Ok(response) => Ok((
                                    session_id,
                                    response.modes,
                                    response.config_options,
                                    false,
                                    false,
                                )),
                                Err(error) => {
                                    routing.lock().discard_staging(session_id.0.as_ref());
                                    log::warn!(
                                        target: "zz::agent",
                                        "could not restore the ACP session: {error}; creating a new session"
                                    );
                                    restored = journal_replay(
                                        journal.as_deref(),
                                        provider,
                                        resume_session.as_deref(),
                                    );
                                    routing.lock().begin_new_session();
                                    let response = await_rpc(
                                        connection
                                            .send_request(new_session_request(
                                                provider,
                                                cwd.clone(),
                                            ))
                                            .block_task(),
                                        RPC_TIMEOUT,
                                        "ACP session creation",
                                    )
                                    .await?;
                                    routing
                                        .lock()
                                        .claim_new_session(response.session_id.0.as_ref());
                                    Ok((
                                        response.session_id,
                                        response.modes,
                                        response.config_options,
                                        true,
                                        true,
                                    ))
                                }
                            }
                        } else {
                            routing.lock().begin_new_session();
                            let response = await_rpc(
                                connection
                                    .send_request(new_session_request(provider, cwd.clone()))
                                    .block_task(),
                                RPC_TIMEOUT,
                                "ACP session creation",
                            )
                            .await?;
                            routing
                                .lock()
                                .claim_new_session(response.session_id.0.as_ref());
                            Ok((
                                response.session_id,
                                response.modes,
                                response.config_options,
                                true,
                                resume_session.is_some() && !restored.is_empty(),
                            ))
                        };
                        match session_result {
                            Ok((
                                session_id,
                                modes,
                                config_options,
                                record_replay,
                                remove_previous_journal,
                            )) => {
                                if !valid_session_id(session_id.0.as_ref()) {
                                    routing.lock().clear_staging();
                                    send_payload(
                                        &event_tx,
                                        AgentStreamPayload::PaneFailed {
                                            message: "agent returned an invalid session ID"
                                                .to_owned(),
                                        },
                                    )
                                    .await?;
                                    continue;
                                }
                                let live = session_id.0.to_string();
                                complete_staged_session(
                                    &routing,
                                    &event_tx,
                                    journal.as_deref(),
                                    provider,
                                    &live,
                                    resume_session.as_deref(),
                                    std::mem::take(&mut restored),
                                    AgentStreamPayload::SessionReady {
                                        session_id: live.clone(),
                                        modes: optional_json(modes.as_ref())?,
                                        config_options: optional_json(config_options.as_ref())?,
                                    },
                                    record_replay,
                                    remove_previous_journal,
                                )
                                .await?;
                                session = Some(session_id);
                            }
                            Err(error) => {
                                routing.lock().clear_staging();
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::PaneFailed {
                                        message: error.to_string(),
                                    },
                                )
                                .await?;
                            }
                        }
                    }
                    RuntimeCommand::ListSessions {
                        client,
                        cwd,
                        cursor,
                        replace,
                    } => {
                        if !capabilities.list {
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionListFailed {
                                        client,
                                        message: "agent does not support session/list".to_owned(),
                                },
                            )
                            .await?;
                            continue;
                        }
                        let request = ListSessionsRequest::new().cwd(cwd.clone()).cursor(cursor);
                        match await_rpc(
                            connection.send_request(request).block_task(),
                            RPC_TIMEOUT,
                            "ACP session list",
                        )
                        .await
                        {
                            Ok(response) => {
                                let sessions = response
                                    .sessions
                                    .into_iter()
                                    .filter_map(|session| {
                                        let summary = AgentSessionSummary {
                                            session_id: session.session_id.0.to_string(),
                                            cwd: session.cwd,
                                            additional_directories: session.additional_directories,
                                            title: session.title.and_then(|title| {
                                                clean_session_metadata(
                                                    &title,
                                                    MAX_SESSION_TITLE_BYTES,
                                                )
                                            }),
                                            updated_at: session.updated_at.and_then(|timestamp| {
                                                clean_session_metadata(
                                                    &timestamp,
                                                    MAX_SESSION_TIMESTAMP_BYTES,
                                                )
                                            }),
                                        };
                                        valid_session_summary(&summary).then_some(summary)
                                    })
                                    .collect();
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionsListed {
                                        client,
                                        sessions,
                                        next_cursor: response
                                            .next_cursor
                                            .filter(|cursor| valid_session_cursor(cursor)),
                                        cwd_filter: cwd,
                                        replace,
                                    },
                                )
                                .await?;
                            }
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionListFailed {
                                    client,
                                    message: format!("could not list agent sessions: {error}"),
                                },
                            )
                            .await?,
                        }
                    }
                    RuntimeCommand::SwitchSession { session: target } => {
                        if !capabilities.load {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionSwitchFailed {
                                    message: "agent does not support session/load".to_owned(),
                                },
                            )
                            .await?;
                            continue;
                        }
                        let session_id = AcpSessionId::new(target.session_id.clone());
                        routing.lock().begin_staging(&target.session_id);
                        let mut request =
                            load_session_request(provider, session_id.clone(), target.cwd.clone());
                        if capabilities.additional_directories {
                            request = request
                                .additional_directories(target.additional_directories.clone());
                        }
                        match await_rpc(
                            connection.send_request(request).block_task(),
                            RPC_TIMEOUT,
                            "ACP session load",
                        )
                        .await
                        {
                            Ok(response) => {
                                let previous = session.clone();
                                let previous = previous.filter(|previous| previous != &session_id);
                                if let Some(previous) = &previous {
                                    routing
                                        .lock()
                                        .live_sessions
                                        .remove(previous.0.as_ref());
                                }
                                cancel_pending_permissions(&routing, &event_tx).await?;
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionReset { restoring: true },
                                )
                                .await?;
                                complete_staged_session(
                                    &routing,
                                    &event_tx,
                                    journal.as_deref(),
                                    provider,
                                    &target.session_id,
                                    previous.as_ref().map(|previous| previous.0.as_ref()),
                                    Vec::new(),
                                    AgentStreamPayload::SessionSwitched {
                                        session_id: target.session_id.clone(),
                                        cwd: target.cwd.clone(),
                                        modes: optional_json(response.modes.as_ref())?,
                                        config_options: optional_json(
                                            response.config_options.as_ref(),
                                        )?,
                                        replay: Vec::new(),
                                    },
                                    false,
                                    false,
                                )
                                .await?;
                                session = Some(session_id);
                                if capabilities.close
                                    && let Some(previous) = previous
                                {
                                    spawn_close_session(
                                        &connection,
                                        previous,
                                        "previous ACP session after switch",
                                    )?;
                                }
                            }
                            Err(error) => {
                                routing.lock().discard_staging(&target.session_id);
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionSwitchFailed {
                                        message: format!("could not load selected session: {error}"),
                                    },
                                )
                                .await?;
                                if capabilities.close {
                                    spawn_close_session(
                                        &connection,
                                        session_id,
                                        "failed ACP session target",
                                    )?;
                                }
                            }
                        }
                    }
                    RuntimeCommand::NewSession { cwd } => {
                        routing.lock().begin_new_session();
                        match await_rpc(
                            connection
                                .send_request(new_session_request(provider, cwd.clone()))
                                .block_task(),
                            RPC_TIMEOUT,
                            "ACP session creation",
                        )
                        .await
                        {
                            Ok(response) if valid_session_id(response.session_id.0.as_ref()) => {
                                let session_id = response.session_id;
                                routing
                                    .lock()
                                    .claim_new_session(session_id.0.as_ref());
                                let previous = session.clone();
                                let previous = previous.filter(|previous| previous != &session_id);
                                if let Some(previous) = &previous {
                                    routing
                                        .lock()
                                        .live_sessions
                                        .remove(previous.0.as_ref());
                                }
                                cancel_pending_permissions(&routing, &event_tx).await?;
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionReset { restoring: true },
                                )
                                .await?;
                                complete_staged_session(
                                    &routing,
                                    &event_tx,
                                    journal.as_deref(),
                                    provider,
                                    session_id.0.as_ref(),
                                    previous.as_ref().map(|previous| previous.0.as_ref()),
                                    Vec::new(),
                                    AgentStreamPayload::SessionSwitched {
                                        session_id: session_id.0.to_string(),
                                        cwd: cwd.clone(),
                                        modes: optional_json(response.modes.as_ref())?,
                                        config_options: optional_json(
                                            response.config_options.as_ref(),
                                        )?,
                                        replay: Vec::new(),
                                    },
                                    true,
                                    false,
                                )
                                .await?;
                                session = Some(session_id);
                                if capabilities.close
                                    && let Some(previous) = previous
                                {
                                    spawn_close_session(
                                        &connection,
                                        previous,
                                        "previous ACP session after creating a new one",
                                    )?;
                                }
                            }
                            Ok(_) => {
                                routing.lock().clear_staging();
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionSwitchFailed {
                                        message: "agent returned an invalid session ID".to_owned(),
                                    },
                                )
                                .await?;
                            }
                            Err(error) => {
                                routing.lock().clear_staging();
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionSwitchFailed {
                                        message: format!("could not create a new session: {error}"),
                                    },
                                )
                                .await?;
                            }
                        }
                    }
                    RuntimeCommand::DeleteSession { client, session_id } => {
                        if !capabilities.delete {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionDeleteFailed {
                                    client,
                                    message: "agent does not support session/delete".to_owned(),
                                },
                            )
                            .await?;
                            continue;
                        }
                        match await_rpc(
                            connection
                                .send_request(DeleteSessionRequest::new(session_id.clone()))
                                .block_task(),
                            RPC_TIMEOUT,
                            "ACP session deletion",
                        )
                        .await
                        {
                            Ok(_) => {
                                routing.lock().journaled.remove(&session_id);
                                if let Some(journal) = journal.as_deref()
                                    && let Err(error) = journal.remove_for(provider, &session_id)
                                {
                                    report_journal_error(&session_id, &error.to_string());
                                }
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionDeleted { client, session_id },
                                )
                                .await?;
                            }
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionDeleteFailed {
                                    client,
                                    message: format!("could not delete session: {error}"),
                                },
                            )
                            .await?,
                        }
                    }
                    RuntimeCommand::Prompt { turn_id, prompt } => {
                        let Some(session_id) = session.clone() else {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::PaneFailed {
                                    message: "agent session is not ready".to_owned(),
                                },
                            )
                            .await?;
                            continue;
                        };
                        let prompt_events = event_tx.clone();
                        let request = connection.send_request(PromptRequest::new(
                            session_id,
                            prompt_blocks(prompt),
                        ));
                        let (cancel_tx, cancel_rx) = async_channel::bounded::<()>(1);
                        let (done_tx, done_rx) = async_channel::bounded::<()>(1);
                        let deferred = {
                            let mut prompts = prompt_controls.lock();
                            prompts.last_consumed = prompts.last_consumed.max(turn_id);
                            if let Some(deferred) = prompts.deferred.remove(&turn_id) {
                                Some(deferred)
                            } else {
                                prompts.pending.insert(
                                    turn_id,
                                    PendingPromptCancellation {
                                        cancel: cancel_tx,
                                        done: done_rx,
                                    },
                                );
                                None
                            }
                        };
                        if let Some(deferred) = deferred {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::PromptAccepted { turn_id },
                            )
                            .await?;
                            match deferred {
                                DeferredPromptControl::Cancel => {
                                    send_payload(
                                        &event_tx,
                                        AgentStreamPayload::PromptFinished {
                                            turn_id,
                                            outcome: AgentPromptOutcome::Finished {
                                                stop_reason: Value::String("cancelled".to_owned()),
                                            },
                                        },
                                    )
                                    .await?;
                                }
                                DeferredPromptControl::Abandon => {
                                    send_payload(
                                        &event_tx,
                                        AgentStreamPayload::TurnAbandoned { turn_id },
                                    )
                                    .await?;
                                }
                            }
                            continue;
                        }
                        let task_prompts = Arc::clone(&prompt_controls);
                        let spawned = connection.spawn(async move {
                            let completed = futures_lite::future::race(
                                async { Some(request.block_task().await) },
                                async {
                                    let _ = cancel_rx.recv().await;
                                    None
                                },
                            )
                            .await;
                            task_prompts.lock().pending.remove(&turn_id);
                            let _ = done_tx.try_send(());
                            let Some(completed) = completed else {
                                return Ok(());
                            };
                            let outcome = match completed {
                                Ok(response) => AgentPromptOutcome::Finished {
                                    stop_reason: serde_json::to_value(response.stop_reason)
                                        .unwrap_or(Value::Null),
                                },
                                Err(error) => AgentPromptOutcome::Failed {
                                    message: error.to_string(),
                                },
                            };
                            let _ = prompt_events
                                .send(AgentStreamPayload::PromptFinished { turn_id, outcome })
                                .await;
                            Ok(())
                        });
                        if let Err(error) = spawned {
                            prompt_controls.lock().pending.remove(&turn_id);
                            return Err(error);
                        }
                        send_payload(
                            &event_tx,
                            AgentStreamPayload::PromptAccepted { turn_id },
                        )
                        .await?;
                    }
                    RuntimeCommand::Authenticate { method_id } => {
                        match await_rpc(
                            connection
                                .send_request(AuthenticateRequest::new(method_id))
                                .block_task(),
                            RPC_TIMEOUT,
                            "ACP authentication",
                        )
                        .await
                        {
                            Ok(_) => {
                                send_payload(&event_tx, AgentStreamPayload::Authenticated).await?;
                            }
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::AuthenticationFailed {
                                    message: format!("authentication failed: {error}"),
                                },
                            )
                            .await?,
                        }
                    }
                    RuntimeCommand::SetConfigOption { option_id, value } => {
                        let Some(session_id) = session.clone() else {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::SettingFailed {
                                    option_id,
                                    message: "agent session is not ready".to_owned(),
                                },
                            )
                            .await?;
                            continue;
                        };
                        match await_rpc(
                            connection
                                .send_request(SetSessionConfigOptionRequest::new(
                                session_id,
                                option_id.clone(),
                                SessionConfigOptionValue::value_id(value.clone()),
                                ))
                                .block_task(),
                            RPC_TIMEOUT,
                            "ACP setting change",
                        )
                        .await
                        {
                            Ok(response) => send_payload(
                                &event_tx,
                                AgentStreamPayload::ConfigOptionsChanged {
                                    option_id,
                                    value,
                                    config_options: json_of(&response.config_options)?,
                                },
                            )
                            .await?,
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SettingFailed {
                                    option_id,
                                    message: format!("could not change agent setting: {error}"),
                                },
                            )
                            .await?,
                        }
                    }
                    RuntimeCommand::SetMode { mode_id } => {
                        let Some(session_id) = session.clone() else {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::SettingFailed {
                                    option_id: mode_id,
                                    message: "agent session is not ready".to_owned(),
                                },
                            )
                            .await?;
                            continue;
                        };
                        match await_rpc(
                            connection
                                .send_request(SetSessionModeRequest::new(
                                    session_id,
                                    mode_id.clone(),
                                ))
                                .block_task(),
                            RPC_TIMEOUT,
                            "ACP mode change",
                        )
                        .await
                        {
                            Ok(_) => send_payload(
                                &event_tx,
                                AgentStreamPayload::ModeChanged { mode_id },
                            )
                            .await?,
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SettingFailed {
                                    option_id: mode_id,
                                    message: format!(
                                        "could not change agent permission mode: {error}"
                                    ),
                                },
                            )
                            .await?,
                        }
                    }
                    RuntimeCommand::Shutdown => {
                        if let Some(session_id) = session.clone() {
                            if capabilities.close {
                                if let Err(error) = await_rpc(
                                    connection
                                        .send_request(CloseSessionRequest::new(session_id))
                                        .block_task(),
                                    CLOSE_TIMEOUT,
                                    "ACP session close",
                                )
                                .await
                                {
                                    log::warn!(target: "zz::agent", "could not close ACP session during shutdown: {error}");
                                }
                            } else {
                                connection.send_notification(CancelNotification::new(session_id))?;
                            }
                        }
                        cancel_pending_permissions(&routing, &event_tx).await?;
                        break;
                    }
                }
            }
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())
}

#[track_caller]
fn spawn_close_session(
    connection: &ConnectionTo<Agent>,
    session_id: AcpSessionId,
    reason: &'static str,
) -> Result<(), agent_client_protocol::Error> {
    let close = connection.send_request(CloseSessionRequest::new(session_id));
    connection.spawn(async move {
        if let Err(error) = await_rpc(close.block_task(), CLOSE_TIMEOUT, "ACP session close").await
        {
            log::warn!(target: "zz::agent", "could not close {reason}: {error}");
        }
        Ok(())
    })
}

fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_BYTES
        && !session_id.chars().any(char::is_control)
}

fn valid_session_cursor(cursor: &str) -> bool {
    !cursor.is_empty()
        && cursor.len() <= MAX_SESSION_CURSOR_BYTES
        && !cursor.chars().any(char::is_control)
}

fn valid_session_summary(session: &AgentSessionSummary) -> bool {
    valid_session_id(&session.session_id)
        && valid_session_directory(&session.cwd)
        && session.additional_directories.len() <= MAX_AGENT_SESSION_DIRECTORIES
        && session
            .additional_directories
            .iter()
            .all(|directory| valid_session_directory(directory))
        && session.title.as_deref().is_none_or(|title| {
            title.len() <= MAX_SESSION_TITLE_BYTES && !title.chars().any(char::is_control)
        })
        && session.updated_at.as_deref().is_none_or(|timestamp| {
            timestamp.len() <= MAX_SESSION_TIMESTAMP_BYTES
                && !timestamp.chars().any(char::is_control)
        })
}

fn valid_session_directory(path: &std::path::Path) -> bool {
    path.is_absolute() && path.as_os_str().as_encoded_bytes().len() <= MAX_GUI_TEXT_BYTES
}

fn clean_session_metadata(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

async fn send_payload(
    sender: &Sender<AgentStreamPayload>,
    payload: AgentStreamPayload,
) -> Result<(), agent_client_protocol::Error> {
    validate_payload(&payload)?;
    sender.send(payload).await.map_err(|error| {
        agent_client_protocol::Error::internal_error()
            .data(format!("agent stream is unavailable: {error}"))
    })
}

fn validate_payload(payload: &AgentStreamPayload) -> Result<usize, agent_client_protocol::Error> {
    let bytes = serde_json::to_vec(payload).map_err(|error| {
        agent_client_protocol::Error::internal_error()
            .data(format!("could not encode agent stream item: {error}"))
    })?;
    let limit = match payload {
        AgentStreamPayload::PermissionRequested { .. } => MAX_AGENT_PERMISSION_BYTES,
        _ => MAX_AGENT_RESULT_BYTES,
    };
    if bytes.len() > limit {
        return Err(agent_client_protocol::Error::internal_error()
            .data(format!("agent stream item exceeds the {limit} byte limit")));
    }
    Ok(bytes.len())
}

async fn await_rpc<T>(
    request: impl Future<Output = Result<T, agent_client_protocol::Error>>,
    timeout: Duration,
    operation: &'static str,
) -> Result<T, agent_client_protocol::Error> {
    futures_lite::future::race(request, async move {
        smol::Timer::after(timeout).await;
        Err(agent_client_protocol::Error::internal_error().data(format!(
            "{operation} timed out after {} seconds",
            timeout.as_secs()
        )))
    })
    .await
}

/// The ACP types are JSON by construction, so an encode failure is a bug, not
/// a runtime condition; it still travels as an ACP error rather than a panic.
fn json_of(value: &impl serde::Serialize) -> Result<Value, agent_client_protocol::Error> {
    serde_json::to_value(value).map_err(|error| {
        agent_client_protocol::Error::internal_error()
            .data(format!("could not encode an agent payload: {error}"))
    })
}

fn optional_json(
    value: Option<&impl serde::Serialize>,
) -> Result<Option<Value>, agent_client_protocol::Error> {
    value.map(json_of).transpose()
}

fn encode_update(update: &SessionUpdate) -> Result<Value, agent_client_protocol::Error> {
    json_of(update)
}

fn update_payload(
    update: &SessionUpdate,
) -> Result<AgentStreamPayload, agent_client_protocol::Error> {
    let payload = AgentStreamPayload::Update {
        update: encode_update(update)?,
    };
    validate_payload(&payload)?;
    Ok(payload)
}

async fn cancel_pending_permissions(
    routing: &Arc<Mutex<RuntimeRouting>>,
    event_tx: &Sender<AgentStreamPayload>,
) -> Result<(), agent_client_protocol::Error> {
    let pending = routing.lock().take_permissions();
    for (request_id, pending) in pending {
        pending.responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ))?;
        send_payload(
            event_tx,
            AgentStreamPayload::PermissionResolved {
                request_id,
                canceled: true,
            },
        )
        .await?;
    }
    Ok(())
}

fn auth_method_model(method: &AuthMethod) -> AgentAuthMethod {
    AgentAuthMethod {
        id: method.id().0.to_string(),
        name: method.name().to_owned(),
        description: method.description().map(ToOwned::to_owned),
    }
}

/// A permission request is a QUESTION rather than a tool approval when any
/// option carries a kind outside the allow/reject set — that is how agents
/// relay user-facing choices. Repeated kinds are not a question signal:
/// codex-acp sends two `allow_always` options on every exec approval.
fn is_user_question(options: &[PermissionOption]) -> bool {
    options.iter().any(|option| {
        !matches!(
            option.kind,
            PermissionOptionKind::AllowOnce
                | PermissionOptionKind::AllowAlways
                | PermissionOptionKind::RejectOnce
                | PermissionOptionKind::RejectAlways
        )
    })
}

/// The option an unattended approval picks: `allow_always` over `allow_once`,
/// and never a reject — a request with no allow option falls through to the
/// permission UI rather than being silently denied.
fn preferred_allow_option(options: &[PermissionOption]) -> Option<String> {
    let by_kind = |kind: PermissionOptionKind| {
        options
            .iter()
            .find(|option| option.kind == kind)
            .map(|option| option.option_id.0.to_string())
    };
    by_kind(PermissionOptionKind::AllowAlways)
        .or_else(|| by_kind(PermissionOptionKind::AllowOnce))
        .filter(|option_id| !option_id.is_empty())
}

fn prompt_blocks(prompt: AgentPrompt) -> Vec<ContentBlock> {
    let AgentPrompt { text, images, .. } = prompt;
    let mut blocks = Vec::with_capacity(usize::from(!text.is_empty()) + images.len());
    if !text.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(text)));
    }
    blocks.extend(images.into_iter().map(|image| {
        ContentBlock::Image(ImageContent::new(BASE64.encode(&image.data), image.format))
    }));
    blocks
}

/// The quiesce window, read once per process. Absent or unparseable falls back
/// to the default; `0` disables the watchdog.
pub(crate) fn quiesce_window() -> Option<Duration> {
    static WINDOW: OnceLock<Option<Duration>> = OnceLock::new();

    *WINDOW.get_or_init(|| parse_quiesce_window(std::env::var_os("ZZ_AGENT_QUIESCE_MS").as_deref()))
}

fn parse_quiesce_window(value: Option<&std::ffi::OsStr>) -> Option<Duration> {
    let millis = value
        .and_then(std::ffi::OsStr::to_str)
        .map(str::trim)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_QUIESCE_MS);
    (millis > 0).then(|| Duration::from_millis(millis))
}

/// Silence alone never ends a run: a live child is the working signal, so the
/// watchdog parks only once the turn has been quiet past `window` with nothing
/// outstanding. A false trip costs a status dip, never data.
pub(crate) fn should_park_turn(
    window: Option<Duration>,
    silence: Duration,
    in_flight: bool,
) -> bool {
    window.is_some_and(|window| !in_flight && silence >= window)
}

fn capped(mut text: String, max_bytes: usize) -> String {
    truncate_payload(&mut text, max_bytes);
    text
}

/// Cut `text` to `max_bytes` on a char boundary, leaving a visible marker. The
/// marker is budgeted inside the cap, so the result never exceeds it.
fn truncate_payload(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes.saturating_sub(TRUNCATION_MARKER.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(TRUNCATION_MARKER);
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use agent_client_protocol::schema::v1::{PermissionOptionId, PermissionOptionKind};

    use super::*;
    use crate::agent::stream::AgentImage;

    fn option(id: &str, kind: PermissionOptionKind) -> PermissionOption {
        let mut option = PermissionOption::new(PermissionOptionId::new(id), "label", kind);
        option.meta = None;
        option
    }

    #[test]
    fn repeated_allow_kinds_are_not_a_user_question() {
        let codex_exec_approval = [
            option("allow", PermissionOptionKind::AllowAlways),
            option("allow-once", PermissionOptionKind::AllowAlways),
            option("reject", PermissionOptionKind::RejectOnce),
        ];
        assert!(!is_user_question(&codex_exec_approval));
        assert!(!is_user_question(&[]));
    }

    #[test]
    fn an_rpc_that_never_answers_hits_its_deadline() {
        let result = futures_lite::future::block_on(await_rpc(
            std::future::pending::<Result<(), agent_client_protocol::Error>>(),
            Duration::from_millis(1),
            "fixture RPC",
        ));
        assert!(
            result
                .expect_err("the pending request must time out")
                .to_string()
                .contains("fixture RPC timed out")
        );
    }

    #[test]
    fn the_preferred_allow_option_never_rejects() {
        assert_eq!(
            preferred_allow_option(&[
                option("once", PermissionOptionKind::AllowOnce),
                option("always", PermissionOptionKind::AllowAlways),
            ]),
            Some("always".to_owned())
        );
        assert_eq!(
            preferred_allow_option(&[
                option("no", PermissionOptionKind::RejectAlways),
                option("once", PermissionOptionKind::AllowOnce),
            ]),
            Some("once".to_owned())
        );
        assert_eq!(
            preferred_allow_option(&[option("no", PermissionOptionKind::RejectOnce)]),
            None
        );
        assert_eq!(preferred_allow_option(&[]), None);
    }

    #[test]
    fn the_quiesce_window_falls_back_to_the_default_and_zero_disables_it() {
        assert_eq!(
            parse_quiesce_window(None),
            Some(Duration::from_millis(DEFAULT_QUIESCE_MS))
        );
        assert_eq!(
            parse_quiesce_window(Some(OsStr::new("not a number"))),
            Some(Duration::from_millis(DEFAULT_QUIESCE_MS))
        );
        assert_eq!(
            parse_quiesce_window(Some(OsStr::new(" 500 "))),
            Some(Duration::from_millis(500))
        );
        assert_eq!(parse_quiesce_window(Some(OsStr::new("0"))), None);
        assert!(!should_park_turn(
            parse_quiesce_window(Some(OsStr::new("0"))),
            Duration::from_hours(1),
            false
        ));
    }

    #[test]
    fn a_turn_parks_only_when_it_is_quiet_and_nothing_is_outstanding() {
        let window = Duration::from_millis(100);
        assert!(should_park_turn(
            Some(window),
            Duration::from_millis(150),
            false
        ));
        assert!(!should_park_turn(
            Some(window),
            Duration::from_millis(150),
            true
        ));
        assert!(!should_park_turn(
            Some(window),
            Duration::from_millis(50),
            false
        ));
    }

    #[test]
    fn a_prompt_carries_its_text_then_its_images() {
        let blocks = prompt_blocks(AgentPrompt {
            owner: ClientInstanceId::default(),
            text: "look".to_owned(),
            images: vec![AgentImage {
                format: "image/png".to_owned(),
                data: b"zz".to_vec(),
            }],
        });

        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Text(text) if text.text == "look"));
        let ContentBlock::Image(image) = &blocks[1] else {
            panic!("the attachment should ride as an image block");
        };
        assert_eq!(image.mime_type, "image/png");
        assert_eq!(image.data, "eno=");

        assert!(prompt_blocks(AgentPrompt::default()).is_empty());
    }

    #[test]
    fn an_unexpected_exit_reports_the_status_and_the_stderr_tail() {
        let tail = StderrTail::default();
        assert_eq!(tail.snapshot(), None);
        tail.push("   ");
        tail.push("could not find module");
        assert_eq!(tail.snapshot().as_deref(), Some("could not find module"));

        assert_eq!(
            runtime_failure_message(
                "Codex",
                "Process exited with status 127: boom",
                tail.snapshot()
            ),
            "Codex exited unexpectedly (status 127: boom): could not find module"
        );
        assert_eq!(
            runtime_failure_message("Codex", "the pipe broke\nand then some", None),
            "Codex exited unexpectedly (the pipe broke)"
        );
    }

    #[test]
    fn session_metadata_is_validated_before_it_is_believed() {
        let summary = |session_id: &str, cwd: &str| AgentSessionSummary {
            session_id: session_id.to_owned(),
            cwd: PathBuf::from(cwd),
            additional_directories: Vec::new(),
            title: None,
            updated_at: None,
        };

        assert!(valid_session_summary(&summary("s-1", "/work")));
        assert!(!valid_session_summary(&summary("", "/work")));
        assert!(!valid_session_summary(&summary("s-1", "relative")));
        assert!(!valid_session_id("with\u{7}bell"));
        assert!(!valid_session_cursor(""));
        assert_eq!(
            clean_session_metadata("  titled  ", 64).as_deref(),
            Some("titled")
        );
        assert_eq!(clean_session_metadata("   ", 64), None);
    }
}
