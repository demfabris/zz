use zz_protocol::{Axis, PaneBorderStatus, PaneId, WindowId};

use super::MuxEngine;
use crate::layout::{CellGeometry, CellNode};

const BORDER_L: u8 = 0x1;
const BORDER_R: u8 = 0x2;
const BORDER_U: u8 = 0x4;
const BORDER_D: u8 = 0x8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanesModeAreaGeometry {
    pub pane: PaneId,
    pub number: u32,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanesModeGeometry {
    pub areas: Vec<PanesModeAreaGeometry>,
    pub borders: Vec<(u16, u16, u8)>,
    pub copy: bool,
}

#[derive(Clone, Copy)]
struct Scale {
    osx: u32,
    osy: u32,
    dsx: u32,
    dsy: u32,
}

impl Scale {
    const fn copies(self) -> bool {
        self.osx <= self.dsx && self.osy <= self.dsy
    }

    const fn x(self, x: u32) -> i32 {
        if self.osx <= self.dsx {
            x as i32
        } else {
            (x * self.dsx / self.osx) as i32
        }
    }

    const fn y(self, y: u32) -> i32 {
        if self.osy <= self.dsy {
            y as i32
        } else {
            (y * self.dsy / self.osy) as i32
        }
    }
}

struct BorderMap {
    width: u32,
    height: u32,
    cells: Vec<u8>,
}

impl BorderMap {
    fn mark(&mut self, x: i32, y: i32, mask: u8) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.cells[(y as u32 * self.width + x as u32) as usize] |= mask;
        }
    }

    fn clear(&mut self, (x, y, x2, y2): (i32, i32, i32, i32)) {
        let (x, y) = (x.max(0), y.max(0));
        let x2 = x2.min(self.width as i32 - 1);
        let y2 = y2.min(self.height as i32 - 1);
        for row in y..=y2 {
            for column in x..=x2 {
                self.cells[(row as u32 * self.width + column as u32) as usize] = 0;
            }
        }
    }

    fn frame(&mut self, (x, y, x2, y2): (i32, i32, i32, i32)) -> Option<usize> {
        let mut frame = Self {
            width: self.width,
            height: self.height,
            cells: vec![0; self.cells.len()],
        };
        frame.hline(x, x2 + 1, y);
        frame.hline(x, x2 + 1, y2);
        frame.vline(x, y, y2 + 1);
        frame.vline(x2, y, y2 + 1);
        let mut last = None;
        for (index, (cell, mask)) in self.cells.iter_mut().zip(frame.cells).enumerate() {
            if mask != 0 {
                *cell = mask;
                last = Some(index);
            }
        }
        last
    }

    fn get(&self, x: i32, y: i32) -> u8 {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.cells[(y as u32 * self.width + x as u32) as usize]
        } else {
            0
        }
    }

    fn vline(&mut self, x: i32, y: i32, y2: i32) {
        if x < 0 || x as u32 >= self.width || y2 <= y {
            return;
        }
        let y = y.max(0);
        let y2 = y2.min(self.height as i32);
        for row in y..y2 {
            let mut mask = 0;
            if row > y {
                mask |= BORDER_U;
            }
            if row + 1 < y2 {
                mask |= BORDER_D;
            }
            if mask == 0 {
                mask = BORDER_U | BORDER_D;
            }
            self.mark(x, row, mask);
        }
    }

    fn hline(&mut self, x: i32, x2: i32, y: i32) {
        if y < 0 || y as u32 >= self.height || x2 <= x {
            return;
        }
        let x = x.max(0);
        let x2 = x2.min(self.width as i32);
        for column in x..x2 {
            let mut mask = 0;
            if column > x {
                mask |= BORDER_L;
            }
            if column + 1 < x2 {
                mask |= BORDER_R;
            }
            if mask == 0 {
                mask = BORDER_L | BORDER_R;
            }
            self.mark(column, y, mask);
        }
    }
}

fn split_lines(node: &CellNode, scale: Scale, map: &mut BorderMap, joins: bool) {
    let CellNode::Node {
        axis,
        geometry,
        children,
    } = node
    else {
        return;
    };
    for (position, child) in children.iter().enumerate() {
        if !child.node.tiled() {
            continue;
        }
        split_lines(&child.node, scale, map, joins);
        if !children[position + 1..]
            .iter()
            .any(|next| next.node.tiled())
        {
            continue;
        }
        let cell = child.node.geometry();
        if *axis == Axis::Horizontal {
            let x = scale.x(at(cell.xoff) + u32::from(cell.sx));
            let y = scale.y(at(geometry.yoff));
            let y2 = scale.y(at(geometry.yoff) + u32::from(geometry.sy));
            if !joins {
                map.vline(x, y, y2);
                continue;
            }
            if x < 0 || x as u32 >= map.width {
                continue;
            }
            if y > 0 && map.get(x, y - 1) & (BORDER_L | BORDER_R) != 0 {
                map.mark(x, y - 1, BORDER_D);
                map.mark(x, y, BORDER_U);
            }
            if (y2 as u32) < map.height && map.get(x, y2) & (BORDER_L | BORDER_R) != 0 {
                map.mark(x, y2, BORDER_U);
                map.mark(x, y2 - 1, BORDER_D);
            }
        } else {
            let x = scale.x(at(geometry.xoff));
            let x2 = scale.x(at(geometry.xoff) + u32::from(geometry.sx));
            let y = scale.y(at(cell.yoff) + u32::from(cell.sy));
            if !joins {
                map.hline(x, x2, y);
                continue;
            }
            if y < 0 || y as u32 >= map.height {
                continue;
            }
            if x > 0 && map.get(x - 1, y) & (BORDER_U | BORDER_D) != 0 {
                map.mark(x - 1, y, BORDER_R);
                map.mark(x, y, BORDER_L);
            }
            if (x2 as u32) < map.width && map.get(x2, y) & (BORDER_U | BORDER_D) != 0 {
                map.mark(x2, y, BORDER_L);
                map.mark(x2 - 1, y, BORDER_R);
            }
        }
    }
}

const fn cell_type(mask: u8) -> u8 {
    match mask {
        0b1111 => 11,
        0b0111 => 8,
        0b1011 => 7,
        0b0001..=0b0011 => 2,
        0b1101 => 10,
        0b0101 => 6,
        0b1001 => 4,
        0b1110 => 9,
        0b0110 => 5,
        0b1010 => 3,
        0b1100 | 0b0100 | 0b1000 => 1,
        _ => 12,
    }
}

fn at(offset: i32) -> u32 {
    u32::try_from(offset).unwrap_or(0)
}

fn float_frame(cell: CellGeometry, scale: Scale) -> (i32, i32, i32, i32) {
    let map = |offset: i32, original: u32, display: u32| {
        if original <= display {
            offset
        } else {
            offset * display as i32 / original as i32
        }
    };
    let x = if cell.xoff == 0 {
        -1
    } else {
        map(cell.xoff - 1, scale.osx, scale.dsx)
    };
    let y = if cell.yoff == 0 {
        -1
    } else {
        map(cell.yoff - 1, scale.osy, scale.dsy)
    };
    (
        x,
        y,
        map(cell.xoff + i32::from(cell.sx), scale.osx, scale.dsx),
        map(cell.yoff + i32::from(cell.sy), scale.osy, scale.dsy),
    )
}

fn carves(cell: CellGeometry, root: CellGeometry, status: PaneBorderStatus) -> bool {
    match status {
        PaneBorderStatus::Off => false,
        PaneBorderStatus::Top => cell.yoff == root.yoff,
        PaneBorderStatus::Bottom => {
            cell.yoff + i32::from(cell.sy) == root.yoff + i32::from(root.sy)
        }
    }
}

impl MuxEngine {
    #[must_use]
    pub fn panes_mode_geometry(
        &self,
        window: WindowId,
        dsx: u16,
        dsy: u16,
    ) -> Option<PanesModeGeometry> {
        let state = self.state.windows.get(&window)?;
        let root = state.layout.root();
        let (sx, sy) = state.layout.extent();
        let extent = CellGeometry {
            sx,
            sy,
            xoff: 0,
            yoff: 0,
        };
        let scale = Scale {
            osx: u32::from(extent.sx),
            osy: u32::from(extent.sy),
            dsx: u32::from(dsx),
            dsy: u32::from(dsy),
        };
        let mut geometry = PanesModeGeometry {
            copy: scale.copies(),
            ..PanesModeGeometry::default()
        };
        if scale.osx == 0 || scale.osy == 0 || dsx == 0 || dsy == 0 {
            return Some(geometry);
        }
        let status = self.pane_border_status(window);
        let painted = state
            .pane_order()
            .iter()
            .filter(|pane| !state.layout.is_floating(**pane))
            .chain(
                state
                    .z_order()
                    .iter()
                    .rev()
                    .filter(|pane| state.layout.is_floating(**pane)),
            )
            .copied()
            .collect::<Vec<_>>();
        for pane in &painted {
            let Some(cell) = state.layout.pane_geometry(*pane) else {
                continue;
            };
            let floating = state.layout.is_floating(*pane);
            if floating && (cell.xoff < 0 || cell.yoff < 0) {
                continue;
            }
            let Some(number) = self.pane_index(window, *pane) else {
                continue;
            };
            let (x, y, mut x2, mut y2) = if scale.copies() {
                (
                    at(cell.xoff),
                    at(cell.yoff),
                    at(cell.xoff) + u32::from(cell.sx),
                    at(cell.yoff) + u32::from(cell.sy),
                )
            } else {
                (
                    at(cell.xoff) * scale.dsx / scale.osx,
                    at(cell.yoff) * scale.dsy / scale.osy,
                    (at(cell.xoff) + u32::from(cell.sx)) * scale.dsx / scale.osx,
                    (at(cell.yoff) + u32::from(cell.sy)) * scale.dsy / scale.osy,
                )
            };
            if x >= scale.dsx || y >= scale.dsy {
                continue;
            }
            if x2 <= x {
                x2 = x + 1;
            }
            if y2 <= y {
                y2 = y + 1;
            }
            let mut width = x2.min(scale.dsx) - x;
            let mut height = y2.min(scale.dsy) - y;
            let (mut x, mut y) = (x, y);
            if carves(cell, extent, status) && height > 1 {
                if status == PaneBorderStatus::Top {
                    y += 1;
                }
                height -= 1;
            }
            if floating {
                let (left, top, right, bottom) = float_frame(cell, scale);
                let (mut bx, mut by) = (x as i32, y as i32);
                let mut bx2 = bx + width as i32 - 1;
                let mut by2 = by + height as i32 - 1;
                if left >= 0 && bx <= left {
                    bx = left + 1;
                }
                if top >= 0 && by <= top {
                    by = top + 1;
                }
                if right < scale.dsx as i32 && bx2 >= right {
                    bx2 = right - 1;
                }
                if bottom < scale.dsy as i32 && by2 >= bottom {
                    by2 = bottom - 1;
                }
                if bx2 < bx || by2 < by {
                    continue;
                }
                (x, y) = (bx as u32, by as u32);
                width = (bx2 - bx + 1) as u32;
                height = (by2 - by + 1) as u32;
            }
            geometry.areas.push(PanesModeAreaGeometry {
                pane: *pane,
                number,
                x: u16::try_from(x).unwrap_or(u16::MAX),
                y: u16::try_from(y).unwrap_or(u16::MAX),
                width: u16::try_from(width).unwrap_or(u16::MAX),
                height: u16::try_from(height).unwrap_or(u16::MAX),
            });
        }
        let mut map = BorderMap {
            width: scale.dsx,
            height: scale.dsy,
            cells: vec![0; (scale.dsx * scale.dsy) as usize],
        };
        split_lines(root, scale, &mut map, false);
        if status != PaneBorderStatus::Off {
            for pane in state.pane_order() {
                let Some(cell) = state.layout.pane_geometry(*pane) else {
                    continue;
                };
                if !carves(cell, extent, status) {
                    continue;
                }
                let x = scale.x(at(cell.xoff));
                let x2 = scale.x(at(cell.xoff) + u32::from(cell.sx));
                let y = if status == PaneBorderStatus::Top {
                    scale.y(at(cell.yoff))
                } else {
                    scale.y(at(cell.yoff) + u32::from(cell.sy)) - 1
                };
                map.hline(x, x2, y);
            }
        }
        split_lines(root, scale, &mut map, true);
        let mut last_drawn = None;
        for pane in painted
            .iter()
            .filter(|pane| state.layout.is_floating(**pane))
        {
            if let Some(cell) = state.layout.pane_geometry(*pane) {
                let frame = float_frame(cell, scale);
                map.clear(frame);
                last_drawn = map.frame(frame).or(last_drawn);
            }
        }
        let mut last = None;
        for row in 0..dsy {
            for column in 0..dsx {
                let mask = map.get(i32::from(column), i32::from(row));
                if mask == 0 {
                    continue;
                }
                let border = (column, row, cell_type(mask));
                if last_drawn == Some(usize::from(row) * usize::from(dsx) + usize::from(column)) {
                    last = Some(border);
                } else {
                    geometry.borders.push(border);
                }
            }
        }
        geometry.borders.extend(last);
        Some(geometry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExecutionContext;
    use zz_protocol::CommandInvocation;

    fn command(name: &str, args: &[&str]) -> CommandInvocation {
        CommandInvocation::new(name, args.iter().copied())
    }

    #[test]
    fn panes_mode_geometry_lays_the_window_out_like_window_panes_draw_screen() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-s", "dp", "-x", "80", "-y", "23"]),
            )
            .unwrap();
        let window = context.window.unwrap();
        let first = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        let second = context.pane.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-v"]))
            .unwrap();
        let third = context.pane.unwrap();
        let geometry = engine.panes_mode_geometry(window, 80, 23).unwrap();
        assert!(geometry.copy);
        let areas = geometry
            .areas
            .iter()
            .map(|area| {
                (
                    area.pane,
                    area.number,
                    area.x,
                    area.y,
                    area.width,
                    area.height,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            areas,
            vec![
                (first, 0, 0, 0, 40, 23),
                (second, 1, 41, 0, 39, 11),
                (third, 2, 41, 12, 39, 11),
            ]
        );
        let cell = |x: u16, y: u16| {
            geometry
                .borders
                .iter()
                .find(|(bx, by, _)| (*bx, *by) == (x, y))
                .map(|(_, _, cell)| *cell)
        };
        assert_eq!(cell(40, 0), Some(1));
        assert_eq!(cell(40, 11), Some(9));
        assert_eq!(cell(41, 11), Some(2));
        assert_eq!(cell(79, 11), Some(2));
        assert_eq!(cell(40, 22), Some(1));
        assert_eq!(cell(0, 11), None);
        assert_eq!(geometry.borders.len(), 23 + 39);

        let scaled = engine.panes_mode_geometry(window, 40, 11).unwrap();
        assert!(!scaled.copy);
        assert_eq!(
            scaled
                .areas
                .iter()
                .map(|area| (area.x, area.y, area.width, area.height))
                .collect::<Vec<_>>(),
            vec![(0, 0, 20, 11), (20, 0, 20, 5), (20, 5, 20, 6)]
        );
    }

    #[test]
    fn panes_mode_paints_floats_bottom_to_top_in_z_order_like_window_panes_draw_screen() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-s", "dz", "-x", "80", "-y", "23"]),
            )
            .unwrap();
        let window = context.window.unwrap();
        let tiled = context.pane.unwrap();
        engine
            .execute(
                &mut context,
                &command("new-pane", &["-x", "30", "-y", "8", "-X", "10", "-Y", "3"]),
            )
            .unwrap();
        let older = context.pane.unwrap();
        engine
            .execute(
                &mut context,
                &command("new-pane", &["-x", "30", "-y", "8", "-X", "24", "-Y", "7"]),
            )
            .unwrap();
        let newer = context.pane.unwrap();
        let painted = |engine: &MuxEngine| {
            engine
                .panes_mode_geometry(window, 80, 23)
                .unwrap()
                .areas
                .iter()
                .map(|area| area.pane)
                .collect::<Vec<_>>()
        };
        assert_eq!(painted(&engine), vec![tiled, older, newer]);
        engine
            .execute(
                &mut context,
                &command("select-pane", &["-t", &older.to_string()]),
            )
            .unwrap();
        assert_eq!(painted(&engine), vec![tiled, newer, older]);
    }

    #[test]
    fn panes_mode_frames_each_float_over_the_split_lines_it_covers() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &command("new-session", &["-s", "df", "-x", "80", "-y", "23"]),
            )
            .unwrap();
        let window = context.window.unwrap();
        engine
            .execute(&mut context, &command("split-window", &["-h"]))
            .unwrap();
        engine
            .execute(
                &mut context,
                &command("new-pane", &["-x", "30", "-y", "8", "-X", "30", "-Y", "3"]),
            )
            .unwrap();
        let float = context.pane.unwrap();
        let geometry = engine.panes_mode_geometry(window, 80, 23).unwrap();
        let cell = |x: u16, y: u16| {
            geometry
                .borders
                .iter()
                .find(|(bx, by, _)| (*bx, *by) == (x, y))
                .map(|(_, _, cell)| *cell)
        };
        assert_eq!(cell(40, 2), Some(1));
        assert_eq!(cell(30, 3), Some(3));
        assert_eq!(cell(40, 3), Some(2));
        assert_eq!(cell(59, 3), Some(4));
        assert_eq!(cell(30, 6), Some(1));
        assert_eq!(cell(40, 6), None);
        assert_eq!(cell(59, 6), Some(1));
        assert_eq!(cell(30, 10), Some(5));
        assert_eq!(cell(40, 10), Some(2));
        assert_eq!(cell(59, 10), Some(6));
        assert_eq!(cell(40, 11), Some(1));
        assert_eq!(geometry.borders.last(), Some(&(59, 10, 6)));
        let area = geometry.areas.last().unwrap();
        assert_eq!(
            (area.pane, area.x, area.y, area.width, area.height),
            (float, 31, 4, 28, 6)
        );
    }
}
