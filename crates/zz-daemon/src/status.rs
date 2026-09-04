//! Turning the `status-*` options into the text clients render.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::Read as _,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::Local;
use glob::{MatchOptions, Pattern};
use regex::RegexBuilder;
use zz_mux::{
    FormatClientRow, FormatEnvironRow, MuxEngine, StatusContext, StatusFormats, StatusHooks,
    StatusRowVariables, TtyTerm, display_width, expand_status,
};
use zz_protocol::{
    ClientId, MAX_STATUS_ROWS, MAX_STATUS_TEXT_BYTES, MuxSnapshot, PaneId, RawText, SessionId,
    StatusLine, WindowId,
};
use zz_terminal::{CellWidth, CopyModeFacts, ProgressBar, TerminalSession, TerminalViewport};

use crate::{configure_shell_job_environment, paths::home_directory, shell_process};

const SHELL_TIMEOUT: Duration = Duration::from_secs(2);
const SHELL_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_SHELL_OUTPUT_BYTES: u64 = 4 * 1024;
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ShellCacheScope {
    Attached(ClientId),
    Unattached,
}

type ShellCacheKey = (ShellCacheScope, PathBuf, String);
pub(crate) const LIST_CLIENTS_CONTEXT_FORMATS: [&str; 1] = ["line"];
/// The `window_copy_formats` names zz answers. tmux adds them to the format
/// tree from the pane's mode entry; zz reads them off the client view that
/// holds the copy session, because copy mode is per client here.
pub(crate) const COPY_MODE_CONTEXT_FORMATS: [&str; 15] = [
    "copy_cursor_line",
    "copy_cursor_word",
    "copy_cursor_x",
    "copy_cursor_y",
    "scroll_position",
    "search_count",
    "search_count_partial",
    "search_match",
    "search_present",
    "search_timed_out",
    "selection_end_x",
    "selection_end_y",
    "selection_present",
    "selection_start_x",
    "selection_start_y",
];
pub(crate) const SHOW_MESSAGES_CONTEXT_FORMATS: [&str; 3] =
    ["message_number", "message_text", "message_time"];

#[derive(Default)]
pub(crate) struct StatusRenderer {
    shell_cache: BTreeMap<ShellCacheKey, String>,
    published: BTreeMap<ClientId, StatusLine>,
    tmux_shim: Option<PathBuf>,
    zz_executable: Option<PathBuf>,
}

pub(crate) struct StatusRequest {
    pub(crate) client: ClientId,
    pub(crate) formats: StatusFormats,
    pub(crate) row_formats: BTreeMap<u32, String>,
    pub(crate) option_snapshot: Arc<StatusRowVariables>,
    pub(crate) message_line: u8,
    pub(crate) customized: bool,
    pub(crate) title_format: Option<String>,
    pub(crate) environment: Vec<(RawText, Option<RawText>)>,
    pub(crate) default_terminal: String,
    pub(crate) startup: bool,
    pub(crate) context: StatusContext,
    pub(crate) facts: FormatHookFacts,
}

#[derive(Clone, Default)]
pub(crate) struct FormatHookFacts {
    pub(crate) terminals: Arc<BTreeMap<PaneId, Arc<TerminalSession>>>,
    pub(crate) pane_pipes: Arc<BTreeMap<PaneId, u32>>,
    pub(crate) session_attachments: Arc<BTreeMap<SessionId, (usize, String)>>,
    pub(crate) session_last_attached: Arc<BTreeMap<SessionId, u64>>,
    /// The panes carrying the pin's `PANE_UNSEENCHANGES`.
    pub(crate) unseen_changes: Arc<BTreeSet<PaneId>>,
    /// Every window some client's own current window is, with that client's
    /// name, in client order.
    pub(crate) window_clients: Arc<BTreeMap<WindowId, Vec<String>>>,
    pub(crate) buffer: Option<BufferFormatFacts>,
    pub(crate) client: Option<ClientFormatFacts>,
    pub(crate) clients: Arc<Vec<FormatClientRow>>,
    /// The environment of the client this expansion was created for, which is
    /// the invoking client for a command and the rendering client for a status
    /// line. `#{Vc:}` reads it.
    pub(crate) client_environment: Arc<Vec<FormatEnvironRow>>,
    pub(crate) message: Option<MessageFormatFacts>,
    pub(crate) mux: Arc<zz_mux::FormatFacts>,
    /// Every pane some client holds a live copy session on, with that client's
    /// name beside the facts, ordered by client id.
    pub(crate) copy_modes: Arc<BTreeMap<PaneId, Vec<(String, Arc<CopyModeFacts>)>>>,
}

#[derive(Clone)]
pub(crate) struct BufferFormatFacts {
    pub(crate) name: String,
    pub(crate) data: Arc<[u8]>,
    pub(crate) created: SystemTime,
}

#[derive(Clone, Default)]
pub(crate) struct ClientFormatFacts {
    pub(crate) activity: String,
    pub(crate) cell_height: String,
    pub(crate) cell_width: String,
    pub(crate) colours: String,
    pub(crate) control_mode: String,
    pub(crate) created: String,
    pub(crate) discarded: String,
    pub(crate) flags: String,
    pub(crate) height: String,
    pub(crate) key_table: String,
    pub(crate) last_session: String,
    pub(crate) name: String,
    pub(crate) pid: String,
    pub(crate) prefix: String,
    pub(crate) readonly: String,
    pub(crate) session: String,
    pub(crate) termfeatures: String,
    pub(crate) termname: String,
    pub(crate) termtype: String,
    pub(crate) theme: String,
    pub(crate) tty: String,
    pub(crate) uid: String,
    pub(crate) user: String,
    pub(crate) utf8: String,
    pub(crate) width: String,
    pub(crate) written: String,
    pub(crate) line: usize,
    pub(crate) environment: Vec<FormatEnvironRow>,
    /// The `struct tty_term` tmux would build for this client, which is what
    /// `#{I/c:}` and `#{I/f:}` interrogate. Absent for a client with no tty,
    /// which is `format_replace`'s null-term early exit.
    pub(crate) terminal: Option<TtyTerm>,
    pub(crate) viewport: Option<ClientViewportFacts>,
}

/// tty.c `tty_window_offset1`: the client's viewport over its own current
/// window, with the cursor of the pane that window is showing. `sx`/`sy` are
/// the tty minus the status lines, which is what the pin compares against the
/// window before it centres anything.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ClientViewportFacts {
    pub(crate) columns: u16,
    pub(crate) rows: u16,
    pub(crate) window_width: u16,
    pub(crate) window_height: u16,
    /// The active pane's cursor in window coordinates, `wp->xoff + cx` and
    /// `wp->yoff + cy`, absent while `MODE_CURSOR` is off.
    pub(crate) cursor: Option<(u16, u16)>,
}

impl ClientViewportFacts {
    fn bigger(self) -> bool {
        self.columns < self.window_width || self.rows < self.window_height
    }

    /// The `else` half of `tty_window_offset1`, which zz reaches with no pan
    /// window because `refresh-client -U/-D/-L/-R` is not implemented here.
    fn offsets(self) -> Option<(u16, u16)> {
        if !self.bigger() {
            return None;
        }
        let Some((cursor_x, cursor_y)) = self.cursor else {
            return Some((0, 0));
        };
        let offset_x = if cursor_x < self.columns {
            0
        } else if cursor_x > self.window_width.saturating_sub(self.columns) {
            self.window_width.saturating_sub(self.columns)
        } else {
            cursor_x.saturating_sub(self.columns / 2)
        };
        let offset_y = if cursor_y < self.rows {
            0
        } else if cursor_y > self.window_height.saturating_sub(self.rows) {
            self.window_height.saturating_sub(self.rows)
        } else {
            cursor_y.saturating_sub(self.rows).saturating_add(1)
        };
        Some((offset_x, offset_y))
    }
}

/// window.c `window_create` and `window_resize`: a window keeps the cell pixel
/// size of the client that sized it, and falls back to `DEFAULT_XPIXEL` and
/// `DEFAULT_YPIXEL` when no client reports one, which is what a pty client
/// leaves the pin reporting.
const DEFAULT_XPIXEL: u32 = 16;
const DEFAULT_YPIXEL: u32 = 32;

/// `tty_term_read_list` on the pin sets up the terminfo entry for the client's
/// TERM and writes each capability it finds as a `name=value` string. zz has no
/// curses linkage, so it reads the same entry through `infocmp -x`, which is
/// the only portable reader that also prints the extended section the tmux
/// capability names live in. The result depends on nothing but the TERM name,
/// so it is read once per name for the life of the process.
fn terminfo_entries(term: &str) -> Option<Arc<Vec<String>>> {
    static ENTRIES: OnceLock<Mutex<BTreeMap<String, Option<Arc<Vec<String>>>>>> = OnceLock::new();
    let cache = ENTRIES.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(cached) = cache.lock().ok()?.get(term) {
        return cached.clone();
    }
    let read = read_terminfo_entries(term).map(Arc::new);
    if read.is_none() {
        log::warn!(
            target: "zz_daemon::diagnostics::status",
            "no terminfo entry for TERM={term}; the client interrogate answers empty"
        );
    }
    if let Ok(mut cache) = cache.lock() {
        cache.insert(term.to_owned(), read.clone());
    }
    read
}

fn read_terminfo_entries(term: &str) -> Option<Vec<String>> {
    if term.is_empty() || term.contains('/') || term.starts_with('-') {
        return None;
    }
    let output = Command::new("infocmp")
        .args(["-x", "-1", term])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(parse_infocmp_entries(&text))
}

/// `infocmp -1` prints a string capability as `name=value` in the terminfo
/// source spelling, a number as `name#value` (hex under ncurses 6.1 and later
/// once it passes 255), a boolean as a bare `name` and a cancelled capability
/// as `name@`. `tty_term_read_list` carries the three live forms as
/// `name=value` with the string decoded the way `tigetstr` hands it back and
/// the number printed `%d`, and never sees a cancelled one.
fn parse_infocmp_entries(text: &str) -> Vec<String> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(line) = line.strip_suffix(',') else {
            continue;
        };
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            entries.push(format!("{name}={}", decode_terminfo_string(value)));
        } else if let Some((name, value)) = line.split_once('#') {
            let number = match value
                .strip_prefix("0x")
                .or_else(|| value.strip_prefix("0X"))
            {
                Some(hex) => i64::from_str_radix(hex, 16).ok(),
                None => value.parse::<i64>().ok(),
            };
            if let Some(number) = number {
                entries.push(format!("{name}={number}"));
            }
        } else if !line.ends_with('@') {
            entries.push(format!("{line}=1"));
        }
    }
    entries
}

/// terminfo(5) string escapes as `tigetstr` decodes them: `\E` and `\e` for
/// ESC, `^X` for a control character, `\NNN` octal, the C escapes, and the
/// backslashed punctuation the source syntax needs (`\,` `\:` `\^` `\\`).
/// `\0` is `\200` because terminfo has no NUL.
fn decode_terminfo_string(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        match byte {
            b'^' => {
                let Some(&control) = bytes.get(index) else {
                    out.push(byte);
                    break;
                };
                index += 1;
                out.push(if control == b'?' {
                    0x7f
                } else {
                    control & 0x1f
                });
            }
            b'\\' => {
                let Some(&next) = bytes.get(index) else {
                    out.push(byte);
                    break;
                };
                index += 1;
                let decoded = match next {
                    b'E' | b'e' => 0x1b,
                    b'n' | b'l' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'b' => 0x08,
                    b'f' => 0x0c,
                    b's' => b' ',
                    b'a' => 0x07,
                    b'0'..=b'7' => {
                        let mut number = u32::from(next - b'0');
                        for _ in 0..2 {
                            match bytes.get(index) {
                                Some(digit @ b'0'..=b'7') => {
                                    number = number * 8 + u32::from(digit - b'0');
                                    index += 1;
                                }
                                _ => break,
                            }
                        }
                        if number == 0 {
                            0x80
                        } else {
                            (number & 0xff) as u8
                        }
                    }
                    other => other,
                };
                out.push(decoded);
            }
            _ => out.push(byte),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read and cache the terminfo entry for `term` without building anything from
/// it. Reading the database is a subprocess, so registration warms the cache
/// while the state lock is down and the format path only ever reads it back.
pub(crate) fn warm_terminfo_entries(environment: &[RawText]) {
    for entry in environment {
        if let Some(term) = entry.strip_prefix("TERM=") {
            let _ = terminfo_entries(term);
            return;
        }
    }
}

/// The `struct tty_term` the pin would build for a client on `term`.
pub(crate) fn client_terminal_facts(
    term: &str,
    colour_term: Option<&str>,
    terminal_features: &[String],
    terminal_overrides: &[String],
) -> Option<TtyTerm> {
    let entries = terminfo_entries(term)?;
    Some(TtyTerm::create(
        term,
        &entries,
        colour_term,
        terminal_features,
        terminal_overrides,
    ))
}

/// A client's own process environment as `#{Vc:}` rows. A client store has no
/// hidden or removed entries: the client sends what it has.
pub(crate) fn client_environment_rows(
    environment: Option<&Arc<BTreeMap<RawText, RawText>>>,
) -> Vec<FormatEnvironRow> {
    environment.map_or_else(Vec::new, |environment| {
        environment
            .iter()
            .map(|(name, value)| FormatEnvironRow {
                name: name.to_string(),
                value: value.to_string(),
                hidden: false,
                removed: false,
            })
            .collect()
    })
}

impl ClientFormatFacts {
    /// The client formats an `L` row answers with, keyed the way the format
    /// engine looks them up.
    pub(crate) fn loop_row(&self, activity: u64) -> FormatClientRow {
        FormatClientRow {
            name: self.name.clone(),
            activity,
            environment: self.environment.clone(),
            variables: BTreeMap::from([
                ("client_activity".to_owned(), self.activity.clone()),
                ("client_cell_height".to_owned(), self.cell_height.clone()),
                ("client_cell_width".to_owned(), self.cell_width.clone()),
                ("client_colours".to_owned(), self.colours.clone()),
                ("client_control_mode".to_owned(), self.control_mode.clone()),
                ("client_created".to_owned(), self.created.clone()),
                ("client_discarded".to_owned(), self.discarded.clone()),
                ("client_flags".to_owned(), self.flags.clone()),
                ("client_height".to_owned(), self.height.clone()),
                ("client_key_table".to_owned(), self.key_table.clone()),
                ("client_last_session".to_owned(), self.last_session.clone()),
                ("client_name".to_owned(), self.name.clone()),
                ("client_pid".to_owned(), self.pid.clone()),
                ("client_prefix".to_owned(), self.prefix.clone()),
                ("client_readonly".to_owned(), self.readonly.clone()),
                ("client_session".to_owned(), self.session.clone()),
                ("client_termfeatures".to_owned(), self.termfeatures.clone()),
                ("client_termname".to_owned(), self.termname.clone()),
                ("client_termtype".to_owned(), self.termtype.clone()),
                ("client_theme".to_owned(), self.theme.clone()),
                ("client_tty".to_owned(), self.tty.clone()),
                ("client_uid".to_owned(), self.uid.clone()),
                ("client_user".to_owned(), self.user.clone()),
                ("client_utf8".to_owned(), self.utf8.clone()),
                ("client_width".to_owned(), self.width.clone()),
                ("client_written".to_owned(), self.written.clone()),
            ]),
        }
    }
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
        self.shell_cache.retain(|(scope, _, _), _| {
            !matches!(scope, ShellCacheScope::Attached(cached) if *cached == client)
        });
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
    let mut context = attached.map_or_else(
        || engine.format_status_context(None, focused_window, None),
        |client_session| {
            engine.format_status_context_for_client(
                Some(client_session),
                focused_window,
                None,
                client_session,
            )
        },
    );
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
    cache: &mut BTreeMap<ShellCacheKey, String>,
    touched: &mut BTreeSet<ShellCacheKey>,
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
                request.client,
                &request.facts,
                &request.context,
                Some(&request.option_snapshot),
                cache,
                touched,
                refresh,
                now,
                &request.environment,
                &request.default_terminal,
                request.startup,
                tmux_shim,
                zz_executable,
            );
            clamp_status_text(expand_status(format, &request.context, &mut hooks))
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
            request.client,
            &request.facts,
            &request.context,
            Some(&request.option_snapshot),
            cache,
            touched,
            refresh,
            now,
            &request.environment,
            &request.default_terminal,
            request.startup,
            tmux_shim,
            zz_executable,
        );
        (
            expand_status(&request.formats.left, &request.context, &mut hooks),
            expand_status(&request.formats.right, &request.context, &mut hooks),
        )
    };
    let mut hooks = DaemonFormatHooks::status(
        request.client,
        &request.facts,
        &request.context,
        Some(&request.option_snapshot),
        cache,
        touched,
        refresh,
        now,
        &request.environment,
        &request.default_terminal,
        request.startup,
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
                    clamp_status_text(expand_status(format, &request.context, &mut hooks))
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

/// The pin's window-scope availability rule: `format_cb_window_cell_width` and
/// its height twin answer null unless a window is in the format's context.
fn window_scoped(context: &StatusContext) -> bool {
    !context.window_id.is_empty()
        || !context.window_name.is_empty()
        || context.window_width.is_some()
}

fn window_cell_pixels(client: Option<&ClientFormatFacts>, width: bool) -> String {
    let default = if width {
        DEFAULT_XPIXEL
    } else {
        DEFAULT_YPIXEL
    };
    client
        .map(|client| {
            if width {
                &client.cell_width
            } else {
                &client.cell_height
            }
        })
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pixels| *pixels != 0)
        .unwrap_or(default)
        .to_string()
}

/// `MAX_STATUS_TEXT_BYTES` is the wire bound `StatusLine` enforces on its own
/// rows, so it belongs here rather than on every finished format expansion: a
/// command-facing expansion never rides this message.
fn clamp_status_text(mut value: String) -> String {
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

fn expand_base_status_style(
    formats: &StatusFormats,
    context: &StatusContext,
    hooks: &mut DaemonFormatHooks<'_>,
) -> String {
    let mut style = clamp_status_text(expand_status(&formats.style, context, hooks));
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
        return clamp_status_text(text.to_owned());
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
    status_client: Option<ClientId>,
    facts: &'a FormatHookFacts,
    option_engine: Option<&'a MuxEngine>,
    status_context: Option<&'a StatusContext>,
    variables: Option<&'a BTreeMap<String, String>>,
    command_item: Option<&'a str>,
    option_snapshot: Option<&'a StatusRowVariables>,
    cache: Option<&'a mut BTreeMap<ShellCacheKey, String>>,
    touched: Option<&'a mut BTreeSet<ShellCacheKey>>,
    refresh: bool,
    now: chrono::DateTime<Local>,
    environment: Option<&'a [(RawText, Option<RawText>)]>,
    default_terminal: Option<&'a str>,
    startup: bool,
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
            status_client: None,
            facts,
            option_engine: None,
            status_context: None,
            variables,
            command_item: None,
            option_snapshot: None,
            cache: None,
            touched: None,
            refresh: false,
            now: Local::now(),
            environment: None,
            default_terminal: None,
            startup: false,
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

    pub(crate) fn with_option_engine(mut self, engine: &'a MuxEngine) -> Self {
        self.option_engine = Some(engine);
        self
    }

    pub(crate) fn with_command_item(mut self, command: &'a str) -> Self {
        self.command_item = Some(command);
        self
    }

    fn status(
        client: ClientId,
        facts: &'a FormatHookFacts,
        context: &'a StatusContext,
        option_snapshot: Option<&'a StatusRowVariables>,
        cache: &'a mut BTreeMap<ShellCacheKey, String>,
        touched: &'a mut BTreeSet<ShellCacheKey>,
        refresh: bool,
        now: chrono::DateTime<Local>,
        environment: &'a [(RawText, Option<RawText>)],
        default_terminal: &'a str,
        startup: bool,
        tmux_shim: Option<&'a std::path::Path>,
        zz_executable: Option<&'a std::path::Path>,
    ) -> Self {
        Self {
            status_client: Some(client),
            facts,
            option_engine: None,
            status_context: Some(context),
            variables: None,
            command_item: None,
            option_snapshot,
            cache: Some(cache),
            touched: Some(touched),
            refresh,
            now,
            environment: Some(environment),
            default_terminal: Some(default_terminal),
            startup,
            tmux_shim,
            zz_executable,
        }
    }
}

impl DaemonFormatHooks<'_> {
    /// The clients holding a live copy session on this context's pane, in
    /// client order.
    fn copy_mode_rows(
        &self,
        context: &StatusContext,
    ) -> Option<&Vec<(String, Arc<CopyModeFacts>)>> {
        self.facts.copy_modes.get(&context.pane_id.parse().ok()?)
    }

    /// tmux reads `window_copy_formats` off the pane's single mode entry. zz
    /// keeps copy mode on the client's terminal view, so the format tree
    /// answers from the client it carries when that client is in the mode on
    /// this pane, and from the earliest client in the mode otherwise.
    fn copy_mode_view(&self, context: &StatusContext) -> Option<&CopyModeFacts> {
        let rows = self.copy_mode_rows(context)?;
        let client = self
            .facts
            .client
            .as_ref()
            .map(|client| client.name.as_str());
        client
            .and_then(|client| rows.iter().find(|(name, _)| name == client))
            .or_else(|| rows.first())
            .map(|(_, facts)| facts.as_ref())
    }

    fn copy_mode_variable(&self, name: &str, context: &StatusContext) -> Option<String> {
        let view = self.copy_mode_view(context)?;
        let [
            cursor_line,
            cursor_word,
            cursor_x,
            cursor_y,
            scroll_position,
            search_count,
            search_count_partial,
            search_match,
            search_present,
            search_timed_out,
            selection_end_x,
            selection_end_y,
            selection_present,
            selection_start_x,
            selection_start_y,
        ] = COPY_MODE_CONTEXT_FORMATS;
        if name == selection_present {
            return Some(
                if view.selection.is_some_and(|selection| {
                    selection.start_x != selection.end_x || selection.start_y != selection.end_y
                }) {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            );
        }
        match name {
            _ if name == search_present => Some(u8::from(view.search_present).to_string()),
            _ if name == search_timed_out => Some(u8::from(view.search_timed_out).to_string()),
            _ if name == search_match => Some(view.search_match.clone()),
            _ if name == search_count => Some(view.search_count?.0.to_string()),
            _ if name == search_count_partial => Some(u8::from(view.search_count?.1).to_string()),
            _ if name == cursor_line => Some(view.cursor_line.clone()),
            _ if name == cursor_word => Some(view.cursor_word.clone()),
            _ if name == cursor_x => Some(view.cursor_x.to_string()),
            _ if name == cursor_y => Some(view.cursor_y.to_string()),
            _ if name == scroll_position => Some(view.scroll_position.to_string()),
            _ if name == selection_start_x => Some(view.selection?.start_x.to_string()),
            _ if name == selection_start_y => Some(view.selection?.start_y.to_string()),
            _ if name == selection_end_x => Some(view.selection?.end_x.to_string()),
            _ if name == selection_end_y => Some(view.selection?.end_y.to_string()),
            _ => None,
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
        let Some(context) = self.status_context else {
            return String::new();
        };
        let Some(client) = self.status_client else {
            return String::new();
        };
        let cwd = status_working_directory(context);
        let scope = if self.facts.client.is_some() {
            ShellCacheScope::Attached(client)
        } else {
            ShellCacheScope::Unattached
        };
        let key = (scope, cwd.clone(), command.to_owned());
        touched.insert(key.clone());
        if !self.refresh
            && let Some(cached) = cache.get(&key)
        {
            return cached.clone();
        }
        let output = run_shell(
            command,
            context,
            &cwd,
            self.environment.unwrap_or_default(),
            self.default_terminal.unwrap_or("tmux-256color"),
            self.startup,
            self.tmux_shim,
            self.zz_executable,
        );
        cache.insert(key, output.clone());
        output
    }

    fn client_loop_rows(&mut self) -> Vec<FormatClientRow> {
        self.facts.clients.as_ref().clone()
    }

    fn client_environment_rows(&mut self) -> Vec<FormatEnvironRow> {
        self.facts.client_environment.as_ref().clone()
    }

    fn client_tty_term(&mut self) -> Option<TtyTerm> {
        self.facts.client.as_ref()?.terminal.clone()
    }

    fn client_terminal_environment(&mut self) -> Vec<FormatEnvironRow> {
        self.facts
            .client
            .as_ref()
            .map(|client| client.environment.clone())
            .unwrap_or_default()
    }

    /// `cmdq_merge_formats` copies the queue item's own entries into `ft->tree`
    /// before any command runs, which is `command` for every command and the
    /// hook variables on top of it inside a hook body.
    fn tree_entries(&mut self) -> Vec<(String, String)> {
        let mut entries = self
            .variables
            .into_iter()
            .flatten()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<Vec<_>>();
        if let Some(command) = self.command_item {
            entries.push(("command".to_owned(), command.to_owned()));
        }
        entries
    }

    fn variable(&mut self, name: &str, context: &StatusContext) -> Option<String> {
        if let Some(value) = self
            .option_engine
            .and_then(|engine| engine.format_option_value(context, name))
        {
            return Some(value);
        }
        if let Some(value) = self.option_snapshot.and_then(|options| {
            options.lookup(
                &context.session_id,
                &context.window_id,
                &context.pane_id,
                name,
            )
        }) {
            return Some(value);
        }
        if let Some(value) = self.variables.and_then(|variables| variables.get(name)) {
            return Some(value.clone());
        }
        if name == "command" {
            return self.command_item.map(str::to_owned);
        }
        if name.starts_with('@') {
            return self
                .facts
                .mux
                .user_option(
                    &context.pane_id,
                    &context.window_id,
                    &context.session_id,
                    name,
                )
                .map(str::to_owned);
        }
        let [list_clients_line] = LIST_CLIENTS_CONTEXT_FORMATS;
        let [message_number, message_text, message_time] = SHOW_MESSAGES_CONTEXT_FORMATS;
        if COPY_MODE_CONTEXT_FORMATS.contains(&name) {
            return self.copy_mode_variable(name, context);
        }
        match name {
            "pane_in_mode" => Some(
                if self.copy_mode_rows(context).is_some() {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            ),
            "pane_mode" => Some(
                if self.copy_mode_view(context)?.view_mode {
                    "view-mode"
                } else {
                    "copy-mode"
                }
                .to_owned(),
            ),
            "pane_kind" => self
                .facts
                .mux
                .pane_kind(&context.pane_id)
                .map(str::to_owned),
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
            "client_activity" => Some(self.facts.client.as_ref()?.activity.clone()),
            "client_cell_height" => Some(self.facts.client.as_ref()?.cell_height.clone()),
            "client_cell_width" => Some(self.facts.client.as_ref()?.cell_width.clone()),
            "client_colours" => Some(self.facts.client.as_ref()?.colours.clone()),
            "client_control_mode" => Some(self.facts.client.as_ref()?.control_mode.clone()),
            "client_created" => Some(self.facts.client.as_ref()?.created.clone()),
            "client_discarded" => Some(self.facts.client.as_ref()?.discarded.clone()),
            "client_flags" => Some(self.facts.client.as_ref()?.flags.clone()),
            "client_height" => Some(self.facts.client.as_ref()?.height.clone()),
            "client_key_table" => Some(self.facts.client.as_ref()?.key_table.clone()),
            // format_cb_client_last_session declines unless the client has a
            // last session that is still alive.
            "client_last_session" => Some(self.facts.client.as_ref()?.last_session.clone())
                .filter(|session| !session.is_empty()),
            "client_name" => Some(self.facts.client.as_ref()?.name.clone()),
            "client_pid" => Some(self.facts.client.as_ref()?.pid.clone()),
            "client_prefix" => Some(self.facts.client.as_ref()?.prefix.clone()),
            "client_readonly" => Some(self.facts.client.as_ref()?.readonly.clone()),
            "client_session" => Some(self.facts.client.as_ref()?.session.clone()),
            "client_termfeatures" => Some(self.facts.client.as_ref()?.termfeatures.clone()),
            "client_termname" => Some(self.facts.client.as_ref()?.termname.clone()),
            "client_termtype" => Some(self.facts.client.as_ref()?.termtype.clone()),
            // THEME_UNKNOWN is a NULL in format_cb_client_theme: the pin waits
            // for the terminal to report, and so does the daemon.
            "client_theme" => {
                Some(self.facts.client.as_ref()?.theme.clone()).filter(|theme| !theme.is_empty())
            }
            "client_tty" => Some(self.facts.client.as_ref()?.tty.clone()),
            "client_uid" => Some(self.facts.client.as_ref()?.uid.clone()),
            "client_user" => Some(self.facts.client.as_ref()?.user.clone()),
            "client_utf8" => Some(self.facts.client.as_ref()?.utf8.clone()),
            "client_width" => Some(self.facts.client.as_ref()?.width.clone()),
            "client_written" => Some(self.facts.client.as_ref()?.written.clone()),
            "window_bigger" => Some(
                if self.facts.client.as_ref()?.viewport?.bigger() {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            ),
            "window_offset_x" => Some(
                self.facts
                    .client
                    .as_ref()?
                    .viewport?
                    .offsets()?
                    .0
                    .to_string(),
            ),
            "window_offset_y" => Some(
                self.facts
                    .client
                    .as_ref()?
                    .viewport?
                    .offsets()?
                    .1
                    .to_string(),
            ),
            "window_cell_height" => window_scoped(context)
                .then(|| window_cell_pixels(self.facts.client.as_ref(), false)),
            "window_cell_width" => {
                window_scoped(context).then(|| window_cell_pixels(self.facts.client.as_ref(), true))
            }
            _ if name == list_clients_line => Some(self.facts.client.as_ref()?.line.to_string()),
            _ if name == message_number => Some(self.facts.message.as_ref()?.number.to_string()),
            _ if name == message_text => Some(self.facts.message.as_ref()?.text.clone()),
            _ if name == message_time => Some(
                self.facts
                    .message
                    .as_ref()?
                    .time
                    .duration_since(UNIX_EPOCH)
                    .ok()?
                    .as_secs()
                    .to_string(),
            ),
            "pane_pipe" => Some(
                if self
                    .facts
                    .pane_pipes
                    .contains_key(&context.pane_id.parse().ok()?)
                {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            ),
            "pane_unseen_changes" => Some(
                if self
                    .facts
                    .unseen_changes
                    .contains(&context.pane_id.parse().ok()?)
                {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            ),
            "pane_pipe_pid" => self
                .facts
                .pane_pipes
                .get(&context.pane_id.parse().ok()?)
                .map(u32::to_string),
            "pane_pb_progress" => Some(
                pane_progress_bar(self.facts, &context.pane_id)?
                    .progress
                    .to_string(),
            ),
            "pane_pb_state" => Some(
                pane_progress_bar(self.facts, &context.pane_id)?
                    .state
                    .as_str()
                    .to_owned(),
            ),
            "window_active_clients" => Some(
                self.facts
                    .window_clients
                    .get(&context.window_id.parse().ok()?)
                    .map_or(0, Vec::len)
                    .to_string(),
            ),
            "window_active_clients_list" => Some(
                self.facts
                    .window_clients
                    .get(&context.window_id.parse().ok()?)
                    .map_or_else(String::new, |names| names.join(",")),
            ),
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
            "session_last_attached" => self
                .facts
                .session_last_attached
                .get(&context.session_id.parse().ok()?)
                .copied()
                .filter(|time| *time != 0)
                .map(|time| time.to_string()),
            _ => None,
        }
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

/// `wp->base.progress_bar`, which lives on the pane's own screen: a pane with
/// no terminal worker has seen no OSC 9;4 and answers the defaults a fresh
/// screen carries.
fn pane_progress_bar(facts: &FormatHookFacts, pane: &str) -> Option<ProgressBar> {
    let pane = pane.parse().ok()?;
    Some(
        facts
            .terminals
            .get(&pane)
            .map(|terminal| terminal.progress_bar())
            .unwrap_or_default(),
    )
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
    context: &StatusContext,
    cwd: &Path,
    environment: &[(RawText, Option<RawText>)],
    default_terminal: &str,
    startup: bool,
    tmux_shim: Option<&std::path::Path>,
    zz_executable: Option<&std::path::Path>,
) -> String {
    let mut process = shell_process(command);
    let tmux = format!("{},{},-1", context.socket_path, std::process::id());
    configure_shell_job_environment(
        &mut process,
        environment,
        default_terminal,
        startup,
        &tmux,
        std::ffi::OsStr::new(&context.socket_path),
        tmux_shim,
        zz_executable,
    );
    process.current_dir(cwd).env("PWD", cwd.as_os_str());
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

fn status_working_directory(context: &StatusContext) -> PathBuf {
    let requested = Path::new(&context.session_path);
    if requested.is_dir() {
        requested.to_path_buf()
    } else {
        home_directory()
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| PathBuf::from("/"))
    }
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
            option_snapshot: Arc::new(StatusRowVariables::default()),
            message_line: 0,
            customized: false,
            title_format: None,
            environment: Vec::new(),
            default_terminal: "tmux-256color".to_owned(),
            startup: false,
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
            option_snapshot: Arc::new(engine.format_option_snapshot()),
            message_line: engine.message_line_for_session(session),
            customized: engine.status_customized_for_session(session),
            title_format: (session.is_some() && engine.set_titles_for_session(session))
                .then(|| engine.set_titles_string_for_session(session)),
            environment: engine.job_environment(None),
            default_terminal: engine.default_terminal_for_spawn().to_owned(),
            startup: false,
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
    fn daemon_delegated_format_consumers_match_mux_inventory() {
        let delegated = zz_mux::delegated_format_variable_names().collect::<Vec<_>>();
        assert_eq!(delegated.len(), 44);

        let session = SessionId(1);
        let pane = PaneId(1);
        let facts = FormatHookFacts {
            session_last_attached: Arc::new(BTreeMap::from([(session, 1)])),
            // pane_pipe_pid declines unless a pipe is attached, the way
            // format_cb_pane_pipe_pid gates on wp->pipe_fd.
            pane_pipes: Arc::new(BTreeMap::from([(pane, 4242)])),
            buffer: Some(BufferFormatFacts {
                name: String::new(),
                data: Arc::from([]),
                created: UNIX_EPOCH,
            }),
            client: Some(ClientFormatFacts {
                // client_last_session and client_theme decline when empty, the
                // way format_cb_client_last_session and format_cb_client_theme
                // do, so the inventory has to give them something.
                last_session: "last".to_owned(),
                theme: "dark".to_owned(),
                // The offsets only answer while the window is bigger than the
                // client viewport, which is the state this inventory needs.
                viewport: Some(ClientViewportFacts {
                    columns: 80,
                    rows: 23,
                    window_width: 200,
                    window_height: 60,
                    cursor: None,
                }),
                ..ClientFormatFacts::default()
            }),
            copy_modes: Arc::new(BTreeMap::from([(
                pane,
                vec![(String::new(), Arc::new(CopyModeFacts::default()))],
            )])),
            ..FormatHookFacts::default()
        };
        let context = StatusContext {
            session_id: session.to_string(),
            window_id: "@1".to_owned(),
            pane_id: pane.to_string(),
            ..StatusContext::default()
        };
        let mut hooks = DaemonFormatHooks::command(&facts);
        for name in delegated {
            assert!(
                hooks.variable(name, &context).is_some(),
                "daemon format hook does not consume {name}"
            );
        }
    }

    #[test]
    fn status_context_uses_the_attached_format_client() {
        let mut engine = MuxEngine::default();
        let mut execution = zz_mux::ExecutionContext::default();
        execute(
            &mut engine,
            &mut execution,
            &["new-session", "-s", "status-active"],
        );
        let session = execution.session.expect("session id");
        let snapshot = engine.state.snapshot();

        let attached = status_context(&snapshot, &engine, Some(session), None);
        assert_eq!(attached.session_active, Some(true));

        let detached = status_context(&snapshot, &engine, None, None);
        assert_eq!(detached.session_active, None);
    }

    #[test]
    fn status_option_snapshot_retargets_every_loop_after_the_engine_is_released() {
        let mut engine = MuxEngine::default();
        let (attached, first_window, first_pane) = engine.state.create_session("attached").unwrap();
        let second_pane = engine
            .state
            .split_pane(first_pane, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let (second_window, _) = engine
            .state
            .create_window_at(
                attached,
                None,
                Some("second".to_owned()),
                PaneKind::Terminal,
                false,
            )
            .unwrap();
        let (other, _, _) = engine.state.create_session("other").unwrap();
        let mut context =
            zz_mux::ExecutionContext::new(Some(attached), Some(first_window), Some(first_pane));
        let second_window = second_window.to_string();
        let first_pane = first_pane.to_string();

        execute(
            &mut engine,
            &mut context,
            &["set-option", "-g", "mouse", "off"],
        );
        execute(&mut engine, &mut context, &["set-option", "mouse", "on"]);
        execute(
            &mut engine,
            &mut context,
            &["set-window-option", "-g", "automatic-rename", "off"],
        );
        execute(
            &mut engine,
            &mut context,
            &[
                "set-window-option",
                "-t",
                &second_window,
                "automatic-rename",
                "on",
            ],
        );
        execute(
            &mut engine,
            &mut context,
            &["set-option", "-gp", "allow-set-title", "on"],
        );
        execute(
            &mut engine,
            &mut context,
            &[
                "set-option",
                "-p",
                "-t",
                &first_pane,
                "allow-set-title",
                "off",
            ],
        );

        let mut request = engine_request(1, &engine, Some(attached));
        request.formats.left =
            "OPTCHAIN:#{mouse}:#{S:#{mouse}}:#{W:#{automatic-rename}}:#{P:#{allow-set-title}}"
                .to_owned();
        request.formats.left_length = u16::MAX;
        assert!(
            request
                .option_snapshot
                .sessions
                .contains_key(&other.to_string())
        );
        assert!(request.option_snapshot.windows.contains_key(&second_window));
        assert!(
            request
                .option_snapshot
                .panes
                .contains_key(&second_pane.to_string())
        );
        drop(engine);

        let status = StatusRenderer::default().render_initial(&request);
        assert!(
            status.left.contains("OPTCHAIN:1:10:01:01"),
            "{}",
            status.left
        );
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
        assert!(five.rows[1].starts_with("#[align=left]    P: "));
        assert!(five.rows[2].starts_with("#[align=left]    S: "));
        assert!(!five.rows[1].contains("#{R:"));
        assert!(!five.rows[2].contains("#{R:"));
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

    /// The engine no longer clamps a finished expansion, because the pin has no
    /// output cap, so every `StatusLine` field this message owns has to hold the
    /// bound itself. Each of these formats expands well past it.
    #[test]
    fn oversized_row_title_and_base_style_stay_inside_the_wire_limit() {
        let overflow = "#{R:x,9000}";
        let mut request = request(1, overflow, overflow);
        request.formats.lines = 2;
        request.formats.style = format!("bold,{}", "italics,".repeat(900));
        request.title_format = Some(overflow.to_owned());
        request.row_formats = BTreeMap::from([(0, overflow.to_owned()), (1, overflow.to_owned())]);
        let status = StatusRenderer::default().render_initial(&request);
        assert_eq!(status.validate(), Ok(()));
        assert_eq!(status.title.len(), MAX_STATUS_TEXT_BYTES);
        assert_eq!(status.rows.len(), 2);
        for row in &status.rows {
            assert_eq!(row.len(), MAX_STATUS_TEXT_BYTES);
        }

        let unstyled = self::request(1, overflow, "");
        let status = StatusRenderer::default().render_initial(&unstyled);
        assert_eq!(status.validate(), Ok(()));
        assert!(status.left.len() <= MAX_STATUS_TEXT_BYTES);

        let mut markers = self::request(1, "#{R:#[bold],700}x", "");
        markers.formats.left_length = 10;
        let status = StatusRenderer::default().render_initial(&markers);
        assert_eq!(status.validate(), Ok(()));
        assert!(status.left.len() <= MAX_STATUS_TEXT_BYTES);
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

    /// `format_cb_window_cell_width` and its height twin publish `w->xpixel`
    /// and `w->ypixel`, which `window_create` and `window_resize` default to
    /// `DEFAULT_XPIXEL` and `DEFAULT_YPIXEL` when no client reports a cell size
    /// and `default_window_size` otherwise fills from `c->tty.xpixel`. Measured
    /// live against the pin on a throwaway server, both a CLI `display-message`
    /// with no client and one beside a pty client answer 16 and 32, and a
    /// `list-windows -F` row answers the same, while a format with no window in
    /// context answers null.
    #[test]
    fn window_cell_metrics_default_to_the_pin_size_and_follow_a_reporting_client() {
        let format = "#{window_cell_width}|#{window_cell_height}";
        let window = StatusContext {
            window_id: "@1".to_owned(),
            ..StatusContext::default()
        };

        let bare = FormatHookFacts::default();
        let mut hooks = DaemonFormatHooks::command(&bare);
        assert_eq!(expand_format_values(format, &window, &mut hooks), "16|32");

        let unsized_client = FormatHookFacts {
            client: Some(ClientFormatFacts {
                cell_width: "0".to_owned(),
                cell_height: String::new(),
                ..ClientFormatFacts::default()
            }),
            ..FormatHookFacts::default()
        };
        let mut hooks = DaemonFormatHooks::command(&unsized_client);
        assert_eq!(expand_format_values(format, &window, &mut hooks), "16|32");

        let reporting_client = FormatHookFacts {
            client: Some(ClientFormatFacts {
                cell_width: "9".to_owned(),
                cell_height: "18".to_owned(),
                ..ClientFormatFacts::default()
            }),
            ..FormatHookFacts::default()
        };
        let mut hooks = DaemonFormatHooks::command(&reporting_client);
        assert_eq!(expand_format_values(format, &window, &mut hooks), "9|18");

        let mut hooks = DaemonFormatHooks::command(&reporting_client);
        assert_eq!(
            expand_format_values(format, &StatusContext::default(), &mut hooks),
            "|"
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
    fn status_command_cache_is_scoped_to_the_working_directory() {
        let directory = tempfile::tempdir().expect("working directory fixture");
        let first_cwd = directory.path().join("first");
        let second_cwd = directory.path().join("second");
        std::fs::create_dir(&first_cwd).expect("first cwd");
        std::fs::create_dir(&second_cwd).expect("second cwd");
        let first_cwd = std::fs::canonicalize(first_cwd).expect("first cwd resolves");
        let second_cwd = std::fs::canonicalize(second_cwd).expect("second cwd resolves");
        let mut first = request(1, "#(pwd -P)", "");
        first.context.session_path = first_cwd.to_string_lossy().into_owned();
        let mut second = request(2, "#(pwd -P)", "");
        second.context.session_path = second_cwd.to_string_lossy().into_owned();

        let statuses = StatusRenderer::default().render_changed(&[first, second], false);

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].1.left, first_cwd.to_string_lossy());
        assert_eq!(statuses[1].1.left, second_cwd.to_string_lossy());
    }

    #[cfg(unix)]
    #[test]
    fn status_command_cache_survives_transient_clients_in_the_same_directory() {
        let directory = tempfile::tempdir().expect("working directory fixture");
        let source = directory.path().join("value");
        std::fs::write(&source, "first\n").expect("the first value is written");
        let format = format!("#(cat '{}')", source.display());
        let cwd = std::fs::canonicalize(directory.path()).expect("working directory resolves");
        let mut first = request(1, &format, "");
        first.context.session_path = cwd.to_string_lossy().into_owned();
        let mut renderer = StatusRenderer::default();

        assert_eq!(renderer.render_initial(&first).left, "first");
        renderer.forget(ClientId(1));
        std::fs::write(&source, "second\n").expect("the second value is written");
        let mut second = request(2, &format, "");
        second.context.session_path = cwd.to_string_lossy().into_owned();

        assert_eq!(renderer.render_initial(&second).left, "first");
        assert_eq!(renderer.shell_cache.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn status_command_cache_keeps_attached_clients_independent() {
        let directory = tempfile::tempdir().expect("working directory fixture");
        let source = directory.path().join("value");
        std::fs::write(&source, "first\n").expect("the first value is written");
        let format = format!("#(cat '{}')", source.display());
        let cwd = std::fs::canonicalize(directory.path()).expect("working directory resolves");
        let mut first = request(1, &format, "");
        first.context.session_path = cwd.to_string_lossy().into_owned();
        first.facts.client = Some(ClientFormatFacts::default());
        let mut renderer = StatusRenderer::default();

        assert_eq!(renderer.render_initial(&first).left, "first");
        std::fs::write(&source, "second\n").expect("the second value is written");
        let mut second = request(2, &format, "");
        second.context.session_path = cwd.to_string_lossy().into_owned();
        second.facts.client = Some(ClientFormatFacts::default());

        assert_eq!(renderer.render_initial(&second).left, "second");
        assert_eq!(renderer.shell_cache.len(), 2);
        renderer.forget(ClientId(1));
        assert_eq!(renderer.shell_cache.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn status_command_cache_prunes_working_directories_no_client_uses() {
        let directory = tempfile::tempdir().expect("working directory fixture");
        let first_cwd = directory.path().join("first");
        let second_cwd = directory.path().join("second");
        std::fs::create_dir(&first_cwd).expect("first cwd");
        std::fs::create_dir(&second_cwd).expect("second cwd");
        let first_cwd = std::fs::canonicalize(first_cwd).expect("first cwd resolves");
        let second_cwd = std::fs::canonicalize(second_cwd).expect("second cwd resolves");
        let mut first = request(1, "#(pwd -P)", "");
        first.context.session_path = first_cwd.to_string_lossy().into_owned();
        let mut second = request(2, "#(pwd -P)", "");
        second.context.session_path = second_cwd.to_string_lossy().into_owned();
        let mut renderer = StatusRenderer::default();

        renderer.render_changed(&[first, second], true);
        assert_eq!(renderer.shell_cache.len(), 2);
        let mut remaining = request(1, "#(pwd -P)", "");
        remaining.context.session_path = first_cwd.to_string_lossy().into_owned();
        renderer.render_changed(&[remaining], true);

        assert_eq!(renderer.shell_cache.len(), 1);
        assert_eq!(renderer.shell_cache.keys().next().unwrap().1, first_cwd);
    }

    #[cfg(unix)]
    #[test]
    fn status_commands_receive_tmux_and_working_directory_without_tmux_pane() {
        let directory = tempfile::Builder::new()
            .prefix("zz-status-environment-")
            .tempdir_in(".")
            .expect("the working directory is created");
        let pane_directory = tempfile::Builder::new()
            .prefix("zz-status-pane-decoy-")
            .tempdir_in(".")
            .expect("the pane working directory is created");
        let socket = "/tmp/zz-status-environment.sock";
        let mut status_request = request(
            1,
            "#(echo \"$TMUX|$ZZ_SOCKET|$PWD|${TMUX_PANE-unset}\")",
            "",
        );
        status_request.context.socket_path = socket.to_owned();
        status_request.context.session_path = directory
            .path()
            .canonicalize()
            .expect("the working directory resolves")
            .to_string_lossy()
            .into_owned();
        status_request.context.pane_current_path = pane_directory
            .path()
            .canonicalize()
            .expect("the pane working directory resolves")
            .to_string_lossy()
            .into_owned();

        let status = StatusRenderer::default().render_initial(&status_request);

        assert_eq!(
            status.left,
            format!(
                "{socket},{},-1|{socket}|{}|unset",
                std::process::id(),
                status_request.context.session_path
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn status_commands_use_global_only_clean_environment_and_the_startup_gate() {
        let mut engine = MuxEngine::default();
        engine.seed_global_environment([
            ("STATUS_SCOPE", "global"),
            ("TMUX_PANE", "status-global-pane"),
            ("TERM", "startup-term"),
            ("TERM_PROGRAM", "startup-program"),
            ("TERM_PROGRAM_VERSION", "startup-version"),
            ("COLORTERM", "startup-colorterm"),
            (crate::STARTUP_REENTRY_ENVIRONMENT_VARIABLE, "stale-reentry"),
            (
                crate::TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE,
                "stale-executable",
            ),
        ]);
        let mut execution = zz_mux::ExecutionContext::default();
        execute(
            &mut engine,
            &mut execution,
            &["new-session", "-d", "-s", "status-environment"],
        );
        let session = execution.session.expect("status session");
        for args in [
            vec!["set-environment", "-g", "-h", "STATUS_HIDDEN", "hidden"],
            vec!["set-environment", "-g", "-r", "STATUS_UNSET"],
            vec![
                "set-environment",
                "-t",
                "status-environment",
                "STATUS_SCOPE",
                "session",
            ],
            vec![
                "set-environment",
                "-t",
                "status-environment",
                "TMUX_PANE",
                "session-pane",
            ],
            vec!["set-option", "-g", "default-terminal", "status-terminal"],
        ] {
            execute(&mut engine, &mut execution, &args);
        }
        let command = "#(printf '%%s|%%s|%%s|%%s|%%s|%%s|%%s|%%s|%%s|%%s|%%s|%%s' \"$STATUS_SCOPE\" \"${STATUS_HIDDEN-unset}\" \"${STATUS_UNSET-unset}\" \"${HOME-unset}\" \"${TERM-unset}\" \"${TERM_PROGRAM-unset}\" \"${TERM_PROGRAM_VERSION-unset}\" \"${COLORTERM-unset}\" \"$TMUX\" \"$TMUX_PANE\" \"${ZZ_STARTUP_REENTRY-unset}\" \"${ZZ_TMUX_EXECUTABLE-unset}\")";
        let socket = "/tmp/zz-status-clean-environment.sock";
        let expected_tmux = format!("{socket},{},-1", std::process::id());

        let mut post_startup = engine_request(1, &engine, Some(session));
        post_startup.formats = StatusFormats {
            enabled: true,
            left: command.to_owned(),
            style: String::new(),
            left_style: String::new(),
            right_style: String::new(),
            foreground: "default".to_owned(),
            background: "default".to_owned(),
            left_length: u16::MAX,
            right_length: u16::MAX,
            ..StatusFormats::default()
        };
        post_startup.context.socket_path = socket.to_owned();
        let status = StatusRenderer::default().render_initial(&post_startup);
        assert_eq!(
            status.left,
            format!(
                "global|unset|unset|unset|status-terminal|tmux|3.8-zz|truecolor|{expected_tmux}|status-global-pane|unset|unset"
            )
        );

        let mut startup = engine_request(1, &engine, Some(session));
        startup.formats = StatusFormats {
            enabled: true,
            left: command.to_owned(),
            style: String::new(),
            left_style: String::new(),
            right_style: String::new(),
            foreground: "default".to_owned(),
            background: "default".to_owned(),
            left_length: u16::MAX,
            right_length: u16::MAX,
            ..StatusFormats::default()
        };
        startup.context.socket_path = socket.to_owned();
        startup.startup = true;
        let status = StatusRenderer::default().render_initial(&startup);
        assert_eq!(
            status.left,
            format!(
                "global|unset|unset|unset|startup-term|startup-program|startup-version|startup-colorterm|{expected_tmux}|status-global-pane|unset|unset"
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
            renderer
                .shell_cache
                .keys()
                .map(|(_, _, command)| command.as_str())
                .collect::<Vec<_>>(),
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
        let context = StatusContext::default();
        let cwd = status_working_directory(&context);

        let started = Instant::now();
        assert_eq!(
            run_shell(
                command,
                &context,
                &cwd,
                &[],
                "tmux-256color",
                false,
                None,
                None
            ),
            ""
        );
        assert!(
            started.elapsed() < SHELL_TIMEOUT * 3,
            "the timeout, not the command, bounds the render"
        );
    }
}

#[cfg(test)]
mod terminfo_tests {
    use super::*;

    #[test]
    fn the_infocmp_reader_finds_the_extended_section() {
        let Some(entries) = terminfo_entries("xterm-256color") else {
            return;
        };
        assert!(entries.iter().any(|entry| entry.starts_with("AX=")));
        assert!(entries.iter().any(|entry| entry.starts_with("colors=256")));
        assert!(
            entries
                .iter()
                .any(|entry| entry.starts_with("clear=\u{1b}["))
        );
        let term = client_terminal_facts(
            "xterm-256color",
            Some("truecolor"),
            &["xterm*:clipboard:ccolour:cstyle:focus:title".to_owned()],
            &[],
        )
        .expect("term");
        assert!(term.has_capability("smcup"));
        assert!(term.has_feature("RGB"));
        assert!(term.has_feature("bpaste"));
    }

    /// Measured on the pin on 2026-09-02 with COLORTERM unset and one pty
    /// client per TERM, against the stock `terminal-features` and
    /// `terminal-overrides` arrays: the entries without XT still get the
    /// VT100-like features off a decoded clear, the stock `linux*:AX@` row
    /// removes AX, and the stock `rxvt*:ignorefkeys` row cancels kf1.
    #[test]
    fn the_stock_arrays_move_the_other_terms_the_way_the_pin_does() {
        let engine = zz_mux::MuxEngine::default();
        let features = engine.terminal_features_option();
        let overrides = engine.terminal_overrides_option();
        let facts = |term: &str| client_terminal_facts(term, None, &features, &overrides);
        if let Some(screen) = facts("screen-256color") {
            assert!(screen.has_feature("bpaste"));
            assert!(screen.has_feature("focus"));
            assert!(screen.has_capability("Enfcs"));
            assert!(!screen.has_capability("XT"));
        }
        if let Some(tmux) = facts("tmux-256color") {
            assert!(tmux.has_feature("bpaste"));
            assert!(tmux.has_feature("focus"));
            assert!(tmux.has_capability("Enfcs"));
        }
        if let Some(linux) = facts("linux") {
            assert!(linux.has_feature("title"));
            assert!(!linux.has_capability("AX"));
        }
        if let Some(rxvt) = facts("rxvt") {
            assert!(!rxvt.has_capability("kf1"));
            assert!(!rxvt.has_capability("kf63"));
        }
    }

    /// ncurses 6.1 and later print a number past 255 in hex, and every string
    /// in the terminfo source spelling; `tty_term_read_list` sees neither.
    #[test]
    fn the_infocmp_reader_speaks_both_ncurses_generations() {
        let entries = parse_infocmp_entries(
            "#\tReconstructed via infocmp\nxterm-256color|xterm with 256 colors,\n\tam,\n\tcolors#0x100,\n\tpairs#65536,\n\tclear=\\E[H\\E[2J,\n\tkf1=\\EOP,\n\tbel=^G,\n\tacsc=``aaffggiijjkk\\,\\:\\^\\\\,\n\tsmcup@,\n\tU8#1,\n\tSs=\\E[%p1%d q$<5>,\n",
        );
        assert_eq!(
            entries,
            [
                "xterm-256color|xterm with 256 colors=1",
                "am=1",
                "colors=256",
                "pairs=65536",
                "clear=\u{1b}[H\u{1b}[2J",
                "kf1=\u{1b}OP",
                "bel=\u{7}",
                "acsc=``aaffggiijjkk,:^\\",
                "U8=1",
                "Ss=\u{1b}[%p1%d q$<5>",
            ]
        );
        let term = TtyTerm::create(
            "xterm-256color",
            &entries,
            None,
            &[],
            &["linux*:AX@".to_owned()],
        );
        assert!(term.has_capability("colors"));
        assert!(term.has_feature("bpaste"));
        assert!(!term.has_capability("smcup"));
    }
}
