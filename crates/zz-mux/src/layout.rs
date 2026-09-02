#![allow(dead_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use zz_protocol::{Axis, LayoutNode, PaneBorderStatus, PaneId, SplitId};

use crate::{PresetOptions, model::LayoutPreset};

pub(crate) const PANE_MINIMUM: u16 = 1;
pub(crate) const PANE_MAXIMUM: u16 = 10_000;
const MAX_LAYOUT_DEPTH: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct CellGeometry {
    pub sx: u16,
    pub sy: u16,
    pub xoff: u16,
    pub yoff: u16,
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum CellNode {
    Leaf {
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
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ParsedLayout {
    root: ParsedNode,
}

#[derive(Clone, PartialEq, Debug)]
enum ParsedNode {
    Leaf {
        geometry: CellGeometry,
    },
    Node {
        axis: Axis,
        geometry: CellGeometry,
        children: Vec<ParsedNode>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LayoutParseError {
    InvalidLayout,
    SizeMismatch,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LayoutError {
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
            Self::Leaf { geometry, .. } | Self::Node { geometry, .. } => *geometry,
        }
    }

    fn geometry_mut(&mut self) -> &mut CellGeometry {
        match self {
            Self::Leaf { geometry, .. } | Self::Node { geometry, .. } => geometry,
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
            Self::Leaf { geometry } | Self::Node { geometry, .. } => *geometry,
        }
    }

    fn geometry_mut(&mut self) -> &mut CellGeometry {
        match self {
            Self::Leaf { geometry } | Self::Node { geometry, .. } => geometry,
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
    pub(crate) const fn cause(self) -> &'static str {
        match self {
            Self::InvalidLayout => "invalid layout",
            Self::SizeMismatch => "size mismatch after applying layout",
        }
    }
}

impl CellLayout {
    pub(crate) fn new(pane: PaneId, sx: u16, sy: u16) -> Self {
        let sx = sx.clamp(PANE_MINIMUM, PANE_MAXIMUM);
        let sy = sy.clamp(PANE_MINIMUM, PANE_MAXIMUM);
        let layout = Self {
            root: CellNode::Leaf {
                pane,
                geometry: CellGeometry {
                    sx,
                    sy,
                    xoff: 0,
                    yoff: 0,
                },
            },
        };
        layout.debug_validate();
        layout
    }

    pub(crate) fn parse(input: &str) -> Result<ParsedLayout, LayoutParseError> {
        let bytes = input.as_bytes();
        let Some(prefix) = bytes.get(..5) else {
            return Err(LayoutParseError::InvalidLayout);
        };
        if prefix[4] != b',' || !prefix[..4].iter().all(u8::is_ascii_hexdigit) {
            return Err(LayoutParseError::InvalidLayout);
        }
        let expected = prefix[..4].iter().fold(0_u16, |value, byte| {
            value * 16
                + u16::from(match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    b'A'..=b'F' => byte - b'A' + 10,
                    _ => 0,
                })
        });
        let body = &bytes[5..];
        if expected != layout_checksum(body) {
            return Err(LayoutParseError::InvalidLayout);
        }
        let mut parser = LayoutParser::new(body);
        let mut root = parser
            .parse_node(0)
            .ok_or(LayoutParseError::InvalidLayout)?;
        if !parser.is_done() {
            return Err(LayoutParseError::InvalidLayout);
        }
        if !correct_parsed_root_size(&mut root) || !check_parsed_node(&root) {
            return Err(LayoutParseError::SizeMismatch);
        }
        Ok(ParsedLayout { root })
    }

    pub(crate) fn extent(&self) -> (u16, u16) {
        let geometry = self.root.geometry();
        (geometry.sx, geometry.sy)
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
                        },
                    },
                );
            } else {
                children.push(CellChild {
                    divider: Some(divider),
                    node: CellNode::Leaf {
                        pane: new_pane,
                        geometry,
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
        let parent_path = &path[..path.len() - 1];
        let index = path[path.len() - 1];
        let parent =
            node_at_path_mut(&mut self.root, parent_path).ok_or(LayoutError::UnknownPane)?;
        let CellNode::Node { axis, children, .. } = parent else {
            return Err(LayoutError::UnknownPane);
        };
        let gift = children[index].node.extent(*axis) + 1;
        let neighbour = if index + 1 < children.len() {
            Some(index + 1)
        } else {
            index.checked_sub(1)
        };
        let Some(neighbour) = neighbour else {
            return Ok(());
        };
        resize_adjust(&mut children[neighbour].node, *axis, i32::from(gift));
        children.remove(index);
        if let Some(first) = children.first_mut() {
            first.divider = None;
        }
        if children.len() == 1 {
            collapse_single_child(&mut self.root, parent_path);
        }
        fix_offsets(&mut self.root);
        self.debug_validate();
        Ok(())
    }

    pub(crate) fn resize(&mut self, sx: u16, sy: u16) {
        let sx = sx.clamp(PANE_MINIMUM, PANE_MAXIMUM);
        let sy = sy.clamp(PANE_MINIMUM, PANE_MAXIMUM);
        resize_root_axis(&mut self.root, Axis::Horizontal, sx);
        resize_root_axis(&mut self.root, Axis::Vertical, sy);
        fix_offsets(&mut self.root);
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
        if index + 1 == children.len() {
            let Some(previous) = index.checked_sub(1) else {
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
        let change = if index + 1 == children.len() {
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
        let (sx, sy) = self.extent();
        if panes.len() == 1 {
            self.root = leaf(panes[0], sx, sy);
            self.debug_validate();
            return;
        }
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
        project_node(&self.root)
    }

    #[must_use]
    pub fn dump(&self) -> String {
        let mut body = String::new();
        dump_node(&self.root, &mut body);
        let checksum = layout_checksum(body.as_bytes());
        format!("{checksum:04x},{body}")
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let geometry = self.root.geometry();
        if geometry.xoff != 0 || geometry.yoff != 0 {
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
    pub(crate) fn pane_count(&self) -> usize {
        count_parsed_panes(&self.root)
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
            let gift = children[index].extent(*axis).saturating_add(1);
            let Some(neighbour) = index.checked_sub(1) else {
                return;
            };
            parsed_resize_adjust(&mut children[neighbour], *axis, i32::from(gift));
            children.remove(index);
            children.len() == 1
        };
        if collapse {
            collapse_parsed_single_child(&mut self.root, parent_path);
        }
    }

    pub(crate) fn into_layout(
        self,
        panes: &[PaneId],
        ids: &mut dyn FnMut() -> SplitId,
    ) -> CellLayout {
        debug_assert_eq!(panes.len(), self.pane_count());
        let mut panes = panes.iter().copied();
        let root = assign_parsed_node(self.root, &mut panes);
        let panes_exhausted = panes.next().is_none();
        debug_assert!(panes_exhausted);
        let mut layout = CellLayout { root };
        fix_offsets(&mut layout.root);
        layout.refresh_divider_ids(ids);
        layout
    }
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
        let geometry = CellGeometry { sx, sy, xoff, yoff };
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
            Some(b',' | b'}' | b']') | None => Some(ParsedNode::Leaf { geometry }),
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
    let Some(last) = children.last() else {
        return false;
    };
    let along = children.iter().try_fold(0_u32, |total, child| {
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
    for child in children {
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
    extent.checked_sub(1) == Some(u32::from(geometry.extent(*axis)))
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
        ParsedNode::Leaf { geometry } => geometry.extent(axis).saturating_sub(PANE_MINIMUM),
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
        ParsedNode::Leaf { geometry } => CellNode::Leaf {
            pane: panes.next().expect("parsed pane count was checked"),
            geometry,
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
    }
}

fn collect_panes(node: &CellNode, panes: &mut Vec<PaneId>) {
    match node {
        CellNode::Leaf { pane, .. } => panes.push(*pane),
        CellNode::Node { children, .. } => {
            for child in children {
                collect_panes(&child.node, panes);
            }
        }
    }
}

fn count_panes(node: &CellNode) -> usize {
    match node {
        CellNode::Leaf { .. } => 1,
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
            geometry.yoff.saturating_add(geometry.sy) == root.yoff.saturating_add(root.sy)
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
    match node {
        CellNode::Leaf { pane, geometry } => (*pane == target).then_some(*geometry),
        CellNode::Node { children, .. } => children
            .iter()
            .find_map(|child| pane_geometry(&child.node, target)),
    }
}

fn pane_path(node: &CellNode, target: PaneId) -> Option<Vec<usize>> {
    fn find(node: &CellNode, target: PaneId, path: &mut Vec<usize>) -> bool {
        match node {
            CellNode::Leaf { pane, .. } => *pane == target,
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
    match node {
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
        for child in children {
            resize_adjust(&mut child.node, axis, change);
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
    let previous = children.iter().fold(
        u32::try_from(children.len().saturating_sub(1)).unwrap_or(u32::MAX),
        |total, child| total + u32::from(child.node.extent(axis)),
    );
    let previous = u16::try_from(previous).unwrap_or(u16::MAX);
    let mut available = geometry.extent(axis);
    let count = children.len();
    for (index, child) in children.iter_mut().enumerate() {
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
            let receiver = index + 1;
            if receiver >= children.len() {
                break;
            }
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
        .position(|child| child.divider == Some(divider))
    {
        let previous = index.checked_sub(1)?;
        let remaining_borders = children.len().saturating_sub(index + 1);
        let span = children[index..].iter().fold(
            u32::try_from(remaining_borders).unwrap_or(u32::MAX),
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
        node @ CellNode::Leaf { .. } => {
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
    if leaves != children.len() {
        return SpreadCell::Refused;
    }
    let count = children.len();
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
    for child in children {
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
    for child in children {
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
        offset = offset.saturating_add(child.node.extent(*axis).saturating_add(1));
    }
}

fn swap_panes(node: &mut CellNode, a: PaneId, b: PaneId) {
    match node {
        CellNode::Leaf { pane, .. } if *pane == a => *pane = b,
        CellNode::Leaf { pane, .. } if *pane == b => *pane = a,
        CellNode::Leaf { .. } => {}
        CellNode::Node { children, .. } => {
            for child in children {
                swap_panes(&mut child.node, a, b);
            }
        }
    }
}

fn replace_pane(node: &mut CellNode, old: PaneId, new: PaneId) -> bool {
    match node {
        CellNode::Leaf { pane, .. } if *pane == old => {
            *pane = new;
            true
        }
        CellNode::Leaf { .. } => false,
        CellNode::Node { children, .. } => children
            .iter_mut()
            .any(|child| replace_pane(&mut child.node, old, new)),
    }
}

fn remap_panes(node: &mut CellNode, mapping: &BTreeMap<PaneId, PaneId>) {
    match node {
        CellNode::Leaf { pane, .. } => {
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
        CellNode::Leaf { pane, .. } => *pane = panes.next().expect("pane count was checked"),
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

fn project_node(node: &CellNode) -> LayoutNode {
    match node {
        CellNode::Leaf { pane, .. } => LayoutNode::Pane(*pane),
        CellNode::Node { axis, children, .. } => project_children(children, *axis, 0),
    }
}

fn project_children(children: &[CellChild], axis: Axis, index: usize) -> LayoutNode {
    if index + 1 == children.len() {
        return project_node(&children[index].node);
    }
    let first = project_node(&children[index].node);
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
        CellNode::Leaf { pane, .. } => {
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
        CellNode::Leaf { geometry, .. } => {
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
        expected_offset = expected_offset.saturating_add(child_extent.saturating_add(1));
    }
    if extent - 1 != u32::from(geometry.extent(*axis)) {
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

    #[test]
    fn parsed_layout_corrects_the_root_and_assigns_dfs_panes() {
        let input = checksummed("999x999,9,9{40x24,8,8,111,39x24,9,9,222}");
        let parsed = CellLayout::parse(&input).unwrap();
        assert_eq!(parsed.pane_count(), 2);
        let mut ids = allocator(20);
        let layout = parsed.into_layout(&[PaneId(7), PaneId(8)], &mut ids);
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
    fn parsed_layout_rejects_checksum_grammar_and_size_errors() {
        assert_eq!(
            CellLayout::parse("0000,80x24,0,0,0"),
            Err(LayoutParseError::InvalidLayout)
        );
        let parsed = CellLayout::parse("B25D,80x24,0,0,0").unwrap();
        let mut ids = allocator(20);
        assert_eq!(
            parsed.into_layout(&[PaneId(7)], &mut ids).dump(),
            checksummed("80x24,0,0,7")
        );
        for malformed in [
            "b25,80x24,0,0,0",
            "0b25d,80x24,0,0,0",
            "0xb25d,80x24,0,0,0",
            " b25d,80x24,0,0,0",
        ] {
            assert_eq!(
                CellLayout::parse(malformed),
                Err(LayoutParseError::InvalidLayout)
            );
        }
        assert_eq!(
            CellLayout::parse(&checksummed("80x24,0,0,0garbage")),
            Err(LayoutParseError::InvalidLayout)
        );
        assert_eq!(
            CellLayout::parse(&checksummed("80x24,0,0{80x24,0,0,0}")),
            Err(LayoutParseError::InvalidLayout)
        );
        assert_eq!(
            CellLayout::parse(&checksummed("80x24,0,0{40x23,0,0,0,39x24,41,0,1}")),
            Err(LayoutParseError::SizeMismatch)
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
        let layout = parsed.into_layout(&[PaneId(7), PaneId(8)], &mut ids);
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
        assert_eq!(
            CellLayout::parse(&checksummed(&rejected)),
            Err(LayoutParseError::InvalidLayout)
        );
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
        };
        assert_eq!(
            layout.validate(),
            Err("non-first child is missing a divider".to_owned())
        );
        let zero_leaf = CellLayout {
            root: leaf(PaneId(0), 0, 1),
        };
        assert_eq!(
            zero_leaf.validate(),
            Err("leaf extent is below the pane minimum".to_owned())
        );
        assert_eq!(geometry(&CellLayout::new(PaneId(5), 1, 1), 5).sx, 1);
    }
}
