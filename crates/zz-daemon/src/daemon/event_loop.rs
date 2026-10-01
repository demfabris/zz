use std::io;
use std::os::{
    fd::{AsRawFd, OwnedFd},
    unix::net::UnixStream,
};

use mio::{Events, Interest, Poll, Token, Waker, unix::SourceFd};
use zz_protocol::decode_protocol_frame;

use super::*;

const LISTENER: Token = Token(0);
const WAKE: Token = Token(1);
const ACCEPT_BURST: usize = 32;
const MAX_PENDING_MESSAGES: usize = 4096;
const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct EventLoop {
    poll: Poll,
    events: Events,
    startup_finished: mpsc::Receiver<()>,
    startup_sender: mpsc::Sender<()>,
    waker: Arc<Waker>,
    connections: BTreeMap<Token, Connection>,
    completed: mpsc::Receiver<Completion>,
    completion_sender: mpsc::Sender<Completion>,
    turn_tokens: Vec<Token>,
    next_token: usize,
    accept_again: bool,
    control_output_poll: bool,
    shutdown_started: bool,
    shutdown_complete: Arc<AtomicBool>,
    #[cfg(test)]
    failure: Option<DaemonError>,
}

pub(super) struct StartupNotifier {
    sender: mpsc::Sender<()>,
    waker: Arc<Waker>,
}

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
    client: Option<ClientId>,
    pending: VecDeque<(ProtocolMessage, usize)>,
    pending_bytes: usize,
    kind: Option<ClientKind>,
    drain_input: bool,
    initializing: bool,
    busy: bool,
    initialized: bool,
    cancel: Arc<AtomicBool>,
    cleanup_started: bool,
    released: Option<Arc<AtomicBool>>,
}

struct Completion {
    token: Token,
    result: Result<Completed, DaemonError>,
}

enum Completed {
    Client(Option<Box<connection::Session>>),
    Exec(Option<Box<exec::LoopExec>>),
}

impl EventLoop {
    fn empty(shared: &Shared) -> Result<Self, DaemonError> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKE)?);
        shared.accept_wake.install(Arc::clone(&waker));
        let (startup_sender, startup_finished) = mpsc::channel();
        let (completion_sender, completed) = mpsc::channel();
        shared.loop_active.store(true, Ordering::Release);
        Ok(Self {
            poll,
            events: Events::with_capacity(128),
            startup_finished,
            startup_sender,
            waker,
            connections: BTreeMap::new(),
            completed,
            completion_sender,
            turn_tokens: Vec::new(),
            next_token: 2,
            accept_again: false,
            control_output_poll: false,
            shutdown_started: false,
            shutdown_complete: Arc::default(),
            #[cfg(test)]
            failure: None,
        })
    }

    pub(super) fn new<T: Transport>(
        listener: &T::Listener,
        shared: &Shared,
    ) -> Result<Self, DaemonError> {
        let event_loop = Self::empty(shared)?;
        event_loop.poll.registry().register(
            &mut SourceFd(&listener.raw_fd()),
            LISTENER,
            Interest::READABLE,
        )?;
        Ok(event_loop)
    }

    pub(super) fn startup_notifier(&self) -> StartupNotifier {
        StartupNotifier {
            sender: self.startup_sender.clone(),
            waker: Arc::clone(&self.waker),
        }
    }

    fn insert(&mut self, descriptor: OwnedFd) -> Result<Token, DaemonError> {
        let stream = UnixStream::from(descriptor);
        stream.set_nonblocking(true)?;
        let token = Token(self.next_token);
        self.next_token += 1;
        self.poll.registry().register(
            &mut SourceFd(&stream.as_raw_fd()),
            token,
            Interest::READABLE,
        )?;
        let outbound = OutboundMailbox::new();
        *outbound.loop_waker.lock() = Some((Arc::clone(&self.waker), thread::current().id()));
        self.connections.insert(
            token,
            Connection {
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
                client: None,
                pending: VecDeque::new(),
                pending_bytes: 0,
                kind: None,
                drain_input: false,
                initializing: false,
                busy: false,
                initialized: false,
                cancel: Arc::new(AtomicBool::new(false)),
                cleanup_started: false,
                released: None,
            },
        );
        Ok(token)
    }

    pub(super) fn run<T: Transport>(
        &mut self,
        listener: &T::Listener,
        shared: &Arc<Shared>,
        initialized: impl FnOnce(),
    ) -> Result<(), DaemonError> {
        let mut initialized = Some(initialized);
        let mut ready = Vec::new();
        loop {
            while self.startup_finished.try_recv().is_ok() {
                if let Some(initialized) = initialized.take() {
                    initialized();
                }
            }
            if shared.stopping.load(Ordering::Acquire) {
                self.start_shutdown(shared)?;
                if self.shutdown_completed() {
                    let tokens = self.connections.keys().copied().collect::<Vec<_>>();
                    for token in tokens {
                        self.remove(token, shared);
                    }
                    return Ok(());
                }
            }
            self.turn(shared)?;
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
        }
    }

    pub(super) fn shutdown_completed(&self) -> bool {
        self.shutdown_complete.load(Ordering::Acquire)
    }

    fn start_shutdown(&mut self, shared: &Arc<Shared>) -> Result<(), DaemonError> {
        if self.shutdown_started {
            return Ok(());
        }
        self.shutdown_started = true;
        let shared = Arc::clone(shared);
        let threads = Arc::clone(&shared.connection_threads);
        let complete = Arc::clone(&self.shutdown_complete);
        let waker = Arc::clone(&self.waker);
        threads.run(Box::new(move || {
            shared.request_shutdown_after_blockers();
            shared.freeze_response_admissions_and_wait(SHUTDOWN_RESPONSE_TIMEOUT);
            shared.announce_shutdown();
            shared.drain_client_writers_for_shutdown(SHUTDOWN_WRITER_TIMEOUT);
            complete.store(true, Ordering::Release);
            let _ = waker.wake();
        }))?;
        Ok(())
    }

    fn poll_ready(&mut self) -> Result<(), DaemonError> {
        match self.poll.poll(
            &mut self.events,
            self.control_output_poll.then_some(COPY_PIPE_POLL_INTERVAL),
        ) {
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
        self.waker.wake()?;
        Ok(())
    }

    fn read_ready(&mut self, token: Token, shared: &Arc<Shared>) {
        let mut scratch = [0; 8192];
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
            let read = connection.stream.read(&mut scratch);
            match read {
                Ok(0) => {
                    let clean = connection.inbound.eof().is_ok();
                    connection.read_closed = true;
                    connection.drain_input = clean
                        && connection.kind == Some(ClientKind::Interactive)
                        && connection.initialized
                        && !connection.initializing;
                    if !connection.drain_input {
                        self.disconnect(token, shared);
                    }
                    return;
                }
                Ok(read) => {
                    received += read;
                    connection.inbound.bytes.extend_from_slice(&scratch[..read]);
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
                let _ = self.waker.wake();
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
        if connection.exec_mode {
            return match message {
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
            connection.busy = true;
            let outbound = Arc::clone(&connection.outbound);
            let cancel = Arc::clone(&connection.cancel);
            self.execute(token, shared, move |shared| {
                connection::Session::register(shared, message, &outbound, &cancel)
                    .map(|session| session.map(Box::new))
            });
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
                        && connection.pending.is_empty()
                        && let Some(mut session) = connection.session.take()
                    {
                        let message =
                            session.try_inline_message(shared, &connection.outbound, message);
                        connection.session = Some(session);
                        let Some(message) = message else {
                            return true;
                        };
                        return self.enqueue_pending(token, shared, message, bytes);
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
        if !connection.exec_mode || connection.busy || connection.read_closed {
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
            ProtocolMessage::Exec(request) => self.run_exec(token, shared, request),
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
                connection.busy = true;
                let outbound = Arc::clone(&connection.outbound);
                let cancel = Arc::clone(&connection.cancel);
                self.execute(token, shared, move |shared| {
                    drop(execution);
                    if let Some(client) = exec_client {
                        shared.client_writers.lock().remove(&client);
                    }
                    connection::Session::register(shared, first, &outbound, &cancel)
                        .map(|session| session.map(Box::new))
                });
            }
            _ => {
                if !connection.pending.is_empty() {
                    let _ = self.waker.wake();
                }
            }
        }
    }

    fn run_exec(
        &mut self,
        token: Token,
        shared: &Arc<Shared>,
        mut request: zz_protocol::ExecRequest,
    ) {
        let connection = self.connections.get_mut(&token).unwrap();
        if shared.stopping.load(Ordering::Acquire)
            || shared.shutdown_pending.load(Ordering::Acquire)
        {
            if *shared.startup_ready.lock() {
                let _ = connection
                    .outbound
                    .enqueue_reliable(&server_stopping_response(0));
            }
            self.disconnect(token, shared);
            return;
        }
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
            if last {
                self.disconnect(token, shared);
            } else if !connection.pending.is_empty() {
                let _ = self.waker.wake();
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
            self.disconnect(token, shared);
            return;
        };
        if connection.client.is_none() {
            connection.client = Some(execution.client);
            connection.kind = Some(ClientKind::Command);
            connection.released = Some(execution.released());
        }
        let mut prepared = execution.prepare(request);
        if prepared.inline
            && let Some(resumed) = execution.run(&mut prepared, true)
        {
            if last && !resumed {
                if execution.can_finish_inline() {
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
        connection.busy = true;
        self.execute_work(token, shared, move |_| {
            let resumed = execution
                .run(&mut prepared, false)
                .expect("worker finishes Exec");
            if last && !resumed {
                drop(execution);
                Ok(Completed::Exec(None))
            } else {
                Ok(Completed::Exec(Some(execution)))
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
            self.run_exec(token, shared, request);
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
        let shared = Arc::clone(shared);
        let sender = self.completion_sender.clone();
        let waker = Arc::clone(&self.waker);
        let threads = Arc::clone(&shared.connection_threads);
        if let Err(error) = threads.run(Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&shared)))
                .unwrap_or_else(|_| {
                    Err(DaemonError::Thread("client execution panicked".to_owned()))
                });
            let _ = sender.send(Completion { token, result });
            let _ = waker.wake();
        })) {
            log::error!("could not start client execution: {error}");
            let _ = self.completion_sender.send(Completion {
                token,
                result: Err(error.into()),
            });
            let _ = self.waker.wake();
        }
    }

    fn turn(&mut self, shared: &Arc<Shared>) -> Result<(), DaemonError> {
        self.control_output_poll = shared.start_ready_control_output_readers();
        while let Ok(completion) = self.completed.try_recv() {
            let Some(connection) = self.connections.get_mut(&completion.token) else {
                if matches!(
                    &completion.result,
                    Ok(Completed::Client(Some(_)) | Completed::Exec(Some(_)))
                ) {
                    let threads = Arc::clone(&shared.connection_threads);
                    let _ = threads.run(Box::new(move || drop(completion.result)));
                }
                continue;
            };
            match completion.result {
                Ok(Completed::Client(Some(session))) => {
                    connection.busy = false;
                    connection.client = Some(session.client);
                    connection.kind = Some(session.hello.kind);
                    connection.initializing = false;
                    connection.released = Some(Arc::clone(&session.released));
                    if connection.cleanup_started {
                        let threads = Arc::clone(&shared.connection_threads);
                        let _ = threads.run(Box::new(move || drop(session)));
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
        let mut tokens = std::mem::take(&mut self.turn_tokens);
        tokens.clear();
        tokens.extend(self.connections.keys().copied());
        for &token in &tokens {
            let connection = self.connections.get_mut(&token).unwrap();
            if std::mem::take(&mut connection.read_again) {
                self.read_ready(token, shared);
            }
            let Some(connection) = self.connections.get_mut(&token) else {
                continue;
            };
            if connection.exec_mode {
                self.advance_exec(token, shared);
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
                    connection.initializing = true;
                    connection.busy = true;
                    self.execute(token, shared, move |shared| {
                        session.initialize(shared, &outbound);
                        Ok(Some(session))
                    });
                } else if let Some((message, bytes)) = connection.pending.pop_front() {
                    connection.pending_bytes -= bytes;
                    if let Some(message) = session.try_inline_message(shared, &outbound, message) {
                        connection.busy = true;
                        self.execute(token, shared, move |shared| {
                            session.message(shared, &outbound, message);
                            Ok(Some(session))
                        });
                    } else {
                        connection.session = Some(session);
                        if !connection.pending.is_empty() {
                            connection.read_again = true;
                            let _ = self.waker.wake();
                        }
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
            {
                self.disconnect(token, shared);
            }
            let connection = self.connections.get_mut(&token).unwrap();
            if let Err(error) = connection.write_ready() {
                log::debug!("client write failed: {error}");
                self.remove(token, shared);
                continue;
            }
            let connection = self.connections.get_mut(&token).unwrap();
            if let Some(client) = connection.client {
                while let Some(pane) = connection.outbound.take_preview_refresh() {
                    let shared = Arc::clone(shared);
                    let outbound = Arc::clone(&connection.outbound);
                    let threads = Arc::clone(&shared.connection_threads);
                    let _ =
                        threads.run(Box::new(move || shared.send_full(client, pane, &outbound)));
                }
            }
            let writing = !connection.frames.is_empty();
            if connection.exec_mode
                && !connection.busy
                && !connection.pending.is_empty()
                && !writing
            {
                let startup_wait = connection.client.is_none()
                    && !*shared.startup_ready.lock()
                    && matches!(connection.pending.front(), Some((ProtocolMessage::Exec(request), _)) if !request.commands.is_empty() && request.startup_reentry != Some(shared.server_id));
                if !startup_wait {
                    let _ = self.waker.wake();
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
            if drained && !writing {
                self.remove(token, shared);
            }
        }
        self.turn_tokens = tokens;
        Ok(())
    }

    fn disconnect(&mut self, token: Token, shared: &Arc<Shared>) {
        let Some(connection) = self.connections.get_mut(&token) else {
            return;
        };
        connection.read_closed = true;
        connection.cancel.store(true, Ordering::Release);
        connection.pending.clear();
        connection.pending_bytes = 0;
        if connection.outbound.state.lock().ctrl_collecting == ControlCollection::Attach {
            connection.outbound.close();
        }
        if connection.cleanup_started {
            if let Some(execution) = connection.exec.take() {
                let threads = Arc::clone(&shared.connection_threads);
                let outbound = Arc::clone(&connection.outbound);
                let _ = threads.run(Box::new(move || {
                    drop(execution);
                    outbound.close_after_flush();
                }));
            }
            return;
        }
        let Some(client) = connection.client else {
            if !connection.busy {
                connection.outbound.close_after_flush();
            }
            return;
        };
        connection.cleanup_started = true;
        let outbound = Arc::clone(&connection.outbound);
        let shared = Arc::clone(shared);
        let threads = Arc::clone(&shared.connection_threads);
        let waker = Arc::clone(&self.waker);
        let session = connection.session.take();
        let execution = connection.exec.take();
        let exec_mode = connection.exec_mode;
        let released = Arc::clone(connection.released.as_ref().expect("registered cleanup"));
        let _ = threads.run(Box::new(move || {
            if !released.swap(true, Ordering::AcqRel) {
                if !exec_mode || !detach_is_inert(&shared.inner.lock(), client) {
                    shared.detach(client);
                }
                shared.unregister(client);
                if !exec_mode {
                    shared.client_writers.lock().remove(&client);
                }
            }
            drop(session);
            drop(execution);
            outbound.close_after_flush();
            let _ = waker.wake();
        }));
    }

    fn remove(&mut self, token: Token, shared: &Arc<Shared>) {
        self.disconnect(token, shared);
        if let Some(mut connection) = self.connections.remove(&token) {
            let _ = self
                .poll
                .registry()
                .deregister(&mut SourceFd(&connection.stream.as_raw_fd()));
            connection.outbound.mark_writer_finished();
            if connection.exec_mode
                && let Some(client) = connection.client
            {
                let mut writers = shared.client_writers.lock();
                if writers
                    .get(&client)
                    .is_some_and(|writer| Arc::ptr_eq(writer, &connection.outbound))
                {
                    writers.remove(&client);
                }
            }
            if let Some(session) = connection.session.take() {
                let threads = Arc::clone(&shared.connection_threads);
                let _ = threads.run(Box::new(move || drop(session)));
            }
        }
    }
}

impl Connection {
    fn prepare_hello(&mut self, message: &ProtocolMessage) {
        let _ = self
            .stream
            .set_send_buffer_size(attach::MAX_BATCHED_WRITE_BYTES);
        self.send_buffer_ready = true;
        if matches!(message, ProtocolMessage::Hello(hello) if hello.client.kind == ClientKind::Control)
        {
            let socket = self.stream.receive_fd().ok();
            #[cfg(target_vendor = "apple")]
            let socket = socket
                .filter(|socket| rustix::net::sockopt::set_socket_nosigpipe(socket, true).is_ok());
            self.outbound.state.lock().quiet_socket = socket;
        }
    }

    fn write_ready(&mut self) -> io::Result<()> {
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
                    if sent >= attach::MAX_BATCHED_WRITE_BYTES {
                        self.outbound.notify_one();
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
pub(super) fn serve_single(
    stream: impl TransportStream,
    shared: &Arc<Shared>,
) -> Result<(), DaemonError> {
    let mut event_loop = EventLoop::empty(shared)?;
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
            .map(mio::event::Event::token)
            .collect::<Vec<_>>();
        for token in events {
            if event_loop.connections.contains_key(&token) {
                event_loop.read_ready(token, shared);
            }
        }
    }
    event_loop.failure.take().map_or(Ok(()), Err)
}

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
#[path = "event_loop_io_tests.rs"]
mod io_tests;

#[cfg(test)]
#[path = "event_loop_b2fix_tests.rs"]
mod b2fix_tests;

#[cfg(test)]
#[path = "event_loop_b3_tests.rs"]
mod b3_tests;

#[cfg(test)]
#[path = "event_loop_b3fix_tests.rs"]
mod b3fix_tests;
