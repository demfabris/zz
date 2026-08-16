//! tmux `status-*` options and the format language they are written in.
//!
//! # Supported format language
//!
//! A subset of tmux's FORMATS. Anything unrecognized expands to nothing, as in
//! tmux.
//!
//! | Form | Meaning |
//! | --- | --- |
//! | `##` | a literal `#` |
//! | `%H`, `%d-%b-%y`, … | strftime, applied to literal runs only |
//! | `#S` `#I` `#W` `#P` `#T` `#D` `#F` `#H` `#h` | single-character variable shorthands |
//! | `#{session_name}` | a variable by name; see [`StatusContext::variable`] |
//! | `#{=20:pane_title}` | keep the first 20 characters; `=-20:` keeps the last |
//! | `#{?window_zoomed_flag,Z,}` | conditional on a variable being truthy; `!` negates |
//! | `#(uptime)` | shell command output, first line only |
//! | `#[fg=green,bold]` | style directives, dropped |

use std::{borrow::Cow, collections::BTreeMap, time::Duration};

use zz_protocol::{Axis, MAX_STATUS_TEXT_BYTES, PaneId, SessionId, WindowId};

use crate::model::{MuxState, window_cell_extent};

/// Empty, where tmux ships `[#S] `: the sidebar and titlebar already name the
/// session and pane. An empty format drops the status section entirely.
pub const DEFAULT_STATUS_LEFT: &str = "";
pub const DEFAULT_STATUS_RIGHT: &str = "";
pub const DEFAULT_STATUS_INTERVAL: Duration = Duration::from_secs(15);
/// Longest `status-left`/`status-right` zz stores. tmux has no limit; this one
/// stops a `source-file` typo handing the expander a megabyte.
pub const MAX_STATUS_FORMAT_BYTES: usize = 4 * 1024;

/// The `status-*` options zz honors, as left by `.tmux.conf` or `set-option`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusFormats {
    pub enabled: bool,
    /// How often `#()` commands are re-run. Zero disables periodic refresh, as
    /// in tmux; the status still refreshes when the mux state changes.
    pub interval: Duration,
    pub left: String,
    pub right: String,
}

impl Default for StatusFormats {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: DEFAULT_STATUS_INTERVAL,
            left: DEFAULT_STATUS_LEFT.to_owned(),
            right: DEFAULT_STATUS_RIGHT.to_owned(),
        }
    }
}

impl StatusFormats {
    /// The current text of a format option, so `set-option -a` can append to it.
    #[must_use]
    pub fn format(&self, option: StatusOption) -> Option<&str> {
        match option {
            StatusOption::Left => Some(self.left.as_str()),
            StatusOption::Right => Some(self.right.as_str()),
            StatusOption::Enabled | StatusOption::Interval => None,
        }
    }

    /// Apply one `set-option` value, or restore the default when `value` is
    /// `None` (`set-option -u`). Reports whether anything moved; a value that
    /// does not parse leaves the option alone.
    pub fn set(&mut self, option: StatusOption, value: Option<&str>) -> Result<bool, &'static str> {
        let defaults = Self::default();
        match option {
            StatusOption::Enabled => {
                let next = match value {
                    Some(value) => parse_enabled(value)?,
                    None => defaults.enabled,
                };
                Ok(std::mem::replace(&mut self.enabled, next) != next)
            }
            StatusOption::Interval => {
                let next = match value {
                    Some(value) => parse_interval(value)?,
                    None => defaults.interval,
                };
                Ok(std::mem::replace(&mut self.interval, next) != next)
            }
            StatusOption::Left | StatusOption::Right => {
                let next = match value {
                    Some(value) => parse_format(value)?,
                    None => match option {
                        StatusOption::Right => defaults.right,
                        _ => defaults.left,
                    },
                };
                let slot = match option {
                    StatusOption::Right => &mut self.right,
                    _ => &mut self.left,
                };
                Ok(std::mem::replace(slot, next) != *slot)
            }
        }
    }
}

/// Which `status-*` option a `set-option` invocation names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusOption {
    Enabled,
    Interval,
    Left,
    Right,
}

impl StatusOption {
    /// Recognize a supported `status-*` option name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "status" => Self::Enabled,
            "status-interval" => Self::Interval,
            "status-left" => Self::Left,
            "status-right" => Self::Right,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "status",
            Self::Interval => "status-interval",
            Self::Left => "status-left",
            Self::Right => "status-right",
        }
    }
}

fn parse_enabled(value: &str) -> Result<bool, &'static str> {
    match value {
        "on" | "1" | "2" | "3" | "4" | "5" => Ok(true),
        "off" | "0" => Ok(false),
        _ => Err("status expects on, off, or a line count in 1..5"),
    }
}

fn parse_interval(value: &str) -> Result<Duration, &'static str> {
    value
        .parse::<u32>()
        .map(|seconds| Duration::from_secs(u64::from(seconds)))
        .map_err(|_| "status-interval expects a whole number of seconds")
}

fn parse_format(value: &str) -> Result<String, &'static str> {
    if value.len() > MAX_STATUS_FORMAT_BYTES {
        return Err("status format exceeds the supported length");
    }
    Ok(value.to_owned())
}

/// Everything the supported variable subset can name. Fields describe the
/// client's current view: attached session, its active window, that pane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusContext {
    pub session_id: String,
    pub session_name: String,
    pub session_windows: usize,
    pub window_id: String,
    pub window_index: u32,
    pub window_name: String,
    pub window_panes: usize,
    pub window_width: Option<u16>,
    pub window_height: Option<u16>,
    pub window_active: Option<bool>,
    pub window_zoomed: bool,
    pub window_bell: bool,
    pub pane_index: u32,
    pub pane_id: String,
    pub pane_title: String,
    pub pane_width: Option<u16>,
    pub pane_height: Option<u16>,
    pub pane_active: Option<bool>,
    pub pane_synchronized: bool,
    pub host: String,
    pub host_short: String,
}

impl StatusContext {
    /// Resolve one variable by its tmux name. `None` is an unknown name, which
    /// the expander renders as nothing.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<Cow<'_, str>> {
        Some(match name {
            "session_id" => Cow::Borrowed(self.session_id.as_str()),
            "session_name" => Cow::Borrowed(self.session_name.as_str()),
            "session_windows" => Cow::Owned(self.session_windows.to_string()),
            "window_id" => Cow::Borrowed(self.window_id.as_str()),
            "window_index" => Cow::Owned(self.window_index.to_string()),
            "window_name" => Cow::Borrowed(self.window_name.as_str()),
            "window_panes" => Cow::Owned(self.window_panes.to_string()),
            "window_width" => optional_number(self.window_width),
            "window_height" => optional_number(self.window_height),
            "window_active" => Cow::Borrowed(optional_flag(self.window_active)),
            "window_flags" => Cow::Owned(self.window_flags()),
            "window_zoomed_flag" => Cow::Borrowed(flag(self.window_zoomed)),
            "window_bell_flag" => Cow::Borrowed(flag(self.window_bell)),
            "pane_index" => Cow::Owned(self.pane_index.to_string()),
            "pane_id" => Cow::Borrowed(self.pane_id.as_str()),
            "pane_title" => Cow::Borrowed(self.pane_title.as_str()),
            "pane_width" => optional_number(self.pane_width),
            "pane_height" => optional_number(self.pane_height),
            "pane_active" => Cow::Borrowed(optional_flag(self.pane_active)),
            "pane_synchronized" => Cow::Borrowed(flag(self.pane_synchronized)),
            "host" => Cow::Borrowed(self.host.as_str()),
            "host_short" => Cow::Borrowed(self.host_short.as_str()),
            _ => return None,
        })
    }

    const fn shorthand(character: char) -> Option<&'static str> {
        Some(match character {
            'S' => "session_name",
            'I' => "window_index",
            'W' => "window_name",
            'F' => "window_flags",
            'P' => "pane_index",
            'D' => "pane_id",
            'T' => "pane_title",
            'H' => "host",
            'h' => "host_short",
            _ => return None,
        })
    }

    /// `None` is the status-line view, whose focused window is always current;
    /// the daemon also fills `Some(true)` there since tmux's status line
    /// resolves against the current window. Command/list rows set the real
    /// per-row value.
    fn window_flags(&self) -> String {
        // tmux's flag order: current, bell, zoomed.
        let mut flags = String::new();
        if self.window_active.unwrap_or(true) {
            flags.push('*');
        }
        if self.window_bell {
            flags.push('!');
        }
        if self.window_zoomed {
            flags.push('Z');
        }
        flags
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FormatContext {
    pub(crate) session: Option<SessionId>,
    pub(crate) window: Option<WindowId>,
    pub(crate) pane: Option<PaneId>,
}

struct ResolvedFormatContext {
    values: StatusContext,
    has_session: bool,
    has_window: bool,
    has_pane: bool,
}

impl FormatContext {
    fn resolve(
        self,
        state: &MuxState,
        pane_cells: &BTreeMap<PaneId, (u16, u16)>,
    ) -> ResolvedFormatContext {
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

        let mut context = StatusContext::default();
        if let Some(session) = session.and_then(|session| state.sessions.get(&session)) {
            context.session_id = session.id.to_string();
            context.session_name.clone_from(&session.name);
            context.session_windows = session.windows.len();
        }
        if let Some(window) = window.and_then(|window| state.windows.get(&window)) {
            context.window_id = window.id.to_string();
            context.window_index = window.index;
            context.window_name.clone_from(&window.name);
            context.window_panes = window.panes.len();
            context.window_width =
                window_cell_extent(state, pane_cells, window.id, Axis::Horizontal)
                    .map(|extent| extent.round() as u16);
            context.window_height =
                window_cell_extent(state, pane_cells, window.id, Axis::Vertical)
                    .map(|extent| extent.round() as u16);
            context.window_active = state
                .sessions
                .get(&window.session)
                .map(|session| session.active_window == window.id);
            context.window_zoomed = window.zoomed_pane.is_some();
            context.window_bell = window.panes.values().any(|pane| pane.bell);

            if let Some(pane) = pane.and_then(|pane| window.panes.get(&pane)) {
                context.pane_index = window
                    .pane_order()
                    .iter()
                    .position(|candidate| *candidate == pane.id)
                    .and_then(|index| u32::try_from(index).ok())
                    .unwrap_or_default();
                context.pane_id = pane.id.to_string();
                context.pane_title.clone_from(&pane.title);
                let geometry = pane_cells.get(&pane.id).copied();
                context.pane_width = geometry.map(|(columns, _)| columns);
                context.pane_height = geometry.map(|(_, rows)| rows);
                context.pane_active = Some(window.active_pane == pane.id);
                context.pane_synchronized =
                    state.pane_synchronize_panes(pane.id).unwrap_or_default();
            }
        }
        ResolvedFormatContext {
            values: context,
            has_session: session.is_some(),
            has_window: window.is_some(),
            has_pane: pane.is_some(),
        }
    }
}

trait FormatVariables {
    fn variable(&self, name: &str) -> Option<Cow<'_, str>>;
}

impl FormatVariables for StatusContext {
    fn variable(&self, name: &str) -> Option<Cow<'_, str>> {
        StatusContext::variable(self, name)
    }
}

impl FormatVariables for ResolvedFormatContext {
    fn variable(&self, name: &str) -> Option<Cow<'_, str>> {
        let missing = (!self.has_session
            && matches!(name, "session_id" | "session_name" | "session_windows"))
            || (!self.has_window
                && matches!(
                    name,
                    "window_id"
                        | "window_index"
                        | "window_name"
                        | "window_panes"
                        | "window_width"
                        | "window_height"
                        | "window_active"
                        | "window_flags"
                        | "window_zoomed_flag"
                        | "window_bell_flag"
                ))
            || (!self.has_pane
                && matches!(
                    name,
                    "pane_index"
                        | "pane_id"
                        | "pane_title"
                        | "pane_width"
                        | "pane_height"
                        | "pane_active"
                        | "pane_synchronized"
                ));
        if missing {
            Some(Cow::Borrowed(""))
        } else {
            self.values.variable(name)
        }
    }
}

pub(crate) fn expand_format(
    format: &str,
    state: &MuxState,
    context: FormatContext,
    pane_cells: &BTreeMap<PaneId, (u16, u16)>,
) -> String {
    struct CommandHooks;

    impl StatusHooks for CommandHooks {
        fn strftime(&mut self, literal: &str) -> String {
            literal.to_owned()
        }

        fn shell(&mut self, _command: &str) -> String {
            String::new()
        }
    }

    expand_with_context(
        format,
        &context.resolve(state, pane_cells),
        &mut CommandHooks,
    )
}

fn optional_number(value: Option<u16>) -> Cow<'static, str> {
    value.map_or(Cow::Borrowed(""), |value| Cow::Owned(value.to_string()))
}

const fn optional_flag(value: Option<bool>) -> &'static str {
    match value {
        Some(value) => flag(value),
        None => "",
    }
}

const fn flag(set: bool) -> &'static str {
    if set { "1" } else { "0" }
}

/// The two impure operations a status format needs, supplied by the daemon.
pub trait StatusHooks {
    /// Expand strftime `%` sequences in one literal run of the format.
    fn strftime(&mut self, literal: &str) -> String;
    /// Run a `#(command)` and return its first output line.
    fn shell(&mut self, command: &str) -> String;
}

/// Expand one status format into the text a client renders, truncated at
/// [`MAX_STATUS_TEXT_BYTES`] on a character boundary.
pub fn expand_status(
    format: &str,
    context: &StatusContext,
    hooks: &mut impl StatusHooks,
) -> String {
    expand_with_context(format, context, hooks)
}

fn expand_with_context(
    format: &str,
    context: &(impl FormatVariables + ?Sized),
    hooks: &mut impl StatusHooks,
) -> String {
    let mut out = String::with_capacity(format.len());
    let mut literal = String::new();
    let mut rest = format;

    while let Some(offset) = rest.find('#') {
        literal.push_str(&rest[..offset]);
        rest = &rest[offset..];
        let mut following = rest[1..].chars();
        let Some(next) = following.next() else {
            literal.push('#');
            rest = "";
            break;
        };
        let consumed = 1 + next.len_utf8();
        match next {
            '#' => {
                literal.push('#');
                rest = &rest[consumed..];
            }
            '[' => rest = skip_group(rest, 1, '[', ']', &mut literal, '['),
            '(' => {
                let Some(end) = group_end(&rest[1..], '(', ')') else {
                    literal.push_str("#(");
                    rest = &rest[consumed..];
                    continue;
                };
                let command = &rest[2..=end];
                flush(&mut out, &mut literal, hooks);
                out.push_str(&hooks.shell(command));
                rest = &rest[2 + end..];
            }
            '{' => {
                let Some(end) = group_end(&rest[1..], '{', '}') else {
                    literal.push_str("#{");
                    rest = &rest[consumed..];
                    continue;
                };
                let body = &rest[2..=end];
                flush(&mut out, &mut literal, hooks);
                out.push_str(&expand_replacement(body, context, hooks));
                rest = &rest[2 + end..];
            }
            _ => {
                if let Some(name) = StatusContext::shorthand(next) {
                    flush(&mut out, &mut literal, hooks);
                    out.push_str(&context.variable(name).unwrap_or_default());
                } else {
                    literal.push('#');
                    literal.push(next);
                }
                rest = &rest[consumed..];
            }
        }
    }
    literal.push_str(rest);
    flush(&mut out, &mut literal, hooks);

    truncate_chars(out, MAX_STATUS_TEXT_BYTES)
}

fn expand_replacement(
    body: &str,
    context: &(impl FormatVariables + ?Sized),
    hooks: &mut impl StatusHooks,
) -> String {
    if let Some(condition) = body.strip_prefix('?') {
        return expand_conditional(condition, context, hooks);
    }
    if let Some((limit, inner)) = truncation(body) {
        let value = expand_replacement(inner, context, hooks);
        return truncate_to_limit(&value, limit);
    }
    context
        .variable(body)
        .map(Cow::into_owned)
        .unwrap_or_default()
}

fn expand_conditional(
    condition: &str,
    context: &(impl FormatVariables + ?Sized),
    hooks: &mut impl StatusHooks,
) -> String {
    let mut parts = split_top_level(condition);
    if parts.len() < 3 {
        return String::new();
    }
    parts.truncate(3);
    let otherwise = parts.pop().unwrap_or_default();
    let then = parts.pop().unwrap_or_default();
    let test = parts.pop().unwrap_or_default();
    let (test, negated) = match test.strip_prefix('!') {
        Some(test) => (test, true),
        None => (test, false),
    };
    let truthy = context
        .variable(test)
        .is_some_and(|value| !value.is_empty() && value != "0");
    let chosen = if truthy == negated { otherwise } else { then };
    expand_with_context(chosen, context, hooks)
}

fn truncation(body: &str) -> Option<(isize, &str)> {
    let rest = body.strip_prefix('=')?;
    let (digits, inner) = rest.split_once(':')?;
    digits.parse::<isize>().ok().map(|limit| (limit, inner))
}

fn truncate_to_limit(value: &str, limit: isize) -> String {
    let keep = limit.unsigned_abs();
    let count = value.chars().count();
    if keep == 0 || count <= keep {
        return value.to_owned();
    }
    if limit.is_negative() {
        value.chars().skip(count - keep).collect()
    } else {
        value.chars().take(keep).collect()
    }
}

fn split_top_level(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let bytes = body.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'#' if matches!(bytes.get(index + 1), Some(b'{' | b'(')) => {
                depth += 1;
                index += 2;
                continue;
            }
            b'}' | b')' if depth > 0 => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&body[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(&body[start..]);
    parts
}

fn group_end(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in s.char_indices() {
        if character == open {
            depth += 1;
        } else if character == close {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn skip_group<'a>(
    rest: &'a str,
    open_len: usize,
    open: char,
    close: char,
    literal: &mut String,
    literal_open: char,
) -> &'a str {
    if let Some(end) = group_end(&rest[open_len..], open, close) {
        &rest[open_len + 1 + end..]
    } else {
        literal.push('#');
        literal.push(literal_open);
        &rest[open_len + 1..]
    }
}

fn flush(out: &mut String, literal: &mut String, hooks: &mut impl StatusHooks) {
    if !literal.is_empty() {
        out.push_str(&hooks.strftime(literal));
        literal.clear();
    }
}

fn truncate_chars(mut text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    let boundary = (0..=limit)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or_default();
    text.truncate(boundary);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub;

    impl StatusHooks for Stub {
        fn strftime(&mut self, literal: &str) -> String {
            literal.replace("%H:%M", "09:41").replace("%d", "25")
        }

        fn shell(&mut self, command: &str) -> String {
            format!("<{command}>")
        }
    }

    fn context() -> StatusContext {
        StatusContext {
            session_id: "$4".to_owned(),
            session_name: "work".to_owned(),
            session_windows: 3,
            window_id: "@5".to_owned(),
            window_index: 1,
            window_name: "frontend".to_owned(),
            window_panes: 2,
            window_width: Some(160),
            window_height: Some(50),
            window_active: Some(true),
            window_zoomed: false,
            window_bell: false,
            pane_index: 0,
            pane_id: "%7".to_owned(),
            pane_title: "cargo watch --workspace".to_owned(),
            pane_width: Some(80),
            pane_height: Some(50),
            pane_active: Some(true),
            pane_synchronized: false,
            host: "tower.local".to_owned(),
            host_short: "tower".to_owned(),
        }
    }

    fn expand(format: &str) -> String {
        expand_status(format, &context(), &mut Stub)
    }

    #[test]
    fn literal_runs_pass_through_strftime_and_shorthands_resolve() {
        assert_eq!(expand("[#S] %H:%M"), "[work] 09:41");
        assert_eq!(expand("#I:#W.#P on #h"), "1:frontend.0 on tower");
    }

    #[test]
    fn named_variables_match_their_shorthands() {
        assert_eq!(expand("#{session_name}"), expand("#S"));
        assert_eq!(expand("#{session_id}:#{window_id}"), "$4:@5");
        assert_eq!(expand("#{window_panes} panes"), "2 panes");
        assert_eq!(expand("#{session_windows}"), "3");
        assert_eq!(
            expand("#{window_width}x#{window_height}:#{window_active}"),
            "160x50:1"
        );
        assert_eq!(
            expand("#{pane_width}x#{pane_height}:#{pane_active}"),
            "80x50:1"
        );
    }

    #[test]
    fn missing_geometry_and_activity_variables_expand_to_nothing() {
        assert_eq!(
            expand_status(
                "#{window_width}:#{window_height}:#{window_active}:#{pane_width}:#{pane_height}:#{pane_active}",
                &StatusContext::default(),
                &mut Stub,
            ),
            ":::::"
        );
    }

    #[test]
    fn a_doubled_hash_is_a_literal_and_unknown_forms_stay_put() {
        assert_eq!(expand("##S"), "#S");
        assert_eq!(expand("100## of #S"), "100# of work");
        assert_eq!(expand("#z"), "#z");
    }

    #[test]
    fn unknown_variables_expand_to_nothing() {
        assert_eq!(expand("[#{client_activity}]"), "[]");
    }

    #[test]
    fn style_directives_are_dropped_without_splitting_the_literal() {
        assert_eq!(expand("#[fg=green,bold]%H:%M#[default]"), "09:41");
        assert_eq!(expand("#[fg=colour[1]]ok"), "ok");
    }

    #[test]
    fn shell_output_is_taken_verbatim_and_not_re_read_for_strftime() {
        assert_eq!(expand("#(date +%H)"), "<date +%H>");
        assert_eq!(expand("up #(uptime) at %H:%M"), "up <uptime> at 09:41");
    }

    #[test]
    fn truncation_keeps_either_end() {
        assert_eq!(expand("#{=5:pane_title}"), "cargo");
        assert_eq!(expand("#{=-9:pane_title}"), "workspace");
        assert_eq!(expand("#{=99:window_name}"), "frontend");
        assert_eq!(expand("#{=0:window_name}"), "frontend");
    }

    #[test]
    fn conditionals_pick_a_branch_and_expand_it() {
        assert_eq!(expand("#{?window_zoomed_flag,Z,-}"), "-");
        assert_eq!(expand("#{?!window_zoomed_flag,Z,-}"), "Z");
        assert_eq!(expand("#{?session_name,#S,none}"), "work");
        assert_eq!(
            expand("#{?window_zoomed_flag,,#{=4:window_name} %H:%M}"),
            "fron 09:41"
        );
    }

    #[test]
    fn a_conditional_missing_a_branch_expands_to_nothing() {
        assert_eq!(expand("#{?window_zoomed_flag,Z}"), "");
    }

    #[test]
    fn window_flags_report_the_bell_in_tmux_order() {
        let flags = |bell, zoomed| {
            expand_status(
                "#F",
                &StatusContext {
                    window_bell: bell,
                    window_zoomed: zoomed,
                    ..context()
                },
                &mut Stub,
            )
        };
        assert_eq!(flags(false, false), "*");
        assert_eq!(flags(true, false), "*!");
        assert_eq!(flags(false, true), "*Z");
        assert_eq!(flags(true, true), "*!Z");
        assert_eq!(
            expand_status(
                "#F",
                &StatusContext {
                    window_active: None,
                    window_bell: true,
                    ..context()
                },
                &mut Stub,
            ),
            "*!"
        );
        assert_eq!(
            expand_status(
                "#F",
                &StatusContext {
                    window_active: Some(false),
                    window_bell: true,
                    ..context()
                },
                &mut Stub,
            ),
            "!"
        );
        assert_eq!(
            expand_status(
                "#{?window_bell_flag,rang,quiet}",
                &StatusContext {
                    window_bell: true,
                    ..context()
                },
                &mut Stub,
            ),
            "rang"
        );
    }

    #[test]
    fn unterminated_groups_survive_as_literal_text() {
        assert_eq!(expand("#{session_name"), "#{session_name");
        assert_eq!(expand("#(uptime"), "#(uptime");
        assert_eq!(expand("#[fg=red"), "#[fg=red");
        assert_eq!(expand("trailing #"), "trailing #");
    }

    #[test]
    fn expansion_is_bounded() {
        struct Flood;
        impl StatusHooks for Flood {
            fn strftime(&mut self, literal: &str) -> String {
                literal.to_owned()
            }
            fn shell(&mut self, _: &str) -> String {
                "x".repeat(MAX_STATUS_TEXT_BYTES * 4)
            }
        }
        let text = expand_status("#(flood)", &context(), &mut Flood);
        assert_eq!(text.len(), MAX_STATUS_TEXT_BYTES);
    }

    #[test]
    fn explicit_context_expands_session_window_and_pane_rows() {
        let mut state = MuxState::default();
        let (session, first_window, first_pane) = state.create_session("work").unwrap();
        state.rename_window(first_window, "shell").unwrap();
        let (second_window, second_pane) = state
            .create_window(
                session,
                Some("editor".to_owned()),
                crate::PaneKind::Terminal,
            )
            .unwrap();
        let third_pane = state
            .split_pane(
                second_pane,
                zz_protocol::Axis::Horizontal,
                crate::PaneKind::Terminal,
            )
            .unwrap();
        let pane_cells = BTreeMap::new();

        assert_eq!(
            expand_format(
                "#{session_id}:#S #{session_windows}[#F]|#{window_index}|#{pane_index}",
                &state,
                FormatContext {
                    session: Some(session),
                    window: None,
                    pane: None,
                },
                &pane_cells,
            ),
            format!("{session}:work 2[*]|1|1")
        );
        assert_eq!(
            expand_format(
                "#{window_id} #I:#W[#F]",
                &state,
                FormatContext {
                    session: Some(session),
                    window: Some(first_window),
                    pane: None,
                },
                &pane_cells,
            ),
            format!("{first_window} 0:shell[]")
        );
        assert_eq!(
            expand_format(
                "#{window_id} #I:#W[#F]",
                &state,
                FormatContext {
                    session: Some(session),
                    window: Some(second_window),
                    pane: None,
                },
                &pane_cells,
            ),
            format!("{second_window} 1:editor[*]")
        );
        assert_eq!(
            expand_format(
                "#{pane_id}:#P:#T:#(ignored)",
                &state,
                FormatContext {
                    session: Some(session),
                    window: Some(second_window),
                    pane: Some(third_pane),
                },
                &pane_cells,
            ),
            format!("{third_pane}:1:terminal:")
        );
        assert_eq!(
            expand_format(
                "#{pane_id}",
                &state,
                FormatContext {
                    session: Some(session),
                    window: Some(first_window),
                    pane: Some(first_pane),
                },
                &pane_cells,
            ),
            first_pane.to_string()
        );
    }

    #[test]
    fn option_values_parse_like_tmux() {
        let mut formats = StatusFormats::default();
        assert_eq!(formats.set(StatusOption::Enabled, Some("off")), Ok(true));
        assert!(!formats.enabled);
        assert_eq!(formats.set(StatusOption::Enabled, Some("2")), Ok(true));
        assert!(formats.enabled);
        assert_eq!(formats.set(StatusOption::Enabled, Some("on")), Ok(false));
        assert!(
            formats
                .set(StatusOption::Enabled, Some("sometimes"))
                .is_err()
        );
        assert!(formats.enabled, "a rejected value leaves the option alone");

        assert_eq!(formats.set(StatusOption::Interval, Some("5")), Ok(true));
        assert_eq!(formats.interval, Duration::from_secs(5));
        assert_eq!(formats.set(StatusOption::Interval, Some("0")), Ok(true));
        assert!(formats.set(StatusOption::Interval, Some("-1")).is_err());

        assert!(
            formats
                .set(
                    StatusOption::Left,
                    Some(&"x".repeat(MAX_STATUS_FORMAT_BYTES + 1))
                )
                .is_err()
        );
        assert_eq!(formats.set(StatusOption::Right, Some("%H:%M")), Ok(true));
        assert_eq!(formats.format(StatusOption::Right), Some("%H:%M"));
        assert_eq!(formats.set(StatusOption::Right, None), Ok(true));
        assert_eq!(formats.right, DEFAULT_STATUS_RIGHT);
        assert_eq!(formats.set(StatusOption::Interval, None), Ok(true));
        assert_eq!(formats.interval, DEFAULT_STATUS_INTERVAL);
    }

    #[test]
    fn option_names_round_trip() {
        for option in [
            StatusOption::Enabled,
            StatusOption::Interval,
            StatusOption::Left,
            StatusOption::Right,
        ] {
            assert_eq!(StatusOption::from_name(option.as_str()), Some(option));
        }
        assert_eq!(StatusOption::from_name("status-justify"), None);
    }
}
