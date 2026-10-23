use crate::connection::Connection;
use zz_gpui::{
    AnyElement, App, Context, Corners, ElementId, Entity, FocusHandle, Focusable, KeyDownEvent,
    MouseButton, Pixels, Render, Window, div, prelude::*, px,
};
use zz_protocol::PaneId;
use zz_ui::{
    Icon, IconName, StyledExt as _,
    compact::{bottom_sheet, close_sheet, sheet_close, sheet_row},
    pane::{pane_picker_row, pane_picker_surface},
    roles,
};

const CHOICES: &[(&str, &str, IconName, &str)] = &[
    ("Terminal", "terminal", IconName::SquareTerminal, "t"),
    ("Agent", "agent", IconName::RobotFace, "a"),
    #[cfg(any(target_os = "ios", target_os = "android"))]
    ("Browser", "browser", IconName::Globe, "b"),
];

pub(super) struct PanePicker {
    pane: PaneId,
    connection: Entity<Connection>,
    focus: FocusHandle,
    selected: usize,
    agent_enabled: bool,
    corner_radii: Corners<Pixels>,
    chosen: Option<usize>,
    sheet_closed: bool,
}

impl PanePicker {
    pub(super) fn new(
        pane: PaneId,
        connection: Entity<Connection>,
        agent_enabled: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            pane,
            connection,
            focus: cx.focus_handle(),
            selected: 0,
            agent_enabled,
            corner_radii: Corners::default(),
            chosen: None,
            sheet_closed: false,
        }
    }

    fn sheet_id(&self) -> ElementId {
        ("pane-picker-sheet", self.pane.0).into()
    }

    /// The choices as a drawer over the phone's page: a pick slides it away
    /// and turns the pane into what was picked; sliding it away without a
    /// pick closes the pane.
    pub(super) fn sheet(&self, bottom_inset: Pixels, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.sheet_closed {
            return None;
        }
        let id = self.sheet_id();
        let rows = CHOICES
            .iter()
            .enumerate()
            .filter(|(index, _)| self.enabled(*index, cx))
            .map(|(index, (title, _, icon, _))| {
                let id = id.clone();
                sheet_row(
                    ("pane-picker-option", index),
                    roles::PHONE_ROW,
                    Some(Icon::new(icon.clone())),
                    *title,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.chosen = Some(index);
                    close_sheet(id.clone(), cx);
                }))
                .into_any_element()
            })
            .collect::<Vec<_>>();
        let close = sheet_close("pane-picker-close").on_click({
            let id = id.clone();
            move |_, _, cx| close_sheet(id.clone(), cx)
        });
        let view = cx.weak_entity();
        Some(
            bottom_sheet(
                id,
                "New pane",
                [close.into_any_element()],
                div()
                    .flex()
                    .flex_col()
                    .list_inset(cx)
                    .pb(px(8.0))
                    .children(rows),
                bottom_inset,
                move |_, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.sheet_closed = true;
                        match this.chosen.take() {
                            Some(index) => this.activate(index, cx),
                            None => this.connection.update(cx, |connection, cx| {
                                connection.command(
                                    "kill-pane",
                                    vec!["-t".into(), this.pane.to_string()],
                                    cx,
                                );
                            }),
                        }
                        cx.notify();
                    });
                },
            )
            .surface_role(roles::NEW_PANE_DRAWER)
            .header_role(roles::PHONE_BAR)
            .into_any_element(),
        )
    }

    pub(super) fn set_corner_radii(&mut self, radii: Corners<Pixels>, cx: &mut Context<Self>) {
        if self.corner_radii != radii {
            self.corner_radii = radii;
            cx.notify();
        }
    }

    pub(super) fn set_agent_enabled(&mut self, enabled: bool) {
        self.agent_enabled = enabled;
        if !enabled {
            self.selected = 0;
        }
    }

    fn enabled(&self, index: usize, cx: &App) -> bool {
        CHOICES[index].1 != "agent"
            || (self.agent_enabled
                && crate::command_palette::agent_pane_available(&self.connection.read(cx).core))
    }

    fn activate(&self, index: usize, cx: &mut Context<Self>) {
        if !self.enabled(index, cx)
            || !self.connection.read(cx).connected
            || self.connection.read(cx).core.attached_read_only()
        {
            return;
        }
        self.connection.update(cx, |connection, cx| {
            connection.command(
                "select-pane-kind",
                vec!["-t".into(), self.pane.to_string(), CHOICES[index].1.into()],
                cx,
            );
        });
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.modifiers.control || key.modifiers.alt || key.modifiers.platform {
            return;
        }
        match key.key.as_str() {
            "down" | "j" | "up" | "k" => {
                let forward = matches!(key.key.as_str(), "down" | "j");
                for offset in 1..=CHOICES.len() {
                    let index = if forward {
                        (self.selected + offset) % CHOICES.len()
                    } else {
                        (self.selected + CHOICES.len() - offset) % CHOICES.len()
                    };
                    if self.enabled(index, cx) {
                        self.selected = index;
                        break;
                    }
                }
                cx.notify();
            }
            "enter" => self.activate(self.selected, cx),
            "escape" => self.connection.update(cx, |connection, cx| {
                connection.command("kill-pane", vec!["-t".into(), self.pane.to_string()], cx);
            }),
            shortcut => {
                let Some(index) = CHOICES.iter().enumerate().find_map(|(index, choice)| {
                    (choice.3 == shortcut && self.enabled(index, cx)).then_some(index)
                }) else {
                    return;
                };
                self.activate(index, cx);
            }
        }
        cx.stop_propagation();
    }
}

impl Focusable for PanePicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PanePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let writable = self.connection.read(cx).connected
            && !self.connection.read(cx).core.attached_read_only();
        if !self.enabled(self.selected, cx) {
            self.selected = 0;
        }
        let phone = super::settings::phone();
        let rows = CHOICES
            .iter()
            .enumerate()
            .filter(|(index, _)| !phone && self.enabled(*index, cx))
            .map(|(index, (title, _, icon, shortcut))| {
                pane_picker_row(
                    ("web-pane-choice", index),
                    title,
                    icon.clone(),
                    shortcut,
                    self.selected == index,
                    writable,
                    cx,
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.enabled(index, cx) && this.selected != index {
                        this.selected = index;
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.activate(index, cx);
                        cx.stop_propagation();
                    }),
                )
                .into_any_element()
            })
            .collect::<Vec<_>>();
        pane_picker_surface(
            ("web-pane-picker", self.pane.0),
            self.corner_radii,
            rows,
            cx,
        )
        .track_focus(&self.focus)
        .on_key_down(cx.listener(Self::key_down))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.sheet_closed = false;
                this.connection.update(cx, |connection, cx| {
                    connection.command(
                        "select-pane",
                        vec!["-Z".into(), "-t".into(), this.pane.to_string()],
                        cx,
                    );
                });
                this.focus.focus(window, cx);
            }),
        )
    }
}
