use crate::connection::Connection;
use gpui::{
    App, Context, Corners, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, Pixels,
    Render, Window, div, prelude::*, px,
};
use zz_protocol::PaneId;
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName,
    pane::{pane_picker_choices, pane_picker_row},
};

const CHOICES: [(&str, &str, IconName, &str); 2] = [
    ("Terminal", "terminal", IconName::SquareTerminal, "t"),
    ("Agent", "agent", IconName::RobotFace, "a"),
];

pub(super) struct PanePicker {
    pane: PaneId,
    connection: Entity<Connection>,
    focus: FocusHandle,
    selected: usize,
    agent_enabled: bool,
    corner_radii: Corners<Pixels>,
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
        }
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
        index == 0
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
        let rows = CHOICES
            .iter()
            .enumerate()
            .filter(|(index, _)| self.enabled(*index, cx))
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
        div()
            .id(("web-pane-picker", self.pane.0))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
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
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .rounded_tl(self.corner_radii.top_left)
            .rounded_tr(self.corner_radii.top_right)
            .rounded_bl(self.corner_radii.bottom_left)
            .rounded_br(self.corner_radii.bottom_right)
            .bg(cx
                .theme()
                .background
                .opaque()
                .opacity(cx.theme().pane_background_opacity))
            .text_color(cx.theme().foreground)
            .px(px(12.0))
            .child(pane_picker_choices(rows))
    }
}
