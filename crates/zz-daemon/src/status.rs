//! Turning the `status-*` options into the text clients render.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::Read as _,
    process::{Child, Stdio},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

use chrono::Local;
use zz_mux::{MuxEngine, StatusContext, StatusFormats, StatusHooks, expand_status};
use zz_protocol::{Axis, ClientId, MuxSnapshot, SessionId, StatusLine, WindowId};

use crate::shell_process;

const SHELL_TIMEOUT: Duration = Duration::from_secs(2);
const SHELL_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_SHELL_OUTPUT_BYTES: u64 = 4 * 1024;

#[derive(Default)]
pub(crate) struct StatusRenderer {
    shell_cache: BTreeMap<String, String>,
    published: BTreeMap<ClientId, StatusLine>,
}

pub(crate) struct StatusRequest {
    pub(crate) client: ClientId,
    pub(crate) formats: StatusFormats,
    pub(crate) context: StatusContext,
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
            let status = render(&mut self.shell_cache, &mut touched, request, refresh);
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
        let status = render(&mut self.shell_cache, &mut touched, request, false);
        self.published.insert(request.client, status.clone());
        status
    }

    pub(crate) fn forget(&mut self, client: ClientId) {
        self.published.remove(&client);
    }
}

pub(crate) fn status_context(
    snapshot: &MuxSnapshot,
    engine: &MuxEngine,
    attached: Option<SessionId>,
    focused_window: Option<WindowId>,
) -> StatusContext {
    let (host, host_short) = host_names();
    let mut context = StatusContext {
        host: host.clone(),
        host_short: host_short.clone(),
        ..StatusContext::default()
    };
    let Some(session) = snapshot
        .sessions
        .iter()
        .find(|session| Some(session.id) == attached)
    else {
        return context;
    };
    context.session_name.clone_from(&session.name);
    context.session_windows = session.windows.len();

    let focused_window = focused_window
        .filter(|focused| session.windows.iter().any(|window| window.id == *focused))
        .unwrap_or(session.active_window);
    let Some(window) = session
        .windows
        .iter()
        .find(|window| window.id == focused_window)
    else {
        return context;
    };
    context.window_index = window.index;
    context.window_name.clone_from(&window.name);
    context.window_panes = window.panes.len();
    context.window_width = engine.window_extent(window.id, Axis::Horizontal);
    context.window_height = engine.window_extent(window.id, Axis::Vertical);
    context.window_active = Some(session.active_window == window.id);
    context.window_zoomed = window.zoomed_pane.is_some();
    context.window_bell = window.panes.values().any(|pane| pane.bell);

    let mut order = Vec::with_capacity(window.panes.len());
    window.layout.panes(&mut order);
    context.pane_index = order
        .iter()
        .position(|pane| *pane == window.active_pane)
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or_default();
    if let Some(pane) = window.panes.get(&window.active_pane) {
        context.pane_id = pane.id.to_string();
        context.pane_title.clone_from(&pane.title);
        if let Some((columns, rows)) = engine.pane_geometry(pane.id) {
            context.pane_width = Some(columns);
            context.pane_height = Some(rows);
        }
        context.pane_active = Some(window.active_pane == pane.id);
        context.pane_synchronized = pane.synchronized_input;
    }
    context
}

fn host_names() -> &'static (String, String) {
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
) -> StatusLine {
    if !request.formats.enabled {
        return StatusLine::default();
    }
    let now = Local::now();
    let mut hooks = DaemonHooks {
        cache,
        touched,
        refresh,
        now,
    };
    StatusLine {
        left: expand_status(&request.formats.left, &request.context, &mut hooks),
        right: expand_status(&request.formats.right, &request.context, &mut hooks),
    }
}

struct DaemonHooks<'a> {
    cache: &'a mut BTreeMap<String, String>,
    touched: &'a mut BTreeSet<String>,
    refresh: bool,
    now: chrono::DateTime<Local>,
}

impl StatusHooks for DaemonHooks<'_> {
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
        self.touched.insert(command.to_owned());
        if !self.refresh
            && let Some(cached) = self.cache.get(command)
        {
            return cached.clone();
        }
        let output = run_shell(command);
        self.cache.insert(command.to_owned(), output.clone());
        output
    }
}

fn run_shell(command: &str) -> String {
    let mut process = shell_process(command);
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

    fn request(client: u64, left: &str, right: &str) -> StatusRequest {
        StatusRequest {
            client: ClientId(client),
            formats: StatusFormats {
                enabled: true,
                interval: Duration::from_secs(15),
                left: left.to_owned(),
                right: right.to_owned(),
            },
            context: StatusContext {
                session_name: "work".to_owned(),
                ..StatusContext::default()
            },
        }
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
    fn a_disabled_status_renders_empty() {
        let mut renderer = StatusRenderer::default();
        let mut request = request(1, "[#S]", "%H");
        request.formats.enabled = false;
        let status = renderer.render_initial(&request);
        assert!(status.is_empty());
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
        assert_eq!(run_shell(command), "");
        assert!(
            started.elapsed() < SHELL_TIMEOUT * 3,
            "the timeout, not the command, bounds the render"
        );
    }
}
