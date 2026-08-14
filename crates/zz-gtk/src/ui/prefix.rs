//! Client-side claim of the daemon's configured prefix chord.
//!
//! GTK dispatches a key from the toplevel down to the focused widget, so a
//! capture-phase controller on the window sees every press before any widget
//! does. That ordering is the whole feature: a `GtkEntry`, a search field or a
//! list with type-ahead can never swallow the prefix, nor the key that follows
//! it while the daemon holds the prefix armed.

use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};

use gtk::{gdk, glib, prelude::*};
use zz_client::ChromeKey;
use zz_protocol::{CommandInvocation, input_key_name, input_typed_text};
use zz_terminal::{KeyAction, KeyInput, Modifiers};

use crate::{engine::Engine, ui::keys};

/// `zz-client`'s chrome catalog has no debug-marker action yet, so the chord
/// lives here as data instead of as an inline chord test. Move it into
/// `crates/zz-client/src/chrome.rs` the moment that catalog grows one.
const DEBUG_MARK_CHORD: &str = "C-S-m";

/// What to do with a claimed press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressDisposition {
    /// A fresh physical press: forward it to the daemon.
    Forward,
    /// A press for a key that is already down. GDK publishes no repeat flag, so
    /// a second press without an intervening release is the only signal an OS
    /// autorepeat gives; swallowing it stops a held prefix from spamming
    /// `send-prefix`.
    Autorepeat,
}

/// Held-key bookkeeping for claimed presses. Presses and releases pair by
/// hardware keycode, never by keyval or modifiers: a release delivered after
/// the user lifted Control still carries the keycode its press did, and lifting
/// Shift mid-chord changes the keyval but not the key.
#[derive(Debug, Default)]
pub struct PrefixClaim {
    held: HashSet<u32>,
}

impl PrefixClaim {
    pub fn press(&mut self, keycode: u32) -> PressDisposition {
        if self.held.insert(keycode) {
            PressDisposition::Forward
        } else {
            PressDisposition::Autorepeat
        }
    }

    /// Whether this release pairs with a claimed press and must be forwarded
    /// and swallowed.
    pub fn consume_release(&mut self, keycode: u32) -> bool {
        self.held.remove(&keycode)
    }

    /// Drop held-key state. A release is only ever lost when something takes
    /// the keyboard away — a grab, a popup, another window — and every one of
    /// those reaches the window as a focus-out, so this is what keeps an
    /// inferred autorepeat from stranding a key forever.
    pub fn clear(&mut self) {
        self.held.clear();
    }

    pub fn is_idle(&self) -> bool {
        self.held.is_empty()
    }
}

/// Install the interceptor on the window.
///
/// Ordering, from first to last: this capture controller, then the shell's own
/// capture controllers (display-panes numbering), then the focused widget's
/// input method and pane handler. Nothing below this point ever sees a key the
/// prefix claimed.
pub fn install(window: &impl IsA<gtk::Widget>, engine: Arc<Engine>) {
    let interceptor = Rc::new(Interceptor {
        engine,
        claim: RefCell::new(PrefixClaim::default()),
        debug_mark: ChromeKey::parse(DEBUG_MARK_CHORD),
        marker: RefCell::new(0),
    });

    // `EventControllerKey` cannot stop a release from reaching the widget below,
    // and an unbalanced release is exactly what breaks kitty-keyboard pairing,
    // so the claim rides the legacy controller that still owns propagation for
    // both halves of a key.
    let controller = gtk::EventControllerLegacy::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let target = Rc::clone(&interceptor);
    controller.connect_event(move |_, event| target.handle(event));
    window.as_ref().add_controller(controller);

    let focus = gtk::EventControllerFocus::new();
    let target = Rc::clone(&interceptor);
    focus.connect_leave(move |_| target.claim.borrow_mut().clear());
    window.as_ref().add_controller(focus);
}

struct Interceptor {
    engine: Arc<Engine>,
    claim: RefCell<PrefixClaim>,
    debug_mark: Option<ChromeKey>,
    marker: RefCell<u64>,
}

impl Interceptor {
    fn handle(&self, event: &gdk::Event) -> glib::Propagation {
        let Some(key) = event.downcast_ref::<gdk::KeyEvent>() else {
            return glib::Propagation::Proceed;
        };
        match event.event_type() {
            gdk::EventType::KeyPress => self.press(key, event.modifier_state()),
            gdk::EventType::KeyRelease => self.release(key, event.modifier_state()),
            _ => glib::Propagation::Proceed,
        }
    }

    fn press(&self, key: &gdk::KeyEvent, state: gdk::ModifierType) -> glib::Propagation {
        let keyval = key.keyval();
        if keys::is_modifier(keyval) {
            return glib::Propagation::Proceed;
        }
        let input = keys::key_input(KeyAction::Press, keyval, state, None);
        if self.is_debug_mark(&input) {
            self.debug_mark();
            return glib::Propagation::Stop;
        }
        if !self.claims(&input) {
            return glib::Propagation::Proceed;
        }
        let Some(pane) = self.engine.active_pane() else {
            log::warn!("zz-gtk dropped a prefix key: the session has no active pane");
            return glib::Propagation::Proceed;
        };
        match self.claim.borrow_mut().press(key.keycode()) {
            PressDisposition::Autorepeat => {}
            PressDisposition::Forward => self.engine.send_key(pane, input, false),
        }
        glib::Propagation::Stop
    }

    /// A release is forwarded only when its press was claimed, so the daemon
    /// sees the same press/release pairs a pane-focused key would have made.
    fn release(&self, key: &gdk::KeyEvent, state: gdk::ModifierType) -> glib::Propagation {
        if !self.claim.borrow_mut().consume_release(key.keycode()) {
            return glib::Propagation::Proceed;
        }
        if let Some(pane) = self.engine.active_pane() {
            let input = keys::key_input(KeyAction::Release, key.keyval(), state, None);
            self.engine.send_key(pane, input, false);
        }
        glib::Propagation::Stop
    }

    /// The claim covers exactly the prefix chord, and then every key until the
    /// daemon disarms. Super chords are left alone: the wire grammar cannot
    /// spell them, so no pane binding can ever want one.
    /// Held keys deliberately do not widen the claim: a release pairs with its
    /// press through the held set on its own, and letting a stranded entry
    /// claim presses would send every later key to the pane.
    fn claims(&self, input: &KeyInput) -> bool {
        if input.modifiers.platform() {
            return false;
        }
        if self.engine.prefix_armed() {
            return true;
        }
        // `display-panes` owns the keyboard while its numbers are up, and the
        // shell forwards those presses from its own capture controller. Yielding
        // here makes the two controllers order-independent.
        if self.engine.display_panes().is_some() {
            return false;
        }
        self.engine
            .prefix_chord()
            .is_some_and(|chord| spells(input, &chord))
    }

    fn is_debug_mark(&self, input: &KeyInput) -> bool {
        self.debug_mark
            .as_ref()
            .is_some_and(|chord| chrome_key(input) == *chord)
    }

    fn debug_mark(&self) {
        let seq = {
            let mut marker = self.marker.borrow_mut();
            *marker += 1;
            *marker
        };
        log::info!(target: "zz_gtk::marker", "user_marker seq={seq}");
        self.engine.execute(CommandInvocation::new(
            "debug-marker",
            [format!("seq={seq}")],
        ));
        self.engine.notify(format!("log marker #{seq}"));
    }
}

/// Whether a press spells a tmux chord, under the daemon's own precedence: the
/// typed character first, the folded key name second. Shift+`/` types `?` while
/// folding to `/`, and a `?` prefix has to win.
fn spells(input: &KeyInput, chord: &str) -> bool {
    if let Some(text) = input_typed_text(input)
        && text.chars().count() == 1
        && text == chord
    {
        return true;
    }
    input_key_name(input).as_str() == chord
}

/// The chrome spelling of a press. Chrome extends the wire grammar with `S-`
/// and `D-`, so the base name is folded with the modifiers stripped and the
/// modifiers are carried separately.
fn chrome_key(input: &KeyInput) -> ChromeKey {
    let bare = KeyInput {
        modifiers: Modifiers::new(false, false, false, false),
        ..input.clone()
    };
    ChromeKey {
        command: input.modifiers.platform(),
        control: input.modifiers.control(),
        alt: input.modifiers.alt(),
        shift: input.modifiers.shift(),
        base: input_key_name(&bare).as_str().to_owned(),
    }
    .normalized()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_terminal::KeyCode;

    fn input(key: KeyCode, modifiers: Modifiers, text: Option<&str>) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key,
            modifiers,
            text: text.map(|text| text.to_owned().into_boxed_str()),
            unshifted_codepoint: match key {
                KeyCode::Character(character) => Some(character),
                _ => None,
            },
        }
    }

    const NONE: Modifiers = Modifiers::new(false, false, false, false);
    const CONTROL: Modifiers = Modifiers::new(false, true, false, false);
    const SHIFT: Modifiers = Modifiers::new(true, false, false, false);
    const CONTROL_SHIFT: Modifiers = Modifiers::new(true, true, false, false);
    const SUPER: Modifiers = Modifiers::new(false, false, false, true);

    #[test]
    fn every_prefix_spelling_a_user_can_configure_is_recognized() {
        let cases: &[(&str, KeyInput, bool)] = &[
            ("C-b", input(KeyCode::Character('b'), CONTROL, None), true),
            ("C-b", input(KeyCode::Character('a'), CONTROL, None), false),
            (
                "C-b",
                input(KeyCode::Character('b'), NONE, Some("b")),
                false,
            ),
            ("C- ", input(KeyCode::Character(' '), CONTROL, None), true),
            ("`", input(KeyCode::Character('`'), NONE, Some("`")), true),
            ("G", input(KeyCode::Character('g'), SHIFT, Some("G")), true),
            ("G", input(KeyCode::Character('g'), NONE, Some("g")), false),
            ("C-Up", input(KeyCode::ArrowUp, CONTROL, None), true),
            ("?", input(KeyCode::Character('/'), SHIFT, Some("?")), true),
        ];

        for (chord, press, expected) in cases {
            assert_eq!(spells(press, chord), *expected, "{chord} vs {press:?}");
        }
    }

    /// The wire fold empties a Super chord's name, so nothing can spell it and
    /// the claim must not try.
    #[test]
    fn a_super_chord_never_spells_a_prefix() {
        let press = input(KeyCode::Character('b'), SUPER, None);

        assert!(!spells(&press, "C-b"));
        assert!(!spells(&press, "b"));
    }

    #[test]
    fn the_debug_mark_chord_is_read_out_of_the_chrome_grammar() {
        let parsed = ChromeKey::parse(DEBUG_MARK_CHORD).expect("a parseable chord");

        assert_eq!(
            chrome_key(&input(KeyCode::Character('m'), CONTROL_SHIFT, None)),
            parsed
        );
        assert_ne!(
            chrome_key(&input(KeyCode::Character('m'), CONTROL, None)),
            parsed
        );
        assert_ne!(
            chrome_key(&input(KeyCode::Character('n'), CONTROL_SHIFT, None)),
            parsed
        );
    }

    #[test]
    fn autorepeats_are_swallowed_and_releases_pair_with_presses() {
        let mut claim = PrefixClaim::default();

        assert_eq!(claim.press(56), PressDisposition::Forward);
        assert_eq!(claim.press(56), PressDisposition::Autorepeat);
        assert_eq!(claim.press(56), PressDisposition::Autorepeat);
        assert!(claim.consume_release(56));
        assert!(!claim.consume_release(56));
        assert_eq!(claim.press(56), PressDisposition::Forward);
    }

    /// Two keys held at once keep separate bookkeeping, and a release only
    /// answers for the key it names.
    #[test]
    fn held_keys_do_not_shadow_each_other() {
        let mut claim = PrefixClaim::default();

        assert_eq!(claim.press(56), PressDisposition::Forward);
        assert_eq!(claim.press(44), PressDisposition::Forward);
        assert!(!claim.is_idle());
        assert!(claim.consume_release(56));
        assert!(!claim.is_idle());
        assert!(claim.consume_release(44));
        assert!(claim.is_idle());
    }

    /// A release swallowed by a grab would otherwise leave the key forever
    /// mistaken for an autorepeat; focus-out is the reset.
    #[test]
    fn losing_focus_releases_every_claimed_key() {
        let mut claim = PrefixClaim::default();

        assert_eq!(claim.press(56), PressDisposition::Forward);
        claim.clear();
        assert!(claim.is_idle());
        assert!(!claim.consume_release(56));
        assert_eq!(claim.press(56), PressDisposition::Forward);
    }

    /// The claim outlives the arming: a key pressed while armed keeps its
    /// release even after the daemon disarms, so the pairing stays balanced.
    #[test]
    fn a_claimed_press_keeps_its_release_after_disarming() {
        let mut claim = PrefixClaim::default();

        assert_eq!(claim.press(44), PressDisposition::Forward);
        assert!(!claim.is_idle());
        assert!(claim.consume_release(44));
    }

    /// A random keyboard, replayed against a model of which keys are physically
    /// down: no fresh press is ever mistaken for a repeat, and no release is
    /// ever forwarded without a claimed press behind it.
    #[test]
    fn a_simulated_keyboard_never_mispairs() {
        const KEYS: [u32; 4] = [38, 44, 56, 65];

        let mut rng: u64 = 0x5eed;
        let mut random = move |bound: u64| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng % bound
        };

        let mut claim = PrefixClaim::default();
        let mut down = [false; KEYS.len()];

        for step in 0..20_000 {
            let index = random(KEYS.len() as u64) as usize;
            let keycode = KEYS[index];
            match random(4) {
                0 => {
                    let expected = if down[index] {
                        PressDisposition::Autorepeat
                    } else {
                        PressDisposition::Forward
                    };
                    assert_eq!(claim.press(keycode), expected, "step {step}: press");
                    down[index] = true;
                }
                1 => {
                    assert_eq!(
                        claim.consume_release(keycode),
                        down[index],
                        "step {step}: release"
                    );
                    down[index] = false;
                }
                2 => {
                    assert_eq!(claim.is_idle(), !down.iter().any(|held| *held));
                }
                _ => {
                    claim.clear();
                    down = [false; KEYS.len()];
                }
            }
        }
    }
}
