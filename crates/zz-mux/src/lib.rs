#![deny(clippy::debug_assert_with_mut_call)]

//! Pure state, commands, layouts, key tables, and tmux-compatible parsing.

mod command;
mod formats;
mod honest_knobs;
mod layout;
#[cfg(test)]
mod layout_pin_tests;
mod model;
mod parser;
mod sort;
mod status;
mod style;
mod tmux_options;

pub use command::{
    AgentOptions, DEFAULT_BUFFER_LIMIT, DetachScope, Execution, ExecutionContext,
    MAX_WORD_SEPARATORS_BYTES, MenuOptions, MuxEffect, MuxEngine, PaneRuntimeFacts, PopupOptions,
    hook_format_variables, if_shell_truthy,
};
pub use formats::{TmuxColour, display_width, format_true, indexed_colour_rgb, parse_tmux_colour};
pub use honest_knobs::{BellAction, PresetOptions, VisualBell, WindowSize};
pub use layout::{CellLayout, SplitSize};
pub use model::{
    LayoutPreset, MuxState, Pane, PaneDirection, PaneKind, Session, SplitPlacement, Window,
    joined_layout, swapped_layout,
};
pub use parser::{ConfigDiagnostic, ParsedConfig, command_block_body, parse_config};
pub use sort::{TmuxSort, TmuxSortOrder};
pub use status::{
    DEFAULT_STATUS_INTERVAL, DEFAULT_STATUS_LEFT, DEFAULT_STATUS_RIGHT, DEFAULT_STATUS_STYLE,
    DEFAULT_WINDOW_STATUS_FORMAT, FormatUniverse, StatusContext, StatusFormats, StatusHooks,
    StatusJustify, StatusOption, StatusPosition, WindowStatusFormats, WindowStatusOption,
    expand_format_values, expand_status,
};
pub use style::{
    StyledSegment, TmuxAlign, TmuxAttributeState, TmuxAttributes, TmuxDefaultType, TmuxList,
    TmuxRange, TmuxStyle, TmuxWidth, parse_style, parse_styled_segments, valid_style,
};
pub use zz_protocol::{Binding, KeyDecision, KeyEngine, KeyTables, canonical_key};
pub use zz_protocol::{
    COMMAND_SPECS, CommandOptionSpec, CommandSpec, CommandValueKind, canonical_command,
    command_spec,
};
