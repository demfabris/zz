use std::io::Read as _;
use std::os::unix::net::UnixStream;

use super::*;

const BURST_KEYS: usize = 2400;
const BURST_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

fn burst_bytes() -> Vec<u8> {
    (0..BURST_KEYS)
        .map(|index| BURST_ALPHABET[index % BURST_ALPHABET.len()])
        .collect()
}

fn capture(shared: &Arc<Shared>, client: ClientId, pane: PaneId) -> String {
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new("capture-pane", ["-p", "-t", &pane.to_string()]),
        )
        .expect("capture the burst pane")
        .output
        .to_string()
}

fn read_until_closed(mut peer: UnixStream) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = vec![0; 64 * 1024];
        while matches!(peer.read(&mut buffer), Ok(read) if read > 0) {}
    })
}

#[test]
fn a_typed_burst_reaches_the_pane_whole_and_in_order() {
    let directory = tempfile::tempdir().unwrap();
    let typed = directory.path().join("typed");
    let go = directory.path().join("go");
    let shared = Arc::new(Shared::new(4207));
    let mailbox = OutboundMailbox::new();
    let (client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some("typed-burst".to_owned()),
        None,
        Arc::clone(&mailbox),
    );
    let (peer, server) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();
    #[cfg(target_vendor = "apple")]
    rustix::net::sockopt::set_socket_nosigpipe(&server, true).unwrap();
    mailbox.state.lock().direct_socket = Some(server.into());
    let reader = read_until_closed(peer);
    let command = format!(
        "stty raw; printf 'BURST_%s\\r\\n' READY; while [ ! -e '{}' ]; do sleep 0.01; done; exec cat > '{}'",
        go.display(),
        typed.display()
    );
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "typed-burst", command.as_str()]),
        )
        .expect("new-session");
    let pane = context.pane.expect("burst pane");
    shared
        .attach(client, context.session.expect("burst session"))
        .expect("attach");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !capture(&shared, client, pane).contains("BURST_READY") {
        assert!(Instant::now() < deadline, "the burst pane never got ready");
        thread::sleep(Duration::from_millis(20));
    }

    let expected = burst_bytes();
    for byte in &expected {
        let character = char::from(*byte);
        shared
            .input(
                client,
                ClientKind::Interactive,
                &mut ExecutionContext::default(),
                InputMessage::Key {
                    pane,
                    input: super::tests::test_key(
                        zz_terminal::KeyCode::Character(character),
                        zz_terminal::Modifiers::default(),
                        Some(character.encode_utf8(&mut [0; 4])),
                    ),
                    text_follows: false,
                },
            )
            .expect("type a key");
    }

    std::fs::write(&go, []).unwrap();
    let mut arrived = Vec::new();
    let mut grew = Instant::now();
    while arrived.len() < expected.len() && grew.elapsed() < Duration::from_secs(5) {
        let read = std::fs::read(&typed).unwrap_or_default();
        if read.len() != arrived.len() {
            grew = Instant::now();
        }
        arrived = read;
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        arrived.len(),
        expected.len(),
        "typed keys went missing on the way to the pane"
    );
    assert!(
        arrived == expected,
        "typed keys reached the pane out of order"
    );

    shared.request_shutdown();
    drop(mailbox.state.lock().direct_socket.take());
    reader.join().unwrap();
}
