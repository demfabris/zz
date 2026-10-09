#![allow(dead_code)]

mod json;

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use zz_protocol::{Axis, LayoutNode, PaneBorderStatus, PaneId, SplitId};

use crate::{PresetOptions, model::LayoutPreset};

use json::Json;

pub(crate) const PANE_MINIMUM: u16 = 1;
pub(crate) const PANE_MAXIMUM: u16 = 10_000;
const WINDOW_MAXIMUM: i64 = 10_000;
const MAX_LAYOUT_DEPTH: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LayoutFormat {
    V1,
    #[default]
    V2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LeafState {
    pub active: bool,
    pub last: Option<u32>,
    pub index: u32,
    pub z: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ParsedFloat {
    pub z: u32,
    pub xoff: i32,
    pub yoff: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct ParsedLeaf {
    pub index: u32,
    pub active: bool,
    pub last: Option<u32>,
    pub float: Option<ParsedFloat>,
    pub id: Option<u64>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct ParsedSelection {
    pub active: Option<PaneId>,
    pub last_panes: Vec<PaneId>,
    pub floats: Vec<PaneId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct CellGeometry {
    pub sx: u16,
    pub sy: u16,
    pub xoff: i32,
    pub yoff: i32,
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum CellNode {
    Leaf {
        pane: PaneId,
        geometry: CellGeometry,
        saved: Option<CellGeometry>,
    },
    Float {
        pane: PaneId,
        geometry: CellGeometry,
    },
    Node {
        axis: Axis,
        geometry: CellGeometry,
        children: Vec<CellChild>,
    },
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct CellChild {
    pub divider: Option<SplitId>,
    pub node: CellNode,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CellLayout {
    root: CellNode,
    extent: (u16, u16),
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ParsedLayout {
    root: ParsedNode,
    version: u8,
}

#[derive(Clone, PartialEq, Debug)]
enum ParsedNode {
    Leaf {
        geometry: CellGeometry,
        leaf: ParsedLeaf,
    },
    Node {
        axis: Axis,
        geometry: CellGeometry,
        children: Vec<ParsedNode>,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct LayoutParseError(String);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LayoutError {
    Floating,
    LastPane,
    NoSpace,
    UnknownDivider,
    UnknownPane,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitSize {
    Default,
    Cells(u16),
    Percent(u8),
}

impl CellGeometry {
    fn extent(self, axis: Axis) -> u16 {
        match axis {
            Axis::Horizontal => self.sx,
            Axis::Vertical => self.sy,
        }
    }

    fn set_extent(&mut self, axis: Axis, size: u16) {
        match axis {
            Axis::Horizontal => self.sx = size,
            Axis::Vertical => self.sy = size,
        }
    }
}

impl CellNode {
    fn geometry(&self) -> CellGeometry {
        match self {
            Self::Leaf { geometry, .. }
            | Self::Float { geometry, .. }
            | Self::Node { geometry, .. } => *geometry,
        }
    }

    fn geometry_mut(&mut self) -> &mut CellGeometry {
        match self {
            Self::Leaf { geometry, .. }
            | Self::Float { geometry, .. }
            | Self::Node { geometry, .. } => geometry,
        }
    }

    fn tiled(&self) -> bool {
        match self {
            Self::Leaf { .. } => true,
            Self::Float { .. } => false,
            Self::Node { children, .. } => children.iter().any(|child| child.node.tiled()),
        }
    }

    const fn pane(&self) -> Option<PaneId> {
        match self {
            Self::Leaf { pane, .. } | Self::Float { pane, .. } => Some(*pane),
            Self::Node { .. } => None,
        }
    }

    fn extent(&self, axis: Axis) -> u16 {
        self.geometry().extent(axis)
    }

    fn set_extent(&mut self, axis: Axis, size: u16) {
        self.geometry_mut().set_extent(axis, size);
    }
}

impl ParsedNode {
    fn geometry(&self) -> CellGeometry {
        match self {
            Self::Leaf { geometry, .. } | Self::Node { geometry, .. } => *geometry,
        }
    }

    fn geometry_mut(&mut self) -> &mut CellGeometry {
        match self {
            Self::Leaf { geometry, .. } | Self::Node { geometry, .. } => geometry,
        }
    }

    fn tiled(&self) -> bool {
        match self {
            Self::Leaf { leaf, .. } => leaf.float.is_none(),
            Self::Node { children, .. } => children.iter().any(Self::tiled),
        }
    }

    fn extent(&self, axis: Axis) -> u16 {
        self.geometry().extent(axis)
    }

    fn set_extent(&mut self, axis: Axis, size: u16) {
        self.geometry_mut().set_extent(axis, size);
    }
}

impl LayoutParseError {
    fn new(cause: impl Into<String>) -> Self {
        Self(cause.into())
    }

    pub(crate) fn cause(&self) -> &str {
        &self.0
    }
}

impl CellLayout {
    pub(crate) fn new(pane: PaneId, sx: u16, sy: u16) -> Self {
        let sx = sx.clamp(PANE_MINIMUM, PANE_MAXIMUM);
        let sy = sy.clamp(PANE_MINIMUM, PANE_MAXIMUM);
        let layout = Self {
            root: leaf(pane, sx, sy),
            extent: (sx, sy),
        };
        layout.debug_validate();
        layout
    }

    pub(crate) fn parse(input: &str) -> Result<ParsedLayout, LayoutParseError> {
        let input = input.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
        if input.starts_with('{') {
            parse_v2(&json::parse(input).map_err(LayoutParseError)?)
        } else {
            parse_v1(input.as_bytes())
        }
    }

    pub(crate) fn extent(&self) -> (u16, u16) {
        if !self.root.tiled() {
            return self.extent;
        }
        let geometry = self.root.geometry();
        (geometry.sx, geometry.sy)
    }

    pub(crate) fn has_tiled(&self) -> bool {
        self.root.tiled()
    }

    pub(crate) fn is_floating(&self, pane: PaneId) -> bool {
        pane_cell(&self.root, pane).is_some_and(|node| matches!(node, CellNode::Float { .. }))
    }

    pub(crate) fn floating_panes(&self) -> Vec<PaneId> {
        let mut panes = Vec::new();
        collect_panes(&self.root, &mut panes);
        panes.retain(|pane| self.is_floating(*pane));
        panes
    }

    pub(crate) fn tiled_panes(&self) -> Vec<PaneId> {
        let mut panes = Vec::new();
        collect_panes(&self.root, &mut panes);
        panes.retain(|pane| !self.is_floating(*pane));
        panes
    }

    pub(crate) fn saved_float(&self, pane: PaneId) -> Option<CellGeometry> {
        match pane_cell(&self.root, pane)? {
            CellNode::Leaf { saved, .. } => *saved,
            CellNode::Float { .. } | CellNode::Node { .. } => None,
        }
    }

    pub(crate) fn panes_in_order(&self) -> Vec<PaneId> {
        let mut panes = Vec::new();
        collect_panes(&self.root, &mut panes);
        panes
    }

    pub(crate) fn pane_geometry(&self, pane: PaneId) -> Option<CellGeometry> {
        pane_geometry(&self.root, pane)
    }

    /// The cell geometry with the `pane-border-status` row carved out of it,
    /// the way `layout_fix_panes` calls `layout_add_horizontal_border`: only a
    /// cell whose top edge is the root's top edge takes the `top` row (`yoff++`
    /// and `sy--`), only a cell whose bottom edge is the root's bottom edge
    /// takes the `bottom` row (`sy--`), and a one-row cell keeps its row.
    pub(crate) fn pane_geometry_with_border(
        &self,
        pane: PaneId,
        status: PaneBorderStatus,
    ) -> Option<CellGeometry> {
        let geometry = pane_geometry(&self.root, pane)?;
        if self.is_floating(pane) {
            return Some(geometry);
        }
        Some(carve_border_row(geometry, self.root.geometry(), status))
    }

    pub(crate) fn contains(&self, pane: PaneId) -> bool {
        self.pane_geometry(pane).is_some()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn split(
        &mut self,
        target: PaneId,
        axis: Axis,
        size: SplitSize,
        before: bool,
        full: bool,
        new_pane: PaneId,
        ids: &mut dyn FnMut() -> SplitId,
    ) -> Result<(), LayoutError> {
        let target_path = pane_path(&self.root, target).ok_or(LayoutError::UnknownPane)?;
        if self.is_floating(target) {
            return Err(LayoutError::Floating);
        }
        let split_path = if full {
            &[][..]
        } else {
            target_path.as_slice()
        };
        let split_cell = node_at_path(&self.root, split_path).ok_or(LayoutError::UnknownPane)?;
        let span = split_cell.extent(axis);
        if span < PANE_MINIMUM * 2 + 1 {
            return Err(LayoutError::NoSpace);
        }

        let requested = match size {
            SplitSize::Default => None,
            SplitSize::Cells(cells) => Some(cells),
            SplitSize::Percent(percent) => {
                let percent = percent.min(100);
                Some(((u32::from(span) * u32::from(percent)) / 100) as u16)
            }
        };
        let mut second = match requested {
            None => span.div_ceil(2) - 1,
            Some(cells) if before => span.saturating_sub(cells).saturating_sub(1),
            Some(cells) => cells,
        };
        second = second.clamp(PANE_MINIMUM, span - 2);
        let first = span - 1 - second;
        let old_size = if before { second } else { first };

        if full && !can_set_size(&self.root, axis, old_size) {
            return Err(LayoutError::NoSpace);
        }

        if !full && !target_path.is_empty() {
            let parent_path = &target_path[..target_path.len() - 1];
            let index = target_path[target_path.len() - 1];
            let same_axis = matches!(
                node_at_path(&self.root, parent_path),
                Some(CellNode::Node { axis: parent_axis, .. }) if *parent_axis == axis
            );
            if same_axis {
                let parent = node_at_path_mut(&mut self.root, parent_path)
                    .ok_or(LayoutError::UnknownPane)?;
                let CellNode::Node { children, .. } = parent else {
                    return Err(LayoutError::UnknownPane);
                };
                let divider = ids();
                let target_geometry = children[index].node.geometry();
                if before {
                    resize_node_to(&mut children[index].node, axis, second);
                    let inherited = children[index].divider;
                    children[index].divider = Some(divider);
                    let mut geometry = target_geometry;
                    geometry.set_extent(axis, first);
                    children.insert(
                        index,
                        CellChild {
                            divider: inherited,
                            node: CellNode::Leaf {
                                pane: new_pane,
                                geometry,
                                saved: None,
                            },
                        },
                    );
                } else {
                    resize_node_to(&mut children[index].node, axis, first);
                    let mut geometry = target_geometry;
                    geometry.set_extent(axis, second);
                    children.insert(
                        index + 1,
                        CellChild {
                            divider: Some(divider),
                            node: CellNode::Leaf {
                                pane: new_pane,
                                geometry,
                                saved: None,
                            },
                        },
                    );
                }
                fix_offsets(&mut self.root);
                self.debug_validate();
                return Ok(());
            }
        }

        let root_same_axis = matches!(
            &self.root,
            CellNode::Node { axis: root_axis, .. } if *root_axis == axis
        );
        if full && root_same_axis {
            let divider = ids();
            let root_geometry = self.root.geometry();
            self.root.set_extent(axis, old_size);
            resize_child_cells(&mut self.root);
            self.root.set_extent(axis, span);
            let mut geometry = root_geometry;
            geometry.set_extent(axis, if before { first } else { second });
            let CellNode::Node { children, .. } = &mut self.root else {
                return Err(LayoutError::UnknownPane);
            };
            if before {
                children[0].divider = Some(divider);
                children.insert(
                    0,
                    CellChild {
                        divider: None,
                        node: CellNode::Leaf {
                            pane: new_pane,
                            geometry,
                            saved: None,
                        },
                    },
                );
            } else {
                children.push(CellChild {
                    divider: Some(divider),
                    node: CellNode::Leaf {
                        pane: new_pane,
                        geometry,
                        saved: None,
                    },
                });
            }
            fix_offsets(&mut self.root);
            self.debug_validate();
            return Ok(());
        }

        let cell = node_at_path_mut(&mut self.root, split_path).ok_or(LayoutError::UnknownPane)?;
        let divider = ids();
        let geometry = cell.geometry();
        let placeholder = CellNode::Leaf {
            pane: new_pane,
            geometry,
            saved: None,
        };
        let mut old = std::mem::replace(cell, placeholder);
        if full {
            old.set_extent(axis, old_size);
            resize_child_cells(&mut old);
        } else {
            resize_node_to(&mut old, axis, old_size);
        }
        let mut new_geometry = geometry;
        new_geometry.set_extent(axis, if before { first } else { second });
        let new = CellNode::Leaf {
            pane: new_pane,
            geometry: new_geometry,
            saved: None,
        };
        let children = if before {
            vec![
                CellChild {
                    divider: None,
                    node: new,
                },
                CellChild {
                    divider: Some(divider),
                    node: old,
                },
            ]
        } else {
            vec![
                CellChild {
                    divider: None,
                    node: old,
                },
                CellChild {
                    divider: Some(divider),
                    node: new,
                },
            ]
        };
        *cell = CellNode::Node {
            axis,
            geometry,
            children,
        };
        fix_offsets(&mut self.root);
        self.debug_validate();
        Ok(())
    }

    pub(crate) fn remove(&mut self, pane: PaneId) -> Result<(), LayoutError> {
        let path = pane_path(&self.root, pane).ok_or(LayoutError::UnknownPane)?;
        if path.is_empty() {
            return Err(LayoutError::LastPane);
        }
        self.extent = self.extent();
        let parent_path = &path[..path.len() - 1];
        let index = path[path.len() - 1];
        if matches!(node_at_path(&self.root, parent_path), Some(CellNode::Node { children, .. }) if children.len() < 2)
        {
            return Ok(());
        }
        let tiled = node_at_path(&self.root, &path).is_some_and(CellNode::tiled);
        if tiled {
            let neighbour = {
                let Some(CellNode::Node { children, .. }) = node_at_path(&self.root, parent_path)
                else {
                    return Err(LayoutError::UnknownPane);
                };
                cell_neighbour(children, index)
            };
            match neighbour {
                Some(neighbour) => {
                    let Some(CellNode::Node { axis, children, .. }) =
                        node_at_path_mut(&mut self.root, parent_path)
                    else {
                        return Err(LayoutError::UnknownPane);
                    };
                    let gift = children[index].node.extent(*axis) + 1;
                    resize_adjust(&mut children[neighbour].node, *axis, i32::from(gift));
                }
                None => {
                    remove_tile(&mut self.root, parent_path);
                }
            }
        }
        let Some(CellNode::Node { children, .. }) = node_at_path_mut(&mut self.root, parent_path)
        else {
            return Err(LayoutError::UnknownPane);
        };
        children.remove(index);
        if let Some(first) = children.first_mut() {
            first.divider = None;
        }
        if children.len() == 1 {
            collapse_single_child(&mut self.root, parent_path);
            if parent_path.is_empty()
                && let CellNode::Leaf { geometry, .. } = &mut self.root
            {
                geometry.xoff = 0;
                geometry.yoff = 0;
            }
        }
        fix_offsets(&mut self.root);
        self.debug_validate();
        Ok(())
    }

    pub(crate) fn float(
        &mut self,
        target: PaneId,
        pane: PaneId,
        geometry: CellGeometry,
        ids: &mut dyn FnMut() -> SplitId,
    ) -> Result<(), LayoutError> {
        let mut path = pane_path(&self.root, target).ok_or(LayoutError::UnknownPane)?;
        if path.is_empty() {
            let root_geometry = self.root.geometry();
            let old = std::mem::replace(&mut self.root, leaf(pane, 1, 1));
            self.root = CellNode::Node {
                axis: Axis::Vertical,
                geometry: root_geometry,
                children: vec![CellChild {
                    divider: None,
                    node: old,
                }],
            };
            path.push(0);
        }
        let index = path[path.len() - 1];
        let Some(CellNode::Node { children, .. }) =
            node_at_path_mut(&mut self.root, &path[..path.len() - 1])
        else {
            return Err(LayoutError::UnknownPane);
        };
        children.insert(
            index + 1,
            CellChild {
                divider: Some(ids()),
                node: CellNode::Float { pane, geometry },
            },
        );
        self.debug_validate();
        Ok(())
    }

    pub(crate) fn float_tile(&mut self, pane: PaneId, geometry: CellGeometry) -> bool {
        let Some(path) = pane_path(&self.root, pane) else {
            return false;
        };
        if !matches!(node_at_path(&self.root, &path), Some(CellNode::Leaf { .. })) {
            return false;
        }
        self.extent = self.extent();
        remove_tile(&mut self.root, &path);
        if let Some(node) = node_at_path_mut(&mut self.root, &path) {
            *node = CellNode::Float { pane, geometry };
        }
        fix_offsets(&mut self.root);
        self.debug_validate();
        true
    }

    pub(crate) fn tile_float(&mut self, pane: PaneId) -> Result<(), LayoutError> {
        let path = pane_path(&self.root, pane).ok_or(LayoutError::UnknownPane)?;
        let Some(CellNode::Float { geometry, .. }) = node_at_path(&self.root, &path) else {
            return Err(LayoutError::UnknownPane);
        };
        let saved = *geometry;
        if !insert_tile(&mut self.root, &path, self.extent) {
            return Err(LayoutError::NoSpace);
        }
        if let Some(node) = node_at_path_mut(&mut self.root, &path) {
            let geometry = node.geometry();
            *node = CellNode::Leaf {
                pane,
                geometry,
                saved: Some(saved),
            };
        }
        fix_offsets(&mut self.root);
        self.debug_validate();
        Ok(())
    }

    pub(crate) fn set_float_geometry(&mut self, pane: PaneId, geometry: CellGeometry) -> bool {
        match pane_cell_mut(&mut self.root, pane) {
            Some(CellNode::Float {
                geometry: current, ..
            }) => {
                let changed = *current != geometry;
                *current = geometry;
                changed
            }
            _ => false,
        }
    }

    pub(crate) fn link_floats(
        &mut self,
        floats: &[(PaneId, CellGeometry)],
        ids: &mut dyn FnMut() -> SplitId,
    ) {
        if floats.is_empty() {
            return;
        }
        if !matches!(self.root, CellNode::Node { .. }) {
            let geometry = self.root.geometry();
            let old = std::mem::replace(&mut self.root, leaf(PaneId(0), 1, 1));
            self.root = CellNode::Node {
                axis: Axis::Vertical,
                geometry,
                children: vec![CellChild {
                    divider: None,
                    node: old,
                }],
            };
        }
        if let CellNode::Node { children, .. } = &mut self.root {
            for (pane, geometry) in floats {
                children.push(CellChild {
                    divider: Some(ids()),
                    node: CellNode::Float {
                        pane: *pane,
                        geometry: *geometry,
                    },
                });
            }
        }
        self.debug_validate();
    }

    pub(crate) fn keep_extent(&mut self, extent: (u16, u16)) {
        self.extent = extent;
    }

    pub(crate) fn clamp_floats(&mut self, sx: u16, sy: u16, pad: &dyn Fn(PaneId) -> u16) {
        clamp_floats(&mut self.root, sx, sy, pad);
    }

    pub(crate) fn resize(&mut self, sx: u16, sy: u16) {
        self.resize_with(sx, sy, &|_| 1);
    }

    pub(crate) fn resize_with(&mut self, sx: u16, sy: u16, pad: &dyn Fn(PaneId) -> u16) {
        let sx = sx.clamp(PANE_MINIMUM, PANE_MAXIMUM);
        let sy = sy.clamp(PANE_MINIMUM, PANE_MAXIMUM);
        self.extent = (sx, sy);
        if !matches!(self.root, CellNode::Float { .. }) {
            resize_root_axis(&mut self.root, Axis::Horizontal, sx);
            resize_root_axis(&mut self.root, Axis::Vertical, sy);
            fix_offsets(&mut self.root);
        }
        clamp_floats(&mut self.root, sx, sy, pad);
        self.debug_validate();
    }

    pub(crate) fn resize_pane(
        &mut self,
        pane: PaneId,
        axis: Axis,
        delta: i32,
    ) -> Result<(), LayoutError> {
        let path = pane_path(&self.root, pane).ok_or(LayoutError::UnknownPane)?;
        let Some((parent_path, mut index)) = matching_ancestor(&self.root, &path, axis) else {
            self.debug_validate();
            return Ok(());
        };
        let parent =
            node_at_path_mut(&mut self.root, &parent_path).ok_or(LayoutError::UnknownPane)?;
        let CellNode::Node { children, .. } = parent else {
            return Err(LayoutError::UnknownPane);
        };
        if last_tiled(children, index) {
            let Some(previous) = neighbour_dir(children, index, false) else {
                return Ok(());
            };
            index = previous;
        }
        resize_siblings(children, index, axis, delta);
        fix_offsets(&mut self.root);
        self.debug_validate();
        Ok(())
    }

    pub(crate) fn resize_pane_to(
        &mut self,
        pane: PaneId,
        axis: Axis,
        size: u16,
    ) -> Result<(), LayoutError> {
        let path = pane_path(&self.root, pane).ok_or(LayoutError::UnknownPane)?;
        let Some((parent_path, index)) = matching_ancestor(&self.root, &path, axis) else {
            self.debug_validate();
            return Ok(());
        };
        let parent = node_at_path(&self.root, &parent_path).ok_or(LayoutError::UnknownPane)?;
        let CellNode::Node { children, .. } = parent else {
            return Err(LayoutError::UnknownPane);
        };
        let current = children[index].node.extent(axis);
        let change = if last_tiled(children, index) {
            i32::from(current) - i32::from(size)
        } else {
            i32::from(size) - i32::from(current)
        };
        self.resize_pane(pane, axis, change)
    }

    pub(crate) fn set_divider_ratio(
        &mut self,
        divider: SplitId,
        ratio: f32,
    ) -> Result<bool, LayoutError> {
        let changed =
            resize_divider(&mut self.root, divider, ratio).ok_or(LayoutError::UnknownDivider)?;
        fix_offsets(&mut self.root);
        self.debug_validate();
        Ok(changed)
    }

    /// The pinned tmux spreads only leaf children and corrupts the cell sums of a
    /// parent that mixes leaves with nested nodes; zz refuses that parent instead
    /// and stops the walk where the pin would have stopped after corrupting.
    pub(crate) fn spread(&mut self, pane: PaneId) -> Result<bool, LayoutError> {
        let path = pane_path(&self.root, pane).ok_or(LayoutError::UnknownPane)?;
        for depth in (0..path.len()).rev() {
            let parent_path = &path[..depth];
            let parent =
                node_at_path_mut(&mut self.root, parent_path).ok_or(LayoutError::UnknownPane)?;
            match spread_node(parent) {
                SpreadCell::Changed => {
                    fix_offsets(&mut self.root);
                    self.debug_validate();
                    return Ok(true);
                }
                SpreadCell::Refused => break,
                SpreadCell::Unchanged => {}
            }
        }
        self.debug_validate();
        Ok(false)
    }

    /// Unlike the pinned tmux bug for two panes, main presets always size the other pane.
    pub(crate) fn apply_preset(
        &mut self,
        preset: LayoutPreset,
        panes: &[PaneId],
        options: &PresetOptions,
        ids: &mut dyn FnMut() -> SplitId,
    ) {
        if panes.is_empty() {
            debug_assert!(!panes.is_empty());
            return;
        }
        let floats = panes
            .iter()
            .filter_map(|pane| match pane_cell(&self.root, *pane) {
                Some(CellNode::Float { geometry, .. }) => Some((*pane, *geometry)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let tiled = panes
            .iter()
            .copied()
            .filter(|pane| !floats.contains_key(pane))
            .collect::<Vec<_>>();
        if !floats.is_empty() && tiled.len() <= 1 {
            return;
        }
        let (sx, sy) = self.extent();
        if panes.len() == 1 {
            self.root = leaf(panes[0], sx, sy);
            self.debug_validate();
            return;
        }
        let order = panes.to_vec();
        let panes = tiled.as_slice();
        self.root = match preset {
            LayoutPreset::EvenHorizontal => even_layout(Axis::Horizontal, sx, sy, panes, ids),
            LayoutPreset::EvenVertical => even_layout(Axis::Vertical, sx, sy, panes, ids),
            LayoutPreset::MainHorizontal => main_horizontal(sx, sy, panes, false, options, ids),
            LayoutPreset::MainHorizontalMirrored => {
                main_horizontal(sx, sy, panes, true, options, ids)
            }
            LayoutPreset::MainVertical => main_vertical(sx, sy, panes, false, options, ids),
            LayoutPreset::MainVerticalMirrored => main_vertical(sx, sy, panes, true, options, ids),
            LayoutPreset::Tiled => {
                tiled_layout(sx, sy, panes, options.tiled_layout_max_columns, ids)
            }
        };
        if !floats.is_empty() {
            let CellNode::Node { children, .. } = &mut self.root else {
                return;
            };
            match preset {
                LayoutPreset::EvenHorizontal | LayoutPreset::EvenVertical => {
                    link_floats(children, &order, &floats, ids);
                }
                LayoutPreset::MainHorizontal
                | LayoutPreset::MainHorizontalMirrored
                | LayoutPreset::MainVertical
                | LayoutPreset::MainVerticalMirrored
                    if tiled.len() > 2 =>
                {
                    let main = tiled[0];
                    let order = order
                        .iter()
                        .copied()
                        .filter(|pane| *pane != main)
                        .collect::<Vec<_>>();
                    if let Some(CellNode::Node { children, .. }) = children
                        .iter_mut()
                        .map(|child| &mut child.node)
                        .find(|node| matches!(node, CellNode::Node { .. }))
                    {
                        link_floats(children, &order, &floats, ids);
                    }
                }
                _ => {
                    let order = order
                        .iter()
                        .copied()
                        .filter(|pane| floats.contains_key(pane))
                        .collect::<Vec<_>>();
                    for pane in order {
                        children.push(CellChild {
                            divider: Some(ids()),
                            node: CellNode::Float {
                                pane,
                                geometry: floats[&pane],
                            },
                        });
                    }
                }
            }
        }
        fix_offsets(&mut self.root);
        self.debug_validate();
    }

    pub(crate) fn swap(&mut self, a: PaneId, b: PaneId) -> bool {
        if a == b || !self.contains(a) || !self.contains(b) {
            return false;
        }
        swap_panes(&mut self.root, a, b);
        self.debug_validate();
        true
    }

    pub(crate) fn replace(&mut self, old: PaneId, new: PaneId) -> bool {
        if old == new {
            return false;
        }
        let changed = replace_pane(&mut self.root, old, new);
        self.debug_validate();
        changed
    }

    pub(crate) fn remap(&mut self, mapping: &BTreeMap<PaneId, PaneId>) {
        remap_panes(&mut self.root, mapping);
        self.debug_validate();
    }

    pub(crate) fn pane_count(&self) -> usize {
        count_panes(&self.root)
    }

    pub(crate) fn replace_panes_in_order(&mut self, panes: &[PaneId]) -> bool {
        if panes.len() != self.pane_count() {
            return false;
        }
        let mut panes = panes.iter().copied();
        replace_panes_in_order(&mut self.root, &mut panes);
        let panes_exhausted = panes.next().is_none();
        debug_assert!(panes_exhausted);
        self.debug_validate();
        true
    }

    pub(crate) fn refresh_divider_ids(&mut self, ids: &mut dyn FnMut() -> SplitId) {
        refresh_divider_ids(&mut self.root, ids);
        self.debug_validate();
    }

    pub fn project(&self) -> LayoutNode {
        project_node(&self.root).unwrap_or_else(|| {
            let mut panes = Vec::new();
            collect_panes(&self.root, &mut panes);
            LayoutNode::Pane(panes[0])
        })
    }

    #[must_use]
    pub fn dump(&self) -> String {
        self.dump_as(LayoutFormat::V1, &|_| LeafState::default())
    }

    #[must_use]
    pub fn dump_as(&self, format: LayoutFormat, leaf: &dyn Fn(PaneId) -> LeafState) -> String {
        match format {
            LayoutFormat::V1 => dump_v1(tiled_copy(&self.root)),
            LayoutFormat::V2 => {
                let mut output = String::from("{\"V\":2,\"L\":");
                dump_v2_node(&self.root, leaf, &mut output);
                output.push('}');
                output
            }
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let geometry = self.root.geometry();
        if !matches!(self.root, CellNode::Float { .. })
            && (geometry.xoff != 0 || geometry.yoff != 0)
        {
            return Err("root offset is not zero".to_owned());
        }
        let mut dividers = BTreeSet::new();
        validate_node(&self.root, &mut dividers)
    }

    fn debug_validate(&self) {
        let result = self.validate();
        debug_assert!(result.is_ok(), "{result:?}");
    }
}

impl ParsedLayout {
    pub(crate) const fn version(&self) -> u8 {
        self.version
    }

    pub(crate) fn pane_count(&self) -> usize {
        count_parsed_panes(&self.root)
    }

    pub(crate) fn has_floating(&self) -> bool {
        let mut leaves = Vec::new();
        parsed_leaves(&self.root, &mut leaves);
        leaves.iter().any(|leaf| leaf.float.is_some())
    }

    pub(crate) fn trim_bottom_right(&mut self) {
        let mut path = Vec::new();
        parsed_bottom_right_path(&self.root, &mut path);
        let Some((&index, parent_path)) = path.split_last() else {
            return;
        };
        let collapse = {
            let Some(ParsedNode::Node { axis, children, .. }) =
                parsed_node_at_path_mut(&mut self.root, parent_path)
            else {
                return;
            };
            if children[index].tiled() {
                let gift = children[index].extent(*axis).saturating_add(1);
                if let Some(neighbour) = (0..index).rev().find(|other| children[*other].tiled()) {
                    parsed_resize_adjust(&mut children[neighbour], *axis, i32::from(gift));
                }
            }
            children.remove(index);
            children.len() == 1
        };
        if collapse {
            collapse_parsed_single_child(&mut self.root, parent_path);
        }
    }

    pub(crate) fn check_sizes(&mut self) -> Result<(), LayoutParseError> {
        if correct_parsed_root_size(&mut self.root) && check_parsed_node(&self.root) {
            Ok(())
        } else {
            Err(LayoutParseError::new("size mismatch after applying layout"))
        }
    }

    pub(crate) fn into_layout(
        self,
        panes: &[PaneId],
        ids: &mut dyn FnMut() -> SplitId,
    ) -> (CellLayout, Option<ParsedSelection>) {
        debug_assert_eq!(panes.len(), self.pane_count());
        let mut leaves = Vec::new();
        parsed_leaves(&self.root, &mut leaves);
        let (order, selection) = if self.version == 1 {
            (panes.to_vec(), None)
        } else {
            let mut ranked = (0..leaves.len()).collect::<Vec<_>>();
            ranked.sort_by_key(|position| leaves[*position].index);
            let mut order = vec![PaneId(0); leaves.len()];
            for (pane, position) in panes.iter().zip(ranked) {
                order[position] = *pane;
            }
            let active = leaves
                .iter()
                .zip(&order)
                .find_map(|(leaf, pane)| leaf.active.then_some(*pane));
            let mut last = leaves
                .iter()
                .zip(&order)
                .filter(|(leaf, _)| !leaf.active)
                .filter_map(|(leaf, pane)| leaf.last.map(|last| (last, *pane)))
                .collect::<Vec<_>>();
            last.sort_by_key(|(last, _)| *last);
            let mut floats = leaves
                .iter()
                .zip(&order)
                .filter_map(|(leaf, pane)| leaf.float.map(|float| (float.z, *pane)))
                .collect::<Vec<_>>();
            floats.sort_by_key(|(z, _)| *z);
            let selection = ParsedSelection {
                active,
                last_panes: last.into_iter().map(|(_, pane)| pane).collect(),
                floats: floats.into_iter().map(|(_, pane)| pane).collect(),
            };
            (order, Some(selection))
        };
        let mut order = order.into_iter();
        let root = assign_parsed_node(self.root, &mut order);
        let panes_exhausted = order.next().is_none();
        debug_assert!(panes_exhausted);
        let geometry = root.geometry();
        let mut layout = CellLayout {
            root,
            extent: (geometry.sx, geometry.sy),
        };
        fix_offsets(&mut layout.root);
        layout.refresh_divider_ids(ids);
        (layout, selection)
    }
}

fn parse_v1(input: &[u8]) -> Result<ParsedLayout, LayoutParseError> {
    let header = input
        .get(..5)
        .filter(|prefix| prefix[4] == b',' && prefix[..4].iter().all(u8::is_ascii_hexdigit));
    let Some(prefix) = header else {
        return Err(LayoutParseError::new("malformed layout header"));
    };
    let expected = prefix[..4].iter().fold(0_u16, |value, byte| {
        value * 16
            + u16::from(match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => 0,
            })
    });
    let body = &input[5..];
    if expected != layout_checksum(body) {
        return Err(LayoutParseError::new("invalid layout checksum"));
    }
    let mut parser = LayoutParser::new(body);
    let root = parser
        .parse_node(0)
        .ok_or_else(|| LayoutParseError::new("invalid layout"))?;
    if !parser.is_done() {
        return Err(LayoutParseError::new("trailing data"));
    }
    Ok(ParsedLayout { root, version: 1 })
}

fn parse_v2(json: &Json<'_>) -> Result<ParsedLayout, LayoutParseError> {
    let version = json.number("V").map_err(LayoutParseError)?;
    let root = json.object("L").map_err(LayoutParseError)?;
    let mut active = 0;
    let root = parse_v2_cell(root, &mut active).map_err(LayoutParseError)?;
    if version != 2 {
        return Err(LayoutParseError::new("version mismatch"));
    }
    if active > 1 {
        return Err(LayoutParseError::new("more than one active pane"));
    }
    let mut leaves = Vec::new();
    parsed_leaves(&root, &mut leaves);
    if leaves.is_empty() {
        return Err(LayoutParseError::new("no panes"));
    }
    let duplicated = |values: &mut Vec<u32>| {
        values.sort_unstable();
        values.windows(2).any(|pair| pair[0] == pair[1])
    };
    if duplicated(&mut leaves.iter().map(|leaf| leaf.index).collect()) {
        return Err(LayoutParseError::new("duplicate pane index"));
    }
    if duplicated(
        &mut leaves
            .iter()
            .filter_map(|leaf| leaf.float.map(|float| float.z))
            .collect(),
    ) {
        return Err(LayoutParseError::new("duplicate pane z-index"));
    }
    if duplicated(&mut leaves.iter().filter_map(|leaf| leaf.last).collect()) {
        return Err(LayoutParseError::new("duplicate last pane index"));
    }
    Ok(ParsedLayout { root, version: 2 })
}

fn parse_v2_cell(cell: &Json<'_>, active: &mut usize) -> Result<ParsedNode, String> {
    let kind = cell.string("t")?;
    let axis = match kind {
        "p" => None,
        "v" => Some(Axis::Vertical),
        "h" => Some(Axis::Horizontal),
        _ => return Err(format!("unknown cell type \"{kind}\"")),
    };
    let bounded = |key: &str, name: &str, range: std::ops::RangeInclusive<i64>| {
        let value = cell.number(key)?;
        if range.contains(&value) {
            Ok(value)
        } else {
            Err(format!("invalid {name} {value}"))
        }
    };
    let size = i64::from(PANE_MINIMUM)..=i64::from(PANE_MAXIMUM);
    let offset = -WINDOW_MAXIMUM..=WINDOW_MAXIMUM;
    let sx = bounded("w", "width", size.clone())?;
    let sy = bounded("h", "height", size)?;
    let xoff = bounded("x", "x-offset", offset.clone())?;
    let yoff = bounded("y", "y-offset", offset)?;
    let narrow = |value: i64| u16::try_from(value.max(0)).unwrap_or(u16::MAX);
    let geometry = CellGeometry {
        sx: narrow(sx),
        sy: narrow(sy),
        xoff: i32::try_from(xoff).unwrap_or_default(),
        yoff: i32::try_from(yoff).unwrap_or_default(),
    };
    let int = i64::from(i32::MAX);
    let Some(axis) = axis else {
        if cell.find("c").is_some() {
            return Err("panes cannot have children".to_owned());
        }
        let index = bounded("i", "index", 0..=int)?;
        let mut leaf = ParsedLeaf {
            index: u32::try_from(index).unwrap_or_default(),
            ..ParsedLeaf::default()
        };
        if cell.find("a").is_some() {
            leaf.active = cell.boolean("a")?;
            if leaf.active {
                *active += 1;
            }
        } else if cell.find("l").is_some() {
            leaf.last = Some(u32::try_from(bounded("l", "last", 0..=int)?).unwrap_or_default());
        }
        if cell.find("z").is_some() {
            let z = bounded("z", "floating zindex", 0..=int - 1)?;
            leaf.float = Some(ParsedFloat {
                z: u32::try_from(z).unwrap_or_default(),
                xoff: i32::try_from(xoff).unwrap_or_default(),
                yoff: i32::try_from(yoff).unwrap_or_default(),
            });
        }
        leaf.id = match cell.find("I") {
            Some(Json::String(id)) => id.strip_prefix('%').and_then(|id| id.parse().ok()),
            _ => None,
        };
        return Ok(ParsedNode::Leaf { geometry, leaf });
    };
    let members = cell.array("c")?;
    if members.len() < 2 {
        return Err("nodes must have more than one child".to_owned());
    }
    let children = members
        .iter()
        .map(|member| parse_v2_cell(member, active))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ParsedNode::Node {
        axis,
        geometry,
        children,
    })
}

fn parsed_leaves(node: &ParsedNode, leaves: &mut Vec<ParsedLeaf>) {
    match node {
        ParsedNode::Leaf { leaf, .. } => leaves.push(*leaf),
        ParsedNode::Node { children, .. } => {
            for child in children {
                parsed_leaves(child, leaves);
            }
        }
    }
}

#[must_use]
pub fn legacy_layout(layout: &str) -> String {
    let trimmed = layout.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    if !trimmed.starts_with('{') {
        return layout.to_owned();
    }
    json::parse_within(trimmed, usize::MAX)
        .and_then(|json| parse_v2_cell(json.object("L")?, &mut 0))
        .map_or_else(
            |_| "0000,".to_owned(),
            |root| dump_v1(tiled_copy_parsed(&root)),
        )
}

#[must_use]
pub fn legacy_layouts_in(text: &str) -> Cow<'_, str> {
    const MARKER: &str = "{\"V\":2,\"L\":";
    if !text.contains(MARKER) {
        return Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(MARKER) {
        output.push_str(&rest[..start]);
        let candidate = &rest[start..];
        let Some(end) = json_object_end(candidate) else {
            output.push_str(candidate);
            return Cow::Owned(output);
        };
        output.push_str(&legacy_layout(&candidate[..end]));
        rest = &candidate[end..];
    }
    output.push_str(rest);
    Cow::Owned(output)
}

fn json_object_end(text: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in text.bytes().enumerate() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn tiled_copy(node: &CellNode) -> Option<CellNode> {
    match node {
        CellNode::Leaf { .. } => Some(node.clone()),
        CellNode::Float { .. } => None,
        CellNode::Node {
            axis,
            geometry,
            children,
        } => collapse_copy(
            *axis,
            *geometry,
            children
                .iter()
                .filter_map(|child| tiled_copy(&child.node))
                .collect(),
        ),
    }
}

fn tiled_copy_parsed(node: &ParsedNode) -> Option<CellNode> {
    match node {
        ParsedNode::Leaf { geometry, leaf } => leaf.float.is_none().then(|| CellNode::Leaf {
            pane: PaneId(leaf.id.unwrap_or_default()),
            geometry: *geometry,
            saved: None,
        }),
        ParsedNode::Node {
            axis,
            geometry,
            children,
        } => collapse_copy(
            *axis,
            *geometry,
            children.iter().filter_map(tiled_copy_parsed).collect(),
        ),
    }
}

fn collapse_copy(
    axis: Axis,
    geometry: CellGeometry,
    mut children: Vec<CellNode>,
) -> Option<CellNode> {
    match children.len() {
        0 => None,
        1 => children.pop(),
        _ => Some(CellNode::Node {
            axis,
            geometry,
            children: children
                .into_iter()
                .map(|node| CellChild {
                    divider: None,
                    node,
                })
                .collect(),
        }),
    }
}

fn dump_v1(root: Option<CellNode>) -> String {
    let Some(mut root) = root else {
        return "0000,".to_owned();
    };
    if let CellNode::Leaf { geometry, .. } = &mut root {
        geometry.xoff = 0;
        geometry.yoff = 0;
    }
    let mut body = String::new();
    dump_node(&root, &mut body);
    let checksum = layout_checksum(body.as_bytes());
    format!("{checksum:04x},{body}")
}

fn dump_v2_node(node: &CellNode, leaf: &dyn Fn(PaneId) -> LeafState, output: &mut String) {
    let geometry = node.geometry();
    let kind = match node {
        CellNode::Leaf { .. } | CellNode::Float { .. } => 'p',
        CellNode::Node {
            axis: Axis::Vertical,
            ..
        } => 'v',
        CellNode::Node {
            axis: Axis::Horizontal,
            ..
        } => 'h',
    };
    let _ = write!(
        output,
        "{{\"t\":\"{kind}\",\"w\":{},\"h\":{},\"x\":{},\"y\":{}",
        geometry.sx, geometry.sy, geometry.xoff, geometry.yoff
    );
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } => {
            let state = leaf(*pane);
            if state.active {
                output.push_str(",\"a\":true");
            } else if let Some(last) = state.last {
                let _ = write!(output, ",\"l\":{last}");
            }
            let _ = write!(output, ",\"i\":{}", state.index);
            if let Some(z) = state.z {
                let _ = write!(output, ",\"z\":{z}");
            }
            let _ = write!(output, ",\"I\":\"%{}\"", pane.0);
        }
        CellNode::Node { children, .. } => {
            output.push_str(",\"c\":[");
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                dump_v2_node(&child.node, leaf, output);
            }
            output.push(']');
        }
    }
    output.push('}');
}

struct LayoutParser<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl<'a> LayoutParser<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, cursor: 0 }
    }

    fn is_done(&self) -> bool {
        self.cursor == self.input.len()
    }

    fn parse_node(&mut self, depth: usize) -> Option<ParsedNode> {
        if depth >= MAX_LAYOUT_DEPTH {
            return None;
        }
        let sx = self.number()?;
        self.expect(b'x')?;
        let sy = self.number()?;
        self.expect(b',')?;
        let xoff = self.number()?;
        self.expect(b',')?;
        let yoff = self.number()?;
        if !(PANE_MINIMUM..=PANE_MAXIMUM).contains(&sx)
            || !(PANE_MINIMUM..=PANE_MAXIMUM).contains(&sy)
            || i64::from(xoff) > WINDOW_MAXIMUM
            || i64::from(yoff) > WINDOW_MAXIMUM
        {
            return None;
        }
        let geometry = CellGeometry {
            sx,
            sy,
            xoff: i32::from(xoff),
            yoff: i32::from(yoff),
        };
        if self.peek() == Some(b',') {
            let saved = self.cursor;
            self.cursor += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.cursor += 1;
            }
            if self.peek() == Some(b'x') {
                self.cursor = saved;
            }
        }
        match self.peek() {
            Some(b'{') => self.parse_children(depth, Axis::Horizontal, geometry, b'}'),
            Some(b'[') => self.parse_children(depth, Axis::Vertical, geometry, b']'),
            Some(b',' | b'}' | b']') | None => Some(ParsedNode::Leaf {
                geometry,
                leaf: ParsedLeaf::default(),
            }),
            _ => None,
        }
    }

    fn parse_children(
        &mut self,
        depth: usize,
        axis: Axis,
        geometry: CellGeometry,
        closing: u8,
    ) -> Option<ParsedNode> {
        self.cursor += 1;
        let mut children = vec![self.parse_node(depth + 1)?];
        while self.peek() == Some(b',') {
            self.cursor += 1;
            children.push(self.parse_node(depth + 1)?);
        }
        self.expect(closing)?;
        if children.len() < 2 {
            return None;
        }
        Some(ParsedNode::Node {
            axis,
            geometry,
            children,
        })
    }

    fn number(&mut self) -> Option<u16> {
        let start = self.cursor;
        let mut value = 0_u16;
        while let Some(byte @ b'0'..=b'9') = self.peek() {
            value = value.checked_mul(10)?.checked_add(u16::from(byte - b'0'))?;
            self.cursor += 1;
        }
        (self.cursor > start).then_some(value)
    }

    fn expect(&mut self, expected: u8) -> Option<()> {
        if self.peek()? != expected {
            return None;
        }
        self.cursor += 1;
        Some(())
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.cursor).copied()
    }
}

fn layout_checksum(layout: &[u8]) -> u16 {
    layout.iter().fold(0_u16, |checksum, byte| {
        checksum.rotate_right(1).wrapping_add(u16::from(*byte))
    })
}

fn correct_parsed_root_size(root: &mut ParsedNode) -> bool {
    let ParsedNode::Node {
        axis,
        geometry,
        children,
    } = root
    else {
        return true;
    };
    let Some(last) = children.iter().rfind(|child| child.tiled()) else {
        return true;
    };
    let along = children
        .iter()
        .filter(|child| child.tiled())
        .try_fold(0_u32, |total, child| {
            total.checked_add(u32::from(child.extent(*axis)) + 1)
        });
    let Some(along) = along.and_then(|total| total.checked_sub(1)) else {
        return false;
    };
    let Ok(along) = u16::try_from(along) else {
        return false;
    };
    let (sx, sy) = match axis {
        Axis::Horizontal => (along, last.geometry().sy),
        Axis::Vertical => (last.geometry().sx, along),
    };
    if geometry.sx != sx || geometry.sy != sy {
        geometry.sx = sx;
        geometry.sy = sy;
    }
    true
}

fn check_parsed_node(node: &ParsedNode) -> bool {
    let ParsedNode::Node {
        axis,
        geometry,
        children,
    } = node
    else {
        let geometry = node.geometry();
        return geometry.sx >= PANE_MINIMUM && geometry.sy >= PANE_MINIMUM;
    };
    if children.len() < 2 {
        return false;
    }
    let mut extent = 0_u32;
    for child in children.iter().filter(|child| child.tiled()) {
        let child_geometry = child.geometry();
        let cross_matches = match axis {
            Axis::Horizontal => child_geometry.sy == geometry.sy,
            Axis::Vertical => child_geometry.sx == geometry.sx,
        };
        if !cross_matches || !check_parsed_node(child) {
            return false;
        }
        let Some(next) = extent.checked_add(u32::from(child.extent(*axis)) + 1) else {
            return false;
        };
        extent = next;
    }
    extent == 0 || extent.checked_sub(1) == Some(u32::from(geometry.extent(*axis)))
}

fn count_parsed_panes(node: &ParsedNode) -> usize {
    match node {
        ParsedNode::Leaf { .. } => 1,
        ParsedNode::Node { children, .. } => children
            .iter()
            .map(count_parsed_panes)
            .fold(0_usize, usize::saturating_add),
    }
}

fn parsed_bottom_right_path(node: &ParsedNode, path: &mut Vec<usize>) {
    let ParsedNode::Node { children, .. } = node else {
        return;
    };
    let index = children.len() - 1;
    path.push(index);
    parsed_bottom_right_path(&children[index], path);
}

fn parsed_node_at_path_mut<'a>(
    mut node: &'a mut ParsedNode,
    path: &[usize],
) -> Option<&'a mut ParsedNode> {
    for index in path {
        let ParsedNode::Node { children, .. } = node else {
            return None;
        };
        node = children.get_mut(*index)?;
    }
    Some(node)
}

fn parsed_resize_check(node: &ParsedNode, axis: Axis) -> u16 {
    match node {
        ParsedNode::Leaf { geometry, .. } => geometry.extent(axis).saturating_sub(PANE_MINIMUM),
        ParsedNode::Node {
            axis: node_axis,
            children,
            ..
        } if *node_axis == axis => children.iter().fold(0_u16, |available, child| {
            available.saturating_add(parsed_resize_check(child, axis))
        }),
        ParsedNode::Node { children, .. } => children
            .iter()
            .map(|child| parsed_resize_check(child, axis))
            .min()
            .unwrap_or(0),
    }
}

fn parsed_resize_adjust(node: &mut ParsedNode, axis: Axis, change: i32) {
    if change == 0 {
        return;
    }
    let size = i32::from(node.extent(axis)) + change;
    let size = u16::try_from(size).unwrap_or(if size < 0 { 0 } else { u16::MAX });
    node.set_extent(axis, size);
    let ParsedNode::Node {
        axis: node_axis,
        children,
        ..
    } = node
    else {
        return;
    };
    if *node_axis != axis {
        for child in children {
            parsed_resize_adjust(child, axis, change);
        }
        return;
    }
    let mut remaining = change;
    while remaining != 0 {
        let mut changed = false;
        for child in &mut *children {
            if remaining == 0 {
                break;
            }
            if remaining > 0 {
                parsed_resize_adjust(child, axis, 1);
                remaining -= 1;
                changed = true;
            } else if parsed_resize_check(child, axis) > 0 {
                parsed_resize_adjust(child, axis, -1);
                remaining += 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn collapse_parsed_single_child(root: &mut ParsedNode, parent_path: &[usize]) {
    if parent_path.is_empty() {
        let replacement = match root {
            ParsedNode::Node { children, .. } if children.len() == 1 => Some(children.remove(0)),
            _ => None,
        };
        if let Some(replacement) = replacement {
            *root = replacement;
        }
        return;
    }
    let grandparent_path = &parent_path[..parent_path.len() - 1];
    let index = parent_path[parent_path.len() - 1];
    let Some(ParsedNode::Node { children, .. }) = parsed_node_at_path_mut(root, grandparent_path)
    else {
        return;
    };
    let parent = children.remove(index);
    let (axis, geometry, mut parent_children) = match parent {
        ParsedNode::Node {
            axis,
            geometry,
            children,
        } => (axis, geometry, children),
        leaf @ ParsedNode::Leaf { .. } => {
            children.insert(index, leaf);
            return;
        }
    };
    if parent_children.len() == 1 {
        children.insert(index, parent_children.remove(0));
    } else {
        children.insert(
            index,
            ParsedNode::Node {
                axis,
                geometry,
                children: parent_children,
            },
        );
    }
}

fn assign_parsed_node(node: ParsedNode, panes: &mut impl Iterator<Item = PaneId>) -> CellNode {
    match node {
        ParsedNode::Leaf {
            geometry,
            leaf: ParsedLeaf {
                float: Some(float), ..
            },
        } => CellNode::Float {
            pane: panes.next().expect("parsed pane count was checked"),
            geometry: CellGeometry {
                xoff: float.xoff,
                yoff: float.yoff,
                ..geometry
            },
        },
        ParsedNode::Leaf { geometry, .. } => CellNode::Leaf {
            pane: panes.next().expect("parsed pane count was checked"),
            geometry,
            saved: None,
        },
        ParsedNode::Node {
            axis,
            geometry,
            children,
        } => CellNode::Node {
            axis,
            geometry,
            children: children
                .into_iter()
                .enumerate()
                .map(|(index, node)| CellChild {
                    divider: (index != 0).then_some(SplitId(0)),
                    node: assign_parsed_node(node, panes),
                })
                .collect(),
        },
    }
}

fn leaf(pane: PaneId, sx: u16, sy: u16) -> CellNode {
    CellNode::Leaf {
        pane,
        geometry: CellGeometry {
            sx,
            sy,
            xoff: 0,
            yoff: 0,
        },
        saved: None,
    }
}

fn collect_panes(node: &CellNode, panes: &mut Vec<PaneId>) {
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } => panes.push(*pane),
        CellNode::Node { children, .. } => {
            for child in children {
                collect_panes(&child.node, panes);
            }
        }
    }
}

fn count_panes(node: &CellNode) -> usize {
    match node {
        CellNode::Leaf { .. } | CellNode::Float { .. } => 1,
        CellNode::Node { children, .. } => {
            children.iter().map(|child| count_panes(&child.node)).sum()
        }
    }
}

pub(crate) fn carve_border_row(
    mut geometry: CellGeometry,
    root: CellGeometry,
    status: PaneBorderStatus,
) -> CellGeometry {
    let carve = match status {
        PaneBorderStatus::Off => false,
        PaneBorderStatus::Top => geometry.yoff == root.yoff,
        PaneBorderStatus::Bottom => {
            geometry.yoff + i32::from(geometry.sy) == root.yoff + i32::from(root.sy)
        }
    };
    if !carve {
        return geometry;
    }
    if status == PaneBorderStatus::Top {
        geometry.yoff = geometry.yoff.saturating_add(1);
    }
    if geometry.sy > 1 {
        geometry.sy -= 1;
    }
    geometry
}

fn pane_geometry(node: &CellNode, target: PaneId) -> Option<CellGeometry> {
    pane_cell(node, target).map(CellNode::geometry)
}

fn pane_cell(node: &CellNode, target: PaneId) -> Option<&CellNode> {
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } => {
            (*pane == target).then_some(node)
        }
        CellNode::Node { children, .. } => children
            .iter()
            .find_map(|child| pane_cell(&child.node, target)),
    }
}

fn pane_cell_mut(node: &mut CellNode, target: PaneId) -> Option<&mut CellNode> {
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } => {
            (*pane == target).then_some(node)
        }
        CellNode::Node { children, .. } => children
            .iter_mut()
            .find_map(|child| pane_cell_mut(&mut child.node, target)),
    }
}

fn pane_path(node: &CellNode, target: PaneId) -> Option<Vec<usize>> {
    fn find(node: &CellNode, target: PaneId, path: &mut Vec<usize>) -> bool {
        match node {
            CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } => *pane == target,
            CellNode::Node { children, .. } => {
                for (index, child) in children.iter().enumerate() {
                    path.push(index);
                    if find(&child.node, target, path) {
                        return true;
                    }
                    path.pop();
                }
                false
            }
        }
    }

    let mut path = Vec::new();
    find(node, target, &mut path).then_some(path)
}

fn node_at_path<'a>(mut node: &'a CellNode, path: &[usize]) -> Option<&'a CellNode> {
    for index in path {
        let CellNode::Node { children, .. } = node else {
            return None;
        };
        node = &children.get(*index)?.node;
    }
    Some(node)
}

fn node_at_path_mut<'a>(mut node: &'a mut CellNode, path: &[usize]) -> Option<&'a mut CellNode> {
    for index in path {
        let CellNode::Node { children, .. } = node else {
            return None;
        };
        node = &mut children.get_mut(*index)?.node;
    }
    Some(node)
}

fn matching_ancestor(root: &CellNode, path: &[usize], axis: Axis) -> Option<(Vec<usize>, usize)> {
    for depth in (0..path.len()).rev() {
        let parent_path = &path[..depth];
        if matches!(
            node_at_path(root, parent_path),
            Some(CellNode::Node { axis: parent_axis, .. }) if *parent_axis == axis
        ) {
            return Some((parent_path.to_vec(), path[depth]));
        }
    }
    None
}

fn resize_check(node: &CellNode, axis: Axis) -> u16 {
    if !node.tiled() {
        return 0;
    }
    match node {
        CellNode::Float { .. } => 0,
        CellNode::Leaf { geometry, .. } => geometry.extent(axis).saturating_sub(PANE_MINIMUM),
        CellNode::Node {
            axis: node_axis,
            children,
            ..
        } if *node_axis == axis => children.iter().fold(0_u16, |available, child| {
            available.saturating_add(resize_check(&child.node, axis))
        }),
        CellNode::Node { children, .. } => children
            .iter()
            .filter(|child| child.node.tiled())
            .map(|child| resize_check(&child.node, axis))
            .min()
            .unwrap_or(0),
    }
}

fn resize_adjust(node: &mut CellNode, axis: Axis, change: i32) {
    if change == 0 {
        return;
    }
    let size = i32::from(node.extent(axis)) + change;
    let size = u16::try_from(size).unwrap_or(if size < 0 { 0 } else { u16::MAX });
    node.set_extent(axis, size);
    let CellNode::Node {
        axis: node_axis,
        children,
        ..
    } = node
    else {
        return;
    };
    if *node_axis != axis {
        for child in children.iter_mut().filter(|child| child.node.tiled()) {
            resize_adjust(&mut child.node, axis, change);
        }
        return;
    }
    if !children.iter().any(|child| child.node.tiled()) {
        return;
    }
    let mut remaining = change;
    while remaining != 0 {
        let mut changed = false;
        for child in children.iter_mut().filter(|child| child.node.tiled()) {
            if remaining == 0 {
                break;
            }
            if remaining > 0 {
                resize_adjust(&mut child.node, axis, 1);
                remaining -= 1;
                changed = true;
            } else if resize_check(&child.node, axis) > 0 {
                resize_adjust(&mut child.node, axis, -1);
                remaining += 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn resize_node_to(node: &mut CellNode, axis: Axis, size: u16) {
    let change = i32::from(size) - i32::from(node.extent(axis));
    resize_adjust(node, axis, change);
}

fn new_pane_size(
    previous: u16,
    child: &CellNode,
    axis: Axis,
    size: u16,
    count_left: usize,
    size_left: u16,
) -> u16 {
    if count_left == 1 {
        return size_left;
    }
    let available = resize_check(child, axis);
    let reserved =
        u32::try_from((PANE_MINIMUM + 1) as usize * (count_left - 1)).unwrap_or(u32::MAX);
    let current_minimum = u32::from(child.extent(axis).saturating_sub(available));
    let minimum = reserved.max(current_minimum);
    let proportional = if previous == 0 {
        0
    } else {
        (u32::from(child.extent(axis)) * u32::from(size)) / u32::from(previous)
    };
    let maximum = u32::from(size_left).saturating_sub(minimum);
    proportional.min(maximum).max(u32::from(PANE_MINIMUM)) as u16
}

fn can_set_size(node: &CellNode, axis: Axis, size: u16) -> bool {
    if matches!(node, CellNode::Float { .. }) {
        return size >= PANE_MINIMUM;
    }
    let CellNode::Node {
        axis: node_axis,
        children,
        ..
    } = node
    else {
        return size >= PANE_MINIMUM;
    };
    if *node_axis != axis {
        return children
            .iter()
            .all(|child| can_set_size(&child.node, axis, size));
    }
    let minimum = children.len().saturating_mul(2).saturating_sub(1);
    if usize::from(size) < minimum {
        return false;
    }
    let previous = node.extent(axis);
    let mut available = size;
    for (index, child) in children.iter().enumerate() {
        let child_size = new_pane_size(
            previous,
            &child.node,
            axis,
            size,
            children.len() - index,
            available,
        );
        if index + 1 == children.len() {
            if child_size > available {
                return false;
            }
            available -= child_size;
        } else {
            if child_size + 1 > available {
                return false;
            }
            available -= child_size + 1;
        }
        if !can_set_size(&child.node, axis, child_size) {
            return false;
        }
    }
    true
}

fn resize_child_cells(node: &mut CellNode) {
    let geometry = node.geometry();
    let CellNode::Node { axis, children, .. } = node else {
        return;
    };
    let axis = *axis;
    let count = children.iter().filter(|child| child.node.tiled()).count();
    let previous = children.iter().filter(|child| child.node.tiled()).fold(
        u32::try_from(count.saturating_sub(1)).unwrap_or(u32::MAX),
        |total, child| total + u32::from(child.node.extent(axis)),
    );
    let previous = u16::try_from(previous).unwrap_or(u16::MAX);
    let mut available = geometry.extent(axis);
    for (index, child) in children
        .iter_mut()
        .filter(|child| child.node.tiled())
        .enumerate()
    {
        match axis {
            Axis::Horizontal => {
                child.node.geometry_mut().sy = geometry.sy;
                child.node.geometry_mut().yoff = geometry.yoff;
            }
            Axis::Vertical => {
                child.node.geometry_mut().sx = geometry.sx;
                child.node.geometry_mut().xoff = geometry.xoff;
            }
        }
        let size = new_pane_size(
            previous,
            &child.node,
            axis,
            geometry.extent(axis),
            count - index,
            available,
        );
        child.node.set_extent(axis, size);
        available = available.saturating_sub(size.saturating_add(1));
        resize_child_cells(&mut child.node);
    }
}

fn resize_root_axis(root: &mut CellNode, axis: Axis, target: u16) {
    let current = root.extent(axis);
    let mut change = i32::from(target) - i32::from(current);
    let limit = i32::from(resize_check(root, axis));
    if change < -limit {
        change = -limit;
    }
    if limit == 0 {
        change = if target <= current {
            0
        } else {
            i32::from(target) - i32::from(current)
        };
    }
    resize_adjust(root, axis, change);
}

fn resize_siblings(children: &mut [CellChild], index: usize, axis: Axis, delta: i32) {
    let mut needed = i64::from(delta);
    while needed != 0 {
        if needed > 0 {
            let victim = ((index + 1)..children.len())
                .find(|candidate| resize_check(&children[*candidate].node, axis) > 0)
                .or_else(|| {
                    (0..index)
                        .rev()
                        .find(|candidate| resize_check(&children[*candidate].node, axis) > 0)
                });
            let Some(victim) = victim else {
                break;
            };
            let moved = i64::from(resize_check(&children[victim].node, axis)).min(needed);
            let moved = i32::try_from(moved).unwrap_or(i32::MAX);
            resize_adjust(&mut children[index].node, axis, moved);
            resize_adjust(&mut children[victim].node, axis, -moved);
            needed -= i64::from(moved);
        } else {
            let victim = (0..=index)
                .rev()
                .find(|candidate| resize_check(&children[*candidate].node, axis) > 0);
            let Some(victim) = victim else {
                break;
            };
            let Some(receiver) = neighbour_dir(children, index, true) else {
                break;
            };
            let moved = i64::from(resize_check(&children[victim].node, axis)).min(-needed);
            let moved = i32::try_from(moved).unwrap_or(i32::MAX);
            resize_adjust(&mut children[receiver].node, axis, moved);
            resize_adjust(&mut children[victim].node, axis, -moved);
            needed += i64::from(moved);
        }
    }
}

fn resize_divider(node: &mut CellNode, divider: SplitId, ratio: f32) -> Option<bool> {
    let CellNode::Node { axis, children, .. } = node else {
        return None;
    };
    if let Some(index) = children
        .iter()
        .position(|child| child.divider == Some(divider) && child.node.tiled())
    {
        let previous = neighbour_dir(children, index, false)?;
        let rest = children[index..]
            .iter()
            .filter(|child| child.node.tiled())
            .collect::<Vec<_>>();
        let span = rest.iter().fold(
            u32::try_from(rest.len().saturating_sub(1)).unwrap_or(u32::MAX),
            |total, child| total.saturating_add(u32::from(child.node.extent(*axis))),
        );
        let span = span.saturating_add(u32::from(children[previous].node.extent(*axis)));
        let current = children[previous].node.extent(*axis);
        let extents = children
            .iter()
            .map(|child| child.node.extent(*axis))
            .collect::<Vec<_>>();
        let target = (ratio.clamp(0.0, 1.0) * span as f32).round() as i32;
        let delta = target - i32::from(current);
        resize_siblings(children, previous, *axis, delta);
        return Some(
            children
                .iter()
                .zip(extents)
                .any(|(child, extent)| child.node.extent(*axis) != extent),
        );
    }
    children
        .iter_mut()
        .find_map(|child| resize_divider(&mut child.node, divider, ratio))
}

fn collapse_single_child(root: &mut CellNode, parent_path: &[usize]) {
    if parent_path.is_empty() {
        let replacement = match root {
            CellNode::Node { children, .. } if children.len() == 1 => Some(children.remove(0).node),
            _ => None,
        };
        if let Some(replacement) = replacement {
            *root = replacement;
        }
        return;
    }
    let grandparent_path = &parent_path[..parent_path.len() - 1];
    let index = parent_path[parent_path.len() - 1];
    let Some(CellNode::Node { children, .. }) = node_at_path_mut(root, grandparent_path) else {
        return;
    };
    let parent = children.remove(index);
    let slot_divider = parent.divider;
    let (parent_axis, parent_geometry, mut parent_children) = match parent.node {
        CellNode::Node {
            axis,
            geometry,
            children,
        } => (axis, geometry, children),
        node @ (CellNode::Leaf { .. } | CellNode::Float { .. }) => {
            children.insert(
                index,
                CellChild {
                    divider: slot_divider,
                    node,
                },
            );
            return;
        }
    };
    if parent_children.len() != 1 {
        children.insert(
            index,
            CellChild {
                divider: slot_divider,
                node: CellNode::Node {
                    axis: parent_axis,
                    geometry: parent_geometry,
                    children: parent_children,
                },
            },
        );
        return;
    }
    children.insert(
        index,
        CellChild {
            divider: slot_divider,
            node: parent_children.remove(0).node,
        },
    );
}

#[allow(clippy::fn_params_excessive_bools)]
pub(crate) fn split_floating_cell(
    old: CellGeometry,
    window: (u16, u16),
    border: bool,
    horizontal: bool,
    before: bool,
    full: bool,
) -> Option<(CellGeometry, CellGeometry)> {
    let (top, bottom) = (1, i32::from(window.1) - 1);
    let (left, right) = (3, i32::from(window.0) - 3);
    let border = i32::from(border);
    let (mut kept_x, mut kept_y) = (old.xoff, old.yoff);
    let (mut kept_w, mut kept_h) = (i32::from(old.sx), i32::from(old.sy));
    if left > kept_x - border {
        kept_x = left + border;
    }
    if right < kept_x + kept_w + border {
        kept_x = right - kept_w - border;
    }
    if top > kept_y - border {
        kept_y = top + border;
    }
    if bottom < kept_y + kept_h + border {
        kept_y = bottom - kept_h - border;
    }
    let (mut made_x, mut made_y, mut made_w, mut made_h) = (kept_x, kept_y, kept_w, kept_h);
    if horizontal {
        if before {
            made_x -= kept_w + 2 * border;
        } else {
            made_x += kept_w + 2 * border;
        }
    } else if before {
        made_y -= kept_h + 2 * border;
    } else {
        made_y += kept_h + 2 * border;
    }
    if left > made_x - border {
        let space = kept_x + kept_w - left - 3 * border + 1;
        let size = space / 2;
        made_w = size;
        kept_w = size;
        made_x = left + border;
        kept_x = made_x + made_w + 2 * border;
        if space % 2 == 0 {
            kept_w -= 1;
        }
    } else if right < made_x + made_w + border {
        let space = right - kept_x - 3 * border + 1;
        let size = space / 2;
        made_w = size;
        kept_w = size;
        made_x = kept_x + kept_w + 2 * border;
        if space % 2 == 0 {
            made_w -= 1;
        }
    } else if top > made_y - border {
        let space = kept_h + kept_y - top - 3 * border + 1;
        let size = space / 2;
        made_h = size;
        kept_h = size;
        made_y = top + border;
        kept_y = made_y + made_h + 2 * border;
        if space % 2 == 0 {
            kept_h -= 1;
        }
    } else if bottom < made_y + made_h + border {
        let space = bottom - kept_y - 3 * border + 1;
        let size = space / 2;
        made_h = size;
        kept_h = size;
        made_y = kept_y + kept_h + 2 * border;
        if space % 2 == 0 {
            made_h -= 1;
        }
    }
    if full {
        if horizontal {
            made_y = top + border;
            made_h = bottom - top - 2 * border;
            if before {
                made_x = left + border;
                made_w = kept_x - made_x - 2 * border;
            } else {
                made_w = right - made_x - border;
            }
        } else {
            made_x = left + border;
            made_w = right - left - 2 * border;
            if before {
                made_y = top + border;
                made_h = kept_y - made_y - 2 * border;
            } else {
                made_h = bottom - made_y - border;
            }
        }
    }
    let minimum = i32::from(PANE_MINIMUM);
    if made_w < minimum || made_h < minimum || kept_w < minimum || kept_h < minimum {
        return None;
    }
    let size = |value: i32| u16::try_from(value).unwrap_or(u16::MAX);
    Some((
        CellGeometry {
            sx: size(kept_w),
            sy: size(kept_h),
            xoff: kept_x,
            yoff: kept_y,
        },
        CellGeometry {
            sx: size(made_w),
            sy: size(made_h),
            xoff: made_x,
            yoff: made_y,
        },
    ))
}

fn link_floats(
    children: &mut Vec<CellChild>,
    order: &[PaneId],
    floats: &BTreeMap<PaneId, CellGeometry>,
    ids: &mut dyn FnMut() -> SplitId,
) {
    let mut dividers = children
        .iter()
        .filter_map(|child| child.divider)
        .collect::<Vec<_>>()
        .into_iter();
    let mut tiled = std::mem::take(children).into_iter();
    for pane in order {
        let node = match floats.get(pane) {
            Some(geometry) => CellNode::Float {
                pane: *pane,
                geometry: *geometry,
            },
            None => match tiled.next() {
                Some(child) => child.node,
                None => break,
            },
        };
        let divider = if children.is_empty() {
            None
        } else {
            Some(dividers.next().unwrap_or_else(&mut *ids))
        };
        children.push(CellChild { divider, node });
    }
}

fn neighbour_dir(children: &[CellChild], index: usize, forward: bool) -> Option<usize> {
    if forward {
        ((index + 1)..children.len()).find(|candidate| children[*candidate].node.tiled())
    } else {
        (0..index)
            .rev()
            .find(|candidate| children[*candidate].node.tiled())
    }
}

fn cell_neighbour(children: &[CellChild], index: usize) -> Option<usize> {
    let forward = index + 1 != children.len();
    neighbour_dir(children, index, forward).or_else(|| neighbour_dir(children, index, !forward))
}

fn last_tiled(children: &[CellChild], index: usize) -> bool {
    children
        .iter()
        .rposition(|child| child.node.tiled())
        .is_none_or(|last| last == index)
}

fn first_tiled_leaf(node: &CellNode) -> Option<&CellNode> {
    match node {
        CellNode::Leaf { .. } => Some(node),
        CellNode::Float { .. } => None,
        CellNode::Node { children, .. } => children
            .iter()
            .find_map(|child| first_tiled_leaf(&child.node)),
    }
}

fn remove_tile(root: &mut CellNode, path: &[usize]) -> bool {
    if matches!(
        node_at_path(root, path),
        Some(CellNode::Float { .. }) | None
    ) {
        return false;
    }
    let Some((&index, parent_path)) = path.split_last() else {
        return true;
    };
    let neighbour = match node_at_path(root, parent_path) {
        Some(CellNode::Node { children, .. }) => cell_neighbour(children, index),
        _ => return false,
    };
    match neighbour {
        None => {
            remove_tile(root, parent_path);
        }
        Some(neighbour) => {
            if let Some(CellNode::Node { axis, children, .. }) = node_at_path_mut(root, parent_path)
            {
                let change = i32::from(children[index].node.extent(*axis)) + 1;
                resize_adjust(&mut children[neighbour].node, *axis, change);
            }
        }
    }
    if let Some(node) = node_at_path_mut(root, path) {
        *node.geometry_mut() = CellGeometry::default();
    }
    true
}

fn insert_tile(root: &mut CellNode, path: &[usize], extent: (u16, u16)) -> bool {
    if matches!(node_at_path(root, path), Some(CellNode::Leaf { .. }) | None) {
        return false;
    }
    let Some((&index, parent_path)) = path.split_last() else {
        *root.geometry_mut() = CellGeometry {
            sx: extent.0,
            sy: extent.1,
            xoff: 0,
            yoff: 0,
        };
        return true;
    };
    let (axis, neighbour) = match node_at_path(root, parent_path) {
        Some(CellNode::Node { axis, children, .. }) => (*axis, cell_neighbour(children, index)),
        _ => return false,
    };
    match neighbour {
        None => {
            insert_tile(root, parent_path, extent);
            let Some(CellNode::Node {
                geometry, children, ..
            }) = node_at_path_mut(root, parent_path)
            else {
                return false;
            };
            let size = geometry.extent(axis);
            resize_node_to(&mut children[index].node, axis, size);
        }
        Some(neighbour) => {
            let Some(CellNode::Node { children, .. }) = node_at_path_mut(root, parent_path) else {
                return false;
            };
            let span = children[neighbour].node.extent(axis);
            if first_tiled_leaf(&children[neighbour].node).is_none() || span < PANE_MINIMUM * 2 + 1
            {
                return false;
            }
            let second = (span.div_ceil(2) - 1).clamp(PANE_MINIMUM, span - 2);
            let first = span - 1 - second;
            resize_node_to(&mut children[index].node, axis, first);
            resize_node_to(&mut children[neighbour].node, axis, second);
        }
    }
    let Some(CellNode::Node {
        geometry, children, ..
    }) = node_at_path_mut(root, parent_path)
    else {
        return false;
    };
    let cross = match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    };
    let size = geometry.extent(cross);
    resize_node_to(&mut children[index].node, cross, size);
    true
}

fn clamp_floats(node: &mut CellNode, sx: u16, sy: u16, pad_of: &dyn Fn(PaneId) -> u16) {
    match node {
        CellNode::Leaf { .. } => {}
        CellNode::Node { children, .. } => {
            for child in children {
                clamp_floats(&mut child.node, sx, sy, pad_of);
            }
        }
        CellNode::Float { pane, geometry } => {
            let pad = pad_of(*pane);
            let fit = |size: u16, window: u16| {
                let available = window.saturating_sub(2 * pad);
                if size > available {
                    available.max(PANE_MINIMUM)
                } else {
                    size
                }
            };
            geometry.sx = fit(geometry.sx, sx);
            geometry.sy = fit(geometry.sy, sy);
            let place = |offset: i32, size: u16, window: u16| {
                let (pad, size, window) = (i32::from(pad), i32::from(size), i32::from(window));
                if offset + size + pad > window {
                    if size + 2 * pad >= window {
                        pad
                    } else {
                        window - size - pad
                    }
                } else {
                    offset
                }
            };
            geometry.xoff = place(geometry.xoff, geometry.sx, sx);
            geometry.yoff = place(geometry.yoff, geometry.sy, sy);
        }
    }
}

enum SpreadCell {
    Changed,
    Unchanged,
    Refused,
}

fn spread_node(node: &mut CellNode) -> SpreadCell {
    let CellNode::Node {
        axis,
        geometry,
        children,
    } = node
    else {
        return SpreadCell::Unchanged;
    };
    let leaves = children
        .iter()
        .filter(|child| matches!(child.node, CellNode::Leaf { .. }))
        .count();
    if leaves <= 1 {
        return SpreadCell::Unchanged;
    }
    if children
        .iter()
        .any(|child| matches!(child.node, CellNode::Node { .. }) && child.node.tiled())
    {
        return SpreadCell::Refused;
    }
    let count = leaves;
    let size = usize::from(geometry.extent(*axis));
    if size < count - 1 {
        return SpreadCell::Unchanged;
    }
    let each = (size - (count - 1)) / count;
    if each == 0 {
        return SpreadCell::Unchanged;
    }
    let mut remainder = size - (count - 1) - each * count;
    let mut changed = false;
    for child in children
        .iter_mut()
        .filter(|child| matches!(child.node, CellNode::Leaf { .. }))
    {
        let extra = usize::from(remainder > 0);
        remainder = remainder.saturating_sub(extra);
        let target = u16::try_from(each + extra).unwrap_or(u16::MAX);
        let change = i32::from(target) - i32::from(child.node.extent(*axis));
        resize_adjust(&mut child.node, *axis, change);
        changed |= change != 0;
    }
    if changed {
        SpreadCell::Changed
    } else {
        SpreadCell::Unchanged
    }
}

fn make_children(
    panes: &[PaneId],
    sx: u16,
    sy: u16,
    ids: &mut dyn FnMut() -> SplitId,
) -> Vec<CellChild> {
    let mut children = Vec::with_capacity(panes.len());
    for (index, pane) in panes.iter().enumerate() {
        children.push(CellChild {
            divider: if index == 0 { None } else { Some(ids()) },
            node: leaf(*pane, sx, sy),
        });
    }
    children
}

fn even_layout(
    axis: Axis,
    sx: u16,
    sy: u16,
    panes: &[PaneId],
    ids: &mut dyn FnMut() -> SplitId,
) -> CellNode {
    let minimum =
        u16::try_from(panes.len().saturating_mul(2).saturating_sub(1)).unwrap_or(u16::MAX);
    let (layout_width, layout_height) = match axis {
        Axis::Horizontal => (sx.max(minimum), sy),
        Axis::Vertical => (sx, sy.max(minimum)),
    };
    let mut root = CellNode::Node {
        axis,
        geometry: CellGeometry {
            sx: layout_width,
            sy: layout_height,
            xoff: 0,
            yoff: 0,
        },
        children: make_children(panes, layout_width, layout_height, ids),
    };
    spread_node(&mut root);
    root
}

fn main_horizontal(
    sx: u16,
    sy: u16,
    panes: &[PaneId],
    mirrored: bool,
    options: &PresetOptions,
    ids: &mut dyn FnMut() -> SplitId,
) -> CellNode {
    let others = panes.len() - 1;
    let available = sy.saturating_sub(1);
    let mut main = layout_option_cells(&options.main_pane_height, available).unwrap_or(24);
    let other;
    if main.saturating_add(PANE_MINIMUM) >= available {
        main = if available <= PANE_MINIMUM * 2 {
            PANE_MINIMUM
        } else {
            available - PANE_MINIMUM
        };
        other = PANE_MINIMUM;
    } else {
        let configured_other = layout_option_cells(&options.other_pane_height, available);
        if configured_other
            .is_none_or(|other| other == 0 || other > available || available - other < main)
        {
            other = available - main;
        } else {
            other = configured_other.expect("other pane height was checked");
            main = available - other;
        }
    }
    let minimum_width =
        u16::try_from(others.saturating_mul(2).saturating_sub(1)).unwrap_or(u16::MAX);
    let layout_width = sx.max(minimum_width);
    let layout_height = main + other + 1;
    let main_node = leaf(panes[0], layout_width, main);
    let other_node = if others == 1 {
        leaf(panes[1], layout_width, other)
    } else {
        let mut node = CellNode::Node {
            axis: Axis::Horizontal,
            geometry: CellGeometry {
                sx: layout_width,
                sy: other,
                xoff: 0,
                yoff: 0,
            },
            children: make_children(&panes[1..], PANE_MINIMUM, other, ids),
        };
        spread_node(&mut node);
        node
    };
    let divider = ids();
    let children = if mirrored {
        vec![
            CellChild {
                divider: None,
                node: other_node,
            },
            CellChild {
                divider: Some(divider),
                node: main_node,
            },
        ]
    } else {
        vec![
            CellChild {
                divider: None,
                node: main_node,
            },
            CellChild {
                divider: Some(divider),
                node: other_node,
            },
        ]
    };
    CellNode::Node {
        axis: Axis::Vertical,
        geometry: CellGeometry {
            sx: layout_width,
            sy: layout_height,
            xoff: 0,
            yoff: 0,
        },
        children,
    }
}

fn main_vertical(
    sx: u16,
    sy: u16,
    panes: &[PaneId],
    mirrored: bool,
    options: &PresetOptions,
    ids: &mut dyn FnMut() -> SplitId,
) -> CellNode {
    let others = panes.len() - 1;
    let available = sx.saturating_sub(1);
    let mut main = layout_option_cells(&options.main_pane_width, available).unwrap_or(80);
    let other;
    if main.saturating_add(PANE_MINIMUM) >= available {
        main = if available <= PANE_MINIMUM * 2 {
            PANE_MINIMUM
        } else {
            available - PANE_MINIMUM
        };
        other = PANE_MINIMUM;
    } else {
        let configured_other = layout_option_cells(&options.other_pane_width, available);
        if configured_other
            .is_none_or(|other| other == 0 || other > available || available - other < main)
        {
            other = available - main;
        } else {
            other = configured_other.expect("other pane width was checked");
            main = available - other;
        }
    }
    let minimum_height =
        u16::try_from(others.saturating_mul(2).saturating_sub(1)).unwrap_or(u16::MAX);
    let layout_width = main + other + 1;
    let layout_height = sy.max(minimum_height);
    let main_node = leaf(panes[0], main, layout_height);
    let other_node = if others == 1 {
        leaf(panes[1], other, layout_height)
    } else {
        let mut node = CellNode::Node {
            axis: Axis::Vertical,
            geometry: CellGeometry {
                sx: other,
                sy: layout_height,
                xoff: 0,
                yoff: 0,
            },
            children: make_children(&panes[1..], other, PANE_MINIMUM, ids),
        };
        spread_node(&mut node);
        node
    };
    let divider = ids();
    let children = if mirrored {
        vec![
            CellChild {
                divider: None,
                node: other_node,
            },
            CellChild {
                divider: Some(divider),
                node: main_node,
            },
        ]
    } else {
        vec![
            CellChild {
                divider: None,
                node: main_node,
            },
            CellChild {
                divider: Some(divider),
                node: other_node,
            },
        ]
    };
    CellNode::Node {
        axis: Axis::Horizontal,
        geometry: CellGeometry {
            sx: layout_width,
            sy: layout_height,
            xoff: 0,
            yoff: 0,
        },
        children,
    }
}

fn tiled_layout(
    sx: u16,
    sy: u16,
    panes: &[PaneId],
    max_columns: u16,
    ids: &mut dyn FnMut() -> SplitId,
) -> CellNode {
    let count = panes.len();
    let mut rows = 1_usize;
    let mut columns = 1_usize;
    while rows * columns < count {
        rows += 1;
        if rows * columns < count && (max_columns == 0 || columns < usize::from(max_columns)) {
            columns += 1;
        }
    }
    let width = usize::from(sx)
        .saturating_sub(columns - 1)
        .checked_div(columns)
        .unwrap_or(0)
        .max(usize::from(PANE_MINIMUM));
    let height = usize::from(sy)
        .saturating_sub(rows - 1)
        .checked_div(rows)
        .unwrap_or(0)
        .max(usize::from(PANE_MINIMUM));
    let layout_width = usize::from(sx).max((width + 1) * columns - 1);
    let layout_height = usize::from(sy).max((height + 1) * rows - 1);
    let layout_width = u16::try_from(layout_width).unwrap_or(u16::MAX);
    let layout_height = u16::try_from(layout_height).unwrap_or(u16::MAX);
    let width = u16::try_from(width).unwrap_or(u16::MAX);
    let height = u16::try_from(height).unwrap_or(u16::MAX);
    let mut row_children = Vec::new();
    let mut start = 0;
    while start < count {
        let end = (start + columns).min(count);
        let row_panes = &panes[start..end];
        let row_node = if row_panes.len() == 1 || columns == 1 {
            leaf(row_panes[0], layout_width, height)
        } else {
            let mut children = make_children(row_panes, width, height, ids);
            let used = row_panes.len() * (usize::from(width) + 1) - 1;
            if usize::from(layout_width) > used {
                let extra = u16::try_from(usize::from(layout_width) - used).unwrap_or(u16::MAX);
                if let Some(last) = children.last_mut() {
                    resize_adjust(&mut last.node, Axis::Horizontal, i32::from(extra));
                }
            }
            CellNode::Node {
                axis: Axis::Horizontal,
                geometry: CellGeometry {
                    sx: layout_width,
                    sy: height,
                    xoff: 0,
                    yoff: 0,
                },
                children,
            }
        };
        let divider = if row_children.is_empty() {
            None
        } else {
            Some(ids())
        };
        row_children.push(CellChild {
            divider,
            node: row_node,
        });
        start = end;
    }
    let used = row_children.len() * usize::from(height) + row_children.len() - 1;
    if usize::from(layout_height) > used {
        let extra = u16::try_from(usize::from(layout_height) - used).unwrap_or(u16::MAX);
        if let Some(last) = row_children.last_mut() {
            resize_adjust(&mut last.node, Axis::Vertical, i32::from(extra));
        }
    }
    CellNode::Node {
        axis: Axis::Vertical,
        geometry: CellGeometry {
            sx: layout_width,
            sy: layout_height,
            xoff: 0,
            yoff: 0,
        },
        children: row_children,
    }
}

fn layout_option_cells(value: &str, available: u16) -> Option<u16> {
    let cells = if let Some(percentage) = value.strip_suffix('%') {
        let percentage = percentage.parse::<u16>().ok()?;
        if percentage > 1000 {
            return None;
        }
        u32::from(available) * u32::from(percentage) / 100
    } else {
        value.parse::<u32>().ok()?
    };
    (cells <= u32::from(available)).then(|| u16::try_from(cells).expect("cells fit available"))
}

fn fix_offsets(root: &mut CellNode) {
    if matches!(root, CellNode::Float { .. }) {
        return;
    }
    root.geometry_mut().xoff = 0;
    root.geometry_mut().yoff = 0;
    fix_child_offsets(root);
}

fn fix_child_offsets(node: &mut CellNode) {
    let geometry = node.geometry();
    let CellNode::Node { axis, children, .. } = node else {
        return;
    };
    let mut offset = match axis {
        Axis::Horizontal => geometry.xoff,
        Axis::Vertical => geometry.yoff,
    };
    for child in children.iter_mut().filter(|child| child.node.tiled()) {
        match axis {
            Axis::Horizontal => {
                child.node.geometry_mut().xoff = offset;
                child.node.geometry_mut().yoff = geometry.yoff;
            }
            Axis::Vertical => {
                child.node.geometry_mut().xoff = geometry.xoff;
                child.node.geometry_mut().yoff = offset;
            }
        }
        fix_child_offsets(&mut child.node);
        offset = offset.saturating_add(i32::from(child.node.extent(*axis)) + 1);
    }
}

fn swap_panes(node: &mut CellNode, a: PaneId, b: PaneId) {
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } if *pane == a => *pane = b,
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } if *pane == b => *pane = a,
        CellNode::Leaf { .. } | CellNode::Float { .. } => {}
        CellNode::Node { children, .. } => {
            for child in children {
                swap_panes(&mut child.node, a, b);
            }
        }
    }
}

fn replace_pane(node: &mut CellNode, old: PaneId, new: PaneId) -> bool {
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } if *pane == old => {
            *pane = new;
            true
        }
        CellNode::Leaf { .. } | CellNode::Float { .. } => false,
        CellNode::Node { children, .. } => children
            .iter_mut()
            .any(|child| replace_pane(&mut child.node, old, new)),
    }
}

fn remap_panes(node: &mut CellNode, mapping: &BTreeMap<PaneId, PaneId>) {
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } => {
            if let Some(mapped) = mapping.get(pane) {
                *pane = *mapped;
            }
        }
        CellNode::Node { children, .. } => {
            for child in children {
                remap_panes(&mut child.node, mapping);
            }
        }
    }
}

fn replace_panes_in_order(node: &mut CellNode, panes: &mut impl Iterator<Item = PaneId>) {
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } => {
            *pane = panes.next().expect("pane count was checked");
        }
        CellNode::Node { children, .. } => {
            for child in children {
                replace_panes_in_order(&mut child.node, panes);
            }
        }
    }
}

fn refresh_divider_ids(node: &mut CellNode, ids: &mut dyn FnMut() -> SplitId) {
    let CellNode::Node { children, .. } = node else {
        return;
    };
    refresh_child_divider_ids(children, 0, ids);
}

fn refresh_child_divider_ids(
    children: &mut [CellChild],
    index: usize,
    ids: &mut dyn FnMut() -> SplitId,
) {
    if index + 1 == children.len() {
        refresh_divider_ids(&mut children[index].node, ids);
        return;
    }
    children[index + 1].divider = Some(ids());
    refresh_divider_ids(&mut children[index].node, ids);
    refresh_child_divider_ids(children, index + 1, ids);
}

fn project_node(node: &CellNode) -> Option<LayoutNode> {
    match node {
        CellNode::Leaf { pane, .. } => Some(LayoutNode::Pane(*pane)),
        CellNode::Float { .. } => None,
        CellNode::Node { axis, children, .. } => {
            let tiled = children
                .iter()
                .filter(|child| child.node.tiled())
                .collect::<Vec<_>>();
            (!tiled.is_empty()).then(|| project_children(&tiled, *axis, 0))
        }
    }
}

fn project_children(children: &[&CellChild], axis: Axis, index: usize) -> LayoutNode {
    if index + 1 == children.len() {
        return project_node(&children[index].node).expect("tiled child projects");
    }
    let first = project_node(&children[index].node).expect("tiled child projects");
    let second = project_children(children, axis, index + 1);
    let first_extent = u32::from(children[index].node.extent(axis));
    let remaining_borders = children.len() - index - 2;
    let rest_extent = children[index + 1..].iter().fold(
        u32::try_from(remaining_borders).unwrap_or(u32::MAX),
        |total, child| total + u32::from(child.node.extent(axis)),
    );
    let denominator = first_extent + rest_extent;
    let ratio = if denominator == 0 {
        0.0
    } else {
        (first_extent as f32 / denominator as f32).clamp(0.0, 1.0)
    };
    let id = children[index + 1].divider.unwrap_or_else(|| {
        debug_assert!(false, "missing projected divider");
        SplitId(0)
    });
    LayoutNode::Split {
        id,
        axis,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn dump_node(node: &CellNode, output: &mut String) {
    let geometry = node.geometry();
    let _ = write!(
        output,
        "{}x{},{},{}",
        geometry.sx, geometry.sy, geometry.xoff, geometry.yoff
    );
    match node {
        CellNode::Leaf { pane, .. } | CellNode::Float { pane, .. } => {
            let _ = write!(output, ",{}", pane.0);
        }
        CellNode::Node { axis, children, .. } => {
            output.push(match axis {
                Axis::Horizontal => '{',
                Axis::Vertical => '[',
            });
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                dump_node(&child.node, output);
            }
            output.push(match axis {
                Axis::Horizontal => '}',
                Axis::Vertical => ']',
            });
        }
    }
}

fn validate_node(node: &CellNode, dividers: &mut BTreeSet<SplitId>) -> Result<(), String> {
    let (axis, geometry, children) = match node {
        CellNode::Leaf { geometry, .. } | CellNode::Float { geometry, .. } => {
            if geometry.sx < PANE_MINIMUM || geometry.sy < PANE_MINIMUM {
                return Err("leaf extent is below the pane minimum".to_owned());
            }
            return Ok(());
        }
        CellNode::Node {
            axis,
            geometry,
            children,
        } => (axis, geometry, children),
    };
    if children.len() < 2 {
        return Err("node has fewer than two children".to_owned());
    }
    let mut extent = 0_u32;
    let mut expected_offset = match axis {
        Axis::Horizontal => geometry.xoff,
        Axis::Vertical => geometry.yoff,
    };
    for (index, child) in children.iter().enumerate() {
        if index == 0 {
            if child.divider.is_some() {
                return Err("first child has a divider".to_owned());
            }
        } else {
            let Some(divider) = child.divider else {
                return Err("non-first child is missing a divider".to_owned());
            };
            if !dividers.insert(divider) {
                return Err(format!("duplicate divider {}", divider.0));
            }
        }
        if !child.node.tiled() {
            validate_node(&child.node, dividers)?;
            continue;
        }
        let child_geometry = child.node.geometry();
        match axis {
            Axis::Horizontal => {
                if child_geometry.sy != geometry.sy {
                    return Err("horizontal child height differs from parent".to_owned());
                }
                if child_geometry.xoff != expected_offset || child_geometry.yoff != geometry.yoff {
                    return Err("horizontal child offset is inconsistent".to_owned());
                }
            }
            Axis::Vertical => {
                if child_geometry.sx != geometry.sx {
                    return Err("vertical child width differs from parent".to_owned());
                }
                if child_geometry.xoff != geometry.xoff || child_geometry.yoff != expected_offset {
                    return Err("vertical child offset is inconsistent".to_owned());
                }
            }
        }
        validate_node(&child.node, dividers)?;
        let child_extent = child.node.extent(*axis);
        extent += u32::from(child_extent) + 1;
        expected_offset = expected_offset.saturating_add(i32::from(child_extent) + 1);
    }
    if extent != 0 && extent - 1 != u32::from(geometry.extent(*axis)) {
        return Err("child extents do not fill parent".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allocator(start: u64) -> impl FnMut() -> SplitId {
        let mut next = start;
        move || {
            let id = SplitId(next);
            next += 1;
            id
        }
    }

    fn checksummed(body: &str) -> String {
        format!("{:04x},{body}", layout_checksum(body.as_bytes()))
    }

    fn nested_layout(depth: usize) -> (String, u16) {
        if depth == 0 {
            return ("1x1,0,0,0".to_owned(), 1);
        }
        let (child, child_width) = nested_layout(depth - 1);
        let width = child_width + 2;
        (format!("{width}x1,0,0{{{child},1x1,0,0,0}}"), width)
    }

    fn geometry(layout: &CellLayout, pane: u64) -> CellGeometry {
        layout.pane_geometry(PaneId(pane)).unwrap()
    }

    fn sizes(layout: &CellLayout, panes: &[PaneId]) -> Vec<(u16, u16)> {
        panes
            .iter()
            .map(|pane| {
                let geometry = layout.pane_geometry(*pane).unwrap();
                (geometry.sx, geometry.sy)
            })
            .collect()
    }

    fn three_horizontal() -> CellLayout {
        let mut layout = CellLayout::new(PaneId(0), 80, 24);
        let mut ids = allocator(1);
        layout
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        layout
            .split(
                PaneId(1),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(2),
                &mut ids,
            )
            .unwrap();
        layout
    }

    #[test]
    fn split_sizes_before_full_and_no_space_match_tmux() {
        let mut ids = allocator(1);
        let mut default = CellLayout::new(PaneId(0), 80, 24);
        default
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        assert_eq!(
            sizes(&default, &[PaneId(0), PaneId(1)]),
            [(40, 24), (39, 24)]
        );

        let mut ids = allocator(1);
        let mut cells = CellLayout::new(PaneId(0), 80, 24);
        cells
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Cells(30),
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        assert_eq!(sizes(&cells, &[PaneId(0), PaneId(1)]), [(49, 24), (30, 24)]);

        let mut ids = allocator(1);
        let mut percent = CellLayout::new(PaneId(0), 80, 24);
        percent
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Percent(25),
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        assert_eq!(
            sizes(&percent, &[PaneId(0), PaneId(1)]),
            [(59, 24), (20, 24)]
        );

        let mut ids = allocator(1);
        let mut before = CellLayout::new(PaneId(0), 80, 24);
        before
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                true,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        assert_eq!(before.panes_in_order(), [PaneId(1), PaneId(0)]);
        assert_eq!(
            sizes(&before, &[PaneId(1), PaneId(0)]),
            [(40, 24), (39, 24)]
        );

        let mut ids = allocator(1);
        let mut full = CellLayout::new(PaneId(0), 80, 24);
        full.split(
            PaneId(0),
            Axis::Horizontal,
            SplitSize::Default,
            false,
            false,
            PaneId(1),
            &mut ids,
        )
        .unwrap();
        full.split(
            PaneId(0),
            Axis::Vertical,
            SplitSize::Default,
            false,
            true,
            PaneId(2),
            &mut ids,
        )
        .unwrap();
        assert_eq!(
            full.dump(),
            "eb1f,80x24,0,0[80x12,0,0{40x12,0,0,0,39x12,41,0,1},80x11,0,13,2]"
        );

        let mut ids = allocator(1);
        let mut same_axis_full = CellLayout::new(PaneId(0), 80, 24);
        same_axis_full
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        same_axis_full
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                true,
                PaneId(2),
                &mut ids,
            )
            .unwrap();
        assert_eq!(
            sizes(&same_axis_full, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(20, 24), (19, 24), (39, 24)]
        );

        let mut ids = allocator(1);
        let mut nested_full = CellLayout::new(PaneId(0), 80, 24);
        nested_full
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        nested_full
            .resize_pane_to(PaneId(0), Axis::Horizontal, 60)
            .unwrap();
        nested_full
            .split(
                PaneId(0),
                Axis::Vertical,
                SplitSize::Default,
                false,
                true,
                PaneId(2),
                &mut ids,
            )
            .unwrap();
        nested_full
            .split(
                PaneId(2),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                true,
                PaneId(3),
                &mut ids,
            )
            .unwrap();
        assert_eq!(
            sizes(&nested_full, &[PaneId(0), PaneId(1), PaneId(2), PaneId(3)]),
            [(30, 12), (9, 12), (40, 11), (39, 24)]
        );

        let mut ids = allocator(1);
        let mut tiny = CellLayout::new(PaneId(0), 2, 24);
        assert_eq!(
            tiny.split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            ),
            Err(LayoutError::NoSpace)
        );

        let mut ids = allocator(1);
        let mut over_percent = CellLayout::new(PaneId(0), 80, 24);
        over_percent
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Percent(200),
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        assert_eq!(
            sizes(&over_percent, &[PaneId(0), PaneId(1)]),
            [(1, 24), (78, 24)]
        );

        let mut allocated = 0;
        let result = {
            let mut ids = || {
                allocated += 1;
                SplitId(allocated)
            };
            default.split(
                PaneId(99),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(2),
                &mut ids,
            )
        };
        assert_eq!(result, Err(LayoutError::UnknownPane));
        assert_eq!(allocated, 0);
    }

    #[test]
    fn remove_gifts_after_and_preserves_promoted_same_axis_nodes() {
        let mut ids = allocator(1);
        let mut layout = CellLayout::new(PaneId(0), 80, 24);
        layout
            .split(
                PaneId(0),
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        layout
            .split(
                PaneId(1),
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                PaneId(2),
                &mut ids,
            )
            .unwrap();
        layout.remove(PaneId(0)).unwrap();
        assert_eq!(sizes(&layout, &[PaneId(1), PaneId(2)]), [(80, 18), (80, 5)]);

        let mut ids = allocator(1);
        let mut nested = CellLayout::new(PaneId(0), 80, 24);
        nested
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        nested
            .split(
                PaneId(1),
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                PaneId(2),
                &mut ids,
            )
            .unwrap();
        nested
            .split(
                PaneId(1),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(3),
                &mut ids,
            )
            .unwrap();
        nested.remove(PaneId(2)).unwrap();
        assert_eq!(nested.panes_in_order(), [PaneId(0), PaneId(1), PaneId(3)]);
        let CellNode::Node { axis, children, .. } = &nested.root else {
            panic!("expected root node");
        };
        assert_eq!(*axis, Axis::Horizontal);
        assert_eq!(children.len(), 2);
        assert_eq!(children[1].divider, Some(SplitId(1)));
        let CellNode::Node {
            axis: promoted_axis,
            children: promoted_children,
            ..
        } = &children[1].node
        else {
            panic!("expected promoted subtree");
        };
        assert_eq!(*promoted_axis, Axis::Horizontal);
        assert_eq!(promoted_children.len(), 2);
        assert!(nested.validate().is_ok());
    }

    #[test]
    fn remove_reports_last_pane_and_guards_single_child_nodes() {
        let mut root_leaf = CellLayout::new(PaneId(0), 80, 24);
        assert_eq!(root_leaf.remove(PaneId(0)), Err(LayoutError::LastPane));

        let mut malformed = CellLayout {
            root: CellNode::Node {
                axis: Axis::Horizontal,
                geometry: CellGeometry {
                    sx: 80,
                    sy: 24,
                    xoff: 0,
                    yoff: 0,
                },
                children: vec![CellChild {
                    divider: None,
                    node: leaf(PaneId(0), 80, 24),
                }],
            },
            extent: (80, 24),
        };
        assert_eq!(malformed.remove(PaneId(0)), Ok(()));
        assert_eq!(
            malformed.resize_pane(PaneId(0), Axis::Horizontal, 1),
            Ok(())
        );
    }

    #[test]
    fn window_resize_uses_round_robin_with_first_remainders() {
        let mut grown = three_horizontal();
        grown.resize(100, 30);
        assert_eq!(
            sizes(&grown, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(47, 30), (26, 30), (25, 30)]
        );

        let mut shrunk = three_horizontal();
        shrunk.resize(50, 20);
        assert_eq!(
            sizes(&shrunk, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(30, 20), (9, 20), (9, 20)]
        );
    }

    #[test]
    fn pane_resize_walks_victims_steps_back_and_clamps() {
        let mut walked = three_horizontal();
        walked
            .resize_pane(PaneId(1), Axis::Horizontal, -18)
            .unwrap();
        walked.resize_pane(PaneId(0), Axis::Horizontal, 5).unwrap();
        assert_eq!(
            sizes(&walked, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(45, 24), (1, 24), (32, 24)]
        );
        walked.resize_pane(PaneId(2), Axis::Horizontal, 5).unwrap();
        assert_eq!(
            sizes(&walked, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(45, 24), (6, 24), (27, 24)]
        );

        let mut clamped = CellLayout::new(PaneId(0), 80, 24);
        let mut ids = allocator(1);
        clamped
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        clamped
            .resize_pane(PaneId(0), Axis::Horizontal, -100)
            .unwrap();
        assert_eq!(
            sizes(&clamped, &[PaneId(0), PaneId(1)]),
            [(1, 24), (78, 24)]
        );
        clamped
            .resize_pane(PaneId(0), Axis::Horizontal, 100)
            .unwrap();
        assert_eq!(
            sizes(&clamped, &[PaneId(0), PaneId(1)]),
            [(78, 24), (1, 24)]
        );
    }

    #[test]
    fn pane_resize_without_a_matching_ancestor_is_a_silent_noop() {
        let mut layout = CellLayout::new(PaneId(0), 80, 24);
        let mut ids = allocator(1);
        layout
            .split(
                PaneId(0),
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        let before = layout.dump();

        assert_eq!(layout.resize_pane(PaneId(0), Axis::Horizontal, 10), Ok(()));
        assert_eq!(layout.dump(), before);
    }

    #[test]
    fn pane_resize_to_inverts_from_last_child() {
        let mut layout = CellLayout::new(PaneId(0), 80, 24);
        let mut ids = allocator(1);
        layout
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        layout
            .resize_pane_to(PaneId(1), Axis::Horizontal, 30)
            .unwrap();
        assert_eq!(
            sizes(&layout, &[PaneId(0), PaneId(1)]),
            [(49, 24), (30, 24)]
        );
    }

    #[test]
    fn divider_ratio_resizes_the_matching_boundary() {
        let mut layout = three_horizontal();
        assert!(layout.set_divider_ratio(SplitId(2), 0.75).unwrap());
        assert_eq!(
            sizes(&layout, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(40, 24), (29, 24), (9, 24)]
        );
        assert!(!layout.set_divider_ratio(SplitId(2), 0.75).unwrap());
        assert_eq!(
            layout.set_divider_ratio(SplitId(99), 0.5),
            Err(LayoutError::UnknownDivider)
        );

        let mut inner = three_horizontal();
        assert!(!inner.set_divider_ratio(SplitId(1), 0.5).unwrap());
        assert_eq!(
            sizes(&inner, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(40, 24), (19, 24), (19, 24)]
        );
        assert!(inner.set_divider_ratio(SplitId(1), 0.25).unwrap());
        assert_eq!(
            sizes(&inner, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(20, 24), (39, 24), (19, 24)]
        );

        let mut walked = three_horizontal();
        walked
            .resize_pane(PaneId(1), Axis::Horizontal, -18)
            .unwrap();
        assert!(walked.set_divider_ratio(SplitId(2), 0.0).unwrap());
        assert_eq!(
            sizes(&walked, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(39, 24), (1, 24), (38, 24)]
        );
    }

    #[test]
    fn spread_biases_remainders_to_first_children() {
        let mut layout = three_horizontal();
        layout.resize(81, 24);
        layout
            .resize_pane_to(PaneId(0), Axis::Horizontal, 60)
            .unwrap();
        assert!(layout.spread(PaneId(2)).unwrap());
        assert_eq!(
            sizes(&layout, &[PaneId(0), PaneId(1), PaneId(2)]),
            [(27, 24), (26, 24), (26, 24)]
        );
    }

    #[test]
    fn spread_skips_parents_with_node_children_like_the_pin() {
        let mut layout = CellLayout::new(PaneId(0), 80, 24);
        let mut ids = allocator(1);
        layout
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        layout
            .split(
                PaneId(1),
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                PaneId(2),
                &mut ids,
            )
            .unwrap();
        layout
            .resize_pane_to(PaneId(0), Axis::Horizontal, 60)
            .unwrap();
        let before = sizes(&layout, &[PaneId(0), PaneId(1), PaneId(2)]);
        assert!(!layout.spread(PaneId(2)).unwrap());
        assert_eq!(sizes(&layout, &[PaneId(0), PaneId(1), PaneId(2)]), before);
    }

    #[test]
    fn spread_refuses_a_mixed_parent_the_pin_corrupts() {
        let mut layout = three_horizontal();
        let mut ids = allocator(3);
        layout
            .split(
                PaneId(1),
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                PaneId(3),
                &mut ids,
            )
            .unwrap();
        layout
            .resize_pane_to(PaneId(0), Axis::Horizontal, 10)
            .unwrap();
        let panes = [PaneId(0), PaneId(1), PaneId(3), PaneId(2)];
        let before = sizes(&layout, &panes);
        assert!(!layout.spread(PaneId(0)).unwrap());
        assert_eq!(sizes(&layout, &panes), before);
        layout.validate().unwrap();
    }

    #[test]
    fn presets_match_exact_three_and_five_pane_geometry() {
        let panes3 = [PaneId(0), PaneId(1), PaneId(2)];
        let panes5 = [PaneId(0), PaneId(1), PaneId(2), PaneId(3), PaneId(4)];
        let expected3 = [
            [(26, 24), (26, 24), (26, 24)],
            [(80, 8), (80, 7), (80, 7)],
            [(80, 22), (40, 1), (39, 1)],
            [(80, 22), (40, 1), (39, 1)],
            [(78, 24), (1, 12), (1, 11)],
            [(78, 24), (1, 12), (1, 11)],
            [(39, 11), (40, 11), (80, 12)],
        ];
        let expected5 = [
            [(16, 24), (15, 24), (15, 24), (15, 24), (15, 24)],
            [(80, 4), (80, 4), (80, 4), (80, 4), (80, 4)],
            [(80, 22), (20, 1), (19, 1), (19, 1), (19, 1)],
            [(80, 22), (20, 1), (19, 1), (19, 1), (19, 1)],
            [(78, 24), (1, 6), (1, 5), (1, 5), (1, 5)],
            [(78, 24), (1, 6), (1, 5), (1, 5), (1, 5)],
            [(39, 7), (40, 7), (39, 7), (40, 7), (80, 8)],
        ];

        for (index, preset) in LayoutPreset::ALL.into_iter().enumerate() {
            let mut three = CellLayout::new(PaneId(0), 80, 24);
            let mut ids = allocator(1);
            three.apply_preset(preset, &panes3, &PresetOptions::default(), &mut ids);
            assert_eq!(
                sizes(&three, &panes3),
                expected3[index],
                "{}",
                preset.name()
            );

            let mut five = CellLayout::new(PaneId(0), 80, 24);
            let mut ids = allocator(1);
            five.apply_preset(preset, &panes5, &PresetOptions::default(), &mut ids);
            assert_eq!(sizes(&five, &panes5), expected5[index], "{}", preset.name());
        }
    }

    #[test]
    fn preset_options_resolve_percentages_other_sizes_and_tiled_caps_at_apply_time() {
        let panes = [PaneId(0), PaneId(1), PaneId(2)];
        let mut layout = CellLayout::new(PaneId(0), 80, 24);
        let mut ids = allocator(1);
        let options = PresetOptions {
            main_pane_height: "50%".to_owned(),
            ..PresetOptions::default()
        };
        layout.apply_preset(LayoutPreset::MainHorizontal, &panes, &options, &mut ids);
        assert_eq!(sizes(&layout, &panes), [(80, 11), (40, 12), (39, 12)]);

        let mut ids = allocator(10);
        let options = PresetOptions {
            main_pane_height: "50%".to_owned(),
            other_pane_height: "5".to_owned(),
            ..PresetOptions::default()
        };
        layout.apply_preset(LayoutPreset::MainHorizontal, &panes, &options, &mut ids);
        assert_eq!(sizes(&layout, &panes), [(80, 18), (40, 5), (39, 5)]);

        let panes = [PaneId(0), PaneId(1), PaneId(2), PaneId(3), PaneId(4)];
        let mut ids = allocator(20);
        let options = PresetOptions {
            tiled_layout_max_columns: 1,
            ..PresetOptions::default()
        };
        layout.apply_preset(LayoutPreset::Tiled, &panes, &options, &mut ids);
        assert_eq!(sizes(&layout, &panes), [(80, 4); 5]);
    }

    #[test]
    fn two_pane_main_presets_size_the_other_pane() {
        let panes = [PaneId(0), PaneId(1)];
        let cases = [
            (
                LayoutPreset::MainHorizontal,
                [(80, 22), (80, 1)],
                [PaneId(0), PaneId(1)],
            ),
            (
                LayoutPreset::MainHorizontalMirrored,
                [(80, 22), (80, 1)],
                [PaneId(1), PaneId(0)],
            ),
            (
                LayoutPreset::MainVertical,
                [(78, 24), (1, 24)],
                [PaneId(0), PaneId(1)],
            ),
            (
                LayoutPreset::MainVerticalMirrored,
                [(78, 24), (1, 24)],
                [PaneId(1), PaneId(0)],
            ),
        ];
        for (preset, expected_sizes, expected_order) in cases {
            let mut layout = CellLayout::new(PaneId(0), 80, 24);
            let mut ids = allocator(1);
            layout.apply_preset(preset, &panes, &PresetOptions::default(), &mut ids);
            assert_eq!(sizes(&layout, &panes), expected_sizes, "{}", preset.name());
            assert_eq!(layout.panes_in_order(), expected_order, "{}", preset.name());
        }
    }

    fn projected_panes(node: &LayoutNode, panes: &mut Vec<PaneId>) {
        match node {
            LayoutNode::Pane(pane) => panes.push(*pane),
            LayoutNode::Split { first, second, .. } => {
                projected_panes(first, panes);
                projected_panes(second, panes);
            }
        }
    }

    #[test]
    fn projection_is_right_associative_and_keeps_dividers_stable() {
        let mut layout = three_horizontal();
        let projected = layout.project();
        let LayoutNode::Split {
            id, ratio, second, ..
        } = &projected
        else {
            panic!("expected projected split");
        };
        assert_eq!(*id, SplitId(1));
        assert!((*ratio - 40.0 / 79.0).abs() < f32::EPSILON);
        let LayoutNode::Split {
            id: nested_id,
            ratio: nested_ratio,
            ..
        } = second.as_ref()
        else {
            panic!("expected nested projected split");
        };
        assert_eq!(*nested_id, SplitId(2));
        assert_eq!(*nested_ratio, 0.5);
        let mut projected_order = Vec::new();
        projected_panes(&projected, &mut projected_order);
        assert_eq!(projected_order, layout.panes_in_order());

        layout.resize_pane(PaneId(0), Axis::Horizontal, 5).unwrap();
        let LayoutNode::Split { id: after, .. } = layout.project() else {
            panic!("expected projected split");
        };
        assert_eq!(after, SplitId(1));
    }

    #[test]
    fn unrelated_split_keeps_the_existing_divider_id() {
        let mut layout = CellLayout::new(PaneId(0), 80, 24);
        let mut ids = allocator(1);
        layout
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        layout
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(2),
                &mut ids,
            )
            .unwrap();
        let LayoutNode::Split { id, second, .. } = layout.project() else {
            panic!("expected projected split");
        };
        assert_eq!(id, SplitId(2));
        let LayoutNode::Split { id, .. } = second.as_ref() else {
            panic!("expected nested projected split");
        };
        assert_eq!(*id, SplitId(1));
    }

    #[test]
    fn remap_is_simultaneous() {
        let mut layout = three_horizontal();
        let mapping = BTreeMap::from([
            (PaneId(0), PaneId(1)),
            (PaneId(1), PaneId(2)),
            (PaneId(2), PaneId(0)),
        ]);
        layout.remap(&mapping);
        assert_eq!(layout.panes_in_order(), [PaneId(1), PaneId(2), PaneId(0)]);
    }

    #[test]
    fn pane_count_counts_every_leaf() {
        assert_eq!(CellLayout::new(PaneId(0), 80, 24).pane_count(), 1);
        assert_eq!(three_horizontal().pane_count(), 3);
    }

    #[test]
    fn replace_panes_in_order_is_atomic_on_count_mismatch() {
        let mut layout = three_horizontal();
        assert!(!layout.replace_panes_in_order(&[PaneId(7), PaneId(8)]));
        assert_eq!(layout.panes_in_order(), [PaneId(0), PaneId(1), PaneId(2)]);
        assert!(layout.replace_panes_in_order(&[PaneId(7), PaneId(8), PaneId(9)]));
        assert_eq!(layout.panes_in_order(), [PaneId(7), PaneId(8), PaneId(9)]);
    }

    #[test]
    fn refresh_divider_ids_replaces_every_projected_id() {
        let mut layout = three_horizontal();
        let mut ids = allocator(10);
        layout.refresh_divider_ids(&mut ids);
        let LayoutNode::Split { id, second, .. } = layout.project() else {
            panic!("expected projected split");
        };
        assert_eq!(id, SplitId(10));
        let LayoutNode::Split { id, .. } = second.as_ref() else {
            panic!("expected nested projected split");
        };
        assert_eq!(*id, SplitId(11));

        let mut nested = CellLayout::new(PaneId(0), 80, 24);
        let mut build = allocator(1);
        nested
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut build,
            )
            .unwrap();
        nested
            .split(
                PaneId(1),
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                PaneId(2),
                &mut build,
            )
            .unwrap();
        let mut fresh = allocator(20);
        nested.refresh_divider_ids(&mut fresh);
        let LayoutNode::Split { id, second, .. } = nested.project() else {
            panic!("expected projected split");
        };
        assert_eq!(id, SplitId(20));
        let LayoutNode::Split { id, .. } = second.as_ref() else {
            panic!("expected nested projected split");
        };
        assert_eq!(*id, SplitId(21));
    }

    #[test]
    fn dump_checksum_and_identity_mutations_work() {
        let mut layout = CellLayout::new(PaneId(0), 80, 24);
        assert_eq!(layout.dump(), "b25d,80x24,0,0,0");
        let mut ids = allocator(1);
        layout
            .split(
                PaneId(0),
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                PaneId(1),
                &mut ids,
            )
            .unwrap();
        assert!(layout.swap(PaneId(0), PaneId(1)));
        assert_eq!(layout.panes_in_order(), [PaneId(1), PaneId(0)]);
        assert!(layout.replace(PaneId(1), PaneId(7)));
        assert_eq!(layout.panes_in_order(), [PaneId(7), PaneId(0)]);
        assert!(!layout.replace(PaneId(99), PaneId(8)));

        let mut maximum = CellLayout::new(PaneId(0), u16::MAX, u16::MAX);
        assert_eq!(maximum.extent(), (PANE_MAXIMUM, PANE_MAXIMUM));
        maximum.resize(u16::MAX, u16::MAX);
        assert_eq!(maximum.extent(), (PANE_MAXIMUM, PANE_MAXIMUM));
        assert_eq!(CellLayout::new(PaneId(0), 0, 0).extent(), (1, 1));
    }

    fn parse_error(input: &str) -> String {
        let mut parsed = match CellLayout::parse(input) {
            Ok(parsed) => parsed,
            Err(error) => return error.cause().to_owned(),
        };
        parsed
            .check_sizes()
            .map_or_else(|error| error.cause().to_owned(), |()| String::new())
    }

    #[test]
    fn parsed_layout_corrects_the_root_and_assigns_dfs_panes() {
        let input = checksummed("999x999,9,9{40x24,8,8,111,39x24,9,9,222}");
        let mut parsed = CellLayout::parse(&input).unwrap();
        parsed.check_sizes().unwrap();
        assert_eq!(parsed.pane_count(), 2);
        let mut ids = allocator(20);
        let (layout, selection) = parsed.into_layout(&[PaneId(7), PaneId(8)], &mut ids);
        assert_eq!(selection, None);
        assert_eq!(
            layout.dump(),
            checksummed("80x24,0,0{40x24,0,0,7,39x24,41,0,8}")
        );
        let LayoutNode::Split { id, .. } = layout.project() else {
            panic!("expected parsed split");
        };
        assert_eq!(id, SplitId(20));
    }

    #[test]
    fn parsed_layout_rejects_checksum_grammar_and_size_errors_like_the_pin() {
        assert_eq!(parse_error("0000,80x24,0,0,0"), "invalid layout checksum");
        let parsed = CellLayout::parse("B25D,80x24,0,0,0").unwrap();
        let mut ids = allocator(20);
        assert_eq!(
            parsed.into_layout(&[PaneId(7)], &mut ids).0.dump(),
            checksummed("80x24,0,0,7")
        );
        assert!(CellLayout::parse(" \t b25d,80x24,0,0,0").is_ok());
        for malformed in [
            "b25,80x24,0,0,0",
            "0b25d,80x24,0,0,0",
            "0xb25d,80x24,0,0,0",
            "",
        ] {
            assert_eq!(
                parse_error(malformed),
                "malformed layout header",
                "{malformed}"
            );
        }
        assert_eq!(
            parse_error(&checksummed("80x24,0,0,0garbage")),
            "invalid layout"
        );
        assert_eq!(parse_error(&checksummed("80x24,0,0,0,5")), "trailing data");
        assert_eq!(parse_error(&checksummed("80x24,0,0,0]")), "trailing data");
        assert_eq!(parse_error(&checksummed("0x24,0,0,0")), "invalid layout");
        assert_eq!(
            parse_error(&checksummed("80x10001,0,0,0")),
            "invalid layout"
        );
        assert_eq!(
            parse_error(&checksummed("80x24,10001,0,0")),
            "invalid layout"
        );
        assert_eq!(
            parse_error(&checksummed("80x24,0,0{80x24,0,0,0}")),
            "invalid layout"
        );
        assert_eq!(
            parse_error(&checksummed("80x24,0,0{40x23,0,0,0,39x24,41,0,1}")),
            "size mismatch after applying layout"
        );
    }

    #[test]
    fn parsed_layout_leaf_pane_numbers_are_optional() {
        let with_ids = CellLayout::parse("8205,80x24,0,0{40x24,0,0,0,39x24,41,0,1}").unwrap();
        let without_ids = CellLayout::parse("347e,80x24,0,0{40x24,0,0,39x24,41,0}").unwrap();
        let panes = [PaneId(7), PaneId(8)];
        let mut with_ids_allocator = allocator(20);
        let mut without_ids_allocator = allocator(20);
        assert_eq!(
            with_ids.into_layout(&panes, &mut with_ids_allocator),
            without_ids.into_layout(&panes, &mut without_ids_allocator)
        );
    }

    #[test]
    fn parsed_layout_trims_the_bottom_right_cell_with_tmux_gifting() {
        let input = checksummed("100x20,0,0[100x9,0,0{49x9,0,0,50,50x9,50,0,51},100x10,0,10,52]");
        let mut parsed = CellLayout::parse(&input).unwrap();
        assert_eq!(parsed.pane_count(), 3);
        parsed.trim_bottom_right();
        assert_eq!(parsed.pane_count(), 2);
        let mut ids = allocator(30);
        let layout = parsed.into_layout(&[PaneId(7), PaneId(8)], &mut ids).0;
        assert_eq!(
            layout.dump(),
            checksummed("100x20,0,0{49x20,0,0,7,50x20,50,0,8}")
        );
        let LayoutNode::Split { id, .. } = layout.project() else {
            panic!("expected trimmed split");
        };
        assert_eq!(id, SplitId(30));
    }

    #[test]
    fn parsed_layout_enforces_the_protocol_depth_budget() {
        let accepted = nested_layout(MAX_LAYOUT_DEPTH - 1).0;
        assert!(CellLayout::parse(&checksummed(&accepted)).is_ok());
        let rejected = nested_layout(MAX_LAYOUT_DEPTH).0;
        assert_eq!(parse_error(&checksummed(&rejected)), "invalid layout");
    }

    #[test]
    fn validate_rejects_corrupt_tree() {
        let layout = CellLayout {
            root: CellNode::Node {
                axis: Axis::Horizontal,
                geometry: CellGeometry {
                    sx: 80,
                    sy: 24,
                    xoff: 0,
                    yoff: 0,
                },
                children: vec![
                    CellChild {
                        divider: None,
                        node: leaf(PaneId(0), 40, 24),
                    },
                    CellChild {
                        divider: None,
                        node: leaf(PaneId(1), 39, 24),
                    },
                ],
            },
            extent: (80, 24),
        };
        assert_eq!(
            layout.validate(),
            Err("non-first child is missing a divider".to_owned())
        );
        let zero_leaf = CellLayout {
            root: leaf(PaneId(0), 0, 1),
            extent: (1, 1),
        };
        assert_eq!(
            zero_leaf.validate(),
            Err("leaf extent is below the pane minimum".to_owned())
        );
        assert_eq!(geometry(&CellLayout::new(PaneId(5), 1, 1), 5).sx, 1);
    }
}
