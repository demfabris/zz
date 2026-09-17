

use std::{marker::PhantomData, mem::MaybeUninit};

use crate::{
    alloc::{Allocator, Object},
    error::{Result, from_result},
    ffi,
};

#[derive(Debug)]
pub struct Parser<'alloc>(Object<'alloc, ffi::OscParserImpl>);

impl<'alloc> Parser<'alloc> {

    pub fn new() -> Result<Self> {

        unsafe { Self::new_inner(std::ptr::null()) }
    }

    pub fn new_with_alloc<'ctx: 'alloc>(alloc: &'alloc Allocator<'ctx>) -> Result<Self> {

        unsafe { Self::new_inner(alloc.to_raw()) }
    }

    unsafe fn new_inner(alloc: *const ffi::Allocator) -> Result<Self> {
        let mut raw: ffi::OscParser = std::ptr::null_mut();
        let result = unsafe { ffi::ghostty_osc_new(alloc, &raw mut raw) };
        from_result(result)?;
        Ok(Self(Object::new(raw)?))
    }

    pub fn reset(&mut self) {
        unsafe { ffi::ghostty_osc_reset(self.0.as_raw()) }
    }

    pub fn next_byte(&mut self, byte: u8) {
        unsafe { ffi::ghostty_osc_next(self.0.as_raw(), byte) }
    }

    #[expect(clippy::missing_panics_doc, reason = "internal invariant")]
    pub fn end<'p>(&'p mut self, terminator: u8) -> Command<'p, 'alloc> {
        let raw = unsafe { ffi::ghostty_osc_end(self.0.as_raw(), terminator) };
        Command {
            inner: Object::new(raw).expect("command must not be null"),
            _parser: PhantomData,
        }
    }
}

impl Drop for Parser<'_> {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_osc_free(self.0.as_raw()) }
    }
}

#[derive(Debug)]
pub struct Command<'p, 'alloc> {
    inner: Object<'alloc, ffi::OscCommandImpl>,
    _parser: PhantomData<&'p Parser<'alloc>>,
}

impl<'p> Command<'p, '_> {

    #[must_use]
    pub fn command_type(self) -> CommandType<'p> {
        self.command_type_inner().unwrap_or(CommandType::Invalid)
    }

    fn command_type_inner(&self) -> Option<CommandType<'p>> {
        use ffi::OscCommandData as Data;
        use ffi::OscCommandType as Type;

        let raw_type = unsafe { ffi::ghostty_osc_command_type(self.inner.as_raw()) };
        Some(match raw_type {
            Type::CHANGE_WINDOW_TITLE => CommandType::ChangeWindowTitle {
                title: self.get(Data::CHANGE_WINDOW_TITLE_STR)?,
            },
            Type::CHANGE_WINDOW_ICON => CommandType::ChangeWindowIcon,
            Type::SEMANTIC_PROMPT => CommandType::SemanticPrompt,
            Type::CLIPBOARD_CONTENTS => CommandType::ClipboardContents,
            Type::REPORT_PWD => CommandType::ReportPwd,
            Type::MOUSE_SHAPE => CommandType::MouseShape,
            Type::COLOR_OPERATION => CommandType::ColorOperation,
            Type::KITTY_COLOR_PROTOCOL => CommandType::KittyColorProtocol,
            Type::SHOW_DESKTOP_NOTIFICATION => CommandType::ShowDesktopNotification,
            Type::HYPERLINK_START => CommandType::HyperlinkStart,
            Type::HYPERLINK_END => CommandType::HyperlinkEnd,
            Type::CONEMU_SLEEP => CommandType::ConemuSleep,
            Type::CONEMU_SHOW_MESSAGE_BOX => CommandType::ConemuShowMessageBox,
            Type::CONEMU_CHANGE_TAB_TITLE => CommandType::ConemuChangeTabTitle,
            Type::CONEMU_PROGRESS_REPORT => CommandType::ConemuProgressReport,
            Type::CONEMU_WAIT_INPUT => CommandType::ConemuWaitInput,
            Type::CONEMU_GUIMACRO => CommandType::ConemuGuiMacro,
            Type::CONEMU_RUN_PROCESS => CommandType::ConemuRunProcess,
            Type::CONEMU_OUTPUT_ENVIRONMENT_VARIABLE => {
                CommandType::ConemuOutputEnvironmentVariable
            }
            Type::CONEMU_XTERM_EMULATION => CommandType::ConemuXtermEmulation,
            Type::CONEMU_COMMENT => CommandType::ConemuComment,
            Type::KITTY_TEXT_SIZING => CommandType::KittyTextSizing,

            _ => return None,
        })
    }

    fn get<T>(&self, tag: ffi::OscCommandData::Type) -> Option<T> {
        let mut value = MaybeUninit::<T>::zeroed();
        let result = unsafe {
            ffi::ghostty_osc_command_data(self.inner.as_raw(), tag, value.as_mut_ptr().cast())
        };

        if result {

            Some(unsafe { value.assume_init() })
        } else {
            None
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Default)]
#[expect(missing_docs, reason = "missing upstream docs")]
pub enum CommandType<'p> {
    #[default]
    Invalid,
    ChangeWindowTitle {

        title: &'p str,
    },
    ChangeWindowIcon,
    SemanticPrompt,
    ClipboardContents,
    ReportPwd,
    MouseShape,
    ColorOperation,
    KittyColorProtocol,
    ShowDesktopNotification,
    HyperlinkStart,
    HyperlinkEnd,
    ConemuSleep,
    ConemuShowMessageBox,
    ConemuChangeTabTitle,
    ConemuProgressReport,
    ConemuWaitInput,
    ConemuGuiMacro,
    ConemuRunProcess,
    ConemuOutputEnvironmentVariable,
    ConemuXtermEmulation,
    ConemuComment,
    KittyTextSizing,
}
