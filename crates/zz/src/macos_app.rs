use gpui::{App, KeyBinding, Menu, MenuItem, SystemMenuType};

use crate::config::settings::OpenSettings;
use crate::workspace::ClosePane;

gpui::actions!(
    zz,
    [Quit, Hide, HideOthers, ShowAll, CloseWindow, Minimize, Zoom,]
);

pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.bind_keys(key_bindings());
    cx.set_menus(app_menus());
}

pub(crate) fn key_bindings() -> [KeyBinding; 6] {
    [
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-w", ClosePane, None),
        KeyBinding::new("cmd-shift-w", CloseWindow, None),
    ]
}

fn app_menus() -> [Menu; 2] {
    [
        Menu::new("zz").items([
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide zz", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit zz", Quit),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use gpui::{KeyContext, Keymap, Keystroke};

    use super::*;

    fn assert_binding<A: gpui::Action>(keymap: &Keymap, source: &str, contexts: &[KeyContext]) {
        let keystroke = Keystroke::parse(source).expect("valid macOS keystroke");
        let (bindings, pending) = keymap.bindings_for_input(&[keystroke], contexts);
        assert!(!pending);
        assert_eq!(bindings.len(), 1, "expected one binding for {source}");
        assert_eq!(bindings[0].action().as_any().type_id(), TypeId::of::<A>());
    }

    #[test]
    fn app_shortcuts_win_inside_a_focused_terminal() {
        let keymap = Keymap::new(key_bindings().into());
        let contexts = [
            KeyContext::parse("Root").expect("valid root context"),
            KeyContext::parse("Terminal").expect("valid terminal context"),
        ];

        assert_binding::<Quit>(&keymap, "cmd-q", &contexts);
        assert_binding::<Hide>(&keymap, "cmd-h", &contexts);
        assert_binding::<HideOthers>(&keymap, "alt-cmd-h", &contexts);
        assert_binding::<Minimize>(&keymap, "cmd-m", &contexts);
        assert_binding::<ClosePane>(&keymap, "cmd-w", &contexts);
        assert_binding::<CloseWindow>(&keymap, "cmd-shift-w", &contexts);
    }

    #[test]
    fn fullscreen_shortcut_is_left_to_macos() {
        let keymap = Keymap::new(key_bindings().into());
        let root = KeyContext::parse("Root").expect("valid root context");

        for context in ["Browser", "Terminal"] {
            let contexts = [
                root.clone(),
                KeyContext::parse(context).expect("valid pane context"),
            ];
            for source in ["f", "ctrl-cmd-f"] {
                let keystroke = Keystroke::parse(source).expect("valid macOS keystroke");
                let (bindings, pending) = keymap.bindings_for_input(&[keystroke], &contexts);
                assert!(!pending);
                assert!(
                    bindings.is_empty(),
                    "{source} in {context} must remain native input"
                );
            }
        }
    }
}
