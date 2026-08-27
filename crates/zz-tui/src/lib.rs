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
use zz_protocol::{
    CommandInvocation, CommandResponse, PreparedCommand, PreparedCommandResult, ProtocolMessage,
    ServerError, canonical_command, command_spec,
};
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
    pub read_only: bool,
    pub client_flags: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            host: None,
            session: None,
            restart_daemon: false,
            detach_others: false,
            read_only: false,
            client_flags: None,
        }
    }
}

/// A TUI run request with an optional client-local browser-frame source.
pub struct RunRequest<'a> {
    options: &'a RunOptions,
    browser_provider: Option<fn() -> Option<Box<dyn BrowserFrameProvider>>>,
    local_reconnect: Option<&'a dyn Fn(&Path, bool) -> Result<InteractiveClient, DaemonError>>,
}

impl RunOptions {
    /// Adds a main-thread-owned browser provider to this attach request.
    #[must_use]
    pub fn with_browser_provider(
        &self,
        browser_provider: fn() -> Option<Box<dyn BrowserFrameProvider>>,
    ) -> RunRequest<'_> {
        RunRequest {
            options: self,
            browser_provider: Some(browser_provider),
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
        reconnect: &'a dyn Fn(&Path, bool) -> Result<InteractiveClient, DaemonError>,
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
    let resolved = resolve_run(options)?;
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    let initial = initial_connection(
        &resolved.endpoint,
        &options.socket_path,
        interactive,
        options.restart_daemon,
        request.local_reconnect,
    )?;
    resolve_attach_target(&initial, options.session.as_deref())?;
    if !interactive {
        if options.client_flags.is_some() {
            return request_headless_attach(&initial, options);
        }
        return Err(Error::message("open terminal failed: not a terminal"));
    }
    let browser_provider = request.browser_provider.and_then(|provider| provider());
    app::run(
        initial,
        resolved.endpoint,
        resolved.local_endpoint,
        app::InitialAttach::Request {
            target: options.session.clone(),
            detach_others: options.detach_others,
            read_only: options.read_only,
            client_flags: options.client_flags.clone(),
        },
        resolved.host_label,
        resolved.local_host_label,
        resolved.fleet_hosts,
        browser_provider,
    )
    .map_err(Error::message)
}

pub fn run_new_session<'a>(
    request: impl Into<RunRequest<'a>>,
    invocations: impl IntoIterator<Item = CommandInvocation>,
) -> Result<(), Error> {
    run_new_session_commands(request, invocations.into_iter().map(NewSessionCommand::Raw))
}

pub fn run_prepared_new_session<'a>(
    request: impl Into<RunRequest<'a>>,
    commands: impl IntoIterator<Item = PreparedCommand>,
) -> Result<(), Error> {
    run_new_session_commands(
        request,
        commands.into_iter().map(NewSessionCommand::Prepared),
    )
}

fn run_new_session_commands<'a>(
    request: impl Into<RunRequest<'a>>,
    commands: impl IntoIterator<Item = NewSessionCommand>,
) -> Result<(), Error> {
    let request = request.into();
    let options = request.options;
    let resolved = resolve_run(options)?;
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    let initial = initial_connection(
        &resolved.endpoint,
        &options.socket_path,
        interactive,
        options.restart_daemon,
        request.local_reconnect,
    )?;
    match execute_new_session(&initial, commands)? {
        NewSessionOutcome::Detached => Ok(()),
        NewSessionOutcome::Attached {
            session,
            messages,
            reconnect,
        } => {
            let (read_only, client_flags) = reconnect.into_attach_arguments();
            let browser_provider = request.browser_provider.and_then(|provider| provider());
            app::run(
                initial,
                resolved.endpoint,
                resolved.local_endpoint,
                app::InitialAttach::AlreadyAttached {
                    session,
                    messages,
                    read_only,
                    client_flags,
                },
                resolved.host_label,
                resolved.local_host_label,
                resolved.fleet_hosts,
                browser_provider,
            )
            .map_err(Error::message)
        }
    }
}

pub fn run_cli(arguments: impl IntoIterator<Item = String>) -> Result<(), Error> {
    let options = parse_arguments(arguments)?;
    let reconnect =
        |path: &Path, client_has_terminal| spawn_and_connect_daemon(path, client_has_terminal);
    run(RunRequest::from(&options).with_local_reconnect(&reconnect))
}

struct ResolvedRun {
    endpoint: Endpoint,
    local_endpoint: Endpoint,
    host_label: String,
    local_host_label: String,
    fleet_hosts: Vec<zz_daemon::HostEntry>,
}

fn resolve_run(options: &RunOptions) -> Result<ResolvedRun, Error> {
    let (fleet_hosts, _) = configured_fleet_hosts()
        .map_err(|error| Error::message(format!("could not read zz/config: {error}")))?;
    let endpoint = resolve_endpoint(options, &fleet_hosts)?;
    let local_endpoint = Endpoint::Local(options.socket_path.clone());
    let local_host_label = short_device_name().unwrap_or_else(|| "localhost".to_owned());
    let host_label = options
        .host
        .clone()
        .unwrap_or_else(|| local_host_label.clone());
    Ok(ResolvedRun {
        endpoint,
        local_endpoint,
        host_label,
        local_host_label,
        fleet_hosts,
    })
}

enum NewSessionOutcome {
    Detached,
    Attached {
        session: zz_protocol::SessionId,
        messages: Vec<ProtocolMessage>,
        reconnect: ReconnectAttachState,
    },
}

enum NewSessionCommand {
    Raw(CommandInvocation),
    Prepared(PreparedCommand),
}

#[derive(Default)]
struct ReconnectAttachState {
    mutations: Vec<String>,
    read_only: bool,
}

impl ReconnectAttachState {
    fn observe(
        &mut self,
        invocation: &CommandInvocation,
        prepared_name: Option<&str>,
        succeeded: bool,
        attached: bool,
    ) {
        if !succeeded || !attached {
            return;
        }
        let name = prepared_name.unwrap_or_else(|| canonical_command(&invocation.name));
        if !matches!(name, "attach-session" | "new-session") {
            return;
        }
        let (mutation, read_only) = attaching_options(name, invocation);
        if let Some(mutation) = mutation {
            self.mutations.push(mutation);
        }
        self.read_only |= read_only;
    }

    fn into_attach_arguments(self) -> (bool, Option<String>) {
        (
            self.read_only,
            (!self.mutations.is_empty()).then(|| self.mutations.join(",")),
        )
    }
}

fn attaching_options(name: &str, invocation: &CommandInvocation) -> (Option<String>, bool) {
    let Some(spec) = command_spec(name) else {
        return (None, false);
    };
    let mut mutation = None;
    let mut read_only = false;
    let mut index = 0;
    while let Some(argument) = invocation.args.get(index) {
        if !argument.starts_with('-') || argument == "-" {
            break;
        }
        index += 1;
        if argument == "--" {
            break;
        }
        if argument.starts_with("--") {
            continue;
        }
        let mut cluster = argument[1..].chars();
        while let Some(character) = cluster.next() {
            let option_name = format!("-{character}");
            if name == "attach-session" && option_name == "-r" {
                read_only = true;
            }
            let takes_value = spec
                .option(&option_name)
                .is_some_and(|option| option.value.is_some());
            if !takes_value {
                continue;
            }
            let attached = cluster.as_str();
            let value = if attached.is_empty() {
                let Some(value) = invocation.args.get(index) else {
                    return (mutation, read_only);
                };
                index += 1;
                value.clone()
            } else {
                attached.to_owned()
            };
            if option_name == "-f" {
                mutation = Some(value);
            }
            break;
        }
    }
    (mutation, read_only)
}

fn execute_new_session(
    client: &InteractiveClient,
    commands: impl IntoIterator<Item = NewSessionCommand>,
) -> Result<NewSessionOutcome, Error> {
    let mut attached_session = None;
    let mut messages = Vec::new();
    let mut reconnect = ReconnectAttachState::default();
    'commands: for command in commands {
        let (request, invocation, prepared_name) = match command {
            NewSessionCommand::Raw(invocation) => {
                (client.execute(invocation.clone()), invocation, None)
            }
            NewSessionCommand::Prepared(PreparedCommand {
                invocation,
                canonical_name,
                result: PreparedCommandResult::Ready,
                ..
            }) => (
                client.execute_prepared(invocation.clone()),
                invocation,
                canonical_name,
            ),
            NewSessionCommand::Prepared(PreparedCommand {
                result: PreparedCommandResult::Error(error),
                ..
            }) => {
                if attached_session.is_none() {
                    return Err(Error::message(error.tmux_message()));
                }
                messages.push(ProtocolMessage::CommandResponse(CommandResponse::Error {
                    request_id: u64::MAX,
                    error,
                    output: String::new(),
                }));
                break 'commands;
            }
        };
        let request_id = request.map_err(|error| Error::message(error.to_string()))?;
        let mut attached = false;
        loop {
            let message = client
                .recv()
                .map_err(|error| Error::message(error.to_string()))?;
            match message {
                ProtocolMessage::CommandResponse(CommandResponse::Success {
                    request_id: response_id,
                    output,
                    exit_code,
                    ..
                }) if response_id == request_id => {
                    print_command_output(&output)?;
                    if exit_code != 0 {
                        return Err(Error::message(format!(
                            "command exited with status {exit_code}"
                        )));
                    }
                    reconnect.observe(&invocation, prepared_name.as_deref(), true, attached);
                    break;
                }
                ProtocolMessage::CommandResponse(CommandResponse::Error {
                    request_id: response_id,
                    error,
                    output,
                }) if response_id == request_id => {
                    print_command_output(&output)?;
                    if attached_session.is_none() {
                        return Err(Error::message(error.tmux_message()));
                    }
                    messages.push(ProtocolMessage::CommandResponse(CommandResponse::Error {
                        request_id: response_id,
                        error,
                        output,
                    }));
                    break 'commands;
                }
                message @ ProtocolMessage::Attached { session, .. } => {
                    attached = true;
                    attached_session = Some(session);
                    messages.push(message);
                }
                message => messages.push(message),
            }
        }
    }
    Ok(match attached_session {
        Some(session) => NewSessionOutcome::Attached {
            session,
            messages,
            reconnect,
        },
        None => NewSessionOutcome::Detached,
    })
}

fn print_command_output(output: &str) -> Result<(), Error> {
    if output.is_empty() {
        return Ok(());
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(output.as_bytes())?;
    if !output.ends_with('\n') {
        stdout.write_all(b"\n")?;
    }
    stdout.flush()?;
    Ok(())
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
    reconnect: Option<&dyn Fn(&Path, bool) -> Result<InteractiveClient, DaemonError>>,
) -> Result<InteractiveClient, Error> {
    match InteractiveClient::connect_endpoint_with_terminal(
        endpoint,
        TerminalColorScheme::Dark,
        interactive,
    ) {
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
                return reconnect.ok_or_else(|| Error::message(error.to_string()))?(
                    local_socket,
                    interactive,
                )
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
                interactive,
            )
            .map_err(|restart| Error::message(format!("daemon restart failed: {restart}")))
        }
        Err(error) => Err(Error::message(error.to_string())),
    }
}

fn resolve_attach_target(client: &InteractiveClient, target: Option<&str>) -> Result<(), Error> {
    let arguments = attach_preflight_arguments(target);
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
                error,
                ..
            }) if response_id == request_id => {
                if target.is_none() && matches!(error, ServerError::SessionNotFound(_)) {
                    return Err(Error::message("no sessions"));
                }
                return Err(Error::message(error.to_string()));
            }
            _ => {}
        }
    }
}

fn request_headless_attach(client: &InteractiveClient, options: &RunOptions) -> Result<(), Error> {
    let request_id = client
        .request_attach_session(
            options.session.clone().unwrap_or_default(),
            options.detach_others,
            options.read_only,
            options.client_flags.as_deref(),
        )
        .map_err(|error| Error::message(error.to_string()))?;
    loop {
        match client
            .recv()
            .map_err(|error| Error::message(error.to_string()))?
        {
            ProtocolMessage::CommandResponse(CommandResponse::Success {
                request_id: response_id,
                ..
            }) if response_id == request_id => {
                return Err(Error::message("open terminal failed: not a terminal"));
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

fn attach_preflight_arguments(target: Option<&str>) -> Vec<String> {
    target.map_or_else(Vec::new, |target| vec!["-t".to_owned(), target.to_owned()])
}

fn spawn_and_connect_daemon(
    path: &Path,
    client_has_terminal: bool,
) -> Result<InteractiveClient, DaemonError> {
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
        match InteractiveClient::connect_terminal_surface(
            path,
            TerminalColorScheme::Dark,
            client_has_terminal,
        ) {
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

    #[test]
    fn targetless_attach_preflights_the_current_session() {
        assert_eq!(attach_preflight_arguments(None), Vec::<String>::new());
        assert_eq!(
            attach_preflight_arguments(Some("work")),
            vec!["-t".to_owned(), "work".to_owned()]
        );
    }

    #[test]
    fn reconnect_replays_the_successful_attach_before_a_missing_target() {
        let mut reconnect = ReconnectAttachState::default();
        reconnect.observe(
            &CommandInvocation::new("new-session", ["-s", "fresh", "-f", "ignore-size"]),
            None,
            true,
            true,
        );
        reconnect.observe(
            &CommandInvocation::new("attach-session", ["-t", "missing", "-f", "!ignore-size"]),
            None,
            false,
            false,
        );
        assert_eq!(
            reconnect.into_attach_arguments(),
            (false, Some("ignore-size".to_owned()))
        );
    }

    #[test]
    fn reconnect_ignores_a_detached_new_session_a_miss_before_attach() {
        let mut reconnect = ReconnectAttachState::default();
        reconnect.observe(
            &CommandInvocation::new(
                "new-session",
                ["-A", "-d", "-s", "missing", "-f", "active-pane"],
            ),
            None,
            true,
            false,
        );
        reconnect.observe(
            &CommandInvocation::new(
                "att",
                [
                    "-r",
                    "-f",
                    "active-pane",
                    "-fignore-size,no-detach-on-destroy",
                ],
            ),
            Some("attach-session"),
            true,
            true,
        );
        assert_eq!(
            reconnect.into_attach_arguments(),
            (true, Some("ignore-size,no-detach-on-destroy".to_owned()))
        );
    }
}
