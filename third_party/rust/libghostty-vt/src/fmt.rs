

use std::{marker::PhantomData, ptr::NonNull};

use crate::{
    alloc::{Allocator, Bytes, Object},
    error::{Error, Result, from_result},
    ffi,
    selection::Selection,
    terminal::Terminal,
};

#[derive(Debug)]
pub struct Formatter<'t, 'alloc: 'cb, 'cb: 't> {
    inner: Object<'alloc, ffi::FormatterImpl>,
    _terminal: PhantomData<&'t Terminal<'alloc, 'cb>>,
}

#[derive(Debug)]
pub struct FormatterOptions<'t, 's> {
    inner: ffi::FormatterTerminalOptions,
    _phan: PhantomData<&'s Selection<'t>>,
}
impl<'t, 's> FormatterOptions<'t, 's> {

    pub fn new() -> Self {
        Self {
            inner: ffi::FormatterTerminalOptions {
                extra: ffi::FormatterTerminalExtra {
                    screen: ffi::FormatterScreenExtra {
                        ..ffi::sized!(ffi::FormatterScreenExtra)
                    },
                    ..ffi::sized!(ffi::FormatterTerminalExtra)
                },
                ..ffi::sized!(ffi::FormatterTerminalOptions)
            },
            _phan: PhantomData,
        }
    }

    pub fn with_format(mut self, value: Format) -> Self {
        self.inner.emit = value.into();
        self
    }

    pub fn with_unwrap(mut self, value: bool) -> Self {
        self.inner.unwrap = value;
        self
    }

    pub fn with_trim(mut self, value: bool) -> Self {
        self.inner.trim = value;
        self
    }

    pub fn with_selection(mut self, value: &'s Selection<'t>) -> Self {
        self.inner.selection = &value.inner;
        self
    }

    pub fn with_palette(mut self, value: bool) -> Self {
        self.inner.extra.palette = value;
        self
    }

    pub fn with_modes(mut self, value: bool) -> Self {
        self.inner.extra.modes = value;
        self
    }

    pub fn with_scrolling_region(mut self, value: bool) -> Self {
        self.inner.extra.scrolling_region = value;
        self
    }

    pub fn with_tabstops(mut self, value: bool) -> Self {
        self.inner.extra.tabstops = value;
        self
    }

    pub fn with_pwd(mut self, value: bool) -> Self {
        self.inner.extra.pwd = value;
        self
    }

    pub fn with_keyboard(mut self, value: bool) -> Self {
        self.inner.extra.keyboard = value;
        self
    }

    pub fn with_cursor(mut self, value: bool) -> Self {
        self.inner.extra.screen.cursor = value;
        self
    }

    pub fn with_style(mut self, value: bool) -> Self {
        self.inner.extra.screen.style = value;
        self
    }

    pub fn with_hyperlink(mut self, value: bool) -> Self {
        self.inner.extra.screen.hyperlink = value;
        self
    }

    pub fn with_protection(mut self, value: bool) -> Self {
        self.inner.extra.screen.protection = value;
        self
    }

    pub fn with_kitty_keyboard(mut self, value: bool) -> Self {
        self.inner.extra.screen.kitty_keyboard = value;
        self
    }

    pub fn with_charsets(mut self, value: bool) -> Self {
        self.inner.extra.screen.charsets = value;
        self
    }
}

impl<'t, 'alloc: 'cb, 'cb: 't> Formatter<'t, 'alloc, 'cb> {

    pub fn new(
        terminal: &'t Terminal<'alloc, 'cb>,
        opts: FormatterOptions<'t, '_>,
    ) -> Result<Self> {

        unsafe { Self::new_inner(std::ptr::null(), terminal, opts) }
    }

    pub fn new_with_alloc<'ctx: 'alloc>(
        alloc: &'alloc Allocator<'ctx>,
        terminal: &'t Terminal<'alloc, 'cb>,
        opts: FormatterOptions,
    ) -> Result<Self> {

        unsafe { Self::new_inner(alloc.to_raw(), terminal, opts) }
    }

    unsafe fn new_inner(
        alloc: *const ffi::Allocator,
        terminal: &'t Terminal<'alloc, 'cb>,
        opts: FormatterOptions,
    ) -> Result<Self> {
        let mut raw: ffi::Formatter = std::ptr::null_mut();

        let result = unsafe {
            ffi::ghostty_formatter_terminal_new(
                alloc,
                &raw mut raw,
                terminal.inner.as_raw(),
                opts.inner,
            )
        };
        from_result(result)?;

        Ok(Self {
            inner: Object::new(raw)?,
            _terminal: PhantomData,
        })
    }

    pub fn format_alloc<'a, 'ctx: 'a>(
        &mut self,
        alloc: Option<&'a Allocator<'ctx>>,
    ) -> Result<Bytes<'a>> {
        let alloc = if let Some(alloc) = alloc {
            alloc.to_raw()
        } else {
            std::ptr::null()
        };

        let mut bytes = std::ptr::null_mut();
        let mut len = 0usize;
        let result = unsafe {
            ffi::ghostty_formatter_format_alloc(
                self.inner.as_raw(),
                alloc,
                std::ptr::from_mut(&mut bytes),
                std::ptr::from_mut(&mut len),
            )
        };
        from_result(result)?;

        let ptr = NonNull::new(bytes).ok_or(Error::OutOfMemory)?;
        Ok(unsafe { Bytes::from_raw_parts(ptr, len, alloc) })
    }

    pub fn format_buf(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut len = 0usize;
        let result = unsafe {
            ffi::ghostty_formatter_format_buf(
                self.inner.as_raw(),
                std::ptr::from_mut(buf).cast(),
                buf.len(),
                std::ptr::from_mut(&mut len),
            )
        };
        from_result(result)?;
        Ok(len)
    }

    pub fn format_len(&mut self) -> Result<usize> {
        let mut len = 0usize;
        let result = unsafe {
            ffi::ghostty_formatter_format_buf(
                self.inner.as_raw(),
                std::ptr::null_mut(),
                0,
                std::ptr::from_mut(&mut len),
            )
        };

        match from_result(result) {
            Err(Error::OutOfSpace { .. }) => Ok(len),
            Err(e) => Err(e),
            Ok(()) => Err(Error::InvalidValue),
        }
    }
}

impl Drop for Formatter<'_, '_, '_> {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_formatter_free(self.inner.as_raw()) }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, int_enum::IntEnum)]
pub enum Format {

    Plain = ffi::FormatterFormat::PLAIN,

    Vt = ffi::FormatterFormat::VT,

    Html = ffi::FormatterFormat::HTML,
}
