//! Window-level overlay cluster: [`Root`], the dialog stack, and the
//! notification stack.
//!
//! Ported from `gpui-component` and kept close to its source, so the style
//! lints stay off here.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

use gpui::{App, KeyBinding};

mod actions;
mod dialog;
mod notification;
mod root;
mod window_ext;

pub use crate::text::ROOT_KEY_CONTEXT;
pub(crate) use dialog::dialog_description;
pub use dialog::{AlertDialog, Dialog, DialogButtonProps};
pub use notification::Notification;
pub use root::Root;
pub use window_ext::WindowExt;

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", actions::Tab, Some(root::CONTEXT)),
        KeyBinding::new("shift-tab", actions::TabPrev, Some(root::CONTEXT)),
        KeyBinding::new("escape", actions::CancelDialog, Some(dialog::CONTEXT)),
        KeyBinding::new("enter", actions::ConfirmDialog, Some(dialog::CONTEXT)),
    ]);
}
