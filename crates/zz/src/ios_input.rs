//! iPad sticky modifiers consumed by the terminal's text-input path.

use gpui::{App, Global, Keystroke, Modifiers};
use zz_terminal::KeyAction;

use crate::mux::{client::MuxClient, prefix};

/// Sticky modifier state: armed by a tap, consumed by the next key.
#[derive(Clone, Copy, Default)]
pub struct IosAccessory {
    pub ctrl: bool,
    pub alt: bool,
}

impl Global for IosAccessory {}

/// Turn software-keyboard text into a modified key press while a sticky modifier
/// is armed. Returns whether the text was consumed.
pub fn send_with_sticky_modifiers(
    mux: &gpui::Entity<MuxClient>,
    pane: zz_protocol::PaneId,
    text: &str,
    cx: &mut App,
) -> bool {
    let sticky = cx.try_global::<IosAccessory>().copied().unwrap_or_default();
    if !sticky.ctrl && !sticky.alt {
        return false;
    }
    let mut characters = text.chars();
    let (Some(character), None) = (characters.next(), characters.next()) else {
        return false;
    };
    let keystroke = Keystroke {
        modifiers: Modifiers {
            control: sticky.ctrl,
            alt: sticky.alt,
            ..Modifiers::default()
        },
        key: character.to_lowercase().to_string(),
        key_char: None,
    };
    let input = prefix::terminal_key_input(&keystroke, KeyAction::Press);
    mux.read(cx).send_input(zz_protocol::InputMessage::Key {
        pane,
        input,
        text_follows: false,
    });
    cx.set_global(IosAccessory::default());
    true
}
