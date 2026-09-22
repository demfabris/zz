//! Client-side claim of the configured multiplexer prefix.

use gpui::Keystroke;
use zz_client::ChromeKey;
use zz_protocol::{Binding, KeyBindingSnapshot, KeyTables, TmuxOption};
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers as TerminalModifiers};

/// The canonical prefix as a GPUI keystroke, so a hint can print it in the
/// platform's own glyphs (`⌃B`, `Ctrl+B`) rather than tmux's `C-b` spelling.
pub(crate) fn display_keystroke(canonical: &str) -> Option<Keystroke> {
    let key = ChromeKey::parse(canonical)?;
    Keystroke::parse(&crate::keymap::gpui_source(&key)?).ok()
}

pub(crate) fn is_sidebar_picker_input(bindings: &[KeyBindingSnapshot], input: &KeyInput) -> bool {
    let mut tables = KeyTables::empty();
    for binding in bindings {
        tables.bind(
            "prefix",
            &binding.key,
            Binding {
                commands: binding.commands.clone(),
                repeat: binding.repeat,
                note: None,
            },
        );
    }
    let Some(binding) = tables.resolve_input("prefix", input) else {
        return false;
    };
    let [command] = binding.commands.as_slice() else {
        return false;
    };
    if command.name != "choose-tree" {
        return false;
    }
    let Some(spec) = zz_protocol::command_spec(&command.name) else {
        return false;
    };
    let Ok(parsed) = zz_protocol::parse_tmux_command_options(spec, command) else {
        return false;
    };
    parsed.positionals.is_empty()
        && (parsed.options.contains(&TmuxOption::Flag("-s"))
            ^ parsed.options.contains(&TmuxOption::Flag("-w")))
        && parsed
            .options
            .iter()
            .all(|option| matches!(option, TmuxOption::Flag("-s" | "-w" | "-Z")))
}

/// Encode a GPUI keystroke as the wire `KeyInput`.
pub(crate) fn terminal_key_input(keystroke: &Keystroke, action: KeyAction) -> KeyInput {
    let key = terminal_key(&keystroke.key);
    let character = match key {
        KeyCode::Character(character) => Some(character),
        _ => None,
    };
    KeyInput {
        action,
        key,
        modifiers: TerminalModifiers::new(
            keystroke.modifiers.shift,
            keystroke.modifiers.control,
            keystroke.modifiers.alt,
            keystroke.modifiers.platform,
        ),
        text: keystroke
            .key_char
            .clone()
            .or_else(|| character.map(|value| value.to_string()))
            .map(String::into_boxed_str),
        unshifted_codepoint: character,
    }
}

fn terminal_key(key: &str) -> KeyCode {
    match key {
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
        value => {
            let mut characters = value.chars();
            if let (Some(character), None) = (characters.next(), characters.next()) {
                KeyCode::Character(character)
            } else {
                value
                    .strip_prefix('f')
                    .and_then(|number| number.parse::<u8>().ok())
                    .map_or(KeyCode::Unidentified, KeyCode::Function)
            }
        }
    }
}
