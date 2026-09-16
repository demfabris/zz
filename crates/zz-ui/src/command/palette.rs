use std::ops::Range;

use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, FontWeight, HighlightStyle, Hsla,
    MouseButton, SharedString, StyledText, Window, div, prelude::*, px, relative,
};

use crate::{
    ActiveTheme as _, Colorize as _, Icon, IconName,
    input::{Input, InputState},
    list::ListItem,
    tag::Tag,
};

use super::COMMAND_PALETTE_ROW_HEIGHT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteStatus {
    Online,
    Running,
    Waiting,
    Offline,
}

impl PaletteStatus {
    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Online => cx.theme().success,
            Self::Running => cx.theme().accent,
            Self::Waiting => cx.theme().warning,
            Self::Offline => cx.theme().foreground.muted().opacity(0.5),
        }
    }
}

pub enum PalettePill {
    Mode {
        prefix: SharedString,
        label: SharedString,
    },
    Host {
        label: SharedString,
        status: PaletteStatus,
    },
    Command(SharedString),
}

#[derive(Clone, Debug, Default)]
pub struct PaletteRow {
    pub label: SharedString,
    pub detail: SharedString,
    pub right: SharedString,
    pub matches: Vec<Range<usize>>,
    pub muted_prefix: usize,
    pub status: Option<PaletteStatus>,
    pub running: bool,
    pub indent: f32,
    pub shortcut: Option<SharedString>,
    pub icon: Option<IconName>,
    pub expanded: Option<bool>,
}

fn status_dot(status: PaletteStatus, cx: &App) -> Div {
    div()
        .size(px(8.0))
        .flex_none()
        .rounded_full()
        .bg(status.color(cx))
}

fn pill(pill: PalettePill, cx: &App) -> Tag {
    let tag = Tag::secondary()
        .flex_none()
        .gap(px(6.0))
        .px(px(8.0))
        .py(px(2.0))
        .border_0()
        .rounded(cx.theme().menu_radius())
        .bg(cx.theme().background.raised(3))
        .text_size(crate::rems_from_px(12.0))
        .line_height(px(16.0));
    match pill {
        PalettePill::Mode { prefix, label } => tag
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(crate::rems_from_px(12.0))
                    .text_color(cx.theme().accent)
                    .child(prefix),
            )
            .child(label),
        PalettePill::Host { label, status } => tag.child(status_dot(status, cx)).child(label),
        PalettePill::Command(label) => tag
            .font_family(cx.theme().mono_font_family.clone())
            .child(label),
    }
}

pub fn unified_command_palette_input(
    input: &Entity<InputState>,
    pills: impl IntoIterator<Item = PalettePill>,
    cx: &App,
) -> Input {
    let pills: Vec<_> = pills.into_iter().collect();
    Input::new(input)
        .appearance(false)
        .w_full()
        .h(px(40.0))
        .px(px(14.0))
        .py(px(8.0))
        .gap(px(6.0))
        .font_family(cx.theme().font_family.clone())
        .text_size(crate::rems_from_px(12.0))
        .line_height(px(16.0))
        .when(!pills.is_empty(), |input| {
            input.prefix(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .children(pills.into_iter().map(|value| pill(value, cx))),
            )
        })
}

pub fn command_palette_entry(
    id: impl Into<ElementId>,
    row: &PaletteRow,
    selected: bool,
    cx: &App,
) -> ListItem {
    palette_entry(id, row, selected, None, None, cx)
}

pub fn command_palette_tree_entry(
    id: impl Into<ElementId>,
    row: &PaletteRow,
    selected: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> ListItem {
    palette_entry(id, row, selected, None, Some(Box::new(on_toggle)), cx)
}

type ToggleHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub(super) fn palette_entry(
    id: impl Into<ElementId>,
    row: &PaletteRow,
    selected: bool,
    badge: Option<AnyElement>,
    on_toggle: Option<ToggleHandler>,
    cx: &App,
) -> ListItem {
    let foreground = cx.theme().foreground;
    let secondary = if selected {
        foreground
    } else {
        foreground.muted()
    };
    let highlights = row
        .label
        .char_indices()
        .filter_map(|(start, character)| {
            let end = start + character.len_utf8();
            let matched = row.matches.iter().any(|range| range.contains(&start));
            (matched || start < row.muted_prefix).then_some((
                start..end,
                HighlightStyle {
                    color: Some(if matched { foreground } else { secondary }),
                    font_weight: matched.then_some(FontWeight::SEMIBOLD),
                    ..Default::default()
                },
            ))
        })
        .collect::<Vec<_>>();

    ListItem::new(id)
        .w_full()
        .h(px(COMMAND_PALETTE_ROW_HEIGHT - 2.0))
        .pl(px(12.0 + row.indent))
        .pr(px(12.0))
        .py(px(4.0))
        .selected(selected)
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.0))
                .font_family(cx.theme().font_family.clone())
                .text_size(crate::rems_from_px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .line_height(px(16.0))
                .when(row.expanded.is_some() || row.indent > 0.0, |element| {
                    element.child(
                        div()
                            .id("disclosure")
                            .debug_selector(|| "command-palette-disclosure".to_owned())
                            .flex_none()
                            .size(px(12.0))
                            .relative()
                            .top(px(0.5))
                            .when_some(row.expanded, |element, expanded| {
                                element
                                    .cursor_pointer()
                                    .child(
                                        Icon::new(if expanded {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .size(px(12.0)),
                                    )
                                    .when_some(on_toggle, |element, on_toggle| {
                                        element
                                            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                                window.prevent_default();
                                                cx.stop_propagation();
                                            })
                                            .on_click(move |event, window, cx| {
                                                on_toggle(event, window, cx);
                                                cx.stop_propagation();
                                            })
                                    })
                            }),
                    )
                })
                .when_some(row.icon.clone(), |element, icon| {
                    element.child(
                        div()
                            .flex_none()
                            .relative()
                            .top(px(0.5))
                            .child(Icon::new(icon).size(px(12.0))),
                    )
                })
                .children(row.status.map(|status| {
                    status_dot(status, cx).when(selected, |dot| {
                        dot.border_1().border_color(foreground.opacity(0.65))
                    })
                }))
                .child(
                    div()
                        .min_w_0()
                        .when(row.detail.is_empty(), gpui::Styled::flex_1)
                        .when(!row.detail.is_empty(), |label| {
                            label.flex_shrink_0().max_w(relative(0.6))
                        })
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(StyledText::new(row.label.clone()).with_highlights(highlights)),
                )
                .when(!row.detail.is_empty(), |element| {
                    element.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(crate::rems_from_px(10.0))
                            .font_weight(FontWeight::NORMAL)
                            .line_height(px(16.0))
                            .text_color(secondary)
                            .child(row.detail.clone()),
                    )
                })
                .when(!row.right.is_empty(), |element| {
                    element.child(
                        div()
                            .flex_none()
                            .text_size(crate::rems_from_px(10.0))
                            .font_weight(FontWeight::NORMAL)
                            .line_height(px(16.0))
                            .text_color(secondary)
                            .child(row.right.clone()),
                    )
                })
                .when_some(row.shortcut.clone(), |element, shortcut| {
                    element.child(
                        Tag::secondary()
                            .flex_none()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(crate::rems_from_px(10.0))
                            .font_weight(FontWeight::NORMAL)
                            .line_height(px(16.0))
                            .px(px(5.0))
                            .py_0()
                            .border_0()
                            .rounded(cx.theme().menu_radius())
                            .bg(if selected {
                                foreground.opacity(0.12)
                            } else {
                                cx.theme().background.raised(3)
                            })
                            .text_color(secondary)
                            .child(shortcut),
                    )
                })
                .children(badge)
                .when(row.running, |element| {
                    element.child(
                        div()
                            .size(px(8.0))
                            .flex_none()
                            .rounded_full()
                            .bg(if selected {
                                foreground
                            } else {
                                cx.theme().accent
                            }),
                    )
                }),
        )
}

pub fn command_palette_section(
    label: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    cx: &App,
) -> Div {
    div()
        .h(px(COMMAND_PALETTE_ROW_HEIGHT))
        .flex()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .pt(px(4.0))
        .text_size(crate::rems_from_px(11.0))
        .line_height(px(16.0))
        .text_color(cx.theme().foreground.muted())
        .child(label.into())
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(crate::rems_from_px(10.0))
                .child(hint.into()),
        )
}

pub fn command_palette_empty(cx: &App) -> Div {
    div()
        .py(px(16.0))
        .text_center()
        .text_size(crate::rems_from_px(12.0))
        .text_color(cx.theme().foreground.muted())
        .child("No matches")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Root, Theme, ThemeMode,
        command::{CommandPaletteSurface, PaletteHint},
    };
    use gpui::{Context, Focusable as _, Render, TestAppContext, Window};

    struct Preview {
        input: Entity<InputState>,
        width: f32,
        expanded: bool,
        activations: usize,
    }

    impl Render for Preview {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let toggle = cx.entity();
            let activate = cx.entity();
            div().w(px(self.width)).px(px(16.0)).child(
                CommandPaletteSurface::new(
                    unified_command_palette_input(
                        &self.input,
                        [
                            PalettePill::Host {
                                label: "gpu-box".into(),
                                status: PaletteStatus::Online,
                            },
                            PalettePill::Mode {
                                prefix: ":".into(),
                                label: "Command".into(),
                            },
                            PalettePill::Command("join-pane".into()),
                        ],
                        cx,
                    ),
                    0,
                )
                .usage("join-pane [-bdhv] [-s src-pane] [-t dst-pane]")
                .rows(
                    div().child(
                        command_palette_tree_entry(
                            "selected",
                            &PaletteRow {
                                label: "zz-dev / editor".into(),
                                muted_prefix: 9,
                                status: Some(PaletteStatus::Running),
                                right: "Running".into(),
                                icon: Some(IconName::Folder),
                                expanded: Some(self.expanded),
                                ..Default::default()
                            },
                            true,
                            move |_, _, cx| {
                                toggle.update(cx, |preview, cx| {
                                    preview.expanded = !preview.expanded;
                                    cx.notify();
                                });
                            },
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            activate.update(cx, |preview, _| preview.activations += 1);
                        }),
                    ),
                )
                .hints([
                    PaletteHint {
                        key: "up down",
                        label: "navigate",
                    },
                    PaletteHint {
                        key: "enter",
                        label: "run",
                    },
                    PaletteHint {
                        key: "backspace",
                        label: "back",
                    },
                ]),
            )
        }
    }

    #[gpui::test]
    fn palette_fits_its_container_and_tracks_light_and_dark_themes(cx: &mut TestAppContext) {
        for (width, mode, radius) in [
            (720.0_f32, ThemeMode::Dark, 12.0),
            (440.0, ThemeMode::Light, 25.0),
        ] {
            cx.update(|cx| {
                crate::init(cx);
                cx.set_reduce_motion(true);
                Theme::change(mode, None, cx);
                Theme::global_mut(cx).radius = px(radius);
            });
            let (_, cx) = cx.add_window_view(|window, cx| {
                window.set_adaptive_corner_fraction(Some(0.45));
                let preview = cx.new(|cx| Preview {
                    input: cx.new(|cx| InputState::new(window, cx).placeholder("Target pane")),
                    width,
                    expanded: false,
                    activations: 0,
                });
                preview.read(cx).input.focus_handle(cx).focus(window, cx);
                Root::new(preview, window, cx)
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                _ = window.draw(cx);
                let quads = window.painted_quads();
                let surface = quads
                    .iter()
                    .find(|quad| {
                        quad.background
                            == gpui::solid_background(cx.theme().background.raised(2).opaque())
                            && quad.bounds.size.width.0 > 300.0 * window.scale_factor()
                    })
                    .expect("palette surface");
                let selection = quads
                    .iter()
                    .find(|quad| {
                        quad.background == gpui::solid_background(cx.theme().selection_background())
                            && quad.bounds.size.width.0 > 100.0 * window.scale_factor()
                    })
                    .expect("selected row");
                assert_eq!(
                    surface.bounds.size.width.0 / window.scale_factor(),
                    (width - 32.0).min(560.0)
                );
                assert!(surface.bounds.contains(&selection.bounds.origin));
                assert!(surface.bounds.contains(&selection.bounds.bottom_right()));
                assert!(selection.corner_radii.top_left.0 < selection.bounds.size.height.0 / 2.0);
                assert!(
                    quads.iter().any(|quad| {
                        quad.border_color == cx.theme().foreground.opacity(0.65)
                            && quad.bounds.size.width.0 == 8.0 * window.scale_factor()
                            && quad.border_widths.top.0 > 0.0
                    }),
                    "selected running agent remains visible on the accent selection"
                );
            });
        }
    }

    #[gpui::test]
    fn disclosure_toggles_without_activating_or_taking_input_focus(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (preview, cx) = cx.add_window_view(|window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx));
            input.focus_handle(cx).focus(window, cx);
            Preview {
                input,
                width: 560.0,
                expanded: false,
                activations: 0,
            }
        });
        cx.run_until_parked();
        let disclosure = cx.debug_bounds("command-palette-disclosure").unwrap();
        cx.simulate_click(disclosure.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        preview.read_with(cx, |preview, _| {
            assert!(preview.expanded);
            assert_eq!(preview.activations, 0);
        });
        assert!(cx.update(|window, cx| preview.read(cx).input.focus_handle(cx).is_focused(window)));
        cx.simulate_click(
            disclosure.center() + gpui::point(px(100.0), px(0.0)),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        assert_eq!(preview.read_with(cx, |preview, _| preview.activations), 1);
    }
}
