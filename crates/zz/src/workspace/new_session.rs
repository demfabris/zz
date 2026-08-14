//! Empty-workspace state: the one action available here, and the keys it unlocks.

use crate::config::settings::OpenSettings;
use crate::mux::client::MuxClient;
use crate::mux::prefix::display_keystroke;
use gpui::{
    App, Context, Div, FocusHandle, Focusable, KeyDownEvent, Keystroke, MouseButton,
    ParentElement as _, Render, Styled as _, Window, div, prelude::*, px,
};
use zz_protocol::{CommandInvocation, KeyBindingSnapshot};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    kbd::Kbd,
};

/// The daemon's own default (`zz_mux::KeyTables`), printed until it publishes one.
const DEFAULT_PREFIX: &str = "C-b";

const KEY_COLUMN_WIDTH: f32 = 92.0;
const KEY_LABEL_GAP: f32 = 10.0;
const PANEL_WIDTH: f32 = 340.0;

struct Hint {
    prefixed: bool,
    key: &'static str,
    label: &'static str,
}

struct BindingHint {
    hint: Hint,
    matches: fn(&CommandInvocation) -> bool,
}

const ACTIONS: [Hint; 2] = [
    Hint {
        prefixed: false,
        key: "enter",
        label: "New session",
    },
    Hint {
        prefixed: false,
        key: crate::config::settings::KEYBIND,
        label: "Settings",
    },
];

const BINDINGS: [BindingHint; 5] = [
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "c",
            label: "New window",
        },
        matches: |command| command.name == "new-window",
    },
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "%",
            label: "Split right",
        },
        matches: |command| {
            command.name == "split-window" && command.args.iter().any(|arg| arg == "-h")
        },
    },
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "\"",
            label: "Split down",
        },
        matches: |command| {
            command.name == "split-window" && command.args.iter().all(|arg| arg != "-h")
        },
    },
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "s",
            label: "Sessions and windows",
        },
        matches: |command| matches!(command.name.as_str(), "focus-sidebar" | "choose-tree"),
    },
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "?",
            label: "Every key binding",
        },
        matches: |command| command.name == "list-keys",
    },
];

fn resolve_binding_key(
    hint: &BindingHint,
    prefix_bindings: &[KeyBindingSnapshot],
) -> Option<Keystroke> {
    let stock_key = || display_keystroke(hint.hint.key);
    if prefix_bindings.is_empty() {
        return stock_key();
    }

    let binding = prefix_bindings
        .iter()
        .filter(|binding| {
            binding
                .commands
                .first()
                .is_some_and(|command| (hint.matches)(command))
        })
        .min_by_key(|binding| binding.key == hint.hint.key)?;
    display_keystroke(&binding.key).or_else(stock_key)
}

fn hint_row(
    hint: &Hint,
    prefix: Option<&Keystroke>,
    key: Option<Keystroke>,
    strong: bool,
    cx: &App,
) -> Div {
    let keys = hint
        .prefixed
        .then(|| prefix.cloned())
        .flatten()
        .into_iter()
        .chain(key)
        .map(|key| Kbd::new(key).lowercase().into_any_element())
        .collect::<Vec<_>>();
    div()
        .flex()
        .items_center()
        .gap(px(KEY_LABEL_GAP))
        .child(
            div()
                .flex()
                .flex_none()
                .w(px(KEY_COLUMN_WIDTH))
                .items_center()
                .justify_end()
                .gap(px(4.0))
                .children(keys),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(zz_ui::rems_from_px(12.0))
                .text_color(if strong {
                    cx.theme().foreground
                } else {
                    cx.theme().foreground.muted()
                })
                .child(hint.label),
        )
}

fn activates_new_session(keystroke: &Keystroke) -> bool {
    let modifiers = keystroke.modifiers;
    !modifiers.control && !modifiers.alt && !modifiers.platform && keystroke.key.as_str() == "enter"
}

pub(crate) struct NewSessionView {
    mux: gpui::Entity<MuxClient>,
    focus_handle: FocusHandle,
}

impl NewSessionView {
    pub(crate) fn new(mux: gpui::Entity<MuxClient>, cx: &mut Context<Self>) -> Self {
        Self {
            mux,
            focus_handle: cx.focus_handle(),
        }
    }

    pub(crate) fn focus(&self) -> &FocusHandle {
        &self.focus_handle
    }

    fn activate(&self, cx: &App) {
        if let Some(host) = self.mux.read(cx).first_host() {
            self.mux.read(cx).new_session(host);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if activates_new_session(&event.keystroke) {
            self.activate(cx);
            cx.stop_propagation();
        }
    }
}

impl Focusable for NewSessionView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for NewSessionView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        if !self.mux.read(cx).has_hosts() {
            return div()
                .id("new-session-empty-state")
                .track_focus(&self.focus_handle)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    focus_handle.focus(window, cx);
                })
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .px(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .max_w(px(PANEL_WIDTH))
                        .items_start()
                        .gap_3()
                        .text_size(zz_ui::rems_from_px(12.0))
                        .child(
                            div()
                                .text_color(cx.theme().foreground)
                                .child("Connect to a computer"),
                        )
                        .child(
                            div()
                                .text_color(cx.theme().foreground.muted())
                                .child(
                                    "zz attaches to a daemon over SSH. On your Mac, enable Remote Login in System Settings, then add it here.",
                                ),
                        )
                        .child(
                            Button::new("new-session-add-host")
                                .primary()
                                .small()
                                .label("Add host…")
                                .on_click(|_, window, cx| {
                                    super::add_host::open(window, cx);
                                    cx.stop_propagation();
                                }),
                        ),
                )
                .into_any_element();
        }
        let view = cx.entity();
        let prefix = self
            .mux
            .read(cx)
            .canonical_prefix()
            .unwrap_or_else(|| DEFAULT_PREFIX.to_owned());
        let prefix = display_keystroke(&prefix);
        let binding_keys = {
            let mux = self.mux.read(cx);
            BINDINGS
                .iter()
                .map(|hint| resolve_binding_key(hint, mux.prefix_bindings()))
                .collect::<Vec<_>>()
        };
        let mut actions = ACTIONS.iter();
        let new_session = actions.next().expect("the new-session hint");
        let settings = actions.next().expect("the settings hint");
        div()
            .id("new-session-empty-state")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                focus_handle.focus(window, cx);
            })
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .text_color(cx.theme().foreground)
            .px(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w_full()
                    .max_w(px(PANEL_WIDTH))
                    .gap(px(2.0))
                    .child(
                        hint_row(
                            new_session,
                            prefix.as_ref(),
                            Keystroke::parse(new_session.key).ok(),
                            true,
                            cx,
                        )
                        .id("new-session-activate")
                        .h(px(32.0))
                        .px(px(12.0))
                        .cursor_pointer()
                        .rounded(cx.theme().radius)
                        .hover(|style| style.bg(cx.theme().background.hover()))
                        .on_mouse_down(
                            MouseButton::Left,
                            move |_, _, cx| {
                                view.update(cx, |view, cx| view.activate(cx));
                                cx.stop_propagation();
                            },
                        ),
                    )
                    .child(
                        hint_row(
                            settings,
                            prefix.as_ref(),
                            Keystroke::parse(settings.key).ok(),
                            false,
                            cx,
                        )
                        .id("new-session-settings")
                        .h(px(32.0))
                        .px(px(12.0))
                        .cursor_pointer()
                        .rounded(cx.theme().radius)
                        .hover(|style| style.bg(cx.theme().background.hover()))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenSettings), cx);
                        }),
                    )
                    .child(
                        div()
                            .mt(px(12.0))
                            .mb(px(4.0))
                            .pl(px(12.0 + KEY_COLUMN_WIDTH + KEY_LABEL_GAP))
                            .text_size(zz_ui::rems_from_px(11.0))
                            .text_color(cx.theme().foreground.muted())
                            .child("In a session"),
                    )
                    .children(BINDINGS.iter().zip(binding_keys).filter_map(|(hint, key)| {
                        key.map(|key| {
                            hint_row(&hint.hint, prefix.as_ref(), Some(key), false, cx)
                                .h(px(24.0))
                                .px(px(12.0))
                        })
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext, VisualTestContext};
    use zz_daemon::DaemonError;

    use super::*;

    fn key(value: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: value.to_owned(),
            key_char: Some(value.to_owned()),
        }
    }

    #[test]
    fn enter_activates_the_empty_session_card() {
        assert!(activates_new_session(&key("enter")));
        assert!(!activates_new_session(&key("space")));
        let mut modified = key("enter");
        modified.modifiers.platform = true;
        assert!(!activates_new_session(&modified));
    }

    #[test]
    fn every_hint_spells_a_key_gpui_can_parse() {
        for hint in &ACTIONS {
            assert!(
                Keystroke::parse(hint.key).is_ok(),
                "`{}` ({})",
                hint.key,
                hint.label
            );
        }
        for hint in &BINDINGS {
            assert!(
                display_keystroke(hint.hint.key).is_some(),
                "`{}` ({})",
                hint.hint.key,
                hint.hint.label
            );
        }
        assert!(crate::mux::prefix::display_keystroke(DEFAULT_PREFIX).is_some());
    }

    #[test]
    fn non_stock_binding_is_preferred_over_stock() {
        let bindings = [
            KeyBindingSnapshot {
                key: "%".to_owned(),
                commands: vec![CommandInvocation {
                    name: "split-window".to_owned(),
                    args: vec!["-h".to_owned()],
                    source: None,
                }],
                repeat: false,
                note: None,
            },
            KeyBindingSnapshot {
                key: "|".to_owned(),
                commands: vec![CommandInvocation {
                    name: "split-window".to_owned(),
                    args: vec!["-h".to_owned()],
                    source: None,
                }],
                repeat: false,
                note: None,
            },
        ];

        assert_eq!(
            resolve_binding_key(&BINDINGS[1], &bindings),
            display_keystroke("|")
        );
    }

    #[test]
    fn empty_binding_table_uses_the_stock_key() {
        assert_eq!(
            resolve_binding_key(&BINDINGS[1], &[]),
            display_keystroke("%")
        );
    }

    #[test]
    fn missing_binding_omits_the_row() {
        let bindings = [KeyBindingSnapshot {
            key: "c".to_owned(),
            commands: vec![CommandInvocation {
                name: "new-window".to_owned(),
                args: Vec::new(),
                source: None,
            }],
            repeat: false,
            note: None,
        }];

        assert_eq!(resolve_binding_key(&BINDINGS[1], &bindings), None);
    }

    #[test]
    fn unparseable_binding_uses_the_stock_key() {
        let bindings = [KeyBindingSnapshot {
            key: String::new(),
            commands: vec![CommandInvocation {
                name: "split-window".to_owned(),
                args: vec!["-h".to_owned()],
                source: None,
            }],
            repeat: false,
            note: None,
        }];

        assert_eq!(
            resolve_binding_key(&BINDINGS[1], &bindings),
            display_keystroke("%")
        );
    }

    #[gpui::test]
    fn the_empty_state_draws_without_a_daemon(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let (_, cx) = cx.add_window_view(|_, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            NewSessionView::new(mux, cx)
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }
}
