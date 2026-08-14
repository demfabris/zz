//! The desktop skin's half of the chrome keymap.
//!
//! `zz-client` owns the chords: default tables per platform, the `bind`
//! spelling, and one resolution semantic shared with the daemon's pane input.
//! This module installs that keymap for the desktop profile, folds `zz/config`
//! overrides into it, and bridges its chords to gpui bindings — so every
//! converted surface switches on a named [`ChromeAction`] and never spells a
//! chord itself.

use std::{collections::BTreeMap, rc::Rc};

use gpui::{App, Global, KeyBinding, Keystroke};
use zz_client::{
    BROWSER_TABLE, CHROME_TABLES, ChromeAction, ChromeKey, ChromeKeymap, ChromeProfile,
};
use zz_terminal::KeyAction;

use crate::mux::prefix::terminal_key_input;

/// A `zz/config` chrome override, validated while the file is parsed so the
/// keymap only ever sees chords and actions it can honour.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChromeOverride {
    Bind {
        table: &'static str,
        key: String,
        action: String,
    },
    Unbind {
        table: &'static str,
        key: String,
    },
}

/// One chord a surface should bind, under the action it carries. A chord an
/// earlier configuration bound and this one does not comes back as not `live`:
/// gpui keymaps only grow, so the surface that owned the chord shadows it with
/// `NoAction` in its own context instead of removing it.
pub(crate) struct ChromeChord {
    action: ChromeAction,
    key: String,
    source: String,
    live: bool,
}

impl ChromeChord {
    pub(crate) const fn action(&self) -> ChromeAction {
        self.action
    }

    /// The chord in the chrome spelling, for surfaces that carry it into the
    /// action they dispatch.
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn binding(&self, action: impl gpui::Action, context: Option<&str>) -> KeyBinding {
        if self.live {
            KeyBinding::new(&self.source, action, context)
        } else {
            KeyBinding::new(&self.source, gpui::NoAction, context)
        }
    }
}

struct ChromeState {
    keymap: Rc<ChromeKeymap>,
    bound: BTreeMap<(&'static str, String), ChromeAction>,
    dropped: Vec<(&'static str, String, ChromeAction)>,
}

impl Global for ChromeState {}

/// Rebuild the chrome keymap from the built-in defaults plus the current
/// configuration, and republish it. Every surface bound through [`bind`]
/// re-emits its bindings.
pub(crate) fn install(overrides: &[ChromeOverride], element_selector_hotkey: &str, cx: &mut App) {
    let mut keymap = ChromeKeymap::for_profile(ChromeProfile::DESKTOP);
    rebind_element_selector(&mut keymap, element_selector_hotkey);
    for entry in overrides {
        match entry {
            ChromeOverride::Bind { table, key, action } => {
                if let Err(error) = keymap.bind(table, key, action) {
                    log::warn!(
                        target: "zz::config",
                        "ignoring chrome binding for unknown action `{}`",
                        error.0,
                    );
                }
            }
            ChromeOverride::Unbind { table, key } => {
                keymap.unbind(table, key);
            }
        }
    }

    let bound = bound_chords(&keymap);
    let dropped: Vec<_> = cx
        .try_global::<ChromeState>()
        .map(|state| {
            state
                .bound
                .iter()
                .filter(|(chord, _)| !bound.contains_key(*chord))
                .map(|((table, key), action)| (*table, key.clone(), *action))
                .collect()
        })
        .unwrap_or_default();
    log::info!(
        target: "zz::config",
        "chrome keymap chords={} overrides={} shadowed={}",
        bound.len(),
        overrides.len(),
        dropped.len(),
    );
    cx.set_global(ChromeState {
        keymap: Rc::new(keymap),
        bound,
        dropped,
    });
}

/// Bind one chrome table into the gpui keymap and keep it in step with the
/// configuration. `build` is the surface's switch from named actions to its own
/// gpui actions.
pub(crate) fn bind(
    cx: &mut App,
    table: &'static str,
    build: fn(&[ChromeChord]) -> Vec<KeyBinding>,
) {
    if cx.try_global::<ChromeState>().is_none() {
        install(
            &[],
            crate::config::DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY,
            cx,
        );
    }
    apply(cx, table, build);
    cx.observe_global::<ChromeState>(move |cx| apply(cx, table, build))
        .detach();
}

fn apply(cx: &mut App, table: &'static str, build: fn(&[ChromeChord]) -> Vec<KeyBinding>) {
    let bindings = build(&chords(cx, table));
    cx.bind_keys(bindings);
}

/// The action a press resolves to in `table`, for surfaces that read raw key
/// events instead of dispatching gpui actions.
pub(crate) fn resolve(cx: &App, table: &str, keystroke: &Keystroke) -> Option<ChromeAction> {
    keymap(cx)?.resolve(table, &terminal_key_input(keystroke, KeyAction::Press))
}

/// The action a chord carries right now, so a surface reached through a chord
/// can decline a binding the configuration has since replaced.
pub(crate) fn action_for(cx: &App, table: &str, key: &str) -> Option<ChromeAction> {
    keymap(cx)?.action_for(table, key)
}

/// The chord bound to `action`, in the platform's own gpui spelling, for hints
/// that print a shortcut.
pub(crate) fn chord_for(cx: &App, table: &str, action: ChromeAction) -> Option<String> {
    keymap(cx)?
        .table_bindings(table)
        .into_iter()
        .find(|(_, bound)| *bound == action)
        .and_then(|(key, _)| gpui_source(&key))
}

fn keymap(cx: &App) -> Option<Rc<ChromeKeymap>> {
    cx.try_global::<ChromeState>()
        .map(|state| Rc::clone(&state.keymap))
}

fn chords(cx: &App, table: &str) -> Vec<ChromeChord> {
    let Some(state) = cx.try_global::<ChromeState>() else {
        return Vec::new();
    };
    let mut chords = state
        .keymap
        .table_bindings(table)
        .into_iter()
        .filter_map(|(key, action)| {
            Some(ChromeChord {
                action,
                source: gpui_source(&key)?,
                key: key.to_string(),
                live: true,
            })
        })
        .collect::<Vec<_>>();
    chords.extend(
        state
            .dropped
            .iter()
            .filter(|(dropped, _, _)| *dropped == table)
            .filter_map(|(_, key, action)| {
                Some(ChromeChord {
                    action: *action,
                    source: gpui_source(&ChromeKey::parse(key)?)?,
                    key: key.clone(),
                    live: false,
                })
            }),
    );
    chords
}

/// How a press resolves with no configuration loaded, for surfaces that assert
/// their built-in chrome.
#[cfg(test)]
pub(crate) fn test_resolve(table: &str, keystroke: &Keystroke) -> Option<ChromeAction> {
    ChromeKeymap::for_profile(ChromeProfile::DESKTOP)
        .resolve(table, &terminal_key_input(keystroke, KeyAction::Press))
}

/// The chords one table carries with no configuration loaded, for surfaces
/// that assert their built-in keymap.
#[cfg(test)]
pub(crate) fn test_chords(table: &str) -> Vec<ChromeChord> {
    ChromeKeymap::for_profile(ChromeProfile::DESKTOP)
        .table_bindings(table)
        .into_iter()
        .filter_map(|(key, action)| {
            Some(ChromeChord {
                action,
                source: gpui_source(&key)?,
                key: key.to_string(),
                live: true,
            })
        })
        .collect()
}

fn bound_chords(keymap: &ChromeKeymap) -> BTreeMap<(&'static str, String), ChromeAction> {
    CHROME_TABLES
        .iter()
        .flat_map(|table| {
            keymap
                .table_bindings(table)
                .into_iter()
                .map(move |(key, action)| ((*table, key.to_string()), action))
        })
        .collect()
}

/// `browser-element-selector-hotkey` predates the chrome tables and keeps its
/// own settings surface, so it lands here as a rebind of the one chord that
/// carries the picker.
fn rebind_element_selector(keymap: &mut ChromeKeymap, hotkey: &str) {
    let Some(key) = Keystroke::parse(hotkey)
        .ok()
        .as_ref()
        .and_then(chrome_key_for_keystroke)
    else {
        log::warn!(
            target: "zz::config",
            "keeping the built-in element selector chord: `{hotkey}` is not a chord zz can bind",
        );
        return;
    };
    for (bound, action) in keymap.table_bindings(BROWSER_TABLE) {
        if action == ChromeAction::BrowserElementSelector && bound != key {
            keymap.unbind(BROWSER_TABLE, &bound.to_string());
        }
    }
    keymap
        .bind(
            BROWSER_TABLE,
            &key.to_string(),
            ChromeAction::BrowserElementSelector.name(),
        )
        .expect("chrome actions name themselves");
}

/// Parse a `chrome-keybind = <table>:<key>=<action>` value.
pub(crate) fn parse_bind(value: &str) -> Result<ChromeOverride, String> {
    let (target, action) = value
        .rsplit_once('=')
        .ok_or_else(|| "expected `<table>:<key>=<action>`".to_owned())?;
    let (table, key) = parse_target(target)?;
    let action = action.trim();
    if ChromeAction::from_name(action).is_none() {
        return Err(format!("unknown chrome action `{action}`"));
    }
    Ok(ChromeOverride::Bind {
        table,
        key,
        action: action.to_owned(),
    })
}

/// Parse a `chrome-unbind = <table>:<key>` value.
pub(crate) fn parse_unbind(value: &str) -> Result<ChromeOverride, String> {
    let (table, key) = parse_target(value)?;
    Ok(ChromeOverride::Unbind { table, key })
}

fn parse_target(target: &str) -> Result<(&'static str, String), String> {
    let (table, key) = target
        .split_once(':')
        .ok_or_else(|| "expected `<table>:<key>`".to_owned())?;
    let table = table.trim();
    let table = CHROME_TABLES
        .into_iter()
        .find(|known| *known == table)
        .ok_or_else(|| {
            format!(
                "unknown chrome table `{table}`; expected one of {}",
                CHROME_TABLES.join(", "),
            )
        })?;
    let chord = ChromeKey::parse(key)
        .filter(|chord| gpui_source(chord).is_some())
        .ok_or_else(|| format!("`{key}` is not a chord zz can bind"))?;
    Ok((table, chord.to_string()))
}

/// A chrome chord in gpui's own spelling, or `None` when gpui cannot express
/// it.
pub(crate) fn gpui_source(key: &ChromeKey) -> Option<String> {
    let mut source = String::new();
    if key.command {
        source.push_str("cmd-");
    }
    if key.control {
        source.push_str("ctrl-");
    }
    if key.alt {
        source.push_str("alt-");
    }
    if key.shift {
        source.push_str("shift-");
    }
    let base = match key.base.as_str() {
        " " => "space",
        named => gpui_key_name(named).unwrap_or(named),
    };
    let mut characters = base.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) if character.is_ascii_uppercase() => {
            if key.shift {
                return None;
            }
            source.push_str("shift-");
            source.extend(character.to_lowercase());
        }
        (Some(_), _) => source.push_str(base),
        (None, _) => return None,
    }
    Keystroke::parse(&source).ok().map(|_| source)
}

fn chrome_key_for_keystroke(keystroke: &Keystroke) -> Option<ChromeKey> {
    if keystroke.modifiers.function {
        return None;
    }
    let base = match keystroke.key.as_str() {
        "space" => " ".to_owned(),
        named => tmux_key_name(named).unwrap_or(named).to_owned(),
    };
    Some(
        ChromeKey {
            command: keystroke.modifiers.platform,
            control: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
            base,
        }
        .normalized(),
    )
}

/// The gpui name for a tmux key name, for the keys whose spellings differ.
pub(crate) fn gpui_key_name(name: &str) -> Option<&'static str> {
    KEY_NAMES
        .iter()
        .find(|(tmux, _)| *tmux == name)
        .map(|(_, gpui)| *gpui)
}

fn tmux_key_name(name: &str) -> Option<&'static str> {
    KEY_NAMES
        .iter()
        .find(|(_, gpui)| *gpui == name)
        .map(|(tmux, _)| *tmux)
}

const KEY_NAMES: [(&str, &str); 26] = [
    ("Enter", "enter"),
    ("Escape", "escape"),
    ("Tab", "tab"),
    ("BSpace", "backspace"),
    ("Up", "up"),
    ("Down", "down"),
    ("Left", "left"),
    ("Right", "right"),
    ("Home", "home"),
    ("End", "end"),
    ("PPage", "pageup"),
    ("NPage", "pagedown"),
    ("DC", "delete"),
    ("IC", "insert"),
    ("F1", "f1"),
    ("F2", "f2"),
    ("F3", "f3"),
    ("F4", "f4"),
    ("F5", "f5"),
    ("F6", "f6"),
    ("F7", "f7"),
    ("F8", "f8"),
    ("F9", "f9"),
    ("F10", "f10"),
    ("F11", "f11"),
    ("F12", "f12"),
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use zz_client::{SIDEBAR_TABLE, TERMINAL_TABLE, UI_TABLE};

    use super::*;

    fn audited(profile: ChromeProfile, table: &str) -> HashSet<(String, &'static str)> {
        ChromeKeymap::for_profile(profile)
            .table_bindings(table)
            .into_iter()
            .map(|(key, action)| {
                (
                    gpui_source(&key).unwrap_or_else(|| panic!("`{key}` bridges to gpui")),
                    action.name(),
                )
            })
            .collect()
    }

    fn expected<const N: usize>(
        bindings: [(&str, &'static str); N],
    ) -> HashSet<(String, &'static str)> {
        bindings
            .into_iter()
            .map(|(source, action)| (source.to_owned(), action))
            .collect()
    }

    /// The chords every converted surface used to spell in `KeyBinding::new`,
    /// per platform. Both halves are asserted on every host: the point of the
    /// keymap being data is that the platform it targets is a value, not a
    /// `cfg`.
    #[test]
    fn desktop_defaults_bridge_to_the_chords_they_replaced() {
        assert_eq!(
            audited(ChromeProfile::DesktopApple, UI_TABLE),
            expected([
                ("cmd-=", "ui-zoom-in"),
                ("cmd-+", "ui-zoom-in"),
                ("cmd--", "ui-zoom-out"),
                ("cmd-0", "ui-zoom-reset"),
                ("cmd-,", "open-settings"),
            ])
        );
        assert_eq!(
            audited(ChromeProfile::Desktop, UI_TABLE),
            expected([
                ("ctrl-=", "ui-zoom-in"),
                ("ctrl-+", "ui-zoom-in"),
                ("ctrl--", "ui-zoom-out"),
                ("ctrl-0", "ui-zoom-reset"),
                ("ctrl-,", "open-settings"),
            ])
        );
        assert_eq!(
            audited(ChromeProfile::DesktopApple, BROWSER_TABLE),
            expected([
                ("cmd-z", "browser-undo"),
                ("cmd-shift-z", "browser-redo"),
                ("cmd-x", "browser-cut"),
                ("cmd-c", "browser-copy"),
                ("cmd-v", "browser-paste"),
                ("cmd-shift-v", "browser-paste-and-match-style"),
                ("cmd-a", "browser-select-all"),
                ("cmd-=", "browser-zoom-in"),
                ("cmd-+", "browser-zoom-in"),
                ("cmd--", "browser-zoom-out"),
                ("cmd-0", "browser-zoom-reset"),
                ("cmd-alt-i", "browser-devtools"),
                ("cmd-t", "browser-new-tab"),
                ("ctrl-tab", "browser-next-tab"),
                ("ctrl-shift-tab", "browser-previous-tab"),
                ("cmd-alt-right", "browser-next-tab"),
                ("cmd-alt-left", "browser-previous-tab"),
                ("cmd-shift-]", "browser-next-tab"),
                ("cmd-shift-[", "browser-previous-tab"),
                ("cmd-9", "browser-select-last-tab"),
                ("cmd-l", "browser-focus-address"),
                ("cmd-r", "browser-reload"),
                ("cmd-[", "browser-back"),
                ("cmd-]", "browser-forward"),
                ("cmd-1", "browser-select-tab-1"),
                ("cmd-2", "browser-select-tab-2"),
                ("cmd-3", "browser-select-tab-3"),
                ("cmd-4", "browser-select-tab-4"),
                ("cmd-5", "browser-select-tab-5"),
                ("cmd-6", "browser-select-tab-6"),
                ("cmd-7", "browser-select-tab-7"),
                ("cmd-8", "browser-select-tab-8"),
                ("cmd-shift-c", "browser-element-selector"),
            ])
        );
        assert_eq!(
            audited(ChromeProfile::Desktop, BROWSER_TABLE),
            expected([
                ("ctrl-z", "browser-undo"),
                ("ctrl-y", "browser-redo"),
                ("ctrl-shift-z", "browser-redo"),
                ("ctrl-x", "browser-cut"),
                ("ctrl-c", "browser-copy"),
                ("ctrl-v", "browser-paste"),
                ("ctrl-shift-v", "browser-paste-and-match-style"),
                ("ctrl-a", "browser-select-all"),
                ("ctrl-=", "browser-zoom-in"),
                ("ctrl-+", "browser-zoom-in"),
                ("ctrl--", "browser-zoom-out"),
                ("ctrl-0", "browser-zoom-reset"),
                ("ctrl-shift-i", "browser-devtools"),
                ("ctrl-t", "browser-new-tab"),
                ("ctrl-w", "close-pane"),
                ("ctrl-tab", "browser-next-tab"),
                ("ctrl-shift-tab", "browser-previous-tab"),
                ("ctrl-pagedown", "browser-next-tab"),
                ("ctrl-pageup", "browser-previous-tab"),
                ("ctrl-9", "browser-select-last-tab"),
                ("ctrl-l", "browser-focus-address"),
                ("ctrl-r", "browser-reload"),
                ("f5", "browser-reload"),
                ("alt-left", "browser-back"),
                ("alt-right", "browser-forward"),
                ("ctrl-1", "browser-select-tab-1"),
                ("ctrl-2", "browser-select-tab-2"),
                ("ctrl-3", "browser-select-tab-3"),
                ("ctrl-4", "browser-select-tab-4"),
                ("ctrl-5", "browser-select-tab-5"),
                ("ctrl-6", "browser-select-tab-6"),
                ("ctrl-7", "browser-select-tab-7"),
                ("ctrl-8", "browser-select-tab-8"),
                ("ctrl-shift-c", "browser-element-selector"),
            ])
        );
        assert_eq!(
            audited(ChromeProfile::Desktop, SIDEBAR_TABLE),
            expected([
                ("escape", "sidebar-cancel"),
                ("q", "sidebar-cancel"),
                ("enter", "sidebar-confirm"),
                ("r", "sidebar-rename"),
                (":", "sidebar-command-palette"),
                ("down", "sidebar-select-down"),
                ("j", "sidebar-select-down"),
                ("up", "sidebar-select-up"),
                ("k", "sidebar-select-up"),
                ("left", "sidebar-select-left"),
                ("h", "sidebar-select-left"),
                ("right", "sidebar-select-right"),
                ("l", "sidebar-select-right"),
                ("g", "sidebar-select-first"),
                ("home", "sidebar-select-first"),
                ("shift-g", "sidebar-select-last"),
                ("end", "sidebar-select-last"),
            ])
        );
        assert_eq!(
            audited(ChromeProfile::DesktopApple, SIDEBAR_TABLE),
            audited(ChromeProfile::Desktop, SIDEBAR_TABLE),
        );
        assert_eq!(
            audited(ChromeProfile::DesktopApple, TERMINAL_TABLE),
            audited(ChromeProfile::Desktop, TERMINAL_TABLE),
        );
    }

    #[gpui::test]
    fn a_reload_republishes_the_keymap_and_shadows_what_it_dropped(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let default_hotkey = crate::config::DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY;
            install(&[], default_hotkey, cx);
            assert!(
                chords(cx, SIDEBAR_TABLE)
                    .iter()
                    .any(|chord| chord.source == "q" && chord.live)
            );

            install(
                &[
                    ChromeOverride::Unbind {
                        table: SIDEBAR_TABLE,
                        key: "q".to_owned(),
                    },
                    ChromeOverride::Bind {
                        table: SIDEBAR_TABLE,
                        key: "C-S-p".to_owned(),
                        action: "sidebar-command-palette".to_owned(),
                    },
                ],
                "ctrl-alt-e",
                cx,
            );

            let sidebar = chords(cx, SIDEBAR_TABLE);
            let dropped = sidebar
                .iter()
                .find(|chord| chord.source == "q")
                .expect("the dropped chord comes back to be shadowed");
            assert!(!dropped.live);
            assert_eq!(dropped.action, ChromeAction::SidebarCancel);
            assert!(
                sidebar
                    .iter()
                    .any(|chord| chord.source == "ctrl-shift-p" && chord.live)
            );

            let selectors = chords(cx, BROWSER_TABLE)
                .into_iter()
                .filter(|chord| chord.action == ChromeAction::BrowserElementSelector && chord.live)
                .map(|chord| chord.source)
                .collect::<Vec<_>>();
            assert_eq!(selectors, ["ctrl-alt-e"]);
            assert_eq!(
                action_for(cx, BROWSER_TABLE, "C-M-e"),
                Some(ChromeAction::BrowserElementSelector)
            );
            assert_eq!(action_for(cx, BROWSER_TABLE, default_hotkey), None);
        });
    }

    #[test]
    fn the_settings_hint_prints_the_chord_the_keymap_binds() {
        assert_eq!(
            audited(ChromeProfile::DESKTOP, UI_TABLE)
                .into_iter()
                .find(|(_, action)| *action == "open-settings")
                .map(|(source, _)| source)
                .as_deref(),
            Some(crate::config::settings::KEYBIND),
        );
    }

    #[test]
    fn chords_bridge_to_the_gpui_spelling_they_replaced() {
        for (chrome, gpui) in [
            ("D-=", "cmd-="),
            ("D-M-Right", "cmd-alt-right"),
            ("D-S-[", "cmd-shift-["),
            ("C-S-Tab", "ctrl-shift-tab"),
            ("C-NPage", "ctrl-pagedown"),
            ("C-PPage", "ctrl-pageup"),
            ("M-Left", "alt-left"),
            ("F5", "f5"),
            ("G", "shift-g"),
            ("Escape", "escape"),
            (":", ":"),
            ("C-,", "ctrl-,"),
            ("C- ", "ctrl-space"),
        ] {
            let key = ChromeKey::parse(chrome).expect("valid chrome chord");
            assert_eq!(gpui_source(&key).as_deref(), Some(gpui), "{chrome}");
        }
    }

    #[test]
    fn a_gpui_hotkey_round_trips_through_the_chrome_spelling() {
        for hotkey in ["cmd-shift-c", "ctrl-shift-c", "shift-alt-e", "ctrl-f5"] {
            let keystroke = Keystroke::parse(hotkey).expect("valid hotkey");
            let key = chrome_key_for_keystroke(&keystroke).expect("bindable hotkey");
            let source = gpui_source(&key).expect("bindable chord");
            assert_eq!(
                Keystroke::parse(&source).expect("valid gpui source"),
                keystroke,
                "{hotkey}",
            );
        }
        assert_eq!(
            chrome_key_for_keystroke(&Keystroke::parse("fn-f5").expect("valid hotkey")),
            None,
        );
    }

    #[test]
    fn overrides_report_what_the_configuration_got_wrong() {
        assert_eq!(
            parse_bind("browser:D-t=browser-new-tab"),
            Ok(ChromeOverride::Bind {
                table: BROWSER_TABLE,
                key: "D-t".to_owned(),
                action: "browser-new-tab".to_owned(),
            })
        );
        assert_eq!(
            parse_bind("browser:D-==browser-zoom-in"),
            Ok(ChromeOverride::Bind {
                table: BROWSER_TABLE,
                key: "D-=".to_owned(),
                action: "browser-zoom-in".to_owned(),
            })
        );
        assert_eq!(
            parse_bind("ui:Ctrl-Shift-p=ui-zoom-in"),
            Ok(ChromeOverride::Bind {
                table: "ui",
                key: "C-S-p".to_owned(),
                action: "ui-zoom-in".to_owned(),
            })
        );
        assert_eq!(
            parse_unbind("sidebar:q"),
            Ok(ChromeOverride::Unbind {
                table: "sidebar",
                key: "q".to_owned(),
            })
        );

        assert!(parse_bind("browser:D-t").is_err());
        assert!(parse_bind("browser=browser-new-tab").is_err());
        assert!(parse_bind("prefix:D-t=browser-new-tab").is_err());
        assert!(parse_bind("browser:D-t=fly-me-to-the-moon").is_err());
        assert!(parse_bind("browser::=browser-new-tab").is_ok());
        assert!(parse_unbind("browser:").is_err());
    }
}
