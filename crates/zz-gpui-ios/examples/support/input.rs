use gpui::Keystroke;
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers};

pub fn key_input(keystroke: &Keystroke, action: KeyAction) -> KeyInput {
    let key = match keystroke.key.as_str() {
        "space" => KeyCode::Character(' '),
        "backspace" => KeyCode::Backspace,
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "escape" => KeyCode::Escape,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        value if value.chars().count() == 1 => KeyCode::Character(value.chars().next().unwrap()),
        value => value
            .strip_prefix('f')
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=24).contains(n))
            .map_or(KeyCode::Unidentified, KeyCode::Function),
    };
    let unshifted_codepoint = if let KeyCode::Character(character) = key {
        Some(character)
    } else {
        None
    };
    KeyInput {
        action,
        key,
        modifiers: wire_modifiers(keystroke.modifiers),
        unshifted_codepoint,
        text: (action != KeyAction::Release && !keystroke.modifiers.alt)
            .then(|| keystroke.key_char.clone())
            .flatten()
            .filter(|text| !text.chars().any(char::is_control))
            .map(String::into_boxed_str),
    }
}

pub fn wire_modifiers(modifiers: gpui::Modifiers) -> Modifiers {
    Modifiers::new(
        modifiers.shift,
        modifiers.control,
        modifiers.alt,
        modifiers.platform,
    )
}

pub fn raw_key_bindings() -> [gpui::KeyBinding; 2] {
    [
        gpui::KeyBinding::new("tab", gpui::NoAction, Some("Terminal")),
        gpui::KeyBinding::new("shift-tab", gpui::NoAction, Some("Terminal")),
    ]
}
