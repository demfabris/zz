//! A menu row answers to its action key whether or not it draws it.
//!
//! The pin drops `key` from the drawn row name when the bracketed form does
//! not fit, but leaves `new_item->key` set, so the row still answers the press
//! (menu.c `menu_add_item`). zz publishes those two facts as separate fields,
//! so the client resolves presses from `key` and draws only `annotation`.

use zz_client::{MenuKeyResult, resolve_menu_key};
use zz_protocol::{MenuAction, MenuItem, MenuState, PopupBorderLines};
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers};

fn state(items: Vec<Option<MenuItem>>) -> MenuState {
    MenuState {
        left: 0,
        top: 0,
        width: 40,
        height: 6,
        client_columns: 40,
        client_rows: 24,
        cell_width_px: 8,
        cell_height_px: 16,
        title: String::new(),
        style: "default".to_owned(),
        selected_style: "default".to_owned(),
        border_style: "default".to_owned(),
        border_lines: PopupBorderLines::Single,
        items,
        selected: Some(0),
        stay_open: false,
    }
}

fn press(key: KeyCode, modifiers: Modifiers) -> KeyInput {
    let character = match key {
        KeyCode::Character(character) => Some(character),
        _ => None,
    };
    KeyInput {
        action: KeyAction::Press,
        key,
        modifiers,
        text: character.map(|character| character.to_string().into_boxed_str()),
        unshifted_codepoint: character,
    }
}

fn drawn_row(item: &MenuItem) -> String {
    item.annotation
        .as_deref()
        .filter(|annotation| !annotation.is_empty())
        .map_or_else(
            || item.name.clone(),
            |annotation| format!("{}#[default] #[align=right]({annotation})", item.name),
        )
}

#[test]
fn a_row_with_no_annotation_still_answers_its_action_key() {
    let hidden = MenuItem {
        name: "B".repeat(30),
        key: Some("M-Enter".to_owned()),
        annotation: None,
        enabled: true,
    };
    let shown = MenuItem {
        name: "CCCCCCCCCC".to_owned(),
        key: Some("M-Enter".to_owned()),
        annotation: Some("M-Enter".to_owned()),
        enabled: true,
    };

    assert_eq!(drawn_row(&hidden), "B".repeat(30));
    assert!(!drawn_row(&hidden).contains("(M-Enter)"));
    assert_eq!(
        drawn_row(&shown),
        "CCCCCCCCCC#[default] #[align=right](M-Enter)"
    );

    let state = state(vec![Some(hidden), Some(shown)]);
    let mut modifiers = Modifiers::default();
    modifiers.set_alt(true);
    assert_eq!(
        resolve_menu_key(&state, Some(0), &press(KeyCode::Enter, modifiers)),
        MenuKeyResult::Action(MenuAction::Choose(0)),
        "the first row owns the spelling even though it draws no annotation"
    );
}

#[test]
fn a_trimmed_row_keeps_its_marker_and_its_key() {
    let trimmed = MenuItem {
        name: format!("{}>", "A".repeat(31)),
        key: Some("a".to_owned()),
        annotation: Some("a".to_owned()),
        enabled: true,
    };
    assert!(trimmed.name.ends_with('>'));
    assert_eq!(
        drawn_row(&trimmed),
        format!("{}>#[default] #[align=right](a)", "A".repeat(31))
    );

    let state = state(vec![Some(trimmed)]);
    assert_eq!(
        resolve_menu_key(
            &state,
            Some(0),
            &press(KeyCode::Character('a'), Modifiers::default())
        ),
        MenuKeyResult::Action(MenuAction::Choose(0))
    );
}
