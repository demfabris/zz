mod assets;
mod showcase;

#[cfg(target_family = "wasm")]
use gpui::ApplicationHandle;
use gpui::{App, AppContext as _, WindowOptions};
use zz_ui::{Root, Theme};

use assets::{INTER_FONT_FAMILY, ShowcaseAssets, inter_fonts};
use showcase::{Showcase, ShowcaseShell};

fn launch(cx: &mut App) {
    cx.text_system()
        .add_fonts(inter_fonts())
        .expect("failed to load the showcase Inter fonts");
    zz_ui::init(cx);

    let theme = Theme::global_mut(cx);
    theme.font_family = INTER_FONT_FAMILY.into();
    theme.mono_font_family = "Lilex".into();

    cx.open_window(WindowOptions::default(), |window, cx| {
        window.set_default_corner_smoothing(4.0);
        window.set_adaptive_corner_fraction(Some(0.45));
        let showcase = cx.new(|cx| Showcase::new(window, cx));
        let shell = cx.new(|_| ShowcaseShell::new(showcase));
        cx.new(|cx| Root::new(shell, window, cx).bordered(false))
    })
    .expect("failed to open the zz UI showcase window");
    cx.activate(true);
}

#[cfg(target_family = "wasm")]
thread_local! {
    static APPLICATION: std::cell::RefCell<Option<ApplicationHandle>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn run() {
    gpui_platform::web_init();
    let application = gpui_platform::single_threaded_web().with_assets(ShowcaseAssets);
    let handle = application.run_embedded(launch);
    APPLICATION.with(|application| application.replace(Some(handle)));
}

/// Native entrypoint, for `cargo check` and for debugging the same view without
/// a browser. Day to day the showcase runs as WASM.
#[cfg(not(target_family = "wasm"))]
pub fn run_native() {
    gpui_platform::application()
        .with_assets(ShowcaseAssets)
        .run(launch);
}
