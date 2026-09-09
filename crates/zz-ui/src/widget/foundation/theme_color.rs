//! The palette: every color the widget layer is allowed to name.

use gpui::Hsla;

use super::{Colorize as _, color::edge_weight};

/// The five palette roots and scrim the widget layer names. Read them off `cx.theme()`; derive
/// everything else with [`Colorize`].
///
/// [`Colorize`]: super::Colorize
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ThemeColor {
    /// The window's base plane. Every other surface is this, raised.
    pub background: Hsla,
    /// Default text, and the source of muted text, focus rings, links and
    /// selection and every edge.
    pub foreground: Hsla,
    /// Something completed or is healthy.
    pub success: Hsla,
    /// Something needs attention but still works.
    pub warning: Hsla,
    /// Something failed or is destructive.
    pub danger: Hsla,
    /// The dimming behind modals and under shadows. Black in both modes, with a
    /// per-mode alpha.
    pub scrim: Hsla,
}

impl ThemeColor {
    pub fn border(&self) -> Hsla {
        self.background
            .mix_oklab(self.foreground, 1.0 - edge_weight())
            .opaque()
    }
}
