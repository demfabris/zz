//! Turning the `status-*` options into the text clients render.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::Read as _,
    path::PathBuf,
    process::{Child, Stdio},
    sync::{Arc, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::Local;
use glob::{MatchOptions, Pattern};
use regex::RegexBuilder;
use zz_mux::{
    MuxEngine, StatusContext, StatusFormats, StatusHooks, StatusRowVariables, display_width,
    expand_status,
};
use zz_protocol::{
    ClientId, MAX_STATUS_ROWS, MAX_STATUS_TEXT_BYTES, MuxSnapshot, PaneId, SessionId, StatusLine,
    WindowId,
};
use zz_terminal::{CellWidth, TerminalSession, TerminalViewport};

use crate::{configure_tmux_shim, shell_process};

const SHELL_TIMEOUT: Duration = Duration::from_secs(2);
const SHELL_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_SHELL_OUTPUT_BYTES: u64 = 4 * 1024;

#[derive(Default)]
pub(crate) struct StatusRenderer {
    shell_cache: BTreeMap<String, String>,
    published: BTreeMap<ClientId, StatusLine>,
    tmux_shim: Option<PathBuf>,
    zz_executable: Option<PathBuf>,
}

pub(crate) struct StatusRequest {
    pub(crate) client: ClientId,
    pub(crate) formats: StatusFormats,
    pub(crate) row_formats: BTreeMap<u32, String>,
    pub(crate) variables: StatusRowVariables,
    pub(crate) message_line: u8,
    pub(crate) customized: bool,
    pub(crate) title_format: Option<String>,
    pub(crate) context: StatusContext,
    pub(crate) facts: FormatHookFacts,
}

#[derive(Clone, Default)]
pub(crate) struct FormatHookFacts {
    pub(crate) terminals: Arc<BTreeMap<PaneId, Arc<TerminalSession>>>,
    pub(crate) pane_pipes: Arc<BTreeMap<PaneId, u32>>,
    pub(crate) session_attachments: Arc<BTreeMap<SessionId, (usize, String)>>,
    pub(crate) session_activity: Arc<BTreeMap<SessionId, u64>>,
    pub(crate) buffer: Option<BufferFormatFacts>,
    pub(crate) client: Option<ClientFormatFacts>,
    pub(crate) message: Option<MessageFormatFacts>,
}

#[derive(Clone)]
pub(crate) struct BufferFormatFacts {
    pub(crate) name: String,
    pub(crate) data: Arc<[u8]>,
    pub(crate) created: SystemTime,
}

#[derive(Clone)]
pub(crate) struct ClientFormatFacts {
    pub(crate) name: String,
    pub(crate) session: String,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) termname: String,
    pub(crate) uid: String,
    pub(crate) user: String,
    pub(crate) flags: String,
    pub(crate) theme: String,
    pub(crate) line: usize,
}

#[derive(Clone)]
pub(crate) struct MessageFormatFacts {
    pub(crate) number: u64,
    pub(crate) text: String,
    pub(crate) time: SystemTime,
}

impl StatusRenderer {
    pub(crate) fn render_changed(
        &mut self,
        requests: &[StatusRequest],
        refresh: bool,
    ) -> Vec<(ClientId, StatusLine)> {
        let mut touched = BTreeSet::new();
        let mut changed = Vec::new();
        for request in requests {
            let status = render(
                &mut self.shell_cache,
                &mut touched,
                request,
                refresh,
                self.tmux_shim.as_deref(),
                self.zz_executable.as_deref(),
            );
            if self.published.get(&request.client) == Some(&status) {
                continue;
            }
            self.published.insert(request.client, status.clone());
            changed.push((request.client, status));
        }
        if refresh {
            self.shell_cache
                .retain(|command, _| touched.contains(command));
        }
        changed
    }

    pub(crate) fn render_initial(&mut self, request: &StatusRequest) -> StatusLine {
        let mut touched = BTreeSet::new();
        let status = render(
            &mut self.shell_cache,
            &mut touched,
            request,
            false,
            self.tmux_shim.as_deref(),
            self.zz_executable.as_deref(),
        );
        self.published.insert(request.client, status.clone());
        status
    }

    pub(crate) fn forget(&mut self, client: ClientId) {
        self.published.remove(&client);
    }

    pub(crate) fn set_tmux_shim(&mut self, directory: PathBuf, executable: PathBuf) {
        self.tmux_shim = Some(directory);
        self.zz_executable = Some(executable);
    }
}

pub(crate) fn status_context(
    snapshot: &MuxSnapshot,
    engine: &MuxEngine,
    attached: Option<SessionId>,
    focused_window: Option<WindowId>,
) -> StatusContext {
    let mut context = engine.format_status_context(attached, focused_window, None);
    if context.host.is_empty() {
        let (host, host_short) = host_names();
        context.host.clone_from(host);
        context.host_short.clone_from(host_short);
    }
    let Some(session) = snapshot
        .sessions
        .iter()
        .find(|session| Some(session.id) == attached)
    else {
        return context;
    };
    let focused_window = focused_window
        .filter(|focused| session.windows.iter().any(|window| window.id == *focused))
        .unwrap_or(session.active_window);
    if context.window_active.is_some() {
        context.window_active = Some(true);
    }
    if context.pane_active.is_some() {
        context.pane_active = Some(true);
    }
    context.session_active = Some(true);
    context.session_attached = session.viewers.len();
    context.session_many_attached = session.viewers.len() > 1;
    context.session_attached_list = session
        .viewers
        .iter()
        .map(|viewer| viewer.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let active_viewers = session
        .viewers
        .iter()
        .filter(|viewer| viewer.window == focused_window)
        .collect::<Vec<_>>();
    context.window_active_clients = active_viewers.len();
    context.window_active_clients_list = active_viewers
        .iter()
        .map(|viewer| viewer.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    context
}

pub(crate) fn host_names() -> &'static (String, String) {
    static HOST: OnceLock<(String, String)> = OnceLock::new();
    HOST.get_or_init(|| {
        let host = sysinfo::System::host_name()
            .map(|host| host.trim().to_owned())
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| "localhost".to_owned());
        let short = host
            .split('.')
            .next()
            .filter(|short| !short.is_empty())
            .unwrap_or(host.as_str())
            .to_owned();
        (host, short)
    })
}

fn render(
    cache: &mut BTreeMap<String, String>,
    touched: &mut BTreeSet<String>,
    request: &StatusRequest,
    refresh: bool,
    tmux_shim: Option<&std::path::Path>,
    zz_executable: Option<&std::path::Path>,
) -> StatusLine {
    let now = Local::now();
    let title = request
        .title_format
        .as_ref()
        .map_or_else(String::new, |format| {
            let mut hooks = DaemonFormatHooks::status(
                &request.facts,
                &request.context,
                None,
                cache,
                touched,
                refresh,
                now,
                tmux_shim,
                zz_executable,
            );
            let mut title = expand_status(format, &request.context, &mut hooks);
            if title.len() > MAX_STATUS_TEXT_BYTES {
                let mut end = MAX_STATUS_TEXT_BYTES;
                while !title.is_char_boundary(end) {
                    end -= 1;
                }
                title.truncate(end);
            }
            title
        });
    if !request.formats.enabled {
        return StatusLine {
            title,
            position: request.formats.position,
            customized: request.customized,
            ..StatusLine::default()
        };
    }
    let (left, right) = {
        let mut hooks = DaemonFormatHooks::status(
            &request.facts,
            &request.context,
            None,
            cache,
            touched,
            refresh,
            now,
            tmux_shim,
            zz_executable,
        );
        (
            expand_status(&request.formats.left, &request.context, &mut hooks),
            expand_status(&request.formats.right, &request.context, &mut hooks),
        )
    };
    let mut hooks = DaemonFormatHooks::status(
        &request.facts,
        &request.context,
        Some(&request.variables),
        cache,
        touched,
        refresh,
        now,
        tmux_shim,
        zz_executable,
    );
    let base_style = expand_base_status_style(&request.formats, &request.context, &mut hooks);
    let lines = usize::from(request.formats.lines).min(MAX_STATUS_ROWS);
    let rows = (0..lines)
        .map(|index| {
            let index = u32::try_from(index).expect("status row index fits u32");
            request
                .row_formats
                .get(&index)
                .map_or_else(String::new, |format| {
                    expand_status(format, &request.context, &mut hooks)
                })
        })
        .collect::<Vec<_>>();
    let message_line = if rows.is_empty() {
        0
    } else {
        request
            .message_line
            .min(u8::try_from(rows.len() - 1).expect("status row count fits u8"))
    };
    StatusLine {
        left: wrap_status_style(
            &request.formats,
            &trim_status_left(&left, usize::from(request.formats.left_length)),
            &request.formats.left_style,
        ),
        right: wrap_status_style(
            &request.formats,
            // Wave B may re-trim status-right client-side to keep the clock visible;
            // the wire remains pin-faithful.
            &trim_status_left(&right, usize::from(request.formats.right_length)),
            &request.formats.right_style,
        ),
        title,
        base_style,
        rows,
        position: request.formats.position,
        message_line,
        customized: request.customized,
    }
}

fn expand_base_status_style(
    formats: &StatusFormats,
    context: &StatusContext,
    hooks: &mut DaemonFormatHooks<'_>,
) -> String {
    let mut style = expand_status(&formats.style, context, hooks);
    if zz_protocol::parse_style(&style).is_none() {
        style = String::new();
    }
    for (key, value) in [("fg", &formats.foreground), ("bg", &formats.background)] {
        if value.as_str() != "default" {
            let separator = if style.is_empty() { "" } else { "," };
            let addition = format!("{separator}{key}={value}");
            if style.len() + addition.len() <= MAX_STATUS_TEXT_BYTES {
                style.push_str(&addition);
            }
        }
    }
    if zz_protocol::parse_style(&style).is_none() {
        return String::new();
    }
    style
}

fn wrap_status_style(formats: &StatusFormats, text: &str, side_style: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut base = formats.style.clone();
    if formats.foreground != "default" {
        if !base.is_empty() {
            base.push(',');
        }
        base.push_str("fg=");
        base.push_str(&formats.foreground);
    }
    if formats.background != "default" {
        if !base.is_empty() {
            base.push(',');
        }
        base.push_str("bg=");
        base.push_str(&formats.background);
    }
    if base.is_empty() && matches!(side_style, "" | "default") {
        return text.to_owned();
    }
    let mut output = String::with_capacity(
        (base.len() + side_style.len() + text.len() + 32).min(MAX_STATUS_TEXT_BYTES),
    );
    let carries_base = if base.is_empty() {
        false
    } else {
        let marker = format!("#[{base}]");
        if marker.len() + "#[push-default]".len() <= MAX_STATUS_TEXT_BYTES {
            output.push_str(&marker);
            true
        } else {
            false
        }
    };
    if !side_style.is_empty() {
        let marker = format!("#[{side_style}]");
        let reserved = if carries_base {
            "#[push-default]".len()
        } else {
            0
        };
        if output.len() + marker.len() + reserved <= MAX_STATUS_TEXT_BYTES {
            output.push_str(&marker);
        }
    }
    if carries_base {
        output.push_str("#[push-default]");
    }
    let remaining = MAX_STATUS_TEXT_BYTES - output.len();
    let boundary = (0..=remaining.min(text.len()))
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or_default();
    output.push_str(&text[..boundary]);
    output
}

fn trim_status_left(value: &str, limit: usize) -> String {
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    let mut width = 0;
    while index < bytes.len() && width < limit {
        if bytes[index] == b'#' {
            let start = index;
            while bytes.get(index) == Some(&b'#') {
                index += 1;
            }
            let hashes = index - start;
            let marker = bytes.get(index) == Some(&b'[') && hashes % 2 == 1;
            let leading_width = if bytes.get(index) == Some(&b'[') {
                hashes / 2
            } else {
                hashes.div_ceil(2)
            };
            let copy_width = leading_width.min(limit - width);
            if copy_width != 0 {
                if hashes == 1 {
                    output.push('#');
                } else {
                    output.extend(std::iter::repeat_n('#', copy_width * 2));
                }
                width += copy_width;
            }
            if marker {
                let marker_start = index - 1;
                let Some(end) = status_style_end(value, index + 1) else {
                    break;
                };
                output.push_str(&value[marker_start..=end]);
                index = end + 1;
            }
            continue;
        }
        let character = value[index..]
            .chars()
            .next()
            .expect("status trim index is on a character boundary");
        let character_width = display_width(character.encode_utf8(&mut [0; 4]));
        if width + character_width <= limit {
            output.push(character);
        }
        width += character_width;
        index += character.len_utf8();
    }
    output
}

fn status_style_end(value: &str, start: usize) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut position = start;
    let mut formats = 0;
    while position < bytes.len() {
        if bytes[position] == b'#' && bytes.get(position + 1) == Some(&b'{') {
            formats += 1;
            position += 2;
            continue;
        }
        if bytes[position] == b'}' && formats != 0 {
            formats -= 1;
            position += 1;
            continue;
        }
        if bytes[position] == b']' && formats == 0 {
            return Some(position);
        }
        position += 1;
    }
    None
}

pub(crate) struct DaemonFormatHooks<'a> {
    facts: &'a FormatHookFacts,
    status_context: Option<&'a StatusContext>,
    variables: Option<&'a BTreeMap<String, String>>,
    options: Option<&'a StatusRowVariables>,
    cache: Option<&'a mut BTreeMap<String, String>>,
    touched: Option<&'a mut BTreeSet<String>>,
    refresh: bool,
    now: chrono::DateTime<Local>,
    tmux_shim: Option<&'a std::path::Path>,
    zz_executable: Option<&'a std::path::Path>,
}

impl<'a> DaemonFormatHooks<'a> {
    pub(crate) fn command(facts: &'a FormatHookFacts) -> Self {
        Self::command_with_optional_variables(facts, None)
    }

    pub(crate) fn command_with_optional_variables(
        facts: &'a FormatHookFacts,
        variables: Option<&'a BTreeMap<String, String>>,
    ) -> Self {
        Self {
            facts,
            status_context: None,
            variables,
            options: None,
            cache: None,
            touched: None,
            refresh: false,
            now: Local::now(),
            tmux_shim: None,
            zz_executable: None,
        }
    }

    pub(crate) fn command_with_variables(
        facts: &'a FormatHookFacts,
        variables: &'a BTreeMap<String, String>,
    ) -> Self {
        Self::command_with_optional_variables(facts, Some(variables))
    }

    pub(crate) fn with_status_options(mut self, options: &'a StatusRowVariables) -> Self {
        self.options = Some(options);
        self
    }

    fn status(
        facts: &'a FormatHookFacts,
        context: &'a StatusContext,
        options: Option<&'a StatusRowVariables>,
        cache: &'a mut BTreeMap<String, String>,
        touched: &'a mut BTreeSet<String>,
        refresh: bool,
        now: chrono::DateTime<Local>,
        tmux_shim: Option<&'a std::path::Path>,
        zz_executable: Option<&'a std::path::Path>,
    ) -> Self {
        Self {
            facts,
            status_context: Some(context),
            variables: None,
            options,
            cache: Some(cache),
            touched: Some(touched),
            refresh,
            now,
            tmux_shim,
            zz_executable,
        }
    }
}

impl StatusHooks for DaemonFormatHooks<'_> {
    /// Byte parity with the pin requires the PLATFORM's strftime: tmux's
    /// `format_strftime` is plain libc strftime, and libcs disagree about
    /// unknown `%` sequences (glibc passes them through, BSD eats them), so a
    /// reimplementation cannot match both. Zero return maps to empty output,
    /// mirroring format.c's too-long/empty handling. This is the workspace's
    /// only unsafe block; it exists because matching the platform means
    /// calling the platform.
    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn strftime(&mut self, literal: &str) -> String {
        let Ok(format) = std::ffi::CString::new(literal) else {
            return literal.to_owned();
        };
        let time: libc::time_t = TryInto::try_into(self.now.timestamp()).unwrap_or(0);
        let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
        if unsafe { libc::localtime_r(&raw const time, &raw mut tm) }.is_null() {
            return literal.to_owned();
        }
        let mut buffer = [0u8; 8192];
        let written = unsafe {
            libc::strftime(
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                format.as_ptr(),
                &raw const tm,
            )
        };
        String::from_utf8_lossy(&buffer[..written]).into_owned()
    }

    #[cfg(not(unix))]
    fn strftime(&mut self, literal: &str) -> String {
        let Ok(items) = chrono::format::StrftimeItems::new(literal).parse() else {
            return literal.to_owned();
        };
        let mut formatted = String::with_capacity(literal.len());
        if write!(
            &mut formatted,
            "{}",
            self.now.format_with_items(items.iter())
        )
        .is_err()
        {
            return literal.to_owned();
        }
        formatted
    }

    fn shell(&mut self, command: &str) -> String {
        let (Some(cache), Some(touched)) = (self.cache.as_deref_mut(), self.touched.as_deref_mut())
        else {
            return String::new();
        };
        touched.insert(command.to_owned());
        if !self.refresh
            && let Some(cached) = cache.get(command)
        {
            return cached.clone();
        }
        let output = run_shell(
            command,
            self.status_context,
            self.tmux_shim,
            self.zz_executable,
        );
        cache.insert(command.to_owned(), output.clone());
        output
    }

    fn variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
        if let Some(value) = self.variables.and_then(|variables| variables.get(name)) {
            return Some(value.clone());
        }
        if let Some(value) = self
            .options
            .and_then(|options| options.lookup(&context.session_id, &context.window_id, name))
        {
            return Some(value.clone());
        }
        match name {
            "buffer_created" => Some(
                self.facts
                    .buffer
                    .as_ref()?
                    .created
                    .duration_since(UNIX_EPOCH)
                    .ok()?
                    .as_secs()
                    .to_string(),
            ),
            "buffer_full" => Some(buffer_full(&self.facts.buffer.as_ref()?.data)),
            "buffer_name" => Some(self.facts.buffer.as_ref()?.name.clone()),
            "buffer_sample" => Some(buffer_sample(&self.facts.buffer.as_ref()?.data)),
            "buffer_size" => Some(self.facts.buffer.as_ref()?.data.len().to_string()),
            "client_flags" => Some(self.facts.client.as_ref()?.flags.clone()),
            "client_height" => Some(self.facts.client.as_ref()?.height.to_string()),
            "client_name" => Some(self.facts.client.as_ref()?.name.clone()),
            "client_session" => Some(self.facts.client.as_ref()?.session.clone()),
            "client_termname" => Some(self.facts.client.as_ref()?.termname.clone()),
            "client_theme" => Some(self.facts.client.as_ref()?.theme.clone()),
            "client_uid" => Some(self.facts.client.as_ref()?.uid.clone()),
            "client_user" => Some(self.facts.client.as_ref()?.user.clone()),
            "client_width" => Some(self.facts.client.as_ref()?.width.to_string()),
            "line" => Some(self.facts.client.as_ref()?.line.to_string()),
            "message_number" => Some(self.facts.message.as_ref()?.number.to_string()),
            "message_text" => Some(self.facts.message.as_ref()?.text.clone()),
            "message_time" => Some(
                self.facts
                    .message
                    .as_ref()?
                    .time
                    .duration_since(UNIX_EPOCH)
                    .ok()?
                    .as_secs()
                    .to_string(),
            ),
            "pane_pipe" => self
                .facts
                .pane_pipes
                .contains_key(&context.pane_id.parse().ok()?)
                .then(|| "1".to_owned()),
            "pane_pipe_pid" => self
                .facts
                .pane_pipes
                .get(&context.pane_id.parse().ok()?)
                .map(u32::to_string),
            "session_attached" => Some(
                self.facts
                    .session_attachments
                    .get(&context.session_id.parse().ok()?)
                    .map_or(0, |(count, _)| *count)
                    .to_string(),
            ),
            "session_attached_list" => Some(
                self.facts
                    .session_attachments
                    .get(&context.session_id.parse().ok()?)
                    .map_or_else(String::new, |(_, names)| names.clone()),
            ),
            "session_many_attached" => Some(
                if self
                    .facts
                    .session_attachments
                    .get(&context.session_id.parse().ok()?)
                    .is_some_and(|(count, _)| *count > 1)
                {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            ),
            _ => None,
        }
    }

    fn session_activity(&mut self, session: SessionId) -> u64 {
        self.facts
            .session_activity
            .get(&session)
            .copied()
            .unwrap_or_default()
    }

    fn pane_search(
        &mut self,
        pane: Option<PaneId>,
        pattern: &str,
        regex: bool,
        ignore_case: bool,
    ) -> usize {
        let Some(viewport) = pane
            .and_then(|pane| self.facts.terminals.get(&pane))
            .map(|terminal| terminal.latest_viewport())
        else {
            return 0;
        };
        search_viewport(&viewport, pattern, regex, ignore_case)
    }
}

fn buffer_full(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}

fn buffer_sample(data: &[u8]) -> String {
    let prefix = &data[..data.len().min(200)];
    let mut output = String::new();
    let mut index = 0;
    while index < prefix.len() {
        let byte = prefix[index];
        match byte {
            b'\n' => output.push_str("\\n"),
            b'\r' => output.push_str("\\r"),
            b'\t' => output.push_str("\\t"),
            b'\x08' => output.push_str("\\b"),
            b'\x07' => output.push_str("\\a"),
            b'\x0b' => output.push_str("\\v"),
            b'\x0c' => output.push_str("\\f"),
            b'\\' => output.push_str("\\\\"),
            0 if prefix
                .get(index + 1)
                .is_some_and(|next| matches!(*next, b'0'..=b'7')) =>
            {
                output.push_str("\\000");
            }
            0 => output.push_str("\\0"),
            0x20..=0x7e => output.push(char::from(byte)),
            0x80..=0xff => {
                let valid = match std::str::from_utf8(&prefix[index..]) {
                    Ok(value) => value,
                    Err(error) if error.valid_up_to() > 0 => {
                        std::str::from_utf8(&prefix[index..index + error.valid_up_to()])
                            .expect("UTF-8 validator identified a valid prefix")
                    }
                    Err(_) => {
                        let _ = write!(&mut output, "\\{byte:03o}");
                        index += 1;
                        continue;
                    }
                };
                let character = valid.chars().next().expect("valid UTF-8 is nonempty");
                output.push(character);
                index += character.len_utf8();
                continue;
            }
            _ => {
                let _ = write!(&mut output, "\\{byte:03o}");
            }
        }
        index += 1;
    }
    let shortened = data.len() > 200 || output.len() > 200;
    if output.len() > 200 {
        let boundary = (0..=200)
            .rev()
            .find(|index| output.is_char_boundary(*index))
            .unwrap_or_default();
        output.truncate(boundary);
    }
    if shortened {
        output.push_str("...");
    }
    output
}

fn search_viewport(
    viewport: &TerminalViewport,
    pattern: &str,
    regex: bool,
    ignore_case: bool,
) -> usize {
    let regex = regex.then(|| {
        RegexBuilder::new(pattern)
            .case_insensitive(ignore_case)
            .build()
            .ok()
    });
    if matches!(regex, Some(None)) {
        return 0;
    }
    let glob = regex
        .is_none()
        .then(|| Pattern::new(&format!("*{pattern}*")).ok());
    for row in 0..viewport.rows {
        let mut text = String::with_capacity(usize::from(viewport.columns));
        for cell in viewport.row(row).unwrap_or_default() {
            if matches!(cell.width(), CellWidth::SpacerHead | CellWidth::SpacerTail) {
                continue;
            }
            if cell.glyph() == 0 {
                text.push(' ');
            } else {
                viewport.push_glyph(*cell, &mut text);
            }
        }
        let text = text.trim_end_matches(|character: char| character.is_ascii_whitespace());
        let matched = if let Some(Some(regex)) = &regex {
            regex.is_match(text)
        } else if let Some(Some(glob)) = &glob {
            glob.matches_with(
                text,
                MatchOptions {
                    case_sensitive: !ignore_case,
                    require_literal_separator: false,
                    require_literal_leading_dot: false,
                },
            )
        } else {
            false
        };
        if matched {
            return usize::from(row) + 1;
        }
    }
    0
}

fn run_shell(
    command: &str,
    context: Option<&StatusContext>,
    tmux_shim: Option<&std::path::Path>,
    zz_executable: Option<&std::path::Path>,
) -> String {
    let mut process = shell_process(command);
    if let Some(context) = context {
        let requested_cwd = std::path::Path::new(&context.pane_current_path);
        let cwd = if requested_cwd.is_dir() {
            requested_cwd.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir())
        };
        process
            .current_dir(&cwd)
            .env("PWD", cwd.as_os_str())
            .env(
                "TMUX",
                format!("{},{},-1", context.socket_path, std::process::id()),
            )
            .env("ZZ_SOCKET", &context.socket_path)
            .env_remove("TMUX_PANE");
    }
    configure_tmux_shim(&mut process, tmux_shim, zz_executable);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;

        process.process_group(0);
    }
    let Ok(mut child) = process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        log::debug!(
            target: "zz_daemon::status",
            "status command failed to start command={command}"
        );
        return String::new();
    };

    let deadline = Instant::now() + SHELL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                log::debug!(
                    target: "zz_daemon::status",
                    "status command timed out command={command}"
                );
                terminate_shell(&mut child);
                return String::new();
            }
            Ok(None) => thread::sleep(SHELL_POLL_INTERVAL),
            Err(_) => {
                terminate_shell(&mut child);
                return String::new();
            }
        }
    }

    let mut buffer = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(MAX_SHELL_OUTPUT_BYTES).read_to_end(&mut buffer);
    }
    first_line(&buffer)
}

fn terminate_shell(child: &mut Child) {
    #[cfg(unix)]
    let _ = rustix::process::kill_process_group(
        rustix::process::Pid::from_child(child),
        rustix::process::Signal::KILL,
    );
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", pid.as_str(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn first_line(output: &[u8]) -> String {
    let line = output
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    String::from_utf8_lossy(line).trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_mux::{PaneKind, SplitSize, expand_format_values};
    use zz_protocol::Axis;
    use zz_terminal::{SessionStatus, TerminalViewId};

    fn request(client: u64, left: &str, right: &str) -> StatusRequest {
        StatusRequest {
            client: ClientId(client),
            formats: StatusFormats {
                left: left.to_owned(),
                right: right.to_owned(),
                style: String::new(),
                left_style: String::new(),
                right_style: String::new(),
                left_length: u16::MAX,
                right_length: u16::MAX,
                ..StatusFormats::default()
            },
            row_formats: BTreeMap::new(),
            variables: StatusRowVariables::default(),
            message_line: 0,
            customized: false,
            title_format: None,
            context: StatusContext {
                session_name: "work".to_owned(),
                ..StatusContext::default()
            },
            facts: FormatHookFacts::default(),
        }
    }

    fn engine_request(
        client: u64,
        engine: &MuxEngine,
        session: Option<SessionId>,
    ) -> StatusRequest {
        let snapshot = engine.state.snapshot();
        StatusRequest {
            client: ClientId(client),
            formats: engine.status_formats_for_session(session),
            row_formats: engine.status_format_array_for_session(session),
            variables: engine.status_row_variables_for_session(session),
            message_line: engine.message_line_for_session(session),
            customized: engine.status_customized_for_session(session),
            title_format: (session.is_some() && engine.set_titles_for_session(session))
                .then(|| engine.set_titles_string_for_session(session)),
            context: status_context(&snapshot, engine, session, None),
            facts: FormatHookFacts::default(),
        }
    }

    fn execute(engine: &mut MuxEngine, context: &mut zz_mux::ExecutionContext, args: &[&str]) {
        engine
            .execute(
                context,
                &zz_protocol::CommandInvocation::new(args[0], args[1..].iter().copied()),
            )
            .unwrap_or_else(|error| panic!("{args:?}: {error:?}"));
    }

    #[test]
    fn set_titles_expands_per_client_and_survives_status_off() {
        let mut engine = MuxEngine::default();
        let mut context = zz_mux::ExecutionContext::default();
        execute(
            &mut engine,
            &mut context,
            &["new-session", "-s", "alpha", "-n", "main"],
        );
        let session = context.session;
        let mut renderer = StatusRenderer::default();

        let baseline = renderer.render_initial(&engine_request(1, &engine, session));
        assert_eq!(baseline.title, "");

        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "set-titles", "on"],
        );
        let titled = renderer.render_initial(&engine_request(1, &engine, session));
        assert!(
            titled.title.starts_with("alpha:0:main"),
            "default set-titles-string expands in client context: {}",
            titled.title
        );

        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "set-titles-string", "#S custom"],
        );
        let custom = renderer.render_initial(&engine_request(1, &engine, session));
        assert_eq!(custom.title, "alpha custom");

        execute(&mut engine, &mut context, &["set-option", "status", "off"]);
        let off = renderer.render_initial(&engine_request(1, &engine, session));
        assert!(off.rows.is_empty());
        assert_eq!(off.title, "alpha custom");

        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "set-titles", "off"],
        );
        let untitled = renderer.render_initial(&engine_request(1, &engine, session));
        assert_eq!(untitled.title, "");
    }

    #[test]
    fn personalized_rows_expand_per_session_and_dedupe() {
        let mut engine = MuxEngine::default();
        let mut context = zz_mux::ExecutionContext::default();
        execute(
            &mut engine,
            &mut context,
            &["new-session", "-s", "alpha", "-n", "main"],
        );
        let alpha = context.session.expect("alpha session");
        execute(
            &mut engine,
            &mut context,
            &["new-session", "-d", "-s", "beta", "-n", "logs"],
        );
        let beta = engine
            .state
            .sessions
            .iter()
            .find(|(_, session)| session.name == "beta")
            .map(|(id, _)| *id)
            .expect("beta session");
        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "status-right", "static"],
        );

        let requests = [
            engine_request(1, &engine, Some(alpha)),
            engine_request(2, &engine, Some(beta)),
        ];
        let mut renderer = StatusRenderer::default();
        let first = renderer.render_changed(&requests, false);
        assert_eq!(first.len(), 2);
        let alpha_status = &first[0].1;
        let beta_status = &first[1].1;
        assert_eq!(alpha_status.rows.len(), 1);
        assert!(
            alpha_status.rows[0].contains("[alpha]"),
            "row 0 carries status-left: {}",
            alpha_status.rows[0]
        );
        assert!(
            alpha_status.rows[0].contains("0:main"),
            "row 0 carries the window list: {}",
            alpha_status.rows[0]
        );
        assert!(
            alpha_status.rows[0].contains("static"),
            "row 0 carries status-right: {}",
            alpha_status.rows[0]
        );
        assert!(beta_status.rows[0].contains("[beta]"));
        assert!(beta_status.rows[0].contains("0:logs"));
        assert_ne!(alpha_status.rows, beta_status.rows);
        assert_eq!(alpha_status.validate(), Ok(()));
        assert_eq!(beta_status.validate(), Ok(()));
        assert!(renderer.render_changed(&requests, false).is_empty());
    }

    #[test]
    fn status_row_counts_follow_the_status_option() {
        let mut engine = MuxEngine::default();
        let mut context = zz_mux::ExecutionContext::default();
        execute(
            &mut engine,
            &mut context,
            &["new-session", "-s", "work", "-n", "main"],
        );
        let session = context.session.expect("session id");
        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "status", "off"],
        );
        let off =
            StatusRenderer::default().render_initial(&engine_request(1, &engine, Some(session)));
        assert!(off.rows.is_empty());
        assert_eq!(off.message_line, 0);
        assert!(off.customized);
        assert!(off.is_empty());

        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "status", "5"],
        );
        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "message-line", "4"],
        );
        let five =
            StatusRenderer::default().render_initial(&engine_request(1, &engine, Some(session)));
        assert_eq!(five.rows.len(), 5);
        assert!(five.rows[0].contains("[work]"));
        assert!(!five.rows[1].is_empty());
        assert!(!five.rows[2].is_empty());
        assert_eq!(five.rows[3], "");
        assert_eq!(five.rows[4], "");
        assert_eq!(five.message_line, 4);
        assert_eq!(five.validate(), Ok(()));

        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "status", "2"],
        );
        let two =
            StatusRenderer::default().render_initial(&engine_request(1, &engine, Some(session)));
        assert_eq!(two.rows.len(), 2);
        assert_eq!(
            two.message_line, 1,
            "message-line clamps below the row count"
        );
        assert_eq!(two.validate(), Ok(()));
    }

    #[test]
    fn per_window_status_overrides_style_the_rendered_row() {
        let mut engine = MuxEngine::default();
        let mut context = zz_mux::ExecutionContext::default();
        execute(
            &mut engine,
            &mut context,
            &["new-session", "-s", "work", "-n", "main"],
        );
        execute(
            &mut engine,
            &mut context,
            &["new-window", "-d", "-n", "other"],
        );
        let session = context.session.expect("session id");
        execute(
            &mut engine,
            &mut context,
            &[
                "set-window-option",
                "-t",
                "work:0",
                "window-status-current-format",
                "OVERRIDE",
            ],
        );
        let status =
            StatusRenderer::default().render_initial(&engine_request(1, &engine, Some(session)));
        let row = status.rows.first().expect("status row");
        assert!(row.contains("OVERRIDE"), "row: {row}");
        assert!(
            !row.contains("0:main"),
            "the current window's default label must be replaced: {row}"
        );
        assert!(
            row.contains("1:other"),
            "windows without overrides keep the global format: {row}"
        );
    }

    #[test]
    fn window_status_separator_uses_each_item_scope_and_skips_the_trailing_item() {
        let mut engine = MuxEngine::default();
        let mut context = zz_mux::ExecutionContext::default();
        execute(
            &mut engine,
            &mut context,
            &["new-session", "-s", "work", "-n", "main"],
        );
        execute(
            &mut engine,
            &mut context,
            &["new-window", "-d", "-n", "logs"],
        );
        let session = context.session.expect("session id");
        for args in [
            &["set-option", "-g", "status-left", ""] as &[&str],
            &["set-option", "-g", "status-right", ""],
            &[
                "set-option",
                "-g",
                "status-format[0]",
                "#{W:#{T:window-status-format}#{?loop_last_flag,,#{E:window-status-separator}},#{T:window-status-current-format}#{?loop_last_flag,,#{E:window-status-separator}}}",
            ],
            &[
                "set-window-option",
                "-g",
                "window-status-format",
                "N#{window_index}:#{window_name}",
            ],
            &[
                "set-window-option",
                "-g",
                "window-status-current-format",
                "C#{window_index}:#{window_name}",
            ],
            &[
                "set-window-option",
                "-g",
                "window-status-separator",
                "#{?#{==:#{window_index},0},#[fg=red]G#{window_index}:#{window_name},#[fg=blue]BAD}",
            ],
        ] {
            execute(&mut engine, &mut context, args);
        }

        let global =
            StatusRenderer::default().render_initial(&engine_request(1, &engine, Some(session)));
        assert_eq!(global.left, "");
        assert_eq!(global.right, "");
        assert_eq!(global.rows, ["C0:main#[fg=red]G0:mainN1:logs"]);

        execute(
            &mut engine,
            &mut context,
            &[
                "set-window-option",
                "-t",
                "work:0",
                "window-status-separator",
                "#{?#{==:#{window_index},0},#[bold]L#{window_index}:#{window_name},#[reverse]BAD}",
            ],
        );
        let local =
            StatusRenderer::default().render_initial(&engine_request(2, &engine, Some(session)));
        assert_eq!(local.rows, ["C0:main#[bold]L0:mainN1:logs"]);
    }

    #[test]
    fn sparse_session_status_formats_keep_blank_rows_without_compaction() {
        let mut engine = MuxEngine::default();
        let mut context = zz_mux::ExecutionContext::default();
        execute(&mut engine, &mut context, &["new-session", "-s", "work"]);
        let session = context.session.expect("session id");
        execute(&mut engine, &mut context, &["set-option", "status", "5"]);
        execute(
            &mut engine,
            &mut context,
            &["set-option", "status-format[3]", "MARK #S"],
        );
        let status =
            StatusRenderer::default().render_initial(&engine_request(1, &engine, Some(session)));
        assert_eq!(status.rows, ["", "", "", "MARK work", ""]);
        assert!(status.customized);
        assert_eq!(status.validate(), Ok(()));
    }

    #[test]
    fn base_style_applies_status_style_then_fg_bg_overrides() {
        let mut renderer = StatusRenderer::default();
        let mut styled = request(1, "", "");
        styled.formats.style = "bg=blue,fg=white".to_owned();
        styled.formats.foreground = "red".to_owned();
        let status = renderer.render_initial(&styled);
        assert_eq!(status.base_style, "bg=blue,fg=white,fg=red");
        assert_eq!(status.validate(), Ok(()));

        let mut dynamic = request(2, "", "");
        dynamic.formats.style = "fg=#{?window_zoomed,red,green}".to_owned();
        dynamic.formats.background = "black".to_owned();
        let status = renderer.render_initial(&dynamic);
        assert_eq!(status.base_style, "fg=green,bg=black");
        assert_eq!(status.validate(), Ok(()));
    }

    #[test]
    fn an_unparseable_expanded_status_style_degrades_instead_of_dropping_the_event() {
        let mut renderer = StatusRenderer::default();
        let mut broken = request(1, "LEFT", "RIGHT");
        broken.formats.style = "bg=#{@theme_bg}".to_owned();
        let status = renderer.render_initial(&broken);
        assert_eq!(status.base_style, "");
        assert_eq!(status.rows.len(), 1);
        assert!(status.left.ends_with("LEFT"));
        assert!(status.right.ends_with("RIGHT"));
        assert_eq!(
            status.validate(),
            Ok(()),
            "the event must survive the encode seam"
        );

        let mut overridden = request(2, "", "");
        overridden.formats.style = "bg=#{@theme_bg}".to_owned();
        overridden.formats.foreground = "red".to_owned();
        let status = renderer.render_initial(&overridden);
        assert_eq!(status.base_style, "fg=red");
        assert_eq!(status.validate(), Ok(()));
    }

    #[test]
    fn message_line_clamps_against_the_published_row_count() {
        let mut renderer = StatusRenderer::default();
        let mut clamped = request(1, "", "");
        clamped.formats.lines = 2;
        clamped.message_line = 4;
        let status = renderer.render_initial(&clamped);
        assert_eq!(status.rows.len(), 2);
        assert_eq!(status.message_line, 1);

        let mut disabled = request(2, "", "");
        disabled.formats.enabled = false;
        disabled.message_line = 3;
        assert_eq!(renderer.render_initial(&disabled).message_line, 0);
    }

    #[test]
    fn only_changed_clients_are_published() {
        let mut renderer = StatusRenderer::default();
        let requests = [request(1, "[#S]", ""), request(2, "[#S]", "")];

        let first = renderer.render_changed(&requests, false);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].1.left, "[work]");
        assert!(renderer.render_changed(&requests, false).is_empty());

        let renamed = [request(1, "[#S]", ""), {
            let mut request = request(2, "[#S]", "");
            request.context.session_name = "infra".to_owned();
            request
        }];
        let second = renderer.render_changed(&renamed, false);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].0, ClientId(2));
        assert_eq!(second[0].1.left, "[infra]");
    }

    #[test]
    fn status_sides_left_trim_display_width_without_counting_styles() {
        assert_eq!(trim_status_left("a#[bold]界bc", 4), "a#[bold]界b");
        assert_eq!(trim_status_left("##[bold]界", 3), "##[b");
    }

    #[test]
    fn status_sides_carry_base_then_side_styles() {
        let mut request = request(1, "abcdef", "uvwxyz");
        request.formats.style = "bg=blue,fg=white".to_owned();
        request.formats.foreground = "red".to_owned();
        request.formats.left_style = "bold".to_owned();
        request.formats.right_style = "italics".to_owned();
        request.formats.left_length = 4;
        request.formats.right_length = 3;
        let status = StatusRenderer::default().render_initial(&request);
        assert_eq!(
            status.left,
            "#[bg=blue,fg=white,fg=red]#[bold]#[push-default]abcd"
        );
        assert_eq!(
            status.right,
            "#[bg=blue,fg=white,fg=red]#[italics]#[push-default]uvw"
        );
    }

    #[test]
    fn status_style_wrapping_stays_inside_the_wire_limit() {
        let mut request = request(1, &"x".repeat(MAX_STATUS_TEXT_BYTES), "");
        request.formats.style = "bold,".repeat(800);
        request.formats.left_style = "italics,".repeat(800);
        let status = StatusRenderer::default().render_initial(&request);
        assert_eq!(status.validate(), Ok(()));
        assert!(status.left.is_char_boundary(status.left.len()));
    }

    #[test]
    fn a_disabled_status_renders_empty() {
        let mut renderer = StatusRenderer::default();
        let mut request = request(1, "[#S]", "%H");
        request.formats.enabled = false;
        let status = renderer.render_initial(&request);
        assert!(status.is_empty());
    }

    #[test]
    fn status_pane_index_uses_effective_pane_base_and_pane_order() {
        let mut engine = MuxEngine::default();
        let (session, window, target) = engine.state.create_session("work").unwrap();
        let source = engine
            .state
            .split_pane(target, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        engine
            .state
            .join_pane(
                source,
                target,
                Axis::Vertical,
                SplitSize::Default,
                true,
                false,
                false,
            )
            .unwrap();
        engine
            .execute(
                &mut zz_mux::ExecutionContext::default(),
                &zz_protocol::CommandInvocation::new(
                    "set-window-option",
                    ["-g", "pane-base-index", "1"],
                ),
            )
            .unwrap();
        let snapshot = engine.state.snapshot();
        let mut layout_order = Vec::new();
        snapshot.sessions[0].windows[0]
            .layout
            .panes(&mut layout_order);

        assert_eq!(layout_order, [source, target]);
        assert_eq!(engine.state.windows[&window].pane_order(), [target, source]);
        let context = status_context(&snapshot, &engine, Some(session), Some(window));
        assert_eq!(context.pane_index, 2);
        assert_eq!(
            context.window_layout,
            engine.state.windows[&window].layout.dump()
        );
    }

    #[test]
    fn content_search_reads_visible_terminal_rows() {
        let pane = PaneId(9);
        let terminal = Arc::new(TerminalSession::spawn_output_view(
            "search".to_owned(),
            "alpha\nbravo\ncharlie".to_owned(),
        ));
        terminal.attach_view(TerminalViewId(1));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !matches!(terminal.latest_viewport().status, SessionStatus::Running) {
            assert!(
                Instant::now() < deadline,
                "output view did not become ready"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let facts = FormatHookFacts {
            terminals: Arc::new(BTreeMap::from([(pane, terminal)])),
            buffer: None,
            ..FormatHookFacts::default()
        };
        let context = StatusContext {
            pane_id: pane.to_string(),
            ..StatusContext::default()
        };
        let mut hooks = DaemonFormatHooks::command(&facts);
        assert_eq!(
            expand_format_values(
                "#{C:bravo}|#{C/i:BRAVO}|#{C/r:^charlie$}|#{C:missing}",
                &context,
                &mut hooks,
            ),
            "2|2|3|0"
        );
    }

    #[test]
    fn buffer_samples_preserve_utf8_and_escape_control_and_invalid_bytes() {
        assert_eq!(buffer_sample("α\n\t\\".as_bytes()), "α\\n\\t\\\\");
        assert_eq!(buffer_sample(&[0xff]), "\\377");
        assert_eq!(buffer_sample(&[0, b'x', 0, b'7']), "\\0x\\0007");
        assert_eq!(buffer_full(b"a\0b"), "a\0b");
        assert_eq!(
            buffer_sample(&vec![b'x'; 201]),
            format!("{}...", "x".repeat(200))
        );
    }

    #[test]
    fn commands_run_once_and_then_come_from_the_cache() {
        let mut renderer = StatusRenderer::default();
        let directory = tempfile::Builder::new()
            .prefix("zz-status-cache-")
            .tempdir_in(".")
            .expect("the cache fixture is created");
        let source = directory.path().join("value");
        std::fs::write(&source, "first\n").expect("the first value is written");
        #[cfg(unix)]
        let command = format!("cat '{}'", source.display());
        #[cfg(windows)]
        let command = format!("type {}", source.display());
        let format = format!("#({command})");
        let requests = [request(1, &format, "")];

        let first = renderer.render_changed(&requests, false);
        let initial = first[0].1.left.clone();
        assert_eq!(initial, "first", "a first render runs the command");
        std::fs::write(&source, "second\n").expect("the second value is written");
        assert!(renderer.render_changed(&requests, false).is_empty());
        let ticked = renderer.render_changed(&requests, true);
        assert_eq!(ticked.len(), 1);
        assert_eq!(ticked[0].1.left, "second");
    }

    #[cfg(unix)]
    #[test]
    fn status_commands_receive_tmux_and_working_directory_without_tmux_pane() {
        let directory = tempfile::Builder::new()
            .prefix("zz-status-environment-")
            .tempdir_in(".")
            .expect("the working directory is created");
        let socket = "/tmp/zz-status-environment.sock";
        let mut status_request = request(
            1,
            "#(echo \"$TMUX|$ZZ_SOCKET|$PWD|${TMUX_PANE-unset}\")",
            "",
        );
        status_request.context.socket_path = socket.to_owned();
        status_request.context.pane_current_path = directory
            .path()
            .canonicalize()
            .expect("the working directory resolves")
            .to_string_lossy()
            .into_owned();

        let status = StatusRenderer::default().render_initial(&status_request);

        assert_eq!(
            status.left,
            format!(
                "{socket},{},-1|{socket}|{}|unset",
                std::process::id(),
                status_request.context.pane_current_path
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn status_commands_resolve_literal_tmux_through_the_private_zz_shim() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().expect("temporary status shim");
        let executable = directory.path().join("fake-zz");
        std::fs::write(
            &executable,
            b"#!/bin/sh\nprintf '%s|%s' \"$1\" \"$ZZ_SOCKET\"\n",
        )
        .expect("write fake zz executable");
        let shim = directory.path().join("shim");
        std::fs::create_dir(&shim).expect("create shim directory");
        std::fs::write(
            shim.join("tmux"),
            b"#!/bin/sh\nexec \"$ZZ_TMUX_EXECUTABLE\" \"$@\"\n",
        )
        .expect("write tmux shim");
        for path in [&executable, &shim.join("tmux")] {
            let mut permissions = std::fs::metadata(path)
                .expect("shim metadata")
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(path, permissions).expect("make shim runnable");
        }

        let socket = "/tmp/zz-status-shim.sock";
        let mut status_request = request(1, "#(tmux status)", "");
        status_request.context.socket_path = socket.to_owned();
        let mut renderer = StatusRenderer::default();
        renderer.set_tmux_shim(shim, executable);

        assert_eq!(
            renderer.render_initial(&status_request).left,
            format!("status|{socket}")
        );
    }

    #[test]
    fn a_tick_forgets_commands_no_format_names_any_more() {
        let mut renderer = StatusRenderer::default();
        renderer.render_changed(&[request(1, "#(echo kept)", "#(echo dropped)")], true);
        assert_eq!(renderer.shell_cache.len(), 2);
        renderer.render_changed(&[request(1, "#(echo kept)", "")], true);
        assert_eq!(
            renderer.shell_cache.keys().collect::<Vec<_>>(),
            ["echo kept"]
        );
    }

    #[test]
    fn strftime_and_shell_failures_degrade_to_text() {
        let mut renderer = StatusRenderer::default();
        let status = renderer.render_initial(&request(1, "%H:%M", "#(exit 3)"));
        assert_eq!(status.left.len(), 5, "a clock renders as HH:MM");
        assert!(
            status.right.is_empty(),
            "a failing command renders as blank"
        );
    }

    #[test]
    fn only_the_first_output_line_is_used() {
        assert_eq!(first_line(b"one\ntwo\n"), "one");
        assert_eq!(first_line(b"trailing \t\n"), "trailing");
        assert_eq!(first_line(b""), "");
    }

    #[test]
    fn a_wedged_process_tree_is_killed_and_renders_blank() {
        #[cfg(unix)]
        let command = "sleep 30 & wait";
        #[cfg(windows)]
        let command = "ping -n 31 127.0.0.1";

        let started = Instant::now();
        assert_eq!(run_shell(command, None, None, None), "");
        assert!(
            started.elapsed() < SHELL_TIMEOUT * 3,
            "the timeout, not the command, bounds the render"
        );
    }
}
