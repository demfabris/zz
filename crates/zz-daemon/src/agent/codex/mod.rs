pub(crate) mod translate;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use agent_client_protocol::schema::v1::{MessageId, ToolKind};
use async_channel::Sender;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};
use zz_protocol::{AgentAutoApprove, AgentProvider, AgentQuestionAnswer};

use crate::agent::{
    child::{
        ChildEvent, Input, Process, cancelled, fit_update, next_input, option, random_u64,
        rewind_count, rewind_shortfall,
    },
    environment::{AgentWorkspaceEnvironment, agent_path, find_executable},
    host::RuntimeChannels,
    journal::{AgentJournal, JournalEntry},
    runtime::{
        RuntimeCommand, RuntimeControl, StderrTail, prompt_blocks, prompt_updates,
        report_journal_error, tier_approves, validate_payload,
    },
    stream::{
        AgentPrompt, AgentPromptOutcome, AgentQuestion, AgentQuestionOption,
        AgentSessionCapabilities, AgentSessionSummary, AgentStreamPayload,
    },
};

use translate::Translator;

const REQUEST_TIMEOUT: Duration = Duration::from_mins(1);
const SESSION_PAGE: u64 = 50;
const AGENT_NAME: &str = "Codex";
const AGENT_KEY: &str = "codex";
const VERB_HELP: &str = "zz commands: `//btw <question>` (or `//side`) asks Codex in a throwaway copy of the thread, `//steer <text>` redirects the running turn, `//fork` continues in a copy of this conversation, and `//rewind [n]` continues from before your last n prompts (1 by default) without changing files. `/compact` and `/review` run Codex's own compaction and review.";
const SIDE_PREFIX: &str = "Answer this side question briefly from what you already know. Do not run commands, edit files, or call tools.\n\n";
const MODES: [(&str, &str, &str); 3] = [
    (
        "read-only",
        "Read only",
        "Ask before changing files or running commands",
    ),
    (
        "auto",
        "Auto",
        "Edit and run inside the workspace, ask beyond it",
    ),
    ("full-access", "Full access", "Run anything without asking"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodexCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
}

impl CodexCommand {
    pub(crate) fn parse(command: &str) -> Option<Self> {
        let mut env = Vec::new();
        let mut tokens = command.split_whitespace();
        let program = loop {
            let token = tokens.next()?;
            match token.split_once('=') {
                Some((name, value))
                    if !name.is_empty()
                        && name.chars().all(|character| {
                            character.is_ascii_alphanumeric() || character == '_'
                        }) =>
                {
                    env.push((name.to_owned(), value.to_owned()));
                }
                _ => break token,
            }
        };
        let name = Path::new(program).file_name()?.to_str()?;
        matches!(name, "codex" | "codex.exe").then(|| Self {
            program: program.to_owned(),
            args: tokens
                .map(str::to_owned)
                .filter(|arg| arg != "app-server")
                .collect(),
            env,
        })
    }
}

pub(crate) async fn run_codex_runtime(
    command: CodexCommand,
    workspace: AgentWorkspaceEnvironment,
    provider: AgentProvider,
    channels: RuntimeChannels,
) -> Result<(), String> {
    let (child_tx, child_rx) = async_channel::unbounded();
    let mut runtime = Runtime {
        provider,
        command,
        workspace,
        auto_approve: channels.auto_approve,
        permission_ids: channels.permission_ids,
        journal: channels.journal,
        events: channels.events,
        child_tx,
        process: None,
        generation: 0,
        stderr: StderrTail::default(),
        initialized: false,
        ready_sent: false,
        session: None,
        starting: None,
        translator: Translator::default(),
        requests: HashMap::new(),
        next_request: 0,
        permissions: HashMap::new(),
        turn: None,
        external_turn: None,
        last_turn: 0,
        deferred_cancels: HashSet::new(),
        settings: Settings::default(),
        notices: 0,
        sides: HashMap::new(),
    };
    let commands = channels.commands;
    let controls = channels.controls;
    loop {
        match next_input(&commands, &controls, &child_rx, runtime.next_deadline()).await {
            Input::Command(Ok(RuntimeCommand::Shutdown) | Err(_)) => {
                runtime.shutdown().await;
                return Ok(());
            }
            Input::Command(Ok(command)) => runtime.command(command).await?,
            Input::Control(Ok(control)) => runtime.control(control).await?,
            Input::Child(Ok(event)) => runtime.child(event).await?,
            Input::Control(Err(_)) | Input::Child(Err(_)) => {}
            Input::Deadline => runtime.expire().await?,
        }
    }
}

struct Session {
    id: String,
    cwd: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Start {
    Open,
    New,
    Switch,
    Fork,
}

struct Starting {
    start: Start,
    cwd: PathBuf,
    resume: Option<String>,
    before: Option<String>,
}

enum Outgoing {
    Initialize,
    Models,
    Thread {
        start: Start,
        cwd: PathBuf,
    },
    History {
        start: Start,
        thread: Value,
        cwd: PathBuf,
    },
    Turn {
        turn_id: u64,
    },
    Command {
        turn_id: u64,
    },
    Quiet,
    Steer,
    Rewind {
        count: usize,
    },
    SideFork {
        question: String,
        message_id: String,
    },
    SideTurn {
        thread: String,
    },
    Sessions {
        client: zz_protocol::ClientId,
        cwd: Option<PathBuf>,
        replace: bool,
    },
}

struct Turn {
    id: u64,
    codex: Option<String>,
}

enum Reply {
    Command,
    FileChange,
    Legacy,
    Permissions(Value),
    Questions(Vec<AgentQuestion>),
}

struct Pending {
    rpc_id: Value,
    reply: Reply,
}

#[derive(Default)]
struct Settings {
    models: Vec<Value>,
    model: Option<String>,
    effort: Option<String>,
    mode: Option<String>,
    model_override: bool,
    effort_override: bool,
    mode_override: bool,
}

impl Settings {
    fn current_model(&self) -> Option<&Value> {
        self.models
            .iter()
            .find(|model| model["model"].as_str() == self.model.as_deref())
            .or_else(|| self.models.iter().find(|model| model["isDefault"] == true))
    }

    fn mode_state(&self) -> Value {
        json!({
            "currentModeId": self.mode.as_deref().unwrap_or("auto"),
            "availableModes": MODES.iter().map(|(id, name, description)| json!({
                "id": id, "name": name, "description": description,
            })).collect::<Vec<_>>(),
        })
    }

    fn config_options(&self) -> Value {
        let mut options = Vec::new();
        if !self.models.is_empty() {
            options.push(json!({
                "id": "model",
                "name": "Model",
                "category": "model",
                "type": "select",
                "currentValue": self.model.as_deref().unwrap_or_default(),
                "options": self.models.iter().filter_map(|model| {
                    let value = model["model"].as_str()?;
                    Some(json!({
                        "value": value,
                        "name": model["displayName"].as_str().unwrap_or(value),
                        "description": model["description"].as_str(),
                    }))
                }).collect::<Vec<_>>(),
            }));
        }
        options.push(json!({
            "id": "mode",
            "name": "Mode",
            "category": "mode",
            "type": "select",
            "currentValue": self.mode.as_deref().unwrap_or("auto"),
            "options": MODES.iter().map(|(id, name, description)| json!({
                "value": id, "name": name, "description": description,
            })).collect::<Vec<_>>(),
        }));
        let efforts = self
            .current_model()
            .and_then(|model| model["supportedReasoningEfforts"].as_array())
            .filter(|efforts| !efforts.is_empty());
        if let Some(efforts) = efforts {
            let current = self
                .effort
                .clone()
                .or_else(|| {
                    self.current_model()
                        .and_then(|model| model["defaultReasoningEffort"].as_str())
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            options.push(json!({
                "id": "effort",
                "name": "Effort",
                "category": "thought_level",
                "type": "select",
                "currentValue": current,
                "options": efforts.iter().filter_map(|effort| {
                    let value = effort["reasoningEffort"].as_str()?;
                    let mut name = value.to_owned();
                    if let Some(first) = name.get_mut(0..1) {
                        first.make_ascii_uppercase();
                    }
                    Some(json!({ "value": value, "name": name, "description": effort["description"].as_str() }))
                }).collect::<Vec<_>>(),
            }));
        }
        Value::Array(options)
    }

    fn adopt(&mut self, response: &Value) {
        if self.model.is_none() || !self.model_override {
            self.model = response["model"].as_str().map(str::to_owned);
        }
        if !self.effort_override {
            self.effort = response["reasoningEffort"].as_str().map(str::to_owned);
        }
        if !self.mode_override {
            self.mode = Some(mode_of(&response["approvalPolicy"], &response["sandbox"]).to_owned());
        }
    }

    fn turn_overrides(&self, params: &mut Value) {
        if self.model_override
            && let Some(model) = &self.model
        {
            params["model"] = Value::from(model.as_str());
        }
        if self.effort_override
            && let Some(effort) = &self.effort
        {
            params["effort"] = Value::from(effort.as_str());
        }
        if self.mode_override {
            let (approval, sandbox) = match self.mode.as_deref() {
                Some("read-only") => (
                    json!("on-request"),
                    json!({ "type": "readOnly", "networkAccess": false }),
                ),
                Some("full-access") => (json!("never"), json!({ "type": "dangerFullAccess" })),
                _ => (
                    json!("on-request"),
                    json!({
                        "type": "workspaceWrite",
                        "writableRoots": [],
                        "networkAccess": false,
                        "excludeTmpdirEnvVar": false,
                        "excludeSlashTmp": false,
                    }),
                ),
            };
            params["approvalPolicy"] = approval;
            params["sandboxPolicy"] = sandbox;
        }
    }
}

fn mode_of(approval: &Value, sandbox: &Value) -> &'static str {
    match (approval.as_str(), sandbox["type"].as_str()) {
        (Some("never"), Some("dangerFullAccess")) => "full-access",
        (_, Some("readOnly")) => "read-only",
        _ => "auto",
    }
}

struct Runtime {
    provider: AgentProvider,
    command: CodexCommand,
    workspace: AgentWorkspaceEnvironment,
    auto_approve: Arc<Mutex<AgentAutoApprove>>,
    permission_ids: Arc<AtomicU64>,
    journal: Option<Arc<AgentJournal>>,
    events: Sender<AgentStreamPayload>,
    child_tx: Sender<ChildEvent>,
    process: Option<Process>,
    generation: u64,
    stderr: StderrTail,
    initialized: bool,
    ready_sent: bool,
    session: Option<Session>,
    starting: Option<Starting>,
    translator: Translator,
    requests: HashMap<u64, (Outgoing, Instant)>,
    next_request: u64,
    permissions: HashMap<u64, Pending>,
    turn: Option<Turn>,
    external_turn: Option<String>,
    last_turn: u64,
    deferred_cancels: HashSet<u64>,
    settings: Settings,
    notices: u64,
    sides: HashMap<String, Side>,
}

struct Side {
    message_id: String,
    text: String,
    streamed: bool,
}

impl Runtime {
    async fn emit(&self, payload: AgentStreamPayload) -> Result<(), String> {
        if let Err(error) = validate_payload(&payload) {
            log::warn!(target: "zz::agent", "dropping an oversized Codex item: {error}");
            return Ok(());
        }
        self.events
            .send(payload)
            .await
            .map_err(|error| format!("agent stream is unavailable: {error}"))
    }

    async fn update(&self, update: Value, record: bool) -> Result<(), String> {
        let update = fit_update(update);
        if record
            && let (Some(journal), Some(session)) = (self.journal.as_deref(), &self.session)
            && let Err(error) = journal.append_for(self.provider, &session.id, &update)
        {
            report_journal_error(&session.id, &error.to_string());
        }
        self.emit(AgentStreamPayload::Update { update }).await
    }

    async fn notice(&mut self, text: &str) -> Result<(), String> {
        self.notices += 1;
        let id = format!(
            "zz-notice-{}-{:08x}",
            self.notices,
            random_u64() & 0xffff_ffff
        );
        self.update(
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": id,
                "content": { "type": "text", "text": text },
                "_meta": { "zz": { "notice": true } },
            }),
            true,
        )
        .await
    }

    fn send(&self, message: &Value) {
        if let Some(process) = &self.process {
            process.send(message);
        }
    }

    fn request(&mut self, method: &str, params: &Value, outgoing: Outgoing) {
        self.next_request += 1;
        let id = self.next_request;
        self.send(&json!({ "method": method, "id": id, "params": params }));
        self.requests
            .insert(id, (outgoing, Instant::now() + REQUEST_TIMEOUT));
    }

    fn reply(&self, rpc_id: &Value, result: &Value) {
        self.send(&json!({ "id": rpc_id, "result": result }));
    }

    fn reply_error(&self, rpc_id: &Value, message: &str) {
        self.send(&json!({ "id": rpc_id, "error": { "code": -32601, "message": message } }));
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.requests.values().map(|(_, deadline)| *deadline).min()
    }

    fn thread_id(&self) -> Option<String> {
        self.session.as_ref().map(|session| session.id.clone())
    }

    fn cwd(&self) -> PathBuf {
        self.session
            .as_ref()
            .map(|session| session.cwd.clone())
            .unwrap_or_default()
    }

    async fn command(&mut self, command: RuntimeCommand) -> Result<(), String> {
        match command {
            RuntimeCommand::Open {
                cwd,
                resume_session,
            } => self.begin(cwd, Start::Open, resume_session).await,
            RuntimeCommand::NewSession { cwd } => self.begin(cwd, Start::New, None).await,
            RuntimeCommand::SwitchSession { session } => {
                self.begin(session.cwd, Start::Switch, Some(session.session_id))
                    .await
            }
            RuntimeCommand::ListSessions {
                client,
                cwd,
                cursor,
                replace,
            } => {
                let mut params = json!({ "limit": SESSION_PAGE });
                if let Some(cwd) = &cwd {
                    params["cwd"] = Value::from(cwd.display().to_string());
                }
                if let Some(cursor) = cursor {
                    params["cursor"] = Value::from(cursor);
                }
                self.request(
                    "thread/list",
                    &params,
                    Outgoing::Sessions {
                        client,
                        cwd,
                        replace,
                    },
                );
                Ok(())
            }
            RuntimeCommand::DeleteSession { client, .. } => {
                self.emit(AgentStreamPayload::SessionDeleteFailed {
                    client,
                    message: "Codex threads are not deleted from zz".to_owned(),
                })
                .await
            }
            RuntimeCommand::Prompt { turn_id, prompt } => self.prompt(turn_id, prompt).await,
            RuntimeCommand::Verb { prompt } => self.verb(&prompt.text).await,
            RuntimeCommand::Authenticate { .. } => {
                self.emit(AgentStreamPayload::AuthenticationFailed {
                    message: "Codex signs in with `codex login` in a terminal".to_owned(),
                })
                .await
            }
            RuntimeCommand::SetConfigOption { option_id, value } => {
                match option_id.as_str() {
                    "model" => {
                        self.settings.model = Some(value.clone());
                        self.settings.model_override = true;
                        let supported = self
                            .settings
                            .current_model()
                            .and_then(|model| model["supportedReasoningEfforts"].as_array())
                            .is_some_and(|efforts| {
                                efforts.iter().any(|effort| {
                                    effort["reasoningEffort"].as_str()
                                        == self.settings.effort.as_deref()
                                })
                            });
                        if !supported {
                            self.settings.effort = None;
                            self.settings.effort_override = false;
                        }
                    }
                    "effort" => {
                        self.settings.effort = Some(value.clone());
                        self.settings.effort_override = true;
                    }
                    "mode" => {
                        self.settings.mode = Some(value.clone());
                        self.settings.mode_override = true;
                    }
                    _ => {
                        return self
                            .emit(AgentStreamPayload::SettingFailed {
                                option_id,
                                message: "Codex has no such setting".to_owned(),
                            })
                            .await;
                    }
                }
                self.emit(AgentStreamPayload::ConfigOptionsChanged {
                    option_id,
                    value,
                    config_options: self.settings.config_options(),
                })
                .await?;
                self.publish_settings().await
            }
            RuntimeCommand::SetMode { mode_id } => {
                self.settings.mode = Some(mode_id.clone());
                self.settings.mode_override = true;
                self.emit(AgentStreamPayload::ModeChanged { mode_id })
                    .await?;
                self.publish_settings().await
            }
            RuntimeCommand::StopTask { .. } | RuntimeCommand::Shutdown => Ok(()),
        }
    }

    async fn control(&mut self, control: RuntimeControl) -> Result<(), String> {
        match control {
            RuntimeControl::Cancel { turn_id: 0, .. } if self.turn.is_none() => {
                if let (Some(thread), Some(codex)) = (self.thread_id(), self.external_turn.clone())
                {
                    self.request(
                        "turn/interrupt",
                        &json!({ "threadId": thread, "turnId": codex }),
                        Outgoing::Quiet,
                    );
                }
                self.cancel_permissions().await
            }
            RuntimeControl::Cancel { turn_id, .. } => {
                if self.turn.as_ref().is_some_and(|turn| turn.id == turn_id) {
                    let turn = self.turn.take();
                    if let (Some(thread), Some(codex)) =
                        (self.thread_id(), turn.and_then(|turn| turn.codex))
                    {
                        self.request(
                            "turn/interrupt",
                            &json!({ "threadId": thread, "turnId": codex }),
                            Outgoing::Quiet,
                        );
                    }
                    self.cancel_permissions().await?;
                    self.emit(AgentStreamPayload::PromptFinished {
                        turn_id,
                        outcome: cancelled(),
                    })
                    .await
                } else {
                    if turn_id > self.last_turn {
                        self.deferred_cancels.insert(turn_id);
                    }
                    Ok(())
                }
            }
            RuntimeControl::RespondPermission {
                request_id,
                option_id,
            } => self.answer(request_id, option_id.as_deref()).await,
            RuntimeControl::AnswerQuestion {
                request_id,
                answers,
            } => self.answer_questions(request_id, &answers).await,
        }
    }

    async fn begin(
        &mut self,
        cwd: PathBuf,
        start: Start,
        resume: Option<String>,
    ) -> Result<(), String> {
        self.begin_with(Starting {
            start,
            cwd,
            resume,
            before: None,
        })
        .await
    }

    async fn begin_with(&mut self, starting: Starting) -> Result<(), String> {
        let start = starting.start;
        if start == Start::Fork && starting.resume.is_none() {
            return self
                .notice("Nothing to fork yet: send a prompt first.")
                .await;
        }
        self.cancel_permissions().await?;
        if let Some(turn) = self.turn.take() {
            self.emit(AgentStreamPayload::PromptFinished {
                turn_id: turn.id,
                outcome: cancelled(),
            })
            .await?;
        }
        if self.process.is_none()
            && let Err(message) = self.spawn()
        {
            return self.fail_start(start, message).await;
        }
        self.starting = Some(starting);
        if self.initialized {
            self.open_thread();
        }
        Ok(())
    }

    fn spawn(&mut self) -> Result<(), String> {
        let program = find_executable(&self.command.program).ok_or_else(|| {
            format!(
                "Codex is not installed: `{}` was not found on PATH. Install it from https://developers.openai.com/codex, or point agent-command at it.",
                self.command.program
            )
        })?;
        let mut command = Command::new(program);
        command.arg("app-server").args(&self.command.args);
        if let Some(path) = agent_path() {
            command.env("PATH", path);
        }
        for (name, value) in self.workspace.entries() {
            command.env(name, value);
        }
        for (name, value) in &self.command.env {
            command.env(name, value);
        }
        self.generation += 1;
        self.process = Some(Process::launch(
            &mut command,
            AGENT_NAME,
            self.generation,
            &self.child_tx,
            &self.stderr,
        )?);
        self.initialized = false;
        self.requests.clear();
        self.request(
            "initialize",
            &json!({
                "clientInfo": { "name": "zz", "title": "zz", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true, "requestAttestation": false },
            }),
            Outgoing::Initialize,
        );
        Ok(())
    }

    fn open_thread(&mut self) {
        let Some(starting) = &self.starting else {
            return;
        };
        let cwd = starting.cwd.clone();
        let start = starting.start;
        let cwd_text = cwd.display().to_string();
        let before = starting.before.clone();
        match (start, starting.resume.clone()) {
            (Start::Fork, Some(thread)) => {
                let mut params = json!({ "threadId": thread, "cwd": cwd_text });
                if let Some(before) = before {
                    params["beforeTurnId"] = Value::from(before);
                }
                self.request("thread/fork", &params, Outgoing::Thread { start, cwd });
            }
            (_, Some(thread)) => self.request(
                "thread/resume",
                &json!({ "threadId": thread, "cwd": cwd_text }),
                Outgoing::Thread { start, cwd },
            ),
            (_, None) => self.request(
                "thread/start",
                &json!({ "cwd": cwd_text }),
                Outgoing::Thread { start, cwd },
            ),
        }
    }

    async fn fail_start(&mut self, start: Start, message: String) -> Result<(), String> {
        self.starting = None;
        match start {
            Start::Open => {
                self.process = None;
                self.emit(AgentStreamPayload::PaneFailed { message }).await
            }
            Start::New | Start::Switch | Start::Fork => {
                self.emit(AgentStreamPayload::SessionSwitchFailed { message })
                    .await
            }
        }
    }

    async fn ready(&mut self) -> Result<(), String> {
        if self.ready_sent {
            return Ok(());
        }
        self.ready_sent = true;
        self.emit(AgentStreamPayload::Ready {
            agent_name: AGENT_NAME.to_owned(),
            agent_key: AGENT_KEY.to_owned(),
            auth_methods: Vec::new(),
            capabilities: AgentSessionCapabilities {
                load: true,
                list: true,
                close: false,
                delete: false,
                additional_directories: false,
                images: true,
                verbs: true,
            },
        })
        .await
    }

    async fn threaded(
        &mut self,
        start: Start,
        cwd: PathBuf,
        response: Value,
    ) -> Result<(), String> {
        let Some(thread) = response["thread"]["id"].as_str().map(str::to_owned) else {
            return self
                .fail_start(start, "Codex returned no thread".to_owned())
                .await;
        };
        self.settings.adopt(&response);
        if start == Start::Open
            && self
                .starting
                .as_ref()
                .and_then(|starting| starting.resume.as_deref())
                == Some(thread.as_str())
            || matches!(start, Start::Switch | Start::Fork)
        {
            self.request(
                "thread/read",
                &json!({ "threadId": thread, "includeTurns": true }),
                Outgoing::History {
                    start,
                    thread: response["thread"].clone(),
                    cwd,
                },
            );
            return Ok(());
        }
        self.settle(start, thread, cwd, Vec::new()).await
    }

    async fn settle(
        &mut self,
        start: Start,
        thread: String,
        cwd: PathBuf,
        history: Vec<(Value, bool)>,
    ) -> Result<(), String> {
        let (source, before) = self
            .starting
            .take()
            .map_or((None, None), |starting| (starting.resume, starting.before));
        self.ready().await?;
        self.translator = Translator::new(cwd.clone());
        self.session = Some(Session {
            id: thread.clone(),
            cwd: cwd.clone(),
        });
        let restoring = match start {
            Start::Open => !history.is_empty(),
            Start::New | Start::Switch | Start::Fork => true,
        };
        self.emit(AgentStreamPayload::SessionReset { restoring })
            .await?;
        for (update, record) in history {
            self.update(update, record).await?;
        }
        let modes = Some(self.settings.mode_state());
        let config_options = Some(self.settings.config_options());
        match start {
            Start::Open => {
                self.emit(AgentStreamPayload::SessionReady {
                    session_id: thread,
                    modes,
                    config_options,
                })
                .await?;
            }
            Start::New | Start::Switch | Start::Fork => {
                self.emit(AgentStreamPayload::SessionSwitched {
                    session_id: thread,
                    cwd,
                    modes,
                    config_options,
                    replay: Vec::new(),
                })
                .await?;
            }
        }
        self.update(
            json!({
                "sessionUpdate": "available_commands_update",
                "availableCommands": [
                    { "name": "compact", "description": "Summarize the conversation to free context" },
                    { "name": "review", "description": "Review the uncommitted changes" },
                ],
            }),
            true,
        )
        .await?;
        if let (Start::Fork, Some(source)) = (start, source) {
            let text = if before.is_some() {
                format!(
                    "Went back to an earlier prompt. The full conversation stays in thread `{source}`, and files on disk are unchanged."
                )
            } else {
                format!("Forked from thread `{source}`. The original is unchanged.")
            };
            self.notice(&text).await?;
        }
        Ok(())
    }

    async fn rewind(&mut self, count: usize, thread: &Value) -> Result<(), String> {
        if self.turn.is_some() {
            return self.notice("Stop the turn before rewinding.").await;
        }
        let Some(source) = thread["id"].as_str().map(str::to_owned) else {
            return Ok(());
        };
        let mut prompts = thread["turns"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, turn)| {
                turn["items"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|item| item["type"] == "userMessage"))
            })
            .filter_map(|(index, turn)| Some((index, turn["id"].as_str()?.to_owned())))
            .collect::<Vec<_>>();
        let Some(index) = prompts.len().checked_sub(count) else {
            return self.notice(&rewind_shortfall(prompts.len())).await;
        };
        let cwd = self.cwd();
        match prompts.swap_remove(index) {
            (0, _) => self.begin(cwd, Start::New, None).await,
            (_, before) => {
                self.begin_with(Starting {
                    start: Start::Fork,
                    cwd,
                    resume: Some(source),
                    before: Some(before),
                })
                .await
            }
        }
    }

    fn history(&mut self, thread: &Value) -> Vec<(Value, bool)> {
        let id = thread["id"].as_str().unwrap_or_default().to_owned();
        let mut translator = Translator::new(self.cwd());
        let updates = thread["turns"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|turn| turn["items"].as_array().cloned().unwrap_or_default())
            .flat_map(|item| translator.history_item(&item))
            .collect::<Vec<_>>();
        if updates.is_empty()
            && let Some(journal) = self.journal.as_deref()
        {
            return match journal.replay_for(self.provider, &id) {
                Ok(records) => records
                    .into_iter()
                    .map(|(_, JournalEntry::Update(update))| (update, false))
                    .collect(),
                Err(error) => {
                    report_journal_error(&id, &error.to_string());
                    Vec::new()
                }
            };
        }
        if let Some(journal) = self.journal.as_deref()
            && let Err(error) = journal.remove_for(self.provider, &id)
        {
            report_journal_error(&id, &error.to_string());
        }
        updates.into_iter().map(|update| (update, true)).collect()
    }

    async fn prompt(&mut self, turn_id: u64, prompt: AgentPrompt) -> Result<(), String> {
        self.last_turn = self.last_turn.max(turn_id);
        if self.deferred_cancels.remove(&turn_id) {
            self.emit(AgentStreamPayload::PromptAccepted { turn_id })
                .await?;
            return self.finish(turn_id, cancelled()).await;
        }
        let Some(thread) = self.thread_id() else {
            return self
                .emit(AgentStreamPayload::PaneFailed {
                    message: "agent session is not ready".to_owned(),
                })
                .await;
        };
        let echo = format!("zz-prompt-{turn_id}-{:016x}", random_u64());
        let text = prompt.text.clone();
        let input = turn_input(&prompt);
        for update in prompt_updates(&prompt_blocks(prompt), &MessageId::new(echo)) {
            let update = serde_json::to_value(&update).map_err(|error| error.to_string())?;
            self.update(update, true).await?;
        }
        self.turn = Some(Turn {
            id: turn_id,
            codex: None,
        });
        self.emit(AgentStreamPayload::PromptAccepted { turn_id })
            .await?;
        match text.trim() {
            "/compact" => {
                self.request(
                    "thread/compact/start",
                    &json!({ "threadId": thread }),
                    Outgoing::Command { turn_id },
                );
            }
            "/review" => {
                self.request(
                    "review/start",
                    &json!({
                        "threadId": thread,
                        "target": { "type": "uncommittedChanges" },
                        "delivery": "inline",
                    }),
                    Outgoing::Turn { turn_id },
                );
            }
            _ => {
                self.translator.expect_prompt(&text);
                let mut params = json!({ "threadId": thread, "input": input });
                self.settings.turn_overrides(&mut params);
                self.request("turn/start", &params, Outgoing::Turn { turn_id });
            }
        }
        Ok(())
    }

    async fn verb(&mut self, text: &str) -> Result<(), String> {
        let line = text.trim_start().strip_prefix("//").unwrap_or(text).trim();
        let (name, rest) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(name, rest)| (name, rest.trim()));
        self.notices += 1;
        self.update(
            json!({
                "sessionUpdate": "user_message_chunk",
                "messageId": format!("zz-command-{}-{:08x}", self.notices, random_u64() & 0xffff_ffff),
                "content": { "type": "text", "text": text.trim() },
            }),
            true,
        )
        .await?;
        let Some(thread) = self.thread_id() else {
            return self.notice("Codex is not running.").await;
        };
        match name {
            "steer" if !rest.is_empty() => {
                let Some(codex) = self.turn.as_ref().and_then(|turn| turn.codex.clone()) else {
                    return self.notice("Nothing is running to steer.").await;
                };
                self.request(
                    "turn/steer",
                    &json!({
                        "threadId": thread,
                        "expectedTurnId": codex,
                        "input": [{ "type": "text", "text": rest, "text_elements": [] }],
                    }),
                    Outgoing::Steer,
                );
                Ok(())
            }
            "fork" => {
                if self.turn.is_some() {
                    return self.notice("Stop the turn before forking.").await;
                }
                let cwd = self.cwd();
                self.begin(cwd, Start::Fork, Some(thread)).await
            }
            "rewind" => {
                let Some(count) = rewind_count(rest) else {
                    return self.notice(VERB_HELP).await;
                };
                if self.turn.is_some() {
                    return self.notice("Stop the turn before rewinding.").await;
                }
                self.request(
                    "thread/read",
                    &json!({ "threadId": thread, "includeTurns": true }),
                    Outgoing::Rewind { count },
                );
                Ok(())
            }
            "btw" | "side" if !rest.is_empty() => {
                self.notices += 1;
                let message_id = format!(
                    "zz-side-{}-{:08x}",
                    self.notices,
                    random_u64() & 0xffff_ffff
                );
                self.request(
                    "thread/fork",
                    &json!({ "threadId": thread, "ephemeral": true, "excludeTurns": true }),
                    Outgoing::SideFork {
                        question: rest.to_owned(),
                        message_id,
                    },
                );
                Ok(())
            }
            _ => self.notice(VERB_HELP).await,
        }
    }

    async fn publish_settings(&self) -> Result<(), String> {
        self.update(
            json!({
                "sessionUpdate": "current_mode_update",
                "currentModeId": self.settings.mode.as_deref().unwrap_or("auto"),
            }),
            true,
        )
        .await?;
        self.update(
            json!({ "sessionUpdate": "config_option_update", "configOptions": self.settings.config_options() }),
            true,
        )
        .await
    }

    async fn finish(&self, turn_id: u64, outcome: AgentPromptOutcome) -> Result<(), String> {
        self.emit(AgentStreamPayload::PromptFinished { turn_id, outcome })
            .await
    }

    async fn shutdown(&mut self) {
        let _ = self.cancel_permissions().await;
        self.process = None;
    }

    async fn expire(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let expired = self
            .requests
            .iter()
            .filter(|(_, (_, deadline))| *deadline <= now)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in expired {
            if let Some((outgoing, _)) = self.requests.remove(&id) {
                self.settled(outgoing, Err("Codex did not answer in time".to_owned()))
                    .await?;
            }
        }
        Ok(())
    }

    async fn settled(
        &mut self,
        outgoing: Outgoing,
        result: Result<Value, String>,
    ) -> Result<(), String> {
        match (outgoing, result) {
            (Outgoing::Initialize, Ok(_)) => {
                self.initialized = true;
                self.send(&json!({ "method": "initialized" }));
                self.request("model/list", &json!({}), Outgoing::Models);
                self.open_thread();
                Ok(())
            }
            (Outgoing::Initialize, Err(error)) => {
                let start = self
                    .starting
                    .as_ref()
                    .map_or(Start::Open, |starting| starting.start);
                let tail = self.stderr.snapshot();
                let message = tail.map_or_else(
                    || format!("Codex did not start: {error}"),
                    |tail| format!("Codex did not start: {error}\n{tail}"),
                );
                self.process = None;
                self.fail_start(start, message).await
            }
            (Outgoing::Models, Ok(response)) => {
                self.settings.models = response["data"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|model| model["hidden"] != true)
                    .cloned()
                    .collect();
                if self.session.is_some() {
                    self.publish_settings().await?;
                }
                Ok(())
            }
            (Outgoing::Models, Err(_))
            | (Outgoing::Quiet, _)
            | (Outgoing::Steer | Outgoing::SideTurn { .. }, Ok(_)) => Ok(()),
            (Outgoing::Thread { start, cwd }, Ok(response)) => {
                self.threaded(start, cwd, response).await
            }
            (Outgoing::Thread { start, cwd }, Err(error)) => {
                if start == Start::Open
                    && let Some(starting) = &mut self.starting
                    && starting.resume.take().is_some()
                {
                    log::warn!(target: "zz::agent", "could not resume the Codex thread: {error}");
                    self.request(
                        "thread/start",
                        &json!({ "cwd": cwd.display().to_string() }),
                        Outgoing::Thread { start, cwd },
                    );
                    return Ok(());
                }
                self.fail_start(start, format!("Codex could not open the thread: {error}"))
                    .await
            }
            (Outgoing::History { start, thread, cwd }, result) => {
                let id = thread["id"].as_str().unwrap_or_default().to_owned();
                let full = result.map_or(thread, |response| response["thread"].clone());
                let previous = self.session.take();
                self.session = Some(Session {
                    id: id.clone(),
                    cwd: cwd.clone(),
                });
                let history = self.history(&full);
                self.session = previous;
                self.settle(start, id, cwd, history).await
            }
            (Outgoing::Turn { turn_id }, Ok(response)) => {
                if let Some(turn) = self.turn.as_mut().filter(|turn| turn.id == turn_id) {
                    turn.codex = response["turn"]["id"].as_str().map(str::to_owned);
                }
                Ok(())
            }
            (Outgoing::Turn { turn_id } | Outgoing::Command { turn_id }, Err(error)) => {
                if self.turn.as_ref().is_some_and(|turn| turn.id == turn_id) {
                    self.turn = None;
                    self.finish(turn_id, AgentPromptOutcome::Failed { message: error })
                        .await?;
                }
                Ok(())
            }
            (Outgoing::Command { turn_id }, Ok(_)) => {
                if self.turn.as_ref().is_some_and(|turn| turn.id == turn_id) {
                    self.turn = None;
                    self.finish(
                        turn_id,
                        AgentPromptOutcome::Finished {
                            stop_reason: Value::from("end_turn"),
                        },
                    )
                    .await?;
                }
                Ok(())
            }
            (
                Outgoing::SideFork {
                    question,
                    message_id,
                },
                Ok(response),
            ) => {
                let Some(side) = response["thread"]["id"].as_str().map(str::to_owned) else {
                    return self.notice("Codex did not open the side thread.").await;
                };
                self.sides.insert(
                    side.clone(),
                    Side {
                        message_id,
                        text: String::new(),
                        streamed: false,
                    },
                );
                self.request(
                    "turn/start",
                    &json!({
                        "threadId": side,
                        "input": [{ "type": "text", "text": format!("{SIDE_PREFIX}{question}"), "text_elements": [] }],
                        "approvalPolicy": "never",
                        "sandboxPolicy": { "type": "readOnly", "networkAccess": false },
                    }),
                    Outgoing::SideTurn { thread: side },
                );
                Ok(())
            }
            (Outgoing::SideFork { .. }, Err(error)) => {
                self.notice(&format!("The side question failed: {error}"))
                    .await
            }
            (Outgoing::SideTurn { thread }, Err(error)) => {
                self.sides.remove(&thread);
                self.notice(&format!("The side question failed: {error}"))
                    .await
            }
            (Outgoing::Rewind { count }, Ok(response)) => {
                self.rewind(count, &response["thread"]).await
            }
            (Outgoing::Rewind { .. }, Err(error)) => {
                self.notice(&format!("Codex could not read the thread: {error}"))
                    .await
            }
            (Outgoing::Steer, Err(error)) => {
                self.notice(&format!("Codex did not take the steer: {error}"))
                    .await
            }
            (
                Outgoing::Sessions {
                    client,
                    cwd,
                    replace,
                },
                Ok(response),
            ) => {
                let sessions = response["data"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|thread| {
                        Some(AgentSessionSummary {
                            session_id: thread["id"].as_str()?.to_owned(),
                            cwd: PathBuf::from(thread["cwd"].as_str()?),
                            additional_directories: Vec::new(),
                            title: thread["name"]
                                .as_str()
                                .or_else(|| thread["preview"].as_str())
                                .filter(|title| !title.is_empty())
                                .map(|title| title.chars().take(200).collect()),
                            updated_at: thread["updatedAt"].as_u64().and_then(|seconds| {
                                crate::agent::claude::sessions::rfc3339(u128::from(seconds) * 1000)
                            }),
                        })
                    })
                    .collect();
                self.emit(AgentStreamPayload::SessionsListed {
                    client,
                    sessions,
                    next_cursor: response["nextCursor"].as_str().map(str::to_owned),
                    cwd_filter: cwd,
                    replace,
                })
                .await
            }
            (Outgoing::Sessions { client, .. }, Err(error)) => {
                self.emit(AgentStreamPayload::SessionListFailed {
                    client,
                    message: format!("could not list Codex threads: {error}"),
                })
                .await
            }
        }
    }

    async fn child(&mut self, event: ChildEvent) -> Result<(), String> {
        match event {
            ChildEvent::Line(generation, _) | ChildEvent::Closed(generation)
                if generation != self.generation =>
            {
                Ok(())
            }
            ChildEvent::Line(_, line) => {
                let Ok(message) = serde_json::from_slice::<Value>(&line) else {
                    return Ok(());
                };
                self.message(message).await
            }
            ChildEvent::Closed(_) => self.closed().await,
        }
    }

    async fn closed(&mut self) -> Result<(), String> {
        let detail = self.process.as_mut().and_then(Process::exit_detail);
        self.process = None;
        self.initialized = false;
        let mut message = detail.map_or_else(
            || "Codex exited".to_owned(),
            |status| format!("Codex exited ({status})"),
        );
        if let Some(tail) = self.stderr.snapshot() {
            message.push('\n');
            message.push_str(&tail);
        }
        self.permissions.clear();
        self.requests.clear();
        if let Some(starting) = self.starting.take() {
            return self.fail_start(starting.start, message).await;
        }
        if let Some(turn) = self.turn.take() {
            self.finish(
                turn.id,
                AgentPromptOutcome::Failed {
                    message: message.clone(),
                },
            )
            .await?;
        }
        self.emit(AgentStreamPayload::PaneFailed { message }).await
    }

    async fn message(&mut self, message: Value) -> Result<(), String> {
        let method = message["method"].as_str().map(str::to_owned);
        match (method, message.get("id")) {
            (Some(method), Some(rpc_id)) => {
                let rpc_id = rpc_id.clone();
                self.server_request(&method, rpc_id, &message["params"])
                    .await
            }
            (Some(method), None) => self.notification(&method, &message["params"]).await,
            (None, Some(id)) => {
                let Some((outgoing, _)) = id.as_u64().and_then(|id| self.requests.remove(&id))
                else {
                    return Ok(());
                };
                let result = match message.get("error") {
                    Some(error) => Err(error["message"]
                        .as_str()
                        .unwrap_or("request failed")
                        .to_owned()),
                    None => Ok(message["result"].clone()),
                };
                self.settled(outgoing, result).await
            }
            (None, None) => Ok(()),
        }
    }

    async fn notification(&mut self, method: &str, params: &Value) -> Result<(), String> {
        if let Some(thread) = params["threadId"].as_str()
            && self.sides.contains_key(thread)
        {
            return self
                .side_notification(thread.to_owned(), method, params)
                .await;
        }
        let current = self.thread_id();
        if let Some(thread) = params["threadId"].as_str()
            && Some(thread) != current.as_deref()
        {
            for update in self
                .translator
                .child(thread, method, params)
                .unwrap_or_default()
            {
                self.update(update, true).await?;
            }
            return Ok(());
        }
        match method {
            "turn/started" => match self.turn.as_mut() {
                Some(turn) => {
                    if turn.codex.is_none() {
                        turn.codex = params["turn"]["id"].as_str().map(str::to_owned);
                    }
                }
                None => {
                    self.external_turn = params["turn"]["id"].as_str().map(str::to_owned);
                }
            },
            "thread/status/changed" if self.turn.is_none() => {
                let busy = params["status"]["type"] == "active";
                if !busy {
                    self.external_turn = None;
                }
                self.emit(AgentStreamPayload::Activity { busy }).await?;
            }
            "turn/completed" => {
                let codex = params["turn"]["id"].as_str();
                let matches = self
                    .turn
                    .as_ref()
                    .is_some_and(|turn| turn.codex.is_none() || turn.codex.as_deref() == codex);
                if matches && let Some(turn) = self.turn.take() {
                    let outcome = match params["turn"]["status"].as_str() {
                        Some("interrupted") => cancelled(),
                        Some("failed") => AgentPromptOutcome::Failed {
                            message: params["turn"]["error"]["message"]
                                .as_str()
                                .unwrap_or("Codex turn failed")
                                .to_owned(),
                        },
                        _ => AgentPromptOutcome::Finished {
                            stop_reason: Value::from("end_turn"),
                        },
                    };
                    self.finish(turn.id, outcome).await?;
                }
            }
            "serverRequest/resolved" => {
                let request = params["requestId"].clone();
                let resolved = self
                    .permissions
                    .iter()
                    .filter(|(_, pending)| pending.rpc_id == request)
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>();
                for request_id in resolved {
                    self.permissions.remove(&request_id);
                    self.emit(AgentStreamPayload::PermissionResolved {
                        request_id,
                        canceled: false,
                    })
                    .await?;
                }
            }
            "error" => {
                if let Some(text) = params["error"]["message"].as_str() {
                    self.notice(text).await?;
                }
            }
            _ => {}
        }
        if self.session.is_none() {
            return Ok(());
        }
        for update in self.translator.notification(method, params) {
            self.update(update, true).await?;
        }
        Ok(())
    }

    async fn side_notification(
        &mut self,
        thread: String,
        method: &str,
        params: &Value,
    ) -> Result<(), String> {
        let Some(side) = self.sides.get_mut(&thread) else {
            return Ok(());
        };
        match method {
            "item/agentMessage/delta" => {
                if let Some(delta) = params["delta"].as_str() {
                    side.text.push_str(delta);
                    side.streamed = true;
                }
            }
            "item/completed" if params["item"]["type"] == "agentMessage" && !side.streamed => {
                if let Some(text) = params["item"]["text"].as_str() {
                    if !side.text.is_empty() {
                        side.text.push_str("\n\n");
                    }
                    side.text.push_str(text);
                }
            }
            "turn/completed" => {
                if let Some(side) = self.sides.remove(&thread) {
                    let answer = if side.text.trim().is_empty() {
                        "Codex had no answer.".to_owned()
                    } else {
                        side.text
                    };
                    self.update(
                        json!({
                            "sessionUpdate": "agent_message_chunk",
                            "messageId": side.message_id,
                            "content": { "type": "text", "text": answer },
                            "_meta": { "zz": { "side": true } },
                        }),
                        true,
                    )
                    .await?;
                    self.request(
                        "thread/unsubscribe",
                        &json!({ "threadId": thread }),
                        Outgoing::Quiet,
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn server_request(
        &mut self,
        method: &str,
        rpc_id: Value,
        params: &Value,
    ) -> Result<(), String> {
        if params["threadId"]
            .as_str()
            .is_some_and(|thread| self.sides.contains_key(thread))
        {
            self.reply_error(&rpc_id, "a side question does not run tools");
            return Ok(());
        }
        let item = params["itemId"].as_str().unwrap_or_default().to_owned();
        let reason = params["reason"]
            .as_str()
            .filter(|reason| !reason.is_empty());
        match method {
            "item/commandExecution/requestApproval" | "execCommandApproval" => {
                let command = params["command"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        params["command"].as_array().map(|parts| {
                            parts
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                    })
                    .unwrap_or_else(|| "Run a command".to_owned());
                let reply = if method == "execCommandApproval" {
                    Reply::Legacy
                } else {
                    Reply::Command
                };
                self.approve(rpc_id, reply, &item, &command, "execute", reason)
                    .await
            }
            "item/fileChange/requestApproval" | "applyPatchApproval" => {
                let reply = if method == "applyPatchApproval" {
                    Reply::Legacy
                } else {
                    Reply::FileChange
                };
                self.approve(rpc_id, reply, &item, "Apply the changes", "edit", reason)
                    .await
            }
            "item/permissions/requestApproval" => {
                self.approve(
                    rpc_id,
                    Reply::Permissions(params["permissions"].clone()),
                    &item,
                    reason.unwrap_or("Grant more permissions"),
                    "other",
                    None,
                )
                .await
            }
            "item/tool/requestUserInput" => self.ask_questions(rpc_id, &item, params).await,
            "mcpServer/elicitation/request" => {
                self.reply(
                    &rpc_id,
                    &json!({ "action": "decline", "content": null, "_meta": null }),
                );
                Ok(())
            }
            _ => {
                self.reply_error(&rpc_id, &format!("zz does not handle {method}"));
                Ok(())
            }
        }
    }

    async fn approve(
        &mut self,
        rpc_id: Value,
        reply: Reply,
        item: &str,
        title: &str,
        kind: &str,
        reason: Option<&str>,
    ) -> Result<(), String> {
        let tool_kind = serde_json::from_value::<ToolKind>(Value::from(kind)).ok();
        if tier_approves(*self.auto_approve.lock(), tool_kind) {
            self.reply(&rpc_id, &decision(&reply, "allow"));
            return Ok(());
        }
        let mut tool_call = json!({ "toolCallId": item, "title": title, "kind": kind });
        if let Some(reason) = reason {
            tool_call["content"] =
                json!([{ "type": "content", "content": { "type": "text", "text": reason } }]);
        }
        let mut options = vec![option("allow", "Allow", "allow_once")];
        if !matches!(reply, Reply::Permissions(_)) {
            options.push(option(
                "allow_always",
                "Always allow this session",
                "allow_always",
            ));
        }
        options.push(option("reject", "Reject", "reject_once"));
        self.ask(
            Pending { rpc_id, reply },
            tool_call,
            Value::Array(options),
            Vec::new(),
        )
        .await
    }

    async fn ask_questions(
        &mut self,
        rpc_id: Value,
        item: &str,
        params: &Value,
    ) -> Result<(), String> {
        let questions = params["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|question| AgentQuestion {
                id: question["id"].as_str().unwrap_or_default().to_owned(),
                header: question["header"]
                    .as_str()
                    .filter(|header| !header.is_empty())
                    .map(str::to_owned),
                question: question["question"]
                    .as_str()
                    .unwrap_or("Question")
                    .to_owned(),
                options: question["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|choice| AgentQuestionOption {
                        label: choice["label"].as_str().unwrap_or_default().to_owned(),
                        description: choice["description"]
                            .as_str()
                            .filter(|text| !text.is_empty())
                            .map(str::to_owned),
                    })
                    .collect(),
                multi_select: false,
                allow_other: question["isOther"] == true,
                secret: question["isSecret"] == true,
            })
            .collect::<Vec<_>>();
        let options = match questions.as_slice() {
            [only] => only
                .options
                .iter()
                .enumerate()
                .map(|(index, choice)| {
                    option(&format!("answer-{index}"), &choice.label, "allow_once")
                })
                .collect(),
            _ => Vec::new(),
        };
        let title = match questions.as_slice() {
            [only] => only.question.clone(),
            _ => "Codex has questions".to_owned(),
        };
        self.ask(
            Pending {
                rpc_id,
                reply: Reply::Questions(questions.clone()),
            },
            json!({ "toolCallId": item, "title": title }),
            Value::Array(options),
            questions,
        )
        .await
    }

    async fn ask(
        &mut self,
        pending: Pending,
        tool_call: Value,
        options: Value,
        questions: Vec<AgentQuestion>,
    ) -> Result<(), String> {
        let request_id = self.permission_ids.fetch_add(1, Ordering::Relaxed);
        let payload = AgentStreamPayload::PermissionRequested {
            request_id,
            tool_call,
            options,
            questions,
        };
        if let Err(error) = validate_payload(&payload) {
            log::warn!(target: "zz::agent", "Codex asked something too large to show: {error}");
            self.reply(&pending.rpc_id, &decision(&pending.reply, "reject"));
            return Ok(());
        }
        self.permissions.insert(request_id, pending);
        self.emit(payload).await
    }

    async fn answer(&mut self, request_id: u64, option_id: Option<&str>) -> Result<(), String> {
        let Some(pending) = self.permissions.remove(&request_id) else {
            return Ok(());
        };
        self.emit(AgentStreamPayload::PermissionResolved {
            request_id,
            canceled: option_id.is_none(),
        })
        .await?;
        let result = match (&pending.reply, option_id) {
            (Reply::Questions(questions), Some(option)) => {
                let label = option
                    .strip_prefix("answer-")
                    .and_then(|index| index.parse::<usize>().ok())
                    .zip(questions.first())
                    .and_then(|(index, question)| {
                        Some((
                            question.id.clone(),
                            question.options.get(index)?.label.clone(),
                        ))
                    });
                match label {
                    Some((id, label)) => question_answers(&[AgentQuestionAnswer {
                        id,
                        answers: vec![label],
                    }]),
                    None => question_answers(&[]),
                }
            }
            (Reply::Questions(_), None) => question_answers(&[]),
            (reply, Some(option)) => decision(reply, option),
            (reply, None) => decision(reply, "cancel"),
        };
        self.reply(&pending.rpc_id, &result);
        Ok(())
    }

    async fn answer_questions(
        &mut self,
        request_id: u64,
        answers: &[AgentQuestionAnswer],
    ) -> Result<(), String> {
        let Some(pending) = self.permissions.remove(&request_id) else {
            return Ok(());
        };
        self.emit(AgentStreamPayload::PermissionResolved {
            request_id,
            canceled: false,
        })
        .await?;
        self.reply(&pending.rpc_id, &question_answers(answers));
        Ok(())
    }

    async fn cancel_permissions(&mut self) -> Result<(), String> {
        let pending = std::mem::take(&mut self.permissions);
        for (request_id, pending) in pending {
            let result = match &pending.reply {
                Reply::Questions(_) => question_answers(&[]),
                reply => decision(reply, "cancel"),
            };
            self.reply(&pending.rpc_id, &result);
            self.emit(AgentStreamPayload::PermissionResolved {
                request_id,
                canceled: true,
            })
            .await?;
        }
        Ok(())
    }
}

fn decision(reply: &Reply, option: &str) -> Value {
    match reply {
        Reply::Command | Reply::FileChange => json!({
            "decision": match option {
                "allow" => "accept",
                "allow_always" => "acceptForSession",
                "cancel" => "cancel",
                _ => "decline",
            },
        }),
        Reply::Legacy => json!({
            "decision": match option {
                "allow" => json!("approved"),
                "allow_always" => json!("approved_for_session"),
                "cancel" => json!("abort"),
                _ => json!({ "denied": { "rejection": "The user rejected this." } }),
            },
        }),
        Reply::Permissions(requested) => match option {
            "allow" | "allow_always" => json!({
                "permissions": {
                    "network": requested["network"],
                    "fileSystem": requested["fileSystem"],
                },
                "scope": "turn",
            }),
            _ => json!({ "permissions": {}, "scope": "turn" }),
        },
        Reply::Questions(_) => question_answers(&[]),
    }
}

fn question_answers(answers: &[AgentQuestionAnswer]) -> Value {
    let map = answers
        .iter()
        .map(|answer| (answer.id.clone(), json!({ "answers": answer.answers })))
        .collect::<Map<_, _>>();
    json!({ "answers": map })
}

fn turn_input(prompt: &AgentPrompt) -> Value {
    let mut input = Vec::new();
    if !prompt.text.is_empty() {
        input.push(json!({ "type": "text", "text": prompt.text, "text_elements": [] }));
    }
    for image in &prompt.images {
        input.push(json!({
            "type": "image",
            "url": format!("data:{};base64,{}", image.format, BASE64.encode(&image.data)),
        }));
    }
    Value::Array(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_codex_binary_selects_the_native_driver() {
        let command = CodexCommand::parse("RUST_LOG=warn codex -c model=o3").expect("native");
        assert_eq!(command.program, "codex");
        assert_eq!(command.args, ["-c", "model=o3"]);
        assert_eq!(command.env, [("RUST_LOG".to_owned(), "warn".to_owned())]);
        assert_eq!(
            CodexCommand::parse("codex app-server").map(|command| command.args),
            Some(Vec::new())
        );
        assert_eq!(
            CodexCommand::parse("npx -y @agentclientprotocol/codex-acp@1.11.0"),
            None
        );
    }

    #[test]
    fn decisions_speak_each_request_kinds_dialect() {
        assert_eq!(decision(&Reply::Command, "allow")["decision"], "accept");
        assert_eq!(
            decision(&Reply::FileChange, "allow_always")["decision"],
            "acceptForSession"
        );
        assert_eq!(decision(&Reply::Command, "reject")["decision"], "decline");
        assert_eq!(decision(&Reply::Legacy, "allow")["decision"], "approved");
        assert_eq!(decision(&Reply::Legacy, "cancel")["decision"], "abort");
        let answers = question_answers(&[AgentQuestionAnswer {
            id: "q1".to_owned(),
            answers: vec!["yes".to_owned()],
        }]);
        assert_eq!(
            answers,
            json!({ "answers": { "q1": { "answers": ["yes"] } } })
        );
    }

    #[test]
    fn settings_map_presets_onto_turn_overrides() {
        let mut settings = Settings {
            models: vec![json!({
                "model": "gpt-a", "displayName": "A", "description": "",
                "isDefault": true, "defaultReasoningEffort": "medium",
                "supportedReasoningEfforts": [{ "reasoningEffort": "low", "description": "" },
                    { "reasoningEffort": "medium", "description": "" }],
            })],
            ..Settings::default()
        };
        settings.adopt(&json!({
            "model": "gpt-a", "reasoningEffort": "medium",
            "approvalPolicy": "on-request", "sandbox": { "type": "readOnly" },
        }));
        assert_eq!(settings.mode.as_deref(), Some("read-only"));
        let options = settings.config_options();
        serde_json::from_value::<Vec<agent_client_protocol::schema::v1::SessionConfigOption>>(
            options.clone(),
        )
        .expect("ACP config options");
        assert_eq!(options.as_array().map(Vec::len), Some(3));
        let mut params = json!({});
        settings.turn_overrides(&mut params);
        assert_eq!(params, json!({}), "nothing overrides until the user picks");
        settings.mode = Some("full-access".to_owned());
        settings.mode_override = true;
        settings.turn_overrides(&mut params);
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["sandboxPolicy"]["type"], "dangerFullAccess");
    }
}
