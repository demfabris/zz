use std::{
    borrow::Cow,
    num::{NonZeroU32, NonZeroU64},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// Marks a glyph as an index into [`TerminalDictionary::grapheme_bytes`].
pub const GRAPHEME_TABLE_BIT: u32 = 1 << 31;
/// Sentinel used for an optional packed RGB color.
pub const NO_COLOR: u32 = u32::MAX;

const CELL_WIDTH_MASK: u16 = 0b11;
const MAX_HOVERED_URI_BYTES: usize = 16 * 1024;

/// Scrollback rows a pane keeps, and the ceiling `history-limit` accepts.
pub const DEFAULT_HISTORY_LIMIT: usize = 10_000;
pub const MAX_HISTORY_LIMIT: usize = 1_000_000;
/// Largest decoded Kitty image zz will move across the daemon boundary.
pub const MAX_KITTY_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// Wire scheme for a recognized agent-CLI image placeholder (`zz-image://2`).
/// The daemon writes the URI; the client that holds the pixels resolves it.
pub const IMAGE_PLACEHOLDER_SCHEME: &str = "zz-image";

pub const ATTR_BOLD: u16 = 1 << 0;
pub const ATTR_ITALIC: u16 = 1 << 1;
pub const ATTR_FAINT: u16 = 1 << 2;
pub const ATTR_BLINK: u16 = 1 << 3;
pub const ATTR_INVISIBLE: u16 = 1 << 4;
pub const ATTR_STRIKETHROUGH: u16 = 1 << 5;
pub const ATTR_OVERLINE: u16 = 1 << 6;
pub const ATTR_EXPLICIT_RGB: u16 = 1 << 7;
pub const ATTR_HYPERLINK: u16 = 1 << 8;
pub const OVERLAY_RECTANGLE: u8 = 1 << 0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    #[must_use]
    pub const fn packed(self) -> u32 {
        (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }

    #[must_use]
    pub const fn from_packed(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: (value & 0xff) as u8,
        }
    }
}

#[repr(u16)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellWidth {
    #[default]
    Narrow = 0,
    Wide = 1,
    SpacerTail = 2,
    SpacerHead = 3,
}

impl CellWidth {
    const fn from_flags(flags: u16) -> Self {
        match flags & CELL_WIDTH_MASK {
            1 => Self::Wide,
            2 => Self::SpacerTail,
            3 => Self::SpacerHead,
            _ => Self::Narrow,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnderlineStyle {
    #[default]
    None = 0,
    Single = 1,
    Double = 2,
    Curly = 3,
    Dotted = 4,
    Dashed = 5,
}

/// The hot terminal cell record. Eight cells fit in a typical cache line.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackedCell {
    glyph: u32,
    style: u16,
    flags: u16,
}

impl PackedCell {
    pub const EMPTY: Self = Self {
        glyph: 0,
        style: 0,
        flags: 0,
    };

    #[must_use]
    pub const fn new(glyph: u32, style: u16, width: CellWidth) -> Self {
        Self {
            glyph,
            style,
            flags: width as u16,
        }
    }

    #[must_use]
    pub const fn from_raw(glyph: u32, style: u16, flags: u16) -> Self {
        Self {
            glyph,
            style,
            flags,
        }
    }

    #[must_use]
    pub const fn glyph(self) -> u32 {
        self.glyph
    }

    #[must_use]
    pub const fn style_id(self) -> u16 {
        self.style
    }

    #[must_use]
    pub const fn flags(self) -> u16 {
        self.flags
    }

    #[must_use]
    pub const fn width(self) -> CellWidth {
        CellWidth::from_flags(self.flags)
    }
}

/// A resolved, interned terminal style referenced by [`PackedCell::style_id`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackedStyle {
    foreground: u32,
    background: u32,
    underline_color: u32,
    attributes: u16,
    underline_kind: u8,
    reserved: u8,
}

impl PackedStyle {
    #[must_use]
    pub const fn new(
        foreground: Color,
        background: Color,
        underline_color: Option<Color>,
        attributes: u16,
        underline: UnderlineStyle,
    ) -> Self {
        Self {
            foreground: foreground.packed(),
            background: background.packed(),
            underline_color: match underline_color {
                Some(color) => color.packed(),
                None => NO_COLOR,
            },
            attributes,
            underline_kind: underline as u8,
            reserved: 0,
        }
    }

    #[must_use]
    pub const fn from_raw(
        foreground: u32,
        background: u32,
        underline_color: u32,
        attributes: u16,
        underline_kind: u8,
    ) -> Self {
        Self {
            foreground,
            background,
            underline_color,
            attributes,
            underline_kind,
            reserved: 0,
        }
    }

    #[must_use]
    pub const fn foreground_raw(self) -> u32 {
        self.foreground
    }

    #[must_use]
    pub const fn background_raw(self) -> u32 {
        self.background
    }

    #[must_use]
    pub const fn underline_color_raw(self) -> u32 {
        self.underline_color
    }

    #[must_use]
    pub const fn underline_kind_raw(self) -> u8 {
        self.underline_kind
    }

    #[must_use]
    pub const fn foreground(self) -> Color {
        Color::from_packed(self.foreground)
    }

    #[must_use]
    pub const fn background(self) -> Color {
        Color::from_packed(self.background)
    }

    #[must_use]
    pub const fn underline_color(self) -> Option<Color> {
        if self.underline_color == NO_COLOR {
            None
        } else {
            Some(Color::from_packed(self.underline_color))
        }
    }

    #[must_use]
    pub const fn attributes(self) -> u16 {
        self.attributes
    }

    #[must_use]
    pub const fn underline(self) -> UnderlineStyle {
        match self.underline_kind {
            1 => UnderlineStyle::Single,
            2 => UnderlineStyle::Double,
            3 => UnderlineStyle::Curly,
            4 => UnderlineStyle::Dotted,
            5 => UnderlineStyle::Dashed,
            _ => UnderlineStyle::None,
        }
    }

    #[must_use]
    pub const fn bold(self) -> bool {
        self.attributes & ATTR_BOLD != 0
    }

    #[must_use]
    pub const fn italic(self) -> bool {
        self.attributes & ATTR_ITALIC != 0
    }

    #[must_use]
    pub const fn faint(self) -> bool {
        self.attributes & ATTR_FAINT != 0
    }

    #[must_use]
    pub const fn blink(self) -> bool {
        self.attributes & ATTR_BLINK != 0
    }

    #[must_use]
    pub const fn invisible(self) -> bool {
        self.attributes & ATTR_INVISIBLE != 0
    }

    #[must_use]
    pub const fn strikethrough(self) -> bool {
        self.attributes & ATTR_STRIKETHROUGH != 0
    }

    #[must_use]
    pub const fn overline(self) -> bool {
        self.attributes & ATTR_OVERLINE != 0
    }

    #[must_use]
    pub const fn explicit_rgb(self) -> bool {
        self.attributes & ATTR_EXPLICIT_RGB != 0
    }

    #[must_use]
    pub const fn hyperlink(self) -> bool {
        self.attributes & ATTR_HYPERLINK != 0
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayKind {
    #[default]
    Selection = 0,
    SearchMatch = 1,
    SearchCurrent = 2,
    LinkHover = 3,
    CopyCursor = 4,
}

/// A half-open visual span. Overlays stay out of the hot cell plane.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlaySpan {
    pub row: u16,
    pub start: u16,
    pub end: u16,
    kind_and_flags: u16,
}

impl OverlaySpan {
    #[must_use]
    pub const fn new(row: u16, start: u16, end: u16, kind: OverlayKind) -> Self {
        Self::with_flags(row, start, end, kind, 0)
    }

    #[must_use]
    pub const fn with_flags(row: u16, start: u16, end: u16, kind: OverlayKind, flags: u8) -> Self {
        Self {
            row,
            start,
            end,
            kind_and_flags: kind as u16 | (flags as u16) << 8,
        }
    }

    #[must_use]
    pub const fn from_raw(row: u16, start: u16, end: u16, kind_and_flags: u16) -> Self {
        Self {
            row,
            start,
            end,
            kind_and_flags,
        }
    }

    #[must_use]
    pub const fn kind_and_flags(self) -> u16 {
        self.kind_and_flags
    }

    #[must_use]
    pub const fn kind(self) -> OverlayKind {
        match self.kind_and_flags & 0xff {
            1 => OverlayKind::SearchMatch,
            2 => OverlayKind::SearchCurrent,
            3 => OverlayKind::LinkHover,
            4 => OverlayKind::CopyCursor,
            _ => OverlayKind::Selection,
        }
    }

    #[must_use]
    pub const fn flags(self) -> u8 {
        (self.kind_and_flags >> 8) as u8
    }
}

/// Coarse paint layer for a Kitty graphics placement.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KittyLayer {
    BelowBg = 0,
    BelowText = 1,
    #[default]
    AboveText = 2,
}

/// Renderer-neutral geometry for one visible Kitty graphics placement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KittyPlacement {
    pub image_id: u32,
    pub image_generation: u64,
    pub layer: KittyLayer,
    pub viewport_col: i32,
    pub viewport_row: i32,
    pub absolute_row: u64,
    pub cell_offset_x: u32,
    pub cell_offset_y: u32,
    pub grid_cols: u32,
    pub grid_rows: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub source_rect: Option<(u32, u32, u32, u32)>,
}

const _: () = assert!(size_of::<PackedCell>() == 8);
const _: () = assert!(align_of::<PackedCell>() == align_of::<u32>());
const _: () = assert!(size_of::<PackedStyle>() == 16);
const _: () = assert!(align_of::<PackedStyle>() == align_of::<u32>());

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollbarState {
    pub total: u32,
    pub offset: u32,
    pub len: u32,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CursorStyle {
    Bar,
    #[default]
    Block,
    Underline,
    BlockHollow,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor(NonZeroU64);

impl Cursor {
    const COLUMN_MASK: u64 = u16::MAX as u64;
    const ROW_SHIFT: u32 = 16;
    const COLOR_SHIFT: u32 = 32;
    const STYLE_SHIFT: u32 = 56;
    const STYLE_MASK: u64 = 0b11;
    const VISIBLE: u64 = 1 << 58;
    const BLINKING: u64 = 1 << 59;
    const WIDE_TAIL: u64 = 1 << 60;
    const PRESENT: u64 = 1 << 63;

    #[must_use]
    pub const fn new(
        column: u16,
        row: u16,
        visible: bool,
        blinking: bool,
        at_wide_tail: bool,
        style: CursorStyle,
        color: Color,
    ) -> Self {
        let style = match style {
            CursorStyle::Bar => 0,
            CursorStyle::Block => 1,
            CursorStyle::Underline => 2,
            CursorStyle::BlockHollow => 3,
        };
        let flags = if visible { Self::VISIBLE } else { 0 }
            | if blinking { Self::BLINKING } else { 0 }
            | if at_wide_tail { Self::WIDE_TAIL } else { 0 };
        let raw = Self::PRESENT
            | column as u64
            | (row as u64) << Self::ROW_SHIFT
            | (color.packed() as u64) << Self::COLOR_SHIFT
            | style << Self::STYLE_SHIFT
            | flags;
        let Some(raw) = NonZeroU64::new(raw) else {
            panic!("cursor presence bit must be set")
        };
        Self(raw)
    }

    #[must_use]
    pub const fn column(self) -> u16 {
        (self.0.get() & Self::COLUMN_MASK) as u16
    }

    #[must_use]
    pub const fn row(self) -> u16 {
        ((self.0.get() >> Self::ROW_SHIFT) & Self::COLUMN_MASK) as u16
    }

    #[must_use]
    pub const fn visible(self) -> bool {
        self.0.get() & Self::VISIBLE != 0
    }

    #[must_use]
    pub const fn blinking(self) -> bool {
        self.0.get() & Self::BLINKING != 0
    }

    #[must_use]
    pub const fn at_wide_tail(self) -> bool {
        self.0.get() & Self::WIDE_TAIL != 0
    }

    #[must_use]
    pub const fn style(self) -> CursorStyle {
        match (self.0.get() >> Self::STYLE_SHIFT) & Self::STYLE_MASK {
            0 => CursorStyle::Bar,
            1 => CursorStyle::Block,
            2 => CursorStyle::Underline,
            _ => CursorStyle::BlockHollow,
        }
    }

    #[must_use]
    pub const fn color(self) -> Color {
        Color::from_packed(((self.0.get() >> Self::COLOR_SHIFT) & 0x00ff_ffff) as u32)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalExitStatus {
    pub code: u32,
    pub signal: Option<String>,
}

#[repr(u8)]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    #[default]
    Starting,
    Running,
    Exited(Arc<TerminalExitStatus>),
    Failed(Arc<String>),
}

impl SessionStatus {
    #[must_use]
    pub fn exited(code: u32, signal: Option<String>) -> Self {
        Self::Exited(Arc::new(TerminalExitStatus { code, signal }))
    }

    #[must_use]
    pub fn failed(error: impl Into<String>) -> Self {
        Self::Failed(Arc::new(error.into()))
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalMode {
    #[default]
    Live,
    Copy {
        position: u32,
        total: u32,
    },
    View {
        position: u32,
        total: u32,
    },
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchStatus {
    current_and_flags: NonZeroU32,
    pub total: u32,
}

impl Default for SearchStatus {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl SearchStatus {
    const PENDING: u32 = 1 << 31;
    const INVALID_PATTERN: u32 = 1 << 30;
    const PRESENT: u32 = 1 << 29;
    const CURRENT_MASK: u32 = Self::PRESENT - 1;

    const fn nonzero(raw: u32) -> NonZeroU32 {
        let Some(raw) = NonZeroU32::new(raw) else {
            panic!("search status presence bit must be set")
        };
        raw
    }

    #[must_use]
    pub const fn new(current: u32, total: u32) -> Self {
        Self {
            current_and_flags: Self::nonzero(
                Self::PRESENT
                    | if current > Self::CURRENT_MASK {
                        Self::CURRENT_MASK
                    } else {
                        current
                    },
            ),
            total,
        }
    }

    #[must_use]
    pub const fn with_pending(mut self, pending: bool) -> Self {
        if pending {
            self.current_and_flags = Self::nonzero(self.current_and_flags.get() | Self::PENDING);
        } else {
            self.current_and_flags = Self::nonzero(self.current_and_flags.get() & !Self::PENDING);
        }
        self
    }

    #[must_use]
    pub const fn with_invalid_pattern(mut self, invalid: bool) -> Self {
        if invalid {
            self.current_and_flags =
                Self::nonzero(self.current_and_flags.get() | Self::INVALID_PATTERN);
        } else {
            self.current_and_flags =
                Self::nonzero(self.current_and_flags.get() & !Self::INVALID_PATTERN);
        }
        self
    }

    #[must_use]
    pub const fn pending(self) -> bool {
        self.current_and_flags.get() & Self::PENDING != 0
    }

    #[must_use]
    pub const fn invalid_pattern(self) -> bool {
        self.current_and_flags.get() & Self::INVALID_PATTERN != 0
    }

    /// One-based current match, or zero when there is no current match.
    #[must_use]
    pub const fn current(self) -> u32 {
        self.current_and_flags.get() & Self::CURRENT_MASK
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph<'a> {
    Empty,
    Scalar(char),
    Grapheme(&'a str),
}

/// Immutable style and grapheme tables shared by viewport generations. The two
/// planes are shared separately, so appending styles never copies graphemes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDictionary {
    pub styles: Arc<[PackedStyle]>,
    pub grapheme_offsets: Arc<[u32]>,
    pub grapheme_bytes: Arc<[u8]>,
}

/// Cold, shared presentation metadata kept out of the hot viewport record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalPresentation {
    pub title: Arc<str>,
    pub working_directory: Option<Arc<str>>,
    pub hovered_uri: Option<Arc<str>>,
}

impl Default for TerminalPresentation {
    fn default() -> Self {
        Self {
            title: Arc::from("zz"),
            working_directory: None,
            hovered_uri: None,
        }
    }
}

impl TerminalPresentation {
    #[must_use]
    pub fn new(
        title: Arc<str>,
        working_directory: Option<Arc<str>>,
        hovered_uri: Option<Arc<str>>,
    ) -> Self {
        Self {
            title,
            working_directory,
            hovered_uri,
        }
    }
}

impl TerminalDictionary {
    #[must_use]
    pub const fn from_shared(
        styles: Arc<[PackedStyle]>,
        grapheme_offsets: Arc<[u32]>,
        grapheme_bytes: Arc<[u8]>,
    ) -> Self {
        Self {
            styles,
            grapheme_offsets,
            grapheme_bytes,
        }
    }
}

/// A compact immutable viewport snapshot. Cells are row-major, styles interned,
/// and graphemes live in one UTF-8 arena. No visible cell owns an allocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalViewport {
    pub generation: u64,
    pub view_generation: u64,
    pub dictionary_generation: u32,
    pub columns: u16,
    pub rows: u16,
    pub foreground: Color,
    pub background: Color,
    pub presentation: Arc<TerminalPresentation>,
    pub cells: Arc<[PackedCell]>,
    pub dictionary: Arc<TerminalDictionary>,
    pub overlays: Arc<[OverlaySpan]>,
    pub kitty_placements: Arc<[KittyPlacement]>,
    pub cursor: Option<Cursor>,
    pub scrollbar: ScrollbarState,
    pub mode: TerminalMode,
    pub search: Option<SearchStatus>,
    pub unseen_output: u32,
    pub kitty_keyboard: bool,
    pub mouse_tracking: bool,
    pub status: SessionStatus,
}

pub type TerminalPatchRowIndices = SmallVec<[u16; 4]>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TerminalPatchRowData {
    row_indices: TerminalPatchRowIndices,
    cells: Box<[PackedCell]>,
}

/// Flat changed-row storage for a viewport patch. Empty and overlay-only
/// patches stay inline as `None`; content patches own one cell plane.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalPatchRows(Option<Box<TerminalPatchRowData>>);

impl TerminalPatchRows {
    #[must_use]
    pub fn from_flat(row_indices: TerminalPatchRowIndices, cells: Vec<PackedCell>) -> Self {
        if row_indices.is_empty() && cells.is_empty() {
            Self::default()
        } else {
            Self(Some(Box::new(TerminalPatchRowData {
                row_indices,
                cells: cells.into_boxed_slice(),
            })))
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.row_indices().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.row_indices().is_empty()
    }

    #[must_use]
    pub fn row_indices(&self) -> &[u16] {
        self.0
            .as_deref()
            .map_or(&[], |changed| changed.row_indices.as_slice())
    }

    #[must_use]
    pub fn cells(&self) -> &[PackedCell] {
        self.0
            .as_deref()
            .map_or(&[], |changed| changed.cells.as_ref())
    }

    #[must_use]
    pub fn cells_mut(&mut self) -> &mut [PackedCell] {
        self.0
            .as_deref_mut()
            .map_or(&mut [], |changed| changed.cells.as_mut())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TerminalDictionaryPatchData {
    styles: Box<[PackedStyle]>,
    grapheme_lengths: Box<[u32]>,
    grapheme_bytes: Box<[u8]>,
}

/// Append-only dictionary payload for a retained viewport patch. The empty
/// case is a null niche, since most patches extend neither table.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDictionaryPatch(Option<Box<TerminalDictionaryPatchData>>);

impl TerminalDictionaryPatch {
    #[must_use]
    pub fn from_parts(
        appended_styles: Vec<PackedStyle>,
        appended_grapheme_lengths: Vec<u32>,
        appended_grapheme_bytes: Vec<u8>,
    ) -> Self {
        if appended_styles.is_empty()
            && appended_grapheme_lengths.is_empty()
            && appended_grapheme_bytes.is_empty()
        {
            return Self::default();
        }
        Self(Some(Box::new(TerminalDictionaryPatchData {
            styles: appended_styles.into_boxed_slice(),
            grapheme_lengths: appended_grapheme_lengths.into_boxed_slice(),
            grapheme_bytes: appended_grapheme_bytes.into_boxed_slice(),
        })))
    }

    #[must_use]
    pub fn appended_styles(&self) -> &[PackedStyle] {
        self.0
            .as_deref()
            .map_or(&[], |dictionary| &dictionary.styles)
    }

    #[must_use]
    pub fn appended_grapheme_lengths(&self) -> &[u32] {
        self.0
            .as_deref()
            .map_or(&[], |dictionary| &dictionary.grapheme_lengths)
    }

    #[must_use]
    pub fn appended_grapheme_bytes(&self) -> &[u8] {
        self.0
            .as_deref()
            .map_or(&[], |dictionary| &dictionary.grapheme_bytes)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalViewportPatch {
    pub base_generation: u64,
    pub base_view_generation: u64,
    pub generation: u64,
    pub view_generation: u64,
    pub dictionary_generation: u32,
    pub columns: u16,
    pub rows: u16,
    /// Destination row shift applied to retained rows before replacements.
    pub scroll: i16,
    /// Replacement rows in strictly ascending destination order.
    pub changed_rows: TerminalPatchRows,
    pub style_base: u32,
    pub grapheme_base: u32,
    pub dictionary: TerminalDictionaryPatch,
    pub foreground: Color,
    pub background: Color,
    pub presentation: Arc<TerminalPresentation>,
    pub overlays: Arc<[OverlaySpan]>,
    pub kitty_placements: Arc<[KittyPlacement]>,
    pub cursor: Option<Cursor>,
    pub scrollbar: ScrollbarState,
    pub mode: TerminalMode,
    pub search: Option<SearchStatus>,
    pub unseen_output: u32,
    pub kitty_keyboard: bool,
    pub mouse_tracking: bool,
    pub status: SessionStatus,
}

impl TerminalViewportPatch {
    #[must_use]
    pub fn title(&self) -> &str {
        &self.presentation.title
    }

    #[must_use]
    pub fn working_directory(&self) -> Option<&str> {
        self.presentation.working_directory.as_deref()
    }

    #[must_use]
    pub fn hovered_uri(&self) -> Option<&str> {
        self.presentation.hovered_uri.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PatchError {
    #[error("terminal patch base generation does not match retained viewport")]
    Generation,
    #[error("terminal patch dictionary does not match retained viewport")]
    Dictionary,
    #[error("terminal patch dimensions do not match retained viewport")]
    Dimensions,
    #[error("terminal patch contains an invalid row")]
    Row,
    #[error("terminal patch contains an invalid cell reference")]
    Cell,
    #[error("terminal patch contains invalid viewport metadata")]
    Metadata,
}

/// Reusable working storage for retained viewport differencing.
#[derive(Debug, Default)]
pub struct TerminalDiffScratch {
    cached_cells: Option<Arc<[PackedCell]>>,
    row_fingerprints: Vec<u64>,
}

impl TerminalDiffScratch {
    /// Rebinds cached fingerprints after a patch applied.
    pub fn remember_applied(&mut self, viewport: &TerminalViewport) {
        if self.row_fingerprints.len() == usize::from(viewport.rows) {
            self.cached_cells = Some(Arc::clone(&viewport.cells));
        } else {
            self.cached_cells = None;
        }
    }

    /// Drops the cached source identity, keeping fingerprint capacity.
    pub fn invalidate(&mut self) {
        self.cached_cells = None;
        self.row_fingerprints.clear();
    }
}

impl TerminalViewport {
    /// An empty viewport for a session that has not published yet.
    #[must_use]
    pub fn blank(columns: u16, rows: u16, status: SessionStatus) -> Self {
        Self::blank_with_appearance(
            columns,
            rows,
            status,
            &crate::appearance::TerminalAppearance::default(),
        )
    }

    /// An empty viewport using the daemon's resolved appearance.
    #[must_use]
    pub fn blank_with_appearance(
        columns: u16,
        rows: u16,
        status: SessionStatus,
        appearance: &crate::appearance::TerminalAppearance,
    ) -> Self {
        let foreground = appearance.foreground;
        let background = appearance.background;
        let cell_count = usize::from(columns).saturating_mul(usize::from(rows));

        Self {
            generation: 0,
            view_generation: 0,
            dictionary_generation: 0,
            columns,
            rows,
            foreground,
            background,
            presentation: Arc::new(TerminalPresentation::default()),
            cells: (0..cell_count).map(|_| PackedCell::EMPTY).collect(),
            dictionary: Arc::new(TerminalDictionary::from_shared(
                Arc::from([PackedStyle::new(
                    foreground,
                    background,
                    None,
                    0,
                    UnderlineStyle::None,
                )]),
                Arc::from([0]),
                Arc::from([]),
            )),
            overlays: Arc::from([]),
            kitty_placements: Arc::from([]),
            cursor: None,
            scrollbar: ScrollbarState {
                total: u32::from(rows),
                offset: 0,
                len: u32::from(rows),
            },
            mode: TerminalMode::Live,
            search: None,
            unseen_output: 0,
            kitty_keyboard: false,
            mouse_tracking: false,
            status,
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.presentation.title
    }

    #[must_use]
    pub fn working_directory(&self) -> Option<&str> {
        self.presentation.working_directory.as_deref()
    }

    #[must_use]
    pub fn hovered_uri(&self) -> Option<&str> {
        self.presentation.hovered_uri.as_deref()
    }

    pub fn set_title(&mut self, title: Arc<str>) {
        Arc::make_mut(&mut self.presentation).title = title;
    }

    pub fn set_working_directory(&mut self, working_directory: Option<Arc<str>>) {
        Arc::make_mut(&mut self.presentation).working_directory = working_directory;
    }

    pub fn set_hovered_uri(&mut self, hovered_uri: Option<Arc<str>>) {
        Arc::make_mut(&mut self.presentation).hovered_uri = hovered_uri;
    }

    #[must_use]
    pub fn row(&self, row: u16) -> Option<&[PackedCell]> {
        if row >= self.rows {
            return None;
        }
        let columns = usize::from(self.columns);
        let start = usize::from(row).checked_mul(columns)?;
        self.cells.get(start..start.checked_add(columns)?)
    }

    #[must_use]
    pub fn cell(&self, row: u16, column: u16) -> Option<PackedCell> {
        if row >= self.rows || column >= self.columns {
            return None;
        }
        let index = usize::from(row)
            .checked_mul(usize::from(self.columns))?
            .checked_add(usize::from(column))?;
        self.cells.get(index).copied()
    }

    #[must_use]
    pub fn styles(&self) -> &[PackedStyle] {
        &self.dictionary.styles
    }

    #[must_use]
    pub fn grapheme_offsets(&self) -> &[u32] {
        &self.dictionary.grapheme_offsets
    }

    #[must_use]
    pub fn grapheme_bytes(&self) -> &[u8] {
        &self.dictionary.grapheme_bytes
    }

    #[must_use]
    pub fn style(&self, cell: PackedCell) -> Option<PackedStyle> {
        self.styles().get(usize::from(cell.style_id())).copied()
    }

    #[must_use]
    pub fn glyph(&self, cell: PackedCell) -> Glyph<'_> {
        let glyph = cell.glyph();
        if glyph == 0 {
            return Glyph::Empty;
        }
        if glyph & GRAPHEME_TABLE_BIT == 0 {
            return char::from_u32(glyph).map_or(Glyph::Empty, Glyph::Scalar);
        }

        let index = (glyph & !GRAPHEME_TABLE_BIT) as usize;
        let Some((&start, &end)) = self
            .grapheme_offsets()
            .get(index)
            .zip(self.grapheme_offsets().get(index.saturating_add(1)))
        else {
            return Glyph::Empty;
        };
        let Some(bytes) = self.grapheme_bytes().get(start as usize..end as usize) else {
            return Glyph::Empty;
        };
        std::str::from_utf8(bytes).map_or(Glyph::Empty, Glyph::Grapheme)
    }

    pub fn push_glyph(&self, cell: PackedCell, output: &mut String) {
        match self.glyph(cell) {
            Glyph::Empty => {}
            Glyph::Scalar(value) => output.push(value),
            Glyph::Grapheme(value) => output.push_str(value),
        }
    }

    #[must_use]
    pub fn cell_text(&self, cell: PackedCell) -> Cow<'_, str> {
        match self.glyph(cell) {
            Glyph::Empty => Cow::Borrowed(""),
            Glyph::Scalar(value) => Cow::Owned(value.to_string()),
            Glyph::Grapheme(value) => Cow::Borrowed(value),
        }
    }

    /// A compact retained-grid patch, or `None` when the viewport needs a reset.
    #[must_use]
    pub fn diff(previous: &Self, current: &Self) -> Option<TerminalViewportPatch> {
        Self::diff_with_scratch(previous, current, &mut TerminalDiffScratch::default())
    }

    /// [`Self::diff`] reusing caller-owned row-diff storage.
    #[must_use]
    pub fn diff_with_scratch(
        previous: &Self,
        current: &Self,
        scratch: &mut TerminalDiffScratch,
    ) -> Option<TerminalViewportPatch> {
        if previous.columns != current.columns
            || previous.rows != current.rows
            || previous.dictionary_generation != current.dictionary_generation
            || !current.styles().starts_with(previous.styles())
            || !current
                .grapheme_offsets()
                .starts_with(previous.grapheme_offsets())
            || !current
                .grapheme_bytes()
                .starts_with(previous.grapheme_bytes())
        {
            return None;
        }

        let shared_cells = Arc::ptr_eq(&previous.cells, &current.cells);
        let scroll = if shared_cells {
            0
        } else {
            best_row_shift(previous, current, scratch)
        };
        let mut changed_row_indices = TerminalPatchRowIndices::new();
        if !shared_cells {
            for row in 0..current.rows {
                let source = i32::from(row) - i32::from(scroll);
                let unchanged = u16::try_from(source).ok().is_some_and(|source| {
                    source < previous.rows && previous.row(source) == current.row(row)
                });
                if !unchanged {
                    changed_row_indices.push(row);
                }
            }
        }
        let mut changed_cells = Vec::with_capacity(
            changed_row_indices
                .len()
                .saturating_mul(usize::from(current.columns)),
        );
        for row in changed_row_indices.iter().copied() {
            changed_cells.extend_from_slice(current.row(row).unwrap_or_default());
        }
        let changed_rows = TerminalPatchRows::from_flat(changed_row_indices, changed_cells);

        let grapheme_base = previous.grapheme_offsets().len().saturating_sub(1);
        let appended_grapheme_lengths = current.grapheme_offsets()[grapheme_base..]
            .windows(2)
            .map(|offsets| offsets[1].saturating_sub(offsets[0]))
            .collect();
        let dictionary = TerminalDictionaryPatch::from_parts(
            current.styles()[previous.styles().len()..].to_vec(),
            appended_grapheme_lengths,
            current.grapheme_bytes()[previous.grapheme_bytes().len()..].to_vec(),
        );

        Some(TerminalViewportPatch {
            base_generation: previous.generation,
            base_view_generation: previous.view_generation,
            generation: current.generation,
            view_generation: current.view_generation,
            dictionary_generation: current.dictionary_generation,
            columns: current.columns,
            rows: current.rows,
            scroll,
            changed_rows,
            style_base: u32::try_from(previous.styles().len()).ok()?,
            grapheme_base: u32::try_from(grapheme_base).ok()?,
            dictionary,
            foreground: current.foreground,
            background: current.background,
            presentation: Arc::clone(&current.presentation),
            overlays: Arc::clone(&current.overlays),
            kitty_placements: Arc::clone(&current.kitty_placements),
            cursor: current.cursor,
            scrollbar: current.scrollbar,
            mode: current.mode,
            search: current.search,
            unseen_output: current.unseen_output,
            kitty_keyboard: current.kitty_keyboard,
            mouse_tracking: current.mouse_tracking,
            status: current.status.clone(),
        })
    }

    /// Applies a patch to this retained viewport.
    ///
    /// # Errors
    ///
    /// [`PatchError`] when the patch is inconsistent with this viewport. An
    /// error leaves the viewport unchanged.
    pub fn apply_patch(&mut self, patch: TerminalViewportPatch) -> Result<(), PatchError> {
        if self.generation != patch.base_generation
            || self.view_generation != patch.base_view_generation
        {
            return Err(PatchError::Generation);
        }
        if self.dictionary_generation != patch.dictionary_generation {
            return Err(PatchError::Dictionary);
        }
        if self.columns != patch.columns || self.rows != patch.rows {
            return Err(PatchError::Dimensions);
        }

        let style_base = usize::try_from(patch.style_base).map_err(|_| PatchError::Dictionary)?;
        if style_base != self.styles().len() {
            return Err(PatchError::Dictionary);
        }
        let appended_styles = patch.dictionary.appended_styles();
        let appended_grapheme_lengths = patch.dictionary.appended_grapheme_lengths();
        let appended_grapheme_bytes = patch.dictionary.appended_grapheme_bytes();
        let style_count = style_base
            .checked_add(appended_styles.len())
            .ok_or(PatchError::Dictionary)?;
        if style_count > usize::from(u16::MAX) + 1
            || appended_styles.iter().any(|style| {
                style.foreground_raw() > 0x00ff_ffff
                    || style.background_raw() > 0x00ff_ffff
                    || (style.underline_color_raw() > 0x00ff_ffff
                        && style.underline_color_raw() != NO_COLOR)
                    || style.underline_kind_raw() > UnderlineStyle::Dashed as u8
            })
        {
            return Err(PatchError::Dictionary);
        }

        let grapheme_base =
            usize::try_from(patch.grapheme_base).map_err(|_| PatchError::Dictionary)?;
        if grapheme_base != self.grapheme_offsets().len().saturating_sub(1) {
            return Err(PatchError::Dictionary);
        }
        let grapheme_count = grapheme_base
            .checked_add(appended_grapheme_lengths.len())
            .ok_or(PatchError::Dictionary)?;
        if grapheme_count > GRAPHEME_TABLE_BIT as usize {
            return Err(PatchError::Dictionary);
        }
        let mut appended_cursor = 0_usize;
        let grapheme_byte_base =
            u32::try_from(self.grapheme_bytes().len()).map_err(|_| PatchError::Dictionary)?;
        let mut absolute_offset = grapheme_byte_base;
        for length in appended_grapheme_lengths {
            let length = usize::try_from(*length).map_err(|_| PatchError::Dictionary)?;
            let end = appended_cursor
                .checked_add(length)
                .ok_or(PatchError::Dictionary)?;
            let bytes = appended_grapheme_bytes
                .get(appended_cursor..end)
                .ok_or(PatchError::Dictionary)?;
            std::str::from_utf8(bytes).map_err(|_| PatchError::Dictionary)?;
            absolute_offset = absolute_offset
                .checked_add(u32::try_from(length).map_err(|_| PatchError::Dictionary)?)
                .ok_or(PatchError::Dictionary)?;
            appended_cursor = end;
        }
        if appended_cursor != appended_grapheme_bytes.len() {
            return Err(PatchError::Dictionary);
        }

        let columns = usize::from(self.columns);
        let rows = usize::from(self.rows);
        let shift = isize::from(patch.scroll);
        if shift.unsigned_abs() >= rows && rows != 0 && shift != 0 {
            return Err(PatchError::Row);
        }
        if rows == 0 && shift != 0 {
            return Err(PatchError::Row);
        }
        let changed_row_indices = patch.changed_rows.row_indices();
        let changed_cells = patch.changed_rows.cells();
        if changed_row_indices
            .len()
            .checked_mul(columns)
            .is_none_or(|expected| expected != changed_cells.len())
        {
            return Err(PatchError::Row);
        }

        let mut previous_row = None;
        for row in changed_row_indices.iter().copied() {
            let index = usize::from(row);
            if index >= rows || previous_row.is_some_and(|previous| previous >= row) {
                return Err(PatchError::Row);
            }
            previous_row = Some(row);
        }
        for cell in changed_cells {
            if usize::from(cell.style_id()) >= style_count {
                return Err(PatchError::Cell);
            }
            let glyph = cell.glyph();
            if glyph & GRAPHEME_TABLE_BIT != 0 {
                let grapheme =
                    usize::try_from(glyph & !GRAPHEME_TABLE_BIT).map_err(|_| PatchError::Cell)?;
                if grapheme >= grapheme_count {
                    return Err(PatchError::Cell);
                }
            } else if glyph != 0 && char::from_u32(glyph).is_none() {
                return Err(PatchError::Cell);
            }
        }
        if shift > 0 {
            let exposed = usize::try_from(shift).map_err(|_| PatchError::Row)?;
            if changed_row_indices.len() < exposed
                || changed_row_indices[..exposed]
                    .iter()
                    .enumerate()
                    .any(|(row, changed)| usize::from(*changed) != row)
            {
                return Err(PatchError::Row);
            }
        } else if shift < 0 {
            let exposed = shift.unsigned_abs();
            if changed_row_indices.len() < exposed
                || changed_row_indices[changed_row_indices.len() - exposed..]
                    .iter()
                    .enumerate()
                    .any(|(offset, changed)| usize::from(*changed) != rows - exposed + offset)
            {
                return Err(PatchError::Row);
            }
        }
        if patch.presentation.hovered_uri.as_ref().is_some_and(|uri| {
            uri.len() > MAX_HOVERED_URI_BYTES
                || uri
                    .chars()
                    .any(|character| character.is_control() || character.is_whitespace())
        }) || patch.overlays.iter().any(|overlay| {
            overlay.row >= patch.rows || overlay.start > overlay.end || overlay.end > patch.columns
        }) || patch
            .cursor
            .is_some_and(|cursor| cursor.row() >= patch.rows || cursor.column() >= patch.columns)
            || patch.scrollbar.offset > patch.scrollbar.total
            || patch.scrollbar.len > patch.scrollbar.total
            || patch.scrollbar.offset.saturating_add(patch.scrollbar.len) > patch.scrollbar.total
            || matches!(
                patch.mode,
                TerminalMode::Copy { position, total } | TerminalMode::View { position, total }
                    if total == 0 || position == 0 || position > total
            )
            || patch
                .search
                .is_some_and(|search| search.current() > search.total)
        {
            return Err(PatchError::Metadata);
        }

        if !patch.dictionary.is_empty() {
            let dictionary = Arc::make_mut(&mut self.dictionary);
            if !appended_styles.is_empty() {
                dictionary.styles = append_shared_slice(&dictionary.styles, appended_styles);
            }
            if !appended_grapheme_lengths.is_empty() {
                dictionary.grapheme_offsets = append_grapheme_offsets(
                    &dictionary.grapheme_offsets,
                    appended_grapheme_lengths,
                    grapheme_byte_base,
                );
            }
            if !appended_grapheme_bytes.is_empty() {
                dictionary.grapheme_bytes =
                    append_shared_slice(&dictionary.grapheme_bytes, appended_grapheme_bytes);
            }
        }
        let cells = Arc::make_mut(&mut self.cells);
        if shift > 0 {
            let shift = shift.unsigned_abs();
            cells.copy_within(0..(rows - shift) * columns, shift * columns);
        } else if shift < 0 {
            let shift = shift.unsigned_abs();
            cells.copy_within(shift * columns..rows * columns, 0);
        }
        for (patch_row, row) in changed_row_indices.iter().copied().enumerate() {
            let source = patch_row * columns;
            let destination = usize::from(row) * columns;
            cells[destination..destination + columns]
                .copy_from_slice(&changed_cells[source..source + columns]);
        }
        self.generation = patch.generation;
        self.view_generation = patch.view_generation;
        self.foreground = patch.foreground;
        self.background = patch.background;
        self.presentation = patch.presentation;
        self.overlays = patch.overlays;
        self.kitty_placements = patch.kitty_placements;
        self.cursor = patch.cursor;
        self.scrollbar = patch.scrollbar;
        self.mode = patch.mode;
        self.search = patch.search;
        self.unseen_output = patch.unseen_output;
        self.kitty_keyboard = patch.kitty_keyboard;
        self.mouse_tracking = patch.mouse_tracking;
        self.status = patch.status;
        Ok(())
    }
}

fn append_shared_slice<T: Copy>(current: &[T], appended: &[T]) -> Arc<[T]> {
    current
        .iter()
        .copied()
        .chain(appended.iter().copied())
        .collect()
}

fn append_grapheme_offsets(current: &[u32], lengths: &[u32], mut offset: u32) -> Arc<[u32]> {
    current
        .iter()
        .copied()
        .chain((0..lengths.len()).map(move |index| {
            offset = offset
                .checked_add(lengths[index])
                .expect("grapheme offsets were validated before publication");
            offset
        }))
        .collect()
}

fn best_row_shift(
    previous: &TerminalViewport,
    current: &TerminalViewport,
    scratch: &mut TerminalDiffScratch,
) -> i16 {
    let rows = usize::from(current.rows);
    let previous_is_cached = scratch
        .cached_cells
        .as_ref()
        .is_some_and(|cells| Arc::ptr_eq(cells, &previous.cells))
        && scratch.row_fingerprints.len() == rows;
    if previous_is_cached {
        scratch.row_fingerprints.reserve(rows);
    } else {
        scratch.row_fingerprints.clear();
        scratch.row_fingerprints.reserve(rows.saturating_mul(2));
        scratch.row_fingerprints.extend(
            (0..previous.rows).map(|row| row_fingerprint(previous.row(row).unwrap_or_default())),
        );
    }
    scratch
        .row_fingerprints
        .extend((0..current.rows).map(|row| row_fingerprint(current.row(row).unwrap_or_default())));
    let shift = {
        let (previous, current) = scratch.row_fingerprints.split_at(rows);
        best_row_shift_from_fingerprints(previous, current)
    };
    scratch
        .row_fingerprints
        .copy_within(rows..rows.saturating_mul(2), 0);
    scratch.row_fingerprints.truncate(rows);
    scratch.cached_cells = Some(Arc::clone(&current.cells));
    shift
}

fn row_fingerprint(cells: &[PackedCell]) -> u64 {
    let mut hash = 0x9e37_79b9_7f4a_7c15_u64 ^ cells.len() as u64;
    for cell in cells {
        let packed = u64::from(cell.glyph())
            | (u64::from(cell.style_id()) << 32)
            | (u64::from(cell.flags()) << 48);
        hash ^= packed.wrapping_mul(0xd6e8_feb8_6659_fd93);
        hash = hash
            .rotate_left(27)
            .wrapping_mul(0x3c79_ac49_2ba7_b653)
            .wrapping_add(0x1c69_b3f7_4ac4_ae35);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^ (hash >> 33)
}

fn best_row_shift_from_fingerprints(previous: &[u64], current: &[u64]) -> i16 {
    debug_assert_eq!(previous.len(), current.len());
    let rows = i32::try_from(current.len()).unwrap_or(i32::MAX);
    if rows < 2 {
        return 0;
    }
    let mut best_shift = 0_i32;
    let mut best_matches = previous
        .iter()
        .zip(current)
        .filter(|(previous, current)| previous == current)
        .count();
    for shift in -(rows - 1)..rows {
        if shift == 0 {
            continue;
        }
        let matches = (0..rows)
            .filter(|source| {
                let destination = source + shift;
                destination >= 0 && destination < rows && {
                    let source = usize::try_from(*source).unwrap_or_default();
                    let destination = usize::try_from(destination).unwrap_or_default();
                    previous[source] == current[destination]
                }
            })
            .count();
        if matches > best_matches {
            best_matches = matches;
            best_shift = shift;
        }
    }
    let minimum = current.len().saturating_div(2).max(2);
    if best_matches < minimum {
        0
    } else {
        i16::try_from(best_shift).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_records_keep_their_layout_contract() {
        assert_eq!(size_of::<PackedCell>(), 8);
        assert_eq!(size_of::<PackedStyle>(), 16);
        assert_eq!(size_of::<OverlaySpan>(), 8);
        assert_eq!(size_of::<SearchStatus>(), 8);
        assert_eq!(size_of::<Option<SearchStatus>>(), 8);
        assert_eq!(size_of::<Cursor>(), 8);
        assert_eq!(size_of::<Option<Cursor>>(), 8);
        assert_eq!(size_of::<ScrollbarState>(), 12);
        assert_eq!(size_of::<TerminalMode>(), 12);
        assert_eq!(align_of::<PackedCell>(), align_of::<u32>());
        assert_eq!(align_of::<PackedStyle>(), align_of::<u32>());
        assert_eq!(align_of::<OverlaySpan>(), align_of::<u16>());
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(size_of::<SessionStatus>(), 16);
            assert_eq!(size_of::<TerminalDictionary>(), 48);
            assert_eq!(size_of::<TerminalPresentation>(), 48);
            assert_eq!(size_of::<Arc<TerminalPresentation>>(), 8);
            assert_eq!(size_of::<TerminalViewport>(), 160);
            assert_eq!(size_of::<TerminalDiffScratch>(), 40);
            assert_eq!(size_of::<TerminalPatchRows>(), 8);
            assert_eq!(size_of::<TerminalPatchRowData>(), 40);
            assert_eq!(size_of::<TerminalDictionaryPatch>(), 8);
            assert_eq!(size_of::<TerminalDictionaryPatchData>(), 48);
            assert_eq!(size_of::<TerminalViewportPatch>(), 176);
        }
    }

    #[test]
    fn cold_status_payloads_are_shared_across_frame_clones() {
        let exited = SessionStatus::exited(7, Some("TERM".to_owned()));
        let exited_clone = exited.clone();
        let (SessionStatus::Exited(left), SessionStatus::Exited(right)) = (&exited, &exited_clone)
        else {
            panic!("expected exited statuses");
        };
        assert!(Arc::ptr_eq(left, right));

        let failed = SessionStatus::failed("boom");
        let failed_clone = failed.clone();
        let (SessionStatus::Failed(left), SessionStatus::Failed(right)) = (&failed, &failed_clone)
        else {
            panic!("expected failed statuses");
        };
        assert!(Arc::ptr_eq(left, right));
    }

    #[test]
    fn niche_encoded_metadata_round_trips_renderer_values() {
        let status = SearchStatus::new(7, 19)
            .with_pending(true)
            .with_invalid_pattern(true);
        assert_eq!(status.current(), 7);
        assert_eq!(status.total, 19);
        assert!(status.pending());
        assert!(status.invalid_pattern());
        assert_eq!(size_of::<SearchStatus>(), 8);
        assert_eq!(size_of::<Option<SearchStatus>>(), 8);

        for style in [
            CursorStyle::Bar,
            CursorStyle::Block,
            CursorStyle::Underline,
            CursorStyle::BlockHollow,
        ] {
            for flags in 0_u8..8 {
                let cursor = Cursor::new(
                    u16::MAX,
                    u16::MAX - 1,
                    flags & 1 != 0,
                    flags & 2 != 0,
                    flags & 4 != 0,
                    style,
                    Color::rgb(1, 2, 3),
                );
                assert_eq!(cursor.column(), u16::MAX);
                assert_eq!(cursor.row(), u16::MAX - 1);
                assert_eq!(cursor.visible(), flags & 1 != 0);
                assert_eq!(cursor.blinking(), flags & 2 != 0);
                assert_eq!(cursor.at_wide_tail(), flags & 4 != 0);
                assert_eq!(cursor.style(), style);
                assert_eq!(cursor.color(), Color::rgb(1, 2, 3));
            }
        }
        assert_eq!(size_of::<Cursor>(), 8);
        assert_eq!(size_of::<Option<Cursor>>(), 8);
    }

    #[test]
    fn renderer_style_and_overlay_flags_preserve_hot_record_layouts() {
        let attributes = ATTR_BOLD | ATTR_EXPLICIT_RGB | ATTR_HYPERLINK;
        let style = PackedStyle::new(
            Color::rgb(1, 2, 3),
            Color::rgb(4, 5, 6),
            None,
            attributes,
            UnderlineStyle::Single,
        );
        assert_eq!(style.attributes(), attributes);
        assert!(style.bold());
        assert!(style.explicit_rgb());
        assert!(style.hyperlink());
        assert_eq!(size_of::<PackedStyle>(), 16);

        let overlay = OverlaySpan::with_flags(3, 4, 8, OverlayKind::Selection, OVERLAY_RECTANGLE);
        assert_eq!(overlay.kind(), OverlayKind::Selection);
        assert_eq!(overlay.flags(), OVERLAY_RECTANGLE);
        assert_eq!(size_of::<OverlaySpan>(), 8);
    }

    #[test]
    fn blank_viewport_is_flat_and_resolved() {
        let viewport = TerminalViewport::blank(80, 24, SessionStatus::Starting);
        assert_eq!(viewport.cells.len(), 80 * 24);
        assert_eq!(viewport.styles().len(), 1);
        assert_eq!(viewport.row(0).expect("first row").len(), 80);
        assert_eq!(
            viewport
                .style(PackedCell::EMPTY)
                .expect("default")
                .background(),
            viewport.background
        );
    }

    #[test]
    fn grapheme_arena_is_bounds_checked() {
        let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        let dictionary = Arc::make_mut(&mut viewport.dictionary);
        dictionary.grapheme_offsets = Arc::from([0, 2]);
        dictionary.grapheme_bytes = Arc::from("e\u{301}".as_bytes());
        let cell = PackedCell::new(GRAPHEME_TABLE_BIT, 0, CellWidth::Narrow);
        assert_eq!(viewport.glyph(cell), Glyph::Empty);
    }

    #[test]
    fn row_fingerprints_cover_the_complete_packed_cell() {
        let base = PackedCell::new(u32::from('a'), 0, CellWidth::Narrow);
        assert_eq!(row_fingerprint(&[base]), row_fingerprint(&[base]));
        assert_ne!(
            row_fingerprint(&[base]),
            row_fingerprint(&[PackedCell::new(u32::from('b'), 0, CellWidth::Narrow)])
        );
        assert_ne!(
            row_fingerprint(&[base]),
            row_fingerprint(&[PackedCell::new(u32::from('a'), 1, CellWidth::Narrow)])
        );
        assert_ne!(
            row_fingerprint(&[base]),
            row_fingerprint(&[PackedCell::new(u32::from('a'), 0, CellWidth::Wide)])
        );
    }

    #[test]
    fn diff_scratch_reuses_fingerprints_and_shared_cells_skip_the_scan() {
        let mut previous = TerminalViewport::blank(1, 3, SessionStatus::Running);
        previous.generation = 1;
        let mut current = previous.clone();
        current.generation = 2;
        current.view_generation = 2;
        Arc::make_mut(&mut current.cells)[2] =
            PackedCell::new(u32::from('a'), 0, CellWidth::Narrow);
        let mut scratch = TerminalDiffScratch::default();

        let patch = TerminalViewport::diff_with_scratch(&previous, &current, &mut scratch)
            .expect("compatible content frame");
        assert_eq!(patch.changed_rows.row_indices(), [2]);
        let fingerprints = scratch.row_fingerprints.as_ptr();
        let capacity = scratch.row_fingerprints.capacity();
        assert!(capacity >= 6);
        assert_eq!(scratch.row_fingerprints.len(), 3);
        assert!(
            scratch
                .cached_cells
                .as_ref()
                .is_some_and(|cells| Arc::ptr_eq(cells, &current.cells))
        );

        let mut metadata = current.clone();
        metadata.view_generation = 3;
        metadata.overlays = Arc::from([OverlaySpan::new(0, 0, 1, OverlayKind::Selection)]);
        let patch = TerminalViewport::diff_with_scratch(&current, &metadata, &mut scratch)
            .expect("compatible metadata frame");
        assert!(patch.changed_rows.is_empty());
        assert_eq!(scratch.row_fingerprints.as_ptr(), fingerprints);
        assert_eq!(scratch.row_fingerprints.capacity(), capacity);
        assert!(
            scratch
                .cached_cells
                .as_ref()
                .is_some_and(|cells| Arc::ptr_eq(cells, &metadata.cells))
        );

        let mut next = metadata.clone();
        next.generation = 3;
        next.view_generation = 4;
        Arc::make_mut(&mut next.cells)[1] = PackedCell::new(u32::from('b'), 0, CellWidth::Narrow);
        TerminalViewport::diff_with_scratch(&metadata, &next, &mut scratch)
            .expect("compatible second content frame");
        assert_eq!(scratch.row_fingerprints.as_ptr(), fingerprints);
        assert_eq!(scratch.row_fingerprints.capacity(), capacity);
        assert!(
            scratch
                .cached_cells
                .as_ref()
                .is_some_and(|cells| Arc::ptr_eq(cells, &next.cells))
        );

        let mut applied = next.clone();
        applied.cells = Arc::from(next.cells.as_ref());
        assert!(!Arc::ptr_eq(&applied.cells, &next.cells));
        scratch.remember_applied(&applied);
        assert!(
            scratch
                .cached_cells
                .as_ref()
                .is_some_and(|cells| Arc::ptr_eq(cells, &applied.cells))
        );
        scratch.invalidate();
        assert!(scratch.cached_cells.is_none());
        assert!(scratch.row_fingerprints.is_empty());
        assert_eq!(scratch.row_fingerprints.capacity(), capacity);
    }

    #[test]
    fn row_patch_detects_scroll_and_rebuilds_the_viewport() {
        let mut previous = TerminalViewport::blank(3, 3, SessionStatus::Running);
        previous.generation = 7;
        let cells = Arc::make_mut(&mut previous.cells);
        for (row, glyph) in ['A', 'B', 'C'].into_iter().enumerate() {
            cells[row * 3..row * 3 + 3].fill(PackedCell::new(
                u32::from(glyph),
                0,
                CellWidth::Narrow,
            ));
        }

        let mut current = previous.clone();
        current.generation = 8;
        current.view_generation = 8;
        let cells = Arc::make_mut(&mut current.cells);
        cells.copy_within(3..9, 0);
        cells[6..9].fill(PackedCell::new(u32::from('D'), 0, CellWidth::Narrow));

        let patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
        assert_eq!(patch.scroll, -1);
        assert_eq!(patch.changed_rows.len(), 1);
        assert_eq!(patch.changed_rows.row_indices(), [2]);

        let mut retained = previous;
        retained.apply_patch(patch).expect("valid patch");
        assert_eq!(retained, current);
    }

    #[test]
    fn rejected_patch_does_not_mutate_the_retained_viewport() {
        let mut retained = TerminalViewport::blank(2, 2, SessionStatus::Running);
        retained.generation = 10;
        let before = retained.clone();
        let mut current = retained.clone();
        current.generation = 11;
        Arc::make_mut(&mut current.cells)[0] =
            PackedCell::new(u32::from('x'), 0, CellWidth::Narrow);
        let mut patch = TerminalViewport::diff(&retained, &current).expect("compatible viewport");
        patch.changed_rows.cells_mut()[0] =
            PackedCell::new(u32::from('x'), u16::MAX, CellWidth::Narrow);

        assert_eq!(retained.apply_patch(patch), Err(PatchError::Cell));
        assert_eq!(retained, before);
    }

    #[test]
    fn patch_rows_are_canonical_and_rejected_atomically_when_out_of_order() {
        let mut retained = TerminalViewport::blank(1, 3, SessionStatus::Running);
        retained.generation = 30;
        let before = retained.clone();
        let mut current = retained.clone();
        current.generation = 31;
        current.view_generation = 31;
        let cells = Arc::make_mut(&mut current.cells);
        cells[0] = PackedCell::new(u32::from('a'), 0, CellWidth::Narrow);
        cells[2] = PackedCell::new(u32::from('c'), 0, CellWidth::Narrow);

        let mut patch = TerminalViewport::diff(&retained, &current).expect("compatible viewport");
        assert_eq!(patch.changed_rows.row_indices(), [0, 2]);
        assert_eq!(
            patch
                .changed_rows
                .cells()
                .iter()
                .map(|cell| cell.glyph())
                .collect::<Vec<_>>(),
            [u32::from('a'), u32::from('c')]
        );
        assert!(
            !patch
                .changed_rows
                .0
                .as_deref()
                .expect("content patch payload")
                .row_indices
                .spilled()
        );
        let reversed = patch
            .changed_rows
            .row_indices()
            .iter()
            .rev()
            .copied()
            .collect::<TerminalPatchRowIndices>();
        let cells = patch.changed_rows.cells().to_vec();
        patch.changed_rows = TerminalPatchRows::from_flat(reversed, cells);

        assert_eq!(retained.apply_patch(patch), Err(PatchError::Row));
        assert_eq!(retained, before);
    }

    #[test]
    fn row_patch_appends_style_and_grapheme_dictionaries() {
        let mut previous = TerminalViewport::blank(1, 1, SessionStatus::Running);
        previous.generation = 20;
        let mut current = previous.clone();
        current.generation = 21;
        current.view_generation = 21;
        let mut styles = current.styles().to_vec();
        styles.push(PackedStyle::new(
            Color::rgb(0xaa, 0xbb, 0xcc),
            current.background,
            None,
            ATTR_BOLD,
            UnderlineStyle::None,
        ));
        let grapheme = "e\u{301}";
        let dictionary = Arc::make_mut(&mut current.dictionary);
        dictionary.styles = styles.into();
        dictionary.grapheme_bytes = Arc::from(grapheme.as_bytes());
        dictionary.grapheme_offsets =
            Arc::from([0, u32::try_from(grapheme.len()).expect("small fixture")]);
        Arc::make_mut(&mut current.cells)[0] =
            PackedCell::new(GRAPHEME_TABLE_BIT, 1, CellWidth::Narrow);

        let patch = TerminalViewport::diff(&previous, &current).expect("append-only dictionary");
        assert!(!patch.dictionary.is_empty());
        assert_eq!(patch.style_base, 1);
        assert_eq!(patch.dictionary.appended_styles().len(), 1);
        assert_eq!(patch.grapheme_base, 0);
        assert_eq!(patch.dictionary.appended_grapheme_lengths(), [3]);
        assert_eq!(
            patch.dictionary.appended_grapheme_bytes(),
            grapheme.as_bytes()
        );

        let previous_dictionary = Arc::clone(&previous.dictionary);
        previous.apply_patch(patch).expect("valid append patch");
        assert_eq!(previous, current);
        assert!(!Arc::ptr_eq(&previous.dictionary, &previous_dictionary));
    }

    #[test]
    fn overlay_only_patch_advances_view_without_cells() {
        let mut previous = TerminalViewport::blank(4, 2, SessionStatus::Running);
        previous.generation = 6;
        previous.view_generation = 11;
        let mut current = previous.clone();
        current.view_generation = 12;
        current.overlays = Arc::from([OverlaySpan::new(0, 1, 3, OverlayKind::Selection)]);

        let patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
        assert!(patch.changed_rows.is_empty());
        assert!(patch.changed_rows.0.is_none());
        assert!(patch.dictionary.is_empty());
        assert_eq!(patch.base_generation, patch.generation);
        assert_eq!(patch.base_view_generation, 11);
        assert!(Arc::ptr_eq(&patch.overlays, &current.overlays));

        previous.apply_patch(patch).expect("valid overlay patch");
        assert_eq!(previous, current);
    }
}
