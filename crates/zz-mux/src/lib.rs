//! Pure state, commands, layouts, key tables, and tmux-compatible parsing.

mod command;
mod model;
mod parser;
mod status;

pub use command::{
    DEFAULT_BUFFER_LIMIT, Execution, ExecutionContext, MAX_WORD_SEPARATORS_BYTES, MuxEffect,
    MuxEngine,
};
pub use model::{
    LayoutPreset, MuxState, Pane, PaneDirection, PaneKind, Session, Window, joined_layout,
    swapped_layout,
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
