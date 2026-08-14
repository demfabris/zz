use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};
use zz_protocol::{Axis, LayoutNode, PaneId};

/// Gap left between neighbouring panes; the grid's own background shows
/// through it and reads as the split divider.
const DIVIDER: f32 = 2.0;

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

mod imp {
    use std::cell::RefCell;

    use gtk::{glib, prelude::*, subclass::prelude::*};
    use zz_protocol::{LayoutNode, PaneId};

    #[derive(Default)]
    pub struct PaneGrid {
        pub layout: RefCell<Option<LayoutNode>>,
        pub children: RefCell<Vec<(PaneId, gtk::Widget)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PaneGrid {
        const NAME: &'static str = "ZzPaneGrid";
        type Type = super::PaneGrid;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for PaneGrid {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for PaneGrid {
        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            self.obj().allocate_panes(width, height);
        }
    }
}

glib::wrapper! {
    /// Lays every pane of the focused window out at the geometry the daemon's
    /// snapshot describes: the split tree is the layout, ratios and all, so the
    /// client never invents geometry of its own.
    pub struct PaneGrid(ObjectSubclass<imp::PaneGrid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for PaneGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneGrid {
    pub fn new() -> Self {
        let grid: Self = glib::Object::new();
        grid.add_css_class("zz-panes");
        grid.set_hexpand(true);
        grid.set_vexpand(true);
        grid
    }

    /// Replace the layout and its widgets. Children absent from `panes` are
    /// unparented, so every remaining child is one the layout allocates.
    pub fn set_panes(&self, layout: LayoutNode, panes: Vec<(PaneId, gtk::Widget)>) {
        let imp = self.imp();
        for (_, widget) in imp.children.borrow().iter() {
            if !panes.iter().any(|(_, next)| next == widget) {
                widget.unparent();
            }
        }
        for (_, widget) in &panes {
            if widget.parent().as_ref() != Some(self.upcast_ref::<gtk::Widget>()) {
                widget.set_parent(self);
            }
        }
        imp.children.replace(panes);
        imp.layout.replace(Some(layout));
        self.queue_allocate();
    }

    fn allocate_panes(&self, width: i32, height: i32) {
        let imp = self.imp();
        let Some(layout) = imp.layout.borrow().clone() else {
            return;
        };
        self.allocate_node(
            &layout,
            Rect {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
        );
    }

    fn allocate_node(&self, node: &LayoutNode, rect: Rect) {
        match node {
            LayoutNode::Pane(pane) => {
                let Some(widget) = self.widget_for(*pane) else {
                    return;
                };
                widget.allocate(
                    rect.width.max(0.0).round() as i32,
                    rect.height.max(0.0).round() as i32,
                    -1,
                    Some(gsk::Transform::new().translate(&graphene::Point::new(rect.x, rect.y))),
                );
            }
            LayoutNode::Split {
                axis,
                ratio,
                first,
                second,
                ..
            } => {
                let (first_rect, second_rect) = split(rect, *axis, *ratio);
                self.allocate_node(first, first_rect);
                self.allocate_node(second, second_rect);
            }
        }
    }

    fn widget_for(&self, pane: PaneId) -> Option<gtk::Widget> {
        self.imp()
            .children
            .borrow()
            .iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, widget)| widget.clone())
    }
}

fn split(rect: Rect, axis: Axis, ratio: f32) -> (Rect, Rect) {
    let ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.5
    };
    match axis {
        Axis::Horizontal => {
            let available = (rect.width - DIVIDER).max(0.0);
            let first = (available * ratio).round();
            (
                Rect {
                    width: first,
                    ..rect
                },
                Rect {
                    x: rect.x + first + DIVIDER,
                    width: available - first,
                    ..rect
                },
            )
        }
        Axis::Vertical => {
            let available = (rect.height - DIVIDER).max(0.0);
            let first = (available * ratio).round();
            (
                Rect {
                    height: first,
                    ..rect
                },
                Rect {
                    y: rect.y + first + DIVIDER,
                    height: available - first,
                    ..rect
                },
            )
        }
    }
}

/// Every pane the layout actually places, in tree order.
pub fn layout_panes(node: &LayoutNode) -> Vec<PaneId> {
    let mut panes = Vec::new();
    collect(node, &mut panes);
    panes
}

fn collect(node: &LayoutNode, panes: &mut Vec<PaneId>) {
    match node {
        LayoutNode::Pane(pane) => panes.push(*pane),
        LayoutNode::Split { first, second, .. } => {
            collect(first, panes);
            collect(second, panes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_protocol::SplitId;

    #[test]
    fn a_horizontal_split_reserves_the_divider_between_the_halves() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 102.0,
            height: 40.0,
        };

        let (first, second) = split(rect, Axis::Horizontal, 0.5);

        assert_eq!(first.width, 50.0);
        assert_eq!(second.x, 52.0);
        assert_eq!(second.width, 50.0);
    }

    #[test]
    fn a_non_finite_ratio_falls_back_to_an_even_split() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 82.0,
        };

        let (first, second) = split(rect, Axis::Vertical, f32::NAN);

        assert_eq!(first.height, 40.0);
        assert_eq!(second.y, 42.0);
    }

    #[test]
    fn layout_traversal_lists_panes_in_tree_order() {
        let layout = LayoutNode::Split {
            id: SplitId(1),
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane(PaneId(7))),
            second: Box::new(LayoutNode::Split {
                id: SplitId(2),
                axis: Axis::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(PaneId(8))),
                second: Box::new(LayoutNode::Pane(PaneId(9))),
            }),
        };

        assert_eq!(layout_panes(&layout), vec![PaneId(7), PaneId(8), PaneId(9)]);
    }
}
