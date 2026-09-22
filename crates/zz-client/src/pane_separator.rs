use zz_protocol::{Axis, LayoutNode, PaneId, SplitId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeparatorSpan {
    start: f32,
    length: f32,
}

impl SeparatorSpan {
    const FULL: Self = Self {
        start: 0.0,
        length: 1.0,
    };

    fn placed_within(self, start: f32, length: f32) -> Self {
        Self {
            start: start + self.start * length,
            length: self.length * length,
        }
    }

    pub const fn start(self) -> f32 {
        self.start
    }

    pub const fn length(self) -> f32 {
        self.length
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct NormalizedPaneRect {
    horizontal: SeparatorSpan,
    vertical: SeparatorSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeparatorSide {
    First,
    Second,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneSeparator {
    span: SeparatorSpan,
    side: SeparatorSide,
}

impl PaneSeparator {
    pub const fn span(self) -> SeparatorSpan {
        self.span
    }

    pub const fn side(self) -> SeparatorSide {
        self.side
    }
}

#[derive(Clone, Copy)]
enum PaneEdge {
    Start,
    End,
}

impl NormalizedPaneRect {
    const FULL: Self = Self {
        horizontal: SeparatorSpan::FULL,
        vertical: SeparatorSpan::FULL,
    };

    const fn span(self, axis: Axis) -> SeparatorSpan {
        match axis {
            Axis::Horizontal => self.horizontal,
            Axis::Vertical => self.vertical,
        }
    }

    fn placed_within(mut self, axis: Axis, start: f32, length: f32) -> Self {
        match axis {
            Axis::Horizontal => self.horizontal = self.horizontal.placed_within(start, length),
            Axis::Vertical => self.vertical = self.vertical.placed_within(start, length),
        }
        self
    }
}

pub fn pane_separator(
    node: &LayoutNode,
    pane: PaneId,
    ratio_override: Option<(SplitId, f32)>,
) -> Option<PaneSeparator> {
    let LayoutNode::Split {
        axis,
        first,
        second,
        ..
    } = node
    else {
        return None;
    };

    let (rect, touches_divider, side) =
        if let Some(rect) = relative_pane_rect(first, pane, ratio_override) {
            (
                rect,
                pane_touches_edge(first, pane, *axis, PaneEdge::End),
                SeparatorSide::First,
            )
        } else {
            let rect = relative_pane_rect(second, pane, ratio_override)?;
            (
                rect,
                pane_touches_edge(second, pane, *axis, PaneEdge::Start),
                SeparatorSide::Second,
            )
        };

    touches_divider.then_some(PaneSeparator {
        span: rect.span(orthogonal_axis(*axis)),
        side,
    })
}

fn pane_touches_edge(node: &LayoutNode, pane: PaneId, axis: Axis, edge: PaneEdge) -> bool {
    match node {
        LayoutNode::Pane(id) => *id == pane,
        LayoutNode::Split {
            axis: split_axis,
            first,
            second,
            ..
        } if *split_axis == axis => match edge {
            PaneEdge::Start => pane_touches_edge(first, pane, axis, edge),
            PaneEdge::End => pane_touches_edge(second, pane, axis, edge),
        },
        LayoutNode::Split { first, second, .. } => {
            pane_touches_edge(first, pane, axis, edge)
                || pane_touches_edge(second, pane, axis, edge)
        }
    }
}

fn relative_pane_rect(
    node: &LayoutNode,
    pane: PaneId,
    ratio_override: Option<(SplitId, f32)>,
) -> Option<NormalizedPaneRect> {
    match node {
        LayoutNode::Pane(id) => (*id == pane).then_some(NormalizedPaneRect::FULL),
        LayoutNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let ratio = resolved_ratio_with_override(*id, *ratio, ratio_override);
            if let Some(rect) = relative_pane_rect(first, pane, ratio_override) {
                Some(rect.placed_within(*axis, 0.0, ratio))
            } else {
                relative_pane_rect(second, pane, ratio_override)
                    .map(|rect| rect.placed_within(*axis, ratio, 1.0 - ratio))
            }
        }
    }
}

fn resolved_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

fn resolved_ratio_with_override(
    split: SplitId,
    snapshot_ratio: f32,
    ratio_override: Option<(SplitId, f32)>,
) -> f32 {
    resolved_ratio(
        ratio_override
            .filter(|(override_split, _)| *override_split == split)
            .map_or(snapshot_ratio, |(_, ratio)| ratio),
    )
}

const fn orthogonal_axis(axis: Axis) -> Axis {
    match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_protocol::SplitId;

    fn pane(id: u64) -> LayoutNode {
        LayoutNode::Pane(PaneId(id))
    }

    fn split(id: u64, axis: Axis, ratio: f32, first: LayoutNode, second: LayoutNode) -> LayoutNode {
        LayoutNode::Split {
            id: SplitId(id),
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn assert_separator(
        actual: Option<PaneSeparator>,
        start: f32,
        length: f32,
        side: SeparatorSide,
    ) {
        let actual = actual.expect("pane should touch divider");
        assert!((actual.span().start() - start).abs() <= f32::EPSILON);
        assert!((actual.span().length() - length).abs() <= f32::EPSILON);
        assert_eq!(actual.side(), side);
    }

    #[test]
    fn split_side_tracks_the_active_pane() {
        let layout = split(1, Axis::Horizontal, 0.5, pane(1), pane(2));

        assert_separator(
            pane_separator(&layout, PaneId(1), None),
            0.0,
            1.0,
            SeparatorSide::First,
        );
        assert_separator(
            pane_separator(&layout, PaneId(2), None),
            0.0,
            1.0,
            SeparatorSide::Second,
        );
    }

    #[test]
    fn t_junction_projects_only_the_active_pane_edge() {
        let layout = split(
            1,
            Axis::Horizontal,
            0.45,
            pane(1),
            split(2, Axis::Vertical, 0.4, pane(2), pane(3)),
        );

        assert_separator(
            pane_separator(&layout, PaneId(2), None),
            0.0,
            0.4,
            SeparatorSide::Second,
        );
        assert_separator(
            pane_separator(&layout, PaneId(3), None),
            0.4,
            0.6,
            SeparatorSide::Second,
        );

        let LayoutNode::Split { second, .. } = &layout else {
            unreachable!();
        };
        assert_separator(
            pane_separator(second, PaneId(2), None),
            0.0,
            1.0,
            SeparatorSide::First,
        );
        assert_separator(
            pane_separator(second, PaneId(3), None),
            0.0,
            1.0,
            SeparatorSide::Second,
        );
    }

    #[test]
    fn ancestor_divider_skips_a_pane_that_cannot_touch_it() {
        let layout = split(
            1,
            Axis::Horizontal,
            0.7,
            split(2, Axis::Horizontal, 0.5, pane(1), pane(2)),
            pane(3),
        );

        assert_eq!(pane_separator(&layout, PaneId(1), None), None);
        assert_separator(
            pane_separator(&layout, PaneId(2), None),
            0.0,
            1.0,
            SeparatorSide::First,
        );
    }

    #[test]
    fn perpendicular_drag_override_moves_the_ancestor_segment() {
        let layout = split(
            1,
            Axis::Horizontal,
            0.45,
            pane(1),
            split(2, Axis::Vertical, 0.4, pane(2), pane(3)),
        );

        assert_separator(
            pane_separator(&layout, PaneId(3), Some((SplitId(2), 0.65))),
            0.65,
            0.35,
            SeparatorSide::Second,
        );
    }

    #[test]
    fn vertical_ancestor_projects_the_active_horizontal_extent() {
        let layout = split(
            1,
            Axis::Vertical,
            0.4,
            pane(1),
            split(2, Axis::Horizontal, 0.3, pane(2), pane(3)),
        );

        assert_separator(
            pane_separator(&layout, PaneId(3), None),
            0.3,
            0.7,
            SeparatorSide::Second,
        );
        assert_eq!(pane_separator(&layout, PaneId(99), None), None);
    }
}
