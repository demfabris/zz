use std::{
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use sysinfo::{Pid, Process, ProcessesToUpdate, Signal, System, get_current_pid};
use thiserror::Error;
use zz_protocol::PROTOCOL_VERSION;

use crate::transport::{LocalTransport, PeerCredentials, Transport};

const IDENTITY_MAGIC_V1: &str = "zz-daemon-identity-v1";
const IDENTITY_MAGIC_V2: &str = "zz-daemon-identity-v2";
const MAX_IDENTITY_BYTES: u64 = 512;
const TERMINATION_TIMEOUT: Duration = Duration::from_secs(3);
const TERMINATION_POLL_INTERVAL: Duration = Duration::from_millis(20);

static IDENTITY_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IdentityRecord {
    pid: u32,
    start_time: u64,
    protocol_version: Option<u16>,
}

impl IdentityRecord {
    fn current() -> io::Result<Self> {
        let pid = get_current_pid().map_err(io::Error::other)?;
        let system = System::new_all();
        let process = system
            .process(pid)
            .ok_or_else(|| io::Error::other(format!("could not inspect current process {pid}")))?;
        Ok(Self {
            pid: pid.as_u32(),
            start_time: process.start_time(),
            protocol_version: Some(PROTOCOL_VERSION),
        })
    }

    fn encode(self) -> String {
        match self.protocol_version {
            Some(protocol_version) => format!(
                "{IDENTITY_MAGIC_V2}\npid={}\nstart_time={}\nprotocol_version={protocol_version}\n",
                self.pid, self.start_time
            ),
            None => format!(
                "{IDENTITY_MAGIC_V1}\npid={}\nstart_time={}\n",
                self.pid, self.start_time
            ),
        }
    }

    fn parse(contents: &str) -> Result<Self, DaemonRecoveryError> {
        let mut lines = contents.lines();
        let identity_version = match lines.next() {
            Some(IDENTITY_MAGIC_V1) => 1,
            Some(IDENTITY_MAGIC_V2) => 2,
            _ => {
                return Err(DaemonRecoveryError::UnsafeTarget(
                    "daemon identity has an invalid header".to_owned(),
                ));
            }
        };
        let pid = parse_identity_value(lines.next(), "pid")?;
        let start_time = parse_identity_value(lines.next(), "start_time")?;
        let protocol_version = if identity_version == 2 {
            Some(parse_identity_value(lines.next(), "protocol_version")?)
        } else {
            None
        };
        if lines.next().is_some() {
            return Err(DaemonRecoveryError::UnsafeTarget(
                "daemon identity has unexpected trailing fields".to_owned(),
            ));
        }
        Ok(Self {
            pid,
            start_time,
            protocol_version,
        })
    }
}

fn parse_identity_value<T>(line: Option<&str>, key: &str) -> Result<T, DaemonRecoveryError>
where
    T: std::str::FromStr,
{
    let value = line
        .and_then(|line| line.strip_prefix(key))
        .and_then(|line| line.strip_prefix('='))
        .ok_or_else(|| {
            DaemonRecoveryError::UnsafeTarget(format!("daemon identity is missing the {key} field"))
        })?;
    value.parse().map_err(|_| {
        DaemonRecoveryError::UnsafeTarget(format!("daemon identity has an invalid {key} field"))
    })
}

#[derive(Debug)]
struct IdentityFile {
    path: PathBuf,
    record: IdentityRecord,
    contents: Vec<u8>,
}

pub(crate) struct DaemonIdentityGuard {
    path: PathBuf,
    contents: Vec<u8>,
}

impl DaemonIdentityGuard {
    pub(crate) fn install(socket_path: &Path) -> io::Result<Self> {
        let record = IdentityRecord::current()?;
        let contents = record.encode().into_bytes();
        let path = identity_path(socket_path);
        write_identity_atomically(&path, &contents)?;
        Ok(Self { path, contents })
    }
}

impl Drop for DaemonIdentityGuard {
    fn drop(&mut self) {
        remove_file_if_contents_match(&self.path, &self.contents);
    }
}

/// A daemon stopped through the guarded incompatible-protocol path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveredDaemon {
    pid: u32,
}

impl RecoveredDaemon {
    #[must_use]
    pub fn pid(self) -> u32 {
        self.pid
    }
}

#[derive(Debug, Error)]
pub enum DaemonRecoveryError {
    #[error("daemon recovery I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("refusing to terminate an unverified daemon: {0}")]
    UnsafeTarget(String),
    #[error("the operating system rejected the termination request for daemon pid {0}")]
    TerminationRejected(u32),
    #[error("daemon pid {0} did not stop within the recovery timeout")]
    Timeout(u32),
    #[error("the daemon endpoint was replaced while pid {0} was stopping")]
    EndpointReplaced(u32),
}

/// Stop the process that owns `socket_path` after a versioned protocol handshake failed.
///
/// Checks the target's owner, command line, and start time before it signals.
pub fn terminate_incompatible_daemon(
    socket_path: &Path,
) -> Result<RecoveredDaemon, DaemonRecoveryError> {
    #[cfg(unix)]
    let socket_identity = SocketFileIdentity::capture(socket_path)?;

    let stream = LocalTransport::connect(socket_path)?;
    let peer = stream.peer_credentials()?;

    #[cfg(unix)]
    if SocketFileIdentity::capture(socket_path)? != socket_identity {
        return Err(DaemonRecoveryError::UnsafeTarget(
            "daemon socket changed while establishing the recovery connection".to_owned(),
        ));
    }

    let identity = read_identity_file(socket_path)?;
    let pid = select_target_pid(peer, identity.as_ref())?;
    let process_pid = Pid::from_u32(pid);
    let current_pid = get_current_pid().map_err(io::Error::other)?;
    if process_pid == current_pid {
        return Err(DaemonRecoveryError::UnsafeTarget(
            "the daemon socket resolves to the current client process".to_owned(),
        ));
    }

    let mut system = System::new_all();
    let current = system.process(current_pid).ok_or_else(|| {
        DaemonRecoveryError::UnsafeTarget("could not inspect the current process".to_owned())
    })?;
    let target = system.process(process_pid).ok_or_else(|| {
        DaemonRecoveryError::UnsafeTarget(format!("daemon pid {pid} is not running"))
    })?;

    validate_target_process(current, target, peer, identity.as_ref())?;
    let start_time = target.start_time();
    if !request_termination(target) {
        return Err(DaemonRecoveryError::TerminationRejected(pid));
    }
    drop(stream);

    wait_for_process_exit(&mut system, process_pid, start_time)
        .then_some(())
        .ok_or(DaemonRecoveryError::Timeout(pid))?;

    #[cfg(unix)]
    cleanup_socket_if_unchanged(socket_path, socket_identity, pid)?;
    #[cfg(windows)]
    ensure_named_pipe_stopped(socket_path, pid)?;

    if let Some(identity) = identity {
        remove_file_if_contents_match(&identity.path, &identity.contents);
    }

    log::warn!(
        target: "zz_daemon::diagnostics::lifecycle",
        "terminated incompatible daemon pid={pid} socket={}",
        socket_path.display(),
    );
    Ok(RecoveredDaemon { pid })
}

fn select_target_pid(
    peer: PeerCredentials,
    identity: Option<&IdentityFile>,
) -> Result<u32, DaemonRecoveryError> {
    match (peer.pid, identity) {
        (Some(peer_pid), Some(identity)) if peer_pid != identity.record.pid => {
            Err(DaemonRecoveryError::UnsafeTarget(format!(
                "socket peer pid {peer_pid} does not match identity pid {}",
                identity.record.pid
            )))
        }
        (Some(peer_pid), _) => Ok(peer_pid),
        (None, Some(identity)) => Ok(identity.record.pid),
        (None, None) => Err(DaemonRecoveryError::UnsafeTarget(
            "the platform did not report a socket peer pid and no identity file exists".to_owned(),
        )),
    }
}

fn validate_target_process(
    current: &Process,
    target: &Process,
    peer: PeerCredentials,
    identity: Option<&IdentityFile>,
) -> Result<(), DaemonRecoveryError> {
    if !daemon_executable_and_command_match(target.exe(), target.cmd()) {
        return Err(DaemonRecoveryError::UnsafeTarget(format!(
            "socket owner pid {} is not a zz daemon",
            target.pid()
        )));
    }

    #[cfg(unix)]
    {
        let current_user = current.effective_user_id().or_else(|| current.user_id());
        let target_user = target.effective_user_id().or_else(|| target.user_id());
        if current_user.is_none() || current_user != target_user {
            return Err(DaemonRecoveryError::UnsafeTarget(
                "daemon and client process owners do not match".to_owned(),
            ));
        }
        if let Some(peer_user) = peer.effective_user_id {
            let target_user = target_user.expect("target user checked above");
            if **target_user != peer_user {
                return Err(DaemonRecoveryError::UnsafeTarget(
                    "socket peer owner does not match the daemon process owner".to_owned(),
                ));
            }
        }
    }

    #[cfg(windows)]
    {
        let _ = peer;
        if let (Some(current_user), Some(target_user)) = (current.user_id(), target.user_id())
            && current_user != target_user
        {
            return Err(DaemonRecoveryError::UnsafeTarget(
                "daemon and client process owners do not match".to_owned(),
            ));
        }
    }

    validate_identity_start_time(identity, target.pid().as_u32(), target.start_time())?;
    Ok(())
}

fn validate_identity_start_time(
    identity: Option<&IdentityFile>,
    pid: u32,
    start_time: u64,
) -> Result<(), DaemonRecoveryError> {
    if let Some(identity) = identity
        && identity.record.start_time != start_time
    {
        return Err(DaemonRecoveryError::UnsafeTarget(format!(
            "identity start time {} does not match pid {pid} start time {start_time}",
            identity.record.start_time,
        )));
    }
    Ok(())
}

fn daemon_executable_and_command_match(executable: Option<&Path>, command: &[OsString]) -> bool {
    let executable_matches = executable
        .and_then(Path::file_name)
        .is_some_and(daemon_executable_name_matches);
    executable_matches
        && command
            .iter()
            .skip(1)
            .any(|argument| argument == OsStr::new("daemon"))
}

fn daemon_executable_name_matches(name: &OsStr) -> bool {
    if name == OsStr::new("zz") || name == OsStr::new("zz.exe") {
        return true;
    }
    #[cfg(unix)]
    {
        // Linux /proc names a rebuilt-over executable `zz (deleted)`.
        name == OsStr::new("zz (deleted)")
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
fn request_termination(process: &Process) -> bool {
    process.kill_with(Signal::Term) == Some(true)
}

#[cfg(windows)]
fn request_termination(process: &Process) -> bool {
    // Windows has no SIGTERM equivalent in sysinfo.
    process.kill_with(Signal::Kill) == Some(true)
}

fn wait_for_process_exit(system: &mut System, pid: Pid, start_time: u64) -> bool {
    let deadline = Instant::now() + TERMINATION_TIMEOUT;
    loop {
        system.refresh_processes(ProcessesToUpdate::All);
        if system
            .process(pid)
            .is_none_or(|process| process.start_time() != start_time)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(TERMINATION_POLL_INTERVAL);
    }
}

fn read_identity_file(socket_path: &Path) -> Result<Option<IdentityFile>, DaemonRecoveryError> {
    let path = identity_path(socket_path);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        return Err(DaemonRecoveryError::UnsafeTarget(
            "daemon identity is not a regular file".to_owned(),
        ));
    }
    if metadata.len() > MAX_IDENTITY_BYTES {
        return Err(DaemonRecoveryError::UnsafeTarget(format!(
            "daemon identity exceeds {MAX_IDENTITY_BYTES} bytes"
        )));
    }

    #[cfg(unix)]
    validate_identity_permissions(socket_path, &metadata)?;

    let contents = fs::read(&path)?;
    let text = std::str::from_utf8(&contents).map_err(|_| {
        DaemonRecoveryError::UnsafeTarget("daemon identity is not valid UTF-8".to_owned())
    })?;
    let record = IdentityRecord::parse(text)?;
    Ok(Some(IdentityFile {
        path,
        record,
        contents,
    }))
}

/// Read the protocol version recorded by the daemon at `socket_path`.
///
/// The outer option distinguishes a valid identity from a missing, unreadable, or malformed one.
/// The inner option is `None` for a valid legacy v1 identity, which had no protocol field.
#[must_use]
pub fn daemon_identity_protocol_version(socket_path: &Path) -> Option<Option<u16>> {
    read_identity_file(socket_path)
        .ok()
        .flatten()
        .map(|identity| identity.record.protocol_version)
}

#[cfg(unix)]
fn validate_identity_permissions(
    socket_path: &Path,
    identity: &fs::Metadata,
) -> Result<(), DaemonRecoveryError> {
    use std::os::unix::fs::MetadataExt;

    if identity.mode() & 0o077 != 0 {
        return Err(DaemonRecoveryError::UnsafeTarget(
            "daemon identity is accessible by another user or group".to_owned(),
        ));
    }
    let socket = fs::symlink_metadata(socket_path)?;
    if identity.uid() != socket.uid() {
        return Err(DaemonRecoveryError::UnsafeTarget(
            "daemon identity and socket owners do not match".to_owned(),
        ));
    }
    Ok(())
}

fn write_identity_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let sequence = IDENTITY_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut temporary_name = path.as_os_str().to_owned();
    temporary_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
    let temporary = PathBuf::from(temporary_name);

    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);

        #[cfg(unix)]
        fs::rename(&temporary, path)?;
        #[cfg(windows)]
        {
            match fs::rename(&temporary, path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    fs::remove_file(path)?;
                    fs::rename(&temporary, path)?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_file_if_contents_match(path: &Path, expected: &[u8]) {
    if fs::read(path).is_ok_and(|contents| contents == expected) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(not(windows))]
fn identity_path(socket_path: &Path) -> PathBuf {
    let mut path = socket_path.as_os_str().to_owned();
    path.push(".identity");
    PathBuf::from(path)
}

#[cfg(windows)]
fn identity_path(socket_path: &Path) -> PathBuf {
    use std::os::windows::ffi::OsStrExt;

    let hash = socket_path
        .as_os_str()
        .encode_wide()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, unit| {
            (hash ^ u64::from(unit)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    std::env::temp_dir()
        .join("zz-daemon-identities")
        .join(format!("{hash:016x}.identity"))
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SocketFileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl SocketFileIdentity {
    fn capture(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a Unix socket", path.display()),
            ));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

#[cfg(unix)]
fn cleanup_socket_if_unchanged(
    path: &Path,
    expected: SocketFileIdentity,
    pid: u32,
) -> Result<(), DaemonRecoveryError> {
    match SocketFileIdentity::capture(path) {
        Ok(current) if current == expected => {}
        Ok(_) => return Err(DaemonRecoveryError::EndpointReplaced(pid)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }

    match LocalTransport::connect(path) {
        Ok(_) => return Err(DaemonRecoveryError::EndpointReplaced(pid)),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }

    match SocketFileIdentity::capture(path) {
        Ok(current) if current == expected => fs::remove_file(path).map_err(Into::into),
        Ok(_) => Err(DaemonRecoveryError::EndpointReplaced(pid)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(windows)]
fn ensure_named_pipe_stopped(path: &Path, pid: u32) -> Result<(), DaemonRecoveryError> {
    match LocalTransport::connect(path) {
        Ok(_) => Err(DaemonRecoveryError::EndpointReplaced(pid)),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_and_v2_identities_round_trip_strictly() {
        let v1 = IdentityRecord {
            pid: 42,
            start_time: 123_456,
            protocol_version: None,
        };
        let v2 = IdentityRecord {
            protocol_version: Some(49),
            ..v1
        };
        assert_eq!(IdentityRecord::parse(&v1.encode()).unwrap(), v1);
        assert_eq!(IdentityRecord::parse(&v2.encode()).unwrap(), v2);
        assert!(IdentityRecord::parse("zz-daemon-identity-v1\npid=42\n").is_err());
        assert!(
            IdentityRecord::parse("zz-daemon-identity-v1\npid=42\nstart_time=123456\nextra=true\n")
                .is_err()
        );
        assert!(
            IdentityRecord::parse(
                "zz-daemon-identity-v2\npid=42\nstart_time=123456\nprotocol_version=49\nextra=true\n"
            )
            .is_err()
        );
        assert!(
            IdentityRecord::parse(
                "zz-daemon-identity-v2\npid=42\nstart_time=123456\nprotocol_version=nope\n"
            )
            .is_err()
        );
    }

    #[test]
    fn identity_protocol_helper_distinguishes_missing_v1_and_v2() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("daemon.sock");
        fs::write(&socket, b"").unwrap();
        assert_eq!(daemon_identity_protocol_version(&socket), None);

        let path = identity_path(&socket);
        write_identity_atomically(
            &path,
            IdentityRecord {
                pid: 42,
                start_time: 100,
                protocol_version: None,
            }
            .encode()
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(daemon_identity_protocol_version(&socket), Some(None));

        write_identity_atomically(
            &path,
            IdentityRecord {
                pid: 42,
                start_time: 100,
                protocol_version: Some(49),
            }
            .encode()
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(daemon_identity_protocol_version(&socket), Some(Some(49)));

        write_identity_atomically(&path, b"not-an-identity").unwrap();
        assert_eq!(daemon_identity_protocol_version(&socket), None);
    }

    #[test]
    fn peer_pid_and_identity_must_match() {
        let identity = IdentityFile {
            path: PathBuf::new(),
            record: IdentityRecord {
                pid: 42,
                start_time: 100,
                protocol_version: Some(49),
            },
            contents: Vec::new(),
        };
        let peer = PeerCredentials {
            pid: Some(41),
            #[cfg(unix)]
            effective_user_id: None,
        };
        assert!(select_target_pid(peer, Some(&identity)).is_err());
    }

    #[test]
    fn reused_pid_with_a_different_start_time_is_rejected() {
        let identity = IdentityFile {
            path: PathBuf::new(),
            record: IdentityRecord {
                pid: 42,
                start_time: 100,
                protocol_version: Some(49),
            },
            contents: Vec::new(),
        };
        assert!(validate_identity_start_time(Some(&identity), 42, 101).is_err());
        assert!(validate_identity_start_time(Some(&identity), 42, 100).is_ok());
    }

    #[test]
    fn executable_and_daemon_argument_are_both_required() {
        let daemon = vec![OsString::from("/tmp/zz"), OsString::from("daemon")];
        assert!(daemon_executable_and_command_match(
            Some(Path::new("/tmp/zz")),
            &daemon
        ));
        assert!(!daemon_executable_and_command_match(
            Some(Path::new("/tmp/not-zz")),
            &daemon
        ));
        assert!(!daemon_executable_and_command_match(
            Some(Path::new("/tmp/zz")),
            &[OsString::from("/tmp/zz"), OsString::from("list-sessions")]
        ));
        assert!(!daemon_executable_and_command_match(None, &daemon));
    }

    #[cfg(unix)]
    #[test]
    fn deleted_executable_requires_the_exact_zz_basename() {
        let daemon = vec![OsString::from("/tmp/zz"), OsString::from("daemon")];
        assert!(daemon_executable_and_command_match(
            Some(Path::new("/tmp/zz (deleted)")),
            &daemon
        ));
        for executable in [
            "/tmp/not-zz (deleted)",
            "/tmp/zz.exe (deleted)",
            "/tmp/zz (deleted) backup",
        ] {
            assert!(!daemon_executable_and_command_match(
                Some(Path::new(executable)),
                &daemon
            ));
        }
        assert!(!daemon_executable_and_command_match(
            Some(Path::new("/tmp/zz (deleted)")),
            &[OsString::from("/tmp/zz"), OsString::from("list-sessions")]
        ));
    }

    #[cfg(unix)]
    #[test]
    fn stale_socket_cleanup_refuses_a_replacement_inode() {
        use std::os::unix::net::UnixListener;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("daemon.sock");
        let first = UnixListener::bind(&path).unwrap();
        let identity = SocketFileIdentity::capture(&path).unwrap();
        drop(first);
        fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();

        assert!(matches!(
            cleanup_socket_if_unchanged(&path, identity, 42),
            Err(DaemonRecoveryError::EndpointReplaced(42))
        ));
        assert!(path.exists());
        drop(replacement);
    }

    #[cfg(unix)]
    #[test]
    fn stale_socket_cleanup_refuses_a_live_endpoint_with_matching_identity() {
        use std::os::unix::net::UnixListener;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("daemon.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let identity = SocketFileIdentity::capture(&path).unwrap();

        assert!(matches!(
            cleanup_socket_if_unchanged(&path, identity, 42),
            Err(DaemonRecoveryError::EndpointReplaced(42))
        ));
        assert!(path.exists());
        drop(listener);
    }

    #[cfg(unix)]
    #[test]
    fn stale_socket_cleanup_removes_an_unowned_stale_endpoint() {
        use std::os::unix::net::UnixListener;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("daemon.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let identity = SocketFileIdentity::capture(&path).unwrap();
        drop(listener);

        cleanup_socket_if_unchanged(&path, identity, 42).unwrap();
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn malformed_and_public_identity_files_are_rejected() {
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};

        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let identity = identity_path(&socket);
        fs::write(&identity, b"not-an-identity").unwrap();
        fs::set_permissions(&identity, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_identity_file(&socket).is_err());

        fs::write(
            &identity,
            IdentityRecord {
                pid: 42,
                start_time: 100,
                protocol_version: Some(49),
            }
            .encode(),
        )
        .unwrap();
        fs::set_permissions(&identity, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_identity_file(&socket).is_err());
        drop(listener);
    }

    #[test]
    fn identity_guard_never_removes_replacement_contents() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("daemon.sock");
        let path = identity_path(&socket);
        write_identity_atomically(&path, b"original").unwrap();
        let guard = DaemonIdentityGuard {
            path: path.clone(),
            contents: b"original".to_vec(),
        };
        fs::write(&path, b"replacement").unwrap();
        drop(guard);
        assert_eq!(fs::read(path).unwrap(), b"replacement");
    }
}
