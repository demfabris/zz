use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    fmt::Write as _,
    ops::Range,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    Anchor, AnyElement, App, Bounds, ClipboardEntry, ClipboardItem, Context, Entity,
    EntityInputHandler, FocusHandle, Focusable, Font, FontFallbacks, FontFeatures, FontStyle,
    FontWeight, Hsla, Image, ImageSource, IntoElement, KeyBinding, KeyDownEvent, KeyUpEvent,
    Keystroke, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent,
    MouseUpEvent, NoAction, ObjectFit, Pixels, Point, Render, Rgba, ScrollWheelEvent, Subscription,
    Task, UTF16Selection, Window, anchored, deferred, div, font, img, point, prelude::*, px,
};
use parking_lot::RwLock;
use zz_client::{ChromeAction, TERMINAL_TABLE};
use zz_protocol::{
    ClientMessageKind, CommandInvocation, InputMessage, PaneId, PopupAction, TerminalUiCommand,
};
use zz_terminal::{
    AppearanceColor, AppearanceConfigKey, AppearanceProvenance, AppearanceSource, ClipboardTarget,
    CursorBlinkPolicy, IMAGE_PLACEHOLDER_SCHEME, KeyAction, KeyCode, KeyInput, Modifiers,
    PointerCellEvent, ScrollbarState, SearchCase, SearchDirection, SearchMode, SearchQuery,
    SearchStatus, SessionStatus, TerminalAppearance, TerminalMode, TerminalMouseButton,
    TerminalMouseInput, TerminalMousePhase, TerminalViewAction, TerminalViewport,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _,
    pane::{
        PaneOverlayCorner, pane_overlay_stack, terminal_link_popup,
        terminal_mode_indicator as terminal_mode_tag, terminal_search_prompt,
        terminal_status_popup,
    },
};

use crate::{
    config::pane_content_radii,
    diagnostics,
    keymap::ChromeChord,
    mux::client::{
        HistoryRing, KittyImageCache, MuxClient, RetainedTerminalViewport,
        TerminalFontSizeAdjustment,
    },
    mux::hosts::HostId,
    terminal::element::{RowRenderCache, TerminalElement},
    window::corners::{WindowCorners, round_div_radii},
};

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) const TERMINAL_FONT: &str = "Menlo";
#[cfg(target_os = "windows")]
pub(crate) const TERMINAL_FONT: &str = "Cascadia Mono";
#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
pub(crate) const TERMINAL_FONT: &str = "Noto Sans Mono";

#[cfg(any(target_os = "macos", target_os = "ios"))]
const LOCAL_TERMINAL_FONT_CANDIDATES: &[&str] = &["Menlo", "Monaco"];
#[cfg(target_os = "windows")]
const LOCAL_TERMINAL_FONT_CANDIDATES: &[&str] = &["Cascadia Mono", "Consolas"];
#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
const LOCAL_TERMINAL_FONT_CANDIDATES: &[&str] = &[
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Ubuntu Mono",
];

// CoreText backends (macOS, iOS) take font size in points; the rest take 96-DPI
// logical pixels.
#[cfg(any(target_os = "macos", target_os = "ios"))]
const GPUI_UNITS_PER_FONT_POINT: f32 = 1.0;
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
const GPUI_UNITS_PER_FONT_POINT: f32 = 96.0 / 72.0;

static COPY_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static PASTE_UPLOAD_ID: AtomicU64 = AtomicU64::new(1);
const SELECTION_AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(33);
const LOCAL_SCROLL_DEBOUNCE: Duration = Duration::from_millis(120);
const LOCAL_SCROLL_TIMEOUT: Duration = Duration::from_secs(2);
const IMAGE_HOVER_DWELL: Duration = Duration::from_millis(250);
const IMAGE_POPOVER_SIDE: f32 = 300.0;
const MAX_SELECTION_AUTOSCROLL_ROWS: i32 = 8;
const MAX_PASTE_BYTES: usize = 4 * 1024 * 1024;
const TERMINAL_KEY_CONTEXT: &str = "Terminal";

#[derive(Debug, PartialEq, Eq)]
struct ModeIndicator {
    label: Option<&'static str>,
    detail: String,
}

fn mode_indicator(mode: TerminalMode, unseen_output: u32) -> Option<ModeIndicator> {
    match mode {
        TerminalMode::Live if unseen_output > 0 => Some(ModeIndicator {
            label: None,
            detail: format!("+{unseen_output} output"),
        }),
        TerminalMode::Live => None,
        TerminalMode::Copy {
            position,
            total,
            hide_position,
        } => Some(ModeIndicator {
            label: Some("COPY MODE"),
            detail: match (hide_position, unseen_output) {
                (true, 0) => String::new(),
                (true, unseen) => format!("+{unseen} output"),
                (false, 0) => format!("{position}/{total}"),
                (false, unseen) => format!("{position}/{total}  ·  +{unseen} output"),
            },
        }),
        TerminalMode::View { position, total } => Some(ModeIndicator {
            label: Some("VIEW MODE"),
            detail: format!("{position}/{total}  ·  q close"),
        }),
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys(raw_key_bindings());
    crate::keymap::bind(cx, TERMINAL_TABLE, terminal_key_bindings);
}

fn raw_key_bindings() -> [KeyBinding; 2] {
    [
        KeyBinding::new("tab", NoAction, Some(TERMINAL_KEY_CONTEXT)),
        KeyBinding::new("shift-tab", NoAction, Some(TERMINAL_KEY_CONTEXT)),
    ]
}

/// A terminal pane resolves its own chrome on the raw key path, so the only
/// gpui bindings it needs are the ones that keep the font chords away from the
/// application zoom bound at the root.
fn terminal_key_bindings(chords: &[ChromeChord]) -> Vec<KeyBinding> {
    let context = Some(TERMINAL_KEY_CONTEXT);
    chords
        .iter()
        .filter(|chord| {
            matches!(
                chord.action(),
                ChromeAction::TerminalFontIncrease | ChromeAction::TerminalFontDecrease
            )
        })
        .map(|chord| chord.binding(NoAction, context))
        .collect()
}

/// Resolve daemon-declared font stacks against the machine that actually
/// renders them. Built-in defaults are platform-specific, while explicit
/// stacks keep their order and promote the first locally installed fallback.
pub(crate) fn localize_terminal_font_families(
    appearance: &mut TerminalAppearance,
    provenance: &AppearanceProvenance,
    available_fonts: &[String],
) {
    let local_default = LOCAL_TERMINAL_FONT_CANDIDATES
        .iter()
        .copied()
        .find(|candidate| font_is_available(candidate, available_fonts))
        .unwrap_or(TERMINAL_FONT);
    localize_terminal_font_families_with_default(
        appearance,
        provenance,
        available_fonts,
        local_default,
    );
}

fn localize_terminal_font_families_with_default(
    appearance: &mut TerminalAppearance,
    provenance: &AppearanceProvenance,
    available_fonts: &[String],
    local_default: &str,
) {
    localize_font_stack(
        &mut appearance.font_families,
        provenance.source(AppearanceConfigKey::FontFamily),
        available_fonts,
        Some(local_default),
    );
    for (families, key) in [
        (
            &mut appearance.font_families_bold,
            AppearanceConfigKey::FontFamilyBold,
        ),
        (
            &mut appearance.font_families_italic,
            AppearanceConfigKey::FontFamilyItalic,
        ),
        (
            &mut appearance.font_families_bold_italic,
            AppearanceConfigKey::FontFamilyBoldItalic,
        ),
    ] {
        localize_font_stack(families, provenance.source(key), available_fonts, None);
    }
}

fn localize_font_stack(
    families: &mut Vec<String>,
    source: AppearanceSource,
    available_fonts: &[String],
    default: Option<&str>,
) {
    if source == AppearanceSource::Default {
        families.clear();
    } else {
        families.retain(|family| font_is_available(family, available_fonts));
    }
    if families.is_empty()
        && let Some(default) = default
    {
        families.push(default.to_owned());
    }
}

fn font_is_available(family: &str, available_fonts: &[String]) -> bool {
    available_fonts
        .iter()
        .any(|available| available.eq_ignore_ascii_case(family))
}

pub(crate) fn terminal_font(appearance: &TerminalAppearance) -> Font {
    terminal_font_for_style(appearance, false, false)
}

pub(crate) fn terminal_font_for_style(
    appearance: &TerminalAppearance,
    bold: bool,
    italic: bool,
) -> Font {
    let configured_families = match (bold, italic) {
        (true, true) => &appearance.font_families_bold_italic,
        (true, false) => &appearance.font_families_bold,
        (false, true) => &appearance.font_families_italic,
        (false, false) => &appearance.font_families,
    };
    let families = if configured_families.is_empty() {
        &appearance.font_families
    } else {
        configured_families
    };
    let primary = families.first().map_or(TERMINAL_FONT, String::as_str);
    let mut font = font(primary.to_owned());
    let mut features = Vec::with_capacity(appearance.font_features.len() + 1);
    features.push(("liga".to_owned(), 1));
    features.extend(
        appearance
            .font_features
            .iter()
            .map(|feature| (feature.tag_string(), feature.value)),
    );
    font.features = FontFeatures(Arc::new(features));
    let fallbacks = families.iter().skip(1).cloned().collect::<Vec<_>>();
    font.fallbacks = (!fallbacks.is_empty()).then(|| FontFallbacks::from_fonts(fallbacks));
    font.weight = FontWeight(
        (f32::from(appearance.font_weight) + if bold { 300.0 } else { 0.0 })
            .min(FontWeight::BLACK.0),
    );
    font.style = if italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };
    font
}

struct TerminalRenderAppearance {
    source: Arc<TerminalAppearance>,
    font: Font,
    stable_hash: u64,
}

impl TerminalRenderAppearance {
    fn new(source: Arc<TerminalAppearance>) -> Self {
        let font = terminal_font(&source);
        let stable_hash = source.stable_hash();
        Self {
            source,
            font,
            stable_hash,
        }
    }
}

pub(crate) fn terminal_font_size(appearance: &TerminalAppearance) -> Pixels {
    px(appearance.font_size_points * GPUI_UNITS_PER_FONT_POINT)
}

pub(crate) fn terminal_line_height(appearance: &TerminalAppearance) -> Pixels {
    px(appearance
        .cell_height_adjustment
        .apply(f32::from(terminal_font_size(appearance)))
        .max(1.0))
}

fn appearance_hsla(color: AppearanceColor) -> Hsla {
    Rgba {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: f32::from(color.a) / 255.0,
    }
    .into()
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated opacity is rounded and clamped to the u8 alpha domain"
)]
fn opacity_byte(opacity: f32) -> u8 {
    (opacity * 255.0).round().clamp(0.0, 255.0) as u8
}

#[allow(
    clippy::disallowed_methods,
    reason = "terminal surfaces use the independent terminal appearance color system"
)]
fn terminal_background(background: zz_terminal::Color, opacity: f32) -> Hsla {
    appearance_hsla(AppearanceColor::rgba(
        background.r,
        background.g,
        background.b,
        opacity_byte(opacity),
    ))
}

fn presented_uri(uri: &str) -> String {
    const MAX_PRESENTED_CHARS: usize = 240;
    if let Some(number) = uri
        .strip_prefix(IMAGE_PLACEHOLDER_SCHEME)
        .and_then(|rest| rest.strip_prefix("://"))
    {
        return format!("Pasted image #{number}");
    }
    let mut characters = uri.chars();
    let mut presented = characters
        .by_ref()
        .take(MAX_PRESENTED_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        presented.push('…');
    }
    presented
}

fn pasted_image_number(uri: &str) -> Option<u32> {
    uri.strip_prefix(IMAGE_PLACEHOLDER_SCHEME)?
        .strip_prefix("://")?
        .parse()
        .ok()
}

fn search_prompt_text(
    query: &SearchQuery,
    marked: &str,
    search_status: Option<SearchStatus>,
) -> (String, usize) {
    let result = search_status.map_or_else(String::new, |search| {
        if search.invalid_pattern() {
            "  invalid pattern".to_owned()
        } else if search.pending() {
            "  searching…".to_owned()
        } else if search.total == 0 {
            "  0/0".to_owned()
        } else {
            format!("  {}/{}", search.current(), search.total)
        }
    });
    let mode = match query.mode {
        SearchMode::Literal => "literal",
        SearchMode::Regex => "regex",
    };
    let case = match query.case {
        SearchCase::Smart => "smart-case",
        SearchCase::Sensitive => "case-sensitive",
        SearchCase::Insensitive => "case-insensitive",
    };
    let direction = match query.direction {
        SearchDirection::Forward => "forward",
        SearchDirection::Backward => "backward",
    };
    let mut text = format!("Find: {}", query.text);
    let caret = text.len();
    text.push_str(marked);
    write!(
        text,
        "{result}  [{direction}, {mode}, {case}]  Alt+R / Alt+C"
    )
    .expect("writing to a String cannot fail");
    (text, caret)
}

const DIAGNOSTIC_TARGET: &str = "zz::diagnostics::terminal_render";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GridSize {
    pub columns: u16,
    pub rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HitGrid {
    bounds: Bounds<Pixels>,
    surface_bounds: Bounds<Pixels>,
    columns: u16,
    rows: u16,
    cell_width: Pixels,
    line_height: Pixels,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SearchPromptBehavior {
    #[default]
    Navigate,
    AcceptAndClose,
}

#[derive(Clone, Copy, Debug)]
struct LocalScroll {
    target_offset: u32,
    started: Instant,
}

fn local_scroll_gate(viewport: &TerminalViewport, history_rows: usize) -> bool {
    matches!(viewport.mode, TerminalMode::Live)
        && !viewport.mouse_tracking
        && viewport.scrollbar.total > viewport.scrollbar.len
        && history_rows != 0
}

fn local_scroll_should_retire(
    local_scroll: LocalScroll,
    server_offset: u32,
    expected_history_invalidations: u64,
    history_invalidations: u64,
    now: Instant,
) -> bool {
    server_offset == local_scroll.target_offset
        || expected_history_invalidations != history_invalidations
        || now.saturating_duration_since(local_scroll.started) >= LOCAL_SCROLL_TIMEOUT
}

fn scroll_fraction_offset(fraction: u32, maximum: u32) -> u32 {
    u32::try_from(u128::from(maximum).saturating_mul(u128::from(fraction)) / u128::from(u32::MAX))
        .unwrap_or(maximum)
}

fn local_scroll_needs_prefetch(
    target_offset: u32,
    scrollbar: ScrollbarState,
    history_rows: usize,
) -> bool {
    let Ok(history_rows) = u32::try_from(history_rows) else {
        return false;
    };
    let front = scrollbar.offset.saturating_sub(history_rows);
    target_offset < front.saturating_add(scrollbar.len.saturating_mul(2))
}

fn terminal_text_input(pane: PaneId, text: &str) -> InputMessage {
    InputMessage::Text {
        pane,
        text: text.to_owned(),
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent GPUI gesture and timer lifecycles are intentionally tracked separately"
)]
pub(crate) struct TerminalView {
    pane: PaneId,
    command_output: bool,
    popup: bool,
    text_opacity: f32,
    window_corners: WindowCorners,
    mux: Entity<MuxClient>,
    render_appearance: TerminalRenderAppearance,
    retained: Arc<RwLock<RetainedTerminalViewport>>,
    kitty_images: Arc<RwLock<KittyImageCache>>,
    observed_kitty_revision: u64,
    observed_generation: u64,
    observed_view_generation: u64,
    observed_row_revision_epoch: u64,
    observed_history_invalidations: u64,
    observed_pasted_image_revision: u64,
    observed_hovered_uri: Option<Arc<str>>,
    local_scroll: Option<LocalScroll>,
    local_scroll_generation: u64,
    row_cache: Rc<RefCell<RowRenderCache>>,
    focus_handle: FocusHandle,
    marked_text: Option<String>,
    search_query: Option<SearchQuery>,
    search_prompt_behavior: SearchPromptBehavior,
    swallowed_overlay_key: Option<KeyCode>,
    forwarded_keys: HashSet<String>,
    terminal_resize_suppressed: Rc<Cell<bool>>,
    last_grid_size: Option<GridSize>,
    hit_grid: Option<HitGrid>,
    pointer_position: Option<Point<Pixels>>,
    pointer_global: Option<Point<Pixels>>,
    link_hover_clear_sent: bool,
    forwarded_mouse_buttons: u8,
    scroll_rows: f32,
    cursor_bounds: Option<Bounds<Pixels>>,
    link_hover_bounds: Option<Bounds<Pixels>>,
    image_hover_dwell_elapsed: bool,
    image_hover_task: Task<()>,
    image_preview_press: Option<(u32, Bounds<Pixels>)>,
    cursor_blink_visible: bool,
    cursor_blink_active: bool,
    cursor_blink_task: Task<()>,
    cursor_focused: bool,
    selection_dragging: bool,
    force_local_selection: bool,
    selection_anchor: Option<PointerCellEvent>,
    selection_pointer: Option<PointerCellEvent>,
    selection_has_extent: bool,
    selection_autoscroll_lines: i32,
    selection_autoscroll_running: bool,
    scrollbar_dragging: bool,
    primary_paste_pressed: bool,
    last_clipboard_image: Option<(u64, Arc<Image>)>,
    _subscriptions: Vec<Subscription>,
}

impl TerminalView {
    fn observe_image_hover(&mut self, hovered_uri: Option<Arc<str>>, cx: &mut Context<Self>) {
        if self.observed_hovered_uri == hovered_uri {
            return;
        }
        self.observed_hovered_uri.clone_from(&hovered_uri);
        self.image_hover_dwell_elapsed = false;
        self.image_hover_task = Task::ready(());
        let Some(uri) = hovered_uri else {
            cx.notify();
            return;
        };
        let Some(number) = pasted_image_number(&uri) else {
            cx.notify();
            return;
        };
        self.mux.read(cx).prefetch_pasted_image(self.pane, number);
        self.image_hover_task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(IMAGE_HOVER_DWELL).await;
            let _ = this.update(cx, |view, cx| {
                if view.observed_hovered_uri.as_deref() == Some(uri.as_ref()) {
                    view.image_hover_dwell_elapsed = true;
                    cx.notify();
                }
            });
        });
    }

    fn visible_image_hover(&self, cx: &Context<Self>) -> Option<(u32, Bounds<Pixels>, Arc<Image>)> {
        if !self.image_hover_dwell_elapsed {
            return None;
        }
        let bounds = self.link_hover_bounds?;
        let number = self
            .observed_hovered_uri
            .as_deref()
            .and_then(pasted_image_number)?;
        let image = self.mux.read(cx).pasted_image(self.pane, number)?;
        Some((number, bounds, image))
    }

    fn image_hover_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, bounds, image) = self.visible_image_hover(cx)?;
        let (anchor, position) = if bounds.origin.y >= px(IMAGE_POPOVER_SIDE + 8.0) {
            (Anchor::BottomLeft, bounds.origin)
        } else {
            (Anchor::TopLeft, bounds.bottom_left())
        };
        let content = div()
            .size(px(IMAGE_POPOVER_SIDE))
            .p_1()
            .bg(cx.theme().background.raised(1).opaque())
            .text_color(cx.theme().foreground)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .shadow_md()
            .child(
                img(ImageSource::Image(image))
                    .size_full()
                    .object_fit(ObjectFit::ScaleDown),
            );
        Some(
            deferred(
                anchored()
                    .anchor(anchor)
                    .position(position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(content),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    pub(crate) fn apply_ui_command(&mut self, command: TerminalUiCommand, cx: &mut Context<Self>) {
        match command {
            TerminalUiCommand::BeginSearch { direction } => {
                let query = SearchQuery {
                    direction,
                    ..SearchQuery::default()
                };
                self.search_query = Some(query.clone());
                self.search_prompt_behavior = SearchPromptBehavior::AcceptAndClose;
                self.marked_text = None;
                self.send_view_action(cx, TerminalViewAction::SearchBegin(query));
                cx.notify();
            }
        }
    }

    pub(crate) fn new(
        pane: PaneId,
        mux: Entity<MuxClient>,
        terminal_resize_suppressed: Rc<Cell<bool>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_surface(
            pane,
            false,
            false,
            mux,
            terminal_resize_suppressed,
            window,
            cx,
        )
    }

    /// Command output is a read-only overlay backed by the same surface.
    pub(crate) fn new_command_output(
        pane: PaneId,
        mux: Entity<MuxClient>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_surface(
            pane,
            true,
            false,
            mux,
            Rc::new(Cell::new(false)),
            window,
            cx,
        )
    }

    pub(crate) fn new_popup(
        pane: PaneId,
        mux: Entity<MuxClient>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_surface(pane, false, true, mux, Rc::new(Cell::new(true)), window, cx)
    }

    fn new_surface(
        pane: PaneId,
        command_output: bool,
        popup: bool,
        mux: Entity<MuxClient>,
        terminal_resize_suppressed: Rc<Cell<bool>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let appearance = mux.read(cx).appearance();
        let retained = if command_output {
            mux.read(cx)
                .command_output()
                .filter(|output| output.pane == pane)
                .map(|output| output.retained)
        } else {
            mux.read(cx).viewport(pane)
        }
        .unwrap_or_else(|| {
            let viewport = TerminalViewport::blank_with_appearance(
                80,
                24,
                SessionStatus::Starting,
                &appearance,
            );
            let row_revisions = (0..viewport.rows)
                .map(|row| u64::MAX - u64::from(row))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let history_scrollbar = viewport.scrollbar;
            Arc::new(RwLock::new(RetainedTerminalViewport {
                viewport,
                history: HistoryRing::default(),
                history_scrollbar,
                history_mutations: 0,
                history_invalidations: 0,
                row_revisions,
                row_revision_epoch: u64::MAX,
                revision_scratch: Vec::new(),
            }))
        });
        let kitty_images = if command_output || popup {
            Arc::new(RwLock::new(KittyImageCache::default()))
        } else {
            mux.read(cx)
                .kitty_images(pane)
                .unwrap_or_else(|| Arc::new(RwLock::new(KittyImageCache::default())))
        };
        let observed_kitty_revision = kitty_images.read().revision();
        let observed_pasted_image_revision = if command_output || popup {
            0
        } else {
            mux.read(cx).pasted_image_revision(pane)
        };
        let (
            observed_generation,
            observed_view_generation,
            observed_row_revision_epoch,
            observed_history_invalidations,
            initial_hovered_uri,
        ) = {
            let state = retained.read();
            (
                state.viewport.generation,
                state.viewport.view_generation,
                state.row_revision_epoch,
                state.history_invalidations,
                state.viewport.presentation.hovered_uri.clone(),
            )
        };
        let focus_handle = cx.focus_handle();
        let entity_id = cx.entity_id();
        let mut subscriptions = Vec::with_capacity(3);
        subscriptions.push(cx.on_focus_out(&focus_handle, window, |view, _, _, _| {
            view.swallowed_overlay_key = None;
            view.forwarded_keys.clear();
        }));
        if command_output {
            let focused_mux = mux.clone();
            subscriptions.push(window.on_focus_in(&focus_handle, cx, move |_, cx| {
                focused_mux
                    .read(cx)
                    .send_input(InputMessage::CommandOutputView {
                        action: TerminalViewAction::Focus(true),
                    });
                cx.notify(entity_id);
            }));
            let blurred_mux = mux.clone();
            subscriptions.push(window.on_focus_out(&focus_handle, cx, move |_, _, cx| {
                blurred_mux
                    .read(cx)
                    .send_input(InputMessage::CommandOutputView {
                        action: TerminalViewAction::Focus(false),
                    });
                cx.notify(entity_id);
            }));
        } else if popup {
            let focused_mux = mux.clone();
            subscriptions.push(window.on_focus_in(&focus_handle, cx, move |_, cx| {
                focused_mux.read(cx).send_input(InputMessage::Popup {
                    action: PopupAction::TerminalView(TerminalViewAction::Focus(true)),
                });
                cx.notify(entity_id);
            }));
            let blurred_mux = mux.clone();
            subscriptions.push(window.on_focus_out(&focus_handle, cx, move |_, _, cx| {
                blurred_mux.read(cx).send_input(InputMessage::Popup {
                    action: PopupAction::TerminalView(TerminalViewAction::Focus(false)),
                });
                cx.notify(entity_id);
            }));
        } else {
            let focused_mux = mux.clone();
            subscriptions.push(window.on_focus_in(&focus_handle, cx, move |_, cx| {
                focused_mux.read(cx).send_input(InputMessage::TerminalView {
                    pane,
                    action: TerminalViewAction::Focus(true),
                });
                cx.notify(entity_id);
            }));
            let blurred_mux = mux.clone();
            subscriptions.push(window.on_focus_out(&focus_handle, cx, move |_, _, cx| {
                blurred_mux.read(cx).send_input(InputMessage::TerminalView {
                    pane,
                    action: TerminalViewAction::Focus(false),
                });
                cx.notify(entity_id);
            }));
        }
        cx.observe(&mux, move |view, mux, cx| {
            let mux = mux.read(cx);
            let appearance = mux.appearance();
            let retained = if view.command_output {
                mux.command_output()
                    .filter(|output| output.pane == pane)
                    .map(|output| output.retained)
            } else {
                mux.viewport(pane)
            };
            let kitty_images = (!view.command_output && !view.popup)
                .then(|| mux.kitty_images(pane))
                .flatten();
            let mut changed = false;
            if view.render_appearance.source.as_ref() != appearance.as_ref() {
                view.render_appearance = TerminalRenderAppearance::new(appearance);
                view.last_grid_size = None;
                view.hit_grid = None;
                view.cursor_blink_visible = true;
                view.cursor_blink_active = false;
                view.cursor_blink_task = Task::ready(());
                changed = true;
            }
            if let Some(retained) = retained {
                let retained_changed = !Arc::ptr_eq(&retained, &view.retained);
                let (
                    generation,
                    view_generation,
                    row_revision_epoch,
                    history_invalidations,
                    server_offset,
                    local_scroll_available,
                ) = {
                    let state = retained.read();
                    (
                        state.viewport.generation,
                        state.viewport.view_generation,
                        state.row_revision_epoch,
                        state.history_invalidations,
                        state.viewport.scrollbar.offset,
                        local_scroll_gate(&state.viewport, state.history.rows.len()),
                    )
                };
                if view.local_scroll.is_some_and(|local_scroll| {
                    retained_changed
                        || !local_scroll_available
                        || local_scroll_should_retire(
                            local_scroll,
                            server_offset,
                            view.observed_history_invalidations,
                            history_invalidations,
                            Instant::now(),
                        )
                }) {
                    view.clear_local_scroll();
                    changed = true;
                }
                if generation != view.observed_generation
                    || view_generation != view.observed_view_generation
                    || row_revision_epoch != view.observed_row_revision_epoch
                    || retained_changed
                {
                    view.retained = retained;
                    view.observed_generation = generation;
                    view.observed_view_generation = view_generation;
                    view.observed_row_revision_epoch = row_revision_epoch;
                    changed = true;
                }
                view.observed_history_invalidations = history_invalidations;
            }
            if let Some(kitty_images) = kitty_images {
                let revision = kitty_images.read().revision();
                if !Arc::ptr_eq(&kitty_images, &view.kitty_images)
                    || revision != view.observed_kitty_revision
                {
                    view.kitty_images = kitty_images;
                    view.observed_kitty_revision = revision;
                    changed = true;
                }
            } else {
                let revision = view.kitty_images.read().revision();
                if revision != view.observed_kitty_revision {
                    view.observed_kitty_revision = revision;
                    changed = true;
                }
            }
            let pasted_image_revision = if view.command_output || view.popup {
                0
            } else {
                mux.pasted_image_revision(pane)
            };
            if pasted_image_revision != view.observed_pasted_image_revision {
                view.observed_pasted_image_revision = pasted_image_revision;
                changed = true;
            }
            let hovered_uri = view
                .retained
                .read()
                .viewport
                .presentation
                .hovered_uri
                .clone();
            view.observe_image_hover(hovered_uri, cx);
            if changed {
                cx.notify();
            }
        })
        .detach();

        let mut view = Self {
            pane,
            command_output,
            popup,
            text_opacity: 1.0,
            window_corners: WindowCorners::NONE,
            mux,
            render_appearance: TerminalRenderAppearance::new(appearance),
            retained,
            kitty_images,
            observed_kitty_revision,
            observed_generation,
            observed_view_generation,
            observed_row_revision_epoch,
            observed_history_invalidations,
            observed_pasted_image_revision,
            observed_hovered_uri: None,
            local_scroll: None,
            local_scroll_generation: 0,
            row_cache: Rc::new(RefCell::new(RowRenderCache::default())),
            focus_handle,
            marked_text: None,
            search_query: None,
            search_prompt_behavior: SearchPromptBehavior::default(),
            swallowed_overlay_key: None,
            forwarded_keys: HashSet::new(),
            terminal_resize_suppressed,
            last_grid_size: None,
            hit_grid: None,
            pointer_position: None,
            pointer_global: None,
            link_hover_clear_sent: false,
            forwarded_mouse_buttons: 0,
            scroll_rows: 0.0,
            cursor_bounds: None,
            link_hover_bounds: None,
            image_hover_dwell_elapsed: false,
            image_hover_task: Task::ready(()),
            image_preview_press: None,
            cursor_blink_visible: true,
            cursor_blink_active: false,
            cursor_blink_task: Task::ready(()),
            cursor_focused: false,
            selection_dragging: false,
            force_local_selection: false,
            selection_anchor: None,
            selection_pointer: None,
            selection_has_extent: false,
            selection_autoscroll_lines: 0,
            selection_autoscroll_running: false,
            scrollbar_dragging: false,
            primary_paste_pressed: false,
            last_clipboard_image: None,
            _subscriptions: subscriptions,
        };
        view.observe_image_hover(initial_hovered_uri, cx);
        view
    }

    pub(crate) fn retained(&self) -> Arc<RwLock<RetainedTerminalViewport>> {
        Arc::clone(&self.retained)
    }

    pub(crate) fn local_scroll_target(&self) -> Option<u32> {
        self.local_scroll
            .map(|local_scroll| local_scroll.target_offset)
    }

    fn clear_local_scroll(&mut self) -> bool {
        let cleared = self.local_scroll.take().is_some();
        if cleared {
            self.local_scroll_generation = self.local_scroll_generation.wrapping_add(1);
        }
        cleared
    }

    fn cancel_local_scroll(&mut self, cx: &mut Context<Self>) {
        self.local_scroll_generation = self.local_scroll_generation.wrapping_add(1);
        if self.local_scroll.take().is_some() {
            cx.notify();
        }
    }

    fn should_use_local_scroll(&self) -> bool {
        let retained = self.retained.read();
        local_scroll_gate(&retained.viewport, retained.history.rows.len())
    }

    fn scroll_locally_by(&mut self, delta: i64, cx: &mut Context<Self>) {
        let base = self
            .local_scroll
            .map(|local_scroll| local_scroll.target_offset);
        let (server_offset, maximum_offset) = {
            let retained = self.retained.read();
            let scrollbar = retained.viewport.scrollbar;
            (
                scrollbar.offset,
                scrollbar.total.saturating_sub(scrollbar.len),
            )
        };
        let base = base.unwrap_or(server_offset);
        let target = i128::from(base)
            .saturating_add(i128::from(delta))
            .clamp(0, i128::from(maximum_offset));
        self.scroll_locally_to(u32::try_from(target).unwrap_or(server_offset), cx);
    }

    fn scroll_locally_to(&mut self, target: u32, cx: &mut Context<Self>) {
        let (target, server_offset, coverage_start, at_tail, history_invalidations) = {
            let retained = self.retained.read();
            let scrollbar = retained.viewport.scrollbar;
            let retained_rows = u32::try_from(retained.history.rows.len()).unwrap_or(u32::MAX);
            let coverage_start = scrollbar.offset.saturating_sub(retained_rows);
            let maximum_offset = scrollbar.total.saturating_sub(scrollbar.len);
            (
                target.min(maximum_offset),
                scrollbar.offset,
                coverage_start,
                scrollbar.offset.saturating_add(scrollbar.len) >= scrollbar.total,
                retained.history_invalidations,
            )
        };
        self.observed_history_invalidations = history_invalidations;
        if target < coverage_start {
            self.request_local_scroll_prefetch(target, cx);
            let cleared = self.clear_local_scroll();
            self.send_view_action(cx, TerminalViewAction::ScrollToOffset(target));
            if cleared {
                cx.notify();
            }
            return;
        }
        if target > server_offset {
            let cleared = self.clear_local_scroll();
            self.send_view_action(cx, TerminalViewAction::ScrollToOffset(target));
            if cleared {
                cx.notify();
            }
            return;
        }
        if target == server_offset {
            let cleared = self.clear_local_scroll();
            if at_tail {
                self.send_view_action(cx, TerminalViewAction::ScrollToOffset(target));
            }
            if cleared {
                cx.notify();
            }
            return;
        }

        self.local_scroll_generation = self.local_scroll_generation.wrapping_add(1);
        let generation = self.local_scroll_generation;
        self.local_scroll = Some(LocalScroll {
            target_offset: target,
            started: Instant::now(),
        });
        cx.notify();
        self.request_local_scroll_prefetch(target, cx);
        self.schedule_local_scroll_sync(generation, cx);
    }

    fn request_local_scroll_prefetch(&self, target: u32, cx: &mut Context<Self>) {
        if self.command_output || self.popup {
            return;
        }
        let near_cold_edge = {
            let retained = self.retained.read();
            local_scroll_needs_prefetch(
                target,
                retained.viewport.scrollbar,
                retained.history.rows.len(),
            )
        };
        if near_cold_edge {
            self.mux
                .update(cx, |mux, _| mux.request_history_prefetch(self.pane, target));
        }
    }

    fn schedule_local_scroll_sync(&self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LOCAL_SCROLL_DEBOUNCE).await;
            let Ok(active) = this.update(cx, |view, cx| {
                if view.local_scroll_generation != generation {
                    return false;
                }
                let Some(local_scroll) = view.local_scroll else {
                    return false;
                };
                let converged =
                    view.retained.read().viewport.scrollbar.offset == local_scroll.target_offset;
                if converged {
                    view.clear_local_scroll();
                    cx.notify();
                    return false;
                }
                if !view.should_use_local_scroll() {
                    view.clear_local_scroll();
                    cx.notify();
                    return false;
                }
                view.send_view_action(
                    cx,
                    TerminalViewAction::ScrollToOffset(local_scroll.target_offset),
                );
                true
            }) else {
                return;
            };
            if !active {
                return;
            }

            cx.background_executor()
                .timer(LOCAL_SCROLL_TIMEOUT.saturating_sub(LOCAL_SCROLL_DEBOUNCE))
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.local_scroll_generation != generation {
                    return;
                }
                if view.local_scroll.is_some_and(|local_scroll| {
                    Instant::now().saturating_duration_since(local_scroll.started)
                        >= LOCAL_SCROLL_TIMEOUT
                }) {
                    view.clear_local_scroll();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn set_window_corners(&mut self, corners: WindowCorners, cx: &mut Context<Self>) {
        if self.window_corners != corners {
            self.window_corners = corners;
            cx.notify();
        }
    }

    pub(crate) fn set_text_dimmed(&mut self, dimmed: bool, opacity: f32, cx: &mut Context<Self>) {
        let opacity = if dimmed { opacity.clamp(0.0, 1.0) } else { 1.0 };
        if self.text_opacity.to_bits() != opacity.to_bits() {
            self.text_opacity = opacity;
            cx.notify();
        }
    }

    pub(crate) fn marked_text(&self) -> Option<String> {
        self.marked_text.clone()
    }

    pub(crate) fn search_ime_layout(
        &self,
        search_status: Option<SearchStatus>,
    ) -> Option<(String, usize)> {
        let query = self.search_query.as_ref()?;
        Some(search_prompt_text(
            query,
            self.marked_text.as_deref().unwrap_or_default(),
            search_status,
        ))
    }

    pub(crate) fn focus(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) const fn is_command_output(&self) -> bool {
        self.command_output
    }

    pub(crate) fn update_geometry(
        &mut self,
        grid_size: GridSize,
        bounds: Bounds<Pixels>,
        surface_bounds: Bounds<Pixels>,
        cell_width: Pixels,
        line_height: Pixels,
        cursor_bounds: Option<Bounds<Pixels>>,
        search_cursor_bounds: Option<Bounds<Pixels>>,
        link_hover_bounds: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let previous = self.last_grid_size;
        if !self.popup
            && self.last_grid_size != Some(grid_size)
            && (self.command_output || !self.terminal_resize_suppressed.get())
        {
            self.mux.read(cx).send_input(if self.command_output {
                InputMessage::ResizeCommandOutput {
                    columns: grid_size.columns,
                    rows: grid_size.rows,
                    cell_width_px: grid_size.cell_width_px,
                    cell_height_px: grid_size.cell_height_px,
                }
            } else {
                InputMessage::ResizeTerminal {
                    pane: self.pane,
                    columns: grid_size.columns,
                    rows: grid_size.rows,
                    cell_width_px: grid_size.cell_width_px,
                    cell_height_px: grid_size.cell_height_px,
                }
            });
            self.last_grid_size = Some(grid_size);
        }
        self.hit_grid = Some(HitGrid {
            bounds,
            surface_bounds,
            columns: grid_size.columns,
            rows: grid_size.rows,
            cell_width,
            line_height,
        });
        self.cursor_bounds = if self.search_query.is_some() {
            search_cursor_bounds
        } else {
            cursor_bounds
        };
        self.link_hover_bounds = link_hover_bounds;
        log::trace!(
            target: "zz::diagnostics::terminal_render",
            "update_geometry pane={} command_output={} previous={previous:?} next={grid_size:?} changed={} bounds={bounds:?} surface_bounds={surface_bounds:?} cell_width={} line_height={} cursor_bounds={:?} elapsed_us={}",
            self.pane,
            self.command_output,
            previous != Some(grid_size),
            f32::from(cell_width),
            f32::from(line_height),
            self.cursor_bounds,
            diagnostics::elapsed_us(started),
        );
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_pointer_position(event.position);
        self.focus_handle.focus(window, cx);
        self.reset_cursor_blink(cx);
        self.image_preview_press = if event.button == MouseButton::Left
            && event.click_count == 1
            && !event.modifiers.control
            && !event.modifiers.platform
            && !event.modifiers.shift
            && !event.modifiers.alt
        {
            self.visible_image_hover(cx)
                .filter(|(_, bounds, _)| bounds.contains(&event.position))
                .map(|(number, bounds, _)| (number, bounds))
        } else {
            None
        };
        if !self.command_output && !self.popup {
            self.mux.read(cx).execute(CommandInvocation::new(
                "select-pane",
                ["-t", &self.pane.to_string()],
            ));
        }
        if event.button == MouseButton::Middle {
            self.primary_paste_pressed = false;
        }
        if event.button == MouseButton::Middle
            && should_paste_primary(
                self.command_output,
                self.retained.read().viewport.mouse_tracking,
                event.modifiers.shift,
            )
        {
            self.primary_paste_pressed = true;
            if let Some(text) = read_primary_paste(cx) {
                self.request_paste(text, cx);
            }
            cx.stop_propagation();
            return;
        }
        if event.button == MouseButton::Left && self.scrollbar_hit(event.position) {
            self.scrollbar_dragging = true;
            self.selection_dragging = false;
            self.force_local_selection = false;
            self.selection_anchor = None;
            self.selection_pointer = None;
            self.selection_has_extent = false;
            self.selection_autoscroll_lines = 0;
            self.clear_link_hover(cx);
            self.scroll_to_pointer(event.position, cx);
            cx.stop_propagation();
            return;
        }
        if event.button == MouseButton::Left {
            self.force_local_selection =
                event.click_count >= 3 && (event.modifiers.control || event.modifiers.platform);
            self.selection_dragging =
                self.local_selection(event.modifiers) || self.force_local_selection;
            self.selection_pointer =
                self.pointer_cell(event.position, event.click_count, event.modifiers);
            self.selection_anchor = self.selection_pointer;
            self.selection_has_extent = self
                .selection_anchor
                .is_some_and(|anchor| anchor.click_count >= 2);
            if self.selection_pointer.is_none() {
                self.selection_dragging = false;
                self.force_local_selection = false;
                self.selection_anchor = None;
                self.selection_has_extent = false;
            }
            self.selection_autoscroll_lines = 0;
        }
        if let Some(input) = self.mouse_input(
            event.position,
            event.click_count,
            event.modifiers,
            TerminalMousePhase::Press,
            Some(terminal_mouse_button(event.button)),
            window.scale_factor(),
        ) {
            self.forwarded_mouse_buttons |= mouse_button_bit(event.button);
            self.link_hover_clear_sent = false;
            self.send_view_action(cx, TerminalViewAction::Mouse(input));
        } else {
            self.clear_link_hover(cx);
        }
        cx.stop_propagation();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_pointer = self.pointer_position;
        self.update_pointer_position(event.position);
        if self.pointer_position != previous_pointer {
            self.reset_cursor_blink(cx);
            if self
                .retained
                .read()
                .viewport
                .presentation
                .hovered_uri
                .is_some()
            {
                cx.notify();
            }
        }
        if event.pressed_button.is_none() && (self.selection_dragging || self.scrollbar_dragging) {
            self.end_local_drag();
            self.force_local_selection = false;
        }
        if self.scrollbar_dragging {
            self.clear_link_hover(cx);
            self.scroll_to_pointer(event.position, cx);
            cx.stop_propagation();
            return;
        }
        if self.selection_dragging {
            self.selection_pointer = self.pointer_cell_clamped(event.position, 1, event.modifiers);
            self.selection_autoscroll_lines = self.selection_autoscroll(event.position);
            self.selection_has_extent = self
                .selection_anchor
                .zip(self.selection_pointer)
                .is_some_and(|(anchor, pointer)| selection_has_extent(anchor, pointer))
                || self.selection_autoscroll_lines != 0;
            self.ensure_selection_autoscroll(cx);
        }
        if !owns_pressed_button(self.forwarded_mouse_buttons, event.pressed_button) {
            return;
        }
        if let Some(input) = self.mouse_input(
            event.position,
            1,
            event.modifiers,
            TerminalMousePhase::Motion,
            event.pressed_button.map(terminal_mouse_button),
            window.scale_factor(),
        ) {
            self.link_hover_clear_sent = false;
            self.send_view_action(cx, TerminalViewAction::Mouse(input));
            if event.dragging() {
                cx.stop_propagation();
            }
        } else {
            self.clear_link_hover(cx);
        }
    }

    fn update_pointer_position(&mut self, position: Point<Pixels>) {
        self.pointer_global = Some(position);
        self.pointer_position = self.hit_grid.map(|grid| {
            point(
                (position.x - grid.surface_bounds.origin.x
                    + px(self.render_appearance.source.padding_left))
                .max(px(0.0)),
                (position.y - grid.surface_bounds.origin.y
                    + px(self.render_appearance.source.padding_top))
                .max(px(0.0)),
            )
        });
    }

    fn on_mouse_exit(&mut self, _: &MouseExitEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.pointer_global = None;
        self.pointer_position = None;
        self.clear_link_hover(cx);
        cx.notify();
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(position) = self.pointer_global else {
            return;
        };
        if let Some(input) = self.mouse_input(
            position,
            1,
            event.modifiers,
            TerminalMousePhase::Motion,
            None,
            window.scale_factor(),
        ) {
            self.link_hover_clear_sent = false;
            self.send_view_action(cx, TerminalViewAction::Mouse(input));
        } else {
            self.clear_link_hover(cx);
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
        let image_preview_press = (event.button == MouseButton::Left)
            .then(|| self.image_preview_press.take())
            .flatten();
        let selection_had_extent = self.selection_dragging && self.selection_has_extent;
        self.release_mouse_button(event, window, cx);
        if event.button == MouseButton::Left
            && event.click_count == 1
            && !event.modifiers.control
            && !event.modifiers.platform
            && !event.modifiers.shift
            && !event.modifiers.alt
            && !selection_had_extent
            && let Some((number, bounds)) = image_preview_press
            && bounds.contains(&event.position)
            && self.mux.read(cx).pasted_image(self.pane, number).is_some()
        {
            let pane = self.pane;
            self.mux
                .update(cx, |mux, cx| mux.open_pasted_image(pane, number, cx));
        }
        cx.stop_propagation();
    }

    fn on_mouse_up_out(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left {
            self.image_preview_press = None;
        }
        if self.mouse_gesture_active() {
            self.release_mouse_button(event, window, cx);
        }
    }

    fn mouse_gesture_active(&self) -> bool {
        self.selection_dragging
            || self.scrollbar_dragging
            || self.primary_paste_pressed
            || self.forwarded_mouse_buttons != 0
    }

    fn end_local_drag(&mut self) {
        self.selection_dragging = false;
        self.scrollbar_dragging = false;
        self.selection_anchor = None;
        self.selection_pointer = None;
        self.selection_has_extent = false;
        self.selection_autoscroll_lines = 0;
    }

    fn release_mouse_button(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Middle && std::mem::take(&mut self.primary_paste_pressed) {
            return;
        }
        if self.scrollbar_dragging && event.button == MouseButton::Left {
            self.scroll_to_pointer(event.position, cx);
            self.scrollbar_dragging = false;
            return;
        }
        let button_bit = mouse_button_bit(event.button);
        let forwarded_press = forwarded_mouse_press(self.forwarded_mouse_buttons, event.button);
        let copied_selection = self.selection_dragging
            && self.selection_has_extent
            && event.button == MouseButton::Left;
        self.end_local_drag();
        let input = if copied_selection || forwarded_press {
            self.mouse_input_clamped(
                event.position,
                event.click_count,
                event.modifiers,
                TerminalMousePhase::Release,
                Some(terminal_mouse_button(event.button)),
                window.scale_factor(),
            )
        } else {
            None
        };
        if let Some(input) = input {
            self.link_hover_clear_sent = false;
            self.send_view_action(cx, TerminalViewAction::Mouse(input));
            if copied_selection {
                self.copy_selection(cx, ClipboardTarget::Primary);
            }
        }
        self.forwarded_mouse_buttons &= !button_bit;
        self.force_local_selection = false;
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let line_height = self.hit_grid.map_or(px(19.0), |grid| grid.line_height);
        let delta = event.delta.pixel_delta(line_height);
        self.scroll_rows += f32::from(delta.y) / f32::from(line_height);
        let rows = self.scroll_rows.trunc();
        self.scroll_rows -= rows;
        if rows != 0.0 {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "wheel deltas are coalesced and bounded by platform event sizes"
            )]
            let lines = -(rows as i32);
            if lines > 0 && self.should_use_local_scroll() {
                self.scroll_locally_by(i64::from(lines), cx);
            } else {
                if lines < 0
                    && let Some(local_scroll) = self.local_scroll
                {
                    self.clear_local_scroll();
                    self.send_view_action(
                        cx,
                        TerminalViewAction::ScrollToOffset(local_scroll.target_offset),
                    );
                    cx.notify();
                }
                let button = if lines < 0 {
                    TerminalMouseButton::ScrollUp
                } else {
                    TerminalMouseButton::ScrollDown
                };
                if let Some(input) = self.mouse_input(
                    event.position,
                    1,
                    event.modifiers,
                    TerminalMousePhase::Press,
                    Some(button),
                    window.scale_factor(),
                ) {
                    self.send_view_action(cx, TerminalViewAction::ScrollWheel { lines, input });
                }
            }
        }
        cx.stop_propagation();
    }

    fn local_selection(&self, modifiers: gpui::Modifiers) -> bool {
        modifiers.shift || !self.retained.read().viewport.mouse_tracking
    }

    fn scrollbar_hit(&self, position: Point<Pixels>) -> bool {
        let Some(grid) = self.hit_grid else {
            return false;
        };
        let scrollbar = self.retained.read().viewport.scrollbar;
        scrollbar_strip_hit(grid, scrollbar, position)
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the scrollbar fraction is clamped to the complete u32 range"
    )]
    fn scroll_to_pointer(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(grid) = self.hit_grid else {
            return;
        };
        let fraction = ((position.y - grid.surface_bounds.origin.y)
            / grid.surface_bounds.size.height)
            .clamp(0.0, 1.0);
        let fraction = (fraction * u32::MAX as f32).round() as u32;
        if self.should_use_local_scroll() {
            let scrollbar = self.retained.read().viewport.scrollbar;
            let maximum = scrollbar.total.saturating_sub(scrollbar.len);
            self.scroll_locally_to(scroll_fraction_offset(fraction, maximum), cx);
        } else {
            self.send_view_action(cx, TerminalViewAction::ScrollToFraction(fraction));
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "the pixel distance is clamped to a small integral row rate before conversion"
    )]
    fn selection_autoscroll(&self, position: Point<Pixels>) -> i32 {
        let Some(grid) = self.hit_grid else {
            return 0;
        };
        let distance = if position.y < grid.bounds.origin.y {
            -f32::from(grid.bounds.origin.y - position.y)
        } else if position.y > grid.bounds.bottom() {
            f32::from(position.y - grid.bounds.bottom())
        } else {
            return 0;
        };
        let rows = (distance.abs() / f32::from(grid.line_height))
            .ceil()
            .max(1.0)
            .min(MAX_SELECTION_AUTOSCROLL_ROWS as f32) as i32;
        if distance.is_sign_negative() {
            -rows
        } else {
            rows
        }
    }

    fn ensure_selection_autoscroll(&mut self, cx: &mut Context<Self>) {
        if !self.selection_dragging
            || self.selection_autoscroll_lines == 0
            || self.selection_autoscroll_running
        {
            return;
        }
        self.selection_autoscroll_running = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(SELECTION_AUTOSCROLL_INTERVAL)
                    .await;
                let Ok(keep_running) = this.update(cx, |view, cx| {
                    let Some(pointer) = view.selection_pointer else {
                        view.selection_autoscroll_running = false;
                        return false;
                    };
                    if !view.selection_dragging || view.selection_autoscroll_lines == 0 {
                        view.selection_autoscroll_running = false;
                        return false;
                    }
                    view.send_view_action(
                        cx,
                        TerminalViewAction::SelectionAutoscroll {
                            lines: view.selection_autoscroll_lines,
                            pointer,
                        },
                    );
                    true
                }) else {
                    break;
                };
                if !keep_running {
                    break;
                }
            }
        })
        .detach();
    }

    fn pointer_cell(
        &self,
        position: Point<Pixels>,
        click_count: usize,
        modifiers: gpui::Modifiers,
    ) -> Option<PointerCellEvent> {
        self.pointer_cell_with_clamping(position, click_count, modifiers, false)
    }

    fn pointer_cell_clamped(
        &self,
        position: Point<Pixels>,
        click_count: usize,
        modifiers: gpui::Modifiers,
    ) -> Option<PointerCellEvent> {
        self.pointer_cell_with_clamping(position, click_count, modifiers, true)
    }

    fn pointer_cell_with_clamping(
        &self,
        position: Point<Pixels>,
        click_count: usize,
        modifiers: gpui::Modifiers,
        clamp: bool,
    ) -> Option<PointerCellEvent> {
        let grid = self.hit_grid?;
        if grid.columns == 0 || grid.rows == 0 {
            return None;
        }
        if !clamp
            && (self.scrollbar_hit(position)
                || position.x < grid.bounds.origin.x
                || position.x >= grid.bounds.right()
                || position.y < grid.bounds.origin.y
                || position.y >= grid.bounds.bottom())
        {
            return None;
        }
        let column = ((position.x - grid.bounds.origin.x) / grid.cell_width)
            .floor()
            .clamp(0.0, f32::from(grid.columns.saturating_sub(1)));
        let row = ((position.y - grid.bounds.origin.y) / grid.line_height)
            .floor()
            .clamp(0.0, f32::from(grid.rows.saturating_sub(1)));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "hit coordinates are floored and clamped to the terminal u16 grid"
        )]
        Some(PointerCellEvent {
            column: column as u16,
            row: row as u16,
            click_count: u8::try_from(click_count).unwrap_or(u8::MAX),
            rectangle: modifiers.alt,
        })
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pointer pixels are rounded and clamped to the u32 surface domain"
    )]
    fn mouse_input(
        &self,
        position: Point<Pixels>,
        click_count: usize,
        input_modifiers: gpui::Modifiers,
        phase: TerminalMousePhase,
        button: Option<TerminalMouseButton>,
        scale: f32,
    ) -> Option<TerminalMouseInput> {
        self.mouse_input_with_clamping(
            position,
            click_count,
            input_modifiers,
            phase,
            button,
            scale,
            false,
        )
    }

    fn mouse_input_clamped(
        &self,
        position: Point<Pixels>,
        click_count: usize,
        input_modifiers: gpui::Modifiers,
        phase: TerminalMousePhase,
        button: Option<TerminalMouseButton>,
        scale: f32,
    ) -> Option<TerminalMouseInput> {
        self.mouse_input_with_clamping(
            position,
            click_count,
            input_modifiers,
            phase,
            button,
            scale,
            true,
        )
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pointer pixels are rounded and clamped to the u32 surface domain"
    )]
    fn mouse_input_with_clamping(
        &self,
        position: Point<Pixels>,
        click_count: usize,
        input_modifiers: gpui::Modifiers,
        phase: TerminalMousePhase,
        button: Option<TerminalMouseButton>,
        scale: f32,
        clamp: bool,
    ) -> Option<TerminalMouseInput> {
        let grid = self.hit_grid?;
        let cell =
            self.pointer_cell_with_clamping(position, click_count, input_modifiers, clamp)?;
        let physical = |value: Pixels| {
            ((f32::from(value) * scale)
                .round()
                .clamp(0.0, u32::MAX as f32)) as u32
        };
        Some(TerminalMouseInput::new(
            phase,
            button,
            cell,
            physical((position.x - grid.bounds.origin.x).clamp(px(0.0), grid.bounds.size.width)),
            physical((position.y - grid.bounds.origin.y).clamp(px(0.0), grid.bounds.size.height)),
            physical(grid.bounds.size.width),
            physical(grid.bounds.size.height),
            physical(grid.cell_width).max(1),
            physical(grid.line_height).max(1),
            modifiers(input_modifiers),
            input_modifiers.shift || self.force_local_selection,
        ))
    }

    fn clear_link_hover(&mut self, cx: &Context<Self>) {
        if self.link_hover_clear_sent {
            return;
        }
        self.link_hover_clear_sent = true;
        self.send_view_action(cx, TerminalViewAction::ClearLinkHover);
    }

    fn send_view_action(&self, cx: &Context<Self>, action: TerminalViewAction) {
        self.mux.read(cx).send_input(if self.command_output {
            InputMessage::CommandOutputView { action }
        } else if self.popup {
            InputMessage::Popup {
                action: PopupAction::TerminalView(action),
            }
        } else {
            InputMessage::TerminalView {
                pane: self.pane,
                action,
            }
        });
    }

    fn copy_selection(&self, cx: &Context<Self>, target: ClipboardTarget) {
        self.send_view_action(
            cx,
            TerminalViewAction::CopySelection {
                request_id: COPY_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
                target,
            },
        );
    }

    fn request_paste(&mut self, text: String, cx: &mut Context<Self>) {
        if self.command_output || text.is_empty() {
            return;
        }
        if text.len() > MAX_PASTE_BYTES {
            self.mux.update(cx, |_, cx| {
                MuxClient::emit_notification(
                    ClientMessageKind::Warning,
                    format!(
                        "Paste skipped: {} bytes exceeds the {} MiB limit",
                        text.len(),
                        MAX_PASTE_BYTES / (1024 * 1024)
                    ),
                    cx,
                );
            });
            return;
        }
        self.cancel_local_scroll(cx);
        self.send_view_action(cx, TerminalViewAction::Paste(text));
    }

    fn snapshot_clipboard_image(&mut self, item: ClipboardItem, cx: &mut Context<Self>) -> bool {
        if self.command_output || self.popup {
            return false;
        }
        let Some(image) = item.into_entries().find_map(|entry| match entry {
            ClipboardEntry::Image(image) => Some(image),
            ClipboardEntry::String(_) | ClipboardEntry::ExternalPaths(_) => None,
        }) else {
            return false;
        };
        let normalized = match self.last_clipboard_image.as_ref() {
            Some((id, normalized)) if *id == image.id() => Arc::clone(normalized),
            _ => match crate::agent::attachment::normalize(&image) {
                Ok(normalized) => {
                    self.last_clipboard_image = Some((image.id(), Arc::clone(&normalized)));
                    normalized
                }
                Err(error) => {
                    log::debug!("pane {} cannot keep the pasted image: {error}", self.pane);
                    return false;
                }
            },
        };
        let _ = self.mux.read(cx).record_pasted_image(
            PASTE_UPLOAD_ID.fetch_add(1, Ordering::Relaxed),
            self.pane,
            normalized.format.extension().to_owned(),
            &normalized.bytes,
        );
        true
    }

    fn pane_is_remote(&self, cx: &Context<Self>) -> bool {
        self.mux.read(cx).attached_host() != HostId::LOCAL
    }

    fn upload_clipboard_image(&mut self, item: ClipboardItem, cx: &mut Context<Self>) -> bool {
        if self.command_output || self.popup {
            return false;
        }
        let Some(image) = item.into_entries().find_map(|entry| match entry {
            ClipboardEntry::Image(image) => Some(image),
            ClipboardEntry::String(_) | ClipboardEntry::ExternalPaths(_) => None,
        }) else {
            return false;
        };
        let normalized = match crate::agent::attachment::normalize(&image) {
            Ok(normalized) => normalized,
            Err(error) => {
                self.mux.update(cx, |_, cx| {
                    MuxClient::emit_notification(
                        ClientMessageKind::Warning,
                        format!("Paste skipped: {error}"),
                        cx,
                    );
                });
                return true;
            }
        };
        let sent = self.mux.read(cx).send_paste_upload(
            PASTE_UPLOAD_ID.fetch_add(1, Ordering::Relaxed),
            self.pane,
            normalized.format.extension().to_owned(),
            &normalized.bytes,
        );
        if sent {
            self.cancel_local_scroll(cx);
        } else {
            self.mux.update(cx, |_, cx| {
                MuxClient::emit_notification(
                    ClientMessageKind::Error,
                    "Paste failed: could not send the image to the remote host",
                    cx,
                );
            });
        }
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
        let code = key_code(&event.keystroke.key);
        let modifiers = modifiers(event.keystroke.modifiers);
        if self.popup {
            let printable = matches!(code, KeyCode::Character(_));
            let text_follows = !self.retained.read().viewport.kitty_keyboard
                && printable
                && !modifiers.control()
                && !modifiers.alt()
                && !modifiers.platform();
            if !text_follows {
                self.forwarded_keys.insert(event.keystroke.key.clone());
            }
            self.mux.read(cx).send_input(InputMessage::Popup {
                action: PopupAction::Key {
                    input: key_input(
                        &event.keystroke,
                        code,
                        if event.is_held {
                            KeyAction::Repeat
                        } else {
                            KeyAction::Press
                        },
                    ),
                    text_follows,
                },
            });
            cx.stop_propagation();
            return;
        }
        let chrome = crate::keymap::resolve(cx, TERMINAL_TABLE, &event.keystroke);
        let font_adjustment = match chrome {
            Some(ChromeAction::TerminalFontIncrease) => Some(TerminalFontSizeAdjustment::Increase),
            Some(ChromeAction::TerminalFontDecrease) => Some(TerminalFontSizeAdjustment::Decrease),
            _ => None,
        };
        if let Some(adjustment) = font_adjustment {
            self.mux.update(cx, |mux, cx| {
                mux.adjust_terminal_font_size(adjustment, cx);
            });
            cx.stop_propagation();
            return;
        }
        if self.swallowed_overlay_key == Some(code) {
            cx.stop_propagation();
            return;
        }
        if chrome == Some(ChromeAction::TerminalSearch) {
            let query = SearchQuery::default();
            self.search_query = Some(query.clone());
            self.search_prompt_behavior = SearchPromptBehavior::Navigate;
            self.marked_text = None;
            self.send_view_action(cx, TerminalViewAction::SearchBegin(query));
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if self.search_query.is_some() {
            match code {
                KeyCode::Escape => {
                    self.search_query = None;
                    self.marked_text = None;
                    self.swallowed_overlay_key = Some(KeyCode::Escape);
                    self.send_view_action(cx, TerminalViewAction::SearchClose);
                }
                KeyCode::Enter => {
                    if self.search_prompt_behavior == SearchPromptBehavior::AcceptAndClose {
                        self.search_query = None;
                        self.marked_text = None;
                        self.swallowed_overlay_key = Some(KeyCode::Enter);
                    } else {
                        let backward = self
                            .search_query
                            .as_ref()
                            .is_some_and(|query| query.direction == SearchDirection::Backward)
                            ^ modifiers.shift();
                        self.cancel_local_scroll(cx);
                        self.send_view_action(
                            cx,
                            if backward {
                                TerminalViewAction::SearchPrevious
                            } else {
                                TerminalViewAction::SearchNext
                            },
                        );
                    }
                }
                KeyCode::Backspace => {
                    let query = {
                        let query = self.search_query.as_mut().expect("checked above");
                        query.text.pop();
                        query.clone()
                    };
                    self.send_view_action(cx, TerminalViewAction::SearchUpdate(query));
                }
                KeyCode::Character('r' | 'R') if modifiers.alt() => {
                    let query = {
                        let query = self.search_query.as_mut().expect("checked above");
                        query.mode = match query.mode {
                            SearchMode::Literal => SearchMode::Regex,
                            SearchMode::Regex => SearchMode::Literal,
                        };
                        query.clone()
                    };
                    self.send_view_action(cx, TerminalViewAction::SearchUpdate(query));
                }
                KeyCode::Character('c' | 'C') if modifiers.alt() => {
                    let query = {
                        let query = self.search_query.as_mut().expect("checked above");
                        query.case = match query.case {
                            SearchCase::Smart => SearchCase::Sensitive,
                            SearchCase::Sensitive => SearchCase::Insensitive,
                            SearchCase::Insensitive => SearchCase::Smart,
                        };
                        query.clone()
                    };
                    self.send_view_action(cx, TerminalViewAction::SearchUpdate(query));
                }
                _ => {}
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if chrome == Some(ChromeAction::TerminalCopy) {
            self.copy_selection(cx, ClipboardTarget::Clipboard);
            cx.stop_propagation();
            return;
        }
        if chrome == Some(ChromeAction::TerminalSelectAll) {
            self.send_view_action(cx, TerminalViewAction::SelectAll);
            cx.stop_propagation();
            return;
        }
        if chrome == Some(ChromeAction::TerminalClearHistory) {
            if !self.command_output {
                self.send_view_action(cx, TerminalViewAction::ClearHistory);
            }
            cx.stop_propagation();
            return;
        }
        if chrome == Some(ChromeAction::TerminalPaste) {
            let item = cx.read_from_clipboard();
            if let Some(text) = item.as_ref().and_then(ClipboardItem::text) {
                self.request_paste(text, cx);
            } else if let Some(item) = item {
                if self.pane_is_remote(cx) {
                    self.upload_clipboard_image(item, cx);
                } else if self.snapshot_clipboard_image(item, cx) {
                    self.mux.read(cx).send_input(InputMessage::Key {
                        pane: self.pane,
                        input: KeyInput {
                            action: KeyAction::Press,
                            key: KeyCode::Character('v'),
                            modifiers: Modifiers::new(false, true, false, false),
                            text: None,
                            unshifted_codepoint: Some('v'),
                        },
                        text_follows: false,
                    });
                }
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.control()
            && !modifiers.shift()
            && !modifiers.alt()
            && !modifiers.platform()
            && matches!(code, KeyCode::Character('v' | 'V'))
            && let Some(item) = cx.read_from_clipboard()
        {
            if self.pane_is_remote(cx) {
                if self.upload_clipboard_image(item, cx) {
                    cx.stop_propagation();
                    return;
                }
            } else {
                self.snapshot_clipboard_image(item, cx);
            }
        }
        if modifiers.shift() && matches!(code, KeyCode::PageUp | KeyCode::PageDown) {
            let pages = if matches!(code, KeyCode::PageUp) {
                -1
            } else {
                1
            };
            if self.should_use_local_scroll() {
                let rows = i64::from(self.retained.read().viewport.rows);
                self.scroll_locally_by(i64::from(pages).saturating_mul(rows), cx);
            } else {
                self.send_view_action(cx, TerminalViewAction::ScrollPages(pages));
            }
            cx.stop_propagation();
            return;
        }
        let printable = matches!(code, KeyCode::Character(_));
        let raw_key = self.retained.read().viewport.kitty_keyboard
            || !printable
            || modifiers.control()
            || modifiers.alt()
            || modifiers.platform();

        if raw_key {
            self.cancel_local_scroll(cx);
            self.forwarded_keys.insert(event.keystroke.key.clone());
            self.mux.read(cx).send_input(InputMessage::Key {
                pane: self.pane,
                input: key_input(
                    &event.keystroke,
                    code,
                    if event.is_held {
                        KeyAction::Repeat
                    } else {
                        KeyAction::Press
                    },
                ),
                text_follows: false,
            });
            cx.stop_propagation();
        }
    }

    fn on_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
        let code = key_code(&event.keystroke.key);
        let forwarded_press = self.forwarded_keys.remove(&event.keystroke.key);
        if self.popup {
            if forwarded_press {
                self.mux.read(cx).send_input(InputMessage::Popup {
                    action: PopupAction::Key {
                        input: key_input(&event.keystroke, code, KeyAction::Release),
                        text_follows: false,
                    },
                });
            }
            cx.stop_propagation();
            return;
        }
        if self.swallowed_overlay_key == Some(code) {
            self.swallowed_overlay_key = None;
            cx.stop_propagation();
            return;
        }
        if self.search_query.is_some() {
            cx.stop_propagation();
            return;
        }
        if forwarded_press && self.retained.read().viewport.kitty_keyboard {
            self.mux.read(cx).send_input(InputMessage::Key {
                pane: self.pane,
                input: key_input(&event.keystroke, code, KeyAction::Release),
                text_follows: false,
            });
        }
    }

    fn status_message(&self) -> Option<String> {
        let retained = self.retained.read();
        match &retained.viewport.status {
            SessionStatus::Starting | SessionStatus::Running => None,
            SessionStatus::Exited(exit) => Some(exit.signal.as_ref().map_or_else(
                || format!("shell exited ({})", exit.code),
                |signal| format!("shell exited: {signal}"),
            )),
            SessionStatus::Failed(error) => Some(error.as_ref().clone()),
        }
    }

    fn cursor_should_blink(&self) -> bool {
        cursor_should_blink(
            self.retained.read().viewport.cursor,
            self.render_appearance.source.cursor_blink_policy,
            self.cursor_focused,
        )
    }

    fn reset_cursor_blink(&mut self, cx: &mut Context<Self>) {
        let cursor_was_hidden = !self.cursor_blink_visible;
        self.cursor_blink_visible = true;
        self.cursor_blink_task = Task::ready(());
        self.cursor_blink_active = false;
        if self.cursor_should_blink() {
            self.start_cursor_blink(cx);
        }
        if cursor_was_hidden {
            cx.notify();
        }
    }

    fn ensure_cursor_blink(&mut self, cx: &mut Context<Self>) {
        if !self.cursor_should_blink() {
            self.cursor_blink_visible = true;
            if self.cursor_blink_active {
                self.cursor_blink_task = Task::ready(());
                self.cursor_blink_active = false;
            }
            return;
        }
        if self.cursor_blink_active {
            return;
        }
        self.start_cursor_blink(cx);
    }

    fn start_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_blink_active = true;
        let interval = Duration::from_millis(u64::from(
            self.render_appearance.source.cursor_blink_interval_ms,
        ));
        self.cursor_blink_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let Ok(keep_running) = this.update(cx, |view, cx| {
                    if !view.cursor_should_blink() {
                        view.cursor_blink_visible = true;
                        view.cursor_blink_active = false;
                        return false;
                    }
                    view.cursor_blink_visible = !view.cursor_blink_visible;
                    cx.notify();
                    true
                }) else {
                    break;
                };
                if !keep_running {
                    break;
                }
            }
        });
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.kitty_images.write().take_retired() {
            if let Err(error) = window.drop_image(image.clone()) {
                log::warn!("failed to release superseded Kitty image: {error}");
            }
            if let Ok(image) = Arc::try_unwrap(image) {
                drop(image.into_frames());
            }
        }
        let focused = window.is_window_active() && self.focus_handle.is_focused(window);
        if focused != self.cursor_focused {
            self.cursor_focused = focused;
            self.cursor_blink_visible = true;
            self.cursor_blink_task = Task::ready(());
            self.cursor_blink_active = false;
        }
        self.ensure_cursor_blink(cx);
        let appearance = Arc::clone(&self.render_appearance.source);
        let font = self.render_appearance.font.clone();
        let appearance_hash = self.render_appearance.stable_hash;
        let font_size = terminal_font_size(&appearance);
        let line_height = terminal_line_height(&appearance);
        let retained = self.retained();
        let marked_text = self.marked_text();
        let status = (!self.popup).then(|| self.status_message()).flatten();
        let search_query = self.search_query.clone();
        let (mode, unseen_output, search_status, hovered_uri, viewport_background) = {
            let state = retained.read();
            (
                state.viewport.mode,
                state.viewport.unseen_output,
                state.viewport.search,
                state.viewport.presentation.hovered_uri.clone(),
                state.viewport.background,
            )
        };
        let background = terminal_background(viewport_background, appearance.background_opacity);
        let mode_indicator = (!self.popup)
            .then(|| mode_indicator(mode, unseen_output))
            .flatten();
        let mut root = round_div_radii(
            div()
                .id("terminal-root")
                .relative()
                .flex()
                .size_full()
                .when(!self.popup, |terminal| {
                    terminal
                        .pl(px(appearance.padding_left))
                        .pr(px(appearance.padding_right))
                        .pt(px(appearance.padding_top))
                        .pb(px(appearance.padding_bottom))
                })
                .overflow_hidden()
                .bg(background)
                .font(font.clone())
                .text_size(font_size)
                .line_height(line_height)
                .key_context(TERMINAL_KEY_CONTEXT)
                .track_focus(&self.focus_handle)
                .on_key_down(cx.listener(Self::on_key_down))
                .on_key_up(cx.listener(Self::on_key_up))
                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
                .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
                .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up_out))
                .on_mouse_up_out(MouseButton::Right, cx.listener(Self::on_mouse_up_out))
                .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::on_mouse_up_out))
                .on_mouse_move(cx.listener(Self::on_mouse_move))
                .on_mouse_exit(cx.listener(Self::on_mouse_exit))
                .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
                .on_scroll_wheel(cx.listener(Self::on_scroll))
                .child(TerminalElement::new(
                    cx.entity(),
                    retained,
                    Arc::clone(&self.kitty_images),
                    Rc::clone(&self.row_cache),
                    Arc::clone(&appearance),
                    appearance_hash,
                    self.text_opacity,
                    self.cursor_blink_visible,
                    search_query.is_none().then_some(marked_text).flatten(),
                )),
            pane_content_radii(cx, self.window_corners),
        );

        if hovered_uri.is_some() {
            root = root.cursor_pointer();
        }

        if let Some(mode_indicator) = mode_indicator {
            let indicator = terminal_mode_tag(mode_indicator.label, mode_indicator.detail)
                .absolute()
                .right(px(8.0))
                .top(px(8.0));
            root = root.child(indicator);
        }

        let mut bottom_right: Vec<AnyElement> = Vec::new();
        if let Some(uri) = hovered_uri.filter(|uri| pasted_image_number(uri).is_none()) {
            bottom_right.push(terminal_link_popup(presented_uri(&uri), cx).into_any_element());
        }
        if let Some(status) = status {
            bottom_right.push(terminal_status_popup(status, cx).into_any_element());
        }
        if let Some(query) = search_query {
            let marked = self.marked_text.as_deref().unwrap_or_default();
            let (prompt, _) = search_prompt_text(&query, marked, search_status);
            bottom_right.push(terminal_search_prompt(prompt, cx).into_any_element());
        }
        if !bottom_right.is_empty() {
            root = root.child(pane_overlay_stack(
                PaneOverlayCorner::BottomRight,
                bottom_right,
            ));
        }
        if let Some(popover) = self.image_hover_popover(cx) {
            root = root.child(popover);
        }
        root
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let marked = self.marked_text.as_ref()?;
        let length = marked.encode_utf16().count();
        adjusted_range.replace(0..length);
        (range.start <= length).then(|| marked.clone())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
        if self.marked_text.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reset_cursor_blink(cx);
        self.marked_text = None;
        if !text.is_empty() {
            if let Some(query) = self.search_query.as_mut() {
                query.text.push_str(text);
                let query = query.clone();
                self.cancel_local_scroll(cx);
                self.send_view_action(cx, TerminalViewAction::SearchUpdate(query));
            } else {
                self.cancel_local_scroll(cx);
                self.mux.read(cx).send_input(if self.popup {
                    InputMessage::Popup {
                        action: PopupAction::Text(text.to_owned()),
                    }
                } else {
                    terminal_text_input(self.pane, text)
                });
            }
        }
        window.invalidate_character_coordinates();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reset_cursor_blink(cx);
        self.marked_text = (!new_text.is_empty()).then(|| new_text.to_owned());
        window.invalidate_character_coordinates();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.cursor_bounds
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}

pub(crate) fn key_input(keystroke: &Keystroke, key: KeyCode, action: KeyAction) -> KeyInput {
    let character = match key {
        KeyCode::Character(character) => Some(character),
        _ => None,
    };
    let text = keystroke
        .key_char
        .clone()
        .or_else(|| character.map(|character| character.to_string()))
        .filter(|text| !text.chars().any(char::is_control))
        .map(String::into_boxed_str);
    KeyInput {
        action,
        key,
        modifiers: modifiers(keystroke.modifiers),
        text,
        unshifted_codepoint: character,
    }
}

const fn terminal_mouse_button(button: MouseButton) -> TerminalMouseButton {
    match button {
        MouseButton::Left | MouseButton::Navigate(_) => TerminalMouseButton::Left,
        MouseButton::Right => TerminalMouseButton::Right,
        MouseButton::Middle => TerminalMouseButton::Middle,
    }
}

const fn mouse_button_bit(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 1 << 0,
        MouseButton::Right => 1 << 1,
        MouseButton::Middle => 1 << 2,
        MouseButton::Navigate(_) => 1 << 3,
    }
}

const fn forwarded_mouse_press(buttons: u8, button: MouseButton) -> bool {
    buttons & mouse_button_bit(button) != 0
}

const fn owns_pressed_button(buttons: u8, pressed: Option<MouseButton>) -> bool {
    match pressed {
        Some(button) => forwarded_mouse_press(buttons, button),
        None => true,
    }
}

const fn selection_has_extent(anchor: PointerCellEvent, pointer: PointerCellEvent) -> bool {
    anchor.click_count >= 2 || anchor.column != pointer.column || anchor.row != pointer.row
}

fn scrollbar_strip_hit(grid: HitGrid, scrollbar: ScrollbarState, position: Point<Pixels>) -> bool {
    scrollbar.total > scrollbar.len
        && position.x >= grid.surface_bounds.right() - zz_ui::scroll::GUTTER_WIDTH
        && position.x <= grid.surface_bounds.right()
        && position.y >= grid.surface_bounds.origin.y
        && position.y <= grid.surface_bounds.bottom()
}

fn modifiers(value: gpui::Modifiers) -> Modifiers {
    Modifiers::new(value.shift, value.control, value.alt, value.platform)
}

fn single_character(key: &str) -> Option<char> {
    let mut characters = key.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

fn cursor_should_blink(
    cursor: Option<zz_terminal::Cursor>,
    policy: CursorBlinkPolicy,
    focused: bool,
) -> bool {
    focused
        && cursor.is_some_and(|cursor| {
            cursor.visible()
                && match policy {
                    CursorBlinkPolicy::Off => false,
                    CursorBlinkPolicy::On => true,
                    CursorBlinkPolicy::Terminal => cursor.blinking(),
                }
        })
}

const fn should_paste_primary(command_output: bool, mouse_tracking: bool, shift: bool) -> bool {
    !command_output && (shift || !mouse_tracking)
}

fn read_primary_paste(cx: &App) -> Option<String> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let item = cx.read_from_primary();
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    let item = cx.read_from_clipboard();
    item.and_then(|item| item.text())
}

pub(crate) fn key_code(key: &str) -> KeyCode {
    match key {
        "space" => KeyCode::Character(' '),
        "backspace" => KeyCode::Backspace,
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "escape" => KeyCode::Escape,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        _ => single_character(key).map_or_else(
            || {
                key.strip_prefix('f')
                    .and_then(|number| number.parse::<u8>().ok())
                    .map_or(KeyCode::Unidentified, KeyCode::Function)
            },
            KeyCode::Character,
        ),
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "terminal view tests use exact terminal appearance fixtures"
)]
mod tests {
    use super::*;

    gpui::actions!(terminal_view_test, [FocusNext, FocusPrevious]);

    #[test]
    fn maps_gpui_key_names() {
        assert_eq!(key_code("left"), KeyCode::ArrowLeft);
        assert_eq!(key_code("f12"), KeyCode::Function(12));
        assert_eq!(key_code("x"), KeyCode::Character('x'));
        assert_eq!(key_code("space"), KeyCode::Character(' '));
        assert_eq!(key_code("tab"), KeyCode::Tab);
    }

    #[test]
    fn space_release_preserves_its_unshifted_codepoint_for_kitty_encoding() {
        let keystroke = Keystroke {
            key: "space".to_owned(),
            key_char: Some(" ".to_owned()),
            modifiers: gpui::Modifiers::default(),
        };

        assert_eq!(
            key_input(&keystroke, KeyCode::Character(' '), KeyAction::Release),
            KeyInput {
                action: KeyAction::Release,
                key: KeyCode::Character(' '),
                modifiers: Modifiers::default(),
                text: Some(Box::from(" ")),
                unshifted_codepoint: Some(' '),
            }
        );
    }

    fn chrome_action(source: &str) -> Option<ChromeAction> {
        let keystroke = Keystroke::parse(source).expect("valid test keystroke");
        crate::keymap::test_resolve(TERMINAL_TABLE, &keystroke)
    }

    #[test]
    fn terminal_context_leaves_tab_for_raw_key_input() {
        let mut bindings = vec![
            KeyBinding::new("tab", FocusNext, Some("Root")),
            KeyBinding::new("shift-tab", FocusPrevious, Some("Root")),
        ];
        bindings.extend(raw_key_bindings());
        let keymap = gpui::Keymap::new(bindings);
        let root_context = gpui::KeyContext::parse("Root").expect("valid root key context");
        let terminal_context =
            gpui::KeyContext::parse(TERMINAL_KEY_CONTEXT).expect("valid terminal key context");

        for source in ["tab", "shift-tab"] {
            let keystroke = Keystroke::parse(source).expect("valid tab keystroke");
            let (terminal_bindings, pending) = keymap.bindings_for_input(
                std::slice::from_ref(&keystroke),
                &[root_context.clone(), terminal_context.clone()],
            );
            assert!(terminal_bindings.is_empty());
            assert!(!pending);

            let (root_bindings, pending) = keymap.bindings_for_input(
                std::slice::from_ref(&keystroke),
                std::slice::from_ref(&root_context),
            );
            assert_eq!(root_bindings.len(), 1);
            assert!(!pending);
        }
    }

    #[test]
    fn terminal_font_shortcuts_accept_minus_equal_and_shifted_plus() {
        for (source, action) in [
            ("ctrl--", ChromeAction::TerminalFontDecrease),
            ("ctrl-=", ChromeAction::TerminalFontIncrease),
            ("ctrl-+", ChromeAction::TerminalFontIncrease),
            ("ctrl-shift-=", ChromeAction::TerminalFontIncrease),
        ] {
            assert_eq!(chrome_action(source), Some(action), "{source}");
        }
    }

    #[test]
    fn terminal_chrome_leaves_every_other_chord_to_the_pane() {
        for source in ["-", "ctrl-alt-=", "cmd-=", "ctrl-c", "ctrl-f", "ctrl-v"] {
            assert_eq!(chrome_action(source), None, "{source}");
        }
    }

    #[test]
    fn terminal_chrome_claims_the_shifted_control_chords() {
        for (source, action) in [
            ("ctrl-shift-f", ChromeAction::TerminalSearch),
            ("ctrl-shift-c", ChromeAction::TerminalCopy),
            ("ctrl-shift-a", ChromeAction::TerminalSelectAll),
            ("ctrl-shift-k", ChromeAction::TerminalClearHistory),
            ("ctrl-shift-v", ChromeAction::TerminalPaste),
            ("cmd-f", ChromeAction::TerminalSearch),
            ("cmd-c", ChromeAction::TerminalCopy),
            ("cmd-a", ChromeAction::TerminalSelectAll),
            ("cmd-k", ChromeAction::TerminalClearHistory),
            ("cmd-v", ChromeAction::TerminalPaste),
        ] {
            assert_eq!(chrome_action(source), Some(action), "{source}");
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    #[test]
    fn terminal_font_zoom_suppresses_application_ui_scaling() {
        let mut bindings =
            crate::ui_scale::key_bindings(&crate::keymap::test_chords(zz_client::UI_TABLE));
        bindings.extend(terminal_key_bindings(&crate::keymap::test_chords(
            TERMINAL_TABLE,
        )));
        let keymap = gpui::Keymap::new(bindings);
        let contexts = [
            gpui::KeyContext::parse(zz_ui::ROOT_KEY_CONTEXT).expect("valid zz root context"),
            gpui::KeyContext::parse(TERMINAL_KEY_CONTEXT).expect("valid terminal context"),
        ];

        for source in ["ctrl-=", "ctrl-+", "ctrl--"] {
            let keystroke = Keystroke::parse(source).expect("valid terminal zoom shortcut");
            let (bindings, pending) = keymap.bindings_for_input(&[keystroke], &contexts);
            assert!(!pending);
            assert!(
                bindings.is_empty(),
                "{source} must stay on the terminal's raw key path"
            );
        }
    }

    #[test]
    fn mode_indicator_uses_tags_without_bracket_decoration() {
        assert_eq!(
            mode_indicator(
                TerminalMode::Copy {
                    position: 42,
                    total: 900,
                    hide_position: false,
                },
                0,
            ),
            Some(ModeIndicator {
                label: Some("COPY MODE"),
                detail: "42/900".to_owned(),
            })
        );
        assert_eq!(
            mode_indicator(
                TerminalMode::View {
                    position: 7,
                    total: 12,
                },
                0,
            ),
            Some(ModeIndicator {
                label: Some("VIEW MODE"),
                detail: "7/12  ·  q close".to_owned(),
            })
        );
    }

    #[test]
    fn copy_mode_indicator_keeps_unseen_output_visible() {
        assert_eq!(
            mode_indicator(
                TerminalMode::Copy {
                    position: 4,
                    total: 20,
                    hide_position: false,
                },
                3,
            ),
            Some(ModeIndicator {
                label: Some("COPY MODE"),
                detail: "4/20  ·  +3 output".to_owned(),
            })
        );
        assert_eq!(mode_indicator(TerminalMode::Live, 0), None);
    }

    #[test]
    fn hide_position_drops_the_copy_position_and_keeps_the_rest() {
        assert_eq!(
            mode_indicator(
                TerminalMode::Copy {
                    position: 42,
                    total: 900,
                    hide_position: true,
                },
                0,
            ),
            Some(ModeIndicator {
                label: Some("COPY MODE"),
                detail: String::new(),
            })
        );
        assert_eq!(
            mode_indicator(
                TerminalMode::Copy {
                    position: 42,
                    total: 900,
                    hide_position: true,
                },
                3,
            ),
            Some(ModeIndicator {
                label: Some("COPY MODE"),
                detail: "+3 output".to_owned(),
            })
        );
    }

    #[test]
    fn local_scroll_gate_requires_live_untracked_scrollback_and_a_warm_ring() {
        let mut viewport = TerminalViewport::blank(80, 24, SessionStatus::Running);
        viewport.scrollbar = ScrollbarState {
            total: 100,
            offset: 76,
            len: 24,
        };
        assert!(local_scroll_gate(&viewport, 1));

        viewport.mode = TerminalMode::Copy {
            position: 1,
            total: 100,
            hide_position: false,
        };
        assert!(!local_scroll_gate(&viewport, 1));
        viewport.mode = TerminalMode::View {
            position: 1,
            total: 100,
        };
        assert!(!local_scroll_gate(&viewport, 1));
        viewport.mode = TerminalMode::Live;

        viewport.mouse_tracking = true;
        assert!(!local_scroll_gate(&viewport, 1));
        viewport.mouse_tracking = false;

        viewport.scrollbar.total = viewport.scrollbar.len;
        assert!(!local_scroll_gate(&viewport, 1));
        viewport.scrollbar.total = 100;
        assert!(!local_scroll_gate(&viewport, 0));
    }

    #[test]
    fn local_scroll_retires_on_convergence_timeout_or_ring_invalidation() {
        let started = Instant::now();
        let local_scroll = LocalScroll {
            target_offset: 40,
            started,
        };
        assert!(!local_scroll_should_retire(
            local_scroll,
            60,
            7,
            7,
            (started + LOCAL_SCROLL_TIMEOUT)
                .checked_sub(Duration::from_millis(1))
                .unwrap(),
        ));
        assert!(local_scroll_should_retire(local_scroll, 40, 7, 7, started,));
        assert!(local_scroll_should_retire(
            local_scroll,
            60,
            7,
            7,
            started + LOCAL_SCROLL_TIMEOUT,
        ));
        assert!(local_scroll_should_retire(local_scroll, 60, 7, 8, started,));
    }

    #[test]
    fn local_scroll_prefetches_only_near_the_cold_edge() {
        let scrollbar = ScrollbarState {
            total: 110,
            offset: 100,
            len: 10,
        };
        assert!(local_scroll_needs_prefetch(99, scrollbar, 20));
        assert!(!local_scroll_needs_prefetch(100, scrollbar, 20));
    }

    #[test]
    fn primary_paste_yields_to_application_mouse_and_rejects_output_views() {
        assert!(should_paste_primary(false, false, false));
        assert!(!should_paste_primary(false, true, false));
        assert!(should_paste_primary(false, true, true));
        assert!(!should_paste_primary(true, false, false));
        assert!(!should_paste_primary(true, true, true));
    }

    #[test]
    fn daemon_default_font_is_replaced_with_the_rendering_hosts_default() {
        let mut appearance = TerminalAppearance {
            font_families: vec!["Other OS Mono".to_owned()],
            ..TerminalAppearance::default()
        };

        localize_terminal_font_families_with_default(
            &mut appearance,
            &AppearanceProvenance::default(),
            &["Client Mono".to_owned()],
            "Client Mono",
        );

        assert_eq!(appearance.font_families, ["Client Mono"]);
    }

    #[test]
    fn installed_configured_fallback_is_promoted_over_a_missing_primary() {
        let mut appearance = TerminalAppearance {
            font_families: vec!["Missing Mono".to_owned(), "Available Mono".to_owned()],
            ..TerminalAppearance::default()
        };
        let mut provenance = AppearanceProvenance::default();
        provenance.set_source(AppearanceConfigKey::FontFamily, AppearanceSource::Override);

        localize_terminal_font_families_with_default(
            &mut appearance,
            &provenance,
            &["available mono".to_owned()],
            "Client Mono",
        );

        assert_eq!(appearance.font_families, ["Available Mono"]);
    }

    #[test]
    fn entirely_missing_stacks_fall_back_to_local_regular_and_inherit_for_styles() {
        let mut appearance = TerminalAppearance {
            font_families: vec!["Missing Regular".to_owned()],
            font_families_bold: vec!["Missing Bold".to_owned()],
            font_families_italic: vec!["Missing Italic".to_owned()],
            ..TerminalAppearance::default()
        };
        let mut provenance = AppearanceProvenance::default();
        for key in [
            AppearanceConfigKey::FontFamily,
            AppearanceConfigKey::FontFamilyBold,
            AppearanceConfigKey::FontFamilyItalic,
        ] {
            provenance.set_source(key, AppearanceSource::Override);
        }

        localize_terminal_font_families_with_default(
            &mut appearance,
            &provenance,
            &[],
            "Client Mono",
        );

        assert_eq!(appearance.font_families, ["Client Mono"]);
        assert!(appearance.font_families_bold.is_empty());
        assert!(appearance.font_families_italic.is_empty());
        assert_eq!(
            terminal_font_for_style(&appearance, true, false).family,
            "Client Mono",
        );
    }

    #[test]
    fn appearance_maps_primary_fallback_features_weight_and_metrics() {
        let appearance = TerminalAppearance {
            font_families: vec!["Primary Mono".to_owned(), "Emoji Fallback".to_owned()],
            font_features: vec![zz_terminal::FontFeature::new(*b"ss01", 2)],
            font_weight: 550,
            font_size_points: 12.0,
            cell_height_adjustment: zz_terminal::CellHeightAdjustment::Percent(25.0),
            ..TerminalAppearance::default()
        };

        let font = terminal_font(&appearance);
        assert_eq!(&*font.family, "Primary Mono");
        assert_eq!(font.weight, FontWeight(550.0));
        assert_eq!(
            font.features.tag_value_list(),
            [("liga".to_owned(), 1), ("ss01".to_owned(), 2)]
        );
        assert_eq!(
            font.fallbacks.expect("fallbacks").fallback_list(),
            ["Emoji Fallback"]
        );
        let (font_size, expanded_line_height, compact_line_height) = if cfg!(target_os = "macos") {
            (px(12.0), px(15.0), px(9.0))
        } else {
            (px(16.0), px(20.0), px(12.0))
        };
        assert_eq!(terminal_font_size(&appearance), font_size);
        assert_eq!(terminal_line_height(&appearance), expanded_line_height);

        let compact = TerminalAppearance {
            cell_height_adjustment: zz_terminal::CellHeightAdjustment::Percent(-25.0),
            ..appearance
        };
        assert_eq!(terminal_line_height(&compact), compact_line_height);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ghostty_font_points_reach_coretext_without_dpi_conversion() {
        let appearance = TerminalAppearance {
            font_size_points: 13.0,
            ..TerminalAppearance::default()
        };

        assert_eq!(terminal_font_size(&appearance), px(13.0));
    }

    #[test]
    fn default_terminal_font_enables_ghostty_ligatures_without_empty_fallbacks() {
        let appearance = TerminalAppearance::default();
        let font = terminal_font(&appearance);
        assert_eq!(font.features.tag_value_list(), [("liga".to_owned(), 1)]);
        assert!(font.fallbacks.is_none());
    }

    #[test]
    fn configured_ligature_override_follows_the_ghostty_default() {
        let appearance = TerminalAppearance {
            font_features: vec![zz_terminal::FontFeature::new(*b"liga", 0)],
            ..TerminalAppearance::default()
        };

        assert_eq!(
            terminal_font(&appearance).features.tag_value_list(),
            [("liga".to_owned(), 1), ("liga".to_owned(), 0)]
        );
    }

    #[test]
    fn styled_terminal_fonts_use_ghostty_face_stacks_and_regular_fallbacks() {
        let appearance = TerminalAppearance {
            font_families: vec!["Regular Mono".to_owned(), "Regular Symbols".to_owned()],
            font_families_bold: vec!["Bold Mono".to_owned(), "Bold Symbols".to_owned()],
            font_families_italic: vec!["Italic Mono".to_owned()],
            font_weight: 425,
            ..TerminalAppearance::default()
        };

        let bold = terminal_font_for_style(&appearance, true, false);
        assert_eq!(&*bold.family, "Bold Mono");
        assert_eq!(
            bold.fallbacks.expect("bold fallback").fallback_list(),
            ["Bold Symbols"]
        );
        assert_eq!(bold.weight, FontWeight(725.0));

        let italic = terminal_font_for_style(&appearance, false, true);
        assert_eq!(&*italic.family, "Italic Mono");
        assert_eq!(italic.style, FontStyle::Italic);

        let bold_italic = terminal_font_for_style(&appearance, true, true);
        assert_eq!(&*bold_italic.family, "Regular Mono");
        assert_eq!(
            bold_italic
                .fallbacks
                .expect("regular fallback")
                .fallback_list(),
            ["Regular Symbols"]
        );
        assert_eq!(bold_italic.weight, FontWeight(725.0));
        assert_eq!(bold_italic.style, FontStyle::Italic);
    }

    #[test]
    fn hovered_uri_presentation_is_bounded_on_character_boundaries() {
        let uri = format!("https://example.com/{}", "界".repeat(300));
        let presented = presented_uri(&uri);
        assert_eq!(presented.chars().count(), 241);
        assert!(presented.ends_with('…'));
        assert!(presented.is_char_boundary(presented.len()));
    }

    #[test]
    fn forwarded_mouse_buttons_keep_matching_releases_on_the_clamped_path() {
        let buttons = mouse_button_bit(MouseButton::Left) | mouse_button_bit(MouseButton::Right);
        assert!(forwarded_mouse_press(buttons, MouseButton::Left));
        assert!(forwarded_mouse_press(buttons, MouseButton::Right));
        assert!(!forwarded_mouse_press(buttons, MouseButton::Middle));
    }

    #[test]
    fn only_a_button_this_pane_pressed_carries_its_motion_and_release() {
        let mut buttons = 0;
        assert!(!owns_pressed_button(buttons, Some(MouseButton::Left)));
        assert!(!forwarded_mouse_press(buttons, MouseButton::Left));
        assert!(owns_pressed_button(buttons, None));

        buttons |= mouse_button_bit(MouseButton::Left);
        assert!(owns_pressed_button(buttons, Some(MouseButton::Left)));
        assert!(!owns_pressed_button(buttons, Some(MouseButton::Right)));
        assert!(owns_pressed_button(buttons, None));

        buttons &= !mouse_button_bit(MouseButton::Left);
        assert!(!owns_pressed_button(buttons, Some(MouseButton::Left)));
    }

    #[test]
    fn mouse_selection_needs_another_cell_unless_it_is_a_multi_click() {
        let anchor = PointerCellEvent {
            column: 4,
            row: 2,
            click_count: 1,
            rectangle: false,
        };
        assert!(!selection_has_extent(anchor, anchor));
        assert!(selection_has_extent(
            anchor,
            PointerCellEvent {
                column: 5,
                ..anchor
            }
        ));
        assert!(selection_has_extent(
            anchor,
            PointerCellEvent { row: 3, ..anchor }
        ));
        assert!(selection_has_extent(
            PointerCellEvent {
                click_count: 2,
                ..anchor
            },
            anchor
        ));
    }

    #[test]
    fn visible_scrollbar_strip_is_not_part_of_the_terminal_hit_grid() {
        let surface_bounds =
            Bounds::new(point(px(10.0), px(20.0)), gpui::size(px(650.0), px(460.0)));
        let grid = HitGrid {
            bounds: Bounds::new(surface_bounds.origin, gpui::size(px(640.0), px(456.0))),
            surface_bounds,
            columns: 80,
            rows: 24,
            cell_width: px(8.0),
            line_height: px(19.0),
        };
        let scrollable = ScrollbarState {
            total: 100,
            offset: 0,
            len: 24,
        };
        assert!(scrollbar_strip_hit(
            grid,
            scrollable,
            point(surface_bounds.right() - px(1.0), px(100.0))
        ));
        assert!(!scrollbar_strip_hit(
            grid,
            scrollable,
            point(surface_bounds.right() - px(17.0), px(100.0))
        ));
        assert!(!scrollbar_strip_hit(
            grid,
            ScrollbarState {
                total: 24,
                offset: 0,
                len: 24,
            },
            point(surface_bounds.right() - px(1.0), px(100.0))
        ));
    }

    #[test]
    fn search_ime_layout_tracks_the_exact_unicode_query_without_a_length_clamp() {
        let query = SearchQuery {
            text: "界".repeat(96),
            ..SearchQuery::default()
        };
        let (prompt, caret) = search_prompt_text(&query, "編集中", None);
        let committed = format!("Find: {}", query.text);
        assert_eq!(&prompt[..caret], committed);
        assert!(caret > 72);
        assert!(prompt[caret..].starts_with("編集中"));
    }

    #[test]
    fn terminal_surface_uses_the_live_viewport_background_as_a_tint() {
        let live = zz_terminal::Color::rgb(0x12, 0x34, 0x56);
        assert_eq!(
            terminal_background(live, 1.0),
            appearance_hsla(AppearanceColor::rgba(0x12, 0x34, 0x56, 255))
        );
        assert_eq!(
            terminal_background(live, 0.5),
            appearance_hsla(AppearanceColor::rgba(0x12, 0x34, 0x56, 128))
        );
    }

    #[test]
    fn cursor_blinking_is_suspended_while_unfocused() {
        let cursor = zz_terminal::Cursor::new(
            0,
            0,
            true,
            true,
            false,
            zz_terminal::CursorStyle::Block,
            zz_terminal::Color::rgb(1, 2, 3),
        );
        assert!(cursor_should_blink(
            Some(cursor),
            CursorBlinkPolicy::Terminal,
            true
        ));
        assert!(!cursor_should_blink(
            Some(cursor),
            CursorBlinkPolicy::Terminal,
            false
        ));
        assert!(!cursor_should_blink(
            Some(cursor),
            CursorBlinkPolicy::Off,
            true
        ));
    }
}
