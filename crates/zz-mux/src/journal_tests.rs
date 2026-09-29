use zz_protocol::Axis;

use crate::{MuxState, PaneKind};

#[test]
fn nothing_is_recorded_while_no_window_is_open() {
    let mut state = MuxState::default();
    let (session, window, _) = state.create_session("alpha").expect("session");
    state.rename_window(window, "renamed").expect("rename");
    state.rename_session(session, "beta").expect("rename");
    let opened = state.open_change_window();
    let changes = state.changes_since(&opened);
    assert!(changes.sessions.is_empty());
    assert!(changes.windows.is_empty());
}

#[test]
fn a_window_sees_the_state_each_entity_had_when_it_opened() {
    let mut state = MuxState::default();
    let (session, window, pane) = state.create_session("alpha").expect("session");
    let opened = state.open_change_window();
    state.rename_window(window, "first").expect("rename");
    state.rename_window(window, "second").expect("rename");
    let split = state
        .split_pane(pane, Axis::Horizontal, PaneKind::Terminal)
        .expect("split");
    let (created, _) = state
        .create_window(session, Some("new".to_owned()), PaneKind::Terminal)
        .expect("window");
    let changes = state.changes_since(&opened);
    let before = changes.windows[&window].expect("window existed");
    assert_eq!(before.name, "0");
    assert_eq!(before.active_pane, pane);
    assert_eq!(
        before.panes.iter().map(|pane| pane.id).collect::<Vec<_>>(),
        [pane]
    );
    assert!(changes.windows[&created].is_none());
    let session_before = changes.sessions[&session].expect("session existed");
    assert_eq!(session_before.windows, [window]);
    assert_eq!(session_before.active_window, window);
    assert!(state.windows[&window].panes.contains_key(&split));
}

#[test]
fn nested_windows_keep_their_own_marks() {
    let mut state = MuxState::default();
    let (_, window, _) = state.create_session("alpha").expect("session");
    let outer = state.open_change_window();
    state.rename_window(window, "outer").expect("rename");
    let inner = state.open_change_window();
    state.rename_window(window, "inner").expect("rename");
    assert_eq!(
        state.changes_since(&outer).windows[&window]
            .expect("existed")
            .name,
        "0"
    );
    assert_eq!(
        state.changes_since(&inner).windows[&window]
            .expect("existed")
            .name,
        "outer"
    );
    drop(inner);
    state.rename_window(window, "after").expect("rename");
    assert_eq!(
        state.changes_since(&outer).windows[&window]
            .expect("existed")
            .name,
        "0"
    );
}

#[test]
fn a_dropped_window_stops_the_recording() {
    let mut state = MuxState::default();
    let (session, window, _) = state.create_session("alpha").expect("session");
    drop(state.open_change_window());
    state.rename_window(window, "unwatched").expect("rename");
    let opened = state.open_change_window();
    assert!(state.changes_since(&opened).windows.is_empty());
    state.kill_session(session).expect("kill");
    let changes = state.changes_since(&opened);
    assert_eq!(changes.sessions[&session].expect("existed").name, "alpha");
    assert_eq!(changes.windows[&window].expect("existed").name, "unwatched");
}
