//! Popup, dropdown, and context menus.

// Vendored from `gpui-component`; keeps upstream's style, not this crate's lints.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

mod actions;
mod context_menu;
mod dropdown_menu;
mod menu_item;
mod popup_menu;

use gpui::App;

pub use context_menu::{ContextMenu, ContextMenuExt, ContextMenuState};
pub use dropdown_menu::DropdownMenu;
pub use popup_menu::{PopupMenu, PopupMenuItem};

/// Register the menu's keyboard bindings. Called once from [`crate::init`].
pub(crate) fn init(cx: &mut App) {
    popup_menu::init(cx);
}
