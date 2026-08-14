use gtk::{gdk, glib::translate::IntoGlib};
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers};

/// Translate a GDK press into the wire's key record.
///
/// `text` is whatever the input method committed for this press; without one
/// the printable keyval stands in, mirroring how the raw-terminal client turns
/// a decoded character into both the key and its typed text. The daemon's
/// resolver prefers that typed character over the folded key name, which is how
/// Shift+`/` reaches a `?` binding instead of a `/` one.
pub fn key_input(
    action: KeyAction,
    keyval: gdk::Key,
    state: gdk::ModifierType,
    text: Option<&str>,
) -> KeyInput {
    let character = keyval
        .to_unicode()
        .filter(|character| !character.is_control());
    let key = key_code(keyval, character);
    let text = text
        .filter(|text| !text.is_empty())
        .map(|text| text.to_owned().into_boxed_str())
        .or_else(|| character.map(|character| character.to_string().into_boxed_str()));
    let unshifted_codepoint = match key {
        KeyCode::Character(character) => Some(character),
        _ => None,
    };
    KeyInput {
        action,
        key,
        modifiers: modifiers(state),
        text,
        unshifted_codepoint,
    }
}

pub fn modifiers(state: gdk::ModifierType) -> Modifiers {
    Modifiers::new(
        state.contains(gdk::ModifierType::SHIFT_MASK),
        state.contains(gdk::ModifierType::CONTROL_MASK),
        state.contains(gdk::ModifierType::ALT_MASK),
        state.contains(gdk::ModifierType::SUPER_MASK),
    )
}

fn key_code(keyval: gdk::Key, character: Option<char>) -> KeyCode {
    if let Some(named) = named_key(keyval) {
        return named;
    }
    if let Some(number) = function_number(keyval) {
        return KeyCode::Function(number);
    }
    character.map_or(KeyCode::Unidentified, |character| {
        KeyCode::Character(if character.is_ascii_uppercase() {
            character.to_ascii_lowercase()
        } else {
            character
        })
    })
}

fn named_key(keyval: gdk::Key) -> Option<KeyCode> {
    Some(match keyval {
        gdk::Key::BackSpace => KeyCode::Backspace,
        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => KeyCode::Enter,
        gdk::Key::Tab | gdk::Key::KP_Tab | gdk::Key::ISO_Left_Tab => KeyCode::Tab,
        gdk::Key::Escape => KeyCode::Escape,
        gdk::Key::Delete | gdk::Key::KP_Delete => KeyCode::Delete,
        gdk::Key::Insert | gdk::Key::KP_Insert => KeyCode::Insert,
        gdk::Key::Home | gdk::Key::KP_Home => KeyCode::Home,
        gdk::Key::End | gdk::Key::KP_End => KeyCode::End,
        gdk::Key::Page_Up | gdk::Key::KP_Page_Up => KeyCode::PageUp,
        gdk::Key::Page_Down | gdk::Key::KP_Page_Down => KeyCode::PageDown,
        gdk::Key::Up | gdk::Key::KP_Up => KeyCode::ArrowUp,
        gdk::Key::Down | gdk::Key::KP_Down => KeyCode::ArrowDown,
        gdk::Key::Left | gdk::Key::KP_Left => KeyCode::ArrowLeft,
        gdk::Key::Right | gdk::Key::KP_Right => KeyCode::ArrowRight,
        _ => return None,
    })
}

/// GDK numbers F1..F35 consecutively from `F1`, so the offset is the number.
fn function_number(keyval: gdk::Key) -> Option<u8> {
    let raw = keyval.into_glib();
    let first = gdk::Key::F1.into_glib();
    let last = gdk::Key::F35.into_glib();
    (first..=last)
        .contains(&raw)
        .then(|| u8::try_from(raw - first + 1).unwrap_or(1))
}

/// True for a press that only changes the modifier state; those never reach a
/// pane and would otherwise fold to an empty wire name.
pub fn is_modifier(keyval: gdk::Key) -> bool {
    matches!(
        keyval,
        gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
            | gdk::Key::Hyper_L
            | gdk::Key::Hyper_R
            | gdk::Key::Caps_Lock
            | gdk::Key::Shift_Lock
            | gdk::Key::Num_Lock
            | gdk::Key::ISO_Level3_Shift
            | gdk::Key::ISO_Level5_Shift
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shifted_character_carries_the_typed_text_and_the_unshifted_key() {
        let input = key_input(
            KeyAction::Press,
            gdk::Key::A,
            gdk::ModifierType::SHIFT_MASK,
            None,
        );

        assert_eq!(input.key, KeyCode::Character('a'));
        assert_eq!(input.text.as_deref(), Some("A"));
        assert_eq!(input.unshifted_codepoint, Some('a'));
        assert!(input.modifiers.shift());
    }

    #[test]
    fn committed_text_wins_over_the_keyval() {
        let input = key_input(
            KeyAction::Press,
            gdk::Key::e,
            gdk::ModifierType::empty(),
            Some("é"),
        );

        assert_eq!(input.key, KeyCode::Character('e'));
        assert_eq!(input.text.as_deref(), Some("é"));
    }

    #[test]
    fn named_and_function_keys_map_onto_the_wire_codes() {
        assert_eq!(
            key_input(
                KeyAction::Press,
                gdk::Key::KP_Enter,
                gdk::ModifierType::empty(),
                None
            )
            .key,
            KeyCode::Enter
        );
        assert_eq!(
            key_input(
                KeyAction::Press,
                gdk::Key::F7,
                gdk::ModifierType::empty(),
                None
            )
            .key,
            KeyCode::Function(7)
        );
        assert!(is_modifier(gdk::Key::Control_L));
        assert!(!is_modifier(gdk::Key::a));
    }
}
