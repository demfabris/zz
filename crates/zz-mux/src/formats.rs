use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt::Write as _,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{Datelike as _, Local, TimeZone as _};
use glob::{MatchOptions, Pattern};
use regex::{Captures, RegexBuilder};
use unicode_width::UnicodeWidthChar as _;
use zz_protocol::{MAX_STATUS_TEXT_BYTES, PaneId, SessionId, WindowId};

use crate::{MuxEngine, PaneKind, layout::CellLayout};

const FORMAT_LOOP_LIMIT: usize = 100;
const FORMAT_MAX_WIDTH: isize = 10_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FormatType {
    #[default]
    None,
    Session,
    Window,
    Pane,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusContext {
    pub active_window_index: Option<u32>,
    pub history_limit: Option<usize>,
    pub host: String,
    pub host_short: String,
    pub last_window_index: Option<u32>,
    pub next_session_id: String,
    pub pane_active: Option<bool>,
    pub pane_at_bottom: Option<bool>,
    pub pane_at_left: Option<bool>,
    pub pane_at_right: Option<bool>,
    pub pane_at_top: Option<bool>,
    pub pane_bottom: Option<u16>,
    pub pane_current_path: String,
    pub pane_flags: String,
    pub pane_height: Option<u16>,
    pub pane_id: String,
    pub pane_index: u32,
    pub pane_last: Option<bool>,
    pub pane_left: Option<u16>,
    pub pane_right: Option<u16>,
    pub pane_synchronized: bool,
    pub pane_title: String,
    pub pane_top: Option<u16>,
    pub pane_width: Option<u16>,
    pub pane_x: Option<u16>,
    pub pane_y: Option<u16>,
    pub pane_z: Option<usize>,
    pub pane_zoomed: bool,
    pub pid: u32,
    pub server_sessions: usize,
    pub session_active: Option<bool>,
    pub session_alert: String,
    pub session_alerts: String,
    pub session_attached: usize,
    pub session_attached_list: String,
    pub session_bell: bool,
    pub session_id: String,
    pub session_many_attached: bool,
    pub session_name: String,
    pub session_stack: String,
    pub session_windows: usize,
    pub socket_path: String,
    pub start_time: Option<i64>,
    pub uid: String,
    pub user: String,
    pub version: String,
    pub window_active: Option<bool>,
    pub window_active_clients: usize,
    pub window_active_clients_list: String,
    pub window_active_sessions: usize,
    pub window_active_sessions_list: String,
    pub window_bell: bool,
    pub window_end: Option<bool>,
    pub window_height: Option<u16>,
    pub window_id: String,
    pub window_index: u32,
    pub window_last: Option<bool>,
    pub window_layout: String,
    pub window_linked_sessions: usize,
    pub window_linked_sessions_list: String,
    pub window_name: String,
    pub window_panes: usize,
    pub window_stack_index: usize,
    pub window_start: Option<bool>,
    pub window_visible_layout: String,
    pub window_width: Option<u16>,
    pub window_zoomed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FormatContext {
    pub(crate) session: Option<SessionId>,
    pub(crate) window: Option<WindowId>,
    pub(crate) pane: Option<PaneId>,
    pub(crate) active_session: Option<SessionId>,
    pub(crate) format_type: FormatType,
}

impl Default for FormatContext {
    fn default() -> Self {
        Self {
            session: None,
            window: None,
            pane: None,
            active_session: None,
            format_type: FormatType::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormatScope {
    Server,
    Buffer,
    Client,
    Session,
    Window,
    Pane,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormatKind {
    String,
    Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormatBacking {
    Empty,
    Zero,
    ActiveWindowIndex,
    HistoryLimit,
    Host,
    HostShort,
    LastWindowIndex,
    NextSessionId,
    PaneActive,
    PaneAtBottom,
    PaneAtLeft,
    PaneAtRight,
    PaneAtTop,
    PaneBottom,
    PaneCurrentPath,
    PaneFlags,
    PaneFormat,
    PaneHeight,
    PaneId,
    PaneIndex,
    PaneLast,
    PaneLeft,
    PaneRight,
    PaneSynchronized,
    PaneTitle,
    PaneTop,
    PaneWidth,
    PaneX,
    PaneY,
    PaneZ,
    PaneZoomed,
    Pid,
    ServerSessions,
    SessionActive,
    SessionAlert,
    SessionAlerts,
    SessionAttached,
    SessionAttachedList,
    SessionBell,
    SessionFormat,
    SessionId,
    SessionManyAttached,
    SessionName,
    SessionStack,
    SessionWindows,
    SocketPath,
    StartTime,
    Uid,
    User,
    Version,
    WindowActive,
    WindowActiveClients,
    WindowActiveClientsList,
    WindowActiveSessions,
    WindowActiveSessionsList,
    WindowBell,
    WindowEnd,
    WindowFlags,
    WindowFormat,
    WindowHeight,
    WindowId,
    WindowIndex,
    WindowLast,
    WindowLayout,
    WindowLinked,
    WindowLinkedSessions,
    WindowLinkedSessionsList,
    WindowName,
    WindowPanes,
    WindowRawFlags,
    WindowStackIndex,
    WindowStart,
    WindowVisibleLayout,
    WindowWidth,
    WindowZoomed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FormatVariableSpec {
    name: &'static str,
    scope: FormatScope,
    kind: FormatKind,
    backing: FormatBacking,
}

macro_rules! variable {
    ($name:literal, $scope:ident, $backing:ident) => {
        FormatVariableSpec {
            name: $name,
            scope: FormatScope::$scope,
            kind: FormatKind::String,
            backing: FormatBacking::$backing,
        }
    };
    ($name:literal, $scope:ident, Time, $backing:ident) => {
        FormatVariableSpec {
            name: $name,
            scope: FormatScope::$scope,
            kind: FormatKind::Time,
            backing: FormatBacking::$backing,
        }
    };
}

const FORMAT_VARIABLES: [FormatVariableSpec; 198] = [
    variable!("active_window_index", Session, ActiveWindowIndex),
    variable!("alternate_on", Terminal, Zero),
    variable!("alternate_saved_x", Terminal, Zero),
    variable!("alternate_saved_y", Terminal, Zero),
    variable!("bracket_paste_flag", Terminal, Zero),
    variable!("buffer_created", Buffer, Time, Empty),
    variable!("buffer_full", Buffer, Empty),
    variable!("buffer_mode_format", Buffer, Empty),
    variable!("buffer_name", Buffer, Empty),
    variable!("buffer_sample", Buffer, Empty),
    variable!("buffer_size", Buffer, Zero),
    variable!("client_activity", Client, Time, Empty),
    variable!("client_cell_height", Client, Zero),
    variable!("client_cell_width", Client, Zero),
    variable!("client_colours", Client, Empty),
    variable!("client_control_mode", Client, Zero),
    variable!("client_created", Client, Time, Empty),
    variable!("client_discarded", Client, Zero),
    variable!("client_flags", Client, Empty),
    variable!("client_height", Client, Zero),
    variable!("client_key_table", Client, Empty),
    variable!("client_last_session", Client, Empty),
    variable!("client_mode_format", Client, Empty),
    variable!("client_name", Client, Empty),
    variable!("client_pid", Client, Zero),
    variable!("client_prefix", Client, Zero),
    variable!("client_readonly", Client, Zero),
    variable!("client_session", Client, Empty),
    variable!("client_termfeatures", Client, Empty),
    variable!("client_termname", Client, Empty),
    variable!("client_termtype", Client, Empty),
    variable!("client_theme", Client, Empty),
    variable!("client_tty", Client, Empty),
    variable!("client_uid", Client, Zero),
    variable!("client_user", Client, Empty),
    variable!("client_utf8", Client, Zero),
    variable!("client_width", Client, Zero),
    variable!("client_written", Client, Zero),
    variable!("config_files", Server, Empty),
    variable!("cursor_blinking", Terminal, Zero),
    variable!("cursor_character", Terminal, Empty),
    variable!("cursor_colour", Terminal, Empty),
    variable!("cursor_flag", Terminal, Zero),
    variable!("cursor_shape", Terminal, Zero),
    variable!("cursor_very_visible", Terminal, Zero),
    variable!("cursor_x", Terminal, Zero),
    variable!("cursor_y", Terminal, Zero),
    variable!("history_all_bytes", Terminal, Zero),
    variable!("history_bytes", Terminal, Zero),
    variable!("history_limit", Terminal, HistoryLimit),
    variable!("history_size", Terminal, Zero),
    variable!("host", Server, Host),
    variable!("host_short", Server, HostShort),
    variable!("insert_flag", Terminal, Zero),
    variable!("keypad_cursor_flag", Terminal, Zero),
    variable!("keypad_flag", Terminal, Zero),
    variable!("last_window_index", Session, LastWindowIndex),
    variable!("mouse_all_flag", Terminal, Zero),
    variable!("mouse_any_flag", Terminal, Zero),
    variable!("mouse_button_flag", Terminal, Zero),
    variable!("mouse_hyperlink", Terminal, Empty),
    variable!("mouse_line", Terminal, Empty),
    variable!("mouse_pane", Terminal, Empty),
    variable!("mouse_sgr_flag", Terminal, Zero),
    variable!("mouse_standard_flag", Terminal, Zero),
    variable!("mouse_status_line", Terminal, Empty),
    variable!("mouse_status_range", Terminal, Empty),
    variable!("mouse_utf8_flag", Terminal, Zero),
    variable!("mouse_word", Terminal, Empty),
    variable!("mouse_x", Terminal, Zero),
    variable!("mouse_y", Terminal, Zero),
    variable!("next_session_id", Server, NextSessionId),
    variable!("origin_flag", Terminal, Zero),
    variable!("pane_active", Pane, PaneActive),
    variable!("pane_at_bottom", Pane, PaneAtBottom),
    variable!("pane_at_left", Pane, PaneAtLeft),
    variable!("pane_at_right", Pane, PaneAtRight),
    variable!("pane_at_top", Pane, PaneAtTop),
    variable!("pane_bg", Pane, Empty),
    variable!("pane_bottom", Pane, PaneBottom),
    variable!("pane_current_command", Pane, Empty),
    variable!("pane_current_path", Pane, PaneCurrentPath),
    variable!("pane_dead", Pane, Zero),
    variable!("pane_dead_signal", Pane, Empty),
    variable!("pane_dead_status", Pane, Empty),
    variable!("pane_dead_time", Pane, Time, Empty),
    variable!("pane_fg", Pane, Empty),
    variable!("pane_flags", Pane, PaneFlags),
    variable!("pane_floating_flag", Pane, Zero),
    variable!("pane_format", Pane, PaneFormat),
    variable!("pane_height", Pane, PaneHeight),
    variable!("pane_id", Pane, PaneId),
    variable!("pane_in_mode", Pane, Zero),
    variable!("pane_index", Pane, PaneIndex),
    variable!("pane_input_off", Pane, Zero),
    variable!("pane_key_mode", Pane, Empty),
    variable!("pane_last", Pane, PaneLast),
    variable!("pane_left", Pane, PaneLeft),
    variable!("pane_marked", Pane, Zero),
    variable!("pane_marked_set", Pane, Zero),
    variable!("pane_mode", Pane, Empty),
    variable!("pane_path", Pane, Empty),
    variable!("pane_pb_progress", Pane, Zero),
    variable!("pane_pb_state", Pane, Empty),
    variable!("pane_pid", Pane, Zero),
    variable!("pane_pipe", Pane, Zero),
    variable!("pane_pipe_pid", Pane, Zero),
    variable!("pane_right", Pane, PaneRight),
    variable!("pane_search_string", Pane, Empty),
    variable!("pane_start_command", Pane, Empty),
    variable!("pane_start_command_list", Pane, Empty),
    variable!("pane_start_path", Pane, PaneCurrentPath),
    variable!("pane_synchronized", Pane, PaneSynchronized),
    variable!("pane_tabs", Pane, Empty),
    variable!("pane_title", Pane, PaneTitle),
    variable!("pane_top", Pane, PaneTop),
    variable!("pane_tty", Pane, Empty),
    variable!("pane_unseen_changes", Pane, Zero),
    variable!("pane_width", Pane, PaneWidth),
    variable!("pane_x", Pane, PaneX),
    variable!("pane_y", Pane, PaneY),
    variable!("pane_z", Pane, PaneZ),
    variable!("pane_zoomed_flag", Pane, PaneZoomed),
    variable!("pid", Server, Pid),
    variable!("scroll_region_lower", Terminal, Zero),
    variable!("scroll_region_upper", Terminal, Zero),
    variable!("server_sessions", Server, ServerSessions),
    variable!("session_active", Session, SessionActive),
    variable!("session_activity", Session, Time, Empty),
    variable!("session_activity_flag", Session, Zero),
    variable!("session_alert", Session, SessionAlert),
    variable!("session_alerts", Session, SessionAlerts),
    variable!("session_attached", Session, SessionAttached),
    variable!("session_attached_list", Session, SessionAttachedList),
    variable!("session_bell_flag", Session, SessionBell),
    variable!("session_created", Session, Time, Empty),
    variable!("session_format", Session, SessionFormat),
    variable!("session_group", Session, Empty),
    variable!("session_group_attached", Session, Zero),
    variable!("session_group_attached_list", Session, Empty),
    variable!("session_group_list", Session, Empty),
    variable!("session_group_many_attached", Session, Zero),
    variable!("session_group_size", Session, Zero),
    variable!("session_grouped", Session, Zero),
    variable!("session_id", Session, SessionId),
    variable!("session_last_attached", Session, Time, Empty),
    variable!("session_many_attached", Session, SessionManyAttached),
    variable!("session_marked", Session, Zero),
    variable!("session_name", Session, SessionName),
    variable!("session_path", Session, Empty),
    variable!("session_silence_flag", Session, Zero),
    variable!("session_stack", Session, SessionStack),
    variable!("session_windows", Session, SessionWindows),
    variable!("sixel_support", Server, Zero),
    variable!("socket_path", Server, SocketPath),
    variable!("start_time", Server, Time, StartTime),
    variable!("synchronized_output_flag", Terminal, Zero),
    variable!("tree_mode_format", Server, Empty),
    variable!("uid", Server, Uid),
    variable!("user", Server, User),
    variable!("version", Server, Version),
    variable!("window_active", Window, WindowActive),
    variable!("window_active_clients", Window, WindowActiveClients),
    variable!(
        "window_active_clients_list",
        Window,
        WindowActiveClientsList
    ),
    variable!("window_active_sessions", Window, WindowActiveSessions),
    variable!(
        "window_active_sessions_list",
        Window,
        WindowActiveSessionsList
    ),
    variable!("window_activity", Window, Time, Empty),
    variable!("window_activity_flag", Window, Zero),
    variable!("window_bell_flag", Window, WindowBell),
    variable!("window_bigger", Window, Zero),
    variable!("window_cell_height", Window, Zero),
    variable!("window_cell_width", Window, Zero),
    variable!("window_end_flag", Window, WindowEnd),
    variable!("window_flags", Window, WindowFlags),
    variable!("window_format", Window, WindowFormat),
    variable!("window_height", Window, WindowHeight),
    variable!("window_id", Window, WindowId),
    variable!("window_index", Window, WindowIndex),
    variable!("window_last_flag", Window, WindowLast),
    variable!("window_layout", Window, WindowLayout),
    variable!("window_linked", Window, WindowLinked),
    variable!("window_linked_sessions", Window, WindowLinkedSessions),
    variable!(
        "window_linked_sessions_list",
        Window,
        WindowLinkedSessionsList
    ),
    variable!("window_manual_height", Window, Zero),
    variable!("window_manual_width", Window, Zero),
    variable!("window_marked_flag", Window, Zero),
    variable!("window_name", Window, WindowName),
    variable!("window_offset_x", Window, Empty),
    variable!("window_offset_y", Window, Empty),
    variable!("window_panes", Window, WindowPanes),
    variable!("window_raw_flags", Window, WindowRawFlags),
    variable!("window_silence_flag", Window, Zero),
    variable!("window_stack_index", Window, WindowStackIndex),
    variable!("window_start_flag", Window, WindowStart),
    variable!("window_visible_layout", Window, WindowVisibleLayout),
    variable!("window_width", Window, WindowWidth),
    variable!("window_zoomed_flag", Window, WindowZoomed),
    variable!("wrap_flag", Terminal, Zero),
];

struct ResolvedFormatContext {
    values: StatusContext,
    has_session: bool,
    has_window: bool,
    has_pane: bool,
    format_type: FormatType,
}

trait FormatVariables {
    fn variable(&self, name: &str) -> Option<Cow<'_, str>>;
    fn variable_kind(&self, name: &str) -> Option<FormatKind>;
}

impl StatusContext {
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<Cow<'_, str>> {
        let spec = format_variable(name)?;
        Some(self.resolve(spec, FormatType::None))
    }

    fn resolve(&self, spec: &FormatVariableSpec, format_type: FormatType) -> Cow<'_, str> {
        let available = match spec.scope {
            FormatScope::Server | FormatScope::Buffer | FormatScope::Client => true,
            FormatScope::Session => {
                !self.session_id.is_empty()
                    || !self.session_name.is_empty()
                    || self.active_window_index.is_some()
            }
            FormatScope::Window => {
                !self.window_id.is_empty()
                    || !self.window_name.is_empty()
                    || self.window_width.is_some()
            }
            FormatScope::Pane | FormatScope::Terminal => {
                !self.pane_id.is_empty() || !self.pane_title.is_empty() || self.pane_width.is_some()
            }
        };
        if !available {
            return Cow::Borrowed("");
        }
        match spec.backing {
            FormatBacking::Empty => Cow::Borrowed(""),
            FormatBacking::Zero => Cow::Borrowed("0"),
            FormatBacking::ActiveWindowIndex => optional_display(self.active_window_index),
            FormatBacking::HistoryLimit => optional_display(self.history_limit),
            FormatBacking::Host => Cow::Borrowed(self.host.as_str()),
            FormatBacking::HostShort => Cow::Borrowed(self.host_short.as_str()),
            FormatBacking::LastWindowIndex => optional_display(self.last_window_index),
            FormatBacking::NextSessionId => Cow::Borrowed(self.next_session_id.as_str()),
            FormatBacking::PaneActive => optional_bool(self.pane_active),
            FormatBacking::PaneAtBottom => optional_bool(self.pane_at_bottom),
            FormatBacking::PaneAtLeft => optional_bool(self.pane_at_left),
            FormatBacking::PaneAtRight => optional_bool(self.pane_at_right),
            FormatBacking::PaneAtTop => optional_bool(self.pane_at_top),
            FormatBacking::PaneBottom => optional_display(self.pane_bottom),
            FormatBacking::PaneCurrentPath => Cow::Borrowed(self.pane_current_path.as_str()),
            FormatBacking::PaneFlags => Cow::Borrowed(self.pane_flags.as_str()),
            FormatBacking::PaneFormat => {
                Cow::Borrowed(bool_string(format_type == FormatType::Pane))
            }
            FormatBacking::PaneHeight => optional_display(self.pane_height),
            FormatBacking::PaneId => Cow::Borrowed(self.pane_id.as_str()),
            FormatBacking::PaneIndex => Cow::Owned(self.pane_index.to_string()),
            FormatBacking::PaneLast => optional_bool(self.pane_last),
            FormatBacking::PaneLeft => optional_display(self.pane_left),
            FormatBacking::PaneRight => optional_display(self.pane_right),
            FormatBacking::PaneSynchronized => Cow::Borrowed(bool_string(self.pane_synchronized)),
            FormatBacking::PaneTitle => Cow::Borrowed(self.pane_title.as_str()),
            FormatBacking::PaneTop => optional_display(self.pane_top),
            FormatBacking::PaneWidth => optional_display(self.pane_width),
            FormatBacking::PaneX => optional_display(self.pane_x),
            FormatBacking::PaneY => optional_display(self.pane_y),
            FormatBacking::PaneZ => optional_display(self.pane_z),
            FormatBacking::PaneZoomed => Cow::Borrowed(bool_string(self.pane_zoomed)),
            FormatBacking::Pid => Cow::Owned(self.pid.to_string()),
            FormatBacking::ServerSessions => Cow::Owned(self.server_sessions.to_string()),
            FormatBacking::SessionActive => optional_bool(self.session_active),
            FormatBacking::SessionAlert => Cow::Borrowed(self.session_alert.as_str()),
            FormatBacking::SessionAlerts => Cow::Borrowed(self.session_alerts.as_str()),
            FormatBacking::SessionAttached => Cow::Owned(self.session_attached.to_string()),
            FormatBacking::SessionAttachedList => {
                Cow::Borrowed(self.session_attached_list.as_str())
            }
            FormatBacking::SessionBell => Cow::Borrowed(bool_string(self.session_bell)),
            FormatBacking::SessionFormat => {
                Cow::Borrowed(bool_string(format_type == FormatType::Session))
            }
            FormatBacking::SessionId => Cow::Borrowed(self.session_id.as_str()),
            FormatBacking::SessionManyAttached => {
                Cow::Borrowed(bool_string(self.session_many_attached))
            }
            FormatBacking::SessionName => Cow::Borrowed(self.session_name.as_str()),
            FormatBacking::SessionStack => Cow::Borrowed(self.session_stack.as_str()),
            FormatBacking::SessionWindows => Cow::Owned(self.session_windows.to_string()),
            FormatBacking::SocketPath => Cow::Borrowed(self.socket_path.as_str()),
            FormatBacking::StartTime => optional_display(self.start_time),
            FormatBacking::Uid => Cow::Borrowed(self.uid.as_str()),
            FormatBacking::User => Cow::Borrowed(self.user.as_str()),
            FormatBacking::Version => Cow::Borrowed(self.version.as_str()),
            FormatBacking::WindowActive => optional_bool(self.window_active),
            FormatBacking::WindowActiveClients => {
                Cow::Owned(self.window_active_clients.to_string())
            }
            FormatBacking::WindowActiveClientsList => {
                Cow::Borrowed(self.window_active_clients_list.as_str())
            }
            FormatBacking::WindowActiveSessions => {
                Cow::Owned(self.window_active_sessions.to_string())
            }
            FormatBacking::WindowActiveSessionsList => {
                Cow::Borrowed(self.window_active_sessions_list.as_str())
            }
            FormatBacking::WindowBell => Cow::Borrowed(bool_string(self.window_bell)),
            FormatBacking::WindowEnd => optional_bool(self.window_end),
            FormatBacking::WindowFlags => Cow::Owned(self.window_flags(true)),
            FormatBacking::WindowFormat => {
                Cow::Borrowed(bool_string(format_type == FormatType::Window))
            }
            FormatBacking::WindowHeight => optional_display(self.window_height),
            FormatBacking::WindowId => Cow::Borrowed(self.window_id.as_str()),
            FormatBacking::WindowIndex => Cow::Owned(self.window_index.to_string()),
            FormatBacking::WindowLast => optional_bool(self.window_last),
            FormatBacking::WindowLayout => Cow::Borrowed(self.window_layout.as_str()),
            FormatBacking::WindowLinked => Cow::Borrowed(bool_string(false)),
            FormatBacking::WindowLinkedSessions => {
                Cow::Owned(self.window_linked_sessions.to_string())
            }
            FormatBacking::WindowLinkedSessionsList => {
                Cow::Borrowed(self.window_linked_sessions_list.as_str())
            }
            FormatBacking::WindowName => Cow::Borrowed(self.window_name.as_str()),
            FormatBacking::WindowPanes => Cow::Owned(self.window_panes.to_string()),
            FormatBacking::WindowRawFlags => Cow::Owned(self.window_flags(false)),
            FormatBacking::WindowStackIndex => Cow::Owned(self.window_stack_index.to_string()),
            FormatBacking::WindowStart => optional_bool(self.window_start),
            FormatBacking::WindowVisibleLayout => {
                Cow::Borrowed(self.window_visible_layout.as_str())
            }
            FormatBacking::WindowWidth => optional_display(self.window_width),
            FormatBacking::WindowZoomed => Cow::Borrowed(bool_string(self.window_zoomed)),
        }
    }

    fn window_flags(&self, escape: bool) -> String {
        let mut flags = String::new();
        if self.window_bell {
            flags.push('!');
        }
        if self.window_active.unwrap_or(false) {
            flags.push('*');
        }
        if self.window_last.unwrap_or(false) {
            flags.push('-');
        }
        if self.window_zoomed {
            flags.push('Z');
        }
        if escape {
            flags.replace('#', "##")
        } else {
            flags
        }
    }
}

impl FormatVariables for StatusContext {
    fn variable(&self, name: &str) -> Option<Cow<'_, str>> {
        StatusContext::variable(self, name)
    }

    fn variable_kind(&self, name: &str) -> Option<FormatKind> {
        format_variable(name).map(|spec| spec.kind)
    }
}

impl FormatVariables for ResolvedFormatContext {
    fn variable(&self, name: &str) -> Option<Cow<'_, str>> {
        let spec = format_variable(name)?;
        let available = match spec.scope {
            FormatScope::Server | FormatScope::Buffer | FormatScope::Client => true,
            FormatScope::Session => self.has_session,
            FormatScope::Window => self.has_window,
            FormatScope::Pane | FormatScope::Terminal => self.has_pane,
        };
        if available {
            Some(self.values.resolve(spec, self.format_type))
        } else {
            Some(Cow::Borrowed(""))
        }
    }

    fn variable_kind(&self, name: &str) -> Option<FormatKind> {
        format_variable(name).map(|spec| spec.kind)
    }
}

impl FormatContext {
    fn resolve(self, engine: &MuxEngine) -> ResolvedFormatContext {
        let state = &engine.state;
        let pane = self
            .pane
            .filter(|pane| state.window_for_pane(*pane).is_some());
        let window = pane
            .and_then(|pane| state.window_for_pane(pane))
            .or_else(|| {
                self.window
                    .filter(|window| state.windows.contains_key(window))
            });
        let session = window
            .and_then(|window| state.windows.get(&window).map(|window| window.session))
            .or_else(|| {
                self.session
                    .filter(|session| state.sessions.contains_key(session))
            });
        let window = window.or_else(|| {
            session
                .and_then(|session| state.sessions.get(&session))
                .map(|session| session.active_window)
                .filter(|window| state.windows.contains_key(window))
        });
        let pane = pane.or_else(|| {
            window
                .and_then(|window| state.windows.get(&window))
                .map(|window| window.active_pane)
        });
        ResolvedFormatContext {
            values: engine.build_status_context(session, window, pane, self.active_session),
            has_session: session.is_some(),
            has_window: window.is_some(),
            has_pane: pane.is_some(),
            format_type: self.format_type,
        }
    }
}

impl MuxEngine {
    #[must_use]
    pub fn format_status_context(
        &self,
        session: Option<SessionId>,
        window: Option<WindowId>,
        pane: Option<PaneId>,
    ) -> StatusContext {
        FormatContext {
            session,
            window,
            pane,
            active_session: session,
            format_type: FormatType::None,
        }
        .resolve(self)
        .values
    }

    fn build_status_context(
        &self,
        session_id: Option<SessionId>,
        window_id: Option<WindowId>,
        pane_id: Option<PaneId>,
        active_session: Option<SessionId>,
    ) -> StatusContext {
        let mut context = StatusContext {
            host: self.format_host().to_owned(),
            host_short: self.format_host_short().to_owned(),
            next_session_id: self.state.next_session_id().to_string(),
            pid: std::process::id(),
            server_sessions: self.state.sessions.len(),
            socket_path: self.format_socket_path().to_owned(),
            start_time: i64::try_from(self.format_start_time()).ok(),
            uid: current_uid(),
            user: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_default(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ..StatusContext::default()
        };
        let Some(session) = session_id.and_then(|id| self.state.sessions.get(&id)) else {
            return context;
        };
        context.session_id = session.id.to_string();
        context.session_name.clone_from(&session.name);
        context.session_windows = session.windows.len();
        context.session_active = active_session.map(|active| active == session.id);
        context.active_window_index = self
            .state
            .windows
            .get(&session.active_window)
            .map(|window| window.index);
        context.last_window_index = session
            .windows
            .iter()
            .filter_map(|window| self.state.windows.get(window).map(|window| window.index))
            .max();
        let bell_windows = session
            .windows
            .iter()
            .filter_map(|window| self.state.windows.get(window))
            .filter(|window| window.panes.values().any(|pane| pane.bell))
            .collect::<Vec<_>>();
        context.session_bell = session
            .windows
            .first()
            .and_then(|window| self.state.windows.get(window))
            .is_some_and(|window| window.panes.values().any(|pane| pane.bell));
        if !bell_windows.is_empty() {
            context.session_alert.push('!');
        }
        context.session_alerts = bell_windows
            .iter()
            .map(|window| format!("{}!", window.index))
            .collect::<Vec<_>>()
            .join(",");
        let mut stack = context
            .active_window_index
            .map(|index| index.to_string())
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(last) = session
            .last_window()
            .and_then(|window| self.state.windows.get(&window))
        {
            stack.push(last.index.to_string());
        }
        context.session_stack = stack.join(",");
        let Some(window) = window_id.and_then(|id| self.state.windows.get(&id)) else {
            return context;
        };
        context.window_id = window.id.to_string();
        context.window_index = window.index;
        context.window_name.clone_from(&window.name);
        context.window_panes = window.panes.len();
        let (width, height) = window.layout.extent();
        context.window_width = Some(width);
        context.window_height = Some(height);
        context.window_layout = window.layout.dump();
        context.window_active = Some(session.active_window == window.id);
        context.window_zoomed = window.zoomed_pane.is_some();
        context.window_visible_layout = window.zoomed_pane.map_or_else(
            || context.window_layout.clone(),
            |pane| CellLayout::new(pane, width, height).dump(),
        );
        context.window_bell = window.panes.values().any(|pane| pane.bell);
        context.window_last = Some(session.last_window() == Some(window.id));
        context.window_start = Some(session.windows.first() == Some(&window.id));
        context.window_end = Some(session.windows.last() == Some(&window.id));
        context.window_stack_index = usize::from(context.window_last == Some(true));
        context.window_linked_sessions = 1;
        context
            .window_linked_sessions_list
            .clone_from(&session.name);
        if context.window_active == Some(true) {
            context.window_active_sessions = 1;
            context
                .window_active_sessions_list
                .clone_from(&session.name);
        }
        let Some(pane) = pane_id.and_then(|id| window.panes.get(&id)) else {
            return context;
        };
        context.history_limit = self.history_limit_for_pane(pane.id).ok();
        context.pane_active = Some(window.active_pane == pane.id);
        context.pane_id = pane.id.to_string();
        context.pane_index = self.pane_index(window.id, pane.id).unwrap_or_default();
        context.pane_last = Some(window.last_pane() == Some(pane.id));
        context.pane_synchronized = self
            .state
            .pane_synchronize_panes(pane.id)
            .unwrap_or_default();
        context.pane_title.clone_from(&pane.title);
        context.pane_zoomed = window.zoomed_pane == Some(pane.id);
        context.pane_current_path = match &pane.kind {
            PaneKind::Agent(agent) => agent
                .cwd
                .as_deref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            PaneKind::Editor(editor) => editor.cwd.clone(),
            PaneKind::Picker { .. } | PaneKind::Terminal | PaneKind::Browser(_) => String::new(),
        };
        let geometry = if context.pane_zoomed {
            let (sx, sy) = window.layout.extent();
            Some((sx, sy, 0, 0))
        } else {
            window
                .layout
                .pane_geometry(pane.id)
                .map(|geometry| (geometry.sx, geometry.sy, geometry.xoff, geometry.yoff))
        };
        if let Some((sx, sy, xoff, yoff)) = geometry {
            context.pane_width = Some(sx);
            context.pane_height = Some(sy);
            context.pane_left = Some(xoff);
            context.pane_top = Some(yoff);
            context.pane_x = Some(xoff);
            context.pane_y = Some(yoff);
            context.pane_right = xoff.checked_add(sx).and_then(|right| right.checked_sub(1));
            context.pane_bottom = yoff
                .checked_add(sy)
                .and_then(|bottom| bottom.checked_sub(1));
            context.pane_at_left = Some(xoff == 0);
            context.pane_at_top = Some(yoff == 0);
            context.pane_at_right = Some(xoff.saturating_add(sx) == width);
            context.pane_at_bottom = Some(yoff.saturating_add(sy) == height);
        }
        context.pane_z = Some(1);
        if context.pane_active == Some(true) {
            context.pane_flags.push('*');
        }
        if context.pane_last == Some(true) {
            context.pane_flags.push('-');
        }
        if context.pane_zoomed {
            context.pane_flags.push('Z');
        }
        context
    }
}

fn format_variable(name: &str) -> Option<&'static FormatVariableSpec> {
    FORMAT_VARIABLES
        .binary_search_by_key(&name, |variable| variable.name)
        .ok()
        .map(|index| &FORMAT_VARIABLES[index])
}

fn optional_display<T: ToString>(value: Option<T>) -> Cow<'static, str> {
    value.map_or(Cow::Borrowed(""), |value| Cow::Owned(value.to_string()))
}

fn optional_bool(value: Option<bool>) -> Cow<'static, str> {
    value.map_or(Cow::Borrowed(""), |value| Cow::Borrowed(bool_string(value)))
}

const fn bool_string(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn current_uid() -> String {
    #[cfg(unix)]
    {
        rustix::process::getuid().as_raw().to_string()
    }
    #[cfg(not(unix))]
    {
        String::new()
    }
}

pub trait StatusHooks {
    fn strftime(&mut self, literal: &str) -> String;
    fn shell(&mut self, command: &str) -> String;
}

pub fn expand_status(
    format: &str,
    context: &StatusContext,
    hooks: &mut impl StatusHooks,
) -> String {
    let mut expander = Expander { context, hooks };
    truncate_output(expander.expand(format, 0, true))
}

pub(crate) fn expand_format(format: &str, engine: &MuxEngine, context: FormatContext) -> String {
    struct CommandHooks;

    impl StatusHooks for CommandHooks {
        fn strftime(&mut self, literal: &str) -> String {
            strftime_now(literal)
        }

        fn shell(&mut self, _command: &str) -> String {
            String::new()
        }
    }

    let context = context.resolve(engine);
    let mut hooks = CommandHooks;
    let mut expander = Expander {
        context: &context,
        hooks: &mut hooks,
    };
    truncate_output(expander.expand(format, 0, false))
}

struct Expander<'a, V: FormatVariables + ?Sized, H: StatusHooks> {
    context: &'a V,
    hooks: &'a mut H,
}

impl<V: FormatVariables + ?Sized, H: StatusHooks> Expander<'_, V, H> {
    fn expand(&mut self, format: &str, depth: usize, literal_time: bool) -> String {
        if depth == FORMAT_LOOP_LIMIT || format.is_empty() {
            return String::new();
        }
        let mut output = String::with_capacity(format.len());
        let mut literal = String::new();
        let mut index = 0usize;
        while index < format.len() {
            let Some(character) = format[index..].chars().next() else {
                break;
            };
            if character != '#' {
                literal.push(character);
                index += character.len_utf8();
                continue;
            }
            let next_index = index + 1;
            let Some(next) = format[next_index..].chars().next() else {
                break;
            };
            let after_next = next_index + next.len_utf8();
            match next {
                '#' | '}' | ',' => {
                    literal.push(next);
                    index = after_next;
                }
                '[' => {
                    if let Some(end) = find_plain_group_end(format, after_next, '[', ']') {
                        index = end + 1;
                    } else {
                        break;
                    }
                }
                '(' => {
                    let Some(end) = find_plain_group_end(format, after_next, '(', ')') else {
                        break;
                    };
                    self.flush(&mut output, &mut literal, literal_time);
                    output.push_str(&self.hooks.shell(&format[after_next..end]));
                    index = end + 1;
                }
                '{' => {
                    let Some(end) = find_format_end(format, index) else {
                        break;
                    };
                    self.flush(&mut output, &mut literal, literal_time);
                    match self.expand_replacement(&format[after_next..end], depth + 1) {
                        Ok(value) => output.push_str(&value),
                        Err(()) => break,
                    }
                    index = end + 1;
                }
                _ => {
                    if let Some(name) = shorthand(next) {
                        self.flush(&mut output, &mut literal, literal_time);
                        output.push_str(&self.context.variable(name).unwrap_or_default());
                    } else {
                        literal.push('#');
                        literal.push(next);
                    }
                    index = after_next;
                }
            }
        }
        self.flush(&mut output, &mut literal, literal_time);
        output
    }

    fn flush(&mut self, output: &mut String, literal: &mut String, literal_time: bool) {
        if literal.is_empty() {
            return;
        }
        if literal_time {
            output.push_str(&self.hooks.strftime(literal));
        } else {
            output.push_str(literal);
        }
        literal.clear();
    }

    fn expand_replacement(&mut self, body: &str, depth: usize) -> Result<String, ()> {
        let (modifiers, copy) = self.build_modifiers(body, depth).map_or_else(
            || (Vec::new(), body),
            |(modifiers, offset)| (modifiers, &body[offset..]),
        );
        let flags = ModifierFlags::from_modifiers(&modifiers);
        let mut value = if flags.literal {
            unescape(copy)
        } else if flags.not {
            bool_string(!format_true(&self.expand(copy, depth, false))).to_owned()
        } else if flags.not_not {
            bool_string(format_true(&self.expand(copy, depth, false))).to_owned()
        } else if let Some(and) = flags.bool_op {
            self.expand_boolean(copy, depth, and)
        } else if let Some(comparison) = flags.comparison {
            self.expand_comparison(copy, depth, comparison)?
        } else if let Some(conditional) = copy.strip_prefix('?') {
            self.expand_conditional(conditional, depth, &flags)
        } else if copy.contains("#{") {
            self.expand(copy, depth, false)
        } else {
            self.lookup(copy, &flags).unwrap_or_default()
        };

        if flags.expand {
            value = self.expand(&value, depth, false);
        } else if flags.expand_time {
            value = self.hooks.strftime(&value);
            value = self.expand(&value, depth, false);
        }
        for substitution in flags.substitutions {
            let pattern = self.expand(&substitution.args[0], depth, false);
            let replacement = self.expand(&substitution.args[1], depth, false);
            value = substitute(
                &value,
                &pattern,
                &replacement,
                substitution
                    .args
                    .get(2)
                    .is_some_and(|flags| flags.contains('i')),
            );
        }
        if let Some((limit, marker)) = flags.limit {
            value = truncate_value(&value, limit, marker.as_deref());
        }
        if flags.length {
            value = value.len().to_string();
        }
        Ok(value)
    }

    fn build_modifiers(&mut self, body: &str, depth: usize) -> Option<(Vec<Modifier>, usize)> {
        let mut modifiers = Vec::new();
        let mut position = 0usize;
        while position < body.len() && body.as_bytes()[position] != b':' {
            if body.as_bytes()[position] == b';' {
                position += 1;
            }
            if position >= body.len() {
                return None;
            }
            let remaining = &body[position..];
            let mut matched = None;
            for (text, kind) in [
                ("||", ModifierKind::Or),
                ("&&", ModifierKind::And),
                ("!!", ModifierKind::NotNot),
                ("!=", ModifierKind::NotEqual),
                ("==", ModifierKind::Equal),
                ("<=", ModifierKind::LessEqual),
                (">=", ModifierKind::GreaterEqual),
            ] {
                if remaining.starts_with(text)
                    && remaining
                        .as_bytes()
                        .get(text.len())
                        .is_some_and(|next| is_modifier_end(*next))
                {
                    matched = Some((kind, text.len()));
                    break;
                }
            }
            if let Some((kind, size)) = matched {
                modifiers.push(Modifier {
                    kind,
                    args: Vec::new(),
                });
                position += size;
                continue;
            }
            let character = remaining.as_bytes()[0] as char;
            let next = remaining.as_bytes().get(1).copied();
            let no_arguments = match character {
                'l' => Some(ModifierKind::Literal),
                'b' => Some(ModifierKind::Basename),
                'd' => Some(ModifierKind::Dirname),
                'n' => Some(ModifierKind::Length),
                'E' => Some(ModifierKind::Expand),
                'T' => Some(ModifierKind::ExpandTime),
                '!' => Some(ModifierKind::Not),
                '<' => Some(ModifierKind::Less),
                '>' => Some(ModifierKind::Greater),
                _ => None,
            };
            if let Some(kind) = no_arguments
                && next.is_some_and(is_modifier_end)
            {
                modifiers.push(Modifier {
                    kind,
                    args: Vec::new(),
                });
                position += 1;
                continue;
            }
            let kind = match character {
                'm' => ModifierKind::Match,
                's' => ModifierKind::Substitute,
                't' => ModifierKind::Time,
                '=' => ModifierKind::Limit,
                'q' => ModifierKind::Quote,
                _ => return None,
            };
            position += 1;
            if position >= body.len() {
                return None;
            }
            if is_modifier_end(body.as_bytes()[position]) {
                modifiers.push(Modifier {
                    kind,
                    args: Vec::new(),
                });
                continue;
            }
            let wrapper = body.as_bytes()[position];
            let mut args = Vec::new();
            if wrapper.is_ascii_punctuation() && wrapper != b'-' {
                loop {
                    if body.as_bytes().get(position) == Some(&wrapper)
                        && body
                            .as_bytes()
                            .get(position + 1)
                            .is_some_and(|next| is_modifier_end(*next))
                    {
                        position += 1;
                        break;
                    }
                    let start = position + 1;
                    let end = find_modifier_argument(body, start, wrapper)?;
                    let value = unescape(&body[start..end]);
                    args.push(self.expand(&value, depth, false));
                    position = end;
                    if is_modifier_end(body.as_bytes()[position]) {
                        break;
                    }
                }
            } else {
                let end = find_modifier_argument(body, position, 0)?;
                let value = unescape(&body[position..end]);
                args.push(self.expand(&value, depth, false));
                position = end;
            }
            modifiers.push(Modifier { kind, args });
        }
        if body.as_bytes().get(position) == Some(&b':') {
            Some((modifiers, position + 1))
        } else {
            None
        }
    }

    fn expand_boolean(&mut self, copy: &str, depth: usize, and: bool) -> String {
        let mut result = and;
        let mut rest = copy;
        loop {
            let (operand, next) = split_once_top(rest, ',');
            let truth = format_true(&self.expand(operand, depth, false));
            result = if and {
                result && truth
            } else {
                result || truth
            };
            if result != and || next.is_none() {
                break;
            }
            rest = next.unwrap_or_default();
        }
        bool_string(result).to_owned()
    }

    fn expand_comparison(
        &mut self,
        copy: &str,
        depth: usize,
        comparison: Comparison,
    ) -> Result<String, ()> {
        let (left, right) = split_once_top(copy, ',');
        let Some(right) = right else {
            return Err(());
        };
        let left = self.expand(left, depth, false);
        let right = self.expand(right, depth, false);
        let value = match comparison {
            Comparison::Equal => bool_string(left == right).to_owned(),
            Comparison::NotEqual => bool_string(left != right).to_owned(),
            Comparison::Less => bool_string(left.cmp(&right) == Ordering::Less).to_owned(),
            Comparison::Greater => bool_string(left.cmp(&right) == Ordering::Greater).to_owned(),
            Comparison::LessEqual => bool_string(left.cmp(&right) != Ordering::Greater).to_owned(),
            Comparison::GreaterEqual => bool_string(left.cmp(&right) != Ordering::Less).to_owned(),
            Comparison::Match(flags) => match_value(&left, &right, flags),
        };
        Ok(value)
    }

    fn expand_conditional(
        &mut self,
        copy: &str,
        depth: usize,
        flags: &ModifierFlags<'_>,
    ) -> String {
        let parts = split_top(copy, ',');
        let paired = parts.len() / 2 * 2;
        for pair in parts[..paired].chunks_exact(2) {
            let condition = pair[0];
            let found = self.lookup(condition, flags).unwrap_or_else(|| {
                let expanded = self.expand(condition, depth, false);
                if expanded == condition {
                    String::new()
                } else {
                    expanded
                }
            });
            if format_true(&found) {
                return self.expand(pair[1], depth, false);
            }
        }
        if parts.len() % 2 == 1 {
            self.expand(parts[parts.len() - 1], depth, false)
        } else {
            String::new()
        }
    }

    fn lookup(&mut self, key: &str, flags: &ModifierFlags<'_>) -> Option<String> {
        let _kind = self.context.variable_kind(key)?;
        let mut value = self.context.variable(key)?.into_owned();
        if flags.time.enabled {
            return Some(format_time_value(&value, &flags.time));
        }
        if flags.basename {
            value = basename(&value);
        }
        if flags.dirname {
            value = dirname(&value);
        }
        if flags.quote_shell {
            value = quote_shell(&value);
        }
        if flags.quote_single {
            value = quote_single(&value);
        }
        if flags.quote_style {
            value = value.replace('#', "##");
        }
        if flags.quote_arguments {
            value = quote_argument(&value);
        }
        Some(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModifierKind {
    Literal,
    Basename,
    Dirname,
    Length,
    Expand,
    ExpandTime,
    Not,
    NotNot,
    And,
    Or,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Match,
    Time,
    Quote,
    Substitute,
    Limit,
}

struct Modifier {
    kind: ModifierKind,
    args: Vec<String>,
}

#[derive(Clone, Copy)]
enum Comparison<'a> {
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Match(&'a str),
}

#[derive(Default)]
struct TimeFlags<'a> {
    enabled: bool,
    pretty: bool,
    relative: bool,
    difference: bool,
    format: Option<&'a str>,
}

#[derive(Default)]
struct ModifierFlags<'a> {
    literal: bool,
    basename: bool,
    dirname: bool,
    length: bool,
    expand: bool,
    expand_time: bool,
    not: bool,
    not_not: bool,
    bool_op: Option<bool>,
    comparison: Option<Comparison<'a>>,
    substitutions: Vec<&'a Modifier>,
    limit: Option<(isize, Option<String>)>,
    time: TimeFlags<'a>,
    quote_shell: bool,
    quote_single: bool,
    quote_style: bool,
    quote_arguments: bool,
}

impl<'a> ModifierFlags<'a> {
    fn from_modifiers(modifiers: &'a [Modifier]) -> Self {
        let mut flags = Self::default();
        for modifier in modifiers {
            match modifier.kind {
                ModifierKind::Literal => flags.literal = true,
                ModifierKind::Basename => flags.basename = true,
                ModifierKind::Dirname => flags.dirname = true,
                ModifierKind::Length => flags.length = true,
                ModifierKind::Expand => flags.expand = true,
                ModifierKind::ExpandTime => flags.expand_time = true,
                ModifierKind::Not => flags.not = true,
                ModifierKind::NotNot => flags.not_not = true,
                ModifierKind::And => flags.bool_op = Some(true),
                ModifierKind::Or => flags.bool_op = Some(false),
                ModifierKind::Equal => flags.comparison = Some(Comparison::Equal),
                ModifierKind::NotEqual => flags.comparison = Some(Comparison::NotEqual),
                ModifierKind::Less => flags.comparison = Some(Comparison::Less),
                ModifierKind::Greater => flags.comparison = Some(Comparison::Greater),
                ModifierKind::LessEqual => flags.comparison = Some(Comparison::LessEqual),
                ModifierKind::GreaterEqual => flags.comparison = Some(Comparison::GreaterEqual),
                ModifierKind::Match => {
                    flags.comparison = Some(Comparison::Match(
                        modifier.args.first().map_or("", String::as_str),
                    ));
                }
                ModifierKind::Time => {
                    flags.time.enabled = true;
                    if let Some(time_flags) = modifier.args.first() {
                        if time_flags.contains('p') {
                            flags.time.pretty = true;
                        } else if time_flags.contains('r') {
                            flags.time.relative = true;
                        } else if time_flags.contains('d') {
                            flags.time.difference = true;
                        } else if time_flags.contains('f')
                            && let Some(format) = modifier.args.get(1)
                        {
                            flags.time.format = Some(format.as_str());
                        }
                    }
                }
                ModifierKind::Quote => {
                    let Some(quote_flags) = modifier.args.first() else {
                        flags.quote_shell = true;
                        continue;
                    };
                    if quote_flags.contains('s') {
                        flags.quote_single = true;
                    } else if quote_flags.contains('e') || quote_flags.contains('h') {
                        flags.quote_style = true;
                    } else if quote_flags.contains('a') {
                        flags.quote_arguments = true;
                    }
                }
                ModifierKind::Substitute if modifier.args.len() >= 2 => {
                    flags.substitutions.push(modifier);
                }
                ModifierKind::Substitute => {}
                ModifierKind::Limit => {
                    if let Some(limit) = modifier.args.first() {
                        let limit = limit
                            .parse::<isize>()
                            .ok()
                            .filter(|limit| (-FORMAT_MAX_WIDTH..=FORMAT_MAX_WIDTH).contains(limit))
                            .unwrap_or_default();
                        let marker = modifier.args.get(1).cloned().or_else(|| {
                            flags.limit.as_ref().and_then(|(_, marker)| marker.clone())
                        });
                        flags.limit = Some((limit, marker));
                    }
                }
            }
        }
        flags
    }
}

fn shorthand(character: char) -> Option<&'static str> {
    Some(match character {
        'D' => "pane_id",
        'F' => "window_flags",
        'H' => "host",
        'I' => "window_index",
        'P' => "pane_index",
        'S' => "session_name",
        'T' => "pane_title",
        'W' => "window_name",
        'h' => "host_short",
        _ => return None,
    })
}

const fn is_modifier_end(character: u8) -> bool {
    character == b';' || character == b':'
}

fn find_plain_group_end(text: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 1usize;
    for (offset, character) in text[start..].char_indices() {
        if character == open {
            depth += 1;
        } else if character == close {
            depth -= 1;
            if depth == 0 {
                return Some(start + offset);
            }
        }
    }
    None
}

fn find_format_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut position = start;
    while position < bytes.len() {
        if bytes[position] == b'#' && bytes.get(position + 1) == Some(&b'{') {
            depth += 1;
            position += 2;
            continue;
        }
        if bytes[position] == b'#'
            && bytes
                .get(position + 1)
                .is_some_and(|next| matches!(*next, b',' | b'#' | b'{' | b'}' | b':'))
        {
            position += 2;
            continue;
        }
        if bytes[position] == b'}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(position);
            }
        }
        position += 1;
    }
    None
}

fn find_modifier_argument(text: &str, start: usize, wrapper: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut position = start;
    while position < bytes.len() {
        if bytes[position] == b'#' && bytes.get(position + 1) == Some(&b'{') {
            depth += 1;
            position += 2;
            continue;
        }
        if bytes[position] == b'#'
            && bytes
                .get(position + 1)
                .is_some_and(|next| matches!(*next, b',' | b'#' | b'{' | b'}' | b':'))
        {
            position += 2;
            continue;
        }
        if bytes[position] == b'}' && depth > 0 {
            depth -= 1;
            position += 1;
            continue;
        }
        if depth == 0
            && (is_modifier_end(bytes[position]) || (wrapper != 0 && bytes[position] == wrapper))
        {
            return Some(position);
        }
        position += 1;
    }
    None
}

fn split_once_top(text: &str, separator: char) -> (&str, Option<&str>) {
    let bytes = text.as_bytes();
    let separator = separator as u8;
    let mut depth = 0usize;
    let mut position = 0usize;
    while position < bytes.len() {
        if bytes[position] == b'#' && bytes.get(position + 1) == Some(&b'{') {
            depth += 1;
            position += 2;
            continue;
        }
        if bytes[position] == b'#'
            && bytes
                .get(position + 1)
                .is_some_and(|next| matches!(*next, b',' | b'#' | b'{' | b'}' | b':'))
        {
            position += 2;
            continue;
        }
        if bytes[position] == b'}' && depth > 0 {
            depth -= 1;
        } else if bytes[position] == separator && depth == 0 {
            return (&text[..position], Some(&text[position + 1..]));
        }
        position += 1;
    }
    (text, None)
}

fn split_top(text: &str, separator: char) -> Vec<&str> {
    let mut values = Vec::new();
    let mut rest = text;
    loop {
        let (value, next) = split_once_top(rest, separator);
        values.push(value);
        let Some(next) = next else {
            break;
        };
        rest = next;
    }
    values
}

fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut depth = 0usize;
    let mut position = 0usize;
    while position < bytes.len() {
        if bytes[position] == b'#' && bytes.get(position + 1) == Some(&b'{') {
            depth += 1;
        }
        if depth == 0
            && bytes[position] == b'#'
            && bytes
                .get(position + 1)
                .is_some_and(|next| matches!(*next, b',' | b'#' | b'{' | b'}' | b':'))
        {
            output.push(bytes[position + 1] as char);
            position += 2;
            continue;
        }
        if bytes[position] == b'}' {
            depth = depth.saturating_sub(1);
        }
        let character = text[position..].chars().next().unwrap_or_default();
        output.push(character);
        position += character.len_utf8();
    }
    output
}

fn format_true(value: &str) -> bool {
    !value.is_empty() && value != "0"
}

fn basename(value: &str) -> String {
    if value.is_empty() {
        return ".".to_owned();
    }
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_owned();
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_owned()
}

fn dirname(value: &str) -> String {
    if value.is_empty() {
        return ".".to_owned();
    }
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_owned();
    }
    let Some(index) = trimmed.rfind('/') else {
        return ".".to_owned();
    };
    let parent = trimmed[..index].trim_end_matches('/');
    if parent.is_empty() {
        "/".to_owned()
    } else {
        parent.to_owned()
    }
}

fn quote_shell(value: &str) -> String {
    let mut output = String::with_capacity(value.len() * 2);
    for character in value.chars() {
        if "|&;<>()$`\\\"'*?[# =%".contains(character) {
            output.push('\\');
        }
        output.push(character);
    }
    output
}

fn quote_single(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('\'');
    for character in value.chars() {
        if character == '\'' {
            output.push_str("'\\''");
        } else {
            output.push(character);
        }
    }
    output.push('\'');
    output
}

fn quote_argument(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    let double = value
        .chars()
        .any(|character| " #';${}%".contains(character));
    let single = !double && value.chars().any(|character| " \"".contains(character));
    if value != " " && value.len() == 1 && (double || single || value == "~") {
        return format!("\\{value}");
    }
    let mut escaped = String::with_capacity(value.len() * 2);
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\u{8}' => escaped.push_str("\\b"),
            '\u{7}' => escaped.push_str("\\a"),
            '\u{b}' => escaped.push_str("\\v"),
            '\t' => escaped.push_str("\\t"),
            '\u{c}' => escaped.push_str("\\f"),
            '\0' => {
                escaped.push_str("\\0");
                if characters
                    .peek()
                    .is_some_and(|next| matches!(next, '0'..='7'))
                {
                    escaped.push_str("00");
                }
            }
            '\\' => escaped.push_str("\\\\"),
            '"' if double => escaped.push_str("\\\""),
            '$' if double
                && characters.peek().is_some_and(|next| {
                    next.is_ascii_alphabetic() || matches!(next, '_' | '{')
                }) =>
            {
                escaped.push_str("\\$");
            }
            character if character.is_ascii_control() => {
                let _ = write!(&mut escaped, "\\{:03o}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    if double {
        if escaped.starts_with('~') {
            format!("\"\\{escaped}\"")
        } else {
            format!("\"{escaped}\"")
        }
    } else if single {
        format!("'{escaped}'")
    } else if escaped.starts_with('~') {
        format!("\\{escaped}")
    } else {
        escaped
    }
}

fn match_value(pattern: &str, text: &str, flags: &str) -> String {
    if flags.contains('p') {
        return fuzzy_positions(pattern, text).unwrap_or_default();
    }
    if flags.contains('z') {
        return bool_string(fuzzy_positions(pattern, text).is_some()).to_owned();
    }
    let matched = if flags.contains('r') {
        RegexBuilder::new(pattern)
            .case_insensitive(flags.contains('i'))
            .dot_matches_new_line(true)
            .build()
            .is_ok_and(|regex| regex.is_match(text))
    } else {
        Pattern::new(pattern).is_ok_and(|pattern| {
            pattern.matches_with(
                text,
                MatchOptions {
                    case_sensitive: !flags.contains('i'),
                    require_literal_separator: false,
                    require_literal_leading_dot: false,
                },
            )
        })
    };
    bool_string(matched).to_owned()
}

#[derive(Clone, Copy)]
struct FuzzyCharacter {
    value: char,
    column: usize,
    width: usize,
}

#[derive(Clone, Copy)]
struct FuzzyTerm<'a> {
    inverse: bool,
    exact: bool,
    prefix: bool,
    suffix: bool,
    text: &'a str,
}

fn fuzzy_positions(pattern: &str, text: &str) -> Option<String> {
    let characters = fuzzy_characters(text);
    if pattern
        .chars()
        .all(|character| matches!(character, ' ' | '|'))
    {
        return Some(String::new());
    }
    let fold = !pattern.bytes().any(|byte| byte.is_ascii_uppercase());
    let mut best: Option<(i32, Vec<bool>)> = None;
    for group in pattern.split('|') {
        let mut any = false;
        let mut score = 0i32;
        let mut selected = vec![false; characters.len()];
        let mut matched_group = true;
        for raw in group.split(' ').filter(|term| !term.is_empty()) {
            let Some(term) = fuzzy_term(raw) else {
                matched_group = false;
                break;
            };
            any = true;
            let found = if term.exact {
                fuzzy_exact(&term, &characters, fold)
            } else {
                fuzzy_subsequence(term.text, &characters, fold)
            };
            if term.inverse {
                if found.is_some() {
                    matched_group = false;
                    break;
                }
                continue;
            }
            let Some((term_score, positions)) = found else {
                matched_group = false;
                break;
            };
            score = score.saturating_add(term_score);
            for position in positions {
                selected[position] = true;
            }
        }
        if !any || !matched_group {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, selected));
        }
    }
    let (_, selected) = best?;
    Some(
        characters
            .iter()
            .zip(selected)
            .filter(|(_, selected)| *selected)
            .flat_map(|(character, _)| character.column..character.column + character.width)
            .map(|column| column.to_string())
            .collect::<Vec<_>>()
            .join(","),
    )
}

fn fuzzy_characters(text: &str) -> Vec<FuzzyCharacter> {
    let mut column = 0usize;
    text.chars()
        .filter_map(|value| {
            if value.is_ascii_control() {
                return None;
            }
            let width = value.width().unwrap_or_default();
            let character = FuzzyCharacter {
                value,
                column,
                width,
            };
            column = column.saturating_add(width);
            Some(character)
        })
        .collect()
}

fn fuzzy_term(raw: &str) -> Option<FuzzyTerm<'_>> {
    let mut text = raw;
    let inverse = text.starts_with('!');
    if inverse {
        text = &text[1..];
    }
    if text.is_empty() {
        return None;
    }
    let mut exact = false;
    let mut prefix = false;
    if text.starts_with('\'') {
        exact = true;
        text = &text[1..];
    } else if text.starts_with('^') {
        exact = true;
        prefix = true;
        text = &text[1..];
    }
    if text.is_empty() {
        return None;
    }
    let suffix = text.ends_with('$');
    if suffix {
        exact = true;
        text = &text[..text.len() - 1];
    }
    if text.is_empty() {
        return None;
    }
    Some(FuzzyTerm {
        inverse,
        exact: exact || inverse,
        prefix,
        suffix,
        text,
    })
}

fn fuzzy_subsequence(
    pattern: &str,
    text: &[FuzzyCharacter],
    fold: bool,
) -> Option<(i32, Vec<usize>)> {
    let pattern = pattern.chars().collect::<Vec<_>>();
    if pattern.is_empty() || text.is_empty() {
        return None;
    }
    let mut positions = Vec::with_capacity(pattern.len());
    let mut text_index = 0usize;
    for wanted in &pattern {
        while text_index < text.len() && !fuzzy_equal(*wanted, text[text_index].value, fold) {
            text_index += 1;
        }
        if text_index == text.len() {
            return None;
        }
        positions.push(text_index);
        text_index += 1;
    }
    let mut candidate = *positions.last()?;
    for pattern_index in (0..pattern.len()).rev() {
        loop {
            if fuzzy_equal(pattern[pattern_index], text[candidate].value, fold) {
                positions[pattern_index] = candidate;
                break;
            }
            candidate = candidate.checked_sub(1)?;
        }
        if pattern_index != 0 {
            candidate = candidate.checked_sub(1)?;
        }
    }
    let score = fuzzy_subsequence_score(&positions, text);
    Some((score, positions))
}

fn fuzzy_exact(
    term: &FuzzyTerm<'_>,
    text: &[FuzzyCharacter],
    fold: bool,
) -> Option<(i32, Vec<usize>)> {
    let pattern = term.text.chars().collect::<Vec<_>>();
    if pattern.is_empty() || pattern.len() > text.len() {
        return None;
    }
    let starts = if term.prefix && term.suffix {
        if pattern.len() != text.len() {
            return None;
        }
        0..1
    } else if term.prefix {
        0..1
    } else if term.suffix {
        let start = text.len() - pattern.len();
        start..start + 1
    } else {
        0..text.len() - pattern.len() + 1
    };
    let mut best: Option<(i32, Vec<usize>)> = None;
    for start in starts {
        if !pattern
            .iter()
            .enumerate()
            .all(|(offset, wanted)| fuzzy_equal(*wanted, text[start + offset].value, fold))
        {
            continue;
        }
        let score = fuzzy_exact_score(start, pattern.len(), text, term.prefix, term.suffix);
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, (start..start + pattern.len()).collect()));
        }
    }
    best
}

fn fuzzy_subsequence_score(positions: &[usize], text: &[FuzzyCharacter]) -> i32 {
    let Some(first) = positions.first().copied() else {
        return 0;
    };
    let mut score = 0i32;
    if first == 0 {
        score += 12;
    } else {
        if fuzzy_boundary(text[first - 1].value) {
            score += 8;
        }
        score -= fuzzy_score_number(first.min(10));
    }
    for pair in positions.windows(2) {
        if pair[1] == pair[0] + 1 {
            score += 6;
        } else if fuzzy_boundary(text[pair[1] - 1].value) {
            score += 8;
        }
    }
    let span = positions[positions.len() - 1] - first + 1;
    score.saturating_sub(fuzzy_score_number(span - positions.len()))
}

fn fuzzy_exact_score(
    start: usize,
    length: usize,
    text: &[FuzzyCharacter],
    prefix: bool,
    suffix: bool,
) -> i32 {
    let mut score = 1_000i32.saturating_add(fuzzy_score_number(length).saturating_mul(6));
    if prefix {
        score = score.saturating_add(200);
    }
    if suffix {
        score = score.saturating_add(100);
    }
    if start == 0 {
        score = score.saturating_add(12);
    } else if fuzzy_boundary(text[start - 1].value) {
        score = score.saturating_add(8);
    }
    score = score.saturating_sub(fuzzy_score_number(start.min(10)));
    if !prefix && !suffix {
        score = score.saturating_sub(fuzzy_score_number(text.len() - (start + length)));
    }
    score
}

fn fuzzy_equal(left: char, right: char, fold: bool) -> bool {
    if fold && left.is_ascii() && right.is_ascii() {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

fn fuzzy_boundary(character: char) -> bool {
    character.is_ascii() && " -_/.:".contains(character)
}

fn fuzzy_score_number(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn substitute(text: &str, pattern: &str, replacement: &str, insensitive: bool) -> String {
    if text.is_empty() || pattern.is_empty() {
        return text.to_owned();
    }
    let Ok(regex) = RegexBuilder::new(pattern)
        .case_insensitive(insensitive)
        .dot_matches_new_line(true)
        .build()
    else {
        return text.to_owned();
    };
    let mut output = String::with_capacity(text.len());
    let mut start = 0usize;
    let mut last = 0usize;
    let mut empty = false;
    while start <= text.len() {
        let Some(captures) = regex.captures(&text[start..]) else {
            output.push_str(&text[start..]);
            break;
        };
        let matched = captures.get(0).expect("a regex capture has a whole match");
        let match_start = start + matched.start();
        let match_end = start + matched.end();
        output.push_str(&text[last..match_start]);
        if pattern.starts_with('^') {
            output.push_str(&expand_substitution(replacement, &captures));
            output.push_str(&text[match_end..]);
            break;
        }
        if empty || match_start != last || !matched.is_empty() {
            output.push_str(&expand_substitution(replacement, &captures));
            last = match_end;
            start = match_end;
            empty = false;
        } else {
            last = match_end;
            let Some(character) = text[match_end..].chars().next() else {
                break;
            };
            start = match_end + character.len_utf8();
            empty = true;
        }
    }
    output
}

fn expand_substitution(replacement: &str, captures: &Captures<'_>) -> String {
    let mut output = String::with_capacity(replacement.len());
    let mut characters = replacement.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        let Some(next) = characters.next() else {
            output.push('\\');
            break;
        };
        if let Some(index) = next.to_digit(10).map(|index| index as usize)
            && let Some(value) = captures.get(index)
            && !value.as_str().is_empty()
        {
            output.push_str(value.as_str());
        } else {
            output.push(next);
        }
    }
    output
}

fn truncate_value(value: &str, limit: isize, marker: Option<&str>) -> String {
    if limit == 0 {
        return value.to_owned();
    }
    let keep = limit.unsigned_abs();
    let trimmed = if limit > 0 {
        let mut width = 0usize;
        value
            .chars()
            .take_while(|character| {
                let next = character.width().unwrap_or_default();
                if width.saturating_add(next) > keep {
                    false
                } else {
                    width += next;
                    true
                }
            })
            .collect::<String>()
    } else {
        let mut width = 0usize;
        let mut characters = value
            .chars()
            .rev()
            .take_while(|character| {
                let next = character.width().unwrap_or_default();
                if width.saturating_add(next) > keep {
                    false
                } else {
                    width += next;
                    true
                }
            })
            .collect::<Vec<_>>();
        characters.reverse();
        characters.into_iter().collect()
    };
    if trimmed == value {
        return trimmed;
    }
    match (limit > 0, marker) {
        (true, Some(marker)) => format!("{trimmed}{marker}"),
        (false, Some(marker)) => format!("{marker}{trimmed}"),
        _ => trimmed,
    }
}

fn format_time_value(value: &str, flags: &TimeFlags<'_>) -> String {
    let Ok(timestamp) = value.parse::<i64>() else {
        return String::new();
    };
    if timestamp <= 0 {
        return String::new();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default();
    if flags.relative {
        return relative_time(timestamp, now);
    }
    if flags.difference {
        return now.saturating_sub(timestamp).to_string();
    }
    if flags.pretty {
        return pretty_time(timestamp, now);
    }
    let Some(time) = Local.timestamp_opt(timestamp, 0).single() else {
        return String::new();
    };
    format_datetime(&time, flags.format.unwrap_or("%a %b %e %H:%M:%S %Y"))
}

fn pretty_time(timestamp: i64, now: i64) -> String {
    let effective_now = now.max(timestamp);
    let age = effective_now.saturating_sub(timestamp);
    let Some(time) = Local.timestamp_opt(timestamp, 0).single() else {
        return String::new();
    };
    let Some(now) = Local.timestamp_opt(effective_now, 0).single() else {
        return String::new();
    };
    let format = if age < 24 * 3600 {
        "%H:%M"
    } else if (time.year() == now.year() && time.month() == now.month()) || age < 28 * 24 * 3600 {
        "%a%d"
    } else if (time.year() == now.year() && time.month() < now.month())
        || (time.year() == now.year() - 1 && time.month() > now.month())
    {
        "%d%b"
    } else {
        "%b%y"
    };
    format_datetime(&time, format)
}

fn relative_time(timestamp: i64, now: i64) -> String {
    if timestamp > now {
        return String::new();
    }
    let age = now - timestamp;
    if age == 0 {
        return "0s".to_owned();
    }
    let days = age / 86_400;
    let hours = age % 86_400 / 3_600;
    let minutes = age % 3_600 / 60;
    let seconds = age % 60;
    if days != 0 {
        if hours != 0 {
            format!("{days}d{hours}h")
        } else {
            format!("{days}d")
        }
    } else if hours != 0 {
        if minutes != 0 {
            format!("{hours}h{minutes}m")
        } else {
            format!("{hours}h")
        }
    } else if minutes != 0 {
        if seconds != 0 {
            format!("{minutes}m{seconds}s")
        } else {
            format!("{minutes}m")
        }
    } else {
        format!("{seconds}s")
    }
}

fn strftime_now(format: &str) -> String {
    format_datetime(&Local::now(), format)
}

fn format_datetime(time: &chrono::DateTime<Local>, format: &str) -> String {
    let Ok(items) = chrono::format::StrftimeItems::new(format).parse() else {
        return String::new();
    };
    let mut output = String::with_capacity(format.len());
    if write!(&mut output, "{}", time.format_with_items(items.iter())).is_err() {
        String::new()
    } else {
        output
    }
}

fn truncate_output(mut value: String) -> String {
    if value.len() <= MAX_STATUS_TEXT_BYTES {
        return value;
    }
    let boundary = (0..=MAX_STATUS_TEXT_BYTES)
        .rev()
        .find(|index| value.is_char_boundary(*index))
        .unwrap_or_default();
    value.truncate(boundary);
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_protocol::Axis;

    struct Stub;

    impl StatusHooks for Stub {
        fn strftime(&mut self, literal: &str) -> String {
            literal.replace("%H:%M", "09:41").replace("%Y", "2026")
        }

        fn shell(&mut self, command: &str) -> String {
            format!("<{command}>")
        }
    }

    fn context() -> StatusContext {
        StatusContext {
            active_window_index: Some(1),
            history_limit: Some(2_000),
            host: "tower.local".to_owned(),
            host_short: "tower".to_owned(),
            last_window_index: Some(3),
            next_session_id: "$5".to_owned(),
            pane_active: Some(true),
            pane_at_bottom: Some(true),
            pane_at_left: Some(false),
            pane_at_right: Some(true),
            pane_at_top: Some(true),
            pane_bottom: Some(49),
            pane_flags: "*".to_owned(),
            pane_height: Some(50),
            pane_id: "%7".to_owned(),
            pane_index: 0,
            pane_last: Some(false),
            pane_left: Some(80),
            pane_right: Some(159),
            pane_synchronized: false,
            pane_title: "/tmp/a b/main".to_owned(),
            pane_top: Some(0),
            pane_width: Some(80),
            pane_x: Some(80),
            pane_y: Some(0),
            pane_z: Some(1),
            pane_zoomed: false,
            pid: 42,
            server_sessions: 2,
            session_active: Some(true),
            session_attached: 1,
            session_attached_list: "client".to_owned(),
            session_id: "$4".to_owned(),
            session_name: "work".to_owned(),
            session_stack: "1,0".to_owned(),
            session_windows: 4,
            socket_path: "/tmp/tmux.sock".to_owned(),
            start_time: Some(946_728_000),
            uid: "1000".to_owned(),
            user: "fabrico".to_owned(),
            version: "0.2.0".to_owned(),
            window_active: Some(true),
            window_active_clients: 1,
            window_active_clients_list: "client".to_owned(),
            window_active_sessions: 1,
            window_active_sessions_list: "work".to_owned(),
            window_end: Some(false),
            window_height: Some(50),
            window_id: "@5".to_owned(),
            window_index: 1,
            window_last: Some(true),
            window_layout: "e582,160x50,0,0,7".to_owned(),
            window_linked_sessions: 1,
            window_linked_sessions_list: "work".to_owned(),
            window_name: "main".to_owned(),
            window_panes: 2,
            window_stack_index: 1,
            window_start: Some(false),
            window_visible_layout: "e582,160x50,0,0,7".to_owned(),
            window_width: Some(160),
            ..StatusContext::default()
        }
    }

    fn expand(format: &str) -> String {
        expand_status(format, &context(), &mut Stub)
    }

    #[test]
    fn vocabulary_is_the_sorted_pinned_198_entry_table() {
        assert_eq!(FORMAT_VARIABLES.len(), 198);
        assert!(
            FORMAT_VARIABLES
                .windows(2)
                .all(|pair| pair[0].name < pair[1].name)
        );
        let times = FORMAT_VARIABLES
            .iter()
            .filter(|variable| variable.kind == FormatKind::Time)
            .map(|variable| variable.name)
            .collect::<Vec<_>>();
        assert_eq!(
            times,
            [
                "buffer_created",
                "client_activity",
                "client_created",
                "pane_dead_time",
                "session_activity",
                "session_created",
                "session_last_attached",
                "start_time",
                "window_activity",
            ]
        );
        let context = context();
        for variable in &FORMAT_VARIABLES {
            let value = context
                .variable(variable.name)
                .unwrap_or_else(|| panic!("missing {}", variable.name));
            match variable.backing {
                FormatBacking::Empty => assert!(value.is_empty(), "{}", variable.name),
                FormatBacking::Zero => assert_eq!(value, "0", "{}", variable.name),
                _ => {}
            }
        }
        assert!(context.variable("not_a_tmux_variable").is_none());
    }

    #[test]
    fn built_scene_backfills_scope_and_reports_live_shapes() {
        let mut engine = MuxEngine::default();
        engine.set_format_server_context("tower.local", "tower", "/tmp/zz.sock");
        let (session, first_window, first_pane) = engine.state.create_session("work").unwrap();
        engine.state.rename_window(first_window, "shell").unwrap();
        let second_pane = engine
            .state
            .split_pane(first_pane, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let (second_window, _) = engine
            .state
            .create_window(session, Some("editor".to_owned()), PaneKind::Terminal)
            .unwrap();
        let resolved = FormatContext {
            session: Some(session),
            window: Some(first_window),
            pane: Some(second_pane),
            active_session: Some(session),
            format_type: FormatType::Pane,
        }
        .resolve(&engine);
        assert_eq!(
            resolved.variable("session_id").unwrap(),
            session.to_string()
        );
        assert_eq!(resolved.variable("session_windows").unwrap(), "2");
        assert_eq!(resolved.variable("active_window_index").unwrap(), "1");
        assert_eq!(resolved.variable("last_window_index").unwrap(), "1");
        assert_eq!(
            resolved.variable("window_id").unwrap(),
            first_window.to_string()
        );
        assert_eq!(resolved.variable("window_active").unwrap(), "0");
        assert_eq!(resolved.variable("window_panes").unwrap(), "2");
        assert_eq!(
            resolved.variable("pane_id").unwrap(),
            second_pane.to_string()
        );
        assert_eq!(resolved.variable("pane_format").unwrap(), "1");
        assert_eq!(resolved.variable("pane_z").unwrap(), "1");
        assert_eq!(resolved.variable("window_format").unwrap(), "0");
        assert_eq!(resolved.variable("server_sessions").unwrap(), "1");
        assert_eq!(resolved.variable("socket_path").unwrap(), "/tmp/zz.sock");
        assert!(engine.state.windows.contains_key(&second_window));
        for variable in &FORMAT_VARIABLES {
            assert!(
                resolved.variable(variable.name).is_some(),
                "{}",
                variable.name
            );
        }

        engine.state.toggle_zoom(second_pane).unwrap();
        let zoomed = FormatContext {
            session: Some(session),
            window: Some(first_window),
            pane: Some(second_pane),
            active_session: Some(session),
            format_type: FormatType::Pane,
        }
        .resolve(&engine);
        assert_ne!(
            zoomed.variable("window_layout"),
            zoomed.variable("window_visible_layout")
        );
        assert!(
            zoomed
                .variable("window_visible_layout")
                .unwrap()
                .ends_with(&format!(",{}", second_pane.0))
        );
    }

    #[test]
    fn aliases_styles_shell_and_literal_time_keep_the_status_seam() {
        assert_eq!(
            expand("[#S] #I:#W.#P #D #F #H #h"),
            "[work] 1:main.0 %7 *- tower.local tower"
        );
        assert_eq!(expand("##S #z"), "#S #z");
        assert_eq!(expand("#[fg=green,bold]%H:%M#[default]"), "09:41");
        assert_eq!(expand("up #(uptime)"), "up <uptime>");
    }

    #[test]
    fn comparisons_logic_truthiness_and_nested_conditionals_match_the_pin() {
        assert_eq!(
            expand("#{==:3,3}|#{!=:3,4}|#{<:10,2}|#{>:b,a}|#{<=:a,a}|#{>=:b,a}"),
            "1|1|1|1|1|1"
        );
        assert_eq!(expand("#{!:#{l:0}}|#{!!:00}|#{!!:-0}|#{!!:0.0}"), "1|1|1|1");
        assert_eq!(
            expand("#{&&:#{l:1},#{l:00},#{l:x}}|#{||:0,#{l:yes}}"),
            "1|1"
        );
        assert_eq!(expand("#{?#{==:#{window_panes},2},two,other}"), "two");
        assert_eq!(expand("#{?missing,no,#{l:default}}"), "default");
    }

    #[test]
    fn path_length_literal_and_quote_modifiers_are_lookup_scoped() {
        assert_eq!(expand("#{b:pane_title}"), "main");
        assert_eq!(expand("#{d:pane_title}"), "/tmp/a b");
        assert_eq!(expand("#{n:pane_title}"), "13");
        assert_eq!(expand("#{l:#{session_name}}"), "#{session_name}");
        assert_eq!(expand("#{q:pane_title}"), "/tmp/a\\ b/main");
        assert_eq!(expand("#{q/s:pane_title}"), "'/tmp/a b/main'");
        assert_eq!(expand("#{b:#{l:/x/y}}"), "/x/y");

        let mut context = context();
        context.pane_title = "/".to_owned();
        assert_eq!(
            expand_status("#{b:pane_title}|#{d:pane_title}", &context, &mut Stub),
            "/|/"
        );
        context.pane_title = "#tag".to_owned();
        assert_eq!(
            expand_status("#{q/h:pane_title}|#{q/e:pane_title}", &context, &mut Stub,),
            "##tag|##tag"
        );
        context.pane_title = " ".to_owned();
        assert_eq!(
            expand_status("#{q/a:pane_title}", &context, &mut Stub),
            "\" \""
        );
        context.pane_title = "$name".to_owned();
        assert_eq!(
            expand_status("#{q/a:pane_title}", &context, &mut Stub),
            "\"\\$name\""
        );
    }

    #[test]
    fn substitutions_are_global_case_aware_and_preserve_invalid_patterns() {
        let mut context = context();
        context.window_name = "main MAIN".to_owned();
        assert_eq!(
            expand_status(
                "#{s/a/A/:window_name}|#{s/main/x/i:window_name}|#{s/[/:window_name}",
                &context,
                &mut Stub,
            ),
            "mAin MAIN|x x|main MAIN"
        );
        context.window_name = "abc123".to_owned();
        assert_eq!(
            expand_status("#{s/([a-z]+)/\\1-/:window_name}", &context, &mut Stub),
            "abc-123"
        );
        context.window_name = "ab".to_owned();
        assert_eq!(
            expand_status("#{s/x*/X/:window_name}", &context, &mut Stub),
            "aXbX"
        );
    }

    #[test]
    fn match_supports_fnmatch_regex_casefold_and_fuzzy_results() {
        assert_eq!(expand("#{m:m*,#{window_name}}"), "1");
        assert_eq!(expand("#{m/ri:MAIN,#{window_name}}"), "1");
        assert_eq!(expand("#{m/r:[,#{window_name}}"), "0");
        assert_eq!(expand("#{m/z:mn,#{window_name}}"), "1");
        assert_eq!(expand("#{m/p:mn,#{window_name}}"), "0,3");

        let mut context = context();
        context.window_name = "axab".to_owned();
        assert_eq!(
            expand_status("#{m/p:ab,#{window_name}}", &context, &mut Stub),
            "2,3"
        );
        context.window_name = "MAIN".to_owned();
        assert_eq!(
            expand_status("#{m/z:mn,#{window_name}}", &context, &mut Stub),
            "1"
        );
        context.window_name = "main worker".to_owned();
        assert_eq!(
            expand_status(
                "#{m/z:^main,#{window_name}}|#{m/z:main$,#{window_name}}|#{m/z:!other main,#{window_name}}|#{m/z:nope|main,#{window_name}}",
                &context,
                &mut Stub,
            ),
            "1|0|1|1"
        );
        context.window_name = "main".to_owned();
        assert_eq!(
            expand_status("#{m/z:MN,#{window_name}}", &context, &mut Stub),
            "0"
        );
        context.window_name = "界a".to_owned();
        assert_eq!(
            expand_status("#{m/p:界,#{window_name}}", &context, &mut Stub),
            "0,1"
        );
    }

    #[test]
    fn truncation_uses_display_cells_markers_and_byte_length() {
        let mut context = context();
        context.pane_title = "ab界cd".to_owned();
        assert_eq!(
            expand_status(
                "#{=/4/...:pane_title}|#{=-3:pane_title}|#{n;=/4/...:pane_title}|#{=/2/x;=3:pane_title}",
                &context,
                &mut Stub,
            ),
            "ab界...|cd|8|abx"
        );
    }

    #[test]
    fn time_expand_again_and_expand_again_with_time_are_distinct() {
        assert_eq!(expand("#{t/f/%Y:start_time}"), "2000");
        assert_eq!(expand("#{t/f/%Y;t/f:start_time}"), "2000");
        assert!(!expand("#{t:start_time}").is_empty());
        assert_eq!(expand("#{t:session_created}"), "");
        let mut context = context();
        context.pane_title = "#{session_name}".to_owned();
        assert_eq!(
            expand_status("#{E:pane_title}", &context, &mut Stub),
            "work"
        );
        context.pane_title = "%Y #{session_name}".to_owned();
        assert_eq!(
            expand_status("#{T:pane_title}", &context, &mut Stub),
            "2026 work"
        );

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        context.start_time = Some(i64::try_from(now - 3_661).unwrap());
        assert_eq!(
            expand_status("#{t/r:start_time}", &context, &mut Stub),
            "1h1m"
        );
        assert_eq!(
            expand_status("#{t/p;t/r:start_time}", &context, &mut Stub),
            "1h1m"
        );
        assert!(expand_status("#{t/p:start_time}", &context, &mut Stub).contains(':'));
        let difference = expand_status("#{t/d:start_time}", &context, &mut Stub)
            .parse::<u64>()
            .unwrap();
        assert!((3_661..=3_663).contains(&difference));
    }

    #[test]
    fn unknown_and_malformed_forms_follow_source_fallbacks() {
        assert_eq!(expand("before#{not_a_tmux_variable}after"), "beforeafter");
        assert_eq!(expand("before#{z:pane_title}after"), "beforeafter");
        assert_eq!(expand("#{z:#{l:foo}}"), "z:foo");
        assert_eq!(expand("#{p:pane_title}"), "");
        assert_eq!(expand("before#{==:one}after"), "before");
    }

    #[test]
    fn recursive_expansion_stops_at_the_pinned_depth() {
        let nested = (0..10).fold("#{l:x}".to_owned(), |value, _| format!("#{{E:{value}}}"));
        assert_eq!(expand(&nested), "x");
        let hostile = (0..110).fold("#{l:x}".to_owned(), |value, _| format!("#{{E:{value}}}"));
        assert_eq!(expand(&hostile), "");
    }
}
