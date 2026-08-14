use std::{
    fmt::{self, Write as _},
    ops::Deref,
    sync::Arc,
};

use smallvec::SmallVec;
use zz_protocol::{ChooseBufferAction, ChooseTreeAction, KeyTables, KeyToken};
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

/// The `send-keys -X <action>` name a chooser table binds `key` to, if any.
fn bound_overlay_action<'a>(keys: &'a KeyTables, table: &str, key: &str) -> Option<&'a str> {
    let binding = keys.get(table, key)?;
    let [command] = binding.commands.as_slice() else {
        return None;
    };
    if !matches!(command.name.as_str(), "send" | "send-keys") {
        return None;
    }
    let mode_index = command.args.iter().position(|argument| argument == "-X")?;
    command.args.get(mode_index + 1).map(String::as_str)
}

/// Resolve a key press against a chooser table, preferring the character the
/// press typed (`?` from shift+`/`) over the folded physical key name.
fn overlay_key_action<'a>(keys: &'a KeyTables, table: &str, input: &KeyInput) -> Option<&'a str> {
    if let Some(text) = overlay_search_text(input)
        && text.chars().count() == 1
        && let Some(action) = bound_overlay_action(keys, table, &text)
    {
        return Some(action);
    }
    bound_overlay_action(keys, table, input_key_name(input).as_str())
        .or_else(|| bound_overlay_action(keys, table, "Any"))
}

/// Printable text a key press contributes to an overlay search query.
fn overlay_search_text(input: &KeyInput) -> Option<String> {
    if input.modifiers.control() || input.modifiers.alt() || input.modifiers.platform() {
        return None;
    }
    let text = input.text.as_deref()?;
    (!text.is_empty() && !text.chars().any(char::is_control)).then(|| text.to_owned())
}

pub(crate) fn choose_tree_key_action(
    keys: &KeyTables,
    input: &KeyInput,
    searching: bool,
) -> Option<ChooseTreeAction> {
    if searching {
        return match input_key_name(input).as_str() {
            "Escape" | "C-g" | "C-c" | "C-[" => Some(ChooseTreeAction::SearchCancel),
            "Enter" => Some(ChooseTreeAction::SearchAccept),
            "BSpace" => Some(ChooseTreeAction::SearchBackspace),
            "Up" => Some(ChooseTreeAction::Previous),
            "Down" => Some(ChooseTreeAction::Next),
            _ => overlay_search_text(input).map(ChooseTreeAction::SearchAppend),
        };
    }
    match overlay_key_action(keys, "choose-tree", input)? {
        "cursor-up" => Some(ChooseTreeAction::Previous),
        "cursor-down" => Some(ChooseTreeAction::Next),
        "page-up" => Some(ChooseTreeAction::PagePrevious),
        "page-down" => Some(ChooseTreeAction::PageNext),
        "history-top" => Some(ChooseTreeAction::First),
        "history-bottom" => Some(ChooseTreeAction::Last),
        "collapse" => Some(ChooseTreeAction::Collapse),
        "expand" => Some(ChooseTreeAction::Expand),
        "accept" => Some(ChooseTreeAction::Activate),
        "cancel" => Some(ChooseTreeAction::Close),
        "search-forward" => Some(ChooseTreeAction::SearchStart { reverse: false }),
        "search-backward" => Some(ChooseTreeAction::SearchStart { reverse: true }),
        "search-again" => Some(ChooseTreeAction::SearchNext { reverse: false }),
        "search-reverse" => Some(ChooseTreeAction::SearchNext { reverse: true }),
        _ => None,
    }
}

pub(crate) fn choose_buffer_key_action(
    keys: &KeyTables,
    input: &KeyInput,
    searching: bool,
) -> Option<ChooseBufferAction> {
    if searching {
        return match input_key_name(input).as_str() {
            "Escape" | "C-g" | "C-c" | "C-[" => Some(ChooseBufferAction::SearchCancel),
            "Enter" => Some(ChooseBufferAction::SearchAccept),
            "BSpace" => Some(ChooseBufferAction::SearchBackspace),
            "Up" => Some(ChooseBufferAction::Previous),
            "Down" => Some(ChooseBufferAction::Next),
            _ => overlay_search_text(input).map(ChooseBufferAction::SearchAppend),
        };
    }
    match overlay_key_action(keys, "choose-buffer", input)? {
        "cursor-up" => Some(ChooseBufferAction::Previous),
        "cursor-down" => Some(ChooseBufferAction::Next),
        "page-up" => Some(ChooseBufferAction::PagePrevious),
        "page-down" => Some(ChooseBufferAction::PageNext),
        "history-top" => Some(ChooseBufferAction::First),
        "history-bottom" => Some(ChooseBufferAction::Last),
        "accept" | "paste" => Some(ChooseBufferAction::Paste),
        "delete" => Some(ChooseBufferAction::Delete),
        "cancel" => Some(ChooseBufferAction::Close),
        "search-forward" => Some(ChooseBufferAction::SearchStart { reverse: false }),
        "search-backward" => Some(ChooseBufferAction::SearchStart { reverse: true }),
        "search-again" => Some(ChooseBufferAction::SearchNext { reverse: false }),
        "search-reverse" => Some(ChooseBufferAction::SearchNext { reverse: true }),
        _ => None,
    }
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

    fn character(value: char) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key: KeyCode::Character(value),
            modifiers: Modifiers::default(),
            text: Some(value.to_string().into_boxed_str()),
            unshifted_codepoint: Some(value),
        }
    }

    fn named(value: KeyCode) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key: value,
            modifiers: Modifiers::default(),
            text: None,
            unshifted_codepoint: None,
        }
    }

    #[test]
    fn default_chooser_tables_resolve_navigation_keys() {
        let keys = KeyTables::default();
        assert_eq!(
            choose_tree_key_action(&keys, &character('j'), false),
            Some(ChooseTreeAction::Next)
        );
        assert_eq!(
            choose_tree_key_action(&keys, &named(KeyCode::Enter), false),
            Some(ChooseTreeAction::Activate)
        );
        assert_eq!(
            choose_tree_key_action(&keys, &named(KeyCode::Escape), false),
            Some(ChooseTreeAction::Close)
        );
        assert_eq!(
            choose_buffer_key_action(&keys, &character('p'), false),
            Some(ChooseBufferAction::Paste)
        );
        assert_eq!(
            choose_buffer_key_action(&keys, &character('d'), false),
            Some(ChooseBufferAction::Delete)
        );
        assert_eq!(choose_tree_key_action(&keys, &character('z'), false), None);
    }

    #[test]
    fn shifted_punctuation_resolves_by_typed_character() {
        let keys = KeyTables::default();
        let mut question = character('?');
        question.key = KeyCode::Character('/');
        question.modifiers = Modifiers::new(true, false, false, false);
        assert_eq!(
            choose_tree_key_action(&keys, &question, false),
            Some(ChooseTreeAction::SearchStart { reverse: true })
        );
    }

    #[test]
    fn chooser_search_mode_edits_and_navigates() {
        let keys = KeyTables::default();
        assert_eq!(
            choose_tree_key_action(&keys, &named(KeyCode::Escape), true),
            Some(ChooseTreeAction::SearchCancel)
        );
        assert_eq!(
            choose_tree_key_action(&keys, &named(KeyCode::Backspace), true),
            Some(ChooseTreeAction::SearchBackspace)
        );
        assert_eq!(
            choose_tree_key_action(&keys, &character('j'), true),
            Some(ChooseTreeAction::SearchAppend("j".to_owned()))
        );
        let mut control = character('g');
        control.modifiers = Modifiers::new(false, true, false, false);
        assert_eq!(
            choose_buffer_key_action(&keys, &control, true),
            Some(ChooseBufferAction::SearchCancel)
        );
    }

    #[test]
    fn chooser_bindings_are_rebindable_through_the_tables() {
        let mut keys = KeyTables::default();
        keys.bind(
            "choose-tree",
            "x",
            zz_protocol::Binding {
                commands: vec![zz_protocol::CommandInvocation::new(
                    "send-keys",
                    ["-X", "cancel"],
                )],
                repeat: false,
                note: None,
            },
        );
        assert_eq!(
            choose_tree_key_action(&keys, &character('x'), false),
            Some(ChooseTreeAction::Close)
        );
        assert!(keys.unbind("choose-tree", "j"));
        assert_eq!(choose_tree_key_action(&keys, &character('j'), false), None);
    }
}
