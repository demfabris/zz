use gtk::gdk;
use zz_terminal::{AppearanceColor, Color, PackedStyle};

const SCALE: f32 = 255.0;

pub fn rgba(color: Color) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(color.r) / SCALE,
        f32::from(color.g) / SCALE,
        f32::from(color.b) / SCALE,
        1.0,
    )
}

pub fn rgba_faded(color: Color, alpha: f32) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(color.r) / SCALE,
        f32::from(color.g) / SCALE,
        f32::from(color.b) / SCALE,
        alpha,
    )
}

pub fn appearance_rgba(color: AppearanceColor) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(color.r) / SCALE,
        f32::from(color.g) / SCALE,
        f32::from(color.b) / SCALE,
        f32::from(color.a) / SCALE,
    )
}

pub fn is_decorative_character(character: char) -> bool {
    matches!(
        character as u32,
        0x2500..=0x257f
            | 0x2580..=0x259f
            | 0x25a0..=0x25ff
            | 0xe0b0..=0xe0ca
            | 0xe0cc..=0xe0d7
    )
}

pub fn resolved_foreground(style: PackedStyle, minimum: f32, decorative: bool) -> Color {
    let foreground = style.foreground();
    if style.explicit_rgb() || decorative || minimum <= 1.0 {
        return foreground;
    }
    ensure_minimum_contrast(foreground, style.background(), minimum)
}

#[allow(
    clippy::disallowed_methods,
    reason = "terminal contrast correction operates on terminal palette colors, not application chrome"
)]
fn ensure_minimum_contrast(foreground: Color, background: Color, minimum: f32) -> Color {
    if contrast_ratio(foreground, background) >= minimum {
        return foreground;
    }
    let black = Color::rgb(0, 0, 0);
    let white = Color::rgb(u8::MAX, u8::MAX, u8::MAX);
    let target = if contrast_ratio(black, background) > contrast_ratio(white, background) {
        black
    } else {
        white
    };
    if contrast_ratio(target, background) < minimum {
        return target;
    }

    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for _ in 0..12 {
        let midpoint = (low + high) * 0.5;
        if contrast_ratio(blend_color(foreground, target, midpoint), background) >= minimum {
            high = midpoint;
        } else {
            low = midpoint;
        }
    }
    blend_color(foreground, target, high)
}

fn contrast_ratio(left: Color, right: Color) -> f32 {
    let left = relative_luminance(left);
    let right = relative_luminance(right);
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}

fn relative_luminance(color: Color) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    channel(color.r) * 0.2126 + channel(color.g) * 0.7152 + channel(color.b) * 0.0722
}

#[allow(
    clippy::disallowed_methods,
    reason = "terminal color blending operates on terminal palette colors, not application chrome"
)]
fn blend_color(from: Color, to: Color, amount: f32) -> Color {
    let channel = |from: u8, to: u8| {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::rgb(
        channel(from.r, to.r),
        channel(from.g, to.g),
        channel(from.b, to.b),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_terminal::{ATTR_EXPLICIT_RGB, UnderlineStyle};

    #[test]
    fn contrast_preserves_explicit_and_decorative_colors() {
        let foreground = Color {
            r: 238,
            g: 238,
            b: 238,
        };
        let background = Color {
            r: 245,
            g: 245,
            b: 245,
        };
        let style = PackedStyle::new(foreground, background, None, 0, UnderlineStyle::None);
        assert!(contrast_ratio(resolved_foreground(style, 4.5, false), background) >= 4.5);
        assert_eq!(resolved_foreground(style, 4.5, true), foreground);
        assert_eq!(resolved_foreground(style, 1.0, false), foreground);
        let explicit = PackedStyle::new(
            foreground,
            background,
            None,
            ATTR_EXPLICIT_RGB,
            UnderlineStyle::None,
        );
        assert_eq!(resolved_foreground(explicit, 4.5, false), foreground);
        assert!(is_decorative_character('█'));
        assert!(!is_decorative_character('A'));
    }
}
