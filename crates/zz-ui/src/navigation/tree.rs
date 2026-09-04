use std::{cmp::Ordering, ops::Range, rc::Rc};

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, Style, UniformListDecoration, Window, fill, point, size,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IndentGuideLayout {
    pub(crate) level: usize,
    pub(crate) start_row: usize,
    pub(crate) row_count: usize,
    pub(crate) continues_offscreen: bool,
}

#[derive(Clone, Copy)]
pub struct IndentGuideColors {
    pub default: Hsla,
    pub active: Hsla,
}

pub struct WorkspaceIndentGuides {
    depths: Rc<[usize]>,
    active_row: Option<usize>,
    indent_size: Pixels,
    left_offset: Pixels,
    end_padding: Pixels,
    colors: IndentGuideColors,
}

impl WorkspaceIndentGuides {
    pub fn new(
        depths: Rc<[usize]>,
        active_row: Option<usize>,
        indent_size: Pixels,
        left_offset: Pixels,
        end_padding: Pixels,
        colors: IndentGuideColors,
    ) -> Self {
        Self {
            depths,
            active_row,
            indent_size,
            left_offset,
            end_padding,
            colors,
        }
    }
}

impl UniformListDecoration for WorkspaceIndentGuides {
    fn compute(
        &self,
        mut visible_range: Range<usize>,
        bounds: Bounds<Pixels>,
        _scroll_offset: Point<Pixels>,
        item_height: Pixels,
        item_count: usize,
        window: &mut Window,
        _cx: &mut App,
    ) -> AnyElement {
        let includes_trailing_depth = visible_range.end < item_count;
        if includes_trailing_depth {
            visible_range.end += 1;
        }
        visible_range.end = visible_range.end.min(self.depths.len());
        visible_range.start = visible_range.start.min(visible_range.end);

        let layouts = compute_indent_guides(
            &self.depths[visible_range.clone()],
            visible_range.start,
            includes_trailing_depth,
        );
        let hairline = gpui::px(1.0) / window.scale_factor();
        let guides = layouts
            .into_iter()
            .map(|layout| {
                let padding = if layout.continues_offscreen {
                    gpui::px(0.0)
                } else {
                    self.end_padding
                };
                let active = self
                    .active_row
                    .is_some_and(|row| indent_guide_is_active(&layout, row, self.depths.as_ref()));
                RenderedIndentGuide {
                    bounds: Bounds::new(
                        point(
                            layout.level * self.indent_size + self.left_offset,
                            layout.start_row * item_height + padding,
                        ) + bounds.origin,
                        size(hairline, layout.row_count * item_height - padding * 2.0),
                    ),
                    active,
                }
            })
            .collect();

        IndentGuidesElement {
            guides: Rc::new(guides),
            colors: self.colors,
        }
        .into_any_element()
    }
}

fn indent_guide_is_active(layout: &IndentGuideLayout, active_row: usize, depths: &[usize]) -> bool {
    let end_row = layout.start_row.saturating_add(layout.row_count);
    let active_depth = depths.get(active_row).copied().unwrap_or_default();

    layout.level + 1 == active_depth && layout.start_row <= active_row && active_row < end_row
}

struct RenderedIndentGuide {
    bounds: Bounds<Pixels>,
    active: bool,
}

struct IndentGuidesElement {
    guides: Rc<Vec<RenderedIndentGuide>>,
    colors: IndentGuideColors,
}

impl Element for IndentGuidesElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        for guide in self.guides.iter() {
            window.paint_quad(fill(
                window.pixel_snap_bounds(guide.bounds),
                if guide.active {
                    self.colors.active
                } else {
                    self.colors.default
                },
            ));
        }
    }
}

impl IntoElement for IndentGuidesElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

// Adapted from Zed's project-panel indent-guide stack algorithm.
pub(crate) fn compute_indent_guides(
    depths: &[usize],
    offset: usize,
    includes_trailing_depth: bool,
) -> Vec<IndentGuideLayout> {
    let mut completed = Vec::new();
    let mut open = Vec::new();

    for (row, &depth) in depths.iter().enumerate() {
        if includes_trailing_depth && row + 1 == depths.len() {
            continue;
        }

        let current_row = row + offset;
        match depth.cmp(&open.len()) {
            Ordering::Less => {
                for _ in depth..open.len() {
                    if let Some(guide) = open.pop() {
                        completed.push(guide);
                    }
                }
            }
            Ordering::Greater => {
                for level in open.len()..depth {
                    open.push(IndentGuideLayout {
                        level,
                        start_row: current_row,
                        row_count: 0,
                        continues_offscreen: false,
                    });
                }
            }
            Ordering::Equal => {}
        }

        for guide in &mut open {
            guide.row_count = current_row - guide.start_row + 1;
        }
    }

    completed.extend(open);
    if includes_trailing_depth {
        let trailing_row = offset + depths.len().saturating_sub(1);
        let trailing_depth = depths.last().copied().unwrap_or_default();
        for guide in &mut completed {
            if guide.start_row + guide.row_count == trailing_row {
                guide.continues_offscreen = guide.level < trailing_depth;
            }
        }
    }
    completed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut layouts: Vec<IndentGuideLayout>) -> Vec<IndentGuideLayout> {
        layouts.sort_by_key(|layout| (layout.level, layout.start_row));
        layouts
    }

    #[test]
    fn nested_depths_become_continuous_guide_ranges() {
        assert_eq!(
            sorted(compute_indent_guides(&[0, 1, 2, 2, 1, 0], 0, false)),
            vec![
                IndentGuideLayout {
                    level: 0,
                    start_row: 1,
                    row_count: 4,
                    continues_offscreen: false,
                },
                IndentGuideLayout {
                    level: 1,
                    start_row: 2,
                    row_count: 2,
                    continues_offscreen: false,
                },
            ]
        );
    }

    #[test]
    fn workspace_root_opens_one_guide_for_each_domain_level() {
        assert_eq!(
            sorted(compute_indent_guides(&[0, 1, 2, 3], 0, false)),
            vec![
                IndentGuideLayout {
                    level: 0,
                    start_row: 1,
                    row_count: 3,
                    continues_offscreen: false,
                },
                IndentGuideLayout {
                    level: 1,
                    start_row: 2,
                    row_count: 2,
                    continues_offscreen: false,
                },
                IndentGuideLayout {
                    level: 2,
                    start_row: 3,
                    row_count: 1,
                    continues_offscreen: false,
                },
            ]
        );
    }

    #[test]
    fn trailing_depth_marks_guides_that_continue_below_the_viewport() {
        let layouts = sorted(compute_indent_guides(&[2, 2, 2], 8, true));
        assert_eq!(layouts.len(), 2);
        assert!(layouts.iter().all(|layout| layout.continues_offscreen));
        assert_eq!(layouts[0].start_row, 8);
        assert_eq!(layouts[0].row_count, 2);
    }

    #[test]
    fn active_path_lights_only_the_deepest_ancestor_guide() {
        let depths = [0, 1, 2, 3, 3, 3, 1, 2, 3];
        let active = sorted(compute_indent_guides(&depths, 0, false))
            .into_iter()
            .filter(|layout| indent_guide_is_active(layout, 4, &depths))
            .map(|layout| (layout.level, layout.start_row, layout.row_count))
            .collect::<Vec<_>>();

        assert_eq!(active, vec![(2, 3, 3)]);
    }
}
