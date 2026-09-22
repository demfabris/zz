#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

#[test]
fn library_is_absent_until_requested_and_typed_calls_remain_usable() {
    assert!(!cef_dll_sys::is_library_loaded());
    assert!(!std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .contains("/libcef.so"));
    cef_dll_sys::load_library().unwrap();
    assert!(cef_dll_sys::is_library_loaded());
    assert!(std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .contains("/libcef.so"));
    unsafe {
        let hash = cef_dll_sys::cef_api_hash(cef_dll_sys::CEF_API_VERSION, 0);
        assert!(!hash.is_null());
        let original = "CEF delayed loading";
        let mut output: cef_dll_sys::cef_string_utf16_t = std::mem::zeroed();
        assert_eq!(
            cef_dll_sys::cef_string_utf8_to_utf16(
                original.as_ptr().cast(),
                original.len(),
                &mut output,
            ),
            1
        );
        assert_eq!(
            String::from_utf16(std::slice::from_raw_parts(output.str_, output.length)).unwrap(),
            original,
        );
        cef_dll_sys::cef_string_utf16_clear(&mut output);
    }
    cef_dll_sys::load_library().unwrap();
}
