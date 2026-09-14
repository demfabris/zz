use crate::{Axis, LayoutNode, PaneId, SplitId};

#[must_use]
pub fn swapped_layout(layout: &LayoutNode, source: PaneId, target: PaneId) -> LayoutNode {
    let mut layout = layout.clone();
    predict_swap_layout_panes(&mut layout, source, target);
    layout
}

#[must_use]
pub fn joined_layout(
    layout: &LayoutNode,
    source: PaneId,
    target: PaneId,
    split: SplitId,
    axis: Axis,
    pane_ratio: f32,
    before: bool,
) -> Option<LayoutNode> {
    if source == target {
        return None;
    }
    let mut layout = layout.clone();
    if !predict_remove_layout_leaf(&mut layout, source) {
        return None;
    }
    predict_insert_layout_pane(
        &mut layout,
        target,
        source,
        split,
        axis,
        pane_ratio,
        before,
        false,
    )
    .then_some(layout)
}

fn predict_insert_layout_pane(
    node: &mut LayoutNode,
    target: PaneId,
    pane: PaneId,
    split: SplitId,
    axis: Axis,
    pane_ratio: f32,
    before: bool,
    full_size: bool,
) -> bool {
    if full_size {
        if !node.contains(target) {
            return false;
        }
        let existing = std::mem::replace(node, LayoutNode::Pane(pane));
        let (ratio, first, second) = if before {
            (
                pane_ratio,
                Box::new(LayoutNode::Pane(pane)),
                Box::new(existing),
            )
        } else {
            (
                1.0 - pane_ratio,
                Box::new(existing),
                Box::new(LayoutNode::Pane(pane)),
            )
        };
        *node = LayoutNode::Split {
            id: split,
            axis,
            ratio,
            first,
            second,
        };
        return true;
    }
    match node {
        LayoutNode::Pane(candidate) if *candidate == target => {
            let existing = LayoutNode::Pane(target);
            let (ratio, first, second) = if before {
                (
                    pane_ratio,
                    Box::new(LayoutNode::Pane(pane)),
                    Box::new(existing),
                )
            } else {
                (
                    1.0 - pane_ratio,
                    Box::new(existing),
                    Box::new(LayoutNode::Pane(pane)),
                )
            };
            *node = LayoutNode::Split {
                id: split,
                axis,
                ratio,
                first,
                second,
            };
            true
        }
        LayoutNode::Pane(_) => false,
        LayoutNode::Split { first, second, .. } => {
            predict_insert_layout_pane(first, target, pane, split, axis, pane_ratio, before, false)
                || predict_insert_layout_pane(
                    second, target, pane, split, axis, pane_ratio, before, false,
                )
        }
    }
}

fn predict_remove_layout_leaf(node: &mut LayoutNode, target: PaneId) -> bool {
    let promote_second = match node {
        LayoutNode::Pane(_) => return false,
        LayoutNode::Split { first, .. } if matches!(first.as_ref(), LayoutNode::Pane(pane) if *pane == target) => {
            Some(true)
        }
        LayoutNode::Split { second, .. } if matches!(second.as_ref(), LayoutNode::Pane(pane) if *pane == target) => {
            Some(false)
        }
        LayoutNode::Split { .. } => None,
    };

    if let Some(promote_second) = promote_second {
        let removed = std::mem::replace(node, LayoutNode::Pane(target));
        let LayoutNode::Split { first, second, .. } = removed else {
            unreachable!("only split nodes can promote a sibling")
        };
        *node = if promote_second { *second } else { *first };
        return true;
    }

    let LayoutNode::Split { first, second, .. } = node else {
        unreachable!("pane nodes return before recursive removal")
    };
    predict_remove_layout_leaf(first, target) || predict_remove_layout_leaf(second, target)
}

fn predict_swap_layout_panes(node: &mut LayoutNode, source: PaneId, target: PaneId) {
    match node {
        LayoutNode::Pane(pane) if *pane == source => *pane = target,
        LayoutNode::Pane(pane) if *pane == target => *pane = source,
        LayoutNode::Pane(_) => {}
        LayoutNode::Split { first, second, .. } => {
            predict_swap_layout_panes(first, source, target);
            predict_swap_layout_panes(second, source, target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joining_a_pane_to_itself_has_no_prediction() {
        let first = PaneId(1);
        let layout = LayoutNode::Split {
            id: SplitId(1),
            axis: Axis::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane(PaneId(2))),
            second: Box::new(LayoutNode::Pane(first)),
        };
        assert_eq!(
            joined_layout(
                &layout,
                first,
                first,
                SplitId(u64::MAX),
                Axis::Vertical,
                0.5,
                true
            ),
            None
        );
    }
}
