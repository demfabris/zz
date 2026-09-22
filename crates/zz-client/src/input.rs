use zz_protocol::PaneId;
use zz_terminal::{KeyAction, KeyCode, KeyInput};

use crate::{BROWSER_TABLE, ChromeAction, ChromeKeymap, SIDEBAR_TABLE, TERMINAL_TABLE, UI_TABLE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Terminal,
    Browser,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputOwner {
    None,
    Pane(PaneId, SurfaceKind),
    Sidebar,
    Overlay,
    NativeEditor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    ActivatePane(PaneId, SurfaceKind),
    SetActivePane(PaneId, SurfaceKind),
    FocusSidebar,
    FocusNativeEditor,
    OverlayOpened,
    OverlayClosed,
    NativeFocusObserved(InputOwner),
    PaneRemoved(PaneId),
    Detached,
    WindowDeactivated,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrefixView {
    pub armed: bool,
    pub claimed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    ForwardKey { pane: PaneId, input: KeyInput },
    Chrome(ChromeAction),
    RequestFocus(InputOwner),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Consumed,
    Native,
}

#[derive(Debug, PartialEq, Eq)]
enum PressDisposition {
    Forward { stale: bool },
    Autorepeat,
}

#[derive(Debug, Default)]
struct PrefixClaim {
    held: Vec<(KeyCode, PaneId)>,
    local_releases: Vec<KeyCode>,
}

impl PrefixClaim {
    fn press(&mut self, key: KeyCode, pane: PaneId, is_held: bool) -> PressDisposition {
        if is_held {
            return PressDisposition::Autorepeat;
        }
        self.local_releases.retain(|held| *held != key);
        let stale = self.consume_release(key).is_some();
        self.held.push((key, pane));
        PressDisposition::Forward { stale }
    }

    fn suppress_release(&mut self, key: KeyCode) {
        if !self.local_releases.contains(&key) {
            self.local_releases.push(key);
        }
    }

    fn consume_local_release(&mut self, key: KeyCode) -> bool {
        let Some(index) = self.local_releases.iter().position(|held| *held == key) else {
            return false;
        };
        self.local_releases.swap_remove(index);
        self.consume_release(key);
        true
    }

    fn consume_release(&mut self, key: KeyCode) -> Option<PaneId> {
        let index = self.held.iter().position(|(held, _)| *held == key)?;
        Some(self.held.swap_remove(index).1)
    }

    fn remove_pane(&mut self, pane: PaneId) {
        self.local_releases.retain(|key| {
            !self
                .held
                .iter()
                .any(|(held, owner)| held == key && *owner == pane)
        });
        self.held.retain(|(_, owner)| *owner != pane);
    }

    fn clear(&mut self) {
        self.held.clear();
        self.local_releases.clear();
    }
}

#[derive(Debug)]
pub struct InputRouter {
    keymap: ChromeKeymap,
    owner: InputOwner,
    pending: Option<InputOwner>,
    observed: InputOwner,
    restore: InputOwner,
    active: Option<(PaneId, SurfaceKind)>,
    claim: PrefixClaim,
    effects: Vec<Effect>,
}

impl InputRouter {
    #[must_use]
    pub fn new(keymap: ChromeKeymap) -> Self {
        Self {
            keymap,
            owner: InputOwner::None,
            pending: None,
            observed: InputOwner::None,
            restore: InputOwner::None,
            active: None,
            claim: PrefixClaim::default(),
            effects: Vec::new(),
        }
    }

    pub fn unbind_action(&mut self, action: ChromeAction) -> usize {
        let bindings = self.keymap.bindings();
        bindings
            .into_iter()
            .filter(|(table, key, bound)| *bound == action && self.keymap.unbind(table, key))
            .count()
    }

    #[must_use]
    pub fn owner(&self) -> InputOwner {
        self.pending.unwrap_or(self.owner)
    }

    #[must_use]
    pub const fn active_pane(&self) -> Option<(PaneId, SurfaceKind)> {
        self.active
    }

    pub fn event(&mut self, event: InputEvent) {
        match event {
            InputEvent::ActivatePane(pane, kind) => {
                self.active = Some((pane, kind));
                let owner = InputOwner::Pane(pane, kind);
                if self.owner() == InputOwner::Overlay {
                    self.restore = owner;
                } else {
                    self.pending = Some(owner);
                    self.effects.push(Effect::RequestFocus(owner));
                }
            }
            InputEvent::SetActivePane(pane, kind) => self.active = Some((pane, kind)),
            InputEvent::FocusSidebar => self.focus(InputOwner::Sidebar),
            InputEvent::FocusNativeEditor => self.focus(InputOwner::NativeEditor),
            InputEvent::OverlayOpened => {
                if self.owner() != InputOwner::Overlay {
                    self.restore = self.owner();
                    self.pending = None;
                    self.owner = InputOwner::Overlay;
                }
            }
            InputEvent::OverlayClosed => {
                if self.owner() == InputOwner::Overlay {
                    self.owner = self.restore;
                    self.pending = None;
                    self.effects.push(Effect::RequestFocus(self.owner));
                }
            }
            InputEvent::NativeFocusObserved(owner) => {
                self.observed = owner;
                if self.pending.is_none() || self.pending == Some(owner) {
                    self.owner = owner;
                    self.pending = None;
                }
            }
            InputEvent::PaneRemoved(pane) => {
                for owner in [&mut self.owner, &mut self.restore, &mut self.observed] {
                    if matches!(*owner, InputOwner::Pane(id, _) if id == pane) {
                        *owner = InputOwner::None;
                    }
                }
                if matches!(self.pending, Some(InputOwner::Pane(id, _)) if id == pane) {
                    self.pending = None;
                    self.owner = InputOwner::None;
                }
                if self.active.is_some_and(|(id, _)| id == pane) {
                    self.active = None;
                }
                self.claim.remove_pane(pane);
            }
            InputEvent::Detached => {
                self.owner = InputOwner::None;
                self.pending = None;
                self.observed = InputOwner::None;
                self.restore = InputOwner::None;
                self.active = None;
                self.claim.clear();
            }
            InputEvent::WindowDeactivated => self.claim.clear(),
        }
    }

    fn focus(&mut self, owner: InputOwner) {
        let previous = self.owner();
        if previous == InputOwner::Overlay {
            self.restore = owner;
        } else {
            if matches!(previous, InputOwner::Pane(..)) {
                self.restore = previous;
            }
            self.pending = None;
            self.owner = owner;
            self.effects.push(Effect::RequestFocus(owner));
        }
    }

    pub fn key(&mut self, input: &KeyInput, prefix: PrefixView) -> Disposition {
        let key = input
            .unshifted_codepoint
            .map_or(input.key, KeyCode::Character);
        if input.action == KeyAction::Release {
            if self.claim.consume_local_release(key) {
                return Disposition::Consumed;
            }
            if let Some(pane) = self.claim.consume_release(key) {
                self.effects.push(Effect::ForwardKey {
                    pane,
                    input: input.clone(),
                });
                return Disposition::Consumed;
            }
            return Disposition::Native;
        }
        self.claim.local_releases.retain(|held| *held != key);
        if self.owner() == InputOwner::Overlay {
            return Disposition::Native;
        }
        if let Some(action) = self.keymap.resolve(UI_TABLE, input) {
            return self.chrome(key, action);
        }
        if prefix.claimed
            && let Some((pane, _)) = self.active
        {
            if matches!(
                self.claim
                    .press(key, pane, input.action == KeyAction::Repeat),
                PressDisposition::Forward { .. }
            ) {
                self.effects.push(Effect::ForwardKey {
                    pane,
                    input: input.clone(),
                });
            }
            return Disposition::Consumed;
        }
        let table = match self.owner() {
            InputOwner::Sidebar => Some(SIDEBAR_TABLE),
            InputOwner::Pane(_, SurfaceKind::Terminal) => Some(TERMINAL_TABLE),
            InputOwner::Pane(_, SurfaceKind::Browser) => Some(BROWSER_TABLE),
            _ => None,
        };
        if let Some(action) = table.and_then(|table| self.keymap.resolve(table, input)) {
            return self.chrome(key, action);
        }
        Disposition::Native
    }

    fn chrome(&mut self, key: KeyCode, action: ChromeAction) -> Disposition {
        self.claim.suppress_release(key);
        self.effects.push(Effect::Chrome(action));
        Disposition::Consumed
    }

    pub fn suppress_release(&mut self, key: KeyCode) {
        self.claim.suppress_release(key);
    }

    pub fn drain_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }
}

#[cfg(test)]
mod tests {
    use zz_terminal::Modifiers;

    use crate::ChromeProfile;

    use super::*;

    const P: PaneId = PaneId(1);
    const Q: PaneId = PaneId(2);
    const CLAIMED: PrefixView = PrefixView {
        armed: true,
        claimed: true,
    };

    fn press(key: KeyCode) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key,
            modifiers: Modifiers::default(),
            text: match key {
                KeyCode::Character(ch) => Some(ch.to_string().into_boxed_str()),
                _ => None,
            },
            unshifted_codepoint: None,
        }
    }

    fn router() -> InputRouter {
        InputRouter::new(ChromeKeymap::for_profile(ChromeProfile::DesktopApple))
    }

    fn activate(router: &mut InputRouter, pane: PaneId) {
        router.event(InputEvent::ActivatePane(pane, SurfaceKind::Terminal));
        assert_eq!(
            router.drain_effects(),
            vec![Effect::RequestFocus(InputOwner::Pane(
                pane,
                SurfaceKind::Terminal
            ))]
        );
    }

    fn forward(router: &mut InputRouter, input: &KeyInput, pane: PaneId) {
        assert_eq!(router.key(input, CLAIMED), Disposition::Consumed);
        assert_eq!(
            router.drain_effects(),
            vec![Effect::ForwardKey {
                pane,
                input: input.clone()
            }]
        );
    }

    #[test]
    fn pane_owner_keeps_return_and_printable_input_native() {
        let mut router = router();
        router.event(InputEvent::FocusSidebar);
        router.drain_effects();
        activate(&mut router, P);
        for key in [KeyCode::Enter, KeyCode::Character('x')] {
            assert_eq!(
                router.key(&press(key), PrefixView::default()),
                Disposition::Native
            );
            assert!(router.drain_effects().is_empty());
        }
    }

    #[test]
    fn sidebar_confirm_then_activation_routes_before_acknowledgment() {
        let mut router = router();
        router.event(InputEvent::FocusSidebar);
        assert_eq!(
            router.drain_effects(),
            vec![Effect::RequestFocus(InputOwner::Sidebar)]
        );
        assert_eq!(
            router.key(&press(KeyCode::Enter), PrefixView::default()),
            Disposition::Consumed
        );
        assert_eq!(
            router.drain_effects(),
            vec![Effect::Chrome(ChromeAction::SidebarConfirm)]
        );
        activate(&mut router, P);
        assert_eq!(router.owner(), InputOwner::Pane(P, SurfaceKind::Terminal));
        assert_eq!(
            router.key(&press(KeyCode::Enter), PrefixView::default()),
            Disposition::Native
        );
        assert!(router.drain_effects().is_empty());
    }

    #[test]
    fn pending_intent_wins_until_acknowledged_or_superseded() {
        let mut router = router();
        activate(&mut router, P);
        for _ in 0..3 {
            router.event(InputEvent::NativeFocusObserved(InputOwner::Sidebar));
            assert_eq!(router.owner(), InputOwner::Pane(P, SurfaceKind::Terminal));
            assert!(router.drain_effects().is_empty());
        }
        router.event(InputEvent::FocusSidebar);
        assert_eq!(router.owner(), InputOwner::Sidebar);
        assert_eq!(
            router.drain_effects(),
            vec![Effect::RequestFocus(InputOwner::Sidebar)]
        );
        activate(&mut router, Q);
        router.event(InputEvent::NativeFocusObserved(InputOwner::Pane(
            Q,
            SurfaceKind::Terminal,
        )));
        router.event(InputEvent::NativeFocusObserved(InputOwner::Sidebar));
        assert_eq!(router.owner(), InputOwner::Sidebar);
        activate(&mut router, P);
        router.event(InputEvent::FocusNativeEditor);
        assert_eq!(router.owner(), InputOwner::NativeEditor);
        assert_eq!(router.restore, InputOwner::Pane(P, SurfaceKind::Terminal));
        assert_eq!(
            router.drain_effects(),
            vec![Effect::RequestFocus(InputOwner::NativeEditor)]
        );
        assert_eq!(
            router.key(&press(KeyCode::Enter), PrefixView::default()),
            Disposition::Native
        );
    }

    #[test]
    fn passive_active_pane_changes_prefix_destination_without_focus() {
        let mut router = router();
        activate(&mut router, P);
        router.event(InputEvent::FocusSidebar);
        router.drain_effects();
        router.event(InputEvent::SetActivePane(Q, SurfaceKind::Browser));
        assert_eq!(router.owner(), InputOwner::Sidebar);
        assert_eq!(router.active_pane(), Some((Q, SurfaceKind::Browser)));
        assert!(router.drain_effects().is_empty());
        forward(&mut router, &press(KeyCode::Enter), Q);
    }

    #[test]
    fn prefix_repeats_and_releases_keep_the_press_pane() {
        let mut router = router();
        activate(&mut router, P);
        let mut input = press(KeyCode::Character('b'));
        input.modifiers = Modifiers::new(false, true, false, false);
        forward(&mut router, &input, P);
        input.action = KeyAction::Repeat;
        assert_eq!(router.key(&input, CLAIMED), Disposition::Consumed);
        assert!(router.drain_effects().is_empty());
        activate(&mut router, Q);
        input.action = KeyAction::Release;
        input.modifiers = Modifiers::default();
        assert_eq!(
            router.key(&input, PrefixView::default()),
            Disposition::Consumed
        );
        assert_eq!(
            router.drain_effects(),
            vec![Effect::ForwardKey {
                pane: P,
                input: input.clone()
            }]
        );
        assert_eq!(router.key(&input, CLAIMED), Disposition::Native);
        input.key = KeyCode::Character('z');
        assert_eq!(router.key(&input, CLAIMED), Disposition::Native);
        assert!(router.drain_effects().is_empty());
    }

    #[test]
    fn lost_release_does_not_block_fresh_press_and_deactivation_clears_it() {
        let mut router = router();
        activate(&mut router, P);
        let mut input = press(KeyCode::Character('j'));
        forward(&mut router, &input, P);
        forward(&mut router, &input, P);
        router.event(InputEvent::WindowDeactivated);
        input.action = KeyAction::Release;
        assert_eq!(router.key(&input, CLAIMED), Disposition::Native);
        assert!(router.drain_effects().is_empty());
        assert_eq!(router.owner(), InputOwner::Pane(P, SurfaceKind::Terminal));
    }

    #[test]
    fn ui_bindings_beat_prefix_and_browser_bindings() {
        let mut map = ChromeKeymap::for_profile(ChromeProfile::DesktopApple);
        map.bind(UI_TABLE, "C-b", ChromeAction::OpenCommandPalette.name())
            .unwrap();
        let mut router = InputRouter::new(map);
        router.event(InputEvent::ActivatePane(P, SurfaceKind::Browser));
        router.drain_effects();
        let mut input = press(KeyCode::Character('b'));
        input.modifiers = Modifiers::new(false, true, false, false);
        assert_eq!(router.key(&input, CLAIMED), Disposition::Consumed);
        assert_eq!(
            router.drain_effects(),
            vec![Effect::Chrome(ChromeAction::OpenCommandPalette)]
        );
        input.action = KeyAction::Release;
        assert_eq!(router.key(&input, CLAIMED), Disposition::Consumed);
        assert!(router.drain_effects().is_empty());
        input = press(KeyCode::Character('1'));
        input.modifiers = Modifiers::new(false, false, false, true);
        assert_eq!(
            router.keymap.resolve(BROWSER_TABLE, &input),
            Some(ChromeAction::BrowserSelectTab(0))
        );
        assert_eq!(
            router.key(&input, PrefixView::default()),
            Disposition::Consumed
        );
        assert_eq!(
            router.drain_effects(),
            vec![Effect::Chrome(ChromeAction::SelectWindow(0))]
        );
    }

    #[test]
    fn unsupported_actions_are_unbound_in_every_table() {
        let mut map = ChromeKeymap::new();
        for table in [UI_TABLE, TERMINAL_TABLE, BROWSER_TABLE, SIDEBAR_TABLE] {
            for chord in ["C-p", "C-o"] {
                map.bind(table, chord, ChromeAction::OpenCommandPalette.name())
                    .unwrap();
            }
        }
        let mut router = InputRouter::new(map);
        activate(&mut router, P);
        assert_eq!(router.unbind_action(ChromeAction::OpenCommandPalette), 8);
        assert_eq!(router.unbind_action(ChromeAction::OpenCommandPalette), 0);
        for ch in ['p', 'o'] {
            let mut input = press(KeyCode::Character(ch));
            input.modifiers = Modifiers::new(false, true, false, false);
            assert_eq!(
                router.key(&input, PrefixView::default()),
                Disposition::Native
            );
        }
        assert!(router.drain_effects().is_empty());
    }

    #[test]
    fn overlay_restores_valid_owner_and_defers_intent() {
        let mut router = router();
        activate(&mut router, P);
        router.event(InputEvent::OverlayOpened);
        assert_eq!(router.owner(), InputOwner::Overlay);
        assert_eq!(
            router.key(&press(KeyCode::Enter), CLAIMED),
            Disposition::Native
        );
        router.event(InputEvent::OverlayOpened);
        router.event(InputEvent::OverlayClosed);
        assert_eq!(router.owner(), InputOwner::Pane(P, SurfaceKind::Terminal));
        assert_eq!(
            router.drain_effects(),
            vec![Effect::RequestFocus(router.owner())]
        );
        router.event(InputEvent::OverlayOpened);
        router.event(InputEvent::PaneRemoved(P));
        router.event(InputEvent::OverlayClosed);
        assert_eq!(router.owner(), InputOwner::None);
        assert_eq!(router.active_pane(), None);
        assert_eq!(
            router.drain_effects(),
            vec![Effect::RequestFocus(InputOwner::None)]
        );
        activate(&mut router, P);
        router.event(InputEvent::OverlayOpened);
        router.event(InputEvent::ActivatePane(Q, SurfaceKind::Browser));
        assert_eq!(router.owner(), InputOwner::Overlay);
        assert!(router.drain_effects().is_empty());
        router.event(InputEvent::OverlayClosed);
        assert_eq!(router.owner(), InputOwner::Pane(Q, SurfaceKind::Browser));
        assert_eq!(
            router.drain_effects(),
            vec![Effect::RequestFocus(router.owner())]
        );
        router.event(InputEvent::OverlayOpened);
        router.event(InputEvent::FocusSidebar);
        router.event(InputEvent::FocusNativeEditor);
        assert_eq!(router.owner(), InputOwner::Overlay);
        assert!(router.drain_effects().is_empty());
        router.event(InputEvent::OverlayClosed);
        assert_eq!(router.owner(), InputOwner::NativeEditor);
        assert_eq!(
            router.drain_effects(),
            vec![Effect::RequestFocus(InputOwner::NativeEditor)]
        );
    }

    #[test]
    fn detach_clears_focus_active_and_held_state() {
        let mut router = router();
        activate(&mut router, P);
        let mut input = press(KeyCode::Character('j'));
        forward(&mut router, &input, P);
        router.event(InputEvent::OverlayOpened);
        router.event(InputEvent::Detached);
        assert_eq!(router.owner(), InputOwner::None);
        assert_eq!(router.active_pane(), None);
        input.action = KeyAction::Release;
        assert_eq!(router.key(&input, CLAIMED), Disposition::Native);
        router.event(InputEvent::OverlayClosed);
        assert!(router.drain_effects().is_empty());
        assert_eq!(router.restore, InputOwner::None);
        assert_eq!(router.observed, InputOwner::None);
        assert_eq!(router.pending, None);
    }

    #[test]
    fn pane_removal_drops_only_its_claimed_releases() {
        let mut router = router();
        activate(&mut router, P);
        let mut p = press(KeyCode::Character('a'));
        forward(&mut router, &p, P);
        activate(&mut router, Q);
        let mut q = press(KeyCode::Character('b'));
        forward(&mut router, &q, Q);
        router.event(InputEvent::PaneRemoved(P));
        p.action = KeyAction::Release;
        q.action = KeyAction::Release;
        assert_eq!(router.key(&p, CLAIMED), Disposition::Native);
        forward(&mut router, &q, Q);
        router.event(InputEvent::PaneRemoved(Q));
        assert_eq!(router.owner(), InputOwner::None);
        assert_eq!(router.active_pane(), None);
    }

    #[test]
    fn suppressed_claimed_release_is_consumed_and_the_next_press_forwards() {
        let mut router = router();
        activate(&mut router, P);
        let mut input = press(KeyCode::Character('s'));
        forward(&mut router, &input, P);
        router.suppress_release(input.key);
        input.action = KeyAction::Release;
        assert_eq!(
            router.key(&input, PrefixView::default()),
            Disposition::Consumed
        );
        assert!(router.drain_effects().is_empty());
        input.action = KeyAction::Press;
        forward(&mut router, &input, P);
    }

    #[test]
    fn local_shortcut_releases_never_reach_the_daemon_or_swallow_a_later_press() {
        let key = KeyCode::Character('s');
        let mut claim = PrefixClaim::default();
        claim.press(key, P, false);
        claim.suppress_release(key);
        assert!(claim.consume_local_release(key));
        assert_eq!(claim.consume_release(key), None);
        claim.press(key, P, false);
        claim.suppress_release(key);
        claim.press(key, P, false);
        assert!(!claim.consume_local_release(key));
        assert_eq!(claim.consume_release(key), Some(P));
        claim.press(key, P, false);
        claim.suppress_release(key);
        claim.clear();
        assert!(!claim.consume_local_release(key));
        assert_eq!(claim.consume_release(key), None);
    }

    fn forward_picker_input(tables: &zz_protocol::KeyTables, input: &KeyInput) {
        let mut router = router();
        activate(&mut router, P);
        let prefix = PrefixView {
            armed: true,
            claimed: tables.resolve_input("prefix", input).is_some(),
        };
        assert!(prefix.claimed);
        assert_eq!(router.key(input, prefix), Disposition::Consumed);
        assert_eq!(
            router.drain_effects(),
            vec![Effect::ForwardKey {
                pane: P,
                input: input.clone()
            }]
        );
    }

    #[test]
    fn sidebar_picker_input_follows_default_and_rebound_picker_commands() {
        let mut tables = zz_protocol::KeyTables::default();
        for key in ['s', 'w'] {
            forward_picker_input(&tables, &press(KeyCode::Character(key)));
        }
        let mut binding = tables
            .resolve_input("prefix", &press(KeyCode::Character('s')))
            .unwrap()
            .clone();
        tables.bind("prefix", "?", binding.clone());
        let mut input = press(KeyCode::Character('/'));
        input.modifiers = Modifiers::new(true, false, false, false);
        input.text = Some("?".into());
        input.unshifted_codepoint = Some('/');
        forward_picker_input(&tables, &input);
        binding.commands = vec![zz_protocol::CommandInvocation::new(
            "choose-tree",
            ["-Z", "-w"],
        )];
        tables.bind("prefix", "?", binding);
        forward_picker_input(&tables, &input);
    }

    #[test]
    fn sidebar_picker_input_preserves_custom_commands_filters_and_templates() {
        let input = press(KeyCode::Character('s'));
        let mut tables = zz_protocol::KeyTables::empty();
        for (name, args) in [
            ("resize-pane", vec!["-Z"]),
            ("choose-tree", vec![]),
            ("choose-tree", vec!["-Zsw"]),
            ("choose-tree", vec!["-Zs", "-f", "#{session_attached}"]),
            ("choose-tree", vec!["-Zw", "select-window -t %%"]),
            ("choose-tree", vec!["-Zw", "-t", "%1"]),
        ] {
            let commands = vec![zz_protocol::CommandInvocation::new(name, args)];
            tables.bind(
                "prefix",
                "s",
                zz_protocol::Binding {
                    commands: commands.clone(),
                    repeat: false,
                    note: None,
                },
            );
            forward_picker_input(&tables, &input);
            assert_eq!(
                tables.resolve_input("prefix", &input).unwrap().commands,
                commands
            );
        }
        let commands = vec![
            zz_protocol::CommandInvocation::new("choose-tree", ["-Zs"]),
            zz_protocol::CommandInvocation::new("display-message", ["custom"]),
        ];
        tables.bind(
            "prefix",
            "s",
            zz_protocol::Binding {
                commands: commands.clone(),
                repeat: false,
                note: None,
            },
        );
        forward_picker_input(&tables, &input);
        assert_eq!(
            tables.resolve_input("prefix", &input).unwrap().commands,
            commands
        );
    }

    #[test]
    fn keystrokes_match_their_canonical_spelling() {
        for (key, control, spelling) in [
            (KeyCode::Character('a'), true, "C-a"),
            (KeyCode::Character(' '), true, "C- "),
            (KeyCode::Character(' '), false, " "),
            (KeyCode::Character('`'), false, "`"),
            (KeyCode::ArrowUp, true, "C-Up"),
        ] {
            let mut input = press(key);
            input.modifiers = Modifiers::new(false, control, false, false);
            assert_eq!(zz_protocol::input_key_name(&input).as_str(), spelling);
            let mut router = router();
            activate(&mut router, P);
            forward(&mut router, &input, P);
        }
        assert_ne!(
            zz_protocol::input_key_name(&press(KeyCode::Character('a'))).as_str(),
            "C-a"
        );
        let mut input = press(KeyCode::Character('b'));
        input.modifiers = Modifiers::new(false, true, false, false);
        assert_ne!(zz_protocol::input_key_name(&input).as_str(), "C-a");
    }

    #[test]
    fn shifted_special_prefixes_use_the_same_fold_as_daemon_input() {
        for (key, spelling) in [
            (KeyCode::ArrowLeft, "S-Left"),
            (KeyCode::Tab, "BTab"),
            (KeyCode::Function(1), "S-F1"),
        ] {
            let mut input = press(key);
            assert_ne!(zz_protocol::input_key_name(&input).as_str(), spelling);
            input.modifiers = Modifiers::new(true, false, false, false);
            assert_eq!(zz_protocol::input_key_name(&input).as_str(), spelling);
            let mut router = router();
            activate(&mut router, P);
            forward(&mut router, &input, P);
        }
        let mut input = press(KeyCode::ArrowLeft);
        input.modifiers = Modifiers::new(true, false, false, false);
        assert_ne!(zz_protocol::input_key_name(&input).as_str(), "Left");
    }

    #[test]
    fn the_displayed_prefix_is_the_keystroke_that_arms_it() {
        for spelling in ["C-b", "C- ", "M-Right", "G", "`"] {
            let displayed = crate::ChromeKey::parse(spelling).unwrap();
            let key = if displayed.base == "Right" {
                KeyCode::ArrowRight
            } else {
                KeyCode::Character(displayed.base.chars().next().unwrap())
            };
            let mut input = press(key);
            input.modifiers = Modifiers::new(
                displayed.shift,
                displayed.control,
                displayed.alt,
                displayed.command,
            );
            assert_eq!(zz_protocol::input_key_name(&input).as_str(), spelling);
            let mut router = router();
            activate(&mut router, P);
            forward(&mut router, &input, P);
        }
        assert!(crate::ChromeKey::parse("").is_none());
    }

    #[test]
    fn platform_chords_never_match() {
        let core = crate::ClientCore::new();
        let mut input = press(KeyCode::Character('a'));
        input.modifiers = Modifiers::new(false, true, false, true);
        assert!(!core.claims_prefix_input(&input));
        let mut router = router();
        activate(&mut router, P);
        assert_eq!(
            router.key(
                &input,
                PrefixView {
                    armed: core.prefix_armed(),
                    claimed: core.claims_prefix_input(&input)
                }
            ),
            Disposition::Native
        );
        assert!(router.drain_effects().is_empty());
    }

    #[test]
    fn shifted_letters_require_shift() {
        let mut input = press(KeyCode::Character('g'));
        assert_ne!(zz_protocol::input_key_name(&input).as_str(), "G");
        input.modifiers = Modifiers::new(true, false, false, false);
        assert_eq!(zz_protocol::input_key_name(&input).as_str(), "G");
        let mut router = router();
        activate(&mut router, P);
        forward(&mut router, &input, P);
    }

    #[test]
    fn autorepeats_are_swallowed_and_releases_pair_with_presses() {
        let mut claim = PrefixClaim::default();
        let key = KeyCode::Character('a');
        assert_eq!(
            claim.press(key, P, false),
            PressDisposition::Forward { stale: false }
        );
        assert_eq!(claim.press(key, P, true), PressDisposition::Autorepeat);
        assert_eq!(claim.consume_release(key), Some(P));
        assert_eq!(claim.consume_release(key), None);
    }

    #[test]
    fn a_release_with_lifted_modifiers_still_pairs_with_its_press() {
        let mut router = router();
        activate(&mut router, P);
        let mut input = press(KeyCode::Character('a'));
        input.modifiers = Modifiers::new(false, true, false, false);
        forward(&mut router, &input, P);
        let mut release = input.clone();
        release.action = KeyAction::Release;
        release.modifiers = Modifiers::default();
        forward(&mut router, &release, P);
        forward(&mut router, &input, P);
    }

    #[test]
    fn releases_pair_by_the_unshifted_key_and_a_fresh_press_clears_a_stale_local_release() {
        let mut router = router();
        activate(&mut router, P);
        let mut shifted = press(KeyCode::Character('A'));
        shifted.modifiers = Modifiers::new(true, false, false, false);
        shifted.unshifted_codepoint = Some('a');
        forward(&mut router, &shifted, P);
        let mut release = press(KeyCode::Character('a'));
        release.action = KeyAction::Release;
        release.unshifted_codepoint = Some('a');
        forward(&mut router, &release, P);
        let mut chord = press(KeyCode::Character('k'));
        chord.modifiers = Modifiers::new(false, false, false, true);
        router
            .keymap
            .bind(UI_TABLE, "D-k", ChromeAction::OpenCommandPalette.name())
            .unwrap();
        assert_eq!(
            router.key(&chord, PrefixView::default()),
            Disposition::Consumed
        );
        router.drain_effects();
        let plain = press(KeyCode::Character('k'));
        assert_eq!(
            router.key(&plain, PrefixView::default()),
            Disposition::Native
        );
        let mut plain_release = plain.clone();
        plain_release.action = KeyAction::Release;
        assert_eq!(
            router.key(&plain_release, PrefixView::default()),
            Disposition::Native
        );
        assert!(router.drain_effects().is_empty());
    }

    #[test]
    fn a_lost_release_cannot_eat_the_next_press() {
        let mut claim = PrefixClaim::default();
        let key = KeyCode::Character('j');
        assert_eq!(
            claim.press(key, P, false),
            PressDisposition::Forward { stale: false }
        );
        assert_eq!(
            claim.press(key, P, false),
            PressDisposition::Forward { stale: true }
        );
        assert_eq!(claim.consume_release(key), Some(P));
        assert_eq!(
            claim.press(key, P, false),
            PressDisposition::Forward { stale: false }
        );
    }

    #[test]
    fn a_key_held_across_arming_keeps_its_release() {
        let mut claim = PrefixClaim::default();
        let key = KeyCode::Character('j');
        assert_eq!(claim.press(key, P, true), PressDisposition::Autorepeat);
        assert_eq!(claim.consume_release(key), None);
    }

    #[test]
    fn simulated_lossy_keyboard_never_eats_a_fresh_press() {
        const KEYS: [KeyCode; 3] = [
            KeyCode::Character('a'),
            KeyCode::Character('j'),
            KeyCode::Character('k'),
        ];

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
            let mut input = press(KEYS[index]);
            input.modifiers = Modifiers::new(false, random(2) != 0, false, false);
            match random(5) {
                0 => {
                    if model[index].down {
                        continue;
                    }
                    model[index].down = true;
                    let claimed = random(2) == 0;
                    model[index].claimed = claimed;
                    if claimed {
                        assert_eq!(
                            claim.press(input.key, P, false),
                            PressDisposition::Forward {
                                stale: model[index].stranded
                            },
                            "step {step}: fresh press of {:?} mishandled",
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
                            claim.press(input.key, P, true),
                            PressDisposition::Autorepeat,
                            "step {step}: repeat of {:?} forwarded",
                            KEYS[index]
                        );
                    }
                }
                2 => {
                    if !model[index].down {
                        continue;
                    }
                    model[index].down = false;
                    let expected = (model[index].claimed || model[index].stranded).then_some(P);
                    assert_eq!(
                        claim.consume_release(input.key),
                        expected,
                        "step {step}: release of {:?} mispaired",
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
