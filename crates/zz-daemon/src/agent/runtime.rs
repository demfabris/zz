//! One ACP connection, driven by commands and answering in stream payloads.
//!
//! This is the runtime half of the desktop's agent controller, moved into the
//! daemon: it owns the adapter child, the ACP session, the permission
//! responders, and the journal, and it never knows what renders the result.
//! One connection serves exactly one pane, so nothing here routes by pane —
//! the host stamps that on the way out.

use std::{
    collections::{HashMap, HashSet},
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
use zz_protocol::AgentProvider;

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
        session_id: String,
    },
    Prompt {
        prompt: AgentPrompt,
    },
    Cancel,
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
    Shutdown,
}

struct PendingPermissionResponder {
    responder: Responder<RequestPermissionResponse>,
}

/// Which sessions this connection still speaks for. A session leaves the moment
/// it is superseded, so the late updates of a switched-away session are dropped
/// instead of landing in the pane that moved on.
#[derive(Default)]
struct RuntimeRouting {
    live_sessions: HashSet<String>,
    staged_updates: HashMap<String, Vec<SessionUpdate>>,
    permissions: HashMap<u64, PendingPermissionResponder>,
    /// Sessions whose updates are recorded. A session only enters once its
    /// transcript has settled in the pane, so the burst an agent replays out of
    /// `session/load` is never journalled on top of what it already replays.
    journaled: HashSet<String>,
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

fn record_update(journal: Option<&AgentJournal>, session_id: &str, update: &SessionUpdate) {
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
    if let Err(error) = journal.append(session_id, &value) {
        report_journal_error(session_id, &error.to_string());
    }
}

fn record_updates(journal: Option<&AgentJournal>, session_id: &str, updates: &[SessionUpdate]) {
    for update in updates {
        record_update(journal, session_id, update);
    }
}

/// The journalled transcript of `session_id`, as updates the reducer replays
/// exactly like the ones an agent sends out of `session/load`.
fn journal_replay(journal: Option<&AgentJournal>, session_id: Option<&str>) -> Vec<SessionUpdate> {
    let (Some(journal), Some(session_id)) = (journal, session_id) else {
        return Vec::new();
    };
    let records = match journal.replay(session_id) {
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
                Ok(update) => Some(update),
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

/// Seed a freshly created session with a journalled transcript, and take over
/// journalling under its id. Staging the live session before the copy is what
/// makes the restore atomic: an update that arrives mid-copy queues behind the
/// restored entries instead of racing them into the pane.
fn restore_journaled_session(
    routing: &Mutex<RuntimeRouting>,
    journal: Option<&AgentJournal>,
    session_id: &str,
    restored_from: Option<&str>,
    restored: Vec<SessionUpdate>,
) -> Vec<SessionUpdate> {
    {
        let mut routes = routing.lock();
        routes
            .staged_updates
            .insert(session_id.to_owned(), Vec::new());
        routes.live_sessions.insert(session_id.to_owned());
    }
    record_updates(journal, session_id, &restored);
    if let (Some(journal), Some(previous)) = (journal, restored_from)
        && previous != session_id
        && let Err(error) = journal.remove(previous)
    {
        report_journal_error(previous, &error.to_string());
    }
    let staged = {
        let mut routes = routing.lock();
        routes.journaled.insert(session_id.to_owned());
        routes.staged_updates.remove(session_id).unwrap_or_default()
    };
    record_updates(journal, session_id, &staged);
    let mut replay = restored;
    replay.extend(staged);
    replay
}

pub(crate) async fn run_agent_connection(
    provider: AgentProvider,
    auto_approve: bool,
    agent: impl ConnectTo<AcpClientRole>,
    permission_ids: Arc<AtomicU64>,
    journal: Option<Arc<AgentJournal>>,
    command_rx: Receiver<RuntimeCommand>,
    event_tx: Sender<AgentStreamPayload>,
) -> Result<(), String> {
    let routing = Arc::new(Mutex::new(RuntimeRouting::default()));

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
                        let mut routing = notification_routing.lock();
                        if let Some(updates) = routing.staged_updates.get_mut(&session_id) {
                            updates.push(notification.update);
                            return Ok(());
                        }
                        let live = routing.live_sessions.contains(&session_id);
                        let journaled = routing.journaled.contains(&session_id);
                        drop(routing);
                        if journaled {
                            record_update(
                                notification_journal.as_deref(),
                                &session_id,
                                &notification.update,
                            );
                        }
                        if live {
                            send_payload(
                                &notification_events,
                                AgentStreamPayload::Update {
                                    update: encode_update(&notification.update)?,
                                },
                            )?;
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
                        let routing = ext_routing.lock();
                        if routing.staged_updates.contains_key(&session_id) {
                            return Ok(());
                        }
                        let live = routing.live_sessions.contains(&session_id);
                        drop(routing);
                        if live {
                            send_payload(&ext_events, AgentStreamPayload::TaskEvent { event })?;
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
                let routing = permission_routing.lock();
                let live = routing.live_sessions.contains(&session_id)
                    && !routing.staged_updates.contains_key(&session_id);
                drop(routing);
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
                    let update = SessionUpdate::ToolCallUpdate(request.tool_call);
                    if let Err(error) = encode_update(&update).and_then(|update| {
                        send_payload(&permission_events, AgentStreamPayload::Update { update })
                    }) {
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
                permission_routing
                    .lock()
                    .permissions
                    .insert(request_id, PendingPermissionResponder { responder });
                let requested = json_of(&request.tool_call)
                    .and_then(|tool_call| Ok((tool_call, json_of(&request.options)?)))
                    .and_then(|(tool_call, options)| {
                        send_payload(
                            &permission_events,
                            AgentStreamPayload::PermissionRequested {
                                request_id,
                                tool_call,
                                options,
                            },
                        )
                    });
                if let Err(error) = requested {
                    if let Some(pending) = permission_routing.lock().permissions.remove(&request_id)
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
            let response = connection.send_request(initialize).block_task().await?;
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
            )?;

            let mut session: Option<AcpSessionId> = None;
            while let Ok(command) = command_rx.recv().await {
                match command {
                    RuntimeCommand::Open {
                        cwd,
                        resume_session,
                    } => {
                        // An agent that cannot load the session itself is
                        // restored from the journal instead, so the pane says
                        // RESTORING on both routes rather than flashing
                        // STARTING at a transcript that is about to appear.
                        let mut restored = if capabilities.load {
                            Vec::new()
                        } else {
                            journal_replay(journal.as_deref(), resume_session.as_deref())
                        };
                        let restoring =
                            (capabilities.load && resume_session.is_some()) || !restored.is_empty();
                        send_payload(&event_tx, AgentStreamPayload::SessionReset { restoring })?;
                        let session_result = if let Some(resume) =
                            resume_session.clone().filter(|_| capabilities.load)
                        {
                            let session_id = AcpSessionId::new(resume);
                            routing
                                .lock()
                                .live_sessions
                                .insert(session_id.0.to_string());
                            match connection
                                .send_request(load_session_request(
                                    provider,
                                    session_id.clone(),
                                    cwd.clone(),
                                ))
                                .block_task()
                                .await
                            {
                                Ok(response) => {
                                    Ok((session_id, response.modes, response.config_options))
                                }
                                Err(error) => {
                                    routing
                                        .lock()
                                        .live_sessions
                                        .remove(session_id.0.as_ref());
                                    log::warn!(
                                        target: "zz::agent",
                                        "could not restore the ACP session: {error}; creating a new session"
                                    );
                                    restored = journal_replay(
                                        journal.as_deref(),
                                        resume_session.as_deref(),
                                    );
                                    connection
                                        .send_request(new_session_request(provider, cwd.clone()))
                                        .block_task()
                                        .await
                                        .map(|response| {
                                            (
                                                response.session_id,
                                                response.modes,
                                                response.config_options,
                                            )
                                        })
                                }
                            }
                        } else {
                            connection
                                .send_request(new_session_request(provider, cwd.clone()))
                                .block_task()
                                .await
                                .map(|response| {
                                    (
                                        response.session_id,
                                        response.modes,
                                        response.config_options,
                                    )
                                })
                        };
                        match session_result {
                            Ok((session_id, modes, config_options)) => {
                                if !valid_session_id(session_id.0.as_ref()) {
                                    routing.lock().live_sessions.clear();
                                    send_payload(
                                        &event_tx,
                                        AgentStreamPayload::PaneFailed {
                                            message: "agent returned an invalid session ID"
                                                .to_owned(),
                                        },
                                    )?;
                                    continue;
                                }
                                let live = session_id.0.to_string();
                                session = Some(session_id);
                                if restored.is_empty() {
                                    let mut routes = routing.lock();
                                    routes.live_sessions.insert(live.clone());
                                    routes.journaled.insert(live.clone());
                                    drop(routes);
                                    send_payload(
                                        &event_tx,
                                        AgentStreamPayload::SessionReady {
                                            session_id: live,
                                            modes: optional_json(modes.as_ref())?,
                                            config_options: optional_json(
                                                config_options.as_ref(),
                                            )?,
                                        },
                                    )?;
                                } else {
                                    let replay = restore_journaled_session(
                                        &routing,
                                        journal.as_deref(),
                                        &live,
                                        resume_session.as_deref(),
                                        restored,
                                    );
                                    send_payload(
                                        &event_tx,
                                        AgentStreamPayload::SessionSwitched {
                                            session_id: live,
                                            cwd,
                                            modes: optional_json(modes.as_ref())?,
                                            config_options: optional_json(
                                                config_options.as_ref(),
                                            )?,
                                            replay: encode_updates(&replay)?,
                                        },
                                    )?;
                                }
                            }
                            Err(error) => {
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::PaneFailed {
                                        message: error.to_string(),
                                    },
                                )?;
                            }
                        }
                    }
                    RuntimeCommand::ListSessions {
                        cwd,
                        cursor,
                        replace,
                    } => {
                        if !capabilities.list {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionListFailed {
                                    message: "agent does not support session/list".to_owned(),
                                },
                            )?;
                            continue;
                        }
                        let request = ListSessionsRequest::new().cwd(cwd.clone()).cursor(cursor);
                        match connection.send_request(request).block_task().await {
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
                                        sessions,
                                        next_cursor: response
                                            .next_cursor
                                            .filter(|cursor| valid_session_cursor(cursor)),
                                        cwd_filter: cwd,
                                        replace,
                                    },
                                )?;
                            }
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionListFailed {
                                    message: format!("could not list agent sessions: {error}"),
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::SwitchSession { session: target } => {
                        if !capabilities.load {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionSwitchFailed {
                                    message: "agent does not support session/load".to_owned(),
                                },
                            )?;
                            continue;
                        }
                        let session_id = AcpSessionId::new(target.session_id.clone());
                        {
                            let mut routing = routing.lock();
                            routing.live_sessions.insert(target.session_id.clone());
                            routing
                                .staged_updates
                                .insert(target.session_id.clone(), Vec::new());
                        }
                        let mut request =
                            load_session_request(provider, session_id.clone(), target.cwd.clone());
                        if capabilities.additional_directories {
                            request = request
                                .additional_directories(target.additional_directories.clone());
                        }
                        match connection.send_request(request).block_task().await {
                            Ok(response) => {
                                let previous = session.replace(session_id.clone());
                                let previous = previous.filter(|previous| previous != &session_id);
                                let replay = {
                                    let mut routes = routing.lock();
                                    let replay = routes
                                        .staged_updates
                                        .remove(&target.session_id)
                                        .unwrap_or_default();
                                    routes.journaled.insert(target.session_id.clone());
                                    if let Some(previous) = &previous {
                                        routes.live_sessions.remove(previous.0.as_ref());
                                        routes.journaled.remove(previous.0.as_ref());
                                    }
                                    replay
                                };
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionSwitched {
                                        session_id: target.session_id,
                                        cwd: target.cwd,
                                        modes: optional_json(response.modes.as_ref())?,
                                        config_options: optional_json(
                                            response.config_options.as_ref(),
                                        )?,
                                        replay: encode_updates(&replay)?,
                                    },
                                )?;
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
                                {
                                    let mut routes = routing.lock();
                                    routes.staged_updates.remove(&target.session_id);
                                    routes.live_sessions.remove(&target.session_id);
                                }
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionSwitchFailed {
                                        message: format!("could not load selected session: {error}"),
                                    },
                                )?;
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
                        match connection
                            .send_request(new_session_request(provider, cwd.clone()))
                            .block_task()
                            .await
                        {
                            Ok(response) if valid_session_id(response.session_id.0.as_ref()) => {
                                let session_id = response.session_id;
                                let previous = session.replace(session_id.clone());
                                let previous = previous.filter(|previous| previous != &session_id);
                                {
                                    let mut routes = routing.lock();
                                    routes.live_sessions.insert(session_id.0.to_string());
                                    routes.journaled.insert(session_id.0.to_string());
                                    if let Some(previous) = &previous {
                                        routes.live_sessions.remove(previous.0.as_ref());
                                        routes.journaled.remove(previous.0.as_ref());
                                    }
                                }
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionSwitched {
                                        session_id: session_id.0.to_string(),
                                        cwd,
                                        modes: optional_json(response.modes.as_ref())?,
                                        config_options: optional_json(
                                            response.config_options.as_ref(),
                                        )?,
                                        replay: Vec::new(),
                                    },
                                )?;
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
                            Ok(_) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionSwitchFailed {
                                    message: "agent returned an invalid session ID".to_owned(),
                                },
                            )?,
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionSwitchFailed {
                                    message: format!("could not create a new session: {error}"),
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::DeleteSession { session_id } => {
                        if !capabilities.delete {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionDeleteFailed {
                                    message: "agent does not support session/delete".to_owned(),
                                },
                            )?;
                            continue;
                        }
                        match connection
                            .send_request(DeleteSessionRequest::new(session_id.clone()))
                            .block_task()
                            .await
                        {
                            Ok(_) => {
                                routing.lock().journaled.remove(&session_id);
                                if let Some(journal) = journal.as_deref()
                                    && let Err(error) = journal.remove(&session_id)
                                {
                                    report_journal_error(&session_id, &error.to_string());
                                }
                                send_payload(
                                    &event_tx,
                                    AgentStreamPayload::SessionDeleted { session_id },
                                )?;
                            }
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SessionDeleteFailed {
                                    message: format!("could not delete session: {error}"),
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::Prompt { prompt } => {
                        let Some(session_id) = session.clone() else {
                            send_payload(
                                &event_tx,
                                AgentStreamPayload::PaneFailed {
                                    message: "agent session is not ready".to_owned(),
                                },
                            )?;
                            continue;
                        };
                        let prompt_events = event_tx.clone();
                        let request = connection.send_request(PromptRequest::new(
                            session_id,
                            prompt_blocks(prompt),
                        ));
                        connection.spawn(async move {
                            let outcome = match request.block_task().await {
                                Ok(response) => AgentPromptOutcome::Finished {
                                    stop_reason: serde_json::to_value(response.stop_reason)
                                        .unwrap_or(Value::Null),
                                },
                                Err(error) => AgentPromptOutcome::Failed {
                                    message: error.to_string(),
                                },
                            };
                            let _ = prompt_events
                                .send(AgentStreamPayload::PromptFinished { outcome })
                                .await;
                            Ok(())
                        })?;
                    }
                    RuntimeCommand::Cancel => {
                        if let Some(session_id) = session.clone() {
                            connection.send_notification(CancelNotification::new(session_id))?;
                            cancel_pending_permissions(&routing, &event_tx)?;
                        }
                    }
                    RuntimeCommand::RespondPermission {
                        request_id,
                        option_id,
                    } => {
                        let pending = routing.lock().permissions.remove(&request_id);
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
                                &event_tx,
                                AgentStreamPayload::PermissionResolved {
                                    request_id,
                                    canceled,
                                },
                            )?;
                        }
                    }
                    RuntimeCommand::Authenticate { method_id } => {
                        match connection
                            .send_request(AuthenticateRequest::new(method_id))
                            .block_task()
                            .await
                        {
                            Ok(_) => send_payload(&event_tx, AgentStreamPayload::Authenticated)?,
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::AuthenticationFailed {
                                    message: format!("authentication failed: {error}"),
                                },
                            )?,
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
                            )?;
                            continue;
                        };
                        match connection
                            .send_request(SetSessionConfigOptionRequest::new(
                                session_id,
                                option_id.clone(),
                                SessionConfigOptionValue::value_id(value.clone()),
                            ))
                            .block_task()
                            .await
                        {
                            Ok(response) => send_payload(
                                &event_tx,
                                AgentStreamPayload::ConfigOptionsChanged {
                                    option_id,
                                    value,
                                    config_options: json_of(&response.config_options)?,
                                },
                            )?,
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SettingFailed {
                                    option_id,
                                    message: format!("could not change agent setting: {error}"),
                                },
                            )?,
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
                            )?;
                            continue;
                        };
                        match connection
                            .send_request(SetSessionModeRequest::new(session_id, mode_id.clone()))
                            .block_task()
                            .await
                        {
                            Ok(_) => send_payload(
                                &event_tx,
                                AgentStreamPayload::ModeChanged { mode_id },
                            )?,
                            Err(error) => send_payload(
                                &event_tx,
                                AgentStreamPayload::SettingFailed {
                                    option_id: mode_id,
                                    message: format!(
                                        "could not change agent permission mode: {error}"
                                    ),
                                },
                            )?,
                        }
                    }
                    RuntimeCommand::Shutdown => {
                        if let Some(session_id) = session.clone() {
                            if capabilities.close {
                                if let Err(error) = connection
                                    .send_request(CloseSessionRequest::new(session_id))
                                    .block_task()
                                    .await
                                {
                                    log::warn!(target: "zz::agent", "could not close ACP session during shutdown: {error}");
                                }
                            } else {
                                connection.send_notification(CancelNotification::new(session_id))?;
                            }
                        }
                        cancel_pending_permissions(&routing, &event_tx)?;
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
        if let Err(error) = close.block_task().await {
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
        && session.cwd.is_absolute()
        && session
            .additional_directories
            .iter()
            .all(|directory| directory.is_absolute())
        && session.title.as_deref().is_none_or(|title| {
            title.len() <= MAX_SESSION_TITLE_BYTES && !title.chars().any(char::is_control)
        })
        && session.updated_at.as_deref().is_none_or(|timestamp| {
            timestamp.len() <= MAX_SESSION_TIMESTAMP_BYTES
                && !timestamp.chars().any(char::is_control)
        })
}

fn clean_session_metadata(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn send_payload(
    sender: &Sender<AgentStreamPayload>,
    payload: AgentStreamPayload,
) -> Result<(), agent_client_protocol::Error> {
    sender.try_send(payload).map_err(|error| {
        agent_client_protocol::Error::internal_error()
            .data(format!("agent stream is unavailable: {error}"))
    })
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

fn encode_updates(updates: &[SessionUpdate]) -> Result<Vec<Value>, agent_client_protocol::Error> {
    updates.iter().map(encode_update).collect()
}

fn cancel_pending_permissions(
    routing: &Arc<Mutex<RuntimeRouting>>,
    event_tx: &Sender<AgentStreamPayload>,
) -> Result<(), agent_client_protocol::Error> {
    let pending_ids = routing
        .lock()
        .permissions
        .keys()
        .copied()
        .collect::<Vec<_>>();
    for request_id in pending_ids {
        let pending = routing.lock().permissions.remove(&request_id);
        if let Some(pending) = pending {
            pending.responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ))?;
            send_payload(
                event_tx,
                AgentStreamPayload::PermissionResolved {
                    request_id,
                    canceled: true,
                },
            )?;
        }
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
    let AgentPrompt { text, images } = prompt;
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
