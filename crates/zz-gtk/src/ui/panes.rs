use std::sync::Arc;

use gtk::{gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};
use zz_protocol::{Axis, LayoutNode, PaneId, SplitId, WindowId};

use crate::engine::{Engine, MAX_SPLIT_RATIO, MIN_SPLIT_RATIO};

/// Gap left between neighbouring panes; the grid's own background shows
/// through it and reads as the split divider.
const DIVIDER: f32 = 2.0;
/// The band that grabs a divider is far wider than the divider itself, matching
/// the desktop's 16 logical pixels.
const HIT_THICKNESS: f32 = 16.0;

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// One draggable boundary: which split it commits to, which way it moves, and
/// the extent the pointer's position is measured against.
#[derive(Clone, Copy)]
pub struct Boundary {
    split: SplitId,
    axis: Axis,
    hit: Rect,
    surface: Rect,
}

impl Boundary {
    /// The ratio the pointer names, measured over the split's whole extent —
    /// divider slot included — exactly as the desktop measures it.
    fn ratio_at(self, x: f32, y: f32) -> f32 {
        let (offset, extent) = match self.axis {
            Axis::Horizontal => (x - self.surface.x, self.surface.width),
            Axis::Vertical => (y - self.surface.y, self.surface.height),
        };
        if extent <= 0.0 {
            return 0.5;
        }
        (offset / extent).clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
    }
}

mod imp {
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
        sync::Arc,
    };

    use gtk::{glib, prelude::*, subclass::prelude::*};
    use zz_protocol::{LayoutNode, PaneId, SplitId, WindowId};

    use super::Boundary;
    use crate::engine::Engine;

    #[derive(Default)]
    pub struct PaneGrid {
        pub layout: RefCell<Option<LayoutNode>>,
        pub children: RefCell<Vec<(PaneId, gtk::Widget)>>,
        pub engine: RefCell<Option<Arc<Engine>>>,
        pub window: Cell<Option<WindowId>>,
        pub zoomed: Cell<bool>,
        pub boundaries: RefCell<Vec<Boundary>>,
        /// The split under the pointer while a drag is live, plus the ratio the
        /// grid is previewing for it. The daemon is only told on release.
        pub dragging: Cell<Option<SplitId>>,
        pub preview: RefCell<HashMap<SplitId, f32>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PaneGrid {
        const NAME: &'static str = "ZzPaneGrid";
        type Type = super::PaneGrid;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for PaneGrid {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().install_controllers();
        }

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
    /// client never invents geometry of its own — except while a divider is
    /// being dragged, where it previews a ratio it has not committed yet.
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

    pub fn set_engine(&self, engine: Arc<Engine>) {
        self.imp().engine.replace(Some(engine));
    }

    /// Replace the layout and its widgets. Children absent from `panes` are
    /// unparented, so every remaining child is one the layout allocates.
    pub fn set_panes(
        &self,
        window: WindowId,
        layout: LayoutNode,
        zoomed: bool,
        panes: Vec<(PaneId, gtk::Widget)>,
    ) {
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
        // A preview only survives while the snapshot still disagrees with it;
        // once the daemon echoes the ratio back there is nothing left to hold.
        imp.preview.borrow_mut().retain(|split, ratio| {
            snapshot_ratio(&layout, *split).is_some_and(|current| !same_ratio(current, *ratio))
        });
        imp.window.set(Some(window));
        imp.zoomed.set(zoomed);
        imp.children.replace(panes);
        imp.layout.replace(Some(layout));
        self.queue_allocate();
    }

    fn allocate_panes(&self, width: i32, height: i32) {
        let imp = self.imp();
        let Some(layout) = imp.layout.borrow().clone() else {
            return;
        };
        let surface = Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        };
        let mut boundaries = Vec::new();
        let preview = imp.preview.borrow().clone();
        self.allocate_node(&layout, surface, &preview, &mut boundaries);
        imp.boundaries.replace(boundaries);
    }

    fn allocate_node(
        &self,
        node: &LayoutNode,
        rect: Rect,
        preview: &std::collections::HashMap<SplitId, f32>,
        boundaries: &mut Vec<Boundary>,
    ) {
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
                id,
                axis,
                ratio,
                first,
                second,
            } => {
                let ratio = preview.get(id).copied().unwrap_or(*ratio);
                let (first_rect, second_rect) = split(rect, *axis, ratio);
                boundaries.push(Boundary {
                    split: *id,
                    axis: *axis,
                    hit: hit_band(rect, *axis, first_rect),
                    surface: rect,
                });
                self.allocate_node(first, first_rect, preview, boundaries);
                self.allocate_node(second, second_rect, preview, boundaries);
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

/// Divider dragging. The preview is local and the commit is one message, so a
/// drag costs nothing on the wire until the pointer comes up.
impl PaneGrid {
    fn install_controllers(&self) {
        let drag = gtk::GestureDrag::new();
        let start = self.downgrade();
        drag.connect_drag_begin(move |gesture, x, y| {
            if let Some(grid) = start.upgrade() {
                grid.on_drag_begin(gesture, x, y);
            }
        });
        let update = self.downgrade();
        drag.connect_drag_update(move |gesture, _, _| {
            if let Some(grid) = update.upgrade() {
                grid.on_drag_update(gesture);
            }
        });
        let end = self.downgrade();
        drag.connect_drag_end(move |gesture, _, _| {
            if let Some(grid) = end.upgrade() {
                grid.on_drag_end(gesture);
            }
        });
        self.add_controller(drag);

        let motion = gtk::EventControllerMotion::new();
        let hover = self.downgrade();
        motion.connect_motion(move |_, x, y| {
            if let Some(grid) = hover.upgrade() {
                grid.set_resize_cursor(grid.boundary_at(x as f32, y as f32).map(|b| b.axis));
            }
        });
        let leave = self.downgrade();
        motion.connect_leave(move |_| {
            if let Some(grid) = leave.upgrade() {
                grid.set_resize_cursor(None);
            }
        });
        self.add_controller(motion);
    }

    fn boundary_at(&self, x: f32, y: f32) -> Option<Boundary> {
        if self.imp().zoomed.get() {
            return None;
        }
        self.imp()
            .boundaries
            .borrow()
            .iter()
            .find(|boundary| boundary.hit.contains(x, y))
            .copied()
    }

    fn set_resize_cursor(&self, axis: Option<Axis>) {
        let cursor = match axis {
            Some(Axis::Horizontal) => gdk::Cursor::from_name("col-resize", None),
            Some(Axis::Vertical) => gdk::Cursor::from_name("row-resize", None),
            None => None,
        };
        self.set_cursor(cursor.as_ref());
    }

    fn on_drag_begin(&self, gesture: &gtk::GestureDrag, x: f64, y: f64) {
        let Some(boundary) = self.boundary_at(x as f32, y as f32) else {
            return;
        };
        self.imp().dragging.set(Some(boundary.split));
        gesture.set_state(gtk::EventSequenceState::Claimed);
    }

    fn on_drag_update(&self, gesture: &gtk::GestureDrag) {
        let Some(split) = self.imp().dragging.get() else {
            return;
        };
        let Some((start_x, start_y)) = gesture.start_point() else {
            return;
        };
        let (dx, dy) = gesture.offset().unwrap_or((0.0, 0.0));
        let Some(boundary) = self
            .imp()
            .boundaries
            .borrow()
            .iter()
            .find(|boundary| boundary.split == split)
            .copied()
        else {
            return;
        };
        let ratio = boundary.ratio_at((start_x + dx) as f32, (start_y + dy) as f32);
        self.imp().preview.borrow_mut().insert(split, ratio);
        self.queue_allocate();
    }

    /// Commit exactly once, and only when the drag actually moved the split at
    /// the precision the wire carries.
    fn on_drag_end(&self, gesture: &gtk::GestureDrag) {
        let Some(split) = self.imp().dragging.replace(None) else {
            return;
        };
        gesture.set_state(gtk::EventSequenceState::Denied);
        let ratio = self.imp().preview.borrow().get(&split).copied();
        let (Some(ratio), Some(window)) = (ratio, self.imp().window.get()) else {
            return;
        };
        let unchanged = self
            .imp()
            .layout
            .borrow()
            .as_ref()
            .and_then(|layout| snapshot_ratio(layout, split))
            .is_some_and(|current| same_ratio(current, ratio));
        if unchanged {
            self.imp().preview.borrow_mut().remove(&split);
            return;
        }
        if let Some(engine) = self.imp().engine.borrow().clone() {
            engine.resize_split(window, split, ratio);
        }
    }
}

/// The band that grabs a divider, centred on the gap between the two halves.
fn hit_band(rect: Rect, axis: Axis, first: Rect) -> Rect {
    let inset = (HIT_THICKNESS - DIVIDER) / 2.0;
    match axis {
        Axis::Horizontal => Rect {
            x: rect.x + first.width - inset,
            width: HIT_THICKNESS,
            ..rect
        },
        Axis::Vertical => Rect {
            y: rect.y + first.height - inset,
            height: HIT_THICKNESS,
            ..rect
        },
    }
}

fn resolved_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// Ratios are compared at the precision `ResizeSplit` carries, so a drag that
/// rounds onto the value the daemon already has is a no-op.
fn same_ratio(left: f32, right: f32) -> bool {
    basis_points(left) == basis_points(right)
}

fn basis_points(ratio: f32) -> u16 {
    (resolved_ratio(ratio) * f32::from(zz_protocol::SPLIT_RATIO_BASIS)).round() as u16
}

fn snapshot_ratio(node: &LayoutNode, split: SplitId) -> Option<f32> {
    match node {
        LayoutNode::Pane(_) => None,
        LayoutNode::Split {
            id,
            ratio,
            first,
            second,
            ..
        } => {
            if *id == split {
                return Some(*ratio);
            }
            snapshot_ratio(first, split).or_else(|| snapshot_ratio(second, split))
        }
    }
}

fn split(rect: Rect, axis: Axis, ratio: f32) -> (Rect, Rect) {
    let ratio = resolved_ratio(ratio);
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

    fn tree() -> LayoutNode {
        LayoutNode::Split {
            id: SplitId(1),
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane(PaneId(7))),
            second: Box::new(LayoutNode::Split {
                id: SplitId(2),
                axis: Axis::Vertical,
                ratio: 0.25,
                first: Box::new(LayoutNode::Pane(PaneId(8))),
                second: Box::new(LayoutNode::Pane(PaneId(9))),
            }),
        }
    }

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
        assert_eq!(layout_panes(&tree()), vec![PaneId(7), PaneId(8), PaneId(9)]);
    }

    /// The band has to straddle the gap, or the divider is only grabbable from
    /// one of the two panes it separates.
    #[test]
    fn the_grab_band_straddles_the_gap_it_sits_in() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 102.0,
            height: 40.0,
        };
        let (first, _) = split(rect, Axis::Horizontal, 0.5);

        let band = hit_band(rect, Axis::Horizontal, first);

        assert_eq!(band.width, HIT_THICKNESS);
        assert!(band.contains(50.0, 20.0), "the near pane's edge");
        assert!(band.contains(56.0, 20.0), "the far pane's edge");
        assert!(!band.contains(40.0, 20.0));
        assert!(!band.contains(64.0, 20.0));
    }

    #[test]
    fn a_pointer_names_a_ratio_over_the_whole_split_and_never_past_the_clamp() {
        let boundary = Boundary {
            split: SplitId(1),
            axis: Axis::Horizontal,
            hit: Rect {
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 40.0,
            },
            surface: Rect {
                x: 10.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        };

        assert_eq!(boundary.ratio_at(35.0, 20.0), 0.25);
        assert_eq!(boundary.ratio_at(0.0, 20.0), MIN_SPLIT_RATIO);
        assert_eq!(boundary.ratio_at(999.0, 20.0), MAX_SPLIT_RATIO);
    }

    #[test]
    fn a_drag_that_lands_on_the_daemons_own_ratio_is_not_worth_a_message() {
        assert!(same_ratio(0.5, 0.500_004));
        assert!(!same_ratio(0.5, 0.51));
        assert_eq!(basis_points(0.25), 2_500);
        assert_eq!(snapshot_ratio(&tree(), SplitId(2)), Some(0.25));
        assert_eq!(snapshot_ratio(&tree(), SplitId(9)), None);
    }
}
