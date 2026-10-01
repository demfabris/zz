use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use libghostty_vt::{
    screen::{CellSemanticContent, CellWide},
    style::RgbColor,
    terminal::{Point, PointCoordinate, ScreenSnapshot},
};
use parking_lot::Mutex;

use super::mode_revision::ModeRowMeta;
use super::{
    ViewportDictionary, WorkerError, resolve_style_color, style_attributes, underline_style,
};
use crate::{CellWidth, Color, PackedCell, PackedStyle, TerminalDictionary};

const CACHED_ROWS: usize = 64;

#[derive(Debug)]
pub(super) struct CopyRow {
    pub cells: Box<[PackedCell]>,
    pub semantics: Box<[u8]>,
    pub meta: ModeRowMeta,
}

pub(super) struct CopyGrid {
    pub terminal: Arc<Mutex<ScreenSnapshot>>,
    dictionary: ViewportDictionary,
    cached: HashMap<u32, Arc<CopyRow>>,
    order: VecDeque<u32>,
    foreground: Color,
    background: Color,
    palette: [RgbColor; 256],
    columns: u16,
    loaded: u32,
    viewport: bool,
    active_start: u32,
}

impl std::fmt::Debug for CopyGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopyGrid")
            .field("columns", &self.columns)
            .field("cached_rows", &self.cached.len())
            .finish_non_exhaustive()
    }
}

impl CopyGrid {
    pub(super) fn new(
        terminal: ScreenSnapshot,
        foreground: Color,
        background: Color,
        palette: &[RgbColor; 256],
        columns: u16,
    ) -> Self {
        let mut dictionary = ViewportDictionary::default();
        dictionary.ensure_default(
            PackedStyle::new(foreground, background, None, 0, crate::UnderlineStyle::None),
            palette,
        );
        let active_start = u32::try_from(terminal.total_rows().expect("snapshot rows"))
            .unwrap_or(u32::MAX)
            .saturating_sub(u32::from(terminal.rows().expect("snapshot viewport")));
        dictionary.tune_live_compaction_limits(usize::from(columns) * CACHED_ROWS);
        Self {
            terminal: Arc::new(Mutex::new(terminal)),
            dictionary,
            cached: HashMap::new(),
            order: VecDeque::new(),
            foreground,
            background,
            palette: *palette,
            columns,
            loaded: 0,
            viewport: false,
            active_start,
        }
    }

    pub(super) fn row(&mut self, row: u32) -> Result<Arc<CopyRow>, WorkerError> {
        if let Some(cached) = self.cached.get(&row) {
            return Ok(Arc::clone(cached));
        }
        if !self.viewport {
            self.compact();
        }
        let mut terminal = self.terminal.lock();
        if self.loaded == 512 {
            if !*super::NO_COMPRESS {
                terminal.compress(libghostty_vt::terminal::CompressionMode::Full)?;
            }
            self.loaded = 0;
        }
        self.loaded += 1;
        let point = |x| {
            if row >= self.active_start {
                Point::Active(PointCoordinate {
                    x,
                    y: row - self.active_start,
                })
            } else {
                Point::Screen(PointCoordinate { x, y: row })
            }
        };
        let native_row = terminal.grid_row(point(0))?;
        let raw_row = native_row.row()?;
        let meta = ModeRowMeta::new(
            raw_row.is_wrapped()?,
            raw_row.is_wrap_continuation()?,
            raw_row.semantic_prompt()?,
        );
        let mut cells = Vec::with_capacity(usize::from(self.columns));
        let mut semantics = Vec::with_capacity(usize::from(self.columns));
        let mut stack = ['\0'; 8];
        let mut extra = Vec::new();
        let mut text = String::new();
        let mut last_style = None;
        for column in 0..self.columns {
            let reference = native_row.cell(column).expect("snapshot column");
            let cell = reference.cell()?;
            let style_id = cell.style_id()?;
            let hyperlink = cell.has_hyperlink()?;
            let packed_style_id = if let Some((previous, previous_hyperlink, packed)) = last_style
                && previous == style_id
                && previous_hyperlink == hyperlink
            {
                packed
            } else {
                let style = reference.style()?;
                let mut foreground =
                    resolve_style_color(style.fg_color, &self.palette).unwrap_or(self.foreground);
                let mut background =
                    resolve_style_color(style.bg_color, &self.palette).unwrap_or(self.background);
                if style.inverse {
                    std::mem::swap(&mut foreground, &mut background);
                }
                let packed_style = PackedStyle::new(
                    foreground,
                    background,
                    resolve_style_color(style.underline_color, &self.palette),
                    style_attributes(
                        &style,
                        matches!(
                            if style.inverse {
                                style.bg_color
                            } else {
                                style.fg_color
                            },
                            libghostty_vt::style::StyleColor::Rgb(_)
                        ),
                        hyperlink,
                    ),
                    underline_style(style.underline),
                );
                let packed = self.dictionary.intern_style(packed_style);
                last_style = Some((style_id, hyperlink, packed));
                packed
            };
            let width = match cell.wide()? {
                CellWide::Narrow => CellWidth::Narrow,
                CellWide::Wide => CellWidth::Wide,
                CellWide::SpacerTail => CellWidth::SpacerTail,
                CellWide::SpacerHead => CellWidth::SpacerHead,
            };
            let glyph = match reference.graphemes(&mut stack) {
                Ok(0) => 0,
                Ok(1) => u32::from(stack[0]),
                Ok(count) => {
                    text.clear();
                    text.extend(stack[..count].iter());
                    self.dictionary.encode_glyph(&text)
                }
                Err(libghostty_vt::Error::OutOfSpace { required }) => {
                    extra.resize(required, '\0');
                    let count = reference.graphemes(&mut extra)?;
                    match count {
                        0 => 0,
                        1 => u32::from(extra[0]),
                        _ => {
                            text.clear();
                            text.extend(extra[..count].iter());
                            self.dictionary.encode_glyph(&text)
                        }
                    }
                }
                Err(error) => return Err(error.into()),
            };
            cells.push(PackedCell::new(glyph, packed_style_id, width));
            semantics.push(match cell.semantic_content()? {
                CellSemanticContent::Output => 0,
                CellSemanticContent::Input => 1,
                CellSemanticContent::Prompt => 2,
            });
        }
        let captured = Arc::new(CopyRow {
            cells: cells.into_boxed_slice(),
            semantics: semantics.into_boxed_slice(),
            meta,
        });
        if self.order.len() == CACHED_ROWS
            && let Some(oldest) = self.order.pop_front()
        {
            self.cached.remove(&oldest);
        }
        self.order.push_back(row);
        self.cached.insert(row, Arc::clone(&captured));
        Ok(captured)
    }

    fn compact(&mut self) {
        if self.dictionary.should_compact_live() {
            self.dictionary.reset_live(
                PackedStyle::new(
                    self.foreground,
                    self.background,
                    None,
                    0,
                    crate::UnderlineStyle::None,
                ),
                &self.palette,
            );
            self.dictionary
                .tune_live_compaction_limits(usize::from(self.columns) * CACHED_ROWS);
            self.cached.clear();
            self.order.clear();
        }
    }

    pub(super) fn begin_viewport(&mut self) {
        self.compact();
        self.viewport = true;
    }

    pub(super) fn end_viewport(&mut self) {
        self.viewport = false;
    }

    pub(super) fn generation(&self) -> u32 {
        self.dictionary.generation
    }

    pub(super) fn dictionary(&mut self) -> Arc<TerminalDictionary> {
        self.dictionary.shared_dictionary()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod actor_tests;
