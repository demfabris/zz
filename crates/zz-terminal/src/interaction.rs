use serde::{Deserialize, Serialize};

use crate::Modifiers;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TerminalViewId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerCellEvent {
    pub column: u16,
    pub row: u16,
    pub click_count: u8,
    pub rectangle: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalMousePhase {
    Press,
    Release,
    Motion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalMouseButton {
    Left,
    Middle,
    Right,
    ScrollUp,
    ScrollDown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TerminalMouseInput {
    pub x: u32,
    pub y: u32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub cell: PointerCellEvent,
    routing: u16,
}

#[allow(
    clippy::missing_fields_in_debug,
    reason = "debug output preserves the logical public fields instead of exposing packed routing storage"
)]
impl std::fmt::Debug for TerminalMouseInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalMouseInput")
            .field("phase", &self.phase())
            .field("button", &self.button())
            .field("cell", &self.cell)
            .field("x", &self.x)
            .field("y", &self.y)
            .field("screen_width", &self.screen_width)
            .field("screen_height", &self.screen_height)
            .field("cell_width", &self.cell_width)
            .field("cell_height", &self.cell_height)
            .field("modifiers", &self.modifiers())
            .field("force_selection", &self.force_selection())
            .finish()
    }
}

impl TerminalMouseInput {
    const PHASE_MASK: u16 = 0b11;
    const BUTTON_SHIFT: u32 = 2;
    const BUTTON_MASK: u16 = 0b111;
    const MODIFIERS_SHIFT: u32 = 5;
    const MODIFIERS_MASK: u16 = 0b1111;
    const FORCE_SELECTION: u16 = 1 << 9;

    /// Build one compact terminal pointer event.
    #[must_use]
    pub const fn new(
        phase: TerminalMousePhase,
        button: Option<TerminalMouseButton>,
        cell: PointerCellEvent,
        x: u32,
        y: u32,
        screen_width: u32,
        screen_height: u32,
        cell_width: u32,
        cell_height: u32,
        modifiers: Modifiers,
        force_selection: bool,
    ) -> Self {
        let phase = match phase {
            TerminalMousePhase::Press => 0,
            TerminalMousePhase::Release => 1,
            TerminalMousePhase::Motion => 2,
        };
        let button = match button {
            None => 0,
            Some(TerminalMouseButton::Left) => 1,
            Some(TerminalMouseButton::Middle) => 2,
            Some(TerminalMouseButton::Right) => 3,
            Some(TerminalMouseButton::ScrollUp) => 4,
            Some(TerminalMouseButton::ScrollDown) => 5,
        };
        Self {
            x,
            y,
            screen_width,
            screen_height,
            cell_width,
            cell_height,
            cell,
            routing: phase
                | button << Self::BUTTON_SHIFT
                | (modifiers.bits() as u16) << Self::MODIFIERS_SHIFT
                | if force_selection {
                    Self::FORCE_SELECTION
                } else {
                    0
                },
        }
    }

    #[must_use]
    pub const fn phase(self) -> TerminalMousePhase {
        match self.routing & Self::PHASE_MASK {
            1 => TerminalMousePhase::Release,
            2 => TerminalMousePhase::Motion,
            _ => TerminalMousePhase::Press,
        }
    }

    #[must_use]
    pub const fn button(self) -> Option<TerminalMouseButton> {
        match (self.routing >> Self::BUTTON_SHIFT) & Self::BUTTON_MASK {
            1 => Some(TerminalMouseButton::Left),
            2 => Some(TerminalMouseButton::Middle),
            3 => Some(TerminalMouseButton::Right),
            4 => Some(TerminalMouseButton::ScrollUp),
            5 => Some(TerminalMouseButton::ScrollDown),
            _ => None,
        }
    }

    #[must_use]
    pub const fn modifiers(self) -> Modifiers {
        let bits = ((self.routing >> Self::MODIFIERS_SHIFT) & Self::MODIFIERS_MASK) as u8;
        Modifiers::from_packed_bits(bits)
    }

    #[must_use]
    pub const fn force_selection(self) -> bool {
        self.routing & Self::FORCE_SELECTION != 0
    }
}

#[derive(Serialize, Deserialize)]
struct TerminalMouseInputWire {
    phase: TerminalMousePhase,
    button: Option<TerminalMouseButton>,
    cell: PointerCellEvent,
    x: u32,
    y: u32,
    screen_width: u32,
    screen_height: u32,
    cell_width: u32,
    cell_height: u32,
    modifiers: Modifiers,
    force_selection: bool,
}

impl Serialize for TerminalMouseInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        TerminalMouseInputWire {
            phase: self.phase(),
            button: self.button(),
            cell: self.cell,
            x: self.x,
            y: self.y,
            screen_width: self.screen_width,
            screen_height: self.screen_height,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
            modifiers: self.modifiers(),
            force_selection: self.force_selection(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TerminalMouseInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TerminalMouseInputWire::deserialize(deserializer)?;
        Ok(Self::new(
            wire.phase,
            wire.button,
            wire.cell,
            wire.x,
            wire.y,
            wire.screen_width,
            wire.screen_height,
            wire.cell_width,
            wire.cell_height,
            wire.modifiers,
            wire.force_selection,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipboardTarget {
    Clipboard,
    Primary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasteBufferAction {
    Create { prefix: Option<String> },
    Append,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyModeCopy {
    pub request_id: u64,
    pub clipboard: bool,
    pub buffer: Option<PasteBufferAction>,
    pub pipe: Option<String>,
    pub clear_selection: bool,
    pub cancel: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyJumpDirection {
    Forward,
    Backward,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyJump {
    pub target: String,
    pub direction: CopyJumpDirection,
    pub to: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMode {
    #[default]
    Literal,
    Regex,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchCase {
    #[default]
    Smart,
    Sensitive,
    Insensitive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchDirection {
    #[default]
    Forward,
    Backward,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub mode: SearchMode,
    pub case: SearchCase,
    pub direction: SearchDirection,
}

impl SearchQuery {
    #[must_use]
    pub fn literal(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            mode: SearchMode::Literal,
            case: SearchCase::Smart,
            direction: SearchDirection::Forward,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyModeAction {
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    Top,
    Bottom,
    TopLine,
    MiddleLine,
    BottomLine,
    StartOfLine,
    BackToIndentation,
    EndOfLine,
    NextWord,
    PreviousWord,
    NextWordEnd,
    NextParagraph,
    PreviousParagraph,
    NextPrompt {
        output: bool,
    },
    PreviousPrompt {
        output: bool,
    },
    SearchAgain {
        reverse: bool,
    },
    StartSelection,
    SelectWord,
    SelectLine,
    ClearSelection,
    /// Clear an active selection, or leave copy mode when there is none.
    ClearSelectionOrCancel,
    ToggleRectangle,
    RectangleOn,
    RectangleOff,
    OtherEnd,
    SetMark,
    JumpToMark,
    Jump(CopyJump),
    RepeatJump {
        reverse: bool,
    },
    CopySelection(Box<CopyModeCopy>),
    Cancel,
    NextSpace,
    PreviousSpace,
    NextSpaceEnd,
    ScrollUp,
    ScrollDown,
    ScrollMiddle,
    NextMatchingBracket,
    SearchCursorWord {
        direction: SearchDirection,
    },
    CopyEndOfLine(Box<CopyModeCopy>),
    GotoLine(u32),
    PageDownScrollExit,
    SelectionMode(CopySelectionMode),
    /// Stop the selection following the cursor without clearing it.
    StopSelection,
    HalfPageDownScrollExit,
    /// Scroll one row toward the bottom and leave copy mode once the viewport
    /// lands there, selection or not.
    ScrollDownAndCancel,
    /// Move the cursor down and leave copy mode only when the whole run was
    /// stuck at the bottom of the retained history.
    CursorDownAndCancel,
    ScrollExitOn,
    ScrollExitOff,
    ScrollExitToggle,
    CursorCentreVertical,
    CursorCentreHorizontal,
    ScrollTop,
    ScrollBottom,
    /// Flip the mode's position readout, the pin's `hide_position`.
    TogglePosition,
    RecentreTopBottom,
    PreviousMatchingBracket,
    /// `refresh-on`: start re-cloning the frozen backing from the live pane.
    RefreshOn,
    /// `refresh-off`: stop re-cloning it.
    RefreshOff,
    RefreshToggle,
    /// One tick of the refresh timer, which is the daemon's here. It is not a
    /// pinned action name; `window_copy_refresh_timer` is a libevent timer.
    RefreshRevision,
    /// `copy-line` and its pipe and cancel spellings: `window_copy_do_copy_line`
    /// selects the whole logical line the cursor sits on without needing a
    /// selection, copies it, then puts the cursor and view back.
    CopyLine(Box<CopyModeCopy>),
}

/// The pin's `selflag`: the unit a live selection extends by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopySelectionMode {
    #[default]
    Char,
    Word,
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyModeCountPolicy {
    Repeat,
    OtherEnd,
    SelectLine,
    CopyEndOfLine,
    CopyLine,
    CursorDownAndCancel,
    Once,
}

impl CopyModeAction {
    #[must_use]
    pub fn copy_selection(copy: CopyModeCopy) -> Self {
        Self::CopySelection(Box::new(copy))
    }

    #[must_use]
    pub fn copy_end_of_line(copy: CopyModeCopy) -> Self {
        Self::CopyEndOfLine(Box::new(copy))
    }

    #[must_use]
    pub fn count_policy(&self) -> CopyModeCountPolicy {
        match self {
            Self::Left
            | Self::Right
            | Self::Up
            | Self::Down
            | Self::PageUp
            | Self::PageDown
            | Self::PageDownScrollExit
            | Self::HalfPageUp
            | Self::HalfPageDown
            | Self::ScrollUp
            | Self::ScrollDown
            | Self::NextWord
            | Self::PreviousWord
            | Self::NextWordEnd
            | Self::NextSpace
            | Self::PreviousSpace
            | Self::NextSpaceEnd
            | Self::NextParagraph
            | Self::PreviousParagraph
            | Self::NextMatchingBracket
            | Self::PreviousMatchingBracket
            | Self::HalfPageDownScrollExit
            | Self::ScrollDownAndCancel
            | Self::Jump(_)
            | Self::RepeatJump { .. }
            | Self::SearchAgain { .. } => CopyModeCountPolicy::Repeat,
            Self::OtherEnd => CopyModeCountPolicy::OtherEnd,
            Self::SelectLine => CopyModeCountPolicy::SelectLine,
            Self::CopyEndOfLine(_) => CopyModeCountPolicy::CopyEndOfLine,
            Self::CopyLine(_) => CopyModeCountPolicy::CopyLine,
            Self::CursorDownAndCancel => CopyModeCountPolicy::CursorDownAndCancel,
            Self::Top
            | Self::Bottom
            | Self::TopLine
            | Self::MiddleLine
            | Self::BottomLine
            | Self::StartOfLine
            | Self::BackToIndentation
            | Self::EndOfLine
            | Self::NextPrompt { .. }
            | Self::PreviousPrompt { .. }
            | Self::StartSelection
            | Self::SelectWord
            | Self::ClearSelection
            | Self::ClearSelectionOrCancel
            | Self::ToggleRectangle
            | Self::RectangleOn
            | Self::RectangleOff
            | Self::SetMark
            | Self::JumpToMark
            | Self::CopySelection(_)
            | Self::Cancel
            | Self::ScrollMiddle
            | Self::SearchCursorWord { .. }
            | Self::SelectionMode(_)
            | Self::StopSelection
            | Self::ScrollExitOn
            | Self::ScrollExitOff
            | Self::ScrollExitToggle
            | Self::CursorCentreVertical
            | Self::CursorCentreHorizontal
            | Self::ScrollTop
            | Self::ScrollBottom
            | Self::TogglePosition
            | Self::RecentreTopBottom
            | Self::RefreshOn
            | Self::RefreshOff
            | Self::RefreshToggle
            | Self::RefreshRevision
            | Self::GotoLine(_) => CopyModeCountPolicy::Once,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalViewAction {
    /// Scroll by logical rows. Negative values move toward older history.
    ScrollLines(i32),
    /// Scroll by terminal pages. Negative values move toward older history.
    ScrollPages(i32),
    ScrollTop,
    ScrollBottom,
    /// Move to an absolute scrollbar fraction (`0` is top, `u32::MAX` bottom).
    ScrollToFraction(u32),
    SelectionPress(PointerCellEvent),
    SelectionDrag(PointerCellEvent),
    SelectionAutoscroll {
        lines: i32,
        pointer: PointerCellEvent,
    },
    SelectionRelease(PointerCellEvent),
    Mouse(TerminalMouseInput),
    /// Clear presentation-only URI hover without synthesizing application mouse input.
    ClearLinkHover,
    ScrollWheel {
        lines: i32,
        input: TerminalMouseInput,
    },
    SelectAll,
    ClearHistory,
    ClearSelection,
    EnterCopyMode,
    CopyMode(CopyModeAction),
    SearchBegin(SearchQuery),
    SearchUpdate(SearchQuery),
    SearchNext,
    SearchPrevious,
    SearchClose,
    CopySelection {
        request_id: u64,
        target: ClipboardTarget,
    },
    Focus(bool),
    Paste(String),
    /// Move the live viewport top to an absolute scrollbar offset.
    ScrollToOffset(u32),
    EnterCopyModeScrollExit,
    /// Enter copy mode with `-e` and `-H` composed. Handled identically to the
    /// composition of [`Self::EnterCopyMode`] and
    /// [`Self::EnterCopyModeScrollExit`]; nothing produces it yet.
    EnterCopyModeWith {
        scroll_exit: bool,
        hide_position: bool,
    },
    CopyModeCounted {
        action: CopyModeAction,
        count: u32,
    },
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn pointer_records_keep_their_packed_layout() {
        assert_eq!(size_of::<PointerCellEvent>(), 6);
        assert_eq!(align_of::<PointerCellEvent>(), align_of::<u16>());
        assert_eq!(size_of::<TerminalMouseInput>(), 32);
        assert_eq!(align_of::<TerminalMouseInput>(), align_of::<u32>());
    }

    #[test]
    fn terminal_mouse_routing_metadata_round_trips_every_variant() {
        let phases = [
            TerminalMousePhase::Press,
            TerminalMousePhase::Release,
            TerminalMousePhase::Motion,
        ];
        let buttons = [
            None,
            Some(TerminalMouseButton::Left),
            Some(TerminalMouseButton::Middle),
            Some(TerminalMouseButton::Right),
            Some(TerminalMouseButton::ScrollUp),
            Some(TerminalMouseButton::ScrollDown),
        ];
        let modifiers = Modifiers::new(true, true, true, true);
        for phase in phases {
            for button in buttons {
                for force_selection in [false, true] {
                    let input = TerminalMouseInput::new(
                        phase,
                        button,
                        PointerCellEvent {
                            column: 0,
                            row: 0,
                            click_count: 0,
                            rectangle: false,
                        },
                        0,
                        0,
                        1,
                        1,
                        1,
                        1,
                        modifiers,
                        force_selection,
                    );
                    assert_eq!(input.phase(), phase);
                    assert_eq!(input.button(), button);
                    assert_eq!(input.modifiers(), modifiers);
                    assert_eq!(input.force_selection(), force_selection);
                }
            }
        }
    }
}
