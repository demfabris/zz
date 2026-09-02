use std::{
    collections::{BTreeSet, HashMap},
    fmt::Write as _,
    io::{self, Write as _},
};

use unicode_width::UnicodeWidthChar as _;
use zz_protocol::{
    PaneBorderIndicators, PaneBorderLines, PaneBorderStatus, PaneId, PaneKindSnapshot,
    PopupBorderLines, StyledSegment, TmuxAttributeState, TmuxColour, TmuxStyle, parse_style,
    parse_styled_segments,
};
use zz_terminal::{
    CellWidth, Color, CursorStyle, Glyph, KittyPlacement, PackedCell, PackedStyle, SearchDirection,
    SearchQuery, TerminalAppearance, TerminalMode, TerminalViewport, UnderlineStyle,
};

use crate::{
    browser::{BROWSER_IMAGE_ID, BrowserFrameUpdate},
    kitty::{FrameTransport, KittyBridge, KittyImageData},
    layout::{
        BORDER_D, BORDER_L, BORDER_R, BORDER_U, CELL_LR, Divider, FloatingSpec, Rect, border_glyph,
        cell_type_of, resolve_floating,
    },
    picker, sidebar,
    state::Model,
};

pub(crate) use zz_client::ViewportDamage as FrameDamage;

/// Folds a coalesced frame's damage into the damage already pending for a pane.
pub(crate) fn merge_damage(damage: &mut FrameDamage, incoming: FrameDamage) {
    match (&mut *damage, incoming) {
        (FrameDamage::All, _) | (_, FrameDamage::All) => *damage = FrameDamage::All,
        (FrameDamage::Rows(existing), FrameDamage::Rows(mut rows)) => {
            existing.append(&mut rows);
            existing.sort_unstable();
            existing.dedup();
        }
    }
}

#[derive(Clone)]
struct PaintedPane {
    viewport: TerminalViewport,
    rect: Rect,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct StyledLine {
    segments: Vec<StyledSegment>,
}

impl StyledLine {
    fn parsed(value: &str) -> Self {
        Self {
            segments: parse_styled_segments(value),
        }
    }

    fn from_segments(segments: Vec<StyledSegment>) -> Self {
        let mut line = Self::default();
        for segment in segments {
            line.push_segment(&segment.text, segment.style);
        }
        line
    }

    fn plain(value: &str) -> Self {
        let mut line = Self::default();
        line.push_plain(value);
        line
    }

    fn push_plain(&mut self, value: &str) {
        self.push_segment(value, TmuxStyle::default());
    }

    fn append(&mut self, other: &Self) {
        for segment in &other.segments {
            self.push_segment(&segment.text, segment.style.clone());
        }
    }

    fn push_segment(&mut self, value: &str, style: TmuxStyle) {
        if value.is_empty() {
            return;
        }
        if let Some(last) = self.segments.last_mut().filter(|last| last.style == style) {
            last.text.push_str(value);
        } else {
            self.segments.push(StyledSegment {
                text: value.to_owned(),
                style,
            });
        }
    }

    fn len(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| text_display_width(&segment.text))
            .sum()
    }

    fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    fn truncate(&self, width: usize) -> Self {
        let mut truncated = Self::default();
        let mut remaining = width;
        for segment in &self.segments {
            let text = truncate_display_cells(&segment.text, remaining);
            let segment_was_truncated = text.len() != segment.text.len();
            remaining = remaining.saturating_sub(text_display_width(&text));
            truncated.push_segment(&text, segment.style.clone());
            if segment_was_truncated {
                break;
            }
        }
        truncated
    }

    #[cfg(test)]
    fn plain_text(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PaintedSidebarRow {
    text: StyledLine,
    selected: bool,
    status: bool,
}

pub(crate) struct Renderer {
    output: Vec<u8>,
    queued_control: Vec<u8>,
    overlay_mask: Vec<bool>,
    painted: HashMap<PaneId, PaintedPane>,
    headers: HashMap<PaneId, String>,
    picker_cards: HashMap<PaneId, (Rect, usize)>,
    sidebar_rows: Vec<PaintedSidebarRow>,
    status_rows: Vec<StyledLine>,
    status_geometry: Option<(u16, u16, u16)>,
    damage: HashMap<PaneId, FrameDamage>,
    browser_placements: HashMap<PaneId, KittyPlacement>,
    browser_painted: HashMap<PaneId, bool>,
    last_title: String,
    border_chrome: Option<(PaneBorderStatus, PaneBorderLines, PaneBorderIndicators)>,
    kitty: KittyBridge,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            output: Vec::with_capacity(64 * 1024),
            queued_control: Vec::new(),
            overlay_mask: Vec::new(),
            painted: HashMap::new(),
            headers: HashMap::new(),
            picker_cards: HashMap::new(),
            sidebar_rows: Vec::new(),
            status_rows: Vec::new(),
            status_geometry: None,
            damage: HashMap::new(),
            browser_placements: HashMap::new(),
            browser_painted: HashMap::new(),
            last_title: String::new(),
            border_chrome: None,
            kitty: KittyBridge::default(),
        }
    }

    pub fn invalidate(&mut self) {
        self.painted.clear();
        self.headers.clear();
        self.picker_cards.clear();
        self.sidebar_rows.clear();
        self.status_rows.clear();
        self.status_geometry = None;
        self.damage.clear();
        self.browser_painted.clear();
        self.border_chrome = None;
        self.kitty.invalidate();
    }

    pub fn note_frame(&mut self, pane: PaneId, damage: FrameDamage) {
        self.damage.insert(pane, damage);
    }

    pub fn queue_control(&mut self, output: Vec<u8>) {
        self.queued_control.extend(output);
    }

    pub fn enable_kitty_graphics(&mut self) {
        self.kitty.enable(&mut self.queued_control);
    }

    pub fn set_frame_transport(&mut self, transport: FrameTransport) {
        self.kitty
            .set_transport(transport, &mut self.queued_control);
    }

    pub fn disable_kitty_graphics(&mut self) {
        self.kitty.disable();
        self.browser_placements.clear();
        self.browser_painted.clear();
    }

    pub fn install_kitty_image(&mut self, image: KittyImageData) {
        let _ = self.kitty.install(image, &mut self.queued_control);
    }

    pub fn install_browser_frame(&mut self, update: BrowserFrameUpdate) -> usize {
        let pane = update.image.pane;
        let transmitted = self.kitty.install(update.image, &mut self.queued_control);
        self.browser_placements.insert(pane, update.placement);
        transmitted
    }

    pub fn resize_browser_frame(&mut self, pane: PaneId, cells: (u16, u16)) {
        let Some(placement) = self.browser_placements.get_mut(&pane) else {
            return;
        };
        placement.grid_cols = u32::from(cells.0);
        placement.grid_rows = u32::from(cells.1);
    }

    pub fn remove_browser_frame(&mut self, pane: PaneId) {
        self.browser_placements.remove(&pane);
        self.browser_painted.remove(&pane);
        self.kitty
            .remove_images(pane, &[BROWSER_IMAGE_ID], &mut self.queued_control);
    }

    fn browser_frame_live(&self, pane: PaneId) -> bool {
        self.browser_placements.contains_key(&pane)
    }

    pub fn remove_kitty_images(&mut self, pane: PaneId, image_ids: &[u32]) {
        self.kitty
            .remove_images(pane, image_ids, &mut self.queued_control);
    }

    pub fn forget_pane(&mut self, pane: PaneId) {
        self.painted.remove(&pane);
        self.headers.remove(&pane);
        self.picker_cards.remove(&pane);
        self.damage.remove(&pane);
        self.browser_placements.remove(&pane);
        self.browser_painted.remove(&pane);
        self.kitty.remove_pane(pane, &mut self.queued_control);
    }

    pub fn reset_kitty_images(&mut self) {
        self.browser_placements.clear();
        self.browser_painted.clear();
        self.kitty.reset(&mut self.queued_control);
    }

    pub fn paint(&mut self, model: &Model, force: bool) -> io::Result<()> {
        self.output.clear();
        self.output.extend_from_slice(b"\x1b[?2026h\x1b[?25l");
        let popup_visible = model.popup.is_some();
        let floating_input = popup_visible || model.menu.is_some() || model.confirm.is_some();
        if model.status.title != self.last_title {
            if !model.status.title.is_empty() {
                self.output.extend_from_slice(b"\x1b]2;");
                self.output.extend(
                    model
                        .status
                        .title
                        .bytes()
                        .filter(|byte| *byte >= 0x20 && *byte != 0x7f),
                );
                self.output.push(0x07);
            }
            self.last_title.clone_from(&model.status.title);
        }
        if force {
            clear_screen(&mut self.output, model.appearance.background);
        }

        if model.choose_tree.is_some() || model.choose_buffer.is_some() {
            self.paint_chooser(model);
            self.output.append(&mut self.queued_control);
            self.kitty.suspend(&mut self.output);
            self.hide_cursor();
        } else if let Some((pane, viewport)) = &model.command_output {
            let rect = model.command_output_content_rect();
            self.paint_header_segment(
                Rect {
                    x: 0,
                    y: rect.y.saturating_sub(1),
                    width: model.size.columns,
                    height: 1,
                },
                " command output ",
                true,
                None,
                model,
            );
            self.paint_terminal(*pane, viewport, rect, force, None);
            self.paint_status_block_in(model, 0, model.size.columns, force);
            self.output.append(&mut self.queued_control);
            self.kitty.suspend(&mut self.output);
            if !floating_input {
                self.place_command_output_cursor(*pane, viewport, rect, model);
            }
        } else {
            self.paint_workspace(model, force);
            if model.sidebar_visible() {
                self.paint_sidebar(model, force);
            }
            self.paint_status_block(model, force);
            self.output.append(&mut self.queued_control);
            if !floating_input {
                self.reconcile_kitty_images(model);
                self.place_active_cursor(model);
            }
        }
        if popup_visible {
            self.kitty.suspend(&mut self.output);
            self.paint_popup(model, true);
            if model.menu.is_none() && model.confirm.is_none() {
                self.reconcile_popup_kitty_images(model);
                self.place_popup_cursor(model);
            }
        }
        if model.menu.is_some() {
            self.kitty.suspend(&mut self.output);
            self.paint_menu(model);
            self.hide_cursor();
        } else if model.confirm.is_some() {
            self.kitty.suspend(&mut self.output);
            if model.sidebar_visible() && model.command_output.is_none() {
                self.paint_sidebar(model, true);
            } else if model.command_output.is_some() {
                self.paint_status_block_in(model, 0, model.size.columns, true);
            } else {
                self.paint_status_block(model, true);
            }
            self.hide_cursor();
        }

        self.output.extend_from_slice(b"\x1b[?2026l");
        self.flush_output()
    }

    pub fn paint_frames(&mut self, model: &Model) -> io::Result<()> {
        if let Some(popup) = model.popup.as_ref()
            && model.menu.is_none()
            && model.confirm.is_none()
        {
            if self.damage.keys().any(|pane| *pane != popup.pane) {
                return self.paint(model, false);
            }
            self.output.clear();
            self.output.extend_from_slice(b"\x1b[?2026h\x1b[?25l");
            self.output.append(&mut self.queued_control);
            self.kitty.suspend(&mut self.output);
            self.paint_popup(model, false);
            self.reconcile_popup_kitty_images(model);
            self.place_popup_cursor(model);
            self.output.extend_from_slice(b"\x1b[?2026l");
            return self.flush_output();
        }
        self.output.clear();
        self.output.extend_from_slice(b"\x1b[?2026h\x1b[?25l");
        if model.choose_tree.is_none()
            && model.choose_buffer.is_none()
            && model.command_output.is_none()
            && model.popup.is_none()
            && model.menu.is_none()
            && model.confirm.is_none()
        {
            self.paint_workspace(model, false);
            self.paint_status_area(model);
            self.output.append(&mut self.queued_control);
            self.reconcile_kitty_images(model);
            self.place_active_cursor(model);
        } else {
            self.output.append(&mut self.queued_control);
            self.kitty.suspend(&mut self.output);
            self.hide_cursor();
        }
        self.output.extend_from_slice(b"\x1b[?2026l");
        self.flush_output()
    }

    fn flush_output(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        stdout.write_all(&self.output)?;
        stdout.flush()
    }

    fn paint_workspace(&mut self, model: &Model, force: bool) {
        let lines = model.pane_border_lines();
        let indicators = model.pane_border_indicators();
        let chrome = (model.pane_border_status(), lines, indicators);
        let force = force || self.border_chrome != Some(chrome);
        self.border_chrome = Some(chrome);
        if force {
            let cells = divider_cells(&model.layout.dividers);
            for divider in &model.layout.dividers {
                let highlighted = divider.highlighted;
                let fallback = if highlighted {
                    model.appearance.link_color
                } else {
                    model.appearance.foreground
                };
                let color = divider
                    .style_pane
                    .and_then(|pane| model.pane_border_colour(pane, highlighted))
                    .map_or(fallback, |colour| {
                        resolve_tmux_colour(colour, fallback, &model.appearance)
                    });
                let index = divider.style_pane.and_then(|pane| model.pane_index(pane));
                for row in divider.rect.y..divider.rect.y.saturating_add(divider.rect.height) {
                    for column in divider.rect.x..divider.rect.x.saturating_add(divider.rect.width)
                    {
                        let mut mask = 0;
                        for (present, bit) in [
                            (cells.contains(&(column.wrapping_sub(1), row)), BORDER_L),
                            (cells.contains(&(column.saturating_add(1), row)), BORDER_R),
                            (cells.contains(&(column, row.wrapping_sub(1))), BORDER_U),
                            (cells.contains(&(column, row.saturating_add(1))), BORDER_D),
                        ] {
                            if present {
                                mask |= bit;
                            }
                        }
                        let cell_type = cell_type_of(mask);
                        let glyph = border_arrow(model, indicators, column, row)
                            .map_or_else(|| border_glyph(lines, cell_type, index), str::to_owned);
                        write_colored_text(
                            &mut self.output,
                            column,
                            row,
                            &glyph,
                            color,
                            model.appearance.background,
                        );
                    }
                }
            }
        }

        let active = model.active_pane();
        for entry in &model.layout.panes {
            let Some(pane) = model.pane_snapshot(entry.pane) else {
                continue;
            };
            let active = active == Some(entry.pane);
            let status_row = model.pane_border_status().is_on();
            let header = if status_row {
                pane.border_status_text.clone()
            } else {
                pane_header(model, entry.pane, &pane.title)
            };
            let header_changed = self.headers.get(&entry.pane) != Some(&header);
            if force || header_changed {
                let border = model.pane_border_colour(entry.pane, active);
                if status_row {
                    self.paint_border_status_row(
                        entry.status_row(),
                        &header,
                        active,
                        border,
                        lines,
                        model.pane_index(entry.pane),
                        model,
                    );
                } else {
                    self.paint_header_segment(entry.rect, &header, active, border, model);
                }
                self.headers.insert(entry.pane, header);
            }
            let content = entry.content();
            let browser_live = matches!(pane.kind, PaneKindSnapshot::Browser(_))
                && self.browser_frame_live(entry.pane);
            let browser_state_changed = if matches!(pane.kind, PaneKindSnapshot::Browser(_)) {
                self.browser_painted.insert(entry.pane, browser_live) != Some(browser_live)
            } else {
                self.browser_painted.remove(&entry.pane);
                false
            };
            match &pane.kind {
                PaneKindSnapshot::Terminal => {
                    self.picker_cards.remove(&entry.pane);
                    if let Some(viewport) = model.viewports.get(&entry.pane) {
                        let damage = self.damage.remove(&entry.pane);
                        self.paint_terminal(entry.pane, viewport, content, force, damage.as_ref());
                    } else if force {
                        self.paint_card(
                            content,
                            "Terminal",
                            &pane.title,
                            "waiting for frame",
                            model,
                        );
                    }
                }
                PaneKindSnapshot::Browser(_) if browser_live => {
                    self.picker_cards.remove(&entry.pane);
                    if force || header_changed || browser_state_changed {
                        self.paint_background(content, model);
                    }
                }
                PaneKindSnapshot::Picker if active => {
                    let card_state = (content, model.picker_selection);
                    if force || self.picker_cards.get(&entry.pane) != Some(&card_state) {
                        self.paint_picker_card(content, model.picker_selection, model);
                        self.picker_cards.insert(entry.pane, card_state);
                    }
                }
                kind if force || header_changed || browser_state_changed => {
                    self.picker_cards.remove(&entry.pane);
                    let (label, detail) = placeholder_text(kind);
                    self.paint_card(content, label, &pane.title, &detail, model);
                }
                _ => {}
            }
        }
        let popup = model.popup.as_ref().map(|popup| popup.pane);
        self.painted.retain(|pane, _| {
            popup == Some(*pane) || model.layout.panes.iter().any(|entry| entry.pane == *pane)
        });
        self.damage.retain(|pane, _| {
            popup == Some(*pane) || model.layout.panes.iter().any(|entry| entry.pane == *pane)
        });
        self.picker_cards
            .retain(|pane, _| model.layout.panes.iter().any(|entry| entry.pane == *pane));
        self.browser_painted
            .retain(|pane, _| model.layout.panes.iter().any(|entry| entry.pane == *pane));

        if let Some(display) = &model.display_panes {
            for indicator in &display.indicators {
                if let Some(entry) = model.pane_rect(indicator.pane) {
                    let key = indicator
                        .selection_key()
                        .map_or_else(|| indicator.index.to_string(), |key| key.to_string());
                    write_colored_text(
                        &mut self.output,
                        entry.rect.x.saturating_add(1),
                        entry.rect.y,
                        &format!(" {key} "),
                        model.appearance.background,
                        model.appearance.link_color,
                    );
                    if indicator.label.is_empty() {
                        continue;
                    }
                    let start = entry.rect.x.saturating_add(4);
                    let width = entry
                        .rect
                        .x
                        .saturating_add(entry.rect.width)
                        .saturating_sub(start);
                    if width == 0 {
                        continue;
                    }
                    let composed = zz_client::compose_status_row(&indicator.label, width, "");
                    let line = StyledLine::from_segments(composed.segments);
                    write_styled_text(
                        &mut self.output,
                        start,
                        entry.rect.y,
                        &line,
                        model.appearance.foreground,
                        model.appearance.background,
                        &model.appearance,
                    );
                }
            }
        }
    }

    /// The pinned tree has no image protocol at all, so a popup's own images
    /// are a zz contract: the popup's job writes into its own pane, and that
    /// pane is not in the workspace layout the ordinary reconcile walks. The
    /// suspend above has already retired every workspace placement, so the
    /// popup reconciles alone and its placements are clipped to the content box
    /// inside the border. When the popup closes, the next reconcile no longer
    /// names its pane and the bridge deletes what it placed.
    fn reconcile_popup_kitty_images(&mut self, model: &Model) {
        let Some(popup) = model.popup.as_ref() else {
            return;
        };
        let Some(layout) = model.popup_layout() else {
            return;
        };
        let Some(viewport) = model.viewports.get(&popup.pane) else {
            return;
        };
        if layout.content.width == 0 || layout.content.height == 0 {
            return;
        }
        self.kitty.reconcile(
            std::iter::once((
                popup.pane,
                layout.content,
                viewport.kitty_placements.as_ref(),
            )),
            &mut self.output,
        );
    }

    fn reconcile_kitty_images(&mut self, model: &Model) {
        let browser_placements = &self.browser_placements;
        let panes = model.layout.panes.iter().filter_map(|entry| {
            let pane = model.pane_snapshot(entry.pane)?;
            match &pane.kind {
                PaneKindSnapshot::Terminal => {
                    let viewport = model.viewports.get(&entry.pane)?;
                    Some((
                        entry.pane,
                        entry.content(),
                        viewport.kitty_placements.as_ref(),
                    ))
                }
                PaneKindSnapshot::Browser(_) => {
                    let placement = browser_placements.get(&entry.pane)?;
                    Some((entry.pane, entry.content(), std::slice::from_ref(placement)))
                }
                _ => None,
            }
        });
        self.kitty.reconcile(panes, &mut self.output);
    }

    fn paint_terminal(
        &mut self,
        pane: PaneId,
        viewport: &TerminalViewport,
        rect: Rect,
        force: bool,
        damage: Option<&FrameDamage>,
    ) {
        let previous = self.painted.get(&pane).cloned();
        let structural_change = previous.as_ref().is_none_or(|previous| {
            previous.rect != rect
                || previous.viewport.columns != viewport.columns
                || previous.viewport.rows != viewport.rows
                || previous.viewport.foreground != viewport.foreground
                || previous.viewport.background != viewport.background
        });
        if force || structural_change || matches!(damage, Some(FrameDamage::All)) {
            for row in 0..rect.height {
                self.blit_row(viewport, row, rect);
            }
        } else if let Some(FrameDamage::Rows(rows)) = damage {
            for row in rows.iter().copied().filter(|row| *row < rect.height) {
                if previous.as_ref().is_none_or(|previous| {
                    row_changed(&previous.viewport, viewport, row, rect.width)
                }) {
                    self.blit_row(viewport, row, rect);
                }
            }
        } else {
            for row in 0..rect.height {
                if previous.as_ref().is_none_or(|previous| {
                    row_changed(&previous.viewport, viewport, row, rect.width)
                }) {
                    self.blit_row(viewport, row, rect);
                }
            }
        }
        self.painted.insert(
            pane,
            PaintedPane {
                viewport: viewport.clone(),
                rect,
            },
        );
    }

    fn blit_row(&mut self, viewport: &TerminalViewport, row: u16, rect: Rect) {
        if rect.width == 0 {
            return;
        }
        self.overlay_mask.resize(usize::from(rect.width), false);
        self.overlay_mask.fill(false);
        for overlay in viewport
            .overlays
            .iter()
            .filter(|overlay| overlay.row == row)
        {
            let start = usize::from(overlay.start.min(rect.width));
            let end = usize::from(overlay.end.min(rect.width));
            self.overlay_mask[start..end].fill(true);
        }

        let default_style = viewport.styles().first().copied().unwrap_or_else(|| {
            PackedStyle::new(
                viewport.foreground,
                viewport.background,
                None,
                0,
                UnderlineStyle::None,
            )
        });
        write_cursor_position(&mut self.output, rect.x, rect.y.saturating_add(row));
        let mut current_style = None;
        let mut terminal_column = 0_u16;
        for column in 0..rect.width {
            let cell = viewport.cell(row, column).unwrap_or(PackedCell::EMPTY);
            let style = viewport.style(cell).unwrap_or(default_style);
            let reverse = self.overlay_mask[usize::from(column)];
            if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
                if terminal_column <= column {
                    if terminal_column != column {
                        write_cursor_position(
                            &mut self.output,
                            rect.x.saturating_add(column),
                            rect.y.saturating_add(row),
                        );
                    }
                    if current_style != Some((style, reverse)) {
                        write_sgr(
                            &mut self.output,
                            style,
                            reverse,
                            viewport.foreground,
                            viewport.background,
                        );
                        current_style = Some((style, reverse));
                    }
                    self.output.push(b' ');
                    terminal_column = column.saturating_add(1);
                }
                continue;
            }
            if terminal_column != column {
                write_cursor_position(
                    &mut self.output,
                    rect.x.saturating_add(column),
                    rect.y.saturating_add(row),
                );
            }
            if current_style != Some((style, reverse)) {
                write_sgr(
                    &mut self.output,
                    style,
                    reverse,
                    viewport.foreground,
                    viewport.background,
                );
                current_style = Some((style, reverse));
            }
            let advance = if cell.width() == CellWidth::Wide {
                2
            } else {
                1
            };
            if advance == 2 && column.saturating_add(1) >= rect.width {
                self.output.push(b' ');
                terminal_column = column.saturating_add(1);
                continue;
            }
            match viewport.glyph(cell) {
                Glyph::Empty => self.output.push(b' '),
                Glyph::Scalar(character) => {
                    let mut bytes = [0; 4];
                    self.output
                        .extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
                }
                Glyph::Grapheme(grapheme) => self.output.extend_from_slice(grapheme.as_bytes()),
            }
            terminal_column = column.saturating_add(advance);
        }
        self.output.extend_from_slice(b"\x1b[0m");
    }

    fn paint_header_segment(
        &mut self,
        rect: Rect,
        title: &str,
        active: bool,
        border: Option<TmuxColour>,
        model: &Model,
    ) {
        if rect.height == 0 || rect.width == 0 {
            return;
        }
        let fallback = if active {
            model.appearance.link_color
        } else {
            model.appearance.foreground
        };
        let color = border.map_or(fallback, |colour| {
            resolve_tmux_colour(colour, fallback, &model.appearance)
        });
        let line = padded_segment(title, rect.width, '─');
        write_colored_text(
            &mut self.output,
            rect.x,
            rect.y,
            &line,
            color,
            model.appearance.background,
        );
    }

    /// `window_make_pane_status` fills the row with border cells first and then
    /// lets `format_draw` write the expanded `pane-border-format` over it,
    /// starting two columns right of the pane's left edge (`wp->xoff + 2`).
    fn paint_border_status_row(
        &mut self,
        rect: Rect,
        expanded: &str,
        active: bool,
        border: Option<TmuxColour>,
        lines: PaneBorderLines,
        index: Option<u32>,
        model: &Model,
    ) {
        if rect.height == 0 || rect.width == 0 {
            return;
        }
        let fallback = if active {
            model.appearance.link_color
        } else {
            model.appearance.foreground
        };
        let color = border.map_or(fallback, |colour| {
            resolve_tmux_colour(colour, fallback, &model.appearance)
        });
        let glyph = border_glyph(lines, CELL_LR, index);
        let lead = 2.min(rect.width);
        let width = rect.width.saturating_sub(lead);
        let underlay = vec![glyph.clone(); usize::from(width)];
        let composed = zz_client::compose_status_row_over(expanded, &underlay, "");
        let mut line = StyledLine::default();
        for _ in 0..lead {
            line.push_plain(&glyph);
        }
        line.append(&StyledLine::from_segments(composed.segments));
        write_styled_text(
            &mut self.output,
            rect.x,
            rect.y,
            &line.truncate(usize::from(rect.width)),
            color,
            model.appearance.background,
            &model.appearance,
        );
    }

    fn paint_background(&mut self, rect: Rect, model: &Model) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let line = " ".repeat(usize::from(rect.width));
        for row in 0..rect.height {
            write_colored_text(
                &mut self.output,
                rect.x,
                rect.y.saturating_add(row),
                &line,
                model.appearance.foreground,
                model.appearance.background,
            );
        }
    }

    fn paint_card(&mut self, rect: Rect, kind: &str, title: &str, detail: &str, model: &Model) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        self.paint_background(rect, model);
        if rect.width < 4 || rect.height < 3 {
            return;
        }
        let top = format!(
            "┌{}┐",
            "─".repeat(usize::from(rect.width.saturating_sub(2)))
        );
        let bottom = format!(
            "└{}┘",
            "─".repeat(usize::from(rect.width.saturating_sub(2)))
        );
        write_colored_text(
            &mut self.output,
            rect.x,
            rect.y,
            &top,
            model.appearance.foreground,
            model.appearance.background,
        );
        for row in 1..rect.height.saturating_sub(1) {
            write_colored_text(
                &mut self.output,
                rect.x,
                rect.y.saturating_add(row),
                "│",
                model.appearance.foreground,
                model.appearance.background,
            );
            write_colored_text(
                &mut self.output,
                rect.x.saturating_add(rect.width.saturating_sub(1)),
                rect.y.saturating_add(row),
                "│",
                model.appearance.foreground,
                model.appearance.background,
            );
        }
        write_colored_text(
            &mut self.output,
            rect.x,
            rect.y.saturating_add(rect.height.saturating_sub(1)),
            &bottom,
            model.appearance.foreground,
            model.appearance.background,
        );
        for (offset, text) in [kind, title, detail, "open in the zz app"]
            .into_iter()
            .enumerate()
        {
            let row = rect
                .y
                .saturating_add(1 + u16::try_from(offset).unwrap_or(u16::MAX));
            if row >= rect.y.saturating_add(rect.height.saturating_sub(1)) {
                break;
            }
            let text = truncate(text, rect.width.saturating_sub(4));
            write_colored_text(
                &mut self.output,
                rect.x.saturating_add(2),
                row,
                &text,
                model.appearance.foreground,
                model.appearance.background,
            );
        }
    }

    fn paint_picker_card(&mut self, rect: Rect, selected: usize, model: &Model) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        for row in 0..rect.height {
            write_colored_text(
                &mut self.output,
                rect.x,
                rect.y.saturating_add(row),
                &" ".repeat(usize::from(rect.width)),
                model.appearance.foreground,
                model.appearance.background,
            );
        }

        let card_width = rect.width.min(38);
        let card_height = rect.height.min(8);
        if card_width < 4 || card_height < 3 {
            return;
        }
        let card = Rect {
            x: rect
                .x
                .saturating_add(rect.width.saturating_sub(card_width) / 2),
            y: rect
                .y
                .saturating_add(rect.height.saturating_sub(card_height) / 2),
            width: card_width,
            height: card_height,
        };
        let inner_width = card.width.saturating_sub(2);
        let top = format!(
            "┌{}┐",
            padded_segment(" Select pane kind ", inner_width, '─')
        );
        let bottom = format!("└{}┘", "─".repeat(usize::from(inner_width)));
        write_colored_text(
            &mut self.output,
            card.x,
            card.y,
            &top,
            model.appearance.link_color,
            model.appearance.background,
        );
        for row in 1..card.height.saturating_sub(1) {
            let absolute_row = card.y.saturating_add(row);
            write_colored_text(
                &mut self.output,
                card.x,
                absolute_row,
                "│",
                model.appearance.foreground,
                model.appearance.background,
            );
            write_colored_text(
                &mut self.output,
                card.x.saturating_add(card.width.saturating_sub(1)),
                absolute_row,
                "│",
                model.appearance.foreground,
                model.appearance.background,
            );
        }
        for (index, choice) in picker::CHOICES.into_iter().enumerate() {
            let row = card
                .y
                .saturating_add(1 + u16::try_from(index).unwrap_or(u16::MAX));
            if row >= card.y.saturating_add(card.height.saturating_sub(1)) {
                break;
            }
            let is_selected = index == selected;
            let line = padded_segment(
                &format!("{} {}", if is_selected { '>' } else { ' ' }, choice.label()),
                inner_width,
                ' ',
            );
            write_colored_text(
                &mut self.output,
                card.x.saturating_add(1),
                row,
                &line,
                if is_selected {
                    model.appearance.background
                } else {
                    model.appearance.foreground
                },
                if is_selected {
                    model.appearance.link_color
                } else {
                    model.appearance.background
                },
            );
        }
        if card.height >= 7 {
            let hint = padded_segment(" ↑/↓ or j/k · Enter · Esc", inner_width, ' ');
            write_colored_text(
                &mut self.output,
                card.x.saturating_add(1),
                card.y.saturating_add(card.height.saturating_sub(2)),
                &hint,
                model.appearance.foreground,
                model.appearance.background,
            );
        }
        write_colored_text(
            &mut self.output,
            card.x,
            card.y.saturating_add(card.height.saturating_sub(1)),
            &bottom,
            model.appearance.link_color,
            model.appearance.background,
        );
    }

    fn paint_popup(&mut self, model: &Model, force: bool) {
        let Some(state) = model.popup.as_ref() else {
            return;
        };
        let Some(layout) = model.popup_layout() else {
            self.painted.remove(&state.pane);
            self.damage.remove(&state.pane);
            return;
        };
        if force {
            let style = parse_style(&state.style).unwrap_or_default();
            let mut line = StyledLine::default();
            line.push_segment(&" ".repeat(usize::from(layout.frame.width)), style);
            for row in 0..layout.frame.height {
                write_styled_text(
                    &mut self.output,
                    layout.frame.x,
                    layout.frame.y.saturating_add(row),
                    &line,
                    model.appearance.foreground,
                    model.appearance.background,
                    &model.appearance,
                );
            }
            if state.border_lines != PopupBorderLines::None {
                paint_floating_border(
                    &mut self.output,
                    layout.frame,
                    state.border_lines,
                    &state.border_style,
                    &state.title,
                    model,
                );
            }
        }
        if layout.content.width == 0 || layout.content.height == 0 {
            self.painted.remove(&state.pane);
            self.damage.remove(&state.pane);
            return;
        }
        if let Some(viewport) = model.viewports.get(&state.pane) {
            let damage = self.damage.remove(&state.pane);
            self.paint_terminal(state.pane, viewport, layout.content, force, damage.as_ref());
        } else {
            self.painted.remove(&state.pane);
            self.damage.remove(&state.pane);
        }
    }

    fn paint_menu(&mut self, model: &Model) {
        let Some(state) = model.menu.as_ref() else {
            return;
        };
        let Some(layout) = resolve_floating(
            FloatingSpec {
                left: state.left,
                top: state.top,
                width: state.width,
                height: state.height,
                client_columns: state.client_columns,
                client_rows: state.client_rows,
                border_lines: state.border_lines,
            },
            Rect {
                x: 0,
                y: 0,
                width: model.size.columns,
                height: model.size.rows,
            },
        ) else {
            return;
        };
        let rect = layout.frame;
        if rect.width < 4 || rect.height < 2 {
            return;
        }
        let border = floating_border(state.border_lines);
        let border_style = parse_style(&state.border_style).unwrap_or_default();
        let horizontal = border
            .horizontal
            .repeat(usize::from(rect.width.saturating_sub(2)));
        let mut top = StyledLine::default();
        top.push_segment(
            &format!("{}{}{}", border.top_left, horizontal, border.top_right),
            border_style.clone(),
        );
        write_styled_text(
            &mut self.output,
            rect.x,
            rect.y,
            &top,
            model.appearance.foreground,
            model.appearance.background,
            &model.appearance,
        );

        let content_width = rect.width.saturating_sub(4);
        if !state.title.is_empty() && content_width > 0 {
            let title =
                zz_client::compose_status_row(&state.title, content_width, &state.border_style);
            write_styled_text(
                &mut self.output,
                rect.x.saturating_add(2),
                rect.y,
                &StyledLine::from_segments(title.segments),
                model.appearance.foreground,
                model.appearance.background,
                &model.appearance,
            );
        }

        for offset in 0..rect.height.saturating_sub(2) {
            let index = usize::from(offset);
            let row = rect.y.saturating_add(offset).saturating_add(1);
            match state.items.get(index) {
                Some(None) => {
                    let mut line = StyledLine::default();
                    line.push_segment(
                        &format!(
                            "{}{}{}",
                            border.separator_left, horizontal, border.separator_right
                        ),
                        border_style.clone(),
                    );
                    write_styled_text(
                        &mut self.output,
                        rect.x,
                        row,
                        &line,
                        model.appearance.foreground,
                        model.appearance.background,
                        &model.appearance,
                    );
                }
                Some(Some(item)) => {
                    let selected = item.enabled && model.menu_selection == Some(index);
                    let base_style = if selected {
                        state.selected_style.clone()
                    } else if item.enabled {
                        state.style.clone()
                    } else {
                        format!("{},dim", state.style)
                    };
                    let content = item
                        .annotation
                        .as_deref()
                        .filter(|key| !key.is_empty())
                        .map_or_else(
                            || item.name.clone(),
                            |key| format!("{}#[default] #[align=right]({key})", item.name),
                        );
                    let content =
                        zz_client::compose_status_row(&content, content_width, &base_style);
                    let padding_style = parse_style(&base_style).unwrap_or_default();
                    let mut line = StyledLine::default();
                    line.push_segment(border.vertical, border_style.clone());
                    line.push_segment(" ", padding_style.clone());
                    line.append(&StyledLine::from_segments(content.segments));
                    line.push_segment(" ", padding_style);
                    line.push_segment(border.vertical, border_style.clone());
                    write_styled_text(
                        &mut self.output,
                        rect.x,
                        row,
                        &line,
                        model.appearance.foreground,
                        model.appearance.background,
                        &model.appearance,
                    );
                }
                None => {
                    let base_style = parse_style(&state.style).unwrap_or_default();
                    let mut line = StyledLine::default();
                    line.push_segment(border.vertical, border_style.clone());
                    line.push_segment(
                        &" ".repeat(usize::from(rect.width.saturating_sub(2))),
                        base_style,
                    );
                    line.push_segment(border.vertical, border_style.clone());
                    write_styled_text(
                        &mut self.output,
                        rect.x,
                        row,
                        &line,
                        model.appearance.foreground,
                        model.appearance.background,
                        &model.appearance,
                    );
                }
            }
        }

        let mut bottom = StyledLine::default();
        bottom.push_segment(
            &format!(
                "{}{}{}",
                border.bottom_left, horizontal, border.bottom_right
            ),
            border_style,
        );
        write_styled_text(
            &mut self.output,
            rect.x,
            rect.y.saturating_add(rect.height.saturating_sub(1)),
            &bottom,
            model.appearance.foreground,
            model.appearance.background,
            &model.appearance,
        );
    }

    fn paint_sidebar(&mut self, model: &Model, force: bool) {
        let rows = sidebar_rows(model);
        for (index, row) in rows.iter().enumerate() {
            if !force && self.sidebar_rows.get(index) == Some(row) {
                continue;
            }
            let (foreground, background) = if row.selected {
                (model.appearance.background, model.appearance.link_color)
            } else if row.status {
                (model.appearance.background, model.appearance.foreground)
            } else {
                (model.appearance.foreground, model.appearance.background)
            };
            write_styled_text(
                &mut self.output,
                0,
                u16::try_from(index).unwrap_or(u16::MAX),
                &row.text,
                foreground,
                background,
                &model.appearance,
            );
        }
        if force {
            for row in 0..model.size.rows {
                write_colored_text(
                    &mut self.output,
                    sidebar::WIDTH,
                    row,
                    "│",
                    model.appearance.foreground,
                    model.appearance.background,
                );
            }
        }
        self.sidebar_rows = rows;
    }

    fn paint_status_area(&mut self, model: &Model) {
        if model.sidebar_visible() {
            let tree_height = usize::from(model.sidebar_tree_height());
            for (offset, text) in sidebar_status_lines(model).into_iter().enumerate() {
                let index = tree_height.saturating_add(offset);
                let row = PaintedSidebarRow {
                    text,
                    selected: false,
                    status: true,
                };
                if self.sidebar_rows.get(index) == Some(&row) {
                    continue;
                }
                write_styled_text(
                    &mut self.output,
                    0,
                    u16::try_from(index).unwrap_or(u16::MAX),
                    &row.text,
                    model.appearance.background,
                    model.appearance.foreground,
                    &model.appearance,
                );
                if let Some(cached) = self.sidebar_rows.get_mut(index) {
                    *cached = row;
                }
            }
        }
        self.paint_status_block(model, false);
    }

    fn paint_status_block(&mut self, model: &Model, force: bool) {
        let (x, width) = model.status_area();
        self.paint_status_block_in(model, x, width, force);
    }

    fn paint_status_block_in(&mut self, model: &Model, x: u16, width: u16, force: bool) {
        if model.size.rows == 0 || width == 0 {
            return;
        }
        let block = usize::from(model.status_block_rows());
        let overlay = status_overlay(model, width);
        let origin = model.status_origin_y();
        let mut lines = Vec::with_capacity(block);
        for index in 0..block {
            let row = model.status.rows.get(index).map_or("", String::as_str);
            let composed = zz_client::compose_status_row(row, width, &model.status.base_style);
            let mut line = StyledLine::from_segments(composed.segments);
            if usize::from(model.status.message_line).min(block.saturating_sub(1)) == index {
                match &overlay {
                    Some(StatusOverlay::Row(full)) => line = full.clone(),
                    Some(StatusOverlay::Right(right)) => {
                        line = overlay_right(&line, right, usize::from(width));
                    }
                    None => {}
                }
            }
            lines.push(line);
        }
        let geometry = (x, origin, width);
        let force = force || self.status_geometry != Some(geometry);
        for (index, line) in lines.iter().enumerate() {
            if force || self.status_rows.get(index) != Some(line) {
                write_styled_text(
                    &mut self.output,
                    x,
                    origin.saturating_add(u16::try_from(index).unwrap_or(u16::MAX)),
                    line,
                    model.appearance.foreground,
                    model.appearance.background,
                    &model.appearance,
                );
            }
        }
        self.status_rows = lines;
        self.status_geometry = Some(geometry);
        if block == 0
            && let Some(StatusOverlay::Row(line)) = overlay
            && let Some(y) = model.message_row_y()
        {
            write_styled_text(
                &mut self.output,
                x,
                y,
                &line,
                model.appearance.foreground,
                model.appearance.background,
                &model.appearance,
            );
        }
    }

    fn paint_chooser(&mut self, model: &Model) {
        clear_screen(&mut self.output, model.appearance.background);
        if let Some(state) = &model.choose_tree {
            let title = match state.kind {
                zz_protocol::ChooseTreeKind::Windows => "Choose window",
                zz_protocol::ChooseTreeKind::Panes => "Choose pane",
            };
            write_colored_text(
                &mut self.output,
                0,
                0,
                &padded_segment(title, model.size.columns, ' '),
                model.appearance.background,
                model.appearance.link_color,
            );
            let mut start_row = 1;
            if state.filter_no_matches {
                write_colored_text(
                    &mut self.output,
                    0,
                    start_row,
                    &padded_segment("filter: no matches", model.size.columns, ' '),
                    model.appearance.foreground,
                    model.appearance.background,
                );
                start_row += 1;
            }
            if let Some(search) = &state.search {
                write_colored_text(
                    &mut self.output,
                    0,
                    start_row,
                    &padded_segment(&format!("/{}", search.query), model.size.columns, ' '),
                    model.appearance.foreground,
                    model.appearance.background,
                );
                start_row += 1;
            }
            let key_column = chooser_key_column(state.items.iter().map(|item| item.key.as_str()));
            for (index, item) in state.items.iter().enumerate() {
                let row = start_row + u16::try_from(index).unwrap_or(u16::MAX);
                if row >= model.size.rows.saturating_sub(1) {
                    break;
                }
                let marker = if u32::try_from(index).ok() == Some(state.selected) {
                    ">"
                } else {
                    " "
                };
                let indent = "  ".repeat(usize::from(item.depth));
                let key = chooser_key_cell(&item.key, key_column);
                let row_text = if item.text.is_empty() {
                    format!("{}  {}", item.label, item.detail)
                } else {
                    item.text.clone()
                };
                let text = format!("{marker} {key}{indent}{row_text}");
                write_colored_text(
                    &mut self.output,
                    0,
                    row,
                    &padded_segment(&text, model.size.columns, ' '),
                    model.appearance.foreground,
                    model.appearance.background,
                );
            }
        } else if let Some(state) = &model.choose_buffer {
            write_colored_text(
                &mut self.output,
                0,
                0,
                &padded_segment("Choose buffer", model.size.columns, ' '),
                model.appearance.background,
                model.appearance.link_color,
            );
            let mut start_row = 1;
            if state.filter_no_matches {
                write_colored_text(
                    &mut self.output,
                    0,
                    start_row,
                    &padded_segment("filter: no matches", model.size.columns, ' '),
                    model.appearance.foreground,
                    model.appearance.background,
                );
                start_row += 1;
            }
            if let Some(search) = &state.search {
                write_colored_text(
                    &mut self.output,
                    0,
                    start_row,
                    &padded_segment(&format!("/{}", search.query), model.size.columns, ' '),
                    model.appearance.foreground,
                    model.appearance.background,
                );
                start_row += 1;
            }
            let key_column = chooser_key_column(state.items.iter().map(|item| item.key.as_str()));
            for (index, item) in state.items.iter().enumerate() {
                let row = start_row + u16::try_from(index).unwrap_or(u16::MAX);
                if row >= model.size.rows.saturating_sub(1) {
                    break;
                }
                let marker = if u32::try_from(index).ok() == Some(state.selected) {
                    ">"
                } else {
                    " "
                };
                let key = chooser_key_cell(&item.key, key_column);
                let row_text = if item.text.is_empty() {
                    format!("{}  {} bytes  {}", item.name, item.size_bytes, item.preview)
                } else {
                    item.text.clone()
                };
                let text = format!("{marker} {key}{row_text}");
                write_colored_text(
                    &mut self.output,
                    0,
                    row,
                    &padded_segment(&text, model.size.columns, ' '),
                    model.appearance.foreground,
                    model.appearance.background,
                );
            }
        }
        let help = padded_segment("Enter select  / search  Esc close", model.size.columns, ' ');
        write_colored_text(
            &mut self.output,
            0,
            model.size.rows.saturating_sub(1),
            &help,
            model.appearance.background,
            model.appearance.foreground,
        );
        self.painted.clear();
        self.headers.clear();
        self.picker_cards.clear();
        self.sidebar_rows.clear();
        self.status_rows.clear();
        self.status_geometry = None;
    }

    fn place_active_cursor(&mut self, model: &Model) {
        if model.menu.is_some() || model.confirm.is_some() {
            self.hide_cursor();
            return;
        }
        if model.sidebar_edit.is_some() {
            self.place_sidebar_edit_cursor(model);
            return;
        }
        if model.sidebar.focused {
            self.hide_cursor();
            return;
        }
        if let Some(prompt) = &model.command_prompt {
            let column = prompt
                .prompt
                .chars()
                .count()
                .saturating_add(usize::try_from(prompt.cursor).unwrap_or(usize::MAX));
            let column = u16::try_from(column).unwrap_or(u16::MAX);
            let (start, width, row) = if model.sidebar_visible() {
                (0, sidebar::WIDTH, model.size.rows.saturating_sub(1))
            } else {
                let Some(row) = model.message_row_y() else {
                    self.hide_cursor();
                    return;
                };
                let (x, width) = model.status_area();
                (x, width, row)
            };
            write_cursor_position(
                &mut self.output,
                start.saturating_add(column.min(width.saturating_sub(1))),
                row,
            );
            self.output.extend_from_slice(b"\x1b[6 q\x1b[?25h");
            return;
        }
        let Some(pane) = model.active_pane() else {
            self.hide_cursor();
            return;
        };
        let Some(viewport) = model.viewports.get(&pane) else {
            self.hide_cursor();
            return;
        };
        let Some(entry) = model.pane_rect(pane) else {
            self.hide_cursor();
            return;
        };
        self.place_viewport_cursor(pane, viewport, entry.content(), model);
    }

    fn place_popup_cursor(&mut self, model: &Model) {
        let Some(popup) = model.popup.as_ref() else {
            self.hide_cursor();
            return;
        };
        let Some(layout) = model.popup_layout() else {
            self.hide_cursor();
            return;
        };
        let Some(viewport) = model.viewports.get(&popup.pane) else {
            self.hide_cursor();
            return;
        };
        self.place_viewport_cursor(popup.pane, viewport, layout.content, model);
    }

    fn place_command_output_cursor(
        &mut self,
        pane: PaneId,
        viewport: &TerminalViewport,
        rect: Rect,
        model: &Model,
    ) {
        let Some(query) = &model.command_output_search else {
            self.place_viewport_cursor(pane, viewport, rect, model);
            return;
        };
        let Some(row) = model.message_row_y() else {
            self.hide_cursor();
            return;
        };
        let (_, column) = command_output_search_display(query, model.size.columns);
        write_cursor_position(&mut self.output, column, row);
        self.output.extend_from_slice(b"\x1b[6 q\x1b[?25h");
    }

    fn place_sidebar_edit_cursor(&mut self, model: &Model) {
        let Some(edit) = &model.sidebar_edit else {
            self.hide_cursor();
            return;
        };
        let Some(row) = model.sidebar_edit_row() else {
            self.hide_cursor();
            return;
        };
        let Some(visible_row) = row.checked_sub(model.sidebar.scroll) else {
            self.hide_cursor();
            return;
        };
        if visible_row >= usize::from(model.sidebar_tree_height()) || !model.sidebar_visible() {
            self.hide_cursor();
            return;
        }
        let (_, column) = edit.viewport(sidebar::WIDTH);
        write_cursor_position(
            &mut self.output,
            column,
            u16::try_from(visible_row).unwrap_or(u16::MAX),
        );
        self.output.extend_from_slice(b"\x1b[6 q\x1b[?25h");
    }

    fn place_viewport_cursor(
        &mut self,
        _pane: PaneId,
        viewport: &TerminalViewport,
        rect: Rect,
        _model: &Model,
    ) {
        let Some(cursor) = viewport.cursor.filter(|cursor| cursor.visible()) else {
            self.hide_cursor();
            return;
        };
        let column = cursor
            .column()
            .saturating_sub(u16::from(cursor.at_wide_tail()));
        if column >= rect.width || cursor.row() >= rect.height {
            self.hide_cursor();
            return;
        }
        write_cursor_position(
            &mut self.output,
            rect.x.saturating_add(column),
            rect.y.saturating_add(cursor.row()),
        );
        let shape = match cursor.style() {
            CursorStyle::Block | CursorStyle::BlockHollow => {
                if cursor.blinking() {
                    1
                } else {
                    2
                }
            }
            CursorStyle::Underline => {
                if cursor.blinking() {
                    3
                } else {
                    4
                }
            }
            CursorStyle::Bar => {
                if cursor.blinking() {
                    5
                } else {
                    6
                }
            }
        };
        let color = cursor.color();
        write!(
            self.output,
            "\x1b[{shape} q\x1b]12;#{:02x}{:02x}{:02x}\x07\x1b[?25h",
            color.r, color.g, color.b
        )
        .expect("writing to Vec cannot fail");
    }

    fn hide_cursor(&mut self) {
        self.output.extend_from_slice(b"\x1b[?25l");
    }
}

fn row_changed(
    previous: &TerminalViewport,
    current: &TerminalViewport,
    row: u16,
    width: u16,
) -> bool {
    if viewport_row(previous, row, width) != viewport_row(current, row, width) {
        return true;
    }
    previous
        .overlays
        .iter()
        .filter(|overlay| overlay.row == row)
        .ne(current.overlays.iter().filter(|overlay| overlay.row == row))
}

fn viewport_row(viewport: &TerminalViewport, row: u16, width: u16) -> Option<&[PackedCell]> {
    viewport
        .row(row)
        .map(|cells| &cells[..cells.len().min(usize::from(width))])
}

fn pane_header(model: &Model, pane: PaneId, title: &str) -> String {
    let Some(entry) = model.pane_rect(pane) else {
        return format!(" {title} ");
    };
    let Some(viewport) = model.viewports.get(&pane) else {
        return format!(" {title} ");
    };
    let content = entry.content();
    if viewport.columns == content.width && viewport.rows == content.height {
        format!(" {title} ")
    } else {
        format!(
            " {title} · grid {}×{} (owned elsewhere) ",
            viewport.columns, viewport.rows
        )
    }
}

fn divider_cells(dividers: &[Divider]) -> BTreeSet<(u16, u16)> {
    let mut cells = BTreeSet::new();
    for divider in dividers {
        for row in divider.rect.y..divider.rect.y.saturating_add(divider.rect.height) {
            for column in divider.rect.x..divider.rect.x.saturating_add(divider.rect.width) {
                cells.insert((column, row));
            }
        }
    }
    cells
}

/// `redraw_mark_border_arrows` marks one cell on each side of every pane, at
/// `xoff + 1` on the rows above and below it and at `yoff + 1` on the columns
/// left and right of it, and `redraw_draw_border_arrow` then draws an arrow
/// there only when the active pane is one of that cell's owners, pointing at
/// it: left owner is a left arrow, then right, then top, then bottom.
fn border_arrow(
    model: &Model,
    indicators: PaneBorderIndicators,
    column: u16,
    row: u16,
) -> Option<&'static str> {
    if !indicators.arrows() {
        return None;
    }
    let marked = model.layout.panes.iter().any(|entry| {
        let rect = entry.rect;
        let vertical = column == rect.x.saturating_add(1)
            && ((rect.y > 0 && row == rect.y.saturating_sub(1))
                || row == rect.y.saturating_add(rect.height));
        let horizontal = row == rect.y.saturating_add(1)
            && ((rect.x > 0 && column == rect.x.saturating_sub(1))
                || column == rect.x.saturating_add(rect.width));
        vertical || horizontal
    });
    if !marked {
        return None;
    }
    let rect = model.pane_rect(model.active_pane()?)?.rect;
    let within_columns =
        column >= rect.x.saturating_sub(1) && column <= rect.x.saturating_add(rect.width);
    let within_rows = row >= rect.y.saturating_sub(1) && row <= rect.y.saturating_add(rect.height);
    if column == rect.x.saturating_add(rect.width) && within_rows {
        return Some("←");
    }
    if rect.x > 0 && column == rect.x.saturating_sub(1) && within_rows {
        return Some("→");
    }
    if row == rect.y.saturating_add(rect.height) && within_columns {
        return Some("↑");
    }
    if rect.y > 0 && row == rect.y.saturating_sub(1) && within_columns {
        return Some("↓");
    }
    None
}

fn placeholder_text(kind: &PaneKindSnapshot) -> (&'static str, String) {
    match kind {
        PaneKindSnapshot::Picker => ("Pane picker", "choose a pane in the zz app".to_owned()),
        PaneKindSnapshot::Browser(browser) => ("Browser", browser.url().to_owned()),
        PaneKindSnapshot::Agent(agent) => (
            "Agent",
            agent.cwd.as_ref().map_or_else(
                || agent.provider.label().to_owned(),
                |cwd| format!("{} · {}", agent.provider.label(), cwd.display()),
            ),
        ),
        PaneKindSnapshot::Editor(editor) => (
            "Editor",
            editor.path.clone().unwrap_or_else(|| editor.cwd.clone()),
        ),
        PaneKindSnapshot::Terminal => ("Terminal", String::new()),
    }
}

fn sidebar_rows(model: &Model) -> Vec<PaintedSidebarRow> {
    let tree = model.sidebar_rows();
    let edit_row = model.sidebar_edit_row();
    let tree_height = model.sidebar_tree_height();
    let mut rows = Vec::with_capacity(usize::from(model.size.rows));
    for visible_row in 0..tree_height {
        let index = model
            .sidebar
            .scroll
            .saturating_add(usize::from(visible_row));
        let text = if edit_row == Some(index) {
            model
                .sidebar_edit
                .as_ref()
                .map_or_else(String::new, |edit| edit.viewport(sidebar::WIDTH).0)
        } else {
            tree.get(index)
                .map_or_else(String::new, |row| row.text.clone())
        };
        rows.push(PaintedSidebarRow {
            text: padded_styled(&StyledLine::plain(&text), sidebar::WIDTH, ' '),
            selected: model.sidebar.focused && index == model.sidebar.selected,
            status: false,
        });
    }
    for text in sidebar_status_lines(model) {
        rows.push(PaintedSidebarRow {
            text,
            selected: false,
            status: true,
        });
    }
    rows
}

fn sidebar_status_lines(model: &Model) -> Vec<StyledLine> {
    let line_count = usize::from(model.size.rows.min(sidebar::STATUS_ROWS));
    if line_count == 0 {
        return Vec::new();
    }
    if let Some(confirm) = &model.confirm {
        let mut lines =
            vec![StyledLine::plain(&" ".repeat(usize::from(sidebar::WIDTH))); line_count];
        lines[line_count - 1] =
            padded_styled(&StyledLine::plain(&confirm.prompt), sidebar::WIDTH, ' ');
        return lines;
    }
    if let Some(prompt) = &model.command_prompt {
        let mut lines =
            vec![StyledLine::plain(&" ".repeat(usize::from(sidebar::WIDTH))); line_count];
        lines[line_count - 1] = padded_styled(
            &StyledLine::plain(&format!("{}{}", prompt.prompt, prompt.input)),
            sidebar::WIDTH,
            ' ',
        );
        return lines;
    }

    let base = combine_status(
        &base_status_left(model),
        &StyledLine::parsed(&model.status.right),
        sidebar::WIDTH,
    );
    let indicators = padded_styled(
        &StyledLine::plain(&status_indicators(model)),
        sidebar::WIDTH,
        ' ',
    );
    let message = combine_status(
        &StyledLine::plain(
            model
                .client_message
                .as_ref()
                .map_or("", |message| message.text.as_str()),
        ),
        &StyledLine::plain(if model.status.customized {
            ""
        } else {
            "Ctrl-\\ detach"
        }),
        sidebar::WIDTH,
    );
    [base, indicators, message]
        .into_iter()
        .skip(usize::from(sidebar::STATUS_ROWS).saturating_sub(line_count))
        .collect()
}

enum StatusOverlay {
    Row(StyledLine),
    Right(StyledLine),
}

fn status_overlay(model: &Model, width: u16) -> Option<StatusOverlay> {
    let style = overlay_style(&model.appearance);
    if let Some(confirm) = &model.confirm {
        if model.sidebar_visible() && model.command_output.is_none() {
            return None;
        }
        let mut line = StyledLine::default();
        line.push_segment(&padded_segment(&confirm.prompt, width, ' '), style);
        return Some(StatusOverlay::Row(line));
    }
    if let Some(query) = &model.command_output_search {
        let mut line = StyledLine::default();
        let (prompt, _) = command_output_search_display(query, width);
        line.push_segment(&prompt, style);
        return Some(StatusOverlay::Row(line));
    }
    if model.sidebar_visible() && model.command_output.is_none() {
        return None;
    }
    if let Some(prompt) = &model.command_prompt {
        let mut line = StyledLine::default();
        line.push_segment(
            &padded_segment(&format!("{}{}", prompt.prompt, prompt.input), width, ' '),
            style,
        );
        return Some(StatusOverlay::Row(line));
    }
    if let Some(message) = &model.client_message {
        let mut line = StyledLine::default();
        line.push_segment(&padded_segment(&message.text, width, ' '), style);
        return Some(StatusOverlay::Row(line));
    }
    let mut right = status_indicators(model);
    if !model.status.customized {
        if !right.is_empty() {
            right.push_str("  ");
        }
        right.push_str("Ctrl-\\ detach");
    }
    if right.is_empty() {
        return None;
    }
    let mut line = StyledLine::default();
    line.push_segment(&format!(" {right} "), style);
    Some(StatusOverlay::Right(line))
}

#[derive(Clone, Copy)]
struct FloatingBorder {
    top_left: &'static str,
    top_right: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
    horizontal: &'static str,
    vertical: &'static str,
    separator_left: &'static str,
    separator_right: &'static str,
}

fn floating_border(lines: PopupBorderLines) -> FloatingBorder {
    match lines {
        PopupBorderLines::Single => FloatingBorder {
            top_left: "┌",
            top_right: "┐",
            bottom_left: "└",
            bottom_right: "┘",
            horizontal: "─",
            vertical: "│",
            separator_left: "├",
            separator_right: "┤",
        },
        PopupBorderLines::Double => FloatingBorder {
            top_left: "╔",
            top_right: "╗",
            bottom_left: "╚",
            bottom_right: "╝",
            horizontal: "═",
            vertical: "║",
            separator_left: "╠",
            separator_right: "╣",
        },
        PopupBorderLines::Heavy => FloatingBorder {
            top_left: "┏",
            top_right: "┓",
            bottom_left: "┗",
            bottom_right: "┛",
            horizontal: "━",
            vertical: "┃",
            separator_left: "┣",
            separator_right: "┫",
        },
        PopupBorderLines::Simple => FloatingBorder {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            horizontal: "-",
            vertical: "|",
            separator_left: "+",
            separator_right: "+",
        },
        PopupBorderLines::Rounded => FloatingBorder {
            top_left: "╭",
            top_right: "╮",
            bottom_left: "╰",
            bottom_right: "╯",
            horizontal: "─",
            vertical: "│",
            separator_left: "├",
            separator_right: "┤",
        },
        PopupBorderLines::Padded | PopupBorderLines::None => FloatingBorder {
            top_left: " ",
            top_right: " ",
            bottom_left: " ",
            bottom_right: " ",
            horizontal: " ",
            vertical: " ",
            separator_left: " ",
            separator_right: " ",
        },
    }
}

fn paint_floating_border(
    output: &mut Vec<u8>,
    rect: Rect,
    lines: PopupBorderLines,
    border_style: &str,
    title: &str,
    model: &Model,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let border = floating_border(lines);
    let style = parse_style(border_style).unwrap_or_default();
    let top_text = floating_border_line(
        border.top_left,
        border.horizontal,
        border.top_right,
        rect.width,
    );
    let mut top = StyledLine::default();
    top.push_segment(&top_text, style.clone());
    write_styled_text(
        output,
        rect.x,
        rect.y,
        &top,
        model.appearance.foreground,
        model.appearance.background,
        &model.appearance,
    );
    for offset in 1..rect.height.saturating_sub(1) {
        let row = rect.y.saturating_add(offset);
        let mut left = StyledLine::default();
        left.push_segment(border.vertical, style.clone());
        write_styled_text(
            output,
            rect.x,
            row,
            &left,
            model.appearance.foreground,
            model.appearance.background,
            &model.appearance,
        );
        if rect.width > 1 {
            write_styled_text(
                output,
                rect.x.saturating_add(rect.width.saturating_sub(1)),
                row,
                &left,
                model.appearance.foreground,
                model.appearance.background,
                &model.appearance,
            );
        }
    }
    if rect.height > 1 {
        let bottom_text = floating_border_line(
            border.bottom_left,
            border.horizontal,
            border.bottom_right,
            rect.width,
        );
        let mut bottom = StyledLine::default();
        bottom.push_segment(&bottom_text, style);
        write_styled_text(
            output,
            rect.x,
            rect.y.saturating_add(rect.height.saturating_sub(1)),
            &bottom,
            model.appearance.foreground,
            model.appearance.background,
            &model.appearance,
        );
    }
    let title_width = rect.width.saturating_sub(4);
    if !title.is_empty() && title_width > 0 {
        let title = zz_client::compose_status_row(title, title_width, border_style);
        write_styled_text(
            output,
            rect.x.saturating_add(2),
            rect.y,
            &StyledLine::from_segments(title.segments),
            model.appearance.foreground,
            model.appearance.background,
            &model.appearance,
        );
    }
}

fn floating_border_line(left: &str, horizontal: &str, right: &str, width: u16) -> String {
    match width {
        0 => String::new(),
        1 => left.to_owned(),
        _ => format!(
            "{left}{}{right}",
            horizontal.repeat(usize::from(width.saturating_sub(2)))
        ),
    }
}

fn overlay_style(appearance: &TerminalAppearance) -> TmuxStyle {
    TmuxStyle {
        fg: Some(TmuxColour::Rgb(appearance.background.packed())),
        bg: Some(TmuxColour::Rgb(appearance.foreground.packed())),
        ..TmuxStyle::default()
    }
}

fn overlay_right(line: &StyledLine, overlay: &StyledLine, width: usize) -> StyledLine {
    let overlay_width = overlay.len();
    let keep = width.saturating_sub(overlay_width);
    let mut merged = line.truncate(keep);
    let padding = keep.saturating_sub(merged.len());
    merged.push_plain(&" ".repeat(padding));
    merged.append(overlay);
    merged
}

fn base_status_left(model: &Model) -> StyledLine {
    let mut left = StyledLine::parsed(&model.status.left);
    if let Some(session) = model.session() {
        if !left.is_empty() {
            left.push_plain(" ");
        }
        left.push_plain(&format!("[{}]", session.name));
        for window in &session.windows {
            append_status_label(&mut left, &window.status_label);
        }
    }
    left
}

/// Width of the shortcut gutter, empty when no row has one. `mode_tree_draw`
/// sizes it from the widest key plus the two parentheses and a trailing space,
/// and rows without a key pad to the same column.
fn chooser_key_column<'a>(keys: impl Iterator<Item = &'a str>) -> usize {
    keys.filter(|key| !key.is_empty())
        .map(|key| key.chars().count() + 3)
        .max()
        .unwrap_or(0)
}

fn chooser_key_cell(key: &str, column: usize) -> String {
    if column == 0 {
        return String::new();
    }
    let cell = if key.is_empty() {
        String::new()
    } else {
        format!("({key})")
    };
    let padding = column.saturating_sub(cell.chars().count());
    format!("{cell}{}", " ".repeat(padding))
}

fn append_status_label(line: &mut StyledLine, value: &str) {
    let label = StyledLine::parsed(value);
    if label.is_empty() {
        return;
    }
    line.push_plain(" ");
    line.append(&label);
}

/// The mode badge for the status row. `copy-mode -H` suppresses the position
/// and nothing else, matching the `!data->hide_position` guard that decides
/// whether `window_copy_write_line` draws `copy-mode-position-format` at all.
fn mode_indicator(mode: TerminalMode) -> String {
    match mode {
        TerminalMode::Live => String::new(),
        TerminalMode::Copy {
            hide_position: true,
            ..
        } => "COPY".to_owned(),
        TerminalMode::Copy {
            position, total, ..
        } => format!("COPY {position}/{total}"),
        TerminalMode::View { position, total } => format!("VIEW {position}/{total}"),
    }
}

fn status_indicators(model: &Model) -> String {
    let mut indicators = String::new();
    let viewport = model
        .command_output
        .as_ref()
        .map(|(_, viewport)| viewport)
        .or_else(|| model.active_viewport());
    if let Some(viewport) = viewport {
        match viewport.mode {
            TerminalMode::Live => {}
            TerminalMode::Copy { .. } | TerminalMode::View { .. } => {
                indicators.push_str(&mode_indicator(viewport.mode));
            }
        }
        if let Some(search) = viewport.search {
            if !indicators.is_empty() {
                indicators.push_str("  ");
            }
            write!(indicators, "search {}/{}", search.current(), search.total)
                .expect("writing to String cannot fail");
        }
    }
    if model.prefix_armed {
        if !indicators.is_empty() {
            indicators.push_str("  ");
        }
        indicators.push_str("PREFIX");
    }
    indicators
}

fn command_output_search_prompt(query: &SearchQuery) -> String {
    let prefix = match query.direction {
        SearchDirection::Forward => '/',
        SearchDirection::Backward => '?',
    };
    format!("{prefix}{}", query.text)
}

fn command_output_search_display(query: &SearchQuery, width: u16) -> (String, u16) {
    let prompt = command_output_search_prompt(query);
    let visible = truncate(&prompt, width.saturating_sub(1));
    let cursor = u16::try_from(text_display_width(&visible)).unwrap_or(u16::MAX);
    (padded_segment(&visible, width, ' '), cursor)
}

fn combine_status(left: &StyledLine, right: &StyledLine, width: u16) -> StyledLine {
    let width = usize::from(width);
    let right = right.truncate(width);
    let right_len = right.len();
    let left_width = width.saturating_sub(right_len + usize::from(!right.is_empty()));
    let mut left = left.truncate(left_width);
    let left_len = left.len();
    let gap = width.saturating_sub(left_len + right_len);
    left.push_plain(&" ".repeat(gap));
    left.append(&right);
    left
}

fn padded_styled(line: &StyledLine, width: u16, fill: char) -> StyledLine {
    let mut line = line.truncate(usize::from(width));
    let padding = usize::from(width).saturating_sub(line.len());
    line.push_plain(&fill_cells(fill, padding));
    line
}

fn padded_segment(text: &str, width: u16, fill: char) -> String {
    let text = truncate(text, width);
    let padding = usize::from(width).saturating_sub(text_display_width(&text));
    format!("{text}{}", fill_cells(fill, padding))
}

fn truncate(text: &str, width: u16) -> String {
    truncate_display_cells(text, usize::from(width))
}

fn truncate_display_cells(text: &str, width: usize) -> String {
    let mut used = 0_usize;
    text.chars()
        .take_while(|character| {
            let next = used.saturating_add(character.width().unwrap_or(0));
            if next > width {
                false
            } else {
                used = next;
                true
            }
        })
        .collect()
}

fn text_display_width(text: &str) -> usize {
    text.chars()
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

fn fill_cells(fill: char, width: usize) -> String {
    let fill_width = fill.width().unwrap_or(0);
    if fill_width == 0 {
        return " ".repeat(width);
    }
    let count = width / fill_width;
    let remainder = width % fill_width;
    format!(
        "{}{}",
        fill.to_string().repeat(count),
        " ".repeat(remainder)
    )
}

fn clear_screen(output: &mut Vec<u8>, background: Color) {
    write!(
        output,
        "\x1b[0;48;2;{};{};{}m\x1b[2J",
        background.r, background.g, background.b
    )
    .expect("writing to Vec cannot fail");
}

fn write_cursor_position(output: &mut Vec<u8>, column: u16, row: u16) {
    write!(
        output,
        "\x1b[{};{}H",
        row.saturating_add(1),
        column.saturating_add(1)
    )
    .expect("writing to Vec cannot fail");
}

fn write_colored_text(
    output: &mut Vec<u8>,
    column: u16,
    row: u16,
    text: &str,
    foreground: Color,
    background: Color,
) {
    write_cursor_position(output, column, row);
    write!(
        output,
        "\x1b[0;38;2;{};{};{};48;2;{};{};{}m",
        foreground.r, foreground.g, foreground.b, background.r, background.g, background.b
    )
    .expect("writing to Vec cannot fail");
    output.extend_from_slice(text.as_bytes());
    output.extend_from_slice(b"\x1b[0m");
}

fn write_styled_text(
    output: &mut Vec<u8>,
    column: u16,
    row: u16,
    line: &StyledLine,
    foreground: Color,
    background: Color,
    appearance: &TerminalAppearance,
) {
    write_cursor_position(output, column, row);
    for segment in &line.segments {
        write_tmux_sgr(output, &segment.style, foreground, background, appearance);
        output.extend_from_slice(segment.text.as_bytes());
    }
    output.extend_from_slice(b"\x1b[0m");
}

fn write_tmux_sgr(
    output: &mut Vec<u8>,
    style: &TmuxStyle,
    default_foreground: Color,
    default_background: Color,
    appearance: &TerminalAppearance,
) {
    output.extend_from_slice(b"\x1b[0m");
    let attributes = &style.attributes;
    if attributes.noattr != TmuxAttributeState::On {
        if attributes.bold == TmuxAttributeState::On {
            output.extend_from_slice(b"\x1b[1m");
        }
        if attributes.dim == TmuxAttributeState::On {
            output.extend_from_slice(b"\x1b[2m");
        }
        if attributes.italics == TmuxAttributeState::On {
            output.extend_from_slice(b"\x1b[3m");
        }
        for (state, sequence) in [
            (attributes.underscore, b"\x1b[4m".as_slice()),
            (attributes.double_underscore, b"\x1b[4:2m".as_slice()),
            (attributes.curly_underscore, b"\x1b[4:3m".as_slice()),
            (attributes.dotted_underscore, b"\x1b[4:4m".as_slice()),
            (attributes.dashed_underscore, b"\x1b[4:5m".as_slice()),
        ] {
            if state == TmuxAttributeState::On {
                output.extend_from_slice(sequence);
            }
        }
        if attributes.blink == TmuxAttributeState::On {
            output.extend_from_slice(b"\x1b[5m");
        }
        if attributes.reverse == TmuxAttributeState::On {
            output.extend_from_slice(b"\x1b[7m");
        }
        if attributes.hidden == TmuxAttributeState::On {
            output.extend_from_slice(b"\x1b[8m");
        }
        if attributes.strikethrough == TmuxAttributeState::On {
            output.extend_from_slice(b"\x1b[9m");
        }
        if attributes.overline == TmuxAttributeState::On {
            output.extend_from_slice(b"\x1b[53m");
        }
    }
    if let Some(colour) = style.us {
        let colour = resolve_tmux_colour(colour, default_foreground, appearance);
        write!(output, "\x1b[58;2;{};{};{}m", colour.r, colour.g, colour.b)
            .expect("writing to Vec cannot fail");
    }
    let foreground = style.fg.map_or(default_foreground, |colour| {
        resolve_tmux_colour(colour, default_foreground, appearance)
    });
    let background = style.bg.map_or(default_background, |colour| {
        resolve_tmux_colour(colour, default_background, appearance)
    });
    write!(
        output,
        "\x1b[38;2;{};{};{};48;2;{};{};{}m",
        foreground.r, foreground.g, foreground.b, background.r, background.g, background.b
    )
    .expect("writing to Vec cannot fail");
}

fn resolve_tmux_colour(
    colour: TmuxColour,
    fallback: Color,
    appearance: &TerminalAppearance,
) -> Color {
    match colour {
        TmuxColour::Basic(index) | TmuxColour::Indexed(index) => {
            appearance.palette[usize::from(index)]
        }
        TmuxColour::Rgb(value) => Color::from_packed(value),
        TmuxColour::Theme(index) => [0, 7, 7, 0, 2, 3, 1, 4, 6, 5]
            .get(usize::from(index))
            .map_or(fallback, |index| appearance.palette[*index]),
        TmuxColour::Default | TmuxColour::Terminal => fallback,
    }
}

fn write_sgr(
    output: &mut Vec<u8>,
    style: PackedStyle,
    reverse: bool,
    default_foreground: Color,
    default_background: Color,
) {
    output.extend_from_slice(b"\x1b[0m");
    if style.bold() {
        output.extend_from_slice(b"\x1b[1m");
    }
    if style.faint() {
        output.extend_from_slice(b"\x1b[2m");
    }
    if style.italic() {
        output.extend_from_slice(b"\x1b[3m");
    }
    match style.underline() {
        UnderlineStyle::None if style.hyperlink() => output.extend_from_slice(b"\x1b[4m"),
        UnderlineStyle::None => {}
        UnderlineStyle::Single => output.extend_from_slice(b"\x1b[4m"),
        UnderlineStyle::Double => output.extend_from_slice(b"\x1b[4:2m"),
        UnderlineStyle::Curly => output.extend_from_slice(b"\x1b[4:3m"),
        UnderlineStyle::Dotted => output.extend_from_slice(b"\x1b[4:4m"),
        UnderlineStyle::Dashed => output.extend_from_slice(b"\x1b[4:5m"),
    }
    if let Some(color) = style.underline_color() {
        write!(output, "\x1b[58;2;{};{};{}m", color.r, color.g, color.b)
            .expect("writing to Vec cannot fail");
    }
    if style.blink() {
        output.extend_from_slice(b"\x1b[5m");
    }
    if style.invisible() {
        output.extend_from_slice(b"\x1b[8m");
    }
    if style.strikethrough() {
        output.extend_from_slice(b"\x1b[9m");
    }
    if style.overline() {
        output.extend_from_slice(b"\x1b[53m");
    }
    let foreground = style.foreground();
    if foreground == default_foreground {
        output.extend_from_slice(b"\x1b[39m");
    } else {
        write!(
            output,
            "\x1b[38;2;{};{};{}m",
            foreground.r, foreground.g, foreground.b
        )
        .expect("writing to Vec cannot fail");
    }
    let background = style.background();
    if background == default_background {
        output.extend_from_slice(b"\x1b[49m");
    } else {
        write!(
            output,
            "\x1b[48;2;{};{};{}m",
            background.r, background.g, background.b
        )
        .expect("writing to Vec cannot fail");
    }
    if reverse {
        output.extend_from_slice(b"\x1b[7m");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::state::ClientMessage;
    use zz_protocol::MenuState;
    use zz_terminal::{CellWidth, Cursor, SessionStatus, TerminalDictionary};

    fn styled_viewport() -> TerminalViewport {
        let mut viewport = TerminalViewport::blank(3, 1, SessionStatus::Running);
        let first = PackedStyle::new(
            Color::rgb(255, 0, 0),
            viewport.background,
            None,
            zz_terminal::ATTR_BOLD,
            UnderlineStyle::None,
        );
        let second = PackedStyle::new(
            Color::rgb(0, 255, 0),
            viewport.background,
            None,
            0,
            UnderlineStyle::None,
        );
        viewport.dictionary = Arc::new(TerminalDictionary::from_shared(
            Arc::from([first, second]),
            Arc::from([0]),
            Arc::from([]),
        ));
        viewport.cells = Arc::from([
            PackedCell::new('a' as u32, 0, CellWidth::Narrow),
            PackedCell::new('b' as u32, 0, CellWidth::Narrow),
            PackedCell::new('c' as u32, 1, CellWidth::Narrow),
        ]);
        viewport
    }

    #[test]
    fn sgr_changes_once_per_style_run() {
        let viewport = styled_viewport();
        let mut renderer = Renderer::new();
        renderer.blit_row(
            &viewport,
            0,
            Rect {
                x: 0,
                y: 0,
                width: 3,
                height: 1,
            },
        );
        let output = String::from_utf8(renderer.output).unwrap();

        assert_eq!(output.matches("\x1b[38;2;").count(), 2);
        assert!(output.contains("ab"));
        assert!(output.contains('c'));
    }

    #[test]
    fn styled_lines_clip_text_without_counting_markers() {
        let line = StyledLine::parsed("#[fg=red]abcd#[bold]ef");
        assert_eq!(line.len(), 6);
        assert_eq!(line.plain_text(), "abcdef");

        let clipped = line.truncate(5);
        assert_eq!(clipped.plain_text(), "abcde");
        assert_eq!(clipped.segments.len(), 2);
        assert_eq!(clipped.segments[0].style.fg, Some(TmuxColour::Basic(1)));
        assert_eq!(
            clipped.segments[1].style.attributes.bold,
            TmuxAttributeState::On
        );
    }

    #[test]
    fn plain_text_clipping_uses_terminal_display_cells() {
        assert_eq!(truncate("界ab", 3), "界a");
        assert_eq!(truncate("界ab", 1), "");
        assert_eq!(truncate("e\u{301}界", 1), "e\u{301}");

        let padded = padded_segment("界e\u{301}", 5, ' ');
        assert_eq!(padded, "界e\u{301}  ");
        assert_eq!(text_display_width(&padded), 5);

        let wide_fill = padded_segment("a", 4, '界');
        assert_eq!(text_display_width(&wide_fill), 4);
        assert_eq!(wide_fill, "a界 ");
    }

    #[test]
    fn styled_lines_render_palette_rgb_and_attributes_without_markers() {
        let appearance = TerminalAppearance::default();
        let line = StyledLine::parsed(
            "#[fg=red,bg=#010203,bold,underscore,reverse]X#[fg=colour42,nobold,nounderscore,noreverse]Y",
        );
        let mut output = Vec::new();
        write_styled_text(
            &mut output,
            0,
            0,
            &line,
            appearance.background,
            appearance.foreground,
            &appearance,
        );
        let output = String::from_utf8(output).unwrap();
        let red = appearance.palette[1];
        let indexed = appearance.palette[42];

        assert!(output.contains("\x1b[1m"));
        assert!(output.contains("\x1b[4m"));
        assert!(output.contains("\x1b[7m"));
        assert!(output.contains(&format!("38;2;{};{};{}", red.r, red.g, red.b)));
        assert!(output.contains("48;2;1;2;3"));
        assert!(output.contains(&format!("38;2;{};{};{}", indexed.r, indexed.g, indexed.b)));
        assert!(output.contains('X'));
        assert!(output.contains('Y'));
        assert!(!output.contains("#["));
    }

    #[test]
    fn style_only_changes_invalidate_cached_lines() {
        assert_ne!(
            StyledLine::parsed("#[fg=red]same"),
            StyledLine::parsed("#[fg=blue]same")
        );
    }

    #[test]
    fn window_status_labels_use_daemon_text_and_keep_empty_formats_empty() {
        let mut line = StyledLine::plain("[session]");
        append_status_label(&mut line, "#[bold]CUSTOM");
        append_status_label(&mut line, "#[default]");

        assert_eq!(line.plain_text(), "[session] CUSTOM");
        assert_eq!(line.segments.len(), 2);
        assert_eq!(
            line.segments[1].style.attributes.bold,
            TmuxAttributeState::On
        );
    }

    #[test]
    fn unchanged_grid_emits_no_row_output() {
        let viewport = styled_viewport();
        let rect = Rect {
            x: 0,
            y: 0,
            width: 3,
            height: 1,
        };
        let mut renderer = Renderer::new();
        renderer.paint_terminal(PaneId(1), &viewport, rect, true, None);
        assert!(!renderer.output.is_empty());
        renderer.output.clear();

        renderer.paint_terminal(PaneId(1), &viewport, rect, false, None);

        assert!(renderer.output.is_empty());
    }

    fn block_model(columns: u16, rows: u16) -> Model {
        let core = zz_client::ClientCore::new();
        let endpoint =
            zz_daemon::Endpoint::parse("unix:///tmp/zz-render-test.sock").expect("test endpoint");
        Model::new(
            &core,
            crate::tty::TerminalSize {
                columns,
                rows,
                cell_width_px: 8,
                cell_height_px: 16,
            },
            "host".to_owned(),
            "host".to_owned(),
            endpoint.clone(),
            endpoint,
            Vec::new(),
        )
    }

    fn menu_state() -> MenuState {
        MenuState {
            left: 5,
            top: 2,
            width: 20,
            height: 5,
            client_columns: 40,
            client_rows: 12,
            cell_width_px: 8,
            cell_height_px: 16,
            title: "Actions".to_owned(),
            style: "fg=white,bg=blue".to_owned(),
            selected_style: "fg=black,bg=yellow,bold".to_owned(),
            border_style: "fg=red".to_owned(),
            border_lines: PopupBorderLines::Single,
            items: vec![
                Some(zz_protocol::MenuItem {
                    name: "界 item".to_owned(),
                    key: Some("q".to_owned()),
                    annotation: Some("q".to_owned()),
                    enabled: true,
                }),
                None,
                Some(zz_protocol::MenuItem {
                    name: "Disabled".to_owned(),
                    key: None,
                    annotation: None,
                    enabled: false,
                }),
            ],
            selected: Some(0),
            stay_open: false,
            mouse_keys: false,
        }
    }

    fn popup_state(border_lines: PopupBorderLines) -> zz_protocol::PopupState {
        zz_protocol::PopupState {
            pane: PaneId(u64::MAX - 1),
            left: 5,
            top: 2,
            width: 12,
            height: 5,
            client_columns: 40,
            client_rows: 12,
            cell_width_px: 8,
            cell_height_px: 16,
            title: "Popup title".to_owned(),
            style: "fg=white,bg=blue".to_owned(),
            border_style: "fg=red,bold".to_owned(),
            border_lines,
            close_on_exit: false,
            close_on_exit_zero: false,
            close_on_any_key: false,
            dead: false,
        }
    }

    fn popup_model(border_lines: PopupBorderLines, with_frame: bool) -> Model {
        let mut model = block_model(40, 12);
        let state = popup_state(border_lines);
        if with_frame {
            let mut viewport = TerminalViewport::blank(10, 3, SessionStatus::Running);
            let mut cells = viewport.cells.as_ref().to_vec();
            cells[0] = PackedCell::new('p' as u32, 0, CellWidth::Narrow);
            cells[1] = PackedCell::new('o' as u32, 0, CellWidth::Narrow);
            cells[2] = PackedCell::new('p' as u32, 0, CellWidth::Narrow);
            viewport.cells = Arc::from(cells);
            viewport.cursor = Some(Cursor::new(
                1,
                0,
                true,
                false,
                false,
                CursorStyle::Bar,
                viewport.foreground,
            ));
            model.viewports.insert(state.pane, viewport);
        }
        model.popup = Some(state);
        model
    }

    #[test]
    fn popup_paints_bounded_title_styles_border_content_and_cursor() {
        let model = popup_model(PopupBorderLines::Single, true);
        let mut renderer = Renderer::new();
        renderer.paint_popup(&model, true);
        renderer.place_popup_cursor(&model);
        let output = String::from_utf8(renderer.output).unwrap();
        let red = model.appearance.palette[1];
        let blue = model.appearance.palette[4];

        assert!(output.contains("\x1b[3;6H"), "{output:?}");
        assert!(output.contains("Popup ti"), "{output:?}");
        assert!(output.contains('┌'));
        assert!(output.contains('┐'));
        assert!(output.contains('└'));
        assert!(output.contains('┘'));
        assert!(output.contains("pop"));
        assert!(output.contains("\x1b[1m"));
        assert!(output.contains(&format!("38;2;{};{};{}", red.r, red.g, red.b)));
        assert!(output.contains(&format!("48;2;{};{};{}", blue.r, blue.g, blue.b)));
        assert!(output.contains("\x1b[4;8H"), "{output:?}");
        assert!(output.contains("\x1b[6 q\x1b]12;"));
    }

    #[test]
    fn borderless_popup_uses_its_full_frame_and_missing_frame_hides_the_underlay_cursor() {
        let model = popup_model(PopupBorderLines::None, false);
        let layout = model.popup_layout().expect("popup layout");
        assert_eq!(layout.content, layout.frame);
        let mut renderer = Renderer::new();
        renderer.paint_popup(&model, true);
        renderer.place_popup_cursor(&model);
        let output = String::from_utf8(renderer.output).unwrap();

        assert!(output.contains("\x1b[3;6H"), "{output:?}");
        assert!(!output.contains('┌'));
        assert!(!output.contains("Popup title"));
        assert!(output.ends_with("\x1b[?25l"));
        assert!(
            !renderer
                .painted
                .contains_key(&popup_state(PopupBorderLines::None).pane)
        );
    }

    #[test]
    fn popup_damage_survives_workspace_retention_and_repaints_only_changed_rows() {
        let mut model = popup_model(PopupBorderLines::Single, true);
        let pane = model.popup.as_ref().expect("popup").pane;
        let content = model.popup_layout().expect("layout").content;
        let mut renderer = Renderer::new();
        renderer.paint_popup(&model, true);
        renderer.output.clear();
        renderer.note_frame(pane, FrameDamage::Rows(vec![1]));
        renderer.paint_workspace(&model, false);
        assert_eq!(
            renderer.damage.get(&pane),
            Some(&FrameDamage::Rows(vec![1]))
        );
        assert!(renderer.painted.contains_key(&pane));

        let viewport = model.viewports.get_mut(&pane).expect("popup frame");
        let mut cells = viewport.cells.as_ref().to_vec();
        cells[usize::from(viewport.columns)] = PackedCell::new('x' as u32, 0, CellWidth::Narrow);
        viewport.cells = Arc::from(cells);
        renderer.paint_popup(&model, false);
        let output = String::from_utf8(renderer.output).unwrap();
        assert!(output.contains('x'));
        assert!(output.contains(&format!(
            "\x1b[{};{}H",
            content.y.saturating_add(2),
            content.x.saturating_add(1)
        )));
        assert!(!output.contains(&format!(
            "\x1b[{};{}H",
            content.y.saturating_add(1),
            content.x.saturating_add(1)
        )));
    }

    #[test]
    fn higher_menu_paints_after_popup_and_suppresses_its_cursor() {
        let mut model = popup_model(PopupBorderLines::Single, true);
        model.menu = Some(menu_state());
        model.menu_selection = Some(0);
        let mut renderer = Renderer::new();
        renderer.paint_popup(&model, true);
        renderer.paint_menu(&model);
        renderer.hide_cursor();
        let output = String::from_utf8(renderer.output).unwrap();
        let popup = output.find("Popup ti").expect("popup title");
        let menu = output.rfind("Actions").expect("menu title");
        assert!(popup < menu);
        assert!(output.rfind("\x1b[?25l").is_some_and(|hide| hide > menu));
    }

    fn popup_kitty_placement(image_id: u32, generation: u64) -> zz_terminal::KittyPlacement {
        zz_terminal::KittyPlacement {
            image_id,
            image_generation: generation,
            layer: zz_terminal::KittyLayer::AboveText,
            viewport_col: 0,
            viewport_row: 0,
            absolute_row: 0,
            cell_offset_x: 0,
            cell_offset_y: 0,
            grid_cols: 2,
            grid_rows: 1,
            pixel_width: 2,
            pixel_height: 1,
            source_rect: None,
        }
    }

    /// The pinned tree carries no image protocol anywhere - popup content goes
    /// through `input_parse_screen` into the popup's own screen with no image
    /// path - so a popup's images are a zz contract rather than a fidelity gap.
    /// This is that contract: a placement inside the popup is drawn against the
    /// content box inside the border, a replacement retires the placement it
    /// replaces, and closing the popup deletes what the popup placed.
    #[test]
    fn popup_images_are_placed_replaced_and_cleaned_up_on_close() {
        let mut model = popup_model(PopupBorderLines::Single, true);
        let pane = model.popup.as_ref().expect("popup").pane;
        let content = model.popup_layout().expect("popup layout").content;
        let mut renderer = Renderer::new();
        renderer.enable_kitty_graphics();
        for image_id in [9, 10] {
            renderer.install_kitty_image(KittyImageData {
                pane,
                image_id,
                generation: 1,
                width: 2,
                height: 1,
                bytes: vec![0, 0, 0, 255, 0, 0, 0, 255],
            });
        }
        renderer.queued_control.clear();

        let viewport = model.viewports.get_mut(&pane).expect("popup viewport");
        viewport.kitty_placements = Arc::from([popup_kitty_placement(9, 1)]);
        renderer.reconcile_popup_kitty_images(&model);
        let placed = String::from_utf8(std::mem::take(&mut renderer.output)).unwrap();
        assert!(placed.contains("\x1b_Ga=p,"), "{placed:?}");
        assert!(
            placed.contains(&format!("\x1b[{};{}H", content.y + 1, content.x + 1)),
            "the placement is anchored inside the border: {placed:?}"
        );

        let viewport = model.viewports.get_mut(&pane).expect("popup viewport");
        viewport.kitty_placements = Arc::from([popup_kitty_placement(10, 1)]);
        renderer.reconcile_popup_kitty_images(&model);
        let replaced = String::from_utf8(std::mem::take(&mut renderer.output)).unwrap();
        assert!(
            replaced.contains("\x1b_Ga=d,d=i,"),
            "the replaced placement is retired: {replaced:?}"
        );
        assert!(replaced.contains("\x1b_Ga=p,"), "{replaced:?}");

        model.popup = None;
        renderer.reconcile_kitty_images(&model);
        let closed = String::from_utf8(std::mem::take(&mut renderer.output)).unwrap();
        assert!(
            closed.contains("\x1b_Ga=d,d=i,"),
            "closing the popup deletes what it placed: {closed:?}"
        );
        assert!(
            !closed.contains("\x1b_Ga=p,"),
            "and places nothing new: {closed:?}"
        );
    }

    #[test]
    fn forgetting_a_popup_purges_terminal_damage_and_image_caches() {
        let model = popup_model(PopupBorderLines::Single, true);
        let pane = model.popup.as_ref().expect("popup").pane;
        let mut renderer = Renderer::new();
        renderer.paint_popup(&model, true);
        renderer.note_frame(pane, FrameDamage::Rows(vec![0]));
        renderer.headers.insert(pane, "header".to_owned());
        renderer.picker_cards.insert(pane, (Rect::default(), 0));
        renderer.enable_kitty_graphics();
        renderer.install_kitty_image(KittyImageData {
            pane,
            image_id: 9,
            generation: 1,
            width: 1,
            height: 1,
            bytes: vec![0, 0, 0, 255],
        });
        renderer.queued_control.clear();

        renderer.forget_pane(pane);

        assert!(!renderer.painted.contains_key(&pane));
        assert!(!renderer.damage.contains_key(&pane));
        assert!(!renderer.headers.contains_key(&pane));
        assert!(!renderer.picker_cards.contains_key(&pane));
        assert!(String::from_utf8_lossy(&renderer.queued_control).contains("\x1b_Ga=d,d=I"));
    }

    #[test]
    fn floating_border_tables_cover_every_published_line_style() {
        assert_eq!(floating_border(PopupBorderLines::Single).top_left, "┌");
        assert_eq!(
            floating_border(PopupBorderLines::Double).separator_left,
            "╠"
        );
        assert_eq!(floating_border(PopupBorderLines::Heavy).bottom_right, "┛");
        assert_eq!(floating_border(PopupBorderLines::Simple).vertical, "|");
        assert_eq!(floating_border(PopupBorderLines::Rounded).top_right, "╮");
        assert_eq!(floating_border(PopupBorderLines::Padded).horizontal, " ");
        assert_eq!(floating_border(PopupBorderLines::None).top_left, " ");
    }

    #[test]
    fn menu_paints_title_rows_shortcuts_selection_and_disabled_style() {
        let mut model = block_model(40, 12);
        model.menu = Some(menu_state());
        model.menu_selection = Some(0);
        let mut renderer = Renderer::new();
        renderer.paint_menu(&model);
        let output = String::from_utf8(renderer.output).unwrap();

        assert!(output.contains("\x1b[3;6H"), "{output:?}");
        assert!(output.contains("Actions"));
        assert!(output.contains("界 item"));
        assert!(output.contains("(q)"));
        assert!(output.contains('┌'));
        assert!(output.contains('├'));
        assert!(output.contains('┘'));
        assert!(output.contains("\x1b[1m"));
        assert!(output.contains("\x1b[2m"));
    }

    #[test]
    fn menu_hides_the_workspace_cursor() {
        let mut model = block_model(40, 12);
        model.menu = Some(menu_state());
        let mut renderer = Renderer::new();
        renderer.place_active_cursor(&model);
        assert_eq!(renderer.output, b"\x1b[?25l");
    }

    fn block_status(rows: Vec<&str>, customized: bool) -> zz_protocol::StatusLine {
        zz_protocol::StatusLine {
            rows: rows.into_iter().map(str::to_owned).collect(),
            customized,
            ..zz_protocol::StatusLine::default()
        }
    }

    #[test]
    fn status_block_paints_daemon_rows_and_blank_rows_carry_base_style() {
        let mut model = block_model(40, 10);
        let mut status = block_status(vec!["#[fg=red,bold]HOT", ""], true);
        status.base_style = "bg=blue".to_owned();
        model.set_status(status);
        let mut renderer = Renderer::new();
        renderer.paint_status_block(&model, true);
        let output = String::from_utf8(renderer.output).unwrap();
        let blue = model.appearance.palette[4];
        let red = model.appearance.palette[1];

        assert!(output.contains("\x1b[9;1H"), "row 8 paints: {output:?}");
        assert!(
            output.contains("\x1b[10;1H"),
            "blank row 9 paints: {output:?}"
        );
        assert!(output.contains("HOT"));
        assert!(output.contains("\x1b[1m"));
        assert!(output.contains(&format!("48;2;{};{};{}", blue.r, blue.g, blue.b)));
        assert!(output.contains(&format!("38;2;{};{};{}", red.r, red.g, red.b)));
        assert!(!output.contains("#["));
    }

    #[test]
    fn status_position_top_paints_the_block_at_row_zero() {
        let mut model = block_model(40, 10);
        let mut status = block_status(vec!["TOPROW"], true);
        status.position = zz_protocol::StatusPosition::Top;
        model.set_status(status);
        let mut renderer = Renderer::new();
        renderer.paint_status_block(&model, true);
        let output = String::from_utf8(renderer.output).unwrap();

        assert!(output.contains("\x1b[1;1H"), "{output:?}");
        assert!(output.contains("TOPROW"));
    }

    #[test]
    fn a_client_message_replaces_the_message_line_row() {
        let mut model = block_model(40, 10);
        let mut status = block_status(vec!["ROWZERO", "ROWONE"], true);
        status.message_line = 1;
        model.set_status(status);
        model.client_message = Some(ClientMessage::local("hello message"));
        let mut renderer = Renderer::new();
        renderer.paint_status_block(&model, true);
        let output = String::from_utf8(renderer.output).unwrap();

        assert!(output.contains("hello message"));
        assert!(output.contains("ROWZERO"));
        assert!(!output.contains("ROWONE"), "{output:?}");
    }

    #[test]
    fn the_detach_hint_overlays_default_status_but_not_customized_status() {
        let mut model = block_model(40, 10);
        model.set_status(block_status(vec!["ROW"], false));
        let mut renderer = Renderer::new();
        renderer.paint_status_block(&model, true);
        let output = String::from_utf8(renderer.output).unwrap();
        assert!(output.contains("Ctrl-\\ detach"), "{output:?}");

        model.set_status(block_status(vec!["ROW"], true));
        let mut renderer = Renderer::new();
        renderer.paint_status_block(&model, true);
        let output = String::from_utf8(renderer.output).unwrap();
        assert!(!output.contains("Ctrl-\\ detach"), "{output:?}");
        assert!(output.contains("ROW"));
    }

    #[test]
    fn a_message_with_status_off_paints_one_virtual_row() {
        let mut model = block_model(40, 10);
        model.set_status(zz_protocol::StatusLine {
            customized: true,
            ..zz_protocol::StatusLine::default()
        });
        model.client_message = Some(ClientMessage::local("virtual"));
        let mut renderer = Renderer::new();
        renderer.paint_status_block(&model, true);
        let output = String::from_utf8(renderer.output).unwrap();

        assert!(
            output.contains("\x1b[10;1H"),
            "bottom virtual row: {output:?}"
        );
        assert!(output.contains("virtual"));
    }

    #[test]
    fn confirm_prompt_replaces_the_status_row_and_other_local_overlays() {
        let mut model = block_model(40, 10);
        model.set_status(zz_protocol::StatusLine {
            customized: true,
            ..zz_protocol::StatusLine::default()
        });
        model.command_output_search = Some(SearchQuery::literal("hidden search"));
        model.client_message = Some(ClientMessage::local("hidden message"));
        model.confirm = Some(zz_protocol::ConfirmState {
            prompt: "Confirm attached? (y/n) ".to_owned(),
            confirm_key: b'y',
            default_yes: false,
        });
        let mut renderer = Renderer::new();
        renderer.paint_status_block(&model, true);
        let output = String::from_utf8(renderer.output).unwrap();

        assert!(output.contains("\x1b[10;1H"), "{output:?}");
        assert!(output.contains("Confirm attached? (y/n) "));
        assert!(!output.contains("hidden search"));
        assert!(!output.contains("hidden message"));

        let mut sidebar_model = block_model(80, 10);
        assert!(sidebar_model.sidebar_visible());
        sidebar_model.confirm = model.confirm;
        assert!(status_overlay(&sidebar_model, 80).is_none());
        let lines = sidebar_status_lines(&sidebar_model);
        assert_eq!(lines.len(), 3);
        assert!(lines.last().is_some_and(|line| {
            line.segments
                .iter()
                .any(|segment| segment.text.contains("Confirm attached? (y/n) "))
        }));

        let mut sidebar_renderer = Renderer::new();
        sidebar_renderer.paint_sidebar(&sidebar_model, true);
        sidebar_renderer.paint_status_block(&sidebar_model, true);
        let output = String::from_utf8(sidebar_renderer.output).unwrap();
        assert_eq!(output.matches("Confirm attached? (y/n) ").count(), 1);
    }

    #[test]
    fn browser_frame_switches_the_renderer_between_card_and_live_modes() {
        let pane = PaneId(7);
        let mut renderer = Renderer::new();
        assert!(!renderer.browser_frame_live(pane));
        renderer.enable_kitty_graphics();
        renderer.install_browser_frame(BrowserFrameUpdate {
            image: KittyImageData {
                pane,
                image_id: BROWSER_IMAGE_ID,
                generation: 4,
                width: 2,
                height: 1,
                bytes: vec![0, 0, 255, 255, 0, 255, 0, 255],
            },
            placement: KittyPlacement {
                image_id: BROWSER_IMAGE_ID,
                image_generation: 4,
                layer: zz_terminal::KittyLayer::AboveText,
                viewport_col: 0,
                viewport_row: 0,
                absolute_row: 0,
                cell_offset_x: 0,
                cell_offset_y: 0,
                grid_cols: 2,
                grid_rows: 1,
                pixel_width: 2,
                pixel_height: 1,
                source_rect: None,
            },
        });

        assert!(renderer.browser_frame_live(pane));
        assert!(String::from_utf8_lossy(&renderer.queued_control).contains("\x1b_Ga=t"));
        renderer.resize_browser_frame(pane, (4, 3));
        assert_eq!(renderer.browser_placements[&pane].grid_cols, 4);
        assert_eq!(renderer.browser_placements[&pane].grid_rows, 3);

        renderer.remove_browser_frame(pane);
        assert!(!renderer.browser_frame_live(pane));
    }
    #[test]
    fn copy_mode_hides_only_its_position_when_the_daemon_asks() {
        assert_eq!(
            mode_indicator(TerminalMode::Copy {
                position: 3,
                total: 40,
                hide_position: false,
            }),
            "COPY 3/40"
        );
        assert_eq!(
            mode_indicator(TerminalMode::Copy {
                position: 3,
                total: 40,
                hide_position: true,
            }),
            "COPY"
        );
        assert_eq!(
            mode_indicator(TerminalMode::View {
                position: 3,
                total: 40,
            }),
            "VIEW 3/40"
        );
        assert_eq!(mode_indicator(TerminalMode::Live), "");
    }

    #[test]
    fn command_output_search_prompt_tracks_direction_and_status_overlay() {
        let mut model = block_model(12, 8);
        model.command_output_search = Some(SearchQuery {
            text: "needle".to_owned(),
            direction: SearchDirection::Backward,
            ..SearchQuery::default()
        });
        assert_eq!(
            command_output_search_prompt(model.command_output_search.as_ref().unwrap()),
            "?needle"
        );
        let Some(StatusOverlay::Row(line)) = status_overlay(&model, 12) else {
            panic!("command output search did not replace the status row");
        };
        assert_eq!(line.plain_text(), "?needle     ");

        model.command_output_search.as_mut().unwrap().direction = SearchDirection::Forward;
        assert_eq!(
            command_output_search_prompt(model.command_output_search.as_ref().unwrap()),
            "/needle"
        );

        let mut sidebar_model = block_model(80, 8);
        assert!(sidebar_model.sidebar_visible());
        sidebar_model.command_output_search = Some(SearchQuery::literal("visible"));
        assert!(matches!(
            status_overlay(&sidebar_model, 80),
            Some(StatusOverlay::Row(_))
        ));

        let query = SearchQuery::literal("界e\u{301}界");
        let (painted, cursor) = command_output_search_display(&query, 6);
        assert_eq!(painted, "/界e\u{301}  ");
        assert_eq!(text_display_width(&painted), 6);
        assert_eq!(cursor, 4);
    }

    #[test]
    fn command_output_viewport_owns_the_mode_indicator() {
        let mut model = block_model(80, 8);
        let mut viewport = TerminalViewport::blank(80, 6, SessionStatus::Running);
        viewport.mode = TerminalMode::View {
            position: 3,
            total: 40,
        };
        model.command_output = Some((PaneId(9), viewport));
        assert!(model.sidebar_visible());
        assert!(status_indicators(&model).starts_with("VIEW 3/40"));
        let Some(StatusOverlay::Right(overlay)) = status_overlay(&model, 80) else {
            panic!("command output mode indicator was suppressed with the hidden sidebar");
        };
        assert!(overlay.plain_text().contains("VIEW 3/40"));
    }

    #[test]
    fn the_chooser_key_gutter_sizes_once_and_pads_keyless_rows() {
        assert_eq!(chooser_key_column(["0", "M-a", ""].into_iter()), 6);
        assert_eq!(chooser_key_column(["", ""].into_iter()), 0);
        assert_eq!(chooser_key_cell("0", 6), "(0)   ");
        assert_eq!(chooser_key_cell("M-a", 6), "(M-a) ");
        assert_eq!(chooser_key_cell("", 6), "      ");
        assert_eq!(chooser_key_cell("", 0), "");
        assert_eq!(chooser_key_cell("0", 0), "");
    }

    #[test]
    fn the_chooser_paints_every_row_key_in_its_gutter() {
        let mut model = block_model(60, 12);
        model.choose_tree = Some(zz_protocol::ChooseTreeState {
            items: vec![
                zz_protocol::ChooseTreeItem {
                    label: "alpha".to_owned(),
                    detail: "1 window".to_owned(),
                    target: zz_protocol::ChooseTreeTarget::Session(zz_protocol::SessionId(1)),
                    depth: 0,
                    flags: 0,
                    pane_kind: None,
                    key: "0".to_owned(),
                    text: String::new(),
                },
                zz_protocol::ChooseTreeItem {
                    label: "beta".to_owned(),
                    detail: "1 window".to_owned(),
                    target: zz_protocol::ChooseTreeTarget::Session(zz_protocol::SessionId(2)),
                    depth: 0,
                    flags: 0,
                    pane_kind: None,
                    key: "M-a".to_owned(),
                    text: String::new(),
                },
                zz_protocol::ChooseTreeItem {
                    label: "gamma".to_owned(),
                    detail: "1 window".to_owned(),
                    target: zz_protocol::ChooseTreeTarget::Session(zz_protocol::SessionId(3)),
                    depth: 0,
                    flags: 0,
                    pane_kind: None,
                    key: String::new(),
                    text: String::new(),
                },
            ],
            search: None,
            selected: 0,
            kind: zz_protocol::ChooseTreeKind::Windows,
            filter_no_matches: false,
        });
        let mut renderer = Renderer::new();
        renderer.paint_chooser(&model);
        let output = String::from_utf8(renderer.output).unwrap();

        assert!(output.contains("(0)   alpha"), "{output}");
        assert!(output.contains("(M-a) beta"), "{output}");
        assert!(output.contains("      gamma"), "{output}");
    }

    #[test]
    fn a_row_format_takes_over_the_row_the_chooser_would_have_composed() {
        let mut model = block_model(60, 12);
        model.choose_tree = Some(zz_protocol::ChooseTreeState {
            items: vec![zz_protocol::ChooseTreeItem {
                label: "alpha".to_owned(),
                detail: "1 window".to_owned(),
                target: zz_protocol::ChooseTreeTarget::Session(zz_protocol::SessionId(1)),
                depth: 0,
                flags: 0,
                pane_kind: None,
                key: "0".to_owned(),
                text: "ZZTREE<%3>".to_owned(),
            }],
            search: None,
            selected: 0,
            kind: zz_protocol::ChooseTreeKind::Windows,
            filter_no_matches: false,
        });
        let mut renderer = Renderer::new();
        renderer.paint_chooser(&model);
        let output = String::from_utf8(renderer.output).unwrap();
        assert!(output.contains("(0) ZZTREE<%3>"), "{output}");
        assert!(!output.contains("alpha"), "{output}");
        assert!(!output.contains("1 window"), "{output}");

        let mut model = block_model(60, 12);
        model.choose_buffer = Some(zz_protocol::ChooseBufferState {
            items: vec![zz_protocol::ChooseBufferItem {
                name: "buffer0".to_owned(),
                preview: "hello".to_owned(),
                size_bytes: 5,
                created_unix_seconds: 0,
                key: "0".to_owned(),
                text: "ZZBUF<buffer0>".to_owned(),
            }],
            search: None,
            selected: 0,
            filter_no_matches: false,
        });
        let mut renderer = Renderer::new();
        renderer.paint_chooser(&model);
        let output = String::from_utf8(renderer.output).unwrap();
        assert!(output.contains("(0) ZZBUF<buffer0>"), "{output}");
        assert!(!output.contains("5 bytes"), "{output}");
        assert!(!output.contains("hello"), "{output}");
    }

    #[test]
    fn chooser_fallback_status_keeps_fully_keyless_rows_selectable_without_a_gutter() {
        let mut model = block_model(60, 12);
        model.choose_buffer = Some(zz_protocol::ChooseBufferState {
            items: vec![zz_protocol::ChooseBufferItem {
                name: "keyless".to_owned(),
                preview: "fallback row".to_owned(),
                size_bytes: 1,
                created_unix_seconds: 0,
                key: String::new(),
                text: String::new(),
            }],
            search: None,
            selected: 0,
            filter_no_matches: true,
        });
        let mut renderer = Renderer::new();
        renderer.paint_chooser(&model);
        let output = String::from_utf8(renderer.output).unwrap();

        assert!(output.contains("filter: no matches"), "{output}");
        assert!(output.contains("> keyless  1 bytes"), "{output}");
        assert!(!output.contains(">      keyless"), "{output}");
    }
}
