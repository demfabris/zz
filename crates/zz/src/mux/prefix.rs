//! Client-side claim of the configured multiplexer prefix.

use std::collections::HashSet;

use gpui::Keystroke;
use zz_client::ChromeKey;
use zz_protocol::{Binding, KeyBindingSnapshot, KeyTables, TmuxOption};
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers as TerminalModifiers};

/// Whether a GPUI keystroke spells the given canonical tmux key.
#[cfg(test)]
pub(crate) fn keystroke_is(keystroke: &Keystroke, canonical: &str) -> bool {
    !keystroke.modifiers.function
        && !canonical.is_empty()
        && zz_protocol::input_key_name(&terminal_key_input(keystroke, KeyAction::Press)).as_str()
            == canonical
}

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

/// What to do with a claimed key press.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PressDisposition {
    /// A fresh physical press: forward it to the daemon. `stale` flags a key
    /// still marked held from a press whose release never arrived.
    Forward { stale: bool },
    /// An OS autorepeat of a held key: swallow it so holding the prefix
    /// cannot spam `send-prefix`.
    Autorepeat,
}

/// Held-key bookkeeping for claimed presses. Presses and releases pair by
/// physical key name only, never modifiers, and the held set never gates
/// whether a press is forwarded: a lost keyUp would strand an entry.
#[derive(Debug, Default)]
pub(crate) struct PrefixClaim {
    held: HashSet<String>,
    local_releases: HashSet<String>,
}

impl PrefixClaim {
    /// Record a claimed press and decide its fate. Autorepeats are never
    /// recorded, so a key held across arming keeps its release.
    pub(crate) fn press(&mut self, keystroke: &Keystroke, is_held: bool) -> PressDisposition {
        if is_held {
            return PressDisposition::Autorepeat;
        }
        self.local_releases.remove(&keystroke.key);
        let stale = !self.held.insert(keystroke.key.clone());
        PressDisposition::Forward { stale }
    }

    pub(crate) fn suppress_release(&mut self, keystroke: &Keystroke) {
        self.local_releases.insert(keystroke.key.clone());
    }

    pub(crate) fn consume_local_release(&mut self, keystroke: &Keystroke) -> bool {
        if !self.local_releases.remove(&keystroke.key) {
            return false;
        }
        self.held.remove(&keystroke.key);
        true
    }

    /// Whether this release pairs with a claimed press and must be swallowed.
    pub(crate) fn consume_release(&mut self, keystroke: &Keystroke) -> bool {
        self.held.remove(&keystroke.key)
    }

    /// Drop held-key state when the window loses focus.
    pub(crate) fn clear(&mut self) {
        self.held.clear();
        self.local_releases.clear();
    }
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

#[cfg(test)]
mod tests {
    use gpui::Modifiers;

    use super::*;

    #[test]
    fn local_shortcut_releases_never_reach_the_daemon_or_swallow_a_later_press() {
        let stroke = keystroke("s", Modifiers::default());
        let mut claim = PrefixClaim::default();
        claim.press(&stroke, false);
        claim.suppress_release(&stroke);
        assert!(claim.consume_local_release(&stroke));
        assert!(!claim.consume_release(&stroke));
        claim.press(&stroke, false);
        claim.suppress_release(&stroke);
        claim.press(&stroke, false);
        assert!(!claim.consume_local_release(&stroke));
        assert!(claim.consume_release(&stroke));
        claim.press(&stroke, false);
        claim.suppress_release(&stroke);
        claim.clear();
        assert!(!claim.consume_local_release(&stroke));
        assert!(!claim.consume_release(&stroke));
    }

    #[test]
    fn sidebar_picker_input_follows_default_and_rebound_picker_commands() {
        let tables = KeyTables::default().snapshot();
        let bindings = &tables
            .iter()
            .find(|table| table.name == "prefix")
            .unwrap()
            .bindings;
        for key in ["s", "w"] {
            let input = terminal_key_input(&keystroke(key, Modifiers::default()), KeyAction::Press);
            assert!(is_sidebar_picker_input(bindings, &input));
        }
        let mut binding = bindings
            .iter()
            .find(|binding| binding.key == "s")
            .unwrap()
            .clone();
        binding.key = "?".to_owned();
        let mut stroke = keystroke(
            "/",
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );
        stroke.key_char = Some("?".to_owned());
        let input = terminal_key_input(&stroke, KeyAction::Press);
        assert!(is_sidebar_picker_input(&[binding.clone()], &input));
        binding.commands = vec![zz_protocol::CommandInvocation::new(
            "choose-tree",
            ["-Z", "-w"],
        )];
        assert!(is_sidebar_picker_input(&[binding], &input));
    }

    #[test]
    fn sidebar_picker_input_preserves_custom_commands_filters_and_templates() {
        let input = terminal_key_input(&keystroke("s", Modifiers::default()), KeyAction::Press);
        for (name, args) in [
            ("resize-pane", vec!["-Z"]),
            ("choose-tree", vec![]),
            ("choose-tree", vec!["-Zsw"]),
            ("choose-tree", vec!["-Zs", "-f", "#{session_attached}"]),
            ("choose-tree", vec!["-Zw", "select-window -t %%"]),
            ("choose-tree", vec!["-Zw", "-t", "%1"]),
        ] {
            let binding = KeyBindingSnapshot {
                key: "s".to_owned(),
                commands: vec![zz_protocol::CommandInvocation::new(name, args)],
                repeat: false,
                note: None,
            };
            assert!(!is_sidebar_picker_input(&[binding], &input), "{name}");
        }
        let binding = KeyBindingSnapshot {
            key: "s".to_owned(),
            commands: vec![
                zz_protocol::CommandInvocation::new("choose-tree", ["-Zs"]),
                zz_protocol::CommandInvocation::new("display-message", ["custom"]),
            ],
            repeat: false,
            note: None,
        };
        assert!(!is_sidebar_picker_input(&[binding], &input));
    }

    fn keystroke(key: &str, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_owned(),
            key_char: None,
        }
    }

    #[test]
    fn keystrokes_match_their_canonical_spelling() {
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert!(keystroke_is(&keystroke("a", ctrl), "C-a"));
        assert!(keystroke_is(&keystroke("space", ctrl), "C- "));
        assert!(keystroke_is(&keystroke("space", Modifiers::default()), " "));
        assert!(keystroke_is(&keystroke("`", Modifiers::default()), "`"));
        assert!(keystroke_is(&keystroke("up", ctrl), "C-Up"));
        assert!(!keystroke_is(&keystroke("a", Modifiers::default()), "C-a"));
        assert!(!keystroke_is(&keystroke("b", ctrl), "C-a"));
    }

    #[test]
    fn shifted_special_prefixes_use_the_same_fold_as_daemon_input() {
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        for (key, canonical) in [("left", "S-Left"), ("tab", "BTab"), ("f1", "S-F1")] {
            assert!(keystroke_is(&keystroke(key, shift), canonical));
            assert!(!keystroke_is(
                &keystroke(key, Modifiers::default()),
                canonical
            ));
        }
        assert!(!keystroke_is(&keystroke("left", shift), "Left"));
    }

    #[test]
    fn the_displayed_prefix_is_the_keystroke_that_arms_it() {
        for canonical in ["C-b", "C- ", "M-Right", "G", "`"] {
            let displayed = display_keystroke(canonical)
                .unwrap_or_else(|| panic!("`{canonical}` has a keystroke"));
            assert!(keystroke_is(&displayed, canonical), "{canonical}");
        }
        assert!(display_keystroke("").is_none());
    }

    #[test]
    fn platform_chords_never_match() {
        let cmd_ctrl = Modifiers {
            control: true,
            platform: true,
            ..Modifiers::default()
        };
        assert!(!keystroke_is(&keystroke("a", cmd_ctrl), "C-a"));
    }

    #[test]
    fn shifted_letters_require_shift() {
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert!(keystroke_is(&keystroke("g", shift), "G"));
        assert!(!keystroke_is(&keystroke("g", Modifiers::default()), "G"));
    }

    #[test]
    fn autorepeats_are_swallowed_and_releases_pair_with_presses() {
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        let mut claim = PrefixClaim::default();
        let press = keystroke("a", ctrl);
        assert_eq!(
            claim.press(&press, false),
            PressDisposition::Forward { stale: false }
        );
        assert_eq!(claim.press(&press, true), PressDisposition::Autorepeat);
        assert!(claim.consume_release(&press));
        assert!(!claim.consume_release(&press));
    }

    #[test]
    fn a_release_with_lifted_modifiers_still_pairs_with_its_press() {
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        let mut claim = PrefixClaim::default();
        assert_eq!(
            claim.press(&keystroke("a", ctrl), false),
            PressDisposition::Forward { stale: false }
        );
        assert!(claim.consume_release(&keystroke("a", Modifiers::default())));
        assert_eq!(
            claim.press(&keystroke("a", ctrl), false),
            PressDisposition::Forward { stale: false }
        );
    }

    #[test]
    fn a_lost_release_cannot_eat_the_next_press() {
        let mut claim = PrefixClaim::default();
        let j = keystroke("j", Modifiers::default());
        assert_eq!(
            claim.press(&j, false),
            PressDisposition::Forward { stale: false }
        );
        assert_eq!(
            claim.press(&j, false),
            PressDisposition::Forward { stale: true }
        );
        assert!(claim.consume_release(&j));
        assert_eq!(
            claim.press(&j, false),
            PressDisposition::Forward { stale: false }
        );
    }

    #[test]
    fn a_key_held_across_arming_keeps_its_release() {
        let mut claim = PrefixClaim::default();
        let j = keystroke("j", Modifiers::default());
        assert_eq!(claim.press(&j, true), PressDisposition::Autorepeat);
        assert!(!claim.consume_release(&j));
    }

    #[test]
    fn simulated_lossy_keyboard_never_eats_a_fresh_press() {
        const KEYS: [&str; 3] = ["a", "j", "k"];

        #[derive(Default, Clone, Copy)]
        struct KeyModel {
            down: bool,
            claimed: bool,
            stranded: bool,
        }

        let mut rng: u64 = 0x5eed;
        let mut random = move |bound: u64| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng % bound
        };

        let mut claim = PrefixClaim::default();
        let mut model = [KeyModel::default(); KEYS.len()];

        for step in 0..20_000 {
            let index = random(KEYS.len() as u64) as usize;
            let modifiers = if random(2) == 0 {
                Modifiers::default()
            } else {
                Modifiers {
                    control: true,
                    ..Modifiers::default()
                }
            };
            let stroke = keystroke(KEYS[index], modifiers);
            match random(5) {
                0 => {
                    if model[index].down {
                        continue;
                    }
                    model[index].down = true;
                    let claimed = random(2) == 0;
                    model[index].claimed = claimed;
                    if claimed {
                        let disposition = claim.press(&stroke, false);
                        assert_eq!(
                            disposition,
                            PressDisposition::Forward {
                                stale: model[index].stranded
                            },
                            "step {step}: fresh press of {} mishandled",
                            KEYS[index]
                        );
                        model[index].stranded = false;
                    }
                }
                1 => {
                    if !model[index].down {
                        continue;
                    }
                    if random(2) == 0 {
                        assert_eq!(
                            claim.press(&stroke, true),
                            PressDisposition::Autorepeat,
                            "step {step}: repeat of {} forwarded",
                            KEYS[index]
                        );
                    }
                }
                2 => {
                    if !model[index].down {
                        continue;
                    }
                    model[index].down = false;
                    let expected = model[index].claimed || model[index].stranded;
                    assert_eq!(
                        claim.consume_release(&stroke),
                        expected,
                        "step {step}: release of {} mispaired",
                        KEYS[index]
                    );
                    model[index].claimed = false;
                    model[index].stranded = false;
                }
                3 => {
                    if !model[index].down {
                        continue;
                    }
                    model[index].down = false;
                    if model[index].claimed {
                        model[index].stranded = true;
                    }
                    model[index].claimed = false;
                }
                _ => {
                    claim.clear();
                    for state in &mut model {
                        state.down = false;
                        state.claimed = false;
                        state.stranded = false;
                    }
                }
            }
        }
    }
}
