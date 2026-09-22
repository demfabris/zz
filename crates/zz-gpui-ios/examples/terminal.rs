#[cfg(target_os = "ios")]
#[path = "support/connection.rs"]
mod connection;
#[cfg(target_os = "ios")]
#[path = "support/input.rs"]
mod input;
#[cfg(target_os = "ios")]
#[path = "support/app.rs"]
mod terminal_app;

#[cfg(target_os = "ios")]
mod app {
    use super::terminal_app::TerminalApp;
    use gpui::{App, AppContext, Application, WindowOptions};
    use std::{borrow::Cow, rc::Rc};
    use zz_ui::{Root, Theme, UiZoom};

    pub fn run() {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
        Application::with_platform(Rc::new(zz_gpui_ios::IosPlatform::new()))
            .with_assets(zz_ui::Assets)
            .run(|cx: &mut App| {
                cx.text_system()
                    .add_fonts(vec![
                        Cow::Borrowed(include_bytes!(
                            "../../../clients/web/assets/fonts/lilex/Lilex-Regular.ttf"
                        )),
                        Cow::Borrowed(include_bytes!(
                            "../../../clients/web/assets/fonts/lilex/Lilex-Bold.ttf"
                        )),
                        Cow::Borrowed(include_bytes!(
                            "../../../clients/web/assets/fonts/lilex/Lilex-Italic.ttf"
                        )),
                        Cow::Borrowed(include_bytes!(
                            "../../../clients/web/assets/fonts/lilex/Lilex-BoldItalic.ttf"
                        )),
                    ])
                    .expect("load terminal font");
                zz_ui::init(cx);
                cx.bind_keys(crate::input::raw_key_bindings());
                Theme::sync_system_appearance(None, cx);
                let theme = Theme::global_mut(cx);
                theme.font_family = "Lilex".into();
                theme.mono_font_family = "Lilex".into();
                cx.set_global(UiZoom(1.0));
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let demo = cx.new(|cx| TerminalApp::new(window, cx));
                    window.focus(&demo.read(cx).focus.clone(), cx);
                    cx.new(|cx| Root::new(demo, window, cx).bordered(false))
                })
                .expect("open GPUI iOS window");
            });
    }
}

#[cfg(target_os = "ios")]
fn main() {
    app::run();
}

#[cfg(not(target_os = "ios"))]
fn main() {}
