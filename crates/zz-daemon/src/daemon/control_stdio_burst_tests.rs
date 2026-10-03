use super::*;
use zz_protocol::Hello;

fn control_hello() -> ProtocolMessage {
    let ProtocolMessage::ClientHello(mut hello) = super::super::io_tests::hello() else {
        unreachable!()
    };
    hello.kind = ClientKind::Control;
    ProtocolMessage::Hello(Hello::from_client(hello))
}

struct Burst {
    shared: Arc<Shared>,
    event_loop: EventLoop,
    token: Token,
    peer: UnixStream,
    stdin: OwnedFd,
    stdout: OwnedFd,
}

impl Burst {
    fn direct() -> Self {
        let shared = Arc::new(Shared::new(17));
        let mut event_loop = EventLoop::empty(&shared).unwrap();
        let (mut peer, server) = UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
        peer.write_all(&encode_protocol_message(&control_hello()).unwrap())
            .unwrap();
        event_loop.read_ready(token, &shared);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !event_loop.connections[&token].initialized || event_loop.connections[&token].busy {
            assert!(Instant::now() < deadline, "control client setup deadline");
            event_loop.turn(&shared).unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        let (stdin_read, stdin) = rustix::pipe::pipe().unwrap();
        let (stdout, stdout_write) = rustix::net::socketpair(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::DGRAM,
            rustix::net::SocketFlags::empty(),
            None,
        )
        .unwrap();
        let connection = event_loop.connections.get_mut(&token).unwrap();
        connection.received_fds = vec![stdin_read, stdout_write];
        event_loop.open_stdio(token);
        event_loop
            .connections
            .get_mut(&token)
            .unwrap()
            .write_ready()
            .unwrap();
        let mut inbound = Inbound {
            bytes: Vec::new(),
            offset: 0,
        };
        super::super::io_tests::messages(&mut peer, &mut inbound);
        event_loop
            .connections
            .get_mut(&token)
            .unwrap()
            .stdio
            .as_mut()
            .unwrap()
            .client_write(b"", Some((0, 1)), false);
        Self {
            shared,
            event_loop,
            token,
            peer,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, lines: &str) {
        rustix::io::write(&self.stdin, lines.as_bytes()).unwrap();
        let stdin_token = self.event_loop.connections[&self.token]
            .stdio
            .as_ref()
            .unwrap()
            .stdin_token;
        self.event_loop.stdio_ready(self.token, stdin_token);
        self.event_loop.turn(&self.shared).unwrap();
    }

    fn writes(&self) -> Vec<String> {
        let mut writes = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            match rustix::net::recv(&self.stdout, &mut buffer, rustix::net::RecvFlags::DONTWAIT) {
                Ok((read, _)) => writes.push(String::from_utf8(buffer[..read].to_vec()).unwrap()),
                Err(rustix::io::Errno::AGAIN) => return writes,
                Err(error) => panic!("read control stdout: {error}"),
            }
        }
    }

    fn forwarded(&mut self) -> Vec<ProtocolMessage> {
        let mut inbound = Inbound {
            bytes: Vec::new(),
            offset: 0,
        };
        super::super::io_tests::messages(&mut self.peer, &mut inbound)
    }
}

fn blocks(text: &str) -> Vec<(u64, String)> {
    let lines = text.lines().collect::<Vec<_>>();
    lines
        .chunks(3)
        .map(|block| {
            let begin = block[0].split(' ').collect::<Vec<_>>();
            assert_eq!(begin[0], "%begin", "{text}");
            assert_eq!(block[2], block[0].replacen("%begin", "%end", 1), "{text}");
            (begin[2].parse().unwrap(), block[1].to_owned())
        })
        .collect()
}

#[test]
fn a_burst_of_daemon_lines_reaches_stdout_in_one_write() {
    let mut burst = Burst::direct();
    let lines = (0..8)
        .map(|line| format!("display-message -p b{line}\n"))
        .collect::<Vec<_>>()
        .concat();
    burst.send(&lines);
    let writes = burst.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert_eq!(
        blocks(&writes[0]),
        (0..8)
            .map(|line| (line + 1, format!("b{line}")))
            .collect::<Vec<_>>()
    );
    assert!(burst.forwarded().is_empty());
}

#[test]
fn a_burst_flushes_its_daemon_replies_before_handing_a_slow_line_to_the_client() {
    let mut burst = Burst::direct();
    burst.send(
        "display-message -p a\ndisplay-message -p b\nsource-file x.conf\ndisplay-message -p c\n",
    );
    let writes = burst.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert_eq!(
        blocks(&writes[0]),
        [(1, "a".to_owned()), (2, "b".to_owned())]
    );
    let forwarded = burst.forwarded();
    assert_eq!(
        forwarded[0],
        ProtocolMessage::ControlStdioSync { next_number: 3 }
    );
    assert_eq!(
        forwarded[1],
        ProtocolMessage::ControlStdin {
            bytes: b"source-file x.conf\n".to_vec(),
            submitted: true,
            closed: false,
            error: None,
        }
    );
    assert!(forwarded.contains(&ProtocolMessage::ControlStdin {
        bytes: b"display-message -p c\n".to_vec(),
        submitted: false,
        closed: false,
        error: None,
    }));
}
