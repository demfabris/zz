use std::{collections::BTreeMap, path::PathBuf, str::FromStr as _};

use zz_protocol::{
    AgentDescriptor, AgentProvider, Axis, BrowserDescriptor, ChooseTreeKind, CommandInvocation,
    DEFAULT_AGENT_AUTO_APPROVE, DEFAULT_AGENT_CLAUDE_CODE_COMMAND, DEFAULT_AGENT_COMMAND,
    DEFAULT_BROWSER_PROFILE, EditorDescriptor, KeyToken, MAX_AGENT_COMMAND_BYTES,
    MAX_GUI_TEXT_BYTES, MuxOptionKey, PaneId, PaneKindSnapshot, ServerError, SessionId,
    TerminalUiCommand, WindowId, normalize_browser_profile_name,
};
use zz_terminal::{
    CopyJump, CopyJumpDirection, CopyModeAction, CopyModeCopy, DEFAULT_HISTORY_LIMIT,
    DEFAULT_WORD_SEPARATORS, MAX_HISTORY_LIMIT, PasteBufferAction, SearchDirection,
    TerminalViewAction,
};

use crate::{
    Binding, KeyTables, LayoutPreset, MuxState, PaneDirection, PaneKind, SplitPlacement,
    StatusFormats, StatusOption, canonical_command, command_spec,
    status::{FormatContext, expand_format},
};

const MAX_COPY_COMMAND_BYTES: usize = 8 * 1024;
const MAX_COMMAND_PROMPT_LABEL_BYTES: usize = 1024;
const MAX_COMMAND_PROMPT_TEMPLATE_BYTES: usize = 8 * 1024;
const DEFAULT_DISPLAY_PANES_DURATION_MS: u32 = 1_000;
const DEFAULT_DISPLAY_MESSAGE: &str =
    "[#{session_name}] #{window_index}:#{window_name}, current pane #{pane_index}";
pub const DEFAULT_BUFFER_LIMIT: usize = 50;
const MAX_BUFFER_LIMIT: usize = i32::MAX.cast_unsigned() as usize;
const DEFAULT_HISTORY_TRICKLE: usize = 2_000;
const MAX_HISTORY_TRICKLE: usize = 10_000;
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
        /// tmux-style shell command for terminal panes; `None` runs the
        /// default shell. Always `None` for other kinds.
        command: Option<String>,
    },
    PaneMaterialized {
        pane: PaneId,
        kind: PaneKindSnapshot,
        inherit_cwd_from: Option<PaneId>,
        command: Option<String>,
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

#[derive(Debug)]
pub struct MuxEngine {
    pub state: MuxState,
    pub keys: KeyTables,
    global_mode_keys: ModeKeys,
    window_mode_keys: BTreeMap<WindowId, ModeKeys>,
    set_clipboard: SetClipboard,
    copy_command: String,
    buffer_limit: usize,
    history_trickle: usize,
    global_history_limit: usize,
    session_history_limits: BTreeMap<SessionId, usize>,
    global_word_separators: String,
    session_word_separators: BTreeMap<SessionId, String>,
    status: StatusFormats,
    experimental_agent_pane: bool,
    experimental_editor_pane: bool,
    agent: AgentOptions,
    pane_cells: BTreeMap<PaneId, (u16, u16)>,
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
            history_trickle: DEFAULT_HISTORY_TRICKLE,
            global_history_limit: DEFAULT_HISTORY_LIMIT,
            session_history_limits: BTreeMap::new(),
            global_word_separators: DEFAULT_WORD_SEPARATORS.to_owned(),
            session_word_separators: BTreeMap::new(),
            status: StatusFormats::default(),
            experimental_agent_pane: false,
            experimental_editor_pane: false,
            agent: AgentOptions::default(),
            pane_cells: BTreeMap::new(),
        }
    }
}

impl MuxEngine {
    #[must_use]
    pub const fn buffer_limit(&self) -> usize {
        self.buffer_limit
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
    /// A validation failure leaves command-specific state untouched.
    pub fn execute(
        &mut self,
        context: &mut ExecutionContext,
        command: &CommandInvocation,
    ) -> Result<Execution, ServerError> {
        let generation = self.state.generation();
        let name = canonical_command(&command.name);
        let mut execution = match name {
            "new-session" => self.new_session(context, &command.args)?,
            "list-sessions" => self.list_sessions(&command.args)?,
            "rename-session" => self.rename_session(context, &command.args)?,
            "kill-session" => self.kill_session(context, &command.args)?,
            "attach-session" => self.attach_session(context, &command.args)?,
            "has-session" => self.has_session(context, &command.args)?,
            "detach-client" => self.detach_client(context, &command.args)?,
            "new-window" => self.new_window(context, &command.args, PaneKind::Terminal)?,
            "new-browser" => {
                let (options, positional) = parse_command_options("new-browser", &command.args)?;
                let browser = browser_from_args(&options, &positional)?;
                self.new_window_with_options(context, &options, PaneKind::Browser(browser), None)?
            }
            "list-windows" => self.list_windows(context, &command.args)?,
            "rename-window" => self.rename_window(context, &command.args)?,
            "select-window" => self.select_window(context, &command.args)?,
            "next-window" => self.step_window(context, &command.args, 1)?,
            "previous-window" => self.step_window(context, &command.args, -1)?,
            "last-window" => self.last_window(context, &command.args)?,
            "kill-window" => self.kill_window(context, &command.args)?,
            "new-pane" => self.new_pane(context, &command.args)?,
            "split-window" => self.split_window(context, &command.args, None)?,
            "split-browser" => self.split_browser(context, &command.args)?,
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
            "list-panes" => self.list_panes(context, &command.args)?,
            "resize-pane" => self.resize_pane(context, &command.args)?,
            "select-layout" | "next-layout" | "previous-layout" => {
                self.select_layout(context, &command.args, name)?
            }
            "rotate-window" => self.rotate_window(context, &command.args)?,
            "kill-pane" => self.kill_pane(context, &command.args)?,
            "send-keys" => self.send_keys(context, &command.args)?,
            "send-prefix" => self.send_prefix(context, &command.args)?,
            "copy-mode" => self.copy_mode(context, &command.args)?,
            "copy-mode-search-prompt" => self.copy_mode_search_prompt(context, &command.args)?,
            "command-prompt" => self.command_prompt(context, &command.args)?,
            "focus-sidebar" => self.focus_sidebar(context, &command.args)?,
            "choose-tree" => self.choose_tree(context, &command.args)?,
            "choose-buffer" => self.choose_buffer(context, &command.args)?,
            "display-message" => self.display_message(context, &command.args)?,
            "display-panes" => self.display_panes(context, &command.args)?,
            "clear-history" => self.clear_history(context, &command.args)?,
            "bind-key" => self.bind_key(&command.args)?,
            "unbind-key" => self.unbind_key(&command.args)?,
            "list-keys" => self.list_keys(&command.args)?,
            "set-option" => self.set_option(context, &command.args, false)?,
            "set-window-option" => self.set_option(context, &command.args, true)?,
            "source-file" => Self::source_file(&command.args)?,
            "reload-config" => {
                let _ = parse_command_options("reload-config", &command.args)?;
                if command.args.is_empty() {
                    Execution::effect(MuxEffect::ReloadConfig)
                } else {
                    return Err(ServerError::InvalidCommand(
                        "reload-config does not take arguments".to_owned(),
                    ));
                }
            }
            "kill-server" => {
                let _ = parse_command_options("kill-server", &command.args)?;
                Execution::effect(MuxEffect::KillServer)
            }
            _ => return Err(ServerError::UnsupportedCommand(command.name.clone())),
        };

        if self.state.generation() != generation {
            execution.effects.push(MuxEffect::SnapshotChanged);
        }
        self.session_history_limits
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.session_word_separators
            .retain(|session, _| self.state.sessions.contains_key(session));
        self.window_mode_keys
            .retain(|window, _| self.state.windows.contains_key(window));
        let state = &self.state;
        self.pane_cells
            .retain(|pane, _| state.window_for_pane(*pane).is_some());
        self.repair_context(context);
        Ok(execution)
    }

    fn new_session(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
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
        let inherit_cwd_from = spawn_cwd_source(
            "new-session",
            &self.state,
            &options,
            context.pane,
            &PaneKind::Terminal,
        )?;
        let name = options
            .value("-s")
            .map_or_else(|| next_session_name(&self.state), str::to_owned);
        let (session, window, pane) = self.state.create_session(name)?;
        if let Some(window_name) = options.value("-n") {
            window_name.clone_into(
                &mut self
                    .state
                    .windows
                    .get_mut(&window)
                    .expect("new window exists")
                    .name,
            );
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

    fn list_sessions(&self, args: &[String]) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-sessions", args)?;
        reject_positionals("list-sessions", &positional)?;
        if let Some(format) = options.value("-F") {
            let output = self
                .state
                .sessions
                .values()
                .map(|session| {
                    expand_format(
                        format,
                        &self.state,
                        FormatContext {
                            session: Some(session.id),
                            window: None,
                            pane: None,
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(Execution::output(output));
        }
        let output = self
            .state
            .sessions
            .values()
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
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("new-window", args)?;
        let command = shell_command_positional(&positional);
        self.new_window_with_options(context, &options, kind, command)
    }

    fn new_window_with_options(
        &mut self,
        context: &mut ExecutionContext,
        options: &Options,
        kind: PaneKind,
        command: Option<String>,
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
        let inherit_cwd_from =
            spawn_cwd_source("new-window", &self.state, options, context.pane, &kind)?;
        let index = if options.has("-a") {
            let after = match destination.index {
                Some(index) => index,
                None => window_index(&self.state, session_active_window(&self.state, session)?)?,
            };
            let index = after
                .checked_add(1)
                .ok_or_else(|| ServerError::InvalidCommand("no free window index".to_owned()))?;
            self.state.shift_windows_up(session, index)?;
            Some(index)
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
        let (window, pane) = self.state.create_window_at(
            session,
            index.filter(|_| replaced.is_none()),
            window_name,
            kind,
            selects,
        )?;
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
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-windows", args)?;
        reject_positionals("list-windows", &positional)?;
        let session_id = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        let session = self
            .state
            .sessions
            .get(&session_id)
            .ok_or_else(|| ServerError::MissingTarget(session_id.to_string()))?;
        if let Some(format) = options.value("-F") {
            let output = session
                .windows
                .iter()
                .filter_map(|window| self.state.windows.get(window))
                .map(|window| {
                    expand_format(
                        format,
                        &self.state,
                        FormatContext {
                            session: Some(session_id),
                            window: Some(window.id),
                            pane: None,
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(Execution::output(output));
        }
        let output = session
            .windows
            .iter()
            .filter_map(|id| self.state.windows.get(id))
            .map(|window| {
                let active = if window.id == session.active_window {
                    '*'
                } else {
                    '-'
                };
                format!(
                    "{}: {}{} ({} panes) [id {}]",
                    window.index,
                    window.name,
                    active,
                    window.panes.len(),
                    window.id
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Execution::output(output))
    }

    fn rename_window(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("rename-window", args)?;
        let name = exactly_one_argument("rename-window", &positional)?;
        let window =
            self.state
                .resolve_window(options.value("-t"), context.session, context.window)?;
        self.state.rename_window(window, name)?;
        Ok(Execution::default())
    }

    fn select_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("select-window", args)?;
        let target = options
            .value("-t")
            .or_else(|| positional.first().map(String::as_str));
        if options.has("-n") || options.has("-p") {
            let session = self.session_of_window_target(target, context)?;
            let direction = if options.has("-n") { 1 } else { -1 };
            return self.step_window_in_session(context, session, direction, false);
        }
        if options.has("-l") {
            let session = self.session_of_window_target(target, context)?;
            return self.activate_last_window(context, session);
        }
        let window = self
            .state
            .resolve_window(target, context.session, context.window)?;
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
        let window = self
            .state
            .resolve_window(Some(target), context.session, context.window)?;
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

    fn kill_window(
        &mut self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("kill-window", args)?;
        reject_positionals("kill-window", &positional)?;
        let window =
            self.state
                .resolve_window(options.value("-t"), context.session, context.window)?;
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
        let mut panes = Vec::new();
        for target in targets {
            panes.extend(self.state.kill_window(target)?);
        }
        Ok(Execution::effect(MuxEffect::PanesRemoved(panes)))
    }

    fn split_window(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
        kind: Option<PaneKind>,
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("split-window", args)?;
        let command = shell_command_positional(&positional);
        self.split_window_with_options(
            context,
            &options,
            kind.unwrap_or(PaneKind::Terminal),
            command,
            split_size(&options),
        )
    }

    fn new_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("new-pane", args)?;
        if !positional.is_empty() {
            return Err(ServerError::InvalidCommand(
                "new-pane does not accept positional arguments".to_owned(),
            ));
        }
        let target = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
        let inherit_cwd_from = spawn_cwd_source(
            "new-pane",
            &self.state,
            &options,
            Some(target),
            &PaneKind::Terminal,
        )?;
        self.split_window_with_options(
            context,
            &options,
            PaneKind::Picker { inherit_cwd_from },
            None,
            split_size(&options),
        )
    }

    fn split_browser(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("split-browser", args)?;
        let browser = browser_from_args(&options, &positional)?;
        self.split_window_with_options(context, &options, PaneKind::Browser(browser), None, None)
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let source = self
            .state
            .resolve_pane(options.value("-s"), context.window, context.pane)?;
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
        let window = self.state.break_pane(
            source,
            destination_session,
            destination.index,
            options.value("-n").map(str::to_owned),
            detached,
        )?;
        if detached {
            if original_context.window == Some(source_window)
                && original_context.pane == Some(source)
            {
                let pane = self.state.windows[&source_window].active_pane;
                *context = ExecutionContext::for_pane(&self.state, pane)
                    .expect("source window retains an active pane");
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
        let source = self
            .state
            .resolve_pane(options.value("-s"), context.window, context.pane)?;
        let target = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        self.state.join_pane(
            source,
            target,
            if options.has("-h") {
                Axis::Horizontal
            } else {
                Axis::Vertical
            },
            options
                .value("-p")
                .map(parse_pane_percentage)
                .transpose()?
                .unwrap_or(0.5),
            options.has("-b"),
            options.has("-f"),
            detached,
        )?;

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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
    ) -> Result<Execution, ServerError> {
        let target = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
        let axis = if options.has("-h") {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        let placement = self.split_placement(options, size, target, axis)?;
        let snapshot_kind = pane_kind_snapshot(&kind);
        let inherit_cwd_from =
            spawn_cwd_source("split-window", &self.state, options, Some(target), &kind)?;
        let pane = self.state.split_pane_with(target, axis, kind, placement)?;
        if !placement.detached {
            *context =
                ExecutionContext::for_pane(&self.state, pane).expect("new pane has a context");
        }
        Ok(Execution::effect(MuxEffect::PaneCreated {
            pane,
            kind: snapshot_kind,
            inherit_cwd_from,
            command,
        }))
    }

    fn split_placement(
        &self,
        options: &Options,
        size: Option<SplitSize<'_>>,
        target: PaneId,
        axis: Axis,
    ) -> Result<SplitPlacement, ServerError> {
        let full_size = options.has("-f");
        let ratio = match size {
            None => SplitPlacement::default().ratio,
            Some(SplitSize::Percentage(value)) => parse_pane_percentage(value)?,
            Some(SplitSize::Cells(value)) => {
                if let Some(percentage) = value.strip_suffix('%') {
                    parse_pane_percentage(percentage)?
                } else {
                    let cells = value.parse::<f32>().map_err(|_| {
                        ServerError::InvalidCommand(format!("invalid pane size: {value}"))
                    })?;
                    cells / self.split_cell_extent(target, axis, full_size)?
                }
            }
        };
        Ok(SplitPlacement {
            ratio,
            before: options.has("-b"),
            full_size,
            detached: options.has("-d"),
        })
    }

    /// Cells along `axis` in the box a new pane divides: the whole window under
    /// `-f`, otherwise the pane being split.
    fn split_cell_extent(
        &self,
        target: PaneId,
        axis: Axis,
        full_size: bool,
    ) -> Result<f32, ServerError> {
        let extent = self
            .window_cell_extent(target, axis)
            .ok_or_else(|| geometry_unavailable("a split size"))?;
        if full_size {
            return Ok(extent);
        }
        let window = self
            .state
            .window_for_pane(target)
            .ok_or_else(|| ServerError::MissingTarget(target.to_string()))?;
        let layout = &self.state.windows[&window].layout;
        crate::model::pane_axis_fraction(layout, target, axis)
            .filter(|fraction| *fraction > 0.0)
            .map(|fraction| extent * fraction)
            .ok_or_else(|| geometry_unavailable("a split size"))
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
                let current = self
                    .state
                    .resolve_pane(None, context.window, context.pane)?;
                self.state.next_pane(current)?
            }
            Some(":.-" | ".-" | ":-") => {
                let current = self
                    .state
                    .resolve_pane(None, context.window, context.pane)?;
                self.state.previous_pane(current)?
            }
            _ => self
                .state
                .resolve_pane(target, context.window, context.pane)?,
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
            self.state.select_pane_with_zoom(pane, options.has("-Z"))?;
            *context = ExecutionContext::for_pane(&self.state, pane).expect("selected pane exists");
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
        self.state.select_pane_with_zoom(pane, options.has("-Z"))?;
        *context = ExecutionContext::for_pane(&self.state, pane).expect("selected pane exists");
        Ok(Execution::default())
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
        let window =
            self.state
                .resolve_window(options.value("-t"), context.session, context.window)?;
        let pane = self.state.last_pane(window)?;
        self.state.select_pane_with_zoom(pane, options.has("-Z"))?;
        *context = ExecutionContext::for_pane(&self.state, pane).expect("selected pane exists");
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
        let target = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
            self.state
                .resolve_pane(Some(source), context.window, context.pane)?
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
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("list-panes", args)?;
        reject_positionals("list-panes", &positional)?;
        let window_id =
            self.state
                .resolve_window(options.value("-t"), context.session, context.window)?;
        let window = self
            .state
            .windows
            .get(&window_id)
            .ok_or_else(|| ServerError::MissingTarget(window_id.to_string()))?;
        if let Some(format) = options.value("-F") {
            let output = window
                .pane_order()
                .iter()
                .filter_map(|pane| window.panes.get(pane))
                .map(|pane| {
                    expand_format(
                        format,
                        &self.state,
                        FormatContext {
                            session: Some(window.session),
                            window: Some(window_id),
                            pane: Some(pane.id),
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(Execution::output(output));
        }
        let output = window
            .pane_order()
            .iter()
            .filter_map(|pane| window.panes.get(pane))
            .map(|pane| {
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
                format!("{}: {kind}{active} {}", pane.id, pane.title)
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Execution::output(output))
    }

    fn resize_pane(
        &mut self,
        context: &mut ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("resize-pane", args)?;
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        if positional.len() > 1 {
            return Err(ServerError::InvalidCommand(
                "resize-pane accepts at most one adjustment".to_owned(),
            ));
        }
        for (option, axis) in [("-x", Axis::Horizontal), ("-y", Axis::Vertical)] {
            let Some(value) = options.value(option) else {
                continue;
            };
            let fraction = if let Some(percentage) = value.strip_suffix('%') {
                parse_pane_percentage(percentage)?
            } else {
                let cells = value.parse::<f32>().map_err(|_| {
                    ServerError::InvalidCommand(format!("invalid pane size: {value}"))
                })?;
                cells
                    / self
                        .window_cell_extent(pane, axis)
                        .ok_or_else(|| geometry_unavailable(&format!("resize-pane {option}")))?
            };
            self.state.resize_pane_to(pane, axis, fraction)?;
        }
        let shared = positional
            .first()
            .map(|value| parse_resize_adjustment(value))
            .transpose()?;
        for (option, axis, sign) in [
            ("-L", Axis::Horizontal, -1.0),
            ("-R", Axis::Horizontal, 1.0),
            ("-U", Axis::Vertical, -1.0),
            ("-D", Axis::Vertical, 1.0),
        ] {
            let attached = options
                .value(option)
                .map(parse_resize_adjustment)
                .transpose()?;
            if attached.is_none() && !options.has(option) {
                continue;
            }
            let cells = attached.or(shared).unwrap_or(1);
            let extent = self.window_cell_extent(pane, axis);
            self.state
                .resize_pane(pane, axis, sign * cells as f32, extent)?;
        }
        Ok(Execution::default())
    }

    pub fn set_pane_geometry(&mut self, pane: PaneId, columns: u16, rows: u16) {
        self.pane_cells.insert(pane, (columns, rows));
    }

    fn window_cell_extent(&self, pane: PaneId, axis: Axis) -> Option<f32> {
        let window = self.state.window_for_pane(pane)?;
        let window = self.state.windows.get(&window)?;
        window.pane_order().iter().find_map(|candidate| {
            let (columns, rows) = *self.pane_cells.get(candidate)?;
            let cells = f32::from(match axis {
                Axis::Horizontal => columns,
                Axis::Vertical => rows,
            });
            let fraction = crate::model::pane_axis_fraction(&window.layout, *candidate, axis)?;
            (cells >= 1.0 && fraction > 0.0).then(|| cells / fraction)
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
        let (window, pane) = if target.is_some_and(|target| target.starts_with('%')) {
            let pane = self
                .state
                .resolve_pane(target, context.window, context.pane)?;
            let window = self
                .state
                .window_for_pane(pane)
                .expect("resolved pane has a window");
            (window, pane)
        } else {
            let window = self
                .state
                .resolve_window(target, context.session, context.window)?;
            (window, window_active_pane(&self.state, window)?)
        };

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
            self.state
                .select_layout(window, parse_layout_preset(name)?)?;
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
        let window =
            self.state
                .resolve_window(options.value("-t"), context.session, context.window)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
        let targets = if options.has("-a") {
            let window = self
                .state
                .window_for_pane(pane)
                .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
            self.state.windows[&window]
                .pane_order()
                .iter()
                .copied()
                .filter(|candidate| *candidate != pane)
                .collect()
        } else {
            vec![pane]
        };
        let mut panes = Vec::new();
        for target in targets {
            panes.extend(self.state.kill_pane(target)?);
        }
        Ok(Execution::effect(MuxEffect::PanesRemoved(panes)))
    }

    fn send_keys(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("send-keys", args)?;
        let repeat = repeat_count("send-keys", &options)?;
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
        let mut effects = vec![MuxEffect::TerminalView {
            pane,
            action: TerminalViewAction::EnterCopyMode,
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
                action: TerminalViewAction::CopyMode(CopyModeAction::PageDown),
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
            let window = self
                .state
                .resolve_window(None, context.session, context.window)?;
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
        if options.has("-H") {
            return Err(ServerError::UnsupportedCommand(
                "clear-history -H".to_owned(),
            ));
        }
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
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
        let pane = self
            .state
            .resolve_pane(options.value("-t"), context.window, context.pane)?;
        Ok(Execution::effect(MuxEffect::ChooseBuffer { pane }))
    }

    fn display_message(
        &self,
        context: &ExecutionContext,
        args: &[String],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("display-message", args)?;
        let pane = match options.value("-t") {
            Some(target) => Some(self.state.resolve_pane(
                Some(target),
                context.window,
                context.pane,
            )?),
            None if self.state.sessions.is_empty() => None,
            None => Some(
                self.state
                    .resolve_pane(None, context.window, context.pane)?,
            ),
        };
        let format_context = pane
            .and_then(|pane| ExecutionContext::for_pane(&self.state, pane))
            .map_or_else(FormatContext::default, |context| FormatContext {
                session: context.session,
                window: context.window,
                pane: context.pane,
            });
        let format = if positional.is_empty() {
            DEFAULT_DISPLAY_MESSAGE.to_owned()
        } else {
            positional.join(" ")
        };
        let text = expand_format(&format, &self.state, format_context);
        if options.has("-p") {
            Ok(Execution::output(text))
        } else {
            Ok(Execution::effect(MuxEffect::DisplayMessage { pane, text }))
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
        let duration_ms =
            options
                .value("-d")
                .map_or(Ok(DEFAULT_DISPLAY_PANES_DURATION_MS), |value| {
                    value.parse::<u32>().map_err(|_| {
                        ServerError::InvalidCommand(format!(
                            "display-panes duration must be an unsigned millisecond value: {value}"
                        ))
                    })
                })?;
        let pane = self
            .state
            .resolve_pane(None, context.window, context.pane)?;
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
        if options.has("-a") {
            return Err(ServerError::UnsupportedCommand("unbind-key -a".to_owned()));
        }
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
        if option == "synchronize-panes" {
            return self.set_synchronize_panes(
                context,
                positional.get(1).map(String::as_str),
                &options,
                force_window,
            );
        }
        if option == "buffer-limit" {
            return self.set_buffer_limit(
                positional.get(1).map(String::as_str),
                &options,
                force_window,
            );
        }
        if option == "history-trickle" {
            return self.set_history_trickle(
                positional.get(1).map(String::as_str),
                &options,
                force_window,
            );
        }
        if option == "history-limit" {
            return self.set_history_limit(
                context,
                positional.get(1).map(String::as_str),
                &options,
                force_window,
            );
        }
        if option == "word-separators" {
            return self.set_word_separators(
                context,
                positional.get(1).map(String::as_str),
                &options,
                force_window,
            );
        }
        if option == "mode-keys" {
            return self.set_mode_keys(
                context,
                positional.get(1).map(String::as_str),
                &options,
                force_window,
            );
        }
        if let Some(status) = StatusOption::from_name(option) {
            return self.set_status_option(status, positional.get(1).map(String::as_str), &options);
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
            return already_set_or_quiet(&options, option);
        }
        let value = positional.get(1).ok_or_else(|| {
            ServerError::InvalidCommand(format!("set-option {option} needs a value"))
        })?;
        let changed = match option.as_str() {
            "prefix" => {
                self.keys.set_prefix(value.as_str());
                MuxOptionKey::Prefix
            }
            "set-clipboard" => {
                self.set_clipboard = match value.as_str() {
                    "on" => SetClipboard::On,
                    "external" => SetClipboard::External,
                    "off" => SetClipboard::Off,
                    value => {
                        return Err(ServerError::InvalidCommand(format!(
                            "invalid set-clipboard value: {value}"
                        )));
                    }
                };
                MuxOptionKey::SetClipboard
            }
            "copy-command" => {
                if value.len() > MAX_COPY_COMMAND_BYTES {
                    return Err(ServerError::InvalidCommand(format!(
                        "copy-command exceeds {MAX_COPY_COMMAND_BYTES} bytes"
                    )));
                }
                self.copy_command.clone_from(value);
                MuxOptionKey::CopyCommand
            }
            "experimental-agent-pane" => {
                self.experimental_agent_pane =
                    parse_flag_value(Some(value.as_str()), self.experimental_agent_pane)?;
                MuxOptionKey::ExperimentalAgentPane
            }
            "experimental-editor-pane" => {
                self.experimental_editor_pane =
                    parse_flag_value(Some(value.as_str()), self.experimental_editor_pane)?;
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
                    self.agent.command.clone_from(value);
                    MuxOptionKey::AgentCommand
                } else {
                    self.agent.claude_code_command.clone_from(value);
                    MuxOptionKey::AgentClaudeCodeCommand
                }
            }
            "agent-auto-approve" => {
                self.agent.auto_approve =
                    parse_flag_value(Some(value.as_str()), self.agent.auto_approve)?;
                MuxOptionKey::AgentAutoApprove
            }
            _ => {
                return Err(ServerError::UnsupportedCommand(format!(
                    "set-option {option}"
                )));
            }
        };
        Ok(Execution::effect(MuxEffect::MuxOptionChanged {
            option: changed,
        }))
    }

    fn set_buffer_limit(
        &mut self,
        value: Option<&str>,
        options: &Options,
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        if force_window {
            return Err(ServerError::InvalidCommand(
                "buffer-limit is a global server option".to_owned(),
            ));
        }
        if let Some(flag) = options
            .flags
            .iter()
            .find(|flag| !matches!(flag.as_str(), "-g" | "-q" | "-u"))
        {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {flag} buffer-limit"
            )));
        }
        let limit = if options.has("-u") {
            if value.is_some() {
                return Err(ServerError::InvalidCommand(
                    "unsetting buffer-limit does not accept a value".to_owned(),
                ));
            }
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
        context: &ExecutionContext,
        value: Option<&str>,
        options: &Options,
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        if force_window {
            return Err(ServerError::InvalidCommand(
                "history-limit is a session option".to_owned(),
            ));
        }
        if let Some(flag) = options
            .flags
            .iter()
            .find(|flag| !matches!(flag.as_str(), "-g" | "-q" | "-u"))
        {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {flag} history-limit"
            )));
        }
        if options.has("-g") && options.value("-t").is_some() {
            return Err(ServerError::InvalidCommand(
                "global history-limit does not accept a target".to_owned(),
            ));
        }
        if options.has("-u") && value.is_some() {
            return Err(ServerError::InvalidCommand(
                "unsetting history-limit does not accept a value".to_owned(),
            ));
        }

        let limit = if options.has("-u") {
            DEFAULT_HISTORY_LIMIT
        } else {
            parse_history_limit(value.ok_or_else(|| {
                ServerError::InvalidCommand("set-option history-limit needs a value".to_owned())
            })?)?
        };
        if options.has("-g") {
            self.global_history_limit = limit;
            return Ok(Execution::effect(MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::HistoryLimit,
            }));
        }
        let session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        if options.has("-u") {
            self.session_history_limits.remove(&session);
        } else {
            self.session_history_limits.insert(session, limit);
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
        if let Some(flag) = options
            .flags
            .iter()
            .find(|flag| !matches!(flag.as_str(), "-a" | "-g" | "-q" | "-u"))
        {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {flag} {}",
                option.as_str()
            )));
        }
        if options.has("-u") && (value.is_some() || options.has("-a")) {
            return Err(ServerError::InvalidCommand(format!(
                "unsetting {} does not accept a value or -a",
                option.as_str()
            )));
        }
        let appended = options
            .has("-a")
            .then(|| self.status.format(option))
            .flatten()
            .zip(value)
            .map(|(current, value)| format!("{current}{value}"));
        let value = match (&appended, options.has("-u")) {
            (Some(appended), _) => Some(appended.as_str()),
            (None, true) => None,
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
        context: &ExecutionContext,
        value: Option<&str>,
        options: &Options,
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        if force_window {
            return Err(ServerError::InvalidCommand(
                "word-separators is a session option".to_owned(),
            ));
        }
        if let Some(flag) = options
            .flags
            .iter()
            .find(|flag| !matches!(flag.as_str(), "-a" | "-g" | "-q" | "-u"))
        {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {flag} word-separators"
            )));
        }
        if options.has("-g") && options.value("-t").is_some() {
            return Err(ServerError::InvalidCommand(
                "global word-separators does not accept a target".to_owned(),
            ));
        }
        if options.has("-u") && (value.is_some() || options.has("-a")) {
            return Err(ServerError::InvalidCommand(
                "unsetting word-separators does not accept a value or -a".to_owned(),
            ));
        }

        if options.has("-g") {
            let next = if options.has("-u") {
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

        let session = self
            .state
            .resolve_session(options.value("-t"), context.session)?;
        if options.has("-u") {
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
        context: &ExecutionContext,
        value: Option<&str>,
        options: &Options,
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        if let Some(flag) = options
            .flags
            .iter()
            .find(|flag| !matches!(flag.as_str(), "-g" | "-o" | "-q" | "-u" | "-w"))
        {
            return Err(ServerError::UnsupportedCommand(format!(
                "set-option {flag} mode-keys"
            )));
        }
        if force_window && options.has("-w") {
            return Err(ServerError::UnsupportedCommand(
                "set-window-option -w mode-keys".to_owned(),
            ));
        }
        if options.has("-o") && options.has("-u") {
            return Err(ServerError::InvalidCommand(
                "mode-keys cannot combine set-once and unset".to_owned(),
            ));
        }
        if options.has("-u") && value.is_some() {
            return Err(ServerError::InvalidCommand(
                "unsetting mode-keys does not accept a value".to_owned(),
            ));
        }

        if options.has("-g") {
            if options.has("-o") {
                return already_set_or_quiet(options, "mode-keys");
            }
            let next = if options.has("-u") {
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

        let window =
            self.state
                .resolve_window(options.value("-t"), context.session, context.window)?;
        if options.has("-o") && self.window_mode_keys.contains_key(&window) {
            return already_set_or_quiet(options, "mode-keys");
        }
        if options.has("-u") {
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
        context: &ExecutionContext,
        value: Option<&str>,
        options: &Options,
        force_window: bool,
    ) -> Result<Execution, ServerError> {
        for flag in &options.flags {
            if !matches!(
                flag.as_str(),
                "-g" | "-w" | "-p" | "-u" | "-U" | "-o" | "-q"
            ) {
                return Err(ServerError::UnsupportedCommand(format!(
                    "set-option {flag} synchronize-panes"
                )));
            }
        }
        let scope_flags = usize::from(options.has("-g"))
            + usize::from(options.has("-w"))
            + usize::from(options.has("-p"));
        if scope_flags > 1 || force_window && options.has("-p") {
            return Err(ServerError::InvalidCommand(
                "synchronize-panes has conflicting scopes".to_owned(),
            ));
        }
        if force_window && (options.has("-w") || options.has("-U")) {
            return Err(ServerError::UnsupportedCommand(
                "set-window-option synchronize-panes scope flag".to_owned(),
            ));
        }
        if (options.has("-u") || options.has("-U")) && value.is_some() {
            return Err(ServerError::InvalidCommand(
                "unsetting synchronize-panes does not accept a value".to_owned(),
            ));
        }
        if options.has("-u") && options.has("-U") {
            return Err(ServerError::InvalidCommand(
                "synchronize-panes cannot combine -u and -U".to_owned(),
            ));
        }
        if options.has("-o") && (options.has("-u") || options.has("-U")) {
            return Err(ServerError::InvalidCommand(
                "synchronize-panes cannot combine set-once and unset".to_owned(),
            ));
        }
        if options.has("-U") && options.has("-p") {
            return Err(ServerError::InvalidCommand(
                "synchronize-panes -U requires window scope".to_owned(),
            ));
        }

        if options.has("-g") {
            if options.value("-t").is_some() || options.has("-U") {
                return Err(ServerError::InvalidCommand(
                    "global synchronize-panes does not accept -t or -U".to_owned(),
                ));
            }
            if options.has("-o") {
                return already_set_or_quiet(options, "synchronize-panes");
            }
            let next = if options.has("-u") {
                false
            } else {
                parse_flag_value(value, self.state.global_synchronize_panes())?
            };
            self.state.set_global_synchronize_panes(next);
            return Ok(Execution::effect(MuxEffect::MuxOptionChanged {
                option: MuxOptionKey::SynchronizePanes,
            }));
        }

        if options.has("-p") {
            let pane =
                self.state
                    .resolve_pane(options.value("-t"), context.window, context.pane)?;
            if options.has("-o") && self.state.pane_synchronize_override(pane)?.is_some() {
                return already_set_or_quiet(options, "synchronize-panes");
            }
            let next = if options.has("-u") {
                None
            } else {
                Some(parse_flag_value(
                    value,
                    self.state.pane_synchronize_panes(pane)?,
                )?)
            };
            self.state.set_pane_synchronize_panes(pane, next)?;
            return Ok(Execution::default());
        }

        let window =
            self.state
                .resolve_window(options.value("-t"), context.session, context.window)?;
        if options.has("-o") && self.state.window_synchronize_override(window)?.is_some() {
            return already_set_or_quiet(options, "synchronize-panes");
        }
        if options.has("-U") {
            self.state.clear_pane_synchronize_overrides(window)?;
            self.state.set_window_synchronize_panes(window, None)?;
        } else {
            let next = if options.has("-u") {
                None
            } else {
                Some(parse_flag_value(
                    value,
                    self.state.window_synchronize_panes(window)?,
                )?)
            };
            self.state.set_window_synchronize_panes(window, next)?;
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

fn parse_flag_value(value: Option<&str>, current: bool) -> Result<bool, ServerError> {
    // tmux: `1` is exact, `on`/`yes` (and their negatives) compare with
    // strcasecmp, and a missing or empty value toggles. `true`/`false` is the
    // zz/config spelling, forwarded verbatim as a value.
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

fn geometry_unavailable(what: &str) -> ServerError {
    ServerError::InvalidCommand(format!(
        "{what} in cells needs pane geometry, which arrives once a client draws the window; use a percentage instead"
    ))
}

fn parse_pane_percentage(value: &str) -> Result<f32, ServerError> {
    let percentage = value
        .strip_suffix('%')
        .unwrap_or(value)
        .parse::<f32>()
        .map_err(|_| ServerError::InvalidCommand(format!("invalid pane percentage: {value}")))?;
    let ratio = percentage / 100.0;
    if !ratio.is_finite() || !(0.1..=0.9).contains(&ratio) {
        return Err(ServerError::InvalidCommand(
            "pane percentage must be between 10 and 90".to_owned(),
        ));
    }
    Ok(ratio)
}

fn parse_layout_preset(value: &str) -> Result<LayoutPreset, ServerError> {
    if let Some(exact) = LayoutPreset::ALL
        .into_iter()
        .find(|preset| preset.name() == value)
    {
        return Ok(exact);
    }
    let mut matches = LayoutPreset::ALL
        .into_iter()
        .filter(|preset| preset.name().starts_with(value));
    let Some(first) = matches.next() else {
        if value.contains([',', '{', '}']) {
            return Err(ServerError::UnsupportedCommand(
                "select-layout serialized layout".to_owned(),
            ));
        }
        return Err(ServerError::InvalidCommand(format!(
            "unknown layout: {value}"
        )));
    };
    if matches.next().is_some() {
        return Err(ServerError::InvalidCommand(format!(
            "ambiguous layout: {value}"
        )));
    }
    Ok(first)
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
    let (options, positional) = parse_options(args, &value_options, &attached_options)?;
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
    Ok((options, positional))
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
        .map_err(|_| ServerError::InvalidCommand(format!("invalid resize adjustment: {value}")))
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
    let session_destination = |session| {
        Ok(WindowDestination {
            session,
            index: None,
        })
    };
    let Some(target) = target else {
        return session_destination(state.resolve_session(None, context.session)?);
    };
    if target.starts_with('$') {
        return session_destination(state.resolve_session(Some(target), context.session)?);
    }
    if target.starts_with('@') {
        let window = state.resolve_window(Some(target), context.session, context.window)?;
        return session_destination(
            state
                .windows
                .get(&window)
                .ok_or_else(|| ServerError::MissingTarget(target.to_owned()))?
                .session,
        );
    }
    if let Some((session_target, window_target)) = target.split_once(':') {
        let session = match session_target {
            "" => state.resolve_session(None, context.session)?,
            session => state.resolve_session(Some(session), context.session)?,
        };
        if window_target.is_empty() {
            return session_destination(session);
        }
        let index = window_target
            .parse::<u32>()
            .map_err(|_| ServerError::MissingTarget(target.to_owned()))?;
        return Ok(WindowDestination {
            session,
            index: Some(index),
        });
    }
    if let Ok(index) = target.parse::<u32>() {
        return Ok(WindowDestination {
            session: state.resolve_session(None, context.session)?,
            index: Some(index),
        });
    }
    session_destination(state.resolve_session(Some(target), context.session)?)
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
    command: &str,
    state: &MuxState,
    options: &Options,
    origin: Option<PaneId>,
    kind: &PaneKind,
) -> Result<Option<PaneId>, ServerError> {
    if !matches!(kind, PaneKind::Terminal) {
        return Ok(None);
    }
    match options.value("-c") {
        None | Some("#{pane_current_path}") => {
            Ok(origin.and_then(|origin| state.cwd_donor(origin)))
        }
        Some(_) => Err(ServerError::InvalidCommand(format!(
            "{command} -c currently supports only #{{pane_current_path}}"
        ))),
    }
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
    if let [argument] = tail
        && let Some(body) = crate::parser::command_block_body(argument)
    {
        let commands: Vec<CommandInvocation> = crate::parse_config("<bind-key>", body)
            .commands
            .into_iter()
            .map(|command| CommandInvocation::new(command.name, command.args))
            .collect();
        if commands.is_empty() {
            return Err(ServerError::InvalidCommand(
                "bind-key command block is empty".to_owned(),
            ));
        }
        return Ok(commands);
    }
    tail.split(|argument| argument == ";")
        .map(|segment| {
            let Some((command, command_args)) = segment.split_first() else {
                return Err(ServerError::InvalidCommand(
                    "bind-key command chain contains an empty command".to_owned(),
                ));
            };
            Ok(CommandInvocation::new(
                command,
                command_args.iter().cloned(),
            ))
        })
        .collect()
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
            Err(ServerError::MissingTarget(target)) if target == "missing"
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
            }]
        );
    }

    #[test]
    fn terminal_splits_inherit_the_target_pane_working_directory() {
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
                ..
            }) if *source == second
        ));

        assert!(matches!(
            engine.execute(&mut context, &command("split-window", &["-c", "/tmp"]),),
            Err(ServerError::InvalidCommand(_))
        ));
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
    fn new_windows_and_sessions_inherit_the_previous_pane_working_directory() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-s", "work"]))
            .expect("session");
        let first = context.pane.expect("first pane");

        let window = engine
            .execute(
                &mut context,
                &command("new-window", &["-c", "#{pane_current_path}"]),
            )
            .expect("new window");
        assert!(matches!(
            window.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                ..
            }) if *source == first
        ));
        let second = context.pane.expect("second pane");
        assert_ne!(second, first);

        let session = engine
            .execute(
                &mut context,
                &command("new-session", &["-s", "next", "-c", "#{pane_current_path}"]),
            )
            .expect("new session");
        assert!(matches!(
            session.effects.first(),
            Some(MuxEffect::PaneCreated {
                inherit_cwd_from: Some(source),
                ..
            }) if *source == second
        ));

        assert!(matches!(
            engine.execute(&mut context, &command("new-window", &["-c", "/tmp"])),
            Err(ServerError::InvalidCommand(_))
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
            .execute(&mut context, &command("new-pane", &["-h"]))
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
            .execute(&mut context, &command("new-pane", &["-v"]))
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
            .execute(&mut context, &command("new-pane", &["-h"]))
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
            .execute(&mut context, &command("new-pane", &["-h"]))
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
            .execute(&mut context, &command("new-pane", &["-h"]))
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
            .execute(&mut context, &command("new-pane", &["-h"]))
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
            &["x", "new-window", ";"][..],
            &["x", "new-window", ";", ";", "new-window"][..],
        ] {
            assert!(matches!(
                engine.execute(&mut context, &command("bind-key", args)),
                Err(ServerError::InvalidCommand(message)) if message.contains("empty command")
            ));
        }
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

        for command in [
            command("set-option", &["buffer-limit", "0"]),
            command("set-option", &["-p", "buffer-limit", "2"]),
            command("set-window-option", &["buffer-limit", "2"]),
        ] {
            assert!(engine.execute(&mut context, &command).is_err());
        }
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

        for command in [
            command(
                "set-option",
                &["-g", "history-limit", &(MAX_HISTORY_LIMIT + 1).to_string()],
            ),
            command("set-option", &["-w", "history-limit", "2"]),
            command("set-window-option", &["history-limit", "2"]),
        ] {
            assert!(engine.execute(&mut context, &command).is_err());
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
        for invalid in [
            command("set-option", &["-g", "word-separators", &oversized]),
            command("set-option", &["-w", "word-separators", "."]),
            command("set-window-option", &["word-separators", "."]),
        ] {
            assert!(engine.execute(&mut context, &invalid).is_err());
        }
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
                    &["-w", "-t", &first_window.to_string(), "mode-keys"],
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
                &command("set-option", &["-gw", "mode-keys", "vi"]),
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
            command("set-option", &["-p", "mode-keys", "vi"]),
            command("set-window-option", &["-w", "mode-keys", "vi"]),
            command("set-option", &["mode-keys", "unknown"]),
            command("set-option", &["-u", "mode-keys", "vi"]),
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
            Err(ServerError::InvalidCommand(message))
                if message == "command-prompt does not support -F"
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
            Err(ServerError::InvalidCommand(_))
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

        for args in [
            vec!["-N"],
            vec!["select-pane", "-t", "%%%"],
            vec!["-d", "forever"],
        ] {
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
            Err(ServerError::InvalidCommand(_))
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
        let window = context.window.unwrap();

        engine.set_pane_geometry(first, 100, 50);
        engine
            .execute(&mut context, &command("resize-pane", &["-R", "10"]))
            .unwrap();
        let zz_protocol::LayoutNode::Split { ratio, .. } = engine.state.windows[&window].layout
        else {
            panic!("split window has a split layout");
        };
        assert!((ratio - 0.55).abs() < 1e-4, "ratio {ratio}");
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
        let mut panes = Vec::new();
        engine.state.windows[&first_window].layout.panes(&mut panes);
        assert_eq!(panes, [left, right, middle]);
        assert_eq!(context.pane, Some(right));

        engine
            .execute(&mut context, &command("swap-pane", &["-d", "-D"]))
            .unwrap();
        panes.clear();
        engine.state.windows[&first_window].layout.panes(&mut panes);
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
            engine.state.windows[&broken_window].layout,
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
        let mut panes = Vec::new();
        engine.state.windows[&original_window]
            .layout
            .panes(&mut panes);
        assert_eq!(panes, [browser, terminal]);

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
            Err(ServerError::InvalidCommand(message))
                if message == "break-pane does not support -W"
        ));
    }

    #[test]
    fn layout_commands_cycle_target_restore_and_reject_non_native_layouts() {
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
            engine.state.windows[&window].layout,
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
            engine.state.windows[&window].layout,
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
            engine.state.windows[&window].layout,
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
            engine.state.windows[&window].layout,
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
            Err(ServerError::UnsupportedCommand(_))
        ));
        assert!(matches!(
            engine.execute(&mut context, &command("next-layout", &["-n"])),
            Err(ServerError::InvalidCommand(message))
                if message == "next-layout does not support -n"
        ));
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
            Err(ServerError::InvalidCommand(message))
                if message == "select-pane does not support -m"
        ));
    }
}
