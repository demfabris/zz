#[cfg(all(unix, feature = "daemon"))]
fn readiness_pair() -> (tempfile::TempDir, super::LocalStream, super::LocalStream) {
    use crate::transport::{LocalTransport, Transport, TransportListener};
    let directory = tempfile::Builder::new()
        .prefix("zz-ready-")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = directory.path().join("s");
    let listener = LocalTransport::bind(&socket).unwrap();
    let client = LocalTransport::connect(&socket).unwrap();
    let daemon = listener.accept().unwrap();
    daemon
        .set_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    client
        .set_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    (directory, daemon, client)
}

#[cfg(all(unix, feature = "daemon"))]
#[test]
fn readiness_retains_partial_frames_and_resumes_blocking_receive() {
    use super::TransportStream as _;
    use std::io::Write as _;
    let (_directory, mut daemon, client) = readiness_pair();
    let fd = client.receive_fd().unwrap();
    let flags = rustix::fs::fcntl_getfl(&fd).unwrap();
    let message = zz_protocol::ProtocolMessage::Attach {
        session: "x".repeat(1024),
    };
    let frame = zz_protocol::encode_protocol_message(&message).unwrap();
    let mut receiver = super::ProtocolReceiver::new(client);
    for cut in [1, 3, 9] {
        let start = receiver.ready.as_ref().map_or(0, |ready| ready.bytes.len());
        daemon.write_all(&frame[start..cut]).unwrap();
        assert!(receiver.try_recv_decodable().unwrap().is_none());
        assert_eq!(receiver.ready.as_ref().unwrap().bytes, frame[..cut]);
    }
    let scratch = receiver.frame.as_ptr();
    for _ in 0..4 {
        assert!(receiver.try_recv_decodable().unwrap().is_none());
        assert_eq!(receiver.frame.as_ptr(), scratch);
    }
    daemon.write_all(&frame[9..]).unwrap();
    assert_eq!(receiver.recv_decodable().unwrap(), (message, false));
    assert!(receiver.try_recv_decodable().unwrap().is_none());
    assert_eq!(rustix::fs::fcntl_getfl(&fd).unwrap(), flags);
}

#[cfg(all(unix, feature = "daemon"))]
#[test]
fn readiness_drains_initial_pending_and_buffered_frames_before_eof() {
    use std::io::Write as _;
    let (_directory, mut daemon, client) = readiness_pair();
    let messages: Vec<_> = ["first", "second", "third"]
        .map(|session| zz_protocol::ProtocolMessage::Attach {
            session: session.to_owned(),
        })
        .into();
    let bytes: Vec<_> = messages
        .iter()
        .flat_map(|message| zz_protocol::encode_protocol_message(message).unwrap())
        .collect();
    daemon.write_all(&bytes).unwrap();
    let mut receiver = super::ProtocolReceiver::new(client);
    assert_eq!(receiver.recv().unwrap(), messages[0]);
    let initial = zz_protocol::ProtocolMessage::Attach {
        session: "initial".to_owned(),
    };
    receiver.pending.push_back(initial.clone());
    drop(daemon);
    assert_eq!(
        receiver.try_recv_decodable().unwrap(),
        Some((Box::new(initial), false))
    );
    for message in &messages[1..] {
        assert_eq!(
            receiver.try_recv_decodable().unwrap(),
            Some((Box::new(message.clone()), false))
        );
    }
    assert!(
        matches!(receiver.try_recv_decodable(), Err(crate::DaemonError::Io(error))
        if error.kind() == std::io::ErrorKind::UnexpectedEof)
    );
}

#[cfg(all(unix, feature = "daemon"))]
#[test]
fn readiness_preserves_decode_resync_across_an_empty_poll() {
    use std::io::Write as _;
    let (_directory, mut daemon, client) = readiness_pair();
    let oversized = zz_protocol::ProtocolMessage::Event(zz_protocol::Event {
        sequence: 1,
        payload: zz_protocol::EventPayload::ChooseTree {
            state: Some(zz_protocol::ChooseTreeState {
                items: Vec::new(),
                search: None,
                selected: 0,
                kind: zz_protocol::ChooseTreeKind::Panes,
                filter_no_matches: false,
                prompt: "x".repeat(zz_protocol::MAX_CHOOSE_ITEM_TEXT_BYTES + 1),
                help: false,
            }),
        },
    });
    daemon
        .write_all(&zz_protocol::encode_protocol_message(&oversized).unwrap())
        .unwrap();
    let mut receiver = super::ProtocolReceiver::new(client);
    assert!(receiver.try_recv_decodable().unwrap().is_none());
    assert!(receiver.try_recv_decodable().unwrap().is_none());
    let next = zz_protocol::ProtocolMessage::Attach {
        session: "next".to_owned(),
    };
    daemon
        .write_all(&zz_protocol::encode_protocol_message(&next).unwrap())
        .unwrap();
    assert_eq!(
        receiver.try_recv_decodable().unwrap(),
        Some((Box::new(next), true))
    );
}

#[cfg(all(unix, feature = "daemon"))]
#[test]
fn readiness_rejects_invalid_lengths_before_reading_a_body() {
    use std::io::Write as _;
    for length in [0, 3, zz_protocol::MAX_FRAME_BYTES as u32 + 1] {
        let (_directory, mut daemon, client) = readiness_pair();
        daemon.write_all(&length.to_le_bytes()).unwrap();
        let mut receiver = super::ProtocolReceiver::new(client);
        assert!(matches!(
            receiver.try_recv_decodable(),
            Err(crate::DaemonError::Protocol(
                zz_protocol::ProtocolError::Truncated
                    | zz_protocol::ProtocolError::FrameTooLarge(_)
            ))
        ));
        assert_eq!(receiver.ready.as_ref().unwrap().bytes.len(), 4);
        assert!(receiver.frame.capacity() <= super::RECEIVE_BUFFER_BYTES);
    }
}
