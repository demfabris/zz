//! Cross-platform terminal session and renderer-neutral snapshots.
#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests spawn helper processes from threads with an empty signal mask"
    )
)]

mod appearance;
mod input;
mod interaction;
mod model;
mod paste;
#[cfg(all(feature = "session", target_os = "macos"))]
pub mod posix_spawn;
#[cfg(feature = "session")]
mod program_status;
#[cfg(all(feature = "session", unix))]
mod pty_types;
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
    ClipboardTarget, CopyJump, CopyJumpDirection, CopyModeAction, CopyModeCopy,
    CopyModeCountPolicy, CopyModeSearch, CopySelectionMode, PasteBufferAction, PointerCellEvent,
    SearchCase, SearchDirection, SearchMode, SearchQuery, TerminalMouseButton, TerminalMouseInput,
    TerminalMousePhase, TerminalViewAction, TerminalViewId,
};
pub use model::ColourClass;
pub use model::{
    ATTR_BLINK, ATTR_BOLD, ATTR_EXPLICIT_RGB, ATTR_FAINT, ATTR_HYPERLINK, ATTR_INVISIBLE,
    ATTR_ITALIC, ATTR_OVERLINE, ATTR_STRIKETHROUGH, CellWidth, Color, Cursor, CursorStyle,
    DEFAULT_HISTORY_LIMIT, GRAPHEME_TABLE_BIT, Glyph, IMAGE_PLACEHOLDER_SCHEME, KittyLayer,
    KittyPlacement, MAX_HISTORY_LIMIT, MAX_KITTY_IMAGE_BYTES, MAX_KITTY_PLACEMENTS, NO_COLOR,
    OVERLAY_RECTANGLE, OverlayKind, OverlaySpan, PackedCell, PackedStyle, PatchError,
    ScrollbarState, SearchStatus, SessionStatus, TerminalDictionary, TerminalDictionaryPatch,
    TerminalDiffScratch, TerminalExitStatus, TerminalMode, TerminalPatchFields, TerminalPatchRef,
    TerminalPatchRows, TerminalPatchSpan, TerminalPatchSpans, TerminalPresentation,
    TerminalViewport, TerminalViewportPatch, UnderlineStyle, exposed_rows_are_replaced,
    shared_default_presentation, shared_empty_kitty_placements, shared_empty_overlays,
};
pub use paste::{PastePreparationError, prepare_paste_buffer};
#[cfg(feature = "session")]
pub use program_status::{
    MAX_PROGRAM_STATUS_RECORDS, ProgramBlockKind, ProgramState, ProgramStatus, ProgramStatusRecord,
};
#[cfg(all(feature = "session", target_os = "linux"))]
pub use session::disable_transparent_huge_pages;
#[cfg(all(feature = "session", unix))]
pub use session::release_held_wakes;
#[cfg(feature = "session")]
pub use session::{
    CaptureBoundary, CaptureOptions, CapturedCopySource, CopyModeFacts, CopyModeSelectionFacts,
    copy_line_number_mode,
    DeferredTerminalEvent, EngineKnobs, KittyImage, KittyImageRequestError, LastCommandCapture,
    MAX_LAST_COMMAND_BYTES, MAX_LAST_COMMAND_LINES, OutputWake, PRIVATE_MODE_NUMBERS,
    PaneOutputFacts, PointerContext, ProgressBar, ProgressBarState, RoundTripGuard,
    TerminalCaptureError, TerminalCopyReady, TerminalEvent, TerminalEvents, TerminalFacts,
    TerminalFrameSink, TerminalProcessExit, TerminalRequest, TerminalRequestError, TerminalSession,
    TerminalSessionDiagnostics, TerminalSize, TerminalSpawn, ViewFrame, ViewStream, WakeHold,
    allow_actor_round_trips, forbid_actor_round_trips, hold_actor_wakes, run_pty_exec_mode,
};
pub use word::{DEFAULT_WORD_SEPARATORS, WordSeparators};
