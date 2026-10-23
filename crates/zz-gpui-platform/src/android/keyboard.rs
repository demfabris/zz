use std::collections::HashMap;
use zz_gpui::{Capslock, KeyDownEvent, Keystroke, Modifiers};

const META_SHIFT_ON: i32 = 0x1;
const META_ALT_ON: i32 = 0x2;
const META_FUNCTION_ON: i32 = 0x8;
const META_CTRL_ON: i32 = 0x1000;
const META_META_ON: i32 = 0x10000;
const META_CAPS_LOCK_ON: i32 = 0x100000;
const COMBINING_ACCENT: u32 = 0x8000_0000;

pub const KEYCODE_TAB: i32 = 61;
pub const KEYCODE_ENTER: i32 = 66;
pub const KEYCODE_DEL: i32 = 67;
pub const KEYCODE_FORWARD_DEL: i32 = 112;

#[derive(Default)]
pub struct Keyboard {
    held: HashMap<i32, Keystroke>,
    pub modifiers: Modifiers,
    pub capslock: Capslock,
}

impl Keyboard {
    pub fn press(
        &mut self,
        code: i32,
        meta: i32,
        character: u32,
        base: u32,
        repeat: i32,
    ) -> Option<KeyDownEvent> {
        self.sync(meta);
        if is_modifier(code) {
            return None;
        }
        let keystroke = keystroke(code, character, base, self.modifiers)?;
        let is_held = self.held.insert(code, keystroke.clone()).is_some() || repeat > 0;
        Some(KeyDownEvent {
            keystroke,
            is_held,
            prefer_character_input: false,
        })
    }

    pub fn release(&mut self, code: i32, meta: i32) -> Option<Keystroke> {
        self.sync(meta);
        self.held.remove(&code).map(|mut key| {
            key.modifiers = self.modifiers;
            key.key_char = None;
            key
        })
    }

    pub fn clear(&mut self) -> Vec<Keystroke> {
        self.modifiers = Modifiers::default();
        self.held
            .drain()
            .map(|(_, mut key)| {
                key.modifiers = Modifiers::default();
                key.key_char = None;
                key
            })
            .collect()
    }

    fn sync(&mut self, meta: i32) {
        self.modifiers = modifiers(meta);
        self.capslock.on = meta & META_CAPS_LOCK_ON != 0;
    }
}

pub fn modifiers(meta: i32) -> Modifiers {
    Modifiers {
        shift: meta & META_SHIFT_ON != 0,
        control: meta & META_CTRL_ON != 0,
        alt: meta & META_ALT_ON != 0,
        platform: meta & META_META_ON != 0,
        function: meta & META_FUNCTION_ON != 0,
    }
}

pub fn is_modifier(code: i32) -> bool {
    matches!(code, 57..=60 | 63 | 113..=119 | 143)
}

fn named(code: i32) -> Option<&'static str> {
    Some(match code {
        19 => "up",
        20 => "down",
        21 => "left",
        22 => "right",
        KEYCODE_TAB => "tab",
        62 => "space",
        KEYCODE_ENTER | 160 => "enter",
        KEYCODE_DEL => "backspace",
        92 => "pageup",
        93 => "pagedown",
        111 => "escape",
        KEYCODE_FORWARD_DEL => "delete",
        122 => "home",
        123 => "end",
        124 => "insert",
        _ => return None,
    })
}

fn printable(character: u32) -> Option<char> {
    if character == 0 || character & COMBINING_ACCENT != 0 {
        return None;
    }
    char::from_u32(character).filter(|character| !character.is_control())
}

pub fn keystroke(code: i32, character: u32, base: u32, modifiers: Modifiers) -> Option<Keystroke> {
    let mut key_char = printable(character).map(String::from);
    let key = if let Some(named) = named(code) {
        if named == "space" {
            key_char.get_or_insert_with(|| " ".into());
        } else {
            key_char = None;
        }
        named.to_owned()
    } else if (131..=142).contains(&code) {
        key_char = None;
        format!("f{}", code - 130)
    } else if let Some(base) = printable(base) {
        base.to_lowercase().to_string()
    } else if let Some(character) = key_char.clone() {
        character.to_lowercase()
    } else {
        return None;
    };
    if modifiers.control || modifiers.platform {
        key_char = None;
    }
    Some(Keystroke {
        modifiers,
        key,
        key_char,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_text_and_space_survive() {
        let mut keyboard = Keyboard::default();
        let key = keyboard
            .press(9, META_SHIFT_ON, '@' as u32, '2' as u32, 0)
            .unwrap();
        assert_eq!(key.keystroke.key, "2");
        assert_eq!(key.keystroke.key_char.as_deref(), Some("@"));
        assert!(key.keystroke.modifiers.shift);
        let space = keyboard.press(62, 0, ' ' as u32, ' ' as u32, 0).unwrap();
        assert_eq!(space.keystroke.key, "space");
        assert_eq!(space.keystroke.key_char.as_deref(), Some(" "));
        let upper = keyboard
            .press(29, META_SHIFT_ON, 'A' as u32, 'a' as u32, 0)
            .unwrap();
        assert_eq!(upper.keystroke.key, "a");
        assert_eq!(upper.keystroke.key_char.as_deref(), Some("A"));
    }

    #[test]
    fn control_and_named_keys_do_not_insert_text() {
        let mut keyboard = Keyboard::default();
        let key = keyboard.press(31, META_CTRL_ON, 0, 'c' as u32, 0).unwrap();
        assert_eq!(key.keystroke.key, "c");
        assert!(key.keystroke.modifiers.control);
        assert_eq!(key.keystroke.key_char, None);
        let enter = keyboard.press(66, 0, '\n' as u32, '\n' as u32, 0).unwrap();
        assert_eq!(enter.keystroke.key, "enter");
        assert_eq!(enter.keystroke.key_char, None);
        assert_eq!(
            keyboard.press(142, 0, 0, 0, 0).unwrap().keystroke.key,
            "f12"
        );
        assert!(keyboard.press(59, META_SHIFT_ON, 0, 0, 0).is_none());
        assert!(keyboard.modifiers.shift);
    }

    #[test]
    fn repeats_are_held_and_releases_drop_text() {
        let mut keyboard = Keyboard::default();
        assert!(!keyboard.press(67, 0, 0, 0, 0).unwrap().is_held);
        assert!(keyboard.press(67, 0, 0, 0, 1).unwrap().is_held);
        let released = keyboard.release(67, 0).unwrap();
        assert_eq!(released.key, "backspace");
        keyboard.press(36, 0, 'h' as u32, 'h' as u32, 0);
        let released = keyboard.release(36, 0).unwrap();
        assert_eq!(released.key_char, None);
        assert!(keyboard.clear().is_empty());
    }

    #[test]
    fn dead_keys_type_nothing() {
        let key = keystroke(
            34,
            COMBINING_ACCENT | 0x301,
            'f' as u32,
            Modifiers {
                alt: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(key.key, "f");
        assert_eq!(key.key_char, None);
    }
}
