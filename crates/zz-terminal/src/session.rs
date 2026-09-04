#[cfg(any(not(unix), test))]
use std::io::Read;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
    io::Write,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};
use libghostty_vt::{
    RenderState, Terminal, TerminalOptions,
    alloc::{Allocator, Bytes},
    fmt::{Format, Formatter, FormatterOptions},
    focus, key,
    kitty::graphics::{self, DecodedImage, ImageFormat, PlacementIterator},
    mouse::{self, EncoderSize},
    render::{CellIterator, CursorVisualStyle, Dirty, RowIterator},
    screen::{
        CellContentTag, CellSemanticContent, CellWide, RowSemanticPrompt, Screen, TrackedGridRef,
    },
    selection::{FormatOptions, SelectLineOptions, SelectWordOptions, Selection},
    style::{Palette, RgbColor, StyleColor, Underline},
    terminal::{
        ClipboardContent, ClipboardLocation, ClipboardWriteError, ColorScheme, ConformanceLevel,
        CursorStyle as GhosttyCursorStyle, DeviceAttributeFeature, DeviceAttributes, DeviceType,
        Mode, Point, PointCoordinate, PointSpace, PrimaryDeviceAttributes, ScrollViewport,
        SecondaryDeviceAttributes, SizeReportSize, TertiaryDeviceAttributes,
    },
};
use parking_lot::{Mutex, RwLock};
use portable_pty::{CommandBuilder, ExitStatus, PtySize, native_pty_system};
use regex::RegexBuilder;
use smallvec::SmallVec;
use thiserror::Error;

use crate::{
    ATTR_BLINK, ATTR_BOLD, ATTR_EXPLICIT_RGB, ATTR_FAINT, ATTR_HYPERLINK, ATTR_INVISIBLE,
    ATTR_ITALIC, ATTR_OVERLINE, ATTR_STRIKETHROUGH, CellWidth, ClipboardTarget, Color, CopyJump,
    CopyJumpDirection, CopyModeAction, CopyModeCountPolicy, CopyModeSearch, CopySelectionMode,
    Cursor, CursorBlinkPolicy, CursorStyle, GRAPHEME_TABLE_BIT, IMAGE_PLACEHOLDER_SCHEME,
    KeyAction, KeyCode, KeyInput, KittyLayer, KittyPlacement, MAX_HISTORY_LIMIT,
    MAX_KITTY_IMAGE_BYTES, Modifiers, OVERLAY_RECTANGLE, OverlayKind, OverlaySpan, PackedCell,
    PackedStyle, PasteBufferAction, PointerCellEvent, ScrollbarState, SearchCase, SearchDirection,
    SearchMode, SearchQuery, SearchStatus, SessionStatus, TerminalAppearance, TerminalColorScheme,
    TerminalDictionary, TerminalMode, TerminalMouseButton, TerminalMouseInput, TerminalMousePhase,
    TerminalPresentation, TerminalViewAction, TerminalViewId, TerminalViewport, UnderlineStyle,
    WordSeparators,
};

mod mode_revision;

use mode_revision::{ModeRevision, ModeSelection};

const INITIAL_COLUMNS: u16 = 80;
const INITIAL_ROWS: u16 = 24;
const INITIAL_CELL_WIDTH: u32 = 8;
const INITIAL_CELL_HEIGHT: u32 = 18;
const MAX_LINK_URI_BYTES: usize = 16 * 1024;
const LINK_URI_SCRATCH_BYTES: usize = 256;
/// The `(prefix, suffix)` an agent CLI wraps around its own attachment number.
/// Current Claude Code and Codex both print `[Image #2]`.
const IMAGE_PLACEHOLDERS: [(&str, &str); 1] = [("[Image #", "]")];
const MAX_PLACEHOLDER_CELLS: usize = 32;
const MAX_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
/// Trailing output rows [`capture_last_command`] keeps.
pub const MAX_LAST_COMMAND_LINES: usize = 200;
/// Byte ceiling applied after the line cap, whichever bites first.
pub const MAX_LAST_COMMAND_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_VIEW_SCROLLBACK: usize = 100_000;
const MAX_STARTUP_OUTPUT_VIEW_SCROLLBACK: usize = 64 * 1024 * 1024;
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);
const SEARCH_REFRESH_DEBOUNCE: Duration = Duration::from_millis(80);
const TERMINATION_GRACE: Duration = Duration::from_millis(500);
const TERMINATION_KILL_WAIT: Duration = Duration::from_millis(500);
const MAX_SEARCH_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const MAX_WHEEL_REPEAT: u32 = 32;
const MAX_KITTY_PLACEMENTS: usize = 512;
const PTY_READ_BUFFER_BYTES: usize = 64 * 1024;
const RAW_OUTPUT_TAP_PENDING_CHUNKS: usize = 4;
const RAW_OUTPUT_PARSE_BACKLOG_BYTES: usize = 4 * 1024 * 1024;
const RAW_OUTPUT_PARSE_READ_RESERVE_BYTES: usize = 8 * PTY_READ_BUFFER_BYTES;
const RAW_OUTPUT_PARSE_TURN_BYTES: usize = 16 * 1024;
#[cfg(target_os = "linux")]
const PTY_BUFFER_POOL_SIZE: usize = 4;
#[cfg(all(not(target_os = "linux"), any(not(unix), test)))]
const PTY_BUFFER_POOL_SIZE: usize = 8;
#[cfg(all(unix, not(target_os = "linux")))]
const PTY_DRAIN_TURN_BYTES: usize = 256 * 1024;
/// Wall-time bound on a single drain turn so the actor lane stays responsive even when the
/// parser runs far below the byte bound's assumed rate (e.g. an unoptimized VT build).
#[cfg(all(unix, not(target_os = "linux")))]
const PTY_DRAIN_TURN_TIME: Duration = Duration::from_millis(1);
#[cfg(unix)]
const PTY_BRIDGE_THRESHOLD_BYTES: usize = 1024;
/// Nonblocking read retries bridging a saturated producer's kernel queue refill.
/// Probed on Mac16,5/macOS 27: spin 64/256/512 gave 281/332/348 MB/s.
#[cfg(all(unix, not(target_os = "linux")))]
const PTY_BRIDGE_SPIN_MAX: u32 = 512;
#[cfg(target_os = "linux")]
const PTY_GATHER_BRIDGE_SPIN_MAX: u32 = 16;
const CONTENT_PUBLISH_STALENESS: Duration = Duration::from_millis(16);
#[cfg(unix)]
const PTY_WRITE_RETRY: Duration = Duration::from_millis(16);
#[cfg(unix)]
const PTY_WRITE_BUDGET_BYTES: usize = 1024 * 1024;
#[cfg(unix)]
const PTY_WRITE_RETAIN_BYTES: usize = 64 * 1024;
const MAX_PENDING_PTY_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PENDING_PTY_INPUT_COMMANDS: usize = 256;
const PTY_INPUT_COMMAND_FLOOR_BYTES: usize = 4 * 1024;
const PENDING_PASTE_WINDOW: Duration = Duration::from_secs(5);
const IDLE_SLEEP: Duration = Duration::from_hours(1);
const MAX_PTY_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_CLIPBOARD_WRITE_BYTES: usize = 8 * 1024 * 1024;
const CLIPBOARD_TEXT_MIME: &str = "text/plain";
const MAX_PENDING_ACTOR_COMMANDS: usize = 1;
const MAX_PENDING_RELIABLE_EVENTS: usize = 4;
const MAX_PENDING_RELIABLE_EVENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PENDING_TERMINAL_EVENTS: usize = MAX_PENDING_RELIABLE_EVENTS + 1;
const RETAINED_CELL_PLANES: usize = 2;
const RETAINED_OVERLAY_PLANES: usize = 2;
const MAX_VIEWPORT_STYLE_COUNT: usize = 65_536;
const MAX_VIEWPORT_GRAPHEME_COUNT: usize = 1024 * 1024;
const MAX_VIEWPORT_GRAPHEME_BYTES: usize = 16 * 1024 * 1024;
const MIN_STYLE_COMPACTION_LIMIT: usize = 4 * 1024;
const MIN_GRAPHEME_COMPACTION_LIMIT: usize = 16 * 1024;
const MIN_GRAPHEME_BYTE_COMPACTION_LIMIT: usize = 1024 * 1024;
const TMUX_PASSTHROUGH_PREFIX: &[u8] = b"\x1bPtmux;";
const MAX_TMUX_PASSTHROUGH_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AllowPassthrough {
    #[default]
    Off,
    All,
}

impl AllowPassthrough {
    const fn unwraps(self) -> bool {
        matches!(self, Self::All)
    }
}

/// The daemon answers every exit, with a notice for a retained pane and with
/// nothing for one it is about to close, so this cap only has to cover the
/// worst case where the pane is already gone from the daemon's map.
const DEAD_NOTICE_WAIT: Duration = Duration::from_secs(5);
const MAX_ENGINE_SEQUENCE_BYTES: usize = 256;
const MAX_ENGINE_RENAME_BYTES: usize = 1024;
const MAX_ENGINE_OSC_BYTES: usize = 64;

/// `enum progress_bar_state`: what the `ConEmu` OSC 9;4 sequence's first argument
/// selects. A pane that has never seen one is `hidden`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProgressBarState {
    #[default]
    Hidden,
    Normal,
    Error,
    Indeterminate,
    Paused,
}

impl ProgressBarState {
    /// The spellings `format_cb_pane_pb_state` prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Normal => "normal",
            Self::Error => "error",
            Self::Indeterminate => "indeterminate",
            Self::Paused => "paused",
        }
    }

    const fn from_digit(digit: u8) -> Option<Self> {
        Some(match digit {
            b'0' => Self::Hidden,
            b'1' => Self::Normal,
            b'2' => Self::Error,
            b'3' => Self::Indeterminate,
            b'4' => Self::Paused,
            _ => return None,
        })
    }
}

/// `struct progress_bar`: the state and percentage a pane's screen keeps after
/// an OSC 9;4 sequence, read by `pane_pb_state` and `pane_pb_progress`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressBar {
    pub state: ProgressBarState,
    pub progress: u8,
}

impl ProgressBar {
    /// `screen_set_progress_bar`: the state always lands, and the percentage
    /// only when the sequence carried one and the state is not indeterminate.
    fn apply(&mut self, state: ProgressBarState, progress: Option<u8>) {
        self.state = state;
        if let Some(progress) = progress
            && state != ProgressBarState::Indeterminate
        {
            self.progress = progress;
        }
    }
}

/// Pane knobs the pin honors inside its own VT layer: `scroll-on-clear`,
/// `alternate-screen`, `allow-rename`, and the byte `backspace` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineKnobs {
    pub scroll_on_clear: bool,
    pub alternate_screen: bool,
    pub allow_rename: bool,
    pub erase_byte: Option<u8>,
    pub verase_byte: u8,
    /// `mode-keys` resolved for the pane's window. window-copy.c reads the
    /// option live in the cursor geometry, so it rides here rather than only
    /// being latched when a mode opens.
    pub mode_keys_vi: bool,
}

impl Default for EngineKnobs {
    fn default() -> Self {
        Self {
            scroll_on_clear: true,
            alternate_screen: true,
            allow_rename: false,
            erase_byte: Some(0x7f),
            verase_byte: 0x7f,
            mode_keys_vi: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EngineState {
    #[default]
    Ground,
    Escape,
    Csi,
    CsiOverflow,
    Osc,
    Rename,
}

/// Rewrites the byte stream ahead of `vt_write` for the knobs libghostty has
/// no switch for: it scrolls a full-screen erase into history, drops the
/// alternate-screen mode switches, and takes `ESC k` off the wire. Everything
/// else reaches the engine in the spans it arrived in: a chunk is scanned for
/// the two rewrite points and handed over whole when it has none, and only a
/// sequence that straddles two chunks is buffered.
#[derive(Default)]
struct EngineFilter {
    state: EngineState,
    sequence: Vec<u8>,
    title: Vec<u8>,
    /// The OSC payload being collected, passed through to the engine as it is
    /// read; `None` once it outgrew the cap and can no longer be parsed.
    osc: Option<Vec<u8>>,
    bar: ProgressBar,
}

impl EngineFilter {
    fn write(
        &mut self,
        mut bytes: &[u8],
        knobs: EngineKnobs,
        terminal: &mut Terminal<'_, '_>,
        renames: &mut Vec<String>,
        bar: &mut Option<ProgressBar>,
    ) {
        while !bytes.is_empty() {
            match self.state {
                EngineState::Ground => {
                    bytes = self.write_ground(bytes, knobs, terminal);
                }
                EngineState::Escape => match bytes[0] {
                    b'[' => {
                        self.sequence.clear();
                        self.state = EngineState::Csi;
                        bytes = &bytes[1..];
                    }
                    b']' => {
                        terminal.vt_write(b"\x1b]");
                        self.osc = Some(Vec::new());
                        self.state = EngineState::Osc;
                        bytes = &bytes[1..];
                    }
                    b'k' => {
                        self.title.clear();
                        self.state = EngineState::Rename;
                        bytes = &bytes[1..];
                    }
                    _ => {
                        terminal.vt_write(b"\x1b");
                        self.state = EngineState::Ground;
                    }
                },
                EngineState::Csi => {
                    let byte = bytes[0];
                    if byte == 0x1b {
                        terminal.vt_write(b"\x1b[");
                        terminal.vt_write(&self.sequence);
                        self.sequence.clear();
                        self.state = EngineState::Ground;
                        continue;
                    }
                    bytes = &bytes[1..];
                    if byte >= 0x40 {
                        if csi_needs_rewrite(&self.sequence, byte, knobs) {
                            write_engine_csi(&self.sequence, byte, knobs, terminal);
                        } else {
                            let mut raw = Vec::with_capacity(self.sequence.len() + 3);
                            raw.extend_from_slice(b"\x1b[");
                            raw.extend_from_slice(&self.sequence);
                            raw.push(byte);
                            terminal.vt_write(&raw);
                        }
                        self.sequence.clear();
                        self.state = EngineState::Ground;
                    } else if self.sequence.len() >= MAX_ENGINE_SEQUENCE_BYTES {
                        terminal.vt_write(b"\x1b[");
                        terminal.vt_write(&self.sequence);
                        terminal.vt_write(&[byte]);
                        self.sequence.clear();
                        self.state = EngineState::CsiOverflow;
                    } else {
                        self.sequence.push(byte);
                    }
                }
                EngineState::CsiOverflow => {
                    let byte = bytes[0];
                    if byte == 0x1b {
                        self.state = EngineState::Ground;
                        continue;
                    }
                    bytes = &bytes[1..];
                    terminal.vt_write(&[byte]);
                    if byte >= 0x40 {
                        self.state = EngineState::Ground;
                    }
                }
                EngineState::Osc => {
                    bytes = self.write_osc(bytes, terminal, bar);
                }
                EngineState::Rename => {
                    let byte = bytes[0];
                    bytes = &bytes[1..];
                    match byte {
                        0x1b => {
                            self.finish_rename(knobs, renames);
                            self.state = EngineState::Escape;
                        }
                        0x18 | 0x1a => {
                            self.finish_rename(knobs, renames);
                            self.state = EngineState::Ground;
                        }
                        0x00..=0x1f => {}
                        _ => {
                            if self.title.len() < MAX_ENGINE_RENAME_BYTES {
                                self.title.push(byte);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Scan a chunk from the ground state: text and sequences that need no
    /// rewrite accumulate into one span, a rewrite point flushes the span and
    /// writes its replacement, and the remainder of a chunk that ends inside
    /// a sequence is handed back with the state set to resume it.
    fn write_ground<'b>(
        &mut self,
        bytes: &'b [u8],
        knobs: EngineKnobs,
        terminal: &mut Terminal<'_, '_>,
    ) -> &'b [u8] {
        let mut start = 0;
        let mut cursor = 0;
        loop {
            let Some(offset) = find_escape(&bytes[cursor..]) else {
                terminal.vt_write(&bytes[start..]);
                return &[];
            };
            let escape = cursor + offset;
            let Some(&next) = bytes.get(escape + 1) else {
                terminal.vt_write(&bytes[start..escape]);
                self.state = EngineState::Escape;
                return &[];
            };
            match next {
                b'[' => {
                    let mut end = escape + 2;
                    while end < bytes.len() && bytes[end] < 0x40 && bytes[end] != 0x1b {
                        end += 1;
                    }
                    if end >= bytes.len() {
                        let parameters = &bytes[escape + 2..];
                        if parameters.len() >= MAX_ENGINE_SEQUENCE_BYTES {
                            terminal.vt_write(&bytes[start..]);
                            self.state = EngineState::CsiOverflow;
                        } else {
                            terminal.vt_write(&bytes[start..escape]);
                            self.sequence.clear();
                            self.sequence.extend_from_slice(parameters);
                            self.state = EngineState::Csi;
                        }
                        return &[];
                    }
                    if bytes[end] == 0x1b {
                        cursor = end;
                        continue;
                    }
                    let final_byte = bytes[end];
                    let parameters = &bytes[escape + 2..end];
                    if csi_needs_rewrite(parameters, final_byte, knobs) {
                        terminal.vt_write(&bytes[start..escape]);
                        write_engine_csi(parameters, final_byte, knobs, terminal);
                        start = end + 1;
                    }
                    cursor = end + 1;
                }
                b']' => {
                    terminal.vt_write(&bytes[start..escape + 2]);
                    self.osc = Some(Vec::new());
                    self.state = EngineState::Osc;
                    return &bytes[escape + 2..];
                }
                b'k' => {
                    terminal.vt_write(&bytes[start..escape]);
                    self.title.clear();
                    self.state = EngineState::Rename;
                    return &bytes[escape + 2..];
                }
                _ => {
                    cursor = escape + 1;
                }
            }
        }
    }

    /// Reads an OSC payload without changing it: the whole run up to the next
    /// terminator goes to the engine in one write while a bounded copy is kept
    /// for `input_exit_osc`, which runs on every way out of the state: BEL,
    /// CAN and SUB commit the payload like ST does. `ESC` is left for the
    /// escape state, which writes it back as the first byte of ST.
    fn write_osc<'b>(
        &mut self,
        bytes: &'b [u8],
        terminal: &mut Terminal<'_, '_>,
        bar: &mut Option<ProgressBar>,
    ) -> &'b [u8] {
        let end = bytes
            .iter()
            .position(|byte| matches!(byte, 0x07 | 0x18 | 0x1a | 0x1b))
            .unwrap_or(bytes.len());
        self.collect_osc(&bytes[..end]);
        let Some(&terminator) = bytes.get(end) else {
            terminal.vt_write(bytes);
            return &[];
        };
        if terminator == 0x1b {
            terminal.vt_write(&bytes[..end]);
            self.finish_osc(bar);
            self.state = EngineState::Escape;
            return &bytes[end + 1..];
        }
        terminal.vt_write(&bytes[..=end]);
        self.finish_osc(bar);
        self.state = EngineState::Ground;
        &bytes[end + 1..]
    }

    /// `input_state_osc_string_table` appends only 0x20..0xff to the payload;
    /// every other control byte inside an OSC is a null transition the pin
    /// drops.
    fn collect_osc(&mut self, bytes: &[u8]) {
        let Some(osc) = self.osc.as_mut() else {
            return;
        };
        if osc.len() + bytes.len() > MAX_ENGINE_OSC_BYTES {
            self.osc = None;
            return;
        }
        osc.extend(bytes.iter().copied().filter(|byte| *byte >= 0x20));
    }

    /// `input_exit_osc` routes 9 to `input_osc_9`, whose OSC 9;4 grammar is the
    /// only OSC the filter reads.
    fn finish_osc(&mut self, bar: &mut Option<ProgressBar>) {
        let Some(osc) = self.osc.take() else {
            return;
        };
        let Some((state, progress)) = parse_osc_progress(&osc) else {
            return;
        };
        let before = self.bar;
        self.bar.apply(state, progress);
        if self.bar != before {
            *bar = Some(self.bar);
        }
    }

    /// `input_exit_rename`: every way out of the rename state applies the
    /// collected name, and `allow-rename` decides whether it lands.
    fn finish_rename(&mut self, knobs: EngineKnobs, renames: &mut Vec<String>) {
        if knobs.allow_rename
            && let Ok(name) = std::str::from_utf8(&self.title)
        {
            renames.push(name.to_owned());
        }
        self.title.clear();
    }
}

/// `input_exit_osc` reads the leading digits as the OSC number, which must be
/// followed by `;` or the end of the payload, and hands the rest to the
/// per-number handler. Only 9 is read here.
fn parse_osc_progress(payload: &[u8]) -> Option<(ProgressBarState, Option<u8>)> {
    let digits = payload
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(payload.len());
    if digits == 0 {
        return None;
    }
    let mut option = 0_u32;
    for byte in &payload[..digits] {
        option = option
            .saturating_mul(10)
            .saturating_add(u32::from(byte - b'0'));
    }
    let rest = match payload.get(digits) {
        None => &payload[digits..],
        Some(b';') => &payload[digits + 1..],
        Some(_) => return None,
    };
    (option == 9).then(|| parse_osc_9_progress(rest)).flatten()
}

/// `input_osc_9`: `4;<state>` with an optional `;<progress>`, where the state
/// is one digit 0 through 4 and the progress is 0 through 100. A payload that
/// stops after `4` or `4;` is silently ignored, and so is anything malformed;
/// a missing progress leaves the pane's percentage where it was.
fn parse_osc_9_progress(payload: &[u8]) -> Option<(ProgressBarState, Option<u8>)> {
    let rest = payload.strip_prefix(b"4")?;
    if rest.is_empty() || rest == b";" {
        return None;
    }
    let rest = rest.strip_prefix(b";")?;
    let state = ProgressBarState::from_digit(*rest.first()?)?;
    let rest = &rest[1..];
    if rest.is_empty() || rest == b";" {
        return Some((state, None));
    }
    let rest = rest.strip_prefix(b";")?;
    let mut progress = 0_u32;
    let mut digits = 0_usize;
    for byte in rest {
        if !byte.is_ascii_digit() {
            break;
        }
        if progress > 100 {
            return None;
        }
        progress = progress * 10 + u32::from(byte - b'0');
        digits += 1;
    }
    if digits != rest.len() || progress > 100 {
        return None;
    }
    Some((state, Some(u8::try_from(progress).ok()?)))
}

/// The two sequences the filter ever rewrites, decided from the bytes alone
/// so that a chunk with neither passes through in one write: a non-private
/// erase that may cover the whole screen while `scroll-on-clear` is on, and a
/// private mode switch naming an alternate-screen mode while
/// `alternate-screen` is off.
fn csi_needs_rewrite(parameters: &[u8], final_byte: u8, knobs: EngineKnobs) -> bool {
    let private = parameters
        .first()
        .is_some_and(|byte| (0x3c..0x40).contains(byte));
    match final_byte {
        b'J' if knobs.scroll_on_clear && !private => {
            let first = parameters.split(|byte| *byte == b';').next().unwrap_or(&[]);
            matches!(engine_parameter(first).unwrap_or(0), 0 | 2)
        }
        b'h' | b'l' if !knobs.alternate_screen && parameters.first() == Some(&b'?') => parameters
            [1..]
            .split(|byte| *byte == b';')
            .any(|parameter| matches!(engine_parameter(parameter), Some(47 | 1047 | 1049))),
        _ => false,
    }
}

/// `screen_write_clearscreen` and, with the cursor at the origin,
/// `screen_write_clearendofscreen` hand a full-screen erase to
/// `grid_view_clear_history`, which scrolls every used row into history first.
/// libghostty always takes the other branch, so the used-row count goes back
/// on the wire as an SU ahead of the erase.
fn write_engine_csi(
    sequence: &[u8],
    final_byte: u8,
    knobs: EngineKnobs,
    terminal: &mut Terminal<'_, '_>,
) {
    let controls = sequence
        .iter()
        .copied()
        .filter(|byte| *byte < 0x20)
        .collect::<Vec<u8>>();
    let cleaned;
    let sequence = if controls.is_empty() {
        sequence
    } else {
        terminal.vt_write(&controls);
        cleaned = sequence
            .iter()
            .copied()
            .filter(|byte| *byte >= 0x20)
            .collect::<Vec<u8>>();
        cleaned.as_slice()
    };
    let private = sequence
        .first()
        .is_some_and(|byte| (0x3c..0x40).contains(byte));
    if knobs.scroll_on_clear
        && final_byte == b'J'
        && !private
        && erases_whole_screen(sequence, terminal)
        && terminal
            .active_screen()
            .is_ok_and(|screen| screen == Screen::Primary)
        && let Some(rows) = used_screen_rows(terminal)
    {
        terminal.vt_write(format!("\x1b[{rows}S").as_bytes());
    }
    if !knobs.alternate_screen
        && matches!(final_byte, b'h' | b'l')
        && sequence.first() == Some(&b'?')
    {
        let kept = sequence[1..]
            .split(|byte| *byte == b';')
            .filter(|parameter| !matches!(engine_parameter(parameter), Some(47 | 1047 | 1049)))
            .collect::<Vec<_>>();
        if kept.is_empty() {
            return;
        }
        let mut rewritten = b"\x1b[?".to_vec();
        for (index, parameter) in kept.iter().enumerate() {
            if index > 0 {
                rewritten.push(b';');
            }
            rewritten.extend_from_slice(parameter);
        }
        rewritten.push(final_byte);
        terminal.vt_write(&rewritten);
        return;
    }
    let mut raw = Vec::with_capacity(sequence.len() + 3);
    raw.extend_from_slice(b"\x1b[");
    raw.extend_from_slice(sequence);
    raw.push(final_byte);
    terminal.vt_write(&raw);
}

fn engine_parameter(parameter: &[u8]) -> Option<u32> {
    if parameter.is_empty() || !parameter.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(parameter).ok()?.parse().ok()
}

fn erases_whole_screen(sequence: &[u8], terminal: &Terminal<'_, '_>) -> bool {
    let first = sequence.split(|byte| *byte == b';').next().unwrap_or(&[]);
    match engine_parameter(first).unwrap_or(0) {
        2 => true,
        0 => terminal.cursor_x().unwrap_or(1) == 0 && terminal.cursor_y().unwrap_or(1) == 0,
        _ => false,
    }
}

/// `grid_view_clear_history` scrolls one line per row up to the last row that
/// holds a written cell, which is the pin's `cellused != 0` test.
fn used_screen_rows(terminal: &Terminal<'_, '_>) -> Option<u16> {
    let rows = terminal.rows().ok()?;
    let columns = terminal.cols().ok()?;
    (0..rows).rev().find_map(|row| {
        let used = (0..columns).any(|column| {
            terminal
                .grid_ref(Point::Active(PointCoordinate {
                    x: column,
                    y: u32::from(row),
                }))
                .ok()
                .and_then(|reference| reference.cell().ok())
                .and_then(|cell| cell.has_text().ok())
                .unwrap_or(false)
        });
        used.then_some(row + 1)
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PassthroughState {
    #[default]
    Ground,
    Prefix(usize),
    Payload {
        escape: bool,
    },
    Discard {
        escape: bool,
    },
}

#[derive(Default)]
struct PassthroughFilter {
    mode: AllowPassthrough,
    state: PassthroughState,
    payload: Vec<u8>,
}

impl PassthroughFilter {
    fn set_mode(&mut self, mode: AllowPassthrough) {
        self.mode = mode;
    }

    #[inline]
    fn start_payload(&mut self) {
        self.state = PassthroughState::Payload { escape: false };
    }

    #[inline]
    fn append_payload(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() > MAX_TMUX_PASSTHROUGH_PAYLOAD_BYTES.saturating_sub(self.payload.len()) {
            self.payload = Vec::new();
            false
        } else {
            self.payload.extend_from_slice(bytes);
            true
        }
    }

    #[inline]
    fn write(&mut self, mut bytes: &[u8], mut sink: impl FnMut(&[u8])) -> usize {
        let mut written = 0;
        loop {
            match self.state {
                PassthroughState::Ground => {
                    let Some(escape) = find_escape(bytes) else {
                        if !bytes.is_empty() {
                            sink(bytes);
                            written += bytes.len();
                        }
                        return written;
                    };
                    if escape > 0 {
                        sink(&bytes[..escape]);
                        written += escape;
                        bytes = &bytes[escape..];
                    }

                    let matched = common_prefix(bytes, TMUX_PASSTHROUGH_PREFIX);
                    if matched == TMUX_PASSTHROUGH_PREFIX.len() {
                        self.start_payload();
                        bytes = &bytes[matched..];
                    } else if matched == bytes.len() {
                        self.state = PassthroughState::Prefix(matched);
                        return written;
                    } else {
                        let next = find_escape(&bytes[1..]).map_or(bytes.len(), |next| next + 1);
                        sink(&bytes[..next]);
                        written += next;
                        bytes = &bytes[next..];
                    }
                }
                PassthroughState::Prefix(matched) => {
                    let remaining = &TMUX_PASSTHROUGH_PREFIX[matched..];
                    let continued = common_prefix(bytes, remaining);
                    if continued == remaining.len() {
                        self.start_payload();
                        bytes = &bytes[continued..];
                    } else if continued == bytes.len() {
                        self.state = PassthroughState::Prefix(matched + continued);
                        return written;
                    } else {
                        sink(&TMUX_PASSTHROUGH_PREFIX[..matched]);
                        written += matched;
                        self.state = PassthroughState::Ground;
                    }
                }
                PassthroughState::Payload { escape: false } => {
                    let Some(escape) = find_escape(bytes) else {
                        if !self.append_payload(bytes) {
                            self.state = PassthroughState::Discard { escape: false };
                        }
                        return written;
                    };
                    self.state = if self.append_payload(&bytes[..escape]) {
                        PassthroughState::Payload { escape: true }
                    } else {
                        PassthroughState::Discard { escape: true }
                    };
                    bytes = &bytes[escape + 1..];
                }
                PassthroughState::Payload { escape: true } => {
                    let Some((&next, remaining)) = bytes.split_first() else {
                        return written;
                    };
                    match next {
                        0x1b => {
                            self.state = if self.append_payload(&[0x1b]) {
                                PassthroughState::Payload { escape: false }
                            } else {
                                PassthroughState::Discard { escape: false }
                            };
                            bytes = remaining;
                        }
                        b'\\' => {
                            if self.mode.unwraps() && !self.payload.is_empty() {
                                sink(&self.payload);
                                written += self.payload.len();
                            }
                            self.payload.clear();
                            self.state = PassthroughState::Ground;
                            bytes = remaining;
                        }
                        _ => {
                            self.state = if self.append_payload(&[next]) {
                                PassthroughState::Payload { escape: false }
                            } else {
                                PassthroughState::Discard { escape: false }
                            };
                            bytes = remaining;
                        }
                    }
                }
                PassthroughState::Discard { escape: false } => {
                    let Some(escape) = find_escape(bytes) else {
                        return written;
                    };
                    self.state = PassthroughState::Discard { escape: true };
                    bytes = &bytes[escape + 1..];
                }
                PassthroughState::Discard { escape: true } => {
                    let Some((&next, remaining)) = bytes.split_first() else {
                        return written;
                    };
                    if next == b'\\' {
                        self.state = PassthroughState::Ground;
                    } else {
                        self.state = PassthroughState::Discard { escape: false };
                    }
                    bytes = remaining;
                }
            }
        }
    }
}

#[inline]
/// `spawn.c` copies the session termios into the child and then forces
/// `VERASE` to the key the `backspace` option names.
#[cfg(unix)]
fn force_pty_erase(descriptor: std::os::fd::RawFd, erase: u8) {
    use rustix::termios::{OptionalActions, SpecialCodeIndex, tcgetattr, tcsetattr};

    let Ok(handle) = filedescriptor::FileDescriptor::dup(&descriptor) else {
        return;
    };
    let Ok(mut termios) = tcgetattr(&handle) else {
        return;
    };
    termios.special_codes[SpecialCodeIndex::VERASE] = erase;
    let _ = tcsetattr(&handle, OptionalActions::Now, &termios);
}

fn find_escape(bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|byte| *byte == 0x1b)
}

fn common_prefix(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

fn diagnostic_timer() -> Option<Instant> {
    log::log_enabled!(target: "zz_terminal::diagnostics", log::Level::Trace).then(Instant::now)
}

fn diagnostic_elapsed_us(started: Option<Instant>) -> u128 {
    started.map_or(0, |started| started.elapsed().as_micros())
}

#[derive(Default)]
struct VtWriteDiagnostics {
    bytes: usize,
    calls: u32,
    micros: u128,
}

impl VtWriteDiagnostics {
    fn record(&mut self, bytes: usize, started: Option<Instant>) {
        if started.is_some() && bytes > 0 {
            self.bytes = self.bytes.saturating_add(bytes);
            self.calls = self.calls.saturating_add(1);
            self.micros = self.micros.saturating_add(diagnostic_elapsed_us(started));
        }
    }

    fn emit(&mut self) {
        if self.calls > 0 {
            log::trace!(
                target: "zz_terminal::diagnostics::vt",
                "vt_write parsed_bytes={} calls={} elapsed_us={}",
                self.bytes,
                self.calls,
                self.micros,
            );
            *self = Self::default();
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WheelRoute {
    ApplicationMouse,
    AlternateScroll,
    Viewport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureBoundary {
    HistoryStart,
    VisibleEnd,
    Relative(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "these independent switches directly model orthogonal capture-pane flags"
)]
pub struct CaptureOptions {
    pub start: CaptureBoundary,
    pub end: CaptureBoundary,
    pub alternate: bool,
    pub mode: bool,
    pub join_wrapped: bool,
    pub preserve_trailing: bool,
    pub escape_sequences: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            start: CaptureBoundary::Relative(0),
            end: CaptureBoundary::VisibleEnd,
            alternate: false,
            mode: false,
            join_wrapped: false,
            preserve_trailing: false,
            escape_sequences: false,
        }
    }
}

/// The last completed command and its output, read from the OSC 133 marks
/// libghostty records on rows and cells. `133;D` exit status is not exposed
/// by the Rust API, so it is absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LastCommandCapture {
    pub command: String,
    pub output: String,
    /// Leading output rows dropped by the line or byte cap; zero when none were.
    pub truncated_rows: usize,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TerminalCaptureError {
    #[error("terminal actor stopped")]
    ActorStopped,
    #[error("terminal capture timed out")]
    TimedOut,
    #[error("alternate screen is not active")]
    AlternateUnavailable,
    #[error("pane is not in a native mode")]
    ModeUnavailable,
    #[error("no shell-integration marks in this pane's scrollback")]
    NoSemanticMarks,
    #[error("terminal capture exceeds the {MAX_CAPTURE_BYTES}-byte limit")]
    TooLarge,
    #[error("terminal capture failed: {0}")]
    Failed(String),
}

/// Cold payload produced after a native selection copy completes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCopyReady {
    pub request_id: u64,
    pub clipboard: Option<ClipboardTarget>,
    pub buffer: Option<PasteBufferAction>,
    pub pipe: Option<String>,
    pub text: String,
    view_changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalOpenUri {
    pub view: TerminalViewId,
    pub uri: String,
}

/// A coalesced notification that a newer terminal snapshot is available or a
/// reliable terminal-side effect is ready for the daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    ViewportReady {
        output_activity: bool,
    },
    ViewClosed(TerminalViewId),
    CopyReady {
        view: TerminalViewId,
        copy: Box<TerminalCopyReady>,
    },
    OpenUri(Box<TerminalOpenUri>),
    /// The program in the pane wrote to a clipboard itself (OSC 52, iTerm2
    /// OSC 1337 Copy). Pane-scoped: every viewer of the pane is the audience.
    ClipboardSet {
        target: ClipboardTarget,
        text: String,
    },
    /// The program wrote `ESC k <name> ST` while `allow-rename` was on.
    RenameWindow(String),
    /// The program rang BEL. Raised once per occurrence.
    Bell,
    PlaceholderBound {
        token: u64,
        number: u32,
    },
    PendingPasteExpired {
        token: u64,
    },
    RawOutputTapClosed {
        token: u64,
    },
}

/// Single-consumer terminal event stream with bounded reliable-event accounting.
#[derive(Clone)]
pub struct TerminalEvents {
    receiver: async_channel::Receiver<TerminalEvent>,
    state: Arc<EventQueueState>,
}

struct ForegroundSource {
    #[cfg(unix)]
    master: Arc<parking_lot::Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    shell: Option<u32>,
    tty: Option<PathBuf>,
}

impl ForegroundSource {
    fn process_id(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            self.master
                .lock()
                .process_group_leader()
                .and_then(|group| u32::try_from(group).ok())
                .filter(|process_id| *process_id != 0)
                .or_else(|| self.shell_process_id())
        }
        #[cfg(not(unix))]
        {
            self.shell_process_id()
        }
    }

    fn shell_process_id(&self) -> Option<u32> {
        self.shell.filter(|process_id| *process_id != 0)
    }
}

#[repr(C)]
struct EventQueueState {
    pending_reliable: AtomicUsize,
    pending_reliable_bytes: AtomicUsize,
    notification_pending: AtomicBool,
    output_activity_pending: AtomicBool,
    foreground: RwLock<Option<Box<ForegroundSource>>>,
    completion: AtomicU64,
}

impl EventQueueState {
    const fn new() -> Self {
        Self {
            pending_reliable: AtomicUsize::new(0),
            pending_reliable_bytes: AtomicUsize::new(0),
            notification_pending: AtomicBool::new(false),
            output_activity_pending: AtomicBool::new(false),
            foreground: RwLock::new(None),
            completion: AtomicU64::new(0),
        }
    }
}

impl TerminalEvents {
    fn received(&self, event: &mut TerminalEvent) {
        if let TerminalEvent::ViewportReady { output_activity } = event {
            self.state
                .notification_pending
                .store(false, Ordering::Release);
            *output_activity = self
                .state
                .output_activity_pending
                .swap(false, Ordering::AcqRel);
            return;
        }

        let previous = self.state.pending_reliable.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "reliable terminal event accounting underflow");
        let bytes = reliable_event_bytes(event);
        let previous_bytes = self
            .state
            .pending_reliable_bytes
            .fetch_sub(bytes, Ordering::AcqRel);
        debug_assert!(
            previous_bytes >= bytes,
            "reliable terminal event byte accounting underflow"
        );
    }

    /// Receives the next terminal event, blocking the caller.
    pub fn recv_blocking(&self) -> Result<TerminalEvent, async_channel::RecvError> {
        let mut event = self.receiver.recv_blocking()?;
        self.received(&mut event);
        Ok(event)
    }

    pub fn try_recv(&self) -> Result<TerminalEvent, async_channel::TryRecvError> {
        let mut event = self.receiver.try_recv()?;
        self.received(&mut event);
        Ok(event)
    }
}

fn terminal_event_channel(
    state: &Arc<EventQueueState>,
) -> (async_channel::Sender<TerminalEvent>, TerminalEvents) {
    let (sender, receiver) = async_channel::bounded(MAX_PENDING_TERMINAL_EVENTS);
    let events = TerminalEvents {
        receiver,
        state: Arc::clone(state),
    };
    (sender, events)
}

struct PublishedViewports {
    fallback: Arc<TerminalViewport>,
    by_view: HashMap<TerminalViewId, Arc<TerminalViewport>>,
    copy_facts: HashMap<TerminalViewId, Arc<CopyModeFacts>>,
    frozen: Option<Arc<FrozenHistory>>,
    bar: ProgressBar,
}

impl PublishedViewports {
    fn new(viewport: TerminalViewport) -> Self {
        Self {
            fallback: Arc::new(viewport),
            by_view: HashMap::new(),
            copy_facts: HashMap::new(),
            frozen: None,
            bar: ProgressBar::default(),
        }
    }
}

/// The whole grid a retained pane keeps after its child exits: scrollback plus
/// the visible screen, so `capture-pane` answers negative `-S` boundaries on a
/// dead pane the way `screen_write_collect_add` left them on the pin, where the
/// pane's `struct screen` simply outlives the process.
#[derive(Debug)]
pub struct FrozenHistory {
    revision: Arc<ModeRevision>,
}

/// What `window_copy_formats` reads off one pane's mode entry, computed for
/// one client's frozen view. tmux keeps this on the pane because the mode
/// entry is the pane's; zz keeps it per view because copy mode is per client,
/// so the daemon reads whichever view the format tree's client owns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyModeFacts {
    /// `view-mode` rather than `copy-mode` for the read-only output overlay.
    pub view_mode: bool,
    /// `data->cx`.
    pub cursor_x: u32,
    /// `data->cy`, the cursor's row inside the visible screen.
    pub cursor_y: u32,
    /// `window_copy_get_line`: the cursor's row with trailing blanks trimmed.
    pub cursor_line: String,
    /// `window_copy_get_word`: the word the cursor sits in, or the one after
    /// it when the cursor is on a single separator.
    pub cursor_word: String,
    /// `data->oy`, rows between the visible screen and the bottom.
    pub scroll_position: u32,
    /// `data->screen.sel`, whose absence removes the four coordinates too.
    pub selection: Option<CopyModeSelectionFacts>,
    /// `data->searchmark != NULL`.
    pub search_present: bool,
    /// `data->searchcount` and `data->searchmore`, both absent while the pin
    /// holds -1 in the count.
    pub search_count: Option<(u32, bool)>,
    /// `data->timeout`, which zz's synchronous copy-mode search never sets.
    pub search_timed_out: bool,
    /// `window_copy_match_at_cursor`: the marked text the cursor stands in.
    pub search_match: String,
}

/// `data->selx`, `sely`, `endselx` and `endsely`: grid rows counted from the
/// top of the history, not from the top of the visible screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyModeSelectionFacts {
    pub start_x: u32,
    pub start_y: u32,
    pub end_x: u32,
    pub end_y: u32,
}

/// What a terminal pane runs and the environment it starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalProcessExit {
    pub code: u32,
    pub signal: Option<u8>,
}

impl TerminalProcessExit {
    const PRESENT: u64 = 1 << 63;

    fn encode(self) -> u64 {
        Self::PRESENT | u64::from(self.code) | (self.signal.map_or(0, u64::from) << u32::BITS)
    }

    fn decode(value: u64) -> Option<Self> {
        if value & Self::PRESENT == 0 {
            return None;
        }
        let code = u32::try_from(value & u64::from(u32::MAX)).expect("masked to u32");
        let signal = u8::try_from((value >> u32::BITS) & u64::from(u8::MAX)).expect("masked to u8");
        Some(Self {
            code,
            signal: (signal != 0).then_some(signal),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct TerminalSpawn {
    pub knobs: EngineKnobs,
    pub working_directory: Option<PathBuf>,
    pub command: Option<Vec<String>>,
    pub shell: Option<String>,
    pub terminal_type: Option<String>,
    pub initial_size: Option<TerminalSize>,
    pub non_login_shell: bool,
    /// Extra environment (`ZZ_PANE` etc.) layered over the defaults. A Unix
    /// environment name and value are byte strings, so this carries `OsString`
    /// rather than text and a non-UTF-8 entry reaches the child verbatim.
    pub env: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>,
}

pub struct TerminalSession {
    commands: CommandSender,
    events: TerminalEvents,
    latest: Arc<RwLock<PublishedViewports>>,
    max_scrollback: usize,
    word_separators: RwLock<WordSeparators>,
    applied_appearance: AtomicU64,
    terminating: AtomicBool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSessionDiagnostics {
    pub command_queue_len: usize,
    pub command_queue_capacity: Option<usize>,
    pub pending_pty_input_bytes: usize,
    pub event_queue_len: usize,
    pub event_queue_capacity: Option<usize>,
    pub pending_reliable_events: usize,
    pub pending_reliable_bytes: usize,
    pub viewport_notification_pending: bool,
    pub max_scrollback: usize,
    pub viewport_generation: u64,
    pub viewport_view_generation: u64,
    pub viewport_dictionary_generation: u32,
    pub viewport_columns: u16,
    pub viewport_rows: u16,
    pub viewport_cells: usize,
    pub viewport_cell_bytes: usize,
    pub viewport_overlays: usize,
    pub viewport_strong_count: usize,
    pub viewport_cell_arc_strong_count: usize,
    pub viewport_dictionary_arc_strong_count: usize,
    pub viewport_overlay_arc_strong_count: usize,
    pub viewport_styles: usize,
    pub viewport_graphemes: usize,
    pub viewport_grapheme_bytes: usize,
    pub viewport_title: Arc<str>,
    pub viewport_status: SessionStatus,
}

/// One decoded Kitty image copied out of libghostty on the terminal actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KittyImage {
    pub image_id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    /// Premultiplied BGRA8 pixels, ready for GPUI's image atlas.
    pub bgra: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum KittyImageRequestError {
    #[error("terminal actor did not answer the Kitty image request in time")]
    TimedOut,
    #[error("terminal actor stopped before answering the Kitty image request")]
    ActorStopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum RawOutputTapError {
    #[error("terminal actor did not answer the raw output tap request in time")]
    TimedOut,
    #[error("terminal actor stopped before answering the raw output tap request")]
    ActorStopped,
    #[error("terminal surface has no PTY output")]
    Unavailable,
}

impl std::fmt::Debug for TerminalSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalSession")
            .finish_non_exhaustive()
    }
}

impl TerminalSession {
    /// Starts the platform's default shell in a new terminal actor.
    ///
    /// The scrollback limit is fixed at spawn; a later mux-option change reaches
    /// only new panes. A `None` working directory inherits the daemon's.
    #[must_use]
    pub fn spawn(
        max_scrollback: usize,
        appearance: Arc<TerminalAppearance>,
        spawn: TerminalSpawn,
    ) -> Self {
        let max_scrollback = max_scrollback.min(MAX_HISTORY_LIMIT);
        let (command_tx, command_rx) = command_channel();
        let (input_tx, input_rx) = input_channel();
        let (wake, wake_rx) = actor_wake();
        let commands = CommandSender {
            queues: Box::new(CommandQueues {
                control: command_tx,
                input: Some(input_tx),
            }),
            wake: wake.clone(),
        };
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let latest = Arc::new(RwLock::new(PublishedViewports::new(
            TerminalViewport::blank_with_appearance(
                spawn
                    .initial_size
                    .map_or(INITIAL_COLUMNS, |size| size.columns),
                spawn.initial_size.map_or(INITIAL_ROWS, |size| size.rows),
                SessionStatus::Starting,
                &appearance,
            ),
        )));
        let publisher = Publisher {
            event_tx,
            latest: Arc::clone(&latest),
            state: event_state,
        };

        let worker_publisher = publisher.clone();
        let appearance_hash = appearance.stable_hash();
        if let Err(error) = thread::Builder::new()
            .name("zz-terminal".into())
            .spawn(move || {
                terminal_worker(
                    command_rx,
                    input_rx,
                    worker_publisher,
                    max_scrollback,
                    appearance,
                    spawn,
                    wake,
                    wake_rx,
                );
            })
        {
            publisher.fail(&WorkerError::Thread(error.to_string()));
        }

        Self {
            commands,
            events,
            latest,
            max_scrollback,
            word_separators: RwLock::new(WordSeparators::default()),
            applied_appearance: AtomicU64::new(appearance_hash),
            terminating: AtomicBool::new(false),
        }
    }

    /// Starts a PTY-free, frozen surface for native command output. It takes the
    /// same resize, selection, search, and copy-mode actions as a live terminal
    /// but never spawns a shell, and exits when its view mode is cancelled.
    #[must_use]
    pub fn spawn_output_view(title: String, text: String) -> Self {
        Self::spawn_output_view_with_appearance(
            title,
            text,
            Arc::new(TerminalAppearance::default()),
        )
    }

    /// Start a PTY-free command-output surface with a resolved appearance.
    #[must_use]
    pub fn spawn_output_view_with_appearance(
        title: String,
        text: String,
        appearance: Arc<TerminalAppearance>,
    ) -> Self {
        Self::spawn_surface_with_appearance(
            title,
            text,
            appearance,
            MAX_OUTPUT_VIEW_SCROLLBACK,
            true,
        )
    }

    #[must_use]
    pub fn spawn_startup_output_view_with_appearance(
        title: String,
        text: String,
        appearance: Arc<TerminalAppearance>,
    ) -> Self {
        Self::spawn_surface_with_appearance(
            title,
            text,
            appearance,
            MAX_STARTUP_OUTPUT_VIEW_SCROLLBACK,
            true,
        )
    }

    #[must_use]
    pub fn spawn_empty_with_appearance(
        max_scrollback: usize,
        appearance: Arc<TerminalAppearance>,
    ) -> Self {
        Self::spawn_surface_with_appearance(
            String::new(),
            String::new(),
            appearance,
            max_scrollback.min(MAX_HISTORY_LIMIT),
            false,
        )
    }

    fn spawn_surface_with_appearance(
        title: String,
        text: String,
        appearance: Arc<TerminalAppearance>,
        max_scrollback: usize,
        frozen: bool,
    ) -> Self {
        let (command_tx, command_rx) = command_channel();
        let commands = CommandSender {
            queues: Box::new(CommandQueues {
                control: command_tx,
                input: None,
            }),
            wake: ActorWake::none(),
        };
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let latest = Arc::new(RwLock::new(PublishedViewports::new(
            TerminalViewport::blank_with_appearance(
                INITIAL_COLUMNS,
                INITIAL_ROWS,
                SessionStatus::Starting,
                &appearance,
            ),
        )));
        let publisher = Publisher {
            event_tx,
            latest: Arc::clone(&latest),
            state: event_state,
        };

        let worker_publisher = publisher.clone();
        let appearance_hash = appearance.stable_hash();
        if let Err(error) = thread::Builder::new()
            .name(if frozen {
                "zz-output-view".into()
            } else {
                "zz-empty-pane".into()
            })
            .spawn(move || {
                output_view_worker(
                    command_rx,
                    worker_publisher,
                    title,
                    text,
                    appearance,
                    max_scrollback,
                    frozen,
                );
            })
        {
            publisher.fail(&WorkerError::Thread(error.to_string()));
        }

        Self {
            commands,
            events,
            latest,
            max_scrollback,
            word_separators: RwLock::new(WordSeparators::default()),
            applied_appearance: AtomicU64::new(appearance_hash),
            terminating: AtomicBool::new(false),
        }
    }

    /// The scrollback limit this actor captured at spawn.
    #[must_use]
    pub const fn max_scrollback(&self) -> usize {
        self.max_scrollback
    }

    /// The process in the PTY foreground group, queried on each call. `None` for
    /// output views, actors that have not started, and exited sessions.
    #[must_use]
    pub fn foreground_process_id(&self) -> Option<u32> {
        self.events.state.foreground.read().as_ref()?.process_id()
    }

    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        self.events
            .state
            .foreground
            .read()
            .as_ref()?
            .shell_process_id()
    }

    #[must_use]
    pub fn completion(&self) -> Option<TerminalProcessExit> {
        TerminalProcessExit::decode(self.events.state.completion.load(Ordering::Acquire))
    }

    pub fn terminate(&self) {
        if !self.terminating.swap(true, Ordering::AcqRel) {
            self.send_command(Command::Terminate);
        }
    }

    #[must_use]
    pub fn tty(&self) -> Option<PathBuf> {
        self.events.state.foreground.read().as_ref()?.tty.clone()
    }

    /// Replaces the word classifier used by desktop selection and copy mode.
    /// Applied in command order, without touching the PTY.
    pub fn set_word_separators(&self, separators: WordSeparators) {
        *self.word_separators.write() = separators.clone();
        self.send_command(Command::SetWordSeparators(Box::new(separators)));
    }

    /// Apply new renderer defaults without replacing PTY or viewport state.
    /// Re-sends of a byte-identical appearance are dropped so callers can
    /// refresh eagerly without forcing terminal re-renders.
    pub fn set_appearance(&self, appearance: Arc<TerminalAppearance>) {
        let hash = appearance.stable_hash();
        if self.applied_appearance.swap(hash, Ordering::AcqRel) == hash {
            return;
        }
        self.send_command(Command::SetAppearance(appearance));
    }

    pub fn set_allow_passthrough(&self, enabled: bool) {
        // The daemon folds tmux `on` into `all` because this worker has no pane-visibility signal.
        self.send_command(Command::SetAllowPassthrough(if enabled {
            AllowPassthrough::All
        } else {
            AllowPassthrough::Off
        }));
    }

    pub fn set_wrap_search(&self, enabled: bool) {
        self.send_command(Command::SetWrapSearch(enabled));
    }

    pub fn set_engine_knobs(&self, knobs: EngineKnobs) {
        self.send_command(Command::SetEngineKnobs(knobs));
    }

    /// `window_copy_clone_screen` runs on the source pane, so the revision a
    /// `copy-mode -s` entry needs has to be built on that pane's own worker.
    pub fn capture_copy_source(&self) -> Result<CapturedCopySource, TerminalCaptureError> {
        let (reply, response) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        self.commands
            .send_timeout(Command::CaptureCopySource { reply }, CAPTURE_TIMEOUT)
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => TerminalCaptureError::TimedOut,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                    TerminalCaptureError::ActorStopped
                }
            })?;
        response
            .recv_timeout(CAPTURE_TIMEOUT.saturating_sub(started.elapsed()))
            .map_err(|error| match error {
                crossbeam_channel::RecvTimeoutError::Timeout => TerminalCaptureError::TimedOut,
                crossbeam_channel::RecvTimeoutError::Disconnected => {
                    TerminalCaptureError::ActorStopped
                }
            })?
    }

    /// Hands a retained pane its expanded `remain-on-exit-format` while the
    /// worker is still holding the VT open after the child exited. `None`
    /// releases the worker without drawing anything.
    pub fn write_dead_notice(&self, text: Option<Arc<str>>) {
        self.send_command(Command::WriteDeadNotice(text));
    }

    /// Arms the revision the next copy-mode entry on this worker takes instead
    /// of cloning the pane's own screen.
    pub fn set_pending_copy_source(&self, source: Option<Box<CapturedCopySource>>) {
        self.send_command(Command::SetPendingCopySource(source));
    }

    /// Apply pinned tmux's `send-keys -R` pane reset, preserving scrollback.
    pub fn reset_screen(&self) {
        self.send_command(Command::ResetScreen);
    }

    #[must_use]
    pub fn word_separators(&self) -> WordSeparators {
        self.word_separators.read().clone()
    }

    /// Clone the single-consumer event stream used by the UI.
    #[must_use]
    pub fn events(&self) -> TerminalEvents {
        self.events.clone()
    }

    /// `wp->base.progress_bar`, which `pane_pb_state` and `pane_pb_progress`
    /// read. A pane that has seen no OSC 9;4 answers hidden and zero.
    #[must_use]
    pub fn progress_bar(&self) -> ProgressBar {
        self.latest.read().bar
    }

    #[must_use]
    pub fn latest_viewport(&self) -> Arc<TerminalViewport> {
        Arc::clone(&self.latest.read().fallback)
    }

    #[must_use]
    pub fn latest_viewport_for(&self, view: TerminalViewId) -> Option<Arc<TerminalViewport>> {
        self.latest.read().by_view.get(&view).cloned()
    }

    #[must_use]
    pub fn latest_viewports(&self) -> HashMap<TerminalViewId, Arc<TerminalViewport>> {
        self.latest.read().by_view.clone()
    }

    /// The copy-mode facts published for one view, absent whenever that view
    /// holds no frozen mode.
    #[must_use]
    pub fn copy_mode_facts(&self, view: TerminalViewId) -> Option<Arc<CopyModeFacts>> {
        self.latest.read().copy_facts.get(&view).cloned()
    }

    /// Copy one stored Kitty image from the actor-owned VT as premultiplied BGRA8.
    pub fn kitty_image(&self, image_id: u32) -> Result<Option<KittyImage>, KittyImageRequestError> {
        let (reply, response) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        self.commands
            .send_timeout(
                Command::KittyImage(Box::new(KittyImageRequest { image_id, reply })),
                CAPTURE_TIMEOUT,
            )
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => KittyImageRequestError::TimedOut,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                    KittyImageRequestError::ActorStopped
                }
            })?;
        response
            .recv_timeout(CAPTURE_TIMEOUT.saturating_sub(started.elapsed()))
            .map_err(|error| match error {
                crossbeam_channel::RecvTimeoutError::Timeout => KittyImageRequestError::TimedOut,
                crossbeam_channel::RecvTimeoutError::Disconnected => {
                    KittyImageRequestError::ActorStopped
                }
            })
    }

    /// Read an image's current storage generation without copying its pixels.
    pub fn kitty_image_generation(
        &self,
        image_id: u32,
    ) -> Result<Option<u64>, KittyImageRequestError> {
        let (reply, response) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        self.commands
            .send_timeout(
                Command::KittyImageGeneration(Box::new(KittyImageGenerationRequest {
                    image_id,
                    reply,
                })),
                CAPTURE_TIMEOUT,
            )
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => KittyImageRequestError::TimedOut,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                    KittyImageRequestError::ActorStopped
                }
            })?;
        response
            .recv_timeout(CAPTURE_TIMEOUT.saturating_sub(started.elapsed()))
            .map_err(|error| match error {
                crossbeam_channel::RecvTimeoutError::Timeout => KittyImageRequestError::TimedOut,
                crossbeam_channel::RecvTimeoutError::Disconnected => {
                    KittyImageRequestError::ActorStopped
                }
            })
    }

    #[must_use]
    pub fn diagnostics(&self) -> TerminalSessionDiagnostics {
        let viewport = self.latest_viewport();
        let (_, pending_pty_input_bytes) = self.commands.pending_input();
        TerminalSessionDiagnostics {
            command_queue_len: self.commands.len(),
            command_queue_capacity: self.commands.capacity(),
            pending_pty_input_bytes,
            event_queue_len: self.events.receiver.len(),
            event_queue_capacity: self.events.receiver.capacity(),
            pending_reliable_events: self.events.state.pending_reliable.load(Ordering::Acquire),
            pending_reliable_bytes: self
                .events
                .state
                .pending_reliable_bytes
                .load(Ordering::Acquire),
            viewport_notification_pending: self
                .events
                .state
                .notification_pending
                .load(Ordering::Acquire),
            max_scrollback: self.max_scrollback,
            viewport_generation: viewport.generation,
            viewport_view_generation: viewport.view_generation,
            viewport_dictionary_generation: viewport.dictionary_generation,
            viewport_columns: viewport.columns,
            viewport_rows: viewport.rows,
            viewport_cells: viewport.cells.len(),
            viewport_cell_bytes: std::mem::size_of_val(viewport.cells.as_ref()),
            viewport_overlays: viewport.overlays.len(),
            viewport_strong_count: Arc::strong_count(&viewport),
            viewport_cell_arc_strong_count: Arc::strong_count(&viewport.cells),
            viewport_dictionary_arc_strong_count: Arc::strong_count(&viewport.dictionary),
            viewport_overlay_arc_strong_count: Arc::strong_count(&viewport.overlays),
            viewport_styles: viewport.dictionary.styles.len(),
            viewport_graphemes: viewport.dictionary.grapheme_offsets.len(),
            viewport_grapheme_bytes: viewport.dictionary.grapheme_bytes.len(),
            viewport_title: Arc::clone(&viewport.presentation.title),
            viewport_status: viewport.status.clone(),
        }
    }

    pub fn send_text(&self, text: impl Into<Arc<str>>) {
        self.try_send_text(text);
    }

    pub fn try_send_text(&self, text: impl Into<Arc<str>>) -> bool {
        self.send_command(Command::Text {
            view: None,
            text: text.into(),
        })
    }

    pub fn send_text_for_view(&self, view: TerminalViewId, text: impl Into<Arc<str>>) {
        self.send_command(Command::Text {
            view: Some(view),
            text: text.into(),
        });
    }

    /// Send a physical key event through libghostty's key encoder.
    pub fn send_key(&self, input: KeyInput) {
        self.try_send_key(input);
    }

    pub fn try_send_key(&self, input: KeyInput) -> bool {
        self.send_command(Command::Key {
            view: None,
            input: Box::new(input),
        })
    }

    pub fn send_key_for_view(&self, view: TerminalViewId, input: KeyInput) {
        self.send_command(Command::Key {
            view: Some(view),
            input: Box::new(input),
        });
    }

    /// Resize both the emulated terminal and the native PTY.
    pub fn resize(&self, columns: u16, rows: u16, cell_width_px: u32, cell_height_px: u32) {
        self.send_command(Command::Resize(Geometry {
            columns: columns.max(1),
            rows: rows.max(1),
            cell_width_px: cell_width_px.max(1),
            cell_height_px: cell_height_px.max(1),
        }));
    }

    /// Activate one client's independent terminal snapshot stream.
    pub fn attach_view(&self, view: TerminalViewId) {
        self.send_command(Command::AttachView(view));
    }

    /// Park one client's view state for a later reattach.
    pub fn detach_view(&self, view: TerminalViewId) {
        self.send_command(Command::DetachView(view));
    }

    /// Permanently release a client view and all of its tracked terminal state.
    pub fn release_view(&self, view: TerminalViewId) {
        self.send_command(Command::ReleaseView(view));
    }

    /// Apply a native viewport, selection, copy, or paste action.
    pub fn view_action(&self, view: TerminalViewId, action: TerminalViewAction) {
        self.send_command(Command::ViewAction { view, action });
    }

    /// Pastes daemon-prepared bytes without forcing them through UTF-8. The payload
    /// must already carry tmux separator and safe/literal transforms. With
    /// `bracketed`, markers are emitted only if the application enabled the mode.
    pub fn paste_prepared_bytes(
        &self,
        view: Option<TerminalViewId>,
        bytes: Arc<[u8]>,
        bracketed: bool,
    ) {
        self.send_command(Command::PastePreparedBytes {
            view,
            bytes,
            bracketed,
        });
    }

    pub fn send_raw_input(&self, bytes: Arc<[u8]>) -> bool {
        self.send_command(Command::RawInput(bytes))
    }

    pub fn raw_output_tap_channel() -> (Sender<Arc<[u8]>>, Receiver<Arc<[u8]>>) {
        crossbeam_channel::bounded(RAW_OUTPUT_TAP_PENDING_CHUNKS)
    }

    pub fn arm_raw_output_tap(
        &self,
        token: u64,
        output: Sender<Arc<[u8]>>,
    ) -> Result<(), RawOutputTapError> {
        let (reply, response) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        self.commands
            .send_timeout(
                Command::ArmRawOutputTap {
                    token,
                    output,
                    reply,
                },
                CAPTURE_TIMEOUT,
            )
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => RawOutputTapError::TimedOut,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                    RawOutputTapError::ActorStopped
                }
            })?;
        match response.recv_timeout(CAPTURE_TIMEOUT.saturating_sub(started.elapsed())) {
            Ok(true) => Ok(()),
            Ok(false) => Err(RawOutputTapError::Unavailable),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => Err(RawOutputTapError::TimedOut),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                Err(RawOutputTapError::ActorStopped)
            }
        }
    }

    pub fn disarm_raw_output_tap(&self, token: u64) -> Result<(), RawOutputTapError> {
        let (reply, response) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        self.commands
            .send_timeout(
                Command::DisarmRawOutputTap { token, reply },
                CAPTURE_TIMEOUT,
            )
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => RawOutputTapError::TimedOut,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                    RawOutputTapError::ActorStopped
                }
            })?;
        response
            .recv_timeout(CAPTURE_TIMEOUT.saturating_sub(started.elapsed()))
            .map_err(|error| match error {
                crossbeam_channel::RecvTimeoutError::Timeout => RawOutputTapError::TimedOut,
                crossbeam_channel::RecvTimeoutError::Disconnected => {
                    RawOutputTapError::ActorStopped
                }
            })
    }

    /// Open the observation window that binds one pasted image to the next
    /// numbered placeholder the application prints.
    pub fn open_pending_paste(&self, token: u64) {
        self.send_command(Command::PendingPasteOpened { token });
    }

    /// Make an evicted pasted-image number inert in every terminal view.
    pub fn unbind_pasted_image(&self, number: u32) {
        self.send_command(Command::UnbindPastedImage { number });
    }

    /// Captures canonical terminal content on the pane actor. Blocks only the
    /// calling command or client thread.
    pub fn capture(&self, options: CaptureOptions) -> Result<String, TerminalCaptureError> {
        let (reply, response) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        self.commands
            .send_timeout(
                Command::Capture(Box::new(CaptureRequest { options, reply })),
                CAPTURE_TIMEOUT,
            )
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => TerminalCaptureError::TimedOut,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                    TerminalCaptureError::ActorStopped
                }
            })?;
        response
            .recv_timeout(CAPTURE_TIMEOUT.saturating_sub(started.elapsed()))
            .map_err(|error| match error {
                crossbeam_channel::RecvTimeoutError::Timeout => TerminalCaptureError::TimedOut,
                crossbeam_channel::RecvTimeoutError::Disconnected => {
                    TerminalCaptureError::ActorStopped
                }
            })?
    }

    /// Answers `capture-pane` on a pane whose worker has already returned. The
    /// grid the worker froze on its way out carries the scrollback, so the rows
    /// the dead-pane notice scrolled off are still reachable; only the two
    /// flags that need a live screen fall back on the published viewport.
    pub fn capture_frozen_frame(
        &self,
        options: CaptureOptions,
    ) -> Result<String, TerminalCaptureError> {
        let frozen = self.latest.read().frozen.clone();
        if let Some(frozen) = frozen
            && !options.alternate
            && !options.mode
        {
            let revision = &frozen.revision;
            return capture_revision(revision, revision.maximum_offset(), options);
        }
        capture_viewport(&self.latest_viewport(), options)
    }

    /// Copies one absolute span of retained primary-screen history without moving
    /// any viewport. Rows come oldest-first with a self-contained dictionary,
    /// clamped to scrollback; live-grid rows are never included.
    #[allow(
        clippy::type_complexity,
        reason = "the tuple is the established public history-page response"
    )]
    pub fn history(
        &self,
        start: u32,
        count: u32,
    ) -> Result<
        (
            u32,
            Vec<Vec<PackedCell>>,
            TerminalDictionary,
            ScrollbarState,
            u16,
        ),
        TerminalCaptureError,
    > {
        let (reply, response) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        self.commands
            .send_timeout(
                Command::History(Box::new(HistoryCommand {
                    start,
                    count,
                    reply,
                })),
                CAPTURE_TIMEOUT,
            )
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => TerminalCaptureError::TimedOut,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                    TerminalCaptureError::ActorStopped
                }
            })?;
        response
            .recv_timeout(CAPTURE_TIMEOUT.saturating_sub(started.elapsed()))
            .map_err(|error| match error {
                crossbeam_channel::RecvTimeoutError::Timeout => TerminalCaptureError::TimedOut,
                crossbeam_channel::RecvTimeoutError::Disconnected => {
                    TerminalCaptureError::ActorStopped
                }
            })?
    }

    /// Extracts the last completed command and its output from the pane actor.
    /// Blocks the calling thread like [`Self::capture`].
    ///
    /// # Errors
    ///
    /// [`TerminalCaptureError::NoSemanticMarks`] when the shell emits no OSC 133
    /// marks, plus the same failures as [`Self::capture`].
    pub fn capture_last_command(&self) -> Result<LastCommandCapture, TerminalCaptureError> {
        let (reply, response) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        self.commands
            .send_timeout(
                Command::SemanticCapture(Box::new(LastCommandRequest { reply })),
                CAPTURE_TIMEOUT,
            )
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => TerminalCaptureError::TimedOut,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                    TerminalCaptureError::ActorStopped
                }
            })?;
        response
            .recv_timeout(CAPTURE_TIMEOUT.saturating_sub(started.elapsed()))
            .map_err(|error| match error {
                crossbeam_channel::RecvTimeoutError::Timeout => TerminalCaptureError::TimedOut,
                crossbeam_channel::RecvTimeoutError::Disconnected => {
                    TerminalCaptureError::ActorStopped
                }
            })?
    }

    /// Feed bytes straight into a PTY-free session's parser, as if a child
    /// had written them: the daemon's projection of an agent transcript.
    /// Sessions with a real child ignore it.
    pub fn feed(&self, bytes: Arc<[u8]>) -> bool {
        self.send_command(Command::Output(bytes))
    }

    fn send_command(&self, command: Command) -> bool {
        let started = diagnostic_timer();
        let queue_before = if started.is_some() {
            self.commands.len()
        } else {
            0
        };
        log::trace!(
            target: "zz_terminal::diagnostics::command",
            "enqueue begin queue_len={queue_before} queue_capacity={:?} command={command:#?}",
            self.commands.capacity(),
        );
        let result = self.commands.send(command);
        let success = result.is_ok();
        if let Err(crossbeam_channel::TrySendError::Full(command)) = &result {
            let (pending_commands, pending_bytes) = self.commands.pending_input();
            log::warn!(
                "rejected terminal PTY input command={} charge_bytes={} pending_commands={} pending_bytes={} limits_commands={} limits_bytes={}",
                command.name(),
                command.pty_input_bytes().unwrap_or(0),
                pending_commands,
                pending_bytes,
                MAX_PENDING_PTY_INPUT_COMMANDS,
                MAX_PENDING_PTY_INPUT_BYTES,
            );
        }
        log::trace!(
            target: "zz_terminal::diagnostics::command",
            "enqueue end success={} queue_before={} queue_after={} elapsed_us={}",
            success,
            queue_before,
            self.commands.len(),
            diagnostic_elapsed_us(started),
        );
        success
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if !self.terminating.load(Ordering::Acquire) {
            let _ = self.commands.try_send(Command::Shutdown);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Geometry {
    columns: u16,
    rows: u16,
    cell_width_px: u32,
    cell_height_px: u32,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            columns: INITIAL_COLUMNS,
            rows: INITIAL_ROWS,
            cell_width_px: INITIAL_CELL_WIDTH,
            cell_height_px: INITIAL_CELL_HEIGHT,
        }
    }
}

impl Geometry {
    fn from_size(size: TerminalSize) -> Self {
        Self {
            columns: size.columns.max(1),
            rows: size.rows.max(1),
            cell_width_px: size.cell_width_px.max(1),
            cell_height_px: size.cell_height_px.max(1),
        }
    }

    fn pty_size(self) -> PtySize {
        let width = u32::from(self.columns).saturating_mul(self.cell_width_px);
        let height = u32::from(self.rows).saturating_mul(self.cell_height_px);
        PtySize {
            rows: self.rows,
            cols: self.columns,
            pixel_width: u16::try_from(width).unwrap_or(u16::MAX),
            pixel_height: u16::try_from(height).unwrap_or(u16::MAX),
        }
    }

    fn size_report(self) -> SizeReportSize {
        SizeReportSize {
            rows: self.rows,
            columns: self.columns,
            cell_width: self.cell_width_px,
            cell_height: self.cell_height_px,
        }
    }
}

#[derive(Debug)]
struct CaptureRequest {
    options: CaptureOptions,
    reply: Sender<Result<String, TerminalCaptureError>>,
}

#[derive(Debug)]
struct LastCommandRequest {
    reply: Sender<Result<LastCommandCapture, TerminalCaptureError>>,
}

#[derive(Debug)]
struct KittyImageRequest {
    image_id: u32,
    reply: Sender<Option<KittyImage>>,
}

#[derive(Debug)]
struct KittyImageGenerationRequest {
    image_id: u32,
    reply: Sender<Option<u64>>,
}

type HistoryCapture = (
    u32,
    Vec<Vec<PackedCell>>,
    TerminalDictionary,
    ScrollbarState,
    u16,
);

#[derive(Debug)]
struct HistoryCommand {
    start: u32,
    count: u32,
    reply: Sender<Result<HistoryCapture, TerminalCaptureError>>,
}

#[derive(Debug)]
enum Command {
    Text {
        view: Option<TerminalViewId>,
        text: Arc<str>,
    },
    Key {
        view: Option<TerminalViewId>,
        input: Box<KeyInput>,
    },
    Resize(Geometry),
    SetWordSeparators(Box<WordSeparators>),
    SetAppearance(Arc<TerminalAppearance>),
    SetAllowPassthrough(AllowPassthrough),
    SetWrapSearch(bool),
    SetEngineKnobs(EngineKnobs),
    CaptureCopySource {
        reply: Sender<Result<CapturedCopySource, TerminalCaptureError>>,
    },
    SetPendingCopySource(Option<Box<CapturedCopySource>>),
    WriteDeadNotice(Option<Arc<str>>),
    ResetScreen,
    AttachView(TerminalViewId),
    DetachView(TerminalViewId),
    ReleaseView(TerminalViewId),
    ViewAction {
        view: TerminalViewId,
        action: TerminalViewAction,
    },
    PastePreparedBytes {
        view: Option<TerminalViewId>,
        bytes: Arc<[u8]>,
        bracketed: bool,
    },
    RawInput(Arc<[u8]>),
    Output(Arc<[u8]>),
    ArmRawOutputTap {
        token: u64,
        output: Sender<Arc<[u8]>>,
        reply: Sender<bool>,
    },
    DisarmRawOutputTap {
        token: u64,
        reply: Sender<()>,
    },
    Capture(Box<CaptureRequest>),
    SemanticCapture(Box<LastCommandRequest>),
    History(Box<HistoryCommand>),
    KittyImage(Box<KittyImageRequest>),
    KittyImageGeneration(Box<KittyImageGenerationRequest>),
    PendingPasteOpened {
        token: u64,
    },
    UnbindPastedImage {
        number: u32,
    },
    Terminate,
    Shutdown,
}

impl Command {
    const fn name(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Key { .. } => "key",
            Self::Resize(_) => "resize",
            Self::SetWordSeparators(_) => "set-word-separators",
            Self::SetAppearance(_) => "set-appearance",
            Self::SetAllowPassthrough(_) => "set-allow-passthrough",
            Self::SetWrapSearch(_) => "set-wrap-search",
            Self::SetEngineKnobs(_) => "set-engine-knobs",
            Self::CaptureCopySource { .. } => "capture-copy-source",
            Self::SetPendingCopySource(_) => "set-pending-copy-source",
            Self::WriteDeadNotice(_) => "write-dead-notice",
            Self::AttachView(_) => "attach-view",
            Self::DetachView(_) => "detach-view",
            Self::ReleaseView(_) => "release-view",
            Self::ViewAction { .. } => "view-action",
            Self::PastePreparedBytes { .. } => "paste-prepared-bytes",
            Self::RawInput(_) => "raw-input",
            Self::Output(_) => "output",
            Self::ArmRawOutputTap { .. } => "arm-raw-output-tap",
            Self::DisarmRawOutputTap { .. } => "disarm-raw-output-tap",
            Self::Capture(_) => "capture",
            Self::SemanticCapture(_) => "semantic-capture",
            Self::History(_) => "history",
            Self::KittyImage(_) => "kitty-image",
            Self::KittyImageGeneration(_) => "kitty-image-generation",
            Self::PendingPasteOpened { .. } => "pending-paste-opened",
            Self::UnbindPastedImage { .. } => "unbind-pasted-image",
            Self::ResetScreen => "reset-screen",
            Self::Terminate => "terminate",
            Self::Shutdown => "shutdown",
        }
    }

    fn pty_input_bytes(&self) -> Option<usize> {
        let payload = match self {
            Self::Text { text, .. } => text.len(),
            Self::Key { input, .. } => input.text.as_deref().map_or(0, str::len),
            Self::ViewAction { action, .. } => match action {
                TerminalViewAction::Mouse(_)
                | TerminalViewAction::ScrollWheel { .. }
                | TerminalViewAction::Focus(_) => 0,
                TerminalViewAction::Paste(text) => text.len(),
                _ => return None,
            },
            Self::PastePreparedBytes { bytes, .. } => bytes.len(),
            Self::PendingPasteOpened { .. } => 0,
            _ => return None,
        };
        Some(payload.saturating_add(PTY_INPUT_COMMAND_FLOOR_BYTES))
    }
}

fn command_channel() -> (Sender<Command>, Receiver<Command>) {
    crossbeam_channel::bounded(MAX_PENDING_ACTOR_COMMANDS)
}

#[derive(Default)]
struct InputAdmission {
    commands: usize,
    bytes: usize,
}

struct InputPermit {
    admission: Arc<Mutex<InputAdmission>>,
    bytes: usize,
}

impl Drop for InputPermit {
    fn drop(&mut self) {
        let mut admission = self.admission.lock();
        admission.commands = admission.commands.saturating_sub(1);
        admission.bytes = admission.bytes.saturating_sub(self.bytes);
    }
}

struct QueuedInput {
    command: Command,
    permit: InputPermit,
}

struct InputSender {
    commands: Sender<QueuedInput>,
    admission: Arc<Mutex<InputAdmission>>,
    max_commands: usize,
    max_bytes: usize,
}

impl InputSender {
    fn try_send(&self, command: Command) -> Result<(), crossbeam_channel::TrySendError<Command>> {
        let bytes = command
            .pty_input_bytes()
            .expect("only PTY input commands use the input queue");
        {
            let mut admission = self.admission.lock();
            if admission.commands >= self.max_commands
                || bytes > self.max_bytes.saturating_sub(admission.bytes)
            {
                return Err(crossbeam_channel::TrySendError::Full(command));
            }
            admission.commands += 1;
            admission.bytes += bytes;
        }
        let queued = QueuedInput {
            command,
            permit: InputPermit {
                admission: Arc::clone(&self.admission),
                bytes,
            },
        };
        self.commands.try_send(queued).map_err(|error| match error {
            crossbeam_channel::TrySendError::Full(queued) => {
                let QueuedInput { command, permit } = queued;
                drop(permit);
                crossbeam_channel::TrySendError::Full(command)
            }
            crossbeam_channel::TrySendError::Disconnected(queued) => {
                let QueuedInput { command, permit } = queued;
                drop(permit);
                crossbeam_channel::TrySendError::Disconnected(command)
            }
        })
    }

    const fn capacity(&self) -> usize {
        self.max_commands
    }

    fn pending(&self) -> (usize, usize) {
        let admission = self.admission.lock();
        (admission.commands, admission.bytes)
    }
}

fn input_channel() -> (InputSender, Receiver<QueuedInput>) {
    input_channel_with_limits(MAX_PENDING_PTY_INPUT_COMMANDS, MAX_PENDING_PTY_INPUT_BYTES)
}

fn input_channel_with_limits(
    max_commands: usize,
    max_bytes: usize,
) -> (InputSender, Receiver<QueuedInput>) {
    let (commands, receiver) = crossbeam_channel::bounded(max_commands);
    let admission = Arc::new(Mutex::new(InputAdmission::default()));
    (
        InputSender {
            commands,
            admission,
            max_commands,
            max_bytes,
        },
        receiver,
    )
}

#[derive(Clone)]
struct ActorWake {
    #[cfg(unix)]
    pipe: Option<Arc<std::os::fd::OwnedFd>>,
}

impl ActorWake {
    const fn none() -> Self {
        Self {
            #[cfg(unix)]
            pipe: None,
        }
    }

    fn notify(&self) {
        #[cfg(unix)]
        if let Some(pipe) = &self.pipe
            && let Err(error) = write_actor_wake(|| rustix::io::write(&**pipe, &[1_u8]))
        {
            log::error!("failed to wake terminal actor: {error}");
        }
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
type WakeReceiver = Result<std::os::fd::OwnedFd, rustix::io::Errno>;
#[cfg(any(target_os = "linux", not(unix)))]
type WakeReceiver = ();

#[cfg(unix)]
fn write_actor_wake(
    mut write: impl FnMut() -> Result<usize, rustix::io::Errno>,
) -> Result<(), rustix::io::Errno> {
    loop {
        match write() {
            Ok(_) | Err(rustix::io::Errno::AGAIN) => return Ok(()),
            Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn configured_actor_wake_pipe()
-> Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd), rustix::io::Errno> {
    let (read, write) = rustix::pipe::pipe()?;
    rustix::io::fcntl_setfd(&read, rustix::io::FdFlags::CLOEXEC)?;
    rustix::io::fcntl_setfd(&write, rustix::io::FdFlags::CLOEXEC)?;
    rustix::io::ioctl_fionbio(&read, true)?;
    rustix::io::ioctl_fionbio(&write, true)?;
    Ok((read, write))
}

fn actor_wake() -> (ActorWake, WakeReceiver) {
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        match configured_actor_wake_pipe() {
            Ok((read, write)) => (
                ActorWake {
                    pipe: Some(Arc::new(write)),
                },
                Ok(read),
            ),
            Err(error) => (ActorWake::none(), Err(error)),
        }
    }
    #[cfg(any(target_os = "linux", not(unix)))]
    (ActorWake::none(), ())
}

struct CommandQueues {
    control: Sender<Command>,
    input: Option<InputSender>,
}

struct CommandSender {
    queues: Box<CommandQueues>,
    wake: ActorWake,
}

impl CommandSender {
    fn send(&self, command: Command) -> Result<(), crossbeam_channel::TrySendError<Command>> {
        let result = if command.pty_input_bytes().is_some()
            && let Some(input) = &self.queues.input
        {
            input.try_send(command)
        } else {
            self.queues
                .control
                .send(command)
                .map_err(|error| crossbeam_channel::TrySendError::Disconnected(error.0))
        };
        if result.is_ok() {
            self.wake.notify();
        }
        result
    }

    fn send_timeout(
        &self,
        command: Command,
        timeout: Duration,
    ) -> Result<(), crossbeam_channel::SendTimeoutError<Command>> {
        let result = if command.pty_input_bytes().is_some()
            && let Some(input) = &self.queues.input
        {
            input.try_send(command).map_err(|error| match error {
                crossbeam_channel::TrySendError::Full(command) => {
                    crossbeam_channel::SendTimeoutError::Timeout(command)
                }
                crossbeam_channel::TrySendError::Disconnected(command) => {
                    crossbeam_channel::SendTimeoutError::Disconnected(command)
                }
            })
        } else {
            self.queues.control.send_timeout(command, timeout)
        };
        if result.is_ok() {
            self.wake.notify();
        }
        result
    }

    fn try_send(&self, command: Command) -> Result<(), crossbeam_channel::TrySendError<Command>> {
        let result = if command.pty_input_bytes().is_some()
            && let Some(input) = &self.queues.input
        {
            input.try_send(command)
        } else {
            self.queues.control.try_send(command)
        };
        if result.is_ok() {
            self.wake.notify();
        }
        result
    }

    fn len(&self) -> usize {
        self.queues
            .control
            .len()
            .saturating_add(self.pending_input().0)
    }

    fn capacity(&self) -> Option<usize> {
        let control = self.queues.control.capacity()?;
        let input = self.queues.input.as_ref().map_or(0, InputSender::capacity);
        Some(control.saturating_add(input))
    }

    fn pending_input(&self) -> (usize, usize) {
        self.queues
            .input
            .as_ref()
            .map_or((0, 0), InputSender::pending)
    }
}

/// Where `recentre-top-bottom` parks the cursor line on its next press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecentreTarget {
    Middle,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionMode {
    Cell,
    Word,
    Line,
}

#[derive(Debug)]
struct SelectionState {
    anchor: TrackedGridRef,
    focus: TrackedGridRef,
    mode: SelectionMode,
    rectangle: bool,
}

#[derive(Debug)]
struct CopyModeState {
    revision: Arc<ModeRevision>,
    cursor: PointCoordinate,
    viewport_offset: u32,
    scroll_exit: bool,
    hide_position: bool,
    selection: Option<ModeSelection>,
    selection_mode: CopySelectionMode,
    selection_origin: PointCoordinate,
    recentre: Option<(u32, RecentreTarget)>,
    mark: Option<PointCoordinate>,
    last_jump: Option<CopyJump>,
    selecting: bool,
    rectangle: bool,
    /// `data->refresh_active`: the timer re-clones the backing from the live
    /// pane while this is set.
    refresh: bool,
    /// `wme->swp != wme->wp`: the backing came from another pane, so
    /// `window_copy_refresh_start` refuses to re-sync it.
    sourced: bool,
    kind: FrozenModeKind,
    /// `data->lastcx` and `data->lastsx`: the desired column a row change keeps
    /// and the length of the row it was taken from.
    last_cx: u16,
    last_sx: u16,
    /// `data->modekeys`: the option's value when the mode was entered. Three
    /// branches in window-copy.c read this latch rather than the live option.
    mode_keys_vi_at_entry: bool,
    /// `data->searchmark != NULL`: whether a copy-mode search laid marks down.
    /// The marks themselves are the view's search state, so this only says
    /// whether the mode owns them.
    search_marks: bool,
    /// `data->searchcount` and `data->searchmore`. The mode is entered with a
    /// zeroed count, which is why the pin publishes 0 before any search, and
    /// `window_copy_clear_marks` puts -1 back, which publishes neither name.
    search_count: Option<(u32, bool)>,
    /// `data->searchx`, `data->searchy` and `data->searcho`: where the mode
    /// stood when the incremental prompt opened, which every changed string
    /// goes back to.
    incremental_origin: Option<CopyModeIncrementalOrigin>,
    /// `data->searchstr`, `data->searchtype` and `data->searchregex`: the
    /// search the mode last ran, which `search-again` and `search-reverse`
    /// re-run and which the incremental spellings compare against.
    search: Option<CopyModeSearch>,
}

/// `data->searchx`, `data->searchy` and `data->searcho`.
#[derive(Clone, Copy, Debug)]
struct CopyModeIncrementalOrigin {
    row: u32,
    viewport_offset: u32,
}

/// `window_copy_clone_screen` with `trim`: one pane's screen, its trailing
/// blank rows dropped and its cursor pulled onto the last used row, ready to
/// back another pane's copy mode.
#[derive(Debug)]
pub struct CapturedCopySource {
    revision: Arc<ModeRevision>,
    cursor: PointCoordinate,
    viewport_offset: u32,
}

type CopyModeSlot = Option<Box<CopyModeState>>;
type SearchSlot = Option<Box<SearchState>>;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrozenModeKind {
    Copy,
    View,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HoverLink {
    row: u16,
    start: u16,
    end: u16,
    uri: String,
}

impl HoverLink {
    fn contains(&self, point: PointerCellEvent) -> bool {
        self.row == point.row && (self.start..self.end).contains(&point.column)
    }
}

#[derive(Debug)]
struct PendingPasteWindow {
    token: u64,
    deadline: Instant,
    baseline: HashMap<u32, usize>,
}

#[derive(Default)]
struct PastedImageBindings {
    pending: VecDeque<PendingPasteWindow>,
    bound_numbers: HashSet<u32>,
}

impl PastedImageBindings {
    fn open(
        &mut self,
        terminal: &Terminal<'_, '_>,
        token: u64,
        now: Instant,
    ) -> Result<(), WorkerError> {
        self.pending.push_back(PendingPasteWindow {
            token,
            deadline: now + PENDING_PASTE_WINDOW,
            baseline: image_placeholder_occurrences(terminal)?,
        });
        Ok(())
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pending.front().map(|pending| pending.deadline)
    }

    fn expire(&mut self, now: Instant) -> Vec<u64> {
        let mut expired = Vec::new();
        while self
            .pending
            .front()
            .is_some_and(|pending| pending.deadline <= now)
        {
            if let Some(pending) = self.pending.pop_front() {
                expired.push(pending.token);
            }
        }
        expired
    }

    fn observe(&mut self, terminal: &Terminal<'_, '_>) -> Result<Option<(u64, u32)>, WorkerError> {
        let Some(oldest) = self.pending.front() else {
            return Ok(None);
        };
        let current = image_placeholder_occurrences(terminal)?;
        let number = current
            .iter()
            .filter_map(|(number, count)| {
                (*count > oldest.baseline.get(number).copied().unwrap_or(0)).then_some(*number)
            })
            .max();
        let Some(number) = number else {
            return Ok(None);
        };
        let token = self
            .pending
            .pop_front()
            .expect("the observed pending paste remains at the front")
            .token;
        for pending in &mut self.pending {
            pending.baseline.clone_from(&current);
        }
        self.bound_numbers.insert(number);
        Ok(Some((token, number)))
    }

    fn unbind(&mut self, number: u32) -> bool {
        self.bound_numbers.remove(&number)
    }

    fn bound_numbers(&self) -> &HashSet<u32> {
        &self.bound_numbers
    }
}

#[derive(Default)]
enum ViewportAnchor {
    #[default]
    FollowBottom,
    Pinned(TrackedGridRef),
}

#[derive(Default)]
struct TerminalScreenViewState {
    viewport: ViewportAnchor,
    selection: Option<SelectionState>,
    copy_mode: CopyModeSlot,
    search: SearchSlot,
    search_origin: Option<PointCoordinate>,
    search_snapshot: Option<Arc<HistorySearchSnapshot>>,
    hover_link: Option<HoverLink>,
    unseen_output: u32,
    mouse_button_pressed: bool,
}

struct TerminalViewState {
    screen: Screen,
    primary: TerminalScreenViewState,
    alternate: TerminalScreenViewState,
}

type ActiveTerminalViews = HashMap<TerminalViewId, Box<TerminalViewState>>;
type InactiveTerminalViews = HashMap<TerminalViewId, Box<TerminalViewState>>;
type InternHashMap<K, V> = HashMap<K, V, foldhash::fast::RandomState>;

impl Default for TerminalViewState {
    fn default() -> Self {
        Self::for_screen(Screen::Primary)
    }
}

impl std::ops::Deref for TerminalViewState {
    type Target = TerminalScreenViewState;

    fn deref(&self) -> &Self::Target {
        self.active()
    }
}

impl std::ops::DerefMut for TerminalViewState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.active_mut()
    }
}

impl TerminalViewState {
    fn for_screen(screen: Screen) -> Self {
        Self {
            screen,
            primary: TerminalScreenViewState::default(),
            alternate: TerminalScreenViewState::default(),
        }
    }

    fn active(&self) -> &TerminalScreenViewState {
        match self.screen {
            Screen::Primary => &self.primary,
            Screen::Alternate => &self.alternate,
        }
    }

    fn active_mut(&mut self) -> &mut TerminalScreenViewState {
        match self.screen {
            Screen::Primary => &mut self.primary,
            Screen::Alternate => &mut self.alternate,
        }
    }

    fn switch_screen(&mut self, screen: Screen) -> bool {
        if self.screen == screen {
            return false;
        }
        self.screen = screen;
        true
    }

    fn invalidate_layout(&mut self) {
        invalidate_screen_layout(&mut self.primary);
        invalidate_screen_layout(&mut self.alternate);
    }

    fn screen_mut(&mut self, screen: Screen) -> &mut TerminalScreenViewState {
        match screen {
            Screen::Primary => &mut self.primary,
            Screen::Alternate => &mut self.alternate,
        }
    }

    fn note_output(&mut self, screen: Screen) {
        if self.active().copy_mode.is_some() {
            let active = self.active_mut();
            active.unseen_output = active.unseen_output.saturating_add(1);
            active.hover_link = None;
            if screen != self.screen {
                invalidate_screen_layout(self.screen_mut(screen));
            }
            return;
        }
        self.invalidate_layout();
        let view = self.screen_mut(screen);
        if matches!(view.viewport, ViewportAnchor::Pinned(_)) {
            view.unseen_output = view.unseen_output.saturating_add(1);
        }
    }
}

fn clear_pasted_image_hover(view: &mut TerminalViewState, number: u32) -> bool {
    let uri = format!("{IMAGE_PLACEHOLDER_SCHEME}://{number}");
    let active = view.screen;
    let mut active_changed = false;
    for (screen, state) in [
        (Screen::Primary, &mut view.primary),
        (Screen::Alternate, &mut view.alternate),
    ] {
        if state
            .hover_link
            .as_ref()
            .is_some_and(|link| link.uri == uri)
        {
            state.hover_link = None;
            active_changed |= screen == active;
        }
    }
    active_changed
}

fn invalidate_screen_layout(view: &mut TerminalScreenViewState) {
    if view.copy_mode.is_some() {
        view.hover_link = None;
        return;
    }
    view.search_snapshot = None;
    view.hover_link = None;
    if let Some(search) = view.search.as_mut() {
        search.matches.clear();
        search.current = None;
        search.pending =
            !search.query.text.is_empty() && search.query.text.len() <= MAX_SEARCH_QUERY_BYTES;
        if search.pending {
            search.invalid_pattern = false;
        }
    }
}

#[derive(Debug, Default)]
struct ViewportDictionary {
    generation: u32,
    last_style_id: u16,
    mode_revision: Option<u64>,
    mode_viewport: Option<(u64, u32)>,
    default_style: Option<PackedStyle>,
    palette: Option<Box<[RgbColor; 256]>>,
    styles: Vec<PackedStyle>,
    style_ids: InternHashMap<PackedStyle, u16>,
    grapheme_ids: InternHashMap<String, u32>,
    grapheme_offsets: Vec<u32>,
    grapheme_bytes: Vec<u8>,
    shared_dictionary: Arc<TerminalDictionary>,
    shared_cells: Arc<[PackedCell]>,
    cell_pool: SmallVec<[Arc<[PackedCell]>; RETAINED_CELL_PLANES]>,
    shared_presentation: Arc<TerminalPresentation>,
    shared_overlays: Arc<[OverlaySpan]>,
    overlay_pool: SmallVec<[Arc<[OverlaySpan]>; RETAINED_OVERLAY_PLANES]>,
    grapheme_scratch: String,
    overlay_scratch: Vec<OverlaySpan>,
    shared_dirty: u8,
    style_compaction_limit: usize,
    grapheme_compaction_limit: usize,
    grapheme_byte_compaction_limit: usize,
    style_overflowed: bool,
    grapheme_overflowed: bool,
}

const SHARED_STYLES_DIRTY: u8 = 1 << 0;
const SHARED_GRAPHEMES_DIRTY: u8 = 1 << 1;
const SHARED_ALL_DIRTY: u8 = SHARED_STYLES_DIRTY | SHARED_GRAPHEMES_DIRTY;

fn adaptive_dictionary_limit(
    minimum: usize,
    viewport_headroom: usize,
    working_set: usize,
    maximum: usize,
) -> usize {
    minimum
        .max(viewport_headroom)
        .max(working_set.saturating_mul(2))
        .min(maximum)
}

impl ViewportDictionary {
    fn reset_live(&mut self, style: PackedStyle, palette: &[RgbColor; 256]) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.mode_revision = None;
        self.mode_viewport = None;
        self.default_style = Some(style);
        self.palette = Some(Box::new(*palette));
        self.last_style_id = 0;
        self.styles.clear();
        self.styles.push(style);
        self.style_ids.clear();
        self.style_ids.insert(style, 0);
        self.grapheme_ids.clear();
        self.grapheme_offsets.clear();
        self.grapheme_offsets.push(0);
        self.grapheme_bytes.clear();
        self.shared_dirty = SHARED_ALL_DIRTY;
        self.style_compaction_limit = 0;
        self.grapheme_compaction_limit = 0;
        self.grapheme_byte_compaction_limit = 0;
        self.style_overflowed = false;
        self.grapheme_overflowed = false;
    }

    fn should_compact_live(&self) -> bool {
        self.style_overflowed
            || self.grapheme_overflowed
            || (self.style_compaction_limit != 0 && self.styles.len() > self.style_compaction_limit)
            || (self.grapheme_compaction_limit != 0
                && self.grapheme_offsets.len().saturating_sub(1) > self.grapheme_compaction_limit)
            || (self.grapheme_byte_compaction_limit != 0
                && self.grapheme_bytes.len() > self.grapheme_byte_compaction_limit)
    }

    fn tune_live_compaction_limits(&mut self, cell_count: usize) {
        self.style_compaction_limit = adaptive_dictionary_limit(
            MIN_STYLE_COMPACTION_LIMIT,
            cell_count.saturating_mul(2),
            self.styles.len(),
            MAX_VIEWPORT_STYLE_COUNT,
        );
        self.grapheme_compaction_limit = adaptive_dictionary_limit(
            MIN_GRAPHEME_COMPACTION_LIMIT,
            cell_count.saturating_mul(2),
            self.grapheme_offsets.len().saturating_sub(1),
            MAX_VIEWPORT_GRAPHEME_COUNT,
        );
        self.grapheme_byte_compaction_limit = adaptive_dictionary_limit(
            MIN_GRAPHEME_BYTE_COMPACTION_LIMIT,
            cell_count.saturating_mul(64),
            self.grapheme_bytes.len(),
            MAX_VIEWPORT_GRAPHEME_BYTES,
        );
    }

    fn acquire_cell_plane(&mut self, len: usize, preserve: bool) -> Arc<[PackedCell]> {
        let reusable = self
            .cell_pool
            .iter_mut()
            .position(|plane| plane.len() == len && Arc::get_mut(plane).is_some());
        let mut plane = reusable.map_or_else(
            || Arc::from(vec![PackedCell::EMPTY; len]),
            |index| self.cell_pool.swap_remove(index),
        );
        let cells = Arc::get_mut(&mut plane).expect("new or uniquely retained cell plane");
        if preserve {
            cells.copy_from_slice(&self.shared_cells);
        } else {
            cells.fill(PackedCell::EMPTY);
        }
        plane
    }

    fn retain_cell_plane(&mut self, plane: Arc<[PackedCell]>) {
        if plane.is_empty() {
            return;
        }
        if self.cell_pool.len() == RETAINED_CELL_PLANES {
            let discard = self
                .cell_pool
                .iter()
                .position(|plane| Arc::strong_count(plane) > 1)
                .unwrap_or(0);
            self.cell_pool.swap_remove(discard);
        }
        self.cell_pool.push(plane);
    }

    fn commit_cell_plane(&mut self, plane: Arc<[PackedCell]>) {
        let previous = std::mem::replace(&mut self.shared_cells, plane);
        self.retain_cell_plane(previous);
    }

    fn acquire_overlay_plane(&mut self, overlays: &[OverlaySpan]) -> Arc<[OverlaySpan]> {
        let reusable = self
            .overlay_pool
            .iter_mut()
            .position(|plane| plane.len() == overlays.len() && Arc::get_mut(plane).is_some());
        reusable.map_or_else(
            || Arc::from(overlays),
            |index| {
                let mut plane = self.overlay_pool.swap_remove(index);
                Arc::get_mut(&mut plane)
                    .expect("uniquely retained overlay plane")
                    .copy_from_slice(overlays);
                plane
            },
        )
    }

    fn retain_overlay_plane(&mut self, plane: Arc<[OverlaySpan]>) {
        if plane.is_empty() {
            return;
        }
        if self.overlay_pool.len() == RETAINED_OVERLAY_PLANES {
            let discard = self
                .overlay_pool
                .iter()
                .position(|plane| Arc::strong_count(plane) > 1)
                .unwrap_or(0);
            self.overlay_pool.swap_remove(discard);
        }
        self.overlay_pool.push(plane);
    }

    fn finish_overlays(&mut self, mut overlays: Vec<OverlaySpan>) -> Arc<[OverlaySpan]> {
        if self.shared_overlays.as_ref() != overlays.as_slice() {
            let next = self.acquire_overlay_plane(&overlays);
            let previous = std::mem::replace(&mut self.shared_overlays, next);
            self.retain_overlay_plane(previous);
        }
        overlays.clear();
        self.overlay_scratch = overlays;
        Arc::clone(&self.shared_overlays)
    }

    fn shared_presentation(
        &mut self,
        title: &str,
        working_directory: Option<&str>,
        hovered_uri: Option<&str>,
    ) -> Arc<TerminalPresentation> {
        if self.shared_presentation.title.as_ref() != title
            || self.shared_presentation.working_directory.as_deref() != working_directory
            || self.shared_presentation.hovered_uri.as_deref() != hovered_uri
        {
            let title = if self.shared_presentation.title.as_ref() == title {
                Arc::clone(&self.shared_presentation.title)
            } else {
                Arc::from(title)
            };
            let working_directory = match (
                self.shared_presentation.working_directory.as_ref(),
                working_directory,
            ) {
                (Some(previous), Some(next)) if previous.as_ref() == next => {
                    Some(Arc::clone(previous))
                }
                (_, Some(next)) => Some(Arc::from(next)),
                (_, None) => None,
            };
            let hovered_uri = match (self.shared_presentation.hovered_uri.as_ref(), hovered_uri) {
                (Some(previous), Some(next)) if previous.as_ref() == next => {
                    Some(Arc::clone(previous))
                }
                (_, Some(next)) => Some(Arc::from(next)),
                (_, None) => None,
            };
            self.shared_presentation = Arc::new(TerminalPresentation::new(
                title,
                working_directory,
                hovered_uri,
            ));
        }
        Arc::clone(&self.shared_presentation)
    }

    fn ensure_default(&mut self, style: PackedStyle, palette: &[RgbColor; 256]) {
        if self.mode_revision.is_none()
            && self.default_style == Some(style)
            && self.palette.as_deref() == Some(palette)
        {
            return;
        }
        self.reset_live(style, palette);
    }

    fn mode_cells(&mut self, revision: &ModeRevision, offset: u32) -> Arc<[PackedCell]> {
        if self.mode_revision != Some(revision.id) {
            self.generation = self.generation.wrapping_add(1).max(1);
            self.mode_revision = Some(revision.id);
            self.mode_viewport = None;
            self.default_style = None;
            self.palette = None;
            self.last_style_id = 0;
            self.styles.clear();
            self.style_ids.clear();
            self.grapheme_ids.clear();
            self.grapheme_offsets.clear();
            self.grapheme_bytes.clear();
            self.shared_dirty = 0;
            self.style_compaction_limit = 0;
            self.grapheme_compaction_limit = 0;
            self.grapheme_byte_compaction_limit = 0;
            self.style_overflowed = false;
            self.grapheme_overflowed = false;
        }
        if self.mode_viewport != Some((revision.id, offset)) {
            let columns = usize::from(revision.columns);
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .saturating_mul(columns);
            let len = usize::from(revision.viewport_rows).saturating_mul(columns);
            let source = revision.cells.get(start..).unwrap_or_default();
            self.shared_cells = if source.len() >= len {
                Arc::from(&source[..len])
            } else {
                (0..len)
                    .map(|index| source.get(index).copied().unwrap_or(PackedCell::EMPTY))
                    .collect()
            };
            self.mode_viewport = Some((revision.id, offset));
        }
        Arc::clone(&self.shared_cells)
    }

    fn intern_style(&mut self, style: PackedStyle) -> u16 {
        if self.styles.get(usize::from(self.last_style_id)) == Some(&style) {
            return self.last_style_id;
        }
        if let Some(id) = self.style_ids.get(&style) {
            self.last_style_id = *id;
            return *id;
        }
        if self.styles.len() >= MAX_VIEWPORT_STYLE_COUNT {
            if !self.style_overflowed {
                log::warn!(
                    "terminal viewport style working set exceeds {MAX_VIEWPORT_STYLE_COUNT} entries; using the default style for excess cells"
                );
            }
            self.style_overflowed = true;
            return 0;
        }
        let id = u16::try_from(self.styles.len()).expect("style table is bounded to u16 IDs");
        self.styles.push(style);
        self.style_ids.insert(style, id);
        self.last_style_id = id;
        self.shared_dirty |= SHARED_STYLES_DIRTY;
        id
    }

    fn encode_glyph(&mut self, text: &str) -> u32 {
        let mut characters = text.chars();
        let Some(first) = characters.next() else {
            return 0;
        };
        if characters.next().is_none() {
            return u32::from(first);
        }
        if let Some(index) = self.grapheme_ids.get(text) {
            return GRAPHEME_TABLE_BIT | *index;
        }

        let index = self.grapheme_offsets.len().saturating_sub(1);
        let end = self.grapheme_bytes.len().checked_add(text.len());
        if index >= MAX_VIEWPORT_GRAPHEME_COUNT
            || end.is_none_or(|end| end > MAX_VIEWPORT_GRAPHEME_BYTES)
        {
            if !self.grapheme_overflowed {
                log::warn!(
                    "terminal viewport grapheme working set exceeds its protocol budget; using the first scalar for excess cells"
                );
            }
            self.grapheme_overflowed = true;
            return u32::from(first);
        }
        let index = u32::try_from(index).expect("grapheme table is bounded to u32 IDs");
        let end = u32::try_from(end.expect("checked above"))
            .expect("grapheme byte arena is bounded to u32 offsets");
        self.grapheme_bytes.extend_from_slice(text.as_bytes());
        self.grapheme_offsets.push(end);
        self.grapheme_ids.insert(text.to_owned(), index);
        self.shared_dirty |= SHARED_GRAPHEMES_DIRTY;
        GRAPHEME_TABLE_BIT | index
    }

    fn shared_dictionary(&mut self) -> Arc<TerminalDictionary> {
        if self.shared_dirty != 0 {
            let styles = if self.shared_dirty & SHARED_STYLES_DIRTY != 0 {
                Arc::from(self.styles.as_slice())
            } else {
                Arc::clone(&self.shared_dictionary.styles)
            };
            let (grapheme_offsets, grapheme_bytes) =
                if self.shared_dirty & SHARED_GRAPHEMES_DIRTY != 0 {
                    (
                        Arc::from(self.grapheme_offsets.as_slice()),
                        Arc::from(self.grapheme_bytes.as_slice()),
                    )
                } else {
                    (
                        Arc::clone(&self.shared_dictionary.grapheme_offsets),
                        Arc::clone(&self.shared_dictionary.grapheme_bytes),
                    )
                };
            self.shared_dictionary = Arc::new(TerminalDictionary::from_shared(
                styles,
                grapheme_offsets,
                grapheme_bytes,
            ));
        }
        self.shared_dirty = 0;
        Arc::clone(&self.shared_dictionary)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SearchMatch {
    row: u32,
    start: u16,
    end: u16,
}

#[derive(Debug, Default)]
struct SearchState {
    query: SearchQuery,
    matches: Vec<SearchMatch>,
    current: Option<usize>,
    request_id: u64,
    pending: bool,
    invalid_pattern: bool,
}

fn store_search_state(slot: &mut SearchSlot, state: SearchState) {
    if let Some(current) = slot.as_mut() {
        **current = state;
    } else {
        *slot = Some(Box::new(state));
    }
}

#[derive(Default)]
struct ViewportGenerations {
    content: u64,
    view: u64,
    kitty: Option<KittyGraphicsState>,
}

impl ViewportGenerations {
    fn new() -> Result<Self, WorkerError> {
        Ok(Self {
            kitty: Some(KittyGraphicsState::new()?),
            ..Self::default()
        })
    }
}

struct KittyGraphicsState {
    iterator: PlacementIterator<'static>,
    last_generation: u64,
    last_extraction_empty: bool,
    oversized_images: HashSet<(u32, u64)>,
}

impl KittyGraphicsState {
    fn new() -> Result<Self, WorkerError> {
        Ok(Self {
            iterator: PlacementIterator::new()?,
            last_generation: 0,
            last_extraction_empty: true,
            oversized_images: HashSet::new(),
        })
    }

    fn placements(
        &mut self,
        terminal: &Terminal<'_, '_>,
        scrollbar: ScrollbarState,
    ) -> Result<Arc<[KittyPlacement]>, WorkerError> {
        let graphics = terminal.kitty_graphics()?;
        let generation = graphics.generation()?;
        if generation == self.last_generation && self.last_extraction_empty {
            return Ok(Arc::from([]));
        }

        let mut iterator = self.iterator.update(&graphics)?;
        let mut saw_stored_placement = false;
        let mut placements = Vec::new();
        while let Some(placement) = iterator.next() {
            let image_id = placement.image_id()?;
            if placement.is_virtual()? || image_id == 0 {
                continue;
            }
            saw_stored_placement = true;
            let Some(image) = graphics.image(image_id) else {
                continue;
            };
            let image_generation = image.generation()?;
            let image_width = image.width()?;
            let image_height = image.height()?;
            let z = placement.z()?;
            let info = placement.placement_render_info(&image, terminal)?;
            if !info.viewport_visible {
                continue;
            }
            if info.grid_cols == 0
                || info.grid_rows == 0
                || info.pixel_width == 0
                || info.pixel_height == 0
                || image_generation == 0
            {
                continue;
            }
            let source_rect = (
                info.source_x,
                info.source_y,
                info.source_width,
                info.source_height,
            );
            let source_rect =
                (source_rect != (0, 0, image_width, image_height)).then_some(source_rect);
            let absolute_row = if info.viewport_row < 0 {
                u64::from(scrollbar.offset)
                    .saturating_sub(u64::from(info.viewport_row.unsigned_abs()))
            } else {
                u64::from(scrollbar.offset)
                    .saturating_add(u64::try_from(info.viewport_row).unwrap_or_default())
            };
            placements.push(KittyPlacement {
                image_id,
                image_generation,
                layer: if z < i32::MIN / 2 {
                    KittyLayer::BelowBg
                } else if z < 0 {
                    KittyLayer::BelowText
                } else {
                    KittyLayer::AboveText
                },
                viewport_col: info.viewport_col,
                viewport_row: info.viewport_row,
                absolute_row,
                cell_offset_x: placement.x_offset()?,
                cell_offset_y: placement.y_offset()?,
                grid_cols: info.grid_cols,
                grid_rows: info.grid_rows,
                pixel_width: info.pixel_width,
                pixel_height: info.pixel_height,
                source_rect,
            });
            if placements.len() == MAX_KITTY_PLACEMENTS {
                break;
            }
        }
        self.last_generation = generation;
        self.last_extraction_empty = !saw_stored_placement;
        Ok(placements.into())
    }

    fn image(
        &mut self,
        terminal: &Terminal<'_, '_>,
        image_id: u32,
    ) -> Result<Option<KittyImage>, WorkerError> {
        let graphics = terminal.kitty_graphics()?;
        let Some(image) = graphics.image(image_id) else {
            return Ok(None);
        };
        let width = image.width()?;
        let height = image.height()?;
        let generation = image.generation()?;
        let format = image.format()?;
        let input = image.data()?;
        let Some(output_len) = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return Ok(None);
        };
        if output_len == 0 || output_len > MAX_KITTY_IMAGE_BYTES {
            if self.oversized_images.insert((image_id, generation)) {
                log::warn!(
                    "refusing Kitty image {image_id} generation {generation}: decoded BGRA size {output_len} exceeds {MAX_KITTY_IMAGE_BYTES} bytes",
                );
            }
            return Ok(None);
        }
        let Some(bytes_per_pixel) = kitty_bytes_per_pixel(format) else {
            return Ok(None);
        };
        let expected_input = output_len
            .checked_div(4)
            .and_then(|pixels| pixels.checked_mul(bytes_per_pixel));
        if expected_input != Some(input.len()) {
            log::warn!(
                "refusing malformed Kitty image {image_id} generation {generation}: format={format:?} bytes={} expected={expected_input:?}",
                input.len(),
            );
            return Ok(None);
        }
        let bgra = kitty_to_premultiplied_bgra(input, format, output_len);
        Ok(Some(KittyImage {
            image_id,
            generation,
            width,
            height,
            bgra,
        }))
    }

    fn image_generation(
        terminal: &Terminal<'_, '_>,
        image_id: u32,
    ) -> Result<Option<u64>, WorkerError> {
        let graphics = terminal.kitty_graphics()?;
        let Some(image) = graphics.image(image_id) else {
            return Ok(None);
        };
        let generation = image.generation()?;
        Ok((generation != 0).then_some(generation))
    }
}

/// The VT's default Kitty storage quota is 10 MB, under the 16 MiB wire cap.
/// Aligned so anything the VT stores is shippable.
fn configure_kitty_storage(terminal: &mut Terminal<'_, '_>) -> Result<(), WorkerError> {
    terminal.set_kitty_image_storage_limit(MAX_KITTY_IMAGE_BYTES as u64)?;
    Ok(())
}

fn kitty_bytes_per_pixel(format: ImageFormat) -> Option<usize> {
    match format {
        ImageFormat::Gray => Some(1),
        ImageFormat::GrayAlpha => Some(2),
        ImageFormat::Rgb => Some(3),
        ImageFormat::Rgba => Some(4),
        _ => None,
    }
}

fn premultiply(component: u8, alpha: u8) -> u8 {
    u8::try_from((u16::from(component) * u16::from(alpha) + 127) / 255).unwrap_or(u8::MAX)
}

fn kitty_to_premultiplied_bgra(input: &[u8], format: ImageFormat, output_len: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(output_len);
    match format {
        ImageFormat::Gray => {
            for gray in input {
                output.extend_from_slice(&[*gray, *gray, *gray, u8::MAX]);
            }
        }
        ImageFormat::GrayAlpha => {
            for pixel in input.chunks_exact(2) {
                let gray = premultiply(pixel[0], pixel[1]);
                output.extend_from_slice(&[gray, gray, gray, pixel[1]]);
            }
        }
        ImageFormat::Rgb => {
            for pixel in input.chunks_exact(3) {
                output.extend_from_slice(&[pixel[2], pixel[1], pixel[0], u8::MAX]);
            }
        }
        ImageFormat::Rgba => {
            for pixel in input.chunks_exact(4) {
                output.extend_from_slice(&[
                    premultiply(pixel[2], pixel[3]),
                    premultiply(pixel[1], pixel[3]),
                    premultiply(pixel[0], pixel[3]),
                    pixel[3],
                ]);
            }
        }
        _ => {}
    }
    debug_assert_eq!(output.len(), output_len);
    output
}

struct KittyPngDecoder;

impl graphics::DecodePng for KittyPngDecoder {
    fn decode_png<'alloc>(
        &mut self,
        allocator: &'alloc Allocator<'_>,
        encoded: &[u8],
    ) -> Option<DecodedImage<'alloc>> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_kitty_png(allocator, encoded)
        }))
        .ok()
        .flatten()
    }
}

fn install_kitty_png_decoder() {
    if let Err(error) = graphics::set_png_decoder(Some(Box::new(KittyPngDecoder))) {
        log::warn!("could not install the Kitty PNG decoder on the terminal worker: {error}");
    }
}

fn decode_kitty_png<'alloc>(
    allocator: &'alloc Allocator<'_>,
    encoded: &[u8],
) -> Option<DecodedImage<'alloc>> {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_KITTY_IMAGE_BYTES as u64);
    let max_dimension = u32::try_from(MAX_KITTY_IMAGE_BYTES / 4).ok()?;
    limits.max_image_width = Some(max_dimension);
    limits.max_image_height = Some(max_dimension);
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(encoded), image::ImageFormat::Png);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let (width, height) = (decoded.width(), decoded.height());
    let expected = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if expected == 0 || expected > MAX_KITTY_IMAGE_BYTES {
        return None;
    }
    let decoded = decoded.into_rgba8().into_raw();
    if decoded.len() != expected {
        return None;
    }
    let mut data = Bytes::new_with_alloc(allocator, decoded.len()).ok()?;
    data.copy_from_slice(&decoded);
    Some(DecodedImage {
        width,
        height,
        data,
    })
}

#[derive(Clone, Copy, Debug)]
enum SnapshotChange {
    Content,
    View,
    Overlay,
}

#[derive(Debug)]
#[cfg(any(target_os = "linux", not(unix), test))]
enum ReaderMessage {
    Data { buffer: Vec<u8>, length: usize },
    Eof,
}

#[derive(Debug)]
struct PtyEffects {
    bytes: Vec<u8>,
    overflowed: bool,
}

impl PtyEffects {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(256),
            overflowed: false,
        }
    }

    fn push(&mut self, response: &[u8]) {
        if self.overflowed {
            return;
        }
        if response.len() > MAX_PTY_RESPONSE_BYTES.saturating_sub(self.bytes.len()) {
            self.overflowed = true;
            return;
        }
        self.bytes.extend_from_slice(response);
    }
}

#[derive(Debug, Error)]
enum WorkerError {
    #[error("thread error: {0}")]
    Thread(String),
    #[error("spawn error: {0}")]
    Spawn(String),
    #[error("PTY error: {0}")]
    Pty(String),
    #[error("terminal emulation error: {0}")]
    Ghostty(#[from] libghostty_vt::Error),
    #[error("terminal I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminal search snapshot exceeds the {MAX_SEARCH_SNAPSHOT_BYTES}-byte limit")]
    SearchSnapshotTooLarge,
    #[error("native mode revision exceeds the 128 MiB limit")]
    ModeRevisionTooLarge,
    #[error("terminal viewport metadata exceeds its 32-bit in-memory limits")]
    ViewportMetadataTooLarge,
    #[error(
        "terminal reliable-event backlog exceeded {MAX_PENDING_RELIABLE_EVENTS} messages or {MAX_PENDING_RELIABLE_EVENT_BYTES} bytes"
    )]
    EventBackpressure,
    #[error("terminal event consumer stopped")]
    EventConsumerStopped,
}

fn warn_and_skip_view_ghostty<T>(
    context: &str,
    result: Result<T, WorkerError>,
) -> Result<Option<T>, WorkerError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(WorkerError::Ghostty(error)) => {
            log::warn!("skipping terminal {context} after emulation error: {error}");
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn normalize_view_action_result(
    result: Result<ViewActionResult, WorkerError>,
) -> Result<ViewActionResult, WorkerError> {
    match result {
        Err(error @ (WorkerError::SearchSnapshotTooLarge | WorkerError::ModeRevisionTooLarge)) => {
            log::warn!("skipping terminal view action: {error}");
            Ok(ViewActionResult::Snapshot)
        }
        result => result,
    }
}

#[derive(Clone)]
struct Publisher {
    event_tx: async_channel::Sender<TerminalEvent>,
    latest: Arc<RwLock<PublishedViewports>>,
    state: Arc<EventQueueState>,
}

impl Publisher {
    fn set_foreground_source(&self, source: Option<Box<ForegroundSource>>) {
        *self.state.foreground.write() = source;
    }

    fn set_completion(&self, completion: TerminalProcessExit) {
        self.state
            .completion
            .store(completion.encode(), Ordering::Release);
    }

    fn set_progress_bar(&self, bar: ProgressBar) {
        self.latest.write().bar = bar;
    }

    fn publish(&self, viewport: TerminalViewport) {
        let viewport = Arc::new(viewport);
        {
            let mut latest = self.latest.write();
            latest.fallback = Arc::clone(&viewport);
            latest.by_view.clear();
            latest.copy_facts.clear();
        }
        self.notify_viewports(&viewport, 0);
    }

    fn publish_copy_facts(&self, facts: HashMap<TerminalViewId, Arc<CopyModeFacts>>) {
        self.latest.write().copy_facts = facts;
    }

    fn publish_frozen_history(&self, revision: Arc<ModeRevision>) {
        self.latest.write().frozen = Some(Arc::new(FrozenHistory { revision }));
    }

    fn publish_viewports(&self, viewports: Vec<(TerminalViewId, TerminalViewport)>) {
        let mut by_view = HashMap::with_capacity(viewports.len());
        let mut fallback = None;
        for (view, viewport) in viewports {
            let viewport = Arc::new(viewport);
            fallback.get_or_insert_with(|| Arc::clone(&viewport));
            by_view.insert(view, viewport);
        }
        let Some(fallback) = fallback else {
            return;
        };
        let view_count = by_view.len();
        {
            let mut latest = self.latest.write();
            latest.fallback = Arc::clone(&fallback);
            latest.by_view = by_view;
        }
        self.notify_viewports(&fallback, view_count);
    }

    fn notify_viewports(&self, viewport: &TerminalViewport, view_count: usize) {
        let notification_was_pending = self.state.notification_pending.swap(true, Ordering::AcqRel);
        if !notification_was_pending {
            match self.event_tx.try_send(TerminalEvent::ViewportReady {
                output_activity: false,
            }) {
                Ok(()) | Err(async_channel::TrySendError::Closed(_)) => {}
                Err(async_channel::TrySendError::Full(_)) => {
                    self.state
                        .notification_pending
                        .store(false, Ordering::Release);
                    log::error!("terminal viewport notification queue overflow");
                }
            }
        }
        if log::log_enabled!(
            target: "zz_terminal::diagnostics::publisher",
            log::Level::Trace
        ) {
            log::trace!(
                target: "zz_terminal::diagnostics::publisher",
                "publish generation={} view_generation={} columns={} rows={} cells={} overlays={} status={:?} views={view_count} notification_was_pending={notification_was_pending} event_queue_len={} event_queue_capacity={:?} pending_reliable={} pending_reliable_bytes={}",
                viewport.generation,
                viewport.view_generation,
                viewport.columns,
                viewport.rows,
                viewport.cells.len(),
                viewport.overlays.len(),
                viewport.status,
                self.event_tx.len(),
                self.event_tx.capacity(),
                self.state.pending_reliable.load(Ordering::Acquire),
                self.state.pending_reliable_bytes.load(Ordering::Acquire),
            );
        }
    }

    fn set_status(&self, status: &SessionStatus) {
        let (fallback, by_view) = {
            let latest = self.latest.read();
            (Arc::clone(&latest.fallback), latest.by_view.clone())
        };
        let update = |viewport: &TerminalViewport| {
            let mut viewport = viewport.clone();
            viewport.generation = viewport.generation.saturating_add(1);
            viewport.view_generation = viewport.view_generation.saturating_add(1);
            viewport.status = status.clone();
            viewport
        };
        if by_view.is_empty() {
            self.publish(update(&fallback));
        } else {
            self.publish_viewports(
                by_view
                    .into_iter()
                    .map(|(view, viewport)| (view, update(&viewport)))
                    .collect(),
            );
        }
    }

    fn mark_output_activity(&self) {
        self.state
            .output_activity_pending
            .store(true, Ordering::Release);
    }

    fn fail(&self, error: &WorkerError) {
        self.set_status(&SessionStatus::failed(error.to_string()));
    }

    fn send_reliable(&self, event: TerminalEvent) -> Result<(), WorkerError> {
        let bytes = reliable_event_bytes(&event);
        self.state
            .pending_reliable
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                (pending < MAX_PENDING_RELIABLE_EVENTS).then_some(pending + 1)
            })
            .map_err(|_| WorkerError::EventBackpressure)?;
        if self
            .state
            .pending_reliable_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                (bytes <= MAX_PENDING_RELIABLE_EVENT_BYTES.saturating_sub(pending))
                    .then_some(pending + bytes)
            })
            .is_err()
        {
            self.state.pending_reliable.fetch_sub(1, Ordering::AcqRel);
            return Err(WorkerError::EventBackpressure);
        }
        match self.event_tx.try_send(event) {
            Ok(()) => Ok(()),
            Err(async_channel::TrySendError::Full(_)) => {
                self.release_reliable(bytes);
                Err(WorkerError::EventBackpressure)
            }
            Err(async_channel::TrySendError::Closed(_)) => {
                self.release_reliable(bytes);
                Err(WorkerError::EventConsumerStopped)
            }
        }
    }

    fn release_reliable(&self, bytes: usize) {
        let previous = self.state.pending_reliable.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "reliable terminal event accounting underflow");
        let previous_bytes = self
            .state
            .pending_reliable_bytes
            .fetch_sub(bytes, Ordering::AcqRel);
        debug_assert!(
            previous_bytes >= bytes,
            "reliable terminal event byte accounting underflow"
        );
    }

    fn copy_ready(
        &self,
        view: TerminalViewId,
        copy: Box<TerminalCopyReady>,
    ) -> Result<(), WorkerError> {
        self.send_user_action(TerminalEvent::CopyReady { view, copy }, "copy result")
    }

    fn open_uri(&self, view: TerminalViewId, uri: String) -> Result<(), WorkerError> {
        self.send_user_action(
            TerminalEvent::OpenUri(Box::new(TerminalOpenUri { view, uri })),
            "open-URI request",
        )
    }

    fn view_closed(&self, view: TerminalViewId) -> Result<(), WorkerError> {
        self.send_reliable(TerminalEvent::ViewClosed(view))
    }

    fn clipboard_set(&self, target: ClipboardTarget, text: String) -> Result<(), WorkerError> {
        self.send_user_action(
            TerminalEvent::ClipboardSet { target, text },
            "clipboard write",
        )
    }

    fn rename_window(&self, name: String) -> Result<(), WorkerError> {
        self.send_user_action(TerminalEvent::RenameWindow(name), "window rename")
    }

    fn bell(&self) {
        if let Err(error) = self.send_reliable(TerminalEvent::Bell)
            && matches!(error, WorkerError::EventBackpressure)
        {
            log::warn!("discarding terminal bell: reliable event backlog is full");
        }
    }

    fn placeholder_bound(&self, token: u64, number: u32) -> Result<(), WorkerError> {
        self.send_reliable(TerminalEvent::PlaceholderBound { token, number })
    }

    fn pending_paste_expired(&self, token: u64) -> Result<(), WorkerError> {
        self.send_reliable(TerminalEvent::PendingPasteExpired { token })
    }

    fn raw_output_tap_closed(&self, token: u64) -> Result<(), WorkerError> {
        self.send_reliable(TerminalEvent::RawOutputTapClosed { token })
    }

    fn send_user_action(&self, event: TerminalEvent, description: &str) -> Result<(), WorkerError> {
        match self.send_reliable(event) {
            Err(WorkerError::EventBackpressure) => {
                log::warn!("discarding terminal {description}: reliable event backlog is full");
                Ok(())
            }
            result => result,
        }
    }
}

fn reliable_event_bytes(event: &TerminalEvent) -> usize {
    let payload = match event {
        TerminalEvent::CopyReady { copy, .. } => copy
            .text
            .len()
            .saturating_add(copy.pipe.as_ref().map_or(0, String::len))
            .saturating_add(copy.buffer.as_ref().map_or(0, |buffer| match buffer {
                PasteBufferAction::Create { prefix } => prefix.as_ref().map_or(0, String::len),
                PasteBufferAction::Append => 0,
            })),
        TerminalEvent::OpenUri(open) => open.uri.len(),
        TerminalEvent::ClipboardSet { text, .. } => text.len(),
        TerminalEvent::RenameWindow(name) => name.len(),
        TerminalEvent::ViewportReady { .. }
        | TerminalEvent::ViewClosed(_)
        | TerminalEvent::Bell
        | TerminalEvent::PlaceholderBound { .. }
        | TerminalEvent::PendingPasteExpired { .. }
        | TerminalEvent::RawOutputTapClosed { .. } => 0,
    };
    std::mem::size_of::<TerminalEvent>().saturating_add(payload)
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "the worker thread must own its channel and publisher"
)]
fn terminal_worker(
    control_rx: Receiver<Command>,
    input_rx: Receiver<QueuedInput>,
    publisher: Publisher,
    max_scrollback: usize,
    appearance: Arc<TerminalAppearance>,
    spawn: TerminalSpawn,
    wake: ActorWake,
    wake_rx: WakeReceiver,
) {
    if let Err(error) = run_terminal(
        &control_rx,
        &input_rx,
        &publisher,
        max_scrollback,
        &appearance,
        &spawn,
        &wake,
        wake_rx,
    ) {
        log::error!("terminal worker stopped: {error}");
        if matches!(&error, WorkerError::Spawn(_)) {
            publisher.set_completion(TerminalProcessExit {
                code: 1,
                signal: None,
            });
            publisher.set_status(&SessionStatus::exited(1, None));
        } else {
            publisher.fail(&error);
        }
    }
    publisher.set_foreground_source(None);
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "the worker thread owns its command-output source and channels"
)]
fn output_view_worker(
    command_rx: Receiver<Command>,
    publisher: Publisher,
    title: String,
    text: String,
    appearance: Arc<TerminalAppearance>,
    max_scrollback: usize,
    frozen: bool,
) {
    if let Err(error) = run_output_view(
        &command_rx,
        &publisher,
        &title,
        &text,
        &appearance,
        max_scrollback,
        frozen,
    ) {
        log::error!("command output view stopped: {error}");
        publisher.fail(&error);
    }
}

fn apply_terminal_appearance(
    terminal: &mut Terminal<'_, '_>,
    appearance: &TerminalAppearance,
) -> Result<(), WorkerError> {
    terminal.set_default_fg_color(Some(ghostty_color(appearance.foreground)))?;
    terminal.set_default_bg_color(Some(ghostty_color(appearance.background)))?;
    terminal.set_default_cursor_color(Some(ghostty_color(appearance.cursor_color)))?;
    terminal.set_default_cursor_style(Some(ghostty_cursor_style(appearance.cursor_style)))?;
    terminal.set_default_cursor_blink(Some(!matches!(
        appearance.cursor_blink_policy,
        CursorBlinkPolicy::Off
    )))?;
    terminal.set_default_color_palette(Some(Palette(
        (*appearance.palette.as_array()).map(ghostty_color),
    )))?;
    log::debug!(
        target: "zz_terminal::diagnostics::appearance",
        "applied appearance hash={} foreground={:?} background={:?} cursor={:?} cursor_style={:?} palette_entries=256",
        appearance.stable_hash(),
        appearance.foreground,
        appearance.background,
        appearance.cursor_color,
        appearance.cursor_style,
    );
    Ok(())
}

const fn ghostty_color(value: Color) -> RgbColor {
    RgbColor {
        r: value.r,
        g: value.g,
        b: value.b,
    }
}

const fn ghostty_cursor_style(value: CursorStyle) -> GhosttyCursorStyle {
    match value {
        CursorStyle::Bar => GhosttyCursorStyle::Bar,
        CursorStyle::Block => GhosttyCursorStyle::Block,
        CursorStyle::Underline => GhosttyCursorStyle::Underline,
        CursorStyle::BlockHollow => GhosttyCursorStyle::BlockHollow,
    }
}

const fn ghostty_color_scheme(value: TerminalColorScheme) -> ColorScheme {
    match value {
        TerminalColorScheme::Light => ColorScheme::Light,
        TerminalColorScheme::Dark => ColorScheme::Dark,
    }
}

fn clipboard_write_request<'a>(
    location: ClipboardLocation,
    contents: impl Iterator<Item = ClipboardContent<'a>>,
) -> Result<(ClipboardTarget, String), ClipboardWriteError> {
    let mut selected = None;
    for content in contents {
        if content.mime == CLIPBOARD_TEXT_MIME {
            selected = Some(content);
            break;
        }
        selected.get_or_insert(content);
    }
    let content = selected.ok_or(ClipboardWriteError::Unsupported)?;
    if content.data.len() > MAX_CLIPBOARD_WRITE_BYTES {
        return Err(ClipboardWriteError::Denied);
    }
    let target = match location {
        ClipboardLocation::Standard => ClipboardTarget::Clipboard,
        ClipboardLocation::Selection | ClipboardLocation::Primary => ClipboardTarget::Primary,
    };
    Ok((target, content.data.to_owned()))
}

fn register_clipboard_write(
    terminal: &mut Terminal<'_, '_>,
    publisher: Publisher,
) -> Result<(), WorkerError> {
    terminal.on_clipboard_write(move |_, write| {
        let (target, text) = clipboard_write_request(write.location(), write.contents())?;
        publisher
            .clipboard_set(target, text)
            .map_err(|_| ClipboardWriteError::Busy)
    })?;
    Ok(())
}

fn register_bell(terminal: &mut Terminal<'_, '_>, publisher: Publisher) -> Result<(), WorkerError> {
    terminal.on_bell(move |_| publisher.bell())?;
    Ok(())
}

fn register_device_attributes(terminal: &mut Terminal<'_, '_>) -> Result<(), WorkerError> {
    terminal.on_device_attributes(|_| {
        Some(DeviceAttributes {
            primary: PrimaryDeviceAttributes::new(
                ConformanceLevel::VT220,
                &[DeviceAttributeFeature::ANSI_COLOR],
            ),
            secondary: SecondaryDeviceAttributes {
                device_type: DeviceType::VT220,
                firmware_version: 1,
                rom_cartridge: 0,
            },
            tertiary: TertiaryDeviceAttributes { unit_id: 0 },
        })
    })?;
    Ok(())
}

fn run_output_view(
    command_rx: &Receiver<Command>,
    publisher: &Publisher,
    title: &str,
    text: &str,
    appearance: &TerminalAppearance,
    max_scrollback: usize,
    frozen: bool,
) -> Result<(), WorkerError> {
    install_kitty_png_decoder();
    let mut geometry = Geometry::default();
    let mut terminal = Terminal::new(TerminalOptions {
        cols: geometry.columns,
        rows: geometry.rows,
        max_scrollback,
    })?;
    let reported_color_scheme = Rc::new(Cell::new(ghostty_color_scheme(appearance.color_scheme)));
    let color_scheme_source = Rc::clone(&reported_color_scheme);
    terminal.on_color_scheme(move |_| Some(color_scheme_source.get()))?;
    apply_terminal_appearance(&mut terminal, appearance)?;
    register_bell(&mut terminal, publisher.clone())?;
    if frozen {
        write_output_view_content(&mut terminal, title, text);
    }
    let mut raw_output_tap: Option<(u64, Sender<Arc<[u8]>>)> = None;

    let mut render_state = RenderState::new()?;
    let mut row_iterator = RowIterator::new()?;
    let mut cell_iterator = CellIterator::new()?;
    let mut mouse_encoder = mouse::Encoder::new()?;
    let mut mouse_event = mouse::Event::new()?;
    let mut input_bytes = Vec::with_capacity(LINK_URI_SCRATCH_BYTES);
    let mut writer: Box<dyn Write + Send> = Box::new(std::io::sink());
    let mut word_separators = WordSeparators::default();
    let mut wrap_search = true;
    let mut mode_keys_vi = false;
    let mut active_views = ActiveTerminalViews::new();
    let mut inactive_views = InactiveTerminalViews::new();
    let bound_pasted_images = HashSet::new();
    let mut generations = ViewportGenerations::new()?;
    let mut dictionary = ViewportDictionary::default();
    let (mut search_worker, search_results) = SearchWorker::spawn(ActorWake::none())?;

    loop {
        crossbeam_channel::select_biased! {
            recv(command_rx) -> message => match message {
                Ok(Command::AttachView(view_id)) => {
                    if frozen {
                        if let Entry::Vacant(entry) = active_views.entry(view_id) {
                            let state = inactive_views
                                .remove(&view_id)
                                .map_or_else(|| output_view_state(&mut terminal).map(Box::new), Ok)?;
                            entry.insert(state);
                        }
                    } else {
                        activate_view(
                            &mut terminal,
                            view_id,
                            &mut active_views,
                            &mut inactive_views,
                            &word_separators,
                        )?;
                    }
                    if let Some(state) = active_views.get_mut(&view_id) {
                        let _ = refresh_view_search(
                            &terminal,
                            view_id,
                            state,
                            &mut search_worker,
                        )?;
                    }
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::Content,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Ok(Command::DetachView(view_id)) => {
                    if frozen {
                        if let Some(state) = active_views.remove(&view_id) {
                            inactive_views.insert(view_id, state);
                        }
                    } else {
                        deactivate_view(
                            &mut terminal,
                            view_id,
                            &mut active_views,
                            &mut inactive_views,
                            &word_separators,
                        )?;
                    }
                    search_worker.cancel(view_id);
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::View,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Ok(Command::ReleaseView(view_id)) => {
                    if frozen {
                        active_views.remove(&view_id);
                        inactive_views.remove(&view_id);
                    } else {
                        release_view(
                            &mut terminal,
                            view_id,
                            &mut active_views,
                            &mut inactive_views,
                        )?;
                    }
                    search_worker.forget(view_id);
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::View,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Ok(Command::Resize(next)) => {
                    if next != geometry {
                        geometry = next;
                        terminal.resize(
                            geometry.columns.max(1),
                            geometry.rows.max(1),
                            geometry.cell_width_px,
                            geometry.cell_height_px,
                        )?;
                        for view in inactive_views.values_mut() {
                            if frozen {
                                refresh_output_view(&mut terminal, view)?;
                            } else {
                                view.invalidate_layout();
                            }
                        }
                        for view in active_views.values_mut() {
                            if frozen {
                                refresh_output_view(&mut terminal, view)?;
                            } else {
                                view.invalidate_layout();
                                reconcile_view_screen(
                                    &mut terminal,
                                    view,
                                    &word_separators,
                                )?;
                            }
                        }
                        publish_active_views(
                            &mut terminal,
                            publisher,
                            &mut render_state,
                            &mut row_iterator,
                            &mut cell_iterator,
                            &mut generations,
                            SnapshotChange::Content,
                            &mut dictionary,
                            &mut active_views,
                            &word_separators,
                            SessionStatus::Running,
                        )?;
                    }
                }
                Ok(Command::SetWordSeparators(next)) => {
                    word_separators = *next;
                }
                Ok(Command::SetWrapSearch(next)) => {
                    wrap_search = next;
                }
                Ok(Command::SetAppearance(next)) => {
                    reported_color_scheme.set(ghostty_color_scheme(next.color_scheme));
                    apply_terminal_appearance(&mut terminal, &next)?;
                    render_state = RenderState::new()?;
                    for view in active_views.values_mut().chain(inactive_views.values_mut()) {
                        refresh_frozen_view_appearance(&mut terminal, view)?;
                    }
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::Content,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Ok(Command::ViewAction { view, action }) => {
                    let Some(state) = active_views.get_mut(&view) else {
                        continue;
                    };
                    let result = normalize_view_action_result(apply_view_action(
                        &mut terminal,
                        view,
                        state,
                        action,
                        geometry,
                        &mut writer,
                        &mut mouse_encoder,
                        &mut mouse_event,
                        &mut input_bytes,
                        &mut search_worker,
                        wrap_search,
                        mode_keys_vi,
                        &word_separators,
                        &bound_pasted_images,
                        &mut None,
                    ))?;
                    let closed = frozen && state.copy_mode.is_none();
                    match result {
                        ViewActionResult::Snapshot | ViewActionResult::ContentSnapshot if !closed => {
                            publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::View,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?;
                        }
                        ViewActionResult::OverlaySnapshot if !closed => {
                            publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::Overlay,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?;
                        }
                        ViewActionResult::Copy(copy) => publisher.copy_ready(view, copy)?,
                        ViewActionResult::OpenUri(uri) => publisher.open_uri(view, uri)?,
                        ViewActionResult::None
                        | ViewActionResult::Snapshot
                        | ViewActionResult::OverlaySnapshot
                        | ViewActionResult::ContentSnapshot => {}
                    }
                    if closed {
                        search_worker.forget(view);
                        active_views.remove(&view);
                        inactive_views.remove(&view);
                        publisher.view_closed(view)?;
                        publish_active_views(
                            &mut terminal,
                            publisher,
                            &mut render_state,
                            &mut row_iterator,
                            &mut cell_iterator,
                            &mut generations,
                            SnapshotChange::View,
                            &mut dictionary,
                            &mut active_views,
                            &word_separators,
                            SessionStatus::Running,
                        )?;
                    }
                }
                Ok(Command::Capture(request)) => {
                    let CaptureRequest { options, reply } = *request;
                    let mut copy_modes = active_views
                        .values()
                        .filter_map(|view| view.copy_mode.as_deref());
                    let mode = match (copy_modes.next(), copy_modes.next()) {
                        (Some(mode), None) => Some(mode),
                        _ => None,
                    };
                    let _ = reply.send(capture_terminal(&terminal, mode, options));
                }
                Ok(Command::SemanticCapture(request)) => {
                    let _ = request.reply.send(capture_last_command(&terminal));
                }
                Ok(Command::History(request)) => {
                    let HistoryCommand { start, reply, .. } = *request;
                    let _ = reply.send(empty_history_capture(&terminal, start));
                }
                Ok(Command::KittyImage(request)) => {
                    let _ = request.reply.send(None);
                }
                Ok(Command::KittyImageGeneration(request)) => {
                    let _ = request.reply.send(None);
                }
                Ok(Command::SetEngineKnobs(next)) => mode_keys_vi = next.mode_keys_vi,
                Ok(
                    Command::Text { .. }
                    | Command::Key { .. }
                        | Command::PastePreparedBytes { .. }
                        | Command::RawInput(_)
                        | Command::SetAllowPassthrough(_)
                        | Command::SetPendingCopySource(_)
                        | Command::WriteDeadNotice(_)
                        | Command::PendingPasteOpened { .. }
                    | Command::ResetScreen
                    | Command::UnbindPastedImage { .. },
                ) => {}
                Ok(Command::CaptureCopySource { reply }) => {
                    let _ = reply.send(
                        capture_copy_source(&mut terminal)
                            .map_err(|_| TerminalCaptureError::ActorStopped),
                    );
                }
                Ok(Command::Output(bytes)) => {
                    if let Some(token) = tap_raw_output_arc(&mut raw_output_tap, &bytes) {
                        publisher.raw_output_tap_closed(token)?;
                    }
                    terminal.vt_write(&bytes);
                    publisher.mark_output_activity();
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::Content,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Ok(Command::ArmRawOutputTap {
                    token,
                    output,
                    reply,
                }) => {
                    raw_output_tap = Some((token, output));
                    let _ = reply.send(true);
                }
                Ok(Command::DisarmRawOutputTap { token, reply }) => {
                    if raw_output_tap
                        .as_ref()
                        .is_some_and(|(armed, _)| *armed == token)
                    {
                        raw_output_tap = None;
                    }
                    let _ = reply.send(());
                }
                Ok(Command::Terminate | Command::Shutdown) | Err(_) => return Ok(()),
            },
            recv(search_results) -> result => {
                let result = result.map_err(|_| {
                    WorkerError::Thread("terminal search worker stopped".to_owned())
                })?;
                if apply_search_results(
                    &mut terminal,
                    &mut active_views,
                    &mut inactive_views,
                    &mut search_worker,
                    result,
                )? {
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::View,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
            }
        }
    }
}

fn write_output_view_content(terminal: &mut Terminal<'_, '_>, title: &str, text: &str) {
    let title = title
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect::<String>();
    terminal.vt_write(format!("\x1b]2;{title}\x07").as_bytes());
    for (index, line) in text.split('\n').enumerate() {
        if index != 0 {
            terminal.vt_write(b"\r\n");
        }
        terminal.vt_write(line.strip_suffix('\r').unwrap_or(line).as_bytes());
    }
}

fn output_view_state(terminal: &mut Terminal<'_, '_>) -> Result<TerminalViewState, WorkerError> {
    let mut view = TerminalViewState::for_screen(terminal.active_screen()?);
    let active = view.active_mut();
    enter_copy_mode(
        terminal,
        &mut active.selection,
        &mut active.copy_mode,
        false,
        false,
        None,
        false,
    )?;
    let mode = active
        .copy_mode
        .as_mut()
        .expect("entering output view creates a frozen revision");
    mode.kind = FrozenModeKind::View;
    mode.cursor = PointCoordinate { x: 0, y: 0 };
    mode.viewport_offset = 0;
    Ok(view)
}

fn refresh_output_view(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
) -> Result<(), WorkerError> {
    let Some(mode) = view.active_mut().copy_mode.as_mut() else {
        return Ok(());
    };
    let old_total = mode.revision.total_rows();
    let old_maximum = mode.revision.maximum_offset();
    let old_cursor = mode.cursor;
    let old_offset = mode.viewport_offset;
    let revision = ModeRevision::capture(terminal)?;
    let scale = |value: u32, old_limit: u32, new_limit: u32| {
        if old_limit == 0 {
            0
        } else {
            u32::try_from(
                u64::from(value)
                    .saturating_mul(u64::from(new_limit))
                    .saturating_div(u64::from(old_limit)),
            )
            .unwrap_or(new_limit)
            .min(new_limit)
        }
    };
    mode.cursor = revision.clamp_point(PointCoordinate {
        x: old_cursor.x.min(revision.columns.saturating_sub(1)),
        y: scale(
            old_cursor.y,
            old_total.saturating_sub(1),
            revision.total_rows().saturating_sub(1),
        ),
    });
    mode.viewport_offset = scale(old_offset, old_maximum, revision.maximum_offset());
    mode.revision = revision;
    mode.selection = None;
    mode.mark = None;
    mode.last_jump = None;
    mode.selecting = false;
    view.search_snapshot = None;
    if view.search.is_some() {
        complete_view_search(terminal, view)?;
    }
    Ok(())
}

fn refresh_frozen_view_appearance(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
) -> Result<bool, WorkerError> {
    let Some(mode) = view.active_mut().copy_mode.as_mut() else {
        return Ok(false);
    };
    if mode.revision.matches_terminal_appearance(terminal)? {
        return Ok(false);
    }

    let revision = ModeRevision::capture(terminal)?;
    mode.cursor = revision.clamp_point(mode.cursor);
    mode.viewport_offset = mode.viewport_offset.min(revision.maximum_offset());
    if let Some(selection) = mode.selection.as_mut() {
        selection.anchor = revision.clamp_point(selection.anchor);
        selection.focus = revision.clamp_point(selection.focus);
    }
    if let Some(mark) = mode.mark.as_mut() {
        *mark = revision.clamp_point(*mark);
    }
    mode.revision = revision;
    view.search_snapshot = None;
    if view.search.is_some() {
        complete_view_search(terminal, view)?;
    }
    Ok(true)
}

fn terminal_command(spawn: &TerminalSpawn) -> CommandBuilder {
    match spawn.command.as_deref() {
        #[cfg(windows)]
        Some(argv) if !argv.is_empty() => {
            let mut command = CommandBuilder::new("cmd.exe");
            let joined = argv.join(" ");
            command.args(["/C", joined.as_str()]);
            command
        }
        #[cfg(not(windows))]
        Some(argv) if argv.len() >= 2 => {
            let mut command = CommandBuilder::new(&argv[0]);
            command.args(&argv[1..]);
            command
        }
        #[cfg(not(windows))]
        Some([shell_command]) => {
            let mut command = CommandBuilder::new(spawn.shell.as_deref().unwrap_or("/bin/sh"));
            command.args(["-c", shell_command]);
            command
        }
        None if spawn.non_login_shell => {
            CommandBuilder::new(spawn.shell.as_deref().unwrap_or("/bin/sh"))
        }
        _ => crate::shell_integration::default_shell_command(spawn.shell.as_deref()),
    }
}

#[cfg(all(test, not(windows)))]
#[test]
fn terminal_command_preserves_tmux_argv_shapes() {
    let direct = terminal_command(&TerminalSpawn {
        command: Some(vec![
            "printf".to_owned(),
            "%s".to_owned(),
            "$HOME".to_owned(),
        ]),
        shell: Some("/bin/zsh".to_owned()),
        ..TerminalSpawn::default()
    });
    assert_eq!(
        direct.get_argv().as_slice(),
        [
            std::ffi::OsString::from("printf"),
            std::ffi::OsString::from("%s"),
            std::ffi::OsString::from("$HOME"),
        ]
    );

    let shell = terminal_command(&TerminalSpawn {
        command: Some(vec!["printf '%s' \"$HOME\"".to_owned()]),
        shell: Some("/bin/zsh".to_owned()),
        ..TerminalSpawn::default()
    });
    assert_eq!(
        shell.get_argv().as_slice(),
        [
            std::ffi::OsString::from("/bin/zsh"),
            std::ffi::OsString::from("-c"),
            std::ffi::OsString::from("printf '%s' \"$HOME\""),
        ]
    );

    let default = terminal_command(&TerminalSpawn {
        shell: Some("/bin/sh".to_owned()),
        ..TerminalSpawn::default()
    });
    assert!(default.is_default_prog());
    assert_eq!(default.get_shell(), "/bin/sh");

    let plain = terminal_command(&TerminalSpawn {
        shell: Some("/bin/sh".to_owned()),
        non_login_shell: true,
        ..TerminalSpawn::default()
    });
    assert!(!plain.is_default_prog());
    assert_eq!(
        plain.get_argv().as_slice(),
        [std::ffi::OsString::from("/bin/sh")]
    );

    let empty = terminal_command(&TerminalSpawn {
        command: Some(vec![String::new()]),
        shell: Some("/bin/sh".to_owned()),
        non_login_shell: true,
        ..TerminalSpawn::default()
    });
    assert_eq!(
        empty.get_argv().as_slice(),
        [
            std::ffi::OsString::from("/bin/sh"),
            std::ffi::OsString::from("-c"),
            std::ffi::OsString::new(),
        ]
    );
}

fn run_terminal(
    control_rx: &Receiver<Command>,
    input_rx: &Receiver<QueuedInput>,
    publisher: &Publisher,
    max_scrollback: usize,
    appearance: &TerminalAppearance,
    spawn: &TerminalSpawn,
    wake: &ActorWake,
    wake_rx: WakeReceiver,
) -> Result<(), WorkerError> {
    install_kitty_png_decoder();
    #[cfg(any(target_os = "linux", not(unix)))]
    let () = wake_rx;
    let mut geometry = spawn
        .initial_size
        .map(Geometry::from_size)
        .unwrap_or_default();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(geometry.pty_size())
        .map_err(|error| WorkerError::Pty(error.to_string()))?;
    #[cfg(unix)]
    let tty = pair.master.tty_name();
    #[cfg(not(unix))]
    let tty = None;

    #[cfg(unix)]
    if let Some(descriptor) = pair.master.as_raw_fd() {
        force_pty_erase(descriptor, spawn.knobs.verase_byte);
    }

    let mut command = terminal_command(spawn);
    command.env(
        "TERM",
        spawn.terminal_type.as_deref().unwrap_or("tmux-256color"),
    );
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "zz");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    for (key, value) in &spawn.env {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    if let Some(shell) = &spawn.shell {
        command.env("SHELL", shell);
    }
    if let Some(working_directory) = &spawn.working_directory {
        command.cwd(working_directory);
    }

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| WorkerError::Spawn(error.to_string()))?;
    drop(pair.slave);

    let shell_process_id = child.process_id();
    let mut killer = child.clone_killer();
    let (exit_tx, exit_rx) = crossbeam_channel::bounded(1);
    #[cfg(windows)]
    let (master_close_tx, master_close_rx) = crossbeam_channel::bounded(1);
    let exit_wake = wake.clone();
    thread::Builder::new()
        .name("zz-child-wait".into())
        .spawn(move || {
            let status = child.wait();
            let _ = exit_tx.send(status);
            exit_wake.notify();
            #[cfg(windows)]
            if let Ok(master) = master_close_rx.recv() {
                drop(master);
            }
        })
        .map_err(WorkerError::Io)?;

    #[cfg(all(unix, not(target_os = "linux")))]
    let wake_rx = wake_rx.map_err(|error| {
        WorkerError::Pty(format!("failed to configure terminal wake pipe: {error}"))
    })?;
    #[cfg(unix)]
    let (drain_fd, mut writer) = {
        let dup = || {
            pair.master
                .as_raw_fd()
                .and_then(|fd| filedescriptor::FileDescriptor::dup(&fd).ok())
                .ok_or_else(|| WorkerError::Pty("failed to duplicate the PTY master".to_owned()))
        };
        let drain_fd = dup()?;
        let writer_fd = dup()?;
        let _ = rustix::io::fcntl_setfd(&drain_fd, rustix::io::FdFlags::CLOEXEC);
        let _ = rustix::io::fcntl_setfd(&writer_fd, rustix::io::FdFlags::CLOEXEC);
        rustix::io::ioctl_fionbio(&drain_fd, true).map_err(|errno| {
            WorkerError::Pty(format!(
                "failed to make the PTY master nonblocking: {errno}"
            ))
        })?;
        let writer = PtyWriter::new(writer_fd);
        (drain_fd, writer)
    };
    #[cfg(not(unix))]
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| WorkerError::Pty(error.to_string()))?;
    #[cfg(not(unix))]
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| WorkerError::Pty(error.to_string()))?;
    #[cfg(not(unix))]
    let mut master = Some(pair.master);
    #[cfg(unix)]
    let master = Arc::new(parking_lot::Mutex::new(pair.master));
    publisher.set_foreground_source(Some(Box::new(ForegroundSource {
        #[cfg(unix)]
        master: Arc::clone(&master),
        shell: shell_process_id,
        tty,
    })));

    #[cfg(any(target_os = "linux", not(unix)))]
    let (output_rx, recycle_tx) = {
        let (output_tx, output_rx) = crossbeam_channel::bounded(PTY_BUFFER_POOL_SIZE);
        let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(PTY_BUFFER_POOL_SIZE);
        for _ in 0..PTY_BUFFER_POOL_SIZE {
            recycle_tx
                .send(vec![0_u8; PTY_READ_BUFFER_BYTES])
                .map_err(|error| WorkerError::Thread(error.to_string()))?;
        }
        #[cfg(target_os = "linux")]
        thread::Builder::new()
            .name("zz-pty-gather".into())
            .spawn(move || gather_pty_linux(drain_fd, output_tx, recycle_rx))
            .map_err(WorkerError::Io)?;
        #[cfg(not(unix))]
        {
            let pending_output: Box<dyn Fn() -> usize + Send> = Box::new(|| 0);
            thread::Builder::new()
                .name("zz-pty-reader".into())
                .spawn(move || read_pty(reader, pending_output, output_tx, recycle_rx))
                .map_err(WorkerError::Io)?;
        }
        (output_rx, recycle_tx)
    };

    let effects = Rc::new(RefCell::new(PtyEffects::new()));
    let effect_sink = Rc::clone(&effects);
    let reported_size = Rc::new(Cell::new(geometry.size_report()));
    let size_source = Rc::clone(&reported_size);

    let mut terminal = Terminal::new(TerminalOptions {
        cols: geometry.columns,
        rows: geometry.rows,
        max_scrollback,
    })?;
    terminal.resize(
        geometry.columns,
        geometry.rows,
        geometry.cell_width_px,
        geometry.cell_height_px,
    )?;
    terminal.on_pty_write(move |_, bytes| {
        effect_sink.borrow_mut().push(bytes);
    })?;
    configure_kitty_storage(&mut terminal)?;
    register_device_attributes(&mut terminal)?;
    terminal.on_size(move |_| Some(size_source.get()))?;
    let reported_color_scheme = Rc::new(Cell::new(ghostty_color_scheme(appearance.color_scheme)));
    let color_scheme_source = Rc::clone(&reported_color_scheme);
    terminal.on_color_scheme(move |_| Some(color_scheme_source.get()))?;
    terminal.on_xtversion(|_| Some(concat!("zz ", env!("CARGO_PKG_VERSION"))))?;
    register_clipboard_write(&mut terminal, publisher.clone())?;
    register_bell(&mut terminal, publisher.clone())?;
    apply_terminal_appearance(&mut terminal, appearance)?;

    let mut render_state = RenderState::new()?;
    let mut row_iterator = RowIterator::new()?;
    let mut cell_iterator = CellIterator::new()?;
    let mut key_encoder = key::Encoder::new()?;
    let mut key_event = key::Event::new()?;
    let mut mouse_encoder = mouse::Encoder::new()?;
    let mut mouse_event = mouse::Event::new()?;
    let mut input_bytes = Vec::with_capacity(LINK_URI_SCRATCH_BYTES);
    let mut word_separators = WordSeparators::default();
    let mut wrap_search = true;
    let mut passthrough = PassthroughFilter::default();
    let mut engine_knobs = spawn.knobs;
    let mut pending_copy_source: Option<Box<CapturedCopySource>> = None;
    let mut engine_filter = EngineFilter::default();
    let mut engine_renames = Vec::new();
    let mut engine_bar: Option<ProgressBar> = None;
    let mut active_views = ActiveTerminalViews::new();
    let mut inactive_views = InactiveTerminalViews::new();
    let mut generations = ViewportGenerations::new()?;
    let mut dictionary = ViewportDictionary::default();
    let mut pasted_image_bindings = PastedImageBindings::default();
    let mut reader_eof = false;
    let mut exit_status = None;
    let mut terminating = false;
    let mut termination_deadline = None::<Instant>;
    let mut termination_escalated = false;
    let no_exit = crossbeam_channel::never();
    #[cfg(any(target_os = "linux", not(unix)))]
    let no_output = crossbeam_channel::never();
    #[cfg(all(unix, not(target_os = "linux")))]
    let mut read_buffer = vec![0_u8; PTY_READ_BUFFER_BYTES];
    let (mut search_worker, search_results) = SearchWorker::spawn(wake.clone())?;
    let mut search_refresh_due = None::<Instant>;
    let mut last_content_publish = Instant::now();
    let mut output_pending = false;
    let mut vt_diagnostics = VtWriteDiagnostics::default();
    let mut raw_output_tap = None;
    let mut raw_output_parse_backlog = VecDeque::<(Arc<[u8]>, usize)>::new();
    let mut raw_output_parse_backlog_bytes = 0_usize;
    let mut raw_output_parse_buffer = Vec::with_capacity(RAW_OUTPUT_PARSE_TURN_BYTES);
    #[cfg(unix)]
    let mut active_input_permit = None::<InputPermit>;

    publisher.publish(snapshot(
        &terminal,
        &mut render_state,
        &mut row_iterator,
        &mut cell_iterator,
        &mut generations,
        SnapshotChange::Content,
        &mut dictionary,
        None,
        SessionStatus::Running,
    )?);

    loop {
        #[cfg(unix)]
        {
            writer.flush_pending()?;
            if !writer.has_pending() {
                active_input_permit.take();
            }
            drain_effects_if_writer_ready(&effects, &mut writer)?;
        }
        let now = Instant::now();
        if termination_deadline.is_some_and(|deadline| now >= deadline) {
            if termination_escalated {
                return Ok(());
            }
            #[cfg(unix)]
            signal_terminal_process_groups(
                &master,
                shell_process_id,
                rustix::process::Signal::KILL,
            );
            #[cfg(not(unix))]
            let _ = killer.kill();
            termination_escalated = true;
            termination_deadline = Some(now + TERMINATION_KILL_WAIT);
        }
        if search_refresh_due.is_some_and(|due| now >= due) {
            search_refresh_due = None;
            let mut view_ids = active_views.keys().copied().collect::<Vec<_>>();
            view_ids.sort_by_key(|view| view.0);
            for view_id in view_ids {
                let view = active_views
                    .get_mut(&view_id)
                    .expect("active search view was collected from the same map");
                if view.search.is_some() && view.copy_mode.is_none() {
                    let _ = refresh_view_search(&terminal, view_id, view, &mut search_worker)?;
                }
            }
        }
        let pending_window_due = pasted_image_bindings
            .next_deadline()
            .is_some_and(|deadline| deadline <= now);
        if output_pending
            && (reader_eof
                || pending_window_due
                || last_content_publish.elapsed() >= CONTENT_PUBLISH_STALENESS)
        {
            #[cfg(unix)]
            drain_effects_if_writer_ready(&effects, &mut writer)?;
            #[cfg(not(unix))]
            drain_effects(&effects, &mut writer)?;
            if let Some((token, number)) = pasted_image_bindings.observe(&terminal)? {
                publisher.placeholder_bound(token, number)?;
            }
            let output_screen = terminal.active_screen()?;
            for view in inactive_views.values_mut() {
                view.note_output(output_screen);
            }
            let mut refresh_search = false;
            for (view_id, view) in &mut active_views {
                note_output_and_revalidate_image_hover(
                    &mut terminal,
                    view,
                    output_screen,
                    &word_separators,
                    pasted_image_bindings.bound_numbers(),
                )?;
                if view.search.is_some() && view.copy_mode.is_none() {
                    search_worker.cancel(*view_id);
                    refresh_search = true;
                }
            }
            if refresh_search {
                search_refresh_due.get_or_insert_with(|| Instant::now() + SEARCH_REFRESH_DEBOUNCE);
            } else {
                search_refresh_due = None;
            }
            publisher.mark_output_activity();
            publish_active_views(
                &mut terminal,
                publisher,
                &mut render_state,
                &mut row_iterator,
                &mut cell_iterator,
                &mut generations,
                SnapshotChange::Content,
                &mut dictionary,
                &mut active_views,
                &word_separators,
                SessionStatus::Running,
            )?;
            last_content_publish = Instant::now();
            output_pending = false;
            vt_diagnostics.emit();
        }
        for token in pasted_image_bindings.expire(Instant::now()) {
            publisher.pending_paste_expired(token)?;
        }
        for name in engine_renames.drain(..) {
            publisher.rename_window(name)?;
        }
        if let Some(bar) = engine_bar.take() {
            publisher.set_progress_bar(bar);
        }

        let mut deadline = Instant::now() + IDLE_SLEEP;
        if output_pending {
            deadline = deadline.min(last_content_publish + CONTENT_PUBLISH_STALENESS);
        }
        if let Some(due) = search_refresh_due {
            deadline = deadline.min(due);
        }
        if let Some(due) = pasted_image_bindings.next_deadline() {
            deadline = deadline.min(due);
        }
        if let Some(due) = termination_deadline {
            deadline = deadline.min(due);
        }
        if !raw_output_parse_backlog.is_empty() {
            deadline = Instant::now();
        }
        #[cfg(unix)]
        if writer.has_pending() {
            deadline = deadline.min(Instant::now() + PTY_WRITE_RETRY);
        }
        let timeout = deadline.saturating_duration_since(Instant::now());
        let child_exit = if exit_status.is_some() {
            &no_exit
        } else {
            &exit_rx
        };
        #[cfg(unix)]
        let available_input = (!writer.has_pending()).then_some(input_rx);
        #[cfg(not(unix))]
        let available_input = Some(input_rx);
        let raw_output_read_ahead = raw_output_parse_backlog_bytes
            <= RAW_OUTPUT_PARSE_BACKLOG_BYTES.saturating_sub(RAW_OUTPUT_PARSE_READ_RESERVE_BYTES);
        #[cfg(all(unix, not(target_os = "linux")))]
        let wakeup = wait_for_wake(
            control_rx,
            available_input,
            &search_results,
            child_exit,
            (!reader_eof && raw_output_read_ahead).then_some(&drain_fd),
            &wake_rx,
            timeout,
        )?;
        #[cfg(target_os = "linux")]
        let available_output = if reader_eof || !raw_output_read_ahead {
            &no_output
        } else {
            &output_rx
        };
        #[cfg(not(unix))]
        let available_output = if reader_eof || !raw_output_read_ahead {
            &no_output
        } else {
            &output_rx
        };
        #[cfg(any(target_os = "linux", not(unix)))]
        let wakeup = wait_for_wake(
            control_rx,
            available_input,
            &search_results,
            child_exit,
            available_output,
            timeout,
        )?;

        let mut input_permit = None;
        let wakeup = match wakeup {
            Wake::Input(QueuedInput { command, permit }) => {
                input_permit = Some(permit);
                Wake::Command(command)
            }
            wakeup => wakeup,
        };
        match wakeup {
            Wake::Command(command) => match command {
                Command::Text { view, text } => {
                    if exit_status.is_none() {
                        let viewport_changed = if let Some(view) = view
                            && let Some(state) = active_views.get_mut(&view)
                        {
                            restore_view_state(&mut terminal, state, &word_separators)?;
                            if state.copy_mode.is_none() && state.search.is_some() {
                                search_worker.cancel(view);
                            }
                            prepare_live_input(&mut terminal, state)?
                        } else {
                            false
                        };
                        writer.write_all(text.as_bytes())?;
                        writer.flush()?;
                        if viewport_changed {
                            publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::View,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?;
                        }
                    }
                }
                Command::Key { view, input } => {
                    if exit_status.is_none() {
                        let viewport_changed = if let Some(view) = view
                            && let Some(state) = active_views.get_mut(&view)
                        {
                            restore_view_state(&mut terminal, state, &word_separators)?;
                            if state.copy_mode.is_none() && state.search.is_some() {
                                search_worker.cancel(view);
                            }
                            prepare_live_input(&mut terminal, state)?
                        } else {
                            false
                        };
                        encode_key(
                            &terminal,
                            &mut key_encoder,
                            &mut key_event,
                            *input,
                            engine_knobs.erase_byte,
                            &mut writer,
                            &mut input_bytes,
                        )?;
                        if viewport_changed {
                            publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::View,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?;
                        }
                    }
                }
                Command::PastePreparedBytes {
                    view,
                    bytes,
                    bracketed,
                } => {
                    if exit_status.is_none() {
                        let viewport_changed = if let Some(view) = view
                            && let Some(state) = active_views.get_mut(&view)
                        {
                            restore_view_state(&mut terminal, state, &word_separators)?;
                            if state.copy_mode.is_none() && state.search.is_some() {
                                search_worker.cancel(view);
                            }
                            prepare_live_input(&mut terminal, state)?
                        } else {
                            false
                        };
                        write_prepared_paste_bytes(&terminal, &bytes, bracketed, &mut writer)?;
                        if viewport_changed {
                            publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::View,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?;
                        }
                    }
                }
                Command::Output(_) => {}
                Command::RawInput(bytes) => {
                    if exit_status.is_none() {
                        writer.write_all(&bytes)?;
                        writer.flush()?;
                    }
                }
                Command::ArmRawOutputTap {
                    token,
                    output,
                    reply,
                } => {
                    raw_output_tap = Some((token, output));
                    let _ = reply.send(true);
                }
                Command::DisarmRawOutputTap { token, reply } => {
                    if raw_output_tap
                        .as_ref()
                        .is_some_and(|(armed, _)| *armed == token)
                    {
                        raw_output_tap = None;
                    }
                    let _ = reply.send(());
                }
                Command::Resize(next) => {
                    if next != geometry {
                        search_refresh_due = None;
                        geometry = next;
                        reported_size.set(geometry.size_report());
                        #[cfg(unix)]
                        master
                            .lock()
                            .resize(geometry.pty_size())
                            .map_err(|error| WorkerError::Pty(error.to_string()))?;
                        #[cfg(not(unix))]
                        if let Some(master) = &master {
                            master
                                .resize(geometry.pty_size())
                                .map_err(|error| WorkerError::Pty(error.to_string()))?;
                        }
                        terminal.resize(
                            geometry.columns.max(1),
                            geometry.rows.max(1),
                            geometry.cell_width_px,
                            geometry.cell_height_px,
                        )?;
                        for view in inactive_views.values_mut() {
                            view.invalidate_layout();
                        }
                        #[cfg(unix)]
                        drain_effects_if_writer_ready(&effects, &mut writer)?;
                        #[cfg(not(unix))]
                        drain_effects(&effects, &mut writer)?;
                        for (view_id, view) in &mut active_views {
                            view.invalidate_layout();
                            reconcile_view_screen(&mut terminal, view, &word_separators)?;
                            if view.copy_mode.is_none() {
                                let _ = refresh_view_search(
                                    &terminal,
                                    *view_id,
                                    view,
                                    &mut search_worker,
                                )?;
                            }
                        }
                        publish_active_views(
                            &mut terminal,
                            publisher,
                            &mut render_state,
                            &mut row_iterator,
                            &mut cell_iterator,
                            &mut generations,
                            SnapshotChange::Content,
                            &mut dictionary,
                            &mut active_views,
                            &word_separators,
                            SessionStatus::Running,
                        )?;
                    }
                }
                Command::SetWordSeparators(next) => {
                    word_separators = *next;
                    let mut selection_changed = false;
                    for view in active_views.values_mut() {
                        if view.copy_mode.is_none()
                            && view
                                .selection
                                .as_ref()
                                .is_some_and(|selection| selection.mode == SelectionMode::Word)
                        {
                            restore_view_state(&mut terminal, view, &word_separators)?;
                            install_view_selection(&terminal, view, &word_separators)?;
                            selection_changed = true;
                        }
                    }
                    if selection_changed {
                        publish_active_views(
                            &mut terminal,
                            publisher,
                            &mut render_state,
                            &mut row_iterator,
                            &mut cell_iterator,
                            &mut generations,
                            SnapshotChange::View,
                            &mut dictionary,
                            &mut active_views,
                            &word_separators,
                            SessionStatus::Running,
                        )?;
                    }
                }
                Command::SetAllowPassthrough(next) => {
                    passthrough.set_mode(next);
                }
                Command::SetWrapSearch(next) => {
                    wrap_search = next;
                }
                Command::SetEngineKnobs(next) => {
                    engine_knobs = next;
                }
                Command::CaptureCopySource { reply } => {
                    let _ = reply.send(
                        capture_copy_source(&mut terminal)
                            .map_err(|_| TerminalCaptureError::ActorStopped),
                    );
                }
                Command::SetPendingCopySource(source) => {
                    pending_copy_source = source;
                }
                Command::WriteDeadNotice(_) => {
                    log::debug!("discarding a dead notice for a live pane");
                }
                Command::ResetScreen => {
                    reset_pane_screen(&mut terminal)?;
                    for view in active_views.values_mut().chain(inactive_views.values_mut()) {
                        let state = view.active_mut();
                        state.selection = None;
                        state.hover_link = None;
                    }
                    terminal.set_selection(None)?;
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::Content,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Command::SetAppearance(next) => {
                    reported_color_scheme.set(ghostty_color_scheme(next.color_scheme));
                    apply_terminal_appearance(&mut terminal, &next)?;
                    render_state = RenderState::new()?;
                    for view in active_views.values_mut().chain(inactive_views.values_mut()) {
                        refresh_frozen_view_appearance(&mut terminal, view)?;
                    }
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::Content,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Command::AttachView(view) => {
                    search_worker.cancel(view);
                    activate_view(
                        &mut terminal,
                        view,
                        &mut active_views,
                        &mut inactive_views,
                        &word_separators,
                    )?;
                    if let Some(state) = active_views.get_mut(&view) {
                        let _ = refresh_view_search(&terminal, view, state, &mut search_worker)?;
                        sync_viewport_anchor(&terminal, state)?;
                    }
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::View,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Command::DetachView(view) => {
                    search_worker.cancel(view);
                    deactivate_view(
                        &mut terminal,
                        view,
                        &mut active_views,
                        &mut inactive_views,
                        &word_separators,
                    )?;
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::View,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Command::ReleaseView(view) => {
                    search_worker.forget(view);
                    release_view(&mut terminal, view, &mut active_views, &mut inactive_views)?;
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::View,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
                Command::ViewAction { view, action } => {
                    if active_views.contains_key(&view) {
                        let explicitly_enters_copy_mode = matches!(
                            &action,
                            TerminalViewAction::EnterCopyMode
                                | TerminalViewAction::EnterCopyModeScrollExit
                                | TerminalViewAction::EnterCopyModeWith { .. }
                        );
                        let clears_history = matches!(&action, TerminalViewAction::ClearHistory);
                        let was_in_copy_mode = active_views
                            .get(&view)
                            .is_some_and(|state| state.copy_mode.is_some());
                        if matches!(
                            &action,
                            TerminalViewAction::SearchBegin(_)
                                | TerminalViewAction::SearchUpdate(_)
                                | TerminalViewAction::SearchClose
                                | TerminalViewAction::ClearHistory
                        ) {
                            search_refresh_due = None;
                        }
                        let result = {
                            let state = active_views
                                .get_mut(&view)
                                .expect("active view was checked above");
                            restore_view_state(&mut terminal, state, &word_separators)?;
                            normalize_view_action_result(apply_view_action(
                                &mut terminal,
                                view,
                                state,
                                action,
                                geometry,
                                &mut writer,
                                &mut mouse_encoder,
                                &mut mouse_event,
                                &mut input_bytes,
                                &mut search_worker,
                                wrap_search,
                                engine_knobs.mode_keys_vi,
                                &word_separators,
                                pasted_image_bindings.bound_numbers(),
                                &mut pending_copy_source,
                            ))?
                        };
                        let is_in_copy_mode = active_views
                            .get(&view)
                            .is_some_and(|state| state.copy_mode.is_some());
                        let entered_copy_mode = !was_in_copy_mode && is_in_copy_mode;
                        let leaves_copy_mode =
                            clears_history || (was_in_copy_mode && !is_in_copy_mode);
                        if explicitly_enters_copy_mode || entered_copy_mode || leaves_copy_mode {
                            render_state.update(&terminal)?.set_dirty(Dirty::Full)?;
                        }
                        if leaves_copy_mode {
                            let state = active_views
                                .get_mut(&view)
                                .expect("active view was checked above");
                            reconcile_view_screen(&mut terminal, state, &word_separators)?;
                        }
                        if matches!(
                            &result,
                            ViewActionResult::Snapshot
                                | ViewActionResult::ContentSnapshot
                                | ViewActionResult::Copy(_)
                        ) {
                            let state = active_views
                                .get_mut(&view)
                                .expect("active view was checked above");
                            sync_viewport_anchor(&terminal, state)?;
                        }
                        match result {
                            ViewActionResult::None => {}
                            ViewActionResult::Snapshot => publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::View,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?,
                            ViewActionResult::OverlaySnapshot => publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::Overlay,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?,
                            ViewActionResult::ContentSnapshot => publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::Content,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?,
                            ViewActionResult::Copy(copy) => {
                                let view_changed = copy.view_changed;
                                publisher.copy_ready(view, copy)?;
                                if view_changed {
                                    publish_active_views(
                                        &mut terminal,
                                        publisher,
                                        &mut render_state,
                                        &mut row_iterator,
                                        &mut cell_iterator,
                                        &mut generations,
                                        SnapshotChange::View,
                                        &mut dictionary,
                                        &mut active_views,
                                        &word_separators,
                                        SessionStatus::Running,
                                    )?;
                                }
                            }
                            ViewActionResult::OpenUri(uri) => publisher.open_uri(view, uri)?,
                        }
                    }
                }
                Command::Capture(request) => {
                    let CaptureRequest { options, reply } = *request;
                    let mut copy_modes = active_views
                        .values()
                        .filter_map(|view| view.copy_mode.as_deref());
                    let mode = match (copy_modes.next(), copy_modes.next()) {
                        (Some(mode), None) => Some(mode),
                        _ => None,
                    };
                    let result = capture_terminal(&terminal, mode, options);
                    let _ = reply.send(result);
                }
                Command::SemanticCapture(request) => {
                    let _ = request.reply.send(capture_last_command(&terminal));
                }
                Command::History(request) => {
                    let HistoryCommand {
                        start,
                        count,
                        reply,
                    } = *request;
                    let _ = reply.send(capture_history(&terminal, start, count));
                }
                Command::KittyImage(request) => {
                    let image = generations
                        .kitty
                        .as_mut()
                        .map_or(Ok(None), |kitty| kitty.image(&terminal, request.image_id))
                        .unwrap_or_else(|error| {
                            log::warn!(
                                "could not export Kitty image {}: {error}",
                                request.image_id
                            );
                            None
                        });
                    let _ = request.reply.send(image);
                }
                Command::KittyImageGeneration(request) => {
                    let generation = generations
                        .kitty
                        .as_ref()
                        .map_or(Ok(None), |_| {
                            KittyGraphicsState::image_generation(&terminal, request.image_id)
                        })
                        .unwrap_or_else(|error| {
                            log::warn!(
                                "could not read Kitty image {} generation: {error}",
                                request.image_id
                            );
                            None
                        });
                    let _ = request.reply.send(generation);
                }
                Command::PendingPasteOpened { token } => {
                    for expired in pasted_image_bindings.expire(Instant::now()) {
                        publisher.pending_paste_expired(expired)?;
                    }
                    pasted_image_bindings.open(&terminal, token, Instant::now())?;
                }
                Command::UnbindPastedImage { number } => {
                    if pasted_image_bindings.unbind(number) {
                        let mut active_hover_changed = false;
                        for view in active_views.values_mut() {
                            active_hover_changed |= clear_pasted_image_hover(view, number);
                        }
                        for view in inactive_views.values_mut() {
                            clear_pasted_image_hover(view, number);
                        }
                        if active_hover_changed {
                            publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::Overlay,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::Running,
                            )?;
                        }
                    }
                }
                Command::Terminate => {
                    if !terminating {
                        terminating = true;
                        termination_deadline = Some(Instant::now() + TERMINATION_GRACE);
                        #[cfg(unix)]
                        signal_terminal_process_groups(
                            &master,
                            shell_process_id,
                            rustix::process::Signal::TERM,
                        );
                        #[cfg(not(unix))]
                        let _ = killer.kill();
                    }
                }
                Command::Shutdown => {
                    let _ = killer.kill();
                    return Ok(());
                }
            },
            Wake::CommandsClosed => {
                if terminating {
                    let remaining = termination_deadline
                        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                        .unwrap_or_default();
                    if exit_rx.recv_timeout(remaining).is_err() && !termination_escalated {
                        #[cfg(unix)]
                        signal_terminal_process_groups(
                            &master,
                            shell_process_id,
                            rustix::process::Signal::KILL,
                        );
                        #[cfg(not(unix))]
                        let _ = killer.kill();
                        let _ = exit_rx.recv_timeout(TERMINATION_KILL_WAIT);
                    }
                } else {
                    let _ = killer.kill();
                }
                return Ok(());
            }
            Wake::Input(_) => unreachable!("PTY input is normalized before dispatch"),
            Wake::Search(result) => {
                if apply_search_results(
                    &mut terminal,
                    &mut active_views,
                    &mut inactive_views,
                    &mut search_worker,
                    result,
                )? {
                    publish_active_views(
                        &mut terminal,
                        publisher,
                        &mut render_state,
                        &mut row_iterator,
                        &mut cell_iterator,
                        &mut generations,
                        SnapshotChange::View,
                        &mut dictionary,
                        &mut active_views,
                        &word_separators,
                        SessionStatus::Running,
                    )?;
                }
            }
            #[cfg(all(unix, not(target_os = "linux")))]
            Wake::PtyReadable => {
                let mut burst = 0_usize;
                let mut spins = 0_u32;
                let turn_started = Instant::now();
                loop {
                    match rustix::io::read(&drain_fd, &mut read_buffer[..]) {
                        Ok(0) => {
                            reader_eof = true;
                            break;
                        }
                        Ok(length) => {
                            log::trace!(
                                target: "zz_terminal::diagnostics::pty",
                                "read length={length} bytes={:?} text={:?}",
                                &read_buffer[..length],
                                String::from_utf8_lossy(&read_buffer[..length]),
                            );
                            if raw_output_tap.is_some() || !raw_output_parse_backlog.is_empty() {
                                let bytes = Arc::<[u8]>::from(&read_buffer[..length]);
                                if let Some(token) = tap_raw_output_arc(&mut raw_output_tap, &bytes)
                                {
                                    publisher.raw_output_tap_closed(token)?;
                                }
                                raw_output_parse_backlog_bytes =
                                    raw_output_parse_backlog_bytes.saturating_add(bytes.len());
                                raw_output_parse_backlog.push_back((bytes, 0));
                            } else {
                                let started = diagnostic_timer();
                                let parsed = feed_pty_output(
                                    &mut terminal,
                                    &mut passthrough,
                                    &mut EngineOutput {
                                        filter: &mut engine_filter,
                                        knobs: engine_knobs,
                                        renames: &mut engine_renames,
                                        bar: &mut engine_bar,
                                    },
                                    &read_buffer[..length],
                                );
                                vt_diagnostics.record(parsed, started);
                                output_pending |= parsed > 0;
                            }
                            burst += length;
                            spins = 0;
                            if burst >= PTY_DRAIN_TURN_BYTES
                                || raw_output_parse_backlog_bytes >= RAW_OUTPUT_PARSE_BACKLOG_BYTES
                                || turn_started.elapsed() >= PTY_DRAIN_TURN_TIME
                            {
                                break;
                            }
                        }
                        Err(rustix::io::Errno::INTR) => {}
                        Err(rustix::io::Errno::AGAIN) => {
                            if burst >= PTY_BRIDGE_THRESHOLD_BYTES && spins < PTY_BRIDGE_SPIN_MAX {
                                spins += 1;
                                continue;
                            }
                            break;
                        }
                        Err(_) => {
                            reader_eof = true;
                            break;
                        }
                    }
                }
            }
            #[cfg(any(target_os = "linux", not(unix)))]
            Wake::PtyMessage(message) => match message {
                ReaderMessage::Data { buffer, length } => {
                    let mut closed_tap = None;
                    let mut consumed_output = false;
                    reader_eof |=
                        drain_pty_output_burst(&output_rx, buffer, length, |buffer, length| {
                            if raw_output_tap.is_some() || !raw_output_parse_backlog.is_empty() {
                                log::trace!(
                                    target: "zz_terminal::diagnostics::pty",
                                    "read length={length} bytes={:?} text={:?}",
                                    &buffer[..length],
                                    String::from_utf8_lossy(&buffer[..length]),
                                );
                                let bytes = Arc::<[u8]>::from(&buffer[..length]);
                                closed_tap = closed_tap
                                    .or_else(|| tap_raw_output_arc(&mut raw_output_tap, &bytes));
                                raw_output_parse_backlog_bytes =
                                    raw_output_parse_backlog_bytes.saturating_add(bytes.len());
                                raw_output_parse_backlog.push_back((bytes, 0));
                                let _ = recycle_tx.try_send(buffer);
                            } else {
                                let started = diagnostic_timer();
                                let (closed, parsed) = consume_pty_output(
                                    &mut terminal,
                                    &mut passthrough,
                                    &mut EngineOutput {
                                        filter: &mut engine_filter,
                                        knobs: engine_knobs,
                                        renames: &mut engine_renames,
                                        bar: &mut engine_bar,
                                    },
                                    &mut raw_output_tap,
                                    buffer,
                                    length,
                                    &recycle_tx,
                                );
                                closed_tap = closed_tap.or(closed);
                                vt_diagnostics.record(parsed, started);
                                consumed_output |= parsed > 0;
                            }
                        });
                    if let Some(token) = closed_tap {
                        publisher.raw_output_tap_closed(token)?;
                    }
                    output_pending |= consumed_output;
                }
                ReaderMessage::Eof => reader_eof = true,
            },
            Wake::ChildExit(status) => {
                exit_status = Some(status?);
                #[cfg(windows)]
                if let Some(master) = master.take() {
                    let _ = master_close_tx.send(master);
                }
            }
            Wake::Deadline => {
                let started = diagnostic_timer();
                let parsed = drain_raw_output_parse_backlog(
                    &mut terminal,
                    &mut passthrough,
                    &mut EngineOutput {
                        filter: &mut engine_filter,
                        knobs: engine_knobs,
                        renames: &mut engine_renames,
                        bar: &mut engine_bar,
                    },
                    &mut raw_output_parse_backlog,
                    &mut raw_output_parse_backlog_bytes,
                    &mut raw_output_parse_buffer,
                );
                output_pending |= parsed > 0;
                vt_diagnostics.record(parsed, started);
            }
        }

        #[cfg(unix)]
        if input_permit.is_some() && writer.has_pending() {
            active_input_permit = input_permit.take();
        }
        #[cfg(not(unix))]
        drop(input_permit);

        if exit_status.is_some() && reader_eof && raw_output_parse_backlog.is_empty() {
            #[cfg(all(unix, not(target_os = "linux")))]
            let had_output = false;
            #[cfg(any(target_os = "linux", not(unix)))]
            let had_output = {
                let mut had_output = false;
                while let Ok(ReaderMessage::Data { buffer, length }) = output_rx.try_recv() {
                    let (closed, parsed) = consume_pty_output(
                        &mut terminal,
                        &mut passthrough,
                        &mut EngineOutput {
                            filter: &mut engine_filter,
                            knobs: engine_knobs,
                            renames: &mut engine_renames,
                            bar: &mut engine_bar,
                        },
                        &mut raw_output_tap,
                        buffer,
                        length,
                        &recycle_tx,
                    );
                    if let Some(token) = closed {
                        publisher.raw_output_tap_closed(token)?;
                    }
                    had_output |= parsed > 0;
                }
                had_output
            };
            #[cfg(unix)]
            drain_effects_if_writer_ready(&effects, &mut writer)?;
            #[cfg(not(unix))]
            drain_effects(&effects, &mut writer)?;
            if (had_output || output_pending)
                && let Some((token, number)) = pasted_image_bindings.observe(&terminal)?
            {
                publisher.placeholder_bound(token, number)?;
            }
            let output_screen = terminal.active_screen()?;
            if had_output {
                for view in inactive_views.values_mut() {
                    view.note_output(output_screen);
                }
            }
            for (view_id, view) in &mut active_views {
                if had_output {
                    note_output_and_revalidate_image_hover(
                        &mut terminal,
                        view,
                        output_screen,
                        &word_separators,
                        pasted_image_bindings.bound_numbers(),
                    )?;
                } else {
                    reconcile_view_screen(&mut terminal, view, &word_separators)?;
                }
                search_worker.cancel(*view_id);
                complete_view_search(&mut terminal, view)?;
            }
            let status = exit_status.take().expect("checked above");
            let signal = status.signal().and_then(signal_number);
            publisher.set_completion(TerminalProcessExit {
                code: status.exit_code(),
                signal,
            });
            if had_output || output_pending {
                publisher.mark_output_activity();
            }
            publish_active_views(
                &mut terminal,
                publisher,
                &mut render_state,
                &mut row_iterator,
                &mut cell_iterator,
                &mut generations,
                SnapshotChange::Content,
                &mut dictionary,
                &mut active_views,
                &word_separators,
                SessionStatus::exited(status.exit_code(), status.signal().map(str::to_owned)),
            )?;
            let notice_deadline = Instant::now() + DEAD_NOTICE_WAIT;
            let mut retained = false;
            while let Some(remaining) = notice_deadline.checked_duration_since(Instant::now()) {
                match control_rx.recv_timeout(remaining) {
                    Ok(Command::WriteDeadNotice(text)) => {
                        if let Some(text) = text {
                            retained = true;
                            write_dead_notice(&mut terminal, &text)?;
                            publish_active_views(
                                &mut terminal,
                                publisher,
                                &mut render_state,
                                &mut row_iterator,
                                &mut cell_iterator,
                                &mut generations,
                                SnapshotChange::Content,
                                &mut dictionary,
                                &mut active_views,
                                &word_separators,
                                SessionStatus::exited(
                                    status.exit_code(),
                                    status.signal().map(str::to_owned),
                                ),
                            )?;
                        }
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            if retained {
                match ModeRevision::capture(&mut terminal) {
                    Ok(revision) => publisher.publish_frozen_history(revision),
                    Err(error) => {
                        log::warn!("retained pane kept no frozen history: {error}");
                    }
                }
            }
            return Ok(());
        }
    }
}

/// `server_destroy_pane` resets the scroll region, parks the cursor on the
/// last row, linefeeds once so the screen scrolls, draws the expanded
/// `remain-on-exit-format` clipped to the pane width rather than wrapped, and
/// then hides the cursor; an empty template only hides the cursor. The text
/// arrives with the daemon's SGR runs for its `#[...]` styles, which take no
/// width and pass through the clip.
fn write_dead_notice(terminal: &mut Terminal<'_, '_>, text: &str) -> Result<(), WorkerError> {
    if text.is_empty() {
        terminal.vt_write(b"\x1b[?25l");
        return Ok(());
    }
    let columns = usize::from(terminal.cols()?);
    let rows = terminal.rows()?;
    let mut drawn = String::with_capacity(text.len());
    let mut width = 0_usize;
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '\x1b' {
            drawn.push(character);
            for control in characters.by_ref() {
                drawn.push(control);
                if control == 'm' {
                    break;
                }
            }
            continue;
        }
        let advance = usize::from(libghostty_vt::unicode::codepoint_width(character));
        if width + advance > columns {
            break;
        }
        width += advance;
        drawn.push(character);
    }
    terminal.vt_write(format!("\x1b[r\x1b[{rows};1H\n").as_bytes());
    terminal.vt_write(drawn.as_bytes());
    terminal.vt_write(b"\x1b[0m\x1b[?25l");
    Ok(())
}

#[cfg(unix)]
fn signal_number(description: &str) -> Option<u8> {
    if let Some(number) = description
        .strip_prefix("Signal ")
        .and_then(|number| number.parse().ok())
        .or_else(|| {
            description
                .rsplit_once(':')
                .and_then(|(_, number)| number.trim().parse().ok())
        })
    {
        return Some(number);
    }
    match description {
        "Hangup" => Some(1),
        "Interrupt" => Some(2),
        "Quit" => Some(3),
        "Illegal instruction" => Some(4),
        "Trace/breakpoint trap" | "Trace trap" => Some(5),
        "Aborted" | "Abort trap" => Some(6),
        "Bus error" => Some(7),
        "Floating point exception" => Some(8),
        "Killed" => Some(9),
        "Segmentation fault" => Some(11),
        "Broken pipe" => Some(13),
        "Alarm clock" => Some(14),
        "Terminated" => Some(15),
        _ => None,
    }
}

#[cfg(not(unix))]
fn signal_number(_description: &str) -> Option<u8> {
    None
}

#[cfg(unix)]
fn signal_terminal_process_groups(
    master: &parking_lot::Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    shell_process_id: Option<u32>,
    signal: rustix::process::Signal,
) {
    let mut groups = SmallVec::<[rustix::process::Pid; 2]>::new();
    if let Some(group) = master
        .lock()
        .process_group_leader()
        .and_then(|group| u32::try_from(group).ok())
        .filter(|group| *group != 0)
        .and_then(|group| i32::try_from(group).ok())
        .and_then(rustix::process::Pid::from_raw)
    {
        groups.push(group);
    }
    if let Some(group) = shell_process_id
        .filter(|group| *group != 0)
        .and_then(|group| i32::try_from(group).ok())
        .and_then(rustix::process::Pid::from_raw)
        && !groups.contains(&group)
    {
        groups.push(group);
    }
    for group in groups {
        let _ = rustix::process::kill_process_group(group, signal);
    }
}

enum ViewActionResult {
    None,
    Snapshot,
    OverlaySnapshot,
    ContentSnapshot,
    Copy(Box<TerminalCopyReady>),
    OpenUri(String),
}

fn sync_viewport_anchor(
    terminal: &Terminal<'_, '_>,
    view: &mut TerminalViewState,
) -> Result<(), WorkerError> {
    if view.copy_mode.is_some() {
        return Ok(());
    }
    let scrollbar = terminal.scrollbar()?;
    if scrollbar.offset.saturating_add(scrollbar.len) >= scrollbar.total {
        view.viewport = ViewportAnchor::FollowBottom;
        view.unseen_output = 0;
    } else {
        view.viewport = ViewportAnchor::Pinned(
            terminal.track_grid_ref(Point::Viewport(PointCoordinate { x: 0, y: 0 }))?,
        );
    }
    Ok(())
}

fn scroll_to_offset(
    terminal: &mut Terminal<'_, '_>,
    copy_mode: &mut CopyModeSlot,
    unseen_output: &mut u32,
    target: u32,
) -> Result<(), WorkerError> {
    if let Some(mode) = copy_mode.as_mut() {
        mode.viewport_offset = target.min(mode.revision.maximum_offset());
        return Ok(());
    }

    let scrollbar = terminal.scrollbar()?;
    let target = u64::from(target).min(scrollbar.total.saturating_sub(scrollbar.len));
    let delta = i128::from(target).saturating_sub(i128::from(scrollbar.offset));
    terminal.scroll_viewport(ScrollViewport::Delta(saturating_isize_i128(delta)));
    let landed = terminal.scrollbar()?;
    if landed.offset.saturating_add(landed.len) >= landed.total {
        *unseen_output = 0;
    }
    Ok(())
}

fn install_view_selection(
    terminal: &Terminal<'_, '_>,
    view: &mut TerminalViewState,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    let result = install_view_selection_inner(terminal, view, word_separators);
    finish_view_selection_install(terminal, view, result)
}

fn finish_view_selection_install(
    terminal: &Terminal<'_, '_>,
    view: &mut TerminalViewState,
    result: Result<(), WorkerError>,
) -> Result<(), WorkerError> {
    if warn_and_skip_view_ghostty("selection reinstall", result)?.is_none() {
        view.selection = None;
        if let Err(error) = terminal.set_selection(None) {
            log::warn!("failed to clear invalid terminal selection: {error}");
        }
    }
    Ok(())
}

fn install_view_selection_inner(
    terminal: &Terminal<'_, '_>,
    view: &mut TerminalViewState,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    let Some(state) = view.selection.as_ref() else {
        terminal.set_selection(None)?;
        return Ok(());
    };
    let Some(anchor) = state.anchor.snapshot(terminal)? else {
        view.selection = None;
        terminal.set_selection(None)?;
        return Ok(());
    };
    let Some(focus) = state.focus.snapshot(terminal)? else {
        view.selection = None;
        terminal.set_selection(None)?;
        return Ok(());
    };
    let cell_selection_has_extent = if state.mode == SelectionMode::Cell {
        terminal
            .point_from_grid_ref(&anchor, PointSpace::Screen)?
            .zip(terminal.point_from_grid_ref(&focus, PointSpace::Screen)?)
            .is_some_and(|(anchor, focus)| anchor != focus)
    } else {
        true
    };
    let selection = match state.mode {
        SelectionMode::Cell => {
            cell_selection_has_extent.then(|| Selection::new(anchor, focus, state.rectangle))
        }
        SelectionMode::Word => {
            let start = select_native_word_between(terminal, &anchor, &focus, word_separators)?;
            let end = select_native_word_between(terminal, &focus, &anchor, word_separators)?;
            start
                .zip(end)
                .map(|(start, end)| Selection::new(start.start(), end.end(), false))
        }
        SelectionMode::Line => {
            let start = terminal.select_line(SelectLineOptions::new(anchor))?;
            let end = terminal.select_line(SelectLineOptions::new(focus))?;
            start
                .zip(end)
                .map(|(start, end)| Selection::new(start.start(), end.end(), false))
        }
    };
    terminal.set_selection(selection.as_ref())?;
    Ok(())
}

fn restore_view_state(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    if warn_and_skip_view_ghostty(
        "view-state restore",
        restore_view_state_inner(terminal, view, word_separators),
    )?
    .is_none()
    {
        view.selection = None;
        view.viewport = ViewportAnchor::FollowBottom;
        terminal.scroll_viewport(ScrollViewport::Bottom);
        if let Err(error) = terminal.set_selection(None) {
            log::warn!("failed to clear selection after view-state restore error: {error}");
        }
    }
    Ok(())
}

fn restore_view_state_inner(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    if view.copy_mode.is_some() {
        terminal.set_selection(None)?;
        return Ok(());
    }
    match &view.viewport {
        ViewportAnchor::FollowBottom => terminal.scroll_viewport(ScrollViewport::Bottom),
        ViewportAnchor::Pinned(anchor) => {
            if let Some(point) = anchor.point(PointSpace::Screen)? {
                terminal.scroll_viewport(ScrollViewport::Top);
                terminal
                    .scroll_viewport(ScrollViewport::Delta(saturating_isize(i64::from(point.y))));
            } else {
                view.viewport = ViewportAnchor::FollowBottom;
                terminal.scroll_viewport(ScrollViewport::Bottom);
            }
        }
    }
    install_view_selection(terminal, view, word_separators)
}

fn reconcile_view_screen(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
    word_separators: &WordSeparators,
) -> Result<bool, WorkerError> {
    Ok(warn_and_skip_view_ghostty(
        "view-screen reconciliation",
        reconcile_view_screen_inner(terminal, view, word_separators),
    )?
    .unwrap_or(false))
}

fn note_output_and_revalidate_image_hover(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
    output_screen: Screen,
    word_separators: &WordSeparators,
    bound_pasted_images: &HashSet<u32>,
) -> Result<(), WorkerError> {
    let previous_screen = view.screen;
    let previous_hover = view
        .active()
        .hover_link
        .as_ref()
        .filter(|link| pasted_image_number(&link.uri).is_some())
        .cloned();
    view.note_output(output_screen);
    reconcile_view_screen(terminal, view, word_separators)?;
    let Some(previous_hover) = previous_hover.filter(|_| view.screen == previous_screen) else {
        return Ok(());
    };
    restore_view_state(terminal, view, word_separators)?;
    let current = image_placeholder_at(
        terminal,
        PointerCellEvent {
            column: previous_hover.start,
            row: previous_hover.row,
            click_count: 1,
            rectangle: false,
        },
        terminal.cols()?,
        bound_pasted_images,
    )?;
    if current.as_ref() == Some(&previous_hover) {
        view.active_mut().hover_link = current;
    }
    Ok(())
}

fn reconcile_view_screen_inner(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
    word_separators: &WordSeparators,
) -> Result<bool, WorkerError> {
    if view.copy_mode.is_some() {
        return Ok(false);
    }
    let screen = terminal.active_screen()?;
    if !view.switch_screen(screen) {
        return Ok(false);
    }
    terminal.set_selection(None)?;
    restore_view_state(terminal, view, word_separators)?;
    Ok(true)
}

fn activate_view(
    terminal: &mut Terminal<'_, '_>,
    view_id: TerminalViewId,
    active: &mut ActiveTerminalViews,
    inactive: &mut InactiveTerminalViews,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    if active.contains_key(&view_id) {
        return Ok(());
    }

    let screen = terminal.active_screen()?;
    let mut state = inactive
        .remove(&view_id)
        .unwrap_or_else(|| Box::new(TerminalViewState::for_screen(screen)));
    state.switch_screen(screen);
    restore_view_state(terminal, &mut state, word_separators)?;
    refresh_frozen_view_appearance(terminal, &mut state)?;
    active.insert(view_id, state);
    Ok(())
}

fn deactivate_view(
    terminal: &mut Terminal<'_, '_>,
    view_id: TerminalViewId,
    active: &mut ActiveTerminalViews,
    inactive: &mut InactiveTerminalViews,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    let Some(mut state) = active.remove(&view_id) else {
        return Ok(());
    };
    restore_view_state(terminal, &mut state, word_separators)?;
    sync_viewport_anchor(terminal, &mut state)?;
    terminal.set_selection(None)?;
    terminal.scroll_viewport(ScrollViewport::Bottom);
    inactive.insert(view_id, state);
    Ok(())
}

fn release_view(
    terminal: &mut Terminal<'_, '_>,
    view_id: TerminalViewId,
    active: &mut ActiveTerminalViews,
    inactive: &mut InactiveTerminalViews,
) -> Result<(), WorkerError> {
    if active.remove(&view_id).is_some() {
        terminal.set_selection(None)?;
        terminal.scroll_viewport(ScrollViewport::Bottom);
    }
    inactive.remove(&view_id);
    Ok(())
}

fn prepare_live_input(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
) -> Result<bool, WorkerError> {
    if view.copy_mode.is_some() {
        return Ok(false);
    }

    let scrollbar = terminal.scrollbar()?;
    let mut changed = scrollbar.offset.saturating_add(scrollbar.len) < scrollbar.total;
    if changed {
        terminal.scroll_viewport(ScrollViewport::Bottom);
    }
    if view.selection.take().is_some() {
        terminal.set_selection(None)?;
        changed = true;
    }
    if view.search.take().is_some() {
        changed = true;
    }
    view.search_origin = None;
    if view.hover_link.take().is_some() {
        changed = true;
    }
    view.search_snapshot = None;
    view.viewport = ViewportAnchor::FollowBottom;
    view.unseen_output = 0;
    view.mouse_button_pressed = false;
    Ok(changed)
}

fn apply_view_action(
    terminal: &mut Terminal<'_, '_>,
    view_id: TerminalViewId,
    view: &mut TerminalViewState,
    action: TerminalViewAction,
    geometry: Geometry,
    writer: &mut dyn Write,
    mouse_encoder: &mut mouse::Encoder<'_>,
    mouse_event: &mut mouse::Event<'_>,
    input_bytes: &mut Vec<u8>,
    search_worker: &mut SearchWorker,
    wrap_search: bool,
    mode_keys_vi: bool,
    word_separators: &WordSeparators,
    bound_pasted_images: &HashSet<u32>,
    pending_copy_source: &mut Option<Box<CapturedCopySource>>,
) -> Result<ViewActionResult, WorkerError> {
    let view_screen = view.screen;
    let TerminalScreenViewState {
        viewport: _,
        selection,
        copy_mode,
        search,
        search_origin,
        search_snapshot,
        hover_link,
        unseen_output,
        mouse_button_pressed,
    } = view.active_mut();
    let hover_cleared = !matches!(
        &action,
        TerminalViewAction::Mouse(_) | TerminalViewAction::CopySelection { .. }
    ) && hover_link.take().is_some();
    let result = match action {
        TerminalViewAction::ScrollWheel { lines, input } => {
            if let Some(mode) = copy_mode.as_mut() {
                scroll_copy_mode(mode, i64::from(lines));
                Ok(ViewActionResult::Snapshot)
            } else {
                let route = wheel_route(terminal, input.force_selection())?;
                match route {
                    WheelRoute::ApplicationMouse => {
                        let selection_changed = selection.take().is_some();
                        if selection_changed {
                            terminal.set_selection(None)?;
                        }
                        for _ in 0..lines.unsigned_abs().min(MAX_WHEEL_REPEAT) {
                            let _ = route_mouse_input(
                                terminal,
                                selection,
                                hover_link,
                                None,
                                input,
                                writer,
                                mouse_encoder,
                                mouse_event,
                                mouse_button_pressed,
                                input_bytes,
                                word_separators,
                                bound_pasted_images,
                            )?;
                        }
                        Ok(if selection_changed {
                            ViewActionResult::OverlaySnapshot
                        } else {
                            ViewActionResult::None
                        })
                    }
                    WheelRoute::AlternateScroll => {
                        let selection_changed = selection.take().is_some();
                        terminal.set_selection(None)?;
                        write_alternate_scroll(terminal, lines, writer)?;
                        Ok(if selection_changed {
                            ViewActionResult::OverlaySnapshot
                        } else {
                            ViewActionResult::None
                        })
                    }
                    WheelRoute::Viewport => {
                        let scrollbar = terminal.scrollbar()?;
                        if scrollbar.total > scrollbar.len {
                            terminal.scroll_viewport(ScrollViewport::Delta(saturating_isize(
                                i64::from(lines),
                            )));
                            Ok(ViewActionResult::Snapshot)
                        } else {
                            Ok(ViewActionResult::None)
                        }
                    }
                }
            }
        }
        TerminalViewAction::ScrollLines(lines) => {
            if let Some(mode) = copy_mode.as_mut() {
                scroll_copy_mode(mode, i64::from(lines));
            } else {
                terminal.scroll_viewport(ScrollViewport::Delta(saturating_isize(i64::from(lines))));
            }
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::ScrollPages(pages) => {
            let rows = copy_mode.as_ref().map_or_else(
                || i64::from(geometry.rows),
                |mode| i64::from(mode.revision.viewport_rows),
            );
            let delta = i64::from(pages).saturating_mul(rows);
            if let Some(mode) = copy_mode.as_mut() {
                scroll_copy_mode(mode, delta);
            } else {
                terminal.scroll_viewport(ScrollViewport::Delta(saturating_isize(delta)));
            }
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::ScrollTop => {
            if let Some(mode) = copy_mode.as_mut() {
                mode.viewport_offset = 0;
            } else {
                terminal.scroll_viewport(ScrollViewport::Top);
            }
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::ScrollBottom => {
            if let Some(mode) = copy_mode.as_mut() {
                mode.viewport_offset = mode.revision.maximum_offset();
            } else {
                terminal.scroll_viewport(ScrollViewport::Bottom);
                *unseen_output = 0;
            }
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::ScrollToFraction(fraction) => {
            if let Some(mode) = copy_mode.as_mut() {
                mode.viewport_offset = u32::try_from(
                    u128::from(mode.revision.maximum_offset()).saturating_mul(u128::from(fraction))
                        / u128::from(u32::MAX),
                )
                .unwrap_or(mode.revision.maximum_offset());
                return Ok(ViewActionResult::Snapshot);
            }
            let scrollbar = terminal.scrollbar()?;
            let maximum = scrollbar.total.saturating_sub(scrollbar.len);
            let target = u64::try_from(
                u128::from(maximum).saturating_mul(u128::from(fraction)) / u128::from(u32::MAX),
            )
            .unwrap_or(maximum);
            let delta = i128::from(target).saturating_sub(i128::from(scrollbar.offset));
            terminal.scroll_viewport(ScrollViewport::Delta(saturating_isize_i128(delta)));
            if target == maximum {
                *unseen_output = 0;
            }
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::ScrollToOffset(target) => {
            scroll_to_offset(terminal, copy_mode, unseen_output, target)?;
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::SelectionPress(event) => {
            if let Some(mode) = copy_mode.as_mut() {
                mode_selection_press(mode, event, word_separators);
            } else {
                selection_press(terminal, selection, event, word_separators)?;
            }
            Ok(ViewActionResult::OverlaySnapshot)
        }
        TerminalViewAction::SelectionDrag(event) => {
            if let Some(mode) = copy_mode.as_mut() {
                mode_selection_drag(mode, event, word_separators);
            } else {
                selection_drag(terminal, selection, event, word_separators)?;
            }
            Ok(ViewActionResult::OverlaySnapshot)
        }
        TerminalViewAction::SelectionAutoscroll { lines, pointer } => {
            if let Some(mode) = copy_mode.as_mut() {
                scroll_copy_mode(mode, i64::from(lines));
                mode_selection_drag(mode, pointer, word_separators);
            } else {
                terminal.scroll_viewport(ScrollViewport::Delta(saturating_isize(i64::from(lines))));
                selection_drag(terminal, selection, pointer, word_separators)?;
            }
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::SelectionRelease(_) | TerminalViewAction::ClearLinkHover => {
            Ok(ViewActionResult::None)
        }
        TerminalViewAction::Mouse(input) => route_mouse_input(
            terminal,
            selection,
            hover_link,
            copy_mode.as_deref_mut(),
            input,
            writer,
            mouse_encoder,
            mouse_event,
            mouse_button_pressed,
            input_bytes,
            word_separators,
            bound_pasted_images,
        ),
        TerminalViewAction::SelectAll => {
            if let Some(mode) = copy_mode.as_mut() {
                mode.selection = Some(ModeSelection {
                    anchor: PointCoordinate { x: 0, y: 0 },
                    focus: PointCoordinate {
                        x: mode.revision.columns.saturating_sub(1),
                        y: mode.revision.total_rows().saturating_sub(1),
                    },
                    mode: SelectionMode::Cell,
                    rectangle: false,
                });
                mode.selecting = true;
            } else {
                select_all_history(terminal, selection)?;
            }
            Ok(ViewActionResult::OverlaySnapshot)
        }
        TerminalViewAction::ClearHistory => {
            search_worker.cancel(view_id);
            terminal.vt_write(b"\x1b[3J");
            *selection = None;
            *copy_mode = None;
            *search = None;
            *search_origin = None;
            *search_snapshot = None;
            *hover_link = None;
            *unseen_output = 0;
            terminal.set_selection(None)?;
            terminal.scroll_viewport(ScrollViewport::Bottom);
            Ok(ViewActionResult::ContentSnapshot)
        }
        TerminalViewAction::ClearSelection => {
            if let Some(mode) = copy_mode.as_mut() {
                mode.selection = None;
                mode.selecting = false;
            } else {
                *selection = None;
                terminal.set_selection(None)?;
            }
            Ok(ViewActionResult::OverlaySnapshot)
        }
        enter_action @ (TerminalViewAction::EnterCopyMode
        | TerminalViewAction::EnterCopyModeScrollExit
        | TerminalViewAction::EnterCopyModeWith { .. }) => {
            if copy_mode.is_none() {
                drop_view_search(
                    view_id,
                    search_worker,
                    search,
                    search_origin,
                    search_snapshot,
                );
            }
            enter_copy_mode(
                terminal,
                selection,
                copy_mode,
                matches!(
                    enter_action,
                    TerminalViewAction::EnterCopyModeScrollExit
                        | TerminalViewAction::EnterCopyModeWith {
                            scroll_exit: true,
                            ..
                        }
                ),
                matches!(
                    enter_action,
                    TerminalViewAction::EnterCopyModeWith {
                        hide_position: true,
                        ..
                    }
                ),
                pending_copy_source.take(),
                mode_keys_vi,
            )?;
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::CopyModeCounted { action, count } => {
            let was_frozen = copy_mode.is_some();
            let result = if let CopyModeAction::Search(spec) = &action {
                run_copy_mode_search(
                    view_id,
                    copy_mode,
                    search,
                    search_snapshot,
                    search_worker,
                    spec,
                    count,
                    mode_keys_vi,
                    wrap_search,
                );
                ViewActionResult::Snapshot
            } else if let CopyModeAction::SearchAgain { reverse } = action {
                if let Some(spec) = copy_mode_search_again(copy_mode.as_deref(), reverse) {
                    run_copy_mode_search(
                        view_id,
                        copy_mode,
                        search,
                        search_snapshot,
                        search_worker,
                        &spec,
                        count,
                        mode_keys_vi,
                        wrap_search,
                    );
                } else {
                    let forward = search
                        .as_ref()
                        .is_none_or(|search| search.query.direction == SearchDirection::Forward);
                    let direction = if forward ^ reverse { 1 } else { -1 };
                    for _ in 0..count {
                        step_search(search, direction, wrap_search);
                    }
                    sync_copy_cursor_to_search(copy_mode, search.as_deref());
                    reveal_search_for_view(terminal, copy_mode.as_deref(), search.as_deref())?;
                }
                ViewActionResult::Snapshot
            } else {
                apply_counted_copy_mode_action(
                    terminal,
                    selection,
                    copy_mode,
                    unseen_output,
                    action,
                    count,
                    word_separators,
                    mode_keys_vi,
                )?
            };
            if was_frozen && copy_mode.is_none() {
                drop_view_search(
                    view_id,
                    search_worker,
                    search,
                    search_origin,
                    search_snapshot,
                );
            }
            Ok(result)
        }
        TerminalViewAction::CopyMode(CopyModeAction::Search(spec)) => {
            run_copy_mode_search(
                view_id,
                copy_mode,
                search,
                search_snapshot,
                search_worker,
                &spec,
                1,
                mode_keys_vi,
                wrap_search,
            );
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::CopyMode(CopyModeAction::SearchAgain { reverse }) => {
            if let Some(spec) = copy_mode_search_again(copy_mode.as_deref(), reverse) {
                run_copy_mode_search(
                    view_id,
                    copy_mode,
                    search,
                    search_snapshot,
                    search_worker,
                    &spec,
                    1,
                    mode_keys_vi,
                    wrap_search,
                );
            } else {
                let forward = search
                    .as_ref()
                    .is_none_or(|search| search.query.direction == SearchDirection::Forward);
                let direction = if forward ^ reverse { 1 } else { -1 };
                step_search(search, direction, wrap_search);
                sync_copy_cursor_to_search(copy_mode, search.as_deref());
                reveal_search_for_view(terminal, copy_mode.as_deref(), search.as_deref())?;
            }
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::CopyMode(CopyModeAction::SearchCursorWord { direction }) => {
            let text = copy_mode
                .as_deref()
                .and_then(|mode| mode_cursor_word(&mode.revision, mode.cursor, word_separators));
            if let Some(text) = text {
                run_copy_mode_search(
                    view_id,
                    copy_mode,
                    search,
                    search_snapshot,
                    search_worker,
                    &CopyModeSearch {
                        text,
                        direction,
                        regex: false,
                        incremental: false,
                    },
                    1,
                    mode_keys_vi,
                    wrap_search,
                );
                Ok(ViewActionResult::Snapshot)
            } else {
                Ok(ViewActionResult::None)
            }
        }
        TerminalViewAction::CopyMode(action) => {
            let was_frozen = copy_mode.is_some();
            let result = apply_copy_mode_action(
                terminal,
                selection,
                copy_mode,
                unseen_output,
                action,
                word_separators,
                mode_keys_vi,
            )?;
            if was_frozen && copy_mode.is_none() {
                drop_view_search(
                    view_id,
                    search_worker,
                    search,
                    search_origin,
                    search_snapshot,
                );
            }
            Ok(result)
        }
        TerminalViewAction::SearchBegin(query) => {
            let selection =
                search_selection_policy(copy_mode.as_deref(), search_origin, query.direction, true);
            if search_snapshot.is_none()
                && let Some(mode) = copy_mode.as_ref()
            {
                *search_snapshot = Some(Arc::clone(&mode.revision.search));
            }
            queue_search(
                terminal,
                SearchTarget {
                    view_id,
                    screen: view_screen,
                    search,
                    snapshot: search_snapshot,
                },
                search_worker,
                query,
                selection,
            )?;
            Ok(ViewActionResult::OverlaySnapshot)
        }
        TerminalViewAction::SearchUpdate(query) => {
            let selection = search_selection_policy(
                copy_mode.as_deref(),
                search_origin,
                query.direction,
                false,
            );
            if search_snapshot.is_none()
                && let Some(mode) = copy_mode.as_ref()
            {
                *search_snapshot = Some(Arc::clone(&mode.revision.search));
            }
            queue_search(
                terminal,
                SearchTarget {
                    view_id,
                    screen: view_screen,
                    search,
                    snapshot: search_snapshot,
                },
                search_worker,
                query,
                selection,
            )?;
            Ok(ViewActionResult::OverlaySnapshot)
        }
        TerminalViewAction::SearchNext => {
            step_search(search, 1, wrap_search);
            sync_copy_cursor_to_search(copy_mode, search.as_deref());
            reveal_search_for_view(terminal, copy_mode.as_deref(), search.as_deref())?;
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::SearchPrevious => {
            step_search(search, -1, wrap_search);
            sync_copy_cursor_to_search(copy_mode, search.as_deref());
            reveal_search_for_view(terminal, copy_mode.as_deref(), search.as_deref())?;
            Ok(ViewActionResult::Snapshot)
        }
        TerminalViewAction::SearchClose => {
            search_worker.cancel(view_id);
            *search = None;
            *search_origin = None;
            *search_snapshot = None;
            Ok(ViewActionResult::OverlaySnapshot)
        }
        TerminalViewAction::CopySelection { request_id, target } => {
            let text = copy_mode.as_ref().map_or_else(
                || format_selection_text(terminal),
                |mode| {
                    Ok(mode.selection.map_or_else(String::new, |selection| {
                        mode.revision.format_selection(selection, mode_keys_vi)
                    }))
                },
            )?;
            Ok(ViewActionResult::Copy(Box::new(TerminalCopyReady {
                request_id,
                clipboard: Some(target),
                buffer: None,
                pipe: None,
                text,
                view_changed: false,
            })))
        }
        TerminalViewAction::Focus(focused) => {
            write_focus_event(terminal, focused, writer)?;
            Ok(ViewActionResult::None)
        }
        TerminalViewAction::Paste(text) => {
            write_paste_bytes(terminal, text.into_bytes(), true, writer)?;
            Ok(ViewActionResult::None)
        }
    };
    let result = match warn_and_skip_view_ghostty("view action", result)? {
        Some(result) => result,
        None if hover_cleared => return Ok(ViewActionResult::OverlaySnapshot),
        None => return Ok(ViewActionResult::None),
    };
    Ok(
        if hover_cleared && matches!(result, ViewActionResult::None) {
            ViewActionResult::OverlaySnapshot
        } else {
            result
        },
    )
}

fn write_paste_bytes(
    terminal: &Terminal<'_, '_>,
    mut source: Vec<u8>,
    bracketed: bool,
    writer: &mut dyn Write,
) -> Result<(), WorkerError> {
    if source.is_empty() {
        return Ok(());
    }
    let mut encoded = vec![0_u8; source.len().saturating_add(16)];
    let bracketed = bracketed && terminal.mode(Mode::BRACKETED_PASTE)?;
    let written = libghostty_vt::paste::encode(&mut source, bracketed, &mut encoded)?;
    writer.write_all(&encoded[..written])?;
    writer.flush()?;
    Ok(())
}

fn write_prepared_paste_bytes(
    terminal: &Terminal<'_, '_>,
    source: &[u8],
    bracketed: bool,
    writer: &mut dyn Write,
) -> Result<(), WorkerError> {
    let bracketed = bracketed && terminal.mode(Mode::BRACKETED_PASTE)?;
    if source.is_empty() && !bracketed {
        return Ok(());
    }
    if bracketed {
        writer.write_all(b"\x1b[200~")?;
    }
    writer.write_all(source)?;
    if bracketed {
        writer.write_all(b"\x1b[201~")?;
    }
    writer.flush()?;
    Ok(())
}

fn scroll_copy_mode(mode: &mut CopyModeState, delta: i64) {
    let next = i128::from(mode.viewport_offset).saturating_add(i128::from(delta));
    mode.viewport_offset = u32::try_from(next.max(0))
        .unwrap_or(u32::MAX)
        .min(mode.revision.maximum_offset());
}

fn mode_pointer_point(mode: &CopyModeState, event: PointerCellEvent) -> PointCoordinate {
    mode.revision.clamp_point(PointCoordinate {
        x: event.column,
        y: mode.viewport_offset.saturating_add(u32::from(event.row)),
    })
}

fn mode_selection_press(
    mode: &mut CopyModeState,
    event: PointerCellEvent,
    word_separators: &WordSeparators,
) {
    let point = mode_pointer_point(mode, event);
    mode.cursor = point;
    mode.rectangle = event.rectangle;
    let selection_mode = match event.click_count {
        0 | 1 => SelectionMode::Cell,
        2 => SelectionMode::Word,
        _ => SelectionMode::Line,
    };
    let (anchor, focus) = match selection_mode {
        SelectionMode::Cell => (point, point),
        SelectionMode::Word => mode_word_bounds(&mode.revision, point, word_separators),
        SelectionMode::Line => (
            PointCoordinate { x: 0, y: point.y },
            PointCoordinate {
                x: mode.revision.columns.saturating_sub(1),
                y: point.y,
            },
        ),
    };
    mode.selection = Some(ModeSelection {
        anchor,
        focus,
        mode: selection_mode,
        rectangle: mode.rectangle,
    });
    mode.selecting = true;
}

fn mode_selection_drag(
    mode: &mut CopyModeState,
    event: PointerCellEvent,
    word_separators: &WordSeparators,
) {
    let point = mode_pointer_point(mode, event);
    mode.cursor = point;
    if let Some(selection) = mode.selection.as_mut() {
        selection.focus = match selection.mode {
            SelectionMode::Cell => point,
            SelectionMode::Word => {
                let (start, end) = mode_word_bounds(&mode.revision, point, word_separators);
                if (point.y, point.x) < (selection.anchor.y, selection.anchor.x) {
                    start
                } else {
                    end
                }
            }
            SelectionMode::Line => PointCoordinate {
                x: mode.revision.columns.saturating_sub(1),
                y: point.y,
            },
        };
        mode.rectangle |= event.rectangle;
        selection.rectangle = mode.rectangle;
    }
}

/// The end of the word at or after `point`, or the start of the word at or
/// before it. A cursor parked in whitespace resolves outward, the way the pin's
/// `window_copy_cursor_next_word_end_pos` and `..._previous_word_pos` do.
fn mode_word_edge(
    revision: &ModeRevision,
    point: PointCoordinate,
    word_separators: &WordSeparators,
    forward: bool,
) -> PointCoordinate {
    let mut at = point;
    while revision_cell_is_whitespace(revision, at) {
        if forward {
            if at.x.saturating_add(1) >= revision.columns {
                return point;
            }
            at.x += 1;
        } else {
            if at.x == 0 {
                return point;
            }
            at.x -= 1;
        }
    }
    let (start, end) = mode_word_bounds(revision, at, word_separators);
    if forward { end } else { start }
}

fn mode_word_bounds(
    revision: &ModeRevision,
    point: PointCoordinate,
    word_separators: &WordSeparators,
) -> (PointCoordinate, PointCoordinate) {
    let class = revision_word_class(revision, point, word_separators);
    let mut start = point.x;
    while start > 0
        && revision_word_class(
            revision,
            PointCoordinate {
                x: start - 1,
                y: point.y,
            },
            word_separators,
        ) == class
    {
        start -= 1;
    }
    let mut end = point.x;
    while end + 1 < revision.columns
        && revision_word_class(
            revision,
            PointCoordinate {
                x: end + 1,
                y: point.y,
            },
            word_separators,
        ) == class
    {
        end += 1;
    }
    (
        PointCoordinate {
            x: start,
            y: point.y,
        },
        PointCoordinate { x: end, y: point.y },
    )
}

fn mode_cursor_word(
    revision: &ModeRevision,
    point: PointCoordinate,
    word_separators: &WordSeparators,
) -> Option<String> {
    if revision_cell_is_whitespace(revision, point) {
        return None;
    }
    let (anchor, focus) = mode_word_bounds(revision, point, word_separators);
    let text = revision.format_selection(
        ModeSelection {
            anchor,
            focus,
            mode: SelectionMode::Word,
            rectangle: false,
        },
        true,
    );
    (!text.is_empty()).then_some(text)
}

/// `window_copy_init` reads its screen from `wme->swp`, so a `-s` entry clones
/// the source pane's grid and takes its cursor; `window_copy_clone_screen`
/// trims the trailing blank rows first and drops the cursor onto the last used
/// row whenever it sat past them.
fn capture_copy_source(terminal: &mut Terminal<'_, '_>) -> Result<CapturedCopySource, WorkerError> {
    let revision = ModeRevision::capture(terminal)?;
    let last_used = used_screen_rows(terminal).unwrap_or(0);
    let cursor_row = terminal.cursor_y()?;
    let (x, y) = if cursor_row + 1 > last_used {
        (0, last_used.saturating_sub(1))
    } else {
        (
            terminal.cursor_x()?.min(terminal.cols()?.saturating_sub(1)),
            cursor_row,
        )
    };
    let cursor = terminal.grid_ref(Point::Active(PointCoordinate { x, y: u32::from(y) }))?;
    let cursor = terminal
        .point_from_grid_ref(&cursor, PointSpace::Screen)?
        .unwrap_or(PointCoordinate { x, y: u32::from(y) });
    let viewport_offset = revision.maximum_offset();
    Ok(CapturedCopySource {
        revision,
        cursor,
        viewport_offset,
    })
}

fn enter_copy_mode(
    terminal: &mut Terminal<'_, '_>,
    selection: &mut Option<SelectionState>,
    copy_mode: &mut CopyModeSlot,
    scroll_exit: bool,
    hide_position: bool,
    source: Option<Box<CapturedCopySource>>,
    mode_keys_vi: bool,
) -> Result<(), WorkerError> {
    if copy_mode.is_some() {
        return Ok(());
    }
    terminal.scroll_viewport(ScrollViewport::Bottom);
    terminal.set_selection(None)?;
    *selection = None;
    let sourced = source.is_some();
    let (revision, cursor, viewport_offset) = if let Some(source) = source {
        let CapturedCopySource {
            revision,
            cursor,
            viewport_offset,
        } = *source;
        (revision, cursor, viewport_offset)
    } else {
        let x = terminal.cursor_x()?.min(terminal.cols()?.saturating_sub(1));
        let y = terminal.cursor_y()?;
        let cursor = terminal.grid_ref(Point::Active(PointCoordinate { x, y: u32::from(y) }))?;
        let cursor = terminal
            .point_from_grid_ref(&cursor, PointSpace::Screen)?
            .unwrap_or(PointCoordinate { x, y: u32::from(y) });
        let revision = ModeRevision::capture(terminal)?;
        let viewport_offset = u32::try_from(terminal.scrollbar()?.offset).unwrap_or(u32::MAX);
        (revision, cursor, viewport_offset)
    };
    *copy_mode = Some(Box::new(CopyModeState {
        revision,
        cursor,
        viewport_offset,
        scroll_exit,
        hide_position,
        selection: None,
        selection_mode: CopySelectionMode::Char,
        selection_origin: PointCoordinate { x: 0, y: 0 },
        recentre: None,
        mark: None,
        last_jump: None,
        selecting: false,
        rectangle: false,
        refresh: false,
        sourced,
        kind: FrozenModeKind::Copy,
        last_cx: 0,
        last_sx: 0,
        mode_keys_vi_at_entry: mode_keys_vi,
        search_marks: false,
        search_count: Some((0, false)),
        incremental_origin: None,
        search: None,
    }));
    Ok(())
}

fn drop_view_search(
    view_id: TerminalViewId,
    search_worker: &mut SearchWorker,
    search: &mut SearchSlot,
    search_origin: &mut Option<PointCoordinate>,
    search_snapshot: &mut Option<Arc<HistorySearchSnapshot>>,
) {
    search_worker.cancel(view_id);
    *search = None;
    *search_origin = None;
    *search_snapshot = None;
}

fn wheel_route(
    terminal: &Terminal<'_, '_>,
    force_selection: bool,
) -> Result<WheelRoute, WorkerError> {
    if force_selection {
        return Ok(WheelRoute::Viewport);
    }
    if terminal.is_mouse_tracking()? {
        return Ok(WheelRoute::ApplicationMouse);
    }
    if terminal.active_screen()? == Screen::Alternate && terminal.mode(Mode::ALT_SCROLL)? {
        return Ok(WheelRoute::AlternateScroll);
    }
    Ok(WheelRoute::Viewport)
}

fn write_alternate_scroll(
    terminal: &Terminal<'_, '_>,
    lines: i32,
    writer: &mut dyn Write,
) -> Result<(), WorkerError> {
    let sequence = if terminal.mode(Mode::DECCKM)? {
        if lines < 0 { b"\x1bOA" } else { b"\x1bOB" }
    } else if lines < 0 {
        b"\x1b[A"
    } else {
        b"\x1b[B"
    };
    let repetitions = usize::try_from(lines.unsigned_abs().min(MAX_WHEEL_REPEAT)).unwrap_or(0);
    let mut bytes = [0_u8; 96];
    let length = repetitions.saturating_mul(sequence.len());
    for chunk in bytes[..length].chunks_exact_mut(sequence.len()) {
        chunk.copy_from_slice(sequence);
    }
    if length != 0 {
        writer.write_all(&bytes[..length])?;
        writer.flush()?;
    }
    Ok(())
}

#[allow(
    clippy::cast_precision_loss,
    reason = "libghostty's mouse ABI uses f32 pixel positions"
)]
fn route_mouse_input(
    terminal: &mut Terminal<'_, '_>,
    selection: &mut Option<SelectionState>,
    hover_link: &mut Option<HoverLink>,
    copy_mode: Option<&mut CopyModeState>,
    input: TerminalMouseInput,
    writer: &mut dyn Write,
    encoder: &mut mouse::Encoder<'_>,
    event: &mut mouse::Event<'_>,
    button_pressed: &mut bool,
    input_bytes: &mut Vec<u8>,
    word_separators: &WordSeparators,
    bound_pasted_images: &HashSet<u32>,
) -> Result<ViewActionResult, WorkerError> {
    let phase = input.phase();
    let button = input.button();
    let modifiers = input.modifiers();
    let force_selection = input.force_selection();
    if let Some(mode) = copy_mode {
        *hover_link = None;
        if button != Some(TerminalMouseButton::Left) {
            return Ok(ViewActionResult::None);
        }
        match phase {
            TerminalMousePhase::Press => {
                mode_selection_press(mode, input.cell, word_separators);
            }
            TerminalMousePhase::Motion => {
                mode_selection_drag(mode, input.cell, word_separators);
            }
            TerminalMousePhase::Release => {}
        }
        return Ok(ViewActionResult::OverlaySnapshot);
    }
    if phase == TerminalMousePhase::Press
        && button == Some(TerminalMouseButton::Left)
        && input.cell.click_count >= 3
        && (modifiers.control() || modifiers.platform())
        && select_semantic_output_at(terminal, selection, input.cell)?
    {
        *hover_link = None;
        return Ok(ViewActionResult::OverlaySnapshot);
    }

    let link_modifier = modifiers.control() || modifiers.platform();
    let plain_image_motion = !force_selection
        && modifiers.bits() == 0
        && phase == TerminalMousePhase::Motion
        && button.is_none();
    let plain_image_hover_changed = if plain_image_motion {
        let columns = terminal.cols()?;
        let next_hover = image_placeholder_at(terminal, input.cell, columns, bound_pasted_images)?;
        let changed = *hover_link != next_hover;
        *hover_link = next_hover;
        Some(changed)
    } else {
        None
    };

    if !force_selection && terminal.is_mouse_tracking()? {
        let hover_changed =
            plain_image_hover_changed.unwrap_or_else(|| hover_link.take().is_some());
        let is_wheel = matches!(
            button,
            Some(TerminalMouseButton::ScrollUp | TerminalMouseButton::ScrollDown)
        );
        *button_pressed = match phase {
            TerminalMousePhase::Press if !is_wheel => true,
            TerminalMousePhase::Press | TerminalMousePhase::Release => false,
            TerminalMousePhase::Motion => *button_pressed,
        };
        let mut modifiers = key::Mods::empty();
        modifiers.set(key::Mods::SHIFT, input.modifiers().shift());
        modifiers.set(key::Mods::CTRL, input.modifiers().control());
        modifiers.set(key::Mods::ALT, input.modifiers().alt());
        modifiers.set(key::Mods::SUPER, input.modifiers().platform());
        event
            .set_action(match phase {
                TerminalMousePhase::Press => mouse::Action::Press,
                TerminalMousePhase::Release => mouse::Action::Release,
                TerminalMousePhase::Motion => mouse::Action::Motion,
            })
            .set_button(button.map(|button| match button {
                TerminalMouseButton::Left => mouse::Button::Left,
                TerminalMouseButton::Middle => mouse::Button::Middle,
                TerminalMouseButton::Right => mouse::Button::Right,
                TerminalMouseButton::ScrollUp => mouse::Button::Four,
                TerminalMouseButton::ScrollDown => mouse::Button::Five,
            }))
            .set_mods(modifiers)
            .set_position(mouse::Position {
                x: input.x as f32,
                y: input.y as f32,
            });
        encoder
            .set_options_from_terminal(terminal)
            .set_size(EncoderSize {
                screen_width: input.screen_width,
                screen_height: input.screen_height,
                cell_width: input.cell_width.max(1),
                cell_height: input.cell_height.max(1),
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            })
            .set_any_button_pressed(*button_pressed)
            .set_track_last_cell(true);
        input_bytes.clear();
        encoder.encode_to_vec(event, input_bytes)?;
        if !input_bytes.is_empty() {
            writer.write_all(input_bytes)?;
            writer.flush()?;
        }
        return Ok(if hover_changed {
            ViewActionResult::OverlaySnapshot
        } else {
            ViewActionResult::None
        });
    }

    if !force_selection
        && phase == TerminalMousePhase::Press
        && button == Some(TerminalMouseButton::Left)
        && (modifiers.control() || modifiers.platform())
    {
        if let Some(link) = hover_link.as_ref().filter(|link| {
            link.contains(input.cell) && hover_link_is_bound(link, bound_pasted_images)
        }) {
            return Ok(ViewActionResult::OpenUri(link.uri.clone()));
        }
        if let Some(link) = hover_link_at(terminal, input.cell, input_bytes, bound_pasted_images)? {
            let uri = link.uri.clone();
            *hover_link = Some(link);
            return Ok(ViewActionResult::OpenUri(uri));
        }
    }

    let keep_cached_hover = !plain_image_motion
        && link_modifier
        && phase == TerminalMousePhase::Motion
        && button.is_none()
        && hover_link
            .as_ref()
            .is_some_and(|link| link.contains(input.cell));
    let hover_changed = if let Some(changed) = plain_image_hover_changed {
        changed
    } else if keep_cached_hover {
        false
    } else {
        let next_hover = (link_modifier && phase == TerminalMousePhase::Motion && button.is_none())
            .then(|| hover_link_at(terminal, input.cell, input_bytes, bound_pasted_images))
            .transpose()?
            .flatten();
        let changed = *hover_link != next_hover;
        if phase == TerminalMousePhase::Motion {
            *hover_link = next_hover;
        } else if phase == TerminalMousePhase::Press {
            *hover_link = None;
        }
        changed
    };
    if button != Some(TerminalMouseButton::Left) {
        return Ok(if hover_changed {
            ViewActionResult::OverlaySnapshot
        } else {
            ViewActionResult::None
        });
    }
    match phase {
        TerminalMousePhase::Press => {
            selection_press(terminal, selection, input.cell, word_separators)?;
        }
        TerminalMousePhase::Motion => {
            selection_drag(terminal, selection, input.cell, word_separators)?;
        }
        TerminalMousePhase::Release => {}
    }
    Ok(ViewActionResult::OverlaySnapshot)
}

fn hover_link_at(
    terminal: &Terminal<'_, '_>,
    point: PointerCellEvent,
    scratch: &mut Vec<u8>,
    bound_pasted_images: &HashSet<u32>,
) -> Result<Option<HoverLink>, WorkerError> {
    let columns = terminal.cols()?;
    let rows = terminal.rows()?;
    if point.column >= columns || point.row >= rows {
        return Ok(None);
    }

    if let Some(length) = hyperlink_uri_bytes(terminal, point.row, point.column, scratch)? {
        let Ok(uri) = std::str::from_utf8(&scratch[..length]) else {
            return Ok(None);
        };
        if !is_safe_link_uri(uri) {
            return Ok(None);
        }
        let uri = uri.to_owned();

        let mut start = point.column;
        while start > 0
            && cell_has_hyperlink_uri(terminal, point.row, start - 1, uri.as_bytes(), scratch)?
        {
            start -= 1;
        }
        let mut end = point.column.saturating_add(1);
        while end < columns
            && cell_has_hyperlink_uri(terminal, point.row, end, uri.as_bytes(), scratch)?
        {
            end += 1;
        }

        return Ok(Some(HoverLink {
            row: point.row,
            start,
            end,
            uri,
        }));
    }

    if let Some(link) = image_placeholder_at(terminal, point, columns, bound_pasted_images)? {
        return Ok(Some(link));
    }

    plain_uri_at(terminal, point, columns, scratch)
}

fn image_placeholder_at(
    terminal: &Terminal<'_, '_>,
    point: PointerCellEvent,
    columns: u16,
    bound_pasted_images: &HashSet<u32>,
) -> Result<Option<HoverLink>, WorkerError> {
    let rows = terminal.rows()?;
    if point.column >= columns || point.row >= rows {
        return Ok(None);
    }
    let point_index = usize::from(point.column);
    let window_start = point_index.saturating_sub(MAX_PLACEHOLDER_CELLS - 1);
    let window_end = point_index
        .saturating_add(MAX_PLACEHOLDER_CELLS)
        .min(usize::from(columns));
    let mut cells = [b' '; 2 * MAX_PLACEHOLDER_CELLS - 1];
    for (cell, column) in cells.iter_mut().zip(window_start..window_end) {
        *cell = ascii_terminal_cell(
            terminal,
            point.row,
            u16::try_from(column).unwrap_or(u16::MAX),
        )?;
    }
    let cells = &cells[..window_end.saturating_sub(window_start)];
    let pointer = point_index.saturating_sub(window_start);

    for (prefix, suffix) in IMAGE_PLACEHOLDERS {
        for start in (0..=pointer.min(cells.len().saturating_sub(1))).rev() {
            let Some(rest) = cells[start..].strip_prefix(prefix.as_bytes()) else {
                continue;
            };
            let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
            if !rest[digits..].starts_with(suffix.as_bytes()) {
                continue;
            }
            let Some(number) = std::str::from_utf8(&rest[..digits])
                .ok()
                .and_then(|digits| digits.parse::<u32>().ok())
            else {
                continue;
            };
            if !bound_pasted_images.contains(&number) {
                continue;
            }
            let end = start + prefix.len() + digits + suffix.len();
            if pointer >= end {
                continue;
            }
            return Ok(Some(HoverLink {
                row: point.row,
                start: u16::try_from(window_start + start).unwrap_or(u16::MAX),
                end: u16::try_from(window_start + end).unwrap_or(u16::MAX),
                uri: format!("{IMAGE_PLACEHOLDER_SCHEME}://{number}"),
            }));
        }
    }
    Ok(None)
}

fn hover_link_is_bound(link: &HoverLink, bound_pasted_images: &HashSet<u32>) -> bool {
    pasted_image_number(&link.uri).is_none_or(|number| bound_pasted_images.contains(&number))
}

fn pasted_image_number(uri: &str) -> Option<u32> {
    uri.strip_prefix(IMAGE_PLACEHOLDER_SCHEME)?
        .strip_prefix("://")?
        .parse()
        .ok()
}

fn image_placeholder_occurrences(
    terminal: &Terminal<'_, '_>,
) -> Result<HashMap<u32, usize>, WorkerError> {
    let columns = terminal.cols()?;
    let rows = terminal.rows()?;
    let first_row = u32::try_from(terminal.scrollback_rows()?).unwrap_or(u32::MAX);
    let mut occurrences = HashMap::new();
    let mut cells = Vec::with_capacity(usize::from(columns));
    for row in 0..rows {
        cells.clear();
        for column in 0..columns {
            cells.push(ascii_terminal_screen_cell(
                terminal,
                first_row.saturating_add(u32::from(row)),
                column,
            )?);
        }
        // Placeholders split by terminal wrapping keep the existing single-row behavior.
        for start in 0..cells.len() {
            for (prefix, suffix) in IMAGE_PLACEHOLDERS {
                let Some(rest) = cells[start..].strip_prefix(prefix.as_bytes()) else {
                    continue;
                };
                let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
                if !rest[digits..].starts_with(suffix.as_bytes()) {
                    continue;
                }
                let Some(number) = std::str::from_utf8(&rest[..digits])
                    .ok()
                    .and_then(|digits| digits.parse::<u32>().ok())
                else {
                    continue;
                };
                *occurrences.entry(number).or_insert(0) += 1;
                break;
            }
        }
    }
    Ok(occurrences)
}

fn plain_uri_at(
    terminal: &Terminal<'_, '_>,
    point: PointerCellEvent,
    columns: u16,
    scratch: &mut Vec<u8>,
) -> Result<Option<HoverLink>, WorkerError> {
    let started = diagnostic_timer();
    scratch.clear();
    let point_index = usize::from(point.column);
    let result = (|| {
        if point.column >= columns
            || ascii_terminal_cell(terminal, point.row, point.column)?.is_ascii_whitespace()
        {
            return Ok(None);
        }

        let mut start = point_index;
        while start > 0 && point_index.saturating_sub(start) < MAX_LINK_URI_BYTES - 1 {
            let previous = ascii_terminal_cell(
                terminal,
                point.row,
                u16::try_from(start - 1).unwrap_or_default(),
            )?;
            if previous.is_ascii_whitespace() {
                break;
            }
            start -= 1;
        }
        let mut end = point_index.saturating_add(1);
        while end < usize::from(columns) && end.saturating_sub(start) < MAX_LINK_URI_BYTES {
            let next =
                ascii_terminal_cell(terminal, point.row, u16::try_from(end).unwrap_or(u16::MAX))?;
            if next.is_ascii_whitespace() {
                break;
            }
            end += 1;
        }
        if (start > 0
            && !ascii_terminal_cell(
                terminal,
                point.row,
                u16::try_from(start - 1).unwrap_or_default(),
            )?
            .is_ascii_whitespace())
            || (end < usize::from(columns)
                && !ascii_terminal_cell(
                    terminal,
                    point.row,
                    u16::try_from(end).unwrap_or(u16::MAX),
                )?
                .is_ascii_whitespace())
        {
            return Ok(None);
        }

        scratch.reserve(end.saturating_sub(start));
        for column in start..end {
            scratch.push(ascii_terminal_cell(
                terminal,
                point.row,
                u16::try_from(column).unwrap_or(u16::MAX),
            )?);
        }
        let mut local_start = 0;
        let mut local_end = scratch.len();
        while local_start < local_end
            && matches!(
                scratch[local_start],
                b'(' | b'[' | b'{' | b'<' | b'\'' | b'"'
            )
        {
            local_start += 1;
        }
        while local_start < local_end
            && matches!(
                scratch[local_end - 1],
                b'.' | b',' | b';' | b'!' | b'?' | b')' | b']' | b'}' | b'>' | b'\'' | b'"'
            )
        {
            local_end -= 1;
        }
        let local_point = point_index.saturating_sub(start);
        if !(local_start..local_end).contains(&local_point) {
            return Ok(None);
        }
        let Ok(uri) = std::str::from_utf8(&scratch[local_start..local_end]) else {
            return Ok(None);
        };
        if !is_safe_link_uri(uri) {
            return Ok(None);
        }
        Ok(Some(HoverLink {
            row: point.row,
            start: u16::try_from(start.saturating_add(local_start)).unwrap_or(u16::MAX),
            end: u16::try_from(start.saturating_add(local_end)).unwrap_or(u16::MAX),
            uri: uri.to_owned(),
        }))
    })();
    log::trace!(
        target: "zz_terminal::diagnostics::link",
        "plain_uri_lookup row={} column={} columns={} scanned_bytes={} hit={} elapsed_us={}",
        point.row,
        point.column,
        columns,
        scratch.len(),
        result.as_ref().is_ok_and(Option::is_some),
        diagnostic_elapsed_us(started),
    );
    result
}

fn ascii_terminal_cell(
    terminal: &Terminal<'_, '_>,
    row: u16,
    column: u16,
) -> Result<u8, WorkerError> {
    let grid_ref = terminal.grid_ref(Point::Viewport(PointCoordinate {
        x: column,
        y: u32::from(row),
    }))?;
    let codepoint = grid_ref.cell()?.codepoint()?;
    Ok(u8::try_from(codepoint)
        .ok()
        .filter(u8::is_ascii_graphic)
        .unwrap_or(b' '))
}

fn ascii_terminal_screen_cell(
    terminal: &Terminal<'_, '_>,
    row: u32,
    column: u16,
) -> Result<u8, WorkerError> {
    let grid_ref = terminal.grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))?;
    let codepoint = grid_ref.cell()?.codepoint()?;
    Ok(u8::try_from(codepoint)
        .ok()
        .filter(u8::is_ascii_graphic)
        .unwrap_or(b' '))
}

fn cell_has_hyperlink_uri(
    terminal: &Terminal<'_, '_>,
    row: u16,
    column: u16,
    expected: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<bool, WorkerError> {
    Ok(hyperlink_uri_bytes(terminal, row, column, scratch)?
        .is_some_and(|length| scratch[..length] == *expected))
}

fn hyperlink_uri_bytes(
    terminal: &Terminal<'_, '_>,
    row: u16,
    column: u16,
    scratch: &mut Vec<u8>,
) -> Result<Option<usize>, WorkerError> {
    if scratch.len() < LINK_URI_SCRATCH_BYTES {
        scratch.resize(LINK_URI_SCRATCH_BYTES, 0);
    }
    let grid_ref = terminal.grid_ref(Point::Viewport(PointCoordinate {
        x: column,
        y: u32::from(row),
    }))?;
    match grid_ref.hyperlink_uri(scratch) {
        Ok(0) => Ok(None),
        Ok(length) => Ok(Some(length)),
        Err(libghostty_vt::Error::OutOfSpace { required })
            if required > 0 && required <= MAX_LINK_URI_BYTES =>
        {
            scratch.resize(required, 0);
            let length = grid_ref.hyperlink_uri(scratch)?;
            Ok((length > 0).then_some(length))
        }
        Err(libghostty_vt::Error::OutOfSpace { .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn is_safe_link_uri(uri: &str) -> bool {
    if uri.len() > MAX_LINK_URI_BYTES
        || uri
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return false;
    }
    let Some((scheme, rest)) = uri.split_once(':') else {
        return false;
    };
    if scheme.eq_ignore_ascii_case("mailto") {
        return !rest.is_empty();
    }
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "file" | "ssh"
    ) && rest.starts_with("//")
}

fn history_scrollbar_state(
    terminal: &Terminal<'_, '_>,
) -> Result<ScrollbarState, TerminalCaptureError> {
    let scrollbar = terminal.scrollbar().map_err(capture_failure)?;
    Ok(ScrollbarState {
        total: u32::try_from(scrollbar.total).map_err(|_| {
            TerminalCaptureError::Failed("terminal history is too large".to_owned())
        })?,
        offset: u32::try_from(scrollbar.offset).map_err(|_| {
            TerminalCaptureError::Failed("terminal history is too large".to_owned())
        })?,
        len: u32::try_from(scrollbar.len).map_err(|_| {
            TerminalCaptureError::Failed("terminal history is too large".to_owned())
        })?,
    })
}

fn empty_history_capture(
    terminal: &Terminal<'_, '_>,
    start: u32,
) -> Result<HistoryCapture, TerminalCaptureError> {
    let history_rows =
        u32::try_from(terminal.scrollback_rows().map_err(capture_failure)?).unwrap_or(u32::MAX);
    Ok((
        start.min(history_rows),
        Vec::new(),
        TerminalDictionary::default(),
        history_scrollbar_state(terminal)?,
        terminal.cols().map_err(capture_failure)?,
    ))
}

fn capture_history(
    terminal: &Terminal<'_, '_>,
    start: u32,
    count: u32,
) -> Result<HistoryCapture, TerminalCaptureError> {
    let history_rows =
        u32::try_from(terminal.scrollback_rows().map_err(capture_failure)?).unwrap_or(u32::MAX);
    let start = start.min(history_rows);
    let end = start.saturating_add(count).min(history_rows);
    let columns = terminal.cols().map_err(capture_failure)?.max(1);
    let foreground = terminal
        .fg_color()
        .map_err(capture_failure)?
        .unwrap_or_default();
    let background = terminal
        .bg_color()
        .map_err(capture_failure)?
        .unwrap_or_default();
    let palette = terminal.color_palette().map_err(capture_failure)?.0;
    let default_style = PackedStyle::new(
        color(foreground),
        color(background),
        None,
        0,
        UnderlineStyle::None,
    );
    let mut dictionary = ViewportDictionary::default();
    dictionary.ensure_default(default_style, &palette);
    let mut rows = Vec::with_capacity(usize::try_from(end - start).unwrap_or(0));
    let mut stack = ['\0'; 8];
    let mut grapheme_scratch = Vec::new();
    let mut grapheme_text = String::with_capacity(8);

    for row in start..end {
        let mut output = Vec::with_capacity(usize::from(columns));
        for column in 0..columns {
            let grid_ref = terminal
                .grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))
                .map_err(capture_failure)?;
            let raw_style = grid_ref.style().map_err(capture_failure)?;
            let raw_cell = grid_ref.cell().map_err(capture_failure)?;
            let mut cell_foreground = resolve_style_color(raw_style.fg_color, &palette)
                .unwrap_or_else(|| color(foreground));
            let mut cell_background = resolve_style_color(raw_style.bg_color, &palette)
                .unwrap_or_else(|| color(background));
            cell_background = match raw_cell.content_tag().map_err(capture_failure)? {
                CellContentTag::BgColorPalette => color(
                    palette[usize::from(raw_cell.bg_color_palette().map_err(capture_failure)?.0)],
                ),
                CellContentTag::BgColorRgb => {
                    color(raw_cell.bg_color_rgb().map_err(capture_failure)?)
                }
                CellContentTag::Codepoint | CellContentTag::CodepointGrapheme => cell_background,
            };
            if raw_style.inverse {
                std::mem::swap(&mut cell_foreground, &mut cell_background);
            }

            grapheme_text.clear();
            match grid_ref.graphemes(&mut stack) {
                Ok(count) => grapheme_text.extend(stack[..count].iter().copied()),
                Err(libghostty_vt::Error::OutOfSpace { required }) => {
                    if grapheme_scratch.len() < required {
                        grapheme_scratch.resize(required, '\0');
                    }
                    let count = grid_ref
                        .graphemes(&mut grapheme_scratch)
                        .map_err(capture_failure)?;
                    grapheme_text.extend(grapheme_scratch[..count].iter().copied());
                }
                Err(error) => return Err(capture_failure(error)),
            }

            let width = match raw_cell.wide().map_err(capture_failure)? {
                CellWide::Narrow => CellWidth::Narrow,
                CellWide::Wide => CellWidth::Wide,
                CellWide::SpacerTail => CellWidth::SpacerTail,
                CellWide::SpacerHead => CellWidth::SpacerHead,
            };
            let foreground_explicit_rgb = matches!(
                if raw_style.inverse {
                    raw_style.bg_color
                } else {
                    raw_style.fg_color
                },
                StyleColor::Rgb(_)
            );
            let style = PackedStyle::new(
                cell_foreground,
                cell_background,
                resolve_style_color(raw_style.underline_color, &palette),
                style_attributes(
                    &raw_style,
                    foreground_explicit_rgb,
                    raw_cell.has_hyperlink().map_err(capture_failure)?,
                ),
                underline_style(raw_style.underline),
            );
            output.push(PackedCell::new(
                dictionary.encode_glyph(&grapheme_text),
                dictionary.intern_style(style),
                width,
            ));
        }
        rows.push(output);
    }

    Ok((
        start,
        rows,
        dictionary.shared_dictionary().as_ref().clone(),
        history_scrollbar_state(terminal)?,
        columns,
    ))
}

fn capture_terminal(
    terminal: &Terminal<'_, '_>,
    mode: Option<&CopyModeState>,
    options: CaptureOptions,
) -> Result<String, TerminalCaptureError> {
    if options.mode {
        let mode = mode.ok_or(TerminalCaptureError::ModeUnavailable)?;
        if options.alternate && mode.revision.screen != Screen::Alternate {
            return Err(TerminalCaptureError::AlternateUnavailable);
        }
        return capture_mode_revision(mode, options);
    }
    let active_screen = terminal.active_screen().map_err(capture_failure)?;
    if options.alternate && active_screen != Screen::Alternate {
        return Err(TerminalCaptureError::AlternateUnavailable);
    }
    let total = terminal.total_rows().map_err(capture_failure)?;
    if total == 0 {
        return Ok(String::new());
    }
    let total = u64::try_from(total).unwrap_or(u64::MAX);
    let rows = u64::from(terminal.rows().map_err(capture_failure)?);
    let visible_start = u64::try_from(terminal.scrollback_rows().map_err(capture_failure)?)
        .unwrap_or(u64::MAX)
        .min(total.saturating_sub(1));
    let visible_end = visible_start
        .saturating_add(rows.saturating_sub(1))
        .min(total.saturating_sub(1));
    let start = resolve_capture_boundary(options.start, visible_start, visible_end, total);
    let end = resolve_capture_boundary(options.end, visible_start, visible_end, total);
    if start > end {
        return Ok(String::new());
    }

    let columns = terminal.cols().map_err(capture_failure)?;
    let start = terminal
        .grid_ref(Point::Screen(PointCoordinate {
            x: 0,
            y: u32::try_from(start).unwrap_or(u32::MAX),
        }))
        .map_err(capture_failure)?;
    let end = terminal
        .grid_ref(Point::Screen(PointCoordinate {
            x: columns.saturating_sub(1),
            y: u32::try_from(end).unwrap_or(u32::MAX),
        }))
        .map_err(capture_failure)?;
    let selection = Selection::new(start, end, false);
    let format = if options.escape_sequences {
        Format::Vt
    } else {
        Format::Plain
    };
    let formatter_options = FormatterOptions::new()
        .with_format(format)
        .with_unwrap(options.join_wrapped)
        .with_trim(!options.preserve_trailing)
        .with_selection(&selection);
    let mut formatter = Formatter::new(terminal, formatter_options).map_err(capture_failure)?;
    let length = match formatter.format_len() {
        Ok(length) => length,
        Err(libghostty_vt::Error::InvalidValue) => return Ok(String::new()),
        Err(error) => return Err(capture_failure(error)),
    };
    if length > MAX_CAPTURE_BYTES {
        return Err(TerminalCaptureError::TooLarge);
    }
    if length == 0 {
        return Ok(String::new());
    }
    let mut output = vec![0_u8; length];
    let written = formatter.format_buf(&mut output).map_err(capture_failure)?;
    output.truncate(written);
    String::from_utf8(output).map_err(|error| TerminalCaptureError::Failed(error.to_string()))
}

const PANE_RESET_PRELUDE: &[u8] = b"\x1b\\\x1b[m\x1b(B\x1b)B\x1b[r\x1b[?7h\x1b[?25h\x1b[?1l\x1b[4l\x1b[?6l\x1b[20l\x1b>\x1b[?12l\x1b[?2026l\x1b[?2031l\x1b[=0u\x1b[>4;0m\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1016l\x1b[?1004l\x1b[?2004l\x1b[3g";

const PANE_RESET_PALETTE: &[u8] = b"\x1b]104\x1b\\\x1b]110\x1b\\\x1b]111\x1b\\\x1b]112\x1b\\";

/// Count the visible rows the pin's `grid_view_clear_history` would push into
/// history: every row up to and including the last one holding content.
fn used_visible_rows(terminal: &Terminal<'_, '_>) -> u16 {
    let rows = terminal.rows().unwrap_or(0);
    match capture_terminal(terminal, None, CaptureOptions::default()) {
        Ok(text) if text.is_empty() => 0,
        Ok(text) => u16::try_from(text.split('\n').count())
            .unwrap_or(rows)
            .min(rows),
        Err(_) => rows,
    }
}

/// Apply pinned tmux's `send-keys -R` to the pane without losing scrollback.
///
/// `cmd-send-keys.c` runs `colour_palette_clear` plus `input_reset(ictx, 1)`,
/// whose `screen_write_reset` restores default tab stops, drops the scroll
/// region, sets the screen mode back to cursor plus wrap, clears the screen
/// through the `scroll-on-clear` path, and homes the cursor. libghostty's own
/// `reset` is RIS and discards history, and its parser ignores DECSTR outright,
/// so the same result is composed here out of a scroll-up sized to the used
/// rows plus one explicit reset per mode the pin's `MODE_CURSOR|MODE_WRAP`
/// assignment drops, then tab, saved-cursor, and palette resets. The leading ST
/// terminates any string sequence the pane left half parsed without printing a
/// cell, which is what `input_clear` plus the return to the ground state does.
fn reset_pane_screen(terminal: &mut Terminal<'_, '_>) -> Result<(), WorkerError> {
    let used = used_visible_rows(terminal);
    let columns = terminal.cols()?;
    let mut program = PANE_RESET_PRELUDE.to_vec();
    let mut column = 8_u16;
    while column < columns {
        write!(program, "\x1b[1;{}H\x1bH", column.saturating_add(1))?;
        column = column.saturating_add(8);
    }
    if used > 0 {
        write!(program, "\x1b[{used}S")?;
    }
    program.extend_from_slice(b"\x1b[2J\x1b[H\x1b7");
    program.extend_from_slice(PANE_RESET_PALETTE);
    terminal.vt_write(&program);
    terminal.scroll_viewport(ScrollViewport::Bottom);
    Ok(())
}

fn capture_viewport(
    viewport: &TerminalViewport,
    options: CaptureOptions,
) -> Result<String, TerminalCaptureError> {
    if options.alternate {
        return Err(TerminalCaptureError::AlternateUnavailable);
    }
    if options.mode && matches!(viewport.mode, TerminalMode::Live) {
        return Err(TerminalCaptureError::ModeUnavailable);
    }
    let total = u64::from(viewport.rows);
    if total == 0 {
        return Ok(String::new());
    }
    let visible_end = total.saturating_sub(1);
    let start = resolve_capture_boundary(options.start, 0, visible_end, total);
    let end = resolve_capture_boundary(options.end, 0, visible_end, total);
    if start > end {
        return Ok(String::new());
    }

    let mut output = String::new();
    for row in start..=end {
        if options.escape_sequences {
            capture_viewport_row_vt(
                viewport,
                u16::try_from(row).unwrap_or(u16::MAX),
                options.preserve_trailing,
                &mut output,
            );
        } else {
            capture_viewport_row(
                viewport,
                u16::try_from(row).unwrap_or(u16::MAX),
                options.preserve_trailing,
                &mut output,
            );
        }
        if row < end {
            output.push('\n');
        }
        if output.len() > MAX_CAPTURE_BYTES {
            return Err(TerminalCaptureError::TooLarge);
        }
    }
    Ok(output)
}

fn capture_viewport_row(
    viewport: &TerminalViewport,
    row: u16,
    preserve_trailing: bool,
    output: &mut String,
) {
    let start = output.len();
    for cell in viewport.row(row).unwrap_or_default() {
        push_viewport_cell(viewport, *cell, output);
    }
    if !preserve_trailing {
        let trimmed = output[start..].trim_end().len();
        output.truncate(start.saturating_add(trimmed));
    }
}

fn capture_viewport_row_vt(
    viewport: &TerminalViewport,
    row: u16,
    preserve_trailing: bool,
    output: &mut String,
) {
    let cells = viewport.row(row).unwrap_or_default();
    let last = if preserve_trailing {
        cells.len().checked_sub(1)
    } else {
        cells
            .iter()
            .rposition(|cell| viewport_cell_has_non_whitespace(viewport, *cell))
    };
    let Some(last) = last else {
        return;
    };
    let mut active_style = None;
    for cell in &cells[..=last] {
        if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
            continue;
        }
        if active_style != Some(cell.style_id()) {
            if let Some(style) = viewport.style(*cell) {
                mode_revision::push_sgr(output, style);
            }
            active_style = Some(cell.style_id());
        }
        push_viewport_cell(viewport, *cell, output);
    }
    if active_style.is_some() {
        output.push_str("\x1b[0m");
    }
}

fn viewport_cell_has_non_whitespace(viewport: &TerminalViewport, cell: PackedCell) -> bool {
    !matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead)
        && cell.glyph() != 0
        && viewport
            .cell_text(cell)
            .chars()
            .any(|character| !character.is_whitespace())
}

fn push_viewport_cell(viewport: &TerminalViewport, cell: PackedCell, output: &mut String) {
    if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
        return;
    }
    if cell.glyph() == 0 {
        output.push(' ');
    } else {
        viewport.push_glyph(cell, output);
    }
}

fn capture_mode_revision(
    mode: &CopyModeState,
    options: CaptureOptions,
) -> Result<String, TerminalCaptureError> {
    capture_revision(&mode.revision, mode.viewport_offset, options)
}

/// Resolves `capture-pane` boundaries against a captured grid, whose visible
/// screen starts at `viewport_offset` rows down from the top of the history.
fn capture_revision(
    revision: &ModeRevision,
    viewport_offset: u32,
    options: CaptureOptions,
) -> Result<String, TerminalCaptureError> {
    let total = u64::from(revision.total_rows());
    let visible_start = u64::from(viewport_offset).min(total.saturating_sub(1));
    let visible_end = visible_start
        .saturating_add(u64::from(revision.viewport_rows).saturating_sub(1))
        .min(total.saturating_sub(1));
    let start = resolve_capture_boundary(options.start, visible_start, visible_end, total);
    let end = resolve_capture_boundary(options.end, visible_start, visible_end, total);
    if start > end {
        return Ok(String::new());
    }
    let output = revision.capture_rows(
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
        options.join_wrapped,
        options.preserve_trailing,
        options.escape_sequences,
    );
    if output.len() > MAX_CAPTURE_BYTES {
        return Err(TerminalCaptureError::TooLarge);
    }
    Ok(output)
}

fn resolve_capture_boundary(
    boundary: CaptureBoundary,
    visible_start: u64,
    visible_end: u64,
    total: u64,
) -> u64 {
    match boundary {
        CaptureBoundary::HistoryStart => 0,
        CaptureBoundary::VisibleEnd => visible_end,
        CaptureBoundary::Relative(offset) => {
            let resolved = i128::from(visible_start).saturating_add(i128::from(offset));
            u64::try_from(resolved.max(0))
                .unwrap_or(u64::MAX)
                .min(total.saturating_sub(1))
        }
    }
}

fn capture_failure(error: libghostty_vt::Error) -> TerminalCaptureError {
    TerminalCaptureError::Failed(error.to_string())
}

fn semantic_failure(error: &WorkerError) -> TerminalCaptureError {
    TerminalCaptureError::Failed(error.to_string())
}

fn capture_last_command(
    terminal: &Terminal<'_, '_>,
) -> Result<LastCommandCapture, TerminalCaptureError> {
    let total = u32::try_from(terminal.total_rows().map_err(capture_failure)?).unwrap_or(u32::MAX);
    if total == 0 {
        return Err(TerminalCaptureError::NoSemanticMarks);
    }
    let columns = terminal.cols().map_err(capture_failure)?;
    let mut saw_prompt = false;
    let mut block_end = total;
    let mut row = total;
    while row > 0 {
        row -= 1;
        if row_semantic_prompt(terminal, row).map_err(|error| semantic_failure(&error))?
            != RowSemanticPrompt::Prompt
        {
            continue;
        }
        saw_prompt = true;
        let input = input_span(terminal, row, block_end, columns)
            .map_err(|error| semantic_failure(&error))?;
        let Some((command_start, command_end, input_rows_end)) = input else {
            block_end = row;
            continue;
        };
        let command = format_span(terminal, command_start, command_end)?;
        if command.trim().is_empty() {
            block_end = row;
            continue;
        }
        let span = output_span(terminal, input_rows_end, block_end, columns)
            .map_err(|error| semantic_failure(&error))?;
        let (output, truncated_rows) = match span {
            Some((start, end, dropped_rows)) => {
                let output = format_span(terminal, start, end)?;
                let (output, dropped) = clamp_output_bytes(output);
                (output, dropped_rows.saturating_add(dropped))
            }
            None => (String::new(), 0),
        };
        return Ok(LastCommandCapture {
            command,
            output,
            truncated_rows,
        });
    }
    if saw_prompt {
        return Ok(LastCommandCapture::default());
    }
    Err(TerminalCaptureError::NoSemanticMarks)
}

fn row_content_span(
    terminal: &Terminal<'_, '_>,
    row: u32,
    columns: u16,
    content: CellSemanticContent,
) -> Result<Option<(u16, u16)>, WorkerError> {
    let mut span: Option<(u16, u16)> = None;
    for column in 0..columns {
        let cell = terminal
            .grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))?
            .cell()?;
        if !cell.has_text()? || cell.semantic_content()? != content {
            continue;
        }
        span = Some(span.map_or((column, column), |(start, _)| (start, column)));
    }
    Ok(span)
}

fn input_span(
    terminal: &Terminal<'_, '_>,
    prompt_row: u32,
    block_end: u32,
    columns: u16,
) -> Result<Option<(PointCoordinate, PointCoordinate, u32)>, WorkerError> {
    let mut span: Option<(PointCoordinate, PointCoordinate)> = None;
    let mut row = prompt_row;
    while row < block_end {
        let Some((first, last)) =
            row_content_span(terminal, row, columns, CellSemanticContent::Input)?
        else {
            break;
        };
        let start = PointCoordinate { x: first, y: row };
        let end = PointCoordinate { x: last, y: row };
        span = Some(span.map_or((start, end), |(existing, _)| (existing, end)));
        row = row.saturating_add(1);
    }
    Ok(span.map(|(start, end)| (start, end, row)))
}

fn output_span(
    terminal: &Terminal<'_, '_>,
    first_row: u32,
    block_end: u32,
    columns: u16,
) -> Result<Option<(PointCoordinate, PointCoordinate, usize)>, WorkerError> {
    let mut last = None;
    let mut row = block_end;
    while row > first_row {
        row -= 1;
        if let Some((_, column)) =
            row_content_span(terminal, row, columns, CellSemanticContent::Output)?
        {
            last = Some(PointCoordinate { x: column, y: row });
            break;
        }
    }
    let Some(last) = last else {
        return Ok(None);
    };
    let mut first = last;
    for row in first_row..=last.y {
        if let Some((column, _)) =
            row_content_span(terminal, row, columns, CellSemanticContent::Output)?
        {
            first = PointCoordinate { x: column, y: row };
            break;
        }
    }
    let limit = u32::try_from(MAX_LAST_COMMAND_LINES).unwrap_or(u32::MAX);
    let rows = last.y.saturating_sub(first.y).saturating_add(1);
    if rows <= limit {
        return Ok(Some((first, last, 0)));
    }
    let dropped = usize::try_from(rows - limit).unwrap_or(usize::MAX);
    let start = PointCoordinate {
        x: 0,
        y: last.y.saturating_sub(limit.saturating_sub(1)),
    };
    Ok(Some((start, last, dropped)))
}

fn clamp_output_bytes(output: String) -> (String, usize) {
    if output.len() <= MAX_LAST_COMMAND_BYTES {
        return (output, 0);
    }
    let mut cut = output.len() - MAX_LAST_COMMAND_BYTES;
    while cut < output.len() && !output.is_char_boundary(cut) {
        cut += 1;
    }
    let start = output
        .match_indices('\n')
        .find(|(offset, _)| *offset >= cut)
        .map_or(cut, |(offset, _)| offset.saturating_add(1));
    let dropped = output[..start].matches('\n').count();
    (output[start..].to_owned(), dropped)
}

fn format_span(
    terminal: &Terminal<'_, '_>,
    start: PointCoordinate,
    end: PointCoordinate,
) -> Result<String, TerminalCaptureError> {
    let start = terminal
        .grid_ref(Point::Screen(start))
        .map_err(capture_failure)?;
    let end = terminal
        .grid_ref(Point::Screen(end))
        .map_err(capture_failure)?;
    let selection = Selection::new(start, end, false);
    let options = FormatterOptions::new()
        .with_format(Format::Plain)
        .with_trim(true)
        .with_selection(&selection);
    let mut formatter = Formatter::new(terminal, options).map_err(capture_failure)?;
    let length = match formatter.format_len() {
        Ok(length) => length,
        Err(libghostty_vt::Error::InvalidValue) => return Ok(String::new()),
        Err(error) => return Err(capture_failure(error)),
    };
    if length > MAX_CAPTURE_BYTES {
        return Err(TerminalCaptureError::TooLarge);
    }
    if length == 0 {
        return Ok(String::new());
    }
    let mut output = vec![0_u8; length];
    let written = formatter.format_buf(&mut output).map_err(capture_failure)?;
    output.truncate(written);
    String::from_utf8(output).map_err(|error| TerminalCaptureError::Failed(error.to_string()))
}

fn apply_copy_mode_action(
    terminal: &mut Terminal<'_, '_>,
    selection: &mut Option<SelectionState>,
    copy_mode: &mut CopyModeSlot,
    unseen_output: &mut u32,
    action: CopyModeAction,
    word_separators: &WordSeparators,
    mode_keys_vi: bool,
) -> Result<ViewActionResult, WorkerError> {
    let Some(mut mode) = copy_mode.take() else {
        return Ok(ViewActionResult::None);
    };

    if mode.search_marks && action.clears_search_marks(mode_keys_vi) {
        mode.search_marks = false;
        mode.search_count = None;
        mode.incremental_origin = None;
    }

    match action {
        ref movement @ (CopyModeAction::Left
        | CopyModeAction::Right
        | CopyModeAction::Up
        | CopyModeAction::Down
        | CopyModeAction::PageUp
        | CopyModeAction::PageDown
        | CopyModeAction::PageDownScrollExit
        | CopyModeAction::HalfPageUp
        | CopyModeAction::HalfPageDown
        | CopyModeAction::HalfPageDownScrollExit
        | CopyModeAction::Top
        | CopyModeAction::Bottom
        | CopyModeAction::TopLine
        | CopyModeAction::MiddleLine
        | CopyModeAction::BottomLine
        | CopyModeAction::StartOfLine
        | CopyModeAction::BackToIndentation
        | CopyModeAction::EndOfLine
        | CopyModeAction::NextWord
        | CopyModeAction::PreviousWord
        | CopyModeAction::NextWordEnd
        | CopyModeAction::NextSpace
        | CopyModeAction::PreviousSpace
        | CopyModeAction::NextSpaceEnd
        | CopyModeAction::NextParagraph
        | CopyModeAction::PreviousParagraph
        | CopyModeAction::NextMatchingBracket
        | CopyModeAction::PreviousMatchingBracket
        | CopyModeAction::NextPrompt { .. }
        | CopyModeAction::PreviousPrompt { .. }
        | CopyModeAction::CursorCentreVertical
        | CopyModeAction::CursorCentreHorizontal
        | CopyModeAction::JumpToMark) => {
            let scroll_exit = match movement {
                CopyModeAction::PageDownScrollExit | CopyModeAction::HalfPageDownScrollExit => true,
                CopyModeAction::PageDown | CopyModeAction::HalfPageDown => mode.scroll_exit,
                _ => false,
            };
            if scroll_exit && copy_mode_scroll_exit_ready(&mode) {
                return cancel_copy_mode(terminal, selection, unseen_output);
            }
            move_copy_cursor(&mut mode, movement, word_separators, mode_keys_vi);
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            if scroll_exit && copy_mode_scroll_exit_ready(&mode) {
                return cancel_copy_mode(terminal, selection, unseen_output);
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::ScrollDownAndCancel => {
            scroll_copy_cursor(&mut mode, false, mode_keys_vi);
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            if mode.viewport_offset == mode.revision.maximum_offset() {
                return cancel_copy_mode(terminal, selection, unseen_output);
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::CursorDownAndCancel => {
            *copy_mode = Some(mode);
            cursor_down_and_cancel(
                terminal,
                selection,
                copy_mode,
                unseen_output,
                1,
                word_separators,
                mode_keys_vi,
            )
        }
        scroll_exit_action @ (CopyModeAction::ScrollExitOn
        | CopyModeAction::ScrollExitOff
        | CopyModeAction::ScrollExitToggle) => {
            mode.scroll_exit = match scroll_exit_action {
                CopyModeAction::ScrollExitOn => true,
                CopyModeAction::ScrollExitOff => false,
                _ => !mode.scroll_exit,
            };
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        scroll @ (CopyModeAction::ScrollUp | CopyModeAction::ScrollDown) => {
            let scrolls_toward_bottom = matches!(scroll, CopyModeAction::ScrollDown);
            if scrolls_toward_bottom && mode.scroll_exit && copy_mode_scroll_exit_ready(&mode) {
                return cancel_copy_mode(terminal, selection, unseen_output);
            }
            scroll_copy_cursor(&mut mode, !scrolls_toward_bottom, mode_keys_vi);
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            if scrolls_toward_bottom && mode.scroll_exit && copy_mode_scroll_exit_ready(&mode) {
                return cancel_copy_mode(terminal, selection, unseen_output);
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        placement @ (CopyModeAction::ScrollTop
        | CopyModeAction::ScrollMiddle
        | CopyModeAction::ScrollBottom) => {
            let last = u32::from(mode.revision.viewport_rows.saturating_sub(1));
            let row = match placement {
                CopyModeAction::ScrollTop => 0,
                CopyModeAction::ScrollBottom => last,
                _ => last / 2,
            };
            scroll_copy_view_to_screen_row(&mut mode, row);
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::GotoLine(line) => {
            goto_copy_line(&mut mode, line);
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::RecentreTopBottom => {
            let row = mode.cursor.y;
            let target = match mode.recentre {
                Some((line, target)) if line == row => target,
                _ => RecentreTarget::Middle,
            };
            let last = u32::from(mode.revision.viewport_rows.saturating_sub(1));
            let (screen_row, next) = match target {
                RecentreTarget::Middle => (last / 2, RecentreTarget::Top),
                RecentreTarget::Top => (0, RecentreTarget::Bottom),
                RecentreTarget::Bottom => (last, RecentreTarget::Middle),
            };
            mode.recentre = Some((row, next));
            mode.viewport_offset = row
                .saturating_sub(screen_row)
                .min(mode.revision.maximum_offset());
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::TogglePosition => {
            mode.hide_position = !mode.hide_position;
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        // `window_copy_refresh_start` refuses a view of another pane and
        // view-mode, and does nothing when the refresh is already running;
        // `window_copy_refresh_stop` always clears the bit.
        toggle @ (CopyModeAction::RefreshOn
        | CopyModeAction::RefreshOff
        | CopyModeAction::RefreshToggle) => {
            let start = match toggle {
                CopyModeAction::RefreshOn => true,
                CopyModeAction::RefreshOff => false,
                _ => !mode.refresh,
            };
            mode.refresh = start && mode.kind == FrozenModeKind::Copy && !mode.sourced;
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        // `window_copy_refresh_timer`: skip the tick unless the pane has
        // unseen output and no selection or cursor drag is live, then
        // `window_copy_do_refresh` re-clones the backing, keeps the view on
        // the row it was already showing, and only follows new output when the
        // view is at the bottom with the cursor on the last row.
        CopyModeAction::RefreshRevision => {
            if !mode.refresh
                || mode.kind != FrozenModeKind::Copy
                || mode.selection.is_some()
                || mode.selecting
                || *unseen_output == 0
            {
                *copy_mode = Some(mode);
                return Ok(ViewActionResult::None);
            }
            let rows = u32::from(mode.revision.viewport_rows.saturating_sub(1));
            let follow = mode.viewport_offset == mode.revision.maximum_offset()
                && mode.cursor.y == mode.viewport_offset.saturating_add(rows);
            let offset_from_top = mode.viewport_offset;
            mode.revision = ModeRevision::capture(terminal)?;
            if follow {
                mode.viewport_offset = mode.revision.maximum_offset();
                mode.cursor = mode.revision.clamp_point(PointCoordinate {
                    x: mode.cursor.x,
                    y: mode
                        .viewport_offset
                        .saturating_add(u32::from(mode.revision.viewport_rows.saturating_sub(1))),
                });
                mode.cursor.x = mode
                    .cursor
                    .x
                    .min(revision_copy_line_end(&mode.revision, mode.cursor.y));
            } else {
                mode.viewport_offset = offset_from_top.min(mode.revision.maximum_offset());
                mode.cursor = mode.revision.clamp_point(mode.cursor);
            }
            mode.recentre = None;
            *unseen_output = 0;
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::StartSelection => {
            mode.selection = Some(ModeSelection {
                anchor: mode.cursor,
                focus: mode.cursor,
                mode: SelectionMode::Cell,
                rectangle: mode.rectangle,
            });
            mode.selection_mode = CopySelectionMode::Char;
            mode.selection_origin = mode.cursor;
            mode.selecting = true;
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::SelectWord => {
            let (anchor, focus) = mode_word_bounds(&mode.revision, mode.cursor, word_separators);
            mode.selection = Some(ModeSelection {
                anchor,
                focus,
                mode: SelectionMode::Word,
                rectangle: false,
            });
            mode.selection_mode = CopySelectionMode::Word;
            mode.selection_origin = mode.cursor;
            mode.cursor = focus;
            mode.selecting = true;
            mode.rectangle = false;
            reveal_copy_cursor(&mut mode);
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::SelectLine => {
            select_copy_mode_lines(&mut mode, 1, mode_keys_vi);
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::ClearSelection => {
            *selection = None;
            terminal.set_selection(None)?;
            mode.selection = None;
            mode.selection_mode = CopySelectionMode::Char;
            mode.selecting = false;
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::ClearSelectionOrCancel if mode.selection.is_some() || mode.selecting => {
            *selection = None;
            terminal.set_selection(None)?;
            mode.selection = None;
            mode.selection_mode = CopySelectionMode::Char;
            mode.selecting = false;
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::ToggleRectangle => {
            mode.rectangle = !mode.rectangle;
            if let Some(selection) = mode.selection.as_mut() {
                selection.rectangle = mode.rectangle;
                mode.selecting = true;
            }
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        rectangle_action @ (CopyModeAction::RectangleOn | CopyModeAction::RectangleOff) => {
            mode.rectangle = matches!(rectangle_action, CopyModeAction::RectangleOn);
            if let Some(selection) = mode.selection.as_mut() {
                selection.rectangle = mode.rectangle;
                mode.selecting = true;
            }
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::OtherEnd => {
            mode.selection_mode = CopySelectionMode::Char;
            if let Some(selection) = mode.selection.as_mut() {
                std::mem::swap(&mut selection.anchor, &mut selection.focus);
                mode.cursor = selection.focus;
                mode.selection_origin = mode.cursor;
                mode.selecting = true;
                reveal_copy_cursor(&mut mode);
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::SetMark => {
            mode.mark = Some(mode.cursor);
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::Jump(jump) => {
            let latched_vi = mode.mode_keys_vi_at_entry;
            apply_copy_jump(&mut mode, &jump, latched_vi);
            mode.last_jump = Some(jump);
            if mode.selecting {
                update_copy_selection(&mut mode, Some(word_separators));
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::RepeatJump { reverse } => {
            if let Some(mut jump) = mode.last_jump.clone() {
                if reverse {
                    jump.direction = match jump.direction {
                        CopyJumpDirection::Forward => CopyJumpDirection::Backward,
                        CopyJumpDirection::Backward => CopyJumpDirection::Forward,
                    };
                }
                let latched_vi = mode.mode_keys_vi_at_entry;
                apply_copy_jump(&mut mode, &jump, latched_vi);
                if mode.selecting {
                    update_copy_selection(&mut mode, Some(word_separators));
                }
            }
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::CopySelection(copy) => {
            let copy = *copy;
            let text = mode.selection.map_or_else(String::new, |selection| {
                mode.revision.format_selection(selection, mode_keys_vi)
            });
            let view_changed = copy.clear_selection || copy.cancel;
            if copy.cancel {
                *selection = None;
                terminal.set_selection(None)?;
                terminal.scroll_viewport(ScrollViewport::Bottom);
                *unseen_output = 0;
            } else {
                if copy.clear_selection {
                    mode.selection = None;
                    mode.selecting = false;
                }
                *copy_mode = Some(mode);
            }
            Ok(ViewActionResult::Copy(Box::new(TerminalCopyReady {
                request_id: copy.request_id,
                clipboard: copy.clipboard.then_some(ClipboardTarget::Clipboard),
                buffer: copy.buffer,
                pipe: copy.pipe,
                text,
                view_changed,
            })))
        }
        CopyModeAction::CopyEndOfLine(copy) => {
            select_copy_mode_to_line_end(&mut mode, 1, mode_keys_vi);
            *copy_mode = Some(mode);
            apply_copy_mode_action(
                terminal,
                selection,
                copy_mode,
                unseen_output,
                CopyModeAction::CopySelection(copy),
                word_separators,
                mode_keys_vi,
            )
        }
        CopyModeAction::CopyLine(copy) => {
            select_copy_mode_whole_line(&mut mode, 1, mode_keys_vi);
            *copy_mode = Some(mode);
            apply_copy_mode_action(
                terminal,
                selection,
                copy_mode,
                unseen_output,
                CopyModeAction::CopySelection(copy),
                word_separators,
                mode_keys_vi,
            )
        }
        CopyModeAction::SelectionMode(unit) => {
            mode.selection_mode = unit;
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::StopSelection => {
            mode.selection_mode = CopySelectionMode::Char;
            mode.selecting = false;
            *copy_mode = Some(mode);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeAction::SearchAgain { .. }
        | CopyModeAction::SearchCursorWord { .. }
        | CopyModeAction::Search(_) => {
            *copy_mode = Some(mode);
            Ok(ViewActionResult::None)
        }
        CopyModeAction::Cancel | CopyModeAction::ClearSelectionOrCancel => {
            cancel_copy_mode(terminal, selection, unseen_output)
        }
    }
}

fn select_copy_mode_to_line_end(mode: &mut CopyModeState, count: u32, mode_keys_vi: bool) {
    if count == 0 {
        return;
    }
    let focus_y = mode
        .cursor
        .y
        .saturating_add(count - 1)
        .min(mode.revision.total_rows().saturating_sub(1));
    mode.selection = Some(ModeSelection {
        anchor: mode.cursor,
        focus: PointCoordinate {
            x: copy_cursor_limit(&mode.revision, focus_y, mode_keys_vi, false),
            y: focus_y,
        },
        mode: SelectionMode::Cell,
        rectangle: false,
    });
    mode.selecting = true;
}

fn apply_counted_copy_mode_action(
    terminal: &mut Terminal<'_, '_>,
    selection: &mut Option<SelectionState>,
    copy_mode: &mut CopyModeSlot,
    unseen_output: &mut u32,
    action: CopyModeAction,
    count: u32,
    word_separators: &WordSeparators,
    mode_keys_vi: bool,
) -> Result<ViewActionResult, WorkerError> {
    if count == 0 {
        return Ok(ViewActionResult::None);
    }
    match action.count_policy() {
        CopyModeCountPolicy::Repeat => {
            if copy_mode.is_none() {
                return Ok(ViewActionResult::None);
            }
            for _ in 0..count {
                apply_copy_mode_action(
                    terminal,
                    selection,
                    copy_mode,
                    unseen_output,
                    action.clone(),
                    word_separators,
                    mode_keys_vi,
                )?;
                if copy_mode.is_none() {
                    break;
                }
            }
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeCountPolicy::OtherEnd if count.is_multiple_of(2) => Ok(ViewActionResult::Snapshot),
        CopyModeCountPolicy::OtherEnd | CopyModeCountPolicy::Once => apply_copy_mode_action(
            terminal,
            selection,
            copy_mode,
            unseen_output,
            action,
            word_separators,
            mode_keys_vi,
        ),
        CopyModeCountPolicy::CursorDownAndCancel => cursor_down_and_cancel(
            terminal,
            selection,
            copy_mode,
            unseen_output,
            count,
            word_separators,
            mode_keys_vi,
        ),
        CopyModeCountPolicy::SelectLine => {
            let Some(mode) = copy_mode.as_mut() else {
                return Ok(ViewActionResult::None);
            };
            select_copy_mode_lines(mode, count, mode_keys_vi);
            Ok(ViewActionResult::Snapshot)
        }
        CopyModeCountPolicy::CopyEndOfLine => {
            let CopyModeAction::CopyEndOfLine(copy) = action else {
                unreachable!("copy-end policy is only assigned to copy-end-of-line")
            };
            let Some(mut mode) = copy_mode.take() else {
                return Ok(ViewActionResult::None);
            };
            select_copy_mode_to_line_end(&mut mode, count, mode_keys_vi);
            *copy_mode = Some(mode);
            apply_copy_mode_action(
                terminal,
                selection,
                copy_mode,
                unseen_output,
                CopyModeAction::CopySelection(copy),
                word_separators,
                mode_keys_vi,
            )
        }
        CopyModeCountPolicy::CopyLine => {
            let CopyModeAction::CopyLine(copy) = action else {
                unreachable!("copy-line policy is only assigned to copy-line")
            };
            let Some(mut mode) = copy_mode.take() else {
                return Ok(ViewActionResult::None);
            };
            select_copy_mode_whole_line(&mut mode, count, mode_keys_vi);
            *copy_mode = Some(mode);
            apply_copy_mode_action(
                terminal,
                selection,
                copy_mode,
                unseen_output,
                CopyModeAction::CopySelection(copy),
                word_separators,
                mode_keys_vi,
            )
        }
    }
}

/// `window_copy_do_copy_line`'s selection: `selflag` back to `SEL_CHAR`, the
/// cursor to the start of its logical line, `np - 1` rows down, then the end of
/// that logical line. The cursor itself is left alone, because the pin puts
/// `cx`, `cy` and `oy` back after the copy.
fn select_copy_mode_whole_line(mode: &mut CopyModeState, count: u32, mode_keys_vi: bool) {
    if count == 0 {
        return;
    }
    let (anchor, _) = mode_logical_line_bounds(&mode.revision, mode.cursor.y);
    let row = mode
        .cursor
        .y
        .saturating_add(count.saturating_sub(1))
        .min(mode.revision.total_rows().saturating_sub(1));
    let (_, end) = mode_logical_line_bounds(&mode.revision, row);
    let focus = PointCoordinate {
        x: copy_cursor_limit(&mode.revision, end.y, mode_keys_vi, false),
        y: end.y,
    };
    mode.selection_mode = CopySelectionMode::Char;
    mode.selection = Some(ModeSelection {
        anchor,
        focus,
        mode: SelectionMode::Cell,
        rectangle: false,
    });
    mode.selecting = true;
}

fn select_copy_mode_lines(mode: &mut CopyModeState, count: u32, mode_keys_vi: bool) {
    if count == 0 {
        return;
    }
    let origin = mode.cursor;
    let (anchor, mut focus) = mode_logical_line_bounds(&mode.revision, mode.cursor.y);
    for _ in 1..count {
        let next = focus.y.saturating_add(1);
        if next >= mode.revision.total_rows() {
            break;
        }
        let (_, next_focus) = mode_logical_line_bounds(&mode.revision, next);
        if next_focus.y == focus.y {
            break;
        }
        focus = next_focus;
    }
    focus.x = copy_cursor_limit(&mode.revision, focus.y, mode_keys_vi, false);
    mode.cursor = PointCoordinate {
        x: revision_copy_line_end(&mode.revision, focus.y),
        y: focus.y,
    };
    mode.selection = Some(ModeSelection {
        anchor,
        focus,
        mode: SelectionMode::Line,
        rectangle: false,
    });
    mode.selection_mode = CopySelectionMode::Line;
    mode.selection_origin = origin;
    mode.selecting = true;
    mode.rectangle = false;
    reveal_copy_cursor(mode);
}

/// The pin's `window_copy_goto_line` with line numbers off: the argument is a
/// scrollback offset counted from the bottom, clamped to the retained history,
/// and the cursor keeps the screen row it was on.
fn goto_copy_line(mode: &mut CopyModeState, line: u32) {
    if i32::try_from(line).is_err() {
        return;
    }
    let maximum = mode.revision.maximum_offset();
    let offset = maximum.saturating_sub(line.min(maximum));
    let delta = i64::from(offset) - i64::from(mode.viewport_offset);
    mode.viewport_offset = offset;
    let last = i64::from(mode.revision.total_rows().saturating_sub(1));
    let row = (i64::from(mode.cursor.y) + delta).clamp(0, last);
    mode.cursor.y = u32::try_from(row).unwrap_or(0);
    mode.cursor = mode.revision.clamp_point(mode.cursor);
}

/// The pin's `window_copy_cmd_scroll_to`: move the view so the cursor's line
/// lands on `row`, keeping the cursor on the same line, or do nothing when the
/// retained revision cannot reach that far.
fn scroll_copy_view_to_screen_row(mode: &mut CopyModeState, row: u32) {
    let Some(target) = mode.cursor.y.checked_sub(row) else {
        return;
    };
    if target <= mode.revision.maximum_offset() {
        mode.viewport_offset = target;
    }
}

fn cursor_down_and_cancel(
    terminal: &mut Terminal<'_, '_>,
    selection: &mut Option<SelectionState>,
    copy_mode: &mut CopyModeSlot,
    unseen_output: &mut u32,
    count: u32,
    word_separators: &WordSeparators,
    mode_keys_vi: bool,
) -> Result<ViewActionResult, WorkerError> {
    let Some(start) = copy_mode.as_deref().map(|mode| mode.cursor.y) else {
        return Ok(ViewActionResult::None);
    };
    for _ in 0..count {
        apply_copy_mode_action(
            terminal,
            selection,
            copy_mode,
            unseen_output,
            CopyModeAction::Down,
            word_separators,
            mode_keys_vi,
        )?;
    }
    let Some(mode) = copy_mode.as_deref() else {
        return Ok(ViewActionResult::Snapshot);
    };
    if mode.cursor.y == start && mode.viewport_offset == mode.revision.maximum_offset() {
        *copy_mode = None;
        return cancel_copy_mode(terminal, selection, unseen_output);
    }
    Ok(ViewActionResult::Snapshot)
}

fn copy_mode_scroll_exit_ready(mode: &CopyModeState) -> bool {
    mode.viewport_offset == mode.revision.maximum_offset()
        && mode.selection.is_none()
        && !mode.selecting
}

fn cancel_copy_mode(
    terminal: &mut Terminal<'_, '_>,
    selection: &mut Option<SelectionState>,
    unseen_output: &mut u32,
) -> Result<ViewActionResult, WorkerError> {
    *selection = None;
    terminal.set_selection(None)?;
    terminal.scroll_viewport(ScrollViewport::Bottom);
    *unseen_output = 0;
    Ok(ViewActionResult::Snapshot)
}

/// `grid_reader_cursor_jump`: the scan runs forward from the start point,
/// bounded by each row's used length, and continues onto the row a wrap
/// carried the line onto.
fn reader_jump_forward(
    revision: &ModeRevision,
    mut point: PointCoordinate,
    target: &str,
) -> Option<PointCoordinate> {
    let last = revision.total_rows().saturating_sub(1);
    loop {
        let length = revision_line_length(revision, point.y);
        while point.x < length {
            if revision.cell_matches_text(point, target) {
                return Some(point);
            }
            point.x += 1;
        }
        if point.y >= last || !revision.row(point.y).wrapped() {
            return None;
        }
        point.y += 1;
        point.x = 0;
    }
}

/// `grid_reader_cursor_jump_back`: the scan runs backward from the cell before
/// the start point and continues onto the row a wrap carried the line from,
/// picking that row's used length back up as its right edge.
fn reader_jump_backward(
    revision: &ModeRevision,
    mut point: PointCoordinate,
    target: &str,
) -> Option<PointCoordinate> {
    let mut edge = point.x.saturating_add(1);
    loop {
        let mut column = edge;
        while column > 0 {
            column -= 1;
            let candidate = PointCoordinate {
                x: column,
                y: point.y,
            };
            if revision.cell_matches_text(candidate, target) {
                return Some(candidate);
            }
        }
        if point.y == 0 || !revision.row(point.y - 1).wrapped() {
            return None;
        }
        point.y -= 1;
        edge = revision_line_length(revision, point.y);
    }
}

/// `window_copy_cursor_jump` and its three siblings. The start point is the
/// pin's: one cell on for `jump-forward`, two for `jump-to-forward`, one back
/// for `jump-backward` and two for `jump-to-backward`, and the `to` spellings
/// then take one reader step back toward where they came from.
/// `window_copy_cursor_jump_to_back` is the one that reads mode-keys: its right
/// step is allowed the emacs one-past column, so a target on the last cell of a
/// wrapped row lands past that cell under emacs and on the next row under vi.
fn apply_copy_jump(mode: &mut CopyModeState, jump: &CopyJump, vi: bool) {
    let start = mode.cursor;
    let found = match (jump.direction, jump.to) {
        (CopyJumpDirection::Forward, false) => reader_jump_forward(
            &mode.revision,
            PointCoordinate {
                x: start.x.saturating_add(1),
                y: start.y,
            },
            &jump.target,
        ),
        (CopyJumpDirection::Forward, true) => reader_jump_forward(
            &mode.revision,
            PointCoordinate {
                x: start.x.saturating_add(2),
                y: start.y,
            },
            &jump.target,
        )
        .map(|mut point| {
            reader_cursor_left(&mode.revision, &mut point, true);
            point
        }),
        (CopyJumpDirection::Backward, false) => {
            let mut point = start;
            reader_cursor_left(&mode.revision, &mut point, false);
            reader_jump_backward(&mode.revision, point, &jump.target)
        }
        (CopyJumpDirection::Backward, true) => {
            let mut point = start;
            reader_cursor_left(&mode.revision, &mut point, false);
            reader_cursor_left(&mode.revision, &mut point, false);
            reader_jump_backward(&mode.revision, point, &jump.target).map(|mut point| {
                reader_cursor_right(&mode.revision, &mut point, true, false, !vi);
                point
            })
        }
    };
    if let Some(point) = found {
        mode.cursor = point;
        reveal_copy_cursor(mode);
    }
}
fn move_copy_cursor(
    mode: &mut CopyModeState,
    action: &CopyModeAction,
    word_separators: &WordSeparators,
    vi: bool,
) {
    let mut point = mode.cursor;
    let total = mode.revision.total_rows();
    let page = u32::from(mode.revision.viewport_rows);
    match action {
        CopyModeAction::Left => {
            reader_cursor_left(&mode.revision, &mut point, true);
        }
        CopyModeAction::Right => {
            let all = mode.selection.is_some() && mode.rectangle;
            reader_cursor_right(&mode.revision, &mut point, true, all, !vi);
        }
        CopyModeAction::Up => move_copy_cursor_row(mode, &mut point, false, vi),
        CopyModeAction::Down => move_copy_cursor_row(mode, &mut point, true, vi),
        CopyModeAction::PageUp => point.y = point.y.saturating_sub(page),
        CopyModeAction::PageDown | CopyModeAction::PageDownScrollExit => {
            point.y = point.y.saturating_add(page).min(total - 1);
        }
        CopyModeAction::HalfPageUp => point.y = point.y.saturating_sub((page / 2).max(1)),
        CopyModeAction::HalfPageDown => {
            point.y = point.y.saturating_add((page / 2).max(1)).min(total - 1);
        }
        CopyModeAction::Top => {
            point.x = 0;
            point.y = 0;
        }
        CopyModeAction::Bottom => point.y = total - 1,
        CopyModeAction::CursorCentreVertical => {
            let middle = u32::from(mode.revision.viewport_rows) / 2;
            point.y = mode
                .viewport_offset
                .saturating_add(middle)
                .min(total.saturating_sub(1));
        }
        CopyModeAction::CursorCentreHorizontal => {
            point.x = (mode.revision.columns / 2).min(mode.revision.columns.saturating_sub(1));
        }
        CopyModeAction::TopLine => {
            point.x = 0;
            point.y = mode.viewport_offset;
        }
        CopyModeAction::MiddleLine => {
            point.x = 0;
            point.y = mode
                .viewport_offset
                .saturating_add(page.saturating_sub(1) / 2)
                .min(total - 1);
        }
        CopyModeAction::BottomLine => {
            point.x = 0;
            point.y = mode
                .viewport_offset
                .saturating_add(page.saturating_sub(1))
                .min(total - 1);
        }
        CopyModeAction::StartOfLine => {
            point.y = reader_logical_line_start(&mode.revision, point.y);
            point.x = 0;
        }
        CopyModeAction::BackToIndentation => {
            point.y = reader_logical_line_start(&mode.revision, point.y);
            point.x = revision_first_non_whitespace(&mode.revision, point.y);
        }
        CopyModeAction::EndOfLine => {
            point.y = reader_logical_line_end(&mode.revision, point.y);
            point.x = revision_line_length(&mode.revision, point.y);
        }
        CopyModeAction::NextWordEnd => {
            point = move_revision_word_end(&mode.revision, point, Some(word_separators), vi);
        }
        CopyModeAction::NextSpaceEnd => {
            point = move_revision_word_end(&mode.revision, point, None, vi);
        }
        CopyModeAction::NextWord
        | CopyModeAction::PreviousWord
        | CopyModeAction::NextSpace
        | CopyModeAction::PreviousSpace => {
            point = move_revision_word(&mode.revision, point, action, word_separators);
        }
        CopyModeAction::PreviousMatchingBracket => {
            let latched_vi = mode.mode_keys_vi_at_entry;
            if let Some(target) = revision_matching_bracket(&mode.revision, point, true, latched_vi)
            {
                point = target;
            } else if !latched_vi {
                point = move_revision_word(
                    &mode.revision,
                    point,
                    &CopyModeAction::PreviousWord,
                    &WordSeparators::new(CLOSING_BRACKETS),
                );
            }
        }
        CopyModeAction::NextMatchingBracket => {
            let latched_vi = mode.mode_keys_vi_at_entry;
            if let Some(target) =
                revision_matching_bracket(&mode.revision, point, false, latched_vi)
            {
                point = target;
            } else if !latched_vi {
                point = move_revision_word_end(
                    &mode.revision,
                    point,
                    Some(&WordSeparators::new(OPENING_BRACKETS)),
                    latched_vi,
                );
            }
        }
        CopyModeAction::NextParagraph => {
            point.y = revision_paragraph_target(&mode.revision, point.y, 1);
        }
        CopyModeAction::PreviousParagraph => {
            point.y = revision_paragraph_target(&mode.revision, point.y, -1);
        }
        CopyModeAction::NextPrompt { output } => {
            if let Some(target) =
                revision_semantic_prompt_target(&mode.revision, point.y, 1, *output)
            {
                point = target;
            }
        }
        CopyModeAction::PreviousPrompt { output } => {
            if let Some(target) =
                revision_semantic_prompt_target(&mode.revision, point.y, -1, *output)
            {
                point = target;
            }
        }
        CopyModeAction::JumpToMark => {
            if let Some(mark) = mode.mark {
                point = mark;
            }
        }
        _ => return,
    }
    place_copy_cursor(mode, point, vi);
    reveal_copy_cursor(mode);
}

/// `window_copy_cursor_up` and `window_copy_cursor_down` with `scroll_only`.
/// The view moves one row either way; what moves with it is the mode-keys
/// branch. Under vi the cursor first steps one screen row against the scroll,
/// so it keeps the text line it was on, unless it already sits on the screen
/// row the step would leave. Under emacs it keeps its screen row instead, so
/// the line under it changes.
fn scroll_copy_cursor(mode: &mut CopyModeState, up: bool, vi: bool) {
    let previous = mode.viewport_offset;
    let page = u32::from(mode.revision.viewport_rows.max(1));
    if up {
        mode.viewport_offset = previous.saturating_sub(1);
        if mode.viewport_offset < previous
            && (!vi || mode.cursor.y >= previous.saturating_add(page.saturating_sub(1)))
        {
            mode.cursor.y = mode.cursor.y.saturating_sub(1);
        }
    } else {
        mode.viewport_offset = previous
            .saturating_add(1)
            .min(mode.revision.maximum_offset());
        if mode.viewport_offset > previous && (!vi || mode.cursor.y <= previous) {
            mode.cursor.y = mode
                .cursor
                .y
                .saturating_add(1)
                .min(mode.revision.total_rows().saturating_sub(1));
        }
    }
}

fn reveal_copy_cursor(mode: &mut CopyModeState) {
    let page = u32::from(mode.revision.viewport_rows);
    if mode.cursor.y < mode.viewport_offset {
        mode.viewport_offset = mode.cursor.y;
    } else if mode.cursor.y >= mode.viewport_offset.saturating_add(page) {
        mode.viewport_offset = mode.cursor.y.saturating_sub(page.saturating_sub(1));
    }
    mode.viewport_offset = mode.viewport_offset.min(mode.revision.maximum_offset());
}

fn revision_copy_line_end(revision: &ModeRevision, row: u32) -> u16 {
    for column in (0..revision.columns).rev() {
        if !revision_cell_is_whitespace(revision, PointCoordinate { x: column, y: row }) {
            return column;
        }
    }
    0
}

/// `grid_line_length`: the used width of a row, trailing blanks trimmed, so an
/// empty row answers zero and a full one answers the column count.
fn revision_line_length(revision: &ModeRevision, row: u32) -> u16 {
    for column in (0..revision.columns).rev() {
        if !revision_cell_is_whitespace(revision, PointCoordinate { x: column, y: row }) {
            return column.saturating_add(1);
        }
    }
    0
}

/// `window_copy_cursor_limit`: the rightmost column the copy cursor may hold on
/// one row. emacs parks one past the last cell, vi stops on it, and a
/// rectangle selection is allowed the emacs answer whatever the keys say.
fn copy_cursor_limit(revision: &ModeRevision, row: u32, vi: bool, allow_onemore: bool) -> u16 {
    let length = revision_line_length(revision, row);
    if allow_onemore || !vi {
        length
    } else {
        length.saturating_sub(1)
    }
}

/// `window_copy_update_cursor` clamps every placement to that limit, and skips
/// the clamp outright while a rectangle is being dragged so the cursor can
/// stand past the line the way `virtualedit=block` does.
fn copy_cursor_clamp(mode: &CopyModeState, point: PointCoordinate, vi: bool) -> PointCoordinate {
    let y = point.y.min(mode.revision.total_rows().saturating_sub(1));
    if mode.rectangle {
        return PointCoordinate {
            x: point.x.min(mode.revision.columns),
            y,
        };
    }
    PointCoordinate {
        x: point.x.min(copy_cursor_limit(&mode.revision, y, vi, false)),
        y,
    }
}

fn place_copy_cursor(mode: &mut CopyModeState, point: PointCoordinate, vi: bool) {
    mode.cursor = copy_cursor_clamp(mode, point, vi);
}

fn revision_cell_is_padding(revision: &ModeRevision, point: PointCoordinate) -> bool {
    matches!(
        revision.cell(point).width(),
        CellWidth::SpacerTail | CellWidth::SpacerHead
    )
}

/// `grid_reader_cursor_right` with wrapping: the row's own limit decides where
/// the cursor stops, and reaching it walks onto the start of the next row
/// rather than into the blanks past the text.
fn reader_cursor_right(
    revision: &ModeRevision,
    point: &mut PointCoordinate,
    wrap: bool,
    all: bool,
    onemore: bool,
) {
    let limit = if all {
        revision.columns
    } else {
        copy_cursor_limit(revision, point.y, !onemore, false)
    };
    if wrap && point.x >= limit && point.y.saturating_add(1) < revision.total_rows() {
        point.x = 0;
        point.y += 1;
    } else if point.x < limit {
        point.x += 1;
        while point.x < limit && revision_cell_is_padding(revision, *point) {
            point.x += 1;
        }
    }
}

/// `grid_reader_cursor_left` with wrapping: column zero steps onto the end of
/// the row above, which `window_copy_update_cursor` then clamps.
fn reader_cursor_left(revision: &ModeRevision, point: &mut PointCoordinate, wrap: bool) {
    while point.x > 0 && revision_cell_is_padding(revision, *point) {
        point.x -= 1;
    }
    if point.x == 0 && point.y > 0 && (wrap || revision.row(point.y - 1).wrapped()) {
        point.y -= 1;
        while point.x > 0 && revision_cell_is_padding(revision, *point) {
            point.x -= 1;
        }
        point.x = revision_line_length(revision, point.y);
    } else if point.x > 0 {
        point.x -= 1;
    }
}

/// `grid_reader_cursor_end_of_line(gr, 1, 0)`: a wrapped row belongs to the row
/// below it, so the walk ends on the last row of the logical line.
fn reader_logical_line_end(revision: &ModeRevision, row: u32) -> u32 {
    let mut row = row;
    let last = revision.total_rows().saturating_sub(1);
    while row < last && revision.row(row).wrapped() {
        row += 1;
    }
    row
}

/// `grid_reader_cursor_start_of_line(gr, 1)`: the mirror walk, up through the
/// rows a wrap continued onto.
fn reader_logical_line_start(revision: &ModeRevision, row: u32) -> u32 {
    let mut row = row;
    while row > 0 && revision.row(row - 1).wrapped() {
        row -= 1;
    }
    row
}

/// `grid_reader_handle_wrap`: keep the cursor inside the row's own bounds,
/// stepping onto the next row when it runs past them, and answer false when
/// there is no next row. A wrapped row bounds at the last column rather than at
/// its used width, which is what keeps a wrapped word whole.
fn reader_handle_wrap(
    revision: &ModeRevision,
    point: &mut PointCoordinate,
    bound: &mut u16,
    last_row: u32,
) -> bool {
    while point.x > *bound {
        if point.y >= last_row {
            return false;
        }
        point.x = 0;
        point.y += 1;
        *bound = reader_row_bound(revision, point.y);
    }
    true
}

fn reader_row_bound(revision: &ModeRevision, row: u32) -> u16 {
    if revision.row(row).wrapped() {
        revision.columns.saturating_sub(1)
    } else {
        revision_line_length(revision, row)
    }
}

/// `grid_reader_cursor_next_word_end`: whitespace is stepped over one cell at a
/// time, a run of separators counts as a word, and anything else runs to the
/// first separator or blank. The cursor lands one past the word, which is the
/// emacs answer; vi's wrapper pulls it back.
fn reader_next_word_end(
    revision: &ModeRevision,
    point: &mut PointCoordinate,
    separators: Option<&WordSeparators>,
) {
    let last_row = revision.total_rows().saturating_sub(1);
    let mut bound = reader_row_bound(revision, point.y);
    let is_blank = |point: PointCoordinate| revision_cell_is_whitespace(revision, point);
    let is_separator = |point: PointCoordinate| {
        separators.is_some_and(|separators| {
            revision
                .first_char(point)
                .is_some_and(|character| separators.contains_separator(character))
        })
    };
    while reader_handle_wrap(revision, point, &mut bound, last_row) {
        if is_blank(*point) {
            let Some(next) = point.x.checked_add(1) else {
                return;
            };
            point.x = next;
        } else if is_separator(*point) {
            loop {
                let Some(next) = point.x.checked_add(1) else {
                    return;
                };
                point.x = next;
                if !(reader_handle_wrap(revision, point, &mut bound, last_row)
                    && is_separator(*point)
                    && !is_blank(*point))
                {
                    return;
                }
            }
        } else {
            loop {
                let Some(next) = point.x.checked_add(1) else {
                    return;
                };
                point.x = next;
                if !(reader_handle_wrap(revision, point, &mut bound, last_row)
                    && !(is_blank(*point) || is_separator(*point)))
                {
                    return;
                }
            }
        }
    }
}

/// `window_copy_cursor_next_word_end`: emacs takes the reader's answer, and vi
/// steps off the current cell first and pulls the cursor back onto the last
/// cell of the word afterwards.
fn move_revision_word_end(
    revision: &ModeRevision,
    mut point: PointCoordinate,
    separators: Option<&WordSeparators>,
    vi: bool,
) -> PointCoordinate {
    if vi {
        if !revision_cell_is_whitespace(revision, point) {
            reader_cursor_right(revision, &mut point, false, false, false);
        }
        reader_next_word_end(revision, &mut point, separators);
        reader_cursor_left(revision, &mut point, true);
    } else {
        reader_next_word_end(revision, &mut point, separators);
    }
    point
}

/// `window_copy_cursor_up` and `window_copy_cursor_down` share one desired
/// column: the column the cursor last held on a row it did not fill, kept with
/// that row's own length. On the new row the desired column is clamped to the
/// row's limit and then pushed to the line end whenever it had reached the
/// remembered one.
fn move_copy_cursor_row(
    mode: &mut CopyModeState,
    point: &mut PointCoordinate,
    down: bool,
    vi: bool,
) {
    let rectangle_selection = mode.selection.is_some() && mode.rectangle;
    let length = revision_line_length(&mode.revision, point.y);
    if !rectangle_selection && point.x != length {
        mode.last_cx = point.x;
        mode.last_sx = length;
    }
    let total = mode.revision.total_rows();
    point.y = if down {
        point.y.saturating_add(1).min(total.saturating_sub(1))
    } else {
        point.y.saturating_sub(1)
    };
    if rectangle_selection {
        return;
    }
    point.x = mode.last_cx;
    *point = copy_cursor_clamp(mode, *point, vi);
    let end = revision_line_length(&mode.revision, point.y);
    if (point.x >= mode.last_sx && point.x != end) || point.x > end {
        point.x = end;
    }
}

/// window-copy.c's `close` and `open` bracket strings, which the emacs
/// branches of the two matching-bracket commands also hand to their word
/// fallbacks as the separator set.
const CLOSING_BRACKETS: &str = "}])";
const OPENING_BRACKETS: &str = "{[(";

/// `window_copy_cmd_previous_matching_bracket` and its next twin, both reading
/// `data->modekeys` as it stood when the mode was entered. vi scans the logical
/// line for a bracket and, going forward, walks a closing one back to its
/// opener. emacs looks at the cursor cell and then at exactly one neighbour,
/// accepts only the direction's own bracket class, and leaves the word fallback
/// to the caller.
fn revision_matching_bracket(
    revision: &ModeRevision,
    point: PointCoordinate,
    backward: bool,
    vi: bool,
) -> Option<PointCoordinate> {
    let columns = u64::from(revision.columns);
    let total = columns.saturating_mul(u64::from(revision.total_rows()));
    let to_point = |index: u64| PointCoordinate {
        x: u16::try_from(index % columns).unwrap_or(0),
        y: u32::try_from(index / columns).unwrap_or(u32::MAX),
    };
    let char_at = |index: u64| revision.first_char(to_point(index));
    let origin = u64::from(point.y)
        .saturating_mul(columns)
        .saturating_add(u64::from(point.x))
        .min(total.saturating_sub(1));

    let mut bracket = origin;
    let is_target = |character: Option<char>| {
        if backward {
            matches!(character, Some(')' | ']' | '}'))
        } else if vi {
            matches!(character, Some('(' | ')' | '[' | ']' | '{' | '}'))
        } else {
            matches!(character, Some('(' | '[' | '{'))
        }
    };
    if !vi {
        if is_target(char_at(origin)) {
            bracket = origin;
        } else {
            let neighbour = if backward {
                (point.x > 0).then(|| origin.saturating_sub(1))
            } else {
                (point.x.saturating_add(1) < revision.columns).then(|| origin.saturating_add(1))
            };
            let neighbour = neighbour.filter(|index| is_target(char_at(*index)))?;
            bracket = neighbour;
        }
    }
    while !is_target(char_at(bracket)) {
        let at = to_point(bracket);
        let reaches_further = if backward {
            at.x > 0 || (at.y > 0 && revision.row(at.y).continuation())
        } else {
            at.x.saturating_add(1) < revision.columns
                || (revision.row(at.y).wrapped() && at.y.saturating_add(1) < revision.total_rows())
        };
        if !reaches_further {
            return None;
        }
        bracket = if backward {
            bracket.saturating_sub(1)
        } else {
            bracket.saturating_add(1)
        };
    }

    let found = char_at(bracket)?;
    let (open, close, forward) = match found {
        '(' => ('(', ')', true),
        '[' => ('[', ']', true),
        '{' => ('{', '}', true),
        ')' => ('(', ')', false),
        ']' => ('[', ']', false),
        '}' => ('{', '}', false),
        _ => return None,
    };
    let mut depth = 1_u32;
    if forward {
        for index in bracket.saturating_add(1)..total {
            match char_at(index) {
                Some(character) if character == open => depth = depth.saturating_add(1),
                Some(character) if character == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(to_point(index));
                    }
                }
                _ => {}
            }
        }
    } else {
        for index in (0..bracket).rev() {
            match char_at(index) {
                Some(character) if character == close => depth = depth.saturating_add(1),
                Some(character) if character == open => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(to_point(index));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn revision_first_non_whitespace(revision: &ModeRevision, row: u32) -> u16 {
    (0..revision.columns)
        .find(|column| {
            !revision_cell_is_whitespace(revision, PointCoordinate { x: *column, y: row })
        })
        .unwrap_or(0)
}

fn revision_row_is_blank(revision: &ModeRevision, row: u32) -> bool {
    (0..revision.columns)
        .all(|column| revision_cell_is_whitespace(revision, PointCoordinate { x: column, y: row }))
}

fn revision_cell_is_whitespace(revision: &ModeRevision, point: PointCoordinate) -> bool {
    revision.first_char(point).is_none_or(char::is_whitespace)
}

fn revision_paragraph_target(revision: &ModeRevision, row: u32, direction: i8) -> u32 {
    let total = revision.total_rows();
    if direction.is_negative() {
        let mut target = row.saturating_sub(1);
        while target > 0 && revision_row_is_blank(revision, target) {
            target -= 1;
        }
        while target > 0 && !revision_row_is_blank(revision, target - 1) {
            target -= 1;
        }
        target
    } else {
        let mut target = row.saturating_add(1).min(total - 1);
        while target + 1 < total && !revision_row_is_blank(revision, target) {
            target += 1;
        }
        while target + 1 < total && revision_row_is_blank(revision, target) {
            target += 1;
        }
        target
    }
}

fn mode_logical_line_bounds(
    revision: &ModeRevision,
    row: u32,
) -> (PointCoordinate, PointCoordinate) {
    let mut start = row;
    while start > 0 && revision.row(start).continuation() {
        start -= 1;
    }
    let mut end = row;
    while end + 1 < revision.total_rows() && revision.row(end).wrapped() {
        end += 1;
    }
    (
        PointCoordinate { x: 0, y: start },
        PointCoordinate {
            x: revision.columns.saturating_sub(1),
            y: end,
        },
    )
}

fn revision_word_class(
    revision: &ModeRevision,
    point: PointCoordinate,
    word_separators: &WordSeparators,
) -> CopyWordClass {
    character_word_class(revision.first_char(point), word_separators)
}

fn character_word_class(
    character: Option<char>,
    word_separators: &WordSeparators,
) -> CopyWordClass {
    match character {
        None => CopyWordClass::Whitespace,
        Some(character) if character.is_whitespace() => CopyWordClass::Whitespace,
        Some(character) if word_separators.contains_separator(character) => {
            CopyWordClass::Separator
        }
        Some(_) => CopyWordClass::Word,
    }
}

fn move_revision_word(
    revision: &ModeRevision,
    point: PointCoordinate,
    action: &CopyModeAction,
    word_separators: &WordSeparators,
) -> PointCoordinate {
    let columns = u64::from(revision.columns);
    let total = columns
        .saturating_mul(u64::from(revision.total_rows()))
        .max(1);
    let index = u64::from(point.y)
        .saturating_mul(columns)
        .saturating_add(u64::from(point.x))
        .min(total - 1);
    let spaces_only = matches!(
        action,
        CopyModeAction::NextSpace | CopyModeAction::PreviousSpace | CopyModeAction::NextSpaceEnd
    );
    let class_at = |index: u64| {
        let point = PointCoordinate {
            x: u16::try_from(index % columns).unwrap_or(0),
            y: u32::try_from(index / columns).unwrap_or(u32::MAX),
        };
        if spaces_only {
            match revision.first_char(point) {
                None => CopyWordClass::Whitespace,
                Some(character) if character.is_whitespace() => CopyWordClass::Whitespace,
                Some(_) => CopyWordClass::Word,
            }
        } else {
            revision_word_class(revision, point, word_separators)
        }
    };
    let destination = match action {
        CopyModeAction::NextWord | CopyModeAction::NextSpace => {
            let current = class_at(index);
            let mut next = index;
            if current != CopyWordClass::Whitespace {
                next = next.saturating_add(1).min(total - 1);
                while next < total && class_at(next) == current {
                    next += 1;
                }
            }
            while next < total && class_at(next) == CopyWordClass::Whitespace {
                next += 1;
            }
            next.min(total - 1)
        }
        CopyModeAction::PreviousWord | CopyModeAction::PreviousSpace => {
            let mut previous = index.saturating_sub(1);
            while previous > 0 && class_at(previous) == CopyWordClass::Whitespace {
                previous -= 1;
            }
            let class = class_at(previous);
            while previous > 0 && class_at(previous - 1) == class {
                previous -= 1;
            }
            previous
        }
        CopyModeAction::NextWordEnd | CopyModeAction::NextSpaceEnd => {
            let mut end = index.saturating_add(1).min(total - 1);
            while end + 1 < total && class_at(end) == CopyWordClass::Whitespace {
                end += 1;
            }
            let class = class_at(end);
            while end + 1 < total && class_at(end + 1) == class {
                end += 1;
            }
            end
        }
        _ => index,
    };
    PointCoordinate {
        x: u16::try_from(destination % columns).unwrap_or(0),
        y: u32::try_from(destination / columns).unwrap_or(u32::MAX),
    }
}

fn revision_semantic_prompt_target(
    revision: &ModeRevision,
    current_row: u32,
    direction: i8,
    output: bool,
) -> Option<PointCoordinate> {
    let total = revision.total_rows();
    let prompt_row = if direction.is_negative() {
        (0..current_row)
            .rev()
            .find(|row| revision.row(*row).prompt())
    } else {
        (current_row.saturating_add(1)..total).find(|row| revision.row(*row).prompt())
    }?;
    if output {
        return revision_semantic_output_target(revision, prompt_row);
    }
    Some(PointCoordinate {
        x: (0..revision.columns)
            .find(|column| {
                revision.is_prompt(PointCoordinate {
                    x: *column,
                    y: prompt_row,
                })
            })
            .unwrap_or(0),
        y: prompt_row,
    })
}

fn revision_semantic_output_target(
    revision: &ModeRevision,
    prompt_row: u32,
) -> Option<PointCoordinate> {
    let mut saw_input = false;
    for row in prompt_row..revision.total_rows() {
        if row > prompt_row && revision.row(row).prompt() {
            break;
        }
        for column in 0..revision.columns {
            let point = PointCoordinate { x: column, y: row };
            if revision.first_char(point).is_none() {
                continue;
            }
            if revision.is_input(point) {
                saw_input = true;
            } else if saw_input && revision.is_output(point) {
                return Some(point);
            }
        }
    }
    None
}

#[cfg(test)]
fn semantic_prompt_target(
    terminal: &Terminal<'_, '_>,
    current_row: u32,
    direction: i8,
    output: bool,
) -> Result<Option<PointCoordinate>, WorkerError> {
    let total = u32::try_from(terminal.total_rows()?).unwrap_or(u32::MAX);
    let prompt_row = if direction.is_negative() {
        let mut row = current_row;
        let mut found = None;
        while row > 0 {
            row -= 1;
            if row_semantic_prompt(terminal, row)? == RowSemanticPrompt::Prompt {
                found = Some(row);
                break;
            }
        }
        found
    } else {
        let mut row = current_row.saturating_add(1);
        let mut found = None;
        while row < total {
            if row_semantic_prompt(terminal, row)? == RowSemanticPrompt::Prompt {
                found = Some(row);
                break;
            }
            row += 1;
        }
        found
    };
    let Some(prompt_row) = prompt_row else {
        return Ok(None);
    };
    if output {
        return semantic_output_target(terminal, prompt_row, total);
    }
    Ok(Some(PointCoordinate {
        x: first_semantic_cell(terminal, prompt_row, CellSemanticContent::Prompt)?.unwrap_or(0),
        y: prompt_row,
    }))
}

fn semantic_output_target(
    terminal: &Terminal<'_, '_>,
    prompt_row: u32,
    total: u32,
) -> Result<Option<PointCoordinate>, WorkerError> {
    let columns = terminal.cols()?;
    let mut saw_input = false;
    for row in prompt_row..total {
        if row > prompt_row && row_semantic_prompt(terminal, row)? == RowSemanticPrompt::Prompt {
            break;
        }
        for column in 0..columns {
            let grid_ref =
                terminal.grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))?;
            let cell = grid_ref.cell()?;
            match cell.semantic_content()? {
                CellSemanticContent::Input if cell.has_text()? => saw_input = true,
                CellSemanticContent::Output if saw_input && cell.has_text()? => {
                    return Ok(Some(PointCoordinate { x: column, y: row }));
                }
                CellSemanticContent::Output
                | CellSemanticContent::Input
                | CellSemanticContent::Prompt => {}
            }
        }
    }
    Ok(None)
}

fn row_semantic_prompt(
    terminal: &Terminal<'_, '_>,
    row: u32,
) -> Result<RowSemanticPrompt, WorkerError> {
    Ok(terminal
        .grid_ref(Point::Screen(PointCoordinate { x: 0, y: row }))?
        .row()?
        .semantic_prompt()?)
}

#[cfg(test)]
fn first_semantic_cell(
    terminal: &Terminal<'_, '_>,
    row: u32,
    semantic: CellSemanticContent,
) -> Result<Option<u16>, WorkerError> {
    for column in 0..terminal.cols()? {
        let grid_ref = terminal.grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))?;
        let cell = grid_ref.cell()?;
        if cell.has_text()? && cell.semantic_content()? == semantic {
            return Ok(Some(column));
        }
    }
    Ok(None)
}

fn select_semantic_output_at(
    terminal: &Terminal<'_, '_>,
    selection_state: &mut Option<SelectionState>,
    event: PointerCellEvent,
) -> Result<bool, WorkerError> {
    let clicked = terminal.grid_ref(viewport_point(terminal, event)?)?;
    let Some(clicked) = terminal.point_from_grid_ref(&clicked, PointSpace::Screen)? else {
        return Ok(false);
    };
    let mut prompt_row = clicked.y;
    loop {
        if row_semantic_prompt(terminal, prompt_row)? == RowSemanticPrompt::Prompt {
            break;
        }
        if prompt_row == 0 {
            return Ok(false);
        }
        prompt_row -= 1;
    }
    let total = u32::try_from(terminal.total_rows()?).unwrap_or(u32::MAX);
    let Some(start) = semantic_output_target(terminal, prompt_row, total)? else {
        return Ok(false);
    };
    let columns = terminal.cols()?;
    let mut end = None;
    for row in start.y..total {
        if row > start.y && row_semantic_prompt(terminal, row)? == RowSemanticPrompt::Prompt {
            break;
        }
        let first_column = if row == start.y { start.x } else { 0 };
        for column in first_column..columns {
            let grid_ref =
                terminal.grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))?;
            let cell = grid_ref.cell()?;
            if cell.has_text()? && cell.semantic_content()? == CellSemanticContent::Output {
                end = Some(PointCoordinate { x: column, y: row });
            }
        }
    }
    let Some(end) = end else {
        return Ok(false);
    };
    let anchor = terminal.track_grid_ref(Point::Screen(start))?;
    let focus = terminal.track_grid_ref(Point::Screen(end))?;
    let start_ref = terminal.grid_ref(Point::Screen(start))?;
    let end_ref = terminal.grid_ref(Point::Screen(end))?;
    terminal.set_selection(Some(&Selection::new(start_ref, end_ref, false)))?;
    *selection_state = Some(SelectionState {
        anchor,
        focus,
        mode: SelectionMode::Cell,
        rectangle: false,
    });
    Ok(true)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CopyWordClass {
    Whitespace,
    Word,
    Separator,
}

#[cfg(test)]
fn copy_line_end(terminal: &Terminal<'_, '_>, row: u32, columns: u16) -> Result<u16, WorkerError> {
    for column in (0..columns).rev() {
        if copy_word_class(terminal, PointCoordinate { x: column, y: row })?
            != CopyWordClass::Whitespace
        {
            return Ok(column);
        }
    }
    Ok(0)
}

#[cfg(test)]
fn move_copy_word(
    terminal: &Terminal<'_, '_>,
    point: PointCoordinate,
    columns: u16,
    rows: u32,
    action: &CopyModeAction,
) -> Result<PointCoordinate, WorkerError> {
    let columns_u64 = u64::from(columns);
    let total = columns_u64.saturating_mul(u64::from(rows)).max(1);
    let index = u64::from(point.y)
        .saturating_mul(columns_u64)
        .saturating_add(u64::from(point.x))
        .min(total - 1);
    let class_at = |index: u64| {
        copy_word_class(
            terminal,
            PointCoordinate {
                x: u16::try_from(index % columns_u64).unwrap_or(0),
                y: u32::try_from(index / columns_u64).unwrap_or(u32::MAX),
            },
        )
    };

    let destination = match action {
        CopyModeAction::NextWord => {
            let current = class_at(index)?;
            let mut next = index;
            if current != CopyWordClass::Whitespace {
                next = next.saturating_add(1).min(total - 1);
                while next < total && class_at(next)? == current {
                    next += 1;
                }
            }
            while next < total && class_at(next)? == CopyWordClass::Whitespace {
                next += 1;
            }
            next.min(total - 1)
        }
        CopyModeAction::PreviousWord => {
            let mut previous = index.saturating_sub(1);
            while previous > 0 && class_at(previous)? == CopyWordClass::Whitespace {
                previous -= 1;
            }
            let class = class_at(previous)?;
            while previous > 0 && class_at(previous - 1)? == class {
                previous -= 1;
            }
            previous
        }
        CopyModeAction::NextWordEnd => {
            let mut end = index.saturating_add(1).min(total - 1);
            while end + 1 < total && class_at(end)? == CopyWordClass::Whitespace {
                end += 1;
            }
            let class = class_at(end)?;
            while end + 1 < total && class_at(end + 1)? == class {
                end += 1;
            }
            end
        }
        _ => index,
    };
    Ok(PointCoordinate {
        x: u16::try_from(destination % columns_u64).unwrap_or(0),
        y: u32::try_from(destination / columns_u64).unwrap_or(u32::MAX),
    })
}

fn write_focus_event(
    terminal: &Terminal<'_, '_>,
    focused: bool,
    writer: &mut dyn Write,
) -> Result<(), WorkerError> {
    if !terminal.mode(Mode::FOCUS_EVENT)? {
        return Ok(());
    }
    let event = if focused {
        focus::Event::Gained
    } else {
        focus::Event::Lost
    };
    let mut encoded = [0_u8; 8];
    let written = event.encode(&mut encoded)?;
    writer.write_all(&encoded[..written])?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
fn copy_word_class(
    terminal: &Terminal<'_, '_>,
    point: PointCoordinate,
) -> Result<CopyWordClass, WorkerError> {
    let grid_ref = terminal.grid_ref(Point::Screen(point))?;
    let mut stack = ['\0'; 8];
    let first = match grid_ref.graphemes(&mut stack) {
        Ok(count) => stack
            .get(..count)
            .and_then(|graphemes| graphemes.first())
            .copied(),
        Err(libghostty_vt::Error::OutOfSpace { required }) => {
            let mut graphemes = vec!['\0'; required];
            let count = grid_ref.graphemes(&mut graphemes)?;
            graphemes
                .get(..count)
                .and_then(|values| values.first())
                .copied()
        }
        Err(error) => return Err(error.into()),
    };
    Ok(match first {
        None => CopyWordClass::Whitespace,
        Some(character) if character.is_whitespace() => CopyWordClass::Whitespace,
        Some(character) if WordSeparators::default().contains_separator(character) => {
            CopyWordClass::Separator
        }
        Some(_) => CopyWordClass::Word,
    })
}

fn update_copy_selection(mode: &mut CopyModeState, word_separators: Option<&WordSeparators>) {
    let unit = mode.selection_mode;
    let origin = mode.selection_origin;
    let cursor = mode.cursor;
    let revision = Arc::clone(&mode.revision);
    let Some(selection) = mode.selection.as_mut() else {
        return;
    };
    selection.focus = cursor;
    match unit {
        CopySelectionMode::Char => {}
        CopySelectionMode::Word => {
            let Some(separators) = word_separators else {
                return;
            };
            if (cursor.y, cursor.x) >= (origin.y, origin.x) {
                selection.anchor = mode_word_edge(&revision, origin, separators, false);
                selection.focus = mode_word_edge(&revision, cursor, separators, true);
            } else {
                selection.anchor = mode_word_edge(&revision, origin, separators, true);
                selection.focus = mode_word_edge(&revision, cursor, separators, false);
            }
            selection.mode = SelectionMode::Word;
        }
        CopySelectionMode::Line => {
            let (origin_start, origin_end) = mode_logical_line_bounds(&revision, origin.y);
            let (cursor_start, cursor_end) = mode_logical_line_bounds(&revision, cursor.y);
            if cursor.y >= origin.y {
                selection.anchor = origin_start;
                selection.focus = cursor_end;
            } else {
                selection.anchor = origin_end;
                selection.focus = cursor_start;
            }
            selection.mode = SelectionMode::Line;
        }
    }
}

fn format_selection_text(terminal: &Terminal<'_, '_>) -> Result<String, WorkerError> {
    let options = FormatOptions::new()
        .with_emit_format(Format::Plain)
        .with_unwrap(true)
        .with_trim(true);
    Ok(terminal
        .format_selection_alloc(None, options)?
        .map_or_else(String::new, |bytes| {
            String::from_utf8_lossy(bytes.as_ref()).into_owned()
        }))
}

const MAX_SEARCH_QUERY_BYTES: usize = 4096;
const MAX_SEARCH_MATCHES: usize = 1_000_000;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct SearchCellOffset {
    start: u32,
    end: u32,
    column: u16,
    width: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct HistorySearchRow {
    text_start: u32,
    text_end: u32,
    offset_start: u32,
    offset_end: u32,
}

#[derive(Debug)]
struct HistorySearchSnapshot {
    columns: u16,
    text: String,
    rows: Vec<HistorySearchRow>,
    offsets: Vec<SearchCellOffset>,
}

#[derive(Clone, Copy)]
enum SearchSelectionPolicy {
    Last,
    From {
        point: PointCoordinate,
        direction: SearchDirection,
    },
    Preserve {
        index: Option<usize>,
        found: Option<SearchMatch>,
    },
}

struct SearchJob {
    request_id: u64,
    view_id: TerminalViewId,
    screen: Screen,
    query: SearchQuery,
    snapshot: Arc<HistorySearchSnapshot>,
    selection: SearchSelectionPolicy,
    match_scratch: Vec<SearchMatch>,
    latest_request: Arc<AtomicU64>,
}

struct SearchResult {
    request_id: u64,
    view_id: TerminalViewId,
    screen: Screen,
    state: SearchState,
}

#[derive(Default)]
struct SearchJobs {
    by_view: HashMap<TerminalViewId, SearchJob>,
}

#[derive(Default)]
struct SearchResults {
    by_view: HashMap<TerminalViewId, SearchResult>,
}

struct SearchWorker {
    jobs: Sender<SearchJobs>,
    discard_jobs: Receiver<SearchJobs>,
    latest_requests: HashMap<TerminalViewId, Arc<AtomicU64>>,
    next_request: u64,
    match_scratch: Vec<SearchMatch>,
}

impl SearchWorker {
    fn spawn(wake: ActorWake) -> Result<(Self, Receiver<SearchResults>), WorkerError> {
        let (jobs, job_rx) = crossbeam_channel::bounded::<SearchJobs>(1);
        let discard_jobs = job_rx.clone();
        let (result_tx, results) = crossbeam_channel::bounded::<SearchResults>(1);
        let discard_results = results.clone();
        thread::Builder::new()
            .name("zz-terminal-search".into())
            .spawn(move || {
                search_worker(&job_rx, &result_tx, &discard_results, &wake);
            })
            .map_err(WorkerError::Io)?;
        Ok((
            Self {
                jobs,
                discard_jobs,
                latest_requests: HashMap::new(),
                next_request: 0,
                match_scratch: Vec::new(),
            },
            results,
        ))
    }

    fn next_request(&mut self, view_id: TerminalViewId) -> (u64, Arc<AtomicU64>) {
        self.next_request = self.next_request.wrapping_add(1).max(1);
        let latest = self
            .latest_requests
            .entry(view_id)
            .or_insert_with(|| Arc::new(AtomicU64::new(0)));
        latest.store(self.next_request, Ordering::Release);
        (self.next_request, Arc::clone(latest))
    }

    fn cancel(&mut self, view_id: TerminalViewId) -> u64 {
        let (request_id, _) = self.next_request(view_id);
        if let Ok(mut pending) = self.discard_jobs.try_recv() {
            if let Some(mut discarded) = pending.by_view.remove(&view_id) {
                keep_larger_match_scratch(&mut self.match_scratch, &mut discarded.match_scratch);
            }
            if !pending.by_view.is_empty() {
                let _ = self.jobs.try_send(pending);
            }
        }
        request_id
    }

    fn forget(&mut self, view_id: TerminalViewId) {
        self.cancel(view_id);
        self.latest_requests.remove(&view_id);
    }

    fn is_current(&self, view_id: TerminalViewId, request_id: u64) -> bool {
        self.latest_requests
            .get(&view_id)
            .is_some_and(|latest| latest.load(Ordering::Acquire) == request_id)
    }

    fn recycle_matches(&mut self, matches: &mut Vec<SearchMatch>) {
        keep_larger_match_scratch(&mut self.match_scratch, matches);
    }

    fn submit(&mut self, job: SearchJob) {
        let mut pending = SearchJobs::default();
        pending.by_view.insert(job.view_id, job);
        loop {
            match self.jobs.try_send(pending) {
                Ok(()) | Err(crossbeam_channel::TrySendError::Disconnected(_)) => return,
                Err(crossbeam_channel::TrySendError::Full(returned)) => {
                    pending = returned;
                    if let Ok(older) = self.discard_jobs.try_recv() {
                        merge_older_search_jobs(&mut pending, older);
                    }
                }
            }
        }
    }
}

fn merge_older_search_jobs(newer: &mut SearchJobs, older: SearchJobs) {
    for (view_id, mut job) in older.by_view {
        if let Some(replacement) = newer.by_view.get_mut(&view_id) {
            keep_larger_match_scratch(&mut replacement.match_scratch, &mut job.match_scratch);
        } else {
            newer.by_view.insert(view_id, job);
        }
    }
}

fn merge_newer_search_jobs(
    current: &mut SearchJobs,
    newer: SearchJobs,
    match_scratch: &mut Vec<SearchMatch>,
) {
    for (view_id, job) in newer.by_view {
        if let Some(mut discarded) = current.by_view.insert(view_id, job) {
            keep_larger_match_scratch(match_scratch, &mut discarded.match_scratch);
        }
    }
}

fn search_worker(
    jobs: &Receiver<SearchJobs>,
    results: &Sender<SearchResults>,
    discard_results: &Receiver<SearchResults>,
    wake: &ActorWake,
) {
    let mut match_scratch = Vec::new();
    while let Ok(mut pending) = jobs.recv() {
        while let Ok(newer) = jobs.try_recv() {
            merge_newer_search_jobs(&mut pending, newer, &mut match_scratch);
        }
        let mut pending = pending.by_view.into_values().collect::<Vec<_>>();
        pending.sort_by_key(|job| job.view_id.0);
        let mut completed = SearchResults::default();
        for mut job in pending {
            keep_larger_match_scratch(&mut match_scratch, &mut job.match_scratch);
            if job.latest_request.load(Ordering::Acquire) != job.request_id {
                continue;
            }
            let Some(mut state) =
                job.snapshot
                    .search_reusing(&job.query, job.request_id, &mut match_scratch, || {
                        job.latest_request.load(Ordering::Acquire) != job.request_id
                    })
            else {
                continue;
            };
            select_search_result(&mut state, job.selection);
            if job.latest_request.load(Ordering::Acquire) != job.request_id {
                keep_larger_match_scratch(&mut match_scratch, &mut state.matches);
                continue;
            }
            completed.by_view.insert(
                job.view_id,
                SearchResult {
                    request_id: job.request_id,
                    view_id: job.view_id,
                    screen: job.screen,
                    state,
                },
            );
        }
        if completed.by_view.is_empty() {
            continue;
        }
        if !send_latest_search_results(results, discard_results, completed, &mut match_scratch) {
            return;
        }
        wake.notify();
    }
}

fn send_latest_search_results(
    results: &Sender<SearchResults>,
    discard_results: &Receiver<SearchResults>,
    mut completed: SearchResults,
    match_scratch: &mut Vec<SearchMatch>,
) -> bool {
    loop {
        match results.try_send(completed) {
            Ok(()) => return true,
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => return false,
            Err(crossbeam_channel::TrySendError::Full(returned)) => {
                completed = returned;
                if let Ok(older) = discard_results.try_recv() {
                    for (view_id, mut result) in older.by_view {
                        match completed.by_view.entry(view_id) {
                            Entry::Occupied(_) => {
                                keep_larger_match_scratch(match_scratch, &mut result.state.matches);
                            }
                            Entry::Vacant(entry) => {
                                entry.insert(result);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn keep_larger_match_scratch(target: &mut Vec<SearchMatch>, candidate: &mut Vec<SearchMatch>) {
    target.clear();
    candidate.clear();
    if candidate.capacity() > target.capacity() {
        std::mem::swap(target, candidate);
    }
}

fn select_search_result(state: &mut SearchState, selection: SearchSelectionPolicy) {
    if state.matches.is_empty() {
        return;
    }
    state.current = match selection {
        SearchSelectionPolicy::Last => Some(state.matches.len() - 1),
        SearchSelectionPolicy::From { point, direction } => match direction {
            SearchDirection::Forward => state
                .matches
                .iter()
                .position(|found| (found.row, found.start) > (point.y, point.x))
                .or(Some(0)),
            SearchDirection::Backward => state
                .matches
                .iter()
                .rposition(|found| (found.row, found.start) < (point.y, point.x))
                .or(Some(state.matches.len() - 1)),
        },
        SearchSelectionPolicy::Preserve { index, found } => found
            .and_then(|found| {
                state
                    .matches
                    .iter()
                    .position(|candidate| *candidate == found)
            })
            .or_else(|| index.map(|index| index.min(state.matches.len() - 1)))
            .or_else(|| Some(state.matches.len() - 1)),
    };
}

struct SearchTarget<'a> {
    view_id: TerminalViewId,
    screen: Screen,
    search: &'a mut SearchSlot,
    snapshot: &'a mut Option<Arc<HistorySearchSnapshot>>,
}

fn search_selection_policy(
    copy_mode: Option<&CopyModeState>,
    origin: &mut Option<PointCoordinate>,
    direction: SearchDirection,
    reset_origin: bool,
) -> SearchSelectionPolicy {
    if reset_origin {
        *origin = None;
    }
    if origin.is_none()
        && let Some(mode) = copy_mode
    {
        *origin = Some(mode.cursor);
    }
    let Some(origin) = origin.as_ref() else {
        return SearchSelectionPolicy::Last;
    };
    SearchSelectionPolicy::From {
        point: *origin,
        direction,
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "the target bundles exclusive view-state borrows for one queued request"
)]
fn queue_search(
    terminal: &Terminal<'_, '_>,
    target: SearchTarget<'_>,
    worker: &mut SearchWorker,
    query: SearchQuery,
    selection: SearchSelectionPolicy,
) -> Result<(), WorkerError> {
    let (request_id, latest_request) = worker.next_request(target.view_id);
    let searchable = !query.text.is_empty() && query.text.len() <= MAX_SEARCH_QUERY_BYTES;
    let invalid_pattern = query.text.len() > MAX_SEARCH_QUERY_BYTES;
    if searchable && target.snapshot.is_none() {
        *target.snapshot = Some(Arc::new(HistorySearchSnapshot::capture(terminal)?));
    }
    let mut match_scratch = target
        .search
        .as_mut()
        .map_or_else(Vec::new, |state| std::mem::take(&mut state.matches));
    store_search_state(
        target.search,
        SearchState {
            query: query.clone(),
            matches: if searchable {
                Vec::new()
            } else {
                std::mem::take(&mut match_scratch)
            },
            request_id,
            pending: searchable,
            invalid_pattern,
            ..SearchState::default()
        },
    );
    if !searchable {
        return Ok(());
    }
    worker.submit(SearchJob {
        request_id,
        view_id: target.view_id,
        screen: target.screen,
        query,
        snapshot: Arc::clone(target.snapshot.as_ref().expect("captured above")),
        selection,
        match_scratch,
        latest_request,
    });
    Ok(())
}

fn refresh_view_search(
    terminal: &Terminal<'_, '_>,
    view_id: TerminalViewId,
    view: &mut TerminalViewState,
    worker: &mut SearchWorker,
) -> Result<bool, WorkerError> {
    let Some(previous) = view.search.as_ref() else {
        worker.cancel(view_id);
        return Ok(false);
    };
    let query = previous.query.clone();
    let selection = SearchSelectionPolicy::Preserve {
        index: previous.current,
        found: previous
            .current
            .and_then(|index| previous.matches.get(index))
            .copied(),
    };
    let screen = view.screen;
    let current = view.active_mut();
    if current.search_snapshot.is_none()
        && let Some(mode) = current.copy_mode.as_ref()
    {
        current.search_snapshot = Some(Arc::clone(&mode.revision.search));
    }
    queue_search(
        terminal,
        SearchTarget {
            view_id,
            screen,
            search: &mut current.search,
            snapshot: &mut current.search_snapshot,
        },
        worker,
        query,
        selection,
    )?;
    Ok(true)
}

fn apply_search_result(
    terminal: &mut Terminal<'_, '_>,
    active: &mut ActiveTerminalViews,
    inactive: &mut InactiveTerminalViews,
    worker: &mut SearchWorker,
    mut result: SearchResult,
) -> Result<bool, WorkerError> {
    if !worker.is_current(result.view_id, result.request_id) {
        worker.recycle_matches(&mut result.state.matches);
        return Ok(false);
    }
    let is_active = active.contains_key(&result.view_id);
    let view = if let Some(view) = active.get_mut(&result.view_id) {
        view
    } else if let Some(view) = inactive.get_mut(&result.view_id) {
        view
    } else {
        worker.recycle_matches(&mut result.state.matches);
        return Ok(false);
    };
    let current_screen = view.screen;
    let screen = view.screen_mut(result.screen);
    let Some(pending) = screen.search.as_mut() else {
        worker.recycle_matches(&mut result.state.matches);
        return Ok(false);
    };
    if pending.request_id != result.request_id || pending.query != result.state.query {
        worker.recycle_matches(&mut result.state.matches);
        return Ok(false);
    }
    worker.recycle_matches(&mut pending.matches);
    store_search_state(&mut screen.search, result.state);
    if !is_active || current_screen != result.screen {
        return Ok(false);
    }
    {
        let current = view.active_mut();
        sync_copy_cursor_to_search(&mut current.copy_mode, current.search.as_deref());
        reveal_search_for_view(
            terminal,
            current.copy_mode.as_deref(),
            current.search.as_deref(),
        )?;
    }
    sync_viewport_anchor(terminal, view)?;
    Ok(true)
}

fn apply_search_results(
    terminal: &mut Terminal<'_, '_>,
    active: &mut ActiveTerminalViews,
    inactive: &mut InactiveTerminalViews,
    worker: &mut SearchWorker,
    results: SearchResults,
) -> Result<bool, WorkerError> {
    let mut results = results.by_view.into_values().collect::<Vec<_>>();
    results.sort_by_key(|result| result.view_id.0);
    let mut changed = false;
    for result in results {
        changed |= apply_search_result(terminal, active, inactive, worker, result)?;
    }
    Ok(changed)
}

fn complete_view_search(
    terminal: &mut Terminal<'_, '_>,
    view: &mut TerminalViewState,
) -> Result<(), WorkerError> {
    let Some(previous) = view.search.as_ref() else {
        return Ok(());
    };
    let query = previous.query.clone();
    let request_id = previous.request_id;
    let selection = SearchSelectionPolicy::Preserve {
        index: previous.current,
        found: previous
            .current
            .and_then(|index| previous.matches.get(index))
            .copied(),
    };
    let snapshot = view.copy_mode.as_ref().map_or_else(
        || HistorySearchSnapshot::capture(terminal).map(Arc::new),
        |mode| Ok(Arc::clone(&mode.revision.search)),
    )?;
    let mut state = snapshot
        .search(&query, request_id, || false)
        .expect("inline terminal search is never cancelled");
    select_search_result(&mut state, selection);
    view.search_snapshot = Some(snapshot);
    store_search_state(&mut view.search, state);
    {
        let current = view.active_mut();
        sync_copy_cursor_to_search(&mut current.copy_mode, current.search.as_deref());
        reveal_search_for_view(
            terminal,
            current.copy_mode.as_deref(),
            current.search.as_deref(),
        )?;
    }
    sync_viewport_anchor(terminal, view)
}

impl HistorySearchSnapshot {
    fn capture(terminal: &Terminal<'_, '_>) -> Result<Self, WorkerError> {
        let row_count = terminal.total_rows()?;
        let columns = terminal.cols()?;
        let mut rows = Vec::with_capacity(row_count);
        let cell_capacity = row_count.saturating_mul(usize::from(columns)).min(
            MAX_SEARCH_SNAPSHOT_BYTES
                / (std::mem::size_of::<SearchCellOffset>() + std::mem::size_of::<u8>()),
        );
        let mut text = String::with_capacity(cell_capacity);
        let mut offsets = Vec::with_capacity(cell_capacity);
        let mut grapheme_scratch = Vec::new();
        for row in 0..row_count {
            let row = u32::try_from(row).unwrap_or(u32::MAX);
            let text_start =
                u32::try_from(text.len()).map_err(|_| WorkerError::SearchSnapshotTooLarge)?;
            let offset_start =
                u32::try_from(offsets.len()).map_err(|_| WorkerError::SearchSnapshotTooLarge)?;
            append_history_row(
                terminal,
                row,
                columns,
                &mut text,
                &mut offsets,
                &mut grapheme_scratch,
            )?;
            rows.push(HistorySearchRow {
                text_start,
                text_end: u32::try_from(text.len())
                    .map_err(|_| WorkerError::SearchSnapshotTooLarge)?,
                offset_start,
                offset_end: u32::try_from(offsets.len())
                    .map_err(|_| WorkerError::SearchSnapshotTooLarge)?,
            });
            let used = text
                .len()
                .saturating_add(
                    offsets
                        .len()
                        .saturating_mul(std::mem::size_of::<SearchCellOffset>()),
                )
                .saturating_add(
                    rows.len()
                        .saturating_mul(std::mem::size_of::<HistorySearchRow>()),
                );
            if used > MAX_SEARCH_SNAPSHOT_BYTES {
                return Err(WorkerError::SearchSnapshotTooLarge);
            }
        }
        Ok(Self {
            columns,
            text,
            rows,
            offsets,
        })
    }

    fn search(
        &self,
        query: &SearchQuery,
        request_id: u64,
        cancelled: impl Fn() -> bool,
    ) -> Option<SearchState> {
        let mut match_scratch = Vec::new();
        self.search_reusing(query, request_id, &mut match_scratch, cancelled)
    }

    fn search_reusing(
        &self,
        query: &SearchQuery,
        request_id: u64,
        match_scratch: &mut Vec<SearchMatch>,
        cancelled: impl Fn() -> bool,
    ) -> Option<SearchState> {
        match_scratch.clear();
        if cancelled() {
            return None;
        }
        if query.text.is_empty() || query.text.len() > MAX_SEARCH_QUERY_BYTES {
            return Some(SearchState {
                query: query.clone(),
                matches: std::mem::take(match_scratch),
                request_id,
                invalid_pattern: query.text.len() > MAX_SEARCH_QUERY_BYTES,
                ..SearchState::default()
            });
        }
        let pattern = match query.mode {
            SearchMode::Literal => regex::escape(&query.text),
            SearchMode::Regex => query.text.clone(),
        };
        let case_sensitive = match query.case {
            SearchCase::Smart => query.text.chars().any(char::is_uppercase),
            SearchCase::Sensitive => true,
            SearchCase::Insensitive => false,
        };
        let Ok(expression) = RegexBuilder::new(&pattern)
            .case_insensitive(!case_sensitive)
            .size_limit(1024 * 1024)
            .dfa_size_limit(1024 * 1024)
            .build()
        else {
            return Some(SearchState {
                query: query.clone(),
                matches: std::mem::take(match_scratch),
                request_id,
                invalid_pattern: true,
                ..SearchState::default()
            });
        };
        for (row, captured) in self.rows.iter().enumerate() {
            if cancelled() {
                return None;
            }
            let text_start = usize::try_from(captured.text_start).ok()?;
            let text_end = usize::try_from(captured.text_end).ok()?;
            let offset_start = usize::try_from(captured.offset_start).ok()?;
            let offset_end = usize::try_from(captured.offset_end).ok()?;
            let row_text = self.text.get(text_start..text_end)?;
            let row_offsets = self.offsets.get(offset_start..offset_end)?;
            for found in expression.find_iter(row_text) {
                let Some((start, end)) =
                    search_match_span(row_offsets, found.start(), found.end(), self.columns)
                else {
                    continue;
                };
                match_scratch.push(SearchMatch {
                    row: u32::try_from(row).unwrap_or(u32::MAX),
                    start,
                    end,
                });
                if match_scratch.len() >= MAX_SEARCH_MATCHES {
                    return Some(SearchState {
                        query: query.clone(),
                        matches: std::mem::take(match_scratch),
                        request_id,
                        ..SearchState::default()
                    });
                }
            }
        }
        Some(SearchState {
            query: query.clone(),
            matches: std::mem::take(match_scratch),
            request_id,
            ..SearchState::default()
        })
    }
}

fn search_match_span(
    offsets: &[SearchCellOffset],
    byte_start: usize,
    byte_end: usize,
    columns: u16,
) -> Option<(u16, u16)> {
    if byte_start >= byte_end {
        return None;
    }
    let first = offsets
        .partition_point(|cell| usize::try_from(cell.end).is_ok_and(|end| end <= byte_start));
    let after_last = offsets
        .partition_point(|cell| usize::try_from(cell.start).is_ok_and(|start| start < byte_end));
    if first >= after_last {
        return None;
    }
    let first = offsets.get(first)?;
    let last = offsets.get(after_last - 1)?;
    Some((
        first.column,
        last.column.saturating_add(last.width).min(columns),
    ))
}

#[cfg(test)]
fn search_history(terminal: &Terminal<'_, '_>, query: &str) -> Result<SearchState, WorkerError> {
    Ok(HistorySearchSnapshot::capture(terminal)?
        .search(&SearchQuery::literal(query), 1, || false)
        .expect("synchronous fixture is never cancelled"))
}

fn append_history_row(
    terminal: &Terminal<'_, '_>,
    row: u32,
    columns: u16,
    text: &mut String,
    offsets: &mut Vec<SearchCellOffset>,
    grapheme_scratch: &mut Vec<char>,
) -> Result<(), WorkerError> {
    let row_text_start = text.len();
    let mut stack = ['\0'; 8];
    for column in 0..columns {
        let grid_ref = terminal.grid_ref(Point::Screen(PointCoordinate { x: column, y: row }))?;
        let wide = grid_ref.cell()?.wide()?;
        if matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
            continue;
        }
        let start = u32::try_from(text.len().saturating_sub(row_text_start))
            .map_err(|_| WorkerError::SearchSnapshotTooLarge)?;
        match grid_ref.graphemes(&mut stack) {
            Ok(count) => text.extend(stack[..count].iter()),
            Err(libghostty_vt::Error::OutOfSpace { required }) => {
                if required > MAX_SEARCH_SNAPSHOT_BYTES / std::mem::size_of::<char>() {
                    return Err(WorkerError::SearchSnapshotTooLarge);
                }
                if grapheme_scratch.len() < required {
                    grapheme_scratch.resize(required, '\0');
                }
                let count = grid_ref.graphemes(grapheme_scratch)?;
                text.extend(grapheme_scratch[..count].iter());
            }
            Err(error) => return Err(error.into()),
        }
        let end = u32::try_from(text.len().saturating_sub(row_text_start))
            .map_err(|_| WorkerError::SearchSnapshotTooLarge)?;
        if end > start {
            offsets.push(SearchCellOffset {
                start,
                end,
                column,
                width: if matches!(wide, CellWide::Wide) { 2 } else { 1 },
            });
        }
    }
    Ok(())
}

/// `window_copy_cmd_search_again` and `window_copy_cmd_search_reverse` re-run
/// the search the mode already holds, with the stored regex bit and either the
/// stored direction or its opposite, so both go through the same placement
/// rule the first search used.
fn copy_mode_search_again(mode: Option<&CopyModeState>, reverse: bool) -> Option<CopyModeSearch> {
    let stored = mode?.search.as_ref()?;
    let direction = match (stored.direction, reverse) {
        (SearchDirection::Forward, false) | (SearchDirection::Backward, true) => {
            SearchDirection::Forward
        }
        (SearchDirection::Backward, false) | (SearchDirection::Forward, true) => {
            SearchDirection::Backward
        }
    };
    Some(CopyModeSearch {
        direction,
        incremental: false,
        ..stored.clone()
    })
}

/// `window_copy_search` picks the match the cursor moves to. Forward, vi steps
/// past the mark the cursor stands on first, so it lands on the next match's
/// start, while emacs searches from the cursor itself and may find the mark it
/// is already on. Backward, both move one cell left first, so both land on the
/// start of the previous match.
fn pick_copy_search_match(
    matches: &[SearchMatch],
    cursor: PointCoordinate,
    direction: SearchDirection,
    inclusive: bool,
    wrap: bool,
) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    let found = match direction {
        SearchDirection::Forward => matches.iter().position(|found| {
            if inclusive {
                (found.row, found.start) >= (cursor.y, cursor.x)
            } else {
                (found.row, found.start) > (cursor.y, cursor.x)
            }
        }),
        SearchDirection::Backward => matches
            .iter()
            .rposition(|found| (found.row, found.start) < (cursor.y, cursor.x)),
    };
    if found.is_some() {
        return found;
    }
    if !wrap {
        return None;
    }
    match direction {
        SearchDirection::Forward => Some(0),
        SearchDirection::Backward => Some(matches.len() - 1),
    }
}

/// The six `search-` entry points that carry a string. The pin runs the whole
/// search inside the command, marks included, so zz runs it against the frozen
/// revision here instead of queuing it on the search worker: everything the
/// next command reads is already in place when this returns.
#[allow(
    clippy::too_many_arguments,
    reason = "one call site threads the view's search state"
)]
fn run_copy_mode_search(
    view_id: TerminalViewId,
    copy_mode: &mut CopyModeSlot,
    search: &mut SearchSlot,
    search_snapshot: &mut Option<Arc<HistorySearchSnapshot>>,
    worker: &mut SearchWorker,
    spec: &CopyModeSearch,
    count: u32,
    mode_keys_vi: bool,
    wrap: bool,
) -> bool {
    let Some(mode) = copy_mode.as_deref_mut() else {
        return false;
    };
    if spec.incremental {
        match mode.incremental_origin {
            None => {
                mode.incremental_origin = Some(CopyModeIncrementalOrigin {
                    row: mode.cursor.y,
                    viewport_offset: mode.viewport_offset,
                });
            }
            Some(origin) => {
                if mode
                    .search
                    .as_ref()
                    .is_some_and(|previous| previous.text != spec.text)
                {
                    mode.viewport_offset = origin.viewport_offset;
                    mode.cursor = PointCoordinate {
                        x: copy_cursor_limit(&mode.revision, origin.row, mode_keys_vi, false),
                        y: origin.row,
                    };
                }
            }
        }
        if spec.text.is_empty() {
            mode.search_marks = false;
            mode.search_count = None;
            *search = None;
            return true;
        }
    } else if spec.text.is_empty() {
        return false;
    }

    let regex = spec.regex && spec.text.contains(|c| "^$*+()?[].\\".contains(c));
    let visible_only = mode
        .search
        .as_ref()
        .is_some_and(|previous| previous.text == spec.text && previous.regex == spec.regex);
    if !visible_only && mode.search_marks {
        mode.search_marks = false;
        mode.search_count = None;
    }
    mode.search = Some(spec.clone());
    let query = SearchQuery {
        text: spec.text.clone(),
        mode: if regex {
            SearchMode::Regex
        } else {
            SearchMode::Literal
        },
        case: SearchCase::Smart,
        direction: spec.direction,
    };
    if search_snapshot.is_none() {
        *search_snapshot = Some(Arc::clone(&mode.revision.search));
    }
    let snapshot = Arc::clone(search_snapshot.as_ref().expect("captured above"));
    let (request_id, _) = worker.next_request(view_id);
    let mut scratch = Vec::new();
    let Some(mut state) = snapshot.search_reusing(&query, request_id, &mut scratch, || false)
    else {
        return false;
    };
    let inclusive = !mode_keys_vi && spec.direction == SearchDirection::Forward;
    let mut placed = None;
    let mut cursor = mode.cursor;
    for _ in 0..count.max(1) {
        let Some(index) =
            pick_copy_search_match(&state.matches, cursor, spec.direction, inclusive, wrap)
        else {
            break;
        };
        let Some(found) = state.matches.get(index).copied() else {
            break;
        };
        cursor = PointCoordinate {
            x: if inclusive { found.end } else { found.start },
            y: found.row,
        };
        placed = Some(index);
    }
    let Some(index) = placed else {
        state.current = None;
        store_search_state(search, state);
        return true;
    };
    state.current = Some(index);
    let total = u32::try_from(state.matches.len()).unwrap_or(u32::MAX);
    store_search_state(search, state);
    mode.search_marks = true;
    if !visible_only {
        mode.search_count = Some((total, false));
    }
    place_copy_cursor(mode, cursor, mode_keys_vi);
    reveal_copy_cursor(mode);
    if mode.selecting {
        update_copy_selection(mode, None);
    }
    true
}

fn step_search(search: &mut SearchSlot, direction: isize, wrap: bool) {
    let Some(search) = search.as_mut() else {
        return;
    };
    if search.matches.is_empty() {
        search.current = None;
        return;
    }
    let len = isize::try_from(search.matches.len()).unwrap_or(isize::MAX);
    let current = isize::try_from(search.current.unwrap_or(0)).unwrap_or(0);
    let next = if wrap {
        current.saturating_add(direction).rem_euclid(len)
    } else {
        current.saturating_add(direction).clamp(0, len - 1)
    };
    search.current = usize::try_from(next).ok();
}

fn sync_copy_cursor_to_search(copy_mode: &mut CopyModeSlot, search: Option<&SearchState>) {
    let Some(mode) = copy_mode.as_mut() else {
        return;
    };
    let Some(found) = search.and_then(|search| {
        search
            .current
            .and_then(|current| search.matches.get(current))
    }) else {
        return;
    };
    mode.cursor = mode.revision.clamp_point(PointCoordinate {
        x: found.start,
        y: found.row,
    });
    reveal_copy_cursor(mode);
    if mode.selecting {
        update_copy_selection(mode, None);
    }
}

fn reveal_search_for_view(
    terminal: &mut Terminal<'_, '_>,
    copy_mode: Option<&CopyModeState>,
    search: Option<&SearchState>,
) -> Result<(), WorkerError> {
    if copy_mode.is_none()
        && let Some(search) = search
    {
        reveal_search_match(terminal, search)?;
    }
    Ok(())
}

fn reveal_search_match(
    terminal: &mut Terminal<'_, '_>,
    search: &SearchState,
) -> Result<(), WorkerError> {
    let Some(found) = search
        .current
        .and_then(|current| search.matches.get(current))
    else {
        return Ok(());
    };
    let half_page = u32::from(terminal.rows()?) / 2;
    let top = found.row.saturating_sub(half_page);
    terminal.scroll_viewport(ScrollViewport::Top);
    terminal.scroll_viewport(ScrollViewport::Delta(saturating_isize(i64::from(top))));
    Ok(())
}

fn grid_ref_first_character(
    grid_ref: &libghostty_vt::screen::GridRef<'_>,
) -> Result<Option<char>, WorkerError> {
    if !grid_ref.cell()?.has_text()? {
        return Ok(None);
    }
    let mut stack = ['\0'; 8];
    match grid_ref.graphemes(&mut stack) {
        Ok(count) => Ok(stack
            .get(..count)
            .and_then(|values| values.first())
            .copied()),
        Err(libghostty_vt::Error::OutOfSpace { required }) => {
            let mut graphemes = vec!['\0'; required];
            let count = grid_ref.graphemes(&mut graphemes)?;
            Ok(graphemes
                .get(..count)
                .and_then(|values| values.first())
                .copied())
        }
        Err(error) => Err(error.into()),
    }
}

fn select_native_word<'terminal>(
    terminal: &'terminal Terminal<'_, '_>,
    grid_ref: libghostty_vt::screen::GridRef<'terminal>,
    word_separators: &WordSeparators,
) -> Result<Option<Selection<'terminal>>, WorkerError> {
    let Some(character) = grid_ref_first_character(&grid_ref)? else {
        return Ok(None);
    };
    let boundaries = match character_word_class(Some(character), word_separators) {
        CopyWordClass::Whitespace => WordSeparators::whitespace_codepoints(),
        CopyWordClass::Separator => word_separators.separator_codepoints(),
        CopyWordClass::Word => word_separators.boundary_codepoints(),
    };
    Ok(terminal
        .select_word(SelectWordOptions::new(grid_ref).with_boundary_codepoints(boundaries))?)
}

fn select_native_word_between<'terminal>(
    terminal: &'terminal Terminal<'_, '_>,
    start: &libghostty_vt::screen::GridRef<'terminal>,
    end: &libghostty_vt::screen::GridRef<'terminal>,
    word_separators: &WordSeparators,
) -> Result<Option<Selection<'terminal>>, WorkerError> {
    let Some(start) = terminal.point_from_grid_ref(start, PointSpace::Screen)? else {
        return Ok(None);
    };
    let Some(end) = terminal.point_from_grid_ref(end, PointSpace::Screen)? else {
        return Ok(None);
    };
    let columns = u64::from(terminal.cols()?);
    let mut index = u64::from(start.y)
        .saturating_mul(columns)
        .saturating_add(u64::from(start.x));
    let end = u64::from(end.y)
        .saturating_mul(columns)
        .saturating_add(u64::from(end.x));
    loop {
        let grid_ref = terminal.grid_ref(Point::Screen(PointCoordinate {
            x: u16::try_from(index % columns).unwrap_or(0),
            y: u32::try_from(index / columns).unwrap_or(u32::MAX),
        }))?;
        if grid_ref.cell()?.has_text()? {
            return select_native_word(terminal, grid_ref, word_separators);
        }
        if index == end {
            return Ok(None);
        }
        if index < end {
            index = index.saturating_add(1);
        } else {
            index = index.saturating_sub(1);
        }
    }
}

fn selection_press(
    terminal: &Terminal<'_, '_>,
    state: &mut Option<SelectionState>,
    event: PointerCellEvent,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    let point = viewport_point(terminal, event)?;
    let anchor = terminal.track_grid_ref(point)?;
    let focus = terminal.track_grid_ref(point)?;
    let grid_ref = terminal.grid_ref(point)?;
    let mode = match event.click_count {
        0 | 1 => SelectionMode::Cell,
        2 => SelectionMode::Word,
        _ => SelectionMode::Line,
    };
    let selection = match mode {
        SelectionMode::Cell => None,
        SelectionMode::Word => select_native_word(terminal, grid_ref, word_separators)?,
        SelectionMode::Line => terminal.select_line(SelectLineOptions::new(grid_ref))?,
    };
    terminal.set_selection(selection.as_ref())?;
    *state = Some(SelectionState {
        anchor,
        focus,
        mode,
        rectangle: event.rectangle,
    });
    Ok(())
}

fn select_all_history(
    terminal: &Terminal<'_, '_>,
    state: &mut Option<SelectionState>,
) -> Result<(), WorkerError> {
    let columns = terminal.cols()?;
    let rows = u32::try_from(terminal.total_rows()?).unwrap_or(u32::MAX);
    let mut end = None;
    'rows: for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            let point = PointCoordinate { x: column, y: row };
            if terminal
                .grid_ref(Point::Screen(point))?
                .cell()?
                .has_text()?
            {
                end = Some(point);
                break 'rows;
            }
        }
    }
    let Some(end) = end else {
        *state = None;
        terminal.set_selection(None)?;
        return Ok(());
    };
    let start = PointCoordinate { x: 0, y: 0 };
    let anchor = terminal.track_grid_ref(Point::Screen(start))?;
    let focus = terminal.track_grid_ref(Point::Screen(end))?;
    let start_ref = terminal.grid_ref(Point::Screen(start))?;
    let end_ref = terminal.grid_ref(Point::Screen(end))?;
    terminal.set_selection(Some(&Selection::new(start_ref, end_ref, false)))?;
    *state = Some(SelectionState {
        anchor,
        focus,
        mode: SelectionMode::Cell,
        rectangle: false,
    });
    Ok(())
}

fn selection_drag(
    terminal: &Terminal<'_, '_>,
    state: &mut Option<SelectionState>,
    event: PointerCellEvent,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    let current = terminal.grid_ref(viewport_point(terminal, event)?)?;
    selection_drag_to(terminal, state, current, word_separators)
}

fn selection_drag_to(
    terminal: &Terminal<'_, '_>,
    state: &mut Option<SelectionState>,
    current: libghostty_vt::screen::GridRef<'_>,
    word_separators: &WordSeparators,
) -> Result<(), WorkerError> {
    let Some(selection_state) = state.as_ref() else {
        return Ok(());
    };
    let Some(current_point) = terminal.point_from_grid_ref(&current, PointSpace::Screen)? else {
        return Ok(());
    };
    let Some(anchor) = selection_state.anchor.snapshot(terminal)? else {
        *state = None;
        terminal.set_selection(None)?;
        return Ok(());
    };
    let anchor_point = terminal.point_from_grid_ref(&anchor, PointSpace::Screen)?;
    let next = match selection_state.mode {
        SelectionMode::Cell => anchor_point
            .is_some_and(|anchor| anchor != current_point)
            .then(|| Selection::new(anchor, current, selection_state.rectangle)),
        SelectionMode::Word => {
            let start = select_native_word_between(terminal, &anchor, &current, word_separators)?;
            let end = select_native_word_between(terminal, &current, &anchor, word_separators)?;
            start
                .zip(end)
                .map(|(start, end)| Selection::new(start.start(), end.end(), false))
        }
        SelectionMode::Line => {
            let start = terminal.select_line(SelectLineOptions::new(anchor))?;
            let end = terminal.select_line(SelectLineOptions::new(current))?;
            start
                .zip(end)
                .map(|(start, end)| Selection::new(start.start(), end.end(), false))
        }
    };
    terminal.set_selection(next.as_ref())?;
    let focus = terminal.track_grid_ref(Point::Screen(current_point))?;
    if let Some(selection_state) = state.as_mut() {
        selection_state.focus = focus;
    }
    Ok(())
}

fn viewport_point(
    terminal: &Terminal<'_, '_>,
    event: PointerCellEvent,
) -> Result<Point, WorkerError> {
    let column = event.column.min(terminal.cols()?.saturating_sub(1));
    let row = event.row.min(terminal.rows()?.saturating_sub(1));
    Ok(Point::Viewport(PointCoordinate {
        x: column,
        y: u32::from(row),
    }))
}

fn saturating_isize(value: i64) -> isize {
    isize::try_from(value).unwrap_or(if value.is_negative() {
        isize::MIN
    } else {
        isize::MAX
    })
}

fn saturating_isize_i128(value: i128) -> isize {
    isize::try_from(value).unwrap_or(if value.is_negative() {
        isize::MIN
    } else {
        isize::MAX
    })
}

enum Wake {
    Command(Command),
    Input(QueuedInput),
    CommandsClosed,
    Search(SearchResults),
    ChildExit(std::io::Result<ExitStatus>),
    #[cfg(all(unix, not(target_os = "linux")))]
    PtyReadable,
    #[cfg(any(target_os = "linux", not(unix)))]
    PtyMessage(ReaderMessage),
    Deadline,
}

#[cfg(unix)]
struct PtyWriter {
    fd: filedescriptor::FileDescriptor,
    pending: Vec<u8>,
    offset: usize,
}

#[cfg(unix)]
impl PtyWriter {
    fn new(fd: filedescriptor::FileDescriptor) -> Self {
        Self {
            fd,
            pending: Vec::new(),
            offset: 0,
        }
    }

    fn has_pending(&self) -> bool {
        self.offset < self.pending.len()
    }

    #[cfg(test)]
    fn queued_bytes(&self) -> usize {
        self.pending.len().saturating_sub(self.offset)
    }

    fn flush_pending(&mut self) -> std::io::Result<()> {
        let mut budget = PTY_WRITE_BUDGET_BYTES;
        while self.has_pending() && budget != 0 {
            let end = self.offset.saturating_add(budget).min(self.pending.len());
            match rustix::io::write(&self.fd, &self.pending[self.offset..end]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "PTY writer accepted no bytes",
                    ));
                }
                Ok(written) => {
                    self.offset = self.offset.saturating_add(written);
                    budget = budget.saturating_sub(written);
                }
                Err(rustix::io::Errno::AGAIN) => return Ok(()),
                Err(rustix::io::Errno::INTR) => {}
                Err(errno) => return Err(std::io::Error::from(errno)),
            }
        }
        if !self.has_pending() {
            self.pending.clear();
            self.offset = 0;
            if self.pending.capacity() > PTY_WRITE_RETAIN_BYTES {
                self.pending = Vec::new();
            }
        }
        Ok(())
    }

    fn compact_for(&mut self, additional: usize) {
        if self.pending.capacity().saturating_sub(self.pending.len()) < additional
            && self.offset != 0
        {
            let queued = self.pending.len().saturating_sub(self.offset);
            self.pending.copy_within(self.offset.., 0);
            self.pending.truncate(queued);
            self.offset = 0;
        }
    }
}

#[cfg(unix)]
impl Write for PtyWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.compact_for(buf.len());
        self.pending.extend_from_slice(buf);
        self.flush_pending()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_pending()
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn wait_for_wake(
    control_rx: &Receiver<Command>,
    input_rx: Option<&Receiver<QueuedInput>>,
    search_results: &Receiver<SearchResults>,
    child_exit: &Receiver<std::io::Result<ExitStatus>>,
    pty: Option<&filedescriptor::FileDescriptor>,
    wake_rx: &std::os::fd::OwnedFd,
    timeout: Duration,
) -> Result<Wake, WorkerError> {
    use crossbeam_channel::TryRecvError;
    use rustix::event::{PollFd, PollFlags};

    fn check_channels(
        control_rx: &Receiver<Command>,
        input_rx: Option<&Receiver<QueuedInput>>,
        search_results: &Receiver<SearchResults>,
        child_exit: &Receiver<std::io::Result<ExitStatus>>,
    ) -> Result<Option<Wake>, WorkerError> {
        match control_rx.try_recv() {
            Ok(command) => return Ok(Some(Wake::Command(command))),
            Err(TryRecvError::Disconnected) => return Ok(Some(Wake::CommandsClosed)),
            Err(TryRecvError::Empty) => {}
        }
        if let Some(input_rx) = input_rx {
            match input_rx.try_recv() {
                Ok(command) => return Ok(Some(Wake::Input(command))),
                Err(TryRecvError::Disconnected | TryRecvError::Empty) => {}
            }
        }
        match search_results.try_recv() {
            Ok(result) => return Ok(Some(Wake::Search(result))),
            Err(TryRecvError::Disconnected) => {
                return Err(WorkerError::Thread(
                    "terminal search worker stopped".to_owned(),
                ));
            }
            Err(TryRecvError::Empty) => {}
        }
        match child_exit.try_recv() {
            Ok(status) => Ok(Some(Wake::ChildExit(status))),
            Err(TryRecvError::Disconnected) => Err(WorkerError::Thread(
                "terminal child waiter stopped".to_owned(),
            )),
            Err(TryRecvError::Empty) => Ok(None),
        }
    }

    if let Some(wake) = check_channels(control_rx, input_rx, search_results, child_exit)? {
        return Ok(wake);
    }

    let timespec = rustix::event::Timespec::try_from(timeout.min(Duration::from_hours(1)))
        .expect("a bounded timeout fits in a timespec");
    let readable = PollFlags::IN | PollFlags::HUP | PollFlags::ERR;
    let wake_pollfd = PollFd::new(wake_rx, PollFlags::IN);
    let (pty_ready, wake_ready) = if let Some(pty) = pty {
        let mut fds = [PollFd::new(pty, PollFlags::IN), wake_pollfd];
        match rustix::event::poll(&mut fds, Some(&timespec)) {
            Ok(_) => (
                fds[0].revents().intersects(readable),
                fds[1].revents().intersects(readable),
            ),
            Err(rustix::io::Errno::INTR) => (false, false),
            Err(error) => return Err(WorkerError::Io(error.into())),
        }
    } else {
        let mut fds = [wake_pollfd];
        match rustix::event::poll(&mut fds, Some(&timespec)) {
            Ok(_) => (false, fds[0].revents().intersects(readable)),
            Err(rustix::io::Errno::INTR) => (false, false),
            Err(error) => return Err(WorkerError::Io(error.into())),
        }
    };
    if wake_ready {
        drain_wake_pipe(wake_rx)?;
        if let Some(wake) = check_channels(control_rx, input_rx, search_results, child_exit)? {
            return Ok(wake);
        }
    }
    if pty_ready {
        return Ok(Wake::PtyReadable);
    }
    Ok(Wake::Deadline)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn drain_wake_pipe(wake_rx: &std::os::fd::OwnedFd) -> Result<(), WorkerError> {
    let mut drained = [0_u8; 64];
    loop {
        match rustix::io::read(wake_rx, &mut drained) {
            Ok(0) | Err(rustix::io::Errno::AGAIN) => return Ok(()),
            Ok(_) | Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(WorkerError::Io(error.into())),
        }
    }
}

#[cfg(any(target_os = "linux", not(unix)))]
fn wait_for_wake(
    control_rx: &Receiver<Command>,
    input_rx: Option<&Receiver<QueuedInput>>,
    search_results: &Receiver<SearchResults>,
    child_exit: &Receiver<std::io::Result<ExitStatus>>,
    output_rx: &Receiver<ReaderMessage>,
    timeout: Duration,
) -> Result<Wake, WorkerError> {
    if let Some(input_rx) = input_rx {
        crossbeam_channel::select_biased! {
            recv(control_rx) -> message => Ok(message.map_or(Wake::CommandsClosed, Wake::Command)),
            recv(input_rx) -> message => message.map(Wake::Input).map_err(|_| {
                WorkerError::Thread("terminal input channel stopped".to_owned())
            }),
            recv(search_results) -> result => result.map(Wake::Search).map_err(|_| {
                WorkerError::Thread("terminal search worker stopped".to_owned())
            }),
            recv(output_rx) -> message => Ok(Wake::PtyMessage(message.unwrap_or(ReaderMessage::Eof))),
            recv(child_exit) -> status => status.map(Wake::ChildExit).map_err(|_| {
                WorkerError::Thread("terminal child waiter stopped".to_owned())
            }),
            default(timeout) => Ok(Wake::Deadline),
        }
    } else {
        crossbeam_channel::select_biased! {
            recv(control_rx) -> message => Ok(message.map_or(Wake::CommandsClosed, Wake::Command)),
            recv(search_results) -> result => result.map(Wake::Search).map_err(|_| {
                WorkerError::Thread("terminal search worker stopped".to_owned())
            }),
            recv(output_rx) -> message => Ok(Wake::PtyMessage(message.unwrap_or(ReaderMessage::Eof))),
            recv(child_exit) -> status => status.map(Wake::ChildExit).map_err(|_| {
                WorkerError::Thread("terminal child waiter stopped".to_owned())
            }),
            default(timeout) => Ok(Wake::Deadline),
        }
    }
}

#[cfg(target_os = "linux")]
#[allow(
    clippy::needless_pass_by_value,
    reason = "the gather thread owns its fd and both channel endpoints"
)]
fn gather_pty_linux(
    drain_fd: impl std::os::fd::AsFd,
    output: Sender<ReaderMessage>,
    recycled: Receiver<Vec<u8>>,
) {
    use rustix::event::{PollFd, PollFlags};

    let mut eof = false;
    while !eof {
        let Ok(mut buffer) = recycled.recv() else {
            return;
        };
        debug_assert_eq!(buffer.len(), PTY_READ_BUFFER_BYTES);

        let mut length = 0_usize;
        let mut spins = 0_u32;
        while length < buffer.len() {
            if length == 0 {
                let mut fds = [PollFd::new(&drain_fd, PollFlags::IN)];
                loop {
                    match rustix::event::poll(&mut fds, None) {
                        Ok(_) => break,
                        Err(rustix::io::Errno::INTR) => {}
                        Err(error) => {
                            log::debug!("Linux PTY gather poll stopped: {error}");
                            eof = true;
                            break;
                        }
                    }
                }
                if eof {
                    break;
                }
            }

            match rustix::io::read(&drain_fd, &mut buffer[length..]) {
                Ok(0) | Err(rustix::io::Errno::IO) => {
                    eof = true;
                    break;
                }
                Ok(read) => {
                    length += read;
                    spins = 0;
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => {
                    if length >= PTY_BRIDGE_THRESHOLD_BYTES && spins < PTY_GATHER_BRIDGE_SPIN_MAX {
                        spins += 1;
                        continue;
                    }
                    if length == 0 {
                        continue;
                    }
                    break;
                }
                Err(error) => {
                    log::debug!("Linux PTY gather stopped: {error}");
                    eof = true;
                    break;
                }
            }
        }

        if length > 0 && output.send(ReaderMessage::Data { buffer, length }).is_err() {
            return;
        }
    }
    let _ = output.send(ReaderMessage::Eof);
}

#[cfg(any(not(unix), test))]
#[allow(
    clippy::needless_pass_by_value,
    reason = "the reader thread owns the final sender so disconnect signals shutdown"
)]
fn read_pty(
    mut reader: Box<dyn Read + Send>,
    pending_output: Box<dyn Fn() -> usize + Send>,
    output: Sender<ReaderMessage>,
    recycled: Receiver<Vec<u8>>,
) {
    let mut eof = false;
    while !eof {
        let Ok(mut buffer) = recycled.recv() else {
            return;
        };
        debug_assert_eq!(buffer.len(), PTY_READ_BUFFER_BYTES);
        let mut length = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                log::debug!("PTY reader reached EOF: {error}");
                break;
            }
        };
        let mut pending = pending_output();
        while length < buffer.len() && pending > 0 {
            let want = pending.min(buffer.len() - length);
            match reader.read(&mut buffer[length..length + want]) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(read) => {
                    length += read;
                    pending = pending.saturating_sub(read);
                    if pending == 0 {
                        pending = pending_output();
                    }
                }
                Err(error) => {
                    log::debug!("PTY reader reached EOF: {error}");
                    eof = true;
                    break;
                }
            }
        }
        if output.send(ReaderMessage::Data { buffer, length }).is_err() {
            return;
        }
    }
    let _ = output.send(ReaderMessage::Eof);
}

#[cfg(any(target_os = "linux", not(unix)))]
fn consume_pty_output(
    terminal: &mut Terminal<'_, '_>,
    passthrough: &mut PassthroughFilter,
    engine: &mut EngineOutput<'_>,
    raw_output_tap: &mut Option<(u64, Sender<Arc<[u8]>>)>,
    buffer: Vec<u8>,
    length: usize,
    recycled: &Sender<Vec<u8>>,
) -> (Option<u64>, usize) {
    log::trace!(
        target: "zz_terminal::diagnostics::pty",
        "read length={length} bytes={:?} text={:?}",
        &buffer[..length],
        String::from_utf8_lossy(&buffer[..length]),
    );
    let closed_tap = tap_raw_output(raw_output_tap, &buffer[..length]);
    let parsed = feed_pty_output(terminal, passthrough, engine, &buffer[..length]);
    let _ = recycled.try_send(buffer);
    (closed_tap, parsed)
}

#[cfg(any(target_os = "linux", not(unix), test))]
fn tap_raw_output(tap: &mut Option<(u64, Sender<Arc<[u8]>>)>, bytes: &[u8]) -> Option<u64> {
    tap_raw_output_arc(tap, &Arc::from(bytes))
}

fn tap_raw_output_arc(
    tap: &mut Option<(u64, Sender<Arc<[u8]>>)>,
    bytes: &Arc<[u8]>,
) -> Option<u64> {
    let disconnected = tap
        .as_ref()
        .is_some_and(|(_, output)| output.send(Arc::clone(bytes)).is_err());
    if disconnected {
        tap.take().map(|(token, _)| token)
    } else {
        None
    }
}

fn drain_raw_output_parse_backlog(
    terminal: &mut Terminal<'_, '_>,
    passthrough: &mut PassthroughFilter,
    engine: &mut EngineOutput<'_>,
    backlog: &mut VecDeque<(Arc<[u8]>, usize)>,
    backlog_bytes: &mut usize,
    buffer: &mut Vec<u8>,
) -> usize {
    buffer.clear();
    while buffer.len() < RAW_OUTPUT_PARSE_TURN_BYTES {
        let Some((bytes, offset)) = backlog.front_mut() else {
            break;
        };
        let length = bytes
            .len()
            .saturating_sub(*offset)
            .min(RAW_OUTPUT_PARSE_TURN_BYTES.saturating_sub(buffer.len()));
        buffer.extend_from_slice(&bytes[*offset..offset.saturating_add(length)]);
        *offset = offset.saturating_add(length);
        *backlog_bytes = backlog_bytes.saturating_sub(length);
        if *offset == bytes.len() {
            backlog.pop_front();
        }
    }
    if buffer.is_empty() {
        0
    } else {
        feed_pty_output(terminal, passthrough, engine, buffer)
    }
}

struct EngineOutput<'a> {
    filter: &'a mut EngineFilter,
    knobs: EngineKnobs,
    renames: &'a mut Vec<String>,
    /// The pane's progress bar, set only by an OSC 9;4 that moved it.
    bar: &'a mut Option<ProgressBar>,
}

fn feed_pty_output(
    terminal: &mut Terminal<'_, '_>,
    passthrough: &mut PassthroughFilter,
    engine: &mut EngineOutput<'_>,
    bytes: &[u8],
) -> usize {
    let EngineOutput {
        filter,
        knobs,
        renames,
        bar,
    } = engine;
    passthrough.write(bytes, |unwrapped| {
        filter.write(unwrapped, *knobs, terminal, renames, bar);
    })
}

#[cfg(any(target_os = "linux", not(unix), test))]
fn drain_pty_output_burst(
    output: &Receiver<ReaderMessage>,
    first_buffer: Vec<u8>,
    first_length: usize,
    mut consume: impl FnMut(Vec<u8>, usize),
) -> bool {
    consume(first_buffer, first_length);

    for _ in 1..PTY_BUFFER_POOL_SIZE {
        match output.try_recv() {
            Ok(ReaderMessage::Data { buffer, length }) => consume(buffer, length),
            Ok(ReaderMessage::Eof) => return true,
            Err(_) => break,
        }
    }
    false
}

fn drain_effects(effects: &RefCell<PtyEffects>, writer: &mut dyn Write) -> Result<(), WorkerError> {
    let mut effects = effects.borrow_mut();
    if !effects.bytes.is_empty() {
        writer.write_all(&effects.bytes)?;
        effects.bytes.clear();
    }
    let overflowed = std::mem::take(&mut effects.overflowed);
    drop(effects);
    if overflowed {
        log::warn!("terminal PTY responses exceeded the {MAX_PTY_RESPONSE_BYTES}-byte limit");
    }
    writer.flush()?;
    Ok(())
}

fn pty_effects_pending(effects: &RefCell<PtyEffects>) -> bool {
    let effects = effects.borrow();
    !effects.bytes.is_empty() || effects.overflowed
}

#[cfg(unix)]
fn drain_effects_if_writer_ready(
    effects: &RefCell<PtyEffects>,
    writer: &mut PtyWriter,
) -> Result<(), WorkerError> {
    if writer.has_pending() || !pty_effects_pending(effects) {
        return Ok(());
    }
    drain_effects(effects, writer)
}

fn encode_key(
    terminal: &Terminal<'_, '_>,
    encoder: &mut key::Encoder<'_>,
    event: &mut key::Event<'_>,
    input: KeyInput,
    erase_byte: Option<u8>,
    writer: &mut dyn Write,
    input_bytes: &mut Vec<u8>,
) -> Result<(), WorkerError> {
    let mut modifiers = key::Mods::empty();
    modifiers.set(key::Mods::SHIFT, input.modifiers.shift());
    modifiers.set(key::Mods::CTRL, input.modifiers.control());
    modifiers.set(key::Mods::ALT, input.modifiers.alt());
    modifiers.set(key::Mods::SUPER, input.modifiers.platform());

    event
        .set_action(match input.action {
            KeyAction::Press => key::Action::Press,
            KeyAction::Repeat => key::Action::Repeat,
            KeyAction::Release => key::Action::Release,
        })
        .set_key(ghostty_key(input.key))
        .set_mods(modifiers)
        .set_composing(false)
        .set_utf8(input.text)
        .set_unshifted_codepoint(input.unshifted_codepoint.unwrap_or('\0'));

    encoder.set_options_from_terminal(terminal);
    #[cfg(target_os = "macos")]
    encoder.set_macos_option_as_alt(key::OptionAsAlt::True);

    input_bytes.clear();
    encoder.encode_to_vec(event, input_bytes)?;
    if input.key == KeyCode::Backspace
        && input.modifiers == Modifiers::default()
        && input_bytes.as_slice() == [0x7f]
    {
        input_bytes.clear();
        input_bytes.extend(erase_byte);
    }
    if !input_bytes.is_empty() {
        writer.write_all(input_bytes)?;
        writer.flush()?;
    }
    Ok(())
}

fn ghostty_key(key: KeyCode) -> key::Key {
    match key {
        KeyCode::Character(character) => match character.to_ascii_lowercase() {
            '`' => key::Key::Backquote,
            '\\' => key::Key::Backslash,
            '[' => key::Key::BracketLeft,
            ']' => key::Key::BracketRight,
            ',' => key::Key::Comma,
            '0' => key::Key::Digit0,
            '1' => key::Key::Digit1,
            '2' => key::Key::Digit2,
            '3' => key::Key::Digit3,
            '4' => key::Key::Digit4,
            '5' => key::Key::Digit5,
            '6' => key::Key::Digit6,
            '7' => key::Key::Digit7,
            '8' => key::Key::Digit8,
            '9' => key::Key::Digit9,
            '=' => key::Key::Equal,
            'a' => key::Key::A,
            'b' => key::Key::B,
            'c' => key::Key::C,
            'd' => key::Key::D,
            'e' => key::Key::E,
            'f' => key::Key::F,
            'g' => key::Key::G,
            'h' => key::Key::H,
            'i' => key::Key::I,
            'j' => key::Key::J,
            'k' => key::Key::K,
            'l' => key::Key::L,
            'm' => key::Key::M,
            'n' => key::Key::N,
            'o' => key::Key::O,
            'p' => key::Key::P,
            'q' => key::Key::Q,
            'r' => key::Key::R,
            's' => key::Key::S,
            't' => key::Key::T,
            'u' => key::Key::U,
            'v' => key::Key::V,
            'w' => key::Key::W,
            'x' => key::Key::X,
            'y' => key::Key::Y,
            'z' => key::Key::Z,
            '-' => key::Key::Minus,
            '.' => key::Key::Period,
            '\'' => key::Key::Quote,
            ';' => key::Key::Semicolon,
            '/' => key::Key::Slash,
            ' ' => key::Key::Space,
            _ => key::Key::Unidentified,
        },
        KeyCode::Backspace => key::Key::Backspace,
        KeyCode::Enter => key::Key::Enter,
        KeyCode::Tab => key::Key::Tab,
        KeyCode::Escape => key::Key::Escape,
        KeyCode::Delete => key::Key::Delete,
        KeyCode::Insert => key::Key::Insert,
        KeyCode::Home => key::Key::Home,
        KeyCode::End => key::Key::End,
        KeyCode::PageUp => key::Key::PageUp,
        KeyCode::PageDown => key::Key::PageDown,
        KeyCode::ArrowUp => key::Key::ArrowUp,
        KeyCode::ArrowDown => key::Key::ArrowDown,
        KeyCode::ArrowLeft => key::Key::ArrowLeft,
        KeyCode::ArrowRight => key::Key::ArrowRight,
        KeyCode::Function(number) => match number {
            1 => key::Key::F1,
            2 => key::Key::F2,
            3 => key::Key::F3,
            4 => key::Key::F4,
            5 => key::Key::F5,
            6 => key::Key::F6,
            7 => key::Key::F7,
            8 => key::Key::F8,
            9 => key::Key::F9,
            10 => key::Key::F10,
            11 => key::Key::F11,
            12 => key::Key::F12,
            13 => key::Key::F13,
            14 => key::Key::F14,
            15 => key::Key::F15,
            16 => key::Key::F16,
            17 => key::Key::F17,
            18 => key::Key::F18,
            19 => key::Key::F19,
            20 => key::Key::F20,
            21 => key::Key::F21,
            22 => key::Key::F22,
            23 => key::Key::F23,
            24 => key::Key::F24,
            _ => key::Key::Unidentified,
        },
        KeyCode::Unidentified => key::Key::Unidentified,
    }
}

fn publish_active_views<'alloc: 'callbacks, 'callbacks>(
    terminal: &mut Terminal<'alloc, 'callbacks>,
    publisher: &Publisher,
    render_state: &mut RenderState<'alloc>,
    rows: &mut RowIterator<'alloc>,
    cells: &mut CellIterator<'alloc>,
    generations: &mut ViewportGenerations,
    change: SnapshotChange,
    dictionary: &mut ViewportDictionary,
    active: &mut ActiveTerminalViews,
    word_separators: &WordSeparators,
    status: SessionStatus,
) -> Result<(), WorkerError> {
    if active.is_empty() {
        publisher.publish(snapshot(
            terminal,
            render_state,
            rows,
            cells,
            generations,
            change,
            dictionary,
            None,
            status,
        )?);
        return Ok(());
    }

    let mut view_ids = active.keys().copied().collect::<Vec<_>>();
    view_ids.sort_by_key(|view| view.0);
    let mut viewports = Vec::with_capacity(view_ids.len());
    let mut copy_facts = HashMap::with_capacity(view_ids.len());
    for view_id in view_ids {
        let view = active
            .get_mut(&view_id)
            .expect("active view id was collected from the same map");
        if view.copy_mode.is_some() {
            terminal.set_selection(None)?;
        } else {
            restore_view_state(terminal, view, word_separators)?;
        }
        let viewport = snapshot(
            terminal,
            render_state,
            rows,
            cells,
            generations,
            change,
            dictionary,
            Some(view),
            status.clone(),
        )?;
        if let Some(facts) = view
            .copy_mode
            .as_deref()
            .map(|mode| copy_mode_facts(mode, view.search.as_deref(), word_separators))
        {
            copy_facts.insert(view_id, Arc::new(facts));
        }
        viewports.push((view_id, viewport));
    }
    publisher.publish_copy_facts(copy_facts);
    publisher.publish_viewports(viewports);
    Ok(())
}

/// `window_copy_formats` read off one frozen view. `data->cy` and `data->oy`
/// are screen-relative and bottom-relative; the selection coordinates the pin
/// stores in `selx`, `sely`, `endselx` and `endsely` are absolute grid rows,
/// which is what the retained revision already indexes by.
fn copy_mode_facts(
    mode: &CopyModeState,
    search: Option<&SearchState>,
    word_separators: &WordSeparators,
) -> CopyModeFacts {
    let cursor = PointCoordinate {
        x: mode.cursor.x,
        y: mode
            .cursor
            .y
            .min(mode.revision.total_rows().saturating_sub(1)),
    };
    CopyModeFacts {
        view_mode: mode.kind == FrozenModeKind::View,
        cursor_x: u32::from(cursor.x),
        cursor_y: cursor.y.saturating_sub(mode.viewport_offset),
        cursor_line: mode_format_line(&mode.revision, cursor.y),
        cursor_word: mode_format_word(&mode.revision, cursor, word_separators),
        scroll_position: mode
            .revision
            .maximum_offset()
            .saturating_sub(mode.viewport_offset),
        selection: mode.selection.map(|selection| CopyModeSelectionFacts {
            start_x: u32::from(selection.anchor.x),
            start_y: selection.anchor.y,
            end_x: u32::from(selection.focus.x),
            end_y: selection.focus.y,
        }),
        search_present: mode.search_marks,
        search_count: mode.search_count,
        search_timed_out: false,
        search_match: if mode.search_marks {
            copy_mode_search_match(mode, search, cursor)
        } else {
            String::new()
        },
    }
}

/// `window_copy_match_at_cursor`: the marked run the cursor stands in, read
/// back off the revision. An unmarked cell steps one position back before
/// giving up, which is how the emacs placement one past a match still answers
/// that match.
fn copy_mode_search_match(
    mode: &CopyModeState,
    search: Option<&SearchState>,
    cursor: PointCoordinate,
) -> String {
    let contains = |search: &SearchState, point: PointCoordinate| {
        search
            .matches
            .iter()
            .find(|found| found.row == point.y && (found.start..found.end).contains(&point.x))
            .copied()
    };
    let Some(found) = search.and_then(|search| {
        contains(search, cursor).or_else(|| {
            let back = if cursor.x > 0 {
                PointCoordinate {
                    x: cursor.x - 1,
                    y: cursor.y,
                }
            } else {
                PointCoordinate {
                    x: mode.revision.columns.saturating_sub(1),
                    y: cursor.y.checked_sub(1)?,
                }
            };
            contains(search, back)
        })
    }) else {
        return String::new();
    };
    let mut text = String::new();
    for column in found.start..found.end {
        let point = PointCoordinate {
            x: column,
            y: found.row,
        };
        if revision_cell_is_padding(&mode.revision, point) {
            continue;
        }
        if let Some(character) = mode.revision.first_char(point) {
            text.push(character);
        }
    }
    text
}

/// `grid_line_length`: the row's width with trailing blank cells trimmed. An
/// unwritten cell reads as a space in the pin's grid, so it trims too.
fn mode_format_line_length(revision: &ModeRevision, row: u32) -> u32 {
    (0..revision.columns)
        .rev()
        .find(|column| {
            let point = PointCoordinate { x: *column, y: row };
            !matches!(revision.first_char(point), None | Some(' '))
        })
        .map_or(0, |column| u32::from(column).saturating_add(1))
}

/// `format_grid_line`: one row's text, trailing blanks trimmed, wraps not
/// followed. An empty row answers NULL on the pin, which expands empty.
fn mode_format_line(revision: &ModeRevision, row: u32) -> String {
    let mut text = String::new();
    for column in 0..mode_format_line_length(revision, row) {
        let point = PointCoordinate {
            x: u16::try_from(column).unwrap_or(u16::MAX),
            y: row,
        };
        revision.push_cell_text(revision.cell(point), &mut text);
    }
    text
}

/// `format_is_word_separator`: the configured set, plus tab and space, plus
/// the unwritten cell the pin reads back as a space. Padding halves are never
/// separators; the pin skips them before the test.
fn mode_format_is_word_separator(
    revision: &ModeRevision,
    point: PointCoordinate,
    word_separators: &WordSeparators,
) -> bool {
    if matches!(
        revision.cell(point).width(),
        CellWidth::SpacerTail | CellWidth::SpacerHead
    ) {
        return false;
    }
    match revision.first_char(point) {
        None => true,
        Some(character) => {
            character == ' ' || character == '\t' || word_separators.contains_separator(character)
        }
    }
}

/// `format_grid_word`: walk back to the start of the word the cursor sits in,
/// crossing a wrap, then collect forward to the next separator. A cursor on a
/// separator collects the word that follows it, and answers empty when the
/// next cell is a separator too.
fn mode_format_word(
    revision: &ModeRevision,
    cursor: PointCoordinate,
    word_separators: &WordSeparators,
) -> String {
    let mut x = cursor.x;
    let mut y = cursor.y;
    let mut found = false;
    loop {
        if mode_format_is_word_separator(revision, PointCoordinate { x, y }, word_separators) {
            found = true;
            break;
        }
        if x == 0 {
            if y == 0 {
                break;
            }
            if !revision.row(y.saturating_sub(1)).wrapped() {
                break;
            }
            y -= 1;
            let length = mode_format_line_length(revision, y);
            if length == 0 {
                break;
            }
            x = u16::try_from(length).unwrap_or(u16::MAX);
        }
        x -= 1;
    }
    let mut text = String::new();
    let last_row = revision.total_rows().saturating_sub(1);
    loop {
        if found {
            let end = mode_format_line_length(revision, y);
            if end == 0 || u32::from(x) == end.saturating_sub(1) {
                if y == last_row || !revision.row(y).wrapped() {
                    break;
                }
                y += 1;
                x = 0;
            } else {
                x += 1;
            }
        }
        found = true;
        let point = PointCoordinate { x, y };
        let cell = revision.cell(point);
        if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
            continue;
        }
        if mode_format_is_word_separator(revision, point, word_separators) {
            break;
        }
        revision.push_cell_text(cell, &mut text);
    }
    text
}

fn snapshot<'alloc: 'callbacks, 'callbacks>(
    terminal: &Terminal<'alloc, 'callbacks>,
    render_state: &mut RenderState<'alloc>,
    rows: &mut RowIterator<'alloc>,
    cells: &mut CellIterator<'alloc>,
    generations: &mut ViewportGenerations,
    change: SnapshotChange,
    dictionary: &mut ViewportDictionary,
    view: Option<&TerminalViewState>,
    status: SessionStatus,
) -> Result<TerminalViewport, WorkerError> {
    let started = diagnostic_timer();
    let result = build_snapshot(
        terminal,
        render_state,
        rows,
        cells,
        generations,
        change,
        dictionary,
        view,
        status,
    );
    match &result {
        Ok(viewport) => log::trace!(
            target: "zz_terminal::diagnostics::snapshot",
            "build change={change:?} elapsed_us={} generation={} view_generation={} dictionary_generation={} columns={} rows={} cells={} cell_bytes={} styles={} graphemes={} grapheme_bytes={} overlays={} cursor={:?} scrollbar={:?} mode={:?} search={:?} unseen_output={} status={:?}",
            diagnostic_elapsed_us(started),
            viewport.generation,
            viewport.view_generation,
            viewport.dictionary_generation,
            viewport.columns,
            viewport.rows,
            viewport.cells.len(),
            std::mem::size_of_val(viewport.cells.as_ref()),
            viewport.dictionary.styles.len(),
            viewport.dictionary.grapheme_offsets.len(),
            viewport.dictionary.grapheme_bytes.len(),
            viewport.overlays.len(),
            viewport.cursor,
            viewport.scrollbar,
            viewport.mode,
            viewport.search,
            viewport.unseen_output,
            viewport.status,
        ),
        Err(error) => log::trace!(
            target: "zz_terminal::diagnostics::snapshot",
            "build change={change:?} elapsed_us={} error={error}",
            diagnostic_elapsed_us(started),
        ),
    }
    result
}

fn reported_working_directory(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    if !value.starts_with("file://") {
        return Some(value.to_owned());
    }
    let mut url = url::Url::parse(value).ok()?;
    url.set_host(Some("localhost")).ok()?;
    url.to_file_path()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn build_snapshot<'alloc: 'callbacks, 'callbacks>(
    terminal: &Terminal<'alloc, 'callbacks>,
    render_state: &mut RenderState<'alloc>,
    rows: &mut RowIterator<'alloc>,
    cells: &mut CellIterator<'alloc>,
    generations: &mut ViewportGenerations,
    change: SnapshotChange,
    dictionary: &mut ViewportDictionary,
    view: Option<&TerminalViewState>,
    status: SessionStatus,
) -> Result<TerminalViewport, WorkerError> {
    if let Some(view) = view
        && let Some(copy_mode) = view.copy_mode.as_ref()
    {
        return Ok(copy_mode_snapshot(
            generations,
            change,
            dictionary,
            view,
            copy_mode,
            status,
        ));
    }
    let copy_mode = view.and_then(|view| view.copy_mode.as_ref());
    let search = view.and_then(|view| view.search.as_ref());
    let hover_link = view.and_then(|view| view.hover_link.as_ref());
    let unseen_output = view.map_or(0, |view| view.unseen_output);
    let snapshot = render_state.update(terminal)?;
    let dirty = snapshot.dirty()?;
    let full_dirty = dirty == Dirty::Full;
    let colors = snapshot.colors()?;
    let columns = snapshot.cols()?;
    let row_count = snapshot.rows()?;
    let foreground = color(colors.foreground);
    let background = color(colors.background);
    let cursor = snapshot
        .cursor_viewport()?
        .map(|position| {
            Cursor::new(
                position.x,
                position.y,
                snapshot.cursor_visible().unwrap_or(true),
                snapshot.cursor_blinking().unwrap_or(false),
                position.at_wide_tail,
                match snapshot
                    .cursor_visual_style()
                    .unwrap_or(CursorVisualStyle::Block)
                {
                    CursorVisualStyle::Bar => CursorStyle::Bar,
                    CursorVisualStyle::Underline => CursorStyle::Underline,
                    CursorVisualStyle::BlockHollow => CursorStyle::BlockHollow,
                    _ => CursorStyle::Block,
                },
                color(colors.cursor.unwrap_or(colors.foreground)),
            )
        })
        .filter(|_| copy_mode.is_none());

    let default_style = PackedStyle::new(foreground, background, None, 0, UnderlineStyle::None);
    let cell_count = usize::from(columns).saturating_mul(usize::from(row_count));
    let previous_dictionary_generation = dictionary.generation;
    dictionary.ensure_default(default_style, &colors.palette);
    if dictionary.generation == previous_dictionary_generation && dictionary.should_compact_live() {
        dictionary.reset_live(default_style, &colors.palette);
    }
    let dictionary_reset = dictionary.generation != previous_dictionary_generation;
    let dimensions_changed = dictionary.shared_cells.len() != cell_count;
    let mut grapheme_scratch = std::mem::take(&mut dictionary.grapheme_scratch);
    if grapheme_scratch.capacity() < 8 {
        grapheme_scratch.reserve(8 - grapheme_scratch.capacity());
    }
    let mut overlays = std::mem::take(&mut dictionary.overlay_scratch);
    let overlay_only = matches!(change, SnapshotChange::Overlay);
    let cell_plane_dirty =
        dimensions_changed || dictionary_reset || (!overlay_only && dirty != Dirty::Clean);
    let preserve_cells = !dimensions_changed && !dictionary_reset && !full_dirty;
    let mut next_cells =
        cell_plane_dirty.then(|| dictionary.acquire_cell_plane(cell_count, preserve_cells));
    let extraction_result = (|| -> Result<(), WorkerError> {
        let mut output_cells = next_cells
            .as_mut()
            .map(|cells| Arc::get_mut(cells).expect("acquired cell plane remains unique"));
        let mut visible_row = 0_u16;
        let mut row_iteration = rows.update(&snapshot)?;
        while let Some(row) = row_iteration.next() {
            let row_start = usize::from(visible_row).saturating_mul(usize::from(columns));
            if let Some(selected) = row.selection()? {
                overlays.push(OverlaySpan::with_flags(
                    visible_row,
                    selected.start_x.min(columns),
                    selected.end_x.saturating_add(1).min(columns),
                    OverlayKind::Selection,
                    if view
                        .and_then(|view| view.selection.as_ref())
                        .is_some_and(|selection| selection.rectangle)
                    {
                        OVERLAY_RECTANGLE
                    } else {
                        0
                    },
                ));
            }
            let update_cells = output_cells.is_some()
                && (dimensions_changed || dictionary_reset || full_dirty || row.dirty()?);
            if update_cells {
                let row_end = row_start.saturating_add(usize::from(columns));
                let output_row = output_cells
                    .as_deref_mut()
                    .and_then(|cells| cells.get_mut(row_start..row_end))
                    .ok_or(WorkerError::ViewportMetadataTooLarge)?;
                output_row.fill(PackedCell::EMPTY);
                let mut cell_iteration = cells.update(row)?;
                let mut column = 0_usize;
                while let Some(cell) = cell_iteration.next() {
                    if column >= usize::from(columns) {
                        break;
                    }
                    let raw_style = cell.style()?;
                    let mut cell_foreground = color(cell.fg_color()?.unwrap_or(colors.foreground));
                    let mut cell_background = color(cell.bg_color()?.unwrap_or(colors.background));
                    if raw_style.inverse {
                        std::mem::swap(&mut cell_foreground, &mut cell_background);
                    }

                    grapheme_scratch.clear();
                    cell.graphemes_utf8(&mut grapheme_scratch)?;
                    let raw_cell = cell.raw_cell()?;
                    let width = match raw_cell.wide()? {
                        CellWide::Narrow => CellWidth::Narrow,
                        CellWide::Wide => CellWidth::Wide,
                        CellWide::SpacerTail => CellWidth::SpacerTail,
                        CellWide::SpacerHead => CellWidth::SpacerHead,
                    };
                    let underline_color =
                        resolve_style_color(raw_style.underline_color, &colors.palette);
                    let foreground_explicit_rgb = matches!(
                        if raw_style.inverse {
                            raw_style.bg_color
                        } else {
                            raw_style.fg_color
                        },
                        StyleColor::Rgb(_)
                    );
                    let style = PackedStyle::new(
                        cell_foreground,
                        cell_background,
                        underline_color,
                        style_attributes(
                            &raw_style,
                            foreground_explicit_rgb,
                            raw_cell.has_hyperlink()?,
                        ),
                        underline_style(raw_style.underline),
                    );
                    let style_id = dictionary.intern_style(style);
                    let glyph = dictionary.encode_glyph(&grapheme_scratch);
                    output_row[column] = PackedCell::new(glyph, style_id, width);
                    column += 1;
                }
                row.set_dirty(false)?;
            }
            visible_row = visible_row.saturating_add(1);
        }
        Ok(())
    })();
    dictionary.grapheme_scratch = grapheme_scratch;
    if let Err(error) = extraction_result {
        if let Some(cells) = next_cells {
            dictionary.retain_cell_plane(cells);
        }
        overlays.clear();
        dictionary.overlay_scratch = overlays;
        return Err(error);
    }
    if let Some(cells) = next_cells {
        dictionary.commit_cell_plane(cells);
    }
    snapshot.set_dirty(Dirty::Clean)?;

    if matches!(change, SnapshotChange::Content) {
        generations.content = generations.content.saturating_add(1);
    }
    generations.view = generations.view.saturating_add(1);
    let scrollbar = terminal.scrollbar()?;
    let scrollbar = ScrollbarState {
        total: u32::try_from(scrollbar.total).map_err(|_| WorkerError::ViewportMetadataTooLarge)?,
        offset: u32::try_from(scrollbar.offset)
            .map_err(|_| WorkerError::ViewportMetadataTooLarge)?,
        len: u32::try_from(scrollbar.len).map_err(|_| WorkerError::ViewportMetadataTooLarge)?,
    };
    let kitty_placements = if let Some(kitty) = generations.kitty.as_mut() {
        kitty.placements(terminal, scrollbar)?
    } else {
        Arc::from([])
    };
    if let Some(search) = search {
        for (index, found) in search.matches.iter().enumerate() {
            let row = found.row;
            if row < scrollbar.offset || row >= scrollbar.offset.saturating_add(scrollbar.len) {
                continue;
            }
            let viewport_row = row.saturating_sub(scrollbar.offset);
            if let Ok(viewport_row) = u16::try_from(viewport_row) {
                overlays.push(OverlaySpan::new(
                    viewport_row,
                    found.start,
                    found.end,
                    if search.current == Some(index) {
                        OverlayKind::SearchCurrent
                    } else {
                        OverlayKind::SearchMatch
                    },
                ));
            }
        }
    }
    if let Some(link) = hover_link
        && link.row < row_count
        && link.start < link.end
    {
        overlays.push(OverlaySpan::new(
            link.row,
            link.start.min(columns),
            link.end.min(columns),
            OverlayKind::LinkHover,
        ));
    }
    let copy_cursor = copy_mode.and_then(|copy_mode| {
        let row = copy_mode.cursor.y.checked_sub(copy_mode.viewport_offset)?;
        (row < u32::from(row_count)).then_some(PointCoordinate {
            x: copy_mode.cursor.x,
            y: row,
        })
    });
    if let Some(point) = copy_cursor
        && point.y < u32::from(row_count)
    {
        overlays.push(OverlaySpan::new(
            u16::try_from(point.y).unwrap_or(u16::MAX),
            point.x.min(columns),
            point.x.saturating_add(1).min(columns),
            OverlayKind::CopyCursor,
        ));
    }
    let mode = if let Some(copy_mode) = copy_mode {
        let total = scrollbar.total.max(1);
        let position = copy_mode.cursor.y.saturating_add(1).min(total);
        match copy_mode.kind {
            FrozenModeKind::Copy => TerminalMode::Copy {
                position,
                total,
                hide_position: copy_mode.hide_position,
            },
            FrozenModeKind::View => TerminalMode::View { position, total },
        }
    } else {
        TerminalMode::Live
    };
    if dictionary_reset {
        dictionary.tune_live_compaction_limits(cell_count);
    }
    let shared_dictionary = dictionary.shared_dictionary();
    let title = terminal.title().unwrap_or("zz");
    let working_directory = terminal.pwd().ok().and_then(reported_working_directory);
    let presentation = dictionary.shared_presentation(
        title,
        working_directory.as_deref(),
        hover_link.map(|link| link.uri.as_str()),
    );
    let overlays = dictionary.finish_overlays(overlays);
    Ok(TerminalViewport {
        generation: generations.content,
        view_generation: generations.view,
        dictionary_generation: dictionary.generation,
        columns,
        rows: row_count,
        foreground,
        background,
        presentation,
        cells: Arc::clone(&dictionary.shared_cells),
        dictionary: shared_dictionary,
        overlays,
        kitty_placements,
        cursor,
        scrollbar,
        mode,
        search: search.map(|search| {
            SearchStatus::new(
                search
                    .current
                    .and_then(|current| u32::try_from(current.saturating_add(1)).ok())
                    .unwrap_or(0),
                u32::try_from(search.matches.len()).unwrap_or(u32::MAX),
            )
            .with_pending(search.pending)
            .with_invalid_pattern(search.invalid_pattern)
        }),
        unseen_output,
        kitty_keyboard: terminal
            .kitty_keyboard_flags()
            .is_ok_and(|flags| !flags.is_empty()),
        mouse_tracking: terminal.is_mouse_tracking()?,
        status,
    })
}

fn copy_mode_snapshot(
    generations: &mut ViewportGenerations,
    change: SnapshotChange,
    dictionary: &mut ViewportDictionary,
    view: &TerminalViewState,
    mode: &CopyModeState,
    status: SessionStatus,
) -> TerminalViewport {
    if matches!(change, SnapshotChange::Content) {
        generations.content = generations.content.saturating_add(1);
    }
    generations.view = generations.view.saturating_add(1);
    let revision = &mode.revision;
    let offset = mode.viewport_offset.min(revision.maximum_offset());
    let cells = dictionary.mode_cells(revision, offset);
    let mut overlays = std::mem::take(&mut dictionary.overlay_scratch);
    if let Some(selection) = mode.selection {
        push_mode_selection_overlays(&mut overlays, revision, selection, offset);
    }
    if let Some(search) = view.search.as_ref() {
        let visible_end = offset.saturating_add(u32::from(revision.viewport_rows));
        for (index, found) in search.matches.iter().enumerate() {
            if found.row < offset || found.row >= visible_end {
                continue;
            }
            overlays.push(OverlaySpan::new(
                u16::try_from(found.row - offset).unwrap_or(u16::MAX),
                found.start.min(revision.columns),
                found.end.min(revision.columns),
                if search.current == Some(index) {
                    OverlayKind::SearchCurrent
                } else {
                    OverlayKind::SearchMatch
                },
            ));
        }
    }
    if let Some(row) = mode.cursor.y.checked_sub(offset)
        && row < u32::from(revision.viewport_rows)
    {
        overlays.push(OverlaySpan::new(
            u16::try_from(row).unwrap_or(u16::MAX),
            mode.cursor.x.min(revision.columns),
            mode.cursor.x.saturating_add(1).min(revision.columns),
            OverlayKind::CopyCursor,
        ));
    }
    let total = revision.total_rows().max(1);
    let overlays = dictionary.finish_overlays(overlays);
    let presentation = dictionary.shared_presentation(
        &revision.title,
        revision.working_directory.as_deref(),
        view.hover_link.as_ref().map(|link| link.uri.as_str()),
    );
    TerminalViewport {
        generation: generations.content,
        view_generation: generations.view,
        dictionary_generation: dictionary.generation,
        columns: revision.columns,
        rows: revision.viewport_rows,
        foreground: revision.foreground,
        background: revision.background,
        presentation,
        cells,
        dictionary: Arc::clone(&revision.dictionary),
        overlays,
        kitty_placements: Arc::from([]),
        cursor: None,
        scrollbar: ScrollbarState {
            total,
            offset,
            len: u32::from(revision.viewport_rows),
        },
        mode: match mode.kind {
            FrozenModeKind::Copy => TerminalMode::Copy {
                position: mode.cursor.y.saturating_add(1).min(total),
                total,
                hide_position: mode.hide_position,
            },
            FrozenModeKind::View => TerminalMode::View {
                position: mode.cursor.y.saturating_add(1).min(total),
                total,
            },
        },
        search: view.search.as_ref().map(|search| {
            SearchStatus::new(
                search
                    .current
                    .and_then(|current| u32::try_from(current.saturating_add(1)).ok())
                    .unwrap_or(0),
                u32::try_from(search.matches.len()).unwrap_or(u32::MAX),
            )
            .with_pending(search.pending)
            .with_invalid_pattern(search.invalid_pattern)
        }),
        unseen_output: view.unseen_output,
        kitty_keyboard: false,
        mouse_tracking: false,
        status,
    }
}

fn push_mode_selection_overlays(
    overlays: &mut Vec<OverlaySpan>,
    revision: &ModeRevision,
    selection: ModeSelection,
    offset: u32,
) {
    let (start, end) =
        if (selection.anchor.y, selection.anchor.x) <= (selection.focus.y, selection.focus.x) {
            (selection.anchor, selection.focus)
        } else {
            (selection.focus, selection.anchor)
        };
    let visible_end = offset.saturating_add(u32::from(revision.viewport_rows));
    let first_row = start.y.max(offset);
    let last_row = end.y.min(visible_end.saturating_sub(1));
    if first_row > last_row {
        return;
    }
    for row in first_row..=last_row {
        let (span_start, span_end) = if selection.rectangle {
            (
                selection.anchor.x.min(selection.focus.x),
                selection.anchor.x.max(selection.focus.x).saturating_add(1),
            )
        } else if selection.mode == SelectionMode::Line {
            (0, revision.columns)
        } else {
            (
                if row == start.y { start.x } else { 0 },
                if row == end.y {
                    end.x.saturating_add(1)
                } else {
                    revision.columns
                },
            )
        };
        overlays.push(OverlaySpan::with_flags(
            u16::try_from(row - offset).unwrap_or(u16::MAX),
            span_start.min(revision.columns),
            span_end.min(revision.columns),
            OverlayKind::Selection,
            if selection.rectangle {
                OVERLAY_RECTANGLE
            } else {
                0
            },
        ));
    }
}

fn style_attributes(
    style: &libghostty_vt::style::Style,
    foreground_explicit_rgb: bool,
    hyperlink: bool,
) -> u16 {
    let mut attributes = 0;
    attributes |= u16::from(style.bold) * ATTR_BOLD;
    attributes |= u16::from(style.italic) * ATTR_ITALIC;
    attributes |= u16::from(style.faint) * ATTR_FAINT;
    attributes |= u16::from(style.blink) * ATTR_BLINK;
    attributes |= u16::from(style.invisible) * ATTR_INVISIBLE;
    attributes |= u16::from(style.strikethrough) * ATTR_STRIKETHROUGH;
    attributes |= u16::from(style.overline) * ATTR_OVERLINE;
    attributes |= u16::from(foreground_explicit_rgb) * ATTR_EXPLICIT_RGB;
    attributes |= u16::from(hyperlink) * ATTR_HYPERLINK;
    attributes
}

const fn underline_style(underline: Underline) -> UnderlineStyle {
    match underline {
        Underline::None => UnderlineStyle::None,
        Underline::Double => UnderlineStyle::Double,
        Underline::Curly => UnderlineStyle::Curly,
        Underline::Dotted => UnderlineStyle::Dotted,
        Underline::Dashed => UnderlineStyle::Dashed,
        _ => UnderlineStyle::Single,
    }
}

const fn color(value: RgbColor) -> Color {
    Color::rgb(value.r, value.g, value.b)
}

fn resolve_style_color(value: StyleColor, palette: &[RgbColor; 256]) -> Option<Color> {
    match value {
        StyleColor::None => None,
        StyleColor::Palette(index) => palette.get(usize::from(index.0)).copied().map(color),
        StyleColor::Rgb(value) => Some(color(value)),
    }
}

#[cfg(test)]
mod tests {
    fn engine_filter_screen(knobs: EngineKnobs, chunks: &[&[u8]]) -> (String, Vec<String>) {
        let (text, renames, _) = engine_filter_run(knobs, chunks);
        (text, renames)
    }

    fn engine_filter_run(
        knobs: EngineKnobs,
        chunks: &[&[u8]],
    ) -> (String, Vec<String>, ProgressBar) {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 4,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut filter = EngineFilter::default();
        let mut renames = Vec::new();
        let mut bar = None;
        for chunk in chunks {
            filter.write(chunk, knobs, &mut terminal, &mut renames, &mut bar);
        }
        let revision = ModeRevision::capture(&mut terminal).expect("revision");
        let rows = revision.total_rows();
        (
            revision.capture_rows(0, rows.saturating_sub(1), false, false, false),
            renames,
            filter.bar,
        )
    }

    fn progress_after(sequences: &[&str]) -> ProgressBar {
        let chunks = sequences
            .iter()
            .map(|sequence| format!("\x1b]{sequence}\x07").into_bytes())
            .collect::<Vec<_>>();
        let borrowed = chunks
            .iter()
            .map(std::vec::Vec::as_slice)
            .collect::<Vec<_>>();
        let (_, _, bar) = engine_filter_run(EngineKnobs::default(), &borrowed);
        bar
    }

    /// Measured on pinned tmux against `#{pane_pb_state}/#{pane_pb_progress}`
    /// with one 40x6 pane: a fresh pane answers `hidden/0`, then `9;4;0` gives
    /// `hidden/0`, `9;4;1;50` `normal/50`, `9;4;2;30` `error/30`, `9;4;3`
    /// `indeterminate/30` and `9;4;4;10` `paused/10`.
    #[test]
    fn engine_filter_reads_the_pinned_progress_bar_walk() {
        assert_eq!(progress_after(&[]), ProgressBar::default());
        assert_eq!(
            progress_after(&["9;4;0"]),
            ProgressBar {
                state: ProgressBarState::Hidden,
                progress: 0,
            }
        );
        assert_eq!(
            progress_after(&["9;4;0", "9;4;1;50"]),
            ProgressBar {
                state: ProgressBarState::Normal,
                progress: 50,
            }
        );
        assert_eq!(
            progress_after(&["9;4;1;50", "9;4;2;30"]),
            ProgressBar {
                state: ProgressBarState::Error,
                progress: 30,
            }
        );
        assert_eq!(
            progress_after(&["9;4;2;30", "9;4;3"]),
            ProgressBar {
                state: ProgressBarState::Indeterminate,
                progress: 30,
            }
        );
        assert_eq!(
            progress_after(&["9;4;2;30", "9;4;4;10"]),
            ProgressBar {
                state: ProgressBarState::Paused,
                progress: 10,
            }
        );
    }

    /// The same probe's edges. From `9;4;1;50`, the pin answered
    /// `indeterminate/50` for `9;4;3;70` because `screen_set_progress_bar`
    /// refuses a percentage in that state, then left `indeterminate/50` alone
    /// through `9;4;5`, `9;4;1;101`, `9;4` and `9;4;`, moved to `error/50` on
    /// `9;4;2;` and ignored `9;5;1`.
    #[test]
    fn engine_filter_holds_the_pinned_progress_bar_edges() {
        let normal = ProgressBar {
            state: ProgressBarState::Normal,
            progress: 50,
        };
        let indeterminate = ProgressBar {
            state: ProgressBarState::Indeterminate,
            progress: 50,
        };
        assert_eq!(progress_after(&["9;4;1;50"]), normal);
        assert_eq!(progress_after(&["9;4;1;50", "9;4;3;70"]), indeterminate);
        for ignored in ["9;4;5", "9;4;1;101", "9;4", "9;4;", "9;5;1", "9;4;1;5x"] {
            assert_eq!(
                progress_after(&["9;4;1;50", "9;4;3;70", ignored]),
                indeterminate,
                "{ignored} moved the progress bar"
            );
        }
        assert_eq!(
            progress_after(&["9;4;1;50", "9;4;3;70", "9;4;2;"]),
            ProgressBar {
                state: ProgressBarState::Error,
                progress: 50,
            }
        );
        assert_eq!(
            progress_after(&["9;4;1;100"]),
            ProgressBar {
                state: ProgressBarState::Normal,
                progress: 100,
            }
        );
    }

    /// The OSC terminators the pin's `input_state_osc_string_table` shares
    /// with `INPUT_STATE_ANYWHERE`: CAN and SUB leave the state through
    /// `input_exit_osc` exactly as BEL and ST do, and a control byte inside the
    /// payload is a null transition the table never appends. Measured on the
    /// pin on 2026-09-02 with the bytes written to the pane's own tty: `9;4;1;50`
    /// ended by CAN answers normal/50, `9;4;2;20` ended by SUB answers
    /// error/20, and `9;4;1;<NUL>77` ended by BEL answers normal/77.
    #[test]
    fn engine_filter_commits_an_osc_on_can_sub_and_drops_control_bytes() {
        let (_, _, bar) =
            engine_filter_run(EngineKnobs::default(), &[b"\x1b]9;4;1;50\x18".as_slice()]);
        assert_eq!(
            bar,
            ProgressBar {
                state: ProgressBarState::Normal,
                progress: 50,
            }
        );
        let (_, _, bar) = engine_filter_run(
            EngineKnobs::default(),
            &[b"\x1b]9;4;1;50\x18".as_slice(), b"\x1b]9;4;2;20\x1a"],
        );
        assert_eq!(
            bar,
            ProgressBar {
                state: ProgressBarState::Error,
                progress: 20,
            }
        );
        let (text, _, bar) = engine_filter_run(
            EngineKnobs::default(),
            &[b"\x1b]9;4;1;\x0077\x07z".as_slice()],
        );
        assert_eq!(
            bar,
            ProgressBar {
                state: ProgressBarState::Normal,
                progress: 77,
            }
        );
        assert!(text.starts_with('z'), "{text:?}");
    }

    /// The OSC has to reach the engine unchanged, whatever the filter reads out
    /// of it, and it has to survive arriving in pieces or ending on ST.
    #[test]
    fn engine_filter_passes_an_osc_through_in_any_shape() {
        let (text, _, bar) = engine_filter_run(
            EngineKnobs::default(),
            &[b"\x1b]0;ti".as_slice(), b"tle\x1b\\a\x1b]9;4", b";1;7\x07b"],
        );
        assert_eq!(text, "ab\n\n\n");
        assert_eq!(
            bar,
            ProgressBar {
                state: ProgressBarState::Normal,
                progress: 7,
            }
        );
    }

    #[test]
    fn engine_filter_rename_follows_the_pin_table() {
        let knobs = EngineKnobs {
            allow_rename: true,
            ..EngineKnobs::default()
        };
        let (text, renames) = engine_filter_screen(
            knobs,
            &[
                b"\x1bkESCCSI\x1b[0m",
                b"\x1bkCAN\x18",
                b"\x1bkA\x07B\x1b\\",
                b"\x1b",
                b"kSPLIT",
                b"\x1b",
                b"\\",
                b"\x1bkQUIET\x1b\\",
            ],
        );
        assert_eq!(renames, ["ESCCSI", "CAN", "AB", "SPLIT", "QUIET"]);
        assert_eq!(text.trim(), "");
        let (_, refused) = engine_filter_screen(EngineKnobs::default(), &[b"\x1bkNAME\x1b\\"]);
        assert!(
            refused.is_empty(),
            "allow-rename off still renamed: {refused:?}"
        );
    }

    #[test]
    fn engine_filter_hands_an_aborted_csi_to_the_engine() {
        let (text, _) =
            engine_filter_screen(EngineKnobs::default(), &[b"hello \x1b[2\x1b[2Jworld"]);
        assert!(!text.contains("2J"), "the aborted CSI leaked: {text:?}");
        assert!(text.contains("world"), "{text:?}");
    }

    #[test]
    fn engine_filter_passes_split_sequences_through_untouched() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 4,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut filter = EngineFilter::default();
        let mut renames = Vec::new();
        for chunk in [
            b"a\x1b[".as_slice(),
            b"1",
            b"mb\x1b[0m",
            b"c\x1b",
            b"[2",
            b"Cd",
        ] {
            filter.write(
                chunk,
                EngineKnobs::default(),
                &mut terminal,
                &mut renames,
                &mut None,
            );
        }
        assert_eq!(terminal.cursor_x().expect("cursor"), 6);
        let revision = ModeRevision::capture(&mut terminal).expect("revision");
        assert_eq!(revision.capture_rows(0, 0, false, false, false), "abcd");
        assert!(revision.cell_matches_text(PointCoordinate { x: 5, y: 0 }, "d"));
    }

    #[test]
    fn engine_filter_scrolls_a_split_clear_into_history() {
        let (text, _) = engine_filter_screen(
            EngineKnobs::default(),
            &[b"one\r\ntwo\r\n", b"\x1b[H\x1b[", b"2J", b"three"],
        );
        assert_eq!(text, "one\ntwo\nthree\n\n\n");
        let (kept, _) = engine_filter_screen(
            EngineKnobs {
                scroll_on_clear: false,
                ..EngineKnobs::default()
            },
            &[b"one\r\ntwo\r\n", b"\x1b[H\x1b[2Jthree"],
        );
        assert_eq!(kept.lines().next().unwrap_or(""), "three", "{kept:?}");
        assert!(!kept.contains("one"), "{kept:?}");
    }

    #[test]
    #[ignore = "throughput measurement; run with --ignored --nocapture"]
    fn engine_filter_throughput() {
        use std::time::Instant;
        let mut coloured = Vec::new();
        while coloured.len() < 26 << 20 {
            coloured.extend_from_slice(
                b"\x1b[0m\x1b[1;34mdrwxr-xr-x\x1b[0m  12 user staff   384 Sep  2 07:00 \x1b[1;36msome-directory\x1b[0m\r\n",
            );
        }
        let mut plain = Vec::new();
        while plain.len() < 26 << 20 {
            plain.extend_from_slice(b"the quick brown fox jumps over the lazy dog 0123456789\r\n");
        }
        let mut clears = Vec::new();
        while clears.len() < 512 << 10 {
            clears.extend_from_slice(b"\x1b[H\x1b[2J");
        }
        for (name, input) in [
            ("coloured", &coloured),
            ("plain", &plain),
            ("clears", &clears),
        ] {
            for round in 0..3 {
                let mut terminal = Terminal::new(TerminalOptions {
                    cols: 200,
                    rows: 50,
                    max_scrollback: 1000,
                })
                .expect("terminal");
                let mut passthrough = PassthroughFilter::default();
                let started = Instant::now();
                for chunk in input.chunks(65536) {
                    passthrough.write(chunk, |bytes| terminal.vt_write(bytes));
                }
                let base = started.elapsed();
                let mut terminal = Terminal::new(TerminalOptions {
                    cols: 200,
                    rows: 50,
                    max_scrollback: 1000,
                })
                .expect("terminal");
                let mut passthrough = PassthroughFilter::default();
                let mut filter = EngineFilter::default();
                let mut renames = Vec::new();
                let mut bar = None;
                let mut engine = EngineOutput {
                    filter: &mut filter,
                    knobs: EngineKnobs::default(),
                    renames: &mut renames,
                    bar: &mut bar,
                };
                let started = Instant::now();
                for chunk in input.chunks(65536) {
                    feed_pty_output(&mut terminal, &mut passthrough, &mut engine, chunk);
                }
                let filtered = started.elapsed();
                println!("{name} round {round}: passthrough {base:?} filtered {filtered:?}");
            }
        }
    }

    use super::*;
    use crate::DEFAULT_HISTORY_LIMIT;

    fn filter_passthrough(mode: AllowPassthrough, chunks: &[&[u8]]) -> Vec<u8> {
        let mut filter = PassthroughFilter::default();
        filter.set_mode(mode);
        let mut output = Vec::new();
        for chunk in chunks {
            filter.write(chunk, |bytes| output.extend_from_slice(bytes));
        }
        output
    }

    #[test]
    fn passthrough_fast_path_is_one_borrowed_write_without_allocation() {
        let input = b"ordinary terminal output without escapes";
        let mut filter = PassthroughFilter::default();
        let mut calls = 0;
        let written = filter.write(input, |output| {
            calls += 1;
            assert!(std::ptr::eq(output.as_ptr(), input.as_ptr()));
        });
        assert_eq!(written, input.len());
        assert_eq!(calls, 1);
        assert_eq!(filter.payload.capacity(), 0);
        assert_eq!(filter.state, PassthroughState::Ground);
    }

    #[test]
    fn tmux_passthrough_unwraps_split_kitty_bytes_once() {
        let output = filter_passthrough(
            AllowPassthrough::All,
            &[
                b"\x1bPt",
                b"mux;\x1b\x1b_Ga=T,f=24,s=1,v=1,i=42;/wAA\x1b",
                b"\x1b\\\x1b\\",
            ],
        );
        assert_eq!(output, b"\x1b_Ga=T,f=24,s=1,v=1,i=42;/wAA\x1b\\");
    }

    #[test]
    fn tmux_passthrough_off_consumes_the_complete_wrapper() {
        let output = filter_passthrough(
            AllowPassthrough::Off,
            &[b"before\x1bPtm", b"ux;hidden\x1b", b"\\after"],
        );
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn tmux_passthrough_un_doubles_payload_escapes() {
        let output = filter_passthrough(
            AllowPassthrough::All,
            &[b"\x1bPtmux;one\x1b", b"\x1b[31mtwo\x1b\\"],
        );
        assert_eq!(output, b"one\x1b[31mtwo");
    }

    #[test]
    fn tmux_passthrough_drops_a_single_dcs_escape_byte() {
        let output =
            filter_passthrough(AllowPassthrough::All, &[b"\x1bPtmux;one\x1b[31mtwo\x1b\\"]);
        assert_eq!(output, b"one[31mtwo");
    }

    #[test]
    fn oversized_tmux_passthrough_is_discarded_bounded_and_released() {
        for (name, mode) in [
            ("off", AllowPassthrough::Off),
            ("on", AllowPassthrough::All),
            ("all", AllowPassthrough::All),
        ] {
            let mut filter = PassthroughFilter::default();
            filter.set_mode(mode);
            let mut output = Vec::new();
            filter.write(TMUX_PASSTHROUGH_PREFIX, |bytes| {
                output.extend_from_slice(bytes);
            });
            let payload = vec![b'x'; MAX_TMUX_PASSTHROUGH_PAYLOAD_BYTES];
            filter.write(&payload, |bytes| output.extend_from_slice(bytes));
            assert_eq!(filter.payload.len(), MAX_TMUX_PASSTHROUGH_PAYLOAD_BYTES);
            filter.write(b"y", |bytes| output.extend_from_slice(bytes));
            assert!(
                matches!(filter.state, PassthroughState::Discard { .. }),
                "{name}"
            );
            assert_eq!(filter.payload.capacity(), 0, "{name}");
            filter.write(b"still hidden", |bytes| output.extend_from_slice(bytes));
            assert!(output.is_empty(), "{name}");
            filter.write(b"\x1b\\after", |bytes| output.extend_from_slice(bytes));
            assert_eq!(output, b"after", "{name}");
            assert_eq!(filter.state, PassthroughState::Ground, "{name}");
            assert_eq!(filter.payload.capacity(), 0, "{name}");
        }
    }

    #[test]
    fn non_tmux_sixel_and_control_dcs_pass_through_in_every_effective_mode() {
        for input in [
            b"\x1bPq#0;2;0;0;0~\x1b\\".as_slice(),
            b"\x1bP1000pbegin %1 1 0\x1b\\".as_slice(),
        ] {
            for (name, mode) in [
                ("off", AllowPassthrough::Off),
                ("on", AllowPassthrough::All),
                ("all", AllowPassthrough::All),
            ] {
                assert_eq!(filter_passthrough(mode, &[input]), input, "{name}");
                for split in 0..=input.len() {
                    assert_eq!(
                        filter_passthrough(mode, &[&input[..split], &input[split..]]),
                        input,
                        "{name} split={split}"
                    );
                }
            }
        }
    }

    #[test]
    fn search_stepping_clamps_when_wrapping_is_disabled() {
        let matches = vec![
            SearchMatch {
                row: 0,
                start: 0,
                end: 1,
            },
            SearchMatch {
                row: 1,
                start: 0,
                end: 1,
            },
            SearchMatch {
                row: 2,
                start: 0,
                end: 1,
            },
        ];
        let mut search = Some(Box::new(SearchState {
            matches,
            current: Some(2),
            ..SearchState::default()
        }));

        step_search(&mut search, 1, false);
        assert_eq!(search.as_ref().and_then(|search| search.current), Some(2));
        step_search(&mut search, -4, false);
        assert_eq!(search.as_ref().and_then(|search| search.current), Some(0));
        step_search(&mut search, -1, true);
        assert_eq!(search.as_ref().and_then(|search| search.current), Some(2));
    }

    fn snapshot_fixture(terminal: &Terminal<'_, '_>) -> TerminalViewport {
        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();
        snapshot(
            terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("snapshot")
    }

    #[test]
    fn reported_working_directories_strip_file_hosts_and_decode_paths() {
        let (uri, expected) = if cfg!(windows) {
            (
                "file://workstation/C:/Users/a%20b/%E7%95%8C",
                "C:\\Users\\a b\\界",
            )
        } else {
            ("file://workstation/tmp/a%20b/%E7%95%8C", "/tmp/a b/界")
        };
        assert_eq!(reported_working_directory(uri).as_deref(), Some(expected));
        if !cfg!(windows) {
            assert_eq!(
                reported_working_directory("file://workstation/tmp/a b").as_deref(),
                Some("/tmp/a b")
            );
        }
        assert_eq!(
            reported_working_directory("/reported/path").as_deref(),
            Some("/reported/path")
        );
        assert_eq!(reported_working_directory(""), None);
        assert_eq!(reported_working_directory("file://[invalid"), None);
    }

    fn copy_mode_survives_downward_action(
        action: CopyModeAction,
        scroll_exit: bool,
        selected: bool,
        starts_at_bottom: bool,
    ) -> bool {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive");
        let mut live_selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            scroll_exit,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mode = copy_mode.as_mut().expect("mode");
        let maximum = mode.revision.maximum_offset();
        assert!(maximum > 0);
        if starts_at_bottom {
            assert_eq!(mode.viewport_offset, maximum);
        } else {
            let page = u32::from(mode.revision.viewport_rows.max(1));
            mode.viewport_offset = maximum - 1;
            mode.cursor.y = mode
                .viewport_offset
                .saturating_add(page.saturating_sub(1))
                .min(mode.revision.total_rows().saturating_sub(1));
        }
        if selected {
            mode.selection = Some(ModeSelection {
                anchor: mode.cursor,
                focus: mode.cursor,
                mode: SelectionMode::Cell,
                rectangle: false,
            });
            mode.selecting = true;
        }
        let mut unseen_output = 7;
        let result = apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            action,
            &WordSeparators::default(),
            false,
        )
        .expect("copy mode action");
        assert!(matches!(result, ViewActionResult::Snapshot));
        let survived = copy_mode.is_some();
        assert_eq!(unseen_output, if survived { 7 } else { 0 });
        survived
    }

    fn test_base64(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let bits = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            output.push(char::from(TABLE[((bits >> 18) & 0x3f) as usize]));
            output.push(char::from(TABLE[((bits >> 12) & 0x3f) as usize]));
            output.push(if chunk.len() > 1 {
                char::from(TABLE[((bits >> 6) & 0x3f) as usize])
            } else {
                '='
            });
            output.push(if chunk.len() > 2 {
                char::from(TABLE[(bits & 0x3f) as usize])
            } else {
                '='
            });
        }
        output
    }

    #[test]
    fn kitty_pixel_formats_convert_to_premultiplied_bgra() {
        assert_eq!(
            kitty_to_premultiplied_bgra(&[0x80], ImageFormat::Gray, 4),
            [0x80, 0x80, 0x80, 0xff]
        );
        assert_eq!(
            kitty_to_premultiplied_bgra(&[200, 128], ImageFormat::GrayAlpha, 4),
            [100, 100, 100, 128]
        );
        assert_eq!(
            kitty_to_premultiplied_bgra(&[1, 2, 3], ImageFormat::Rgb, 4),
            [3, 2, 1, 0xff]
        );
        assert_eq!(
            kitty_to_premultiplied_bgra(&[100, 50, 200, 128], ImageFormat::Rgba, 4),
            [100, 25, 50, 128]
        );
    }

    #[test]
    fn kitty_vt_write_exports_placement_metadata_and_owned_pixels() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.resize(8, 2, 8, 18).expect("terminal pixels");
        terminal.vt_write(b"\x1b_Ga=T,f=24,s=1,v=1,i=42;/wAA\x1b\\");

        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::new().expect("Kitty iterator");
        let mut dictionary = ViewportDictionary::default();
        let viewport = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("Kitty snapshot");
        let placement = viewport
            .kitty_placements
            .first()
            .expect("displayed Kitty placement");
        assert_eq!(placement.image_id, 42);
        assert_eq!(placement.viewport_col, 0);
        assert_eq!(placement.viewport_row, 0);
        assert_eq!(placement.absolute_row, u64::from(viewport.scrollbar.offset));
        assert_eq!(placement.layer, KittyLayer::AboveText);

        let image = generations
            .kitty
            .as_mut()
            .expect("Kitty state")
            .image(&terminal, 42)
            .expect("image query")
            .expect("stored image");
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.generation, placement.image_generation);
        assert_eq!(image.bgra, [0, 0, 255, 255]);
    }

    #[test]
    fn kitty_storage_quota_admits_wire_cap_sized_images() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.resize(8, 2, 8, 18).expect("terminal pixels");
        configure_kitty_storage(&mut terminal).expect("raise Kitty storage quota");

        let encoded = "AAAA".repeat(2000 * 2000);
        let chunks = encoded.as_bytes().chunks(4096).collect::<Vec<_>>();
        for (index, chunk) in chunks.iter().enumerate() {
            let more = usize::from(index < chunks.len() - 1);
            let mut apc = if index == 0 {
                format!("\x1b_Ga=T,f=24,s=2000,v=2000,i=91,m={more};").into_bytes()
            } else {
                format!("\x1b_Gm={more};").into_bytes()
            };
            apc.extend_from_slice(chunk);
            apc.extend_from_slice(b"\x1b\\");
            terminal.vt_write(&apc);
        }

        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::new().expect("Kitty iterator");
        let mut dictionary = ViewportDictionary::default();
        let viewport = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("Kitty snapshot");
        assert!(
            viewport
                .kitty_placements
                .iter()
                .any(|placement| placement.image_id == 91),
            "a 12 MB image should fit the raised Kitty storage quota"
        );
    }

    #[test]
    fn kitty_png_decoder_installs_through_the_safe_wrapper() {
        use image::ImageEncoder as _;

        install_kitty_png_decoder();
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 128], 1, 1, image::ExtendedColorType::Rgba8)
            .expect("encode PNG fixture");
        let payload = test_base64(&png);
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(format!("\x1b_Ga=T,f=100,i=43;{payload}\x1b\\").as_bytes());

        let mut kitty = KittyGraphicsState::new().expect("Kitty iterator");
        let image = kitty
            .image(&terminal, 43)
            .expect("image query")
            .expect("decoded PNG image");
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.bgra, [0, 0, 128, 128]);
    }

    fn test_pointer_input(phase: TerminalMousePhase, column: u16) -> TerminalMouseInput {
        TerminalMouseInput::new(
            phase,
            (phase == TerminalMousePhase::Press).then_some(TerminalMouseButton::Left),
            PointerCellEvent {
                column: column % 80,
                row: column % 24,
                click_count: 1,
                rectangle: false,
            },
            u32::from(column % 80) * 8 + 1,
            u32::from(column % 24) * 18 + 1,
            640,
            432,
            8,
            18,
            crate::Modifiers::default(),
            false,
        )
    }

    #[test]
    fn actor_command_mailbox_backpressures_after_one_pending_command() {
        let (commands, pending) = command_channel();
        commands
            .try_send(Command::Shutdown)
            .expect("first pending command");
        assert!(matches!(
            commands.try_send(Command::Shutdown),
            Err(crossbeam_channel::TrySendError::Full(Command::Shutdown))
        ));
        assert!(matches!(
            pending.try_recv().expect("pending command"),
            Command::Shutdown
        ));
    }

    #[test]
    fn view_action_size_limits_are_nonfatal() {
        for error in [
            WorkerError::SearchSnapshotTooLarge,
            WorkerError::ModeRevisionTooLarge,
        ] {
            assert!(matches!(
                normalize_view_action_result(Err(error)),
                Ok(ViewActionResult::Snapshot)
            ));
        }
        assert!(matches!(
            normalize_view_action_result(Err(WorkerError::Thread("stopped".to_owned()))),
            Err(WorkerError::Thread(message)) if message == "stopped"
        ));
    }

    #[cfg(unix)]
    fn term_ignoring_test_session(view: TerminalViewId, command: &str) -> TerminalSession {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                command: Some(vec![command.to_owned()]),
                ..TerminalSpawn::default()
            },
        );
        session.attach_view(view);
        wait_for_test_viewport(&session, |viewport| {
            let mut contents = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut contents);
            }
            contents.contains("TERMINATION_READY")
        });
        session
    }

    #[cfg(unix)]
    fn kill_test_process_group(process_id: u32) {
        if let Some(group) = i32::try_from(process_id)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
    }

    #[cfg(unix)]
    #[test]
    fn terminate_escalates_while_session_is_retained() {
        let session = term_ignoring_test_session(
            TerminalViewId(301),
            "sh -c 'trap \"\" TERM HUP; while :; do sleep 1; done' & printf 'TERMINATION_READY\\r\\n'",
        );
        let process_id = session.process_id().expect("shell process id");
        session.terminate();
        let deadline = Instant::now() + Duration::from_secs(2);
        while session.completion().is_none() {
            if Instant::now() >= deadline {
                kill_test_process_group(process_id);
                panic!("terminal did not escalate termination");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn dropping_a_terminating_session_never_waits_forever() {
        let session = term_ignoring_test_session(
            TerminalViewId(302),
            "trap '' TERM HUP; printf 'TERMINATION_READY\\r\\n'; while :; do sleep 1; done",
        );
        let process_id = session.process_id().expect("shell process id");
        let events = session.events();
        session.terminate();
        drop(session);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !events.receiver.is_closed() {
            if Instant::now() >= deadline {
                kill_test_process_group(process_id);
                panic!("terminating terminal worker did not stop");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    #[test]
    fn wake_pipe_drain_reads_until_empty() {
        let (read, write) = configured_actor_wake_pipe().expect("configured wake pipe");
        for fd in [&read, &write] {
            assert!(
                rustix::io::fcntl_getfd(fd)
                    .expect("wake fd flags")
                    .contains(rustix::io::FdFlags::CLOEXEC)
            );
            assert!(
                rustix::fs::fcntl_getfl(fd)
                    .expect("wake status flags")
                    .contains(rustix::fs::OFlags::NONBLOCK)
            );
        }

        for length in [128, 129] {
            let bytes = vec![1_u8; length];
            assert_eq!(
                rustix::io::write(&write, &bytes).expect("fill wake pipe"),
                length
            );
            drain_wake_pipe(&read).expect("drain wake pipe");
            let mut byte = [0_u8; 1];
            assert!(matches!(
                rustix::io::read(&read, &mut byte),
                Err(rustix::io::Errno::AGAIN)
            ));
        }
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    #[test]
    fn wake_pipe_write_retries_interrupt_and_accepts_a_full_pipe() {
        let mut attempts = 0;
        write_actor_wake(|| {
            attempts += 1;
            if attempts == 1 {
                Err(rustix::io::Errno::INTR)
            } else {
                Ok(1)
            }
        })
        .expect("interrupted wake retry");
        assert_eq!(attempts, 2);

        write_actor_wake(|| Err(rustix::io::Errno::AGAIN)).expect("full pipe is already readable");
    }

    #[cfg(unix)]
    #[test]
    fn saturated_pty_input_does_not_block_resize_or_shutdown() {
        let session = Arc::new(TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                command: Some(vec!["sleep 30".to_owned()]),
                ..TerminalSpawn::default()
            },
        ));
        wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.status, SessionStatus::Running)
        });
        let view = TerminalViewId(1);
        session.attach_view(view);
        let deadline = Instant::now() + Duration::from_secs(2);
        while session.latest_viewport_for(view).is_none() {
            assert!(Instant::now() < deadline, "view never attached");
            thread::sleep(Duration::from_millis(10));
        }

        let chunk = Arc::<str>::from("x".repeat(1024 * 1024));
        let mut accepted = 0;
        let mut rejected = 0;
        for _ in 0..(MAX_PENDING_PTY_INPUT_BYTES / chunk.len() + 8) {
            if session.send_command(Command::Text {
                view: Some(view),
                text: Arc::clone(&chunk),
            }) {
                accepted += 1;
            } else {
                rejected += 1;
                break;
            }
        }
        assert!(accepted > 0);
        assert_eq!(rejected, 1);

        let (pending_commands, pending_bytes) = session.commands.pending_input();
        let diagnostics = session.diagnostics();
        assert_eq!(diagnostics.command_queue_len, pending_commands);
        assert_eq!(diagnostics.pending_pty_input_bytes, pending_bytes);
        assert!(diagnostics.pending_pty_input_bytes >= chunk.len());

        session.resize(37, 9, 8, 18);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let viewport = session.latest_viewport();
            if viewport.columns == 37 && viewport.rows == 9 {
                break;
            }
            assert!(Instant::now() < deadline, "resize stayed behind PTY input");
            thread::sleep(Duration::from_millis(10));
        }

        assert!(session.send_command(Command::ViewAction {
            view,
            action: TerminalViewAction::EnterCopyMode,
        }));
        wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.mode, TerminalMode::Copy { .. })
        });

        let events = session.events();
        drop(session);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !events.receiver.is_closed() {
            assert!(
                Instant::now() < deadline,
                "shutdown stayed behind PTY input"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn pty_output_drains_while_the_input_writer_is_backpressured() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                command: Some(vec![
                    "stty raw -echo; sleep 1; dd if=/dev/zero bs=262144 count=1 2>/dev/null; IFS= read -r line; printf '\\r\\nZZ_DUPLEX_DRAIN_OK\\r\\n'; sleep 30"
                        .to_owned(),
                ]),
                ..TerminalSpawn::default()
            },
        );
        let view = TerminalViewId(2);
        session.attach_view(view);
        wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.status, SessionStatus::Running)
        });

        assert!(session.send_command(Command::Text {
            view: Some(view),
            text: Arc::from(format!("{}\n", "i".repeat(256 * 1024))),
        }));
        let deadline = Instant::now() + Duration::from_secs(2);
        while session.commands.pending_input().0 == 0
            || !session
                .commands
                .queues
                .input
                .as_ref()
                .expect("live terminal input queue")
                .commands
                .is_empty()
        {
            assert!(
                Instant::now() < deadline,
                "PTY input never reached the backpressured writer"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let viewport = session.latest_viewport_for(view);
            let mut contents = String::new();
            if let Some(viewport) = viewport {
                for cell in viewport.cells.iter() {
                    viewport.push_glyph(*cell, &mut contents);
                }
            }
            if contents.contains("ZZ_DUPLEX_DRAIN_OK") {
                assert_eq!(session.commands.pending_input(), (0, 0));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "PTY output stopped behind the backpressured writer"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn pty_input_admission_is_count_and_byte_bounded() {
        let (input, pending) = input_channel_with_limits(2, PTY_INPUT_COMMAND_FLOOR_BYTES * 4);
        let small = || Command::PendingPasteOpened { token: 1 };
        input.try_send(small()).expect("first input");
        input.try_send(small()).expect("second input");
        assert!(matches!(
            input.try_send(small()),
            Err(crossbeam_channel::TrySendError::Full(
                Command::PendingPasteOpened { .. }
            ))
        ));
        assert_eq!(input.pending(), (2, PTY_INPUT_COMMAND_FLOOR_BYTES * 2));
        drop(pending.recv().expect("release first input"));
        input.try_send(small()).expect("count released");

        let (input, pending) = input_channel_with_limits(8, PTY_INPUT_COMMAND_FLOOR_BYTES * 2);
        assert!(matches!(
            input.try_send(Command::Text {
                view: None,
                text: Arc::from("x".repeat(PTY_INPUT_COMMAND_FLOOR_BYTES + 1)),
            }),
            Err(crossbeam_channel::TrySendError::Full(Command::Text { .. }))
        ));
        assert!(pending.is_empty());
        assert_eq!(input.pending(), (0, 0));
    }

    #[test]
    fn pty_input_lane_preserves_paste_order_and_keeps_view_actions_on_control() {
        let (control, control_rx) = command_channel();
        let (input, input_rx) = input_channel();
        let commands = CommandSender {
            queues: Box::new(CommandQueues {
                control,
                input: Some(input),
            }),
            wake: ActorWake::none(),
        };
        commands
            .send(Command::PendingPasteOpened { token: 7 })
            .expect("open pending paste");
        commands
            .send(Command::ViewAction {
                view: TerminalViewId(1),
                action: TerminalViewAction::Paste("image.png".to_owned()),
            })
            .expect("paste path");
        commands
            .send(Command::ViewAction {
                view: TerminalViewId(1),
                action: TerminalViewAction::ScrollTop,
            })
            .expect("scroll view");

        assert!(matches!(
            input_rx.recv().expect("pending paste input").command,
            Command::PendingPasteOpened { token: 7 }
        ));
        assert!(matches!(
            input_rx.recv().expect("paste input").command,
            Command::ViewAction {
                action: TerminalViewAction::Paste(_),
                ..
            }
        ));
        assert!(matches!(
            control_rx.recv().expect("view control"),
            Command::ViewAction {
                action: TerminalViewAction::ScrollTop,
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn pty_writer_retains_every_reported_byte_during_saturation() {
        let (read, write) = rustix::pipe::pipe().expect("PTY writer test pipe");
        rustix::io::ioctl_fionbio(&read, true).expect("nonblocking pipe reader");
        rustix::io::ioctl_fionbio(&write, true).expect("nonblocking pipe writer");
        let filler = [0_u8; 4096];
        loop {
            match rustix::io::write(&write, &filler) {
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => break,
                Err(error) => panic!("fill writer test pipe: {error}"),
            }
        }

        let fd = filedescriptor::FileDescriptor::dup(&write).expect("duplicate pipe writer");
        let mut writer = PtyWriter::new(fd);
        let chunk = vec![1_u8; 32 * 1024];
        for _ in 0..8 {
            assert_eq!(
                writer.write(&chunk).expect("queue saturated input"),
                chunk.len()
            );
        }
        assert_eq!(writer.queued_bytes(), chunk.len() * 8);
    }

    #[cfg(unix)]
    #[test]
    fn pty_effects_wait_for_the_existing_writer_backlog() {
        let (read, write) = rustix::pipe::pipe().expect("PTY effects test pipe");
        rustix::io::ioctl_fionbio(&read, true).expect("nonblocking pipe reader");
        rustix::io::ioctl_fionbio(&write, true).expect("nonblocking pipe writer");
        let filler = [0_u8; 4096];
        let mut filler_bytes = 0;
        loop {
            match rustix::io::write(&write, &filler) {
                Ok(written) => filler_bytes += written,
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => break,
                Err(error) => panic!("fill PTY effects pipe: {error}"),
            }
        }

        let fd = filedescriptor::FileDescriptor::dup(&write).expect("duplicate pipe writer");
        let mut writer = PtyWriter::new(fd);
        writer.write_all(b"input").expect("queue PTY input");
        assert_eq!(writer.queued_bytes(), 5);

        let effects = RefCell::new(PtyEffects::new());
        effects.borrow_mut().push(b"reply");
        drain_effects_if_writer_ready(&effects, &mut writer).expect("defer PTY effect");
        assert_eq!(effects.borrow().bytes, b"reply");

        let mut scratch = [0_u8; 8192];
        let mut drained_filler = 0;
        while drained_filler < filler_bytes {
            match rustix::io::read(&read, &mut scratch) {
                Ok(read) => drained_filler += read,
                Err(rustix::io::Errno::INTR) => {}
                Err(error) => panic!("drain PTY effects filler: {error}"),
            }
        }
        writer.flush_pending().expect("flush PTY input");
        assert!(!writer.has_pending());
        drain_effects_if_writer_ready(&effects, &mut writer).expect("flush PTY effect");
        assert!(effects.borrow().bytes.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn pty_writer_preserves_order_across_partial_writes() {
        let (read, write) = rustix::pipe::pipe().expect("PTY writer test pipe");
        rustix::io::ioctl_fionbio(&read, true).expect("nonblocking pipe reader");
        rustix::io::ioctl_fionbio(&write, true).expect("nonblocking pipe writer");
        let filler = [0_u8; 4096];
        let mut filler_bytes = 0;
        loop {
            match rustix::io::write(&write, &filler) {
                Ok(written) => filler_bytes += written,
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => break,
                Err(error) => panic!("fill writer test pipe: {error}"),
            }
        }

        let fd = filedescriptor::FileDescriptor::dup(&write).expect("duplicate pipe writer");
        let mut writer = PtyWriter::new(fd);
        let first = vec![0x31_u8; 96 * 1024];
        let second = vec![0x32_u8; 96 * 1024];
        assert_eq!(
            writer.write(&first).expect("queue first input"),
            first.len()
        );
        assert_eq!(
            writer.write(&second).expect("queue second input"),
            second.len()
        );

        let mut scratch = [0_u8; 8192];
        let mut drained_filler = 0;
        while drained_filler < filler_bytes {
            match rustix::io::read(&read, &mut scratch) {
                Ok(read) => drained_filler += read,
                Err(rustix::io::Errno::INTR) => {}
                Err(error) => panic!("drain writer test filler: {error}"),
            }
        }

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut actual = Vec::with_capacity(first.len() + second.len());
        while writer.has_pending() {
            writer.flush_pending().expect("flush queued input");
            loop {
                match rustix::io::read(&read, &mut scratch) {
                    Ok(read) => actual.extend_from_slice(&scratch[..read]),
                    Err(rustix::io::Errno::INTR) => {}
                    Err(rustix::io::Errno::AGAIN) => break,
                    Err(error) => panic!("read flushed input: {error}"),
                }
            }
            assert!(Instant::now() < deadline, "partial writes never drained");
        }
        loop {
            match rustix::io::read(&read, &mut scratch) {
                Ok(read) => actual.extend_from_slice(&scratch[..read]),
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => break,
                Err(error) => panic!("read final flushed input: {error}"),
            }
        }

        let mut expected = first;
        expected.extend_from_slice(&second);
        assert_eq!(actual, expected);
    }

    #[cfg(unix)]
    #[test]
    fn pty_writer_releases_large_drained_allocation() {
        let sink = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .expect("null sink");
        let fd = filedescriptor::FileDescriptor::dup(&sink).expect("duplicate null sink");
        let mut writer = PtyWriter::new(fd);

        writer
            .write_all(&vec![0_u8; PTY_WRITE_RETAIN_BYTES * 2])
            .expect("write large input");
        writer.flush_pending().expect("drain large input");

        assert!(!writer.has_pending());
        assert_eq!(writer.pending.capacity(), 0);
    }

    #[test]
    fn terminal_event_queue_has_one_coalesced_and_four_reliable_slots() {
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let publisher = Publisher {
            event_tx,
            latest: Arc::new(RwLock::new(PublishedViewports::new(
                TerminalViewport::blank(1, 1, SessionStatus::Running),
            ))),
            state: Arc::clone(&event_state),
        };

        for _ in 0..8 {
            publisher.publish(TerminalViewport::blank(1, 1, SessionStatus::Running));
        }
        assert_eq!(
            events.receiver.capacity(),
            Some(MAX_PENDING_TERMINAL_EVENTS)
        );
        assert_eq!(events.receiver.len(), 1);

        for index in 0..MAX_PENDING_RELIABLE_EVENTS {
            publisher
                .send_reliable(TerminalEvent::OpenUri(Box::new(TerminalOpenUri {
                    view: TerminalViewId(1),
                    uri: format!("https://example.com/{index}"),
                })))
                .expect("reliable event within physical queue budget");
        }
        assert_eq!(events.receiver.len(), MAX_PENDING_TERMINAL_EVENTS);
        assert!(events.receiver.is_full());
        assert!(matches!(
            publisher.send_reliable(TerminalEvent::OpenUri(Box::new(TerminalOpenUri {
                view: TerminalViewId(1),
                uri: "https://example.com/overflow".to_owned(),
            }))),
            Err(WorkerError::EventBackpressure)
        ));

        assert!(matches!(
            events.try_recv().expect("coalesced viewport event"),
            TerminalEvent::ViewportReady {
                output_activity: false
            }
        ));
        assert!(!event_state.notification_pending.load(Ordering::Acquire));
        publisher.mark_output_activity();
        publisher.publish(TerminalViewport::blank(1, 1, SessionStatus::Running));
        assert_eq!(events.receiver.len(), MAX_PENDING_TERMINAL_EVENTS);
        assert!(events.receiver.is_full());

        let mut reliable_events = 0;
        let mut viewport_events = 0;
        let mut output_activity_events = 0;
        while let Ok(event) = events.try_recv() {
            match event {
                TerminalEvent::ViewportReady { output_activity } => {
                    viewport_events += 1;
                    output_activity_events += usize::from(output_activity);
                }
                TerminalEvent::OpenUri(_) => reliable_events += 1,
                TerminalEvent::ViewClosed(_)
                | TerminalEvent::CopyReady { .. }
                | TerminalEvent::ClipboardSet { .. }
                | TerminalEvent::Bell
                | TerminalEvent::RenameWindow(_)
                | TerminalEvent::PlaceholderBound { .. }
                | TerminalEvent::PendingPasteExpired { .. }
                | TerminalEvent::RawOutputTapClosed { .. } => {
                    panic!("unexpected event in queue invariant test")
                }
            }
        }
        assert_eq!(viewport_events, 1);
        assert_eq!(output_activity_events, 1);
        assert_eq!(reliable_events, MAX_PENDING_RELIABLE_EVENTS);
        assert_eq!(event_state.pending_reliable.load(Ordering::Acquire), 0);
        assert_eq!(
            event_state.pending_reliable_bytes.load(Ordering::Acquire),
            0
        );
        assert!(!event_state.notification_pending.load(Ordering::Acquire));
    }

    #[test]
    fn reliable_terminal_event_backlog_is_bounded_and_released_on_receive() {
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let publisher = Publisher {
            event_tx,
            latest: Arc::new(RwLock::new(PublishedViewports::new(
                TerminalViewport::blank(1, 1, SessionStatus::Running),
            ))),
            state: Arc::clone(&event_state),
        };

        for index in 0..MAX_PENDING_RELIABLE_EVENTS {
            publisher
                .send_reliable(TerminalEvent::OpenUri(Box::new(TerminalOpenUri {
                    view: TerminalViewId(1),
                    uri: format!("https://example.com/{index}"),
                })))
                .expect("reliable event within budget");
        }
        assert_eq!(
            event_state.pending_reliable.load(Ordering::Acquire),
            MAX_PENDING_RELIABLE_EVENTS
        );
        assert!(
            event_state.pending_reliable_bytes.load(Ordering::Acquire)
                <= MAX_PENDING_RELIABLE_EVENT_BYTES
        );
        assert!(matches!(
            publisher.send_reliable(TerminalEvent::OpenUri(Box::new(TerminalOpenUri {
                view: TerminalViewId(1),
                uri: "https://example.com/overflow".to_owned(),
            }))),
            Err(WorkerError::EventBackpressure)
        ));

        let pending_bytes_before_receive =
            event_state.pending_reliable_bytes.load(Ordering::Acquire);
        let event = events.try_recv().expect("first reliable event");
        let received_bytes = reliable_event_bytes(&event);
        assert!(matches!(event, TerminalEvent::OpenUri(_)));
        assert_eq!(
            event_state.pending_reliable.load(Ordering::Acquire),
            MAX_PENDING_RELIABLE_EVENTS - 1
        );
        assert_eq!(
            event_state.pending_reliable_bytes.load(Ordering::Acquire),
            pending_bytes_before_receive - received_bytes
        );
        publisher
            .send_reliable(TerminalEvent::OpenUri(Box::new(TerminalOpenUri {
                view: TerminalViewId(1),
                uri: "https://example.com/reused-slot".to_owned(),
            })))
            .expect("released reliable-event slot");
        assert_eq!(
            event_state.pending_reliable.load(Ordering::Acquire),
            MAX_PENDING_RELIABLE_EVENTS
        );
    }

    #[test]
    fn reliable_terminal_event_byte_limit_rolls_back_reserved_message_slot() {
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let publisher = Publisher {
            event_tx,
            latest: Arc::new(RwLock::new(PublishedViewports::new(
                TerminalViewport::blank(1, 1, SessionStatus::Running),
            ))),
            state: Arc::clone(&event_state),
        };
        let event = TerminalEvent::OpenUri(Box::new(TerminalOpenUri {
            view: TerminalViewId(1),
            uri: "x".to_owned(),
        }));
        let occupied_bytes = MAX_PENDING_RELIABLE_EVENT_BYTES
            .saturating_sub(reliable_event_bytes(&event))
            .saturating_add(1);
        event_state
            .pending_reliable_bytes
            .store(occupied_bytes, Ordering::Release);

        assert!(matches!(
            publisher.send_reliable(TerminalEvent::OpenUri(Box::new(TerminalOpenUri {
                view: TerminalViewId(1),
                uri: "x".to_owned(),
            }))),
            Err(WorkerError::EventBackpressure)
        ));
        assert_eq!(event_state.pending_reliable.load(Ordering::Acquire), 0);
        assert_eq!(
            event_state.pending_reliable_bytes.load(Ordering::Acquire),
            occupied_bytes
        );
        assert!(matches!(
            events.try_recv(),
            Err(async_channel::TryRecvError::Empty)
        ));
    }

    #[test]
    fn user_actions_drop_backpressure_without_stopping_the_worker() {
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let publisher = Publisher {
            event_tx,
            latest: Arc::new(RwLock::new(PublishedViewports::new(
                TerminalViewport::blank(1, 1, SessionStatus::Running),
            ))),
            state: Arc::clone(&event_state),
        };

        for index in 0..MAX_PENDING_RELIABLE_EVENTS {
            publisher
                .send_reliable(TerminalEvent::OpenUri(Box::new(TerminalOpenUri {
                    view: TerminalViewId(1),
                    uri: format!("https://example.com/{index}"),
                })))
                .expect("fill reliable queue");
        }

        publisher
            .open_uri(TerminalViewId(1), "https://example.com/dropped".to_owned())
            .expect("URI backpressure is nonfatal");
        publisher
            .copy_ready(
                TerminalViewId(1),
                Box::new(TerminalCopyReady {
                    request_id: 7,
                    clipboard: None,
                    buffer: None,
                    pipe: None,
                    text: "dropped copy".to_owned(),
                    view_changed: false,
                }),
            )
            .expect("copy backpressure is nonfatal");
        assert_eq!(
            event_state.pending_reliable.load(Ordering::Acquire),
            MAX_PENDING_RELIABLE_EVENTS
        );
        assert_eq!(events.receiver.len(), MAX_PENDING_RELIABLE_EVENTS);
    }

    #[test]
    fn stopped_event_consumer_remains_fatal_for_user_actions() {
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let publisher = Publisher {
            event_tx,
            latest: Arc::new(RwLock::new(PublishedViewports::new(
                TerminalViewport::blank(1, 1, SessionStatus::Running),
            ))),
            state: Arc::clone(&event_state),
        };
        drop(events);

        assert!(matches!(
            publisher.open_uri(TerminalViewId(1), "https://example.com".to_owned()),
            Err(WorkerError::EventConsumerStopped)
        ));
        assert_eq!(event_state.pending_reliable.load(Ordering::Acquire), 0);
        assert_eq!(
            event_state.pending_reliable_bytes.load(Ordering::Acquire),
            0
        );
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn actor_event_handles_share_compact_queue_state() {
        let word = std::mem::size_of::<usize>();
        assert!(std::mem::size_of::<EventQueueState>() <= 6 * word);
        assert_eq!(std::mem::size_of::<Publisher>(), 3 * word);
        assert_eq!(std::mem::size_of::<TerminalEvents>(), 3 * word);
        assert!(
            std::mem::size_of::<TerminalSession>() <= 16 * word,
            "{}",
            std::mem::size_of::<TerminalSession>()
        );
    }

    #[test]
    fn dictionary_last_style_cache_bypasses_hash_lookup_for_runs() {
        let foreground = Color::rgb(216, 222, 233);
        let background = Color::rgb(16, 19, 24);
        let normal = PackedStyle::new(foreground, background, None, 0, UnderlineStyle::None);
        let bold = PackedStyle::new(
            foreground,
            background,
            None,
            ATTR_BOLD,
            UnderlineStyle::None,
        );
        let mut dictionary = ViewportDictionary::default();
        dictionary.ensure_default(normal, &[RgbColor::default(); 256]);

        dictionary.style_ids.clear();
        assert_eq!(dictionary.intern_style(normal), 0);
        assert_eq!(dictionary.styles.len(), 1);

        assert_eq!(dictionary.intern_style(bold), 1);
        dictionary.style_ids.clear();
        assert_eq!(dictionary.intern_style(bold), 1);
        assert_eq!(dictionary.styles.len(), 2);
    }

    #[test]
    fn palette_changes_reset_the_resolved_style_dictionary() {
        let foreground = Color::rgb(216, 222, 233);
        let background = Color::rgb(16, 19, 24);
        let normal = PackedStyle::new(foreground, background, None, 0, UnderlineStyle::None);
        let indexed = PackedStyle::new(
            Color::rgb(10, 20, 30),
            background,
            None,
            0,
            UnderlineStyle::None,
        );
        let mut palette = [RgbColor::default(); 256];
        let mut dictionary = ViewportDictionary::default();
        dictionary.ensure_default(normal, &palette);
        assert_eq!(dictionary.intern_style(indexed), 1);
        let generation = dictionary.generation;

        palette[42] = RgbColor {
            r: 0xaa,
            g: 0xbb,
            b: 0xcc,
        };
        dictionary.ensure_default(normal, &palette);

        assert_ne!(dictionary.generation, generation);
        assert_eq!(dictionary.styles, [normal]);
        assert_eq!(dictionary.last_style_id, 0);
    }

    #[test]
    fn dictionary_hard_limits_degrade_excess_cells_instead_of_failing() {
        let foreground = Color::rgb(216, 222, 233);
        let background = Color::rgb(16, 19, 24);
        let normal = PackedStyle::new(foreground, background, None, 0, UnderlineStyle::None);
        let excess = PackedStyle::new(
            Color::rgb(1, 2, 3),
            background,
            None,
            ATTR_BOLD,
            UnderlineStyle::None,
        );
        let palette = [RgbColor::default(); 256];
        let mut dictionary = ViewportDictionary::default();
        dictionary.ensure_default(normal, &palette);
        dictionary.styles.resize(MAX_VIEWPORT_STYLE_COUNT, normal);

        assert_eq!(dictionary.intern_style(excess), 0);
        assert!(dictionary.style_overflowed);

        dictionary.reset_live(normal, &palette);
        dictionary
            .grapheme_offsets
            .resize(MAX_VIEWPORT_GRAPHEME_COUNT + 1, 0);
        assert_eq!(dictionary.encode_glyph("e\u{301}"), u32::from('e'));
        assert!(dictionary.grapheme_overflowed);
    }

    #[test]
    fn live_dictionary_compaction_rebuilds_the_visible_working_set() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();

        terminal.vt_write(b"a");
        let first = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("initial snapshot");
        let previous_dictionary_generation = first.dictionary_generation;
        let compaction_limit = dictionary.style_compaction_limit;
        assert!(compaction_limit >= MIN_STYLE_COMPACTION_LIMIT);
        let default_style = dictionary.styles[0];
        dictionary
            .styles
            .resize(compaction_limit.saturating_add(1), default_style);

        terminal.vt_write(b"b");
        let compacted = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("compacted snapshot");

        assert_ne!(
            compacted.dictionary_generation,
            previous_dictionary_generation
        );
        assert!(dictionary.styles.len() < compaction_limit);
        assert_eq!(
            compacted.cell_text(compacted.cell(0, 0).expect("first cell")),
            "a"
        );
        assert_eq!(
            compacted.cell_text(compacted.cell(0, 1).expect("second cell")),
            "b"
        );
        assert!(TerminalViewport::diff(&first, &compacted).is_none());
    }

    #[test]
    fn viewport_dictionary_reuses_a_released_cell_plane() {
        let mut dictionary = ViewportDictionary {
            shared_cells: Arc::from([
                PackedCell::new(u32::from('a'), 0, CellWidth::Narrow),
                PackedCell::EMPTY,
            ]),
            ..ViewportDictionary::default()
        };
        let retained_snapshot = Arc::clone(&dictionary.shared_cells);
        let original_allocation = dictionary.shared_cells.as_ptr();

        let mut next = dictionary.acquire_cell_plane(2, true);
        Arc::get_mut(&mut next).expect("unique next plane")[1] =
            PackedCell::new(u32::from('b'), 0, CellWidth::Narrow);
        dictionary.commit_cell_plane(next);
        assert_eq!(dictionary.cell_pool.len(), 1);

        drop(retained_snapshot);
        let recycled = dictionary.acquire_cell_plane(2, true);
        assert_eq!(recycled.as_ptr(), original_allocation);
        assert_eq!(recycled.as_ref(), dictionary.shared_cells.as_ref());
    }

    #[test]
    fn viewport_dictionary_cell_pool_stays_bounded_for_retained_snapshots() {
        let mut dictionary = ViewportDictionary {
            shared_cells: Arc::from([PackedCell::EMPTY; 2]),
            ..ViewportDictionary::default()
        };
        let mut retained = Vec::new();

        for glyph in b'a'..=b'h' {
            retained.push(Arc::clone(&dictionary.shared_cells));
            let mut next = dictionary.acquire_cell_plane(2, true);
            Arc::get_mut(&mut next).expect("unique next plane")[0] =
                PackedCell::new(u32::from(glyph), 0, CellWidth::Narrow);
            dictionary.commit_cell_plane(next);
            assert!(dictionary.cell_pool.len() <= RETAINED_CELL_PLANES);
        }

        assert_eq!(retained.len(), 8);
        assert_eq!(dictionary.cell_pool.len(), RETAINED_CELL_PLANES);
    }

    #[test]
    fn viewport_dictionary_reuses_a_released_overlay_plane() {
        let first_span = OverlaySpan::new(0, 0, 1, OverlayKind::Selection);
        let mut dictionary = ViewportDictionary {
            shared_overlays: Arc::from([first_span]),
            ..ViewportDictionary::default()
        };
        let retained_snapshot = Arc::clone(&dictionary.shared_overlays);
        let first_allocation = dictionary.shared_overlays.as_ptr();

        let second =
            dictionary.finish_overlays(vec![OverlaySpan::new(0, 1, 2, OverlayKind::Selection)]);
        assert_ne!(second.as_ptr(), first_allocation);

        drop(retained_snapshot);
        let recycled =
            dictionary.finish_overlays(vec![OverlaySpan::new(0, 2, 3, OverlayKind::Selection)]);
        assert_eq!(recycled.as_ptr(), first_allocation);
        assert_eq!(
            recycled.as_ref(),
            [OverlaySpan::new(0, 2, 3, OverlayKind::Selection)]
        );
        assert!(dictionary.overlay_pool.len() <= RETAINED_OVERLAY_PLANES);
    }

    #[test]
    fn incremental_search_reuses_its_control_record() {
        let mut slot = SearchSlot::default();
        store_search_state(
            &mut slot,
            SearchState {
                query: SearchQuery::literal("first"),
                ..SearchState::default()
            },
        );
        let address = std::ptr::from_ref(slot.as_deref().expect("first state"));

        store_search_state(
            &mut slot,
            SearchState {
                query: SearchQuery::literal("second"),
                request_id: 2,
                ..SearchState::default()
            },
        );

        assert_eq!(
            std::ptr::from_ref(slot.as_deref().expect("updated state")),
            address
        );
        assert_eq!(slot.as_deref().expect("updated state").query.text, "second");
    }

    #[test]
    fn pty_geometry_uses_total_pixel_dimensions() {
        let geometry = Geometry {
            columns: 120,
            rows: 40,
            cell_width_px: 9,
            cell_height_px: 20,
        };
        assert_eq!(
            geometry.pty_size(),
            PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 1080,
                pixel_height: 800,
            }
        );
    }

    #[test]
    fn byte_paste_uses_terminal_mode_without_forcing_utf8() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut output = Vec::new();

        write_paste_bytes(&terminal, b"a\nb\xff".to_vec(), false, &mut output)
            .expect("literal paste");
        assert_eq!(output, b"a\rb\xff");

        output.clear();
        write_paste_bytes(&terminal, b"a\nb\xff".to_vec(), true, &mut output)
            .expect("requested paste without terminal mode");
        assert_eq!(output, b"a\rb\xff");

        terminal.vt_write(b"\x1b[?2004h");
        output.clear();
        write_paste_bytes(&terminal, b"a\nb\xff".to_vec(), true, &mut output)
            .expect("bracketed paste");
        assert_eq!(output, b"\x1b[200~a\nb\xff\x1b[201~");
    }

    #[test]
    fn prepared_buffer_paste_preserves_exact_bytes_and_only_adds_requested_brackets() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut output = Vec::new();

        write_prepared_paste_bytes(&terminal, b"a\nb\0\xff", true, &mut output)
            .expect("unbracketed prepared paste");
        assert_eq!(output, b"a\nb\0\xff");

        terminal.vt_write(b"\x1b[?2004h");
        output.clear();
        write_prepared_paste_bytes(&terminal, b"a\rb\0\xff", true, &mut output)
            .expect("bracketed prepared paste");
        assert_eq!(output, b"\x1b[200~a\rb\0\xff\x1b[201~");

        output.clear();
        write_prepared_paste_bytes(&terminal, b"", true, &mut output)
            .expect("empty bracketed paste");
        assert_eq!(output, b"\x1b[200~\x1b[201~");
    }

    #[test]
    fn snapshot_copies_styled_and_wide_cells() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"\x1b]2;fixture\x07\x1b[1;38;2;12;34;56mA\xe7\x95\x8c\x1b[0m");

        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();
        let viewport = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("snapshot");

        assert_eq!(viewport.title(), "fixture");
        assert_eq!(viewport.generation, 1);
        let a = viewport.cell(0, 0).expect("A cell");
        assert_eq!(viewport.cell_text(a), "A");
        assert!(viewport.style(a).expect("A style").bold());
        assert_eq!(
            viewport.style(a).expect("A style").foreground(),
            Color::rgb(12, 34, 56)
        );
        assert!(viewport.style(a).expect("A style").explicit_rgb());
        let wide = viewport.cell(0, 1).expect("wide cell");
        assert_eq!(viewport.cell_text(wide), "界");
        assert_eq!(wide.width(), CellWidth::Wide);
        assert_eq!(
            viewport.cell(0, 2).expect("wide spacer").width(),
            CellWidth::SpacerTail
        );
    }

    #[test]
    fn configured_defaults_and_complete_palette_reach_ghostty() {
        let mut appearance = TerminalAppearance {
            foreground: Color::rgb(0x11, 0x22, 0x33),
            background: Color::rgb(0x04, 0x05, 0x06),
            cursor_color: Color::rgb(0xaa, 0xbb, 0xcc),
            cursor_style: CursorStyle::Underline,
            ..TerminalAppearance::default()
        };
        for index in 0_u16..=255 {
            let index = u8::try_from(index).expect("palette index");
            appearance.palette[usize::from(index)] =
                Color::rgb(index, u8::MAX - index, index.wrapping_mul(37));
        }

        let mut terminal = Terminal::new(TerminalOptions {
            cols: 256,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        apply_terminal_appearance(&mut terminal, &appearance).expect("apply appearance");
        assert_eq!(
            snapshot_fixture(&terminal).cursor.expect("cursor").style(),
            CursorStyle::Underline
        );
        let mut input = b"\x1b[0 q".to_vec();
        for index in 0_u16..=255 {
            input.extend_from_slice(format!("\x1b[38;5;{index}mX").as_bytes());
        }
        terminal.vt_write(&input);

        let viewport = snapshot_fixture(&terminal);
        assert_eq!(viewport.foreground, appearance.foreground);
        assert_eq!(viewport.background, appearance.background);
        assert_eq!(
            viewport.cursor.expect("cursor").color(),
            appearance.cursor_color
        );
        assert_eq!(
            viewport.cursor.expect("cursor").style(),
            CursorStyle::Underline
        );
        for (index, cell) in viewport.row(0).expect("first row").iter().enumerate() {
            assert_eq!(
                viewport.style(*cell).expect("style").foreground(),
                appearance.palette[index],
                "palette entry {index}"
            );
        }

        let mut reloaded = appearance.clone();
        reloaded.background = Color::rgb(0xde, 0xad, 0xbe);
        apply_terminal_appearance(&mut terminal, &reloaded).expect("reapply appearance");
        assert_eq!(
            terminal.bg_color().expect("effective background"),
            Some(ghostty_color(reloaded.background))
        );
    }

    #[test]
    fn decscusr_outranks_the_configured_cursor_style_until_it_resets() {
        let appearance = TerminalAppearance {
            cursor_style: CursorStyle::Underline,
            ..TerminalAppearance::default()
        };
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 4,
            rows: 1,
            max_scrollback: 0,
        })
        .expect("terminal");
        apply_terminal_appearance(&mut terminal, &appearance).expect("apply appearance");

        terminal.vt_write(b"\x1b[5 q");
        assert_eq!(
            snapshot_fixture(&terminal).cursor.expect("cursor").style(),
            CursorStyle::Bar
        );

        let reloaded = TerminalAppearance {
            cursor_style: CursorStyle::Block,
            ..appearance
        };
        apply_terminal_appearance(&mut terminal, &reloaded).expect("reapply appearance");
        assert_eq!(
            snapshot_fixture(&terminal).cursor.expect("cursor").style(),
            CursorStyle::Bar,
            "a reload must not stomp the style the program selected"
        );

        terminal.vt_write(b"\x1b[0 q");
        let cursor = snapshot_fixture(&terminal).cursor.expect("cursor");
        assert_eq!(cursor.style(), CursorStyle::Block);
        assert!(cursor.blinking());
    }

    #[test]
    fn osc_palette_override_takes_precedence_over_configured_palette() {
        let mut appearance = TerminalAppearance::default();
        appearance.palette[42] = Color::rgb(1, 2, 3);
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 2,
            rows: 1,
            max_scrollback: 16,
        })
        .expect("terminal");
        apply_terminal_appearance(&mut terminal, &appearance).expect("apply appearance");
        terminal.vt_write(b"\x1b]4;42;rgb:12/34/56\x1b\\\x1b[38;5;42mX");

        let viewport = snapshot_fixture(&terminal);
        let cell = viewport.row(0).expect("first row")[0];
        assert_eq!(
            viewport.style(cell).expect("style").foreground(),
            Color::rgb(0x12, 0x34, 0x56)
        );
    }

    #[test]
    fn overlay_snapshots_reuse_cells_and_dictionary_tables() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"one");
        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();
        let first = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("first snapshot");
        let grapheme_allocation = dictionary.grapheme_scratch.as_ptr();
        let grapheme_capacity = dictionary.grapheme_scratch.capacity();
        let overlay_only = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::View,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("overlay snapshot");
        assert!(Arc::ptr_eq(
            &first.presentation.title,
            &overlay_only.presentation.title
        ));
        assert!(Arc::ptr_eq(&first.presentation, &overlay_only.presentation));
        assert!(Arc::ptr_eq(&first.cells, &overlay_only.cells));
        assert!(Arc::ptr_eq(&first.overlays, &overlay_only.overlays));
        assert!(Arc::ptr_eq(&first.dictionary, &overlay_only.dictionary));
        assert_eq!(dictionary.grapheme_scratch.as_ptr(), grapheme_allocation);
        assert_eq!(dictionary.grapheme_scratch.capacity(), grapheme_capacity);

        terminal.vt_write(b"two");
        let changed = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("changed snapshot");
        assert!(Arc::ptr_eq(
            &overlay_only.presentation.title,
            &changed.presentation.title
        ));
        assert!(Arc::ptr_eq(
            &overlay_only.presentation,
            &changed.presentation
        ));
        assert!(!Arc::ptr_eq(&overlay_only.cells, &changed.cells));
        assert!(Arc::ptr_eq(&overlay_only.dictionary, &changed.dictionary));
        assert_eq!(dictionary.grapheme_scratch.as_ptr(), grapheme_allocation);
        assert_eq!(dictionary.grapheme_scratch.capacity(), grapheme_capacity);

        terminal.vt_write(b"\x1b]0;renamed\x07");
        let renamed = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("renamed snapshot");
        assert_eq!(renamed.title(), "renamed");
        assert!(!Arc::ptr_eq(
            &changed.presentation.title,
            &renamed.presentation.title
        ));
        assert!(!Arc::ptr_eq(&changed.presentation, &renamed.presentation));
        assert_eq!(dictionary.grapheme_scratch.as_ptr(), grapheme_allocation);
        assert_eq!(dictionary.grapheme_scratch.capacity(), grapheme_capacity);
    }

    #[test]
    fn dirty_snapshots_reuse_a_released_packed_cell_plane() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();

        terminal.vt_write(b"a");
        let first = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("first snapshot");
        let first_allocation = first.cells.as_ptr();

        terminal.vt_write(b"b");
        let second = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("second snapshot");
        assert_ne!(second.cells.as_ptr(), first_allocation);

        drop(first);
        terminal.vt_write(b"c");
        let third = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("third snapshot");
        assert_eq!(third.cells.as_ptr(), first_allocation);
        assert_eq!(third.cell_text(third.cell(0, 0).expect("a")), "a");
        assert_eq!(third.cell_text(third.cell(0, 1).expect("b")), "b");
        assert_eq!(third.cell_text(third.cell(0, 2).expect("c")), "c");
    }

    #[test]
    fn dictionary_tables_publish_only_the_changed_plane() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write("e\u{301}".as_bytes());
        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();
        let initial = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("initial dictionary snapshot");
        assert!(initial.grapheme_offsets().len() > 1);

        terminal.vt_write(b"\x1b[31mX");
        let styled = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("style-only dictionary append");
        assert!(!Arc::ptr_eq(&initial.dictionary, &styled.dictionary));
        assert!(!Arc::ptr_eq(
            &initial.dictionary.styles,
            &styled.dictionary.styles
        ));
        assert!(Arc::ptr_eq(
            &initial.dictionary.grapheme_offsets,
            &styled.dictionary.grapheme_offsets
        ));
        assert!(Arc::ptr_eq(
            &initial.dictionary.grapheme_bytes,
            &styled.dictionary.grapheme_bytes
        ));

        terminal.vt_write("o\u{302}".as_bytes());
        let grapheme = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("grapheme-only dictionary append");
        assert!(!Arc::ptr_eq(&styled.dictionary, &grapheme.dictionary));
        assert!(Arc::ptr_eq(
            &styled.dictionary.styles,
            &grapheme.dictionary.styles
        ));
        assert!(!Arc::ptr_eq(
            &styled.dictionary.grapheme_offsets,
            &grapheme.dictionary.grapheme_offsets
        ));
        assert!(!Arc::ptr_eq(
            &styled.dictionary.grapheme_bytes,
            &grapheme.dictionary.grapheme_bytes
        ));
    }

    #[test]
    fn single_click_stays_hidden_until_selection_crosses_a_cell() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"hello");
        let mut selection = None;
        let pointer = |column| PointerCellEvent {
            column,
            row: 0,
            click_count: 1,
            rectangle: false,
        };
        selection_press(
            &terminal,
            &mut selection,
            pointer(1),
            &WordSeparators::default(),
        )
        .expect("anchor selection");
        assert_eq!(format_selection_text(&terminal).expect("copy"), "");

        let mut render_state = RenderState::new().expect("render state");
        let mut rows = RowIterator::new().expect("rows");
        let mut cells = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();
        let viewport = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("snapshot");
        assert!(viewport.overlays.is_empty());

        selection_drag(
            &terminal,
            &mut selection,
            pointer(1),
            &WordSeparators::default(),
        )
        .expect("drag inside anchor cell");
        assert_eq!(format_selection_text(&terminal).expect("copy"), "");

        selection_drag(
            &terminal,
            &mut selection,
            pointer(3),
            &WordSeparators::default(),
        )
        .expect("extend selection");
        assert_eq!(format_selection_text(&terminal).expect("copy"), "ell");
        let moved = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::Overlay,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("selection-only snapshot");
        assert!(Arc::ptr_eq(&viewport.cells, &moved.cells));
        assert_eq!(moved.generation, viewport.generation);
        assert_eq!(moved.view_generation, viewport.view_generation + 1);
        assert_eq!(
            moved.overlays.as_ref(),
            [OverlaySpan::new(0, 1, 4, OverlayKind::Selection)]
        );
        assert!(dictionary.cell_pool.is_empty());

        let scratch_allocation = dictionary.overlay_scratch.as_ptr();
        let scratch_capacity = dictionary.overlay_scratch.capacity();
        let unchanged = snapshot(
            &terminal,
            &mut render_state,
            &mut rows,
            &mut cells,
            &mut generations,
            SnapshotChange::View,
            &mut dictionary,
            None,
            SessionStatus::Running,
        )
        .expect("unchanged overlay snapshot");
        assert!(Arc::ptr_eq(&moved.overlays, &unchanged.overlays));
        assert!(Arc::ptr_eq(&moved.cells, &unchanged.cells));
        assert_eq!(dictionary.overlay_scratch.as_ptr(), scratch_allocation);
        assert_eq!(dictionary.overlay_scratch.capacity(), scratch_capacity);
    }

    #[test]
    fn out_of_range_pointer_selection_clamps_and_pty_free_worker_survives() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 3,
            rows: 2,
            max_scrollback: 4,
        })
        .expect("terminal");
        terminal.vt_write(b"abc\r\ndef");
        let event = PointerCellEvent {
            column: u16::MAX,
            row: u16::MAX,
            click_count: 1,
            rectangle: false,
        };
        let Point::Viewport(point) = viewport_point(&terminal, event).expect("clamped point")
        else {
            panic!("pointer selection must remain viewport-relative");
        };
        assert_eq!(point, PointCoordinate { x: 2, y: 1 });
        let mut selection = None;
        selection_press(&terminal, &mut selection, event, &WordSeparators::default())
            .expect("clamped selection");
        assert!(selection.is_some());

        let session =
            TerminalSession::spawn_output_view("selection".to_owned(), "abcdef".to_owned());
        let view = TerminalViewId(78);
        session.resize(3, 2, 8, 18);
        session.attach_view(view);
        let before = wait_for_test_viewport(&session, |viewport| {
            viewport.columns == 3
                && viewport.rows == 2
                && matches!(viewport.status, SessionStatus::Running)
        });
        session.view_action(view, TerminalViewAction::SelectionPress(event));
        let selected = wait_for_test_viewport(&session, |viewport| {
            viewport.view_generation > before.view_generation
        });
        assert!(matches!(selected.status, SessionStatus::Running));
    }

    #[test]
    fn out_of_space_selection_reinstall_clears_pending_selection_without_error() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 3,
            rows: 2,
            max_scrollback: 4,
        })
        .expect("terminal");
        terminal.vt_write(b"abc");
        let mut view = TerminalViewState::for_screen(Screen::Primary);
        selection_press(
            &terminal,
            &mut view.selection,
            PointerCellEvent {
                column: 0,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            &WordSeparators::default(),
        )
        .expect("selection fixture");
        assert!(view.selection.is_some());

        finish_view_selection_install(
            &terminal,
            &mut view,
            Err(WorkerError::Ghostty(libghostty_vt::Error::OutOfSpace {
                required: 1,
            })),
        )
        .expect("selection reinstall failure is nonfatal");
        assert!(view.selection.is_none());
    }

    #[test]
    fn desktop_word_selection_uses_the_compiled_tmux_boundaries() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"foo.bar baz");
        let mut selection = None;
        let event = PointerCellEvent {
            column: 1,
            row: 0,
            click_count: 2,
            rectangle: false,
        };

        selection_press(&terminal, &mut selection, event, &WordSeparators::default())
            .expect("default word selection");
        assert_eq!(
            format_selection_text(&terminal).expect("default copy"),
            "foo"
        );

        selection = None;
        terminal.set_selection(None).expect("clear selection");
        selection_press(&terminal, &mut selection, event, &WordSeparators::new(""))
            .expect("whitespace-only word selection");
        assert_eq!(
            format_selection_text(&terminal).expect("whitespace-only copy"),
            "foo.bar"
        );

        let mut adjacent = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("adjacent terminal");
        adjacent.vt_write(b"foo. bar");
        let mut adjacent_selection = None;
        selection_press(
            &adjacent,
            &mut adjacent_selection,
            PointerCellEvent { column: 3, ..event },
            &WordSeparators::default(),
        )
        .expect("separator selection");
        assert_eq!(
            format_selection_text(&adjacent).expect("separator copy"),
            "."
        );
        selection_press(
            &adjacent,
            &mut adjacent_selection,
            PointerCellEvent { column: 4, ..event },
            &WordSeparators::default(),
        )
        .expect("whitespace selection");
        assert_eq!(
            format_selection_text(&adjacent).expect("space copy"),
            "",
            "libghostty intentionally does not select an unstyled empty-space cell"
        );
    }

    #[test]
    fn client_views_restore_isolated_viewport_and_selection_state() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"zero\r\none\r\ntwo\r\nthree");

        let first_id = TerminalViewId(1);
        let second_id = TerminalViewId(2);
        let word_separators = WordSeparators::default();
        let mut active = ActiveTerminalViews::new();
        let mut inactive = InactiveTerminalViews::new();
        activate_view(
            &mut terminal,
            first_id,
            &mut active,
            &mut inactive,
            &word_separators,
        )
        .expect("activate first view");
        terminal.scroll_viewport(ScrollViewport::Top);
        let first = active.get_mut(&first_id).expect("first active");
        selection_press(
            &terminal,
            &mut first.selection,
            PointerCellEvent {
                column: 0,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            &word_separators,
        )
        .expect("select in first view");
        selection_drag(
            &terminal,
            &mut first.selection,
            PointerCellEvent {
                column: 1,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            &word_separators,
        )
        .expect("extend selection in first view");
        first.unseen_output = 7;
        sync_viewport_anchor(&terminal, first).expect("pin first viewport");
        let first_offset = terminal.scrollbar().expect("scrollbar").offset;
        let first_address = std::ptr::from_ref(active[&first_id].as_ref());
        assert_eq!(
            format_selection_text(&terminal).expect("first selection"),
            "ze"
        );

        activate_view(
            &mut terminal,
            second_id,
            &mut active,
            &mut inactive,
            &word_separators,
        )
        .expect("activate second view");
        assert_eq!(active.len(), 2, "activating a view must not park its peers");
        let second_scrollbar = terminal.scrollbar().expect("second scrollbar");
        assert_eq!(
            second_scrollbar.offset.saturating_add(second_scrollbar.len),
            second_scrollbar.total
        );
        assert_eq!(
            format_selection_text(&terminal).expect("second selection"),
            ""
        );

        restore_view_state(
            &mut terminal,
            active.get_mut(&first_id).expect("first remains active"),
            &word_separators,
        )
        .expect("restore first view");
        assert_eq!(
            terminal.scrollbar().expect("restored scrollbar").offset,
            first_offset
        );
        assert_eq!(
            format_selection_text(&terminal).expect("restored selection"),
            "ze"
        );
        assert_eq!(active[&first_id].unseen_output, 7);
        assert_eq!(
            std::ptr::from_ref(active[&first_id].as_ref()),
            first_address,
            "plural activation must preserve the boxed view state"
        );

        deactivate_view(
            &mut terminal,
            second_id,
            &mut active,
            &mut inactive,
            &word_separators,
        )
        .expect("park second view");
        release_view(&mut terminal, first_id, &mut active, &mut inactive)
            .expect("release first view");
        assert!(active.is_empty());
        assert!(!inactive.contains_key(&first_id));
        assert!(inactive.contains_key(&second_id));
    }

    #[test]
    fn primary_and_alternate_screens_keep_independent_native_view_state() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"zero\r\none\r\ntwo\r\nthree");
        let mut view = TerminalViewState::for_screen(Screen::Primary);
        let word_separators = WordSeparators::default();
        terminal.scroll_viewport(ScrollViewport::Top);
        selection_press(
            &terminal,
            &mut view.selection,
            PointerCellEvent {
                column: 0,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            &word_separators,
        )
        .expect("primary selection");
        selection_drag(
            &terminal,
            &mut view.selection,
            PointerCellEvent {
                column: 1,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            &word_separators,
        )
        .expect("extend primary selection");
        store_search_state(
            &mut view.search,
            SearchState {
                query: SearchQuery::literal("zero"),
                ..SearchState::default()
            },
        );
        sync_viewport_anchor(&terminal, &mut view).expect("primary anchor");
        view.unseen_output = 5;
        let primary_offset = terminal.scrollbar().expect("primary scrollbar").offset;

        terminal.vt_write(b"\x1b[?1049hALT");
        view.note_output(terminal.active_screen().expect("alternate screen"));
        assert!(
            reconcile_view_screen(&mut terminal, &mut view, &word_separators)
                .expect("enter alternate")
        );
        assert_eq!(view.screen, Screen::Alternate);
        assert!(view.selection.is_none());
        assert!(view.search.is_none());
        assert_eq!(
            format_selection_text(&terminal).expect("alternate selection"),
            ""
        );

        terminal.vt_write(b"\x1b[?1049l");
        assert!(
            reconcile_view_screen(&mut terminal, &mut view, &word_separators)
                .expect("restore primary")
        );
        assert_eq!(view.screen, Screen::Primary);
        assert_eq!(
            terminal.scrollbar().expect("restored scrollbar").offset,
            primary_offset
        );
        assert_eq!(
            format_selection_text(&terminal).expect("primary restored"),
            "ze"
        );
        assert_eq!(
            view.search.as_ref().expect("primary search").query.text,
            "zero"
        );
        assert_eq!(view.unseen_output, 5);
    }

    #[test]
    fn select_all_covers_retained_history_without_trailing_blank_rows() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"old\r\nmid\r\nnew");
        let mut selection = None;
        select_all_history(&terminal, &mut selection).expect("select all");
        assert_eq!(
            format_selection_text(&terminal).expect("copy all"),
            "old\nmid\nnew"
        );
    }

    #[test]
    fn xterm_history_erase_keeps_visible_content() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"old\r\nvisible\r\nlast");
        assert!(terminal.scrollback_rows().expect("history") > 0);
        terminal.vt_write(b"\x1b[3J");
        assert_eq!(terminal.scrollback_rows().expect("cleared history"), 0);
        assert_eq!(
            capture_terminal(&terminal, None, CaptureOptions::default()).expect("visible"),
            "visible\nlast"
        );
    }

    #[test]
    fn osc8_hover_uses_terminal_link_metadata_and_contiguous_span() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 12,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"\x1b]8;;https://example.com/docs\x1b\\link\x1b]8;;\x1b\\ plain");
        let mut scratch = Vec::with_capacity(LINK_URI_SCRATCH_BYTES);
        let allocation = scratch.as_ptr();
        let capacity = scratch.capacity();

        let link = hover_link_at(
            &terminal,
            PointerCellEvent {
                column: 2,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            &mut scratch,
            &HashSet::new(),
        )
        .expect("hover lookup")
        .expect("OSC 8 link");
        assert_eq!(link.uri, "https://example.com/docs");
        assert_eq!((link.row, link.start, link.end), (0, 0, 4));
        assert_eq!(
            hover_link_at(
                &terminal,
                PointerCellEvent {
                    column: 5,
                    row: 0,
                    click_count: 1,
                    rectangle: false,
                },
                &mut scratch,
                &HashSet::new(),
            )
            .expect("plain cell lookup"),
            None
        );
        assert_eq!(scratch.as_ptr(), allocation);
        assert_eq!(scratch.capacity(), capacity);
        let viewport = snapshot_fixture(&terminal);
        assert!(
            viewport
                .style(viewport.cell(0, 0).expect("link cell"))
                .expect("link style")
                .hyperlink()
        );
    }

    #[test]
    fn osc8_hover_rejects_active_content_schemes() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"\x1b]8;;javascript:alert(1)\x1b\\bad\x1b]8;;\x1b\\");
        let mut scratch = Vec::with_capacity(LINK_URI_SCRATCH_BYTES);

        assert_eq!(
            hover_link_at(
                &terminal,
                PointerCellEvent {
                    column: 1,
                    row: 0,
                    click_count: 1,
                    rectangle: false,
                },
                &mut scratch,
                &HashSet::new(),
            )
            .expect("unsafe lookup"),
            None
        );
    }

    #[test]
    fn plain_uri_hover_trims_wrappers_and_trailing_punctuation() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 1,
            max_scrollback: 16,
        })
        .expect("terminal");
        let uri = "https://example.com/docs";
        terminal.vt_write(format!("({uri}), after").as_bytes());
        let mut scratch = Vec::new();

        let link = hover_link_at(
            &terminal,
            PointerCellEvent {
                column: 10,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            &mut scratch,
            &HashSet::new(),
        )
        .expect("plain URI lookup")
        .expect("plain URI");
        assert_eq!(link.uri, uri);
        assert_eq!(link.start, 1);
        assert_eq!(usize::from(link.end), 1 + uri.len());
    }

    #[test]
    fn image_placeholder_hover_spans_the_bracketed_text_and_carries_the_number() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 1,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"read [Image #12] now");
        let mut scratch = Vec::new();
        let bound = HashSet::from([12]);

        let link = hover_link_at(
            &terminal,
            PointerCellEvent {
                column: 11,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            &mut scratch,
            &bound,
        )
        .expect("placeholder lookup")
        .expect("placeholder");
        assert_eq!(link.uri, "zz-image://12");
        assert_eq!((link.row, link.start, link.end), (0, 5, 16));

        for column in [4, 16] {
            assert_eq!(
                hover_link_at(
                    &terminal,
                    PointerCellEvent {
                        column,
                        row: 0,
                        click_count: 1,
                        rectangle: false,
                    },
                    &mut scratch,
                    &bound,
                )
                .expect("outside lookup"),
                None,
                "column {column} sits outside the placeholder"
            );
        }
    }

    #[test]
    fn image_placeholder_hover_ignores_other_bracketed_text() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"[Pasted text #1]\r\n[Image #]");
        let mut scratch = Vec::new();

        for row in [0, 1] {
            assert_eq!(
                hover_link_at(
                    &terminal,
                    PointerCellEvent {
                        column: 3,
                        row,
                        click_count: 1,
                        rectangle: false,
                    },
                    &mut scratch,
                    &HashSet::new(),
                )
                .expect("non-placeholder lookup"),
                None,
                "row {row} is not an image placeholder"
            );
        }
    }

    #[test]
    fn pending_paste_baseline_ignores_existing_placeholder_occurrences() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 3,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"[Image #1]");
        let now = Instant::now();
        let mut bindings = PastedImageBindings::default();

        bindings.open(&terminal, 7, now).expect("open window");

        assert_eq!(bindings.observe(&terminal).expect("observe baseline"), None);
        assert!(bindings.bound_numbers().is_empty());
    }

    #[test]
    fn pending_paste_binds_the_highest_new_placeholder_number() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 3,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut bindings = PastedImageBindings::default();
        bindings
            .open(&terminal, 8, Instant::now())
            .expect("open window");

        terminal.vt_write(b"[Image #2] [Image #5]");

        assert_eq!(bindings.observe(&terminal).expect("observe"), Some((8, 5)));
        assert_eq!(bindings.bound_numbers(), &HashSet::from([5]));
    }

    #[test]
    fn pending_paste_rebinds_when_an_existing_number_count_increases() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 3,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"[Image #1]");
        let mut bindings = PastedImageBindings::default();
        bindings
            .open(&terminal, 9, Instant::now())
            .expect("open window");

        terminal.vt_write(b"\r\n[Image #1]");

        assert_eq!(bindings.observe(&terminal).expect("observe"), Some((9, 1)));
    }

    #[test]
    fn pending_paste_counts_the_live_grid_even_when_a_viewport_is_scrolled_back() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"old\r\nlive\r\n[Image #7]");
        terminal.scroll_viewport(ScrollViewport::Top);
        let mut bindings = PastedImageBindings::default();
        bindings
            .open(&terminal, 70, Instant::now())
            .expect("open window");

        terminal.vt_write(b"\r\n[Image #7]");

        assert_eq!(bindings.observe(&terminal).expect("observe"), Some((70, 7)));
    }

    #[test]
    fn placeholder_output_outside_a_pending_window_never_binds() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"response echoes [Image #2]");
        let mut bindings = PastedImageBindings::default();

        assert_eq!(bindings.observe(&terminal).expect("observe"), None);
        assert!(bindings.bound_numbers().is_empty());
    }

    #[test]
    fn pending_paste_expiry_returns_its_token_without_binding() {
        let terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let now = Instant::now();
        let mut bindings = PastedImageBindings::default();
        bindings.open(&terminal, 10, now).expect("open window");
        let deadline = now + PENDING_PASTE_WINDOW;

        assert!(
            bindings
                .expire(
                    deadline
                        .checked_sub(Duration::from_millis(1))
                        .expect("one millisecond before the deadline"),
                )
                .is_empty()
        );
        assert_eq!(bindings.expire(deadline), [10]);
        assert!(bindings.bound_numbers().is_empty());
    }

    #[test]
    fn unbinding_a_pasted_image_removes_its_link() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut bindings = PastedImageBindings::default();
        bindings
            .open(&terminal, 11, Instant::now())
            .expect("open window");
        terminal.vt_write(b"[Image #3]");
        assert_eq!(bindings.observe(&terminal).expect("observe"), Some((11, 3)));
        let point = PointerCellEvent {
            column: 4,
            row: 0,
            click_count: 1,
            rectangle: false,
        };
        assert!(
            image_placeholder_at(&terminal, point, 40, bindings.bound_numbers())
                .expect("bound lookup")
                .is_some()
        );

        assert!(bindings.unbind(3));

        assert_eq!(
            image_placeholder_at(&terminal, point, 40, bindings.bound_numbers())
                .expect("unbound lookup"),
            None
        );
    }

    #[test]
    fn plain_uri_hover_rejects_unsafe_and_overlong_tokens() {
        let mut unsafe_terminal = Terminal::new(TerminalOptions {
            cols: 32,
            rows: 1,
            max_scrollback: 16,
        })
        .expect("terminal");
        unsafe_terminal.vt_write(b"javascript:alert(1)");
        let mut scratch = Vec::new();
        let point = PointerCellEvent {
            column: 5,
            row: 0,
            click_count: 1,
            rectangle: false,
        };
        assert_eq!(
            hover_link_at(&unsafe_terminal, point, &mut scratch, &HashSet::new())
                .expect("unsafe lookup"),
            None
        );

        let columns = u16::try_from(MAX_LINK_URI_BYTES + 2).expect("bounded terminal width");
        let mut long_terminal = Terminal::new(TerminalOptions {
            cols: columns,
            rows: 1,
            max_scrollback: 16,
        })
        .expect("terminal");
        long_terminal.vt_write(format!("https://{}", "a".repeat(MAX_LINK_URI_BYTES)).as_bytes());
        assert_eq!(
            hover_link_at(&long_terminal, point, &mut scratch, &HashSet::new())
                .expect("long lookup"),
            None
        );
        assert!(scratch.len() <= MAX_LINK_URI_BYTES);
    }

    #[test]
    fn capture_reads_visible_rows_and_full_canonical_history() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"old-0\r\nold-1\r\nview-0\r\nview-1");

        let visible =
            capture_terminal(&terminal, None, CaptureOptions::default()).expect("visible capture");
        assert_eq!(visible, "view-0\nview-1");

        let full = capture_terminal(
            &terminal,
            None,
            CaptureOptions {
                start: CaptureBoundary::HistoryStart,
                ..CaptureOptions::default()
            },
        )
        .expect("history capture");
        assert_eq!(full, "old-0\nold-1\nview-0\nview-1");
    }

    #[test]
    fn capture_can_join_soft_wrapped_rows() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 4,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"abcdefgh");

        let output = capture_terminal(
            &terminal,
            None,
            CaptureOptions {
                join_wrapped: true,
                ..CaptureOptions::default()
            },
        )
        .expect("joined capture");
        assert_eq!(output, "abcdefgh");
    }

    #[test]
    fn history_search_maps_matches_back_to_cells() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 12,
            rows: 2,
            max_scrollback: 32,
        })
        .expect("terminal");
        terminal.vt_write(b"alpha\r\nbeta target\r\ngamma\r\n");
        let search = search_history(&terminal, "target").expect("search");
        assert_eq!(search.matches.len(), 1);
        assert_eq!(search.matches[0].start, 5);
        assert_eq!(search.matches[0].end, 11);
        assert_eq!(std::mem::size_of::<SearchCellOffset>(), 12);
        assert_eq!(std::mem::align_of::<SearchCellOffset>(), 4);
        assert_eq!(std::mem::size_of::<HistorySearchRow>(), 16);
        assert_eq!(std::mem::align_of::<HistorySearchRow>(), 4);
    }

    #[test]
    fn history_snapshot_reuses_oversized_grapheme_scratch_across_rows() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 4,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let cluster = format!("a{}", "\u{301}".repeat(12));
        terminal.vt_write(format!("{cluster}\r\n{cluster}").as_bytes());

        let mut text = String::new();
        let mut offsets = Vec::new();
        let mut grapheme_scratch = Vec::new();
        append_history_row(
            &terminal,
            0,
            4,
            &mut text,
            &mut offsets,
            &mut grapheme_scratch,
        )
        .expect("first history row");
        assert!(grapheme_scratch.len() > 8);
        let allocation = grapheme_scratch.as_ptr();
        let capacity = grapheme_scratch.capacity();

        append_history_row(
            &terminal,
            1,
            4,
            &mut text,
            &mut offsets,
            &mut grapheme_scratch,
        )
        .expect("second history row");
        assert_eq!(grapheme_scratch.as_ptr(), allocation);
        assert_eq!(grapheme_scratch.capacity(), capacity);
        assert_eq!(text.matches('a').count(), 2);
    }

    #[test]
    fn history_search_supports_regex_case_modes_and_cancellation() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 3,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"Alpha 123\r\nalpha 456\r\nomega");
        let snapshot = HistorySearchSnapshot::capture(&terminal).expect("search snapshot");
        let mut query = SearchQuery {
            text: r"alpha \d+".to_owned(),
            mode: SearchMode::Regex,
            case: SearchCase::Smart,
            direction: SearchDirection::Forward,
        };
        let smart = snapshot
            .search(&query, 1, || false)
            .expect("smart-case search");
        assert_eq!(smart.matches.len(), 2);

        query.case = SearchCase::Sensitive;
        let sensitive = snapshot
            .search(&query, 2, || false)
            .expect("case-sensitive search");
        assert_eq!(sensitive.matches.len(), 1);
        assert_eq!(sensitive.matches[0].row, 1);

        query.text = "(".to_owned();
        let invalid = snapshot
            .search(&query, 3, || false)
            .expect("invalid regex result");
        assert!(invalid.invalid_pattern);
        assert!(invalid.matches.is_empty());
        assert!(snapshot.search(&query, 4, || true).is_none());
    }

    #[test]
    fn incremental_history_search_reuses_match_storage() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"target target\r\ntarget");
        let snapshot = HistorySearchSnapshot::capture(&terminal).expect("snapshot");
        let query = SearchQuery::literal("target");
        let mut scratch = Vec::with_capacity(16);
        let allocation = scratch.as_ptr();
        let capacity = scratch.capacity();

        let first = snapshot
            .search_reusing(&query, 1, &mut scratch, || false)
            .expect("first result");
        assert_eq!(first.matches.len(), 3);
        assert_eq!(first.matches.as_ptr(), allocation);
        assert_eq!(first.matches.capacity(), capacity);

        scratch = first.matches;
        let second = snapshot
            .search_reusing(&query, 2, &mut scratch, || false)
            .expect("second result");
        assert_eq!(second.matches.as_ptr(), allocation);
        assert_eq!(second.matches.capacity(), capacity);

        scratch = second.matches;
        assert!(
            snapshot
                .search_reusing(&query, 3, &mut scratch, || true)
                .is_none()
        );
        assert_eq!(scratch.as_ptr(), allocation);
        assert_eq!(scratch.capacity(), capacity);
    }

    #[test]
    fn completed_search_replaces_stale_pending_result_and_recycles_its_storage() {
        let (results, result_rx) = crossbeam_channel::bounded(1);
        let discard_results = result_rx.clone();
        let view = TerminalViewId(1);
        let mut stale_matches = Vec::with_capacity(16);
        stale_matches.push(SearchMatch {
            row: 1,
            start: 2,
            end: 3,
        });
        let stale_allocation = stale_matches.as_ptr();
        let stale_capacity = stale_matches.capacity();
        let mut pending = SearchResults::default();
        pending.by_view.insert(
            view,
            SearchResult {
                request_id: 1,
                view_id: view,
                screen: Screen::Primary,
                state: SearchState {
                    query: SearchQuery::literal("stale"),
                    matches: stale_matches,
                    request_id: 1,
                    ..SearchState::default()
                },
            },
        );
        results.try_send(pending).expect("initial pending result");

        let latest_match = SearchMatch {
            row: 4,
            start: 5,
            end: 6,
        };
        let mut latest = SearchResults::default();
        latest.by_view.insert(
            view,
            SearchResult {
                request_id: 2,
                view_id: view,
                screen: Screen::Primary,
                state: SearchState {
                    query: SearchQuery::literal("latest"),
                    matches: vec![latest_match],
                    request_id: 2,
                    ..SearchState::default()
                },
            },
        );
        let mut match_scratch = Vec::new();
        assert!(send_latest_search_results(
            &results,
            &discard_results,
            latest,
            &mut match_scratch,
        ));

        let mut received = result_rx.try_recv().expect("latest pending result");
        let received = received.by_view.remove(&view).expect("view result");
        assert_eq!(received.request_id, 2);
        assert_eq!(received.state.matches, [latest_match]);
        assert_eq!(match_scratch.as_ptr(), stale_allocation);
        assert_eq!(match_scratch.capacity(), stale_capacity);
    }

    #[test]
    fn completed_search_coalescing_preserves_results_for_other_views() {
        let (results, result_rx) = crossbeam_channel::bounded(1);
        let discard_results = result_rx.clone();
        let first = TerminalViewId(1);
        let second = TerminalViewId(2);
        let result = |view_id, request_id| SearchResult {
            request_id,
            view_id,
            screen: Screen::Primary,
            state: SearchState {
                query: SearchQuery::literal(format!("view-{}", view_id.0)),
                request_id,
                ..SearchState::default()
            },
        };
        let mut pending = SearchResults::default();
        pending.by_view.insert(first, result(first, 1));
        results.try_send(pending).expect("first view result");

        let mut latest = SearchResults::default();
        latest.by_view.insert(second, result(second, 2));
        assert!(send_latest_search_results(
            &results,
            &discard_results,
            latest,
            &mut Vec::new(),
        ));

        let received = result_rx.try_recv().expect("coalesced view results");
        assert_eq!(received.by_view.len(), 2);
        assert_eq!(received.by_view[&first].request_id, 1);
        assert_eq!(received.by_view[&second].request_id, 2);
    }

    #[test]
    fn cancelling_queued_search_retains_its_match_storage() {
        let (jobs, job_rx) = crossbeam_channel::bounded::<SearchJobs>(1);
        let mut worker = SearchWorker {
            jobs,
            discard_jobs: job_rx,
            latest_requests: HashMap::new(),
            next_request: 0,
            match_scratch: Vec::new(),
        };
        let matches = Vec::with_capacity(16);
        let allocation = matches.as_ptr();
        let capacity = matches.capacity();
        let view = TerminalViewId(1);
        let (request_id, latest_request) = worker.next_request(view);
        worker.submit(SearchJob {
            request_id,
            view_id: view,
            screen: Screen::Primary,
            query: SearchQuery::literal("queued"),
            snapshot: Arc::new(HistorySearchSnapshot {
                columns: 1,
                text: String::new(),
                rows: Vec::new(),
                offsets: Vec::new(),
            }),
            selection: SearchSelectionPolicy::Last,
            match_scratch: matches,
            latest_request,
        });

        assert!(worker.cancel(view) > request_id);
        assert_eq!(worker.match_scratch.as_ptr(), allocation);
        assert_eq!(worker.match_scratch.capacity(), capacity);
    }

    #[test]
    fn search_match_mapping_uses_ordered_utf8_cell_offsets() {
        let offsets = [
            SearchCellOffset {
                start: 0,
                end: 1,
                column: 0,
                width: 1,
            },
            SearchCellOffset {
                start: 1,
                end: 4,
                column: 1,
                width: 2,
            },
            SearchCellOffset {
                start: 4,
                end: 5,
                column: 3,
                width: 1,
            },
        ];

        assert_eq!(search_match_span(&offsets, 0, 5, 4), Some((0, 4)));
        assert_eq!(search_match_span(&offsets, 2, 3, 4), Some((1, 3)));
        assert_eq!(search_match_span(&offsets, 4, 5, 4), Some((3, 4)));
        assert_eq!(search_match_span(&offsets, 1, 1, 4), None);
        assert_eq!(search_match_span(&offsets, 5, 6, 4), None);
    }

    #[test]
    fn copy_mode_search_selects_relative_to_cursor_and_wraps() {
        let matches = vec![
            SearchMatch {
                row: 0,
                start: 1,
                end: 4,
            },
            SearchMatch {
                row: 1,
                start: 2,
                end: 5,
            },
            SearchMatch {
                row: 2,
                start: 3,
                end: 6,
            },
        ];
        let mut search = SearchState {
            matches: matches.clone(),
            ..SearchState::default()
        };

        select_search_result(
            &mut search,
            SearchSelectionPolicy::From {
                point: PointCoordinate { x: 1, y: 0 },
                direction: SearchDirection::Forward,
            },
        );
        assert_eq!(search.current, Some(1));

        search.current = None;
        select_search_result(
            &mut search,
            SearchSelectionPolicy::From {
                point: PointCoordinate { x: 1, y: 0 },
                direction: SearchDirection::Backward,
            },
        );
        assert_eq!(search.current, Some(2));

        search.current = None;
        select_search_result(
            &mut search,
            SearchSelectionPolicy::From {
                point: PointCoordinate {
                    x: u16::MAX,
                    y: u32::MAX,
                },
                direction: SearchDirection::Forward,
            },
        );
        assert_eq!(search.current, Some(0));

        search.matches = matches;
        search.current = None;
        select_search_result(
            &mut search,
            SearchSelectionPolicy::From {
                point: PointCoordinate { x: 0, y: 0 },
                direction: SearchDirection::Backward,
            },
        );
        assert_eq!(search.current, Some(2));
    }

    #[test]
    fn stale_background_search_results_cannot_mutate_a_newer_view_request() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 12,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"target\r\nother");
        let snapshot = HistorySearchSnapshot::capture(&terminal).expect("snapshot");
        let query = SearchQuery::literal("target");
        let view_id = TerminalViewId(9);
        let mut view = TerminalViewState::for_screen(Screen::Primary);
        store_search_state(
            &mut view.search,
            SearchState {
                query: query.clone(),
                request_id: 2,
                pending: true,
                ..SearchState::default()
            },
        );
        let mut active = ActiveTerminalViews::from([(view_id, Box::new(view))]);
        let mut inactive = InactiveTerminalViews::new();
        let (jobs, job_rx) = crossbeam_channel::bounded::<SearchJobs>(1);
        let mut worker = SearchWorker {
            jobs,
            discard_jobs: job_rx,
            latest_requests: HashMap::from([(view_id, Arc::new(AtomicU64::new(2)))]),
            next_request: 2,
            match_scratch: Vec::new(),
        };

        let stale = snapshot.search(&query, 1, || false).expect("stale result");
        let stale_allocation = stale.matches.as_ptr();
        let stale_capacity = stale.matches.capacity();
        assert!(
            !apply_search_result(
                &mut terminal,
                &mut active,
                &mut inactive,
                &mut worker,
                SearchResult {
                    request_id: 1,
                    view_id,
                    screen: Screen::Primary,
                    state: stale,
                },
            )
            .expect("reject stale")
        );
        let pending = active
            .get(&view_id)
            .expect("active")
            .search
            .as_ref()
            .expect("pending search");
        assert_eq!(pending.request_id, 2);
        assert!(pending.matches.is_empty());
        assert_eq!(worker.match_scratch.as_ptr(), stale_allocation);
        assert_eq!(worker.match_scratch.capacity(), stale_capacity);

        let mut fresh = snapshot.search(&query, 2, || false).expect("fresh result");
        select_search_result(&mut fresh, SearchSelectionPolicy::Last);
        assert!(
            apply_search_result(
                &mut terminal,
                &mut active,
                &mut inactive,
                &mut worker,
                SearchResult {
                    request_id: 2,
                    view_id,
                    screen: Screen::Primary,
                    state: fresh,
                },
            )
            .expect("accept fresh")
        );
        let search = active
            .get(&view_id)
            .expect("active")
            .search
            .as_ref()
            .expect("completed search");
        assert!(!search.pending);
        assert_eq!(search.matches.len(), 1);
        assert_eq!(worker.match_scratch.as_ptr(), stale_allocation);
        assert_eq!(worker.match_scratch.capacity(), stale_capacity);
    }

    #[test]
    fn current_search_result_for_a_detached_view_recycles_its_match_storage() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 24,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut active = ActiveTerminalViews::new();
        let mut inactive = InactiveTerminalViews::new();
        let view_id = TerminalViewId(99);
        let (jobs, job_rx) = crossbeam_channel::bounded::<SearchJobs>(1);
        let mut worker = SearchWorker {
            jobs,
            discard_jobs: job_rx,
            latest_requests: HashMap::from([(view_id, Arc::new(AtomicU64::new(1)))]),
            next_request: 1,
            match_scratch: Vec::new(),
        };
        let mut matches = Vec::with_capacity(16);
        matches.push(SearchMatch {
            row: 0,
            start: 0,
            end: 1,
        });
        let allocation = matches.as_ptr();
        let capacity = matches.capacity();

        assert!(
            !apply_search_result(
                &mut terminal,
                &mut active,
                &mut inactive,
                &mut worker,
                SearchResult {
                    request_id: 1,
                    view_id,
                    screen: Screen::Primary,
                    state: SearchState {
                        query: SearchQuery::literal("orphaned"),
                        matches,
                        request_id: 1,
                        ..SearchState::default()
                    },
                },
            )
            .expect("reject detached result")
        );
        assert_eq!(worker.match_scratch.as_ptr(), allocation);
        assert_eq!(worker.match_scratch.capacity(), capacity);
    }

    #[test]
    fn copy_mode_word_and_line_motions_use_terminal_cells() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"alpha beta!");
        let columns = terminal.cols().expect("columns");
        let rows = u32::try_from(terminal.total_rows().expect("rows")).expect("small fixture");

        let word = move_copy_word(
            &terminal,
            PointCoordinate { x: 0, y: 0 },
            columns,
            rows,
            &CopyModeAction::NextWord,
        )
        .expect("next word");
        assert_eq!(word.x, 6);
        let end = move_copy_word(&terminal, word, columns, rows, &CopyModeAction::NextWordEnd)
            .expect("word end");
        assert_eq!(end.x, 9);
        let previous = move_copy_word(
            &terminal,
            PointCoordinate { x: 10, y: 0 },
            columns,
            rows,
            &CopyModeAction::PreviousWord,
        )
        .expect("previous word");
        assert_eq!(previous.x, 6);
        assert_eq!(copy_line_end(&terminal, 0, columns).expect("line end"), 10);
    }

    #[test]
    fn native_copy_mode_word_motion_honors_separator_runs_and_empty_values() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"foo..bar baz");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mode = copy_mode.as_mut().expect("mode");
        let row = (0..mode.revision.total_rows())
            .find(|row| mode.revision.first_char(PointCoordinate { x: 0, y: *row }) == Some('f'))
            .expect("fixture row");

        mode.cursor = PointCoordinate { x: 0, y: row };
        move_copy_cursor(
            mode,
            &CopyModeAction::NextWord,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(mode.cursor.x, 3, "separator run is its own word");
        move_copy_cursor(
            mode,
            &CopyModeAction::NextWord,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(mode.cursor.x, 5, "next word follows the separator run");

        mode.cursor = PointCoordinate { x: 0, y: row };
        move_copy_cursor(
            mode,
            &CopyModeAction::NextWord,
            &WordSeparators::new(""),
            false,
        );
        assert_eq!(mode.cursor.x, 9, "empty separators stop only at whitespace");

        mode.cursor = PointCoordinate { x: 0, y: row };
        move_copy_cursor(
            mode,
            &CopyModeAction::NextSpaceEnd,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(mode.cursor.x, 8, "E treats punctuation as part of the word");
        mode.cursor = PointCoordinate { x: 0, y: row };
        move_copy_cursor(
            mode,
            &CopyModeAction::NextSpaceEnd,
            &WordSeparators::default(),
            true,
        );
        assert_eq!(mode.cursor.x, 7, "vi stops on the word's last cell");
        move_copy_cursor(
            mode,
            &CopyModeAction::NextSpace,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(
            mode.cursor.x, 9,
            "W advances to the next whitespace-delimited word"
        );
        move_copy_cursor(
            mode,
            &CopyModeAction::PreviousSpace,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(
            mode.cursor.x, 0,
            "B returns across punctuation to the word start"
        );
    }

    #[test]
    fn native_copy_mode_percent_matches_nested_brackets() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 24,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"foo..bar baz (a[b]c)");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mode = copy_mode.as_mut().expect("mode");
        let row = (0..mode.revision.total_rows())
            .find(|row| mode.revision.first_char(PointCoordinate { x: 0, y: *row }) == Some('f'))
            .expect("fixture row");

        mode.cursor = PointCoordinate { x: 0, y: row };
        move_copy_cursor(
            mode,
            &CopyModeAction::NextMatchingBracket,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(mode.cursor, PointCoordinate { x: 19, y: row });
        move_copy_cursor(
            mode,
            &CopyModeAction::NextMatchingBracket,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(mode.cursor, PointCoordinate { x: 13, y: row });
    }

    #[test]
    fn counted_copy_mode_actions_apply_pinned_parity_brackets_and_line_span() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 24,
            rows: 4,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"(a[b]c)\r\ntwo\r\nthree\r\nfour");
        let mut live_selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let first = (0..copy_mode.as_ref().expect("mode").revision.total_rows())
            .find(|row| {
                copy_mode
                    .as_ref()
                    .expect("mode")
                    .revision
                    .first_char(PointCoordinate { x: 0, y: *row })
                    == Some('(')
            })
            .expect("first fixture row");
        let mut unseen_output = 0;
        let separators = WordSeparators::default();

        {
            let mode = copy_mode.as_mut().expect("mode");
            mode.cursor = PointCoordinate { x: 2, y: first };
            mode.selection = Some(ModeSelection {
                anchor: PointCoordinate { x: 0, y: first },
                focus: PointCoordinate { x: 2, y: first },
                mode: SelectionMode::Cell,
                rectangle: false,
            });
            mode.selecting = true;
        }
        apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::OtherEnd,
            2,
            &separators,
            false,
        )
        .expect("even other-end");
        let mode = copy_mode.as_ref().expect("mode");
        let selection = mode.selection.expect("selection");
        assert_eq!(selection.anchor.x, 0);
        assert_eq!(selection.focus.x, 2);
        assert_eq!(mode.cursor.x, 2);

        apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::OtherEnd,
            3,
            &separators,
            false,
        )
        .expect("odd other-end");
        let mode = copy_mode.as_ref().expect("mode");
        let selection = mode.selection.expect("selection");
        assert_eq!(selection.anchor.x, 2);
        assert_eq!(selection.focus.x, 0);
        assert_eq!(mode.cursor.x, 0);

        copy_mode.as_mut().expect("mode").cursor = PointCoordinate { x: 0, y: first };
        apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::NextMatchingBracket,
            2,
            &separators,
            false,
        )
        .expect("two bracket transitions");
        assert_eq!(
            copy_mode.as_ref().expect("mode").cursor,
            PointCoordinate { x: 0, y: first }
        );
        apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::NextMatchingBracket,
            3,
            &separators,
            false,
        )
        .expect("three bracket transitions");
        assert_eq!(
            copy_mode.as_ref().expect("mode").cursor,
            PointCoordinate { x: 6, y: first }
        );

        copy_mode.as_mut().expect("mode").cursor = PointCoordinate { x: 0, y: first };
        apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::SelectLine,
            3,
            &separators,
            false,
        )
        .expect("three-line selection");
        let mode = copy_mode.as_ref().expect("mode");
        let selection = mode.selection.expect("line selection");
        assert_eq!(selection.anchor.y, first);
        assert_eq!(selection.focus.y, first + 2);
        assert_eq!(selection.mode, SelectionMode::Line);
        assert_eq!(mode.cursor.y, first + 2);
    }

    #[test]
    fn counted_copy_mode_once_policy_runs_stateful_actions_once() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"abcdef");
        let mut live_selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mut unseen_output = 0;
        let separators = WordSeparators::default();

        apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::ToggleRectangle,
            2,
            &separators,
            false,
        )
        .expect("toggle once");
        assert!(copy_mode.as_ref().expect("mode").rectangle);

        apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::StartSelection,
            2,
            &separators,
            false,
        )
        .expect("selection once");
        assert!(copy_mode.as_ref().expect("mode").selection.is_some());

        let copied = apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::copy_selection(crate::CopyModeCopy {
                request_id: 41,
                clipboard: false,
                buffer: None,
                pipe: None,
                clear_selection: true,
                cancel: true,
            }),
            2,
            &separators,
            false,
        )
        .expect("copy once");
        assert!(matches!(
            copied,
            ViewActionResult::Copy(copy) if copy.request_id == 41
        ));
        assert!(copy_mode.is_none());

        enter_copy_mode(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode again");
        apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::Cancel,
            2,
            &separators,
            false,
        )
        .expect("cancel once");
        assert!(copy_mode.is_none());
    }

    #[test]
    fn osc133_prompt_navigation_finds_prompts_and_command_output() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 24,
            rows: 8,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(
            b"\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\one\x1b]133;C\x1b\\\r\nout-one\r\n\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\two\x1b]133;C\x1b\\\r\nout-two",
        );

        assert_eq!(
            semantic_prompt_target(&terminal, 3, -1, false).expect("previous prompt"),
            Some(PointCoordinate { x: 0, y: 2 })
        );
        assert_eq!(
            semantic_prompt_target(&terminal, 2, -1, false).expect("older prompt"),
            Some(PointCoordinate { x: 0, y: 0 })
        );
        assert_eq!(
            semantic_prompt_target(&terminal, 0, 1, false).expect("next prompt"),
            Some(PointCoordinate { x: 0, y: 2 })
        );
        assert_eq!(
            semantic_prompt_target(&terminal, 3, -1, true).expect("previous output"),
            Some(PointCoordinate { x: 0, y: 3 })
        );

        let mut selection = None;
        assert!(
            select_semantic_output_at(
                &terminal,
                &mut selection,
                PointerCellEvent {
                    column: 2,
                    row: 1,
                    click_count: 3,
                    rectangle: false,
                },
            )
            .expect("semantic output selection")
        );
        assert_eq!(
            format_selection_text(&terminal).expect("selected output"),
            "out-one"
        );
    }

    #[test]
    fn last_command_capture_skips_the_fresh_prompt_and_keeps_the_previous_output() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 24,
            rows: 8,
            max_scrollback: 64,
        })
        .expect("terminal");
        terminal.vt_write(
            b"\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\one\x1b]133;C\x1b\\\r\nout-one\r\n\x1b]133;D;0\x1b\\\
              \x1b]133;A\x1b\\$ \x1b]133;B\x1b\\two --flag\x1b]133;C\x1b\\\r\nout-two\r\nmore-two\r\n\x1b]133;D;1\x1b\\\
              \x1b]133;A\x1b\\$ ",
        );

        let capture = capture_last_command(&terminal).expect("last command");
        assert_eq!(capture.command, "two --flag");
        assert_eq!(capture.output, "out-two\nmore-two");
        assert_eq!(capture.truncated_rows, 0);
    }

    #[test]
    fn last_command_capture_reports_missing_shell_integration() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 24,
            rows: 4,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"$ ls\r\nfile-a\r\n$ ");

        assert_eq!(
            capture_last_command(&terminal),
            Err(TerminalCaptureError::NoSemanticMarks)
        );
    }

    #[test]
    fn last_command_capture_caps_long_output_by_rows() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 24,
            rows: 8,
            max_scrollback: 4096,
        })
        .expect("terminal");
        let mut bytes = b"\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\loop\x1b]133;C\x1b\\\r\n".to_vec();
        for index in 0..(MAX_LAST_COMMAND_LINES + 25) {
            bytes.extend_from_slice(format!("line-{index}\r\n").as_bytes());
        }
        bytes.extend_from_slice(b"\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ ");
        terminal.vt_write(&bytes);

        let capture = capture_last_command(&terminal).expect("last command");
        assert_eq!(capture.command, "loop");
        assert_eq!(capture.output.lines().count(), MAX_LAST_COMMAND_LINES);
        assert!(capture.truncated_rows >= 25);
        assert!(capture.output.starts_with("line-25"));
        assert!(
            capture
                .output
                .ends_with(&format!("line-{}", MAX_LAST_COMMAND_LINES + 24))
        );
    }

    #[test]
    fn last_command_capture_walks_past_prompts_the_user_only_sat_on() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 24,
            rows: 8,
            max_scrollback: 64,
        })
        .expect("terminal");
        terminal.vt_write(
            b"\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\one\x1b]133;C\x1b\\\r\nout-one\r\n\x1b]133;D;0\x1b\\\
              \x1b]133;A\x1b\\$ \x1b]133;B\x1b\\\r\n\
              \x1b]133;A\x1b\\$ ",
        );

        let capture = capture_last_command(&terminal).expect("last command");
        assert_eq!(capture.command, "one");
        assert_eq!(capture.output, "out-one");
        assert_eq!(capture.truncated_rows, 0);
    }

    #[test]
    fn output_byte_cap_trims_whole_leading_lines() {
        let line = "x".repeat(1024);
        let mut output = String::new();
        for _ in 0..400 {
            output.push_str(&line);
            output.push('\n');
        }
        output.push_str(&line);

        let (clamped, dropped) = clamp_output_bytes(output);
        assert!(clamped.len() <= MAX_LAST_COMMAND_BYTES);
        assert!(dropped > 0);
        assert!(clamped.lines().all(|row| row.len() == 1024));
    }

    #[test]
    fn copy_mode_tracks_a_native_cursor() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"one\r\ntwo\r\nthree");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mode = copy_mode.as_mut().expect("mode");
        move_copy_cursor(mode, &CopyModeAction::Up, &WordSeparators::default(), false);
        let point = mode.cursor;
        assert!(point.y < u32::try_from(terminal.total_rows().expect("rows")).unwrap());
    }

    #[test]
    fn copy_mode_fixed_rows_reset_column() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 3,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"abcdefgh\r\nmiddle\r\nxy");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mode = copy_mode.as_mut().expect("mode");
        let viewport = mode.viewport_offset;
        let page = u32::from(mode.revision.viewport_rows);

        for (action, y) in [
            (CopyModeAction::TopLine, viewport),
            (
                CopyModeAction::MiddleLine,
                viewport.saturating_add(page.saturating_sub(1) / 2),
            ),
            (
                CopyModeAction::BottomLine,
                viewport.saturating_add(page.saturating_sub(1)),
            ),
        ] {
            mode.cursor = PointCoordinate { x: 6, y: viewport };
            move_copy_cursor(mode, &action, &WordSeparators::default(), false);
            assert_eq!(mode.cursor, PointCoordinate { x: 0, y }, "{action:?}");
            assert_eq!(mode.viewport_offset, viewport, "{action:?}");
        }
    }

    #[test]
    fn scroll_exit_cancels_at_and_when_reaching_the_bottom() {
        for action in [
            CopyModeAction::ScrollDown,
            CopyModeAction::PageDown,
            CopyModeAction::HalfPageDown,
        ] {
            assert!(
                !copy_mode_survives_downward_action(action.clone(), true, false, true),
                "{action:?} at bottom"
            );
            assert!(
                !copy_mode_survives_downward_action(action.clone(), true, false, false),
                "{action:?} reaching bottom"
            );
        }
    }

    #[test]
    fn scroll_exit_preserves_copy_mode_with_a_selection() {
        for action in [
            CopyModeAction::ScrollDown,
            CopyModeAction::PageDown,
            CopyModeAction::HalfPageDown,
        ] {
            assert!(
                copy_mode_survives_downward_action(action.clone(), true, true, true),
                "{action:?}"
            );
        }
    }

    #[test]
    fn scroll_exit_does_not_apply_to_cursor_down() {
        assert!(copy_mode_survives_downward_action(
            CopyModeAction::Down,
            true,
            false,
            true,
        ));
    }

    #[test]
    fn copy_mode_without_scroll_exit_never_cancels_on_downward_actions() {
        for action in [
            CopyModeAction::ScrollDown,
            CopyModeAction::PageDown,
            CopyModeAction::HalfPageDown,
        ] {
            assert!(
                copy_mode_survives_downward_action(action.clone(), false, false, true),
                "{action:?}"
            );
        }
    }

    #[test]
    fn one_shot_page_down_scroll_exit_works_in_existing_plain_copy_mode() {
        assert!(!copy_mode_survives_downward_action(
            CopyModeAction::PageDownScrollExit,
            false,
            false,
            true,
        ));
        assert!(!copy_mode_survives_downward_action(
            CopyModeAction::PageDownScrollExit,
            false,
            false,
            false,
        ));
        assert!(copy_mode_survives_downward_action(
            CopyModeAction::PageDownScrollExit,
            false,
            true,
            true,
        ));
    }

    #[test]
    fn scroll_exit_latches_only_on_fresh_copy_mode_entry() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut selection = None;
        let mut copy_mode = None;

        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("plain entry");
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            true,
            false,
            None,
            true,
        )
        .expect("repeated scroll-exit entry");
        assert!(!copy_mode.as_ref().expect("mode").scroll_exit);

        copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            true,
            false,
            None,
            true,
        )
        .expect("scroll-exit entry");
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("repeated plain entry");
        assert!(copy_mode.as_ref().expect("mode").scroll_exit);
    }

    #[test]
    fn hide_position_latches_on_entry_and_leaves_scroll_exit_alone() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        for (scroll_exit, hide_position) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut selection = None;
            let mut copy_mode = None;
            enter_copy_mode(
                &mut terminal,
                &mut selection,
                &mut copy_mode,
                scroll_exit,
                hide_position,
                None,
                true,
            )
            .expect("copy mode");
            let mode = copy_mode.as_ref().expect("mode");
            assert_eq!(mode.scroll_exit, scroll_exit);
            assert_eq!(mode.hide_position, hide_position);
            assert_eq!(mode.kind, FrozenModeKind::Copy);
        }
    }

    #[test]
    fn absolute_scroll_clamps_and_restores_follow_bottom() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 3,
            max_scrollback: 32,
        })
        .expect("terminal");
        terminal.vt_write(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive");
        let maximum = terminal
            .scrollbar()
            .expect("scrollbar")
            .total
            .saturating_sub(terminal.scrollbar().expect("scrollbar").len);
        assert!(maximum > 1);

        let mut view = TerminalViewState::for_screen(Screen::Primary);
        {
            let state = view.active_mut();
            state.unseen_output = 4;
            scroll_to_offset(
                &mut terminal,
                &mut state.copy_mode,
                &mut state.unseen_output,
                1,
            )
            .expect("absolute scroll");
        }
        assert_eq!(terminal.scrollbar().expect("scrollbar").offset, 1);
        sync_viewport_anchor(&terminal, &mut view).expect("pin viewport");
        assert!(matches!(view.viewport, ViewportAnchor::Pinned(_)));

        {
            let state = view.active_mut();
            scroll_to_offset(
                &mut terminal,
                &mut state.copy_mode,
                &mut state.unseen_output,
                u32::MAX,
            )
            .expect("clamped tail scroll");
        }
        let scrollbar = terminal.scrollbar().expect("scrollbar");
        assert_eq!(scrollbar.offset, maximum);
        assert_eq!(view.unseen_output, 0);
        sync_viewport_anchor(&terminal, &mut view).expect("follow viewport");
        assert!(matches!(view.viewport, ViewportAnchor::FollowBottom));

        terminal.vt_write(b"\r\nsix");
        restore_view_state(&mut terminal, &mut view, &WordSeparators::default())
            .expect("restore followed viewport");
        let scrollbar = terminal.scrollbar().expect("scrollbar after output");
        assert!(scrollbar.offset.saturating_add(scrollbar.len) >= scrollbar.total);

        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let copy_maximum = copy_mode
            .as_ref()
            .expect("copy mode")
            .revision
            .maximum_offset();
        let mut unseen_output = 0;
        scroll_to_offset(&mut terminal, &mut copy_mode, &mut unseen_output, u32::MAX)
            .expect("copy mode absolute scroll");
        assert_eq!(
            copy_mode.as_ref().expect("copy mode").viewport_offset,
            copy_maximum
        );
    }

    #[test]
    fn frozen_copy_formatter_preserves_code_whitespace_and_line_structure() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 4,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"  one\r\n\r\n    two");
        let revision = ModeRevision::capture(&mut terminal).expect("revision");
        let row_with = |needle| {
            (0..revision.total_rows())
                .find(|row| {
                    (0..revision.columns).any(|column| {
                        revision.first_char(PointCoordinate { x: column, y: *row }) == Some(needle)
                    })
                })
                .expect("fixture row")
        };
        let one = row_with('o');
        let two = row_with('t');

        assert_eq!(
            revision.format_selection(
                ModeSelection {
                    anchor: PointCoordinate { x: 0, y: one },
                    focus: PointCoordinate { x: 6, y: two },
                    mode: SelectionMode::Cell,
                    rectangle: false,
                },
                true,
            ),
            "  one\n\n    two"
        );
        assert_eq!(
            revision.format_selection(
                ModeSelection {
                    anchor: PointCoordinate { x: 0, y: one },
                    focus: PointCoordinate {
                        x: revision.columns - 1,
                        y: one,
                    },
                    mode: SelectionMode::Line,
                    rectangle: false,
                },
                true,
            ),
            "  one\n"
        );
    }

    #[test]
    fn frozen_copy_formatter_preserves_wrapped_and_rectangular_columns() {
        let mut wrapped = Terminal::new(TerminalOptions {
            cols: 5,
            rows: 3,
            max_scrollback: 16,
        })
        .expect("wrapped terminal");
        wrapped.vt_write(b"ab  cdef");
        let revision = ModeRevision::capture(&mut wrapped).expect("wrapped revision");
        let first = (0..revision.total_rows())
            .find(|row| revision.first_char(PointCoordinate { x: 0, y: *row }) == Some('a'))
            .expect("wrapped row");
        assert!(revision.row(first).wrapped());
        assert_eq!(
            revision.format_selection(
                ModeSelection {
                    anchor: PointCoordinate { x: 0, y: first },
                    focus: PointCoordinate { x: 2, y: first + 1 },
                    mode: SelectionMode::Cell,
                    rectangle: false,
                },
                true,
            ),
            "ab  cdef"
        );

        let mut rectangle = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("rectangle terminal");
        rectangle.vt_write(b"ab  z\r\nc   y");
        let revision = ModeRevision::capture(&mut rectangle).expect("rectangle revision");
        let first = (0..revision.total_rows())
            .find(|row| revision.first_char(PointCoordinate { x: 0, y: *row }) == Some('a'))
            .expect("first rectangle row");
        let second = (0..revision.total_rows())
            .find(|row| revision.first_char(PointCoordinate { x: 0, y: *row }) == Some('c'))
            .expect("second rectangle row");
        assert_eq!(
            revision.format_selection(
                ModeSelection {
                    anchor: PointCoordinate { x: 1, y: first },
                    focus: PointCoordinate { x: 3, y: second },
                    mode: SelectionMode::Cell,
                    rectangle: true,
                },
                true,
            ),
            "b  \n   "
        );
        assert_eq!(
            revision.format_selection(
                ModeSelection {
                    anchor: PointCoordinate { x: 1, y: first },
                    focus: PointCoordinate { x: 3, y: second },
                    mode: SelectionMode::Cell,
                    rectangle: true,
                },
                false,
            ),
            "b \n  "
        );
    }

    #[test]
    fn copy_mode_text_round_trips_through_the_paste_encoder() {
        let expected = "    let  x = 1";
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(expected.as_bytes());
        let mut live_selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let row = (0..copy_mode.as_ref().expect("mode").revision.total_rows())
            .find(|row| {
                copy_mode
                    .as_ref()
                    .expect("mode")
                    .revision
                    .first_char(PointCoordinate { x: 4, y: *row })
                    == Some('l')
            })
            .expect("fixture row");
        let mode = copy_mode.as_mut().expect("mode");
        mode.selection = Some(ModeSelection {
            anchor: PointCoordinate { x: 0, y: row },
            focus: PointCoordinate { x: 13, y: row },
            mode: SelectionMode::Cell,
            rectangle: false,
        });
        mode.selecting = true;
        let mut unseen_output = 0;

        let result = apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::copy_selection(crate::CopyModeCopy {
                request_id: 1,
                clipboard: true,
                buffer: None,
                pipe: None,
                clear_selection: false,
                cancel: false,
            }),
            &WordSeparators::default(),
            true,
        )
        .expect("copy");
        let ViewActionResult::Copy(copy) = result else {
            panic!("expected copied text");
        };
        assert_eq!(copy.text, expected);

        let mut pasted = Vec::new();
        write_paste_bytes(&terminal, copy.text.into_bytes(), true, &mut pasted)
            .expect("paste encoding");
        assert_eq!(pasted, expected.as_bytes());

        copy_mode.as_mut().expect("mode").cursor = PointCoordinate { x: 4, y: row };
        let result = apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::copy_end_of_line(crate::CopyModeCopy {
                request_id: 2,
                clipboard: true,
                buffer: None,
                pipe: None,
                clear_selection: false,
                cancel: false,
            }),
            &WordSeparators::default(),
            false,
        )
        .expect("copy to end of line");
        assert!(matches!(
            result,
            ViewActionResult::Copy(copy) if copy.text == "let  x = 1"
        ));
    }

    #[test]
    fn counted_copy_mode_end_of_line_spans_the_requested_rows_once() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 4,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"abcdef\r\nghijkl\r\nmnopqr");
        let mut live_selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let first = (0..copy_mode.as_ref().expect("mode").revision.total_rows())
            .find(|row| {
                copy_mode
                    .as_ref()
                    .expect("mode")
                    .revision
                    .first_char(PointCoordinate { x: 0, y: *row })
                    == Some('a')
            })
            .expect("first fixture row");
        copy_mode.as_mut().expect("mode").cursor = PointCoordinate { x: 2, y: first };
        let mut unseen_output = 0;

        let result = apply_counted_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::copy_end_of_line(crate::CopyModeCopy {
                request_id: 3,
                clipboard: true,
                buffer: None,
                pipe: None,
                clear_selection: false,
                cancel: false,
            }),
            3,
            &WordSeparators::default(),
            false,
        )
        .expect("counted copy to end of line");

        assert!(matches!(
            result,
            ViewActionResult::Copy(copy) if copy.text == "cdef\nghijkl\nmnopqr"
        ));
        assert_eq!(
            copy_mode.as_ref().expect("mode").cursor,
            PointCoordinate { x: 2, y: first }
        );
    }

    #[test]
    fn copy_mode_copy_variants_preserve_clear_and_cancel_independently() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"abcdef");
        let mut live_selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let row = copy_mode.as_ref().expect("mode").cursor.y;
        let selection = ModeSelection {
            anchor: PointCoordinate { x: 0, y: row },
            focus: PointCoordinate { x: 2, y: row },
            mode: SelectionMode::Cell,
            rectangle: false,
        };
        {
            let mode = copy_mode.as_mut().expect("mode");
            mode.selection = Some(selection);
            mode.selecting = true;
        }
        let mut unseen_output = 4;
        let word_separators = WordSeparators::default();

        let result = apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::copy_selection(crate::CopyModeCopy {
                request_id: 7,
                clipboard: false,
                buffer: Some(PasteBufferAction::Create {
                    prefix: Some("native".to_owned()),
                }),
                pipe: Some("cat".to_owned()),
                clear_selection: false,
                cancel: false,
            }),
            &word_separators,
            false,
        )
        .expect("copy without clear");
        assert!(matches!(
            result,
            ViewActionResult::Copy(copy)
                if copy.request_id == 7
                    && copy.clipboard.is_none()
                    && matches!(copy.buffer, Some(PasteBufferAction::Create { .. }))
                    && copy.pipe.as_deref() == Some("cat")
                    && !copy.view_changed
        ));
        assert_eq!(copy_mode.as_ref().expect("mode").selection, Some(selection));

        let result = apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::copy_selection(crate::CopyModeCopy {
                request_id: 8,
                clipboard: true,
                buffer: None,
                pipe: None,
                clear_selection: true,
                cancel: false,
            }),
            &word_separators,
            false,
        )
        .expect("copy and clear");
        assert!(matches!(
            result,
            ViewActionResult::Copy(copy)
                if copy.clipboard == Some(ClipboardTarget::Clipboard) && copy.view_changed
        ));
        assert!(copy_mode.as_ref().expect("mode").selection.is_none());
        assert_eq!(unseen_output, 4);

        {
            let mode = copy_mode.as_mut().expect("mode");
            mode.selection = Some(selection);
            mode.selecting = true;
        }
        let result = apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::copy_selection(crate::CopyModeCopy {
                request_id: 9,
                clipboard: false,
                buffer: Some(PasteBufferAction::Append),
                pipe: None,
                clear_selection: true,
                cancel: true,
            }),
            &word_separators,
            false,
        )
        .expect("append and cancel");
        assert!(matches!(
            result,
            ViewActionResult::Copy(copy)
                if matches!(copy.buffer, Some(PasteBufferAction::Append)) && copy.view_changed
        ));
        assert!(copy_mode.is_none());
        assert_eq!(unseen_output, 0);
    }

    #[test]
    fn copy_mode_marks_paragraphs_and_selection_modes_use_the_frozen_revision() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 12,
            rows: 4,
            max_scrollback: 32,
        })
        .expect("terminal");
        terminal.vt_write(b"  one\r\n\r\ntwo\r\nthree\r\n\r\nfour");
        let mut live_selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let row_with = |mode: &CopyModeState, needle: char| {
            (0..mode.revision.total_rows())
                .find(|row| {
                    (0..mode.revision.columns).any(|column| {
                        mode.revision
                            .first_char(PointCoordinate { x: column, y: *row })
                            .is_some_and(|character| character == needle)
                    })
                })
                .expect("fixture row")
        };
        let one = row_with(copy_mode.as_ref().expect("mode"), 'o');
        let two = row_with(copy_mode.as_ref().expect("mode"), 't');
        let four = row_with(copy_mode.as_ref().expect("mode"), 'f');
        let three = (0..copy_mode.as_ref().expect("mode").revision.total_rows())
            .find(|row| {
                copy_mode
                    .as_ref()
                    .expect("mode")
                    .revision
                    .first_char(PointCoordinate { x: 1, y: *row })
                    == Some('h')
            })
            .expect("three row");
        let mut unseen_output = 0;
        let word_separators = WordSeparators::default();

        {
            let mode = copy_mode.as_mut().expect("mode");
            mode.cursor = PointCoordinate { x: 3, y: four };
        }
        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::SetMark,
            &word_separators,
            false,
        )
        .expect("set mark");
        copy_mode.as_mut().expect("mode").cursor = PointCoordinate { x: 0, y: one };
        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::JumpToMark,
            &word_separators,
            false,
        )
        .expect("jump to mark");
        assert_eq!(
            copy_mode.as_ref().expect("mode").cursor,
            PointCoordinate { x: 3, y: four }
        );

        {
            let mode = copy_mode.as_mut().expect("mode");
            mode.cursor = PointCoordinate { x: 0, y: one };
            move_copy_cursor(
                mode,
                &CopyModeAction::BackToIndentation,
                &word_separators,
                false,
            );
            assert_eq!(mode.cursor.x, 2);
            mode.cursor = PointCoordinate { x: 0, y: two };
            move_copy_cursor(
                mode,
                &CopyModeAction::NextParagraph,
                &word_separators,
                false,
            );
            assert_eq!(mode.cursor.y, four);
            move_copy_cursor(
                mode,
                &CopyModeAction::PreviousParagraph,
                &word_separators,
                false,
            );
            assert_eq!(mode.cursor.y, two);
            mode.cursor = PointCoordinate { x: 3, y: one };
        }

        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::SelectWord,
            &word_separators,
            false,
        )
        .expect("select word");
        let selection = copy_mode
            .as_ref()
            .expect("mode")
            .selection
            .expect("word selection");
        assert_eq!(selection.anchor.x, 2);
        assert_eq!(selection.focus.x, 4);

        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::OtherEnd,
            &word_separators,
            false,
        )
        .expect("switch active end");
        assert_eq!(copy_mode.as_ref().expect("mode").cursor.x, 2);

        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::ClearSelection,
            &word_separators,
            false,
        )
        .expect("clear selection");
        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::ToggleRectangle,
            &word_separators,
            false,
        )
        .expect("enable rectangle before selection");
        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::StartSelection,
            &word_separators,
            false,
        )
        .expect("start rectangle selection");
        assert!(
            copy_mode
                .as_ref()
                .expect("mode")
                .selection
                .expect("selection")
                .rectangle
        );

        copy_mode.as_mut().expect("mode").cursor = PointCoordinate { x: 0, y: three };
        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::Jump(CopyJump {
                target: "e".to_owned(),
                direction: CopyJumpDirection::Forward,
                to: false,
            }),
            &word_separators,
            false,
        )
        .expect("jump forward");
        assert_eq!(copy_mode.as_ref().expect("mode").cursor.x, 3);
        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::RepeatJump { reverse: false },
            &word_separators,
            false,
        )
        .expect("repeat jump");
        assert_eq!(copy_mode.as_ref().expect("mode").cursor.x, 4);
        apply_copy_mode_action(
            &mut terminal,
            &mut live_selection,
            &mut copy_mode,
            &mut unseen_output,
            CopyModeAction::RepeatJump { reverse: true },
            &word_separators,
            false,
        )
        .expect("reverse jump");
        assert_eq!(copy_mode.as_ref().expect("mode").cursor.x, 3);
    }

    #[test]
    fn incremental_copy_mode_search_keeps_its_original_anchor() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"one\r\ntwo\r\nthree");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mut origin = None;

        let initial = search_selection_policy(
            copy_mode.as_deref(),
            &mut origin,
            SearchDirection::Backward,
            true,
        );
        let SearchSelectionPolicy::From {
            point: initial_point,
            ..
        } = initial
        else {
            panic!("copy mode must establish a search origin");
        };

        move_copy_cursor(
            copy_mode.as_mut().expect("copy mode"),
            &CopyModeAction::Up,
            &WordSeparators::default(),
            false,
        );
        let moved_point = copy_mode.as_ref().expect("copy mode").cursor;
        assert_ne!(moved_point, initial_point);

        let updated = search_selection_policy(
            copy_mode.as_deref(),
            &mut origin,
            SearchDirection::Backward,
            false,
        );
        assert!(matches!(
            updated,
            SearchSelectionPolicy::From { point, .. } if point == initial_point
        ));
    }

    #[test]
    fn copy_mode_revision_stays_frozen_across_output_resize_and_screen_switch() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 16,
            rows: 3,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"old-one\r\nold-two\r\n\x1b[1;31mfrozen-marker\x1b[0m");
        let mut selection = None;
        let mut mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mut view = TerminalViewState::for_screen(Screen::Primary);
        view.copy_mode = mode;
        let mut render_state = RenderState::new().expect("render state");
        let mut row_iterator = RowIterator::new().expect("rows");
        let mut cell_iterator = CellIterator::new().expect("cells");
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();

        let frozen = snapshot(
            &terminal,
            &mut render_state,
            &mut row_iterator,
            &mut cell_iterator,
            &mut generations,
            SnapshotChange::View,
            &mut dictionary,
            Some(&view),
            SessionStatus::Running,
        )
        .expect("frozen snapshot");
        terminal.vt_write(b"\r\nlive-one\r\nlive-two\x1b[?1049hALT-SCREEN");
        terminal.resize(22, 5, 8, 18).expect("resize live terminal");
        view.note_output(Screen::Alternate);
        reconcile_view_screen(&mut terminal, &mut view, &WordSeparators::default())
            .expect("frozen screen");

        let after = snapshot(
            &terminal,
            &mut render_state,
            &mut row_iterator,
            &mut cell_iterator,
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            Some(&view),
            SessionStatus::Running,
        )
        .expect("snapshot after live changes");
        assert_eq!((after.columns, after.rows), (16, 3));
        assert!(Arc::ptr_eq(&frozen.cells, &after.cells));
        assert_eq!(frozen.cells, after.cells);
        assert_eq!(after.unseen_output, 1);
        assert_eq!(view.screen, Screen::Primary);
        let patch = TerminalViewport::diff(&frozen, &after).expect("metadata-only mode patch");
        assert!(patch.changed_rows.is_empty());

        let captured = capture_terminal(
            &terminal,
            view.copy_mode.as_deref(),
            CaptureOptions {
                mode: true,
                ..CaptureOptions::default()
            },
        )
        .expect("mode capture");
        assert!(captured.contains("frozen-marker"));
        assert!(!captured.contains("live-one"));
        assert!(!captured.contains("ALT-SCREEN"));
        let captured_vt = capture_terminal(
            &terminal,
            view.copy_mode.as_deref(),
            CaptureOptions {
                mode: true,
                escape_sequences: true,
                ..CaptureOptions::default()
            },
        )
        .expect("styled mode capture");
        assert!(captured_vt.contains("\x1b[0;1;38;2;"));
        assert!(captured_vt.contains("frozen-marker"));
    }

    #[test]
    fn key_mapping_covers_terminal_navigation() {
        assert_eq!(ghostty_key(KeyCode::Character('c')), key::Key::C);
        assert_eq!(ghostty_key(KeyCode::ArrowLeft), key::Key::ArrowLeft);
        assert_eq!(ghostty_key(KeyCode::Function(12)), key::Key::F12);
    }

    #[test]
    fn pty_reader_recycles_its_bounded_buffer_pool() {
        let (output_tx, output_rx) = crossbeam_channel::bounded(1);
        let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(2);
        let mut pool_allocations = Vec::new();
        for _ in 0..2 {
            let buffer = vec![0_u8; PTY_READ_BUFFER_BYTES];
            pool_allocations.push(buffer.as_ptr() as usize);
            recycle_tx.send(buffer).expect("seed recycle pool");
        }
        let input = vec![b'x'; PTY_READ_BUFFER_BYTES * 3];
        let reader = thread::spawn(move || {
            read_pty(
                Box::new(std::io::Cursor::new(input)),
                Box::new(|| 0),
                output_tx,
                recycle_rx,
            );
        });

        let mut observed = Vec::new();
        for _ in 0..3 {
            let ReaderMessage::Data { buffer, length } = output_rx.recv().expect("PTY data") else {
                panic!("PTY reader reached EOF before all input was delivered");
            };
            assert_eq!(length, PTY_READ_BUFFER_BYTES);
            assert!(buffer[..length].iter().all(|byte| *byte == b'x'));
            observed.push(buffer.as_ptr() as usize);
            let _ = recycle_tx.send(buffer);
        }
        assert!(matches!(
            output_rx.recv().expect("PTY EOF"),
            ReaderMessage::Eof
        ));
        reader.join().expect("PTY reader thread");

        assert!(
            observed
                .iter()
                .all(|pointer| pool_allocations.contains(pointer))
        );
        observed.sort_unstable();
        observed.dedup();
        assert!(observed.len() < 3);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_pty_gather_preserves_order_through_bounded_buffers() {
        let (read_fd, write_fd) = rustix::pipe::pipe().expect("PTY fixture pipe");
        rustix::io::ioctl_fionbio(&read_fd, true).expect("nonblocking fixture reader");
        let (output_tx, output_rx) = crossbeam_channel::bounded(PTY_BUFFER_POOL_SIZE);
        let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(PTY_BUFFER_POOL_SIZE);
        let mut allocations = Vec::new();
        for _ in 0..PTY_BUFFER_POOL_SIZE {
            let buffer = vec![0_u8; PTY_READ_BUFFER_BYTES];
            allocations.push(buffer.as_ptr() as usize);
            recycle_tx.send(buffer).expect("seed gather pool");
        }
        let gather = thread::spawn(move || gather_pty_linux(read_fd, output_tx, recycle_rx));
        let expected = (0..PTY_READ_BUFFER_BYTES * 2 + 137)
            .map(|index| u8::try_from(index % 251).expect("bounded byte"))
            .collect::<Vec<_>>();
        let writer_expected = expected.clone();
        let writer = thread::spawn(move || {
            let mut offset = 0;
            while offset < writer_expected.len() {
                match rustix::io::write(&write_fd, &writer_expected[offset..]) {
                    Ok(written) => offset += written,
                    Err(rustix::io::Errno::INTR) => {}
                    Err(error) => panic!("fixture write failed: {error}"),
                }
            }
        });

        let mut observed = Vec::with_capacity(expected.len());
        let mut observed_allocations = Vec::new();
        while let ReaderMessage::Data { buffer, length } = output_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("gather output")
        {
            observed.extend_from_slice(&buffer[..length]);
            observed_allocations.push(buffer.as_ptr() as usize);
            let _ = recycle_tx.send(buffer);
        }
        writer.join().expect("fixture writer");
        gather.join().expect("gather thread");

        assert_eq!(observed, expected);
        assert!(
            observed_allocations
                .iter()
                .all(|pointer| allocations.contains(pointer))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_pty_gather_delivers_an_interactive_partial_batch() {
        let (read_fd, write_fd) = rustix::pipe::pipe().expect("PTY fixture pipe");
        rustix::io::ioctl_fionbio(&read_fd, true).expect("nonblocking fixture reader");
        let (output_tx, output_rx) = crossbeam_channel::bounded(PTY_BUFFER_POOL_SIZE);
        let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(PTY_BUFFER_POOL_SIZE);
        for _ in 0..PTY_BUFFER_POOL_SIZE {
            recycle_tx
                .send(vec![0_u8; PTY_READ_BUFFER_BYTES])
                .expect("seed gather pool");
        }
        let gather = thread::spawn(move || gather_pty_linux(read_fd, output_tx, recycle_rx));

        assert_eq!(
            rustix::io::write(&write_fd, b"prompt").expect("interactive fixture write"),
            6
        );
        let ReaderMessage::Data { buffer, length } = output_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("interactive gather output")
        else {
            panic!("gather reached EOF before delivering the partial batch");
        };
        assert_eq!(&buffer[..length], b"prompt");
        recycle_tx.send(buffer).expect("recycle gather buffer");

        drop(write_fd);
        assert!(matches!(
            output_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("gather EOF"),
            ReaderMessage::Eof
        ));
        gather.join().expect("gather thread");
    }

    #[test]
    fn pty_output_bursts_leave_ready_data_for_the_next_actor_turn() {
        let (output_tx, output_rx) = crossbeam_channel::unbounded();
        for byte in 1..=PTY_BUFFER_POOL_SIZE {
            output_tx
                .send(ReaderMessage::Data {
                    buffer: vec![u8::try_from(byte).expect("test byte")],
                    length: 1,
                })
                .expect("queue PTY data");
        }

        let mut consumed = Vec::new();
        let reached_eof = drain_pty_output_burst(&output_rx, vec![0], 1, |buffer, length| {
            consumed.extend_from_slice(&buffer[..length]);
        });

        let burst_limit = u8::try_from(PTY_BUFFER_POOL_SIZE).expect("burst limit fits in a byte");
        assert!(!reached_eof);
        assert_eq!(consumed, (0..burst_limit).collect::<Vec<_>>());
        assert_eq!(output_rx.len(), 1);
        assert!(matches!(
            output_rx.try_recv(),
            Ok(ReaderMessage::Data { buffer, length: 1 })
                if buffer == vec![burst_limit]
        ));
    }

    #[test]
    fn raw_output_tap_blocks_at_four_read_chunks_and_receiver_drop_unblocks_it() {
        let (output, receiver) = crossbeam_channel::bounded(RAW_OUTPUT_TAP_PENDING_CHUNKS);
        let mut tap = Some((1, output));
        for _ in 0..RAW_OUTPUT_TAP_PENDING_CHUNKS {
            assert_eq!(
                tap_raw_output(&mut tap, &vec![0_u8; PTY_READ_BUFFER_BYTES]),
                None
            );
        }
        let (entered, waiting) = crossbeam_channel::bounded(1);
        let (finished, done) = crossbeam_channel::bounded(1);
        let worker = thread::spawn(move || {
            entered.send(()).expect("announce blocked send");
            let closed = tap_raw_output(&mut tap, &vec![1_u8; PTY_READ_BUFFER_BYTES]);
            finished.send(closed).expect("announce completion");
        });
        waiting.recv().expect("worker entered send");
        assert!(matches!(
            done.recv_timeout(Duration::from_millis(50)),
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
        ));
        drop(receiver);
        assert_eq!(
            done.recv_timeout(Duration::from_secs(2))
                .expect("receiver drop unblocked tap"),
            Some(1)
        );
        worker.join().expect("tap worker");
    }

    #[test]
    fn pty_free_surface_feeds_bytes_into_capture_marks_taps_and_bell() {
        let session = TerminalSession::spawn_empty_with_appearance(
            64,
            Arc::new(TerminalAppearance::default()),
        );
        let events = session.events();
        let (output, tapped) = TerminalSession::raw_output_tap_channel();
        session
            .arm_raw_output_tap(1, output)
            .expect("a PTY-free surface accepts a tap");

        assert!(session.feed(Arc::from(
            b"\x1b]133;A\x07> \x1b]133;B\x07say hi\x1b]133;C\x07\r\nhello from the agent\r\n\x1b]133;D;0\x07\x07"
                .as_slice()
        )));
        let capture =
            wait_for_test_capture(&session, |capture| capture.contains("hello from the agent"));
        assert!(capture.contains("> say hi"), "{capture:?}");
        let last = session
            .capture_last_command()
            .expect("OSC 133 marks land from fed bytes");
        assert_eq!(last.command, "say hi");
        assert_eq!(last.output, "hello from the agent");

        let tapped = tapped
            .recv_timeout(Duration::from_secs(5))
            .expect("the tap sees the fed bytes");
        assert!(tapped.starts_with(b"\x1b]133;A"));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match events.recv_blocking().expect("events stay open") {
                TerminalEvent::Bell => break,
                _ => assert!(Instant::now() < deadline, "no bell from the fed BEL"),
            }
        }
    }

    fn wait_for_test_capture(session: &TerminalSession, accept: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let capture = session
                .capture(CaptureOptions::default())
                .expect("capture the fed surface");
            if accept(&capture) {
                return capture;
            }
            assert!(
                Instant::now() < deadline,
                "capture never converged: {capture:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn raw_output_tap_starts_at_arm_and_stops_at_disarm() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                command: Some(vec![
                    "read _; printf 'BEFORE\\n'; read _; printf 'AFTER\\001\\002\\n'; read _; printf 'LATER\\n'; read _"
                        .to_owned(),
                ]),
                ..TerminalSpawn::default()
            },
        );
        session.attach_view(TerminalViewId(104));
        wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.status, SessionStatus::Running)
        });
        session.send_raw_input(Arc::from(b"first\n".as_slice()));
        wait_for_test_viewport(&session, |viewport| {
            let mut contents = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut contents);
            }
            contents.contains("BEFORE")
        });

        let (tap, output) = TerminalSession::raw_output_tap_channel();
        session.arm_raw_output_tap(9, tap).expect("arm tap");
        session.send_raw_input(Arc::from(b"second\n".as_slice()));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut bytes = Vec::new();
        while !bytes.windows(5).any(|window| window == b"AFTER") {
            let chunk = output
                .recv_deadline(deadline)
                .expect("post-arm terminal output");
            bytes.extend_from_slice(&chunk);
        }
        assert!(!bytes.windows(6).any(|window| window == b"BEFORE"));
        assert!(bytes.windows(7).any(|window| window == b"AFTER\x01\x02"));

        session.disarm_raw_output_tap(9).expect("disarm tap");
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(2)),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected)
        ));
        session.send_raw_input(Arc::from(b"third\n".as_slice()));
        wait_for_test_viewport(&session, |viewport| {
            let mut contents = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut contents);
            }
            contents.contains("LATER")
        });
    }

    #[cfg(unix)]
    #[test]
    fn raw_output_tap_receiver_loss_is_reported() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                command: Some(vec!["read _; printf 'TAP_CLOSED\\n'; read _".to_owned()]),
                ..TerminalSpawn::default()
            },
        );
        let events = session.events();
        wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.status, SessionStatus::Running)
        });

        let (tap, output) = TerminalSession::raw_output_tap_channel();
        session.arm_raw_output_tap(27, tap).expect("arm tap");
        drop(output);
        session.send_raw_input(Arc::from(b"ready\n".as_slice()));

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match events.try_recv() {
                Ok(TerminalEvent::RawOutputTapClosed { token }) => {
                    assert_eq!(token, 27);
                    break;
                }
                Ok(_) | Err(async_channel::TryRecvError::Empty) => {}
                Err(async_channel::TryRecvError::Closed) => {
                    panic!("terminal stopped before reporting the closed tap")
                }
            }
            assert!(
                Instant::now() < deadline,
                "terminal did not report the closed tap"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn terminal_responses_reuse_one_ordered_actor_buffer() {
        let effects = Rc::new(RefCell::new(PtyEffects::new()));
        let effect_sink = Rc::clone(&effects);
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal
            .on_pty_write(move |_, bytes| effect_sink.borrow_mut().push(bytes))
            .expect("PTY response callback");

        terminal.vt_write(b"\x1b[5n\x1b[6n");
        assert_eq!(effects.borrow().bytes, b"\x1b[0n\x1b[1;1R");
        let allocation = effects.borrow().bytes.as_ptr();
        let capacity = effects.borrow().bytes.capacity();
        let mut output = Vec::new();
        drain_effects(&effects, &mut output).expect("first response drain");
        assert_eq!(output, b"\x1b[0n\x1b[1;1R");
        assert!(effects.borrow().bytes.is_empty());

        terminal.vt_write(b"\x1b[5n\x1b[6n");
        drain_effects(&effects, &mut output).expect("second response drain");
        assert_eq!(output, b"\x1b[0n\x1b[1;1R\x1b[0n\x1b[1;1R");
        assert_eq!(effects.borrow().bytes.as_ptr(), allocation);
        assert_eq!(effects.borrow().bytes.capacity(), capacity);
    }

    #[test]
    fn device_attributes_answer_da1_after_kitty_probe_queries() {
        let effects = Rc::new(RefCell::new(PtyEffects::new()));
        let effect_sink = Rc::clone(&effects);
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal
            .on_pty_write(move |_, bytes| effect_sink.borrow_mut().push(bytes))
            .expect("PTY response callback");
        register_device_attributes(&mut terminal).expect("device attributes callback");

        terminal.vt_write(b"\x1b[c");
        assert_eq!(effects.borrow().bytes, b"\x1b[?62;22c");
    }

    #[test]
    fn terminal_reports_configured_colors_for_osc_10_and_11_queries() {
        let effects = Rc::new(RefCell::new(PtyEffects::new()));
        let effect_sink = Rc::clone(&effects);
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal
            .on_pty_write(move |_, bytes| effect_sink.borrow_mut().push(bytes))
            .expect("PTY response callback");
        apply_terminal_appearance(&mut terminal, &TerminalAppearance::default())
            .expect("terminal appearance");

        terminal.vt_write(b"\x1b]10;?\x1b\\\x1b]11;?\x07");

        assert_eq!(
            effects.borrow().bytes,
            b"\x1b]10;rgb:d8d8/dede/e9e9\x1b\\\x1b]11;rgb:1010/1313/1818\x07"
        );
    }

    #[test]
    fn osc_52_writes_become_pane_scoped_clipboard_events() {
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let publisher = Publisher {
            event_tx,
            latest: Arc::new(RwLock::new(PublishedViewports::new(
                TerminalViewport::blank(1, 1, SessionStatus::Running),
            ))),
            state: Arc::clone(&event_state),
        };
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        register_clipboard_write(&mut terminal, publisher).expect("clipboard write callback");

        terminal.vt_write(b"\x1b]52;c;enogY2xpcGJvYXJk\x07");
        assert!(matches!(
            events.try_recv().expect("clipboard write event"),
            TerminalEvent::ClipboardSet {
                target: ClipboardTarget::Clipboard,
                text,
            } if text == "zz clipboard"
        ));

        terminal.vt_write(b"\x1b]52;p;cHJpbWFyeSBwaWNr\x1b\\");
        assert!(matches!(
            events.try_recv().expect("primary selection write event"),
            TerminalEvent::ClipboardSet {
                target: ClipboardTarget::Primary,
                text,
            } if text == "primary pick"
        ));

        terminal.vt_write(b"\x1b]52;c;?\x07");
        assert!(matches!(
            events.try_recv(),
            Err(async_channel::TryRecvError::Empty)
        ));
    }

    #[test]
    fn bel_raises_each_occurrence() {
        let event_state = Arc::new(EventQueueState::new());
        let (event_tx, events) = terminal_event_channel(&event_state);
        let publisher = Publisher {
            event_tx,
            latest: Arc::new(RwLock::new(PublishedViewports::new(
                TerminalViewport::blank(1, 1, SessionStatus::Running),
            ))),
            state: Arc::clone(&event_state),
        };
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        register_bell(&mut terminal, publisher).expect("bell callback");

        terminal.vt_write(b"\x07\x07");
        assert!(matches!(
            events.try_recv().expect("first bell event"),
            TerminalEvent::Bell
        ));
        assert!(matches!(
            events.try_recv().expect("second bell event"),
            TerminalEvent::Bell
        ));
        assert!(matches!(
            events.try_recv(),
            Err(async_channel::TryRecvError::Empty)
        ));
    }

    #[test]
    fn clipboard_writes_prefer_plain_text_and_refuse_oversize_payloads() {
        let html = ClipboardContent {
            mime: "text/html",
            data: "<b>hi</b>",
        };
        let plain = ClipboardContent {
            mime: CLIPBOARD_TEXT_MIME,
            data: "hi",
        };

        assert_eq!(
            clipboard_write_request(ClipboardLocation::Standard, [html, plain].into_iter()),
            Ok((ClipboardTarget::Clipboard, "hi".to_owned()))
        );
        assert_eq!(
            clipboard_write_request(ClipboardLocation::Selection, std::iter::once(html)),
            Ok((ClipboardTarget::Primary, "<b>hi</b>".to_owned()))
        );
        assert_eq!(
            clipboard_write_request(ClipboardLocation::Standard, std::iter::empty()),
            Err(ClipboardWriteError::Unsupported)
        );

        let oversize = "x".repeat(MAX_CLIPBOARD_WRITE_BYTES + 1);
        assert_eq!(
            clipboard_write_request(
                ClipboardLocation::Standard,
                std::iter::once(ClipboardContent {
                    mime: CLIPBOARD_TEXT_MIME,
                    data: &oversize,
                })
            ),
            Err(ClipboardWriteError::Denied)
        );
    }

    #[test]
    fn terminal_response_limit_never_appends_a_partial_sequence() {
        let mut effects = PtyEffects::new();
        effects.bytes.resize(MAX_PTY_RESPONSE_BYTES - 2, b'x');
        effects.push(b"abc");
        assert!(effects.overflowed);
        assert_eq!(effects.bytes.len(), MAX_PTY_RESPONSE_BYTES - 2);
        effects.push(b"z");
        assert_eq!(effects.bytes.len(), MAX_PTY_RESPONSE_BYTES - 2);
    }

    #[test]
    fn unmodified_motion_hovers_only_bound_image_placeholders() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"read [Image #4]");
        let input = TerminalMouseInput::new(
            TerminalMousePhase::Motion,
            None,
            PointerCellEvent {
                column: 10,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            81,
            1,
            320,
            36,
            8,
            18,
            crate::Modifiers::default(),
            false,
        );
        let mut selection = None;
        let mut hover_link = None;
        let mut writer: Box<dyn Write + Send> = Box::new(std::io::sink());
        let mut encoder = mouse::Encoder::new().expect("mouse encoder");
        let mut event = mouse::Event::new().expect("mouse event");
        let mut button_pressed = false;
        let mut scratch = Vec::new();

        let result = route_mouse_input(
            &mut terminal,
            &mut selection,
            &mut hover_link,
            None,
            input,
            &mut writer,
            &mut encoder,
            &mut event,
            &mut button_pressed,
            &mut scratch,
            &WordSeparators::default(),
            &HashSet::from([4]),
        )
        .expect("bound motion");

        assert!(matches!(result, ViewActionResult::OverlaySnapshot));
        assert_eq!(hover_link.expect("bound hover").uri, "zz-image://4");

        let mut hover_link = None;
        let result = route_mouse_input(
            &mut terminal,
            &mut selection,
            &mut hover_link,
            None,
            input,
            &mut writer,
            &mut encoder,
            &mut event,
            &mut button_pressed,
            &mut scratch,
            &WordSeparators::default(),
            &HashSet::new(),
        )
        .expect("unbound motion");
        assert!(matches!(result, ViewActionResult::None));
        assert!(hover_link.is_none());

        let modified = TerminalMouseInput::new(
            TerminalMousePhase::Motion,
            None,
            input.cell,
            81,
            1,
            320,
            36,
            8,
            18,
            crate::Modifiers::new(false, false, true, false),
            false,
        );
        let result = route_mouse_input(
            &mut terminal,
            &mut selection,
            &mut hover_link,
            None,
            modified,
            &mut writer,
            &mut encoder,
            &mut event,
            &mut button_pressed,
            &mut scratch,
            &WordSeparators::default(),
            &HashSet::from([4]),
        )
        .expect("modified motion");
        assert!(matches!(result, ViewActionResult::None));
        assert!(hover_link.is_none());
    }

    #[test]
    fn unmodified_image_hover_survives_alternate_screen_mouse_passthrough() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"\x1b[?1049h\x1b[?1000h\x1b[?1006h[Image #6]");
        let input = TerminalMouseInput::new(
            TerminalMousePhase::Motion,
            None,
            PointerCellEvent {
                column: 5,
                row: 0,
                click_count: 1,
                rectangle: false,
            },
            41,
            1,
            320,
            36,
            8,
            18,
            crate::Modifiers::default(),
            false,
        );
        let mut selection = None;
        let mut hover_link = None;
        let mut writer: Box<dyn Write + Send> = Box::new(std::io::sink());
        let mut encoder = mouse::Encoder::new().expect("mouse encoder");
        let mut event = mouse::Event::new().expect("mouse event");
        let mut button_pressed = false;
        let mut scratch = Vec::new();

        let result = route_mouse_input(
            &mut terminal,
            &mut selection,
            &mut hover_link,
            None,
            input,
            &mut writer,
            &mut encoder,
            &mut event,
            &mut button_pressed,
            &mut scratch,
            &WordSeparators::default(),
            &HashSet::from([6]),
        )
        .expect("mouse-tracking motion");

        assert!(matches!(result, ViewActionResult::OverlaySnapshot));
        assert_eq!(hover_link.expect("alternate hover").uri, "zz-image://6");
    }

    #[test]
    fn pane_reset_scrolls_the_used_rows_into_history_like_the_pinned_send_keys_r() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 6,
            max_scrollback: 1 << 16,
        })
        .expect("terminal");
        for line in 1..=20 {
            terminal.vt_write(format!("L{line:02}\r\n").as_bytes());
        }
        assert_eq!(terminal.scrollback_rows().expect("scrollback"), 15);

        reset_pane_screen(&mut terminal).expect("reset");

        assert_eq!(
            terminal.scrollback_rows().expect("scrollback"),
            20,
            "pinned tmux answers history_size 15 before send-keys -R and 20 after on this pane"
        );
        assert_eq!(terminal.cursor_x().expect("cursor x"), 0);
        assert_eq!(terminal.cursor_y().expect("cursor y"), 0);
        let history = capture_terminal(
            &terminal,
            None,
            CaptureOptions {
                start: CaptureBoundary::HistoryStart,
                ..CaptureOptions::default()
            },
        )
        .expect("capture history");
        let lines = history.split('\n').collect::<Vec<_>>();
        assert_eq!(lines.first().copied(), Some("L01"));
        assert_eq!(lines.get(19).copied(), Some("L20"));
        assert!(
            lines[20..].iter().all(|line| line.is_empty()),
            "the pin leaves the visible rows blank after -R: {lines:?}"
        );
    }

    #[test]
    fn pane_reset_restores_the_pinned_default_tab_stops() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 6,
            max_scrollback: 1 << 16,
        })
        .expect("terminal");
        terminal.vt_write(b"\x1b[3g\x1b[1;5H\x1bH\x1b[2;1H\t");
        assert_eq!(terminal.cursor_x().expect("cursor x"), 4);

        reset_pane_screen(&mut terminal).expect("reset");
        terminal.vt_write(b"\t");

        assert_eq!(
            terminal.cursor_x().expect("cursor x"),
            8,
            "pinned tmux answers cursor_x 4 with a custom tab stop and 8 after send-keys -R"
        );
    }

    #[test]
    fn pane_reset_clears_the_pane_palette_and_the_active_pen() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 4,
            max_scrollback: 1 << 16,
        })
        .expect("terminal");
        terminal.vt_write(b"\x1b]4;42;rgb:12/34/56\x1b\\\x1b[38;5;42mA");
        let before = snapshot_fixture(&terminal);
        let cell = before.row(0).expect("first row")[0];
        assert_eq!(
            before.style(cell).expect("style").foreground(),
            Color::rgb(0x12, 0x34, 0x56)
        );

        reset_pane_screen(&mut terminal).expect("reset");
        terminal.vt_write(b"B\x1b[38;5;42mC");

        let mut fresh = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 4,
            max_scrollback: 1 << 16,
        })
        .expect("terminal");
        fresh.vt_write(b"B\x1b[38;5;42mC");
        let expected = snapshot_fixture(&fresh);
        let expected_row = expected.row(0).expect("first row");

        let after = snapshot_fixture(&terminal);
        let row = after.row(0).expect("first row");
        assert_eq!(
            after.style(row[0]).expect("style").foreground(),
            expected.style(expected_row[0]).expect("style").foreground(),
            "the pin's input_reset_cell puts the pen back to the default cell"
        );
        assert_eq!(
            after.style(row[1]).expect("style").foreground(),
            expected.style(expected_row[1]).expect("style").foreground(),
            "the pin's colour_palette_clear drops the pane's OSC 4 overrides"
        );
        assert_ne!(
            after.style(row[1]).expect("style").foreground(),
            Color::rgb(0x12, 0x34, 0x56)
        );
    }

    #[test]
    fn pane_reset_drops_the_scroll_region_and_abandons_a_partial_sequence() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 6,
            max_scrollback: 1 << 16,
        })
        .expect("terminal");
        terminal.vt_write(b"one\r\ntwo\r\n\x1b[2;4r\x1bPtmux;partial");
        let before = terminal.scrollback_rows().expect("scrollback");

        reset_pane_screen(&mut terminal).expect("reset");
        terminal.vt_write(b"after");

        assert_eq!(
            terminal.scrollback_rows().expect("scrollback"),
            before + 2,
            "the pin's screen_write_scrollregion restores the full region before the clear scrolls"
        );
        let visible =
            capture_terminal(&terminal, None, CaptureOptions::default()).expect("capture visible");
        assert_eq!(
            visible, "after",
            "the pin's input_reset abandons the half-parsed sequence and returns to ground"
        );
    }

    #[test]
    fn pane_reset_drops_the_modes_the_pin_clears_with_send_keys_r() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 4,
            max_scrollback: 1 << 16,
        })
        .expect("terminal");
        let mut key_encoder = key::Encoder::new().expect("key encoder");
        let mut key_event = key::Event::new().expect("key event");
        let mut writer: Box<dyn Write + Send> = Box::new(std::io::sink());
        let up = |terminal: &Terminal<'_, '_>,
                  key_encoder: &mut key::Encoder<'_>,
                  key_event: &mut key::Event<'_>,
                  writer: &mut Box<dyn Write + Send>,
                  key: KeyCode| {
            let mut input_bytes = Vec::new();
            encode_key(
                terminal,
                key_encoder,
                key_event,
                KeyInput {
                    action: KeyAction::Press,
                    key,
                    modifiers: crate::Modifiers::default(),
                    text: None,
                    unshifted_codepoint: None,
                },
                Some(0x7f),
                writer.as_mut(),
                &mut input_bytes,
            )
            .expect("encode key");
            input_bytes
        };

        terminal.vt_write(b"\x1b[?1h\x1b[4h\x1b[?6h");
        assert_eq!(
            up(
                &terminal,
                &mut key_encoder,
                &mut key_event,
                &mut writer,
                KeyCode::ArrowUp
            ),
            b"\x1bOA"
        );
        terminal.vt_write(b"\x1b[=1u");
        assert_eq!(
            up(
                &terminal,
                &mut key_encoder,
                &mut key_event,
                &mut writer,
                KeyCode::Escape
            ),
            b"\x1b[27u"
        );

        reset_pane_screen(&mut terminal).expect("reset");

        assert_eq!(
            up(
                &terminal,
                &mut key_encoder,
                &mut key_event,
                &mut writer,
                KeyCode::ArrowUp
            ),
            b"\x1b[A",
            "pinned tmux answers keypad_cursor_flag 1 before send-keys -R and 0 after, and Up then echoes ^[[A"
        );
        assert_eq!(
            up(
                &terminal,
                &mut key_encoder,
                &mut key_event,
                &mut writer,
                KeyCode::Escape
            ),
            b"\x1b",
            "the pin's mode reset drops MODE_KEYS_EXTENDED with the rest"
        );
        terminal.vt_write(b"ab\rX");
        let visible =
            capture_terminal(&terminal, None, CaptureOptions::default()).expect("capture visible");
        assert_eq!(
            visible, "Xb",
            "pinned tmux answers insert_flag 1 before send-keys -R and 0 after, so X overwrites a"
        );
        terminal.vt_write(b"\x1b[3;5H\x1b8Y");
        assert_eq!(terminal.cursor_x().expect("cursor x"), 1);
        assert_eq!(
            terminal.cursor_y().expect("cursor y"),
            0,
            "the pin's input_reset_cell puts the saved cursor back at 0,0"
        );
    }

    #[test]
    fn backspace_encodes_the_pinned_default_erase_byte() {
        let terminal = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 24,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut key_encoder = key::Encoder::new().expect("key encoder");
        let mut key_event = key::Event::new().expect("key event");
        let mut writer: Box<dyn Write + Send> = Box::new(std::io::sink());
        let mut input_bytes = Vec::new();

        encode_key(
            &terminal,
            &mut key_encoder,
            &mut key_event,
            KeyInput {
                action: KeyAction::Press,
                key: KeyCode::Backspace,
                modifiers: crate::Modifiers::default(),
                text: None,
                unshifted_codepoint: None,
            },
            Some(0x7f),
            &mut writer,
            &mut input_bytes,
        )
        .expect("encode backspace");

        assert_eq!(
            input_bytes.as_slice(),
            b"\x7f",
            "pinned tmux writes 0x7f for send-keys BSpace while the backspace option holds its C-? default"
        );
    }

    #[test]
    fn clearing_the_whole_screen_keeps_history_where_the_pin_scrolls_into_it() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 5,
            max_scrollback: 1 << 16,
        })
        .expect("terminal");
        for line in 1..=12 {
            terminal.vt_write(format!("L{line:02}\r\n").as_bytes());
        }
        assert_eq!(terminal.scrollback_rows().expect("scrollback"), 8);

        terminal.vt_write(b"\x1b[H\x1b[2J");

        assert_eq!(
            terminal.scrollback_rows().expect("scrollback"),
            8,
            "pinned tmux answers history_size 12 here with scroll-on-clear on and 8 with it off; zz is unconditionally the off case"
        );
    }

    #[test]
    fn key_and_mouse_encoding_reuse_the_actor_scratch_buffer() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 24,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"\x1b[?1000h\x1b[?1006h");

        let mut key_encoder = key::Encoder::new().expect("key encoder");
        let mut key_event = key::Event::new().expect("key event");
        let mut mouse_encoder = mouse::Encoder::new().expect("mouse encoder");
        let mut mouse_event = mouse::Event::new().expect("mouse event");
        let mut writer: Box<dyn Write + Send> = Box::new(std::io::sink());
        let mut input_bytes = Vec::with_capacity(LINK_URI_SCRATCH_BYTES);
        let mut selection = None;
        let mut hover_link = None;
        let mut button_pressed = false;
        let word_separators = WordSeparators::default();

        encode_key(
            &terminal,
            &mut key_encoder,
            &mut key_event,
            KeyInput {
                action: KeyAction::Press,
                key: KeyCode::ArrowUp,
                modifiers: crate::Modifiers::default(),
                text: None,
                unshifted_codepoint: None,
            },
            Some(0x7f),
            &mut writer,
            &mut input_bytes,
        )
        .expect("warm key encoding");
        route_mouse_input(
            &mut terminal,
            &mut selection,
            &mut hover_link,
            None,
            test_pointer_input(TerminalMousePhase::Press, 0),
            &mut writer,
            &mut mouse_encoder,
            &mut mouse_event,
            &mut button_pressed,
            &mut input_bytes,
            &word_separators,
            &HashSet::new(),
        )
        .expect("warm mouse encoding");

        let allocation = input_bytes.as_ptr();
        let capacity = input_bytes.capacity();
        for column in 1..=128_u16 {
            encode_key(
                &terminal,
                &mut key_encoder,
                &mut key_event,
                KeyInput {
                    action: KeyAction::Press,
                    key: KeyCode::Character('x'),
                    modifiers: crate::Modifiers::default(),
                    text: Some(Box::from("x")),
                    unshifted_codepoint: Some('x'),
                },
                Some(0x7f),
                &mut writer,
                &mut input_bytes,
            )
            .expect("key encoding");
            route_mouse_input(
                &mut terminal,
                &mut selection,
                &mut hover_link,
                None,
                test_pointer_input(TerminalMousePhase::Motion, column),
                &mut writer,
                &mut mouse_encoder,
                &mut mouse_event,
                &mut button_pressed,
                &mut input_bytes,
                &word_separators,
                &HashSet::new(),
            )
            .expect("mouse encoding");
        }

        assert_eq!(input_bytes.as_ptr(), allocation);
        assert_eq!(input_bytes.capacity(), capacity);
    }

    #[test]
    fn boxed_key_text_moves_into_libghostty_without_reallocation() {
        let text = Box::<str>::from("composed λ");
        let allocation = text.as_ptr();
        let mut event = key::Event::new().expect("key event");

        event.set_utf8(Some(text));

        assert_eq!(event.utf8().expect("stored key text").as_ptr(), allocation);
    }

    #[test]
    fn focus_events_only_reach_applications_that_enable_reporting() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        let mut output = Vec::new();
        write_focus_event(&terminal, true, &mut output).expect("disabled focus event");
        assert!(output.is_empty());

        terminal.vt_write(b"\x1b[?1004h");
        write_focus_event(&terminal, true, &mut output).expect("focus gained");
        write_focus_event(&terminal, false, &mut output).expect("focus lost");
        assert_eq!(output, b"\x1b[I\x1b[O");
    }

    #[test]
    fn wheel_routing_prioritizes_application_mouse_then_alternate_scroll() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        assert_eq!(
            wheel_route(&terminal, false).expect("primary route"),
            WheelRoute::Viewport
        );

        terminal.vt_write(b"\x1b[?1007h\x1b[?1049h");
        assert_eq!(
            wheel_route(&terminal, false).expect("alternate route"),
            WheelRoute::AlternateScroll
        );

        terminal.vt_write(b"\x1b[?1000h");
        assert_eq!(
            wheel_route(&terminal, false).expect("application mouse route"),
            WheelRoute::ApplicationMouse
        );
        assert_eq!(
            wheel_route(&terminal, true).expect("forced local route"),
            WheelRoute::Viewport
        );

        terminal.vt_write(b"\x1b[?1000l\x1b[?1007l");
        assert_eq!(
            wheel_route(&terminal, false).expect("disabled alternate scroll"),
            WheelRoute::Viewport
        );
    }

    #[test]
    fn a_grid_without_history_reports_nothing_to_scroll_into() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 3,
            max_scrollback: 32,
        })
        .expect("terminal");
        terminal.vt_write(b"zero\r\none\r\ntwo");
        let scrollbar = terminal.scrollbar().expect("scrollbar");
        assert_eq!(scrollbar.total, scrollbar.len);

        terminal.vt_write(b"\r\nthree\r\nfour");
        let scrollbar = terminal.scrollbar().expect("scrollbar");
        assert!(scrollbar.total > scrollbar.len);

        terminal.vt_write(b"\x1b[?1049h");
        let scrollbar = terminal.scrollbar().expect("alternate scrollbar");
        assert_eq!(scrollbar.total, scrollbar.len);
    }

    #[test]
    fn alternate_scroll_emits_bounded_normal_and_application_cursor_keys() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"\x1b[?1049h");
        let mut output = Vec::new();

        write_alternate_scroll(&terminal, -2, &mut output).expect("normal cursor up");
        write_alternate_scroll(&terminal, 2, &mut output).expect("normal cursor down");
        assert_eq!(output, b"\x1b[A\x1b[A\x1b[B\x1b[B");

        terminal.vt_write(b"\x1b[?1h");
        output.clear();
        write_alternate_scroll(&terminal, -1, &mut output).expect("application cursor up");
        write_alternate_scroll(&terminal, 1, &mut output).expect("application cursor down");
        assert_eq!(output, b"\x1bOA\x1bOB");

        output.clear();
        write_alternate_scroll(&terminal, i32::MIN, &mut output).expect("bounded repeat");
        assert_eq!(
            output.len(),
            usize::try_from(MAX_WHEEL_REPEAT).unwrap_or(0) * 3
        );
        assert!(output.chunks_exact(3).all(|sequence| sequence == b"\x1bOA"));
    }

    #[test]
    fn command_output_view_is_pty_free_reflowable_and_closes() {
        let text = (0..40)
            .map(|line| format!("\x1b[3{}mline {line:02} αβ\x1b[0m", line % 7 + 1))
            .collect::<Vec<_>>()
            .join("\n");
        let session = TerminalSession::spawn_output_view("list-keys".to_owned(), text);
        let events = session.events();
        let view = TerminalViewId(77);
        session.attach_view(view);

        let viewport = wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.mode, TerminalMode::View { .. })
                && matches!(viewport.status, SessionStatus::Running)
        });
        assert_eq!(viewport.title(), "list-keys");
        assert_eq!(viewport.columns, INITIAL_COLUMNS);
        assert_eq!(viewport.rows, INITIAL_ROWS);
        let capture = session
            .capture(CaptureOptions {
                start: CaptureBoundary::HistoryStart,
                end: CaptureBoundary::Relative(i64::MAX),
                mode: true,
                ..CaptureOptions::default()
            })
            .expect("capture frozen command output");
        assert!(capture.contains("line 00 αβ"));
        assert!(capture.contains("line 39 αβ"));
        assert!(!capture.contains("\x1b["));

        session.resize(16, 5, 8, 18);
        let viewport = wait_for_test_viewport(&session, |viewport| {
            viewport.columns == 16
                && viewport.rows == 5
                && matches!(viewport.mode, TerminalMode::View { .. })
        });
        assert!(matches!(
            viewport.mode,
            TerminalMode::View { total, .. } if total > 5
        ));

        session.view_action(
            view,
            TerminalViewAction::SearchBegin(SearchQuery::literal("line 31")),
        );
        let viewport = wait_for_test_viewport(&session, |viewport| {
            viewport
                .search
                .is_some_and(|search| !search.pending() && search.total == 1)
        });
        assert_eq!(viewport.search.expect("completed search").total, 1);

        session.view_action(view, TerminalViewAction::SelectAll);
        session.view_action(
            view,
            TerminalViewAction::CopySelection {
                request_id: 9,
                target: ClipboardTarget::Clipboard,
            },
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            match events.try_recv() {
                Ok(TerminalEvent::CopyReady { copy, .. }) if copy.request_id == 9 => {
                    assert!(copy.text.contains("line 00 αβ"));
                    assert!(copy.text.contains("line 39 αβ"));
                    break;
                }
                Ok(_) | Err(async_channel::TryRecvError::Empty) => {}
                Err(async_channel::TryRecvError::Closed) => {
                    panic!("command-output actor closed before copying")
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for output selection"
            );
            thread::sleep(Duration::from_millis(10));
        }

        session.view_action(view, TerminalViewAction::CopyMode(CopyModeAction::Cancel));
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            match events.try_recv() {
                Ok(TerminalEvent::ViewClosed(_)) => break,
                Ok(_) | Err(async_channel::TryRecvError::Empty) => {}
                Err(async_channel::TryRecvError::Closed) => {
                    panic!("command-output actor closed without a view-close event")
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for output view to close"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn ordinary_and_startup_output_views_use_their_distinct_byte_caps() {
        let ordinary = TerminalSession::spawn_output_view(String::new(), String::new());
        let startup = TerminalSession::spawn_startup_output_view_with_appearance(
            String::new(),
            String::new(),
            Arc::new(TerminalAppearance::default()),
        );

        assert_eq!(ordinary.max_scrollback(), 100_000);
        assert_eq!(startup.max_scrollback(), 64 * 1024 * 1024);
    }

    #[test]
    fn empty_surface_is_live_pty_free_and_ignores_input() {
        let session = TerminalSession::spawn_empty_with_appearance(
            64,
            Arc::new(TerminalAppearance::default()),
        );
        let view = TerminalViewId(91);
        session.attach_view(view);

        let viewport = wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.mode, TerminalMode::Live)
                && matches!(viewport.status, SessionStatus::Running)
        });
        assert_eq!(viewport.columns, INITIAL_COLUMNS);
        assert_eq!(viewport.rows, INITIAL_ROWS);
        assert_eq!(session.process_id(), None);
        assert_eq!(session.foreground_process_id(), None);
        assert_eq!(session.tty(), None);

        session.send_text("ZZ_EMPTY_MUST_STAY_BLANK");
        let capture = session
            .capture(CaptureOptions::default())
            .expect("capture empty surface");
        assert!(!capture.contains("ZZ_EMPTY_MUST_STAY_BLANK"));

        session.resize(23, 7, 8, 18);
        let viewport = wait_for_test_viewport(&session, |viewport| {
            viewport.columns == 23
                && viewport.rows == 7
                && matches!(viewport.mode, TerminalMode::Live)
        });
        assert_eq!(viewport.status, SessionStatus::Running);
    }

    #[test]
    fn live_appearance_update_preserves_command_output_and_resets_colors() {
        let mut initial = TerminalAppearance {
            background: Color::rgb(0x11, 0x22, 0x33),
            ..TerminalAppearance::default()
        };
        initial.palette[3] = Color::rgb(0x44, 0x55, 0x66);
        let session = TerminalSession::spawn_output_view_with_appearance(
            "appearance".to_owned(),
            "preserved content".to_owned(),
            Arc::new(initial.clone()),
        );
        session.attach_view(TerminalViewId(93));
        let before = wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.status, SessionStatus::Running)
                && viewport.background == initial.background
        });
        let dictionary_generation = before.dictionary_generation;

        let mut next = initial;
        next.color_scheme = TerminalColorScheme::Light;
        next.background = Color::rgb(0xee, 0xdd, 0xcc);
        next.palette[3] = Color::rgb(0xaa, 0xbb, 0xcc);
        session.set_appearance(Arc::new(next.clone()));
        let after = wait_for_test_viewport(&session, |viewport| {
            viewport.background == next.background
                && viewport.dictionary_generation != dictionary_generation
        });
        assert_eq!(after.background, next.background);
        let captured = session
            .capture(CaptureOptions {
                start: CaptureBoundary::HistoryStart,
                end: CaptureBoundary::Relative(i64::MAX),
                mode: true,
                ..CaptureOptions::default()
            })
            .expect("capture after appearance update");
        assert!(captured.contains("preserved content"));
    }

    fn wait_for_test_viewport(
        session: &TerminalSession,
        mut predicate: impl FnMut(&TerminalViewport) -> bool,
    ) -> Arc<TerminalViewport> {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let viewport = session.latest_viewport();
            if predicate(&viewport) {
                return viewport;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for terminal viewport; last title: {:?}, mode: {:?}, background: {:?}, dictionary_generation: {}, status: {:?}",
                viewport.title(),
                viewport.mode,
                viewport.background,
                viewport.dictionary_generation,
                viewport.status,
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn history_row_text(row: &[PackedCell], dictionary: &TerminalDictionary) -> String {
        let mut output = String::new();
        for cell in row {
            let glyph = cell.glyph();
            if glyph == 0 {
                continue;
            }
            if glyph & GRAPHEME_TABLE_BIT == 0 {
                if let Some(character) = char::from_u32(glyph) {
                    output.push(character);
                }
                continue;
            }
            let index = usize::try_from(glyph & !GRAPHEME_TABLE_BIT).unwrap_or(usize::MAX);
            let Some((&start, &end)) = dictionary
                .grapheme_offsets
                .get(index)
                .zip(dictionary.grapheme_offsets.get(index.saturating_add(1)))
            else {
                continue;
            };
            let Some(bytes) = dictionary.grapheme_bytes.get(start as usize..end as usize) else {
                continue;
            };
            if let Ok(grapheme) = std::str::from_utf8(bytes) {
                output.push_str(grapheme);
            }
        }
        output
    }

    #[cfg(unix)]
    #[test]
    fn history_api_is_clamped_self_contained_and_does_not_move_a_view() {
        let session = TerminalSession::spawn(
            128,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                command: Some(vec![
                    "read _; i=0; while [ $i -lt 20 ]; do printf 'ZZH%02d\\n' \"$i\"; i=$((i+1)); done; printf 'ZZ_HISTORY_DONE\\n'; read _"
                        .to_owned(),
                ]),
                ..TerminalSpawn::default()
            },
        );
        let view = TerminalViewId(96);
        session.resize(24, 4, 8, 18);
        session.attach_view(view);
        wait_for_test_viewport(&session, |viewport| {
            viewport.columns == 24
                && viewport.rows == 4
                && matches!(viewport.status, SessionStatus::Running)
        });
        session.send_text("ready\n");
        wait_for_test_viewport(&session, |viewport| {
            let mut contents = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut contents);
            }
            contents.contains("ZZ_HISTORY_DONE")
                && viewport.scrollbar.total > viewport.scrollbar.len
        });

        session.view_action(view, TerminalViewAction::ScrollTop);
        let before = wait_for_test_viewport(&session, |viewport| viewport.scrollbar.offset == 0);
        let before_generation = (before.generation, before.view_generation, before.scrollbar);

        let (start, rows, dictionary, scrollbar, columns) =
            session.history(0, u32::MAX).expect("history");
        assert_eq!(start, 0);
        assert_eq!(columns, 24);
        assert_eq!(
            rows.len(),
            usize::try_from(scrollbar.total.saturating_sub(scrollbar.len))
                .expect("small fixture history")
        );
        assert!(rows.iter().all(|row| row.len() == usize::from(columns)));
        let history_text = rows
            .iter()
            .map(|row| history_row_text(row, &dictionary))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(history_text.contains("ZZH"), "{history_text:?}");

        let (clamped_start, clamped, _, clamped_scrollbar, _) =
            session.history(u32::MAX, 10).expect("clamped history");
        assert_eq!(
            clamped_start,
            clamped_scrollbar
                .total
                .saturating_sub(clamped_scrollbar.len)
        );
        assert!(clamped.is_empty());

        let after = session
            .latest_viewport_for(view)
            .expect("attached view remains published");
        assert_eq!(
            (after.generation, after.view_generation, after.scrollbar),
            before_generation
        );
    }

    #[test]
    fn output_view_history_is_empty() {
        let session =
            TerminalSession::spawn_output_view("history".to_owned(), "one\ntwo".to_owned());
        let (_, rows, _, _, _) = session.history(0, 512).expect("empty output history");
        assert!(rows.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn live_shell_starts_in_the_requested_working_directory() {
        let directory = tempfile::tempdir().expect("temporary working directory");
        let expected = directory
            .path()
            .canonicalize()
            .expect("canonical working directory");
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                working_directory: Some(expected.clone()),
                ..TerminalSpawn::default()
            },
        );
        let view = TerminalViewId(43);
        session.attach_view(view);
        session.send_text("printf 'ZZ_WORKING_DIRECTORY:'; /bin/pwd -P\n");

        let expected = expected.to_string_lossy();
        let viewport = wait_for_test_viewport(&session, |viewport| {
            let mut contents = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut contents);
            }
            contents.contains("ZZ_WORKING_DIRECTORY:") && contents.contains(expected.as_ref())
        });
        assert!(matches!(viewport.status, SessionStatus::Running));
        assert!(session.foreground_process_id().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn live_command_applies_spawn_environment_additions_and_removals() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                command: Some(vec![
                    "read _; printf 'ZZ_PHASE4D_SET=%s\\n' \"$ZZ_PHASE4D_SET\"; if [ \"${ZZ_PHASE4D_REMOVE+x}\" = x ]; then printf 'ZZ_PHASE4D_REMOVE=set\\n'; else printf 'ZZ_PHASE4D_REMOVE=unset\\n'; fi; read _"
                        .to_owned(),
                ]),
                env: vec![
                    ("ZZ_PHASE4D_SET".into(), Some("session".into())),
                    (
                        "ZZ_PHASE4D_REMOVE".into(),
                        Some("daemon".into()),
                    ),
                    ("ZZ_PHASE4D_REMOVE".into(), None),
                ],
                ..TerminalSpawn::default()
            },
        );
        session.attach_view(TerminalViewId(47));
        wait_for_test_viewport(&session, |viewport| {
            matches!(viewport.status, SessionStatus::Running)
        });
        session.send_text("ready\n");

        let viewport = wait_for_test_viewport(&session, |viewport| {
            let mut contents = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut contents);
            }
            contents.contains("ZZ_PHASE4D_REMOVE")
        });
        let mut contents = String::new();
        for cell in viewport.cells.iter() {
            viewport.push_glyph(*cell, &mut contents);
        }
        assert!(contents.contains("ZZ_PHASE4D_SET=session"), "{contents:?}");
        assert!(contents.contains("ZZ_PHASE4D_REMOVE=unset"), "{contents:?}");
    }

    #[cfg(unix)]
    #[test]
    fn terminal_type_seeds_term_and_spawn_environment_can_override_it() {
        let capture =
            |view: u64,
             terminal_type: Option<&str>,
             env: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>| {
                let session = TerminalSession::spawn(
                    DEFAULT_HISTORY_LIMIT,
                    Arc::new(TerminalAppearance::default()),
                    TerminalSpawn {
                        command: Some(vec!["printf 'ZZ_TERM:%s\\n' \"$TERM\"; read _".to_owned()]),
                        terminal_type: terminal_type.map(str::to_owned),
                        env,
                        ..TerminalSpawn::default()
                    },
                );
                session.attach_view(TerminalViewId(view));
                let viewport = wait_for_test_viewport(&session, |viewport| {
                    let mut contents = String::new();
                    for cell in viewport.cells.iter() {
                        viewport.push_glyph(*cell, &mut contents);
                    }
                    contents.contains("ZZ_TERM:")
                });
                let mut contents = String::new();
                for cell in viewport.cells.iter() {
                    viewport.push_glyph(*cell, &mut contents);
                }
                contents
            };

        assert!(capture(47, None, Vec::new()).contains("ZZ_TERM:tmux-256color"));
        assert!(capture(48, Some("zz-term"), Vec::new()).contains("ZZ_TERM:zz-term"));
        assert!(
            capture(
                49,
                Some("zz-term"),
                vec![("TERM".into(), Some("override".into()))]
            )
            .contains("ZZ_TERM:override")
        );
    }

    #[cfg(unix)]
    #[test]
    fn shell_integration_updates_title_and_working_directory() {
        let shell = std::env::var("SHELL").unwrap_or_default();
        let shell_name = std::path::Path::new(&shell)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .map(ToOwned::to_owned);
        if !matches!(shell_name.as_deref(), Some("bash" | "zsh"))
            || cfg!(target_os = "macos") && shell == "/bin/bash"
        {
            return;
        }

        let temporary = tempfile::tempdir().expect("temporary working directory");
        let directory = temporary.path().join("title fixture");
        std::fs::create_dir(&directory).expect("working directory with spaces");
        let expected_directory = directory
            .canonicalize()
            .expect("canonical working directory");
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance {
                cursor_style: CursorStyle::Underline,
                ..TerminalAppearance::default()
            }),
            TerminalSpawn {
                working_directory: Some(expected_directory.clone()),
                ..TerminalSpawn::default()
            },
        );
        session.attach_view(TerminalViewId(44));

        let expected_directory = expected_directory.to_string_lossy();
        wait_for_test_viewport(&session, |viewport| {
            viewport.title() == shell_name.as_deref().unwrap_or_default()
                && viewport
                    .cursor
                    .is_some_and(|cursor| cursor.style() == CursorStyle::Underline)
                && viewport
                    .working_directory()
                    .is_some_and(|working_directory| {
                        working_directory.ends_with(expected_directory.as_ref())
                    })
        });
        session.send_text("sleep 1\n");
        wait_for_test_viewport(&session, |viewport| {
            viewport.title() == "sleep 1"
                && viewport
                    .cursor
                    .is_some_and(|cursor| cursor.style() == CursorStyle::Underline)
                && viewport
                    .working_directory()
                    .is_some_and(|working_directory| {
                        working_directory.ends_with(expected_directory.as_ref())
                    })
        });
        let viewport = wait_for_test_viewport(&session, |viewport| {
            viewport.title() == shell_name.as_deref().unwrap_or_default()
                && viewport
                    .cursor
                    .is_some_and(|cursor| cursor.style() == CursorStyle::Underline)
                && viewport
                    .working_directory()
                    .is_some_and(|working_directory| {
                        working_directory.ends_with(expected_directory.as_ref())
                    })
        });
        assert!(matches!(viewport.status, SessionStatus::Running));
    }

    #[cfg(unix)]
    #[test]
    fn prepared_paste_reaches_a_pty_while_its_client_view_is_hidden() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn::default(),
        );
        let view = TerminalViewId(42);
        session.paste_prepared_bytes(
            Some(view),
            Arc::from(b"printf 'ZZ_HIDDEN_PASTE_OK\\n'\r".as_slice()),
            false,
        );
        session.attach_view(view);

        let viewport = wait_for_test_viewport(&session, |viewport| {
            let mut contents = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut contents);
            }
            contents.contains("ZZ_HIDDEN_PASTE_OK")
        });
        assert!(matches!(viewport.status, SessionStatus::Running));
    }

    #[cfg(unix)]
    #[test]
    fn live_session_decodes_and_places_a_png_transmission() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn::default(),
        );
        let view = TerminalViewId(11);
        session.attach_view(view);
        session.resize(100, 30, 8, 18);
        session.send_text(
            "printf '\\033_Ga=T,f=100,i=78;iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91Jpz\
             AAAAEklEQVR4nGP4z8DAAMIM/4EAAB/uBfsL2WiLAAAAAElFTkSuQmCC\\033\\\\'\n",
        );

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            while session.events().try_recv().is_ok() {}
            let placed = session.latest_viewport_for(view).is_some_and(|viewport| {
                viewport
                    .kitty_placements
                    .iter()
                    .any(|placement| placement.image_id == 78)
            });
            if placed {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "PNG transmission never produced a Kitty placement"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn live_session_publishes_kitty_placements() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn::default(),
        );
        let view = TerminalViewId(9);
        session.attach_view(view);
        session.resize(100, 30, 8, 18);
        session.send_text("printf '\\033_Ga=T,f=24,s=1,v=1,i=77;/wAA\\033\\\\'\n");

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            while session.events().try_recv().is_ok() {}
            let placed = session.latest_viewport_for(view).is_some_and(|viewport| {
                viewport
                    .kitty_placements
                    .iter()
                    .any(|placement| placement.image_id == 77)
            });
            if placed {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "live session never published a Kitty placement"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// A popup is spawned at the size it wants and nobody ever resizes it, so
    /// the placements a popup publishes have to be there before the first
    /// `Command::Resize`. `Terminal::new` takes no cell pixel geometry, and the
    /// Kitty placement's render info is measured in cell pixels, so a session
    /// that never resized reported no placement at all.
    #[cfg(unix)]
    #[test]
    fn a_session_that_is_never_resized_still_publishes_kitty_placements() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn {
                initial_size: Some(TerminalSize {
                    columns: 38,
                    rows: 12,
                    cell_width_px: 8,
                    cell_height_px: 18,
                }),
                ..TerminalSpawn::default()
            },
        );
        let view = TerminalViewId(21);
        session.attach_view(view);
        session.send_text("printf '\\033_Ga=T,f=24,s=1,v=1,i=77;/wAA\\033\\\\'\n");

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            while session.events().try_recv().is_ok() {}
            let placed = session.latest_viewport_for(view).is_some_and(|viewport| {
                viewport
                    .kitty_placements
                    .iter()
                    .any(|placement| placement.image_id == 77)
            });
            if placed {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "unresized session never published a Kitty placement"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn live_shell_round_trip_reaches_render_snapshot() {
        let session = TerminalSession::spawn(
            DEFAULT_HISTORY_LIMIT,
            Arc::new(TerminalAppearance::default()),
            TerminalSpawn::default(),
        );
        let view = TerminalViewId(42);
        session.attach_view(view);
        session.send_text("printf 'ZZ_TERMINAL_ROUND_TRIP\\n'\n");

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            while session.events().try_recv().is_ok() {}
            let viewport = session.latest_viewport();
            let mut contents = String::new();
            for cell in viewport.cells.iter() {
                viewport.push_glyph(*cell, &mut contents);
            }
            if contents.contains("ZZ_TERMINAL_ROUND_TRIP") {
                assert!(matches!(viewport.status, SessionStatus::Running));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "shell output never reached a terminal snapshot; last status: {:?}",
                viewport.status
            );
            thread::sleep(Duration::from_millis(20));
        }

        session.view_action(
            view,
            TerminalViewAction::SearchUpdate(SearchQuery {
                text: "ZZ_TERMINAL_[A-Z_]+".to_owned(),
                mode: SearchMode::Regex,
                case: SearchCase::Sensitive,
                direction: SearchDirection::Forward,
            }),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            while session.events().try_recv().is_ok() {}
            let viewport = session.latest_viewport();
            if viewport.search.is_some_and(|search| {
                !search.pending() && !search.invalid_pattern() && search.total > 0
            }) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "background regex search never completed: {:?}",
                viewport.search
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn selection_lifecycle_fixture(
        text: &[u8],
        cols: u16,
        rows: u16,
    ) -> (Option<SelectionState>, CopyModeSlot) {
        let mut terminal = Terminal::new(TerminalOptions {
            cols,
            rows,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(text);
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        (selection, copy_mode)
    }

    fn run_copy_actions(
        text: &[u8],
        cols: u16,
        rows: u16,
        start: PointCoordinate,
        actions: &[CopyModeAction],
    ) -> CopyModeState {
        let mut terminal = Terminal::new(TerminalOptions {
            cols,
            rows,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(text);
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let row = copy_mode.as_ref().expect("mode").viewport_offset;
        copy_mode.as_mut().expect("mode").cursor = PointCoordinate {
            x: start.x,
            y: row.saturating_add(start.y),
        };
        let mut unseen_output = 0;
        for action in actions {
            apply_copy_mode_action(
                &mut terminal,
                &mut selection,
                &mut copy_mode,
                &mut unseen_output,
                action.clone(),
                &WordSeparators::default(),
                false,
            )
            .expect("copy action");
        }
        *copy_mode.expect("mode survives the sequence")
    }

    #[test]
    fn selection_mode_word_widens_the_live_selection_to_whole_words() {
        let forward = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 2, y: 0 },
            &[
                CopyModeAction::StartSelection,
                CopyModeAction::SelectionMode(CopySelectionMode::Word),
                CopyModeAction::Right,
                CopyModeAction::Right,
                CopyModeAction::Right,
                CopyModeAction::Right,
                CopyModeAction::Right,
            ],
        );
        let row = forward.cursor.y;
        let selection = forward.selection.expect("selection");
        assert_eq!(forward.cursor.x, 7);
        assert_eq!(selection.anchor, PointCoordinate { x: 0, y: row });
        assert_eq!(selection.focus, PointCoordinate { x: 9, y: row });
        assert_eq!(selection.mode, SelectionMode::Word);

        let backward = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 7, y: 0 },
            &[
                CopyModeAction::StartSelection,
                CopyModeAction::SelectionMode(CopySelectionMode::Word),
                CopyModeAction::Left,
                CopyModeAction::Left,
                CopyModeAction::Left,
                CopyModeAction::Left,
                CopyModeAction::Left,
            ],
        );
        let row = backward.cursor.y;
        let selection = backward.selection.expect("selection");
        assert_eq!(backward.cursor.x, 2);
        assert_eq!(selection.anchor, PointCoordinate { x: 9, y: row });
        assert_eq!(selection.focus, PointCoordinate { x: 0, y: row });
    }

    #[test]
    fn word_selection_resolves_a_whitespace_cursor_outward_like_the_pin() {
        let forward = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 7, y: 0 },
            &[CopyModeAction::SelectWord, CopyModeAction::Right],
        );
        let row = forward.cursor.y;
        let selection = forward.selection.expect("selection");
        assert_eq!(forward.cursor.x, 10);
        assert_eq!(selection.anchor, PointCoordinate { x: 6, y: row });
        assert_eq!(selection.focus, PointCoordinate { x: 15, y: row });

        let backward = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 7, y: 0 },
            &[
                CopyModeAction::SelectWord,
                CopyModeAction::Left,
                CopyModeAction::Left,
                CopyModeAction::Left,
                CopyModeAction::Left,
            ],
        );
        let row = backward.cursor.y;
        let selection = backward.selection.expect("selection");
        assert_eq!(backward.cursor.x, 5);
        assert_eq!(selection.anchor, PointCoordinate { x: 9, y: row });
        assert_eq!(selection.focus, PointCoordinate { x: 0, y: row });
    }

    #[test]
    fn selection_mode_line_widens_the_live_selection_to_whole_lines() {
        let mode = run_copy_actions(
            b"one\r\ntwo\r\nthree",
            10,
            3,
            PointCoordinate { x: 1, y: 0 },
            &[
                CopyModeAction::StartSelection,
                CopyModeAction::SelectionMode(CopySelectionMode::Line),
                CopyModeAction::Down,
            ],
        );
        let selection = mode.selection.expect("selection");
        let origin = mode.cursor.y.saturating_sub(1);
        assert_eq!(selection.anchor, PointCoordinate { x: 0, y: origin });
        assert_eq!(
            selection.focus,
            PointCoordinate {
                x: mode.revision.columns - 1,
                y: mode.cursor.y,
            }
        );
        assert_eq!(selection.mode, SelectionMode::Line);
    }

    #[test]
    fn begin_selection_and_other_end_reset_the_selection_unit_to_characters() {
        let mode = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 2, y: 0 },
            &[
                CopyModeAction::SelectionMode(CopySelectionMode::Word),
                CopyModeAction::StartSelection,
                CopyModeAction::Right,
                CopyModeAction::Right,
            ],
        );
        let row = mode.cursor.y;
        let selection = mode.selection.expect("selection");
        assert_eq!(selection.anchor, PointCoordinate { x: 2, y: row });
        assert_eq!(selection.focus, PointCoordinate { x: 4, y: row });
        assert_eq!(selection.mode, SelectionMode::Cell);

        let swapped = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 2, y: 0 },
            &[
                CopyModeAction::SelectWord,
                CopyModeAction::OtherEnd,
                CopyModeAction::Right,
            ],
        );
        let row = swapped.cursor.y;
        let selection = swapped.selection.expect("selection");
        assert_eq!(selection.anchor, PointCoordinate { x: 4, y: row });
        assert_eq!(selection.focus, PointCoordinate { x: 1, y: row });
    }

    #[test]
    fn select_word_and_select_line_arm_their_own_selection_units() {
        let word = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 7, y: 0 },
            &[CopyModeAction::SelectWord],
        );
        let row = word.cursor.y;
        let selection = word.selection.expect("selection");
        assert_eq!(word.cursor, PointCoordinate { x: 9, y: row });
        assert_eq!(selection.anchor, PointCoordinate { x: 6, y: row });
        assert_eq!(selection.focus, PointCoordinate { x: 9, y: row });

        let line = run_copy_actions(
            b"one\r\ntwo\r\nthree",
            10,
            3,
            PointCoordinate { x: 1, y: 0 },
            &[CopyModeAction::SelectLine, CopyModeAction::Down],
        );
        let selection = line.selection.expect("selection");
        assert_eq!(selection.mode, SelectionMode::Line);
        assert_eq!(
            selection.focus,
            PointCoordinate {
                x: line.revision.columns - 1,
                y: line.cursor.y,
            }
        );
    }

    #[test]
    fn stop_selection_freezes_the_range_while_clear_selection_drops_it() {
        let stopped = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 2, y: 0 },
            &[
                CopyModeAction::StartSelection,
                CopyModeAction::Right,
                CopyModeAction::StopSelection,
                CopyModeAction::Right,
                CopyModeAction::Right,
            ],
        );
        let row = stopped.cursor.y;
        let selection = stopped
            .selection
            .expect("selection outlives stop-selection");
        assert!(!stopped.selecting);
        assert_eq!(stopped.cursor.x, 5);
        assert_eq!(selection.anchor, PointCoordinate { x: 2, y: row });
        assert_eq!(selection.focus, PointCoordinate { x: 3, y: row });

        let cleared = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 2, y: 0 },
            &[
                CopyModeAction::StartSelection,
                CopyModeAction::Right,
                CopyModeAction::ClearSelection,
            ],
        );
        assert!(cleared.selection.is_none());
        assert!(!cleared.selecting);
    }

    #[test]
    fn rectangle_toggles_keep_the_live_selection_and_survive_entry_defaults() {
        let (_, copy_mode) = selection_lifecycle_fixture(b"alpha beta", 20, 2);
        let mode = copy_mode.expect("mode");
        assert!(!mode.rectangle);
        assert_eq!(mode.selection_mode, CopySelectionMode::Char);

        let toggled = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 2, y: 0 },
            &[
                CopyModeAction::StartSelection,
                CopyModeAction::Right,
                CopyModeAction::ToggleRectangle,
            ],
        );
        assert!(toggled.rectangle);
        let selection = toggled.selection.expect("selection");
        assert!(selection.rectangle);
        assert!(toggled.selecting);

        let off = run_copy_actions(
            b"alpha beta gamma",
            20,
            2,
            PointCoordinate { x: 2, y: 0 },
            &[
                CopyModeAction::StartSelection,
                CopyModeAction::RectangleOn,
                CopyModeAction::RectangleOff,
            ],
        );
        assert!(!off.rectangle);
        assert!(!off.selection.expect("selection").rectangle);
    }

    fn copy_mode_after_counted_action(
        action: CopyModeAction,
        count: u32,
        starts_at_bottom: bool,
    ) -> Option<Box<CopyModeState>> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        {
            let mode = copy_mode.as_mut().expect("mode");
            let maximum = mode.revision.maximum_offset();
            assert!(maximum > 1);
            if !starts_at_bottom {
                mode.viewport_offset = maximum - 1;
                let page = u32::from(mode.revision.viewport_rows.max(1));
                mode.cursor.y = mode
                    .viewport_offset
                    .saturating_add(page.saturating_sub(1))
                    .min(mode.revision.total_rows().saturating_sub(1));
            }
        }
        let mut unseen_output = 0;
        apply_counted_copy_mode_action(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            &mut unseen_output,
            action,
            count,
            &WordSeparators::default(),
            false,
        )
        .expect("counted copy action");
        copy_mode
    }

    #[test]
    fn scroll_exit_toggles_rearm_the_latch_taken_at_copy_mode_entry() {
        for (action, expected) in [
            (CopyModeAction::ScrollExitOn, true),
            (CopyModeAction::ScrollExitOff, false),
            (CopyModeAction::ScrollExitToggle, true),
        ] {
            let mode = run_copy_actions(
                b"alpha beta",
                20,
                2,
                PointCoordinate { x: 0, y: 0 },
                std::slice::from_ref(&action),
            );
            assert_eq!(mode.scroll_exit, expected, "{action:?}");
        }
        let toggled_twice = run_copy_actions(
            b"alpha beta",
            20,
            2,
            PointCoordinate { x: 0, y: 0 },
            &[
                CopyModeAction::ScrollExitToggle,
                CopyModeAction::ScrollExitToggle,
            ],
        );
        assert!(!toggled_twice.scroll_exit);
    }

    #[test]
    fn a_rearmed_scroll_exit_latch_drives_the_plain_downward_actions() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 2,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"zero\r\none\r\ntwo\r\nthree");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mut unseen_output = 0;
        for action in [CopyModeAction::ScrollExitOn, CopyModeAction::PageDown] {
            apply_copy_mode_action(
                &mut terminal,
                &mut selection,
                &mut copy_mode,
                &mut unseen_output,
                action,
                &WordSeparators::default(),
                false,
            )
            .expect("copy action");
        }
        assert!(copy_mode.is_none());
    }

    #[test]
    fn the_and_cancel_actions_exit_without_the_scroll_exit_latch() {
        for action in [
            CopyModeAction::PageDownScrollExit,
            CopyModeAction::HalfPageDownScrollExit,
            CopyModeAction::ScrollDownAndCancel,
            CopyModeAction::CursorDownAndCancel,
        ] {
            assert!(
                copy_mode_after_counted_action(action.clone(), 1, true).is_none(),
                "{action:?} at bottom"
            );
        }
    }

    #[test]
    fn scroll_down_and_cancel_ignores_a_live_selection_but_page_down_and_cancel_does_not() {
        assert!(
            copy_mode_survives_downward_action(
                CopyModeAction::PageDownScrollExit,
                false,
                true,
                true
            ),
            "page-down-and-cancel keeps a selected mode"
        );
        assert!(
            !copy_mode_survives_downward_action(
                CopyModeAction::ScrollDownAndCancel,
                false,
                true,
                true
            ),
            "scroll-down-and-cancel exits with a selection"
        );
    }

    #[test]
    fn counted_cursor_down_and_cancel_only_exits_when_the_whole_run_was_stuck() {
        assert!(
            copy_mode_after_counted_action(CopyModeAction::CursorDownAndCancel, 3, false).is_some(),
            "a run that moves keeps copy mode"
        );
        assert!(
            copy_mode_after_counted_action(CopyModeAction::CursorDownAndCancel, 3, true).is_none(),
            "a run stuck at the bottom exits"
        );
    }

    #[test]
    fn cursor_centre_actions_park_the_cursor_mid_view_and_mid_row() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 9,
            rows: 4,
            max_scrollback: 16,
        })
        .expect("terminal");
        terminal.vt_write(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mode = copy_mode.as_mut().expect("mode");
        let viewport = mode.viewport_offset;
        mode.cursor = PointCoordinate { x: 1, y: viewport };
        move_copy_cursor(
            mode,
            &CopyModeAction::CursorCentreVertical,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(
            mode.cursor,
            PointCoordinate {
                x: 1,
                y: viewport + 2,
            }
        );
        assert_eq!(mode.viewport_offset, viewport);
        move_copy_cursor(
            mode,
            &CopyModeAction::CursorCentreHorizontal,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(
            mode.cursor,
            PointCoordinate {
                x: 4,
                y: viewport + 2,
            }
        );
        mode.cursor = PointCoordinate { x: 7, y: viewport };
        move_copy_cursor(
            mode,
            &CopyModeAction::CursorCentreVertical,
            &WordSeparators::default(),
            false,
        );
        assert_eq!(
            mode.cursor,
            PointCoordinate {
                x: 4,
                y: viewport + 2,
            }
        );
    }

    #[test]
    fn scroll_placement_moves_the_view_and_leaves_the_cursor_line_alone() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 3,
            max_scrollback: 32,
        })
        .expect("terminal");
        terminal.vt_write(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mut unseen_output = 0;
        let anchor = {
            let mode = copy_mode.as_mut().expect("mode");
            let maximum = mode.revision.maximum_offset();
            assert!(maximum >= 4);
            mode.viewport_offset = maximum - 2;
            mode.cursor = PointCoordinate {
                x: 0,
                y: maximum - 1,
            };
            mode.cursor.y
        };
        for (action, screen_row) in [
            (CopyModeAction::ScrollTop, 0),
            (CopyModeAction::ScrollMiddle, 1),
            (CopyModeAction::ScrollBottom, 2),
        ] {
            apply_copy_mode_action(
                &mut terminal,
                &mut selection,
                &mut copy_mode,
                &mut unseen_output,
                action.clone(),
                &WordSeparators::default(),
                false,
            )
            .expect("copy action");
            let mode = copy_mode.as_ref().expect("mode");
            assert_eq!(mode.cursor.y, anchor, "{action:?} moved the cursor line");
            assert_eq!(
                mode.viewport_offset,
                anchor - screen_row,
                "{action:?} placed the wrong view"
            );
        }
    }

    #[test]
    fn goto_line_scrolls_back_from_the_bottom_and_holds_the_cursor_screen_row() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 3,
            max_scrollback: 64,
        })
        .expect("terminal");
        for line in 0..20_u32 {
            terminal.vt_write(format!("L{line}\r\n").as_bytes());
        }
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mut unseen_output = 0;
        let (maximum, screen_row) = {
            let mode = copy_mode.as_mut().expect("mode");
            let maximum = mode.revision.maximum_offset();
            assert!(maximum >= 8);
            mode.viewport_offset = maximum;
            mode.cursor = PointCoordinate {
                x: 0,
                y: maximum + 1,
            };
            (maximum, 1)
        };
        let goto = |copy_mode: &mut CopyModeSlot,
                    terminal: &mut Terminal<'_, '_>,
                    selection: &mut Option<SelectionState>,
                    unseen_output: &mut u32,
                    line: u32| {
            apply_copy_mode_action(
                terminal,
                selection,
                copy_mode,
                unseen_output,
                CopyModeAction::GotoLine(line),
                &WordSeparators::default(),
                false,
            )
            .expect("goto-line");
        };
        {
            let mode = copy_mode.as_ref().expect("mode");
            assert_eq!(mode.viewport_offset, maximum);
        }
        goto(
            &mut copy_mode,
            &mut terminal,
            &mut selection,
            &mut unseen_output,
            u32::MAX,
        );
        {
            let mode = copy_mode.as_ref().expect("mode");
            assert_eq!(
                mode.viewport_offset, maximum,
                "an argument the pin's strtonum rejects must move nothing"
            );
        }
        for line in [0_u32, 1, 5, maximum, maximum + 1, i32::MAX.unsigned_abs()] {
            goto(
                &mut copy_mode,
                &mut terminal,
                &mut selection,
                &mut unseen_output,
                line,
            );
            let mode = copy_mode.as_ref().expect("mode");
            assert_eq!(
                mode.viewport_offset,
                maximum - line.min(maximum),
                "goto-line {line} placed the wrong view"
            );
            assert_eq!(
                mode.cursor.y - mode.viewport_offset,
                screen_row,
                "goto-line {line} moved the cursor screen row"
            );
        }
    }

    #[test]
    fn scroll_placement_does_nothing_when_the_revision_cannot_reach_that_far() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 3,
            max_scrollback: 32,
        })
        .expect("terminal");
        terminal.vt_write(b"a\r\nb\r\nc\r\nd\r\ne\r\nf");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mut unseen_output = 0;
        {
            let mode = copy_mode.as_mut().expect("mode");
            mode.viewport_offset = 0;
            mode.cursor = PointCoordinate { x: 0, y: 0 };
        }
        for action in [CopyModeAction::ScrollMiddle, CopyModeAction::ScrollBottom] {
            apply_copy_mode_action(
                &mut terminal,
                &mut selection,
                &mut copy_mode,
                &mut unseen_output,
                action.clone(),
                &WordSeparators::default(),
                false,
            )
            .expect("copy action");
            let mode = copy_mode.as_ref().expect("mode");
            assert_eq!(mode.viewport_offset, 0, "{action:?}");
            assert_eq!(mode.cursor.y, 0, "{action:?}");
        }
    }

    #[test]
    fn toggle_position_flips_the_published_position_readout() {
        let hidden = run_copy_actions(
            b"alpha beta",
            20,
            2,
            PointCoordinate { x: 0, y: 0 },
            &[CopyModeAction::TogglePosition],
        );
        assert!(hidden.hide_position);
        let shown = run_copy_actions(
            b"alpha beta",
            20,
            2,
            PointCoordinate { x: 0, y: 0 },
            &[
                CopyModeAction::TogglePosition,
                CopyModeAction::TogglePosition,
            ],
        );
        assert!(!shown.hide_position);
    }

    #[test]
    fn recentre_cycles_middle_top_bottom_and_restarts_on_a_new_line() {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 8,
            rows: 3,
            max_scrollback: 32,
        })
        .expect("terminal");
        terminal.vt_write(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni");
        let mut selection = None;
        let mut copy_mode = None;
        enter_copy_mode(
            &mut terminal,
            &mut selection,
            &mut copy_mode,
            false,
            false,
            None,
            true,
        )
        .expect("copy mode");
        let mut unseen_output = 0;
        let anchor = {
            let mode = copy_mode.as_mut().expect("mode");
            let maximum = mode.revision.maximum_offset();
            assert!(maximum >= 4);
            mode.viewport_offset = maximum - 2;
            mode.cursor = PointCoordinate {
                x: 0,
                y: maximum - 1,
            };
            mode.cursor.y
        };
        let mut recentre = |copy_mode: &mut CopyModeSlot| {
            apply_copy_mode_action(
                &mut terminal,
                &mut selection,
                copy_mode,
                &mut unseen_output,
                CopyModeAction::RecentreTopBottom,
                &WordSeparators::default(),
                false,
            )
            .expect("recentre");
            let mode = copy_mode.as_ref().expect("mode");
            (mode.cursor.y, mode.viewport_offset)
        };
        assert_eq!(recentre(&mut copy_mode), (anchor, anchor - 1));
        assert_eq!(recentre(&mut copy_mode), (anchor, anchor));
        assert_eq!(recentre(&mut copy_mode), (anchor, anchor - 2));
        assert_eq!(recentre(&mut copy_mode), (anchor, anchor - 1));

        copy_mode.as_mut().expect("mode").cursor.y = anchor - 1;
        assert_eq!(recentre(&mut copy_mode), (anchor - 1, anchor - 2));
    }

    #[test]
    fn recentre_clamps_instead_of_refusing_when_the_view_cannot_reach() {
        let mode = run_copy_actions(
            b"a\r\nb\r\nc",
            8,
            3,
            PointCoordinate { x: 0, y: 0 },
            &[CopyModeAction::RecentreTopBottom],
        );
        assert_eq!(mode.viewport_offset, 0);
        assert_eq!(mode.cursor.y, mode.revision.maximum_offset());
    }

    #[test]
    fn previous_matching_bracket_walks_back_from_the_nearest_bracket() {
        let closer = run_copy_actions(
            b"(alpha [beta] gamma)",
            24,
            2,
            PointCoordinate { x: 12, y: 0 },
            &[CopyModeAction::PreviousMatchingBracket],
        );
        assert_eq!(closer.cursor.x, 7);

        let inside = run_copy_actions(
            b"(alpha [beta] gamma)",
            24,
            2,
            PointCoordinate { x: 10, y: 0 },
            &[CopyModeAction::PreviousMatchingBracket],
        );
        assert_eq!(inside.cursor.x, 10);

        let nested = run_copy_actions(
            b"(alpha [beta] gamma)",
            24,
            2,
            PointCoordinate { x: 19, y: 0 },
            &[CopyModeAction::PreviousMatchingBracket],
        );
        assert_eq!(nested.cursor.x, 0);
    }

    #[test]
    fn previous_matching_bracket_stops_at_the_start_of_the_logical_line() {
        let stuck = run_copy_actions(
            b"alpha beta",
            24,
            2,
            PointCoordinate { x: 3, y: 0 },
            &[CopyModeAction::PreviousMatchingBracket],
        );
        assert_eq!(stuck.cursor.x, 3);

        let unmatched = run_copy_actions(
            b"alpha) beta",
            24,
            2,
            PointCoordinate { x: 8, y: 0 },
            &[CopyModeAction::PreviousMatchingBracket],
        );
        assert_eq!(unmatched.cursor.x, 8);
    }

    #[test]
    fn the_matching_bracket_pair_scans_in_opposite_directions() {
        let forward = run_copy_actions(
            b"one (two) three",
            24,
            2,
            PointCoordinate { x: 1, y: 0 },
            &[CopyModeAction::NextMatchingBracket],
        );
        assert_eq!(forward.cursor.x, 8);

        let backward = run_copy_actions(
            b"one (two) three",
            24,
            2,
            PointCoordinate { x: 12, y: 0 },
            &[CopyModeAction::PreviousMatchingBracket],
        );
        assert_eq!(backward.cursor.x, 4);
    }
}
