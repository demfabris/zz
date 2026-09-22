#[path = "../examples/support/input.rs"]
mod input;
#[path = "../src/keyboard.rs"]
mod keyboard;

#[test]
fn terminal_input_preserves_layout_modifiers_and_release() {
    let mut keyboard = keyboard::Keyboard::default();
    let down = keyboard.press(31, "\"", "2", 1 << 17).unwrap();
    let input = input::key_input(&down.keystroke, zz_terminal::KeyAction::Press);
    assert_eq!(input.key, zz_terminal::KeyCode::Character('2'));
    assert_eq!(input.unshifted_codepoint, Some('2'));
    assert_eq!(input.text.as_deref(), Some("\""));
    assert!(input.modifiers.shift());
    let up = keyboard.release(31, 0).unwrap();
    let input = input::key_input(&up, zz_terminal::KeyAction::Release);
    assert_eq!(input.action, zz_terminal::KeyAction::Release);
    assert_eq!(input.text, None);
    assert_eq!(input.key, zz_terminal::KeyCode::Character('2'));
}

#[test]
fn terminal_tab_does_not_enter_toolbar_focus_navigation() {
    gpui::actions!(test_navigation, [Next, Previous]);
    let mut bindings = vec![
        gpui::KeyBinding::new("tab", Next, Some("Root")),
        gpui::KeyBinding::new("shift-tab", Previous, Some("Root")),
    ];
    bindings.extend(input::raw_key_bindings());
    let keymap = gpui::Keymap::new(bindings);
    let root = gpui::KeyContext::parse("Root").unwrap();
    let terminal = gpui::KeyContext::parse("Terminal").unwrap();
    for key in ["tab", "shift-tab"] {
        let key = gpui::Keystroke::parse(key).unwrap();
        let (bindings, pending) = keymap.bindings_for_input(
            std::slice::from_ref(&key),
            &[root.clone(), terminal.clone()],
        );
        assert!(bindings.is_empty());
        assert!(!pending);
        assert_eq!(
            keymap
                .bindings_for_input(&[key], std::slice::from_ref(&root))
                .0
                .len(),
            1
        );
    }
}

#[test]
fn option_chords_leave_encoding_to_the_daemon() {
    let mut keyboard = keyboard::Keyboard::default();
    let down = keyboard.press(5, "∫", "b", 1 << 19).unwrap();
    let input = input::key_input(&down.keystroke, zz_terminal::KeyAction::Press);
    assert!(input.modifiers.alt());
    assert_eq!(input.key, zz_terminal::KeyCode::Character('b'));
    assert_eq!(input.text, None);
}

#[test]
fn chrome_chords_become_menu_key_commands() {
    assert_eq!(keyboard::key_command("D-k"), Some(("k".into(), 1 << 20)));
    assert_eq!(
        keyboard::key_command("D-S-n"),
        Some(("n".into(), (1 << 20) | (1 << 17)))
    );
    assert_eq!(keyboard::key_command("D--"), Some(("-".into(), 1 << 20)));
    assert_eq!(keyboard::key_command("D-,"), Some((",".into(), 1 << 20)));
    assert_eq!(keyboard::key_command("D-Up"), None);
}
