use crate::{
    ActiveTheme as _, Colorize as _, Icon, IconName, MACOS_TRAFFIC_LIGHT_INSET,
    MACOS_TRAFFIC_LIGHT_SPAN, Sizable as _, TITLE_BAR_HEIGHT, UiZoom,
    button::{Button, ButtonVariants as _},
    rems_from_px,
    tooltip::Tooltip,
};
use gpui::{
    AnyElement, App, ElementId, Hsla, IntoElement, ParentElement as _, Pixels, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, div, prelude::*, px,
};

pub const WORKSPACE_TREE_ROW_HEIGHT: f32 = 32.0;
pub const WORKSPACE_TREE_INDENT_WIDTH: f32 = 20.0;
pub const WORKSPACE_TREE_CONTENT_INSET: f32 = 8.0;
/// Extra right inset the tree-row action strip adds inside the row's content
/// inset. Anything else against that edge pads by the two combined.
pub const WORKSPACE_TREE_ACTION_INSET: f32 = 4.0;
pub const WORKSPACE_TREE_MARKER_SLOT_WIDTH: f32 = 18.0;
/// Icon size for a tree row's node marker. Matches `Size::Small` in `icon/mod.rs`.
pub const WORKSPACE_TREE_NODE_ICON_SIZE: f32 = 14.0;
pub const WORKSPACE_TREE_MARKER_LABEL_GAP: f32 = 6.0;
const WORKSPACE_TREE_FILL_INSET: f32 = 4.0;
pub const WORKSPACE_SIDEBAR_DEFAULT_WIDTH: f32 = 256.0;
/// Vertical padding around the sidebar's status section.
pub const WORKSPACE_SIDEBAR_STATUS_PADDING: f32 = 6.0;
pub const WORKSPACE_STRIP_GAP: f32 = 6.0;
const WORKSPACE_STRIP_CHIP_HEIGHT: f32 = 24.0;
const WORKSPACE_STRIP_CHIP_CONNECTOR_WIDTH: f32 = 8.0;
const WORKSPACE_STRIP_WINDOW_PILL_WIDTH: f32 = 104.0;
/// Leading inset the titlebar strip keeps clear on macOS for the traffic
/// lights: their own leading margin, the cluster, then that margin again, so
/// the lights sit in equal gaps between the window edge and the first control.
pub const WORKSPACE_STRIP_TRAFFIC_LIGHT_INSET: f32 =
    2.0 * MACOS_TRAFFIC_LIGHT_INSET + MACOS_TRAFFIC_LIGHT_SPAN;

/// Where a titlebar-height strip may start putting controls of its own: past
/// the macOS traffic lights, or at the plain content inset elsewhere. Both
/// chromes start their cluster here, so it holds its axis across a chrome flip.
#[must_use]
pub fn workspace_controls_leading_inset(cx: &App) -> Pixels {
    if cfg!(target_os = "macos") {
        UiZoom::unzoomed(px(WORKSPACE_STRIP_TRAFFIC_LIGHT_INSET), cx)
    } else {
        px(WORKSPACE_TREE_CONTENT_INSET)
    }
}

/// The workspace-layout menu trigger, in the sidebar's titlebar strip and again
/// in the strip that replaces the sidebar.
#[must_use]
pub fn workspace_layout_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .compact()
        .icon(IconName::PanelsTopLeft)
        .tooltip("Workspace layout")
}

/// The settings entry beside the workspace-layout menu, sized to match
/// [`workspace_layout_button`].
#[must_use]
pub fn sidebar_settings_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .compact()
        .icon(IconName::Settings)
        .tooltip("Settings")
}

/// The settings/layout pair, in either chrome. Both start it at
/// [`workspace_controls_leading_inset`], so it holds position across a flip.
#[must_use]
pub fn workspace_sidebar_controls(
    settings: impl IntoElement,
    toggle: impl IntoElement,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(settings)
        .child(toggle)
}

/// Titlebar-height strip at the top of the full-height workspace sidebar,
/// carrying `controls` at its leading end. No border and no background of its
/// own: the parent sidebar owns the shared surface.
#[must_use]
pub fn workspace_sidebar_titlebar(
    id: impl Into<ElementId>,
    controls: impl IntoElement,
    cx: &App,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .w_full()
        .h(TITLE_BAR_HEIGHT)
        .items_center()
        .pl(workspace_controls_leading_inset(cx))
        .child(controls)
}

/// Ink for the seam between the sidebar and the content column.
pub fn workspace_sidebar_divider(cx: &App) -> Hsla {
    cx.theme().border.raised(2)
}

/// The fill that says "this one" anywhere the workspace is navigated: a tree
/// row under the pointer, the keyboard cursor or the mux, and a strip chip in
/// the same three states. Washed, so a highlighted row keeps the desktop blur.
#[must_use]
pub fn workspace_row_highlight(cx: &App) -> Hsla {
    cx.theme().background.washed(2)
}

/// Full-height sidebar interior. Native callers attach window dragging to the
/// titlebar slot and wrap the surface with resize handling. `status` is the
/// optional bottom section a tmux status line renders into.
#[must_use]
pub fn workspace_sidebar_surface(
    id: impl Into<ElementId>,
    width: f32,
    titlebar: impl IntoElement,
    navigation: impl IntoElement,
    status: Option<AnyElement>,
    cx: &App,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .w(px(width))
        .h_full()
        .min_h_0()
        .flex_none()
        .overflow_hidden()
        .relative()
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
        .border_r_1()
        .border_color(workspace_sidebar_divider(cx))
        .child(titlebar)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .pb(px(8.0))
                .child(navigation),
        )
        .children(status)
}

/// The sidebar's bottom section, one line like the tmux status bar it stands
/// in for: the attention rollup and `left` at the left, `right` at the right.
/// Every slot may be empty, and `left` ellipsizes first.
#[must_use]
pub fn workspace_sidebar_status(
    id: impl Into<ElementId>,
    attention: Option<AnyElement>,
    left: SharedString,
    right: SharedString,
    cx: &App,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .w_full()
        .gap(px(6.0))
        .px(px(WORKSPACE_TREE_CONTENT_INSET))
        .py(px(WORKSPACE_SIDEBAR_STATUS_PADDING))
        .border_t_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .text_xs()
        .text_color(cx.theme().foreground.muted())
        .children(attention)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(left),
        )
        .child(div().flex_none().whitespace_nowrap().child(right))
}

/// One segment of the status section's attention rollup: a status-colored dot
/// and a count, e.g. "2 running". `clickable` adds the hover invitation.
#[must_use]
pub fn workspace_sidebar_attention(
    id: impl Into<ElementId>,
    label: SharedString,
    color: Hsla,
    clickable: bool,
    cx: &App,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.0))
        .whitespace_nowrap()
        .child(div().flex_none().size(px(6.0)).rounded_full().bg(color))
        .child(label)
        .when(clickable, |this| {
            this.cursor_pointer()
                .hover(|this| this.text_color(cx.theme().foreground))
        })
}

// Six independent row states, not a config object waiting to be extracted.
#[must_use]
#[allow(clippy::fn_params_excessive_bools)]
pub fn workspace_tree_row(
    id: impl Into<ElementId>,
    depth: u8,
    active: bool,
    selected: bool,
    focused: bool,
    connected: bool,
    clickable: bool,
    hover_actions: bool,
    row_group: SharedString,
    marker: impl IntoElement,
    label: impl IntoElement,
    actions: impl IntoElement,
    cx: &App,
) -> Stateful<gpui::Div> {
    let radius = cx.theme().radius;
    let fill_inset = if radius > px(0.) {
        px(WORKSPACE_TREE_FILL_INSET)
    } else {
        px(0.)
    };
    let fill_color = workspace_row_highlight(cx);
    let fill = div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(fill_inset)
        .right(fill_inset)
        .rounded(radius)
        .group_hover(row_group.clone(), |this| this.bg(fill_color))
        .when(selected || active && focused, |this| this.bg(fill_color));
    let trailing = div()
        .h_full()
        .flex()
        .flex_none()
        .items_center()
        .pl(px(6.0))
        .when(hover_actions, |this| {
            this.invisible()
                .group_hover(row_group.clone(), gpui::Styled::visible)
        })
        .child(actions);
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .h(px(WORKSPACE_TREE_ROW_HEIGHT))
        .flex_none()
        .relative()
        .flex()
        .items_center()
        .pl(px(
            WORKSPACE_TREE_CONTENT_INSET + f32::from(depth) * WORKSPACE_TREE_INDENT_WIDTH
        ))
        .pr(px(WORKSPACE_TREE_CONTENT_INSET))
        .text_sm()
        .group(row_group)
        .child(fill)
        .text_color(if connected {
            cx.theme().foreground
        } else {
            cx.theme().foreground.muted()
        })
        .when(active, crate::StyledExt::font_medium)
        .when(clickable, gpui::Styled::cursor_pointer)
        .child(marker)
        .child(
            div()
                .min_w_0()
                .flex_1()
                .ml(px(WORKSPACE_TREE_MARKER_LABEL_GAP))
                .flex()
                .items_center()
                .gap_2()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label),
        )
        .child(trailing)
}

#[must_use]
pub fn workspace_tree_marker(marker: impl IntoElement) -> gpui::Div {
    div()
        .h_full()
        .w(px(WORKSPACE_TREE_MARKER_SLOT_WIDTH))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .child(marker)
}

/// The expand/collapse affordance an expandable row hangs on its marker slot:
/// the node's own icon, swapped for a chevron while the row is hovered. The
/// caller wires the click and passes the [`workspace_tree_marker`] it built.
#[must_use]
pub fn workspace_tree_disclosure(
    marker: impl IntoElement,
    expanded: bool,
    row_group: SharedString,
    cx: &App,
) -> gpui::Div {
    let chevron = Icon::new(if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    })
    .size(rems_from_px(WORKSPACE_TREE_NODE_ICON_SIZE))
    .text_color(cx.theme().foreground.muted());
    div()
        .relative()
        .h_full()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(
            div()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .group_hover(row_group.clone(), gpui::Styled::invisible)
                .child(marker),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .invisible()
                .group_hover(row_group, gpui::Styled::visible)
                .child(chevron),
        )
}

/// Titlebar-height strip spanning the whole window, standing in for the
/// sidebar column. `leading` starts at [`workspace_controls_leading_inset`],
/// `content` clips, and `trailing` holds any window-control buttons.
#[must_use]
pub fn workspace_titlebar_strip(
    id: impl Into<ElementId>,
    leading: impl IntoElement,
    content: impl IntoElement,
    trailing: impl IntoElement,
    cx: &App,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .w_full()
        .h(TITLE_BAR_HEIGHT)
        .items_center()
        .gap(px(WORKSPACE_STRIP_GAP))
        .child(
            div()
                .flex_none()
                .flex()
                .h_full()
                .items_center()
                .child(
                    div()
                        .flex_none()
                        .h_full()
                        .w(workspace_controls_leading_inset(cx))
                        .window_control_area(gpui::WindowControlArea::Drag),
                )
                .child(leading),
        )
        .child(
            div()
                .flex_1()
                .h_full()
                .min_w_0()
                .flex()
                .items_center()
                .overflow_hidden()
                .window_control_area(gpui::WindowControlArea::Drag)
                .child(content),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .h_full()
                .items_center()
                .child(trailing),
        )
}

/// The rule between the strip's controls and the chips that name the fleet.
#[must_use]
pub fn workspace_strip_group_separator(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .w(px(1.0))
        .h(px(16.0))
        .bg(cx.theme().border)
}

/// The dash joining one strip chip to the next. It is the whole distance
/// between two chips, so the strip adds no gap of its own.
#[must_use]
pub fn workspace_strip_chip_connector(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .w(px(WORKSPACE_STRIP_CHIP_CONNECTOR_WIDTH))
        .h(px(1.0))
        .bg(cx.theme().border)
}

/// A session's one-letter badge in the titlebar strip. The attached session
/// takes [`workspace_row_highlight`] and full-strength text; parked ones sit
/// dimmed, as does every badge while the daemon is disconnected.
#[must_use]
pub fn workspace_strip_session_badge(
    id: impl Into<ElementId>,
    initial: SharedString,
    tooltip: impl Into<SharedString>,
    attached: bool,
    connected: bool,
    cx: &App,
) -> Stateful<gpui::Div> {
    let tooltip = tooltip.into();
    let rest = cx.theme().background.washed(1);
    let highlight = workspace_row_highlight(cx);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(WORKSPACE_STRIP_CHIP_HEIGHT))
        .min_w(px(WORKSPACE_STRIP_CHIP_HEIGHT))
        .px(px(6.0))
        .rounded(cx.theme().radius)
        .bg(if attached && connected {
            highlight
        } else {
            rest
        })
        .text_size(rems_from_px(11.0))
        .text_color(if attached && connected {
            cx.theme().foreground
        } else {
            cx.theme().foreground.muted()
        })
        .when(connected, |this| {
            this.cursor_pointer().hover(move |this| this.bg(highlight))
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(initial)
}

/// One window of the attached session in the titlebar strip: the same chip as
/// [`workspace_strip_session_badge`], with `label` ellipsized into the tooltip
/// at a fixed width and `delete` revealed on hover of `group`, unique per chip.
#[must_use]
pub fn workspace_strip_window_pill(
    id: impl Into<ElementId>,
    group: SharedString,
    label: SharedString,
    active: bool,
    connected: bool,
    delete: impl IntoElement,
    cx: &App,
) -> Stateful<gpui::Div> {
    let tooltip = label.clone();
    let rest = cx.theme().background.washed(1);
    let highlight = workspace_row_highlight(cx);
    div()
        .id(id)
        .group(group.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.0))
        .h(px(WORKSPACE_STRIP_CHIP_HEIGHT))
        .w(px(WORKSPACE_STRIP_WINDOW_PILL_WIDTH))
        .pl(px(10.0))
        .pr(px(4.0))
        .rounded(cx.theme().radius)
        .bg(if active && connected { highlight } else { rest })
        .text_size(rems_from_px(11.0))
        .text_color(if active && connected {
            cx.theme().foreground
        } else {
            cx.theme().foreground.muted()
        })
        .when(connected, |this| {
            this.cursor_pointer().hover(move |this| this.bg(highlight))
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label),
        )
        .child(
            div()
                .flex_none()
                .invisible()
                .group_hover(group, gpui::Styled::visible)
                .child(delete),
        )
}
