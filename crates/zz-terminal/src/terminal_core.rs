//! Cross-platform terminal session and renderer-neutral snapshots.

mod appearance;
mod input;
mod interaction;
mod model;
mod paste;
#[cfg(feature = "session")]
mod session;
#[cfg(feature = "session")]
mod shell_integration;
mod word;

pub use appearance::{
    AppearanceColor, AppearanceConfigDiagnostic, AppearanceConfigDisposition, AppearanceConfigKey,
    AppearanceLoad, AppearanceProvenance, AppearanceSource, AppearanceValidationError,
    CellHeightAdjustment, CursorBlinkPolicy, FontFeature, FontSyntheticStyle, GhosttyTheme,
    TerminalAppearance, TerminalColorScheme, TerminalPalette, apply_appearance_overrides,
    discover_ghostty_config, enumerate_ghostty_themes_for, load_ghostty_appearance,
    load_ghostty_appearance_for, load_ghostty_appearance_for_with_overrides,
    load_ghostty_appearance_from, load_ghostty_appearance_from_for,
    load_ghostty_appearance_from_for_with_overrides, parse_x11_color,
};
pub use input::{KeyAction, KeyCode, KeyInput, Modifiers};
pub use interaction::{
    ClipboardTarget, CopyJump, CopyJumpDirection, CopyModeAction, CopyModeCopy, PasteBufferAction,
    PointerCellEvent, SearchCase, SearchDirection, SearchMode, SearchQuery, TerminalMouseButton,
    TerminalMouseInput, TerminalMousePhase, TerminalViewAction, TerminalViewId,
};
pub use model::{
    ATTR_BLINK, ATTR_BOLD, ATTR_EXPLICIT_RGB, ATTR_FAINT, ATTR_HYPERLINK, ATTR_INVISIBLE,
    ATTR_ITALIC, ATTR_OVERLINE, ATTR_STRIKETHROUGH, CellWidth, Color, Cursor, CursorStyle,
    DEFAULT_HISTORY_LIMIT, GRAPHEME_TABLE_BIT, Glyph, IMAGE_PLACEHOLDER_SCHEME, KittyLayer,
    KittyPlacement, MAX_HISTORY_LIMIT, MAX_KITTY_IMAGE_BYTES, NO_COLOR, OVERLAY_RECTANGLE,
    OverlayKind, OverlaySpan, PackedCell, PackedStyle, PatchError, ScrollbarState, SearchStatus,
    SessionStatus, TerminalDictionary, TerminalDictionaryPatch, TerminalDiffScratch,
    TerminalExitStatus, TerminalMode, TerminalPatchRowIndices, TerminalPatchRows,
    TerminalPresentation, TerminalViewport, TerminalViewportPatch, UnderlineStyle,
};
pub use paste::{PastePreparationError, prepare_paste_buffer};
#[cfg(feature = "session")]
pub use session::{
    CaptureBoundary, CaptureOptions, KittyImage, KittyImageRequestError, LastCommandCapture,
    MAX_LAST_COMMAND_BYTES, MAX_LAST_COMMAND_LINES, RawOutputTapError, TerminalCaptureError,
    TerminalCopyReady, TerminalEvent, TerminalEvents, TerminalSession, TerminalSessionDiagnostics,
    TerminalSpawn,
};
pub use word::{DEFAULT_WORD_SEPARATORS, WordSeparators};
