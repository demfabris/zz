//! The app's primary interactive control.

// Vendored from `gpui-component`; keeps upstream's style, not this crate's lints.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

mod button;
mod button_icon;

pub use button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariant, ButtonVariants};
