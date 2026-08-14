mod colors;
mod keys;
mod panes;
mod terminal;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};
use zz_daemon::Endpoint;
use zz_terminal::TerminalColorScheme;

use crate::engine::Engine;

pub const APP_ID: &str = "sh.zzmux.zz.Gtk";

/// Where to attach. An empty `session` means the daemon's default, which on a
/// freshly booted daemon is session "0" rather than the newest session.
pub struct Launch {
    pub endpoint: Endpoint,
    pub session: String,
}

pub fn run(launch: Launch) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| activate(app, &launch));
    app.run_with_args::<&str>(&[])
}

fn activate(app: &adw::Application, launch: &Launch) {
    let color_scheme = if adw::StyleManager::default().is_dark() {
        TerminalColorScheme::Dark
    } else {
        TerminalColorScheme::Light
    };
    match Engine::connect(&launch.endpoint, &launch.session, color_scheme) {
        Ok(engine) => window::Shell::build(app, engine).present(),
        Err(error) => present_failure(app, &error),
    }
}

fn present_failure(app: &adw::Application, error: &str) {
    log::error!("zz-gtk could not reach the daemon: {error}");
    let status = adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title("No zz daemon")
        .description(error)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&status));
    adw::ApplicationWindow::builder()
        .application(app)
        .default_width(520)
        .default_height(360)
        .title("zz")
        .content(&toolbar)
        .build()
        .present();
}
