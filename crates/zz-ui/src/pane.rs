use crate::{ActiveTheme as _, Colorize as _, surface_ring, tag::Tag};
use gpui::{
    AnyElement, App, BoxShadow, Corners, CursorStyle, ElementId, FontWeight, Hsla, IntoElement,
    ParentElement as _, Pixels, SharedString, Stateful, Styled as _, div, point, prelude::*, px,
    relative,
};

const PANE_DRAG_SOURCE_FADE: f32 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneChrome {
    pub radii: Corners<Pixels>,
    pub border_width: Pixels,
    pub border_color: Hsla,
    pub gap_background: Hsla,
    pub shadow: bool,
    pub inactive_opacity: f32,
}

impl PaneChrome {
    #[must_use]
    pub const fn new(
        radii: Corners<Pixels>,
        border_width: Pixels,
        border_color: Hsla,
        gap_background: Hsla,
        shadow: bool,
    ) -> Self {
        Self {
            radii,
            border_width,
            border_color,
            gap_background,
            shadow,
            inactive_opacity: 1.0,
        }
    }

    #[must_use]
    pub fn dimmed(mut self, dimmed: bool, opacity: f32) -> Self {
        self.inactive_opacity = if dimmed { opacity.clamp(0.0, 1.0) } else { 1.0 };
        self
    }
}

/// A pane leaf, filling the box the layout gives it. Rounded corners leave four
/// wedges of that box bare, which this fills with the gap plane before the
/// surface paints its border.
pub fn pane_surface(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    overlays: impl IntoIterator<Item = AnyElement>,
    chrome: PaneChrome,
    cx: &App,
) -> gpui::Div {
    div()
        .relative()
        .flex()
        .size_full()
        .children(pane_corner_notches(chrome))
        .child(
            div()
                .id(id)
                .relative()
                .flex()
                .size_full()
                .overflow_hidden()
                .rounded_tl(chrome.radii.top_left)
                .rounded_tr(chrome.radii.top_right)
                .rounded_bl(chrome.radii.bottom_left)
                .rounded_br(chrome.radii.bottom_right)
                .bg(cx.theme().background.opaque())
                .border(chrome.border_width)
                .border_color(chrome.border_color)
                .child(content)
                .when(chrome.inactive_opacity < 1.0, |surface| {
                    surface.child(pane_inactive_scrim(
                        chrome.radii,
                        chrome.inactive_opacity,
                        cx,
                    ))
                })
                .children(overlays),
        )
        .children(pane_surface_shadow(chrome, cx))
}

fn pane_surface_shadow(chrome: PaneChrome, cx: &App) -> Option<gpui::Div> {
    let shadows = pane_surface_shadow_style(chrome.shadow, cx);
    if !cx.theme().shadow || shadows.is_empty() {
        return None;
    }
    Some(
        div()
            .absolute()
            .inset_0()
            .rounded_tl(chrome.radii.top_left)
            .rounded_tr(chrome.radii.top_right)
            .rounded_bl(chrome.radii.bottom_left)
            .rounded_br(chrome.radii.bottom_right)
            .shadow(shadows),
    )
}

fn pane_surface_shadow_style(shadow: bool, cx: &App) -> Vec<BoxShadow> {
    if shadow { surface_ring(cx) } else { Vec::new() }
}

fn pane_corner_notches(chrome: PaneChrome) -> Option<gpui::Div> {
    let radii = chrome.radii;
    let band = radii
        .top_left
        .max(radii.top_right)
        .max(radii.bottom_left)
        .max(radii.bottom_right);
    if band <= px(0.0) {
        return None;
    }
    let outset = px(-f32::from(band));
    Some(
        div().absolute().inset_0().overflow_hidden().child(
            div()
                .absolute()
                .left(outset)
                .top(outset)
                .right(outset)
                .bottom(outset)
                .rounded_tl(radii.top_left + band)
                .rounded_tr(radii.top_right + band)
                .rounded_bl(radii.bottom_left + band)
                .rounded_br(radii.bottom_right + band)
                .border(band + chrome.border_width.max(Pixels::ZERO))
                .border_color(chrome.gap_background),
        ),
    )
}

fn pane_inactive_scrim(radii: Corners<Pixels>, opacity: f32, cx: &App) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .rounded_tl(radii.top_left)
        .rounded_tr(radii.top_right)
        .rounded_bl(radii.bottom_left)
        .rounded_br(radii.bottom_right)
        .bg(cx.theme().background.opaque().opacity(1.0 - opacity))
}

fn floating_surface_shadow(cx: &App) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: cx.theme().scrim,
        offset: point(px(0.0), px(8.0)),
        blur_radius: px(24.0),
        spread_radius: px(-8.0),
        inset: false,
    }]
}

#[derive(IntoElement)]
pub struct FloatingSurface {
    id: ElementId,
    title: SharedString,
    content: AnyElement,
    inset_x: Pixels,
    inset_y: Pixels,
    background: Hsla,
    foreground: Hsla,
    border_color: Hsla,
    bordered: bool,
}

impl FloatingSurface {
    pub fn new(id: impl Into<ElementId>, content: impl IntoElement, cx: &App) -> Self {
        Self {
            id: id.into(),
            title: SharedString::default(),
            content: content.into_any_element(),
            inset_x: Pixels::ZERO,
            inset_y: Pixels::ZERO,
            background: cx.theme().background.raised(1).opaque(),
            foreground: cx.theme().foreground,
            border_color: cx.theme().border,
            bordered: true,
        }
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    #[must_use]
    pub const fn content_inset(mut self, x: Pixels, y: Pixels) -> Self {
        self.inset_x = x;
        self.inset_y = y;
        self
    }

    #[must_use]
    pub const fn colors(mut self, background: Hsla, foreground: Hsla, border_color: Hsla) -> Self {
        self.background = background;
        self.foreground = foreground;
        self.border_color = border_color;
        self
    }

    #[must_use]
    pub const fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }
}

impl RenderOnce for FloatingSurface {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        let mut shadows = surface_ring(cx);
        if cx.theme().shadow {
            shadows.extend(floating_surface_shadow(cx));
        }
        div()
            .id(self.id)
            .relative()
            .size_full()
            .overflow_hidden()
            .occlude()
            .rounded(cx.theme().radius)
            .bg(self.background)
            .text_color(self.foreground)
            .shadow(shadows)
            .when(self.bordered, |surface| {
                surface.border_1().border_color(self.border_color)
            })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .absolute()
                    .left(self.inset_x)
                    .right(self.inset_x)
                    .top(self.inset_y)
                    .bottom(self.inset_y)
                    .overflow_hidden()
                    .child(self.content),
            )
            .when(self.bordered && !self.title.is_empty(), |surface| {
                surface.child(
                    div()
                        .absolute()
                        .top(px(1.0))
                        .left(self.inset_x.max(px(8.0)))
                        .max_w_full()
                        .px(px(4.0))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(crate::rems_from_px(11.0))
                        .line_height(self.inset_y.max(px(14.0)))
                        .bg(self.background)
                        .child(self.title),
                )
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneDragOverlayState {
    /// The prefix is armed: every pane is a handle waiting to be picked up.
    Armed,
    /// This pane is the one in flight. It renders in place and recedes.
    Source,
}

/// The occluding pane layer used while the tmux prefix is armed.
pub fn pane_drag_overlay(
    id: impl Into<ElementId>,
    state: PaneDragOverlayState,
    cx: &App,
) -> Stateful<gpui::Div> {
    let (tint, cursor) = match state {
        PaneDragOverlayState::Armed => (cx.theme().border.opacity(0.08), CursorStyle::OpenHand),
        PaneDragOverlayState::Source => (
            cx.theme()
                .background
                .opaque()
                .opacity(PANE_DRAG_SOURCE_FADE),
            CursorStyle::ClosedHand,
        ),
    };

    div()
        .id(id)
        .absolute()
        .inset_0()
        .occlude()
        .cursor(cursor)
        .bg(tint)
}

/// The rectangle a dragged pane lands in if it is dropped now. Positioning and
/// animation belong to the caller; pass the target pane's own radius and border
/// width so the preview's arcs land on the pane's.
pub fn pane_drop_preview(radius: Pixels, border_width: Pixels, cx: &App) -> gpui::Div {
    div()
        .absolute()
        .rounded(radius)
        .border(border_width)
        .border_color(cx.theme().foreground)
        .bg(cx.theme().foreground.fill())
}

/// The chip that follows the pointer while a pane is being dragged, carrying
/// the pane's name rather than a copy of its contents.
pub fn pane_drag_chip(
    pane: impl Into<SharedString>,
    title: impl Into<SharedString>,
    cx: &App,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .max_w(px(280.0))
        .px(px(10.0))
        .py(px(6.0))
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.opaque().raised(1))
        .shadow(floating_surface_shadow(cx))
        .text_size(crate::rems_from_px(12.0))
        .text_color(cx.theme().foreground)
        .child(
            div()
                .flex_none()
                .font_family(cx.theme().mono_font_family.clone())
                .font_weight(FontWeight::BOLD)
                .child(pane.into()),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(cx.theme().foreground.muted())
                .child(title.into()),
        )
}

const PANE_SPLIT_DIVIDER_THICKNESS: f32 = 1.0;
const PANE_SPLIT_HIT_THICKNESS: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneSplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneSplitSide {
    First,
    Second,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneSplitHighlight {
    pub start: f32,
    pub length: f32,
    pub side: PaneSplitSide,
    pub color: Hsla,
}

impl PaneSplitHighlight {
    #[must_use]
    pub const fn new(start: f32, length: f32, side: PaneSplitSide, color: Hsla) -> Self {
        Self {
            start,
            length,
            side,
            color,
        }
    }
}

/// The space a split keeps between its two children: the configured gap, or a
/// 1px hairline slot when there is none. Anything reconstructing pane boxes
/// from slot geometry must use this too.
#[must_use]
pub fn pane_split_slot(gap: Pixels) -> Pixels {
    px(f32::from(gap).max(PANE_SPLIT_DIVIDER_THICKNESS))
}

/// Draggable hit target for a pane split. The caller attaches the drag
/// callbacks to the returned element.
pub fn pane_split_hit_target(
    id: impl Into<ElementId>,
    axis: PaneSplitAxis,
    ratio: f32,
    gap: Pixels,
) -> Stateful<gpui::Div> {
    let slot = f32::from(pane_split_slot(gap));
    let offset = -(PANE_SPLIT_HIT_THICKNESS - slot) / 2.0;
    div()
        .id(id)
        .absolute()
        .cursor(match axis {
            PaneSplitAxis::Horizontal => CursorStyle::ResizeLeftRight,
            PaneSplitAxis::Vertical => CursorStyle::ResizeUpDown,
        })
        .occlude()
        .when(axis == PaneSplitAxis::Horizontal, |element| {
            element
                .left(relative(ratio))
                .ml(px(offset))
                .w(px(PANE_SPLIT_HIT_THICKNESS))
                .h_full()
        })
        .when(axis == PaneSplitAxis::Vertical, |element| {
            element
                .top(relative(ratio))
                .mt(px(offset))
                .h(px(PANE_SPLIT_HIT_THICKNESS))
                .w_full()
        })
}

/// Split composition. The slot between the children is the single inter-pane
/// gap: `gap` wide when a pane margin is configured, a 1px hairline otherwise.
/// The hit target centers on the slot.
pub fn pane_split_surface(
    id: impl Into<ElementId>,
    axis: PaneSplitAxis,
    ratio: f32,
    resizing: bool,
    gaps: bool,
    gap: Pixels,
    hairline: Option<Hsla>,
    highlight: Option<PaneSplitHighlight>,
    first_content: impl IntoElement,
    second_content: impl IntoElement,
    hit_target: impl IntoElement,
    gap_background: Hsla,
    cx: &App,
) -> Stateful<gpui::Div> {
    let slot = f32::from(pane_split_slot(gap));
    let first = div()
        .flex()
        .flex_none()
        .when(!gaps, gpui::Styled::overflow_hidden)
        .when(axis == PaneSplitAxis::Horizontal, |element| {
            element.w(relative(ratio)).h_full()
        })
        .when(axis == PaneSplitAxis::Vertical, |element| {
            element.h(relative(ratio)).w_full()
        })
        .child(first_content);
    let divider = div()
        .relative()
        .flex_none()
        .when(axis == PaneSplitAxis::Horizontal, |element| {
            element.w(px(slot)).h_full()
        })
        .when(axis == PaneSplitAxis::Vertical, |element| {
            element.h(px(slot)).w_full()
        })
        .when(!gaps, |divider| {
            let hairline = div()
                .absolute()
                .bg(if resizing {
                    cx.theme().foreground.wash()
                } else {
                    hairline.unwrap_or_else(|| cx.theme().border)
                })
                .when(axis == PaneSplitAxis::Horizontal, |line| {
                    line.left(relative(0.5))
                        .ml(px(-PANE_SPLIT_DIVIDER_THICKNESS / 2.0))
                        .w(px(PANE_SPLIT_DIVIDER_THICKNESS))
                        .h_full()
                })
                .when(axis == PaneSplitAxis::Vertical, |line| {
                    line.top(relative(0.5))
                        .mt(px(-PANE_SPLIT_DIVIDER_THICKNESS / 2.0))
                        .h(px(PANE_SPLIT_DIVIDER_THICKNESS))
                        .w_full()
                });
            divider.child(hairline).when_some(
                highlight.filter(|_| !resizing),
                |divider, highlight| {
                    let half = PANE_SPLIT_DIVIDER_THICKNESS / 2.0;
                    divider.child(
                        div()
                            .absolute()
                            .bg(highlight.color)
                            .when(axis == PaneSplitAxis::Horizontal, |segment| {
                                segment
                                    .top(relative(highlight.start))
                                    .h(relative(highlight.length))
                                    .w(px(half))
                                    .when(highlight.side == PaneSplitSide::First, |segment| {
                                        segment.left(relative(0.5)).ml(px(-half))
                                    })
                                    .when(highlight.side == PaneSplitSide::Second, |segment| {
                                        segment.left(relative(0.5))
                                    })
                            })
                            .when(axis == PaneSplitAxis::Vertical, |segment| {
                                segment
                                    .left(relative(highlight.start))
                                    .w(relative(highlight.length))
                                    .h(px(half))
                                    .when(highlight.side == PaneSplitSide::First, |segment| {
                                        segment.top(relative(0.5)).mt(px(-half))
                                    })
                                    .when(highlight.side == PaneSplitSide::Second, |segment| {
                                        segment.top(relative(0.5))
                                    })
                            }),
                    )
                },
            )
        });

    div()
        .id(id)
        .relative()
        .flex()
        .size_full()
        .when(!gaps, gpui::Styled::overflow_hidden)
        .when(axis == PaneSplitAxis::Vertical, |element| {
            element.flex_col()
        })
        .when(gaps, |element| {
            element.child(
                div()
                    .absolute()
                    .bg(gap_background)
                    .when(axis == PaneSplitAxis::Horizontal, |gap| {
                        gap.left(relative(ratio)).w(px(slot)).h_full()
                    })
                    .when(axis == PaneSplitAxis::Vertical, |gap| {
                        gap.top(relative(ratio)).h(px(slot)).w_full()
                    }),
            )
        })
        .child(first)
        .child(divider)
        .child(
            div()
                .flex()
                .flex_1()
                .when(!gaps, gpui::Styled::overflow_hidden)
                .child(second_content),
        )
        .child(hit_target)
}

/// A pending-entity placeholder shown as a top-right status tag while a pane
/// waits for its backing terminal, browser, or agent to attach.
pub fn pane_waiting_state(label: impl IntoElement) -> Tag {
    Tag::secondary()
        .text_size(crate::rems_from_px(11.0))
        .child(label)
}

/// Warns that keyboard input is mirrored to every pane in a synchronized group.
pub fn pane_sync_badge(cx: &App) -> Tag {
    Tag::secondary()
        .bg(cx.theme().danger.fill())
        .border_color(cx.theme().danger.fill())
        .text_color(cx.theme().danger)
        .text_size(crate::rems_from_px(11.0))
        .child("SYNC")
}

#[must_use]
pub fn frame_rate_badge(label: &'static str, fps: Option<f64>, cx: &App) -> gpui::Div {
    let rate = fps
        .filter(|fps| fps.is_finite())
        .map_or_else(|| "--.-".to_owned(), |fps| format!("{fps:>5.1}"));
    div()
        .h(px(22.0))
        .px(px(7.0))
        .flex()
        .items_center()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border.subtle())
        .bg(cx.theme().background.floating())
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(crate::rems_from_px(10.0))
        .text_color(cx.theme().foreground)
        .child(format!("{label} {rate} FPS"))
}

pub fn pane_indicator_overlay(indicator: impl IntoElement) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(indicator)
}

/// Which corner a [`pane_overlay_stack`] anchors to. Both right-align their
/// tags and differ only in whether the column grows down or up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneOverlayCorner {
    TopRight,
    BottomRight,
}

/// A right-aligned column of status tags pinned to one corner of a pane. The
/// caller passes the already-built tags in visual order.
pub fn pane_overlay_stack(
    corner: PaneOverlayCorner,
    tags: impl IntoIterator<Item = AnyElement>,
) -> gpui::Div {
    let column = div()
        .absolute()
        .right(px(8.0))
        .flex()
        .flex_col()
        .items_end()
        .gap(px(6.0));
    match corner {
        PaneOverlayCorner::TopRight => column.top(px(8.0)),
        PaneOverlayCorner::BottomRight => column.bottom(px(8.0)),
    }
    .children(tags)
}

/// The pane-selection card for tmux `display-panes`: the pane index over the
/// caller's [`crate::kbd::Kbd`] pill for `key`. The active pane takes danger
/// semantics, the rest the neutral list surface.
pub fn pane_indicator_card(
    id: impl Into<ElementId>,
    index: impl Into<SharedString>,
    key: impl IntoElement,
    active: bool,
    font_family: impl Into<SharedString>,
    cx: &App,
) -> Stateful<gpui::Div> {
    let background = if active {
        cx.theme().danger.fill()
    } else {
        cx.theme().foreground.wash()
    };
    let hover_background = if active {
        cx.theme().danger.fill().hover()
    } else {
        cx.theme().background.hover()
    };
    let index_color = if active {
        cx.theme().danger
    } else {
        cx.theme().foreground
    };

    div()
        .id(id)
        .min_w(px(40.0))
        .px(px(8.0))
        .py(px(6.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .rounded(cx.theme().radius)
        .bg(background)
        .shadow(vec![
            BoxShadow {
                color: cx.theme().border.subtle(),
                offset: point(px(0.0), px(0.0)),
                blur_radius: px(0.0),
                spread_radius: px(1.0),
                inset: false,
            },
            BoxShadow {
                color: cx.theme().scrim,
                offset: point(px(0.0), px(6.0)),
                blur_radius: px(16.0),
                spread_radius: px(0.0),
                inset: false,
            },
        ])
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(move |style| style.bg(hover_background))
        .child(
            div()
                .font_family(font_family.into())
                .font_weight(FontWeight::BOLD)
                .text_size(crate::rems_from_px(18.0))
                .line_height(crate::rems_from_px(20.0))
                .text_color(index_color)
                .child(index.into()),
        )
        .child(key)
}

/// The release control for a zoomed pane, as a status tag. The caller wraps it
/// in an interactive element, since [`Tag`] is not interactive.
pub fn pane_unzoom_control() -> Tag {
    Tag::secondary()
        .text_size(crate::rems_from_px(11.0))
        .child("UNZOOM")
}

pub fn terminal_mode_indicator(
    label: Option<impl Into<SharedString>>,
    detail: impl Into<SharedString>,
) -> Tag {
    let mut indicator = Tag::primary()
        .gap(px(6.0))
        .text_size(crate::rems_from_px(12.0));
    if let Some(label) = label {
        indicator = indicator.child(label.into());
    }
    indicator.child(detail.into())
}

/// The find prompt, shown as a focused status tag in the pane's bottom-right
/// overlay stack. Borrows the focus ring so it reads as the active input.
pub fn terminal_search_prompt(message: impl IntoElement, cx: &App) -> Tag {
    Tag::secondary()
        .max_w(px(560.0))
        .border_color(cx.theme().foreground)
        .text_color(cx.theme().foreground)
        .text_size(crate::rems_from_px(11.0))
        .child(message)
}

/// A transient status line (copy confirmations, search errors) for the pane's
/// bottom-right overlay stack.
pub fn terminal_status_popup(message: impl IntoElement, cx: &App) -> Tag {
    Tag::secondary()
        .max_w(px(560.0))
        .text_color(cx.theme().foreground.muted())
        .text_size(crate::rems_from_px(11.0))
        .child(message)
}

/// A preview of the URI under the pointer, shown in the pane's bottom-right
/// overlay stack.
pub fn terminal_link_popup(uri: impl IntoElement, cx: &App) -> Tag {
    Tag::secondary()
        .max_w(px(560.0))
        .overflow_hidden()
        .text_color(cx.theme().foreground)
        .text_size(crate::rems_from_px(11.0))
        .child(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_opacity_one_disables_surface_dimming() {
        let chrome = PaneChrome::new(
            Corners::default(),
            px(0.0),
            gpui::transparent_black(),
            gpui::transparent_black(),
            false,
        );

        assert_eq!(chrome.dimmed(true, 1.0).inactive_opacity, 1.0);
        assert_eq!(chrome.dimmed(false, 0.0).inactive_opacity, 1.0);
        assert_eq!(chrome.dimmed(true, 0.7).inactive_opacity, 0.7);
    }

    #[gpui::test]
    fn pane_surface_shadow_stays_inside_the_pane_curve(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| {
            assert!(pane_surface_shadow_style(false, cx).is_empty());

            let expected = surface_ring(cx);
            assert_eq!(pane_surface_shadow_style(true, cx), expected);
            assert!(expected[0].inset);
        });
    }
}
