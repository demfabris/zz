use std::{
    fmt::{self, Write as _},
    ops::Deref,
    sync::Arc,
};

use smallvec::SmallVec;
use zz_protocol::KeyToken;
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers, TerminalSession};

const INLINE_KEY_NAME_BYTES: usize = 16;

pub(crate) struct KeyName {
    bytes: SmallVec<[u8; INLINE_KEY_NAME_BYTES]>,
}

impl KeyName {
    fn new() -> Self {
        Self {
            bytes: SmallVec::new(),
        }
    }

    fn push_str(&mut self, value: &str) {
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn push_char(&mut self, value: char) {
        let mut encoded = [0_u8; 4];
        self.push_str(value.encode_utf8(&mut encoded));
    }

    pub(crate) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes).expect("key names are assembled from valid UTF-8")
    }

    pub(crate) fn into_string(self) -> String {
        String::from_utf8(self.bytes.into_vec()).expect("key names are assembled from valid UTF-8")
    }
}

impl fmt::Write for KeyName {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.push_str(value);
        Ok(())
    }
}

impl Deref for KeyName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

fn shifted_character(input: &KeyInput, character: char) -> char {
    if input.modifiers.control() || input.modifiers.alt() {
        return character;
    }
    if input.modifiers.shift() && character.is_ascii_lowercase() {
        return character.to_ascii_uppercase();
    }
    character
}

pub(crate) fn input_key_name(input: &KeyInput) -> KeyName {
    let mut name = KeyName::new();
    if input.modifiers.platform() {
        return name;
    }
    if input.modifiers.control() {
        name.push_str("C-");
    }
    if input.modifiers.alt() {
        name.push_str("M-");
    }
    match input.key {
        KeyCode::Character(character) => name.push_char(shifted_character(input, character)),
        KeyCode::Backspace => name.push_str("BSpace"),
        KeyCode::Enter => name.push_str("Enter"),
        KeyCode::Tab => name.push_str("Tab"),
        KeyCode::Escape => name.push_str("Escape"),
        KeyCode::Delete => name.push_str("DC"),
        KeyCode::Insert => name.push_str("IC"),
        KeyCode::Home => name.push_str("Home"),
        KeyCode::End => name.push_str("End"),
        KeyCode::PageUp => name.push_str("PPage"),
        KeyCode::PageDown => name.push_str("NPage"),
        KeyCode::ArrowUp => name.push_str("Up"),
        KeyCode::ArrowDown => name.push_str("Down"),
        KeyCode::ArrowLeft => name.push_str("Left"),
        KeyCode::ArrowRight => name.push_str("Right"),
        KeyCode::Function(number) => write!(&mut name, "F{number}").expect("writing key name"),
        KeyCode::Unidentified => name.push_str(input.text.as_deref().unwrap_or_default()),
    }
    name
}

pub(crate) fn send_tokens(sessions: &[Arc<TerminalSession>], tokens: &[KeyToken]) {
    for token in tokens {
        match token {
            KeyToken::Literal(text) => {
                let text = Arc::<str>::from(text.as_str());
                for session in sessions {
                    session.send_text(Arc::clone(&text));
                }
            }
            KeyToken::Named(name) => {
                if let Some(input) = named_key(name) {
                    let mut input = Some(input);
                    for (index, session) in sessions.iter().enumerate() {
                        let session_input = if index + 1 == sessions.len() {
                            input.take().expect("last terminal owns key input")
                        } else {
                            input
                                .as_ref()
                                .expect("key input is retained until the last terminal")
                                .clone()
                        };
                        session.send_key(session_input);
                    }
                }
            }
        }
    }
}

fn named_key(name: &str) -> Option<KeyInput> {
    let mut modifiers = Modifiers::default();
    let mut key_name = name;
    loop {
        if let Some(rest) = key_name.strip_prefix("C-") {
            modifiers.set_control(true);
            key_name = rest;
        } else if let Some(rest) = key_name.strip_prefix("M-") {
            modifiers.set_alt(true);
            key_name = rest;
        } else {
            break;
        }
    }
    let key = match key_name {
        "Enter" => KeyCode::Enter,
        "Escape" => KeyCode::Escape,
        "Space" => KeyCode::Character(' '),
        "Tab" => KeyCode::Tab,
        "BSpace" => KeyCode::Backspace,
        "Up" => KeyCode::ArrowUp,
        "Down" => KeyCode::ArrowDown,
        "Left" => KeyCode::ArrowLeft,
        "Right" => KeyCode::ArrowRight,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PPage" => KeyCode::PageUp,
        "NPage" => KeyCode::PageDown,
        "DC" => KeyCode::Delete,
        "IC" => KeyCode::Insert,
        value if value.chars().count() == 1 => KeyCode::Character(value.chars().next()?),
        value => KeyCode::Function(value.strip_prefix('F')?.parse().ok()?),
    };
    let text = match key {
        KeyCode::Character(character) if !modifiers.control() && !modifiers.alt() => {
            Some(character.to_string().into_boxed_str())
        }
        _ => None,
    };
    Some(KeyInput {
        action: KeyAction::Press,
        key,
        modifiers,
        text,
        unshifted_codepoint: match key {
            KeyCode::Character(character) => Some(character),
            _ => None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_named_keys_map_to_terminal_keys() {
        assert_eq!(named_key("Enter").unwrap().key, KeyCode::Enter);
        let interrupt = named_key("C-c").unwrap();
        assert_eq!(interrupt.key, KeyCode::Character('c'));
        assert!(interrupt.modifiers.control());
        assert_eq!(named_key("F12").unwrap().key, KeyCode::Function(12));
        assert_eq!(input_key_name(&interrupt).as_str(), "C-c");
    }

    fn shifted_letter(key: char) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key: KeyCode::Character(key),
            modifiers: Modifiers::new(true, false, false, false),
            text: Some(key.to_ascii_uppercase().to_string().into_boxed_str()),
            unshifted_codepoint: Some(key),
        }
    }

    #[test]
    fn shifted_letters_resolve_by_their_uppercase_binding() {
        for letter in [
            'a', 'b', 'd', 'e', 'f', 'g', 'h', 'j', 'k', 'l', 'm', 'n', 'p', 'r', 't', 'v', 'w',
            'x',
        ] {
            let upper = letter.to_ascii_uppercase();
            assert_eq!(
                input_key_name(&shifted_letter(letter)).as_str(),
                upper.to_string(),
                "shift+{letter} must resolve as {upper}"
            );
        }
    }

    #[test]
    fn already_resolved_shifted_symbols_are_left_alone() {
        for symbol in ['#', '*', '%', ':', '?'] {
            let input = KeyInput {
                action: KeyAction::Press,
                key: KeyCode::Character(symbol),
                modifiers: Modifiers::default(),
                text: Some(symbol.to_string().into_boxed_str()),
                unshifted_codepoint: Some(symbol),
            };
            assert_eq!(input_key_name(&input).as_str(), symbol.to_string());
        }
    }

    #[test]
    fn a_key_is_spelled_the_same_on_press_and_release() {
        let mut release = shifted_letter('g');
        release.action = KeyAction::Release;
        release.text = None;
        assert_eq!(input_key_name(&release).as_str(), "G");
        assert_eq!(
            input_key_name(&shifted_letter('g')).as_str(),
            input_key_name(&release).as_str()
        );
    }

    #[test]
    fn a_platform_chord_never_names_a_bare_key() {
        let mut command_x = named_key("x").expect("supported named key");
        command_x.modifiers = Modifiers::new(false, false, false, true);
        assert!(input_key_name(&command_x).as_str().is_empty());

        let mut command_c = named_key("c").expect("supported named key");
        command_c.modifiers = Modifiers::new(false, false, false, true);
        assert!(input_key_name(&command_c).as_str().is_empty());
    }

    #[test]
    fn modified_keys_keep_their_base_spelling() {
        let interrupt = named_key("C-c").expect("supported named key");
        assert_eq!(input_key_name(&interrupt).as_str(), "C-c");

        let mut control_shift = named_key("C-c").expect("supported named key");
        control_shift.modifiers = Modifiers::new(true, true, false, false);
        assert_eq!(input_key_name(&control_shift).as_str(), "C-c");
    }

    #[test]
    fn common_tmux_key_names_stay_in_stack_storage() {
        for name in ["Enter", "C-M-Left", "C-M-F255", "C-M-λ"] {
            let input = named_key(name).expect("supported named key");
            let rendered = input_key_name(&input);
            assert_eq!(rendered.as_str(), name);
            assert!(!rendered.bytes.spilled());
        }
    }
}
