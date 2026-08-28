use zz_protocol::{MenuAction, MenuItem, MenuState, input_key_name};
use zz_terminal::{KeyAction, KeyCode, KeyInput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKeyResult {
    Action(MenuAction),
    Select(Option<usize>),
    Consumed,
}

#[must_use]
pub fn resolve_menu_key(
    state: &MenuState,
    selected: Option<usize>,
    input: &KeyInput,
) -> MenuKeyResult {
    if input.action == KeyAction::Release {
        return MenuKeyResult::Consumed;
    }
    let input_name = input_key_name(input);
    let key = if input.key == KeyCode::Tab && input.modifiers.shift() {
        "BTab"
    } else {
        input_name.as_str()
    };
    if let Some((index, _)) = state.items.iter().enumerate().find(|(_, item)| {
        item.as_ref()
            .is_some_and(|item| item.enabled && item.key.as_deref() == Some(key))
    }) {
        return MenuKeyResult::Action(MenuAction::Choose(u32::try_from(index).unwrap_or(u32::MAX)));
    }
    match key {
        "Escape" | "C-[" | "C-c" | "C-g" | "q" => MenuKeyResult::Action(MenuAction::Cancel),
        "Enter" => match selected {
            None => MenuKeyResult::Action(MenuAction::Cancel),
            Some(index) => match state.items.get(index) {
                Some(Some(item)) if item.enabled => MenuKeyResult::Action(MenuAction::Choose(
                    u32::try_from(index).unwrap_or(u32::MAX),
                )),
                _ if state.stay_open => MenuKeyResult::Consumed,
                _ => MenuKeyResult::Action(MenuAction::Cancel),
            },
        },
        "Up" | "k" | "BTab" => MenuKeyResult::Select(menu_step(&state.items, selected, -1)),
        "Down" | "j" => MenuKeyResult::Select(menu_step(&state.items, selected, 1)),
        "Home" | "g" => MenuKeyResult::Select(menu_edge(&state.items, false)),
        "End" | "G" => MenuKeyResult::Select(menu_edge(&state.items, true)),
        "PPage" | "C-b" => MenuKeyResult::Select(menu_page_up(&state.items, selected)),
        "NPage" => MenuKeyResult::Select(menu_page_down(&state.items, selected)),
        _ => MenuKeyResult::Consumed,
    }
}

fn menu_edge(items: &[Option<MenuItem>], reverse: bool) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    if reverse {
        items
            .iter()
            .rposition(|item| item.as_ref().is_some_and(|item| item.enabled))
            .or(Some(0))
    } else {
        items
            .iter()
            .position(|item| item.as_ref().is_some_and(|item| item.enabled))
            .or(Some(items.len().saturating_sub(1)))
    }
}

fn menu_step(
    items: &[Option<MenuItem>],
    selected: Option<usize>,
    direction: isize,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    let Some(selected) = selected else {
        return if direction < 0 {
            items
                .iter()
                .rposition(|item| item.as_ref().is_some_and(|item| item.enabled))
                .or(Some(0))
        } else {
            Some(0)
        };
    };
    let mut next = selected;
    loop {
        next = if direction < 0 {
            next.checked_sub(1).unwrap_or(items.len().saturating_sub(1))
        } else {
            next.saturating_add(1) % items.len()
        };
        if items[next].as_ref().is_some_and(|item| item.enabled) || next == selected {
            break;
        }
    }
    Some(next)
}

fn menu_page_up(items: &[Option<MenuItem>], selected: Option<usize>) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    let choice = selected.map_or(-1_isize, |selected| {
        isize::try_from(selected).unwrap_or(isize::MAX)
    });
    if choice < 6 {
        return Some(0);
    }
    let mut choice = usize::try_from(choice).unwrap_or_default();
    let mut remaining = 5;
    while remaining > 0 {
        choice = choice.saturating_sub(1);
        let selectable = items[choice].as_ref().is_some_and(|item| item.enabled);
        if choice != 0 && selectable {
            remaining -= 1;
        } else if choice == 0 {
            break;
        }
    }
    Some(choice)
}

fn menu_page_down(items: &[Option<MenuItem>], selected: Option<usize>) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    let count = isize::try_from(items.len()).unwrap_or(isize::MAX);
    let mut choice = selected.map_or(-1_isize, |selected| {
        isize::try_from(selected).unwrap_or(isize::MAX)
    });
    if choice > count - 6 {
        choice = count - 1;
    } else {
        let mut remaining = 5;
        while remaining > 0 {
            choice += 1;
            let selectable = items[usize::try_from(choice).unwrap_or_default()]
                .as_ref()
                .is_some_and(|item| item.enabled);
            if choice != count - 1 && selectable {
                remaining -= 1;
            } else if choice == count - 1 {
                break;
            }
        }
    }
    let mut choice = usize::try_from(choice).unwrap_or_default();
    while !items[choice].as_ref().is_some_and(|item| item.enabled) && choice != 0 {
        choice -= 1;
    }
    Some(choice)
}

#[cfg(test)]
mod tests {
    use zz_protocol::PopupBorderLines;
    use zz_terminal::Modifiers;

    use super::*;

    fn state() -> MenuState {
        MenuState {
            left: 0,
            top: 0,
            width: 20,
            height: 6,
            client_columns: 80,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 18,
            title: String::new(),
            style: "default".to_owned(),
            selected_style: "default".to_owned(),
            border_style: "default".to_owned(),
            border_lines: PopupBorderLines::Single,
            items: vec![
                Some(MenuItem {
                    name: "Quit item".to_owned(),
                    key: Some("q".to_owned()),
                    enabled: true,
                }),
                None,
                Some(MenuItem {
                    name: "Disabled".to_owned(),
                    key: None,
                    enabled: false,
                }),
                Some(MenuItem {
                    name: "Last".to_owned(),
                    key: None,
                    enabled: true,
                }),
            ],
            selected: Some(0),
            stay_open: false,
        }
    }

    fn input(key: KeyCode, modifiers: Modifiers, action: KeyAction) -> KeyInput {
        let character = match key {
            KeyCode::Character(character) => Some(character),
            _ => None,
        };
        KeyInput {
            action,
            key,
            modifiers,
            text: character.map(|character| character.to_string().into_boxed_str()),
            unshifted_codepoint: character,
        }
    }

    fn press(key: KeyCode) -> KeyInput {
        input(key, Modifiers::default(), KeyAction::Press)
    }

    #[test]
    fn shortcuts_win_before_cancel_and_disabled_shortcuts_do_not() {
        assert_eq!(
            resolve_menu_key(&state(), Some(0), &press(KeyCode::Character('q'))),
            MenuKeyResult::Action(MenuAction::Choose(0))
        );
        let disabled = MenuState {
            items: vec![Some(MenuItem {
                name: "Disabled".to_owned(),
                key: Some("q".to_owned()),
                enabled: false,
            })],
            ..state()
        };
        assert_eq!(
            resolve_menu_key(&disabled, Some(0), &press(KeyCode::Character('q'))),
            MenuKeyResult::Action(MenuAction::Cancel)
        );
    }

    #[test]
    fn navigation_skips_unusable_rows_wraps_and_handles_edges() {
        assert_eq!(
            resolve_menu_key(&state(), Some(0), &press(KeyCode::ArrowDown)),
            MenuKeyResult::Select(Some(3))
        );
        assert_eq!(
            resolve_menu_key(&state(), Some(3), &press(KeyCode::ArrowDown)),
            MenuKeyResult::Select(Some(0))
        );
        assert_eq!(
            resolve_menu_key(&state(), Some(0), &press(KeyCode::ArrowUp)),
            MenuKeyResult::Select(Some(3))
        );
        assert_eq!(
            resolve_menu_key(&state(), Some(3), &press(KeyCode::Home)),
            MenuKeyResult::Select(Some(0))
        );
        assert_eq!(
            resolve_menu_key(&state(), Some(0), &press(KeyCode::End)),
            MenuKeyResult::Select(Some(3))
        );
    }

    #[test]
    fn unselected_and_all_disabled_navigation_keeps_raw_boundary_rows() {
        let starts_disabled = MenuState {
            items: vec![
                Some(MenuItem {
                    name: "Disabled".to_owned(),
                    key: None,
                    enabled: false,
                }),
                Some(MenuItem {
                    name: "Enabled".to_owned(),
                    key: None,
                    enabled: true,
                }),
            ],
            selected: None,
            ..state()
        };
        assert_eq!(
            resolve_menu_key(&starts_disabled, None, &press(KeyCode::ArrowDown)),
            MenuKeyResult::Select(Some(0))
        );
        assert_eq!(
            resolve_menu_key(&starts_disabled, None, &press(KeyCode::ArrowUp)),
            MenuKeyResult::Select(Some(1))
        );

        let all_disabled = MenuState {
            items: (0..3)
                .map(|index| {
                    Some(MenuItem {
                        name: format!("Disabled {index}"),
                        key: None,
                        enabled: false,
                    })
                })
                .collect(),
            selected: None,
            stay_open: true,
            ..state()
        };
        for key in [KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::End] {
            assert_eq!(
                resolve_menu_key(&all_disabled, None, &press(key)),
                MenuKeyResult::Select(Some(0))
            );
        }
        assert_eq!(
            resolve_menu_key(&all_disabled, None, &press(KeyCode::Home)),
            MenuKeyResult::Select(Some(2))
        );
        assert_eq!(
            resolve_menu_key(&all_disabled, Some(0), &press(KeyCode::Enter)),
            MenuKeyResult::Consumed
        );
    }

    #[test]
    fn shift_tab_is_backtab_and_repeat_matches_press() {
        let shift_tab = input(
            KeyCode::Tab,
            Modifiers::new(true, false, false, false),
            KeyAction::Press,
        );
        assert_eq!(
            resolve_menu_key(&state(), Some(3), &shift_tab),
            MenuKeyResult::Select(Some(0))
        );
        let repeat = input(KeyCode::ArrowDown, Modifiers::default(), KeyAction::Repeat);
        assert_eq!(
            resolve_menu_key(&state(), Some(0), &repeat),
            MenuKeyResult::Select(Some(3))
        );
    }

    #[test]
    fn paging_uses_raw_rows_and_clamps_at_edges() {
        let long = MenuState {
            items: (0..12)
                .map(|index| {
                    Some(MenuItem {
                        name: format!("item {index}"),
                        key: None,
                        enabled: true,
                    })
                })
                .collect(),
            ..state()
        };
        assert_eq!(
            resolve_menu_key(&long, Some(0), &press(KeyCode::PageDown)),
            MenuKeyResult::Select(Some(5))
        );
        assert_eq!(
            resolve_menu_key(&long, Some(8), &press(KeyCode::PageDown)),
            MenuKeyResult::Select(Some(11))
        );
        assert_eq!(
            resolve_menu_key(&long, Some(11), &press(KeyCode::PageUp)),
            MenuKeyResult::Select(Some(6))
        );
        assert_eq!(
            resolve_menu_key(&long, Some(3), &press(KeyCode::PageUp)),
            MenuKeyResult::Select(Some(0))
        );
        let raw_row_zero = MenuState {
            items: vec![
                None,
                Some(MenuItem {
                    name: "One".to_owned(),
                    key: None,
                    enabled: true,
                }),
                Some(MenuItem {
                    name: "Two".to_owned(),
                    key: None,
                    enabled: true,
                }),
                Some(MenuItem {
                    name: "Three".to_owned(),
                    key: None,
                    enabled: true,
                }),
            ],
            stay_open: true,
            ..state()
        };
        assert_eq!(
            resolve_menu_key(&raw_row_zero, Some(3), &press(KeyCode::PageUp)),
            MenuKeyResult::Select(Some(0))
        );
        assert_eq!(
            resolve_menu_key(&raw_row_zero, Some(0), &press(KeyCode::Enter)),
            MenuKeyResult::Consumed
        );
    }

    #[test]
    fn enter_chooses_enabled_rows_and_closes_unusable_selections() {
        assert_eq!(
            resolve_menu_key(&state(), Some(3), &press(KeyCode::Enter)),
            MenuKeyResult::Action(MenuAction::Choose(3))
        );
        assert_eq!(
            resolve_menu_key(&state(), Some(2), &press(KeyCode::Enter)),
            MenuKeyResult::Action(MenuAction::Cancel)
        );
        assert_eq!(
            resolve_menu_key(&state(), None, &press(KeyCode::Enter)),
            MenuKeyResult::Action(MenuAction::Cancel)
        );
        let stay_open = MenuState {
            stay_open: true,
            ..state()
        };
        assert_eq!(
            resolve_menu_key(&stay_open, Some(2), &press(KeyCode::Enter)),
            MenuKeyResult::Consumed
        );
    }

    #[test]
    fn every_cancel_key_closes_and_releases_are_consumed() {
        let without_shortcuts = MenuState {
            items: Vec::new(),
            ..state()
        };
        let cancel_keys = [
            press(KeyCode::Escape),
            input(
                KeyCode::Character('['),
                Modifiers::new(false, true, false, false),
                KeyAction::Press,
            ),
            input(
                KeyCode::Character('c'),
                Modifiers::new(false, true, false, false),
                KeyAction::Press,
            ),
            input(
                KeyCode::Character('g'),
                Modifiers::new(false, true, false, false),
                KeyAction::Press,
            ),
            press(KeyCode::Character('q')),
        ];
        for input in &cancel_keys {
            assert_eq!(
                resolve_menu_key(&without_shortcuts, None, input),
                MenuKeyResult::Action(MenuAction::Cancel)
            );
        }
        let release = input(
            KeyCode::Character('q'),
            Modifiers::default(),
            KeyAction::Release,
        );
        assert_eq!(
            resolve_menu_key(&state(), Some(0), &release),
            MenuKeyResult::Consumed
        );
    }
}
