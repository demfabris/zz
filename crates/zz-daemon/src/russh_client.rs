//! In-process ssh attach for iOS.

use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, mpsc as std_mpsc},
    thread,
    time::Duration,
};

use parking_lot::Mutex;
use russh::{
    ChannelMsg, Disconnect,
    client::{self, AuthResult},
    keys::{
        Algorithm, PrivateKey, PrivateKeyWithHashAlg, PublicKey, check_known_hosts_path,
        decode_secret_key, known_hosts::learn_known_hosts_path,
    },
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::{mpsc as tokio_mpsc, oneshot},
};
use zeroize::Zeroizing;

use crate::{
    askpass::{AskpassMode, AskpassPrompt, AskpassReply, SshPrompts},
    endpoint::{
        EndpointError, PROXY_READY_MARKER, REMOTE_DAEMON_TIMEOUT_STATUS, REMOTE_SOCKET_PROBE,
        REMOTE_ZZ_MISSING_STATUS, SshEndpoint, parse_remote_probe_output,
        remote_daemon_start_script, shell_quote,
    },
    ios_keychain::{self, KeychainError},
    transport::TransportStream,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);
const OUTBOUND_QUEUE: usize = 64;
const READ_CHUNK: usize = 32 * 1024;

pub(crate) struct RusshStream {
    to_remote: tokio_mpsc::Sender<Vec<u8>>,
    from_remote: Arc<Mutex<IncomingBytes>>,
}

struct IncomingBytes {
    receiver: std_mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
    cursor: usize,
}

impl Clone for RusshStream {
    fn clone(&self) -> Self {
        Self {
            to_remote: self.to_remote.clone(),
            from_remote: Arc::clone(&self.from_remote),
        }
    }
}

impl Read for RusshStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut incoming = self.from_remote.lock();
        if incoming.cursor >= incoming.pending.len() {
            match incoming.receiver.recv() {
                Ok(chunk) => {
                    incoming.pending = chunk;
                    incoming.cursor = 0;
                }
                Err(std_mpsc::RecvError) => return Ok(0),
            }
        }
        let available = &incoming.pending[incoming.cursor..];
        let count = available.len().min(buffer.len());
        buffer[..count].copy_from_slice(&available[..count]);
        incoming.cursor += count;
        Ok(count)
    }
}

impl Write for RusshStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.to_remote
            .blocking_send(buffer.to_vec())
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl TransportStream for RusshStream {
    fn try_clone(&self) -> io::Result<Self> {
        Ok(self.clone())
    }
}

pub(crate) struct RusshForward {
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl RusshForward {
    pub(crate) fn start(
        endpoint: &SshEndpoint,
        prompts: Option<SshPrompts>,
    ) -> Result<(Self, RusshStream), EndpointError> {
        let target = endpoint.to_string();
        let endpoint = endpoint.clone();
        let (to_remote_tx, to_remote_rx) = tokio_mpsc::channel(OUTBOUND_QUEUE);
        let (from_remote_tx, from_remote_rx) = std_mpsc::channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let stream = RusshStream {
            to_remote: to_remote_tx,
            from_remote: Arc::new(Mutex::new(IncomingBytes {
                receiver: from_remote_rx,
                pending: Vec::new(),
                cursor: 0,
            })),
        };

        let thread = thread::Builder::new()
            .name("zz-russh".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_tx.send(Err(EndpointError::SshFailed {
                            target: endpoint.to_string(),
                            reason: format!("failed to start the ssh runtime: {error}"),
                        }));
                        return;
                    }
                };
                runtime.block_on(run_tunnel(
                    endpoint,
                    prompts,
                    to_remote_rx,
                    from_remote_tx,
                    ready_tx,
                    shutdown_rx,
                ));
            })
            .map_err(|error| EndpointError::SshFailed {
                target: target.clone(),
                reason: format!("failed to spawn the ssh thread: {error}"),
            })?;

        match ready_rx.recv_timeout(HANDSHAKE_TIMEOUT) {
            Ok(Ok(())) => Ok((
                Self {
                    shutdown: Mutex::new(Some(shutdown_tx)),
                    thread: Mutex::new(Some(thread)),
                },
                stream,
            )),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                drop(shutdown_tx);
                Err(EndpointError::SshFailed {
                    target,
                    reason: "timed out establishing the ssh tunnel".to_owned(),
                })
            }
        }
    }

    pub(crate) fn socks_port(&self) -> Option<u16> {
        None
    }

    pub(crate) fn shutdown(&self) {
        if let Some(shutdown) = self.shutdown.lock().take() {
            let _ = shutdown.send(());
        }
    }
}

impl Drop for RusshForward {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(thread) = self.thread.get_mut().take() {
            let _ = thread.join();
        }
    }
}

async fn run_tunnel(
    endpoint: SshEndpoint,
    prompts: Option<SshPrompts>,
    mut to_remote_rx: tokio_mpsc::Receiver<Vec<u8>>,
    from_remote_tx: std_mpsc::Sender<Vec<u8>>,
    ready_tx: std_mpsc::Sender<Result<(), EndpointError>>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let target = endpoint.to_string();
    let established = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        establish(&endpoint, prompts, &from_remote_tx),
    )
    .await
    .unwrap_or_else(|_| {
        Err(EndpointError::SshFailed {
            target: target.clone(),
            reason: "ssh handshake timed out".to_owned(),
        })
    });

    let (session, channel) = match established {
        Ok(pair) => pair,
        Err(error) => {
            let _ = ready_tx.send(Err(error));
            return;
        }
    };
    if ready_tx.send(Ok(())).is_err() {
        let _ = session
            .disconnect(Disconnect::ByApplication, "zz client gone", "")
            .await;
        return;
    }

    let (mut read_half, mut write_half) = tokio::io::split(channel.into_stream());
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown_rx => break,
            outbound = to_remote_rx.recv() => match outbound {
                Some(bytes) => {
                    if write_half.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            incoming = read_half.read(&mut chunk) => match incoming {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if from_remote_tx.send(chunk[..count].to_vec()).is_err() {
                        break;
                    }
                }
            },
        }
    }
    let _ = session
        .disconnect(Disconnect::ByApplication, "zz detaching", "")
        .await;
    log::info!(target: "zz_daemon::russh", "tunnel to {target} closed");
}

type SshSession = client::Handle<TofuHandler>;

async fn establish(
    endpoint: &SshEndpoint,
    prompts: Option<SshPrompts>,
    from_remote_tx: &std_mpsc::Sender<Vec<u8>>,
) -> Result<(SshSession, russh::Channel<client::Msg>), EndpointError> {
    let target = endpoint.to_string();
    let host = endpoint.host.clone();
    let port = endpoint.port.unwrap_or(22);
    let ssh_failed = |reason: String| EndpointError::SshFailed {
        target: target.clone(),
        reason,
    };

    let user = endpoint
        .user
        .clone()
        .or_else(|| std::env::var("USER").ok())
        .ok_or_else(|| ssh_failed("ssh endpoints need an explicit user on iOS".to_owned()))?;

    let directory = ssh_directory().map_err(|error| ssh_failed(error.to_string()))?;
    let config = Arc::new(client::Config::default());
    let handler = TofuHandler {
        host: host.clone(),
        port,
        known_hosts: directory.join("known_hosts"),
    };
    let mut session = client::connect(config, (host.as_str(), port), handler)
        .await
        .map_err(|error| ssh_failed(format!("connecting: {error}")))?;

    authenticate(&mut session, &user, &host, &directory, prompts, &ssh_failed).await?;

    let probed_socket = probe_remote_socket(&mut session, &target).await?;
    let remote_socket = endpoint
        .remote_socket
        .clone()
        .unwrap_or_else(|| probed_socket.clone());
    if endpoint.remote_socket.is_none() {
        ensure_remote_daemon(&mut session, &target, &probed_socket).await?;
    }

    let channel = session
        .channel_open_session()
        .await
        .map_err(|error| ssh_failed(format!("opening the proxy channel: {error}")))?;
    let socket = shell_quote(&remote_socket.to_string_lossy());
    let command = format!(
        "sh -lc {}",
        shell_quote(&format!("exec zz proxy --socket {socket}"))
    );
    channel
        .exec(true, command)
        .await
        .map_err(|error| ssh_failed(format!("starting zz proxy: {error}")))?;

    let channel = await_proxy_marker(channel, from_remote_tx, &target).await?;
    Ok((session, channel))
}

async fn authenticate(
    session: &mut SshSession,
    user: &str,
    host: &str,
    directory: &Path,
    prompts: Option<SshPrompts>,
    ssh_failed: &impl Fn(String) -> EndpointError,
) -> Result<(), EndpointError> {
    let key_path = directory.join("id_ed25519");
    let key = load_or_generate_key(&key_path).map_err(|error| ssh_failed(error.to_string()))?;
    let attempt = session
        .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), None))
        .await
        .map_err(|error| ssh_failed(format!("public-key auth: {error}")))?;
    if matches!(attempt, AuthResult::Success) {
        return Ok(());
    }

    let Some(prompts) = prompts else {
        return Err(ssh_failed(format!(
            "the host rejected the zz identity; install {} in authorized_keys or retry from a \
             window that can prompt for a password",
            key_path.with_extension("pub").display(),
        )));
    };
    // Three tries mirrors OpenSSH before it gives up on a method.
    for _ in 0..3 {
        let prompt = AskpassPrompt::new(AskpassMode::Answer, format!("{user}@{host}'s password: "));
        match prompts.respond(&prompt) {
            AskpassReply::Answer(password) => {
                let attempt = session
                    .authenticate_password(user, password.as_str())
                    .await
                    .map_err(|error| ssh_failed(format!("password auth: {error}")))?;
                if matches!(attempt, AuthResult::Success) {
                    return Ok(());
                }
            }
            AskpassReply::Cancel => {
                return Err(ssh_failed("password prompt cancelled".to_owned()));
            }
        }
    }
    Err(ssh_failed("authentication failed".to_owned()))
}

async fn probe_remote_socket(
    session: &mut SshSession,
    target: &str,
) -> Result<PathBuf, EndpointError> {
    let (status, stdout, stderr) = exec_capture(session, format!("sh -lc {REMOTE_SOCKET_PROBE}"))
        .await
        .map_err(|error| EndpointError::ProbeFailure {
            target: target.to_owned(),
            reason: error.to_string(),
        })?;
    if status != Some(0) {
        return Err(EndpointError::ProbeFailure {
            target: target.to_owned(),
            reason: format!(
                "probe exited with {status:?}: {}",
                String::from_utf8_lossy(&stderr).trim(),
            ),
        });
    }
    parse_remote_probe_output(target, &stdout)
}

async fn ensure_remote_daemon(
    session: &mut SshSession,
    target: &str,
    remote_socket: &Path,
) -> Result<(), EndpointError> {
    let script = shell_quote(&remote_daemon_start_script(remote_socket));
    let (status, _stdout, stderr) = exec_capture(session, format!("sh -lc {script}"))
        .await
        .map_err(|error| EndpointError::SshFailed {
            target: target.to_owned(),
            reason: format!("starting the remote daemon: {error}"),
        })?;
    match status.map(|status| status as i32) {
        Some(0) => Ok(()),
        Some(REMOTE_ZZ_MISSING_STATUS) => Err(EndpointError::RemoteBinaryMissing {
            target: target.to_owned(),
        }),
        Some(REMOTE_DAEMON_TIMEOUT_STATUS) => Err(EndpointError::RemoteDaemonUnavailable {
            target: target.to_owned(),
        }),
        other => Err(EndpointError::SshFailed {
            target: target.to_owned(),
            reason: format!(
                "daemon start exited with {other:?}: {}",
                String::from_utf8_lossy(&stderr).trim(),
            ),
        }),
    }
}

async fn await_proxy_marker(
    mut channel: russh::Channel<client::Msg>,
    from_remote_tx: &std_mpsc::Sender<Vec<u8>>,
    target: &str,
) -> Result<russh::Channel<client::Msg>, EndpointError> {
    let mut scanned = Vec::new();
    let mut stderr = Vec::new();
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => {
                scanned.extend_from_slice(&data);
                if let Some(position) = find_marker(&scanned) {
                    let leftover = scanned.split_off(position + PROXY_READY_MARKER.len());
                    if !leftover.is_empty() {
                        let _ = from_remote_tx.send(leftover);
                    }
                    return Ok(channel);
                }
            }
            ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
            ChannelMsg::ExitStatus { .. } | ChannelMsg::Close | ChannelMsg::Eof => break,
            _ => {}
        }
    }
    Err(EndpointError::SshFailed {
        target: target.to_owned(),
        reason: format!(
            "zz proxy never reported ready: {}",
            String::from_utf8_lossy(&stderr).trim(),
        ),
    })
}

fn find_marker(scanned: &[u8]) -> Option<usize> {
    scanned
        .windows(PROXY_READY_MARKER.len())
        .position(|window| window == PROXY_READY_MARKER)
}

async fn exec_capture(
    session: &mut SshSession,
    command: String,
) -> Result<(Option<u32>, Vec<u8>, Vec<u8>), russh::Error> {
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, command).await?;
    let mut status = None;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
            ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
            ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
            _ => {}
        }
    }
    Ok((status, stdout, stderr))
}

struct TofuHandler {
    host: String,
    port: u16,
    known_hosts: PathBuf,
}

impl client::Handler for TofuHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        match check_known_hosts_path(&self.host, self.port, server_public_key, &self.known_hosts) {
            Ok(true) => Ok(true),
            Ok(false) => {
                if let Err(error) = learn_known_hosts_path(
                    &self.host,
                    self.port,
                    server_public_key,
                    &self.known_hosts,
                ) {
                    log::error!(
                        target: "zz_daemon::russh",
                        "failed to record the host key for {}:{}: {error}",
                        self.host,
                        self.port,
                    );
                    return Ok(false);
                }
                log::warn!(
                    target: "zz_daemon::russh",
                    "trusted {}:{} on first use ({})",
                    self.host,
                    self.port,
                    server_public_key.fingerprint(Default::default()),
                );
                Ok(true)
            }
            Err(error) => {
                log::error!(
                    target: "zz_daemon::russh",
                    "HOST KEY MISMATCH for {}:{} — refusing to connect ({error}); \
                     if the host was reinstalled, remove its line from {}",
                    self.host,
                    self.port,
                    self.known_hosts.display(),
                );
                Ok(false)
            }
        }
    }
}

fn ssh_directory() -> io::Result<PathBuf> {
    let directory = match std::env::var_os("ZZ_IOS_SSH_DIR") {
        Some(directory) => PathBuf::from(directory),
        None => {
            let home = std::env::var_os("HOME")
                .ok_or_else(|| io::Error::other("HOME is unset; cannot store ssh state"))?;
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("zz")
                .join("ssh")
        }
    };
    std::fs::create_dir_all(&directory)?;
    Ok(directory)
}

fn load_or_generate_key(path: &Path) -> io::Result<PrivateKey> {
    let stored = match ios_keychain::load_identity() {
        Ok(stored) => stored,
        Err(KeychainError::Unavailable) => {
            log::warn!(
                target: "zz_daemon::russh",
                "no keychain in this build; keeping the zz identity in {} instead",
                path.display(),
            );
            return file_identity(path);
        }
        Err(error) => {
            return Err(io::Error::other(format!(
                "reading the zz identity from the keychain: {error}"
            )));
        }
    };
    if let Some(encoded) = stored {
        let key = decode_identity(&encoded)?;
        write_public_half(path, &key)?;
        return Ok(key);
    }

    let from_file = read_identity_file(path)?;
    let migrating = from_file.is_some();
    let (key, encoded) = match from_file {
        Some(encoded) => (decode_identity(&encoded)?, encoded),
        None => generate_identity()?,
    };
    ios_keychain::store_identity(&encoded).map_err(|error| {
        io::Error::other(format!("storing the zz identity in the keychain: {error}"))
    })?;
    write_public_half(path, &key)?;
    if migrating {
        match std::fs::remove_file(path) {
            Ok(()) => log::info!(
                target: "zz_daemon::russh",
                "moved the zz identity from {} into the keychain",
                path.display(),
            ),
            Err(error) => log::error!(
                target: "zz_daemon::russh",
                "the zz identity is in the keychain but {} could not be removed ({error}); \
                 delete it by hand",
                path.display(),
            ),
        }
    } else {
        log::info!(
            target: "zz_daemon::russh",
            "generated a new zz identity in the keychain; install {} on your hosts",
            path.with_extension("pub").display(),
        );
    }
    Ok(key)
}

fn file_identity(path: &Path) -> io::Result<PrivateKey> {
    if let Some(encoded) = read_identity_file(path)? {
        let key = decode_identity(&encoded)?;
        write_public_half(path, &key)?;
        return Ok(key);
    }
    let (key, encoded) = generate_identity()?;
    std::fs::write(path, encoded.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    write_public_half(path, &key)?;
    log::info!(
        target: "zz_daemon::russh",
        "generated a new zz identity at {}; install the .pub next to it on your hosts",
        path.display(),
    );
    Ok(key)
}

fn read_identity_file(path: &Path) -> io::Result<Option<Zeroizing<String>>> {
    match std::fs::read_to_string(path) {
        Ok(encoded) => Ok(Some(Zeroizing::new(encoded))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io::Error::other(format!(
            "reading {}: {error}",
            path.display(),
        ))),
    }
}

fn decode_identity(encoded: &str) -> io::Result<PrivateKey> {
    decode_secret_key(encoded, None)
        .map_err(|error| io::Error::other(format!("decoding the zz identity: {error}")))
}

fn generate_identity() -> io::Result<(PrivateKey, Zeroizing<String>)> {
    let key = PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519)
        .map_err(|error| io::Error::other(format!("generating the zz identity: {error}")))?;
    let encoded = key
        .to_openssh(Default::default())
        .map_err(|error| io::Error::other(format!("encoding the zz identity: {error}")))?;
    Ok((key, encoded))
}

fn write_public_half(path: &Path, key: &PrivateKey) -> io::Result<()> {
    let public = key
        .public_key()
        .to_openssh()
        .map_err(|error| io::Error::other(format!("encoding the public half: {error}")))?;
    let line = format!("{public} zz-iphone\n");
    let path = path.with_extension("pub");
    if std::fs::read_to_string(&path).is_ok_and(|current| current == line) {
        return Ok(());
    }
    std::fs::write(&path, line)
}
