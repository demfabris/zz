use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
    time::Duration,
};

use russh::{ChannelMsg, ChannelOpenFailure, client};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpSocket, TcpStream},
    sync::{Semaphore, oneshot},
    task::{JoinHandle, JoinSet},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONNECTIONS: usize = 128;
const MAX_LOOPBACK_PORTS: usize = 64;
const INVENTORY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_INVENTORY_BYTES: usize = 64 * 1024;
const INVENTORY_COMMAND: &str = "LC_ALL=C; export LC_ALL; if [ \"$(uname -s)\" = Darwin ]; then printf 'lsof\\n'; /usr/sbin/lsof -nP -iTCP -sTCP:LISTEN -F n; elif command -v ss >/dev/null 2>&1; then printf 'ss\\n'; ss -H -ltn; else printf 'lsof\\n'; lsof -nP -iTCP -sTCP:LISTEN -F n; fi";

pub(crate) struct SocksForward {
    task: JoinHandle<()>,
    port: Arc<AtomicU16>,
}

impl SocksForward {
    pub(crate) async fn start<H: client::Handler + 'static>(
        session: Arc<client::Handle<H>>,
        port: Arc<AtomicU16>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let bound_port = listener.local_addr()?.port();
        let worker_port = Arc::clone(&port);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                        let Ok((stream, origin)) = accepted else { break };
                        let session = Arc::clone(&session);
                        connections.spawn(async move {
                            let _ = forward(stream, origin, &session).await;
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
            worker_port.store(0, Ordering::Release);
        });
        port.store(bound_port, Ordering::Release);
        Ok(Self { task, port })
    }
}

impl Drop for SocksForward {
    fn drop(&mut self) {
        self.port.store(0, Ordering::Release);
        self.task.abort();
    }
}

pub(crate) struct LoopbackForward<H: client::Handler + 'static> {
    session: Arc<client::Handle<H>>,
    runtime: tokio::runtime::Handle,
    ports: BTreeMap<u16, LoopbackPort>,
    explicit: BTreeSet<u16>,
    retired: Vec<JoinHandle<()>>,
    connections: Arc<Semaphore>,
}

struct LoopbackPort {
    task: JoinHandle<()>,
    retire: oneshot::Sender<()>,
}

impl<H: client::Handler + 'static> LoopbackForward<H> {
    pub(crate) fn new(session: Arc<client::Handle<H>>) -> Self {
        Self {
            session,
            runtime: tokio::runtime::Handle::current(),
            ports: BTreeMap::new(),
            explicit: BTreeSet::new(),
            retired: Vec::new(),
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        }
    }

    pub(crate) fn ensure(&mut self, port: u16) -> io::Result<()> {
        self.bind(port)?;
        self.explicit.insert(port);
        Ok(())
    }

    pub(crate) fn reconcile(&mut self, discovered: &BTreeSet<u16>) {
        self.retired.retain(|task| !task.is_finished());
        let removed: Vec<_> = self
            .ports
            .keys()
            .copied()
            .filter(|port| !self.explicit.contains(port) && !discovered.contains(port))
            .collect();
        for port in removed {
            if let Some(forward) = self.ports.remove(&port) {
                let _ = forward.retire.send(());
                self.retired.push(forward.task);
            }
        }
        for &port in discovered {
            if let Err(error) = self.bind(port) {
                log::debug!("Cannot forward discovered localhost port {port}: {error}");
            }
        }
    }

    fn bind(&mut self, port: u16) -> io::Result<()> {
        if port == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "A nonzero localhost port is required.",
            ));
        }
        if self.session.is_closed() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "The SSH connection is closed.",
            ));
        }
        if let Some(task) = self.ports.get(&port) {
            return if task.task.is_finished() {
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "The localhost forward has stopped.",
                ))
            } else {
                Ok(())
            };
        }
        if self.ports.len() >= MAX_LOOPBACK_PORTS {
            return Err(io::Error::new(
                io::ErrorKind::ResourceBusy,
                "The SSH connection already forwards 64 localhost ports.",
            ));
        }
        let ipv4 = TcpSocket::new_v4()?;
        let ipv6 = TcpSocket::new_v6()?;
        #[cfg(unix)]
        {
            ipv4.set_reuseaddr(true)?;
            ipv6.set_reuseaddr(true)?;
        }
        ipv4.bind((Ipv4Addr::LOCALHOST, port).into())?;
        ipv6.bind((Ipv6Addr::LOCALHOST, port).into())?;
        let _runtime = self.runtime.enter();
        let ipv4 = ipv4.listen(128)?;
        let ipv6 = ipv6.listen(128)?;
        let session = Arc::clone(&self.session);
        let capacity = Arc::clone(&self.connections);
        let (retire, mut retired) = oneshot::channel();
        let task = self.runtime.spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                let accepted = tokio::select! {
                    _ = &mut retired => break,
                    accepted = ipv4.accept() => accepted,
                    accepted = ipv6.accept() => accepted,
                    _ = connections.join_next(), if !connections.is_empty() => continue,
                };
                let Ok((mut stream, origin)) = accepted else {
                    break;
                };
                let Ok(permit) = Arc::clone(&capacity).try_acquire_owned() else {
                    continue;
                };
                let session = Arc::clone(&session);
                connections.spawn(async move {
                    let _permit = permit;
                    let channel = tokio::time::timeout(
                        CONNECT_TIMEOUT,
                        session.channel_open_direct_tcpip(
                            "localhost",
                            u32::from(port),
                            origin.ip().to_string(),
                            u32::from(origin.port()),
                        ),
                    )
                    .await;
                    if let Ok(Ok(channel)) = channel {
                        let _ =
                            tokio::io::copy_bidirectional(&mut stream, &mut channel.into_stream())
                                .await;
                    }
                });
            }
            drop((ipv4, ipv6));
            while connections.join_next().await.is_some() {}
        });
        self.ports.insert(port, LoopbackPort { task, retire });
        Ok(())
    }
}

impl<H: client::Handler + 'static> Drop for LoopbackForward<H> {
    fn drop(&mut self) {
        for forward in self.ports.values() {
            forward.task.abort();
        }
        for task in &self.retired {
            task.abort();
        }
    }
}

pub(crate) async fn discover_loopback_ports<H: client::Handler>(
    session: &client::Handle<H>,
) -> io::Result<BTreeSet<u16>> {
    let mut channel = tokio::time::timeout(INVENTORY_TIMEOUT, session.channel_open_session())
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
        .map_err(io::Error::other)?;
    let result = tokio::time::timeout(INVENTORY_TIMEOUT, async {
        channel
            .exec(true, INVENTORY_COMMAND)
            .await
            .map_err(io::Error::other)?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut status = None;
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    if stdout.len() + stderr.len() + data.len() > MAX_INVENTORY_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "The host listener inventory is too large.",
                        ));
                    }
                    stdout.extend_from_slice(&data);
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    if stdout.len() + stderr.len() + data.len() > MAX_INVENTORY_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "The host listener inventory is too large.",
                        ));
                    }
                    stderr.extend_from_slice(&data);
                }
                ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                _ => {}
            }
        }
        if status != Some(0) && !(status == Some(1) && stdout == b"lsof\n" && stderr.is_empty()) {
            return Err(io::Error::other(
                "Could not discover the host's TCP listeners.",
            ));
        }
        parse_listener_inventory(&stdout)
    })
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))
    .and_then(|result| result);
    let _ = tokio::time::timeout(Duration::from_millis(200), channel.close()).await;
    result
}

fn parse_listener_inventory(output: &[u8]) -> io::Result<BTreeSet<u16>> {
    let text = std::str::from_utf8(output).map_err(|_| io::ErrorKind::InvalidData)?;
    let mut lines = text.lines();
    let format = lines.next().ok_or(io::ErrorKind::InvalidData)?;
    if format != "lsof" && format != "ss" {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut ports = BTreeSet::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let address = match format {
            "lsof" => {
                let Some(address) = line.strip_prefix('n') else {
                    continue;
                };
                address
            }
            _ => line
                .split_whitespace()
                .nth(3)
                .ok_or(io::ErrorKind::InvalidData)?,
        };
        let (host, port) = address.rsplit_once(':').ok_or(io::ErrorKind::InvalidData)?;
        let port: u16 = port.parse().map_err(|_| io::ErrorKind::InvalidData)?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        if port != 0 && matches!(host, "*" | "0.0.0.0" | "::" | "127.0.0.1" | "::1") {
            ports.insert(port);
        }
    }
    Ok(ports)
}

async fn reply(stream: &mut TcpStream, status: u8) -> io::Result<()> {
    stream.write_all(&[5, status, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

async fn destination(stream: &mut TcpStream) -> io::Result<(String, u16)> {
    let mut greeting = [0; 2];
    stream.read_exact(&mut greeting).await?;
    if greeting[0] != 5 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut methods = vec![0; usize::from(greeting[1])];
    stream.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        stream.write_all(&[5, 255]).await?;
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    stream.write_all(&[5, 0]).await?;
    let mut header = [0; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 5 || header[2] != 0 {
        reply(stream, 1).await?;
        return Err(io::ErrorKind::InvalidData.into());
    }
    if header[1] != 1 {
        reply(stream, 7).await?;
        return Err(io::ErrorKind::Unsupported.into());
    }
    let host = match header[3] {
        1 => {
            let mut address = [0; 4];
            stream.read_exact(&mut address).await?;
            Ipv4Addr::from(address).to_string()
        }
        3 => {
            let length = stream.read_u8().await?;
            let mut name = vec![0; usize::from(length)];
            stream.read_exact(&mut name).await?;
            match String::from_utf8(name) {
                Ok(name) if !name.is_empty() && !name.contains('\0') => name,
                _ => {
                    reply(stream, 8).await?;
                    return Err(io::ErrorKind::InvalidData.into());
                }
            }
        }
        4 => {
            let mut address = [0; 16];
            stream.read_exact(&mut address).await?;
            Ipv6Addr::from(address).to_string()
        }
        _ => {
            reply(stream, 8).await?;
            return Err(io::ErrorKind::Unsupported.into());
        }
    };
    Ok((host, stream.read_u16().await?))
}

async fn forward<H: client::Handler>(
    mut stream: TcpStream,
    origin: SocketAddr,
    session: &client::Handle<H>,
) -> io::Result<()> {
    let channel = tokio::time::timeout(CONNECT_TIMEOUT, async {
        let (host, port) = destination(&mut stream).await?;
        match session
            .channel_open_direct_tcpip(
                host,
                u32::from(port),
                origin.ip().to_string(),
                u32::from(origin.port()),
            )
            .await
        {
            Ok(channel) => Ok(channel),
            Err(error) => {
                let status = match &error {
                    russh::Error::ChannelOpenFailure(
                        ChannelOpenFailure::AdministrativelyProhibited,
                    ) => 2,
                    russh::Error::ChannelOpenFailure(ChannelOpenFailure::ConnectFailed) => 5,
                    _ => 1,
                };
                reply(&mut stream, status).await?;
                Err(io::Error::other(error))
            }
        }
    })
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    reply(&mut stream, 0).await?;
    tokio::io::copy_bidirectional(&mut stream, &mut channel.into_stream()).await?;
    Ok(())
}

#[cfg(test)]
#[path = "russh_socks_tests.rs"]
mod tests;
