use crate::terminal_event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Choice {
    Terminal,
    Browser,
    Agent,
    Editor,
}

impl Choice {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal (t)",
            Self::Browser => "Browser (b)",
            Self::Agent => "Agent (a) — runs in the zz app",
            Self::Editor => "Editor (e)",
        }
    }

    pub const fn argument(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Browser => "browser",
            Self::Agent => "agent",
            Self::Editor => "editor",
        }
    }
}

pub(crate) const CHOICES: [Choice; 4] = [
    Choice::Terminal,
    Choice::Browser,
    Choice::Agent,
    Choice::Editor,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Previous,
    Next,
    Materialize(Choice),
    Cancel,
}

pub(crate) fn key_action(event: KeyEvent, selected: usize) -> Option<Action> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    match event.code {
        KeyCode::Esc => Some(Action::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Next),
        KeyCode::Enter => CHOICES.get(selected).copied().map(Action::Materialize),
        KeyCode::Char(character)
            if !event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
        {
            match character.to_ascii_lowercase() {
                't' => Some(Action::Materialize(Choice::Terminal)),
                'b' => Some(Action::Materialize(Choice::Browser)),
                'a' => Some(Action::Materialize(Choice::Agent)),
                'e' => Some(Action::Materialize(Choice::Editor)),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_maps_navigation_enter_direct_keys_and_cancel() {
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);

        assert_eq!(key_action(key(KeyCode::Up), 0), Some(Action::Previous));
        assert_eq!(key_action(key(KeyCode::Char('j')), 0), Some(Action::Next));
        assert_eq!(
            key_action(key(KeyCode::Enter), 2),
            Some(Action::Materialize(Choice::Agent))
        );
        assert_eq!(
            key_action(key(KeyCode::Char('E')), 0),
            Some(Action::Materialize(Choice::Editor))
        );
        assert_eq!(key_action(key(KeyCode::Esc), 0), Some(Action::Cancel));
    }
}
