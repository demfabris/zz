use adw::prelude::*;
use gtk::gdk;
use zz_client::{ChromeAction, ChromeKey, ChromeKeymap, SIDEBAR_TABLE, TERMINAL_TABLE, UI_TABLE};

const GENERAL: &[(ChromeAction, &str)] = &[
    (ChromeAction::Detach, "Detach"),
    (ChromeAction::ToggleSidebar, "Toggle Sidebar"),
    (ChromeAction::OpenSettings, "Open Preferences"),
    (ChromeAction::UiZoomIn, "Zoom In"),
    (ChromeAction::UiZoomOut, "Zoom Out"),
    (ChromeAction::UiZoomReset, "Reset Zoom"),
];

const TERMINAL: &[(ChromeAction, &str)] = &[
    (ChromeAction::ClosePane, "Close Pane"),
    (ChromeAction::TerminalSearch, "Search"),
    (ChromeAction::TerminalCopy, "Copy"),
    (ChromeAction::TerminalPaste, "Paste"),
    (ChromeAction::TerminalSelectAll, "Select All"),
    (ChromeAction::TerminalClearHistory, "Clear Scrollback"),
    (ChromeAction::TerminalFontIncrease, "Increase Text Size"),
    (ChromeAction::TerminalFontDecrease, "Decrease Text Size"),
];

const SIDEBAR: &[(ChromeAction, &str)] = &[
    (ChromeAction::SidebarConfirm, "Open Selection"),
    (ChromeAction::SidebarCancel, "Cancel"),
    (ChromeAction::SidebarRename, "Rename"),
    (ChromeAction::SidebarCommandPalette, "Open Command Palette"),
    (ChromeAction::SidebarSelectUp, "Move Up"),
    (ChromeAction::SidebarSelectDown, "Move Down"),
    (ChromeAction::SidebarSelectLeft, "Collapse"),
    (ChromeAction::SidebarSelectRight, "Expand"),
    (ChromeAction::SidebarSelectFirst, "Move to First Item"),
    (ChromeAction::SidebarSelectLast, "Move to Last Item"),
];

pub fn present(parent: &impl IsA<gtk::Widget>, chrome: &ChromeKeymap) {
    let dialog = adw::ShortcutsDialog::builder()
        .title("Keyboard Shortcuts")
        .build();
    let general = adw::ShortcutsSection::new(Some("General"));
    add_bindings(&general, chrome, UI_TABLE, GENERAL);
    general.add(adw::ShortcutsItem::new(
        "Open Menu",
        &gtk::accelerator_name(gdk::Key::F10, gdk::ModifierType::empty()),
    ));
    general.add(adw::ShortcutsItem::new(
        "Keyboard Shortcuts",
        &gtk::accelerator_name(gdk::Key::question, gdk::ModifierType::CONTROL_MASK),
    ));
    dialog.add(general);
    add_section(&dialog, chrome, "Terminal", TERMINAL_TABLE, TERMINAL);
    add_section(&dialog, chrome, "Sessions", SIDEBAR_TABLE, SIDEBAR);
    dialog.present(Some(parent));
}

fn add_section(
    dialog: &adw::ShortcutsDialog,
    chrome: &ChromeKeymap,
    title: &str,
    table: &str,
    entries: &[(ChromeAction, &str)],
) {
    let section = adw::ShortcutsSection::new(Some(title));
    add_bindings(&section, chrome, table, entries);
    if section.n_items() > 0 {
        dialog.add(section);
    }
}

fn add_bindings(
    section: &adw::ShortcutsSection,
    chrome: &ChromeKeymap,
    table: &str,
    entries: &[(ChromeAction, &str)],
) {
    let bindings = chrome.table_bindings(table);
    for (action, title) in entries {
        let mut accelerators: Vec<String> = bindings
            .iter()
            .filter(|(_, bound)| bound == action)
            .filter_map(|(key, _)| accelerator(key))
            .collect();
        accelerators.sort_by_key(String::len);
        accelerators.dedup();
        if !accelerators.is_empty() {
            section.add(adw::ShortcutsItem::new(title, &accelerators.join(" ")));
        }
    }
}

fn accelerator(key: &ChromeKey) -> Option<String> {
    let (keyval, modifiers) = accelerator_input(key)?;
    Some(gtk::accelerator_name(keyval, modifiers).to_string())
}

fn accelerator_input(key: &ChromeKey) -> Option<(gdk::Key, gdk::ModifierType)> {
    let keyval = match key.base.as_str() {
        " " => gdk::Key::space,
        "Enter" => gdk::Key::Return,
        "NPage" => gdk::Key::Page_Down,
        "PPage" => gdk::Key::Page_Up,
        base => gdk::Key::from_name(base).or_else(|| punctuation_key(base))?,
    };
    let mut modifiers = gdk::ModifierType::empty();
    modifiers.set(gdk::ModifierType::SUPER_MASK, key.command);
    modifiers.set(gdk::ModifierType::CONTROL_MASK, key.control);
    modifiers.set(gdk::ModifierType::ALT_MASK, key.alt);
    modifiers.set(gdk::ModifierType::SHIFT_MASK, key.shift);
    Some((keyval, modifiers))
}

fn punctuation_key(value: &str) -> Option<gdk::Key> {
    let name = match value {
        "!" => "exclam",
        "\"" => "quotedbl",
        "#" => "numbersign",
        "$" => "dollar",
        "%" => "percent",
        "&" => "ampersand",
        "'" => "apostrophe",
        "(" => "parenleft",
        ")" => "parenright",
        "*" => "asterisk",
        "+" => "plus",
        "," => "comma",
        "-" => "minus",
        "." => "period",
        "/" => "slash",
        ":" => "colon",
        ";" => "semicolon",
        "<" => "less",
        "=" => "equal",
        ">" => "greater",
        "?" => "question",
        "@" => "at",
        "[" => "bracketleft",
        "\\" => "backslash",
        "]" => "bracketright",
        "^" => "asciicircum",
        "_" => "underscore",
        "`" => "grave",
        "{" => "braceleft",
        "|" => "bar",
        "}" => "braceright",
        "~" => "asciitilde",
        _ => return None,
    };
    gdk::Key::from_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_keys_map_to_gtk_accelerator_parts() {
        let cases = [
            (
                "C-S-f",
                gdk::Key::f,
                gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK,
            ),
            ("M-Left", gdk::Key::Left, gdk::ModifierType::ALT_MASK),
            (
                "C-NPage",
                gdk::Key::Page_Down,
                gdk::ModifierType::CONTROL_MASK,
            ),
            ("Enter", gdk::Key::Return, gdk::ModifierType::empty()),
            ("Space", gdk::Key::space, gdk::ModifierType::empty()),
        ];

        for (spelling, keyval, modifiers) in cases {
            let key = ChromeKey::parse(spelling).unwrap();
            assert_eq!(accelerator_input(&key), Some((keyval, modifiers)));
        }
    }

    #[test]
    fn every_displayed_desktop_binding_has_a_gtk_spelling() {
        let chrome = ChromeKeymap::for_profile(zz_client::ChromeProfile::DESKTOP);

        for table in [UI_TABLE, TERMINAL_TABLE, SIDEBAR_TABLE] {
            for (key, _) in chrome.table_bindings(table) {
                assert!(accelerator_input(&key).is_some(), "{table}: {key}");
            }
        }
    }
}
