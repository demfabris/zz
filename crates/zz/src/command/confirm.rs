use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Keystroke, Render,
    Window, div, prelude::*, px,
};
use zz_protocol::{ConfirmAction, ConfirmState, InputMessage};

use crate::{mux::client::MuxClient, terminal::view::TERMINAL_FONT};

pub(crate) struct ConfirmView {
    focus_handle: FocusHandle,
    mux: Entity<MuxClient>,
    state: ConfirmState,
}

impl ConfirmView {
    pub(crate) fn new(mux: Entity<MuxClient>, state: ConfirmState, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            mux,
            state,
        }
    }

    pub(crate) fn focus(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(crate) fn state(&self) -> &ConfirmState {
        &self.state
    }

    pub(crate) fn synchronize(&mut self, state: ConfirmState, cx: &mut Context<Self>) {
        self.state = state;
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let accepted = confirm_accepts(&self.state, &event.keystroke);
        self.mux.read(cx).send_input(InputMessage::Confirm {
            action: ConfirmAction::Reply(accepted),
        });
        cx.stop_propagation();
    }
}

impl Focusable for ConfirmView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ConfirmView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("confirm-before-input")
            .size_full()
            .flex()
            .items_center()
            .px(px(12.0))
            .font_family(TERMINAL_FONT)
            .text_size(px(13.0))
            .line_height(px(16.0))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(|_, _, cx| cx.stop_propagation())
            .child(self.state.prompt.clone())
    }
}

fn confirm_accepts(state: &ConfirmState, keystroke: &Keystroke) -> bool {
    if keystroke.key == "enter" {
        return state.default_yes && !keystroke.modifiers.platform && !keystroke.modifiers.function;
    }
    if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.function {
        return false;
    }
    let Some(character) = keystroke.key_char.as_deref().and_then(|value| {
        value
            .as_bytes()
            .first()
            .copied()
            .filter(|_| value.len() == 1)
    }) else {
        return false;
    };
    character == state.confirm_key
}

#[cfg(test)]
mod tests {
    use gpui::Modifiers;

    use super::*;

    fn key(value: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            key: value.to_owned(),
            key_char: key_char.map(str::to_owned),
            modifiers,
        }
    }

    #[test]
    fn confirmation_is_case_sensitive_and_enter_uses_default_yes() {
        let state = ConfirmState {
            prompt: "Confirm? ".to_owned(),
            confirm_key: b'Y',
            default_yes: false,
        };
        assert!(confirm_accepts(
            &state,
            &key("Y", Some("Y"), Modifiers::default())
        ));
        assert!(!confirm_accepts(
            &state,
            &key("y", Some("y"), Modifiers::default())
        ));
        assert!(!confirm_accepts(
            &state,
            &key("enter", None, Modifiers::default())
        ));
        assert!(confirm_accepts(
            &ConfirmState {
                default_yes: true,
                ..state
            },
            &key("enter", None, Modifiers::default())
        ));
    }

    #[test]
    fn confirmation_modifiers_follow_single_key_prompt_rules() {
        let state = ConfirmState {
            prompt: String::new(),
            confirm_key: b'y',
            default_yes: false,
        };
        let control = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert!(!confirm_accepts(&state, &key("y", Some("y"), control)));
        assert!(!confirm_accepts(
            &state,
            &key("escape", None, Modifiers::default())
        ));

        let alt = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        assert!(confirm_accepts(&state, &key("y", Some("y"), alt)));
        assert!(confirm_accepts(
            &ConfirmState {
                default_yes: true,
                ..state
            },
            &key("enter", None, alt)
        ));
    }
}
