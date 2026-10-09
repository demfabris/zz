use crate::{Bounds, Half};
use std::{
    fmt::Debug,
    ops::{Add, Sub},
};

/// The side of a grid cell, in the units of the bounds. A frame's bounds are
/// mostly a few dozen scaled pixels across, so each takes a cell or a few.
const CELL_SIZE: f64 = 64.;

/// The most cells the grid spans along either axis. Bounds reaching past the
/// last cell are kept in it.
const MAX_CELLS_PER_AXIS: usize = 256;

/// No entry: the end of a cell's list.
const NONE: u32 = u32::MAX;

/// Hands out orderings for bounds inserted one after another, each one
/// greater than that of every bounds inserted before it that it intersects,
/// and no greater than it has to be, so that primitives that don't overlap
/// share an ordering and are drawn in one batch.
///
/// The bounds are kept in a uniform grid over the plane. Each cell lists the
/// bounds that reach into it, newest first, except those that cover it
/// whole: every search that reaches into the cell meets those, so the cell
/// keeps only the greatest of their orderings. Upstream kept an R-tree,
/// while the grid indexes each frame's bounds by the cells they touch.
#[derive(Debug)]
pub(crate) struct BoundsTree<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    grid: Grid<U>,
    /// The bounds with the greatest ordering so far, and that ordering: a
    /// search that meets it has its answer at once.
    max: Option<(Bounds<U>, u32)>,
    /// The bounds inserted since the tree was last cleared, in order, each
    /// with the ordering it was given.
    recorded: Vec<(Bounds<U>, u32)>,
}

/// The grid a [`BoundsTree`] keeps its bounds in. Cell `(column, row)`
/// spans `column * CELL_SIZE` to `(column + 1) * CELL_SIZE` across, and
/// likewise down, except that the first and last column and row reach on
/// without end, so every point of the plane is in exactly one cell.
#[derive(Debug)]
struct Grid<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    columns: usize,
    rows: usize,
    /// Row by row.
    cells: Vec<Cell>,
    /// Every cell's list, linked through `next`.
    entries: Vec<Entry<U>>,
    /// Bounds without a positive width and height. Such bounds may still
    /// meet others, `intersects` being what it is, but have no cells.
    degenerate: Vec<(Bounds<U>, u32)>,
}

#[derive(Clone, Copy, Debug)]
struct Cell {
    /// The newest entry of the cell's list.
    head: u32,
    /// The greatest ordering among the bounds that cover the cell whole.
    cover: u32,
    /// The greatest ordering among all the cell's bounds, covering or not.
    max: u32,
}

impl Cell {
    const EMPTY: Cell = Cell {
        head: NONE,
        cover: 0,
        max: 0,
    };
}

#[derive(Clone, Debug)]
struct Entry<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    bounds: Bounds<U>,
    order: u32,
    /// The greatest ordering of this entry and every one after it in its
    /// list, so a search stops as soon as the rest can't raise its result.
    rest_max: u32,
    next: u32,
}

/// A bounds' extent along one axis, as the grid sees it.
#[derive(Clone, Copy)]
struct Span {
    start: f64,
    end: f64,
    first_cell: usize,
    last_cell: usize,
}

impl Span {
    fn new(start: f64, end: f64, cells: usize) -> Self {
        Span {
            start,
            end,
            first_cell: cell_at(start, cells),
            last_cell: cell_at(end, cells),
        }
    }

    /// Whether this span reaches into the interior of `cell`, of `cells`,
    /// rather than stopping at one of its edges.
    fn enters(&self, cell: usize, cells: usize) -> bool {
        self.start < cell_end(cell, cells) && self.end > cell_start(cell)
    }

    /// Whether this span covers all of `cell`, of `cells`.
    fn covers(&self, cell: usize, cells: usize) -> bool {
        self.start <= cell_start(cell) && self.end >= cell_end(cell, cells)
    }
}

fn cell_at(coordinate: f64, cells: usize) -> usize {
    // `as` truncates toward zero, which is flooring for all that isn't
    // clamped to the first cell anyway; it saturates, and turns NaN into 0.
    ((coordinate * (1. / CELL_SIZE)) as isize).clamp(0, cells as isize - 1) as usize
}

fn cell_start(cell: usize) -> f64 {
    if cell == 0 {
        f64::NEG_INFINITY
    } else {
        cell as f64 * CELL_SIZE
    }
}

fn cell_end(cell: usize, cells: usize) -> f64 {
    if cell + 1 == cells {
        f64::INFINITY
    } else {
        (cell + 1) as f64 * CELL_SIZE
    }
}

impl<U> Grid<U>
where
    U: Clone
        + Debug
        + PartialEq
        + PartialOrd
        + Add<U, Output = U>
        + Sub<Output = U>
        + Half
        + Default
        + Into<f64>,
{
    fn clear(&mut self) {
        self.cells.fill(Cell::EMPTY);
        self.entries.clear();
        self.degenerate.clear();
    }

    /// Whether `bounds` has a positive width and height, and an origin that
    /// is a number, which is what it takes to be kept in cells or searched
    /// for through them.
    #[allow(clippy::eq_op)]
    fn has_area(bounds: &Bounds<U>) -> bool {
        bounds.size.width > U::default()
            && bounds.size.height > U::default()
            && bounds.origin.x == bounds.origin.x
            && bounds.origin.y == bounds.origin.y
    }

    /// Where `bounds` lies along each axis.
    fn spans(&self, bounds: &Bounds<U>) -> (Span, Span) {
        let right = bounds.origin.x.clone() + bounds.size.width.clone();
        let bottom = bounds.origin.y.clone() + bounds.size.height.clone();
        (
            Span::new(bounds.origin.x.clone().into(), right.into(), self.columns),
            Span::new(bounds.origin.y.clone().into(), bottom.into(), self.rows),
        )
    }

    /// How many columns and rows a grid needs to hold `bounds` without
    /// keeping it in its last column or row, if more than it has.
    fn needs(&self, bounds: &Bounds<U>) -> Option<(usize, usize)> {
        let right: f64 = (bounds.origin.x.clone() + bounds.size.width.clone()).into();
        let bottom: f64 = (bounds.origin.y.clone() + bounds.size.height.clone()).into();
        if right <= self.columns as f64 * CELL_SIZE && bottom <= self.rows as f64 * CELL_SIZE {
            return None;
        }
        let needed = |end: f64| {
            ((end / CELL_SIZE).ceil() as isize).clamp(1, MAX_CELLS_PER_AXIS as isize) as usize
        };
        let (columns, rows) = (
            needed(right).max(self.columns),
            needed(bottom).max(self.rows),
        );
        (columns > self.columns || rows > self.rows).then_some((columns, rows))
    }

    /// Makes the grid `columns` by `rows` and empties it.
    fn resize(&mut self, columns: usize, rows: usize) {
        self.columns = columns;
        self.rows = rows;
        self.cells.clear();
        self.cells.resize(columns * rows, Cell::EMPTY);
        self.entries.clear();
        self.degenerate.clear();
    }

    fn add(&mut self, bounds: &Bounds<U>, order: u32) {
        if !Self::has_area(bounds) {
            self.degenerate.push((bounds.clone(), order));
            return;
        }
        let (x, y) = self.spans(bounds);
        for row in y.first_cell..=y.last_cell {
            let covers_row = y.covers(row, self.rows);
            for column in x.first_cell..=x.last_cell {
                let cell = &mut self.cells[row * self.columns + column];
                cell.max = cell.max.max(order);
                if covers_row && x.covers(column, self.columns) {
                    cell.cover = cell.cover.max(order);
                } else {
                    let rest_max = match self.entries.get(cell.head as usize) {
                        Some(next) => next.rest_max.max(order),
                        None => order,
                    };
                    let entry = self.entries.len() as u32;
                    self.entries.push(Entry {
                        bounds: bounds.clone(),
                        order,
                        rest_max,
                        next: cell.head,
                    });
                    cell.head = entry;
                }
            }
        }
    }

    /// The greatest ordering among the bounds that intersect `query`, which
    /// has a positive width and height, or 0.
    ///
    /// Two such bounds that intersect share a point inside both, and the
    /// cell holding that point lists one of them if it doesn't cover it, and
    /// the query reaches into its interior. A bounds that covers a cell whole
    /// meets every query that reaches into its interior, so the cell's cover
    /// counts there without looking at the bounds.
    fn max_intersecting(&self, query: &Bounds<U>) -> u32 {
        let mut max = self
            .degenerate
            .iter()
            .filter(|(bounds, _)| bounds.intersects(query))
            .map(|(_, order)| *order)
            .max()
            .unwrap_or(0);
        let (x, y) = self.spans(query);
        for row in y.first_cell..=y.last_cell {
            let enters_row = y.enters(row, self.rows);
            for column in x.first_cell..=x.last_cell {
                let cell = &self.cells[row * self.columns + column];
                if cell.max <= max {
                    continue;
                }
                if cell.cover > max && enters_row && x.enters(column, self.columns) {
                    max = cell.cover;
                }
                let mut entry_ix = cell.head;
                while let Some(entry) = self.entries.get(entry_ix as usize) {
                    if entry.rest_max <= max {
                        break;
                    }
                    if entry.order > max && entry.bounds.intersects(query) {
                        max = entry.order;
                    }
                    entry_ix = entry.next;
                }
            }
        }
        max
    }
}

impl<U> BoundsTree<U>
where
    U: Clone
        + Debug
        + PartialEq
        + PartialOrd
        + Add<U, Output = U>
        + Sub<Output = U>
        + Half
        + Default
        + Into<f64>,
{
    /// Clears the scene bounds, keeping the allocated grid storage.
    pub fn clear(&mut self) {
        self.grid.clear();
        self.max = None;
        self.recorded.clear();
    }

    /// Inserts bounds into the tree and returns its assigned ordering.
    ///
    /// The ordering is one greater than the maximum ordering of any
    /// existing bounds that intersect with the new bounds.
    pub fn insert(&mut self, new_bounds: Bounds<U>) -> u32 {
        let ordering = self.find_max_ordering(&new_bounds) + 1;
        self.add(&new_bounds, ordering);
        self.recorded.push((new_bounds, ordering));
        ordering
    }

    /// Adds `bounds` with `ordering` to the grid, growing it first if the
    /// bounds reach past it.
    fn add(&mut self, bounds: &Bounds<U>, ordering: u32) {
        if Grid::has_area(bounds)
            && let Some((columns, rows)) = self.grid.needs(bounds)
        {
            self.grid.resize(columns, rows);
            for (recorded, recorded_ordering) in &self.recorded {
                self.grid.add(recorded, *recorded_ordering);
            }
        }
        self.grid.add(bounds, ordering);
        if self.max.as_ref().is_none_or(|(_, max)| *max < ordering) {
            self.max = Some((bounds.clone(), ordering));
        }
    }

    /// Finds the maximum ordering among all bounds that intersect with the query.
    fn find_max_ordering(&self, query: &Bounds<U>) -> u32 {
        if let Some((max_bounds, max)) = &self.max
            && query.intersects(max_bounds)
        {
            return *max;
        }
        if Grid::has_area(query) {
            self.grid.max_intersecting(query)
        } else {
            // Without an area of its own the query may miss the interior of
            // every cell it touches, so the covers can't answer for it.
            self.recorded
                .iter()
                .filter(|(bounds, _)| bounds.intersects(query))
                .map(|(_, ordering)| *ordering)
                .max()
                .unwrap_or(0)
        }
    }
}

impl<U> Default for BoundsTree<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    fn default() -> Self {
        BoundsTree {
            grid: Grid {
                columns: 1,
                rows: 1,
                cells: vec![Cell::EMPTY],
                entries: Vec::new(),
                degenerate: Vec::new(),
            },
            max: None,
            recorded: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundsTree, CELL_SIZE, MAX_CELLS_PER_AXIS};
    use crate::{Bounds, Point, Size};
    use rand::{Rng, SeedableRng};

    #[test]
    fn test_insert() {
        let mut tree = BoundsTree::<f32>::default();
        let bounds1 = Bounds {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 10.0,
                height: 10.0,
            },
        };
        let bounds2 = Bounds {
            origin: Point { x: 5.0, y: 5.0 },
            size: Size {
                width: 10.0,
                height: 10.0,
            },
        };
        let bounds3 = Bounds {
            origin: Point { x: 10.0, y: 10.0 },
            size: Size {
                width: 10.0,
                height: 10.0,
            },
        };

        // Insert the bounds into the tree and verify the order is correct
        assert_eq!(tree.insert(bounds1), 1);
        assert_eq!(tree.insert(bounds2), 2);
        assert_eq!(tree.insert(bounds3), 3);

        // Insert non-overlapping bounds and verify they can reuse orders
        let bounds4 = Bounds {
            origin: Point { x: 20.0, y: 20.0 },
            size: Size {
                width: 10.0,
                height: 10.0,
            },
        };
        let bounds5 = Bounds {
            origin: Point { x: 40.0, y: 40.0 },
            size: Size {
                width: 10.0,
                height: 10.0,
            },
        };
        let bounds6 = Bounds {
            origin: Point { x: 25.0, y: 25.0 },
            size: Size {
                width: 10.0,
                height: 10.0,
            },
        };
        assert_eq!(tree.insert(bounds4), 1); // bounds4 does not overlap with bounds1, bounds2, or bounds3
        assert_eq!(tree.insert(bounds5), 1); // bounds5 does not overlap with any other bounds
        assert_eq!(tree.insert(bounds6), 2); // bounds6 overlaps with bounds4, so it should have a different order
    }

    #[test]
    fn test_random_iterations() {
        let max_bounds = 100;
        for seed in 1..=1000 {
            // let seed = 44;
            let mut tree = BoundsTree::default();
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed as u64);
            let mut expected_quads: Vec<(Bounds<f32>, u32)> = Vec::new();

            // Insert a random number of random AABBs into the tree.
            let num_bounds = rng.random_range(1..=max_bounds);
            for _ in 0..num_bounds {
                let min_x: f32 = rng.random_range(-100.0..100.0);
                let min_y: f32 = rng.random_range(-100.0..100.0);
                let width: f32 = rng.random_range(0.0..50.0);
                let height: f32 = rng.random_range(0.0..50.0);
                let bounds = Bounds {
                    origin: Point { x: min_x, y: min_y },
                    size: Size { width, height },
                };

                let expected_ordering = expected_quads
                    .iter()
                    .filter_map(|quad| quad.0.intersects(&bounds).then_some(quad.1))
                    .max()
                    .unwrap_or(0)
                    + 1;
                expected_quads.push((bounds, expected_ordering));

                // Insert the AABB into the tree and collect intersections.
                let actual_ordering = tree.insert(bounds);
                assert_eq!(actual_ordering, expected_ordering);
            }
        }
    }

    /// Large random frames check each ordering against all earlier bounds.
    #[test]
    fn test_large_random_iterations() {
        for seed in 1..=10 {
            let mut tree = BoundsTree::default();
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed as u64);
            let mut expected_quads: Vec<(Bounds<f32>, u32)> = Vec::new();
            for _ in 0..2000 {
                let bounds = Bounds {
                    origin: Point {
                        x: rng.random_range(-1000.0..1000.0),
                        y: rng.random_range(-1000.0..1000.0),
                    },
                    size: Size {
                        width: rng.random_range(0.0..80.0),
                        height: rng.random_range(0.0..80.0),
                    },
                };
                let expected_ordering = expected_quads
                    .iter()
                    .filter_map(|quad| quad.0.intersects(&bounds).then_some(quad.1))
                    .max()
                    .unwrap_or(0)
                    + 1;
                expected_quads.push((bounds, expected_ordering));
                assert_eq!(tree.insert(bounds), expected_ordering);
            }
        }
    }

    /// Bounds the grid can't simply file under the cells they reach into:
    /// ones on cells' edges, covering cells whole or reaching past the grid,
    /// without a width or height, negative, infinite or not a number. Each
    /// still gets the ordering comparing it with every bounds before it gives.
    #[test]
    fn awkward_bounds_are_ordered_as_comparing_them_with_all_would() {
        let edge = CELL_SIZE as f32;
        let coordinate = |rng: &mut rand::rngs::StdRng| match rng.random_range(0..12) {
            0 => f32::INFINITY,
            1 => f32::NEG_INFINITY,
            2 => f32::NAN,
            3 => rng.random_range(-4..(MAX_CELLS_PER_AXIS as i32 + 4)) as f32 * edge,
            4..8 => rng.random_range(-3..8) as f32 * edge,
            _ => rng.random_range(-2.0 * edge..6.0 * edge),
        };
        let length = |rng: &mut rand::rngs::StdRng| match rng.random_range(0..10) {
            0 => 0.,
            1 => -rng.random_range(0.0..edge),
            2 => f32::INFINITY,
            3..6 => rng.random_range(0..4) as f32 * edge,
            _ => rng.random_range(0.0..3.0 * edge),
        };
        for seed in 1..=400 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let mut tree = BoundsTree::default();
            for _ in 0..3 {
                let frame: Vec<_> = (0..rng.random_range(1..200))
                    .map(|_| Bounds {
                        origin: Point {
                            x: coordinate(&mut rng),
                            y: coordinate(&mut rng),
                        },
                        size: Size {
                            width: length(&mut rng),
                            height: length(&mut rng),
                        },
                    })
                    .collect();
                tree.clear();
                let mut inserted: Vec<(Bounds<f32>, u32)> = Vec::new();
                for bounds in &frame {
                    let expected = inserted
                        .iter()
                        .filter_map(|(other, order)| other.intersects(bounds).then_some(*order))
                        .max()
                        .unwrap_or(0)
                        + 1;
                    assert_eq!(tree.insert(*bounds), expected, "seed {seed}: {bounds:?}");
                    inserted.push((*bounds, expected));
                }
            }
        }
    }
}
