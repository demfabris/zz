use std::{
    fmt,
    io::{self, Write as _},
    path::{Path, PathBuf},
    str::FromStr,
};

#[cfg(any(unix, windows, test))]
use std::process::Command;
#[cfg(unix)]
use std::{
    ffi::{OsStr, OsString},
    fs,
    hash::{Hash as _, Hasher as _},
    net::{Ipv4Addr, TcpListener},
    os::unix::fs::{FileTypeExt, PermissionsExt},
    process::{Child, Stdio},
    sync::OnceLock,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};
#[cfg(windows)]
use std::{
    fmt::Write as _,
    io::{BufRead as _, BufReader},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

#[cfg(windows)]
use parking_lot::Mutex;
use thiserror::Error;

use crate::DaemonError;
#[cfg(any(unix, windows))]
use crate::askpass::{AskpassListener, AskpassMode, AskpassPrompt, SshPrompts};
#[cfg(any(unix, windows))]
use crate::transport::LocalStream;
#[cfg(windows)]
use crate::transport::{LocalListener, TransportListener as _};
use crate::transport::{LocalTransport, Transport as _, TransportStream as _};
#[cfg(any(unix, windows, test))]
use crate::unmasked::SpawnUnmasked as _;

/// `sh -l` reads `/etc/profile` and `~/.profile`; a PATH exported from `~/.bash_profile` or
/// `~/.zshrc` never reaches it, which hides the CLI that install.sh linked into one of these.
/// Appending leaves whatever the profile did contribute ahead of the fallbacks. A Mac app dragged
/// out of the disk image has no CLI link at all, so `$zz_cli` falls back to the bundle's own `cli`.
#[cfg(any(unix, windows, test))]
pub(crate) fn remote_path_fallback() -> String {
    format!(
        "PATH=\"$PATH:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin\"; export PATH; \
         zz_managed=\"$HOME/.local/bin/{name}\"; zz_cli={name}; \
         if [ -x \"$zz_managed\" ]; then zz_cli=\"$zz_managed\"; \
         elif ! command -v {name} >/dev/null 2>&1; then \
         for zz_apps in /Applications \"$HOME/Applications\"; do \
         if [ -x \"$zz_apps/{app}.app/Contents/MacOS/cli\" ]; \
         then zz_cli=\"$zz_apps/{app}.app/Contents/MacOS/cli\"; break; fi; done; fi; ",
        name = zz_protocol::app_identity::DIRECTORY,
        app = zz_protocol::app_identity::DISPLAY_NAME,
    )
}

#[cfg(any(unix, windows, test))]
// Runs under `sh -lc` so `zz` resolves through the login shell's PATH; the sentinel prefixes
// let the parser skip whatever a login profile prints around the probe's own output.
pub(crate) fn remote_socket_probe(key_line: Option<&str>) -> String {
    shell_quote(&remote_socket_probe_script(key_line))
}

#[cfg(any(unix, windows, test))]
pub(crate) fn remote_socket_probe_script(key_line: Option<&str>) -> String {
    let desktop_socket = format!(
        "elif [ \"$(uname -s)\" = Darwin ] && [ -S \"/tmp/{name}-$USER/default.sock\" ]; \
         then zz_dir=\"/tmp/{name}-$USER\"; ",
        name = zz_protocol::app_identity::DIRECTORY,
    );
    let enrollment = key_line.map_or_else(String::new, |line| {
        format!(
            "( {} ) || printf 'zz-probe-key=failed\\n'; {}",
            authorized_key_script(crate::ssh_key::key_body(line), line),
            remote_zz_key_script(),
        )
    });
    format!(
        "{fallback}{enrollment}if [ -n \"$XDG_RUNTIME_DIR\" ]; then zz_dir=\"$XDG_RUNTIME_DIR/{name}\"; \
         {desktop_socket}else zz_tmp=\"$TMPDIR\"; \
         if [ -z \"$zz_tmp\" ]; then zz_tmp=$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null); fi; \
         case \"$zz_tmp\" in /*) ;; *) zz_tmp=/tmp ;; esac; \
         zz_dir=\"${{zz_tmp%/}}/{name}-$USER\"; fi; \
         printf \"zz-probe-socket=%s\\n\" \"$zz_dir/default.sock\"; \
         if command -v \"$zz_cli\" >/dev/null 2>&1; then printf \"zz-probe-protocol=%s\\n\" \"$(\"$zz_cli\" protocol-version 2>/dev/null || echo unknown)\"; \
         else printf \"zz-probe-protocol=missing\\n\"; fi",
        fallback = remote_path_fallback(),
        name = zz_protocol::app_identity::DIRECTORY,
    )
}

#[cfg(any(unix, windows, test))]
pub(crate) fn remote_zz_key_script() -> String {
    format!(
        "if [ \"$(uname -s)\" = Darwin ]; then zz_ssh=\"$HOME/Library/Application Support/{name}/ssh\"; \
         else case \"$XDG_DATA_HOME\" in /*) zz_ssh=\"$XDG_DATA_HOME/{name}/ssh\" ;; \
         *) zz_ssh=\"$HOME/.local/share/{name}/ssh\" ;; esac; fi; \
         if [ ! -f \"$zz_ssh/id_ed25519\" ] && command -v ssh-keygen >/dev/null 2>&1; then \
         zz_host=$(uname -n | cut -d. -f1 | tr -cd 'A-Za-z0-9_-'); \
         ( umask 077; mkdir -p \"$zz_ssh\" && chmod 700 \"$zz_ssh\" && ssh-keygen -q -t ed25519 -N '' \
         -C \"zz@${{zz_host:-host}}\" -f \"$zz_ssh/id_ed25519\" </dev/null >/dev/null 2>&1 ); fi; \
         if [ -f \"$zz_ssh/id_ed25519.pub\" ]; \
         then printf 'zz-probe-pubkey=%s\\n' \"$(cat \"$zz_ssh/id_ed25519.pub\")\"; fi; ",
        name = zz_protocol::app_identity::DIRECTORY,
    )
}
/// Exit codes the auto-start script picks for itself; ssh reports its own failures as 255.
#[cfg(any(unix, windows, test))]
pub(crate) const REMOTE_ZZ_MISSING_STATUS: i32 = 127;
#[cfg(any(unix, windows, test))]
pub(crate) const REMOTE_DAEMON_TIMEOUT_STATUS: i32 = 3;
#[cfg(any(unix, windows, test))]
const REMOTE_INSTALL_ELSEWHERE_STATUS: i32 = 4;
#[cfg(any(unix, windows, test))]
const REMOTE_INSTALL_FAILED_STATUS: i32 = 5;
#[cfg(any(unix, windows, test))]
const REMOTE_INSTALL_BROKEN_STATUS: i32 = 6;
#[cfg(any(unix, windows, test))]
const INSTALL_SCRIPT: &str = include_str!("../../../install.sh");
#[cfg(any(unix, windows, test))]
const REMOTE_DAEMON_POLL_ATTEMPTS: u32 = 50;
#[cfg(any(unix, windows, test))]
const REMOTE_DAEMON_POLL_INTERVAL: &str = "0.1";
/// POSIX only requires whole-second `sleep`, so a remote that rejects the fraction polls seconds.
#[cfg(any(unix, windows, test))]
const REMOTE_DAEMON_POLL_FALLBACK_ATTEMPTS: u32 = 5;
#[cfg(unix)]
const FORWARD_POLL_ATTEMPTS: usize = 250;
#[cfg(unix)]
const FORWARD_POLL_INTERVAL: Duration = Duration::from_millis(20);
#[cfg(unix)]
const CONTROL_PERSIST_SECONDS: u32 = 600;
#[cfg(unix)]
const LEGACY_SSH_DIRECTORY_AGE: Duration = Duration::from_mins(10);
/// Leaves room inside `sun_path` for the private directory and the socket names under it.
#[cfg(unix)]
const MAX_SSH_RUNTIME_ROOT_BYTES: usize = 60;
#[cfg(unix)]
static FORWARD_COUNTER: AtomicU64 = AtomicU64::new(1);
#[cfg(windows)]
static FORWARD_COUNTER: AtomicU64 = AtomicU64::new(1);
/// What the remote proxy prints before its first protocol byte, ahead of any login chatter.
pub(crate) const PROXY_READY_MARKER: &[u8] = b"zz-proxy-1\n";
#[cfg(windows)]
const MAX_PROXY_PREAMBLE_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const MAX_SSH_STDERR_BYTES: usize = 8 * 1024;
const PUMP_BUFFER_BYTES: usize = 16 * 1024;
#[cfg(windows)]
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Local(PathBuf),
    Ssh(SshEndpoint),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshEndpoint {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub remote_socket: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum EndpointError {
    #[error("invalid endpoint URI `{input}`: {reason}")]
    UriParse { input: String, reason: String },
    #[error("failed to spawn ssh for {operation}: {source}")]
    SshSpawn {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("ssh socket probe for {target} failed: {reason}")]
    ProbeFailure { target: String, reason: String },
    #[error("ssh to {target} failed: {reason}")]
    SshFailed { target: String, reason: String },
    #[error("the SSH host key for {target} was not trusted")]
    HostKeyRejected { target: String },
    #[error("SSH authentication to {target} failed: {reason}")]
    AuthenticationFailed { target: String, reason: String },
    #[error("{target} closed the connection before sign-in finished")]
    SignInExpired { target: String },
    #[error("zz is not installed on {target}")]
    RemoteBinaryMissing { target: String },
    #[error("the zz daemon on {target} never started listening")]
    RemoteDaemonUnavailable { target: String },
    #[error("zz on {target} speaks protocol v{daemon}; this zz speaks v{client}")]
    RemoteProtocolMismatch {
        target: String,
        daemon: u16,
        client: u16,
    },
    #[error("zz on {target} predates protocol version reporting; update it, then reconnect")]
    RemoteProtocolUnknown { target: String },
    #[error("zz on {target} is {path}, a link to {link} that zz did not install")]
    RemoteInstallElsewhere {
        target: String,
        path: String,
        link: String,
    },
    #[error("installing zz on {target} failed: {reason}")]
    RemoteInstallFailed { target: String, reason: String },
    #[error("the zz installed on {target} speaks protocol v{remote}; this zz speaks v{client}")]
    RemoteInstallMismatch {
        target: String,
        remote: u16,
        client: u16,
    },
    #[error(
        "ssh forward to {target} exited with {status} before creating {local_socket}",
        local_socket = local_socket.display()
    )]
    ForwardExited {
        target: String,
        status: String,
        local_socket: PathBuf,
    },
    #[error(
        "ssh forward to {target} did not create {local_socket} within five seconds",
        local_socket = local_socket.display()
    )]
    ForwardTimeout {
        target: String,
        local_socket: PathBuf,
    },
    #[error("ssh forward to {target} failed while {action} {path}: {source}", path = path.display())]
    ForwardIo {
        target: String,
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "{target} stopped answering for {seconds} s; the command may have run there, check before retrying"
    )]
    StoppedAnswering { target: String, seconds: u64 },
    #[error(
        "ssh remote socket path must be absolute: {path}",
        path = .0.display()
    )]
    InvalidRemoteSocket(PathBuf),
    #[error("ssh endpoints are unsupported on this platform")]
    UnsupportedPlatform,
}

impl EndpointError {
    /// The endpoint failure behind `error`, looking through the `io::Error` a connect wraps it in.
    #[must_use]
    pub fn find<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a Self> {
        let mut source = Some(error);
        while let Some(current) = source {
            if let Some(found) = current.downcast_ref::<Self>() {
                return Some(found);
            }
            source = match current
                .downcast_ref::<io::Error>()
                .and_then(io::Error::get_ref)
            {
                Some(inner) => Some(inner),
                None => current.source(),
            };
        }
        None
    }

    /// The same failure `Display` reports, said in terms of what the user can do about it.
    #[must_use]
    pub fn ssh_reason(&self) -> Option<String> {
        Some(match self {
            Self::SshSpawn { operation, source } => {
                format!("Could not run ssh for the {operation}: {source}")
            }
            Self::ProbeFailure { target, reason } | Self::SshFailed { target, reason } => {
                format!("Could not reach {target} over ssh: {reason}")
            }
            Self::HostKeyRejected { target } => {
                format!("The SSH host key for {target} was not trusted.")
            }
            Self::AuthenticationFailed { target, reason } => {
                format!("Could not sign in to {target}: {reason}")
            }
            Self::SignInExpired { target } => format!(
                "{target} stopped waiting for the password.\nssh allows about two minutes to sign \
                 in; connect again."
            ),
            Self::RemoteBinaryMissing { target } => format!(
                "{} is not installed on {target}.\nInstall it there with `curl -fsSL \
                 https://zzmux.sh/install.sh | sh`, then connect again.",
                zz_protocol::app_identity::DIRECTORY
            ),
            Self::RemoteDaemonUnavailable { target } => format!(
                "Started {name} on {target}, but its daemon socket never appeared.\nRun `{name} daemon` \
                 there to see why.",
                name = zz_protocol::app_identity::DIRECTORY,
            ),
            Self::RemoteProtocolMismatch {
                target,
                daemon,
                client,
            } => format!(
                "{name} on {target} speaks protocol v{daemon}; this {name} speaks v{client}.\nUpdate {name} on \
                 that host, then reconnect.",
                name = zz_protocol::app_identity::DIRECTORY,
            ),
            Self::RemoteProtocolUnknown { target } => {
                format!(
                    "{} on {target} predates protocol version reporting; update it, then reconnect",
                    zz_protocol::app_identity::DIRECTORY,
                )
            }
            Self::RemoteInstallElsewhere { target, path, link } => format!(
                "{path} on {target} links to {link}, which {name} did not install.\nRemove the \
                 link or update that install, then reconnect.",
                name = zz_protocol::app_identity::DIRECTORY,
            ),
            Self::RemoteInstallFailed { target, reason } => format!(
                "Could not install {} on {target}: {reason}",
                zz_protocol::app_identity::DIRECTORY,
            ),
            Self::RemoteInstallMismatch {
                target,
                remote,
                client,
            } => format!(
                "Installed {name} {version} on {target}, but it speaks protocol v{remote} and this \
                 {name} speaks v{client}.\nThis build has no matching release.",
                name = zz_protocol::app_identity::DIRECTORY,
                version = env!("CARGO_PKG_VERSION"),
            ),
            Self::ForwardExited { target, status, .. } => format!(
                "The ssh forward to {target} exited ({status}).\nDoes its sshd allow local \
                 forwarding?"
            ),
            Self::ForwardTimeout { target, .. } => {
                format!("ssh connected to {target}, but never created the forwarded socket.")
            }
            Self::ForwardIo {
                target,
                action,
                source,
                ..
            } => format!("The ssh forward to {target} failed while {action} its socket: {source}"),
            Self::StoppedAnswering { .. } => self.to_string(),
            Self::InvalidRemoteSocket(path) => format!(
                "The remote socket path must be absolute: {path}",
                path = path.display()
            ),
            Self::UnsupportedPlatform => "ssh hosts are not supported on this platform.".to_owned(),
            Self::UriParse { .. } => return None,
        })
    }

    #[must_use]
    pub fn needs_sign_in(&self) -> bool {
        match self {
            Self::AuthenticationFailed { .. }
            | Self::HostKeyRejected { .. }
            | Self::SignInExpired { .. } => true,
            #[cfg(any(unix, windows, test))]
            Self::ProbeFailure { reason, .. } | Self::SshFailed { reason, .. } => [
                SshFailure::SecurityKey,
                SshFailure::Login,
                SshFailure::HostKey,
            ]
            .into_iter()
            .any(|failure| reason == failure.hint()),
            _ => false,
        }
    }
}

impl Endpoint {
    pub fn parse(input: &str) -> Result<Self, EndpointError> {
        if input.starts_with("quic://") {
            return Err(parse_error(
                input,
                "quic endpoints were removed; use ssh://",
            ));
        }
        if let Some(value) = input.strip_prefix("ssh://") {
            return parse_ssh_endpoint(input, value).map(Self::Ssh);
        }
        if let Some(path) = input.strip_prefix("unix://") {
            if !path.starts_with('/') {
                return Err(parse_error(input, "unix endpoint path must be absolute"));
            }
            return Ok(Self::Local(PathBuf::from(path)));
        }
        if input.is_empty() {
            return Err(parse_error(input, "endpoint is empty"));
        }
        if let Some((scheme, _)) = input.split_once("://") {
            return Err(parse_error(
                input,
                format!("unsupported endpoint scheme `{scheme}`"),
            ));
        }
        Ok(Self::Local(PathBuf::from(input)))
    }
}

impl FromStr for Endpoint {
    type Err = EndpointError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(path) => path.display().fmt(formatter),
            Self::Ssh(endpoint) => endpoint.fmt(formatter),
        }
    }
}

impl fmt::Display for SshEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ssh://")?;
        if let Some(user) = &self.user {
            write!(formatter, "{user}@")?;
        }
        if self.host.contains(':') {
            write!(formatter, "[{}]", self.host)?;
        } else {
            formatter.write_str(&self.host)?;
        }
        if let Some(port) = self.port {
            write!(formatter, ":{port}")?;
        }
        if let Some(remote_socket) = &self.remote_socket {
            remote_socket.display().fmt(formatter)?;
        }
        Ok(())
    }
}

impl From<EndpointError> for DaemonError {
    fn from(error: EndpointError) -> Self {
        Self::Io(io::Error::other(error))
    }
}

fn parse_ssh_endpoint(input: &str, value: &str) -> Result<SshEndpoint, EndpointError> {
    let (authority, remote_socket) = match value.split_once('/') {
        Some((authority, path)) => (authority, Some(PathBuf::from(format!("/{path}")))),
        None => (value, None),
    };
    if authority.is_empty() {
        return Err(parse_error(input, "ssh host is empty"));
    }

    let (user, host_and_port) = match authority.split_once('@') {
        Some((user, host)) => {
            if user.is_empty() {
                return Err(parse_error(input, "ssh user is empty"));
            }
            if host.contains('@') {
                return Err(parse_error(
                    input,
                    "ssh authority contains multiple `@` signs",
                ));
            }
            if user.chars().any(char::is_whitespace) {
                return Err(parse_error(input, "ssh user contains whitespace"));
            }
            (Some(user.to_owned()), host)
        }
        None => (None, authority),
    };

    let (host, port) = parse_host_and_port(input, host_and_port)?;
    if host.is_empty() {
        return Err(parse_error(input, "ssh host is empty"));
    }
    if host.chars().any(char::is_whitespace) {
        return Err(parse_error(input, "ssh host contains whitespace"));
    }
    // A leading `-` reaches ssh's option parser instead of its destination argument.
    if host.starts_with('-') {
        return Err(parse_error(input, "ssh host must not start with `-`"));
    }

    Ok(SshEndpoint {
        user,
        host,
        port,
        remote_socket,
    })
}

fn parse_host_and_port(
    input: &str,
    host_and_port: &str,
) -> Result<(String, Option<u16>), EndpointError> {
    if let Some(bracketed) = host_and_port.strip_prefix('[') {
        let Some(closing) = bracketed.find(']') else {
            return Err(parse_error(input, "unterminated bracketed ssh host"));
        };
        let host = &bracketed[..closing];
        let suffix = &bracketed[closing + 1..];
        let port = if suffix.is_empty() {
            None
        } else {
            let Some(port) = suffix.strip_prefix(':') else {
                return Err(parse_error(input, "invalid bracketed ssh authority"));
            };
            Some(parse_port(input, port)?)
        };
        return Ok((host.to_owned(), port));
    }
    if host_and_port.contains(['[', ']']) {
        return Err(parse_error(input, "invalid bracketed ssh authority"));
    }

    match host_and_port.rsplit_once(':') {
        Some((host, _port)) if host.contains(':') => Err(parse_error(
            input,
            "IPv6 ssh hosts must be enclosed in brackets",
        )),
        Some((host, port)) => Ok((host.to_owned(), Some(parse_port(input, port)?))),
        None => Ok((host_and_port.to_owned(), None)),
    }
}

fn parse_port(input: &str, port: &str) -> Result<u16, EndpointError> {
    match port.parse::<u16>() {
        Ok(port) if port != 0 => Ok(port),
        _ => Err(parse_error(input, "ssh port must be between 1 and 65535")),
    }
}

fn parse_error(input: &str, reason: impl Into<String>) -> EndpointError {
    EndpointError::UriParse {
        input: input.to_owned(),
        reason: reason.into(),
    }
}

#[cfg(any(unix, windows, test))]
#[derive(Clone, Copy, Debug, Default)]
struct SshSession<'a> {
    #[cfg(not(windows))]
    control: Option<&'a SshControl>,
    askpass: Option<SshAskpass<'a>>,
    identity: Option<&'a Path>,
    batch: bool,
}

#[cfg(not(windows))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct SshControl {
    path: PathBuf,
    user_persist: bool,
}

#[cfg(any(unix, windows, test))]
#[derive(Clone, Copy, Debug)]
struct SshAskpass<'a> {
    helper: &'a Path,
    socket: &'a Path,
}

#[cfg(any(unix, test))]
fn forwarding_spec(local_socket: &Path, remote_socket: &Path) -> std::ffi::OsString {
    let mut forwarding = local_socket.as_os_str().to_os_string();
    forwarding.push(":");
    forwarding.push(remote_socket.as_os_str());
    forwarding
}

/// Binds SOCKS to loopback explicitly: ssh's default bind address follows `GatewayPorts`.
#[cfg(any(unix, test))]
fn socks_spec(port: u16) -> String {
    format!("127.0.0.1:{port}")
}

#[cfg(any(unix, test))]
fn ssh_forward_command(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    local_socket: &Path,
    remote_socket: &Path,
    socks_port: Option<u16>,
) -> Command {
    let mut command = Command::new("ssh");
    command
        .arg("-N")
        .arg("-n")
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-o")
        .arg("StreamLocalBindMask=0177")
        .arg("-L")
        .arg(forwarding_spec(local_socket, remote_socket));
    if let Some(port) = socks_port {
        command.arg("-D").arg(socks_spec(port));
    }
    append_ssh_options(&mut command, endpoint, session);
    command.arg("--").arg(&endpoint.host);
    command
}

/// `ssh -D 0` is invalid, so ask the kernel for a candidate and retry startup if the handoff races.
#[cfg(unix)]
fn available_socks_port() -> Option<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(port)
}

#[cfg(any(unix, windows, test))]
fn ssh_probe_command(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    key_line: Option<&str>,
) -> Command {
    ssh_script_command(endpoint, session, &[], &remote_socket_probe(key_line))
}

#[cfg(any(unix, windows, test))]
fn ssh_script_command(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    options: &[&str],
    script: &str,
) -> Command {
    let mut command = Command::new("ssh");
    for option in options {
        command.arg("-o").arg(option);
    }
    append_ssh_options(&mut command, endpoint, session);
    command
        .arg("--")
        .arg(&endpoint.host)
        .arg("sh")
        .arg("-lc")
        .arg(script);
    command
}

#[cfg(any(unix, windows, test))]
fn login_options(command: &mut Command, session: SshSession<'_>) {
    if let Some(identity) = session.identity {
        command
            .arg("-o")
            .arg("IdentitiesOnly=yes")
            .arg("-o")
            .arg(format!(
                "IdentityFile=\"{}\"",
                identity.to_string_lossy().replace('%', "%%")
            ))
            .arg("-o")
            .arg("PreferredAuthentications=publickey");
    }
    let batch = session.batch || batch_logins();
    if batch || session.identity.is_some() {
        command.arg("-o").arg("BatchMode=yes");
    }
    if batch {
        command.arg("-o").arg(NO_SECURITY_KEYS);
    }
}

#[cfg(any(unix, windows, test))]
fn batch_logins() -> bool {
    std::env::var_os(crate::SSH_BATCH_ENVIRONMENT_VARIABLE).is_some_and(|value| !value.is_empty())
}

#[cfg(any(unix, windows))]
fn zz_key_first(
    identity: Option<&Path>,
    command: impl Fn(Option<&Path>) -> Command,
) -> io::Result<(std::process::Output, bool)> {
    if let Some(identity) = identity {
        let output = command(Some(identity)).output_unmasked()?;
        if !refused_login(output.status.code(), &output.stderr) {
            return Ok((output, true));
        }
        log::debug!(
            target: "zz_daemon::diagnostics",
            "the zz key did not sign in: {}",
            String::from_utf8_lossy(&output.stderr).trim(),
        );
    }
    Ok((command(None).output_unmasked()?, false))
}

#[cfg(any(unix, windows, test))]
fn refused_login(code: Option<i32>, stderr: &[u8]) -> bool {
    code == Some(SSH_FAILURE_STATUS)
        && SshFailure::classify(&String::from_utf8_lossy(stderr)).is_some_and(SshFailure::is_login)
}

#[cfg(any(unix, windows, test))]
const NO_SECURITY_KEYS: &str = "PubkeyAcceptedKeyTypes=-sk-*,webauthn-sk-*";

#[cfg(any(unix, windows))]
pub(crate) fn run_remote_script(
    endpoint: &SshEndpoint,
    connect_timeout_seconds: u32,
    script: &str,
) -> io::Result<std::process::Output> {
    let connect_timeout = format!("ConnectTimeout={connect_timeout_seconds}");
    let script = shell_quote(script);
    let identity = zz_identity().map(|(path, _)| path);
    let run = move |endpoint: &SshEndpoint, session: SshSession<'_>| {
        zz_key_first(identity.as_deref(), |phase| {
            ssh_script_command(
                endpoint,
                SshSession {
                    identity: phase,
                    batch: true,
                    ..session
                },
                &[&connect_timeout],
                &script,
            )
        })
        .map(|(output, _)| output)
    };
    #[cfg(unix)]
    if let Some((control, alive)) = live_control(endpoint) {
        let step = SshStep {
            endpoint: endpoint.clone(),
            control: Some(control),
            askpass: None,
            identity: None,
        };
        return match step.run(alive, "remote script", move |step| {
            Ok(run(&step.endpoint, step.session()))
        }) {
            Ok(output) => output,
            Err(EndpointError::SshFailed { reason, .. }) => {
                Err(io::Error::new(io::ErrorKind::TimedOut, reason))
            }
            Err(error) => Err(io::Error::other(error)),
        };
    }
    run(endpoint, SshSession::default())
}

#[cfg(any(unix, windows))]
fn zz_identity() -> Option<(PathBuf, String)> {
    let identity = crate::ssh_key::ssh_key_path()
        .and_then(|path| crate::ssh_key::ssh_public_key().map(|line| (path, line)));
    match identity {
        Ok(identity) => Some(identity),
        Err(error) => {
            log::debug!(target: "zz_daemon::diagnostics", "no zz ssh key: {error}");
            None
        }
    }
}

#[cfg(unix)]
fn ssh_daemon_start_command(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    remote_socket: &Path,
) -> Command {
    let mut command = Command::new("ssh");
    append_ssh_options(&mut command, endpoint, session);
    command
        .arg("--")
        .arg(&endpoint.host)
        .arg("sh")
        .arg("-lc")
        .arg(shell_quote(&remote_daemon_start_script(remote_socket)));
    command
}

#[cfg(any(unix, test))]
pub(crate) fn remote_daemon_start_script(remote_socket: &Path) -> String {
    format!(
        "{}{}",
        remote_path_fallback(),
        remote_daemon_start_body(&shell_quote(&remote_socket.to_string_lossy())),
    )
}

#[cfg(any(unix, windows, test))]
fn remote_daemon_start_body(socket: &str) -> String {
    format!(
        "command -v \"$zz_cli\" >/dev/null 2>&1 || exit {REMOTE_ZZ_MISSING_STATUS}; \
         if setsid true >/dev/null 2>&1; \
         then setsid \"$zz_cli\" daemon --socket {socket} >/dev/null 2>&1 </dev/null & \
         else nohup \"$zz_cli\" daemon --socket {socket} >/dev/null 2>&1 </dev/null & fi; \
         if sleep {REMOTE_DAEMON_POLL_INTERVAL} 2>/dev/null; \
         then delay={REMOTE_DAEMON_POLL_INTERVAL}; attempts={REMOTE_DAEMON_POLL_ATTEMPTS}; \
         else delay=1; attempts={REMOTE_DAEMON_POLL_FALLBACK_ATTEMPTS}; fi; \
         attempt=0; \
         while [ \"$attempt\" -lt \"$attempts\" ]; do \
         if [ -S {socket} ]; then exit 0; fi; \
         sleep \"$delay\"; attempt=$((attempt + 1)); done; \
         exit {REMOTE_DAEMON_TIMEOUT_STATUS}",
    )
}

#[cfg(any(unix, windows, test))]
pub(crate) fn remote_install_script(remote_socket: Option<&Path>, restart_daemon: bool) -> String {
    let restart = if restart_daemon {
        let socket = remote_socket.map_or_else(String::new, |socket| {
            format!("--socket {} ", shell_quote(&socket.to_string_lossy()))
        });
        format!(
            "if [ \"$zz_protocol\" = {protocol} ]; then \
             zz_restart=$(\"$zz_managed\" {socket}kill-server 2>&1 >/dev/null); \
             printf 'zz-install-restart=%s\\n' \"$?\"; \
             printf '%s\\n' \"$zz_restart\" | sed 's/^/zz-install-restart-error=/'; fi; ",
            protocol = zz_protocol::PROTOCOL_VERSION,
        )
    } else {
        String::new()
    };
    format!(
        "{fallback}if [ \"$(\"$zz_managed\" --version 2>/dev/null </dev/null)\" = 'zz {version}' ] \
         && [ \"$(\"$zz_managed\" protocol-version 2>/dev/null </dev/null)\" = {protocol} ]; \
         then printf 'zz-install-current=1\\n'; \
         else zz_link=$(readlink \"$zz_managed\" 2>/dev/null); case \"$zz_link\" in \
         \"\") printf 'zz-install-mode=headless\\n'; sh -s -- --headless --version {version} ;; \
         ../lib/zz/cli|\"$HOME/.local/lib/zz/cli\") printf 'zz-install-mode=desktop\\n'; \
         sh -s -- --prefix \"$HOME/.local\" --version {version} ;; \
         *) printf 'zz-install-elsewhere=%s\\nzz-install-link=%s\\n' \"$zz_managed\" \"$zz_link\"; \
         exit {REMOTE_INSTALL_ELSEWHERE_STATUS} ;; esac || exit {REMOTE_INSTALL_FAILED_STATUS}; fi; \
         zz_protocol=$(\"$zz_managed\" protocol-version) || exit {REMOTE_INSTALL_BROKEN_STATUS}; \
         {restart}printf 'zz-install-protocol=%s\\n' \"$zz_protocol\"",
        fallback = remote_path_fallback(),
        version = env!("CARGO_PKG_VERSION"),
        protocol = zz_protocol::PROTOCOL_VERSION,
    )
}

#[cfg(any(unix, windows, test))]
fn ssh_install_command(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    restart_daemon: bool,
) -> Command {
    let mut command = Command::new("ssh");
    append_ssh_options(&mut command, endpoint, session);
    command
        .arg("--")
        .arg(&endpoint.host)
        .arg("sh")
        .arg("-lc")
        .arg(shell_quote(&remote_install_script(
            endpoint.remote_socket.as_deref(),
            restart_daemon,
        )));
    command
}

#[cfg(any(unix, windows, test))]
pub(crate) fn parse_remote_install_output(
    target: &str,
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
    status: &str,
) -> Result<RemoteInstall, EndpointError> {
    let stdout = String::from_utf8_lossy(stdout);
    let value = |prefix: &str| {
        stdout
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .map(|value| value.trim().to_owned())
    };
    let stderr = String::from_utf8_lossy(stderr);
    let target = target.to_owned();
    match code {
        Some(0) => match value("zz-install-protocol=").map(|value| value.parse::<u16>()) {
            Some(Ok(zz_protocol::PROTOCOL_VERSION)) => Ok(RemoteInstall {
                installed: if value("zz-install-current=").is_some() {
                    RemoteInstalled::Current
                } else if value("zz-install-mode=").as_deref() == Some("desktop") {
                    RemoteInstalled::Desktop
                } else {
                    RemoteInstalled::Headless
                },
                restart: match value("zz-install-restart=").as_deref() {
                    None => RemoteRestart::NotAsked,
                    Some("0") => RemoteRestart::Restarted,
                    Some(code) => match stdout
                        .lines()
                        .filter_map(|line| line.strip_prefix("zz-install-restart-error="))
                        .map(str::trim)
                        .rfind(|line| !line.is_empty())
                    {
                        Some(reason) if daemon_was_not_running(reason) => RemoteRestart::NotRunning,
                        Some(reason) => RemoteRestart::Failed(
                            reason.strip_prefix("zz: ").unwrap_or(reason).to_owned(),
                        ),
                        None => RemoteRestart::Failed(format!("kill-server exited with {code}")),
                    },
                },
                warnings: stderr
                    .lines()
                    .filter_map(|line| line.trim().strip_prefix("warning: "))
                    .map(ToOwned::to_owned)
                    .collect(),
            }),
            Some(Ok(remote)) => Err(EndpointError::RemoteInstallMismatch {
                target,
                remote,
                client: zz_protocol::PROTOCOL_VERSION,
            }),
            _ => Err(EndpointError::RemoteInstallFailed {
                target,
                reason: "the installed binary did not report its protocol version".to_owned(),
            }),
        },
        Some(REMOTE_INSTALL_ELSEWHERE_STATUS)
            if let Some(path) = value("zz-install-elsewhere=")
                && let Some(link) = value("zz-install-link=") =>
        {
            Err(EndpointError::RemoteInstallElsewhere { target, path, link })
        }
        Some(REMOTE_INSTALL_FAILED_STATUS) => {
            let reason = ssh_failure_reason(&stderr, status);
            Err(EndpointError::RemoteInstallFailed {
                target,
                reason: reason.strip_prefix("error: ").unwrap_or(&reason).to_owned(),
            })
        }
        Some(REMOTE_INSTALL_BROKEN_STATUS) => Err(EndpointError::RemoteInstallFailed {
            target,
            reason: format!(
                "the installed binary does not run there: {}",
                ssh_failure_reason(&stderr, status)
            ),
        }),
        _ => Err(EndpointError::SshFailed {
            target,
            reason: ssh_failure_reason(&stderr, status),
        }),
    }
}

#[cfg(any(unix, windows, test))]
fn daemon_was_not_running(message: &str) -> bool {
    message.starts_with("no server running on ")
        || (message.starts_with("error connecting to ")
            && message.ends_with("(No such file or directory)"))
}

/// The remote half of an attach that cannot forward a Unix socket: `zz proxy` on its stdio.
#[cfg(any(target_os = "ios", target_os = "android", test))]
pub(crate) fn remote_proxy_script(remote_socket: &Path) -> String {
    format!(
        "{fallback}exec \"$zz_cli\" proxy --socket {socket}",
        fallback = remote_path_fallback(),
        socket = shell_quote(&remote_socket.to_string_lossy()),
    )
}

/// One login for a whole Windows attach: the probe, a daemon start when its protocol matches, and
/// `zz proxy`, whose ready marker follows the probe lines.
#[cfg(any(windows, test))]
pub(crate) fn remote_connect_script(
    remote_socket: Option<&Path>,
    key_line: Option<&str>,
) -> String {
    let socket = remote_socket.map_or_else(
        || "\"$zz_dir/default.sock\"".to_owned(),
        |socket| shell_quote(&socket.to_string_lossy()),
    );
    format!(
        "{probe}; command -v \"$zz_cli\" >/dev/null 2>&1 || exit {REMOTE_ZZ_MISSING_STATUS}; \
         [ \"$(\"$zz_cli\" protocol-version 2>/dev/null)\" = {protocol} ] || exit 0; \
         zz_socket={socket}; ( {start} ) || exit $?; \
         exec \"$zz_cli\" proxy --socket \"$zz_socket\"",
        probe = remote_socket_probe_script(key_line),
        protocol = zz_protocol::PROTOCOL_VERSION,
        start = remote_daemon_start_body("\"$zz_socket\""),
    )
}

/// Appends `line` to the remote `~/.ssh/authorized_keys` unless `key` is already there, creating
/// the directory and file private as sshd's `StrictModes` wants, and never gluing onto a last line
/// that lacks its newline.
#[cfg(any(unix, windows, test))]
pub(crate) fn authorized_key_script(key: &str, line: &str) -> String {
    let file = "\"$HOME/.ssh/authorized_keys\"";
    format!(
        "umask 077; mkdir -p \"$HOME/.ssh\" && touch {file} || exit 1; \
         if grep -qF {key} {file}; then printf 'zz-probe-key=present\\n'; exit 0; fi; \
         if [ -s {file} ] && [ -n \"$(tail -c 1 {file})\" ]; then echo >> {file}; fi; \
         printf '%s\\n' {line} >> {file} || exit 1; printf 'zz-probe-key=added\\n'",
        key = shell_quote(key),
        line = shell_quote(line),
    )
}

/// Quote a value so the remote shell sees it verbatim; an ssh command line is parsed twice.
#[cfg(any(unix, windows, test))]
pub fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(any(unix, windows, test))]
pub(crate) const SSH_FAILURE_STATUS: i32 = 255;
#[cfg(any(unix, windows, test))]
const SSH_CONNECT_TIMEOUT_SECONDS: u32 = 10;
#[cfg(any(unix, windows, test))]
pub(crate) const UNATTENDED_CONNECT_TIMEOUT_SECONDS: u32 = 3;

#[cfg(any(unix, windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SshFailure {
    SecurityKey,
    Login,
    HostKey,
    Resolve,
    Refused,
    Timeout,
    Unreachable,
    WindowsShell,
}

#[cfg(any(unix, windows, test))]
impl SshFailure {
    pub(crate) fn classify(stderr: &str) -> Option<Self> {
        let stderr = stderr.to_ascii_lowercase();
        let contains = |needle: &str| stderr.contains(needle);
        if (contains("signing failed for") && contains("-sk"))
            || contains("device not found")
            || contains("user presence")
        {
            Some(Self::SecurityKey)
        } else if contains("permission denied")
            || contains("too many authentication failures")
            || contains("no supported authentication methods")
        {
            Some(Self::Login)
        } else if contains("host key verification failed")
            || contains("remote host identification has changed")
        {
            Some(Self::HostKey)
        } else if contains("could not resolve hostname")
            || contains("name or service not known")
            || contains("nodename nor servname")
        {
            Some(Self::Resolve)
        } else if contains("connection refused") {
            Some(Self::Refused)
        } else if contains("timed out") {
            Some(Self::Timeout)
        } else if contains("no route to host") || contains("network is unreachable") {
            Some(Self::Unreachable)
        } else if contains("fullyqualifiederrorid")
            || contains("is not recognized as an internal or external command")
        {
            Some(Self::WindowsShell)
        } else {
            None
        }
    }

    pub(crate) const fn is_login(self) -> bool {
        matches!(self, Self::SecurityKey | Self::Login | Self::HostKey)
    }

    const fn hint(self) -> &'static str {
        match self {
            Self::SecurityKey => "the security key was not touched",
            Self::Login => "ssh rejected the login: add your key to the host or start an ssh agent",
            Self::HostKey => {
                "known_hosts has no matching key for the host: verify the host before connecting"
            }
            Self::Resolve => "the host name did not resolve",
            Self::Refused => {
                "nothing accepted the ssh connection: start sshd there, or on a Mac turn on Remote \
                 Login in System Settings › General › Sharing"
            }
            Self::Timeout => "the ssh connection timed out",
            Self::Unreachable => "the host is not reachable from this network",
            Self::WindowsShell => {
                "the host's ssh shell is PowerShell or cmd, and zz needs a Linux or macOS shell there"
            }
        }
    }
}

/// ssh collapses every failure of its own into exit 255, so stderr is all there is to go on.
#[cfg(any(unix, windows, test))]
pub(crate) fn ssh_failure_hint(stderr: &str) -> Option<&'static str> {
    SshFailure::classify(stderr).map(SshFailure::hint)
}

#[cfg(any(unix, windows, test))]
pub(crate) fn ssh_failure_reason(stderr: &str, status: &str) -> String {
    if let Some(hint) = ssh_failure_hint(stderr) {
        return hint.to_owned();
    }
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map_or_else(|| format!("ssh exited with {status}"), ToOwned::to_owned)
}

#[cfg(any(unix, windows, test))]
fn append_ssh_options(command: &mut Command, endpoint: &SshEndpoint, session: SshSession<'_>) {
    login_options(command, session);
    let connect_timeout = if batch_logins() {
        UNATTENDED_CONNECT_TIMEOUT_SECONDS
    } else {
        SSH_CONNECT_TIMEOUT_SECONDS
    };
    command
        .arg("-o")
        .arg(format!("ConnectTimeout={connect_timeout}"))
        .arg("-o")
        .arg("ServerAliveInterval=5")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-o")
        .arg("Compression=yes");
    // Windows OpenSSH has no connection sharing, and any set `ControlPath` still routes children
    // into its broken mux stub, so both options are overridden rather than omitted.
    #[cfg(windows)]
    command
        .arg("-o")
        .arg("ControlMaster=no")
        .arg("-o")
        .arg("ControlPath=none");
    #[cfg(not(windows))]
    if let Some(control) = session.control {
        command
            .arg("-o")
            .arg("ControlMaster=auto")
            .arg("-o")
            .arg(control_path_option(&control.path));
        if !control.user_persist {
            command
                .arg("-o")
                .arg(format!("ControlPersist={CONTROL_PERSIST_SECONDS}"));
        }
    }
    if let Some(port) = endpoint.port {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(user) = &endpoint.user {
        command.arg("-l").arg(user);
    }
    if let Some(askpass) = session.askpass {
        // `force`, not `prefer`: `prefer` skips the helper whenever ssh has a controlling TTY, and
        // on Windows it resolves to off outright. `BatchMode` stays unset; it kills prompts.
        command
            .env(crate::askpass::SSH_ASKPASS_ENV, askpass.helper)
            .env(crate::askpass::SSH_ASKPASS_REQUIRE_ENV, "force")
            .env(crate::askpass::ASKPASS_SOCKET_ENV, askpass.socket);
    }
}

/// ssh expands `%` tokens inside `ControlPath`, so a literal one has to be escaped.
#[cfg(any(unix, test))]
fn control_path_option(control_path: &Path) -> String {
    format!(
        "ControlPath={}",
        control_path.to_string_lossy().replace('%', "%%")
    )
}

#[cfg(unix)]
pub(crate) fn ssh_runtime_dir() -> io::Result<&'static Path> {
    static DIRECTORY: OnceLock<Result<SshRuntimeDir, String>> = OnceLock::new();
    match DIRECTORY.get_or_init(|| {
        let stable = stable_ssh_runtime_dir();
        remove_legacy_ssh_dirs(&ssh_runtime_root());
        match private_directory(&stable) {
            Ok(()) => return Ok(SshRuntimeDir::Stable(stable)),
            Err(error) => log::warn!(
                target: "zz_daemon::diagnostics",
                "ssh sockets fall back to a per-process directory: {} is unusable: {error}",
                stable.display(),
            ),
        }
        let directory = tempfile::Builder::new()
            .prefix(&format!("{}-", user_ssh_dir_name()))
            .tempdir_in(ssh_runtime_root())
            .map_err(|error| error.to_string())?;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        if directory.path().to_str().is_none() {
            return Err(format!(
                "temporary directory {} is not valid UTF-8",
                directory.path().display()
            ));
        }
        Ok(SshRuntimeDir::Process(directory))
    }) {
        Ok(directory) => Ok(directory.path()),
        Err(error) => Err(io::Error::other(error.clone())),
    }
}

#[cfg(unix)]
enum SshRuntimeDir {
    Stable(PathBuf),
    Process(tempfile::TempDir),
}

#[cfg(unix)]
impl SshRuntimeDir {
    fn path(&self) -> &Path {
        match self {
            Self::Stable(path) => path,
            Self::Process(directory) => directory.path(),
        }
    }
}

#[cfg(unix)]
fn stable_ssh_runtime_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|runtime| runtime.is_absolute())
    {
        return runtime
            .join(zz_protocol::app_identity::DIRECTORY)
            .join("ssh");
    }
    ssh_runtime_root().join(user_ssh_dir_name())
}

#[cfg(unix)]
fn user_ssh_dir_name() -> String {
    format!(
        "{}-ssh-{}",
        zz_protocol::app_identity::DIRECTORY,
        rustix::process::geteuid().as_raw()
    )
}

#[cfg(unix)]
fn private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Err(io::Error::other("it is not a directory"));
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(io::Error::other("another user owns it"));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(io::Error::other(format!(
            "its mode is {:o}, not 700",
            metadata.mode() & 0o777
        )));
    }
    if path.to_str().is_none() {
        return Err(io::Error::other("it is not valid UTF-8"));
    }
    Ok(())
}

#[cfg(unix)]
fn remove_legacy_ssh_dirs(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_str().is_some_and(legacy_ssh_dir_name) {
            continue;
        }
        let path = entry.path();
        let settled = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= LEGACY_SSH_DIRECTORY_AGE);
        if settled {
            let _ = fs::remove_dir(&path);
        }
    }
}

#[cfg(unix)]
fn legacy_ssh_dir_name(name: &str) -> bool {
    name.strip_prefix("zz-ssh-").is_some_and(|suffix| {
        suffix.len() == 6
            && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
            && !suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[cfg(unix)]
fn ssh_runtime_root() -> PathBuf {
    std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|path| {
            path.is_absolute()
                && path.as_os_str().as_encoded_bytes().len() <= MAX_SSH_RUNTIME_ROOT_BYTES
        })
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

#[cfg(unix)]
fn control_master_path(endpoint: &SshEndpoint) -> io::Result<PathBuf> {
    Ok(ssh_runtime_dir()?.join(format!("c{:016x}", endpoint_hash(endpoint, None))))
}

#[cfg(unix)]
pub(crate) fn kept_forward_path(endpoint: &SshEndpoint) -> io::Result<PathBuf> {
    Ok(ssh_runtime_dir()?.join(format!(
        "c{:016x}.sock",
        endpoint_hash(endpoint, endpoint.remote_socket.as_deref())
    )))
}

#[cfg(unix)]
fn endpoint_hash(endpoint: &SshEndpoint, remote_socket: Option<&Path>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    endpoint.user.hash(&mut hasher);
    endpoint.host.hash(&mut hasher);
    endpoint.port.hash(&mut hasher);
    if let Some(remote_socket) = remote_socket {
        remote_socket.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(unix)]
fn control_master_alive(endpoint: &SshEndpoint, control_path: &Path) -> bool {
    control_path.exists()
        && ssh_control_command(endpoint, control_path, "check", &[])
            .status_unmasked()
            .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn ssh_control_command(
    endpoint: &SshEndpoint,
    control_path: &Path,
    request: &str,
    forwarding: &[(&str, &OsStr)],
) -> Command {
    let mut command = Command::new("ssh");
    command
        .arg("-O")
        .arg(request)
        .arg("-o")
        .arg(control_path_option(control_path));
    for (flag, spec) in forwarding {
        command.arg(flag).arg(spec);
    }
    if let Some(port) = endpoint.port {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(user) = &endpoint.user {
        command.arg("-l").arg(user);
    }
    command
        .arg("--")
        .arg(&endpoint.host)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(unix)]
fn probe_remote_socket(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
) -> Result<(PathBuf, Option<PathBuf>), EndpointError> {
    let (output, identity) = probe_login(endpoint, session, false)?;
    if !output.status.success() {
        return Err(probe_ssh_failure(endpoint, &output));
    }
    let probe = parse_remote_probe_output(&endpoint.to_string(), &output.stdout);
    note_key_enrollment(endpoint, session, &probe);
    probe.socket.map(|socket| (socket, identity))
}

#[cfg(any(unix, windows))]
fn sign_in(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
) -> Result<Option<PathBuf>, EndpointError> {
    use std::io::IsTerminal as _;

    #[cfg(not(windows))]
    let shared = session.control.is_some();
    #[cfg(windows)]
    let shared = false;
    if !shared {
        return zz_key_signs_in(endpoint, session);
    }
    let shows_login = session.askpass.is_none()
        && !session.batch
        && !batch_logins()
        && io::stderr().is_terminal();
    let (output, identity) = probe_login(endpoint, session, shows_login)?;
    if output.status.code() == Some(SSH_FAILURE_STATUS) {
        return Err(probe_ssh_failure(endpoint, &output));
    }
    let probe = parse_remote_probe_output(&endpoint.to_string(), &output.stdout);
    note_key_enrollment(endpoint, session, &probe);
    Ok(identity)
}

#[cfg(any(unix, windows))]
fn zz_key_signs_in(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
) -> Result<Option<PathBuf>, EndpointError> {
    let Some((path, _)) = zz_identity() else {
        return Ok(None);
    };
    let output = ssh_probe_command(
        endpoint,
        SshSession {
            identity: Some(&path),
            ..session
        },
        None,
    )
    .output_unmasked()
    .map_err(|source| EndpointError::SshSpawn {
        operation: "remote socket probe",
        source,
    })?;
    if refused_login(output.status.code(), &output.stderr) {
        return Ok(None);
    }
    if output.status.code() == Some(SSH_FAILURE_STATUS) {
        return Err(probe_ssh_failure(endpoint, &output));
    }
    Ok(Some(path))
}

#[cfg(any(unix, windows))]
fn probe_login(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    shows_login: bool,
) -> Result<(std::process::Output, Option<PathBuf>), EndpointError> {
    let identity = zz_identity();
    let key_line = identity
        .as_ref()
        .filter(|_| configured_fleet_endpoint(endpoint))
        .map(|(_, line)| line.as_str());
    let path = identity.as_ref().map(|(path, _)| path.as_path());
    let (output, signed_in) = zz_key_first(path, |phase| {
        let mut command = ssh_probe_command(
            endpoint,
            SshSession {
                identity: phase,
                ..session
            },
            key_line,
        );
        if shows_login && phase.is_none() {
            command.stderr(Stdio::inherit());
        }
        command
    })
    .map_err(|source| EndpointError::SshSpawn {
        operation: "remote socket probe",
        source,
    })?;
    Ok((output, path.filter(|_| signed_in).map(Path::to_owned)))
}

#[cfg(any(unix, windows))]
fn probe_ssh_failure(endpoint: &SshEndpoint, output: &std::process::Output) -> EndpointError {
    EndpointError::SshFailed {
        target: endpoint.to_string(),
        reason: ssh_failure_reason(
            &String::from_utf8_lossy(&output.stderr),
            &output.status.to_string(),
        ),
    }
}

#[cfg(any(unix, windows))]
fn configured_fleet_endpoint(endpoint: &SshEndpoint) -> bool {
    crate::configured_fleet_hosts().is_ok_and(|(hosts, _)| {
        hosts.iter().any(|host| {
            matches!(&host.endpoint, Endpoint::Ssh(configured)
                if configured.user == endpoint.user
                    && configured.host == endpoint.host
                    && configured.port == endpoint.port)
        })
    })
}

#[cfg(any(unix, windows))]
fn note_key_enrollment(endpoint: &SshEndpoint, session: SshSession<'_>, probe: &RemoteProbe) {
    match probe.key {
        Some(KeyEnrollment::Added) => {
            log::debug!(
                target: "zz_daemon::diagnostics",
                "added this machine's zz key to {endpoint}'s authorized_keys; its own zz key is {}",
                probe.pubkey.as_deref().unwrap_or("unknown"),
            );
            if let Some(askpass) = session.askpass {
                let notice = AskpassPrompt::new(AskpassMode::KeyAdded, endpoint.to_string());
                if let Err(error) = crate::askpass::notify(askpass.socket, &notice) {
                    log::debug!(
                        target: "zz_daemon::askpass",
                        "could not tell the window about {endpoint}'s new key: {error}",
                    );
                }
            }
        }
        Some(KeyEnrollment::Failed) => log::debug!(
            target: "zz_daemon::diagnostics",
            "could not add this machine's zz key to {endpoint}'s authorized_keys",
        ),
        Some(KeyEnrollment::Present) | None => {}
    }
}

#[derive(Debug)]
pub(crate) struct RemoteProbe {
    pub(crate) socket: Result<PathBuf, EndpointError>,
    pub(crate) key: Option<KeyEnrollment>,
    pub(crate) pubkey: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyEnrollment {
    Added,
    Present,
    Failed,
}

// Login profiles may print arbitrary noise around the probe's output; only the
// sentinel-prefixed lines are ours.
pub(crate) fn probe_value<'a>(stdout: &'a str, prefix: &str) -> Option<&'a str> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(str::trim)
}

pub(crate) fn parse_remote_probe_output(target: &str, output: &[u8]) -> RemoteProbe {
    let Ok(stdout) = std::str::from_utf8(output) else {
        return RemoteProbe {
            socket: Err(EndpointError::ProbeFailure {
                target: target.to_owned(),
                reason: "probe output was not valid UTF-8".to_owned(),
            }),
            key: None,
            pubkey: None,
        };
    };
    let key = match probe_value(stdout, "zz-probe-key=") {
        Some("added") => Some(KeyEnrollment::Added),
        Some("present") => Some(KeyEnrollment::Present),
        Some(_) => Some(KeyEnrollment::Failed),
        None => None,
    };
    let pubkey = probe_value(stdout, "zz-probe-pubkey=")
        .filter(|line| crate::ssh_key::is_zz_key_line(line))
        .map(str::to_owned);
    RemoteProbe {
        socket: remote_probe_socket(target, stdout),
        key,
        pubkey,
    }
}

fn remote_probe_socket(target: &str, stdout: &str) -> Result<PathBuf, EndpointError> {
    let probe_value = |prefix: &str| probe_value(stdout, prefix);
    let path = probe_value("zz-probe-socket=").unwrap_or_default();
    if path.is_empty() || !path.starts_with('/') {
        return Err(EndpointError::ProbeFailure {
            target: target.to_owned(),
            reason: "probe output did not include an absolute socket path".to_owned(),
        });
    }
    match probe_value("zz-probe-protocol=") {
        // No zz on the remote PATH says nothing about a running daemon; dial anyway.
        // A daemon that must be auto-started still fails with the install-zz message.
        Some("missing") => {
            log::debug!("zz is not on the login-shell PATH of {target}; skipping version gate");
        }
        Some("unknown") => {
            return Err(EndpointError::RemoteProtocolUnknown {
                target: target.to_owned(),
            });
        }
        Some(protocol) => {
            let daemon = protocol
                .parse::<u16>()
                .map_err(|_| EndpointError::ProbeFailure {
                    target: target.to_owned(),
                    reason: "probe protocol version was neither a number nor `unknown`".to_owned(),
                })?;
            if daemon != zz_protocol::PROTOCOL_VERSION {
                return Err(EndpointError::RemoteProtocolMismatch {
                    target: target.to_owned(),
                    daemon,
                    client: zz_protocol::PROTOCOL_VERSION,
                });
            }
        }
        None => {
            return Err(EndpointError::ProbeFailure {
                target: target.to_owned(),
                reason: "probe output did not include a protocol line".to_owned(),
            });
        }
    }
    Ok(PathBuf::from(path))
}

#[cfg(unix)]
fn ensure_remote_daemon(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    remote_socket: &Path,
) -> Result<(), EndpointError> {
    let target = endpoint.to_string();
    let output = ssh_daemon_start_command(endpoint, session, remote_socket)
        .output_unmasked()
        .map_err(|source| EndpointError::SshSpawn {
            operation: "remote daemon start",
            source,
        })?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(REMOTE_ZZ_MISSING_STATUS) => Err(EndpointError::RemoteBinaryMissing { target }),
        Some(REMOTE_DAEMON_TIMEOUT_STATUS) => {
            Err(EndpointError::RemoteDaemonUnavailable { target })
        }
        _ => Err(EndpointError::SshFailed {
            target,
            reason: ssh_failure_reason(
                &String::from_utf8_lossy(&output.stderr),
                &output.status.to_string(),
            ),
        }),
    }
}

#[cfg(any(unix, windows))]
fn start_askpass(endpoint: &SshEndpoint, prompts: Option<SshPrompts>) -> Option<AskpassListener> {
    prompts.and_then(|prompts| match AskpassListener::start(prompts) {
        Ok(listener) => Some(listener),
        Err(error) => {
            log::warn!(
                target: "zz_daemon::askpass",
                "ssh prompts are unavailable for {endpoint}: {error}",
            );
            None
        }
    })
}

#[cfg(any(unix, windows))]
fn session_askpass(askpass: Option<&AskpassListener>) -> Option<SshAskpass<'_>> {
    askpass.map(|listener| SshAskpass {
        helper: listener.helper(),
        socket: listener.socket(),
    })
}

#[cfg(unix)]
fn live_control(endpoint: &SshEndpoint) -> Option<(SshControl, bool)> {
    let control = configured_control(endpoint)?;
    let alive = control_master_alive(endpoint, &control.path);
    if !alive && control.path.exists() {
        let _ = fs::remove_file(&control.path);
    }
    Some((control, alive))
}

#[cfg(unix)]
fn configured_control(endpoint: &SshEndpoint) -> Option<SshControl> {
    ssh_config_command(endpoint)
        .output_unmasked()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| user_control(&String::from_utf8_lossy(&output.stdout)))
        .or_else(|| match control_master_path(endpoint) {
            Ok(path) => Some(SshControl {
                path,
                user_persist: false,
            }),
            Err(error) => {
                log::warn!(
                    target: "zz_daemon::diagnostics",
                    "ssh connection sharing is unavailable for {endpoint}: {error}",
                );
                None
            }
        })
}

#[cfg(unix)]
fn ssh_config_command(endpoint: &SshEndpoint) -> Command {
    let mut command = Command::new("ssh");
    command.arg("-G");
    if let Some(port) = endpoint.port {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(user) = &endpoint.user {
        command.arg("-l").arg(user);
    }
    command
        .arg("--")
        .arg(&endpoint.host)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(unix)]
fn user_control(config: &str) -> Option<SshControl> {
    let value = |key: &str| {
        config.lines().find_map(|line| {
            let (name, value) = line.split_once(' ')?;
            name.eq_ignore_ascii_case(key).then(|| value.trim())
        })
    };
    if !matches!(value("controlmaster"), Some("auto" | "true" | "yes")) {
        return None;
    }
    let path = value("controlpath")
        .filter(|path| path.starts_with('/') && !path.contains(char::is_whitespace))?;
    Some(SshControl {
        path: PathBuf::from(path),
        user_persist: !matches!(value("controlpersist"), None | Some("no" | "false")),
    })
}

#[cfg(any(unix, windows))]
pub fn run_remote_cli(
    endpoint: &SshEndpoint,
    arguments: &[String],
) -> Result<std::process::ExitStatus, EndpointError> {
    use std::io::IsTerminal as _;

    #[cfg(unix)]
    let (control, answered) = riding_control(endpoint);
    #[cfg(not(unix))]
    let answered = false;
    let session = SshSession {
        #[cfg(unix)]
        control: control.as_ref(),
        ..SshSession::default()
    };
    let identity = if answered {
        None
    } else {
        sign_in(endpoint, session)?
    };
    let session = SshSession {
        identity: identity.as_deref(),
        ..session
    };
    let mut command = ssh_cli_command(endpoint, session, arguments);
    if !io::stdin().is_terminal() {
        command.stdin(Stdio::null());
    }
    command
        .status_unmasked()
        .map_err(|source| EndpointError::SshSpawn {
            operation: "remote command",
            source,
        })
}

#[cfg(unix)]
fn riding_control(endpoint: &SshEndpoint) -> (Option<SshControl>, bool) {
    let Some((control, alive)) = live_control(endpoint) else {
        return (None, false);
    };
    if !alive {
        return (Some(control), false);
    }
    let step = SshStep {
        endpoint: endpoint.clone(),
        control: Some(control),
        askpass: None,
        identity: None,
    };
    let answered = match step.run(true, "remote command", master_answers) {
        Ok(()) => true,
        Err(error) => {
            log::debug!(
                target: "zz_daemon::diagnostics",
                "{error}; signing in again",
            );
            false
        }
    };
    (step.control, answered)
}

#[cfg(unix)]
fn master_answers(step: &SshStep) -> Result<(), EndpointError> {
    let mut command = Command::new("ssh");
    command.arg("-T");
    append_ssh_options(&mut command, &step.endpoint, step.session());
    command
        .arg("--")
        .arg(&step.endpoint.host)
        .arg("exit")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status_unmasked()
        .map(drop)
        .map_err(|source| EndpointError::SshSpawn {
            operation: "remote command",
            source,
        })
}

#[cfg(any(unix, windows, test))]
fn ssh_cli_command(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    arguments: &[String],
) -> Command {
    let mut script = format!("{}exec \"$zz_cli\"", remote_path_fallback());
    for argument in arguments {
        script.push(' ');
        script.push_str(&shell_quote(argument));
    }
    let mut command = Command::new("ssh");
    command.arg("-T");
    append_ssh_options(&mut command, endpoint, session);
    command
        .arg("--")
        .arg(&endpoint.host)
        .arg("sh")
        .arg("-lc")
        .arg(shell_quote(&script));
    command
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteInstall {
    pub installed: RemoteInstalled,
    pub restart: RemoteRestart,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteInstalled {
    Current,
    Headless,
    Desktop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteRestart {
    NotAsked,
    Restarted,
    NotRunning,
    Failed(String),
}

impl RemoteInstall {
    #[must_use]
    pub fn summary(&self) -> String {
        let name = zz_protocol::app_identity::DIRECTORY;
        let version = env!("CARGO_PKG_VERSION");
        let installed = match self.installed {
            RemoteInstalled::Current => format!("already {name} {version}"),
            RemoteInstalled::Headless => format!("{name} {version} installed (headless)"),
            RemoteInstalled::Desktop => format!("{name} {version} installed (desktop)"),
        };
        match &self.restart {
            RemoteRestart::NotAsked => installed,
            RemoteRestart::Restarted => format!("{installed}, daemon restarted"),
            RemoteRestart::NotRunning => format!("{installed}, no daemon was running"),
            RemoteRestart::Failed(reason) => {
                format!("{installed}, but its daemon did not stop: {reason}")
            }
        }
    }
}

#[cfg(any(unix, windows))]
pub fn install_remote(
    endpoint: &SshEndpoint,
    prompts: Option<SshPrompts>,
    restart_daemon: bool,
) -> Result<RemoteInstall, EndpointError> {
    install_remote_with(endpoint, prompts, restart_daemon, false)
}

#[cfg(any(unix, windows))]
pub fn install_remote_batch(
    endpoint: &SshEndpoint,
    restart_daemon: bool,
) -> Result<RemoteInstall, EndpointError> {
    install_remote_with(endpoint, None, restart_daemon, true)
}

#[cfg(any(unix, windows))]
fn install_remote_with(
    endpoint: &SshEndpoint,
    prompts: Option<SshPrompts>,
    restart_daemon: bool,
    batch: bool,
) -> Result<RemoteInstall, EndpointError> {
    let target = endpoint.to_string();
    if zz_protocol::app_identity::DEVELOPMENT {
        return Err(EndpointError::RemoteInstallFailed {
            target,
            reason: format!(
                "{} looks for {} there, and no release ships it",
                zz_protocol::app_identity::DISPLAY_NAME,
                zz_protocol::app_identity::DIRECTORY,
            ),
        });
    }
    let askpass = start_askpass(endpoint, prompts);
    #[cfg(unix)]
    let (control, answered) = riding_control(endpoint);
    #[cfg(not(unix))]
    let answered = false;
    let session = SshSession {
        #[cfg(unix)]
        control: control.as_ref(),
        askpass: session_askpass(askpass.as_ref()),
        identity: None,
        batch,
    };
    let identity = if answered {
        None
    } else {
        sign_in(endpoint, session)?
    };
    let session = SshSession {
        identity: identity.as_deref(),
        ..session
    };
    let spawn_failed = |source| EndpointError::SshSpawn {
        operation: "remote install",
        source,
    };
    let mut child = ssh_install_command(endpoint, session, restart_daemon)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_unmasked()
        .map_err(spawn_failed)?;
    if let Some(mut stdin) = child.stdin.take()
        && let Err(error) = stdin.write_all(INSTALL_SCRIPT.as_bytes())
    {
        log::debug!("the remote install for {endpoint} stopped reading its script: {error}");
    }
    let output = child.wait_with_output().map_err(spawn_failed)?;
    parse_remote_install_output(
        &target,
        output.status.code(),
        &output.stdout,
        &output.stderr,
        &output.status.to_string(),
    )
}

#[cfg(unix)]
fn forwarded_local_socket_path() -> io::Result<PathBuf> {
    let counter = FORWARD_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(ssh_runtime_dir()?.join(format!("fwd-{}-{counter}.sock", std::process::id())))
}

#[cfg(unix)]
pub(crate) fn connect_kept<T>(
    endpoint: &SshEndpoint,
    prompts: Option<SshPrompts>,
    resendable: bool,
    mut connect: impl FnMut(&Path, LocalStream) -> Result<T, DaemonError>,
) -> Result<(T, Option<SshForward>), DaemonError> {
    let kept = kept_forward_path(endpoint).map_err(|source| EndpointError::ForwardIo {
        target: endpoint.to_string(),
        action: "making a directory for",
        path: ssh_runtime_root(),
        source,
    })?;
    let mut dial = |socket: &Path| -> Result<T, DaemonError> {
        let stream = LocalTransport::connect(socket)?;
        let bound = stream.try_clone()?;
        bound.set_timeout(Some(MASTER_STEP_TIMEOUT))?;
        let connected = connect(socket, stream)?;
        bound.set_timeout(None)?;
        Ok(connected)
    };
    let stalled = match dial(&kept) {
        Ok(connected) => return Ok((connected, None)),
        Err(error) if !kept_forward_failed(&error) => return Err(error),
        Err(error) if !resendable && link_stalled(&error) => {
            return Err(give_up_on_stalled_link(endpoint).into());
        }
        Err(error) => {
            log::debug!(
                target: "zz_daemon::diagnostics",
                "the kept forward to {endpoint} did not answer ({error}); making it again",
            );
            link_stalled(&error)
        }
    };
    let lock = KeptForwardLock::acquire(&kept);
    if lock.contended
        && let Ok(connected) = dial(&kept)
    {
        return Ok((connected, None));
    }
    if stalled {
        abandon_stalled_master(endpoint, &kept);
    }
    let forward = SshForward::keep(endpoint, prompts, &kept)?;
    let connected = dial(forward.local_socket())?;
    Ok((connected, (!forward.kept).then_some(forward)))
}

#[cfg(unix)]
pub(crate) fn link_stalled(error: &DaemonError) -> bool {
    match error {
        DaemonError::Io(error) | DaemonError::Protocol(zz_protocol::ProtocolError::Io(error)) => {
            matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            )
        }
        _ => false,
    }
}

#[cfg(unix)]
fn abandon_stalled_master(endpoint: &SshEndpoint, kept: &Path) {
    log::debug!(
        target: "zz_daemon::diagnostics",
        "the ssh master for {endpoint} gave no answer within {} s; stopping it",
        MASTER_STEP_TIMEOUT.as_secs(),
    );
    if let Some(control) = configured_control(endpoint) {
        let _ = ssh_control_command(endpoint, &control.path, "stop", &[]).status_unmasked();
    }
    let _ = fs::remove_file(kept);
}

#[cfg(unix)]
fn kept_forward_failed(error: &DaemonError) -> bool {
    match error {
        DaemonError::Io(_) | DaemonError::Protocol(_) | DaemonError::IncompatibleDaemon { .. } => {
            true
        }
        DaemonError::Server(error) => {
            matches!(error, zz_protocol::ServerError::ProtocolMismatch { .. })
        }
        _ => false,
    }
}

#[cfg(unix)]
pub(crate) fn give_up_on_stalled_link(endpoint: &SshEndpoint) -> EndpointError {
    if let Ok(kept) = kept_forward_path(endpoint) {
        let lock = KeptForwardLock::acquire(&kept);
        if !lock.contended {
            abandon_stalled_master(endpoint, &kept);
        }
    }
    stopped_answering(endpoint)
}

#[cfg(unix)]
pub(crate) fn stopped_answering(endpoint: &SshEndpoint) -> EndpointError {
    EndpointError::StoppedAnswering {
        target: endpoint.to_string(),
        seconds: MASTER_STEP_TIMEOUT.as_secs(),
    }
}

#[cfg(all(unix, not(any(target_os = "ios", target_os = "android"))))]
pub(crate) fn remake_kept_forward(
    endpoint: &SshEndpoint,
    stalled: bool,
    answers: impl FnOnce(&Path) -> bool,
) -> Result<(PathBuf, Option<SshForward>), EndpointError> {
    let kept = kept_forward_path(endpoint).map_err(|source| EndpointError::ForwardIo {
        target: endpoint.to_string(),
        action: "making a directory for",
        path: ssh_runtime_root(),
        source,
    })?;
    let lock = KeptForwardLock::acquire(&kept);
    if lock.contended && answers(&kept) {
        return Ok((kept, None));
    }
    if stalled {
        abandon_stalled_master(endpoint, &kept);
    }
    let forward = SshForward::keep(endpoint, None, &kept)?;
    let socket = forward.local_socket.clone();
    Ok((socket, (!forward.kept).then_some(forward)))
}

#[cfg(unix)]
struct KeptForwardLock {
    _file: Option<fs::File>,
    contended: bool,
}

#[cfg(unix)]
impl KeptForwardLock {
    fn acquire(kept: &Path) -> Self {
        use std::os::unix::fs::OpenOptionsExt as _;

        let mut path = kept.as_os_str().to_owned();
        path.push(".lock");
        let Ok(file) = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(PathBuf::from(path))
        else {
            return Self {
                _file: None,
                contended: false,
            };
        };
        let contended = match file.try_lock() {
            Err(fs::TryLockError::WouldBlock) => file.lock().is_ok(),
            Ok(()) | Err(fs::TryLockError::Error(_)) => false,
        };
        Self {
            _file: Some(file),
            contended,
        }
    }
}

#[cfg(unix)]
pub(crate) const MASTER_STEP_TIMEOUT: Duration = Duration::from_secs(15);

#[cfg(unix)]
#[derive(Clone)]
struct SshStep {
    endpoint: SshEndpoint,
    control: Option<SshControl>,
    askpass: Option<(PathBuf, PathBuf)>,
    identity: Option<PathBuf>,
}

#[cfg(unix)]
impl SshStep {
    fn session(&self) -> SshSession<'_> {
        SshSession {
            control: self.control.as_ref(),
            askpass: self
                .askpass
                .as_ref()
                .map(|(helper, socket)| SshAskpass { helper, socket }),
            identity: self.identity.as_deref(),
            batch: false,
        }
    }

    fn run<T: Send + 'static>(
        &self,
        riding: bool,
        operation: &'static str,
        step: impl FnOnce(&Self) -> Result<T, EndpointError> + Send + 'static,
    ) -> Result<T, EndpointError> {
        self.run_within(MASTER_STEP_TIMEOUT, riding, operation, step)
    }

    fn run_within<T: Send + 'static>(
        &self,
        timeout: Duration,
        riding: bool,
        operation: &'static str,
        step: impl FnOnce(&Self) -> Result<T, EndpointError> + Send + 'static,
    ) -> Result<T, EndpointError> {
        let Some(control) = self.control.as_ref().filter(|_| riding) else {
            return step(self);
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let owned = self.clone();
        thread::Builder::new()
            .name("zz-ssh-step".to_owned())
            .spawn(move || {
                let _ = sender.send(step(&owned));
            })
            .map_err(|source| EndpointError::SshSpawn { operation, source })?;
        receiver.recv_timeout(timeout).unwrap_or_else(|_| {
            let _ =
                ssh_control_command(&self.endpoint, &control.path, "stop", &[]).status_unmasked();
            Err(EndpointError::SshFailed {
                target: self.endpoint.to_string(),
                reason: format!(
                    "the {operation} got no answer over the open ssh connection within {} s",
                    timeout.as_secs()
                ),
            })
        })
    }
}

#[cfg(unix)]
pub(crate) struct SshForward {
    child: Option<Child>,
    local_socket: PathBuf,
    endpoint: SshEndpoint,
    control: Option<SshControl>,
    master: bool,
    forwarding: OsString,
    socks_port: Option<u16>,
    askpass: Option<AskpassListener>,
    identity: Option<PathBuf>,
    kept: bool,
}

#[cfg(unix)]
impl SshForward {
    pub(crate) fn start(
        endpoint: &SshEndpoint,
        prompts: Option<SshPrompts>,
    ) -> Result<Self, EndpointError> {
        Self::open_new(endpoint, prompts, None)
    }

    fn keep(
        endpoint: &SshEndpoint,
        prompts: Option<SshPrompts>,
        kept: &Path,
    ) -> Result<Self, EndpointError> {
        Self::open_new(endpoint, prompts, Some(kept))
    }

    fn open_new(
        endpoint: &SshEndpoint,
        prompts: Option<SshPrompts>,
        kept: Option<&Path>,
    ) -> Result<Self, EndpointError> {
        let askpass = start_askpass(endpoint, prompts);
        let (control, riding) =
            live_control(endpoint).map_or((None, false), |(control, alive)| (Some(control), alive));
        let mut step = SshStep {
            endpoint: endpoint.clone(),
            control,
            askpass: askpass
                .as_ref()
                .map(|listener| (listener.helper().to_owned(), listener.socket().to_owned())),
            identity: None,
        };

        let (probed_socket, identity) = step.run(riding, "remote socket probe", |step| {
            probe_remote_socket(&step.endpoint, step.session())
        })?;
        step.identity = identity;
        let remote_socket = endpoint.remote_socket.clone().unwrap_or(probed_socket);
        if !remote_socket.is_absolute() {
            return Err(EndpointError::InvalidRemoteSocket(remote_socket));
        }
        let master = step
            .control
            .as_ref()
            .is_some_and(|control| riding || control_master_alive(endpoint, &control.path));
        let start_socket = remote_socket.clone();
        step.run(master, "remote daemon start", move |step| {
            ensure_remote_daemon(&step.endpoint, step.session(), &start_socket)
        })?;

        let socks = kept.is_none();
        let kept = kept.filter(|_| master);
        let local_socket = match kept {
            Some(kept) => kept.to_owned(),
            None => forwarded_local_socket_path().map_err(|source| EndpointError::ForwardIo {
                target: endpoint.to_string(),
                action: "making a directory for",
                path: ssh_runtime_root(),
                source,
            })?,
        };
        let mut forward = Self {
            child: None,
            forwarding: forwarding_spec(&local_socket, &remote_socket),
            local_socket,
            endpoint: endpoint.clone(),
            control: step.control,
            master,
            socks_port: socks.then(available_socks_port).flatten(),
            askpass,
            identity: step.identity,
            kept: kept.is_some(),
        };
        if forward.kept {
            forward.cancel_forwards();
            let _ = fs::remove_file(&forward.local_socket);
        }
        if let Err(error) = forward.open(&remote_socket) {
            let Some(port) = forward.socks_port else {
                return Err(error);
            };
            log::debug!(
                target: "zz_daemon::diagnostics",
                "ssh forward to {endpoint} failed while binding -D {}; retrying on a fresh port: {error}",
                socks_spec(port),
            );
            forward.restart_with_fresh_socks_port(&remote_socket)?;
        }

        let mut permissions = fs::metadata(&forward.local_socket)
            .map_err(|source| forward.io_error("reading permissions for", source))?
            .permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&forward.local_socket, permissions)
            .map_err(|source| forward.io_error("setting permissions on", source))?;
        Ok(forward)
    }

    pub(crate) fn local_socket(&self) -> &Path {
        &self.local_socket
    }

    pub(crate) const fn socks_port(&self) -> Option<u16> {
        self.socks_port
    }

    fn open(&mut self, remote_socket: &Path) -> Result<(), EndpointError> {
        let Some(control_path) = self
            .control
            .as_ref()
            .filter(|_| self.master)
            .map(|control| control.path.clone())
        else {
            let session = SshSession {
                control: self.control.as_ref(),
                askpass: session_askpass(self.askpass.as_ref()),
                identity: self.identity.as_deref(),
                batch: false,
            };
            let child = ssh_forward_command(
                &self.endpoint,
                session,
                &self.local_socket,
                remote_socket,
                self.socks_port,
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn_unmasked()
            .map_err(|source| EndpointError::SshSpawn {
                operation: "socket forward",
                source,
            })?;
            self.child = Some(child);
            return self.wait_for_socket();
        };
        let socks = self.socks_port.map(socks_spec);
        let mut forwarding = vec![("-L", self.forwarding.as_os_str())];
        if let Some(socks) = &socks {
            forwarding.push(("-D", OsStr::new(socks)));
        }
        let output = ssh_control_command(&self.endpoint, &control_path, "forward", &forwarding)
            .stderr(Stdio::piped())
            .output_unmasked()
            .map_err(|source| EndpointError::SshSpawn {
                operation: "socket forward",
                source,
            })?;
        if !output.status.success() {
            log::debug!(
                target: "zz_daemon::diagnostics",
                "ssh -O forward to {} failed: {}",
                self.endpoint,
                String::from_utf8_lossy(&output.stderr).trim(),
            );
            return Err(EndpointError::ForwardExited {
                target: self.endpoint.to_string(),
                status: output.status.to_string(),
                local_socket: self.local_socket.clone(),
            });
        }
        self.wait_for_socket()
    }

    fn restart_with_fresh_socks_port(&mut self, remote_socket: &Path) -> Result<(), EndpointError> {
        self.stop_child();
        self.cancel_forwards();
        let _ = fs::remove_file(&self.local_socket);
        self.socks_port = available_socks_port();
        self.open(remote_socket)
    }

    fn stop_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn cancel_forwards(&self) {
        let Some(control_path) = self.control.as_ref().map(|control| &control.path) else {
            return;
        };
        if !control_path.exists() {
            return;
        }
        let cancel = |forwarding| {
            let _ = ssh_control_command(&self.endpoint, control_path, "cancel", &[forwarding])
                .status_unmasked();
        };
        cancel(("-L", self.forwarding.as_os_str()));
        if let Some(port) = self.socks_port {
            let spec = socks_spec(port);
            cancel(("-D", OsStr::new(&spec)));
        }
    }

    fn wait_for_socket(&mut self) -> Result<(), EndpointError> {
        for _ in 0..FORWARD_POLL_ATTEMPTS {
            if self.socket_exists()? {
                return Ok(());
            }
            let exited = match &mut self.child {
                Some(child) => child
                    .try_wait()
                    .map_err(|source| self.io_error("checking ssh child for", source))?,
                None => None,
            };
            if let Some(status) = exited {
                if self.socket_exists()? {
                    return Ok(());
                }
                return Err(EndpointError::ForwardExited {
                    target: self.endpoint.to_string(),
                    status: status.to_string(),
                    local_socket: self.local_socket.clone(),
                });
            }
            thread::sleep(FORWARD_POLL_INTERVAL);
        }
        if self.socket_exists()? {
            return Ok(());
        }
        Err(EndpointError::ForwardTimeout {
            target: self.endpoint.to_string(),
            local_socket: self.local_socket.clone(),
        })
    }

    fn socket_exists(&self) -> Result<bool, EndpointError> {
        match fs::metadata(&self.local_socket) {
            Ok(metadata) => Ok(metadata.file_type().is_socket()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(self.io_error("inspecting", source)),
        }
    }

    fn io_error(&self, action: &'static str, source: io::Error) -> EndpointError {
        EndpointError::ForwardIo {
            target: self.endpoint.to_string(),
            action,
            path: self.local_socket.clone(),
            source,
        }
    }
}

#[cfg(unix)]
impl Drop for SshForward {
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        self.stop_child();
        self.cancel_forwards();
        let _ = fs::remove_file(&self.local_socket);
    }
}

#[cfg(windows)]
pub(crate) fn connect_kept<T>(
    endpoint: &SshEndpoint,
    prompts: Option<SshPrompts>,
    _resendable: bool,
    mut connect: impl FnMut(&Path, LocalStream) -> Result<T, DaemonError>,
) -> Result<(T, Option<SshForward>), DaemonError> {
    let forward = SshForward::start(endpoint, prompts)?;
    let socket = forward.local_socket();
    let connected = connect(socket, LocalTransport::connect(socket)?)?;
    Ok((connected, Some(forward)))
}

#[cfg(windows)]
const FORWARD_PIPE_ROLE: &str = "zz-fwd";

/// A pipe whose 128-bit random name is its only access control: Windows named pipes have no
/// `0600`, and a default descriptor grants read access to Everyone.
#[cfg(windows)]
pub(crate) fn ssh_pipe(role: &str, serial: u64) -> io::Result<(PathBuf, LocalListener)> {
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy).map_err(io::Error::other)?;
    let mut suffix = String::with_capacity(entropy.len() * 2);
    for byte in entropy {
        write!(&mut suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    let path = PathBuf::from(format!(
        r"\\.\pipe\{role}-{}-{serial}-{suffix}",
        std::process::id()
    ));
    let listener = LocalTransport::bind(&path)?;
    listener.set_nonblocking(true)?;
    Ok((path, listener))
}

/// Polled rather than woken by a self-connect: `interprocess` dials with an unbounded wait, so
/// poking a pipe that is already serving a client would hang the teardown.
#[cfg(windows)]
pub(crate) fn accept_until_stopped(
    listener: &LocalListener,
    stopped: &AtomicBool,
) -> Option<LocalStream> {
    loop {
        if stopped.load(Ordering::SeqCst) {
            return None;
        }
        match listener.accept() {
            Ok(stream) => return Some(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(error) => {
                log::debug!(target: "zz_daemon::diagnostics", "pipe listener stopped: {error}");
                return None;
            }
        }
    }
}

/// `Path::is_absolute` asks the local platform's question, which a Windows client would answer
/// wrongly about a remote POSIX path.
#[cfg(any(windows, target_os = "ios", target_os = "android"))]
pub(crate) fn remote_socket_is_absolute(path: &Path) -> bool {
    path.as_os_str().as_encoded_bytes().starts_with(b"/")
}

#[cfg(windows)]
fn ssh_connect_command(endpoint: &SshEndpoint, session: SshSession<'_>, script: &str) -> Command {
    let mut command = Command::new("ssh");
    append_ssh_options(&mut command, endpoint, session);
    command
        .arg("--")
        .arg(&endpoint.host)
        .arg("sh")
        .arg("-lc")
        .arg(shell_quote(script))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

// Drained off-thread because ssh blocks once its stderr pipe fills.
#[cfg(windows)]
struct SshStderr {
    text: Arc<Mutex<String>>,
    drain: Option<thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl SshStderr {
    fn drain(stderr: ChildStderr) -> Self {
        let text = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&text);
        let drain = thread::Builder::new()
            .name("zz-ssh-stderr".to_owned())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::debug!(target: "zz_daemon::diagnostics", "ssh: {line}");
                    let mut sink = sink.lock();
                    if sink.len() < MAX_SSH_STDERR_BYTES {
                        sink.push_str(&line);
                        sink.push('\n');
                    }
                }
            })
            .ok();
        Self { text, drain }
    }

    fn settled(&mut self) -> String {
        if let Some(drain) = self.drain.take() {
            let _ = drain.join();
        }
        self.text.lock().clone()
    }
}

// The Windows ssh client cannot bind a Unix-domain socket, so the remote runs `zz proxy` and this
// end bridges its stdio to a named pipe. A TCP `-L` forward would work but a Windows loopback port
// has no owner to check, where the unix arm chmods its forwarded socket 0600.
#[cfg(windows)]
pub(crate) struct SshForward {
    child: Child,
    local_socket: PathBuf,
    stopped: Arc<AtomicBool>,
    _askpass: Option<AskpassListener>,
}

#[cfg(windows)]
impl SshForward {
    pub(crate) fn start(
        endpoint: &SshEndpoint,
        prompts: Option<SshPrompts>,
    ) -> Result<Self, EndpointError> {
        let askpass = start_askpass(endpoint, prompts);
        let session = SshSession {
            askpass: session_askpass(askpass.as_ref()),
            ..SshSession::default()
        };
        let identity = zz_identity();
        let key_line = identity
            .as_ref()
            .filter(|_| configured_fleet_endpoint(endpoint))
            .map(|(_, line)| line.as_str());
        let script = remote_connect_script(endpoint.remote_socket.as_deref(), key_line);
        let (local_socket, listener) = ssh_pipe(
            FORWARD_PIPE_ROLE,
            FORWARD_COUNTER.fetch_add(1, Ordering::Relaxed),
        )
        .map_err(|source| EndpointError::ForwardIo {
            target: endpoint.to_string(),
            action: "creating",
            path: PathBuf::from(FORWARD_PIPE_ROLE),
            source,
        })?;

        let mut phase = identity.as_ref().map(|(path, _)| path.as_path());
        loop {
            let session = SshSession {
                identity: phase,
                ..session
            };
            let mut child = ssh_connect_command(endpoint, session, &script)
                .spawn_unmasked()
                .map_err(|source| EndpointError::SshSpawn {
                    operation: "socket proxy",
                    source,
                })?;
            end_with_this_process(&child);
            let (Some(stdin), Some(stdout), Some(stderr)) =
                (child.stdin.take(), child.stdout.take(), child.stderr.take())
            else {
                let _ = child.kill();
                return Err(EndpointError::ForwardIo {
                    target: endpoint.to_string(),
                    action: "opening a pipe to ssh for",
                    path: local_socket,
                    source: io::Error::other("ssh was spawned without redirected stdio"),
                });
            };

            let mut stderr = SshStderr::drain(stderr);
            let mut stdout = BufReader::new(stdout);
            let preamble = match await_proxy_ready(endpoint, &mut stdout, &local_socket) {
                Ok(ProxyStart::Ready(preamble)) => preamble,
                Ok(ProxyStart::Exited(preamble)) => {
                    let status = child.wait().ok();
                    let stderr = stderr.settled();
                    let code = status.and_then(|status| status.code());
                    if phase.is_some() && refused_login(code, stderr.as_bytes()) {
                        log::debug!(
                            target: "zz_daemon::diagnostics",
                            "the zz key did not sign in: {}",
                            stderr.trim(),
                        );
                        phase = None;
                        continue;
                    }
                    return Err(connect_failure(endpoint, status, &preamble, &stderr));
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            };
            let probe = parse_remote_probe_output(&endpoint.to_string(), &preamble);
            note_key_enrollment(endpoint, session, &probe);
            let probed = endpoint.remote_socket.clone().map_or(probe.socket, Ok);
            let stopped = Arc::new(AtomicBool::new(false));
            let started = probed
                .and_then(|socket| {
                    if remote_socket_is_absolute(&socket) {
                        Ok(())
                    } else {
                        Err(EndpointError::InvalidRemoteSocket(socket))
                    }
                })
                .and_then(|()| {
                    spawn_bridge(listener, stdin, stdout, Arc::clone(&stopped)).map_err(|source| {
                        EndpointError::ForwardIo {
                            target: endpoint.to_string(),
                            action: "starting the bridge for",
                            path: local_socket.clone(),
                            source,
                        }
                    })
                });
            if let Err(error) = started {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            return Ok(Self {
                child,
                local_socket,
                stopped,
                _askpass: askpass,
            });
        }
    }

    pub(crate) fn local_socket(&self) -> &Path {
        &self.local_socket
    }

    // Always absent: `ssh -D` is an unauthenticated loopback listener, and a Windows loopback port
    // has no owner to check against.
    #[allow(
        clippy::unused_self,
        reason = "the unix arm answers from its reserved port; both are called as SshForward::socks_port"
    )]
    pub(crate) const fn socks_port(&self) -> Option<u16> {
        None
    }
}

#[cfg(windows)]
enum ProxyStart {
    Ready(Vec<u8>),
    Exited(Vec<u8>),
}

#[cfg(windows)]
fn await_proxy_ready(
    endpoint: &SshEndpoint,
    stdout: &mut BufReader<ChildStdout>,
    local_socket: &Path,
) -> Result<ProxyStart, EndpointError> {
    let mut preamble = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read =
            stdout
                .read_until(b'\n', &mut line)
                .map_err(|source| EndpointError::ForwardIo {
                    target: endpoint.to_string(),
                    action: "reading the proxy handshake for",
                    path: local_socket.to_owned(),
                    source,
                })?;
        if read == 0 {
            return Ok(ProxyStart::Exited(preamble));
        }
        if line.ends_with(PROXY_READY_MARKER) {
            return Ok(ProxyStart::Ready(preamble));
        }
        if !line.starts_with(b"zz-probe-") {
            log::debug!(
                target: "zz_daemon::diagnostics",
                "remote login preamble: {}",
                String::from_utf8_lossy(&line).trim_end(),
            );
        }
        preamble.extend_from_slice(&line);
        if preamble.len() > MAX_PROXY_PREAMBLE_BYTES {
            return Err(EndpointError::SshFailed {
                target: endpoint.to_string(),
                reason: "the remote kept writing to stdout instead of starting the zz proxy; \
                         check what its login shell prints"
                    .to_owned(),
            });
        }
    }
}

#[cfg(windows)]
fn connect_failure(
    endpoint: &SshEndpoint,
    status: Option<std::process::ExitStatus>,
    preamble: &[u8],
    stderr: &str,
) -> EndpointError {
    let target = endpoint.to_string();
    let code = status.and_then(|status| status.code());
    if code != Some(SSH_FAILURE_STATUS)
        && preamble
            .windows(b"zz-probe-protocol=".len())
            .any(|window| window == b"zz-probe-protocol=")
        && let Err(error) = parse_remote_probe_output(&target, preamble).socket
    {
        return error;
    }
    match code {
        Some(REMOTE_ZZ_MISSING_STATUS) => EndpointError::RemoteBinaryMissing { target },
        Some(REMOTE_DAEMON_TIMEOUT_STATUS) => EndpointError::RemoteDaemonUnavailable { target },
        _ => EndpointError::SshFailed {
            target,
            reason: ssh_failure_reason(
                stderr,
                &status.map_or_else(|| "no exit status".to_owned(), |status| status.to_string()),
            ),
        },
    }
}

#[cfg(windows)]
fn end_with_this_process(child: &Child) {
    use std::{os::windows::io::AsRawHandle as _, sync::OnceLock};

    use windows::Win32::{
        Foundation::HANDLE,
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
    };

    static JOB: OnceLock<Option<usize>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).ok()?;
        #[allow(
            unsafe_code,
            reason = "job objects are raw Win32 entry points with no safe wrapper"
        )]
        // SAFETY: `limits` outlives the call and `size` is its exact size; the job handle is
        // never closed, so the kernel closes it when this process ends and kills the job.
        let job = unsafe {
            let job = CreateJobObjectW(None, windows::core::PCWSTR::null()).ok()?;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            )
            .ok()?;
            job
        };
        Some(job.0 as usize)
    });
    let Some(job) = *job else {
        return;
    };
    #[allow(
        unsafe_code,
        reason = "AssignProcessToJobObject is a raw Win32 entry point with no safe wrapper"
    )]
    // SAFETY: both handles are live: the job is never closed and `child` owns its process handle.
    let assigned =
        unsafe { AssignProcessToJobObject(HANDLE(job as *mut _), HANDLE(child.as_raw_handle())) };
    if let Err(error) = assigned {
        log::debug!(
            target: "zz_daemon::diagnostics",
            "ssh will outlive zz if zz is killed: {error}",
        );
    }
}

#[cfg(windows)]
fn spawn_bridge(
    listener: LocalListener,
    mut stdin: ChildStdin,
    mut stdout: BufReader<ChildStdout>,
    stopped: Arc<AtomicBool>,
) -> io::Result<()> {
    thread::Builder::new()
        .name("zz-ssh-bridge".to_owned())
        .spawn(move || {
            let Some(client) = accept_until_stopped(&listener, &stopped) else {
                return;
            };
            let Ok(mut to_ssh) = client.try_clone() else {
                return;
            };
            let mut from_ssh = client;
            if thread::Builder::new()
                .name("zz-ssh-bridge-up".to_owned())
                .spawn(move || drop(pump(&mut to_ssh, &mut stdin)))
                .is_err()
            {
                return;
            }
            drop(pump(&mut stdout, &mut from_ssh));
        })
        .map(drop)
}

#[cfg(windows)]
impl Drop for SshForward {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The remote half of the Windows attach path, run as `zz proxy` over ssh.
///
/// Connects to this host's daemon socket and pumps it to stdin and stdout.
pub fn run_socket_proxy(socket: &Path) -> io::Result<()> {
    let mut from_daemon = LocalTransport::connect(socket)?;
    let mut to_daemon = from_daemon.try_clone()?;

    let mut stdout = io::stdout().lock();
    stdout.write_all(PROXY_READY_MARKER)?;
    stdout.flush()?;

    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        drop(pump(&mut stdin, &mut to_daemon));
    });
    pump(&mut from_daemon, &mut stdout)
}

// Flushes every chunk: the protocol is framed binary with no newlines, and stdout is a
// `LineWriter`.
fn pump(reader: &mut impl io::Read, writer: &mut impl io::Write) -> io::Result<()> {
    let mut buffer = [0_u8; PUMP_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        writer.write_all(&buffer[..read])?;
        writer.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_path() {
        assert_eq!(
            Endpoint::parse("/tmp/zz.sock").unwrap(),
            Endpoint::Local(PathBuf::from("/tmp/zz.sock"))
        );
    }

    #[test]
    fn parses_unix_uri() {
        assert_eq!(
            Endpoint::parse("unix:///tmp/zz.sock").unwrap(),
            Endpoint::Local(PathBuf::from("/tmp/zz.sock"))
        );
    }

    #[test]
    fn parses_ssh_host() {
        assert_eq!(
            Endpoint::parse("ssh://desktop").unwrap(),
            Endpoint::Ssh(SshEndpoint {
                user: None,
                host: "desktop".to_owned(),
                port: None,
                remote_socket: None,
            })
        );
    }

    #[test]
    fn parses_full_ssh_uri() {
        assert_eq!(
            Endpoint::parse("ssh://user@host:2222/run/user/1000/zz/default.sock").unwrap(),
            Endpoint::Ssh(SshEndpoint {
                user: Some("user".to_owned()),
                host: "host".to_owned(),
                port: Some(2222),
                remote_socket: Some(PathBuf::from("/run/user/1000/zz/default.sock")),
            })
        );
    }

    #[test]
    fn rejects_unknown_scheme() {
        assert!(matches!(
            Endpoint::parse("tcp://host:7777"),
            Err(EndpointError::UriParse { .. })
        ));
    }

    #[test]
    fn rejects_removed_quic_scheme_with_an_ssh_pointer() {
        let error = Endpoint::parse("quic://gpu:7777").unwrap_err();
        assert!(matches!(error, EndpointError::UriParse { .. }));
        assert!(error.to_string().contains("use ssh://"));
    }

    #[test]
    fn rejects_empty_ssh_host() {
        assert!(matches!(
            Endpoint::parse("ssh:///tmp/default.sock"),
            Err(EndpointError::UriParse { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn builds_forward_ssh_argv_for_user_and_port_permutations() {
        for (user, port, options) in ssh_option_cases() {
            for socks_port in [None, Some(41_080)] {
                let endpoint = test_ssh_endpoint(user, port);
                let command = ssh_forward_command(
                    &endpoint,
                    test_session(),
                    Path::new("/tmp/local.sock"),
                    Path::new("/run/user/1000/zz/default.sock"),
                    socks_port,
                );
                let mut expected = vec![
                    "-N",
                    "-n",
                    "-o",
                    "ExitOnForwardFailure=yes",
                    "-o",
                    "StreamLocalBindMask=0177",
                    "-L",
                    "/tmp/local.sock:/run/user/1000/zz/default.sock",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
                if let Some(socks_port) = socks_port {
                    expected.extend(["-D".to_owned(), format!("127.0.0.1:{socks_port}")]);
                }
                expected.extend(shared_ssh_options());
                expected.extend(options.iter().copied().map(str::to_owned));
                expected.extend(["--", "host"].into_iter().map(str::to_owned));
                assert_eq!(command_args(&command), expected);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn control_cancel_names_the_forward_kind_it_gives_back() {
        let endpoint = test_ssh_endpoint(None, None);
        let control_path = Path::new(TEST_CONTROL_PATH);
        for (flag, spec) in [
            ("-L", "/tmp/local.sock:/run/zz.sock"),
            ("-D", "127.0.0.1:41080"),
        ] {
            for request in ["cancel", "forward"] {
                let command = ssh_control_command(
                    &endpoint,
                    control_path,
                    request,
                    &[(flag, OsStr::new(spec))],
                );
                let arguments = command_args(&command);
                assert!(
                    arguments
                        .windows(2)
                        .any(|pair| pair == [flag.to_owned(), spec.to_owned()]),
                    "{arguments:?}",
                );
                assert_eq!(arguments[..2], ["-O".to_owned(), request.to_owned()]);
            }
        }
        let both = command_args(&ssh_control_command(
            &endpoint,
            control_path,
            "forward",
            &[
                ("-L", OsStr::new("/tmp/local.sock:/run/zz.sock")),
                ("-D", OsStr::new("127.0.0.1:41080")),
            ],
        ));
        assert!(
            both.windows(4).any(|window| window
                == [
                    "-L",
                    "/tmp/local.sock:/run/zz.sock",
                    "-D",
                    "127.0.0.1:41080"
                ]),
            "{both:?}",
        );
        let check = command_args(&ssh_control_command(&endpoint, control_path, "check", &[]));
        assert!(
            !check
                .iter()
                .any(|argument| argument == "-L" || argument == "-D"),
            "{check:?}",
        );
    }

    #[cfg(unix)]
    #[test]
    fn builds_probe_ssh_argv_for_user_and_port_permutations() {
        for (user, port, options) in ssh_option_cases() {
            let endpoint = test_ssh_endpoint(user, port);
            let command = ssh_probe_command(&endpoint, test_session(), None);
            let mut expected = shared_ssh_options();
            expected.extend(options.into_iter().map(str::to_owned));
            expected.extend(
                ["--", "host", "sh", "-lc", &remote_socket_probe(None)]
                    .into_iter()
                    .map(str::to_owned),
            );
            assert_eq!(command_args(&command), expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn builds_autostart_ssh_argv_for_user_and_port_permutations() {
        let remote_socket = Path::new("/run/user/1000/zz/default.sock");
        for (user, port, options) in ssh_option_cases() {
            let endpoint = test_ssh_endpoint(user, port);
            let command = ssh_daemon_start_command(&endpoint, test_session(), remote_socket);
            let mut expected = shared_ssh_options();
            expected.extend(options.into_iter().map(str::to_owned));
            expected.extend(["--", "host", "sh", "-lc"].into_iter().map(str::to_owned));
            expected.push(shell_quote(&remote_daemon_start_script(remote_socket)));
            assert_eq!(command_args(&command), expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_user_master_keeps_its_own_persistence() {
        let endpoint = test_ssh_endpoint(None, None);
        for (user_persist, persists) in [(true, false), (false, true)] {
            let control = SshControl {
                path: PathBuf::from("/home/ada/.ssh/cm-f5de03fd"),
                user_persist,
            };
            let session = SshSession {
                control: Some(&control),
                ..SshSession::default()
            };
            let arguments = command_args(&ssh_probe_command(&endpoint, session, None));
            assert!(
                arguments.contains(&"ControlPath=/home/ada/.ssh/cm-f5de03fd".to_owned()),
                "{arguments:?}"
            );
            assert!(
                arguments.contains(&"ControlMaster=auto".to_owned()),
                "{arguments:?}"
            );
            assert_eq!(
                arguments
                    .iter()
                    .any(|argument| argument.starts_with("ControlPersist=")),
                persists,
                "{arguments:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn every_ssh_child_shares_one_multiplexing_master() {
        let endpoint = test_ssh_endpoint(Some("user"), Some(2222));
        let commands = [
            command_args(&ssh_probe_command(&endpoint, test_session(), None)),
            command_args(&ssh_daemon_start_command(
                &endpoint,
                test_session(),
                Path::new("/run/zz.sock"),
            )),
            command_args(&ssh_forward_command(
                &endpoint,
                test_session(),
                Path::new("/tmp/local.sock"),
                Path::new("/run/zz.sock"),
                Some(41_080),
            )),
        ];
        for arguments in &commands {
            assert!(
                arguments.contains(&"ControlMaster=auto".to_owned()),
                "{arguments:?}"
            );
            assert!(
                arguments.contains(&"ControlPath=/tmp/zz-ssh/c0".to_owned()),
                "{arguments:?}",
            );
            assert!(
                arguments.contains(&"ControlPersist=600".to_owned()),
                "{arguments:?}",
            );
            assert!(
                !arguments
                    .iter()
                    .any(|argument| argument.contains("BatchMode")),
                "{arguments:?}",
            );
        }
    }

    #[test]
    fn a_session_without_a_master_uses_platform_safe_options() {
        let endpoint = test_ssh_endpoint(None, None);
        let command = ssh_probe_command(&endpoint, SshSession::default(), None);
        let mut expected = [
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=5",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "Compression=yes",
        ]
        .map(str::to_owned)
        .to_vec();
        #[cfg(windows)]
        expected.extend(["-o", "ControlMaster=no", "-o", "ControlPath=none"].map(str::to_owned));
        expected.extend(["--", "host", "sh", "-lc", &remote_socket_probe(None)].map(str::to_owned));
        assert_eq!(command_args(&command), expected);
        assert_eq!(
            command_envs(&command),
            Vec::<(String, Option<String>)>::new()
        );
    }

    #[test]
    fn remote_probe_requires_a_matching_reported_protocol() {
        let output = format!(
            "zz-probe-socket=/run/user/1000/zz/default.sock\nzz-probe-protocol={}\n",
            zz_protocol::PROTOCOL_VERSION
        );
        assert_eq!(
            parse_remote_probe_output("host", output.as_bytes())
                .socket
                .unwrap(),
            Path::new("/run/user/1000/zz/default.sock")
        );

        assert!(matches!(
            parse_remote_probe_output(
                "host",
                b"zz-probe-socket=/run/zz.sock\nzz-probe-protocol=48\n"
            )
            .socket,
            Err(EndpointError::RemoteProtocolMismatch {
                target,
                daemon: 48,
                client: zz_protocol::PROTOCOL_VERSION,
            }) if target == "host"
        ));
        assert_eq!(
            parse_remote_probe_output(
                "host",
                b"zz-probe-socket=/run/zz.sock\nzz-probe-protocol=unknown\n"
            )
            .socket
            .unwrap_err()
            .to_string(),
            "zz on host predates protocol version reporting; update it, then reconnect"
        );
        assert!(matches!(
            parse_remote_probe_output(
                "host",
                b"zz-probe-socket=/run/zz.sock\nzz-probe-protocol=banana\n"
            )
            .socket,
            Err(EndpointError::ProbeFailure { .. })
        ));
    }

    #[test]
    fn remote_probe_tolerates_login_profile_noise_and_a_pathless_zz() {
        // A chatty ~/.profile prints around the sentinel lines under `sh -lc`.
        let noisy = format!(
            "Welcome to devbox!\nzz-probe-socket=/run/zz.sock\nmotd: 3 updates\nzz-probe-protocol={}\n",
            zz_protocol::PROTOCOL_VERSION
        );
        assert_eq!(
            parse_remote_probe_output("host", noisy.as_bytes())
                .socket
                .unwrap(),
            Path::new("/run/zz.sock")
        );

        // No zz on the login-shell PATH must not block dialing a running daemon.
        assert_eq!(
            parse_remote_probe_output(
                "host",
                b"zz-probe-socket=/run/zz.sock\nzz-probe-protocol=missing\n"
            )
            .socket
            .unwrap(),
            Path::new("/run/zz.sock")
        );

        assert!(matches!(
            parse_remote_probe_output("host", b"zz-probe-socket=/run/zz.sock\n").socket,
            Err(EndpointError::ProbeFailure { .. })
        ));
        assert!(matches!(
            parse_remote_probe_output("host", b"only noise, no sentinels\n").socket,
            Err(EndpointError::ProbeFailure { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn zz_key_phase_offers_only_the_zz_identity() {
        let endpoint = test_ssh_endpoint(Some("user"), None);
        let identity = Path::new("/Users/a/Library/Application Support/zz-100%/ssh/id_ed25519");
        let line = "ssh-ed25519 AAAAzz zz@desk";
        let mut expected = [
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "IdentityFile=\"/Users/a/Library/Application Support/zz-100%%/ssh/id_ed25519\"",
            "-o",
            "PreferredAuthentications=publickey",
            "-o",
            "BatchMode=yes",
        ]
        .map(str::to_owned)
        .to_vec();
        expected.extend(shared_ssh_options());
        expected.extend(["-l", "user", "--", "host", "sh", "-lc"].map(str::to_owned));
        expected.push(remote_socket_probe(Some(line)));
        assert_eq!(
            command_args(&ssh_probe_command(
                &endpoint,
                SshSession {
                    identity: Some(identity),
                    ..test_session()
                },
                Some(line)
            )),
            expected
        );

        let fallback = command_args(&ssh_probe_command(&endpoint, test_session(), Some(line)));
        assert_eq!(fallback[..shared_ssh_options().len()], shared_ssh_options());
        for option in [
            "IdentitiesOnly",
            "IdentityFile",
            "PreferredAuthentications",
            "BatchMode",
        ] {
            assert!(
                !fallback.iter().any(|argument| argument.starts_with(option)),
                "the user's own login must keep OpenSSH's default identities: {fallback:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_login_the_zz_key_won_and_a_batch_login_reach_every_later_command() {
        let endpoint = test_ssh_endpoint(None, None);
        let identity = Path::new("/home/a/.local/share/zz/ssh/id_ed25519");
        let signed_in = SshSession {
            identity: Some(identity),
            ..test_session()
        };
        let batch = SshSession {
            batch: true,
            ..test_session()
        };
        let socket = Path::new("/run/zz.sock");
        let local = Path::new("/tmp/local.sock");
        let commands = |session| {
            [
                ssh_daemon_start_command(&endpoint, session, socket),
                ssh_forward_command(&endpoint, session, local, socket, None),
                ssh_cli_command(&endpoint, session, &["protocol-version".to_owned()]),
                ssh_install_command(&endpoint, session, false),
            ]
            .map(|command| command_args(&command))
        };
        for arguments in commands(signed_in) {
            assert!(
                arguments.contains(&format!("IdentityFile=\"{}\"", identity.display())),
                "{arguments:?}"
            );
            assert!(
                arguments.contains(&"BatchMode=yes".to_owned()),
                "{arguments:?}"
            );
            assert!(
                !arguments.contains(&NO_SECURITY_KEYS.to_owned()),
                "{arguments:?}"
            );
        }
        for arguments in commands(batch) {
            assert!(
                arguments.contains(&"BatchMode=yes".to_owned()),
                "{arguments:?}"
            );
            assert!(
                arguments.contains(&NO_SECURITY_KEYS.to_owned()),
                "{arguments:?}"
            );
            assert!(
                !arguments
                    .iter()
                    .any(|argument| argument.starts_with("IdentityFile")),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn only_a_login_a_person_can_answer_is_retried_on_the_terminal() {
        let failed = |stderr: &str| EndpointError::SshFailed {
            target: "ssh://host".to_owned(),
            reason: ssh_failure_reason(stderr, "exit status: 255"),
        };
        for stderr in [
            "you@host: Permission denied (publickey).",
            "Host key verification failed.",
            "sign_and_send_pubkey: signing failed for ED25519-SK \"/k\": device not found",
        ] {
            assert!(failed(stderr).needs_sign_in(), "{stderr}");
        }
        for stderr in [
            "ssh: connect to host host port 22: Operation timed out",
            "ssh: Could not resolve hostname host: nodename nor servname",
            "bash: line 1: zz: command not found",
        ] {
            assert!(!failed(stderr).needs_sign_in(), "{stderr}");
        }
        assert!(
            !EndpointError::RemoteBinaryMissing {
                target: "ssh://host".to_owned()
            }
            .needs_sign_in()
        );
    }

    #[test]
    fn only_a_refused_login_falls_back_to_the_users_own() {
        for stderr in [
            "user@host: Permission denied (publickey).",
            "Host key verification failed.",
            "sign_and_send_pubkey: signing failed for ED25519-SK \"/k\": device not found",
        ] {
            assert!(refused_login(Some(255), stderr.as_bytes()), "{stderr}");
        }
        for (code, stderr) in [
            (
                Some(255),
                "ssh: connect to host desk port 22: Operation timed out",
            ),
            (
                Some(255),
                "ssh: Could not resolve hostname desk: nodename nor servname",
            ),
            (
                Some(255),
                "ssh: connect to host desk port 22: Connection refused",
            ),
            (Some(1), "mkdir: Permission denied"),
            (None, "Permission denied"),
        ] {
            assert!(!refused_login(code, stderr.as_bytes()), "{code:?} {stderr}");
        }
    }

    #[cfg(unix)]
    fn path_with_a_current_zz() -> (tempfile::TempDir, String) {
        let bin = tempfile::tempdir().expect("temporary bin");
        let zz = bin.path().join(zz_protocol::app_identity::DIRECTORY);
        fs::write(
            &zz,
            format!(
                "#!/bin/sh\ncase \"$1\" in protocol-version) echo {};; esac\n",
                zz_protocol::PROTOCOL_VERSION
            ),
        )
        .expect("write zz");
        fs::set_permissions(&zz, fs::Permissions::from_mode(0o755)).expect("make zz runnable");
        let path = format!("{}:/usr/bin:/bin", bin.path().display());
        (bin, path)
    }

    #[cfg(unix)]
    #[test]
    fn the_probe_enrolls_this_machine_once_and_reports_the_hosts_own_key() {
        let line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGW3/OPNnD2l/h6LfRGUUDgkTB5X9f1CU1CuD1GC4d5u zz@hub";
        let (_bin, path) = path_with_a_current_zz();
        let shells = ["/bin/sh", "/bin/tcsh"]
            .into_iter()
            .filter(|shell| Path::new(shell).exists());
        for shell in shells {
            let home = tempfile::tempdir().expect("temporary home");
            let probe = || {
                let output = Command::new(shell)
                    .args(["-c", &format!("sh -c {}", remote_socket_probe(Some(line)))])
                    .env("HOME", home.path())
                    .env("PATH", &path)
                    .env_remove("XDG_DATA_HOME")
                    .env_remove("XDG_RUNTIME_DIR")
                    .output()
                    .expect("run the probe");
                assert!(output.status.success(), "{shell}: {output:?}");
                parse_remote_probe_output("host", &output.stdout)
            };

            let first = probe();
            assert!(first.socket.is_ok(), "{shell}: {:?}", first.socket);
            assert_eq!(first.key, Some(KeyEnrollment::Added), "{shell}");
            let host_key = first.pubkey.expect("the probe makes the host's own key");
            assert!(host_key.starts_with("ssh-ed25519 "), "{host_key}");

            let second = probe();
            assert_eq!(second.key, Some(KeyEnrollment::Present), "{shell}");
            assert_eq!(second.pubkey.as_deref(), Some(host_key.as_str()));

            let ssh = home.path().join(".ssh");
            let authorized = ssh.join("authorized_keys");
            assert_eq!(
                fs::read_to_string(&authorized).unwrap(),
                format!("{line}\n")
            );
            assert_eq!(
                fs::metadata(&ssh).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&authorized).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let data = if cfg!(target_os = "macos") {
                home.path().join("Library/Application Support")
            } else {
                home.path().join(".local/share")
            };
            let key_directory = data.join(zz_protocol::app_identity::DIRECTORY).join("ssh");
            assert_eq!(
                fs::metadata(&key_directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(key_directory.join("id_ed25519"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_probe_without_a_key_line_writes_nothing() {
        let home = tempfile::tempdir().expect("temporary home");
        let (_bin, path) = path_with_a_current_zz();
        let output = Command::new("/bin/sh")
            .args(["-c", &format!("sh -c {}", remote_socket_probe(None))])
            .env("HOME", home.path())
            .env("PATH", &path)
            .env_remove("XDG_DATA_HOME")
            .output()
            .expect("run the probe");
        let probe = parse_remote_probe_output("host", &output.stdout);
        assert!(probe.socket.is_ok());
        assert_eq!((probe.key, probe.pubkey), (None, None));
        assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_probe_reports_enrollment_and_only_a_well_formed_host_key() {
        let output = format!(
            "zz-probe-key=added\nzz-probe-pubkey=ssh-ed25519 AAAAzz zz@xps\n\
             zz-probe-socket=/run/zz.sock\nzz-probe-protocol={}\n",
            zz_protocol::PROTOCOL_VERSION
        );
        let probe = parse_remote_probe_output("host", output.as_bytes());
        assert_eq!(probe.key, Some(KeyEnrollment::Added));
        assert_eq!(probe.pubkey.as_deref(), Some("ssh-ed25519 AAAAzz zz@xps"));
        assert!(probe.socket.is_ok());

        let probe = parse_remote_probe_output(
            "host",
            b"zz-probe-key=failed\nzz-probe-pubkey=command=\"id\" ssh-ed25519 AAAAzz zz@xps\n\
              zz-probe-socket=/run/zz.sock\nzz-probe-protocol=48\n",
        );
        assert_eq!(probe.key, Some(KeyEnrollment::Failed));
        assert_eq!(probe.pubkey, None);
        assert!(matches!(
            probe.socket,
            Err(EndpointError::RemoteProtocolMismatch { daemon: 48, .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_percent_in_the_control_path_is_escaped_not_expanded() {
        assert_eq!(
            control_path_option(Path::new("/tmp/zz-100%-a/c1")),
            "ControlPath=/tmp/zz-100%%-a/c1",
        );
    }

    #[cfg(unix)]
    #[test]
    fn askpass_reaches_every_ssh_child_through_the_environment() {
        let endpoint = test_ssh_endpoint(None, None);
        let helper = PathBuf::from("/opt/zz/bin/zz");
        let socket = PathBuf::from("/tmp/zz-ssh/a1");
        let session = SshSession {
            askpass: Some(SshAskpass {
                helper: &helper,
                socket: &socket,
            }),
            ..SshSession::default()
        };
        for command in [
            ssh_probe_command(&endpoint, session, None),
            ssh_daemon_start_command(&endpoint, session, Path::new("/run/zz.sock")),
            ssh_forward_command(
                &endpoint,
                session,
                Path::new("/tmp/local.sock"),
                Path::new("/run/zz.sock"),
                Some(41_080),
            ),
        ] {
            let envs = command_envs(&command);
            assert!(
                envs.contains(&("SSH_ASKPASS".to_owned(), Some("/opt/zz/bin/zz".to_owned()))),
                "{envs:?}",
            );
            assert!(
                envs.contains(&("SSH_ASKPASS_REQUIRE".to_owned(), Some("force".to_owned()))),
                "{envs:?}",
            );
            assert!(
                envs.contains(&(
                    "ZZ_ASKPASS_SOCKET".to_owned(),
                    Some("/tmp/zz-ssh/a1".to_owned()),
                )),
                "{envs:?}",
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn remote_cli_runs_the_hosts_own_zz_with_quoted_arguments() {
        let endpoint = test_ssh_endpoint(Some("user"), Some(2222));
        let arguments = ["fleet", "add", "box", "ssh://ada@box", "it's"].map(str::to_owned);
        let command = ssh_cli_command(&endpoint, test_session(), &arguments);
        let mut expected = vec!["-T".to_owned()];
        expected.extend(shared_ssh_options());
        expected.extend(["-p", "2222", "-l", "user", "--", "host", "sh", "-lc"].map(str::to_owned));
        expected.push(shell_quote(&format!(
            "{}exec \"$zz_cli\" 'fleet' 'add' 'box' 'ssh://ada@box' 'it'\\''s'",
            remote_path_fallback()
        )));
        assert_eq!(command_args(&command), expected);

        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        let cli = bin.join(zz_protocol::app_identity::DIRECTORY);
        fs::write(
            &cli,
            "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]' \"$a\"; done; exit 7\n",
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        let script = command_args(&command).pop().unwrap();
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("sh -c {script}"))
            .env("HOME", directory.path())
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "[fleet][add][box][ssh://ada@box][it's]"
        );
    }

    #[test]
    fn every_remote_script_looks_where_the_installer_puts_the_cli() {
        let scripts = [
            remote_socket_probe(None),
            remote_daemon_start_script(Path::new("/run/zz.sock")),
            remote_proxy_script(Path::new("/run/zz.sock")),
            remote_install_script(Some(Path::new("/run/zz.sock")), true),
        ];
        for script in scripts {
            for dir in ["$HOME/.local/bin", "/opt/homebrew/bin", "/usr/local/bin"] {
                assert!(script.contains(dir), "{dir} missing from: {script}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn one_windows_login_probes_starts_the_daemon_and_runs_the_proxy() {
        let home = tempfile::tempdir().expect("temporary home");
        let bin = home.path().join("bin");
        fs::create_dir(&bin).expect("bin directory");
        let socket = home.path().join("s.sock");
        let _daemon = std::os::unix::net::UnixListener::bind(&socket).expect("daemon socket");
        let zz = bin.join(zz_protocol::app_identity::DIRECTORY);
        let script = remote_connect_script(Some(&socket), None);
        let run = || {
            Command::new("/bin/sh")
                .args(["-c", &format!("sh -c {}", shell_quote(&script))])
                .env("HOME", home.path())
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .output()
                .expect("run the connect script")
        };
        let ours = zz_protocol::PROTOCOL_VERSION.to_string();
        for (protocol, proxied) in [(ours.as_str(), true), ("1", false)] {
            fs::write(
                &zz,
                format!(
                    "#!/bin/sh\ncase \"$1\" in protocol-version) echo {protocol};; \
                     proxy) echo \"proxy $3\";; esac\n"
                ),
            )
            .expect("write zz");
            fs::set_permissions(&zz, fs::Permissions::from_mode(0o755)).expect("make zz runnable");
            let output = run();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(output.status.code(), Some(0), "{output:?}");
            assert!(
                stdout.contains(&format!("zz-probe-protocol={protocol}\n")),
                "{stdout}"
            );
            assert_eq!(
                stdout.ends_with(&format!("proxy {}\n", socket.display())),
                proxied,
                "{stdout}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn remote_probe_and_proxy_select_the_builds_executable() {
        let directory = tempfile::tempdir().expect("temporary directory");
        for name in ["zz", "zz-dev"] {
            let binary = directory.path().join(name);
            fs::write(&binary, format!("#!/bin/sh\nprintf '%s\\n' '{name}'\n"))
                .expect("write executable");
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
                .expect("executable permissions");
        }
        let home = tempfile::tempdir().expect("temporary home");
        let path = format!("{}:/usr/bin:/bin", directory.path().display());
        let name = zz_protocol::app_identity::DIRECTORY;
        for (script, expected) in [
            (
                remote_socket_probe(None),
                format!("zz-probe-protocol={name}\n"),
            ),
            (
                shell_quote(&remote_proxy_script(Path::new("/tmp/explicit.sock"))),
                format!("{name}\n"),
            ),
        ] {
            let output = Command::new("/bin/sh")
                .args(["-c", &format!("sh -c {script}")])
                .env("PATH", &path)
                .env("HOME", home.path())
                .output()
                .expect("run remote script");
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).ends_with(&expected));
        }
    }

    #[cfg(unix)]
    #[test]
    fn remote_scripts_prefer_the_managed_copy_over_an_earlier_path_zz() {
        let name = zz_protocol::app_identity::DIRECTORY;
        let home = tempfile::tempdir().expect("temporary home");
        let package = tempfile::tempdir().expect("package prefix");
        let managed = home.path().join(".local/bin");
        fs::create_dir_all(&managed).expect("managed bin");
        for (directory, label) in [(package.path(), "package"), (managed.as_path(), "managed")] {
            let binary = directory.join(name);
            fs::write(
                &binary,
                format!(
                    "#!/bin/sh\nif [ \"$1\" = daemon ]; then : > \"$HOME/{label}-ran\"; fi\n\
                     printf '%s\\n' '{label}'\n"
                ),
            )
            .expect("write executable");
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
                .expect("executable permissions");
        }
        let socket_dir = tempfile::tempdir().expect("socket directory");
        let socket = socket_dir.path().join("zz.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind socket");
        let path = format!("{}:/usr/bin:/bin", package.path().display());
        let run = |script: String| {
            let output = Command::new("/bin/sh")
                .args(["-c", &format!("sh -c {script}")])
                .env("PATH", &path)
                .env("HOME", home.path())
                .output()
                .expect("run remote script");
            assert!(output.status.success(), "{output:?}");
            String::from_utf8_lossy(&output.stdout).into_owned()
        };

        assert!(run(remote_socket_probe(None)).ends_with("zz-probe-protocol=managed\n"));
        assert!(run(shell_quote(&remote_proxy_script(&socket))).ends_with("managed\n"));
        run(shell_quote(&remote_daemon_start_script(&socket)));
        let started = home.path().join("managed-ran");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !started.exists() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(started.exists(), "the auto-start ran another zz");
        assert!(!home.path().join("package-ran").exists());
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().expect("parent directory")).unwrap();
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    fn fake_zz(protocol: &str, calls: &str) -> String {
        format!(
            "#!/bin/sh\ncase \"$1\" in protocol-version) echo {protocol} ;; \
             --version) echo \"zz 0.0.1\" ;; *) echo \"$*\" >> \"$HOME/{calls}\" ;; esac\n"
        )
    }

    #[cfg(unix)]
    fn fake_installer(protocol: &str) -> String {
        format!(
            "printf '%s\\n' \"$*\" > \"$HOME/installer-args\"\n\
             zz_body='{body}'\n\
             case \"$*\" in\n\
             \"--headless --version {version}\")\n\
             mkdir -p \"$HOME/.local/bin\"; rm -f \"$HOME/.local/bin/{name}\"\n\
             printf '%s' \"$zz_body\" > \"$HOME/.local/bin/{name}\"\n\
             chmod 755 \"$HOME/.local/bin/{name}\" ;;\n\
             \"--prefix $HOME/.local --version {version}\")\n\
             rm -rf \"$HOME/.local/lib/zz\"; mkdir -p \"$HOME/.local/lib/zz\" \"$HOME/.local/bin\"\n\
             printf '%s' \"$zz_body\" > \"$HOME/.local/lib/zz/cli\"\n\
             chmod 755 \"$HOME/.local/lib/zz/cli\"\n\
             ln -sfn ../lib/zz/cli \"$HOME/.local/bin/{name}\" ;;\n\
             *) echo \"error: unexpected arguments $*\" >&2; exit 1 ;;\n\
             esac\n",
            body = fake_zz(protocol, "calls"),
            version = env!("CARGO_PKG_VERSION"),
            name = zz_protocol::app_identity::DIRECTORY,
        )
    }

    #[cfg(unix)]
    fn run_install_script(
        shell: &str,
        home: &Path,
        path: &str,
        installer: &str,
        restart: bool,
    ) -> Result<RemoteInstall, EndpointError> {
        let script = shell_quote(&remote_install_script(
            Some(Path::new("/run/zz.sock")),
            restart,
        ));
        let mut child = Command::new(shell)
            .args(["-c", &format!("sh -c {script}")])
            .env("HOME", home)
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn_unmasked()
            .expect("run install script");
        let _ = child
            .stdin
            .take()
            .expect("install stdin")
            .write_all(installer.as_bytes());
        let output = child.wait_with_output().expect("install output");
        parse_remote_install_output(
            "ssh://desk",
            output.status.code(),
            &output.stdout,
            &output.stderr,
            &output.status.to_string(),
        )
    }

    #[cfg(unix)]
    fn install_shells() -> impl Iterator<Item = &'static str> {
        ["/bin/sh", "/bin/tcsh"]
            .into_iter()
            .filter(|shell| Path::new(shell).exists())
    }

    #[cfg(unix)]
    fn installer_args(home: &Path) -> String {
        fs::read_to_string(home.join("installer-args")).unwrap_or_default()
    }

    #[cfg(unix)]
    #[test]
    fn remote_install_replaces_its_own_binary_and_restarts_the_daemon() {
        for shell in install_shells() {
            let home = tempfile::tempdir().expect("temporary home");
            let bin = home.path().join(".local/bin");
            write_executable(
                &bin.join(zz_protocol::app_identity::DIRECTORY),
                "#!/bin/sh\necho 1\n",
            );
            let path = format!("{}:/usr/bin:/bin", bin.display());
            let installer = fake_installer(&zz_protocol::PROTOCOL_VERSION.to_string());

            let outcome = run_install_script(shell, home.path(), &path, &installer, true)
                .unwrap_or_else(|error| panic!("{shell}: {error}"));
            assert_eq!(
                outcome,
                RemoteInstall {
                    installed: RemoteInstalled::Headless,
                    restart: RemoteRestart::Restarted,
                    warnings: Vec::new(),
                },
                "{shell}",
            );
            assert_eq!(
                installer_args(home.path()),
                format!("--headless --version {}\n", env!("CARGO_PKG_VERSION")),
                "{shell}",
            );
            assert_eq!(
                fs::read_to_string(home.path().join("calls")).unwrap(),
                "--socket /run/zz.sock kill-server\n",
                "{shell}",
            );

            fs::remove_file(home.path().join("calls")).unwrap();
            write_executable(
                &bin.join(zz_protocol::app_identity::DIRECTORY),
                "#!/bin/sh\necho 1\n",
            );
            let outcome = run_install_script(shell, home.path(), &path, &installer, false).unwrap();
            assert_eq!(outcome.restart, RemoteRestart::NotAsked, "{shell}");
            assert!(!home.path().join("calls").exists(), "{shell}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn remote_install_puts_its_own_copy_beside_a_zz_it_did_not_install() {
        let home = tempfile::tempdir().expect("temporary home");
        let other = tempfile::tempdir().expect("other prefix");
        let name = zz_protocol::app_identity::DIRECTORY;
        let foreign = other.path().join(name);
        write_executable(&foreign, &fake_zz("1", "foreign-calls"));
        let installer = fake_installer(&zz_protocol::PROTOCOL_VERSION.to_string());

        let path = format!("{}:/usr/bin:/bin", other.path().display());
        run_install_script("/bin/sh", home.path(), &path, &installer, true)
            .expect("a zz elsewhere on PATH gets a managed copy beside it");
        assert_eq!(
            installer_args(home.path()),
            format!("--headless --version {}\n", env!("CARGO_PKG_VERSION")),
        );
        let managed = home.path().join(".local/bin").join(name);
        assert!(fs::symlink_metadata(&managed).unwrap().is_file());
        assert_eq!(
            fs::read_to_string(home.path().join("calls")).unwrap(),
            "--socket /run/zz.sock kill-server\n",
        );
        assert!(!home.path().join("foreign-calls").exists());
        assert_eq!(
            fs::read_to_string(&foreign).unwrap(),
            fake_zz("1", "foreign-calls")
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_install_refreshes_the_desktop_tarball_it_linked() {
        let name = zz_protocol::app_identity::DIRECTORY;
        for shell in install_shells() {
            for absolute in [false, true] {
                let home = tempfile::tempdir().expect("temporary home");
                let lib = home.path().join(".local/lib/zz");
                write_executable(&lib.join("cli"), "#!/bin/sh\necho 1\n");
                write_executable(&lib.join("zz"), "#!/bin/sh\nexit 0\n");
                let bin = home.path().join(".local/bin");
                fs::create_dir_all(&bin).unwrap();
                let link = if absolute {
                    lib.join("cli")
                } else {
                    PathBuf::from("../lib/zz/cli")
                };
                std::os::unix::fs::symlink(&link, bin.join(name)).unwrap();
                let path = format!("{}:/usr/bin:/bin", bin.display());
                let installer = fake_installer(&zz_protocol::PROTOCOL_VERSION.to_string());

                let outcome = run_install_script(shell, home.path(), &path, &installer, true)
                    .unwrap_or_else(|error| panic!("{shell}: {error}"));
                assert_eq!(outcome.installed, RemoteInstalled::Desktop, "{shell}");
                assert_eq!(
                    installer_args(home.path()),
                    format!(
                        "--prefix {}/.local --version {}\n",
                        home.path().display(),
                        env!("CARGO_PKG_VERSION")
                    ),
                    "{shell}",
                );
                assert_eq!(
                    fs::read_link(bin.join(name)).unwrap(),
                    Path::new("../lib/zz/cli"),
                    "{shell}",
                );
                assert_eq!(
                    fs::read_to_string(home.path().join("calls")).unwrap(),
                    "--socket /run/zz.sock kill-server\n",
                    "{shell}",
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn remote_install_refuses_a_link_it_did_not_make_and_names_its_target() {
        let home = tempfile::tempdir().expect("temporary home");
        let other = tempfile::tempdir().expect("other prefix");
        let name = zz_protocol::app_identity::DIRECTORY;
        let foreign = other.path().join(name);
        write_executable(&foreign, &fake_zz("1", "foreign-calls"));
        let bin = home.path().join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(&foreign, bin.join(name)).unwrap();
        let installer = fake_installer(&zz_protocol::PROTOCOL_VERSION.to_string());

        let path = format!("{}:/usr/bin:/bin", bin.display());
        let error = run_install_script("/bin/sh", home.path(), &path, &installer, true)
            .expect_err("a link in ~/.local/bin points at someone else's install");
        let EndpointError::RemoteInstallElsewhere {
            path: found, link, ..
        } = &error
        else {
            panic!("unexpected error: {error}");
        };
        assert_eq!(*found, bin.join(name).display().to_string());
        assert_eq!(*link, foreign.display().to_string());
        let reason = error.ssh_reason().unwrap();
        assert!(
            reason.contains(&format!("links to {}", foreign.display())),
            "{reason}"
        );
        assert!(installer_args(home.path()).is_empty());
        assert!(!home.path().join("calls").exists());
        assert!(!home.path().join("foreign-calls").exists());
    }

    #[cfg(unix)]
    fn current_zz(protocol: u16, kill_server: &str) -> String {
        format!(
            "#!/bin/sh\ncase \"$1\" in protocol-version) echo {protocol} ;; \
             --version) echo 'zz {version}' ;; *) echo \"$*\" >> \"$HOME/calls\"; {kill_server} ;; esac\n",
            version = env!("CARGO_PKG_VERSION"),
        )
    }

    #[cfg(unix)]
    #[test]
    fn remote_install_skips_the_download_when_its_copy_is_current() {
        for shell in install_shells() {
            let home = tempfile::tempdir().expect("temporary home");
            let bin = home.path().join(".local/bin");
            write_executable(
                &bin.join(zz_protocol::app_identity::DIRECTORY),
                &current_zz(zz_protocol::PROTOCOL_VERSION, "exit 0"),
            );
            let path = format!("{}:/usr/bin:/bin", bin.display());
            let installer = fake_installer(&zz_protocol::PROTOCOL_VERSION.to_string());

            let outcome = run_install_script(shell, home.path(), &path, &installer, false)
                .unwrap_or_else(|error| panic!("{shell}: {error}"));
            assert_eq!(outcome.installed, RemoteInstalled::Current, "{shell}");
            assert_eq!(outcome.restart, RemoteRestart::NotAsked, "{shell}");
            assert!(installer_args(home.path()).is_empty(), "{shell}");
            assert!(!home.path().join("calls").exists(), "{shell}");

            let outcome = run_install_script(shell, home.path(), &path, &installer, true)
                .unwrap_or_else(|error| panic!("{shell}: {error}"));
            assert_eq!(outcome.installed, RemoteInstalled::Current, "{shell}");
            assert_eq!(outcome.restart, RemoteRestart::Restarted, "{shell}");
            assert!(installer_args(home.path()).is_empty(), "{shell}");
            assert_eq!(
                outcome.summary(),
                format!(
                    "already {} {}, daemon restarted",
                    zz_protocol::app_identity::DIRECTORY,
                    env!("CARGO_PKG_VERSION")
                ),
            );

            write_executable(
                &bin.join(zz_protocol::app_identity::DIRECTORY),
                &current_zz(zz_protocol::PROTOCOL_VERSION + 1, "exit 0"),
            );
            let outcome = run_install_script(shell, home.path(), &path, &installer, false)
                .unwrap_or_else(|error| panic!("{shell}: {error}"));
            assert_eq!(
                outcome.installed,
                RemoteInstalled::Headless,
                "{shell}: the same version speaking another protocol is replaced"
            );
            assert_eq!(
                installer_args(home.path()),
                format!("--headless --version {}\n", env!("CARGO_PKG_VERSION")),
                "{shell}",
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn remote_install_reports_a_restart_that_failed_and_one_with_nothing_to_stop() {
        let home = tempfile::tempdir().expect("temporary home");
        let bin = home.path().join(".local/bin");
        let binary = bin.join(zz_protocol::app_identity::DIRECTORY);
        let path = format!("{}:/usr/bin:/bin", bin.display());
        for (kill_server, expected) in [
            (
                "echo 'no server running on /run/zz.sock' >&2; exit 1",
                RemoteRestart::NotRunning,
            ),
            (
                "echo 'error connecting to /run/zz.sock (No such file or directory)' >&2; exit 1",
                RemoteRestart::NotRunning,
            ),
            (
                "echo 'error connecting to /run/zz.sock (Permission denied)' >&2; exit 1",
                RemoteRestart::Failed(
                    "error connecting to /run/zz.sock (Permission denied)".to_owned(),
                ),
            ),
            (
                "exit 2",
                RemoteRestart::Failed("kill-server exited with 2".to_owned()),
            ),
            (
                "echo 'zz: daemon I/O error; recovery failed: no daemon pid' >&2; exit 1",
                RemoteRestart::Failed(
                    "daemon I/O error; recovery failed: no daemon pid".to_owned(),
                ),
            ),
            (
                "echo 'stopping the daemon' >&2; echo 'zz: no daemon pid' >&2; exit 1",
                RemoteRestart::Failed("no daemon pid".to_owned()),
            ),
        ] {
            write_executable(
                &binary,
                &current_zz(zz_protocol::PROTOCOL_VERSION, kill_server),
            );
            let outcome = run_install_script("/bin/sh", home.path(), &path, "exit 1\n", true)
                .expect("the binary is in place whatever the daemon did");
            assert_eq!(outcome.restart, expected, "{kill_server}");
        }

        let stdout = format!(
            "zz-install-current=1\nzz-install-restart=2\nzz-install-restart-error=\n\
             zz-install-protocol={}\n",
            zz_protocol::PROTOCOL_VERSION
        );
        let outcome = parse_remote_install_output(
            "ssh://desk",
            Some(0),
            stdout.as_bytes(),
            b"Last login banner from the profile\nwarning: zz is running from ~/.local\n",
            "exit status: 0",
        )
        .expect("installed");
        assert_eq!(
            outcome.restart,
            RemoteRestart::Failed("kill-server exited with 2".to_owned()),
            "other stderr lines in the session are not kill-server's"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_install_passes_on_the_installers_warnings() {
        let home = tempfile::tempdir().expect("temporary home");
        let installer = format!(
            "echo 'warning: zz is running from ~/.local; restart it after the install' >&2\n{}",
            fake_installer(&zz_protocol::PROTOCOL_VERSION.to_string())
        );
        let outcome =
            run_install_script("/bin/sh", home.path(), "/usr/bin:/bin", &installer, false)
                .expect("install");
        assert_eq!(
            outcome.warnings,
            ["zz is running from ~/.local; restart it after the install"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_install_reports_why_it_failed() {
        let home = tempfile::tempdir().expect("temporary home");
        let bin = home.path().join(".local/bin");
        let binary = bin.join(zz_protocol::app_identity::DIRECTORY);
        write_executable(&binary, "#!/bin/sh\necho 1\n");
        let path = format!("{}:/usr/bin:/bin", bin.display());

        let error = run_install_script(
            "/bin/sh",
            home.path(),
            &path,
            "echo 'error: curl is missing' >&2; exit 1\n",
            true,
        )
        .expect_err("the installer failed");
        let EndpointError::RemoteInstallFailed { reason, .. } = error else {
            panic!("unexpected error: {error}");
        };
        assert_eq!(reason, "curl is missing");

        let broken = format!(
            "printf '#!/bin/sh\\necho \"GLIBC_2.39 not found\" >&2; exit 1\\n' > \"{}\"\n",
            binary.display()
        );
        let error = run_install_script("/bin/sh", home.path(), &path, &broken, true)
            .expect_err("the installed binary cannot run");
        let EndpointError::RemoteInstallFailed { reason, .. } = error else {
            panic!("unexpected error: {error}");
        };
        assert!(reason.ends_with("GLIBC_2.39 not found"), "{reason}");

        let error = run_install_script("/bin/sh", home.path(), &path, &fake_installer("1"), true)
            .expect_err("the release speaks another protocol");
        assert!(
            matches!(
                error,
                EndpointError::RemoteInstallMismatch { remote: 1, .. }
            ),
            "{error}"
        );
        assert!(!home.path().join("calls").exists());
    }

    #[cfg(unix)]
    #[test]
    fn authorized_key_script_appends_once_on_its_own_line() {
        let home = tempfile::tempdir().expect("temporary home");
        let file = home.path().join(".ssh/authorized_keys");
        let run = || {
            let script =
                authorized_key_script("ssh-ed25519 AAAAzz", "ssh-ed25519 AAAAzz zz-iphone");
            let status = Command::new("/bin/sh")
                .args(["-c", &format!("sh -c {}", shell_quote(&script))])
                .env("HOME", home.path())
                .status()
                .expect("run key script");
            assert!(status.success());
        };
        run();
        assert_eq!(
            fs::metadata(home.path().join(".ssh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "ssh-ed25519 AAAAzz zz-iphone\n"
        );
        run();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "ssh-ed25519 AAAAzz zz-iphone\n"
        );

        fs::write(&file, "ssh-rsa AAAAother laptop").unwrap();
        run();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "ssh-rsa AAAAother laptop\nssh-ed25519 AAAAzz zz-iphone\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_scripts_fall_back_to_the_mac_app_bundle_cli() {
        let name = zz_protocol::app_identity::DIRECTORY;
        let app = format!("{}.app", zz_protocol::app_identity::DISPLAY_NAME);
        if ["/opt/homebrew/bin", "/usr/local/bin"]
            .iter()
            .any(|dir| Path::new(dir).join(name).exists())
            || Path::new("/Applications").join(&app).exists()
        {
            return;
        }
        let home = tempfile::tempdir().expect("temporary home");
        let macos = home
            .path()
            .join("Applications")
            .join(&app)
            .join("Contents/MacOS");
        fs::create_dir_all(&macos).expect("app bundle");
        let cli = macos.join("cli");
        fs::write(&cli, "#!/bin/sh\nprintf 'bundle %s\\n' \"$1\"\n").expect("write cli");
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755))
            .expect("executable permissions");
        let tools = home.path().join("bin");
        fs::create_dir(&tools).expect("tool directory");
        for tool in ["sh", "uname", "getconf"] {
            if let Some(path) = ["/usr/bin", "/bin"]
                .iter()
                .map(|dir| Path::new(dir).join(tool))
                .find(|path| path.exists())
            {
                std::os::unix::fs::symlink(path, tools.join(tool)).expect("tool link");
            }
        }
        for (script, expected) in [
            (
                remote_socket_probe(None),
                "zz-probe-protocol=bundle protocol-version\n",
            ),
            (
                shell_quote(&remote_proxy_script(Path::new("/tmp/explicit.sock"))),
                "bundle proxy\n",
            ),
        ] {
            let output = Command::new("/bin/sh")
                .args(["-c", &format!("sh -c {script}")])
                .env("PATH", &tools)
                .env("HOME", home.path())
                .output()
                .expect("run remote script");
            assert!(output.status.success());
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.ends_with(expected), "{script}\n{stdout}");
        }
    }

    #[test]
    fn autostart_script_starts_the_daemon_on_the_resolved_socket() {
        let script = remote_daemon_start_script(Path::new("/run/user/1000/zz/default.sock"));
        assert!(
            script.starts_with(&format!(
                "{}command -v \"$zz_cli\" >/dev/null 2>&1 || exit 127;",
                remote_path_fallback(),
            )),
            "missing zz needs its own exit status before anything but the PATH line runs: {script}"
        );
        assert!(
            script.contains(
                "setsid \"$zz_cli\" daemon --socket '/run/user/1000/zz/default.sock' >/dev/null \
                 2>&1 </dev/null &"
            ) && script.contains(
                "nohup \"$zz_cli\" daemon --socket '/run/user/1000/zz/default.sock' >/dev/null \
                 2>&1 </dev/null &"
            ),
            "the daemon must be detached and pinned to the resolved socket by either arm: \
             {script}"
        );
        assert!(
            script.ends_with("exit 3"),
            "a daemon that never listens needs its own exit status: {script}"
        );
    }

    #[test]
    fn ssh_hosts_may_not_open_with_a_dash() {
        for input in [
            "ssh://-oProxyCommand=id",
            "ssh://user@-oProxyCommand=id",
            "ssh://-",
        ] {
            let error = Endpoint::parse(input).expect_err(input);
            assert!(
                error.to_string().contains("must not start with `-`"),
                "{input}: {error}"
            );
        }
    }

    #[test]
    fn autostart_script_starts_the_daemon_without_trusting_an_existing_socket_file() {
        let script = remote_daemon_start_script(Path::new("/run/user/1000/zz/default.sock"));
        let start = script
            .find("\"$zz_cli\" daemon")
            .expect("the script must start the daemon");
        assert!(
            !script[..start].contains("[ -S "),
            "nothing may short-circuit the start on the socket file alone: {script}"
        );
    }

    #[test]
    fn autostart_script_stays_on_one_line_for_csh_logins() {
        for script in [
            remote_daemon_start_script(Path::new("/run/user/1000/zz/default.sock")),
            remote_install_script(Some(Path::new("/run/user/1000/zz/default.sock")), true),
        ] {
            assert!(!script.contains('\n'), "{script}");
        }
    }

    #[test]
    fn autostart_script_quotes_a_remote_socket_path_containing_a_quote() {
        assert_eq!(shell_quote("o'brien"), "'o'\\''brien'");
        let script = remote_daemon_start_script(Path::new("/tmp/zz-o'brien/default.sock"));
        assert!(
            script.contains("[ -S '/tmp/zz-o'\\''brien/default.sock' ]"),
            "{script}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn shell_quoting_survives_the_login_shell_and_the_inner_sh() {
        let path = "/tmp/zz-o'brien $HOME `hostname`/default.sock";
        let script = format!("printf %s {}", shell_quote(path));
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!("sh -c {}", shell_quote(&script)))
            .output()
            .expect("sh should run");
        assert_eq!(String::from_utf8_lossy(&output.stdout), path);
    }

    #[test]
    fn classifies_ssh_stderr_into_actionable_hints() {
        for (stderr, expected) in [
            (
                "user@host: Permission denied (publickey).",
                "rejected the login",
            ),
            ("Host key verification failed.", "known_hosts"),
            (
                "ssh: Could not resolve hostname desk: Name or service not known",
                "did not resolve",
            ),
            (
                "ssh: connect to host desk port 22: Connection refused",
                "Remote Login",
            ),
            (
                "ssh: connect to host desk port 22: Operation timed out",
                "timed out",
            ),
            (
                "ssh: connect to host desk port 22: No route to host",
                "not reachable",
            ),
            (
                "sign_and_send_pubkey: signing failed for ED25519-SK \"/Users/f/.ssh/id_ed25519_sk\": \
                 device not found\nuser@host: Permission denied (publickey).",
                "the security key was not touched",
            ),
            (
                "sign_and_send_pubkey: signing failed for ECDSA-SK \"/k\" from agent: invalid format",
                "the security key was not touched",
            ),
        ] {
            let reason = ssh_failure_reason(stderr, "exit status: 255");
            assert!(
                reason.contains(expected),
                "{stderr:?} should mention {expected:?}, got {reason:?}"
            );
        }
    }

    #[test]
    fn unclassified_ssh_stderr_falls_back_to_its_last_line_then_the_status() {
        assert_eq!(
            ssh_failure_reason(
                "Warning: Permanently added 'desk' to the list of known hosts.\nbanner exchange \
                 failed\n",
                "exit status: 255",
            ),
            "banner exchange failed"
        );
        assert_eq!(
            ssh_failure_reason("  \n", "exit status: 255"),
            "ssh exited with exit status: 255"
        );
    }

    #[test]
    fn ssh_failures_read_as_advice_and_parse_errors_stay_out_of_the_host_row() {
        let reason = |error: EndpointError| error.ssh_reason().expect("ssh reason");
        assert!(
            reason(EndpointError::RemoteBinaryMissing {
                target: "ssh://desk".to_owned(),
            })
            .starts_with(&format!(
                "{} is not installed on ssh://desk",
                zz_protocol::app_identity::DIRECTORY
            ))
        );
        assert!(
            reason(EndpointError::RemoteDaemonUnavailable {
                target: "ssh://desk".to_owned(),
            })
            .contains("never appeared")
        );
        assert_eq!(
            reason(EndpointError::SshFailed {
                target: "ssh://desk".to_owned(),
                reason: "the ssh connection timed out".to_owned(),
            }),
            "Could not reach ssh://desk over ssh: the ssh connection timed out"
        );
        assert!(
            reason(EndpointError::ForwardTimeout {
                target: "ssh://desk".to_owned(),
                local_socket: PathBuf::from("/tmp/zz-fwd-1-1.sock"),
            })
            .contains("forwarded socket"),
        );
        assert!(parse_error("nope://x", "bad").ssh_reason().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn forwarded_socket_path_stays_short() {
        let path = forwarded_local_socket_path().expect("ssh runtime directory");
        assert!(path.as_os_str().as_encoded_bytes().len() < 100, "{path:?}");
        assert_eq!(path.parent(), Some(ssh_runtime_dir().unwrap()));
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("fwd-")
        );
    }

    #[cfg(unix)]
    #[test]
    fn each_host_keeps_one_short_forward_beside_its_master() {
        let endpoint = test_ssh_endpoint(Some("ada"), Some(2222));
        let kept = kept_forward_path(&endpoint).unwrap();
        assert_eq!(kept, kept_forward_path(&endpoint.clone()).unwrap());
        assert_eq!(kept.parent(), Some(ssh_runtime_dir().unwrap()));
        assert!(kept.as_os_str().as_encoded_bytes().len() < 100, "{kept:?}");
        let mut master = control_master_path(&endpoint).unwrap().into_os_string();
        master.push(".sock");
        assert_eq!(kept, PathBuf::from(master));

        let mut elsewhere = endpoint.clone();
        elsewhere.remote_socket = Some(PathBuf::from("/run/user/1000/zz/other.sock"));
        assert_ne!(kept_forward_path(&elsewhere).unwrap(), kept);
        assert_eq!(
            control_master_path(&elsewhere).unwrap(),
            control_master_path(&endpoint).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_step_riding_a_master_gives_up_but_a_login_may_take_its_time() {
        let directory = tempfile::tempdir().unwrap();
        let step = SshStep {
            endpoint: test_ssh_endpoint(None, None),
            control: Some(SshControl {
                path: directory.path().join("c0"),
                user_persist: false,
            }),
            askpass: None,
            identity: None,
        };
        let stalled = |_: &SshStep| {
            thread::sleep(Duration::from_secs(30));
            Ok(())
        };
        let slow = |_: &SshStep| {
            thread::sleep(Duration::from_millis(300));
            Ok(())
        };
        let started = std::time::Instant::now();
        let error = step
            .run_within(
                Duration::from_millis(20),
                true,
                "remote socket probe",
                stalled,
            )
            .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(
            matches!(&error, EndpointError::SshFailed { reason, .. }
                if reason.starts_with("the remote socket probe got no answer")),
            "{error}"
        );
        step.run_within(
            Duration::from_millis(20),
            false,
            "remote socket probe",
            slow,
        )
        .expect("a step without a live master is not time-boxed");
    }

    #[cfg(unix)]
    #[test]
    fn only_a_dead_link_remakes_the_kept_forward() {
        for kind in [
            io::ErrorKind::NotFound,
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::UnexpectedEof,
        ] {
            assert!(
                kept_forward_failed(&DaemonError::Io(kind.into())),
                "{kind:?}"
            );
        }
        assert!(kept_forward_failed(&DaemonError::Server(
            zz_protocol::ServerError::ProtocolMismatch {
                client: zz_protocol::PROTOCOL_VERSION,
                server: 48,
            }
        )));
        assert!(!kept_forward_failed(&DaemonError::Server(
            zz_protocol::ServerError::InvalidCommand("can't find session".to_owned())
        )));
    }

    #[cfg(unix)]
    #[test]
    fn ssh_runtime_dir_is_stable_and_private() {
        use std::os::unix::fs::MetadataExt as _;

        let directory = ssh_runtime_dir().expect("ssh runtime directory");
        assert_eq!(ssh_runtime_dir().unwrap(), directory);
        assert_eq!(
            directory == stable_ssh_runtime_dir(),
            private_directory(&stable_ssh_runtime_dir()).is_ok()
        );
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir());
        assert_eq!(metadata.mode() & 0o777, 0o700);

        let root = tempfile::tempdir().unwrap();
        let fresh = root.path().join("zz/ssh");
        private_directory(&fresh).expect("a fresh directory is made private");
        assert_eq!(fs::metadata(&fresh).unwrap().mode() & 0o777, 0o700);
        private_directory(&fresh).expect("the same directory is accepted again");

        let open = root.path().join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_directory(&open).is_err());

        let link = root.path().join("link");
        std::os::unix::fs::symlink(&fresh, &link).unwrap();
        assert!(private_directory(&link).is_err());

        let file = root.path().join("file");
        fs::write(&file, b"").unwrap();
        assert!(private_directory(&file).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn only_empty_settled_legacy_directories_are_removed() {
        let root = tempfile::tempdir().unwrap();
        let stale = root.path().join("zz-ssh-Ab12Cd");
        let busy = root.path().join("zz-ssh-Ef34Gh");
        let fresh = root.path().join("zz-ssh-Ij56Kl");
        let live = [
            "zz-ssh-501",
            "zz-ssh-100000",
            "zz-dev-ssh-501",
            "zz-ssh-501-Mn78Op",
            "zz-other",
        ]
        .map(|name| root.path().join(name));
        for directory in [&stale, &busy, &fresh].into_iter().chain(&live) {
            fs::create_dir(directory).unwrap();
        }
        fs::write(busy.join("a1"), b"").unwrap();
        let old = std::time::SystemTime::now() - LEGACY_SSH_DIRECTORY_AGE * 2;
        for directory in [&stale, &busy].into_iter().chain(&live) {
            fs::File::open(directory)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        remove_legacy_ssh_dirs(root.path());
        assert!(!stale.exists());
        for directory in [&busy, &fresh].into_iter().chain(&live) {
            assert!(directory.exists(), "{directory:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn user_control_path_is_reused() {
        let config = |master: &str, path: Option<&str>, persist: &str| {
            let path = path.map_or_else(String::new, |path| format!("controlpath {path}\n"));
            format!(
                "user ada\ncontrolmaster {master}\n{path}controlpersist {persist}\nhostname box\n"
            )
        };
        let user_path = "/home/ada/.ssh/cm-f5de03fd";
        for (master, persist, user_persist) in [
            ("auto", "14400", true),
            ("true", "yes", true),
            ("auto", "no", false),
        ] {
            assert_eq!(
                user_control(&config(master, Some(user_path), persist)),
                Some(SshControl {
                    path: PathBuf::from(user_path),
                    user_persist,
                }),
                "{master} {persist}",
            );
        }
        for master in ["false", "ask", "autoask"] {
            assert_eq!(user_control(&config(master, Some(user_path), "600")), None);
        }
        assert_eq!(user_control(&config("auto", None, "600")), None);
        assert_eq!(user_control(&config("auto", Some("none"), "600")), None);
        assert_eq!(
            user_control(&config("auto", Some("/tmp/zz rv/cm-1"), "600")),
            None
        );
        assert_eq!(user_control(""), None);
    }

    #[cfg(unix)]
    const TEST_CONTROL_PATH: &str = "/tmp/zz-ssh/c0";

    #[cfg(unix)]
    static TEST_CONTROL: std::sync::LazyLock<SshControl> =
        std::sync::LazyLock::new(|| SshControl {
            path: PathBuf::from(TEST_CONTROL_PATH),
            user_persist: false,
        });

    #[cfg(unix)]
    fn test_session() -> SshSession<'static> {
        SshSession {
            control: Some(&TEST_CONTROL),
            ..SshSession::default()
        }
    }

    #[cfg(unix)]
    fn shared_ssh_options() -> Vec<String> {
        vec![
            "-o".to_owned(),
            "ConnectTimeout=10".to_owned(),
            "-o".to_owned(),
            "ServerAliveInterval=5".to_owned(),
            "-o".to_owned(),
            "ServerAliveCountMax=3".to_owned(),
            "-o".to_owned(),
            "Compression=yes".to_owned(),
            "-o".to_owned(),
            "ControlMaster=auto".to_owned(),
            "-o".to_owned(),
            format!("ControlPath={TEST_CONTROL_PATH}"),
            "-o".to_owned(),
            format!("ControlPersist={CONTROL_PERSIST_SECONDS}"),
        ]
    }

    fn ssh_option_cases() -> Vec<(Option<&'static str>, Option<u16>, Vec<&'static str>)> {
        vec![
            (None, None, vec![]),
            (Some("user"), None, vec!["-l", "user"]),
            (None, Some(2222), vec!["-p", "2222"]),
            (Some("user"), Some(2222), vec!["-p", "2222", "-l", "user"]),
        ]
    }

    fn test_ssh_endpoint(user: Option<&str>, port: Option<u16>) -> SshEndpoint {
        SshEndpoint {
            user: user.map(str::to_owned),
            host: "host".to_owned(),
            port,
            remote_socket: None,
        }
    }

    fn command_args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[cfg(unix)]
    fn probe_socket_path(environment: &[(&str, Option<&str>)]) -> String {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("sh -c {}", remote_socket_probe(None)));
        for (key, value) in environment {
            if let Some(value) = value {
                command.env(key, value);
            } else {
                command.env_remove(key);
            }
        }
        let output = command.output().expect("sh should run the probe");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("zz-probe-socket="))
            .map(str::to_owned)
            .expect("the probe should print a socket line")
    }

    #[cfg(unix)]
    #[test]
    fn remote_socket_probe_prefers_xdg_runtime_dir_then_tmpdir() {
        for (environment, expected) in [
            (
                [
                    ("XDG_RUNTIME_DIR", Some("/run/user/1000")),
                    ("TMPDIR", Some("/launchd/tmp")),
                    ("USER", Some("ada")),
                ],
                "/run/user/1000/zz/default.sock",
            ),
            (
                [
                    ("XDG_RUNTIME_DIR", None),
                    ("TMPDIR", Some("/launchd/tmp/")),
                    ("USER", Some("ada")),
                ],
                "/launchd/tmp/zz-ada/default.sock",
            ),
            (
                [
                    ("XDG_RUNTIME_DIR", None),
                    ("TMPDIR", Some("relative")),
                    ("USER", Some("ada")),
                ],
                "/tmp/zz-ada/default.sock",
            ),
        ] {
            assert_eq!(
                probe_socket_path(&environment),
                expected.replace("/zz", &format!("/{}", zz_protocol::app_identity::DIRECTORY))
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn remote_socket_probe_resolves_the_launchd_temporary_directory() {
        let expected_root = Command::new("getconf")
            .arg("DARWIN_USER_TEMP_DIR")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|root| root.starts_with('/'))
            .map_or_else(
                || "/tmp".to_owned(),
                |root| root.trim_end_matches('/').to_owned(),
            );
        assert!(
            cfg!(not(target_os = "macos")) || expected_root != "/tmp",
            "{expected_root}"
        );
        assert_eq!(
            probe_socket_path(&[
                ("XDG_RUNTIME_DIR", None),
                ("TMPDIR", None),
                ("USER", Some("ada")),
            ]),
            format!(
                "{expected_root}/{}-ada/default.sock",
                zz_protocol::app_identity::DIRECTORY
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_socket_probe_prefers_existing_macos_desktop_socket() {
        let name = zz_protocol::app_identity::DIRECTORY;
        let desktop = tempfile::Builder::new()
            .prefix(&format!("{name}-"))
            .tempdir_in("/tmp")
            .expect("desktop directory");
        let desktop_name = desktop.path().file_name().unwrap().to_str().unwrap();
        let user = desktop_name.strip_prefix(&format!("{name}-")).unwrap();
        let desktop_socket = desktop.path().join("default.sock");
        let listener =
            std::os::unix::net::UnixListener::bind(&desktop_socket).expect("desktop socket");
        let temporary = tempfile::tempdir_in("/tmp").expect("SSH temporary directory");
        let ssh_directory = temporary.path().join(desktop_name);
        fs::create_dir(&ssh_directory).expect("SSH socket directory");
        let ssh_socket = ssh_directory.join("default.sock");
        let _ssh_listener =
            std::os::unix::net::UnixListener::bind(&ssh_socket).expect("SSH socket");
        let uname = temporary.path().join("uname");
        fs::write(&uname, "#!/bin/sh\nprintf 'Darwin\\n'\n").expect("uname stub");
        fs::set_permissions(&uname, fs::Permissions::from_mode(0o755)).expect("stub permissions");
        let path = format!(
            "{}:{}",
            temporary.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut environment = [
            ("XDG_RUNTIME_DIR", None),
            ("TMPDIR", temporary.path().to_str()),
            ("USER", Some(user)),
            ("PATH", Some(path.as_str())),
        ];
        assert_eq!(
            probe_socket_path(&environment),
            desktop_socket.to_str().unwrap()
        );
        environment[1].1 = None;
        assert_eq!(
            probe_socket_path(&environment),
            desktop_socket.to_str().unwrap()
        );
        environment[1].1 = temporary.path().to_str();
        environment[0].1 = Some("/run/zz-test-runtime");
        assert_eq!(
            probe_socket_path(&environment),
            format!("/run/zz-test-runtime/{name}/default.sock")
        );
        environment[0].1 = None;
        fs::write(&uname, "#!/bin/sh\nprintf 'Linux\\n'\n").expect("Linux uname stub");
        assert_eq!(
            probe_socket_path(&environment),
            ssh_socket.to_str().unwrap()
        );
        fs::write(&uname, "#!/bin/sh\nprintf 'Darwin\\n'\n").expect("Darwin uname stub");
        drop(listener);
        fs::remove_file(desktop_socket).expect("remove desktop socket");
        assert_eq!(
            probe_socket_path(&environment),
            ssh_socket.to_str().unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_socket_probe_falls_back_to_slash_tmp_when_getconf_rejects_the_variable() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let stub = directory.path().join("getconf");
        fs::write(&stub, "#!/bin/sh\necho unsupported >&2\nexit 1\n").expect("getconf stub");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("stub permissions");
        let path = format!(
            "{}:{}",
            directory.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        assert_eq!(
            probe_socket_path(&[
                ("XDG_RUNTIME_DIR", None),
                ("TMPDIR", None),
                ("USER", Some("ada")),
                ("PATH", Some(&path)),
            ]),
            format!(
                "/tmp/{}-ada/default.sock",
                zz_protocol::app_identity::DIRECTORY
            )
        );
    }

    fn command_envs(command: &Command) -> Vec<(String, Option<String>)> {
        command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }
}
