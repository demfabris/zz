use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use interprocess::{
    TryClone,
    local_socket::{GenericFilePath, ListenerNonblockingMode, ListenerOptions, prelude::*},
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
    type Listener: TransportListener<Stream = Self::Stream>;
    type Stream: TransportStream;

    fn bind(endpoint: &Self::Endpoint) -> io::Result<Self::Listener>;
    fn connect(endpoint: &Self::Endpoint) -> io::Result<Self::Stream>;
}

pub(crate) trait TransportListener {
    type Stream: TransportStream;

    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()>;
    fn accept(&self) -> io::Result<Self::Stream>;

    fn wait_for_incoming(&self, timeout: Duration) -> io::Result<()> {
        std::thread::sleep(timeout);
        Ok(())
    }
}

pub(crate) trait TransportStream: Read + Write + Send + Sized + 'static {
    fn try_clone(&self) -> io::Result<Self>;

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
        return PathBuf::from(runtime).join("zz/default.sock");
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_owned());
    std::env::temp_dir().join(format!("zz-{user}/default.sock"))
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
    PathBuf::from(format!(r"\\.\pipe\zz-{user}-default"))
}

pub(crate) struct LocalTransport;

impl Transport for LocalTransport {
    type Endpoint = Path;
    type Listener = LocalListener;
    type Stream = LocalStream;

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

pub(crate) struct LocalListener(LocalSocketListener);

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
    fn wait_for_incoming(&self, timeout: Duration) -> io::Result<()> {
        let LocalSocketListener::UdSocket(listener) = &self.0;
        let timeout = rustix::event::Timespec::try_from(timeout).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "poll timeout is too large")
        })?;
        let mut fds = [rustix::event::PollFd::new(
            listener,
            rustix::event::PollFlags::IN,
        )];
        match rustix::event::poll(&mut fds, Some(&timeout)) {
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
        assert_eq!(
            platform_default_socket_path(),
            std::env::temp_dir().join(format!("zz-{user}/default.sock"))
        );
    }
}
