use serde_json::json;
use zz_config::update::{Channel, build_version, checks_enabled, fetch_latest, native_macos_asset};

use super::ZzJson;

#[unsafe(no_mangle)]
pub extern "C" fn zz_update_checks_enabled() -> bool {
    checks_enabled()
}

#[unsafe(no_mangle)]
pub extern "C" fn zz_update_check_native() -> *mut ZzJson {
    Box::into_raw(Box::new(ZzJson::new(check_native())))
}

fn check_native() -> serde_json::Value {
    let current = match build_version() {
        Ok(version) => version,
        Err(error) => {
            return json!({"current": env!("CARGO_PKG_VERSION"), "channel": "unknown", "state": "failed", "version": null, "release_url": null, "asset": null, "error": error});
        }
    };
    let channel = Channel::of(&current);
    let mut result = json!({
        "current": current.to_string(),
        "channel": channel.label(),
        "state": "up_to_date",
        "version": null,
        "release_url": null,
        "asset": null,
        "error": null
    });
    match fetch_latest(channel) {
        Ok(Some(release)) if release.version > current => {
            result["state"] = json!("available");
            result["version"] = json!(release.version.to_string());
            result["release_url"] = json!(release.url);
            if cfg!(target_os = "macos")
                && let Some(asset) = native_macos_asset(&release, std::env::consts::ARCH)
            {
                result["asset"] = json!({"name": asset.name, "url": asset.url});
            }
        }
        Ok(Some(_)) => {}
        outcome => {
            result["state"] = json!("failed");
            result["error"] = json!(match outcome {
                Err(error) => error,
                _ => "the release list carries no release".to_owned(),
            });
        }
    }
    result
}
