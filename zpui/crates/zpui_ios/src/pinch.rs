use zpui::{Pixels, Point, TouchPhase, point};

const SLOP: f32 = 0.06;
const SLOP_POINTS: f32 = 8.0;

pub struct Pinch {
    keys: [usize; 2],
    down: [bool; 2],
    positions: [Point<Pixels>; 2],
    start: f32,
    base: Option<f32>,
    scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinchStep {
    pub phase: TouchPhase,
    pub delta: f32,
    pub position: Point<Pixels>,
}

impl Pinch {
    pub fn new(keys: [usize; 2], positions: [Point<Pixels>; 2]) -> Self {
        Self {
            keys,
            down: [true; 2],
            positions,
            start: distance(positions),
            base: None,
            scale: 1.0,
        }
    }

    pub fn owns(&self, key: usize) -> bool {
        self.slot(key).is_some()
    }

    pub fn finished(&self) -> bool {
        !self.down[0] && !self.down[1]
    }

    pub fn touch(
        &mut self,
        key: usize,
        phase: TouchPhase,
        position: Point<Pixels>,
    ) -> Option<PinchStep> {
        let slot = self.slot(key)?;
        let both_down = self.down[0] && self.down[1];
        match phase {
            TouchPhase::Moved if both_down => {
                self.positions[slot] = position;
                self.track()
            }
            TouchPhase::Started | TouchPhase::Moved => None,
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.down[slot] = false;
                (both_down && self.base.is_some()).then(|| PinchStep {
                    phase,
                    delta: 0.0,
                    position: self.center(),
                })
            }
        }
    }

    fn slot(&self, key: usize) -> Option<usize> {
        (0..2).find(|&slot| self.down[slot] && self.keys[slot] == key)
    }

    fn center(&self) -> Point<Pixels> {
        let [a, b] = self.positions;
        point((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
    }

    fn track(&mut self) -> Option<PinchStep> {
        let span = distance(self.positions);
        let position = self.center();
        let Some(base) = self.base else {
            if (span - self.start).abs() <= (self.start * SLOP).max(SLOP_POINTS) {
                return None;
            }
            self.base = Some(span.max(SLOP_POINTS));
            return Some(PinchStep {
                phase: TouchPhase::Started,
                delta: 0.0,
                position,
            });
        };
        let scale = span / base;
        let delta = scale - self.scale;
        self.scale = scale;
        (delta != 0.0).then_some(PinchStep {
            phase: TouchPhase::Moved,
            delta,
            position,
        })
    }
}

fn distance([a, b]: [Point<Pixels>; 2]) -> f32 {
    let (x, y) = (f32::from(a.x - b.x), f32::from(a.y - b.y));
    (x * x + y * y).sqrt()
}
