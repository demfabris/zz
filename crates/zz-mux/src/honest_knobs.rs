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
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            preset: PresetOptions::default(),
            window_size: WindowSize::Latest,
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
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaneOption {
    AllowRename,
    AllowSetTitle,
}

impl PaneOption {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "allow-rename" => Self::AllowRename,
            "allow-set-title" => Self::AllowSetTitle,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AllowRename => "allow-rename",
            Self::AllowSetTitle => "allow-set-title",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneOptions {
    pub(crate) allow_rename: bool,
    pub(crate) allow_set_title: bool,
}

impl Default for PaneOptions {
    fn default() -> Self {
        Self {
            allow_rename: false,
            allow_set_title: true,
        }
    }
}

impl PaneOptions {
    pub(crate) fn value(&self, option: PaneOption) -> String {
        flag(match option {
            PaneOption::AllowRename => self.allow_rename,
            PaneOption::AllowSetTitle => self.allow_set_title,
        })
        .to_owned()
    }

    pub(crate) fn reset(&mut self, option: PaneOption) -> bool {
        let defaults = Self::default();
        match option {
            PaneOption::AllowRename => replace(&mut self.allow_rename, defaults.allow_rename),
            PaneOption::AllowSetTitle => {
                replace(&mut self.allow_set_title, defaults.allow_set_title)
            }
        }
    }

    pub(crate) fn set_command(
        &mut self,
        option: PaneOption,
        value: Option<&str>,
    ) -> Result<bool, String> {
        let current = match option {
            PaneOption::AllowRename => self.allow_rename,
            PaneOption::AllowSetTitle => self.allow_set_title,
        };
        let next = parse_flag(value, current)?;
        Ok(match option {
            PaneOption::AllowRename => replace(&mut self.allow_rename, next),
            PaneOption::AllowSetTitle => replace(&mut self.allow_set_title, next),
        })
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
