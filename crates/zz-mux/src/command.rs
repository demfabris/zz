use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::PathBuf,
    str::FromStr as _,
};

use zz_protocol::{
    AgentDescriptor, AgentProvider, Axis, BrowserDescriptor, COMMAND_SPECS, ChooseTreeKind,
    CommandInvocation, CommandSpec, DAEMON_COMMAND_SPECS, DEFAULT_AGENT_AUTO_APPROVE,
    DEFAULT_AGENT_CLAUDE_CODE_COMMAND, DEFAULT_AGENT_COMMAND, DEFAULT_BROWSER_PROFILE,
    EditorDescriptor, KeyToken, MAX_AGENT_COMMAND_BYTES, MAX_GUI_TEXT_BYTES, MuxOptionKey, PaneId,
    PaneKindSnapshot, ServerError, SessionId, TerminalUiCommand, WindowId,
    normalize_browser_profile_name,
};
use zz_terminal::{
    CopyJump, CopyJumpDirection, CopyModeAction, CopyModeCopy, DEFAULT_HISTORY_LIMIT,
    DEFAULT_WORD_SEPARATORS, MAX_HISTORY_LIMIT, PasteBufferAction, SearchDirection,
    TerminalViewAction,
};

use crate::{
    Binding, KeyTables, LayoutPreset, MuxState, PaneDirection, PaneKind, SplitPlacement,
    SplitSize as LayoutSplitSize, StatusContext, StatusFormats, StatusOption, canonical_command,
    command_spec,
    formats::{
        CommandHooks, FormatContext, FormatType, StatusHooks, expand_format_time_with_hooks,
        expand_format_with_hooks,
    },
    layout::PANE_MAXIMUM,
    model::DEFAULT_WINDOW_EXTENT,
    tmux_options::{
        TmuxOption, TmuxOptionScope, UPDATE_ENVIRONMENT_DEFAULT, match_tmux_option,
        parse_tmux_option, tmux_options,
    },
};

const MAX_COPY_COMMAND_BYTES: usize = 8 * 1024;
const MAX_COMMAND_PROMPT_LABEL_BYTES: usize = 1024;
const MAX_COMMAND_PROMPT_TEMPLATE_BYTES: usize = 8 * 1024;
const DEFAULT_DISPLAY_MESSAGE: &str =
    "[#{session_name}] #{window_index}:#{window_name}, current pane #{pane_index}";
const DEFAULT_LIST_COMMANDS_FORMAT: &str =
    "#{command_list_name}#{?command_list_alias, (#{command_list_alias}),} #{command_list_usage}";
pub const DEFAULT_BUFFER_LIMIT: usize = 50;
const MAX_BUFFER_LIMIT: usize = i32::MAX.cast_unsigned() as usize;
const DEFAULT_MESSAGE_LIMIT: usize = 1_000;
const MAX_MESSAGE_LIMIT: usize = i32::MAX.cast_unsigned() as usize;
const DEFAULT_HISTORY_TRICKLE: usize = 2_000;
const MAX_HISTORY_TRICKLE: usize = 10_000;
const DEFAULT_BASE_INDEX: u32 = 0;
const MAX_BASE_INDEX: u32 = i32::MAX.cast_unsigned();
const DEFAULT_PANE_BASE_INDEX: u32 = 0;
const MAX_PANE_BASE_INDEX: u32 = u16::MAX as u32;
const DEFAULT_RENUMBER_WINDOWS: bool = false;
const DEFAULT_PREFIX: &str = "C-b";
const DEFAULT_MOUSE: bool = true;
const DEFAULT_ESCAPE_TIME_MS: u32 = 10;
const DEFAULT_AUTOMATIC_RENAME_FORMAT: &str =
    "#{?pane_in_mode,[tmux],#{pane_current_command}}#{?pane_dead,[dead],}";
const DEFAULT_TERMINAL: &str = "tmux-256color";
const DEFAULT_DISPLAY_TIME_MS: u32 = 750;
const DEFAULT_REPEAT_TIME_MS: u32 = 500;
const MAX_REPEAT_TIME_MS: u32 = 2_000_000;
pub const MAX_WORD_SEPARATORS_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutionContext {
    pub session: Option<SessionId>,
    pub window: Option<WindowId>,
    pub pane: Option<PaneId>,
}

impl ExecutionContext {
    #[must_use]
    pub fn for_pane(state: &MuxState, pane: PaneId) -> Option<Self> {
        let window = state.window_for_pane(pane)?;
        let session = state.windows.get(&window)?.session;
        Some(Self {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MuxEffect {
    PaneCreated {
        pane: PaneId,
        kind: PaneKindSnapshot,
        inherit_cwd_from: Option<PaneId>,
        cwd: Option<String>,
        /// tmux-style shell command for terminal panes; `None` runs the
        /// default shell. Always `None` for other kinds.
        command: Option<String>,
    },
    PaneMaterialized {
        pane: PaneId,
        kind: PaneKindSnapshot,
        inherit_cwd_from: Option<PaneId>,
        cwd: Option<String>,
        command: Option<String>,
    },
    PaneRespawned {
        pane: PaneId,
        cwd: Option<String>,
        command: Option<String>,
        environment: Vec<(String, String)>,
    },
    PanesRemoved(Vec<PaneId>),
    PaneRelocated {
        pane: PaneId,
        from: SessionId,
        to: SessionId,
    },
    SendKeys {
        pane: PaneId,
        keys: Vec<KeyToken>,
    },
    CopyModeRepeat {
        pane: PaneId,
        count: usize,
    },
    TerminalView {
        pane: PaneId,
        action: TerminalViewAction,
    },
    TerminalUi {
        pane: PaneId,
        command: TerminalUiCommand,
    },
    CommandPrompt {
        prompt: String,
        input: String,
        template: Option<String>,
    },
    ChooseTree {
        pane: PaneId,
        kind: ChooseTreeKind,
    },
    FocusSidebar {
        pane: PaneId,
    },
    ChooseBuffer {
        pane: PaneId,
    },
    DisplayPanes {
        pane: PaneId,
        duration_ms: u32,
    },
    DisplayMessage {
        pane: Option<PaneId>,
        text: String,
        duration_ms: u32,
    },
    BufferLimitChanged(usize),
    /// `None` updates every session that inherits the global value.
    WordSeparatorsChanged {
        session: Option<SessionId>,
    },
    /// `None` updates every window after the global default changes.
    ModeKeysChanged {
        window: Option<WindowId>,
    },
    MuxOptionChanged {
        option: MuxOptionKey,
    },
    AgentPaneRestart {
        pane: PaneId,
    },
    StatusFormatsChanged,
    Attach {
        session: SessionId,
        detach_others: bool,
    },
    Detach(DetachScope),
    SourceFile {
        path: String,
        quiet: bool,
    },
    ReloadConfig,
    KillServer,
    SnapshotChanged,
}

/// Which clients `detach-client` hangs up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetachScope {
    /// The client that ran the command.
    Client,
    /// Every attached client except the caller.
    Others,
    /// Every client attached to one session, the caller included.
    Session(SessionId),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Execution {
    pub output: String,
    pub effects: Vec<MuxEffect>,
}

struct ListCommandHooks<'a, H> {
    inner: &'a mut H,
    spec: &'a CommandSpec,
}

impl<H: StatusHooks> StatusHooks for ListCommandHooks<'_, H> {
    fn strftime(&mut self, literal: &str) -> String {
        self.inner.strftime(literal)
    }

    fn shell(&mut self, command: &str) -> String {
        self.inner.shell(command)
    }

    fn variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
        match name {
            "command_list_name" => Some(self.spec.name.to_owned()),
            "command_list_alias" => Some(
                self.spec
                    .aliases
                    .first()
                    .copied()
                    .unwrap_or_default()
                    .to_owned(),
            ),
            "command_list_usage" => Some(self.spec.usage.to_owned()),
            _ => self.inner.variable(name, context),
        }
    }

    fn pane_search(
        &mut self,
        pane: Option<PaneId>,
        pattern: &str,
        regex: bool,
        ignore_case: bool,
    ) -> usize {
        self.inner.pane_search(pane, pattern, regex, ignore_case)
    }
}

impl Execution {
    fn output(output: String) -> Self {
        Self {
            output,
            effects: Vec::new(),
        }
    }

    fn effect(effect: MuxEffect) -> Self {
        Self {
            output: String::new(),
            effects: vec![effect],
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ModeKeys {
    Vi,
    #[default]
    Emacs,
}

impl ModeKeys {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Vi => "vi",
            Self::Emacs => "emacs",
        }
    }

    const fn table(self) -> &'static str {
        match self {
            Self::Vi => "copy-mode-vi",
            Self::Emacs => "copy-mode",
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::Vi => Self::Emacs,
            Self::Emacs => Self::Vi,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SetClipboard {
    On,
    #[default]
    External,
    Off,
}

impl SetClipboard {
    const fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::External => "external",
            Self::Off => "off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TmuxOptionTarget {
    Server,
    GlobalSession,
    Session(SessionId),
    GlobalWindow,
    Window(WindowId),
    Pane(PaneId),
}

type UserOptions = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct EnvironmentEntry {
    value: Option<String>,
    hidden: bool,
}

type Environment = BTreeMap<String, EnvironmentEntry>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RemainOnExit {
    #[default]
    Off,
    On,
    Failed,
    Key,
}

impl RemainOnExit {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Failed => "failed",
            Self::Key => "key",
        }
    }
}

static EMPTY_ENVIRONMENT: Environment = Environment::new();

const NATIVE_OPTIONS: &[&str] = &[
    "history-trickle",
    "experimental-agent-pane",
    "experimental-editor-pane",
    "agent-command",
    "agent-claude-code-command",
    "agent-auto-approve",
];

#[derive(Debug)]
pub struct MuxEngine {
    pub state: MuxState,
    pub keys: KeyTables,
    global_mode_keys: ModeKeys,
    window_mode_keys: BTreeMap<WindowId, ModeKeys>,
    set_clipboard: SetClipboard,
    copy_command: String,
    buffer_limit: usize,
    message_limit: usize,
    history_trickle: usize,
    global_history_limit: usize,
    session_history_limits: BTreeMap<SessionId, usize>,
    global_base_index: u32,
    session_base_indices: BTreeMap<SessionId, u32>,
    global_renumber_windows: bool,
    session_renumber_windows: BTreeMap<SessionId, bool>,
    global_pane_base_index: u32,
    window_pane_base_indices: BTreeMap<WindowId, u32>,
    global_word_separators: String,
    session_word_separators: BTreeMap<SessionId, String>,
    global_mouse: bool,
    session_mouse: BTreeMap<SessionId, bool>,
    escape_time_ms: u32,
    global_automatic_rename_format: String,
    window_automatic_rename_formats: BTreeMap<WindowId, String>,
    global_remain_on_exit: RemainOnExit,
    window_remain_on_exit: BTreeMap<WindowId, RemainOnExit>,
    pane_remain_on_exit: BTreeMap<PaneId, RemainOnExit>,
    default_terminal: Option<String>,
    global_display_time_ms: u32,
    session_display_time_ms: BTreeMap<SessionId, u32>,
    global_repeat_time_ms: u32,
    session_repeat_time_ms: BTreeMap<SessionId, u32>,
    server_user_options: UserOptions,
    global_session_user_options: UserOptions,
    session_user_options: BTreeMap<SessionId, UserOptions>,
    global_window_user_options: UserOptions,
    window_user_options: BTreeMap<WindowId, UserOptions>,
    pane_user_options: BTreeMap<PaneId, UserOptions>,
    global_environment: Environment,
    session_environments: BTreeMap<SessionId, Environment>,
    status: StatusFormats,
    format_host: String,
    format_host_short: String,
    format_pid: u32,
    format_socket_path: String,
    format_start_time: u64,
    format_now: u64,
    format_uid: String,
    format_user: String,
    pane_runtime_facts: BTreeMap<PaneId, PaneRuntimeFacts>,
    experimental_agent_pane: bool,
    experimental_editor_pane: bool,
    agent: AgentOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneRuntimeFacts {
    pub current_command: String,
    pub current_path: String,
    pub reported_path: String,
    pub start_path: String,
    pub pid: Option<u32>,
    pub tty: String,
}

/// What an agent pane's daemon-owned adapter is started with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentOptions {
    pub command: String,
    pub claude_code_command: String,
    pub auto_approve: bool,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            command: DEFAULT_AGENT_COMMAND.to_owned(),
            claude_code_command: DEFAULT_AGENT_CLAUDE_CODE_COMMAND.to_owned(),
            auto_approve: DEFAULT_AGENT_AUTO_APPROVE,
        }
    }
}

impl Default for MuxEngine {
    fn default() -> Self {
        Self {
            state: MuxState::default(),
            keys: KeyTables::default(),
            global_mode_keys: ModeKeys::default(),
            window_mode_keys: BTreeMap::new(),
            set_clipboard: SetClipboard::default(),
            copy_command: String::new(),
            buffer_limit: DEFAULT_BUFFER_LIMIT,
            message_limit: DEFAULT_MESSAGE_LIMIT,
            history_trickle: DEFAULT_HISTORY_TRICKLE,
            global_history_limit: DEFAULT_HISTORY_LIMIT,
            session_history_limits: BTreeMap::new(),
            global_base_index: DEFAULT_BASE_INDEX,
            session_base_indices: BTreeMap::new(),
            global_renumber_windows: DEFAULT_RENUMBER_WINDOWS,
            session_renumber_windows: BTreeMap::new(),
            global_pane_base_index: DEFAULT_PANE_BASE_INDEX,
            window_pane_base_indices: BTreeMap::new(),
            global_word_separators: DEFAULT_WORD_SEPARATORS.to_owned(),
            session_word_separators: BTreeMap::new(),
            global_mouse: DEFAULT_MOUSE,
            session_mouse: BTreeMap::new(),
            escape_time_ms: DEFAULT_ESCAPE_TIME_MS,
            global_automatic_rename_format: DEFAULT_AUTOMATIC_RENAME_FORMAT.to_owned(),
            window_automatic_rename_formats: BTreeMap::new(),
            global_remain_on_exit: RemainOnExit::default(),
            window_remain_on_exit: BTreeMap::new(),
            pane_remain_on_exit: BTreeMap::new(),
            default_terminal: None,
            global_display_time_ms: DEFAULT_DISPLAY_TIME_MS,
            session_display_time_ms: BTreeMap::new(),
            global_repeat_time_ms: DEFAULT_REPEAT_TIME_MS,
            session_repeat_time_ms: BTreeMap::new(),
            server_user_options: UserOptions::new(),
            global_session_user_options: UserOptions::new(),
            session_user_options: BTreeMap::new(),
            global_window_user_options: UserOptions::new(),
            window_user_options: BTreeMap::new(),
            pane_user_options: BTreeMap::new(),
            global_environment: Environment::new(),
            session_environments: BTreeMap::new(),
            status: StatusFormats::default(),
            format_host: String::new(),
            format_host_short: String::new(),
            format_pid: 0,
            format_socket_path: String::new(),
            format_start_time: 0,
            format_now: 0,
            format_uid: String::new(),
            format_user: String::new(),
            pane_runtime_facts: BTreeMap::new(),
            experimental_agent_pane: false,
            experimental_editor_pane: false,
            agent: AgentOptions::default(),
        }
    }
}

impl MuxEngine {
    #[must_use]
    pub const fn buffer_limit(&self) -> usize {
        self.buffer_limit
    }

    #[must_use]
    pub const fn message_limit(&self) -> usize {
        self.message_limit
    }

    #[must_use]
    pub const fn history_trickle(&self) -> usize {
        self.history_trickle
    }

    #[must_use]
    /// The `status-*` options a client's status line is rendered from.
    pub const fn status_formats(&self) -> &StatusFormats {
        &self.status
    }

    pub fn set_format_server_context(
        &mut self,
        host: impl Into<String>,
        host_short: impl Into<String>,
        socket_path: impl Into<String>,
        start_time: u64,
    ) {
        self.format_host = host.into();
        self.format_host_short = host_short.into();
        self.format_socket_path = socket_path.into();
        self.format_start_time = start_time;
        self.format_now = start_time;
    }

    pub const fn set_format_now(&mut self, now: u64) {
        self.format_now = now;
    }

    pub fn set_format_server_identity(
        &mut self,
        pid: u32,
        uid: impl Into<String>,
        user: impl Into<String>,
    ) {
        self.format_pid = pid;
        self.format_uid = uid.into();
        self.format_user = user.into();
    }

    pub fn set_default_mode_keys(&mut self, value: &str) -> Result<(), ServerError> {
        self.global_mode_keys = parse_mode_keys(Some(value), self.global_mode_keys)?;
        Ok(())
    }

    pub fn seed_global_environment<I, K, V>(&mut self, entries: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.global_environment = entries
            .into_iter()
            .map(|(name, value)| {
                (
                    name.into(),
                    EnvironmentEntry {
                        value: Some(value.into()),
                        hidden: false,
                    },
                )
            })
            .collect();
    }

    pub fn mark_session_active(&mut self, session: SessionId) {
        self.state.mark_session_active(session);
    }

    pub fn set_pane_runtime_facts(&mut self, pane: PaneId, facts: PaneRuntimeFacts) -> bool {
        if self.state.pane(pane).is_none() || self.pane_runtime_facts.get(&pane) == Some(&facts) {
            return false;
        }
        self.pane_runtime_facts.insert(pane, facts);
        self.state.bump_generation();
        true
    }

    #[must_use]
    pub fn pane_runtime_facts(&self, pane: PaneId) -> Option<&PaneRuntimeFacts> {
        self.pane_runtime_facts.get(&pane)
    }

    pub(crate) fn format_host(&self) -> &str {
        &self.format_host
    }

    pub(crate) fn format_host_short(&self) -> &str {
        &self.format_host_short
    }

    pub(crate) fn format_socket_path(&self) -> &str {
        &self.format_socket_path
    }

    pub(crate) const fn format_pid(&self) -> u32 {
        self.format_pid
    }

    pub(crate) const fn format_start_time(&self) -> u64 {
        self.format_start_time
    }

    pub(crate) const fn format_now(&self) -> u64 {
        self.format_now
    }

    pub(crate) fn format_uid(&self) -> &str {
        &self.format_uid
    }

    pub(crate) fn format_user(&self) -> &str {
        &self.format_user
    }

    #[must_use]
    pub fn mux_option_value(&self, option: MuxOptionKey) -> String {
        match option {
            MuxOptionKey::Prefix => if self.keys.prefix() == " " {
                "Space"
            } else {
                self.keys.prefix()
            }
            .to_owned(),
            MuxOptionKey::ModeKeys => self.global_mode_keys.as_str().to_owned(),
            MuxOptionKey::HistoryLimit => self.global_history_limit.to_string(),
            MuxOptionKey::WordSeparators => self.global_word_separators.clone(),
            MuxOptionKey::CopyCommand => self.copy_command.clone(),
            MuxOptionKey::SetClipboard => self.set_clipboard.as_str().to_owned(),
            MuxOptionKey::BufferLimit => self.buffer_limit.to_string(),
            MuxOptionKey::HistoryTrickle => self.history_trickle.to_string(),
            MuxOptionKey::SynchronizePanes => if self.state.global_synchronize_panes() {
                "on"
            } else {
                "off"
            }
            .to_owned(),
            MuxOptionKey::ExperimentalAgentPane => if self.experimental_agent_pane {
                "on"
            } else {
                "off"
            }
            .to_owned(),
            MuxOptionKey::ExperimentalEditorPane => if self.experimental_editor_pane {
                "on"
            } else {
                "off"
            }
            .to_owned(),
            MuxOptionKey::AgentCommand => self.agent.command.clone(),
            MuxOptionKey::AgentClaudeCodeCommand => self.agent.claude_code_command.clone(),
            MuxOptionKey::AgentAutoApprove => {
                if self.agent.auto_approve { "on" } else { "off" }.to_owned()
            }
        }
    }

    /// What the daemon starts an agent pane's adapter with.
    #[must_use]
    pub const fn agent_options(&self) -> &AgentOptions {
        &self.agent
    }

    #[must_use]
    pub const fn experimental_agent_pane(&self) -> bool {
        self.experimental_agent_pane
    }

    /// The history limit a newly created pane inherits.
    pub fn history_limit_for_pane(&self, pane: PaneId) -> Result<usize, ServerError> {
        let window = self
            .state
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let session = self.state.windows[&window].session;
        Ok(self.history_limit_for_session(session))
    }

    #[must_use]
    pub fn history_limit_for_session(&self, session: SessionId) -> usize {
        self.session_history_limits
            .get(&session)
            .copied()
            .unwrap_or(self.global_history_limit)
    }

    #[must_use]
    pub fn base_index_for_session(&self, session: SessionId) -> u32 {
        self.session_base_indices
            .get(&session)
            .copied()
            .unwrap_or(self.global_base_index)
    }

    #[must_use]
    pub fn renumber_windows_for_session(&self, session: SessionId) -> bool {
        self.session_renumber_windows
            .get(&session)
            .copied()
            .unwrap_or(self.global_renumber_windows)
    }

    fn pane_base_index_for_window(&self, window: WindowId) -> u32 {
        self.window_pane_base_indices
            .get(&window)
            .copied()
            .unwrap_or(self.global_pane_base_index)
    }

    #[must_use]
    pub fn pane_index(&self, window: WindowId, pane: PaneId) -> Option<u32> {
        let offset = self
            .state
            .windows
            .get(&window)?
            .pane_order()
            .iter()
            .position(|candidate| *candidate == pane)?;
        self.pane_base_index_for_window(window)
            .checked_add(u32::try_from(offset).ok()?)
    }

    fn pane_at_index(&self, window: WindowId, index: u32) -> Option<PaneId> {
        let offset = index.checked_sub(self.pane_base_index_for_window(window))?;
        self.state
            .windows
            .get(&window)?
            .pane_order()
            .get(usize::try_from(offset).ok()?)
            .copied()
    }

    pub fn resolve_window(
        &self,
        target: Option<&str>,
        current_session: Option<SessionId>,
        current_window: Option<WindowId>,
    ) -> Result<WindowId, ServerError> {
        self.state.resolve_window_with_pane_index(
            target,
            current_session,
            current_window,
            &|window, index| self.pane_at_index(window, index),
        )
    }

    pub fn resolve_pane(
        &self,
        target: Option<&str>,
        current_window: Option<WindowId>,
        current_pane: Option<PaneId>,
    ) -> Result<PaneId, ServerError> {
        self.state.resolve_pane_with_index(
            target,
            current_window,
            current_pane,
            &|window, index| self.pane_at_index(window, index),
        )
    }

    /// The effective word-separator string for a pane's session.
    pub fn word_separators_for_pane(&self, pane: PaneId) -> Result<&str, ServerError> {
        let window = self
            .state
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let session = self.state.windows[&window].session;
        Ok(self.word_separators_for_session(session))
    }

    #[must_use]
    pub fn word_separators_for_session(&self, session: SessionId) -> &str {
        self.session_word_separators
            .get(&session)
            .map_or(self.global_word_separators.as_str(), String::as_str)
    }

    #[must_use]
    pub fn default_terminal_for_spawn(&self) -> Option<&str> {
        self.default_terminal.as_deref()
    }

    #[must_use]
    pub fn repeat_time_for_session(&self, session: SessionId) -> u32 {
        self.session_repeat_time_ms
            .get(&session)
            .copied()
            .unwrap_or(self.global_repeat_time_ms)
    }

    pub fn display_time_for_pane(&self, pane: PaneId) -> Result<u32, ServerError> {
        let window = self
            .state
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let session = self.state.windows[&window].session;
        Ok(self.display_time_for_session(session))
    }

    pub fn retain_exited_pane(&self, pane: PaneId, failed: bool) -> Result<bool, ServerError> {
        Ok(match self.remain_on_exit_for_pane(pane)? {
            RemainOnExit::Off => false,
            RemainOnExit::On | RemainOnExit::Key => true,
            RemainOnExit::Failed => failed,
        })
    }

    #[must_use]
    pub fn dead_pane_dismisses_on_key(&self, pane: PaneId) -> bool {
        self.state.pane(pane).is_some_and(|pane| pane.dead)
            && self
                .remain_on_exit_for_pane(pane)
                .is_ok_and(|value| value == RemainOnExit::Key)
    }

    fn display_time_for_session(&self, session: SessionId) -> u32 {
        self.session_display_time_ms
            .get(&session)
            .copied()
            .unwrap_or(self.global_display_time_ms)
    }

    fn mouse_for_session(&self, session: SessionId) -> bool {
        self.session_mouse
            .get(&session)
            .copied()
            .unwrap_or(self.global_mouse)
    }

    fn automatic_rename_format_for_window(&self, window: WindowId) -> &str {
        self.window_automatic_rename_formats
            .get(&window)
            .map_or(self.global_automatic_rename_format.as_str(), String::as_str)
    }

    fn remain_on_exit_for_window(&self, window: WindowId) -> RemainOnExit {
        self.window_remain_on_exit
            .get(&window)
            .copied()
            .unwrap_or(self.global_remain_on_exit)
    }

    fn remain_on_exit_for_pane(&self, pane: PaneId) -> Result<RemainOnExit, ServerError> {
        let window = self
            .state
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        Ok(self
            .pane_remain_on_exit
            .get(&pane)
            .copied()
            .unwrap_or_else(|| self.remain_on_exit_for_window(window)))
    }

    pub fn environment_for_session(
        &self,
        session: SessionId,
    ) -> Result<Vec<(String, Option<String>)>, ServerError> {
        if !self.state.sessions.contains_key(&session) {
            return Err(ServerError::MissingTarget(session.to_string()));
        }
        let mut environment = BTreeMap::new();
        for (name, entry) in &self.global_environment {
            let value = if entry.hidden {
                None
            } else {
                entry.value.clone()
            };
            environment.insert(name.clone(), value);
        }
        if let Some(overlay) = self.session_environments.get(&session) {
            for (name, entry) in overlay {
                let value = if entry.hidden {
                    None
                } else {
                    entry.value.clone()
                };
                environment.insert(name.clone(), value);
            }
        }
        Ok(environment.into_iter().collect())
    }

    /// The effective native copy-mode key table for a pane's window.
    pub fn copy_mode_table_for_pane(&self, pane: PaneId) -> Result<&'static str, ServerError> {
        let window = self
            .state
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        Ok(self.mode_keys_for_window(window).table())
    }

    fn mode_keys_for_window(&self, window: WindowId) -> ModeKeys {
        self.window_mode_keys
            .get(&window)
            .copied()
            .unwrap_or(self.global_mode_keys)
    }

    /// Resolve the panes that should receive input originating at `source`.
    pub fn synchronized_input_targets(&self, source: PaneId) -> Result<Vec<PaneId>, ServerError> {
        self.state.synchronized_input_targets(source)
    }

    /// Apply one parsed command and return side effects for the daemon adapter.
    pub fn execute(
        &mut self,
        context: &mut ExecutionContext,
        command: &CommandInvocation,
    ) -> Result<Execution, ServerError> {
        let mut hooks = CommandHooks::new(self.format_now);
        self.execute_with_format_hooks(context, command, &mut hooks)
    }

    pub fn execute_with_format_hooks(
        &mut self,
        context: &mut ExecutionContext,
        command: &CommandInvocation,
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let generation = self.state.generation();
        let name = canonical_command(&command.name);
        let mut execution = match name {
            "new-session" => self.new_session(context, &command.args, hooks)?,
            "list-sessions" => self.list_sessions(context, &command.args, hooks)?,
            "rename-session" => self.rename_session(context, &command.args)?,
            "kill-session" => self.kill_session(context, &command.args)?,
            "attach-session" => self.attach_session(context, &command.args)?,
            "has-session" => self.has_session(context, &command.args)?,
            "detach-client" => self.detach_client(context, &command.args)?,
            "list-clients" | "refresh-client" | "show-messages" => {
                parse_command_options(name, &command.args)?;
                return Err(ServerError::UnsupportedCommand(name.to_owned()));
            }
            "new-window" => self.new_window(context, &command.args, PaneKind::Terminal, hooks)?,
            "new-browser" => {
                let (options, positional) = parse_command_options("new-browser", &command.args)?;
                let browser = browser_from_args(&options, &positional)?;
                self.new_window_with_options(
                    context,
                    &options,
                    PaneKind::Browser(browser),
                    None,
                    hooks,
                )?
            }
            "list-windows" => self.list_windows(context, &command.args, hooks)?,
            "rename-window" => self.rename_window(context, &command.args)?,
            "select-window" => self.select_window(context, &command.args)?,
            "next-window" => self.step_window(context, &command.args, 1)?,
            "previous-window" => self.step_window(context, &command.args, -1)?,
            "last-window" => self.last_window(context, &command.args)?,
            "kill-window" => self.kill_window(context, &command.args)?,
            "move-window" => self.move_window(context, &command.args)?,
            "swap-window" => self.swap_window(context, &command.args)?,
            "find-window" => self.find_window(context, &command.args)?,
            "split-picker" => self.split_picker(context, &command.args, hooks)?,
            "split-window" => self.split_window(context, &command.args, None, hooks)?,
            "split-browser" => self.split_browser(context, &command.args, hooks)?,
            "select-pane-kind" => self.select_pane_kind(context, &command.args)?,
            "break-pane" => self.break_pane(context, &command.args)?,
            "join-pane" | "move-pane" => self.join_pane(context, &command.args, name)?,
            "set-browser-url" => self.set_browser_url(context, &command.args)?,
            "set-browser-tabs" => self.set_browser_tabs(context, &command.args)?,
            "set-browser-profile" => self.set_browser_profile(context, &command.args)?,
            "set-agent-session" => self.set_agent_session(context, &command.args)?,
            "set-agent-provider" => self.set_agent_provider(context, &command.args)?,
            "restart-agent-pane" => self.restart_agent_pane(context, &command.args)?,
            "set-editor-path" => self.set_editor_path(context, &command.args)?,
            "select-pane" => self.select_pane(context, &command.args)?,
            "last-pane" => self.last_pane(context, &command.args)?,
            "swap-pane" => self.swap_pane(context, &command.args)?,
            "list-panes" => self.list_panes(context, &command.args, hooks)?,
            "resize-pane" => self.resize_pane(context, &command.args)?,
            "select-layout" | "next-layout" | "previous-layout" => {
                self.select_layout(context, &command.args, name)?
            }
            "rotate-window" => self.rotate_window(context, &command.args)?,
            "kill-pane" => self.kill_pane(context, &command.args)?,
            "respawn-pane" => self.respawn_pane(context, &command.args, hooks)?,
            "respawn-window" => self.respawn_window(context, &command.args, hooks)?,
            "send-keys" => self.send_keys(context, &command.args)?,
            "send-prefix" => self.send_prefix(context, &command.args)?,
            "copy-mode" => self.copy_mode(context, &command.args)?,
            "copy-mode-search-prompt" => self.copy_mode_search_prompt(context, &command.args)?,
            "command-prompt" => self.command_prompt(context, &command.args)?,
            "focus-sidebar" => self.focus_sidebar(context, &command.args)?,
            "choose-tree" => self.choose_tree(context, &command.args)?,
            "choose-buffer" => self.choose_buffer(context, &command.args)?,
            "display-message" => self.display_message(context, &command.args, hooks)?,
            "display-panes" => self.display_panes(context, &command.args)?,
            "clear-history" => self.clear_history(context, &command.args)?,
            "bind-key" => self.bind_key(&command.args)?,
            "unbind-key" => self.unbind_key(&command.args)?,
            "list-keys" => self.list_keys(&command.args)?,
            "list-commands" => self.list_commands(context, &command.args, hooks)?,
            "set-option" => self.set_option(context, &command.args, false)?,
            "set-window-option" => self.set_option(context, &command.args, true)?,
            "show-options" => self.show_options(context, &command.args, false)?,
            "show-window-options" => self.show_options(context, &command.args, true)?,
            "set-environment" => self.set_environment(context, &command.args, hooks)?,
            "show-environment" => self.show_environment(context, &command.args)?,
            "source-file" => Self::source_file(&command.args)?,
            "reload-config" => {
                parse_command_options("reload-config", &command.args)?;
                if command.args.is_empty() {
                    Execution::effect(MuxEffect::ReloadConfig)
                } else {
                    return Err(ServerError::InvalidCommand(
                        "reload-config does not take arguments".to_owned(),
                    ));
                }
            }
            "start-server" => {
                let (_, positional) = parse_command_options("start-server", &command.args)?;
                reject_positionals("start-server", &positional)?;
                Execution::default()
            }
            "kill-server" => {
                parse_command_options("kill-server", &command.args)?;
                Execution::effect(MuxEffect::KillServer)
            }
            _ => return Err(ServerError::UnsupportedCommand(command.name.clone())),
        };

        if self.state.generation() != generation {
            execution.effects.push(MuxEffect::SnapshotChanged);
        }
        self.session_history_limits
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_base_indices
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_renumber_windows
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_word_separators
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_mouse
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_display_time_ms
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_repeat_time_ms
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_user_options
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.window_user_options
            .retain(|window, _| self.state.windows.contains_key(window));
        self.pane_user_options
            .retain(|pane, _| self.state.pane(*pane).is_some());
        self.session_environments
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.window_mode_keys
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_automatic_rename_formats
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_remain_on_exit
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_pane_base_indices
            .retain(|window, _| self.state.windows.contains_key(window));
        self.pane_runtime_facts
            .retain(|pane, _| self.state.pane(*pane).is_some());
        self.pane_remain_on_exit
            .retain(|pane, _| self.state.pane(*pane).is_some());
        self.repair_context(context);
        Ok(execution)
    }

    fn new_session(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("new-session", args)?;
        let command = shell_command_positional(&positional);
        let detached = options.has("-d");
        if options.has("-A") {
            let existing = match options.value("-s") {
                Some(name) => session_named(&self.state, name),
                None => self.state.resolve_session(None, context.session).ok(),
            };
            if let Some(session) = existing {
                let window = session_active_window(&self.state, session)?;
                let pane = window_active_pane(&self.state, window)?;
                *context = ExecutionContext {
                    session: Some(session),
                    window: Some(window),
                    pane: Some(pane),
                };
                return Ok(Execution::effect(MuxEffect::Attach {
                    session,
                    detach_others: options.has("-D"),
                }));
            }
        }
        let (inherit_cwd_from, cwd) =
            spawn_cwd_source(self, &options, context.pane, &PaneKind::Terminal, hooks);
        let name = options
            .value("-s")
            .map_or_else(|| next_session_name(&self.state), str::to_owned);
        let extent = initial_window_extent(&options)?;
        let base_index = self.global_base_index;
        let (session, window, pane) = self
            .state
            .create_session_with_extent_at(name, extent, base_index)?;
        self.seed_session_environment(session);
        self.state
            .sessions
            .get_mut(&session)
            .expect("new session exists")
            .created = i64::try_from(self.format_now)
            .ok()
            .filter(|created| *created != 0);
        if let Some(window_name) = options.value("-n") {
            window_name.clone_into(
                &mut self
                    .state
                    .windows
                    .get_mut(&window)
                    .expect("new window exists")
                    .name,
            );
            self.state
                .set_window_automatic_rename(window, Some(false))?;
        }
        *context = ExecutionContext {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
        };
        let mut effects = vec![MuxEffect::PaneCreated {
            pane,
            kind: PaneKindSnapshot::Terminal,
            inherit_cwd_from,
            cwd,
            command,
        }];
        if !detached {
            effects.push(MuxEffect::Attach {
                session,
                detach_others: false,
            });
        }
        Ok(Execution {
            output: String::new(),
            effects,
        })
    }

    fn list_sessions(
        &self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-sessions", args)?;
        reject_positionals("list-sessions", &positional)?;
        if let Some(format) = options.value("-F") {
            let output = self
                .state
                .sessions_by_name()
                .into_iter()
                .map(|session| {
                    expand_format_with_hooks(
                        format,
                        self,
                        FormatContext {
                            session: Some(session.id),
                            window: None,
                            pane: None,
                            active_session: context.session,
                            format_type: FormatType::Session,
                        },
                        hooks,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(Execution::output(output));
        }
        let output = self
            .state
            .sessions_by_name()
            .into_iter()
            .map(|session| {
                format!(
                    "{}: {} windows (id {})",
                    session.name,
                    session.windows.len(),
                    session.id
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Execution::output(output))
    }

    fn rename_session(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("rename-session", args)?;
        let name = exactly_one_argument("rename-session", &positional)?;
        let session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        self.state.rename_session(session, name)?;
        Ok(Execution::default())
    }

    fn kill_session(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("kill-session", args)?;
        reject_positionals("kill-session", &positional)?;
        let session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        if options.has("-C") {
            let panes = self
                .state
                .sessions
                .get(&session)
                .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?
                .windows
                .iter()
                .filter_map(|window| self.state.windows.get(window))
                .flat_map(|window| window.pane_order().to_vec())
                .collect::<Vec<_>>();
            for pane in panes {
                self.state.set_pane_bell(pane, false);
            }
            return Ok(Execution::default());
        }
        let targets = if options.has("-a") {
            self.state
                .sessions
                .keys()
                .copied()
                .filter(|candidate| *candidate != session)
                .collect()
        } else {
            vec![session]
        };
        let mut panes = Vec::new();
        for target in targets {
            panes.extend(self.state.kill_session(target)?);
        }
        Ok(Execution::effect(MuxEffect::PanesRemoved(panes)))
    }

    fn attach_session(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("attach-session", args)?;
        reject_positionals("attach-session", &positional)?;
        let detach_others = options.has("-d");
        let session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        let window = session_active_window(&self.state, session)?;
        let pane = window_active_pane(&self.state, window)?;
        *context = ExecutionContext {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
        };
        Ok(Execution::effect(MuxEffect::Attach {
            session,
            detach_others,
        }))
    }

    fn detach_client(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("detach-client", args)?;
        reject_positionals("detach-client", &positional)?;
        let scope = match options.value("-s") {
            Some(target) => {
                DetachScope::Session(self.state.resolve_session(Some(target), context.session)?)
            }
            None if options.has("-a") => DetachScope::Others,
            None => DetachScope::Client,
        };
        Ok(Execution::effect(MuxEffect::Detach(scope)))
    }

    fn has_session(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("has-session", args)?;
        reject_positionals("has-session", &positional)?;
        self.state
            .resolve_session(options.value("-t"), context.session)?;
        Ok(Execution::default())
    }

    fn new_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        kind: PaneKind,
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("new-window", args)?;
        let command = shell_command_positional(&positional);
        self.new_window_with_options(context, &options, kind, command, hooks)
    }

    fn new_window_with_options(
        &mut self,
        context: &mut ExecutionContext,
        options: &Options,
        kind: PaneKind,
        command: Option<String>,
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let destination = window_destination(&self.state, options.value("-t"), context)?;
        let session = destination.session;
        let selects = !options.has("-d");
        let name = options.value("-n");
        if options.has("-S")
            && let Some(existing) = name.and_then(|name| self.state.window_named(session, name))
        {
            if selects {
                self.state.select_window(session, existing)?;
                *context = ExecutionContext {
                    session: Some(session),
                    window: Some(existing),
                    pane: Some(window_active_pane(&self.state, existing)?),
                };
            }
            return Ok(Execution::default());
        }
        let context_pane_in_session = context.pane.filter(|pane| {
            self.state
                .window_for_pane(*pane)
                .and_then(|window| self.state.windows.get(&window))
                .is_some_and(|window| window.session == session)
        });
        let origin = match context_pane_in_session {
            Some(pane) => Some(pane),
            None => Some(window_active_pane(
                &self.state,
                session_active_window(&self.state, session)?,
            )?),
        };
        let (inherit_cwd_from, cwd) = spawn_cwd_source(self, options, origin, &kind, hooks);
        let index = if options.has("-a") {
            let target = match destination.index {
                Some(index) => index,
                None => window_index(&self.state, session_active_window(&self.state, session)?)?,
            };
            if self.state.window_at_index(session, target).is_some() {
                let index = target.checked_add(1).ok_or_else(|| {
                    ServerError::InvalidCommand("no free window index".to_owned())
                })?;
                self.state.shift_windows_up(session, index)?;
                Some(index)
            } else {
                Some(target)
            }
        } else {
            destination.index
        };
        let replaced = match index {
            Some(index) if options.has("-k") => self.state.window_at_index(session, index),
            _ => None,
        };
        let snapshot_kind = pane_kind_snapshot(&kind);
        let mut effects = Vec::new();
        let window_name = name
            .map(str::to_owned)
            .or_else(|| index.map(|index| index.to_string()));
        let base_index = self.base_index_for_session(session);
        if let Some(index) = index
            && replaced.is_none()
            && self.state.window_at_index(session, index).is_some()
        {
            return Err(ServerError::InvalidCommand(format!(
                "create window failed: index {index} in use"
            )));
        }
        let (window, pane) = self.state.create_window_at_with_base_index(
            session,
            index.filter(|_| replaced.is_none()),
            window_name,
            kind,
            selects,
            base_index,
        )?;
        if name.is_some() {
            self.state
                .set_window_automatic_rename(window, Some(false))?;
        }
        if let Some(replaced) = replaced {
            effects.push(MuxEffect::PanesRemoved(self.state.kill_window(replaced)?));
            self.state
                .set_window_index(window, index.expect("replacing an occupied index"))?;
        }
        if selects {
            *context = ExecutionContext {
                session: Some(session),
                window: Some(window),
                pane: Some(pane),
            };
        }
        effects.push(MuxEffect::PaneCreated {
            pane,
            kind: snapshot_kind,
            inherit_cwd_from,
            cwd,
            command,
        });
        Ok(Execution {
            output: String::new(),
            effects,
        })
    }

    fn list_windows(
        &self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-windows", args)?;
        reject_positionals("list-windows", &positional)?;
        let target_session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        let session_ids = if options.has("-a") {
            self.state
                .sessions_by_name()
                .into_iter()
                .map(|session| session.id)
                .collect::<Vec<_>>()
        } else {
            vec![target_session]
        };
        let mut output = Vec::new();
        for session_id in session_ids {
            let session = self
                .state
                .sessions
                .get(&session_id)
                .ok_or_else(|| ServerError::MissingTarget(session_id.to_string()))?;
            for window_id in &session.windows {
                let Some(window) = self.state.windows.get(window_id) else {
                    continue;
                };
                if let Some(format) = options.value("-F") {
                    output.push(expand_format_with_hooks(
                        format,
                        self,
                        FormatContext {
                            session: Some(session_id),
                            window: Some(window.id),
                            pane: None,
                            active_session: context.session,
                            format_type: FormatType::Window,
                        },
                        hooks,
                    ));
                    continue;
                }
                let active = if window.id == session.active_window {
                    '*'
                } else {
                    '-'
                };
                let prefix = if options.has("-a") {
                    format!("{}:", session.name)
                } else {
                    String::new()
                };
                output.push(format!(
                    "{prefix}{}: {}{} ({} panes) [id {}]",
                    window.index,
                    window.name,
                    active,
                    window.panes.len(),
                    window.id
                ));
            }
        }
        let output = output.join("\n");
        Ok(Execution::output(output))
    }

    fn rename_window(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("rename-window", args)?;
        let name = exactly_one_argument("rename-window", &positional)?;
        let window = self.resolve_window(options.value("-t"), context.session, context.window)?;
        self.state.rename_window(window, name)?;
        self.state
            .set_window_automatic_rename(window, Some(false))?;
        Ok(Execution::default())
    }

    fn select_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("select-window", args)?;
        reject_positionals("select-window", &positional)?;
        let target = options.value("-t");
        if options.has("-n") || options.has("-p") {
            let session = self.session_of_window_target(target, context)?;
            let direction = if options.has("-n") { 1 } else { -1 };
            return self.step_window_in_session(context, session, direction, false);
        }
        if options.has("-l") {
            let session = self.session_of_window_target(target, context)?;
            return self.activate_last_window(context, session);
        }
        let window = self.resolve_window(target, context.session, context.window)?;
        let session = self
            .state
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
            .session;
        if options.has("-T") && session_active_window(&self.state, session)? == window {
            return self.activate_last_window(context, session);
        }
        self.state.select_window(session, window)?;
        context.session = Some(session);
        context.window = Some(window);
        context.pane = Some(window_active_pane(&self.state, window)?);
        Ok(Execution::default())
    }

    fn session_of_window_target(
        &self,
        target: Option<&str>,
        context: &ExecutionContext,
    ) -> Result<SessionId, ServerError> {
        let Some(target) = target else {
            return self.state.resolve_session(None, context.session);
        };
        let window = self.resolve_window(Some(target), context.session, context.window)?;
        self.state
            .windows
            .get(&window)
            .map(|window| window.session)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))
    }

    fn last_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, _) = parse_command_options("last-window", args)?;
        let session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        self.activate_last_window(context, session)
    }

    fn activate_last_window(
        &mut self,
        context: &mut ExecutionContext,
        session: SessionId,
    ) -> Result<Execution, ServerError> {
        let window = self.state.last_window(session)?;
        self.state.select_window(session, window)?;
        context.session = Some(session);
        context.window = Some(window);
        context.pane = Some(window_active_pane(&self.state, window)?);
        Ok(Execution::default())
    }

    fn step_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        direction: isize,
    ) -> Result<Execution, ServerError> {
        let command = if direction > 0 {
            "next-window"
        } else {
            "previous-window"
        };
        let (options, positional) = parse_command_options(command, args)?;
        reject_positionals(command, &positional)?;
        let session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        self.step_window_in_session(context, session, direction, options.has("-a"))
    }

    fn step_window_in_session(
        &mut self,
        context: &mut ExecutionContext,
        session: SessionId,
        direction: isize,
        alerted_only: bool,
    ) -> Result<Execution, ServerError> {
        let state = self
            .state
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        let len = isize::try_from(state.windows.len()).expect("window count fits isize");
        if len == 0 {
            return Err(ServerError::MissingTarget(session.to_string()));
        }
        let current = context
            .window
            .filter(|window| state.windows.contains(window))
            .unwrap_or(state.active_window);
        let current_index = isize::try_from(
            state
                .windows
                .iter()
                .position(|window| *window == current)
                .unwrap_or(0),
        )
        .expect("index fits isize");
        let step_to = |steps: isize| {
            let index = (current_index + steps * direction).rem_euclid(len);
            state.windows[usize::try_from(index).expect("nonnegative index")]
        };
        let no_window = || {
            ServerError::InvalidCommand(format!(
                "no {} window",
                if direction > 0 { "next" } else { "previous" }
            ))
        };
        let window = if alerted_only {
            (1..=len)
                .map(step_to)
                .find(|window| self.window_alerted(*window))
                .ok_or_else(no_window)?
        } else {
            step_to(1)
        };
        if window == current {
            return Err(no_window());
        }
        self.state.select_window(session, window)?;
        context.session = Some(session);
        context.window = Some(window);
        context.pane = Some(window_active_pane(&self.state, window)?);
        Ok(Execution::default())
    }

    fn window_alerted(&self, window: WindowId) -> bool {
        self.state
            .windows
            .get(&window)
            .is_some_and(|window| window.panes.values().any(|pane| pane.bell))
    }

    fn renumber_session_if_enabled(&mut self, session: SessionId) -> Result<(), ServerError> {
        if !self.state.sessions.contains_key(&session)
            || !self.renumber_windows_for_session(session)
        {
            return Ok(());
        }
        let base_index = self.base_index_for_session(session);
        self.state.renumber_windows(session, base_index)
    }

    fn kill_window(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("kill-window", args)?;
        reject_positionals("kill-window", &positional)?;
        let window = self.resolve_window(options.value("-t"), context.session, context.window)?;
        let targets = if options.has("-a") {
            let session = self
                .state
                .windows
                .get(&window)
                .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
                .session;
            self.state
                .sessions
                .get(&session)
                .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?
                .windows
                .iter()
                .copied()
                .filter(|candidate| *candidate != window)
                .collect()
        } else {
            vec![window]
        };
        let sessions = targets
            .iter()
            .filter_map(|target| self.state.windows.get(target).map(|window| window.session))
            .collect::<BTreeSet<_>>();
        for session in &sessions {
            if self.renumber_windows_for_session(*session) {
                let removed_windows = targets
                    .iter()
                    .filter(|target| {
                        self.state
                            .windows
                            .get(target)
                            .is_some_and(|window| window.session == *session)
                    })
                    .count();
                self.state.validate_renumber_capacity(
                    *session,
                    self.base_index_for_session(*session),
                    removed_windows,
                )?;
            }
        }
        let mut panes = Vec::new();
        for target in targets {
            panes.extend(self.state.kill_window(target)?);
        }
        for session in sessions {
            self.renumber_session_if_enabled(session)?;
        }
        Ok(Execution::effect(MuxEffect::PanesRemoved(panes)))
    }

    fn move_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("move-window", args)?;
        reject_positionals("move-window", &positional)?;
        if options.has("-r") {
            if options.value("-s").is_some() {
                self.resolve_window(options.value("-s"), context.session, context.window)?;
            }
            let session = self
                .state
                .resolve_session(options.value("-t"), context.session)?;
            let base_index = self.base_index_for_session(session);
            self.state.renumber_windows(session, base_index)?;
            return Ok(Execution::default());
        }

        let source = self.resolve_window(options.value("-s"), context.session, context.window)?;
        let source_session = self.state.windows[&source].session;
        let source_panes = self.state.windows[&source].pane_order().to_vec();
        let destination = window_destination(&self.state, options.value("-t"), context)?;
        let destination_session = destination.session;
        let base_index = self.base_index_for_session(destination_session);
        if options.value("-s").is_none() && self.renumber_windows_for_session(source_session) {
            let removed_windows = usize::from(source_session != destination_session);
            self.state.validate_renumber_capacity(
                source_session,
                self.base_index_for_session(source_session),
                removed_windows,
            )?;
        }
        let destination_index = if options.has("-a") || options.has("-b") {
            let target = destination
                .index
                .and_then(|index| self.state.window_at_index(destination_session, index))
                .unwrap_or(self.state.sessions[&destination_session].active_window);
            let target_index = self.state.windows[&target].index;
            let index = if options.has("-b") {
                target_index
            } else {
                target_index
                    .checked_add(1)
                    .ok_or_else(|| ServerError::InvalidCommand("no free window index".to_owned()))?
            };
            self.state.shift_windows_up(destination_session, index)?;
            index
        } else {
            destination.index.map_or_else(
                || {
                    self.state
                        .next_window_index(destination_session, base_index)
                },
                Ok,
            )?
        };

        let detached = options.has("-d");
        let original_context = context.clone();
        let removed = self.state.move_window(
            source,
            destination_session,
            destination_index,
            options.has("-k"),
            !detached,
        )?;
        if options.value("-s").is_none() {
            self.renumber_session_if_enabled(source_session)?;
        }

        if detached && original_context.window == Some(source) {
            let window = if self.state.sessions.contains_key(&source_session) {
                session_active_window(&self.state, source_session)?
            } else {
                session_active_window(&self.state, destination_session)?
            };
            *context =
                ExecutionContext::for_pane(&self.state, window_active_pane(&self.state, window)?)
                    .expect("moved window leaves a valid command context");
        } else if !detached {
            *context =
                ExecutionContext::for_pane(&self.state, window_active_pane(&self.state, source)?)
                    .expect("moved window has an active pane");
        }

        let mut effects = Vec::new();
        if !removed.is_empty() {
            effects.push(MuxEffect::PanesRemoved(removed));
        }
        if source_session != destination_session {
            effects.extend(
                source_panes
                    .into_iter()
                    .map(|pane| MuxEffect::PaneRelocated {
                        pane,
                        from: source_session,
                        to: destination_session,
                    }),
            );
        }
        Ok(Execution {
            output: String::new(),
            effects,
        })
    }

    fn swap_window(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("swap-window", args)?;
        reject_positionals("swap-window", &positional)?;
        let source = self.resolve_window(options.value("-s"), context.session, context.window)?;
        let target = self.resolve_window(options.value("-t"), context.session, context.window)?;
        if source == target {
            return Ok(Execution::default());
        }
        let source_session = self.state.windows[&source].session;
        let target_session = self.state.windows[&target].session;
        let source_panes = self.state.windows[&source].pane_order().to_vec();
        let target_panes = self.state.windows[&target].pane_order().to_vec();
        self.state.swap_windows(source, target, options.has("-d"))?;

        let mut effects = Vec::new();
        if source_session != target_session {
            effects.extend(
                source_panes
                    .into_iter()
                    .map(|pane| MuxEffect::PaneRelocated {
                        pane,
                        from: source_session,
                        to: target_session,
                    }),
            );
            effects.extend(
                target_panes
                    .into_iter()
                    .map(|pane| MuxEffect::PaneRelocated {
                        pane,
                        from: target_session,
                        to: source_session,
                    }),
            );
        }
        Ok(Execution {
            output: String::new(),
            effects,
        })
    }

    fn find_window(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("find-window", args)?;
        if positional.len() != 1 {
            return Err(ServerError::InvalidCommand(
                "find-window requires exactly one match string".to_owned(),
            ));
        }
        self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        Ok(Execution::default())
    }

    fn split_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        kind: Option<PaneKind>,
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("split-window", args)?;
        let command = shell_command_positional(&positional);
        self.split_window_with_options(
            context,
            &options,
            kind.unwrap_or(PaneKind::Terminal),
            command,
            split_size(&options),
            hooks,
        )
    }

    fn split_picker(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("split-picker", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "split-picker does not accept positional arguments".to_owned(),
            ));
        }
        let target = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let (inherit_cwd_from, _) =
            spawn_cwd_source(self, &options, Some(target), &PaneKind::Terminal, hooks);
        self.split_window_with_options(
            context,
            &options,
            PaneKind::Picker { inherit_cwd_from },
            None,
            split_size(&options),
            hooks,
        )
    }

    fn split_browser(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("split-browser", args)?;
        let browser = browser_from_args(&options, &positional)?;
        self.split_window_with_options(
            context,
            &options,
            PaneKind::Browser(browser),
            None,
            None,
            hooks,
        )
    }

    fn select_pane_kind(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("select-pane-kind", args)?;
        let [selection] = positional.as_slice() else {
            return Err(ServerError::InvalidCommand(
                "select-pane-kind requires exactly one of: terminal, browser, agent, editor"
                    .to_owned(),
            ));
        };
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let agent_cwd = options.value("-c").map(PathBuf::from);
        if agent_cwd.as_ref().is_some_and(|cwd| {
            !cwd.is_absolute() || cwd.as_os_str().as_encoded_bytes().len() > MAX_GUI_TEXT_BYTES
        }) {
            return Err(ServerError::InvalidCommand(
                "agent working directory must be absolute and stay inside the wire limit"
                    .to_owned(),
            ));
        }
        let kind = match selection.as_str() {
            "terminal" => PaneKind::Terminal,
            "browser" => PaneKind::Browser(BrowserDescriptor::single(
                "about:blank".to_owned(),
                DEFAULT_BROWSER_PROFILE.to_owned(),
            )),
            "agent" => {
                if !self.experimental_agent_pane {
                    return Err(ServerError::InvalidCommand(
                        "agent panes are experimental; enable experimental-agent-pane in \
                         Settings → Advanced first"
                            .to_owned(),
                    ));
                }
                PaneKind::Agent(AgentDescriptor {
                    cwd: agent_cwd,
                    ..AgentDescriptor::default()
                })
            }
            "editor" => {
                if !self.experimental_editor_pane {
                    return Err(ServerError::InvalidCommand(
                        "editor panes are experimental; enable experimental-editor-pane in \
                         Settings → Advanced first"
                            .to_owned(),
                    ));
                }
                let cwd = std::env::current_dir()
                    .map_err(|error| {
                        ServerError::Internal(format!(
                            "could not resolve editor working directory: {error}"
                        ))
                    })?
                    .to_string_lossy()
                    .into_owned();
                PaneKind::Editor(EditorDescriptor { path: None, cwd })
            }
            _ => {
                return Err(ServerError::InvalidCommand(format!(
                    "unknown pane kind {selection:?}; expected terminal, browser, agent, or editor"
                )));
            }
        };
        let snapshot_kind = pane_kind_snapshot(&kind);
        let inherit_cwd_from = self.state.materialize_pane(pane, kind)?;
        Ok(Execution::effect(MuxEffect::PaneMaterialized {
            pane,
            kind: snapshot_kind,
            inherit_cwd_from,
            cwd: None,
            command: None,
        }))
    }

    fn break_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("break-pane", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "break-pane does not accept positional arguments".to_owned(),
            ));
        }
        let source = self.resolve_pane(options.value("-s"), context.window, context.pane)?;
        let source_window = self
            .state
            .window_for_pane(source)
            .expect("resolved source pane has a window");
        let source_session = self.state.windows[&source_window].session;
        let destination = match options.value("-t") {
            Some(target) => window_destination(&self.state, Some(target), context)?,
            None => WindowDestination {
                session: source_session,
                index: None,
            },
        };
        let destination_session = destination.session;
        let detached = options.has("-d");
        let original_context = context.clone();
        let base_index = self.base_index_for_session(destination_session);
        let window = self.state.break_pane_with_base_index(
            source,
            destination_session,
            destination.index,
            options.value("-n").map(str::to_owned),
            detached,
            base_index,
        )?;
        if detached {
            if original_context.window == Some(source_window)
                && original_context.pane == Some(source)
            {
                let context_window = if self
                    .state
                    .windows
                    .get(&source_window)
                    .is_some_and(|window| window.session == source_session)
                {
                    source_window
                } else if self.state.sessions.contains_key(&source_session) {
                    session_active_window(&self.state, source_session)?
                } else {
                    window
                };
                let pane = self.state.windows[&context_window].active_pane;
                *context = ExecutionContext::for_pane(&self.state, pane)
                    .expect("break-pane retains a valid command context");
            }
        } else {
            *context = ExecutionContext::for_pane(&self.state, source)
                .expect("broken pane belongs to its new window");
        }
        let mut execution = Execution::default();
        if source_session != destination_session {
            execution.effects.push(MuxEffect::PaneRelocated {
                pane: source,
                from: source_session,
                to: destination_session,
            });
        }
        debug_assert_eq!(self.state.window_for_pane(source), Some(window));
        Ok(execution)
    }

    fn join_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        command: &str,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options(command, args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(format!(
                "{command} does not accept positional arguments"
            )));
        }
        let source = self.resolve_pane(options.value("-s"), context.window, context.pane)?;
        let target = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let source_window = self
            .state
            .window_for_pane(source)
            .expect("resolved source pane has a window");
        let target_window = self
            .state
            .window_for_pane(target)
            .expect("resolved target pane has a window");
        let source_session = self.state.windows[&source_window].session;
        let target_session = self.state.windows[&target_window].session;
        let original_context = context.clone();
        let detached = options.has("-d");
        let size = options
            .value("-p")
            .map(parse_pane_percentage)
            .transpose()?
            .map_or(LayoutSplitSize::Default, LayoutSplitSize::Percent);
        if source_window != target_window
            && self.state.windows[&source_window].panes.len() == 1
            && self.renumber_windows_for_session(source_session)
        {
            self.state.validate_renumber_capacity(
                source_session,
                self.base_index_for_session(source_session),
                1,
            )?;
        }
        self.state.join_pane(
            source,
            target,
            if options.has("-h") {
                Axis::Horizontal
            } else {
                Axis::Vertical
            },
            size,
            options.has("-b"),
            options.has("-f"),
            detached,
        )?;
        if !self.state.windows.contains_key(&source_window) {
            self.renumber_session_if_enabled(source_session)?;
        }

        if detached {
            if original_context.window == Some(source_window)
                && original_context.pane == Some(source)
            {
                let window = if self.state.windows.contains_key(&source_window) {
                    source_window
                } else {
                    session_active_window(&self.state, source_session)?
                };
                let pane = window_active_pane(&self.state, window)?;
                *context = ExecutionContext::for_pane(&self.state, pane)
                    .expect("detached join retains a valid source context");
            }
        } else {
            *context = ExecutionContext::for_pane(&self.state, source)
                .expect("joined pane belongs to the target window");
        }
        let mut execution = Execution::default();
        if source_session != target_session {
            execution.effects.push(MuxEffect::PaneRelocated {
                pane: source,
                from: source_session,
                to: target_session,
            });
        }
        Ok(execution)
    }

    fn set_browser_url(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("set-browser-url", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let url = positional
            .first()
            .ok_or_else(|| ServerError::InvalidCommand("set-browser-url needs a URL".to_owned()))?;
        self.state.update_browser_url(pane, url.clone())?;
        Ok(Execution::default())
    }

    fn set_browser_tabs(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("set-browser-tabs", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let active_tab = options
            .value("-a")
            .map_or(Ok(0), str::parse)
            .map_err(|_| ServerError::InvalidCommand("-a needs a numeric tab index".to_owned()))?;
        if positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "set-browser-tabs needs at least one URL".to_owned(),
            ));
        }
        self.state
            .update_browser_tabs(pane, positional, active_tab)?;
        Ok(Execution::default())
    }

    fn set_browser_profile(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("set-browser-profile", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if positional.len() != 1 {
            return Err(ServerError::InvalidCommand(
                "set-browser-profile needs exactly one profile name".to_owned(),
            ));
        }
        self.state.update_browser_profile(pane, &positional[0])?;
        Ok(Execution::default())
    }

    fn set_agent_session(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("set-agent-session", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let [session_id] = positional.as_slice() else {
            return Err(ServerError::InvalidCommand(
                "set-agent-session needs exactly one ACP session ID".to_owned(),
            ));
        };
        let cwd = options.value("-c").map(PathBuf::from);
        self.state
            .update_agent_session(pane, session_id.clone(), cwd)?;
        Ok(Execution::default())
    }

    fn set_agent_provider(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("set-agent-provider", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let [provider] = positional.as_slice() else {
            return Err(ServerError::InvalidCommand(
                "set-agent-provider needs exactly one provider".to_owned(),
            ));
        };
        let provider = AgentProvider::from_str(provider).map_err(ServerError::InvalidCommand)?;
        if self.state.update_agent_provider(pane, provider)? {
            Ok(Execution::effect(MuxEffect::AgentPaneRestart { pane }))
        } else {
            Ok(Execution::default())
        }
    }

    fn restart_agent_pane(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("restart-agent-pane", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "restart-agent-pane does not accept positional arguments".to_owned(),
            ));
        }
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if !matches!(
            self.state.pane(pane).map(|pane| &pane.kind),
            Some(PaneKind::Agent(_))
        ) {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not an agent"
            )));
        }
        Ok(Execution::effect(MuxEffect::AgentPaneRestart { pane }))
    }

    fn set_editor_path(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("set-editor-path", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let path = match positional.as_slice() {
            [] => None,
            [path] => Some(path.clone()),
            _ => {
                return Err(ServerError::InvalidCommand(
                    "set-editor-path accepts at most one absolute path".to_owned(),
                ));
            }
        };
        self.state.update_editor_path(pane, path)?;
        Ok(Execution::default())
    }

    fn split_window_with_options(
        &mut self,
        context: &mut ExecutionContext,
        options: &Options,
        kind: PaneKind,
        command: Option<String>,
        size: Option<SplitSize<'_>>,
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let target = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let axis = if options.has("-h") {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        let placement = self.split_placement(options, size)?;
        let snapshot_kind = pane_kind_snapshot(&kind);
        let (inherit_cwd_from, cwd) = spawn_cwd_source(self, options, Some(target), &kind, hooks);
        let pane = self.state.split_pane_with(target, axis, kind, placement)?;
        if !placement.detached {
            *context =
                ExecutionContext::for_pane(&self.state, pane).expect("new pane has a context");
        }
        Ok(Execution::effect(MuxEffect::PaneCreated {
            pane,
            kind: snapshot_kind,
            inherit_cwd_from,
            cwd,
            command,
        }))
    }

    fn split_placement(
        &self,
        options: &Options,
        size: Option<SplitSize<'_>>,
    ) -> Result<SplitPlacement, ServerError> {
        let full_size = options.has("-f");
        let size = match size {
            None => LayoutSplitSize::Default,
            Some(SplitSize::Percentage(value)) => {
                LayoutSplitSize::Percent(parse_pane_percentage(value)?)
            }
            Some(SplitSize::Cells(value)) => {
                if let Some(percentage) = value.strip_suffix('%') {
                    LayoutSplitSize::Percent(parse_pane_percentage(percentage)?)
                } else {
                    let cells = value.parse::<u16>().map_err(|_| {
                        ServerError::InvalidCommand(format!("invalid pane size: {value}"))
                    })?;
                    LayoutSplitSize::Cells(cells)
                }
            }
        };
        Ok(SplitPlacement {
            size,
            before: options.has("-b"),
            full_size,
            detached: options.has("-d"),
        })
    }

    fn select_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("select-pane", args)?;
        if positional.len() > 1 {
            return Err(ServerError::InvalidCommand(
                "select-pane accepts at most one target".to_owned(),
            ));
        }
        let target = options
            .value("-t")
            .or_else(|| positional.first().map(String::as_str));
        let start = match target {
            Some(":.+" | ".+" | ":+") => {
                let current = self.resolve_pane(None, context.window, context.pane)?;
                self.state.next_pane(current)?
            }
            Some(":.-" | ".-" | ":-") => {
                let current = self.resolve_pane(None, context.window, context.pane)?;
                self.state.previous_pane(current)?
            }
            _ => self.resolve_pane(target, context.window, context.pane)?,
        };
        let direction = if options.has("-L") {
            Some(PaneDirection::Left)
        } else if options.has("-R") {
            Some(PaneDirection::Right)
        } else if options.has("-U") {
            Some(PaneDirection::Up)
        } else if options.has("-D") {
            Some(PaneDirection::Down)
        } else {
            None
        };
        if options.has("-l") {
            let window = self
                .state
                .window_for_pane(start)
                .expect("resolved pane has a window");
            let pane = self.state.last_pane(window)?;
            self.select_pane_target(context, pane, options.has("-Z"))?;
            return Ok(Execution::default());
        }
        let pane = if let Some(direction) = direction {
            let Some(pane) = self.state.pane_in_direction(start, direction)? else {
                return Ok(Execution::default());
            };
            pane
        } else {
            start
        };
        if let Some(title) = options.value("-T") {
            self.state.update_pane_title(pane, title)?;
            return Ok(Execution::default());
        }
        self.select_pane_target(context, pane, options.has("-Z"))?;
        Ok(Execution::default())
    }

    fn select_pane_target(
        &mut self,
        context: &mut ExecutionContext,
        pane: PaneId,
        preserve_zoom: bool,
    ) -> Result<(), ServerError> {
        if self.state.select_pane_with_zoom(pane, preserve_zoom)? {
            *context = ExecutionContext::for_pane(&self.state, pane).expect("selected pane exists");
        }
        Ok(())
    }

    fn last_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("last-pane", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "last-pane supports only -t and -Z".to_owned(),
            ));
        }
        let window = self.resolve_window(options.value("-t"), context.session, context.window)?;
        let pane = self.state.last_pane(window)?;
        self.select_pane_target(context, pane, options.has("-Z"))?;
        Ok(Execution::default())
    }

    fn swap_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("swap-pane", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "swap-pane supports -d, -D, -U, -Z, -s, and -t".to_owned(),
            ));
        }
        let target = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let source = if options.has("-D") {
            self.state.next_pane(target)?
        } else if options.has("-U") {
            self.state.previous_pane(target)?
        } else {
            let source = options.value("-s").ok_or_else(|| {
                ServerError::InvalidCommand(
                    "swap-pane needs -s when neither -U nor -D is used".to_owned(),
                )
            })?;
            self.resolve_pane(Some(source), context.window, context.pane)?
        };

        let source_window = self
            .state
            .window_for_pane(source)
            .expect("resolved source pane has a window");
        let target_window = self
            .state
            .window_for_pane(target)
            .expect("resolved target pane has a window");
        let source_session = self.state.windows[&source_window].session;
        let target_session = self.state.windows[&target_window].session;
        self.state
            .swap_panes(source, target, options.has("-d"), options.has("-Z"))?;
        let active = self.state.windows[&target_window].active_pane;
        *context = ExecutionContext::for_pane(&self.state, active)
            .expect("the target window retains an active pane after a swap");

        let mut execution = Execution::default();
        if source_session != target_session && source != target {
            execution.effects.extend([
                MuxEffect::PaneRelocated {
                    pane: source,
                    from: source_session,
                    to: target_session,
                },
                MuxEffect::PaneRelocated {
                    pane: target,
                    from: target_session,
                    to: source_session,
                },
            ]);
        }
        Ok(execution)
    }

    fn list_panes(
        &self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-panes", args)?;
        reject_positionals("list-panes", &positional)?;
        let target_window =
            self.resolve_window(options.value("-t"), context.session, context.window)?;
        let target_session = self.state.windows[&target_window].session;
        let window_ids = if options.has("-a") {
            self.state
                .sessions_by_name()
                .into_iter()
                .flat_map(|session| session.windows.iter().copied())
                .collect::<Vec<_>>()
        } else if options.has("-s") {
            self.state.sessions[&target_session].windows.clone()
        } else {
            vec![target_window]
        };
        let mut output = Vec::new();
        for window_id in window_ids {
            let window = self
                .state
                .windows
                .get(&window_id)
                .ok_or_else(|| ServerError::MissingTarget(window_id.to_string()))?;
            let session = &self.state.sessions[&window.session];
            for pane_id in window.pane_order() {
                let Some(pane) = window.panes.get(pane_id) else {
                    continue;
                };
                if let Some(format) = options.value("-F") {
                    output.push(expand_format_with_hooks(
                        format,
                        self,
                        FormatContext {
                            session: Some(window.session),
                            window: Some(window_id),
                            pane: Some(pane.id),
                            active_session: context.session,
                            format_type: FormatType::Pane,
                        },
                        hooks,
                    ));
                    continue;
                }
                let kind = match pane.kind {
                    PaneKind::Picker { .. } => "picker",
                    PaneKind::Terminal => "terminal",
                    PaneKind::Browser(_) => "browser",
                    PaneKind::Agent(_) => "agent",
                    PaneKind::Editor(_) => "editor",
                };
                let active = if pane.id == window.active_pane {
                    '*'
                } else {
                    '-'
                };
                let prefix = if options.has("-a") {
                    format!("{}:{}.", session.name, window.index)
                } else if options.has("-s") {
                    format!("{}.", window.index)
                } else {
                    String::new()
                };
                let pane_label = if options.has("-a") || options.has("-s") {
                    self.pane_index(window_id, pane.id)
                        .expect("listed pane has an index")
                        .to_string()
                } else {
                    pane.id.to_string()
                };
                output.push(format!(
                    "{prefix}{pane_label}: {kind}{active} {}",
                    pane.title
                ));
            }
        }
        let output = output.join("\n");
        Ok(Execution::output(output))
    }

    fn resize_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("resize-pane", args)?;
        if positional.len() > 1 {
            return Err(ServerError::InvalidCommand(
                "resize-pane accepts at most one adjustment".to_owned(),
            ));
        }
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if options.has("-Z") {
            self.state.toggle_zoom(pane)?;
            let window = self
                .state
                .window_for_pane(pane)
                .expect("resolved pane remains in its window");
            let session = self.state.windows[&window].session;
            if self.state.sessions[&session].active_window == window {
                let active = self.state.windows[&window].active_pane;
                *context = ExecutionContext::for_pane(&self.state, active)
                    .expect("zoomed window has an active pane context");
            }
            return Ok(Execution::default());
        }
        let window = self
            .state
            .window_for_pane(pane)
            .expect("resolved pane has a window");
        if self.state.windows[&window].zoomed_pane.is_some() {
            self.state.windows.get_mut(&window).unwrap().zoomed_pane = None;
            self.state.bump_generation();
        }
        let mut absolute = Vec::new();
        for (option, axis, dimension) in [
            ("-x", Axis::Horizontal, "width"),
            ("-y", Axis::Vertical, "height"),
        ] {
            let Some(value) = options.value(option) else {
                continue;
            };
            let window = self
                .state
                .window_for_pane(pane)
                .expect("resolved pane has a window");
            let extent = self
                .window_extent(window, axis)
                .expect("resolved window has a cell extent");
            let cells = parse_resize_size(value, extent, dimension)?;
            absolute.push((axis, cells));
        }
        let mut relative = Vec::new();
        for (option, axis, sign) in [
            ("-L", Axis::Horizontal, -1),
            ("-R", Axis::Horizontal, 1),
            ("-U", Axis::Vertical, -1),
            ("-D", Axis::Vertical, 1),
        ] {
            let attached = options
                .value(option)
                .map(parse_resize_adjustment)
                .transpose()?;
            if attached.is_none() && !options.has(option) {
                continue;
            }
            let cells = match attached {
                Some(cells) => cells,
                None => positional
                    .first()
                    .map(|value| parse_resize_adjustment(value))
                    .transpose()?
                    .unwrap_or(1),
            };
            relative.push((axis, cells.saturating_mul(sign)));
        }
        for (axis, cells) in absolute {
            self.state.resize_pane_to(pane, axis, cells)?;
        }
        for (axis, cells) in relative {
            self.state.resize_pane(pane, axis, cells)?;
        }
        Ok(Execution::default())
    }

    pub fn set_pane_geometry(&mut self, pane: PaneId, columns: u16, rows: u16) -> bool {
        let Some(window_id) = self.state.window_for_pane(pane) else {
            return false;
        };
        let changed = 'probe: {
            let window = self
                .state
                .windows
                .get_mut(&window_id)
                .expect("pane window exists");
            let probe = (pane, columns, rows);
            if let Some(zoomed_pane) = window.zoomed_pane {
                if zoomed_pane != pane || window.last_extent_probe == Some(probe) {
                    break 'probe false;
                }
                let measured = (columns, rows);
                if measured == window.layout.extent() {
                    break 'probe false;
                }
                window.layout.resize(measured.0, measured.1);
                window.last_extent_probe = Some(probe);
                break 'probe true;
            }
            if window.active_pane != pane {
                break 'probe false;
            }
            let Some(pane_geometry) = window.layout.pane_geometry(pane) else {
                debug_assert!(false, "window pane is missing from its layout");
                break 'probe false;
            };
            if pane_geometry.sx.abs_diff(columns) <= 1 && pane_geometry.sy.abs_diff(rows) <= 1 {
                break 'probe false;
            }
            if window.last_extent_probe == Some(probe) {
                break 'probe false;
            }
            let current = window.layout.extent();
            let implied = (
                implied_window_extent(columns, current.0, pane_geometry.sx),
                implied_window_extent(rows, current.1, pane_geometry.sy),
            );
            if implied == current {
                break 'probe false;
            }
            window.layout.resize(implied.0, implied.1);
            window.last_extent_probe = Some(probe);
            true
        };
        if changed {
            self.state.bump_generation();
        }
        changed
    }

    #[must_use]
    pub fn pane_geometry(&self, pane: PaneId) -> Option<(u16, u16)> {
        let window = self.state.window_for_pane(pane)?;
        self.state
            .windows
            .get(&window)?
            .displayed_pane_geometry(pane)
    }

    #[must_use]
    pub fn window_extent(&self, window: WindowId, axis: Axis) -> Option<u16> {
        let extent = self.state.windows.get(&window)?.layout.extent();
        Some(match axis {
            Axis::Horizontal => extent.0,
            Axis::Vertical => extent.1,
        })
    }

    fn select_layout(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
        command: &str,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options(command, args)?;
        if positional.len() > usize::from(command == "select-layout") {
            return Err(ServerError::InvalidCommand(format!(
                "{command} accepts {} layout names",
                usize::from(command == "select-layout")
            )));
        }

        let target = options.value("-t");
        let (window, pane) =
            if target.is_some_and(|target| target.starts_with('%') || target.contains('.')) {
                let pane = self.resolve_pane(target, context.window, context.pane)?;
                let window = self
                    .state
                    .window_for_pane(pane)
                    .expect("resolved pane has a window");
                (window, pane)
            } else {
                let window = self.resolve_window(target, context.session, context.window)?;
                (window, window_active_pane(&self.state, window)?)
            };

        if self
            .state
            .windows
            .get_mut(&window)
            .expect("window was resolved")
            .zoomed_pane
            .take()
            .is_some()
        {
            self.state.bump_generation();
        }

        if command == "next-layout" || options.has("-n") {
            self.state.cycle_layout(window, 1)?;
        } else if command == "previous-layout" || options.has("-p") {
            self.state.cycle_layout(window, -1)?;
        } else if options.has("-E") {
            self.state.spread_layout(pane)?;
        } else if options.has("-o") {
            if !positional.is_empty() {
                return Err(ServerError::InvalidCommand(
                    "select-layout -o does not accept a layout name".to_owned(),
                ));
            }
            self.state.restore_previous_layout(window)?;
        } else if let Some(name) = positional.first() {
            if let Some(preset) = parse_layout_preset(name)? {
                self.state.select_layout(window, preset)?;
            } else {
                self.state.select_layout_string(window, name)?;
            }
        } else if let Some(last) = self.state.last_layout(window)? {
            self.state.select_layout(window, last)?;
        }
        Ok(Execution::default())
    }

    fn rotate_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("rotate-window", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "rotate-window does not accept positional arguments".to_owned(),
            ));
        }
        let window = self.resolve_window(options.value("-t"), context.session, context.window)?;
        let pane = self
            .state
            .rotate_window(window, options.has("-D"), options.has("-Z"))?;
        *context = ExecutionContext::for_pane(&self.state, pane)
            .expect("rotated window retains an active pane");
        Ok(Execution::default())
    }

    fn kill_pane(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("kill-pane", args)?;
        reject_positionals("kill-pane", &positional)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let window = self
            .state
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let session = self.state.windows[&window].session;
        let targets = if options.has("-a") {
            self.state.windows[&window]
                .pane_order()
                .iter()
                .copied()
                .filter(|candidate| *candidate != pane)
                .collect()
        } else {
            vec![pane]
        };
        if targets.len() == self.state.windows[&window].panes.len()
            && self.renumber_windows_for_session(session)
        {
            self.state.validate_renumber_capacity(
                session,
                self.base_index_for_session(session),
                1,
            )?;
        }
        let mut panes = Vec::new();
        for target in targets {
            panes.extend(self.state.kill_pane(target)?);
        }
        if !self.state.windows.contains_key(&window) {
            self.renumber_session_if_enabled(session)?;
        }
        Ok(Execution::effect(MuxEffect::PanesRemoved(panes)))
    }

    fn respawn_pane(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("respawn-pane", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        let pane_state = self
            .state
            .pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if !matches!(&pane_state.kind, PaneKind::Terminal) {
            return Err(ServerError::InvalidCommand(format!(
                "respawn pane failed: pane {pane} is not a terminal"
            )));
        }
        if !pane_state.dead && !options.has("-k") {
            return Err(ServerError::InvalidCommand(format!(
                "respawn pane failed: pane {} still active",
                self.pane_target_description(pane)?
            )));
        }
        let (_, cwd) = spawn_cwd_source(self, &options, Some(pane), &PaneKind::Terminal, hooks);
        let environment = respawn_environment(&options);
        let command = shell_command_positional(&positional);
        self.state.revive_pane(pane)?;
        Ok(Execution::effect(MuxEffect::PaneRespawned {
            pane,
            cwd,
            command,
            environment,
        }))
    }

    fn respawn_window(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("respawn-window", args)?;
        let window = self.resolve_window(options.value("-t"), context.session, context.window)?;
        let window_state = self
            .state
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        if !options.has("-k") && window_state.panes.values().any(|pane| !pane.dead) {
            return Err(ServerError::InvalidCommand(format!(
                "respawn window failed: window {} still active",
                self.window_target_description(window)?
            )));
        }
        let pane = *window_state
            .pane_order()
            .first()
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        if !matches!(&window_state.panes[&pane].kind, PaneKind::Terminal) {
            return Err(ServerError::InvalidCommand(format!(
                "respawn window failed: pane {pane} is not a terminal"
            )));
        }
        let other_panes = window_state
            .pane_order()
            .iter()
            .copied()
            .filter(|candidate| *candidate != pane)
            .collect::<Vec<_>>();
        let (_, cwd) = spawn_cwd_source(self, &options, Some(pane), &PaneKind::Terminal, hooks);
        let environment = respawn_environment(&options);
        let command = shell_command_positional(&positional);
        let mut removed = Vec::new();
        for other in other_panes {
            removed.extend(self.state.kill_pane(other)?);
        }
        self.state.revive_pane(pane)?;
        let mut effects = Vec::with_capacity(2);
        if !removed.is_empty() {
            effects.push(MuxEffect::PanesRemoved(removed));
        }
        effects.push(MuxEffect::PaneRespawned {
            pane,
            cwd,
            command,
            environment,
        });
        Ok(Execution {
            output: String::new(),
            effects,
        })
    }

    fn pane_target_description(&self, pane: PaneId) -> Result<String, ServerError> {
        let window = self
            .state
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let window_state = &self.state.windows[&window];
        let session = &self.state.sessions[&window_state.session];
        let pane_index = self
            .pane_index(window, pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        Ok(format!(
            "{}:{}.{}",
            session.name, window_state.index, pane_index
        ))
    }

    fn window_target_description(&self, window: WindowId) -> Result<String, ServerError> {
        let window_state = self
            .state
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        let session = &self.state.sessions[&window_state.session];
        Ok(format!("{}:{}", session.name, window_state.index))
    }

    fn send_keys(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("send-keys", args)?;
        let repeat = repeat_count("send-keys", &options)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if options.value("-N").is_some() && positional.is_empty() && !options.has("-X") {
            return Ok(Execution::effect(MuxEffect::CopyModeRepeat {
                pane,
                count: repeat,
            }));
        }
        if options.has("-X") {
            let command = positional.first().ok_or_else(|| {
                ServerError::InvalidCommand("send-keys -X needs a copy-mode command".to_owned())
            })?;
            let action = copy_mode_action(
                command,
                &positional[1..],
                &options,
                self.set_clipboard,
                &self.copy_command,
            )?;
            let repeat = if repeats_in_copy_mode(&action) {
                repeat
            } else {
                1
            };
            return Ok(Execution {
                output: String::new(),
                effects: vec![
                    MuxEffect::TerminalView {
                        pane,
                        action: TerminalViewAction::CopyMode(action),
                    };
                    repeat
                ],
            });
        }
        let keys = if options.has("-H") {
            positional
                .iter()
                .map(|value| hex_key_token(value))
                .collect::<Result<Vec<_>, _>>()?
        } else if options.has("-l") {
            vec![KeyToken::Literal(positional.concat())]
        } else {
            positional.iter().map(|value| key_token(value)).collect()
        };
        Ok(Execution::effect(MuxEffect::SendKeys {
            pane,
            keys: std::iter::repeat_n(keys, repeat).flatten().collect(),
        }))
    }

    fn send_prefix(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, _) = parse_command_options("send-prefix", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        Ok(Execution::effect(MuxEffect::SendKeys {
            pane,
            keys: vec![KeyToken::Named(self.keys.prefix().to_owned())],
        }))
    }

    fn copy_mode(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("copy-mode", args)?;
        reject_positionals("copy-mode", &positional)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if options.has("-q") {
            return Ok(Execution::effect(MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(CopyModeAction::Cancel),
            }));
        }
        if options.has("-M") {
            return Ok(Execution::default());
        }
        let mut effects = vec![MuxEffect::TerminalView {
            pane,
            action: if options.has("-e") {
                TerminalViewAction::EnterCopyModeScrollExit
            } else {
                TerminalViewAction::EnterCopyMode
            },
        }];
        if options.has("-u") {
            effects.push(MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(CopyModeAction::PageUp),
            });
        }
        if options.has("-d") {
            effects.push(MuxEffect::TerminalView {
                pane,
                action: TerminalViewAction::CopyMode(if options.has("-e") {
                    CopyModeAction::PageDownScrollExit
                } else {
                    CopyModeAction::PageDown
                }),
            });
        }
        Ok(Execution {
            output: String::new(),
            effects,
        })
    }

    fn copy_mode_search_prompt(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, _) = parse_command_options("copy-mode-search-prompt", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        Ok(Execution::effect(MuxEffect::TerminalUi {
            pane,
            command: TerminalUiCommand::BeginSearch {
                direction: if options.has("-b") {
                    SearchDirection::Backward
                } else {
                    SearchDirection::Forward
                },
            },
        }))
    }

    fn command_prompt(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("command-prompt", args)?;
        if positional.len() > 1 {
            return Err(ServerError::InvalidCommand(
                "command-prompt accepts at most one template".to_owned(),
            ));
        }
        let prompt = options.value("-p").unwrap_or(":").to_owned();
        let input = self.expand_prompt_input(context, options.value("-I").unwrap_or_default())?;
        let template = positional.first().cloned();
        if prompt.len() > MAX_COMMAND_PROMPT_LABEL_BYTES {
            return Err(ServerError::InvalidCommand(format!(
                "command prompt label exceeds {MAX_COMMAND_PROMPT_LABEL_BYTES} bytes"
            )));
        }
        if input.len() > zz_protocol::MAX_COMMAND_PROMPT_BYTES {
            return Err(ServerError::InvalidCommand(format!(
                "command prompt input exceeds {} bytes",
                zz_protocol::MAX_COMMAND_PROMPT_BYTES
            )));
        }
        if template
            .as_ref()
            .is_some_and(|template| template.len() > MAX_COMMAND_PROMPT_TEMPLATE_BYTES)
        {
            return Err(ServerError::InvalidCommand(format!(
                "command prompt template exceeds {MAX_COMMAND_PROMPT_TEMPLATE_BYTES} bytes"
            )));
        }
        Ok(Execution::effect(MuxEffect::CommandPrompt {
            prompt,
            input,
            template,
        }))
    }

    fn expand_prompt_input(
        &self,
        context: &ExecutionContext,
        input: &str,
    ) -> Result<String, ServerError> {
        let session_name = if input.contains("#S") {
            let session = self.state.resolve_session(None, context.session)?;
            Some(
                self.state
                    .sessions
                    .get(&session)
                    .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?
                    .name
                    .as_str(),
            )
        } else {
            None
        };
        let window_name = if input.contains("#W") {
            let window = self.resolve_window(None, context.session, context.window)?;
            Some(
                self.state
                    .windows
                    .get(&window)
                    .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
                    .name
                    .as_str(),
            )
        } else {
            None
        };
        Ok(expand_short_formats(input, session_name, window_name))
    }

    fn clear_history(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, _) = parse_command_options("clear-history", args)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        Ok(Execution::effect(MuxEffect::TerminalView {
            pane,
            action: TerminalViewAction::ClearHistory,
        }))
    }

    fn choose_tree(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("choose-tree", args)?;
        if options.has("-s") && options.has("-w") {
            return Err(ServerError::InvalidCommand(
                "choose-tree accepts only one of -s or -w".to_owned(),
            ));
        }
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "choose-tree command templates are not supported yet".to_owned(),
            ));
        }
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if options.has("-s") || options.has("-w") {
            return Ok(Execution::effect(MuxEffect::FocusSidebar { pane }));
        }
        Ok(Execution::effect(MuxEffect::ChooseTree {
            pane,
            kind: ChooseTreeKind::Panes,
        }))
    }

    fn focus_sidebar(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("focus-sidebar", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "focus-sidebar does not take positional arguments".to_owned(),
            ));
        }
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        Ok(Execution::effect(MuxEffect::FocusSidebar { pane }))
    }

    fn choose_buffer(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("choose-buffer", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "choose-buffer command templates are not supported yet".to_owned(),
            ));
        }
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        Ok(Execution::effect(MuxEffect::ChooseBuffer { pane }))
    }

    fn display_message(
        &self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("display-message", args)?;
        let pane = match options.value("-t") {
            Some(target) => Some(self.resolve_pane(Some(target), context.window, context.pane)?),
            None if self.state.sessions.is_empty() => None,
            None => Some(self.resolve_pane(None, context.window, context.pane)?),
        };
        let format_context = pane
            .and_then(|pane| ExecutionContext::for_pane(&self.state, pane))
            .map_or_else(FormatContext::default, |target| FormatContext {
                session: target.session,
                window: target.window,
                pane: target.pane,
                active_session: context.session,
                format_type: FormatType::Pane,
            });
        let format = if positional.is_empty() {
            DEFAULT_DISPLAY_MESSAGE.to_owned()
        } else {
            positional.join(" ")
        };
        let text = expand_format_time_with_hooks(&format, self, format_context, hooks);
        if options.has("-p") {
            Ok(Execution::output(text))
        } else {
            let duration_ms = pane.map_or(self.global_display_time_ms, |pane| {
                self.display_time_for_pane(pane)
                    .expect("display-message pane was resolved")
            });
            Ok(Execution::effect(MuxEffect::DisplayMessage {
                pane,
                text,
                duration_ms,
            }))
        }
    }

    fn display_panes(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("display-panes", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "display-panes command templates are not supported yet".to_owned(),
            ));
        }
        let pane = self.resolve_pane(None, context.window, context.pane)?;
        let duration_ms = options.value("-d").map_or_else(
            || self.display_time_for_pane(pane),
            |value| {
                value.parse::<u32>().map_err(|_| {
                    ServerError::InvalidCommand(format!(
                        "display-panes duration must be an unsigned millisecond value: {value}"
                    ))
                })
            },
        )?;
        Ok(Execution::effect(MuxEffect::DisplayPanes {
            pane,
            duration_ms,
        }))
    }

    fn bind_key(&mut self, args: &[String]) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("bind-key", args)?;
        let table = key_table(&options);
        let repeat = options.has("-r");
        let note = options.value("-N").map(str::to_owned);
        let key = required_arg(&positional, 0, "key")?;
        required_arg(&positional, 1, "command")?;
        let commands = bound_commands(&positional[1..])?;
        self.keys.bind(
            table,
            key,
            Binding {
                commands,
                repeat,
                note,
            },
        );
        Ok(Execution::default())
    }

    fn unbind_key(&mut self, args: &[String]) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("unbind-key", args)?;
        let table = key_table(&options);
        let key = required_arg(&positional, 0, "key")?;
        self.keys.unbind(table, key);
        Ok(Execution::default())
    }

    fn list_keys(&self, args: &[String]) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-keys", args)?;
        if let Some(argument) = positional.first() {
            return Err(ServerError::UnsupportedCommand(format!(
                "list-keys {argument} (key filter)"
            )));
        }
        if let Some(table) = options.value("-T")
            && !matches!(table, "root" | "prefix" | "copy-mode" | "copy-mode-vi")
            && self.keys.table_names().all(|name| name != table)
        {
            return Err(ServerError::InvalidCommand(format!(
                "table {table} doesn't exist"
            )));
        }
        let output = self
            .keys
            .list(options.value("-T"))
            .map(|(table, key, binding)| {
                let commands = binding
                    .commands
                    .iter()
                    .map(format_command)
                    .collect::<Vec<_>>()
                    .join(" \\; ");
                format!("bind-key -T {table} {key} {commands}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Execution::output(output))
    }

    fn list_commands(
        &self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-commands", args)?;
        if positional.len() > 1 {
            return Err(ServerError::InvalidCommand(
                "list-commands accepts at most one command".to_owned(),
            ));
        }
        let listed_spec = |name: &str| {
            command_spec(name).or_else(|| {
                DAEMON_COMMAND_SPECS
                    .iter()
                    .find(|spec| spec.name == name || spec.aliases.contains(&name))
            })
        };
        let mut specs =
            if let Some(name) = positional.first() {
                vec![listed_spec(name).ok_or_else(|| {
                    ServerError::InvalidCommand(format!("unknown command: {name}"))
                })?]
            } else {
                COMMAND_SPECS
                    .iter()
                    .chain(DAEMON_COMMAND_SPECS.iter())
                    .collect::<Vec<_>>()
            };
        specs.sort_by_key(|spec| spec.name);
        let format = options.value("-F").unwrap_or(DEFAULT_LIST_COMMANDS_FORMAT);
        let mut output = Vec::with_capacity(specs.len());
        for spec in specs {
            let mut item_hooks = ListCommandHooks {
                inner: &mut *hooks,
                spec,
            };
            output.push(expand_format_with_hooks(
                format,
                self,
                FormatContext {
                    session: context.session,
                    window: context.window,
                    pane: context.pane,
                    active_session: context.session,
                    format_type: FormatType::None,
                },
                &mut item_hooks,
            ));
        }
        Ok(Execution::output(output.join("\n")))
    }

    fn set_option(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        let command = if force_window {
            "set-window-option"
        } else {
            "set-option"
        };
        let (options, positional) = parse_command_options(command, args)?;
        let Some(option) = positional.first() else {
            return Err(ServerError::InvalidCommand(
                "set-option needs an option".to_owned(),
            ));
        };
        if positional.len() > 2 {
            return Err(ServerError::InvalidCommand(
                "set-option accepts at most one value".to_owned(),
            ));
        }
        let value = positional.get(1).map(String::as_str);
        let parsed = match parse_tmux_option(option) {
            Ok(parsed) => parsed,
            Err(()) if options.has("-q") => return Ok(Execution::default()),
            Err(()) => {
                return Err(ServerError::InvalidCommand(format!(
                    "invalid option: {option}"
                )));
            }
        };
        if parsed.index.is_some() && (parsed.name.starts_with('@') || is_native_option(parsed.name))
        {
            return Err(ServerError::InvalidCommand(format!(
                "not an array: {option}"
            )));
        }
        if parsed.name.starts_with('@') {
            return self.set_user_option(context, parsed.name, value, &options, force_window);
        }
        if is_native_option(parsed.name) {
            return self.set_native_option(parsed.name, value, &options, force_window);
        }
        let table_option = match match_tmux_option(parsed.name) {
            Ok(Some(option)) => option,
            Ok(None) | Err(()) if options.has("-q") => return Ok(Execution::default()),
            Ok(None) => {
                return Err(ServerError::InvalidCommand(format!(
                    "invalid option: {option}"
                )));
            }
            Err(()) => {
                return Err(ServerError::InvalidCommand(format!(
                    "ambiguous option: {option}"
                )));
            }
        };
        if table_option.is_array {
            return Ok(Execution::default());
        }
        if table_option.default.is_none() && parsed.index.is_none() {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {}",
                table_option.name
            )));
        }
        let target = match self.resolve_tmux_option_target(
            context,
            &options,
            force_window,
            table_option.scope,
        ) {
            Ok(target) => target,
            Err(_) if options.has("-q") => return Ok(Execution::default()),
            Err(error) => return Err(error),
        };
        if parsed.index.is_some() {
            return Err(ServerError::InvalidCommand(format!(
                "not an array: {option}"
            )));
        }
        match table_option.name {
            "synchronize-panes" => self.set_synchronize_panes(value, &options, target),
            "mouse" => self.set_mouse(value, &options, target),
            "escape-time" => self.set_escape_time(value, &options),
            "automatic-rename" => self.set_automatic_rename(value, &options, target),
            "automatic-rename-format" => self.set_automatic_rename_format(value, &options, target),
            "remain-on-exit" => self.set_remain_on_exit(value, &options, target),
            "default-terminal" => self.set_default_terminal(value, &options),
            "display-time" | "repeat-time" => {
                self.set_behavior_time(table_option.name, value, &options, target)
            }
            "buffer-limit" => self.set_buffer_limit(value, &options),
            "message-limit" => self.set_message_limit(value, &options),
            "history-limit" => self.set_history_limit(value, &options, target),
            "base-index" => self.set_base_index(value, &options, target),
            "renumber-windows" => self.set_renumber_windows(value, &options, target),
            "pane-base-index" => self.set_pane_base_index(value, &options, target),
            "word-separators" => self.set_word_separators(value, &options, target),
            "mode-keys" => self.set_mode_keys(value, &options, target),
            "prefix" | "set-clipboard" | "copy-command" => {
                self.set_scalar_tmux_option(table_option.name, value, &options)
            }
            option => self.set_status_option(
                StatusOption::from_name(option).expect("implemented status option"),
                value,
                &options,
            ),
        }
    }

    fn set_user_option(
        &mut self,
        context: &ExecutionContext,
        option: &str,
        value: Option<&str>,
        options: &Options,
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        let target = match self.resolve_user_option_target(context, options, force_window) {
            Ok(target) => target,
            Err(_) if options.has("-q") => return Ok(Execution::default()),
            Err(error) => return Err(error),
        };
        let unset = option_is_unset(options);
        if options.has("-o") && !unset && self.user_option_at_target(target, option).is_some() {
            return already_set_or_quiet(options, option);
        }
        if unset {
            if options.has("-U")
                && let TmuxOptionTarget::Window(window) = target
            {
                let panes = self
                    .state
                    .windows
                    .get(&window)
                    .map(|window| window.pane_order().to_vec())
                    .unwrap_or_default();
                for pane in panes {
                    if let Some(values) = self.pane_user_options.get_mut(&pane) {
                        values.remove(option);
                    }
                }
            }
            self.user_options_at_target_mut(target).remove(option);
            return Ok(Execution::default());
        }
        let value = value.ok_or_else(|| ServerError::InvalidCommand("empty value".to_owned()))?;
        let values = self.user_options_at_target_mut(target);
        if options.has("-a")
            && let Some(current) = values.get_mut(option)
        {
            current.push_str(value);
        } else {
            values.insert(option.to_owned(), value.to_owned());
        }
        Ok(Execution::default())
    }

    fn show_options(
        &self,
        context: &ExecutionContext,
        args: &[String],
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        let command = if force_window {
            "show-window-options"
        } else {
            "show-options"
        };
        let (options, positional) = parse_command_options(command, args)?;
        if positional.len() > 1 {
            return Err(ServerError::InvalidCommand(
                "show-options accepts at most one option".to_owned(),
            ));
        }
        let value_only = options.has("-v");
        let include_inherited = options.has("-A");
        let Some(argument) = positional.first() else {
            let target = match self.resolve_user_option_target(context, &options, force_window) {
                Ok(target) => target,
                Err(_) if options.has("-q") => return Ok(Execution::default()),
                Err(error) => return Err(error),
            };
            let mut lines = Vec::new();
            if let Some(values) = self.user_options_at_target(target) {
                for (name, value) in values {
                    push_shown_option(&mut lines, name, value, true, false, value_only);
                }
            }
            for option in tmux_options().filter(|option| {
                !option.is_array
                    && option.default.is_some()
                    && option_scope_matches_target(option.scope, target)
            }) {
                if let Some((value, inherited)) =
                    self.tmux_option_readback(option, target, include_inherited)?
                {
                    push_shown_option(
                        &mut lines,
                        option.name,
                        &value,
                        option
                            .default
                            .expect("implemented option has a default")
                            .is_string(),
                        inherited,
                        value_only,
                    );
                }
            }
            return Ok(Execution::output(lines.join("\n")));
        };

        let parsed = match parse_tmux_option(argument) {
            Ok(parsed) => parsed,
            Err(()) if options.has("-q") => return Ok(Execution::default()),
            Err(()) => {
                return Err(ServerError::InvalidCommand(format!(
                    "invalid option: {argument}"
                )));
            }
        };
        if parsed.name.starts_with('@') {
            let target = match self.resolve_user_option_target(context, &options, force_window) {
                Ok(target) => target,
                Err(_) if options.has("-q") => return Ok(Execution::default()),
                Err(error) => return Err(error),
            };
            if let Some((value, inherited)) =
                self.user_option_readback(target, parsed.name, include_inherited)
            {
                let mut lines = Vec::new();
                let name = indexed_option_name(parsed.name, parsed.index.as_deref());
                push_shown_option(&mut lines, &name, value, true, inherited, value_only);
                return Ok(Execution::output(lines.remove(0)));
            }
            if options.has("-q") {
                return Ok(Execution::default());
            }
            return Err(ServerError::InvalidCommand(format!(
                "invalid option: {argument}"
            )));
        }
        if is_native_option(parsed.name) {
            let (value, is_string) = self.native_option_readback(parsed.name);
            let mut lines = Vec::new();
            let name = indexed_option_name(parsed.name, parsed.index.as_deref());
            push_shown_option(&mut lines, &name, &value, is_string, false, value_only);
            return Ok(Execution::output(lines.remove(0)));
        }
        let option = match match_tmux_option(parsed.name) {
            Ok(Some(option)) => option,
            Ok(None) | Err(()) if options.has("-q") => return Ok(Execution::default()),
            Ok(None) => {
                return Err(ServerError::InvalidCommand(format!(
                    "invalid option: {argument}"
                )));
            }
            Err(()) => {
                return Err(ServerError::InvalidCommand(format!(
                    "ambiguous option: {argument}"
                )));
            }
        };
        if option.is_array {
            return Ok(Execution::default());
        }
        if option.default.is_none() {
            return Ok(Execution::default());
        }
        let target =
            match self.resolve_tmux_option_target(context, &options, force_window, option.scope) {
                Ok(target) => target,
                Err(_) if options.has("-q") => return Ok(Execution::default()),
                Err(error) => return Err(error),
            };
        let Some((value, inherited)) =
            self.tmux_option_readback(option, target, include_inherited)?
        else {
            return Ok(Execution::default());
        };
        let mut lines = Vec::new();
        let name = indexed_option_name(option.name, parsed.index.as_deref());
        push_shown_option(
            &mut lines,
            &name,
            &value,
            option
                .default
                .expect("implemented option has a default")
                .is_string(),
            inherited,
            value_only,
        );
        Ok(Execution::output(lines.remove(0)))
    }

    fn set_environment(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("set-environment", args)?;
        if !(1..=2).contains(&positional.len()) {
            return Err(ServerError::InvalidCommand(
                "set-environment needs a variable and optional value".to_owned(),
            ));
        }
        let name = &positional[0];
        validate_environment_name(name)?;
        let target_session = if options.has("-g") {
            self.state
                .resolve_session(options.value("-t"), context.session)
                .ok()
        } else {
            Some(
                self.state
                    .resolve_session(options.value("-t"), context.session)?,
            )
        };
        let mut value = positional.get(1).cloned();
        if options.has("-F")
            && let Some(raw) = value.as_deref()
        {
            value = Some(expand_format_with_hooks(
                raw,
                self,
                self.environment_format_context(target_session, context.session),
                hooks,
            ));
        }
        if (options.has("-r") || options.has("-u")) && value.is_some() {
            let flag = if options.has("-u") { "-u" } else { "-r" };
            return Err(ServerError::InvalidCommand(format!(
                "can't specify a value with {flag}"
            )));
        }
        let environment = if options.has("-g") {
            &mut self.global_environment
        } else {
            self.session_environments
                .entry(target_session.expect("local environment has a session"))
                .or_default()
        };
        if options.has("-u") {
            environment.remove(name);
        } else if options.has("-r") {
            environment.insert(
                name.clone(),
                EnvironmentEntry {
                    value: None,
                    hidden: false,
                },
            );
        } else {
            let value = value
                .ok_or_else(|| ServerError::InvalidCommand("no value specified".to_owned()))?;
            environment.insert(
                name.clone(),
                EnvironmentEntry {
                    value: Some(value),
                    hidden: options.has("-h"),
                },
            );
        }
        Ok(Execution::default())
    }

    fn show_environment(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("show-environment", args)?;
        if positional.len() > 1 {
            return Err(ServerError::InvalidCommand(
                "show-environment accepts at most one variable".to_owned(),
            ));
        }
        let environment = if options.has("-g") {
            if let Some(target) = options.value("-t") {
                self.state.resolve_session(Some(target), context.session)?;
            }
            &self.global_environment
        } else {
            let session = self
                .state
                .resolve_session(options.value("-t"), context.session)?;
            self.session_environments
                .get(&session)
                .unwrap_or(&EMPTY_ENVIRONMENT)
        };
        if let Some(name) = positional.first() {
            let entry = environment
                .get(name)
                .ok_or_else(|| ServerError::InvalidCommand(format!("unknown variable: {name}")))?;
            return Ok(Execution::output(
                environment_line(name, entry, &options).unwrap_or_default(),
            ));
        }
        let output = environment
            .iter()
            .filter_map(|(name, entry)| environment_line(name, entry, &options))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Execution::output(output))
    }

    fn resolve_user_option_target(
        &self,
        context: &ExecutionContext,
        options: &Options,
        force_window: bool,
    ) -> Result<TmuxOptionTarget, ServerError> {
        if options.has("-s") {
            return Ok(TmuxOptionTarget::Server);
        }
        if options.has("-p") {
            return self
                .resolve_pane(options.value("-t"), context.window, context.pane)
                .map(TmuxOptionTarget::Pane);
        }
        if force_window || options.has("-w") {
            if options.has("-g") {
                return Ok(TmuxOptionTarget::GlobalWindow);
            }
            return self
                .resolve_option_window(context, options, force_window)
                .map(TmuxOptionTarget::Window);
        }
        if options.has("-g") {
            return Ok(TmuxOptionTarget::GlobalSession);
        }
        let window = self.resolve_option_window(context, options, false)?;
        Ok(TmuxOptionTarget::Session(
            self.state.windows[&window].session,
        ))
    }

    fn user_option_at_target(&self, target: TmuxOptionTarget, name: &str) -> Option<&str> {
        self.user_options_at_target(target)?
            .get(name)
            .map(String::as_str)
    }

    fn user_options_at_target(&self, target: TmuxOptionTarget) -> Option<&UserOptions> {
        match target {
            TmuxOptionTarget::Server => Some(&self.server_user_options),
            TmuxOptionTarget::GlobalSession => Some(&self.global_session_user_options),
            TmuxOptionTarget::Session(session) => self.session_user_options.get(&session),
            TmuxOptionTarget::GlobalWindow => Some(&self.global_window_user_options),
            TmuxOptionTarget::Window(window) => self.window_user_options.get(&window),
            TmuxOptionTarget::Pane(pane) => self.pane_user_options.get(&pane),
        }
    }

    fn user_options_at_target_mut(&mut self, target: TmuxOptionTarget) -> &mut UserOptions {
        match target {
            TmuxOptionTarget::Server => &mut self.server_user_options,
            TmuxOptionTarget::GlobalSession => &mut self.global_session_user_options,
            TmuxOptionTarget::Session(session) => {
                self.session_user_options.entry(session).or_default()
            }
            TmuxOptionTarget::GlobalWindow => &mut self.global_window_user_options,
            TmuxOptionTarget::Window(window) => self.window_user_options.entry(window).or_default(),
            TmuxOptionTarget::Pane(pane) => self.pane_user_options.entry(pane).or_default(),
        }
    }

    fn user_option_readback<'a>(
        &'a self,
        target: TmuxOptionTarget,
        name: &str,
        include_inherited: bool,
    ) -> Option<(&'a str, bool)> {
        if let Some(value) = self.user_option_at_target(target, name) {
            return Some((value, false));
        }
        if !include_inherited {
            return None;
        }
        let parent = match target {
            TmuxOptionTarget::Session(_) => self.global_session_user_options.get(name),
            TmuxOptionTarget::Window(_) => self.global_window_user_options.get(name),
            TmuxOptionTarget::Pane(pane) => self
                .state
                .window_for_pane(pane)
                .and_then(|window| self.window_user_options.get(&window))
                .and_then(|values| values.get(name))
                .or_else(|| self.global_window_user_options.get(name)),
            _ => None,
        }?;
        Some((parent, true))
    }

    fn tmux_option_readback(
        &self,
        option: TmuxOption,
        target: TmuxOptionTarget,
        include_inherited: bool,
    ) -> Result<Option<(String, bool)>, ServerError> {
        let inherited = || {
            include_inherited
                .then(|| {
                    self.global_tmux_option_value(option.name)
                        .or_else(|| option.default.map(|default| default.value().to_owned()))
                })
                .flatten()
                .map(|value| (value, true))
        };
        let value = match target {
            TmuxOptionTarget::Server
            | TmuxOptionTarget::GlobalSession
            | TmuxOptionTarget::GlobalWindow => self
                .global_tmux_option_value(option.name)
                .or_else(|| option.default.map(|default| default.value().to_owned()))
                .map(|value| (value, false)),
            TmuxOptionTarget::Session(session) => match option.name {
                "mouse" => self
                    .session_mouse
                    .get(&session)
                    .map(|value| (tmux_flag(*value).to_owned(), false))
                    .or_else(inherited),
                "display-time" => self
                    .session_display_time_ms
                    .get(&session)
                    .map(|value| (value.to_string(), false))
                    .or_else(inherited),
                "repeat-time" => self
                    .session_repeat_time_ms
                    .get(&session)
                    .map(|value| (value.to_string(), false))
                    .or_else(inherited),
                "history-limit" => self
                    .session_history_limits
                    .get(&session)
                    .map(|value| (value.to_string(), false))
                    .or_else(inherited),
                "base-index" => self
                    .session_base_indices
                    .get(&session)
                    .map(|value| (value.to_string(), false))
                    .or_else(inherited),
                "renumber-windows" => self
                    .session_renumber_windows
                    .get(&session)
                    .map(|value| (tmux_flag(*value).to_owned(), false))
                    .or_else(inherited),
                "word-separators" => self
                    .session_word_separators
                    .get(&session)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                _ => inherited(),
            },
            TmuxOptionTarget::Window(window) => match option.name {
                "automatic-rename" => self
                    .state
                    .window_automatic_rename_override(window)?
                    .map(|value| (tmux_flag(value).to_owned(), false))
                    .or_else(inherited),
                "automatic-rename-format" => self
                    .window_automatic_rename_formats
                    .get(&window)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                "remain-on-exit" => self
                    .window_remain_on_exit
                    .get(&window)
                    .map(|value| (value.as_str().to_owned(), false))
                    .or_else(inherited),
                "mode-keys" => self
                    .window_mode_keys
                    .get(&window)
                    .map(|value| (value.as_str().to_owned(), false))
                    .or_else(inherited),
                "pane-base-index" => self
                    .window_pane_base_indices
                    .get(&window)
                    .map(|value| (value.to_string(), false))
                    .or_else(inherited),
                "synchronize-panes" => self
                    .state
                    .window_synchronize_override(window)?
                    .map(|value| (tmux_flag(value).to_owned(), false))
                    .or_else(inherited),
                _ => inherited(),
            },
            TmuxOptionTarget::Pane(pane) => match option.name {
                "remain-on-exit" => self
                    .pane_remain_on_exit
                    .get(&pane)
                    .map(|value| (value.as_str().to_owned(), false))
                    .or_else(|| {
                        include_inherited.then(|| {
                            let window = self
                                .state
                                .window_for_pane(pane)
                                .expect("pane target was resolved");
                            (
                                self.remain_on_exit_for_window(window).as_str().to_owned(),
                                true,
                            )
                        })
                    }),
                "synchronize-panes" => self
                    .state
                    .pane_synchronize_override(pane)?
                    .map(|value| (tmux_flag(value).to_owned(), false))
                    .or_else(|| {
                        include_inherited.then(|| {
                            (
                                tmux_flag(
                                    self.state
                                        .pane_synchronize_panes(pane)
                                        .expect("pane target was resolved"),
                                )
                                .to_owned(),
                                true,
                            )
                        })
                    }),
                _ => None,
            },
        };
        Ok(value)
    }

    fn global_tmux_option_value(&self, name: &str) -> Option<String> {
        Some(match name {
            "default-terminal" => self
                .default_terminal
                .clone()
                .unwrap_or_else(|| DEFAULT_TERMINAL.to_owned()),
            "escape-time" => self.escape_time_ms.to_string(),
            "base-index" => self.global_base_index.to_string(),
            "buffer-limit" => self.buffer_limit.to_string(),
            "copy-command" => self.copy_command.clone(),
            "history-limit" => self.global_history_limit.to_string(),
            "display-time" => self.global_display_time_ms.to_string(),
            "message-limit" => self.message_limit.to_string(),
            "mode-keys" => self.global_mode_keys.as_str().to_owned(),
            "mouse" => tmux_flag(self.global_mouse).to_owned(),
            "pane-base-index" => self.global_pane_base_index.to_string(),
            "prefix" => self.mux_option_value(MuxOptionKey::Prefix),
            "renumber-windows" => tmux_flag(self.global_renumber_windows).to_owned(),
            "repeat-time" => self.global_repeat_time_ms.to_string(),
            "set-clipboard" => self.set_clipboard.as_str().to_owned(),
            "status" => tmux_flag(self.status.enabled).to_owned(),
            "status-interval" => self.status.interval.as_secs().to_string(),
            "status-left" => self.status.left.clone(),
            "status-right" => self.status.right.clone(),
            "synchronize-panes" => tmux_flag(self.state.global_synchronize_panes()).to_owned(),
            "update-environment" => UPDATE_ENVIRONMENT_DEFAULT.to_owned(),
            "word-separators" => self.global_word_separators.clone(),
            "automatic-rename" => tmux_flag(self.state.global_automatic_rename()).to_owned(),
            "automatic-rename-format" => self.global_automatic_rename_format.clone(),
            "remain-on-exit" => self.global_remain_on_exit.as_str().to_owned(),
            _ => return None,
        })
    }

    fn seed_session_environment(&mut self, session: SessionId) {
        let environment = UPDATE_ENVIRONMENT_DEFAULT
            .split_ascii_whitespace()
            .map(|name| {
                let value = self
                    .global_environment
                    .get(name)
                    .and_then(|entry| entry.value.clone());
                (
                    name.to_owned(),
                    EnvironmentEntry {
                        value,
                        hidden: false,
                    },
                )
            })
            .collect();
        self.session_environments.insert(session, environment);
    }

    fn native_option_readback(&self, name: &str) -> (String, bool) {
        match name {
            "history-trickle" => (self.history_trickle.to_string(), false),
            "experimental-agent-pane" => {
                (tmux_flag(self.experimental_agent_pane).to_owned(), false)
            }
            "experimental-editor-pane" => {
                (tmux_flag(self.experimental_editor_pane).to_owned(), false)
            }
            "agent-command" => (self.agent.command.clone(), true),
            "agent-claude-code-command" => (self.agent.claude_code_command.clone(), true),
            "agent-auto-approve" => (tmux_flag(self.agent.auto_approve).to_owned(), false),
            _ => unreachable!("native option is catalogued"),
        }
    }

    fn environment_format_context(
        &self,
        session: Option<SessionId>,
        active_session: Option<SessionId>,
    ) -> FormatContext {
        let window = session.and_then(|session| {
            self.state
                .sessions
                .get(&session)
                .map(|session| session.active_window)
        });
        let pane = window.and_then(|window| {
            self.state
                .windows
                .get(&window)
                .map(|window| window.active_pane)
        });
        FormatContext {
            session,
            window,
            pane,
            active_session,
            format_type: FormatType::Pane,
        }
    }

    fn set_native_option(
        &mut self,
        option: &str,
        value: Option<&str>,
        options: &Options,
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        if option == "history-trickle" {
            return self.set_history_trickle(value, options, force_window);
        }
        if let Some(flag) = options
            .flags
            .iter()
            .find(|flag| !matches!(flag.as_str(), "-g" | "-o" | "-q"))
        {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {flag} {option}"
            )));
        }
        if options.has("-o") {
            return already_set_or_quiet(options, option);
        }
        let value = value.ok_or_else(|| {
            ServerError::InvalidCommand(format!("set-option {option} needs a value"))
        })?;
        let changed = match option {
            "experimental-agent-pane" => {
                self.experimental_agent_pane =
                    parse_flag_value(Some(value), self.experimental_agent_pane)?;
                MuxOptionKey::ExperimentalAgentPane
            }
            "experimental-editor-pane" => {
                self.experimental_editor_pane =
                    parse_flag_value(Some(value), self.experimental_editor_pane)?;
                MuxOptionKey::ExperimentalEditorPane
            }
            "agent-command" | "agent-claude-code-command" => {
                if value.len() > MAX_AGENT_COMMAND_BYTES {
                    return Err(ServerError::InvalidCommand(format!(
                        "{option} exceeds {MAX_AGENT_COMMAND_BYTES} bytes"
                    )));
                }
                if value.trim().is_empty() {
                    return Err(ServerError::InvalidCommand(format!(
                        "{option} needs an adapter command"
                    )));
                }
                if option == "agent-command" {
                    value.clone_into(&mut self.agent.command);
                    MuxOptionKey::AgentCommand
                } else {
                    value.clone_into(&mut self.agent.claude_code_command);
                    MuxOptionKey::AgentClaudeCodeCommand
                }
            }
            "agent-auto-approve" => {
                self.agent.auto_approve = parse_flag_value(Some(value), self.agent.auto_approve)?;
                MuxOptionKey::AgentAutoApprove
            }
            _ => unreachable!("native option is catalogued"),
        };
        Ok(Execution::effect(MuxEffect::MuxOptionChanged {
            option: changed,
        }))
    }

    fn set_scalar_tmux_option(
        &mut self,
        option: &str,
        value: Option<&str>,
        options: &Options,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            return already_set_or_quiet(options, option);
        }
        let changed = match option {
            "prefix" => {
                let value = if unset {
                    DEFAULT_PREFIX
                } else {
                    value.ok_or_else(|| {
                        ServerError::InvalidCommand("set-option prefix needs a value".to_owned())
                    })?
                };
                self.keys.set_prefix(value);
                MuxOptionKey::Prefix
            }
            "set-clipboard" => {
                self.set_clipboard = if unset {
                    SetClipboard::default()
                } else {
                    match value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option set-clipboard needs a value".to_owned(),
                        )
                    })? {
                        "on" => SetClipboard::On,
                        "external" => SetClipboard::External,
                        "off" => SetClipboard::Off,
                        value => {
                            return Err(ServerError::InvalidCommand(format!(
                                "invalid set-clipboard value: {value}"
                            )));
                        }
                    }
                };
                MuxOptionKey::SetClipboard
            }
            "copy-command" => {
                let next = if unset {
                    String::new()
                } else {
                    let value = value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option copy-command needs a value".to_owned(),
                        )
                    })?;
                    if options.has("-a") {
                        format!("{}{value}", self.copy_command)
                    } else {
                        value.to_owned()
                    }
                };
                if next.len() > MAX_COPY_COMMAND_BYTES {
                    return Err(ServerError::InvalidCommand(format!(
                        "copy-command exceeds {MAX_COPY_COMMAND_BYTES} bytes"
                    )));
                }
                self.copy_command = next;
                MuxOptionKey::CopyCommand
            }
            _ => unreachable!("scalar tmux option is catalogued"),
        };
        Ok(Execution::effect(MuxEffect::MuxOptionChanged {
            option: changed,
        }))
    }

    fn resolve_tmux_option_target(
        &self,
        context: &ExecutionContext,
        options: &Options,
        force_window: bool,
        scope: TmuxOptionScope,
    ) -> Result<TmuxOptionTarget, ServerError> {
        match scope {
            TmuxOptionScope::Server => Ok(TmuxOptionTarget::Server),
            TmuxOptionScope::Session if options.has("-g") => Ok(TmuxOptionTarget::GlobalSession),
            TmuxOptionScope::Session => {
                let window = self.resolve_option_window(context, options, force_window)?;
                Ok(TmuxOptionTarget::Session(
                    self.state.windows[&window].session,
                ))
            }
            TmuxOptionScope::WindowPane if options.has("-p") => self
                .resolve_pane(options.value("-t"), context.window, context.pane)
                .map(TmuxOptionTarget::Pane),
            TmuxOptionScope::Window | TmuxOptionScope::WindowPane if options.has("-g") => {
                Ok(TmuxOptionTarget::GlobalWindow)
            }
            TmuxOptionScope::Window | TmuxOptionScope::WindowPane => self
                .resolve_option_window(context, options, force_window)
                .map(TmuxOptionTarget::Window),
        }
    }

    fn resolve_option_window(
        &self,
        context: &ExecutionContext,
        options: &Options,
        force_window: bool,
    ) -> Result<WindowId, ServerError> {
        if force_window {
            self.resolve_window(options.value("-t"), context.session, context.window)
        } else {
            let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
            self.state
                .window_for_pane(pane)
                .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))
        }
    }

    fn set_buffer_limit(
        &mut self,
        value: Option<&str>,
        options: &Options,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            return already_set_or_quiet(options, "buffer-limit");
        }
        let limit = if unset {
            DEFAULT_BUFFER_LIMIT
        } else {
            let value = value.ok_or_else(|| {
                ServerError::InvalidCommand("set-option buffer-limit needs a value".to_owned())
            })?;
            let limit = value.parse::<usize>().map_err(|_| {
                ServerError::InvalidCommand(format!("invalid buffer-limit value: {value}"))
            })?;
            if !(1..=MAX_BUFFER_LIMIT).contains(&limit) {
                return Err(ServerError::InvalidCommand(format!(
                    "buffer-limit must be between 1 and {MAX_BUFFER_LIMIT}"
                )));
            }
            limit
        };
        self.buffer_limit = limit;
        Ok(Execution {
            output: String::new(),
            effects: vec![
                MuxEffect::BufferLimitChanged(limit),
                MuxEffect::MuxOptionChanged {
                    option: MuxOptionKey::BufferLimit,
                },
            ],
        })
    }

    fn set_message_limit(
        &mut self,
        value: Option<&str>,
        options: &Options,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            return already_set_or_quiet(options, "message-limit");
        }
        let limit = if unset {
            DEFAULT_MESSAGE_LIMIT
        } else {
            let value = value.ok_or_else(|| {
                ServerError::InvalidCommand("set-option message-limit needs a value".to_owned())
            })?;
            let limit = value.parse::<usize>().map_err(|_| {
                ServerError::InvalidCommand(format!("invalid message-limit value: {value}"))
            })?;
            if limit > MAX_MESSAGE_LIMIT {
                return Err(ServerError::InvalidCommand(format!(
                    "message-limit must be between 0 and {MAX_MESSAGE_LIMIT}"
                )));
            }
            limit
        };
        self.message_limit = limit;
        Ok(Execution::default())
    }

    fn set_history_trickle(
        &mut self,
        value: Option<&str>,
        options: &Options,
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        if force_window {
            return Err(ServerError::InvalidCommand(
                "history-trickle is a global server option".to_owned(),
            ));
        }
        if let Some(flag) = options
            .flags
            .iter()
            .find(|flag| !matches!(flag.as_str(), "-g" | "-o" | "-q" | "-u"))
        {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {flag} history-trickle"
            )));
        }
        if options.has("-o") {
            if options.has("-u") {
                return Err(ServerError::InvalidCommand(
                    "history-trickle cannot combine set-once and unset".to_owned(),
                ));
            }
            return already_set_or_quiet(options, "history-trickle");
        }
        let limit = if options.has("-u") {
            if value.is_some() {
                return Err(ServerError::InvalidCommand(
                    "unsetting history-trickle does not accept a value".to_owned(),
                ));
            }
            DEFAULT_HISTORY_TRICKLE
        } else {
            let value = value.ok_or_else(|| {
                ServerError::InvalidCommand("set-option history-trickle needs a value".to_owned())
            })?;
            let limit = value.parse::<usize>().map_err(|_| {
                ServerError::InvalidCommand(format!("invalid history-trickle value: {value}"))
            })?;
            if limit > MAX_HISTORY_TRICKLE {
                return Err(ServerError::InvalidCommand(format!(
                    "history-trickle must be between 0 and {MAX_HISTORY_TRICKLE}"
                )));
            }
            limit
        };
        self.history_trickle = limit;
        Ok(Execution::effect(MuxEffect::MuxOptionChanged {
            option: MuxOptionKey::HistoryTrickle,
        }))
    }

    fn set_history_limit(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            match target {
                TmuxOptionTarget::GlobalSession => {
                    return already_set_or_quiet(options, "history-limit");
                }
                TmuxOptionTarget::Session(session)
                    if self.session_history_limits.contains_key(&session) =>
                {
                    return already_set_or_quiet(options, "history-limit");
                }
                _ => {}
            }
        }
        let limit = if unset {
            DEFAULT_HISTORY_LIMIT
        } else {
            parse_history_limit(value.ok_or_else(|| {
                ServerError::InvalidCommand("set-option history-limit needs a value".to_owned())
            })?)?
        };
        match target {
            TmuxOptionTarget::GlobalSession => {
                self.global_history_limit = limit;
                Ok(Execution::effect(MuxEffect::MuxOptionChanged {
                    option: MuxOptionKey::HistoryLimit,
                }))
            }
            TmuxOptionTarget::Session(session) => {
                if unset {
                    self.session_history_limits.remove(&session);
                } else {
                    self.session_history_limits.insert(session, limit);
                }
                Ok(Execution::default())
            }
            _ => unreachable!("history-limit has session scope"),
        }
    }

    fn set_base_index(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            match target {
                TmuxOptionTarget::GlobalSession => {
                    return already_set_or_quiet(options, "base-index");
                }
                TmuxOptionTarget::Session(session)
                    if self.session_base_indices.contains_key(&session) =>
                {
                    return already_set_or_quiet(options, "base-index");
                }
                _ => {}
            }
        }
        let index = if unset {
            DEFAULT_BASE_INDEX
        } else {
            parse_index_option(
                value.ok_or_else(|| {
                    ServerError::InvalidCommand("set-option base-index needs a value".to_owned())
                })?,
                MAX_BASE_INDEX,
            )?
        };
        match target {
            TmuxOptionTarget::GlobalSession => self.global_base_index = index,
            TmuxOptionTarget::Session(session) => {
                if unset {
                    self.session_base_indices.remove(&session);
                } else {
                    self.session_base_indices.insert(session, index);
                }
            }
            _ => unreachable!("base-index has session scope"),
        }
        Ok(Execution::default())
    }

    fn set_renumber_windows(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            match target {
                TmuxOptionTarget::GlobalSession => {
                    return already_set_or_quiet(options, "renumber-windows");
                }
                TmuxOptionTarget::Session(session)
                    if self.session_renumber_windows.contains_key(&session) =>
                {
                    return already_set_or_quiet(options, "renumber-windows");
                }
                _ => {}
            }
        }
        match target {
            TmuxOptionTarget::GlobalSession => {
                self.global_renumber_windows = if unset {
                    DEFAULT_RENUMBER_WINDOWS
                } else {
                    parse_tmux_flag_value(value, self.global_renumber_windows)?
                };
            }
            TmuxOptionTarget::Session(session) => {
                if unset {
                    self.session_renumber_windows.remove(&session);
                } else {
                    let next =
                        parse_tmux_flag_value(value, self.renumber_windows_for_session(session))?;
                    self.session_renumber_windows.insert(session, next);
                }
            }
            _ => unreachable!("renumber-windows has session scope"),
        }
        Ok(Execution::default())
    }

    fn set_pane_base_index(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            match target {
                TmuxOptionTarget::GlobalWindow => {
                    return already_set_or_quiet(options, "pane-base-index");
                }
                TmuxOptionTarget::Window(window)
                    if self.window_pane_base_indices.contains_key(&window) =>
                {
                    return already_set_or_quiet(options, "pane-base-index");
                }
                _ => {}
            }
        }
        if target == TmuxOptionTarget::GlobalWindow {
            if options.has("-o") && !unset {
                return already_set_or_quiet(options, "pane-base-index");
            }
            let next = if unset {
                DEFAULT_PANE_BASE_INDEX
            } else {
                parse_index_option(
                    value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option pane-base-index needs a value".to_owned(),
                        )
                    })?,
                    MAX_PANE_BASE_INDEX,
                )?
            };
            if self.global_pane_base_index != next {
                self.global_pane_base_index = next;
                self.state.bump_generation();
            }
            return Ok(Execution::default());
        }

        let TmuxOptionTarget::Window(window) = target else {
            unreachable!("pane-base-index has window scope")
        };
        let previous = self.pane_base_index_for_window(window);
        if unset {
            self.window_pane_base_indices.remove(&window);
        } else {
            let next = parse_index_option(
                value.ok_or_else(|| {
                    ServerError::InvalidCommand(
                        "set-option pane-base-index needs a value".to_owned(),
                    )
                })?,
                MAX_PANE_BASE_INDEX,
            )?;
            self.window_pane_base_indices.insert(window, next);
        }
        if self.pane_base_index_for_window(window) != previous {
            self.state.bump_generation();
        }
        Ok(Execution::default())
    }

    /// Only the global scope exists: zz renders one status section per window.
    fn set_status_option(
        &mut self,
        option: StatusOption,
        value: Option<&str>,
        options: &Options,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        let appended = (!unset && options.has("-a"))
            .then(|| self.status.format(option))
            .flatten()
            .zip(value)
            .map(|(current, value)| format!("{current}{value}"));
        let value = match (&appended, unset) {
            (_, true) => None,
            (Some(appended), false) => Some(appended.as_str()),
            (None, false) => Some(value.ok_or_else(|| {
                ServerError::InvalidCommand(format!("set-option {} needs a value", option.as_str()))
            })?),
        };

        let changed = self
            .status
            .set(option, value)
            .map_err(|message| ServerError::InvalidCommand(message.to_owned()))?;
        Ok(if changed {
            Execution::effect(MuxEffect::StatusFormatsChanged)
        } else {
            Execution::default()
        })
    }

    fn set_word_separators(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            match target {
                TmuxOptionTarget::GlobalSession => {
                    return already_set_or_quiet(options, "word-separators");
                }
                TmuxOptionTarget::Session(session)
                    if self.session_word_separators.contains_key(&session) =>
                {
                    return already_set_or_quiet(options, "word-separators");
                }
                _ => {}
            }
        }

        if target == TmuxOptionTarget::GlobalSession {
            let next = if unset {
                DEFAULT_WORD_SEPARATORS.to_owned()
            } else {
                let value = value.ok_or_else(|| {
                    ServerError::InvalidCommand(
                        "set-option word-separators needs a value".to_owned(),
                    )
                })?;
                if options.has("-a") {
                    let mut appended = self.global_word_separators.clone();
                    appended.push_str(value);
                    appended
                } else {
                    value.to_owned()
                }
            };
            validate_word_separators(&next)?;
            self.global_word_separators = next;
            return Ok(Execution {
                output: String::new(),
                effects: vec![
                    MuxEffect::WordSeparatorsChanged { session: None },
                    MuxEffect::MuxOptionChanged {
                        option: MuxOptionKey::WordSeparators,
                    },
                ],
            });
        }

        let TmuxOptionTarget::Session(session) = target else {
            unreachable!("word-separators has session scope")
        };
        if unset {
            self.session_word_separators.remove(&session);
        } else {
            let value = value.ok_or_else(|| {
                ServerError::InvalidCommand("set-option word-separators needs a value".to_owned())
            })?;
            let next = if options.has("-a") {
                let mut appended = self.word_separators_for_session(session).to_owned();
                appended.push_str(value);
                appended
            } else {
                value.to_owned()
            };
            validate_word_separators(&next)?;
            self.session_word_separators.insert(session, next);
        }
        Ok(Execution::effect(MuxEffect::WordSeparatorsChanged {
            session: Some(session),
        }))
    }

    fn set_mode_keys(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            if target == TmuxOptionTarget::GlobalWindow {
                return already_set_or_quiet(options, "mode-keys");
            }
            if let TmuxOptionTarget::Window(window) = target
                && self.window_mode_keys.contains_key(&window)
            {
                return already_set_or_quiet(options, "mode-keys");
            }
        }

        if target == TmuxOptionTarget::GlobalWindow {
            let next = if unset {
                ModeKeys::default()
            } else {
                parse_mode_keys(value, self.global_mode_keys)?
            };
            self.global_mode_keys = next;
            return Ok(Execution {
                output: String::new(),
                effects: vec![
                    MuxEffect::ModeKeysChanged { window: None },
                    MuxEffect::MuxOptionChanged {
                        option: MuxOptionKey::ModeKeys,
                    },
                ],
            });
        }

        let TmuxOptionTarget::Window(window) = target else {
            unreachable!("mode-keys has window scope")
        };
        if unset {
            self.window_mode_keys.remove(&window);
        } else {
            let next = parse_mode_keys(value, self.mode_keys_for_window(window))?;
            self.window_mode_keys.insert(window, next);
        }
        Ok(Execution::effect(MuxEffect::ModeKeysChanged {
            window: Some(window),
        }))
    }

    fn set_synchronize_panes(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            let already = match target {
                TmuxOptionTarget::GlobalWindow => true,
                TmuxOptionTarget::Window(window) => {
                    self.state.window_synchronize_override(window)?.is_some()
                }
                TmuxOptionTarget::Pane(pane) => {
                    self.state.pane_synchronize_override(pane)?.is_some()
                }
                _ => unreachable!("synchronize-panes has window or pane scope"),
            };
            if already {
                return already_set_or_quiet(options, "synchronize-panes");
            }
        }
        match target {
            TmuxOptionTarget::GlobalWindow => {
                let next = if unset {
                    false
                } else {
                    parse_tmux_flag_value(value, self.state.global_synchronize_panes())?
                };
                self.state.set_global_synchronize_panes(next);
                Ok(Execution::effect(MuxEffect::MuxOptionChanged {
                    option: MuxOptionKey::SynchronizePanes,
                }))
            }
            TmuxOptionTarget::Pane(pane) => {
                let next = if unset {
                    None
                } else {
                    Some(parse_tmux_flag_value(
                        value,
                        self.state.pane_synchronize_panes(pane)?,
                    )?)
                };
                self.state.set_pane_synchronize_panes(pane, next)?;
                Ok(Execution::default())
            }
            TmuxOptionTarget::Window(window) => {
                if options.has("-U") {
                    self.state.clear_pane_synchronize_overrides(window)?;
                    self.state.set_window_synchronize_panes(window, None)?;
                } else {
                    let next = if unset {
                        None
                    } else {
                        Some(parse_tmux_flag_value(
                            value,
                            self.state.window_synchronize_panes(window)?,
                        )?)
                    };
                    self.state.set_window_synchronize_panes(window, next)?;
                }
                Ok(Execution::default())
            }
            _ => unreachable!("synchronize-panes has window or pane scope"),
        }
    }

    fn set_mouse(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            let already = match target {
                TmuxOptionTarget::GlobalSession => true,
                TmuxOptionTarget::Session(session) => self.session_mouse.contains_key(&session),
                _ => unreachable!("mouse has session scope"),
            };
            if already {
                return already_set_or_quiet(options, "mouse");
            }
        }
        match target {
            TmuxOptionTarget::GlobalSession => {
                self.global_mouse = if unset {
                    DEFAULT_MOUSE
                } else {
                    parse_tmux_flag_value(value, self.global_mouse)?
                };
            }
            TmuxOptionTarget::Session(session) => {
                if unset {
                    self.session_mouse.remove(&session);
                } else {
                    let next = parse_tmux_flag_value(value, self.mouse_for_session(session))?;
                    self.session_mouse.insert(session, next);
                }
            }
            _ => unreachable!("mouse has session scope"),
        }
        Ok(Execution::default())
    }

    fn set_escape_time(
        &mut self,
        value: Option<&str>,
        options: &Options,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            return already_set_or_quiet(options, "escape-time");
        }
        self.escape_time_ms = if unset {
            DEFAULT_ESCAPE_TIME_MS
        } else {
            parse_index_option(
                value.ok_or_else(|| {
                    ServerError::InvalidCommand("set-option escape-time needs a value".to_owned())
                })?,
                i32::MAX.cast_unsigned(),
            )?
        };
        Ok(Execution::default())
    }

    fn set_automatic_rename(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            let already = match target {
                TmuxOptionTarget::GlobalWindow => true,
                TmuxOptionTarget::Window(window) => self
                    .state
                    .window_automatic_rename_override(window)?
                    .is_some(),
                _ => unreachable!("automatic-rename has window scope"),
            };
            if already {
                return already_set_or_quiet(options, "automatic-rename");
            }
        }
        match target {
            TmuxOptionTarget::GlobalWindow => {
                let next = if unset {
                    None
                } else {
                    Some(parse_tmux_flag_value(
                        value,
                        self.state.global_automatic_rename(),
                    )?)
                };
                self.state.set_global_automatic_rename(next);
            }
            TmuxOptionTarget::Window(window) => {
                let next = if unset {
                    None
                } else {
                    Some(parse_tmux_flag_value(
                        value,
                        self.state.window_automatic_rename(window)?,
                    )?)
                };
                self.state.set_window_automatic_rename(window, next)?;
            }
            _ => unreachable!("automatic-rename has window scope"),
        }
        Ok(Execution::default())
    }

    fn set_automatic_rename_format(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            let already = match target {
                TmuxOptionTarget::GlobalWindow => true,
                TmuxOptionTarget::Window(window) => {
                    self.window_automatic_rename_formats.contains_key(&window)
                }
                _ => unreachable!("automatic-rename-format has window scope"),
            };
            if already {
                return already_set_or_quiet(options, "automatic-rename-format");
            }
        }
        match target {
            TmuxOptionTarget::GlobalWindow => {
                self.global_automatic_rename_format = if unset {
                    DEFAULT_AUTOMATIC_RENAME_FORMAT.to_owned()
                } else {
                    let value = value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option automatic-rename-format needs a value".to_owned(),
                        )
                    })?;
                    if options.has("-a") {
                        format!("{}{value}", self.global_automatic_rename_format)
                    } else {
                        value.to_owned()
                    }
                };
            }
            TmuxOptionTarget::Window(window) => {
                if unset {
                    self.window_automatic_rename_formats.remove(&window);
                } else {
                    let value = value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option automatic-rename-format needs a value".to_owned(),
                        )
                    })?;
                    let next = if options.has("-a") {
                        format!("{}{value}", self.automatic_rename_format_for_window(window))
                    } else {
                        value.to_owned()
                    };
                    self.window_automatic_rename_formats.insert(window, next);
                }
            }
            _ => unreachable!("automatic-rename-format has window scope"),
        }
        Ok(Execution::default())
    }

    fn set_remain_on_exit(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            let already = match target {
                TmuxOptionTarget::GlobalWindow => true,
                TmuxOptionTarget::Window(window) => {
                    self.window_remain_on_exit.contains_key(&window)
                }
                TmuxOptionTarget::Pane(pane) => self.pane_remain_on_exit.contains_key(&pane),
                _ => unreachable!("remain-on-exit has window or pane scope"),
            };
            if already {
                return already_set_or_quiet(options, "remain-on-exit");
            }
        }
        match target {
            TmuxOptionTarget::GlobalWindow => {
                self.global_remain_on_exit = if unset {
                    RemainOnExit::Off
                } else {
                    parse_remain_on_exit(value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option remain-on-exit needs a value".to_owned(),
                        )
                    })?)?
                };
            }
            TmuxOptionTarget::Window(window) => {
                if options.has("-U") {
                    let panes = self
                        .state
                        .windows
                        .get(&window)
                        .map(|window| window.pane_order().to_vec())
                        .unwrap_or_default();
                    for pane in panes {
                        self.pane_remain_on_exit.remove(&pane);
                    }
                    self.window_remain_on_exit.remove(&window);
                } else if unset {
                    self.window_remain_on_exit.remove(&window);
                } else {
                    let next = parse_remain_on_exit(value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option remain-on-exit needs a value".to_owned(),
                        )
                    })?)?;
                    self.window_remain_on_exit.insert(window, next);
                }
            }
            TmuxOptionTarget::Pane(pane) => {
                if unset {
                    self.pane_remain_on_exit.remove(&pane);
                } else {
                    let next = parse_remain_on_exit(value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option remain-on-exit needs a value".to_owned(),
                        )
                    })?)?;
                    self.pane_remain_on_exit.insert(pane, next);
                }
            }
            _ => unreachable!("remain-on-exit has window or pane scope"),
        }
        Ok(Execution::default())
    }

    fn set_default_terminal(
        &mut self,
        value: Option<&str>,
        options: &Options,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            return already_set_or_quiet(options, "default-terminal");
        }
        self.default_terminal = if unset {
            None
        } else {
            let value = value.ok_or_else(|| {
                ServerError::InvalidCommand("set-option default-terminal needs a value".to_owned())
            })?;
            Some(if options.has("-a") {
                format!(
                    "{}{value}",
                    self.default_terminal.as_deref().unwrap_or(DEFAULT_TERMINAL)
                )
            } else {
                value.to_owned()
            })
        };
        Ok(Execution::default())
    }

    fn set_behavior_time(
        &mut self,
        option: &str,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            let already = match (option, target) {
                ("display-time" | "repeat-time", TmuxOptionTarget::GlobalSession) => true,
                ("display-time", TmuxOptionTarget::Session(session)) => {
                    self.session_display_time_ms.contains_key(&session)
                }
                ("repeat-time", TmuxOptionTarget::Session(session)) => {
                    self.session_repeat_time_ms.contains_key(&session)
                }
                _ => unreachable!("behavior time has session scope"),
            };
            if already {
                return already_set_or_quiet(options, option);
            }
        }
        let maximum = if option == "repeat-time" {
            MAX_REPEAT_TIME_MS
        } else {
            i32::MAX.cast_unsigned()
        };
        let default = if option == "repeat-time" {
            DEFAULT_REPEAT_TIME_MS
        } else {
            DEFAULT_DISPLAY_TIME_MS
        };
        let parsed = if unset {
            default
        } else {
            parse_index_option(
                value.ok_or_else(|| {
                    ServerError::InvalidCommand(format!("set-option {option} needs a value"))
                })?,
                maximum,
            )?
        };
        match (option, target) {
            ("display-time", TmuxOptionTarget::GlobalSession) => {
                self.global_display_time_ms = parsed;
            }
            ("display-time", TmuxOptionTarget::Session(session)) => {
                if unset {
                    self.session_display_time_ms.remove(&session);
                } else {
                    self.session_display_time_ms.insert(session, parsed);
                }
            }
            ("repeat-time", TmuxOptionTarget::GlobalSession) => {
                self.global_repeat_time_ms = parsed;
            }
            ("repeat-time", TmuxOptionTarget::Session(session)) => {
                if unset {
                    self.session_repeat_time_ms.remove(&session);
                } else {
                    self.session_repeat_time_ms.insert(session, parsed);
                }
            }
            _ => unreachable!("behavior time has session scope"),
        }
        Ok(Execution::default())
    }

    fn source_file(args: &[String]) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("source-file", args)?;
        if positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "source-file needs a path".to_owned(),
            ));
        }
        let quiet = options.has("-q");
        Ok(Execution {
            output: String::new(),
            effects: positional
                .into_iter()
                .map(|path| MuxEffect::SourceFile { path, quiet })
                .collect(),
        })
    }

    /// Re-point a connection's context at live ids. Any command path that
    /// returns before [`MuxEngine::execute`] reaches this must call it itself.
    pub fn repair_context(&self, context: &mut ExecutionContext) {
        let valid = context
            .pane
            .and_then(|pane| ExecutionContext::for_pane(&self.state, pane));
        if let Some(valid) = valid {
            *context = valid;
        } else if let Some((session, window, pane)) = self.state.default_context() {
            *context = ExecutionContext {
                session: Some(session),
                window: Some(window),
                pane: Some(pane),
            };
        } else {
            *context = ExecutionContext::default();
        }
    }
}

fn is_native_option(option: &str) -> bool {
    NATIVE_OPTIONS.contains(&option)
}

fn option_scope_matches_target(scope: TmuxOptionScope, target: TmuxOptionTarget) -> bool {
    matches!(
        (scope, target),
        (TmuxOptionScope::Server, TmuxOptionTarget::Server)
            | (
                TmuxOptionScope::Session,
                TmuxOptionTarget::GlobalSession | TmuxOptionTarget::Session(_)
            )
            | (
                TmuxOptionScope::Window,
                TmuxOptionTarget::GlobalWindow | TmuxOptionTarget::Window(_)
            )
            | (
                TmuxOptionScope::WindowPane,
                TmuxOptionTarget::GlobalWindow
                    | TmuxOptionTarget::Window(_)
                    | TmuxOptionTarget::Pane(_)
            )
    )
}

fn push_shown_option(
    lines: &mut Vec<String>,
    name: &str,
    value: &str,
    is_string: bool,
    inherited: bool,
    value_only: bool,
) {
    if value_only {
        lines.push(value.to_owned());
        return;
    }
    let name = if inherited {
        format!("{name}*")
    } else {
        name.to_owned()
    };
    let value = if is_string {
        tmux_args_escape(value)
    } else {
        value.to_owned()
    };
    lines.push(format!("{name} {value}"));
}

fn indexed_option_name(name: &str, index: Option<&str>) -> String {
    index.map_or_else(|| name.to_owned(), |index| format!("{name}[{index}]"))
}

fn tmux_args_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    let double_quoted = value.bytes().any(|byte| b" #';${}%".contains(&byte));
    let single_quoted = !double_quoted && value.bytes().any(|byte| b" \"".contains(&byte));
    let bytes = value.as_bytes();
    if bytes.len() == 1 && bytes[0] != b' ' && (double_quoted || single_quoted || bytes[0] == b'~')
    {
        return format!("\\{}", char::from(bytes[0]));
    }

    let escaped = tmux_vis(value, double_quoted);
    if single_quoted {
        format!("'{escaped}'")
    } else if double_quoted {
        if escaped.starts_with('~') {
            format!("\"\\{escaped}\"")
        } else {
            format!("\"{escaped}\"")
        }
    } else if escaped.starts_with('~') {
        format!("\\{escaped}")
    } else {
        escaped
    }
}

fn tmux_vis(value: &str, double_quoted: bool) -> String {
    let mut escaped = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if !character.is_ascii() {
            escaped.push(character);
            continue;
        }
        let byte = character as u8;
        if byte == b'$'
            && double_quoted
            && characters
                .peek()
                .is_some_and(|next| next.is_ascii_alphabetic() || matches!(*next, '_' | '{'))
        {
            escaped.push('\\');
            escaped.push('$');
            continue;
        }
        if byte == b'\\' || byte == b'"' && double_quoted {
            escaped.push('\\');
            escaped.push(character);
            continue;
        }
        if byte == b' ' || byte.is_ascii_graphic() {
            escaped.push(character);
            continue;
        }
        match byte {
            b'\n' => escaped.push_str("\\n"),
            b'\r' => escaped.push_str("\\r"),
            0x08 => escaped.push_str("\\b"),
            0x07 => escaped.push_str("\\a"),
            0x0b => escaped.push_str("\\v"),
            b'\t' => escaped.push_str("\\t"),
            0x0c => escaped.push_str("\\f"),
            0 => {
                escaped.push_str("\\0");
                if characters
                    .peek()
                    .is_some_and(|next| matches!(*next, '0'..='7'))
                {
                    escaped.push_str("00");
                }
            }
            _ => write!(escaped, "\\{byte:03o}").expect("writing to a string cannot fail"),
        }
    }
    escaped
}

fn tmux_flag(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn validate_environment_name(name: &str) -> Result<(), ServerError> {
    if name.is_empty() {
        return Err(ServerError::InvalidCommand(
            "empty variable name".to_owned(),
        ));
    }
    if name.contains('=') {
        return Err(ServerError::InvalidCommand(
            "variable name contains =".to_owned(),
        ));
    }
    Ok(())
}

fn environment_line(name: &str, entry: &EnvironmentEntry, options: &Options) -> Option<String> {
    if options.has("-h") != entry.hidden {
        return None;
    }
    if !options.has("-s") {
        return Some(match &entry.value {
            Some(value) => format!("{name}={value}"),
            None => format!("-{name}"),
        });
    }
    Some(match &entry.value {
        Some(value) => format!(
            "{name}=\"{}\"; export {name};",
            shell_environment_escape(value)
        ),
        None => format!("unset {name};"),
    })
}

fn shell_environment_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '$' | '`' | '"' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn option_is_unset(options: &Options) -> bool {
    options.has("-u") || options.has("-U")
}

fn parse_tmux_flag_value(value: Option<&str>, current: bool) -> Result<bool, ServerError> {
    let Some(value) = value else {
        return Ok(!current);
    };
    if value.is_empty() {
        return Ok(!current);
    }
    if value == "1"
        || ["on", "yes"]
            .iter()
            .any(|spelling| value.eq_ignore_ascii_case(spelling))
    {
        return Ok(true);
    }
    if value == "0"
        || ["off", "no"]
            .iter()
            .any(|spelling| value.eq_ignore_ascii_case(spelling))
    {
        return Ok(false);
    }
    Err(ServerError::InvalidCommand(format!("bad value: {value}")))
}

fn parse_remain_on_exit(value: &str) -> Result<RemainOnExit, ServerError> {
    match value {
        "off" => Ok(RemainOnExit::Off),
        "on" => Ok(RemainOnExit::On),
        "failed" => Ok(RemainOnExit::Failed),
        "key" => Ok(RemainOnExit::Key),
        _ => Err(ServerError::InvalidCommand(format!(
            "unknown value: {value}"
        ))),
    }
}

fn parse_flag_value(value: Option<&str>, current: bool) -> Result<bool, ServerError> {
    let Some(value) = value else {
        return Ok(!current);
    };
    if value.is_empty() {
        return Ok(!current);
    }
    if value == "1"
        || ["on", "yes", "true"]
            .iter()
            .any(|spelling| value.eq_ignore_ascii_case(spelling))
    {
        return Ok(true);
    }
    if value == "0"
        || ["off", "no", "false"]
            .iter()
            .any(|spelling| value.eq_ignore_ascii_case(spelling))
    {
        return Ok(false);
    }
    Err(ServerError::InvalidCommand(format!(
        "invalid flag value: {value}"
    )))
}

fn parse_mode_keys(value: Option<&str>, current: ModeKeys) -> Result<ModeKeys, ServerError> {
    match value {
        None => Ok(current.toggled()),
        Some("vi") => Ok(ModeKeys::Vi),
        Some("emacs") => Ok(ModeKeys::Emacs),
        Some(value) => Err(ServerError::InvalidCommand(format!(
            "invalid mode-keys value: {value}"
        ))),
    }
}

fn parse_history_limit(value: &str) -> Result<usize, ServerError> {
    let limit = value.parse::<usize>().map_err(|_| {
        ServerError::InvalidCommand(format!("invalid history-limit value: {value}"))
    })?;
    if limit > MAX_HISTORY_LIMIT {
        return Err(ServerError::InvalidCommand(format!(
            "history-limit must be between 0 and {MAX_HISTORY_LIMIT}"
        )));
    }
    Ok(limit)
}

fn parse_index_option(value: &str, maximum: u32) -> Result<u32, ServerError> {
    match value.parse::<i128>() {
        Ok(index) if index < 0 => Err(ServerError::InvalidCommand(format!(
            "value is too small: {value}"
        ))),
        Ok(index) if index > i128::from(maximum) => Err(ServerError::InvalidCommand(format!(
            "value is too large: {value}"
        ))),
        Ok(index) => Ok(u32::try_from(index).expect("bounded index fits u32")),
        Err(_) if decimal_digits(value.strip_prefix('-')) => Err(ServerError::InvalidCommand(
            format!("value is too small: {value}"),
        )),
        Err(_) if decimal_digits(Some(value.strip_prefix('+').unwrap_or(value))) => Err(
            ServerError::InvalidCommand(format!("value is too large: {value}")),
        ),
        Err(_) => Err(ServerError::InvalidCommand(format!(
            "value is invalid: {value}"
        ))),
    }
}

fn decimal_digits(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
}

fn validate_word_separators(value: &str) -> Result<(), ServerError> {
    if value.len() > MAX_WORD_SEPARATORS_BYTES {
        return Err(ServerError::InvalidCommand(format!(
            "word-separators exceeds {MAX_WORD_SEPARATORS_BYTES} bytes"
        )));
    }
    Ok(())
}

/// How a split verb spells the size of the pane it creates: `-p` is always a
/// percentage, `-l` is cells unless it carries a `%` suffix.
#[derive(Clone, Copy)]
enum SplitSize<'a> {
    Percentage(&'a str),
    Cells(&'a str),
}

fn split_size(options: &Options) -> Option<SplitSize<'_>> {
    options.value("-l").map_or_else(
        || options.value("-p").map(SplitSize::Percentage),
        |size| Some(SplitSize::Cells(size)),
    )
}

fn initial_window_extent(options: &Options) -> Result<(u16, u16), ServerError> {
    Ok((
        initial_window_dimension(options, "-x", DEFAULT_WINDOW_EXTENT.0)?,
        initial_window_dimension(options, "-y", DEFAULT_WINDOW_EXTENT.1)?,
    ))
}

fn initial_window_dimension(
    options: &Options,
    option: &str,
    default: u16,
) -> Result<u16, ServerError> {
    options.value(option).map_or(Ok(default), |value| {
        value
            .parse::<u16>()
            .map_err(|_| ServerError::InvalidCommand(format!("invalid window size: {value}")))
    })
}

fn implied_window_extent(measured: u16, window: u16, pane: u16) -> u16 {
    if pane == 0 {
        return window;
    }
    let numerator = u64::from(measured) * u64::from(window);
    let rounded = (numerator + u64::from(pane) / 2) / u64::from(pane);
    u16::try_from(rounded).unwrap_or(u16::MAX)
}

fn parse_pane_percentage(value: &str) -> Result<u8, ServerError> {
    let percentage = value
        .strip_suffix('%')
        .unwrap_or(value)
        .parse::<u8>()
        .map_err(|_| ServerError::InvalidCommand(format!("invalid pane percentage: {value}")))?;
    if percentage > 100 {
        return Err(ServerError::InvalidCommand(
            "pane percentage must be between 0 and 100".to_owned(),
        ));
    }
    Ok(percentage)
}

fn parse_layout_preset(value: &str) -> Result<Option<LayoutPreset>, ServerError> {
    if let Some(exact) = LayoutPreset::ALL
        .into_iter()
        .find(|preset| preset.name() == value)
    {
        return Ok(Some(exact));
    }
    let mut matches = LayoutPreset::ALL
        .into_iter()
        .filter(|preset| preset.name().starts_with(value));
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(ServerError::InvalidCommand(format!(
            "ambiguous layout: {value}"
        )));
    }
    Ok(Some(first))
}

fn already_set_or_quiet(options: &Options, option: &str) -> Result<Execution, ServerError> {
    if options.has("-q") {
        Ok(Execution::default())
    } else {
        Err(ServerError::InvalidCommand(format!(
            "option is already set: {option}"
        )))
    }
}

#[derive(Debug, Default)]
struct Options {
    flags: Vec<String>,
    values: Vec<(String, String)>,
}

impl Options {
    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|candidate| candidate == flag)
    }

    fn value(&self, option: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find_map(|(name, value)| (name == option).then_some(value.as_str()))
    }

    fn values<'a>(&'a self, option: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.values
            .iter()
            .filter_map(move |(name, value)| (name == option).then_some(value.as_str()))
    }
}

fn respawn_environment(options: &Options) -> Vec<(String, String)> {
    options
        .values("-e")
        .filter_map(|value| value.split_once('='))
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

fn key_table(options: &Options) -> &str {
    options
        .value("-T")
        .unwrap_or(if options.has("-n") { "root" } else { "prefix" })
}

fn parse_command_options(
    command: &str,
    args: &[String],
) -> Result<(Options, Vec<String>), ServerError> {
    let spec = command_spec(command).expect("executable command has catalog metadata");
    let (options, positional) = parse_options_for_spec(args, spec)?;
    validate_options(command, spec, &options)?;
    Ok((options, positional))
}

fn parse_options_for_spec(
    args: &[String],
    spec: &zz_protocol::CommandSpec,
) -> Result<(Options, Vec<String>), ServerError> {
    let value_options = spec
        .options
        .iter()
        .filter_map(|option| option.value.map(|_| option.name))
        .collect::<Vec<_>>();
    let attached_options = spec
        .options
        .iter()
        .filter_map(|option| option.attached_value.then_some(option.name))
        .collect::<Vec<_>>();
    parse_options(args, &value_options, &attached_options)
}

fn validate_options(
    command: &str,
    spec: &zz_protocol::CommandSpec,
    options: &Options,
) -> Result<(), ServerError> {
    for name in options
        .flags
        .iter()
        .map(String::as_str)
        .chain(options.values.iter().map(|(name, _)| name.as_str()))
    {
        let Some(option) = spec.option(name) else {
            return Err(ServerError::InvalidCommand(format!(
                "{command} does not support {name}"
            )));
        };
        if option.unsupported {
            return Err(ServerError::UnsupportedCommand(format!("{command} {name}")));
        }
    }
    Ok(())
}

fn parse_options(
    args: &[String],
    value_options: &[&str],
    attached_options: &[&str],
) -> Result<(Options, Vec<String>), ServerError> {
    let mut options = Options::default();
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if !argument.starts_with('-') || argument == "-" {
            break;
        }
        index += 1;
        if argument == "--" {
            break;
        }
        let mut cluster = argument[1..].chars();
        while let Some(character) = cluster.next() {
            let name = format!("-{character}");
            if attached_options.contains(&name.as_str()) {
                let attached = cluster.as_str();
                if attached.is_empty() {
                    options.flags.push(name);
                } else {
                    options.values.push((name, attached.to_owned()));
                }
                break;
            }
            if !value_options.contains(&name.as_str()) {
                options.flags.push(name);
                continue;
            }
            let attached = cluster.as_str();
            let value = if attached.is_empty() {
                let value = required_arg(args, index, &name)?.to_owned();
                index += 1;
                value
            } else {
                attached.to_owned()
            };
            options.values.push((name, value));
            break;
        }
    }
    Ok((options, args[index..].to_vec()))
}

fn required_arg<'a>(
    args: &'a [String],
    index: usize,
    option: &str,
) -> Result<&'a str, ServerError> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| ServerError::InvalidCommand(format!("{option} requires an argument")))
}

fn reject_positionals(command: &str, positional: &[String]) -> Result<(), ServerError> {
    if let Some(argument) = positional.first() {
        Err(ServerError::InvalidCommand(format!(
            "{command} does not take positional arguments: {argument}"
        )))
    } else {
        Ok(())
    }
}

fn parse_resize_adjustment(value: &str) -> Result<i32, ServerError> {
    value
        .parse::<i32>()
        .map_err(|_| ServerError::InvalidCommand("adjustment invalid".to_owned()))
}

fn parse_resize_size(value: &str, extent: u16, dimension: &str) -> Result<u16, ServerError> {
    let (number, maximum) = value
        .strip_suffix('%')
        .map_or((value, i64::from(PANE_MAXIMUM)), |percentage| {
            (percentage, 1_000)
        });
    let number = number
        .parse::<i64>()
        .map_err(|_| ServerError::InvalidCommand(format!("{dimension} invalid")))?;
    if number < 0 {
        return Err(ServerError::InvalidCommand(format!(
            "{dimension} too small"
        )));
    }
    if number > maximum {
        return Err(ServerError::InvalidCommand(format!(
            "{dimension} too large"
        )));
    }
    let cells = if value.ends_with('%') {
        u64::from(extent) * u64::try_from(number).expect("percentage is nonnegative") / 100
    } else {
        u64::try_from(number).expect("pane size is nonnegative")
    };
    u16::try_from(cells)
        .ok()
        .filter(|cells| *cells <= PANE_MAXIMUM)
        .ok_or_else(|| ServerError::InvalidCommand(format!("{dimension} too large")))
}

fn exactly_one_argument<'a>(
    command: &str,
    positional: &'a [String],
) -> Result<&'a str, ServerError> {
    if positional.len() == 1 {
        Ok(positional[0].as_str())
    } else {
        Err(ServerError::InvalidCommand(format!(
            "{command} requires exactly one new name"
        )))
    }
}

fn pane_kind_snapshot(kind: &PaneKind) -> PaneKindSnapshot {
    match kind {
        PaneKind::Picker { .. } => PaneKindSnapshot::Picker,
        PaneKind::Terminal => PaneKindSnapshot::Terminal,
        PaneKind::Browser(browser) => PaneKindSnapshot::Browser(browser.clone()),
        PaneKind::Agent(agent) => PaneKindSnapshot::Agent(agent.clone()),
        PaneKind::Editor(editor) => PaneKindSnapshot::Editor(editor.clone()),
    }
}

/// Where a freshly created window lands: an explicit index makes the command
/// fail when that slot is taken, `None` takes the lowest free index.
struct WindowDestination {
    session: SessionId,
    index: Option<u32>,
}

/// tmux's target-window-with-index resolution, the form `new-window -t` and
/// `break-pane -t` use: the index need not exist yet, and a bare numeric
/// target is a window index in the current session before anything else —
/// even when a session shares that name (cmd-find.c tries the window part
/// first and only falls back to a session lookup when every window
/// interpretation fails).
fn window_destination(
    state: &MuxState,
    target: Option<&str>,
    context: &ExecutionContext,
) -> Result<WindowDestination, ServerError> {
    let (session, index) =
        state.resolve_window_index_target(target, context.session, context.window)?;
    Ok(WindowDestination { session, index })
}

fn session_named(state: &MuxState, name: &str) -> Option<SessionId> {
    state
        .sessions
        .values()
        .find(|session| session.name == name)
        .map(|session| session.id)
}

fn window_index(state: &MuxState, window: WindowId) -> Result<u32, ServerError> {
    state
        .windows
        .get(&window)
        .map(|window| window.index)
        .ok_or_else(|| ServerError::MissingTarget(window.to_string()))
}

fn session_active_window(state: &MuxState, session: SessionId) -> Result<WindowId, ServerError> {
    state
        .sessions
        .get(&session)
        .map(|session| session.active_window)
        .ok_or_else(|| ServerError::MissingTarget(session.to_string()))
}

fn window_active_pane(state: &MuxState, window: WindowId) -> Result<PaneId, ServerError> {
    state
        .windows
        .get(&window)
        .map(|window| window.active_pane)
        .ok_or_else(|| ServerError::MissingTarget(window.to_string()))
}

fn shell_command_positional(positional: &[String]) -> Option<String> {
    (!positional.is_empty()).then(|| positional.join(" "))
}

fn spawn_cwd_source(
    engine: &MuxEngine,
    options: &Options,
    origin: Option<PaneId>,
    kind: &PaneKind,
    hooks: &mut impl StatusHooks,
) -> (Option<PaneId>, Option<String>) {
    if !matches!(kind, PaneKind::Terminal) {
        return (None, None);
    }
    let inherit_cwd_from = origin.and_then(|origin| engine.state.cwd_donor(origin));
    let cwd = options.value("-c").and_then(|value| {
        let format_context = origin
            .and_then(|pane| ExecutionContext::for_pane(&engine.state, pane))
            .map_or_else(FormatContext::default, |origin| FormatContext {
                session: origin.session,
                window: origin.window,
                pane: origin.pane,
                active_session: origin.session,
                format_type: FormatType::Pane,
            });
        let expanded = expand_format_with_hooks(value, engine, format_context, hooks);
        (!expanded.is_empty()).then_some(expanded)
    });
    (inherit_cwd_from, cwd)
}

fn browser_from_args(
    options: &Options,
    positional: &[String],
) -> Result<BrowserDescriptor, ServerError> {
    let profile = normalize_browser_profile_name(options.value("-p").unwrap_or("default"))
        .map_err(|error| ServerError::InvalidCommand(error.to_string()))?;
    Ok(BrowserDescriptor::single(
        positional
            .first()
            .cloned()
            .unwrap_or_else(|| "about:blank".to_owned()),
        profile,
    ))
}

fn next_session_name(state: &MuxState) -> String {
    let mut used = vec![false; state.sessions.len().saturating_add(1)];
    for session in state.sessions.values() {
        let bytes = session.name.as_bytes();
        if bytes.is_empty()
            || (bytes.len() > 1 && bytes[0] == b'0')
            || !bytes.iter().all(u8::is_ascii_digit)
        {
            continue;
        }
        let Ok(candidate) = session.name.parse::<usize>() else {
            continue;
        };
        if let Some(slot) = used.get_mut(candidate) {
            *slot = true;
        }
    }
    used.iter()
        .position(|used| !used)
        .expect("one of n + 1 numeric names is unused")
        .to_string()
}

fn expand_short_formats(
    input: &str,
    session_name: Option<&str>,
    window_name: Option<&str>,
) -> String {
    let mut expanded = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '#' {
            expanded.push(character);
            continue;
        }
        match (characters.peek().copied(), session_name, window_name) {
            (Some('S'), Some(name), _) | (Some('W'), _, Some(name)) => {
                characters.next();
                expanded.push_str(name);
            }
            _ => expanded.push(character),
        }
    }
    expanded
}

fn key_token(value: &str) -> KeyToken {
    const NAMED: &[&str] = &[
        "Enter", "Escape", "Space", "Tab", "BSpace", "Up", "Down", "Left", "Right", "Home", "End",
        "PPage", "NPage", "DC", "IC",
    ];
    if NAMED.contains(&value)
        || value.starts_with("C-")
        || value.starts_with("M-")
        || value
            .strip_prefix('F')
            .is_some_and(|number| number.parse::<u8>().is_ok())
    {
        KeyToken::Named(value.to_owned())
    } else {
        KeyToken::Literal(value.to_owned())
    }
}

/// tmux's `send-keys -H` is strtol(16) clamped to one byte, written raw to the
/// pane. `KeyToken::Literal` carries UTF-8 text, so only the ASCII range maps
/// to the same bytes tmux writes; high bytes are refused rather than silently
/// re-encoded.
fn hex_key_token(value: &str) -> Result<KeyToken, ServerError> {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    let code = u32::from_str_radix(digits, 16).map_err(|_| {
        ServerError::InvalidCommand(format!("send-keys -H needs a character code: {value}"))
    })?;
    match code {
        0x00..=0x7f => Ok(KeyToken::Literal(
            char::from_u32(code).expect("ascii range").to_string(),
        )),
        0x80..=0xff => Err(ServerError::UnsupportedCommand(format!(
            "send-keys -H {value} (raw bytes above 7f)"
        ))),
        _ => Err(ServerError::InvalidCommand(format!(
            "send-keys -H needs a character code: {value}"
        ))),
    }
}

fn repeat_count(command: &str, options: &Options) -> Result<usize, ServerError> {
    let Some(value) = options.value("-N") else {
        return Ok(1);
    };
    match value.parse::<usize>() {
        Ok(count) if count > 0 => Ok(count),
        _ => Err(ServerError::InvalidCommand(format!(
            "{command} -N needs a positive repeat count: {value}"
        ))),
    }
}

/// tmux hands the repeat count to the copy-mode command, which uses it only when
/// it is a movement; repeating a copy or a cancel would mean something else.
/// The copy-mode commands that honor tmux's `-N` repeat prefix: exactly the
/// window-copy handlers that read `wme->prefix` and loop. Everything else runs
/// once no matter the count (a repeated `rectangle-toggle` or copy would be a
/// different command, not a repeated one).
fn repeats_in_copy_mode(action: &CopyModeAction) -> bool {
    matches!(
        action,
        CopyModeAction::Left
            | CopyModeAction::Right
            | CopyModeAction::Up
            | CopyModeAction::Down
            | CopyModeAction::PageUp
            | CopyModeAction::PageDown
            | CopyModeAction::HalfPageUp
            | CopyModeAction::HalfPageDown
            | CopyModeAction::ScrollUp
            | CopyModeAction::ScrollDown
            | CopyModeAction::NextWord
            | CopyModeAction::PreviousWord
            | CopyModeAction::NextWordEnd
            | CopyModeAction::NextSpace
            | CopyModeAction::PreviousSpace
            | CopyModeAction::NextSpaceEnd
            | CopyModeAction::NextParagraph
            | CopyModeAction::PreviousParagraph
            | CopyModeAction::Jump(_)
            | CopyModeAction::RepeatJump { .. }
            | CopyModeAction::SearchAgain { .. }
    )
}

fn copy_mode_action(
    command: &str,
    arguments: &[String],
    options: &Options,
    set_clipboard: SetClipboard,
    copy_command: &str,
) -> Result<CopyModeAction, ServerError> {
    let output = options.has("-o");
    match command {
        "cursor-left" => Ok(CopyModeAction::Left),
        "cursor-right" => Ok(CopyModeAction::Right),
        "cursor-up" => Ok(CopyModeAction::Up),
        "cursor-down" => Ok(CopyModeAction::Down),
        "page-up" => Ok(CopyModeAction::PageUp),
        "page-down" => Ok(CopyModeAction::PageDown),
        "halfpage-up" => Ok(CopyModeAction::HalfPageUp),
        "halfpage-down" => Ok(CopyModeAction::HalfPageDown),
        "history-top" => Ok(CopyModeAction::Top),
        "history-bottom" => Ok(CopyModeAction::Bottom),
        "top-line" => Ok(CopyModeAction::TopLine),
        "middle-line" => Ok(CopyModeAction::MiddleLine),
        "bottom-line" => Ok(CopyModeAction::BottomLine),
        "start-of-line" => Ok(CopyModeAction::StartOfLine),
        "back-to-indentation" => Ok(CopyModeAction::BackToIndentation),
        "end-of-line" => Ok(CopyModeAction::EndOfLine),
        "next-word" => Ok(CopyModeAction::NextWord),
        "previous-word" => Ok(CopyModeAction::PreviousWord),
        "next-word-end" => Ok(CopyModeAction::NextWordEnd),
        "next-space" => Ok(CopyModeAction::NextSpace),
        "previous-space" => Ok(CopyModeAction::PreviousSpace),
        "next-space-end" => Ok(CopyModeAction::NextSpaceEnd),
        "scroll-up" => Ok(CopyModeAction::ScrollUp),
        "scroll-down" => Ok(CopyModeAction::ScrollDown),
        "scroll-middle" => Ok(CopyModeAction::ScrollMiddle),
        "goto-line" => copy_goto_line_action(arguments),
        "next-matching-bracket" => Ok(CopyModeAction::NextMatchingBracket),
        "search-forward-cursor-word" => Ok(CopyModeAction::SearchCursorWord {
            direction: SearchDirection::Forward,
        }),
        "search-backward-cursor-word" => Ok(CopyModeAction::SearchCursorWord {
            direction: SearchDirection::Backward,
        }),
        "next-paragraph" => Ok(CopyModeAction::NextParagraph),
        "previous-paragraph" => Ok(CopyModeAction::PreviousParagraph),
        "next-prompt" => Ok(CopyModeAction::NextPrompt { output }),
        "previous-prompt" => Ok(CopyModeAction::PreviousPrompt { output }),
        "search-again" => Ok(CopyModeAction::SearchAgain { reverse: false }),
        "search-reverse" => Ok(CopyModeAction::SearchAgain { reverse: true }),
        "begin-selection" => Ok(CopyModeAction::StartSelection),
        "select-word" => Ok(CopyModeAction::SelectWord),
        "select-line" => Ok(CopyModeAction::SelectLine),
        "clear-selection" | "stop-selection" => Ok(CopyModeAction::ClearSelection),
        "clear-selection-or-cancel" => Ok(CopyModeAction::ClearSelectionOrCancel),
        "rectangle-toggle" => Ok(CopyModeAction::ToggleRectangle),
        "rectangle-on" => Ok(CopyModeAction::RectangleOn),
        "rectangle-off" => Ok(CopyModeAction::RectangleOff),
        "other-end" => Ok(CopyModeAction::OtherEnd),
        "set-mark" => Ok(CopyModeAction::SetMark),
        "jump-to-mark" => Ok(CopyModeAction::JumpToMark),
        "jump-forward" => copy_jump_action(arguments, CopyJumpDirection::Forward, false),
        "jump-backward" => copy_jump_action(arguments, CopyJumpDirection::Backward, false),
        "jump-to-forward" => copy_jump_action(arguments, CopyJumpDirection::Forward, true),
        "jump-to-backward" => copy_jump_action(arguments, CopyJumpDirection::Backward, true),
        "jump-again" => Ok(CopyModeAction::RepeatJump { reverse: false }),
        "jump-reverse" => Ok(CopyModeAction::RepeatJump { reverse: true }),
        "copy-selection" => copy_selection_action(arguments, options, set_clipboard, true, false),
        "copy-selection-no-clear" => {
            copy_selection_action(arguments, options, set_clipboard, false, false)
        }
        "copy-selection-and-cancel" => {
            copy_selection_action(arguments, options, set_clipboard, true, true)
        }
        "append-selection" | "append-selection-and-cancel" => {
            if !arguments.is_empty() {
                return Err(ServerError::InvalidCommand(format!(
                    "send-keys -X {command} does not accept arguments"
                )));
            }
            Ok(CopyModeAction::copy_selection(CopyModeCopy {
                request_id: 0,
                clipboard: false,
                buffer: Some(PasteBufferAction::Append),
                pipe: None,
                clear_selection: true,
                cancel: command.ends_with("-and-cancel"),
            }))
        }
        "copy-pipe" | "copy-pipe-no-clear" | "copy-pipe-and-cancel" => copy_pipe_action(
            command,
            arguments,
            options,
            set_clipboard,
            copy_command,
            true,
        ),
        "pipe" | "pipe-no-clear" | "pipe-and-cancel" => copy_pipe_action(
            command,
            arguments,
            options,
            set_clipboard,
            copy_command,
            false,
        ),
        "copy-pipe-end-of-line" | "copy-pipe-end-of-line-and-cancel" => copy_pipe_action(
            command,
            arguments,
            options,
            set_clipboard,
            copy_command,
            true,
        )
        .map(copy_end_of_line_action),
        "copy-end-of-line" | "copy-end-of-line-and-cancel" => copy_selection_action(
            arguments,
            options,
            set_clipboard,
            true,
            command.ends_with("-and-cancel"),
        )
        .map(copy_end_of_line_action),
        "cancel" => Ok(CopyModeAction::Cancel),
        other => Err(ServerError::UnsupportedCommand(format!(
            "send-keys -X {other}"
        ))),
    }
}

fn copy_end_of_line_action(action: CopyModeAction) -> CopyModeAction {
    let CopyModeAction::CopySelection(copy) = action else {
        unreachable!("copy helper always returns a selection action");
    };
    CopyModeAction::CopyEndOfLine(copy)
}

fn copy_goto_line_action(arguments: &[String]) -> Result<CopyModeAction, ServerError> {
    let [line] = arguments else {
        return Err(ServerError::InvalidCommand(
            "send-keys -X goto-line needs exactly one line number".to_owned(),
        ));
    };
    let line = line.parse::<u32>().map_err(|_| {
        ServerError::InvalidCommand("copy-mode line number must be an integer".to_owned())
    })?;
    Ok(CopyModeAction::GotoLine(line))
}

fn copy_pipe_action(
    command_name: &str,
    arguments: &[String],
    options: &Options,
    set_clipboard: SetClipboard,
    copy_command: &str,
    copy: bool,
) -> Result<CopyModeAction, ServerError> {
    let maximum_arguments = if copy { 2 } else { 1 };
    if arguments.len() > maximum_arguments {
        return Err(ServerError::InvalidCommand(format!(
            "send-keys -X {command_name} accepts at most {maximum_arguments} arguments"
        )));
    }
    let pipe = arguments.first().map_or(copy_command, String::as_str);
    if pipe.len() > MAX_COPY_COMMAND_BYTES {
        return Err(ServerError::InvalidCommand(format!(
            "copy pipe command exceeds {MAX_COPY_COMMAND_BYTES} bytes"
        )));
    }
    let prefix = if copy {
        arguments.get(1).cloned()
    } else {
        None
    };
    Ok(CopyModeAction::copy_selection(CopyModeCopy {
        request_id: 0,
        clipboard: copy && !options.has("-C") && set_clipboard != SetClipboard::Off,
        buffer: (copy && !options.has("-P")).then_some(PasteBufferAction::Create { prefix }),
        pipe: (!pipe.is_empty()).then(|| pipe.to_owned()),
        clear_selection: !command_name.ends_with("-no-clear"),
        cancel: command_name.ends_with("-and-cancel"),
    }))
}

fn copy_jump_action(
    arguments: &[String],
    direction: CopyJumpDirection,
    to: bool,
) -> Result<CopyModeAction, ServerError> {
    if arguments.len() != 1 || arguments[0].is_empty() {
        return Err(ServerError::InvalidCommand(
            "copy-mode jump needs exactly one nonempty target".to_owned(),
        ));
    }
    Ok(CopyModeAction::Jump(CopyJump {
        target: arguments[0].clone(),
        direction,
        to,
    }))
}

fn copy_selection_action(
    arguments: &[String],
    options: &Options,
    set_clipboard: SetClipboard,
    clear_selection: bool,
    cancel: bool,
) -> Result<CopyModeAction, ServerError> {
    if arguments.len() > 1 {
        return Err(ServerError::InvalidCommand(
            "copy-selection accepts at most one buffer prefix".to_owned(),
        ));
    }
    Ok(CopyModeAction::copy_selection(CopyModeCopy {
        request_id: 0,
        clipboard: !options.has("-C") && set_clipboard != SetClipboard::Off,
        buffer: (!options.has("-P")).then(|| PasteBufferAction::Create {
            prefix: arguments.first().cloned(),
        }),
        pipe: None,
        clear_selection,
        cancel,
    }))
}

fn bound_commands(tail: &[String]) -> Result<Vec<CommandInvocation>, ServerError> {
    let commands = if let [argument] = tail
        && let Some(body) = crate::parser::command_block_body(argument)
    {
        let parsed = crate::parse_config("<bind-key>", body);
        if let Some(diagnostic) = parsed.diagnostics.into_iter().next() {
            return Err(ServerError::InvalidCommand(diagnostic.message));
        }
        parsed
            .commands
            .into_iter()
            .map(|command| CommandInvocation::new(command.name, command.args))
            .collect::<Vec<_>>()
    } else {
        let mut commands = Vec::new();
        let mut segments = tail.split(|argument| argument == ";").peekable();
        while let Some(segment) = segments.next() {
            let Some((command, command_args)) = segment.split_first() else {
                if segments.peek().is_none() && !commands.is_empty() {
                    break;
                }
                return Err(ServerError::InvalidCommand(
                    "bind-key command chain contains an empty command".to_owned(),
                ));
            };
            commands.push(CommandInvocation::new(
                command,
                command_args.iter().cloned(),
            ));
        }
        commands
    };
    for command in &commands {
        validate_bound_command(command)?;
    }
    Ok(commands)
}

fn validate_bound_command(command: &CommandInvocation) -> Result<(), ServerError> {
    let name = canonical_command(&command.name);
    if let Some(spec) = command_spec(name) {
        let (options, _) = parse_options_for_spec(&command.args, spec)?;
        return validate_options(name, spec, &options);
    }
    if name == "copy-mode-repeat" || CommandSpec::DAEMON_COMMAND_NAMES.contains(&name) {
        return Ok(());
    }
    if CommandSpec::UNIMPLEMENTED_TMUX_COMMANDS.contains(&name) {
        return Err(ServerError::UnsupportedCommand(format!("bind-key {name}")));
    }
    Err(ServerError::InvalidCommand(format!(
        "unknown command: {name}"
    )))
}

fn format_command(command: &CommandInvocation) -> String {
    std::iter::once(command.name.as_str())
        .chain(command.args.iter().map(String::as_str))
        .map(|part| {
            if part.contains(char::is_whitespace) {
                format!("'{part}'")
            } else {
                part.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str, args: &[&str]) -> CommandInvocation {
        CommandInvocation::new(name, args.iter().copied())
    }

    fn window_layout(engine: &MuxEngine, session: SessionId) -> Vec<String> {
        engine.state.sessions[&session]
            .windows
            .iter()
            .map(|window| {
                let window = &engine.state.windows[window];
                format!("{}:{}", window.index, window.name)
            })
            .collect()
    }

    fn window_index_named(engine: &MuxEngine, session: SessionId, name: &str) -> u32 {
        engine.state.sessions[&session]
            .windows
            .iter()
            .find_map(|window| {
                let window = &engine.state.windows[window];
                (window.name == name).then_some(window.index)
            })
            .expect("named window")
    }

    fn absolute_test_path(name: &str) -> PathBuf {
        std::env::current_dir()
            .expect("current test directory")
            .join("target")
            .join("zz-mux-tests")
            .join(name)
    }

    #[test]
    fn creates_navigates_and_removes_mux_resources() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        let created = engine
            .execute(&mut context, &command("new", &["-s", "work"]))
            .unwrap();
        assert!(matches!(created.effects[0], MuxEffect::PaneCreated { .. }));
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("splitw", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        assert_ne!(first, second);
        assert!(engine.state.validate().is_ok());

        engine
            .execute(&mut context, &command("selectp", &["-L"]))
            .unwrap();
        assert_eq!(context.pane, Some(first));

        engine
            .execute(
                &mut context,
                &command("selectp", &["-t", &first.to_string()]),
            )
            .unwrap();
        assert_eq!(context.pane, Some(first));
        engine
            .execute(
                &mut context,
                &command("killp", &["-t", &second.to_string()]),
            )
            .unwrap();
        assert!(engine.state.pane(second).is_none());
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn new_session_requests_attachment_after_creating_its_terminal() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        let execution = engine
            .execute(&mut context, &command("new-session", &[]))
            .expect("new session");
        let session = context.session.expect("new session id");
        let pane = context.pane.expect("new pane id");

        assert!(matches!(
            execution.effects.as_slice(),
            [
                MuxEffect::PaneCreated { pane: created, .. },
                MuxEffect::Attach {
                    session: attached,
                    detach_others: false,
                },
                MuxEffect::SnapshotChanged,
            ] if *created == pane && *attached == session
        ));
    }

    #[test]
    fn detached_new_session_does_not_request_attachment() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        let execution = engine
            .execute(&mut context, &command("new-session", &["-d"]))
            .expect("detached new session");
        let pane = context.pane.expect("new pane id");

        assert!(matches!(
            execution.effects.as_slice(),
            [
                MuxEffect::PaneCreated { pane: created, .. },
                MuxEffect::SnapshotChanged,
            ] if *created == pane
        ));
    }

    #[test]
    fn most_recent_context_drives_originless_new_session_cwd() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "first"]),
            )
            .expect("first session");
        let first = context.session.expect("first session id");
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "second"]),
            )
            .expect("second session");
        let second = context.session.expect("second session id");
        let second_pane = context.pane.expect("second session pane");
        assert!(engine.set_pane_runtime_facts(
            second_pane,
            PaneRuntimeFacts {
                current_path: "/private/tmp".to_owned(),
                ..PaneRuntimeFacts::default()
            },
        ));

        assert_eq!(
            engine.state.default_context().map(|state| state.0),
            Some(first)
        );
        let (session, window, pane) = engine.state.most_recent_context().expect("recent context");
        assert_eq!(session, second);

        let mut command_context = ExecutionContext {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
        };
        let execution = engine
            .execute(
                &mut command_context,
                &command(
                    "new-session",
                    &["-d", "-s", "third", "-c", "#{pane_current_path}"],
                ),
            )
            .expect("originless new session");
        assert!(matches!(
            execution.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: Some(cwd),
                ..
            }) if *source == second_pane && cwd == "/private/tmp"
        ));
    }

    #[test]
    fn command_targeting_does_not_change_the_most_recent_session() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "A"]))
            .expect("first session");
        let first = context.session.expect("first session");
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "B"]))
            .expect("second session");
        let second = context.session.expect("second session");
        let (session, window, pane) = engine.state.most_recent_context().expect("recent context");
        let mut command_context = ExecutionContext {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
        };

        engine
            .execute(
                &mut command_context,
                &command("select-window", &["-t", "A:0"]),
            )
            .expect("target first session");

        assert_eq!(command_context.session, Some(first));
        assert_eq!(
            engine.state.most_recent_context().map(|context| context.0),
            Some(second)
        );
    }

    #[test]
    fn most_recent_session_controls_empty_session_window_targets() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "A"]))
            .expect("first session");
        engine
            .execute(&mut context, &command("new-window", &["-d", "-t", "A"]))
            .expect("second window in first session");
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "B"]))
            .expect("second session");
        let (session, window, pane) = engine.state.most_recent_context().expect("recent context");
        let mut command_context = ExecutionContext {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
        };
        engine
            .execute(
                &mut command_context,
                &command("select-window", &["-t", "A:0"]),
            )
            .expect("target first session");

        let (session, window, pane) = engine.state.most_recent_context().expect("recent context");
        let mut followup_context = ExecutionContext {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
        };
        assert!(matches!(
            engine.execute(
                &mut followup_context,
                &command("list-panes", &["-t", ":1"]),
            ),
            Err(ServerError::WindowNotFound(target)) if target == "1"
        ));
    }

    #[test]
    fn first_window_uses_the_default_extent() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        let window = context.window.unwrap();
        assert_eq!(engine.state.windows[&window].layout.extent(), (80, 24));
    }

    #[test]
    fn new_session_honors_explicit_window_extent() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        engine
            .execute(
                &mut context,
                &command(
                    "new-session",
                    &["-d", "-s", "work", "-x", "200", "-y", "50"],
                ),
            )
            .unwrap();

        let window = context.window.unwrap();
        assert_eq!(engine.state.windows[&window].layout.extent(), (200, 50));
    }

    #[test]
    fn measurement_write_back_bumps_generation_and_reports_change() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();
        let generation = engine.state.generation();
        assert!(engine.set_pane_geometry(pane, 200, 50));
        assert!(engine.state.generation() > generation);
        let repeat_generation = engine.state.generation();
        assert!(!engine.set_pane_geometry(pane, 200, 50));
        assert_eq!(engine.state.generation(), repeat_generation);
    }

    #[test]
    fn new_window_inherits_the_active_window_extent() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        let first_window = context.window.unwrap();
        let first_pane = context.pane.unwrap();
        engine.set_pane_geometry(first_pane, 200, 50);

        engine
            .execute(&mut context, &command("new-window", &["-d"]))
            .unwrap();

        let inherited = engine.state.sessions[&session]
            .windows
            .iter()
            .copied()
            .find(|window| *window != first_window)
            .unwrap();
        assert_eq!(engine.state.windows[&inherited].layout.extent(), (200, 50));
    }

    #[test]
    fn break_pane_destination_inherits_the_active_window_extent() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let original_window = context.window.unwrap();
        let first = context.pane.unwrap();
        engine.set_pane_geometry(first, 200, 50);
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let moving = context.pane.unwrap();

        engine
            .execute(
                &mut context,
                &command("break-pane", &["-d", "-s", &moving.to_string()]),
            )
            .unwrap();

        let inherited = engine.state.window_for_pane(moving).unwrap();
        assert_ne!(inherited, original_window);
        assert_eq!(engine.state.windows[&inherited].layout.extent(), (200, 50));
    }

    #[test]
    fn new_session_dash_a_attaches_to_the_named_session_instead_of_failing() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("first session");
        let session = context.session.expect("session id");
        let pane = context.pane.expect("pane id");
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "other"]),
            )
            .expect("second session");

        let attached = engine
            .execute(&mut context, &command("new-session", &["-A", "-s", "work"]))
            .expect("attach or create");
        assert_eq!(
            attached.effects,
            [MuxEffect::Attach {
                session,
                detach_others: false,
            }]
        );
        assert_eq!(context.session, Some(session));
        assert_eq!(context.pane, Some(pane));
        assert_eq!(engine.state.sessions.len(), 2);

        let ignores_detach = engine
            .execute(
                &mut context,
                &command("new-session", &["-A", "-d", "-s", "work"]),
            )
            .expect("attach or create");
        assert_eq!(
            ignores_detach.effects,
            [MuxEffect::Attach {
                session,
                detach_others: false,
            }]
        );

        let detaches_others = engine
            .execute(
                &mut context,
                &command("new-session", &["-A", "-D", "-s", "work"]),
            )
            .expect("attach or create");
        assert_eq!(
            detaches_others.effects,
            [MuxEffect::Attach {
                session,
                detach_others: true,
            }]
        );

        let bare = engine
            .execute(&mut context, &command("new-session", &["-A"]))
            .expect("attach to the current session");
        assert_eq!(bare.effects.len(), 1);
        assert!(matches!(bare.effects[0], MuxEffect::Attach { .. }));
        assert_eq!(engine.state.sessions.len(), 2);

        engine
            .execute(
                &mut context,
                &command("new-session", &["-A", "-d", "-s", "fresh"]),
            )
            .expect("create");
        assert!(
            engine
                .state
                .sessions
                .values()
                .any(|session| session.name == "fresh")
        );

        assert!(matches!(
            engine.execute(&mut context, &command("new-session", &["-s", "work"])),
            Err(ServerError::InvalidCommand(_))
        ));
    }

    #[test]
    fn new_window_dash_d_creates_without_selecting() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        let first = context.window.unwrap();

        let created = engine
            .execute(&mut context, &command("new-window", &["-d", "-n", "logs"]))
            .expect("detached window");
        assert_eq!(context.window, Some(first));
        assert_eq!(engine.state.sessions[&session].active_window, first);
        let pane = match created.effects.first() {
            Some(MuxEffect::PaneCreated { pane, .. }) => *pane,
            other => panic!("expected a created pane: {other:?}"),
        };
        let created_window = engine.state.window_for_pane(pane).expect("window");
        assert_eq!(engine.state.windows[&created_window].name, "logs");
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn new_window_dash_a_inserts_after_the_target_and_shifts_the_run_up() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        for name in ["one", "two", "three"] {
            engine
                .execute(&mut context, &command("new-window", &["-n", name]))
                .unwrap();
        }
        assert_eq!(
            window_layout(&engine, session),
            ["0:0", "1:one", "2:two", "3:three"]
        );

        engine
            .execute(
                &mut context,
                &command("new-window", &["-a", "-t", "1", "-n", "inserted"]),
            )
            .expect("insert after window 1");
        assert_eq!(
            window_layout(&engine, session),
            ["0:0", "1:one", "2:inserted", "3:two", "4:three"]
        );
        assert_eq!(
            engine.state.windows[&engine.state.sessions[&session].active_window].name,
            "inserted"
        );
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn new_window_dash_a_keeps_an_explicit_free_index() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-a", "-t", "5"]),
            )
            .unwrap();
        assert_eq!(window_layout(&engine, session), ["0:0", "5:5"]);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn new_window_dash_k_replaces_the_window_holding_the_index() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        let doomed = context.window.unwrap();
        let doomed_pane = context.pane.unwrap();

        let replaced = engine
            .execute(
                &mut context,
                &command("new-window", &["-k", "-t", "0", "-n", "replacement"]),
            )
            .expect("replace the only window");
        assert!(
            replaced
                .effects
                .contains(&MuxEffect::PanesRemoved(vec![doomed_pane]))
        );
        assert!(!engine.state.windows.contains_key(&doomed));
        assert_eq!(window_layout(&engine, session), ["0:replacement"]);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn new_window_dash_s_selects_an_existing_window_with_the_same_name() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "logs"]))
            .unwrap();
        let logs = context.window.unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "editor"]))
            .unwrap();

        let selected = engine
            .execute(&mut context, &command("new-window", &["-S", "-n", "logs"]))
            .expect("select the existing window");
        assert_eq!(selected.effects, [MuxEffect::SnapshotChanged]);
        assert_eq!(context.window, Some(logs));
        assert_eq!(engine.state.sessions[&session].windows.len(), 3);

        engine
            .execute(&mut context, &command("new-window", &["-S", "-n", "fresh"]))
            .expect("create the missing window");
        assert_eq!(engine.state.sessions[&session].windows.len(), 4);
    }

    #[test]
    fn break_pane_dash_t_names_the_destination_index() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        let home = context.window.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let broken = context.pane.unwrap();

        engine
            .execute(
                &mut context,
                &command("break-pane", &["-d", "-t", "work:5", "-n", "moved"]),
            )
            .expect("break to an explicit index");
        assert_eq!(window_layout(&engine, session), ["0:0", "5:moved"]);
        assert_eq!(context.window, Some(home));
        assert_eq!(engine.state.sessions[&session].active_window, home);

        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("break-pane", &["-d", "-t", "work:5"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "index in use: 5"
        ));

        engine
            .execute(&mut context, &command("break-pane", &["-t", "7"]))
            .expect("break to a bare index in the current session");
        assert_eq!(engine.state.window_for_pane(second), context.window);
        assert_eq!(
            engine.state.windows[&context.window.unwrap()].index,
            7,
            "the broken pane lands at the requested index"
        );
        let first_break = engine
            .state
            .window_for_pane(broken)
            .expect("the first broken pane kept its window");
        assert_eq!(engine.state.windows[&first_break].index, 5);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn break_pane_moves_a_single_pane_window_without_replacing_it() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("rename-window", &["kept"]))
            .unwrap();
        let session = context.session.unwrap();
        let window = context.window.unwrap();
        let pane = context.pane.unwrap();
        let layout = engine.state.windows[&window].layout.clone();

        engine
            .execute(&mut context, &command("break-pane", &["-d", "-t", "1"]))
            .unwrap();

        assert_eq!(engine.state.window_for_pane(pane), Some(window));
        assert_eq!(engine.state.windows[&window].name, "kept");
        assert_eq!(engine.state.windows[&window].index, 1);
        assert_eq!(engine.state.windows[&window].layout, layout);
        assert_eq!(engine.state.sessions[&session].windows, [window]);
        assert_eq!(engine.state.sessions[&session].active_window, window);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn base_index_drives_default_allocation_but_not_explicit_targets() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "base-index", "1"]),
            )
            .expect("global base index");
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("session at the global base");
        let session = context.session.expect("session id");
        let first_window = context.window.expect("first window");
        assert_eq!(engine.state.windows[&first_window].index, 1);
        assert_eq!(engine.state.windows[&first_window].name, "1");

        engine
            .execute(
                &mut context,
                &command("split-window", &["-h", "-t", "work:1.0"]),
            )
            .expect("split first window");
        let moving = context.pane.expect("split pane");
        engine
            .execute(
                &mut context,
                &command("break-pane", &["-d", "-s", &moving.to_string()]),
            )
            .expect("default break destination");
        let broken_window = engine.state.window_for_pane(moving).expect("broken window");
        assert_eq!(engine.state.windows[&broken_window].index, 2);

        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "work:0", "-n", "low"]),
            )
            .expect("explicit index below the base");
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "work", "-n", "next"]),
            )
            .expect("next default index");
        assert_eq!(window_index_named(&engine, session, "low"), 0);
        assert_eq!(window_index_named(&engine, session, "next"), 3);

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-t", &session.to_string(), "base-index", "5"],
                ),
            )
            .expect("session base override");
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "work", "-n", "override"]),
            )
            .expect("session default index");
        assert_eq!(window_index_named(&engine, session, "override"), 5);

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-u", "-t", &session.to_string(), "base-index"],
                ),
            )
            .expect("restore session inheritance");
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "work", "-n", "inherited"]),
            )
            .expect("inherited default index");
        assert_eq!(window_index_named(&engine, session, "inherited"), 4);

        engine
            .execute(&mut context, &command("set-option", &["-gu", "base-index"]))
            .expect("restore global default");
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "zero"]))
            .expect("default-base session");
        let zero_window = context.window.expect("zero-base window");
        assert_eq!(engine.state.windows[&zero_window].index, 0);
        assert!(engine.state.validate().is_ok());

        let mut edge = MuxEngine::default();
        let mut edge_context = ExecutionContext::default();
        edge.execute(
            &mut edge_context,
            &command(
                "set-option",
                &["-g", "base-index", &MAX_BASE_INDEX.to_string()],
            ),
        )
        .expect("maximum base index");
        edge.execute(&mut edge_context, &command("new-session", &["-s", "edge"]))
            .expect("maximum-base session");
        assert_eq!(
            edge.state.windows[&edge_context.window.expect("edge window")].index,
            MAX_BASE_INDEX
        );
        edge.execute(
            &mut edge_context,
            &command("new-window", &["-d", "-t", "edge", "-n", "wrapped"]),
        )
        .expect("wrapped allocation");
        assert_eq!(
            window_index_named(
                &edge,
                edge_context.session.expect("edge session"),
                "wrapped"
            ),
            0
        );
    }

    #[test]
    fn attach_session_preserves_detach_others_flag_in_effect() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "shared"]),
            )
            .expect("detached session");
        let session = context.session.expect("session id");

        let execution = engine
            .execute(
                &mut context,
                &command("attach-session", &["-d", "-t", "shared"]),
            )
            .expect("steal attach");

        assert_eq!(
            execution.effects,
            [MuxEffect::Attach {
                session,
                detach_others: true,
            }]
        );
    }

    #[test]
    fn session_listing_is_name_sorted_but_the_s_loop_stays_creation_sorted() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        for name in ["w", "A", "B"] {
            engine
                .execute(&mut context, &command("new-session", &["-d", "-s", name]))
                .unwrap();
        }

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-sessions", &["-F", "#{session_name}"]),
                )
                .unwrap()
                .output,
            "A\nB\nw"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("display-message", &["-p", "#{S:#{session_name} }"]),
                )
                .unwrap()
                .output,
            "w A B "
        );
    }

    #[test]
    fn list_formats_are_contextual_and_default_output_is_unchanged() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("rename-window", &["main"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();

        assert_eq!(
            engine
                .execute(&mut context, &command("list-sessions", &[]))
                .unwrap()
                .output,
            "work: 1 windows (id $0)"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("list-windows", &[]))
                .unwrap()
                .output,
            "0: main* (2 panes) [id @0]"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("list-panes", &[]))
                .unwrap()
                .output,
            "%0: terminal- terminal\n%1: terminal* terminal"
        );

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-sessions",
                        &["-F", "#{session_id}:#S:#{session_windows}"],
                    ),
                )
                .unwrap()
                .output,
            "$0:work:1"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-windows",
                        &["-F", "#{session_id}:#{window_id}:#I:#W:#F"],
                    ),
                )
                .unwrap()
                .output,
            "$0:@0:0:main:*"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &["-F", "#{session_id}:#{window_id}:#{pane_id}:#P:#T",],
                    ),
                )
                .unwrap()
                .output,
            "$0:@0:%0:0:terminal\n$0:@0:%1:1:terminal"
        );
        assert!(
            engine
                .execute(&mut context, &command("list-sessions", &["-x"]))
                .is_err()
        );
    }

    #[test]
    fn pane_base_index_drives_targets_formats_and_window_inheritance() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("session");
        let window = context.window.expect("window");
        let first = context.pane.expect("first pane");
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .expect("second pane");
        let second = context.pane.expect("second pane");

        let changed = engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "pane-base-index", "1"]),
            )
            .expect("global pane base");
        assert_eq!(changed.effects, [MuxEffect::SnapshotChanged]);
        assert_eq!(engine.pane_index(window, first), Some(1));
        assert_eq!(engine.pane_index(window, second), Some(2));
        assert_eq!(
            engine
                .resolve_pane(Some("work:0.1"), Some(window), Some(second))
                .unwrap(),
            first
        );
        assert!(matches!(
            engine.resolve_pane(Some("work:0.0"), Some(window), Some(second)),
            Err(ServerError::PaneNotFound(target)) if target == "0"
        ));
        assert_eq!(
            engine
                .resolve_pane(Some(&second.to_string()), Some(window), Some(first))
                .unwrap(),
            second
        );

        engine
            .execute(&mut context, &command("select-pane", &["-t", "work:0.1"]))
            .expect("one-based pane target");
        assert_eq!(context.pane, Some(first));
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &["-t", "work:0.1", "-F", "#{pane_index}:#P:#{window_index}"],
                    ),
                )
                .unwrap()
                .output,
            "1:1:0\n2:2:0"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        &["-p", "-t", "work:0.2", "#{pane_index}:#P:#{window_index}",],
                    ),
                )
                .unwrap()
                .output,
            "2:2:0"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-o", "-t", &window.to_string(), "pane-base-index", "3"],
                ),
            )
            .expect("window pane base");
        assert_eq!(engine.pane_index(window, first), Some(3));
        let quiet = engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-oq", "-t", &window.to_string(), "pane-base-index", "4"],
                ),
            )
            .expect("quiet duplicate override");
        assert!(quiet.effects.is_empty());
        assert_eq!(engine.pane_index(window, first), Some(3));

        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-u", "-t", &window.to_string(), "pane-base-index"],
                ),
            )
            .expect("restore window inheritance");
        assert_eq!(engine.pane_index(window, first), Some(1));
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-gu", "pane-base-index"]),
            )
            .expect("restore global pane base");
        assert_eq!(engine.pane_index(window, first), Some(0));
        assert_eq!(engine.pane_index(window, second), Some(1));
    }

    #[test]
    fn format_geometry_reads_tree_allocations_and_tracks_measurements() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &["-F", "#{pane_id}:#{pane_width}x#{pane_height}"],
                    ),
                )
                .unwrap()
                .output,
            format!("{first}:40x24\n{second}:39x24")
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-windows", &["-F", "#{window_width}x#{window_height}"],),
                )
                .unwrap()
                .output,
            "80x24"
        );

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string()]),
            )
            .unwrap();
        engine.set_pane_geometry(first, 80, 24);
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &["-F", "#{pane_id}:#{pane_width}x#{pane_height}"],
                    ),
                )
                .unwrap()
                .output,
            format!("{first}:80x24\n{second}:79x24")
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-windows", &["-F", "#{window_width}x#{window_height}"],),
                )
                .unwrap()
                .output,
            "160x24"
        );
    }

    #[test]
    fn format_geometry_reports_headless_defaults() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &[
                            "-F",
                            "#{pane_width}:#{pane_height}:#{window_width}:#{window_height}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            "80:24:80:24"
        );
    }

    #[test]
    fn format_activity_tracks_each_rows_window_and_pane() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first_window = context.window.unwrap();
        let first_pane = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second_pane = context.pane.unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .unwrap();
        let second_window = context.window.unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &[
                            "-t",
                            &first_window.to_string(),
                            "-F",
                            "#{pane_id}:#{pane_active}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            format!("{first_pane}:0\n{second_pane}:1")
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &[
                            "-t",
                            &first_window.to_string(),
                            "-F",
                            "#{pane_id}#{?pane_active,(active),}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            format!("{first_pane}\n{second_pane}(active)")
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-windows", &["-F", "#{window_id}:#{window_active}"],),
                )
                .unwrap()
                .output,
            format!("{first_window}:0\n{second_window}:1")
        );
    }

    #[test]
    fn session_and_window_rows_backfill_the_active_pane_like_tmux() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first_window = context.window.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second_pane = context.pane.unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .unwrap();
        let second_window = context.window.unwrap();
        let third_pane = context.pane.unwrap();
        engine.set_pane_geometry(third_pane, 80, 24);

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-sessions",
                        &[
                            "-F",
                            "#{window_index}:#{pane_id}:#{pane_width}:#{pane_active}:#{window_active}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            format!("1:{third_pane}:80:1:1")
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-windows",
                        &["-F", "#{window_id}:#{pane_id}:#{pane_active}"],
                    ),
                )
                .unwrap()
                .output,
            format!("{first_window}:{second_pane}:1\n{second_window}:{third_pane}:1")
        );
    }

    #[test]
    fn window_extent_survives_a_zoomed_measurement() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string()]),
            )
            .unwrap();
        engine.set_pane_geometry(first, 80, 24);
        let extent = |engine: &mut MuxEngine, context: &mut ExecutionContext| {
            engine
                .execute(
                    context,
                    &command("list-windows", &["-F", "#{window_width}x#{window_height}"]),
                )
                .unwrap()
                .output
        };

        assert_eq!(extent(&mut engine, &mut context), "160x24");
        engine
            .execute(
                &mut context,
                &command("resize-pane", &["-Z", "-t", &first.to_string()]),
            )
            .unwrap();
        engine.set_pane_geometry(first, 160, 24);
        assert_eq!(extent(&mut engine, &mut context), "160x24");
        engine
            .execute(
                &mut context,
                &command("resize-pane", &["-Z", "-t", &first.to_string()]),
            )
            .unwrap();
        engine.set_pane_geometry(first, 80, 24);
        assert_eq!(extent(&mut engine, &mut context), "160x24");
    }

    #[test]
    fn alternating_three_pane_measurements_reach_a_fixed_extent() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let third = context.pane.unwrap();
        let window = context.window.unwrap();

        for (pane, columns) in [(first, 80), (second, 38), (third, 38)] {
            engine.set_pane_geometry(pane, columns, 24);
        }
        let stable = engine.state.windows[&window].layout.extent();
        assert_eq!(stable, (160, 24));

        for _ in 0..8 {
            for (pane, columns) in [(first, 80), (second, 38), (third, 38)] {
                engine.set_pane_geometry(pane, columns, 24);
            }
            assert_eq!(engine.state.windows[&window].layout.extent(), stable);
        }
    }

    #[test]
    fn hidden_pane_measurement_is_ignored_while_zoomed() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let hidden = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let zoomed = context.pane.unwrap();
        let window = context.window.unwrap();
        engine
            .execute(
                &mut context,
                &command("resize-pane", &["-Z", "-t", &zoomed.to_string()]),
            )
            .unwrap();

        engine.set_pane_geometry(hidden, 200, 50);

        assert_eq!(engine.state.windows[&window].layout.extent(), (80, 24));
    }

    #[test]
    fn zoomed_pane_measurement_sets_the_window_extent_directly() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let zoomed = context.pane.unwrap();
        let window = context.window.unwrap();
        engine
            .execute(
                &mut context,
                &command("resize-pane", &["-Z", "-t", &zoomed.to_string()]),
            )
            .unwrap();

        engine.set_pane_geometry(zoomed, 200, 50);

        assert_eq!(engine.state.windows[&window].layout.extent(), (200, 50));
    }

    #[test]
    fn extent_probe_memo_accepts_a_different_measurement() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();
        let window = context.window.unwrap();

        engine.set_pane_geometry(pane, 160, 48);
        assert_eq!(engine.state.windows[&window].layout.extent(), (160, 48));
        assert_eq!(
            engine.state.windows[&window].last_extent_probe,
            Some((pane, 160, 48))
        );
        engine.set_pane_geometry(pane, 160, 48);
        assert_eq!(engine.state.windows[&window].layout.extent(), (160, 48));

        engine.set_pane_geometry(pane, 200, 50);
        assert_eq!(engine.state.windows[&window].layout.extent(), (200, 50));
        assert_eq!(
            engine.state.windows[&window].last_extent_probe,
            Some((pane, 200, 50))
        );
    }

    #[test]
    fn zoomed_pane_reports_the_full_window_extent_like_tmux() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        let sizes = |engine: &mut MuxEngine, context: &mut ExecutionContext| {
            engine
                .execute(
                    context,
                    &command("list-panes", &["-F", "#{pane_width}x#{pane_height}"]),
                )
                .unwrap()
                .output
        };

        assert_eq!(sizes(&mut engine, &mut context), "40x24\n39x24");
        engine
            .execute(
                &mut context,
                &command("resize-pane", &["-Z", "-t", &second.to_string()]),
            )
            .unwrap();
        assert_eq!(sizes(&mut engine, &mut context), "40x24\n80x24");
        assert_eq!(engine.pane_geometry(second), Some((80, 24)));
        engine
            .execute(
                &mut context,
                &command("resize-pane", &["-Z", "-t", &second.to_string()]),
            )
            .unwrap();
        assert_eq!(sizes(&mut engine, &mut context), "40x24\n39x24");
    }

    #[test]
    fn window_activity_conditionals_and_display_message_read_the_new_variables() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first_window = context.window.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second_pane = context.pane.unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .unwrap();
        let second_window = context.window.unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-windows",
                        &[
                            "-F",
                            "#{window_id}#{?window_active,*,}#{?#{!:#{window_active}},-,}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            format!("{first_window}-\n{second_window}*")
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        &[
                            "-p",
                            "-t",
                            &second_pane.to_string(),
                            "#{pane_width}x#{pane_height}|#{pane_active}|#{window_active}"
                        ],
                    ),
                )
                .unwrap()
                .output,
            "39x24|1|0"
        );
        engine
            .execute(&mut context, &command("new-session", &["-s", "other"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        &["-p", "-t", &second_pane.to_string(), "#{session_active}"],
                    ),
                )
                .unwrap()
                .output,
            ""
        );
    }

    #[test]
    fn has_session_uses_existing_target_errors_and_aliases() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        assert_eq!(
            engine
                .execute(&mut context, &command("has", &["-t", "work"]))
                .unwrap(),
            Execution::default()
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("has-session", &["-t", "missing"]),
            ),
            Err(ServerError::SessionNotFound(target)) if target == "missing"
        ));
    }

    #[test]
    fn display_message_prints_or_targets_the_requesting_client() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("rename-window", &["editor"]))
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();

        assert_eq!(
            engine
                .execute(&mut context, &command("display-message", &["-p"]))
                .unwrap()
                .output,
            "[work] 0:editor, current pane 1"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        &["-p", "#{pane_format}|#{window_format}|#{session_format}"],
                    ),
                )
                .unwrap()
                .output,
            "1|0|0"
        );
        let displayed = engine
            .execute(
                &mut context,
                &command(
                    "display",
                    &["-t", &first.to_string(), "hello", "#{pane_id}", "#P"],
                ),
            )
            .unwrap();
        assert_eq!(
            displayed.effects,
            [MuxEffect::DisplayMessage {
                pane: Some(first),
                text: format!("hello {first} 0"),
                duration_ms: 750,
            }]
        );

        let mut empty = MuxEngine::default();
        assert_eq!(
            empty
                .execute(
                    &mut ExecutionContext::default(),
                    &command("display-message", &[]),
                )
                .unwrap()
                .effects,
            [MuxEffect::DisplayMessage {
                pane: None,
                text: "[] :, current pane ".to_owned(),
                duration_ms: 750,
            }]
        );
    }

    #[test]
    fn daemon_format_facts_feed_runtime_values_and_session_time() {
        let mut engine = MuxEngine::default();
        engine.set_format_server_context("tower.local", "tower", "/tmp/zz.sock", 40);
        engine.set_format_server_identity(41, "501", "fab");
        engine.set_format_now(55);
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();
        let facts = PaneRuntimeFacts {
            current_command: "fish".to_owned(),
            current_path: "/work/live".to_owned(),
            reported_path: "/work/reported".to_owned(),
            start_path: "/work/start".to_owned(),
            pid: Some(4242),
            tty: "/dev/ttys007".to_owned(),
        };
        assert!(engine.set_pane_runtime_facts(pane, facts.clone()));
        assert!(!engine.set_pane_runtime_facts(pane, facts));
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        &[
                            "-p",
                            "#{pane_current_command}|#{pane_current_path}|#{pane_path}|#{pane_start_path}|#{pane_pid}|#{pane_tty}|#{session_created}|#{cursor_flag}|#{wrap_flag}|#{pid}|#{uid}|#{user}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            "fish|/work/live|/work/reported|/work/start|4242|/dev/ttys007|55|1|1|41|501|fab"
        );
    }

    #[test]
    fn terminal_splits_expand_cwd_and_keep_the_target_pane_as_donor() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("session");
        let first = context.pane.expect("first pane");

        let plain = engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .expect("plain split");
        assert!(matches!(
            plain.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: None,
                ..
            }) if *source == first
        ));
        let second = context.pane.expect("second pane");

        let parsed = crate::parse_config(
            "test.conf",
            "bind | split-window -v -c \"#{pane_current_path}\"",
        );
        assert!(parsed.diagnostics.is_empty());
        engine
            .execute(&mut context, &parsed.commands[0])
            .expect("tmux-style binding");
        let bound_split = engine
            .keys
            .get("prefix", "|")
            .expect("split binding")
            .commands[0]
            .clone();
        let tmux_style = engine
            .execute(&mut context, &bound_split)
            .expect("tmux-style split");
        assert!(matches!(
            tmux_style.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: None,
                ..
            }) if *source == second
        ));
        let third = context.pane.expect("third pane");

        let literal = engine
            .execute(&mut context, &command("split-window", &["-c", "/tmp"]))
            .expect("literal cwd split");
        assert!(matches!(
            literal.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: Some(cwd),
                ..
            }) if *source == third && cwd == "/tmp"
        ));
        let literal_pane = context.pane.expect("literal cwd pane");

        let empty = engine
            .execute(
                &mut context,
                &command("split-window", &["-c", "#{not_a_tmux_variable}"]),
            )
            .expect("empty cwd format split");
        assert!(matches!(
            empty.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: None,
                ..
            }) if *source == literal_pane
        ));
    }

    #[test]
    fn pane_commands_descend_bare_session_and_window_targets() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "w"]))
            .expect("target session");
        let session = context.session.expect("target session id");
        let window = context.window.expect("target window");
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .expect("initial split");
        let active = context.pane.expect("active target pane");
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "elsewhere"]),
            )
            .expect("current session");

        let session_target = engine
            .execute(
                &mut context,
                &command("split-window", &["-d", "-h", "-t", "w"]),
            )
            .expect("bare session target");
        assert!(matches!(
            session_target.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                ..
            }) if *source == active
        ));

        let window_target = engine
            .execute(
                &mut context,
                &command("split-window", &["-d", "-v", "-t", "w:0"]),
            )
            .expect("bare window target");
        assert!(matches!(
            window_target.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                ..
            }) if *source == active
        ));
        assert_eq!(engine.state.windows[&window].active_pane, active);

        engine
            .execute(&mut context, &command("break-pane", &["-d", "-s", "w:0"]))
            .expect("bare window source");
        let moved_window = engine.state.window_for_pane(active).expect("moved pane");
        assert_ne!(moved_window, window);
        assert_eq!(engine.state.windows[&moved_window].session, session);
    }

    #[test]
    fn commands_survive_their_context_pane_dying_on_another_connection() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("session");
        let first = context.pane.expect("first pane");
        engine
            .execute(&mut context, &command("split-window", &["-v"]))
            .expect("split");
        let second = context.pane.expect("second pane");
        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string()]),
            )
            .expect("select first");
        assert_eq!(context.pane, Some(first));

        let mut watcher = ExecutionContext::default();
        engine
            .execute(
                &mut watcher,
                &command("kill-pane", &["-t", &first.to_string()]),
            )
            .expect("external kill");

        let split = engine
            .execute(&mut context, &command("split-window", &["-v"]))
            .expect("split resolves the live active pane, not the corpse");
        assert!(matches!(
            split.effects.first(),
            Some(MuxEffect::PaneCreated { .. })
        ));
        let third = context.pane.expect("third pane");
        assert_ne!(third, first);

        engine
            .execute(&mut context, &command("select-pane", &["-U"]))
            .expect("directional select from a healed context");
        assert_eq!(context.pane, Some(second));
    }

    #[test]
    fn new_window_cwd_origin_is_the_target_session_not_the_context_pane() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "target"]))
            .expect("target session");
        let target_pane = context.pane.expect("target pane");
        assert!(engine.set_pane_runtime_facts(
            target_pane,
            PaneRuntimeFacts {
                current_path: "/private/tmp".to_owned(),
                ..PaneRuntimeFacts::default()
            },
        ));
        engine
            .execute(&mut context, &command("new-session", &["-s", "other"]))
            .expect("other session");
        let other_pane = context.pane.expect("other pane");
        assert_ne!(other_pane, target_pane);

        let window = engine
            .execute(
                &mut context,
                &command(
                    "new-window",
                    &["-d", "-t", "target", "-c", "#{pane_current_path}"],
                ),
            )
            .expect("cross-session new-window");
        assert!(matches!(
            window.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: Some(cwd),
                ..
            }) if *source == target_pane && cwd == "/private/tmp"
        ));
    }

    #[test]
    fn new_windows_and_sessions_expand_cwd_and_keep_the_origin_pane_as_donor() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("session");
        let first = context.pane.expect("first pane");
        assert!(engine.set_pane_runtime_facts(
            first,
            PaneRuntimeFacts {
                current_path: "/private/tmp".to_owned(),
                reported_path: "/tmp".to_owned(),
                ..PaneRuntimeFacts::default()
            },
        ));
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        &["-p", "#{pane_current_path}|#{pane_path}"],
                    ),
                )
                .expect("source paths")
                .output,
            "/private/tmp|/tmp"
        );

        let mut outside = ExecutionContext::default();
        let formatted = engine
            .execute(
                &mut outside,
                &command(
                    "new-window",
                    &["-d", "-t", "work", "-c", "#{pane_current_path}"],
                ),
            )
            .expect("formatted cwd window from outside a pane");
        assert!(matches!(
            formatted.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: Some(cwd),
                ..
            }) if *source == first && cwd == "/private/tmp"
        ));

        let window = engine
            .execute(&mut context, &command("new-window", &["-c", "/tmp"]))
            .expect("literal cwd window");
        assert!(matches!(
            window.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: Some(cwd),
                ..
            }) if *source == first && cwd == "/tmp"
        ));
        let second = context.pane.expect("second pane");
        assert_ne!(second, first);

        let session = engine
            .execute(
                &mut context,
                &command("new-session", &["-s", "next", "-c", "/tmp"]),
            )
            .expect("literal cwd session");
        assert!(matches!(
            session.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                cwd: Some(cwd),
                ..
            }) if *source == second && cwd == "/tmp"
        ));
    }

    #[test]
    fn new_panes_preserve_their_id_while_materializing_the_selected_kind() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("session");
        let source = context.pane.expect("source pane");

        let created = engine
            .execute(&mut context, &command("split-picker", &["-h"]))
            .expect("picker split");
        let picker = context.pane.expect("picker pane");
        assert_ne!(picker, source);
        assert!(matches!(
            engine.state.pane(picker).map(|pane| &pane.kind),
            Some(PaneKind::Picker {
                inherit_cwd_from: Some(donor)
            }) if *donor == source
        ));
        assert!(matches!(
            created.effects.first(),
            Some(MuxEffect::PaneCreated {
                pane,
                kind: PaneKindSnapshot::Picker,
                inherit_cwd_from: None,
                ..
            }) if *pane == picker
        ));

        let materialized = engine
            .execute(
                &mut context,
                &command("select-pane-kind", &["-t", &picker.to_string(), "terminal"]),
            )
            .expect("terminal selection");
        assert!(matches!(
            engine.state.pane(picker).map(|pane| &pane.kind),
            Some(PaneKind::Terminal)
        ));
        assert!(matches!(
            materialized.effects.first(),
            Some(MuxEffect::PaneMaterialized {
                pane,
                kind: PaneKindSnapshot::Terminal,
                inherit_cwd_from: Some(donor),
                ..
            }) if *pane == picker && *donor == source
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("select-pane-kind", &["-t", &picker.to_string(), "browser"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message.contains("not awaiting")
        ));

        engine
            .execute(&mut context, &command("split-picker", &["-v"]))
            .expect("second picker");
        let browser_picker = context.pane.expect("browser picker");
        engine
            .execute(
                &mut context,
                &command(
                    "select-pane-kind",
                    &["-t", &browser_picker.to_string(), "browser"],
                ),
            )
            .expect("browser selection");
        assert!(matches!(
            engine.state.pane(browser_picker).map(|pane| &pane.kind),
            Some(PaneKind::Browser(browser))
                if browser.url() == "about:blank" && browser.profile == DEFAULT_BROWSER_PROFILE
        ));

        engine
            .execute(&mut context, &command("split-picker", &["-h"]))
            .expect("third picker");
        let agent_picker = context.pane.expect("agent picker");
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "experimental-agent-pane", "on"]),
            )
            .expect("enable agent panes");
        let configured_cwd = absolute_test_path("configured agent project");
        let configured_cwd_arg = configured_cwd.to_string_lossy().into_owned();
        let materialized = engine
            .execute(
                &mut context,
                &command(
                    "select-pane-kind",
                    &[
                        "-t",
                        &agent_picker.to_string(),
                        "-c",
                        &configured_cwd_arg,
                        "agent",
                    ],
                ),
            )
            .expect("agent selection");
        assert!(matches!(
            engine.state.pane(agent_picker).map(|pane| &pane.kind),
            Some(PaneKind::Agent(descriptor)) if descriptor.cwd.as_ref() == Some(&configured_cwd)
        ));
        assert!(matches!(
            materialized.effects.first(),
            Some(MuxEffect::PaneMaterialized {
                pane,
                kind: PaneKindSnapshot::Agent(descriptor),
                inherit_cwd_from: Some(donor),
                ..
            }) if *pane == agent_picker
                && *donor == picker
                && descriptor.cwd.as_ref() == Some(&configured_cwd)
        ));
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn agent_session_metadata_persists_restore_identity_and_working_directory() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "agent"]))
            .expect("session");
        let terminal = context.pane.expect("terminal pane");
        engine
            .execute(&mut context, &command("split-picker", &["-h"]))
            .expect("picker split");
        let agent = context.pane.expect("agent pane");
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "experimental-agent-pane", "on"]),
            )
            .expect("enable agent panes");
        engine
            .execute(
                &mut context,
                &command("select-pane-kind", &["-t", &agent.to_string(), "agent"]),
            )
            .expect("agent selection");

        let cwd = absolute_test_path("agent project");
        let cwd_arg = cwd.to_string_lossy().into_owned();
        let persisted = engine
            .execute(
                &mut context,
                &command(
                    "set-agent-session",
                    &[
                        "-t",
                        &agent.to_string(),
                        "-c",
                        &cwd_arg,
                        "session-with-opaque-format",
                    ],
                ),
            )
            .expect("persist agent session metadata");
        assert_eq!(persisted.effects, [MuxEffect::SnapshotChanged]);
        assert!(matches!(
            &engine.state.pane(agent).expect("agent state").kind,
            PaneKind::Agent(descriptor)
                if descriptor.session_id.as_deref() == Some("session-with-opaque-format")
                    && descriptor.cwd.as_ref() == Some(&cwd)
        ));

        let switched = engine
            .execute(
                &mut context,
                &command(
                    "set-agent-provider",
                    &["-t", &agent.to_string(), "claude-code"],
                ),
            )
            .expect("switch agent provider");
        assert_eq!(
            switched.effects,
            [
                MuxEffect::AgentPaneRestart { pane: agent },
                MuxEffect::SnapshotChanged,
            ]
        );
        assert!(matches!(
            &engine.state.pane(agent).expect("agent state").kind,
            PaneKind::Agent(descriptor)
                if descriptor.provider == AgentProvider::ClaudeCode
                    && descriptor.session_id.is_none()
                    && descriptor.cwd.as_ref() == Some(&cwd)
        ));

        let restarted = engine
            .execute(
                &mut context,
                &command("restart-agent-pane", &["-t", &agent.to_string()]),
            )
            .expect("restart agent pane");
        assert_eq!(
            restarted.effects,
            [MuxEffect::AgentPaneRestart { pane: agent }]
        );

        assert!(matches!(
            engine.execute(
                &mut context,
                &command(
                    "set-agent-session",
                    &["-t", &agent.to_string(), "-c", "relative", "replacement"],
                ),
            ),
            Err(ServerError::InvalidCommand(message))
                if message.contains("working directory must be absolute")
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-agent-session", &["-t", &terminal.to_string(), "session"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message.contains("not an agent")
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-agent-session", &["-t", &agent.to_string(), "bad\nsession"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message.contains("non-control")
        ));
    }

    #[test]
    fn editor_materialization_and_restore_path_preserve_the_picker_identity() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "editor"]))
            .expect("session");
        let terminal = context.pane.expect("terminal pane");
        engine
            .execute(&mut context, &command("split-picker", &["-h"]))
            .expect("picker split");
        let editor = context.pane.expect("editor pane");
        let window = context.window.expect("window");
        let layout = engine.state.windows[&window].layout.clone();

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "experimental-editor-pane", "on"]),
            )
            .expect("enable editor panes");
        let materialized = engine
            .execute(
                &mut context,
                &command("select-pane-kind", &["-t", &editor.to_string(), "editor"]),
            )
            .expect("editor selection");
        assert_eq!(context.pane, Some(editor));
        assert_eq!(engine.state.windows[&window].layout, layout);
        assert!(matches!(
            materialized.effects.first(),
            Some(MuxEffect::PaneMaterialized {
                pane,
                kind: PaneKindSnapshot::Editor(descriptor),
                inherit_cwd_from: Some(donor),
                ..
            }) if *pane == editor && *donor == terminal && descriptor.validate().is_ok()
        ));

        let path = absolute_test_path("editor pane.rs")
            .to_string_lossy()
            .into_owned();
        let persisted = engine
            .execute(
                &mut context,
                &command("set-editor-path", &["-t", &editor.to_string(), &path]),
            )
            .expect("persist editor path");
        assert_eq!(persisted.effects, [MuxEffect::SnapshotChanged]);
        assert!(matches!(
            &engine.state.snapshot().sessions[0].windows[0].panes[&editor].kind,
            PaneKindSnapshot::Editor(descriptor)
                if descriptor.path.as_deref() == Some(path.as_str())
                    && descriptor.validate().is_ok()
        ));

        for invalid in [
            "relative.rs".to_owned(),
            absolute_test_path("bad\nname.rs")
                .to_string_lossy()
                .into_owned(),
            absolute_test_path(&"x".repeat(16 * 1024))
                .to_string_lossy()
                .into_owned(),
        ] {
            assert!(matches!(
                engine.execute(
                    &mut context,
                    &command("set-editor-path", &["-t", &editor.to_string(), &invalid],),
                ),
                Err(ServerError::InvalidCommand(_))
            ));
        }

        engine
            .execute(
                &mut context,
                &command("set-editor-path", &["-t", &editor.to_string()]),
            )
            .expect("clear editor path");
        assert!(matches!(
            &engine.state.pane(editor).expect("editor state").kind,
            PaneKind::Editor(descriptor) if descriptor.path.is_none()
        ));
    }

    #[test]
    fn experimental_pane_kinds_are_rejected_until_their_flag_is_enabled() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "gated"]))
            .expect("session");
        engine
            .execute(&mut context, &command("split-picker", &["-h"]))
            .expect("picker split");
        let picker = context.pane.expect("picker pane");

        for (kind, flag, option) in [
            (
                "agent",
                "experimental-agent-pane",
                MuxOptionKey::ExperimentalAgentPane,
            ),
            (
                "editor",
                "experimental-editor-pane",
                MuxOptionKey::ExperimentalEditorPane,
            ),
        ] {
            assert_eq!(engine.mux_option_value(option), "off");
            assert!(matches!(
                engine.execute(
                    &mut context,
                    &command("select-pane-kind", &["-t", &picker.to_string(), kind]),
                ),
                Err(ServerError::InvalidCommand(message)) if message.contains(flag)
            ));
            assert!(matches!(
                engine.state.pane(picker).map(|pane| &pane.kind),
                Some(PaneKind::Picker { .. })
            ));
        }

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "experimental-editor-pane", "true"]),
            )
            .expect("enable editor panes");
        assert_eq!(
            engine.mux_option_value(MuxOptionKey::ExperimentalEditorPane),
            "on"
        );
        engine
            .execute(
                &mut context,
                &command("select-pane-kind", &["-t", &picker.to_string(), "editor"]),
            )
            .expect("editor materializes once enabled");
        assert!(matches!(
            engine.state.pane(picker).map(|pane| &pane.kind),
            Some(PaneKind::Editor(_))
        ));
    }

    #[test]
    fn rename_commands_preserve_ids_and_support_aliases_and_targets() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "first"]))
            .expect("first session");
        let first_session = context.session.expect("first session id");
        let first_window = context.window.expect("first window id");

        let renamed = engine
            .execute(&mut context, &command("rename", &["primary"]))
            .expect("rename current session");
        assert_eq!(renamed.effects, [MuxEffect::SnapshotChanged]);
        assert_eq!(engine.state.sessions[&first_session].name, "primary");
        assert_eq!(context.session, Some(first_session));

        let generation = engine.state.generation();
        let unchanged = engine
            .execute(&mut context, &command("rename-session", &["primary"]))
            .expect("same session name is a no-op");
        assert!(unchanged.effects.is_empty());
        assert_eq!(engine.state.generation(), generation);

        engine
            .execute(&mut context, &command("new-session", &["-s", "second"]))
            .expect("second session");
        let second_session = context.session.expect("second session id");
        engine
            .execute(
                &mut context,
                &command(
                    "rename-session",
                    &["-t", &first_session.to_string(), "work"],
                ),
            )
            .expect("rename targeted session");
        assert_eq!(engine.state.sessions[&first_session].name, "work");
        assert_eq!(context.session, Some(second_session));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("rename-session", &["-t", "second", "work"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "duplicate session: work"
        ));

        engine
            .execute(&mut context, &command("renamew", &["editor"]))
            .expect("rename current window");
        let second_window = context.window.expect("second window id");
        assert_eq!(engine.state.windows[&second_window].name, "editor");
        engine
            .execute(
                &mut context,
                &command(
                    "rename-window",
                    &["-t", &first_window.to_string(), "editor"],
                ),
            )
            .expect("duplicate window names are allowed");
        assert_eq!(engine.state.windows[&first_window].name, "editor");
        assert_eq!(context.window, Some(second_window));

        engine
            .execute(&mut context, &command("rename-window", &["--", "-logs"]))
            .expect("double dash permits a leading hyphen");
        assert_eq!(engine.state.windows[&second_window].name, "-logs");
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn rename_commands_require_one_name_and_reject_unknown_flags() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &[]))
            .expect("session");

        for invalid in [
            command("rename-session", &[]),
            command("rename-session", &["one", "two"]),
            command("rename-window", &[]),
            command("rename-window", &["-q", "name"]),
        ] {
            assert!(matches!(
                engine.execute(&mut context, &invalid),
                Err(ServerError::InvalidCommand(_))
            ));
        }
    }

    #[test]
    fn select_pane_title_updates_metadata_without_changing_the_active_pane() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &[]))
            .expect("session");
        let first = context.pane.expect("first pane");
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .expect("second pane");
        let second = context.pane.expect("second pane");

        let renamed = engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string(), "-T", "~/src/zz"]),
            )
            .expect("set pane title");
        assert_eq!(renamed.effects, [MuxEffect::SnapshotChanged]);
        assert_eq!(engine.state.pane(first).unwrap().title, "~/src/zz");
        assert_eq!(context.pane, Some(second));
        assert_eq!(
            engine.state.windows[&context.window.unwrap()].active_pane,
            second
        );

        let unchanged = engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string(), "-T", "~/src/zz"]),
            )
            .expect("same title is a no-op");
        assert!(unchanged.effects.is_empty());
        assert_eq!(context.pane, Some(second));
    }

    #[test]
    fn automatic_session_names_use_the_first_canonical_numeric_gap() {
        let mut state = MuxState::default();
        for name in [
            "0",
            "1",
            "3",
            "",
            "00",
            "01",
            "+2",
            " 2",
            "184467440737095516160",
        ] {
            state.create_session(name).expect("unique session name");
        }

        assert_eq!(next_session_name(&state), "2");
        state.create_session("2").expect("numeric gap is unused");
        assert_eq!(next_session_name(&state), "4");
    }

    #[test]
    fn reload_config_is_a_native_argument_free_effect() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        let execution = engine
            .execute(&mut context, &command("reload-config", &[]))
            .expect("reload effect");
        assert_eq!(execution.effects, [MuxEffect::ReloadConfig]);
        assert!(matches!(
            engine.execute(&mut context, &command("reload-config", &["unexpected"])),
            Err(ServerError::InvalidCommand(_))
        ));
    }

    #[test]
    fn browser_commands_and_send_keys_produce_server_effects() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &[]))
            .unwrap();
        let execution = engine
            .execute(
                &mut context,
                &command("split-browser", &["-h", "https://zed.dev"]),
            )
            .unwrap();
        assert!(matches!(
            execution.effects[0],
            MuxEffect::PaneCreated {
                kind: PaneKindSnapshot::Browser(_),
                ..
            }
        ));
        let terminal = engine.state.default_context().unwrap().2;
        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &["-t", &terminal.to_string(), "cargo test", "Enter"],
                ),
            )
            .unwrap();
        assert!(matches!(
            &execution.effects[0],
            MuxEffect::SendKeys { keys, .. }
                if keys == &vec![
                    KeyToken::Literal("cargo test".to_owned()),
                    KeyToken::Named("Enter".to_owned())
                ]
        ));

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &["-t", &terminal.to_string(), "-X", "-o", "previous-prompt"],
                ),
            )
            .unwrap();
        assert!(matches!(
            execution.effects.as_slice(),
            [MuxEffect::TerminalView {
                action: TerminalViewAction::CopyMode(CopyModeAction::PreviousPrompt {
                    output: true
                }),
                ..
            }]
        ));

        let execution = engine
            .execute(
                &mut context,
                &command("clearhist", &["-t", &terminal.to_string()]),
            )
            .unwrap();
        assert!(matches!(
            execution.effects.as_slice(),
            [MuxEffect::TerminalView {
                action: TerminalViewAction::ClearHistory,
                ..
            }]
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("clear-history", &["-H", "-t", &terminal.to_string()]),
            ),
            Err(ServerError::UnsupportedCommand(_))
        ));

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &["-t", &terminal.to_string(), "-X", "jump-to-forward", "é"],
                ),
            )
            .unwrap();
        assert!(matches!(
            execution.effects.as_slice(),
            [MuxEffect::TerminalView {
                action: TerminalViewAction::CopyMode(CopyModeAction::Jump(CopyJump {
                    target,
                    direction: CopyJumpDirection::Forward,
                    to: true,
                })),
                ..
            }] if target == "é"
        ));
    }

    #[test]
    fn browser_profile_commands_create_and_switch_named_profiles() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &[]))
            .expect("session");
        engine
            .execute(
                &mut context,
                &command(
                    "split-browser",
                    &["-h", "-p", "personal", "https://zed.dev"],
                ),
            )
            .expect("profiled browser");

        let browser = context.pane.expect("browser pane");
        assert!(matches!(
            &engine.state.pane(browser).expect("browser state").kind,
            PaneKind::Browser(descriptor) if descriptor.profile == "personal"
        ));
        let switched = engine
            .execute(
                &mut context,
                &command(
                    "set-browser-profile",
                    &["-t", &browser.to_string(), " Work "],
                ),
            )
            .expect("switch browser profile");
        assert_eq!(switched.effects, [MuxEffect::SnapshotChanged]);
        assert!(matches!(
            &engine.state.pane(browser).expect("browser state").kind,
            PaneKind::Browser(descriptor) if descriptor.profile == "Work"
        ));
    }

    #[test]
    fn copy_mode_search_commands_produce_native_terminal_effects() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &[]))
            .unwrap();
        let terminal = context.pane.expect("new session terminal");

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "copy-mode-search-prompt",
                    &["-b", "-t", &terminal.to_string()],
                ),
            )
            .unwrap();
        assert_eq!(
            execution.effects,
            vec![MuxEffect::TerminalUi {
                pane: terminal,
                command: TerminalUiCommand::BeginSearch {
                    direction: SearchDirection::Backward,
                },
            }]
        );

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &["-t", &terminal.to_string(), "-X", "search-again"],
                ),
            )
            .unwrap();
        assert_eq!(
            execution.effects,
            vec![MuxEffect::TerminalView {
                pane: terminal,
                action: TerminalViewAction::CopyMode(CopyModeAction::SearchAgain {
                    reverse: false,
                }),
            }]
        );
    }

    #[test]
    fn vi_copy_mode_actions_parse_to_native_terminal_effects() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &[]))
            .expect("session");
        let terminal = context.pane.expect("terminal");

        for (name, expected) in [
            ("next-space", CopyModeAction::NextSpace),
            ("previous-space", CopyModeAction::PreviousSpace),
            ("next-space-end", CopyModeAction::NextSpaceEnd),
            ("scroll-up", CopyModeAction::ScrollUp),
            ("scroll-down", CopyModeAction::ScrollDown),
            ("scroll-middle", CopyModeAction::ScrollMiddle),
            ("next-matching-bracket", CopyModeAction::NextMatchingBracket),
        ] {
            let execution = engine
                .execute(
                    &mut context,
                    &command("send-keys", &["-t", &terminal.to_string(), "-X", name]),
                )
                .unwrap_or_else(|error| panic!("parse {name}: {error}"));
            assert!(matches!(
                execution.effects.as_slice(),
                [MuxEffect::TerminalView {
                    action: TerminalViewAction::CopyMode(action),
                    ..
                }] if action == &expected
            ));
        }

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &[
                        "-t",
                        &terminal.to_string(),
                        "-X",
                        "copy-pipe-end-of-line-and-cancel",
                    ],
                ),
            )
            .expect("copy to end of line");
        assert!(matches!(
            execution.effects.as_slice(),
            [MuxEffect::TerminalView {
                action: TerminalViewAction::CopyMode(CopyModeAction::CopyEndOfLine(copy)),
                ..
            }] if copy.cancel && copy.clear_selection
        ));

        for (name, direction) in [
            ("search-forward-cursor-word", SearchDirection::Forward),
            ("search-backward-cursor-word", SearchDirection::Backward),
        ] {
            let execution = engine
                .execute(
                    &mut context,
                    &command("send-keys", &["-t", &terminal.to_string(), "-X", name]),
                )
                .expect("cursor word search");
            assert!(matches!(
                execution.effects.as_slice(),
                [MuxEffect::TerminalView {
                    action: TerminalViewAction::CopyMode(CopyModeAction::SearchCursorWord {
                        direction: actual,
                    }),
                    ..
                }] if *actual == direction
            ));
        }

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &["-t", &terminal.to_string(), "-X", "goto-line", "42"],
                ),
            )
            .expect("goto line");
        assert!(matches!(
            execution.effects.as_slice(),
            [MuxEffect::TerminalView {
                action: TerminalViewAction::CopyMode(CopyModeAction::GotoLine(42)),
                ..
            }]
        ));
    }

    #[test]
    fn copy_mode_copy_variants_keep_destinations_and_lifecycle_independent() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &[]))
            .unwrap();
        let terminal = context.pane.expect("new session terminal");

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "set-clipboard", "off"]),
            )
            .unwrap();
        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &[
                        "-t",
                        &terminal.to_string(),
                        "-X",
                        "copy-selection-no-clear",
                        "named",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(
            execution.effects,
            vec![MuxEffect::TerminalView {
                pane: terminal,
                action: TerminalViewAction::CopyMode(CopyModeAction::copy_selection(
                    CopyModeCopy {
                        request_id: 0,
                        clipboard: false,
                        buffer: Some(PasteBufferAction::Create {
                            prefix: Some("named".to_owned()),
                        }),
                        pipe: None,
                        clear_selection: false,
                        cancel: false,
                    },
                )),
            }]
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "set-clipboard", "external"]),
            )
            .unwrap();
        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &[
                        "-t",
                        &terminal.to_string(),
                        "-X",
                        "-P",
                        "copy-selection-and-cancel",
                    ],
                ),
            )
            .unwrap();
        let [
            MuxEffect::TerminalView {
                action: TerminalViewAction::CopyMode(CopyModeAction::CopySelection(copy)),
                ..
            },
        ] = execution.effects.as_slice()
        else {
            panic!("expected one copy-mode effect");
        };
        assert!(copy.clipboard);
        assert_eq!(copy.buffer, None);
        assert_eq!(copy.pipe, None);
        assert!(copy.clear_selection);
        assert!(copy.cancel);

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &["-t", &terminal.to_string(), "-X", "append-selection"],
                ),
            )
            .unwrap();
        let [
            MuxEffect::TerminalView {
                action: TerminalViewAction::CopyMode(CopyModeAction::CopySelection(copy)),
                ..
            },
        ] = execution.effects.as_slice()
        else {
            panic!("expected one copy-mode effect");
        };
        assert!(!copy.clipboard);
        assert_eq!(copy.buffer, Some(PasteBufferAction::Append));
        assert_eq!(copy.pipe, None);
        assert!(!copy.cancel);

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &[
                        "-t",
                        &terminal.to_string(),
                        "-X",
                        "-C",
                        "copy-pipe-no-clear",
                        "cat >/tmp/zz-copy",
                        "named",
                    ],
                ),
            )
            .unwrap();
        let [
            MuxEffect::TerminalView {
                action: TerminalViewAction::CopyMode(CopyModeAction::CopySelection(copy)),
                ..
            },
        ] = execution.effects.as_slice()
        else {
            panic!("expected one copy-mode effect");
        };
        assert!(!copy.clipboard);
        assert_eq!(
            copy.buffer,
            Some(PasteBufferAction::Create {
                prefix: Some("named".to_owned()),
            })
        );
        assert_eq!(copy.pipe.as_deref(), Some("cat >/tmp/zz-copy"));
        assert!(!copy.clear_selection);
        assert!(!copy.cancel);

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-g", "copy-command", "cat >/tmp/zz-default"],
                ),
            )
            .unwrap();
        let execution = engine
            .execute(
                &mut context,
                &command(
                    "send-keys",
                    &["-t", &terminal.to_string(), "-X", "-P", "pipe-and-cancel"],
                ),
            )
            .unwrap();
        let [
            MuxEffect::TerminalView {
                action: TerminalViewAction::CopyMode(CopyModeAction::CopySelection(copy)),
                ..
            },
        ] = execution.effects.as_slice()
        else {
            panic!("expected one copy-mode effect");
        };
        assert!(!copy.clipboard);
        assert_eq!(copy.buffer, None);
        assert_eq!(copy.pipe.as_deref(), Some("cat >/tmp/zz-default"));
        assert!(copy.clear_selection);
        assert!(copy.cancel);
    }

    #[test]
    fn config_commands_update_prefix_and_bindings() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("set", &["-g", "prefix", "C-a"]))
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("bind", &["-n", "F2", "new-window", "-n", "scratch"]),
            )
            .unwrap();
        assert_eq!(engine.keys.prefix(), "C-a");
        assert_eq!(
            engine.keys.get("root", "F2").unwrap().commands[0].name,
            "new-window"
        );
    }

    #[test]
    fn option_parsing_follows_getopt_word_semantics() {
        let parse = |args: &[&str], value_options: &[&str]| {
            let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
            parse_options(&args, value_options, &[]).expect("parsed options")
        };

        let (options, positional) = parse(&["-As", "main", "htop"], &["-s", "-c", "-n"]);
        assert!(options.has("-A"));
        assert_eq!(options.value("-s"), Some("main"));
        assert_eq!(positional, ["htop"]);

        let (options, positional) = parse(&["-dc", "#{pane_path}"], &["-s", "-c", "-n"]);
        assert!(options.has("-d"));
        assert_eq!(options.value("-c"), Some("#{pane_path}"));
        assert!(positional.is_empty());

        let (options, positional) = parse(&["-n1", "date"], &["-n"]);
        assert_eq!(options.value("-n"), Some("1"));
        assert_eq!(positional, ["date"]);

        let (options, positional) = parse(&["-c", "-foo", "-g"], &["-c"]);
        assert_eq!(options.value("-c"), Some("-foo"));
        assert!(options.has("-g"));
        assert!(positional.is_empty());

        let (options, positional) = parse(&["-g", "--", "-s", "value"], &["-s"]);
        assert!(options.has("-g"));
        assert_eq!(options.value("-s"), None);
        assert_eq!(positional, ["-s", "value"]);

        let (options, positional) = parse(&["ls", "-la", "Enter"], &["-t"]);
        assert!(options.flags.is_empty());
        assert_eq!(positional, ["ls", "-la", "Enter"]);

        let (options, positional) = parse(&["-", "-t", "1"], &["-t"]);
        assert!(options.flags.is_empty());
        assert_eq!(positional, ["-", "-t", "1"]);

        assert!(matches!(
            parse_options(&["-t".to_owned()], &["-t"], &[]),
            Err(ServerError::InvalidCommand(message)) if message == "-t requires an argument"
        ));

        let (options, positional) = parse_options(
            &["-R10".to_owned(), "-U5".to_owned(), "-R".to_owned()],
            &[],
            &["-R", "-U"],
        )
        .expect("parsed options");
        assert_eq!(options.value("-R"), Some("10"));
        assert_eq!(options.value("-U"), Some("5"));
        assert!(options.has("-R"));
        assert!(positional.is_empty());
    }

    #[test]
    fn commands_keep_dash_arguments_after_their_first_positional() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let created = engine
            .execute(
                &mut context,
                &command("new-window", &["echo", "-n", "hello"]),
            )
            .unwrap();
        assert!(matches!(
            created.effects.first(),
            Some(MuxEffect::PaneCreated {
                command: Some(command),
                ..
            }) if command == "echo -n hello"
        ));
        let window = &engine.state.windows[&context.window.unwrap()];
        assert_eq!(window.name, window.index.to_string());

        let sent = engine
            .execute(&mut context, &command("send-keys", &["ls", "-la", "Enter"]))
            .unwrap();
        assert!(matches!(
            sent.effects.first(),
            Some(MuxEffect::SendKeys { keys, .. })
                if keys == &vec![
                    KeyToken::Literal("ls".to_owned()),
                    KeyToken::Literal("-la".to_owned()),
                    KeyToken::Named("Enter".to_owned())
                ]
        ));

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "--", "prefix", "C-z"]),
            )
            .unwrap();
        assert_eq!(engine.keys.prefix(), "C-z");

        engine
            .execute(
                &mut context,
                &command("set", &["-g", "status-left", "-foo"]),
            )
            .unwrap();
        assert_eq!(engine.status.format(StatusOption::Left), Some("-foo"));
    }

    #[test]
    fn bind_key_chains_execute_in_order_and_list_keys_round_trips_them() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let parsed = crate::parse_config(
            "chain.conf",
            r"bind x new-window -n one \; new-window -n two",
        );
        assert!(parsed.diagnostics.is_empty());
        engine
            .execute(&mut context, &parsed.commands[0])
            .expect("bind command chain");

        let commands = engine
            .keys
            .get("prefix", "x")
            .expect("chained binding")
            .commands
            .clone();
        assert_eq!(commands.len(), 2);
        for command in commands {
            engine
                .execute(&mut context, &command)
                .expect("execute bound command");
        }
        let session = context.session.unwrap();
        assert_eq!(engine.state.sessions[&session].windows.len(), 3);

        let listed = engine
            .execute(&mut context, &command("list-keys", &["-T", "prefix"]))
            .unwrap()
            .output;
        assert!(listed.lines().any(|line| {
            line == "bind-key -T prefix x new-window -n one \\; new-window -n two"
        }));

        for args in [
            &["x", ";", "new-window"][..],
            &["x", "new-window", ";", ";", "new-window"][..],
        ] {
            assert!(matches!(
                engine.execute(&mut context, &command("bind-key", args)),
                Err(ServerError::InvalidCommand(message)) if message.contains("empty command")
            ));
        }
        engine
            .execute(
                &mut context,
                &command("bind-key", &["y", "new-window", ";"]),
            )
            .expect("trailing separator");
        assert_eq!(
            engine.keys.get("prefix", "y").expect("binding").commands,
            [CommandInvocation::new("new-window", [] as [&str; 0])]
        );
    }

    #[test]
    fn buffer_limit_is_a_validated_global_server_option() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        let changed = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "buffer-limit", "3"]),
            )
            .expect("set buffer limit");
        assert_eq!(
            changed.effects,
            [
                MuxEffect::BufferLimitChanged(3),
                MuxEffect::MuxOptionChanged {
                    option: MuxOptionKey::BufferLimit,
                },
            ]
        );
        assert_eq!(engine.buffer_limit(), 3);

        let reset = engine
            .execute(
                &mut context,
                &command("set-option", &["-gu", "buffer-limit"]),
            )
            .expect("reset buffer limit");
        assert_eq!(
            reset.effects,
            [
                MuxEffect::BufferLimitChanged(DEFAULT_BUFFER_LIMIT),
                MuxEffect::MuxOptionChanged {
                    option: MuxOptionKey::BufferLimit,
                },
            ]
        );
        assert_eq!(engine.buffer_limit(), DEFAULT_BUFFER_LIMIT);

        assert!(
            engine
                .execute(&mut context, &command("set-option", &["buffer-limit", "0"]),)
                .is_err()
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-p", "buffer-limit", "2"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["buffer-limit", "3"]),
            )
            .unwrap();
        assert_eq!(engine.buffer_limit(), 3);
    }

    #[test]
    fn message_limit_is_a_live_server_option() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        assert_eq!(engine.message_limit(), DEFAULT_MESSAGE_LIMIT);
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-s", "message-limit", "3"]),
                )
                .unwrap(),
            Execution::default()
        );
        assert_eq!(engine.message_limit(), 3);
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-sv", "message-limit"]),
                )
                .unwrap()
                .output,
            "3"
        );

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-su", "message-limit"]),
                )
                .unwrap(),
            Execution::default()
        );
        assert_eq!(engine.message_limit(), DEFAULT_MESSAGE_LIMIT);
    }

    #[test]
    fn history_trickle_is_a_validated_global_server_option() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        let changed = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "history-trickle", "500"]),
            )
            .expect("set history trickle");
        assert_eq!(
            changed.effects,
            [MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::HistoryTrickle,
            }]
        );
        assert_eq!(engine.history_trickle(), 500);
        assert_eq!(engine.mux_option_value(MuxOptionKey::HistoryTrickle), "500");

        let disabled = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "history-trickle", "0"]),
            )
            .expect("disable history trickle");
        assert_eq!(
            disabled.effects,
            [MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::HistoryTrickle,
            }]
        );
        assert_eq!(engine.history_trickle(), 0);

        engine
            .execute(
                &mut context,
                &command("set-option", &["-gu", "history-trickle"]),
            )
            .expect("reset history trickle");
        assert_eq!(engine.history_trickle(), DEFAULT_HISTORY_TRICKLE);

        for command in [
            command(
                "set-option",
                &["history-trickle", &(MAX_HISTORY_TRICKLE + 1).to_string()],
            ),
            command("set-option", &["-p", "history-trickle", "2"]),
            command("set-window-option", &["history-trickle", "2"]),
        ] {
            assert!(engine.execute(&mut context, &command).is_err());
        }
    }

    #[test]
    fn history_limit_inherits_by_session_and_only_configures_new_panes() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "history-limit", "7"]),
            )
            .expect("global history limit");
        engine
            .execute(&mut context, &command("new-session", &["-s", "first"]))
            .expect("first session");
        let first_session = context.session.expect("first session id");
        let first_pane = context.pane.expect("first pane");
        assert_eq!(engine.history_limit_for_pane(first_pane).unwrap(), 7);

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-t", &first_session.to_string(), "history-limit", "3"],
                ),
            )
            .expect("session history limit");
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .expect("session pane");
        let overridden_pane = context.pane.expect("overridden pane");
        assert_eq!(engine.history_limit_for_pane(overridden_pane).unwrap(), 3);

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "history-limit", "9"]),
            )
            .expect("update global limit");
        assert_eq!(engine.history_limit_for_session(first_session), 3);
        engine
            .execute(&mut context, &command("new-session", &["-s", "second"]))
            .expect("second session");
        assert_eq!(
            engine
                .history_limit_for_pane(context.pane.unwrap())
                .unwrap(),
            9
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-u", "-t", &first_session.to_string(), "history-limit"],
                ),
            )
            .expect("restore session inheritance");
        assert_eq!(engine.history_limit_for_session(first_session), 9);

        let invalid = command(
            "set-option",
            &["-g", "history-limit", &(MAX_HISTORY_LIMIT + 1).to_string()],
        );
        assert!(engine.execute(&mut context, &invalid).is_err());
    }

    #[test]
    fn show_options_uses_pin_escaping_and_declared_scope_readback() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        assert_eq!(tmux_args_escape(""), "''");
        assert_eq!(tmux_args_escape("#"), "\\#");
        assert_eq!(tmux_args_escape("a b"), "\"a b\"");
        assert_eq!(tmux_args_escape("a\"b"), "'a\"b'");
        assert_eq!(tmux_args_escape("~value"), "\\~value");
        assert_eq!(tmux_args_escape("${value}"), "\"\\${value}\"");
        assert_eq!(tmux_args_escape("line\nnext"), "line\\nnext");
        assert_eq!(tmux_args_escape("界"), "界");

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "base-index", "1"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["base-index"]))
                .unwrap()
                .output,
            ""
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-A", "base-index"]),
                )
                .unwrap()
                .output,
            "base-index* 1"
        );
        engine
            .execute(&mut context, &command("set-option", &["base-index", "2"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-v", "base-index"]),
                )
                .unwrap()
                .output,
            "2"
        );

        let status_left = "left ${session_name} right";
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status-left", status_left]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "status-left"]),
                )
                .unwrap()
                .output,
            "status-left \"left \\${session_name} right\""
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "status-left"]),
                )
                .unwrap()
                .output,
            status_left
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gq", "not-an-option"]),
                )
                .unwrap()
                .output,
            ""
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gA", "escape-time"]),
                )
                .unwrap()
                .output,
            "escape-time 10"
        );
    }

    #[test]
    fn behavior_option_defaults_match_pin_readback() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        for (command_name, args, expected) in [
            ("show-options", vec!["-gv", "mouse"], "on"),
            ("show-options", vec!["-sv", "escape-time"], "10"),
            ("show-window-options", vec!["-gv", "automatic-rename"], "on"),
            (
                "show-window-options",
                vec!["-gv", "automatic-rename-format"],
                DEFAULT_AUTOMATIC_RENAME_FORMAT,
            ),
            ("show-window-options", vec!["-gv", "remain-on-exit"], "off"),
            (
                "show-options",
                vec!["-sv", "default-terminal"],
                "tmux-256color",
            ),
            ("show-options", vec!["-gv", "display-time"], "750"),
            ("show-options", vec!["-gv", "repeat-time"], "500"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command(command_name, &args))
                    .unwrap()
                    .output,
                expected
            );
        }
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-g", "automatic-rename-format"],),
                )
                .unwrap()
                .output,
            format!("automatic-rename-format \"{DEFAULT_AUTOMATIC_RENAME_FORMAT}\"")
        );
        assert_eq!(engine.default_terminal_for_spawn(), None);
    }

    #[test]
    fn behavior_options_store_inherit_unset_and_validate_typed_values() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();

        for args in [
            &["mouse", "on"] as &[&str],
            &["display-time", "1200"],
            &["repeat-time", "650"],
        ] {
            engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
        }
        for (name, expected) in [
            ("mouse", "on"),
            ("display-time", "1200"),
            ("repeat-time", "650"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &["-v", name]),)
                    .unwrap()
                    .output,
                expected
            );
        }
        assert_eq!(engine.repeat_time_for_session(session), 650);

        engine
            .execute(
                &mut context,
                &command("set-window-option", &["automatic-rename", "off"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["automatic-rename-format", "#{pane_title}"],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["remain-on-exit", "on"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-p", "remain-on-exit", "failed"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-pv", "remain-on-exit"]),
                )
                .unwrap()
                .output,
            "failed"
        );
        let pane = context.pane.unwrap();
        assert!(!engine.retain_exited_pane(pane, false).unwrap());
        assert!(engine.retain_exited_pane(pane, true).unwrap());
        engine
            .execute(
                &mut context,
                &command("set-option", &["-pu", "remain-on-exit"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-pA", "remain-on-exit"]),
                )
                .unwrap()
                .output,
            "remain-on-exit* on"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-p", "remain-on-exit", "key"]),
            )
            .unwrap();
        engine.state.mark_pane_dead(pane, Some(0)).unwrap();
        assert!(engine.dead_pane_dismisses_on_key(pane));
        engine.state.revive_pane(pane).unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-pu", "remain-on-exit"]),
            )
            .unwrap();

        engine
            .execute(
                &mut context,
                &command("set-option", &["-s", "default-terminal", "zz-term"]),
            )
            .unwrap();
        assert_eq!(engine.default_terminal_for_spawn(), Some("zz-term"));
        engine
            .execute(
                &mut context,
                &command("set-option", &["-su", "default-terminal"]),
            )
            .unwrap();
        assert_eq!(engine.default_terminal_for_spawn(), None);
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-sv", "default-terminal"]),
                )
                .unwrap()
                .output,
            "tmux-256color"
        );

        for (args, expected) in [
            (vec!["-g", "mouse", "maybe"], "bad value: maybe"),
            (
                vec!["-gw", "remain-on-exit", "maybe"],
                "unknown value: maybe",
            ),
            (vec!["-s", "escape-time", "-1"], "value is too small: -1"),
            (
                vec!["-g", "repeat-time", "2000001"],
                "value is too large: 2000001",
            ),
        ] {
            assert!(matches!(
                engine.execute(&mut context, &command("set-option", &args)),
                Err(ServerError::InvalidCommand(message)) if message == expected
            ));
        }

        for name in ["mouse", "display-time", "repeat-time"] {
            engine
                .execute(&mut context, &command("set-option", &["-u", name]))
                .unwrap();
        }
        assert!(engine.mouse_for_session(session));
        assert_eq!(engine.display_time_for_session(session), 750);
        assert_eq!(engine.repeat_time_for_session(session), 500);
    }

    #[test]
    fn explicit_window_names_pin_automatic_rename_off() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.window.unwrap();
        assert!(engine.state.window_automatic_rename(first).unwrap());

        engine
            .execute(&mut context, &command("rename-window", &["manual"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "show-options",
                        &["-wv", "-t", &first.to_string(), "automatic-rename"],
                    ),
                )
                .unwrap()
                .output,
            "off"
        );
        assert!(!engine.state.snapshot().sessions[0].windows[0].automatic_rename);

        engine
            .execute(&mut context, &command("new-window", &["-n", "explicit"]))
            .unwrap();
        let named = context.window.unwrap();
        assert!(!engine.state.window_automatic_rename(named).unwrap());

        engine
            .execute(&mut context, &command("new-window", &[]))
            .unwrap();
        let automatic = context.window.unwrap();
        assert!(engine.state.window_automatic_rename(automatic).unwrap());

        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "named", "-n", "first-window"]),
            )
            .unwrap();
        let first_named = context.window.unwrap();
        assert!(!engine.state.window_automatic_rename(first_named).unwrap());
    }

    #[test]
    fn retained_dead_facts_and_respawn_keep_pane_identity_and_layout() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let pane = context.pane.unwrap();
        let window = context.window.unwrap();
        let layout = engine.state.windows[&window].layout.project();

        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "remain-on-exit", "on"]),
            )
            .unwrap();
        engine.state.mark_pane_dead(pane, Some(7)).unwrap();
        assert!(engine.retain_exited_pane(pane, false).unwrap());
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        &[
                            "-p",
                            "-t",
                            &pane.to_string(),
                            "#{pane_id}:#{pane_dead}:#{pane_dead_status}"
                        ],
                    ),
                )
                .unwrap()
                .output,
            format!("{pane}:1:7")
        );
        assert!(matches!(
            engine
                .execute(
                    &mut context,
                    &command("send-keys", &["-t", &pane.to_string(), "x"]),
                )
                .unwrap()
                .effects
                .as_slice(),
            [MuxEffect::SendKeys { pane: target, .. }] if *target == pane
        ));

        let respawn = engine
            .execute(
                &mut context,
                &command(
                    "respawn-pane",
                    &[
                        "-t",
                        &pane.to_string(),
                        "-c",
                        "/tmp",
                        "-e",
                        "ONE=1",
                        "-e",
                        "TWO=2",
                        "printf ready",
                    ],
                ),
            )
            .unwrap();
        assert!(matches!(
            respawn.effects.as_slice(),
            [
                MuxEffect::PaneRespawned {
                    pane: actual,
                    cwd: Some(cwd),
                    command: Some(command),
                    environment,
                },
                MuxEffect::SnapshotChanged,
            ] if *actual == pane
                && cwd == "/tmp"
                && command == "printf ready"
                && environment == &vec![("ONE".to_owned(), "1".to_owned()), ("TWO".to_owned(), "2".to_owned())]
        ));
        assert_eq!(engine.state.windows[&window].layout.project(), layout);
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        &[
                            "-p",
                            "-t",
                            &pane.to_string(),
                            "#{pane_id}:#{pane_dead}:#{pane_dead_status}"
                        ],
                    ),
                )
                .unwrap()
                .output,
            format!("{pane}:0:")
        );

        assert!(matches!(
            engine.execute(
                &mut context,
                &command("respawn-pane", &["-t", &pane.to_string()]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "respawn pane failed: pane work:0.1 still active"
        ));
        assert!(
            engine
                .execute(
                    &mut context,
                    &command("respawn-pane", &["-k", "-t", &pane.to_string()]),
                )
                .is_ok()
        );
    }

    #[test]
    fn respawn_window_keeps_the_first_pane_and_collapses_the_layout() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-v"]))
            .unwrap();
        let window = context.window.unwrap();
        let panes = engine.state.windows[&window].pane_order().to_vec();
        let retained = panes[0];
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("respawn-window", &["-t", &window.to_string()]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "respawn window failed: window work:0 still active"
        ));
        for pane in &panes {
            engine.state.mark_pane_dead(*pane, Some(0)).unwrap();
        }

        let execution = engine
            .execute(
                &mut context,
                &command("respawn-window", &["-t", &window.to_string()]),
            )
            .unwrap();
        assert!(matches!(
            execution.effects.as_slice(),
            [MuxEffect::PanesRemoved(removed), MuxEffect::PaneRespawned { pane, .. }, MuxEffect::SnapshotChanged]
                if removed == &panes[1..] && *pane == retained
        ));
        let window_state = &engine.state.windows[&window];
        assert_eq!(window_state.pane_order(), &[retained]);
        assert_eq!(
            window_state.layout.project(),
            zz_protocol::LayoutNode::Pane(retained)
        );
        assert!(!window_state.panes[&retained].dead);
    }

    #[test]
    fn option_table_defaults_match_the_engine_except_history_limit() {
        let engine = MuxEngine::default();
        let mismatches = tmux_options()
            .filter_map(|option| option.default.map(|default| (option, default)))
            .filter_map(|(option, default)| {
                let runtime = engine
                    .global_tmux_option_value(option.name)
                    .unwrap_or_else(|| panic!("missing runtime default for {}", option.name));
                (runtime != default.value()).then_some(option.name)
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(mismatches, BTreeSet::from(["history-limit"]));
    }

    #[test]
    fn status_readback_uses_the_pin_defaults() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "status-left"]),
                )
                .unwrap()
                .output,
            "status-left \"[#{session_name}] \""
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "status-right"]),
                )
                .unwrap()
                .output,
            "status-right \"#{?window_bigger,[#{window_offset_x}#,#{window_offset_y}] ,}\\\"#{=21:pane_title}\\\" %H:%M %d-%b-%y\""
        );
    }

    #[test]
    fn fed_mode_keys_default_precedes_explicit_configuration() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine.set_default_mode_keys("vi").unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.expect("pane");

        assert_eq!(
            engine.copy_mode_table_for_pane(pane).unwrap(),
            "copy-mode-vi"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-g", "mode-keys"]),)
                .unwrap()
                .output,
            "mode-keys vi"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-gw", "mode-keys", "emacs"]),
            )
            .unwrap();
        assert_eq!(engine.copy_mode_table_for_pane(pane).unwrap(), "copy-mode");
    }

    #[test]
    fn indexed_option_spellings_route_like_the_pin() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        for args in [
            &["-g", "status-format", "value"] as &[&str],
            &["-g", "status-format[0]", "value"],
            &["-g", "command-alias[0]", "value"],
            &["-g", "terminal-features[0]", "value"],
            &["-g", "update-environment[0]", "value"],
            &["-gw", "pane-colors[0]", "value"],
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("set-option", args))
                    .unwrap(),
                Execution::default()
            );
        }
        for argument in ["status-format", "status-format[0]", "pane-colors[0]"] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &["-g", argument]),)
                    .unwrap()
                    .output,
                ""
            );
        }

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@plain", "solo"]),
            )
            .unwrap();
        for index in ["0", "7", "key"] {
            let spelling = format!("@plain[{index}]");
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &["-gv", &spelling]),)
                    .unwrap()
                    .output,
                "solo"
            );
        }
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "@plain[000]"]),
                )
                .unwrap()
                .output,
            "@plain[0] solo"
        );

        for args in [
            &["-g", "@arr[0]", "first"] as &[&str],
            &["-gq", "@arr[0]", "first"],
            &["-g", "base-index[0]", "1"],
            &["-gq", "base-index[0]", "1"],
            &["-g", "escape-time[0]", "1"],
        ] {
            let spelling = args[1];
            assert!(matches!(
                engine.execute(&mut context, &command("set-option", args)),
                Err(ServerError::InvalidCommand(message))
                    if message == format!("not an array: {spelling}")
            ));
        }
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "base-index[000]"]),
                )
                .unwrap()
                .output,
            "base-index[0] 0"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "escape-time[0]"]),
                )
                .unwrap()
                .output,
            "escape-time[0] 10"
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("show-options", &["-g", "status-format[]"]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "invalid option: status-format[]"
        ));
    }

    #[test]
    fn plain_option_listings_expose_only_tmux_and_user_names() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        for args in [
            &["-s", "@listed", "server"] as &[&str],
            &["-g", "@listed", "global-session"],
            &["@listed", "session"],
            &["-gw", "@listed", "global-window"],
            &["-w", "@listed", "window"],
            &["-p", "@listed", "pane"],
        ] {
            engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
        }
        let tmux_names = tmux_options()
            .map(|option| option.name)
            .collect::<BTreeSet<_>>();
        for args in [&["-s"] as &[&str], &["-g"], &[], &["-gw"], &["-w"], &["-p"]] {
            let output = engine
                .execute(&mut context, &command("show-options", args))
                .unwrap()
                .output;
            assert!(output.lines().any(|line| line.starts_with("@listed ")));
            for line in output.lines() {
                let name = line
                    .split_ascii_whitespace()
                    .next()
                    .expect("listed option name")
                    .trim_end_matches('*');
                assert!(name.starts_with('@') || tmux_names.contains(name), "{line}");
                assert!(!is_native_option(name), "{line}");
            }
        }
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-s", "history-trickle"]),
                )
                .unwrap()
                .output,
            "history-trickle 2000"
        );
    }

    #[test]
    fn user_options_are_exact_pure_storage_at_every_scope() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        for args in [
            &["-s", "@scope", "server"] as &[&str],
            &["-g", "@scope", "global-session"],
            &["@scope", "session"],
            &["-gw", "@scope", "global-window"],
            &["-w", "@scope", "window"],
            &["-p", "@scope", "pane"],
        ] {
            engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
        }
        for (args, expected) in [
            (&["-sv", "@scope"] as &[&str], "server"),
            (&["-gv", "@scope"], "global-session"),
            (&["-v", "@scope"], "session"),
            (&["-gwv", "@scope"], "global-window"),
            (&["-wv", "@scope"], "window"),
            (&["-pv", "@scope"], "pane"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", args))
                    .unwrap()
                    .output,
                expected
            );
        }

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@plugin", "one two"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-ga", "@plugin", " three"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@plug", "exact"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-g", "@plugin"]),)
                .unwrap()
                .output,
            "@plugin \"one two three\""
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-gv", "@plug"]),)
                .unwrap()
                .output,
            "exact"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@inherited", "parent"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-A", "@inherited"]),
                )
                .unwrap()
                .output,
            "@inherited* parent"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["@inherited", "child"]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("set-option", &["-u", "@inherited"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-A", "@inherited"]),
                )
                .unwrap()
                .output,
            "@inherited* parent"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-q", "@missing"]),)
                .unwrap()
                .output,
            ""
        );
    }

    #[test]
    fn environments_read_back_and_merge_with_pin_shell_syntax() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "first"]))
            .unwrap();
        let first = context.session.unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-s", "second"]))
            .unwrap();
        let second = context.session.unwrap();

        for args in [
            &["-g", "GLOBAL", "global"] as &[&str],
            &["-g", "SHARED", "global"],
            &["-gF", "GLOBAL_FORMAT", "#{session_name}"],
            &["-t", "first", "SHARED", "session"],
            &["-t", "first", "QUOTED", "a$b`c\"d\\e"],
            &["-ht", "first", "SECRET", "hidden"],
            &["-rt", "first", "REMOVED"],
            &["-Ft", "first", "FORMATTED", "#{session_name}"],
        ] {
            engine
                .execute(&mut context, &command("set-environment", args))
                .unwrap();
        }

        for (args, expected) in [
            (&["-g", "GLOBAL"] as &[&str], "GLOBAL=global"),
            (&["-g", "GLOBAL_FORMAT"], "GLOBAL_FORMAT=second"),
            (&["-t", "first", "SHARED"], "SHARED=session"),
            (
                &["-st", "first", "QUOTED"],
                r#"QUOTED="a\$b\`c\"d\\e"; export QUOTED;"#,
            ),
            (&["-t", "first", "SECRET"], ""),
            (&["-ht", "first", "SECRET"], "SECRET=hidden"),
            (&["-t", "first", "REMOVED"], "-REMOVED"),
            (&["-st", "first", "REMOVED"], "unset REMOVED;"),
            (&["-t", "first", "FORMATTED"], "FORMATTED=first"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-environment", args))
                    .unwrap()
                    .output,
                expected
            );
        }

        let first_environment = engine
            .environment_for_session(first)
            .unwrap()
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(first_environment["GLOBAL"].as_deref(), Some("global"));
        assert_eq!(first_environment["SHARED"].as_deref(), Some("session"));
        assert_eq!(first_environment["SECRET"], None);
        assert_eq!(first_environment["REMOVED"], None);
        let second_environment = engine
            .environment_for_session(second)
            .unwrap()
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(second_environment["SHARED"].as_deref(), Some("global"));

        engine
            .execute(
                &mut context,
                &command("set-environment", &["-ut", "first", "SHARED"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .environment_for_session(first)
                .unwrap()
                .into_iter()
                .collect::<BTreeMap<_, _>>()["SHARED"]
                .as_deref(),
            Some("global")
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("show-environment", &["-t", "first", "MISSING"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "unknown variable: MISSING"
        ));
    }

    #[test]
    fn global_environment_seed_populates_session_update_markers() {
        let mut engine = MuxEngine::default();
        engine.seed_global_environment([
            ("DISPLAY", ":7"),
            ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
            ("PHASE4D_EXTRA", "global"),
        ]);
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session");

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-environment", &["-g", "PHASE4D_EXTRA"]),
                )
                .unwrap()
                .output,
            "PHASE4D_EXTRA=global"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("show-environment", &["KRB5CCNAME"]),)
                .unwrap()
                .output,
            "-KRB5CCNAME"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("show-environment", &[]))
                .unwrap()
                .output
                .lines()
                .count(),
            13
        );
        let environment = engine
            .environment_for_session(session)
            .unwrap()
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(environment["DISPLAY"].as_deref(), Some(":7"));
        assert_eq!(
            environment["SSH_AUTH_SOCK"].as_deref(),
            Some("/tmp/agent.sock")
        );
        assert_eq!(environment["KRB5CCNAME"], None);
        assert_eq!(environment["PHASE4D_EXTRA"].as_deref(), Some("global"));
    }

    #[test]
    fn index_options_validate_pin_bounds_scopes_and_unset_values() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("session");
        let session = context.session.expect("session");
        let window = context.window.expect("window");
        let pane = context.pane.expect("pane");

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-g", "base-index", &MAX_BASE_INDEX.to_string()],
                ),
            )
            .expect("maximum base index");
        assert_eq!(engine.base_index_for_session(session), MAX_BASE_INDEX);
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-g", "pane-base-index", &MAX_PANE_BASE_INDEX.to_string()],
                ),
            )
            .expect("maximum pane base index");
        assert_eq!(engine.pane_index(window, pane), Some(MAX_PANE_BASE_INDEX));

        for invalid in [
            command("set-option", &["-g", "base-index"]),
            command("set-option", &["-g", "base-index", "-1"]),
            command("set-option", &["-g", "base-index", "nope"]),
            command(
                "set-option",
                &["-g", "base-index", &(MAX_BASE_INDEX + 1).to_string()],
            ),
            command("set-window-option", &["-g", "pane-base-index"]),
            command("set-window-option", &["-g", "pane-base-index", "-1"]),
            command("set-window-option", &["-g", "pane-base-index", "nope"]),
            command(
                "set-window-option",
                &[
                    "-g",
                    "pane-base-index",
                    &(MAX_PANE_BASE_INDEX + 1).to_string(),
                ],
            ),
            command("set-option", &["-g", "renumber-windows", "maybe"]),
        ] {
            assert!(
                engine.execute(&mut context, &invalid).is_err(),
                "{invalid:?}"
            );
        }
        for (invalid, expected) in [
            (
                command("set-option", &["-g", "base-index", "nope"]),
                "value is invalid: nope",
            ),
            (
                command("set-option", &["-g", "base-index", "-1"]),
                "value is too small: -1",
            ),
            (
                command(
                    "set-window-option",
                    &[
                        "-g",
                        "pane-base-index",
                        &(MAX_PANE_BASE_INDEX + 1).to_string(),
                    ],
                ),
                "value is too large: 65536",
            ),
        ] {
            assert!(matches!(
                engine.execute(&mut context, &invalid),
                Err(ServerError::InvalidCommand(message)) if message == expected
            ));
        }

        engine
            .execute(&mut context, &command("set-option", &["-gu", "base-index"]))
            .expect("restore base default");
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-gu", "pane-base-index"]),
            )
            .expect("restore pane base default");
        assert_eq!(engine.base_index_for_session(session), DEFAULT_BASE_INDEX);
        assert_eq!(
            engine.pane_index(window, pane),
            Some(DEFAULT_PANE_BASE_INDEX)
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "renumber-windows"]),
            )
            .expect("toggle global renumbering on");
        assert!(engine.renumber_windows_for_session(session));
        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-t", &session.to_string(), "renumber-windows", "off"],
                ),
            )
            .expect("session renumber override");
        assert!(!engine.renumber_windows_for_session(session));
        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-u", "-t", &session.to_string(), "renumber-windows"],
                ),
            )
            .expect("restore renumber inheritance");
        assert!(engine.renumber_windows_for_session(session));
        engine
            .execute(
                &mut context,
                &command("set-option", &["-gu", "renumber-windows"]),
            )
            .expect("restore renumber default");
        assert!(!engine.renumber_windows_for_session(session));
    }

    #[test]
    fn table_option_scope_ignores_command_spelling_and_unrelated_scope_flags() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "first"]))
            .unwrap();
        let first_session = context.session.unwrap();
        let first_window = context.window.unwrap();
        let first_pane = context.pane.unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-s", "second"]))
            .unwrap();
        let second_session = context.session.unwrap();
        let second_window = context.window.unwrap();
        let second_pane = context.pane.unwrap();

        engine
            .execute(&mut context, &command("setw", &["-g", "base-index", "1"]))
            .unwrap();
        assert_eq!(engine.base_index_for_session(first_session), 1);
        assert_eq!(engine.base_index_for_session(second_session), 1);
        for (flag, value) in [("-w", "2"), ("-s", "3"), ("-p", "4")] {
            engine
                .execute(&mut context, &command("set", &[flag, "base-index", value]))
                .unwrap();
        }
        engine
            .execute(&mut context, &command("setw", &["base-index", "5"]))
            .unwrap();
        assert_eq!(engine.base_index_for_session(first_session), 1);
        assert_eq!(engine.base_index_for_session(second_session), 5);

        for (flag, value) in [("-w", "2"), ("-s", "3"), ("-p", "4")] {
            engine
                .execute(
                    &mut context,
                    &command("set", &[flag, "pane-base-index", value]),
                )
                .unwrap();
        }
        engine
            .execute(&mut context, &command("setw", &["pane-base-index", "5"]))
            .unwrap();
        assert_eq!(engine.pane_index(second_window, second_pane), Some(5));

        engine
            .execute(
                &mut context,
                &command("set", &["-t", &first_pane.to_string(), "base-index", "7"]),
            )
            .unwrap();
        assert_eq!(engine.base_index_for_session(first_session), 7);
        engine
            .execute(
                &mut context,
                &command(
                    "set",
                    &["-s", "-t", &first_window.to_string(), "base-index", "8"],
                ),
            )
            .unwrap();
        assert_eq!(engine.base_index_for_session(first_session), 8);
        engine
            .execute(
                &mut context,
                &command(
                    "set",
                    &["-p", "-t", &first_pane.to_string(), "pane-base-index", "6"],
                ),
            )
            .unwrap();
        assert_eq!(engine.pane_index(first_window, first_pane), Some(6));
        engine
            .execute(
                &mut context,
                &command(
                    "setw",
                    &["-t", &first_window.to_string(), "pane-base-index", "7"],
                ),
            )
            .unwrap();
        assert_eq!(engine.pane_index(first_window, first_pane), Some(7));
    }

    #[test]
    fn set_option_matches_full_table_prefixes_and_quiets_unknown_names() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();

        engine
            .execute(&mut context, &command("set", &["-g", "base-ind", "6"]))
            .unwrap();
        assert_eq!(engine.base_index_for_session(session), 6);

        let error = engine
            .execute(&mut context, &command("set", &["-g", "status-l", "value"]))
            .unwrap_err();
        assert!(matches!(
            error,
            ServerError::InvalidCommand(message) if message == "ambiguous option: status-l"
        ));
        engine
            .execute(&mut context, &command("set", &["-g", "escape-t", "17"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-sv", "escape-time"])
                )
                .unwrap()
                .output,
            "17"
        );
        let error = engine
            .execute(&mut context, &command("set", &["-g", "not-an-option", "1"]))
            .unwrap_err();
        assert!(matches!(
            error,
            ServerError::InvalidCommand(message) if message == "invalid option: not-an-option"
        ));
        for args in [
            &["-gq", "not-an-option", "1"][..],
            &["-gq", "status-l", "value"][..],
        ] {
            assert_eq!(
                engine.execute(&mut context, &command("set", args)).unwrap(),
                Execution::default()
            );
        }
    }

    #[test]
    fn index_options_accept_tmux_operation_flag_combinations() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        let window = context.window.unwrap();
        let pane = context.pane.unwrap();

        engine
            .execute(&mut context, &command("set", &["-a", "base-index", "2"]))
            .unwrap();
        assert_eq!(engine.base_index_for_session(session), 2);
        engine
            .execute(
                &mut context,
                &command("set", &["-u", "base-index", "not-a-number"]),
            )
            .unwrap();
        assert_eq!(engine.base_index_for_session(session), DEFAULT_BASE_INDEX);
        engine
            .execute(
                &mut context,
                &command("set", &["-ou", "base-index", "not-a-number"]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("set", &["-o", "base-index", "3"]))
            .unwrap();
        assert_eq!(engine.base_index_for_session(session), 3);
        assert!(
            engine
                .execute(&mut context, &command("set", &["-o", "base-index", "4"]))
                .is_err()
        );
        engine
            .execute(&mut context, &command("set", &["-oq", "base-index", "4"]))
            .unwrap();
        assert_eq!(engine.base_index_for_session(session), 3);
        engine
            .execute(
                &mut context,
                &command("set", &["-U", "base-index", "not-a-number"]),
            )
            .unwrap();
        assert_eq!(engine.base_index_for_session(session), DEFAULT_BASE_INDEX);

        engine
            .execute(
                &mut context,
                &command("set", &["-a", "pane-base-index", "2"]),
            )
            .unwrap();
        assert_eq!(engine.pane_index(window, pane), Some(2));
        engine
            .execute(
                &mut context,
                &command("set", &["-ou", "pane-base-index", "not-a-number"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set", &["-o", "pane-base-index", "3"]),
            )
            .unwrap();
        assert_eq!(engine.pane_index(window, pane), Some(3));
        engine
            .execute(
                &mut context,
                &command("set", &["-U", "pane-base-index", "not-a-number"]),
            )
            .unwrap();
        assert_eq!(
            engine.pane_index(window, pane),
            Some(DEFAULT_PANE_BASE_INDEX)
        );

        engine
            .execute(
                &mut context,
                &command("set", &["-a", "renumber-windows", "on"]),
            )
            .unwrap();
        assert!(engine.renumber_windows_for_session(session));
        engine
            .execute(
                &mut context,
                &command("set", &["-u", "renumber-windows", "true"]),
            )
            .unwrap();
        assert!(!engine.renumber_windows_for_session(session));
        engine
            .execute(
                &mut context,
                &command("set", &["-o", "renumber-windows", "on"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set", &["-ou", "renumber-windows", "false"]),
            )
            .unwrap();
        assert!(!engine.renumber_windows_for_session(session));
    }

    #[test]
    fn tmux_boolean_options_reject_true_and_false() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        for option in ["renumber-windows", "synchronize-panes"] {
            for value in ["true", "false"] {
                let error = engine
                    .execute(&mut context, &command("set", &[option, value]))
                    .unwrap_err();
                assert!(matches!(
                    error,
                    ServerError::InvalidCommand(message)
                        if message == format!("bad value: {value}")
                ));
            }
        }
    }

    #[test]
    fn word_separators_are_live_inherited_session_options() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "first"]))
            .expect("first session");
        let first_session = context.session.expect("first session id");
        let first_pane = context.pane.expect("first pane");
        assert_eq!(
            engine.word_separators_for_pane(first_pane).unwrap(),
            DEFAULT_WORD_SEPARATORS
        );

        let global = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "word-separators", ""]),
            )
            .expect("empty global separators");
        assert_eq!(
            global.effects,
            [
                MuxEffect::WordSeparatorsChanged { session: None },
                MuxEffect::MuxOptionChanged {
                    option: MuxOptionKey::WordSeparators,
                },
            ]
        );
        assert_eq!(engine.word_separators_for_session(first_session), "");

        let targeted = engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-t", &first_session.to_string(), "word-separators", "."],
                ),
            )
            .expect("session separators");
        assert_eq!(
            targeted.effects,
            [MuxEffect::WordSeparatorsChanged {
                session: Some(first_session)
            }]
        );
        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &[
                        "-a",
                        "-t",
                        &first_session.to_string(),
                        "word-separators",
                        "λ",
                    ],
                ),
            )
            .expect("append unicode separator");
        assert_eq!(engine.word_separators_for_session(first_session), ".λ");

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "word-separators", "|"]),
            )
            .expect("replace global separators");
        assert_eq!(engine.word_separators_for_session(first_session), ".λ");
        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-u", "-t", &first_session.to_string(), "word-separators"],
                ),
            )
            .expect("restore inheritance");
        assert_eq!(engine.word_separators_for_session(first_session), "|");

        let oversized = "x".repeat(MAX_WORD_SEPARATORS_BYTES + 1);
        assert!(
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-g", "word-separators", &oversized]),
                )
                .is_err()
        );
        assert_eq!(engine.word_separators_for_session(first_session), "|");
    }

    #[test]
    fn mode_keys_follow_window_scope_inheritance_and_live_effects() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "modes"]))
            .expect("first window");
        let first_window = context.window.expect("first window id");
        let first_pane = context.pane.expect("first pane");
        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .expect("second window");
        let second_window = context.window.expect("second window id");
        let second_pane = context.pane.expect("second pane");
        assert_eq!(
            engine.copy_mode_table_for_pane(first_pane).unwrap(),
            "copy-mode"
        );
        assert_eq!(
            engine.copy_mode_table_for_pane(second_pane).unwrap(),
            "copy-mode"
        );

        let targeted = engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-t", &first_window.to_string(), "mode-keys", "vi"],
                ),
            )
            .expect("window mode table");
        assert_eq!(
            targeted.effects,
            [MuxEffect::ModeKeysChanged {
                window: Some(first_window)
            }]
        );
        assert_eq!(
            engine.copy_mode_table_for_pane(first_pane).unwrap(),
            "copy-mode-vi"
        );
        assert_eq!(
            engine.copy_mode_table_for_pane(second_pane).unwrap(),
            "copy-mode"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-t", &first_window.to_string(), "mode-keys"],
                ),
            )
            .expect("toggle choice");
        assert_eq!(
            engine.copy_mode_table_for_pane(first_pane).unwrap(),
            "copy-mode"
        );

        let global = engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "mode-keys", "vi"]),
            )
            .expect("global window default");
        assert_eq!(
            global.effects,
            [
                MuxEffect::ModeKeysChanged { window: None },
                MuxEffect::MuxOptionChanged {
                    option: MuxOptionKey::ModeKeys,
                },
            ]
        );
        assert_eq!(
            engine.copy_mode_table_for_pane(first_pane).unwrap(),
            "copy-mode"
        );
        assert_eq!(
            engine.copy_mode_table_for_pane(second_pane).unwrap(),
            "copy-mode-vi"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-u", "-t", &first_window.to_string(), "mode-keys"],
                ),
            )
            .expect("restore window inheritance");
        assert_eq!(
            engine.copy_mode_table_for_pane(first_pane).unwrap(),
            "copy-mode-vi"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-o", "-t", &second_window.to_string(), "mode-keys", "emacs"],
                ),
            )
            .expect("set inherited option once");
        let quiet = engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-oq", "-t", &second_window.to_string(), "mode-keys", "vi"],
                ),
            )
            .expect("quiet duplicate");
        assert!(quiet.effects.is_empty());
        assert_eq!(
            engine.copy_mode_table_for_pane(second_pane).unwrap(),
            "copy-mode"
        );

        for invalid in [
            command("set-window-option", &["-w", "mode-keys", "vi"]),
            command("set-option", &["mode-keys", "unknown"]),
        ] {
            assert!(engine.execute(&mut context, &invalid).is_err());
        }
        assert_eq!(
            engine.copy_mode_table_for_pane(second_pane).unwrap(),
            "copy-mode"
        );
    }

    #[test]
    fn synchronize_panes_commands_preserve_native_option_inheritance() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &[]))
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();

        let changed = engine
            .execute(
                &mut context,
                &command("set-option", &["-w", "synchronize-panes", "on"]),
            )
            .unwrap();
        assert!(changed.effects.contains(&MuxEffect::SnapshotChanged));
        assert_eq!(
            engine.synchronized_input_targets(first).unwrap(),
            [first, second]
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-p", "-t", &second.to_string(), "synchronize-panes", "off"],
                ),
            )
            .unwrap();
        assert_eq!(engine.synchronized_input_targets(first).unwrap(), [first]);
        assert_eq!(engine.synchronized_input_targets(second).unwrap(), [second]);

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-p", "-u", "-t", &second.to_string(), "synchronize-panes"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine.synchronized_input_targets(second).unwrap(),
            [first, second]
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-w", "synchronize-panes"]),
            )
            .unwrap();
        assert_eq!(engine.synchronized_input_targets(first).unwrap(), [first]);

        engine
            .execute(
                &mut context,
                &command("setw", &["-g", "synchronize-panes", "on"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-p", "-t", &second.to_string(), "synchronize-panes", "off"],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-U", "synchronize-panes"]),
            )
            .unwrap();
        assert_eq!(
            engine.synchronized_input_targets(first).unwrap(),
            [first, second]
        );
    }

    #[test]
    fn command_prompt_builds_a_native_client_effect() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        let execution = engine
            .execute(
                &mut context,
                &command(
                    "command-prompt",
                    &[
                        "-b",
                        "-p",
                        "window name",
                        "-I",
                        "scratch",
                        "new-window -n %%",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(
            execution.effects,
            vec![MuxEffect::CommandPrompt {
                prompt: "window name".to_owned(),
                input: "scratch".to_owned(),
                template: Some("new-window -n %%".to_owned()),
            }]
        );

        engine
            .execute(&mut context, &command("new-session", &["-s", "work tree"]))
            .expect("prompt context session");
        engine
            .execute(&mut context, &command("rename-window", &["editor pane"]))
            .expect("prompt context window");
        let execution = engine
            .execute(
                &mut context,
                &command(
                    "command-prompt",
                    &["-I", "#S / #W", "rename-window -- '%%'"],
                ),
            )
            .expect("expanded prompt input");
        assert_eq!(
            execution.effects,
            vec![MuxEffect::CommandPrompt {
                prompt: ":".to_owned(),
                input: "work tree / editor pane".to_owned(),
                template: Some("rename-window -- '%%'".to_owned()),
            }]
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("command-prompt", &["-F", "unsupported"]),
            ),
            Err(ServerError::UnsupportedCommand(message))
                if message == "command-prompt -F"
        ));
    }

    #[test]
    fn session_and_window_choosers_route_to_sidebar_while_pane_tree_remains_native() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();

        for (name, args) in [
            ("choose-tree", vec!["-Zs"]),
            ("choose-tree", vec!["-Zw"]),
            ("focus-sidebar", Vec::new()),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command(name, &args))
                    .unwrap()
                    .effects,
                vec![MuxEffect::FocusSidebar { pane }]
            );
        }

        assert_eq!(
            engine
                .execute(&mut context, &command("choose-tree", &[]))
                .unwrap()
                .effects,
            vec![MuxEffect::ChooseTree {
                pane,
                kind: ChooseTreeKind::Panes,
            }]
        );

        assert!(matches!(
            engine.execute(&mut context, &command("choose-tree", &["-sw"])),
            Err(ServerError::InvalidCommand(_))
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("choose-tree", &["select-pane", "-t", "%%"]),
            ),
            Err(ServerError::InvalidCommand(_))
        ));
    }

    #[test]
    fn choose_buffer_builds_a_native_effect_and_rejects_extended_templates() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();

        assert_eq!(
            engine
                .execute(&mut context, &command("choose-buffer", &["-Z"]))
                .unwrap()
                .effects,
            vec![MuxEffect::ChooseBuffer { pane }]
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("choose-buffer", &["-F", "#{buffer_name}"])
            ),
            Err(ServerError::UnsupportedCommand(message))
                if message == "choose-buffer -F"
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("choose-buffer", &["paste-buffer", "-b", "%%"]),
            ),
            Err(ServerError::InvalidCommand(_))
        ));
    }

    #[test]
    fn display_panes_builds_a_timed_native_overlay_effect() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();

        for (name, args, duration_ms) in [
            ("display-panes", Vec::new(), 750),
            ("displayp", vec!["-b", "-d2500"], 2_500),
            ("display-panes", vec!["-d", "0"], 0),
        ] {
            let execution = engine.execute(&mut context, &command(name, &args)).unwrap();
            assert_eq!(
                execution.effects,
                vec![MuxEffect::DisplayPanes { pane, duration_ms }]
            );
        }
        engine
            .execute(
                &mut context,
                &command("set-option", &["display-time", "1200"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("display-panes", &[]))
                .unwrap()
                .effects,
            vec![MuxEffect::DisplayPanes {
                pane,
                duration_ms: 1200,
            }]
        );
        assert!(matches!(
            engine
                .execute(&mut context, &command("display-message", &["hello"]))
                .unwrap()
                .effects
                .as_slice(),
            [MuxEffect::DisplayMessage { text, duration_ms: 1200, .. }] if text == "hello"
        ));

        assert!(matches!(
            engine.execute(&mut context, &command("display-panes", &["-N"])),
            Err(ServerError::UnsupportedCommand(message))
                if message == "display-panes -N"
        ));
        for args in [vec!["select-pane", "-t", "%%%"], vec!["-d", "forever"]] {
            assert!(matches!(
                engine.execute(&mut context, &command("display-panes", &args)),
                Err(ServerError::InvalidCommand(_))
            ));
        }
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("display-panes", &["-t", "$1"]),
            ),
            Err(ServerError::UnsupportedCommand(message))
                if message == "display-panes -t"
        ));
    }

    #[test]
    fn resize_pane_zoom_toggles_and_select_pane_can_preserve_it() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        let window = context.window.unwrap();

        engine
            .execute(&mut context, &command("resizep", &["-Z"]))
            .unwrap();
        assert_eq!(engine.state.windows[&window].zoomed_pane, Some(second));
        engine
            .execute(
                &mut context,
                &command("select-pane", &["-Z", "-t", &first.to_string()]),
            )
            .unwrap();
        assert_eq!(engine.state.windows[&window].active_pane, first);
        assert_eq!(engine.state.windows[&window].zoomed_pane, Some(first));

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &second.to_string()]),
            )
            .unwrap();
        assert_eq!(engine.state.windows[&window].active_pane, second);
        assert_eq!(engine.state.windows[&window].zoomed_pane, None);
    }

    #[test]
    fn select_pane_changes_cross_window_activity_without_switching_the_session_window() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        let first_window = context.window.unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string()]),
            )
            .unwrap();
        assert_eq!(engine.state.sessions[&session].active_window, first_window);
        assert_eq!(engine.state.windows[&first_window].active_pane, first);
        assert_eq!(context.pane, Some(first));

        engine
            .execute(&mut context, &command("new-window", &["-n", "other"]))
            .unwrap();
        let other_window = context.window.unwrap();
        let other_pane = context.pane.unwrap();
        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &second.to_string()]),
            )
            .unwrap();
        assert_eq!(engine.state.sessions[&session].active_window, other_window);
        assert_eq!(engine.state.windows[&first_window].active_pane, second);
        assert_eq!(context.window, Some(first_window));
        assert_eq!(context.pane, Some(second));

        context = ExecutionContext::for_pane(&engine.state, other_pane).unwrap();
        let generation = engine.state.generation();
        let no_op = engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &second.to_string()]),
            )
            .unwrap();
        assert_eq!(engine.state.sessions[&session].active_window, other_window);
        assert_eq!(context.window, Some(other_window));
        assert_eq!(context.pane, Some(other_pane));
        assert_eq!(engine.state.generation(), generation);
        assert!(no_op.effects.is_empty());
    }

    #[test]
    fn select_pane_directions_follow_layout_geometry_from_explicit_targets() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let left = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let right_top = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &[]))
            .unwrap();
        let right_bottom = context.pane.unwrap();

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &left.to_string()]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("select-pane", &["-R"]))
            .unwrap();
        assert_eq!(context.pane, Some(right_bottom));

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &left.to_string()]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &right_top.to_string(), "-D"]),
            )
            .unwrap();
        assert_eq!(context.pane, Some(right_bottom));

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &left.to_string()]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("select-pane", &["-U"]))
            .unwrap();
        assert_eq!(context.pane, Some(left));
    }

    #[test]
    fn last_pane_toggles_history_and_can_preserve_zoom() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        let window = context.window.unwrap();

        engine
            .execute(&mut context, &command("lastp", &[]))
            .unwrap();
        assert_eq!(context.pane, Some(first));
        engine
            .execute(&mut context, &command("last-pane", &[]))
            .unwrap();
        assert_eq!(context.pane, Some(second));

        engine
            .execute(&mut context, &command("resize-pane", &["-Z"]))
            .unwrap();
        engine
            .execute(&mut context, &command("last-pane", &["-Z"]))
            .unwrap();
        assert_eq!(context.pane, Some(first));
        assert_eq!(engine.state.windows[&window].zoomed_pane, Some(first));

        assert!(matches!(
            engine.execute(&mut context, &command("last-pane", &["-d"])),
            Err(ServerError::UnsupportedCommand(message))
                if message == "last-pane -d"
        ));
    }

    #[test]
    fn resize_pane_moves_by_cells_when_geometry_is_known() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        let window = context.window.unwrap();

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string()]),
            )
            .unwrap();
        engine.set_pane_geometry(first, 100, 50);
        engine
            .execute(&mut context, &command("resize-pane", &["-R", "10"]))
            .unwrap();
        assert_eq!(engine.pane_geometry(first), Some((110, 50)));
        assert_eq!(engine.pane_geometry(second), Some((89, 50)));
        assert_eq!(engine.window_extent(window, Axis::Horizontal), Some(200));
        assert_eq!(engine.window_extent(window, Axis::Vertical), Some(50));
    }

    #[test]
    fn last_window_toggles_history_and_survives_window_removal() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.window.unwrap();

        assert!(matches!(
            engine.execute(&mut context, &command("last-window", &[])),
            Err(ServerError::InvalidCommand(_))
        ));

        engine
            .execute(&mut context, &command("new-window", &[]))
            .unwrap();
        let second = context.window.unwrap();
        engine.execute(&mut context, &command("last", &[])).unwrap();
        assert_eq!(context.window, Some(first));
        engine
            .execute(&mut context, &command("select-window", &["-l"]))
            .unwrap();
        assert_eq!(context.window, Some(second));

        engine
            .execute(&mut context, &command("kill-window", &[]))
            .unwrap();
        assert_eq!(context.window, Some(first));
        assert!(matches!(
            engine.execute(&mut context, &command("last-window", &[])),
            Err(ServerError::InvalidCommand(_))
        ));
    }

    #[test]
    fn renumber_windows_compacts_close_paths_and_preserves_window_identity() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "base-index", "1"]),
            )
            .expect("global base index");
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "renumber-windows", "on"]),
            )
            .expect("global renumbering");
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("first window");
        let session = context.session.expect("session");
        let first = context.window.expect("first window");
        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .expect("second window");
        let second = context.window.expect("second window");
        let second_pane = context.pane.expect("second pane");
        engine
            .execute(&mut context, &command("new-window", &["-n", "third"]))
            .expect("third window");
        let third = context.window.expect("third window");
        let third_pane = context.pane.expect("third pane");
        assert_eq!(engine.state.sessions[&session].active_window, third);
        assert_eq!(engine.state.sessions[&session].last_window(), Some(second));

        let generation = engine.state.generation();
        let killed = engine
            .execute(&mut context, &command("kill-window", &["-t", "work:1"]))
            .expect("kill first window");
        assert!(killed.effects.contains(&MuxEffect::SnapshotChanged));
        assert!(engine.state.generation() > generation);
        assert!(!engine.state.windows.contains_key(&first));
        assert_eq!(engine.state.windows[&second].index, 1);
        assert_eq!(engine.state.windows[&third].index, 2);
        assert_eq!(engine.state.sessions[&session].active_window, third);
        assert_eq!(engine.state.sessions[&session].last_window(), Some(second));
        let snapshot = engine.state.snapshot();
        assert_eq!(
            snapshot.sessions[0]
                .windows
                .iter()
                .map(|window| (window.id, window.index))
                .collect::<Vec<_>>(),
            [(second, 1), (third, 2)]
        );

        engine
            .execute(
                &mut context,
                &command("kill-pane", &["-t", &second_pane.to_string()]),
            )
            .expect("last pane closes its window");
        assert!(!engine.state.windows.contains_key(&second));
        assert_eq!(engine.state.windows[&third].index, 1);

        let source = engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-n", "source"]),
            )
            .expect("join source")
            .effects
            .iter()
            .find_map(|effect| match effect {
                MuxEffect::PaneCreated { pane, .. } => Some(*pane),
                _ => None,
            })
            .expect("source pane");
        let source_window = engine.state.window_for_pane(source).expect("source window");
        let survivor = engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-n", "survivor"]),
            )
            .expect("surviving window")
            .effects
            .iter()
            .find_map(|effect| match effect {
                MuxEffect::PaneCreated { pane, .. } => Some(*pane),
                _ => None,
            })
            .expect("survivor pane");
        let survivor_window = engine
            .state
            .window_for_pane(survivor)
            .expect("survivor window");
        assert_eq!(engine.state.windows[&source_window].index, 2);
        assert_eq!(engine.state.windows[&survivor_window].index, 3);

        engine
            .execute(
                &mut context,
                &command(
                    "join-pane",
                    &[
                        "-d",
                        "-s",
                        &source.to_string(),
                        "-t",
                        &third_pane.to_string(),
                    ],
                ),
            )
            .expect("join closes source window");
        assert!(!engine.state.windows.contains_key(&source_window));
        assert_eq!(engine.state.windows[&third].index, 1);
        assert_eq!(engine.state.windows[&survivor_window].index, 2);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn swap_pane_moves_native_layout_slots_and_reports_cross_session_relocation() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "first"]))
            .unwrap();
        let left = context.pane.unwrap();
        let first_session = context.session.unwrap();
        let first_window = context.window.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let middle = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &[]))
            .unwrap();
        let right = context.pane.unwrap();

        engine
            .execute(&mut context, &command("swapp", &["-U"]))
            .unwrap();
        let mut panes = engine.state.windows[&first_window].layout.panes_in_order();
        assert_eq!(panes, [left, right, middle]);
        assert_eq!(context.pane, Some(right));

        engine
            .execute(&mut context, &command("swap-pane", &["-d", "-D"]))
            .unwrap();
        panes = engine.state.windows[&first_window].layout.panes_in_order();
        assert_eq!(panes, [left, middle, right]);
        assert_eq!(context.pane, Some(middle));

        engine
            .execute(&mut context, &command("new-session", &["-s", "second"]))
            .unwrap();
        let target = context.pane.unwrap();
        let second_session = context.session.unwrap();
        let second_window = context.window.unwrap();
        let swapped = engine
            .execute(
                &mut context,
                &command(
                    "swap-pane",
                    &["-s", &right.to_string(), "-t", &target.to_string()],
                ),
            )
            .unwrap();
        assert_eq!(
            &swapped.effects[..2],
            [
                MuxEffect::PaneRelocated {
                    pane: right,
                    from: first_session,
                    to: second_session,
                },
                MuxEffect::PaneRelocated {
                    pane: target,
                    from: second_session,
                    to: first_session,
                },
            ]
        );
        assert!(swapped.effects.contains(&MuxEffect::SnapshotChanged));
        assert!(
            engine.state.windows[&first_window]
                .panes
                .contains_key(&target)
        );
        assert!(
            engine.state.windows[&second_window]
                .panes
                .contains_key(&right)
        );
        assert_eq!(context.pane, Some(right));
        assert!(engine.state.validate().is_ok());

        assert!(matches!(
            engine.execute(&mut context, &command("swap-pane", &[])),
            Err(ServerError::InvalidCommand(_))
        ));
    }

    #[test]
    fn break_and_join_commands_reparent_mixed_surfaces_and_validate_the_subset() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let terminal = context.pane.unwrap();
        let session = context.session.unwrap();
        let original_window = context.window.unwrap();
        engine
            .execute(
                &mut context,
                &command("split-browser", &["-h", "https://example.com"]),
            )
            .unwrap();
        let browser = context.pane.unwrap();

        engine
            .execute(
                &mut context,
                &command("breakp", &["-n", "docs", "-s", &browser.to_string()]),
            )
            .unwrap();
        let broken_window = context.window.unwrap();
        assert_ne!(broken_window, original_window);
        assert_eq!(engine.state.windows[&broken_window].name, "docs");
        assert_eq!(
            engine.state.windows[&broken_window].layout.project(),
            zz_protocol::LayoutNode::Pane(browser)
        );

        engine
            .execute(
                &mut context,
                &command(
                    "joinp",
                    &[
                        "-b",
                        "-h",
                        "-p",
                        "30",
                        "-s",
                        &browser.to_string(),
                        "-t",
                        &terminal.to_string(),
                    ],
                ),
            )
            .unwrap();
        assert!(!engine.state.windows.contains_key(&broken_window));
        assert_eq!(context.pane, Some(browser));
        let panes = engine.state.windows[&original_window]
            .layout
            .panes_in_order();
        assert_eq!(panes, [browser, terminal]);
        assert_eq!(
            engine.state.windows[&original_window].pane_order(),
            [terminal, browser]
        );

        engine
            .execute(
                &mut context,
                &command("break-pane", &["-d", "-s", &browser.to_string()]),
            )
            .unwrap();
        let detached_window = engine.state.window_for_pane(browser).unwrap();
        assert_ne!(detached_window, original_window);
        assert_eq!(context.pane, Some(terminal));
        assert_eq!(
            engine.state.sessions[&session].active_window,
            original_window
        );
        engine
            .execute(
                &mut context,
                &command(
                    "movep",
                    &[
                        "-f",
                        "-v",
                        "-p25%",
                        "-s",
                        &browser.to_string(),
                        "-t",
                        &terminal.to_string(),
                    ],
                ),
            )
            .unwrap();
        assert!(!engine.state.windows.contains_key(&detached_window));
        assert_eq!(context.pane, Some(browser));

        engine
            .execute(
                &mut context,
                &command("break-pane", &["-d", "-s", &browser.to_string()]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-s", "remote"]))
            .unwrap();
        let remote = context.pane.unwrap();
        let remote_session = context.session.unwrap();
        let relocated = engine
            .execute(
                &mut context,
                &command(
                    "join-pane",
                    &["-s", &browser.to_string(), "-t", &remote.to_string()],
                ),
            )
            .unwrap();
        assert!(relocated.effects.contains(&MuxEffect::PaneRelocated {
            pane: browser,
            from: session,
            to: remote_session,
        }));
        assert!(relocated.effects.contains(&MuxEffect::SnapshotChanged));
        assert_eq!(context.pane, Some(browser));
        assert!(engine.state.validate().is_ok());

        assert!(matches!(
            engine.execute(
                &mut context,
                &command("join-pane", &["-l", "10", "-s", &browser.to_string()]),
            ),
            Err(ServerError::UnsupportedCommand(_))
        ));
        assert!(matches!(
            engine.execute(&mut context, &command("break-pane", &["-W"])),
            Err(ServerError::UnsupportedCommand(message))
                if message == "break-pane -W"
        ));
    }

    #[test]
    fn join_and_move_preflight_post_close_renumber_capacity() {
        for name in ["join-pane", "move-pane"] {
            let mut engine = MuxEngine::default();
            let mut context = ExecutionContext::default();
            engine
                .execute(&mut context, &command("new-session", &["-s", "work"]))
                .unwrap();
            let target = context.pane.unwrap();
            engine
                .execute(&mut context, &command("new-window", &["-n", "source"]))
                .unwrap();
            let source = context.pane.unwrap();
            engine
                .execute(&mut context, &command("new-window", &["-n", "survivor"]))
                .unwrap();
            engine
                .execute(
                    &mut context,
                    &command(
                        "set-option",
                        &["-g", "base-index", &MAX_BASE_INDEX.to_string()],
                    ),
                )
                .unwrap();
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-g", "renumber-windows", "on"]),
                )
                .unwrap();
            let snapshot = engine.state.snapshot();
            let generation = engine.state.generation();
            let original_context = context.clone();

            let error = engine
                .execute(
                    &mut context,
                    &command(
                        name,
                        &["-s", &source.to_string(), "-t", &target.to_string()],
                    ),
                )
                .unwrap_err();
            assert!(matches!(error, ServerError::InvalidCommand(message)
                if message == "no free window index"));
            assert_eq!(engine.state.snapshot(), snapshot, "{name}");
            assert_eq!(engine.state.generation(), generation, "{name}");
            assert_eq!(context, original_context, "{name}");
            assert!(engine.state.validate().is_ok(), "{name}");
        }
    }

    #[test]
    fn kill_commands_preflight_post_close_renumber_capacity() {
        for name in ["kill-window", "kill-pane"] {
            let mut engine = MuxEngine::default();
            let mut context = ExecutionContext::default();
            engine
                .execute(&mut context, &command("new-session", &["-s", "work"]))
                .unwrap();
            engine
                .execute(&mut context, &command("new-window", &["-n", "target"]))
                .unwrap();
            let target = if name == "kill-window" {
                context.window.unwrap().to_string()
            } else {
                context.pane.unwrap().to_string()
            };
            engine
                .execute(&mut context, &command("new-window", &["-n", "survivor"]))
                .unwrap();
            engine
                .execute(
                    &mut context,
                    &command(
                        "set-option",
                        &["-g", "base-index", &MAX_BASE_INDEX.to_string()],
                    ),
                )
                .unwrap();
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-g", "renumber-windows", "on"]),
                )
                .unwrap();
            let snapshot = engine.state.snapshot();
            let generation = engine.state.generation();
            let original_context = context.clone();

            let error = engine
                .execute(&mut context, &command(name, &["-t", &target]))
                .unwrap_err();
            assert!(matches!(error, ServerError::InvalidCommand(message)
                if message == "no free window index"));
            assert_eq!(engine.state.snapshot(), snapshot, "{name}");
            assert_eq!(engine.state.generation(), generation, "{name}");
            assert_eq!(context, original_context, "{name}");
            assert!(engine.state.validate().is_ok(), "{name}");
        }
    }

    #[test]
    fn join_pane_before_keeps_tmux_list_order_after_the_target() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let target = context.pane.unwrap();
        let target_window = context.window.unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "source"]))
            .unwrap();
        let source = context.pane.unwrap();

        engine
            .execute(
                &mut context,
                &command(
                    "join-pane",
                    &[
                        "-b",
                        "-v",
                        "-s",
                        &source.to_string(),
                        "-t",
                        &target.to_string(),
                    ],
                ),
            )
            .unwrap();

        let window = &engine.state.windows[&target_window];
        assert_eq!(window.layout.panes_in_order(), [source, target]);
        assert_eq!(window.pane_order(), [target, source]);
        assert_eq!(window.layout.pane_geometry(source).unwrap().yoff, 0);
        assert_eq!(window.layout.pane_geometry(source).unwrap().sy, 12);
        assert_eq!(window.layout.pane_geometry(target).unwrap().yoff, 13);
        assert_eq!(window.layout.pane_geometry(target).unwrap().sy, 11);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn layout_commands_cycle_target_restore_and_validate_names() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.pane.unwrap();
        let window = context.window.unwrap();
        for _ in 0..3 {
            engine
                .execute(&mut context, &command("split-window", &["-h"]))
                .unwrap();
        }
        let panes = engine.state.windows[&window].panes.clone();

        let selected = engine
            .execute(
                &mut context,
                &command("selectl", &["-t", &first.to_string(), "even-h"]),
            )
            .unwrap();
        assert!(selected.effects.contains(&MuxEffect::SnapshotChanged));
        assert_eq!(engine.state.windows[&window].panes, panes);
        assert!(matches!(
            engine.state.windows[&window].layout.project(),
            zz_protocol::LayoutNode::Split {
                axis: Axis::Horizontal,
                ..
            }
        ));
        let horizontal = engine.state.windows[&window].layout.clone();

        engine
            .execute(
                &mut context,
                &command("nextl", &["-t", &window.to_string()]),
            )
            .unwrap();
        assert!(matches!(
            engine.state.windows[&window].layout.project(),
            zz_protocol::LayoutNode::Split {
                axis: Axis::Vertical,
                ..
            }
        ));
        engine
            .execute(
                &mut context,
                &command("prevl", &["-t", &window.to_string()]),
            )
            .unwrap();
        assert!(matches!(
            engine.state.windows[&window].layout.project(),
            zz_protocol::LayoutNode::Split {
                axis: Axis::Horizontal,
                ..
            }
        ));

        engine
            .execute(
                &mut context,
                &command("select-layout", &["-t", &window.to_string(), "tiled"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("select-layout", &["-o", "-t", &window.to_string()]),
            )
            .unwrap();
        assert_ne!(engine.state.windows[&window].layout, horizontal);
        assert!(matches!(
            engine.state.windows[&window].layout.project(),
            zz_protocol::LayoutNode::Split {
                axis: Axis::Horizontal,
                ..
            }
        ));
        assert_eq!(context.window, Some(window));
        assert!(engine.state.validate().is_ok());

        assert!(matches!(
            engine.execute(&mut context, &command("select-layout", &["main"])),
            Err(ServerError::InvalidCommand(message)) if message.contains("ambiguous")
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("select-layout", &["b25f,80x24,0,0{40x24,0,0,0}"]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "invalid layout: b25f,80x24,0,0{40x24,0,0,0}"
        ));
        assert!(matches!(
            engine.execute(&mut context, &command("next-layout", &["-n"])),
            Err(ServerError::InvalidCommand(message))
                if message == "next-layout does not support -n"
        ));
    }

    #[test]
    fn select_layout_applies_serialized_layouts_and_reports_window_layout() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("split-window", &["-h", "-t", "w:0.0"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("split-window", &["-v", "-t", "w:0.1"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "select-layout",
                    &[
                        "-t",
                        "w:0",
                        "b78d,120x30,0,0{50x30,0,0,9,69x30,51,0[69x14,51,0,8,69x15,51,15,7]}",
                    ],
                ),
            )
            .unwrap();
        let first_dump = "6e85,120x30,0,0{50x30,0,0,0,69x30,51,0[69x14,51,0,1,69x15,51,15,2]}";
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-windows", &["-t", "w", "-F", "#{window_layout}"]),
                )
                .unwrap()
                .output,
            first_dump
        );

        engine
            .execute(&mut context, &command("kill-pane", &["-t", "w:0.1"]))
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "select-layout",
                    &[
                        "-t",
                        "w:0",
                        "e7f0,100x20,0,0[100x9,0,0{49x9,0,0,50,50x9,50,0,51},100x10,0,10,52]",
                    ],
                ),
            )
            .unwrap();
        let window = context.window.unwrap();
        assert_eq!(
            engine.state.windows[&window].layout.dump(),
            "eaf9,100x20,0,0{49x20,0,0,0,50x20,50,0,2}"
        );

        let before = engine.state.windows[&window].layout.clone();
        assert_eq!(
            engine.execute(
                &mut context,
                &command("select-layout", &["-t", "w:0", "0000,80x24,0,0,0"],),
            ),
            Err(ServerError::InvalidCommand(
                "invalid layout: 0000,80x24,0,0,0".to_owned()
            ))
        );
        assert_eq!(engine.state.windows[&window].layout, before);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn select_layout_restore_without_history_primes_the_next_restore() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let window = context.window.unwrap();
        let panes = engine.state.windows[&window].layout.panes_in_order();
        let original = panes
            .iter()
            .map(|pane| engine.state.windows[&window].layout.pane_geometry(*pane))
            .collect::<Vec<_>>();
        let generation = engine.state.generation();

        let first = engine
            .execute(&mut context, &command("select-layout", &["-o"]))
            .expect("first restore");
        assert!(first.effects.is_empty());
        assert_eq!(engine.state.generation(), generation);

        engine
            .execute(&mut context, &command("resize-pane", &["-x", "20"]))
            .unwrap();
        let resized = panes
            .iter()
            .map(|pane| engine.state.windows[&window].layout.pane_geometry(*pane))
            .collect::<Vec<_>>();
        assert_ne!(resized, original);
        let restored = engine
            .execute(&mut context, &command("select-layout", &["-o"]))
            .expect("second restore");
        let restored_geometry = panes
            .iter()
            .map(|pane| engine.state.windows[&window].layout.pane_geometry(*pane))
            .collect::<Vec<_>>();
        assert_eq!(restored_geometry, original);
        assert_eq!(restored.effects, [MuxEffect::SnapshotChanged]);
    }

    #[test]
    fn select_layout_unzooms_before_success_or_failure() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let window = context.window.unwrap();

        engine
            .execute(&mut context, &command("resize-pane", &["-Z"]))
            .unwrap();
        let layout = engine.state.windows[&window].layout.clone();
        let generation = engine.state.generation();
        assert!(matches!(
            engine.execute(&mut context, &command("select-layout", &["bogus"])),
            Err(ServerError::InvalidCommand(message)) if message == "invalid layout: bogus"
        ));
        assert_eq!(engine.state.windows[&window].layout, layout);
        assert_eq!(engine.state.windows[&window].zoomed_pane, None);
        assert_eq!(engine.state.generation(), generation + 1);

        engine
            .execute(&mut context, &command("resize-pane", &["-Z"]))
            .unwrap();
        let generation = engine.state.generation();
        let execution = engine
            .execute(&mut context, &command("select-layout", &[]))
            .unwrap();
        assert_eq!(engine.state.windows[&window].zoomed_pane, None);
        assert_eq!(engine.state.generation(), generation + 1);
        assert_eq!(execution.effects, [MuxEffect::SnapshotChanged]);
    }

    #[test]
    fn resize_pane_unzooms_before_parse_failure() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let window = context.window.unwrap();
        engine
            .execute(&mut context, &command("resize-pane", &["-Z"]))
            .unwrap();
        let layout = engine.state.windows[&window].layout.clone();
        let generation = engine.state.generation();

        assert!(matches!(
            engine.execute(&mut context, &command("resize-pane", &["-x", "abc"])),
            Err(ServerError::InvalidCommand(_))
        ));
        assert_eq!(engine.state.windows[&window].zoomed_pane, None);
        assert_eq!(engine.state.windows[&window].layout, layout);
        assert_eq!(engine.state.generation(), generation + 1);
    }

    #[test]
    fn select_layout_resolves_window_typed_session_targets() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let window = context.window.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-v"]))
            .unwrap();
        assert!(matches!(
            engine.state.windows[&window].layout.project(),
            zz_protocol::LayoutNode::Split {
                axis: Axis::Vertical,
                ..
            }
        ));

        for (target, layout, axis) in [
            ("work", "even-horizontal", Axis::Horizontal),
            ("work:", "even-vertical", Axis::Vertical),
            ("", "even-horizontal", Axis::Horizontal),
        ] {
            engine
                .execute(
                    &mut context,
                    &command("select-layout", &["-t", target, layout]),
                )
                .unwrap();
            assert!(matches!(
                engine.state.windows[&window].layout.project(),
                zz_protocol::LayoutNode::Split {
                    axis: actual,
                    ..
                } if actual == axis
            ));
        }
    }

    #[test]
    fn move_window_supports_insertion_replacement_and_cross_session_moves() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "alpha"]))
            .unwrap();
        let alpha = context.session.unwrap();
        let main = context.window.unwrap();
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "alpha:1", "-n", "target"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "alpha:2", "-n", "moving"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "alpha:5", "-n", "spare"]),
            )
            .unwrap();
        let target = engine.state.window_named(alpha, "target").unwrap();
        let moving = engine.state.window_named(alpha, "moving").unwrap();
        let spare = engine.state.window_named(alpha, "spare").unwrap();
        let target_pane = engine.state.windows[&target].active_pane;

        let snapshot = engine.state.snapshot();
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("move-window", &["-s", "alpha:spare", "-t", "alpha:2"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "index in use: 2"
        ));
        assert_eq!(engine.state.snapshot(), snapshot);

        engine
            .execute(
                &mut context,
                &command(
                    "move-window",
                    &["-a", "-d", "-s", "alpha:spare", "-t", "alpha:target"],
                ),
            )
            .unwrap();
        assert_eq!(
            window_layout(&engine, alpha),
            ["0:0", "1:target", "2:spare", "3:moving"]
        );
        assert_eq!(engine.state.sessions[&alpha].active_window, main);

        engine
            .execute(
                &mut context,
                &command(
                    "move-window",
                    &["-b", "-s", "alpha:moving", "-t", "alpha:target"],
                ),
            )
            .unwrap();
        assert_eq!(
            window_layout(&engine, alpha),
            ["0:0", "1:moving", "2:target", "3:spare"]
        );
        assert_eq!(context.window, Some(moving));
        assert_eq!(engine.state.sessions[&alpha].active_window, moving);

        let replaced = engine
            .execute(
                &mut context,
                &command(
                    "move-window",
                    &["-d", "-k", "-s", "alpha:spare", "-t", "alpha:2"],
                ),
            )
            .unwrap();
        assert!(
            replaced
                .effects
                .contains(&MuxEffect::PanesRemoved(vec![target_pane]))
        );
        assert!(!engine.state.windows.contains_key(&target));
        assert_eq!(
            window_layout(&engine, alpha),
            ["0:0", "1:moving", "2:spare"]
        );

        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "beta", "-n", "home"]),
            )
            .unwrap();
        let beta = session_named(&engine.state, "beta").unwrap();
        let spare_pane = engine.state.windows[&spare].active_pane;
        let moved = engine
            .execute(
                &mut context,
                &command("move-window", &["-d", "-s", "alpha:spare", "-t", "beta:4"]),
            )
            .unwrap();
        assert!(moved.effects.contains(&MuxEffect::PaneRelocated {
            pane: spare_pane,
            from: alpha,
            to: beta,
        }));
        assert_eq!(engine.state.windows[&spare].session, beta);
        assert_eq!(engine.state.windows[&spare].index, 4);
        assert_eq!(engine.state.sessions[&alpha].active_window, moving);
        assert_eq!(engine.state.windows[&main].session, alpha);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn move_window_r_still_resolves_a_bogus_source_and_mutates_nothing() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "rn3"]))
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "rn3:3", "-n", "a3"]),
            )
            .unwrap();

        let error = engine
            .execute(
                &mut context,
                &command("move-window", &["-r", "-s", "rn3:99", "-t", "rn3"]),
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            ServerError::WindowNotFound("99".to_owned()).to_string()
        );
        let indexes = engine
            .execute(
                &mut context,
                &command("list-windows", &["-t", "rn3", "-F", "#{window_index}"]),
            )
            .unwrap()
            .output;
        assert_eq!(indexes, "0\n3");
    }

    #[test]
    fn move_window_renumbers_from_base_index_and_preflights_option_compaction() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "base-index", "1"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "renumber-windows", "on"]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let work = context.session.unwrap();
        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "work:3", "-n", "three"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("new-window", &["-t", "work:5", "-n", "five"]),
            )
            .unwrap();
        let five = context.window.unwrap();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "remote"]),
            )
            .unwrap();
        let remote = session_named(&engine.state, "remote").unwrap();
        engine
            .execute(
                &mut context,
                &command("select-window", &["-t", "work:five"]),
            )
            .unwrap();

        engine
            .execute(
                &mut context,
                &command("move-window", &["-d", "-t", "remote:4"]),
            )
            .unwrap();
        assert_eq!(engine.state.windows[&five].session, remote);
        assert_eq!(engine.state.windows[&five].index, 4);
        assert_eq!(window_layout(&engine, work), ["1:1", "2:three"]);

        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "work:9", "-n", "nine"]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("move-window", &["-r", "-t", "work"]))
            .unwrap();
        assert_eq!(window_layout(&engine, work), ["1:1", "2:three", "3:nine"]);

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "base-index", "2147483647"]),
            )
            .unwrap();
        let snapshot = engine.state.snapshot();
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("move-window", &["-t", "remote:7"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "no free window index"
        ));
        assert_eq!(engine.state.snapshot(), snapshot);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn swap_window_exchanges_slots_and_dash_d_selects_destination_slots() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-s", "alpha", "-n", "first"]),
            )
            .unwrap();
        let alpha = context.session.unwrap();
        let first = context.window.unwrap();
        let first_pane = context.pane.unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .unwrap();
        let second = context.window.unwrap();
        let second_pane = context.pane.unwrap();
        engine
            .execute(
                &mut context,
                &command("select-window", &["-t", "alpha:first"]),
            )
            .unwrap();

        engine
            .execute(
                &mut context,
                &command("swap-window", &["-s", "alpha:first", "-t", "alpha:second"]),
            )
            .unwrap();
        assert_eq!(engine.state.windows[&first].index, 1);
        assert_eq!(engine.state.windows[&second].index, 0);
        assert_eq!(engine.state.sessions[&alpha].active_window, second);
        assert_eq!(engine.state.windows[&first].active_pane, first_pane);
        assert_eq!(engine.state.windows[&second].active_pane, second_pane);

        engine
            .execute(
                &mut context,
                &command(
                    "swap-window",
                    &["-d", "-s", "alpha:first", "-t", "alpha:second"],
                ),
            )
            .unwrap();
        assert_eq!(engine.state.windows[&first].index, 0);
        assert_eq!(engine.state.windows[&second].index, 1);
        assert_eq!(engine.state.sessions[&alpha].active_window, first);

        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "beta", "-n", "third"]),
            )
            .unwrap();
        let beta = session_named(&engine.state, "beta").unwrap();
        let third = engine.state.window_named(beta, "third").unwrap();
        let third_pane = engine.state.windows[&third].active_pane;
        let swapped = engine
            .execute(
                &mut context,
                &command("swap-window", &["-s", "alpha:first", "-t", "beta:third"]),
            )
            .unwrap();
        assert!(swapped.effects.contains(&MuxEffect::PaneRelocated {
            pane: first_pane,
            from: alpha,
            to: beta,
        }));
        assert!(swapped.effects.contains(&MuxEffect::PaneRelocated {
            pane: third_pane,
            from: beta,
            to: alpha,
        }));
        assert_eq!(engine.state.windows[&first].session, beta);
        assert_eq!(engine.state.windows[&third].session, alpha);
        assert_eq!(engine.state.sessions[&alpha].active_window, third);
        assert_eq!(engine.state.sessions[&beta].active_window, first);

        engine
            .execute(
                &mut context,
                &command(
                    "swap-window",
                    &["-d", "-s", &first.to_string(), "-t", &third.to_string()],
                ),
            )
            .unwrap();
        assert_eq!(engine.state.windows[&first].session, alpha);
        assert_eq!(engine.state.windows[&third].session, beta);
        assert_eq!(engine.state.sessions[&alpha].active_window, first);
        assert_eq!(engine.state.sessions[&beta].active_window, third);
        assert!(engine.state.validate().is_ok());
    }

    #[test]
    fn gap_queries_are_silent_sorted_and_catalog_backed() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "w"]))
            .unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "B"]))
            .unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "A"]))
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-d", "-t", "B:0"]))
            .unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-d", "-t", "B:2"]))
            .unwrap();

        let windows = engine
            .execute(
                &mut context,
                &command(
                    "list-windows",
                    &["-a", "-F", "#{session_name}:#{window_index}"],
                ),
            )
            .unwrap();
        assert_eq!(windows.output, "A:0\nB:0\nB:2\nw:0");
        let panes = engine
            .execute(
                &mut context,
                &command(
                    "list-panes",
                    &["-a", "-F", "#{session_name}:#{window_index}.#{pane_index}"],
                ),
            )
            .unwrap();
        assert_eq!(panes.output, "A:0.0\nB:0.0\nB:0.1\nB:2.0\nw:0.0");
        let session_panes = engine
            .execute(
                &mut context,
                &command(
                    "list-panes",
                    &[
                        "-s",
                        "-t",
                        "B:0",
                        "-F",
                        "#{session_name}:#{window_index}.#{pane_index}",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(session_panes.output, "B:0.0\nB:0.1\nB:2.0");

        for pattern in ["ma*", "zzz*"] {
            let found = engine
                .execute(
                    &mut context,
                    &command("find-window", &["-CiNrTZ", "-t", "w:0", pattern]),
                )
                .unwrap();
            assert_eq!(found, Execution::default());
        }

        let row = engine
            .execute(&mut context, &command("list-commands", &["move-window"]))
            .unwrap();
        assert_eq!(
            row.output,
            "move-window (movew) [-abdkr] [-s src-window] [-t dst-window]"
        );
        let formatted = engine
            .execute(
                &mut context,
                &command(
                    "list-commands",
                    &[
                        "-F",
                        "#{command_list_name}|#{command_list_alias}|#{command_list_usage}",
                        "start",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(formatted.output, "start-server|start|");
        let commands = engine
            .execute(&mut context, &command("list-commands", &[]))
            .unwrap();
        let rows = commands.output.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), COMMAND_SPECS.len() + DAEMON_COMMAND_SPECS.len());
        assert!(rows.windows(2).all(|rows| rows[0] < rows[1]));
        assert!(rows.contains(&"kill-server "));
        assert!(rows.contains(&"list-commands (lscm) [-F format] [command]"));
        assert!(rows.contains(&"start-server (start) "));
        assert!(rows.contains(
            &"capture-pane (capturep) [-aeJMNpqT] [-b buffer-name] [-E end-line] [-S start-line] [-t target-pane]"
        ));
        assert!(rows.contains(
            &"paste-buffer (pasteb) [-dprS] [-s separator] [-b buffer-name] [-t target-pane]"
        ));
        assert_eq!(
            engine
                .execute(&mut context, &command("list-commands", &["capturep"]))
                .unwrap()
                .output,
            "capture-pane (capturep) [-aeJMNpqT] [-b buffer-name] [-E end-line] [-S start-line] [-t target-pane]"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("start", &[]))
                .unwrap(),
            Execution::default()
        );
    }

    #[test]
    fn relative_pane_targets_and_window_rotation_follow_canonical_pane_order() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.pane.unwrap();
        let window = context.window.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        engine
            .execute(
                &mut context,
                &command("split-browser", &["-h", "https://rotate.example"]),
            )
            .unwrap();
        let browser = context.pane.unwrap();
        assert_eq!(
            engine.state.windows[&window].pane_order(),
            [first, second, browser]
        );

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string()]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("selectp", &["-t:.+"]))
            .unwrap();
        assert_eq!(context.pane, Some(second));
        engine
            .execute(&mut context, &command("select-pane", &["-t:.-"]))
            .unwrap();
        assert_eq!(context.pane, Some(first));
        engine
            .execute(&mut context, &command("select-pane", &["-l"]))
            .unwrap();
        assert_eq!(context.pane, Some(second));

        let rotated = engine
            .execute(&mut context, &command("rotatew", &[]))
            .unwrap();
        assert!(rotated.effects.contains(&MuxEffect::SnapshotChanged));
        assert_eq!(
            engine.state.windows[&window].pane_order(),
            [second, browser, first]
        );
        assert_eq!(context.pane, Some(browser));

        engine
            .execute(&mut context, &command("resize-pane", &["-Z"]))
            .unwrap();
        engine
            .execute(&mut context, &command("rotate-window", &["-D", "-Z"]))
            .unwrap();
        assert_eq!(
            engine.state.windows[&window].pane_order(),
            [first, second, browser]
        );
        assert_eq!(context.pane, Some(second));
        assert_eq!(engine.state.windows[&window].zoomed_pane, Some(second));
        assert!(engine.state.validate().is_ok());

        assert!(matches!(
            engine.execute(&mut context, &command("rotate-window", &["-d"])),
            Err(ServerError::InvalidCommand(message))
                if message == "rotate-window does not support -d"
        ));
        assert!(matches!(
            engine.execute(&mut context, &command("select-pane", &["-m"])),
            Err(ServerError::UnsupportedCommand(message))
                if message == "select-pane -m"
        ));
    }
}
