use crate::connection::Connection;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, Render, Window, div,
    prelude::*, px,
};
use zz_protocol::PaneId;
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName,
    pane::{pane_picker_choices, pane_picker_row},
};

const CHOICES: [(&str, &str, IconName, &str, bool); 4] = [
    ("Terminal", "terminal", IconName::SquareTerminal, "t", true),
    ("Browser", "browser", IconName::BrandChrome, "b", false),
    ("Editor", "editor", IconName::File, "e", false),
    ("Agent", "agent", IconName::RobotFace, "a", true),
];

pub(super) struct PanePicker {
    pane: PaneId,
    connection: Entity<Connection>,
    focus: FocusHandle,
    selected: usize,
}

impl PanePicker {
    pub(super) fn new(
        pane: PaneId,
        connection: Entity<Connection>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            pane,
            connection,
            focus: cx.focus_handle(),
            selected: 0,
        }
    }

    fn activate(&self, index: usize, cx: &mut Context<Self>) {
        if !CHOICES[index].4
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
                self.selected = if self.selected == 0 { 3 } else { 0 };
                cx.notify();
            }
            "enter" => self.activate(self.selected, cx),
            "escape" => self.connection.update(cx, |connection, cx| {
                connection.command("kill-pane", vec!["-t".into(), self.pane.to_string()], cx);
            }),
            shortcut => {
                let Some(index) = CHOICES
                    .iter()
                    .position(|choice| choice.3 == shortcut && choice.4)
                else {
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
        let rows = CHOICES
            .iter()
            .enumerate()
            .map(|(index, (title, _, icon, shortcut, supported))| {
                pane_picker_row(
                    ("web-pane-choice", index),
                    title,
                    icon.clone(),
                    shortcut,
                    self.selected == index,
                    writable && *supported,
                    cx,
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if CHOICES[index].4 && this.selected != index {
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
                            vec!["-t".into(), this.pane.to_string()],
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
