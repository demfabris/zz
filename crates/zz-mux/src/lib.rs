#![deny(clippy::debug_assert_with_mut_call)]

//! Pure state, commands, layouts, key tables, and tmux-compatible parsing.

mod command;
#[cfg(test)]
mod compat_manifest_tests;
mod copy_actions;
mod formats;
mod honest_knobs;
mod layout;
#[cfg(test)]
mod layout_pin_tests;
mod model;
mod parser;
mod sort;
mod status;
mod terminfo;
mod tmux_options;

pub use command::TMUX_OPTION_CONSUMERS as BEHAVES;
pub use command::{
    AgentOptions, CommandAliasBodyError, CommandAliasResolution, CommandPromptStep,
    CommandPromptTemplate, CopyModeStyleValues, DEFAULT_BUFFER_LIMIT, DetachRequest, DetachScope,
    Execution, ExecutionContext, FormatFacts, MAX_WORD_SEPARATORS_BYTES, MenuOptions, MuxEffect,
    MuxEngine, PaneBorderStyleValues, PaneRuntimeFacts, PopupOptions, RetainedJobEnvironment,
    StatusRowVariables, TMUX_OPTION_CONSUMERS, TerminalWorkerOptions, WindowStyleValues,
    copy_mode_action_is_read_only_safe, format_command, hook_format_variables, if_shell_truthy,
    parse_tmux_key, send_keys_is_read_only_safe, send_keys_target_client,
    validate_static_command_chain,
};
#[doc(hidden)]
pub use command::{
    accepted_native_literal_format_context_scopes, missing_derived_format_context_families,
    missing_literal_format_context_scopes, mux_derived_format_context_families,
    mux_literal_format_context_scopes,
};
pub use copy_actions::{
    CopyActionCategory, PINNED_COPY_MODE_ACTIONS, PinnedCopyAction, copy_mode_action_is_mapped,
    missing_copy_mode_actions, pinned_copy_action,
};
pub use formats::{
    FormatClient, FormatClientRow, FormatEnvironRow, TmuxColour, delegated_format_variable_names,
    display_width, format_true, indexed_colour_rgb, parse_tmux_colour,
};
pub use honest_knobs::{BellAction, PresetOptions, VisualBell, WindowSize};
pub use layout::{CellLayout, SplitSize};
pub use model::{
    LayoutPreset, MuxState, Pane, PaneDirection, PaneKind, Session, SplitPlacement, Window,
    joined_layout, swapped_layout,
};
pub use parser::{
    ConfigCommandBytes, ConfigDiagnostic, ConfigEnvironmentAssignmentBytes, ParsedConfig,
    ParsedConfigBytes, command_block_body, config_home_directory_names, parse_config,
    parse_config_with_home_directories, user_home,
};
pub use sort::{TmuxSort, TmuxSortOrder};
pub use status::{
    DEFAULT_STATUS_INTERVAL, DEFAULT_STATUS_LEFT, DEFAULT_STATUS_RIGHT, DEFAULT_STATUS_STYLE,
    DEFAULT_WINDOW_STATUS_FORMAT, FormatUniverse, StatusContext, StatusFormats, StatusHooks,
    StatusJustify, StatusOption, StatusPosition, WindowStatusFormats, WindowStatusOption,
    expand_format_values, expand_status,
};
pub use terminfo::TtyTerm;
pub use zz_protocol::{Binding, KeyDecision, KeyEngine, KeyTables, canonical_key};
pub use zz_protocol::{
    COMMAND_SPECS, CommandOptionSpec, CommandSpec, CommandValueKind, canonical_command,
    command_spec,
};
pub use zz_protocol::{
    StyledSegment, TmuxAlign, TmuxAttributeState, TmuxAttributes, TmuxDefaultType, TmuxList,
    TmuxRange, TmuxStyle, TmuxWidth, parse_style, parse_styled_segments, valid_style,
};
