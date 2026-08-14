//! A GTK4/libadwaita client for the zz daemon.
//!
//! [`engine`] owns the socket, the [`zz_client::ClientCore`] reduction and the
//! reader thread and imports nothing from GTK, so the protocol path is testable
//! without a display. [`ui`] is the only half that touches widgets.

pub mod config;
pub mod engine;
pub mod ui;
