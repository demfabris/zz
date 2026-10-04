use std::io;
use std::os::{
    fd::{AsRawFd, OwnedFd},
    unix::net::UnixStream,
};

use mio::{Events, Interest, Poll, Token, Waker, unix::SourceFd};
use zz_protocol::decode_protocol_frame;

use super::*;

#[path = "control_stdio.rs"]
mod control_stdio;

#[path = "tty_input.rs"]
mod tty_input;

#[cfg(test)]
#[path = "event_loop_e19_tests.rs"]
mod e19_tests;

#[cfg(test)]
#[path = "event_loop_e15_tests.rs"]
mod e15_tests;

const LISTENER: Token = Token(0);
const WAKE: Token = Token(1);
const SHUTDOWN_SIGNAL: Token = Token(2);
const CHILD_SIGNAL: Token = Token(3);
const ACCEPT_BURST: usize = 32;
const MAX_PENDING_MESSAGES: usize = 4096;
const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;

struct SignalPipes {
    shutdown: UnixStream,
    child: UnixStream,
    registrations: Vec<signal_hook::SigId>,
}

impl SignalPipes {
    #[allow(
        unsafe_code,
        reason = "clear the signal mask the spawning thread left on the loop thread"
    )]
    fn new(poll: &Poll) -> io::Result<Self> {
        unsafe {
            let empty: libc::sigset_t = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_SETMASK, &raw const empty, std::ptr::null_mut());
        }
        let (shutdown, shutdown_write) = UnixStream::pair()?;
        let (child, child_write) = UnixStream::pair()?;
        shutdown.set_nonblocking(true)?;
        child.set_nonblocking(true)?;
        let mut pipes = Self {
            shutdown,
            child,
            registrations: Vec::new(),
        };
        for signal in [libc::SIGTERM, libc::SIGINT] {
            pipes
                .registrations
                .push(signal_hook::low_level::pipe::register(
                    signal,
                    shutdown_write.try_clone()?,
                )?);
        }
        pipes
            .registrations
            .push(signal_hook::low_level::pipe::register(
                libc::SIGCHLD,
                child_write,
            )?);
        for (stream, token) in [
            (&pipes.shutdown, SHUTDOWN_SIGNAL),
            (&pipes.child, CHILD_SIGNAL),
        ] {
            poll.registry().register(
                &mut SourceFd(&stream.as_raw_fd()),
                token,
                Interest::READABLE,
            )?;
        }
        Ok(pipes)
    }

    fn drain(stream: &mut UnixStream) -> io::Result<usize> {
        let mut count = 0;
        let mut buffer = [0; 256];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => return Ok(count),
                Ok(read) => count += read,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(count),
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for SignalPipes {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}

#[derive(Default)]
enum ShutdownPhase {
    #[default]
    Running,
    Admissions,
    Announcing,
    Writers(Vec<Arc<OutboundMailbox>>),
    Done,
}

pub(super) struct EventLoop {
    poll: Poll,
    events: Events,
    startup: Option<lifecycle::Startup>,
    lifecycle_hooks: hook_queue::LoopHooks,
    lifecycle_shutdown: Option<bool>,
    lifecycle_shutdown_started: bool,
    retired: Vec<(
        Option<Box<connection::Session>>,
        Option<Box<exec::LoopExec>>,
        Option<exec::PreparedExec>,
    )>,
    startup_finished: mpsc::Receiver<()>,
    #[cfg(test)]
    startup_sender: mpsc::Sender<()>,
    waker: Arc<Waker>,
    control_wake: Arc<shard_sink::ControlWake>,
    timers: timers::LoopTimers,
    watchers: watchers::LoopWatchers,
    jobs: jobs::JobRegistry,
    helper_jobs: Vec<(jobs::JobId, Arc<AtomicBool>)>,
    status_client: jobs::StatusClient,
    status_jobs: BTreeMap<u64, jobs::JobId>,
    #[cfg(feature = "agent")]
    agents: agent_inbox::AgentInbox,
    connections: BTreeMap<Token, Box<Connection>>,
    stdio_tokens: BTreeMap<Token, Token>,
    tty_tokens: BTreeMap<Token, Token>,
    inserted_queues: Vec<wait_queue::InsertedTask>,
    completed: mpsc::Receiver<Completion>,
    completion_sender: mpsc::Sender<Completion>,
    turn_tokens: Vec<Token>,
    read_buffer: Box<[u8]>,
    next_token: usize,
    accept_again: bool,
    control_output_deadline: Option<Instant>,
    terminal_request_deadline: Option<Instant>,
    shutdown_started: bool,
    shutdown_phase: ShutdownPhase,
    signal_requested: bool,
    force_requested: bool,
    signals: Option<SignalPipes>,
    #[cfg(test)]
    failure: Option<DaemonError>,
}

#[cfg(test)]
pub(super) struct StartupNotifier {
    sender: mpsc::Sender<()>,
    waker: Arc<Waker>,
}

#[cfg(test)]
impl Drop for StartupNotifier {
    fn drop(&mut self) {
        let _ = self.sender.send(());
        if let Err(error) = self.waker.wake() {
            log::warn!("could not wake the mux loop after startup: {error}");
        }
    }
}

#[derive(Default)]
struct Inbound {
    bytes: Vec<u8>,
    offset: usize,
}

impl Inbound {
    fn next(&mut self) -> Result<Option<ProtocolMessage>, ProtocolError> {
        let buffered = &self.bytes[self.offset..];
        if buffered.len() < 4 {
            return Ok(None);
        }
        let length = u32::from_le_bytes(buffered[..4].try_into().unwrap()) as usize;
        if length > zz_protocol::MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(length));
        }
        if length < 4 {
            return Err(ProtocolError::Truncated);
        }
        let total = length + 4;
        if buffered.len() < total {
            return Ok(None);
        }
        let message = decode_protocol_frame(&buffered[..total])?;
        self.offset += total;
        Ok(Some(message))
    }

    fn compact(&mut self) {
        if self.offset != 0 {
            self.bytes.drain(..self.offset);
            self.offset = 0;
        }
    }

    fn eof(&self) -> Result<(), ProtocolError> {
        if self.bytes.len() == self.offset {
            Ok(())
        } else {
            Err(ProtocolError::Truncated)
        }
    }
}

struct Connection {
    stream: UnixStream,
    inbound: Inbound,
    outbound: Arc<OutboundMailbox>,
    frames: Vec<OutboundFrame>,
    frame_index: usize,
    write_offset: usize,
    writable: bool,
    send_buffer_ready: bool,
    read_closed: bool,
    read_again: bool,
    session: Option<Box<connection::Session>>,
    exec_mode: bool,
    exec: Option<Box<exec::LoopExec>>,
    exec_request: Option<exec::PreparedExec>,
    command: Option<cmdq::CommandItem<()>>,
    queue: Option<cmdq::QueueId>,
    output_wait: bool,
    client: Option<ClientId>,
    pending: VecDeque<(ProtocolMessage, usize)>,
    pending_bytes: usize,
    kind: Option<ClientKind>,
    drain_input: bool,
    hello: Option<ProtocolMessage>,
    terminfo: Option<mpsc::Receiver<()>>,
    initializing: bool,
    busy: bool,
    initialized: bool,
    cancel: Arc<AtomicBool>,
    cleanup_started: bool,
    released: Option<Arc<AtomicBool>>,
    ancillary: bool,
    received_fds: Vec<OwnedFd>,
    stdio: Option<Box<control_stdio::ControlStdio>>,
    tty: Option<Box<tty_input::TtyInput>>,
}

#[cfg(test)]
#[path = "event_loop_e02_tests.rs"]
mod e02_tests;

#[cfg(test)]
#[path = "event_loop_e06_tests.rs"]
mod e06_tests;

#[cfg(test)]
#[path = "event_loop_e21_tests.rs"]
mod e21_tests;

#[cfg(test)]
#[path = "event_loop_e21fix_tests.rs"]
mod e21fix_tests;

#[cfg(test)]
#[path = "hookpanic_tests.rs"]
mod hookpanic_tests;

struct Completion {
    token: Token,
    result: Result<Completed, DaemonError>,
    continuation: Option<cmdq::ContinuationToken>,
}

enum Completed {
    Inserted(Box<wait_queue::InsertedTask>),
    Client(Option<Box<connection::Session>>),
    Exec(Option<Box<exec::LoopExec>>),
    ExecPhase(Box<exec::LoopExec>, exec::PreparedExec),
}

impl EventLoop {
    pub(super) fn empty(shared: &Shared) -> Result<Self, DaemonError> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKE)?);
        shared.accept_wake.install(Arc::clone(&waker));
        shared.helpers.install(Arc::clone(&waker));
        shared.terminal_requests.install(Arc::clone(&waker));
        shared.pipe_jobs.wake.install(Arc::clone(&waker));
        shared.control_wake.install(Arc::clone(&waker));
        let status_client = shared.status.lock().job_client();
        status_client.install(Arc::clone(&waker));
        let (_startup_sender, startup_finished) = mpsc::channel();
        let (completion_sender, completed) = mpsc::channel();
        let timers = timers::LoopTimers::new(shared, &waker);
        let watchers = watchers::LoopWatchers::new(shared);
        shared.watcher_tx.wake.install(Arc::clone(&waker));
        #[cfg(feature = "agent")]
        let agents = agent_inbox::AgentInbox::new(shared);
        #[cfg(feature = "agent")]
        shared.agent_tx.wake.install(Arc::clone(&waker));
        shared.loop_active.store(true, Ordering::Release);
        Ok(Self {
            poll,
            events: Events::with_capacity(128),
            startup: None,
            lifecycle_hooks: hook_queue::LoopHooks::new(),
            lifecycle_shutdown: None,
            lifecycle_shutdown_started: false,
            retired: Vec::new(),
            startup_finished,
            #[cfg(test)]
            startup_sender: _startup_sender,
            waker,
            control_wake: Arc::clone(&shared.control_wake),
            timers,
            watchers,
            jobs: jobs::JobRegistry::default(),
            helper_jobs: Vec::new(),
            status_client,
            status_jobs: BTreeMap::new(),
            #[cfg(feature = "agent")]
            agents,
            connections: BTreeMap::new(),
            stdio_tokens: BTreeMap::new(),
            tty_tokens: BTreeMap::new(),
            inserted_queues: Vec::new(),
            completed,
            completion_sender,
            turn_tokens: Vec::new(),
            read_buffer: vec![0; 8192].into_boxed_slice(),
            next_token: 4,
            accept_again: false,
            control_output_deadline: None,
            terminal_request_deadline: None,
            shutdown_started: false,
            shutdown_phase: ShutdownPhase::Running,
            signal_requested: false,
            force_requested: false,
            signals: None,
            #[cfg(test)]
            failure: None,
        })
    }

    pub(super) fn new<T: Transport>(
        listener: &T::Listener,
        shared: &Shared,
    ) -> Result<Self, DaemonError> {
        let mut event_loop = Self::empty(shared)?;
        event_loop.signals = Some(SignalPipes::new(&event_loop.poll)?);
        event_loop.poll.registry().register(
            &mut SourceFd(&listener.raw_fd()),
            LISTENER,
            Interest::READABLE,
        )?;
        Ok(event_loop)
    }

    #[cfg(test)]
    pub(super) fn startup_notifier(&self) -> StartupNotifier {
        StartupNotifier {
            sender: self.startup_sender.clone(),
            waker: Arc::clone(&self.waker),
        }
    }

    pub(super) fn start_startup(
        &mut self,
        shared: &Arc<Shared>,
        load_user_config: bool,
        files: Option<&[PathBuf]>,
        base: Option<&Path>,
    ) -> Result<(), DaemonError> {
        self.startup = Some(lifecycle::Startup::new(
            shared,
            load_user_config,
            files,
            base,
        ));
        crate::transport::wake_loop(&self.waker)?;
        Ok(())
    }

    #[allow(dead_code, reason = "job families migrate in subsequent slices")]
    pub(super) fn register_job(&mut self, launch: jobs::Launch) -> io::Result<jobs::JobId> {
        self.jobs
            .register(self.poll.registry(), &mut self.next_token, launch)
    }

    fn insert(&mut self, descriptor: OwnedFd) -> Result<Token, DaemonError> {
        let stream = UnixStream::from(descriptor);
        stream.set_nonblocking(true)?;
        let token = jobs::allocate_token(&mut self.next_token)?;
        self.poll.registry().register(
            &mut SourceFd(&stream.as_raw_fd()),
            token,
            Interest::READABLE,
        )?;
        let outbound = OutboundMailbox::new();
        *outbound.loop_waker.lock() = Some((Arc::clone(&self.waker), thread::current().id()));
        self.connections.insert(
            token,
            Box::new(Connection {
                stream,
                inbound: Inbound::default(),
                outbound,
                frames: Vec::new(),
                frame_index: 0,
                write_offset: 0,
                writable: false,
                send_buffer_ready: false,
                read_closed: false,
                read_again: false,
                session: None,
                exec_mode: false,
                exec: None,
                exec_request: None,
                command: None,
                queue: None,
                output_wait: false,
                client: None,
                pending: VecDeque::new(),
                pending_bytes: 0,
                kind: None,
                drain_input: false,
                hello: None,
                terminfo: None,
                initializing: false,
                busy: false,
                initialized: false,
                cancel: Arc::new(AtomicBool::new(false)),
                cleanup_started: false,
                released: None,
                ancillary: false,
                received_fds: Vec::new(),
                stdio: None,
                tty: None,
            }),
        );
        Ok(token)
    }

    pub(super) fn run<T: Transport>(
        &mut self,
        listener: &T::Listener,
        shared: &Arc<Shared>,
        initialized: impl FnOnce(),
    ) -> Result<(), DaemonError> {
        let _loop_thread = crate::transport::LoopThread::enter(&self.waker);
        let mut initialized = Some(initialized);
        let mut startup_error = None;
        let mut ready = Vec::new();
        loop {
            while self.startup_finished.try_recv().is_ok() {
                if let Some(initialized) = initialized.take() {
                    initialized();
                }
            }
            if let Some(startup) = &mut self.startup
                && let Some(result) = startup.turn()
            {
                self.startup = None;
                match result {
                    Ok(()) => {
                        if let Some(initialized) = initialized.take() {
                            initialized();
                        }
                    }
                    Err(error) => {
                        startup_error = Some(error);
                        shared.request_shutdown();
                    }
                }
            }
            self.turn(shared)?;
            if self.shutdown_completed() {
                let tokens = self.connections.keys().copied().collect::<Vec<_>>();
                for token in tokens {
                    self.remove(token, shared);
                }
                drop(std::mem::take(&mut self.retired));
                self.turn_lifecycle(shared)?;
                return startup_error.map_or(Ok(()), Err);
            }
            if std::mem::take(&mut self.accept_again) && !self.shutdown_started {
                self.accept_ready::<T>(listener, shared)?;
            }
            self.poll_ready()?;
            ready.clear();
            ready.extend(self.events.iter().map(|event| {
                (
                    event.token(),
                    event.is_readable() || event.is_read_closed(),
                    event.is_writable() || event.is_write_closed(),
                )
            }));
            for &(token, readable, writable) in &ready {
                if token == LISTENER {
                    if !self.shutdown_started {
                        self.accept_ready::<T>(listener, shared)?;
                    }
                } else if token == SHUTDOWN_SIGNAL {
                    let count = SignalPipes::drain(&mut self.signals.as_mut().unwrap().shutdown)?;
                    for _ in 0..count {
                        self.request_signal_shutdown(shared, SIGNAL_SHUTDOWN_GRACE);
                    }
                } else if token == CHILD_SIGNAL {
                    SignalPipes::drain(&mut self.signals.as_mut().unwrap().child)?;
                    self.jobs.child_signal(self.poll.registry());
                } else if self.jobs.contains_token(token) {
                    self.jobs
                        .ready(self.poll.registry(), token, readable, writable);
                } else if let Some(&owner) = self.stdio_tokens.get(&token) {
                    self.stdio_ready(owner, token, shared);
                } else if self.connections.contains_key(&token) {
                    if readable {
                        self.read_ready(token, shared);
                    }
                    if writable
                        && let Some(connection) = self.connections.get_mut(&token)
                        && let Err(error) = connection.write_ready()
                    {
                        log::debug!("client write failed: {error}");
                        connection.outbound.close();
                        self.remove(token, shared);
                    }
                }
            }
            self.ttys_ready(&ready, shared);
        }
    }

    pub(super) fn shutdown_completed(&self) -> bool {
        matches!(self.shutdown_phase, ShutdownPhase::Done)
    }

    pub(super) fn request_signal_shutdown(&mut self, shared: &Arc<Shared>, grace: Duration) {
        if self.signal_requested {
            return self.force_shutdown(shared);
        }
        self.signal_requested = true;
        shared.shutdown_pending.store(true, Ordering::Release);
        self.timers.shutdown_deadline(Some(Instant::now() + grace));
        shared.request_shutdown();
    }

    fn force_shutdown(&mut self, shared: &Arc<Shared>) {
        if self.force_requested || self.shutdown_started {
            return;
        }
        self.force_requested = true;
        self.timers.shutdown_deadline(None);
        shared.force_shutdown();
    }

    fn start_shutdown(&mut self, shared: &Arc<Shared>) {
        self.shutdown_started = true;
        for token in self.tty_tokens.values().copied().collect::<Vec<_>>() {
            self.close_tty(token, true);
        }
        self.status_client.stop();
        self.turn_status_jobs();
        self.jobs.cancel_all(self.poll.registry());
        shared.response_admissions.lock().frozen = true;
        self.shutdown_phase = ShutdownPhase::Admissions;
        self.timers
            .shutdown_deadline(Some(Instant::now() + SHUTDOWN_RESPONSE_TIMEOUT));
    }

    fn advance_shutdown(&mut self, shared: &Arc<Shared>) -> Result<(), DaemonError> {
        let mut due = self.timers.take_shutdown_due();
        if !self.shutdown_started {
            if shared.shutdown_cleanup_complete.load(Ordering::Acquire)
                && !self.lifecycle_hooks.pending()
                && shared.active_shutdown_blockers() == 0
            {
                self.start_shutdown(shared);
                due = false;
            } else if due {
                self.force_shutdown(shared);
            }
        }
        match &self.shutdown_phase {
            ShutdownPhase::Admissions => {
                let active = shared.response_admissions.lock().active;
                if active == 0 || due {
                    if active != 0 {
                        log::warn!(
                            "shutdown proceeding before {active} command responses were admitted"
                        );
                    }
                    self.timers.shutdown_deadline(None);
                    self.shutdown_phase = ShutdownPhase::Announcing;
                    shared.announce_shutdown();
                }
            }
            ShutdownPhase::Announcing => {
                let mut mailboxes = shared
                    .client_writers
                    .lock()
                    .values()
                    .cloned()
                    .collect::<Vec<_>>();
                mailboxes.extend(
                    self.connections
                        .values()
                        .map(|connection| Arc::clone(&connection.outbound)),
                );
                for mailbox in &mailboxes {
                    mailbox.close_after_flush();
                }
                self.shutdown_phase = ShutdownPhase::Writers(mailboxes);
                crate::transport::wake_loop(&self.waker)?;
                self.timers
                    .shutdown_deadline(Some(Instant::now() + SHUTDOWN_WRITER_TIMEOUT));
            }
            ShutdownPhase::Writers(mailboxes) => {
                let drained = mailboxes
                    .iter()
                    .all(|mailbox| mailbox.state.lock().writer_finished);
                if drained || due {
                    if !drained {
                        log::warn!("shutdown proceeding before a client writer drained");
                    }
                    self.timers.shutdown_deadline(None);
                    self.shutdown_phase = ShutdownPhase::Done;
                }
            }
            ShutdownPhase::Running | ShutdownPhase::Done => {}
        }
        Ok(())
    }

    fn poll_timeout(&self, now: Instant) -> Option<Duration> {
        let timer = self
            .timers
            .next(now)
            .map(|deadline| deadline.saturating_duration_since(now));
        let timer = timer
            .into_iter()
            .chain(
                self.jobs
                    .next(now)
                    .map(|deadline| deadline.saturating_duration_since(now)),
            )
            .min();
        let control = self
            .control_output_deadline
            .into_iter()
            .chain(self.terminal_request_deadline)
            .map(|deadline| deadline.saturating_duration_since(now))
            .min();
        match (timer, control) {
            (Some(timer), Some(control)) => Some(timer.min(control)),
            (timer, control) => timer.or(control),
        }
    }

    fn poll_ready(&mut self) -> Result<(), DaemonError> {
        crate::transport::clear_loop_again();
        let timeout = if self.control_wake.park() {
            Some(Duration::ZERO)
        } else {
            self.poll_timeout(Instant::now())
        };
        let polled = self.poll.poll(&mut self.events, timeout);
        self.control_wake.unpark();
        match polled {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::Interrupted => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn accept_ready<T: Transport>(
        &mut self,
        listener: &T::Listener,
        shared: &Arc<Shared>,
    ) -> Result<(), DaemonError> {
        for _ in 0..ACCEPT_BURST {
            if shared.stopping.load(Ordering::Acquire) {
                self.accept_again = false;
                return Ok(());
            }
            match listener.accept_fd() {
                Ok(descriptor) => {
                    let token = self.insert(descriptor)?;
                    self.read_ready(token, shared);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    self.accept_again = false;
                    return Ok(());
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.accept_again = true;
        crate::transport::wake_loop(&self.waker)?;
        Ok(())
    }

    fn read_ready(&mut self, token: Token, shared: &Arc<Shared>) {
        let mut received = 0;
        loop {
            loop {
                let inbound = &mut self.connections.get_mut(&token).unwrap().inbound;
                let offset = inbound.offset;
                let message = inbound.next();
                let bytes = inbound.offset - offset;
                match message {
                    Ok(Some(message)) => {
                        if !self.dispatch(token, shared, message, bytes) {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let connection = self.connections.get_mut(&token).unwrap();
                        if let ProtocolError::VersionMismatch { received, .. } = error {
                            let _ = connection.outbound.enqueue_reliable(
                                &ProtocolMessage::CommandResponse(CommandResponse::Error {
                                    request_id: 0,
                                    error: ServerError::ProtocolMismatch {
                                        client: received,
                                        server: PROTOCOL_VERSION,
                                    },
                                    output: RawText::default(),
                                }),
                            );
                        }
                        log::debug!("client frame rejected: {error}");
                        #[cfg(test)]
                        if self.failure.is_none() {
                            self.failure = Some(error.into());
                        }
                        connection.read_closed = true;
                        self.disconnect(token, shared);
                        return;
                    }
                }
            }
            self.connections.get_mut(&token).unwrap().inbound.compact();

            let Some(connection) = self.connections.get_mut(&token) else {
                return;
            };
            if connection.read_closed {
                return;
            }
            let read = if connection.ancillary {
                control_stdio::receive(
                    &connection.stream,
                    &mut self.read_buffer,
                    &mut connection.received_fds,
                )
            } else {
                connection.stream.read(&mut self.read_buffer)
            };
            match read {
                Ok(0) => {
                    let clean = connection.inbound.eof().is_ok();
                    connection.read_closed = true;
                    connection.drain_input = clean
                        && connection.kind == Some(ClientKind::Interactive)
                        && connection.initialized
                        && !connection.initializing;
                    let drain_input = connection.drain_input;
                    self.drop_stdio(token);
                    self.close_tty(token, false);
                    if !drain_input {
                        self.disconnect(token, shared);
                    }
                    return;
                }
                Ok(read) => {
                    received += read;
                    connection
                        .inbound
                        .bytes
                        .extend_from_slice(&self.read_buffer[..read]);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => return,
                Err(error) => {
                    log::debug!("client read failed: {error}");
                    self.remove(token, shared);
                    return;
                }
            }
            self.connections.get_mut(&token).unwrap().inbound.compact();
            if received >= attach::MAX_BATCHED_WRITE_BYTES {
                self.connections.get_mut(&token).unwrap().read_again = true;
                let _ = crate::transport::wake_loop(&self.waker);
                return;
            }
        }
    }

    fn dispatch(
        &mut self,
        token: Token,
        shared: &Arc<Shared>,
        message: ProtocolMessage,
        bytes: usize,
    ) -> bool {
        let connection = self.connections.get_mut(&token).unwrap();
        match message {
            ProtocolMessage::ControlStdio => {
                self.open_stdio(token);
                return true;
            }
            ProtocolMessage::ControlWrite { bytes, idle, close } => {
                if let Some(stdio) = connection.stdio.as_mut() {
                    stdio.client_write(&bytes, idle, close);
                }
                return true;
            }
            ProtocolMessage::TtyInput { handoff } => {
                self.open_tty(token, handoff);
                return true;
            }
            ProtocolMessage::TtyInputReady { received, pane } => {
                if let Some(tty) = connection.tty.as_mut() {
                    tty.ready(received, pane);
                }
                return true;
            }
            ProtocolMessage::TtyInputRelease { handoff } => {
                self.release_tty(token, handoff);
                return true;
            }
            _ => {}
        }
        if connection.exec_mode {
            return match message {
                ProtocolMessage::GuiResponse(response) => {
                    if let Some(client) = connection.client {
                        shared.complete_gui_request(client, response);
                    }
                    true
                }
                ProtocolMessage::ClientFileResponse(response) => {
                    if let Some(client) = connection.client {
                        shared.complete_client_file(client, response);
                    }
                    true
                }
                message => self.enqueue_pending(token, shared, message, bytes),
            };
        }
        if connection.client.is_none() && !connection.busy {
            if matches!(message, ProtocolMessage::Exec(_)) {
                connection.exec_mode = true;
                return self.enqueue_pending(token, shared, message, bytes);
            }
            connection.prepare_hello(&message);
            connection.start_command();
            connection.busy = true;
            connection.hello = Some(message);
            let _ = crate::transport::wake_loop(&self.waker);
        } else if let Some(client) = connection.client {
            match message {
                ProtocolMessage::GuiResponse(response) => {
                    shared.complete_gui_request(client, response);
                }
                ProtocolMessage::ClientFileResponse(response) => {
                    shared.complete_client_file(client, response);
                }
                message => {
                    if connection.initialized
                        && !connection.busy
                        && connection.command.is_none()
                        && connection.pending.is_empty()
                        && let Some(mut session) = connection.session.take()
                    {
                        connection.start_command();
                        session.start_message(message);
                        match session.run_message(shared, &connection.outbound, true) {
                            connection::MessageProgress::Done => {
                                connection.command.take().unwrap().finish();
                                connection.initializing = session.initializing();
                                connection.session = Some(session);
                            }
                            connection::MessageProgress::Wait => {
                                connection.command.as_ref().unwrap().wait();
                                connection.session = Some(session);
                            }
                            connection::MessageProgress::Output => {
                                connection.command.as_ref().unwrap().wait();
                                connection.output_wait = true;
                                connection.session = Some(session);
                            }
                            connection::MessageProgress::Worker => {
                                connection.busy = true;
                                let outbound = Arc::clone(&connection.outbound);
                                self.execute(token, shared, move |shared| {
                                    session.run_message(shared, &outbound, false);
                                    Ok(Some(session))
                                });
                            }
                            connection::MessageProgress::Ready => unreachable!(),
                        }
                        return true;
                    }
                    return self.enqueue_pending(token, shared, message, bytes);
                }
            }
        } else {
            return self.enqueue_pending(token, shared, message, bytes);
        }
        true
    }

    fn advance_exec(&mut self, token: Token, shared: &Arc<Shared>) {
        let connection = self.connections.get_mut(&token).unwrap();
        if !connection.exec_mode
            || connection.busy
            || connection.read_closed
            || connection.command.is_some()
        {
            return;
        }
        let Some((message, bytes)) = connection.pending.front() else {
            return;
        };
        if let ProtocolMessage::Exec(request) = message
            && connection.client.is_none()
            && !request.commands.is_empty()
            && request.startup_reentry != Some(shared.server_id)
            && !*shared.startup_ready.lock()
            && !shared.stopping.load(Ordering::Acquire)
        {
            return;
        }
        if matches!(
            message,
            ProtocolMessage::Hello(_) | ProtocolMessage::ClientHello(_)
        ) {
            let state = connection.outbound.state.lock();
            if state.queued_bytes != 0 || state.writer_inflight_bytes != 0 {
                return;
            }
        }
        let bytes = *bytes;
        let (message, _) = connection.pending.pop_front().unwrap();
        connection.pending_bytes -= bytes;
        match message {
            ProtocolMessage::Exec(request) => self.run_exec(token, shared, Some(request)),
            first @ (ProtocolMessage::Hello(_) | ProtocolMessage::ClientHello(_)) => {
                connection.prepare_hello(&first);
                let execution = connection.exec.take();
                let exec_client = connection.client;
                connection.exec_mode = false;
                connection.client = None;
                connection.kind = None;
                connection.released = None;
                connection.initialized = false;
                connection.cancel = Arc::new(AtomicBool::new(false));
                connection.start_command();
                connection.busy = true;
                drop(execution);
                if let Some(client) = exec_client {
                    shared.client_writers.lock().remove(&client);
                }
                connection.hello = Some(first);
                let _ = crate::transport::wake_loop(&self.waker);
            }
            _ => {
                if !connection.pending.is_empty() {
                    let _ = crate::transport::wake_loop(&self.waker);
                }
            }
        }
    }

    fn run_exec(
        &mut self,
        token: Token,
        shared: &Arc<Shared>,
        request: Option<zz_protocol::ExecRequest>,
    ) {
        let connection = self.connections.get_mut(&token).unwrap();
        if connection.busy || connection.read_closed {
            return;
        }
        let shutdown_pending = shared.shutdown_pending.load(Ordering::Acquire);
        if request.is_some() && (shared.stopping.load(Ordering::Acquire) || shutdown_pending) {
            if *shared.startup_ready.lock() {
                let _ = connection
                    .outbound
                    .enqueue_reliable(&server_stopping_response(0));
            }
            connection.exec_request = None;
            self.disconnect(token, shared);
            return;
        }
        let (mut execution, mut prepared) = if let Some(mut request) = request {
            connection.start_command();
            let last = request.flags.contains(zz_protocol::ExecFlags::LAST);
            if connection.exec.is_none() && request.commands.is_empty() {
                let outcome = if request
                    .expect_server_id
                    .is_some_and(|id| id != shared.server_id)
                {
                    zz_protocol::ExecOutcome::ServerMismatch
                } else {
                    zz_protocol::ExecOutcome::Ran
                };
                let _ = connection
                    .outbound
                    .enqueue_reliable(&ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                        server_id: shared.server_id,
                        outcome,
                    }));
                connection.command.take().unwrap().finish();
                if last {
                    self.disconnect(token, shared);
                } else if !connection.pending.is_empty() {
                    let _ = crate::transport::wake_loop(&self.waker);
                }
                return;
            }
            let execution = connection.exec.take().or_else(|| {
                exec::LoopExec::register(
                    shared,
                    &mut request,
                    &connection.outbound,
                    &connection.cancel,
                )
                .map(Box::new)
            });
            let Some(mut execution) = execution else {
                let _ = connection
                    .outbound
                    .enqueue_reliable(&server_stopping_response(0));
                connection.command.take().unwrap().finish();
                self.disconnect(token, shared);
                return;
            };
            if connection.client.is_none() {
                connection.client = Some(execution.client);
                connection.kind = Some(ClientKind::Command);
                connection.released = Some(execution.released());
            }
            let prepared = execution.prepare(request);
            (execution, prepared)
        } else {
            if shutdown_pending && connection.output_wait {
                connection.output_wait = false;
                let command = connection.command.as_ref().expect("output continuation");
                if let Some(continuation) = command.wait() {
                    command.resume(continuation);
                }
            }
            if connection
                .command
                .as_ref()
                .is_none_or(|item| item.state() != cmdq::State::Ready)
            {
                return;
            }
            let Some(prepared) = connection.exec_request.take() else {
                return;
            };
            (connection.exec.take().expect("parked Exec"), prepared)
        };
        let last = prepared.last;
        if let Some(resumed) = execution.run(&mut prepared, !shutdown_pending) {
            connection.command.take().unwrap().finish();
            if last && !resumed {
                if execution.can_finish_inline() {
                    execution.finish();
                    drop(execution);
                    self.complete_exec(token, None, shared);
                } else {
                    connection.busy = true;
                    self.execute_work(token, shared, move |_| {
                        drop(execution);
                        Ok(Completed::Exec(None))
                    });
                }
            } else {
                self.complete_exec(token, Some(execution), shared);
            }
            return;
        }
        if prepared.waiting_command {
            connection.command.as_ref().unwrap().wait();
            connection.exec = Some(execution);
            connection.exec_request = Some(prepared);
            return;
        }
        if prepared.ready_on_loop {
            connection.exec = Some(execution);
            connection.exec_request = Some(prepared);
            let _ = crate::transport::wake_loop(&self.waker);
            return;
        }
        if prepared.waiting_output {
            connection.command.as_ref().unwrap().wait();
            connection.output_wait = true;
            connection.exec = Some(execution);
            connection.exec_request = Some(prepared);
            return;
        }
        connection.busy = true;
        self.execute_work(token, shared, move |_| {
            if let Some(resumed) = execution.run(&mut prepared, false) {
                if last && !resumed {
                    drop(execution);
                    Ok(Completed::Exec(None))
                } else {
                    Ok(Completed::Exec(Some(execution)))
                }
            } else {
                Ok(Completed::ExecPhase(execution, prepared))
            }
        });
    }

    fn complete_exec(
        &mut self,
        token: Token,
        execution: Option<Box<exec::LoopExec>>,
        shared: &Arc<Shared>,
    ) {
        let connection = self.connections.get_mut(&token).unwrap();
        connection.busy = false;
        if let Some(command) = connection.command.take() {
            command.finish();
        }
        if let Some(execution) = execution {
            connection.exec = Some(execution);
            if connection.cleanup_started || connection.read_closed {
                self.disconnect(token, shared);
            }
        } else {
            connection.cancel.store(true, Ordering::Release);
            connection.read_closed = true;
            connection.cleanup_started = true;
            connection.pending.clear();
            connection.pending_bytes = 0;
            connection.outbound.close_after_flush();
        }
    }

    fn enqueue_pending(
        &mut self,
        token: Token,
        shared: &Arc<Shared>,
        message: ProtocolMessage,
        bytes: usize,
    ) -> bool {
        let connection = self.connections.get_mut(&token).unwrap();
        if connection.pending.len() >= MAX_PENDING_MESSAGES
            || bytes > MAX_PENDING_BYTES.saturating_sub(connection.pending_bytes)
        {
            log::debug!(
                "client pending admission exceeded: token={token:?} messages={} bytes={} incoming={bytes}",
                connection.pending.len(),
                connection.pending_bytes,
            );
            connection.outbound.close();
            self.remove(token, shared);
            return false;
        }
        if connection.exec_mode
            && !connection.busy
            && connection.command.is_none()
            && !connection.read_closed
            && connection.pending.is_empty()
            && matches!(&message, ProtocolMessage::Exec(request) if connection.client.is_some()
                || request.commands.is_empty()
                || request.startup_reentry == Some(shared.server_id)
                || *shared.startup_ready.lock()
                || shared.stopping.load(Ordering::Acquire))
        {
            let ProtocolMessage::Exec(request) = message else {
                unreachable!();
            };
            self.run_exec(token, shared, Some(request));
            return self
                .connections
                .get(&token)
                .is_some_and(|connection| !connection.read_closed);
        }
        connection.pending_bytes += bytes;
        if connection.pending.capacity() == 0 {
            connection.pending.reserve_exact(1);
        }
        connection.pending.push_back((message, bytes));
        if connection.exec_mode && !connection.busy {
            self.advance_exec(token, shared);
        }
        true
    }

    fn execute(
        &self,
        token: Token,
        shared: &Arc<Shared>,
        work: impl FnOnce(&Arc<Shared>) -> Result<Option<Box<connection::Session>>, DaemonError>
        + Send
        + 'static,
    ) {
        self.execute_work(token, shared, move |shared| {
            work(shared).map(Completed::Client)
        });
    }

    fn execute_work(
        &self,
        token: Token,
        shared: &Arc<Shared>,
        work: impl FnOnce(&Arc<Shared>) -> Result<Completed, DaemonError> + Send + 'static,
    ) {
        let continuation = self
            .connections
            .get(&token)
            .and_then(|connection| connection.command.as_ref())
            .and_then(cmdq::CommandItem::wait);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(shared)))
            .unwrap_or_else(|_| Err(DaemonError::Thread("client execution panicked".to_owned())));
        let _ = self.completion_sender.send(Completion {
            token,
            result,
            continuation,
        });
        let _ = crate::transport::wake_loop(&self.waker);
    }

    fn turn_pipe_jobs(&mut self, shared: &Arc<Shared>) {
        if !shared.pipe_jobs.take_pending() {
            return;
        }
        for request in shared.pipe_jobs.receiver.try_iter().take(64) {
            if let Err(error) = self.register_job(request.into_launch()) {
                log::warn!("pipe job registration failed: {error}");
            }
        }
        if !shared.pipe_jobs.receiver.is_empty() {
            shared.pipe_jobs.notify();
        }
    }

    #[cfg(test)]
    pub(super) fn pipe_test_turn(&mut self, shared: &Arc<Shared>) {
        self.turn_lifecycle(shared).unwrap();
        self.terminal_request_deadline = shared.terminal_requests.turn(shared);
        self.turn_pipe_jobs(shared);
        self.jobs.turn(self.poll.registry(), Instant::now());
        self.jobs.child_signal(self.poll.registry());
        self.poll
            .poll(&mut self.events, Some(Duration::ZERO))
            .unwrap();
        let ready = self
            .events
            .iter()
            .map(|event| {
                (
                    event.token(),
                    event.is_readable() || event.is_read_closed(),
                    event.is_writable() || event.is_write_closed(),
                )
            })
            .collect::<Vec<_>>();
        for (token, readable, writable) in ready {
            self.jobs
                .ready(self.poll.registry(), token, readable, writable);
        }
        shared.drain_background_insertions();
        shared.turn_control_output(false);
        self.watchers.turn(shared);
    }

    #[cfg(test)]
    pub(super) fn pipe_test_stop(&mut self, shared: &Arc<Shared>) {
        self.turn_pipe_jobs(shared);
        self.jobs.cancel_all(self.poll.registry());
        self.jobs.child_signal(self.poll.registry());
    }

    #[cfg(test)]
    pub(super) fn shell_test_turn(&mut self, shared: &Arc<Shared>) {
        self.jobs.child_signal(self.poll.registry());
        self.turn(shared).unwrap();
    }

    fn turn_status_jobs(&mut self) {
        if !self.status_client.take_pending() {
            return;
        }
        self.status_jobs.retain(|_, id| self.jobs.contains(*id));
        let client = self.status_client.clone();
        for request in client.requests() {
            match request {
                jobs::StatusRequest::Launch {
                    serial,
                    command,
                    mut output,
                } => {
                    if self.shutdown_started {
                        output.publish(true, false);
                        continue;
                    }
                    match jobs::launch_status(*command, output)
                        .and_then(|launch| self.register_job(launch))
                    {
                        Ok(id) => {
                            self.status_jobs.insert(serial, id);
                        }
                        Err(error) => log::debug!("status command failed to start: {error}"),
                    }
                }
                jobs::StatusRequest::Cancel(serial) => {
                    if let Some(id) = self.status_jobs.remove(&serial) {
                        self.jobs.cancel(self.poll.registry(), id);
                    }
                }
            }
        }
    }

    fn turn_helpers(&mut self, shared: &Arc<Shared>) {
        if !shared.helpers.take_pending() && self.helper_jobs.is_empty() {
            return;
        }
        for result in shared.helpers.results.try_iter().take(64) {
            shared.apply_helper_result(result);
        }
        if !shared.helpers.results.is_empty() {
            shared.helpers.notify_loop();
        }
        self.helper_jobs.retain(|(id, cancel)| {
            if cancel.load(Ordering::Acquire) {
                self.jobs.cancel(self.poll.registry(), *id);
            }
            self.jobs.contains(*id)
        });
        for request in shared.helpers.job_requests.try_iter().take(64) {
            let helpers::JobRequest {
                mut command,
                limit,
                deadline,
                cancel,
                reply,
            } = request;
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                let _ = reply.send(Err("discovery job cancelled or timed out".into()));
                continue;
            }
            use std::os::unix::process::CommandExt as _;
            command
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(error) => {
                    let _ = reply.send(Err(error.to_string()));
                    continue;
                }
            };
            let descriptors = vec![
                jobs::Descriptor {
                    fd: child.stdout.take().unwrap().into(),
                    read: true,
                    input: None,
                    socket: false,
                },
                jobs::Descriptor {
                    fd: child.stderr.take().unwrap().into(),
                    read: true,
                    input: None,
                    socket: false,
                },
            ];
            let failed = reply.clone();
            let launch = jobs::Launch {
                child,
                descriptors,
                policy: jobs::CompletionPolicy::ChildExit,
                deadline: Some(deadline),
                process_group: true,
                output_limit: Some(limit),
                stream: None,
                pipe: None,
                cancel: None,
                complete: Box::new(move |mut completion| {
                    let result = if let Some(error) = completion.error {
                        Err(error.to_string())
                    } else if completion.cancelled {
                        Err("discovery job cancelled or timed out".into())
                    } else if let Some(status) = completion.status {
                        let stderr = completion.output.pop().unwrap_or_default();
                        let stdout = completion.output.pop().unwrap_or_default();
                        Ok(std::process::Output {
                            status,
                            stdout,
                            stderr,
                        })
                    } else {
                        Err("discovery job exited without status".into())
                    };
                    let _ = reply.send(result);
                }),
            };
            match self.register_job(launch) {
                Ok(id) => self.helper_jobs.push((id, cancel)),
                Err(error) => {
                    let _ = failed.send(Err(error.to_string()));
                }
            }
        }
        if !shared.helpers.job_requests.is_empty() {
            shared.helpers.notify_loop();
        }
    }

    fn turn_lifecycle(&mut self, shared: &Arc<Shared>) -> Result<(), DaemonError> {
        if !shared.lifecycle.take_pending()
            && self.lifecycle_shutdown.is_none()
            && !self.lifecycle_hooks.pending()
        {
            return Ok(());
        }
        if let Some(run_hooks) = shared.lifecycle.turn(shared) {
            self.lifecycle_shutdown = Some(self.lifecycle_shutdown.unwrap_or(false) || run_hooks);
        }
        if let Some(run_hooks) = self.lifecycle_shutdown
            && !self.lifecycle_shutdown_started
        {
            self.lifecycle_shutdown_started = true;
            if run_hooks && !shared.stopping.load(Ordering::Acquire) {
                shared.enter_queue_shutdown_phase();
            }
        }
        self.lifecycle_hooks
            .shutdown_events(shared, shared.lifecycle.take_hooks());
        self.lifecycle_hooks.turn(shared, &self.waker)?;
        if let Some(run_hooks) = self.lifecycle_shutdown
            && !self.lifecycle_hooks.pending()
            && shared.close_shutdown_blockers()
        {
            self.lifecycle_shutdown = None;
            shared.begin_stopping(run_hooks);
            self.lifecycle_hooks
                .shutdown_events(shared, shared.lifecycle.take_hooks());
            self.lifecycle_hooks.turn(shared, &self.waker)?;
        }
        Ok(())
    }

    fn turn(&mut self, shared: &Arc<Shared>) -> Result<(), DaemonError> {
        #[cfg(test)]
        self.jobs.child_signal(self.poll.registry());
        #[cfg(feature = "agent")]
        self.agents.turn(shared);
        if shared.terminal_requests.pending()
            || self
                .terminal_request_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.terminal_request_deadline = shared.terminal_requests.turn(shared);
        }
        self.turn_helpers(shared);
        self.turn_status_jobs();
        self.turn_pipe_jobs(shared);
        self.jobs.turn(self.poll.registry(), Instant::now());
        shared.drain_background_insertions();
        self.watchers.turn(shared);
        self.timers.turn(shared, &self.waker)?;
        shared.turn_control_output(
            self.control_output_deadline
                .is_some_and(|deadline| Instant::now() >= deadline),
        );
        self.control_output_deadline = shared.control_output_deadline();
        drop(std::mem::take(&mut self.retired));
        self.turn_lifecycle(shared)?;
        while let Ok(completion) = self.completed.try_recv() {
            if let Ok(Completed::Inserted(task)) = completion.result {
                self.inserted_queues.push(*task);
                continue;
            }
            let Some(connection) = self.connections.get_mut(&completion.token) else {
                drop(completion.result);
                continue;
            };
            if let Some(continuation) = completion.continuation {
                connection
                    .command
                    .as_ref()
                    .expect("waiting command")
                    .resume(continuation);
            }
            match completion.result {
                Ok(Completed::Inserted(_)) => unreachable!(),
                Ok(Completed::ExecPhase(execution, prepared)) => {
                    connection.busy = false;
                    if connection.read_closed || connection.cleanup_started {
                        drop(prepared);
                        self.complete_exec(completion.token, Some(execution), shared);
                    } else {
                        connection.exec = Some(execution);
                        connection.exec_request = Some(prepared);
                    }
                }
                Ok(Completed::Client(Some(session))) => {
                    connection.busy = false;
                    connection.client = Some(session.client);
                    connection.kind = Some(session.hello.kind);
                    connection.initializing = session.initializing();
                    connection.released = Some(Arc::clone(&session.released));
                    if connection.initialized
                        && !session.message_pending()
                        && let Some(command) = connection.command.take()
                    {
                        command.finish();
                    }
                    if connection.cleanup_started {
                        drop(session);
                    } else {
                        connection.session = Some(session);
                    }
                    if connection.read_closed && !connection.drain_input {
                        self.disconnect(completion.token, shared);
                    }
                }
                Ok(Completed::Exec(execution)) => {
                    self.complete_exec(completion.token, execution, shared);
                }
                result => {
                    if let Err(error) = result {
                        log::debug!("client execution failed: {error}");
                        #[cfg(test)]
                        if self.failure.is_none() {
                            self.failure = Some(error);
                        }
                    }
                    connection.busy = false;
                    connection.read_closed = true;
                    self.disconnect(completion.token, shared);
                }
            }
        }
        self.inserted_queues
            .extend(std::mem::take(&mut *shared.pending_wait_queues.lock()));
        for mut task in std::mem::take(&mut self.inserted_queues) {
            if !task.ready() {
                self.inserted_queues.push(task);
                continue;
            }
            match task.run(true) {
                wait_queue::Progress::Done => {}
                wait_queue::Progress::Waiting => self.inserted_queues.push(task),
                wait_queue::Progress::Worker => {
                    self.execute_work(LISTENER, shared, move |_| {
                        task.run(false);
                        Ok(Completed::Inserted(Box::new(task)))
                    });
                }
                wait_queue::Progress::Ready => unreachable!(),
            }
        }
        let mut tokens = std::mem::take(&mut self.turn_tokens);
        tokens.clear();
        tokens.extend(self.connections.keys().copied());
        for &token in &tokens {
            let connection = self.connections.get_mut(&token).unwrap();
            if connection.read_closed && !connection.drain_input && !connection.busy {
                connection.exec_request = None;
                if let Some(command) = connection.command.take() {
                    command.finish();
                }
                connection.output_wait = false;
            }
            let wait_ready = connection
                .exec_request
                .as_ref()
                .is_some_and(|request| request.waiting_command && request.command_ready())
                || connection
                    .session
                    .as_ref()
                    .is_some_and(|session| session.command_wait_ready());
            if wait_ready
                && let Some(command) = &connection.command
                && let cmdq::State::Waiting(continuation) = command.state()
            {
                command.resume(continuation);
            }
            if connection.hello.is_some() {
                let message = connection.hello.as_ref().unwrap();
                let hello = match message {
                    ProtocolMessage::Hello(hello) => Some(hello.clone().into_client()),
                    ProtocolMessage::ClientHello(hello) => Some(hello.clone()),
                    _ => None,
                };
                let reentry = hello.as_ref().is_some_and(|hello| {
                    hello.kind == ClientKind::Command
                        && hello.capabilities.contains(&format!(
                            "{}{}",
                            crate::STARTUP_REENTRY_CAPABILITY_PREFIX,
                            shared.server_id
                        ))
                });
                if *shared.startup_ready.lock()
                    || reentry
                    || shared.stopping.load(Ordering::Acquire)
                {
                    if let Some(hello) =
                        hello.as_ref().filter(|hello| validate_hello(hello).is_ok())
                        && !crate::status::terminfo_is_warm(&hello.environment)
                        && connection.terminfo.is_none()
                    {
                        let (reply, result) = mpsc::sync_channel(1);
                        shared.helpers.submit(helpers::Task::Terminfo {
                            environment: hello.environment.clone(),
                            jobs: Some(shared.helpers.jobs.clone()),
                            reply,
                        })?;
                        connection.terminfo = Some(result);
                    }
                    if connection.terminfo.as_ref().is_none_or(|result| {
                        !matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty))
                    }) {
                        connection.terminfo = None;
                        let message = connection.hello.take().unwrap();
                        let result = connection::Session::register(
                            shared,
                            message,
                            &connection.outbound,
                            &connection.cancel,
                        )
                        .map(|session| Completed::Client(session.map(Box::new)));
                        let continuation = connection
                            .command
                            .as_ref()
                            .and_then(cmdq::CommandItem::wait);
                        let _ = self.completion_sender.send(Completion {
                            token,
                            result,
                            continuation,
                        });
                        crate::transport::wake_loop(&self.waker)?;
                    }
                }
            }
            if std::mem::take(&mut connection.read_again) {
                self.read_ready(token, shared);
            }
            if self
                .connections
                .get(&token)
                .and_then(|connection| connection.tty.as_ref())
                .is_some_and(|tty| tty.again())
            {
                self.tty_ready(token, shared);
            }
            let Some(connection) = self.connections.get_mut(&token) else {
                continue;
            };
            if connection.exec_mode {
                if connection.exec_request.is_some() {
                    self.run_exec(token, shared, None);
                } else {
                    self.advance_exec(token, shared);
                }
            }
            let Some(connection) = self.connections.get_mut(&token) else {
                continue;
            };
            if !connection.exec_mode && connection.client.is_some() && !connection.initialized {
                let mut pending = std::mem::take(&mut connection.pending);
                connection.pending_bytes = 0;
                while let Some((message, bytes)) = pending.pop_front() {
                    if !self.dispatch(token, shared, message, bytes) {
                        break;
                    }
                }
            }
            let Some(connection) = self.connections.get_mut(&token) else {
                continue;
            };
            if (!connection.read_closed || connection.drain_input)
                && !connection.busy
                && let Some(mut session) = connection.session.take()
            {
                let outbound = Arc::clone(&connection.outbound);
                if !connection.initialized {
                    connection.initialized = true;
                    session.start_initialize(shared, &outbound);
                    connection.initializing = session.initializing();
                } else if !session.message_pending()
                    && let Some((message, bytes)) = connection.pending.pop_front()
                {
                    connection.pending_bytes -= bytes;
                    connection.start_command();
                    session.start_message(message);
                }
                if connection
                    .command
                    .as_ref()
                    .is_some_and(|item| item.state() == cmdq::State::Ready)
                {
                    match session.run_message(shared, &outbound, true) {
                        connection::MessageProgress::Done => {
                            connection.command.take().unwrap().finish();
                            connection.initializing = session.initializing();
                            connection.session = Some(session);
                            if !connection.pending.is_empty() {
                                connection.read_again = true;
                                let _ = crate::transport::wake_loop(&self.waker);
                            }
                        }
                        connection::MessageProgress::Wait => {
                            connection.command.as_ref().unwrap().wait();
                            connection.session = Some(session);
                        }
                        connection::MessageProgress::Output => {
                            connection.command.as_ref().unwrap().wait();
                            connection.output_wait = true;
                            connection.session = Some(session);
                        }
                        connection::MessageProgress::Worker => {
                            connection.busy = true;
                            self.execute(token, shared, move |shared| {
                                session.run_message(shared, &outbound, false);
                                Ok(Some(session))
                            });
                        }
                        connection::MessageProgress::Ready => unreachable!(),
                    }
                } else {
                    connection.session = Some(session);
                }
            }
            let connection = self.connections.get_mut(&token).unwrap();
            if connection.read_closed
                && connection.drain_input
                && !connection.busy
                && connection.pending.is_empty()
                && connection.command.is_none()
            {
                self.disconnect(token, shared);
            }
            let connection = self.connections.get_mut(&token).unwrap();
            #[cfg(target_os = "linux")]
            let control_written = (self.control_output_deadline.is_some()
                && connection.kind == Some(ClientKind::Control))
            .then(|| connection.outbound.state.lock().written_bytes);
            if let Err(error) = connection.write_ready() {
                log::debug!("client write failed: {error}");
                self.remove(token, shared);
                continue;
            }
            if self.connections[&token].stdio.is_some() && !self.pump_stdio(token, shared) {
                continue;
            }
            let connection = self.connections.get_mut(&token).unwrap();
            #[cfg(target_os = "linux")]
            if self.control_output_deadline.is_some()
                && control_written
                    .is_some_and(|before| connection.outbound.state.lock().written_bytes > before)
            {
                crate::transport::wake_loop(&self.waker)?;
            }
            if let Some(client) = connection.client {
                while let Some(pane) = connection.outbound.take_preview_refresh() {
                    shared.send_full(client, pane, &connection.outbound);
                }
            }
            let writing = !connection.frames.is_empty();
            if connection.exec_mode
                && !connection.busy
                && connection.command.is_none()
                && !connection.pending.is_empty()
                && !writing
            {
                let startup_wait = connection.client.is_none()
                    && !*shared.startup_ready.lock()
                    && matches!(connection.pending.front(), Some((ProtocolMessage::Exec(request), _)) if !request.commands.is_empty() && request.startup_reentry != Some(shared.server_id));
                if !startup_wait {
                    let _ = crate::transport::wake_loop(&self.waker);
                }
            }
            if connection.writable != writing {
                let interest = if writing {
                    Interest::READABLE | Interest::WRITABLE
                } else {
                    Interest::READABLE
                };
                self.poll.registry().reregister(
                    &mut SourceFd(&connection.stream.as_raw_fd()),
                    token,
                    interest,
                )?;
                connection.writable = writing;
            }
            let drained = {
                let state = connection.outbound.state.lock();
                state.closed && state.queued_bytes == 0
            };
            if drained
                && !writing
                && connection
                    .stdio
                    .as_ref()
                    .is_none_or(|stdio| !stdio.holds_connection() || connection.read_closed)
            {
                self.remove(token, shared);
            }
        }
        self.turn_tokens = tokens;
        self.advance_shutdown(shared)?;
        Ok(())
    }

    fn disconnect(&mut self, token: Token, shared: &Arc<Shared>) {
        self.close_tty(token, false);
        let Some(connection) = self.connections.get_mut(&token) else {
            return;
        };
        connection.read_closed = true;
        connection.cancel.store(true, Ordering::Release);
        connection.pending.clear();
        connection.pending_bytes = 0;
        if connection.hello.take().is_some() {
            connection.busy = false;
            if let Some(command) = connection.command.take() {
                command.finish();
            }
        }
        if connection.outbound.state.lock().ctrl_collecting == ControlCollection::Attach {
            connection.outbound.close();
        }
        if !connection.cleanup_started
            && let Some(client) = connection.client
        {
            connection.cleanup_started = true;
            if let Some(released) = &connection.released
                && !released.swap(true, Ordering::AcqRel)
            {
                shared.lifecycle.release(client, connection.exec_mode);
            }
            connection.session.take();
            connection.exec.take();
            crate::transport::wake_loop(&self.waker).ok();
        }
        if !connection.busy {
            connection.outbound.close_after_flush();
        }
    }

    fn remove(&mut self, token: Token, shared: &Arc<Shared>) {
        if let Some(mut connection) = self.connections.remove(&token) {
            if let Some(stdio) = connection.stdio.take() {
                self.close_stdio(stdio);
            }
            if let Some(tty) = connection.tty.take() {
                self.tty_tokens.remove(&tty.token);
                tty.close();
            }
            let mut cleanup_pending = false;
            let _ = self
                .poll
                .registry()
                .deregister(&mut SourceFd(&connection.stream.as_raw_fd()));
            connection.read_closed = true;
            connection.cancel.store(true, Ordering::Release);
            if let Some(client) = connection.client {
                let mut writers = shared.client_writers.lock();
                if writers
                    .get(&client)
                    .is_some_and(|writer| Arc::ptr_eq(writer, &connection.outbound))
                {
                    writers.remove(&client);
                }
                if let Some(released) = &connection.released
                    && !released.swap(true, Ordering::AcqRel)
                {
                    shared.lifecycle.release(client, connection.exec_mode);
                    cleanup_pending = true;
                }
            }
            connection.outbound.mark_writer_finished();
            if connection.session.is_some()
                || connection.exec.is_some()
                || connection.exec_request.is_some()
            {
                self.retired.push((
                    connection.session.take(),
                    connection.exec.take(),
                    connection.exec_request.take(),
                ));
                cleanup_pending = true;
            }
            if cleanup_pending {
                crate::transport::wake_loop(&self.waker).ok();
            }
        }
    }
}

#[cfg(test)]
#[path = "event_loop_attach_wake_tests.rs"]
mod attach_wake_tests;

impl Drop for Connection {
    fn drop(&mut self) {
        self.outbound.state.lock().direct_socket = None;
    }
}

impl Connection {
    fn start_command(&mut self) {
        assert!(self.command.is_none());
        let command = cmdq::CommandItem::new(self.queue, ());
        self.queue = Some(command.queue);
        self.command = Some(command);
    }

    fn prepare_hello(&mut self, message: &ProtocolMessage) {
        let _ = self
            .stream
            .set_send_buffer_size(attach::MAX_BATCHED_WRITE_BYTES);
        self.send_buffer_ready = true;
        if matches!(message, ProtocolMessage::Hello(hello) if hello.client.kind == ClientKind::Control)
        {
            self.ancillary = true;
            let socket = self.stream.receive_fd().ok();
            #[cfg(target_vendor = "apple")]
            let socket = socket
                .filter(|socket| rustix::net::sockopt::set_socket_nosigpipe(socket, true).is_ok());
            self.outbound.state.lock().quiet_socket = socket;
        }
        let interactive = match message {
            ProtocolMessage::Hello(hello) => hello.client.kind == ClientKind::Interactive,
            ProtocolMessage::ClientHello(hello) => hello.kind == ClientKind::Interactive,
            _ => false,
        };
        if interactive
            && let ProtocolMessage::Hello(hello) = message
            && hello
                .client
                .capabilities
                .iter()
                .any(|capability| capability == zz_protocol::TTY_INPUT_CAPABILITY)
        {
            self.ancillary = true;
        }
        let socket = interactive.then(|| self.stream.receive_fd().ok()).flatten();
        #[cfg(target_vendor = "apple")]
        let socket = socket
            .filter(|socket| rustix::net::sockopt::set_socket_nosigpipe(socket, true).is_ok());
        self.outbound.state.lock().direct_socket = socket;
    }

    fn write_ready(&mut self) -> io::Result<()> {
        if self.stdio.is_some() {
            return self.write_stdio_ready();
        }
        let mut sent = 0;
        loop {
            if self.frames.is_empty() {
                self.outbound
                    .try_recv_batch(&mut self.frames, attach::MAX_BATCHED_WRITE_BYTES);
                self.frame_index = 0;
                self.write_offset = 0;
                if self.frames.is_empty() {
                    return Ok(());
                }
                if !self.send_buffer_ready
                    && self.frames.iter().map(OutboundFrame::len).sum::<usize>() >= 16 * 1024
                {
                    let _ = self
                        .stream
                        .set_send_buffer_size(attach::MAX_BATCHED_WRITE_BYTES);
                    self.send_buffer_ready = true;
                }
            }
            let mut slices = self.frames[self.frame_index..]
                .iter()
                .map(|frame| io::IoSlice::new(frame))
                .collect::<Vec<_>>();
            slices[0] = io::IoSlice::new(&self.frames[self.frame_index][self.write_offset..]);
            match self.stream.write_vectored(&slices) {
                Ok(0) => return Err(io::Error::from(ErrorKind::WriteZero)),
                Ok(mut written) => {
                    sent += written;
                    self.outbound.record_write(written);
                    while written != 0 {
                        let remaining = self.frames[self.frame_index].len() - self.write_offset;
                        if written < remaining {
                            self.write_offset += written;
                            break;
                        }
                        written -= remaining;
                        self.outbound.complete_inflight_frame(self.frame_index);
                        self.frame_index += 1;
                        self.write_offset = 0;
                    }
                    if self.frame_index == self.frames.len() {
                        self.outbound.recycle_written_batch(&mut self.frames);
                        if self.exec_mode {
                            self.outbound.ready.notify_all();
                        }
                    }
                    if self.output_wait && !exec::output_pending(&self.outbound) {
                        self.output_wait = false;
                        let command = self.command.as_ref().expect("output continuation");
                        if let Some(continuation) = command.wait() {
                            command.resume(continuation);
                        }
                        if let Some((waker, _)) = self.outbound.loop_waker.lock().as_ref() {
                            crate::transport::wake_loop(waker)?;
                        }
                    }
                    if sent >= attach::MAX_BATCHED_WRITE_BYTES {
                        if (!self.frames.is_empty() || self.outbound.state.lock().queued_bytes != 0)
                            && let Some((waker, _)) = self.outbound.loop_waker.lock().as_ref()
                        {
                            crate::transport::wake_loop(waker)?;
                        }
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(test)]
#[path = "event_loop_writestall_tests.rs"]
mod writestall_tests;

#[cfg(test)]
pub(super) fn serve_single(
    stream: impl TransportStream,
    shared: &Arc<Shared>,
) -> Result<(), DaemonError> {
    let mut event_loop = EventLoop::empty(shared)?;
    event_loop.signals = Some(SignalPipes::new(&event_loop.poll)?);
    event_loop.insert(stream.receive_fd()?)?;
    drop(stream);
    serve_test_loop(&mut event_loop, shared)
}

#[cfg(test)]
fn serve_test_loop(event_loop: &mut EventLoop, shared: &Arc<Shared>) -> Result<(), DaemonError> {
    loop {
        event_loop.turn(shared)?;
        if event_loop.connections.is_empty() {
            break;
        }
        event_loop.poll_ready()?;
        let events = event_loop
            .events
            .iter()
            .map(|event| {
                (
                    event.token(),
                    event.is_readable() || event.is_read_closed(),
                    event.is_writable() || event.is_write_closed(),
                )
            })
            .collect::<Vec<_>>();
        for (token, readable, writable) in events {
            if token == CHILD_SIGNAL {
                SignalPipes::drain(&mut event_loop.signals.as_mut().unwrap().child)?;
                event_loop.jobs.child_signal(event_loop.poll.registry());
            } else if event_loop.jobs.contains_token(token) {
                event_loop
                    .jobs
                    .ready(event_loop.poll.registry(), token, readable, writable);
            } else if event_loop.connections.contains_key(&token) {
                if readable {
                    event_loop.read_ready(token, shared);
                }
                if writable && let Some(connection) = event_loop.connections.get_mut(&token) {
                    connection.write_ready()?;
                }
            }
        }
    }
    event_loop.failure.take().map_or(Ok(()), Err)
}

#[cfg(test)]
pub(super) fn join_startup(
    startup: thread::JoinHandle<Result<(), DaemonError>>,
) -> Result<(), DaemonError> {
    startup
        .join()
        .map_err(|_| DaemonError::Thread("daemon startup thread panicked".to_owned()))?
}

#[cfg(test)]
pub(super) fn write_fixture(
    stream: &mut impl TransportStream,
    outbound: &OutboundMailbox,
    shared: &Weak<Shared>,
    client: ClientId,
) {
    let _finished = OutboundWriterGuard(outbound);
    let socket = UnixStream::from(stream.receive_fd().expect("fixture socket"));
    #[cfg(target_vendor = "apple")]
    let _ = rustix::net::sockopt::set_socket_nosigpipe(&socket, true);
    let mut poll = Poll::new().expect("fixture poll");
    let waker = Arc::new(Waker::new(poll.registry(), WAKE).expect("fixture waker"));
    *outbound.loop_waker.lock() = Some((waker, thread::current().id()));
    let mut events = Events::with_capacity(8);
    let mut frames = Vec::new();
    let mut frame_index = 0;
    let mut offset = 0;
    let mut registered = false;
    loop {
        if frames.is_empty() {
            outbound.try_recv_batch(&mut frames, attach::MAX_BATCHED_WRITE_BYTES);
            frame_index = 0;
            offset = 0;
        }
        if frames.is_empty() && outbound.state.lock().closed {
            break;
        }
        if !frames.is_empty() {
            let mut slices = frames[frame_index..]
                .iter()
                .map(|frame| io::IoSlice::new(frame))
                .collect::<Vec<_>>();
            slices[0] = io::IoSlice::new(&frames[frame_index][offset..]);
            let flags = rustix::net::SendFlags::DONTWAIT;
            #[cfg(not(any(target_vendor = "apple", target_os = "redox", target_os = "vita")))]
            let flags = flags | rustix::net::SendFlags::NOSIGNAL;
            match rustix::net::sendmsg(
                &socket,
                &slices,
                &mut rustix::net::SendAncillaryBuffer::default(),
                flags,
            )
            .map_err(io::Error::from)
            {
                Ok(0) => {
                    outbound.close();
                    break;
                }
                Ok(mut written) => {
                    outbound.record_write(written);
                    while written != 0 {
                        let remaining = frames[frame_index].len() - offset;
                        if written < remaining {
                            offset += written;
                            break;
                        }
                        written -= remaining;
                        outbound.complete_inflight_frame(frame_index);
                        frame_index += 1;
                        offset = 0;
                    }
                    if frame_index == frames.len() {
                        outbound.recycle_written_batch(&mut frames);
                        while let Some(pane) = outbound.take_preview_refresh() {
                            if let Some(shared) = shared.upgrade() {
                                shared.send_full(client, pane, outbound);
                            }
                        }
                    }
                    continue;
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(_) => {
                    outbound.close();
                    break;
                }
            }
        }
        if !frames.is_empty() && !registered {
            poll.registry()
                .register(
                    &mut SourceFd(&socket.as_raw_fd()),
                    Token(2),
                    Interest::WRITABLE,
                )
                .unwrap();
            registered = true;
        } else if frames.is_empty() && registered {
            poll.registry()
                .deregister(&mut SourceFd(&socket.as_raw_fd()))
                .unwrap();
            registered = false;
        }
        match poll.poll(&mut events, None) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    let _ = stream.shutdown();
}

#[cfg(test)]
pub(super) fn start_timer_fixture(shared: &Arc<Shared>) -> Result<(), DaemonError> {
    let mut event_loop = EventLoop::empty(shared)?;
    event_loop.signals = Some(SignalPipes::new(&event_loop.poll)?);
    shared.helpers.set_loop_thread(None);
    let shared = Arc::downgrade(shared);
    thread::Builder::new()
        .name("zz-mux-test".to_owned())
        .spawn(move || {
            if let Some(shared) = shared.upgrade() {
                shared.helpers.set_loop_thread(Some(thread::current().id()));
            }
            loop {
                let Some(shared) = shared.upgrade() else {
                    return;
                };
                if shared.stopping.load(Ordering::Acquire) {
                    return;
                }
                if let Err(error) = event_loop.turn(&shared) {
                    log::error!("timer fixture turn failed: {error}");
                    return;
                }
                drop(shared);
                if let Err(error) = event_loop.poll_ready() {
                    log::error!("timer fixture poll failed: {error}");
                    return;
                }
                let ready = event_loop
                    .events
                    .iter()
                    .map(|event| {
                        (
                            event.token(),
                            event.is_readable() || event.is_read_closed(),
                            event.is_writable() || event.is_write_closed(),
                        )
                    })
                    .collect::<Vec<_>>();
                for (token, readable, writable) in ready {
                    if token == CHILD_SIGNAL {
                        let _ = SignalPipes::drain(&mut event_loop.signals.as_mut().unwrap().child);
                        event_loop.jobs.child_signal(event_loop.poll.registry());
                    } else if event_loop.jobs.contains_token(token) {
                        event_loop.jobs.ready(
                            event_loop.poll.registry(),
                            token,
                            readable,
                            writable,
                        );
                    }
                }
            }
        })
        .map(drop)
        .map_err(|error| DaemonError::Thread(error.to_string()))
}

#[cfg(test)]
#[path = "event_loop_io_tests.rs"]
mod io_tests;

#[cfg(test)]
#[path = "event_loop_b2fix_tests.rs"]
mod b2fix_tests;

#[cfg(test)]
#[path = "drainfix_tests.rs"]
mod drainfix_tests;

#[cfg(test)]
#[path = "event_loop_b3_tests.rs"]
mod b3_tests;

#[cfg(test)]
#[path = "event_loop_b3fix_tests.rs"]
mod b3fix_tests;

#[cfg(test)]
#[path = "event_loop_shutdown_tests.rs"]
mod shutdown_tests;

#[cfg(test)]
#[path = "event_loop_b4_tests.rs"]
mod b4_tests;

#[cfg(test)]
#[path = "event_loop_b5_tests.rs"]
mod b5_tests;

#[cfg(test)]
#[path = "event_loop_b5fix_tests.rs"]
mod b5fix_tests;

#[cfg(test)]
#[path = "event_loop_e12_tests.rs"]
mod e12_tests;

#[cfg(test)]
#[path = "event_loop_e04_tests.rs"]
mod e04_tests;

#[cfg(test)]
#[path = "file_commands_e11_tests.rs"]
mod e11_tests;

#[cfg(test)]
#[path = "source_queue_e11fix_tests.rs"]
mod e11fix_tests;
