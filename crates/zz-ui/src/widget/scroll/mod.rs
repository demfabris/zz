//! Scrollbars, and the extension trait that puts them on a container.

// Vendored from `gpui-component`; keeps upstream's style, not this crate's lints.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

mod scrollable;
mod scrollbar;

pub use scrollable::{Scrollable, ScrollableElement};
pub use scrollbar::{
    MIN_THUMB_SIZE, Scrollbar, ScrollbarAxis, ScrollbarHandle, ScrollbarShow, THUMB_INSET,
    THUMB_WIDTH, WIDTH as GUTTER_WIDTH, thumb_radius,
};
