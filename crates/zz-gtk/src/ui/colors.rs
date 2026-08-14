use gtk::gdk;
use zz_terminal::{AppearanceColor, Color};

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
