use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Keystroke, Render,
    Window, div, prelude::*, px,
};
use zz_protocol::{DisplayPanesAction, InputMessage};
use zz_terminal::KeyAction;

use crate::{
    mux::client::MuxClient,
    terminal::view::{key_code, key_input},
};

pub(crate) struct DisplayPanesView {
    focus_handle: FocusHandle,
    mux: Entity<MuxClient>,
    revision: u64,
}

impl DisplayPanesView {
    pub(crate) fn new(mux: Entity<MuxClient>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            mux,
            revision: 0,
        }
    }

    pub(crate) fn focus(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(crate) fn synchronize(&mut self, revision: u64, cx: &mut Context<Self>) {
        if self.revision != revision {
            self.revision = revision;
            cx.notify();
        }
    }

    fn send(&self, action: DisplayPanesAction, cx: &Context<Self>) {
        self.mux
            .read(cx)
            .send_input(InputMessage::DisplayPanes { action });
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.send(display_panes_action(&event.keystroke), cx);
        cx.stop_propagation();
    }
}

impl Focusable for DisplayPanesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DisplayPanesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("display-panes-input")
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .w(px(1.0))
            .h(px(1.0))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(|_, _, cx| cx.stop_propagation())
    }
}

fn display_panes_action(keystroke: &Keystroke) -> DisplayPanesAction {
    if keystroke.key == "escape" {
        DisplayPanesAction::Close
    } else {
        DisplayPanesAction::Key(key_input(
            keystroke,
            key_code(&keystroke.key),
            KeyAction::Press,
        ))
    }
}

#[cfg(test)]
mod tests {
    use gpui::Modifiers;
    use zz_terminal::{KeyCode, Modifiers as TerminalModifiers};

    use super::*;

    fn key(key: &str, key_char: Option<&str>) -> Keystroke {
        Keystroke {
            key: key.to_owned(),
            key_char: key_char.map(str::to_owned),
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn escape_closes_and_other_keys_preserve_terminal_input() {
        assert_eq!(
            display_panes_action(&key("escape", None)),
            DisplayPanesAction::Close
        );
        assert_eq!(
            display_panes_action(&key("q", Some("q"))),
            DisplayPanesAction::Key(zz_terminal::KeyInput {
                action: KeyAction::Press,
                key: KeyCode::Character('q'),
                modifiers: TerminalModifiers::default(),
                text: Some(Box::from("q")),
                unshifted_codepoint: Some('q'),
            })
        );
    }
}
