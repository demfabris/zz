//! Versioned, renderer-neutral protocol shared by zz clients and the daemon.

mod catalog;
mod framing;
mod id;
mod key;
mod message;
mod snapshot;
mod terminal_codec;

pub use catalog::{
    COMMAND_SPECS, CommandOptionSpec, CommandSpec, CommandValueKind, canonical_command,
    command_spec,
};
pub use framing::{MAX_ENCODED_FRAME_BYTES, MAX_FRAME_BYTES, ProtocolError};
pub use id::{ClientId, PaneId, SessionId, SplitId, WindowId};
pub use key::{Binding, KeyDecision, KeyEngine, KeyTables, canonical_key};
pub use message::{
    AgentCommand, BrowserCommand, ChooseBufferAction, ChooseBufferItem, ChooseBufferSearchState,
    ChooseBufferState, ChooseTreeAction, ChooseTreeItem, ChooseTreeKind, ChooseTreePaneKind,
    ChooseTreeSearchState, ChooseTreeState, ChooseTreeTarget, ClientHello, ClientKind,
    ClientMessageKind, CommandInvocation, CommandPromptAction, CommandPromptKind,
    CommandPromptState, CommandRequest, CommandResponse, ConfigOverrideEntry, DisplayPanesAction,
    DisplayPanesState, Event, EventPayload, GuiResponse, InputMessage, KeyBindingSnapshot,
    KeyTableSnapshot, KeyToken, MAX_AGENT_SEND_BYTES, MAX_CHOOSE_BUFFER_QUERY_BYTES,
    MAX_CHOOSE_TREE_QUERY_BYTES, MAX_COMMAND_PROMPT_BYTES, MAX_GUI_TEXT_BYTES,
    MAX_PASTE_UPLOAD_BYTES, MAX_PASTE_UPLOAD_CHUNK_BYTES, MAX_PASTE_UPLOAD_EXTENSION_BYTES,
    MAX_STATUS_TEXT_BYTES, MuxOptionKey, MuxOptionSource, MuxOptionValue, MuxOptions,
    NEW_SESSION_ATTACH_CAPABILITY, PROTOCOL_VERSION, PaneIndicator, PasteUploadPurpose,
    PastedImageFormat, ProtocolMessage, SPLIT_RATIO_BASIS, ServerError, ServerHello, SourceSpan,
    StatusLine, TerminalUiCommand, paste_upload_extension_is_valid,
};
pub use snapshot::{
    AgentDescriptor, AgentProvider, Axis, BrowserDescriptor, BrowserProfileNameError,
    DEFAULT_BROWSER_PROFILE, EditorDescriptor, EditorDescriptorError, LayoutNode,
    MAX_BROWSER_PROFILE_NAME_BYTES, MAX_EDITOR_PATH_BYTES, MuxSnapshot, PaneKindSnapshot,
    PaneSnapshot, SessionSnapshot, SessionViewer, WindowSnapshot, normalize_browser_profile_name,
};
pub use terminal_codec::{
    decode_protocol_frame, encode_protocol_message, encode_protocol_message_into,
    encode_terminal_viewport_event, encode_terminal_viewport_event_into, read_protocol_message,
    read_protocol_message_into, write_protocol_message,
};
