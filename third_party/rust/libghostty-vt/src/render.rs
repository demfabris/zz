

use std::{convert::Into, marker::PhantomData, mem::MaybeUninit};

use crate::{
    alloc::{Allocator, Object},
    error::{Error, Result, from_optional_result, from_result},
    ffi,
    screen::{Cell, Row},
    style::{RgbColor, Style},
    terminal::Terminal,
};

pub use ffi::RenderStateRowSelection as RowSelection;

#[derive(Debug)]
pub struct RenderState<'alloc>(Object<'alloc, ffi::RenderStateImpl>);

#[derive(Debug)]
pub struct Snapshot<'alloc, 's>(&'s mut RenderState<'alloc>);

#[derive(Debug)]
pub struct Update<'alloc, 's> {
    state: Option<&'s mut RenderState<'alloc>>,
}

#[derive(Debug)]
pub struct RowIterator<'alloc>(Object<'alloc, ffi::RenderStateRowIteratorImpl>);

#[derive(Debug)]
pub struct RowIteration<'alloc, 's> {
    iter: &'s mut RowIterator<'alloc>,

    _phan: PhantomData<&'s Snapshot<'alloc, 's>>,
}

#[derive(Debug)]
pub struct CellIterator<'alloc>(Object<'alloc, ffi::RenderStateRowCellsImpl>);

#[derive(Debug)]
pub struct CellIteration<'alloc, 's> {
    iter: &'s mut CellIterator<'alloc>,
    _phan: PhantomData<&'s RowIteration<'alloc, 's>>,
}

impl<'alloc> RenderState<'alloc> {

    pub fn new() -> Result<Self> {

        unsafe { Self::new_inner(std::ptr::null()) }
    }

    pub fn new_with_alloc<'ctx: 'alloc>(alloc: &'alloc Allocator<'ctx>) -> Result<Self> {

        unsafe { Self::new_inner(alloc.to_raw()) }
    }

    unsafe fn new_inner(alloc: *const ffi::Allocator) -> Result<Self> {
        let mut raw: ffi::RenderState = std::ptr::null_mut();
        let result = unsafe { ffi::ghostty_render_state_new(alloc, &raw mut raw) };
        from_result(result)?;
        Ok(Self(Object::new(raw)?))
    }

    pub fn update<'cb>(
        &mut self,
        terminal: &Terminal<'alloc, 'cb>,
    ) -> Result<Snapshot<'alloc, '_>> {
        let result =
            unsafe { ffi::ghostty_render_state_update(self.0.as_raw(), terminal.inner.as_raw()) };
        from_result(result)?;
        Ok(Snapshot(self))
    }

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
}

impl Drop for RenderState<'_> {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_render_state_free(self.0.as_raw()) }
    }
}

impl<'alloc, 's> Update<'alloc, 's> {

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

        from_result(result)?;

        Ok(unsafe { value.assume_init() })
    }

    fn set<T>(&self, tag: ffi::RenderStateOption::Type, value: &T) -> Result<()> {
        let result = unsafe {
            ffi::ghostty_render_state_set(self.0.0.as_raw(), tag, std::ptr::from_ref(value).cast())
        };

        from_result(result)
    }

    pub fn dirty(&self) -> Result<Dirty> {
        self.get::<ffi::RenderStateDirty::Type>(ffi::RenderStateData::DIRTY)
            .and_then(|v| v.try_into().map_err(|_| Error::InvalidValue))
    }

    pub fn cols(&self) -> Result<u16> {
        self.get(ffi::RenderStateData::COLS)
    }

    pub fn rows(&self) -> Result<u16> {
        self.get(ffi::RenderStateData::ROWS)
    }

    pub fn cursor_color(&self) -> Result<Option<RgbColor>> {
        let has_value = self.get(ffi::RenderStateData::COLOR_CURSOR_HAS_VALUE)?;
        if has_value {
            let color = self.get(ffi::RenderStateData::COLOR_CURSOR)?;
            Ok(Some(color))
        } else {
            Ok(None)
        }
    }

    pub fn cursor_visible(&self) -> Result<bool> {
        self.get(ffi::RenderStateData::CURSOR_VISIBLE)
    }

    pub fn cursor_blinking(&self) -> Result<bool> {
        self.get(ffi::RenderStateData::CURSOR_BLINKING)
    }

    pub fn cursor_password_input(&self) -> Result<bool> {
        self.get(ffi::RenderStateData::CURSOR_PASSWORD_INPUT)
    }

    pub fn cursor_visual_style(&self) -> Result<CursorVisualStyle> {
        self.get::<ffi::RenderStateCursorVisualStyle::Type>(
            ffi::RenderStateData::CURSOR_VISUAL_STYLE,
        )
        .and_then(|v| v.try_into().map_err(|_| Error::InvalidValue))
    }

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

    pub fn colors(&self) -> Result<Colors> {
        let mut colors = ffi::sized!(ffi::RenderStateColors);
        let result =
            unsafe { ffi::ghostty_render_state_colors_get(self.0.0.as_raw(), &raw mut colors) };
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

    pub fn set_dirty(&self, dirty: Dirty) -> Result<()> {
        self.set(
            ffi::RenderStateOption::DIRTY,
            &(dirty as ffi::RenderStateDirty::Type),
        )
    }
}

impl<'alloc> RowIterator<'alloc> {

    pub fn new() -> Result<Self> {

        unsafe { Self::new_inner(std::ptr::null()) }
    }

    pub fn new_with_alloc<'ctx: 'alloc>(alloc: &'alloc Allocator<'ctx>) -> Result<Self> {

        unsafe { Self::new_inner(alloc.to_raw()) }
    }

    unsafe fn new_inner(alloc: *const ffi::Allocator) -> Result<Self> {
        let mut raw: ffi::RenderStateRowIterator = std::ptr::null_mut();
        let result = unsafe { ffi::ghostty_render_state_row_iterator_new(alloc, &raw mut raw) };
        from_result(result)?;
        Ok(Self(Object::new(raw)?))
    }

    pub fn update(
        &mut self,
        snapshot: &'_ Snapshot<'alloc, '_>,
    ) -> Result<RowIteration<'alloc, '_>> {
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

        from_result(result)?;

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

    pub fn dirty(&self) -> Result<bool> {
        self.get(ffi::RenderStateRowData::DIRTY)
    }

    pub fn raw_row(&self) -> Result<Row> {
        self.get(ffi::RenderStateRowData::RAW).map(Row)
    }

    pub fn set_dirty(&self, dirty: bool) -> Result<()> {
        self.set(ffi::RenderStateRowOption::DIRTY, &dirty)
    }

    pub fn selection(&self) -> Result<Option<RowSelection>> {
        let mut value = ffi::sized!(RowSelection);
        let result = unsafe {
            ffi::ghostty_render_state_row_get(
                self.iter.0.as_raw(),
                ffi::RenderStateRowData::SELECTION,
                std::ptr::from_mut(&mut value).cast(),
            )
        };

        from_optional_result(result, value)
    }
}

impl<'alloc> CellIterator<'alloc> {

    pub fn new() -> Result<Self> {

        unsafe { Self::new_inner(std::ptr::null()) }
    }

    pub fn new_with_alloc<'ctx: 'alloc>(alloc: &'alloc Allocator<'ctx>) -> Result<Self> {

        unsafe { Self::new_inner(alloc.to_raw()) }
    }

    unsafe fn new_inner(alloc: *const ffi::Allocator) -> Result<Self> {
        let mut raw: ffi::RenderStateRowCells = std::ptr::null_mut();
        let result = unsafe { ffi::ghostty_render_state_row_cells_new(alloc, &raw mut raw) };
        from_result(result)?;
        Ok(Self(Object::new(raw)?))
    }

    pub fn update(
        &mut self,
        row: &'_ RowIteration<'alloc, '_>,
    ) -> Result<CellIteration<'alloc, '_>> {
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

        Ok(unsafe { value.assume_init() })
    }

    pub fn raw_cell(&self) -> Result<Cell> {
        self.get(ffi::RenderStateRowCellsData::RAW).map(Cell)
    }

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

    pub fn fg_color(&self) -> Result<Option<RgbColor>> {
        let res = self.get::<ffi::ColorRgb>(ffi::RenderStateRowCellsData::FG_COLOR);
        match res {
            Ok(o) => Ok(Some(o.into())),
            Err(Error::InvalidValue) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn bg_color(&self) -> Result<Option<RgbColor>> {
        let res = self.get::<ffi::ColorRgb>(ffi::RenderStateRowCellsData::BG_COLOR);
        match res {
            Ok(o) => Ok(Some(o.into())),
            Err(Error::InvalidValue) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn graphemes(&self) -> Result<Vec<char>> {
        let len = self.graphemes_len()?;
        let mut graphemes = vec!['\0'; len];
        self.graphemes_buf(&mut graphemes)?;
        Ok(graphemes)
    }

    pub fn graphemes_len(&self) -> Result<usize> {
        self.get(ffi::RenderStateRowCellsData::GRAPHEMES_LEN)
    }

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

    pub fn graphemes_utf8(&self, buf: &mut String) -> Result<()> {

        let cbuf = loop {

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

                    buf.reserve(cbuf.len - len);
                    continue;
                }
                ffi::Result::NO_VALUE | ffi::Result::INVALID_VALUE | _ => {
                    break Err(Error::InvalidValue);
                }
            };
        }?;

        unsafe {
            std::ptr::write(buf, String::from_raw_parts(cbuf.ptr, cbuf.len, cbuf.cap));
        }
        Ok(())
    }

    pub fn is_selected(&self) -> Result<bool> {
        self.get(ffi::RenderStateRowCellsData::SELECTED)
    }

    pub fn has_styling(&self) -> Result<bool> {
        self.get(ffi::RenderStateRowCellsData::HAS_STYLING)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorViewport {

    pub x: u16,

    pub y: u16,

    pub at_wide_tail: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colors {

    pub background: RgbColor,

    pub foreground: RgbColor,

    pub cursor: Option<RgbColor>,

    pub palette: [RgbColor; 256],
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, int_enum::IntEnum)]
pub enum Dirty {

    Clean = ffi::RenderStateDirty::FALSE,

    Partial = ffi::RenderStateDirty::PARTIAL,

    Full = ffi::RenderStateDirty::FULL,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, int_enum::IntEnum)]
#[non_exhaustive]
pub enum CursorVisualStyle {

    Bar = ffi::RenderStateCursorVisualStyle::BAR,

    Block = ffi::RenderStateCursorVisualStyle::BLOCK,

    Underline = ffi::RenderStateCursorVisualStyle::UNDERLINE,

    BlockHollow = ffi::RenderStateCursorVisualStyle::BLOCK_HOLLOW,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{Options, Terminal};

    #[test]
    fn dirty_decodes_after_set_dirty_then_update() {
        let terminal = Terminal::new(Options {
            cols: 8,
            rows: 3,
            max_scrollback: 0,
        })
        .unwrap();
        let mut state = RenderState::new().unwrap();

        state
            .update(&terminal)
            .unwrap()
            .set_dirty(Dirty::Clean)
            .unwrap();

        assert!(state.update(&terminal).unwrap().dirty().is_ok());
    }
}
