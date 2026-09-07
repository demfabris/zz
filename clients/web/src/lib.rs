mod app;
mod attachments;
mod connection;
mod terminal;

use std::borrow::Cow;

use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, point, px, size};
use zz_ui::{Root, Theme, UiZoom};

fn launch(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!(
                "../../../examples/ui-showcase/assets/fonts/inter/InterVariable.ttf"
            )),
            Cow::Borrowed(include_bytes!(
                "../../../examples/ui-showcase/assets/fonts/inter/InterVariable-Italic.ttf"
            )),
        ])
        .expect("failed to load the interface fonts");
    #[cfg(target_family = "wasm")]
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/NotoSansCJKjp-Regular.otf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/NotoColorEmoji.ttf")),
        ])
        .expect("failed to load the browser fallback fonts");
    zz_ui::init(cx);
    Theme::sync_system_appearance(None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = "Inter Variable".into();
    theme.mono_font_family = "Lilex".into();
    theme.radius = px(6.0);
    cx.set_global(UiZoom(1.0));

    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(80.0), px(80.0)),
                size(px(1200.0), px(800.0)),
            ))),
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title("zz");
            window.set_default_corner_smoothing(4.0);
            window.set_adaptive_corner_fraction(Some(0.45));
            let client = cx.new(|cx| app::WebClient::new(window, cx));
            #[cfg(target_family = "wasm")]
            window.on_next_frame(|_, _| READY.with(|ready| ready.set(true)));
            cx.new(|cx| Root::new(client, window, cx).bordered(false))
        },
    )
    .expect("failed to open zz");
    cx.activate(true);
}

#[cfg(target_family = "wasm")]
thread_local! {
    static APPLICATION: std::cell::RefCell<Option<gpui::ApplicationHandle>> = const {
        std::cell::RefCell::new(None)
    };
    static READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn run() -> Result<(), wasm_bindgen::JsValue> {
    if APPLICATION.with(|application| application.borrow().is_some()) {
        return Err(wasm_bindgen::JsValue::from_str("zz is already running"));
    }
    gpui_platform::web_init();
    let application = gpui_platform::single_threaded_web().with_assets(zz_ui::Assets);
    let handle = application.run_embedded(launch);
    APPLICATION.with(|application| application.replace(Some(handle)));
    Ok(())
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn is_ready() -> bool {
    READY.with(std::cell::Cell::get)
}

#[cfg(not(target_family = "wasm"))]
pub fn run_native() {
    gpui_platform::application()
        .with_assets(zz_ui::Assets)
        .run(launch);
}
