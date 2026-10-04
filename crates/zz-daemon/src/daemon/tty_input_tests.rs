use std::os::fd::FromRawFd as _;

use zz_protocol::{Hello, encode_protocol_message};
use zz_terminal::KeyInput;

use super::*;

fn tty_hello() -> ProtocolMessage {
    let ProtocolMessage::ClientHello(mut hello) = super::super::io_tests::hello() else {
        unreachable!()
    };
    hello.kind = ClientKind::Interactive;
    hello.capabilities = vec![
        zz_protocol::TTY_INPUT_CAPABILITY.to_owned(),
        crate::CLIENT_EXITS_ON_DETACH_CAPABILITY.to_owned(),
    ];
    ProtocolMessage::Hello(Hello::from_client(hello))
}

#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
fn raw_pty() -> (OwnedFd, OwnedFd) {
    let mut master = -1;
    let mut slave = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    let mut termios = rustix::termios::tcgetattr(&slave).unwrap();
    termios.make_raw();
    rustix::termios::tcsetattr(&slave, rustix::termios::OptionalActions::Now, &termios).unwrap();
    (master, slave)
}

fn key(text: &str) -> KeyInput {
    let character = text.chars().next().unwrap();
    KeyInput {
        action: zz_terminal::KeyAction::Press,
        key: zz_terminal::KeyCode::Character(character),
        modifiers: zz_terminal::Modifiers::default(),
        text: Some(text.into()),
        unshifted_codepoint: Some(character),
    }
}

struct Tty {
    shared: Arc<Shared>,
    event_loop: EventLoop,
    token: Token,
    peer: UnixStream,
    inbound: Inbound,
    master: OwnedFd,
    slave: OwnedFd,
    client: ClientId,
    pane: PaneId,
    tty_messages: Vec<ProtocolMessage>,
}

impl Tty {
    fn open() -> Self {
        let shared = Arc::new(Shared::new(23));
        let mut event_loop = EventLoop::empty(&shared).unwrap();
        let (mut peer, server) = UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
        peer.write_all(&encode_protocol_message(&tty_hello()).unwrap())
            .unwrap();
        event_loop.read_ready(token, &shared);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !event_loop.connections[&token].initialized || event_loop.connections[&token].busy {
            assert!(
                Instant::now() < deadline,
                "interactive client setup deadline"
            );
            event_loop.turn(&shared).unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        let client = event_loop.connections[&token].client.unwrap();
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "tty", "exec /bin/cat"]),
            )
            .expect("new session");
        shared
            .attach(client, context.session.expect("session"))
            .expect("attach");
        let (master, slave) = raw_pty();
        let mut tty = Self {
            shared,
            event_loop,
            token,
            peer,
            inbound: Inbound::default(),
            master,
            slave,
            client,
            pane: context.pane.expect("pane"),
            tty_messages: Vec::new(),
        };
        tty.hand_over();
        assert_eq!(tty.take(), [ProtocolMessage::TtyInputStarted]);
        tty
    }

    fn hand_over(&mut self) {
        let handed = self.slave.try_clone().unwrap();
        self.event_loop
            .connections
            .get_mut(&self.token)
            .unwrap()
            .received_fds = vec![handed];
        self.send(&ProtocolMessage::TtyInput);
    }

    fn send(&mut self, message: &ProtocolMessage) {
        self.peer
            .write_all(&encode_protocol_message(message).unwrap())
            .unwrap();
        self.event_loop.read_ready(self.token, &self.shared);
        self.event_loop.turn(&self.shared).unwrap();
        self.collect();
    }

    fn collect(&mut self) {
        if let Some(connection) = self.event_loop.connections.get_mut(&self.token) {
            connection.write_ready().unwrap();
        }
        let messages = super::super::io_tests::messages(&mut self.peer, &mut self.inbound);
        self.tty_messages
            .extend(messages.into_iter().filter(|message| {
                matches!(
                    message,
                    ProtocolMessage::TtyInputStarted
                        | ProtocolMessage::TtyInputBytes { .. }
                        | ProtocolMessage::TtyInputClosed
                )
            }));
    }

    fn take(&mut self) -> Vec<ProtocolMessage> {
        self.collect();
        std::mem::take(&mut self.tty_messages)
    }

    fn tty_fd(&self) -> Option<&OwnedFd> {
        self.event_loop.connections[&self.token]
            .tty
            .as_ref()
            .map(|tty| &tty.fd)
    }

    fn type_bytes(&mut self, bytes: &[u8]) {
        rustix::io::write(&self.master, bytes).unwrap();
        let fd = self.tty_fd().expect("open tty").try_clone().unwrap();
        let mut poll = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        let timeout = rustix::event::Timespec {
            tv_sec: 5,
            tv_nsec: 0,
        };
        assert_eq!(rustix::event::poll(&mut poll, Some(&timeout)).unwrap(), 1);
        self.event_loop.tty_ready(self.token, &self.shared);
        self.event_loop.turn(&self.shared).unwrap();
        self.collect();
    }

    fn ready(&mut self, received: u64) {
        self.send(&ProtocolMessage::TtyInputReady {
            received,
            pane: Some(self.pane),
        });
    }

    fn client_key(&mut self, input: KeyInput) {
        self.send(&ProtocolMessage::Input(InputMessage::Key {
            pane: self.pane,
            input,
            text_follows: false,
        }));
    }

    fn screen_shows(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.event_loop.turn(&self.shared).unwrap();
            let screen = self
                .shared
                .execute(
                    ClientId(u64::MAX),
                    ClientKind::Command,
                    &mut ExecutionContext::default(),
                    &CommandInvocation::new("capture-pane", ["-p", "-t", "tty"]),
                )
                .expect("capture-pane")
                .output
                .to_string();
            if screen.contains(text) {
                return;
            }
            assert!(Instant::now() < deadline, "{screen:?} lacks {text:?}");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn bytes(bytes: &[u8]) -> ProtocolMessage {
    ProtocolMessage::TtyInputBytes {
        bytes: bytes.to_vec(),
    }
}

#[test]
fn plain_keys_reach_the_pane_and_the_rest_waits_for_the_client_to_catch_up() {
    let mut tty = Tty::open();
    tty.type_bytes(b"a");
    assert_eq!(tty.take(), [bytes(b"a")]);
    tty.client_key(key("a"));
    tty.ready(1);
    tty.type_bytes(b"b");
    assert!(tty.take().is_empty());
    tty.screen_shows("ab");
    tty.type_bytes("\u{e9}".as_bytes());
    assert_eq!(tty.take(), [bytes("\u{e9}".as_bytes())]);
    tty.type_bytes(b"c");
    assert_eq!(tty.take(), [bytes(b"c")]);
    tty.client_key(key("\u{e9}"));
    tty.ready(2);
    tty.ready(1);
    tty.type_bytes(b"d");
    assert_eq!(tty.take(), [bytes(b"d")]);
    tty.client_key(key("c"));
    tty.ready(3);
    tty.client_key(key("d"));
    tty.ready(4);
    tty.type_bytes(b"e");
    assert!(tty.take().is_empty());
    tty.screen_shows("ab\u{e9}cde");
}

#[test]
fn a_bound_key_or_the_prefix_goes_back_to_the_client_in_order() {
    let mut tty = Tty::open();
    tty.shared
        .execute(
            tty.client,
            ClientKind::Interactive,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("bind-key", ["-n", "z", "display-message", "zed"]),
        )
        .expect("bind");
    tty.ready(0);
    tty.type_bytes(b"xzy");
    assert_eq!(tty.take(), [bytes(b"zy")]);
    tty.ready(1);
    tty.type_bytes(b"\x02");
    assert_eq!(tty.take(), [bytes(b"\x02")]);
    tty.ready(2);
    tty.type_bytes(b"\x1b");
    assert_eq!(tty.take(), [bytes(b"\x1b")]);
    tty.ready(3);
    tty.type_bytes(b"Gi=");
    assert_eq!(tty.take(), [bytes(b"Gi=")]);
    tty.screen_shows("x");
}

#[test]
fn release_detach_and_socket_loss_close_the_tty_without_reading_more() {
    let mut tty = Tty::open();
    tty.send(&ProtocolMessage::TtyInputRelease);
    assert_eq!(tty.take(), [ProtocolMessage::TtyInputClosed]);
    assert!(tty.tty_fd().is_none());
    assert!(tty.event_loop.tty_tokens.is_empty());

    tty.hand_over();
    assert_eq!(tty.take(), [ProtocolMessage::TtyInputStarted]);
    tty.ready(0);
    tty.type_bytes(b"q");
    tty.screen_shows("q");
    tty.shared
        .execute(
            tty.client,
            ClientKind::Interactive,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("detach-client", Vec::<String>::new()),
        )
        .expect("detach");
    rustix::io::write(&tty.master, b"w").unwrap();
    thread::sleep(Duration::from_millis(50));
    tty.event_loop.tty_ready(tty.token, &tty.shared);
    assert!(tty.take().contains(&ProtocolMessage::TtyInputClosed));
    assert!(tty.tty_fd().is_none());
    let mut left = [0_u8; 4];
    assert_eq!(rustix::io::read(&tty.slave, &mut left).unwrap(), 1);
    assert_eq!(&left[..1], b"w");

    let mut tty = Tty::open();
    tty.peer.shutdown(std::net::Shutdown::Both).unwrap();
    tty.event_loop.read_ready(tty.token, &tty.shared);
    assert!(
        tty.event_loop
            .connections
            .get(&tty.token)
            .is_none_or(|connection| connection.tty.is_none())
    );
    assert!(tty.event_loop.tty_tokens.is_empty());
}

#[test]
fn a_handoff_without_a_terminal_is_refused() {
    let mut tty = Tty::open();
    tty.send(&ProtocolMessage::TtyInputRelease);
    tty.take();
    let (read, _write) = rustix::pipe::pipe().unwrap();
    tty.event_loop
        .connections
        .get_mut(&tty.token)
        .unwrap()
        .received_fds = vec![read];
    tty.send(&ProtocolMessage::TtyInput);
    assert_eq!(tty.take(), [ProtocolMessage::TtyInputClosed]);
    tty.send(&ProtocolMessage::TtyInput);
    assert_eq!(tty.take(), [ProtocolMessage::TtyInputClosed]);
    assert!(tty.tty_fd().is_none());
}
