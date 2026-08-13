use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, hash_map::Entry},
    hash::{BuildHasherDefault, Hasher},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, Element, ElementId, ElementInputHandler,
    Entity, Font, FontSmoothing, FontStyle, FontWeight, GlobalElementId, GlyphRasterData,
    GlyphRenderOptions, Hsla, InspectorElementId, IntoElement, LayoutId, PaintQuad, Path,
    PathBuilder, Pixels, Point, RenderImage, Rgba, ShapedLine, StrikethroughStyle, TextAlign,
    TextRun, UnderlineStyle, Window, fill, point, px, quad, size,
};
use parking_lot::RwLock;
use smallvec::SmallVec;
use zz_terminal::{
    AppearanceColor, CellWidth, Color, Cursor, CursorStyle, GRAPHEME_TABLE_BIT, Glyph, KittyLayer,
    KittyPlacement, OVERLAY_RECTANGLE, OverlayKind, PackedCell, PackedStyle, ScrollbarState,
    TerminalAppearance, TerminalDictionary, TerminalViewport,
    UnderlineStyle as TerminalUnderlineStyle,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _,
    scroll::{MIN_THUMB_SIZE, THUMB_INSET, THUMB_WIDTH, thumb_radius},
};

use crate::{
    diagnostics,
    mux::client::{KittyImageCache, RetainedTerminalViewport},
    pane,
    terminal::view::{GridSize, TerminalView, terminal_font_for_style},
};

const DIAGNOSTIC_TARGET: &str = "zz::diagnostics::terminal_render";

pub(crate) struct TerminalElement {
    view: Entity<TerminalView>,
    retained: Arc<RwLock<RetainedTerminalViewport>>,
    kitty_images: Arc<RwLock<KittyImageCache>>,
    row_cache: Rc<RefCell<RowRenderCache>>,
    appearance: Arc<TerminalAppearance>,
    appearance_hash: u64,
    text_opacity: f32,
    cursor_blink_visible: bool,
    marked_text: Option<String>,
}

impl TerminalElement {
    pub(crate) fn new(
        view: Entity<TerminalView>,
        retained: Arc<RwLock<RetainedTerminalViewport>>,
        kitty_images: Arc<RwLock<KittyImageCache>>,
        row_cache: Rc<RefCell<RowRenderCache>>,
        appearance: Arc<TerminalAppearance>,
        appearance_hash: u64,
        text_opacity: f32,
        cursor_blink_visible: bool,
        marked_text: Option<String>,
    ) -> Self {
        Self {
            view,
            retained,
            kitty_images,
            row_cache,
            appearance,
            appearance_hash,
            text_opacity,
            cursor_blink_visible,
            marked_text,
        }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct PositionedTextRow {
    row: Rc<CachedTextRow>,
    origin: Point<Pixels>,
}

struct PositionedLine {
    line: ShapedLine,
    origin: Point<Pixels>,
    glyph_options: GlyphRenderOptions,
}

struct PositionedKittyImage {
    visible_bounds: Bounds<Pixels>,
    image_bounds: Bounds<Pixels>,
    image: Arc<RenderImage>,
}

struct CachedLine {
    line: ShapedLine,
    start_column: usize,
    glyph_options: GlyphRenderOptions,
    raster: RefCell<RasterState>,
}

enum RasterState {
    Untried,
    Cached(GlyphRasterData),
    SlowPathOnly,
}

#[derive(Clone, Copy)]
struct CachedBackground {
    start_column: usize,
    cell_count: usize,
    color: Color,
}

#[derive(Clone, Copy)]
struct BackgroundBatch {
    start_column: usize,
    color: Color,
}

#[derive(Clone, Copy)]
struct CachedOverline {
    start_column: usize,
    cell_count: usize,
    color: Color,
    alpha: f32,
}

#[derive(Clone, Copy)]
struct CachedUnderline {
    start_column: usize,
    cell_count: usize,
    color: Color,
    alpha: f32,
    kind: TerminalUnderlineStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoxDrawingEdges(u8);

impl BoxDrawingEdges {
    const UP: u8 = 1 << 0;
    const RIGHT: u8 = 1 << 1;
    const DOWN: u8 = 1 << 2;
    const LEFT: u8 = 1 << 3;

    const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    const fn connects(self, edge: u8) -> bool {
        self.0 & edge != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoxDiagonals(u8);

impl BoxDiagonals {
    const FALLING: u8 = 1 << 0;
    const RISING: u8 = 1 << 1;

    const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    const fn contains(self, diagonal: u8) -> bool {
        self.0 & diagonal != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LightBoxDrawing {
    Segments(BoxDrawingEdges),
    Rounded(BoxDrawingEdges),
    Diagonals(BoxDiagonals),
}

#[derive(Clone, Copy)]
struct CachedBoxConnector {
    column: usize,
    drawing: LightBoxDrawing,
    color: Color,
    alpha: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SolidBlockElement {
    Rect {
        left_eighths: u8,
        top_eighths: u8,
        right_eighths: u8,
        bottom_eighths: u8,
    },
    Quadrants(u8),
}

impl SolidBlockElement {
    const TOP_LEFT: u8 = 1 << 0;
    const TOP_RIGHT: u8 = 1 << 1;
    const BOTTOM_LEFT: u8 = 1 << 2;
    const BOTTOM_RIGHT: u8 = 1 << 3;
}

#[derive(Clone, Copy)]
struct CachedSolidBlock {
    column: usize,
    element: SolidBlockElement,
    color: Color,
    alpha: f32,
}

enum TerminalGraphicPaint {
    Quad(PaintQuad),
    Path { path: Path<Pixels>, color: Hsla },
}

#[derive(Default)]
struct CachedTextRow {
    backgrounds: Vec<CachedBackground>,
    box_connectors: Vec<CachedBoxConnector>,
    solid_blocks: Vec<CachedSolidBlock>,
    lines: Vec<CachedLine>,
    overlines: Vec<CachedOverline>,
    underlines: Vec<CachedUnderline>,
}

#[derive(Clone, PartialEq)]
struct CellMetricsSignature {
    scale_bits: u32,
    font: Font,
    font_size: Pixels,
}

#[derive(Clone, Copy)]
struct CellMetrics {
    width: Pixels,
    line_height: Pixels,
    box_stroke: Pixels,
}

struct CachedCellMetrics {
    signature: CellMetricsSignature,
    metrics: CellMetrics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SelectionCacheKey {
    revision: u64,
    start: u16,
    end: u16,
}

#[derive(Clone, PartialEq)]
struct RowCacheSignature {
    dictionary_generation: u32,
    scale_bits: u32,
    font: Font,
    font_size: Pixels,
    cell_width: Pixels,
    foreground: Color,
    background: Color,
    appearance_hash: u64,
    text_opacity_bits: u32,
}

#[derive(Default)]
struct RevisionHasher(u64);

impl Hasher for RevisionHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = hash;
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

type RevisionMap<T> = HashMap<u64, T, BuildHasherDefault<RevisionHasher>>;
type RevisionSet = HashSet<u64, BuildHasherDefault<RevisionHasher>>;

#[derive(Default)]
pub(crate) struct RowRenderCache {
    signature: Option<RowCacheSignature>,
    revision_epoch: Option<u64>,
    rows: RevisionMap<Rc<CachedTextRow>>,
    selection_rows: HashMap<SelectionCacheKey, Rc<CachedTextRow>>,
    live_selection_keys: HashSet<SelectionCacheKey>,
    live_revisions: RevisionSet,
    cell_metrics: Option<CachedCellMetrics>,
    paint: PaintBuffers,
}

impl RowRenderCache {
    fn cell_metrics(
        &mut self,
        signature: CellMetricsSignature,
        probe_color: Hsla,
        window: &mut Window,
    ) -> CellMetrics {
        if let Some(metrics) = self.cell_metrics.as_ref()
            && metrics.signature == signature
        {
            return metrics.metrics;
        }
        let probe_run = TextRun {
            len: 1,
            font: signature.font.clone(),
            color: probe_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let probe =
            window
                .text_system()
                .shape_line("m".into(), signature.font_size, &[probe_run], None);
        let width = if f32::from(probe.width) > 1.0 {
            probe.width
        } else {
            px(8.0)
        };
        let font_id = probe.runs.first().map_or_else(
            || window.text_system().resolve_font(&signature.font),
            |run| run.font_id,
        );
        let line_height = (probe.ascent
            + probe.descent
            + window.text_system().line_gap(font_id, signature.font_size))
        .max(px(1.0));
        let box_stroke = box_stroke_width(
            window
                .text_system()
                .underline_thickness(font_id, signature.font_size),
            f32::from_bits(signature.scale_bits),
        );
        let metrics = CellMetrics {
            width,
            line_height,
            box_stroke,
        };
        self.cell_metrics = Some(CachedCellMetrics { signature, metrics });
        metrics
    }

    fn prepare(
        &mut self,
        signature: RowCacheSignature,
        revision_epoch: u64,
        revisions: impl Iterator<Item = u64>,
    ) {
        if self.signature.as_ref() != Some(&signature) {
            if let Some(previous) = self.signature.as_ref() {
                log::trace!(
                    target: "zz::diagnostics::terminal_render",
                    "row_cache_invalidate previous_scale_bits={} next_scale_bits={} previous_appearance_hash={} next_appearance_hash={} previous_dictionary_generation={} next_dictionary_generation={}",
                    previous.scale_bits,
                    signature.scale_bits,
                    previous.appearance_hash,
                    signature.appearance_hash,
                    previous.dictionary_generation,
                    signature.dictionary_generation,
                );
            }
            self.signature = Some(signature);
            self.rows.clear();
            self.selection_rows.clear();
            self.live_selection_keys.clear();
            self.revision_epoch = None;
            self.live_revisions.clear();
        }
        if self.revision_epoch == Some(revision_epoch) {
            return;
        }

        self.live_revisions.clear();
        self.live_revisions.extend(revisions);
        let live_revisions = &self.live_revisions;
        self.rows
            .retain(|revision, _| live_revisions.contains(revision));
        self.selection_rows
            .retain(|key, _| live_revisions.contains(&key.revision));
        self.revision_epoch = Some(revision_epoch);
    }
}

#[derive(Default)]
struct PaintBuffers {
    kitty_below_bg: Vec<PositionedKittyImage>,
    backgrounds: Vec<PaintQuad>,
    overlays: Vec<PaintQuad>,
    box_connectors: Vec<TerminalGraphicPaint>,
    kitty_below_text: Vec<PositionedKittyImage>,
    cursor: Vec<PaintQuad>,
    text: Vec<PositionedTextRow>,
    decorations: Vec<PaintQuad>,
    kitty_above_text: Vec<PositionedKittyImage>,
}

impl PaintBuffers {
    fn clear(&mut self) {
        self.kitty_below_bg.clear();
        self.backgrounds.clear();
        self.overlays.clear();
        self.box_connectors.clear();
        self.kitty_below_text.clear();
        self.cursor.clear();
        self.text.clear();
        self.decorations.clear();
        self.kitty_above_text.clear();
    }
}

pub(crate) struct PaintState {
    buffers: PaintBuffers,
    cursor_glyph: Option<PositionedLine>,
    composition: Option<PositionedLine>,
    cell_width: Pixels,
    line_height: Pixels,
}

struct TextBatch {
    text: String,
    start_column: usize,
    cell_count: usize,
    runs: SmallVec<[TextRun; 4]>,
    last_style: Option<PackedStyle>,
    last_foreground: Option<Color>,
    font_style: TerminalFontStyle,
    glyph_options: GlyphRenderOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalFontStyle {
    bold: bool,
    italic: bool,
}

impl TerminalFontStyle {
    fn from_packed(style: PackedStyle) -> Self {
        Self {
            bold: style.bold(),
            italic: style.italic(),
        }
    }
}

#[derive(Clone, Copy)]
struct CursorCellPaint {
    column: usize,
    row: usize,
    width: usize,
    cell: PackedCell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowProjection {
    source_start: u16,
    destination_start: u16,
    count: u16,
}

impl RowProjection {
    fn new(viewport_rows: u16, grid_rows: u16, bottom_anchored: bool) -> Self {
        let count = viewport_rows.min(grid_rows);
        if bottom_anchored {
            Self {
                source_start: viewport_rows.saturating_sub(count),
                destination_start: grid_rows.saturating_sub(count),
                count,
            }
        } else {
            Self {
                source_start: 0,
                destination_start: 0,
                count,
            }
        }
    }

    fn source_row(self, offset: u16) -> Option<u16> {
        (offset < self.count).then(|| self.source_start.saturating_add(offset))
    }

    fn display_row(self, source_row: u16) -> Option<usize> {
        let offset = source_row.checked_sub(self.source_start)?;
        (offset < self.count).then(|| usize::from(self.destination_start.saturating_add(offset)))
    }
}

fn kitty_display_row(
    placement: &KittyPlacement,
    local_scroll_target: Option<u32>,
    projection: RowProjection,
) -> Option<i64> {
    let anchor = if let Some(target) = local_scroll_target {
        i64::try_from(i128::from(placement.absolute_row) - i128::from(target)).ok()?
    } else {
        i64::from(placement.viewport_row)
    };
    let placement_end = anchor.checked_add(i64::from(placement.grid_rows))?;
    let projected_start = i64::from(projection.source_start);
    let projected_end = projected_start.checked_add(i64::from(projection.count))?;
    let first_visible = anchor.max(projected_start);
    if first_visible >= placement_end || first_visible >= projected_end {
        return None;
    }
    let projected_source = u16::try_from(first_visible).ok()?;
    let display = i64::try_from(projection.display_row(projected_source)?).ok()?;
    display.checked_sub(first_visible.checked_sub(anchor)?)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "terminal image geometry is bounded viewport/pixel metadata converted to GPUI floats"
)]
fn collect_kitty_images(
    viewport: &TerminalViewport,
    cache: &KittyImageCache,
    local_scroll_target: Option<u32>,
    row_projection: RowProjection,
    origin: Point<Pixels>,
    grid_bounds: Bounds<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    scale: f32,
    output: &mut PaintBuffers,
) {
    if scale <= 0.0 {
        return;
    }
    for placement in viewport.kitty_placements.iter() {
        let Some(image) = cache.image(placement.image_id, placement.image_generation) else {
            continue;
        };
        let Some(display_row) = kitty_display_row(placement, local_scroll_target, row_projection)
        else {
            continue;
        };
        if placement.pixel_width == 0
            || placement.pixel_height == 0
            || placement.grid_cols == 0
            || placement.grid_rows == 0
        {
            continue;
        }
        let destination = Bounds::new(
            point(
                origin.x
                    + px(f32::from(cell_width) * placement.viewport_col as f32)
                    + px(placement.cell_offset_x as f32 / scale),
                origin.y
                    + px(f32::from(line_height) * display_row as f32)
                    + px(placement.cell_offset_y as f32 / scale),
            ),
            size(
                px(f32::from(cell_width) * placement.grid_cols as f32),
                px(f32::from(line_height) * placement.grid_rows as f32),
            ),
        );
        let destination_aspect =
            f32::from(destination.size.width) / f32::from(destination.size.height);
        let shipped_aspect = placement.pixel_width as f32 / placement.pixel_height as f32;
        if !destination_aspect.is_finite()
            || destination_aspect <= 0.0
            || !shipped_aspect.is_finite()
            || shipped_aspect <= 0.0
        {
            continue;
        }
        let visible_bounds = destination.intersect(&grid_bounds);
        if visible_bounds.size.width <= px(0.0) || visible_bounds.size.height <= px(0.0) {
            continue;
        }
        let image_bounds = if let Some((source_x, source_y, source_width, source_height)) =
            placement.source_rect
        {
            let image_size = image.size(0);
            let Ok(image_width) = u32::try_from(image_size.width.0) else {
                continue;
            };
            let Ok(image_height) = u32::try_from(image_size.height.0) else {
                continue;
            };
            if source_width == 0
                || source_height == 0
                || source_x
                    .checked_add(source_width)
                    .is_none_or(|right| right > image_width)
                || source_y
                    .checked_add(source_height)
                    .is_none_or(|bottom| bottom > image_height)
            {
                continue;
            }
            let x_scale = f32::from(destination.size.width) / source_width as f32;
            let y_scale = f32::from(destination.size.height) / source_height as f32;
            Bounds::new(
                point(
                    destination.origin.x - px(source_x as f32 * x_scale),
                    destination.origin.y - px(source_y as f32 * y_scale),
                ),
                size(
                    px(image_width as f32 * x_scale),
                    px(image_height as f32 * y_scale),
                ),
            )
        } else {
            destination
        };
        let positioned = PositionedKittyImage {
            visible_bounds,
            image_bounds,
            image,
        };
        match placement.layer {
            KittyLayer::BelowBg => output.kitty_below_bg.push(positioned),
            KittyLayer::BelowText => output.kitty_below_text.push(positioned),
            KittyLayer::AboveText => output.kitty_above_text.push(positioned),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalRowSource {
    History(usize),
    Live(u16),
    Shimmer,
}

fn local_row_source(
    absolute_row: u32,
    server_offset: u32,
    live_rows: u16,
    history_rows: usize,
) -> LocalRowSource {
    if absolute_row >= server_offset {
        let live_row = absolute_row - server_offset;
        return u16::try_from(live_row)
            .ok()
            .filter(|row| *row < live_rows)
            .map_or(LocalRowSource::Shimmer, LocalRowSource::Live);
    }

    let position = server_offset - absolute_row;
    usize::try_from(position)
        .ok()
        .and_then(|position| history_rows.checked_sub(position))
        .map_or(LocalRowSource::Shimmer, LocalRowSource::History)
}

fn local_live_projection(
    target_offset: u32,
    server_offset: u32,
    viewport_rows: u16,
    grid_rows: u16,
) -> RowProjection {
    let display_count = u32::from(viewport_rows.min(grid_rows));
    let display_end = target_offset.saturating_add(display_count);
    let live_end = server_offset.saturating_add(u32::from(viewport_rows));
    let intersection_start = target_offset.max(server_offset);
    let intersection_end = display_end.min(live_end);
    if intersection_start >= intersection_end {
        return RowProjection {
            source_start: 0,
            destination_start: 0,
            count: 0,
        };
    }

    RowProjection {
        source_start: u16::try_from(intersection_start - server_offset)
            .expect("live projection is bounded by viewport rows"),
        destination_start: u16::try_from(intersection_start - target_offset)
            .expect("live projection is bounded by grid rows"),
        count: u16::try_from(intersection_end - intersection_start)
            .expect("live projection is bounded by viewport rows"),
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = PaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (pane::fill_parent(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let retained = self.retained.read();
        let viewport = &retained.viewport;
        let row_revisions = &retained.row_revisions;
        let row_revision_epoch = retained.row_revision_epoch;
        let (command_output, local_scroll_target) = {
            let view = self.view.read(cx);
            (view.is_command_output(), view.local_scroll_target())
        };
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let base_font = text_style.font();
        let scale = window.scale_factor();
        let CellMetrics {
            width: cell_width,
            line_height: natural_line_height,
            box_stroke,
        } = self.row_cache.borrow_mut().cell_metrics(
            CellMetricsSignature {
                scale_bits: scale.to_bits(),
                font: base_font.clone(),
                font_size,
            },
            color(viewport.foreground, 1.0),
            window,
        );
        let mut buffers = {
            let mut row_cache = self.row_cache.borrow_mut();
            let mut buffers = std::mem::take(&mut row_cache.paint);
            buffers.clear();
            buffers
        };
        let raw_line_height = px(self
            .appearance
            .cell_height_adjustment
            .apply(f32::from(natural_line_height))
            .max(1.0));
        let line_height = snap_length(raw_line_height, scale);
        let columns = grid_dimension(bounds.size.width, cell_width, scale);
        let rows = grid_dimension(bounds.size.height, line_height, scale);
        let spare_height = (bounds.size.height - line_height * usize::from(rows)).max(px(0.0));
        let bottom_anchored = local_scroll_target.is_none()
            && matches!(viewport.mode, zz_terminal::TerminalMode::Live)
            && !command_output
            && viewport.search.is_none()
            && viewport
                .scrollbar
                .offset
                .saturating_add(viewport.scrollbar.len)
                >= viewport.scrollbar.total
            && last_visible_row_has_content(viewport, viewport.rows);
        let row_projection = RowProjection::new(viewport.rows, rows, bottom_anchored);
        let live_row_projection = local_scroll_target.map_or(row_projection, |target_offset| {
            local_live_projection(
                target_offset,
                viewport.scrollbar.offset,
                viewport.rows,
                rows,
            )
        });
        let origin = point(
            snap(bounds.origin.x, scale),
            snap(
                bounds.origin.y
                    + if bottom_anchored {
                        spare_height
                    } else {
                        px(0.0)
                    },
                scale,
            ),
        );
        let grid_bounds = terminal_grid_bounds(origin, columns, rows, cell_width, line_height);
        let cursor = viewport.cursor;
        let cursor_cell =
            cursor.and_then(|cursor| cursor_cell(viewport, cursor, columns, live_row_projection));
        let cursor_bounds = cursor_cell.map(|cursor| {
            Bounds::new(
                point(
                    origin.x + cell_width * cursor.column,
                    origin.y + line_height * cursor.row,
                ),
                size(cell_width * cursor.width, line_height),
            )
        });
        let composing = self
            .marked_text
            .as_ref()
            .is_some_and(|text| !text.is_empty());
        let composition = self.marked_text.as_ref().and_then(|marked_text| {
            if marked_text.is_empty() {
                return None;
            }
            let bounds = cursor_bounds?;
            let run = TextRun {
                len: marked_text.len(),
                font: base_font.clone(),
                color: color(viewport.foreground, self.text_opacity),
                background_color: None,
                underline: Some(UnderlineStyle {
                    thickness: device_pixel(scale),
                    color: Some(color(viewport.foreground, self.text_opacity)),
                    wavy: false,
                }),
                strikethrough: None,
            };
            Some(PositionedLine {
                line: window.text_system().shape_line(
                    marked_text.clone().into(),
                    font_size,
                    &[run],
                    None,
                ),
                origin: bounds.origin,
                glyph_options: base_glyph_render_options(&self.appearance),
            })
        });
        let hidden_composition_cells = composition.as_ref().and_then(|composition| {
            let cursor = cursor_cell?;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "positive shaped composition width is rounded up to a visible cell span"
            )]
            let shaped_cells = (f32::from(composition.line.width) / f32::from(cell_width))
                .ceil()
                .max(1.0) as usize;
            let width = cursor.width.max(shaped_cells);
            Some((
                cursor.row,
                cursor.column..cursor.column.saturating_add(width),
            ))
        });
        let grid_size = GridSize {
            columns,
            rows,
            cell_width_px: physical_pixels(cell_width, scale),
            cell_height_px: physical_pixels(line_height, scale),
        };
        let mut cached_row_hits = 0;
        let mut cached_row_misses = 0;
        let mut uncached_rows = 0;
        {
            let mut row_cache = self.row_cache.borrow_mut();
            row_cache.prepare(
                RowCacheSignature {
                    dictionary_generation: viewport.dictionary_generation,
                    scale_bits: scale.to_bits(),
                    font: base_font.clone(),
                    font_size,
                    cell_width,
                    foreground: viewport.foreground,
                    background: viewport.background,
                    appearance_hash: self.appearance_hash,
                    text_opacity_bits: self.text_opacity.to_bits(),
                },
                row_revision_epoch,
                retained
                    .history
                    .rows
                    .iter()
                    .filter(|_| local_scroll_target.is_some())
                    .map(|row| row.revision)
                    .chain(row_revisions.iter().copied()),
            );
            for offset in 0..row_projection.count {
                let source_row = row_projection
                    .source_row(offset)
                    .expect("projection offset is bounded by its count");
                let row_index = usize::from(row_projection.destination_start + offset);
                let source =
                    local_scroll_target.map_or(LocalRowSource::Live(source_row), |target_offset| {
                        local_row_source(
                            target_offset.saturating_add(u32::from(source_row)),
                            viewport.scrollbar.offset,
                            viewport.rows,
                            retained.history.rows.len(),
                        )
                    });
                let resolved = match source {
                    LocalRowSource::Live(live_row) => Some((
                        viewport.row(live_row).unwrap_or_default(),
                        viewport.dictionary.as_ref(),
                        row_revisions.get(usize::from(live_row)).copied(),
                        true,
                    )),
                    LocalRowSource::History(history_row) => {
                        retained.history.rows.get(history_row).map(|row| {
                            (
                                row.cells.as_ref(),
                                row.dictionary.as_ref(),
                                Some(row.revision),
                                false,
                            )
                        })
                    }
                    LocalRowSource::Shimmer => None,
                };
                let Some((row, dictionary, revision, is_live)) = resolved else {
                    uncached_rows += 1;
                    buffers.backgrounds.push(fill(
                        Bounds::new(
                            point(origin.x, origin.y + line_height * row_index),
                            size(cell_width * usize::from(columns), line_height),
                        ),
                        color(viewport.foreground, 0.04),
                    ));
                    continue;
                };
                let hidden_columns = hidden_composition_cells
                    .as_ref()
                    .filter(|(hidden_row, _)| is_live && *hidden_row == row_index)
                    .map(|(_, columns)| columns.clone());
                if hidden_columns.is_some() {
                    uncached_rows += 1;
                    let masked = Rc::new(shape_text_row(
                        row,
                        dictionary,
                        viewport.foreground,
                        viewport.background,
                        &self.appearance,
                        cell_width,
                        font_size,
                        scale,
                        &base_font,
                        self.text_opacity,
                        None,
                        hidden_columns.as_ref(),
                        window,
                    ));
                    position_text_row(
                        row_index,
                        &masked,
                        origin,
                        cell_width,
                        line_height,
                        box_stroke,
                        scale,
                        &mut buffers,
                    );
                    continue;
                }
                if let Some(revision) = revision {
                    let cached: &Rc<CachedTextRow> = match row_cache.rows.entry(revision) {
                        Entry::Occupied(entry) => {
                            cached_row_hits += 1;
                            entry.into_mut()
                        }
                        Entry::Vacant(entry) => {
                            cached_row_misses += 1;
                            entry.insert(Rc::new(shape_text_row(
                                row,
                                dictionary,
                                viewport.foreground,
                                viewport.background,
                                &self.appearance,
                                cell_width,
                                font_size,
                                scale,
                                &base_font,
                                self.text_opacity,
                                None,
                                None,
                                window,
                            )))
                        }
                    };
                    position_text_row(
                        row_index,
                        cached,
                        origin,
                        cell_width,
                        line_height,
                        box_stroke,
                        scale,
                        &mut buffers,
                    );
                } else {
                    uncached_rows += 1;
                    let cached = Rc::new(shape_text_row(
                        row,
                        dictionary,
                        viewport.foreground,
                        viewport.background,
                        &self.appearance,
                        cell_width,
                        font_size,
                        scale,
                        &base_font,
                        self.text_opacity,
                        None,
                        None,
                        window,
                    ));
                    position_text_row(
                        row_index,
                        &cached,
                        origin,
                        cell_width,
                        line_height,
                        box_stroke,
                        scale,
                        &mut buffers,
                    );
                }
            }
        }
        {
            let mut row_cache = self.row_cache.borrow_mut();
            collect_selection_text(
                viewport,
                row_revisions,
                &self.appearance,
                live_row_projection,
                hidden_composition_cells.as_ref(),
                origin,
                cell_width,
                line_height,
                box_stroke,
                font_size,
                scale,
                &base_font,
                self.text_opacity,
                window,
                &mut row_cache,
                &mut buffers.text,
                &mut buffers.box_connectors,
            );
        }
        collect_kitty_images(
            viewport,
            &self.kitty_images.read(),
            local_scroll_target,
            row_projection,
            origin,
            grid_bounds,
            cell_width,
            line_height,
            scale,
            &mut buffers,
        );
        let focus = self.view.read(cx).focus();
        let link_hover_bounds = collect_overlays(
            viewport,
            &self.appearance,
            live_row_projection,
            columns,
            origin,
            cell_width,
            line_height,
            scale,
            &mut buffers.overlays,
        );
        let scrollbar =
            local_scroll_target.map_or(viewport.scrollbar, |target_offset| ScrollbarState {
                total: viewport.scrollbar.total,
                offset: target_offset,
                len: viewport.scrollbar.len,
            });
        push_scrollbar_quad(scrollbar, bounds, cx, &mut buffers.overlays);
        let focused = focus.is_focused(window);
        let cursor_visible =
            cursor_visible_for_paint(cursor, composing, focused, self.cursor_blink_visible);
        let mut cursor_bounds = cursor_bounds;
        let mut cursor_glyph = None;
        if let Some(mut visible_cursor_bounds) = cursor_bounds.filter(|_| cursor_visible) {
            let cursor = cursor.expect("cursor bounds require a cursor");
            if focused && matches!(cursor.style(), CursorStyle::Block) {
                cursor_glyph = cursor_cell.and_then(|cursor_cell| {
                    cursor_glyph_line(
                        viewport,
                        cursor_cell,
                        visible_cursor_bounds.origin,
                        font_size,
                        &base_font,
                        &self.appearance,
                        window,
                    )
                });
                if let Some(glyph) = cursor_glyph.as_ref() {
                    visible_cursor_bounds.size.width =
                        visible_cursor_bounds.size.width.max(glyph.line.width);
                    cursor_bounds = Some(visible_cursor_bounds);
                }
            }
            buffers.cursor.push(cursor_quad(
                visible_cursor_bounds,
                cursor.style(),
                cursor.color(),
                focused,
                scale,
            ));
        }

        let input_bounds = composition.as_ref().map_or(cursor_bounds, |composition| {
            Some(Bounds::new(
                composition.origin,
                size(composition.line.width.max(cell_width), line_height),
            ))
        });
        let search_cursor_bounds = self
            .view
            .read(cx)
            .search_ime_layout(viewport.search)
            .and_then(|(text, caret)| {
                search_ime_bounds(
                    bounds,
                    &self.appearance,
                    &text,
                    caret,
                    &base_font,
                    line_height,
                    viewport.foreground,
                    window,
                )
            });
        self.view.update(cx, |view, cx| {
            view.update_geometry(
                grid_size,
                grid_bounds,
                bounds,
                cell_width,
                line_height,
                input_bounds,
                search_cursor_bounds,
                link_hover_bounds,
                cx,
            );
        });

        if started.is_some() {
            let cache = self.row_cache.borrow();
            log::trace!(
                target: "zz::diagnostics::terminal_render",
                "prepaint bounds={bounds:?} scale_factor={} grid={grid_size:?} origin={origin:?} raw_line_height={} snapped_line_height={} spare_height={} bottom_anchored={} appearance_hash={} viewport_generation={} viewport_view_generation={} viewport_dictionary_generation={} viewport_columns={} viewport_rows={} viewport_cells={} viewport_overlays={} row_revision_epoch={} row_revisions={} cached_row_hits={} cached_row_misses={} uncached_rows={} cache_rows={} cache_selection_rows={} cache_live_revisions={} cache_paint_backgrounds_capacity={} cache_paint_overlays_capacity={} cache_paint_box_connectors_capacity={} cache_paint_cursor_capacity={} cache_paint_text_capacity={} cache_paint_decorations_capacity={} prepared_backgrounds={} prepared_overlays={} prepared_box_connectors={} prepared_cursor={} prepared_text_rows={} prepared_decorations={} cursor_style={:?} cursor_width={} blink_policy={:?} composition={} cursor_suppressed_by_composition={} elapsed_us={}",
                scale,
                f32::from(raw_line_height),
                f32::from(line_height),
                f32::from(spare_height),
                bottom_anchored,
                self.appearance_hash,
                viewport.generation,
                viewport.view_generation,
                viewport.dictionary_generation,
                viewport.columns,
                viewport.rows,
                viewport.cells.len(),
                viewport.overlays.len(),
                row_revision_epoch,
                row_revisions.len(),
                cached_row_hits,
                cached_row_misses,
                uncached_rows,
                cache.rows.len(),
                cache.selection_rows.len(),
                cache.live_revisions.len(),
                cache.paint.backgrounds.capacity(),
                cache.paint.overlays.capacity(),
                cache.paint.box_connectors.capacity(),
                cache.paint.cursor.capacity(),
                cache.paint.text.capacity(),
                cache.paint.decorations.capacity(),
                buffers.backgrounds.len(),
                buffers.overlays.len(),
                buffers.box_connectors.len(),
                buffers.cursor.len(),
                buffers.text.len(),
                buffers.decorations.len(),
                viewport.cursor.map(Cursor::style),
                cursor_cell.map_or(0, |cursor| cursor.width),
                self.appearance.cursor_blink_policy,
                composition.is_some(),
                composing,
                diagnostics::elapsed_us(started),
            );
        }

        PaintState {
            buffers,
            cursor_glyph,
            composition,
            cell_width,
            line_height,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        paint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let kitty_below_bg_count = paint.buffers.kitty_below_bg.len();
        let kitty_below_text_count = paint.buffers.kitty_below_text.len();
        let kitty_above_text_count = paint.buffers.kitty_above_text.len();
        let background_count = paint.buffers.backgrounds.len();
        let overlay_count = paint.buffers.overlays.len();
        let box_connector_count = paint.buffers.box_connectors.len();
        let cursor_count = paint.buffers.cursor.len();
        let text_count = paint.buffers.text.len();
        let decoration_count = paint.buffers.decorations.len();
        let had_composition = paint.composition.is_some();
        let focus = self.view.read(cx).focus();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            paint_kitty_images(&mut paint.buffers.kitty_below_bg, window);
            for quad in paint.buffers.backgrounds.drain(..) {
                window.paint_quad(quad);
            }
            for quad in paint.buffers.overlays.drain(..) {
                window.paint_quad(quad);
            }
            for graphic in paint.buffers.box_connectors.drain(..) {
                match graphic {
                    TerminalGraphicPaint::Quad(quad) => window.paint_quad(quad),
                    TerminalGraphicPaint::Path { path, color } => window.paint_path(path, color),
                }
            }
            paint_kitty_images(&mut paint.buffers.kitty_below_text, window);
            for row in paint.buffers.text.drain(..) {
                for line in &row.row.lines {
                    let origin = point(
                        row.origin.x + paint.cell_width * line.start_column,
                        row.origin.y,
                    );
                    let should_compute_raster = {
                        let raster = line.raster.borrow();
                        match &*raster {
                            RasterState::Cached(data) => {
                                if line.line.paint_with_raster_data(
                                    data,
                                    origin,
                                    paint.line_height,
                                    window,
                                ) {
                                    continue;
                                }
                                true
                            }
                            RasterState::Untried => true,
                            RasterState::SlowPathOnly => false,
                        }
                    };
                    if should_compute_raster {
                        *line.raster.borrow_mut() = RasterState::Untried;
                    }
                    if let Err(error) = line.line.paint_with_options(
                        origin,
                        paint.line_height,
                        TextAlign::Left,
                        None,
                        line.glyph_options,
                        window,
                        cx,
                    ) {
                        log::error!("failed to paint terminal text: {error}");
                    }
                    if should_compute_raster {
                        match line.line.compute_glyph_raster_data(
                            origin,
                            paint.line_height,
                            line.glyph_options,
                            window,
                            cx,
                        ) {
                            Ok(Some(data)) => {
                                *line.raster.borrow_mut() = RasterState::Cached(data);
                            }
                            Ok(None) => {
                                *line.raster.borrow_mut() = RasterState::SlowPathOnly;
                            }
                            Err(error) => {
                                log::trace!(
                                    target: "zz::diagnostics::terminal_render",
                                    "failed to compute terminal glyph raster data: {error}"
                                );
                            }
                        }
                    }
                }
            }
            for quad in paint.buffers.decorations.drain(..) {
                window.paint_quad(quad);
            }
            paint_kitty_images(&mut paint.buffers.kitty_above_text, window);
            for quad in paint.buffers.cursor.drain(..) {
                window.paint_quad(quad);
            }
            if let Some(cursor_glyph) = paint.cursor_glyph.take()
                && let Err(error) = cursor_glyph.line.paint_with_options(
                    cursor_glyph.origin,
                    paint.line_height,
                    TextAlign::Left,
                    None,
                    cursor_glyph.glyph_options,
                    window,
                    cx,
                )
            {
                log::error!("failed to paint terminal cursor glyph: {error}");
            }
            if let Some(composition) = paint.composition.take()
                && let Err(error) = composition.line.paint_with_options(
                    composition.origin,
                    paint.line_height,
                    TextAlign::Left,
                    None,
                    composition.glyph_options,
                    window,
                    cx,
                )
            {
                log::error!("failed to paint IME composition: {error}");
            }
        });
        self.row_cache.borrow_mut().paint = std::mem::take(&mut paint.buffers);
        log::trace!(
            target: "zz::diagnostics::terminal_render",
            "paint bounds={bounds:?} focused={} kitty_below_bg={} backgrounds={} overlays={} box_connectors={} kitty_below_text={} cursors={} text_rows={} decorations={} kitty_above_text={} composition={} elapsed_us={}",
            focus.is_focused(window),
            kitty_below_bg_count,
            background_count,
            overlay_count,
            box_connector_count,
            kitty_below_text_count,
            cursor_count,
            text_count,
            decoration_count,
            kitty_above_text_count,
            had_composition,
            diagnostics::elapsed_us(started),
        );
    }
}

fn paint_kitty_images(images: &mut Vec<PositionedKittyImage>, window: &mut Window) {
    for image in images.drain(..) {
        if let Err(error) = window.paint_image(
            image.visible_bounds,
            image.image_bounds,
            Corners::default(),
            image.image,
            0,
            false,
        ) {
            log::warn!("failed to paint Kitty terminal image: {error}");
        }
    }
}

fn collect_overlays(
    viewport: &TerminalViewport,
    appearance: &TerminalAppearance,
    row_projection: RowProjection,
    grid_columns: u16,
    origin: Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    scale: f32,
    output: &mut Vec<PaintQuad>,
) -> Option<Bounds<Pixels>> {
    let mut link_hover_bounds = None;
    for span in viewport
        .overlays
        .iter()
        .filter(|span| span.kind() == OverlayKind::CopyCursor)
    {
        let Some(display_row) = row_projection.display_row(span.row) else {
            continue;
        };
        output.push(fill(
            Bounds::new(
                point(origin.x, origin.y + line_height * display_row),
                size(cell_width * usize::from(grid_columns), line_height),
            ),
            copy_cursor_line_color(appearance),
        ));
    }

    for (index, span) in viewport.overlays.iter().enumerate() {
        let Some(display_row) = row_projection.display_row(span.row) else {
            continue;
        };
        if span.start >= span.end {
            continue;
        }
        let start = usize::from(span.start.min(viewport.columns));
        let end = usize::from(span.end.min(viewport.columns));
        let bounds = Bounds::new(
            point(
                origin.x + cell_width * start,
                origin.y + line_height * display_row,
            ),
            size(cell_width * end.saturating_sub(start), line_height),
        );
        let mut quad = fill(bounds, overlay_color(span.kind(), appearance));
        if appearance.rounded_selection
            && span.kind() == OverlayKind::Selection
            && span.flags() & OVERLAY_RECTANGLE == 0
        {
            quad = quad.corner_radii(selection_corners(
                viewport,
                index,
                device_pixel(scale) * 3.0,
            ));
        }
        output.push(quad);
        if span.kind() == OverlayKind::LinkHover {
            link_hover_bounds = Some(bounds);
            output.push(fill(
                Bounds::new(
                    point(bounds.origin.x, bounds.bottom() - device_pixel(scale)),
                    size(bounds.size.width, device_pixel(scale)),
                ),
                color(appearance.link_color, 1.0),
            ));
        }
    }
    link_hover_bounds
}

#[allow(
    clippy::disallowed_methods,
    reason = "terminal overlay colors come from the independent terminal appearance color system"
)]
fn overlay_color(kind: OverlayKind, appearance: &TerminalAppearance) -> Hsla {
    match kind {
        OverlayKind::Selection => appearance_color(appearance.selection_background),
        OverlayKind::SearchMatch => appearance_color(appearance.search_match_color),
        OverlayKind::SearchCurrent => appearance_color(appearance.search_current_color),
        OverlayKind::LinkHover => appearance_color(AppearanceColor::rgba(
            appearance.link_color.r,
            appearance.link_color.g,
            appearance.link_color.b,
            72,
        )),
        OverlayKind::CopyCursor => appearance_color(appearance.copy_cursor_color),
    }
}

fn copy_cursor_line_color(appearance: &TerminalAppearance) -> Hsla {
    let mut color = appearance_color(appearance.copy_cursor_color);
    color.a *= 0.25;
    color
}

#[allow(
    clippy::cast_precision_loss,
    reason = "scrollbar ratios are normalized and only used for subpixel paint geometry"
)]
fn push_scrollbar_quad(
    scrollbar: ScrollbarState,
    bounds: Bounds<Pixels>,
    cx: &App,
    output: &mut Vec<PaintQuad>,
) {
    if scrollbar.total <= scrollbar.len || scrollbar.total == 0 {
        return;
    }
    let track_height = bounds.size.height - THUMB_INSET * 2.0;
    let ratio = scrollbar.len as f32 / scrollbar.total as f32;
    let thumb_height = (track_height * ratio).max(px(MIN_THUMB_SIZE));
    let travel = (track_height - thumb_height).max(px(0.0));
    let denominator = scrollbar.total.saturating_sub(scrollbar.len).max(1);
    let progress = scrollbar.offset as f32 / denominator as f32;
    let origin = point(
        bounds.right() - THUMB_WIDTH - THUMB_INSET,
        bounds.origin.y + THUMB_INSET + travel * progress,
    );
    output.push(
        fill(
            Bounds::new(origin, size(THUMB_WIDTH, thumb_height)),
            cx.theme().foreground.wash(),
        )
        .corner_radii(thumb_radius(THUMB_WIDTH, cx)),
    );
}

fn push_background(
    batch: &mut Option<BackgroundBatch>,
    column: usize,
    background: Color,
    default: Color,
    output: &mut Vec<CachedBackground>,
) {
    match *batch {
        Some(previous) if previous.color == background => {}
        Some(previous) => {
            push_background_run(previous, column, default, output);
            *batch = Some(BackgroundBatch {
                start_column: column,
                color: background,
            });
        }
        None => {
            *batch = Some(BackgroundBatch {
                start_column: column,
                color: background,
            });
        }
    }
}

fn finish_backgrounds(
    batch: Option<BackgroundBatch>,
    columns: usize,
    default: Color,
    output: &mut Vec<CachedBackground>,
) {
    if let Some(batch) = batch {
        push_background_run(batch, columns, default, output);
    }
}

fn push_background_run(
    batch: BackgroundBatch,
    end_column: usize,
    default: Color,
    output: &mut Vec<CachedBackground>,
) {
    if batch.color != default && batch.start_column < end_column {
        output.push(CachedBackground {
            start_column: batch.start_column,
            cell_count: end_column - batch.start_column,
            color: batch.color,
        });
    }
}

fn collect_selection_text(
    viewport: &TerminalViewport,
    row_revisions: &[u64],
    appearance: &TerminalAppearance,
    row_projection: RowProjection,
    hidden_composition_cells: Option<&(usize, Range<usize>)>,
    origin: Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    box_stroke: Pixels,
    font_size: Pixels,
    scale: f32,
    base_font: &Font,
    text_opacity: f32,
    window: &mut Window,
    row_cache: &mut RowRenderCache,
    text_output: &mut Vec<PositionedTextRow>,
    connector_output: &mut Vec<TerminalGraphicPaint>,
) {
    row_cache.live_selection_keys.clear();
    for span in viewport
        .overlays
        .iter()
        .filter(|span| span.kind() == OverlayKind::Selection)
    {
        let Some(display_row) = row_projection.display_row(span.row) else {
            continue;
        };
        if span.start >= span.end {
            continue;
        }
        let Some(row) = viewport.row(span.row) else {
            continue;
        };
        let start = usize::from(span.start.min(viewport.columns));
        let end = usize::from(span.end.min(viewport.columns));
        let Some(cells) = row.get(start..end).filter(|cells| !cells.is_empty()) else {
            continue;
        };
        let hidden_columns =
            selection_hidden_columns(display_row, start..end, hidden_composition_cells);
        let shape = |window: &mut Window| {
            Rc::new(shape_text_row(
                cells,
                viewport.dictionary.as_ref(),
                viewport.foreground,
                viewport.background,
                appearance,
                cell_width,
                font_size,
                scale,
                base_font,
                text_opacity,
                Some(appearance.selection_foreground),
                hidden_columns.as_ref(),
                window,
            ))
        };
        let selected = if hidden_columns.is_some() {
            shape(window)
        } else if let Some(revision) = row_revisions.get(usize::from(span.row)).copied() {
            let key = SelectionCacheKey {
                revision,
                start: span.start,
                end: span.end,
            };
            row_cache.live_selection_keys.insert(key);
            if let Some(cached) = row_cache.selection_rows.get(&key) {
                Rc::clone(cached)
            } else {
                let selected = shape(window);
                row_cache.selection_rows.insert(key, Rc::clone(&selected));
                selected
            }
        } else {
            shape(window)
        };
        push_box_drawing_connectors(
            &selected.box_connectors,
            origin.x + cell_width * start,
            origin.y + line_height * display_row,
            cell_width,
            line_height,
            box_stroke,
            scale,
            connector_output,
        );
        push_solid_blocks(
            &selected.solid_blocks,
            origin.x + cell_width * start,
            origin.y + line_height * display_row,
            cell_width,
            line_height,
            scale,
            connector_output,
        );
        if !selected.lines.is_empty() {
            text_output.push(PositionedTextRow {
                row: selected,
                origin: point(
                    origin.x + cell_width * start,
                    origin.y + line_height * display_row,
                ),
            });
        }
    }
    let live_selection_keys = &row_cache.live_selection_keys;
    row_cache
        .selection_rows
        .retain(|key, _| live_selection_keys.contains(key));
}

fn selection_hidden_columns(
    display_row: usize,
    selection: Range<usize>,
    hidden_composition_cells: Option<&(usize, Range<usize>)>,
) -> Option<Range<usize>> {
    hidden_composition_cells
        .filter(|(hidden_row, _)| *hidden_row == display_row)
        .and_then(|(_, hidden)| {
            let hidden_start = hidden.start.max(selection.start);
            let hidden_end = hidden.end.min(selection.end);
            (hidden_start < hidden_end).then(|| {
                hidden_start.saturating_sub(selection.start)
                    ..hidden_end.saturating_sub(selection.start)
            })
        })
}

fn shape_text_row(
    cells: &[PackedCell],
    dictionary: &TerminalDictionary,
    default_foreground: Color,
    default_background: Color,
    appearance: &TerminalAppearance,
    cell_width: Pixels,
    font_size: Pixels,
    scale: f32,
    base_font: &Font,
    text_opacity: f32,
    forced_foreground: Option<Color>,
    hidden_columns: Option<&Range<usize>>,
    window: &mut Window,
) -> CachedTextRow {
    let mut output = CachedTextRow::default();
    let mut background_batch = None;
    let mut batch: Option<TextBatch> = None;
    for (column, cell) in cells.iter().copied().enumerate() {
        let style = style_for_dictionary(dictionary, default_foreground, default_background, cell);
        let glyph = glyph_for_dictionary(dictionary, cell);
        let decorative = decorative_glyph(glyph);
        let foreground =
            forced_foreground.unwrap_or_else(|| resolved_foreground(style, appearance, decorative));
        push_background(
            &mut background_batch,
            column,
            style.background(),
            default_background,
            &mut output.backgrounds,
        );
        if hidden_columns.is_some_and(|hidden| hidden.contains(&column)) {
            flush_cached_batch(
                batch.take(),
                cell_width,
                font_size,
                window,
                &mut output.lines,
            );
            continue;
        }
        if style.overline() && !matches!(cell.width(), CellWidth::SpacerTail) {
            output.overlines.push(CachedOverline {
                start_column: column,
                cell_count: if matches!(cell.width(), CellWidth::Wide) {
                    2
                } else {
                    1
                },
                color: foreground,
                alpha: text_opacity,
            });
        }
        if matches!(
            style.underline(),
            TerminalUnderlineStyle::Double
                | TerminalUnderlineStyle::Dotted
                | TerminalUnderlineStyle::Dashed
        ) && !matches!(cell.width(), CellWidth::SpacerTail)
        {
            output.underlines.push(CachedUnderline {
                start_column: column,
                cell_count: if matches!(cell.width(), CellWidth::Wide) {
                    2
                } else {
                    1
                },
                color: style.underline_color().unwrap_or(foreground),
                alpha: text_opacity,
                kind: style.underline(),
            });
        }

        if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
            continue;
        }
        if matches!(glyph, Glyph::Empty) || style.invisible() {
            flush_cached_batch(
                batch.take(),
                cell_width,
                font_size,
                window,
                &mut output.lines,
            );
            continue;
        }
        if let Some(drawing) = connected_light_box_drawing(glyph) {
            output.box_connectors.push(CachedBoxConnector {
                column,
                drawing,
                color: foreground,
                alpha: terminal_text_alpha(style) * text_opacity,
            });
            let font_decorates_glyph = style.strikethrough()
                || style.hyperlink()
                || matches!(
                    style.underline(),
                    TerminalUnderlineStyle::Single | TerminalUnderlineStyle::Curly
                );
            if !font_decorates_glyph {
                flush_cached_batch(
                    batch.take(),
                    cell_width,
                    font_size,
                    window,
                    &mut output.lines,
                );
                continue;
            }
        }
        if let Some(element) = connected_solid_block_element(glyph) {
            output.solid_blocks.push(CachedSolidBlock {
                column,
                element,
                color: foreground,
                alpha: terminal_text_alpha(style) * text_opacity,
            });
            let font_decorates_glyph = style.strikethrough()
                || style.hyperlink()
                || matches!(
                    style.underline(),
                    TerminalUnderlineStyle::Single | TerminalUnderlineStyle::Curly
                );
            if !font_decorates_glyph {
                flush_cached_batch(
                    batch.take(),
                    cell_width,
                    font_size,
                    window,
                    &mut output.lines,
                );
                continue;
            }
        }

        let width = if matches!(cell.width(), CellWidth::Wide) {
            2
        } else {
            1
        };
        let font_style = TerminalFontStyle::from_packed(style);
        if let Glyph::Grapheme(text) = glyph {
            flush_cached_batch(
                batch.take(),
                cell_width,
                font_size,
                window,
                &mut output.lines,
            );
            let run = text_run(
                style,
                text.len(),
                base_font,
                foreground,
                appearance,
                scale,
                text_opacity,
            );
            let glyph_options = glyph_render_options(font_style, &run.font, appearance, window);
            let line =
                window
                    .text_system()
                    .shape_line(text.to_owned().into(), font_size, &[run], None);
            output.lines.push(CachedLine {
                line,
                start_column: column,
                glyph_options,
                raster: RefCell::new(RasterState::Untried),
            });
            continue;
        }

        let can_append = batch.as_ref().is_some_and(|batch| {
            batch.start_column + batch.cell_count == column && batch.font_style == font_style
        });
        if !can_append {
            flush_cached_batch(
                batch.take(),
                cell_width,
                font_size,
                window,
                &mut output.lines,
            );
            batch = Some(TextBatch {
                text: String::new(),
                start_column: column,
                cell_count: 0,
                runs: SmallVec::new(),
                last_style: None,
                last_foreground: None,
                font_style,
                glyph_options: glyph_render_options(
                    font_style,
                    &font_for_style(font_style, base_font, appearance),
                    appearance,
                    window,
                ),
            });
        }
        let batch = batch.as_mut().expect("created above");
        let text_start = batch.text.len();
        if let Glyph::Scalar(value) = glyph {
            batch.text.push(value);
        }
        if width == 2 {
            batch.text.push(' ');
        }
        let appended_len = batch.text.len() - text_start;
        push_cell_text_run(
            &mut batch.runs,
            &mut batch.last_style,
            &mut batch.last_foreground,
            style,
            foreground,
            appended_len,
            base_font,
            appearance,
            scale,
            text_opacity,
        );
        batch.cell_count += width;
    }

    finish_backgrounds(
        background_batch,
        cells.len(),
        default_background,
        &mut output.backgrounds,
    );
    flush_cached_batch(batch, cell_width, font_size, window, &mut output.lines);
    output
}

fn flush_cached_batch(
    batch: Option<TextBatch>,
    cell_width: Pixels,
    font_size: Pixels,
    window: &mut Window,
    output: &mut Vec<CachedLine>,
) {
    let Some(batch) = batch else {
        return;
    };
    let TextBatch {
        text,
        start_column,
        runs,
        glyph_options,
        ..
    } = batch;
    let line = window
        .text_system()
        .shape_line(text.into(), font_size, &runs, Some(cell_width));
    output.push(CachedLine {
        line,
        start_column,
        glyph_options,
        raster: RefCell::new(RasterState::Untried),
    });
}

fn push_text_run(runs: &mut SmallVec<[TextRun; 4]>, mut run: TextRun, len: usize) {
    run.len = len;
    if let Some(previous) = runs.last_mut()
        && same_visual_style(previous, &run)
    {
        previous.len += len;
    } else {
        runs.push(run);
    }
}

fn push_cell_text_run(
    runs: &mut SmallVec<[TextRun; 4]>,
    last_style: &mut Option<PackedStyle>,
    last_foreground: &mut Option<Color>,
    style: PackedStyle,
    foreground: Color,
    len: usize,
    base_font: &Font,
    appearance: &TerminalAppearance,
    scale: f32,
    text_opacity: f32,
) {
    if *last_style == Some(style)
        && *last_foreground == Some(foreground)
        && let Some(previous) = runs.last_mut()
    {
        previous.len += len;
        return;
    }

    push_text_run(
        runs,
        text_run(
            style,
            len,
            base_font,
            foreground,
            appearance,
            scale,
            text_opacity,
        ),
        len,
    );
    *last_style = Some(style);
    *last_foreground = Some(foreground);
}

fn position_text_row(
    row: usize,
    cached: &Rc<CachedTextRow>,
    origin: Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    box_stroke: Pixels,
    scale: f32,
    output: &mut PaintBuffers,
) {
    let y = origin.y + line_height * row;
    output
        .backgrounds
        .extend(cached.backgrounds.iter().map(|background| {
            fill(
                Bounds::new(
                    point(
                        origin.x + cell_width * background.start_column,
                        origin.y + line_height * row,
                    ),
                    size(cell_width * background.cell_count, line_height),
                ),
                color(background.color, 1.0),
            )
        }));
    push_box_drawing_connectors(
        &cached.box_connectors,
        origin.x,
        y,
        cell_width,
        line_height,
        box_stroke,
        scale,
        &mut output.box_connectors,
    );
    push_solid_blocks(
        &cached.solid_blocks,
        origin.x,
        y,
        cell_width,
        line_height,
        scale,
        &mut output.box_connectors,
    );
    if !cached.lines.is_empty() {
        output.text.push(PositionedTextRow {
            row: Rc::clone(cached),
            origin: point(origin.x, y),
        });
    }
    output
        .decorations
        .extend(cached.overlines.iter().map(|overline| {
            fill(
                Bounds::new(
                    point(origin.x + cell_width * overline.start_column, y),
                    size(cell_width * overline.cell_count, device_pixel(scale)),
                ),
                color(overline.color, overline.alpha),
            )
        }));
    for underline in &cached.underlines {
        push_explicit_underline(
            underline,
            origin.x,
            y,
            cell_width,
            line_height,
            scale,
            &mut output.decorations,
        );
    }
}

fn push_box_drawing_connectors(
    connectors: &[CachedBoxConnector],
    row_x: Pixels,
    row_y: Pixels,
    cell_width: Pixels,
    line_height: Pixels,
    stroke: Pixels,
    scale: f32,
    output: &mut Vec<TerminalGraphicPaint>,
) {
    for connector in connectors {
        let left = snap(row_x + cell_width * connector.column, scale);
        let right = snap(row_x + cell_width * (connector.column + 1), scale);
        let top = snap(row_y, scale);
        let bottom = snap(row_y + line_height, scale);
        let width = right - left;
        let height = bottom - top;
        let horizontal_span = snap_length(width * 0.5, scale);
        let vertical_span = snap_length(height * 0.5, scale);
        let vertical_x = snap(left + (width - stroke) * 0.5, scale);
        let horizontal_y = snap(top + (height - stroke) * 0.5, scale);
        let paint = |bounds| fill(bounds, color(connector.color, connector.alpha));
        let cell = Bounds::new(point(left, top), size(width, height));

        let edges = match connector.drawing {
            LightBoxDrawing::Rounded(edges) => {
                if let Some(path) = rounded_box_drawing_path(edges, cell, stroke, scale) {
                    output.push(TerminalGraphicPaint::Path {
                        path,
                        color: color(connector.color, connector.alpha),
                    });
                }
                continue;
            }
            LightBoxDrawing::Diagonals(diagonals) => {
                if let Some(path) = diagonal_box_drawing_path(diagonals, cell, stroke, scale) {
                    output.push(TerminalGraphicPaint::Path {
                        path,
                        color: color(connector.color, connector.alpha),
                    });
                }
                continue;
            }
            LightBoxDrawing::Segments(edges) => edges,
        };

        if edges.connects(BoxDrawingEdges::UP) && edges.connects(BoxDrawingEdges::DOWN) {
            output.push(TerminalGraphicPaint::Quad(paint(Bounds::new(
                point(vertical_x, top),
                size(stroke, height),
            ))));
        } else if edges.connects(BoxDrawingEdges::UP) {
            output.push(TerminalGraphicPaint::Quad(paint(Bounds::new(
                point(vertical_x, top),
                size(stroke, vertical_span),
            ))));
        } else if edges.connects(BoxDrawingEdges::DOWN) {
            output.push(TerminalGraphicPaint::Quad(paint(Bounds::new(
                point(vertical_x, bottom - vertical_span),
                size(stroke, vertical_span),
            ))));
        }

        if edges.connects(BoxDrawingEdges::LEFT) && edges.connects(BoxDrawingEdges::RIGHT) {
            output.push(TerminalGraphicPaint::Quad(paint(Bounds::new(
                point(left, horizontal_y),
                size(width, stroke),
            ))));
        } else if edges.connects(BoxDrawingEdges::LEFT) {
            output.push(TerminalGraphicPaint::Quad(paint(Bounds::new(
                point(left, horizontal_y),
                size(horizontal_span, stroke),
            ))));
        } else if edges.connects(BoxDrawingEdges::RIGHT) {
            output.push(TerminalGraphicPaint::Quad(paint(Bounds::new(
                point(right - horizontal_span, horizontal_y),
                size(horizontal_span, stroke),
            ))));
        }
    }
}

fn rounded_box_drawing_path(
    edges: BoxDrawingEdges,
    bounds: Bounds<Pixels>,
    stroke: Pixels,
    scale: f32,
) -> Option<Path<Pixels>> {
    const CURVE_CONTROL_FRACTION: f32 = 0.25;

    let connects_up = edges.connects(BoxDrawingEdges::UP);
    let connects_right = edges.connects(BoxDrawingEdges::RIGHT);
    let connects_down = edges.connects(BoxDrawingEdges::DOWN);
    let connects_left = edges.connects(BoxDrawingEdges::LEFT);
    if connects_up == connects_down || connects_left == connects_right {
        return None;
    }

    let vertical_direction = if connects_down { 1.0 } else { -1.0 };
    let horizontal_direction = if connects_right { 1.0 } else { -1.0 };
    let top = snap(bounds.origin.y, scale);
    let bottom = snap(bounds.bottom(), scale);
    let left = snap(bounds.origin.x, scale);
    let right = snap(bounds.right(), scale);
    let width = right - left;
    let height = bottom - top;
    let center = point(
        snap(left + (width - stroke) * 0.5, scale) + stroke * 0.5,
        snap(top + (height - stroke) * 0.5, scale) + stroke * 0.5,
    );
    let radius = width.min(height) * 0.5;
    let vertical_edge = if connects_down { bottom } else { top };
    let horizontal_edge = if connects_right { right } else { left };

    // Matches Ghostty's generated rounded box glyph.
    let mut path = PathBuilder::stroke(stroke);
    path.move_to(point(center.x, vertical_edge));
    path.line_to(point(center.x, center.y + radius * vertical_direction));
    path.cubic_bezier_to(
        point(center.x + radius * horizontal_direction, center.y),
        point(
            center.x,
            center.y + radius * CURVE_CONTROL_FRACTION * vertical_direction,
        ),
        point(
            center.x + radius * CURVE_CONTROL_FRACTION * horizontal_direction,
            center.y,
        ),
    );
    path.line_to(point(horizontal_edge, center.y));
    match path.build() {
        Ok(path) => Some(path),
        Err(error) => {
            log::error!("failed to build rounded terminal box path: {error}");
            None
        }
    }
}

// Ghostty's `box_thickness`: face underline thickness rounded up to a whole device pixel.
fn box_stroke_width(underline_thickness: Pixels, scale: f32) -> Pixels {
    px((f32::from(underline_thickness) * scale).ceil().max(1.0) / scale)
}

fn diagonal_box_drawing_path(
    diagonals: BoxDiagonals,
    bounds: Bounds<Pixels>,
    stroke: Pixels,
    scale: f32,
) -> Option<Path<Pixels>> {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    // Matches Ghostty's generated diagonals, overshot at each corner to fill the
    // notch where two cells' butt caps meet.
    let device = 1.0 / scale;
    let overshoot_x = px(0.5 * device * (width / height).min(1.0));
    let overshoot_y = px(0.5 * device * (height / width).min(1.0));
    let left = bounds.origin.x - overshoot_x;
    let right = bounds.right() + overshoot_x;
    let top = bounds.origin.y - overshoot_y;
    let bottom = bounds.bottom() + overshoot_y;

    let mut path = PathBuilder::stroke(stroke);
    if diagonals.contains(BoxDiagonals::FALLING) {
        path.move_to(point(left, top));
        path.line_to(point(right, bottom));
    }
    if diagonals.contains(BoxDiagonals::RISING) {
        path.move_to(point(right, top));
        path.line_to(point(left, bottom));
    }
    match path.build() {
        Ok(path) => Some(path),
        Err(error) => {
            log::error!("failed to build diagonal terminal box path: {error}");
            None
        }
    }
}

fn push_solid_blocks(
    blocks: &[CachedSolidBlock],
    row_x: Pixels,
    row_y: Pixels,
    cell_width: Pixels,
    line_height: Pixels,
    scale: f32,
    output: &mut Vec<TerminalGraphicPaint>,
) {
    for block in blocks {
        let left = snap(row_x + cell_width * block.column, scale);
        let right = snap(row_x + cell_width * (block.column + 1), scale);
        let top = snap(row_y, scale);
        let bottom = snap(row_y + line_height, scale);
        let width = right - left;
        let height = bottom - top;
        let paint_rect =
            |left_eighths: u8, top_eighths: u8, right_eighths: u8, bottom_eighths: u8| {
                let x0 = snap(left + width * (f32::from(left_eighths) / 8.0), scale);
                let y0 = snap(top + height * (f32::from(top_eighths) / 8.0), scale);
                let x1 = snap(left + width * (f32::from(right_eighths) / 8.0), scale);
                let y1 = snap(top + height * (f32::from(bottom_eighths) / 8.0), scale);
                TerminalGraphicPaint::Quad(fill(
                    Bounds::new(point(x0, y0), size(x1 - x0, y1 - y0)),
                    color(block.color, block.alpha),
                ))
            };

        match block.element {
            SolidBlockElement::Rect {
                left_eighths,
                top_eighths,
                right_eighths,
                bottom_eighths,
            } => output.push(paint_rect(
                left_eighths,
                top_eighths,
                right_eighths,
                bottom_eighths,
            )),
            SolidBlockElement::Quadrants(quadrants) => {
                for (flag, rect) in [
                    (SolidBlockElement::TOP_LEFT, (0, 0, 4, 4)),
                    (SolidBlockElement::TOP_RIGHT, (4, 0, 8, 4)),
                    (SolidBlockElement::BOTTOM_LEFT, (0, 4, 4, 8)),
                    (SolidBlockElement::BOTTOM_RIGHT, (4, 4, 8, 8)),
                ] {
                    if quadrants & flag != 0 {
                        output.push(paint_rect(rect.0, rect.1, rect.2, rect.3));
                    }
                }
            }
        }
    }
}

fn push_explicit_underline(
    underline: &CachedUnderline,
    row_x: Pixels,
    row_y: Pixels,
    cell_width: Pixels,
    line_height: Pixels,
    scale: f32,
    output: &mut Vec<PaintQuad>,
) {
    let stroke = device_pixel(scale);
    let start = row_x + cell_width * underline.start_column;
    let end = start + cell_width * underline.cell_count;
    let bottom = row_y + line_height - stroke;
    let paint_segment = |x: Pixels, y: Pixels, width: Pixels| {
        fill(
            Bounds::new(point(x, y), size(width, stroke)),
            color(underline.color, underline.alpha),
        )
    };

    match underline.kind {
        TerminalUnderlineStyle::Double => {
            output.push(paint_segment(start, bottom, end - start));
            output.push(paint_segment(
                start,
                (bottom - stroke * 2.0).max(row_y),
                end - start,
            ));
        }
        TerminalUnderlineStyle::Dotted | TerminalUnderlineStyle::Dashed => {
            let segment = if matches!(underline.kind, TerminalUnderlineStyle::Dotted) {
                stroke
            } else {
                stroke * 3.0
            };
            let gap = if matches!(underline.kind, TerminalUnderlineStyle::Dotted) {
                stroke
            } else {
                stroke * 2.0
            };
            let mut x = start;
            while x < end {
                output.push(paint_segment(x, bottom, segment.min(end - x)));
                x += segment + gap;
            }
        }
        _ => {}
    }
}

fn font_for_style(
    style: TerminalFontStyle,
    base_font: &Font,
    appearance: &TerminalAppearance,
) -> Font {
    let configured_families = match (style.bold, style.italic) {
        (true, true) => &appearance.font_families_bold_italic,
        (true, false) => &appearance.font_families_bold,
        (false, true) => &appearance.font_families_italic,
        (false, false) => return base_font.clone(),
    };
    if configured_families.is_empty() {
        let mut font = base_font.clone();
        if style.bold {
            font.weight = gpui::FontWeight((font.weight.0 + 300.0).min(gpui::FontWeight::BLACK.0));
        }
        if style.italic {
            font.style = gpui::FontStyle::Italic;
        }
        font
    } else {
        terminal_font_for_style(appearance, style.bold, style.italic)
    }
}

fn base_glyph_render_options(appearance: &TerminalAppearance) -> GlyphRenderOptions {
    GlyphRenderOptions {
        font_smoothing: if appearance.font_thicken {
            FontSmoothing::Enabled(appearance.font_thicken_strength)
        } else {
            FontSmoothing::Disabled
        },
        synthetic_bold: false,
        synthetic_italic: false,
    }
}

fn glyph_render_options(
    style: TerminalFontStyle,
    font: &Font,
    appearance: &TerminalAppearance,
    window: &mut Window,
) -> GlyphRenderOptions {
    let mut options = base_glyph_render_options(appearance);
    if !appearance
        .font_synthetic_style
        .allows(style.bold, style.italic)
    {
        return options;
    }

    let resolved = window.text_system().resolve_font(font);
    if style.bold {
        let mut without_bold = font.clone();
        without_bold.weight = FontWeight(f32::from(appearance.font_weight));
        options.synthetic_bold = resolved == window.text_system().resolve_font(&without_bold);
    }
    if style.italic {
        let mut without_italic = font.clone();
        without_italic.style = FontStyle::Normal;
        options.synthetic_italic = resolved == window.text_system().resolve_font(&without_italic);
    }
    options
}

fn styled_font(style: PackedStyle, base_font: &Font, appearance: &TerminalAppearance) -> Font {
    font_for_style(TerminalFontStyle::from_packed(style), base_font, appearance)
}

fn text_run(
    style: PackedStyle,
    text_len: usize,
    base_font: &Font,
    resolved_foreground: Color,
    appearance: &TerminalAppearance,
    scale: f32,
    text_opacity: f32,
) -> TextRun {
    let font = styled_font(style, base_font, appearance);
    let foreground = color(
        resolved_foreground,
        terminal_text_alpha(style) * text_opacity,
    );
    TextRun {
        len: text_len,
        font,
        color: foreground,
        background_color: None,
        underline: match style.underline() {
            TerminalUnderlineStyle::Single | TerminalUnderlineStyle::Curly => {
                Some(UnderlineStyle {
                    thickness: device_pixel(scale),
                    color: Some(color(
                        style.underline_color().unwrap_or(resolved_foreground),
                        text_opacity,
                    )),
                    wavy: matches!(style.underline(), TerminalUnderlineStyle::Curly),
                })
            }
            TerminalUnderlineStyle::None if style.hyperlink() => Some(UnderlineStyle {
                thickness: device_pixel(scale),
                color: Some(color(appearance.link_color, text_opacity)),
                wavy: false,
            }),
            _ => None,
        },
        strikethrough: style.strikethrough().then_some(StrikethroughStyle {
            thickness: device_pixel(scale),
            color: Some(foreground),
        }),
    }
}

fn terminal_text_alpha(style: PackedStyle) -> f32 {
    if style.faint() { 0.58 } else { 1.0 }
}

fn style_for_dictionary(
    dictionary: &TerminalDictionary,
    default_foreground: Color,
    default_background: Color,
    cell: PackedCell,
) -> PackedStyle {
    dictionary
        .styles
        .get(usize::from(cell.style_id()))
        .copied()
        .unwrap_or_else(|| {
            PackedStyle::new(
                default_foreground,
                default_background,
                None,
                0,
                TerminalUnderlineStyle::None,
            )
        })
}

fn glyph_for_dictionary(dictionary: &TerminalDictionary, cell: PackedCell) -> Glyph<'_> {
    let glyph = cell.glyph();
    if glyph == 0 {
        return Glyph::Empty;
    }
    if glyph & GRAPHEME_TABLE_BIT == 0 {
        return char::from_u32(glyph).map_or(Glyph::Empty, Glyph::Scalar);
    }

    let index = usize::try_from(glyph & !GRAPHEME_TABLE_BIT).unwrap_or(usize::MAX);
    let Some((&start, &end)) = dictionary
        .grapheme_offsets
        .get(index)
        .zip(dictionary.grapheme_offsets.get(index.saturating_add(1)))
    else {
        return Glyph::Empty;
    };
    let Some(bytes) = usize::try_from(start)
        .ok()
        .zip(usize::try_from(end).ok())
        .and_then(|(start, end)| dictionary.grapheme_bytes.get(start..end))
    else {
        return Glyph::Empty;
    };
    std::str::from_utf8(bytes).map_or(Glyph::Empty, Glyph::Grapheme)
}

fn style_for(viewport: &TerminalViewport, cell: PackedCell) -> PackedStyle {
    style_for_dictionary(
        &viewport.dictionary,
        viewport.foreground,
        viewport.background,
        cell,
    )
}

fn glyph_for(viewport: &TerminalViewport, cell: PackedCell) -> Glyph<'_> {
    glyph_for_dictionary(&viewport.dictionary, cell)
}

fn same_visual_style(left: &TextRun, right: &TextRun) -> bool {
    left.font == right.font
        && left.color == right.color
        && left.background_color == right.background_color
        && left.underline == right.underline
        && left.strikethrough == right.strikethrough
}

fn cursor_cell(
    viewport: &TerminalViewport,
    cursor: Cursor,
    visible_columns: u16,
    row_projection: RowProjection,
) -> Option<CursorCellPaint> {
    let row = row_projection.display_row(cursor.row())?;
    let mut column = usize::from(cursor.column());
    if column >= usize::from(visible_columns.min(viewport.columns)) {
        return None;
    }
    let cells = viewport.row(cursor.row())?;
    let mut cell = *cells.get(column)?;
    if (cursor.at_wide_tail() || matches!(cell.width(), CellWidth::SpacerTail)) && column > 0 {
        column -= 1;
        cell = *cells.get(column)?;
    }
    let width = if matches!(cell.width(), CellWidth::Wide) {
        2
    } else {
        1
    };
    Some(CursorCellPaint {
        column,
        row,
        width: width.min(usize::from(visible_columns).saturating_sub(column)),
        cell,
    })
}

fn cursor_glyph_line(
    viewport: &TerminalViewport,
    cursor: CursorCellPaint,
    origin: Point<Pixels>,
    font_size: Pixels,
    base_font: &Font,
    appearance: &TerminalAppearance,
    window: &mut Window,
) -> Option<PositionedLine> {
    let style = style_for(viewport, cursor.cell);
    if style.invisible() {
        return None;
    }
    let text = match glyph_for(viewport, cursor.cell) {
        Glyph::Empty => return None,
        Glyph::Scalar(character) if character.is_whitespace() => return None,
        Glyph::Scalar(character) => character.to_string(),
        Glyph::Grapheme(text) if text.chars().all(char::is_whitespace) => return None,
        Glyph::Grapheme(text) => text.to_owned(),
    };
    let font = styled_font(style, base_font, appearance);
    let glyph_options = glyph_render_options(
        TerminalFontStyle::from_packed(style),
        &font,
        appearance,
        window,
    );
    let run = TextRun {
        len: text.len(),
        font,
        color: color(viewport.background, 1.0),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    Some(PositionedLine {
        line: window
            .text_system()
            .shape_line(text.into(), font_size, &[run], None),
        origin,
        glyph_options,
    })
}

fn decorative_glyph(glyph: Glyph<'_>) -> bool {
    let character = match glyph {
        Glyph::Scalar(character) => Some(character),
        Glyph::Grapheme(text) => text.chars().next(),
        Glyph::Empty => None,
    };
    character.is_some_and(is_decorative_character)
}

fn connected_light_box_drawing(glyph: Glyph<'_>) -> Option<LightBoxDrawing> {
    single_cell_character(glyph).and_then(light_box_drawing)
}

fn connected_solid_block_element(glyph: Glyph<'_>) -> Option<SolidBlockElement> {
    single_cell_character(glyph).and_then(solid_block_element)
}

fn single_cell_character(glyph: Glyph<'_>) -> Option<char> {
    match glyph {
        Glyph::Scalar(character) => Some(character),
        Glyph::Grapheme(text) => {
            let mut characters = text.chars();
            let character = characters.next()?;
            characters.next().is_none().then_some(character)
        }
        Glyph::Empty => None,
    }
}

fn light_box_drawing(character: char) -> Option<LightBoxDrawing> {
    if let Some(diagonals) = light_box_diagonals(character) {
        return Some(LightBoxDrawing::Diagonals(diagonals));
    }
    let edges = light_box_edges(character)?;
    Some(if matches!(character, '╭' | '╮' | '╯' | '╰') {
        LightBoxDrawing::Rounded(edges)
    } else {
        LightBoxDrawing::Segments(edges)
    })
}

fn light_box_diagonals(character: char) -> Option<BoxDiagonals> {
    let bits = match character {
        '╱' => BoxDiagonals::RISING,
        '╲' => BoxDiagonals::FALLING,
        '╳' => BoxDiagonals::RISING | BoxDiagonals::FALLING,
        _ => return None,
    };
    Some(BoxDiagonals::from_bits(bits))
}

fn light_box_edges(character: char) -> Option<BoxDrawingEdges> {
    let bits = match character {
        '─' => BoxDrawingEdges::RIGHT | BoxDrawingEdges::LEFT,
        '│' => BoxDrawingEdges::UP | BoxDrawingEdges::DOWN,
        '┌' | '╭' => BoxDrawingEdges::RIGHT | BoxDrawingEdges::DOWN,
        '┐' | '╮' => BoxDrawingEdges::DOWN | BoxDrawingEdges::LEFT,
        '└' | '╰' => BoxDrawingEdges::UP | BoxDrawingEdges::RIGHT,
        '┘' | '╯' => BoxDrawingEdges::UP | BoxDrawingEdges::LEFT,
        '├' => BoxDrawingEdges::UP | BoxDrawingEdges::RIGHT | BoxDrawingEdges::DOWN,
        '┤' => BoxDrawingEdges::UP | BoxDrawingEdges::DOWN | BoxDrawingEdges::LEFT,
        '┬' => BoxDrawingEdges::RIGHT | BoxDrawingEdges::DOWN | BoxDrawingEdges::LEFT,
        '┴' => BoxDrawingEdges::UP | BoxDrawingEdges::RIGHT | BoxDrawingEdges::LEFT,
        '┼' => {
            BoxDrawingEdges::UP
                | BoxDrawingEdges::RIGHT
                | BoxDrawingEdges::DOWN
                | BoxDrawingEdges::LEFT
        }
        '╴' => BoxDrawingEdges::LEFT,
        '╵' => BoxDrawingEdges::UP,
        '╶' => BoxDrawingEdges::RIGHT,
        '╷' => BoxDrawingEdges::DOWN,
        _ => return None,
    };
    Some(BoxDrawingEdges::from_bits(bits))
}

fn solid_block_element(character: char) -> Option<SolidBlockElement> {
    let codepoint = character as u32;
    let rect = |left_eighths, top_eighths, right_eighths, bottom_eighths| SolidBlockElement::Rect {
        left_eighths,
        top_eighths,
        right_eighths,
        bottom_eighths,
    };

    match codepoint {
        0x2580 => Some(rect(0, 0, 8, 4)),
        0x2581..=0x2587 => {
            let height = u8::try_from(codepoint - 0x2580).ok()?;
            Some(rect(0, 8 - height, 8, 8))
        }
        0x2588 => Some(rect(0, 0, 8, 8)),
        0x2589..=0x258f => {
            let width = u8::try_from(0x2590 - codepoint).ok()?;
            Some(rect(0, 0, width, 8))
        }
        0x2590 => Some(rect(4, 0, 8, 8)),
        0x2594 => Some(rect(0, 0, 8, 1)),
        0x2595 => Some(rect(7, 0, 8, 8)),
        0x2596 => Some(SolidBlockElement::Quadrants(SolidBlockElement::BOTTOM_LEFT)),
        0x2597 => Some(SolidBlockElement::Quadrants(
            SolidBlockElement::BOTTOM_RIGHT,
        )),
        0x2598 => Some(SolidBlockElement::Quadrants(SolidBlockElement::TOP_LEFT)),
        0x2599 => Some(SolidBlockElement::Quadrants(
            SolidBlockElement::TOP_LEFT
                | SolidBlockElement::BOTTOM_LEFT
                | SolidBlockElement::BOTTOM_RIGHT,
        )),
        0x259a => Some(SolidBlockElement::Quadrants(
            SolidBlockElement::TOP_LEFT | SolidBlockElement::BOTTOM_RIGHT,
        )),
        0x259b => Some(SolidBlockElement::Quadrants(
            SolidBlockElement::TOP_LEFT
                | SolidBlockElement::TOP_RIGHT
                | SolidBlockElement::BOTTOM_LEFT,
        )),
        0x259c => Some(SolidBlockElement::Quadrants(
            SolidBlockElement::TOP_LEFT
                | SolidBlockElement::TOP_RIGHT
                | SolidBlockElement::BOTTOM_RIGHT,
        )),
        0x259d => Some(SolidBlockElement::Quadrants(SolidBlockElement::TOP_RIGHT)),
        0x259e => Some(SolidBlockElement::Quadrants(
            SolidBlockElement::TOP_RIGHT | SolidBlockElement::BOTTOM_LEFT,
        )),
        0x259f => Some(SolidBlockElement::Quadrants(
            SolidBlockElement::TOP_RIGHT
                | SolidBlockElement::BOTTOM_LEFT
                | SolidBlockElement::BOTTOM_RIGHT,
        )),
        _ => None,
    }
}

fn is_decorative_character(character: char) -> bool {
    matches!(
        character as u32,
        0x2500..=0x257f
            | 0x2580..=0x259f
            | 0x25a0..=0x25ff
            | 0xe0b0..=0xe0ca
            | 0xe0cc..=0xe0d7
    )
}

fn resolved_foreground(
    style: PackedStyle,
    appearance: &TerminalAppearance,
    decorative: bool,
) -> Color {
    let foreground = style.foreground();
    if style.explicit_rgb() || decorative || appearance.minimum_contrast <= 1.0 {
        return foreground;
    }
    ensure_minimum_contrast(foreground, style.background(), appearance.minimum_contrast)
}

#[allow(
    clippy::disallowed_methods,
    reason = "terminal contrast correction operates on terminal palette colors, not application chrome"
)]
fn ensure_minimum_contrast(foreground: Color, background: Color, minimum: f32) -> Color {
    if contrast_ratio(foreground, background) >= minimum {
        return foreground;
    }
    let black = Color::rgb(0, 0, 0);
    let white = Color::rgb(u8::MAX, u8::MAX, u8::MAX);
    let target = if contrast_ratio(black, background) > contrast_ratio(white, background) {
        black
    } else {
        white
    };
    if contrast_ratio(target, background) < minimum {
        return target;
    }

    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for _ in 0..12 {
        let midpoint = (low + high) * 0.5;
        if contrast_ratio(blend_color(foreground, target, midpoint), background) >= minimum {
            high = midpoint;
        } else {
            low = midpoint;
        }
    }
    blend_color(foreground, target, high)
}

fn contrast_ratio(left: Color, right: Color) -> f32 {
    let left = relative_luminance(left);
    let right = relative_luminance(right);
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}

fn relative_luminance(color: Color) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    channel(color.r) * 0.2126 + channel(color.g) * 0.7152 + channel(color.b) * 0.0722
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the interpolated channel is rounded and clamped to the u8 color domain"
)]
#[allow(
    clippy::disallowed_methods,
    reason = "terminal color blending operates on terminal palette colors, not application chrome"
)]
fn blend_color(from: Color, to: Color, amount: f32) -> Color {
    let channel = |from: u8, to: u8| {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::rgb(
        channel(from.r, to.r),
        channel(from.g, to.g),
        channel(from.b, to.b),
    )
}

fn cursor_quad(
    bounds: Bounds<Pixels>,
    style: CursorStyle,
    cursor_color: Color,
    focused: bool,
    scale: f32,
) -> PaintQuad {
    let cursor_hsla = color(cursor_color, 1.0);
    let stroke = device_pixel(scale);
    if !focused || matches!(style, CursorStyle::BlockHollow) {
        return quad(
            bounds,
            0.0,
            color(cursor_color, 0.0),
            stroke,
            cursor_hsla,
            BorderStyle::Solid,
        );
    }
    match style {
        CursorStyle::Bar => fill(
            Bounds::new(bounds.origin, size(stroke, bounds.size.height)),
            cursor_hsla,
        ),
        CursorStyle::Underline => fill(
            Bounds::new(
                point(bounds.origin.x, bounds.bottom() - stroke * 2.0),
                size(bounds.size.width, stroke * 2.0),
            ),
            cursor_hsla,
        ),
        CursorStyle::Block => fill(bounds, cursor_hsla),
        CursorStyle::BlockHollow => quad(
            bounds,
            0.0,
            color(cursor_color, 0.0),
            stroke,
            cursor_hsla,
            BorderStyle::Solid,
        ),
    }
}

fn cursor_visible_for_paint(
    cursor: Option<Cursor>,
    composing: bool,
    focused: bool,
    blink_visible: bool,
) -> bool {
    !composing && cursor.is_some_and(|cursor| cursor.visible() && (!focused || blink_visible))
}

fn selection_corners(viewport: &TerminalViewport, index: usize, radius: Pixels) -> Corners<Pixels> {
    let Some(span) = viewport.overlays.get(index).copied() else {
        return Corners::default();
    };
    let start = span.start;
    let end = span.end.saturating_sub(1);
    let above_start = span
        .row
        .checked_sub(1)
        .is_some_and(|row| selection_covers(viewport, row, start));
    let above_end = span
        .row
        .checked_sub(1)
        .is_some_and(|row| selection_covers(viewport, row, end));
    let below_start = span
        .row
        .checked_add(1)
        .is_some_and(|row| selection_covers(viewport, row, start));
    let below_end = span
        .row
        .checked_add(1)
        .is_some_and(|row| selection_covers(viewport, row, end));
    Corners {
        top_left: if above_start { px(0.0) } else { radius },
        top_right: if above_end { px(0.0) } else { radius },
        bottom_right: if below_end { px(0.0) } else { radius },
        bottom_left: if below_start { px(0.0) } else { radius },
    }
}

fn selection_covers(viewport: &TerminalViewport, row: u16, column: u16) -> bool {
    viewport.overlays.iter().any(|candidate| {
        candidate.kind() == OverlayKind::Selection
            && candidate.flags() & OVERLAY_RECTANGLE == 0
            && candidate.row == row
            && candidate.start <= column
            && column < candidate.end
    })
}

fn last_visible_row_has_content(viewport: &TerminalViewport, visible_rows: u16) -> bool {
    let row = visible_rows.min(viewport.rows).saturating_sub(1);
    viewport.row(row).is_some_and(|cells| {
        cells.iter().copied().any(|cell| {
            !matches!(viewport.glyph(cell), Glyph::Empty)
                || style_for(viewport, cell).background() != viewport.background
        })
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the floating-point value is floored and clamped to the u16 domain"
)]
fn grid_dimension(available: Pixels, cell: Pixels, scale: f32) -> u16 {
    let tolerance = device_pixel(scale) * 0.01;
    let cells = ((available + tolerance) / cell).floor();
    if !cells.is_finite() {
        return 1;
    }
    cells.clamp(1.0, f32::from(u16::MAX)) as u16
}

fn terminal_grid_bounds(
    origin: Point<Pixels>,
    columns: u16,
    rows: u16,
    cell_width: Pixels,
    line_height: Pixels,
) -> Bounds<Pixels> {
    Bounds::new(
        origin,
        size(
            cell_width * usize::from(columns),
            line_height * usize::from(rows),
        ),
    )
}

fn search_ime_bounds(
    terminal_surface: Bounds<Pixels>,
    appearance: &TerminalAppearance,
    text: &str,
    caret: usize,
    font: &Font,
    line_height: Pixels,
    foreground: Color,
    window: &mut Window,
) -> Option<Bounds<Pixels>> {
    let root_origin = point(
        terminal_surface.origin.x - px(appearance.padding_left),
        terminal_surface.origin.y - px(appearance.padding_top),
    );
    let root_width =
        terminal_surface.size.width + px(appearance.padding_left) + px(appearance.padding_right);
    let outer_width = (root_width - px(32.0)).max(px(260.0)).min(px(560.0));
    let wrap_width = (outer_width - px(20.0)).max(px(1.0));
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color: color(foreground, 1.0),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let lines = window
        .text_system()
        .shape_text(
            text.to_owned().into(),
            px(11.0),
            &[run],
            Some(wrap_width),
            None,
        )
        .ok()?;
    let mut line_start = 0;
    let mut y = px(0.0);
    let mut caret_position = None;
    for line in &lines {
        let line_end = line_start + line.len();
        if caret <= line_end {
            caret_position = line
                .position_for_index(caret.saturating_sub(line_start), line_height)
                .map(|position| point(position.x, position.y + y));
            break;
        }
        y += line.size(line_height).height;
        line_start = line_end.saturating_add(1);
    }
    let caret_position = caret_position?;
    let text_origin = point(root_origin.x + px(26.0), root_origin.y + px(18.0));
    Some(Bounds::new(
        text_origin + caret_position,
        size(device_pixel(window.scale_factor()), line_height),
    ))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the device-pixel value is rounded and clamped to a positive u16-sized domain"
)]
fn physical_pixels(value: Pixels, scale: f32) -> u32 {
    (f32::from(value) * scale)
        .round()
        .clamp(1.0, f32::from(u16::MAX)) as u32
}

fn snap(value: Pixels, scale: f32) -> Pixels {
    px((f32::from(value) * scale).round() / scale)
}

fn snap_length(value: Pixels, scale: f32) -> Pixels {
    snap(value.max(device_pixel(scale)), scale).max(device_pixel(scale))
}

fn device_pixel(scale: f32) -> Pixels {
    px(1.0 / scale)
}

fn color(value: Color, alpha: f32) -> Hsla {
    Rgba {
        r: f32::from(value.r) / 255.0,
        g: f32::from(value.g) / 255.0,
        b: f32::from(value.b) / 255.0,
        a: alpha,
    }
    .into()
}

fn appearance_color(value: AppearanceColor) -> Hsla {
    Rgba {
        r: f32::from(value.r) / 255.0,
        g: f32::from(value.g) / 255.0,
        b: f32::from(value.b) / 255.0,
        a: f32::from(value.a) / 255.0,
    }
    .into()
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "terminal renderer tests use exact terminal palette fixtures"
)]
mod tests {
    use super::*;

    fn signature(dictionary_generation: u32) -> RowCacheSignature {
        RowCacheSignature {
            dictionary_generation,
            scale_bits: 1.0_f32.to_bits(),
            font: gpui::font("monospace"),
            font_size: px(14.0),
            cell_width: px(8.0),
            foreground: Color::rgb(216, 222, 233),
            background: Color::rgb(16, 19, 24),
            appearance_hash: TerminalAppearance::default().stable_hash(),
            text_opacity_bits: 1.0_f32.to_bits(),
        }
    }

    #[test]
    fn retained_row_cache_prunes_only_stale_revisions() {
        let mut cache = RowRenderCache::default();
        let initial = [1, 2];
        cache.prepare(signature(1), 1, initial.iter().copied());
        cache.rows.insert(1, Rc::new(CachedTextRow::default()));
        cache.rows.insert(3, Rc::new(CachedTextRow::default()));
        cache.selection_rows.insert(
            SelectionCacheKey {
                revision: 1,
                start: 0,
                end: 2,
            },
            Rc::new(CachedTextRow::default()),
        );
        cache.selection_rows.insert(
            SelectionCacheKey {
                revision: 3,
                start: 0,
                end: 2,
            },
            Rc::new(CachedTextRow::default()),
        );

        let updated = [1, 2];
        cache.prepare(signature(1), 2, updated.iter().copied());
        assert!(cache.rows.contains_key(&1));
        assert!(!cache.rows.contains_key(&3));
        assert_eq!(cache.selection_rows.len(), 1);
        let live_capacity = cache.live_revisions.capacity();

        let reduced = [1];
        cache.prepare(signature(1), 3, reduced.iter().copied());
        assert!(cache.live_revisions.capacity() >= live_capacity);

        cache.prepare(signature(2), 3, reduced.iter().copied());
        assert!(cache.rows.is_empty());
        assert!(cache.selection_rows.is_empty());
    }

    #[test]
    fn text_opacity_change_invalidates_shaped_rows() {
        let mut cache = RowRenderCache::default();
        let revisions = [1];
        cache.prepare(signature(1), 1, revisions.iter().copied());
        cache.rows.insert(1, Rc::new(CachedTextRow::default()));
        cache.selection_rows.insert(
            SelectionCacheKey {
                revision: 1,
                start: 0,
                end: 2,
            },
            Rc::new(CachedTextRow::default()),
        );

        let mut dimmed = signature(1);
        dimmed.text_opacity_bits = 0.7_f32.to_bits();
        cache.prepare(dimmed, 1, revisions.iter().copied());

        assert!(cache.rows.is_empty());
        assert!(cache.selection_rows.is_empty());
    }

    #[test]
    fn hit_grid_bounds_share_the_bottom_anchored_paint_origin() {
        let origin = point(px(4.0), px(13.0));
        let bounds = terminal_grid_bounds(origin, 80, 24, px(8.0), px(19.0));
        assert_eq!(bounds.origin, origin);
        assert_eq!(bounds.size, size(px(640.0), px(456.0)));
    }

    #[test]
    fn pending_live_resize_projects_retained_rows_from_the_bottom() {
        let shrink = RowProjection::new(30, 24, true);
        assert_eq!(shrink.source_start, 6);
        assert_eq!(shrink.destination_start, 0);
        assert_eq!(shrink.display_row(6), Some(0));
        assert_eq!(shrink.display_row(29), Some(23));
        assert_eq!(shrink.display_row(5), None);

        let grow = RowProjection::new(24, 30, true);
        assert_eq!(grow.source_start, 0);
        assert_eq!(grow.destination_start, 6);
        assert_eq!(grow.display_row(0), Some(6));
        assert_eq!(grow.display_row(23), Some(29));

        let top = RowProjection::new(30, 24, false);
        assert_eq!(top.display_row(0), Some(0));
        assert_eq!(top.display_row(23), Some(23));
        assert_eq!(top.display_row(24), None);
    }

    #[test]
    fn local_scroll_projection_splits_history_live_and_shimmer_rows() {
        assert_eq!(local_row_source(96, 100, 4, 3), LocalRowSource::Shimmer);
        assert_eq!(local_row_source(97, 100, 4, 3), LocalRowSource::History(0));
        assert_eq!(local_row_source(98, 100, 4, 3), LocalRowSource::History(1));
        assert_eq!(local_row_source(99, 100, 4, 3), LocalRowSource::History(2));
        assert_eq!(local_row_source(100, 100, 4, 3), LocalRowSource::Live(0));
        assert_eq!(local_row_source(103, 100, 4, 3), LocalRowSource::Live(3));
        assert_eq!(local_row_source(104, 100, 4, 3), LocalRowSource::Shimmer);

        let projection = local_live_projection(97, 100, 4, 4);
        assert_eq!(
            projection,
            RowProjection {
                source_start: 0,
                destination_start: 3,
                count: 1,
            }
        );
        assert_eq!(projection.display_row(0), Some(3));

        assert_eq!(
            local_live_projection(95, 100, 4, 4),
            RowProjection {
                source_start: 0,
                destination_start: 0,
                count: 0,
            }
        );
    }

    #[test]
    fn retained_rows_decode_graphemes_with_their_own_dictionary() {
        let cell = PackedCell::new(GRAPHEME_TABLE_BIT, 0, CellWidth::Narrow);
        let dictionary = |text: &'static [u8]| {
            TerminalDictionary::from_shared(
                Arc::<[PackedStyle]>::from([]),
                Arc::from([0, u32::try_from(text.len()).expect("small grapheme")]),
                Arc::from(text),
            )
        };
        let older = dictionary(b"old");
        let newer = dictionary(b"new");

        assert_eq!(glyph_for_dictionary(&older, cell), Glyph::Grapheme("old"));
        assert_eq!(glyph_for_dictionary(&newer, cell), Glyph::Grapheme("new"));
    }

    #[test]
    fn copy_cursor_paints_a_full_row_beneath_existing_overlays() {
        let mut viewport = TerminalViewport::blank(80, 24, zz_terminal::SessionStatus::Running);
        viewport.overlays = Arc::from([
            zz_terminal::OverlaySpan::new(5, 2, 6, OverlayKind::Selection),
            zz_terminal::OverlaySpan::new(5, 4, 5, OverlayKind::CopyCursor),
        ]);
        let mut output = Vec::new();

        let _ = collect_overlays(
            &viewport,
            &TerminalAppearance::default(),
            RowProjection::new(24, 24, false),
            80,
            point(px(10.0), px(20.0)),
            px(8.0),
            px(19.0),
            1.0,
            &mut output,
        );

        assert_eq!(output.len(), 3);
        assert_eq!(
            output[0].bounds,
            Bounds::new(point(px(10.0), px(115.0)), size(px(640.0), px(19.0)))
        );
        assert_eq!(
            output[1].bounds,
            Bounds::new(point(px(26.0), px(115.0)), size(px(32.0), px(19.0)))
        );
        assert_eq!(
            output[2].bounds,
            Bounds::new(point(px(42.0), px(115.0)), size(px(8.0), px(19.0)))
        );
        assert!(
            copy_cursor_line_color(&TerminalAppearance::default()).a
                < overlay_color(OverlayKind::CopyCursor, &TerminalAppearance::default()).a
        );
    }

    #[test]
    fn link_hover_reports_its_window_space_bounds() {
        let mut viewport = TerminalViewport::blank(20, 4, zz_terminal::SessionStatus::Running);
        viewport.overlays = Arc::from([zz_terminal::OverlaySpan::new(
            2,
            3,
            7,
            OverlayKind::LinkHover,
        )]);
        let mut output = Vec::new();

        let bounds = collect_overlays(
            &viewport,
            &TerminalAppearance::default(),
            RowProjection::new(4, 4, false),
            20,
            point(px(10.0), px(20.0)),
            px(8.0),
            px(19.0),
            1.0,
            &mut output,
        );

        assert_eq!(
            bounds,
            Some(Bounds::new(
                point(px(34.0), px(58.0)),
                size(px(32.0), px(19.0)),
            ))
        );
        assert_eq!(output.len(), 2, "hover fill and underline are painted");
    }

    #[test]
    fn selected_ime_cells_are_masked_relative_to_the_selected_span() {
        let hidden = (3, 7..11);
        assert_eq!(
            selection_hidden_columns(3, 5..10, Some(&hidden)),
            Some(2..5)
        );
        assert_eq!(selection_hidden_columns(2, 5..10, Some(&hidden)), None);
        assert_eq!(selection_hidden_columns(3, 0..7, Some(&hidden)), None);
    }

    #[test]
    fn internal_revision_keys_bypass_general_purpose_hashing() {
        let mut hasher = RevisionHasher::default();
        hasher.write_u64(0x1234_5678_9abc_def0);
        assert_eq!(hasher.finish(), 0x1234_5678_9abc_def0);

        let mut revisions = RevisionMap::default();
        for revision in 1_u64..=128 {
            revisions.insert(revision, revision * 2);
        }
        assert_eq!(revisions.len(), 128);
        assert_eq!(revisions.get(&64), Some(&128));
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn cached_row_handles_keep_paint_records_compact() {
        assert_eq!(std::mem::size_of::<PositionedTextRow>(), 16);
        assert_eq!(std::mem::align_of::<PositionedTextRow>(), 8);
        assert!(std::mem::size_of::<PositionedTextRow>() < std::mem::size_of::<CachedLine>());

        let cached = Rc::new(CachedTextRow::default());
        let positioned = PositionedTextRow {
            row: Rc::clone(&cached),
            origin: point(px(1.0), px(2.0)),
        };
        assert!(Rc::ptr_eq(&positioned.row, &cached));
    }

    #[test]
    fn adjacent_styles_share_one_multirun_shape_batch() {
        let foreground = Color::rgb(216, 222, 233);
        let background = Color::rgb(16, 19, 24);
        let normal = PackedStyle::new(
            foreground,
            background,
            None,
            0,
            TerminalUnderlineStyle::None,
        );
        let bold = PackedStyle::new(
            foreground,
            background,
            None,
            zz_terminal::ATTR_BOLD,
            TerminalUnderlineStyle::None,
        );
        let font = gpui::font("monospace");
        let appearance = TerminalAppearance::default();
        let mut runs = SmallVec::new();
        let mut last_style = None;
        let mut last_foreground = None;

        push_cell_text_run(
            &mut runs,
            &mut last_style,
            &mut last_foreground,
            normal,
            foreground,
            1,
            &font,
            &appearance,
            1.0,
            1.0,
        );
        push_cell_text_run(
            &mut runs,
            &mut last_style,
            &mut last_foreground,
            normal,
            foreground,
            2,
            &font,
            &appearance,
            1.0,
            1.0,
        );
        push_cell_text_run(
            &mut runs,
            &mut last_style,
            &mut last_foreground,
            bold,
            foreground,
            1,
            &font,
            &appearance,
            1.0,
            1.0,
        );
        push_cell_text_run(
            &mut runs,
            &mut last_style,
            &mut last_foreground,
            bold,
            foreground,
            3,
            &font,
            &appearance,
            1.0,
            1.0,
        );

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len, 3);
        assert_eq!(runs[1].len, 4);
        assert_eq!(last_style, Some(bold));
        assert!(!same_visual_style(&runs[0], &runs[1]));
    }

    #[test]
    fn text_opacity_scales_glyphs_and_decorations() {
        let foreground = Color::rgb(216, 222, 233);
        let style = PackedStyle::new(
            foreground,
            Color::rgb(16, 19, 24),
            None,
            0,
            TerminalUnderlineStyle::Single,
        );
        let run = text_run(
            style,
            1,
            &gpui::font("monospace"),
            foreground,
            &TerminalAppearance::default(),
            1.0,
            0.7,
        );

        assert!((run.color.a - 0.7).abs() < f32::EPSILON);
        let underline = run.underline.expect("single underline is shaped with text");
        assert!(
            (underline
                .color
                .expect("terminal underline has an explicit color")
                .a
                - 0.7)
                .abs()
                < f32::EPSILON
        );
        assert!(run.background_color.is_none());
    }

    #[test]
    fn cached_backgrounds_group_styled_cells_once_per_row_revision() {
        let mut viewport = TerminalViewport::blank(4, 1, zz_terminal::SessionStatus::Running);
        let highlighted = Color::rgb(44, 52, 68);
        let mut styles = viewport.styles().to_vec();
        styles.push(PackedStyle::new(
            viewport.foreground,
            highlighted,
            None,
            0,
            TerminalUnderlineStyle::None,
        ));
        Arc::make_mut(&mut viewport.dictionary).styles = styles.into();
        let cells = Arc::make_mut(&mut viewport.cells);
        cells[0] = PackedCell::new(u32::from('a'), 1, CellWidth::Narrow);
        cells[1] = PackedCell::new(u32::from('b'), 1, CellWidth::Narrow);
        cells[3] = PackedCell::new(u32::from('c'), 1, CellWidth::Narrow);

        let mut backgrounds = Vec::new();
        let mut batch = None;
        for (column, cell) in viewport
            .row(0)
            .expect("first row")
            .iter()
            .copied()
            .enumerate()
        {
            push_background(
                &mut batch,
                column,
                style_for(&viewport, cell).background(),
                viewport.background,
                &mut backgrounds,
            );
        }
        finish_backgrounds(batch, 4, viewport.background, &mut backgrounds);

        assert_eq!(backgrounds.len(), 2);
        assert_eq!(
            (backgrounds[0].start_column, backgrounds[0].cell_count),
            (0, 2)
        );
        assert_eq!(
            (backgrounds[1].start_column, backgrounds[1].cell_count),
            (3, 1)
        );
        assert!(backgrounds.iter().all(|run| run.color == highlighted));
    }

    #[test]
    fn contrast_correction_respects_exact_colors_and_decorative_glyphs() {
        let mut appearance = TerminalAppearance {
            minimum_contrast: 4.5,
            ..TerminalAppearance::default()
        };
        let background = Color::rgb(245, 245, 245);
        let foreground = Color::rgb(238, 238, 238);
        let style = PackedStyle::new(
            foreground,
            background,
            None,
            0,
            TerminalUnderlineStyle::None,
        );
        let corrected = resolved_foreground(style, &appearance, false);
        assert_ne!(corrected, foreground);
        assert!(contrast_ratio(corrected, background) >= 4.5);

        let exact = PackedStyle::new(
            foreground,
            background,
            None,
            zz_terminal::ATTR_EXPLICIT_RGB,
            TerminalUnderlineStyle::None,
        );
        assert_eq!(resolved_foreground(exact, &appearance, false), foreground);
        assert_eq!(resolved_foreground(style, &appearance, true), foreground);

        appearance.minimum_contrast = 1.0;
        assert_eq!(resolved_foreground(style, &appearance, false), foreground);
    }

    #[test]
    fn decorative_character_ranges_match_terminal_separators_only() {
        for character in ['─', '█', '◆', '\u{e0b0}', '\u{e0d7}'] {
            assert!(is_decorative_character(character), "{character:?}");
        }
        for character in ['A', '$', '→', '\u{f00c}', '😀'] {
            assert!(!is_decorative_character(character), "{character:?}");
        }
    }

    #[test]
    fn light_box_connectors_follow_only_connected_cell_edges() {
        assert_eq!(
            light_box_edges('─'),
            Some(BoxDrawingEdges::from_bits(
                BoxDrawingEdges::RIGHT | BoxDrawingEdges::LEFT
            ))
        );
        assert_eq!(
            light_box_edges('│'),
            Some(BoxDrawingEdges::from_bits(
                BoxDrawingEdges::UP | BoxDrawingEdges::DOWN
            ))
        );
        assert_eq!(
            light_box_edges('╮'),
            Some(BoxDrawingEdges::from_bits(
                BoxDrawingEdges::DOWN | BoxDrawingEdges::LEFT
            ))
        );
        assert_eq!(
            light_box_edges('╰'),
            Some(BoxDrawingEdges::from_bits(
                BoxDrawingEdges::UP | BoxDrawingEdges::RIGHT
            ))
        );
        assert_eq!(
            light_box_edges('┼'),
            Some(BoxDrawingEdges::from_bits(
                BoxDrawingEdges::UP
                    | BoxDrawingEdges::RIGHT
                    | BoxDrawingEdges::DOWN
                    | BoxDrawingEdges::LEFT
            ))
        );
        for character in ['A', '┄', '┃', '═', '╱'] {
            assert_eq!(light_box_edges(character), None, "{character:?}");
        }
        assert!(matches!(
            light_box_drawing('─'),
            Some(LightBoxDrawing::Segments(_))
        ));
        assert!(matches!(
            light_box_drawing('╮'),
            Some(LightBoxDrawing::Rounded(_))
        ));
    }

    #[test]
    fn light_box_diagonals_cover_the_three_diagonal_forms() {
        assert_eq!(
            light_box_diagonals('╲'),
            Some(BoxDiagonals::from_bits(BoxDiagonals::FALLING))
        );
        assert_eq!(
            light_box_diagonals('╱'),
            Some(BoxDiagonals::from_bits(BoxDiagonals::RISING))
        );
        assert_eq!(
            light_box_diagonals('╳'),
            Some(BoxDiagonals::from_bits(
                BoxDiagonals::RISING | BoxDiagonals::FALLING
            ))
        );
        for character in ['A', '─', '╮', '╌'] {
            assert_eq!(light_box_diagonals(character), None, "{character:?}");
        }
    }

    #[test]
    fn box_connector_quads_reach_exact_cell_boundaries() {
        let connector = [CachedBoxConnector {
            column: 2,
            drawing: LightBoxDrawing::Segments(BoxDrawingEdges::from_bits(
                BoxDrawingEdges::UP
                    | BoxDrawingEdges::RIGHT
                    | BoxDrawingEdges::DOWN
                    | BoxDrawingEdges::LEFT,
            )),
            color: Color::rgb(0xaa, 0xbb, 0xcc),
            alpha: 1.0,
        }];
        let mut output = Vec::new();
        push_box_drawing_connectors(
            &connector,
            px(10.0),
            px(20.0),
            px(8.0),
            px(20.0),
            px(1.0),
            2.0,
            &mut output,
        );

        assert_eq!(output.len(), 2);
        let TerminalGraphicPaint::Quad(vertical) = &output[0] else {
            panic!("straight vertical connector must be a quad");
        };
        let TerminalGraphicPaint::Quad(horizontal) = &output[1] else {
            panic!("straight horizontal connector must be a quad");
        };
        let stroke = px(1.0);
        assert_eq!(vertical.bounds.origin.y, px(20.0));
        assert_eq!(vertical.bounds.size.width, stroke);
        assert_eq!(vertical.bounds.bottom(), px(40.0));
        assert_eq!(horizontal.bounds.origin.x, px(26.0));
        assert_eq!(horizontal.bounds.right(), px(34.0));
        assert_eq!(horizontal.bounds.size.height, stroke);

        output.clear();
        push_box_drawing_connectors(
            &connector,
            px(10.0),
            px(20.0),
            px(16.0),
            px(40.0),
            px(1.5),
            2.0,
            &mut output,
        );
        let TerminalGraphicPaint::Quad(vertical) = &output[0] else {
            panic!("straight vertical connector must be a quad");
        };
        let TerminalGraphicPaint::Quad(horizontal) = &output[1] else {
            panic!("straight horizontal connector must be a quad");
        };
        assert_eq!(vertical.bounds.size.width, px(1.5));
        assert_eq!(vertical.bounds.bottom(), px(60.0));
        assert_eq!(horizontal.bounds.size.height, px(1.5));
        assert_eq!(horizontal.bounds.right(), px(58.0));
    }

    #[test]
    fn rounded_box_connectors_are_single_paths_that_reach_cell_edges() {
        for (character, horizontal_edge, vertical_edge) in [
            ('╭', px(34.0), px(40.0)),
            ('╮', px(26.0), px(40.0)),
            ('╯', px(26.0), px(20.0)),
            ('╰', px(34.0), px(20.0)),
        ] {
            let connector = [CachedBoxConnector {
                column: 2,
                drawing: light_box_drawing(character).expect("rounded box drawing"),
                color: Color::rgb(0xaa, 0xbb, 0xcc),
                alpha: 1.0,
            }];
            let mut output = Vec::new();
            push_box_drawing_connectors(
                &connector,
                px(10.0),
                px(20.0),
                px(8.0),
                px(20.0),
                px(1.0),
                2.0,
                &mut output,
            );

            assert_eq!(output.len(), 1, "{character:?}");
            let TerminalGraphicPaint::Path { path, .. } = &output[0] else {
                panic!("rounded connector {character:?} must be a single path");
            };
            let near_edge = |actual: Pixels, expected: Pixels| {
                (f32::from(actual) - f32::from(expected)).abs() < 0.1
            };
            if matches!(character, '╭' | '╰') {
                assert!(
                    near_edge(path.bounds.right(), horizontal_edge),
                    "{character:?}: {:?}",
                    path.bounds
                );
            } else {
                assert!(
                    near_edge(path.bounds.origin.x, horizontal_edge),
                    "{character:?}: {:?}",
                    path.bounds
                );
            }
            if matches!(character, '╭' | '╮') {
                assert!(
                    near_edge(path.bounds.bottom(), vertical_edge),
                    "{character:?}: {:?}",
                    path.bounds
                );
            } else {
                assert!(
                    near_edge(path.bounds.origin.y, vertical_edge),
                    "{character:?}: {:?}",
                    path.bounds
                );
            }
        }
    }

    #[test]
    fn box_stroke_rounds_the_face_underline_up_to_whole_device_pixels() {
        // BerkeleyMono at 13pt reports a 1.3px underline.
        assert_eq!(box_stroke_width(px(1.3), 2.0), px(1.5));
        assert_eq!(box_stroke_width(px(1.3), 1.0), px(2.0));
        assert_eq!(box_stroke_width(px(0.0), 2.0), px(0.5));
        assert_eq!(box_stroke_width(px(0.1), 3.0), px(1.0 / 3.0));
        assert_eq!(box_stroke_width(px(1.0), 2.0), px(1.0));
    }

    #[test]
    fn diagonal_box_connectors_are_centered_and_overlap_across_rows() {
        let paint_row = |row_y: Pixels| {
            let connector = [CachedBoxConnector {
                column: 2,
                drawing: light_box_drawing('╲').expect("diagonal box drawing"),
                color: Color::rgb(0xaa, 0xbb, 0xcc),
                alpha: 1.0,
            }];
            let mut output = Vec::new();
            push_box_drawing_connectors(
                &connector,
                px(10.0),
                row_y,
                px(8.0),
                px(20.0),
                px(1.0),
                2.0,
                &mut output,
            );
            assert_eq!(output.len(), 1);
            let TerminalGraphicPaint::Path { path, .. } = output.remove(0) else {
                panic!("diagonal connector must be a single path");
            };
            path.bounds
        };

        let first = paint_row(px(20.0));
        let cell_center = point(px(26.0 + 4.0), px(20.0 + 10.0));
        assert!((f32::from(first.center().x) - f32::from(cell_center.x)).abs() < 0.05);
        assert!((f32::from(first.center().y) - f32::from(cell_center.y)).abs() < 0.05);
        assert!(first.origin.x < px(26.0) && first.right() > px(34.0));

        let second = paint_row(px(40.0));
        assert!(first.bottom() > second.origin.y);
        assert_eq!(second.center().x, first.center().x);
    }

    #[test]
    fn diagonal_cross_draws_both_strokes_in_one_path() {
        let connector = [CachedBoxConnector {
            column: 0,
            drawing: light_box_drawing('╳').expect("diagonal cross"),
            color: Color::rgb(0xaa, 0xbb, 0xcc),
            alpha: 1.0,
        }];
        let mut output = Vec::new();
        push_box_drawing_connectors(
            &connector,
            px(0.0),
            px(0.0),
            px(8.0),
            px(20.0),
            px(1.0),
            2.0,
            &mut output,
        );
        assert_eq!(output.len(), 1);
        let TerminalGraphicPaint::Path { path, .. } = &output[0] else {
            panic!("diagonal cross must be a single path");
        };
        assert!(path.bounds.origin.x < px(0.0) && path.bounds.right() > px(8.0));
        assert!(path.bounds.origin.y < px(0.0) && path.bounds.bottom() > px(20.0));
    }

    #[test]
    fn solid_block_elements_fill_snapped_cell_bounds_without_gaps() {
        assert_eq!(
            solid_block_element('█'),
            Some(SolidBlockElement::Rect {
                left_eighths: 0,
                top_eighths: 0,
                right_eighths: 8,
                bottom_eighths: 8,
            })
        );
        assert_eq!(
            solid_block_element('▄'),
            Some(SolidBlockElement::Rect {
                left_eighths: 0,
                top_eighths: 4,
                right_eighths: 8,
                bottom_eighths: 8,
            })
        );
        assert_eq!(solid_block_element('▒'), None);

        let blocks = [2, 3].map(|column| CachedSolidBlock {
            column,
            element: solid_block_element('█').expect("full block"),
            color: Color::rgb(0xaa, 0xbb, 0xcc),
            alpha: 1.0,
        });
        let mut output = Vec::new();
        push_solid_blocks(
            &blocks,
            px(10.0),
            px(20.0),
            px(8.0),
            px(20.0),
            2.0,
            &mut output,
        );

        assert_eq!(output.len(), 2);
        let TerminalGraphicPaint::Quad(left) = &output[0] else {
            panic!("solid block must be a quad");
        };
        let TerminalGraphicPaint::Quad(right) = &output[1] else {
            panic!("solid block must be a quad");
        };
        assert_eq!(
            left.bounds,
            Bounds::new(point(px(26.0), px(20.0)), size(px(8.0), px(20.0)))
        );
        assert_eq!(left.bounds.right(), right.bounds.origin.x);
    }

    #[test]
    fn fractional_scale_grid_tolerates_only_subdevice_rounding_noise() {
        for scale in [1.0, 1.25, 1.3, 1.5, 2.0] {
            let cell = snap_length(px(19.0), scale);
            let physical = f32::from(cell) * scale;
            assert!((physical - physical.round()).abs() < 0.0001);
            let exact = cell * 80;
            assert_eq!(grid_dimension(exact, cell, scale), 80);
            assert_eq!(
                grid_dimension(exact - device_pixel(scale) * 0.005, cell, scale),
                80
            );
            assert_eq!(
                grid_dimension(exact - device_pixel(scale) * 0.02, cell, scale),
                79
            );
        }
    }

    #[test]
    fn grid_dimension_rejects_non_finite_geometry() {
        assert_eq!(grid_dimension(px(f32::NAN), px(8.0), 1.0), 1);
        assert_eq!(grid_dimension(px(f32::INFINITY), px(8.0), 1.0), 1);
        assert_eq!(grid_dimension(px(80.0), px(0.0), 1.0), 1);
    }

    #[test]
    fn wide_tail_cursor_resolves_to_the_leading_cell() {
        let mut viewport = TerminalViewport::blank(2, 1, zz_terminal::SessionStatus::Running);
        let expected = {
            let cells = Arc::make_mut(&mut viewport.cells);
            cells[0] = PackedCell::new(u32::from('界'), 0, CellWidth::Wide);
            cells[1] = PackedCell::new(0, 0, CellWidth::SpacerTail);
            cells[0]
        };
        let cursor = Cursor::new(
            1,
            0,
            true,
            false,
            true,
            CursorStyle::Block,
            Color::rgb(1, 2, 3),
        );

        let paint = cursor_cell(&viewport, cursor, 2, RowProjection::new(1, 1, false))
            .expect("cursor paint cell");
        assert_eq!(paint.column, 0);
        assert_eq!(paint.row, 0);
        assert_eq!(paint.width, 2);
        assert_eq!(paint.cell, expected);
    }

    #[test]
    fn linear_selection_rounds_only_exposed_row_corners() {
        let mut viewport = TerminalViewport::blank(10, 3, zz_terminal::SessionStatus::Running);
        viewport.overlays = Arc::from([
            zz_terminal::OverlaySpan::new(0, 2, 5, OverlayKind::Selection),
            zz_terminal::OverlaySpan::new(1, 2, 8, OverlayKind::Selection),
        ]);
        let radius = px(3.0);
        let first = selection_corners(&viewport, 0, radius);
        assert_eq!(first.top_left, radius);
        assert_eq!(first.top_right, radius);
        assert_eq!(first.bottom_left, px(0.0));
        assert_eq!(first.bottom_right, px(0.0));

        let second = selection_corners(&viewport, 1, radius);
        assert_eq!(second.top_left, px(0.0));
        assert_eq!(second.top_right, radius);
        assert_eq!(second.bottom_left, radius);
        assert_eq!(second.bottom_right, radius);
    }

    #[test]
    fn appearance_hash_change_invalidates_shaped_rows() {
        let mut cache = RowRenderCache::default();
        let first = signature(1);
        cache.prepare(first.clone(), 1, [7].into_iter());
        cache.rows.insert(7, Rc::new(CachedTextRow::default()));
        let mut changed = first;
        changed.appearance_hash = changed.appearance_hash.wrapping_add(1);
        cache.prepare(changed, 1, [7].into_iter());
        assert!(cache.rows.is_empty());
    }

    #[test]
    fn bold_italic_preserves_terminal_features_and_fallbacks() {
        let appearance = TerminalAppearance {
            font_families: vec!["Primary Mono".to_owned(), "Emoji Fallback".to_owned()],
            font_features: vec![zz_terminal::FontFeature::new(*b"ss01", 1)],
            font_weight: 450,
            ..TerminalAppearance::default()
        };
        let base = crate::terminal::view::terminal_font(&appearance);
        let style = PackedStyle::new(
            appearance.foreground,
            appearance.background,
            None,
            zz_terminal::ATTR_BOLD | zz_terminal::ATTR_ITALIC,
            TerminalUnderlineStyle::None,
        );

        let styled = styled_font(style, &base, &appearance);
        assert_eq!(styled.features, base.features);
        assert_eq!(styled.fallbacks, base.fallbacks);
        assert_eq!(styled.weight, FontWeight(750.0));
        assert_eq!(styled.style, FontStyle::Italic);
    }

    #[test]
    fn ghostty_font_thickening_maps_to_explicit_glyph_smoothing() {
        let disabled = base_glyph_render_options(&TerminalAppearance::default());
        assert_eq!(disabled.font_smoothing, FontSmoothing::Disabled);
        assert!(!disabled.synthetic_bold);
        assert!(!disabled.synthetic_italic);

        let enabled = base_glyph_render_options(&TerminalAppearance {
            font_thicken: true,
            font_thicken_strength: 173,
            ..TerminalAppearance::default()
        });
        assert_eq!(enabled.font_smoothing, FontSmoothing::Enabled(173));
    }

    #[test]
    fn cursor_shapes_use_device_pixel_strokes_and_hollow_when_unfocused() {
        let bounds = Bounds::new(point(px(2.0), px(3.0)), size(px(8.0), px(20.0)));
        let scale = 1.3;
        let stroke = device_pixel(scale);
        let cursor_color = Color::rgb(0xaa, 0xbb, 0xcc);

        let bar = cursor_quad(bounds, CursorStyle::Bar, cursor_color, true, scale);
        assert_eq!(bar.bounds.size.width, stroke);
        assert_eq!(bar.bounds.size.height, bounds.size.height);

        let underline = cursor_quad(bounds, CursorStyle::Underline, cursor_color, true, scale);
        assert_eq!(underline.bounds.size.height, stroke * 2.0);
        assert_eq!(underline.bounds.bottom(), bounds.bottom());

        let hollow = cursor_quad(bounds, CursorStyle::Block, cursor_color, false, scale);
        assert_eq!(hollow.bounds, bounds);
        assert_eq!(hollow.border_widths.top, stroke);
        assert_eq!(hollow.border_widths.right, stroke);
        assert_eq!(hollow.border_widths.bottom, stroke);
        assert_eq!(hollow.border_widths.left, stroke);
    }

    #[test]
    fn unfocused_cursor_stays_visible_and_composition_suppresses_it() {
        let cursor = Cursor::new(
            0,
            0,
            true,
            true,
            false,
            CursorStyle::Block,
            Color::rgb(1, 2, 3),
        );
        assert!(!cursor_visible_for_paint(Some(cursor), false, true, false));
        assert!(cursor_visible_for_paint(Some(cursor), false, false, false));
        assert!(!cursor_visible_for_paint(Some(cursor), true, false, true));
    }
}
