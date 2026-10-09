use zz_protocol::{Axis, PaneId};

use crate::{
    layout::{CellGeometry, LayoutFormat},
    model::{FloatSpawn, FloatZ, MuxState, PaneKind},
};

fn geometry(sx: u16, sy: u16, xoff: i32, yoff: i32) -> CellGeometry {
    CellGeometry { sx, sy, xoff, yoff }
}

fn float(state: &mut MuxState, target: PaneId, cell: CellGeometry) -> PaneId {
    state
        .float_pane_with(
            target,
            PaneKind::Terminal,
            &FloatSpawn {
                geometry: cell,
                ..FloatSpawn::default()
            },
        )
        .unwrap()
}

fn layout(state: &MuxState, format: LayoutFormat) -> String {
    let window = state.windows.values().next().unwrap();
    window.layout_string(format, 0)
}

#[test]
fn two_floats_sit_beside_the_root_tile_in_a_top_bottom_node() {
    let mut state = MuxState::default();
    let (_, window, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let a = float(&mut state, first, geometry(40, 6, 4, 2));
    let b = float(&mut state, a, geometry(40, 6, 8, 4));
    assert!(state.validate().is_ok());
    assert_eq!(
        layout(&state, LayoutFormat::V2),
        concat!(
            r#"{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":["#,
            r#"{"t":"p","w":80,"h":24,"x":0,"y":0,"l":1,"i":0,"I":"%0"},"#,
            r#"{"t":"p","w":40,"h":6,"x":4,"y":2,"l":0,"i":1,"z":1,"I":"%1"},"#,
            r#"{"t":"p","w":40,"h":6,"x":8,"y":4,"a":true,"i":2,"z":0,"I":"%2"}]}}"#
        )
    );
    assert_eq!(layout(&state, LayoutFormat::V1), "b25d,80x24,0,0,0");
    let window = &state.windows[&window];
    assert_eq!(window.z_order(), &[b, a, first]);
    assert_eq!(
        [first, a, b].map(|pane| window.pane_z(pane).unwrap()),
        [3, 1, 0]
    );
    assert_eq!(
        window.layout.project(),
        zz_protocol::LayoutNode::Pane(first)
    );
}

#[test]
fn a_float_targeting_a_split_tile_goes_after_that_tile() {
    let mut state = MuxState::default();
    let (_, _, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let right = state
        .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
        .unwrap();
    state.select_pane(first).unwrap();
    let floated = float(&mut state, first, geometry(40, 6, 12, 6));
    assert!(state.validate().is_ok());
    let dump = layout(&state, LayoutFormat::V2);
    let tiles = dump.find("\"I\":\"%0\"").unwrap();
    let new = dump.find(&format!("\"I\":\"%{}\"", floated.0)).unwrap();
    let other = dump.find(&format!("\"I\":\"%{}\"", right.0)).unwrap();
    assert!(tiles < new && new < other);
    assert_eq!(
        layout(&state, LayoutFormat::V1),
        "8205,80x24,0,0{40x24,0,0,0,39x24,41,0,1}"
    );
}

#[test]
fn breaking_and_tiling_restores_a_tile_beside_the_whole_column() {
    let mut state = MuxState::default();
    let (_, window, a) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let b = state
        .split_pane(a, Axis::Horizontal, PaneKind::Terminal)
        .unwrap();
    state
        .split_pane(b, Axis::Vertical, PaneKind::Terminal)
        .unwrap();
    let before = layout(&state, LayoutFormat::V1);
    state
        .float_existing(a, geometry(20, 6, 11, 4), false)
        .unwrap();
    assert!(state.validate().is_ok());
    assert_eq!(
        layout(&state, LayoutFormat::V1),
        "419a,80x24,0,0[80x12,0,0,1,80x11,0,13,2]"
    );
    assert_eq!(state.windows[&window].active_pane, a);
    assert_eq!(state.windows[&window].z_order()[0], a);
    state.tile_floating(a, false).unwrap();
    assert!(state.validate().is_ok());
    assert_eq!(layout(&state, LayoutFormat::V1), before);
    assert_eq!(
        state.windows[&window].layout.saved_float(a),
        Some(geometry(20, 6, 11, 4))
    );
    assert_eq!(state.windows[&window].z_order().last(), Some(&a));
}

#[test]
fn killing_the_last_tile_leaves_a_window_of_floats() {
    let mut state = MuxState::default();
    let (_, window, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let a = float(&mut state, first, geometry(40, 6, 4, 2));
    let b = float(&mut state, a, geometry(40, 6, 8, 4));
    state.kill_pane(first).unwrap();
    assert!(state.validate().is_ok());
    let state_window = &state.windows[&window];
    assert!(!state_window.layout.has_tiled());
    assert_eq!(state_window.layout.extent(), (80, 24));
    assert_eq!(layout(&state, LayoutFormat::V1), "0000,");
    state.kill_pane(b).unwrap();
    assert!(state.validate().is_ok());
    assert_eq!(
        layout(&state, LayoutFormat::V2),
        r#"{"V":2,"L":{"t":"p","w":40,"h":6,"x":4,"y":2,"a":true,"i":0,"z":0,"I":"%1"}}"#
    );
    state.tile_floating(a, false).unwrap();
    assert_eq!(layout(&state, LayoutFormat::V1), "b25e,80x24,0,0,1");
    state.kill_pane(a).unwrap();
    assert!(!state.windows.contains_key(&window));
}

#[test]
fn killing_a_float_gives_no_space_to_the_tiles() {
    let mut state = MuxState::default();
    let (_, _, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let second = state
        .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
        .unwrap();
    let before = layout(&state, LayoutFormat::V1);
    let floated = float(&mut state, second, geometry(10, 5, 3, 3));
    state.kill_pane(floated).unwrap();
    assert!(state.validate().is_ok());
    assert_eq!(layout(&state, LayoutFormat::V1), before);
}

#[test]
fn a_v2_string_with_floats_round_trips_through_select_layout() {
    let mut state = MuxState::default();
    let (_, window, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let a = float(&mut state, first, geometry(40, 6, 4, 2));
    float(&mut state, a, geometry(30, 5, -3, 4));
    state.select_pane(a).unwrap();
    let dump = layout(&state, LayoutFormat::V2);
    let z = state.windows[&window].z_order().to_vec();
    state.move_float_z(a, FloatZ::Back).unwrap();
    state.select_pane(first).unwrap();
    state.select_layout_string(window, &dump).unwrap();
    assert!(state.validate().is_ok());
    assert_eq!(layout(&state, LayoutFormat::V2), dump);
    assert_eq!(state.windows[&window].z_order(), z.as_slice());
}

#[test]
fn a_v1_string_keeps_existing_floats_at_the_root() {
    let mut state = MuxState::default();
    let (_, window, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let second = state
        .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
        .unwrap();
    let floated = float(&mut state, second, geometry(10, 5, 3, 3));
    state
        .select_layout_string(window, "c195,80x24,0,0[80x12,0,0,0,80x11,0,13,1]")
        .unwrap();
    assert!(state.validate().is_ok());
    let dump = layout(&state, LayoutFormat::V2);
    assert!(dump.starts_with(
        r#"{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":12"#
    ));
    assert!(dump.ends_with(&format!(
        r#"{{"t":"p","w":10,"h":5,"x":3,"y":3,"a":true,"i":2,"z":0,"I":"%{}"}}]}}}}"#,
        floated.0
    )));
}

#[test]
fn presets_keep_floats_and_skip_them_when_sizing() {
    let mut state = MuxState::default();
    let (_, window, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let second = state
        .split_pane(first, Axis::Vertical, PaneKind::Terminal)
        .unwrap();
    let floated = float(&mut state, second, geometry(10, 5, 3, 3));
    state
        .select_layout(
            window,
            crate::model::LayoutPreset::EvenHorizontal,
            &crate::PresetOptions::default(),
        )
        .unwrap();
    assert!(state.validate().is_ok());
    assert_eq!(
        layout(&state, LayoutFormat::V1),
        "8205,80x24,0,0{40x24,0,0,0,39x24,41,0,1}"
    );
    assert!(state.windows[&window].is_floating(floated));
    assert_eq!(
        state.windows[&window].layout.pane_geometry(floated),
        Some(geometry(10, 5, 3, 3))
    );
}

#[test]
fn window_shrink_clamps_floats_like_tmux() {
    let mut state = MuxState::default();
    let (_, window, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let a = float(&mut state, first, geometry(40, 6, 30, 15));
    let b = float(&mut state, first, geometry(60, 20, -4, 2));
    state.resize_window(window, 30, 10).unwrap();
    let layout = &state.windows[&window].layout;
    assert_eq!(layout.pane_geometry(a), Some(geometry(28, 6, 1, 3)));
    assert_eq!(layout.pane_geometry(b), Some(geometry(28, 8, -4, 1)));
}

#[test]
fn modal_close_falls_back_like_window_lost_pane() {
    let mut state = MuxState::default();
    let (_, window, first) = state.create_session_with_extent("s", (80, 24)).unwrap();
    let second = state
        .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
        .unwrap();
    state.select_pane(first).unwrap();
    let modal = state
        .float_pane_with(
            first,
            PaneKind::Terminal,
            &FloatSpawn {
                geometry: geometry(20, 5, 4, 2),
                over_zoom: true,
                modal: Some(crate::model::Modal::default()),
                ..FloatSpawn::default()
            },
        )
        .unwrap();
    assert_eq!(state.windows[&window].modal_pane(), Some(modal));
    state.select_pane(second).unwrap();
    assert_eq!(state.windows[&window].active_pane, modal);
    state.kill_pane(modal).unwrap();
    assert_eq!(state.windows[&window].active_pane, first);
    assert_eq!(state.windows[&window].modal_pane(), None);

    let modal = state
        .float_pane_with(
            second,
            PaneKind::Terminal,
            &FloatSpawn {
                geometry: geometry(20, 5, 4, 2),
                modal: Some(crate::model::Modal::default()),
                ..FloatSpawn::default()
            },
        )
        .unwrap();
    state.kill_pane(first).unwrap();
    state.kill_pane(modal).unwrap();
    assert!(state.validate().is_ok());
    assert_eq!(state.windows[&window].active_pane, second);
}
