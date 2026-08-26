//! Versioned, renderer-neutral protocol shared by zz clients and the daemon.

mod catalog;
mod framing;
mod id;
mod key;
mod message;
mod snapshot;
mod style;
mod terminal_codec;

pub use catalog::{
    COMMAND_ARGS_PARSE_BEHAVES, COMMAND_ARGS_PARSE_SPECS, COMMAND_SPECS, CommandArgsParseRule,
    CommandArgsParseSpec, CommandOptionSpec, CommandResolution, CommandSpec, CommandValueKind,
    DAEMON_COMMAND_SPECS, NATIVE_COMMAND_NAMES, canonical_command, catalog_command_spec,
    command_spec, command_specs, resolve_command,
};
pub use framing::{MAX_ENCODED_FRAME_BYTES, MAX_FRAME_BYTES, ProtocolError};
pub use id::{ClientId, ClientInstanceId, PaneId, SessionId, SplitId, WindowId};
pub use key::{
    Binding, KeyDecision, KeyEngine, KeyName, KeyTables, canonical_key, input_key_name,
    input_typed_text, is_key_name,
};
pub use message::{
    AgentCommand, AgentConnectionPhase, AgentGitSummary, AgentImage, AgentPaneWire,
    AgentPermissionWire, AgentSessionOpKind, BrowserCommand, ChooseBufferAction, ChooseBufferItem,
    ChooseBufferSearchState, ChooseBufferState, ChooseTreeAction, ChooseTreeItem, ChooseTreeKind,
    ChooseTreePaneKind, ChooseTreeSearchState, ChooseTreeState, ChooseTreeTarget, ClientHello,
    ClientKind, ClientMessageKind, CommandInvocation, CommandPromptAction, CommandPromptKind,
    CommandPromptMode, CommandPromptState, CommandPromptType, CommandRequest, CommandResponse,
    ConfigOverrideEntry, ConfirmAction, ConfirmState, ControlSourceFileEvent,
    DEFAULT_AGENT_AUTO_APPROVE, DEFAULT_AGENT_CLAUDE_CODE_COMMAND, DEFAULT_AGENT_COMMAND,
    DisplayPanesAction, DisplayPanesState, Event, EventPayload, GuiResponse, InputMessage,
    KeyBindingSnapshot, KeyTableSnapshot, KeyToken, MAX_AGENT_AUTH_METHODS,
    MAX_AGENT_AVAILABLE_COMMANDS, MAX_AGENT_COMMAND_BYTES, MAX_AGENT_CONFIG_CHOICES,
    MAX_AGENT_CONFIG_OPTIONS, MAX_AGENT_IMAGE_FORMAT_BYTES, MAX_AGENT_MODES,
    MAX_AGENT_OPTION_BYTES, MAX_AGENT_PERMISSION_BYTES, MAX_AGENT_PERMISSION_OPTIONS,
    MAX_AGENT_PROMPT_BYTES, MAX_AGENT_PROMPT_IMAGES, MAX_AGENT_QUEUED_PROMPTS,
    MAX_AGENT_RESULT_BYTES, MAX_AGENT_SEND_BYTES, MAX_AGENT_SESSION_DIRECTORIES,
    MAX_AGENT_SESSION_ID_BYTES, MAX_AGENT_STATE_BLOB_BYTES, MAX_AGENT_TOOL_CONTENT_ITEMS,
    MAX_AGENT_UPDATES_BYTES, MAX_BROWSER_KEY_REPEAT, MAX_CHOOSE_BUFFER_QUERY_BYTES,
    MAX_CHOOSE_ITEM_KEY_BYTES, MAX_CHOOSE_TREE_QUERY_BYTES, MAX_CLIENT_WORKING_DIRECTORY_BYTES,
    MAX_COMMAND_PROMPT_BYTES, MAX_GUI_TEXT_BYTES, MAX_PANE_INDICATOR_LABEL_BYTES,
    MAX_PASTE_UPLOAD_BYTES, MAX_PASTE_UPLOAD_CHUNK_BYTES, MAX_PASTE_UPLOAD_EXTENSION_BYTES,
    MAX_STATUS_ROWS, MAX_STATUS_TEXT_BYTES, MenuAction, MenuItem, MenuState, MuxOptionKey,
    MuxOptionSource, MuxOptionValue, MuxOptions, NEW_SESSION_ATTACH_CAPABILITY, PROTOCOL_VERSION,
    PaneIndicator, PasteUploadPurpose, PastedImageFormat, PopupAction, PopupBorderLines,
    PopupState, PreparedCommand, PreparedCommandResult, ProtocolMessage, SPLIT_RATIO_BASIS,
    ServerError, ServerHello, SourceSpan, StatusLine, StatusPosition, TerminalUiCommand,
    agent_update_batch_bytes, paste_upload_extension_is_valid, split_command_words,
};
pub use snapshot::{
    AgentDescriptor, AgentProvider, Axis, BrowserDescriptor, BrowserProfileNameError,
    DEFAULT_BROWSER_PROFILE, EditorDescriptor, EditorDescriptorError, LayoutNode,
    MAX_BROWSER_PROFILE_NAME_BYTES, MAX_EDITOR_PATH_BYTES, MAX_WINDOW_STATUS_LABEL_BYTES,
    MuxSnapshot, PaneKindSnapshot, PaneSnapshot, SessionSnapshot, SessionViewer, WindowSnapshot,
    normalize_browser_profile_name,
};
pub use style::{
    StyledSegment, TmuxAlign, TmuxAttributeState, TmuxAttributes, TmuxColour, TmuxDefaultType,
    TmuxList, TmuxRange, TmuxStyle, TmuxWidth, apply_style, parse_style, parse_styled_segments,
    parse_tmux_colour, valid_style,
};
pub use terminal_codec::{
    decode_protocol_frame, encode_protocol_message, encode_protocol_message_into,
    encode_terminal_viewport_event, encode_terminal_viewport_event_into, read_protocol_message,
    read_protocol_message_into, write_protocol_message,
};
