//! The two built-in palettes, light and dark.

use std::sync::{Arc, LazyLock};

use gpui::Hsla;

use super::{ThemeColor, ThemeMode, color::hsl};

static LIGHT: LazyLock<Arc<ThemeColor>> = LazyLock::new(|| Arc::new(palette(ThemeMode::Light)));
static DARK: LazyLock<Arc<ThemeColor>> = LazyLock::new(|| Arc::new(palette(ThemeMode::Dark)));

impl ThemeColor {
    pub fn light() -> Arc<Self> {
        LIGHT.clone()
    }

    pub fn dark() -> Arc<Self> {
        DARK.clone()
    }

    pub fn for_mode(mode: ThemeMode) -> Arc<Self> {
        if mode.is_dark() {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

#[inline]
fn scrim(alpha: f32) -> Hsla {
    gpui::hsla(0., 0., 0., alpha)
}

fn palette(mode: ThemeMode) -> ThemeColor {
    // shadcn/Tailwind scale values, as `hsl(deg, %, %)`.
    let neutral_50 = hsl(0., 0., 98.);
    let neutral_200 = hsl(0., 0., 89.8);
    let neutral_800 = hsl(0., 0., 14.9);
    let neutral_950 = hsl(0., 0., 3.9);
    let white = hsl(0., 0., 100.);
    let red_400 = hsl(0., 90.6, 70.8);
    let red_500 = hsl(0., 84.2, 60.2);
    let green_400 = hsl(141.9, 69.2, 58.);
    let green_500 = hsl(142.1, 70.6, 45.3);
    let yellow_400 = hsl(47.9, 95.8, 53.1);
    let yellow_500 = hsl(45.4, 93.4, 47.5);

    let is_dark = mode.is_dark();

    macro_rules! per_mode {
        ($light:expr, $dark:expr) => {
            if is_dark { $dark } else { $light }
        };
    }

    ThemeColor {
        background: per_mode!(white, neutral_950),
        foreground: per_mode!(neutral_950, neutral_50),
        border: per_mode!(neutral_200, neutral_800),
        success: per_mode!(green_500, green_400),
        warning: per_mode!(yellow_500, yellow_400),
        danger: per_mode!(red_500, red_400),
        scrim: per_mode!(scrim(0.05), scrim(0.2)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::foundation::Colorize as _;

    #[test]
    fn palettes_are_cached() {
        assert!(Arc::ptr_eq(&ThemeColor::light(), &ThemeColor::light()));
    }

    #[test]
    fn the_chrome_is_achromatic() {
        for palette in [ThemeColor::light(), ThemeColor::dark()] {
            for neutral in [palette.background, palette.foreground, palette.border] {
                assert_eq!(neutral.s, 0.0);
            }
        }
    }

    #[test]
    fn panels_stay_on_the_background_side_of_the_foreground() {
        for palette in [ThemeColor::light(), ThemeColor::dark()] {
            let distance = |color: Hsla| (color.l - palette.background.l).abs();
            assert!(distance(palette.background.raised(3)) < distance(palette.foreground));
        }
    }
}
