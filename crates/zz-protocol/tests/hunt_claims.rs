use std::collections::BTreeMap;

use zz_protocol::{
    BrowserDescriptor, ClientHello, ClientInstanceId, ClientKind, CommandResponse, Event,
    EventPayload, GuiResponse, InputMessage, LayoutNode, MuxOptionKey, MuxSnapshot,
    PROTOCOL_VERSION, PaneId, PaneKindSnapshot, PaneSnapshot, PasteUploadPurpose, ProtocolMessage,
    ServerError, TmuxColour, WindowId, WindowSnapshot, encode_protocol_message,
};
use zz_terminal::TerminalColorScheme;

fn payload(frame: &[u8]) -> &[u8] {
    &frame[8..]
}

#[test]
fn protocol_version_on_this_commit_is_seventy_seven() {
    assert_eq!(PROTOCOL_VERSION, 77);
}

#[test]
fn chooser_states_append_the_v72_filter_fallback_flag() {
    let tree = zz_protocol::ChooseTreeState {
        items: Vec::new(),
        search: None,
        selected: 2,
        kind: zz_protocol::ChooseTreeKind::Panes,
        filter_no_matches: true,
    };
    assert_eq!(
        postcard::to_stdvec(&tree).expect("encode tree chooser state"),
        [0, 0, 2, 1, 1]
    );
    assert_eq!(
        postcard::from_bytes::<zz_protocol::ChooseTreeState>(&[0, 0, 2, 1, 1])
            .expect("decode tree chooser state"),
        tree
    );

    let buffer = zz_protocol::ChooseBufferState {
        items: Vec::new(),
        search: None,
        selected: 3,
        filter_no_matches: true,
    };
    assert_eq!(
        postcard::to_stdvec(&buffer).expect("encode buffer chooser state"),
        [0, 0, 3, 1]
    );
    assert_eq!(
        postcard::from_bytes::<zz_protocol::ChooseBufferState>(&[0, 0, 3, 1])
            .expect("decode buffer chooser state"),
        buffer
    );
}

#[test]
fn control_events_and_window_layout_fields_keep_the_frozen_wire_tail() {
    let pane = PaneId(3);
    let event_tag = |payload| {
        postcard::to_stdvec(&Event {
            sequence: 0,
            payload,
        })
        .expect("encode event")[1]
    };
    assert_eq!(
        event_tag(EventPayload::ControlExit {
            reason: String::new(),
        }),
        39
    );
    assert_eq!(
        event_tag(EventPayload::HookEvent {
            name: String::new(),
            variables: BTreeMap::new(),
        }),
        40
    );
    assert_eq!(
        event_tag(EventPayload::PaneOutput {
            pane,
            bytes: Vec::new(),
        }),
        41
    );
    assert_eq!(
        event_tag(EventPayload::PaneOutputState {
            pane,
            paused: false,
        }),
        42
    );
    assert_eq!(
        event_tag(EventPayload::PaneOutputAged {
            pane,
            age_ms: 0,
            bytes: Vec::new(),
        }),
        43
    );
    assert_eq!(
        event_tag(EventPayload::ControlFlags {
            wait_exit: false,
            pause_after_ms: None,
            no_output: false,
        }),
        44
    );
    assert_eq!(
        event_tag(EventPayload::SubscriptionChanged {
            name: String::new(),
            session: zz_protocol::SessionId(1),
            window: None,
            window_index: None,
            pane: None,
            value: String::new(),
        }),
        45
    );
    for flags in [0, 1] {
        let event = Event {
            sequence: 0,
            payload: EventPayload::ControlCommandGuard {
                output: String::new(),
                error: false,
                sticky_failure: false,
                flags,
            },
        };
        let bytes = postcard::to_stdvec(&event).expect("encode control command guard");
        assert_eq!(bytes, [0, 47, 0, 0, 0, flags]);
        assert_eq!(
            postcard::from_bytes::<Event>(&bytes).expect("decode control command guard"),
            event
        );
    }

    let window = WindowSnapshot {
        id: WindowId(1),
        index: 2,
        name: "w".to_owned(),
        automatic_rename: true,
        active_pane: pane,
        zoomed_pane: None,
        layout: LayoutNode::Pane(pane),
        panes: BTreeMap::new(),
        layout_dump: "L".to_owned(),
        visible_layout_dump: "V".to_owned(),
        status_label: "S".to_owned(),
    };
    assert_eq!(
        postcard::to_stdvec(&window).expect("encode window"),
        [1, 2, 1, b'w', 1, 3, 0, 0, 3, 0, 1, b'L', 1, b'V', 1, b'S']
    );
}

#[test]
fn control_client_kind_keeps_frozen_wire_tag_two() {
    assert_eq!(
        postcard::to_stdvec(&ClientKind::Control).expect("encode control client kind"),
        [2]
    );
}

#[test]
fn target_lookup_errors_use_tmux_wording() {
    assert_eq!(
        ServerError::SessionNotFound("work".to_owned()).to_string(),
        "can't find session: work"
    );
    assert_eq!(
        ServerError::WindowNotFound("logs".to_owned()).to_string(),
        "can't find window: logs"
    );
    assert_eq!(
        ServerError::PaneNotFound("9".to_owned()).to_string(),
        "can't find pane: 9"
    );
}

#[test]
fn mux_option_key_has_seventeen_daemon_owned_keys() {
    assert_eq!(MuxOptionKey::ALL.len(), 17);
    assert!(MuxOptionKey::ALL.contains(&MuxOptionKey::HistoryTrickle));
    assert!(MuxOptionKey::ALL.contains(&MuxOptionKey::Prefix2));
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
        working_directory: None,
    }))
    .expect("encode hello");
    assert_eq!(
        frame,
        [
            0x0e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4d, 0x00, 0x00, 0x4d, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x01, 0x00, 0x00,
        ]
    );
}

#[test]
fn command_success_round_trips_output_and_exit_code() {
    let message = ProtocolMessage::CommandResponse(CommandResponse::Success {
        request_id: 7,
        output: "job output".to_owned(),
        exit_code: 3,
        stderr: "job error".to_owned(),
    });
    let frame = encode_protocol_message(&message).expect("encode command response");
    assert_eq!(
        zz_protocol::decode_protocol_frame(&frame).expect("decode command response"),
        message
    );
}

#[test]
fn command_error_round_trips_output_after_the_error() {
    let message = ProtocolMessage::CommandResponse(CommandResponse::Error {
        request_id: 8,
        error: ServerError::WindowNotFound("missing".to_owned()),
        output: "hook output".to_owned(),
    });
    let frame = encode_protocol_message(&message).expect("encode command response");
    assert_eq!(
        zz_protocol::decode_protocol_frame(&frame).expect("decode command response"),
        message
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
        dead: false,
        dead_status: None,
        border_colour: None,
        active_border_colour: None,
    };
    assert!(snapshot.bell);
}

#[test]
fn pane_snapshot_border_colours_round_trip_and_reject_invalid_rgb() {
    let snapshot = PaneSnapshot {
        id: PaneId(3),
        title: "bash".to_owned(),
        kind: PaneKindSnapshot::Terminal,
        synchronized_input: false,
        bell: false,
        dead: false,
        dead_status: None,
        border_colour: Some(TmuxColour::Rgb(0x00ff_00ff)),
        active_border_colour: Some(TmuxColour::Basic(1)),
    };
    let bytes = postcard::to_stdvec(&snapshot).expect("pane snapshot encodes");
    assert_eq!(
        postcard::from_bytes::<PaneSnapshot>(&bytes).expect("pane snapshot decodes"),
        snapshot
    );

    let hostile = PaneSnapshot {
        border_colour: Some(TmuxColour::Rgb(0x0100_0000)),
        ..snapshot
    };
    let bytes = postcard::to_stdvec(&hostile).expect("hostile snapshot encodes");
    assert!(postcard::from_bytes::<PaneSnapshot>(&bytes).is_err());
}
