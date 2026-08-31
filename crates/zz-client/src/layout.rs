use zz_protocol::{Axis, LayoutNode, PaneId};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NormalizedPaneRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl NormalizedPaneRect {
    pub const FULL: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };

    fn placed_within(self, axis: Axis, start: f32, length: f32) -> Self {
        match axis {
            Axis::Horizontal => Self {
                x: start.mul_add(self.width, self.x),
                width: self.width * length,
                ..self
            },
            Axis::Vertical => Self {
                y: start.mul_add(self.height, self.y),
                height: self.height * length,
                ..self
            },
        }
    }
}

pub fn pane_rects(layout: &LayoutNode) -> Vec<(PaneId, NormalizedPaneRect)> {
    let mut rects = Vec::new();
    collect_pane_rects(layout, NormalizedPaneRect::FULL, &mut rects);
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
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        assert_eq!(actual.0, pane);
        assert!((actual.1.x - x).abs() <= f32::EPSILON);
        assert!((actual.1.y - y).abs() <= f32::EPSILON);
        assert!((actual.1.width - width).abs() <= f32::EPSILON);
        assert!((actual.1.height - height).abs() <= f32::EPSILON);
    }

    #[test]
    fn single_pane_fills_the_layout() {
        let rects = pane_rects(&pane(7));

        assert_eq!(rects.len(), 1);
        assert_rect(rects[0], PaneId(7), 0.0, 0.0, 1.0, 1.0);
    }

    #[test]
    fn nested_splits_compose_normalized_geometry() {
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

    #[test]
    fn nested_splits_on_the_same_axis_stay_inside_the_parent() {
        let layout = split(
            1,
            Axis::Horizontal,
            0.4,
            pane(1),
            split(2, Axis::Horizontal, 0.5, pane(2), pane(3)),
        );
        let rects = pane_rects(&layout);

        assert_rect(rects[0], PaneId(1), 0.0, 0.0, 0.4, 1.0);
        assert_rect(rects[1], PaneId(2), 0.4, 0.0, 0.3, 1.0);
        assert_rect(rects[2], PaneId(3), 0.7, 0.0, 0.3, 1.0);
    }

    #[test]
    fn invalid_ratios_are_resolved_deterministically() {
        let non_finite = pane_rects(&split(1, Axis::Horizontal, f32::NAN, pane(1), pane(2)));
        let below_zero = pane_rects(&split(2, Axis::Vertical, -1.0, pane(3), pane(4)));
        let above_one = pane_rects(&split(3, Axis::Vertical, 2.0, pane(5), pane(6)));

        assert_rect(non_finite[0], PaneId(1), 0.0, 0.0, 0.5, 1.0);
        assert_rect(non_finite[1], PaneId(2), 0.5, 0.0, 0.5, 1.0);
        assert_rect(below_zero[0], PaneId(3), 0.0, 0.0, 1.0, 0.0);
        assert_rect(below_zero[1], PaneId(4), 0.0, 0.0, 1.0, 1.0);
        assert_rect(above_one[0], PaneId(5), 0.0, 0.0, 1.0, 1.0);
        assert_rect(above_one[1], PaneId(6), 0.0, 1.0, 1.0, 0.0);
    }
}
