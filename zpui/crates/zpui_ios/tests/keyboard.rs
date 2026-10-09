use zpui_ios::keyboard;

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
