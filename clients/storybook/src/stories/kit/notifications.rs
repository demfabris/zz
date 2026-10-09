use std::cell::Cell;

use zpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, KeyBinding, MouseButton,
    ParentElement as _, Render, Styled as _, Subscription, Window, actions, div, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    notification::Notification,
    popover::Popover,
    tooltip::Tooltip,
    v_flex,
};

use super::support::{Probe, after_first_frame, hover, nudge, press, when_visible};
use crate::story::{Section, Story, row, states};

actions!(storybook_tooltip, [SplitRight, CopyPath]);

thread_local! {
    static BOUND: Cell<bool> = const { Cell::new(false) };
}

fn bind_keys(cx: &mut App) {
    if BOUND.with(|bound| bound.replace(true)) {
        return;
    }
    cx.bind_keys([
        KeyBinding::new("cmd-d", SplitRight, None),
        KeyBinding::new("cmd-alt-c", CopyPath, None),
    ]);
}

pub const STORY: Story = Story {
    id: "notifications",
    name: "Notifications and tooltips",
    group: "Kit",
    summary: "Transient surfaces: toasts pushed into the Root layer, hover tooltips and the Popover panel.",
    sections: &[
        Section {
            id: "toast-kinds",
            name: "Toast kinds",
            summary: "Notification entities rendered inline: untyped, info, success, warning, error and custom content. The close button shows on hover.",
            build: |_, cx| cx.new(Toasts::new).into(),
        },
        Section {
            id: "toast-stack",
            name: "Toast stack",
            summary: "Toasts pushed with push_notification stack at the top center under the title bar inset. These never auto-hide.",
            build: |window, cx| Stack::view(window, cx),
        },
        Section {
            id: "tooltip",
            name: "Tooltip",
            summary: "The tooltip box on its own: a label, a label trailed by the bound key, and a long label, which never wraps.",
            build: |_, cx| {
                bind_keys(cx);
                cx.new(Tooltips::new).into()
            },
        },
        Section {
            id: "tooltip-hovered",
            name: "Tooltip, hovered",
            summary: "The pointer rests on a button; after the show delay its tooltip floats below it.",
            build: |window, cx| Hovered::view(window, cx),
        },
        Section {
            id: "popover",
            name: "Popover",
            summary: "A press on the trigger opens the panel under it and selects the trigger. A press outside closes it. There is no API to open one from code.",
            build: |window, cx| Panel::view(window, cx),
        },
    ],
};

struct Toasts {
    items: Vec<(&'static str, Entity<Notification>)>,
}

impl Toasts {
    fn new(cx: &mut Context<Self>) -> Self {
        let make = |notification: Notification, cx: &mut Context<Self>| {
            cx.new(|_| notification.autohide(false))
        };
        Self {
            items: vec![
                (
                    "new, title and message",
                    make(
                        Notification::new()
                            .title("Session restored")
                            .message("3 windows and 7 panes came back."),
                        cx,
                    ),
                ),
                ("info", make(Notification::info("Copied 2 lines."), cx)),
                (
                    "success",
                    make(Notification::success("Config reloaded."), cx),
                ),
                (
                    "warning, with title",
                    make(
                        Notification::warning("Sessions keep the old daemon until it restarts.")
                            .title("Daemon is out of date"),
                        cx,
                    ),
                ),
                (
                    "error, with title",
                    make(
                        Notification::error("ssh devbox exited with status 255.")
                            .title("Connection lost"),
                        cx,
                    ),
                ),
                (
                    "custom content",
                    make(
                        Notification::info("zz 0.16.1 is ready.")
                            .title("Update available")
                            .content(|_, _, _| {
                                h_flex()
                                    .gap_2()
                                    .pt_2()
                                    .child(
                                        Button::new("toast-restart")
                                            .label("Restart")
                                            .accent()
                                            .small(),
                                    )
                                    .child(
                                        Button::new("toast-later").label("Later").ghost().small(),
                                    )
                                    .into_any_element()
                            }),
                        cx,
                    ),
                ),
            ],
        }
    }
}

impl Render for Toasts {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.items.iter().fold(states(), |states, (label, toast)| {
            states.state(*label, toast.clone())
        })
    }
}

struct Stack;

impl Stack {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        after_first_frame(window, |window, cx| {
            window.push_notification(
                Notification::new()
                    .title("Session restored")
                    .message("3 windows and 7 panes came back.")
                    .autohide(false),
                cx,
            );
            window.push_notification(
                Notification::success("Config reloaded.").autohide(false),
                cx,
            );
            window.push_notification(
                Notification::error("ssh devbox exited with status 255.")
                    .title("Connection lost")
                    .autohide(false),
                cx,
            );
        });
        cx.new(|_| Self).into()
    }
}

impl Render for Stack {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(380.0))
            .text_sm()
            .text_color(cx.theme().foreground.muted())
            .child("Pane content under the toasts.")
    }
}

struct Tooltips {
    plain: Entity<Tooltip>,
    keyed: Entity<Tooltip>,
    keyed_copy: Entity<Tooltip>,
    long: Entity<Tooltip>,
}

impl Tooltips {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            plain: cx.new(|_| Tooltip::new("Notifications")),
            keyed: cx.new(|_| Tooltip::new("Split right").action(&SplitRight, None)),
            keyed_copy: cx.new(|_| Tooltip::new("Copy path").action(&CopyPath, None)),
            long: cx.new(|_| {
                Tooltip::new("Detach from the session. The daemon keeps every pane running until you attach again.")
            }),
        }
    }
}

impl Render for Tooltips {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        states()
            .columns(2)
            .state("label", row().child(self.plain.clone()))
            .state(
                "label and key",
                row()
                    .child(self.keyed.clone())
                    .child(self.keyed_copy.clone()),
            )
            .state("long label, one line", row().child(self.long.clone()))
    }
}

struct Hovered {
    target: Probe,
    _bounds: Subscription,
}

impl Hovered {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        bind_keys(cx);
        let target = Probe::default();
        let probe = target.clone();
        when_visible(&target, window, move |window, cx| {
            hover(&probe, window, cx);
            nudge(90, window);
        });
        cx.new(|cx| Self {
            target,
            _bounds: cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                hover(&this.target, window, cx);
            }),
        })
        .into()
    }
}

impl Render for Hovered {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().h(px(120.0)).child(
            h_flex().child(
                div()
                    .relative()
                    .child(
                        Button::new("hovered-split")
                            .icon(IconName::PanelRight)
                            .ghost()
                            .tooltip("Split right"),
                    )
                    .child(self.target.measure()),
            ),
        )
    }
}

struct Panel {
    name: Entity<InputState>,
    trigger: Probe,
    _bounds: Subscription,
}

impl Panel {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let name = cx.new(|cx| InputState::new(window, cx).default_value("scratch"));
        let trigger = Probe::default();
        let probe = trigger.clone();
        when_visible(&trigger, window, move |window, cx| {
            press(&probe, MouseButton::Left, window, cx);
        });
        cx.new(|cx| Self {
            name,
            trigger,
            _bounds: cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                hover(&this.trigger, window, cx);
            }),
        })
        .into()
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.name.clone();
        let muted = cx.theme().foreground.muted();
        div().h(px(260.0)).child(
            h_flex().child(
                div()
                    .relative()
                    .child(
                        Popover::new("rename-popover")
                            .trigger(
                                Button::new("rename-trigger")
                                    .icon(IconName::Pencil)
                                    .label("Rename"),
                            )
                            .content(move |_, _, _| {
                                v_flex()
                                    .w(px(260.0))
                                    .gap_2()
                                    .child(div().text_xs().text_color(muted).child("Session name"))
                                    .child(Input::new(&name).small())
                                    .child(
                                        h_flex()
                                            .justify_end()
                                            .gap_2()
                                            .child(
                                                Button::new("popover-cancel")
                                                    .label("Cancel")
                                                    .ghost()
                                                    .small(),
                                            )
                                            .child(
                                                Button::new("popover-save")
                                                    .label("Save")
                                                    .accent()
                                                    .small(),
                                            ),
                                    )
                            }),
                    )
                    .child(self.trigger.measure()),
            ),
        )
    }
}
