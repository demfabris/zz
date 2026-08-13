#[cfg(target_os = "linux")]
use gpui::Decorations;
#[cfg(any(target_os = "linux", test))]
use gpui::Tiling;
use gpui::{Corners, Pixels, Styled, Window, px};
use zz_protocol::Axis;

const TOP_LEFT: u8 = 1 << 0;
const TOP_RIGHT: u8 = 1 << 1;
const BOTTOM_RIGHT: u8 = 1 << 2;
const BOTTOM_LEFT: u8 = 1 << 3;
const TOP: u8 = TOP_LEFT | TOP_RIGHT;
const RIGHT: u8 = TOP_RIGHT | BOTTOM_RIGHT;
const BOTTOM: u8 = BOTTOM_LEFT | BOTTOM_RIGHT;
const LEFT: u8 = TOP_LEFT | BOTTOM_LEFT;
#[cfg(any(target_os = "linux", test))]
const ALL: u8 = TOP | BOTTOM;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WindowCorners(u8);

impl WindowCorners {
    pub(crate) const NONE: Self = Self(0);

    pub(crate) fn for_window(window: &Window) -> Self {
        #[cfg(target_os = "linux")]
        {
            let Decorations::Client { tiling } = window.window_decorations() else {
                return Self::NONE;
            };
            Self::from_tiling(tiling)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = window;
            Self::NONE
        }
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) const fn from_tiling(tiling: Tiling) -> Self {
        let mut corners = ALL;
        if tiling.top {
            corners &= !TOP;
        }
        if tiling.right {
            corners &= !RIGHT;
        }
        if tiling.bottom {
            corners &= !BOTTOM;
        }
        if tiling.left {
            corners &= !LEFT;
        }
        Self(corners)
    }

    pub(crate) const fn top(self) -> Self {
        Self(self.0 & TOP)
    }

    pub(crate) const fn bottom(self) -> Self {
        Self(self.0 & BOTTOM)
    }

    pub(crate) const fn left(self) -> Self {
        Self(self.0 & LEFT)
    }

    pub(crate) const fn right(self) -> Self {
        Self(self.0 & RIGHT)
    }

    pub(crate) const fn split(self, axis: Axis) -> (Self, Self) {
        match axis {
            Axis::Horizontal => (Self(self.0 & LEFT), Self(self.0 & RIGHT)),
            Axis::Vertical => (Self(self.0 & TOP), Self(self.0 & BOTTOM)),
        }
    }

    pub(crate) const fn top_left(self) -> bool {
        self.0 & TOP_LEFT != 0
    }

    pub(crate) const fn top_right(self) -> bool {
        self.0 & TOP_RIGHT != 0
    }

    pub(crate) const fn bottom_left_is_rounded(self) -> bool {
        self.0 & BOTTOM_LEFT != 0
    }

    pub(crate) const fn bottom_right_is_rounded(self) -> bool {
        self.0 & BOTTOM_RIGHT != 0
    }

    pub(crate) fn radii(self, radius: Pixels) -> Corners<Pixels> {
        self.surface_radii(radius, px(0.0))
    }

    /// Per-corner radii for a pane surface: `base` everywhere, `exposed` at
    /// window-exposed corners.
    pub(crate) fn surface_radii(self, exposed: Pixels, base: Pixels) -> Corners<Pixels> {
        Corners {
            top_left: if self.top_left() { exposed } else { base },
            top_right: if self.top_right() { exposed } else { base },
            bottom_right: if self.bottom_right_is_rounded() {
                exposed
            } else {
                base
            },
            bottom_left: if self.bottom_left_is_rounded() {
                exposed
            } else {
                base
            },
        }
    }

    pub(crate) fn round_div<E: Styled>(self, element: E, radius: Pixels) -> E {
        round_div_radii(element, self.radii(radius))
    }
}

pub(crate) fn round_div_radii<E: Styled>(element: E, radii: Corners<Pixels>) -> E {
    element
        .rounded_tl(radii.top_left)
        .rounded_tr(radii.top_right)
        .rounded_bl(radii.bottom_left)
        .rounded_br(radii.bottom_right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowed_and_tiled_corner_sets_match_exposed_edges() {
        assert_eq!(
            WindowCorners::from_tiling(Tiling::default()),
            WindowCorners(ALL)
        );
        assert_eq!(
            WindowCorners::from_tiling(Tiling {
                top: true,
                ..Tiling::default()
            }),
            WindowCorners(BOTTOM)
        );
        assert_eq!(
            WindowCorners::from_tiling(Tiling::tiled()),
            WindowCorners::NONE
        );
    }

    #[test]
    fn split_routes_outer_corners_to_the_correct_children() {
        let all = WindowCorners(ALL);
        assert_eq!(
            all.split(Axis::Horizontal),
            (WindowCorners(LEFT), WindowCorners(RIGHT))
        );
        assert_eq!(
            all.split(Axis::Vertical),
            (WindowCorners(TOP), WindowCorners(BOTTOM))
        );

        let (_, right) = all.split(Axis::Horizontal);
        assert_eq!(
            right.split(Axis::Vertical),
            (WindowCorners(TOP_RIGHT), WindowCorners(BOTTOM_RIGHT))
        );
    }
}
