// Adapted from gpui-component's Apache-2.0 Linux client-side window border:
// https://github.com/longbridge/gpui-component/blob/b004e595cf5de98a73b6b561394a559a94ae1e2a/crates/zz-ui/src/window_border.rs
use gpui::{
    AnyElement, App, BorderStyle, Bounds, CursorStyle, Decorations, Edges, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement, Pixels, Point, RenderOnce,
    ResizeEdge, Size, Styled as _, Tiling, Window, canvas, div, point, prelude::FluentBuilder as _,
    px, quad, size,
};
use zz_ui::ActiveTheme as _;

use crate::{
    config::{WINDOW_FRAME_BORDER_SIZE, window_corner_radius},
    window::corners::WindowCorners,
};

const SHADOW_SIZE: Pixels = px(12.0);
const RESIZE_HIT_SIZE: Pixels = px(4.0);

/// The rounded Linux client-side frame around the complete application shell.
#[derive(IntoElement)]
pub(crate) struct RoundedWindowFrame {
    children: Vec<AnyElement>,
}

pub(crate) fn rounded_window_frame() -> RoundedWindowFrame {
    RoundedWindowFrame {
        children: Vec::new(),
    }
}

impl ParentElement for RoundedWindowFrame {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for RoundedWindowFrame {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let window_radius = window_corner_radius(cx);
        let decorations = window.window_decorations();
        let visual_shadow = match decorations {
            Decorations::Client { tiling } if tiling == Tiling::tiled() => px(0.0),
            _ => SHADOW_SIZE,
        };

        if matches!(decorations, Decorations::Client { .. }) {
            window.set_client_inset(SHADOW_SIZE);
        }
        update_corner_mask(window, decorations, visual_shadow, window_radius);

        let window_size = window.window_bounds().get_bounds().size;

        div()
            .id("rounded-window-backdrop")
            .bg(gpui::transparent_black())
            .map(|frame| match decorations {
                Decorations::Server => frame,
                Decorations::Client { tiling } => frame
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .when(!tiling.top, |frame| frame.pt(visual_shadow))
                    .when(!tiling.bottom, |frame| frame.pb(visual_shadow))
                    .when(!tiling.left, |frame| frame.pl(visual_shadow))
                    .when(!tiling.right, |frame| frame.pr(visual_shadow))
                    .on_mouse_down(MouseButton::Left, move |_, window, _| {
                        let Decorations::Client { tiling } = window.window_decorations() else {
                            return;
                        };
                        if tiling == Tiling::tiled() {
                            return;
                        }

                        let size = window.window_bounds().get_bounds().size;
                        let position = window.mouse_position();
                        let insets = client_frame_insets(SHADOW_SIZE, tiling);
                        if let Some(edge) =
                            resize_edge(position, size, insets, tiling, RESIZE_HIT_SIZE)
                        {
                            window.start_window_resize(edge);
                        }
                    }),
            })
            .size_full()
            .child(
                div()
                    .cursor(CursorStyle::default())
                    .map(|surface| match decorations {
                        Decorations::Server => surface.size_full(),
                        Decorations::Client { tiling } => WindowCorners::from_tiling(tiling)
                            .round_div(
                                surface
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .when(!tiling.top, |surface| {
                                        surface.pt(WINDOW_FRAME_BORDER_SIZE)
                                    })
                                    .when(!tiling.bottom, |surface| {
                                        surface.pb(WINDOW_FRAME_BORDER_SIZE)
                                    })
                                    .when(!tiling.left, |surface| {
                                        surface.pl(WINDOW_FRAME_BORDER_SIZE)
                                    })
                                    .when(!tiling.right, |surface| {
                                        surface.pr(WINDOW_FRAME_BORDER_SIZE)
                                    })
                                    .when(!tiling.is_tiled(), |surface| {
                                        surface.shadow(vec![gpui::BoxShadow {
                                            color: cx.theme().scrim,
                                            blur_radius: visual_shadow / 2.0,
                                            spread_radius: px(0.0),
                                            offset: point(px(0.0), px(0.0)),
                                            inset: false,
                                        }])
                                    }),
                                window_radius,
                            ),
                    })
                    .on_mouse_move(|_, _, cx| cx.stop_propagation())
                    .when(!crate::theme::chrome_blur(cx), |surface| {
                        surface.bg(cx.theme().background)
                    })
                    .children(self.children)
                    .map(|surface| match decorations {
                        Decorations::Server => surface,
                        Decorations::Client { tiling } => {
                            surface.child(border_ring(tiling, window_radius, cx.theme().border))
                        }
                    }),
            )
            .when(matches!(decorations, Decorations::Client { .. }), |frame| {
                let Decorations::Client { tiling } = decorations else {
                    return frame;
                };
                frame.child(
                    div()
                        .absolute()
                        .w(px(0.0))
                        .h(px(0.0))
                        .children(resize_hit_zones(
                            window_size,
                            SHADOW_SIZE,
                            RESIZE_HIT_SIZE,
                            tiling,
                        )),
                )
            })
    }
}

fn update_corner_mask(
    window: &mut Window,
    decorations: Decorations,
    visual_shadow: Pixels,
    radius: Pixels,
) {
    let mask = match decorations {
        Decorations::Server => None,
        Decorations::Client { tiling } => {
            let corners = WindowCorners::from_tiling(tiling);
            (corners != WindowCorners::NONE).then(|| {
                let window_size = window.window_bounds().get_bounds().size;
                let insets = client_frame_insets(visual_shadow, tiling);
                let bounds = Bounds {
                    origin: point(insets.left, insets.top),
                    size: size(
                        window_size.width - insets.left - insets.right,
                        window_size.height - insets.top - insets.bottom,
                    ),
                };
                (bounds, corners.radii(radius))
            })
        }
    };
    window.set_window_corner_mask(mask);
}

fn border_ring(tiling: Tiling, radius: Pixels, color: Hsla) -> impl IntoElement {
    let corners = WindowCorners::from_tiling(tiling);
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            window.paint_quad(quad(
                bounds,
                corners.radii(radius),
                gpui::transparent_black(),
                client_frame_insets(WINDOW_FRAME_BORDER_SIZE, tiling),
                color,
                BorderStyle::default(),
            ));
        },
    )
    .absolute()
    .inset_0()
}

fn client_frame_insets(shadow_size: Pixels, tiling: Tiling) -> Edges<Pixels> {
    Edges {
        top: if tiling.top { px(0.0) } else { shadow_size },
        right: if tiling.right { px(0.0) } else { shadow_size },
        bottom: if tiling.bottom { px(0.0) } else { shadow_size },
        left: if tiling.left { px(0.0) } else { shadow_size },
    }
}

fn cursor_style_for_resize_edge(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

fn resize_hit_zones(
    window_size: Size<Pixels>,
    shadow_size: Pixels,
    hit_size: Pixels,
    tiling: Tiling,
) -> Vec<AnyElement> {
    if tiling == Tiling::tiled() {
        return Vec::new();
    }

    let insets = client_frame_insets(shadow_size, tiling);
    let inner_left = insets.left;
    let inner_right = window_size.width - insets.right;
    let inner_top = insets.top;
    let inner_bottom = window_size.height - insets.bottom;
    let frame_origin = point(insets.left, insets.top);
    let band = hit_size + hit_size;
    let horizontal_span = inner_right - inner_left + band;
    let vertical_span = inner_bottom - inner_top + band;
    let mut zones = Vec::new();

    let mut push_zone = |edge: ResizeEdge, origin: Point<Pixels>, size: Size<Pixels>| {
        let origin = origin - frame_origin;
        zones.push(
            div()
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .w(size.width)
                .h(size.height)
                .cursor(cursor_style_for_resize_edge(edge))
                .into_any_element(),
        );
    };

    if !tiling.top {
        push_zone(
            ResizeEdge::Top,
            point(inner_left - hit_size, inner_top - hit_size),
            Size::new(horizontal_span, band),
        );
    }
    if !tiling.bottom {
        push_zone(
            ResizeEdge::Bottom,
            point(inner_left - hit_size, inner_bottom - hit_size),
            Size::new(horizontal_span, band),
        );
    }
    if !tiling.left {
        push_zone(
            ResizeEdge::Left,
            point(inner_left - hit_size, inner_top - hit_size),
            Size::new(band, vertical_span),
        );
    }
    if !tiling.right {
        push_zone(
            ResizeEdge::Right,
            point(inner_right - hit_size, inner_top - hit_size),
            Size::new(band, vertical_span),
        );
    }

    if !tiling.top && !tiling.left {
        push_zone(
            ResizeEdge::TopLeft,
            point(inner_left - hit_size, inner_top - hit_size),
            Size::new(band, band),
        );
    }
    if !tiling.top && !tiling.right {
        push_zone(
            ResizeEdge::TopRight,
            point(inner_right - hit_size, inner_top - hit_size),
            Size::new(band, band),
        );
    }
    if !tiling.bottom && !tiling.left {
        push_zone(
            ResizeEdge::BottomLeft,
            point(inner_left - hit_size, inner_bottom - hit_size),
            Size::new(band, band),
        );
    }
    if !tiling.bottom && !tiling.right {
        push_zone(
            ResizeEdge::BottomRight,
            point(inner_right - hit_size, inner_bottom - hit_size),
            Size::new(band, band),
        );
    }

    zones
}

fn resize_edge(
    position: Point<Pixels>,
    size: Size<Pixels>,
    insets: Edges<Pixels>,
    tiling: Tiling,
    hit_size: Pixels,
) -> Option<ResizeEdge> {
    let inner_left = insets.left;
    let inner_right = size.width - insets.right;
    let inner_top = insets.top;
    let inner_bottom = size.height - insets.bottom;

    let on_left = position.x >= inner_left - hit_size
        && position.x <= inner_left + hit_size
        && position.y >= inner_top - hit_size
        && position.y <= inner_bottom + hit_size;
    let on_right = position.x >= inner_right - hit_size
        && position.x <= inner_right + hit_size
        && position.y >= inner_top - hit_size
        && position.y <= inner_bottom + hit_size;
    let on_top = position.y >= inner_top - hit_size
        && position.y <= inner_top + hit_size
        && position.x >= inner_left - hit_size
        && position.x <= inner_right + hit_size;
    let on_bottom = position.y >= inner_bottom - hit_size
        && position.y <= inner_bottom + hit_size
        && position.x >= inner_left - hit_size
        && position.x <= inner_right + hit_size;

    if !tiling.top && !tiling.left && on_top && on_left {
        return Some(ResizeEdge::TopLeft);
    }
    if !tiling.top && !tiling.right && on_top && on_right {
        return Some(ResizeEdge::TopRight);
    }
    if !tiling.bottom && !tiling.left && on_bottom && on_left {
        return Some(ResizeEdge::BottomLeft);
    }
    if !tiling.bottom && !tiling.right && on_bottom && on_right {
        return Some(ResizeEdge::BottomRight);
    }
    if !tiling.top && on_top {
        return Some(ResizeEdge::Top);
    }
    if !tiling.bottom && on_bottom {
        return Some(ResizeEdge::Bottom);
    }
    if !tiling.left && on_left {
        return Some(ResizeEdge::Left);
    }
    if !tiling.right && on_right {
        return Some(ResizeEdge::Right);
    }
    None
}
