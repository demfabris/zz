//! Persistent mux daemon, local IPC transport, and command client.

#[cfg(feature = "daemon")]
use std::process::Command;
use std::{
    io,
    path::{Path, PathBuf},
    time::Instant,
};

use thiserror::Error;
use zz_protocol::{ProtocolError, ServerError};

const STARTUP_REENTRY_CAPABILITY_PREFIX: &str = "zz-startup-reentry=";
const STARTUP_REENTRY_ENVIRONMENT_VARIABLE: &str = "ZZ_STARTUP_REENTRY";
const TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE: &str = "ZZ_TMUX_EXECUTABLE";

// iOS uses the in-process russh tunnel, leaving the spawned-ssh and askpass halves unreachable.
#[cfg(feature = "agent")]
mod agent;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod askpass;
mod client;
#[cfg(feature = "daemon")]
mod daemon;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod endpoint;
mod fleet_hosts;
#[cfg(target_os = "ios")]
mod ios_keychain;
#[cfg(feature = "daemon")]
mod keys;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod lifecycle;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod paths;
#[cfg(target_os = "ios")]
mod russh_client;
#[cfg(feature = "daemon")]
mod status;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod transport;
pub mod user_data;

#[cfg(feature = "agent")]
pub use agent::stream::{
    AgentAuthMethod, AgentImage as AgentStreamImage, AgentPrompt, AgentPromptOutcome,
    AgentSessionCapabilities, AgentSessionSummary, AgentStreamItem, AgentStreamPayload,
};
/// Only unix and Windows spawn ssh, so only they carry the askpass helper.
#[cfg(any(unix, windows))]
pub use askpass::run_helper;
pub use askpass::{ASKPASS_SOCKET_ENV, AskpassPrompt, AskpassPromptKind, AskpassReply, SshPrompts};
pub use client::{CommandClient, CommandOutcome, InteractiveClient, short_device_name};
#[cfg(feature = "daemon")]
pub use daemon::{Daemon, agent_send_reads_stdin};
pub use endpoint::{Endpoint, EndpointError, SshEndpoint, run_socket_proxy};
pub use fleet_hosts::{
    HostEntry, RejectedHost, apply_fleet_host_entry, configured_fleet_hosts, validate_fleet_host,
    write_fleet_host,
};
pub use lifecycle::{
    DaemonRecoveryError, RecoveredDaemon, daemon_identity_protocol_version,
    terminate_incompatible_daemon,
};
pub use paths::{default_mux_config, mux_config_candidates, mux_config_write_path};
pub use transport::default_socket_path;

/// Every failure a client can see from the daemon, whether it hosts one or only talks to one.
#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("daemon I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("daemon protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("mux command failed: {0}")]
    Server(#[from] ServerError),
    #[error("another zz daemon is already listening at {0}")]
    AlreadyRunning(PathBuf),
    #[error("failed to start daemon thread: {0}")]
    Thread(String),
    #[error("command exited with status {exit_code}")]
    CommandExit { output: String, exit_code: u8 },
    #[error("{error}")]
    CommandFailed {
        output: String,
        #[source]
        error: Box<DaemonError>,
    },
    #[error("{}", incompatible_daemon_message(*daemon, *client))]
    IncompatibleDaemon { daemon: Option<u16>, client: u16 },
}

fn incompatible_daemon_message(daemon: Option<u16>, client: u16) -> String {
    match daemon {
        Some(daemon) => {
            format!("the running zz daemon speaks protocol v{daemon}; this zz speaks v{client}")
        }
        None => format!("the running zz daemon is older than this zz (protocol v{client})"),
    }
}

#[cfg(feature = "daemon")]
fn configure_tmux_shim(
    process: &mut Command,
    tmux_shim: Option<&Path>,
    zz_executable: Option<&Path>,
) {
    let (Some(tmux_shim), Some(zz_executable)) = (tmux_shim, zz_executable) else {
        return;
    };
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(tmux_shim.to_path_buf()).chain(std::env::split_paths(&inherited));
    if let Ok(path) = std::env::join_paths(paths) {
        process.env("PATH", path);
    }
    process.env(TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE, zz_executable);
}

/// Classify a failed local handshake as a stale daemon when the wire error or guarded identity
/// provides enough evidence. Unrelated protocol failures are returned unchanged.
#[must_use]
pub fn classify_local_connect_error(socket_path: &Path, error: DaemonError) -> DaemonError {
    let mut source: &(dyn std::error::Error + 'static) = &error;
    loop {
        if let Some(ProtocolError::VersionMismatch { received, .. }) =
            source.downcast_ref::<ProtocolError>()
        {
            return DaemonError::IncompatibleDaemon {
                daemon: Some(*received),
                client: zz_protocol::PROTOCOL_VERSION,
            };
        }
        if let Some(ServerError::ProtocolMismatch { server, .. }) =
            source.downcast_ref::<ServerError>()
        {
            return DaemonError::IncompatibleDaemon {
                daemon: Some(*server),
                client: zz_protocol::PROTOCOL_VERSION,
            };
        }
        if let Some(error) = source.downcast_ref::<io::Error>()
            && let Some(inner) = error.get_ref()
        {
            source = inner;
            continue;
        }
        let Some(next) = source.source() else {
            break;
        };
        source = next;
    }

    let eof_during_handshake = matches!(
        &error,
        DaemonError::Protocol(ProtocolError::Io(error)) | DaemonError::Io(error)
            if matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
            )
    );
    if eof_during_handshake {
        match daemon_identity_protocol_version(socket_path) {
            Some(None) => {
                return DaemonError::IncompatibleDaemon {
                    daemon: None,
                    client: zz_protocol::PROTOCOL_VERSION,
                };
            }
            Some(Some(daemon)) if daemon != zz_protocol::PROTOCOL_VERSION => {
                return DaemonError::IncompatibleDaemon {
                    daemon: Some(daemon),
                    client: zz_protocol::PROTOCOL_VERSION,
                };
            }
            Some(Some(_)) => {
                return DaemonError::Io(connect_errno(
                    io::ErrorKind::ConnectionReset,
                    "daemon closed the connection during the handshake",
                ));
            }
            None if !socket_path.exists() => {
                return DaemonError::Io(connect_errno(
                    io::ErrorKind::NotFound,
                    "daemon released the socket during the handshake",
                ));
            }
            None => {}
        }
    }
    error
}

#[cfg(unix)]
fn connect_errno(kind: io::ErrorKind, _reason: &'static str) -> io::Error {
    let errno = match kind {
        io::ErrorKind::NotFound => libc::ENOENT,
        _ => libc::ECONNRESET,
    };
    io::Error::from_raw_os_error(errno)
}

#[cfg(not(unix))]
fn connect_errno(kind: io::ErrorKind, reason: &'static str) -> io::Error {
    io::Error::new(kind, reason)
}

fn diagnostic_timer() -> Option<Instant> {
    log::log_enabled!(target: "zz_daemon::diagnostics", log::Level::Trace).then(Instant::now)
}

fn diagnostic_elapsed_us(started: Option<Instant>) -> u128 {
    started.map_or(0, |started| started.elapsed().as_micros())
}

#[cfg(all(feature = "daemon", not(windows)))]
fn shell_process(command: &str) -> Command {
    let mut process = Command::new("/bin/sh");
    process.arg("-c").arg(command);
    process
}

#[cfg(all(feature = "daemon", windows))]
fn shell_process(command: &str) -> Command {
    let mut process = Command::new("cmd");
    process.args(["/D", "/S", "/C"]).arg(command);
    process
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_finds_protocol_versions_through_error_sources() {
        let nested = DaemonError::Io(io::Error::other(ProtocolError::VersionMismatch {
            expected: zz_protocol::PROTOCOL_VERSION,
            received: 7,
        }));
        assert!(matches!(
            classify_local_connect_error(Path::new("ignored"), nested),
            DaemonError::IncompatibleDaemon {
                daemon: Some(7),
                client: zz_protocol::PROTOCOL_VERSION,
            }
        ));

        let server = DaemonError::Server(ServerError::ProtocolMismatch {
            client: zz_protocol::PROTOCOL_VERSION,
            server: 8,
        });
        assert!(matches!(
            classify_local_connect_error(Path::new("ignored"), server),
            DaemonError::IncompatibleDaemon {
                daemon: Some(8),
                client: zz_protocol::PROTOCOL_VERSION,
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn eof_classifier_uses_identity_and_treats_a_matching_v2_as_dying() {
        use std::{fs, os::unix::fs::PermissionsExt as _};

        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("daemon.sock");
        fs::write(&socket, b"").unwrap();
        let mut identity = socket.as_os_str().to_owned();
        identity.push(".identity");
        let identity = PathBuf::from(identity);

        let eof = || {
            DaemonError::Protocol(ProtocolError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "closed during handshake",
            )))
        };
        fs::write(
            &identity,
            b"zz-daemon-identity-v1\npid=42\nstart_time=100\n",
        )
        .unwrap();
        fs::set_permissions(&identity, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            classify_local_connect_error(&socket, eof()),
            DaemonError::IncompatibleDaemon { daemon: None, .. }
        ));

        let stale = zz_protocol::PROTOCOL_VERSION.saturating_sub(1);
        fs::write(
            &identity,
            format!("zz-daemon-identity-v2\npid=42\nstart_time=100\nprotocol_version={stale}\n"),
        )
        .unwrap();
        assert!(matches!(
            classify_local_connect_error(&socket, eof()),
            DaemonError::IncompatibleDaemon {
                daemon: Some(version),
                ..
            } if version == stale
        ));

        fs::write(
            &identity,
            format!(
                "zz-daemon-identity-v2\npid=42\nstart_time=100\nprotocol_version={}\n",
                zz_protocol::PROTOCOL_VERSION
            ),
        )
        .unwrap();
        assert!(matches!(
            classify_local_connect_error(&socket, eof()),
            DaemonError::Io(error) if error.kind() == io::ErrorKind::ConnectionReset
        ));
    }
}
