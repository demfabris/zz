use gpui::{Capslock, KeyDownEvent, Keystroke, Modifiers};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Keyboard {
    held: HashMap<u16, Keystroke>,
    modifiers_held: HashSet<u16>,
    pub modifiers: Modifiers,
    pub capslock: Capslock,
    repeat: Option<(u16, Instant)>,
}

impl Keyboard {
    pub fn press(
        &mut self,
        hid: u16,
        characters: &str,
        ignoring: &str,
        flags: u64,
    ) -> Option<KeyDownEvent> {
        self.capslock.on = flags & (1 << 16) != 0;
        self.modifiers = modifiers(flags);
        if is_modifier(hid) {
            self.repeat = None;
            self.modifiers_held.insert(hid);
            return None;
        }
        let keystroke = keystroke(hid, characters, ignoring, self.modifiers)?;
        let is_held = self.held.insert(hid, keystroke.clone()).is_some();
        Some(KeyDownEvent {
            keystroke,
            is_held,
            prefer_character_input: false,
        })
    }

    pub fn consumed(&mut self, hid: u16, is_held: bool, now: Instant) {
        self.repeat = Some((
            hid,
            now + if is_held {
                Duration::from_millis(33)
            } else {
                Duration::from_millis(450)
            },
        ));
    }

    pub fn release(&mut self, hid: u16, flags: u64) -> Option<Keystroke> {
        self.capslock.on = flags & (1 << 16) != 0;
        self.modifiers = modifiers(flags);
        if is_modifier(hid) {
            self.repeat = None;
            self.modifiers_held.remove(&hid);
            let held = |a, b| self.modifiers_held.contains(&a) || self.modifiers_held.contains(&b);
            match hid {
                224 | 228 => self.modifiers.control = held(224, 228),
                225 | 229 => self.modifiers.shift = held(225, 229),
                226 | 230 => self.modifiers.alt = held(226, 230),
                227 | 231 => self.modifiers.platform = held(227, 231),
                _ => {}
            }
        }
        if self.repeat.is_some_and(|(key, _)| key == hid) {
            self.repeat = None;
        }
        self.held.remove(&hid).map(|mut key| {
            key.modifiers = self.modifiers;
            key.key_char = None;
            key
        })
    }

    pub fn repeat(&mut self, now: Instant) -> Option<KeyDownEvent> {
        let (hid, deadline) = self.repeat?;
        if now < deadline {
            return None;
        }
        let keystroke = self.held.get(&hid)?.clone();
        self.repeat = Some((hid, now + Duration::from_millis(33)));
        Some(KeyDownEvent {
            keystroke,
            is_held: true,
            prefer_character_input: false,
        })
    }

    pub fn clear(&mut self) -> Vec<Keystroke> {
        self.repeat = None;
        self.modifiers_held.clear();
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
}

pub fn is_modifier(hid: u16) -> bool {
    matches!(hid, 57 | 224..=231)
}

fn modifiers(flags: u64) -> Modifiers {
    Modifiers {
        shift: flags & (1 << 17) != 0,
        control: flags & (1 << 18) != 0,
        alt: flags & (1 << 19) != 0,
        platform: flags & (1 << 20) != 0,
        function: false,
    }
}

fn printable(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with("UIKeyInput")
        && !text
            .chars()
            .any(|c| c.is_control() || ('\u{f700}'..='\u{f8ff}').contains(&c))
}

fn keystroke(
    hid: u16,
    characters: &str,
    ignoring: &str,
    modifiers: Modifiers,
) -> Option<Keystroke> {
    let named = match hid {
        40 | 88 => Some("enter"),
        41 => Some("escape"),
        42 => Some("backspace"),
        43 => Some("tab"),
        44 => Some("space"),
        73 => Some("insert"),
        74 => Some("home"),
        75 => Some("pageup"),
        76 => Some("delete"),
        77 => Some("end"),
        78 => Some("pagedown"),
        79 => Some("right"),
        80 => Some("left"),
        81 => Some("down"),
        82 => Some("up"),
        _ => None,
    };
    let mut key_char = printable(characters).then(|| characters.to_owned());
    let key = if let Some(named) = named {
        if hid != 44 {
            key_char = None;
        }
        named.to_owned()
    } else if (58..=69).contains(&hid) {
        key_char = None;
        format!("f{}", hid - 57)
    } else if (104..=115).contains(&hid) {
        key_char = None;
        format!("f{}", hid - 91)
    } else if printable(ignoring) {
        if ignoring.chars().all(char::is_alphabetic) {
            ignoring.to_lowercase()
        } else {
            ignoring.to_owned()
        }
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
        let key = keyboard.press(31, "\"", "2", 1 << 17).unwrap();
        assert_eq!(key.keystroke.key, "2");
        assert_eq!(key.keystroke.key_char.as_deref(), Some("\""));
        assert!(key.keystroke.modifiers.shift);
        assert_eq!(
            keyboard
                .press(44, " ", " ", 0)
                .unwrap()
                .keystroke
                .key_char
                .as_deref(),
            Some(" ")
        );
        assert_eq!(keyboard.press(4, "q", "q", 0).unwrap().keystroke.key, "q");
    }

    #[test]
    fn control_and_named_keys_do_not_insert_text() {
        let mut keyboard = Keyboard::default();
        let key = keyboard.press(6, "\u{3}", "c", 1 << 18).unwrap();
        assert_eq!(key.keystroke.key, "c");
        assert!(key.keystroke.modifiers.control);
        assert_eq!(key.keystroke.key_char, None);
        assert_eq!(
            keyboard
                .press(82, "\u{f700}", "\u{f700}", 0)
                .unwrap()
                .keystroke
                .key,
            "up"
        );
        assert_eq!(keyboard.press(115, "", "", 0).unwrap().keystroke.key, "f24");
    }

    #[test]
    fn release_identity_survives_modifier_release() {
        let mut keyboard = Keyboard::default();
        keyboard.press(225, "", "", 1 << 17);
        assert_eq!(
            keyboard.press(46, "+", "=", 1 << 17).unwrap().keystroke.key,
            "="
        );
        keyboard.release(225, 1 << 17);
        assert!(!keyboard.modifiers.shift);
        assert_eq!(keyboard.release(46, 0).unwrap().key, "=");
    }

    #[test]
    fn both_shifts_and_cancelled_repeat() {
        let mut keyboard = Keyboard::default();
        keyboard.press(225, "", "", 1 << 17);
        keyboard.press(229, "", "", 1 << 17);
        keyboard.release(225, 1 << 17);
        assert!(keyboard.modifiers.shift);
        keyboard.release(229, 1 << 17);
        let now = Instant::now();
        keyboard.press(42, "", "", 0);
        keyboard.consumed(42, false, now);
        assert!(keyboard.repeat(now + Duration::from_millis(449)).is_none());
        assert!(
            keyboard
                .repeat(now + Duration::from_millis(450))
                .unwrap()
                .is_held
        );
        assert_eq!(keyboard.clear().len(), 1);
        assert!(keyboard.repeat(now + Duration::from_secs(1)).is_none());
    }
}
