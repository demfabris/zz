use super::*;

fn frame(message: &ProtocolMessage) -> OutboundFrame {
    OutboundFrame::Owned(encode_protocol_message(message).expect("encode test frame"))
}

fn decoded(frames: &[OutboundFrame]) -> Vec<ProtocolMessage> {
    frames
        .iter()
        .map(|frame| zz_protocol::decode_protocol_frame(frame).expect("decode test frame"))
        .collect()
}

fn started(name: &str) -> ProtocolMessage {
    ProtocolMessage::Event(Event {
        sequence: 0,
        payload: EventPayload::ControlCommandStarted {
            request_id: 1,
            flags: 1,
            canonical_name: Some(name.to_owned()),
            guard: true,
        },
    })
}

fn ran() -> ProtocolMessage {
    ProtocolMessage::ExecExit(zz_protocol::ExecExit {
        server_id: 7,
        outcome: zz_protocol::ExecOutcome::Ran,
    })
}

fn stdio_pair() -> (ControlStdio, OwnedFd, OwnedFd, mio::Poll) {
    let poll = mio::Poll::new().expect("poll");
    let (stdin_read, stdin_write) = rustix::pipe::pipe().expect("stdin pipe");
    let (stdout_read, stdout_write) = rustix::pipe::pipe().expect("stdout pipe");
    let mut next_token = 100;
    let stdio = ControlStdio::open(
        vec![stdin_read, stdout_write],
        poll.registry(),
        &mut next_token,
    )
    .expect("open control stdio");
    (stdio, stdin_write, stdout_read, poll)
}

fn stdout_text(stdio: &mut ControlStdio, stdout: &OwnedFd) -> String {
    assert!(stdio.flush().expect("flush control stdout"));
    let mut buffer = [0_u8; 4096];
    let read = rustix::io::read(stdout, &mut buffer).expect("read control stdout");
    String::from_utf8(buffer[..read].to_vec()).expect("UTF-8 control stdout")
}

#[test]
fn idle_client_lines_render_in_the_daemon_and_other_frames_go_back_to_the_client() {
    let (mut control, input, stdout, _poll) = stdio_pair();
    let mut forwarded = Vec::new();
    control.client_write(b"%session-changed $0 c\n", Some((0, 5)), false);
    rustix::io::write(&input, b"display-message -p x\n").expect("write control line");
    control.read_input();
    assert_eq!(
        control.take_line(true, &mut forwarded).as_deref(),
        Some("display-message -p x")
    );
    assert_eq!(
        control.accept(frame(&started("display-message")), &mut forwarded),
        Some(0)
    );
    let response = ProtocolMessage::CommandResponse(CommandResponse::Success {
        request_id: 1,
        output: RawText::from("x"),
        exit_code: 0,
        stderr: String::new(),
        stdout_claim: zz_protocol::StdoutClaim::default(),
    });
    assert_eq!(control.accept(frame(&response), &mut forwarded), Some(0));
    assert!(
        control
            .accept(frame(&ran()), &mut forwarded)
            .is_some_and(|length| length > 0)
    );
    let output = ProtocolMessage::Event(Event {
        sequence: 0,
        payload: EventPayload::PaneOutput {
            pane: zz_protocol::PaneId(3),
            bytes: b"a\\b\r\n".to_vec(),
        },
    });
    assert!(control.accept(frame(&output), &mut forwarded).is_some());
    assert!(forwarded.is_empty());
    let text = stdout_text(&mut control, &stdout);
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "%session-changed $0 c");
    assert!(lines[1].starts_with("%begin ") && lines[1].ends_with(" 5 1"));
    assert_eq!(lines[2], "x");
    assert_eq!(lines[3], lines[1].replacen("%begin", "%end", 1));
    assert_eq!(lines[4], "%output %3 a\\134b\\015\\012");
    let hook = ProtocolMessage::Event(Event {
        sequence: 0,
        payload: EventPayload::HookEvent {
            name: "window-renamed".to_owned(),
            variables: BTreeMap::new(),
        },
    });
    assert_eq!(control.accept(frame(&hook), &mut forwarded), None);
    assert_eq!(
        decoded(&forwarded),
        [ProtocolMessage::ControlStdioSync { next_number: 6 }, hook]
    );
}

#[test]
fn a_line_that_does_not_settle_in_the_daemon_goes_to_the_client_as_submitted() {
    let (mut control, input, _stdout, _poll) = stdio_pair();
    let mut forwarded = Vec::new();
    control.client_write(b"", Some((0, 2)), false);
    rustix::io::write(&input, b"source-file x.conf\nls\n").expect("write control lines");
    control.read_input();
    assert!(control.take_line(true, &mut forwarded).is_some());
    let first = started("source-file");
    assert_eq!(control.accept(frame(&first), &mut forwarded), None);
    assert_eq!(
        decoded(&forwarded),
        [
            ProtocolMessage::ControlStdioSync { next_number: 2 },
            ProtocolMessage::ControlStdin {
                bytes: b"source-file x.conf\n".to_vec(),
                submitted: true,
                closed: false,
                error: None,
            },
            first,
            ProtocolMessage::ControlStdin {
                bytes: b"ls\n".to_vec(),
                submitted: false,
                closed: false,
                error: None,
            },
        ]
    );
    control.client_write(b"", Some((3, 3)), false);
    assert!(!control.wants_line());
    control.client_write(b"", Some((4, 3)), false);
    rustix::io::write(&input, b"\n").expect("write blank line");
    control.read_input();
    forwarded.clear();
    assert!(control.take_line(true, &mut forwarded).is_none());
    assert_eq!(
        decoded(&forwarded),
        [
            ProtocolMessage::ControlStdioSync { next_number: 3 },
            ProtocolMessage::ControlStdin {
                bytes: b"\n".to_vec(),
                submitted: false,
                closed: false,
                error: None,
            },
        ]
    );
}
