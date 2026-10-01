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

pub(super) struct EventLoop {
    poll: Poll,
    events: Events,
    startup_finished: mpsc::Receiver<()>,
    startup_sender: mpsc::Sender<()>,
    waker: Arc<Waker>,
    connections: BTreeMap<Token, Connection>,
    completed: mpsc::Receiver<Completion>,
    completion_sender: mpsc::Sender<Completion>,
    next_token: usize,
    handoffs: mpsc::Receiver<Handoff>,
    exec_workers: Arc<std::sync::atomic::AtomicUsize>,
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
    read_closed: bool,
    read_again: bool,
    session: Option<connection::Session>,
    client: Option<ClientId>,
    pending: VecDeque<ProtocolMessage>,
    busy: bool,
    initialized: bool,
    cancel: Arc<AtomicBool>,
    cleanup_started: bool,
    released: Option<Arc<AtomicBool>>,
}

pub(super) struct Handoff {
    pub(super) descriptor: OwnedFd,
    pub(super) first: ProtocolMessage,
    pub(super) buffered: Vec<u8>,
}

struct Completion {
    token: Token,
    result: Result<Option<connection::Session>, DaemonError>,
}

impl EventLoop {
    fn empty(shared: &Shared) -> Result<Self, DaemonError> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKE)?);
        shared.accept_wake.install(Arc::clone(&waker));
        let (startup_sender, startup_finished) = mpsc::channel();
        let (completion_sender, completed) = mpsc::channel();
        let (handoff_sender, handoffs) = mpsc::channel();
        *shared.loop_handoffs.lock() = Some(handoff_sender);
        Ok(Self {
            poll,
            events: Events::with_capacity(128),
            startup_finished,
            startup_sender,
            waker,
            connections: BTreeMap::new(),
            completed,
            completion_sender,
            next_token: 2,
            handoffs,
            exec_workers: Arc::default(),
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
        let _ = stream.set_send_buffer_size(attach::MAX_BATCHED_WRITE_BYTES);
        let token = Token(self.next_token);
        self.next_token += 1;
        self.poll.registry().register(
            &mut SourceFd(&stream.as_raw_fd()),
            token,
            Interest::READABLE,
        )?;
        let outbound = OutboundMailbox::new();
        *outbound.loop_waker.lock() = Some(Arc::clone(&self.waker));
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
                read_closed: false,
                read_again: false,
                session: None,
                client: None,
                pending: VecDeque::new(),
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
            self.poll_ready()?;
            let events = self
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
                if token == LISTENER {
                    if !self.shutdown_started {
                        self.accept_ready::<T>(listener)?;
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
        match self.poll.poll(&mut self.events, None) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::Interrupted => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn accept_ready<T: Transport>(&mut self, listener: &T::Listener) -> Result<(), DaemonError> {
        loop {
            match listener.accept() {
                Ok(stream) => {
                    self.insert(stream.receive_fd()?)?;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn read_ready(&mut self, token: Token, shared: &Arc<Shared>) {
        let mut scratch = [0; 8192];
        let mut received = 0;
        loop {
            let Some(connection) = self.connections.get_mut(&token) else {
                return;
            };
            if connection.read_closed {
                return;
            }
            let read = connection.stream.read(&mut scratch);
            match read {
                Ok(0) => {
                    if let Err(error) = connection.inbound.eof() {
                        log::debug!("truncated client input: {error}");
                    }
                    connection.read_closed = true;
                    self.disconnect(token, shared);
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
            loop {
                let message = self.connections.get_mut(&token).unwrap().inbound.next();
                match message {
                    Ok(Some(message)) => {
                        if !self.dispatch(token, shared, message) {
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
            if received >= attach::MAX_BATCHED_WRITE_BYTES {
                self.connections.get_mut(&token).unwrap().read_again = true;
                let _ = self.waker.wake();
                return;
            }
        }
    }

    fn dispatch(&mut self, token: Token, shared: &Arc<Shared>, message: ProtocolMessage) -> bool {
        let connection = self.connections.get_mut(&token).unwrap();
        if connection.client.is_none() && !connection.busy {
            if let ProtocolMessage::Exec(request) = message {
                let mut connection = self.connections.remove(&token).unwrap();
                let _ = self
                    .poll
                    .registry()
                    .deregister(&mut SourceFd(&connection.stream.as_raw_fd()));
                connection.inbound.compact();
                let stream = BufferedStream {
                    stream: connection.stream,
                    buffered: Arc::new(Mutex::new(io::Cursor::new(connection.inbound.bytes))),
                };
                let shared = Arc::clone(shared);
                let threads = Arc::clone(&shared.connection_threads);
                let _ = stream.stream.set_nonblocking(false);
                self.exec_workers.fetch_add(1, Ordering::AcqRel);
                let finished = ExecNotifier {
                    workers: Arc::clone(&self.exec_workers),
                    waker: Arc::clone(&self.waker),
                };
                if let Err(error) = threads.run(Box::new(move || {
                    let _finished = finished;
                    if let Err(error) = exec::serve_exec(stream, &shared, request) {
                        log::debug!("exec disconnected: {error}");
                    }
                })) {
                    log::warn!("could not start exec worker: {error}");
                }
                return false;
            }
            connection.busy = true;
            let outbound = Arc::clone(&connection.outbound);
            let cancel = Arc::clone(&connection.cancel);
            self.execute(token, shared, move |shared| {
                connection::Session::register(shared, message, &outbound, &cancel)
            });
        } else if let Some(client) = connection.client {
            match message {
                ProtocolMessage::GuiResponse(response) => {
                    let shared = Arc::clone(shared);
                    let threads = Arc::clone(&shared.connection_threads);
                    let _ = threads.run(Box::new(move || {
                        shared.complete_gui_request(client, response);
                    }));
                }
                ProtocolMessage::ClientFileResponse(response) => {
                    let shared = Arc::clone(shared);
                    let threads = Arc::clone(&shared.connection_threads);
                    let _ = threads.run(Box::new(move || {
                        shared.complete_client_file(client, response);
                    }));
                }
                message => connection.pending.push_back(message),
            }
        } else {
            connection.pending.push_back(message);
        }
        true
    }

    fn execute(
        &self,
        token: Token,
        shared: &Arc<Shared>,
        work: impl FnOnce(&Arc<Shared>) -> Result<Option<connection::Session>, DaemonError>
        + Send
        + 'static,
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
        }
    }

    fn turn(&mut self, shared: &Arc<Shared>) -> Result<(), DaemonError> {
        while let Ok(handoff) = self.handoffs.try_recv() {
            if let Err(error) = self.handoff(handoff, shared) {
                log::debug!("client handoff failed: {error}");
            }
        }
        while let Ok(completion) = self.completed.try_recv() {
            let Some(connection) = self.connections.get_mut(&completion.token) else {
                if let Ok(Some(session)) = completion.result {
                    let threads = Arc::clone(&shared.connection_threads);
                    let _ = threads.run(Box::new(move || drop(session)));
                }
                continue;
            };
            match completion.result {
                Ok(Some(session)) => {
                    connection.busy = false;
                    connection.client = Some(session.client);
                    connection.released = Some(Arc::clone(&session.released));
                    if connection.cleanup_started {
                        let threads = Arc::clone(&shared.connection_threads);
                        let _ = threads.run(Box::new(move || drop(session)));
                    } else {
                        connection.session = Some(session);
                    }
                    if connection.read_closed {
                        self.disconnect(completion.token, shared);
                    }
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
        let tokens = self.connections.keys().copied().collect::<Vec<_>>();
        for token in tokens {
            let connection = self.connections.get_mut(&token).unwrap();
            if std::mem::take(&mut connection.read_again) {
                self.read_ready(token, shared);
            }
            let Some(connection) = self.connections.get_mut(&token) else {
                continue;
            };
            if connection.client.is_some() && !connection.initialized {
                let mut pending = std::mem::take(&mut connection.pending);
                while let Some(message) = pending.pop_front() {
                    self.dispatch(token, shared, message);
                }
            }
            let connection = self.connections.get_mut(&token).unwrap();
            if !connection.read_closed
                && !connection.busy
                && let Some(mut session) = connection.session.take()
            {
                let outbound = Arc::clone(&connection.outbound);
                if !connection.initialized {
                    connection.initialized = true;
                    connection.busy = true;
                    self.execute(token, shared, move |shared| {
                        session.initialize(shared, &outbound);
                        Ok(Some(session))
                    });
                } else if let Some(message) = connection.pending.pop_front() {
                    connection.busy = true;
                    self.execute(token, shared, move |shared| {
                        session.message(shared, &outbound, message);
                        Ok(Some(session))
                    });
                } else {
                    connection.session = Some(session);
                }
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
        Ok(())
    }

    fn handoff(&mut self, handoff: Handoff, shared: &Arc<Shared>) -> Result<(), DaemonError> {
        let token = self.insert(handoff.descriptor)?;
        self.connections.get_mut(&token).unwrap().inbound.bytes = handoff.buffered;
        self.dispatch(token, shared, handoff.first);
        loop {
            match self.connections.get_mut(&token).unwrap().inbound.next() {
                Ok(Some(message)) => {
                    self.dispatch(token, shared, message);
                }
                Ok(None) => break,
                Err(error) => {
                    self.disconnect(token, shared);
                    return Err(error.into());
                }
            }
        }
        self.connections.get_mut(&token).unwrap().inbound.compact();
        Ok(())
    }

    fn disconnect(&mut self, token: Token, shared: &Arc<Shared>) {
        let Some(connection) = self.connections.get_mut(&token) else {
            return;
        };
        connection.read_closed = true;
        connection.cancel.store(true, Ordering::Release);
        connection.pending.clear();
        if connection.cleanup_started {
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
        let released = Arc::clone(connection.released.as_ref().expect("registered cleanup"));
        let _ = threads.run(Box::new(move || {
            if !released.swap(true, Ordering::AcqRel) {
                shared.detach(client);
                shared.unregister(client);
                shared.client_writers.lock().remove(&client);
            }
            if let Some(session) = session {
                drop(session);
            }
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
            let _ = connection.stream.shutdown(std::net::Shutdown::Both);
            connection.outbound.mark_writer_finished();
            if let Some(session) = connection.session.take() {
                let threads = Arc::clone(&shared.connection_threads);
                let _ = threads.run(Box::new(move || drop(session)));
            }
        }
    }
}

impl Connection {
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

struct ExecNotifier {
    workers: Arc<std::sync::atomic::AtomicUsize>,
    waker: Arc<Waker>,
}

impl Drop for ExecNotifier {
    fn drop(&mut self) {
        self.workers.fetch_sub(1, Ordering::AcqRel);
        let _ = self.waker.wake();
    }
}

struct BufferedStream {
    stream: UnixStream,
    buffered: Arc<Mutex<io::Cursor<Vec<u8>>>>,
}

impl Read for BufferedStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let read = self.buffered.lock().read(bytes)?;
        if read != 0 {
            Ok(read)
        } else {
            self.stream.read(bytes)
        }
    }
}

impl Write for BufferedStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.write(bytes)
    }
    fn write_vectored(&mut self, bytes: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.stream.write_vectored(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl TransportStream for BufferedStream {
    fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            stream: self.stream.try_clone()?,
            buffered: Arc::clone(&self.buffered),
        })
    }
    fn receive_fd(&self) -> io::Result<OwnedFd> {
        self.stream.receive_fd()
    }
    fn take_buffered_input(&mut self) -> Vec<u8> {
        let mut buffered = self.buffered.lock();
        let position = buffered.position() as usize;
        let bytes = std::mem::take(buffered.get_mut());
        buffered.set_position(0);
        bytes[position..].to_vec()
    }
    fn read_ready(&self, bytes: &mut [u8]) -> io::Result<usize> {
        let read = self.buffered.lock().read(bytes)?;
        if read != 0 {
            return Ok(read);
        }
        rustix::net::recv(&self.stream, bytes, rustix::net::RecvFlags::DONTWAIT)
            .map(|(read, _)| read)
            .map_err(io::Error::from)
    }
    fn shutdown(&self) -> io::Result<()> {
        self.stream.shutdown(std::net::Shutdown::Both)
    }
    fn set_send_buffer_size(&self, bytes: usize) -> io::Result<()> {
        self.stream.set_send_buffer_size(bytes)
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
pub(super) fn serve_handoff(handoff: Handoff, shared: &Arc<Shared>) -> Result<(), DaemonError> {
    let mut event_loop = EventLoop::empty(shared)?;
    event_loop.handoff(handoff, shared)?;
    serve_test_loop(&mut event_loop, shared)
}

#[cfg(test)]
fn serve_test_loop(event_loop: &mut EventLoop, shared: &Arc<Shared>) -> Result<(), DaemonError> {
    loop {
        event_loop.turn(shared)?;
        if event_loop.connections.is_empty() && event_loop.exec_workers.load(Ordering::Acquire) == 0
        {
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
    *outbound.loop_waker.lock() = Some(waker);
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
