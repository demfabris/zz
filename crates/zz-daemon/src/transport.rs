#[cfg(any(feature = "daemon", windows))]
use std::time::Duration;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[cfg(any(feature = "daemon", windows))]
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions};
use interprocess::{
    TryClone,
    local_socket::{GenericFilePath, prelude::*},
};

pub(crate) const SOCKET_ENVIRONMENT_VARIABLE: &str = "ZZ_SOCKET";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PeerCredentials {
    pub(crate) pid: Option<u32>,
    #[cfg(unix)]
    pub(crate) effective_user_id: Option<u32>,
}

pub(crate) trait Transport {
    type Endpoint: ?Sized;
    #[cfg(any(feature = "daemon", windows))]
    type Listener: TransportListener<Stream = Self::Stream>;
    type Stream: TransportStream;

    #[cfg(any(feature = "daemon", windows))]
    fn bind(endpoint: &Self::Endpoint) -> io::Result<Self::Listener>;
    fn connect(endpoint: &Self::Endpoint) -> io::Result<Self::Stream>;
}

#[cfg(any(feature = "daemon", windows))]
pub(crate) trait TransportListener {
    type Stream: TransportStream;

    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()>;
    fn accept(&self) -> io::Result<Self::Stream>;

    fn wait_for_incoming(&self, timeout: Duration, _wake: &AcceptWake) -> io::Result<()> {
        std::thread::sleep(timeout);
        Ok(())
    }
}

#[cfg(any(feature = "daemon", windows))]
pub(crate) struct AcceptWake {
    #[cfg(unix)]
    pair: Option<(
        std::os::unix::net::UnixStream,
        std::os::unix::net::UnixStream,
    )>,
}

#[cfg(any(feature = "daemon", windows))]
impl AcceptWake {
    pub(crate) fn new() -> Self {
        #[cfg(unix)]
        {
            let pair = std::os::unix::net::UnixStream::pair().and_then(|(reader, writer)| {
                reader.set_nonblocking(true)?;
                writer.set_nonblocking(true)?;
                Ok((reader, writer))
            });
            if let Err(error) = &pair {
                log::warn!("could not create the accept wake pair: {error}");
            }
            Self { pair: pair.ok() }
        }
        #[cfg(not(unix))]
        Self {}
    }

    pub(crate) fn wake(&self) {
        #[cfg(unix)]
        if let Some((_, writer)) = &self.pair {
            let _ = (&*writer).write(&[1]);
        }
    }
}

pub(crate) trait TransportStream: Read + Write + Send + Sized + 'static {
    fn try_clone(&self) -> io::Result<Self>;

    #[cfg(feature = "daemon")]
    fn shutdown(&self) -> io::Result<()> {
        Ok(())
    }
}

#[must_use]
pub fn default_socket_path() -> PathBuf {
    resolve_socket_path(std::env::var_os(SOCKET_ENVIRONMENT_VARIABLE))
}

fn resolve_socket_path(override_path: Option<OsString>) -> PathBuf {
    override_path.map_or_else(platform_default_socket_path, PathBuf::from)
}

#[cfg(unix)]
fn platform_default_socket_path() -> PathBuf {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime)
            .join(zz_protocol::app_identity::DIRECTORY)
            .join("default.sock");
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_owned());
    let directory = zz_protocol::app_identity::DIRECTORY;
    std::env::temp_dir().join(format!("{directory}-{user}/default.sock"))
}

#[cfg(windows)]
fn platform_default_socket_path() -> PathBuf {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
    let user = user
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let directory = zz_protocol::app_identity::DIRECTORY;
    PathBuf::from(format!(r"\\.\pipe\{directory}-{user}-default"))
}

pub(crate) struct LocalTransport;

impl Transport for LocalTransport {
    type Endpoint = Path;
    #[cfg(any(feature = "daemon", windows))]
    type Listener = LocalListener;
    type Stream = LocalStream;

    #[cfg(any(feature = "daemon", windows))]
    fn bind(endpoint: &Self::Endpoint) -> io::Result<Self::Listener> {
        let name = endpoint.as_os_str().to_fs_name::<GenericFilePath>()?;
        ListenerOptions::new()
            .name(name)
            .create_sync()
            .map(LocalListener)
    }

    fn connect(endpoint: &Self::Endpoint) -> io::Result<Self::Stream> {
        let name = endpoint.as_os_str().to_fs_name::<GenericFilePath>()?;
        LocalSocketStream::connect(name).map(LocalStream)
    }
}

#[cfg(any(feature = "daemon", windows))]
pub(crate) struct LocalListener(LocalSocketListener);

#[cfg(any(feature = "daemon", windows))]
impl TransportListener for LocalListener {
    type Stream = LocalStream;

    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        self.0.set_nonblocking(if nonblocking {
            ListenerNonblockingMode::Accept
        } else {
            ListenerNonblockingMode::Neither
        })
    }

    fn accept(&self) -> io::Result<Self::Stream> {
        let stream = self.0.accept()?;
        stream.set_nonblocking(false)?;
        Ok(LocalStream(stream))
    }

    #[cfg(unix)]
    fn wait_for_incoming(&self, timeout: Duration, wake: &AcceptWake) -> io::Result<()> {
        let LocalSocketListener::UdSocket(listener) = &self.0;
        let timeout = rustix::event::Timespec::try_from(timeout).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "poll timeout is too large")
        })?;
        let listener = rustix::event::PollFd::new(listener, rustix::event::PollFlags::IN);
        let result = match &wake.pair {
            Some((reader, _)) => {
                let mut fds = [
                    listener,
                    rustix::event::PollFd::new(reader, rustix::event::PollFlags::IN),
                ];
                let result = rustix::event::poll(&mut fds, Some(&timeout));
                if !fds[1].revents().is_empty() {
                    let mut buffer = [0; 64];
                    while matches!((&*reader).read(&mut buffer), Ok(read) if read > 0) {}
                }
                result
            }
            None => rustix::event::poll(&mut [listener], Some(&timeout)),
        };
        match result {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

pub(crate) struct LocalStream(LocalSocketStream);

impl LocalStream {
    pub(crate) fn peer_credentials(&self) -> io::Result<PeerCredentials> {
        let credentials = self.0.peer_creds()?;
        #[cfg(unix)]
        let pid = credentials.pid().and_then(|pid| u32::try_from(pid).ok());
        #[cfg(windows)]
        let pid = credentials.pid();

        Ok(PeerCredentials {
            pid,
            #[cfg(unix)]
            effective_user_id: credentials.euid(),
        })
    }

    #[cfg(unix)]
    pub(crate) fn set_timeout(&self, timeout: Option<std::time::Duration>) -> io::Result<()> {
        match &self.0 {
            LocalSocketStream::UdSocket(stream) => {
                stream.inner().set_read_timeout(timeout)?;
                stream.inner().set_write_timeout(timeout)
            }
        }
    }

    #[cfg(unix)]
    pub(crate) fn shutdown(&self) -> io::Result<()> {
        match &self.0 {
            LocalSocketStream::UdSocket(stream) => {
                stream.inner().shutdown(std::net::Shutdown::Both)
            }
        }
    }
}

impl TransportStream for LocalStream {
    fn try_clone(&self) -> io::Result<Self> {
        self.0.try_clone().map(Self)
    }

    #[cfg(all(feature = "daemon", unix))]
    fn shutdown(&self) -> io::Result<()> {
        LocalStream::shutdown(self)
    }
}

impl Read for LocalStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for LocalStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_socket_path_takes_precedence() {
        let path = PathBuf::from("custom.sock");
        assert_eq!(
            resolve_socket_path(Some(path.clone().into_os_string())),
            path
        );
    }

    #[cfg(unix)]
    #[test]
    fn platform_default_uses_std_temp_dir_when_xdg_runtime_dir_is_unset() {
        if std::env::var_os("XDG_RUNTIME_DIR").is_some() {
            return;
        }
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_owned());
        let directory = zz_protocol::app_identity::DIRECTORY;
        assert_eq!(
            platform_default_socket_path(),
            std::env::temp_dir().join(format!("{directory}-{user}/default.sock"))
        );
    }

    #[cfg(all(unix, feature = "daemon"))]
    #[test]
    fn accept_wake_ends_an_idle_wait_before_its_timeout() {
        let directory = tempfile::Builder::new()
            .prefix("zzw.")
            .tempdir_in("/tmp")
            .expect("socket directory");
        let listener = LocalTransport::bind(&directory.path().join("s")).expect("listener");
        let wake = AcceptWake::new();
        wake.wake();
        let started = std::time::Instant::now();
        listener
            .wait_for_incoming(Duration::from_secs(5), &wake)
            .expect("woken wait");
        assert!(started.elapsed() < Duration::from_secs(1));
        let started = std::time::Instant::now();
        listener
            .wait_for_incoming(Duration::from_millis(50), &wake)
            .expect("drained wait");
        assert!(started.elapsed() >= Duration::from_millis(40));
    }
}
