//! Managing [render states](RenderState) of the terminal.

use std::{convert::Into, marker::PhantomData, mem::MaybeUninit};

use crate::{
    alloc::{Allocator, Object},
    error::{Error, Result, from_optional_result, from_result},
    ffi,
    screen::{Cell, CellContentTag, CellSemanticContent, CellWide, Row},
    style::{PaletteIndex, RgbColor, Style},
    terminal::Terminal,
};

pub use ffi::RenderStateRowSelection as RowSelection;

/// Represents the state required to render a visible screen (a viewport) of
/// a terminal instance.
///
/// This is stateful and optimized for repeated updates from a single terminal
/// instance and only updating dirty regions of the screen.
///
/// The key design principle of this API is that it only needs read/write
/// access to the terminal instance during the update call. This allows the
/// render state to minimally impact terminal IO performance and also allows
/// the renderer to be safely multi-threaded (as long as a lock is held
/// during the update call to ensure exclusive access to the terminal instance).
///
/// The basic usage of this API is:
///
///  1. Create an empty render state
///  2. Update it from a terminal instance whenever you need.
///  3. Read from the render state to get the data needed to draw your frame.
///
/// # Dirty Tracking
///
/// Dirty tracking is a key feature of the render state that allows renderers
/// to efficiently determine what parts of the screen have changed and only
/// redraw changed regions.
///
/// The render state API keeps track of dirty state at two independent layers:
/// a global dirty state that indicates whether the entire frame is clean,
/// partially dirty, or fully dirty, and a per-row dirty state that allows
/// tracking which rows in a partially dirty frame have changed.
///
/// The user of the render state API is expected to unset both of these.
/// The update call does not unset dirty state, it only updates it.
///
/// An extremely important detail: **setting one dirty state doesn't unset
/// the other.** For example, setting the global dirty state to false does
/// not reset the row-level dirty flags. So, the caller of the render state
/// API must be careful to manage both layers of dirty state correctly.
///
/// # Examples
///
/// ## Creating and updating render state
///
/// ```rust
/// // Create a terminal and render state, then update the render state
/// // from the terminal. The render state captures a snapshot of everything
/// // needed to draw a frame.
/// use libghostty_vt::{Terminal, RenderState};
///
/// let mut terminal = Terminal::new(40, 5).unwrap();
/// let mut render_state = RenderState::new().unwrap();
///
/// // Feed some styled content into the terminal.
/// terminal.vt_write(b"Hello, \x1b[1;32mworld\x1b[0m!\r\n");
/// terminal.vt_write(b"\x1b[4munderlined\x1b[0m text\r\n");
/// terminal.vt_write(b"\x1b[38;2;255;128;0morange\x1b[0m\r\n");
///
/// assert!(render_state.update(&terminal).is_ok());
/// ```
///
/// ## Splitting an update
///
/// ```rust
/// use libghostty_vt::{RenderState, Terminal};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let terminal = Terminal::new(80, 25)?;
/// let mut render_state = RenderState::new()?;
///
/// // Use `update` unless you need to minimize how long terminal access is
/// // held. `begin_update` copies the terminal-dependent state into an update
/// // token, then `end` finishes the deferred render-state work.
/// let update = render_state.begin_update(&terminal)?;
///
/// // Terminal access is no longer needed here.
/// let snapshot = update.end()?;
///
/// // Read from the snapshot to draw the frame.
/// let _dirty = snapshot.dirty()?;
/// # Ok(())}
/// ```
///
/// ## Checking dirty state
///
/// ```rust
/// // Check the global dirty state to decide how much work the renderer
/// // needs to do. After rendering, reset it to false.
/// # use libghostty_vt::{Terminal, RenderState, render::Dirty};
/// # let terminal = Terminal::new(80, 25).unwrap();
/// # let mut render_state = RenderState::new().unwrap();
/// let snapshot = render_state.update(&terminal).unwrap();
///
/// match snapshot.dirty().unwrap() {
///     Dirty::Clean => println!("Frame is clean, nothing to draw."),
///     Dirty::Partial => println!("Partial redraw needed."),
///     Dirty::Full => println!("Full redraw needed."),
/// }
/// ```
///
/// ## Reading colors
///
/// ```rust
/// // Retrieve colors (background, foreground, palette) from the render
/// // state. These are needed to resolve palette-indexed cell colors.
/// # use libghostty_vt::{Terminal, RenderState};
/// # let terminal = Terminal::new(80, 25).unwrap();
/// # let mut render_state = RenderState::new().unwrap();
/// let snapshot = render_state.update(&terminal).unwrap();
/// let colors = snapshot.colors().unwrap();
///
/// println!(
///     "Background: {:02x}{:02x}{:02x}",
///     colors.background.r, colors.background.g, colors.background.b
/// );
/// println!(
///     "Foreground: {:02x}{:02x}{:02x}",
///     colors.background.r, colors.background.g, colors.background.b
/// );
/// ```
///
/// ## Reading cursor state
///
/// ```rust
/// // Read cursor position and visual style from the render state.
/// use libghostty_vt::render::CursorViewport;
/// # use libghostty_vt::{Terminal, RenderState};
/// # let terminal = Terminal::new(80, 25).unwrap();
/// # let mut render_state = RenderState::new().unwrap();
/// let snapshot = render_state.update(&terminal).unwrap();
///
/// if snapshot.cursor_visible().unwrap() {
///     if let Some(CursorViewport { x, y, .. }) = snapshot.cursor_viewport().unwrap() {
///         let style = snapshot.cursor_visual_style().unwrap();
///         println!("Cursor at ({x}, {y}), style {style:?}");
///     }
/// }
/// ```
///
/// ## Iterating rows and cells
///
/// ```rust
/// // Iterate rows via the row iterator. For each dirty row, iterate its
/// // cells, read codepoints/graphemes and styles, and emit ANSI-colored
/// // output as a simple "renderer".
/// use libghostty_vt::{Terminal, RenderState};
/// use libghostty_vt::style::Underline;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let terminal = Terminal::new(80, 25).unwrap();
/// # let mut render_state = RenderState::new()?;
/// use libghostty_vt::render::{RowIterator, CellIterator};
///
/// // During setup:
/// let mut rows = RowIterator::new()?;
/// let mut cells = CellIterator::new()?;
///
/// // On each frame:
/// let snapshot = render_state.update(&terminal)?;
/// let colors = snapshot.colors()?;
///
/// let mut row_iter = rows.update(&snapshot)?;
/// let mut row_index = 0;
///
/// while let Some(row) = row_iter.next() {
///     // Check per-row dirty state; a real renderer would skip clean rows.
///     print!(
///         "Row {row_index} [{}]",
///         if row.dirty()? { "dirty" } else { "clean" }
///     );
///
///     // Get cells for this row (reuses the same cells handle).
///     let mut cell_iter = cells.update(&row)?;
///     while let Some(cell) = cell_iter.next() {
///         let graphemes = cell.graphemes()?;
///
///         if graphemes.is_empty() {
///             print!(" ");
///             continue;
///         }
///
///         // Resolve foreground color for this cell.
///         let fg = cell.fg_color()?.unwrap_or(colors.foreground);
///         // Emit ANSI true-color escape for the foreground.
///         print!("\x1b[38;2;{};{};{}m", fg.r, fg.g, fg.b);
///
///         // Read the style for this cell. Returns the default style for
///         // cells that have no explicit styling.
///         let style = cell.style()?;
///         if style.bold {
///             print!("\x1b[1m");
///         }
///         if style.underline != Underline::None {
///             print!("\x1b[4m");
///         }
///
///         for grapheme in graphemes {
///             print!("{}", grapheme.escape_default());
///         }
///         print!("\x1b[0m"); // Reset style after each cell.
///     }
///     println!();
///
///     // Clear per-row dirty flag after "rendering" it.
///     row.set_dirty(false);
///
///     row_index += 1;
/// }
/// # Ok(())}
/// ```
#[derive(Debug)]
pub struct RenderState<'alloc>(Object<'alloc, ffi::RenderStateImpl>);

/// A snapshot of the render state after an update.
///
/// This struct exists to guard data accessed from the render state from
/// being accidentally modified after an update. If you find yourself unable
/// to update the render state due to borrow checker errors, make sure to
/// drop the active snapshot (and data that depends on it) before updating.
#[derive(Debug)]
pub struct Snapshot<'alloc, 's>(&'s mut RenderState<'alloc>);

/// An in-progress render state update.
///
/// This token is returned by [`RenderState::begin_update`] and keeps the render
/// state borrowed until [`Self::end`] completes the deferred update work. This
/// makes it impossible to read from the render state while it is incomplete.
#[derive(Debug)]
pub struct Update<'alloc, 's> {
    state: Option<&'s mut RenderState<'alloc>>,
}

/// Opaque handle to a render-state row iterator.
///
/// The row iterator must be [updated](RowIterator::update) from a snapshot of
/// the render state in order to function, as most data is only accessible
/// per [iteration](RowIteration).
#[derive(Debug)]
pub struct RowIterator<'alloc>(Object<'alloc, ffi::RenderStateRowIteratorImpl>);

/// An active iteration over the rows in the render state.
///
/// Row iterations are created by [updating](RowIterator::update) row iterators
/// with a snapshot of the render state. The borrow checker statically
/// guarantees that all accesses of the data do not outlive the given snapshot,
/// at the cost of added lifetime annotations.
#[derive(Debug)]
pub struct RowIteration<'alloc, 's> {
    iter: &'s mut RowIterator<'alloc>,
    // NOTE: While in theory the snapshot borrow should have its own
    // lifetime 'ss where 'rs: 'ss, but it gets very unwieldy and honestly
    // one wouldn't run into too many situations where this simpler constraint
    // isn't enough.
    _phan: PhantomData<&'s Snapshot<'alloc, 's>>,
}

/// Opaque handle to a render state cell iterator.
///
/// The cell iterator must be [updated](CellIterator::update) from a
/// [row](RowIteration) in order to function, as most data is only
/// accessible per [iteration](CellIteration).
#[derive(Debug)]
pub struct CellIterator<'alloc>(Object<'alloc, ffi::RenderStateRowCellsImpl>);

/// An active iteration over the cells on a given row
/// within the render state.
///
/// Cell iterations are created by [updating](CellIterator::update) row iterators
/// at a given [row](RowIteration). The borrow checker statically
/// guarantees that all accesses of the data do not outlive the given snapshot,
/// at the cost of added lifetime annotations.
#[derive(Debug)]
pub struct CellIteration<'alloc, 's> {
    iter: &'s mut CellIterator<'alloc>,
    _phan: PhantomData<&'s RowIteration<'alloc, 's>>,
}

//--------------------------
// Impl blocks
//--------------------------

impl<'alloc> RenderState<'alloc> {
    /// Create a new render state instance.
    pub fn new() -> Result<Self> {
        // SAFETY: A NULL allocator is always valid
        unsafe { Self::new_inner(std::ptr::null()) }
    }

    /// Create a new render state instance with a custom allocator.
    ///
    /// See the [crate-level documentation](crate#memory-management-and-lifetimes)
    /// regarding custom memory management and lifetimes.
    pub fn new_with_alloc<'ctx: 'alloc>(alloc: &'alloc Allocator<'ctx>) -> Result<Self> {
        // SAFETY: Borrow checking should forbid invalid allocators
        unsafe { Self::new_inner(alloc.to_raw()) }
    }

    unsafe fn new_inner(alloc: *const ffi::Allocator) -> Result<Self> {
        let mut raw: ffi::RenderState = std::ptr::null_mut();
        let result = unsafe { ffi::ghostty_render_state_new(alloc, &raw mut raw) };
        from_result(result)?;
        Ok(Self(Object::new(raw)?))
    }

    /// Update a render state instance from a terminal,
    /// returning a new [snapshot](Snapshot).
    ///
    /// This consumes terminal/screen dirty state in the same way as the
    /// internal render state update path.
    ///
    /// # Errors
    ///
    /// Returns `Err(Error::OutOfMemory)` if updating the state requires
    /// allocation and that allocation fails.
    pub fn update<'cb>(
        &mut self,
        terminal: &Terminal<'alloc, 'cb>,
    ) -> Result<Snapshot<'alloc, '_>> {
        let result =
            unsafe { ffi::ghostty_render_state_update(self.0.as_raw(), terminal.inner.as_raw()) };
        from_result(result)?;
        Ok(Snapshot(self))
    }

    /// Begin an update of a render state instance from a terminal.
    ///
    /// Every begin must be completed with [`Update::end`] before the render
    /// state is read.
    ///
    /// This two-phase structure exists for callers that synchronize access to
    /// the terminal state: only this function requires terminal access, so a
    /// caller can hold its lock for this call only and then call [`Update::end`]
    /// after releasing it. The end phase exclusively reads
    /// and writes memory owned by the render state, so it is safe to call while
    /// the terminal is being modified.
    ///
    /// Work that doesn't require terminal access may be deferred to the end
    /// phase to keep this call, and therefore lock hold time, as short as
    /// possible. Callers must treat the render state as incomplete until
    /// [`Update::end`] is called.
    ///
    /// This consumes terminal and screen dirty state in the same way as the
    /// internal render state update path.
    pub fn begin_update<'cb>(
        &mut self,
        terminal: &Terminal<'alloc, 'cb>,
    ) -> Result<Update<'alloc, '_>> {
        let result = unsafe {
            ffi::ghostty_render_state_begin_update(self.0.as_raw(), terminal.inner.as_raw())
        };
        from_result(result)?;
        Ok(Update { state: Some(self) })
    }

    /// Capture only part of the viewport from the next update on.
    ///
    /// Rows and columns outside the clip are not copied, so an update of a
    /// very large viewport costs what the clip holds. Rows, cells, and the
    /// cursor keep viewport coordinates; read [`Snapshot::clip`] for what
    /// an update captured. The default [`Clip`] captures the whole viewport.
    /// Expect a full redraw on the update after a change.
    pub fn set_clip(&mut self, clip: Clip) -> Result<()> {
        let raw = ffi::RenderStateClip {
            y: clip.y,
            rows: clip.rows,
            cols: clip.cols,
        };
        let result = unsafe {
            ffi::ghostty_render_state_set(
                self.0.as_raw(),
                ffi::RenderStateOption::CLIP,
                std::ptr::from_ref(&raw).cast(),
            )
        };
        from_result(result)
    }
}

impl Drop for RenderState<'_> {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_render_state_free(self.0.as_raw()) }
    }
}

impl<'alloc, 's> Update<'alloc, 's> {
    /// Complete a prior [`RenderState::begin_update`] call by performing any deferred work.
    ///
    /// This only reads and writes memory owned by the render state, so it is
    /// safe to call while the terminal is being modified. Consumes the update
    /// token and returns a snapshot that can be read to draw the frame.
    pub fn end(mut self) -> Result<Snapshot<'alloc, 's>> {
        let Some(state) = self.state.take() else {
            return Err(Error::InvalidValue);
        };
        let result = unsafe { ffi::ghostty_render_state_end_update(state.0.as_raw()) };
        from_result(result)?;
        Ok(Snapshot(state))
    }
}

impl Drop for Update<'_, '_> {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            let _ = unsafe { ffi::ghostty_render_state_end_update(state.0.as_raw()) };
        }
    }
}

impl Snapshot<'_, '_> {
    fn get<T>(&self, tag: ffi::RenderStateData::Type) -> Result<T> {
        let mut value = MaybeUninit::<T>::zeroed();
        let result = unsafe {
            ffi::ghostty_render_state_get(self.0.0.as_raw(), tag, value.as_mut_ptr().cast())
        };
        // Since we manually model every possible query, this should never fail.
        from_result(result)?;
        // SAFETY: Value should be initialized after successful call.
        Ok(unsafe { value.assume_init() })
    }

    fn set<T>(&self, tag: ffi::RenderStateOption::Type, value: &T) -> Result<()> {
        let result = unsafe {
            ffi::ghostty_render_state_set(self.0.0.as_raw(), tag, std::ptr::from_ref(value).cast())
        };
        // Since we manually model every possible query, this should never fail.
        from_result(result)
    }

    /// Get the current dirty state.
    pub fn dirty(&self) -> Result<Dirty> {
        self.get::<ffi::RenderStateDirty::Type>(ffi::RenderStateData::DIRTY)
            .and_then(|v| v.try_into().map_err(|_| Error::InvalidValue))
    }

    /// Get the viewport width.
    pub fn cols(&self) -> Result<u16> {
        self.get(ffi::RenderStateData::COLS)
    }

    /// Get the viewport height.
    pub fn rows(&self) -> Result<u16> {
        self.get(ffi::RenderStateData::ROWS)
    }

    /// The part of the viewport this snapshot captured: the
    /// [`RenderState::set_clip`] request limited to the viewport, with zero
    /// counts replaced by the rows and columns they stand for.
    pub fn clip(&self) -> Result<Clip> {
        let raw: ffi::RenderStateClip = self.get(ffi::RenderStateData::CLIP)?;
        Ok(Clip {
            y: raw.y,
            rows: raw.rows,
            cols: raw.cols,
        })
    }

    /// Get the cursor color that may have been explicitly set by the terminal state.
    pub fn cursor_color(&self) -> Result<Option<RgbColor>> {
        let has_value = self.get(ffi::RenderStateData::COLOR_CURSOR_HAS_VALUE)?;
        if has_value {
            let color = self.get(ffi::RenderStateData::COLOR_CURSOR)?;
            Ok(Some(color))
        } else {
            Ok(None)
        }
    }

    /// Whether the cursor is currently visible based on terminal modes.
    pub fn cursor_visible(&self) -> Result<bool> {
        self.get(ffi::RenderStateData::CURSOR_VISIBLE)
    }

    /// Whether the cursor is currently blinking based on terminal modes.
    pub fn cursor_blinking(&self) -> Result<bool> {
        self.get(ffi::RenderStateData::CURSOR_BLINKING)
    }

    /// Whether the cursor is at a password input field.
    pub fn cursor_password_input(&self) -> Result<bool> {
        self.get(ffi::RenderStateData::CURSOR_PASSWORD_INPUT)
    }

    /// Get the visual style of the cursor.
    pub fn cursor_visual_style(&self) -> Result<CursorVisualStyle> {
        self.get::<ffi::RenderStateCursorVisualStyle::Type>(
            ffi::RenderStateData::CURSOR_VISUAL_STYLE,
        )
        .and_then(|v| v.try_into().map_err(|_| Error::InvalidValue))
    }

    /// Get the relative position of the cursor and other information
    /// if it is currently visible within the viewport.
    pub fn cursor_viewport(&self) -> Result<Option<CursorViewport>> {
        let has_value = self.get(ffi::RenderStateData::CURSOR_VIEWPORT_HAS_VALUE)?;
        if has_value {
            let x = self.get(ffi::RenderStateData::CURSOR_VIEWPORT_X)?;
            let y = self.get(ffi::RenderStateData::CURSOR_VIEWPORT_Y)?;
            let at_wide_tail = self.get(ffi::RenderStateData::CURSOR_VIEWPORT_WIDE_TAIL)?;
            Ok(Some(CursorViewport { x, y, at_wide_tail }))
        } else {
            Ok(None)
        }
    }

    /// Get the current color information from a render state.
    pub fn colors(&self) -> Result<Colors> {
        let mut colors = ffi::sized!(ffi::RenderStateColors);
        let result = unsafe {
            ffi::ghostty_render_state_get(
                self.0.0.as_raw(),
                ffi::RenderStateData::COLORS,
                (&raw mut colors).cast(),
            )
        };
        from_result(result)?;

        Ok(Colors {
            background: colors.background.into(),
            foreground: colors.foreground.into(),
            cursor: if colors.cursor_has_value {
                Some(colors.cursor.into())
            } else {
                None
            },
            palette: colors.palette.map(Into::into),
        })
    }

    /// Set dirty state.
    pub fn set_dirty(&self, dirty: Dirty) -> Result<()> {
        self.set(
            ffi::RenderStateOption::DIRTY,
            &(dirty as ffi::RenderStateDirty::Type),
        )
    }
}

impl<'alloc> RowIterator<'alloc> {
    /// Create a new row iterator instance.
    pub fn new() -> Result<Self> {
        // SAFETY: A NULL allocator is always valid
        unsafe { Self::new_inner(std::ptr::null()) }
    }

    /// Create a new cell iterator instance with a custom allocator.
    ///
    /// See the [crate-level documentation](crate#memory-management-and-lifetimes)
    /// regarding custom memory management and lifetimes.
    pub fn new_with_alloc<'ctx: 'alloc>(alloc: &'alloc Allocator<'ctx>) -> Result<Self> {
        // SAFETY: Borrow checking should forbid invalid allocators
        unsafe { Self::new_inner(alloc.to_raw()) }
    }

    unsafe fn new_inner(alloc: *const ffi::Allocator) -> Result<Self> {
        let mut raw: ffi::RenderStateRowIterator = std::ptr::null_mut();
        let result = unsafe { ffi::ghostty_render_state_row_iterator_new(alloc, &raw mut raw) };
        from_result(result)?;
        Ok(Self(Object::new(raw)?))
    }

    /// Update the row iterator for a snapshot of the render state,
    /// returning a new row iteration.
    pub fn update<'s>(
        &'s mut self,
        snapshot: &'s Snapshot<'alloc, '_>,
    ) -> Result<RowIteration<'alloc, 's>> {
        let result = unsafe {
            ffi::ghostty_render_state_get(
                snapshot.0.0.as_raw(),
                ffi::RenderStateData::ROW_ITERATOR,
                std::ptr::from_mut(&mut self.0.ptr).cast(),
            )
        };
        from_result(result)?;

        Ok(RowIteration {
            iter: self,
            _phan: PhantomData,
        })
    }
}

impl Drop for RowIterator<'_> {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_render_state_row_iterator_free(self.0.as_raw()) }
    }
}

impl RowIteration<'_, '_> {
    /// Move a row iteration to the next row.
    ///
    /// Returns `Some(row)` if the iteration moved successfully and row
    /// data is available to read at the new position using `row`.
    #[expect(
        clippy::should_implement_trait,
        reason = "lending `next` cannot implement trait"
    )]
    pub fn next(&mut self) -> Option<&Self> {
        if unsafe { ffi::ghostty_render_state_row_iterator_next(self.iter.0.as_raw()) } {
            Some(self)
        } else {
            None
        }
    }

    fn get<T>(&self, tag: ffi::RenderStateRowData::Type) -> Result<T> {
        let mut value = MaybeUninit::<T>::zeroed();
        let result = unsafe {
            ffi::ghostty_render_state_row_get(self.iter.0.as_raw(), tag, value.as_mut_ptr().cast())
        };
        // Since we manually model every possible query, this should never fail.
        from_result(result)?;
        // SAFETY: Value should be initialized after successful call.
        Ok(unsafe { value.assume_init() })
    }

    fn set<T>(&self, tag: ffi::RenderStateRowOption::Type, value: &T) -> Result<()> {
        let result = unsafe {
            ffi::ghostty_render_state_row_set(
                self.iter.0.as_raw(),
                tag,
                std::ptr::from_ref(value).cast(),
            )
        };
        from_result(result)
    }

    /// Whether the current row is dirty.
    pub fn dirty(&self) -> Result<bool> {
        self.get(ffi::RenderStateRowData::DIRTY)
    }

    /// The raw row value.
    pub fn raw_row(&self) -> Result<Row> {
        self.get(ffi::RenderStateRowData::RAW).map(Row)
    }

    /// Set dirty state for the current row.
    pub fn set_dirty(&self, dirty: bool) -> Result<()> {
        self.set(ffi::RenderStateRowOption::DIRTY, &dirty)
    }

    /// Row-local selected cell range.
    pub fn selection(&self) -> Result<Option<RowSelection>> {
        let mut value = ffi::sized!(RowSelection);
        let result = unsafe {
            ffi::ghostty_render_state_row_get(
                self.iter.0.as_raw(),
                ffi::RenderStateRowData::SELECTION,
                std::ptr::from_mut(&mut value).cast(),
            )
        };
        // Since we manually model every possible query, this should never fail.
        // SAFETY: Value should be initialized after successful call.
        from_optional_result(result, value)
    }
}

impl<'alloc> CellIterator<'alloc> {
    /// Create a new cell iterator instance.
    pub fn new() -> Result<Self> {
        // SAFETY: A NULL allocator is always valid
        unsafe { Self::new_inner(std::ptr::null()) }
    }

    /// Create a new cell iterator instance with a custom allocator.
    ///
    /// See the [crate-level documentation](crate#memory-management-and-lifetimes)
    /// regarding custom memory management and lifetimes.
    pub fn new_with_alloc<'ctx: 'alloc>(alloc: &'alloc Allocator<'ctx>) -> Result<Self> {
        // SAFETY: Borrow checking should forbid invalid allocators
        unsafe { Self::new_inner(alloc.to_raw()) }
    }

    unsafe fn new_inner(alloc: *const ffi::Allocator) -> Result<Self> {
        let mut raw: ffi::RenderStateRowCells = std::ptr::null_mut();
        let result = unsafe { ffi::ghostty_render_state_row_cells_new(alloc, &raw mut raw) };
        from_result(result)?;
        Ok(Self(Object::new(raw)?))
    }

    /// Update the cell iterator for a new row iteration,
    /// returning a new cell iteration.
    pub fn update<'s>(
        &'s mut self,
        row: &'s RowIteration<'alloc, '_>,
    ) -> Result<CellIteration<'alloc, 's>> {
        let result = unsafe {
            ffi::ghostty_render_state_row_get(
                row.iter.0.as_raw(),
                ffi::RenderStateRowData::CELLS,
                std::ptr::from_mut(&mut self.0.ptr).cast(),
            )
        };
        from_result(result)?;

        Ok(CellIteration {
            iter: self,
            _phan: PhantomData,
        })
    }
}

impl Drop for CellIterator<'_> {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_render_state_row_cells_free(self.0.as_raw()) }
    }
}

impl CellIteration<'_, '_> {
    /// Move a cell iteration to the next cell.
    ///
    /// Returns `Some(cell)` if the iteration moved successfully and cell
    /// data is available to read at the new position using `cell`.
    #[expect(
        clippy::should_implement_trait,
        reason = "lending `next` cannot implement trait"
    )]
    pub fn next(&mut self) -> Option<&Self> {
        if unsafe { ffi::ghostty_render_state_row_cells_next(self.iter.0.as_raw()) } {
            Some(self)
        } else {
            None
        }
    }

    /// Move a cell iteration to a specific column.
    ///
    /// Positions the iteration at the given x (column) index so that
    /// subsequent reads return data for that cell.
    pub fn select(&mut self, x: u16) -> Result<()> {
        let result = unsafe { ffi::ghostty_render_state_row_cells_select(self.iter.0.as_raw(), x) };
        from_result(result)
    }

    fn get<T>(&self, tag: ffi::RenderStateRowCellsData::Type) -> Result<T> {
        let mut value = MaybeUninit::<T>::zeroed();
        let result = unsafe {
            ffi::ghostty_render_state_row_cells_get(
                self.iter.0.as_raw(),
                tag,
                value.as_mut_ptr().cast(),
            )
        };
        from_result(result)?;
        // SAFETY: Value should be initialized after successful call.
        Ok(unsafe { value.assume_init() })
    }

    /// The raw cell value.
    pub fn raw_cell(&self) -> Result<Cell> {
        self.get(ffi::RenderStateRowCellsData::RAW).map(Cell)
    }

    /// The style for the current cell.
    pub fn style(&self) -> Result<Style> {
        let mut value = ffi::sized!(ffi::Style);
        let result = unsafe {
            ffi::ghostty_render_state_row_cells_get(
                self.iter.0.as_raw(),
                ffi::RenderStateRowCellsData::STYLE,
                std::ptr::from_mut(&mut value).cast(),
            )
        };
        from_result(result)?;
        Style::try_from(value)
    }

    /// The resolved foreground color of the cell.
    ///
    /// Resolves palette indices through the palette. Bold color handling
    /// is not applied; the caller should handle bold styling separately.
    ///
    /// Returns `None` if the cell has no explicit foreground color, in which
    /// case the caller should use whatever default foreground color it want
    /// (e.g. the terminal foreground).
    pub fn fg_color(&self) -> Result<Option<RgbColor>> {
        let res = self.get::<ffi::ColorRgb>(ffi::RenderStateRowCellsData::FG_COLOR);
        match res {
            Ok(o) => Ok(Some(o.into())),
            Err(Error::InvalidValue) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The resolved background color of the cell.
    ///
    /// Flattens the three possible sources: [`Cell::bg_color_rgb`],
    /// [`Cell::bg_color_palette`] (looked up in the palette), or the
    /// style's [`bg_color`][Style::bg_color].
    ///
    /// Returns `None` if the cell has no background color, in which case the
    /// caller should use whatever default background color it wants
    /// (e.g. the terminal background).
    pub fn bg_color(&self) -> Result<Option<RgbColor>> {
        let res = self.get::<ffi::ColorRgb>(ffi::RenderStateRowCellsData::BG_COLOR);
        match res {
            Ok(o) => Ok(Some(o.into())),
            Err(Error::InvalidValue) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get the grapheme codepoints.
    ///
    /// The base codepoint is placed first, followed by any extra codepoints.
    pub fn graphemes(&self) -> Result<Vec<char>> {
        let len = self.graphemes_len()?;
        let mut graphemes = vec!['\0'; len];
        self.graphemes_buf(&mut graphemes)?;
        Ok(graphemes)
    }

    /// The total number of grapheme codepoints including the base codepoint.
    ///
    /// Returns 0 if the cell has no text.
    pub fn graphemes_len(&self) -> Result<usize> {
        self.get(ffi::RenderStateRowCellsData::GRAPHEMES_LEN)
    }

    /// Write grapheme codepoints into a caller-provided buffer.
    ///
    /// The buffer must be at least [`CellIteration::graphemes_len`] elements.
    /// The base codepoint is written first, followed by any extra codepoints.
    pub fn graphemes_buf(&self, buf: &mut [char]) -> Result<()> {
        let result = unsafe {
            ffi::ghostty_render_state_row_cells_get(
                self.iter.0.as_raw(),
                ffi::RenderStateRowCellsData::GRAPHEMES_BUF,
                buf.as_mut_ptr().cast(),
            )
        };
        from_result(result)
    }

    /// Encode the current cell's full grapheme cluster as UTF-8 into a
    /// caller-provided string buffer.
    ///
    /// The base codepoint is encoded first, followed by any extra grapheme
    /// codepoints.
    ///
    /// May grow the buffer if more space is required.
    pub fn graphemes_utf8(&self, buf: &mut String) -> Result<()> {
        // SAFETY: String comes with some very stringent safety requirements,
        // so we'll detail them here. The safety protocol for the C API is
        // essentially that, in case of an error, no data will be written
        // to the String's underlying buffer, and the buffer should appear
        // as if unmodified. As such, we should be fine to operate on the
        // original buffer directly and not cause any UB or break any
        // invariants with the String's internal state.
        //
        // Since Strings do not have a `set_len` method like Vecs, in the
        // happy path we have to recombine the entire string from its
        // constituents, i.e. its pointer, length and capacity. This should
        // be fine as the pointer indeed came from the original String,
        // and that we do not attempt to copy the pointer anywhere and
        // potentially cause aliasing issues. As for the remaining factors,
        // we have to trust that the API will not cause length and capacity
        // to have nonsensical values, and that the underlying bytes are
        // indeed UTF-8.
        //
        // TODO: Use `String::into_raw_parts` to make this slightly simpler

        let cbuf = loop {
            // Save the old length of the String for later
            let len = buf.len();
            let mut cbuf = ffi::Buffer {
                ptr: buf.as_mut_ptr(),
                cap: buf.capacity(),
                len,
            };

            let result = unsafe {
                ffi::ghostty_render_state_row_cells_get(
                    self.iter.0.as_raw(),
                    ffi::RenderStateRowCellsData::GRAPHEMES_UTF8,
                    std::ptr::from_mut(&mut cbuf).cast(),
                )
            };
            match result {
                ffi::Result::SUCCESS => break Ok(cbuf),
                ffi::Result::OUT_OF_MEMORY => break Err(Error::OutOfMemory),
                ffi::Result::OUT_OF_SPACE => {
                    // When OutOfSpace is returned, the new length is written
                    // to `cbuf.len`, so we reserve additional space for that
                    buf.reserve(cbuf.len - len);
                }
                _ => {
                    break Err(Error::InvalidValue);
                }
            }
        }?;

        // Reconstitute the original String
        // WITHOUT DROPPING THE EXISTING STRING OBJECT (!!)
        // Otherwise, memory corruption, double frees, etc. WILL happen.
        unsafe {
            std::ptr::write(buf, String::from_raw_parts(cbuf.ptr, cbuf.len, cbuf.cap));
        }
        Ok(())
    }

    /// Whether the cell is contained within the current selection.
    ///
    /// This returns true when the cell's column is within the current row's
    /// row-local selection range, and false otherwise. Rendering policy for
    /// selected cells (colors, inversion, etc.) is left to the caller.
    ///
    /// Renderers that can draw cells in spans may be more efficient calling
    /// [`RowIteration::selection`] once per row and applying that range
    /// directly, avoiding one C API call per cell for selection state.
    pub fn is_selected(&self) -> Result<bool> {
        self.get(ffi::RenderStateRowCellsData::SELECTED)
    }

    /// Whether the cell has any explicit styling.
    ///
    /// This is equivalent to querying the raw cell's [`Cell::has_styling`]
    /// value, but avoids materializing the raw [`Cell`] for renderers that
    /// only need to know whether fetching the full style is necessary.
    pub fn has_styling(&self) -> Result<bool> {
        self.get(ffi::RenderStateRowCellsData::HAS_STYLING)
    }

    /// Copy the columns `[x, x + len)` of this row, clamped to the row
    /// width, into `out` with one C call.
    ///
    /// Styles and multi-codepoint graphemes arrive as indexes into tables
    /// in `out`, so a renderer resolves each distinct style once per row
    /// instead of once per cell. The buffers in `out` grow as needed and
    /// are reused by the next copy. The iteration position is unchanged.
    pub fn copy_into(&self, x: u16, len: u16, out: &mut CellsCopy) -> Result<()> {
        self.copy_with_flags(x, len, 0, out)
    }

    /// Like [`Self::copy_into`], but stop after the last cell of the range
    /// that is not a default cell.
    ///
    /// A default cell is an erased or never written column: no text, the
    /// default style, narrow, without hyperlink or protection, and output
    /// semantic content. [`CellsCopy::cells`] can then hold fewer cells
    /// than the range, and every column after them is a default cell the
    /// caller fills in itself.
    pub fn copy_trimmed_into(&self, x: u16, len: u16, out: &mut CellsCopy) -> Result<()> {
        self.copy_with_flags(x, len, ffi::RENDER_STATE_ROW_CELLS_COPY_TRIM, out)
    }

    fn copy_with_flags(&self, x: u16, len: u16, flags: u32, out: &mut CellsCopy) -> Result<()> {
        out.cells.clear();
        out.styles.clear();
        out.graphemes.clear();
        out.grapheme_bytes.clear();
        out.cells.reserve(usize::from(len));
        out.styles.reserve(usize::from(len) + 1);
        for _ in 0..4 {
            let mut raw = ffi::sized!(ffi::RenderStateRowCellsCopy);
            raw.cells = out.cells.as_mut_ptr().cast();
            raw.cells_cap = out.cells.capacity();
            raw.styles = out.styles.as_mut_ptr();
            raw.styles_cap = out.styles.capacity();
            raw.graphemes = out.graphemes.as_mut_ptr();
            raw.graphemes_cap = out.graphemes.capacity();
            raw.grapheme_bytes = out.grapheme_bytes.as_mut_ptr();
            raw.grapheme_bytes_cap = out.grapheme_bytes.capacity();
            raw.flags = flags;
            let result = unsafe {
                ffi::ghostty_render_state_row_cells_copy(self.iter.0.as_raw(), x, len, &raw mut raw)
            };
            match result {
                ffi::Result::SUCCESS => {
                    if raw.cells_len > out.cells.capacity()
                        || raw.styles_len > out.styles.capacity()
                        || raw.graphemes_len > out.graphemes.capacity()
                        || raw.grapheme_bytes_len > out.grapheme_bytes.capacity()
                    {
                        return Err(Error::InvalidValue);
                    }
                    // SAFETY: On success the C API initialized the first
                    // `*_len` entries of each buffer, all within capacity.
                    unsafe {
                        out.cells.set_len(raw.cells_len);
                        out.styles.set_len(raw.styles_len);
                        out.graphemes.set_len(raw.graphemes_len);
                        out.grapheme_bytes.set_len(raw.grapheme_bytes_len);
                    }
                    return Ok(());
                }
                ffi::Result::OUT_OF_SPACE => {
                    out.cells.reserve(raw.cells_len);
                    out.styles.reserve(raw.styles_len);
                    out.graphemes.reserve(raw.graphemes_len);
                    out.grapheme_bytes.reserve(raw.grapheme_bytes_len);
                }
                code => return from_result(code),
            }
        }
        Err(Error::OutOfSpace { required: 0 })
    }
}

/// Packed cells copied from one render-state row by
/// [`CellIteration::copy_into`], with the row's styles and multi-codepoint
/// graphemes in tables the cells index.
#[derive(Default)]
pub struct CellsCopy {
    cells: Vec<CopiedCell>,
    styles: Vec<ffi::Style>,
    graphemes: Vec<ffi::RenderStateGrapheme>,
    grapheme_bytes: Vec<u8>,
}

impl std::fmt::Debug for CellsCopy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CellsCopy")
            .field("cells", &self.cells)
            .field("styles", &self.styles.len())
            .field("graphemes", &self.graphemes)
            .field("grapheme_bytes", &self.grapheme_bytes)
            .finish()
    }
}

impl CellsCopy {
    /// Create an empty copy buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The copied cells, one per column.
    #[must_use]
    pub fn cells(&self) -> &[CopiedCell] {
        &self.cells
    }

    /// The number of entries in the style table. Index 0 is the default
    /// style.
    #[must_use]
    pub fn style_count(&self) -> usize {
        self.styles.len()
    }

    /// The style table entry a cell's [`CopiedCell::style_index`] names.
    pub fn style(&self, index: usize) -> Result<Style> {
        let raw = self.styles.get(index).ok_or(Error::InvalidValue)?;
        Style::try_from(*raw)
    }

    /// The UTF-8 grapheme cluster of a cell with more than one codepoint,
    /// base codepoint first, or `None` for a cell with at most one.
    pub fn grapheme(&self, cell: CopiedCell) -> Result<Option<&str>> {
        let Some(index) = usize::from(cell.0.grapheme).checked_sub(1) else {
            return Ok(None);
        };
        let span = self.graphemes.get(index).ok_or(Error::InvalidValue)?;
        let start = usize::try_from(span.offset).map_err(|_| Error::InvalidValue)?;
        let len = usize::try_from(span.len).map_err(|_| Error::InvalidValue)?;
        let bytes = start
            .checked_add(len)
            .and_then(|end| self.grapheme_bytes.get(start..end))
            .ok_or(Error::InvalidValue)?;
        std::str::from_utf8(bytes)
            .map(Some)
            .map_err(|_| Error::InvalidValue)
    }
}

/// One cell of a [`CellsCopy`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct CopiedCell(ffi::RenderStateCell);

impl CopiedCell {
    /// What kind of content the cell holds.
    pub fn content_tag(self) -> Result<CellContentTag> {
        CellContentTag::try_from(i32::from(self.0.content_tag)).map_err(|_| Error::InvalidValue)
    }

    /// The base codepoint of a cell with text, or `None` for a cell
    /// without text.
    #[must_use]
    pub fn codepoint(self) -> Option<char> {
        let tag = i32::from(self.0.content_tag);
        let text =
            tag == ffi::CellContentTag::CODEPOINT || tag == ffi::CellContentTag::CODEPOINT_GRAPHEME;
        if text && self.0.content != 0 {
            char::from_u32(self.0.content)
        } else {
            None
        }
    }

    /// The background palette index of a cell whose content tag is
    /// [`CellContentTag::BgColorPalette`].
    #[must_use]
    pub fn bg_color_palette(self) -> Option<PaletteIndex> {
        let [_, _, _, index] = self.0.content.to_be_bytes();
        (i32::from(self.0.content_tag) == ffi::CellContentTag::BG_COLOR_PALETTE)
            .then_some(PaletteIndex(index))
    }

    /// The background color of a cell whose content tag is
    /// [`CellContentTag::BgColorRgb`].
    #[must_use]
    pub fn bg_color_rgb(self) -> Option<RgbColor> {
        let [_, r, g, b] = self.0.content.to_be_bytes();
        (i32::from(self.0.content_tag) == ffi::CellContentTag::BG_COLOR_RGB).then_some(RgbColor {
            r,
            g,
            b,
        })
    }

    /// The width of the cell.
    pub fn wide(self) -> Result<CellWide> {
        CellWide::try_from(i32::from(self.0.wide)).map_err(|_| Error::InvalidValue)
    }

    /// Whether the cell has a hyperlink.
    #[must_use]
    pub fn has_hyperlink(self) -> bool {
        u32::from(self.0.flags) & ffi::RENDER_STATE_CELL_HYPERLINK != 0
    }

    /// Whether the cell is protected.
    #[must_use]
    pub fn is_protected(self) -> bool {
        u32::from(self.0.flags) & ffi::RENDER_STATE_CELL_PROTECTED != 0
    }

    /// The semantic content of the cell.
    pub fn semantic_content(self) -> Result<CellSemanticContent> {
        CellSemanticContent::try_from(i32::from(self.0.semantic_content))
            .map_err(|_| Error::InvalidValue)
    }

    /// The cell's index into [`CellsCopy::style`]; 0 is the default style.
    #[must_use]
    pub fn style_index(self) -> usize {
        usize::from(self.0.style)
    }

    /// Whether the cell's text is a multi-codepoint grapheme cluster read
    /// through [`CellsCopy::grapheme`].
    #[must_use]
    pub fn has_grapheme(self) -> bool {
        self.0.grapheme != 0
    }
}

//---------------------------
// Auxiliary types
//---------------------------

/// A part of the viewport: a range of rows and the leftmost columns of each.
///
/// Requested with [`RenderState::set_clip`] and reported by
/// [`Snapshot::clip`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clip {
    /// The first viewport row.
    pub y: u16,
    /// The number of rows starting at `y`. In a request, zero means every
    /// row from `y` to the bottom of the viewport.
    pub rows: u16,
    /// The number of columns from the left edge. In a request, zero means
    /// every column.
    pub cols: u16,
}

/// Cursor viewport position information.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorViewport {
    /// Cursor viewport x position in cells.
    pub x: u16,
    /// Cursor viewport y position in cells.
    pub y: u16,
    /// Whether the cursor is on the tail of a wide character.
    pub at_wide_tail: bool,
}

/// Render-state color information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colors {
    /// The default/current background color for the render state.
    pub background: RgbColor,
    /// The default/current foreground color for the render state.
    pub foreground: RgbColor,
    /// The cursor color which may be explicitly set by terminal state.
    pub cursor: Option<RgbColor>,
    /// The active 256-color palette for this render state.
    pub palette: [RgbColor; 256],
}

/// Dirty state of a render state after update.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, int_enum::IntEnum)]
pub enum Dirty {
    /// Not dirty at all; rendering can be skipped.
    Clean = ffi::RenderStateDirty::FALSE,
    /// Some rows changed; renderer can redraw incrementally.
    Partial = ffi::RenderStateDirty::PARTIAL,
    /// Global state changed; renderer should redraw everything.
    Full = ffi::RenderStateDirty::FULL,
}

/// Visual style of the cursor.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, int_enum::IntEnum)]
#[non_exhaustive]
pub enum CursorVisualStyle {
    /// Bar cursor (DECSCUSR 5, 6).
    Bar = ffi::RenderStateCursorVisualStyle::BAR,
    /// Block cursor (DECSCUSR 1, 2).
    Block = ffi::RenderStateCursorVisualStyle::BLOCK,
    /// Underline cursor (DECSCUSR 3, 4).
    Underline = ffi::RenderStateCursorVisualStyle::UNDERLINE,
    /// Hollow block cursor.
    BlockHollow = ffi::RenderStateCursorVisualStyle::BLOCK_HOLLOW,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::Terminal;

    /// Guards the `set_dirty` → `update` → `dirty()` round-trip. If
    /// `Snapshot::set(value: &T)` calls `from_ref(&value)`, the result has
    /// type `*const &T` (a pointer to the local reference), not `*const T`.
    /// C reads stack-address bytes into the dirty field, the next `update`
    /// propagates them, and `dirty()` fails enum decoding.
    #[test]
    fn copy_into_matches_the_per_cell_getters() {
        let mut terminal = Terminal::new(12, 4).unwrap();
        terminal.vt_write(
            "\x1b[1;31mab\x1b[0mc\x1b[1;31md\x1b[0me\u{301}\u{4E2D}\x1b]8;;http://x\x1b\\L\x1b]8;;\x1b\\\x1b[4;38;2;1;2;3mu\x1b[0m\r\n\x1b[48;5;4m\x1b[K\x1b[0m\r\n\x1b[48;2;7;8;9m\x1b[K\x1b[0mz"
                .as_bytes(),
        );
        let mut state = RenderState::new().unwrap();
        let snapshot = state.update(&terminal).unwrap();
        let mut rows = RowIterator::new().unwrap();
        let mut cells = CellIterator::new().unwrap();
        let mut copy = CellsCopy::new();
        let mut rows = rows.update(&snapshot).unwrap();
        let mut text = String::new();
        let mut row_count = 0;
        while let Some(row) = rows.next() {
            let mut iteration = cells.update(row).unwrap();
            iteration.copy_into(0, u16::MAX, &mut copy).unwrap();
            assert_eq!(copy.cells().len(), 12);
            if row_count == 0 {
                assert_eq!(copy.style_count(), 3);
                assert!(copy.cells()[7].has_hyperlink());
                assert_eq!(copy.grapheme(copy.cells()[4]).unwrap(), Some("e\u{301}"));
            }
            for (x, copied) in copy.cells().iter().copied().enumerate() {
                iteration.select(u16::try_from(x).unwrap()).unwrap();
                let raw = iteration.raw_cell().unwrap();
                assert_eq!(copied.content_tag().unwrap(), raw.content_tag().unwrap());
                assert_eq!(copied.wide().unwrap(), raw.wide().unwrap());
                assert_eq!(copied.has_hyperlink(), raw.has_hyperlink().unwrap());
                assert_eq!(
                    copied.semantic_content().unwrap(),
                    raw.semantic_content().unwrap()
                );
                assert_eq!(
                    copy.style(copied.style_index()).unwrap(),
                    iteration.style().unwrap()
                );
                text.clear();
                iteration.graphemes_utf8(&mut text).unwrap();
                let copied_text = match copy.grapheme(copied).unwrap() {
                    Some(cluster) => cluster.to_owned(),
                    None => copied.codepoint().map(String::from).unwrap_or_default(),
                };
                assert_eq!(copied_text, text);
                match raw.content_tag().unwrap() {
                    CellContentTag::BgColorPalette => {
                        assert_eq!(
                            copied.bg_color_palette(),
                            Some(raw.bg_color_palette().unwrap())
                        );
                    }
                    CellContentTag::BgColorRgb => {
                        assert_eq!(copied.bg_color_rgb(), Some(raw.bg_color_rgb().unwrap()));
                    }
                    _ => assert_eq!(copied.bg_color_palette(), None),
                }
            }
            row_count += 1;
        }
        assert_eq!(row_count, 4);
    }

    #[test]
    fn copy_trimmed_into_stops_at_the_last_written_cell() {
        let mut terminal = Terminal::new(20, 3).unwrap();
        terminal.vt_write("ab\x1b[5Gc\r\n\x1b[48;5;4m\x1b[K\x1b[0m".as_bytes());
        let mut state = RenderState::new().unwrap();
        let snapshot = state.update(&terminal).unwrap();
        let mut rows = RowIterator::new().unwrap();
        let mut cells = CellIterator::new().unwrap();
        let mut full = CellsCopy::new();
        let mut trimmed = CellsCopy::new();
        let mut rows = rows.update(&snapshot).unwrap();
        let mut lengths = Vec::new();
        while let Some(row) = rows.next() {
            let iteration = cells.update(row).unwrap();
            iteration.copy_into(0, 20, &mut full).unwrap();
            iteration.copy_trimmed_into(0, 20, &mut trimmed).unwrap();
            let used = trimmed.cells().len();
            let fields = |cell: &CopiedCell| {
                let raw = cell.0;
                (
                    raw.content,
                    raw.style,
                    raw.grapheme,
                    raw.content_tag,
                    raw.wide,
                    raw.flags,
                    raw.semantic_content,
                )
            };
            for (a, b) in full.cells()[..used].iter().zip(trimmed.cells()) {
                assert_eq!(fields(a), fields(b));
            }
            for cell in &full.cells()[used..] {
                assert_eq!(cell.codepoint(), None);
                assert_eq!(cell.style_index(), 0);
                assert_eq!(cell.content_tag().unwrap(), CellContentTag::Codepoint);
                assert!(!cell.has_hyperlink());
            }
            lengths.push(used);
        }
        assert_eq!(lengths, [5, 20, 0]);
    }

    #[test]
    fn clip_limits_the_captured_rows_and_columns() {
        let mut terminal = Terminal::new(10, 6).unwrap();
        terminal.vt_write(b"r0\r\nr1\r\nr2\r\nr3\r\nr4\r\nr5");
        let mut state = RenderState::new().unwrap();
        assert_eq!(
            state.update(&terminal).unwrap().clip().unwrap(),
            Clip {
                y: 0,
                rows: 6,
                cols: 10
            }
        );
        let clip = Clip {
            y: 2,
            rows: 3,
            cols: 4,
        };
        state.set_clip(clip).unwrap();
        let snapshot = state.update(&terminal).unwrap();
        assert_eq!(snapshot.clip().unwrap(), clip);
        assert_eq!(snapshot.rows().unwrap(), 6);
        assert_eq!(snapshot.cols().unwrap(), 10);
        assert_eq!(snapshot.cursor_viewport().unwrap(), None);
        let mut rows = RowIterator::new().unwrap();
        let mut cells = CellIterator::new().unwrap();
        let mut copy = CellsCopy::new();
        let mut rows = rows.update(&snapshot).unwrap();
        let mut seen = Vec::new();
        while let Some(row) = rows.next() {
            cells
                .update(row)
                .unwrap()
                .copy_into(0, 10, &mut copy)
                .unwrap();
            assert_eq!(copy.cells().len(), 4);
            seen.push(copy.cells()[1].codepoint().unwrap());
        }
        assert_eq!(seen, ['2', '3', '4']);
    }

    #[test]
    fn dirty_decodes_after_set_dirty_then_update() {
        let terminal = Terminal::new(8, 3).unwrap();
        let mut state = RenderState::new().unwrap();

        state
            .update(&terminal)
            .unwrap()
            .set_dirty(Dirty::Clean)
            .unwrap();

        assert!(state.update(&terminal).unwrap().dirty().is_ok());
    }
}
