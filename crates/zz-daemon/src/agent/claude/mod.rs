pub(crate) mod sessions;
pub(crate) mod translate;

use std::{
    collections::{HashMap, HashSet},
    io::{BufRead as _, BufReader, Write as _},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use agent_client_protocol::schema::v1::{MessageId, ToolKind};
use async_channel::{Receiver, Sender};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};
use zz_protocol::{AgentAutoApprove, AgentProvider, MAX_AGENT_RESULT_BYTES};

use crate::{
    agent::{
        environment::{AgentWorkspaceEnvironment, agent_path, find_executable},
        host::RuntimeChannels,
        journal::{AgentJournal, JournalEntry},
        runtime::{
            RuntimeCommand, RuntimeControl, StderrTail, prompt_blocks, prompt_updates,
            report_journal_error, tier_approves, validate_payload,
        },
        stream::{AgentPromptOutcome, AgentSessionCapabilities, AgentStreamPayload},
    },
    unmasked::SpawnUnmasked as _,
};

use translate::{Translator, available_commands, tool_info};

const INITIALIZE_TIMEOUT: Duration = Duration::from_mins(1);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
const REAP_GRACE: Duration = Duration::from_secs(2);
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const MAX_REPLAY_UPDATES: usize = 4096;
const SESSION_PAGE: usize = 50;
const SIDE_TIMEOUT: Duration = Duration::from_mins(2);
const VERB_HELP: &str = "zz commands: `//btw <question>` (or `//side`) asks Claude without adding to the conversation, `//steer <text>` redirects the running turn, and `//fork` continues in a copy of this conversation. A single `/` sends Claude Code's own commands.";
const AGENT_NAME: &str = "Claude Code";
const AGENT_KEY: &str = "claude-code";
const BASE_ARGS: [&str; 11] = [
    "-p",
    "--output-format",
    "stream-json",
    "--input-format",
    "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--replay-user-messages",
    "--permission-prompt-tool",
    "stdio",
    "--await-initialize",
];
const MODES: [(&str, &str, &str); 5] = [
    ("default", "Default", "Ask before edits and commands"),
    ("acceptEdits", "Accept edits", "Edit files without asking"),
    ("plan", "Plan", "Plan before changing anything"),
    ("auto", "Auto", "Claude decides which actions need approval"),
    (
        "bypassPermissions",
        "Bypass permissions",
        "Run everything without asking",
    ),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClaudeCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
}

impl ClaudeCommand {
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
        matches!(name, "claude" | "claude.exe").then(|| Self {
            program: program.to_owned(),
            args: tokens.map(str::to_owned).collect(),
            env,
        })
    }

    fn model_flag(&self) -> Option<String> {
        let mut args = self.args.iter();
        while let Some(arg) = args.next() {
            if let Some(model) = arg.strip_prefix("--model=") {
                return Some(model.to_owned());
            }
            if arg == "--model" {
                return args.next().cloned();
            }
        }
        None
    }

    fn mode_flag(&self) -> Option<String> {
        let mut args = self.args.iter();
        while let Some(arg) = args.next() {
            if let Some(mode) = arg.strip_prefix("--permission-mode=") {
                return Some(mode.to_owned());
            }
            if arg == "--permission-mode" {
                return args.next().cloned();
            }
            if arg == "--dangerously-skip-permissions" {
                return Some("bypassPermissions".to_owned());
            }
        }
        None
    }
}

pub(crate) async fn run_claude_runtime(
    command: ClaudeCommand,
    workspace: AgentWorkspaceEnvironment,
    provider: AgentProvider,
    channels: RuntimeChannels,
) -> Result<(), String> {
    let (child_tx, child_rx) = async_channel::unbounded();
    let mut runtime = Runtime {
        provider,
        settings: Settings::new(&command),
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
        ready_sent: false,
        session: None,
        starting: None,
        translator: Translator::default(),
        requests: HashMap::new(),
        next_request: 0,
        permissions: HashMap::new(),
        questions: HashMap::new(),
        turn: None,
        last_turn: 0,
        deferred_cancels: HashSet::new(),
        stale_commands: HashSet::new(),
        lifecycle_seen: false,
        verbs: 0,
    };
    let commands = channels.commands;
    let controls = channels.controls;
    loop {
        let input = next_input(&commands, &controls, &child_rx, runtime.next_deadline()).await;
        match input {
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

enum Input {
    Command(Result<RuntimeCommand, async_channel::RecvError>),
    Control(Result<RuntimeControl, async_channel::RecvError>),
    Child(Result<ChildEvent, async_channel::RecvError>),
    Deadline,
}

async fn next_input(
    commands: &Receiver<RuntimeCommand>,
    controls: &Receiver<RuntimeControl>,
    child: &Receiver<ChildEvent>,
    deadline: Option<Instant>,
) -> Input {
    let timer = async {
        match deadline {
            Some(deadline) => {
                smol::Timer::at(deadline).await;
            }
            None => futures_lite::future::pending::<()>().await,
        }
        Input::Deadline
    };
    let controls = async {
        if controls.is_closed() && controls.is_empty() {
            futures_lite::future::pending::<()>().await;
        }
        Input::Control(controls.recv().await)
    };
    futures_lite::future::or(
        futures_lite::future::or(controls, async { Input::Child(child.recv().await) }),
        futures_lite::future::or(async { Input::Command(commands.recv().await) }, timer),
    )
    .await
}

enum ChildEvent {
    Line(u64, Vec<u8>),
    Closed(u64),
}

struct Process {
    child: Option<Child>,
    stdin: Sender<String>,
}

impl Process {
    fn send(&self, frame: &Value) {
        let _ = self.stdin.try_send(frame.to_string());
    }

    fn exit_detail(&mut self) -> Option<String> {
        let child = self.child.as_mut()?;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status.to_string()),
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                _ => return None,
            }
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.stdin.close();
        let Some(mut child) = self.child.take() else {
            return;
        };
        let spawned = thread::Builder::new()
            .name("zz-claude-reap".to_owned())
            .spawn(move || reap(&mut child));
        if let Err(error) = spawned {
            log::warn!(target: "zz::agent", "could not reap Claude Code: {error}");
        }
    }
}

fn reap(child: &mut Child) {
    let deadline = Instant::now() + REAP_GRACE;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    signal_group(child, Signal::Terminate);
    let deadline = Instant::now() + REAP_GRACE;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    signal_group(child, Signal::Kill);
    let _ = child.wait();
}

#[derive(Clone, Copy)]
enum Signal {
    Terminate,
    Kill,
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "signal the process group zz created for this Claude Code child"
)]
fn signal_group(child: &mut Child, signal: Signal) {
    let Ok(pid) = libc::pid_t::try_from(child.id()) else {
        return;
    };
    let signal = match signal {
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    unsafe {
        libc::killpg(pid, signal);
    }
}

#[cfg(not(unix))]
fn signal_group(child: &mut Child, _signal: Signal) {
    let _ = child.kill();
}

struct Session {
    id: String,
    cwd: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Start {
    Open { resume: bool },
    New,
    Switch,
    Fork,
}

enum Launch {
    New(String),
    Resume(String),
    Fork { source: String, id: String },
}

impl Launch {
    fn session_id(&self) -> &str {
        match self {
            Self::New(id) | Self::Resume(id) | Self::Fork { id, .. } => id,
        }
    }

    fn history(&self) -> Option<&str> {
        match self {
            Self::New(_) => None,
            Self::Resume(id) => Some(id),
            Self::Fork { source, .. } => Some(source),
        }
    }
}

struct Starting {
    start: Start,
    session_id: String,
    history: Option<String>,
    cwd: PathBuf,
}

enum Outgoing {
    Initialize,
    Interrupt,
    Setting { option_id: String, value: String },
    Mode { mode_id: String },
    Side { message_id: String },
}

struct Turn {
    id: u64,
    uuid: String,
    started: bool,
    error: Option<String>,
}

enum Permission {
    Approval {
        claude_id: String,
        input: Value,
        suggestions: Value,
    },
    Plan {
        claude_id: String,
        input: Value,
    },
    Question {
        claude_id: String,
        question: String,
        labels: Vec<String>,
    },
}

impl Permission {
    fn claude_id(&self) -> &str {
        match self {
            Self::Approval { claude_id, .. }
            | Self::Plan { claude_id, .. }
            | Self::Question { claude_id, .. } => claude_id,
        }
    }
}

struct QuestionGroup {
    input: Value,
    answers: Map<String, Value>,
    pending: HashSet<u64>,
}

struct Settings {
    commands: Vec<Value>,
    hidden_commands: HashSet<String>,
    models: Vec<Value>,
    model: String,
    mode: String,
    effort: String,
}

impl Settings {
    fn new(command: &ClaudeCommand) -> Self {
        Self {
            commands: Vec::new(),
            hidden_commands: HashSet::new(),
            models: Vec::new(),
            model: command.model_flag().unwrap_or_else(|| "default".to_owned()),
            mode: command.mode_flag().unwrap_or_else(|| "default".to_owned()),
            effort: "default".to_owned(),
        }
    }

    fn current_model(&self) -> Option<&Value> {
        self.models
            .iter()
            .find(|model| model["value"].as_str() == Some(self.model.as_str()))
            .or_else(|| self.models.first())
    }

    fn modes(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        let auto = self
            .current_model()
            .is_some_and(|model| model["supportsAutoMode"] == true);
        MODES
            .iter()
            .copied()
            .filter(|(id, ..)| match *id {
                "auto" => auto || self.mode == "auto",
                "bypassPermissions" => self.mode == "bypassPermissions",
                _ => true,
            })
            .collect()
    }

    fn mode_state(&self) -> Value {
        json!({
            "currentModeId": self.mode,
            "availableModes": self.modes().iter().map(|(id, name, description)| json!({
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
                "currentValue": self.model,
                "options": self.models.iter().filter_map(|model| {
                    let value = model["value"].as_str()?;
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
            "currentValue": self.mode,
            "options": self.modes().iter().map(|(id, name, description)| json!({
                "value": id, "name": name, "description": description,
            })).collect::<Vec<_>>(),
        }));
        let levels = self
            .current_model()
            .filter(|model| model["supportsEffort"] == true)
            .and_then(|model| model["supportedEffortLevels"].as_array())
            .filter(|levels| !levels.is_empty());
        if let Some(levels) = levels {
            let mut choices = vec![json!({ "value": "default", "name": "Default" })];
            choices.extend(levels.iter().filter_map(Value::as_str).map(|level| {
                let mut name = level.to_owned();
                if let Some(first) = name.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                json!({ "value": level, "name": name })
            }));
            options.push(json!({
                "id": "effort",
                "name": "Effort",
                "category": "thought_level",
                "type": "select",
                "currentValue": self.effort,
                "options": choices,
            }));
        }
        Value::Array(options)
    }
}

struct Runtime {
    provider: AgentProvider,
    command: ClaudeCommand,
    workspace: AgentWorkspaceEnvironment,
    auto_approve: Arc<Mutex<AgentAutoApprove>>,
    permission_ids: Arc<AtomicU64>,
    journal: Option<Arc<AgentJournal>>,
    events: Sender<AgentStreamPayload>,
    child_tx: Sender<ChildEvent>,
    process: Option<Process>,
    generation: u64,
    stderr: StderrTail,
    ready_sent: bool,
    session: Option<Session>,
    starting: Option<Starting>,
    translator: Translator,
    requests: HashMap<String, (Outgoing, Instant)>,
    next_request: u64,
    permissions: HashMap<u64, Permission>,
    questions: HashMap<String, QuestionGroup>,
    turn: Option<Turn>,
    last_turn: u64,
    deferred_cancels: HashSet<u64>,
    stale_commands: HashSet<String>,
    lifecycle_seen: bool,
    settings: Settings,
    verbs: u64,
}

impl Runtime {
    async fn emit(&self, payload: AgentStreamPayload) -> Result<(), String> {
        if let Err(error) = validate_payload(&payload) {
            log::warn!(target: "zz::agent", "dropping an oversized Claude Code item: {error}");
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

    fn send(&self, frame: &Value) {
        if let Some(process) = &self.process {
            process.send(frame);
        }
    }

    fn respond(&self, request_id: &str, response: &Value) {
        self.send(&json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": request_id, "response": response },
        }));
    }

    fn respond_error(&self, request_id: &str, error: &str) {
        self.send(&json!({
            "type": "control_response",
            "response": { "subtype": "error", "request_id": request_id, "error": error },
        }));
    }

    fn request(&mut self, body: &Value, outgoing: Outgoing, timeout: Duration) {
        self.next_request += 1;
        let request_id = format!("zz-{}", self.next_request);
        self.send(&json!({ "type": "control_request", "request_id": request_id, "request": body }));
        self.requests
            .insert(request_id, (outgoing, Instant::now() + timeout));
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.requests.values().map(|(_, deadline)| *deadline).min()
    }

    fn cwd(&self) -> PathBuf {
        self.session
            .as_ref()
            .map(|session| session.cwd.clone())
            .or_else(|| self.starting.as_ref().map(|starting| starting.cwd.clone()))
            .unwrap_or_default()
    }

    async fn command(&mut self, command: RuntimeCommand) -> Result<(), String> {
        match command {
            RuntimeCommand::Open {
                cwd,
                resume_session,
            } => {
                let resume = resume_session.filter(|id| sessions::valid_uuid(id));
                let start = Start::Open {
                    resume: resume.is_some(),
                };
                self.begin(cwd, start, resume).await
            }
            RuntimeCommand::NewSession { cwd } => self.begin(cwd, Start::New, None).await,
            RuntimeCommand::SwitchSession { session } => {
                if !sessions::valid_uuid(&session.session_id) {
                    return self
                        .emit(AgentStreamPayload::SessionSwitchFailed {
                            message: "not a Claude Code session ID".to_owned(),
                        })
                        .await;
                }
                self.begin(session.cwd, Start::Switch, Some(session.session_id))
                    .await
            }
            RuntimeCommand::ListSessions {
                client,
                cwd,
                cursor,
                replace,
            } => {
                let filter = cwd.clone();
                let all = smol::unblock(move || {
                    sessions::config_home()
                        .map(|config| sessions::list(&config, filter.as_deref()))
                        .unwrap_or_default()
                })
                .await;
                let offset = cursor
                    .and_then(|cursor| cursor.parse::<usize>().ok())
                    .unwrap_or(0)
                    .min(all.len());
                let end = (offset + SESSION_PAGE).min(all.len());
                self.emit(AgentStreamPayload::SessionsListed {
                    client,
                    sessions: all[offset..end].to_vec(),
                    next_cursor: (end < all.len()).then(|| end.to_string()),
                    cwd_filter: cwd,
                    replace,
                })
                .await
            }
            RuntimeCommand::DeleteSession { client, .. } => {
                self.emit(AgentStreamPayload::SessionDeleteFailed {
                    client,
                    message: "Claude Code sessions are not deleted from zz".to_owned(),
                })
                .await
            }
            RuntimeCommand::Prompt { turn_id, prompt } => {
                self.last_turn = self.last_turn.max(turn_id);
                if self.deferred_cancels.remove(&turn_id) {
                    self.emit(AgentStreamPayload::PromptAccepted { turn_id })
                        .await?;
                    return self.finish(turn_id, cancelled()).await;
                }
                if self.session.is_none() || self.process.is_none() {
                    return self
                        .emit(AgentStreamPayload::PaneFailed {
                            message: "agent session is not ready".to_owned(),
                        })
                        .await;
                }
                let content = prompt_content(&prompt);
                let echo = format!("zz-prompt-{turn_id}-{:016x}", random_u64());
                for update in prompt_updates(&prompt_blocks(prompt), &MessageId::new(echo)) {
                    let update =
                        serde_json::to_value(&update).map_err(|error| error.to_string())?;
                    self.update(update, true).await?;
                }
                let uuid = new_uuid();
                self.send(&json!({
                    "type": "user",
                    "uuid": uuid,
                    "session_id": "",
                    "parent_tool_use_id": null,
                    "message": { "role": "user", "content": content },
                    "origin": { "kind": "human" },
                }));
                self.turn = Some(Turn {
                    id: turn_id,
                    uuid,
                    started: false,
                    error: None,
                });
                self.emit(AgentStreamPayload::PromptAccepted { turn_id })
                    .await
            }
            RuntimeCommand::Authenticate { .. } => {
                self.emit(AgentStreamPayload::AuthenticationFailed {
                    message: "Claude Code signs in with `claude /login` in a terminal".to_owned(),
                })
                .await
            }
            RuntimeCommand::SetConfigOption { option_id, value } => {
                if self.session.is_none() {
                    return self
                        .emit(AgentStreamPayload::SettingFailed {
                            option_id,
                            message: "agent session is not ready".to_owned(),
                        })
                        .await;
                }
                let body = match option_id.as_str() {
                    "model" => json!({
                        "subtype": "set_model",
                        "model": (value != "default").then_some(value.as_str()),
                    }),
                    "mode" => json!({ "subtype": "set_permission_mode", "mode": value }),
                    "effort" => json!({
                        "subtype": "apply_flag_settings",
                        "settings": { "effortLevel": (value != "default").then_some(value.as_str()) },
                    }),
                    _ => {
                        return self
                            .emit(AgentStreamPayload::SettingFailed {
                                option_id,
                                message: "Claude Code has no such setting".to_owned(),
                            })
                            .await;
                    }
                };
                self.request(
                    &body,
                    Outgoing::Setting { option_id, value },
                    CONTROL_TIMEOUT,
                );
                Ok(())
            }
            RuntimeCommand::SetMode { mode_id } => {
                if self.session.is_none() {
                    return self
                        .emit(AgentStreamPayload::SettingFailed {
                            option_id: mode_id,
                            message: "agent session is not ready".to_owned(),
                        })
                        .await;
                }
                self.request(
                    &json!({ "subtype": "set_permission_mode", "mode": mode_id }),
                    Outgoing::Mode { mode_id },
                    CONTROL_TIMEOUT,
                );
                Ok(())
            }
            RuntimeCommand::Verb { prompt } => self.verb(&prompt.text).await,
            RuntimeCommand::Shutdown => Ok(()),
        }
    }

    async fn control(&mut self, control: RuntimeControl) -> Result<(), String> {
        match control {
            RuntimeControl::Cancel { turn_id, .. } => {
                if self.turn.as_ref().is_some_and(|turn| turn.id == turn_id) {
                    if let Some(turn) = self.turn.take() {
                        self.stale_commands.insert(turn.uuid);
                    }
                    self.request(
                        &json!({ "subtype": "interrupt" }),
                        Outgoing::Interrupt,
                        CONTROL_TIMEOUT,
                    );
                    self.cancel_permissions(true).await?;
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
            } => self.answer(request_id, option_id).await,
        }
    }

    async fn begin(
        &mut self,
        cwd: PathBuf,
        start: Start,
        resume: Option<String>,
    ) -> Result<(), String> {
        let config = sessions::config_home();
        let resume = resume.filter(|id| {
            config
                .as_deref()
                .and_then(|config| sessions::session_file(config, &cwd, id))
                .is_some()
        });
        let launch = match (start, resume) {
            (Start::Fork, Some(source)) => Launch::Fork {
                source,
                id: new_uuid(),
            },
            (Start::Fork, None) => {
                return self
                    .notice("Nothing to fork yet: send a prompt first.")
                    .await;
            }
            (_, Some(id)) => Launch::Resume(id),
            (_, None) => Launch::New(new_uuid()),
        };
        self.cancel_permissions(false).await?;
        if let Some(turn) = self.turn.take() {
            self.emit(AgentStreamPayload::PromptFinished {
                turn_id: turn.id,
                outcome: cancelled(),
            })
            .await?;
        }
        self.process = None;
        self.requests.clear();
        self.lifecycle_seen = false;
        if let Err(message) = self.spawn(&cwd, &launch) {
            return self.fail_start(start, message).await;
        }
        let start = match start {
            Start::Open { .. } => Start::Open {
                resume: matches!(launch, Launch::Resume(_)),
            },
            other => other,
        };
        self.starting = Some(Starting {
            start,
            session_id: launch.session_id().to_owned(),
            history: launch.history().map(str::to_owned),
            cwd,
        });
        self.request(
            &json!({
                "subtype": "initialize",
                "supportedDialogKinds": [],
                "perTaskStopAffordance": false,
                "agentProgressSummaries": true,
            }),
            Outgoing::Initialize,
            INITIALIZE_TIMEOUT,
        );
        Ok(())
    }

    fn spawn(&mut self, cwd: &Path, launch: &Launch) -> Result<(), String> {
        let program = find_executable(&self.command.program).ok_or_else(|| {
            format!(
                "Claude Code is not installed: `{}` was not found on PATH. Install it from https://code.claude.com, or point agent-claude-code-command at it.",
                self.command.program
            )
        })?;
        let mut command = Command::new(program);
        command.args(BASE_ARGS);
        match launch {
            Launch::New(id) => command.args(["--session-id", id]),
            Launch::Resume(id) => command.args(["--resume", id]),
            Launch::Fork { source, id } => {
                command.args(["--resume", source, "--fork-session", "--session-id", id])
            }
        };
        command
            .args(&self.command.args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in crate::PARENT_CLAUDE_SESSION_ENVIRONMENT {
            command.env_remove(name);
        }
        if let Some(path) = agent_path() {
            command.env("PATH", path);
        }
        for (name, value) in self.workspace.entries() {
            command.env(name, value);
        }
        command.env("CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS", "1");
        for (name, value) in &self.command.env {
            command.env(name, value);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            command.process_group(0);
        }
        let mut child = command
            .spawn_unmasked()
            .map_err(|error| format!("could not start Claude Code: {error}"))?;
        self.generation += 1;
        let generation = self.generation;
        let (stdin_tx, stdin_rx) = async_channel::unbounded::<String>();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let process = Process {
            child: Some(child),
            stdin: stdin_tx,
        };
        if let Some(mut stdin) = stdin {
            spawn_thread("zz-claude-in", move || {
                while let Ok(line) = stdin_rx.recv_blocking() {
                    if stdin
                        .write_all(line.as_bytes())
                        .and_then(|()| stdin.write_all(b"\n"))
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        }
        if let Some(stdout) = stdout {
            let events = self.child_tx.clone();
            spawn_thread("zz-claude-out", move || {
                let mut reader = BufReader::with_capacity(1 << 16, stdout);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match reader.read_until(b'\n', &mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) if line.len() > MAX_FRAME_BYTES => {
                            log::warn!(target: "zz::agent", "dropping a {} byte Claude Code frame", line.len());
                        }
                        Ok(_) => {
                            if events
                                .send_blocking(ChildEvent::Line(
                                    generation,
                                    std::mem::take(&mut line),
                                ))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                let _ = events.send_blocking(ChildEvent::Closed(generation));
            })?;
        }
        if let Some(stderr) = stderr {
            let tail = self.stderr.clone();
            spawn_thread("zz-claude-err", move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::warn!(target: "zz::agent::stderr", "{line}");
                    tail.push(&line);
                }
            })?;
        }
        self.process = Some(process);
        Ok(())
    }

    async fn fail_start(&mut self, start: Start, message: String) -> Result<(), String> {
        self.process = None;
        self.starting = None;
        match start {
            Start::Open { .. } => self.emit(AgentStreamPayload::PaneFailed { message }).await,
            Start::New | Start::Switch | Start::Fork => {
                self.emit(AgentStreamPayload::SessionSwitchFailed { message })
                    .await
            }
        }
    }

    async fn initialized(&mut self, response: &Value) -> Result<(), String> {
        let Some(starting) = self.starting.take() else {
            return Ok(());
        };
        self.settings.commands = response["commands"].as_array().cloned().unwrap_or_default();
        self.settings.models = response["models"].as_array().cloned().unwrap_or_default();
        if let Some(mode) = response["current_permission_mode"].as_str() {
            mode.clone_into(&mut self.settings.mode);
        }
        if !self.ready_sent {
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
            .await?;
        }
        self.translator = Translator::new(starting.cwd.clone());
        let replay = match &starting.history {
            Some(source) => {
                self.history(source, &starting.session_id, &starting.cwd)
                    .await
            }
            None => Vec::new(),
        };
        self.session = Some(Session {
            id: starting.session_id.clone(),
            cwd: starting.cwd.clone(),
        });
        let restoring = match starting.start {
            Start::Open { .. } => !replay.is_empty(),
            Start::New | Start::Switch | Start::Fork => true,
        };
        self.emit(AgentStreamPayload::SessionReset { restoring })
            .await?;
        for (update, record) in replay {
            self.update(update, record).await?;
        }
        let modes = Some(self.settings.mode_state());
        let config_options = Some(self.settings.config_options());
        match starting.start {
            Start::Open { .. } => {
                self.emit(AgentStreamPayload::SessionReady {
                    session_id: starting.session_id,
                    modes,
                    config_options,
                })
                .await?;
            }
            Start::New | Start::Switch | Start::Fork => {
                self.emit(AgentStreamPayload::SessionSwitched {
                    session_id: starting.session_id,
                    cwd: starting.cwd,
                    modes,
                    config_options,
                    replay: Vec::new(),
                })
                .await?;
            }
        }
        self.publish_commands().await?;
        if let (Start::Fork, Some(source)) = (starting.start, &starting.history) {
            self.notice(&format!(
                "Forked from session `{source}`. The original is unchanged."
            ))
            .await?;
        }
        Ok(())
    }

    async fn history(&self, source: &str, session_id: &str, cwd: &Path) -> Vec<(Value, bool)> {
        let file =
            sessions::config_home().and_then(|config| sessions::session_file(&config, cwd, source));
        if let Some(file) = file {
            let cwd = cwd.to_path_buf();
            let mut updates = smol::unblock(move || sessions::transcript(&file, cwd)).await;
            if updates.len() > MAX_REPLAY_UPDATES {
                updates.drain(..updates.len() - MAX_REPLAY_UPDATES);
            }
            if let Some(journal) = self.journal.as_deref()
                && let Err(error) = journal.remove_for(self.provider, session_id)
            {
                report_journal_error(session_id, &error.to_string());
            }
            return updates.into_iter().map(|update| (update, true)).collect();
        }
        let Some(journal) = self.journal.as_deref() else {
            return Vec::new();
        };
        match journal.replay_for(self.provider, source) {
            Ok(records) => records
                .into_iter()
                .map(|(_, JournalEntry::Update(update))| (update, false))
                .collect(),
            Err(error) => {
                report_journal_error(session_id, &error.to_string());
                Vec::new()
            }
        }
    }

    async fn verb(&mut self, text: &str) -> Result<(), String> {
        let line = text.trim_start().strip_prefix("//").unwrap_or(text).trim();
        let (name, rest) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(name, rest)| (name, rest.trim()));
        self.verbs += 1;
        let id = format!(
            "zz-command-{}-{:08x}",
            self.verbs,
            random_u64() & 0xffff_ffff
        );
        self.update(
            json!({
                "sessionUpdate": "user_message_chunk",
                "messageId": format!("{id}-in"),
                "content": { "type": "text", "text": text.trim() },
            }),
            true,
        )
        .await?;
        if self.session.is_none() || self.process.is_none() {
            return self.notice("Claude Code is not running.").await;
        }
        match name {
            "btw" | "side" if !rest.is_empty() => {
                self.request(
                    &json!({ "subtype": "side_question", "question": rest }),
                    Outgoing::Side {
                        message_id: format!("{id}-out"),
                    },
                    SIDE_TIMEOUT,
                );
                Ok(())
            }
            "steer" if !rest.is_empty() => {
                if self.turn.is_none() {
                    return self.notice("Nothing is running to steer.").await;
                }
                self.send(&json!({
                    "type": "user",
                    "uuid": new_uuid(),
                    "session_id": "",
                    "parent_tool_use_id": null,
                    "priority": "now",
                    "message": { "role": "user", "content": [{ "type": "text", "text": rest }] },
                    "origin": { "kind": "human" },
                }));
                Ok(())
            }
            "fork" => {
                if self.turn.is_some() {
                    return self.notice("Stop the turn before forking.").await;
                }
                let Some(session) = &self.session else {
                    return Ok(());
                };
                let (cwd, source) = (session.cwd.clone(), session.id.clone());
                self.begin(cwd, Start::Fork, Some(source)).await
            }
            _ => self.notice(VERB_HELP).await,
        }
    }

    async fn notice(&mut self, text: &str) -> Result<(), String> {
        self.verbs += 1;
        let id = format!(
            "zz-notice-{}-{:08x}",
            self.verbs,
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

    async fn publish_commands(&self) -> Result<(), String> {
        let update = available_commands(&self.settings.commands, &self.settings.hidden_commands);
        self.update(update, true).await
    }

    async fn publish_settings(&self) -> Result<(), String> {
        self.update(
            json!({ "sessionUpdate": "current_mode_update", "currentModeId": self.settings.mode }),
            true,
        )
        .await?;
        self.update(
            json!({ "sessionUpdate": "config_option_update", "configOptions": self.settings.config_options() }),
            true,
        )
        .await
    }

    async fn shutdown(&mut self) {
        let _ = self.cancel_permissions(true).await;
        self.process = None;
    }

    async fn expire(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let expired = self
            .requests
            .iter()
            .filter(|(_, (_, deadline))| *deadline <= now)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            let Some((outgoing, _)) = self.requests.remove(&id) else {
                continue;
            };
            self.settle(
                outgoing,
                Err("Claude Code did not answer in time".to_owned()),
            )
            .await?;
        }
        Ok(())
    }

    async fn settle(
        &mut self,
        outgoing: Outgoing,
        result: Result<Value, String>,
    ) -> Result<(), String> {
        match (outgoing, result) {
            (Outgoing::Initialize, Ok(response)) => self.initialized(&response).await,
            (Outgoing::Initialize, Err(error)) => {
                let start = self
                    .starting
                    .as_ref()
                    .map_or(Start::Open { resume: false }, |starting| starting.start);
                let tail = self.stderr.snapshot();
                let message = tail.map_or_else(
                    || format!("Claude Code did not start: {error}"),
                    |tail| format!("Claude Code did not start: {error}\n{tail}"),
                );
                self.fail_start(start, message).await
            }
            (Outgoing::Interrupt, _) => Ok(()),
            (Outgoing::Side { message_id }, Ok(response)) => {
                let answer = response["response"]
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or("Claude had no answer.");
                self.update(side_answer(&message_id, answer), true).await
            }
            (Outgoing::Side { .. }, Err(error)) => {
                self.notice(&format!("The side question failed: {error}"))
                    .await
            }
            (Outgoing::Setting { option_id, value }, Ok(_)) => {
                match option_id.as_str() {
                    "model" => {
                        self.settings.model.clone_from(&value);
                        if self.settings.effort != "default"
                            && !self
                                .settings
                                .current_model()
                                .and_then(|model| model["supportedEffortLevels"].as_array())
                                .is_some_and(|levels| {
                                    levels
                                        .iter()
                                        .any(|level| level == self.settings.effort.as_str())
                                })
                        {
                            "default".clone_into(&mut self.settings.effort);
                        }
                    }
                    "mode" => self.settings.mode.clone_from(&value),
                    "effort" => self.settings.effort.clone_from(&value),
                    _ => {}
                }
                self.emit(AgentStreamPayload::ConfigOptionsChanged {
                    option_id,
                    value,
                    config_options: self.settings.config_options(),
                })
                .await?;
                self.publish_settings().await
            }
            (Outgoing::Setting { option_id, .. }, Err(error)) => {
                self.emit(AgentStreamPayload::SettingFailed {
                    option_id,
                    message: format!("could not change the setting: {error}"),
                })
                .await
            }
            (Outgoing::Mode { mode_id }, Ok(_)) => {
                self.settings.mode.clone_from(&mode_id);
                self.emit(AgentStreamPayload::ModeChanged { mode_id })
                    .await?;
                self.publish_settings().await
            }
            (Outgoing::Mode { mode_id }, Err(error)) => {
                self.emit(AgentStreamPayload::SettingFailed {
                    option_id: mode_id,
                    message: format!("could not change the permission mode: {error}"),
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
                let Ok(frame) = serde_json::from_slice::<Value>(&line) else {
                    log::debug!(
                        target: "zz::agent",
                        "ignoring a non-JSON Claude Code line: {}",
                        String::from_utf8_lossy(&line[..line.len().min(200)])
                    );
                    return Ok(());
                };
                self.frame(frame).await
            }
            ChildEvent::Closed(_) => self.closed().await,
        }
    }

    async fn closed(&mut self) -> Result<(), String> {
        let detail = self.process.as_mut().and_then(Process::exit_detail);
        self.process = None;
        let mut message = detail.map_or_else(
            || "Claude Code exited".to_owned(),
            |status| format!("Claude Code exited ({status})"),
        );
        if let Some(tail) = self.stderr.snapshot() {
            message.push('\n');
            message.push_str(&tail);
        }
        self.permissions.clear();
        self.questions.clear();
        if let Some(starting) = self.starting.take() {
            self.requests.clear();
            return self.fail_start(starting.start, message).await;
        }
        self.requests.clear();
        if let Some(turn) = self.turn.take() {
            self.emit(AgentStreamPayload::PromptFinished {
                turn_id: turn.id,
                outcome: AgentPromptOutcome::Failed {
                    message: message.clone(),
                },
            })
            .await?;
        }
        self.emit(AgentStreamPayload::PaneFailed { message }).await
    }

    async fn frame(&mut self, frame: Value) -> Result<(), String> {
        match frame["type"].as_str() {
            Some("control_request") => return self.control_request(&frame).await,
            Some("control_response") => {
                let response = &frame["response"];
                let Some(id) = response["request_id"].as_str() else {
                    return Ok(());
                };
                let Some((outgoing, _)) = self.requests.remove(id) else {
                    return Ok(());
                };
                let result = if response["subtype"] == "success" {
                    Ok(response["response"].clone())
                } else {
                    Err(response["error"]
                        .as_str()
                        .unwrap_or("request failed")
                        .to_owned())
                };
                return self.settle(outgoing, result).await;
            }
            Some("control_cancel_request") => {
                if let Some(id) = frame["request_id"].as_str() {
                    self.withdraw(id).await?;
                }
                return Ok(());
            }
            Some("command_lifecycle") => return self.lifecycle(&frame).await,
            Some("system") => self.system(&frame).await?,
            Some("result") => {
                if frame["is_error"] == true
                    && let Some(turn) = &mut self.turn
                {
                    turn.error = Some(result_error(&frame));
                }
            }
            _ => {}
        }
        if self.session.is_none() {
            return Ok(());
        }
        for update in self.translator.frame(&frame) {
            self.update(update, true).await?;
        }
        Ok(())
    }

    async fn system(&mut self, frame: &Value) -> Result<(), String> {
        match frame["subtype"].as_str() {
            Some("init") => {
                if let Some(version) = frame["claude_code_version"].as_str() {
                    log::debug!(target: "zz::agent", "Claude Code {version}");
                }
                let hidden = frame["terminal_slash_commands"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<HashSet<_>>();
                if hidden != self.settings.hidden_commands {
                    self.settings.hidden_commands = hidden;
                    self.publish_commands().await?;
                }
                let mut changed = false;
                if let Some(mode) = frame["permissionMode"].as_str()
                    && mode != self.settings.mode
                {
                    mode.clone_into(&mut self.settings.mode);
                    changed = true;
                }
                if let Some(model) = frame["model"].as_str()
                    && let Some(value) = self.settings.models.iter().find_map(|entry| {
                        (entry["resolvedModel"].as_str() == Some(model))
                            .then(|| entry["value"].as_str())
                            .flatten()
                    })
                    && self.settings.model == "default"
                    && self
                        .settings
                        .current_model()
                        .and_then(|entry| entry["resolvedModel"].as_str())
                        != Some(model)
                {
                    value.clone_into(&mut self.settings.model);
                    changed = true;
                }
                if changed {
                    self.publish_settings().await?;
                }
            }
            Some("status") => {
                if let Some(mode) = frame["permissionMode"].as_str()
                    && mode != self.settings.mode
                {
                    mode.clone_into(&mut self.settings.mode);
                    self.publish_settings().await?;
                }
            }
            Some("commands_changed") => {
                self.settings.commands = frame["commands"].as_array().cloned().unwrap_or_default();
                self.publish_commands().await?;
            }
            Some("session_state_changed") if frame["state"] == "idle" => {
                let settled = self
                    .turn
                    .as_ref()
                    .is_some_and(|turn| turn.started || !self.lifecycle_seen);
                if settled {
                    self.finish_active().await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn lifecycle(&mut self, frame: &Value) -> Result<(), String> {
        self.lifecycle_seen = true;
        let (Some(uuid), Some(state)) = (frame["command_uuid"].as_str(), frame["state"].as_str())
        else {
            return Ok(());
        };
        if self.stale_commands.contains(uuid) {
            if !matches!(state, "queued" | "started") {
                self.stale_commands.remove(uuid);
            }
            return Ok(());
        }
        let Some(turn) = self.turn.as_mut().filter(|turn| turn.uuid == uuid) else {
            return Ok(());
        };
        match state {
            "started" => {
                turn.started = true;
                Ok(())
            }
            "cancelled" => {
                let turn_id = turn.id;
                self.turn = None;
                self.finish(turn_id, cancelled()).await
            }
            "completed" | "failed" | "errored" => self.finish_active().await,
            _ => Ok(()),
        }
    }

    async fn finish_active(&mut self) -> Result<(), String> {
        let Some(turn) = self.turn.take() else {
            return Ok(());
        };
        let outcome = turn.error.map_or_else(
            || AgentPromptOutcome::Finished {
                stop_reason: Value::from("end_turn"),
            },
            |message| AgentPromptOutcome::Failed { message },
        );
        self.finish(turn.id, outcome).await
    }

    async fn finish(&self, turn_id: u64, outcome: AgentPromptOutcome) -> Result<(), String> {
        self.emit(AgentStreamPayload::PromptFinished { turn_id, outcome })
            .await
    }

    async fn control_request(&mut self, frame: &Value) -> Result<(), String> {
        let Some(claude_id) = frame["request_id"].as_str().map(str::to_owned) else {
            return Ok(());
        };
        let request = &frame["request"];
        if request["subtype"] != "can_use_tool" {
            self.respond_error(
                &claude_id,
                &format!(
                    "zz does not handle {}",
                    request["subtype"].as_str().unwrap_or("this request")
                ),
            );
            return Ok(());
        }
        let tool = request["tool_name"].as_str().unwrap_or_default().to_owned();
        let input = request["input"].clone();
        let tool_use_id = request["tool_use_id"]
            .as_str()
            .unwrap_or(&claude_id)
            .to_owned();
        match tool.as_str() {
            "AskUserQuestion" => self.ask_questions(claude_id, &tool_use_id, input).await,
            "ExitPlanMode" => {
                let content = input["plan"]
                    .as_str()
                    .map_or_else(
                        || json!([]),
                        |plan| json!([{ "type": "content", "content": { "type": "text", "text": plan } }]),
                    );
                let tool_call = json!({
                    "toolCallId": tool_use_id,
                    "title": "Ready to code?",
                    "kind": "switch_mode",
                    "content": content,
                });
                let options = json!([
                    option("accept_edits", "Yes, and auto-accept edits", "allow_always"),
                    option("approve", "Yes, and manually approve edits", "allow_once"),
                    option("keep_planning", "No, keep planning", "reject_once"),
                ]);
                self.ask(Permission::Plan { claude_id, input }, tool_call, options)
                    .await
                    .map(drop)
            }
            _ => {
                let info = tool_info(&tool, &input, &self.cwd());
                let kind = serde_json::from_value::<ToolKind>(Value::from(info.kind)).ok();
                if request["requires_user_interaction"] != true
                    && tier_approves(*self.auto_approve.lock(), kind)
                {
                    self.respond(&claude_id, &allow(&input, None));
                    return Ok(());
                }
                let suggestions = request["permission_suggestions"].clone();
                let mut options = vec![option("allow", "Allow", "allow_once")];
                if suggestions.as_array().is_some_and(|list| !list.is_empty())
                    && request["suppress_always_allow_rule"] != true
                {
                    options.push(option("allow_always", "Always allow", "allow_always"));
                }
                options.push(option("reject", "Reject", "reject_once"));
                let mut tool_call = json!({
                    "toolCallId": tool_use_id,
                    "title": info.title,
                    "kind": info.kind,
                    "rawInput": input,
                });
                if !info.content.is_empty() {
                    tool_call["content"] = Value::Array(info.content);
                }
                self.ask(
                    Permission::Approval {
                        claude_id,
                        input,
                        suggestions,
                    },
                    tool_call,
                    Value::Array(options),
                )
                .await
                .map(drop)
            }
        }
    }

    async fn ask(
        &mut self,
        permission: Permission,
        tool_call: Value,
        options: Value,
    ) -> Result<u64, String> {
        let request_id = self.permission_ids.fetch_add(1, Ordering::Relaxed);
        let mut payload = AgentStreamPayload::PermissionRequested {
            request_id,
            tool_call,
            options,
        };
        if validate_payload(&payload).is_err()
            && let AgentStreamPayload::PermissionRequested { tool_call, .. } = &mut payload
            && let Some(fields) = tool_call.as_object_mut()
        {
            fields.remove("rawInput");
            fields.remove("content");
        }
        if let Err(error) = validate_payload(&payload) {
            log::warn!(target: "zz::agent", "Claude Code asked a question too large to show: {error}");
            self.respond(
                permission.claude_id(),
                &deny("zz could not show this request.", false),
            );
            return Ok(request_id);
        }
        self.permissions.insert(request_id, permission);
        self.emit(payload).await?;
        Ok(request_id)
    }

    async fn ask_questions(
        &mut self,
        claude_id: String,
        tool_use_id: &str,
        input: Value,
    ) -> Result<(), String> {
        let questions = input["questions"].as_array().cloned().unwrap_or_default();
        if questions.is_empty() {
            self.respond(&claude_id, &allow(&input, None));
            return Ok(());
        }
        let mut pending = HashSet::new();
        for question in &questions {
            let text = question["question"]
                .as_str()
                .unwrap_or("Question")
                .to_owned();
            let choices = question["options"].as_array().cloned().unwrap_or_default();
            let labels = choices
                .iter()
                .map(|choice| choice["label"].as_str().unwrap_or_default().to_owned())
                .collect::<Vec<_>>();
            let options = choices
                .iter()
                .enumerate()
                .map(|(index, choice)| {
                    let label = choice["label"].as_str().unwrap_or_default();
                    let name = match choice["description"]
                        .as_str()
                        .filter(|text| !text.is_empty())
                    {
                        Some(description) => format!("{label}: {description}"),
                        None => label.to_owned(),
                    };
                    option(&format!("answer-{index}"), &name, "allow_once")
                })
                .collect::<Vec<_>>();
            let tool_call = json!({ "toolCallId": tool_use_id, "title": text });
            let request_id = self
                .ask(
                    Permission::Question {
                        claude_id: claude_id.clone(),
                        question: text,
                        labels,
                    },
                    tool_call,
                    Value::Array(options),
                )
                .await?;
            pending.insert(request_id);
        }
        self.questions.insert(
            claude_id,
            QuestionGroup {
                input,
                answers: Map::new(),
                pending,
            },
        );
        Ok(())
    }

    async fn answer(&mut self, request_id: u64, option_id: Option<String>) -> Result<(), String> {
        let Some(permission) = self.permissions.remove(&request_id) else {
            return Ok(());
        };
        self.emit(AgentStreamPayload::PermissionResolved {
            request_id,
            canceled: option_id.is_none(),
        })
        .await?;
        match permission {
            Permission::Approval {
                claude_id,
                input,
                suggestions,
            } => {
                let response = match option_id.as_deref() {
                    Some("allow") => allow(&input, None),
                    Some("allow_always") => allow(&input, Some(suggestions)),
                    Some(_) => deny("The user rejected this tool use.", false),
                    None => deny("The user cancelled the request.", true),
                };
                self.respond(&claude_id, &response);
            }
            Permission::Plan { claude_id, input } => {
                let response = match option_id.as_deref() {
                    Some("accept_edits") => allow_with_mode(&input, "acceptEdits"),
                    Some("approve") => allow_with_mode(&input, "default"),
                    Some(_) => deny("The user wants to keep planning.", false),
                    None => deny("The user cancelled the request.", true),
                };
                self.respond(&claude_id, &response);
            }
            Permission::Question {
                claude_id,
                question,
                labels,
            } => {
                let label = option_id
                    .as_deref()
                    .and_then(|option| option.strip_prefix("answer-"))
                    .and_then(|index| index.parse::<usize>().ok())
                    .and_then(|index| labels.get(index).cloned());
                let Some(label) = label else {
                    if let Some(group) = self.questions.remove(&claude_id) {
                        for sibling in group.pending {
                            if self.permissions.remove(&sibling).is_some() {
                                self.emit(AgentStreamPayload::PermissionResolved {
                                    request_id: sibling,
                                    canceled: true,
                                })
                                .await?;
                            }
                        }
                    }
                    self.respond(&claude_id, &deny("The user dismissed the question.", false));
                    return Ok(());
                };
                let Some(group) = self.questions.get_mut(&claude_id) else {
                    return Ok(());
                };
                group.pending.remove(&request_id);
                group.answers.insert(question, Value::from(label));
                if group.pending.is_empty()
                    && let Some(group) = self.questions.remove(&claude_id)
                {
                    let mut input = group.input;
                    input["answers"] = Value::Object(group.answers);
                    self.respond(
                        &claude_id,
                        &json!({ "behavior": "allow", "updatedInput": input }),
                    );
                }
            }
        }
        Ok(())
    }

    async fn withdraw(&mut self, claude_id: &str) -> Result<(), String> {
        let withdrawn = self
            .permissions
            .iter()
            .filter(|(_, permission)| permission.claude_id() == claude_id)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        self.questions.remove(claude_id);
        for request_id in withdrawn {
            self.permissions.remove(&request_id);
            self.emit(AgentStreamPayload::PermissionResolved {
                request_id,
                canceled: true,
            })
            .await?;
        }
        Ok(())
    }

    async fn cancel_permissions(&mut self, interrupt: bool) -> Result<(), String> {
        let pending = std::mem::take(&mut self.permissions);
        self.questions.clear();
        let mut answered = HashSet::new();
        for (request_id, permission) in pending {
            if answered.insert(permission.claude_id().to_owned()) {
                self.respond(
                    permission.claude_id(),
                    &deny("The user cancelled the request.", interrupt),
                );
            }
            self.emit(AgentStreamPayload::PermissionResolved {
                request_id,
                canceled: true,
            })
            .await?;
        }
        Ok(())
    }
}

fn side_answer(message_id: &str, answer: &str) -> Value {
    json!({
        "sessionUpdate": "agent_message_chunk",
        "messageId": message_id,
        "content": { "type": "text", "text": answer },
        "_meta": { "zz": { "side": true } },
    })
}

fn spawn_thread(name: &str, body: impl FnOnce() + Send + 'static) -> Result<(), String> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(|_| ())
        .map_err(|error| format!("could not start {name}: {error}"))
}

fn cancelled() -> AgentPromptOutcome {
    AgentPromptOutcome::Finished {
        stop_reason: Value::from("cancelled"),
    }
}

fn option(id: &str, name: &str, kind: &str) -> Value {
    json!({ "optionId": id, "name": name, "kind": kind })
}

fn allow(input: &Value, permissions: Option<Value>) -> Value {
    let mut response = json!({ "behavior": "allow", "updatedInput": input });
    if let Some(permissions) = permissions {
        response["updatedPermissions"] = permissions;
    }
    response
}

fn allow_with_mode(input: &Value, mode: &str) -> Value {
    allow(
        input,
        Some(json!([{ "type": "setMode", "mode": mode, "destination": "session" }])),
    )
}

fn deny(message: &str, interrupt: bool) -> Value {
    let mut response = json!({ "behavior": "deny", "message": message });
    if interrupt {
        response["interrupt"] = Value::Bool(true);
    }
    response
}

fn result_error(frame: &Value) -> String {
    frame["result"]
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            frame["errors"].as_array().map(|errors| {
                errors
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| {
            format!(
                "Claude Code stopped: {}",
                frame["subtype"].as_str().unwrap_or("error")
            )
        })
}

fn prompt_content(prompt: &crate::agent::stream::AgentPrompt) -> Value {
    let mut content = Vec::new();
    if !prompt.text.is_empty() {
        content.push(json!({ "type": "text", "text": prompt.text }));
    }
    for image in &prompt.images {
        content.push(json!({
            "type": "image",
            "source": { "type": "base64", "media_type": image.format, "data": BASE64.encode(&image.data) },
        }));
    }
    Value::Array(content)
}

fn fit_update(mut update: Value) -> Value {
    let fits = |update: &Value| {
        validate_payload(&AgentStreamPayload::Update {
            update: update.clone(),
        })
        .is_ok()
    };
    if fits(&update) {
        return update;
    }
    if let Some(fields) = update.as_object_mut() {
        fields.remove("rawInput");
        fields.remove("rawOutput");
    }
    if fits(&update) {
        return update;
    }
    if update.get("content").is_some_and(Value::is_array) {
        update["content"] = json!([{
            "type": "content",
            "content": { "type": "text", "text": format!("[output larger than {} KiB not shown]", MAX_AGENT_RESULT_BYTES / 1024) },
        }]);
    } else if update["content"]["type"] == "text"
        && let Some(text) = update["content"]["text"].as_str()
    {
        let mut end = MAX_AGENT_RESULT_BYTES / 2;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        update["content"]["text"] = Value::from(format!("{}…", &text[..end]));
    }
    update
}

fn random_u64() -> u64 {
    getrandom::u64().unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos() as u64)
    })
}

fn new_uuid() -> String {
    let high = random_u64();
    let low = random_u64();
    let high = (high & 0xffff_ffff_ffff_0fff) | 0x0000_0000_0000_4000;
    let low = (low & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        high >> 32,
        (high >> 16) & 0xffff,
        high & 0xffff,
        low >> 48,
        low & 0xffff_ffff_ffff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_claude_binary_selects_the_native_driver() {
        assert_eq!(
            ClaudeCommand::parse("claude"),
            Some(ClaudeCommand {
                program: "claude".to_owned(),
                args: Vec::new(),
                env: Vec::new(),
            })
        );
        let command = ClaudeCommand::parse("FOO=1 /opt/bin/claude --model opus").expect("native");
        assert_eq!(command.program, "/opt/bin/claude");
        assert_eq!(command.env, [("FOO".to_owned(), "1".to_owned())]);
        assert_eq!(command.model_flag().as_deref(), Some("opus"));
        assert_eq!(
            ClaudeCommand::parse("npx -y @agentclientprotocol/claude-agent-acp@0.76.0"),
            None
        );
        assert_eq!(ClaudeCommand::parse("claude-agent-acp --stdio"), None);
    }

    #[test]
    fn session_ids_are_v4_uuids() {
        let id = new_uuid();
        assert!(sessions::valid_uuid(&id), "{id}");
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, new_uuid());
    }

    #[test]
    fn settings_offer_models_modes_and_the_current_models_effort() {
        let mut settings = Settings::new(&ClaudeCommand::parse("claude").expect("native"));
        settings.models = vec![
            json!({"value":"default","displayName":"Default","description":"Opus","supportsEffort":true,"supportedEffortLevels":["low","high"],"supportsAutoMode":true}),
            json!({"value":"haiku","displayName":"Haiku","description":"Fast"}),
        ];
        let options = settings.config_options();
        let ids = options
            .as_array()
            .expect("options")
            .iter()
            .map(|option| option["id"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["model", "mode", "effort"]);
        let parsed = serde_json::from_value::<
            Vec<agent_client_protocol::schema::v1::SessionConfigOption>,
        >(options.clone())
        .expect("ACP config options");
        assert_eq!(parsed.len(), 3);
        assert!(
            options[1]["options"]
                .as_array()
                .expect("modes")
                .iter()
                .any(|mode| mode["value"] == "auto")
        );
        settings.model = "haiku".to_owned();
        assert_eq!(settings.config_options().as_array().map(Vec::len), Some(2));
        serde_json::from_value::<agent_client_protocol::schema::v1::SessionModeState>(
            settings.mode_state(),
        )
        .expect("ACP mode state");
    }

    #[test]
    fn oversized_updates_shed_raw_input_before_content() {
        let big = "x".repeat(MAX_AGENT_RESULT_BYTES);
        let update = fit_update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t",
            "title": "Write a",
            "rawInput": { "content": big },
            "content": [{ "type": "content", "content": { "type": "text", "text": "small" } }],
        }));
        assert!(update.get("rawInput").is_none());
        assert_eq!(update["content"][0]["content"]["text"], "small");
        let update = fit_update(json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "m",
            "content": { "type": "text", "text": big },
        }));
        assert!(validate_payload(&AgentStreamPayload::Update { update }).is_ok());
    }

    #[cfg(unix)]
    const FAKE_CLAUDE: &str = r#"#!/bin/sh
read -r init
printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"zz-1","response":{"commands":[{"name":"compact","description":"Compact","argumentHint":""}],"models":[{"value":"default","displayName":"Default","description":"Opus"}],"current_permission_mode":"default"}}}'
read -r prompt
case "$prompt" in *'"text":"hello"'*) ;; *) echo "bad prompt: $prompt" >&2; exit 3 ;; esac
printf '%s\n' '{"type":"system","subtype":"session_state_changed","state":"running"}'
printf '%s\n' '{"type":"assistant","parent_tool_use_id":null,"message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"rm -rf build"}}]}}'
printf '%s\n' '{"type":"control_request","request_id":"perm-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"rm -rf build"},"tool_use_id":"t1","permission_suggestions":[]}}'
read -r answer
case "$answer" in *'"request_id":"perm-1"'*'"behavior":"allow"'*) ;; *) echo "bad answer: $answer" >&2; exit 4 ;; esac
printf '%s\n' '{"type":"user","parent_tool_use_id":null,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"removed"}]},"tool_use_result":{"stdout":"removed","stderr":""}}'
printf '%s\n' '{"type":"assistant","parent_tool_use_id":null,"message":{"id":"m2","content":[{"type":"text","text":"done"}]}}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":5,"output_tokens":2},"modelUsage":{"m":{"contextWindow":1000}},"total_cost_usd":0.01}'
printf '%s\n' '{"type":"system","subtype":"session_state_changed","state":"idle"}'
while read -r line; do :; done
"#;

    #[cfg(unix)]
    #[test]
    fn a_turn_runs_end_to_end_against_a_claude_process() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().expect("tempdir");
        let program = directory.path().join("claude");
        std::fs::write(&program, FAKE_CLAUDE).expect("fake claude");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
        let command = ClaudeCommand::parse(&program.display().to_string()).expect("native");
        let (commands, command_rx) = async_channel::unbounded();
        let (controls, control_rx) = async_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        let runtime = run_claude_runtime(
            command,
            AgentWorkspaceEnvironment::default(),
            AgentProvider::ClaudeCode,
            RuntimeChannels {
                auto_approve: Arc::new(Mutex::new(AgentAutoApprove::Off)),
                permission_ids: Arc::new(AtomicU64::new(1)),
                journal: None,
                commands: command_rx,
                controls: control_rx,
                events,
            },
        );
        let driver = async {
            let next = || async {
                futures_lite::future::or(async { event_rx.recv().await.expect("event") }, async {
                    smol::Timer::after(Duration::from_secs(20)).await;
                    panic!("timed out waiting for the runtime");
                })
                .await
            };
            commands
                .send(RuntimeCommand::Open {
                    cwd: directory.path().to_path_buf(),
                    resume_session: None,
                })
                .await
                .expect("open");
            assert!(
                matches!(next().await, AgentStreamPayload::Ready { agent_key, .. } if agent_key == AGENT_KEY)
            );
            assert!(matches!(
                next().await,
                AgentStreamPayload::SessionReset { restoring: false }
            ));
            let AgentStreamPayload::SessionReady {
                session_id,
                config_options,
                ..
            } = next().await
            else {
                panic!("expected the session to be ready");
            };
            assert!(sessions::valid_uuid(&session_id));
            assert_eq!(config_options.expect("options")[0]["id"], "model");
            assert!(
                matches!(next().await, AgentStreamPayload::Update { update } if update["sessionUpdate"] == "available_commands_update")
            );
            commands
                .send(RuntimeCommand::Prompt {
                    turn_id: 1,
                    prompt: crate::agent::stream::AgentPrompt {
                        text: "hello".to_owned(),
                        ..Default::default()
                    },
                })
                .await
                .expect("prompt");
            assert!(
                matches!(next().await, AgentStreamPayload::Update { update } if update["sessionUpdate"] == "user_message_chunk")
            );
            assert!(matches!(
                next().await,
                AgentStreamPayload::PromptAccepted { turn_id: 1 }
            ));
            assert!(
                matches!(next().await, AgentStreamPayload::Update { update } if update["title"] == "rm -rf build")
            );
            let AgentStreamPayload::PermissionRequested {
                request_id,
                tool_call,
                options,
            } = next().await
            else {
                panic!("expected a permission request");
            };
            let tool_call = serde_json::from_value::<
                agent_client_protocol::schema::v1::ToolCallUpdate,
            >(tool_call)
            .expect("clients decode the tool call");
            assert_eq!(tool_call.tool_call_id.0.as_ref(), "t1");
            let options = serde_json::from_value::<
                Vec<agent_client_protocol::schema::v1::PermissionOption>,
            >(options)
            .expect("clients decode the options");
            assert_eq!(options[0].option_id.0.as_ref(), "allow");
            controls
                .send(RuntimeControl::RespondPermission {
                    request_id,
                    option_id: Some("allow".to_owned()),
                })
                .await
                .expect("answer");
            assert!(matches!(
                next().await,
                AgentStreamPayload::PermissionResolved {
                    canceled: false,
                    ..
                }
            ));
            assert!(
                matches!(next().await, AgentStreamPayload::Update { update } if update["status"] == "completed" && update["content"][0]["content"]["text"] == "removed")
            );
            assert!(
                matches!(next().await, AgentStreamPayload::Update { update } if update["content"]["text"] == "done")
            );
            assert!(
                matches!(next().await, AgentStreamPayload::Update { update } if update["sessionUpdate"] == "usage_update" && update["size"] == 1000)
            );
            assert!(matches!(
                next().await,
                AgentStreamPayload::PromptFinished { turn_id: 1, outcome: AgentPromptOutcome::Finished { stop_reason } } if stop_reason == "end_turn"
            ));
            commands
                .send(RuntimeCommand::Shutdown)
                .await
                .expect("shutdown");
        };
        let (result, ()) = smol::block_on(futures_lite::future::zip(runtime, driver));
        assert_eq!(result, Ok(()));
    }

    #[cfg(unix)]
    const FAKE_SIDE_CLAUDE: &str = r#"#!/bin/sh
read -r init
printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"zz-1","response":{"commands":[],"models":[],"current_permission_mode":"default"}}}'
read -r side
case "$side" in *'"request_id":"zz-2"'*'"subtype":"side_question"'*'"question":"what is 2+2"'*) ;; *) echo "bad side: $side" >&2; exit 3 ;; esac
printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"zz-2","response":{"response":"4","synthetic":false}}}'
while read -r line; do :; done
"#;

    #[cfg(unix)]
    #[test]
    fn side_questions_answer_outside_the_conversation() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().expect("tempdir");
        let program = directory.path().join("claude");
        std::fs::write(&program, FAKE_SIDE_CLAUDE).expect("fake claude");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
        let command = ClaudeCommand::parse(&program.display().to_string()).expect("native");
        let (commands, command_rx) = async_channel::unbounded();
        let (_controls, control_rx) = async_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        let runtime = run_claude_runtime(
            command,
            AgentWorkspaceEnvironment::default(),
            AgentProvider::ClaudeCode,
            RuntimeChannels {
                auto_approve: Arc::new(Mutex::new(AgentAutoApprove::Off)),
                permission_ids: Arc::new(AtomicU64::new(1)),
                journal: None,
                commands: command_rx,
                controls: control_rx,
                events,
            },
        );
        let driver = async {
            let text_of = |payload: &AgentStreamPayload| match payload {
                AgentStreamPayload::Update { update } => {
                    update["content"]["text"].as_str().map(str::to_owned)
                }
                _ => None,
            };
            let next_text = || async {
                loop {
                    let payload = futures_lite::future::or(
                        async { event_rx.recv().await.expect("event") },
                        async {
                            smol::Timer::after(Duration::from_secs(20)).await;
                            panic!("timed out waiting for the runtime");
                        },
                    )
                    .await;
                    if let AgentStreamPayload::Update { update } = &payload
                        && update["sessionUpdate"] == "available_commands_update"
                    {
                        continue;
                    }
                    if let Some(text) = text_of(&payload) {
                        return (text, payload);
                    }
                }
            };
            commands
                .send(RuntimeCommand::Open {
                    cwd: directory.path().to_path_buf(),
                    resume_session: None,
                })
                .await
                .expect("open");
            let verb = |text: &str| RuntimeCommand::Verb {
                prompt: crate::agent::stream::AgentPrompt {
                    text: text.to_owned(),
                    ..Default::default()
                },
            };
            while !matches!(
                event_rx.recv().await.expect("event"),
                AgentStreamPayload::SessionReady { .. }
            ) {}
            commands.send(verb("//btw what is 2+2")).await.expect("btw");
            assert_eq!(next_text().await.0, "//btw what is 2+2");
            let (answer, payload) = next_text().await;
            assert_eq!(answer, "4");
            assert!(
                matches!(payload, AgentStreamPayload::Update { update } if update["_meta"]["zz"]["side"] == true)
            );
            commands.send(verb("//steer left")).await.expect("steer");
            assert_eq!(next_text().await.0, "//steer left");
            assert_eq!(next_text().await.0, "Nothing is running to steer.");
            commands.send(verb("//what")).await.expect("unknown");
            assert_eq!(next_text().await.0, "//what");
            assert_eq!(next_text().await.0, VERB_HELP);
            commands
                .send(RuntimeCommand::Shutdown)
                .await
                .expect("shutdown");
        };
        let (result, ()) = smol::block_on(futures_lite::future::zip(runtime, driver));
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn a_question_answer_and_a_plan_choice_shape_the_reply() {
        let reply = allow_with_mode(&json!({"plan":"p"}), "acceptEdits");
        assert_eq!(reply["updatedPermissions"][0]["mode"], "acceptEdits");
        assert_eq!(deny("no", true)["interrupt"], true);
        assert!(deny("no", false).get("interrupt").is_none());
    }
}
