//! Grafts CEF's `CefAppProtocol` onto GPUI's live `NSApplication` class, whose
//! selectors Chromium sends to `NSApp` when closing the `DevTools` popup.

use std::{
    ffi::{CStr, CString},
    mem, ptr,
    sync::{
        Once,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use objc2::{
    encode::Encode,
    ffi::{
        class_addMethod, class_addProtocol, method_getImplementation, method_setImplementation,
        objc_getProtocol, object_getClass,
    },
    msg_send,
    runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
    sel,
};

static HANDLING_SEND_EVENT: AtomicBool = AtomicBool::new(false);
static ORIGINAL_SEND_EVENT: AtomicUsize = AtomicUsize::new(0);

type SendEvent = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);

const SEND_EVENT_TYPES: &CStr = c"v@:@";

/// Install the protocol once, after GPUI creates its application instance and
/// before CEF opens a native window. A failure degrades to the unpatched class.
pub(super) fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(install_once);
}

fn install_once() {
    let Some(class) = application_class() else {
        log::error!("could not resolve the NSApplication class for the CEF app protocol");
        return;
    };
    let Ok(is_handling_types) = CString::new(format!("{}@:", Bool::ENCODING)) else {
        return;
    };
    let Ok(set_handling_types) = CString::new(format!("v@:{}", Bool::ENCODING)) else {
        return;
    };

    add_method(
        class,
        sel!(isHandlingSendEvent),
        is_handling_send_event as *const (),
        &is_handling_types,
    );
    add_method(
        class,
        sel!(setHandlingSendEvent:),
        set_handling_send_event as *const (),
        &set_handling_types,
    );
    wrap_send_event(class);
    adopt_protocols(class);
}

fn application_class() -> Option<&'static AnyClass> {
    let class = AnyClass::get(c"NSApplication")?;
    // SAFETY: `+sharedApplication` returns the application singleton.
    let application: *mut AnyObject = unsafe { msg_send![class, sharedApplication] };
    if application.is_null() {
        return None;
    }
    // SAFETY: the singleton is live, and classes outlive the process.
    unsafe { object_getClass(application).as_ref() }
}

fn add_method(class: &AnyClass, selector: Sel, function: *const (), types: &CStr) {
    // SAFETY: `function` matches `types`, which describes the signature the
    // runtime will dispatch with.
    let added =
        unsafe { class_addMethod(class_ptr(class), selector, erase(function), types.as_ptr()) };
    if !added.as_bool() {
        log::error!("could not add {selector} to {class}");
    }
}

fn wrap_send_event(class: &AnyClass) {
    let selector = sel!(sendEvent:);
    let Some(method) = class.instance_method(selector) else {
        log::error!("could not find sendEvent: on {class}");
        return;
    };
    // SAFETY: `method` belongs to the class we just queried.
    let Some(original) = (unsafe { method_getImplementation(method) }) else {
        log::error!("could not read the sendEvent: implementation of {class}");
        return;
    };
    ORIGINAL_SEND_EVENT.store(original as usize, Ordering::Relaxed);

    let wrapper = erase(send_event as *const ());
    // SAFETY: the wrapper has `sendEvent:`'s signature.
    let added = unsafe {
        class_addMethod(
            class_ptr(class),
            selector,
            wrapper,
            SEND_EVENT_TYPES.as_ptr(),
        )
    };
    if !added.as_bool() {
        // SAFETY: same signature as the implementation being replaced.
        unsafe { method_setImplementation(method, wrapper) };
    }
}

fn adopt_protocols(class: &AnyClass) {
    for name in [c"CefAppProtocol", c"CrAppControlProtocol", c"CrAppProtocol"] {
        // SAFETY: `name` is a valid C string.
        let protocol = unsafe { objc_getProtocol(name.as_ptr()) };
        if protocol.is_null() {
            continue;
        }
        // SAFETY: both pointers came from the runtime.
        unsafe { class_addProtocol(class_ptr(class), protocol) };
    }
}

unsafe extern "C-unwind" fn is_handling_send_event(_: *mut AnyObject, _: Sel) -> Bool {
    Bool::new(HANDLING_SEND_EVENT.load(Ordering::Relaxed))
}

unsafe extern "C-unwind" fn set_handling_send_event(_: *mut AnyObject, _: Sel, handling: Bool) {
    HANDLING_SEND_EVENT.store(handling.as_bool(), Ordering::Relaxed);
}

unsafe extern "C-unwind" fn send_event(
    application: *mut AnyObject,
    selector: Sel,
    event: *mut AnyObject,
) {
    // Restore rather than clear: AppKit dispatch nests.
    let outer = HANDLING_SEND_EVENT.swap(true, Ordering::Relaxed);
    let original = ORIGINAL_SEND_EVENT.load(Ordering::Relaxed);
    if original != 0 {
        // SAFETY: `original` is the implementation `sendEvent:` resolved to
        // before `wrap_send_event` replaced it, so it takes these arguments.
        let original: SendEvent = unsafe { mem::transmute(original) };
        // SAFETY: forwarding the arguments AppKit passed us.
        unsafe { original(application, selector, event) };
    }
    HANDLING_SEND_EVENT.store(outer, Ordering::Relaxed);
}

fn class_ptr(class: &AnyClass) -> *mut AnyClass {
    ptr::from_ref(class).cast_mut()
}

fn erase(function: *const ()) -> Imp {
    // SAFETY: `Imp` is a function pointer of the same width.
    unsafe { mem::transmute(function) }
}
