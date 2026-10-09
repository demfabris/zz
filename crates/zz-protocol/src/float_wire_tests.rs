use std::collections::BTreeMap;

use crate::*;

fn window() -> WindowSnapshot {
    let pane = PaneId(7);
    WindowSnapshot {
        id: WindowId(5),
        index: 0,
        name: "window".to_owned(),
        automatic_rename: true,
        active_pane: pane,
        zoomed_pane: None,
        layout: LayoutNode::Empty,
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
                status: None,
            },
        )]),
        layout_dump: String::new(),
        visible_layout_dump: String::new(),
        status_label: String::new(),
        activity: false,
        silence: false,
        pane_border_status: PaneBorderStatus::Off,
        pane_border_lines: PaneBorderLines::Single,
        pane_border_indicators: PaneBorderIndicators::Colour,
        pane_order: vec![pane],
        pane_z_order: vec![pane],
        floating: vec![FloatingPaneSnapshot {
            pane,
            xoff: -3,
            yoff: 2,
            sx: 20,
            sy: 6,
            visible: true,
            border_lines: PaneBorderLines::Single,
            border_status: PaneBorderStatus::Top,
        }],
        modal: Some(ModalPaneSnapshot {
            pane,
            capture_keys: true,
            close_on_click: false,
            close_on_cancel: true,
        }),
    }
}

fn snapshot(window: WindowSnapshot) -> MuxSnapshot {
    MuxSnapshot {
        generation: 1,
        focused_window: None,
        sessions: vec![SessionSnapshot {
            id: SessionId(3),
            name: "session".to_owned(),
            active_window: window.id,
            viewers: Vec::new(),
            windows: vec![window],
        }],
    }
}

const FLOATING_AND_MODAL_TAIL: [u8; 14] = [1, 7, 5, 4, 20, 6, 1, 0, 1, 1, 7, 1, 0, 1];

#[test]
fn empty_layout_holds_tag_two() {
    let bytes = postcard::to_stdvec(&LayoutNode::Empty).unwrap();
    assert_eq!(bytes, [2]);
    assert_eq!(
        postcard::from_bytes::<LayoutNode>(&bytes).unwrap(),
        LayoutNode::Empty
    );
    assert_eq!(
        serde_json::to_string(&LayoutNode::Empty).unwrap(),
        "\"Empty\""
    );
}

#[test]
fn window_snapshot_appends_floating_then_modal() {
    let window = window();
    let bytes = postcard::to_stdvec(&window).unwrap();
    assert!(bytes.ends_with(&FLOATING_AND_MODAL_TAIL));
    assert_eq!(
        postcard::from_bytes::<WindowSnapshot>(&bytes).unwrap(),
        window
    );
    let mut json = serde_json::to_value(&window).unwrap();
    let object = json.as_object_mut().unwrap();
    object.remove("floating");
    object.remove("modal");
    let old = serde_json::from_value::<WindowSnapshot>(json).unwrap();
    assert!(old.floating.is_empty());
    assert_eq!(old.modal, None);
}

#[test]
fn window_layout_delta_carries_floating_and_modal() {
    let mut before = window();
    before.floating.clear();
    before.modal = None;
    let after = window();
    let before = snapshot(before);
    let after = snapshot(after);
    let delta = TreeDelta::between(&before, &after);
    let layouts = delta
        .ops
        .iter()
        .filter(|op| matches!(op, TreeOp::WindowLayout { .. }))
        .collect::<Vec<_>>();
    assert_eq!(layouts.len(), 1);
    let bytes = postcard::to_stdvec(layouts[0]).unwrap();
    assert!(bytes.ends_with(&FLOATING_AND_MODAL_TAIL));
    assert_eq!(&postcard::from_bytes::<TreeOp>(&bytes).unwrap(), layouts[0]);
    let mut applied = before;
    delta.apply(&mut applied).unwrap();
    assert_eq!(applied.sessions, after.sessions);
}

#[test]
fn mouse_key_appends_the_press_cell() {
    let message = InputMessage::MouseKey {
        key: "MouseDrag1Pane".to_owned(),
        pane: None,
        window: Some(WindowId(5)),
        column: 9,
        row: 4,
        border: None,
        view_action: None,
        press_action: None,
        status_range_start: None,
        press: Some((3, 2)),
    };
    let bytes = postcard::to_stdvec(&message).unwrap();
    assert!(bytes.ends_with(&[0, 1, 3, 2]));
    assert_eq!(
        postcard::from_bytes::<InputMessage>(&bytes).unwrap(),
        message
    );
}
