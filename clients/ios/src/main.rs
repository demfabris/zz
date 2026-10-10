#[cfg(target_os = "ios")]
use zz_app::{app, input};

#[cfg(target_os = "ios")]
fn main() {
    use std::{borrow::Cow, rc::Rc};
    use zz_gpui::{App, Application, px};
    use zz_ui::{Theme, UiZoom};

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let application = Application::with_platform(Rc::new(
        zz_gpui_platform::ios::IosPlatform::new().with_touch_gestures(true),
    ))
    .with_assets(zz_ui::Assets);
    let (url_sender, mut url_receiver) = futures::channel::mpsc::unbounded::<Vec<String>>();
    application.on_open_urls(move |urls| {
        url_sender.unbounded_send(urls).ok();
    });
    application.on_reopen(open_workspace);
    application.run(|cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![
                Cow::Borrowed(include_bytes!(
                    "../../web/assets/fonts/inter/InterVariable.ttf"
                )),
                Cow::Borrowed(include_bytes!(
                    "../../web/assets/fonts/inter/InterVariable-Italic.ttf"
                )),
                Cow::Borrowed(include_bytes!(
                    "../../web/assets/fonts/lilex/Lilex-Regular.ttf"
                )),
                Cow::Borrowed(include_bytes!(
                    "../../web/assets/fonts/lilex/Lilex-Bold.ttf"
                )),
                Cow::Borrowed(include_bytes!(
                    "../../web/assets/fonts/lilex/Lilex-Italic.ttf"
                )),
                Cow::Borrowed(include_bytes!(
                    "../../web/assets/fonts/lilex/Lilex-BoldItalic.ttf"
                )),
            ])
            .expect("load interface fonts");
        zz_ui::init(cx);
        cx.bind_keys(input::raw_key_bindings());
        Theme::sync_system_appearance(None, cx);
        let theme = Theme::global_mut(cx);
        theme.font_family = "Inter Variable".into();
        theme.mono_font_family = "Lilex".into();
        theme.radius = px(6.);
        theme.glass = std::env::var("ZZ_GPUI_GLASS")
            .ok()
            .and_then(|name| zz_gpui::GlassMaterial::preset(&name));
        cx.set_global(UiZoom(1.0));
        open_workspace(cx);
        cx.spawn(async move |cx| {
            use futures::StreamExt as _;
            while let Some(urls) = url_receiver.next().await {
                for name in urls.iter().filter_map(|url| app::session_from_url(url)) {
                    cx.update(|cx| cx.dispatch_action(&app::OpenSession { name }));
                }
            }
        })
        .detach();
    });
}

#[cfg(target_os = "ios")]
fn open_workspace(cx: &mut zz_gpui::App) {
    use zz_gpui::{AppContext, WindowOptions};
    use zz_ui::Root;

    cx.open_window(WindowOptions::default(), |window, cx| {
        window.set_default_corner_smoothing(4.0);
        window.set_adaptive_corner_fraction(Some(0.45));
        let app = cx.new(|cx| app::AppShell::new(window, cx));
        window.focus(&app.read(cx).focus.clone(), cx);
        cx.new(|cx| Root::new(app, window, cx).bordered(false))
    })
    .expect("open zz iOS window");
}

#[cfg(not(target_os = "ios"))]
fn main() {}
