use zz_protocol::layout::{joined_layout, swapped_layout};
use zz_protocol::{Axis, CommandInvocation, LayoutNode, PaneId, SplitId};

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

const MIN_DROP_EDGE: f32 = 80.0;
const DROP_EDGE_FRACTION: f32 = 0.25;
const OPTIMISTIC_SPLIT: SplitId = SplitId(u64::MAX);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaneRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl DropZone {
    const fn axis(self) -> Option<Axis> {
        match self {
            Self::Left | Self::Right => Some(Axis::Horizontal),
            Self::Top | Self::Bottom => Some(Axis::Vertical),
            Self::Center => None,
        }
    }

    const fn inserts_first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

pub fn pane_swap_command(source: PaneId, target: PaneId) -> CommandInvocation {
    CommandInvocation::new(
        "swap-pane",
        vec![
            "-d".to_owned(),
            "-s".to_owned(),
            source.to_string(),
            "-t".to_owned(),
            target.to_string(),
        ],
    )
}

pub fn pane_join_command(
    source: PaneId,
    target: PaneId,
    zone: DropZone,
) -> Option<CommandInvocation> {
    let axis = zone.axis()?;
    let mut args = vec!["-d".to_owned()];
    if zone.inserts_first() {
        args.push("-b".to_owned());
    }
    args.push(
        match axis {
            Axis::Horizontal => "-h",
            Axis::Vertical => "-v",
        }
        .to_owned(),
    );
    args.extend([
        "-s".to_owned(),
        source.to_string(),
        "-t".to_owned(),
        target.to_string(),
    ]);
    Some(CommandInvocation::new("join-pane", args))
}

pub fn pane_drop_command(
    source: PaneId,
    target: PaneId,
    zone: DropZone,
) -> Option<CommandInvocation> {
    if source == target {
        return None;
    }
    match zone {
        DropZone::Center => Some(pane_swap_command(source, target)),
        zone => pane_join_command(source, target, zone),
    }
}

pub fn predicted_drop_layout(
    layout: &LayoutNode,
    source: PaneId,
    target: PaneId,
    zone: DropZone,
) -> Option<LayoutNode> {
    match zone.axis() {
        None => Some(swapped_layout(layout, source, target)),
        Some(axis) => joined_layout(
            layout,
            source,
            target,
            OPTIMISTIC_SPLIT,
            axis,
            0.5,
            zone.inserts_first(),
        ),
    }
}

pub fn drop_zone_at(slot: PaneRect, position: (f32, f32)) -> DropZone {
    let local_x = position.0 - slot.x;
    let local_y = position.1 - slot.y;
    let edge_x = MIN_DROP_EDGE.max(slot.width * DROP_EDGE_FRACTION);
    let edge_y = MIN_DROP_EDGE.max(slot.height * DROP_EDGE_FRACTION);
    if local_x < edge_x {
        DropZone::Left
    } else if local_x > slot.width - edge_x {
        DropZone::Right
    } else if local_y < edge_y {
        DropZone::Top
    } else if local_y > slot.height - edge_y {
        DropZone::Bottom
    } else {
        DropZone::Center
    }
}

pub fn coerced_drop_zone(
    layout: &LayoutNode,
    source: PaneId,
    target: PaneId,
    zone: DropZone,
) -> DropZone {
    if zone == DropZone::Center {
        return zone;
    }
    let redundant = predicted_drop_layout(layout, source, target, zone)
        .is_some_and(|predicted| same_arrangement(&predicted, layout));
    if redundant { DropZone::Center } else { zone }
}

pub fn same_arrangement(left: &LayoutNode, right: &LayoutNode) -> bool {
    match (left, right) {
        (LayoutNode::Pane(left), LayoutNode::Pane(right)) => left == right,
        (
            LayoutNode::Split {
                axis: left_axis,
                first: left_first,
                second: left_second,
                ..
            },
            LayoutNode::Split {
                axis: right_axis,
                first: right_first,
                second: right_second,
                ..
            },
        ) => {
            left_axis == right_axis
                && same_arrangement(left_first, right_first)
                && same_arrangement(left_second, right_second)
        }
        _ => false,
    }
}

pub fn pane_box(slot: PaneRect, divider: f32) -> PaneRect {
    let left = if slot.x > 0.0 { divider } else { 0.0 };
    let top = if slot.y > 0.0 { divider } else { 0.0 };
    PaneRect {
        x: slot.x + left,
        y: slot.y + top,
        width: slot.width - left,
        height: slot.height - top,
    }
}

pub fn drop_preview_bounds(slot: PaneRect, zone: DropZone, divider: f32) -> PaneRect {
    let pane = pane_box(slot, divider);
    let half_width = pane.width / 2.0;
    let half_height = pane.height / 2.0;
    match zone {
        DropZone::Center => pane,
        DropZone::Left => PaneRect {
            width: half_width,
            ..pane
        },
        DropZone::Right => PaneRect {
            x: pane.x + half_width + divider,
            width: half_width - divider,
            ..pane
        },
        DropZone::Top => PaneRect {
            height: half_height,
            ..pane
        },
        DropZone::Bottom => PaneRect {
            y: pane.y + half_height + divider,
            height: half_height - divider,
            ..pane
        },
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
    fn three_pane_layout() -> LayoutNode {
        LayoutNode::Split {
            id: SplitId(1),
            axis: Axis::Horizontal,
            ratio: 0.4,
            first: Box::new(LayoutNode::Pane(PaneId(3))),
            second: Box::new(LayoutNode::Split {
                id: SplitId(2),
                axis: Axis::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(PaneId(7))),
                second: Box::new(LayoutNode::Pane(PaneId(9))),
            }),
        }
    }

    #[test]
    fn drop_zones_resolve_corners_horizontally_and_flood_narrow_panes() {
        let slot = PaneRect {
            x: 100.0,
            y: 50.0,
            width: 800.0,
            height: 400.0,
        };
        assert_eq!(drop_zone_at(slot, (150.0, 250.0)), DropZone::Left);
        assert_eq!(drop_zone_at(slot, (850.0, 250.0)), DropZone::Right);
        assert_eq!(drop_zone_at(slot, (500.0, 80.0)), DropZone::Top);
        assert_eq!(drop_zone_at(slot, (500.0, 420.0)), DropZone::Bottom);
        assert_eq!(drop_zone_at(slot, (500.0, 250.0)), DropZone::Center);
        assert_eq!(drop_zone_at(slot, (110.0, 60.0)), DropZone::Left);
        assert_eq!(drop_zone_at(slot, (890.0, 440.0)), DropZone::Right);

        let narrow = PaneRect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 90.0,
        };
        assert_eq!(drop_zone_at(narrow, (79.0, 45.0)), DropZone::Left);
        assert_eq!(drop_zone_at(narrow, (81.0, 45.0)), DropZone::Right);
    }

    #[test]
    fn pane_boxes_step_inside_interior_leading_edges_only() {
        let divider = 8.0;
        let corner = PaneRect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 800.0,
        };
        assert_eq!(pane_box(corner, divider), corner);
        let interior = PaneRect {
            x: 400.0,
            y: 400.0,
            width: 600.0,
            height: 400.0,
        };
        assert_eq!(
            pane_box(interior, divider),
            PaneRect {
                x: 408.0,
                y: 408.0,
                width: 592.0,
                height: 392.0
            }
        );

        let pane = pane_box(interior, divider);
        let left = drop_preview_bounds(interior, DropZone::Left, divider);
        let right = drop_preview_bounds(interior, DropZone::Right, divider);
        assert_eq!((left.x, left.y), (pane.x, pane.y));
        assert_eq!(left.width, 296.0);
        assert_eq!(right.x, 712.0);
        assert_eq!(right.width, 288.0);
        assert_eq!(right.x + right.width, pane.x + pane.width);
        assert_eq!(
            drop_preview_bounds(interior, DropZone::Center, divider),
            pane
        );
    }

    #[test]
    fn a_drop_that_rebuilds_the_same_arrangement_collapses_into_a_swap() {
        let pair = LayoutNode::Split {
            id: SplitId(1),
            axis: Axis::Horizontal,
            ratio: 0.35,
            first: Box::new(LayoutNode::Pane(PaneId(3))),
            second: Box::new(LayoutNode::Pane(PaneId(7))),
        };
        assert_eq!(
            coerced_drop_zone(&pair, PaneId(3), PaneId(7), DropZone::Left),
            DropZone::Center
        );
        assert_eq!(
            coerced_drop_zone(&pair, PaneId(7), PaneId(3), DropZone::Right),
            DropZone::Center
        );
        let stack = LayoutNode::Split {
            id: SplitId(1),
            axis: Axis::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane(PaneId(3))),
            second: Box::new(LayoutNode::Pane(PaneId(7))),
        };
        assert_eq!(
            coerced_drop_zone(&stack, PaneId(3), PaneId(7), DropZone::Top),
            DropZone::Center
        );
        assert_eq!(
            coerced_drop_zone(&pair, PaneId(3), PaneId(7), DropZone::Right),
            DropZone::Right
        );
        assert_eq!(
            coerced_drop_zone(&pair, PaneId(3), PaneId(7), DropZone::Top),
            DropZone::Top
        );
        assert_eq!(
            coerced_drop_zone(&stack, PaneId(3), PaneId(7), DropZone::Left),
            DropZone::Left
        );

        let layout = three_pane_layout();
        assert_eq!(
            coerced_drop_zone(&layout, PaneId(3), PaneId(7), DropZone::Left),
            DropZone::Left
        );
        assert_eq!(
            coerced_drop_zone(&layout, PaneId(3), PaneId(9), DropZone::Left),
            DropZone::Left
        );
        assert_eq!(
            coerced_drop_zone(&layout, PaneId(7), PaneId(9), DropZone::Top),
            DropZone::Center
        );
    }

    #[test]
    fn pane_drops_swap_at_the_center_and_join_at_every_edge() {
        let expected_swap = CommandInvocation::new("swap-pane", ["-d", "-s", "%3", "-t", "%7"]);
        assert_eq!(pane_swap_command(PaneId(3), PaneId(7)), expected_swap);
        assert_eq!(
            pane_drop_command(PaneId(3), PaneId(7), DropZone::Center),
            Some(expected_swap)
        );
        assert_eq!(
            pane_drop_command(PaneId(3), PaneId(3), DropZone::Left),
            None
        );

        for (zone, flags) in [
            (DropZone::Left, vec!["-d", "-b", "-h"]),
            (DropZone::Right, vec!["-d", "-h"]),
            (DropZone::Top, vec!["-d", "-b", "-v"]),
            (DropZone::Bottom, vec!["-d", "-v"]),
        ] {
            let mut args = flags;
            args.extend(["-s", "%3", "-t", "%7"]);
            assert_eq!(
                pane_drop_command(PaneId(3), PaneId(7), zone),
                Some(CommandInvocation::new("join-pane", args))
            );
        }
    }

    #[test]
    fn predicted_drops_reuse_the_daemon_layout_transforms() {
        let layout = three_pane_layout();

        assert_eq!(
            predicted_drop_layout(&layout, PaneId(3), PaneId(9), DropZone::Center),
            Some(swapped_layout(&layout, PaneId(3), PaneId(9)))
        );
        assert_eq!(
            predicted_drop_layout(&layout, PaneId(3), PaneId(9), DropZone::Top),
            joined_layout(
                &layout,
                PaneId(3),
                PaneId(9),
                OPTIMISTIC_SPLIT,
                Axis::Vertical,
                0.5,
                true,
            )
        );
        assert_eq!(
            predicted_drop_layout(&layout, PaneId(3), PaneId(9), DropZone::Right),
            joined_layout(
                &layout,
                PaneId(3),
                PaneId(9),
                OPTIMISTIC_SPLIT,
                Axis::Horizontal,
                0.5,
                false,
            )
        );
        assert_eq!(
            predicted_drop_layout(&layout, PaneId(3), PaneId(9), DropZone::Bottom),
            Some(LayoutNode::Split {
                id: SplitId(2),
                axis: Axis::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(PaneId(7))),
                second: Box::new(LayoutNode::Split {
                    id: OPTIMISTIC_SPLIT,
                    axis: Axis::Vertical,
                    ratio: 0.5,
                    first: Box::new(LayoutNode::Pane(PaneId(9))),
                    second: Box::new(LayoutNode::Pane(PaneId(3))),
                }),
            })
        );
    }
}
