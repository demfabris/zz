pub mod appearance;

use crate::Colorize as _;
use crate::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    control_shadow,
    scroll::ScrollableElement as _,
    select::SelectItem,
    tag::Tag,
    widget::icon::Icon,
};
use gpui::{
    AnyElement, App, ElementId, IntoElement, ListAlignment, ListSizingBehavior, ListState,
    ParentElement, RenderOnce, SharedString, Styled as _, Window, div, list, prelude::*, px,
    relative,
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
                "Edit the tmux-compatible configuration used for multiplexer behavior and key bindings."
            }
            Self::Hosts => "Manage the ssh machines in the fleet.",
            Self::Advanced => {
                "Control daemon lifecycle, diagnostics, and experimental pane features."
            }
            Self::About => "tmux, ghostty and gpui walked into a mux.",
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

/// Label above a settings navigation group.
pub fn settings_navigation_group_label(group: SettingsNavigationGroup, cx: &App) -> gpui::Div {
    div()
        .px(px(8.0))
        .pt(px(12.0))
        .pb(px(2.0))
        .text_sm()
        .text_color(cx.theme().foreground.muted())
        .map(crate::StyledExt::font_medium)
        .child(group.title())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext, VisualTestContext};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

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
                            .h(px(50.0))
                            .flex_none()
                            .debug_selector(move || format!("virtual-settings-row-{index}"))
                            .into_any_element()
                    },
                ))
        }
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

    #[gpui::test]
    fn virtual_column_only_constructs_rows_near_the_viewport(cx: &mut TestAppContext) {
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
        assert!(
            rendered.load(Ordering::Relaxed) < 20,
            "a short viewport must not construct the full settings page"
        );
    }

    #[gpui::test]
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
pub fn settings_page_content() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .max_w(px(SETTINGS_CONTENT_MAX_WIDTH))
        .mx_auto()
}

pub fn settings_page_description(section: SettingsSection, cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(4.0))
        .child(
            crate::StyledExt::font_medium(div().text_size(crate::rems_from_px(20.0)))
                .child(section.title()),
        )
        .child(
            div()
                .text_size(crate::rems_from_px(11.0))
                .text_color(cx.theme().foreground.muted())
                .child(section.description()),
        )
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

impl gpui::ParentElement for SettingsScrollColumn {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SettingsScrollColumn {
    fn render(self, window: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        let handle = window
            .use_keyed_state(gpui::ElementId::Name(self.id.into()), cx, |_, _| {
                gpui::ScrollHandle::default()
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
                    ),
            )
            .vertical_scrollbar(&handle)
    }
}

const SETTINGS_LIST_ITEM_HEIGHT_HINT: f32 = 82.0;
const SETTINGS_LIST_OVERDRAW: f32 = 24.0;

type SettingsItemRenderer = Box<dyn FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static>;

/// A settings page that only constructs rows in or around the viewport. Rows
/// keep [`settings_scroll_column`]'s bounded content width, but each owns the
/// space beneath it, so a glued run of entries stays glued.
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
    }
}

#[derive(IntoElement)]
pub struct SettingsVirtualColumn {
    id: &'static str,
    item_count: usize,
    render_item: SettingsItemRenderer,
}

impl RenderOnce for SettingsVirtualColumn {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state_key = ElementId::Name(format!("{}-list-state", self.id).into());
        let list_state = window
            .use_keyed_state(state_key, cx, |_, _| {
                ListState::new(
                    self.item_count,
                    ListAlignment::Top,
                    px(SETTINGS_LIST_OVERDRAW),
                )
                .with_uniform_item_height(px(SETTINGS_LIST_ITEM_HEIGHT_HINT))
            })
            .read(cx)
            .clone();
        if list_state.item_count() != self.item_count {
            list_state
                .reset_with_uniform_height(self.item_count, px(SETTINGS_LIST_ITEM_HEIGHT_HINT));
        }

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
        .py(px(SETTINGS_PAGE_PADDING));

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
) -> gpui::Div {
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

/// Group heading used as a standalone row in [`settings_virtual_column`].
pub fn settings_list_group_header(
    title: &'static str,
    description: Option<&'static str>,
    cx: &App,
) -> gpui::Div {
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
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
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
}

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
        }
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
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        let disabled = self.disabled;
        let position = self.position;

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
            .child(
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
                    .when_some(self.control, gpui::ParentElement::child),
            )
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
                        .bg(cx.theme().border),
                )
            })
            .child(body);

        div()
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
            .child(surface)
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
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
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
                    .when_some(self.title_actions, gpui::ParentElement::child),
            )
            .child(
                div()
                    .text_size(crate::rems_from_px(11.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(self.description),
            )
    }
}

/// Fill for a value control mounted on a settings card or mux row. Those
/// surfaces are already `raised(1)`, so a control at that default would
/// dissolve into the card behind it.
#[must_use]
pub fn settings_control_fill(cx: &App) -> gpui::Hsla {
    cx.theme().background.raised(2)
}

pub fn settings_provenance_badge(label: impl Into<SharedString>) -> Tag {
    Tag::secondary().small().outline().child(label.into())
}

pub fn settings_reset_button(
    id: impl Into<gpui::ElementId>,
    tooltip: impl Into<SharedString>,
    enabled: bool,
) -> Button {
    Button::new(id)
        .xsmall()
        .compact()
        .ghost()
        .flat()
        .icon(crate::IconName::Undo2)
        .tooltip(tooltip)
        .disabled(!enabled)
}

#[derive(Clone)]
pub struct SettingsSelectItem {
    title: SharedString,
    value: String,
}

impl SettingsSelectItem {
    pub fn new(title: impl Into<SharedString>, value: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            value: value.into(),
        }
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
}

pub fn panes_page(
    gaps: SettingEntry,
    opacity: SettingEntry,
    margin: SettingEntry,
    radius: SettingEntry,
    border: SettingEntry,
    cx: &App,
) -> SettingsScrollColumn {
    settings_scroll_column("settings-panes")
        .child(settings_page_description(SettingsSection::Panes, cx))
        .child(SettingsStack::titled("Layout").child(gaps))
        .child(SettingsStack::titled("Focus").child(opacity))
        .child(
            SettingsStack::titled("Frame")
                .description("Applies only while pane gaps are enabled.")
                .child(margin)
                .child(radius)
                .child(border),
        )
}
