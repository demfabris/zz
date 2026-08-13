//! The executable inside `ZZ.app`. The non-iOS entry point stays empty so host
//! workspace checks can still build this package.

#[cfg(target_os = "ios")]
mod app;
#[cfg(target_os = "ios")]
mod chrome;
// The touch-first shell (src/drawer.rs, src/settings.rs) is parked un-compiled;
// re-add the module declarations to revive it.

#[cfg(target_os = "ios")]
fn main() {
    crate::app::run(zz::AppProfile {
        settings_sections: vec![
            zz::SettingsSection::Appearance,
            zz::SettingsSection::Panes,
            zz::SettingsSection::Terminal,
            zz::SettingsSection::Multiplexer,
            zz::SettingsSection::Advanced,
            zz::SettingsSection::About,
        ],
        has_tray: false,
        has_window_blur: false,
        has_daemon_lifecycle: false,
        has_config_import: false,
        local_host: zz::LocalHostPolicy::IfEnvSocket,
        fixed_window: true,
    });
}

#[cfg(not(target_os = "ios"))]
fn main() {}
