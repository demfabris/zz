use std::{fmt::Write as _, sync::Arc};

use gpui::{
    AnyElement, App, ElementId, IntoElement, MouseButton, RenderOnce, SharedString, Window, div,
    prelude::*, px,
};
use zz_protocol::PaneKindSnapshot;

use crate::{ActiveTheme as _, Colorize as _, Icon, IconName, tooltip::Tooltip};

use super::StatusAction;

const CARD_SIZE: f32 = 26.0;
const CARD_OVERLAP: f32 = 9.0;
const MAX_PANE_CARDS: usize = 3;

#[derive(Clone)]
pub struct StatusPaneEntry {
    pub label: SharedString,
    pub detail: SharedString,
    pub icon: IconName,
    pub favicon: Option<Arc<[u8]>>,
    pub active: bool,
    pub select: StatusAction,
}

impl StatusPaneEntry {
    pub fn from_pane(pane: &zz_client::StatusBarPane, select: StatusAction) -> Self {
        let icon = match &pane.kind {
            PaneKindSnapshot::Terminal => IconName::SquareTerminal,
            PaneKindSnapshot::Browser(_) => IconName::Globe,
            PaneKindSnapshot::Agent(agent) => crate::pane::agent_provider_icon(agent.provider),
            PaneKindSnapshot::Editor(_) => IconName::File,
            PaneKindSnapshot::Picker => IconName::Plus,
        };
        let detail = match &pane.kind {
            PaneKindSnapshot::Terminal => format!("Terminal · {}", pane.id),
            PaneKindSnapshot::Browser(browser) => format!(
                "Browser · {} · {} tab{} · {}\n{}",
                pane.id,
                browser.tabs.len(),
                if browser.tabs.len() == 1 { "" } else { "s" },
                browser.profile,
                browser.url(),
            ),
            PaneKindSnapshot::Agent(agent) => {
                let mut detail = format!("{} · {}", agent.provider.label(), pane.id);
                if let Some(cwd) = &agent.cwd {
                    let _ = write!(detail, "\n{}", cwd.display());
                }
                detail
            }
            PaneKindSnapshot::Editor(editor) => format!(
                "Editor · {}\n{}",
                pane.id,
                editor.path.as_deref().unwrap_or(&editor.cwd),
            ),
            PaneKindSnapshot::Picker => format!("New pane · {}", pane.id),
        };
        Self {
            label: pane.label.clone().into(),
            detail: detail.into(),
            icon,
            favicon: None,
            active: pane.active,
            select,
        }
    }
}

fn visible_pane_indices(total: usize, active: usize) -> Vec<usize> {
    let mut indices = (0..total.min(MAX_PANE_CARDS)).collect::<Vec<_>>();
    if active >= MAX_PANE_CARDS && active < total {
        indices[MAX_PANE_CARDS - 1] = active;
    }
    indices
}

pub fn status_pane_deck(
    id: impl Into<ElementId>,
    panes: Vec<StatusPaneEntry>,
    connected: bool,
    _cx: &App,
) -> Option<AnyElement> {
    (!panes.is_empty()).then(|| {
        PaneDeck {
            id: id.into(),
            panes,
            connected,
        }
        .into_any_element()
    })
}

#[derive(IntoElement)]
struct PaneDeck {
    id: ElementId,
    panes: Vec<StatusPaneEntry>,
    connected: bool,
}

fn card_paint_order(count: usize, active: Option<usize>, hovered: Option<usize>) -> Vec<usize> {
    let mut slots = (0..count).rev().collect::<Vec<_>>();
    slots.sort_by_key(|slot| (Some(*slot) == active, Some(*slot) == hovered));
    slots
}

impl RenderOnce for PaneDeck {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let hover = window.use_keyed_state(self.id.clone(), cx, |_, _| None::<usize>);
        let hovered = *hover.read(cx);
        let parent = window.current_view();
        let active = self.panes.iter().position(|pane| pane.active).unwrap_or(0);
        let indices = visible_pane_indices(self.panes.len(), active);
        let active_slot = indices.iter().position(|index| self.panes[*index].active);
        let hidden = self.panes.len() - indices.len();
        let count = indices.len() + usize::from(hidden > 0);
        let width = CARD_SIZE + (count - 1) as f32 * (CARD_SIZE - CARD_OVERLAP);
        let overflow_text = self
            .panes
            .iter()
            .enumerate()
            .filter(|(index, _)| !indices.contains(index))
            .map(|(_, pane)| pane.label.as_ref())
            .collect::<Vec<_>>()
            .join("\n");
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(CARD_SIZE + 2.0))
            .children(
                card_paint_order(count, active_slot, hovered)
                    .into_iter()
                    .map(|slot| {
                        let pane = indices.get(slot).map(|index| &self.panes[*index]);
                        let is_hovered = hovered == Some(slot);
                        let active = pane.is_some_and(|pane| pane.active);
                        let tooltip: SharedString = pane
                            .map_or_else(
                                || {
                                    format!(
                                        "{hidden} more pane{}\n{overflow_text}",
                                        if hidden == 1 { "" } else { "s" }
                                    )
                                },
                                |pane| {
                                    format!(
                                        "{}\n{}{}",
                                        pane.label,
                                        pane.detail,
                                        if active { "\nFocused pane" } else { "" }
                                    )
                                },
                            )
                            .into();
                        let select = pane.map(|pane| pane.select.clone());
                        let connected = self.connected;
                        div()
                            .id(("pane-card", slot))
                            .absolute()
                            .left(px(slot as f32 * (CARD_SIZE - CARD_OVERLAP)))
                            .w(px(CARD_SIZE))
                            .h(px(CARD_SIZE + 2.0))
                            .occlude()
                            .when(connected && pane.is_some(), gpui::Styled::cursor_pointer)
                            .on_hover({
                                let hover = hover.clone();
                                move |entered, _, cx| {
                                    hover.update(cx, |hovered, _| {
                                        if *entered {
                                            *hovered = Some(slot);
                                        } else if *hovered == Some(slot) {
                                            *hovered = None;
                                        }
                                    });
                                    cx.notify(parent);
                                }
                            })
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone())
                                    .max_w(px(360.0))
                                    .build(window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                if connected && let Some(select) = &select {
                                    select(window, cx);
                                }
                            })
                            .child(
                                pane_card(active, is_hovered, cx)
                                    .debug_selector(move || {
                                        format!("pane-deck-card-{slot}-{is_hovered}")
                                    })
                                    .child(if let Some(pane) = pane {
                                        if pane.icon == IconName::Globe {
                                            crate::browser::browser_favicon(
                                                pane.favicon.clone(),
                                                cx,
                                            )
                                        } else {
                                            Icon::new(pane.icon.clone())
                                                .size(px(15.0))
                                                .into_any_element()
                                        }
                                    } else {
                                        div()
                                            .text_size(crate::rems_from_px(11.0))
                                            .line_height(px(16.0))
                                            .child(format!("+{hidden}"))
                                            .into_any_element()
                                    }),
                            )
                    }),
            )
    }
}

fn pane_card(active: bool, hovered: bool, cx: &App) -> gpui::Div {
    div()
        .absolute()
        .top(px(if hovered {
            0.0
        } else if active {
            1.0
        } else {
            2.0
        }))
        .size(px(CARD_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().control_radius())
        .border(px(0.5))
        .border_color(cx.theme().foreground.opacity(0.16))
        .bg(cx
            .theme()
            .background
            .raised(if active || hovered { 2 } else { 1 })
            .opaque())
        .text_color(if active || hovered {
            cx.theme().foreground
        } else {
            cx.theme().foreground.muted()
        })
        .when(cx.theme().shadow, |card| {
            card.shadow(
                crate::control_shadow(cx)
                    .into_iter()
                    .map(|mut shadow| {
                        shadow.offset.x = px(-1.5);
                        shadow
                    })
                    .collect::<Vec<_>>(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Context, Modifiers, MouseButton, Render, TestAppContext, VisualTestContext, Window,
    };
    use std::{cell::Cell, rc::Rc};

    struct DeckTest {
        selected: Rc<Cell<Option<usize>>>,
        window_selected: Rc<Cell<bool>>,
        connected: bool,
    }

    impl Render for DeckTest {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let window_selected = self.window_selected.clone();
            let connected = self.connected;
            div()
                .id("test-window")
                .flex()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, _| {
                    if connected {
                        window_selected.set(true);
                    }
                })
                .child(
                    div()
                        .h(px(30.0))
                        .debug_selector(|| "deck".into())
                        .children(status_pane_deck(
                            "test-deck",
                            (0..6)
                                .map(|index| {
                                    let selected = self.selected.clone();
                                    StatusPaneEntry {
                                        label: format!("Pane {index}").into(),
                                        detail: "Terminal".into(),
                                        icon: IconName::SquareTerminal,
                                        favicon: None,
                                        active: index == 1,
                                        select: Rc::new(move |_, _| selected.set(Some(index))),
                                    }
                                })
                                .collect(),
                            self.connected,
                            cx,
                        )),
                )
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn cards_select_panes_directly_without_opening_a_menu(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let selected = Rc::new(Cell::new(None));
        let window_selected = Rc::new(Cell::new(false));
        let (view, cx) = cx.add_window_view({
            let selected = selected.clone();
            let window_selected = window_selected.clone();
            move |_, _| DeckTest {
                selected,
                window_selected,
                connected: true,
            }
        });
        draw(cx);
        let bounds = cx.debug_bounds("deck").unwrap();
        let resting = cx.debug_bounds("pane-deck-card-1-false").unwrap();
        cx.simulate_mouse_move(bounds.center(), None, Modifiers::none());
        draw(cx);
        let raised = cx.debug_bounds("pane-deck-card-1-true").unwrap();
        assert_eq!(raised.origin.y, resting.origin.y - px(1.0));
        cx.simulate_mouse_move(gpui::point(px(200.0), px(80.0)), None, Modifiers::none());
        draw(cx);
        assert_eq!(cx.debug_bounds("pane-deck-card-1-false").unwrap(), resting);
        cx.simulate_click(bounds.center(), Modifiers::none());
        draw(cx);
        assert_eq!(selected.get(), Some(1));
        cx.update(|window, cx| assert!(window.focused(cx).is_none()));
        assert!(!window_selected.get());
        selected.set(None);
        view.update(cx, |view, cx| {
            view.connected = false;
            cx.notify();
        });
        draw(cx);
        let bounds = cx.debug_bounds("deck").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::none());
        draw(cx);
        cx.simulate_keystrokes("down enter");
        draw(cx);
        assert_eq!(selected.get(), None);
        assert!(!window_selected.get());
    }

    #[test]
    fn active_card_always_paints_above_hover_and_the_rest_of_the_deck() {
        assert_eq!(card_paint_order(4, None, None), vec![3, 2, 1, 0]);
        assert_eq!(card_paint_order(4, None, Some(1)), vec![3, 2, 0, 1]);
        assert_eq!(card_paint_order(4, Some(1), None), vec![3, 2, 0, 1]);
        assert_eq!(card_paint_order(4, Some(1), Some(2)), vec![3, 0, 2, 1]);
        assert_eq!(card_paint_order(4, Some(1), Some(3)), vec![2, 0, 3, 1]);
        for active in 0..3 {
            for hovered in [None, Some(0), Some(1), Some(2), Some(3)] {
                assert_eq!(
                    card_paint_order(4, Some(active), hovered).last(),
                    Some(&active)
                );
            }
        }
    }

    #[test]
    fn overflow_keeps_the_active_pane_visible_without_reordering_the_leading_panes() {
        assert_eq!(visible_pane_indices(0, 0), Vec::<usize>::new());
        assert_eq!(visible_pane_indices(2, 1), vec![0, 1]);
        assert_eq!(visible_pane_indices(6, 1), vec![0, 1, 2]);
        assert_eq!(visible_pane_indices(6, 5), vec![0, 1, 5]);
        assert_eq!(visible_pane_indices(6, 9), vec![0, 1, 2]);
    }
}
