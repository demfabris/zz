#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BellAction {
    None,
    Any,
    Current,
    Other,
}

impl BellAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Any => "any",
            Self::Current => "current",
            Self::Other => "other",
        }
    }

    #[must_use]
    pub const fn applies(self, current: bool) -> bool {
        match self {
            Self::None => false,
            Self::Any => true,
            Self::Current => current,
            Self::Other => !current,
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "any" => Some(Self::Any),
            "current" => Some(Self::Current),
            "other" => Some(Self::Other),
            _ => None,
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::None => Self::Any,
            Self::Any => Self::None,
            Self::Current | Self::Other => self,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualBell {
    Off,
    On,
    Both,
}

impl VisualBell {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Both => "both",
        }
    }

    #[must_use]
    pub const fn rings(self) -> bool {
        matches!(self, Self::Off | Self::Both)
    }

    #[must_use]
    pub const fn shows_message(self) -> bool {
        matches!(self, Self::On | Self::Both)
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "on" => Some(Self::On),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::Off => Self::On,
            Self::On => Self::Off,
            Self::Both => Self::Both,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowSize {
    Largest,
    Smallest,
    Manual,
    Latest,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AllowPassthrough {
    #[default]
    Off,
    On,
    All,
}

impl AllowPassthrough {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::All => "all",
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::Off => Self::On,
            Self::On => Self::Off,
            Self::All => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PaneCursorStyle {
    #[default]
    Default,
    BlinkingBlock,
    Block,
    BlinkingUnderline,
    Underline,
    BlinkingBar,
    Bar,
}

impl PaneCursorStyle {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::BlinkingBlock => "blinking-block",
            Self::Block => "block",
            Self::BlinkingUnderline => "blinking-underline",
            Self::Underline => "underline",
            Self::BlinkingBar => "blinking-bar",
            Self::Bar => "bar",
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::Default => Self::BlinkingBlock,
            Self::BlinkingBlock => Self::Default,
            Self::Block
            | Self::BlinkingUnderline
            | Self::Underline
            | Self::BlinkingBar
            | Self::Bar => self,
        }
    }
}

impl WindowSize {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Largest => "largest",
            Self::Smallest => "smallest",
            Self::Manual => "manual",
            Self::Latest => "latest",
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::Largest => Self::Smallest,
            Self::Smallest => Self::Largest,
            Self::Manual | Self::Latest => self,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetOptions {
    pub main_pane_width: String,
    pub main_pane_height: String,
    pub other_pane_width: String,
    pub other_pane_height: String,
    pub tiled_layout_max_columns: u16,
}

impl Default for PresetOptions {
    fn default() -> Self {
        Self {
            main_pane_width: "80".to_owned(),
            main_pane_height: "24".to_owned(),
            other_pane_width: "0".to_owned(),
            other_pane_height: "0".to_owned(),
            tiled_layout_max_columns: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ServerOption {
    Backspace,
    DefaultClientCommand,
    Editor,
    ExtendedKeys,
    ExtendedKeysFormat,
    FocusEvents,
    GetClipboard,
    HistoryFile,
    InputBufferSize,
    PrefixTimeout,
    PromptHistoryLimit,
    VariationSelectorAlwaysWide,
}

impl ServerOption {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "backspace" => Self::Backspace,
            "default-client-command" => Self::DefaultClientCommand,
            "editor" => Self::Editor,
            "extended-keys" => Self::ExtendedKeys,
            "extended-keys-format" => Self::ExtendedKeysFormat,
            "focus-events" => Self::FocusEvents,
            "get-clipboard" => Self::GetClipboard,
            "history-file" => Self::HistoryFile,
            "input-buffer-size" => Self::InputBufferSize,
            "prefix-timeout" => Self::PrefixTimeout,
            "prompt-history-limit" => Self::PromptHistoryLimit,
            "variation-selector-always-wide" => Self::VariationSelectorAlwaysWide,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Backspace => "backspace",
            Self::DefaultClientCommand => "default-client-command",
            Self::Editor => "editor",
            Self::ExtendedKeys => "extended-keys",
            Self::ExtendedKeysFormat => "extended-keys-format",
            Self::FocusEvents => "focus-events",
            Self::GetClipboard => "get-clipboard",
            Self::HistoryFile => "history-file",
            Self::InputBufferSize => "input-buffer-size",
            Self::PrefixTimeout => "prefix-timeout",
            Self::PromptHistoryLimit => "prompt-history-limit",
            Self::VariationSelectorAlwaysWide => "variation-selector-always-wide",
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(self, Self::Editor | Self::HistoryFile)
    }

    pub(crate) const fn append_separator(self) -> Option<&'static str> {
        if self.is_string() { Some("") } else { None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ServerOptions {
    pub(crate) backspace: String,
    pub(crate) default_client_command: String,
    pub(crate) editor: String,
    pub(crate) extended_keys: String,
    pub(crate) extended_keys_format: String,
    pub(crate) focus_events: bool,
    pub(crate) get_clipboard: String,
    pub(crate) history_file: String,
    pub(crate) input_buffer_size: u32,
    pub(crate) prefix_timeout_ms: u32,
    pub(crate) prompt_history_limit: usize,
    pub(crate) variation_selector_always_wide: bool,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            backspace: "C-?".to_owned(),
            default_client_command: "new-session".to_owned(),
            editor: "/usr/bin/vi".to_owned(),
            extended_keys: "off".to_owned(),
            extended_keys_format: "xterm".to_owned(),
            focus_events: false,
            get_clipboard: "buffer".to_owned(),
            history_file: String::new(),
            input_buffer_size: 1_048_576,
            prefix_timeout_ms: 0,
            prompt_history_limit: 100,
            variation_selector_always_wide: true,
        }
    }
}

impl ServerOptions {
    pub(crate) fn value(&self, option: ServerOption) -> String {
        match option {
            ServerOption::Backspace => self.backspace.clone(),
            ServerOption::DefaultClientCommand => self.default_client_command.clone(),
            ServerOption::Editor => self.editor.clone(),
            ServerOption::ExtendedKeys => self.extended_keys.clone(),
            ServerOption::ExtendedKeysFormat => self.extended_keys_format.clone(),
            ServerOption::FocusEvents => flag(self.focus_events).to_owned(),
            ServerOption::GetClipboard => self.get_clipboard.clone(),
            ServerOption::HistoryFile => self.history_file.clone(),
            ServerOption::InputBufferSize => self.input_buffer_size.to_string(),
            ServerOption::PrefixTimeout => self.prefix_timeout_ms.to_string(),
            ServerOption::PromptHistoryLimit => self.prompt_history_limit.to_string(),
            ServerOption::VariationSelectorAlwaysWide => {
                flag(self.variation_selector_always_wide).to_owned()
            }
        }
    }

    pub(crate) fn reset(&mut self, option: ServerOption) -> bool {
        let defaults = Self::default();
        match option {
            ServerOption::Backspace => replace(&mut self.backspace, defaults.backspace),
            ServerOption::DefaultClientCommand => replace(
                &mut self.default_client_command,
                defaults.default_client_command,
            ),
            ServerOption::Editor => replace(&mut self.editor, defaults.editor),
            ServerOption::ExtendedKeys => replace(&mut self.extended_keys, defaults.extended_keys),
            ServerOption::ExtendedKeysFormat => replace(
                &mut self.extended_keys_format,
                defaults.extended_keys_format,
            ),
            ServerOption::FocusEvents => replace(&mut self.focus_events, defaults.focus_events),
            ServerOption::GetClipboard => replace(&mut self.get_clipboard, defaults.get_clipboard),
            ServerOption::HistoryFile => replace(&mut self.history_file, defaults.history_file),
            ServerOption::InputBufferSize => {
                replace(&mut self.input_buffer_size, defaults.input_buffer_size)
            }
            ServerOption::PrefixTimeout => {
                replace(&mut self.prefix_timeout_ms, defaults.prefix_timeout_ms)
            }
            ServerOption::PromptHistoryLimit => replace(
                &mut self.prompt_history_limit,
                defaults.prompt_history_limit,
            ),
            ServerOption::VariationSelectorAlwaysWide => replace(
                &mut self.variation_selector_always_wide,
                defaults.variation_selector_always_wide,
            ),
        }
    }

    pub(crate) fn set_command(
        &mut self,
        option: ServerOption,
        value: Option<&str>,
    ) -> Result<bool, String> {
        match option {
            ServerOption::Backspace => replace_string(&mut self.backspace, value),
            ServerOption::DefaultClientCommand => {
                replace_string(&mut self.default_client_command, value)
            }
            ServerOption::Editor => replace_string(&mut self.editor, value),
            ServerOption::ExtendedKeys => {
                replace_choice(&mut self.extended_keys, value, &["off", "on", "always"])
            }
            ServerOption::ExtendedKeysFormat => {
                replace_choice(&mut self.extended_keys_format, value, &["csi-u", "xterm"])
            }
            ServerOption::FocusEvents => {
                let next = parse_flag(value, self.focus_events)?;
                Ok(replace(&mut self.focus_events, next))
            }
            ServerOption::GetClipboard => replace_choice(
                &mut self.get_clipboard,
                value,
                &["off", "buffer", "request", "both"],
            ),
            ServerOption::HistoryFile => {
                let next = required_string(value)?;
                Ok(replace(&mut self.history_file, next))
            }
            ServerOption::InputBufferSize => {
                let next = parse_number(value, 1_048_576, u32::MAX.into())?;
                Ok(replace(
                    &mut self.input_buffer_size,
                    u32::try_from(next).expect("input buffer size is bounded"),
                ))
            }
            ServerOption::PrefixTimeout => {
                let next = parse_number(value, 0, i32::MAX as u64)?;
                Ok(replace(
                    &mut self.prefix_timeout_ms,
                    u32::try_from(next).expect("prefix timeout is bounded"),
                ))
            }
            ServerOption::PromptHistoryLimit => {
                let next = parse_number(value, 0, i32::MAX as u64)?;
                Ok(replace(
                    &mut self.prompt_history_limit,
                    usize::try_from(next).expect("prompt history limit fits usize"),
                ))
            }
            ServerOption::VariationSelectorAlwaysWide => {
                let next = parse_flag(value, self.variation_selector_always_wide)?;
                Ok(replace(&mut self.variation_selector_always_wide, next))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SessionOption {
    ActivityAction,
    AssumePasteTime,
    BellAction,
    DefaultSize,
    DisplayPanesTime,
    KeyTable,
    MessageCommandStyle,
    MessageFormat,
    MessageLine,
    MessageStyle,
    PromptCommandCursorColour,
    PromptCommandCursorStyle,
    PromptCursorColour,
    PromptCursorStyle,
    SilenceAction,
    VisualBell,
}

impl SessionOption {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "activity-action" => Self::ActivityAction,
            "assume-paste-time" => Self::AssumePasteTime,
            "bell-action" => Self::BellAction,
            "default-size" => Self::DefaultSize,
            "display-panes-time" => Self::DisplayPanesTime,
            "key-table" => Self::KeyTable,
            "message-command-style" => Self::MessageCommandStyle,
            "message-format" => Self::MessageFormat,
            "message-line" => Self::MessageLine,
            "message-style" => Self::MessageStyle,
            "prompt-command-cursor-colour" => Self::PromptCommandCursorColour,
            "prompt-command-cursor-style" => Self::PromptCommandCursorStyle,
            "prompt-cursor-colour" => Self::PromptCursorColour,
            "prompt-cursor-style" => Self::PromptCursorStyle,
            "silence-action" => Self::SilenceAction,
            "visual-bell" => Self::VisualBell,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ActivityAction => "activity-action",
            Self::AssumePasteTime => "assume-paste-time",
            Self::BellAction => "bell-action",
            Self::DefaultSize => "default-size",
            Self::DisplayPanesTime => "display-panes-time",
            Self::KeyTable => "key-table",
            Self::MessageCommandStyle => "message-command-style",
            Self::MessageFormat => "message-format",
            Self::MessageLine => "message-line",
            Self::MessageStyle => "message-style",
            Self::PromptCommandCursorColour => "prompt-command-cursor-colour",
            Self::PromptCommandCursorStyle => "prompt-command-cursor-style",
            Self::PromptCursorColour => "prompt-cursor-colour",
            Self::PromptCursorStyle => "prompt-cursor-style",
            Self::SilenceAction => "silence-action",
            Self::VisualBell => "visual-bell",
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(
            self,
            Self::DefaultSize
                | Self::KeyTable
                | Self::MessageCommandStyle
                | Self::MessageFormat
                | Self::MessageStyle
                | Self::PromptCommandCursorColour
                | Self::PromptCursorColour
        )
    }

    pub(crate) const fn append_separator(self) -> Option<&'static str> {
        match self {
            Self::MessageCommandStyle | Self::MessageStyle => Some(","),
            option if option.is_string() => Some(""),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionOptions {
    pub(crate) activity_action: String,
    pub(crate) assume_paste_time_ms: u32,
    pub(crate) bell_action: BellAction,
    pub(crate) default_size: String,
    pub(crate) display_panes_time_ms: u32,
    pub(crate) key_table: String,
    pub(crate) message_command_style: String,
    pub(crate) message_format: String,
    pub(crate) message_line: String,
    pub(crate) message_style: String,
    pub(crate) prompt_command_cursor_colour: String,
    pub(crate) prompt_command_cursor_style: String,
    pub(crate) prompt_cursor_colour: String,
    pub(crate) prompt_cursor_style: String,
    pub(crate) silence_action: String,
    pub(crate) visual_bell: VisualBell,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            activity_action: "other".to_owned(),
            assume_paste_time_ms: 1,
            bell_action: BellAction::Any,
            default_size: "80x24".to_owned(),
            display_panes_time_ms: 1000,
            key_table: "root".to_owned(),
            message_command_style: crate::tmux_options::MESSAGE_COMMAND_STYLE_DEFAULT.to_owned(),
            message_format: crate::tmux_options::MESSAGE_FORMAT_DEFAULT.to_owned(),
            message_line: "0".to_owned(),
            message_style: crate::tmux_options::MESSAGE_STYLE_DEFAULT.to_owned(),
            prompt_command_cursor_colour: String::new(),
            prompt_command_cursor_style: "default".to_owned(),
            prompt_cursor_colour: String::new(),
            prompt_cursor_style: "default".to_owned(),
            silence_action: "other".to_owned(),
            visual_bell: VisualBell::Off,
        }
    }
}

impl SessionOptions {
    pub(crate) fn value(&self, option: SessionOption) -> String {
        match option {
            SessionOption::ActivityAction => self.activity_action.clone(),
            SessionOption::AssumePasteTime => self.assume_paste_time_ms.to_string(),
            SessionOption::BellAction => self.bell_action.as_str().to_owned(),
            SessionOption::DefaultSize => self.default_size.clone(),
            SessionOption::DisplayPanesTime => self.display_panes_time_ms.to_string(),
            SessionOption::KeyTable => self.key_table.clone(),
            SessionOption::MessageCommandStyle => self.message_command_style.clone(),
            SessionOption::MessageFormat => self.message_format.clone(),
            SessionOption::MessageLine => self.message_line.clone(),
            SessionOption::MessageStyle => self.message_style.clone(),
            SessionOption::PromptCommandCursorColour => self.prompt_command_cursor_colour.clone(),
            SessionOption::PromptCommandCursorStyle => self.prompt_command_cursor_style.clone(),
            SessionOption::PromptCursorColour => self.prompt_cursor_colour.clone(),
            SessionOption::PromptCursorStyle => self.prompt_cursor_style.clone(),
            SessionOption::SilenceAction => self.silence_action.clone(),
            SessionOption::VisualBell => self.visual_bell.as_str().to_owned(),
        }
    }

    pub(crate) fn reset(&mut self, option: SessionOption) -> bool {
        let defaults = Self::default();
        match option {
            SessionOption::ActivityAction => {
                replace(&mut self.activity_action, defaults.activity_action)
            }
            SessionOption::AssumePasteTime => replace(
                &mut self.assume_paste_time_ms,
                defaults.assume_paste_time_ms,
            ),
            SessionOption::BellAction => replace(&mut self.bell_action, defaults.bell_action),
            SessionOption::DefaultSize => replace(&mut self.default_size, defaults.default_size),
            SessionOption::DisplayPanesTime => replace(
                &mut self.display_panes_time_ms,
                defaults.display_panes_time_ms,
            ),
            SessionOption::KeyTable => replace(&mut self.key_table, defaults.key_table),
            SessionOption::MessageCommandStyle => replace(
                &mut self.message_command_style,
                defaults.message_command_style,
            ),
            SessionOption::MessageFormat => {
                replace(&mut self.message_format, defaults.message_format)
            }
            SessionOption::MessageLine => replace(&mut self.message_line, defaults.message_line),
            SessionOption::MessageStyle => replace(&mut self.message_style, defaults.message_style),
            SessionOption::PromptCommandCursorColour => replace(
                &mut self.prompt_command_cursor_colour,
                defaults.prompt_command_cursor_colour,
            ),
            SessionOption::PromptCommandCursorStyle => replace(
                &mut self.prompt_command_cursor_style,
                defaults.prompt_command_cursor_style,
            ),
            SessionOption::PromptCursorColour => replace(
                &mut self.prompt_cursor_colour,
                defaults.prompt_cursor_colour,
            ),
            SessionOption::PromptCursorStyle => {
                replace(&mut self.prompt_cursor_style, defaults.prompt_cursor_style)
            }
            SessionOption::SilenceAction => {
                replace(&mut self.silence_action, defaults.silence_action)
            }
            SessionOption::VisualBell => replace(&mut self.visual_bell, defaults.visual_bell),
        }
    }

    pub(crate) fn set_command(
        &mut self,
        option: SessionOption,
        value: Option<&str>,
    ) -> Result<bool, String> {
        match option {
            SessionOption::ActivityAction => replace_choice(
                &mut self.activity_action,
                value,
                &["none", "any", "current", "other"],
            ),
            SessionOption::AssumePasteTime => {
                let next = parse_number(value, 0, i32::MAX as u64)?;
                Ok(replace(
                    &mut self.assume_paste_time_ms,
                    u32::try_from(next).expect("assume paste time is bounded"),
                ))
            }
            SessionOption::BellAction => {
                let next = parse_bell_action(value, self.bell_action)?;
                Ok(replace(&mut self.bell_action, next))
            }
            SessionOption::DefaultSize => {
                let next = required_string(value)?;
                if !default_size_pattern(&next) {
                    return Err(format!("value is invalid: {next}"));
                }
                Ok(replace(&mut self.default_size, next))
            }
            SessionOption::DisplayPanesTime => {
                let next = parse_number(value, 1, i32::MAX as u64)?;
                Ok(replace(
                    &mut self.display_panes_time_ms,
                    u32::try_from(next).expect("display panes time is bounded"),
                ))
            }
            SessionOption::KeyTable => {
                let next = required_string(value)?;
                Ok(replace(&mut self.key_table, next))
            }
            SessionOption::MessageCommandStyle => {
                replace_style(&mut self.message_command_style, value)
            }
            SessionOption::MessageFormat => replace_string(&mut self.message_format, value),
            SessionOption::MessageLine => {
                replace_choice(&mut self.message_line, value, &["0", "1", "2", "3", "4"])
            }
            SessionOption::MessageStyle => replace_style(&mut self.message_style, value),
            SessionOption::PromptCommandCursorColour => {
                replace_colour(&mut self.prompt_command_cursor_colour, value)
            }
            SessionOption::PromptCommandCursorStyle => {
                replace_cursor_style(&mut self.prompt_command_cursor_style, value)
            }
            SessionOption::PromptCursorColour => {
                replace_colour(&mut self.prompt_cursor_colour, value)
            }
            SessionOption::PromptCursorStyle => {
                replace_cursor_style(&mut self.prompt_cursor_style, value)
            }
            SessionOption::SilenceAction => replace_choice(
                &mut self.silence_action,
                value,
                &["none", "any", "current", "other"],
            ),
            SessionOption::VisualBell => {
                let next = parse_visual_bell(value, self.visual_bell)?;
                Ok(replace(&mut self.visual_bell, next))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WindowOption {
    ClockModeColour,
    ClockModeStyle,
    FillCharacter,
    MainPaneHeight,
    MainPaneWidth,
    MonitorActivity,
    MonitorBell,
    MonitorSilence,
    OtherPaneHeight,
    OtherPaneWidth,
    PaneBorderIndicators,
    PaneScrollbars,
    PaneScrollbarsPosition,
    PaneScrollbarsTimeout,
    TiledLayoutMaxColumns,
    WindowSize,
    WrapSearch,
    XtermKeys,
}

impl WindowOption {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "clock-mode-colour" => Self::ClockModeColour,
            "clock-mode-style" => Self::ClockModeStyle,
            "fill-character" => Self::FillCharacter,
            "main-pane-height" => Self::MainPaneHeight,
            "main-pane-width" => Self::MainPaneWidth,
            "monitor-activity" => Self::MonitorActivity,
            "monitor-bell" => Self::MonitorBell,
            "monitor-silence" => Self::MonitorSilence,
            "other-pane-height" => Self::OtherPaneHeight,
            "other-pane-width" => Self::OtherPaneWidth,
            "pane-border-indicators" => Self::PaneBorderIndicators,
            "pane-scrollbars" => Self::PaneScrollbars,
            "pane-scrollbars-position" => Self::PaneScrollbarsPosition,
            "pane-scrollbars-timeout" => Self::PaneScrollbarsTimeout,
            "tiled-layout-max-columns" => Self::TiledLayoutMaxColumns,
            "window-size" => Self::WindowSize,
            "wrap-search" => Self::WrapSearch,
            "xterm-keys" => Self::XtermKeys,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClockModeColour => "clock-mode-colour",
            Self::ClockModeStyle => "clock-mode-style",
            Self::FillCharacter => "fill-character",
            Self::MainPaneHeight => "main-pane-height",
            Self::MainPaneWidth => "main-pane-width",
            Self::MonitorActivity => "monitor-activity",
            Self::MonitorBell => "monitor-bell",
            Self::MonitorSilence => "monitor-silence",
            Self::OtherPaneHeight => "other-pane-height",
            Self::OtherPaneWidth => "other-pane-width",
            Self::PaneBorderIndicators => "pane-border-indicators",
            Self::PaneScrollbars => "pane-scrollbars",
            Self::PaneScrollbarsPosition => "pane-scrollbars-position",
            Self::PaneScrollbarsTimeout => "pane-scrollbars-timeout",
            Self::TiledLayoutMaxColumns => "tiled-layout-max-columns",
            Self::WindowSize => "window-size",
            Self::WrapSearch => "wrap-search",
            Self::XtermKeys => "xterm-keys",
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(
            self,
            Self::ClockModeColour
                | Self::FillCharacter
                | Self::MainPaneHeight
                | Self::MainPaneWidth
                | Self::OtherPaneHeight
                | Self::OtherPaneWidth
        )
    }

    pub(crate) const fn append_separator(self) -> Option<&'static str> {
        if self.is_string() { Some("") } else { None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowOptions {
    pub(crate) clock_mode_colour: String,
    pub(crate) clock_mode_style: String,
    pub(crate) fill_character: String,
    pub(crate) monitor_activity: bool,
    pub(crate) monitor_bell: bool,
    pub(crate) monitor_silence_seconds: u32,
    pub(crate) pane_border_indicators: String,
    pub(crate) pane_scrollbars: String,
    pub(crate) pane_scrollbars_position: String,
    pub(crate) pane_scrollbars_timeout_ms: u32,
    pub(crate) preset: PresetOptions,
    pub(crate) window_size: WindowSize,
    pub(crate) wrap_search: bool,
    pub(crate) xterm_keys: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            clock_mode_colour: "themeblue".to_owned(),
            clock_mode_style: "24".to_owned(),
            fill_character: String::new(),
            monitor_activity: false,
            monitor_bell: true,
            monitor_silence_seconds: 0,
            pane_border_indicators: "colour".to_owned(),
            pane_scrollbars: "off".to_owned(),
            pane_scrollbars_position: "right".to_owned(),
            pane_scrollbars_timeout_ms: 500,
            preset: PresetOptions::default(),
            window_size: WindowSize::Latest,
            wrap_search: true,
            xterm_keys: true,
        }
    }
}

impl WindowOptions {
    pub(crate) fn value(&self, option: WindowOption) -> String {
        match option {
            WindowOption::ClockModeColour => self.clock_mode_colour.clone(),
            WindowOption::ClockModeStyle => self.clock_mode_style.clone(),
            WindowOption::FillCharacter => self.fill_character.clone(),
            WindowOption::MainPaneHeight => self.preset.main_pane_height.clone(),
            WindowOption::MainPaneWidth => self.preset.main_pane_width.clone(),
            WindowOption::MonitorActivity => flag(self.monitor_activity).to_owned(),
            WindowOption::MonitorBell => flag(self.monitor_bell).to_owned(),
            WindowOption::MonitorSilence => self.monitor_silence_seconds.to_string(),
            WindowOption::OtherPaneHeight => self.preset.other_pane_height.clone(),
            WindowOption::OtherPaneWidth => self.preset.other_pane_width.clone(),
            WindowOption::PaneBorderIndicators => self.pane_border_indicators.clone(),
            WindowOption::PaneScrollbars => self.pane_scrollbars.clone(),
            WindowOption::PaneScrollbarsPosition => self.pane_scrollbars_position.clone(),
            WindowOption::PaneScrollbarsTimeout => self.pane_scrollbars_timeout_ms.to_string(),
            WindowOption::TiledLayoutMaxColumns => self.preset.tiled_layout_max_columns.to_string(),
            WindowOption::WindowSize => self.window_size.as_str().to_owned(),
            WindowOption::WrapSearch => flag(self.wrap_search).to_owned(),
            WindowOption::XtermKeys => flag(self.xterm_keys).to_owned(),
        }
    }

    pub(crate) fn reset(&mut self, option: WindowOption) -> bool {
        let defaults = Self::default();
        match option {
            WindowOption::ClockModeColour => {
                replace(&mut self.clock_mode_colour, defaults.clock_mode_colour)
            }
            WindowOption::ClockModeStyle => {
                replace(&mut self.clock_mode_style, defaults.clock_mode_style)
            }
            WindowOption::FillCharacter => {
                replace(&mut self.fill_character, defaults.fill_character)
            }
            WindowOption::MainPaneHeight => replace(
                &mut self.preset.main_pane_height,
                defaults.preset.main_pane_height,
            ),
            WindowOption::MainPaneWidth => replace(
                &mut self.preset.main_pane_width,
                defaults.preset.main_pane_width,
            ),
            WindowOption::MonitorActivity => {
                replace(&mut self.monitor_activity, defaults.monitor_activity)
            }
            WindowOption::MonitorBell => replace(&mut self.monitor_bell, defaults.monitor_bell),
            WindowOption::MonitorSilence => replace(
                &mut self.monitor_silence_seconds,
                defaults.monitor_silence_seconds,
            ),
            WindowOption::OtherPaneHeight => replace(
                &mut self.preset.other_pane_height,
                defaults.preset.other_pane_height,
            ),
            WindowOption::OtherPaneWidth => replace(
                &mut self.preset.other_pane_width,
                defaults.preset.other_pane_width,
            ),
            WindowOption::PaneBorderIndicators => replace(
                &mut self.pane_border_indicators,
                defaults.pane_border_indicators,
            ),
            WindowOption::PaneScrollbars => {
                replace(&mut self.pane_scrollbars, defaults.pane_scrollbars)
            }
            WindowOption::PaneScrollbarsPosition => replace(
                &mut self.pane_scrollbars_position,
                defaults.pane_scrollbars_position,
            ),
            WindowOption::PaneScrollbarsTimeout => replace(
                &mut self.pane_scrollbars_timeout_ms,
                defaults.pane_scrollbars_timeout_ms,
            ),
            WindowOption::TiledLayoutMaxColumns => replace(
                &mut self.preset.tiled_layout_max_columns,
                defaults.preset.tiled_layout_max_columns,
            ),
            WindowOption::WindowSize => replace(&mut self.window_size, defaults.window_size),
            WindowOption::WrapSearch => replace(&mut self.wrap_search, defaults.wrap_search),
            WindowOption::XtermKeys => replace(&mut self.xterm_keys, defaults.xterm_keys),
        }
    }

    pub(crate) fn set_command(
        &mut self,
        option: WindowOption,
        value: Option<&str>,
    ) -> Result<bool, String> {
        match option {
            WindowOption::ClockModeColour => replace_colour(&mut self.clock_mode_colour, value),
            WindowOption::ClockModeStyle => replace_choice(
                &mut self.clock_mode_style,
                value,
                &["12", "24", "12-with-seconds", "24-with-seconds"],
            ),
            WindowOption::FillCharacter => replace_string(&mut self.fill_character, value),
            WindowOption::MainPaneHeight => {
                replace_string(&mut self.preset.main_pane_height, value)
            }
            WindowOption::MainPaneWidth => replace_string(&mut self.preset.main_pane_width, value),
            WindowOption::MonitorActivity => {
                let next = parse_flag(value, self.monitor_activity)?;
                Ok(replace(&mut self.monitor_activity, next))
            }
            WindowOption::MonitorBell => {
                let next = parse_flag(value, self.monitor_bell)?;
                Ok(replace(&mut self.monitor_bell, next))
            }
            WindowOption::MonitorSilence => {
                let next = parse_number(value, 0, i32::MAX as u64)?;
                Ok(replace(
                    &mut self.monitor_silence_seconds,
                    u32::try_from(next).expect("monitor silence is bounded"),
                ))
            }
            WindowOption::OtherPaneHeight => {
                replace_string(&mut self.preset.other_pane_height, value)
            }
            WindowOption::OtherPaneWidth => {
                replace_string(&mut self.preset.other_pane_width, value)
            }
            WindowOption::PaneBorderIndicators => replace_choice(
                &mut self.pane_border_indicators,
                value,
                &["off", "colour", "arrows", "both"],
            ),
            WindowOption::PaneScrollbars => replace_choice(
                &mut self.pane_scrollbars,
                value,
                &["off", "modal", "on", "auto-hide"],
            ),
            WindowOption::PaneScrollbarsPosition => replace_choice(
                &mut self.pane_scrollbars_position,
                value,
                &["right", "left"],
            ),
            WindowOption::PaneScrollbarsTimeout => {
                let next = parse_number(value, 0, i32::MAX as u64)?;
                Ok(replace(
                    &mut self.pane_scrollbars_timeout_ms,
                    u32::try_from(next).expect("pane scrollbar timeout is bounded"),
                ))
            }
            WindowOption::TiledLayoutMaxColumns => {
                let next = parse_number(value, 0, u16::MAX.into())?;
                Ok(replace(
                    &mut self.preset.tiled_layout_max_columns,
                    u16::try_from(next).expect("tiled column limit is bounded"),
                ))
            }
            WindowOption::WindowSize => {
                let next = parse_window_size(value, self.window_size)?;
                Ok(replace(&mut self.window_size, next))
            }
            WindowOption::WrapSearch => {
                let next = parse_flag(value, self.wrap_search)?;
                Ok(replace(&mut self.wrap_search, next))
            }
            WindowOption::XtermKeys => {
                let next = parse_flag(value, self.xterm_keys)?;
                Ok(replace(&mut self.xterm_keys, next))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaneOption {
    AllowPassthrough,
    AllowRename,
    AllowSetTitle,
    AlternateScreen,
    CursorColour,
    CursorStyle,
    PaneBorderLines,
    PaneScrollbarsStyle,
    ScrollOnClear,
}

impl PaneOption {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "allow-passthrough" => Self::AllowPassthrough,
            "allow-rename" => Self::AllowRename,
            "allow-set-title" => Self::AllowSetTitle,
            "alternate-screen" => Self::AlternateScreen,
            "cursor-colour" => Self::CursorColour,
            "cursor-style" => Self::CursorStyle,
            "pane-border-lines" => Self::PaneBorderLines,
            "pane-scrollbars-style" => Self::PaneScrollbarsStyle,
            "scroll-on-clear" => Self::ScrollOnClear,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AllowPassthrough => "allow-passthrough",
            Self::AllowRename => "allow-rename",
            Self::AllowSetTitle => "allow-set-title",
            Self::AlternateScreen => "alternate-screen",
            Self::CursorColour => "cursor-colour",
            Self::CursorStyle => "cursor-style",
            Self::PaneBorderLines => "pane-border-lines",
            Self::PaneScrollbarsStyle => "pane-scrollbars-style",
            Self::ScrollOnClear => "scroll-on-clear",
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(self, Self::CursorColour | Self::PaneScrollbarsStyle)
    }

    pub(crate) const fn append_separator(self) -> Option<&'static str> {
        match self {
            Self::PaneScrollbarsStyle => Some(","),
            option if option.is_string() => Some(""),
            _ => None,
        }
    }

    pub(crate) const fn updates_terminal_worker(self) -> bool {
        matches!(
            self,
            Self::AllowPassthrough | Self::CursorColour | Self::CursorStyle
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneOptions {
    pub(crate) allow_passthrough: AllowPassthrough,
    pub(crate) allow_rename: bool,
    pub(crate) allow_set_title: bool,
    pub(crate) alternate_screen: bool,
    pub(crate) cursor_colour: String,
    pub(crate) cursor_style: PaneCursorStyle,
    pub(crate) pane_border_lines: String,
    pub(crate) pane_scrollbars_style: String,
    pub(crate) scroll_on_clear: bool,
}

impl Default for PaneOptions {
    fn default() -> Self {
        Self {
            allow_passthrough: AllowPassthrough::Off,
            allow_rename: false,
            allow_set_title: true,
            alternate_screen: true,
            cursor_colour: String::new(),
            cursor_style: PaneCursorStyle::Default,
            pane_border_lines: "single".to_owned(),
            pane_scrollbars_style: crate::tmux_options::PANE_SCROLLBARS_STYLE_DEFAULT.to_owned(),
            scroll_on_clear: true,
        }
    }
}

impl PaneOptions {
    pub(crate) fn value(&self, option: PaneOption) -> String {
        match option {
            PaneOption::AllowPassthrough => self.allow_passthrough.as_str().to_owned(),
            PaneOption::AllowRename => flag(self.allow_rename).to_owned(),
            PaneOption::AllowSetTitle => flag(self.allow_set_title).to_owned(),
            PaneOption::AlternateScreen => flag(self.alternate_screen).to_owned(),
            PaneOption::CursorColour => self.cursor_colour.clone(),
            PaneOption::CursorStyle => self.cursor_style.as_str().to_owned(),
            PaneOption::PaneBorderLines => self.pane_border_lines.clone(),
            PaneOption::PaneScrollbarsStyle => self.pane_scrollbars_style.clone(),
            PaneOption::ScrollOnClear => flag(self.scroll_on_clear).to_owned(),
        }
    }

    pub(crate) fn reset(&mut self, option: PaneOption) -> bool {
        let defaults = Self::default();
        match option {
            PaneOption::AllowPassthrough => {
                replace(&mut self.allow_passthrough, defaults.allow_passthrough)
            }
            PaneOption::AllowRename => replace(&mut self.allow_rename, defaults.allow_rename),
            PaneOption::AllowSetTitle => {
                replace(&mut self.allow_set_title, defaults.allow_set_title)
            }
            PaneOption::AlternateScreen => {
                replace(&mut self.alternate_screen, defaults.alternate_screen)
            }
            PaneOption::CursorColour => replace(&mut self.cursor_colour, defaults.cursor_colour),
            PaneOption::CursorStyle => replace(&mut self.cursor_style, defaults.cursor_style),
            PaneOption::PaneBorderLines => {
                replace(&mut self.pane_border_lines, defaults.pane_border_lines)
            }
            PaneOption::PaneScrollbarsStyle => replace(
                &mut self.pane_scrollbars_style,
                defaults.pane_scrollbars_style,
            ),
            PaneOption::ScrollOnClear => {
                replace(&mut self.scroll_on_clear, defaults.scroll_on_clear)
            }
        }
    }

    pub(crate) fn set_command(
        &mut self,
        option: PaneOption,
        value: Option<&str>,
    ) -> Result<bool, String> {
        match option {
            PaneOption::AllowPassthrough => {
                let next = parse_allow_passthrough(value, self.allow_passthrough)?;
                Ok(replace(&mut self.allow_passthrough, next))
            }
            PaneOption::AllowRename => {
                let next = parse_flag(value, self.allow_rename)?;
                Ok(replace(&mut self.allow_rename, next))
            }
            PaneOption::AllowSetTitle => {
                let next = parse_flag(value, self.allow_set_title)?;
                Ok(replace(&mut self.allow_set_title, next))
            }
            PaneOption::AlternateScreen => {
                let next = parse_flag(value, self.alternate_screen)?;
                Ok(replace(&mut self.alternate_screen, next))
            }
            PaneOption::CursorColour => {
                let next = parse_cursor_colour(value)?;
                Ok(replace(&mut self.cursor_colour, next))
            }
            PaneOption::CursorStyle => {
                let next = parse_pane_cursor_style(value, self.cursor_style)?;
                Ok(replace(&mut self.cursor_style, next))
            }
            PaneOption::PaneBorderLines => replace_choice(
                &mut self.pane_border_lines,
                value,
                &[
                    "single", "double", "heavy", "simple", "number", "spaces", "none",
                ],
            ),
            PaneOption::PaneScrollbarsStyle => {
                replace_style(&mut self.pane_scrollbars_style, value)
            }
            PaneOption::ScrollOnClear => {
                let next = parse_flag(value, self.scroll_on_clear)?;
                Ok(replace(&mut self.scroll_on_clear, next))
            }
        }
    }
}

fn replace<T: PartialEq>(slot: &mut T, next: T) -> bool {
    std::mem::replace(slot, next) != *slot
}

fn replace_string(slot: &mut String, value: Option<&str>) -> Result<bool, String> {
    Ok(replace(slot, required_string(value)?))
}

fn replace_choice(
    slot: &mut String,
    value: Option<&str>,
    choices: &[&str],
) -> Result<bool, String> {
    let next = match value {
        Some(value) if choices.contains(&value) => value.to_owned(),
        Some(value) => return Err(format!("unknown value: {value}")),
        None => match choices.iter().position(|choice| *choice == slot) {
            Some(0) => choices[1].to_owned(),
            Some(1) => choices[0].to_owned(),
            _ => slot.clone(),
        },
    };
    Ok(replace(slot, next))
}

fn replace_cursor_style(slot: &mut String, value: Option<&str>) -> Result<bool, String> {
    replace_choice(
        slot,
        value,
        &[
            "default",
            "blinking-block",
            "block",
            "blinking-underline",
            "underline",
            "blinking-bar",
            "bar",
        ],
    )
}

fn replace_style(slot: &mut String, value: Option<&str>) -> Result<bool, String> {
    let value = required_string(value)?;
    if !value.contains("#{") && !valid_style(&value) {
        return Err(format!("invalid style: {value}"));
    }
    Ok(replace(slot, value))
}

fn replace_colour(slot: &mut String, value: Option<&str>) -> Result<bool, String> {
    Ok(replace(slot, parse_colour_string(value)?))
}

fn required_string(value: Option<&str>) -> Result<String, String> {
    value
        .map(str::to_owned)
        .ok_or_else(|| "empty value".to_owned())
}

fn flag(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn parse_flag(value: Option<&str>, current: bool) -> Result<bool, String> {
    let Some(value) = value else {
        return Ok(!current);
    };
    if value.is_empty() {
        return Ok(!current);
    }
    if value == "1"
        || ["on", "yes"]
            .iter()
            .any(|word| value.eq_ignore_ascii_case(word))
    {
        return Ok(true);
    }
    if value == "0"
        || ["off", "no"]
            .iter()
            .any(|word| value.eq_ignore_ascii_case(word))
    {
        return Ok(false);
    }
    Err(format!("bad value: {value}"))
}

fn parse_number(value: Option<&str>, minimum: u64, maximum: u64) -> Result<u64, String> {
    let value = value.ok_or_else(|| "empty value".to_owned())?;
    match value.parse::<i128>() {
        Ok(number) if number < i128::from(minimum) => Err(format!("value is too small: {value}")),
        Ok(number) if number > i128::from(maximum) => Err(format!("value is too large: {value}")),
        Ok(number) => Ok(u64::try_from(number).expect("nonnegative number fits u64")),
        Err(_) if decimal_digits(value.strip_prefix('-')) => {
            Err(format!("value is too small: {value}"))
        }
        Err(_) if decimal_digits(Some(value.strip_prefix('+').unwrap_or(value))) => {
            Err(format!("value is too large: {value}"))
        }
        Err(_) => Err(format!("value is invalid: {value}")),
    }
}

fn decimal_digits(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
}

fn parse_bell_action(value: Option<&str>, current: BellAction) -> Result<BellAction, String> {
    let Some(value) = value else {
        return Ok(current.toggled());
    };
    BellAction::parse(value).ok_or_else(|| format!("unknown value: {value}"))
}

fn parse_visual_bell(value: Option<&str>, current: VisualBell) -> Result<VisualBell, String> {
    let Some(value) = value else {
        return Ok(current.toggled());
    };
    VisualBell::parse(value).ok_or_else(|| format!("unknown value: {value}"))
}

fn parse_window_size(value: Option<&str>, current: WindowSize) -> Result<WindowSize, String> {
    let Some(value) = value else {
        return Ok(current.toggled());
    };
    match value {
        "largest" => Ok(WindowSize::Largest),
        "smallest" => Ok(WindowSize::Smallest),
        "manual" => Ok(WindowSize::Manual),
        "latest" => Ok(WindowSize::Latest),
        _ => Err(format!("unknown value: {value}")),
    }
}

fn parse_allow_passthrough(
    value: Option<&str>,
    current: AllowPassthrough,
) -> Result<AllowPassthrough, String> {
    let Some(value) = value else {
        return Ok(current.toggled());
    };
    match value {
        "off" => Ok(AllowPassthrough::Off),
        "on" => Ok(AllowPassthrough::On),
        "all" => Ok(AllowPassthrough::All),
        _ => Err(format!("unknown value: {value}")),
    }
}

fn parse_pane_cursor_style(
    value: Option<&str>,
    current: PaneCursorStyle,
) -> Result<PaneCursorStyle, String> {
    let Some(value) = value else {
        return Ok(current.toggled());
    };
    match value {
        "default" => Ok(PaneCursorStyle::Default),
        "blinking-block" => Ok(PaneCursorStyle::BlinkingBlock),
        "block" => Ok(PaneCursorStyle::Block),
        "blinking-underline" => Ok(PaneCursorStyle::BlinkingUnderline),
        "underline" => Ok(PaneCursorStyle::Underline),
        "blinking-bar" => Ok(PaneCursorStyle::BlinkingBar),
        "bar" => Ok(PaneCursorStyle::Bar),
        _ => Err(format!("unknown value: {value}")),
    }
}

fn parse_cursor_colour(value: Option<&str>) -> Result<String, String> {
    parse_colour_string(value)
}

fn parse_colour_string(value: Option<&str>) -> Result<String, String> {
    let value = required_string(value)?;
    if value.is_empty()
        || value.contains("#{")
        || parse_tmux_colour(&value).is_some()
        || parse_x11_color(&value).is_some()
    {
        Ok(value)
    } else {
        Err(format!("invalid colour: {value}"))
    }
}

fn default_size_pattern(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && value
            .char_indices()
            .filter(|(_, character)| *character == 'x')
            .any(|(index, _)| {
                value
                    .as_bytes()
                    .get(index + 1)
                    .is_some_and(u8::is_ascii_digit)
            })
}
use crate::{formats::parse_tmux_colour, valid_style};
use zz_terminal::parse_x11_color;
