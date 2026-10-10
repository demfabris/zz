pub mod about;
pub mod appearance;
pub mod panes_preview;
pub mod status_bar_preview;

use std::rc::Rc;

use crate::Colorize as _;
use crate::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    control_shadow,
    icon::Icon,
    scroll::ScrollableElement as _,
    select::SelectItem,
    tag::Tag,
};
use zz_gpui::{
    AnyElement, App, Bounds, ElementId, FocusHandle, IntoElement, ListAlignment,
    ListSizingBehavior, ListState, ParentElement, Pixels, RenderOnce, ScrollHandle, SharedString,
    Styled as _, Window, div, list, prelude::*, px, relative,
};

/// A page in the settings sidebar, ordered by the labeled groups the sidebar
/// shows: Appearance, Tools, Advanced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    StatusBar,
    Browser,
    Terminal,
    Editor,
    Panes,
    Hosts,
    Advanced,
    Multiplexer,
    About,
}

/// A labeled group in the settings sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsNavigationGroup {
    Appearance,
    Tools,
    Advanced,
}

impl SettingsNavigationGroup {
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Tools => "Tools",
            Self::Advanced => "Advanced",
        }
    }
}

impl SettingsSection {
    pub const ALL: [Self; 10] = [
        Self::Appearance,
        Self::StatusBar,
        Self::Editor,
        Self::Panes,
        Self::Multiplexer,
        Self::Browser,
        Self::Terminal,
        Self::Hosts,
        Self::Advanced,
        Self::About,
    ];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Interface",
            Self::StatusBar => "Status bar",
            Self::Browser => "Browser",
            Self::Terminal => "Terminal",
            Self::Editor => "Editor",
            Self::Panes => "Panes",
            Self::Multiplexer => "Multiplexer",
            Self::Hosts => "Hosts",
            Self::Advanced => "System",
            Self::About => "About",
        }
    }

    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Appearance => "Customize the app theme, chrome colors, icon, and visual details.",
            Self::StatusBar => {
                "Choose what appears in the title bar when the sidebar is retracted."
            }
            Self::Browser => "Configure browser-specific controls and shortcuts.",
            Self::Terminal => {
                "Edit the Ghostty-compatible configuration for terminal fonts, colors, cursor, \
                 and spacing."
            }
            Self::Editor => "Set the typography and editing behavior used by editor panes.",
            Self::Panes => "Tune pane spacing, borders, corners, and shadows across the workspace.",
            Self::Multiplexer => {
                "zz loads only zz/mux.conf. Import a tmux file here, or edit the options and bindings below."
            }
            Self::Hosts => "Manage the ssh machines in the fleet.",
            Self::Advanced => "Control daemon lifecycle and experimental pane features.",
            Self::About => "tmux, ghostty and zz_gpui walked into a mux.",
        }
    }

    #[must_use]
    pub const fn icon(self) -> crate::IconName {
        match self {
            Self::Appearance => crate::IconName::Palette,
            Self::StatusBar => crate::IconName::PanelBottom,
            Self::Browser => crate::IconName::Globe,
            Self::Terminal => crate::IconName::SquareTerminal,
            Self::Editor => crate::IconName::File,
            Self::Panes => crate::IconName::LayoutDashboard,
            Self::Multiplexer => crate::IconName::GalleryVerticalEnd,
            Self::Hosts => crate::IconName::HardDrive,
            Self::Advanced => crate::IconName::Cpu,
            Self::About => crate::IconName::Info,
        }
    }

    #[must_use]
    pub const fn navigation_group(self) -> SettingsNavigationGroup {
        match self {
            Self::Appearance | Self::StatusBar | Self::Editor | Self::Panes => {
                SettingsNavigationGroup::Appearance
            }
            Self::Multiplexer | Self::Browser | Self::Terminal => SettingsNavigationGroup::Tools,
            Self::Hosts | Self::Advanced | Self::About => SettingsNavigationGroup::Advanced,
        }
    }
}

/// A settings-section navigation button. The caller attaches behavior. Its
/// fills are [`crate::navigation::workspace_tree_row`]'s, so the two sidebars
/// highlight identically.
pub fn settings_navigation_button(section: SettingsSection, selected: bool, _: &App) -> Button {
    Button::new(section.title())
        .w_full()
        .px(px(8.0))
        .small()
        .ghost()
        .icon(section.icon())
        .selected(selected)
        .label(section.title())
        .child(div().flex_1())
        .when(selected, crate::StyledExt::font_medium)
}

/// The row that leaves the settings route, above the section list.
pub fn settings_navigation_back_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .w_full()
        .px(px(8.0))
        .small()
        .ghost()
        .icon(crate::IconName::ArrowLeft)
        .label("Back")
        .child(div().flex_1())
}

/// Holds the back button at the workspace tree's row height.
pub fn settings_navigation_back_row(button: Button) -> zz_gpui::Div {
    div()
        .flex()
        .flex_none()
        .h(px(crate::navigation::WORKSPACE_TREE_ROW_HEIGHT))
        .items_center()
        .child(button)
}

/// Label above a settings navigation group.
pub fn settings_navigation_group_label(group: SettingsNavigationGroup, cx: &App) -> zz_gpui::Div {
    div()
        .px(px(8.0))
        .pt(px(12.0))
        .pb(px(2.0))
        .text_sm()
        .text_color(cx.theme().foreground.muted())
        .map(crate::StyledExt::font_medium)
        .child(group.title())
}

const SECTION_ROW_HEIGHT: f32 = 48.0;

/// The settings root on a narrow screen: the sidebar's labeled groups at
/// touch size. `meta` puts a short value at the end of a row, and tapping a
/// row hands its section to `on_pick`.
pub fn settings_section_index(
    sections: &[SettingsSection],
    meta: impl Fn(SettingsSection) -> Option<SharedString>,
    on_pick: impl Fn(SettingsSection, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> SettingsScrollColumn {
    let on_pick = Rc::new(on_pick);
    let mut groups: Vec<(SettingsNavigationGroup, Vec<SettingsSection>)> = Vec::new();
    for &section in sections {
        let group = section.navigation_group();
        match groups.last_mut() {
            Some((last, members)) if *last == group => members.push(section),
            _ => groups.push((group, vec![section])),
        }
    }
    let muted = cx.theme().foreground.muted();
    let dim = cx.theme().foreground.opacity(0.3);
    let radius = cx.theme().radius;
    let wash = cx.theme().foreground.opacity(0.08);
    let mut column = settings_scroll_column("settings-index");
    for (group, members) in groups {
        let rows = members
            .into_iter()
            .map(|section| {
                let id = ElementId::Name(format!("settings-index-{}", section.title()).into());
                let on_pick = Rc::clone(&on_pick);
                let press = crate::touch::press_feedback(id.clone(), window, cx);
                let pressed = press.amount;
                div()
                    .id(id)
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .h(px(SECTION_ROW_HEIGHT))
                    .px(px(10.0))
                    .rounded(radius)
                    .cursor_pointer()
                    .when(pressed > 0.0, |row| {
                        row.bg(wash.opacity(wash.a * pressed))
                            .border(px(0.5))
                            .border_color(cx.theme().foreground.opacity(0.1 * pressed))
                    })
                    .child(Icon::new(section.icon()).small().text_color(muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(crate::rems_from_px(13.0))
                            .child(section.title()),
                    )
                    .children(meta(section).map(|value| {
                        div()
                            .text_size(crate::rems_from_px(11.0))
                            .text_color(muted)
                            .child(value)
                    }))
                    .child(
                        Icon::new(crate::IconName::ChevronRight)
                            .size(px(14.0))
                            .text_color(dim),
                    )
                    .on_click(move |_, window, cx| on_pick(section, window, cx))
                    .child(press.listener())
            })
            .collect::<Vec<_>>();
        column = column.child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(6.0))
                        .pb(px(6.0))
                        .text_size(crate::rems_from_px(12.0))
                        .text_color(muted)
                        .map(crate::StyledExt::font_medium)
                        .child(group.title()),
                )
                .children(rows),
        );
    }
    column
}

#[cfg(test)]
mod scroll_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use zz_gpui::{Context, Render, TestAppContext, VisualTestContext};

    struct VirtualSettingsTest {
        rendered: Arc<AtomicUsize>,
    }

    impl Render for VirtualSettingsTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let rendered = Arc::clone(&self.rendered);
            div()
                .flex()
                .w(px(400.0))
                .h(px(220.0))
                .child(settings_virtual_column(
                    "virtual-settings-test",
                    100,
                    move |index, _, _| {
                        rendered.fetch_add(1, Ordering::Relaxed);
                        div()
                            .h(px(120.0))
                            .flex_none()
                            .debug_selector(move || format!("virtual-settings-row-{index}"))
                            .into_any_element()
                    },
                ))
        }
    }

    struct FocusedRowTest {
        focus: FocusHandle,
    }

    impl Render for FocusedRowTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let focus = self.focus.clone();
            div().flex().w(px(400.0)).h(px(220.0)).child(
                settings_virtual_column("focused-row-test", 100, move |index, _, _| {
                    div()
                        .h(px(120.0))
                        .flex_none()
                        .when(index == 99, |row| row.track_focus(&focus))
                        .debug_selector(move || format!("focused-row-{index}"))
                        .into_any_element()
                })
                .row_focus((0..100).map(|index| (index == 99).then(|| self.focus.clone()))),
            )
        }
    }

    struct RevealTest {
        input: zz_gpui::Entity<crate::input::InputState>,
        virtual_rows: bool,
    }

    impl Render for RevealTest {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let input = self.input.clone();
            let focus = zz_gpui::Focusable::focus_handle(self.input.read(cx), cx);
            let row = move |index: usize| {
                if index == 8 {
                    div()
                        .flex_none()
                        .debug_selector(|| "reveal-field".to_owned())
                        .child(crate::input::Input::new(&input))
                } else {
                    div().h(px(50.0)).flex_none()
                }
            };
            let page = if self.virtual_rows {
                settings_virtual_column("reveal-virtual", 10, move |index, _, _| {
                    row(index).into_any_element()
                })
                .row_focus((0..10).map(|index| (index == 8).then(|| focus.clone())))
                .into_any_element()
            } else {
                settings_scroll_column("reveal-scroll")
                    .children((0..10).map(row))
                    .into_any_element()
            };
            div().flex().w(px(400.0)).h(px(220.0)).child(page)
        }
    }

    fn reveal_field(cx: &mut TestAppContext, virtual_rows: bool) -> Bounds<Pixels> {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|window, cx| RevealTest {
            input: cx.new(|cx| crate::input::InputState::new(window, cx)),
            virtual_rows,
        });
        let cx: &mut VisualTestContext = cx;
        cx.update(|window, cx| {
            window.set_a11y_forced(true);
            _ = window.draw(cx);
        });
        let hidden = cx.debug_bounds("reveal-field");
        assert!(hidden.is_none_or(|field| field.top() >= px(220.0)));
        cx.update(|window, cx| {
            let focus = zz_gpui::Focusable::focus_handle(view.read(cx).input.read(cx), cx);
            focus.focus(window, cx);
        });
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                _ = window.draw(cx);
            });
        }
        cx.debug_bounds("reveal-field")
            .expect("the focused field paints")
    }

    #[zz_gpui::test]
    fn a_focused_field_scrolls_into_a_list_page(cx: &mut TestAppContext) {
        let field = reveal_field(cx, true);
        assert!(field.top() >= px(0.0) && field.bottom() <= px(220.0));
    }

    #[zz_gpui::test]
    fn a_focused_field_scrolls_into_a_scroll_page(cx: &mut TestAppContext) {
        let field = reveal_field(cx, false);
        assert!(field.top() >= px(0.0) && field.bottom() <= px(220.0));
    }

    struct SettingsColumnGutterTest;

    impl Render for SettingsColumnGutterTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .w(px(400.0))
                .h(px(440.0))
                .child(
                    div().flex().w(px(400.0)).h(px(220.0)).child(
                        settings_scroll_column("scroll-gutter-test").child(
                            div()
                                .h(px(50.0))
                                .flex_none()
                                .debug_selector(|| "scroll-gutter-row".to_owned()),
                        ),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .w(px(400.0))
                        .h(px(220.0))
                        .child(settings_virtual_column(
                            "virtual-gutter-test",
                            1,
                            |_, _, _| {
                                div()
                                    .h(px(50.0))
                                    .flex_none()
                                    .debug_selector(|| "virtual-gutter-row".to_owned())
                                    .into_any_element()
                            },
                        )),
                )
        }
    }

    #[test]
    fn stack_positions_round_and_rule_the_ends_of_a_run() {
        assert_eq!(StackPosition::at(0, 0), StackPosition::Only);
        assert_eq!(StackPosition::at(0, 2), StackPosition::First);
        assert_eq!(StackPosition::at(1, 2), StackPosition::Middle);
        assert_eq!(StackPosition::at(2, 2), StackPosition::Last);

        for last in 0..4 {
            let run: Vec<_> = (0..=last)
                .map(|index| StackPosition::at(index, last))
                .collect();
            assert_eq!(run.iter().filter(|p| p.rounds_top()).count(), 1);
            assert_eq!(run.iter().filter(|p| p.rounds_bottom()).count(), 1);
            assert_eq!(run.iter().filter(|p| p.rules_above()).count(), last);
            assert!(run[0].rounds_top() && !run[0].rules_above());
            assert!(run[last].rounds_bottom());
        }
    }

    #[test]
    fn every_section_has_its_own_description() {
        for (index, section) in SettingsSection::ALL.into_iter().enumerate() {
            assert!(!section.description().is_empty());
            for other in &SettingsSection::ALL[index + 1..] {
                assert_ne!(section.description(), other.description());
            }
        }
    }

    #[test]
    fn settings_section_titles_match_the_sidebar_labels() {
        assert_eq!(
            SettingsSection::ALL.map(SettingsSection::title),
            [
                "Interface",
                "Status bar",
                "Editor",
                "Panes",
                "Multiplexer",
                "Browser",
                "Terminal",
                "Hosts",
                "System",
                "About",
            ]
        );
    }

    #[test]
    fn sections_follow_the_labeled_sidebar_groups() {
        assert_eq!(
            SettingsSection::ALL,
            [
                SettingsSection::Appearance,
                SettingsSection::StatusBar,
                SettingsSection::Editor,
                SettingsSection::Panes,
                SettingsSection::Multiplexer,
                SettingsSection::Browser,
                SettingsSection::Terminal,
                SettingsSection::Hosts,
                SettingsSection::Advanced,
                SettingsSection::About,
            ]
        );
        assert_eq!(
            SettingsSection::ALL.map(SettingsSection::navigation_group),
            [
                SettingsNavigationGroup::Appearance,
                SettingsNavigationGroup::Appearance,
                SettingsNavigationGroup::Appearance,
                SettingsNavigationGroup::Appearance,
                SettingsNavigationGroup::Tools,
                SettingsNavigationGroup::Tools,
                SettingsNavigationGroup::Tools,
                SettingsNavigationGroup::Advanced,
                SettingsNavigationGroup::Advanced,
                SettingsNavigationGroup::Advanced,
            ]
        );
    }

    struct PanesPageTest;

    impl Render for PanesPageTest {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let [gaps, background, opacity, glow, margin, radius, border] =
                std::array::from_fn::<_, 7, _>(|index| {
                    SettingEntry::new(format!("Setting {index}"), "Sample pane setting").control(
                        div()
                            .w(px(100.0))
                            .h(px(80.0))
                            .debug_selector(move || format!("panes-control-{index}")),
                    )
                });
            div().w(px(500.0)).h(px(600.0)).child(panes_page(
                panes_preview::PanesPreview {
                    gaps: true,
                    margin: 6.0,
                    radius: 13.5,
                    border_width: 0.5,
                    inactive_opacity: 0.7,
                },
                [gaps],
                background,
                [opacity, glow],
                [margin, radius, border],
                cx,
            ))
        }
    }

    #[zz_gpui::test]
    fn panes_preview_scrolls_with_controls(cx: &mut TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| cx.set_reduce_motion(true));
        let (_, cx) = cx.add_window_view(|_, _| PanesPageTest);
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let preview = cx.debug_bounds("settings-preview-terminal").unwrap();
        let control = cx.debug_bounds("panes-control-0").unwrap();
        cx.simulate_event(zz_gpui::ScrollWheelEvent {
            position: control.center(),
            delta: zz_gpui::ScrollDelta::Pixels(zz_gpui::point(px(0.0), px(-120.0))),
            ..Default::default()
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let preview_offset = cx
            .debug_bounds("settings-preview-terminal")
            .unwrap()
            .origin
            .y
            - preview.origin.y;
        let control_offset =
            cx.debug_bounds("panes-control-0").unwrap().origin.y - control.origin.y;
        assert!(preview_offset < px(0.0));
        assert_eq!(preview_offset, control_offset);
    }

    #[zz_gpui::test]
    fn one_long_scroll_reaches_the_last_row(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let rendered = Arc::new(AtomicUsize::new(0));
        let rendered_for_view = Arc::clone(&rendered);
        let (_, cx) = cx.add_window_view(move |_, _| VirtualSettingsTest {
            rendered: rendered_for_view,
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        assert!(cx.debug_bounds("virtual-settings-row-0").is_some());
        assert!(cx.debug_bounds("virtual-settings-row-99").is_none());

        cx.simulate_event(zz_gpui::ScrollWheelEvent {
            position: zz_gpui::point(px(100.0), px(100.0)),
            delta: zz_gpui::ScrollDelta::Pixels(zz_gpui::point(px(0.0), px(-100_000.0))),
            ..Default::default()
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert!(cx.debug_bounds("virtual-settings-row-99").is_some());
        assert!(cx.debug_bounds("virtual-settings-row-0").is_none());
    }

    #[zz_gpui::test]
    fn a_focused_row_keeps_painting_out_of_view(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, cx| FocusedRowTest {
            focus: cx.focus_handle(),
        });
        let cx: &mut VisualTestContext = cx;
        cx.update(|window, cx| {
            view.read(cx).focus.clone().focus(window, cx);
            _ = window.draw(cx);
        });

        assert!(cx.debug_bounds("focused-row-0").is_some());
        assert!(cx.debug_bounds("focused-row-99").is_some());
        assert!(cx.debug_bounds("focused-row-98").is_none());

        cx.update(|window, cx| {
            window.blur(cx);
            _ = window.draw(cx);
        });
        assert!(cx.debug_bounds("focused-row-99").is_none());
    }

    struct InsetPageTest;

    impl Render for InsetPageTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().w(px(400.0)).h(px(220.0)).child(
                settings_scroll_column("inset-test").children((0..10).map(|index| {
                    div()
                        .h(px(50.0))
                        .flex_none()
                        .debug_selector(move || format!("inset-row-{index}"))
                })),
            )
        }
    }

    #[zz_gpui::test]
    fn the_bottom_inset_keeps_the_last_row_clear(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_global(SettingsBottomInset(px(40.0)));
        });
        let (_, cx) = cx.add_window_view(|_, _| InsetPageTest);
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        cx.simulate_event(zz_gpui::ScrollWheelEvent {
            position: zz_gpui::point(px(100.0), px(100.0)),
            delta: zz_gpui::ScrollDelta::Pixels(zz_gpui::point(px(0.0), px(-10_000.0))),
            ..Default::default()
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let last = cx.debug_bounds("inset-row-9").expect("last row");
        assert!(last.bottom() <= px(220.0 - 40.0));
    }

    #[zz_gpui::test]
    fn virtual_rows_land_where_scrolled_rows_do(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| SettingsColumnGutterTest);
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let scrolled = cx
            .debug_bounds("scroll-gutter-row")
            .expect("the scrolled column renders its row");
        let virtualized = cx
            .debug_bounds("virtual-gutter-row")
            .expect("the virtual column renders its row");
        assert_eq!(scrolled.origin.x, virtualized.origin.x);
        assert_eq!(scrolled.size.width, virtualized.size.width);
    }
}

const SETTINGS_CONTENT_MAX_WIDTH: f32 = 960.0;
const SETTINGS_PAGE_PADDING: f32 = 14.0;

/// Centered, bounded content shared by every settings page.
pub fn settings_page_content() -> zz_gpui::Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .max_w(px(SETTINGS_CONTENT_MAX_WIDTH))
        .mx_auto()
}

pub fn settings_page_description(section: SettingsSection, cx: &App) -> zz_gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(4.0))
        .when(!crate::touch::CoarsePointer::get(cx), |this| {
            this.child(
                crate::StyledExt::font_medium(div().text_size(crate::rems_from_px(20.0)))
                    .child(section.title()),
            )
        })
        .child(
            div()
                .text_size(crate::rems_from_px(11.0))
                .text_color(cx.theme().foreground.muted())
                .child(section.description()),
        )
}

/// Room a host keeps clear below the last row of every settings page, such as
/// an iOS home indicator the page draws under. Unset means none.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SettingsBottomInset(pub Pixels);

impl zz_gpui::Global for SettingsBottomInset {}

impl SettingsBottomInset {
    #[must_use]
    pub fn get(cx: &App) -> Pixels {
        cx.try_global::<Self>().map_or(px(0.0), |inset| inset.0)
    }
}

/// A settings page: a scrolling column of [`SettingsStack`]s, with the shared
/// scrollbar overlaid. `id` keys the scroll handle, so each page keeps its own
/// position.
#[must_use]
pub fn settings_scroll_column(id: &'static str) -> SettingsScrollColumn {
    SettingsScrollColumn {
        id,
        children: Vec::new(),
    }
}

#[derive(IntoElement)]
pub struct SettingsScrollColumn {
    id: &'static str,
    children: Vec<AnyElement>,
}

impl zz_gpui::ParentElement for SettingsScrollColumn {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SettingsScrollColumn {
    fn render(self, window: &mut zz_gpui::Window, cx: &mut App) -> impl IntoElement {
        let handle = window
            .use_keyed_state(zz_gpui::ElementId::Name(self.id.into()), cx, |_, _| {
                zz_gpui::ScrollHandle::default()
            })
            .read(cx)
            .clone();
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .child(
                div()
                    .on_children_prepainted({
                        let handle = handle.clone();
                        move |_, window, _| {
                            if let Some(target) = window.take_autoscroll()
                                && reveal_in_scroll(&handle, target)
                            {
                                window.request_animation_frame();
                            }
                        }
                    })
                    .id(self.id)
                    .flex()
                    .flex_col()
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&handle)
                    .p(px(SETTINGS_PAGE_PADDING))
                    .child(
                        settings_page_content()
                            .flex_none()
                            .gap(px(18.0))
                            .children(self.children),
                    )
                    .child(div().flex_none().h(SettingsBottomInset::get(cx))),
            )
            .vertical_scrollbar(&handle)
    }
}

fn reveal_shift(viewport: Bounds<Pixels>, target: Bounds<Pixels>) -> Option<Pixels> {
    if target.right() <= viewport.left() || target.left() >= viewport.right() {
        return None;
    }
    if target.bottom() > viewport.bottom() {
        Some((target.bottom() - viewport.bottom()).min(target.top() - viewport.top()))
    } else if target.top() < viewport.top() {
        Some(target.top() - viewport.top())
    } else {
        None
    }
}

fn reveal_in_scroll(handle: &ScrollHandle, target: Bounds<Pixels>) -> bool {
    let Some(shift) = reveal_shift(handle.bounds(), target) else {
        return false;
    };
    let mut offset = handle.offset();
    let floor = -handle.max_offset().y.max(px(0.0));
    let next = (offset.y - shift).clamp(floor, px(0.0));
    if next == offset.y {
        return false;
    }
    offset.y = next;
    handle.set_offset(offset);
    true
}

const SETTINGS_LIST_OVERDRAW: f32 = 24.0;

type SettingsItemRenderer = Box<dyn FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static>;

/// A settings page that paints only rows in or around the viewport. Every row
/// is measured once per width, so a fling reaches the real end of the page.
/// Rows keep [`settings_scroll_column`]'s bounded content width, but each owns
/// the space beneath it, so a glued run of entries stays glued.
#[must_use]
pub fn settings_virtual_column(
    id: &'static str,
    item_count: usize,
    render_item: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
) -> SettingsVirtualColumn {
    SettingsVirtualColumn {
        id,
        item_count,
        render_item: Box::new(render_item),
        row_focus: Vec::new(),
    }
}

#[derive(IntoElement)]
pub struct SettingsVirtualColumn {
    id: &'static str,
    item_count: usize,
    render_item: SettingsItemRenderer,
    row_focus: Vec<Option<FocusHandle>>,
}

impl SettingsVirtualColumn {
    /// The focus handle of each row's text field. A row whose field holds focus
    /// keeps painting after it leaves the viewport, so scrolling or a soft
    /// keyboard covering it does not take its input away.
    #[must_use]
    pub fn row_focus(mut self, handles: impl IntoIterator<Item = Option<FocusHandle>>) -> Self {
        self.row_focus = handles.into_iter().collect();
        self
    }
}

impl RenderOnce for SettingsVirtualColumn {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state_key = ElementId::Name(format!("{}-list-state", self.id).into());
        let mut focus = self.row_focus;
        focus.resize(self.item_count, None);
        let list_state = window
            .use_keyed_state(state_key, cx, |_, _| {
                (
                    ListState::new(0, ListAlignment::Top, px(SETTINGS_LIST_OVERDRAW)).measure_all(),
                    Vec::new(),
                )
            })
            .update(cx, |(list_state, applied), _| {
                if *applied != focus {
                    list_state.reset(0);
                    list_state.splice_focusable(0..0, focus.iter().cloned());
                    *applied = focus;
                }
                list_state.clone()
            });

        let mut render_item = self.render_item;
        let rows = list(list_state.clone(), move |index, window, cx| {
            div()
                .flex()
                .w_full()
                .px(px(SETTINGS_PAGE_PADDING))
                .child(
                    settings_page_content()
                        .flex_none()
                        .child(render_item(index, window, cx)),
                )
                .into_any_element()
        })
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .size_full()
        .pt(px(SETTINGS_PAGE_PADDING))
        .pb(px(SETTINGS_PAGE_PADDING) + SettingsBottomInset::get(cx));

        div()
            .id(self.id)
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .child(rows)
            .vertical_scrollbar(&list_state)
    }
}

fn settings_group_header(
    title: SharedString,
    description: Option<SharedString>,
    cx: &App,
) -> zz_gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .px(px(2.0))
        .child(
            crate::StyledExt::font_medium(div().text_size(crate::rems_from_px(12.0))).child(title),
        )
        .when_some(description, |this, description| {
            this.child(
                div()
                    .text_size(crate::rems_from_px(11.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(description),
            )
        })
}

/// A group heading that shows or hides the rows after it, as a standalone
/// row in [`settings_virtual_column`].
pub fn settings_list_disclosure_header(
    id: &'static str,
    title: &'static str,
    description: Option<&'static str>,
    expanded: bool,
    cx: &App,
) -> zz_gpui::Stateful<zz_gpui::Div> {
    let muted = cx.theme().foreground.muted();
    div()
        .id(id)
        .pt(px(10.0))
        .flex()
        .items_start()
        .gap(px(2.0))
        .cursor_pointer()
        .child(
            Icon::new(if expanded {
                crate::IconName::ChevronDown
            } else {
                crate::IconName::ChevronRight
            })
            .size(px(14.0))
            .text_color(muted),
        )
        .child(settings_group_header(
            title.into(),
            description.map(Into::into),
            cx,
        ))
}

/// Group heading used as a standalone row in [`settings_virtual_column`].
pub fn settings_list_group_header(
    title: &'static str,
    description: Option<&'static str>,
    cx: &App,
) -> zz_gpui::Div {
    div().pt(px(10.0)).child(settings_group_header(
        title.into(),
        description.map(Into::into),
        cx,
    ))
}

const SETTINGS_STACK_PADDING: f32 = 12.0;

/// A run of settings sharing one surface, divided by hairlines. Fill it with
/// [`SettingEntry`].
#[derive(IntoElement)]
pub struct SettingsStack {
    title: Option<SharedString>,
    description: Option<SharedString>,
    entries: Vec<SettingEntry>,
}

impl SettingsStack {
    /// An untitled stack, for a run that needs no heading of its own.
    #[must_use]
    pub fn new() -> Self {
        Self {
            title: None,
            description: None,
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn titled(title: impl Into<SharedString>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::new()
        }
    }

    /// A line of context under the title. No effect on an untitled stack.
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Append an entry. The stack assigns its [`StackPosition`] on render.
    #[must_use]
    pub fn child(mut self, entry: SettingEntry) -> Self {
        self.entries.push(entry);
        self
    }

    #[must_use]
    pub fn children(mut self, entries: impl IntoIterator<Item = SettingEntry>) -> Self {
        self.entries.extend(entries);
        self
    }
}

impl Default for SettingsStack {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for SettingsStack {
    fn render(self, _: &mut zz_gpui::Window, cx: &mut App) -> impl IntoElement {
        let last = self.entries.len().saturating_sub(1);
        let rows = self
            .entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| entry.position(StackPosition::at(index, last)));

        div()
            .flex()
            .flex_col()
            .w_full()
            .gap(px(8.0))
            .when_some(self.title, |this, title| {
                this.child(settings_group_header(title, self.description, cx))
            })
            .child(div().flex().flex_col().w_full().children(rows))
    }
}

/// Where a [`SettingEntry`] sits in its run: which corners it rounds, and
/// whether a rule separates it from the entry above. [`SettingsStack`] assigns
/// this; a virtualized page has to assign it by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StackPosition {
    /// The only entry in its run: rounds all four corners.
    Only,
    /// Rounds the top corners, and draws no rule above.
    First,
    #[default]
    Middle,
    /// Rounds the bottom corners.
    Last,
}

impl StackPosition {
    /// The position implied by which ends of the run an entry is on. This is
    /// the form a virtualized page needs, where a run is bounded by whatever is
    /// not an entry rather than by a comparable index.
    #[must_use]
    pub const fn new(is_first: bool, is_last: bool) -> Self {
        match (is_first, is_last) {
            (true, true) => Self::Only,
            (true, false) => Self::First,
            (false, true) => Self::Last,
            (false, false) => Self::Middle,
        }
    }

    /// The position of item `index` in a run whose last index is `last`.
    #[must_use]
    pub const fn at(index: usize, last: usize) -> Self {
        Self::new(index == 0, index == last)
    }

    /// Whether the run stops here. The entry rounds its bottom corners, and a
    /// virtualized page owes it the gap before whatever comes next.
    #[must_use]
    pub const fn ends_run(self) -> bool {
        matches!(self, Self::Only | Self::Last)
    }

    const fn rounds_top(self) -> bool {
        matches!(self, Self::Only | Self::First)
    }

    const fn rounds_bottom(self) -> bool {
        self.ends_run()
    }

    const fn rules_above(self) -> bool {
        matches!(self, Self::Middle | Self::Last)
    }
}

/// One setting inside a [`SettingsStack`]: copy on the left, its control
/// centered at the right edge, and room beneath for a full-width control. Each
/// entry draws its own share of the run's surface. See [`StackPosition`].
#[derive(IntoElement)]
pub struct SettingEntry {
    title: SharedString,
    description: SharedString,
    title_icon: Option<Icon>,
    title_actions: Option<AnyElement>,
    control: Option<AnyElement>,
    disabled: bool,
    position: StackPosition,
    children: Vec<AnyElement>,
    on_click: Option<(ElementId, EntryClick)>,
}

type EntryClick = Rc<dyn Fn(&zz_gpui::ClickEvent, &mut zz_gpui::Window, &mut App)>;

impl SettingEntry {
    pub fn new(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            title_icon: None,
            title_actions: None,
            control: None,
            disabled: false,
            position: StackPosition::Middle,
            children: Vec::new(),
            on_click: None,
        }
    }

    /// Make the whole row a button, for a row that opens something.
    #[must_use]
    pub fn on_click(
        mut self,
        id: impl Into<ElementId>,
        handler: impl Fn(&zz_gpui::ClickEvent, &mut zz_gpui::Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some((id.into(), Rc::new(handler)));
        self
    }

    /// A state glyph drawn before the title; see [`SettingCopy::title_icon`].
    #[must_use]
    pub fn title_icon(mut self, icon: impl Into<Icon>) -> Self {
        self.title_icon = Some(icon.into());
        self
    }

    /// Which end of its run the entry sits on. [`SettingsStack`] sets this;
    /// call it directly only on a virtualized page.
    #[must_use]
    pub fn position(mut self, position: StackPosition) -> Self {
        self.position = position;
        self
    }

    /// Reset and provenance widgets, which sit beside the title rather than
    /// with the control.
    #[must_use]
    pub fn title_actions(mut self, actions: impl IntoElement) -> Self {
        self.title_actions = Some(actions.into_any_element());
        self
    }

    /// **Must be bounded.** The control keeps its natural width while the copy
    /// column shrinks, so one that grows with its content squeezes the copy to
    /// a character per line. Give it a width, or a menu trigger.
    #[must_use]
    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    /// Dim the row and make it inert, for a setting the current configuration
    /// has no effect on.
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl ParentElement for SettingEntry {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SettingEntry {
    fn render(self, window: &mut zz_gpui::Window, cx: &mut App) -> impl IntoElement {
        let disabled = self.disabled;
        let position = self.position;
        let press = self
            .on_click
            .as_ref()
            .map(|(id, _)| crate::touch::press_feedback(id.clone(), window, cx));
        let pressed = press.as_ref().map_or(0.0, |press| press.amount);
        let coarse = crate::touch::CoarsePointer::get(cx);
        let heading = if coarse {
            let description = (!self.description.is_empty()).then(|| {
                div()
                    .text_size(crate::rems_from_px(11.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(self.description.clone())
            });
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(16.0))
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                .gap(px(4.0))
                                .when_some(self.title_icon, |this, icon| {
                                    this.child(icon.with_size(crate::Size::Small))
                                })
                                .child(div().text_size(crate::rems_from_px(13.0)).child(self.title))
                                .when_some(self.title_actions, zz_gpui::ParentElement::child),
                        )
                        .when_some(self.control, zz_gpui::ParentElement::child),
                )
                .children(description)
        } else {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.0))
                .child(
                    SettingCopy::new(self.title, self.description)
                        .when_some(self.title_icon, SettingCopy::title_icon)
                        .when_some(self.title_actions, SettingCopy::title_actions),
                )
                .when_some(self.control, zz_gpui::ParentElement::child)
        };

        let body = div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .gap(px(10.0))
            .px(px(SETTINGS_STACK_PADDING))
            .py(px(11.0))
            .when(disabled, |this| {
                this.opacity(0.5)
                    .child(div().absolute().inset_0().occlude())
            })
            .when(pressed > 0.0, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .when(position.rounds_top(), |this| {
                            this.rounded_t(cx.theme().radius)
                        })
                        .when(position.rounds_bottom(), |this| {
                            this.rounded_b(cx.theme().radius)
                        })
                        .bg(cx.theme().foreground.opacity(0.1 * pressed)),
                )
            })
            .child(heading)
            .children(self.children);

        let surface = div()
            .flex()
            .flex_col()
            .w_full()
            .bg(cx.theme().background.raised(1).opaque())
            .border_color(cx.theme().foreground.opacity(0.1))
            .border_l(px(0.5))
            .border_r(px(0.5))
            .when(position.rounds_top(), |this| {
                this.border_t(px(0.5)).rounded_t(cx.theme().radius)
            })
            .when(position.rounds_bottom(), |this| {
                this.border_b(px(0.5)).rounded_b(cx.theme().radius)
            })
            .when(position.rules_above(), |this| {
                this.child(
                    div()
                        .flex_none()
                        .h(px(1.0))
                        .mx(px(SETTINGS_STACK_PADDING))
                        .bg(cx.theme().border()),
                )
            })
            .child(body);

        let row = div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .when(cx.theme().shadow, |this| {
                let extent = px(8.0);
                this.child(
                    div()
                        .absolute()
                        .left(-extent)
                        .right(-extent)
                        .top(if position.rounds_top() {
                            -extent
                        } else {
                            px(0.0)
                        })
                        .bottom(if position.rounds_bottom() {
                            -extent
                        } else {
                            px(0.0)
                        })
                        .overflow_hidden()
                        .child(
                            div()
                                .absolute()
                                .left(extent)
                                .right(extent)
                                .top(if position.rounds_top() {
                                    extent
                                } else {
                                    -extent
                                })
                                .bottom(if position.rounds_bottom() {
                                    extent
                                } else {
                                    -extent
                                })
                                .when(position.rounds_top(), |this| {
                                    this.rounded_t(cx.theme().radius)
                                })
                                .when(position.rounds_bottom(), |this| {
                                    this.rounded_b(cx.theme().radius)
                                })
                                .shadow(control_shadow(cx)),
                        ),
                )
            })
            .child(surface);
        match self.on_click {
            Some((id, handler)) => row
                .id(id)
                .cursor_pointer()
                .on_click(move |event, window, cx| handler(event, window, cx))
                .children(press.map(crate::touch::PressFeedback::listener))
                .into_any_element(),
            None => row.into_any_element(),
        }
    }
}

#[derive(IntoElement)]
pub struct SettingCopy {
    title: SharedString,
    description: SharedString,
    title_icon: Option<Icon>,
    title_actions: Option<AnyElement>,
}

impl SettingCopy {
    pub fn new(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            title_icon: None,
            title_actions: None,
        }
    }

    /// A state glyph drawn before the title, sized to the title's line. Color
    /// it before passing.
    #[must_use]
    pub fn title_icon(mut self, icon: impl Into<Icon>) -> Self {
        self.title_icon = Some(icon.into());
        self
    }

    #[must_use]
    pub fn title_actions(mut self, actions: impl IntoElement) -> Self {
        self.title_actions = Some(actions.into_any_element());
        self
    }
}

impl RenderOnce for SettingCopy {
    fn render(self, _: &mut zz_gpui::Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w_full()
            .max_w(relative(0.7))
            .min_w_0()
            .gap(px(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .when_some(self.title_icon, |this, icon| {
                        this.child(icon.with_size(crate::Size::Small))
                    })
                    .child(div().text_size(crate::rems_from_px(13.0)).child(self.title))
                    .when_some(self.title_actions, zz_gpui::ParentElement::child),
            )
            .when(!self.description.is_empty(), |this| {
                this.child(
                    div()
                        .text_size(crate::rems_from_px(11.0))
                        .text_color(cx.theme().foreground.muted())
                        .child(self.description),
                )
            })
    }
}

/// Fill for a value control mounted on a settings card or mux row. Those
/// surfaces are already `raised(1)`, so a control at that default would
/// dissolve into the card behind it.
#[must_use]
pub fn settings_control_fill(cx: &App) -> zz_gpui::Hsla {
    cx.theme().background.raised(2)
}

pub fn settings_provenance_badge(label: impl Into<SharedString>) -> Tag {
    Tag::secondary().small().outline().child(label.into())
}

pub fn settings_reset_button(
    id: impl Into<zz_gpui::ElementId>,
    tooltip: impl Into<SharedString>,
    enabled: bool,
) -> Button {
    let slop = crate::touch::control_slop(crate::Size::XSmall);
    Button::new(id)
        .xsmall()
        .compact()
        .ghost()
        .flat()
        .hit_slop(slop, slop)
        .icon(crate::IconName::Undo2)
        .tooltip(tooltip)
        .disabled(!enabled)
}

#[derive(Clone)]
pub struct SettingsSelectItem {
    title: SharedString,
    value: String,
    font: bool,
}

impl SettingsSelectItem {
    pub fn new(title: impl Into<SharedString>, value: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            value: value.into(),
            font: false,
        }
    }

    /// Draw the row in the font family its value names.
    #[must_use]
    pub fn preview_font(mut self) -> Self {
        self.font = true;
        self
    }
}

impl SelectItem for SettingsSelectItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }

    fn font_family(&self) -> Option<SharedString> {
        self.font.then(|| self.value.clone().into())
    }
}

pub fn panes_page(
    preview: panes_preview::PanesPreview,
    layout: impl IntoIterator<Item = SettingEntry>,
    background_opacity: SettingEntry,
    focus: [SettingEntry; 2],
    frame: [SettingEntry; 3],
    cx: &App,
) -> zz_gpui::Div {
    div()
        .flex()
        .flex_col()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .child(
            settings_scroll_column("settings-panes")
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(settings_page_description(SettingsSection::Panes, cx))
                        .child(settings_group_header(
                            "Preview".into(),
                            Some("Click a pane to preview focus.".into()),
                            cx,
                        ))
                        .child(preview),
                )
                .child(SettingsStack::titled("Layout").children(layout))
                .child(SettingsStack::titled("Appearance").child(background_opacity))
                .child(SettingsStack::titled("Focus").children(focus))
                .child(
                    SettingsStack::titled("Frame")
                        .description("Applies only while pane gaps are enabled.")
                        .children(frame),
                ),
        )
}
