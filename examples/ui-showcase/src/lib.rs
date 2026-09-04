mod assets;
mod preview;
mod showcase;

#[cfg(target_family = "wasm")]
use gpui::ApplicationHandle;
use gpui::{
    App, AppContext as _, Bounds, Styled as _, WindowBounds, WindowOptions, point, px, size,
};
use zz_ui::{Root, Theme};

use assets::inter_fonts;
use preview::{Preview, PreviewOptions};
use showcase::{Showcase, ShowcaseShell};

fn launch(cx: &mut App, mut options: PreviewOptions, fonts: Vec<std::borrow::Cow<'static, [u8]>>) {
    cx.text_system()
        .add_fonts(inter_fonts())
        .expect("failed to load the showcase Inter fonts");
    zz_ui::init(cx);
    cx.text_system()
        .add_fonts(fonts)
        .expect("failed to load the preview fonts");
    options.normalize();
    Theme::change(
        if options.dark {
            zz_ui::ThemeMode::Dark
        } else {
            zz_ui::ThemeMode::Light
        },
        None,
        cx,
    );
    let theme = Theme::global_mut(cx);
    if !options.ui_font.is_empty() {
        theme.font_family = options.ui_font.clone().into();
    }
    if !options.mono_font.is_empty() {
        theme.mono_font_family = options.mono_font.clone().into();
    }
    theme.radius = gpui::px(options.radius);
    cx.set_global(zz_ui::UiZoom(options.zoom));
    cx.set_global(options.clone());

    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(80.0), px(80.0)),
                size(px(options.width), px(options.height)),
            ))),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("zz UI preview".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| {
            #[cfg(all(not(target_family = "wasm"), feature = "native-capture"))]
            if let Ok(path) = std::env::var("ZZ_PREVIEW_CAPTURE") {
                window.on_next_frame(move |window, _| {
                    window.on_next_frame(move |window, cx| {
                        match window.render_to_image().and_then(|image| {
                            image.save(&path)?;
                            Ok(())
                        }) {
                            Ok(()) => println!("Saved native preview: {path}"),
                            Err(error) => {
                                eprintln!("Failed to capture native preview: {error}");
                                std::process::exit(1);
                            }
                        }
                        cx.quit();
                    });
                    window.refresh();
                });
            }
            window.set_default_corner_smoothing(4.0);
            window.set_adaptive_corner_fraction(Some(0.45));
            let shell: gpui::AnyView = if options.scene == "catalog" {
                let showcase = cx.new(|cx| Showcase::new(window, cx));
                cx.new(|_| ShowcaseShell::new(showcase)).into()
            } else {
                cx.new(|cx| Preview::new(options, window, cx)).into()
            };
            cx.new(|cx| {
                Root::new(shell, window, cx)
                    .bordered(false)
                    .bg(Theme::global(cx).transparent)
            })
        },
    )
    .expect("failed to open the zz UI showcase window");
    cx.activate(true);
}

#[cfg(target_family = "wasm")]
thread_local! {
    static APPLICATION: std::cell::RefCell<Option<ApplicationHandle>> = const {
        std::cell::RefCell::new(None)
    };
    static FONTS: std::cell::RefCell<Vec<std::borrow::Cow<'static, [u8]>>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn register_font(bytes: Vec<u8>) {
    FONTS.with(|fonts| fonts.borrow_mut().push(std::borrow::Cow::Owned(bytes)));
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn run(settings: &str) -> Result<(), wasm_bindgen::JsValue> {
    let options: PreviewOptions = serde_json::from_str(settings)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    gpui_platform::web_init();
    let application = gpui_platform::single_threaded_web().with_assets(zz_ui::Assets);
    let fonts = FONTS.with(|fonts| fonts.take());
    let handle = application.run_embedded(move |cx| launch(cx, options, fonts));
    APPLICATION.with(|application| application.replace(Some(handle)));
    Ok(())
}

/// Native entrypoint, for `cargo check` and for debugging the same view without
/// a browser. Day to day the showcase runs as WASM.
#[cfg(not(target_family = "wasm"))]
pub fn run_native() {
    let options = std::env::var("ZZ_PREVIEW_OPTIONS")
        .ok()
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .unwrap_or_else(|error| {
            eprintln!("invalid ZZ_PREVIEW_OPTIONS: {error}");
            std::process::exit(2);
        })
        .unwrap_or_default();
    gpui_platform::application()
        .with_assets(zz_ui::Assets)
        .run(move |cx| launch(cx, options, Vec::new()));
}
