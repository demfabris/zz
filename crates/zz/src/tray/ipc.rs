use std::{
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
};

use async_channel::Sender;
use interprocess::{
    TryClone as _,
    local_socket::{GenericFilePath, ListenerOptions, Stream, prelude::*},
};
use sha2::{Digest as _, Sha256};

use super::host::HostEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DesktopEvent {
    Available(bool),
    Toggle,
    Quit,
    Show,
}

impl DesktopEvent {
    fn encode(self) -> u8 {
        match self {
            Self::Available(false) => 0,
            Self::Available(true) => 1,
            Self::Toggle => 2,
            Self::Quit => 3,
            Self::Show => 4,
        }
    }

    fn decode(value: u8) -> io::Result<Self> {
        match value {
            0 => Ok(Self::Available(false)),
            1 => Ok(Self::Available(true)),
            2 => Ok(Self::Toggle),
            3 => Ok(Self::Quit),
            4 => Ok(Self::Show),
            _ => Err(io::Error::other("invalid tray event")),
        }
    }
}

pub(super) fn endpoint(socket: &Path, server_id: u64) -> PathBuf {
    let digest = Sha256::digest(socket.as_os_str().as_encoded_bytes());
    let hash = u64::from_le_bytes(digest[..8].try_into().expect("eight digest bytes"));
    #[cfg(unix)]
    {
        let uid = rustix::process::geteuid().as_raw();
        PathBuf::from(format!(
            "/tmp/zz-tray-{uid}-{hash:016x}-{server_id:016x}.sock"
        ))
    }
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\zz-tray-{hash:016x}-{server_id:016x}"))
    }
}

pub(super) struct ListenerGuard {
    #[cfg(unix)]
    path: PathBuf,
    _lock: std::fs::File,
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) fn listen(path: PathBuf, events: Sender<HostEvent>) -> io::Result<ListenerGuard> {
    let lock_path = {
        #[cfg(unix)]
        {
            path.with_extension("lock")
        }
        #[cfg(windows)]
        {
            std::env::temp_dir()
                .join(
                    path.file_name()
                        .ok_or_else(|| io::Error::other("tray endpoint name"))?,
                )
                .with_extension("lock")
        }
    };
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(lock_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = lock.metadata()?;
        if !metadata.is_file() || metadata.uid() != rustix::process::geteuid().as_raw() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "foreign tray lock",
            ));
        }
    }
    lock.try_lock()
        .map_err(|error| io::Error::new(io::ErrorKind::AddrInUse, error.to_string()))?;
    #[cfg(unix)]
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
        if !metadata.file_type().is_socket()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || connect(&path).is_ok()
        {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "tray endpoint is in use",
            ));
        }
        std::fs::remove_file(&path)?;
    }
    let options = ListenerOptions::new()
        .name(path.as_os_str().to_fs_name::<GenericFilePath>()?)
        .reclaim_name(false);
    let listener = options.create_sync()?;
    let guard = ListenerGuard {
        #[cfg(unix)]
        path,
        _lock: lock,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&guard.path, std::fs::Permissions::from_mode(0o600))?;
    }
    thread::Builder::new()
        .name("zz-tray-accept".into())
        .spawn(move || {
            let next_id = AtomicU64::new(1);
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let events = events.clone();
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                let _ = thread::Builder::new()
                    .name("zz-tray-client".into())
                    .spawn(move || serve(stream, id, &events));
            }
        })?;
    Ok(guard)
}

fn serve(mut stream: Stream, id: u64, events: &Sender<HostEvent>) {
    if !same_user(&stream) {
        return;
    }
    let mut message = [0];
    if stream.read_exact(&mut message).is_err() || message[0] != b'1' {
        return;
    }
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let (sender, receiver) = async_channel::bounded::<DesktopEvent>(16);
    if thread::Builder::new()
        .name("zz-tray-events".into())
        .spawn(move || {
            while let Ok(event) = receiver.recv_blocking() {
                if writer.write_all(&[event.encode()]).is_err() {
                    break;
                }
            }
        })
        .is_err()
    {
        return;
    }
    let (acknowledge, acknowledged) = crossbeam_channel::bounded(1);
    if events
        .try_send(HostEvent::Connected(
            id,
            DesktopPeer {
                sender,
                acknowledged,
            },
        ))
        .is_err()
    {
        return;
    }
    while stream.read_exact(&mut message).is_ok() {
        match message[0] {
            b'A' => {
                if events.try_send(HostEvent::Focused(id)).is_err() {
                    break;
                }
            }
            b'Q' => {
                let _ = acknowledge.try_send(());
            }
            _ => break,
        }
    }
    let _ = events.try_send(HostEvent::Disconnected(id));
}

#[derive(Clone)]
pub(super) struct DesktopPeer {
    pub(super) sender: Sender<DesktopEvent>,
    pub(super) acknowledged: crossbeam_channel::Receiver<()>,
}

#[cfg(unix)]
fn same_user(stream: &Stream) -> bool {
    stream
        .peer_creds()
        .is_ok_and(|credentials| credentials.euid() == Some(rustix::process::geteuid().as_raw()))
}

#[cfg(windows)]
fn same_user(stream: &Stream) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let Ok(credentials) = stream.peer_creds() else {
        return false;
    };
    let Some(pid) = credentials.pid() else {
        return false;
    };
    let own = Pid::from_u32(std::process::id());
    let peer = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[own, peer]),
        ProcessRefreshKind::new().with_user(UpdateKind::Always),
    );
    let own = system.process(own).and_then(|process| process.user_id());
    let peer = system.process(peer).and_then(|process| process.user_id());
    own.is_some() && own == peer
}

pub(super) fn connect(path: &Path) -> io::Result<Stream> {
    Stream::connect(path.as_os_str().to_fs_name::<GenericFilePath>()?)
}

pub(super) fn register(stream: &mut Stream) -> io::Result<()> {
    if !same_user(stream) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "foreign tray owner",
        ));
    }
    stream.write_all(b"1")
}

pub(super) fn focus(stream: &mut Stream) -> io::Result<()> {
    stream.write_all(b"A")
}

pub(super) fn acknowledge_quit(stream: &mut Stream) -> io::Result<()> {
    stream.write_all(b"Q")
}

pub(super) fn disconnect(stream: &mut Stream) -> io::Result<()> {
    stream.write_all(b"D")
}

pub(super) fn receive(stream: &mut Stream) -> io::Result<DesktopEvent> {
    let mut message = [0];
    stream.read_exact(&mut message)?;
    DesktopEvent::decode(message[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_separates_daemon_instances_and_sockets() {
        let socket = Path::new("/tmp/one.sock");
        assert_ne!(endpoint(socket, 1), endpoint(socket, 2));
        assert_ne!(endpoint(socket, 1), endpoint(Path::new("/tmp/two.sock"), 1));
        assert!(
            endpoint(&PathBuf::from("x".repeat(200)), 1)
                .as_os_str()
                .len()
                < 100
        );
    }

    #[test]
    fn desktop_connects_receives_events_and_disconnects() {
        let path = endpoint(Path::new("tray-ipc-test"), std::process::id().into());
        let (events, receiver) = async_channel::unbounded();
        let _listener = listen(path.clone(), events.clone()).expect("listen");
        assert!(
            matches!(listen(path.clone(), events), Err(error) if error.kind() == io::ErrorKind::AddrInUse)
        );
        let mut client = connect(&path).expect("connect");
        register(&mut client).expect("register");
        let HostEvent::Connected(id, peer) = receiver.recv_blocking().expect("registered") else {
            panic!("expected GUI registration");
        };
        peer.sender
            .try_send(DesktopEvent::Available(true))
            .expect("available");
        assert_eq!(
            receive(&mut client).expect("receive"),
            DesktopEvent::Available(true)
        );
        peer.sender.try_send(DesktopEvent::Toggle).expect("toggle");
        assert_eq!(receive(&mut client).expect("receive"), DesktopEvent::Toggle);
        focus(&mut client).expect("focus");
        assert!(matches!(receiver.recv_blocking(), Ok(HostEvent::Focused(value)) if value == id));
        acknowledge_quit(&mut client).expect("acknowledge quit");
        peer.acknowledged.recv().expect("GUI accepted quit");
        disconnect(&mut client).expect("disconnect");
        assert!(
            matches!(receiver.recv_blocking(), Ok(HostEvent::Disconnected(value)) if value == id)
        );
    }
}
