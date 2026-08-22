use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::PathBuf,
    str::FromStr as _,
};

use zz_protocol::{
    AgentDescriptor, AgentProvider, Axis, BrowserDescriptor, COMMAND_SPECS, ChooseTreeKind,
    CommandInvocation, CommandResolution, CommandSpec, DAEMON_COMMAND_SPECS,
    DEFAULT_AGENT_AUTO_APPROVE, DEFAULT_AGENT_CLAUDE_CODE_COMMAND, DEFAULT_AGENT_COMMAND,
    DEFAULT_BROWSER_PROFILE, EditorDescriptor, KeyToken, MAX_AGENT_COMMAND_BYTES,
    MAX_GUI_TEXT_BYTES, MuxOptionKey, PaneId, PaneKindSnapshot, PopupBorderLines, ServerError,
    SessionId, TerminalUiCommand, WindowId, canonical_key, normalize_browser_profile_name,
    resolve_command,
};
use zz_terminal::{
    CopyJump, CopyJumpDirection, CopyModeAction, CopyModeCopy, DEFAULT_HISTORY_LIMIT,
    DEFAULT_WORD_SEPARATORS, MAX_HISTORY_LIMIT, PasteBufferAction, SearchDirection,
    TerminalViewAction,
};

use crate::{
    BellAction, Binding, KeyTables, LayoutPreset, MuxState, PaneDirection, PaneKind, PresetOptions,
    SplitPlacement, SplitSize as LayoutSplitSize, StatusContext, StatusFormats, StatusOption,
    TmuxSort, TmuxSortOrder, VisualBell, WindowSize, WindowStatusFormats, WindowStatusOption,
    canonical_command, command_spec,
    formats::{
        CommandHooks, FormatContext, FormatType, StatusHooks, expand_format_time_with_hooks,
        expand_format_with_hooks, format_true, parse_tmux_colour,
    },
    honest_knobs::{
        AllowPassthrough, PaneOption, PaneOptions, ServerOption, ServerOptions, SessionOption,
        SessionOptions, WindowOption, WindowOptions,
    },
    layout::PANE_MAXIMUM,
    model::DEFAULT_WINDOW_EXTENT,
    tmux_options::{
        HOOK_NAMES, TmuxArrayValue, TmuxOption, TmuxOptionScope, TmuxStoredScalarKind,
        UPDATE_ENVIRONMENT_DEFAULT, match_tmux_option, parse_tmux_option, tmux_option_is_hook,
        tmux_option_table_order, tmux_options, tmux_stored_array, tmux_stored_scalar,
    },
    valid_style,
};

#[cfg(test)]
use crate::tmux_options::{
    MESSAGE_COMMAND_STYLE_DEFAULT, MESSAGE_FORMAT_DEFAULT, MESSAGE_STYLE_DEFAULT,
    PANE_SCROLLBARS_STYLE_DEFAULT,
};

const MAX_COPY_COMMAND_BYTES: usize = 8 * 1024;
const MAX_COMMAND_PROMPT_LABEL_BYTES: usize = 1024;
const MAX_COMMAND_PROMPT_TEMPLATE_BYTES: usize = 8 * 1024;
const DEFAULT_DISPLAY_MESSAGE: &str =
    "[#{session_name}] #{window_index}:#{window_name}, current pane #{pane_index}";
const DEFAULT_NEW_SESSION_FORMAT: &str = "#{session_name}:";
const DEFAULT_PANE_CREATION_FORMAT: &str = "#{session_name}:#{window_index}.#{pane_index}";
const DEFAULT_LIST_COMMANDS_FORMAT: &str =
    "#{command_list_name}#{?command_list_alias, (#{command_list_alias}),} #{command_list_usage}";
const DEFAULT_LIST_SESSIONS_FORMAT: &str = concat!(
    "#{session_name}: #{session_windows} windows (created #{t:session_created})",
    "#{?session_grouped, (group ,}#{session_group}#{?session_grouped,),}",
    "#{?session_attached, (attached),}",
);
const DEFAULT_LIST_WINDOWS_FORMAT: &str = concat!(
    "#{window_index}: #{window_name}#{window_raw_flags} (#{window_panes} panes) ",
    "[#{window_width}x#{window_height}] [layout #{window_layout}] #{window_id}",
    "#{?window_active, (active),}",
);
const DEFAULT_LIST_WINDOWS_WITH_SESSION_FORMAT: &str = concat!(
    "#{session_name}:#{window_index}: #{window_name}#{window_raw_flags} ",
    "(#{window_panes} panes) [#{window_width}x#{window_height}] ",
);
const DEFAULT_LIST_PANES_FORMAT: &str = concat!(
    "#{pane_index}: [#{pane_width}x#{pane_height}",
    "#{?pane_floating_flag, #{pane_x}#,#{pane_y}#,#{pane_z}}] ",
    "[history #{history_size}/#{history_limit}, #{history_bytes} bytes] #{pane_id}",
    "#{?pane_active, (active),}#{?pane_dead, (dead),}",
);
const DEFAULT_LIST_PANES_WITH_SESSION_FORMAT: &str = concat!(
    "#{window_index}.#{pane_index}: [#{pane_width}x#{pane_height}",
    "#{?pane_floating_flag, #{pane_x}#,#{pane_y}#,#{pane_z}}] ",
    "[history #{history_size}/#{history_limit}, #{history_bytes} bytes] #{pane_id}",
    "#{?pane_active, (active),}#{?pane_dead, (dead),}",
);
const DEFAULT_LIST_PANES_WITH_SERVER_FORMAT: &str = concat!(
    "#{session_name}:#{window_index}.#{pane_index}: [#{pane_width}x#{pane_height}",
    "#{?pane_floating_flag, #{pane_x}#,#{pane_y}#,#{pane_z}}] ",
    "[history #{history_size}/#{history_limit}, #{history_bytes} bytes] #{pane_id}",
    "#{?pane_active, (active),}#{?pane_dead, (dead),}",
);
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
const DEFAULT_SHELL: &str = "/bin/sh";
const DEFAULT_TERMINAL: &str = "tmux-256color";
const DEFAULT_DISPLAY_TIME_MS: u32 = 750;
const DEFAULT_INITIAL_REPEAT_TIME_MS: u32 = 0;
const DEFAULT_REPEAT_TIME_MS: u32 = 500;
const MAX_REPEAT_TIME_MS: u32 = 2_000_000;
pub const MAX_WORD_SEPARATORS_BYTES: usize = 8 * 1024;

#[must_use]
pub fn if_shell_truthy(value: &str) -> bool {
    value.as_bytes().first().is_some_and(|first| *first != b'0')
}

struct RowFormatHooks<'a, H> {
    inner: &'a mut H,
    line: usize,
}

impl<H: StatusHooks> StatusHooks for RowFormatHooks<'_, H> {
    fn strftime(&mut self, literal: &str) -> String {
        self.inner.strftime(literal)
    }

    fn shell(&mut self, command: &str) -> String {
        self.inner.shell(command)
    }

    fn variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
        if name == "line" {
            Some(self.line.to_string())
        } else {
            self.inner.variable(name, context)
        }
    }

    fn session_activity(&mut self, session: SessionId) -> u64 {
        self.inner.session_activity(session)
    }

    fn window_activity(&mut self, window: WindowId) -> u64 {
        self.inner.window_activity(window)
    }

    fn pane_activity(&mut self, pane: PaneId) -> u64 {
        self.inner.pane_activity(pane)
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionContext {
    pub session: Option<SessionId>,
    pub window: Option<WindowId>,
    pub pane: Option<PaneId>,
    client_terminal: ClientTerminal,
    client_size: Option<(u16, u16)>,
    repeat_binding: bool,
    pub no_hooks: bool,
    pub format_variables: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ClientTerminal {
    NoClient,
    Absent,
    #[default]
    Present,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            session: None,
            window: None,
            pane: None,
            client_terminal: ClientTerminal::Present,
            client_size: None,
            repeat_binding: false,
            no_hooks: false,
            format_variables: BTreeMap::new(),
        }
    }
}

impl ExecutionContext {
    #[must_use]
    pub fn new(session: Option<SessionId>, window: Option<WindowId>, pane: Option<PaneId>) -> Self {
        Self {
            session,
            window,
            pane,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn for_pane(state: &MuxState, pane: PaneId) -> Option<Self> {
        let window = state.window_for_pane(pane)?;
        let session = state.windows.get(&window)?.session;
        Some(Self::new(Some(session), Some(window), Some(pane)))
    }

    pub fn retarget(&mut self, target: &Self) {
        self.session = target.session;
        self.window = target.window;
        self.pane = target.pane;
    }

    #[must_use]
    pub fn has_client_terminal(&self) -> bool {
        self.client_terminal == ClientTerminal::Present
    }

    #[must_use]
    pub fn has_no_client(&self) -> bool {
        self.client_terminal == ClientTerminal::NoClient
    }

    pub fn set_client_terminal(&mut self, present: bool) {
        self.client_terminal = if present {
            ClientTerminal::Present
        } else {
            ClientTerminal::Absent
        };
    }

    pub fn set_no_client(&mut self) {
        self.client_terminal = ClientTerminal::NoClient;
    }

    #[must_use]
    pub fn client_size(&self) -> Option<(u16, u16)> {
        self.client_size
    }

    pub fn set_client_size(&mut self, size: Option<(u16, u16)>) {
        self.client_size = size;
    }

    #[must_use]
    pub fn repeat_binding(&self) -> bool {
        self.repeat_binding
    }

    pub fn set_repeat_binding(&mut self, repeat: bool) {
        self.repeat_binding = repeat;
    }

    pub fn retarget_to_pane(&mut self, state: &MuxState, pane: PaneId) -> bool {
        let Some(target) = Self::for_pane(state, pane) else {
            return false;
        };
        self.retarget(&target);
        true
    }

    pub fn enter_hook(&mut self, variables: BTreeMap<String, String>) {
        self.no_hooks = true;
        self.format_variables = variables;
    }

    #[must_use]
    pub fn format_variables(&self) -> Option<&BTreeMap<String, String>> {
        (!self.format_variables.is_empty()).then_some(&self.format_variables)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MuxEffect {
    PaneCreated {
        pane: PaneId,
        kind: PaneKindSnapshot,
        inherit_cwd_from: Option<PaneId>,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    },
    PaneMaterialized {
        pane: PaneId,
        kind: PaneKindSnapshot,
        inherit_cwd_from: Option<PaneId>,
        cwd: Option<String>,
        command: Option<Vec<String>>,
    },
    PaneRespawned {
        pane: PaneId,
        cwd: Option<String>,
        command: Option<Vec<String>>,
        environment: Vec<(String, String)>,
        empty: bool,
    },
    PaneFormatOutput {
        pane: PaneId,
        format: String,
        active_session: Option<SessionId>,
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
        sessions_only: bool,
        filter: Option<String>,
        sort: TmuxSort,
    },
    FocusSidebar {
        pane: PaneId,
    },
    ChooseBuffer {
        pane: PaneId,
        filter: Option<String>,
        sort: TmuxSort,
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
    AggressiveResizeChanged {
        window: Option<WindowId>,
    },
    WindowSizeChanged {
        window: Option<WindowId>,
    },
    TerminalKnobsChanged {
        window: Option<WindowId>,
        pane: Option<PaneId>,
    },
    /// `session` scopes a session-effective write; `None` is a global write.
    MuxOptionChanged {
        option: MuxOptionKey,
        session: Option<SessionId>,
    },
    AgentPaneRestart {
        pane: PaneId,
    },
    StatusFormatsChanged {
        session: Option<SessionId>,
    },
    Attach {
        session: SessionId,
        detach_others: bool,
        read_only: bool,
    },
    Detach(DetachScope),
    SourceFile {
        path: String,
        quiet: bool,
    },
    RunHook {
        name: String,
        commands: Vec<Vec<CommandInvocation>>,
        context: ExecutionContext,
    },
    ReloadConfig,
    KillServer,
    SuppressAfterHook,
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

struct ListKeyHooks<'a, H> {
    inner: &'a mut H,
    table: &'a str,
    key: &'a str,
    binding: &'a Binding,
    prefix: &'a str,
}

struct ConfigConditionHooks<'a> {
    engine: &'a MuxEngine,
    inner: CommandHooks,
}

impl StatusHooks for ConfigConditionHooks<'_> {
    fn strftime(&mut self, literal: &str) -> String {
        self.inner.strftime(literal)
    }

    fn shell(&mut self, _command: &str) -> String {
        String::new()
    }

    fn variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
        let option = if name.starts_with('@') {
            self.engine
                .server_user_options
                .get(name)
                .or_else(|| self.engine.global_window_user_options.get(name))
                .or_else(|| self.engine.global_session_user_options.get(name))
                .cloned()
        } else {
            self.engine.global_tmux_option_value(name)
        };
        option.or_else(|| {
            context.variable(name).is_none().then(|| {
                self.engine
                    .global_environment
                    .get(name)
                    .and_then(|entry| entry.value.clone())
            })?
        })
    }
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

impl<H: StatusHooks> StatusHooks for ListKeyHooks<'_, H> {
    fn strftime(&mut self, literal: &str) -> String {
        self.inner.strftime(literal)
    }

    fn shell(&mut self, command: &str) -> String {
        self.inner.shell(command)
    }

    fn variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
        match name {
            "key_repeat" => Some(if self.binding.repeat { "1" } else { "0" }.to_owned()),
            "key_note" => Some(self.binding.note.clone().unwrap_or_default()),
            "key_prefix" => Some(self.prefix.to_owned()),
            "key_table" => Some(self.table.to_owned()),
            "key_string" => Some(self.key.to_owned()),
            "key_command" => Some(format_key_command(self.binding)),
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

    const fn toggled(self) -> Self {
        match self {
            Self::Off => Self::External,
            Self::External => Self::Off,
            Self::On => Self::On,
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ArrayIndex {
    Numeric(u32),
    Named(String),
}

impl ArrayIndex {
    fn parse(value: String) -> Self {
        value
            .parse()
            .map_or_else(|_| Self::Named(value), Self::Numeric)
    }

    fn display(&self) -> String {
        match self {
            Self::Numeric(index) => index.to_string(),
            Self::Named(index) => index.clone(),
        }
    }
}

type StringArray = BTreeMap<ArrayIndex, String>;
type ArrayTable = BTreeMap<&'static str, StringArray>;
type HookArray = BTreeMap<ArrayIndex, Vec<CommandInvocation>>;
type HookTable = BTreeMap<String, HookArray>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredArrays {
    server: ArrayTable,
    global_session: ArrayTable,
    sessions: BTreeMap<SessionId, ArrayTable>,
    global_window: ArrayTable,
    windows: BTreeMap<WindowId, ArrayTable>,
    panes: BTreeMap<PaneId, ArrayTable>,
}

type ScalarTable = BTreeMap<&'static str, String>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct StoredScalars {
    server: ScalarTable,
    global_session: ScalarTable,
    sessions: BTreeMap<SessionId, ScalarTable>,
    global_window: ScalarTable,
    windows: BTreeMap<WindowId, ScalarTable>,
    panes: BTreeMap<PaneId, ScalarTable>,
}

impl Default for StoredArrays {
    fn default() -> Self {
        Self {
            server: default_array_table(TmuxOptionScope::Server),
            global_session: default_array_table(TmuxOptionScope::Session),
            sessions: BTreeMap::new(),
            global_window: default_array_table(TmuxOptionScope::WindowPane),
            windows: BTreeMap::new(),
            panes: BTreeMap::new(),
        }
    }
}

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

    const fn toggled(self) -> Self {
        match self {
            Self::Off => Self::On,
            Self::On => Self::Off,
            Self::Failed => Self::Failed,
            Self::Key => Self::Key,
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
    global_popup_style: String,
    window_popup_styles: BTreeMap<WindowId, String>,
    global_popup_border_style: String,
    window_popup_border_styles: BTreeMap<WindowId, String>,
    global_popup_border_lines: PopupBorderLines,
    window_popup_border_lines: BTreeMap<WindowId, PopupBorderLines>,
    global_menu_style: String,
    window_menu_styles: BTreeMap<WindowId, String>,
    global_menu_selected_style: String,
    window_menu_selected_styles: BTreeMap<WindowId, String>,
    global_menu_border_style: String,
    window_menu_border_styles: BTreeMap<WindowId, String>,
    global_menu_border_lines: PopupBorderLines,
    window_menu_border_lines: BTreeMap<WindowId, PopupBorderLines>,
    global_lock_command: String,
    session_lock_commands: BTreeMap<SessionId, String>,
    global_lock_after_time: u32,
    session_lock_after_times: BTreeMap<SessionId, u32>,
    global_default_command: String,
    session_default_commands: BTreeMap<SessionId, String>,
    global_default_shell: String,
    session_default_shells: BTreeMap<SessionId, String>,
    default_terminal: Option<String>,
    global_display_time_ms: u32,
    session_display_time_ms: BTreeMap<SessionId, u32>,
    global_initial_repeat_time_ms: u32,
    session_initial_repeat_time_ms: BTreeMap<SessionId, u32>,
    global_repeat_time_ms: u32,
    session_repeat_time_ms: BTreeMap<SessionId, u32>,
    server_user_options: UserOptions,
    global_session_user_options: UserOptions,
    session_user_options: BTreeMap<SessionId, UserOptions>,
    global_window_user_options: UserOptions,
    window_user_options: BTreeMap<WindowId, UserOptions>,
    pane_user_options: BTreeMap<PaneId, UserOptions>,
    stored_arrays: StoredArrays,
    stored_scalars: StoredScalars,
    global_hooks: HookTable,
    session_hooks: BTreeMap<SessionId, HookTable>,
    global_window_hooks: HookTable,
    window_hooks: BTreeMap<WindowId, HookTable>,
    pane_hooks: BTreeMap<PaneId, HookTable>,
    global_environment: Environment,
    session_environments: BTreeMap<SessionId, Environment>,
    status: StatusFormats,
    session_status_options: BTreeMap<SessionId, BTreeMap<StatusOption, String>>,
    explicit_status_options: BTreeSet<&'static str>,
    session_explicit_status_options: BTreeMap<SessionId, BTreeSet<&'static str>>,
    window_status: WindowStatusFormats,
    window_status_options: BTreeMap<WindowId, BTreeMap<WindowStatusOption, String>>,
    server_options: ServerOptions,
    global_session_options: SessionOptions,
    session_options: BTreeMap<SessionId, BTreeMap<SessionOption, String>>,
    global_window_options: WindowOptions,
    window_options: BTreeMap<WindowId, BTreeMap<WindowOption, String>>,
    global_pane_options: PaneOptions,
    window_pane_options: BTreeMap<WindowId, BTreeMap<PaneOption, String>>,
    pane_options: BTreeMap<PaneId, BTreeMap<PaneOption, String>>,
    format_host: String,
    format_host_short: String,
    format_pid: u32,
    format_socket_path: String,
    format_start_time: u64,
    format_now: u64,
    format_uid: String,
    format_user: String,
    pane_runtime_facts: BTreeMap<PaneId, PaneRuntimeFacts>,
    pane_start_commands: BTreeMap<PaneId, Vec<String>>,
    destroyed_sessions: Vec<(SessionId, String, String)>,
    experimental_agent_pane: bool,
    experimental_editor_pane: bool,
    agent: AgentOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneRuntimeFacts {
    pub current_command: String,
    pub current_path: String,
    pub dead_signal: String,
    pub reported_path: String,
    pub start_path: String,
    pub pid: Option<u32>,
    pub tty: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalWorkerOptions {
    pub allow_passthrough: bool,
    pub wrap_search: bool,
    pub cursor_style: &'static str,
    pub cursor_colour: String,
}

/// What an agent pane's daemon-owned adapter is started with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentOptions {
    pub command: String,
    pub claude_code_command: String,
    pub auto_approve: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PopupOptions {
    pub style: String,
    pub border_style: String,
    pub border_lines: PopupBorderLines,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuOptions {
    pub style: String,
    pub selected_style: String,
    pub border_style: String,
    pub border_lines: PopupBorderLines,
}

impl Default for PopupOptions {
    fn default() -> Self {
        Self {
            style: "bg=themedarkgrey,fg=themewhite".to_owned(),
            border_style: "bg=themedarkgrey,fg=themelightgrey".to_owned(),
            border_lines: PopupBorderLines::Single,
        }
    }
}

impl Default for MenuOptions {
    fn default() -> Self {
        Self {
            style: "bg=themedarkgrey,fg=themewhite".to_owned(),
            selected_style: "bg=themeyellow,fg=themeblack".to_owned(),
            border_style: "bg=themedarkgrey,fg=themelightgrey".to_owned(),
            border_lines: PopupBorderLines::Single,
        }
    }
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
            global_popup_style: PopupOptions::default().style,
            window_popup_styles: BTreeMap::new(),
            global_popup_border_style: PopupOptions::default().border_style,
            window_popup_border_styles: BTreeMap::new(),
            global_popup_border_lines: PopupBorderLines::Single,
            window_popup_border_lines: BTreeMap::new(),
            global_menu_style: MenuOptions::default().style,
            window_menu_styles: BTreeMap::new(),
            global_menu_selected_style: MenuOptions::default().selected_style,
            window_menu_selected_styles: BTreeMap::new(),
            global_menu_border_style: MenuOptions::default().border_style,
            window_menu_border_styles: BTreeMap::new(),
            global_menu_border_lines: PopupBorderLines::Single,
            window_menu_border_lines: BTreeMap::new(),
            global_lock_command: "lock -np".to_owned(),
            session_lock_commands: BTreeMap::new(),
            global_lock_after_time: 0,
            session_lock_after_times: BTreeMap::new(),
            global_default_command: String::new(),
            session_default_commands: BTreeMap::new(),
            global_default_shell: DEFAULT_SHELL.to_owned(),
            session_default_shells: BTreeMap::new(),
            default_terminal: None,
            global_display_time_ms: DEFAULT_DISPLAY_TIME_MS,
            session_display_time_ms: BTreeMap::new(),
            global_initial_repeat_time_ms: DEFAULT_INITIAL_REPEAT_TIME_MS,
            session_initial_repeat_time_ms: BTreeMap::new(),
            global_repeat_time_ms: DEFAULT_REPEAT_TIME_MS,
            session_repeat_time_ms: BTreeMap::new(),
            server_user_options: UserOptions::new(),
            global_session_user_options: UserOptions::new(),
            session_user_options: BTreeMap::new(),
            global_window_user_options: UserOptions::new(),
            window_user_options: BTreeMap::new(),
            pane_user_options: BTreeMap::new(),
            stored_arrays: StoredArrays::default(),
            stored_scalars: StoredScalars::default(),
            global_hooks: global_hook_table(TmuxOptionScope::Session),
            session_hooks: BTreeMap::new(),
            global_window_hooks: global_hook_table(TmuxOptionScope::Window),
            window_hooks: BTreeMap::new(),
            pane_hooks: BTreeMap::new(),
            global_environment: Environment::new(),
            session_environments: BTreeMap::new(),
            status: StatusFormats::default(),
            session_status_options: BTreeMap::new(),
            explicit_status_options: BTreeSet::new(),
            session_explicit_status_options: BTreeMap::new(),
            window_status: WindowStatusFormats::default(),
            window_status_options: BTreeMap::new(),
            server_options: ServerOptions::default(),
            global_session_options: SessionOptions::default(),
            session_options: BTreeMap::new(),
            global_window_options: WindowOptions::default(),
            window_options: BTreeMap::new(),
            global_pane_options: PaneOptions::default(),
            window_pane_options: BTreeMap::new(),
            pane_options: BTreeMap::new(),
            format_host: String::new(),
            format_host_short: String::new(),
            format_pid: 0,
            format_socket_path: String::new(),
            format_start_time: 0,
            format_now: 0,
            format_uid: String::new(),
            format_user: String::new(),
            pane_runtime_facts: BTreeMap::new(),
            pane_start_commands: BTreeMap::new(),
            destroyed_sessions: Vec::new(),
            experimental_agent_pane: false,
            experimental_editor_pane: false,
            agent: AgentOptions::default(),
        }
    }
}

impl MuxEngine {
    #[must_use]
    pub fn hook_commands(
        &self,
        session: Option<SessionId>,
        name: &str,
    ) -> Option<Vec<Vec<CommandInvocation>>> {
        let local = session
            .and_then(|session| self.session_hooks.get(&session))
            .and_then(|hooks| hooks.get(name));
        local
            .or_else(|| self.global_hooks.get(name))
            .map(|hooks| hooks.values().cloned().collect())
    }

    #[must_use]
    pub fn event_hook_commands(
        &self,
        context: &ExecutionContext,
        name: &str,
    ) -> Option<Vec<Vec<CommandInvocation>>> {
        let option = match_tmux_option(name).ok().flatten()?;
        if !tmux_option_is_hook(option.name) {
            return None;
        }
        if option.scope == TmuxOptionScope::Session {
            return self.hook_commands(context.session, option.name);
        }
        let window = context
            .pane
            .and_then(|pane| self.state.window_for_pane(pane))
            .or(context.window);
        let pane_hook = if option.scope == TmuxOptionScope::WindowPane {
            context
                .pane
                .and_then(|pane| self.pane_hooks.get(&pane))
                .and_then(|hooks| hooks.get(option.name))
        } else {
            None
        };
        pane_hook
            .or_else(|| {
                window
                    .and_then(|window| self.window_hooks.get(&window))
                    .and_then(|hooks| hooks.get(option.name))
            })
            .or_else(|| self.global_window_hooks.get(option.name))
            .map(|hooks| hooks.values().cloned().collect())
    }

    fn user_hook_commands(
        &self,
        context: &ExecutionContext,
        name: &str,
    ) -> Option<Vec<Vec<CommandInvocation>>> {
        let session = context
            .session
            .and_then(|session| {
                self.user_option_readback(TmuxOptionTarget::Session(session), name, true)
            })
            .map(|(value, _)| value);
        let pane = context
            .pane
            .and_then(|pane| self.user_option_readback(TmuxOptionTarget::Pane(pane), name, true))
            .map(|(value, _)| value);
        let window = context
            .pane
            .and_then(|pane| self.state.window_for_pane(pane))
            .or(context.window)
            .and_then(|window| {
                self.user_option_readback(TmuxOptionTarget::Window(window), name, true)
            })
            .map(|(value, _)| value);
        let commands = parse_hook_commands(self, session.or(pane).or(window)?).ok()?;
        Some(vec![commands])
    }

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

    #[must_use]
    pub fn status_formats_for_session(&self, session: Option<SessionId>) -> StatusFormats {
        let mut formats = self.status.clone();
        if let Some(overrides) =
            session.and_then(|session| self.session_status_options.get(&session))
        {
            for (option, value) in overrides {
                formats
                    .set(*option, Some(value))
                    .expect("stored status option was validated");
            }
        }
        formats
    }

    #[must_use]
    pub fn status_format_array_for_session(
        &self,
        session: Option<SessionId>,
    ) -> BTreeMap<u32, String> {
        let target = session.map_or(TmuxOptionTarget::GlobalSession, TmuxOptionTarget::Session);
        self.array_option_readback(target, "status-format", true)
            .map(|(array, _)| {
                array
                    .iter()
                    .filter_map(|(index, value)| match index {
                        ArrayIndex::Numeric(index) => Some((*index, value.clone())),
                        ArrayIndex::Named(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn status_row_variables_for_session(
        &self,
        session: Option<SessionId>,
    ) -> BTreeMap<String, String> {
        let formats = self.status_formats_for_session(session);
        let mut variables = BTreeMap::new();
        for option in [
            StatusOption::Enabled,
            StatusOption::Background,
            StatusOption::Foreground,
            StatusOption::Interval,
            StatusOption::Justify,
            StatusOption::Left,
            StatusOption::LeftLength,
            StatusOption::LeftStyle,
            StatusOption::Position,
            StatusOption::Right,
            StatusOption::RightLength,
            StatusOption::RightStyle,
            StatusOption::Style,
        ] {
            variables.insert(option.as_str().to_owned(), formats.value(option));
        }
        for name in [
            "window-status-format",
            "window-status-current-format",
            "window-status-separator",
            "window-status-style",
            "window-status-current-style",
            "window-status-last-style",
            "window-status-bell-style",
            "window-status-activity-style",
            "pane-status-style",
            "pane-status-current-style",
            "session-status-style",
            "session-status-current-style",
            "window-pane-status-format",
            "window-pane-current-status-format",
        ] {
            if let Some(value) = self.global_tmux_option_value(name) {
                variables.insert(name.to_owned(), value);
            }
        }
        variables
    }

    #[must_use]
    pub fn message_line_for_session(&self, session: Option<SessionId>) -> u8 {
        session
            .map_or_else(
                || self.global_session_options.message_line.clone(),
                |session| self.session_knobs(session).message_line,
            )
            .parse()
            .unwrap_or_default()
    }

    #[must_use]
    pub fn set_titles_for_session(&self, session: Option<SessionId>) -> bool {
        let target = session.map_or(TmuxOptionTarget::GlobalSession, TmuxOptionTarget::Session);
        self.scalar_option_effective(target, "set-titles") == Some("on")
    }

    #[must_use]
    pub fn set_titles_string_for_session(&self, session: Option<SessionId>) -> String {
        let target = session.map_or(TmuxOptionTarget::GlobalSession, TmuxOptionTarget::Session);
        self.scalar_option_effective(target, "set-titles-string")
            .unwrap_or_default()
            .to_owned()
    }

    #[must_use]
    pub fn status_customized_for_session(&self, session: Option<SessionId>) -> bool {
        !self.explicit_status_options.is_empty()
            || session.is_some_and(|session| {
                self.session_explicit_status_options
                    .get(&session)
                    .is_some_and(|options| !options.is_empty())
            })
    }

    fn mark_explicit_status_option(
        &mut self,
        session: Option<SessionId>,
        name: &'static str,
        explicit: bool,
    ) -> bool {
        match (session, explicit) {
            (None, true) => self.explicit_status_options.insert(name),
            (None, false) => self.explicit_status_options.remove(name),
            (Some(session), true) => self
                .session_explicit_status_options
                .entry(session)
                .or_default()
                .insert(name),
            (Some(session), false) => {
                let Some(options) = self.session_explicit_status_options.get_mut(&session) else {
                    return false;
                };
                let removed = options.remove(name);
                if options.is_empty() {
                    self.session_explicit_status_options.remove(&session);
                }
                removed
            }
        }
    }

    #[must_use]
    pub fn window_status_formats(&self, window: WindowId) -> WindowStatusFormats {
        let mut formats = self.window_status.clone();
        if let Some(overrides) = self.window_status_options.get(&window) {
            for (option, value) in overrides {
                formats
                    .set(*option, Some(value))
                    .expect("stored window status option was validated");
            }
        }
        formats
    }

    #[must_use]
    pub const fn focus_events(&self) -> bool {
        self.server_options.focus_events
    }

    #[must_use]
    pub const fn history_file(&self) -> &str {
        self.server_options.history_file.as_str()
    }

    #[must_use]
    pub const fn prefix_timeout_ms(&self) -> u32 {
        self.server_options.prefix_timeout_ms
    }

    #[must_use]
    pub const fn prompt_history_limit(&self) -> usize {
        self.server_options.prompt_history_limit
    }

    #[must_use]
    fn session_knobs(&self, session: SessionId) -> SessionOptions {
        let mut values = self.global_session_options.clone();
        if let Some(overrides) = self.session_options.get(&session) {
            for (option, value) in overrides {
                values
                    .set_command(*option, Some(value))
                    .expect("stored session option was validated");
            }
        }
        values
    }

    #[must_use]
    pub fn bell_action_for_session(&self, session: SessionId) -> BellAction {
        self.session_knobs(session).bell_action
    }

    #[must_use]
    pub fn visual_bell_for_session(&self, session: SessionId) -> VisualBell {
        self.session_knobs(session).visual_bell
    }

    #[must_use]
    pub fn key_table_for_session(&self, session: SessionId) -> String {
        let table = self.session_knobs(session).key_table;
        if table.is_empty() {
            "root".to_owned()
        } else {
            table
        }
    }

    #[must_use]
    pub fn detach_on_destroy_for_session(&self, session: SessionId) -> String {
        self.scalar_option_effective(TmuxOptionTarget::Session(session), "detach-on-destroy")
            .unwrap_or("on")
            .to_owned()
    }

    pub fn take_destroyed_sessions(&mut self) -> Vec<(SessionId, String, String)> {
        std::mem::take(&mut self.destroyed_sessions)
    }

    fn destroyed_session_marker(
        &self,
        session: SessionId,
    ) -> Result<(SessionId, String, String), ServerError> {
        let policy = self.detach_on_destroy_for_session(session);
        let name = self
            .state
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?
            .name
            .clone();
        Ok((session, name, policy))
    }

    fn record_destroyed_session(&mut self, marker: (SessionId, String, String)) {
        if !self.state.sessions.contains_key(&marker.0)
            && !self
                .destroyed_sessions
                .iter()
                .any(|(session, _, _)| *session == marker.0)
        {
            self.destroyed_sessions.push(marker);
        }
    }

    fn kill_session_state(&mut self, session: SessionId) -> Result<Vec<PaneId>, ServerError> {
        let marker = self.destroyed_session_marker(session)?;
        let panes = self.state.kill_session(session)?;
        self.record_destroyed_session(marker);
        Ok(panes)
    }

    fn kill_window_state(&mut self, window: WindowId) -> Result<Vec<PaneId>, ServerError> {
        let session = self
            .state
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
            .session;
        let marker = self.destroyed_session_marker(session)?;
        let panes = self.state.kill_window(window)?;
        self.record_destroyed_session(marker);
        Ok(panes)
    }

    fn kill_pane_state(&mut self, pane: PaneId) -> Result<Vec<PaneId>, ServerError> {
        let session = self
            .state
            .window_for_pane(pane)
            .and_then(|window| self.state.windows.get(&window))
            .map(|window| window.session)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let marker = self.destroyed_session_marker(session)?;
        let panes = self.state.kill_pane(pane)?;
        self.record_destroyed_session(marker);
        Ok(panes)
    }

    #[must_use]
    pub fn display_panes_time_for_session(&self, session: SessionId) -> u32 {
        self.session_knobs(session).display_panes_time_ms
    }

    #[must_use]
    pub fn global_default_size(&self) -> &str {
        self.global_session_options.default_size.as_str()
    }

    #[must_use]
    fn window_knobs(&self, window: WindowId) -> WindowOptions {
        let mut values = self.global_window_options.clone();
        if let Some(overrides) = self.window_options.get(&window) {
            for (option, value) in overrides {
                values
                    .set_command(*option, Some(value))
                    .expect("stored window option was validated");
            }
        }
        values
    }

    #[must_use]
    pub fn preset_options_for_window(&self, window: WindowId) -> PresetOptions {
        self.window_knobs(window).preset
    }

    #[must_use]
    pub fn window_size(&self, window: WindowId) -> WindowSize {
        self.window_knobs(window).window_size
    }

    #[must_use]
    fn pane_knobs(&self, pane: PaneId) -> PaneOptions {
        let mut values = self.global_pane_options.clone();
        if let Some(window) = self.state.window_for_pane(pane)
            && let Some(overrides) = self.window_pane_options.get(&window)
        {
            for (option, value) in overrides {
                values
                    .set_command(*option, Some(value))
                    .expect("stored window-pane option was validated");
            }
        }
        if let Some(overrides) = self.pane_options.get(&pane) {
            for (option, value) in overrides {
                values
                    .set_command(*option, Some(value))
                    .expect("stored pane option was validated");
            }
        }
        values
    }

    #[must_use]
    pub fn allow_set_title(&self, pane: PaneId) -> bool {
        self.pane_knobs(pane).allow_set_title
    }

    pub fn terminal_worker_options_for_pane(
        &self,
        pane: PaneId,
    ) -> Result<TerminalWorkerOptions, ServerError> {
        let window = self
            .state
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let pane_options = self.pane_knobs(pane);
        Ok(TerminalWorkerOptions {
            allow_passthrough: pane_options.allow_passthrough != AllowPassthrough::Off,
            wrap_search: self.window_knobs(window).wrap_search,
            cursor_style: pane_options.cursor_style.as_str(),
            cursor_colour: pane_options.cursor_colour,
        })
    }

    fn pane_knobs_for_window(&self, window: WindowId) -> PaneOptions {
        let mut values = self.global_pane_options.clone();
        if let Some(overrides) = self.window_pane_options.get(&window) {
            for (option, value) in overrides {
                values
                    .set_command(*option, Some(value))
                    .expect("stored window-pane option was validated");
            }
        }
        values
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

    #[must_use]
    pub fn evaluate_config_condition(&self, condition: &str) -> bool {
        let mut hooks = ConfigConditionHooks {
            engine: self,
            inner: CommandHooks::new(self.format_now()),
        };
        let expanded =
            expand_format_with_hooks(condition, self, FormatContext::default(), &mut hooks);
        format_true(&expanded)
    }

    #[must_use]
    pub fn parse_config(&self, source: impl Into<String>, input: &str) -> crate::ParsedConfig {
        let mut context = (
            |name: &str| self.global_environment_variable(name),
            |condition: &str| self.evaluate_config_condition(condition),
        );
        crate::parser::parse_config_with(source, input, &mut context)
    }

    pub fn parse_config_without_variable_expansion(
        source: impl Into<String>,
        input: &str,
    ) -> crate::ParsedConfig {
        crate::parser::parse_config_without_variable_expansion(source, input)
    }

    pub fn expand_pane_format_time(
        &self,
        format: &str,
        target: &ExecutionContext,
        active_session: Option<SessionId>,
        hooks: &mut impl StatusHooks,
    ) -> String {
        expand_format_time_with_hooks(
            format,
            self,
            FormatContext {
                session: target.session,
                window: target.window,
                pane: target.pane,
                active_session,
                format_type: FormatType::Pane,
            },
            hooks,
        )
    }

    pub fn expand_pane_format(
        &self,
        format: &str,
        target: &ExecutionContext,
        active_session: Option<SessionId>,
        hooks: &mut impl StatusHooks,
    ) -> String {
        expand_format_with_hooks(
            format,
            self,
            FormatContext {
                session: target.session,
                window: target.window,
                pane: target.pane,
                active_session,
                format_type: FormatType::Pane,
            },
            hooks,
        )
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

    pub fn initialize_default_shell(&mut self, value: impl Into<String>) {
        self.global_default_shell = value.into();
    }

    pub fn initialize_default_editor(&mut self, value: impl Into<String>) {
        self.server_options.editor = value.into();
    }

    pub fn global_default_shell(&self) -> &str {
        &self.global_default_shell
    }

    pub fn default_shell_for_session(&self, session: SessionId) -> Result<&str, ServerError> {
        if !self.state.sessions.contains_key(&session) {
            return Err(ServerError::MissingTarget(session.to_string()));
        }
        Ok(self
            .session_default_shells
            .get(&session)
            .map_or(self.global_default_shell.as_str(), String::as_str))
    }

    pub fn default_command_for_session(&self, session: SessionId) -> Result<&str, ServerError> {
        if !self.state.sessions.contains_key(&session) {
            return Err(ServerError::MissingTarget(session.to_string()));
        }
        Ok(self
            .session_default_commands
            .get(&session)
            .map_or(self.global_default_command.as_str(), String::as_str))
    }

    pub fn popup_options_for_window(&self, window: WindowId) -> Result<PopupOptions, ServerError> {
        if !self.state.windows.contains_key(&window) {
            return Err(ServerError::MissingTarget(window.to_string()));
        }
        Ok(PopupOptions {
            style: self
                .window_popup_styles
                .get(&window)
                .cloned()
                .unwrap_or_else(|| self.global_popup_style.clone()),
            border_style: self
                .window_popup_border_styles
                .get(&window)
                .cloned()
                .unwrap_or_else(|| self.global_popup_border_style.clone()),
            border_lines: self
                .window_popup_border_lines
                .get(&window)
                .copied()
                .unwrap_or(self.global_popup_border_lines),
        })
    }

    pub fn menu_options_for_window(&self, window: WindowId) -> Result<MenuOptions, ServerError> {
        if !self.state.windows.contains_key(&window) {
            return Err(ServerError::MissingTarget(window.to_string()));
        }
        Ok(MenuOptions {
            style: self
                .window_menu_styles
                .get(&window)
                .cloned()
                .unwrap_or_else(|| self.global_menu_style.clone()),
            selected_style: self
                .window_menu_selected_styles
                .get(&window)
                .cloned()
                .unwrap_or_else(|| self.global_menu_selected_style.clone()),
            border_style: self
                .window_menu_border_styles
                .get(&window)
                .cloned()
                .unwrap_or_else(|| self.global_menu_border_style.clone()),
            border_lines: self
                .window_menu_border_lines
                .get(&window)
                .copied()
                .unwrap_or(self.global_menu_border_lines),
        })
    }

    pub fn set_pane_start_command(
        &mut self,
        pane: PaneId,
        command: Vec<String>,
    ) -> Result<(), ServerError> {
        if self.state.pane(pane).is_none() {
            return Err(ServerError::MissingTarget(pane.to_string()));
        }
        self.pane_start_commands.insert(pane, command);
        Ok(())
    }

    pub(crate) fn pane_start_command(&self, pane: PaneId) -> Option<&[String]> {
        self.pane_start_commands.get(&pane).map(Vec::as_slice)
    }

    fn retain_pane_start_commands(&mut self, effects: &[MuxEffect]) {
        for effect in effects {
            match effect {
                MuxEffect::PaneCreated { pane, command, .. }
                | MuxEffect::PaneMaterialized { pane, command, .. } => {
                    self.pane_start_commands
                        .insert(*pane, command.clone().unwrap_or_default());
                }
                MuxEffect::PaneRespawned {
                    pane,
                    command: Some(command),
                    ..
                } if !command.is_empty() => {
                    self.pane_start_commands.insert(*pane, command.clone());
                }
                MuxEffect::PanesRemoved(panes) => {
                    for pane in panes {
                        self.pane_start_commands.remove(pane);
                    }
                }
                _ => {}
            }
        }
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

    #[must_use]
    pub fn global_environment_variable(&self, name: &str) -> Option<String> {
        self.global_environment
            .get(name)
            .and_then(|entry| entry.value.clone())
    }

    pub fn set_config_environment(&mut self, name: String, value: String, hidden: bool) {
        self.global_environment.insert(
            name,
            EnvironmentEntry {
                value: Some(value),
                hidden,
            },
        );
    }

    pub fn mark_session_active(&mut self, session: SessionId) {
        self.state.mark_session_active(session);
    }

    pub fn touch_session_activity(&mut self, session: SessionId) {
        self.state.touch_session_activity(session);
    }

    pub fn set_pane_runtime_facts(&mut self, pane: PaneId, facts: PaneRuntimeFacts) -> bool {
        let mut hooks = CommandHooks::new(self.format_now);
        self.set_pane_runtime_facts_with_hooks(pane, facts, &mut hooks)
    }

    pub fn set_pane_runtime_facts_with_hooks(
        &mut self,
        pane: PaneId,
        mut facts: PaneRuntimeFacts,
        hooks: &mut impl StatusHooks,
    ) -> bool {
        if self.state.pane(pane).is_none() {
            return false;
        }
        facts.dead_signal = tmux_signal_name(&facts.dead_signal);
        let command_changed = self
            .pane_runtime_facts
            .get(&pane)
            .map_or(!facts.current_command.is_empty(), |previous| {
                previous.current_command != facts.current_command
            });
        if self.pane_runtime_facts.get(&pane) == Some(&facts) {
            return false;
        }
        self.pane_runtime_facts.insert(pane, facts);
        self.state.bump_generation();
        if command_changed {
            self.refresh_automatic_window_name_for_pane(pane, hooks);
        }
        true
    }

    pub fn mark_pane_dead(
        &mut self,
        pane: PaneId,
        status: Option<u32>,
        signal: Option<&str>,
    ) -> Result<bool, ServerError> {
        let mut hooks = CommandHooks::new(self.format_now);
        self.mark_pane_dead_with_hooks(pane, status, signal, &mut hooks)
    }

    pub fn mark_pane_dead_with_hooks(
        &mut self,
        pane: PaneId,
        status: Option<u32>,
        signal: Option<&str>,
        hooks: &mut impl StatusHooks,
    ) -> Result<bool, ServerError> {
        if self.state.pane(pane).is_none() {
            return Err(ServerError::MissingTarget(pane.to_string()));
        }
        let dead_signal = signal.map_or_else(String::new, tmux_signal_name);
        let facts = self.pane_runtime_facts.entry(pane).or_default();
        let facts_changed = facts.dead_signal != dead_signal;
        facts.dead_signal = dead_signal;
        let state_changed = self.state.mark_pane_dead(pane, status)?;
        if facts_changed && !state_changed {
            self.state.bump_generation();
        }
        let name_changed = self.refresh_automatic_window_name_for_pane(pane, hooks);
        Ok(facts_changed || state_changed || name_changed)
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
            MuxOptionKey::Mouse => tmux_flag(self.global_mouse).to_owned(),
            MuxOptionKey::EscapeTime => self.escape_time_ms.to_string(),
            MuxOptionKey::Prefix2 => self
                .scalar_option_effective(TmuxOptionTarget::GlobalSession, "prefix2")
                .unwrap_or("None")
                .to_owned(),
        }
    }

    /// The `mouse` value effective for one attached session, or the global
    /// value for an unattached client.
    #[must_use]
    pub fn effective_mouse(&self, session: Option<SessionId>) -> bool {
        session.map_or(self.global_mouse, |session| self.mouse_for_session(session))
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
    pub fn default_terminal_for_spawn(&self) -> &str {
        self.default_terminal.as_deref().unwrap_or(DEFAULT_TERMINAL)
    }

    #[must_use]
    pub fn initial_repeat_time_for_session(&self, session: SessionId) -> u32 {
        self.session_initial_repeat_time_ms
            .get(&session)
            .copied()
            .unwrap_or(self.global_initial_repeat_time_ms)
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

    #[must_use]
    pub fn display_time_for_session(&self, session: SessionId) -> u32 {
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

    fn refresh_automatic_window_name_for_pane(
        &mut self,
        pane: PaneId,
        hooks: &mut impl StatusHooks,
    ) -> bool {
        let Some(window) = self.state.window_for_pane(pane) else {
            return false;
        };
        let Some(window_state) = self.state.windows.get(&window) else {
            return false;
        };
        if window_state.active_pane != pane
            || !self
                .state
                .window_automatic_rename(window)
                .unwrap_or_default()
        {
            return false;
        }
        let session = window_state.session;
        let pane_dead = window_state.panes[&pane].dead;
        let format = self.automatic_rename_format_for_window(window).to_owned();
        if !pane_dead
            && !self.pane_runtime_facts.contains_key(&pane)
            && format == DEFAULT_AUTOMATIC_RENAME_FORMAT
        {
            return false;
        }
        let name = expand_format_with_hooks(
            &format,
            self,
            FormatContext {
                session: Some(session),
                window: Some(window),
                pane: None,
                active_session: Some(session),
                format_type: FormatType::Window,
            },
            hooks,
        );
        let changed = self.state.windows[&window].name != name;
        if changed {
            self.state
                .rename_window(window, name)
                .expect("automatic rename window exists");
        }
        changed
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
        Ok(self.job_environment(Some(session)))
    }

    /// The environment a shell job inherits: the global overlay, then the
    /// session overlay when the job has one. Hidden entries and child-unset
    /// markers come through as `None` so the spawner removes them.
    pub fn job_environment(&self, session: Option<SessionId>) -> Vec<(String, Option<String>)> {
        let mut environment = BTreeMap::new();
        for (name, entry) in &self.global_environment {
            let value = if entry.hidden {
                None
            } else {
                entry.value.clone()
            };
            environment.insert(name.clone(), value);
        }
        if let Some(overlay) = session.and_then(|session| self.session_environments.get(&session)) {
            for (name, entry) in overlay {
                let value = if entry.hidden {
                    None
                } else {
                    entry.value.clone()
                };
                environment.insert(name.clone(), value);
            }
        }
        environment.into_iter().collect()
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

    pub fn new_session_attaches(args: &[String]) -> Result<bool, ServerError> {
        let (options, _) = parse_command_options("new-session", args)?;
        Ok(options.has("-A") || !options.has("-d"))
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
        self.execute_with_shell_validator(context, command, hooks, &mut |_| true)
    }

    pub fn execute_with_shell_validator(
        &mut self,
        context: &mut ExecutionContext,
        command: &CommandInvocation,
        hooks: &mut impl StatusHooks,
        default_shell_is_valid: &mut impl FnMut(&str) -> bool,
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
            "break-pane" => self.break_pane(context, &command.args, hooks)?,
            "join-pane" | "move-pane" => self.join_pane(context, &command.args, name)?,
            "set-browser-url" => self.set_browser_url(context, &command.args)?,
            "set-browser-tabs" => self.set_browser_tabs(context, &command.args)?,
            "set-browser-profile" => self.set_browser_profile(context, &command.args)?,
            "set-agent-session" => self.set_agent_session(context, &command.args)?,
            "set-agent-provider" => self.set_agent_provider(context, &command.args)?,
            "restart-agent-pane" => self.restart_agent_pane(context, &command.args)?,
            "set-editor-path" => self.set_editor_path(context, &command.args)?,
            "select-pane" => self.select_pane(context, &command.args, hooks)?,
            "last-pane" => self.last_pane(context, &command.args, hooks)?,
            "swap-pane" => self.swap_pane(context, &command.args)?,
            "list-panes" => self.list_panes(context, &command.args, hooks)?,
            "resize-pane" => self.resize_pane(context, &command.args)?,
            "select-layout" | "next-layout" | "previous-layout" => {
                self.select_layout(context, &command.args, name)?
            }
            "rotate-window" => self.rotate_window(context, &command.args)?,
            "kill-pane" => self.kill_pane(context, &command.args, hooks)?,
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
            "list-keys" => self.list_keys(context, &command.args, hooks)?,
            "list-commands" => self.list_commands(context, &command.args, hooks)?,
            "set-hook" => self.set_hook(context, &command.args, hooks, default_shell_is_valid)?,
            "show-hooks" => self.show_hooks(context, &command.args, hooks)?,
            "set-option" => {
                self.set_option(context, &command.args, false, hooks, default_shell_is_valid)?
            }
            "set-window-option" => {
                self.set_option(context, &command.args, true, hooks, default_shell_is_valid)?
            }
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
            _ => {
                return Err(match resolve_command(&command.name) {
                    CommandResolution::Unimplemented(name) => {
                        ServerError::UnsupportedCommand(name.to_owned())
                    }
                    CommandResolution::Ambiguous(message) => ServerError::InvalidCommand(message),
                    _ => ServerError::InvalidCommand(format!("unknown command: {}", command.name)),
                });
            }
        };

        self.retain_pane_start_commands(&execution.effects);
        let created_panes = execution
            .effects
            .iter()
            .filter_map(|effect| match effect {
                MuxEffect::PaneCreated { pane, .. } | MuxEffect::PaneMaterialized { pane, .. } => {
                    Some(*pane)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for pane in created_panes {
            self.refresh_automatic_window_name_for_pane(pane, hooks);
        }
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
        self.session_initial_repeat_time_ms
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_repeat_time_ms
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_options
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_default_commands
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_default_shells
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_lock_commands
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_lock_after_times
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_user_options
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_hooks
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.window_user_options
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_options
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_pane_options
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_hooks
            .retain(|window, _| self.state.windows.contains_key(window));
        self.pane_user_options
            .retain(|pane, _| self.state.pane(*pane).is_some());
        self.pane_options
            .retain(|pane, _| self.state.pane(*pane).is_some());
        self.pane_hooks
            .retain(|pane, _| self.state.pane(*pane).is_some());
        self.pane_start_commands
            .retain(|pane, _| self.state.pane(*pane).is_some());
        self.session_environments
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.window_mode_keys
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_automatic_rename_formats
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_remain_on_exit
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_popup_styles
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_popup_border_styles
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_popup_border_lines
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_menu_styles
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_menu_selected_styles
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_menu_border_styles
            .retain(|window, _| self.state.windows.contains_key(window));
        self.window_menu_border_lines
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
        let detached = options.has("-d") || context.client_terminal == ClientTerminal::NoClient;
        if options.has("-A") {
            let existing = match options.value("-s") {
                Some(name) => session_named(&self.state, name),
                None => self.state.resolve_session(None, context.session).ok(),
            };
            if let Some(session) = existing {
                if context.has_no_client() {
                    return Ok(Execution::default());
                }
                require_client_terminal(context)?;
                let window = session_active_window(&self.state, session)?;
                let pane = window_active_pane(&self.state, window)?;
                context.retarget(&ExecutionContext::new(
                    Some(session),
                    Some(window),
                    Some(pane),
                ));
                return Ok(Execution::effect(MuxEffect::Attach {
                    session,
                    detach_others: options.has("-D"),
                    read_only: false,
                }));
            }
        }
        let name = options
            .value("-s")
            .map_or_else(|| next_session_name(&self.state), str::to_owned);
        if session_named(&self.state, &name).is_some() {
            return Err(ServerError::InvalidCommand(format!(
                "duplicate session: {name}"
            )));
        }
        if !detached {
            require_client_terminal(context)?;
        }
        let extent =
            initial_window_extent(&options, self.global_default_size(), context.client_size())?;
        let (inherit_cwd_from, cwd) =
            spawn_cwd_source(self, &options, context.pane, &PaneKind::Terminal, hooks);
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
        context.retarget(&ExecutionContext::new(
            Some(session),
            Some(window),
            Some(pane),
        ));
        let output = if options.has("-P") {
            expand_format_with_hooks(
                options.value("-F").unwrap_or(DEFAULT_NEW_SESSION_FORMAT),
                self,
                FormatContext {
                    session: Some(session),
                    window: Some(window),
                    pane: None,
                    active_session: Some(session),
                    format_type: FormatType::None,
                },
                hooks,
            )
        } else {
            String::new()
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
                read_only: false,
            });
        }
        Ok(Execution { output, effects })
    }

    fn list_sessions(
        &self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-sessions", args)?;
        reject_positionals("list-sessions", &positional)?;
        let sort = TmuxSort::parse(options.value("-O"), options.has("-r"), None)?;
        let format = options.value("-F").unwrap_or(DEFAULT_LIST_SESSIONS_FORMAT);
        let mut sessions = self
            .state
            .sessions_by_name()
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();
        sort.apply(&mut sessions, |left, right| {
            let left = &self.state.sessions[left];
            let right = &self.state.sessions[right];
            let ordering = match sort.order() {
                Some(TmuxSortOrder::Activity) => right.sort_activity.cmp(&left.sort_activity),
                Some(TmuxSortOrder::Creation) => left.sort_created.cmp(&right.sort_created),
                Some(TmuxSortOrder::Index) => left.id.cmp(&right.id),
                Some(TmuxSortOrder::Name) => left.name.cmp(&right.name),
                _ => std::cmp::Ordering::Equal,
            };
            ordering.then_with(|| left.name.cmp(&right.name))
        });
        let mut output = Vec::new();
        for (line, session) in sessions.into_iter().enumerate() {
            let format_context = FormatContext {
                session: Some(session),
                window: None,
                pane: None,
                active_session: context.session,
                format_type: FormatType::Session,
            };
            if let Some(filter) = options.value("-f") {
                let mut row_hooks = RowFormatHooks { inner: hooks, line };
                let expanded =
                    expand_format_with_hooks(filter, self, format_context, &mut row_hooks);
                if !format_true(&expanded) {
                    continue;
                }
            }
            let mut row_hooks = RowFormatHooks { inner: hooks, line };
            output.push(expand_format_with_hooks(
                format,
                self,
                format_context,
                &mut row_hooks,
            ));
        }
        Ok(Execution::output(output.join("\n")))
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
            panes.extend(self.kill_session_state(target)?);
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
        if context.has_no_client() {
            return Ok(Execution::default());
        }
        let detach_others = options.has("-d");
        let session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        require_client_terminal(context)?;
        let window = session_active_window(&self.state, session)?;
        let pane = window_active_pane(&self.state, window)?;
        context.retarget(&ExecutionContext::new(
            Some(session),
            Some(window),
            Some(pane),
        ));
        Ok(Execution::effect(MuxEffect::Attach {
            session,
            detach_others,
            read_only: options.has("-r"),
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
        command: Option<Vec<String>>,
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let active_session = context.session;
        let destination = window_destination(&self.state, options.value("-t"), context)?;
        let session = destination.session;
        let selects = !options.has("-d");
        let name = options.value("-n").map(|name| {
            expand_format_with_hooks(
                name,
                self,
                FormatContext {
                    session: Some(session),
                    window: None,
                    pane: None,
                    active_session,
                    format_type: FormatType::Window,
                },
                hooks,
            )
        });
        if options.has("-S")
            && destination.index.is_none()
            && let Some(name) = name.as_deref()
        {
            let matches = self.state.sessions[&session]
                .windows
                .iter()
                .copied()
                .filter(|window| self.state.windows[window].name == name)
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return Err(ServerError::InvalidCommand(format!(
                    "multiple windows named {name}"
                )));
            }
            if let Some(existing) = matches.first().copied() {
                if selects {
                    self.state.select_window(session, existing)?;
                    context.retarget(&ExecutionContext::new(
                        Some(session),
                        Some(existing),
                        Some(window_active_pane(&self.state, existing)?),
                    ));
                }
                return Ok(Execution::effect(MuxEffect::SuppressAfterHook));
            }
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
        let has_name = name.is_some();
        let window_name = name.or_else(|| index.map(|index| index.to_string()));
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
        if has_name {
            self.state
                .set_window_automatic_rename(window, Some(false))?;
        }
        if let Some(replaced) = replaced {
            effects.push(MuxEffect::PanesRemoved(self.kill_window_state(replaced)?));
            self.state
                .set_window_index(window, index.expect("replacing an occupied index"))?;
        }
        if selects {
            context.retarget(&ExecutionContext::new(
                Some(session),
                Some(window),
                Some(pane),
            ));
        }
        effects.push(MuxEffect::PaneCreated {
            pane,
            kind: snapshot_kind,
            inherit_cwd_from,
            cwd,
            command,
        });
        if options.has("-P") {
            effects.push(MuxEffect::PaneFormatOutput {
                pane,
                format: options
                    .value("-F")
                    .unwrap_or(DEFAULT_PANE_CREATION_FORMAT)
                    .to_owned(),
                active_session,
            });
        }
        let output =
            self.pane_creation_output(options, session, window, pane, active_session, hooks);
        Ok(Execution { output, effects })
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
        let sort = TmuxSort::parse(options.value("-O"), options.has("-r"), None)?;
        let session_ids = if options.has("-a") {
            self.state
                .sessions_by_name()
                .into_iter()
                .map(|session| session.id)
                .collect::<Vec<_>>()
        } else {
            vec![target_session]
        };
        let format = options.value("-F").unwrap_or(if options.has("-a") {
            DEFAULT_LIST_WINDOWS_WITH_SESSION_FORMAT
        } else {
            DEFAULT_LIST_WINDOWS_FORMAT
        });
        let mut windows = Vec::new();
        for session_id in session_ids {
            let session = self
                .state
                .sessions
                .get(&session_id)
                .ok_or_else(|| ServerError::MissingTarget(session_id.to_string()))?;
            for window_id in &session.windows {
                if !self.state.windows.contains_key(window_id) {
                    continue;
                }
                windows.push((session_id, *window_id));
            }
        }
        sort.apply(&mut windows, |(_, left), (_, right)| {
            let left = &self.state.windows[left];
            let right = &self.state.windows[right];
            let ordering = match sort.order() {
                Some(TmuxSortOrder::Activity) => right.activity.cmp(&left.activity),
                Some(TmuxSortOrder::Creation) => left.created.cmp(&right.created),
                Some(TmuxSortOrder::Index) => left.index.cmp(&right.index),
                Some(TmuxSortOrder::Name) => left.name.cmp(&right.name),
                Some(TmuxSortOrder::Size) => {
                    let left_extent = left.layout.extent();
                    let right_extent = right.layout.extent();
                    (u32::from(left_extent.0) * u32::from(left_extent.1))
                        .cmp(&(u32::from(right_extent.0) * u32::from(right_extent.1)))
                }
                _ => std::cmp::Ordering::Equal,
            };
            ordering.then_with(|| left.name.cmp(&right.name))
        });
        let line = windows.len();
        let mut output = Vec::new();
        for (session, window) in windows {
            let format_context = FormatContext {
                session: Some(session),
                window: Some(window),
                pane: None,
                active_session: context.session,
                format_type: FormatType::Window,
            };
            if let Some(filter) = options.value("-f") {
                let mut row_hooks = RowFormatHooks { inner: hooks, line };
                let expanded =
                    expand_format_with_hooks(filter, self, format_context, &mut row_hooks);
                if !format_true(&expanded) {
                    continue;
                }
            }
            let mut row_hooks = RowFormatHooks { inner: hooks, line };
            output.push(expand_format_with_hooks(
                format,
                self,
                format_context,
                &mut row_hooks,
            ));
        }
        Ok(Execution::output(output.join("\n")))
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
            panes.extend(self.kill_window_state(target)?);
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
        let destroyed_source = self.destroyed_session_marker(source_session)?;
        let removed = self.state.move_window(
            source,
            destination_session,
            destination_index,
            options.has("-k"),
            !detached,
        )?;
        self.record_destroyed_session(destroyed_source);
        if options.value("-s").is_none() {
            self.renumber_session_if_enabled(source_session)?;
        }

        if detached && original_context.window == Some(source) {
            let window = if self.state.sessions.contains_key(&source_session) {
                session_active_window(&self.state, source_session)?
            } else {
                session_active_window(&self.state, destination_session)?
            };
            let target =
                ExecutionContext::for_pane(&self.state, window_active_pane(&self.state, window)?)
                    .expect("moved window leaves a valid command context");
            context.retarget(&target);
        } else if !detached {
            let target =
                ExecutionContext::for_pane(&self.state, window_active_pane(&self.state, source)?)
                    .expect("moved window has an active pane");
            context.retarget(&target);
        }

        let mut effects = Vec::new();
        if !removed.is_empty() {
            effects.push(MuxEffect::PanesRemoved(removed));
        }
        if source_session == destination_session {
            effects.extend(
                source_panes
                    .into_iter()
                    .map(|pane| MuxEffect::TerminalKnobsChanged {
                        window: None,
                        pane: Some(pane),
                    }),
            );
        } else {
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
        if source_session == target_session {
            effects.extend(source_panes.into_iter().chain(target_panes).map(|pane| {
                MuxEffect::TerminalKnobsChanged {
                    window: None,
                    pane: Some(pane),
                }
            }));
        } else {
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
        hooks: &mut impl StatusHooks,
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
        let destroyed_source = self.destroyed_session_marker(source_session)?;
        let window = self.state.break_pane_with_base_index(
            source,
            destination_session,
            destination.index,
            options.value("-n").map(str::to_owned),
            detached,
            base_index,
        )?;
        self.record_destroyed_session(destroyed_source);
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
                let target = ExecutionContext::for_pane(&self.state, pane)
                    .expect("break-pane retains a valid command context");
                context.retarget(&target);
            }
        } else {
            let target = ExecutionContext::for_pane(&self.state, source)
                .expect("broken pane belongs to its new window");
            context.retarget(&target);
        }
        let mut execution = Execution::default();
        if source_session != destination_session {
            execution.effects.push(MuxEffect::PaneRelocated {
                pane: source,
                from: source_session,
                to: destination_session,
            });
        } else if source_window != window {
            execution.effects.push(MuxEffect::TerminalKnobsChanged {
                window: None,
                pane: Some(source),
            });
        }
        debug_assert_eq!(self.state.window_for_pane(source), Some(window));
        execution.output = self.pane_creation_output(
            &options,
            destination_session,
            window,
            source,
            original_context.session,
            hooks,
        );
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
                let target = ExecutionContext::for_pane(&self.state, pane)
                    .expect("detached join retains a valid source context");
                context.retarget(&target);
            }
        } else {
            let target = ExecutionContext::for_pane(&self.state, source)
                .expect("joined pane belongs to the target window");
            context.retarget(&target);
        }
        let mut execution = Execution::default();
        if source_session != target_session {
            execution.effects.push(MuxEffect::PaneRelocated {
                pane: source,
                from: source_session,
                to: target_session,
            });
        } else if source_window != target_window {
            execution.effects.push(MuxEffect::TerminalKnobsChanged {
                window: None,
                pane: Some(source),
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
        command: Option<Vec<String>>,
        size: Option<SplitSize<'_>>,
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let active_session = context.session;
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
            let target =
                ExecutionContext::for_pane(&self.state, pane).expect("new pane has a context");
            context.retarget(&target);
        }
        let window = self
            .state
            .window_for_pane(pane)
            .expect("new pane belongs to a window");
        let session = self.state.windows[&window].session;
        let mut effects = vec![MuxEffect::PaneCreated {
            pane,
            kind: snapshot_kind,
            inherit_cwd_from,
            cwd,
            command,
        }];
        if options.has("-P") {
            effects.push(MuxEffect::PaneFormatOutput {
                pane,
                format: options
                    .value("-F")
                    .unwrap_or(DEFAULT_PANE_CREATION_FORMAT)
                    .to_owned(),
                active_session,
            });
        }
        Ok(Execution {
            output: self.pane_creation_output(
                options,
                session,
                window,
                pane,
                active_session,
                hooks,
            ),
            effects,
        })
    }

    fn pane_creation_output(
        &self,
        options: &Options,
        session: SessionId,
        window: WindowId,
        pane: PaneId,
        active_session: Option<SessionId>,
        hooks: &mut impl StatusHooks,
    ) -> String {
        if !options.has("-P") {
            return String::new();
        }
        let mut output = expand_format_with_hooks(
            options.value("-F").unwrap_or(DEFAULT_PANE_CREATION_FORMAT),
            self,
            FormatContext {
                session: Some(session),
                window: Some(window),
                pane: Some(pane),
                active_session,
                format_type: FormatType::Pane,
            },
            hooks,
        );
        output.push('\n');
        output
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
        hooks: &mut impl StatusHooks,
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
            self.select_pane_target(context, pane, options.has("-Z"), hooks)?;
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
        self.select_pane_target(context, pane, options.has("-Z"), hooks)?;
        Ok(Execution::default())
    }

    fn select_pane_target(
        &mut self,
        context: &mut ExecutionContext,
        pane: PaneId,
        preserve_zoom: bool,
        hooks: &mut impl StatusHooks,
    ) -> Result<(), ServerError> {
        if self.state.select_pane_with_zoom(pane, preserve_zoom)? {
            let target =
                ExecutionContext::for_pane(&self.state, pane).expect("selected pane exists");
            context.retarget(&target);
            self.refresh_automatic_window_name_for_pane(pane, hooks);
        }
        Ok(())
    }

    fn last_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("last-pane", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "last-pane supports only -t and -Z".to_owned(),
            ));
        }
        let window = self.resolve_window(options.value("-t"), context.session, context.window)?;
        let pane = self.state.last_pane(window)?;
        self.select_pane_target(context, pane, options.has("-Z"), hooks)?;
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
        let context_target = ExecutionContext::for_pane(&self.state, active)
            .expect("the target window retains an active pane after a swap");
        context.retarget(&context_target);

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
        } else if source_window != target_window {
            execution.effects.extend([
                MuxEffect::TerminalKnobsChanged {
                    window: None,
                    pane: Some(source),
                },
                MuxEffect::TerminalKnobsChanged {
                    window: None,
                    pane: Some(target),
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
        let sort = TmuxSort::parse(options.value("-O"), options.has("-r"), None)?;
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
        let format = options.value("-F").unwrap_or(if options.has("-a") {
            DEFAULT_LIST_PANES_WITH_SERVER_FORMAT
        } else if options.has("-s") {
            DEFAULT_LIST_PANES_WITH_SESSION_FORMAT
        } else {
            DEFAULT_LIST_PANES_FORMAT
        });
        let mut output = Vec::new();
        for window_id in window_ids {
            let window = self
                .state
                .windows
                .get(&window_id)
                .ok_or_else(|| ServerError::MissingTarget(window_id.to_string()))?;
            let mut panes = window.pane_order().to_vec();
            sort.apply(&mut panes, |left, right| {
                let left_pane = &window.panes[left];
                let right_pane = &window.panes[right];
                let left_index = window
                    .pane_order()
                    .iter()
                    .position(|pane| pane == left)
                    .unwrap_or_default();
                let right_index = window
                    .pane_order()
                    .iter()
                    .position(|pane| pane == right)
                    .unwrap_or_default();
                let ordering = match sort.order() {
                    Some(TmuxSortOrder::Activity) => {
                        left_pane.active_point.cmp(&right_pane.active_point)
                    }
                    Some(TmuxSortOrder::Creation) => left.cmp(right),
                    Some(TmuxSortOrder::Index) => left_index.cmp(&right_index),
                    Some(TmuxSortOrder::Name) => left_pane.title.cmp(&right_pane.title),
                    Some(TmuxSortOrder::Size) => {
                        let left_extent = window.displayed_pane_geometry(*left).unwrap_or_default();
                        let right_extent =
                            window.displayed_pane_geometry(*right).unwrap_or_default();
                        (u32::from(left_extent.0) * u32::from(left_extent.1))
                            .cmp(&(u32::from(right_extent.0) * u32::from(right_extent.1)))
                    }
                    _ => std::cmp::Ordering::Equal,
                };
                ordering.then_with(|| left_pane.title.cmp(&right_pane.title))
            });
            let line = panes.len();
            for pane_id in panes {
                let Some(pane) = window.panes.get(&pane_id) else {
                    continue;
                };
                let format_context = FormatContext {
                    session: Some(window.session),
                    window: Some(window_id),
                    pane: Some(pane.id),
                    active_session: context.session,
                    format_type: FormatType::Pane,
                };
                if let Some(filter) = options.value("-f") {
                    let mut row_hooks = RowFormatHooks { inner: hooks, line };
                    let expanded =
                        expand_format_with_hooks(filter, self, format_context, &mut row_hooks);
                    if !format_true(&expanded) {
                        continue;
                    }
                }
                let mut row_hooks = RowFormatHooks { inner: hooks, line };
                output.push(expand_format_with_hooks(
                    format,
                    self,
                    format_context,
                    &mut row_hooks,
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
                let target = ExecutionContext::for_pane(&self.state, active)
                    .expect("zoomed window has an active pane context");
                context.retarget(&target);
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
    pub fn pane_geometry_at_window_extent(
        &self,
        pane: PaneId,
        columns: u16,
        rows: u16,
    ) -> Option<(u16, u16)> {
        let window = self.state.window_for_pane(pane)?;
        let window = self.state.windows.get(&window)?;
        if let Some(zoomed) = window.zoomed_pane {
            return (zoomed == pane).then_some((columns, rows));
        }
        let mut layout = window.layout.clone();
        layout.resize(columns, rows);
        let geometry = layout.pane_geometry(pane)?;
        Some((geometry.sx, geometry.sy))
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
        let preset_options = self.preset_options_for_window(window);

        if command == "next-layout" || options.has("-n") {
            self.state.cycle_layout(window, 1, &preset_options)?;
        } else if command == "previous-layout" || options.has("-p") {
            self.state.cycle_layout(window, -1, &preset_options)?;
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
                self.state.select_layout(window, preset, &preset_options)?;
            } else {
                self.state.select_layout_string(window, name)?;
            }
        } else if let Some(last) = self.state.last_layout(window)? {
            self.state.select_layout(window, last, &preset_options)?;
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
        let target = ExecutionContext::for_pane(&self.state, pane)
            .expect("rotated window retains an active pane");
        context.retarget(&target);
        Ok(Execution::default())
    }

    fn kill_pane(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
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
            panes.extend(self.kill_pane_state(target)?);
        }
        if self.state.windows.contains_key(&window) {
            let active_pane = self.state.windows[&window].active_pane;
            self.refresh_automatic_window_name_for_pane(active_pane, hooks);
        } else {
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
        if !pane_state.dead && !pane_state.empty && !options.has("-k") {
            return Err(ServerError::InvalidCommand(format!(
                "respawn pane failed: pane {} still active",
                self.pane_target_description(pane)?
            )));
        }
        let (_, cwd) = spawn_cwd_source(self, &options, Some(pane), &PaneKind::Terminal, hooks);
        let environment = respawn_environment(&options);
        let command = shell_command_positional(&positional);
        let empty = options.has("-E");
        if empty {
            self.state.mark_pane_empty(pane)?;
        } else {
            self.state.revive_pane(pane)?;
        }
        Ok(Execution::effect(MuxEffect::PaneRespawned {
            pane,
            cwd,
            command,
            environment,
            empty,
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
        if !options.has("-k")
            && window_state
                .panes
                .values()
                .any(|pane| !pane.dead && !pane.empty)
        {
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
        let empty = options.has("-E");
        let mut removed = Vec::new();
        for other in other_panes {
            removed.extend(self.kill_pane_state(other)?);
        }
        if empty {
            self.state.mark_pane_empty(pane)?;
        } else {
            self.state.revive_pane(pane)?;
        }
        let mut effects = Vec::with_capacity(2);
        if !removed.is_empty() {
            effects.push(MuxEffect::PanesRemoved(removed));
        }
        effects.push(MuxEffect::PaneRespawned {
            pane,
            cwd,
            command,
            environment,
            empty,
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
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "choose-tree command templates are not supported yet".to_owned(),
            ));
        }
        let sort = TmuxSort::parse(
            options.value("-O"),
            options.has("-r"),
            Some(TmuxSortOrder::Index),
        )?;
        Ok(Execution::effect(MuxEffect::ChooseTree {
            pane,
            kind: if options.has("-s") || options.has("-w") {
                ChooseTreeKind::Windows
            } else {
                ChooseTreeKind::Panes
            },
            sessions_only: options.has("-s"),
            filter: options.value("-f").map(str::to_owned),
            sort,
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
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "choose-buffer command templates are not supported yet".to_owned(),
            ));
        }
        let sort = TmuxSort::parse(
            options.value("-O"),
            options.has("-r"),
            Some(TmuxSortOrder::Creation),
        )?;
        Ok(Execution::effect(MuxEffect::ChooseBuffer {
            pane,
            filter: options.value("-f").map(str::to_owned),
            sort,
        }))
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
            options
                .value("-F")
                .unwrap_or(DEFAULT_DISPLAY_MESSAGE)
                .to_owned()
        } else {
            if options.value("-F").is_some() {
                return Err(ServerError::InvalidCommand(
                    "only one of -F or argument must be given".to_owned(),
                ));
            }
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
            || {
                let window = self
                    .state
                    .window_for_pane(pane)
                    .expect("display-panes pane was resolved");
                let session = self.state.windows[&window].session;
                Ok(self.display_panes_time_for_session(session))
            },
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
        let commands = bound_commands(self, &positional[1..])?;
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

    fn list_keys(
        &self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
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
        let output = if let Some(format) = options.value("-F") {
            let mut output = Vec::new();
            for (table, key, binding) in self.keys.list(options.value("-T")) {
                let mut item_hooks = ListKeyHooks {
                    inner: &mut *hooks,
                    table,
                    key,
                    binding,
                    prefix: self.keys.prefix(),
                };
                let line = expand_format_with_hooks(
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
                );
                if !line.is_empty() {
                    output.push(line);
                }
            }
            output.join("\n")
        } else {
            self.keys
                .list(options.value("-T"))
                .map(|(table, key, binding)| {
                    let commands = binding
                        .commands
                        .iter()
                        .map(format_command)
                        .collect::<Vec<_>>()
                        .join(" \\; ");
                    let repeat = if binding.repeat { " -r" } else { "" };
                    format!("bind-key{repeat} -T {table} {key} {commands}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
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
            let name = canonical_command(name);
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

    fn set_hook(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
        default_shell_is_valid: &mut impl FnMut(&str) -> bool,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("set-hook", args)?;
        if options.value("-B").is_some() {
            return Err(ServerError::InvalidCommand("invalid flag -B".to_owned()));
        }
        let Some(argument) = positional.first() else {
            return Err(ServerError::InvalidCommand("missing argument".to_owned()));
        };
        if positional.len() > 2 {
            return Err(ServerError::InvalidCommand("too many arguments".to_owned()));
        }
        let (argument, target_context) =
            self.expand_hook_name(context, &options, argument, hooks)?;
        let parsed = parse_tmux_option(&argument)
            .map_err(|()| ServerError::InvalidCommand(format!("invalid option: {argument}")))?;
        if parsed.name.starts_with('@') {
            if options.has("-R") {
                let Some(commands) = self.user_hook_commands(&target_context, &argument) else {
                    return Ok(Execution::default());
                };
                return Ok(Execution::effect(MuxEffect::RunHook {
                    name: argument,
                    commands,
                    context: target_context,
                }));
            }
            if parsed.index.is_some() {
                return Err(ServerError::InvalidCommand(format!(
                    "not an array: {argument}"
                )));
            }
            return self.set_user_option(
                context,
                parsed.name,
                positional.get(1).map(String::as_str),
                &options,
                false,
            );
        }
        let table_option = match match_tmux_option(parsed.name) {
            Ok(Some(option)) => option,
            Ok(None) | Err(()) if options.has("-R") => return Ok(Execution::default()),
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
        if options.has("-R") {
            if !tmux_option_is_hook(table_option.name) {
                return Ok(Execution::default());
            }
            let Some(commands) = self.event_hook_commands(&target_context, table_option.name)
            else {
                return Ok(Execution::default());
            };
            if commands.is_empty() {
                return Ok(Execution::default());
            }
            return Ok(Execution::effect(MuxEffect::RunHook {
                name: table_option.name.to_owned(),
                commands,
                context: target_context,
            }));
        }
        if !tmux_option_is_hook(table_option.name) {
            let mut forwarded = Vec::new();
            for flag in ["-a", "-g", "-p", "-u", "-w"] {
                if options.has(flag) {
                    forwarded.push(flag.to_owned());
                }
            }
            if let Some(target) = options.value("-t") {
                forwarded.extend(["-t".to_owned(), target.to_owned()]);
            }
            forwarded.extend(["--".to_owned(), argument]);
            if let Some(value) = positional.get(1) {
                forwarded.push(value.clone());
            }
            return self.set_option(context, &forwarded, false, hooks, default_shell_is_valid);
        }
        let target =
            self.resolve_tmux_option_target(context, &options, false, table_option.scope)?;
        self.set_hook_array_option(
            target,
            table_option.name,
            parsed.index,
            positional.get(1).map(String::as_str),
            &options,
        )
    }

    fn set_hook_array_option(
        &mut self,
        target: TmuxOptionTarget,
        name: &'static str,
        index: Option<String>,
        value: Option<&str>,
        options: &Options,
    ) -> Result<Execution, ServerError> {
        let index = index.map(ArrayIndex::parse);
        let unset = option_is_unset(options);
        let already = self
            .hook_array(target, name)
            .is_some_and(|hook| index.as_ref().is_none_or(|index| hook.contains_key(index)));
        if options.has("-o") && !unset && already {
            let displayed_index = index.as_ref().map(ArrayIndex::display);
            let option = indexed_option_name(name, displayed_index.as_deref());
            return already_set_or_quiet(options, &option);
        }
        if options.has("-U")
            && let TmuxOptionTarget::Window(window) = target
        {
            self.remove_pane_hook_overrides(window, name, index.as_ref());
        }
        if unset {
            self.unset_hook_array(target, name, index.as_ref());
            return Ok(Execution::default());
        }
        let value = value.ok_or_else(|| ServerError::InvalidCommand("empty value".to_owned()))?;
        let commands = parse_hook_commands(self, value)?;
        let hook = self.hook_array_mut_or_insert(target, name);
        if let Some(index) = index {
            hook.insert(index, commands);
        } else if options.has("-a") {
            let index = first_free_array_index(hook.keys())?;
            hook.insert(ArrayIndex::Numeric(index), commands);
        } else {
            hook.clear();
            hook.insert(ArrayIndex::Numeric(0), commands);
        }
        Ok(Execution::default())
    }

    fn hook_array(&self, target: TmuxOptionTarget, name: &str) -> Option<&HookArray> {
        self.hook_table(target).and_then(|hooks| hooks.get(name))
    }

    fn hook_array_readback(
        &self,
        target: TmuxOptionTarget,
        name: &str,
        include_inherited: bool,
    ) -> Option<(&HookArray, bool)> {
        if let Some(hook) = self.hook_array(target, name) {
            return Some((hook, false));
        }
        if !include_inherited {
            return None;
        }
        let inherited = match target {
            TmuxOptionTarget::Session(_) => self.global_hooks.get(name),
            TmuxOptionTarget::Window(_) => self.global_window_hooks.get(name),
            TmuxOptionTarget::Pane(pane) => self
                .state
                .window_for_pane(pane)
                .and_then(|window| self.window_hooks.get(&window))
                .and_then(|hooks| hooks.get(name))
                .or_else(|| self.global_window_hooks.get(name)),
            _ => None,
        }?;
        Some((inherited, true))
    }

    fn hook_array_mut(&mut self, target: TmuxOptionTarget, name: &str) -> Option<&mut HookArray> {
        self.hook_table_mut(target)
            .and_then(|hooks| hooks.get_mut(name))
    }

    fn hook_array_mut_or_insert(&mut self, target: TmuxOptionTarget, name: &str) -> &mut HookArray {
        match target {
            TmuxOptionTarget::GlobalSession => self
                .global_hooks
                .get_mut(name)
                .expect("global session hook table is complete"),
            TmuxOptionTarget::Session(session) => self
                .session_hooks
                .entry(session)
                .or_default()
                .entry(name.to_owned())
                .or_default(),
            TmuxOptionTarget::GlobalWindow => self
                .global_window_hooks
                .get_mut(name)
                .expect("global window hook table is complete"),
            TmuxOptionTarget::Window(window) => self
                .window_hooks
                .entry(window)
                .or_default()
                .entry(name.to_owned())
                .or_default(),
            TmuxOptionTarget::Pane(pane) => self
                .pane_hooks
                .entry(pane)
                .or_default()
                .entry(name.to_owned())
                .or_default(),
            TmuxOptionTarget::Server => unreachable!("hooks are not server scoped"),
        }
    }

    fn hook_table(&self, target: TmuxOptionTarget) -> Option<&HookTable> {
        match target {
            TmuxOptionTarget::GlobalSession => Some(&self.global_hooks),
            TmuxOptionTarget::Session(session) => self.session_hooks.get(&session),
            TmuxOptionTarget::GlobalWindow => Some(&self.global_window_hooks),
            TmuxOptionTarget::Window(window) => self.window_hooks.get(&window),
            TmuxOptionTarget::Pane(pane) => self.pane_hooks.get(&pane),
            TmuxOptionTarget::Server => None,
        }
    }

    fn hook_table_mut(&mut self, target: TmuxOptionTarget) -> Option<&mut HookTable> {
        match target {
            TmuxOptionTarget::GlobalSession => Some(&mut self.global_hooks),
            TmuxOptionTarget::Session(session) => self.session_hooks.get_mut(&session),
            TmuxOptionTarget::GlobalWindow => Some(&mut self.global_window_hooks),
            TmuxOptionTarget::Window(window) => self.window_hooks.get_mut(&window),
            TmuxOptionTarget::Pane(pane) => self.pane_hooks.get_mut(&pane),
            TmuxOptionTarget::Server => None,
        }
    }

    fn unset_hook_array(
        &mut self,
        target: TmuxOptionTarget,
        name: &str,
        index: Option<&ArrayIndex>,
    ) {
        if let Some(index) = index {
            if let Some(hook) = self.hook_array_mut(target, name) {
                hook.remove(index);
            }
        } else if matches!(
            target,
            TmuxOptionTarget::GlobalSession | TmuxOptionTarget::GlobalWindow
        ) {
            self.hook_array_mut(target, name)
                .expect("global hook table is complete")
                .clear();
        } else if let Some(hooks) = self.hook_table_mut(target) {
            hooks.remove(name);
        }
    }

    fn remove_pane_hook_overrides(
        &mut self,
        window: WindowId,
        name: &str,
        index: Option<&ArrayIndex>,
    ) {
        let panes = self
            .state
            .windows
            .get(&window)
            .map(|window| window.pane_order().to_vec())
            .unwrap_or_default();
        for pane in panes {
            self.unset_hook_array(TmuxOptionTarget::Pane(pane), name, index);
        }
    }

    fn scalar_table(&self, target: TmuxOptionTarget) -> Option<&ScalarTable> {
        match target {
            TmuxOptionTarget::Server => Some(&self.stored_scalars.server),
            TmuxOptionTarget::GlobalSession => Some(&self.stored_scalars.global_session),
            TmuxOptionTarget::Session(session) => self.stored_scalars.sessions.get(&session),
            TmuxOptionTarget::GlobalWindow => Some(&self.stored_scalars.global_window),
            TmuxOptionTarget::Window(window) => self.stored_scalars.windows.get(&window),
            TmuxOptionTarget::Pane(pane) => self.stored_scalars.panes.get(&pane),
        }
    }

    fn scalar_table_mut_or_insert(&mut self, target: TmuxOptionTarget) -> &mut ScalarTable {
        match target {
            TmuxOptionTarget::Server => &mut self.stored_scalars.server,
            TmuxOptionTarget::GlobalSession => &mut self.stored_scalars.global_session,
            TmuxOptionTarget::Session(session) => {
                self.stored_scalars.sessions.entry(session).or_default()
            }
            TmuxOptionTarget::GlobalWindow => &mut self.stored_scalars.global_window,
            TmuxOptionTarget::Window(window) => {
                self.stored_scalars.windows.entry(window).or_default()
            }
            TmuxOptionTarget::Pane(pane) => self.stored_scalars.panes.entry(pane).or_default(),
        }
    }

    fn scalar_option_effective(&self, target: TmuxOptionTarget, name: &str) -> Option<&str> {
        if let Some(value) = self
            .scalar_table(target)
            .and_then(|options| options.get(name))
        {
            return Some(value);
        }
        let inherited = match target {
            TmuxOptionTarget::Session(_) => self.stored_scalars.global_session.get(name),
            TmuxOptionTarget::Window(_) => self.stored_scalars.global_window.get(name),
            TmuxOptionTarget::Pane(pane) => self
                .state
                .window_for_pane(pane)
                .and_then(|window| self.stored_scalars.windows.get(&window))
                .and_then(|options| options.get(name))
                .or_else(|| self.stored_scalars.global_window.get(name)),
            TmuxOptionTarget::Server
            | TmuxOptionTarget::GlobalSession
            | TmuxOptionTarget::GlobalWindow => None,
        };
        inherited
            .map(String::as_str)
            .or_else(|| tmux_stored_scalar(name).map(|metadata| metadata.default))
    }

    fn scalar_option_readback(
        &self,
        target: TmuxOptionTarget,
        name: &str,
        include_inherited: bool,
    ) -> Option<(String, bool)> {
        if let Some(value) = self
            .scalar_table(target)
            .and_then(|options| options.get(name))
        {
            return Some((value.clone(), false));
        }
        if matches!(
            target,
            TmuxOptionTarget::Server
                | TmuxOptionTarget::GlobalSession
                | TmuxOptionTarget::GlobalWindow
        ) {
            return tmux_stored_scalar(name).map(|metadata| (metadata.default.to_owned(), false));
        }
        include_inherited
            .then(|| self.scalar_option_effective(target, name))
            .flatten()
            .map(|value| (value.to_owned(), true))
    }

    fn set_stored_scalar_option(
        &mut self,
        name: &'static str,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let metadata = tmux_stored_scalar(name).expect("stored scalar metadata");
        let unset = option_is_unset(options);
        let locally_set = match target {
            TmuxOptionTarget::Server
            | TmuxOptionTarget::GlobalSession
            | TmuxOptionTarget::GlobalWindow => true,
            _ => self
                .scalar_table(target)
                .is_some_and(|values| values.contains_key(name)),
        };
        if options.has("-o") && !unset && locally_set {
            return already_set_or_quiet(options, name);
        }
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
                remove_named_option_override(&mut self.stored_scalars.panes, pane, name);
            }
        }
        if unset {
            match target {
                TmuxOptionTarget::Server => {
                    self.stored_scalars.server.remove(name);
                }
                TmuxOptionTarget::GlobalSession => {
                    self.stored_scalars.global_session.remove(name);
                }
                TmuxOptionTarget::Session(session) => {
                    remove_named_option_override(&mut self.stored_scalars.sessions, session, name);
                }
                TmuxOptionTarget::GlobalWindow => {
                    self.stored_scalars.global_window.remove(name);
                }
                TmuxOptionTarget::Window(window) => {
                    remove_named_option_override(&mut self.stored_scalars.windows, window, name);
                }
                TmuxOptionTarget::Pane(pane) => {
                    remove_named_option_override(&mut self.stored_scalars.panes, pane, name);
                }
            }
            return Ok(stored_scalar_execution(name, target));
        }
        let current = self
            .scalar_option_effective(target, name)
            .expect("stored scalar has a default");
        let appended = options
            .has("-a")
            .then(|| {
                metadata
                    .kind
                    .append_separator()
                    .and_then(|separator| value.map(|value| format!("{current}{separator}{value}")))
            })
            .flatten();
        let next =
            normalize_stored_scalar_value(metadata.kind, appended.as_deref().or(value), current)?;
        self.scalar_table_mut_or_insert(target).insert(name, next);
        Ok(stored_scalar_execution(name, target))
    }

    fn array_table(&self, target: TmuxOptionTarget) -> Option<&ArrayTable> {
        match target {
            TmuxOptionTarget::Server => Some(&self.stored_arrays.server),
            TmuxOptionTarget::GlobalSession => Some(&self.stored_arrays.global_session),
            TmuxOptionTarget::Session(session) => self.stored_arrays.sessions.get(&session),
            TmuxOptionTarget::GlobalWindow => Some(&self.stored_arrays.global_window),
            TmuxOptionTarget::Window(window) => self.stored_arrays.windows.get(&window),
            TmuxOptionTarget::Pane(pane) => self.stored_arrays.panes.get(&pane),
        }
    }

    fn array_table_mut(&mut self, target: TmuxOptionTarget) -> Option<&mut ArrayTable> {
        match target {
            TmuxOptionTarget::Server => Some(&mut self.stored_arrays.server),
            TmuxOptionTarget::GlobalSession => Some(&mut self.stored_arrays.global_session),
            TmuxOptionTarget::Session(session) => self.stored_arrays.sessions.get_mut(&session),
            TmuxOptionTarget::GlobalWindow => Some(&mut self.stored_arrays.global_window),
            TmuxOptionTarget::Window(window) => self.stored_arrays.windows.get_mut(&window),
            TmuxOptionTarget::Pane(pane) => self.stored_arrays.panes.get_mut(&pane),
        }
    }

    fn array_table_mut_or_insert(&mut self, target: TmuxOptionTarget) -> &mut ArrayTable {
        match target {
            TmuxOptionTarget::Server => &mut self.stored_arrays.server,
            TmuxOptionTarget::GlobalSession => &mut self.stored_arrays.global_session,
            TmuxOptionTarget::Session(session) => {
                self.stored_arrays.sessions.entry(session).or_default()
            }
            TmuxOptionTarget::GlobalWindow => &mut self.stored_arrays.global_window,
            TmuxOptionTarget::Window(window) => {
                self.stored_arrays.windows.entry(window).or_default()
            }
            TmuxOptionTarget::Pane(pane) => self.stored_arrays.panes.entry(pane).or_default(),
        }
    }

    fn array_option(&self, target: TmuxOptionTarget, name: &str) -> Option<&StringArray> {
        self.array_table(target)
            .and_then(|options| options.get(name))
    }

    fn array_option_readback(
        &self,
        target: TmuxOptionTarget,
        name: &str,
        include_inherited: bool,
    ) -> Option<(&StringArray, bool)> {
        if let Some(array) = self.array_option(target, name) {
            return Some((array, false));
        }
        if !include_inherited {
            return None;
        }
        let inherited = match target {
            TmuxOptionTarget::Session(_) => self.stored_arrays.global_session.get(name),
            TmuxOptionTarget::Window(_) => self.stored_arrays.global_window.get(name),
            TmuxOptionTarget::Pane(pane) => self
                .state
                .window_for_pane(pane)
                .and_then(|window| self.stored_arrays.windows.get(&window))
                .and_then(|options| options.get(name))
                .or_else(|| self.stored_arrays.global_window.get(name)),
            _ => None,
        }?;
        Some((inherited, true))
    }

    fn set_array_option(
        &mut self,
        target: TmuxOptionTarget,
        name: &'static str,
        index: Option<String>,
        value: Option<&str>,
        options: &Options,
    ) -> Result<Execution, ServerError> {
        let metadata = tmux_stored_array(name).expect("stored array metadata");
        let index = index.map(ArrayIndex::parse);
        let unset = option_is_unset(options);
        let status_format_before =
            (name == "status-format").then(|| self.array_option(target, name).cloned());
        let already = self
            .array_option(target, name)
            .is_some_and(|array| index.as_ref().is_none_or(|index| array.contains_key(index)));
        if options.has("-o") && !unset && already {
            let displayed_index = index.as_ref().map(ArrayIndex::display);
            let option = indexed_option_name(name, displayed_index.as_deref());
            return already_set_or_quiet(options, &option);
        }
        if options.has("-U")
            && let TmuxOptionTarget::Window(window) = target
        {
            self.remove_pane_array_overrides(window, name, index.as_ref());
        }
        if unset {
            let whole = index.is_none();
            self.unset_array_option(target, name, index.as_ref());
            if let Some(before) = status_format_before {
                return Ok(self.status_format_execution(
                    target,
                    before.as_ref(),
                    whole.then_some(false),
                ));
            }
            return Ok(Execution::default());
        }
        let value = value.ok_or_else(|| ServerError::InvalidCommand("empty value".to_owned()))?;
        if let Some(index) = index {
            validate_array_value(metadata.value, value)?;
            let array = self
                .array_table_mut_or_insert(target)
                .entry(name)
                .or_default();
            if options.has("-a") && metadata.value == TmuxArrayValue::String {
                array
                    .entry(index)
                    .and_modify(|current| current.push_str(value))
                    .or_insert_with(|| value.to_owned());
            } else {
                array.insert(index, value.to_owned());
            }
            if let Some(before) = status_format_before {
                return Ok(self.status_format_execution(target, before.as_ref(), Some(true)));
            }
            return Ok(Execution::default());
        }
        let append = options.has("-a");
        if !append {
            self.array_table_mut_or_insert(target)
                .entry(name)
                .or_default()
                .clear();
        }
        for item in value
            .split(|character| metadata.separators.contains(character))
            .filter(|item| !item.is_empty())
        {
            validate_array_value(metadata.value, item)?;
            let array = self
                .array_table_mut_or_insert(target)
                .entry(name)
                .or_default();
            let index = first_free_array_index(array.keys())?;
            array.insert(ArrayIndex::Numeric(index), item.to_owned());
        }
        if let Some(before) = status_format_before {
            return Ok(self.status_format_execution(target, before.as_ref(), Some(true)));
        }
        Ok(Execution::default())
    }

    fn status_format_execution(
        &mut self,
        target: TmuxOptionTarget,
        before: Option<&StringArray>,
        explicit: Option<bool>,
    ) -> Execution {
        let session = match target {
            TmuxOptionTarget::GlobalSession => None,
            TmuxOptionTarget::Session(session) => Some(session),
            _ => return Execution::default(),
        };
        let mut changed = self.array_option(target, "status-format") != before;
        if let Some(explicit) = explicit {
            changed |= self.mark_explicit_status_option(session, "status-format", explicit);
        }
        if changed {
            Execution::effect(MuxEffect::StatusFormatsChanged { session })
        } else {
            Execution::default()
        }
    }

    fn unset_array_option(
        &mut self,
        target: TmuxOptionTarget,
        name: &'static str,
        index: Option<&ArrayIndex>,
    ) {
        if let Some(index) = index {
            if let Some(array) = self
                .array_table_mut(target)
                .and_then(|options| options.get_mut(name))
            {
                array.remove(index);
            }
            return;
        }
        if matches!(
            target,
            TmuxOptionTarget::Server
                | TmuxOptionTarget::GlobalSession
                | TmuxOptionTarget::GlobalWindow
        ) {
            let defaults = default_array(name);
            self.array_table_mut(target)
                .expect("global array table")
                .insert(name, defaults);
        } else if let Some(options) = self.array_table_mut(target) {
            options.remove(name);
        }
    }

    fn remove_pane_array_overrides(
        &mut self,
        window: WindowId,
        name: &'static str,
        index: Option<&ArrayIndex>,
    ) {
        let panes = self
            .state
            .windows
            .get(&window)
            .map(|window| window.pane_order().to_vec())
            .unwrap_or_default();
        for pane in panes {
            self.unset_array_option(TmuxOptionTarget::Pane(pane), name, index);
        }
    }

    fn hook_listing_target(
        &self,
        context: &ExecutionContext,
        options: &Options,
    ) -> Result<TmuxOptionTarget, ServerError> {
        if options.has("-p") {
            return self
                .resolve_pane(options.value("-t"), context.window, context.pane)
                .map(TmuxOptionTarget::Pane);
        }
        if options.has("-w") {
            if options.has("-g") {
                return Ok(TmuxOptionTarget::GlobalWindow);
            }
            return self
                .resolve_option_window(context, options, false)
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

    fn hook_names_for_target(target: TmuxOptionTarget) -> Vec<&'static str> {
        let mut names = tmux_options()
            .filter(|option| tmux_option_is_hook(option.name))
            .filter(|option| match target {
                TmuxOptionTarget::GlobalSession | TmuxOptionTarget::Session(_) => {
                    option.scope == TmuxOptionScope::Session
                }
                TmuxOptionTarget::GlobalWindow | TmuxOptionTarget::Window(_) => matches!(
                    option.scope,
                    TmuxOptionScope::Window | TmuxOptionScope::WindowPane
                ),
                TmuxOptionTarget::Pane(_) => option.scope == TmuxOptionScope::WindowPane,
                TmuxOptionTarget::Server => false,
            })
            .map(|option| option.name)
            .collect::<Vec<_>>();
        names.sort_unstable_by_key(|name| {
            HOOK_NAMES
                .iter()
                .position(|hook| hook == name)
                .expect("hook table entry")
        });
        names
    }

    fn show_hooks(
        &self,
        context: &ExecutionContext,
        args: &[String],
        hooks: &mut impl StatusHooks,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("show-hooks", args)?;
        if options.has("-B") {
            return Err(ServerError::InvalidCommand("invalid flag -B".to_owned()));
        }
        if positional.len() > 1 {
            return Err(ServerError::InvalidCommand("too many arguments".to_owned()));
        }
        let Some(argument) = positional.first() else {
            let target = self.hook_listing_target(context, &options)?;
            let mut lines = Vec::new();
            if let Some(stored) = self.hook_table(target) {
                for name in Self::hook_names_for_target(target) {
                    if let Some(hook) = stored.get(name) {
                        push_shown_hook(&mut lines, name, hook, None);
                    }
                }
            }
            return Ok(Execution::output(lines.join("\n")));
        };
        let (argument, _) = self.expand_hook_name(context, &options, argument, hooks)?;
        let parsed = parse_tmux_option(&argument)
            .map_err(|()| ServerError::InvalidCommand(format!("invalid option: {argument}")))?;
        if parsed.name.starts_with('@') {
            if parsed.index.is_some() {
                return Err(ServerError::InvalidCommand(format!(
                    "not an array: {argument}"
                )));
            }
            let mut forwarded = Vec::new();
            for flag in ["-g", "-p", "-w"] {
                if options.has(flag) {
                    forwarded.push(flag.to_owned());
                }
            }
            if let Some(target) = options.value("-t") {
                forwarded.extend(["-t".to_owned(), target.to_owned()]);
            }
            forwarded.extend(["--".to_owned(), argument]);
            return self.show_options(context, &forwarded, false);
        }
        let table_option = match match_tmux_option(parsed.name) {
            Ok(Some(option)) => option,
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
        if !tmux_option_is_hook(table_option.name) {
            let mut forwarded = Vec::new();
            for flag in ["-g", "-p", "-w"] {
                if options.has(flag) {
                    forwarded.push(flag.to_owned());
                }
            }
            if let Some(target) = options.value("-t") {
                forwarded.extend(["-t".to_owned(), target.to_owned()]);
            }
            forwarded.extend(["--".to_owned(), argument]);
            return self.show_options(context, &forwarded, false);
        }
        let target =
            self.resolve_tmux_option_target(context, &options, false, table_option.scope)?;
        let hook = self.hook_array(target, table_option.name);
        let Some(hook) = hook else {
            return Ok(Execution::default());
        };
        let mut lines = Vec::new();
        let index = parsed.index.map(ArrayIndex::parse);
        push_shown_hook(&mut lines, table_option.name, hook, index.as_ref());
        Ok(Execution::output(lines.join("\n")))
    }

    fn expand_hook_name(
        &self,
        context: &ExecutionContext,
        options: &Options,
        argument: &str,
        hooks: &mut impl StatusHooks,
    ) -> Result<(String, ExecutionContext), ServerError> {
        let target = self.hook_target_context(context, options)?;
        let argument = expand_format_with_hooks(
            argument,
            self,
            FormatContext {
                session: target.session,
                window: target.window,
                pane: target.pane,
                active_session: context.session,
                format_type: FormatType::Pane,
            },
            hooks,
        );
        Ok((argument, target))
    }

    fn hook_target_context(
        &self,
        context: &ExecutionContext,
        options: &Options,
    ) -> Result<ExecutionContext, ServerError> {
        let pane = match options.value("-t") {
            Some(target) => Some(self.resolve_pane(Some(target), context.window, context.pane)?),
            None => context
                .pane
                .filter(|pane| self.state.pane(*pane).is_some())
                .or_else(|| self.resolve_pane(None, context.window, context.pane).ok()),
        };
        Ok(pane
            .and_then(|pane| ExecutionContext::for_pane(&self.state, pane))
            .unwrap_or_else(|| context.clone()))
    }

    fn set_option(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
        force_window: bool,
        hooks: &mut impl StatusHooks,
        default_shell_is_valid: &mut impl FnMut(&str) -> bool,
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
        let format_target = match options.value("-t") {
            Some(target) if force_window => self
                .resolve_window(Some(target), context.session, context.window)
                .ok()
                .and_then(|window| {
                    let pane = self.state.windows.get(&window)?.active_pane;
                    ExecutionContext::for_pane(&self.state, pane)
                })
                .unwrap_or_else(|| context.clone()),
            Some(target) => self
                .resolve_pane(Some(target), context.window, context.pane)
                .ok()
                .and_then(|pane| ExecutionContext::for_pane(&self.state, pane))
                .unwrap_or_else(|| context.clone()),
            None => context.clone(),
        };
        let format_context = FormatContext {
            session: format_target.session,
            window: format_target.window,
            pane: format_target.pane,
            active_session: context.session,
            format_type: FormatType::Pane,
        };
        let option = expand_format_with_hooks(option, self, format_context, hooks);
        let value = positional.get(1).map(|value| {
            if options.has("-F") {
                expand_format_with_hooks(value, self, format_context, hooks)
            } else {
                value.clone()
            }
        });
        let value = value.as_deref();
        let parsed = match parse_tmux_option(&option) {
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
            if tmux_option_is_hook(table_option.name)
                || tmux_stored_array(table_option.name).is_some()
            {
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
                return if tmux_option_is_hook(table_option.name) {
                    self.set_hook_array_option(
                        target,
                        table_option.name,
                        parsed.index,
                        value,
                        &options,
                    )
                } else {
                    self.set_array_option(target, table_option.name, parsed.index, value, &options)
                };
            }
            return Ok(Execution::default());
        }
        if table_option.default.is_none()
            && StatusOption::from_name(table_option.name).is_none()
            && WindowStatusOption::from_name(table_option.name).is_none()
            && ServerOption::from_name(table_option.name).is_none()
            && SessionOption::from_name(table_option.name).is_none()
            && WindowOption::from_name(table_option.name).is_none()
            && PaneOption::from_name(table_option.name).is_none()
            && parsed.index.is_none()
        {
            validate_unimplemented_option_value(table_option.name, value)?;
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
            "aggressive-resize" => self.set_aggressive_resize(value, &options, target),
            "synchronize-panes" => self.set_synchronize_panes(value, &options, target),
            "mouse" => self.set_mouse(value, &options, target),
            "escape-time" => self.set_escape_time(value, &options),
            "automatic-rename" => self.set_automatic_rename(value, &options, target, hooks),
            "automatic-rename-format" => self.set_automatic_rename_format(value, &options, target),
            "remain-on-exit" => self.set_remain_on_exit(value, &options, target),
            "default-terminal" => self.set_default_terminal(value, &options),
            "default-command" | "default-shell" => self.set_spawn_string_option(
                table_option.name,
                value,
                &options,
                target,
                default_shell_is_valid,
            ),
            "display-time" | "initial-repeat-time" | "repeat-time" => {
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
            "popup-style" | "popup-border-style" | "popup-border-lines" => {
                self.set_popup_option(table_option.name, value, &options, target)
            }
            "menu-style" | "menu-selected-style" | "menu-border-style" | "menu-border-lines" => {
                self.set_menu_option(table_option.name, value, &options, target)
            }
            "lock-command" | "lock-after-time" => {
                self.set_lock_option(table_option.name, value, &options, target)
            }
            "prefix" | "set-clipboard" | "copy-command" => {
                self.set_scalar_tmux_option(table_option.name, value, &options)
            }
            option if let Some(option) = StatusOption::from_name(option) => {
                self.set_status_option(option, value, &options, target)
            }
            option if let Some(option) = WindowStatusOption::from_name(option) => {
                self.set_window_status_option(option, value, &options, target)
            }
            option if let Some(option) = ServerOption::from_name(option) => {
                self.set_server_option(option, value, &options, target)
            }
            option if let Some(option) = SessionOption::from_name(option) => {
                self.set_session_option(option, value, &options, target)
            }
            option if let Some(option) = WindowOption::from_name(option) => {
                self.set_window_option(option, value, &options, target)
            }
            option if let Some(option) = PaneOption::from_name(option) => {
                self.set_pane_option(option, value, &options, target)
            }
            option if tmux_stored_scalar(option).is_some() => {
                self.set_stored_scalar_option(option, value, &options, target)
            }
            option => unreachable!("implemented option {option} has a setter"),
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
            for option in tmux_options_for_listing(target) {
                if let Some(metadata) = tmux_stored_array(option.name) {
                    if let Some((array, inherited)) =
                        self.array_option_readback(target, option.name, include_inherited)
                    {
                        push_shown_array(
                            &mut lines,
                            option.name,
                            array,
                            None,
                            metadata.value == TmuxArrayValue::String,
                            inherited,
                            value_only,
                        );
                    }
                } else if let Some((value, inherited)) =
                    self.tmux_option_readback(option, target, include_inherited)?
                {
                    push_shown_option(
                        &mut lines,
                        option.name,
                        &value,
                        tmux_option_value_is_string(option),
                        inherited,
                        value_only,
                    );
                }
            }
            return Ok(Execution::output(shown_options_output(&lines)));
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
                return Ok(Execution::output(shown_options_output(&lines)));
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
            return Ok(Execution::output(shown_options_output(&lines)));
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
            if tmux_option_is_hook(option.name) || tmux_stored_array(option.name).is_some() {
                let target = match self.resolve_tmux_option_target(
                    context,
                    &options,
                    force_window,
                    option.scope,
                ) {
                    Ok(target) => target,
                    Err(_) if options.has("-q") => return Ok(Execution::default()),
                    Err(error) => return Err(error),
                };
                let requested = parsed.index.map(ArrayIndex::parse);
                let mut lines = Vec::new();
                if tmux_option_is_hook(option.name) {
                    if let Some((hook, inherited)) =
                        self.hook_array_readback(target, option.name, include_inherited)
                    {
                        push_shown_hook_option(
                            &mut lines,
                            option.name,
                            hook,
                            requested.as_ref(),
                            inherited,
                            value_only,
                        );
                    }
                } else if let Some((array, inherited)) =
                    self.array_option_readback(target, option.name, include_inherited)
                {
                    let metadata = tmux_stored_array(option.name).expect("stored array metadata");
                    push_shown_array(
                        &mut lines,
                        option.name,
                        array,
                        requested.as_ref(),
                        metadata.value == TmuxArrayValue::String,
                        inherited,
                        value_only,
                    );
                }
                return Ok(Execution::output(shown_options_output(&lines)));
            }
            return Ok(Execution::default());
        }
        if !tmux_option_is_implemented(option) {
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
            tmux_option_value_is_string(option),
            inherited,
            value_only,
        );
        Ok(Execution::output(shown_options_output(&lines)))
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
        if tmux_stored_scalar(option.name).is_some() {
            return Ok(self.scalar_option_readback(target, option.name, include_inherited));
        }
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
                name if SessionOption::from_name(name).is_some() => {
                    let option = SessionOption::from_name(name).expect("guarded session option");
                    self.session_options
                        .get(&session)
                        .and_then(|values| values.get(&option))
                        .map(|value| (value.clone(), false))
                        .or_else(inherited)
                }
                name if StatusOption::from_name(name).is_some() => {
                    let option = StatusOption::from_name(name).expect("guarded status option");
                    self.session_status_options
                        .get(&session)
                        .and_then(|values| values.get(&option))
                        .map(|value| (value.clone(), false))
                        .or_else(inherited)
                }
                "lock-command" => self
                    .session_lock_commands
                    .get(&session)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                "lock-after-time" => self
                    .session_lock_after_times
                    .get(&session)
                    .map(|value| (value.to_string(), false))
                    .or_else(inherited),
                "default-command" => self
                    .session_default_commands
                    .get(&session)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                "default-shell" => self
                    .session_default_shells
                    .get(&session)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
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
                "initial-repeat-time" => self
                    .session_initial_repeat_time_ms
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
                name if WindowOption::from_name(name).is_some() => {
                    let option = WindowOption::from_name(name).expect("guarded window option");
                    self.window_options
                        .get(&window)
                        .and_then(|values| values.get(&option))
                        .map(|value| (value.clone(), false))
                        .or_else(inherited)
                }
                name if PaneOption::from_name(name).is_some() => {
                    let option = PaneOption::from_name(name).expect("guarded pane option");
                    self.window_pane_options
                        .get(&window)
                        .and_then(|values| values.get(&option))
                        .map(|value| (value.clone(), false))
                        .or_else(inherited)
                }
                name if WindowStatusOption::from_name(name).is_some() => {
                    let option =
                        WindowStatusOption::from_name(name).expect("guarded window status option");
                    self.window_status_options
                        .get(&window)
                        .and_then(|values| values.get(&option))
                        .map(|value| (value.clone(), false))
                        .or_else(inherited)
                }
                "aggressive-resize" => self
                    .state
                    .window_aggressive_resize_override(window)?
                    .map(|value| (tmux_flag(value).to_owned(), false))
                    .or_else(inherited),
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
                "popup-style" => self
                    .window_popup_styles
                    .get(&window)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                "popup-border-style" => self
                    .window_popup_border_styles
                    .get(&window)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                "popup-border-lines" => self
                    .window_popup_border_lines
                    .get(&window)
                    .map(|value| (value.as_str().to_owned(), false))
                    .or_else(inherited),
                "menu-style" => self
                    .window_menu_styles
                    .get(&window)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                "menu-selected-style" => self
                    .window_menu_selected_styles
                    .get(&window)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                "menu-border-style" => self
                    .window_menu_border_styles
                    .get(&window)
                    .map(|value| (value.clone(), false))
                    .or_else(inherited),
                "menu-border-lines" => self
                    .window_menu_border_lines
                    .get(&window)
                    .map(|value| (value.as_str().to_owned(), false))
                    .or_else(inherited),
                _ => inherited(),
            },
            TmuxOptionTarget::Pane(pane) => match option.name {
                name if PaneOption::from_name(name).is_some() => {
                    let option = PaneOption::from_name(name).expect("guarded pane option");
                    self.pane_options
                        .get(&pane)
                        .and_then(|values| values.get(&option))
                        .map(|value| (value.clone(), false))
                        .or_else(|| {
                            include_inherited.then(|| (self.pane_knobs(pane).value(option), true))
                        })
                }
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
        if let Some(option) = ServerOption::from_name(name) {
            return Some(self.server_options.value(option));
        }
        if let Some(option) = SessionOption::from_name(name) {
            return Some(self.global_session_options.value(option));
        }
        if let Some(option) = WindowOption::from_name(name) {
            return Some(self.global_window_options.value(option));
        }
        if let Some(option) = PaneOption::from_name(name) {
            return Some(self.global_pane_options.value(option));
        }
        if let Some(option) = StatusOption::from_name(name) {
            return Some(self.status.value(option));
        }
        if let Some(option) = WindowStatusOption::from_name(name) {
            return Some(self.window_status.value(option).to_owned());
        }
        Some(match name {
            "default-command" => self.global_default_command.clone(),
            "default-shell" => self.global_default_shell.clone(),
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
            "initial-repeat-time" => self.global_initial_repeat_time_ms.to_string(),
            "message-limit" => self.message_limit.to_string(),
            "mode-keys" => self.global_mode_keys.as_str().to_owned(),
            "mouse" => tmux_flag(self.global_mouse).to_owned(),
            "pane-base-index" => self.global_pane_base_index.to_string(),
            "prefix" => self.mux_option_value(MuxOptionKey::Prefix),
            "renumber-windows" => tmux_flag(self.global_renumber_windows).to_owned(),
            "repeat-time" => self.global_repeat_time_ms.to_string(),
            "set-clipboard" => self.set_clipboard.as_str().to_owned(),
            "synchronize-panes" => tmux_flag(self.state.global_synchronize_panes()).to_owned(),
            "update-environment" => UPDATE_ENVIRONMENT_DEFAULT.to_owned(),
            "word-separators" => self.global_word_separators.clone(),
            "aggressive-resize" => tmux_flag(self.state.global_aggressive_resize()).to_owned(),
            "automatic-rename" => tmux_flag(self.state.global_automatic_rename()).to_owned(),
            "automatic-rename-format" => self.global_automatic_rename_format.clone(),
            "remain-on-exit" => self.global_remain_on_exit.as_str().to_owned(),
            "popup-style" => self.global_popup_style.clone(),
            "popup-border-style" => self.global_popup_border_style.clone(),
            "popup-border-lines" => self.global_popup_border_lines.as_str().to_owned(),
            "menu-style" => self.global_menu_style.clone(),
            "menu-selected-style" => self.global_menu_selected_style.clone(),
            "menu-border-style" => self.global_menu_border_style.clone(),
            "menu-border-lines" => self.global_menu_border_lines.as_str().to_owned(),
            "lock-command" => self.global_lock_command.clone(),
            "lock-after-time" => self.global_lock_after_time.to_string(),
            name => {
                let target = match match_tmux_option(name).ok().flatten()?.scope {
                    TmuxOptionScope::Server => TmuxOptionTarget::Server,
                    TmuxOptionScope::Session => TmuxOptionTarget::GlobalSession,
                    TmuxOptionScope::Window | TmuxOptionScope::WindowPane => {
                        TmuxOptionTarget::GlobalWindow
                    }
                };
                return self
                    .scalar_option_effective(target, name)
                    .map(str::to_owned);
            }
        })
    }

    fn set_popup_option(
        &mut self,
        option: &str,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        match option {
            "popup-style" | "popup-border-style" => {
                let default = PopupOptions::default();
                let (global, locals, local_key) = if option == "popup-style" {
                    (
                        &mut self.global_popup_style,
                        &mut self.window_popup_styles,
                        match target {
                            TmuxOptionTarget::Window(window) => Some(window),
                            TmuxOptionTarget::GlobalWindow => None,
                            _ => unreachable!("popup style is window scoped"),
                        },
                    )
                } else {
                    (
                        &mut self.global_popup_border_style,
                        &mut self.window_popup_border_styles,
                        match target {
                            TmuxOptionTarget::Window(window) => Some(window),
                            TmuxOptionTarget::GlobalWindow => None,
                            _ => unreachable!("popup style is window scoped"),
                        },
                    )
                };
                if options.has("-o")
                    && !unset
                    && local_key.is_none_or(|key| locals.contains_key(&key))
                {
                    return already_set_or_quiet(options, option);
                }
                if unset {
                    if let Some(key) = local_key {
                        locals.remove(&key);
                    } else {
                        *global = if option == "popup-style" {
                            default.style
                        } else {
                            default.border_style
                        };
                    }
                    return Ok(Execution::default());
                }
                let next = {
                    let value = value.ok_or_else(|| {
                        ServerError::InvalidCommand(format!("set-option {option} needs a value"))
                    })?;
                    if options.has("-a") {
                        let existing = local_key.and_then(|key| locals.get(&key)).unwrap_or(global);
                        format!("{existing}{value}")
                    } else {
                        value.to_owned()
                    }
                };
                if !valid_style(&next) {
                    return Err(ServerError::InvalidCommand(format!(
                        "invalid style: {next}"
                    )));
                }
                if let Some(key) = local_key {
                    locals.insert(key, next);
                } else {
                    *global = next;
                }
            }
            "popup-border-lines" => {
                let local_key = match target {
                    TmuxOptionTarget::Window(window) => Some(window),
                    TmuxOptionTarget::GlobalWindow => None,
                    _ => unreachable!("popup border lines is window scoped"),
                };
                if options.has("-o")
                    && !unset
                    && local_key.is_none_or(|key| self.window_popup_border_lines.contains_key(&key))
                {
                    return already_set_or_quiet(options, option);
                }
                if unset {
                    if let Some(key) = local_key {
                        self.window_popup_border_lines.remove(&key);
                    } else {
                        self.global_popup_border_lines = PopupBorderLines::Single;
                    }
                    return Ok(Execution::default());
                }
                let next = {
                    let value = value.ok_or_else(|| {
                        ServerError::InvalidCommand(
                            "set-option popup-border-lines needs a value".to_owned(),
                        )
                    })?;
                    value.parse().map_err(|()| {
                        ServerError::InvalidCommand(format!("unknown value: {value}"))
                    })?
                };
                if let Some(key) = local_key {
                    self.window_popup_border_lines.insert(key, next);
                } else {
                    self.global_popup_border_lines = next;
                }
            }
            _ => unreachable!("popup option is catalogued"),
        }
        Ok(Execution::default())
    }

    fn set_menu_option(
        &mut self,
        option: &str,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        let local_key = match target {
            TmuxOptionTarget::Window(window) => Some(window),
            TmuxOptionTarget::GlobalWindow => None,
            _ => unreachable!("menu options are window scoped"),
        };
        if option == "menu-border-lines" {
            if options.has("-o")
                && !unset
                && local_key.is_none_or(|key| self.window_menu_border_lines.contains_key(&key))
            {
                return already_set_or_quiet(options, option);
            }
            if unset {
                if let Some(key) = local_key {
                    self.window_menu_border_lines.remove(&key);
                } else {
                    self.global_menu_border_lines = PopupBorderLines::Single;
                }
                return Ok(Execution::default());
            }
            let value = value.ok_or_else(|| {
                ServerError::InvalidCommand(format!("set-option {option} needs a value"))
            })?;
            let next = value
                .parse()
                .map_err(|()| ServerError::InvalidCommand(format!("unknown value: {value}")))?;
            if let Some(key) = local_key {
                self.window_menu_border_lines.insert(key, next);
            } else {
                self.global_menu_border_lines = next;
            }
            return Ok(Execution::default());
        }
        let defaults = MenuOptions::default();
        let (global, locals, default) = match option {
            "menu-style" => (
                &mut self.global_menu_style,
                &mut self.window_menu_styles,
                defaults.style,
            ),
            "menu-selected-style" => (
                &mut self.global_menu_selected_style,
                &mut self.window_menu_selected_styles,
                defaults.selected_style,
            ),
            "menu-border-style" => (
                &mut self.global_menu_border_style,
                &mut self.window_menu_border_styles,
                defaults.border_style,
            ),
            _ => unreachable!("menu option is catalogued"),
        };
        if options.has("-o") && !unset && local_key.is_none_or(|key| locals.contains_key(&key)) {
            return already_set_or_quiet(options, option);
        }
        if unset {
            if let Some(key) = local_key {
                locals.remove(&key);
            } else {
                *global = default;
            }
            return Ok(Execution::default());
        }
        let value = value.ok_or_else(|| {
            ServerError::InvalidCommand(format!("set-option {option} needs a value"))
        })?;
        let next = if options.has("-a") {
            format!(
                "{}{value}",
                local_key.and_then(|key| locals.get(&key)).unwrap_or(global)
            )
        } else {
            value.to_owned()
        };
        if !valid_style(&next) {
            return Err(ServerError::InvalidCommand(format!(
                "invalid style: {next}"
            )));
        }
        if let Some(key) = local_key {
            locals.insert(key, next);
        } else {
            *global = next;
        }
        Ok(Execution::default())
    }

    fn set_lock_option(
        &mut self,
        option: &str,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        let local_key = match target {
            TmuxOptionTarget::Session(session) => Some(session),
            TmuxOptionTarget::GlobalSession => None,
            _ => unreachable!("lock options are session scoped"),
        };
        if option == "lock-after-time" {
            if options.has("-o")
                && !unset
                && local_key.is_none_or(|key| self.session_lock_after_times.contains_key(&key))
            {
                return already_set_or_quiet(options, option);
            }
            let next = if unset {
                0
            } else {
                parse_index_option(
                    value.ok_or_else(|| {
                        ServerError::InvalidCommand(format!("set-option {option} needs a value"))
                    })?,
                    i32::MAX.cast_unsigned(),
                )?
            };
            if let Some(key) = local_key {
                if unset {
                    self.session_lock_after_times.remove(&key);
                } else {
                    self.session_lock_after_times.insert(key, next);
                }
            } else {
                self.global_lock_after_time = next;
            }
            return Ok(Execution::default());
        }
        if options.has("-o")
            && !unset
            && local_key.is_none_or(|key| self.session_lock_commands.contains_key(&key))
        {
            return already_set_or_quiet(options, option);
        }
        if unset {
            if let Some(key) = local_key {
                self.session_lock_commands.remove(&key);
            } else {
                "lock -np".clone_into(&mut self.global_lock_command);
            }
            return Ok(Execution::default());
        }
        let value = value.ok_or_else(|| {
            ServerError::InvalidCommand(format!("set-option {option} needs a value"))
        })?;
        let next = if options.has("-a") {
            format!(
                "{}{value}",
                local_key
                    .and_then(|key| self.session_lock_commands.get(&key))
                    .unwrap_or(&self.global_lock_command)
            )
        } else {
            value.to_owned()
        };
        if let Some(key) = local_key {
            self.session_lock_commands.insert(key, next);
        } else {
            self.global_lock_command = next;
        }
        Ok(Execution::default())
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
            .find(|flag| !matches!(flag.as_str(), "-F" | "-g" | "-o" | "-q"))
        {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {flag} {option}"
            )));
        }
        if options.has("-o") {
            return already_set_or_quiet(options, option);
        }
        let changed = match option {
            "experimental-agent-pane" => {
                self.experimental_agent_pane =
                    parse_flag_value(value, self.experimental_agent_pane)?;
                MuxOptionKey::ExperimentalAgentPane
            }
            "experimental-editor-pane" => {
                self.experimental_editor_pane =
                    parse_flag_value(value, self.experimental_editor_pane)?;
                MuxOptionKey::ExperimentalEditorPane
            }
            "agent-command" | "agent-claude-code-command" => {
                let value = value.ok_or_else(|| {
                    ServerError::InvalidCommand(format!("set-option {option} needs a value"))
                })?;
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
                self.agent.auto_approve = parse_flag_value(value, self.agent.auto_approve)?;
                MuxOptionKey::AgentAutoApprove
            }
            _ => unreachable!("native option is catalogued"),
        };
        Ok(Execution::effect(MuxEffect::MuxOptionChanged {
            option: changed,
            session: None,
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
                if !valid_option_key(value) {
                    return Err(ServerError::InvalidCommand(format!("bad key: {value}")));
                }
                self.keys.set_prefix(value);
                MuxOptionKey::Prefix
            }
            "set-clipboard" => {
                self.set_clipboard = if unset {
                    SetClipboard::default()
                } else {
                    match value {
                        None | Some("") => self.set_clipboard.toggled(),
                        Some(value) => match value {
                            "on" => SetClipboard::On,
                            "external" => SetClipboard::External,
                            "off" => SetClipboard::Off,
                            value => {
                                return Err(ServerError::InvalidCommand(format!(
                                    "invalid set-clipboard value: {value}"
                                )));
                            }
                        },
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
            session: None,
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
                    session: None,
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
            .find(|flag| !matches!(flag.as_str(), "-F" | "-g" | "-o" | "-q" | "-u"))
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
            session: None,
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
                    session: None,
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

    fn set_status_option(
        &mut self,
        option: StatusOption,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        let session = match target {
            TmuxOptionTarget::GlobalSession => None,
            TmuxOptionTarget::Session(session) => Some(session),
            _ => unreachable!("status options have session scope"),
        };
        let already_set = session.is_none_or(|session| {
            self.session_status_options
                .get(&session)
                .is_some_and(|values| values.contains_key(&option))
        });
        if options.has("-o") && !unset && already_set {
            return already_set_or_quiet(options, option.as_str());
        }
        let previous = self.status_formats_for_session(session);
        if unset {
            if let Some(session) = session {
                if let Some(values) = self.session_status_options.get_mut(&session) {
                    values.remove(&option);
                    if values.is_empty() {
                        self.session_status_options.remove(&session);
                    }
                }
            } else {
                self.status
                    .set(option, None)
                    .map_err(ServerError::InvalidCommand)?;
            }
            let changed = self.status_formats_for_session(session) != previous;
            let unmarked = self.mark_explicit_status_option(session, option.as_str(), false);
            return Ok(if changed || unmarked {
                Execution::effect(MuxEffect::StatusFormatsChanged { session })
            } else {
                Execution::default()
            });
        }

        let mut next = previous.clone();
        let toggled = if option == StatusOption::Enabled && value.is_none_or(str::is_empty) {
            next.toggle_enabled_choice();
            Some(next.value(option))
        } else if option == StatusOption::Justify && value.is_none() {
            Some(
                match next.justify {
                    crate::StatusJustify::Left => crate::StatusJustify::Centre,
                    crate::StatusJustify::Centre => crate::StatusJustify::Left,
                    value => value,
                }
                .as_str()
                .to_owned(),
            )
        } else if option == StatusOption::Position && value.is_none() {
            Some(
                match next.position {
                    crate::StatusPosition::Top => crate::StatusPosition::Bottom,
                    crate::StatusPosition::Bottom => crate::StatusPosition::Top,
                }
                .as_str()
                .to_owned(),
            )
        } else {
            None
        };
        let value = toggled.as_deref().or(value);
        let appended = (!unset && options.has("-a"))
            .then(|| previous.format(option))
            .flatten()
            .zip(value)
            .map(|(current, value)| {
                let separator = if option.is_style() && !current.is_empty() && !value.is_empty() {
                    ","
                } else {
                    ""
                };
                format!("{current}{separator}{value}")
            });
        let value = appended.as_deref().or(value).ok_or_else(|| {
            ServerError::InvalidCommand(format!("set-option {} needs a value", option.as_str()))
        })?;
        next.set(option, Some(value))
            .map_err(ServerError::InvalidCommand)?;
        let changed = next != previous;
        if let Some(session) = session {
            self.session_status_options
                .entry(session)
                .or_default()
                .insert(option, next.value(option));
        } else {
            self.status = next;
        }
        let marked = self.mark_explicit_status_option(session, option.as_str(), true);
        Ok(if changed || marked {
            Execution::effect(MuxEffect::StatusFormatsChanged { session })
        } else {
            Execution::default()
        })
    }

    fn set_window_status_option(
        &mut self,
        option: WindowStatusOption,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        let window = match target {
            TmuxOptionTarget::GlobalWindow => None,
            TmuxOptionTarget::Window(window) => Some(window),
            _ => unreachable!("window status options have window scope"),
        };
        let already_set = window.is_none_or(|window| {
            self.window_status_options
                .get(&window)
                .is_some_and(|values| values.contains_key(&option))
        });
        if options.has("-o") && !unset && already_set {
            return already_set_or_quiet(options, option.as_str());
        }
        let previous = window.map_or_else(
            || self.window_status.clone(),
            |window| self.window_status_formats(window),
        );
        if unset {
            if let Some(window) = window {
                if let Some(values) = self.window_status_options.get_mut(&window) {
                    values.remove(&option);
                    if values.is_empty() {
                        self.window_status_options.remove(&window);
                    }
                }
            } else {
                self.window_status
                    .set(option, None)
                    .map_err(ServerError::InvalidCommand)?;
            }
        } else {
            let value = value.ok_or_else(|| {
                ServerError::InvalidCommand(format!("set-option {} needs a value", option.as_str()))
            })?;
            let appended = options.has("-a").then(|| {
                let current = previous.value(option);
                let separator = if option.is_style() && !current.is_empty() && !value.is_empty() {
                    ","
                } else {
                    ""
                };
                format!("{current}{separator}{value}")
            });
            let value = appended.as_deref().unwrap_or(value);
            let mut next = previous.clone();
            next.set(option, Some(value))
                .map_err(ServerError::InvalidCommand)?;
            if let Some(window) = window {
                self.window_status_options
                    .entry(window)
                    .or_default()
                    .insert(option, next.value(option).to_owned());
            } else {
                self.window_status = next;
            }
        }
        let next = window.map_or_else(
            || self.window_status.clone(),
            |window| self.window_status_formats(window),
        );
        if next == previous {
            Ok(Execution::default())
        } else {
            self.state.bump_generation();
            Ok(Execution::effect(MuxEffect::SnapshotChanged))
        }
    }

    fn set_server_option(
        &mut self,
        option: ServerOption,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        assert_eq!(target, TmuxOptionTarget::Server);
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            return already_set_or_quiet(options, option.as_str());
        }
        if unset {
            self.server_options.reset(option);
            return Ok(Execution::default());
        }
        let normalized = match option {
            ServerOption::Backspace => value
                .map(|value| {
                    if valid_option_key(value) {
                        Ok(canonical_key(value))
                    } else {
                        Err(ServerError::InvalidCommand(format!("bad key: {value}")))
                    }
                })
                .transpose()?,
            ServerOption::DefaultClientCommand => {
                value.map(normalize_option_command).transpose()?
            }
            _ => None,
        };
        let value = normalized.as_deref().or(value);
        let appended = (options.has("-a"))
            .then(|| {
                option.append_separator().and_then(|separator| {
                    value.map(|value| {
                        format!("{}{separator}{value}", self.server_options.value(option))
                    })
                })
            })
            .flatten();
        self.server_options
            .set_command(option, appended.as_deref().or(value))
            .map_err(ServerError::InvalidCommand)?;
        Ok(Execution::default())
    }

    fn set_session_option(
        &mut self,
        option: SessionOption,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        let session = match target {
            TmuxOptionTarget::GlobalSession => None,
            TmuxOptionTarget::Session(session) => Some(session),
            _ => unreachable!("session options have session scope"),
        };
        let locally_set = session.is_none_or(|session| {
            self.session_options
                .get(&session)
                .is_some_and(|values| values.contains_key(&option))
        });
        if options.has("-o") && !unset && locally_set {
            return already_set_or_quiet(options, option.as_str());
        }
        let message_line_before =
            (option == SessionOption::MessageLine).then(|| self.message_line_for_session(session));
        if unset {
            if let Some(session) = session {
                remove_option_override(&mut self.session_options, session, option);
            } else {
                self.global_session_options.reset(option);
            }
            return Ok(self.session_option_execution(session, message_line_before));
        }
        let previous = session.map_or_else(
            || self.global_session_options.clone(),
            |session| self.session_knobs(session),
        );
        let appended = options
            .has("-a")
            .then(|| {
                option.append_separator().and_then(|separator| {
                    value.map(|value| format!("{}{separator}{value}", previous.value(option)))
                })
            })
            .flatten();
        let mut next = previous;
        next.set_command(option, appended.as_deref().or(value))
            .map_err(ServerError::InvalidCommand)?;
        if let Some(session) = session {
            self.session_options
                .entry(session)
                .or_default()
                .insert(option, next.value(option));
        } else {
            self.global_session_options = next;
        }
        Ok(self.session_option_execution(session, message_line_before))
    }

    fn session_option_execution(
        &self,
        session: Option<SessionId>,
        message_line_before: Option<u8>,
    ) -> Execution {
        if message_line_before
            .is_some_and(|before| self.message_line_for_session(session) != before)
        {
            Execution::effect(MuxEffect::StatusFormatsChanged { session })
        } else {
            Execution::default()
        }
    }

    fn set_window_option(
        &mut self,
        option: WindowOption,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        let window = match target {
            TmuxOptionTarget::GlobalWindow => None,
            TmuxOptionTarget::Window(window) => Some(window),
            _ => unreachable!("window options have window scope"),
        };
        let locally_set = window.is_none_or(|window| {
            self.window_options
                .get(&window)
                .is_some_and(|values| values.contains_key(&option))
        });
        if options.has("-o") && !unset && locally_set {
            return already_set_or_quiet(options, option.as_str());
        }
        let previous = window.map_or_else(
            || self.global_window_options.clone(),
            |window| self.window_knobs(window),
        );
        if unset {
            if let Some(window) = window {
                remove_option_override(&mut self.window_options, window, option);
            } else {
                self.global_window_options.reset(option);
            }
        } else {
            let appended = options
                .has("-a")
                .then(|| {
                    option.append_separator().and_then(|separator| {
                        value.map(|value| format!("{}{separator}{value}", previous.value(option)))
                    })
                })
                .flatten();
            let mut next = previous.clone();
            next.set_command(option, appended.as_deref().or(value))
                .map_err(ServerError::InvalidCommand)?;
            if let Some(window) = window {
                self.window_options
                    .entry(window)
                    .or_default()
                    .insert(option, next.value(option));
            } else {
                self.global_window_options = next;
            }
        }
        let next = window.map_or_else(
            || self.global_window_options.clone(),
            |window| self.window_knobs(window),
        );
        if next == previous {
            return Ok(Execution::default());
        }
        match option {
            WindowOption::WindowSize => {
                Ok(Execution::effect(MuxEffect::WindowSizeChanged { window }))
            }
            WindowOption::WrapSearch => Ok(Execution::effect(MuxEffect::TerminalKnobsChanged {
                window,
                pane: None,
            })),
            _ => Ok(Execution::default()),
        }
    }

    fn set_pane_option(
        &mut self,
        option: PaneOption,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        let locally_set = match target {
            TmuxOptionTarget::GlobalWindow => true,
            TmuxOptionTarget::Window(window) => self
                .window_pane_options
                .get(&window)
                .is_some_and(|values| values.contains_key(&option)),
            TmuxOptionTarget::Pane(pane) => self
                .pane_options
                .get(&pane)
                .is_some_and(|values| values.contains_key(&option)),
            _ => unreachable!("pane options have window-pane scope"),
        };
        if options.has("-o") && !unset && locally_set {
            return already_set_or_quiet(options, option.as_str());
        }
        let pane_overrides_removed = match target {
            TmuxOptionTarget::Window(window) if options.has("-U") => {
                self.remove_pane_option_overrides(window, option)
            }
            _ => false,
        };
        let previous = match target {
            TmuxOptionTarget::GlobalWindow => self.global_pane_options.clone(),
            TmuxOptionTarget::Window(window) => self.pane_knobs_for_window(window),
            TmuxOptionTarget::Pane(pane) => self.pane_knobs(pane),
            _ => unreachable!("pane options have window-pane scope"),
        };
        if unset {
            match target {
                TmuxOptionTarget::GlobalWindow => {
                    self.global_pane_options.reset(option);
                }
                TmuxOptionTarget::Window(window) => {
                    remove_option_override(&mut self.window_pane_options, window, option);
                }
                TmuxOptionTarget::Pane(pane) => {
                    remove_option_override(&mut self.pane_options, pane, option);
                }
                _ => unreachable!("pane options have window-pane scope"),
            }
        } else {
            let appended = options
                .has("-a")
                .then(|| {
                    option.append_separator().and_then(|separator| {
                        value.map(|value| format!("{}{separator}{value}", previous.value(option)))
                    })
                })
                .flatten();
            let mut next = previous.clone();
            next.set_command(option, appended.as_deref().or(value))
                .map_err(ServerError::InvalidCommand)?;
            match target {
                TmuxOptionTarget::GlobalWindow => self.global_pane_options = next,
                TmuxOptionTarget::Window(window) => {
                    self.window_pane_options
                        .entry(window)
                        .or_default()
                        .insert(option, next.value(option));
                }
                TmuxOptionTarget::Pane(pane) => {
                    self.pane_options
                        .entry(pane)
                        .or_default()
                        .insert(option, next.value(option));
                }
                _ => unreachable!("pane options have window-pane scope"),
            }
        }
        let next = match target {
            TmuxOptionTarget::GlobalWindow => self.global_pane_options.clone(),
            TmuxOptionTarget::Window(window) => self.pane_knobs_for_window(window),
            TmuxOptionTarget::Pane(pane) => self.pane_knobs(pane),
            _ => unreachable!("pane options have window-pane scope"),
        };
        if next == previous && !pane_overrides_removed || !option.updates_terminal_worker() {
            return Ok(Execution::default());
        }
        let (window, pane) = match target {
            TmuxOptionTarget::GlobalWindow => (None, None),
            TmuxOptionTarget::Window(window) => (Some(window), None),
            TmuxOptionTarget::Pane(pane) => (None, Some(pane)),
            _ => unreachable!("pane options have window-pane scope"),
        };
        Ok(Execution::effect(MuxEffect::TerminalKnobsChanged {
            window,
            pane,
        }))
    }

    fn remove_pane_option_overrides(&mut self, window: WindowId, option: PaneOption) -> bool {
        let panes = self
            .state
            .windows
            .get(&window)
            .map(|window| window.pane_order().to_vec())
            .unwrap_or_default();
        let mut removed = false;
        for pane in panes {
            removed |= self
                .pane_options
                .get(&pane)
                .is_some_and(|values| values.contains_key(&option));
            remove_option_override(&mut self.pane_options, pane, option);
        }
        removed
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
                        session: None,
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
                        session: None,
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
                    session: None,
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
        let scope = match target {
            TmuxOptionTarget::GlobalSession => {
                self.global_mouse = if unset {
                    DEFAULT_MOUSE
                } else {
                    parse_tmux_flag_value(value, self.global_mouse)?
                };
                None
            }
            TmuxOptionTarget::Session(session) => {
                if unset {
                    self.session_mouse.remove(&session);
                } else {
                    let next = parse_tmux_flag_value(value, self.mouse_for_session(session))?;
                    self.session_mouse.insert(session, next);
                }
                Some(session)
            }
            _ => unreachable!("mouse has session scope"),
        };
        Ok(Execution::effect(MuxEffect::MuxOptionChanged {
            option: MuxOptionKey::Mouse,
            session: scope,
        }))
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
        Ok(Execution::effect(MuxEffect::MuxOptionChanged {
            option: MuxOptionKey::EscapeTime,
            session: None,
        }))
    }

    fn set_automatic_rename(
        &mut self,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
        hooks: &mut impl StatusHooks,
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
        let panes = match target {
            TmuxOptionTarget::GlobalWindow => self
                .state
                .windows
                .values()
                .filter(|window| {
                    self.state
                        .window_automatic_rename(window.id)
                        .unwrap_or_default()
                })
                .map(|window| window.active_pane)
                .collect::<Vec<_>>(),
            TmuxOptionTarget::Window(window)
                if self
                    .state
                    .window_automatic_rename(window)
                    .unwrap_or_default() =>
            {
                vec![self.state.windows[&window].active_pane]
            }
            TmuxOptionTarget::Window(_) => Vec::new(),
            _ => unreachable!("automatic-rename has window scope"),
        };
        for pane in panes {
            self.refresh_automatic_window_name_for_pane(pane, hooks);
        }
        Ok(Execution::default())
    }

    fn set_aggressive_resize(
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
                    .window_aggressive_resize_override(window)?
                    .is_some(),
                _ => unreachable!("aggressive-resize has window scope"),
            };
            if already {
                return already_set_or_quiet(options, "aggressive-resize");
            }
        }
        let window = match target {
            TmuxOptionTarget::GlobalWindow => {
                let next = if unset {
                    None
                } else {
                    Some(parse_tmux_flag_value(
                        value,
                        self.state.global_aggressive_resize(),
                    )?)
                };
                self.state.set_global_aggressive_resize(next);
                None
            }
            TmuxOptionTarget::Window(window) => {
                let next = if unset {
                    None
                } else {
                    Some(parse_tmux_flag_value(
                        value,
                        self.state.window_aggressive_resize(window)?,
                    )?)
                };
                self.state.set_window_aggressive_resize(window, next)?;
                Some(window)
            }
            _ => unreachable!("aggressive-resize has window scope"),
        };
        Ok(Execution::effect(MuxEffect::AggressiveResizeChanged {
            window,
        }))
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
                    parse_remain_on_exit(value, self.global_remain_on_exit)?
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
                    let next = parse_remain_on_exit(value, self.remain_on_exit_for_window(window))?;
                    self.window_remain_on_exit.insert(window, next);
                }
            }
            TmuxOptionTarget::Pane(pane) => {
                if unset {
                    self.pane_remain_on_exit.remove(&pane);
                } else {
                    let next = parse_remain_on_exit(value, self.remain_on_exit_for_pane(pane)?)?;
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

    fn set_spawn_string_option(
        &mut self,
        option: &str,
        value: Option<&str>,
        options: &Options,
        target: TmuxOptionTarget,
        default_shell_is_valid: &mut impl FnMut(&str) -> bool,
    ) -> Result<Execution, ServerError> {
        let unset = option_is_unset(options);
        if options.has("-o") && !unset {
            let already = match (option, target) {
                ("default-command" | "default-shell", TmuxOptionTarget::GlobalSession) => true,
                ("default-command", TmuxOptionTarget::Session(session)) => {
                    self.session_default_commands.contains_key(&session)
                }
                ("default-shell", TmuxOptionTarget::Session(session)) => {
                    self.session_default_shells.contains_key(&session)
                }
                _ => unreachable!("spawn strings have session scope"),
            };
            if already {
                return already_set_or_quiet(options, option);
            }
        }
        let value = if unset {
            None
        } else {
            Some(value.ok_or_else(|| {
                ServerError::InvalidCommand(format!("set-option {option} needs a value"))
            })?)
        };
        match (option, target) {
            ("default-command", TmuxOptionTarget::GlobalSession) => {
                if let Some(value) = value {
                    if options.has("-a") {
                        self.global_default_command.push_str(value);
                    } else {
                        value.clone_into(&mut self.global_default_command);
                    }
                } else {
                    self.global_default_command.clear();
                }
            }
            ("default-command", TmuxOptionTarget::Session(session)) => {
                if let Some(value) = value {
                    let next = if options.has("-a") {
                        format!(
                            "{}{value}",
                            self.session_default_commands
                                .get(&session)
                                .map(String::as_str)
                                .unwrap_or_default()
                        )
                    } else {
                        value.to_owned()
                    };
                    self.session_default_commands.insert(session, next);
                } else {
                    self.session_default_commands.remove(&session);
                }
            }
            ("default-shell", TmuxOptionTarget::GlobalSession) => {
                if let Some(value) = value {
                    let next = if options.has("-a") {
                        format!("{}{value}", self.global_default_shell)
                    } else {
                        value.to_owned()
                    };
                    if !default_shell_is_valid(&next) {
                        return Err(ServerError::InvalidCommand(format!(
                            "not a suitable shell: {next}"
                        )));
                    }
                    self.global_default_shell = next;
                } else {
                    DEFAULT_SHELL.clone_into(&mut self.global_default_shell);
                }
            }
            ("default-shell", TmuxOptionTarget::Session(session)) => {
                if let Some(value) = value {
                    let next = if options.has("-a") {
                        format!(
                            "{}{value}",
                            self.session_default_shells
                                .get(&session)
                                .map(String::as_str)
                                .unwrap_or_default()
                        )
                    } else {
                        value.to_owned()
                    };
                    if !default_shell_is_valid(&next) {
                        return Err(ServerError::InvalidCommand(format!(
                            "not a suitable shell: {next}"
                        )));
                    }
                    self.session_default_shells.insert(session, next);
                } else {
                    self.session_default_shells.remove(&session);
                }
            }
            _ => unreachable!("spawn strings have session scope"),
        }
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
                (
                    "display-time" | "initial-repeat-time" | "repeat-time",
                    TmuxOptionTarget::GlobalSession,
                ) => true,
                ("display-time", TmuxOptionTarget::Session(session)) => {
                    self.session_display_time_ms.contains_key(&session)
                }
                ("initial-repeat-time", TmuxOptionTarget::Session(session)) => {
                    self.session_initial_repeat_time_ms.contains_key(&session)
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
        let maximum = if matches!(option, "initial-repeat-time" | "repeat-time") {
            MAX_REPEAT_TIME_MS
        } else {
            i32::MAX.cast_unsigned()
        };
        let default = match option {
            "initial-repeat-time" => DEFAULT_INITIAL_REPEAT_TIME_MS,
            "repeat-time" => DEFAULT_REPEAT_TIME_MS,
            _ => DEFAULT_DISPLAY_TIME_MS,
        };
        let parsed = if unset {
            default
        } else {
            parse_index_option(
                value.ok_or_else(|| ServerError::InvalidCommand("empty value".to_owned()))?,
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
            ("initial-repeat-time", TmuxOptionTarget::GlobalSession) => {
                self.global_initial_repeat_time_ms = parsed;
            }
            ("initial-repeat-time", TmuxOptionTarget::Session(session)) => {
                if unset {
                    self.session_initial_repeat_time_ms.remove(&session);
                } else {
                    self.session_initial_repeat_time_ms.insert(session, parsed);
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
            context.retarget(&valid);
        } else if let Some((session, window, pane)) = self.state.default_context() {
            context.retarget(&ExecutionContext::new(
                Some(session),
                Some(window),
                Some(pane),
            ));
        } else {
            context.session = None;
            context.window = None;
            context.pane = None;
        }
    }

    pub fn repair_event_context(&self, context: &mut ExecutionContext) {
        if let Some(pane) = context.pane
            && let Some(valid) = ExecutionContext::for_pane(&self.state, pane)
        {
            context.retarget(&valid);
            return;
        }
        if let Some(window) = context.window
            && let Some(window_state) = self.state.windows.get(&window)
        {
            context.retarget(&ExecutionContext::new(
                Some(window_state.session),
                Some(window),
                Some(window_state.active_pane),
            ));
            return;
        }
        if let Some(session) = context.session
            && let Some(session_state) = self.state.sessions.get(&session)
            && let Some(window_state) = self.state.windows.get(&session_state.active_window)
        {
            context.retarget(&ExecutionContext::new(
                Some(session),
                Some(session_state.active_window),
                Some(window_state.active_pane),
            ));
            return;
        }
        self.repair_context(context);
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

fn remove_option_override<K: Copy + Ord, O: Copy + Ord>(
    values: &mut BTreeMap<K, BTreeMap<O, String>>,
    target: K,
    option: O,
) {
    if let Some(options) = values.get_mut(&target) {
        options.remove(&option);
        if options.is_empty() {
            values.remove(&target);
        }
    }
}

fn stored_scalar_execution(name: &str, target: TmuxOptionTarget) -> Execution {
    if name == "prefix2" && target == TmuxOptionTarget::GlobalSession {
        return Execution::effect(MuxEffect::MuxOptionChanged {
            option: MuxOptionKey::Prefix2,
            session: None,
        });
    }
    if matches!(name, "set-titles" | "set-titles-string") {
        let session = match target {
            TmuxOptionTarget::Session(session) => Some(session),
            _ => None,
        };
        return Execution::effect(MuxEffect::StatusFormatsChanged { session });
    }
    Execution::default()
}

fn remove_named_option_override<K: Copy + Ord>(
    values: &mut BTreeMap<K, ScalarTable>,
    target: K,
    option: &str,
) {
    if let Some(options) = values.get_mut(&target) {
        options.remove(option);
        if options.is_empty() {
            values.remove(&target);
        }
    }
}

fn tmux_option_is_implemented(option: TmuxOption) -> bool {
    option.default.is_some()
        || StatusOption::from_name(option.name).is_some()
        || WindowStatusOption::from_name(option.name).is_some()
        || ServerOption::from_name(option.name).is_some()
        || SessionOption::from_name(option.name).is_some()
        || WindowOption::from_name(option.name).is_some()
        || PaneOption::from_name(option.name).is_some()
}

fn tmux_options_for_listing(target: TmuxOptionTarget) -> Vec<TmuxOption> {
    let mut options = tmux_options()
        .filter(|option| option_scope_matches_target(option.scope, target))
        .filter(|option| {
            tmux_stored_array(option.name).is_some()
                || !option.is_array && tmux_option_is_implemented(*option)
        })
        .collect::<Vec<_>>();
    options.sort_unstable_by_key(|option| tmux_option_table_order(option.name));
    options
}

fn tmux_option_value_is_string(option: TmuxOption) -> bool {
    option
        .default
        .is_some_and(super::tmux_options::TmuxOptionDefault::is_string)
        || StatusOption::from_name(option.name).is_some_and(StatusOption::is_string)
        || WindowStatusOption::from_name(option.name).is_some()
        || ServerOption::from_name(option.name).is_some_and(ServerOption::is_string)
        || SessionOption::from_name(option.name).is_some_and(SessionOption::is_string)
        || WindowOption::from_name(option.name).is_some_and(WindowOption::is_string)
        || PaneOption::from_name(option.name).is_some_and(PaneOption::is_string)
        || tmux_stored_scalar(option.name).is_some_and(|metadata| metadata.kind.is_string())
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

fn push_shown_array(
    lines: &mut Vec<String>,
    name: &str,
    array: &StringArray,
    requested: Option<&ArrayIndex>,
    is_string: bool,
    inherited: bool,
    value_only: bool,
) {
    if let Some(index) = requested {
        let name = indexed_option_name(name, Some(&index.display()));
        push_shown_option(
            lines,
            &name,
            array.get(index).map_or("", String::as_str),
            is_string,
            inherited,
            value_only,
        );
        return;
    }
    if array.is_empty() {
        if !value_only {
            lines.push(if inherited {
                format!("{name}*")
            } else {
                name.to_owned()
            });
        }
        return;
    }
    for (index, value) in array {
        let name = indexed_option_name(name, Some(&index.display()));
        push_shown_option(lines, &name, value, is_string, inherited, value_only);
    }
}

fn shown_options_output(lines: &[String]) -> String {
    let mut output = lines.join("\n");
    if lines.last().is_some_and(String::is_empty) {
        output.push('\n');
    }
    output
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

fn tmux_signal_name(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    let lower = value.to_ascii_lowercase();
    let short = lower.strip_prefix("sig").unwrap_or(&lower);
    if matches!(
        short,
        "hup"
            | "int"
            | "quit"
            | "ill"
            | "trap"
            | "abrt"
            | "emt"
            | "fpe"
            | "kill"
            | "bus"
            | "segv"
            | "sys"
            | "pipe"
            | "alrm"
            | "term"
            | "urg"
            | "stop"
            | "tstp"
            | "cont"
            | "chld"
            | "ttin"
            | "ttou"
            | "io"
            | "xcpu"
            | "xfsz"
            | "vtalrm"
            | "prof"
            | "winch"
            | "info"
            | "usr1"
            | "usr2"
    ) {
        return short.to_owned();
    }
    for (description, name) in [
        ("hangup", "hup"),
        ("interrupt", "int"),
        ("quit", "quit"),
        ("illegal instruction", "ill"),
        ("trace/bpt trap", "trap"),
        ("trace trap", "trap"),
        ("abort trap", "abrt"),
        ("aborted", "abrt"),
        ("emt trap", "emt"),
        ("floating point exception", "fpe"),
        ("killed", "kill"),
        ("bus error", "bus"),
        ("segmentation fault", "segv"),
        ("bad system call", "sys"),
        ("broken pipe", "pipe"),
        ("alarm clock", "alrm"),
        ("terminated", "term"),
        ("urgent i/o condition", "urg"),
        ("stopped (signal)", "stop"),
        ("stopped (tty input)", "ttin"),
        ("stopped (tty output)", "ttou"),
        ("stopped", "tstp"),
        ("continued", "cont"),
        ("child exited", "chld"),
        ("i/o possible", "io"),
        ("cpu time limit exceeded", "xcpu"),
        ("cputime limit exceeded", "xcpu"),
        ("file size limit exceeded", "xfsz"),
        ("filesize limit exceeded", "xfsz"),
        ("virtual timer expired", "vtalrm"),
        ("profiling timer expired", "prof"),
        ("window size changes", "winch"),
        ("window changed", "winch"),
        ("information request", "info"),
        ("user defined signal 1", "usr1"),
        ("user defined signal 2", "usr2"),
    ] {
        if lower.starts_with(description) {
            return name.to_owned();
        }
    }
    let number = lower
        .split(|character: char| !character.is_ascii_digit())
        .rfind(|part| !part.is_empty())
        .map(str::to_owned);
    number.unwrap_or(lower)
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

fn parse_remain_on_exit(
    value: Option<&str>,
    current: RemainOnExit,
) -> Result<RemainOnExit, ServerError> {
    let Some(value) = value else {
        return Ok(current.toggled());
    };
    if value.is_empty() {
        return Ok(current.toggled());
    }
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

fn initial_window_extent(
    options: &Options,
    default_size: &str,
    client_size: Option<(u16, u16)>,
) -> Result<(u16, u16), ServerError> {
    let default = parse_default_size(default_size).unwrap_or(DEFAULT_WINDOW_EXTENT);
    Ok((
        initial_window_dimension(
            options,
            "-x",
            default.0,
            client_size.map_or(DEFAULT_WINDOW_EXTENT.0, |size| size.0),
            "width",
        )?,
        initial_window_dimension(
            options,
            "-y",
            default.1,
            client_size.map_or(DEFAULT_WINDOW_EXTENT.1, |size| size.1),
            "height",
        )?,
    ))
}

fn parse_default_size(value: &str) -> Option<(u16, u16)> {
    let (width, rest) = parse_unsigned_prefix(value)?;
    let (height, _) = parse_unsigned_prefix(rest.strip_prefix('x')?)?;
    let clamp = |dimension: u64| {
        u16::try_from(dimension.clamp(1, 10_000)).expect("default size is clamped")
    };
    Some((clamp(width), clamp(height)))
}

fn parse_unsigned_prefix(value: &str) -> Option<(u64, &str)> {
    let end = value.bytes().take_while(u8::is_ascii_digit).count();
    (end != 0).then(|| {
        (
            value[..end].parse::<u64>().unwrap_or(u64::MAX),
            &value[end..],
        )
    })
}

fn initial_window_dimension(
    options: &Options,
    option: &str,
    default: u16,
    dash: u16,
    dimension: &str,
) -> Result<u16, ServerError> {
    let Some(value) = options.value(option) else {
        return Ok(default);
    };
    if value == "-" {
        return Ok(dash);
    }
    match value.parse::<i128>() {
        Ok(number) if number < 1 => Err(ServerError::InvalidCommand(format!(
            "{dimension} too small"
        ))),
        Ok(number) if number > i128::from(u16::MAX) => Err(ServerError::InvalidCommand(format!(
            "{dimension} too large"
        ))),
        Ok(number) => Ok(u16::try_from(number).expect("bounded dimension fits u16")),
        Err(_) if decimal_digits(value.strip_prefix('-')) => Err(ServerError::InvalidCommand(
            format!("{dimension} too small"),
        )),
        Err(_) if decimal_digits(Some(value.strip_prefix('+').unwrap_or(value))) => Err(
            ServerError::InvalidCommand(format!("{dimension} too large")),
        ),
        Err(_) => Err(ServerError::InvalidCommand(format!("{dimension} invalid"))),
    }
}

fn require_client_terminal(context: &ExecutionContext) -> Result<(), ServerError> {
    match context.client_terminal {
        ClientTerminal::Absent => Err(ServerError::InvalidCommand(
            "open terminal failed: not a terminal".to_owned(),
        )),
        ClientTerminal::NoClient | ClientTerminal::Present => Ok(()),
    }
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
            "already set: {option}"
        )))
    }
}

fn validate_unimplemented_option_value(
    option: &str,
    value: Option<&str>,
) -> Result<(), ServerError> {
    let Some(value) = value else {
        return Ok(());
    };
    match option {
        "status-keys" if !matches!(value, "emacs" | "vi") => Err(ServerError::InvalidCommand(
            format!("unknown value: {value}"),
        )),
        "status-bg" if parse_tmux_colour(value).is_none() => {
            Err(ServerError::InvalidCommand(format!("bad colour: {value}")))
        }
        "status-style" if !valid_style(value) => Err(ServerError::InvalidCommand(format!(
            "invalid style: {value}"
        ))),
        "default-client-command"
            if !crate::parse_config("<set-option>", value)
                .diagnostics
                .is_empty() =>
        {
            Err(ServerError::InvalidCommand("syntax error".to_owned()))
        }
        _ => Ok(()),
    }
}

fn normalize_stored_scalar_value(
    kind: TmuxStoredScalarKind,
    value: Option<&str>,
    current: &str,
) -> Result<String, ServerError> {
    match kind {
        TmuxStoredScalarKind::String => value
            .map(str::to_owned)
            .ok_or_else(|| ServerError::InvalidCommand("empty value".to_owned())),
        TmuxStoredScalarKind::Style => {
            let value =
                value.ok_or_else(|| ServerError::InvalidCommand("empty value".to_owned()))?;
            if value.contains("#{") || valid_style(value) {
                Ok(value.to_owned())
            } else {
                Err(ServerError::InvalidCommand(format!(
                    "invalid style: {value}"
                )))
            }
        }
        TmuxStoredScalarKind::Colour => {
            let value =
                value.ok_or_else(|| ServerError::InvalidCommand("empty value".to_owned()))?;
            if value.is_empty()
                || value.contains("#{")
                || parse_tmux_colour(value).is_some()
                || zz_terminal::parse_x11_color(value).is_some()
            {
                Ok(value.to_owned())
            } else {
                Err(ServerError::InvalidCommand(format!(
                    "invalid colour: {value}"
                )))
            }
        }
        TmuxStoredScalarKind::Flag => {
            let enabled = match value {
                None | Some("") => current != "on",
                Some("1") => true,
                Some("0") => false,
                Some(value)
                    if value.eq_ignore_ascii_case("on") || value.eq_ignore_ascii_case("yes") =>
                {
                    true
                }
                Some(value)
                    if value.eq_ignore_ascii_case("off") || value.eq_ignore_ascii_case("no") =>
                {
                    false
                }
                Some(value) => {
                    return Err(ServerError::InvalidCommand(format!("bad value: {value}")));
                }
            };
            Ok(tmux_flag(enabled).to_owned())
        }
        TmuxStoredScalarKind::Choice(choices) => {
            if let Some(value) = value {
                return choices
                    .contains(&value)
                    .then(|| value.to_owned())
                    .ok_or_else(|| ServerError::InvalidCommand(format!("unknown value: {value}")));
            }
            Ok(match choices.iter().position(|choice| *choice == current) {
                Some(0) => choices[1].to_owned(),
                Some(1) => choices[0].to_owned(),
                _ => current.to_owned(),
            })
        }
        TmuxStoredScalarKind::Key => {
            let value =
                value.ok_or_else(|| ServerError::InvalidCommand("empty value".to_owned()))?;
            if valid_option_key(value) {
                Ok(canonical_key(value))
            } else {
                Err(ServerError::InvalidCommand(format!("bad key: {value}")))
            }
        }
    }
}

fn valid_option_key(value: &str) -> bool {
    matches!(key_token(value), KeyToken::Named(_))
        || value.chars().count() == 1
        || value.eq_ignore_ascii_case("none")
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

fn shell_command_positional(positional: &[String]) -> Option<Vec<String>> {
    (!positional.is_empty()).then(|| positional.to_vec())
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

// Accepted divergence: opaque blocks expand against the live global environment when
// reparsed, so a mutation after config load can change the payload.
fn bound_commands(
    engine: &MuxEngine,
    tail: &[String],
) -> Result<Vec<CommandInvocation>, ServerError> {
    let commands = if let [argument] = tail
        && let Some(body) = crate::parser::command_block_body(argument)
    {
        let parsed = engine.parse_config("<bind-key>", body);
        if let Some(diagnostic) = parsed.diagnostics.into_iter().next() {
            return Err(ServerError::InvalidCommand(diagnostic.message));
        }
        parsed
            .commands
            .into_iter()
            .map(|command| CommandInvocation::new(command.name, command.args))
            .collect::<Vec<_>>()
    } else {
        let commands = zz_protocol::split_command_words(tail.iter().cloned())
            .into_iter()
            .filter_map(|words| {
                let mut words = words.into_iter();
                let name = words.next()?;
                Some(CommandInvocation::new(name, words))
            })
            .collect::<Vec<_>>();
        if commands.is_empty() {
            return Err(ServerError::InvalidCommand(
                "bind-key command chain contains an empty command".to_owned(),
            ));
        }
        commands
    };
    for command in &commands {
        validate_bound_command(command, "bind-key")?;
    }
    Ok(commands)
}

fn parse_hook_commands(
    engine: &MuxEngine,
    value: &str,
) -> Result<Vec<CommandInvocation>, ServerError> {
    let input = crate::parser::command_block_body(value).unwrap_or(value);
    if has_unquoted_hook_format(input) {
        return Err(ServerError::InvalidCommand("syntax error".to_owned()));
    }
    let parsed = engine.parse_config("<set-hook>", input);
    if let Some(diagnostic) = parsed.diagnostics.into_iter().next() {
        return Err(ServerError::InvalidCommand(diagnostic.message));
    }
    for command in &parsed.commands {
        validate_bound_command(command, "set-hook")?;
    }
    Ok(parsed.commands)
}

fn normalize_option_command(value: &str) -> Result<String, ServerError> {
    let parsed = crate::parse_config("<set-option>", value);
    if !parsed.diagnostics.is_empty() {
        return Err(ServerError::InvalidCommand("syntax error".to_owned()));
    }
    parsed
        .commands
        .into_iter()
        .map(|mut command| {
            command.name = match resolve_command(&command.name) {
                CommandResolution::Canonical(name) | CommandResolution::Unimplemented(name) => {
                    name.to_owned()
                }
                CommandResolution::Ambiguous(message) => {
                    return Err(ServerError::InvalidCommand(message));
                }
                CommandResolution::Unknown => {
                    return Err(ServerError::InvalidCommand(format!(
                        "unknown command: {}",
                        command.name
                    )));
                }
            };
            Ok(format_command(&command))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|commands| commands.join(" ; "))
}

fn has_unquoted_hook_format(input: &str) -> bool {
    let mut characters = input.chars().peekable();
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    let mut word_started = false;
    while let Some(character) = characters.next() {
        if escaped {
            escaped = false;
            word_started = true;
            continue;
        }
        match character {
            '\\' if !single_quoted => {
                escaped = true;
                word_started = true;
            }
            '\'' if !double_quoted => {
                single_quoted = !single_quoted;
                word_started = true;
            }
            '"' if !single_quoted => {
                double_quoted = !double_quoted;
                word_started = true;
            }
            '#' if !single_quoted
                && !double_quoted
                && word_started
                && characters.peek().is_some_and(|next| *next == '{') =>
            {
                return true;
            }
            '#' if !single_quoted && !double_quoted && !word_started => {
                for character in characters.by_ref() {
                    if character == '\n' {
                        break;
                    }
                }
                word_started = false;
            }
            ';' | '\n' if !single_quoted && !double_quoted => word_started = false,
            character if character.is_whitespace() && !single_quoted && !double_quoted => {
                word_started = false;
            }
            _ => word_started = true,
        }
    }
    false
}

fn validate_bound_command(command: &CommandInvocation, owner: &str) -> Result<(), ServerError> {
    let name = canonical_command(&command.name);
    if let Some(spec) = command_spec(name) {
        let (options, _) = parse_options_for_spec(&command.args, spec)?;
        return validate_options(name, spec, &options);
    }
    if name == "copy-mode-repeat" || CommandSpec::DAEMON_COMMAND_NAMES.contains(&name) {
        return Ok(());
    }
    if CommandSpec::UNIMPLEMENTED_TMUX_COMMANDS.contains(&name) {
        return Err(ServerError::UnsupportedCommand(format!("{owner} {name}")));
    }
    match resolve_command(&command.name) {
        CommandResolution::Ambiguous(message) => Err(ServerError::InvalidCommand(message)),
        _ => Err(ServerError::InvalidCommand(format!(
            "unknown command: {name}"
        ))),
    }
}

fn format_command(command: &CommandInvocation) -> String {
    tmux_command_print(command)
}

fn format_key_command(binding: &Binding) -> String {
    binding
        .commands
        .iter()
        .map(tmux_command_print)
        .collect::<Vec<_>>()
        .join(" \\; ")
}

/// Render a stored command the way the pin's `cmd_print` does (`arguments.c`
/// `args_print`): the canonical name, value-less flags merged into one group in
/// flag order, valued flags in flag order with `args_escape`d values, then the
/// positionals.
fn tmux_command_print(command: &CommandInvocation) -> String {
    let name = canonical_command(&command.name);
    let spec = command_spec(name).or_else(|| {
        DAEMON_COMMAND_SPECS
            .iter()
            .find(|spec| spec.name == name || spec.aliases.contains(&name))
    });
    let Some(spec) = spec else {
        return std::iter::once(name)
            .chain(command.args.iter().map(String::as_str))
            .map(tmux_args_escape)
            .collect::<Vec<_>>()
            .join(" ");
    };
    let option_for = |flag: char| {
        spec.options.iter().find(|option| {
            option
                .name
                .strip_prefix('-')
                .is_some_and(|rest| rest.chars().eq(std::iter::once(flag)))
        })
    };

    let mut flags = BTreeMap::<char, usize>::new();
    let mut valued = BTreeMap::<char, Vec<&str>>::new();
    let mut positional = Vec::new();
    let mut args = command.args.iter().map(String::as_str);
    let mut parsing_flags = true;
    while let Some(arg) = args.next() {
        if !parsing_flags {
            positional.push(arg);
            continue;
        }
        if arg == "--" {
            parsing_flags = false;
            continue;
        }
        let Some(cluster) = arg.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
            parsing_flags = false;
            positional.push(arg);
            continue;
        };
        for (offset, flag) in cluster.char_indices() {
            let rest = &cluster[offset + flag.len_utf8()..];
            match option_for(flag) {
                Some(option) if option.attached_value => {
                    if rest.is_empty() {
                        *flags.entry(flag).or_default() += 1;
                    } else {
                        valued.entry(flag).or_default().push(rest);
                    }
                    break;
                }
                Some(option) if option.value.is_some() => {
                    let value = if rest.is_empty() {
                        args.next().unwrap_or("")
                    } else {
                        rest
                    };
                    valued.entry(flag).or_default().push(value);
                    break;
                }
                _ => *flags.entry(flag).or_default() += 1,
            }
        }
    }

    let mut output = name.to_owned();
    if !flags.is_empty() {
        output.push_str(" -");
        for (flag, count) in &flags {
            output.extend(std::iter::repeat_n(*flag, *count));
        }
    }
    for (flag, values) in &valued {
        for value in values {
            output.push_str(" -");
            output.push(*flag);
            output.push(' ');
            output.push_str(&tmux_args_escape(value));
        }
    }
    for arg in positional {
        output.push(' ');
        output.push_str(&tmux_args_escape(arg));
    }
    output
}

fn format_command_arguments(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| tmux_args_escape(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn first_free_array_index<'a>(
    keys: impl IntoIterator<Item = &'a ArrayIndex>,
) -> Result<u32, ServerError> {
    let mut next = 0_u32;
    for key in keys {
        match key {
            ArrayIndex::Numeric(index) if *index == next => {
                next = next
                    .checked_add(1)
                    .ok_or_else(|| ServerError::InvalidCommand("no free array index".to_owned()))?;
            }
            ArrayIndex::Numeric(index) if *index < next => {}
            _ => break,
        }
    }
    Ok(next)
}

fn default_array(name: &'static str) -> StringArray {
    tmux_stored_array(name)
        .expect("stored array metadata")
        .defaults
        .iter()
        .enumerate()
        .map(|(index, value)| {
            (
                ArrayIndex::Numeric(u32::try_from(index).expect("array default index fits u32")),
                (*value).to_owned(),
            )
        })
        .collect()
}

fn default_array_table(scope: TmuxOptionScope) -> ArrayTable {
    tmux_options()
        .filter(|option| option.scope == scope && tmux_stored_array(option.name).is_some())
        .map(|option| (option.name, default_array(option.name)))
        .collect()
}

fn validate_array_value(kind: TmuxArrayValue, value: &str) -> Result<(), ServerError> {
    if kind == TmuxArrayValue::Colour
        && parse_tmux_colour(value).is_none()
        && zz_terminal::parse_x11_color(value).is_none()
    {
        return Err(ServerError::InvalidCommand(format!("bad colour: {value}")));
    }
    Ok(())
}

fn global_hook_table(scope: TmuxOptionScope) -> HookTable {
    tmux_options()
        .filter(|option| tmux_option_is_hook(option.name))
        .filter(|option| match scope {
            TmuxOptionScope::Session => option.scope == TmuxOptionScope::Session,
            TmuxOptionScope::Window => matches!(
                option.scope,
                TmuxOptionScope::Window | TmuxOptionScope::WindowPane
            ),
            TmuxOptionScope::Server | TmuxOptionScope::WindowPane => false,
        })
        .map(|option| (option.name.to_owned(), HookArray::new()))
        .collect()
}

fn push_shown_hook(
    lines: &mut Vec<String>,
    name: &str,
    hook: &HookArray,
    requested: Option<&ArrayIndex>,
) {
    push_shown_hook_option(lines, name, hook, requested, false, false);
}

fn push_shown_hook_option(
    lines: &mut Vec<String>,
    name: &str,
    hook: &HookArray,
    requested: Option<&ArrayIndex>,
    inherited: bool,
    value_only: bool,
) {
    if let Some(index) = requested {
        let value = hook.get(index).map_or_else(String::new, |commands| {
            commands
                .iter()
                .map(format_command)
                .collect::<Vec<_>>()
                .join(" ; ")
        });
        let name = indexed_option_name(name, Some(&index.display()));
        push_shown_option(lines, &name, &value, false, inherited, value_only);
        return;
    }
    if hook.is_empty() {
        if !value_only {
            lines.push(if inherited {
                format!("{name}*")
            } else {
                name.to_owned()
            });
        }
        return;
    }
    for (index, commands) in hook {
        let value = commands
            .iter()
            .map(format_command)
            .collect::<Vec<_>>()
            .join(" ; ");
        let name = indexed_option_name(name, Some(&index.display()));
        push_shown_option(lines, &name, &value, false, inherited, value_only);
    }
}

#[must_use]
pub fn hook_format_variables(command: &CommandInvocation, hook: &str) -> BTreeMap<String, String> {
    let mut variables = BTreeMap::from([
        ("hook".to_owned(), hook.to_owned()),
        (
            "hook_arguments".to_owned(),
            format_command_arguments(&command.args),
        ),
    ]);
    let name = canonical_command(&command.name);
    let spec = command_spec(name).or_else(|| {
        DAEMON_COMMAND_SPECS
            .iter()
            .find(|spec| spec.name == name || spec.aliases.contains(&name))
    });
    let (options, positional) = spec
        .and_then(|spec| parse_options_for_spec(&command.args, spec).ok())
        .unwrap_or_else(|| (Options::default(), command.args.clone()));
    for (index, argument) in positional.iter().enumerate() {
        variables.insert(format!("hook_argument_{index}"), argument.clone());
    }
    for flag in &options.flags {
        let flag = flag.trim_start_matches('-');
        variables.insert(format!("hook_flag_{flag}"), "1".to_owned());
    }
    let mut counts = BTreeMap::<&str, usize>::new();
    for (flag, value) in &options.values {
        let flag = flag.trim_start_matches('-');
        variables.insert(format!("hook_flag_{flag}"), value.clone());
        let index = counts.entry(flag).or_default();
        variables.insert(format!("hook_flag_{flag}_{index}"), value.clone());
        *index += 1;
    }
    variables
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str, args: &[&str]) -> CommandInvocation {
        CommandInvocation::new(name, args.iter().copied())
    }

    fn assert_bare_matches_format(
        engine: &mut MuxEngine,
        context: &mut ExecutionContext,
        name: &str,
        args: &[&str],
        format: &str,
    ) -> String {
        let bare = engine
            .execute(context, &command(name, args))
            .unwrap()
            .output;
        let mut explicit = command(name, args);
        explicit.args.extend(["-F".to_owned(), format.to_owned()]);
        let formatted = engine.execute(context, &explicit).unwrap().output;
        assert_eq!(bare, formatted);
        bare
    }

    struct AttachedSessionHooks;

    impl StatusHooks for AttachedSessionHooks {
        fn strftime(&mut self, literal: &str) -> String {
            literal.to_owned()
        }

        fn shell(&mut self, _command: &str) -> String {
            String::new()
        }

        fn variable(&mut self, name: &str, _context: &StatusContext) -> Option<String> {
            (name == "session_attached").then(|| "1".to_owned())
        }
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
    fn unknown_and_unimplemented_commands_keep_distinct_error_classes() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        assert_eq!(
            engine
                .execute(&mut context, &command("wibble", &[]))
                .unwrap_err(),
            ServerError::InvalidCommand("unknown command: wibble".to_owned())
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("new-pane", &[]))
                .unwrap_err(),
            ServerError::UnsupportedCommand("new-pane".to_owned())
        );
    }

    #[test]
    fn option_value_error_text_matches_the_pinned_matrix() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "error-shapes"]),
            )
            .unwrap();
        let mut hooks = CommandHooks::new(0);
        let mut valid_shell = |shell: &str| shell != "/not/a/shell";
        for (arguments, expected) in [
            (
                &["-g", "display-time", "-5"] as &[&str],
                "value is too small: -5",
            ),
            (&["-g", "display-time", "abc"], "value is invalid: abc"),
            (&["-g", "display-time"], "empty value"),
            (&["-g", "@novalue"], "empty value"),
            (&["-g", "status-keys", "bogus"], "unknown value: bogus"),
            (&["-g", "focus-events", "maybe"], "bad value: maybe"),
            (&["-g", "status-bg", "xxxyyy"], "bad colour: xxxyyy"),
            (
                &["-g", "status-style", "bg=xxxyyy"],
                "invalid style: bg=xxxyyy",
            ),
            (&["-g", "prefix", "boguskey"], "bad key: boguskey"),
            (
                &["-g", "default-shell", "/not/a/shell"],
                "not a suitable shell: /not/a/shell",
            ),
            (&["-g", "default-client-command", "if -x {"], "syntax error"),
        ] {
            assert_eq!(
                engine
                    .execute_with_shell_validator(
                        &mut context,
                        &command("set-option", arguments),
                        &mut hooks,
                        &mut valid_shell,
                    )
                    .unwrap_err(),
                ServerError::InvalidCommand(expected.to_owned()),
                "{arguments:?}"
            );
        }
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@once", "first"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-go", "@once", "second"]),
                )
                .unwrap_err(),
            ServerError::InvalidCommand("already set: @once".to_owned())
        );
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
                    read_only: false,
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
    fn clientless_new_session_forces_detached_creation() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext {
            client_terminal: ClientTerminal::NoClient,
            ..ExecutionContext::default()
        };

        let execution = engine
            .execute(&mut context, &command("new-session", &["-s", "fromconfig"]))
            .expect("clientless new session");

        assert!(
            execution
                .effects
                .iter()
                .all(|effect| !matches!(effect, MuxEffect::Attach { .. }))
        );
        assert!(session_named(&engine.state, "fromconfig").is_some());
    }

    #[test]
    fn clientless_attach_commands_are_silent_noops() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "base"]))
            .expect("base session");
        let session = context.session;
        context.client_terminal = ClientTerminal::NoClient;

        let new_session = engine
            .execute(&mut context, &command("new-session", &["-A", "-s", "base"]))
            .expect("clientless new-session -A");
        assert!(new_session.output.is_empty());
        assert!(new_session.effects.is_empty());
        assert_eq!(context.session, session);

        let attach = engine
            .execute(&mut context, &command("attach-session", &["-t", "bogus"]))
            .expect("clientless attach-session");
        assert!(attach.output.is_empty());
        assert!(attach.effects.is_empty());
        assert_eq!(context.session, session);
    }

    #[test]
    fn new_session_check_order_matches_terminal_duplicate_and_size_rules() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext {
            client_terminal: ClientTerminal::Absent,
            ..ExecutionContext::default()
        };

        assert!(matches!(
            engine.execute(
                &mut context,
                &command("new-session", &["-s", "missing-terminal"]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "open terminal failed: not a terminal"
        ));
        assert!(engine.state.sessions.is_empty());

        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "duplicate"]),
            )
            .expect("detached session");
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("new-session", &["-s", "duplicate", "-x", "0"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "duplicate session: duplicate"
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("new-session", &["-s", "fresh-size", "-x", "0"]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "open terminal failed: not a terminal"
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("new-session", &["-A", "-d", "-s", "duplicate"]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "open terminal failed: not a terminal"
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("attach-session", &["-t", "duplicate"]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "open terminal failed: not a terminal"
        ));

        engine
            .execute(
                &mut context,
                &command("new-session", &["-A", "-d", "-s", "detached"]),
            )
            .expect("fresh -A -d remains detached");
        context.client_terminal = ClientTerminal::Present;
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("new-session", &["-s", "bad-width", "-x", "0"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "width too small"
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("new-session", &["-s", "bad-height", "-y", "0"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "height too small"
        ));
        assert_eq!(engine.state.sessions.len(), 2);
    }

    #[test]
    fn new_session_prints_the_created_session_with_the_requested_format() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        let default = engine
            .execute(
                &mut context,
                &command("new-session", &["-P", "-d", "-s", "printed"]),
            )
            .expect("printed session");
        assert_eq!(default.output, "printed:");

        let formatted = engine
            .execute(
                &mut context,
                &command(
                    "new-session",
                    &[
                        "-P",
                        "-d",
                        "-F",
                        "#{session_name}/#{window_index}",
                        "-s",
                        "formatted",
                    ],
                ),
            )
            .expect("formatted session");
        assert_eq!(formatted.output, "formatted/0");

        let ignored = engine
            .execute(
                &mut context,
                &command(
                    "new-session",
                    &["-d", "-F", "#{session_name}", "-s", "silent"],
                ),
            )
            .expect("silent session");
        assert!(ignored.output.is_empty());
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

        let mut command_context = ExecutionContext::new(Some(session), Some(window), Some(pane));
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
        let mut command_context = ExecutionContext::new(Some(session), Some(window), Some(pane));

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
        let mut command_context = ExecutionContext::new(Some(session), Some(window), Some(pane));
        engine
            .execute(
                &mut command_context,
                &command("select-window", &["-t", "A:0"]),
            )
            .expect("target first session");

        let (session, window, pane) = engine.state.most_recent_context().expect("recent context");
        let mut followup_context = ExecutionContext::new(Some(session), Some(window), Some(pane));
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
    fn detached_new_session_uses_default_size_with_per_dimension_overrides() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "default-size", "132x43"]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "wide"]))
            .unwrap();
        assert_eq!(
            engine.state.windows[&context.window.unwrap()]
                .layout
                .extent(),
            (132, 43)
        );
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "tall", "-x", "90"]),
            )
            .unwrap();
        assert_eq!(
            engine.state.windows[&context.window.unwrap()]
                .layout
                .extent(),
            (90, 43)
        );
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "dash-width", "-x", "-"]),
            )
            .unwrap();
        assert_eq!(
            engine.state.windows[&context.window.unwrap()]
                .layout
                .extent(),
            (80, 43)
        );
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "dash-height", "-y", "-"]),
            )
            .unwrap();
        assert_eq!(
            engine.state.windows[&context.window.unwrap()]
                .layout
                .extent(),
            (132, 24)
        );
        engine
            .execute(
                &mut context,
                &command(
                    "new-session",
                    &["-d", "-s", "dash-both", "-x", "-", "-y", "-"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine.state.windows[&context.window.unwrap()]
                .layout
                .extent(),
            (80, 24)
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "default-size", "0x20000tail"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "clamped"]),
            )
            .unwrap();
        assert_eq!(
            engine.state.windows[&context.window.unwrap()]
                .layout
                .extent(),
            (1, 10_000)
        );
    }

    #[test]
    fn dash_creation_dimensions_use_the_caller_terminal_size() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        context.set_client_size(Some((132, 43)));
        engine
            .execute(
                &mut context,
                &command("new-session", &["-d", "-s", "sized", "-x", "-", "-y", "-"]),
            )
            .unwrap();
        assert_eq!(
            engine.state.windows[&context.window.unwrap()]
                .layout
                .extent(),
            (132, 43)
        );
        context.set_client_size(None);
        engine
            .execute(
                &mut context,
                &command(
                    "new-session",
                    &["-d", "-s", "sizeless", "-x", "-", "-y", "-"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine.state.windows[&context.window.unwrap()]
                .layout
                .extent(),
            (80, 24)
        );
    }

    #[test]
    fn set_titles_readers_resolve_scope_and_emit_status_effects() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();

        assert!(!engine.set_titles_for_session(Some(session)));
        assert_eq!(
            engine.set_titles_string_for_session(Some(session)),
            "#S:#I:#W - \"#T\" #{session_alerts}"
        );

        let global = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "set-titles", "on"]),
            )
            .unwrap();
        assert_eq!(
            global.effects,
            [MuxEffect::StatusFormatsChanged { session: None }]
        );
        assert!(engine.set_titles_for_session(Some(session)));
        assert!(engine.set_titles_for_session(None));

        let scoped = engine
            .execute(
                &mut context,
                &command("set-option", &["set-titles-string", "#S custom"]),
            )
            .unwrap();
        assert_eq!(
            scoped.effects,
            [MuxEffect::StatusFormatsChanged {
                session: Some(session),
            }]
        );
        assert_eq!(
            engine.set_titles_string_for_session(Some(session)),
            "#S custom"
        );
        assert_eq!(
            engine.set_titles_string_for_session(None),
            "#S:#I:#W - \"#T\" #{session_alerts}"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-u", "set-titles-string"]),
            )
            .unwrap();
        assert_eq!(
            engine.set_titles_string_for_session(Some(session)),
            "#S:#I:#W - \"#T\" #{session_alerts}"
        );
    }

    #[test]
    fn projected_client_extent_preserves_split_and_zoom_geometry() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-x", "80", "-y", "24"]),
            )
            .unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        let first_size = engine
            .pane_geometry_at_window_extent(first, 100, 50)
            .unwrap();
        let second_size = engine
            .pane_geometry_at_window_extent(second, 100, 50)
            .unwrap();
        assert_eq!(first_size.0 + second_size.0 + 1, 100);
        assert_eq!((first_size.1, second_size.1), (50, 50));

        engine
            .execute(&mut context, &command("resize-pane", &["-Z"]))
            .unwrap();
        assert_eq!(
            engine.pane_geometry_at_window_extent(second, 100, 50),
            Some((100, 50))
        );
        assert_eq!(engine.pane_geometry_at_window_extent(first, 100, 50), None);
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
                read_only: false,
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
                read_only: false,
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
                read_only: false,
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
    fn pane_creation_commands_print_the_created_pane_format() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        let new_window = engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-P", "-n", "logs"]),
            )
            .unwrap();
        assert_eq!(new_window.output, "work:1.0\n");

        let split = engine
            .execute(
                &mut context,
                &command(
                    "split-window",
                    &["-P", "-F", "#{session_name}|#{window_index}|#{pane_index}"],
                ),
            )
            .unwrap();
        assert_eq!(split.output, "work|0|1\n");
        let moving = context.pane.unwrap();

        let broken = engine
            .execute(
                &mut context,
                &command(
                    "break-pane",
                    &[
                        "-d",
                        "-P",
                        "-F",
                        "#{session_name}|#{window_index}|#{pane_index}",
                        "-s",
                        &moving.to_string(),
                    ],
                ),
            )
            .unwrap();
        assert_eq!(broken.output, "work|2|0\n");

        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let trailing_newline = engine
            .execute(
                &mut context,
                &command("break-pane", &["-d", "-P", "-F", "X\n"]),
            )
            .unwrap();
        assert_eq!(trailing_newline.output.as_bytes(), b"X\n\n");

        let format_without_print = engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-F", "#{window_name}"]),
            )
            .unwrap();
        assert!(format_without_print.output.is_empty());
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
            .execute(
                &mut context,
                &command(
                    "new-window",
                    &["-P", "-F", "should-not-print", "-S", "-n", "logs"],
                ),
            )
            .expect("select the existing window");
        assert_eq!(
            selected.effects,
            [MuxEffect::SuppressAfterHook, MuxEffect::SnapshotChanged]
        );
        assert!(selected.output.is_empty());
        assert_eq!(context.window, Some(logs));
        assert_eq!(engine.state.sessions[&session].windows.len(), 3);

        let expanded = engine
            .execute(
                &mut context,
                &command("new-window", &["-S", "-n", "#{window_name}"]),
            )
            .expect("expand the reuse name");
        assert_eq!(
            expanded.effects,
            [MuxEffect::SuppressAfterHook, MuxEffect::SnapshotChanged]
        );
        assert_eq!(context.window, Some(logs));

        engine
            .execute(&mut context, &command("new-window", &["-S", "-n", "fresh"]))
            .expect("create the missing window");
        assert_eq!(engine.state.sessions[&session].windows.len(), 4);

        let indexed = engine
            .execute(
                &mut context,
                &command(
                    "new-window",
                    &[
                        "-d",
                        "-S",
                        "-t",
                        "work:5",
                        "-n",
                        "logs",
                        "-P",
                        "-F",
                        "#{window_index}:#{window_name}",
                    ],
                ),
            )
            .expect("an explicit index bypasses reuse");
        assert_eq!(indexed.output, "5:logs\n");
        assert_eq!(engine.state.sessions[&session].windows.len(), 5);
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("new-window", &["-S", "-n", "logs"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "multiple windows named logs"
        ));
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
                read_only: false,
            }]
        );
    }

    #[test]
    fn session_listing_is_name_sorted_but_the_s_loop_stays_creation_sorted() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine.set_format_now(1_700_000_000);
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

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-sessions",
                        &["-O", "creation", "-F", "#{session_name}"],
                    ),
                )
                .unwrap()
                .output,
            "w\nA\nB"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-sessions",
                        &["-O", "activity", "-F", "#{session_name}"],
                    ),
                )
                .unwrap()
                .output,
            "B\nA\nw"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-sessions",
                        &["-r", "-O", "creation", "-F", "#{session_name}"],
                    ),
                )
                .unwrap()
                .output,
            "B\nA\nw"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-sessions", &["-r", "-F", "#{session_name}"]),
                )
                .unwrap()
                .output,
            "A\nB\nw"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-sessions",
                        &[
                            "-f",
                            "#{==:#{session_name},w}",
                            "-F",
                            "#{line}:#{session_name}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            "2:w"
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("list-sessions", &["-O", "not-an-order"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "invalid sort order"
        ));
    }

    #[test]
    fn window_and_pane_lists_sort_then_filter_with_pin_line_values() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        engine
            .execute(&mut context, &command("rename-window", &["base"]))
            .unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-d", "-n", "z"]))
            .unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-d", "-n", "a"]))
            .unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-windows",
                        &["-O", "name", "-F", "#{line}:#{window_name}"],
                    ),
                )
                .unwrap()
                .output,
            "3:a\n3:base\n3:z"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-windows",
                        &[
                            "-O",
                            "name",
                            "-f",
                            "#{==:#{window_name},z}",
                            "-F",
                            "#{line}:#{window_name}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            "3:z"
        );

        let first = context.pane.unwrap();
        engine.state.update_pane_title(first, "z").unwrap();
        let split = engine
            .execute(&mut context, &command("split-window", &["-d", "-h"]))
            .unwrap();
        let second = split
            .effects
            .iter()
            .find_map(|effect| match effect {
                MuxEffect::PaneCreated { pane, .. } => Some(*pane),
                _ => None,
            })
            .unwrap();
        engine.state.update_pane_title(second, "a").unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &["-O", "title", "-F", "#{line}:#{pane_title}"],
                    ),
                )
                .unwrap()
                .output,
            "2:a\n2:z"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-panes",
                        &[
                            "-O",
                            "name",
                            "-f",
                            "#{==:#{pane_title},z}",
                            "-F",
                            "#{line}:#{pane_title}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            "2:z"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-panes", &["-O", "z", "-F", "#{pane_title}"]),
                )
                .unwrap()
                .output,
            "a\nz"
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command(
                    "list-windows",
                    &["-t", "missing", "-O", "not-an-order"],
                ),
            ),
            Err(ServerError::SessionNotFound(target)) if target == "missing"
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command(
                    "list-panes",
                    &["-t", "missing", "-O", "not-an-order"],
                ),
            ),
            Err(ServerError::WindowNotFound(target)) if target == "missing"
        ));
    }

    #[test]
    fn list_formats_are_contextual_and_bare_output_uses_the_pin_templates() {
        let mut engine = MuxEngine::default();
        engine.set_format_now(1_700_000_000);
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

        let session_format = concat!(
            "#{session_name}: #{session_windows} windows (created #{t:session_created})",
            "#{?session_grouped, (group ,}#{session_group}#{?session_grouped,),}",
            "#{?session_attached, (attached),}",
        );
        let window_format = concat!(
            "#{window_index}: #{window_name}#{window_raw_flags} (#{window_panes} panes) ",
            "[#{window_width}x#{window_height}] [layout #{window_layout}] #{window_id}",
            "#{?window_active, (active),}",
        );
        let window_with_session_format = concat!(
            "#{session_name}:#{window_index}: #{window_name}#{window_raw_flags} ",
            "(#{window_panes} panes) [#{window_width}x#{window_height}] ",
        );
        let pane_format = concat!(
            "#{pane_index}: [#{pane_width}x#{pane_height}",
            "#{?pane_floating_flag, #{pane_x}#,#{pane_y}#,#{pane_z}}] ",
            "[history #{history_size}/#{history_limit}, #{history_bytes} bytes] #{pane_id}",
            "#{?pane_active, (active),}#{?pane_dead, (dead),}",
        );
        let pane_with_session_format = concat!(
            "#{window_index}.#{pane_index}: [#{pane_width}x#{pane_height}",
            "#{?pane_floating_flag, #{pane_x}#,#{pane_y}#,#{pane_z}}] ",
            "[history #{history_size}/#{history_limit}, #{history_bytes} bytes] #{pane_id}",
            "#{?pane_active, (active),}#{?pane_dead, (dead),}",
        );
        let pane_with_server_format = concat!(
            "#{session_name}:#{window_index}.#{pane_index}: [#{pane_width}x#{pane_height}",
            "#{?pane_floating_flag, #{pane_x}#,#{pane_y}#,#{pane_z}}] ",
            "[history #{history_size}/#{history_limit}, #{history_bytes} bytes] #{pane_id}",
            "#{?pane_active, (active),}#{?pane_dead, (dead),}",
        );

        assert_bare_matches_format(
            &mut engine,
            &mut context,
            "list-sessions",
            &[],
            session_format,
        );
        assert_bare_matches_format(
            &mut engine,
            &mut context,
            "list-windows",
            &[],
            window_format,
        );
        assert_bare_matches_format(
            &mut engine,
            &mut context,
            "list-windows",
            &["-a"],
            window_with_session_format,
        );
        assert_bare_matches_format(&mut engine, &mut context, "list-panes", &[], pane_format);
        assert_bare_matches_format(
            &mut engine,
            &mut context,
            "list-panes",
            &["-s"],
            pane_with_session_format,
        );
        assert_bare_matches_format(
            &mut engine,
            &mut context,
            "list-panes",
            &["-a"],
            pane_with_server_format,
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

        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-windows", &["-F", "#{window_raw_flags}"]),
                )
                .unwrap()
                .output,
            "-\n*"
        );

        let mut hooks = AttachedSessionHooks;
        let attached = engine
            .execute_with_format_hooks(&mut context, &command("list-sessions", &[]), &mut hooks)
            .unwrap()
            .output;
        assert!(attached.ends_with(" (attached)"));
        let mut hooks = AttachedSessionHooks;
        let explicitly_attached = engine
            .execute_with_format_hooks(
                &mut context,
                &command("list-sessions", &["-F", session_format]),
                &mut hooks,
            )
            .unwrap()
            .output;
        assert_eq!(attached, explicitly_attached);

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
    fn display_message_format_option_selects_the_print_template() {
        let mut engine = MuxEngine::default();
        engine.set_format_server_context("tower.local", "tower", "/tmp/zz.sock", 40);
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("display-message", &["-p", "-F", "#{start_time}|#{version}"]),
                )
                .unwrap()
                .output,
            "40|3.8-zz"
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("display-message", &["-p", "-F", "#{start_time}", "message"]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "only one of -F or argument must be given"
        ));
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
            dead_signal: String::new(),
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
    fn new_session_attach_routing_uses_the_command_option_parser() {
        let attaches = |args: &[&str]| {
            MuxEngine::new_session_attaches(
                &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
            )
            .expect("valid new-session arguments")
        };

        assert!(attaches(&["-s", "a", "/usr/bin/true", "-d"]));
        assert!(attaches(&["-s", "b", "--", "-d"]));
        assert!(attaches(&["-dA", "-s", "existing"]));
        assert!(!attaches(&["-dsfoo"]));
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
            }) if command.as_slice() == ["echo", "-n", "hello"]
        ));
        let window = &engine.state.windows[&context.window.unwrap()];
        assert_eq!(window.name, window.index.to_string());

        let single = engine
            .execute(
                &mut context,
                &command("new-window", &["printf '%s' \"$HOME\""]),
            )
            .unwrap();
        assert!(matches!(
            single.effects.first(),
            Some(MuxEffect::PaneCreated {
                command: Some(command),
                ..
            }) if command.as_slice() == ["printf '%s' \"$HOME\""]
        ));

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
    fn pane_start_command_formats_render_retained_argv() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        let created = engine
            .execute(
                &mut context,
                &command(
                    "new-session",
                    &["-s", "work", "printf", "a b", "it's", "$HOME", ""],
                ),
            )
            .unwrap();
        assert!(matches!(
            created.effects.first(),
            Some(MuxEffect::PaneCreated {
                command: Some(command),
                ..
            }) if command.as_slice() == ["printf", "a b", "it's", "$HOME", ""]
        ));
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("display-message", &["-p", "#{pane_start_command}"]),
                )
                .unwrap()
                .output,
            r#"printf "a b" "it's" "\$HOME" ''"#
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("display-message", &["-p", "#{pane_start_command_list}"],),
                )
                .unwrap()
                .output,
            r"'printf' 'a b' 'it'\''s' '$HOME' ''"
        );
    }

    #[test]
    fn bind_key_blocks_expand_the_live_global_environment() {
        let mut engine = MuxEngine::default();
        engine.seed_global_environment([("FOO", "hello")]);
        engine
            .execute(
                &mut ExecutionContext::default(),
                &command("bind-key", &["Q", "{ send-keys $FOO }"]),
            )
            .expect("environment-backed binding");

        assert_eq!(
            engine.keys.get("prefix", "Q").expect("binding").commands,
            [CommandInvocation::new("send-keys", ["hello"])]
        );
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
        engine
            .execute(
                &mut context,
                &command("bind-key", &["-r", "z", "new-window"]),
            )
            .expect("repeat binding");
        assert!(
            engine
                .execute(&mut context, &command("list-keys", &["-T", "prefix"]))
                .unwrap()
                .output
                .lines()
                .any(|line| line == "bind-key -r -T prefix z new-window")
        );

        for args in [
            &["x", ";", "new-window"][..],
            &["x", "new-window", ";", ";", "new-window"][..],
        ] {
            engine
                .execute(&mut context, &command("bind-key", args))
                .expect("empty chain segments are dropped like the pin");
        }
        assert!(
            engine
                .execute(&mut context, &command("list-keys", &["-T", "prefix"]))
                .unwrap()
                .output
                .lines()
                .any(|line| line == "bind-key -T prefix x new-window \\; new-window")
        );
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
        engine
            .execute(
                &mut context,
                &command("bind-key", &["p", "pipe-pane", "cat"]),
            )
            .expect("pipe-pane binding");
        assert_eq!(
            engine.keys.get("prefix", "p").expect("binding").commands,
            [CommandInvocation::new("pipe-pane", ["cat"])]
        );
    }

    #[test]
    fn list_keys_format_expands_the_pinned_per_binding_facts() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command(
                    "bind-key",
                    &[
                        "-r",
                        "-T",
                        "phase7d",
                        "-N",
                        "sample note",
                        "\"",
                        "split-window",
                        "-h",
                    ],
                ),
            )
            .unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "list-keys",
                        &[
                            "-T",
                            "phase7d",
                            "-F",
                            "#{key_repeat}|#{key_note}|#{key_prefix}|#{key_table}|#{key_string}|#{key_command}|#{key_has_repeat}|#{key_string_width}|#{key_table_width}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            "1|sample note|C-b|phase7d|\"|split-window -h|||"
        );
    }

    #[test]
    fn list_keys_key_command_uses_the_pinned_escaped_command_shape() {
        let mut engine = MuxEngine::default();
        engine.keys.bind(
            "phase7d",
            "X",
            Binding {
                commands: vec![
                    CommandInvocation::new(
                        "new-pane",
                        ["-E", "-X", "0", "-Y", "0", "-x", "75%", "-y", "30%"],
                    ),
                    CommandInvocation::new("move-pane", ["-P", "bottom-centre"]),
                ],
                repeat: false,
                note: None,
            },
        );

        assert_eq!(
            engine
                .execute(
                    &mut ExecutionContext::default(),
                    &command(
                        "list-keys",
                        &[
                            "-T",
                            "phase7d",
                            "-F",
                            "#{key_repeat}|#{key_note}|#{key_prefix}|#{key_table}|#{key_string}|#{key_command}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            "0||C-b|phase7d|X|new-pane -E -X 0 -Y 0 -x \"75%\" -y \"30%\" \\; move-pane -P bottom-centre"
        );
    }

    #[test]
    fn list_keys_key_command_orders_flags_like_the_pinned_args_print() {
        let mut engine = MuxEngine::default();
        let vectors: [(&str, &str, &[&str], &str); 7] = [
            (
                "a",
                "split-window",
                &["-c", "#{pane_current_path}", "-v"],
                "split-window -v -c \"#{pane_current_path}\"",
            ),
            (
                "b",
                "new-window",
                &["-d", "-n", "foo", "-c", "/tmp", "-P"],
                "new-window -Pd -c /tmp -n foo",
            ),
            (
                "c",
                "send-keys",
                &["-l", "-t", "x", "hello", "world"],
                "send-keys -l -t x hello world",
            ),
            (
                "d",
                "run-shell",
                &["-b", "-d", "1", "echo hi"],
                "run-shell -b -d 1 \"echo hi\"",
            ),
            ("f", "copy-mode", &["-e", "-u"], "copy-mode -eu"),
            (
                "g",
                "send-keys",
                &["-X", "-N", "5", "cursor-down"],
                "send-keys -X -N 5 cursor-down",
            ),
            (
                "h",
                "splitw",
                &["-h", "-l", "20%", "-c", "/tmp"],
                "split-window -h -c /tmp -l \"20%\"",
            ),
        ];
        for (key, name, args, _) in vectors {
            engine.keys.bind(
                "probe",
                key,
                Binding {
                    commands: vec![CommandInvocation::new(name, args.iter().copied())],
                    repeat: false,
                    note: None,
                },
            );
        }

        let output = engine
            .execute(
                &mut ExecutionContext::default(),
                &command(
                    "list-keys",
                    &["-T", "probe", "-F", "#{key_string}|#{key_command}"],
                ),
            )
            .unwrap()
            .output;
        let expected = vectors
            .iter()
            .map(|(key, _, _, rendered)| format!("{key}|{rendered}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(output, expected);
    }

    #[test]
    fn show_hooks_renders_commands_like_the_pinned_cmd_print() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        for args in [
            &["-g", "after-new-window", "run-shell \"echo hi\""] as &[&str],
            &[
                "-g",
                "after-kill-pane",
                "splitw -v -c \"#{pane_current_path}\"",
            ],
            &["-g", "after-select-pane", "display-message -p 'it''s'"],
        ] {
            engine
                .execute(&mut context, &command("set-hook", args))
                .unwrap();
        }
        for (hook, expected) in [
            (
                "after-new-window",
                "after-new-window[0] run-shell \"echo hi\"",
            ),
            (
                "after-kill-pane",
                "after-kill-pane[0] split-window -v -c \"#{pane_current_path}\"",
            ),
            (
                "after-select-pane",
                "after-select-pane[0] display-message -p its",
            ),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-hooks", &["-g", hook]))
                    .unwrap()
                    .output,
                expected
            );
        }
    }

    #[test]
    fn hooks_store_show_and_override_in_numeric_first_order() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session");
        let window = context.window.expect("window");
        let pane = context.pane.expect("pane");

        let global = engine
            .execute(&mut context, &command("show-hooks", &["-g"]))
            .unwrap()
            .output;
        assert_eq!(
            global.lines().collect::<Vec<_>>(),
            MuxEngine::hook_names_for_target(TmuxOptionTarget::GlobalSession)
        );
        assert_eq!(global.lines().count(), 57);
        assert_eq!(
            global
                .lines()
                .filter(|name| name.starts_with("client-"))
                .collect::<Vec<_>>(),
            [
                "client-active",
                "client-attached",
                "client-detached",
                "client-focus-in",
                "client-focus-out",
                "client-resized",
                "client-session-changed",
                "client-light-theme",
                "client-dark-theme",
            ]
        );
        let global_window = engine
            .execute(&mut context, &command("show-hooks", &["-g", "-w"]))
            .unwrap()
            .output;
        assert_eq!(
            global_window.lines().collect::<Vec<_>>(),
            MuxEngine::hook_names_for_target(TmuxOptionTarget::GlobalWindow)
        );
        assert_eq!(global_window.lines().count(), 11);
        assert_eq!(
            engine
                .execute(&mut context, &command("show-hooks", &[]))
                .unwrap()
                .output,
            ""
        );

        for args in [
            &["after-select-window", "display-message zero"] as &[&str],
            &["-a", "after-select-window", "display-message one"],
            &["after-select-window[named]", "display-message named"],
        ] {
            engine
                .execute(&mut context, &command("set-hook", args))
                .unwrap();
        }
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-hooks", &["after-select-window"]),
                )
                .unwrap()
                .output,
            "after-select-window[0] display-message zero\n\
             after-select-window[1] display-message one\n\
             after-select-window[named] display-message named"
        );

        engine
            .execute(
                &mut context,
                &command("set-hook", &["-u", "after-select-window[0]"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &["-a", "after-select-window", "display-message reused"],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &["after-select-window[1]", "display-message replaced"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-hooks", &["after-select-window"]),
                )
                .unwrap()
                .output,
            "after-select-window[0] display-message reused\n\
             after-select-window[1] display-message replaced\n\
             after-select-window[named] display-message named"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &["-g", "after-select-window", "display-message global"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .hook_commands(Some(session), "after-select-window")
                .expect("session hook")
                .len(),
            3
        );
        engine
            .execute(
                &mut context,
                &command("set-hook", &["-u", "after-select-window"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .hook_commands(Some(session), "after-select-window")
                .expect("global fallback"),
            [parse_hook_commands(&engine, "display-message global").unwrap()]
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &["-w", "after-select-window", "display-message window"],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &["-p", "after-select-window", "display-message pane"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-hooks", &["-w", "after-select-window"]),
                )
                .unwrap()
                .output,
            "after-select-window[0] display-message pane"
        );

        engine
            .execute(
                &mut context,
                &command("set-hook", &["-w", "pane-died", "display-message window"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-hook", &["-p", "pane-died", "display-message pane"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &["-w", "window-renamed", "display-message renamed"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-hooks", &["-w"]))
                .unwrap()
                .output,
            "pane-died[0] display-message window\n\
             window-renamed[0] display-message renamed"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("show-hooks", &["-p"]))
                .unwrap()
                .output,
            "pane-died[0] display-message pane"
        );
        assert_eq!(
            engine
                .hook_array(TmuxOptionTarget::Window(window), "pane-died")
                .expect("window hook")
                .len(),
            1
        );
        assert_eq!(
            engine
                .hook_array(TmuxOptionTarget::Pane(pane), "pane-died")
                .expect("pane hook")
                .len(),
            1
        );

        engine
            .execute(
                &mut context,
                &command("set-hook", &["-g", "alert-b", "display-message alert"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-hooks", &["-g", "alert-bell"]),)
                .unwrap()
                .output,
            "alert-bell[0] display-message alert"
        );
        engine
            .execute(
                &mut context,
                &command("set-hook", &["-gu", "alert-bell[0]"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-hooks", &["-g", "alert-bell"]),)
                .unwrap()
                .output,
            "alert-bell"
        );
        engine
            .execute(
                &mut context,
                &command("set-hook", &["-g", "alert-bell", "display-message alert"]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("set-hook", &["-gu", "alert-bell"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-hooks", &["-g", "alert-bell"]),)
                .unwrap()
                .output,
            "alert-bell"
        );
    }

    #[test]
    fn global_hook_listings_filter_and_route_by_declared_scope() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &["-g", "pane-died", "display-message global-window"],
                ),
            )
            .unwrap();

        let session_hooks = engine
            .execute(&mut context, &command("show-hooks", &["-g"]))
            .unwrap()
            .output;
        assert_eq!(session_hooks.lines().count(), 57);
        assert!(
            !session_hooks
                .lines()
                .any(|line| line.starts_with("pane-died"))
        );

        let window_hooks = engine
            .execute(&mut context, &command("show-hooks", &["-g", "-w"]))
            .unwrap()
            .output;
        assert_eq!(window_hooks.lines().count(), 11);
        assert!(
            window_hooks
                .lines()
                .any(|line| line == "pane-died[0] display-message global-window")
        );
        assert!(!engine.global_hooks.contains_key("pane-died"));
        assert_eq!(
            engine
                .global_window_hooks
                .get("pane-died")
                .expect("global window hook")
                .len(),
            1
        );
    }

    #[test]
    fn user_hooks_share_user_option_storage_and_stay_unlisted() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@myhook", "v"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "-v", "@myhook"]),
                )
                .unwrap()
                .output,
            "v"
        );

        let hook_command = "display-message -p AT-HOOK-RAN";
        engine
            .execute(
                &mut context,
                &command("set-hook", &["-g", "@myhook", hook_command]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "-v", "@myhook"]),
                )
                .unwrap()
                .output,
            hook_command
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("show-hooks", &["-g", "@myhook"]),)
                .unwrap()
                .output,
            engine
                .execute(&mut context, &command("show-options", &["-g", "@myhook"]),)
                .unwrap()
                .output
        );
        assert!(
            !engine
                .execute(&mut context, &command("show-hooks", &["-g"]))
                .unwrap()
                .output
                .lines()
                .any(|line| line.starts_with('@'))
        );

        let fired = engine
            .execute(&mut context, &command("set-hook", &["-R", "@myhook"]))
            .unwrap();
        assert!(matches!(
            fired.effects.as_slice(),
            [MuxEffect::RunHook {
                name,
                commands,
                context: target,
            }] if name == "@myhook"
                && target == &context
                && commands == &vec![parse_hook_commands(&engine, hook_command).unwrap()]
        ));

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@myhook", "v"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("set-hook", &["-R", "@myhook"]),)
                .unwrap(),
            Execution::default()
        );

        engine
            .execute(
                &mut context,
                &command("set-hook", &["-g", "@myhook", hook_command]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("set-hook", &["-g", "-u", "@myhook"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "-q", "-v", "@myhook"]),
                )
                .unwrap(),
            Execution::default()
        );

        engine
            .execute(
                &mut context,
                &command("set-hook", &["-g", "@myhook", hook_command]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "-u", "@myhook"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("set-hook", &["-R", "@myhook"]),)
                .unwrap(),
            Execution::default()
        );

        engine
            .execute(
                &mut context,
                &command("set-hook", &["-w", "@scoped", "display-message window"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-hook", &["-g", "@scoped", "display-message session"]),
            )
            .unwrap();
        let session_scoped = engine
            .execute(&mut context, &command("set-hook", &["-R", "@scoped"]))
            .unwrap();
        assert!(matches!(
            session_scoped.effects.as_slice(),
            [MuxEffect::RunHook { commands, .. }]
                if commands == &vec![parse_hook_commands(&engine, "display-message session").unwrap()]
        ));
        engine
            .execute(&mut context, &command("set-hook", &["-g", "-u", "@scoped"]))
            .unwrap();
        let window_scoped = engine
            .execute(&mut context, &command("set-hook", &["-R", "@scoped"]))
            .unwrap();
        assert!(matches!(
            window_scoped.effects.as_slice(),
            [MuxEffect::RunHook { commands, .. }]
                if commands == &vec![parse_hook_commands(&engine, "display-message window").unwrap()]
        ));
    }

    #[test]
    fn hook_validation_and_bound_command_support_match_the_command_surface() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        for (args, expected) in [
            (&[] as &[&str], "missing argument"),
            (&["not-a-hook"], "invalid option: not-a-hook"),
            (&["after-"], "ambiguous option: after-"),
            (&["-B", "name:what:format"], "invalid flag -B"),
        ] {
            assert!(matches!(
                engine.execute(&mut context, &command("set-hook", args)),
                Err(ServerError::InvalidCommand(message)) if message == expected
            ));
        }
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-hook", &["base-index[0]", "1"]),
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "not an array: base-index[0]"
        ));
        assert!(matches!(
            engine.execute(&mut context, &command("show-hooks", &["-B"])),
            Err(ServerError::InvalidCommand(message)) if message == "invalid flag -B"
        ));

        let invalid = "display-message '";
        let expected = crate::parse_config("<set-hook>", invalid)
            .diagnostics
            .into_iter()
            .next()
            .expect("parser diagnostic")
            .message;
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-hook", &["after-select-window", invalid]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == expected
        ));

        assert!(matches!(
            engine.execute(
                &mut context,
                &command(
                    "set-hook",
                    &[
                        "after-select-window",
                        "display-message -p A=#{window_name}",
                    ],
                ),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "syntax error"
        ));
        for value in [
            "display-message -p 'A=#{window_name}'",
            r#"display-message -p "A=#{window_name}""#,
        ] {
            engine
                .execute(
                    &mut context,
                    &command("set-hook", &["after-select-window", value]),
                )
                .unwrap();
        }

        engine
            .execute(
                &mut context,
                &command(
                    "bind-key",
                    &[
                        "x",
                        "set-hook",
                        "after-select-window",
                        "display-message bound",
                    ],
                ),
            )
            .expect("set-hook binding");
        assert_eq!(
            engine.keys.get("prefix", "x").expect("binding").commands,
            [CommandInvocation::new(
                "set-hook",
                ["after-select-window", "display-message bound"],
            )]
        );
    }

    #[test]
    fn event_hooks_follow_declared_scope_and_isolate_windows() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first_window = context.window.expect("first window");
        let first_pane = context.pane.expect("first pane");
        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .unwrap();
        let second_window = context.window.expect("second window");
        let second_pane = context.pane.expect("second pane");
        let first_window_target = first_window.to_string();
        let second_window_target = second_window.to_string();
        let first_pane_target = first_pane.to_string();

        for args in [
            &["-g", "pane-died", "display-message global-window"] as &[&str],
            &[
                "-w",
                "-t",
                first_window_target.as_str(),
                "pane-died",
                "display-message first-window",
            ],
        ] {
            engine
                .execute(&mut context, &command("set-hook", args))
                .unwrap();
        }
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "show-hooks",
                        &["-w", "-t", first_window_target.as_str(), "pane-died",],
                    ),
                )
                .unwrap()
                .output,
            "pane-died[0] display-message first-window"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "show-hooks",
                        &["-w", "-t", second_window_target.as_str(), "pane-died",],
                    ),
                )
                .unwrap()
                .output,
            ""
        );

        let first_context =
            ExecutionContext::for_pane(&engine.state, first_pane).expect("first context");
        let second_context =
            ExecutionContext::for_pane(&engine.state, second_pane).expect("second context");
        assert_eq!(
            engine
                .event_hook_commands(&first_context, "pane-died")
                .expect("first window hook"),
            [parse_hook_commands(&engine, "display-message first-window").unwrap()]
        );
        assert_eq!(
            engine
                .event_hook_commands(&second_context, "pane-died")
                .expect("second window fallback"),
            [parse_hook_commands(&engine, "display-message global-window").unwrap()]
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &[
                        "-p",
                        "-t",
                        first_pane_target.as_str(),
                        "pane-died",
                        "display-message first-pane",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .event_hook_commands(&first_context, "pane-died")
                .expect("first pane hook"),
            [parse_hook_commands(&engine, "display-message first-pane").unwrap()]
        );
        engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &["-p", "-u", "-t", first_pane_target.as_str(), "pane-died"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .event_hook_commands(&first_context, "pane-died")
                .expect("first window hook after pane unset"),
            [parse_hook_commands(&engine, "display-message first-window").unwrap()]
        );

        for args in [
            &["-g", "alert-bell", "display-message global-session"] as &[&str],
            &["alert-bell", "display-message local-session"],
        ] {
            engine
                .execute(&mut context, &command("set-hook", args))
                .unwrap();
        }
        assert_eq!(
            engine
                .event_hook_commands(&second_context, "alert-bell")
                .expect("local session hook"),
            [parse_hook_commands(&engine, "display-message local-session").unwrap()]
        );
        engine
            .execute(&mut context, &command("set-hook", &["-u", "alert-bell"]))
            .unwrap();
        assert_eq!(
            engine
                .event_hook_commands(&second_context, "alert-bell")
                .expect("global session fallback"),
            [parse_hook_commands(&engine, "display-message global-session").unwrap()]
        );
    }

    #[test]
    fn set_hook_run_now_returns_effective_units_in_index_order() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.expect("pane");
        for args in [
            &["-g", "after-select-window[2]", "display-message second"] as &[&str],
            &["-g", "after-select-window[0]", "display-message first"],
            &["-g", "after-select-window[named]", "display-message named"],
        ] {
            engine
                .execute(&mut context, &command("set-hook", args))
                .unwrap();
        }

        let execution = engine
            .execute(
                &mut context,
                &command(
                    "set-hook",
                    &[
                        "-R",
                        "-t",
                        &pane.to_string(),
                        "after-select-window",
                        "ignored",
                    ],
                ),
            )
            .unwrap();
        let expected_commands = [
            parse_hook_commands(&engine, "display-message first").unwrap(),
            parse_hook_commands(&engine, "display-message second").unwrap(),
            parse_hook_commands(&engine, "display-message named").unwrap(),
        ];
        assert!(matches!(
            execution.effects.as_slice(),
            [MuxEffect::RunHook {
                name,
                commands,
                context: target,
            }] if name == "after-select-window"
                && target.pane == Some(pane)
                && commands == &expected_commands
        ));
        assert_eq!(
            engine
                .execute(&mut context, &command("set-hook", &["-R", "unknown-hook"]),)
                .unwrap(),
            Execution::default()
        );
    }

    #[test]
    fn hook_format_variables_cover_arguments_positionals_and_flags() {
        let variables = hook_format_variables(
            &CommandInvocation::new("select-window", ["-T", "-t", "work:1", "-t", "work:2"]),
            "after-select-window",
        );
        assert_eq!(variables["hook"], "after-select-window");
        assert_eq!(variables["hook_arguments"], "-T -t work:1 -t work:2");
        assert_eq!(variables["hook_flag_T"], "1");
        assert_eq!(variables["hook_flag_t"], "work:2");
        assert_eq!(variables["hook_flag_t_0"], "work:1");
        assert_eq!(variables["hook_flag_t_1"], "work:2");
        assert!(!variables.contains_key("hook_argument_0"));

        let variables = hook_format_variables(
            &CommandInvocation::new("set-option", ["-g", "@name", "two words"]),
            "after-set-option",
        );
        assert_eq!(variables["hook_arguments"], "-g @name \"two words\"");
        assert_eq!(variables["hook_argument_0"], "@name");
        assert_eq!(variables["hook_argument_1"], "two words");
        assert_eq!(variables["hook_flag_g"], "1");
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
                    session: None,
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
                    session: None,
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
                session: None,
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
                session: None,
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
            (
                "show-window-options",
                vec!["-gv", "aggressive-resize"],
                "off",
            ),
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
            ("show-options", vec!["-gv", "initial-repeat-time"], "0"),
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
        assert_eq!(engine.default_terminal_for_spawn(), "tmux-256color");
    }

    #[test]
    fn aggressive_resize_stores_inherits_and_unsets_window_values() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let window = context.window.unwrap();

        let global = engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "aggressive-resize", "on"]),
            )
            .unwrap();
        assert_eq!(
            global.effects,
            vec![
                MuxEffect::AggressiveResizeChanged { window: None },
                MuxEffect::SnapshotChanged,
            ]
        );
        assert!(engine.state.global_aggressive_resize());
        assert!(engine.state.window_aggressive_resize(window).unwrap());
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-gv", "aggressive-resize"]),
                )
                .unwrap()
                .output,
            "on"
        );

        let local = engine
            .execute(
                &mut context,
                &command("set-window-option", &["aggressive-resize", "off"]),
            )
            .unwrap();
        assert_eq!(
            local.effects,
            vec![
                MuxEffect::AggressiveResizeChanged {
                    window: Some(window),
                },
                MuxEffect::SnapshotChanged,
            ]
        );
        assert!(!engine.state.window_aggressive_resize(window).unwrap());
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-v", "aggressive-resize"]),
                )
                .unwrap()
                .output,
            "off"
        );

        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-u", "aggressive-resize"]),
            )
            .unwrap();
        assert!(engine.state.window_aggressive_resize(window).unwrap());
        assert_eq!(
            engine
                .state
                .window_aggressive_resize_override(window)
                .unwrap(),
            None
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-wAv", "aggressive-resize"]),
                )
                .unwrap()
                .output,
            "on"
        );

        let error = engine
            .execute(
                &mut context,
                &command("set-window-option", &["aggressive-resize", "maybe"]),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            ServerError::InvalidCommand(message) if message == "bad value: maybe"
        ));
    }

    #[test]
    fn v71_option_writes_emit_scoped_mux_option_effects() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();

        let global = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "mouse", "off"]),
            )
            .unwrap();
        assert_eq!(
            global.effects,
            [MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::Mouse,
                session: None,
            }]
        );
        assert_eq!(engine.mux_option_value(MuxOptionKey::Mouse), "off");
        assert!(!engine.effective_mouse(Some(session)));

        let scoped = engine
            .execute(&mut context, &command("set-option", &["mouse", "on"]))
            .unwrap();
        assert_eq!(
            scoped.effects,
            [MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::Mouse,
                session: Some(session),
            }]
        );
        assert_eq!(engine.mux_option_value(MuxOptionKey::Mouse), "off");
        assert!(engine.effective_mouse(Some(session)));
        assert!(!engine.effective_mouse(None));

        let escape = engine
            .execute(
                &mut context,
                &command("set-option", &["-s", "escape-time", "50"]),
            )
            .unwrap();
        assert_eq!(
            escape.effects,
            [MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::EscapeTime,
                session: None,
            }]
        );
        assert_eq!(engine.mux_option_value(MuxOptionKey::EscapeTime), "50");

        let prefix2 = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "prefix2", "C-a"]),
            )
            .unwrap();
        assert_eq!(
            prefix2.effects,
            [MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::Prefix2,
                session: None,
            }]
        );
        assert_eq!(engine.mux_option_value(MuxOptionKey::Prefix2), "C-a");

        let session_prefix2 = engine
            .execute(&mut context, &command("set-option", &["prefix2", "C-s"]))
            .unwrap();
        assert!(session_prefix2.effects.is_empty());
        assert_eq!(engine.mux_option_value(MuxOptionKey::Prefix2), "C-a");

        let unset = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "-u", "prefix2"]),
            )
            .unwrap();
        assert_eq!(
            unset.effects,
            [MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::Prefix2,
                session: None,
            }]
        );
        assert_eq!(engine.mux_option_value(MuxOptionKey::Prefix2), "None");
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
            &["initial-repeat-time", "900"],
            &["repeat-time", "650"],
        ] {
            engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
        }
        for (name, expected) in [
            ("mouse", "on"),
            ("display-time", "1200"),
            ("initial-repeat-time", "900"),
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
        assert_eq!(engine.initial_repeat_time_for_session(session), 900);
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
        assert_eq!(engine.default_terminal_for_spawn(), "zz-term");
        engine
            .execute(
                &mut context,
                &command("set-option", &["-su", "default-terminal"]),
            )
            .unwrap();
        assert_eq!(engine.default_terminal_for_spawn(), "tmux-256color");
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
                vec!["-g", "initial-repeat-time", "2000001"],
                "value is too large: 2000001",
            ),
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

        for name in [
            "mouse",
            "display-time",
            "initial-repeat-time",
            "repeat-time",
        ] {
            engine
                .execute(&mut context, &command("set-option", &["-u", name]))
                .unwrap();
        }
        assert!(engine.mouse_for_session(session));
        assert_eq!(engine.display_time_for_session(session), 750);
        assert_eq!(engine.initial_repeat_time_for_session(session), 0);
        assert_eq!(engine.repeat_time_for_session(session), 500);
    }

    #[test]
    fn bare_flag_and_choice_options_toggle_like_the_pin() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        for expected in ["off", "on"] {
            engine
                .execute(&mut context, &command("set-option", &["-g", "mouse"]))
                .unwrap();
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &["-gv", "mouse"]))
                    .unwrap()
                    .output,
                expected
            );
        }

        for expected in ["on", "off"] {
            engine
                .execute(
                    &mut context,
                    &command("set-window-option", &["-g", "remain-on-exit"]),
                )
                .unwrap();
            assert_eq!(
                engine
                    .execute(
                        &mut context,
                        &command("show-window-options", &["-gv", "remain-on-exit"]),
                    )
                    .unwrap()
                    .output,
                expected
            );
        }
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "remain-on-exit", "failed"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "remain-on-exit"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-gv", "remain-on-exit"]),
                )
                .unwrap()
                .output,
            "failed"
        );

        for expected in ["on", "off"] {
            engine
                .execute(
                    &mut context,
                    &command("set-window-option", &["-g", "allow-passthrough"]),
                )
                .unwrap();
            assert_eq!(
                engine
                    .execute(
                        &mut context,
                        &command("show-window-options", &["-gv", "allow-passthrough"],),
                    )
                    .unwrap()
                    .output,
                expected
            );
        }
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "allow-passthrough", "all"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "allow-passthrough"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-gv", "allow-passthrough"],),
                )
                .unwrap()
                .output,
            "all"
        );

        for expected in ["blinking-block", "default"] {
            engine
                .execute(
                    &mut context,
                    &command("set-window-option", &["-g", "cursor-style"]),
                )
                .unwrap();
            assert_eq!(
                engine
                    .execute(
                        &mut context,
                        &command("show-window-options", &["-gv", "cursor-style"]),
                    )
                    .unwrap()
                    .output,
                expected
            );
        }
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "cursor-style", "bar"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "cursor-style"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-gv", "cursor-style"]),
                )
                .unwrap()
                .output,
            "bar"
        );

        for expected in ["off", "external"] {
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-s", "set-clipboard"]),
                )
                .unwrap();
            assert_eq!(
                engine
                    .execute(
                        &mut context,
                        &command("show-options", &["-sv", "set-clipboard"]),
                    )
                    .unwrap()
                    .output,
                expected
            );
        }
        engine
            .execute(
                &mut context,
                &command("set-option", &["-s", "set-clipboard", "on"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-s", "set-clipboard"]),
            )
            .unwrap();
        assert_eq!(engine.mux_option_value(MuxOptionKey::SetClipboard), "on");

        engine
            .execute(&mut context, &command("set-option", &["-g", "status", "2"]))
            .unwrap();
        engine
            .execute(&mut context, &command("set-option", &["-g", "status"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-gv", "status"]))
                .unwrap()
                .output,
            "2"
        );

        for expected in ["on", "off"] {
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-g", "experimental-agent-pane"]),
                )
                .unwrap();
            assert_eq!(
                engine.mux_option_value(MuxOptionKey::ExperimentalAgentPane),
                expected
            );
        }
    }

    #[test]
    fn honest_knob_families_store_inherit_read_back_and_reject_pin_values() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        let window = context.window.unwrap();
        let pane = context.pane.unwrap();

        for (args, expected) in [
            (vec!["-sv", "focus-events"], "off"),
            (vec!["-sv", "history-file"], "\n"),
            (vec!["-sv", "prefix-timeout"], "0"),
            (vec!["-sv", "prompt-history-limit"], "100"),
            (vec!["-gv", "bell-action"], "any"),
            (vec!["-gv", "default-size"], "80x24"),
            (vec!["-gv", "display-panes-time"], "1000"),
            (vec!["-gv", "key-table"], "root"),
            (vec!["-gv", "visual-bell"], "off"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &args))
                    .unwrap()
                    .output,
                expected
            );
        }
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-s", "history-file"]),
                )
                .unwrap()
                .output,
            "history-file ''"
        );
        for (name, expected) in [
            ("main-pane-height", "24"),
            ("main-pane-width", "80"),
            ("other-pane-height", "0"),
            ("other-pane-width", "0"),
            ("tiled-layout-max-columns", "0"),
            ("window-size", "latest"),
            ("wrap-search", "on"),
            ("allow-passthrough", "off"),
            ("allow-rename", "off"),
            ("allow-set-title", "on"),
            ("alternate-screen", "on"),
            ("cursor-colour", "\n"),
            ("cursor-style", "default"),
            ("scroll-on-clear", "on"),
        ] {
            assert_eq!(
                engine
                    .execute(
                        &mut context,
                        &command("show-window-options", &["-gv", name]),
                    )
                    .unwrap()
                    .output,
                expected
            );
        }

        for args in [
            &["-s", "focus-events", "on"] as &[&str],
            &["-s", "history-file", "/tmp/history"],
            &["-s", "prefix-timeout", "250"],
            &["-s", "prompt-history-limit", "3"],
            &["bell-action", "other"],
            &["default-size", "132x43"],
            &["display-panes-time", "1400"],
            &["key-table", "custom"],
            &["visual-bell", "both"],
        ] {
            engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
        }
        for args in [
            &["main-pane-height", "40%"] as &[&str],
            &["main-pane-width", "120"],
            &["other-pane-height", "8"],
            &["other-pane-width", "12%"],
            &["tiled-layout-max-columns", "2"],
            &["window-size", "largest"],
            &["wrap-search", "off"],
        ] {
            engine
                .execute(&mut context, &command("set-window-option", args))
                .unwrap();
        }
        for args in [
            &["-p", "allow-passthrough", "all"] as &[&str],
            &["-p", "allow-rename", "on"] as &[&str],
            &["-p", "allow-set-title", "off"],
            &["-p", "alternate-screen", "off"],
            &["-p", "cursor-colour", "sky blue"],
            &["-p", "cursor-style", "blinking-underline"],
            &["-p", "scroll-on-clear", "off"],
        ] {
            engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
        }

        assert!(engine.focus_events());
        assert_eq!(engine.history_file(), "/tmp/history");
        assert_eq!(engine.prefix_timeout_ms(), 250);
        assert_eq!(engine.prompt_history_limit(), 3);
        assert_eq!(engine.bell_action_for_session(session), BellAction::Other);
        assert_eq!(engine.visual_bell_for_session(session), VisualBell::Both);
        assert_eq!(engine.key_table_for_session(session), "custom");
        assert_eq!(engine.display_panes_time_for_session(session), 1400);
        assert_eq!(engine.window_size(window), WindowSize::Largest);
        assert!(!engine.allow_set_title(pane));
        assert_eq!(
            engine.terminal_worker_options_for_pane(pane).unwrap(),
            TerminalWorkerOptions {
                allow_passthrough: true,
                wrap_search: false,
                cursor_style: "blinking-underline",
                cursor_colour: "sky blue".to_owned(),
            }
        );
        assert_eq!(
            engine.preset_options_for_window(window).main_pane_height,
            "40%"
        );

        for (args, expected) in [
            (vec!["-s", "focus-events", "maybe"], "bad value: maybe"),
            (vec!["-s", "prefix-timeout"], "empty value"),
            (
                vec!["-s", "prompt-history-limit", "-1"],
                "value is too small: -1",
            ),
            (vec!["-g", "bell-action", "maybe"], "unknown value: maybe"),
            (vec!["-g", "default-size", "bad"], "value is invalid: bad"),
            (
                vec!["-g", "display-panes-time", "0"],
                "value is too small: 0",
            ),
            (vec!["-g", "visual-bell", "maybe"], "unknown value: maybe"),
            (
                vec!["-gw", "tiled-layout-max-columns", "65536"],
                "value is too large: 65536",
            ),
            (vec!["-gw", "window-size", "maybe"], "unknown value: maybe"),
            (vec!["-gw", "wrap-search", "maybe"], "bad value: maybe"),
            (
                vec!["-gp", "allow-passthrough", "maybe"],
                "unknown value: maybe",
            ),
            (vec!["-gp", "allow-set-title", "maybe"], "bad value: maybe"),
            (vec!["-gp", "alternate-screen", "maybe"], "bad value: maybe"),
            (
                vec!["-gp", "cursor-colour", "not-a-colour"],
                "invalid colour: not-a-colour",
            ),
            (vec!["-gp", "cursor-colour"], "empty value"),
            (
                vec!["-gp", "cursor-style", "Blinking-Bar"],
                "unknown value: Blinking-Bar",
            ),
            (vec!["-gp", "scroll-on-clear", "maybe"], "bad value: maybe"),
        ] {
            assert!(matches!(
                engine.execute(&mut context, &command("set-option", &args)),
                Err(ServerError::InvalidCommand(message)) if message == expected
            ));
        }

        engine
            .execute(&mut context, &command("set-option", &["-u", "key-table"]))
            .unwrap();
        assert_eq!(engine.key_table_for_session(session), "root");
    }

    #[test]
    fn lane2_scalar_families_store_read_back_unset_and_validate() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "lane2"]))
            .unwrap();
        for (flags, name, expected) in [
            ("-sv", "backspace", "C-?"),
            ("-sv", "default-client-command", "new-session"),
            ("-sv", "editor", "/usr/bin/vi"),
            ("-sv", "extended-keys", "off"),
            ("-sv", "extended-keys-format", "xterm"),
            ("-sv", "get-clipboard", "buffer"),
            ("-sv", "input-buffer-size", "1048576"),
            ("-sv", "variation-selector-always-wide", "on"),
            ("-gv", "assume-paste-time", "1"),
            ("-gv", "message-line", "0"),
            ("-gv", "prompt-cursor-colour", "\n"),
            ("-gv", "prompt-command-cursor-colour", "\n"),
            ("-gv", "prompt-cursor-style", "default"),
            ("-gv", "prompt-command-cursor-style", "default"),
            ("-gwv", "clock-mode-colour", "themeblue"),
            ("-gwv", "clock-mode-style", "24"),
            ("-gwv", "fill-character", "\n"),
            ("-gwv", "pane-border-indicators", "colour"),
            ("-gwv", "pane-border-lines", "single"),
            ("-gwv", "pane-scrollbars", "off"),
            ("-gwv", "pane-scrollbars-timeout", "500"),
            (
                "-gwv",
                "pane-scrollbars-style",
                PANE_SCROLLBARS_STYLE_DEFAULT,
            ),
            ("-gwv", "pane-scrollbars-position", "right"),
            ("-gwv", "xterm-keys", "on"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &[flags, name]))
                    .unwrap()
                    .output,
                expected,
                "{name}"
            );
        }
        for (name, expected) in [
            ("message-command-style", MESSAGE_COMMAND_STYLE_DEFAULT),
            ("message-format", MESSAGE_FORMAT_DEFAULT),
            ("message-style", MESSAGE_STYLE_DEFAULT),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &["-gv", name]))
                    .unwrap()
                    .output,
                expected,
                "{name}"
            );
        }

        for args in [
            &["-s", "backspace", "BSpace"] as &[&str],
            &["-s", "default-client-command", "neww -d"],
            &["-s", "editor", "nvim"],
            &["-s", "extended-keys", "always"],
            &["-s", "extended-keys-format", "csi-u"],
            &["-s", "get-clipboard", "both"],
            &["-s", "input-buffer-size", "1048577"],
            &["-s", "variation-selector-always-wide", "off"],
            &["-g", "assume-paste-time", "9"],
            &["-g", "message-command-style", "fg=red"],
            &["-g", "message-format", "message"],
            &["-g", "message-line", "4"],
            &["-g", "message-style", "bg=blue"],
            &["-g", "prompt-cursor-colour", "red"],
            &["-g", "prompt-command-cursor-colour", "blue"],
            &["-g", "prompt-cursor-style", "block"],
            &["-g", "prompt-command-cursor-style", "bar"],
            &["-gw", "clock-mode-colour", "cyan"],
            &["-gw", "clock-mode-style", "12-with-seconds"],
            &["-gw", "fill-character", "x"],
            &["-gw", "pane-border-indicators", "arrows"],
            &["-gw", "pane-border-lines", "heavy"],
            &["-gw", "pane-scrollbars", "auto-hide"],
            &["-gw", "pane-scrollbars-timeout", "750"],
            &["-gw", "pane-scrollbars-style", "fg=white"],
            &["-gw", "pane-scrollbars-position", "left"],
            &["-gw", "xterm-keys", "off"],
        ] {
            engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
        }
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-sv", "default-client-command"]),
                )
                .unwrap()
                .output,
            "new-window -d"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-ag", "message-style", "bold"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "message-style"]),
                )
                .unwrap()
                .output,
            "bg=blue,bold"
        );
        engine
            .execute(&mut context, &command("set-option", &["-su", "editor"]))
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-sv", "editor"]))
                .unwrap()
                .output,
            "/usr/bin/vi"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-p", "pane-border-lines", "double"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-wU", "pane-border-lines"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-pA", "pane-border-lines"]),
                )
                .unwrap()
                .output,
            "pane-border-lines* heavy"
        );

        for (args, expected) in [
            (
                &["-s", "backspace", "not-a-key"] as &[&str],
                "bad key: not-a-key",
            ),
            (
                &["-s", "extended-keys", "sometimes"],
                "unknown value: sometimes",
            ),
            (
                &["-s", "input-buffer-size", "1048575"],
                "value is too small: 1048575",
            ),
            (
                &["-s", "variation-selector-always-wide", "maybe"],
                "bad value: maybe",
            ),
            (
                &["-g", "prompt-cursor-colour", "not-a-colour"],
                "invalid colour: not-a-colour",
            ),
            (
                &["-gw", "pane-scrollbars-style", "not-a-style"],
                "invalid style: not-a-style",
            ),
            (
                &["-gw", "pane-scrollbars-timeout", "nope"],
                "value is invalid: nope",
            ),
        ] {
            assert!(matches!(
                engine.execute(&mut context, &command("set-option", args)),
                Err(ServerError::InvalidCommand(message)) if message == expected
            ));
        }
    }

    #[test]
    fn boot_resolved_editor_is_visible_through_option_readback() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine.initialize_default_editor("nvim");

        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-sv", "editor"]),)
                .unwrap()
                .output,
            "nvim"
        );
        assert!(
            engine
                .execute(&mut context, &command("show-options", &["-s"]))
                .unwrap()
                .output
                .lines()
                .any(|line| line == "editor nvim")
        );
    }

    #[test]
    fn every_remaining_named_scalar_stores_and_bare_listings_cover_the_pin_table() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-s", "all-options"]),
            )
            .unwrap();

        let storage_only = tmux_options()
            .filter(|option| tmux_stored_scalar(option.name).is_some())
            .collect::<Vec<_>>();
        assert_eq!(storage_only.len(), 63);
        for option in storage_only {
            let metadata = tmux_stored_scalar(option.name).expect("storage-only metadata");
            let alternate = match metadata.kind {
                TmuxStoredScalarKind::String => "stored",
                TmuxStoredScalarKind::Style => "fg=red",
                TmuxStoredScalarKind::Colour => "red",
                TmuxStoredScalarKind::Flag => {
                    if metadata.default == "on" {
                        "off"
                    } else {
                        "on"
                    }
                }
                TmuxStoredScalarKind::Choice(choices) => choices
                    .iter()
                    .copied()
                    .find(|choice| *choice != metadata.default)
                    .expect("choice has an alternate"),
                TmuxStoredScalarKind::Key => "C-a",
            };
            let set_flag = match option.scope {
                TmuxOptionScope::Server => "-s",
                TmuxOptionScope::Session => "-g",
                TmuxOptionScope::Window | TmuxOptionScope::WindowPane => "-gw",
            };
            engine
                .execute(
                    &mut context,
                    &command("set-option", &[set_flag, option.name, alternate]),
                )
                .unwrap_or_else(|error| panic!("{}: {error}", option.name));
            let show_flag = match option.scope {
                TmuxOptionScope::Server => "-sv",
                TmuxOptionScope::Session => "-gv",
                TmuxOptionScope::Window | TmuxOptionScope::WindowPane => "-gwv",
            };
            assert_eq!(
                engine
                    .execute(
                        &mut context,
                        &command("show-options", &[show_flag, option.name]),
                    )
                    .unwrap()
                    .output,
                alternate,
                "{}",
                option.name
            );
            let unset_flag = match option.scope {
                TmuxOptionScope::Server => "-su",
                TmuxOptionScope::Session => "-gu",
                TmuxOptionScope::Window | TmuxOptionScope::WindowPane => "-gwu",
            };
            engine
                .execute(
                    &mut context,
                    &command("set-option", &[unset_flag, option.name]),
                )
                .unwrap();
            assert_eq!(
                engine
                    .execute(
                        &mut context,
                        &command("show-options", &[show_flag, option.name]),
                    )
                    .unwrap()
                    .output,
                metadata.default,
                "{}",
                option.name
            );
        }

        let mut listed = BTreeSet::new();
        for flags in [["-s"].as_slice(), ["-g"].as_slice(), ["-gw"].as_slice()] {
            for line in engine
                .execute(&mut context, &command("show-options", flags))
                .unwrap()
                .output
                .lines()
            {
                let name = line
                    .split_ascii_whitespace()
                    .next()
                    .expect("listed option name")
                    .split('[')
                    .next()
                    .expect("base option name");
                listed.insert(name.to_owned());
            }
        }
        let expected = tmux_options()
            .filter(|option| !tmux_option_is_hook(option.name))
            .map(|option| option.name)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        assert_eq!(listed, expected);

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "destroy-unattached", "keep-last"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["destroy-unattached", "on"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-v", "destroy-unattached"]),
                )
                .unwrap()
                .output,
            "on"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-u", "destroy-unattached"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-Av", "destroy-unattached"]),
                )
                .unwrap()
                .output,
            "keep-last"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-gw", "window-style", "fg=red"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-p", "window-style", "fg=blue"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-wU", "window-style"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-pAv", "window-style"]),
                )
                .unwrap()
                .output,
            "fg=red"
        );

        for (args, expected) in [
            (
                &["-s", "theme", "automatic"] as &[&str],
                "unknown value: automatic",
            ),
            (
                &["-g", "display-panes-colour", "not-a-colour"],
                "invalid colour: not-a-colour",
            ),
            (
                &["-gw", "copy-mode-match-style", "not-a-style"],
                "invalid style: not-a-style",
            ),
            (&["-g", "prefix2", "not-a-key"], "bad key: not-a-key"),
        ] {
            assert!(matches!(
                engine.execute(&mut context, &command("set-option", args)),
                Err(ServerError::InvalidCommand(message)) if message == expected
            ));
        }
    }

    #[test]
    fn lane2_monitor_options_store_read_back_unset_and_validate_without_effects() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        for (flags, name, expected) in [
            ("-gv", "activity-action", "other"),
            ("-gv", "silence-action", "other"),
            ("-gwv", "monitor-activity", "off"),
            ("-gwv", "monitor-bell", "on"),
            ("-gwv", "monitor-silence", "0"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &[flags, name]))
                    .unwrap()
                    .output,
                expected,
                "{name}"
            );
        }

        for args in [
            &["-g", "activity-action", "any"] as &[&str],
            &["-g", "silence-action", "none"],
            &["-gw", "monitor-activity", "on"],
            &["-gw", "monitor-bell", "off"],
            &["-gw", "monitor-silence", "30"],
        ] {
            let execution = engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
            assert!(execution.effects.is_empty());
        }
        for (flags, name, expected) in [
            ("-gv", "activity-action", "any"),
            ("-gv", "silence-action", "none"),
            ("-gwv", "monitor-activity", "on"),
            ("-gwv", "monitor-bell", "off"),
            ("-gwv", "monitor-silence", "30"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &[flags, name]))
                    .unwrap()
                    .output,
                expected,
                "{name}"
            );
        }

        for args in [
            &["-gu", "activity-action"] as &[&str],
            &["-gu", "silence-action"],
            &["-gwu", "monitor-activity"],
            &["-gwu", "monitor-bell"],
            &["-gwu", "monitor-silence"],
        ] {
            engine
                .execute(&mut context, &command("set-option", args))
                .unwrap();
        }
        for (flags, name, expected) in [
            ("-gv", "activity-action", "other"),
            ("-gv", "silence-action", "other"),
            ("-gwv", "monitor-activity", "off"),
            ("-gwv", "monitor-bell", "on"),
            ("-gwv", "monitor-silence", "0"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &[flags, name]))
                    .unwrap()
                    .output,
                expected,
                "{name}"
            );
        }

        for args in [
            &["-g", "activity-action", "all"] as &[&str],
            &["-g", "silence-action", "ALL"],
            &["-gw", "monitor-activity", "maybe"],
            &["-gw", "monitor-bell", "maybe"],
            &["-gw", "monitor-silence", "-1"],
        ] {
            assert!(
                engine
                    .execute(&mut context, &command("set-option", args))
                    .is_err()
            );
        }
    }

    #[test]
    fn terminal_knob_effects_preserve_global_window_and_pane_scope() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let window = context.window.unwrap();
        let pane = context.pane.unwrap();

        let window_effect = engine
            .execute(
                &mut context,
                &command("set-window-option", &["wrap-search", "off"]),
            )
            .unwrap();
        assert_eq!(
            window_effect.effects,
            vec![MuxEffect::TerminalKnobsChanged {
                window: Some(window),
                pane: None,
            }]
        );

        let pane_effect = engine
            .execute(
                &mut context,
                &command("set-option", &["-p", "cursor-style", "bar"]),
            )
            .unwrap();
        assert_eq!(
            pane_effect.effects,
            vec![MuxEffect::TerminalKnobsChanged {
                window: None,
                pane: Some(pane),
            }]
        );

        let global_effect = engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "allow-passthrough", "all"]),
            )
            .unwrap();
        assert_eq!(
            global_effect.effects,
            vec![MuxEffect::TerminalKnobsChanged {
                window: None,
                pane: None,
            }]
        );

        for name in ["alternate-screen", "scroll-on-clear"] {
            let store_only = engine
                .execute(
                    &mut context,
                    &command("set-window-option", &["-g", name, "off"]),
                )
                .unwrap();
            assert!(store_only.effects.is_empty());
        }
    }

    #[test]
    fn same_session_reparenting_refreshes_terminal_worker_knobs() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-s", "work", "-n", "source"]),
            )
            .unwrap();
        let moving = context.pane.unwrap();
        let source_window = context.window.unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-t", &source_window.to_string(), "wrap-search", "off"],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-p", "-t", &moving.to_string(), "cursor-style", "bar"],
                ),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "destination"]))
            .unwrap();
        let target = context.pane.unwrap();
        let destination_window = context.window.unwrap();

        let joined = engine
            .execute(
                &mut context,
                &command(
                    "join-pane",
                    &["-d", "-s", &moving.to_string(), "-t", &target.to_string()],
                ),
            )
            .unwrap();
        assert!(joined.effects.contains(&MuxEffect::TerminalKnobsChanged {
            window: None,
            pane: Some(moving),
        }));
        let joined_options = engine.terminal_worker_options_for_pane(moving).unwrap();
        assert!(joined_options.wrap_search);
        assert_eq!(joined_options.cursor_style, "bar");

        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-t", &destination_window.to_string(), "wrap-search", "off"],
                ),
            )
            .unwrap();
        let broken = engine
            .execute(
                &mut context,
                &command("break-pane", &["-d", "-s", &moving.to_string()]),
            )
            .unwrap();
        assert!(broken.effects.contains(&MuxEffect::TerminalKnobsChanged {
            window: None,
            pane: Some(moving),
        }));
        let broken_window = engine.state.window_for_pane(moving).unwrap();
        let broken_options = engine.terminal_worker_options_for_pane(moving).unwrap();
        assert!(broken_options.wrap_search);
        assert_eq!(broken_options.cursor_style, "bar");

        let swapped = engine
            .execute(
                &mut context,
                &command(
                    "swap-pane",
                    &["-d", "-s", &moving.to_string(), "-t", &target.to_string()],
                ),
            )
            .unwrap();
        for pane in [moving, target] {
            assert!(swapped.effects.contains(&MuxEffect::TerminalKnobsChanged {
                window: None,
                pane: Some(pane),
            }));
        }
        assert_eq!(
            engine.state.window_for_pane(moving),
            Some(destination_window)
        );
        assert_eq!(engine.state.window_for_pane(target), Some(broken_window));
        assert!(
            !engine
                .terminal_worker_options_for_pane(moving)
                .unwrap()
                .wrap_search
        );
        assert!(
            engine
                .terminal_worker_options_for_pane(target)
                .unwrap()
                .wrap_search
        );
        assert_eq!(
            engine
                .terminal_worker_options_for_pane(moving)
                .unwrap()
                .cursor_style,
            "bar"
        );
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
    fn runtime_facts_apply_automatic_window_names_and_dead_signals() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();
        let window = context.window.unwrap();

        assert!(engine.set_pane_runtime_facts(
            pane,
            PaneRuntimeFacts {
                current_command: "sleep".to_owned(),
                ..PaneRuntimeFacts::default()
            },
        ));
        assert_eq!(engine.state.windows[&window].name, "sleep");
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("list-windows", &["-F", "#{window_name}"]),
                )
                .unwrap()
                .output,
            "sleep"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["automatic-rename-format", "PFX-#{pane_current_command}-SFX"],
                ),
            )
            .unwrap();
        assert!(engine.set_pane_runtime_facts(
            pane,
            PaneRuntimeFacts {
                current_command: "fish".to_owned(),
                ..PaneRuntimeFacts::default()
            },
        ));
        assert_eq!(engine.state.windows[&window].name, "PFX-fish-SFX");

        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-u", "automatic-rename-format"]),
            )
            .unwrap();
        assert!(
            engine
                .mark_pane_dead(pane, None, Some("Terminated: 15"))
                .unwrap()
        );
        assert_eq!(engine.state.windows[&window].name, "fish[dead]");
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "display-message",
                        ["-p", "#{pane_dead}:#{pane_dead_status}:#{pane_dead_signal}"].as_slice(),
                    ),
                )
                .unwrap()
                .output,
            "1::term"
        );

        engine
            .execute(&mut context, &command("rename-window", &["manual"]))
            .unwrap();
        assert!(engine.set_pane_runtime_facts(
            pane,
            PaneRuntimeFacts {
                current_command: "vim".to_owned(),
                ..PaneRuntimeFacts::default()
            },
        ));
        assert_eq!(engine.state.windows[&window].name, "manual");
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
                    empty: false,
                },
                MuxEffect::SnapshotChanged,
            ] if *actual == pane
                && cwd == "/tmp"
                && command.as_slice() == ["printf ready"]
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
    fn respawn_empty_is_live_and_can_be_respawned_without_kill() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("list-commands", &["respawn-pane"]),)
                .unwrap()
                .output,
            "respawn-pane (respawnp) [-Ek] [-c start-directory] [-e environment] [-t target-pane] [shell-command [argument ...]]"
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("list-commands", &["respawn-window"]),)
                .unwrap()
                .output,
            "respawn-window (respawnw) [-Ek] [-c start-directory] [-e environment] [-t target-window] [shell-command [argument ...]]"
        );
        engine.state.mark_pane_dead(pane, Some(7)).unwrap();

        let empty = engine
            .execute(
                &mut context,
                &command(
                    "respawn-pane",
                    &["-E", "-t", &pane.to_string(), "printf ready"],
                ),
            )
            .unwrap();
        assert!(matches!(
            empty.effects.as_slice(),
            [MuxEffect::PaneRespawned {
                pane: actual,
                command: Some(command),
                empty: true,
                ..
            }, MuxEffect::SnapshotChanged] if *actual == pane && command.as_slice() == ["printf ready"]
        ));
        let pane_state = engine.state.pane(pane).unwrap();
        assert!(!pane_state.dead);
        assert!(pane_state.empty);

        let respawned = engine
            .execute(
                &mut context,
                &command("respawn-pane", &["-t", &pane.to_string()]),
            )
            .unwrap();
        assert!(matches!(
            respawned.effects.as_slice(),
            [MuxEffect::PaneRespawned {
                pane: actual,
                command: None,
                empty: false,
                ..
            }, MuxEffect::SnapshotChanged] if *actual == pane
        ));
        assert!(!engine.state.pane(pane).unwrap().empty);
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
                            "#{pane_start_command}|#{pane_start_command_list}",
                        ],
                    ),
                )
                .unwrap()
                .output,
            "\"printf ready\"|'printf ready'"
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
    fn default_shell_is_concrete_validated_and_unsets_to_the_table_default() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "default-shell"]),
                )
                .unwrap()
                .output,
            "/bin/sh"
        );
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.unwrap();
        let mut hooks = CommandHooks::new(0);
        let mut valid = |shell: &str| matches!(shell, "/bin/sh" | "/valid-shell");

        engine
            .execute_with_shell_validator(
                &mut context,
                &command("set-option", &["-g", "default-shell", "/valid-shell"]),
                &mut hooks,
                &mut valid,
            )
            .unwrap();
        assert_eq!(
            engine.default_shell_for_session(session).unwrap(),
            "/valid-shell"
        );

        assert!(matches!(
            engine.execute_with_shell_validator(
                &mut context,
                &command("set-option", &["-g", "default-shell", "/invalid"]),
                &mut hooks,
                &mut valid,
            ),
            Err(ServerError::InvalidCommand(message)) if message == "not a suitable shell: /invalid"
        ));
        assert_eq!(engine.global_default_shell(), "/valid-shell");

        assert!(matches!(
            engine.execute_with_shell_validator(
                &mut context,
                &command("set-option", &["-ga", "default-shell", "-invalid"]),
                &mut hooks,
                &mut valid,
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "not a suitable shell: /valid-shell-invalid"
        ));
        assert_eq!(engine.global_default_shell(), "/valid-shell");

        engine
            .execute_with_shell_validator(
                &mut context,
                &command("set-option", &["default-shell", "/bin/sh"]),
                &mut hooks,
                &mut valid,
            )
            .unwrap();
        assert_eq!(
            engine.default_shell_for_session(session).unwrap(),
            "/bin/sh"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-u", "default-shell"]),
            )
            .unwrap();
        assert_eq!(
            engine.default_shell_for_session(session).unwrap(),
            "/valid-shell"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-gu", "default-shell"]),
            )
            .unwrap();
        assert_eq!(engine.global_default_shell(), "/bin/sh");
        assert_eq!(
            engine.default_shell_for_session(session).unwrap(),
            "/bin/sh"
        );
    }

    #[test]
    fn session_spawn_string_append_uses_only_the_sessions_own_value() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "default-command", "AAA"]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("new-session", &["-s", "q"]))
            .unwrap();
        let session = context.session.unwrap();

        engine
            .execute(
                &mut context,
                &command("set-option", &["-at", "q", "default-command", "BBB"]),
            )
            .unwrap();
        assert_eq!(engine.default_command_for_session(session).unwrap(), "BBB");

        engine
            .execute(
                &mut context,
                &command("set-option", &["-t", "q", "default-command", "SSS"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-at", "q", "default-command", "BBB"]),
            )
            .unwrap();
        assert_eq!(
            engine.default_command_for_session(session).unwrap(),
            "SSSBBB"
        );

        let mut hooks = CommandHooks::new(0);
        let mut valid = |shell: &str| shell == "/bin";
        engine
            .execute_with_shell_validator(
                &mut context,
                &command("set-option", &["-g", "default-shell", "/bin"]),
                &mut hooks,
                &mut valid,
            )
            .unwrap();
        assert!(matches!(
            engine.execute_with_shell_validator(
                &mut context,
                &command("set-option", &["-at", "q", "default-shell", "/sh"]),
                &mut hooks,
                &mut valid,
            ),
            Err(ServerError::InvalidCommand(message)) if message == "not a suitable shell: /sh"
        ));
    }

    #[test]
    fn option_table_defaults_match_the_engine_except_history_limit() {
        let engine = MuxEngine::default();
        let mismatches = tmux_options()
            .filter_map(|option| option.default.map(|default| (option, default)))
            .filter_map(|(option, default)| {
                let expected = default.value();
                let runtime = engine
                    .global_tmux_option_value(option.name)
                    .unwrap_or_else(|| panic!("missing runtime default for {}", option.name));
                (runtime != expected).then_some(option.name)
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
        for (option, expected) in [
            ("status-style", "bg=themegreen,fg=themeblack"),
            ("status-bg", "default"),
            ("status-fg", "default"),
            ("status-left-style", "default"),
            ("status-right-style", "default"),
            ("status-left-length", "10"),
            ("status-right-length", "40"),
            ("status-justify", "left"),
            ("status-position", "bottom"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &["-gv", option]))
                    .unwrap()
                    .output,
                expected,
                "{option}"
            );
        }
        for (option, expected) in [
            (
                "window-status-format",
                "#I:#W#{?window_flags,#{window_flags}, }",
            ),
            (
                "window-status-current-format",
                "#I:#W#{?window_flags,#{window_flags}, }",
            ),
            ("window-status-separator", " "),
            ("window-status-style", "default"),
            ("window-status-current-style", "underscore"),
            ("window-status-last-style", "default"),
            ("window-status-bell-style", "reverse"),
            ("window-status-activity-style", "reverse"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &["-gwv", option]))
                    .unwrap()
                    .output,
                expected,
                "{option}"
            );
        }
    }

    #[test]
    fn status_options_store_inherit_unset_and_validate() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session");
        let window = context.window.expect("window");

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status-left-length", "12"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-t", "work", "status-left-length", "5"]),
            )
            .unwrap();
        assert_eq!(
            engine.status_formats_for_session(Some(session)).left_length,
            5
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-ut", "work", "status-left-length"]),
            )
            .unwrap();
        assert_eq!(
            engine.status_formats_for_session(Some(session)).left_length,
            12
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-g", "window-status-format", "plain:#I:#W"],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-t", "work:0", "window-status-current-format", "current:#W"],
                ),
            )
            .unwrap();
        let formats = engine.window_status_formats(window);
        assert_eq!(formats.format, "plain:#I:#W");
        assert_eq!(formats.current_format, "current:#W");

        for (option, value, error) in [
            ("status-style", "bogus", "invalid style: bogus"),
            ("status-bg", "bogus", "bad colour: bogus"),
            ("status-left-length", "-1", "value is too small: -1"),
            ("status-right-length", "32768", "value is too large: 32768"),
            ("status-justify", "middle", "unknown value: middle"),
            ("status-position", "middle", "unknown value: middle"),
        ] {
            assert!(matches!(
                engine.execute(
                    &mut context,
                    &command("set-option", &["-g", option, value]),
                ),
                Err(ServerError::InvalidCommand(message)) if message == error
            ));
        }
    }

    #[test]
    fn status_style_options_defer_expansion_and_append_with_commas() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        engine
            .execute(
                &mut context,
                &command("set-option", &["-ag", "status-style", "fg=white"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "status-style"]),
                )
                .unwrap()
                .output,
            "bg=themegreen,fg=themeblack,fg=white"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status-left-style", "bold"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-ag", "status-left-style", "italics"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "status-left-style"]),
                )
                .unwrap()
                .output,
            "bold,italics"
        );

        let dynamic = "fg=#{?client_prefix,red,green}";
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status-right-style", dynamic]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-ag", "status-right-style", "bold"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "status-right-style"]),
                )
                .unwrap()
                .output,
            format!("{dynamic},bold")
        );

        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "window-status-style", dynamic]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-ag", "window-status-style", "italics"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gwv", "window-status-style"]),
                )
                .unwrap()
                .output,
            format!("{dynamic},italics")
        );
    }

    #[test]
    fn bare_status_option_listings_include_the_full_wave_a_family() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();

        let session = engine
            .execute(&mut context, &command("show-options", &["-g"]))
            .unwrap()
            .output;
        for name in [
            "status-style",
            "status-bg",
            "status-fg",
            "status-left-style",
            "status-right-style",
            "status-left-length",
            "status-right-length",
            "status-justify",
            "status-position",
        ] {
            let prefix = format!("{name} ");
            assert!(
                session.lines().any(|line| line.starts_with(&prefix)),
                "bare session listing omitted {name}"
            );
        }

        let window = engine
            .execute(&mut context, &command("show-options", &["-gw"]))
            .unwrap()
            .output;
        for name in [
            "window-status-format",
            "window-status-current-format",
            "window-status-separator",
            "window-status-style",
            "window-status-current-style",
            "window-status-last-style",
            "window-status-bell-style",
            "window-status-activity-style",
        ] {
            let prefix = format!("{name} ");
            assert!(
                window.lines().any(|line| line.starts_with(&prefix)),
                "bare window listing omitted {name}"
            );
        }
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

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status-format", "one,two three"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "status-format"]),
                )
                .unwrap()
                .output,
            "status-format[0] one\nstatus-format[1] two\nstatus-format[2] three"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-ga", "status-format[1]", " tail"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "status-format[1]"]),
                )
                .unwrap()
                .output,
            "status-format[1] \"two tail\""
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-gu", "status-format[0]"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-ga", "status-format", "reused"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "status-format[0]"]),
                )
                .unwrap()
                .output,
            "reused"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-s", "terminal-features", "one,two"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-sv", "terminal-features"]),
                )
                .unwrap()
                .output,
            "one\ntwo"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-gw", "pane-colors", "red,blue"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gw", "pane-colours"]),
                )
                .unwrap()
                .output,
            "pane-colours[0] red\npane-colours[1] blue"
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-option", &["-gw", "pane-colours[0]", "value"]),
            ),
            Err(ServerError::InvalidCommand(message)) if message == "bad colour: value"
        ));

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
    fn status_format_array_exposes_sparse_session_effective_entries() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session id");

        let defaults = engine.status_format_array_for_session(None);
        assert_eq!(
            defaults.keys().copied().collect::<Vec<_>>(),
            [0, 1, 2],
            "the pinned defaults materialize three rows"
        );
        assert_eq!(
            engine.status_format_array_for_session(Some(session)),
            defaults,
            "sessions without overrides inherit the global array"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status-format[4]", "tail"]),
            )
            .unwrap();
        let sparse = engine.status_format_array_for_session(None);
        assert_eq!(sparse.keys().copied().collect::<Vec<_>>(), [0, 1, 2, 4]);
        assert!(!sparse.contains_key(&3), "missing indices stay missing");

        engine
            .execute(
                &mut context,
                &command("set-option", &["status-format[3]", "local"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["status-format[1]", ""]),
            )
            .unwrap();
        let scoped = engine.status_format_array_for_session(Some(session));
        assert_eq!(
            scoped,
            BTreeMap::from([(1, String::new()), (3, "local".to_owned())]),
            "a session array overrides whole and keeps explicit-empty rows distinct from unset"
        );
        assert_eq!(
            engine.status_format_array_for_session(None),
            sparse,
            "the global array is untouched by session writes"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-u", "status-format"]),
            )
            .unwrap();
        assert_eq!(
            engine.status_format_array_for_session(Some(session)),
            sparse
        );
    }

    #[test]
    fn explicit_status_writes_flip_and_clear_customized() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session id");
        assert!(!engine.status_customized_for_session(None));
        assert!(!engine.status_customized_for_session(Some(session)));

        let same_as_default = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status", "on"]),
            )
            .unwrap();
        assert_eq!(
            same_as_default.effects,
            [MuxEffect::StatusFormatsChanged { session: None }],
            "an explicit write equal to the default still publishes"
        );
        assert!(engine.status_customized_for_session(None));
        assert!(engine.status_customized_for_session(Some(session)));

        let cleared = engine
            .execute(&mut context, &command("set-option", &["-gu", "status"]))
            .unwrap();
        assert_eq!(
            cleared.effects,
            [MuxEffect::StatusFormatsChanged { session: None }]
        );
        assert!(!engine.status_customized_for_session(None));
        assert!(!engine.status_customized_for_session(Some(session)));

        engine
            .execute(
                &mut context,
                &command("set-option", &["status-left", "[#S] "]),
            )
            .unwrap();
        assert!(!engine.status_customized_for_session(None));
        assert!(engine.status_customized_for_session(Some(session)));
        engine
            .execute(&mut context, &command("set-option", &["-u", "status-left"]))
            .unwrap();
        assert!(!engine.status_customized_for_session(Some(session)));

        engine
            .execute(
                &mut context,
                &command("set-option", &["status-format[0]", "row"]),
            )
            .unwrap();
        assert!(engine.status_customized_for_session(Some(session)));
        engine
            .execute(
                &mut context,
                &command("set-option", &["-u", "status-format"]),
            )
            .unwrap();
        assert!(!engine.status_customized_for_session(Some(session)));
    }

    #[test]
    fn status_format_writes_emit_status_effects_with_their_scope() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session id");

        let global = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status-format[0]", "global-row"]),
            )
            .unwrap();
        assert_eq!(
            global.effects,
            [MuxEffect::StatusFormatsChanged { session: None }]
        );

        let scoped = engine
            .execute(
                &mut context,
                &command("set-option", &["status-format[3]", "scoped-row"]),
            )
            .unwrap();
        assert_eq!(
            scoped.effects,
            [MuxEffect::StatusFormatsChanged {
                session: Some(session)
            }]
        );

        let unchanged = engine
            .execute(
                &mut context,
                &command("set-option", &["status-format[3]", "scoped-row"]),
            )
            .unwrap();
        assert!(unchanged.effects.is_empty());

        let scoped_unset = engine
            .execute(
                &mut context,
                &command("set-option", &["-u", "status-format"]),
            )
            .unwrap();
        assert_eq!(
            scoped_unset.effects,
            [MuxEffect::StatusFormatsChanged {
                session: Some(session)
            }]
        );

        let global_unset = engine
            .execute(
                &mut context,
                &command("set-option", &["-gu", "status-format"]),
            )
            .unwrap();
        assert_eq!(
            global_unset.effects,
            [MuxEffect::StatusFormatsChanged { session: None }]
        );
    }

    #[test]
    fn message_line_reads_session_effective_values_and_publishes_changes() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session id");
        assert_eq!(engine.message_line_for_session(None), 0);
        assert_eq!(engine.message_line_for_session(Some(session)), 0);

        let global = engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "message-line", "4"]),
            )
            .unwrap();
        assert_eq!(
            global.effects,
            [MuxEffect::StatusFormatsChanged { session: None }]
        );
        assert_eq!(engine.message_line_for_session(Some(session)), 4);

        let scoped = engine
            .execute(&mut context, &command("set-option", &["message-line", "2"]))
            .unwrap();
        assert_eq!(
            scoped.effects,
            [MuxEffect::StatusFormatsChanged {
                session: Some(session)
            }]
        );
        assert_eq!(engine.message_line_for_session(Some(session)), 2);
        assert_eq!(engine.message_line_for_session(None), 4);
        assert!(
            !engine.status_customized_for_session(Some(session)),
            "message-line is not part of the status customization ledger"
        );

        let unset = engine
            .execute(
                &mut context,
                &command("set-option", &["-u", "message-line"]),
            )
            .unwrap();
        assert_eq!(
            unset.effects,
            [MuxEffect::StatusFormatsChanged {
                session: Some(session)
            }]
        );
        assert_eq!(engine.message_line_for_session(Some(session)), 4);
    }

    #[test]
    fn status_row_variables_carry_session_effective_option_values() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session id");
        engine
            .execute(
                &mut context,
                &command("set-option", &["status-left", "LOCAL "]),
            )
            .unwrap();

        let global = engine.status_row_variables_for_session(None);
        assert_eq!(
            global.get("status-left").map(String::as_str),
            Some(crate::tmux_options::STATUS_LEFT_DEFAULT)
        );
        assert_eq!(
            global.get("status-justify").map(String::as_str),
            Some("left")
        );
        assert_eq!(
            global.get("window-status-format").map(String::as_str),
            Some(crate::DEFAULT_WINDOW_STATUS_FORMAT)
        );
        assert_eq!(
            global.get("pane-status-current-style").map(String::as_str),
            Some("underscore")
        );

        let scoped = engine.status_row_variables_for_session(Some(session));
        assert_eq!(
            scoped.get("status-left").map(String::as_str),
            Some("LOCAL ")
        );
    }

    #[test]
    fn per_window_status_overrides_reach_the_label_surface_but_not_row_variables() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let session = context.session.expect("session id");
        let window = context.window.expect("window id");
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-t", "work:0", "window-status-current-format", "OVERRIDE"],
                ),
            )
            .unwrap();

        let formats = engine.window_status_formats(window);
        assert_eq!(
            formats.current_format, "OVERRIDE",
            "the status_label surface honors the per-window override"
        );
        let variables = engine.status_row_variables_for_session(Some(session));
        assert_eq!(
            variables
                .get("window-status-current-format")
                .map(String::as_str),
            Some(crate::DEFAULT_WINDOW_STATUS_FORMAT),
            "the row-loop variables map stays global: the ledgered scoping divergence"
        );
    }

    #[test]
    fn lane2_array_storage_and_hook_option_routing_match_the_pin_shapes() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "lane2"]))
            .unwrap();

        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-s", "command-alias"]),
                )
                .unwrap()
                .output,
            concat!(
                "command-alias[0] split-pane=split-window\n",
                "command-alias[1] splitp=split-window\n",
                "command-alias[2] \"server-info=show-messages -JT\"\n",
                "command-alias[3] \"info=show-messages -JT\"\n",
                "command-alias[4] \"choose-window=choose-tree -w\"\n",
                "command-alias[5] \"choose-session=choose-tree -s\"",
            )
        );
        for (name, value) in [
            ("command-alias", "alias=display-message"),
            ("codepoint-widths", "U+41=1"),
            ("terminal-overrides", "term:RGB"),
            ("terminal-features", "term:title"),
            ("user-keys", "abc"),
        ] {
            let indexed = format!("{name}[7]");
            engine
                .execute(
                    &mut context,
                    &command("set-option", &["-s", &indexed, value]),
                )
                .unwrap();
            assert_eq!(
                engine
                    .execute(&mut context, &command("show-options", &["-sv", &indexed]),)
                    .unwrap()
                    .output,
                value,
                "{name}"
            );
        }
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "update-environment", "FOO BAR"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-g", "update-environment"]),
                )
                .unwrap()
                .output,
            "update-environment[0] FOO\nupdate-environment[1] BAR"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-s", "codepoint-widths", ""]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-s", "codepoint-widths"]),
                )
                .unwrap()
                .output,
            "codepoint-widths"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-sv", "codepoint-widths"]),
                )
                .unwrap()
                .output,
            ""
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "status-format", "global"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["status-format"]),)
                .unwrap()
                .output,
            ""
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-A", "status-format"]),
                )
                .unwrap()
                .output,
            "status-format[0]* global"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["status-format[3]", "local"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-A", "status-format"]),
                )
                .unwrap()
                .output,
            "status-format[3] local"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["-u", "status-format"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-A", "status-format"]),
                )
                .unwrap()
                .output,
            "status-format[0]* global"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-p", "pane-colours[0]", "red"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-wU", "pane-colours"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-pA", "pane-colours"]),
                )
                .unwrap()
                .output,
            "pane-colours*"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "alert-bell", "display-message"]),
            )
            .unwrap();
        let through_hooks = engine
            .execute(&mut context, &command("show-hooks", &["-g", "alert-bell"]))
            .unwrap()
            .output;
        let through_options = engine
            .execute(
                &mut context,
                &command("show-options", &["-g", "alert-bell"]),
            )
            .unwrap()
            .output;
        assert_eq!(through_options, through_hooks);
        assert_eq!(through_options, "alert-bell[0] display-message");
        engine
            .execute(
                &mut context,
                &command("set-option", &["-ga", "alert-bell", "kill-pane"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "alert-bell"]),
                )
                .unwrap()
                .output,
            "display-message\nkill-pane"
        );

        let server_listing = engine
            .execute(&mut context, &command("show-options", &["-s"]))
            .unwrap()
            .output;
        assert!(
            server_listing.find("command-alias[0]").unwrap()
                < server_listing.find("codepoint-widths").unwrap()
        );
        assert!(
            server_listing.find("terminal-overrides[0]").unwrap()
                < server_listing.find("terminal-features[0]").unwrap()
        );
        let window_listing = engine
            .execute(&mut context, &command("show-options", &["-gw"]))
            .unwrap()
            .output;
        let scrollbars = window_listing.find("pane-scrollbars off").unwrap();
        let timeout = window_listing.find("pane-scrollbars-timeout 500").unwrap();
        let style = window_listing.find("pane-scrollbars-style ").unwrap();
        let position = window_listing
            .find("pane-scrollbars-position right")
            .unwrap();
        assert!(scrollbars < timeout && timeout < style && style < position);
        assert!(!server_listing.contains("alert-bell"));
        assert!(!window_listing.contains("alert-bell"));
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
                let name = name.split('[').next().expect("option name before index");
                assert!(name.starts_with('@') || tmux_names.contains(name), "{line}");
                assert!(!is_native_option(name), "{line}");
                assert!(!tmux_option_is_hook(name), "{line}");
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
    fn set_options_expand_names_and_dash_f_expands_values() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@#{session_name}", "#{window_name}"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-gv", "@work"]),)
                .unwrap()
                .output,
            "#{window_name}"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &[
                        "-Fg",
                        "@#{session_name}",
                        "#{session_name}:#{window_index}.#{pane_index}",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(&mut context, &command("show-options", &["-gv", "@work"]),)
                .unwrap()
                .output,
            "work:0.0"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &[
                        "-F",
                        "@window_#{window_index}",
                        "#{window_name}:#{pane_index}",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-v", "@window_0"]),
                )
                .unwrap()
                .output,
            "0:0"
        );

        engine
            .execute(
                &mut context,
                &command("new-window", &["-d", "-t", "work:1", "-n", "logs"]),
            )
            .unwrap();
        engine
            .execute(&mut context, &command("split-window", &[]))
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-F", "-t", "1", "@target", "#{window_index}.#{pane_index}"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-window-options", &["-t", "1", "-v", "@target"]),
                )
                .unwrap()
                .output,
            "1.0"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &[
                        "-gF",
                        "-t",
                        "missing",
                        "@fallback",
                        "#{session_name}:#{window_index}.#{pane_index}",
                    ],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "@fallback"]),
                )
                .unwrap()
                .output,
            "work:0.1"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-option",
                    &["-gF", "history-trickle", "#{?session_name,2001,0}"],
                ),
            )
            .unwrap();
        assert_eq!(engine.history_trickle(), 2001);
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
                    session: None,
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
                    session: None,
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
    fn tree_chooser_preserves_session_window_and_pane_scopes() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let pane = context.pane.unwrap();

        assert_eq!(
            engine
                .execute(&mut context, &command("choose-tree", &["-Zs"]))
                .unwrap()
                .effects,
            vec![MuxEffect::ChooseTree {
                pane,
                kind: ChooseTreeKind::Windows,
                sessions_only: true,
                filter: None,
                sort: TmuxSort::parse(None, false, Some(TmuxSortOrder::Index)).unwrap(),
            }]
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "choose-tree",
                        &["-Zw", "-f", "#{pane_active}", "-Oname", "-r"]
                    ),
                )
                .unwrap()
                .effects,
            vec![MuxEffect::ChooseTree {
                pane,
                kind: ChooseTreeKind::Windows,
                sessions_only: false,
                filter: Some("#{pane_active}".to_owned()),
                sort: TmuxSort::parse(Some("name"), true, Some(TmuxSortOrder::Index)).unwrap(),
            }]
        );
        assert_eq!(
            engine
                .execute(&mut context, &command("focus-sidebar", &[]))
                .unwrap()
                .effects,
            vec![MuxEffect::FocusSidebar { pane }]
        );

        assert_eq!(
            engine
                .execute(&mut context, &command("choose-tree", &[]))
                .unwrap()
                .effects,
            vec![MuxEffect::ChooseTree {
                pane,
                kind: ChooseTreeKind::Panes,
                sessions_only: false,
                filter: None,
                sort: TmuxSort::parse(None, false, Some(TmuxSortOrder::Index)).unwrap(),
            }]
        );

        assert_eq!(
            engine
                .execute(&mut context, &command("choose-tree", &["-sw"]))
                .unwrap()
                .effects,
            vec![MuxEffect::ChooseTree {
                pane,
                kind: ChooseTreeKind::Windows,
                sessions_only: true,
                filter: None,
                sort: TmuxSort::parse(None, false, Some(TmuxSortOrder::Index)).unwrap(),
            }]
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("choose-tree", &["select-pane", "-t", "%%"]),
            ),
            Err(ServerError::InvalidCommand(_))
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command(
                    "choose-tree",
                    &["-t", "missing", "-O", "not-an-order"],
                ),
            ),
            Err(ServerError::PaneNotFound(target)) if target == "missing"
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
            vec![MuxEffect::ChooseBuffer {
                pane,
                filter: None,
                sort: TmuxSort::parse(None, false, Some(TmuxSortOrder::Creation)).unwrap(),
            }]
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
        assert!(matches!(
            engine.execute(
                &mut context,
                &command(
                    "choose-buffer",
                    &["-t", "missing", "-O", "not-an-order"],
                ),
            ),
            Err(ServerError::PaneNotFound(target)) if target == "missing"
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
            ("display-panes", Vec::new(), 1_000),
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
                duration_ms: 1_000,
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
        engine
            .execute(
                &mut context,
                &command("set-option", &["display-panes-time", "1200"]),
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
    fn select_pane_changes_pane_activity_without_touching_window_or_session_selection() {
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
        let window_activity = engine.state.windows[&first_window].activity;
        let first_active_point = engine.state.windows[&first_window].panes[&first].active_point;

        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &first.to_string()]),
            )
            .unwrap();
        assert_eq!(engine.state.sessions[&session].active_window, first_window);
        assert_eq!(engine.state.windows[&first_window].active_pane, first);
        assert_eq!(context.pane, Some(first));
        assert_eq!(
            engine.state.windows[&first_window].activity,
            window_activity
        );
        assert!(
            engine.state.windows[&first_window].panes[&first].active_point > first_active_point
        );

        engine
            .execute(&mut context, &command("new-window", &["-n", "other"]))
            .unwrap();
        let other_window = context.window.unwrap();
        let other_pane = context.pane.unwrap();
        let first_window_activity = engine.state.windows[&first_window].activity;
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
        assert_eq!(
            engine.state.windows[&first_window].activity,
            first_window_activity
        );

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
        assert!(
            rows.contains(&"if-shell (if) [-bF] [-t target-pane] shell-command command [command]")
        );
        assert!(rows.contains(
            &"paste-buffer (pasteb) [-dprS] [-s separator] [-b buffer-name] [-t target-pane]"
        ));
        assert!(rows.contains(
            &"run-shell (run) [-bCE] [-c start-directory] [-d delay] [-t target-pane] [shell-command [argument ...]]"
        ));
        assert!(rows.contains(&"wait-for (wait) [-L|-S|-U] channel"));
        assert!(rows.contains(&"pipe-pane (pipep) [-IOo] [-t target-pane] [shell-command]"));
        assert_eq!(
            engine
                .execute(&mut context, &command("list-commands", &["capturep"]))
                .unwrap()
                .output,
            "capture-pane (capturep) [-aeJMNpqT] [-b buffer-name] [-E end-line] [-S start-line] [-t target-pane]"
        );
        for name in ["if-shell", "if"] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("list-commands", &[name]))
                    .unwrap()
                    .output,
                "if-shell (if) [-bF] [-t target-pane] shell-command command [command]"
            );
        }
        for name in ["run-shell", "run"] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("list-commands", &[name]))
                    .unwrap()
                    .output,
                "run-shell (run) [-bCE] [-c start-directory] [-d delay] [-t target-pane] [shell-command [argument ...]]"
            );
        }
        for name in ["wait-for", "wait"] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("list-commands", &[name]))
                    .unwrap()
                    .output,
                "wait-for (wait) [-L|-S|-U] channel"
            );
        }
        for name in ["pipe-pane", "pipep"] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("list-commands", &[name]))
                    .unwrap()
                    .output,
                "pipe-pane (pipep) [-IOo] [-t target-pane] [shell-command]"
            );
        }
        assert_eq!(
            engine
                .execute(&mut context, &command("start", &[]))
                .unwrap(),
            Execution::default()
        );
    }

    #[test]
    fn if_shell_truthiness_uses_the_first_byte() {
        for value in ["", "0", "0abc"] {
            assert!(!if_shell_truthy(value), "{value:?}");
        }
        for value in [" 0", "1", "false"] {
            assert!(if_shell_truthy(value), "{value:?}");
        }
    }

    #[test]
    fn config_condition_formats_use_pin_truth_options_environment_and_no_jobs() {
        let mut engine = MuxEngine::default();
        engine.set_format_server_context("tower.local", "tower", "/tmp/zz.sock", 1);
        engine.seed_global_environment([
            ("WAVE_ENV", "yes"),
            ("host", "environment-must-not-shadow"),
        ]);
        engine.set_config_environment("HIDDEN_WAVE".to_owned(), "yes".to_owned(), true);
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "prefix", "C-a"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "@grammar-wave", "yes"]),
            )
            .unwrap();

        for condition in [
            "00",
            "0.0",
            "false",
            " ",
            "#{==:#{host},tower.local}",
            "#{==:#{prefix},C-a}",
            "#{==:#{@grammar-wave},yes}",
            "#{==:#{WAVE_ENV},yes}",
            "#{==:#{HIDDEN_WAVE},yes}",
            "#{==:#(printf spawned),}",
        ] {
            assert!(engine.evaluate_config_condition(condition), "{condition}");
        }
        for condition in ["", "0", "#(printf spawned)", "#{session_name}"] {
            assert!(!engine.evaluate_config_condition(condition), "{condition}");
        }
    }

    #[test]
    fn config_conditions_observe_preparse_state_not_same_file_commands() {
        let mut engine = MuxEngine::default();
        let parsed = engine.parse_config(
            "test.conf",
            "set-option -g @grammar-wave yes\n\
             %if #{@grammar-wave}\n\
             set-option -g @branch same-file\n\
             %else\n\
             set-option -g @branch preparse\n\
             %endif\n",
        );
        assert_eq!(parsed.commands.len(), 2);
        assert_eq!(parsed.commands[1].args[2], "preparse");

        engine
            .execute(
                &mut ExecutionContext::default(),
                &command("set-option", &["-g", "@grammar-wave", "yes"]),
            )
            .unwrap();
        let parsed = engine.parse_config(
            "test.conf",
            "%if #{@grammar-wave}\nset-option -g @branch preexisting\n%endif\n",
        );
        assert_eq!(parsed.commands.len(), 1);
        assert_eq!(parsed.commands[0].args[2], "preexisting");
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

    #[test]
    fn popup_options_keep_window_inheritance_independent() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let first = context.window.unwrap();
        engine
            .execute(&mut context, &command("new-window", &["-n", "second"]))
            .unwrap();
        let second = context.window.unwrap();
        let second_target = second.to_string();
        assert_eq!(
            engine.popup_options_for_window(first).unwrap(),
            PopupOptions::default()
        );
        assert_eq!(
            engine.popup_options_for_window(second).unwrap(),
            PopupOptions::default()
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-g", "popup-style", "bg=blue,fg=white"],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-o", "-t", &second_target, "popup-style", "bg=red"],
                ),
            )
            .unwrap();
        assert!(matches!(
            engine.execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-o", "-t", &second_target, "popup-style", "bg=green"]
                )
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "already set: popup-style"
        ));
        assert_eq!(
            engine.popup_options_for_window(first).unwrap().style,
            "bg=blue,fg=white"
        );
        assert_eq!(
            engine.popup_options_for_window(second).unwrap().style,
            "bg=red"
        );
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-u", "-t", &second_target, "popup-style"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine.popup_options_for_window(second).unwrap().style,
            "bg=blue,fg=white"
        );

        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &[
                        "-t",
                        &second_target,
                        "popup-border-style",
                        "bg=black,fg=cyan",
                    ],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-window-option", &["-g", "popup-border-lines", "double"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-t", &second_target, "popup-border-lines", "rounded"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine.popup_options_for_window(first).unwrap(),
            PopupOptions {
                style: "bg=blue,fg=white".to_owned(),
                border_style: PopupOptions::default().border_style,
                border_lines: PopupBorderLines::Double,
            }
        );
        assert_eq!(
            engine.popup_options_for_window(second).unwrap(),
            PopupOptions {
                style: "bg=blue,fg=white".to_owned(),
                border_style: "bg=black,fg=cyan".to_owned(),
                border_lines: PopupBorderLines::Rounded,
            }
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "show-window-options",
                        &["-t", &second_target, "-v", "popup-border-style"],
                    ),
                )
                .unwrap()
                .output,
            "bg=black,fg=cyan"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command(
                        "show-window-options",
                        &["-t", &second_target, "-v", "popup-border-lines"],
                    ),
                )
                .unwrap()
                .output,
            "rounded"
        );
        engine
            .execute(
                &mut context,
                &command(
                    "set-window-option",
                    &["-u", "-t", &second_target, "popup-border-lines"],
                ),
            )
            .unwrap();
        assert_eq!(
            engine
                .popup_options_for_window(second)
                .unwrap()
                .border_lines,
            PopupBorderLines::Double
        );
        let session_listing = engine
            .execute(&mut context, &command("show-options", &["-g"]))
            .unwrap()
            .output;
        let window_listing = engine
            .execute(&mut context, &command("show-options", &["-gw"]))
            .unwrap()
            .output;
        for option in ["popup-style", "popup-border-style"] {
            let prefix = format!("{option} ");
            assert!(
                !session_listing
                    .lines()
                    .any(|line| line.starts_with(&prefix))
            );
            assert!(window_listing.lines().any(|line| line.starts_with(&prefix)));
        }
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-window-option", &["popup-border-lines", "zigzag"])
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "unknown value: zigzag"
        ));
    }

    #[test]
    fn menu_and_lock_options_store_inherit_validate_and_read_back() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let window = context.window.unwrap();
        assert_eq!(
            engine.menu_options_for_window(window).unwrap(),
            MenuOptions::default()
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "lock-command"]),
                )
                .unwrap()
                .output,
            "lock -np"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-gv", "lock-after-time"]),
                )
                .unwrap()
                .output,
            "0"
        );
        engine
            .execute(
                &mut context,
                &command("set-option", &["lock-command", "secure-lock"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["lock-after-time", "90"]),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-v", "lock-command"]),
                )
                .unwrap()
                .output,
            "secure-lock"
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-v", "lock-after-time"]),
                )
                .unwrap()
                .output,
            "90"
        );

        engine
            .execute(
                &mut context,
                &command("set-option", &["-g", "menu-style", "bg=blue,fg=white"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["menu-selected-style", "bg=yellow,fg=black"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["menu-border-style", "fg=cyan"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("set-option", &["menu-border-lines", "rounded"]),
            )
            .unwrap();
        assert_eq!(
            engine.menu_options_for_window(window).unwrap(),
            MenuOptions {
                style: "bg=blue,fg=white".to_owned(),
                selected_style: "bg=yellow,fg=black".to_owned(),
                border_style: "fg=cyan".to_owned(),
                border_lines: PopupBorderLines::Rounded,
            }
        );
        assert_eq!(
            engine
                .execute(
                    &mut context,
                    &command("show-options", &["-v", "menu-border-lines"]),
                )
                .unwrap()
                .output,
            "rounded"
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-option", &["menu-border-lines", "zigzag"])
            ),
            Err(ServerError::InvalidCommand(message)) if message == "unknown value: zigzag"
        ));
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-option", &["menu-style", "fg=not-a-colour"])
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "invalid style: fg=not-a-colour"
        ));
    }

    #[test]
    fn popup_style_options_reject_unknown_tokens_at_global_and_local_scope() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();

        for (scope, option, value) in [
            ("-gw", "popup-style", "fg=red,bg=#00ff7f,bold,nounderscore"),
            ("-gw", "popup-border-style", "fg=colour255,bg=black,italics"),
            ("-w", "popup-style", "default,none,bright|reverse"),
            ("-w", "popup-border-style", "fg=cyan,bg=colour0,noitalics"),
        ] {
            let args = vec![scope, option, value];
            engine
                .execute(&mut context, &command("set-option", &args))
                .unwrap();
        }

        for (scope, option, value) in [
            ("-gw", "popup-style", "bogus-not-a-style"),
            ("-gw", "popup-border-style", "fg=not-a-colour"),
            ("-w", "popup-style", "bold,unknown"),
            ("-w", "popup-border-style", "bg=#12345g"),
        ] {
            let args = vec![scope, option, value];
            assert!(matches!(
                engine.execute(&mut context, &command("set-option", &args)),
                Err(ServerError::InvalidCommand(message))
                    if message == format!("invalid style: {value}")
            ));
        }
    }

    #[test]
    fn style_validator_accepts_the_pinned_style_parse_keys() {
        for value in [
            "align=left",
            "align=centre",
            "align=right",
            "align=absolute-centre",
            "noalign",
            "fill=red",
            "us=blue",
            "list=on",
            "list=focus",
            "list=left-marker",
            "list=right-marker",
            "nolist",
            "range=left",
            "range=right",
            "range=control|9",
            "range=pane|%12",
            "range=window|12",
            "range=session|$12",
            "range=user|owner",
            "range=custom",
            "range=custom|value",
            "norange",
            "push-default",
            "pop-default",
            "set-default",
            "ignore",
            "noignore",
            "dim=50%",
            "width=80%",
            "pad=2",
            "link=https://example.com",
            "nolink",
            "nodefault",
            "nobold|underscore",
        ] {
            assert!(valid_style(value), "{value}");
        }
    }

    #[test]
    fn style_validator_rejects_invalid_pinned_values() {
        for value in [
            "fg=colour256",
            "fg=#zzzzzz",
            "bogus",
            "fg=",
            "align=middle",
            "fill=",
            "us=",
            "list=bogus",
            "list=no",
            "range=control|10",
            "range=pane|12",
            "range=session|12",
            "range=user|",
            "dim=101",
            "width=-1",
            "pad=-1",
            "hyperlink",
            "nohyperlink",
            "bold|noitalics",
        ] {
            assert!(!valid_style(value), "{value}");
        }
    }

    #[test]
    fn display_popup_catalogue_and_dead_hook_match_the_pin() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .unwrap();
        let listing = engine
            .execute(&mut context, &command("list-commands", &["display-popup"]))
            .unwrap()
            .output;
        assert_eq!(
            listing,
            "display-popup (popup) [-BCEkN] [-b border-lines] [-c target-client] [-d start-directory] [-e environment] [-h height] [-s style] [-S border-style] [-t target-pane] [-T title] [-w width] [-x position] [-y position] [shell-command [argument ...]]"
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command(
                    "set-hook",
                    &["after-display-popup", "display-message nope"]
                )
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "invalid option: after-display-popup"
        ));
        assert_eq!(
            engine
                .execute(&mut context, &command("list-commands", &["display-menu"]))
                .unwrap()
                .output,
            "display-menu (menu) [-MO] [-b border-lines] [-c target-client] [-C starting-choice] [-H selected-style] [-s style] [-S border-style] [-t target-pane] [-T title] [-x position] [-y position] name [key] [command] ..."
        );
        assert!(matches!(
            engine.execute(
                &mut context,
                &command("set-hook", &["after-display-menu", "display-message nope"])
            ),
            Err(ServerError::InvalidCommand(message))
                if message == "invalid option: after-display-menu"
        ));
        assert_eq!(
            engine
                .execute(&mut context, &command("list-commands", &["confirm-before"]))
                .unwrap()
                .output,
            "confirm-before (confirm) [-by] [-c confirm-key] [-p prompt] [-t target-client] command"
        );
        for (name, expected) in [
            ("lock-server", "lock-server (lock) "),
            ("lock-session", "lock-session (locks) [-t target-session]"),
            ("lock-client", "lock-client (lockc) [-t target-client]"),
        ] {
            assert_eq!(
                engine
                    .execute(&mut context, &command("list-commands", &[name]))
                    .unwrap()
                    .output,
                expected
            );
        }
    }
}
