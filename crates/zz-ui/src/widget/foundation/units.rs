use gpui::{App, Global, Pixels, Rems, rems};

/// The design-system baseline: at the default UI size, one rem is 16 pixels.
pub const BASE_UI_FONT_SIZE: f32 = 16.0;

/// The content zoom the chrome lays out against. `1.0` is unzoomed, and the app
/// layer is the only writer. Logical-pixel metrics ride the zoom for free; space
/// reserved for native window furniture does not, and must divide by this.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct UiZoom(pub f32);

impl Default for UiZoom {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Global for UiZoom {}

impl UiZoom {
    /// The zoom in effect, or `1.0` before the app layer has set one.
    #[must_use]
    pub fn get(cx: &App) -> f32 {
        cx.try_global::<Self>().map_or(1.0, |zoom| zoom.0)
    }

    /// Undo the zoom on a measurement, so its physical size stays put. Only for
    /// space reserved for native window furniture.
    #[must_use]
    pub fn unzoomed(value: Pixels, cx: &App) -> Pixels {
        value / Self::get(cx)
    }
}

/// Express a design measurement in rems, preserving its default pixel size. Use
/// it for named UI metrics; one-pixel borders and other physical details stay on
/// [`gpui::px`].
pub const fn rems_from_px(value: f32) -> Rems {
    rems(value / BASE_UI_FONT_SIZE)
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::*;

    #[test]
    fn larger_root_rem_scales_the_design_measurement() {
        assert_eq!(rems_from_px(36.0).to_pixels(px(20.0)), px(45.0));
    }
}
