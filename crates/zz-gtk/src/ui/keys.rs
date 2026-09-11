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
    use zz_protocol::{input_key_name, input_typed_text};

    const NONE: gdk::ModifierType = gdk::ModifierType::empty();
    const SHIFT: gdk::ModifierType = gdk::ModifierType::SHIFT_MASK;
    const CONTROL: gdk::ModifierType = gdk::ModifierType::CONTROL_MASK;
    const ALT: gdk::ModifierType = gdk::ModifierType::ALT_MASK;
    const SUPER: gdk::ModifierType = gdk::ModifierType::SUPER_MASK;

    fn press(keyval: gdk::Key, state: gdk::ModifierType) -> KeyInput {
        key_input(KeyAction::Press, keyval, state, None)
    }

    /// Named keys, both keypad spellings of them, function keys across the
    /// whole F1..F35 range, and the printable keyvals GDK resolves for a
    /// keypad with num lock on.
    #[test]
    fn every_keyval_a_pane_can_receive_folds_onto_a_wire_code() {
        let cases: &[(gdk::Key, KeyCode)] = &[
            (gdk::Key::BackSpace, KeyCode::Backspace),
            (gdk::Key::Return, KeyCode::Enter),
            (gdk::Key::KP_Enter, KeyCode::Enter),
            (gdk::Key::ISO_Enter, KeyCode::Enter),
            (gdk::Key::Tab, KeyCode::Tab),
            (gdk::Key::KP_Tab, KeyCode::Tab),
            (gdk::Key::ISO_Left_Tab, KeyCode::Tab),
            (gdk::Key::Escape, KeyCode::Escape),
            (gdk::Key::Delete, KeyCode::Delete),
            (gdk::Key::KP_Delete, KeyCode::Delete),
            (gdk::Key::Insert, KeyCode::Insert),
            (gdk::Key::KP_Insert, KeyCode::Insert),
            (gdk::Key::Home, KeyCode::Home),
            (gdk::Key::KP_Home, KeyCode::Home),
            (gdk::Key::End, KeyCode::End),
            (gdk::Key::KP_End, KeyCode::End),
            (gdk::Key::Page_Up, KeyCode::PageUp),
            (gdk::Key::KP_Page_Up, KeyCode::PageUp),
            (gdk::Key::Page_Down, KeyCode::PageDown),
            (gdk::Key::KP_Page_Down, KeyCode::PageDown),
            (gdk::Key::Up, KeyCode::ArrowUp),
            (gdk::Key::KP_Up, KeyCode::ArrowUp),
            (gdk::Key::Down, KeyCode::ArrowDown),
            (gdk::Key::KP_Down, KeyCode::ArrowDown),
            (gdk::Key::Left, KeyCode::ArrowLeft),
            (gdk::Key::KP_Left, KeyCode::ArrowLeft),
            (gdk::Key::Right, KeyCode::ArrowRight),
            (gdk::Key::KP_Right, KeyCode::ArrowRight),
            (gdk::Key::F1, KeyCode::Function(1)),
            (gdk::Key::F7, KeyCode::Function(7)),
            (gdk::Key::F12, KeyCode::Function(12)),
            (gdk::Key::F13, KeyCode::Function(13)),
            (gdk::Key::F35, KeyCode::Function(35)),
            (gdk::Key::a, KeyCode::Character('a')),
            (gdk::Key::A, KeyCode::Character('a')),
            (gdk::Key::space, KeyCode::Character(' ')),
            (gdk::Key::slash, KeyCode::Character('/')),
            (gdk::Key::KP_1, KeyCode::Character('1')),
            (gdk::Key::KP_Add, KeyCode::Character('+')),
            (gdk::Key::KP_Decimal, KeyCode::Character('.')),
        ];

        for (keyval, expected) in cases {
            assert_eq!(press(*keyval, NONE).key, *expected, "{keyval:?}");
        }
    }

    /// The daemon resolves a binding from the folded chord name, falling back
    /// on the typed character, so what the client sends has to spell the chords
    /// a user actually binds. `input_typed_text` is empty for anything carrying
    /// a command modifier, which is what keeps Ctrl-C off a plain `c` binding.
    #[test]
    fn chords_spell_the_names_the_daemon_binds() {
        let cases: &[(
            gdk::Key,
            gdk::ModifierType,
            Option<&str>,
            &str,
            Option<&str>,
        )] = &[
            (gdk::Key::a, NONE, None, "a", Some("a")),
            (gdk::Key::A, SHIFT, None, "A", Some("A")),
            (gdk::Key::c, CONTROL, None, "C-c", None),
            (gdk::Key::C, CONTROL | SHIFT, None, "C-c", None),
            (gdk::Key::f, ALT, None, "M-f", None),
            (gdk::Key::b, CONTROL | ALT, None, "C-M-b", None),
            (gdk::Key::F7, NONE, None, "F7", None),
            (gdk::Key::KP_Enter, NONE, None, "Enter", None),
            (gdk::Key::Up, NONE, None, "Up", None),
            (gdk::Key::Escape, NONE, None, "Escape", None),
            (gdk::Key::slash, SHIFT, Some("?"), "/", Some("?")),
        ];

        for (keyval, state, text, name, typed) in cases {
            let input = key_input(KeyAction::Press, *keyval, *state, *text);
            assert_eq!(input_key_name(&input).as_str(), *name, "{keyval:?}");
            assert_eq!(input_typed_text(&input), *typed, "{keyval:?}");
        }
    }

    #[test]
    fn a_shifted_character_carries_the_typed_text_and_the_unshifted_key() {
        let input = press(gdk::Key::A, SHIFT);

        assert_eq!(input.key, KeyCode::Character('a'));
        assert_eq!(input.text.as_deref(), Some("A"));
        assert_eq!(input.unshifted_codepoint, Some('a'));
        assert!(input.modifiers.shift());
    }

    #[test]
    fn committed_text_wins_over_the_keyval() {
        let input = key_input(KeyAction::Press, gdk::Key::e, NONE, Some("é"));

        assert_eq!(input.key, KeyCode::Character('e'));
        assert_eq!(input.text.as_deref(), Some("é"));
    }

    #[test]
    fn every_modifier_bit_survives_the_translation() {
        let input = press(gdk::Key::a, SHIFT | CONTROL | ALT | SUPER);

        assert!(input.modifiers.shift());
        assert!(input.modifiers.control());
        assert!(input.modifiers.alt());
        assert!(input.modifiers.platform());
    }

    #[test]
    fn control_keyvals_carry_no_typed_text() {
        for keyval in [gdk::Key::Escape, gdk::Key::BackSpace, gdk::Key::Return] {
            let input = press(keyval, NONE);
            assert_eq!(input.text, None, "{keyval:?} produced typed text");
            assert_eq!(input.unshifted_codepoint, None);
        }
    }

    /// A release only reaches the daemon for a pane in kitty keyboard mode, and
    /// it has to arrive as the same chord the press did.
    #[test]
    fn a_release_keeps_its_action_and_its_chord() {
        let input = key_input(KeyAction::Release, gdk::Key::a, CONTROL, None);

        assert_eq!(input.action, KeyAction::Release);
        assert_eq!(input.key, KeyCode::Character('a'));
        assert_eq!(input_key_name(&input).as_str(), "C-a");
    }

    /// A bare modifier press folds to an empty wire name, which no table can
    /// ever match, so it is filtered out before it reaches the daemon.
    #[test]
    fn modifier_presses_are_filtered_out_before_the_wire() {
        for keyval in [
            gdk::Key::Shift_L,
            gdk::Key::Control_R,
            gdk::Key::Alt_L,
            gdk::Key::Super_L,
            gdk::Key::Meta_R,
            gdk::Key::Caps_Lock,
            gdk::Key::Num_Lock,
            gdk::Key::ISO_Level3_Shift,
        ] {
            assert!(is_modifier(keyval), "{keyval:?} is a modifier");
            assert!(
                input_key_name(&press(keyval, NONE)).as_str().is_empty(),
                "{keyval:?} folded to a name a table could match"
            );
        }
        for keyval in [gdk::Key::a, gdk::Key::F7, gdk::Key::Escape, gdk::Key::KP_1] {
            assert!(!is_modifier(keyval), "{keyval:?} is not a modifier");
        }
    }
}
