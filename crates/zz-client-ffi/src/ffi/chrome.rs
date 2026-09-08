use std::ffi::{CStr, c_char};

use zz_client::{CHROME_TABLES, ChromeKey, ChromeKeymap, ChromeProfile};
use zz_terminal::{KeyAction, KeyInput, Modifiers};

use super::{ZzBytes, key_action, key_code};

pub struct ZzChromeKeymap(pub(super) ChromeKeymap);

#[unsafe(no_mangle)]
pub extern "C" fn zz_chrome_keymap_new() -> *mut ZzChromeKeymap {
    Box::into_raw(Box::new(ZzChromeKeymap(ChromeKeymap::for_profile(
        ChromeProfile::DESKTOP,
    ))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_chrome_keymap_free(keymap: *mut ZzChromeKeymap) {
    if !keymap.is_null() {
        drop(unsafe { Box::from_raw(keymap) });
    }
}

unsafe fn string<'a>(value: *const c_char) -> Option<&'a str> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value) }.to_str().ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_chrome_keymap_bind(
    keymap: *mut ZzChromeKeymap,
    table: *const c_char,
    key: *const c_char,
    action: *const c_char,
) -> bool {
    let (Some(keymap), Some(table), Some(key), Some(action)) =
        (unsafe { (keymap.as_mut(), string(table), string(key), string(action)) })
    else {
        return false;
    };
    CHROME_TABLES.contains(&table)
        && ChromeKey::parse(key).is_some()
        && keymap.0.bind(table, key, action).is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_chrome_keymap_unbind(
    keymap: *mut ZzChromeKeymap,
    table: *const c_char,
    key: *const c_char,
) -> bool {
    let (Some(keymap), Some(table), Some(key)) =
        (unsafe { (keymap.as_mut(), string(table), string(key)) })
    else {
        return false;
    };
    keymap.0.unbind(table, key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_chrome_keymap_resolve(
    keymap: *const ZzChromeKeymap,
    table: *const c_char,
    code: u32,
    codepoint: u32,
    function: u8,
    action: u32,
    modifiers: u8,
    text: *const c_char,
) -> ZzBytes {
    let (Some(keymap), Some(table), Some(key), Some(action), Some(modifiers)) = (
        unsafe { keymap.as_ref() },
        unsafe { string(table) },
        key_code(code, codepoint, function),
        key_action(action),
        Modifiers::from_bits(modifiers),
    ) else {
        return ZzBytes::EMPTY;
    };
    if action == KeyAction::Release {
        return ZzBytes::EMPTY;
    }
    let input = KeyInput {
        key,
        action,
        modifiers,
        text: unsafe { string(text) }.map(Into::into),
        unshifted_codepoint: None,
    };
    keymap
        .0
        .resolve(table, &input)
        .map_or(ZzBytes::EMPTY, |action| ZzBytes::new(action.name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_resolve_through_the_shared_keymap_and_ignore_releases() {
        unsafe {
            let keymap = zz_chrome_keymap_new();
            assert!(zz_chrome_keymap_bind(
                keymap,
                c"ui".as_ptr(),
                c"D-j".as_ptr(),
                c"open-settings".as_ptr()
            ));
            let result = zz_chrome_keymap_resolve(
                keymap,
                c"ui".as_ptr(),
                0,
                'j' as u32,
                0,
                0,
                8,
                c"j".as_ptr(),
            );
            assert_eq!(
                std::slice::from_raw_parts(result.ptr, result.len),
                b"open-settings"
            );
            assert_eq!(
                zz_chrome_keymap_resolve(
                    keymap,
                    c"ui".as_ptr(),
                    0,
                    'j' as u32,
                    0,
                    2,
                    8,
                    c"j".as_ptr()
                )
                .len,
                0
            );
            assert!(zz_chrome_keymap_unbind(
                keymap,
                c"ui".as_ptr(),
                c"D-j".as_ptr()
            ));
            assert_eq!(
                zz_chrome_keymap_resolve(
                    keymap,
                    c"ui".as_ptr(),
                    0,
                    'j' as u32,
                    0,
                    0,
                    8,
                    c"j".as_ptr()
                )
                .len,
                0
            );
            assert!(!zz_chrome_keymap_bind(
                keymap,
                c"missing".as_ptr(),
                c"D-j".as_ptr(),
                c"open-settings".as_ptr()
            ));
            zz_chrome_keymap_free(keymap);
        }
    }
}
