//! The engine surface: everything an app crate needs to assemble a zz app.
//! Shell code is not part of it; an app that wants different chrome builds its
//! own against this surface.

pub mod config {
    pub use crate::app_icon::AppIconSetting;
    pub use crate::config::{
        AppConfig, ConfigKey, ConfigProvenance, ConfigValue, MAX_CONFIG_BYTES, agent_config,
        agent_pane_enabled, appearance_editor_view, daemon_config_overrides, import_target_path,
        init, read_config_editor_source, remove_config_key, request_daemon_reload, resolved_config,
        save_appearance_editor, set_chrome_preset, set_config_key, write_config_editor_source,
    };

    #[must_use]
    pub fn show_fps(cx: &gpui::App) -> bool {
        crate::config::resolved_config(cx).show_fps.value
    }

    pub mod settings {
        pub use crate::config::settings::{OpenSettings, init};
    }
}

pub mod nav {
    pub use crate::mux::nav::*;
}

pub mod diagnostics {
    pub use crate::diagnostics::{
        init, init_debug_mark, start_app_state_sampler, start_main_thread_watchdog,
    };
}

pub mod mux {
    pub use crate::mux::client::MuxClient;
    pub use crate::mux::hosts::{HostId, HostState};
}

pub mod agent {
    pub use crate::agent::{AgentController, AgentPreferences};
}

pub mod browser {
    pub use crate::browser::controller::BrowserController;

    pub mod recent_pages {
        pub use crate::browser::recent_pages::init;
    }

    pub mod view {
        pub use crate::browser::view::init;
    }
}

pub mod editor {
    pub use crate::editor::init;
}

pub mod terminal {
    pub mod view {
        pub use crate::terminal::view::init;
    }
}

pub mod workspace {
    pub use crate::workspace::{AppView, init};

    pub mod add_host {
        pub use crate::workspace::add_host::open;
    }

    /// Writes `host-<name> = <endpoint>` and republishes the live fleet.
    pub use crate::config::add_fleet_host;

    pub use crate::workspace::sidebar::{WorkspaceRoute, WorkspaceSidebar};
}

pub mod theme {
    pub use crate::theme::{
        CHROME_PRESETS, ChromeColor, ChromePreset, ChromePresetId, ThemeModeSetting,
        chrome_background, inherited_chrome_colors, sync_system_appearance,
    };
}

pub mod ui_scale {
    #[cfg(target_os = "ios")]
    pub use crate::ui_scale::scale_by;
    pub use crate::ui_scale::{
        MAX_UI_ZOOM, MIN_UI_ZOOM, UI_ZOOM_STEP, apply_to_new_window, init, is_default,
        is_effective_percent, percent, reset, set_percent,
    };
}

pub mod window {
    pub mod background {
        pub use crate::window::background::detect_compositor_support;
    }

    pub mod toast {
        pub use crate::window::toast::set_host;
    }
}

pub use crate::app_shell::AppFpsMeter;
pub use crate::app_shell::AppShell;
pub use crate::build_root;
pub use crate::terminal_color_scheme;
pub use zz_daemon::default_socket_path;
pub use zz_daemon::mux_config_write_path;

#[cfg(target_os = "ios")]
pub use crate::ios_input::{IosAccessory, send_with_sticky_modifiers};

/// Attach to the local daemon when this app profile has one.
#[cfg(target_os = "ios")]
pub fn connect_local(
    profile: &crate::AppProfile,
    path: &std::path::Path,
    color_scheme: zz_terminal::TerminalColorScheme,
) -> Result<zz_daemon::InteractiveClient, zz_daemon::DaemonError> {
    if profile.local_host.synthesize_local() {
        crate::connect_interactive_client(path, color_scheme)
    } else {
        Err(zz_daemon::DaemonError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no local daemon on this platform",
        )))
    }
}

/// The gpui revision this build links, stamped by zz's build script.
#[must_use]
pub fn gpui_source() -> &'static str {
    crate::GPUI_SOURCE
}
