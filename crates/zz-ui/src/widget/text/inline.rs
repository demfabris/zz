//! One painted run of text: link hit-testing, the selection highlight, and the
//! line bounds the window selection controller hit-tests against.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

use std::{
    ops::Range,
    rc::Rc,
    sync::{Arc, Mutex},
};

use gpui::{
    App, BorderStyle, Bounds, CursorStyle, Edges, Element, ElementId, GlobalElementId, Half,
    HighlightStyle, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, StyledText,
    TextAlign, TextLayout, Window, WrappedLineLayout, point, px, quad,
};

use crate::{ActiveTheme, WindowExt as _};

use super::{
    global::TextGlobal,
    node::LinkMark,
    selection::{Selection, word_range_at},
    state::TextViewMultiClickKind,
    veil::{StreamingVeil, VeilKey},
    window_selection,
};
use crate::Colorize as _;

/// Horizontal and vertical breathing room around an inline code fill. The fill
/// is painted under the glyphs rather than reserved in layout, so the padding
/// has to stay under the width of the space that separates code from prose.
const CODE_FILL_PAD_X: f32 = 1.0;
const CODE_FILL_PAD_Y: f32 = 1.0;
/// `widget-corner-radius` is sized for panels and buttons and goes as high as
/// 24px; on a fill barely taller than the glyphs anything near half the height
/// rounds into a stadium. Square themes still come out square, since this only
/// ever lowers the radius.
const CODE_FILL_RADIUS_MAX: f32 = 6.0;

fn selectable_line_bounds(
    lines: &[Arc<WrappedLineLayout>],
    origin: Point<Pixels>,
    line_height: Pixels,
    mask_bounds: Bounds<Pixels>,
) -> Vec<Bounds<Pixels>> {
    let mut line_bounds = Vec::new();
    let mut top = origin.y;
    for (index, line) in lines.iter().enumerate() {
        let layout = &line.unwrapped_layout;
        let newline_width = if index + 1 < lines.len() {
            line_height.half()
        } else {
            px(0.0)
        };
        let rows = line.wrap_boundaries.len() + 1;
        let mut row_start = 0;
        for (row, row_end) in line
            .wrap_boundaries
            .iter()
            .map(|boundary| layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index)
            .chain([line.len()])
            .enumerate()
        {
            let mut width = layout.x_for_index(row_end) - layout.x_for_index(row_start);
            if row + 1 == rows {
                width += newline_width;
            }
            let bounds = Bounds::new(point(origin.x, top), gpui::size(width, line_height))
                .intersect(&mask_bounds);
            if bounds.size.width > px(0.) && bounds.size.height > px(0.) {
                line_bounds.push(bounds);
            }
            top += line_height;
            row_start = row_end;
        }
    }
    line_bounds
}

fn code_fill_bounds(
    text: &str,
    range: Range<usize>,
    lines: &[Arc<WrappedLineLayout>],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    align: TextAlign,
) -> Vec<Bounds<Pixels>> {
    let mut fills = Vec::new();
    let mut offset = 0;
    let mut top = bounds.origin.y;
    for line in lines {
        let layout = &line.unwrapped_layout;
        let ink = layout.ascent + layout.descent;
        let top_inset = ((line_height - ink) / 2.0).max(px(0.0)) - px(CODE_FILL_PAD_Y);
        let height = ink + px(CODE_FILL_PAD_Y * 2.0);
        let mut row_start = 0;
        for row_end in line
            .wrap_boundaries
            .iter()
            .map(|boundary| layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index)
            .chain([line.len()])
        {
            let start = range.start.max(offset + row_start);
            let mut end = range.end.min(offset + row_end);
            if start < end {
                if end == offset + row_end && row_end < line.len() {
                    end = start + text[start..end].trim_end_matches(char::is_whitespace).len();
                }
                if start < end {
                    let row_x = layout.x_for_index(row_start);
                    let row_width = layout.x_for_index(row_end) - row_x;
                    let align_offset = match align {
                        TextAlign::Left => px(0.0),
                        TextAlign::Center => (bounds.size.width - row_width) / 2.0,
                        TextAlign::Right => bounds.size.width - row_width,
                    };
                    let left =
                        bounds.left() + align_offset + layout.x_for_index(start - offset) - row_x;
                    let right =
                        bounds.left() + align_offset + layout.x_for_index(end - offset) - row_x;
                    if right > left {
                        fills.push(Bounds::from_corners(
                            point(left - px(CODE_FILL_PAD_X), top + top_inset),
                            point(right + px(CODE_FILL_PAD_X), top + top_inset + height),
                        ));
                    }
                }
            }
            row_start = row_end;
            top += line_height;
        }
        offset += line.len() + 1;
    }
    fills
}

pub(super) struct Inline {
    id: ElementId,
    text: SharedString,
    links: Rc<Vec<(Range<usize>, LinkMark)>>,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    code_ranges: Vec<Range<usize>>,
    styled_text: StyledText,
    streaming_veil: Option<(StreamingVeil, VeilKey)>,

    state: Arc<Mutex<InlineState>>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct InlineState {
    hovered_index: Option<usize>,
    pub(super) text: SharedString,
    pub(super) selection: Option<Selection>,
}

impl InlineState {
    pub(crate) fn set_text(&mut self, text: SharedString) {
        self.text = text;
    }

    fn set_hovered_index(&mut self, hovered_index: Option<usize>) -> bool {
        let changed = self.hovered_index != hovered_index;
        self.hovered_index = hovered_index;
        changed
    }
}

impl Inline {
    pub(super) fn new(
        id: impl Into<ElementId>,
        state: Arc<Mutex<InlineState>>,
        links: Vec<(Range<usize>, LinkMark)>,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
    ) -> Self {
        let text = state
            .lock()
            .map(|state| state.text.clone())
            .unwrap_or_default();

        Self {
            id: id.into(),
            links: Rc::new(links),
            highlights,
            code_ranges: Vec::new(),
            text: text.clone(),
            styled_text: StyledText::new(text),
            streaming_veil: None,
            state,
        }
    }

    /// Backtick spans, which take the mono family and a rounded fill. They stay
    /// part of this one shaped run: an element per span would align by box and
    /// drop the spans off the shared baseline.
    pub(super) fn code_ranges(mut self, code_ranges: Vec<Range<usize>>) -> Self {
        self.code_ranges = code_ranges;
        self
    }

    pub(super) fn streaming_veil(
        mut self,
        streaming_veil: Option<StreamingVeil>,
        key: VeilKey,
    ) -> Self {
        self.streaming_veil = streaming_veil.map(|veil| (veil, key));
        self
    }

    /// Painted before the glyphs so the fill lands under them. gpui's own run
    /// background is a bare rect; this one is rounded and hugs the font's ink
    /// box instead of the taller line box.
    fn paint_code_fills(&self, layout: &TextLayout, window: &mut Window, cx: &mut App) {
        if self.code_ranges.is_empty() {
            return;
        }
        let fill_color = cx.theme().background.raised(2);
        let radius = cx.theme().radius.min(px(CODE_FILL_RADIUS_MAX));
        let line_height = layout.line_height();
        let bounds = layout.bounds();
        let lines = layout.line_layouts();

        for range in &self.code_ranges {
            for fill in code_fill_bounds(
                &self.text,
                range.clone(),
                &lines,
                bounds,
                line_height,
                window.text_style().text_align,
            ) {
                window.paint_quad(gpui::fill(fill, fill_color).corner_radii(radius));
            }
        }
    }

    fn link_for_position(
        layout: &TextLayout,
        links: &Vec<(Range<usize>, LinkMark)>,
        position: Point<Pixels>,
    ) -> Option<LinkMark> {
        let offset = layout.index_for_position(position).ok()?;
        for (range, link) in links.iter() {
            if range.contains(&offset) {
                return Some(link.clone());
            }
        }

        None
    }

    fn layout_selections(
        &self,
        text_layout: &TextLayout,
        bounds: &Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> (bool, bool, Option<Selection>) {
        let Some(text_view_state) = TextGlobal::current_view(cx) else {
            return (false, false, None);
        };

        let text_view_state = text_view_state.read(cx);
        let is_selectable = text_view_state.is_selectable();
        if !is_selectable {
            return (false, false, None);
        }

        if text_view_state.is_all_selected() {
            return (is_selectable, true, Some((0..self.text.len()).into()));
        }

        if let Some(selection) = text_view_state.multi_click_selection() {
            return (
                is_selectable,
                true,
                selection_for_multi_click(
                    &self.text,
                    text_layout,
                    *bounds,
                    selection.pos,
                    selection.kind,
                )
                .map(Selection::from),
            );
        }

        let Some((selection_start, selection_end)) = text_view_state.selection_points(window, cx)
        else {
            return (is_selectable, false, None);
        };
        let line_height = window.line_height();
        let mask_bounds = window.content_mask().bounds;

        let mut selection: Option<Selection> = None;
        let mut offset = 0;
        let mut chars = self.text.chars().peekable();
        while let Some(c) = chars.next() {
            let Some(pos) = text_layout.position_for_index(offset) else {
                offset += c.len_utf8();
                continue;
            };

            let next_offset = offset + c.len_utf8();
            let mut char_width = line_height.half();
            if let Some(next_pos) = text_layout.position_for_index(next_offset) {
                if next_pos.y == pos.y {
                    char_width = next_pos.x - pos.x;
                }
            }

            let char_center = point(pos.x + char_width.half(), pos.y + line_height.half());
            if mask_bounds.contains(&char_center)
                && point_in_text_selection(
                    pos,
                    char_width,
                    selection_start,
                    selection_end,
                    line_height,
                )
            {
                if selection.is_none() {
                    selection = Some((offset..offset).into());
                }

                if let Some(selection) = selection.as_mut() {
                    selection.end = next_offset;
                }
            }

            offset = next_offset;
        }

        (true, true, selection)
    }

    fn paint_selection(
        selection: &Selection,
        text_layout: &TextLayout,
        bounds: &Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut start = selection.start;
        let mut end = selection.end;
        if end < start {
            std::mem::swap(&mut start, &mut end);
        }
        let Some(start_position) = text_layout.position_for_index(start) else {
            return;
        };
        let Some(end_position) = text_layout.position_for_index(end) else {
            return;
        };

        let line_height = text_layout.line_height();
        if start_position.y == end_position.y {
            window.paint_quad(quad(
                Bounds::from_corners(
                    start_position,
                    point(end_position.x, end_position.y + line_height),
                ),
                px(0.),
                cx.theme().foreground.wash(),
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        } else {
            window.paint_quad(quad(
                Bounds::from_corners(
                    start_position,
                    point(bounds.right(), start_position.y + line_height),
                ),
                px(0.),
                cx.theme().foreground.wash(),
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));

            if end_position.y > start_position.y + line_height {
                window.paint_quad(quad(
                    Bounds::from_corners(
                        point(bounds.left(), start_position.y + line_height),
                        point(bounds.right(), end_position.y),
                    ),
                    px(0.),
                    cx.theme().foreground.wash(),
                    Edges::default(),
                    gpui::transparent_black(),
                    BorderStyle::default(),
                ));
            }

            window.paint_quad(quad(
                Bounds::from_corners(
                    point(bounds.left(), end_position.y),
                    point(end_position.x, end_position.y + line_height),
                ),
                px(0.),
                cx.theme().foreground.wash(),
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        }
    }
}

impl IntoElement for Inline {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Inline {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_element_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let text_style = window.text_style();

        let mut runs = Vec::new();
        let mut ix = 0;
        for (range, highlight) in self.highlights.iter() {
            if ix < range.start {
                runs.push(text_style.clone().to_run(range.start - ix));
            }
            runs.push(text_style.clone().highlight(*highlight).to_run(range.len()));
            ix = range.end;
        }
        if ix < self.text.len() {
            runs.push(text_style.to_run(self.text.len() - ix));
        }
        if !self.code_ranges.is_empty() {
            let family = cx.theme().mono_font_family.clone();
            let mut start = 0;
            for run in &mut runs {
                let end = start + run.len;
                if self
                    .code_ranges
                    .iter()
                    .any(|range| range.start <= start && end <= range.end)
                {
                    run.font.family = family.clone();
                }
                start = end;
            }
        }
        if let Some((veil, key)) = &self.streaming_veil {
            runs = veil.runs(*key, self.text.as_ref(), runs, cx);
        }

        self.styled_text = StyledText::new(self.text.clone()).with_runs(runs);
        let (layout_id, _) =
            self.styled_text
                .request_layout(global_element_id, inspector_id, window, cx);

        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current_view = window.current_view();
        let hitbox = prepaint;
        let Ok(mut state) = self.state.lock() else {
            return;
        };

        let text_layout = self.styled_text.layout().clone();
        self.paint_code_fills(&text_layout, window, cx);
        self.styled_text
            .paint(global_id, None, bounds, &mut (), &mut (), window, cx);

        let (is_selectable, is_selection, selection) =
            self.layout_selections(&text_layout, &bounds, window, cx);

        state.selection = selection;

        if is_selection || is_selectable {
            window.set_cursor_style(CursorStyle::IBeam, &hitbox);
        }

        let mouse_position = window.mouse_position();
        if let Some(_) = Self::link_for_position(&text_layout, &self.links, mouse_position) {
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
        }

        if let Some(selection) = &state.selection {
            Self::paint_selection(selection, &text_layout, &bounds, window, cx);
        }

        if is_selectable {
            if let Some(text_view_state) = TextGlobal::current_view(cx).cloned() {
                let text_bounds = selectable_line_bounds(
                    &text_layout.line_layouts(),
                    text_layout.bounds().origin,
                    text_layout.line_height(),
                    window.content_mask().bounds,
                );
                window_selection::register_selectable_text_inline(
                    &text_view_state,
                    text_bounds,
                    window,
                    cx,
                );
            }

            window.on_mouse_event({
                let hitbox = hitbox.clone();
                let text_layout = text_layout.clone();
                let inline_state = self.state.clone();
                let text = self.text.clone();
                let text_view_state = TextGlobal::current_view(cx).cloned();

                move |event: &MouseDownEvent, phase, window, cx| {
                    if !phase.bubble()
                        || !hitbox.is_hovered(window)
                        || event.button != MouseButton::Left
                    {
                        return;
                    }

                    let kind = match event.click_count {
                        2 => TextViewMultiClickKind::Word,
                        3 => TextViewMultiClickKind::Paragraph,
                        _ => return,
                    };

                    let Some(range) = selection_for_multi_click(
                        &text,
                        &text_layout,
                        hitbox.bounds,
                        event.position,
                        kind,
                    ) else {
                        return;
                    };

                    let selected_text = text[range.clone()].to_string();

                    if let Ok(mut inline_state) = inline_state.lock() {
                        inline_state.selection = Some(range.into());
                    }
                    if let Some(text_view_state) = &text_view_state {
                        text_view_state.update(cx, |state, _| {
                            state.set_multi_click_selection(event.position, kind, selected_text);
                        });
                    }
                    cx.notify(current_view);
                }
            });
        }

        window.on_mouse_event({
            let hitbox = hitbox.clone();
            let text_layout = text_layout.clone();
            let inline_state = self.state.clone();
            let mut hovered_index = state.hovered_index;
            move |event: &MouseMoveEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }

                let updated = hitbox
                    .is_hovered(window)
                    .then(|| text_layout.index_for_position(event.position).ok())
                    .flatten();
                if hovered_index == updated {
                    return;
                }

                hovered_index = updated;
                if let Ok(mut state) = inline_state.lock()
                    && state.set_hovered_index(updated)
                {
                    cx.notify(current_view);
                }
            }
        });

        if !is_selection {
            window.on_mouse_event({
                let links = self.links.clone();
                let text_layout = text_layout.clone();
                let hitbox = hitbox.clone();
                let text_view_state = TextGlobal::current_view(cx).cloned();

                move |event: &MouseUpEvent, phase, window, cx| {
                    if !phase.bubble() || !hitbox.is_hovered(window) {
                        return;
                    }
                    if text_view_state
                        .as_ref()
                        .is_some_and(|state| state.read(cx).has_selection(window, cx))
                    {
                        return;
                    }

                    if let Some(link) =
                        Self::link_for_position(&text_layout, &links, event.position)
                    {
                        window.end_text_selection(cx);
                        cx.stop_propagation();
                        if is_openable(&link.url) {
                            cx.open_url(&link.url);
                        }
                    }
                }
            });
        }
    }
}

fn is_openable(url: &str) -> bool {
    let scheme = url.split_once(':').map_or("", |(scheme, _)| scheme);
    !(scheme.eq_ignore_ascii_case("data") || scheme.eq_ignore_ascii_case("javascript"))
}

fn selection_for_multi_click(
    text: &str,
    text_layout: &TextLayout,
    bounds: Bounds<Pixels>,
    pos: Point<Pixels>,
    kind: TextViewMultiClickKind,
) -> Option<std::ops::Range<usize>> {
    if !bounds.contains(&pos) {
        return None;
    }

    let offset = text_layout.index_for_position(pos).ok()?;

    match kind {
        TextViewMultiClickKind::Word => word_range_at(text, offset),
        TextViewMultiClickKind::Paragraph => (!text.is_empty()).then_some(0..text.len()),
    }
}

fn point_in_text_selection(
    pos: Point<Pixels>,
    char_width: Pixels,
    selection_start: Point<Pixels>,
    selection_end: Point<Pixels>,
    line_height: Pixels,
) -> bool {
    let point_in_line = |point: Point<Pixels>| point.y >= pos.y && point.y < pos.y + line_height;
    let top = selection_start.y.min(selection_end.y);
    let bottom = selection_start.y.max(selection_end.y);
    let x = pos.x + char_width.half();

    if pos.y + line_height <= top || pos.y > bottom {
        return false;
    }

    if point_in_line(selection_start) && point_in_line(selection_end) {
        let left = selection_start.x.min(selection_end.x);
        let right = selection_start.x.max(selection_end.x);
        return x >= left && x <= right;
    }

    let (top_point, bottom_point) = if selection_start.y < selection_end.y {
        (selection_start, selection_end)
    } else {
        (selection_end, selection_start)
    };
    let is_top_line = point_in_line(top_point);
    let is_bottom_line = point_in_line(bottom_point);

    if is_top_line {
        return x >= top_point.x;
    } else if is_bottom_line {
        return x <= bottom_point.x;
    } else {
        return true;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Bounds, CODE_FILL_PAD_X, CODE_FILL_PAD_Y, InlineState, TextAlign, code_fill_bounds,
        is_openable, point_in_text_selection, selectable_line_bounds,
    };
    use gpui::{
        FontId, GlyphId, LineLayout, ShapedGlyph, ShapedRun, WrapBoundary, WrappedLineLayout,
        point, px,
    };
    use std::sync::Arc;

    fn code_line(text: &str, wraps: &[usize]) -> Arc<WrappedLineLayout> {
        let glyphs = text
            .char_indices()
            .enumerate()
            .map(|(column, (index, _))| ShapedGlyph {
                id: GlyphId(0),
                position: point(px(column as f32 * 10.0), px(0.0)),
                index,
                is_emoji: false,
            })
            .collect::<Vec<_>>();
        let boundaries = wraps
            .iter()
            .map(|index| WrapBoundary {
                run_ix: 0,
                glyph_ix: glyphs
                    .iter()
                    .position(|glyph| glyph.index == *index)
                    .unwrap(),
            })
            .collect();
        Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                font_size: px(13.0),
                width: px(glyphs.len() as f32 * 10.0),
                ascent: px(12.0),
                descent: px(3.31),
                runs: vec![ShapedRun {
                    font_id: FontId(0),
                    glyphs,
                }],
                len: text.len(),
            }),
            wrap_boundaries: boundaries,
            wrap_width: Some(px(400.0)),
        })
    }

    #[test]
    fn selectable_bounds_cover_each_visual_row_once() {
        let first = "wrapped first line";
        let lines = [
            code_line(first, &[8]),
            code_line("", &[]),
            code_line("tail", &[]),
        ];
        let mask = Bounds::from_corners(point(px(0.0), px(0.0)), point(px(400.0), px(400.0)));
        let bounds = selectable_line_bounds(&lines, point(px(5.0), px(10.0)), px(20.0), mask);
        let rows = bounds
            .iter()
            .map(|row| (row.left(), row.top(), row.size.width))
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [
                (px(5.0), px(10.0), px(80.0)),
                (px(5.0), px(30.0), px(100.0 + 10.0)),
                (px(5.0), px(50.0), px(10.0)),
                (px(5.0), px(70.0), px(40.0)),
            ]
        );
        let clipped = Bounds::from_corners(point(px(0.0), px(25.0)), point(px(50.0), px(60.0)));
        let bounds = selectable_line_bounds(&lines, point(px(5.0), px(10.0)), px(20.0), clipped);
        assert_eq!(bounds.len(), 3);
        assert!(bounds.iter().all(|row| row.right() <= px(50.0)
            && row.top() >= px(25.0)
            && row.bottom() <= px(60.0)));
    }

    #[test]
    fn a_code_fill_hugs_the_ink_box_on_one_line() {
        let text = "before code after";
        let fills = code_fill_bounds(
            text,
            7..11,
            &[code_line(text, &[])],
            Bounds::from_corners(point(px(5.0), px(0.0)), point(px(405.0), px(100.0))),
            px(21.0),
            TextAlign::Left,
        );
        assert_eq!(fills.len(), 1);
        let fill = fills[0];
        assert_eq!(fill.left(), px(75.0 - CODE_FILL_PAD_X));
        assert_eq!(fill.right(), px(115.0 + CODE_FILL_PAD_X));
        assert!((fill.size.height - px(15.31 + CODE_FILL_PAD_Y * 2.0)).abs() < px(0.01));
        assert!((fill.top() + fill.size.height / 2.0 - px(10.5)).abs() < px(0.01));
        assert!(fill.left() > px(65.0));
        assert!(fill.right() < px(125.0));
    }

    #[test]
    fn a_wrapped_code_fill_stops_at_the_fragment_not_the_paragraph_edge() {
        let text = "before cargo run -p zz-xtask after";
        let fills = code_fill_bounds(
            text,
            7..28,
            &[code_line(text, &[20])],
            Bounds::from_corners(point(px(5.0), px(0.0)), point(px(405.0), px(100.0))),
            px(21.0),
            TextAlign::Left,
        );
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].right(), px(195.0 + CODE_FILL_PAD_X));
        assert_eq!(fills[1].left(), px(5.0 - CODE_FILL_PAD_X));
        assert_eq!(fills[1].right(), px(85.0 + CODE_FILL_PAD_X));
        assert_eq!(fills[1].top() - fills[0].top(), px(21.0));
    }

    #[test]
    fn code_at_a_wrap_boundary_has_no_fill_on_the_previous_row() {
        let text = "before café after";
        let lines = [code_line(text, &[7, 13])];
        let fills = code_fill_bounds(
            text,
            7..12,
            &lines,
            Bounds::from_corners(point(px(0.0), px(0.0)), point(px(400.0), px(100.0))),
            px(21.0),
            TextAlign::Left,
        );
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].left(), px(-CODE_FILL_PAD_X));
        assert_eq!(fills[0].right(), px(40.0 + CODE_FILL_PAD_X));
        assert!(fills[0].top() >= px(21.0));
        assert!(fills[0].bottom() <= px(42.0));
    }

    #[test]
    fn code_fills_follow_centered_and_right_aligned_table_text() {
        let text = "code";
        for (align, left) in [(TextAlign::Center, 30.0), (TextAlign::Right, 60.0)] {
            let fills = code_fill_bounds(
                text,
                0..text.len(),
                &[code_line(text, &[])],
                Bounds::from_corners(point(px(0.0), px(0.0)), point(px(100.0), px(21.0))),
                px(21.0),
                align,
            );
            assert_eq!(fills[0].left(), px(left - CODE_FILL_PAD_X));
            assert_eq!(fills[0].right(), px(left + 40.0 + CODE_FILL_PAD_X));
        }
    }

    #[gpui::test]
    fn shaped_code_wrap_leaves_the_unused_line_width_unpainted(cx: &mut gpui::TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, _| {
            let text = gpui::SharedString::from("cargo run -p zz-xtask");
            let run = gpui::TextStyle::default().to_run(text.len());
            let wrapped = window
                .text_system()
                .shape_text(text.clone(), px(13.0), &[run], Some(px(113.0)), None)
                .expect("shape code");
            let lines = wrapped
                .iter()
                .map(|line| Arc::clone(line))
                .collect::<Vec<_>>();
            let fills = code_fill_bounds(
                &text,
                0..text.len(),
                &lines,
                Bounds::from_corners(point(px(0.0), px(0.0)), point(px(113.0), px(100.0))),
                px(21.0),
                TextAlign::Left,
            );
            assert!(fills.len() >= 2);
            assert!(fills[0].right() < px(113.0));
        });
    }

    #[test]
    fn hovered_index_only_changes_on_glyph_transition() {
        let mut state = InlineState::default();

        assert!(state.set_hovered_index(Some(4)));
        assert!(!state.set_hovered_index(Some(4)));
        assert!(state.set_hovered_index(Some(5)));
        assert!(state.set_hovered_index(None));
        assert!(!state.set_hovered_index(None));
    }

    #[test]
    fn inline_payload_links_are_never_handed_to_the_os() {
        assert!(!is_openable("data:image/png;base64,iVBORw0KGgo="));
        assert!(!is_openable("DATA:image/png;base64,iVBORw0KGgo="));
        assert!(!is_openable("javascript:alert(1)"));

        assert!(is_openable("https://gpui.rs"));
        assert!(is_openable("file:///home/u/project"));
        assert!(is_openable("mailto:user@example.com"));
        assert!(is_openable("zed://file/main.rs"));
        assert!(is_openable("./relative/path.md"));
    }

    #[test]
    fn test_point_in_text_selection() {
        let line_height = px(20.);
        let char_width = px(10.);
        let start = point(px(50.), px(50.));
        let end = point(px(150.), px(150.));

        assert!(point_in_text_selection(
            point(px(50.), px(40.)),
            char_width,
            start,
            end,
            line_height
        ));

        assert!(point_in_text_selection(
            point(px(50.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(40.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(160.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));

        assert!(point_in_text_selection(
            point(px(100.), px(70.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(40.), px(70.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(160.), px(70.)),
            char_width,
            start,
            end,
            line_height
        ));

        assert!(point_in_text_selection(
            point(px(100.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(40.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(160.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));

        assert!(!point_in_text_selection(
            point(px(100.), px(20.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(100.), px(160.)),
            char_width,
            start,
            end,
            line_height
        ));
    }

    #[test]
    fn test_point_in_text_selection_reversed_drag_direction() {
        let line_height = px(20.);
        let char_width = px(10.);

        let start = point(px(80.), px(150.));
        let end = point(px(150.), px(50.));

        assert!(!point_in_text_selection(
            point(px(140.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(150.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));

        assert!(point_in_text_selection(
            point(px(75.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(80.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));
    }

    #[test]
    fn test_point_in_text_selection_same_visual_line_with_different_y() {
        let line_height = px(20.);
        let char_width = px(10.);
        let start = point(px(100.), px(55.));
        let end = point(px(60.), px(58.));

        assert!(!point_in_text_selection(
            point(px(40.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(70.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(110.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
    }

    #[test]
    fn test_point_in_text_selection_same_visual_line_with_reversed_y() {
        let line_height = px(20.);
        let char_width = px(10.);
        let start = point(px(60.), px(58.));
        let end = point(px(100.), px(55.));

        assert!(!point_in_text_selection(
            point(px(40.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(70.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(110.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
    }
}
