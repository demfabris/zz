//! The foundation every widget stands on: the theme global and the shared
//! traits.

#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

mod animation;
mod color;
mod element_ext;
mod geometry;
mod index_path;
mod palette;
mod styled;
mod theme;
mod theme_color;
mod units;
mod window_border;

pub use animation::cubic_ease;
pub use color::{Colorize, oklab_lightness, parse_hex, to_hex};
pub use element_ext::{ElementExt, InteractiveElementExt};
pub use geometry::Side;
pub use index_path::IndexPath;
pub use styled::{
    Disableable, SURFACE_RING_OUTSET, Selectable, Sizable, Size, StyleSized, StyledExt,
    control_shadow, h_flex, stacked_ring, surface_ring, v_flex,
};
pub use theme::{ActiveTheme, CHROME_GAP, ScrollbarShow, Theme, ThemeMode};
pub use theme_color::ThemeColor;
pub use units::{BASE_UI_FONT_SIZE, UiZoom, rems_from_px};
pub use window_border::{window_border, window_paddings};

/// Install the [`Theme`] global. Must run once, before any window opens;
/// `zz_ui::init` calls it first, ahead of every widget's own `init`.
pub fn init(cx: &mut gpui::App) {
    theme::init(cx);
}
