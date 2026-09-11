use std::path::{Path, PathBuf};

use gpui::{App, Window};
use zz_ui::WindowExt as _;
use zz_ui::feedback::import_configuration_file_alert;

use crate::{config, user_data::platform_data_dir};

const MARKER_FILE_NAME: &str = "import-prompted-v2";

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
    let ghostty = zz_terminal::discover_ghostty_config();
    let tmux = zz_daemon::discover_tmux_config();
    if ghostty.is_none() && tmux.is_none() {
        mark_prompted();
        return;
    }
    let donors = ghostty
        .iter()
        .chain(tmux.iter())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" and ");
    let title = match (ghostty.is_some(), tmux.is_some()) {
        (true, true) => "Import from Ghostty and tmux?",
        (true, false) => "Import from Ghostty?",
        _ => "Import from tmux?",
    };
    let targets = match (ghostty.is_some(), tmux.is_some()) {
        (true, true) => "zz/config and zz/mux.conf",
        (true, false) => "zz/config",
        _ => "zz/mux.conf",
    };
    window.open_alert_dialog(cx, move |alert, _, _| {
        let tmux = tmux.clone();
        let has_ghostty = ghostty.is_some();
        import_configuration_file_alert(
            alert, title,
            format!("Found {donors}. zz does not read these files on its own. Import them into {targets}? Use Settings to choose other paths. Cancel imports nothing; you can import later from Settings."),
        )
        .on_ok(move |_, window, cx| {
            if has_ghostty { crate::config::settings::run_import(cx); }
            if let Some(path) = tmux.clone() {
                let task = cx
                    .background_executor()
                    .spawn(async move { config::import_tmux_config(&path) });
                window
                    .spawn(cx, async move |window| {
                        let result = task.await;
                        let _ = window.update(|window, cx| match result {
                            Ok(output) => crate::window::toast::push(
                                zz_ui::notification::Notification::success(output),
                                cx,
                            ),
                            Err(error) => {
                                crate::window::toast::push(
                                    zz_ui::notification::Notification::info(error),
                                    cx,
                                );
                                cx.set_global(config::settings::PendingMuxImport(true));
                                window.dispatch_action(
                                    Box::new(config::settings::OpenSettings),
                                    cx,
                                );
                            }
                        });
                    })
                    .detach();
            }
            mark_prompted();
            true
        })
        .on_cancel(|_, _, _| {
            mark_prompted();
            true
        })
    });
}
