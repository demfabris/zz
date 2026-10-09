use std::{
    collections::VecDeque,
    io,
    os::fd::{AsFd as _, AsRawFd as _, OwnedFd},
    rc::Rc,
};

#[cfg(target_os = "macos")]
use std::os::fd::RawFd;

use super::*;
use crate::writer::TerminalWriter;

pub(super) struct SignalInbox {
    fd: OwnedFd,
    registrations: Vec<signal_hook_registry::SigId>,
}

fn nonblocking_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let (read, write) = rustix::pipe::pipe()?;
    for fd in [&read, &write] {
        rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::CLOEXEC)?;
        let flags = rustix::fs::fcntl_getfl(fd)?;
        rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)?;
    }
    Ok((read, write))
}

impl SignalInbox {
    #[allow(
        unsafe_code,
        reason = "registered handlers only write one byte to a nonblocking pipe"
    )]
    pub fn new() -> io::Result<Self> {
        use rustix::process::Signal;
        let (fd, writer) = nonblocking_pipe()?;
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

impl Drop for EventLoop {
    fn drop(&mut self) {
        self.release_tty();
    }
}

impl Drop for SignalInbox {
    fn drop(&mut self) {
        for id in self.registrations.drain(..) {
            signal_hook_registry::unregister(id);
        }
    }
}

#[cfg(target_os = "macos")]
struct Queue {
    fd: OwnedFd,
    armed: [Option<(RawFd, bool)>; 4],
}

#[cfg(target_os = "macos")]
impl Queue {
    fn new() -> io::Result<Self> {
        let fd = rustix::event::kqueue::kqueue()?;
        rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
        Ok(Self {
            fd,
            armed: [None; 4],
        })
    }

    #[allow(
        unsafe_code,
        reason = "kevent registers descriptors the loop and terminal writer keep open"
    )]
    fn wait(
        &mut self,
        wanted: [Option<RawFd>; 4],
        timeout: Option<Duration>,
    ) -> io::Result<Option<[bool; 4]>> {
        use rustix::event::kqueue::{Event, EventFilter, EventFlags, kevent};
        let filter = |slot: usize, fd: RawFd| {
            if slot == 3 {
                EventFilter::Write(fd)
            } else {
                EventFilter::Read(fd)
            }
        };
        let mut changes = [Event::new(
            EventFilter::Read(0),
            EventFlags::empty(),
            std::ptr::null_mut(),
        ); 8];
        let mut count = 0;
        for (slot, want) in wanted.into_iter().enumerate() {
            let change = match (self.armed[slot], want) {
                (Some((fd, true)), Some(want)) if fd == want => None,
                (Some((fd, false)), Some(want)) if fd == want => {
                    Some((filter(slot, fd), EventFlags::ENABLE))
                }
                (_, Some(want)) => Some((filter(slot, want), EventFlags::ADD | EventFlags::ENABLE)),
                (Some((fd, true)), None) => Some((filter(slot, fd), EventFlags::DISABLE)),
                (_, None) => None,
            };
            if let Some((event_filter, flags)) = change {
                changes[count] = Event::new(event_filter, flags, std::ptr::null_mut());
                count += 1;
                self.armed[slot] = match want {
                    Some(fd) => Some((fd, true)),
                    None => self.armed[slot].map(|(fd, _)| (fd, false)),
                };
            }
        }
        let mut events = [std::mem::MaybeUninit::<Event>::uninit(); 8];
        let ready = match unsafe { kevent(&self.fd, &changes[..count], &mut events, timeout) } {
            Ok((ready, _)) => ready,
            Err(rustix::io::Errno::INTR) => return Ok(Some([false; 4])),
            Err(error) => return Err(error.into()),
        };
        let mut result = [false; 4];
        for event in ready.iter() {
            if event.flags().contains(EventFlags::ERROR) {
                if event.data() != 0 {
                    return Ok(None);
                }
                continue;
            }
            let (fd, write) = match event.filter() {
                EventFilter::Read(fd) => (fd, false),
                EventFilter::Write(fd) => (fd, true),
                _ => continue,
            };
            for (slot, armed) in self.armed.iter().enumerate() {
                if *armed == Some((fd, true)) && (slot == 3) == write {
                    result[slot] = true;
                }
            }
        }
        Ok(Some(result))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Handoff {
    Own,
    Requested,
    Daemon,
    Releasing,
}

const RELEASE_WAIT: Duration = Duration::from_secs(1);
const GRAPHICS_REPLY_WAIT: Duration = Duration::from_secs(1);
const CLIPBOARD_REPLY_WAIT: Duration = Duration::from_secs(5);

pub(super) struct EventLoop {
    client: Option<Arc<InteractiveClient>>,
    tty: Handoff,
    tty_handoff: u64,
    tty_received: u64,
    tty_reported: Option<(u64, Option<PaneId>)>,
    escape_ms: u64,
    stash: VecDeque<ProtocolMessage>,
    socket: OwnedFd,
    stdin: OwnedFd,
    signals: SignalInbox,
    parser: EventParser,
    negotiation: Option<(Vec<String>, Vec<String>)>,
    escape_deadline: Option<Instant>,
    terminal_events: VecDeque<TerminalEvent>,
    prefer_terminal: bool,
    pending_main: Option<MainEvent>,
    disconnected: bool,
    buffered: bool,
    #[cfg(target_os = "macos")]
    queue: Option<Queue>,
    readable: Vec<rustix::event::FdSetElement>,
    writable: Vec<rustix::event::FdSetElement>,
}

impl EventLoop {
    pub fn new(client: &Arc<InteractiveClient>) -> io::Result<Self> {
        Ok(Self {
            client: Some(Arc::clone(client)),
            tty: Handoff::Own,
            tty_handoff: 0,
            tty_received: 0,
            tty_reported: None,
            escape_ms: 0,
            stash: VecDeque::new(),
            socket: client.receive_fd()?,
            stdin: io::stdin().as_fd().try_clone_to_owned()?,
            signals: SignalInbox::new()?,
            parser: EventParser::default(),
            negotiation: None,
            escape_deadline: None,
            terminal_events: VecDeque::new(),
            prefer_terminal: false,
            pending_main: None,
            disconnected: false,
            buffered: true,
            #[cfg(target_os = "macos")]
            queue: Queue::new().ok(),
            readable: Vec::new(),
            writable: Vec::new(),
        })
    }

    pub fn replace(&mut self, client: &Arc<InteractiveClient>, local: bool) -> io::Result<()> {
        let socket = client.receive_fd()?;
        self.release_tty();
        self.client = Some(Arc::clone(client));
        self.tty = Handoff::Own;
        self.tty_received = 0;
        self.tty_reported = None;
        self.stash.clear();
        self.socket = socket;
        self.disconnected = false;
        self.buffered = true;
        #[cfg(target_os = "macos")]
        if let Some(queue) = self.queue.as_mut() {
            queue.armed[1] = None;
        }
        self.request_tty(local);
        Ok(())
    }

    pub fn request_tty(&mut self, local: bool) {
        let Some(client) = self.client.as_ref() else {
            return;
        };
        if self.tty != Handoff::Own
            || self.disconnected
            || !local
            || std::env::var_os("ZZ_TUI_RELAY").is_some_and(|value| value == "1")
            || !rustix::termios::isatty(&self.stdin)
            || !client
                .server_hello()
                .capabilities
                .iter()
                .any(|capability| capability == zz_protocol::TTY_INPUT_CAPABILITY)
        {
            return;
        }
        self.tty_handoff += 1;
        let request = ProtocolMessage::TtyInput {
            handoff: self.tty_handoff,
        };
        match client.send_with_fd(&request, self.stdin.as_fd()) {
            Ok(()) => self.requested(),
            Err(error) => log::debug!("tty input handoff failed: {error}"),
        }
    }

    fn requested(&mut self) {
        self.tty = Handoff::Requested;
        self.tty_reported = None;
    }

    pub fn release_tty(&mut self) {
        if !matches!(self.tty, Handoff::Requested | Handoff::Daemon) {
            return;
        }
        let Some(client) = self.client.clone() else {
            self.tty = Handoff::Own;
            return;
        };
        self.tty = Handoff::Releasing;
        let release = ProtocolMessage::TtyInputRelease {
            handoff: self.tty_handoff,
        };
        if client.send(&release).is_err() {
            self.tty = Handoff::Own;
            return;
        }
        let deadline = Instant::now() + RELEASE_WAIT;
        while self.tty == Handoff::Releasing {
            match client.try_recv() {
                Ok(Some(message)) => {
                    if let Some(message) = self.take_tty_message(*message) {
                        self.stash.push_back(message);
                    }
                }
                Ok(None) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    let timeout = rustix::event::Timespec {
                        tv_sec: remaining.as_secs().try_into().unwrap_or(i64::MAX),
                        tv_nsec: i64::from(remaining.subsec_nanos()),
                    };
                    let mut poll = [rustix::event::PollFd::new(
                        &self.socket,
                        rustix::event::PollFlags::IN,
                    )];
                    match rustix::event::poll(&mut poll, Some(&timeout)) {
                        Ok(_) | Err(rustix::io::Errno::INTR) => {}
                        Err(_) => break,
                    }
                }
                Err(_) => break,
            }
        }
        self.tty = Handoff::Own;
        self.tty_reported = None;
        self.buffered = true;
    }

    fn relay(
        &mut self,
        message: ProtocolMessage,
        core: &Mutex<ClientCore>,
        forward: impl FnOnce(ProtocolMessage),
    ) {
        let Some(message) = self.take_tty_message(message) else {
            return;
        };
        forward(message);
        self.adopt_negotiation(core);
    }

    pub fn adopt_negotiation(&mut self, core: &Mutex<ClientCore>) {
        let core = lock_core(core);
        let Some(negotiation) = core.terminal_negotiation() else {
            return;
        };
        if self.negotiation.as_ref() == Some(negotiation) {
            return;
        }
        self.parser.set_user_keys(&negotiation.1);
        crate::tty::adopt_negotiated_features(&negotiation.0);
        self.negotiation = Some(negotiation.clone());
    }

    pub fn await_clipboard_reply(&mut self) {
        self.parser
            .await_clipboard_reply(Instant::now() + CLIPBOARD_REPLY_WAIT);
    }

    pub fn await_graphics_reply(&mut self) {
        self.parser
            .await_graphics_reply(Instant::now() + GRAPHICS_REPLY_WAIT);
        self.report_tty(|| None);
    }

    pub fn report_tty(&mut self, pane: impl FnOnce() -> Option<PaneId>) {
        if let Some(report) = self.next_report(pane)
            && let Some(client) = self.client.as_ref()
        {
            let _ = client.send(&report);
        }
    }

    fn next_report(&mut self, pane: impl FnOnce() -> Option<PaneId>) -> Option<ProtocolMessage> {
        if self.tty != Handoff::Daemon {
            return None;
        }
        let pane = (self.terminal_events.is_empty()
            && self.pending_main.is_none()
            && self.escape_deadline.is_none()
            && self.parser.is_idle())
        .then(pane)
        .flatten();
        let direct = matches!(
            self.tty_reported,
            Some((received, Some(_))) if received == self.tty_received
        );
        if (pane.is_none() && !direct) || self.tty_reported == Some((self.tty_received, pane)) {
            return None;
        }
        self.tty_reported = Some((self.tty_received, pane));
        Some(ProtocolMessage::TtyInputReady {
            received: self.tty_received,
            pane,
        })
    }

    fn take_tty_message(&mut self, message: ProtocolMessage) -> Option<ProtocolMessage> {
        match message {
            ProtocolMessage::TtyInputStarted { handoff } => {
                if self.tty == Handoff::Requested && handoff == self.tty_handoff {
                    self.tty = Handoff::Daemon;
                    self.tty_received = 0;
                    self.tty_reported = None;
                }
            }
            ProtocolMessage::TtyInputBytes { bytes } => {
                if self.tty == Handoff::Daemon {
                    self.tty_received += 1;
                }
                self.parse_terminal(&bytes);
            }
            ProtocolMessage::TtyInputClosed { handoff } => {
                if handoff == self.tty_handoff {
                    self.tty = Handoff::Own;
                    self.tty_reported = None;
                }
            }
            message => return Some(message),
        }
        None
    }

    fn parse_terminal(&mut self, bytes: &[u8]) {
        let mut decoded = Vec::new();
        self.parser.push(bytes, &mut decoded);
        self.terminal_events.extend(decoded);
        self.escape_deadline = self
            .parser
            .pending_escape_delay(self.escape_ms)
            .map(|delay| Instant::now() + delay);
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
                self.escape_ms = escape_time.load(Ordering::Relaxed);
                self.parse_terminal(&bytes[..count]);
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

    fn expire_graphics_reply(&mut self, now: Instant) {
        if self.parser.expire_graphics_reply(now) {
            self.parse_terminal(&[]);
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
        let mut started = None;
        let mut read_socket = read_socket;
        self.buffered = true;
        for handled in 0..MAX_COALESCED_EVENTS {
            let received = if let Some(message) = self.stash.pop_front() {
                Ok(Some(Box::new(message)))
            } else if read_socket {
                client.try_recv()
            } else {
                client.try_recv_buffered()
            };
            match received {
                Ok(Some(message)) => {
                    read_socket = false;
                    self.relay(*message, core, |message| {
                        forward_protocol_message(
                            core,
                            message,
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
                    });
                }
                Ok(None) => {
                    self.buffered = false;
                    break;
                }
                Err(error) => {
                    self.disconnected = true;
                    self.tty = Handoff::Own;
                    self.tty_reported = None;
                    let _ = events.send(MainEvent::Disconnected {
                        connection,
                        error: error.to_string(),
                    });
                    break;
                }
            }
            if handled > 0
                && started.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(1)
            {
                break;
            }
        }
    }

    fn wait(
        &mut self,
        output: &TerminalWriter,
        timeout: Option<Duration>,
    ) -> io::Result<(bool, bool, bool)> {
        #[cfg(target_os = "macos")]
        {
            let wanted = [
                (self.terminal_events.is_empty() && self.tty == Handoff::Own)
                    .then(|| self.stdin.as_raw_fd()),
                (!self.disconnected).then(|| self.socket.as_raw_fd()),
                Some(self.signals.fd.as_raw_fd()),
                output.pending_fd().map(|fd| fd.as_raw_fd()),
            ];
            if let Some(queue) = self.queue.as_mut() {
                if let Some([terminal, socket, signal, _]) = queue.wait(wanted, timeout)? {
                    return Ok((terminal, socket, signal));
                }
                self.queue = None;
            }
        }
        self.select(output, timeout)
    }

    #[allow(
        unsafe_code,
        reason = "select borrows descriptors owned by the loop and terminal writer"
    )]
    fn select(
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
        if self.terminal_events.is_empty() && self.tty == Handoff::Own {
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
        now: Instant,
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
        self.escape_ms = escape_time.load(Ordering::Relaxed);
        self.expire_escape(now);
        self.expire_graphics_reply(now);
        {
            let mut output = output.borrow_mut();
            output.tick(now);
            output.flush().map_err(|error| error.to_string())?;
            if output.take_unblocked() {
                return Ok(Some(MainEvent::Repaint));
            }
        }
        if self.buffered {
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
        }
        if self.pending_main.is_none() {
            self.pending_main = incoming.try_recv().ok();
        }
        let mut timeout = match wait {
            BrowserWait::Blocking => None,
            BrowserWait::Timeout(timeout) => Some(timeout),
        };
        for deadline in [
            self.escape_deadline,
            self.parser.graphics_reply_deadline(),
            output.borrow().deadline(),
        ]
        .into_iter()
        .flatten()
        {
            let remaining = deadline.saturating_duration_since(now);
            timeout = Some(timeout.map_or(remaining, |timeout| timeout.min(remaining)));
        }
        if self.pending_main.is_some() || !self.terminal_events.is_empty() || !self.stash.is_empty()
        {
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
        if self.escape_deadline.is_some() {
            self.expire_escape(Instant::now());
        }
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
        let (stdin, input) = nonblocking_pipe().unwrap();
        let (socket, peer) = UnixStream::pair().unwrap();
        let (signal_fd, signal_writer) = nonblocking_pipe().unwrap();
        (
            EventLoop {
                client: None,
                tty: Handoff::Own,
                tty_handoff: 0,
                tty_received: 0,
                tty_reported: None,
                escape_ms: 25,
                stash: VecDeque::new(),
                socket: socket.into(),
                stdin,
                signals: SignalInbox {
                    fd: signal_fd,
                    registrations: Vec::new(),
                },
                parser: EventParser::default(),
                negotiation: None,
                escape_deadline: None,
                terminal_events: VecDeque::new(),
                prefer_terminal: false,
                pending_main: None,
                disconnected: false,
                buffered: true,
                #[cfg(target_os = "macos")]
                queue: Queue::new().ok(),
                readable: Vec::new(),
                writable: Vec::new(),
            },
            input,
            peer,
            signal_writer,
        )
    }

    #[test]
    fn bytes_relayed_after_a_negotiation_decode_with_its_user_keys() {
        let (mut event_loop, _input, _peer, _signals) = pipe_loop();
        let core = Mutex::new(ClientCore::new());
        let forward = |message| lock_core(&core).handle_message(message);
        event_loop.relay(
            ProtocolMessage::Event(zz_protocol::Event {
                sequence: 1,
                payload: zz_protocol::EventPayload::TerminalNegotiation {
                    features: Vec::new(),
                    user_keys: vec!["\x1b[99~".to_owned()],
                },
            }),
            &core,
            forward,
        );
        event_loop.relay(
            ProtocolMessage::TtyInputBytes {
                bytes: b"\x1b[99~".to_vec(),
            },
            &core,
            forward,
        );
        assert_eq!(
            event_loop.terminal_events.drain(..).collect::<Vec<_>>(),
            [TerminalEvent::Key(crate::terminal_event::KeyEvent::new(
                crate::terminal_event::KeyCode::User(0),
                crate::terminal_event::KeyModifiers::NONE,
            ))]
        );
    }

    #[test]
    fn user_keys_set_on_the_loop_decode_daemon_relayed_bytes() {
        let (mut event_loop, _input, _peer, _signals) = pipe_loop();
        event_loop
            .parser
            .set_user_keys(&[String::new(), "\x1b[99~".to_owned()]);
        assert!(
            event_loop
                .take_tty_message(ProtocolMessage::TtyInputBytes {
                    bytes: b"\x1b[99~".to_vec()
                })
                .is_none()
        );
        assert_eq!(
            event_loop.terminal_events.drain(..).collect::<Vec<_>>(),
            [TerminalEvent::Key(crate::terminal_event::KeyEvent::new(
                crate::terminal_event::KeyCode::User(1),
                crate::terminal_event::KeyModifiers::NONE,
            ))]
        );
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
    fn a_handed_tty_stays_unread_and_daemon_bytes_parse_in_order() {
        let (mut event_loop, input, _peer, _signal) = pipe_loop();
        let output = TerminalWriter::with_sink(Box::new(|_| Ok(())));
        event_loop.tty_handoff = 1;
        event_loop.requested();
        assert!(
            event_loop
                .take_tty_message(ProtocolMessage::TtyInputStarted { handoff: 1 })
                .is_none()
        );
        assert_eq!(event_loop.tty, Handoff::Daemon);
        rustix::io::write(&input, b"a").unwrap();
        assert_eq!(
            event_loop.wait(&output, Some(Duration::ZERO)).unwrap(),
            (false, false, false)
        );
        let pane = PaneId(4);
        let ready = |received, pane| Some(ProtocolMessage::TtyInputReady { received, pane });
        assert_eq!(event_loop.next_report(|| Some(pane)), ready(0, Some(pane)));
        assert!(
            event_loop
                .take_tty_message(ProtocolMessage::TtyInputBytes {
                    bytes: b"x\x1b".to_vec()
                })
                .is_none()
        );
        assert_eq!(event_loop.tty_received, 1);
        assert_eq!(event_loop.terminal_events.len(), 1);
        assert_eq!(event_loop.next_report(|| Some(pane)), None);
        event_loop.terminal_events.clear();
        assert_eq!(event_loop.next_report(|| Some(pane)), None);
        let deadline = event_loop.escape_deadline.unwrap();
        event_loop.expire_escape(deadline);
        assert_eq!(event_loop.terminal_events.len(), 1);
        event_loop.terminal_events.clear();
        assert_eq!(event_loop.next_report(|| Some(pane)), ready(1, Some(pane)));
        assert_eq!(event_loop.next_report(|| Some(pane)), None);
        assert_eq!(event_loop.next_report(|| None), ready(1, None));
        assert_eq!(event_loop.next_report(|| None), None);
        assert_eq!(event_loop.next_report(|| Some(pane)), ready(1, Some(pane)));
        assert!(
            event_loop
                .take_tty_message(ProtocolMessage::TtyInputClosed { handoff: 1 })
                .is_none()
        );
        assert_eq!(event_loop.tty, Handoff::Own);
        assert_eq!(event_loop.next_report(|| Some(pane)), None);
        assert_eq!(
            event_loop.wait(&output, Some(Duration::ZERO)).unwrap(),
            (true, false, false)
        );
    }

    #[test]
    fn a_graphics_probe_takes_keys_back_from_the_daemon_until_its_fence_or_deadline() {
        let (mut event_loop, _input, _peer, _signal) = pipe_loop();
        event_loop.tty_handoff = 1;
        event_loop.requested();
        event_loop.take_tty_message(ProtocolMessage::TtyInputStarted { handoff: 1 });
        let pane = PaneId(4);
        let ready = |received, pane| Some(ProtocolMessage::TtyInputReady { received, pane });
        assert_eq!(event_loop.next_report(|| Some(pane)), ready(0, Some(pane)));
        event_loop.await_graphics_reply();
        assert_eq!(event_loop.tty_reported, Some((0, None)));
        assert_eq!(event_loop.next_report(|| Some(pane)), None);
        event_loop.take_tty_message(ProtocolMessage::TtyInputBytes {
            bytes: b"Gi=4294967295;OK\x1b\\\x1b[?62c".to_vec(),
        });
        assert_eq!(event_loop.terminal_events.len(), 2);
        event_loop.terminal_events.clear();
        assert_eq!(event_loop.next_report(|| Some(pane)), ready(1, Some(pane)));

        event_loop.await_graphics_reply();
        assert_eq!(event_loop.tty_reported, Some((1, None)));
        event_loop.take_tty_message(ProtocolMessage::TtyInputBytes {
            bytes: b"Gi=1\r".to_vec(),
        });
        assert!(event_loop.terminal_events.is_empty());
        let deadline = event_loop.parser.graphics_reply_deadline().unwrap();
        event_loop.expire_graphics_reply(deadline);
        assert_eq!(event_loop.terminal_events.len(), 5);
        event_loop.terminal_events.clear();
        assert_eq!(event_loop.next_report(|| Some(pane)), ready(2, Some(pane)));
    }

    #[test]
    fn a_late_close_for_an_earlier_handoff_leaves_the_next_one_standing() {
        let (mut event_loop, input, _peer, _signal) = pipe_loop();
        let output = TerminalWriter::with_sink(Box::new(|_| Ok(())));
        rustix::io::write(&input, b"c").unwrap();
        event_loop.tty_handoff = 1;
        event_loop.requested();
        event_loop.take_tty_message(ProtocolMessage::TtyInputStarted { handoff: 1 });
        event_loop.take_tty_message(ProtocolMessage::TtyInputBytes {
            bytes: b"a".to_vec(),
        });
        assert_eq!(event_loop.tty_received, 1);
        event_loop.tty = Handoff::Own;
        event_loop.tty_handoff = 2;
        event_loop.requested();
        event_loop.take_tty_message(ProtocolMessage::TtyInputBytes {
            bytes: b"b".to_vec(),
        });
        assert_eq!(event_loop.terminal_events.len(), 2);
        event_loop.take_tty_message(ProtocolMessage::TtyInputClosed { handoff: 1 });
        assert_eq!(event_loop.tty, Handoff::Requested);
        event_loop.take_tty_message(ProtocolMessage::TtyInputStarted { handoff: 1 });
        assert_eq!(event_loop.tty, Handoff::Requested);
        event_loop.take_tty_message(ProtocolMessage::TtyInputStarted { handoff: 2 });
        assert_eq!(event_loop.tty, Handoff::Daemon);
        event_loop.terminal_events.clear();
        let pane = PaneId(4);
        assert_eq!(
            event_loop.next_report(|| Some(pane)),
            Some(ProtocolMessage::TtyInputReady {
                received: 0,
                pane: Some(pane)
            })
        );
        assert_eq!(
            event_loop.wait(&output, Some(Duration::ZERO)).unwrap(),
            (false, false, false)
        );
        event_loop.take_tty_message(ProtocolMessage::TtyInputClosed { handoff: 2 });
        assert_eq!(event_loop.tty, Handoff::Own);
        assert_eq!(
            event_loop.wait(&output, Some(Duration::ZERO)).unwrap(),
            (true, false, false)
        );
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
