use std::sync::Arc;

pub(crate) use zz_protocol::input_key_name;
use zz_protocol::{ChooseBufferAction, ChooseTreeAction, KeyTables, KeyToken, input_typed_text};
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers, TerminalSession};

/// The `send-keys -X <action>` name a chooser table resolves a key press to.
fn overlay_key_action<'a>(keys: &'a KeyTables, table: &str, input: &KeyInput) -> Option<&'a str> {
    let binding = keys.resolve_input(table, input)?;
    let [command] = binding.commands.as_slice() else {
        return None;
    };
    if !matches!(command.name.as_str(), "send" | "send-keys") {
        return None;
    }
    let mode_index = command.args.iter().position(|argument| argument == "-X")?;
    command.args.get(mode_index + 1).map(String::as_str)
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
            _ => {
                input_typed_text(input).map(|text| ChooseTreeAction::SearchAppend(text.to_owned()))
            }
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
        "tag" => Some(ChooseTreeAction::Tag),
        "tag-none" => Some(ChooseTreeAction::TagNone),
        "tag-all" => Some(ChooseTreeAction::TagAll),
        "kill" => Some(ChooseTreeAction::KillCurrent),
        "kill-tagged" => Some(ChooseTreeAction::KillTagged),
        _ => None,
    }
}

/// `prompt_key` under `PROMPT_SINGLE` takes one character as the whole answer;
/// a key that carries none is handled and leaves the prompt open.
pub(crate) fn chooser_prompt_answer(input: &KeyInput) -> Option<char> {
    if let Some(text) = input_typed_text(input) {
        return text.chars().next();
    }
    match input_key_name(input).as_str() {
        "Escape" | "C-[" => Some('\u{1b}'),
        "Enter" => Some('\r'),
        "Space" => Some(' '),
        "BSpace" => Some('\u{7f}'),
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
            _ => input_typed_text(input)
                .map(|text| ChooseBufferAction::SearchAppend(text.to_owned())),
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

pub(crate) fn send_tokens(sessions: &[Arc<TerminalSession>], tokens: &[KeyToken]) -> bool {
    let mut sent = false;
    for token in tokens {
        match token {
            KeyToken::Literal(text) => {
                let text = Arc::<str>::from(text.as_str());
                for session in sessions {
                    if !session.try_send_text(Arc::clone(&text)) {
                        return false;
                    }
                    sent = true;
                }
            }
            KeyToken::Raw(byte) => {
                let bytes = Arc::<[u8]>::from([*byte].as_slice());
                for session in sessions {
                    if !session.send_raw_input(Arc::clone(&bytes)) {
                        return false;
                    }
                    sent = true;
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
                        if !session.try_send_key(session_input) {
                            return false;
                        }
                        sent = true;
                    }
                }
            }
        }
    }
    sent
}

/// The key presses `cmd_send_keys_inject_key` hands to
/// `server_client_handle_key` for one `send-keys -K` argument.
/// `cmd_send_keys_inject_string` looks the argument up as a key name first and
/// falls back to injecting one key per character, so a literal token spends a
/// press per character rather than writing text.
pub(crate) fn client_key_inputs(token: &KeyToken) -> Vec<KeyInput> {
    match token {
        KeyToken::Named(name) => named_key(name).into_iter().collect(),
        KeyToken::Literal(text) => text
            .chars()
            .filter_map(|character| named_key(character.encode_utf8(&mut [0_u8; 4])))
            .collect(),
        KeyToken::Raw(byte) => char::from_u32(u32::from(*byte))
            .filter(char::is_ascii)
            .and_then(|character| named_key(character.encode_utf8(&mut [0_u8; 4])))
            .into_iter()
            .collect(),
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
    fn named_keys_round_trip_through_the_shared_fold() {
        for name in ["Enter", "C-M-Left", "C-M-F255", "C-M-λ"] {
            let input = named_key(name).expect("supported named key");
            assert_eq!(input_key_name(&input).as_str(), name);
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
