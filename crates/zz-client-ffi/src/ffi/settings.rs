use super::{ZzClient, ZzJson, lock};
use serde_json::json;
use std::{
    ffi::{CStr, c_char},
    path::PathBuf,
};
use zz_config::settings::{SettingsAction, SettingsModel};
use zz_protocol::{CommandInvocation, PROTOCOL_VERSION};

pub struct ZzSettingsModel(SettingsModel, u64);

unsafe fn string<'a>(value: *const c_char) -> Option<&'a str> {
    if value.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(value) }.to_str().ok()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_new(
    system_font: *const c_char,
    config_path: *const c_char,
    mux_path: *const c_char,
) -> *mut ZzSettingsModel {
    let system_font = unsafe { string(system_font) }
        .unwrap_or_default()
        .to_owned();
    let config = unsafe { string(config_path) }.map(PathBuf::from);
    let mux = unsafe { string(mux_path) }.map(PathBuf::from);
    if config.as_ref().is_some_and(|path| !path.is_absolute())
        || mux.as_ref().is_some_and(|path| !path.is_absolute())
    {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(ZzSettingsModel(
        SettingsModel::new(system_font, config, mux),
        0,
    )))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_free(model: *mut ZzSettingsModel) {
    if !model.is_null() {
        drop(unsafe { Box::from_raw(model) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_poll(model: *mut ZzSettingsModel) -> bool {
    unsafe { model.as_mut() }.is_some_and(|model| model.0.poll())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_snapshot(
    model: *const ZzSettingsModel,
    client: *const ZzClient,
) -> *mut ZzJson {
    let Some(model) = (unsafe { model.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let bindings = unsafe { client.as_ref() }
        .map(|client| lock(&client.core).prefix_bindings().to_vec())
        .unwrap_or_default();
    Box::into_raw(Box::new(ZzJson::new(model.0.snapshot(&bindings))))
}

fn apply(model: &SettingsModel, client: &ZzClient, endpoint: &str) -> Result<(), String> {
    let hello = client.client.server_hello();
    if hello.protocol_version != PROTOCOL_VERSION
        || !hello
            .capabilities
            .iter()
            .any(|value| value == "config-overrides-v1")
    {
        return Ok(());
    }
    let remote = matches!(
        zz_daemon::Endpoint::parse(endpoint),
        Ok(zz_daemon::Endpoint::Ssh(_))
    );
    client
        .client
        .set_config_overrides(zz_config::config_overrides_for_host(
            model.parsed.daemon_entries.clone(),
            remote,
        ))
        .map_err(|error| error.to_string())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_apply(
    model: *mut ZzSettingsModel,
    client: *const ZzClient,
    endpoint: *const c_char,
) -> bool {
    let (Some(model), Some(client), Some(endpoint)) =
        (unsafe { (model.as_mut(), client.as_ref(), string(endpoint)) })
    else {
        return false;
    };
    if let Err(error) = apply(&model.0, client, endpoint) {
        model.0.error = Some(error);
        return false;
    }
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_action(
    model: *mut ZzSettingsModel,
    client: *const ZzClient,
    endpoint: *const c_char,
    action: *const c_char,
) -> bool {
    let (Some(model), Some(action)) = (unsafe { (model.as_mut(), string(action)) }) else {
        return false;
    };
    let client = unsafe { client.as_ref() };
    let bindings = client
        .map(|client| lock(&client.core).prefix_bindings().to_vec())
        .unwrap_or_default();
    let action = match serde_json::from_str::<SettingsAction>(action) {
        Ok(action) => action,
        Err(error) => {
            model.0.error = Some(error.to_string());
            return false;
        }
    };
    let mux_changed = match model.0.action(action, &bindings) {
        Ok(changed) => changed,
        Err(error) => {
            model.0.error = Some(error.to_string());
            return false;
        }
    };
    if let Some(client) = client {
        if let Some(endpoint) = unsafe { string(endpoint) }
            && let Err(error) = apply(&model.0, client, endpoint)
        {
            model.0.error = Some(format!(
                "Saved, but the daemon could not apply the settings: {error}"
            ));
            return false;
        }
        if mux_changed {
            match client
                .client
                .execute(CommandInvocation::new("reload-config", [] as [&str; 0]))
            {
                Ok(request) => model.1 = request,
                Err(error) => {
                    model.0.error = Some(format!(
                        "Saved, but the multiplexer could not reload: {error}"
                    ));
                    return false;
                }
            }
        }
    }
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_appearance_json(client: *const ZzClient) -> *mut ZzJson {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let core = lock(&client.core);
    let Some(appearance) = core.appearance() else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(ZzJson::new(
        json!({"font_families":appearance.font_families,
        "font_size":appearance.font_size_points,"font_weight":appearance.font_weight,
        "background_opacity":appearance.background_opacity,
        "padding":[appearance.padding_top,appearance.padding_right,appearance.padding_bottom,appearance.padding_left],
        "cursor_blink_ms":appearance.cursor_blink_interval_ms}),
    )))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_chrome_keymap(
    model: *const ZzSettingsModel,
) -> *mut super::chrome::ZzChromeKeymap {
    let Some(model) = (unsafe { model.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let keymap = zz_config::keymap::configured_keymap(
        &model.0.parsed.chrome_overrides,
        &model.0.parsed.browser.element_selector_hotkey.value,
    );
    Box::into_raw(Box::new(super::chrome::ZzChromeKeymap(keymap)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_set_color_scheme(client: *const ZzClient, dark: bool) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    client
        .client
        .set_color_scheme(if dark {
            zz_terminal::TerminalColorScheme::Dark
        } else {
            zz_terminal::TerminalColorScheme::Light
        })
        .is_ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_take_reload_request(model: *mut ZzSettingsModel) -> u64 {
    unsafe { model.as_mut() }.map_or(0, |model| std::mem::take(&mut model.1))
}
