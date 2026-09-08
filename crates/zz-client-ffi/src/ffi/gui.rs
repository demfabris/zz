use super::*;

pub struct ZzJson(Vec<u8>);

impl ZzJson {
    pub(super) fn new(value: serde_json::Value) -> Self {
        Self(value.to_string().into_bytes())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_json_bytes(value: *const ZzJson) -> ZzBytes {
    unsafe { value.as_ref() }.map_or(ZzBytes::EMPTY, |value| ZzBytes::from_bytes(&value.0))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_json_free(value: *mut ZzJson) {
    if !value.is_null() {
        drop(unsafe { Box::from_raw(value) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_snapshot_pane_descriptor(
    snapshot: *const ZzMuxSnapshot,
    pane: u64,
) -> *mut ZzJson {
    let Some(snapshot) = (unsafe { snapshot.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let pane = snapshot
        .snapshot
        .sessions
        .iter()
        .flat_map(|session| &session.windows)
        .flat_map(|window| window.panes.values())
        .find(|candidate| candidate.id.0 == pane);
    let Some(pane) = pane else {
        return std::ptr::null_mut();
    };
    let value = match &pane.kind {
        PaneKindSnapshot::Browser(descriptor) => serde_json::json!({"browser": descriptor}),
        PaneKindSnapshot::Agent(descriptor) => serde_json::json!({"agent": descriptor}),
        _ => serde_json::json!({}),
    };
    Box::into_raw(Box::new(ZzJson::new(value)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_gui_command_next(client: *mut ZzClient) -> *mut ZzJson {
    unsafe { client.as_ref() }
        .and_then(|client| lock(&client.queues.gui_commands).pop_front())
        .map_or(std::ptr::null_mut(), |value| Box::into_raw(Box::new(value)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_gui_response(
    client: *mut ZzClient,
    request_id: u64,
    ok: bool,
    text: *const c_char,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    if text.is_null() {
        return false;
    }
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return false;
    };
    let response = if ok {
        zz_protocol::GuiResponse::Success {
            request_id,
            output: text.to_owned(),
        }
    } else {
        zz_protocol::GuiResponse::Error {
            request_id,
            message: text.to_owned(),
        }
    };
    response.validate().is_ok() && client.client.send_gui_response(response).is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_send_browser_text(
    client: *mut ZzClient,
    pane: u64,
    text: *const c_char,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    if text.is_null() {
        return false;
    }
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return false;
    };
    client
        .client
        .send_input(InputMessage::BrowserSurfaceText {
            pane: PaneId(pane),
            text: text.to_owned(),
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_send_browser_key(
    client: *mut ZzClient,
    pane: u64,
    code: u32,
    codepoint: u32,
    function: u8,
    action: u32,
    modifiers: u8,
    text: *const c_char,
    text_follows: bool,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    let (Some(key), Some(action), Some(modifiers)) = (
        key_code(code, codepoint, function),
        key_action(action),
        Modifiers::from_bits(modifiers),
    ) else {
        return false;
    };
    let text = if text.is_null() {
        None
    } else {
        let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
            return false;
        };
        Some(text.to_owned().into_boxed_str())
    };
    client
        .client
        .send_input(InputMessage::BrowserSurfaceKey {
            pane: PaneId(pane),
            input: KeyInput {
                action,
                key,
                modifiers,
                text,
                unshifted_codepoint: None,
            },
            text_follows,
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_socks_port(client: *const ZzClient) -> u16 {
    unsafe { client.as_ref() }
        .and_then(|client| client.client.socks_port())
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_claims_prefix_key(
    client: *const ZzClient,
    code: u32,
    scalar: u32,
    function: u8,
    modifiers: u8,
) -> bool {
    let (Some(client), Some(key), Some(modifiers)) = (
        unsafe { client.as_ref() },
        key_code(code, scalar, function),
        Modifiers::from_bits(modifiers),
    ) else {
        return false;
    };
    lock(&client.core).claims_prefix_input(&KeyInput {
        key,
        action: KeyAction::Press,
        modifiers,
        text: None,
        unshifted_codepoint: None,
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_cancel_prefix(client: *const ZzClient, request: u64) -> bool {
    unsafe { client.as_ref() }.is_some_and(|client| {
        client
            .client
            .send_input(InputMessage::CancelPrefix {
                request_id: request,
            })
            .is_ok()
    })
}
