//! The widget layer: the primitives our screens are built from.
//!
//! `crates/zz-ui/UPSTREAM.md` records the pinned [gpui-component][upstream]
//! revision each module was forked from.
//!
//! [upstream]: https://github.com/longbridge/gpui-component

pub mod button;
#[cfg(feature = "editor")]
pub mod code_editor;
pub mod color_picker;
pub mod foundation;
pub mod highlighter;
pub mod icon;
pub mod input;
pub mod kbd;
pub mod list;
pub mod menu;
pub mod overlay;
pub mod popover;
pub mod scroll;
pub mod select;
pub mod separator;
pub mod spinner;
pub mod switch;
pub mod tag;
pub mod text;
pub mod title_bar;
pub mod tooltip;

/// The dialog types, which live in [`overlay`] because `Root` owns the layer
/// they render into.
pub mod dialog {
    pub use super::overlay::{AlertDialog, Dialog, DialogButtonProps};
}

/// The notification type, which lives in [`overlay`] for the same reason as
/// [`dialog`].
pub mod notification {
    pub use super::overlay::Notification;
}

/// Theme storage and lookup, the single source of truth for theme values.
pub mod theme {
    pub use super::foundation::{ActiveTheme, Colorize, Theme, ThemeColor, ThemeMode};
}
