//! Client half of the zz daemon: endpoints, ssh tunnels, the command and interactive clients, and the transport, paths, and process facts the server shares.
#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests spawn helper processes from threads with an empty signal mask"
    )
)]

use std::{
    io,
    path::{Path, PathBuf},
    time::Instant,
};

use thiserror::Error;
use zz_protocol::{ProtocolError, RawText, ServerError};

pub const STARTUP_REENTRY_CAPABILITY_PREFIX: &str = "zz-startup-reentry=";
pub const CLIENT_EXITS_ON_DETACH_CAPABILITY: &str = "client-exits-on-detach-v1";
pub const STARTUP_REENTRY_ENVIRONMENT_VARIABLE: &str = "ZZ_STARTUP_REENTRY";
pub const COLD_START_PREPARE_ABORT_COMMAND: &str = "__zz-cold-start-prepare-abort";

// iOS uses the in-process russh tunnel, leaving the spawned-ssh and askpass halves unreachable.
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod askpass;
mod client;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod endpoint;
mod fleet_hosts;
#[cfg(target_os = "ios")]
mod ios_keychain;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod lifecycle;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod paths;
pub mod process_info;
#[cfg(target_os = "ios")]
mod russh_client;
#[cfg(any(target_os = "ios", test))]
mod russh_prompt;
#[cfg(any(target_os = "ios", test))]
mod russh_socks;
#[cfg(test)]
mod solo_tests;
#[cfg_attr(target_os = "ios", allow(dead_code))]
mod terminal_features;
pub mod transport;
pub mod unmasked;
pub mod user_data;

/// Only unix and Windows spawn ssh, so only they carry the askpass helper.
#[cfg(any(unix, windows))]
pub use askpass::run_helper;
pub use askpass::{ASKPASS_SOCKET_ENV, AskpassPrompt, AskpassPromptKind, AskpassReply, SshPrompts};
pub use client::{
    ClientTerminalFlags, DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX, TracedMessage,
    adopt_negotiated_terminal_features, cell_pixel_extent, client_takes_utf8_terminal,
    client_terminal_colour_count, client_terminal_feature_mask, learn_client_terminal_features,
    report_terminal_type, set_client_terminal_flags,
};
pub use client::{
    CommandClient, CommandOutcome, ExecChain, ExecChainEnd, ExecClassifier, InteractiveClient,
    short_device_name,
};
pub use endpoint::{Endpoint, EndpointError, SshEndpoint, run_socket_proxy, shell_quote};
pub use fleet_hosts::{
    HostEntry, RejectedHost, apply_fleet_host_entry, atomic_write, configured_fleet_hosts,
    validate_fleet_host, write_fleet_host,
};
pub use lifecycle::{
    DaemonIdentityGuard, DaemonRecoveryError, RecoveredDaemon, daemon_identity_protocol_version,
    terminate_incompatible_daemon,
};
pub use paths::{
    default_mux_config, discover_tmux_config, home_directory, mux_config_candidates,
    mux_config_write_path,
};
#[cfg(target_os = "ios")]
pub use russh_client::ios_ssh_public_key;
pub use terminal_features::{
    TERMINAL_FEATURES, terminal_colour_count, terminal_default_features, terminal_feature_mask,
    terminal_features_list,
};
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
    CommandExit { output: RawText, exit_code: u8 },
    #[error("command reported status {exit_code}")]
    ReportedCommandExit { output: RawText, exit_code: u8 },
    #[error("{0}")]
    InsertedCommandParse(String),
    #[error("{error}")]
    CommandFailed {
        output: RawText,
        #[source]
        error: Box<DaemonError>,
    },
    #[error("{}", incompatible_daemon_message(*daemon, *client))]
    IncompatibleDaemon { daemon: Option<u16>, client: u16 },
}

fn incompatible_daemon_message(daemon: Option<u16>, client: u16) -> String {
    match daemon {
        Some(daemon) if daemon == client => format!(
            "the running zz daemon is an older build of protocol v{daemon} whose terminal frames this zz cannot read"
        ),
        Some(daemon) => {
            format!("the running zz daemon speaks protocol v{daemon}; this zz speaks v{client}")
        }
        None => format!("the running zz daemon is older than this zz (protocol v{client})"),
    }
}

/// The text `strerror` would give for an IO failure. Rust appends its own
/// `(os error N)` tail, which no tmux message carries.
pub fn strerror_text(error: &io::Error) -> String {
    let text = error.to_string();
    text.split_once(" (os error ")
        .map_or(text.as_str(), |(message, _)| message)
        .to_owned()
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

pub fn diagnostic_timer() -> Option<Instant> {
    log::log_enabled!(target: "zz_daemon::diagnostics", log::Level::Trace).then(Instant::now)
}

pub fn diagnostic_elapsed_us(started: Option<Instant>) -> u128 {
    started.map_or(0, |started| started.elapsed().as_micros())
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
