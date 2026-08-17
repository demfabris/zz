#![deny(clippy::debug_assert_with_mut_call)]

//! Pure state, commands, layouts, key tables, and tmux-compatible parsing.

mod command;
mod layout;
#[cfg(test)]
mod layout_pin_tests;
mod model;
mod parser;
mod status;

pub use command::{
    AgentOptions, DEFAULT_BUFFER_LIMIT, DetachScope, Execution, ExecutionContext,
    MAX_WORD_SEPARATORS_BYTES, MuxEffect, MuxEngine,
};
pub use layout::{CellLayout, SplitSize};
pub use model::{
    LayoutPreset, MuxState, Pane, PaneDirection, PaneKind, Session, SplitPlacement, Window,
    joined_layout, swapped_layout,
};
pub use parser::{ConfigDiagnostic, ParsedConfig, parse_config};
pub use status::{
    DEFAULT_STATUS_INTERVAL, DEFAULT_STATUS_LEFT, DEFAULT_STATUS_RIGHT, StatusContext,
    StatusFormats, StatusHooks, StatusOption, expand_status,
};
pub use zz_protocol::{Binding, KeyDecision, KeyEngine, KeyTables, canonical_key};
pub use zz_protocol::{
    COMMAND_SPECS, CommandOptionSpec, CommandSpec, CommandValueKind, canonical_command,
    command_spec,
};
