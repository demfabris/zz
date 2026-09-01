//! Which row shortcut spellings `display-menu` turns into a key.
//!
//! The pin runs every spelling through `key_string_lookup_string` and renders
//! nothing when it answers `KEYC_UNKNOWN` (menu.c `menu_add_item`). zz answers
//! the same question with the vocabulary its own clients speak, so a spelling
//! the pin parses but no attached client can press is no key here either.

#![cfg(all(unix, feature = "daemon"))]

mod overlay;

use zz_protocol::{CommandInvocation, InputMessage, MenuAction};

use overlay::Overlays;

/// `name`, the spelling handed to `display-menu`, and the key the descriptor
/// should carry.
const ROWS: &[(&str, &str, Option<&str>)] = &[
    ("plain", "a", Some("a")),
    ("upper", "A", Some("A")),
    ("digit", "7", Some("7")),
    ("control", "C-a", Some("C-a")),
    ("both-modifiers", "C-M-x", Some("C-M-x")),
    ("caret", "^A", Some("C-a")),
    ("hex", "0x41", None),
    ("long-modifier", "Ctrl-Alt-x", None),
    ("space", "Space", Some(" ")),
    ("wide", "\u{e9}", Some("\u{e9}")),
    ("named", "PPage", Some("PPage")),
    ("erase", "BSpace", Some("BSpace")),
    ("backtab", "BTab", Some("BTab")),
    ("last-function", "F12", Some("F12")),
    ("past-function", "F13", None),
    ("wrapped-function", "F256", None),
    ("zero-function", "F0", None),
    ("shifted", "S-a", None),
    ("word", "Frobnicate", None),
    ("nothing", "None", None),
    ("blank", "", None),
];

#[test]
fn display_menu_keeps_only_shortcuts_an_attached_client_can_press() {
    let overlays = Overlays::start("menu-grammar");
    let mut args = vec!["-c".to_owned(), overlays.client_name.clone()];
    for (name, key, _) in ROWS {
        args.extend([(*name).to_owned(), (*key).to_owned(), String::new()]);
    }
    let command = overlays.spawn_command(CommandInvocation::new("display-menu", args));

    let state = overlays.await_menu();
    let observed = state
        .items
        .iter()
        .map(|item| {
            let item = item.as_ref().expect("no separator rows were asked for");
            (item.name.clone(), item.key.clone())
        })
        .collect::<Vec<_>>();
    let expected = ROWS
        .iter()
        .map(|(name, _, key)| ((*name).to_owned(), key.map(str::to_owned)))
        .collect::<Vec<_>>();
    assert_eq!(observed, expected);

    overlays
        .client
        .send_input(InputMessage::Menu {
            action: MenuAction::Cancel,
        })
        .expect("close the menu");
    command
        .join()
        .expect("the display-menu thread")
        .expect("display-menu");
}

#[test]
fn a_disabled_row_carries_no_shortcut() {
    let overlays = Overlays::start("menu-grammar-disabled");
    let command = overlays.spawn_command(CommandInvocation::new(
        "display-menu",
        [
            "-c",
            &overlays.client_name,
            "live",
            "l",
            "",
            "-greyed",
            "g",
            "",
        ],
    ));

    let state = overlays.await_menu();
    let items = state
        .items
        .iter()
        .map(|item| item.as_ref().expect("no separator rows"))
        .collect::<Vec<_>>();
    assert_eq!(items[0].name, "live");
    assert!(items[0].enabled);
    assert_eq!(items[0].key.as_deref(), Some("l"));
    assert_eq!(items[1].name, "greyed");
    assert!(!items[1].enabled);
    assert_eq!(items[1].key, None);

    overlays
        .client
        .send_input(InputMessage::Menu {
            action: MenuAction::Cancel,
        })
        .expect("close the menu");
    command
        .join()
        .expect("the display-menu thread")
        .expect("display-menu");
}
