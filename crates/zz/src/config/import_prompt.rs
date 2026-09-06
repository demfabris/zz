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
            "zz found existing Ghostty configuration. Import its appearance into zz/config? \
             You can import again from Settings. zz reads your tmux configuration in place at \
             daemon startup; put zz-specific overrides in zz/mux.conf.",
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
