use std::{ops::Range, rc::Rc};

use gpui::{
    App, Bounds, ContentMask, Corners, CursorStyle, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
    IntoElement, LayoutId, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString,
    Size, Style, TextAlign, TextRun, Window, WrappedLine, fill, point, px, relative, size,
};

use crate::{ActiveTheme as _, Colorize as _};

use super::{CodeEditorState, RopeExt as _, blink_cursor::CURSOR_WIDTH};

const GUTTER_PADDING: Pixels = px(5.0);
const NEWLINE_SELECTION_WIDTH: Pixels = px(4.0);
const LINE_NUMBER_SCALE: f32 = 0.85;

#[derive(Clone)]
pub(crate) struct LastLayout {
    pub(super) visible_range: Range<usize>,
    pub(super) visible_buffer_lines: Vec<usize>,
    pub(super) visible_line_byte_offsets: Vec<usize>,
    pub(super) visible_top: Pixels,
    pub(super) visible_range_offset: Range<usize>,
    pub(super) lines: Rc<Vec<super::display_map::LineLayout>>,
    pub(super) line_height: Pixels,
    pub(super) wrap_width: Option<Pixels>,
    pub(super) line_number_width: Pixels,
    pub(super) cursor_bounds: Option<Bounds<Pixels>>,
    pub(super) text_align: TextAlign,
    pub(super) content_width: Pixels,
}

impl LastLayout {
    pub(super) fn alignment_offset(&self, line_width: Pixels) -> Pixels {
        match self.text_align {
            TextAlign::Left => Pixels::ZERO,
            TextAlign::Center => ((self.content_width - line_width) / 2.).max(Pixels::ZERO),
            TextAlign::Right => (self.content_width - line_width).max(Pixels::ZERO),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct WhitespaceIndicators {
    pub(super) space: gpui::ShapedLine,
    pub(super) tab: gpui::ShapedLine,
}

#[derive(Clone)]
pub(super) struct EditorLayout {
    lines: Rc<Vec<WrappedLine>>,
    line_starts: Rc<Vec<usize>>,
    line_tops: Rc<Vec<Pixels>>,
    placeholder: bool,
    pub(super) line_height: Pixels,
    pub(super) text_origin: Point<Pixels>,
    pub(super) text_bounds: Bounds<Pixels>,
    pub(super) viewport_bounds: Bounds<Pixels>,
    pub(super) content_size: Size<Pixels>,
    pub(super) gutter_width: Pixels,
}

impl EditorLayout {
    fn rows(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.wrap_boundaries().len() + 1)
            .sum::<usize>()
            .max(1)
    }

    pub(super) fn offset_for_position(&self, position: Point<Pixels>) -> usize {
        self.offset_for_local_position(position - self.text_origin)
    }

    fn offset_for_local_position(&self, local: Point<Pixels>) -> usize {
        if self.placeholder || self.lines.is_empty() {
            return 0;
        }

        let mut index = self.lines.len() - 1;
        for (candidate, top) in self.line_tops.iter().enumerate() {
            let height = self.lines[candidate].size(self.line_height).height;
            if local.y < *top + height {
                index = candidate;
                break;
            }
        }

        let line = &self.lines[index];
        let height = line.size(self.line_height).height;
        let ceiling = (height - px(1.0)).max(Pixels::ZERO);
        let inner = point(
            local.x.max(Pixels::ZERO),
            (local.y - self.line_tops[index]).clamp(Pixels::ZERO, ceiling),
        );
        let offset = line
            .closest_index_for_position(inner, self.line_height)
            .unwrap_or_else(|offset| offset);
        self.line_starts[index] + offset.min(line.len())
    }

    pub(super) fn position_for_offset(&self, offset: usize) -> Point<Pixels> {
        if self.placeholder || self.lines.is_empty() {
            return Point::default();
        }
        let index = self.line_index_for_offset(offset);
        let line = &self.lines[index];
        let local = offset
            .saturating_sub(self.line_starts[index])
            .min(line.len());
        let position = line
            .position_for_index(local, self.line_height)
            .unwrap_or_default();
        point(position.x, position.y + self.line_tops[index])
    }

    fn line_index_for_offset(&self, offset: usize) -> usize {
        self.line_starts
            .iter()
            .enumerate()
            .find_map(|(index, start)| (offset <= start + self.lines[index].len()).then_some(index))
            .unwrap_or_else(|| self.lines.len().saturating_sub(1))
    }

    fn selection_quads(&self, selection: &Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut quads = Vec::new();
        if selection.is_empty() || self.placeholder {
            return quads;
        }

        for (index, line) in self.lines.iter().enumerate() {
            let line_start = self.line_starts[index];
            let line_end = line_start + line.len();
            if selection.end < line_start || selection.start > line_end {
                continue;
            }

            let from = selection.start.saturating_sub(line_start).min(line.len());
            let to = selection.end.saturating_sub(line_start).min(line.len());
            let covers_break = selection.end > line_end;
            let starts = row_starts(line);

            for (row, row_start) in starts.iter().copied().enumerate() {
                let row_end = starts.get(row + 1).copied().unwrap_or(line.len());
                let start = from.max(row_start);
                let end = to.min(row_end);
                let trailing = if row + 1 == starts.len() && covers_break {
                    NEWLINE_SELECTION_WIDTH
                } else {
                    Pixels::ZERO
                };
                if start > end || (start == end && trailing == Pixels::ZERO) {
                    continue;
                }

                let x = |offset: usize| {
                    if offset == row_start {
                        Pixels::ZERO
                    } else {
                        line.position_for_index(offset, self.line_height)
                            .unwrap_or_default()
                            .x
                    }
                };
                let left = x(start);
                let right = x(end);
                quads.push(Bounds {
                    origin: point(left, self.line_tops[index] + self.line_height * row as f32),
                    size: size((right - left) + trailing, self.line_height),
                });
            }
        }
        quads
    }
}

fn row_starts(line: &WrappedLine) -> Vec<usize> {
    let mut starts = Vec::with_capacity(line.wrap_boundaries().len() + 1);
    starts.push(0);
    for boundary in line.wrap_boundaries() {
        starts.push(line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index);
    }
    starts
}

#[derive(Clone)]
pub(super) struct ShapedText {
    lines: Rc<Vec<WrappedLine>>,
    line_starts: Rc<Vec<usize>>,
    line_tops: Rc<Vec<Pixels>>,
    content_size: Size<Pixels>,
}

pub(super) struct ShapedCache {
    generation: u64,
    wrap_width: Option<Pixels>,
    font: gpui::Font,
    font_size: Pixels,
    color: gpui::Hsla,
    shaped: ShapedText,
}

impl ShapedCache {
    fn matches(
        &self,
        generation: u64,
        wrap_width: Option<Pixels>,
        font_size: Pixels,
        color: gpui::Hsla,
        font: &gpui::Font,
    ) -> bool {
        self.generation == generation
            && self.wrap_width == wrap_width
            && self.font_size == font_size
            && self.color == color
            && &self.font == font
    }
}

pub(super) struct TextElement {
    state: Entity<CodeEditorState>,
}

impl TextElement {
    pub(super) const fn new(state: Entity<CodeEditorState>) -> Self {
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
    layout: EditorLayout,
    text_hitbox: Hitbox,
    line_numbers: Vec<(usize, gpui::ShapedLine)>,
    selection_quads: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
    cursor_glyph: Option<gpui::ShapedLine>,
    current_line: usize,
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
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
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
        let text = state.render_text.clone();
        let placeholder = state.placeholder.clone();
        let is_placeholder = text.is_empty() && !placeholder.is_empty();
        let display_text = if is_placeholder { placeholder } else { text };
        let line_count = state.text.lines_len().max(1);
        let line_digits = line_count.to_string().len().max(2);
        let digit_sample: SharedString = "8".repeat(line_digits).into();
        let muted = cx.theme().foreground.muted();
        let number_font_size = font_size * LINE_NUMBER_SCALE;
        let digit_run = TextRun {
            color: muted,
            ..text_style.to_run(digit_sample.len())
        };
        let digit_line =
            window
                .text_system()
                .shape_line(digit_sample, number_font_size, &[digit_run], None);
        let gutter_width = if state.mode.line_numbers() {
            digit_line.width() + GUTTER_PADDING * 2.0
        } else {
            Pixels::ZERO
        };
        let text_bounds = Bounds {
            origin: bounds.origin + point(gutter_width, Pixels::ZERO),
            size: size(
                (bounds.size.width - gutter_width).max(Pixels::ZERO),
                bounds.size.height,
            ),
        };

        let wrap_width = state.soft_wrap.then_some(text_bounds.size.width);
        let font = text_style.font();
        let generation = state.layout_generation;
        let show_line_numbers = gutter_width > Pixels::ZERO;
        let cached = if is_placeholder {
            None
        } else {
            state.shaped_cache.as_ref().and_then(|cache| {
                cache
                    .matches(generation, wrap_width, font_size, text_style.color, &font)
                    .then(|| cache.shaped.clone())
            })
        };
        let (shaped, fresh_cache) = if let Some(shaped) = cached {
            (shaped, None)
        } else {
            let runs = if is_placeholder {
                vec![TextRun {
                    color: muted,
                    ..text_style.to_run(display_text.len())
                }]
            } else {
                state.text_runs(&text_style, cx)
            };
            let lines: Vec<WrappedLine> = window
                .text_system()
                .shape_text(display_text, font_size, &runs, wrap_width, None)
                .unwrap_or_default()
                .into_iter()
                .collect();

            let mut line_starts = Vec::with_capacity(lines.len());
            let mut line_tops = Vec::with_capacity(lines.len());
            let mut offset = 0usize;
            let mut height = Pixels::ZERO;
            let mut width = Pixels::ZERO;
            for line in &lines {
                line_starts.push(offset);
                line_tops.push(height);
                offset += line.len() + 1;
                height += line.size(line_height).height;
                width = width.max(line.width());
            }
            height = height.max(line_height);

            let shaped = ShapedText {
                lines: Rc::new(lines),
                line_starts: Rc::new(line_starts),
                line_tops: Rc::new(line_tops),
                content_size: size(width, height),
            };
            let cache = (!is_placeholder).then(|| ShapedCache {
                generation,
                wrap_width,
                font,
                font_size,
                color: text_style.color,
                shaped: shaped.clone(),
            });
            (shaped, cache)
        };

        let mut layout = EditorLayout {
            lines: shaped.lines.clone(),
            line_starts: shaped.line_starts.clone(),
            line_tops: shaped.line_tops.clone(),
            placeholder: is_placeholder,
            line_height,
            text_origin: text_bounds.origin,
            text_bounds,
            viewport_bounds: bounds,
            content_size: shaped.content_size,
            gutter_width,
        };

        let cursor = state.cursor();
        let caret_position = layout.position_for_offset(cursor);
        let scroll = scroll_offset(
            &layout,
            caret_position,
            state.scroll,
            state.follow_cursor,
            state.reset_scroll,
        );
        layout.text_origin = text_bounds.origin + scroll;
        let selection = state
            .vim_highlight_range()
            .unwrap_or_else(|| state.selected_range());
        let show_caret = state.show_cursor(window, cx);
        let current_line = layout.line_index_for_offset(cursor);
        let selection_quads = layout
            .selection_quads(&selection)
            .into_iter()
            .map(|quad| Bounds {
                origin: layout.text_origin + quad.origin,
                size: quad.size,
            })
            .collect();

        let block_cursor = state.vim_block_cursor() && !is_placeholder;
        let cursor_grapheme = block_cursor
            .then(|| state.render_text.get(cursor..state.next_boundary(cursor)))
            .flatten()
            .filter(|grapheme| !grapheme.is_empty() && *grapheme != "\n");
        let cursor_width = if block_cursor {
            cursor_grapheme
                .and_then(|grapheme| {
                    let end = layout.position_for_offset(cursor + grapheme.len());
                    (end.y == caret_position.y && end.x > caret_position.x)
                        .then(|| end.x - caret_position.x)
                })
                .unwrap_or_else(|| {
                    let run = text_style.to_run(1);
                    window
                        .text_system()
                        .shape_line(" ".into(), font_size, &[run], None)
                        .width()
                })
        } else {
            CURSOR_WIDTH
        };
        let caret = show_caret.then(|| Bounds {
            origin: layout.text_origin + caret_position,
            size: size(cursor_width, line_height),
        });
        let cursor_glyph = show_caret
            .then_some(cursor_grapheme)
            .flatten()
            .map(|grapheme| {
                let grapheme: SharedString = grapheme.to_string().into();
                let run = TextRun {
                    color: cx.theme().editor_background(),
                    ..text_style.to_run(grapheme.len())
                };
                window
                    .text_system()
                    .shape_line(grapheme, font_size, &[run], None)
            });

        let line_numbers = if show_line_numbers {
            let relative = state.mode.relative_line_numbers();
            visible_lines(&layout)
                .map(|index| {
                    let number = if relative && index != current_line {
                        current_line.abs_diff(index)
                    } else {
                        index + 1
                    };
                    let number: SharedString = number.to_string().into();
                    let run = TextRun {
                        color: if relative && index == current_line {
                            cx.theme().foreground
                        } else {
                            muted
                        },
                        ..text_style.to_run(number.len())
                    };
                    (
                        index,
                        window
                            .text_system()
                            .shape_line(number, number_font_size, &[run], None),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        self.state.update(cx, |state, cx| {
            if let Some(cache) = fresh_cache {
                state.shaped_cache = Some(cache);
            }
            state.editor_layout = Some(layout.clone());
            state.scroll = scroll;
            state.follow_cursor = false;
            state.reset_scroll = false;
            state.sync_display_map_layout(
                state.soft_wrap.then_some(text_bounds.size.width),
                text_style.font(),
                font_size,
                cx,
            );
        });

        PrepaintState {
            text_hitbox: window.insert_hitbox(
                Bounds {
                    origin: bounds.origin + point(gutter_width, Pixels::ZERO),
                    size: size(
                        (bounds.size.width - gutter_width).max(Pixels::ZERO),
                        bounds.size.height,
                    ),
                },
                HitboxBehavior::Normal,
            ),
            layout,
            line_numbers,
            selection_quads,
            caret,
            cursor_glyph,
            current_line,
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
        if !self.state.read(cx).disabled {
            window.set_cursor_style(CursorStyle::IBeam, &prepaint.text_hitbox);
        }
        let focus_handle = self.state.read(cx).focus_handle_ref().clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(layout.viewport_bounds, self.state.clone()),
            cx,
        );

        let background = cx.theme().editor_background();
        let active_line = background.raised(1);
        let selection = cx.theme().foreground.wash();
        let caret = cx.theme().foreground;
        let gutter = background.raised(1).opaque();
        let divider = cx.theme().border;
        let radii = self.state.read(cx).corner_radii;
        let gutter_radii = Corners {
            top_left: radii.top_left,
            bottom_left: radii.bottom_left,
            top_right: Pixels::ZERO,
            bottom_right: Pixels::ZERO,
        };

        window.with_content_mask(
            Some(ContentMask {
                bounds: layout.viewport_bounds,
            }),
            |window| {
                window.paint_quad(fill(layout.viewport_bounds, background).corner_radii(radii));

                if layout.gutter_width > Pixels::ZERO {
                    let gutter_bounds = Bounds {
                        origin: layout.viewport_bounds.origin,
                        size: size(layout.gutter_width, layout.viewport_bounds.size.height),
                    };
                    window.paint_quad(fill(gutter_bounds, gutter).corner_radii(gutter_radii));
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(gutter_bounds.right() - px(1.0), gutter_bounds.origin.y),
                            size: size(px(1.0), gutter_bounds.size.height),
                        },
                        divider,
                    ));
                }

                if let Some(line) = layout.lines.get(prepaint.current_line) {
                    let active_bounds = Bounds {
                        origin: point(
                            layout.text_bounds.origin.x,
                            layout.text_origin.y + layout.line_tops[prepaint.current_line],
                        ),
                        size: size(
                            layout.text_bounds.size.width,
                            line.size(layout.line_height).height,
                        ),
                    };
                    paint_viewport_quad(
                        active_bounds,
                        active_line,
                        layout.viewport_bounds,
                        radii,
                        window,
                    );
                }

                for quad in &prepaint.selection_quads {
                    paint_viewport_quad(*quad, selection, layout.viewport_bounds, radii, window);
                }

                for (line, top) in layout.lines.iter().zip(layout.line_tops.iter()) {
                    let origin = layout.text_origin + point(Pixels::ZERO, *top);
                    if origin.y + line.size(layout.line_height).height
                        < layout.viewport_bounds.origin.y
                        || origin.y > layout.viewport_bounds.bottom()
                    {
                        continue;
                    }
                    _ = line.paint(
                        origin,
                        layout.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }

                for (index, number) in &prepaint.line_numbers {
                    let number_origin = point(
                        layout.viewport_bounds.origin.x + layout.gutter_width
                            - GUTTER_PADDING
                            - number.width(),
                        layout.text_origin.y + layout.line_tops[*index],
                    );
                    _ = number.paint(
                        number_origin,
                        layout.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }

                if let Some(caret_bounds) = prepaint.caret {
                    paint_viewport_quad(caret_bounds, caret, layout.viewport_bounds, radii, window);
                    if let Some(glyph) = prepaint.cursor_glyph.as_ref() {
                        _ = glyph.paint(
                            caret_bounds.origin,
                            layout.line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
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

fn paint_viewport_quad(
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
    viewport: Bounds<Pixels>,
    radii: Corners<Pixels>,
    window: &mut Window,
) {
    let quad = bounds.intersect(&viewport);
    if quad.size.width <= Pixels::ZERO || quad.size.height <= Pixels::ZERO {
        return;
    }
    let epsilon = px(1.0);
    let left = quad.origin.x - viewport.origin.x < epsilon;
    let right = viewport.right() - quad.right() < epsilon;
    let top = quad.origin.y - viewport.origin.y < epsilon;
    let bottom = viewport.bottom() - quad.bottom() < epsilon;
    let cap = quad.size.width.min(quad.size.height) / 2.0;
    let corner = |flush: bool, radius: Pixels| if flush { radius.min(cap) } else { Pixels::ZERO };
    let quad_radii = Corners {
        top_left: corner(top && left, radii.top_left),
        top_right: corner(top && right, radii.top_right),
        bottom_right: corner(bottom && right, radii.bottom_right),
        bottom_left: corner(bottom && left, radii.bottom_left),
    };
    window.paint_quad(fill(quad, color).corner_radii(quad_radii));
}

fn visible_lines(layout: &EditorLayout) -> Range<usize> {
    let top = layout.viewport_bounds.origin.y - layout.text_origin.y;
    let bottom = layout.viewport_bounds.bottom() - layout.text_origin.y;
    let mut first = 0;
    let mut last = 0;
    for (index, line_top) in layout.line_tops.iter().enumerate() {
        if *line_top + layout.lines[index].size(layout.line_height).height < top {
            first = index + 1;
            continue;
        }
        if *line_top > bottom {
            break;
        }
        last = index + 1;
    }
    first..last.max(first)
}

fn scroll_offset(
    layout: &EditorLayout,
    caret: Point<Pixels>,
    previous: Point<Pixels>,
    follow_cursor: bool,
    reset: bool,
) -> Point<Pixels> {
    let viewport = layout.text_bounds.size;
    let mut scroll = if reset { Point::default() } else { previous };
    if follow_cursor && !reset {
        let top = caret.y;
        let bottom = top + layout.line_height;
        if top + scroll.y < Pixels::ZERO {
            scroll.y = -top;
        } else if bottom + scroll.y > viewport.height {
            scroll.y = viewport.height - bottom;
        }

        let right = caret.x + CURSOR_WIDTH;
        if caret.x + scroll.x < Pixels::ZERO {
            scroll.x = -caret.x;
        } else if right + scroll.x > viewport.width {
            scroll.x = viewport.width - right;
        }
    }

    let overflow_x = (layout.content_size.width + CURSOR_WIDTH - viewport.width).max(Pixels::ZERO);
    let overflow_y = (layout.content_size.height - viewport.height).max(Pixels::ZERO);
    scroll.x = scroll.x.clamp(-overflow_x, Pixels::ZERO);
    scroll.y = scroll.y.clamp(-overflow_y, Pixels::ZERO);
    scroll
}
