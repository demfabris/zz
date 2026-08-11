//! The element that turns [`InputState`] into pixels.

use std::{ops::Range, rc::Rc};

use gpui::{
    App, Bounds, ContentMask, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, SharedString, Size, Style, TextAlign, TextRun, Window,
    WrappedLine, fill, point, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::ActiveTheme as _;

use super::state::{CURSOR_WIDTH, InputMode, InputState, NEWLINE_SELECTION_WIDTH};
use crate::Colorize as _;

const MASK_CHAR: char = '•';

#[derive(Clone)]
struct Mask {
    boundaries: Rc<Vec<usize>>,
}

impl Mask {
    fn new(value: &str) -> Self {
        let mut boundaries: Vec<usize> = value.grapheme_indices(true).map(|(at, _)| at).collect();
        boundaries.push(value.len());
        Self {
            boundaries: Rc::new(boundaries),
        }
    }

    fn bullets(&self) -> usize {
        self.boundaries.len() - 1
    }

    fn display_text(&self) -> SharedString {
        MASK_CHAR.to_string().repeat(self.bullets()).into()
    }

    fn to_display(&self, value_offset: usize) -> usize {
        let bullet = self
            .boundaries
            .partition_point(|&at| at <= value_offset)
            .saturating_sub(1);
        bullet * MASK_CHAR.len_utf8()
    }

    fn to_value(&self, display_offset: usize) -> usize {
        let bullet = (display_offset / MASK_CHAR.len_utf8()).min(self.bullets());
        self.boundaries[bullet]
    }
}

#[derive(Clone)]
pub(super) struct LastLayout {
    lines: Rc<Vec<WrappedLine>>,
    line_starts: Rc<Vec<usize>>,
    line_tops: Rc<Vec<Pixels>>,
    line_lefts: Rc<Vec<Pixels>>,
    placeholder: bool,
    mask: Option<Mask>,

    pub(super) line_height: Pixels,
    pub(super) origin: Point<Pixels>,
    pub(super) bounds: Bounds<Pixels>,
    pub(super) content_size: Size<Pixels>,
}

fn line_left(align: TextAlign, viewport_width: Pixels, line_width: Pixels) -> Pixels {
    let slack = (viewport_width - line_width).max(Pixels::ZERO);
    match align {
        TextAlign::Left => Pixels::ZERO,
        TextAlign::Center => slack / 2.,
        TextAlign::Right => slack,
    }
}

fn row_starts(line: &WrappedLine) -> Vec<usize> {
    let mut starts = Vec::with_capacity(line.wrap_boundaries().len() + 1);
    starts.push(0);
    for boundary in line.wrap_boundaries() {
        let glyph = &line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix];
        starts.push(glyph.index);
    }
    starts
}

impl LastLayout {
    fn rows(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.wrap_boundaries().len() + 1)
            .sum::<usize>()
            .max(1)
    }

    fn local_offset(&self, index: usize, offset: usize) -> usize {
        let local = match &self.mask {
            Some(mask) => mask.to_display(offset),
            None => offset.saturating_sub(self.line_starts[index]),
        };
        local.min(self.lines[index].len())
    }

    fn value_offset(&self, index: usize, local: usize) -> usize {
        match &self.mask {
            Some(mask) => mask.to_value(local),
            None => self.line_starts[index] + local,
        }
    }

    pub(super) fn offset_for_position(&self, position: Point<Pixels>) -> usize {
        self.offset_for_local_position(position - self.origin)
    }

    pub(super) fn offset_for_local_position(&self, local: Point<Pixels>) -> usize {
        if self.placeholder || self.lines.is_empty() {
            return 0;
        }

        let mut index = self.lines.len() - 1;
        for (i, top) in self.line_tops.iter().enumerate() {
            let height = self.lines[i].size(self.line_height).height;
            if local.y < *top + height {
                index = i;
                break;
            }
        }

        let line = &self.lines[index];
        let height = line.size(self.line_height).height;
        let ceiling = (height - px(1.)).max(Pixels::ZERO);
        let inner = point(
            local.x - self.line_lefts[index],
            (local.y - self.line_tops[index]).clamp(Pixels::ZERO, ceiling),
        );

        let offset = line
            .closest_index_for_position(inner, self.line_height)
            .unwrap_or_else(|offset| offset);
        self.value_offset(index, offset.min(line.len()))
    }

    pub(super) fn position_for_offset(&self, offset: usize) -> Point<Pixels> {
        if self.placeholder || self.lines.is_empty() {
            return Point::default();
        }

        let index = self.line_index_for_offset(offset);
        let line = &self.lines[index];
        let local = self.local_offset(index, offset);
        let position = line
            .position_for_index(local, self.line_height)
            .unwrap_or_default();
        point(
            position.x + self.line_lefts[index],
            position.y + self.line_tops[index],
        )
    }

    fn line_index_for_offset(&self, offset: usize) -> usize {
        if self.mask.is_some() {
            return 0;
        }
        for (index, start) in self.line_starts.iter().enumerate() {
            if offset <= start + self.lines[index].len() {
                return index;
            }
        }
        self.lines.len() - 1
    }

    fn selection_quads(&self, selection: &Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut quads = Vec::new();
        if selection.is_empty() || self.placeholder {
            return quads;
        }

        let selection = match &self.mask {
            Some(mask) => mask.to_display(selection.start)..mask.to_display(selection.end),
            None => selection.clone(),
        };

        for (index, line) in self.lines.iter().enumerate() {
            let line_start = self.line_starts[index];
            let line_end = line_start + line.len();
            if selection.end < line_start || selection.start > line_end {
                continue;
            }

            let from_line = selection.start.saturating_sub(line_start).min(line.len());
            let to_line = selection.end.saturating_sub(line_start).min(line.len());
            let covers_break = selection.end > line_end;

            let starts = row_starts(line);
            for (row, &row_start) in starts.iter().enumerate() {
                let row_end = starts.get(row + 1).copied().unwrap_or(line.len());
                let last_row = row + 1 == starts.len();
                let from = from_line.max(row_start);
                let to = to_line.min(row_end);
                let trailing = if last_row && covers_break {
                    NEWLINE_SELECTION_WIDTH
                } else {
                    Pixels::ZERO
                };

                if from > to || (from == to && trailing == Pixels::ZERO) {
                    continue;
                }

                let x_of = |offset: usize| {
                    if offset == row_start {
                        Pixels::ZERO
                    } else {
                        line.position_for_index(offset, self.line_height)
                            .unwrap_or_default()
                            .x
                    }
                };

                let left = x_of(from);
                let right = x_of(to);
                quads.push(Bounds {
                    origin: point(
                        left + self.line_lefts[index],
                        self.line_tops[index] + self.line_height * row as f32,
                    ),
                    size: size((right - left) + trailing, self.line_height),
                });
            }
        }

        quads
    }
}

pub(super) struct TextElement {
    state: Entity<InputState>,
}

impl TextElement {
    pub(super) const fn new(state: Entity<InputState>) -> Self {
        Self { state }
    }
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub(super) struct PrepaintState {
    layout: LastLayout,
    selection_quads: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let line_height = window.line_height();
        let state = self.state.read(cx);

        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = match state.mode() {
            InputMode::SingleLine => line_height.into(),
            InputMode::AutoGrow { min_rows, max_rows } => {
                let rows = state.measured_rows.clamp(min_rows, max_rows.max(min_rows));
                (line_height * rows as f32).into()
            }
        };

        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        let state = self.state.read(cx);
        let mode = state.mode();
        let align = state.align;
        let disabled = state.disabled;
        let selection = state.selection();
        let cursor = state.cursor();
        let show_caret = state.show_cursor(window, cx);
        let value = state.text().clone();
        let placeholder = state.placeholder_text().clone();
        let previous_scroll = state.scroll;
        let follow_cursor = state.follow_cursor;
        let reset_scroll = state.reset_scroll;

        let is_placeholder = value.is_empty();
        let mask =
            (state.masked && !mode.is_multi_line() && !is_placeholder).then(|| Mask::new(&value));
        let display_text: SharedString = match (&mask, is_placeholder) {
            (Some(mask), _) => mask.display_text(),
            (None, true) => placeholder,
            (None, false) => value,
        };

        let color = if is_placeholder {
            cx.theme().foreground.muted()
        } else {
            text_style.color
        };
        let color = if disabled { color.opacity(0.5) } else { color };
        let run = TextRun {
            color,
            ..text_style.to_run(display_text.len())
        };

        let wrap_width = mode.is_multi_line().then_some(bounds.size.width);
        let lines: Vec<WrappedLine> = window
            .text_system()
            .shape_text(display_text, font_size, &[run], wrap_width, None)
            .unwrap_or_default()
            .into_iter()
            .collect();

        let mut line_starts = Vec::with_capacity(lines.len());
        let mut line_tops = Vec::with_capacity(lines.len());
        let mut line_lefts = Vec::with_capacity(lines.len());
        let mut offset = 0usize;
        let mut y = Pixels::ZERO;
        let mut width = Pixels::ZERO;
        for line in &lines {
            line_starts.push(offset);
            line_tops.push(y);
            line_lefts.push(line_left(align, bounds.size.width, line.width()));
            offset += line.len() + 1;
            y += line_height * (line.wrap_boundaries().len() + 1) as f32;
            width = width.max(line.width());
        }

        let mut layout = LastLayout {
            lines: Rc::new(lines),
            line_starts: Rc::new(line_starts),
            line_tops: Rc::new(line_tops),
            line_lefts: Rc::new(line_lefts),
            placeholder: is_placeholder,
            mask,
            line_height,
            origin: bounds.origin,
            bounds,
            content_size: size(width, y),
        };

        let caret_position = layout.position_for_offset(cursor);
        let scroll = Self::scroll_offset(
            &layout,
            mode.is_multi_line(),
            caret_position,
            previous_scroll,
            follow_cursor,
            reset_scroll,
        );
        layout.origin = bounds.origin + scroll;

        let rows = if is_placeholder { 1 } else { layout.rows() };
        let selection_quads = layout
            .selection_quads(&selection)
            .into_iter()
            .map(|quad| Bounds {
                origin: layout.origin + quad.origin,
                size: quad.size,
            })
            .collect();

        let caret = show_caret.then(|| Bounds {
            origin: layout.origin + caret_position,
            size: size(CURSOR_WIDTH, line_height),
        });

        self.state.update(cx, |state, cx| {
            state.last_layout = Some(layout.clone());
            state.scroll = scroll;
            state.follow_cursor = false;
            state.reset_scroll = false;
            if state.measured_rows != rows {
                state.measured_rows = rows;
                cx.notify();
            }
        });

        PrepaintState {
            layout,
            selection_quads,
            caret,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = &prepaint.layout;
        let focus_handle = self.state.read(cx).focus_handle_ref().clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(layout.bounds, self.state.clone()),
            cx,
        );

        let selection_color = cx.theme().foreground.wash();
        let caret_color = cx.theme().foreground;

        window.with_content_mask(
            Some(ContentMask {
                bounds: layout.bounds,
            }),
            |window| {
                for quad in &prepaint.selection_quads {
                    window.paint_quad(fill(*quad, selection_color));
                }

                for ((line, top), left) in layout
                    .lines
                    .iter()
                    .zip(layout.line_tops.iter())
                    .zip(layout.line_lefts.iter())
                {
                    let origin = layout.origin + point(*left, *top);
                    _ = line.paint(
                        origin,
                        layout.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }

                if let Some(caret) = prepaint.caret {
                    window.paint_quad(fill(caret, caret_color));
                }
            },
        );

        let state = self.state.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.pressed_button == Some(MouseButton::Left) {
                state.update(cx, |state, cx| state.on_drag(event.position, cx));
            }
        });

        let state = self.state.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                state.update(cx, |state, _| state.end_drag());
            }
        });
    }
}

impl TextElement {
    fn scroll_offset(
        layout: &LastLayout,
        multi_line: bool,
        caret: Point<Pixels>,
        previous: Point<Pixels>,
        follow_cursor: bool,
        reset: bool,
    ) -> Point<Pixels> {
        let viewport = layout.bounds.size;
        let mut scroll = if reset { Point::default() } else { previous };

        if follow_cursor && !reset {
            if multi_line {
                let top = caret.y;
                let bottom = top + layout.line_height;
                if top + scroll.y < Pixels::ZERO {
                    scroll.y = -top;
                } else if bottom + scroll.y > viewport.height {
                    scroll.y = viewport.height - bottom;
                }
            } else {
                let right_edge = viewport.width - CURSOR_WIDTH;
                if caret.x + scroll.x < Pixels::ZERO {
                    scroll.x = -caret.x;
                } else if caret.x + scroll.x > right_edge {
                    scroll.x = right_edge - caret.x;
                }
            }
        }

        if multi_line {
            scroll.x = Pixels::ZERO;
        } else {
            scroll.y = Pixels::ZERO;
        }

        let overflow_x =
            (layout.content_size.width + CURSOR_WIDTH - viewport.width).max(Pixels::ZERO);
        let overflow_y = (layout.content_size.height - viewport.height).max(Pixels::ZERO);
        scroll.x = scroll.x.clamp(-overflow_x, Pixels::ZERO);
        scroll.y = scroll.y.clamp(-overflow_y, Pixels::ZERO);
        scroll
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        AppContext as _, Context, Entity, Modifiers, ParentElement as _, Render, Styled as _,
        TestAppContext, VisualTestContext, div,
    };

    use super::super::{Input, InputContentType, InputState};
    use super::*;

    #[test]
    fn alignment_offsets_place_the_line_inside_the_viewport() {
        let viewport = px(100.);
        let line = px(40.);

        assert_eq!(line_left(TextAlign::Left, viewport, line), px(0.));
        assert_eq!(line_left(TextAlign::Center, viewport, line), px(30.));
        assert_eq!(line_left(TextAlign::Right, viewport, line), px(60.));
    }

    #[test]
    fn overflowing_text_collapses_to_the_left_edge_at_every_alignment() {
        let viewport = px(50.);
        let line = px(120.);

        for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
            assert_eq!(line_left(align, viewport, line), px(0.), "{align:?}");
        }
    }

    #[test]
    fn a_left_aligned_field_is_never_shifted() {
        for width in [px(0.), px(10.), px(320.)] {
            assert_eq!(line_left(TextAlign::Left, px(200.), width), px(0.));
        }
    }

    const FAMILY: &str = "👨‍👩‍👧‍👦";
    const BULLET: usize = 3;

    #[test]
    fn a_mask_is_one_bullet_per_grapheme_cluster() {
        for (value, bullets) in [
            ("", 0),
            ("abc", 3),
            ("caf\u{e9}", 4),
            ("cafe\u{301}", 4),
            ("a😀b", 3),
            (FAMILY, 1),
            ("🇧🇷", 1),
        ] {
            let mask = Mask::new(value);
            assert_eq!(mask.bullets(), bullets, "{value:?}");
            assert_eq!(
                mask.display_text().chars().count(),
                bullets,
                "{value:?} shapes one bullet per grapheme"
            );
            assert!(
                mask.display_text().chars().all(|c| c == MASK_CHAR),
                "{value:?} leaks a glyph of its own"
            );
        }
    }

    #[test]
    fn mask_offsets_round_trip_at_every_grapheme_boundary() {
        for value in ["", "abc", "cafe\u{301}", "a😀b", FAMILY, "🇧🇷x"] {
            let mask = Mask::new(value);
            let boundaries = value
                .grapheme_indices(true)
                .map(|(at, _)| at)
                .chain([value.len()]);
            for (bullet, at) in boundaries.enumerate() {
                assert_eq!(mask.to_display(at), bullet * BULLET, "{value:?} at {at}");
                assert_eq!(mask.to_value(bullet * BULLET), at, "{value:?} at {at}");
            }
        }
    }

    #[test]
    fn an_offset_inside_a_grapheme_resolves_to_the_start_of_its_bullet() {
        let mask = Mask::new("a😀b");
        assert_eq!(mask.to_display(0), 0);
        assert_eq!(mask.to_display(1), BULLET);
        for inside in 2..5 {
            assert_eq!(mask.to_display(inside), BULLET, "byte {inside}");
        }
        assert_eq!(mask.to_display(5), 2 * BULLET);
        assert_eq!(mask.to_display(6), 3 * BULLET);
    }

    #[test]
    fn mask_offsets_past_either_end_clamp_instead_of_panicking() {
        let mask = Mask::new("ab");
        assert_eq!(mask.to_display(99), 2 * BULLET);
        assert_eq!(mask.to_value(99), 2);

        let empty = Mask::new("");
        assert_eq!(empty.bullets(), 0);
        assert_eq!(empty.to_display(0), 0);
        assert_eq!(empty.to_value(0), 0);
        assert_eq!(empty.to_value(BULLET), 0);
    }

    const SECRET: Option<InputContentType> = Some(InputContentType::Password);
    const PLAIN: Option<InputContentType> = None;

    struct Field {
        state: Entity<InputState>,
        content_type: Option<InputContentType>,
    }

    impl Render for Field {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let input = Input::new(&self.state);
            let input = match self.content_type {
                Some(content_type) => input.content_type(content_type),
                None => input,
            };
            div().w(px(300.)).child(input)
        }
    }

    fn field<'a>(
        value: &str,
        content_type: Option<InputContentType>,
        cx: &'a mut TestAppContext,
    ) -> (Entity<InputState>, &'a mut VisualTestContext) {
        field_with(value, content_type, false, cx)
    }

    fn field_with<'a>(
        value: &str,
        content_type: Option<InputContentType>,
        multi_line: bool,
        cx: &'a mut TestAppContext,
    ) -> (Entity<InputState>, &'a mut VisualTestContext) {
        cx.update(crate::init);
        let value = value.to_owned();
        let (view, cx) = cx.add_window_view(move |window, cx| {
            let state = cx.new(|cx| {
                let state = InputState::new(window, cx);
                let state = if multi_line {
                    state.auto_grow(1, 5)
                } else {
                    state
                };
                state.default_value(value).placeholder("Passphrase")
            });
            Field {
                state,
                content_type,
            }
        });
        let state = view.read_with(cx, |view, _| view.state.clone());
        cx.update(|window, cx| {
            state.update(cx, |state, cx| state.focus(window, cx));
        });
        draw(cx);
        (state, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    fn shaped(state: &Entity<InputState>, cx: &mut VisualTestContext) -> String {
        layout(state, cx)
            .lines
            .iter()
            .map(|line| line.text.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn layout(state: &Entity<InputState>, cx: &mut VisualTestContext) -> LastLayout {
        state.read_with(cx, |state, _| {
            state
                .last_layout
                .clone()
                .expect("the field has been drawn once")
        })
    }

    fn bullet_width(layout: &LastLayout) -> Pixels {
        layout.lines[0].width() / layout.lines[0].text.chars().count() as f32
    }

    #[gpui::test]
    fn typing_into_a_password_field_shapes_bullets_and_keeps_the_value(cx: &mut TestAppContext) {
        let (state, cx) = field("", SECRET, cx);
        cx.simulate_input("hunter2");
        draw(cx);

        assert_eq!(shaped(&state, cx), "•".repeat(7));
        assert_eq!(
            state.read_with(cx, |state, _| state.value().to_string()),
            "hunter2",
            "the model is never masked"
        );
    }

    #[gpui::test]
    fn only_the_password_content_type_masks(cx: &mut TestAppContext) {
        for content_type in [PLAIN, Some(InputContentType::Url)] {
            let (state, cx) = field("hunter2", content_type, cx);
            assert_eq!(shaped(&state, cx), "hunter2", "{content_type:?}");
            assert!(layout(&state, cx).mask.is_none(), "{content_type:?}");
        }
    }

    #[gpui::test]
    fn one_bullet_per_grapheme_however_many_scalars_it_took(cx: &mut TestAppContext) {
        let (state, cx) = field("e\u{301}👨‍👩‍👧‍👦!", SECRET, cx);
        assert_eq!(shaped(&state, cx), "•••");
    }

    #[gpui::test]
    fn the_caret_lands_on_bullet_boundaries_for_multi_byte_graphemes(cx: &mut TestAppContext) {
        let (state, cx) = field("a😀b", SECRET, cx);
        let layout = layout(&state, cx);
        let bullet = bullet_width(&layout);

        for (bullets, offset) in [(0., 0), (1., 1), (2., 5), (3., 6)] {
            assert_eq!(
                layout.position_for_offset(offset).x,
                bullet * bullets,
                "value offset {offset}"
            );
        }
    }

    #[gpui::test]
    fn clicking_a_masked_field_selects_the_grapheme_boundary_under_the_pointer(
        cx: &mut TestAppContext,
    ) {
        let (state, cx) = field("a😀b", SECRET, cx);
        let layout = layout(&state, cx);
        let bullet = bullet_width(&layout);

        let position = layout.origin + point(bullet * 1.7, layout.line_height / 2.);
        cx.simulate_click(position, Modifiers::none());
        draw(cx);

        assert_eq!(
            state.read_with(cx, |state, _| state.selected_range()),
            5..5,
            "a click has to answer in the value's offsets"
        );
    }

    const LOPSIDED: &str = "👨‍👩‍👧‍👦a";

    #[gpui::test]
    fn the_caret_at_the_end_of_a_long_grapheme_sits_past_its_bullet(cx: &mut TestAppContext) {
        let (state, cx) = field(LOPSIDED, SECRET, cx);
        let layout = layout(&state, cx);
        let bullet = bullet_width(&layout);

        assert_eq!(layout.position_for_offset(LOPSIDED.len()).x, bullet * 2.);
    }

    #[gpui::test]
    fn a_selection_past_a_long_grapheme_still_draws(cx: &mut TestAppContext) {
        let (state, cx) = field(LOPSIDED, SECRET, cx);
        let layout = layout(&state, cx);
        let bullet = bullet_width(&layout);

        let selection = LOPSIDED.len() - 1..LOPSIDED.len();
        let [quad] = layout.selection_quads(&selection)[..] else {
            panic!("one row, one quad");
        };
        assert_eq!(quad.origin.x, bullet);
        assert_eq!(quad.size.width, bullet);
    }

    #[gpui::test]
    fn a_masked_selection_covers_the_bullets_it_spans(cx: &mut TestAppContext) {
        let (state, cx) = field("a😀b", SECRET, cx);
        let layout = layout(&state, cx);
        let bullet = bullet_width(&layout);

        let [quad] = layout.selection_quads(&(0..5))[..] else {
            panic!("one row, one quad");
        };
        assert_eq!(quad.origin.x, px(0.));
        assert_eq!(quad.size.width, bullet * 2.);
    }

    #[gpui::test]
    fn editing_a_masked_field_edits_the_real_value(cx: &mut TestAppContext) {
        let (state, cx) = field("hunter2", SECRET, cx);
        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                let end = state.value().len();
                state.set_selected_range(0..end, cx);
                state.replace("x", window, cx);
            });
        });
        draw(cx);

        assert_eq!(
            state.read_with(cx, |state, _| state.value().to_string()),
            "x"
        );
        assert_eq!(shaped(&state, cx), "•");
    }

    #[gpui::test]
    fn backspacing_a_masked_emoji_removes_one_bullet(cx: &mut TestAppContext) {
        let (state, cx) = field("a👨‍👩‍👧‍👦", SECRET, cx);
        assert_eq!(shaped(&state, cx), "••");

        cx.simulate_keystrokes("backspace");
        draw(cx);

        assert_eq!(shaped(&state, cx), "•");
        assert_eq!(
            state.read_with(cx, |state, _| state.value().to_string()),
            "a"
        );
    }

    #[gpui::test]
    fn a_password_placeholder_is_shown_as_itself(cx: &mut TestAppContext) {
        let (state, cx) = field("", SECRET, cx);
        assert_eq!(shaped(&state, cx), "Passphrase");
        assert!(layout(&state, cx).mask.is_none());
    }

    #[gpui::test]
    fn a_multi_line_field_ignores_masking(cx: &mut TestAppContext) {
        let (state, cx) = field_with("one\ntwo", SECRET, true, cx);
        assert_eq!(shaped(&state, cx), "one\ntwo");
        assert!(layout(&state, cx).mask.is_none());
    }
}
