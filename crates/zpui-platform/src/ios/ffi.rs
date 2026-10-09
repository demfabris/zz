use objc::runtime::Object;

pub type id = *mut Object;
pub const nil: id = std::ptr::null_mut();

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UIEdgeInsets {
    pub top: f64,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NSRange {
    pub location: usize,
    pub length: usize,
}

unsafe impl objc::Encode for NSRange {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{_NSRange=QQ}") }
    }
}

unsafe impl objc::Encode for UIEdgeInsets {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{UIEdgeInsets=dddd}") }
    }
}

unsafe impl objc::Encode for CGPoint {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGPoint=dd}") }
    }
}

unsafe impl objc::Encode for CGSize {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGSize=dd}") }
    }
}

unsafe impl objc::Encode for CGRect {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}

pub unsafe fn ns_string(string: &str) -> id {
    use objc::{class, msg_send, sel, sel_impl};
    let c = std::ffi::CString::new(string).unwrap();
    let ns: id = msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
    ns
}

pub unsafe fn ns_array(objects: &[id]) -> id {
    use objc::{class, msg_send, sel, sel_impl};
    msg_send![class!(NSArray), arrayWithObjects: objects.as_ptr() count: objects.len()]
}

pub unsafe fn nsstring_to_string(string: id) -> Option<String> {
    use objc::{msg_send, sel, sel_impl};
    if string.is_null() {
        return None;
    }
    let bytes: *const std::ffi::c_char = msg_send![string, UTF8String];
    (!bytes.is_null()).then(|| {
        std::ffi::CStr::from_ptr(bytes)
            .to_string_lossy()
            .into_owned()
    })
}
