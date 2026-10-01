use std::{
    collections::VecDeque,
    io,
    os::fd::{AsFd as _, AsRawFd as _, OwnedFd},
    rc::Rc,
};

use super::*;
use crate::writer::TerminalWriter;

pub(super) struct SignalInbox {
    fd: OwnedFd,
    registrations: Vec<signal_hook_registry::SigId>,
}

impl SignalInbox {
    #[allow(
        unsafe_code,
        reason = "registered handlers only write one byte to a nonblocking pipe"
    )]
    pub fn new() -> io::Result<Self> {
        use rustix::{pipe::PipeFlags, process::Signal};
        let (fd, writer) = rustix::pipe::pipe_with(PipeFlags::NONBLOCK | PipeFlags::CLOEXEC)?;
        let writer = Arc::new(writer);
        let mut inbox = Self {
            fd,
            registrations: Vec::new(),
        };
        for (signal, byte) in [
            (Signal::HUP, b'h'),
            (Signal::INT, b'i'),
            (Signal::TERM, b't'),
            (Signal::WINCH, b'w'),
            (Signal::TSTP, b's'),
            (Signal::CONT, b'c'),
        ] {
            let writer = Arc::clone(&writer);
            let id = unsafe {
                signal_hook_registry::register(signal.as_raw(), move || {
                    let _ = rustix::io::write(&*writer, &[byte]);
                })
            }?;
            inbox.registrations.push(id);
        }
        Ok(inbox)
    }

    #[cfg(test)]
    pub fn wait_for_signal(&self, events: &mpsc::Sender<MainEvent>) -> io::Result<()> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        let mut descriptors = [PollFd::new(&self.fd, PollFlags::IN)];
        let timeout = Timespec {
            tv_sec: 2,
            tv_nsec: 0,
        };
        loop {
            match poll(&mut descriptors, Some(&timeout)) {
                Ok(0) => return Err(io::ErrorKind::TimedOut.into()),
                Ok(_) => return self.read(events),
                Err(rustix::io::Errno::INTR) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub fn read(&self, events: &mpsc::Sender<MainEvent>) -> io::Result<()> {
        let mut bytes = [0; 64];
        match rustix::io::read(&self.fd, &mut bytes) {
            Ok(count) => {
                for byte in &bytes[..count] {
                    let event = match byte {
                        b'w' => MainEvent::Resize,
                        b's' => MainEvent::Suspend,
                        b'c' => MainEvent::Resume,
                        _ => MainEvent::Signal,
                    };
                    let _ = events.send(event);
                }
                Ok(())
            }
            Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

impl Drop for SignalInbox {
    fn drop(&mut self) {
        for id in self.registrations.drain(..) {
            signal_hook_registry::unregister(id);
        }
    }
}

pub(super) struct EventLoop {
    socket: OwnedFd,
    stdin: OwnedFd,
    signals: SignalInbox,
    parser: EventParser,
    escape_deadline: Option<Instant>,
    terminal_events: VecDeque<TerminalEvent>,
    prefer_terminal: bool,
    pending_main: Option<MainEvent>,
    disconnected: bool,
    readable: Vec<rustix::event::FdSetElement>,
    writable: Vec<rustix::event::FdSetElement>,
}

impl EventLoop {
    pub fn new(client: &InteractiveClient) -> io::Result<Self> {
        Ok(Self {
            socket: client.receive_fd()?,
            stdin: io::stdin().as_fd().try_clone_to_owned()?,
            signals: SignalInbox::new()?,
            parser: EventParser::default(),
            escape_deadline: None,
            terminal_events: VecDeque::new(),
            prefer_terminal: false,
            pending_main: None,
            disconnected: false,
            readable: Vec::new(),
            writable: Vec::new(),
        })
    }

    pub fn replace(&mut self, client: &InteractiveClient) -> io::Result<()> {
        self.socket = client.receive_fd()?;
        self.disconnected = false;
        Ok(())
    }

    fn read_terminal(&mut self, escape_time: &AtomicU64) -> io::Result<()> {
        let mut bytes = [0; 4096];
        match rustix::io::read(&self.stdin, &mut bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "terminal input closed",
                ));
            }
            Ok(count) => {
                let mut decoded = Vec::new();
                self.parser.push(&bytes[..count], &mut decoded);
                self.terminal_events.extend(decoded);
                self.escape_deadline = self.parser.has_pending_escape().then(|| {
                    Instant::now() + Duration::from_millis(escape_time.load(Ordering::Relaxed))
                });
            }
            Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    fn expire_escape(&mut self, now: Instant) {
        if self.escape_deadline.is_some_and(|deadline| deadline <= now) {
            let mut decoded = Vec::new();
            self.parser.flush_escape(&mut decoded);
            self.terminal_events.extend(decoded);
            self.escape_deadline = None;
        }
    }

    fn read_protocol(
        &mut self,
        client: &InteractiveClient,
        core: &Mutex<ClientCore>,
        connection: u64,
        events: &mpsc::Sender<MainEvent>,
        frames: &FrameInbox,
        kitty_images: &KittyImageInbox,
        kitty_gate: &AtomicU8,
        read_socket: bool,
    ) {
        if self.disconnected {
            return;
        }
        let started = Instant::now();
        for _ in 0..MAX_COALESCED_EVENTS {
            let received = if read_socket {
                client.try_recv()
            } else {
                client.try_recv_buffered()
            };
            match received {
                Ok(Some(message)) => {
                    forward_protocol_message(
                        core,
                        *message,
                        connection,
                        events,
                        frames,
                        kitty_images,
                        kitty_gate,
                        |outbound| match outbound {
                            Outbound::RequestFull(pane) => client.request_full(pane),
                            Outbound::TreeSync => client.request_tree_sync(),
                        },
                    );
                }
                Ok(None) => break,
                Err(error) => {
                    self.disconnected = true;
                    let _ = events.send(MainEvent::Disconnected {
                        connection,
                        error: error.to_string(),
                    });
                    break;
                }
            }
            if started.elapsed() >= Duration::from_millis(1) {
                break;
            }
        }
    }

    #[allow(
        unsafe_code,
        reason = "select borrows descriptors owned by the loop and terminal writer"
    )]
    fn wait(
        &mut self,
        output: &TerminalWriter,
        timeout: Option<Duration>,
    ) -> io::Result<(bool, bool, bool)> {
        use rustix::event::{
            FdSetElement, FdSetIter, Timespec, fd_set_bound, fd_set_insert, fd_set_num_elements,
            select,
        };
        let output = output.pending_fd();
        let bound = [
            self.stdin.as_raw_fd(),
            self.socket.as_raw_fd(),
            self.signals.fd.as_raw_fd(),
            output.map_or(0, |fd| fd.as_raw_fd()),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
            + 1;
        let elements = fd_set_num_elements(4, bound);
        self.readable.resize(elements, FdSetElement::default());
        self.writable.resize(elements, FdSetElement::default());
        self.readable.fill(FdSetElement::default());
        self.writable.fill(FdSetElement::default());
        if self.terminal_events.is_empty() {
            fd_set_insert(&mut self.readable, self.stdin.as_raw_fd());
        }
        if !self.disconnected {
            fd_set_insert(&mut self.readable, self.socket.as_raw_fd());
        }
        fd_set_insert(&mut self.readable, self.signals.fd.as_raw_fd());
        if let Some(fd) = output {
            fd_set_insert(&mut self.writable, fd.as_raw_fd());
        }
        let timeout = timeout.map(|timeout| Timespec {
            tv_sec: timeout.as_secs().try_into().unwrap_or(i64::MAX),
            tv_nsec: i64::from(timeout.subsec_nanos()),
        });
        let bound = fd_set_bound(&self.readable).max(fd_set_bound(&self.writable));
        match unsafe {
            select(
                bound,
                Some(&mut self.readable),
                Some(&mut self.writable),
                None,
                timeout.as_ref(),
            )
        } {
            Ok(_) => Ok((
                FdSetIter::new(&self.readable).any(|fd| fd == self.stdin.as_raw_fd()),
                FdSetIter::new(&self.readable).any(|fd| fd == self.socket.as_raw_fd()),
                FdSetIter::new(&self.readable).any(|fd| fd == self.signals.fd.as_raw_fd()),
            )),
            Err(rustix::io::Errno::INTR) => Ok((false, false, false)),
            Err(error) => Err(error.into()),
        }
    }

    pub fn receive(
        &mut self,
        wait: BrowserWait,
        incoming: &mpsc::Receiver<MainEvent>,
        events: &mpsc::Sender<MainEvent>,
        client: &InteractiveClient,
        core: &Mutex<ClientCore>,
        connection: u64,
        frames: &FrameInbox,
        kitty_images: &KittyImageInbox,
        kitty_gate: &AtomicU8,
        escape_time: &AtomicU64,
        output: &Rc<std::cell::RefCell<TerminalWriter>>,
    ) -> Result<Option<MainEvent>, String> {
        let now = Instant::now();
        self.expire_escape(now);
        {
            let mut output = output.borrow_mut();
            output.tick(now);
            output.flush().map_err(|error| error.to_string())?;
            if output.take_unblocked() {
                return Ok(Some(MainEvent::Repaint));
            }
        }
        self.read_protocol(
            client,
            core,
            connection,
            events,
            frames,
            kitty_images,
            kitty_gate,
            false,
        );
        if self.pending_main.is_none() {
            self.pending_main = incoming.try_recv().ok();
        }
        let mut timeout = match wait {
            BrowserWait::Blocking => None,
            BrowserWait::Timeout(timeout) => Some(timeout),
        };
        for deadline in [self.escape_deadline, output.borrow().deadline()]
            .into_iter()
            .flatten()
        {
            let remaining = deadline.saturating_duration_since(now);
            timeout = Some(timeout.map_or(remaining, |timeout| timeout.min(remaining)));
        }
        if self.pending_main.is_some() || !self.terminal_events.is_empty() {
            timeout = Some(Duration::ZERO);
        }
        let (terminal_ready, socket_ready, signal_ready) = self
            .wait(&output.borrow(), timeout)
            .map_err(|error| error.to_string())?;
        if signal_ready {
            self.signals
                .read(events)
                .map_err(|error| error.to_string())?;
        }
        if terminal_ready {
            self.read_terminal(escape_time)
                .map_err(|error| error.to_string())?;
        }
        if socket_ready {
            self.read_protocol(
                client,
                core,
                connection,
                events,
                frames,
                kitty_images,
                kitty_gate,
                true,
            );
        }
        self.expire_escape(Instant::now());
        if self.prefer_terminal
            && let Some(event) = self.terminal_events.pop_front()
        {
            self.prefer_terminal = false;
            return Ok(Some(MainEvent::Terminal(Ok(event))));
        }
        if let Some(event) = self
            .pending_main
            .take()
            .or_else(|| incoming.try_recv().ok())
        {
            self.prefer_terminal = true;
            return Ok(Some(event));
        }
        Ok(self
            .terminal_events
            .pop_front()
            .map(|event| MainEvent::Terminal(Ok(event))))
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Write as _, os::unix::net::UnixStream};

    use super::*;

    fn pipe_loop() -> (EventLoop, OwnedFd, UnixStream, OwnedFd) {
        let (stdin, input) = rustix::pipe::pipe_with(
            rustix::pipe::PipeFlags::NONBLOCK | rustix::pipe::PipeFlags::CLOEXEC,
        )
        .unwrap();
        let (socket, peer) = UnixStream::pair().unwrap();
        let (signal_fd, signal_writer) = rustix::pipe::pipe_with(
            rustix::pipe::PipeFlags::NONBLOCK | rustix::pipe::PipeFlags::CLOEXEC,
        )
        .unwrap();
        (
            EventLoop {
                socket: socket.into(),
                stdin,
                signals: SignalInbox {
                    fd: signal_fd,
                    registrations: Vec::new(),
                },
                parser: EventParser::default(),
                escape_deadline: None,
                terminal_events: VecDeque::new(),
                prefer_terminal: false,
                pending_main: None,
                disconnected: false,
                readable: Vec::new(),
                writable: Vec::new(),
            },
            input,
            peer,
            signal_writer,
        )
    }

    #[test]
    fn the_loop_reads_and_parses_tty_input_without_a_relay() {
        let (mut event_loop, input, _peer, _signal) = pipe_loop();
        let output = TerminalWriter::with_sink(Box::new(|_| Ok(())));
        rustix::io::write(input, b"a\x1b").unwrap();
        assert_eq!(
            event_loop.wait(&output, Some(Duration::ZERO)).unwrap(),
            (true, false, false)
        );
        event_loop.read_terminal(&AtomicU64::new(25)).unwrap();
        assert_eq!(event_loop.terminal_events.len(), 1);
        assert!(event_loop.parser.has_pending_escape());
        let deadline = event_loop.escape_deadline.unwrap();
        event_loop.expire_escape(deadline.checked_sub(Duration::from_millis(1)).unwrap());
        assert_eq!(event_loop.terminal_events.len(), 1);
        event_loop.expire_escape(deadline);
        assert_eq!(event_loop.terminal_events.len(), 2);
        assert!(!event_loop.parser.has_pending_escape());
    }

    #[test]
    fn socket_readiness_remains_live_with_pending_tty_keys() {
        let (mut event_loop, input, mut peer, _signal) = pipe_loop();
        let output = TerminalWriter::with_sink(Box::new(|_| Ok(())));
        rustix::io::write(input, b"abc").unwrap();
        event_loop.read_terminal(&AtomicU64::new(25)).unwrap();
        peer.write_all(b"frame").unwrap();
        assert_eq!(
            event_loop.wait(&output, Some(Duration::ZERO)).unwrap(),
            (false, true, false)
        );
        assert_eq!(event_loop.terminal_events.len(), 3);
    }
}
