//! Raw-terminal presentation client for daemon-owned zz sessions.

mod app;
pub mod browser;
mod clipboard;
mod input;
mod kitty;
mod layout;
mod picker;
mod render;
mod sidebar;
mod state;
mod terminal_event;
mod tty;

use std::{
    error::Error as StdError,
    fmt,
    io::{self, IsTerminal as _, Write as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use zz_daemon::{
    DaemonError, Endpoint, InteractiveClient, classify_local_connect_error, configured_fleet_hosts,
    default_socket_path, short_device_name, terminate_incompatible_daemon,
};
use zz_protocol::{CommandInvocation, CommandResponse, ProtocolMessage, ServerError};
use zz_terminal::TerminalColorScheme;

use crate::browser::BrowserFrameProvider;

const USAGE: &str =
    "usage: zz-tui [--socket <path> | --host <name>] attach [--restart-daemon] [session]";
const MANUAL_RESTART_HINT: &str = "run 'zz kill-server' to restart it (sessions will be lost)";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunOptions {
    pub socket_path: PathBuf,
    pub host: Option<String>,
    pub session: Option<String>,
    pub restart_daemon: bool,
    pub detach_others: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            host: None,
            session: None,
            restart_daemon: false,
            detach_others: false,
        }
    }
}

/// A TUI run request with an optional client-local browser-frame source.
pub struct RunRequest<'a> {
    options: &'a RunOptions,
    browser_provider: Option<Box<dyn BrowserFrameProvider>>,
    local_reconnect: Option<&'a dyn Fn(&Path) -> Result<InteractiveClient, DaemonError>>,
}

impl RunOptions {
    /// Adds a main-thread-owned browser provider to this attach request.
    #[must_use]
    pub fn with_browser_provider(
        &self,
        browser_provider: Option<Box<dyn BrowserFrameProvider>>,
    ) -> RunRequest<'_> {
        RunRequest {
            options: self,
            browser_provider,
            local_reconnect: None,
        }
    }
}

impl<'a> From<&'a RunOptions> for RunRequest<'a> {
    fn from(options: &'a RunOptions) -> Self {
        Self {
            options,
            browser_provider: None,
            local_reconnect: None,
        }
    }
}

impl<'a> RunRequest<'a> {
    /// Supplies the launcher used after a verified stale local daemon is terminated.
    #[must_use]
    pub fn with_local_reconnect(
        mut self,
        reconnect: &'a dyn Fn(&Path) -> Result<InteractiveClient, DaemonError>,
    ) -> Self {
        self.local_reconnect = Some(reconnect);
        self
    }
}

#[derive(Debug)]
pub struct Error(String);

impl Error {
    fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl StdError for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}

pub fn run<'a>(request: impl Into<RunRequest<'a>>) -> Result<(), Error> {
    let request = request.into();
    let options = request.options;
    let (fleet_hosts, _) = configured_fleet_hosts()
        .map_err(|error| Error::message(format!("could not read zz/config: {error}")))?;
    let endpoint = resolve_endpoint(options, &fleet_hosts)?;
    let local_endpoint = Endpoint::Local(options.socket_path.clone());
    let local_host_label = short_device_name().unwrap_or_else(|| "localhost".to_owned());
    let host_label = options
        .host
        .clone()
        .unwrap_or_else(|| local_host_label.clone());
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    let initial = initial_connection(
        &endpoint,
        &options.socket_path,
        interactive,
        options.restart_daemon,
        request.local_reconnect,
    )?;
    resolve_attach_target(&initial, options.session.as_deref())?;
    if !interactive {
        return Err(Error::message("attach requires an interactive terminal"));
    }
    app::run(
        initial,
        endpoint,
        local_endpoint,
        options.session.as_deref(),
        options.detach_others,
        host_label,
        local_host_label,
        fleet_hosts,
        request.browser_provider,
    )
    .map_err(Error::message)
}

pub fn run_cli(arguments: impl IntoIterator<Item = String>) -> Result<(), Error> {
    let options = parse_arguments(arguments)?;
    let reconnect = |path: &Path| spawn_and_connect_daemon(path);
    run(RunRequest::from(&options).with_local_reconnect(&reconnect))
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<RunOptions, Error> {
    let mut options = RunOptions::default();
    let mut socket_overridden = false;
    let mut positional = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--host" => {
                options.host = Some(
                    arguments
                        .next()
                        .filter(|name| !name.is_empty())
                        .ok_or_else(|| Error::message("--host requires a name"))?,
                );
            }
            "--socket" => {
                options.socket_path = PathBuf::from(
                    arguments
                        .next()
                        .filter(|path| !path.is_empty())
                        .ok_or_else(|| Error::message("--socket requires a path"))?,
                );
                socket_overridden = true;
            }
            "--restart-daemon" => options.restart_daemon = true,
            _ if argument.starts_with("--host=") => {
                let name = argument.trim_start_matches("--host=");
                if name.is_empty() {
                    return Err(Error::message("--host requires a name"));
                }
                options.host = Some(name.to_owned());
            }
            _ if argument.starts_with("--socket=") => {
                let path = argument.trim_start_matches("--socket=");
                if path.is_empty() {
                    return Err(Error::message("--socket requires a path"));
                }
                options.socket_path = PathBuf::from(path);
                socket_overridden = true;
            }
            _ if argument.starts_with('-') => return Err(Error::message(USAGE)),
            _ => positional.push(argument),
        }
    }
    if socket_overridden && options.host.is_some() {
        return Err(Error::message(
            "--host cannot be used together with --socket",
        ));
    }
    if options.restart_daemon && options.host.is_some() {
        return Err(Error::message(
            "--restart-daemon is only supported for the local daemon",
        ));
    }
    let [command, session @ ..] = positional.as_slice() else {
        return Err(Error::message(USAGE));
    };
    if command != "attach" || session.len() > 1 {
        return Err(Error::message(USAGE));
    }
    options.session = session.first().cloned();
    Ok(options)
}

fn initial_connection(
    endpoint: &Endpoint,
    local_socket: &Path,
    interactive: bool,
    restart_daemon: bool,
    reconnect: Option<&dyn Fn(&Path) -> Result<InteractiveClient, DaemonError>>,
) -> Result<InteractiveClient, Error> {
    match InteractiveClient::connect_endpoint(endpoint, TerminalColorScheme::Dark) {
        Ok(client) => Ok(client),
        Err(error) if matches!(endpoint, Endpoint::Local(_)) => {
            let error = classify_local_connect_error(local_socket, error);
            if matches!(
                &error,
                DaemonError::Io(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound
                            | io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::ConnectionReset
                    )
            ) {
                return reconnect.ok_or_else(|| Error::message(error.to_string()))?(local_socket)
                    .map_err(|restart| Error::message(format!("daemon start failed: {restart}")));
            }
            let DaemonError::IncompatibleDaemon { .. } = error else {
                return Err(Error::message(error.to_string()));
            };
            if !interactive {
                return Err(Error::message(format!("{error}\n{MANUAL_RESTART_HINT}")));
            }
            let confirmed = if restart_daemon {
                true
            } else {
                eprint!("zz: {error}\nRestart the daemon? Running sessions will be lost. [y/N] ");
                io::stderr().flush()?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                answer.trim().eq_ignore_ascii_case("y")
            };
            if !confirmed {
                return Err(Error::message(MANUAL_RESTART_HINT));
            }
            terminate_incompatible_daemon(local_socket)
                .map_err(|restart| Error::message(format!("{error}; restart failed: {restart}")))?;
            reconnect.ok_or_else(|| Error::message("no daemon launcher is available"))?(
                local_socket,
            )
            .map_err(|restart| Error::message(format!("daemon restart failed: {restart}")))
        }
        Err(error) => Err(Error::message(error.to_string())),
    }
}

fn resolve_attach_target(client: &InteractiveClient, target: Option<&str>) -> Result<(), Error> {
    let arguments = target.map_or_else(Vec::new, |target| vec!["-t".to_owned(), target.to_owned()]);
    let request_id = client
        .execute(CommandInvocation::new("has-session", arguments))
        .map_err(|error| Error::message(error.to_string()))?;
    loop {
        match client
            .recv()
            .map_err(|error| Error::message(error.to_string()))?
        {
            ProtocolMessage::CommandResponse(CommandResponse::Success {
                request_id: response_id,
                exit_code: 0,
                ..
            }) if response_id == request_id => return Ok(()),
            ProtocolMessage::CommandResponse(CommandResponse::Success {
                request_id: response_id,
                exit_code,
                ..
            }) if response_id == request_id => {
                return Err(Error::message(format!(
                    "command exited with status {exit_code}"
                )));
            }
            ProtocolMessage::CommandResponse(CommandResponse::Error {
                request_id: response_id,
                error: ServerError::SessionNotFound(_),
                ..
            }) if response_id == request_id && target.is_none() => {
                return Err(Error::message("no sessions"));
            }
            ProtocolMessage::CommandResponse(CommandResponse::Error {
                request_id: response_id,
                error,
                ..
            }) if response_id == request_id => return Err(Error::message(error.to_string())),
            _ => {}
        }
    }
}

fn spawn_and_connect_daemon(path: &Path) -> Result<InteractiveClient, DaemonError> {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(|directory| directory.join("zz")))
        .filter(|executable| executable.is_file())
        .unwrap_or_else(|| PathBuf::from("zz"));
    let mut command = Command::new(executable);
    command.arg("--socket").arg(path).arg("daemon");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;

        command.process_group(0);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match InteractiveClient::connect_with_color_scheme(path, TerminalColorScheme::Dark) {
            Ok(client) => return Ok(client),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn resolve_endpoint(
    options: &RunOptions,
    hosts: &[zz_daemon::HostEntry],
) -> Result<Endpoint, Error> {
    let Some(name) = options.host.as_deref() else {
        return Ok(Endpoint::Local(options.socket_path.clone()));
    };
    if let Some(host) = hosts.iter().find(|host| host.name == name) {
        return Ok(host.endpoint.clone());
    }
    let known = if hosts.is_empty() {
        "(none)".to_owned()
    } else {
        hosts
            .iter()
            .map(|host| host.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Err(Error::message(format!(
        "unknown fleet host `{name}`; known hosts: {known}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_accepts_local_and_named_host_attach_forms() {
        let local = parse_arguments(["attach".to_owned(), "work".to_owned()]).unwrap();
        assert_eq!(local.session.as_deref(), Some("work"));
        assert!(local.host.is_none());
        assert!(!local.restart_daemon);
        assert!(!local.detach_others);

        let remote =
            parse_arguments(["--host".to_owned(), "box".to_owned(), "attach".to_owned()]).unwrap();
        assert_eq!(remote.host.as_deref(), Some("box"));
        assert!(remote.session.is_none());

        let restart = parse_arguments([
            "attach".to_owned(),
            "--restart-daemon".to_owned(),
            "work".to_owned(),
        ])
        .unwrap();
        assert!(restart.restart_daemon);
        assert_eq!(restart.session.as_deref(), Some("work"));
    }

    #[test]
    fn cli_rejects_conflicting_or_extra_arguments() {
        assert!(
            parse_arguments([
                "--host=box".to_owned(),
                "--socket=/tmp/zz.sock".to_owned(),
                "attach".to_owned(),
            ])
            .is_err()
        );
        assert!(
            parse_arguments(["attach".to_owned(), "one".to_owned(), "two".to_owned(),]).is_err()
        );
        assert!(
            parse_arguments([
                "--host=box".to_owned(),
                "--restart-daemon".to_owned(),
                "attach".to_owned(),
            ])
            .is_err()
        );
    }
}
