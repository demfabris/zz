use std::{
    collections::BTreeSet,
    ffi::{CStr, c_char},
    path::PathBuf,
    time::{Duration, Instant},
};

use agent_client_protocol_schema::{
    MaybeUndefined,
    v1::{SessionConfigOption, SessionModeState, SessionUpdate, StopReason},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zz_client::{
    agent_completion::{AgentCommand, active_command_hint, completion_query, ranked_completions},
    agent_config::{
        AgentConfigOption, AgentMode, agent_command_model, config_option_models, rendered_error,
        valid_session_cursor, valid_session_directory, valid_session_id, valid_session_summary,
    },
    agent_transcript::AgentTranscript,
};
use zz_protocol::{
    AgentConnectionPhase, AgentPaneWire, AgentProvider, AgentSessionOpKind, ClientInstanceId,
    GuiResponse, MAX_AGENT_AUTH_METHODS, MAX_AGENT_AVAILABLE_COMMANDS, MAX_AGENT_MODES,
    MAX_AGENT_PROMPT_BYTES, MAX_AGENT_PROMPT_IMAGES, MAX_AGENT_QUEUED_PROMPTS,
    MAX_AGENT_SESSION_DIRECTORIES, PaneId, PaneKindSnapshot,
    agent_stream::{
        AgentAuthMethod, AgentImage, AgentPrompt, AgentPromptOutcome, AgentSessionCapabilities,
        AgentSessionSummary, AgentStreamItem, AgentStreamPayload,
    },
};

use std::sync::{Arc, Mutex};
use zz_config::agent_preferences::{
    AgentPreferenceKind, AgentPreferences, LEGACY_MODE_PREFERENCE_ID, preference_kind_for_category,
};

use super::{ZzAgentBatch, ZzAgentSessionsReply, ZzAgentState, ZzClient, ZzJson, lock};

#[derive(Default, Serialize)]
struct History {
    sessions: Vec<AgentSessionSummary>,
    loading: bool,
    error: Option<String>,
    next_cursor: Option<String>,
    cwd_filter: Option<PathBuf>,
}

#[derive(Serialize)]
struct RestoredPrompt {
    reclaim_id: u64,
    text: String,
    images: Vec<AgentImage>,
}

struct SettingRequest {
    kind: Option<AgentPreferenceKind>,
    option: String,
    value: String,
    user: bool,
}

pub struct ZzAgentModel {
    pane: PaneId,
    provider: AgentProvider,
    cwd: PathBuf,
    owner: ClientInstanceId,
    transcript: AgentTranscript<AgentImage>,
    wire: AgentPaneWire,
    next_seq: u64,
    epoch: u64,
    session_reset: bool,
    replaying: bool,
    agent_name: Option<String>,
    agent_key: String,
    capabilities: AgentSessionCapabilities,
    commands: Vec<AgentCommand>,
    options: Vec<AgentConfigOption>,
    modes: Vec<AgentMode>,
    mode: Option<String>,
    auth_methods: Vec<AgentAuthMethod>,
    usage: Option<(u64, u64)>,
    history: History,
    restored: Vec<RestoredPrompt>,
    last_reclaim_id: u64,
    unknown_updates: u64,
    error: Option<String>,
    pending: Option<(String, Instant)>,
    cancelling: bool,
    active_turn: bool,
    preferences: Arc<Mutex<AgentPreferences>>,
    setting_request: Option<SettingRequest>,
    preference_skips: BTreeSet<(AgentPreferenceKind, String)>,
}

fn image(format: &str, data: Vec<u8>) -> Option<AgentImage> {
    matches!(
        format,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/svg+xml"
    )
    .then(|| AgentImage {
        format: format.to_owned(),
        data,
    })
}

impl ZzAgentModel {
    fn new(pane: PaneId, provider: AgentProvider, cwd: PathBuf, owner: ClientInstanceId) -> Self {
        Self {
            pane,
            provider,
            cwd,
            owner,
            transcript: AgentTranscript::new(image),
            wire: AgentPaneWire::default(),
            next_seq: 1,
            epoch: 1,
            session_reset: false,
            replaying: false,
            agent_name: None,
            agent_key: "acp-agent".to_owned(),
            capabilities: AgentSessionCapabilities::default(),
            commands: Vec::new(),
            options: Vec::new(),
            modes: Vec::new(),
            mode: None,
            auth_methods: Vec::new(),
            usage: None,
            history: History::default(),
            restored: Vec::new(),
            last_reclaim_id: 0,
            unknown_updates: 0,
            error: None,
            pending: None,
            cancelling: false,
            active_turn: false,
            preferences: Arc::default(),
            setting_request: None,
            preference_skips: BTreeSet::new(),
        }
    }

    fn reset(&mut self) {
        self.transcript.reset();
        self.epoch = self.epoch.saturating_add(1);
        self.commands.clear();
        self.options.clear();
        self.modes.clear();
        self.mode = None;
        self.usage = None;
        self.error = None;
        self.history.loading = false;
        self.pending = None;
        self.cancelling = false;
        self.active_turn = false;
        self.setting_request = None;
        self.preference_skips.clear();
        self.session_reset = true;
    }

    fn configuration(&mut self, modes: Option<Value>, options: Option<Value>) {
        if let Some(options) =
            options.and_then(|value| serde_json::from_value::<Vec<SessionConfigOption>>(value).ok())
        {
            self.options = config_option_models(options);
            self.mode = None;
            self.modes.clear();
        } else if let Some(modes) =
            modes.and_then(|value| serde_json::from_value::<SessionModeState>(value).ok())
        {
            self.options.clear();
            self.mode = Some(modes.current_mode_id.0.to_string());
            self.modes = modes
                .available_modes
                .into_iter()
                .take(MAX_AGENT_MODES)
                .map(|mode| AgentMode {
                    id: mode.id.0.to_string(),
                    name: mode.name,
                    description: mode.description,
                })
                .collect();
        }
    }

    fn phase(&self) -> &AgentConnectionPhase {
        if self.active_turn {
            if matches!(self.wire.phase, AgentConnectionPhase::AwaitingPermission) {
                &AgentConnectionPhase::AwaitingPermission
            } else {
                &AgentConnectionPhase::Running
            }
        } else if matches!(
            self.wire.phase,
            AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
        ) {
            &AgentConnectionPhase::Ready
        } else {
            &self.wire.phase
        }
    }

    fn begin_prompt(&mut self, text: String, images: Vec<AgentImage>) {
        self.transcript.begin_prompt(text, images);
        self.active_turn = true;
    }

    fn sync(&mut self, state: &AgentPaneWire) {
        if self.wire == *state {
            return;
        }
        match state.phase {
            AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission => {
                self.active_turn = true
            }
            AgentConnectionPhase::Starting | AgentConnectionPhase::Failed { .. } => {
                self.active_turn = false
            }
            AgentConnectionPhase::Ready => {}
        }
        let failed = matches!(state.phase, AgentConnectionPhase::Failed { .. });
        if failed {
            self.transcript.fail_inflight();
            self.pending = None;
            self.cancelling = false;
        }
        if !self.active_turn {
            self.cancelling = false;
        }
        self.error = state.error.clone().or_else(|| match &state.phase {
            AgentConnectionPhase::Failed { message } => Some(message.clone()),
            _ => None,
        });
        if let Ok(methods) = serde_json::from_str::<Vec<AgentAuthMethod>>(&state.auth_methods) {
            self.auth_methods = methods.into_iter().take(MAX_AGENT_AUTH_METHODS).collect();
        } else if state.auth_methods.is_empty() {
            self.auth_methods.clear();
        }
        self.transcript.clear_permissions();
        if let Some(permission) = &state.pending_permission
            && let Ok(payload) = serde_json::from_str::<Value>(&permission.payload)
            && let (Some(tool), Some(options)) = (payload.get("toolCall"), payload.get("options"))
            && let (Ok(tool), Ok(options)) = (
                serde_json::from_value(tool.clone()),
                serde_json::from_value(options.clone()),
            )
        {
            self.transcript
                .request_permission(permission.request_id, tool, options);
        }
        if state.config_options.is_empty() && state.modes.is_empty() {
            self.options.clear();
            self.modes.clear();
            self.mode = None;
        } else {
            self.configuration(
                serde_json::from_str(&state.modes).ok(),
                serde_json::from_str(&state.config_options).ok(),
            );
        }
        self.wire = state.clone();
    }

    fn update(&mut self, value: Value) {
        let Ok(update) = serde_json::from_value::<SessionUpdate>(value) else {
            self.unknown_updates = self.unknown_updates.saturating_add(1);
            return;
        };
        match update {
            SessionUpdate::AvailableCommandsUpdate(value) => {
                self.commands = value
                    .available_commands
                    .into_iter()
                    .take(MAX_AGENT_AVAILABLE_COMMANDS)
                    .map(agent_command_model)
                    .collect();
            }
            SessionUpdate::UsageUpdate(value) => self.usage = Some((value.used, value.size)),
            SessionUpdate::CurrentModeUpdate(value) => {
                self.mode = Some(value.current_mode_id.0.to_string())
            }
            SessionUpdate::ConfigOptionUpdate(value) => {
                self.options = config_option_models(value.config_options)
            }
            SessionUpdate::SessionInfoUpdate(value) => match value.title {
                MaybeUndefined::Undefined => {}
                MaybeUndefined::Null => self.wire.title = None,
                MaybeUndefined::Value(title) => self.wire.title = Some(title),
            },
            update => self.transcript.apply_update(update),
        }
    }

    fn restore(&mut self, reclaim_id: u64, prompts: Vec<AgentPrompt>) {
        if reclaim_id != 0 && reclaim_id <= self.last_reclaim_id {
            return;
        }
        let prompts = prompts
            .into_iter()
            .filter(|prompt| prompt.owner.0 == 0 || prompt.owner == self.owner)
            .collect::<Vec<_>>();
        if prompts.is_empty() {
            return;
        }
        let mut text = String::new();
        let mut images = Vec::new();
        for prompt in prompts {
            if !prompt.text.trim().is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&prompt.text);
            }
            images.extend(prompt.images);
        }
        self.last_reclaim_id = self.last_reclaim_id.max(reclaim_id);
        self.restored.push(RestoredPrompt {
            reclaim_id,
            text,
            images,
        });
    }

    fn payload(&mut self, payload: AgentStreamPayload) {
        match payload {
            AgentStreamPayload::Ready {
                agent_name,
                agent_key,
                auth_methods,
                capabilities,
            } => {
                self.agent_name = Some(agent_name);
                self.agent_key = agent_key;
                self.capabilities = capabilities;
                self.auth_methods = auth_methods
                    .into_iter()
                    .take(MAX_AGENT_AUTH_METHODS)
                    .collect();
            }
            AgentStreamPayload::SessionReset { .. } => self.reset(),
            AgentStreamPayload::SessionReady {
                session_id,
                modes,
                config_options,
            } => {
                self.wire.session_id = Some(session_id);
                self.configuration(modes, config_options);
                self.transcript.finish_replay();
                self.transcript.finish_turn();
                self.active_turn = false;
                self.session_reset = false;
                self.pending = None;
            }
            AgentStreamPayload::SessionSwitched {
                session_id,
                cwd,
                modes,
                config_options,
                replay,
            } => {
                if !self.session_reset {
                    self.reset();
                }
                self.cwd = cwd;
                for update in replay {
                    self.update(update);
                }
                self.configuration(modes, config_options);
                self.wire.session_id = Some(session_id);
                self.transcript.finish_replay();
                self.transcript.finish_turn();
                self.active_turn = false;
                self.session_reset = false;
                self.pending = None;
            }
            AgentStreamPayload::StateSynced { state } => {
                self.active_turn = matches!(
                    state.phase,
                    AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
                );
                self.sync(&state);
            }
            AgentStreamPayload::Update { update } => self.update(update),
            AgentStreamPayload::PermissionRequested {
                request_id,
                tool_call,
                options,
            } => {
                if let (Ok(tool), Ok(options)) = (
                    serde_json::from_value(tool_call),
                    serde_json::from_value(options),
                ) {
                    self.transcript
                        .request_permission(request_id, tool, options);
                }
            }
            AgentStreamPayload::PermissionResolved {
                request_id,
                canceled,
            } => self.transcript.resolve_permission(request_id, canceled),
            AgentStreamPayload::PromptFinished { outcome, .. } => {
                self.active_turn = false;
                self.cancelling = false;
                match outcome {
                    AgentPromptOutcome::Finished { stop_reason } => {
                        if serde_json::from_value::<StopReason>(stop_reason).ok()
                            == Some(StopReason::Cancelled)
                        {
                            self.transcript.cancel_inflight();
                        } else {
                            self.transcript.finish_turn();
                        }
                    }
                    AgentPromptOutcome::Failed { message } => {
                        self.error = Some(message);
                        self.transcript.fail_inflight();
                    }
                }
            }
            AgentStreamPayload::SessionsListed {
                sessions,
                next_cursor,
                cwd_filter,
                replace,
                ..
            } => {
                if replace {
                    self.history.sessions.clear();
                }
                let mut known = self
                    .history
                    .sessions
                    .iter()
                    .map(|session| session.session_id.clone())
                    .collect::<BTreeSet<_>>();
                self.history.sessions.extend(
                    sessions
                        .into_iter()
                        .filter(valid_session_summary)
                        .filter(|session| known.insert(session.session_id.clone())),
                );
                self.history.next_cursor =
                    next_cursor.filter(|cursor| valid_session_cursor(cursor));
                self.history.cwd_filter = cwd_filter;
                self.history.loading = false;
                self.history.error = None;
                self.pending = None;
            }
            AgentStreamPayload::SessionListFailed { message, .. }
            | AgentStreamPayload::SessionDeleteFailed { message, .. } => {
                self.history.error = Some(rendered_error(&message));
                self.history.loading = false;
                self.pending = None;
            }
            AgentStreamPayload::SessionDeleted { session_id, .. } => {
                self.history
                    .sessions
                    .retain(|session| session.session_id != session_id);
                self.history.loading = false;
                self.history.error = None;
                self.pending = None;
            }
            AgentStreamPayload::SessionSwitchFailed { message }
            | AgentStreamPayload::AuthenticationFailed { message } => {
                self.error = Some(message);
                self.pending = None;
            }
            AgentStreamPayload::SettingFailed { option_id, message } => {
                if self
                    .setting_request
                    .as_ref()
                    .is_some_and(|request| request.option == option_id)
                {
                    self.finish_setting(false, Some(message));
                }
            }
            AgentStreamPayload::PaneFailed { message } => {
                self.error = Some(message);
                self.pending = None;
                self.active_turn = false;
                self.transcript.fail_inflight();
            }
            AgentStreamPayload::Authenticated => {
                self.error = None;
                self.pending = None;
            }
            AgentStreamPayload::ConfigOptionsChanged {
                option_id,
                value,
                config_options,
            } => {
                self.configuration(None, Some(config_options));
                if self
                    .setting_request
                    .as_ref()
                    .is_some_and(|request| request.option == option_id && request.value == value)
                {
                    let applied = self
                        .options
                        .iter()
                        .any(|option| option.id == option_id && option.current_value == value);
                    self.finish_setting(applied, None);
                }
            }
            AgentStreamPayload::ModeChanged { mode_id } => {
                self.mode = Some(mode_id.clone());
                if self.setting_request.as_ref().is_some_and(|request| {
                    request.option == LEGACY_MODE_PREFERENCE_ID && request.value == mode_id
                }) {
                    self.finish_setting(true, None);
                }
            }
            AgentStreamPayload::PromptsReclaimed { prompts } => self.restore(0, prompts),
            AgentStreamPayload::PromptsRestored {
                reclaim_id,
                prompts,
            } => self.restore(reclaim_id, prompts),
            AgentStreamPayload::TurnStarted { .. } => self.active_turn = true,
            AgentStreamPayload::PromptAccepted { .. } => {}
        }
    }

    fn batch(&mut self, batch: &ZzAgentBatch) -> bool {
        if batch.pane != self.pane.0 {
            return false;
        }
        for (index, bytes) in batch.items.iter().enumerate() {
            let seq = batch.first_seq.saturating_add(index as u64);
            if seq < self.next_seq {
                continue;
            }
            let item = serde_json::from_slice::<AgentStreamItem>(bytes);
            if seq > self.next_seq
                && !(self.replaying
                    && item.as_ref().is_ok_and(|item| {
                        matches!(item.payload, AgentStreamPayload::SessionReset { .. })
                    }))
            {
                return false;
            }
            self.replaying = false;
            if let Ok(item) = item {
                if item.seq != seq {
                    return false;
                }
                self.payload(item.payload);
            } else {
                self.unknown_updates = self.unknown_updates.saturating_add(1);
            }
            self.next_seq = seq.saturating_add(1);
        }
        true
    }

    fn snapshot(&mut self, since: u64) -> Value {
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, started)| started.elapsed() >= Duration::from_secs(10))
        {
            self.finish_setting(
                false,
                Some("The agent did not answer. Try again.".to_owned()),
            );
            self.pending = None;
            self.history.loading = false;
            self.error = Some("The agent did not answer. Try again.".to_owned());
        }
        let changed = self.transcript.changed_entries(since);
        let reset = since == 0 || changed.is_none();
        let indices = if reset {
            (0..self.transcript.entries().len()).collect::<Vec<_>>()
        } else {
            changed.unwrap_or_default()
        };
        let entries = indices.into_iter().map(|index| json!({"index":index,"revision":self.transcript.entry_revisions()[index],"entry":self.transcript.entries()[index]})).collect::<Vec<_>>();
        let phase = match self.phase() {
            AgentConnectionPhase::Starting => "starting",
            AgentConnectionPhase::Ready => "ready",
            AgentConnectionPhase::Running => "running",
            AgentConnectionPhase::AwaitingPermission => "permission",
            AgentConnectionPhase::Failed { .. } => "failed",
        };
        json!({"epoch":self.epoch,"revision":self.transcript.revision(),"reset":reset,"entry_count":self.transcript.entries().len(),"entries":entries,
            "provider":self.provider,"cwd":self.cwd,"agent_name":self.agent_name,"agent_key":self.agent_key,"phase":phase,"session_id":self.wire.session_id,"title":self.wire.title,
            "capabilities":self.capabilities,"options":self.options,"modes":self.modes,"mode":self.mode,"commands":self.commands,"auth_methods":self.auth_methods,"usage":self.usage,"git":self.wire.git,
            "queued_prompts":self.wire.queued_prompts,"permissions":self.transcript.permissions(),"history":self.history,"error":self.error.as_deref().map(rendered_error),"busy":self.pending.is_some(),"cancelling":self.cancelling,"unknown_updates":self.unknown_updates})
    }

    fn prompt(
        &mut self,
        client: &ZzClient,
        text: String,
        images: Vec<AgentImage>,
        allow_queue: bool,
    ) -> Result<(), String> {
        if text.trim().is_empty() && images.is_empty() {
            return Err("Write a prompt or attach an image.".to_owned());
        }
        if !matches!(self.phase(), AgentConnectionPhase::Ready)
            && !(allow_queue
                && matches!(
                    self.phase(),
                    AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
                ))
        {
            return Err("The agent is not ready.".to_owned());
        }
        if self.pending.is_some() {
            return Err("Wait for the agent's current settings change.".to_owned());
        }
        if self.wire.queued_prompts as usize >= MAX_AGENT_QUEUED_PROMPTS {
            return Err("The prompt queue is full.".to_owned());
        }
        if !images.is_empty() && !self.capabilities.images {
            return Err("This agent does not accept images.".to_owned());
        }
        if images.len() > MAX_AGENT_PROMPT_IMAGES
            || images.iter().fold(text.len(), |total, image| {
                total.saturating_add(image.data.len())
            }) > MAX_AGENT_PROMPT_BYTES
        {
            return Err("The prompt is too large.".to_owned());
        }
        client
            .client
            .agent_prompt(
                self.pane,
                text.clone(),
                images
                    .iter()
                    .map(|image| zz_protocol::AgentImage {
                        format: image.format.clone(),
                        data: image.data.clone(),
                    })
                    .collect(),
            )
            .map_err(|error| error.to_string())?;
        if matches!(self.phase(), AgentConnectionPhase::Ready) {
            self.begin_prompt(text, images);
        }
        self.error = None;
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_new(
    client: *const ZzClient,
    pane: u64,
) -> *mut ZzAgentModel {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let core = lock(&client.core);
    let descriptor = core
        .snapshot()
        .sessions
        .iter()
        .flat_map(|session| &session.windows)
        .flat_map(|window| window.panes.values())
        .find_map(|candidate| {
            if candidate.id.0 != pane {
                return None;
            }
            match &candidate.kind {
                PaneKindSnapshot::Agent(descriptor) => Some(descriptor),
                _ => None,
            }
        });
    let Some(descriptor) = descriptor else {
        return std::ptr::null_mut();
    };
    let mut model = ZzAgentModel::new(
        PaneId(pane),
        descriptor.provider,
        descriptor.cwd.clone().unwrap_or_else(|| PathBuf::from("/")),
        client.client.client_instance_id(),
    );
    model.preferences = Arc::clone(&client.agent_preferences);
    if let Some(state) = core.agent_state(PaneId(pane)) {
        model.sync(state);
    }
    Box::into_raw(Box::new(model))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_free(model: *mut ZzAgentModel) {
    if !model.is_null() {
        drop(unsafe { Box::from_raw(model) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_apply_updates(
    model: *mut ZzAgentModel,
    client: *const ZzClient,
    batch: *const ZzAgentBatch,
) -> bool {
    let (Some(model), Some(client), Some(batch)) =
        (unsafe { (model.as_mut(), client.as_ref(), batch.as_ref()) })
    else {
        return false;
    };
    if model.batch(batch) {
        return true;
    }
    if !model.replaying {
        model.replaying = client
            .client
            .agent_replay(model.pane, model.next_seq)
            .is_ok();
    }
    false
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_replay(
    model: *mut ZzAgentModel,
    client: *const ZzClient,
) -> bool {
    let (Some(model), Some(client)) = (unsafe { (model.as_mut(), client.as_ref()) }) else {
        return false;
    };
    model.replaying = client
        .client
        .agent_replay(model.pane, model.next_seq)
        .is_ok();
    model.replaying
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_sync(model: *mut ZzAgentModel, state: *const ZzAgentState) {
    if let (Some(model), Some(state)) = unsafe { (model.as_mut(), state.as_ref()) } {
        model.sync(&state.wire);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_apply_sessions(
    model: *mut ZzAgentModel,
    reply: *const ZzAgentSessionsReply,
) {
    if let (Some(model), Some(reply)) = unsafe { (model.as_mut(), reply.as_ref()) }
        && model.pane.0 == reply.pane
        && let Ok(payload) = serde_json::from_str(&reply.result)
    {
        model.payload(payload);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_snapshot(
    model: *mut ZzAgentModel,
    since: u64,
) -> *mut ZzJson {
    unsafe { model.as_mut() }.map_or(std::ptr::null_mut(), |model| {
        Box::into_raw(Box::new(ZzJson::new(model.snapshot(since))))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_restored(model: *const ZzAgentModel) -> *mut ZzJson {
    unsafe { model.as_ref() }
        .filter(|model| !model.restored.is_empty())
        .map_or(std::ptr::null_mut(), |model| {
            Box::into_raw(Box::new(ZzJson::new(json!(model.restored))))
        })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_acknowledge_restored(
    model: *mut ZzAgentModel,
    client: *const ZzClient,
) {
    if let (Some(model), Some(client)) = unsafe { (model.as_mut(), client.as_ref()) } {
        model.restored.retain(|prompt| {
            prompt.reclaim_id != 0
                && client
                    .client
                    .agent_acknowledge_prompt_restore(model.pane, prompt.reclaim_id)
                    .is_err()
        });
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
enum Action {
    Prompt {
        text: String,
        images: Vec<AgentImage>,
    },
    Cancel,
    Unqueue,
    Permission {
        request_id: u64,
        option_id: Option<String>,
    },
    Configure {
        option: String,
        value: String,
    },
    Mode {
        value: String,
    },
    Authenticate {
        method: String,
    },
    ListSessions {
        cwd: Option<PathBuf>,
        next: bool,
    },
    NewSession {
        cwd: PathBuf,
    },
    LoadSession {
        session_id: String,
        cwd: PathBuf,
        directories: Vec<PathBuf>,
    },
    DeleteSession {
        session_id: String,
    },
    ClearError,
}

impl ZzAgentModel {
    fn send_setting(&mut self, client: &ZzClient, request: SettingRequest) -> Result<(), String> {
        let result = if request.option == LEGACY_MODE_PREFERENCE_ID {
            client
                .client
                .agent_set_mode(self.pane, request.value.clone())
        } else {
            client.client.agent_set_config_option(
                self.pane,
                request.option.clone(),
                request.value.clone(),
            )
        };
        result.map_err(|error| error.to_string())?;
        self.setting_request = Some(request);
        self.pending = Some(("setting".to_owned(), Instant::now()));
        self.error = None;
        Ok(())
    }

    fn finish_setting(&mut self, applied: bool, message: Option<String>) {
        let Some(request) = self.setting_request.take() else {
            return;
        };
        self.pending = None;
        if let Some(kind) = request.kind {
            if applied {
                self.preference_skips
                    .remove(&(kind, request.option.clone()));
                if request.user {
                    lock(&self.preferences).remember(
                        self.provider,
                        &self.agent_key,
                        kind,
                        &request.option,
                        &request.value,
                    );
                }
            } else {
                self.preference_skips.insert((kind, request.option.clone()));
            }
        }
        if !applied && request.user {
            self.error = Some(message.unwrap_or_else(|| {
                format!(
                    "The agent did not apply the selected value for `{}`.",
                    request.option
                )
            }));
        }
    }

    fn reconcile_preferences(&mut self, client: &ZzClient) {
        if self.agent_name.is_none()
            || self.replaying
            || self.session_reset
            || !matches!(self.phase(), AgentConnectionPhase::Ready)
            || self.pending.is_some()
        {
            return;
        }
        let setting = lock(&self.preferences).preferred(
            self.provider,
            &self.agent_key,
            &self.options,
            &self.modes,
            self.mode.as_deref(),
            &self.preference_skips,
        );
        if let Some(setting) = setting {
            let kind = setting.kind;
            let option = setting.config_id;
            let result = self.send_setting(
                client,
                SettingRequest {
                    kind: Some(kind),
                    option: option.clone(),
                    value: setting.value,
                    user: false,
                },
            );
            if result.is_err() {
                self.preference_skips.insert((kind, option));
            }
        }
    }

    fn action(&mut self, client: &ZzClient, action: Action) -> Result<(), String> {
        if matches!(
            action,
            Action::Configure { .. }
                | Action::Mode { .. }
                | Action::NewSession { .. }
                | Action::LoadSession { .. }
                | Action::DeleteSession { .. }
        ) && (!matches!(self.phase(), AgentConnectionPhase::Ready) || self.pending.is_some())
        {
            return Err("Wait for the agent to finish its current action.".to_owned());
        }
        let send =
            |result: Result<(), zz_daemon::DaemonError>| result.map_err(|error| error.to_string());
        let pane = self.pane;
        match action {
            Action::Prompt { text, images } => return self.prompt(client, text, images, true),
            Action::Cancel => {
                send(client.client.agent_cancel(pane))?;
                self.cancelling = true;
                return Ok(());
            }
            Action::Unqueue => return send(client.client.agent_unqueue(pane)),
            Action::Permission {
                request_id,
                option_id,
            } => {
                if !self.transcript.permissions().iter().any(|permission| {
                    permission.request_id == request_id
                        && option_id.as_ref().is_none_or(|id| {
                            permission.options.iter().any(|option| option.id == *id)
                        })
                }) {
                    return Err("This permission request is no longer available.".to_owned());
                }
                return send(
                    client
                        .client
                        .agent_respond_permission(pane, request_id, option_id),
                );
            }
            Action::ClearError => {
                self.error = None;
                return Ok(());
            }
            Action::Configure { option, value } => {
                if !self.options.iter().any(|candidate| {
                    candidate.id == option
                        && candidate.choices.iter().any(|choice| choice.value == value)
                }) {
                    return Err("This setting value is not available.".to_owned());
                }
                let kind = self
                    .options
                    .iter()
                    .find(|candidate| candidate.id == option)
                    .and_then(|option| preference_kind_for_category(option.category));
                return self.send_setting(
                    client,
                    SettingRequest {
                        kind,
                        option,
                        value,
                        user: true,
                    },
                );
            }
            Action::Mode { value } => {
                if !self.modes.iter().any(|mode| mode.id == value) {
                    return Err("This mode is not available.".to_owned());
                }
                return self.send_setting(
                    client,
                    SettingRequest {
                        kind: Some(AgentPreferenceKind::Permission),
                        option: LEGACY_MODE_PREFERENCE_ID.to_owned(),
                        value,
                        user: true,
                    },
                );
            }
            Action::Authenticate { method } => {
                if !self
                    .auth_methods
                    .iter()
                    .any(|candidate| candidate.id == method)
                {
                    return Err("This authentication method is not available.".to_owned());
                }
                send(client.client.agent_authenticate(pane, method))?;
            }
            Action::ListSessions { cwd, next } => {
                if !self.capabilities.list {
                    return Err("This agent does not support session history.".to_owned());
                }
                let cursor = if next {
                    self.history.next_cursor.clone()
                } else {
                    None
                };
                if next && cursor.is_none() {
                    return Ok(());
                }
                if cwd
                    .as_deref()
                    .is_some_and(|cwd| !valid_session_directory(cwd))
                {
                    return Err("Enter a valid working directory.".to_owned());
                }
                send(client.client.agent_session_op(
                    pane,
                    AgentSessionOpKind::List {
                        cwd,
                        cursor,
                        replace: !next,
                    },
                ))?;
                self.history.loading = true;
                self.history.error = None;
            }
            Action::LoadSession {
                session_id: _,
                cwd,
                directories,
            } if !valid_session_directory(&cwd)
                || !cwd.is_absolute()
                || directories.len() > MAX_AGENT_SESSION_DIRECTORIES
                || directories
                    .iter()
                    .any(|path| !valid_session_directory(path) || !path.is_absolute()) =>
            {
                return Err("Enter absolute working directories.".to_owned());
            }
            Action::NewSession { cwd } => {
                if !valid_session_directory(&cwd) || !cwd.is_absolute() {
                    return Err("Enter an absolute working directory.".to_owned());
                }
                send(
                    client
                        .client
                        .agent_session_op(pane, AgentSessionOpKind::New { cwd }),
                )?;
            }
            Action::LoadSession {
                session_id,
                cwd,
                directories,
            } => {
                if !self.capabilities.load || !valid_session_id(&session_id) {
                    return Err("This session cannot be loaded.".to_owned());
                }
                if !directories.is_empty() && !self.capabilities.additional_directories {
                    return Err("This agent does not support additional directories.".to_owned());
                }
                send(client.client.agent_session_op(
                    pane,
                    AgentSessionOpKind::Switch {
                        session_id,
                        cwd,
                        additional_directories: directories,
                    },
                ))?;
            }
            Action::DeleteSession { session_id } => {
                if !self.capabilities.delete || !valid_session_id(&session_id) {
                    return Err("This session cannot be deleted.".to_owned());
                }
                send(
                    client
                        .client
                        .agent_session_op(pane, AgentSessionOpKind::Delete { session_id }),
                )?;
                self.history.loading = true;
            }
        }
        self.pending = Some(("agent".to_owned(), Instant::now()));
        self.error = None;
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_action(
    model: *mut ZzAgentModel,
    client: *const ZzClient,
    json: *const c_char,
) -> bool {
    let (Some(model), Some(client)) = (unsafe { (model.as_mut(), client.as_ref()) }) else {
        return false;
    };
    if json.is_null() {
        return false;
    }
    let action = unsafe { CStr::from_ptr(json) }
        .to_str()
        .ok()
        .and_then(|json| serde_json::from_str(json).ok());
    let Some(action) = action else {
        model.error = Some("Invalid agent action.".to_owned());
        return false;
    };
    match model.action(client, action) {
        Ok(()) => true,
        Err(error) => {
            model.error = Some(error);
            false
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_gui_command(
    model: *mut ZzAgentModel,
    client: *const ZzClient,
    request_id: u64,
    json: *const c_char,
) -> *mut ZzJson {
    let (Some(model), Some(client)) = (unsafe { (model.as_mut(), client.as_ref()) }) else {
        return std::ptr::null_mut();
    };
    if json.is_null() {
        return std::ptr::null_mut();
    }
    let command = unsafe { CStr::from_ptr(json) }
        .to_str()
        .ok()
        .and_then(|json| serde_json::from_str::<zz_protocol::AgentCommand>(json).ok());
    let result = match command {
        Some(zz_protocol::AgentCommand::ComposerAppend { text }) => {
            return Box::into_raw(Box::new(ZzJson::new(
                json!({"text":text,"request_id":request_id}),
            )));
        }
        Some(zz_protocol::AgentCommand::Prompt { text }) => {
            model.prompt(client, text, Vec::new(), false)
        }
        None => Err("Invalid agent command.".to_owned()),
    };
    let response = match result {
        Ok(()) => GuiResponse::Success {
            request_id,
            output: String::new(),
        },
        Err(message) => GuiResponse::Error {
            request_id,
            message,
        },
    };
    let _ = client.client.send_gui_response(response);
    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_completions(
    model: *const ZzAgentModel,
    text: *const c_char,
    cursor: usize,
) -> *mut ZzJson {
    let Some(model) = (unsafe { model.as_ref() }) else {
        return std::ptr::null_mut();
    };
    if text.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return std::ptr::null_mut();
    };
    let completions = completion_query(text, cursor)
        .map(|query| ranked_completions(&model.commands, &query))
        .unwrap_or_default();
    let entries = completions.iter().map(|completion|json!({"command":completion.command,"start":completion.replacement.start,"end":completion.replacement.end,"insertion":completion.insertion()})).collect::<Vec<_>>();
    Box::into_raw(Box::new(ZzJson::new(
        json!({"entries":entries,"hint":active_command_hint(text,&model.commands)}),
    )))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_load_agent_preferences(
    client: *const ZzClient,
    path: *const c_char,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    let preferences = if path.is_null() {
        AgentPreferences::load_persistent()
    } else {
        let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
            return false;
        };
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return false;
        }
        AgentPreferences::load_path(path)
    };
    *lock(&client.agent_preferences) = preferences;
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_agent_model_reconcile_preferences(
    model: *mut ZzAgentModel,
    client: *const ZzClient,
) {
    if let (Some(model), Some(client)) = unsafe { (model.as_mut(), client.as_ref()) } {
        model.reconcile_preferences(client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> ZzAgentModel {
        ZzAgentModel::new(
            PaneId(7),
            AgentProvider::Codex,
            PathBuf::from("/tmp"),
            ClientInstanceId(42),
        )
    }

    fn batch(first_seq: u64, values: Vec<Value>) -> ZzAgentBatch {
        ZzAgentBatch {
            pane: 7,
            first_seq,
            items: values
                .into_iter()
                .enumerate()
                .map(|(index, update)| {
                    serde_json::to_vec(&AgentStreamItem {
                        seq: first_seq + index as u64,
                        payload: AgentStreamPayload::Update { update },
                    })
                    .unwrap()
                })
                .collect(),
        }
    }

    fn text(value: &str) -> Value {
        json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":value}})
    }

    #[test]
    fn reliable_ready_waits_for_ordered_turn_completion_before_the_next_optimistic_prompt() {
        let mut model = model();
        let ready = AgentPaneWire {
            phase: AgentConnectionPhase::Ready,
            ..AgentPaneWire::default()
        };
        model.sync(&ready);
        model.begin_prompt("first".to_owned(), Vec::new());
        model.sync(&AgentPaneWire {
            phase: AgentConnectionPhase::Running,
            ..ready.clone()
        });
        model.sync(&ready);
        assert_eq!(model.snapshot(0)["phase"], "running");
        let finish = || AgentStreamPayload::PromptFinished {
            turn_id: 1,
            outcome: AgentPromptOutcome::Finished {
                stop_reason: json!("end_turn"),
            },
        };
        model.payload(finish());
        assert_eq!(model.snapshot(0)["phase"], "ready");
        let echo = || json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hang"}});
        model.begin_prompt("hang".to_owned(), Vec::new());
        model.update(echo());
        let occurrences = |model: &mut ZzAgentModel| {
            model.snapshot(0)["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["entry"]["markdown"] == "hang")
                .count()
        };
        assert_eq!(occurrences(&mut model), 1);
        model.payload(finish());
        model.begin_prompt("hang".to_owned(), Vec::new());
        model.update(echo());
        assert_eq!(occurrences(&mut model), 2);
    }

    #[test]
    fn preferences_remember_only_the_acknowledged_user_selection() {
        let mut model = model();
        model.setting_request = Some(SettingRequest {
            kind: Some(AgentPreferenceKind::Permission),
            option: LEGACY_MODE_PREFERENCE_ID.to_owned(),
            value: "safe".to_owned(),
            user: true,
        });
        model.payload(AgentStreamPayload::ModeChanged {
            mode_id: "other".to_owned(),
        });
        assert!(
            lock(&model.preferences)
                .desired(
                    model.provider,
                    &model.agent_key,
                    AgentPreferenceKind::Permission,
                    LEGACY_MODE_PREFERENCE_ID
                )
                .is_none()
        );
        assert!(model.setting_request.is_some());
        model.payload(AgentStreamPayload::ModeChanged {
            mode_id: "safe".to_owned(),
        });
        assert_eq!(
            lock(&model.preferences).desired(
                model.provider,
                &model.agent_key,
                AgentPreferenceKind::Permission,
                LEGACY_MODE_PREFERENCE_ID
            ),
            Some("safe")
        );
        model.setting_request = Some(SettingRequest {
            kind: Some(AgentPreferenceKind::Permission),
            option: LEGACY_MODE_PREFERENCE_ID.to_owned(),
            value: "failed".to_owned(),
            user: true,
        });
        model.payload(AgentStreamPayload::SettingFailed {
            option_id: LEGACY_MODE_PREFERENCE_ID.to_owned(),
            message: "rejected".to_owned(),
        });
        assert_eq!(
            lock(&model.preferences).desired(
                model.provider,
                &model.agent_key,
                AgentPreferenceKind::Permission,
                LEGACY_MODE_PREFERENCE_ID
            ),
            Some("safe")
        );
        assert_eq!(model.error.as_deref(), Some("rejected"));
    }

    #[test]
    fn stream_replay_deduplicates_and_recovers_a_gap_without_losing_chunks() {
        let mut model = model();
        let first = batch(1, vec![text("first")]);
        assert!(model.batch(&first));
        let revision = model.transcript.revision();
        assert!(model.batch(&first));
        assert_eq!(model.transcript.revision(), revision);
        assert!(!model.batch(&batch(3, vec![text("third")])));
        assert_eq!(model.next_seq, 2);
        assert!(model.batch(&batch(2, vec![text("second"), text("third")])));
        let snapshot = model.snapshot(revision);
        assert_eq!(snapshot["entries"].as_array().unwrap().len(), 1);
        assert_eq!(
            snapshot["entries"][0]["entry"]["markdown"],
            "firstsecondthird"
        );
        assert_eq!(model.next_seq, 4);
    }

    #[test]
    fn session_reset_replaces_entries_even_without_a_new_transcript_revision() {
        let mut model = model();
        assert!(model.batch(&batch(1, vec![text("old session")])));
        let before = model.snapshot(0);
        model.payload(AgentStreamPayload::SessionReset { restoring: false });
        let reset = model.snapshot(before["revision"].as_u64().unwrap());
        assert_ne!(before["epoch"], reset["epoch"]);
        assert_eq!(reset["entry_count"], 0);
        assert!(model.batch(&batch(2, vec![text("new session")])));
        let next = model.snapshot(0);
        assert_eq!(next["entries"][0]["entry"]["markdown"], "new session");
    }

    #[test]
    fn restored_prompts_are_owned_deduplicated_and_retained_until_acknowledged() {
        let mut model = model();
        let prompt = |owner, text: &str| AgentPrompt {
            owner: ClientInstanceId(owner),
            text: text.to_owned(),
            images: Vec::new(),
        };
        model.restore(
            5,
            vec![
                prompt(99, "someone else"),
                prompt(42, "mine"),
                prompt(0, "legacy"),
            ],
        );
        model.restore(5, vec![prompt(42, "mine")]);
        assert_eq!(model.restored.len(), 1);
        assert_eq!(model.restored[0].text, "mine\nlegacy");
        model.snapshot(0);
        assert_eq!(model.restored.len(), 1);
    }

    #[test]
    fn unknown_updates_advance_the_cursor_and_do_not_discard_later_text() {
        let mut model = model();
        assert!(model.batch(&batch(
            1,
            vec![json!({"sessionUpdate":"future_update"}), text("still here")]
        )));
        assert_eq!(model.next_seq, 3);
        assert_eq!(model.unknown_updates, 1);
        assert_eq!(
            model.snapshot(0)["entries"][0]["entry"]["markdown"],
            "still here"
        );
    }

    #[test]
    fn journal_fallback_can_reset_at_a_new_sequence_during_replay() {
        let mut model = model();
        assert!(model.batch(&batch(1, vec![text("old")])));
        model.replaying = true;
        let reset = AgentStreamItem {
            seq: 100,
            payload: AgentStreamPayload::SessionReset { restoring: true },
        };
        assert!(model.batch(&ZzAgentBatch {
            pane: 7,
            first_seq: 100,
            items: vec![serde_json::to_vec(&reset).unwrap()]
        }));
        assert!(model.batch(&batch(101, vec![text("journal")])));
        assert_eq!(
            model.snapshot(0)["entries"][0]["entry"]["markdown"],
            "journal"
        );
        assert_eq!(model.next_seq, 102);
    }
}
