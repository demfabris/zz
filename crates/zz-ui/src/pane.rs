use crate::{ActiveTheme as _, Colorize as _, tag::Tag};
use gpui::{
    AnyElement, App, BoxShadow, Corners, CursorStyle, ElementId, FontWeight, Hsla, IntoElement,
    ParentElement as _, Pixels, SharedString, Stateful, Styled as _, div, point, prelude::*, px,
    relative,
};

/// How far an inactive pane fades toward the window background.
pub const INACTIVE_PANE_FADE: f32 = 0.3;

/// Opacity multiplier for inactive terminal glyphs, which dim themselves
/// instead of taking the scrim.
pub const INACTIVE_PANE_CONTENT_OPACITY: f32 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneChrome {
    pub radii: Corners<Pixels>,
    pub border_width: Pixels,
    pub border_color: Hsla,
    pub gap_background: Hsla,
    pub dimmed: bool,
}

impl PaneChrome {
    #[must_use]
    pub const fn new(
        radii: Corners<Pixels>,
        border_width: Pixels,
        border_color: Hsla,
        gap_background: Hsla,
    ) -> Self {
        Self {
            radii,
            border_width,
            border_color,
            gap_background,
            dimmed: false,
        }
    }

    /// Marks this pane as *not* the active one, fading it behind a scrim.
    #[must_use]
    pub const fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
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
                .border(chrome.border_width)
                .border_color(chrome.border_color)
                .child(content)
                .when(chrome.dimmed, |surface| {
                    surface.child(pane_inactive_scrim(chrome.radii, cx))
                })
                .children(overlays),
        )
        .children(pane_border_shadow(chrome, cx))
}

fn pane_border_shadow(chrome: PaneChrome, cx: &App) -> Option<gpui::Div> {
    let width = chrome.border_width.max(Pixels::ZERO);
    if width <= Pixels::ZERO || !cx.theme().shadow {
        return None;
    }
    let scrim = cx.theme().scrim;
    Some(
        div()
            .absolute()
            .inset_0()
            .rounded_tl(chrome.radii.top_left)
            .rounded_tr(chrome.radii.top_right)
            .rounded_bl(chrome.radii.bottom_left)
            .rounded_br(chrome.radii.bottom_right)
            .shadow(pane_border_shadow_style(width, scrim)),
    )
}

fn pane_border_shadow_style(width: Pixels, scrim: Hsla) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: scrim.divide(0.12),
        offset: point(Pixels::ZERO, Pixels::ZERO),
        blur_radius: Pixels::ZERO,
        spread_radius: width,
        inset: true,
    }]
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

fn pane_inactive_scrim(radii: Corners<Pixels>, cx: &App) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .rounded_tl(radii.top_left)
        .rounded_tr(radii.top_right)
        .rounded_bl(radii.bottom_left)
        .rounded_br(radii.bottom_right)
        .bg(cx.theme().background.opaque().opacity(INACTIVE_PANE_FADE))
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
            cx.theme().background.opaque().opacity(INACTIVE_PANE_FADE),
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
pub fn pane_split_slot(gap: Pixels, scale: f32) -> Pixels {
    px(f32::from(gap).max(PANE_SPLIT_DIVIDER_THICKNESS * scale.max(0.1)))
}

/// Draggable hit target for a pane split. The caller attaches the drag
/// callbacks to the returned element.
pub fn pane_split_hit_target(
    id: impl Into<ElementId>,
    axis: PaneSplitAxis,
    ratio: f32,
    gap: Pixels,
    scale: f32,
) -> Stateful<gpui::Div> {
    let scale = scale.max(0.1);
    let slot = f32::from(pane_split_slot(gap, scale));
    let hit_thickness = PANE_SPLIT_HIT_THICKNESS * scale;
    let offset = -(hit_thickness - slot) / 2.0;
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
                .w(px(hit_thickness))
                .h_full()
        })
        .when(axis == PaneSplitAxis::Vertical, |element| {
            element
                .top(relative(ratio))
                .mt(px(offset))
                .h(px(hit_thickness))
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
    scale: f32,
    highlight: Option<PaneSplitHighlight>,
    first_content: impl IntoElement,
    second_content: impl IntoElement,
    hit_target: impl IntoElement,
    gap_background: Hsla,
    cx: &App,
) -> Stateful<gpui::Div> {
    let scale = scale.max(0.1);
    let divider_thickness = PANE_SPLIT_DIVIDER_THICKNESS * scale;
    let slot = f32::from(pane_split_slot(gap, scale));
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
                    cx.theme().border
                })
                .when(axis == PaneSplitAxis::Horizontal, |line| {
                    line.left(relative(0.5))
                        .ml(px(-divider_thickness / 2.0))
                        .w(px(divider_thickness))
                        .h_full()
                })
                .when(axis == PaneSplitAxis::Vertical, |line| {
                    line.top(relative(0.5))
                        .mt(px(-divider_thickness / 2.0))
                        .h(px(divider_thickness))
                        .w_full()
                });
            divider.child(hairline).when_some(
                highlight.filter(|_| !resizing),
                |divider, highlight| {
                    let half = divider_thickness / 2.0;
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
                                        segment.left(relative(0.5)).ml(px(-divider_thickness / 2.0))
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
                                        segment.top(relative(0.5)).mt(px(-divider_thickness / 2.0))
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
    fn pane_border_shadow_is_an_unblurred_inset() {
        let shadow = pane_border_shadow_style(px(3.0), gpui::hsla(0.0, 0.0, 0.0, 0.2));
        let shadow = &shadow[0];

        assert!(shadow.inset);
        assert_eq!(shadow.spread_radius, px(3.0));
        assert_eq!(shadow.blur_radius, Pixels::ZERO);
        assert_eq!(shadow.offset, point(Pixels::ZERO, Pixels::ZERO));
        assert_eq!(shadow.color.a, 0.12);
    }
}
