//! The renderer-free client brain shared by every zz client.
//!
//! [`ClientCore`] is a sans-IO state machine: decoded [`zz_protocol::ProtocolMessage`]s
//! go in, typed [`CoreEvent`]s and [`Outbound`] requests come out, and the
//! reduced state (snapshot, viewports, overlays, key tables) is read through
//! plain accessors. It owns no socket, spawns no thread, and reads no clock,
//! so a shell can drive it from any runtime — a gpui entity, a TUI reader
//! thread, a deterministic simulator, or a C caller behind FFI.

pub mod agent_completion;
pub mod agent_config;
pub mod agent_transcript;
mod chrome;
pub mod completion;
mod core;
mod layout;
mod menu;
pub mod navigation;
mod status;
mod status_bar;

pub use chrome::{
    BROWSER_TABLE, CHROME_TABLES, ChromeAction, ChromeKey, ChromeKeymap, ChromeProfile,
    SIDEBAR_TABLE, TERMINAL_TABLE, UI_TABLE, UnknownChromeAction,
};
pub use core::{
    AgentAttentionEdge, AgentAttentionStatus, ClientCore, CoreEvent, Outbound, ViewportDamage,
    agent_attention_status,
};
pub use layout::{
    DropZone, NormalizedPaneRect, PaneRect, coerced_drop_zone, drop_preview_bounds, drop_zone_at,
    pane_box, pane_drop_command, pane_join_command, pane_rects, pane_swap_command,
    predicted_drop_layout, same_arrangement,
};
pub use menu::{
    MOUSE_BUTTON_1, MenuBox, MenuKeyResult, MenuPasteResult, MenuPointerKind, resolve_menu_key,
    resolve_menu_mouse, resolve_menu_paste,
};
pub use status::{ComposedStatusRow, StatusHitRange, compose_status_row, compose_status_row_over};
pub use status_bar::{
    StatusBarAgent, StatusBarModel, StatusBarPane, StatusBarSettings, StatusBarWindow,
};

pub mod chrome_palette;
pub mod url_input;
