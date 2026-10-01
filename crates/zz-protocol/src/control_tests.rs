use std::collections::BTreeMap;

use crate::*;

fn snapshot() -> MuxSnapshot {
    let pane = PaneId(7);
    let window = WindowId(5);
    MuxSnapshot {
        generation: 1,
        focused_window: None,
        sessions: vec![SessionSnapshot {
            id: SessionId(3),
            name: "session".to_owned(),
            active_window: window,
            viewers: Vec::new(),
            windows: vec![WindowSnapshot {
                id: window,
                index: 0,
                name: "window".to_owned(),
                automatic_rename: true,
                active_pane: pane,
                zoomed_pane: None,
                layout: LayoutNode::Pane(pane),
                panes: BTreeMap::from([(
                    pane,
                    PaneSnapshot {
                        id: pane,
                        title: "title".to_owned(),
                        kind: PaneKindSnapshot::Terminal,
                        synchronized_input: false,
                        bell: false,
                        dead: false,
                        dead_status: None,
                        border_colour: None,
                        active_border_colour: None,
                        border_status_text: String::new(),
                        mode: None,
                    },
                )]),
                layout_dump: String::new(),
                visible_layout_dump: String::new(),
                status_label: String::new(),
                activity: false,
                pane_border_status: PaneBorderStatus::Off,
                pane_border_lines: PaneBorderLines::Single,
                pane_border_indicators: PaneBorderIndicators::Colour,
                pane_order: vec![pane],
                pane_z_order: vec![pane],
            }],
        }],
    }
}

fn key_table(name: &str, keys: &[&str]) -> KeyTableSnapshot {
    KeyTableSnapshot {
        name: name.to_owned(),
        bindings: keys
            .iter()
            .map(|key| KeyBindingSnapshot {
                key: (*key).to_owned(),
                commands: vec![],
                repeat: false,
                note: None,
            })
            .collect(),
    }
}

#[test]
fn compact_welcome_round_trip_is_bounded_and_caps_recover_names() {
    let capabilities = CAPABILITY_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let welcome = Welcome {
        protocol_version: PROTOCOL_VERSION,
        server_id: u64::MAX,
        client_id: ClientId(42),
        client_instance_id: ClientInstanceId(u64::MAX),
        caps: Welcome::caps_from_strings(&capabilities),
    };
    let message = ProtocolMessage::Welcome(welcome);
    let encoded = encode_protocol_message(&message).unwrap();
    assert!(encoded.len() <= 48, "welcome bytes: {}", encoded.len());
    assert_eq!(decode_protocol_frame(&encoded).unwrap(), message);
    assert_eq!(welcome.capability_strings(), capabilities);
}

#[test]
fn compact_welcome_rejects_missing_required_capabilities_on_encode_and_decode() {
    for missing in [CONTROL_CAPABILITY, PANE_FRAME_CAPABILITY] {
        let capabilities = CAPABILITY_NAMES
            .iter()
            .filter(|name| **name != missing)
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>();
        let welcome = Welcome {
            protocol_version: PROTOCOL_VERSION,
            server_id: 1,
            client_id: ClientId(2),
            client_instance_id: ClientInstanceId(3),
            caps: Welcome::caps_from_strings(&capabilities),
        };
        assert!(!welcome.has_capability(missing));
        let message = ProtocolMessage::Welcome(welcome);
        let error = encode_protocol_message(&message).unwrap_err();
        assert!(
            matches!(error, ProtocolError::InvalidServerHello(reason) if reason.contains(missing))
        );
        let payload = postcard::to_stdvec(&message).unwrap();
        let frame =
            crate::framing::encode_enveloped(crate::framing::Lane::Control, &payload).unwrap();
        let error = decode_protocol_frame(&frame).unwrap_err();
        assert!(
            matches!(error, ProtocolError::InvalidServerHello(reason) if reason.contains(missing))
        );
    }
}

#[test]
fn control_command_start_is_compact_and_bounds_the_canonical_name() {
    for (canonical_name, guard) in [(None, false), (Some("source-file".to_owned()), true)] {
        let message = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::ControlCommandStarted {
                request_id: u64::MAX,
                flags: 1,
                canonical_name,
                guard,
            },
        });
        let frame = encode_protocol_message(&message).unwrap();
        assert!(frame.len() <= 48);
        assert_eq!(decode_protocol_frame(&frame).unwrap(), message);
    }
    let message = ProtocolMessage::Event(Event {
        sequence: 1,
        payload: EventPayload::ControlCommandStarted {
            request_id: 1,
            flags: 0,
            canonical_name: Some("x".repeat(MAX_GUI_TEXT_BYTES + 1)),
            guard: true,
        },
    });
    assert!(matches!(
        encode_protocol_message(&message),
        Err(ProtocolError::InvalidServerHello(_))
    ));
    let payload = postcard::to_stdvec(&message).unwrap();
    let frame = crate::framing::encode_enveloped(crate::framing::Lane::Control, &payload).unwrap();
    assert!(matches!(
        decode_protocol_frame(&frame),
        Err(ProtocolError::InvalidServerHello(_))
    ));
}

#[test]
fn hello_moves_environment_into_one_blob_and_keeps_raw_bytes() {
    let client = ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(1),
        kind: ClientKind::Interactive,
        device_name: None,
        capabilities: Vec::new(),
        color_scheme: None,
        origin: None,
        working_directory: None,
        environment: vec![RawText::from(&b"K=\xff"[..])],
        process_id: 1,
    };
    let mut hello = Hello::from_client(client.clone());
    hello.viewport = Some(ClientViewport {
        columns: 120,
        rows: 40,
        cell_width_px: 8,
        cell_height_px: 16,
    });
    hello.subscriptions = Subscriptions::terminal();
    hello.attach = Some(AttachOperation::Session("session".to_owned()));
    assert!(hello.client.environment.is_empty());
    assert_eq!(hello.clone().into_client(), client);
    let message = ProtocolMessage::Hello(hello);
    assert_eq!(
        decode_protocol_frame(&encode_protocol_message(&message).unwrap()).unwrap(),
        message
    );
}

#[test]
fn small_tree_edits_stay_small_and_apply_without_replacing_windows() {
    let before = snapshot();
    for change in 0..4 {
        let mut after = before.clone();
        after.generation += 1;
        match change {
            0 => after.sessions[0].name = "renamed".to_owned(),
            1 => after.sessions[0].windows[0].name = "renamed".to_owned(),
            2 => {
                after.sessions[0].windows[0]
                    .panes
                    .get_mut(&PaneId(7))
                    .unwrap()
                    .title = "renamed".to_owned();
            }
            _ => after.sessions[0].windows[0].status_label = "own label".to_owned(),
        }
        let delta = TreeDelta::between(&before, &after);
        assert_eq!(delta.ops.len(), 1);
        let message = ProtocolMessage::Event(Event {
            sequence: 2,
            payload: EventPayload::TreeDelta(delta.clone()),
        });
        let frame = encode_protocol_message(&message).unwrap();
        assert!(
            frame.len() <= 60,
            "small tree change bytes: {}",
            frame.len()
        );
        let mut actual = before.clone();
        delta.apply(&mut actual).unwrap();
        assert_eq!(actual, after);
    }
}

#[test]
fn tree_delta_rejects_bad_base_and_invalid_operations_atomically() {
    let before = snapshot();
    let mut actual = before.clone();
    let delta = TreeDelta {
        base: before.generation + 1,
        version: 3,
        ops: vec![TreeOp::RemoveSession(SessionId(3))],
    };
    assert!(matches!(
        delta.apply(&mut actual),
        Err(TreeDeltaError::BaseMismatch { .. })
    ));
    assert_eq!(actual, before);
    let delta = TreeDelta {
        base: before.generation,
        version: 3,
        ops: vec![
            TreeOp::SessionName {
                session: SessionId(3),
                name: "changed".to_owned(),
            },
            TreeOp::WindowName {
                session: SessionId(3),
                window: WindowId(999),
                name: "missing".to_owned(),
            },
        ],
    };
    assert_eq!(
        delta.apply(&mut actual),
        Err(TreeDeltaError::MissingWindow(WindowId(999)))
    );
    assert_eq!(actual, before);
}

#[test]
fn mouse_bitset_preserves_modifiers_locations_and_copy_reachability() {
    let root = [
        "MouseDown1Pane",
        "C-M-S-MouseDragEnd3Control255",
        "DoubleClick2StatusLeft",
        "WheelDownBorder",
    ];
    let tables = vec![
        key_table("root", &root),
        key_table("copy-mode", &["MouseDrag1Pane"]),
        key_table("copy-mode-vi", &["WheelUpPane"]),
        key_table("prefix", &["MouseDown2Pane"]),
    ];
    let mouse = MouseBindings::from_tables(&tables);
    for key in root {
        assert!(mouse.contains(key));
    }
    assert!(!mouse.contains("MouseDrag1Pane"));
    assert!(!mouse.contains("MouseDown2Pane"));
    assert!(mouse.copy_keys().contains(&"MouseDrag1Pane".to_owned()));
    assert!(mouse.copy_keys().contains(&"WheelUpPane".to_owned()));
    let mut actual = mouse.keys();
    actual.sort();
    let mut expected = root.iter().map(|key| (*key).to_owned()).collect::<Vec<_>>();
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn default_mouse_hash_update_fits_sixty_four_bytes() {
    let tables = vec![
        key_table(
            "root",
            &[
                "MouseDown1Pane",
                "MouseDown1Status",
                "MouseDown3Pane",
                "MouseDrag1Pane",
                "MouseDrag1Border",
                "WheelUpPane",
                "WheelDownPane",
                "DoubleClick1Pane",
                "TripleClick1Pane",
            ],
        ),
        key_table(
            "copy-mode",
            &[
                "MouseDrag1Pane",
                "MouseDragEnd1Pane",
                "WheelUpPane",
                "WheelDownPane",
            ],
        ),
        key_table(
            "copy-mode-vi",
            &[
                "MouseDrag1Pane",
                "MouseDragEnd1Pane",
                "WheelUpPane",
                "WheelDownPane",
            ],
        ),
    ];
    let message = ProtocolMessage::Event(Event {
        sequence: 127,
        payload: EventPayload::KeyTablesHashChanged {
            hash: u64::MAX,
            mouse: MouseBindings::from_tables(&tables),
        },
    });
    let frame = encode_protocol_message(&message).unwrap();
    assert!(frame.len() <= 64, "hash update bytes: {}", frame.len());
    assert_eq!(decode_protocol_frame(&frame).unwrap(), message);
}

#[test]
fn batch_carries_terminal_frames_and_rejects_nested_groups() {
    let messages = vec![
        ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::Snapshot(snapshot()),
        }),
        ProtocolMessage::Event(Event {
            sequence: 2,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(7),
                viewport: zz_terminal::TerminalViewport::blank(
                    80,
                    24,
                    zz_terminal::SessionStatus::Running,
                ),
            },
        }),
    ];
    let batch = Batch::from_messages(2, messages.clone()).unwrap();
    assert_eq!(batch.frames[1][4], 1);
    assert_eq!(batch.messages().unwrap(), messages);
    let wire = ProtocolMessage::Batch(batch.clone());
    assert_eq!(
        decode_protocol_frame(&encode_protocol_message(&wire).unwrap()).unwrap(),
        wire
    );
    assert!(Batch::from_messages(3, [wire.clone()]).is_err());
    let nested = Batch {
        sequence: 3,
        frames: vec![encode_protocol_message(&wire).unwrap()],
    };
    assert!(nested.messages().is_err());
}

#[test]
fn direct_control_guard_keeps_non_utf8_output_bytes() {
    let message = ProtocolMessage::Event(Event {
        sequence: 1,
        payload: EventPayload::ControlCommandGuardRaw {
            output: RawText::from(&b"value=\xff\n"[..]),
            error: false,
            sticky_failure: false,
            flags: 1,
        },
    });
    let frame = encode_protocol_message(&message).unwrap();
    assert_eq!(decode_protocol_frame(&frame).unwrap(), message);
}
