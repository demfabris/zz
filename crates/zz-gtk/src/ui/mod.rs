mod agent;
pub mod browser;
mod colors;
pub mod completion;
mod dialogs;
mod keys;
mod overlay;
mod pager;
pub mod palette;
mod pane;
mod panes;
mod picker;
pub mod prefix;
mod search;
mod settings;
mod shortcuts;
mod sidebar;
mod ssh_prompt;
mod status;
mod terminal;
mod tray;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};
use zz_daemon::Endpoint;
use zz_terminal::TerminalColorScheme;

use crate::engine::Engine;

pub const APP_ID: &str = "sh.zzmux.zz.Gtk";

pub struct Launch {
    pub endpoint: Endpoint,
    pub session: String,
}

pub fn run(launch: Launch) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.set_accels_for_action("win.shortcuts", &["<Primary>question"]);
    glib::set_application_name("zz");
    app.connect_startup(|_| gtk::Window::set_default_icon_name(APP_ID));
    app.connect_activate(move |app| activate(app, &launch));
    let result = app.run_with_args::<&str>(&[]);
    browser::shutdown();
    result
}

fn activate(app: &adw::Application, launch: &Launch) {
    let loading = adw::StatusPage::builder()
        .title("Connecting to zz")
        .child(&adw::Spinner::new())
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&loading));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("zz")
        .default_width(720)
        .default_height(480)
        .content(&toolbar)
        .build();
    window.present();
    let endpoint = launch.endpoint.clone();
    let session = launch.session.clone();
    let scheme = color_scheme();
    let (send, receive) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = send.send_blocking(Engine::connect(&endpoint, &session, scheme));
    });
    let app = app.clone();
    glib::spawn_future_local(async move {
        let Ok(result) = receive.recv().await else {
            return;
        };
        if !window.is_visible() {
            if let Ok(engine) = result {
                engine.detach_all();
            }
            return;
        }
        match result {
            Ok(engine) => {
                follow_system_theme(&engine);
                window::Shell::build(&app, engine).present();
            }
            Err(error) => present_failure(&app, &error),
        }
        window.close();
    });
}

fn color_scheme() -> TerminalColorScheme {
    if adw::StyleManager::default().is_dark() {
        TerminalColorScheme::Dark
    } else {
        TerminalColorScheme::Light
    }
}

/// The daemon resolves the palette, so a live light/dark switch is republished
/// rather than recolored here; the appearance it answers with drives every pane.
fn follow_system_theme(engine: &std::sync::Arc<Engine>) {
    let engine = std::sync::Arc::downgrade(engine);
    adw::StyleManager::default().connect_dark_notify(move |_| {
        if let Some(engine) = engine.upgrade() {
            engine.set_color_scheme(color_scheme());
        }
    });
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
