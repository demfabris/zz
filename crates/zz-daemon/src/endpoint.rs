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
use crate::askpass::{AskpassListener, SshPrompts};
#[cfg(windows)]
use crate::transport::{LocalListener, LocalStream, TransportListener as _};
use crate::transport::{LocalTransport, Transport as _, TransportStream as _};

#[cfg(any(unix, windows, test))]
// Runs under `sh -lc` so `zz` resolves through the login shell's PATH; the sentinel prefixes
// let the parser skip whatever a login profile prints around the probe's own output.
pub(crate) const REMOTE_SOCKET_PROBE: &str = "'if [ -n \"$XDG_RUNTIME_DIR\" ]; then zz_dir=\"$XDG_RUNTIME_DIR/zz\"; \
     else zz_tmp=\"$TMPDIR\"; \
     if [ -z \"$zz_tmp\" ]; then zz_tmp=$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null); fi; \
     case \"$zz_tmp\" in /*) ;; *) zz_tmp=/tmp ;; esac; \
     zz_dir=\"${zz_tmp%/}/zz-$USER\"; fi; \
     printf \"zz-probe-socket=%s\\n\" \"$zz_dir/default.sock\"; \
     if command -v zz >/dev/null 2>&1; then printf \"zz-probe-protocol=%s\\n\" \"$(zz protocol-version 2>/dev/null || echo unknown)\"; \
     else printf \"zz-probe-protocol=missing\\n\"; fi'";
/// Exit codes the auto-start script picks for itself; ssh reports its own failures as 255.
#[cfg(any(unix, windows, test))]
pub(crate) const REMOTE_ZZ_MISSING_STATUS: i32 = 127;
#[cfg(any(unix, windows, test))]
pub(crate) const REMOTE_DAEMON_TIMEOUT_STATUS: i32 = 3;
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
const MAX_FORWARD_SOCKET_PATH_BYTES: usize = 100;
#[cfg(unix)]
const CONTROL_PERSIST_SECONDS: u32 = 60;
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
        "ssh remote socket path must be absolute: {path}",
        path = .0.display()
    )]
    InvalidRemoteSocket(PathBuf),
    #[error("ssh endpoints are unsupported on this platform")]
    UnsupportedPlatform,
}

impl EndpointError {
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
            Self::RemoteBinaryMissing { target } => format!(
                "zz is not installed on {target}.\nInstall it there, or put it on the login \
                 shell's PATH."
            ),
            Self::RemoteDaemonUnavailable { target } => format!(
                "Started zz on {target}, but its daemon socket never appeared.\nRun `zz daemon` \
                 there to see why."
            ),
            Self::RemoteProtocolMismatch {
                target,
                daemon,
                client,
            } => format!(
                "zz on {target} speaks protocol v{daemon}; this zz speaks v{client}.\nUpdate zz on \
                 that host, then reconnect."
            ),
            Self::RemoteProtocolUnknown { target } => {
                format!(
                    "zz on {target} predates protocol version reporting; update it, then reconnect"
                )
            }
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
            Self::InvalidRemoteSocket(path) => format!(
                "The remote socket path must be absolute: {path}",
                path = path.display()
            ),
            Self::UnsupportedPlatform => "ssh hosts are not supported on this platform.".to_owned(),
            Self::UriParse { .. } => return None,
        })
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
    control_path: Option<&'a Path>,
    askpass: Option<SshAskpass<'a>>,
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
fn ssh_probe_command(endpoint: &SshEndpoint, session: SshSession<'_>) -> Command {
    let mut command = Command::new("ssh");
    append_ssh_options(&mut command, endpoint, session);
    command
        .arg("--")
        .arg(&endpoint.host)
        .arg("sh")
        .arg("-lc")
        .arg(REMOTE_SOCKET_PROBE);
    command
}

#[cfg(any(unix, windows, test))]
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

#[cfg(any(unix, windows, test))]
pub(crate) fn remote_daemon_start_script(remote_socket: &Path) -> String {
    let socket = shell_quote(&remote_socket.to_string_lossy());
    format!(
        "command -v zz >/dev/null 2>&1 || exit {REMOTE_ZZ_MISSING_STATUS}; \
         if setsid true >/dev/null 2>&1; \
         then setsid zz daemon --socket {socket} >/dev/null 2>&1 </dev/null & \
         else nohup zz daemon --socket {socket} >/dev/null 2>&1 </dev/null & fi; \
         if sleep {REMOTE_DAEMON_POLL_INTERVAL} 2>/dev/null; \
         then delay={REMOTE_DAEMON_POLL_INTERVAL}; attempts={REMOTE_DAEMON_POLL_ATTEMPTS}; \
         else delay=1; attempts={REMOTE_DAEMON_POLL_FALLBACK_ATTEMPTS}; fi; \
         attempt=0; \
         while [ \"$attempt\" -lt \"$attempts\" ]; do \
         if [ -S {socket} ]; then exit 0; fi; \
         sleep \"$delay\"; attempt=$((attempt + 1)); done; \
         exit {REMOTE_DAEMON_TIMEOUT_STATUS}"
    )
}

/// Quote a value so the remote shell sees it verbatim; an ssh command line is parsed twice.
#[cfg(any(unix, windows, test))]
pub(crate) fn shell_quote(value: &str) -> String {
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

/// ssh collapses every failure of its own into exit 255, so stderr is all there is to go on.
#[cfg(any(unix, windows, test))]
fn ssh_failure_hint(stderr: &str) -> Option<&'static str> {
    let stderr = stderr.to_ascii_lowercase();
    let contains = |needle: &str| stderr.contains(needle);
    if contains("permission denied")
        || contains("too many authentication failures")
        || contains("no supported authentication methods")
    {
        Some("ssh rejected the login: add your key to the host or start an ssh agent")
    } else if contains("host key verification failed")
        || contains("remote host identification has changed")
    {
        Some("the host key does not match known_hosts: verify the host before connecting")
    } else if contains("could not resolve hostname")
        || contains("name or service not known")
        || contains("nodename nor servname")
    {
        Some("the host name did not resolve")
    } else if contains("connection refused") {
        Some("the ssh port refused the connection: is sshd running?")
    } else if contains("timed out") {
        Some("the ssh connection timed out")
    } else if contains("no route to host") || contains("network is unreachable") {
        Some("the host is not reachable from this network")
    } else {
        None
    }
}

#[cfg(any(unix, windows, test))]
fn ssh_failure_reason(stderr: &str, status: &str) -> String {
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
    command.arg("-o").arg("ConnectTimeout=10");
    // Windows OpenSSH has no connection sharing, and any set `ControlPath` still routes children
    // into its broken mux stub, so both options are overridden rather than omitted.
    #[cfg(windows)]
    command
        .arg("-o")
        .arg("ControlMaster=no")
        .arg("-o")
        .arg("ControlPath=none");
    #[cfg(not(windows))]
    if let Some(control_path) = session.control_path {
        command
            .arg("-o")
            .arg("ControlMaster=auto")
            .arg("-o")
            .arg(control_path_option(control_path))
            .arg("-o")
            .arg(format!("ControlPersist={CONTROL_PERSIST_SECONDS}"));
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

/// A 0700 per-process directory for the ssh control and askpass sockets, both of which are
/// capabilities: one owns an ssh session, the other can phish a password out of the dialog.
#[cfg(unix)]
pub(crate) fn ssh_runtime_dir() -> io::Result<&'static Path> {
    static DIRECTORY: OnceLock<Result<tempfile::TempDir, String>> = OnceLock::new();
    match DIRECTORY.get_or_init(|| {
        let directory = tempfile::Builder::new()
            .prefix("zz-ssh-")
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
        Ok(directory)
    }) {
        Ok(directory) => Ok(directory.path()),
        Err(error) => Err(io::Error::other(error.clone())),
    }
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
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    endpoint.user.hash(&mut hasher);
    endpoint.host.hash(&mut hasher);
    endpoint.port.hash(&mut hasher);
    Ok(ssh_runtime_dir()?.join(format!("c{:016x}", hasher.finish())))
}

#[cfg(unix)]
fn discard_dead_control_master(endpoint: &SshEndpoint, control_path: &Path) {
    if !control_path.exists() {
        return;
    }
    let alive = ssh_control_command(endpoint, control_path, "check", None)
        .status()
        .is_ok_and(|status| status.success());
    if !alive {
        let _ = fs::remove_file(control_path);
    }
}

#[cfg(unix)]
fn ssh_control_command(
    endpoint: &SshEndpoint,
    control_path: &Path,
    request: &str,
    forwarding: Option<(&str, &OsStr)>,
) -> Command {
    let mut command = Command::new("ssh");
    command
        .arg("-O")
        .arg(request)
        .arg("-o")
        .arg(control_path_option(control_path));
    if let Some((flag, spec)) = forwarding {
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

#[cfg(any(unix, windows))]
fn probe_remote_socket(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
) -> Result<PathBuf, EndpointError> {
    let target = endpoint.to_string();
    let output = ssh_probe_command(endpoint, session)
        .output()
        .map_err(|source| EndpointError::SshSpawn {
            operation: "remote socket probe",
            source,
        })?;
    if !output.status.success() {
        return Err(EndpointError::SshFailed {
            target,
            reason: ssh_failure_reason(
                &String::from_utf8_lossy(&output.stderr),
                &output.status.to_string(),
            ),
        });
    }

    parse_remote_probe_output(&target, &output.stdout)
}

pub(crate) fn parse_remote_probe_output(
    target: &str,
    output: &[u8],
) -> Result<PathBuf, EndpointError> {
    let stdout = std::str::from_utf8(output).map_err(|_| EndpointError::ProbeFailure {
        target: target.to_owned(),
        reason: "probe output was not valid UTF-8".to_owned(),
    })?;
    // Login profiles may print arbitrary noise around the probe's output; only the
    // sentinel-prefixed lines are ours.
    let probe_value = |prefix: &str| {
        stdout
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .map(str::trim)
    };
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

#[cfg(any(unix, windows))]
fn ensure_remote_daemon(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    remote_socket: &Path,
) -> Result<(), EndpointError> {
    let target = endpoint.to_string();
    let output = ssh_daemon_start_command(endpoint, session, remote_socket)
        .output()
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

#[cfg(unix)]
fn forwarded_local_socket_path() -> PathBuf {
    let counter = FORWARD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = format!("zz-fwd-{}-{counter}.sock", std::process::id());
    let temporary_directory = std::env::var("TMPDIR")
        .ok()
        .filter(|path| !path.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    let candidate = temporary_directory.join(&file_name);
    if candidate.as_os_str().as_encoded_bytes().len() < MAX_FORWARD_SOCKET_PATH_BYTES {
        candidate
    } else {
        PathBuf::from("/tmp").join(file_name)
    }
}

#[cfg(unix)]
pub(crate) struct SshForward {
    child: Child,
    local_socket: PathBuf,
    endpoint: SshEndpoint,
    control_path: Option<PathBuf>,
    forwarding: OsString,
    socks_port: Option<u16>,
    askpass: Option<AskpassListener>,
}

#[cfg(unix)]
impl SshForward {
    pub(crate) fn start(
        endpoint: &SshEndpoint,
        prompts: Option<SshPrompts>,
    ) -> Result<Self, EndpointError> {
        let askpass = prompts.and_then(|prompts| match AskpassListener::start(prompts) {
            Ok(listener) => Some(listener),
            Err(error) => {
                log::warn!(
                    target: "zz_daemon::askpass",
                    "ssh prompts are unavailable for {endpoint}: {error}",
                );
                None
            }
        });
        let control_path = match control_master_path(endpoint) {
            Ok(path) => {
                discard_dead_control_master(endpoint, &path);
                Some(path)
            }
            Err(error) => {
                log::warn!(
                    target: "zz_daemon::diagnostics",
                    "ssh connection sharing is unavailable for {endpoint}: {error}",
                );
                None
            }
        };
        let session = SshSession {
            control_path: control_path.as_deref(),
            askpass: askpass.as_ref().map(|listener| SshAskpass {
                helper: listener.helper(),
                socket: listener.socket(),
            }),
        };

        let probed_socket = probe_remote_socket(endpoint, session)?;
        let remote_socket = endpoint.remote_socket.clone().unwrap_or(probed_socket);
        if !remote_socket.is_absolute() {
            return Err(EndpointError::InvalidRemoteSocket(remote_socket));
        }
        ensure_remote_daemon(endpoint, session, &remote_socket)?;

        let local_socket = forwarded_local_socket_path();
        let socks_port = available_socks_port();
        let child =
            ssh_forward_command(endpoint, session, &local_socket, &remote_socket, socks_port)
                .spawn()
                .map_err(|source| EndpointError::SshSpawn {
                    operation: "socket forward",
                    source,
                })?;
        let mut forward = Self {
            child,
            forwarding: forwarding_spec(&local_socket, &remote_socket),
            local_socket,
            endpoint: endpoint.clone(),
            control_path,
            socks_port,
            askpass,
        };
        if let Err(error) = forward.wait_for_socket(endpoint) {
            let Some(port) = forward.socks_port else {
                return Err(error);
            };
            log::debug!(
                target: "zz_daemon::diagnostics",
                "ssh forward to {endpoint} failed while binding -D {}; retrying on a fresh port: {error}",
                socks_spec(port),
            );
            forward.restart_with_fresh_socks_port(endpoint, &remote_socket)?;
        }

        let mut permissions = fs::metadata(&forward.local_socket)
            .map_err(|source| forward.io_error(endpoint, "reading permissions for", source))?
            .permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&forward.local_socket, permissions)
            .map_err(|source| forward.io_error(endpoint, "setting permissions on", source))?;
        Ok(forward)
    }

    pub(crate) fn local_socket(&self) -> &Path {
        &self.local_socket
    }

    pub(crate) const fn socks_port(&self) -> Option<u16> {
        self.socks_port
    }

    fn restart_with_fresh_socks_port(
        &mut self,
        endpoint: &SshEndpoint,
        remote_socket: &Path,
    ) -> Result<(), EndpointError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.cancel_forwards();
        let _ = fs::remove_file(&self.local_socket);
        self.socks_port = available_socks_port();
        let mut command = ssh_forward_command(
            endpoint,
            SshSession {
                control_path: self.control_path.as_deref(),
                askpass: self.askpass.as_ref().map(|listener| SshAskpass {
                    helper: listener.helper(),
                    socket: listener.socket(),
                }),
            },
            &self.local_socket,
            remote_socket,
            self.socks_port,
        );
        self.child = command.spawn().map_err(|source| EndpointError::SshSpawn {
            operation: "socket forward",
            source,
        })?;
        self.wait_for_socket(endpoint)
    }

    fn cancel_forwards(&self) {
        let Some(control_path) = &self.control_path else {
            return;
        };
        if !control_path.exists() {
            return;
        }
        let cancel = |forwarding| {
            let _ = ssh_control_command(&self.endpoint, control_path, "cancel", Some(forwarding))
                .status();
        };
        cancel(("-L", self.forwarding.as_os_str()));
        if let Some(port) = self.socks_port {
            let spec = socks_spec(port);
            cancel(("-D", OsStr::new(&spec)));
        }
    }

    fn wait_for_socket(&mut self, endpoint: &SshEndpoint) -> Result<(), EndpointError> {
        for _ in 0..FORWARD_POLL_ATTEMPTS {
            if self.socket_exists(endpoint)? {
                return Ok(());
            }
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|source| self.io_error(endpoint, "checking ssh child for", source))?
            {
                if self.socket_exists(endpoint)? {
                    return Ok(());
                }
                return Err(EndpointError::ForwardExited {
                    target: endpoint.to_string(),
                    status: status.to_string(),
                    local_socket: self.local_socket.clone(),
                });
            }
            thread::sleep(FORWARD_POLL_INTERVAL);
        }
        if self.socket_exists(endpoint)? {
            return Ok(());
        }
        Err(EndpointError::ForwardTimeout {
            target: endpoint.to_string(),
            local_socket: self.local_socket.clone(),
        })
    }

    fn socket_exists(&self, endpoint: &SshEndpoint) -> Result<bool, EndpointError> {
        match fs::metadata(&self.local_socket) {
            Ok(metadata) => Ok(metadata.file_type().is_socket()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(self.io_error(endpoint, "inspecting", source)),
        }
    }

    fn io_error(
        &self,
        endpoint: &SshEndpoint,
        action: &'static str,
        source: io::Error,
    ) -> EndpointError {
        EndpointError::ForwardIo {
            target: endpoint.to_string(),
            action,
            path: self.local_socket.clone(),
            source,
        }
    }
}

#[cfg(unix)]
impl Drop for SshForward {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.cancel_forwards();
        let _ = fs::remove_file(&self.local_socket);
    }
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
#[cfg(any(windows, target_os = "ios"))]
pub(crate) fn remote_socket_is_absolute(path: &Path) -> bool {
    path.as_os_str().as_encoded_bytes().starts_with(b"/")
}

#[cfg(windows)]
fn ssh_proxy_command(
    endpoint: &SshEndpoint,
    session: SshSession<'_>,
    remote_socket: &Path,
) -> Command {
    let socket = shell_quote(&remote_socket.to_string_lossy());
    let mut command = Command::new("ssh");
    append_ssh_options(&mut command, endpoint, session);
    command
        .arg("--")
        .arg(&endpoint.host)
        .arg("sh")
        .arg("-lc")
        .arg(shell_quote(&format!("exec zz proxy --socket {socket}")))
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
        let askpass = prompts.and_then(|prompts| match AskpassListener::start(prompts) {
            Ok(listener) => Some(listener),
            Err(error) => {
                log::warn!(
                    target: "zz_daemon::askpass",
                    "ssh prompts are unavailable for {endpoint}: {error}",
                );
                None
            }
        });
        let session = SshSession {
            askpass: askpass.as_ref().map(|listener| SshAskpass {
                helper: listener.helper(),
                socket: listener.socket(),
            }),
        };

        let probed_socket = probe_remote_socket(endpoint, session)?;
        let remote_socket = endpoint.remote_socket.clone().unwrap_or(probed_socket);
        if !remote_socket_is_absolute(&remote_socket) {
            return Err(EndpointError::InvalidRemoteSocket(remote_socket));
        }
        ensure_remote_daemon(endpoint, session, &remote_socket)?;

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

        let mut child = ssh_proxy_command(endpoint, session, &remote_socket)
            .spawn()
            .map_err(|source| EndpointError::SshSpawn {
                operation: "socket proxy",
                source,
            })?;
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
        let stopped = Arc::new(AtomicBool::new(false));
        let started = await_proxy_ready(
            endpoint,
            &mut child,
            &mut stdout,
            &mut stderr,
            &local_socket,
        )
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
        Ok(Self {
            child,
            local_socket,
            stopped,
            _askpass: askpass,
        })
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
fn await_proxy_ready(
    endpoint: &SshEndpoint,
    child: &mut Child,
    stdout: &mut BufReader<ChildStdout>,
    stderr: &mut SshStderr,
    local_socket: &Path,
) -> Result<(), EndpointError> {
    let mut discarded = 0_usize;
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
            return Err(proxy_never_started(endpoint, child, stderr));
        }
        if line.ends_with(PROXY_READY_MARKER) {
            return Ok(());
        }
        log::debug!(
            target: "zz_daemon::diagnostics",
            "remote login preamble: {}",
            String::from_utf8_lossy(&line).trim_end(),
        );
        discarded = discarded.saturating_add(read);
        if discarded > MAX_PROXY_PREAMBLE_BYTES {
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
fn proxy_never_started(
    endpoint: &SshEndpoint,
    child: &mut Child,
    stderr: &mut SshStderr,
) -> EndpointError {
    let target = endpoint.to_string();
    let status = child.wait().ok();
    match status.and_then(|status| status.code()) {
        // `exec zz` in a POSIX shell exits 127 when there is no `zz` to exec.
        Some(REMOTE_ZZ_MISSING_STATUS) => EndpointError::RemoteBinaryMissing { target },
        _ => EndpointError::SshFailed {
            target,
            reason: ssh_failure_reason(
                &stderr.settled(),
                &status.map_or_else(|| "no exit status".to_owned(), |status| status.to_string()),
            ),
        },
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
            let command = ssh_control_command(
                &endpoint,
                control_path,
                "cancel",
                Some((flag, OsStr::new(spec))),
            );
            let arguments = command_args(&command);
            assert!(
                arguments
                    .windows(2)
                    .any(|pair| pair == [flag.to_owned(), spec.to_owned()]),
                "{arguments:?}",
            );
        }
        let check = command_args(&ssh_control_command(&endpoint, control_path, "check", None));
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
            let command = ssh_probe_command(&endpoint, test_session());
            let mut expected = shared_ssh_options();
            expected.extend(options.into_iter().map(str::to_owned));
            expected.extend(
                ["--", "host", "sh", "-lc", REMOTE_SOCKET_PROBE]
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
    fn every_ssh_child_shares_one_multiplexing_master() {
        let endpoint = test_ssh_endpoint(Some("user"), Some(2222));
        let commands = [
            command_args(&ssh_probe_command(&endpoint, test_session())),
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
                arguments.contains(&format!("ControlPersist={CONTROL_PERSIST_SECONDS}")),
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
        let command = ssh_probe_command(&endpoint, SshSession::default());
        let mut expected = ["-o", "ConnectTimeout=10"].map(str::to_owned).to_vec();
        #[cfg(windows)]
        expected.extend(["-o", "ControlMaster=no", "-o", "ControlPath=none"].map(str::to_owned));
        expected.extend(["--", "host", "sh", "-lc", REMOTE_SOCKET_PROBE].map(str::to_owned));
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
            parse_remote_probe_output("host", output.as_bytes()).unwrap(),
            Path::new("/run/user/1000/zz/default.sock")
        );

        assert!(matches!(
            parse_remote_probe_output(
                "host",
                b"zz-probe-socket=/run/zz.sock\nzz-probe-protocol=48\n"
            ),
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
            .unwrap_err()
            .to_string(),
            "zz on host predates protocol version reporting; update it, then reconnect"
        );
        assert!(matches!(
            parse_remote_probe_output(
                "host",
                b"zz-probe-socket=/run/zz.sock\nzz-probe-protocol=banana\n"
            ),
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
            parse_remote_probe_output("host", noisy.as_bytes()).unwrap(),
            Path::new("/run/zz.sock")
        );

        // No zz on the login-shell PATH must not block dialing a running daemon.
        assert_eq!(
            parse_remote_probe_output(
                "host",
                b"zz-probe-socket=/run/zz.sock\nzz-probe-protocol=missing\n"
            )
            .unwrap(),
            Path::new("/run/zz.sock")
        );

        assert!(matches!(
            parse_remote_probe_output("host", b"zz-probe-socket=/run/zz.sock\n"),
            Err(EndpointError::ProbeFailure { .. })
        ));
        assert!(matches!(
            parse_remote_probe_output("host", b"only noise, no sentinels\n"),
            Err(EndpointError::ProbeFailure { .. })
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
            control_path: None,
            askpass: Some(SshAskpass {
                helper: &helper,
                socket: &socket,
            }),
        };
        for command in [
            ssh_probe_command(&endpoint, session),
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

    #[test]
    fn autostart_script_starts_the_daemon_on_the_resolved_socket() {
        let script = remote_daemon_start_script(Path::new("/run/user/1000/zz/default.sock"));
        assert!(
            script.starts_with("command -v zz >/dev/null 2>&1 || exit 127;"),
            "missing zz needs its own exit status before anything else runs: {script}"
        );
        assert!(
            script.contains(
                "setsid zz daemon --socket '/run/user/1000/zz/default.sock' >/dev/null 2>&1 \
                 </dev/null &"
            ) && script.contains(
                "nohup zz daemon --socket '/run/user/1000/zz/default.sock' >/dev/null 2>&1 \
                 </dev/null &"
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
            .find("zz daemon")
            .expect("the script must start the daemon");
        assert!(
            !script[..start].contains("[ -S "),
            "nothing may short-circuit the start on the socket file alone: {script}"
        );
    }

    #[test]
    fn autostart_script_stays_on_one_line_for_csh_logins() {
        let script = remote_daemon_start_script(Path::new("/run/user/1000/zz/default.sock"));
        assert!(!script.contains('\n'), "{script}");
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
                "is sshd running?",
            ),
            (
                "ssh: connect to host desk port 22: Operation timed out",
                "timed out",
            ),
            (
                "ssh: connect to host desk port 22: No route to host",
                "not reachable",
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
            .starts_with("zz is not installed on ssh://desk")
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
        let path = forwarded_local_socket_path();
        assert!(path.as_os_str().as_encoded_bytes().len() < 100);
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("zz-fwd-")
        );
    }

    #[cfg(unix)]
    const TEST_CONTROL_PATH: &str = "/tmp/zz-ssh/c0";

    #[cfg(unix)]
    fn test_session() -> SshSession<'static> {
        SshSession {
            control_path: Some(Path::new(TEST_CONTROL_PATH)),
            askpass: None,
        }
    }

    #[cfg(unix)]
    fn shared_ssh_options() -> Vec<String> {
        vec![
            "-o".to_owned(),
            "ConnectTimeout=10".to_owned(),
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
            .arg(format!("sh -c {REMOTE_SOCKET_PROBE}"));
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
            assert_eq!(probe_socket_path(&environment), expected);
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
            format!("{expected_root}/zz-ada/default.sock")
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
            "/tmp/zz-ada/default.sock"
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
