#![allow(unsafe_code)]

use std::{ffi::CStr, path::Path, ptr};

use gtk::glib;

pub(super) struct SourceBoundary(u32);

impl SourceBoundary {
    pub(super) fn capture() -> Self {
        let marker = glib::idle_add_local_once(|| {});
        let id = marker.as_raw();
        marker.remove();
        Self(id)
    }

    pub(super) fn detach_cef_work_source(self) -> Result<(), String> {
        let end = Self::capture().0;
        let mut matches = Vec::new();
        for id in self.0.saturating_add(1)..end {
            unsafe {
                let source = glib::ffi::g_main_context_find_source_by_id(ptr::null_mut(), id);
                if is_cef_work_source(source) {
                    matches.push(source);
                }
            }
        }
        match matches.as_slice() {
            [source] => {
                unsafe { glib::ffi::g_source_destroy(*source) };
                Ok(())
            }
            _ => Err(format!(
                "CEF GLib integration expected one browser work source, found {}",
                matches.len()
            )),
        }
    }
}

unsafe fn is_cef_work_source(source: *mut glib::ffi::GSource) -> bool {
    unsafe {
        if source.is_null()
            || glib::ffi::g_source_get_priority(source) != glib::ffi::G_PRIORITY_DEFAULT_IDLE
            || glib::ffi::g_source_get_can_recurse(source) == 0
            || !glib::ffi::g_source_get_name(source).is_null()
            || (*source).source_funcs.is_null()
        {
            return false;
        }
        let functions = &*(*source).source_funcs;
        [
            functions.prepare.map(|function| function as *const ()),
            functions.check.map(|function| function as *const ()),
            functions.dispatch.map(|function| function as *const ()),
            functions.finalize.map(|function| function as *const ()),
        ]
        .into_iter()
        .all(|function| function.is_some_and(is_cef_callback))
    }
}

fn is_cef_callback(function: *const ()) -> bool {
    unsafe {
        let mut info = std::mem::MaybeUninit::<libc::Dl_info>::uninit();
        if libc::dladdr(function.cast(), info.as_mut_ptr()) == 0 {
            return false;
        }
        let info = info.assume_init();
        if info.dli_fname.is_null() {
            return false;
        }
        CStr::from_ptr(info.dli_fname)
            .to_str()
            .ok()
            .and_then(|path| Path::new(path).file_name())
            .is_some_and(|file| file == "libcef.so")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_non_cef_sources_attached() {
        let boundary = SourceBoundary::capture();
        let source = glib::idle_add_local(|| glib::ControlFlow::Continue);
        assert!(boundary.detach_cef_work_source().is_err());
        assert!(
            glib::MainContext::default()
                .find_source_by_id(&source)
                .is_some()
        );
        source.remove();
        assert!(!is_cef_callback(is_cef_callback as *const ()));
    }
}
