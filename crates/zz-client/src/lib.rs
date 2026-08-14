//! The renderer-free client brain shared by every zz client.
//!
//! [`ClientCore`] is a sans-IO state machine: decoded [`zz_protocol::ProtocolMessage`]s
//! go in, typed [`CoreEvent`]s and [`Outbound`] requests come out, and the
//! reduced state (snapshot, viewports, overlays, key tables) is read through
//! plain accessors. It owns no socket, spawns no thread, and reads no clock,
//! so a shell can drive it from any runtime — a gpui entity, a TUI reader
//! thread, a deterministic simulator, or a C caller behind FFI.

mod core;

pub use core::{ClientCore, CoreEvent, Outbound, ViewportDamage};
