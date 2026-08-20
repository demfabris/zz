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
    FocusEvents,
    HistoryFile,
    PrefixTimeout,
    PromptHistoryLimit,
}

impl ServerOption {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "focus-events" => Self::FocusEvents,
            "history-file" => Self::HistoryFile,
            "prefix-timeout" => Self::PrefixTimeout,
            "prompt-history-limit" => Self::PromptHistoryLimit,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::FocusEvents => "focus-events",
            Self::HistoryFile => "history-file",
            Self::PrefixTimeout => "prefix-timeout",
            Self::PromptHistoryLimit => "prompt-history-limit",
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(self, Self::HistoryFile)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ServerOptions {
    pub(crate) focus_events: bool,
    pub(crate) history_file: String,
    pub(crate) prefix_timeout_ms: u32,
    pub(crate) prompt_history_limit: usize,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            focus_events: false,
            history_file: String::new(),
            prefix_timeout_ms: 0,
            prompt_history_limit: 100,
        }
    }
}

impl ServerOptions {
    pub(crate) fn value(&self, option: ServerOption) -> String {
        match option {
            ServerOption::FocusEvents => flag(self.focus_events).to_owned(),
            ServerOption::HistoryFile => self.history_file.clone(),
            ServerOption::PrefixTimeout => self.prefix_timeout_ms.to_string(),
            ServerOption::PromptHistoryLimit => self.prompt_history_limit.to_string(),
        }
    }

    pub(crate) fn reset(&mut self, option: ServerOption) -> bool {
        let defaults = Self::default();
        match option {
            ServerOption::FocusEvents => replace(&mut self.focus_events, defaults.focus_events),
            ServerOption::HistoryFile => replace(&mut self.history_file, defaults.history_file),
            ServerOption::PrefixTimeout => {
                replace(&mut self.prefix_timeout_ms, defaults.prefix_timeout_ms)
            }
            ServerOption::PromptHistoryLimit => replace(
                &mut self.prompt_history_limit,
                defaults.prompt_history_limit,
            ),
        }
    }

    pub(crate) fn set_command(
        &mut self,
        option: ServerOption,
        value: Option<&str>,
    ) -> Result<bool, String> {
        match option {
            ServerOption::FocusEvents => {
                let next = parse_flag(value, self.focus_events)?;
                Ok(replace(&mut self.focus_events, next))
            }
            ServerOption::HistoryFile => {
                let next = required_string(value)?;
                Ok(replace(&mut self.history_file, next))
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
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SessionOption {
    BellAction,
    DefaultSize,
    DisplayPanesTime,
    KeyTable,
    VisualBell,
}

impl SessionOption {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "bell-action" => Self::BellAction,
            "default-size" => Self::DefaultSize,
            "display-panes-time" => Self::DisplayPanesTime,
            "key-table" => Self::KeyTable,
            "visual-bell" => Self::VisualBell,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::BellAction => "bell-action",
            Self::DefaultSize => "default-size",
            Self::DisplayPanesTime => "display-panes-time",
            Self::KeyTable => "key-table",
            Self::VisualBell => "visual-bell",
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(self, Self::DefaultSize | Self::KeyTable)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionOptions {
    pub(crate) bell_action: BellAction,
    pub(crate) default_size: String,
    pub(crate) display_panes_time_ms: u32,
    pub(crate) key_table: String,
    pub(crate) visual_bell: VisualBell,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            bell_action: BellAction::Any,
            default_size: "80x24".to_owned(),
            display_panes_time_ms: 1000,
            key_table: "root".to_owned(),
            visual_bell: VisualBell::Off,
        }
    }
}

impl SessionOptions {
    pub(crate) fn value(&self, option: SessionOption) -> String {
        match option {
            SessionOption::BellAction => self.bell_action.as_str().to_owned(),
            SessionOption::DefaultSize => self.default_size.clone(),
            SessionOption::DisplayPanesTime => self.display_panes_time_ms.to_string(),
            SessionOption::KeyTable => self.key_table.clone(),
            SessionOption::VisualBell => self.visual_bell.as_str().to_owned(),
        }
    }

    pub(crate) fn reset(&mut self, option: SessionOption) -> bool {
        let defaults = Self::default();
        match option {
            SessionOption::BellAction => replace(&mut self.bell_action, defaults.bell_action),
            SessionOption::DefaultSize => replace(&mut self.default_size, defaults.default_size),
            SessionOption::DisplayPanesTime => replace(
                &mut self.display_panes_time_ms,
                defaults.display_panes_time_ms,
            ),
            SessionOption::KeyTable => replace(&mut self.key_table, defaults.key_table),
            SessionOption::VisualBell => replace(&mut self.visual_bell, defaults.visual_bell),
        }
    }

    pub(crate) fn set_command(
        &mut self,
        option: SessionOption,
        value: Option<&str>,
    ) -> Result<bool, String> {
        match option {
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
            SessionOption::VisualBell => {
                let next = parse_visual_bell(value, self.visual_bell)?;
                Ok(replace(&mut self.visual_bell, next))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WindowOption {
    MainPaneHeight,
    MainPaneWidth,
    OtherPaneHeight,
    OtherPaneWidth,
    TiledLayoutMaxColumns,
    WindowSize,
    WrapSearch,
}

impl WindowOption {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "main-pane-height" => Self::MainPaneHeight,
            "main-pane-width" => Self::MainPaneWidth,
            "other-pane-height" => Self::OtherPaneHeight,
            "other-pane-width" => Self::OtherPaneWidth,
            "tiled-layout-max-columns" => Self::TiledLayoutMaxColumns,
            "window-size" => Self::WindowSize,
            "wrap-search" => Self::WrapSearch,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::MainPaneHeight => "main-pane-height",
            Self::MainPaneWidth => "main-pane-width",
            Self::OtherPaneHeight => "other-pane-height",
            Self::OtherPaneWidth => "other-pane-width",
            Self::TiledLayoutMaxColumns => "tiled-layout-max-columns",
            Self::WindowSize => "window-size",
            Self::WrapSearch => "wrap-search",
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(
            self,
            Self::MainPaneHeight
                | Self::MainPaneWidth
                | Self::OtherPaneHeight
                | Self::OtherPaneWidth
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowOptions {
    pub(crate) preset: PresetOptions,
    pub(crate) window_size: WindowSize,
    pub(crate) wrap_search: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            preset: PresetOptions::default(),
            window_size: WindowSize::Latest,
            wrap_search: true,
        }
    }
}

impl WindowOptions {
    pub(crate) fn value(&self, option: WindowOption) -> String {
        match option {
            WindowOption::MainPaneHeight => self.preset.main_pane_height.clone(),
            WindowOption::MainPaneWidth => self.preset.main_pane_width.clone(),
            WindowOption::OtherPaneHeight => self.preset.other_pane_height.clone(),
            WindowOption::OtherPaneWidth => self.preset.other_pane_width.clone(),
            WindowOption::TiledLayoutMaxColumns => self.preset.tiled_layout_max_columns.to_string(),
            WindowOption::WindowSize => self.window_size.as_str().to_owned(),
            WindowOption::WrapSearch => flag(self.wrap_search).to_owned(),
        }
    }

    pub(crate) fn reset(&mut self, option: WindowOption) -> bool {
        let defaults = Self::default();
        match option {
            WindowOption::MainPaneHeight => replace(
                &mut self.preset.main_pane_height,
                defaults.preset.main_pane_height,
            ),
            WindowOption::MainPaneWidth => replace(
                &mut self.preset.main_pane_width,
                defaults.preset.main_pane_width,
            ),
            WindowOption::OtherPaneHeight => replace(
                &mut self.preset.other_pane_height,
                defaults.preset.other_pane_height,
            ),
            WindowOption::OtherPaneWidth => replace(
                &mut self.preset.other_pane_width,
                defaults.preset.other_pane_width,
            ),
            WindowOption::TiledLayoutMaxColumns => replace(
                &mut self.preset.tiled_layout_max_columns,
                defaults.preset.tiled_layout_max_columns,
            ),
            WindowOption::WindowSize => replace(&mut self.window_size, defaults.window_size),
            WindowOption::WrapSearch => replace(&mut self.wrap_search, defaults.wrap_search),
        }
    }

    pub(crate) fn set_command(
        &mut self,
        option: WindowOption,
        value: Option<&str>,
    ) -> Result<bool, String> {
        match option {
            WindowOption::MainPaneHeight => {
                replace_string(&mut self.preset.main_pane_height, value)
            }
            WindowOption::MainPaneWidth => replace_string(&mut self.preset.main_pane_width, value),
            WindowOption::OtherPaneHeight => {
                replace_string(&mut self.preset.other_pane_height, value)
            }
            WindowOption::OtherPaneWidth => {
                replace_string(&mut self.preset.other_pane_width, value)
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
            Self::ScrollOnClear => "scroll-on-clear",
        }
    }

    pub(crate) const fn is_string(self) -> bool {
        matches!(self, Self::CursorColour)
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
    match value {
        "none" => Ok(BellAction::None),
        "any" => Ok(BellAction::Any),
        "current" => Ok(BellAction::Current),
        "other" => Ok(BellAction::Other),
        _ => Err(format!("unknown value: {value}")),
    }
}

fn parse_visual_bell(value: Option<&str>, current: VisualBell) -> Result<VisualBell, String> {
    let Some(value) = value else {
        return Ok(current.toggled());
    };
    match value {
        "off" => Ok(VisualBell::Off),
        "on" => Ok(VisualBell::On),
        "both" => Ok(VisualBell::Both),
        _ => Err(format!("unknown value: {value}")),
    }
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
use crate::formats::parse_tmux_colour;
use zz_terminal::parse_x11_color;
