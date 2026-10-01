#[cfg(windows)]
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

    #[cfg(unix)]
    fn raw_fd(&self) -> std::os::fd::RawFd;

    #[cfg(unix)]
    fn accept_fd(&self) -> io::Result<std::os::fd::OwnedFd> {
        self.accept()?.receive_fd()
    }

    #[cfg(windows)]
    fn wait_for_incoming(&self, timeout: Duration, _wake: &AcceptWake) -> io::Result<()> {
        std::thread::sleep(timeout);
        Ok(())
    }
}

#[cfg(any(feature = "daemon", windows))]
pub(crate) struct AcceptWake {
    #[cfg(all(unix, feature = "daemon"))]
    waker: parking_lot::Mutex<Option<std::sync::Arc<mio::Waker>>>,
}

#[cfg(any(feature = "daemon", windows))]
impl AcceptWake {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(all(unix, feature = "daemon"))]
            waker: parking_lot::Mutex::new(None),
        }
    }

    #[cfg(all(unix, feature = "daemon"))]
    pub(crate) fn install(&self, waker: std::sync::Arc<mio::Waker>) {
        *self.waker.lock() = Some(waker);
    }

    pub(crate) fn wake(&self) {
        #[cfg(all(unix, feature = "daemon"))]
        if let Some(waker) = self.waker.lock().as_ref()
            && let Err(error) = waker.wake()
        {
            log::warn!("could not wake the mux loop: {error}");
        }
    }
}

pub(crate) trait TransportStream: Read + Write + Send + Sized + 'static {
    fn try_clone(&self) -> io::Result<Self>;

    #[cfg(unix)]
    fn receive_fd(&self) -> io::Result<std::os::fd::OwnedFd> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    #[cfg(unix)]
    fn read_ready(&self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    #[cfg(all(feature = "daemon", any(windows, test)))]
    fn shutdown(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(feature = "daemon")]
    fn set_send_buffer_size(&self, _bytes: usize) -> io::Result<()> {
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
    fn raw_fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::{AsFd as _, AsRawFd as _};
        let LocalSocketListener::UdSocket(listener) = &self.0;
        listener.as_fd().as_raw_fd()
    }

    #[cfg(unix)]
    fn accept_fd(&self) -> io::Result<std::os::fd::OwnedFd> {
        let LocalSocketStream::UdSocket(stream) = self.0.accept()?;
        Ok(stream.into())
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

    #[cfg(unix)]
    fn receive_fd(&self) -> io::Result<std::os::fd::OwnedFd> {
        use std::os::fd::AsFd as _;
        match &self.0 {
            LocalSocketStream::UdSocket(stream) => stream.inner().as_fd().try_clone_to_owned(),
        }
    }

    #[cfg(unix)]
    fn read_ready(&self, buffer: &mut [u8]) -> io::Result<usize> {
        match &self.0 {
            LocalSocketStream::UdSocket(stream) => {
                rustix::net::recv(stream.inner(), buffer, rustix::net::RecvFlags::DONTWAIT)
                    .map(|(read, _)| read)
                    .map_err(io::Error::from)
            }
        }
    }

    #[cfg(all(feature = "daemon", unix, test))]
    fn shutdown(&self) -> io::Result<()> {
        LocalStream::shutdown(self)
    }

    #[cfg(all(feature = "daemon", unix))]
    fn set_send_buffer_size(&self, bytes: usize) -> io::Result<()> {
        match &self.0 {
            LocalSocketStream::UdSocket(stream) => {
                rustix::net::sockopt::set_socket_send_buffer_size(stream.inner(), bytes)
                    .map_err(io::Error::from)
            }
        }
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

    fn write_vectored(&mut self, buffers: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.0.write_vectored(buffers)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(all(unix, feature = "daemon"))]
impl TransportStream for std::os::unix::net::UnixStream {
    fn try_clone(&self) -> io::Result<Self> {
        Self::try_clone(self)
    }
    fn receive_fd(&self) -> io::Result<std::os::fd::OwnedFd> {
        use std::os::fd::AsFd;
        self.as_fd().try_clone_to_owned()
    }
    #[cfg(test)]
    fn shutdown(&self) -> io::Result<()> {
        self.shutdown(std::net::Shutdown::Both)
    }
    fn set_send_buffer_size(&self, bytes: usize) -> io::Result<()> {
        rustix::net::sockopt::set_socket_send_buffer_size(self, bytes).map_err(io::Error::from)
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
}
