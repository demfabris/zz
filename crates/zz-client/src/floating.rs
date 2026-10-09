//! Floating panes for GUI clients: where a float sits in pixels, and the
//! commands a title-bar or edge drag commits. Geometry is the daemon's, in
//! window cells; a GUI only previews a drag locally and lands it with
//! ordinary commands.

use zz_protocol::{CommandInvocation, FloatingPaneSnapshot, PaneBorderLines, PaneId};

/// A float's content box in window cells, borders outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatCells {
    pub xoff: i32,
    pub yoff: i32,
    pub sx: u16,
    pub sy: u16,
}

impl From<&FloatingPaneSnapshot> for FloatCells {
    fn from(float: &FloatingPaneSnapshot) -> Self {
        Self {
            xoff: float.xoff,
            yoff: float.yoff,
            sx: float.sx,
            sy: float.sy,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PixelRect {
    fn clipped(self, width: f32, height: f32) -> Option<Self> {
        let left = self.x.max(0.0);
        let top = self.y.max(0.0);
        let right = (self.x + self.width).min(width);
        let bottom = (self.y + self.height).min(height);
        (right > left && bottom > top).then_some(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// The float's box with its borders (`frame`) and its content box, in pixels
/// from the canvas origin, both clipped to the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatPixels {
    pub frame: PixelRect,
    pub content: Option<PixelRect>,
}

/// Places a float on a canvas of `canvas` pixels with cells of `cell`
/// pixels. `None` when nothing of it is on the canvas.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "window cell offsets are far below f32's exact integer range"
)]
pub fn float_pixels(
    float: FloatCells,
    bordered: bool,
    cell: (f32, f32),
    canvas: (f32, f32),
) -> Option<FloatPixels> {
    let pad = if bordered { 1.0 } else { 0.0 };
    let content = PixelRect {
        x: float.xoff as f32 * cell.0,
        y: float.yoff as f32 * cell.1,
        width: f32::from(float.sx) * cell.0,
        height: f32::from(float.sy) * cell.1,
    };
    let frame = PixelRect {
        x: content.x - pad * cell.0,
        y: content.y - pad * cell.1,
        width: content.width + 2.0 * pad * cell.0,
        height: content.height + 2.0 * pad * cell.1,
    };
    Some(FloatPixels {
        frame: frame.clipped(canvas.0, canvas.1)?,
        content: content.clipped(canvas.0, canvas.1),
    })
}

/// What a pointer drag on a float grabbed: its title bar, or any mix of its
/// edges (a corner is two).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FloatGrip {
    pub left: bool,
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
}

impl FloatGrip {
    pub const MOVE: Self = Self {
        left: false,
        top: false,
        right: false,
        bottom: false,
    };

    #[must_use]
    pub const fn is_move(self) -> bool {
        !(self.left || self.top || self.right || self.bottom)
    }

    /// `resize-pane` keeps the offsets (`layout_resize_floating_pane_to`), so
    /// a drag that moves the origin also needs a `move-pane`.
    #[must_use]
    pub const fn moves_origin(self) -> bool {
        self.is_move() || self.left || self.top
    }
}

/// The rectangle a drag of `delta` cells previews from `start`. A resize
/// never shrinks the content below one cell; the far edge stays put.
#[must_use]
pub fn float_drag_preview(start: FloatCells, grip: FloatGrip, delta: (i32, i32)) -> FloatCells {
    if grip.is_move() {
        return FloatCells {
            xoff: start.xoff + delta.0,
            yoff: start.yoff + delta.1,
            ..start
        };
    }
    let (xoff, sx) = drag_axis(start.xoff, start.sx, grip.left, grip.right, delta.0);
    let (yoff, sy) = drag_axis(start.yoff, start.sy, grip.top, grip.bottom, delta.1);
    FloatCells { xoff, yoff, sx, sy }
}

fn drag_axis(offset: i32, size: u16, near: bool, far: bool, delta: i32) -> (i32, u16) {
    let size = i32::from(size);
    let (offset, size) = if near {
        let delta = delta.min(size - 1);
        (offset + delta, size - delta)
    } else if far {
        (offset, (size + delta).max(1))
    } else {
        (offset, size)
    };
    (offset, u16::try_from(size).unwrap_or(u16::MAX))
}

/// The commands that land a dragged float on `target`, in the 3.8 command
/// surface: sizes and positions are outer (borders included). A move is
/// `move-pane -X -Y`; a right or bottom edge is `resize-pane -x -y`; an edge
/// that moves the origin is `resize-pane -x -y ; move-pane -X -Y`, one command
/// list sent in order.
#[must_use]
pub fn float_drag_commands(
    pane: PaneId,
    grip: FloatGrip,
    target: FloatCells,
    lines: PaneBorderLines,
) -> Vec<CommandInvocation> {
    let pad = i32::from(lines != PaneBorderLines::None);
    let target_pane = pane.to_string();
    let mut commands = Vec::new();
    if !grip.is_move() {
        commands.push(CommandInvocation::new(
            "resize-pane",
            [
                "-t".to_owned(),
                target_pane.clone(),
                "-x".to_owned(),
                (i32::from(target.sx) + 2 * pad).to_string(),
                "-y".to_owned(),
                (i32::from(target.sy) + 2 * pad).to_string(),
            ],
        ));
    }
    if grip.moves_origin() {
        commands.push(CommandInvocation::new(
            "move-pane",
            [
                "-t".to_owned(),
                target_pane,
                "-X".to_owned(),
                (target.xoff - pad).to_string(),
                "-Y".to_owned(),
                (target.yoff - pad).to_string(),
            ],
        ));
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOAT: FloatCells = FloatCells {
        xoff: 10,
        yoff: 4,
        sx: 20,
        sy: 6,
    };

    fn words(commands: &[CommandInvocation]) -> Vec<String> {
        commands
            .iter()
            .map(|command| {
                std::iter::once(command.name.clone())
                    .chain(command.args.iter().map(ToString::to_string))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    /// The 3.8 grammar the commands are read with: `-X`/`-Y` are the outer
    /// corner (content plus one when bordered), `-x`/`-y` the outer size.
    fn apply(start: FloatCells, commands: &[CommandInvocation]) -> FloatCells {
        let mut float = start;
        for command in commands {
            let args: Vec<String> = command.args.iter().map(ToString::to_string).collect();
            let value = |flag: &str| {
                args.iter()
                    .position(|arg| arg == flag)
                    .and_then(|index| args.get(index + 1))
                    .and_then(|value| value.parse::<i32>().ok())
            };
            match command.name.as_str() {
                "resize-pane" => {
                    float.sx = u16::try_from(value("-x").unwrap() - 2).unwrap();
                    float.sy = u16::try_from(value("-y").unwrap() - 2).unwrap();
                }
                "move-pane" => {
                    float.xoff = value("-X").unwrap() + 1;
                    float.yoff = value("-Y").unwrap() + 1;
                }
                other => panic!("unexpected {other}"),
            }
        }
        float
    }

    #[test]
    fn cells_become_canvas_pixels_with_the_border_outside_the_content() {
        let placed = float_pixels(FLOAT, true, (8.0, 16.0), (800.0, 600.0)).unwrap();
        assert_eq!(
            placed.frame,
            PixelRect {
                x: 72.0,
                y: 48.0,
                width: 176.0,
                height: 128.0,
            }
        );
        assert_eq!(
            placed.content,
            Some(PixelRect {
                x: 80.0,
                y: 64.0,
                width: 160.0,
                height: 96.0,
            })
        );
        let bare = float_pixels(FLOAT, false, (8.0, 16.0), (800.0, 600.0)).unwrap();
        assert_eq!(bare.frame, bare.content.unwrap());
    }

    #[test]
    fn a_float_past_the_canvas_edges_is_clipped_and_one_off_it_is_gone() {
        let hanging = FloatCells {
            xoff: -3,
            yoff: 30,
            sx: 10,
            sy: 10,
        };
        let placed = float_pixels(hanging, true, (8.0, 16.0), (400.0, 560.0)).unwrap();
        assert_eq!(
            placed.frame,
            PixelRect {
                x: 0.0,
                y: 464.0,
                width: 64.0,
                height: 96.0,
            }
        );
        assert_eq!(
            placed.content,
            Some(PixelRect {
                x: 0.0,
                y: 480.0,
                width: 56.0,
                height: 80.0,
            })
        );
        let gone = FloatCells { xoff: 60, ..FLOAT };
        assert_eq!(float_pixels(gone, true, (8.0, 16.0), (400.0, 560.0)), None);
    }

    #[test]
    fn a_move_sends_move_pane_with_the_outer_corner() {
        let target = float_drag_preview(FLOAT, FloatGrip::MOVE, (5, -2));
        let commands =
            float_drag_commands(PaneId(3), FloatGrip::MOVE, target, PaneBorderLines::Single);
        assert_eq!(words(&commands), ["move-pane -t %3 -X 14 -Y 1"]);
        assert_eq!(apply(FLOAT, &commands), target);
        let bare = float_drag_commands(PaneId(3), FloatGrip::MOVE, target, PaneBorderLines::None);
        assert_eq!(words(&bare), ["move-pane -t %3 -X 15 -Y 2"]);
    }

    #[test]
    fn a_right_or_bottom_edge_sends_only_resize_pane() {
        for (grip, delta) in [
            (
                FloatGrip {
                    right: true,
                    ..FloatGrip::MOVE
                },
                (4, 0),
            ),
            (
                FloatGrip {
                    bottom: true,
                    ..FloatGrip::MOVE
                },
                (0, 3),
            ),
            (
                FloatGrip {
                    right: true,
                    bottom: true,
                    ..FloatGrip::MOVE
                },
                (-2, -1),
            ),
        ] {
            let target = float_drag_preview(FLOAT, grip, delta);
            let commands = float_drag_commands(PaneId(3), grip, target, PaneBorderLines::Single);
            assert_eq!(commands.len(), 1);
            assert_eq!(commands[0].name, "resize-pane");
            assert_eq!(apply(FLOAT, &commands), target);
            assert_eq!((target.xoff, target.yoff), (FLOAT.xoff, FLOAT.yoff));
        }
        let target = float_drag_preview(
            FLOAT,
            FloatGrip {
                right: true,
                ..FloatGrip::MOVE
            },
            (4, 0),
        );
        assert_eq!(
            words(&float_drag_commands(
                PaneId(3),
                FloatGrip {
                    right: true,
                    ..FloatGrip::MOVE
                },
                target,
                PaneBorderLines::Single
            )),
            ["resize-pane -t %3 -x 26 -y 8"]
        );
    }

    #[test]
    fn a_left_top_or_top_left_resize_sends_resize_then_move_landing_on_the_preview() {
        for (grip, delta) in [
            (
                FloatGrip {
                    left: true,
                    ..FloatGrip::MOVE
                },
                (-4, 0),
            ),
            (
                FloatGrip {
                    top: true,
                    ..FloatGrip::MOVE
                },
                (0, 2),
            ),
            (
                FloatGrip {
                    left: true,
                    top: true,
                    ..FloatGrip::MOVE
                },
                (3, -2),
            ),
        ] {
            let target = float_drag_preview(FLOAT, grip, delta);
            let commands = float_drag_commands(PaneId(7), grip, target, PaneBorderLines::Single);
            let names: Vec<&str> = commands
                .iter()
                .map(|command| command.name.as_str())
                .collect();
            assert_eq!(names, ["resize-pane", "move-pane"]);
            assert_eq!(apply(FLOAT, &commands), target);
            assert_eq!(
                target.xoff + i32::from(target.sx),
                FLOAT.xoff + i32::from(FLOAT.sx)
            );
            assert_eq!(
                target.yoff + i32::from(target.sy),
                FLOAT.yoff + i32::from(FLOAT.sy)
            );
        }
        let grip = FloatGrip {
            left: true,
            top: true,
            ..FloatGrip::MOVE
        };
        let target = float_drag_preview(FLOAT, grip, (3, -2));
        assert_eq!(
            words(&float_drag_commands(
                PaneId(7),
                grip,
                target,
                PaneBorderLines::Single
            )),
            [
                "resize-pane -t %7 -x 19 -y 10",
                "move-pane -t %7 -X 12 -Y 1"
            ]
        );
    }

    #[test]
    fn a_resize_never_collapses_the_content() {
        let grip = FloatGrip {
            left: true,
            ..FloatGrip::MOVE
        };
        let target = float_drag_preview(FLOAT, grip, (50, 0));
        assert_eq!(target.sx, 1);
        assert_eq!(target.xoff, FLOAT.xoff + i32::from(FLOAT.sx) - 1);
    }
}
