use zz_protocol::{
    BrowserDescriptor, ClientHello, ClientInstanceId, ClientKind, Event, EventPayload, GuiResponse,
    InputMessage, MuxOptionKey, MuxSnapshot, PROTOCOL_VERSION, PaneId, PaneKindSnapshot,
    PaneSnapshot, PasteUploadPurpose, ProtocolMessage, encode_protocol_message,
};
use zz_terminal::TerminalColorScheme;

fn payload(frame: &[u8]) -> &[u8] {
    &frame[8..]
}

#[test]
fn protocol_version_on_this_commit_is_fifty_five() {
    assert_eq!(PROTOCOL_VERSION, 55);
}

#[test]
fn mux_option_key_has_fourteen_daemon_owned_keys() {
    assert_eq!(MuxOptionKey::ALL.len(), 14);
    assert!(MuxOptionKey::ALL.contains(&MuxOptionKey::HistoryTrickle));
}

#[test]
fn dark_interactive_hello_encodes_version_and_instance_id_as_varints() {
    let frame = encode_protocol_message(&ProtocolMessage::ClientHello(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(0),
        kind: ClientKind::Interactive,
        device_name: None,
        capabilities: Vec::new(),
        color_scheme: Some(TerminalColorScheme::Dark),
        origin: None,
    }))
    .expect("encode hello");
    assert_eq!(
        frame,
        [
            0x0d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x37, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x01, 0x00,
        ]
    );
}

#[test]
fn postcard_tags_put_input_before_event_before_gui_response() {
    let input = encode_protocol_message(&ProtocolMessage::Input(InputMessage::Text {
        pane: PaneId(1),
        text: String::new(),
    }))
    .expect("encode input");
    let event = encode_protocol_message(&ProtocolMessage::Event(Event {
        sequence: 0,
        payload: EventPayload::Snapshot(MuxSnapshot::default()),
    }))
    .expect("encode event");
    let gui = encode_protocol_message(&ProtocolMessage::GuiResponse(GuiResponse::Success {
        request_id: 0,
        output: String::new(),
    }))
    .expect("encode gui");
    assert_eq!(payload(&input)[0], 9);
    assert_eq!(payload(&event)[0], 10);
    assert_eq!(payload(&gui)[0], 11);
}

#[test]
fn paste_upload_begin_carries_purpose() {
    let message = ProtocolMessage::PasteUploadBegin {
        upload_id: 1,
        pane: PaneId(2),
        purpose: PasteUploadPurpose::RecordPastedImage,
        extension: "png".to_owned(),
        total_bytes: 8,
    };
    let frame = encode_protocol_message(&message).expect("encode paste begin");
    assert_eq!(
        zz_protocol::decode_protocol_frame(&frame).expect("decode paste begin"),
        message
    );
}

#[test]
fn browser_descriptor_is_tabs_not_a_single_url() {
    let descriptor = BrowserDescriptor {
        tabs: vec![
            "https://a.example".to_owned(),
            "https://b.example".to_owned(),
        ],
        active_tab: 1,
        profile: "work".to_owned(),
    };
    assert_eq!(descriptor.url(), "https://b.example");
}

#[test]
fn pane_snapshot_carries_bell() {
    let snapshot = PaneSnapshot {
        id: PaneId(3),
        title: "bash".to_owned(),
        kind: PaneKindSnapshot::Terminal,
        synchronized_input: false,
        bell: true,
    };
    assert!(snapshot.bell);
}
