//! One-time first-run prompt offering to import Ghostty/tmux configuration.

use std::path::{Path, PathBuf};

use gpui::{App, Window};
use zz_ui::WindowExt as _;
use zz_ui::feedback::import_configuration_alert;

use crate::{config, user_data::platform_data_dir};

const MARKER_FILE_NAME: &str = "import-prompted";

fn marker_path() -> Option<PathBuf> {
    platform_data_dir().map(|data| data.join("zz").join(MARKER_FILE_NAME))
}

fn mark_prompted() {
    let Some(path) = marker_path() else {
        return;
    };
    if let Err(error) = config::atomic_write(&path, b"") {
        log::warn!(
            target: "zz::config",
            "could not persist the import prompt marker path={} error={error}",
            path.display(),
        );
    }
}

/// Offer the one-time import on first launch, only when a Ghostty or tmux config
/// exists. Call it after the window's Root dialog layer is mounted.
pub(crate) fn maybe_prompt(window: &mut Window, cx: &mut App) {
    if marker_path().as_deref().is_some_and(Path::exists) {
        return;
    }
    if !config::import::donors_present() {
        mark_prompted();
        return;
    }
    window.open_alert_dialog(cx, |alert, _, _| {
        import_configuration_alert(
            alert,
            "zz found existing Ghostty or tmux configuration. Import it now? zz reads only its \
             own files: your Ghostty appearance is copied into zz/config and your tmux \
             configuration into zz/mux.conf; the originals are never modified. If you skip, zz \
             starts with its defaults and you can import any time from Settings.",
        )
        .on_ok(|_, _, cx| {
            crate::config::settings::run_import(cx);
            mark_prompted();
            true
        })
        .on_cancel(|_, _, _| {
            mark_prompted();
            true
        })
    });
}
