use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, MACOS_TRAFFIC_LIGHT_INSET,
    MACOS_TRAFFIC_LIGHT_SPAN, TITLE_BAR_HEIGHT, UiZoom,
    button::{Button, COMPACT_ICON_BUTTON_SIZE},
    rems_from_px,
    tooltip::Tooltip,
};
use gpui::{
    App, ElementId, Hsla, IntoElement, ParentElement as _, Pixels, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::*, px,
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
const WORKSPACE_TREE_FILL_VERTICAL_INSET: f32 = 1.0;
pub const WORKSPACE_SIDEBAR_DEFAULT_WIDTH: f32 = 256.0;
pub const WORKSPACE_CONTROL_TRAFFIC_LIGHT_INSET: f32 =
    2.0 * MACOS_TRAFFIC_LIGHT_INSET + MACOS_TRAFFIC_LIGHT_SPAN;
const WORKSPACE_CHROME_CONTROL_GAP: f32 = 4.0;
pub const WORKSPACE_STATUS_CONTENT_HEIGHT: Pixels = px(24.0);
const WORKSPACE_STATUS_LINE_HEIGHT: Pixels = px(16.0);
const WORKSPACE_STATUS_ITEM_MAX_WIDTH: Pixels = px(180.0);
const WORKSPACE_STATUS_WINDOW_MAX_WIDTH: Pixels = px(180.0);
const WORKSPACE_STATUS_WINDOW_MIN_WIDTH: Pixels = px(36.0);
const WORKSPACE_STATUS_ICON_SIZE: Pixels = px(13.0);
const WORKSPACE_STATUS_ICON_DROP: Pixels = px(0.5);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceStatusWindowState {
    pub connected: bool,
    pub active: bool,
    pub zoomed: bool,
    pub bell: bool,
}

#[must_use]
pub fn workspace_controls_leading_inset(cx: &App) -> Pixels {
    if cfg!(target_os = "macos") {
        UiZoom::unzoomed(px(WORKSPACE_CONTROL_TRAFFIC_LIGHT_INSET), cx)
    } else {
        px(WORKSPACE_TREE_CONTENT_INSET)
    }
}

#[must_use]
pub fn workspace_layout_button(id: impl Into<ElementId>) -> Button {
    workspace_chrome_button(id, IconName::PanelsTopLeft, "Toggle sidebar")
}

#[must_use]
pub fn workspace_settings_button(id: impl Into<ElementId>) -> Button {
    workspace_chrome_button(id, IconName::Settings, "Settings")
}

fn workspace_chrome_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: &'static str,
) -> Button {
    Button::compact_icon(id, icon).tooltip(tooltip)
}

#[must_use]
pub fn workspace_chrome_controls(
    settings: impl IntoElement,
    layout: Option<gpui::AnyElement>,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(settings)
        .children(layout)
}

#[must_use]
pub fn workspace_chrome_controls_width(has_layout: bool, window: &Window) -> Pixels {
    let width = if has_layout {
        2.0 * COMPACT_ICON_BUTTON_SIZE + WORKSPACE_CHROME_CONTROL_GAP
    } else {
        COMPACT_ICON_BUTTON_SIZE
    };
    rems_from_px(width).to_pixels(window.rem_size())
}

#[must_use]
pub fn workspace_status_item(
    id: impl Into<ElementId>,
    icon: Option<IconName>,
    text: SharedString,
    cx: &App,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_shrink_1()
        .min_w_0()
        .max_w(WORKSPACE_STATUS_ITEM_MAX_WIDTH)
        .h(WORKSPACE_STATUS_CONTENT_HEIGHT)
        .items_center()
        .gap(px(5.0))
        .overflow_hidden()
        .text_color(cx.theme().foreground.muted())
        .text_size(rems_from_px(12.0))
        .line_height(WORKSPACE_STATUS_LINE_HEIGHT)
        .children(icon.map(|icon| {
            div()
                .flex_none()
                .relative()
                .top(WORKSPACE_STATUS_ICON_DROP)
                .child(Icon::new(icon).size(WORKSPACE_STATUS_ICON_SIZE))
        }))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(text),
        )
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn workspace_status_window(
    id: impl Into<ElementId>,
    index: SharedString,
    name: SharedString,
    tooltip: SharedString,
    state: WorkspaceStatusWindowState,
    cx: &App,
) -> Stateful<gpui::Div> {
    let foreground = cx.theme().foreground;
    let highlight = workspace_row_highlight(cx);
    let text_color = if state.active {
        foreground
    } else {
        foreground.muted()
    };
    div()
        .id(id)
        .relative()
        .flex()
        .flex_shrink_1()
        .min_w(WORKSPACE_STATUS_WINDOW_MIN_WIDTH)
        .max_w(WORKSPACE_STATUS_WINDOW_MAX_WIDTH)
        .h(WORKSPACE_STATUS_CONTENT_HEIGHT)
        .items_center()
        .gap(px(5.0))
        .px(px(9.0))
        .rounded(cx.theme().radius)
        .overflow_hidden()
        .when(state.active, |item| item.bg(highlight))
        .text_color(text_color)
        .text_size(rems_from_px(13.0))
        .line_height(WORKSPACE_STATUS_LINE_HEIGHT)
        .when(state.connected, |item| {
            item.cursor_pointer()
                .hover(move |item| item.bg(highlight).text_color(foreground))
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(
            div()
                .flex_none()
                .text_size(rems_from_px(12.0))
                .text_color(foreground.muted())
                .child(index),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(name),
        )
        .children(state.zoomed.then(|| {
            div()
                .flex_none()
                .relative()
                .top(WORKSPACE_STATUS_ICON_DROP)
                .text_color(foreground.muted())
                .child(Icon::new(IconName::ZoomIn).size(WORKSPACE_STATUS_ICON_SIZE))
        }))
        .when(state.bell, |item| {
            item.child(
                div()
                    .absolute()
                    .top(px(4.0))
                    .right(px(4.0))
                    .size(px(5.0))
                    .rounded_full()
                    .bg(cx.theme().warning),
            )
        })
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

/// The fill that says "this one" in the workspace tree: a row under the
/// pointer, the keyboard cursor, or the mux.
#[must_use]
pub fn workspace_row_highlight(cx: &App) -> Hsla {
    cx.theme().background.washed(2)
}

/// Full-height sidebar interior. Native callers attach window dragging to the
/// titlebar slot and wrap the surface with resize handling.
#[must_use]
pub fn workspace_sidebar_surface(
    id: impl Into<ElementId>,
    width: f32,
    titlebar: impl IntoElement,
    navigation: impl IntoElement,
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
        .top(px(WORKSPACE_TREE_FILL_VERTICAL_INSET))
        .bottom(px(WORKSPACE_TREE_FILL_VERTICAL_INSET))
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
    workspace_tree_row_frame(id, depth)
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
        .child(workspace_tree_row_label(label))
        .child(trailing)
}

#[must_use]
pub fn workspace_tree_action_row(
    id: impl Into<ElementId>,
    depth: u8,
    icon: IconName,
    label: impl Into<SharedString>,
    cx: &App,
) -> Stateful<gpui::Div> {
    let foreground = cx.theme().foreground;
    workspace_tree_row_frame(id, depth)
        .text_color(foreground.muted())
        .cursor_pointer()
        .hover(move |row| row.text_color(foreground))
        .child(workspace_tree_marker(
            Icon::new(icon)
                .size(rems_from_px(WORKSPACE_TREE_NODE_ICON_SIZE))
                .text_color(foreground.muted()),
        ))
        .child(workspace_tree_row_label(div().child(label.into())))
}

#[must_use]
pub fn workspace_tree_action_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    disabled: bool,
    cx: &App,
) -> Button {
    Button::compact_icon(id, icon)
        .when(!disabled, |button| {
            button.text_color(cx.theme().foreground.muted())
        })
        .tooltip(tooltip)
        .disabled(disabled)
}

fn workspace_tree_row_frame(id: impl Into<ElementId>, depth: u8) -> Stateful<gpui::Div> {
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
}

fn workspace_tree_row_label(label: impl IntoElement) -> gpui::Div {
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
        .child(label)
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
    id: impl Into<ElementId>,
    marker: impl IntoElement,
    expanded: bool,
    row_group: SharedString,
    cx: &App,
) -> Stateful<gpui::Div> {
    let foreground = cx.theme().foreground;
    let chevron = Icon::new(if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    })
    .size(rems_from_px(WORKSPACE_TREE_NODE_ICON_SIZE));
    div()
        .id(id)
        .relative()
        .h_full()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(foreground.muted())
        .hover(move |this| this.text_color(foreground))
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
