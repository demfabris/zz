use serde::{Deserialize, Serialize};

use crate::{
    LayoutNode, MuxSnapshot, PaneBorderIndicators, PaneBorderLines, PaneBorderStatus, PaneId,
    PaneKindSnapshot, PaneMode, PaneSnapshot, SessionId, SessionSnapshot, SessionViewer,
    TmuxColour, WindowId, WindowSnapshot,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TreeOp {
    UpsertSession(SessionSnapshot),
    RemoveSession(SessionId),
    SessionName {
        session: SessionId,
        name: String,
    },
    SessionActiveWindow {
        session: SessionId,
        window: WindowId,
    },
    SessionViewers {
        session: SessionId,
        viewers: Vec<SessionViewer>,
    },
    UpsertWindow {
        session: SessionId,
        window: WindowSnapshot,
    },
    RemoveWindow {
        session: SessionId,
        window: WindowId,
    },
    WindowName {
        session: SessionId,
        window: WindowId,
        name: String,
    },
    WindowActivePane {
        session: SessionId,
        window: WindowId,
        pane: PaneId,
    },
    WindowLayout {
        session: SessionId,
        window: WindowId,
        layout: LayoutNode,
        zoomed_pane: Option<PaneId>,
        layout_dump: String,
        visible_layout_dump: String,
        pane_order: Vec<PaneId>,
        pane_z_order: Vec<PaneId>,
    },
    WindowFlags {
        session: SessionId,
        window: WindowId,
        index: u32,
        automatic_rename: bool,
        activity: bool,
        silence: bool,
    },
    WindowPresentation {
        session: SessionId,
        window: WindowId,
        #[serde(deserialize_with = "deserialize_window_status_label")]
        status_label: String,
        pane_border_status: PaneBorderStatus,
        pane_border_lines: PaneBorderLines,
        pane_border_indicators: PaneBorderIndicators,
    },
    UpsertPane {
        session: SessionId,
        window: WindowId,
        pane: PaneSnapshot,
    },
    RemovePane {
        session: SessionId,
        window: WindowId,
        pane: PaneId,
    },
    PaneTitle {
        session: SessionId,
        window: WindowId,
        pane: PaneId,
        title: String,
    },
    PaneState {
        session: SessionId,
        window: WindowId,
        pane: PaneId,
        kind: PaneKindSnapshot,
        synchronized_input: bool,
        bell: bool,
        dead: bool,
        dead_status: Option<u32>,
    },
    PanePresentation {
        session: SessionId,
        window: WindowId,
        pane: PaneId,
        border_colour: Option<TmuxColour>,
        active_border_colour: Option<TmuxColour>,
        border_status_text: String,
        mode: Option<PaneMode>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TreeDelta {
    pub base: u64,
    pub version: u64,
    pub ops: Vec<TreeOp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeDeltaError {
    BaseMismatch { expected: u64, received: u64 },
    MissingSession(SessionId),
    MissingWindow(WindowId),
    MissingPane(PaneId),
}

fn deserialize_window_status_label<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    crate::message::deserialize_bounded_text(deserializer, crate::MAX_WINDOW_STATUS_LABEL_BYTES)
}

impl TreeDelta {
    #[must_use]
    pub fn from_snapshot(snapshot: &MuxSnapshot) -> Self {
        Self::between(&MuxSnapshot::default(), snapshot)
    }

    #[must_use]
    pub fn between(before: &MuxSnapshot, after: &MuxSnapshot) -> Self {
        let mut ops = Vec::new();
        for session in &before.sessions {
            if !after.sessions.iter().any(|next| next.id == session.id) {
                ops.push(TreeOp::RemoveSession(session.id));
            }
        }
        for next in &after.sessions {
            let Some(previous) = before.sessions.iter().find(|session| session.id == next.id)
            else {
                ops.push(TreeOp::UpsertSession(next.clone()));
                continue;
            };
            if previous.name != next.name {
                ops.push(TreeOp::SessionName {
                    session: next.id,
                    name: next.name.clone(),
                });
            }
            if previous.active_window != next.active_window {
                ops.push(TreeOp::SessionActiveWindow {
                    session: next.id,
                    window: next.active_window,
                });
            }
            if previous.viewers != next.viewers {
                ops.push(TreeOp::SessionViewers {
                    session: next.id,
                    viewers: next.viewers.clone(),
                });
            }
            for window in &previous.windows {
                if !next
                    .windows
                    .iter()
                    .any(|candidate| candidate.id == window.id)
                {
                    ops.push(TreeOp::RemoveWindow {
                        session: next.id,
                        window: window.id,
                    });
                }
            }
            for window in &next.windows {
                let Some(old) = previous
                    .windows
                    .iter()
                    .find(|candidate| candidate.id == window.id)
                else {
                    ops.push(TreeOp::UpsertWindow {
                        session: next.id,
                        window: window.clone(),
                    });
                    continue;
                };
                window_diff(next.id, old, window, &mut ops);
            }
        }
        Self {
            base: before.generation,
            version: after.generation,
            ops,
        }
    }

    pub fn apply(&self, snapshot: &mut MuxSnapshot) -> Result<(), TreeDeltaError> {
        if snapshot.generation != self.base {
            return Err(TreeDeltaError::BaseMismatch {
                expected: snapshot.generation,
                received: self.base,
            });
        }
        Self::apply_overlay(snapshot, &self.ops)?;
        snapshot.generation = self.version;
        Ok(())
    }

    pub fn apply_overlay(snapshot: &mut MuxSnapshot, ops: &[TreeOp]) -> Result<(), TreeDeltaError> {
        if ops.is_empty() {
            return Ok(());
        }
        let mut next = snapshot.clone();
        for op in ops {
            apply_op(&mut next, op)?;
        }
        next.sessions.sort_by_key(|session| session.id);
        for session in &mut next.sessions {
            session
                .windows
                .sort_by_key(|window| (window.index, window.id));
        }
        *snapshot = next;
        Ok(())
    }
}

fn window_diff(
    session: SessionId,
    old: &WindowSnapshot,
    next: &WindowSnapshot,
    ops: &mut Vec<TreeOp>,
) {
    let window = next.id;
    if old.name != next.name {
        ops.push(TreeOp::WindowName {
            session,
            window,
            name: next.name.clone(),
        });
    }
    if old.active_pane != next.active_pane {
        ops.push(TreeOp::WindowActivePane {
            session,
            window,
            pane: next.active_pane,
        });
    }
    if old.layout != next.layout
        || old.zoomed_pane != next.zoomed_pane
        || old.layout_dump != next.layout_dump
        || old.visible_layout_dump != next.visible_layout_dump
        || old.pane_order != next.pane_order
        || old.pane_z_order != next.pane_z_order
    {
        ops.push(TreeOp::WindowLayout {
            session,
            window,
            layout: next.layout.clone(),
            zoomed_pane: next.zoomed_pane,
            layout_dump: next.layout_dump.clone(),
            visible_layout_dump: next.visible_layout_dump.clone(),
            pane_order: next.pane_order.clone(),
            pane_z_order: next.pane_z_order.clone(),
        });
    }
    if old.index != next.index
        || old.automatic_rename != next.automatic_rename
        || old.activity != next.activity
        || old.silence != next.silence
    {
        ops.push(TreeOp::WindowFlags {
            session,
            window,
            index: next.index,
            automatic_rename: next.automatic_rename,
            activity: next.activity,
            silence: next.silence,
        });
    }
    if old.status_label != next.status_label
        || old.pane_border_status != next.pane_border_status
        || old.pane_border_lines != next.pane_border_lines
        || old.pane_border_indicators != next.pane_border_indicators
    {
        ops.push(TreeOp::WindowPresentation {
            session,
            window,
            status_label: next.status_label.clone(),
            pane_border_status: next.pane_border_status,
            pane_border_lines: next.pane_border_lines,
            pane_border_indicators: next.pane_border_indicators,
        });
    }
    for pane in old.panes.keys() {
        if !next.panes.contains_key(pane) {
            ops.push(TreeOp::RemovePane {
                session,
                window,
                pane: *pane,
            });
        }
    }
    for pane in next.panes.values() {
        let Some(previous) = old.panes.get(&pane.id) else {
            ops.push(TreeOp::UpsertPane {
                session,
                window,
                pane: pane.clone(),
            });
            continue;
        };
        if previous.title != pane.title {
            ops.push(TreeOp::PaneTitle {
                session,
                window,
                pane: pane.id,
                title: pane.title.clone(),
            });
        }
        if previous.kind != pane.kind
            || previous.synchronized_input != pane.synchronized_input
            || previous.bell != pane.bell
            || previous.dead != pane.dead
            || previous.dead_status != pane.dead_status
        {
            ops.push(TreeOp::PaneState {
                session,
                window,
                pane: pane.id,
                kind: pane.kind.clone(),
                synchronized_input: pane.synchronized_input,
                bell: pane.bell,
                dead: pane.dead,
                dead_status: pane.dead_status,
            });
        }
        if previous.border_colour != pane.border_colour
            || previous.active_border_colour != pane.active_border_colour
            || previous.border_status_text != pane.border_status_text
            || previous.mode != pane.mode
        {
            ops.push(TreeOp::PanePresentation {
                session,
                window,
                pane: pane.id,
                border_colour: pane.border_colour,
                active_border_colour: pane.active_border_colour,
                border_status_text: pane.border_status_text.clone(),
                mode: pane.mode.clone(),
            });
        }
    }
}

fn session_mut(
    snapshot: &mut MuxSnapshot,
    id: SessionId,
) -> Result<&mut SessionSnapshot, TreeDeltaError> {
    snapshot
        .sessions
        .iter_mut()
        .find(|session| session.id == id)
        .ok_or(TreeDeltaError::MissingSession(id))
}

fn window_mut(
    snapshot: &mut MuxSnapshot,
    session: SessionId,
    id: WindowId,
) -> Result<&mut WindowSnapshot, TreeDeltaError> {
    session_mut(snapshot, session)?
        .windows
        .iter_mut()
        .find(|window| window.id == id)
        .ok_or(TreeDeltaError::MissingWindow(id))
}

fn pane_mut(
    snapshot: &mut MuxSnapshot,
    session: SessionId,
    window: WindowId,
    id: PaneId,
) -> Result<&mut PaneSnapshot, TreeDeltaError> {
    window_mut(snapshot, session, window)?
        .panes
        .get_mut(&id)
        .ok_or(TreeDeltaError::MissingPane(id))
}

fn apply_op(snapshot: &mut MuxSnapshot, op: &TreeOp) -> Result<(), TreeDeltaError> {
    match op {
        TreeOp::UpsertSession(next) => {
            if let Some(session) = snapshot
                .sessions
                .iter_mut()
                .find(|session| session.id == next.id)
            {
                *session = next.clone();
            } else {
                snapshot.sessions.push(next.clone());
            }
        }
        TreeOp::RemoveSession(id) => snapshot.sessions.retain(|session| session.id != *id),
        TreeOp::SessionName { session, name } => {
            session_mut(snapshot, *session)?.name.clone_from(name);
        }
        TreeOp::SessionActiveWindow { session, window } => {
            session_mut(snapshot, *session)?.active_window = *window;
        }
        TreeOp::SessionViewers { session, viewers } => {
            session_mut(snapshot, *session)?.viewers.clone_from(viewers);
        }
        TreeOp::UpsertWindow {
            session,
            window: next,
        } => {
            let windows = &mut session_mut(snapshot, *session)?.windows;
            if let Some(window) = windows.iter_mut().find(|window| window.id == next.id) {
                *window = next.clone();
            } else {
                windows.push(next.clone());
            }
        }
        TreeOp::RemoveWindow { session, window } => session_mut(snapshot, *session)?
            .windows
            .retain(|candidate| candidate.id != *window),
        TreeOp::WindowName {
            session,
            window,
            name,
        } => window_mut(snapshot, *session, *window)?
            .name
            .clone_from(name),
        TreeOp::WindowActivePane {
            session,
            window,
            pane,
        } => window_mut(snapshot, *session, *window)?.active_pane = *pane,
        TreeOp::WindowLayout {
            session,
            window,
            layout,
            zoomed_pane,
            layout_dump,
            visible_layout_dump,
            pane_order,
            pane_z_order,
        } => {
            let window = window_mut(snapshot, *session, *window)?;
            window.layout.clone_from(layout);
            window.zoomed_pane = *zoomed_pane;
            window.layout_dump.clone_from(layout_dump);
            window.visible_layout_dump.clone_from(visible_layout_dump);
            window.pane_order.clone_from(pane_order);
            window.pane_z_order.clone_from(pane_z_order);
        }
        TreeOp::WindowFlags {
            session,
            window,
            index,
            automatic_rename,
            activity,
            silence,
        } => {
            let window = window_mut(snapshot, *session, *window)?;
            window.index = *index;
            window.automatic_rename = *automatic_rename;
            window.activity = *activity;
            window.silence = *silence;
        }
        TreeOp::WindowPresentation {
            session,
            window,
            status_label,
            pane_border_status,
            pane_border_lines,
            pane_border_indicators,
        } => {
            let window = window_mut(snapshot, *session, *window)?;
            window.status_label.clone_from(status_label);
            window.pane_border_status = *pane_border_status;
            window.pane_border_lines = *pane_border_lines;
            window.pane_border_indicators = *pane_border_indicators;
        }
        TreeOp::UpsertPane {
            session,
            window,
            pane,
        } => {
            window_mut(snapshot, *session, *window)?
                .panes
                .insert(pane.id, pane.clone());
        }
        TreeOp::RemovePane {
            session,
            window,
            pane,
        } => {
            window_mut(snapshot, *session, *window)?.panes.remove(pane);
        }
        TreeOp::PaneTitle {
            session,
            window,
            pane,
            title,
        } => pane_mut(snapshot, *session, *window, *pane)?
            .title
            .clone_from(title),
        TreeOp::PaneState {
            session,
            window,
            pane,
            kind,
            synchronized_input,
            bell,
            dead,
            dead_status,
        } => {
            let pane = pane_mut(snapshot, *session, *window, *pane)?;
            pane.kind.clone_from(kind);
            pane.synchronized_input = *synchronized_input;
            pane.bell = *bell;
            pane.dead = *dead;
            pane.dead_status = *dead_status;
        }
        TreeOp::PanePresentation {
            session,
            window,
            pane,
            border_colour,
            active_border_colour,
            border_status_text,
            mode,
        } => {
            let pane = pane_mut(snapshot, *session, *window, *pane)?;
            pane.border_colour = *border_colour;
            pane.active_border_colour = *active_border_colour;
            pane.border_status_text.clone_from(border_status_text);
            pane.mode.clone_from(mode);
        }
    }
    Ok(())
}

impl crate::ClientView {
    pub fn apply(&self, snapshot: &mut MuxSnapshot) -> Result<(), TreeDeltaError> {
        TreeDelta::apply_overlay(snapshot, &self.overlay)?;
        snapshot.focused_window = self.focused_window;
        Ok(())
    }

    pub fn apply_owned(&self, mut snapshot: MuxSnapshot) -> Result<MuxSnapshot, TreeDeltaError> {
        for op in &self.overlay {
            apply_op(&mut snapshot, op)?;
        }
        snapshot.focused_window = self.focused_window;
        Ok(snapshot)
    }
}
