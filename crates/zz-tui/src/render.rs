use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{self, Write as _},
};

use zz_protocol::{Axis, PaneId, PaneKindSnapshot};
use zz_terminal::{
    CellWidth, Color, CursorStyle, Glyph, KittyPlacement, PackedCell, PackedStyle, TerminalMode,
    TerminalViewport, UnderlineStyle,
};

use crate::{
    browser::{BROWSER_IMAGE_ID, BrowserFrameUpdate},
    kitty::{FrameTransport, KittyBridge, KittyImageData},
    layout::Rect,
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct PaintedSidebarRow {
    text: String,
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
    status: String,
    damage: HashMap<PaneId, FrameDamage>,
    browser_placements: HashMap<PaneId, KittyPlacement>,
    browser_painted: HashMap<PaneId, bool>,
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
            status: String::new(),
            damage: HashMap::new(),
            browser_placements: HashMap::new(),
            browser_painted: HashMap::new(),
            kitty: KittyBridge::default(),
        }
    }

    pub fn invalidate(&mut self) {
        self.painted.clear();
        self.headers.clear();
        self.picker_cards.clear();
        self.sidebar_rows.clear();
        self.status.clear();
        self.damage.clear();
        self.browser_painted.clear();
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

    pub fn remove_kitty_pane(&mut self, pane: PaneId) {
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
        if force {
            clear_screen(&mut self.output, model.appearance.background);
        }

        if model.choose_tree.is_some() || model.choose_buffer.is_some() {
            self.paint_chooser(model);
            self.output.append(&mut self.queued_control);
            self.kitty.suspend(&mut self.output);
            self.hide_cursor();
        } else if let Some((pane, viewport)) = &model.command_output {
            let rect = Rect {
                x: 0,
                y: 1,
                width: model.size.columns,
                height: model.size.rows.saturating_sub(2),
            };
            self.paint_header_segment(
                Rect {
                    x: 0,
                    y: 0,
                    width: model.size.columns,
                    height: 1,
                },
                " command output ",
                true,
                model,
            );
            self.paint_terminal(*pane, viewport, rect, force, None);
            self.paint_status(model);
            self.output.append(&mut self.queued_control);
            self.kitty.suspend(&mut self.output);
            self.place_viewport_cursor(*pane, viewport, rect, model);
        } else {
            self.paint_workspace(model, force);
            if model.sidebar_visible() {
                self.paint_sidebar(model, force);
            } else {
                self.paint_status(model);
            }
            self.output.append(&mut self.queued_control);
            self.reconcile_kitty_images(model);
            self.place_active_cursor(model);
        }

        self.output.extend_from_slice(b"\x1b[?2026l");
        self.flush_output()
    }

    pub fn paint_frames(&mut self, model: &Model) -> io::Result<()> {
        self.output.clear();
        self.output.extend_from_slice(b"\x1b[?2026h\x1b[?25l");
        if model.choose_tree.is_none()
            && model.choose_buffer.is_none()
            && model.command_output.is_none()
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
        if force {
            for divider in &model.layout.dividers {
                let color = if divider.highlighted {
                    model.appearance.link_color
                } else {
                    model.appearance.foreground
                };
                match divider.axis {
                    Axis::Horizontal => {
                        for row in
                            divider.rect.y..divider.rect.y.saturating_add(divider.rect.height)
                        {
                            write_colored_text(
                                &mut self.output,
                                divider.rect.x,
                                row,
                                "│",
                                color,
                                model.appearance.background,
                            );
                        }
                    }
                    Axis::Vertical => {
                        let line = "─".repeat(usize::from(divider.rect.width));
                        write_colored_text(
                            &mut self.output,
                            divider.rect.x,
                            divider.rect.y,
                            &line,
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
            let header = pane_header(model, entry.pane, &pane.title);
            let header_changed = self.headers.get(&entry.pane) != Some(&header);
            if force || header_changed {
                self.paint_header_segment(entry.rect, &header, active, model);
                self.headers.insert(entry.pane, header);
            }
            let content = entry.rect.content();
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
        self.painted
            .retain(|pane, _| model.layout.panes.iter().any(|entry| entry.pane == *pane));
        self.damage
            .retain(|pane, _| model.layout.panes.iter().any(|entry| entry.pane == *pane));
        self.picker_cards
            .retain(|pane, _| model.layout.panes.iter().any(|entry| entry.pane == *pane));
        self.browser_painted
            .retain(|pane, _| model.layout.panes.iter().any(|entry| entry.pane == *pane));

        if let Some(display) = &model.display_panes {
            for indicator in &display.indicators {
                if let Some(entry) = model.pane_rect(indicator.pane) {
                    let label = indicator
                        .selection_key()
                        .map_or_else(|| indicator.index.to_string(), |key| key.to_string());
                    write_colored_text(
                        &mut self.output,
                        entry.rect.x.saturating_add(1),
                        entry.rect.y,
                        &format!(" {label} "),
                        model.appearance.background,
                        model.appearance.link_color,
                    );
                }
            }
        }
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
                        entry.rect.content(),
                        viewport.kitty_placements.as_ref(),
                    ))
                }
                PaneKindSnapshot::Browser(_) => {
                    let placement = browser_placements.get(&entry.pane)?;
                    Some((
                        entry.pane,
                        entry.rect.content(),
                        std::slice::from_ref(placement),
                    ))
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

    fn paint_header_segment(&mut self, rect: Rect, title: &str, active: bool, model: &Model) {
        if rect.height == 0 || rect.width == 0 {
            return;
        }
        let color = if active {
            model.appearance.link_color
        } else {
            model.appearance.foreground
        };
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
            write_colored_text(
                &mut self.output,
                0,
                u16::try_from(index).unwrap_or(u16::MAX),
                &row.text,
                foreground,
                background,
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
        if !model.sidebar_visible() {
            self.paint_status(model);
            return;
        }
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
            write_colored_text(
                &mut self.output,
                0,
                u16::try_from(index).unwrap_or(u16::MAX),
                &row.text,
                model.appearance.background,
                model.appearance.foreground,
            );
            if let Some(cached) = self.sidebar_rows.get_mut(index) {
                *cached = row;
            }
        }
    }

    fn paint_status(&mut self, model: &Model) {
        if model.size.rows == 0 {
            return;
        }
        let line = status_line(model, model.size.columns);
        if self.status == line {
            return;
        }
        write_colored_text(
            &mut self.output,
            0,
            model.size.rows.saturating_sub(1),
            &line,
            model.appearance.background,
            model.appearance.foreground,
        );
        self.status = line;
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
            let start_row = if let Some(search) = &state.search {
                write_colored_text(
                    &mut self.output,
                    0,
                    1,
                    &padded_segment(&format!("/{}", search.query), model.size.columns, ' '),
                    model.appearance.foreground,
                    model.appearance.background,
                );
                2
            } else {
                1
            };
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
                let text = format!("{marker} {indent}{}  {}", item.label, item.detail);
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
            let start_row = if let Some(search) = &state.search {
                write_colored_text(
                    &mut self.output,
                    0,
                    1,
                    &padded_segment(&format!("/{}", search.query), model.size.columns, ' '),
                    model.appearance.foreground,
                    model.appearance.background,
                );
                2
            } else {
                1
            };
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
                let text = format!(
                    "{marker} {}  {} bytes  {}",
                    item.name, item.size_bytes, item.preview
                );
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
        self.status.clear();
    }

    fn place_active_cursor(&mut self, model: &Model) {
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
            let status_width = if model.sidebar_visible() {
                sidebar::WIDTH
            } else {
                model.size.columns
            };
            let column = u16::try_from(column)
                .unwrap_or(u16::MAX)
                .min(status_width.saturating_sub(1));
            write_cursor_position(&mut self.output, column, model.size.rows.saturating_sub(1));
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
        self.place_viewport_cursor(pane, viewport, entry.rect.content(), model);
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
    let content = entry.rect.content();
    if viewport.columns == content.width && viewport.rows == content.height {
        format!(" {title} ")
    } else {
        format!(
            " {title} · grid {}×{} (owned elsewhere) ",
            viewport.columns, viewport.rows
        )
    }
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
            text: padded_segment(&text, sidebar::WIDTH, ' '),
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

fn sidebar_status_lines(model: &Model) -> Vec<String> {
    let line_count = usize::from(model.size.rows.min(sidebar::STATUS_ROWS));
    if line_count == 0 {
        return Vec::new();
    }
    if let Some(prompt) = &model.command_prompt {
        let mut lines = vec![" ".repeat(usize::from(sidebar::WIDTH)); line_count];
        lines[line_count - 1] = padded_segment(
            &format!("{}{}", prompt.prompt, prompt.input),
            sidebar::WIDTH,
            ' ',
        );
        return lines;
    }

    let base = combine_status(
        &base_status_left(model),
        &model.status.right,
        sidebar::WIDTH,
    );
    let indicators = padded_segment(&status_indicators(model), sidebar::WIDTH, ' ');
    let message = combine_status(
        model.client_message.as_deref().unwrap_or_default(),
        "Ctrl-\\ detach",
        sidebar::WIDTH,
    );
    [base, indicators, message]
        .into_iter()
        .skip(usize::from(sidebar::STATUS_ROWS).saturating_sub(line_count))
        .collect()
}

fn status_line(model: &Model, width: u16) -> String {
    if let Some(prompt) = &model.command_prompt {
        return padded_segment(&format!("{}{}", prompt.prompt, prompt.input), width, ' ');
    }
    let mut left = base_status_left(model);
    let indicators = status_indicators(model);
    if !indicators.is_empty() {
        left.push_str("  ");
        left.push_str(&indicators);
    }
    if let Some(message) = &model.client_message {
        left.push_str("  ");
        left.push_str(message);
    }
    left.push_str("  Ctrl-\\ detach");
    combine_status(&left, &model.status.right, width)
}

fn base_status_left(model: &Model) -> String {
    let mut left = model.status.left.clone();
    if let Some(session) = model.session() {
        if !left.is_empty() {
            left.push(' ');
        }
        write!(left, "[{}]", session.name).expect("writing to String cannot fail");
        for window in &session.windows {
            let marker = if model.snapshot.focused_window_for(session) == window.id {
                '*'
            } else {
                ' '
            };
            write!(left, " {marker}{}:{}", window.index, window.name)
                .expect("writing to String cannot fail");
        }
    }
    left
}

fn status_indicators(model: &Model) -> String {
    let mut indicators = String::new();
    if let Some(viewport) = model.active_viewport() {
        match viewport.mode {
            TerminalMode::Live => {}
            TerminalMode::Copy { position, total } => {
                write!(indicators, "COPY {position}/{total}")
                    .expect("writing to String cannot fail");
            }
            TerminalMode::View { position, total } => {
                write!(indicators, "VIEW {position}/{total}")
                    .expect("writing to String cannot fail");
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

fn combine_status(left: &str, right: &str, width: u16) -> String {
    let width = usize::from(width);
    let right = truncate(right, u16::try_from(width).unwrap_or(u16::MAX));
    let right_len = right.chars().count();
    let left_width = width.saturating_sub(right_len + usize::from(!right.is_empty()));
    let left = truncate(left, u16::try_from(left_width).unwrap_or(u16::MAX));
    let left_len = left.chars().count();
    let gap = width.saturating_sub(left_len + right_len);
    format!("{left}{}{right}", " ".repeat(gap))
}

fn padded_segment(text: &str, width: u16, fill: char) -> String {
    let text = truncate(text, width);
    let padding = usize::from(width).saturating_sub(text.chars().count());
    format!("{text}{}", fill.to_string().repeat(padding))
}

fn truncate(text: &str, width: u16) -> String {
    text.chars().take(usize::from(width)).collect()
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
    use zz_terminal::{CellWidth, SessionStatus, TerminalDictionary};

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
}
