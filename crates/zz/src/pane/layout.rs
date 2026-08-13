use zz_protocol::{Axis, LayoutNode, PaneId};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Span {
    start: f32,
    length: f32,
}

impl Span {
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NormalizedPaneRect {
    horizontal: Span,
    vertical: Span,
}

impl NormalizedPaneRect {
    const FULL: Self = Self {
        horizontal: Span::FULL,
        vertical: Span::FULL,
    };

    pub(crate) const fn left(self) -> f32 {
        self.horizontal.start
    }

    pub(crate) const fn top(self) -> f32 {
        self.vertical.start
    }

    pub(crate) const fn width(self) -> f32 {
        self.horizontal.length
    }

    pub(crate) const fn height(self) -> f32 {
        self.vertical.length
    }

    fn placed_within(mut self, axis: Axis, start: f32, length: f32) -> Self {
        match axis {
            Axis::Horizontal => self.horizontal = self.horizontal.placed_within(start, length),
            Axis::Vertical => self.vertical = self.vertical.placed_within(start, length),
        }
        self
    }
}

pub(crate) fn pane_rects(node: &LayoutNode) -> Vec<(PaneId, NormalizedPaneRect)> {
    let mut rects = Vec::new();
    collect_pane_rects(node, NormalizedPaneRect::FULL, &mut rects);
    rects
}

fn collect_pane_rects(
    node: &LayoutNode,
    rect: NormalizedPaneRect,
    rects: &mut Vec<(PaneId, NormalizedPaneRect)>,
) {
    match node {
        LayoutNode::Pane(pane) => rects.push((*pane, rect)),
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
            ..
        } => {
            let ratio = resolved_ratio(*ratio);
            collect_pane_rects(first, rect.placed_within(*axis, 0.0, ratio), rects);
            collect_pane_rects(second, rect.placed_within(*axis, ratio, 1.0 - ratio), rects);
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

    fn assert_rect(
        actual: (PaneId, NormalizedPaneRect),
        pane: PaneId,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
    ) {
        assert_eq!(actual.0, pane);
        let actual = actual.1;
        assert!((actual.left() - left).abs() <= f32::EPSILON);
        assert!((actual.top() - top).abs() <= f32::EPSILON);
        assert!((actual.width() - width).abs() <= f32::EPSILON);
        assert!((actual.height() - height).abs() <= f32::EPSILON);
    }

    #[test]
    fn pane_rects_map_a_single_pane_to_the_full_layout() {
        let rects = pane_rects(&pane(7));

        assert_eq!(rects.len(), 1);
        assert_rect(rects[0], PaneId(7), 0.0, 0.0, 1.0, 1.0);
    }

    #[test]
    fn pane_rects_preserve_split_ratios() {
        let layout = split(1, Axis::Horizontal, 0.35, pane(1), pane(2));
        let rects = pane_rects(&layout);

        assert_eq!(rects.len(), 2);
        assert_rect(rects[0], PaneId(1), 0.0, 0.0, 0.35, 1.0);
        assert_rect(rects[1], PaneId(2), 0.35, 0.0, 0.65, 1.0);
    }

    #[test]
    fn pane_rects_compose_nested_split_geometry() {
        let layout = split(
            1,
            Axis::Horizontal,
            0.4,
            pane(1),
            split(2, Axis::Vertical, 0.25, pane(2), pane(3)),
        );
        let rects = pane_rects(&layout);

        assert_eq!(rects.len(), 3);
        assert_rect(rects[0], PaneId(1), 0.0, 0.0, 0.4, 1.0);
        assert_rect(rects[1], PaneId(2), 0.4, 0.0, 0.6, 0.25);
        assert_rect(rects[2], PaneId(3), 0.4, 0.25, 0.6, 0.75);
    }
}
