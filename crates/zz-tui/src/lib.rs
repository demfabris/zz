//! Raw-terminal presentation client for daemon-owned zz sessions.

mod app;
pub mod browser;
mod clipboard;
mod input;
mod kitty;
mod layout;
mod mode_view;
mod overlay;
mod picker;
mod render;
mod sidebar;
mod state;
mod terminal_event;
mod tty;
mod writer;

use std::{
    error::Error as StdError,
    fmt,
    io::{self, IsTerminal as _, Write as _},
    path::{Path, PathBuf},
};

use zz_daemon::{
    DaemonError, Endpoint, InteractiveClient, classify_local_connect_error, configured_fleet_hosts,
    default_socket_path, short_device_name, terminate_incompatible_daemon,
};
use zz_protocol::{
    AttachOperation, CommandInvocation, CommandResponse, PreparedCommand, PreparedCommandResult,
    ProtocolMessage,
};

use crate::browser::BrowserFrameProvider;

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
    local_reconnect: Option<
        &'a dyn Fn(&Path, bool, Option<AttachOperation>) -> Result<InteractiveClient, DaemonError>,
    >,
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
        reconnect: &'a dyn Fn(
            &Path,
            bool,
            Option<AttachOperation>,
        ) -> Result<InteractiveClient, DaemonError>,
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
        Some(attach_operation(options)),
    )?;
    run_connected(request, initial).map_err(|error| {
        if options.session.is_none() && error.0 == "can't find session: current session" {
            Error::message("no sessions")
        } else {
            error
        }
    })
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
    let commands = commands
        .into_iter()
        .map(|command| match command {
            NewSessionCommand::Raw(invocation) => PreparedCommand {
                invocation,
                canonical_name: None,
                alias_matched: false,
                result: PreparedCommandResult::Ready,
            },
            NewSessionCommand::Prepared(command) => command,
        })
        .collect();
    let initial = initial_connection(
        &resolved.endpoint,
        &options.socket_path,
        interactive,
        options.restart_daemon,
        request.local_reconnect,
        Some(AttachOperation::Commands(commands)),
    )?;
    run_connected(request, initial)
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

enum NewSessionCommand {
    Raw(CommandInvocation),
    Prepared(PreparedCommand),
}

fn print_command_output(output: &[u8]) -> Result<(), Error> {
    if output.is_empty() {
        return Ok(());
    }
    let mut stdout = io::stdout().lock();
    write_command_output(&mut stdout, output)?;
    stdout.flush()?;
    Ok(())
}

fn write_command_output(sink: &mut impl io::Write, output: &[u8]) -> io::Result<()> {
    sink.write_all(output)?;
    if output.last() != Some(&b'\n') {
        sink.write_all(b"\n")?;
    }
    Ok(())
}

fn initial_connection(
    endpoint: &Endpoint,
    local_socket: &Path,
    interactive: bool,
    restart_daemon: bool,
    reconnect: Option<
        &dyn Fn(&Path, bool, Option<AttachOperation>) -> Result<InteractiveClient, DaemonError>,
    >,
    attach: Option<AttachOperation>,
) -> Result<InteractiveClient, Error> {
    let connection = if let Some(attach) = attach.as_ref() {
        InteractiveClient::connect_endpoint_with_attach(endpoint, attach.clone())
    } else {
        InteractiveClient::connect_endpoint_without_theme(endpoint, interactive)
    };
    match connection {
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
                    attach,
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
                attach,
            )
            .map_err(|restart| Error::message(format!("daemon restart failed: {restart}")))
        }
        Err(error) => Err(Error::message(error.to_string())),
    }
}

fn attach_operation(options: &RunOptions) -> AttachOperation {
    let mut args = Vec::new();
    if options.detach_others {
        args.push("-d".to_owned());
    }
    if options.read_only {
        args.push("-r".to_owned());
    }
    if let Some(flags) = &options.client_flags {
        args.extend(["-f".to_owned(), flags.clone()]);
    }
    if let Some(session) = &options.session {
        args.extend(["-t".to_owned(), session.clone()]);
    }
    AttachOperation::Commands(vec![PreparedCommand {
        invocation: CommandInvocation::new("attach-session", args),
        canonical_name: Some("attach-session".to_owned()),
        alias_matched: false,
        result: PreparedCommandResult::Ready,
    }])
}

pub fn run_connected<'a>(
    request: impl Into<RunRequest<'a>>,
    initial: InteractiveClient,
) -> Result<(), Error> {
    let request = request.into();
    let resolved = resolve_run(request.options)?;
    let attached = initial.is_initially_attached();
    let mut messages = Vec::new();
    for response in initial.take_initial_responses() {
        match response {
            CommandResponse::Success {
                output, exit_code, ..
            } => {
                print_command_output(output.as_bytes())?;
                if exit_code != 0 {
                    return Err(Error::message(format!(
                        "command exited with status {exit_code}"
                    )));
                }
            }
            response @ CommandResponse::Error { .. } if attached => {
                messages.push(ProtocolMessage::CommandResponse(response));
            }
            CommandResponse::Error { output, error, .. } => {
                print_command_output(output.as_bytes())?;
                return Err(Error::message(error.tmux_message()));
            }
        }
    }
    if !attached {
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::message("open terminal failed: not a terminal"));
    }
    let terminal_options = tty::TerminalOptions::from_hello(initial.server_hello());
    let browser_provider = request.browser_provider.and_then(|provider| provider());
    app::run(
        initial,
        resolved.endpoint,
        resolved.local_endpoint,
        app::InitialAttach::Connected { messages },
        terminal_options,
        resolved.host_label,
        resolved.local_host_label,
        resolved.fleet_hosts,
        browser_provider,
    )
    .map_err(Error::message)
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

    /// The raw-terminal client writes a `CommandResponse` output as the bytes
    /// the daemon sent, the way the plain CLI does. Measured against the pin
    /// on 2026-09-04: `show-environment -g ZZBYTES` after
    /// `set-environment -g ZZBYTES a<0xff>b` answers
    /// `5a5a42595445533d61ff620a`, and a lossy `&str` sink answered
    /// `5a5a42595445533d61efbfbd620a` instead.
    #[test]
    fn command_output_reaches_the_terminal_as_bytes() {
        let mut sink = Vec::new();
        write_command_output(&mut sink, b"ZZBYTES=a\xffb\n").unwrap();
        assert_eq!(sink, b"ZZBYTES=a\xffb\n");

        let mut unterminated = Vec::new();
        write_command_output(&mut unterminated, b"a\xffb").unwrap();
        assert_eq!(unterminated, b"a\xffb\n");
    }

    #[test]
    fn hello_attach_keeps_target_flags_and_read_only() {
        let operation = attach_operation(&RunOptions {
            session: Some("work".to_owned()),
            detach_others: true,
            read_only: true,
            client_flags: Some("ignore-size".to_owned()),
            ..RunOptions::default()
        });
        let AttachOperation::Commands(commands) = operation else {
            panic!("expected prepared attach");
        };
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].invocation.name, "attach-session");
        assert_eq!(
            commands[0].invocation.args,
            ["-d", "-r", "-f", "ignore-size", "-t", "work"]
        );
        assert_eq!(commands[0].result, PreparedCommandResult::Ready);
    }
}
