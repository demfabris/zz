//! The palette: every color the widget layer is allowed to name.

use gpui::Hsla;

/// The seven colors the widget layer names. Read them off `cx.theme()`; derive
/// everything else with [`Colorize`].
///
/// [`Colorize`]: super::Colorize
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ThemeColor {
    /// The window's base plane. Every other surface is this, raised.
    pub background: Hsla,
    /// Default text, and the source of muted text, focus rings, links and
    /// selection.
    pub foreground: Hsla,
    /// Every edge: panel borders, dividers, input outlines, the window frame.
    pub border: Hsla,
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
