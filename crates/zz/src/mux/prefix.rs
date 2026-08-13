//! Client-side claim of the configured multiplexer prefix.

use std::collections::HashSet;

use gpui::Keystroke;
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers as TerminalModifiers};

/// Fold the daemon-published prefix spelling into the form a keystroke is
/// compared against: `Ctrl-`/`Alt-` become `C-`/`M-`, `Space` becomes a literal
/// space, mirroring the daemon's `canonical_key`.
pub(crate) fn canonical_prefix(value: &str) -> String {
    let trimmed = value.trim();
    if value == " " || trimmed == "Space" {
        return " ".to_owned();
    }
    let mut modifiers = String::new();
    let mut rest = trimmed;
    loop {
        if let Some(tail) = rest
            .strip_prefix("Ctrl-")
            .or_else(|| rest.strip_prefix("C-"))
        {
            modifiers.push_str("C-");
            rest = tail;
        } else if let Some(tail) = rest
            .strip_prefix("Alt-")
            .or_else(|| rest.strip_prefix("M-"))
        {
            modifiers.push_str("M-");
            rest = tail;
        } else {
            break;
        }
    }
    if modifiers.is_empty() {
        return trimmed.to_owned();
    }
    if rest == "Space" {
        rest = " ";
    }
    format!("{modifiers}{rest}")
}

/// Whether a GPUI keystroke spells the given canonical tmux key.
pub(crate) fn keystroke_is(keystroke: &Keystroke, canonical: &str) -> bool {
    let mut want_control = false;
    let mut want_alt = false;
    let mut rest = canonical;
    loop {
        if let Some(tail) = rest.strip_prefix("C-") {
            want_control = true;
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("M-") {
            want_alt = true;
            rest = tail;
        } else {
            break;
        }
    }
    let modifiers = keystroke.modifiers;
    if modifiers.control != want_control
        || modifiers.alt != want_alt
        || modifiers.platform
        || modifiers.function
    {
        return false;
    }
    if rest == " " {
        return keystroke.key == "space";
    }
    if let Some(gpui_name) = gpui_key_name(rest) {
        return keystroke.key == gpui_name;
    }
    let mut characters = rest.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) if character.is_ascii_uppercase() => {
            modifiers.shift && keystroke.key == character.to_ascii_lowercase().to_string()
        }
        (Some(character), None) => keystroke.key == character.to_string(),
        _ => false,
    }
}

/// The canonical prefix as a GPUI keystroke, so a hint can print it in the
/// platform's own glyphs (`⌃B`, `Ctrl+B`) rather than tmux's `C-b` spelling.
pub(crate) fn display_keystroke(canonical: &str) -> Option<Keystroke> {
    let mut spelling = String::new();
    let mut rest = canonical;
    loop {
        if let Some(tail) = rest.strip_prefix("C-") {
            spelling.push_str("ctrl-");
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("M-") {
            spelling.push_str("alt-");
            rest = tail;
        } else {
            break;
        }
    }
    let key = match rest {
        " " => "space",
        named => gpui_key_name(named).unwrap_or(named),
    };
    let mut characters = key.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) if character.is_ascii_uppercase() => {
            spelling.push_str("shift-");
            spelling.extend(character.to_lowercase());
        }
        (Some(_), _) => spelling.push_str(key),
        (None, _) => return None,
    }
    Keystroke::parse(&spelling).ok()
}

fn gpui_key_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "Enter" => "enter",
        "Escape" => "escape",
        "Tab" => "tab",
        "BSpace" => "backspace",
        "Up" => "up",
        "Down" => "down",
        "Left" => "left",
        "Right" => "right",
        "Home" => "home",
        "End" => "end",
        "PPage" => "pageup",
        "NPage" => "pagedown",
        "DC" => "delete",
        "IC" => "insert",
        "F1" => "f1",
        "F2" => "f2",
        "F3" => "f3",
        "F4" => "f4",
        "F5" => "f5",
        "F6" => "f6",
        "F7" => "f7",
        "F8" => "f8",
        "F9" => "f9",
        "F10" => "f10",
        "F11" => "f11",
        "F12" => "f12",
        _ => return None,
    })
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
}

impl PrefixClaim {
    /// Record a claimed press and decide its fate. Autorepeats are never
    /// recorded, so a key held across arming keeps its release.
    pub(crate) fn press(&mut self, keystroke: &Keystroke, is_held: bool) -> PressDisposition {
        if is_held {
            return PressDisposition::Autorepeat;
        }
        let stale = !self.held.insert(keystroke.key.clone());
        PressDisposition::Forward { stale }
    }

    /// Whether this release pairs with a claimed press and must be swallowed.
    pub(crate) fn consume_release(&mut self, keystroke: &Keystroke) -> bool {
        self.held.remove(&keystroke.key)
    }

    /// Drop held-key state when the window loses focus.
    pub(crate) fn clear(&mut self) {
        self.held.clear();
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

    fn keystroke(key: &str, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_owned(),
            key_char: None,
        }
    }

    #[test]
    fn published_prefix_spellings_fold_to_keystroke_form() {
        assert_eq!(canonical_prefix("C-b"), "C-b");
        assert_eq!(canonical_prefix("Ctrl-a"), "C-a");
        assert_eq!(canonical_prefix("C-Space"), "C- ");
        assert_eq!(canonical_prefix("Space"), " ");
        assert_eq!(canonical_prefix("M-Right"), "M-Right");
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
