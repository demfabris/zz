//! zz's storybook: every piece of `zz-ui` in its states, on one web page.
//!
//! The page (`web/`) renders the navigation, prose and knobs as HTML from the
//! registry in [`stories`]. Each section of the story on screen is its own
//! zz-gpui window, mounted into that section's element and sized to its content,
//! so people and agents get real page structure around live zz UI. The
//! sections' accessibility trees are mirrored into the page, and
//! `globalThis.zzGpui` drives them.

mod backdrop;
mod host;
mod knobs;
mod stories;
mod story;

use std::borrow::Cow;
use std::cell::RefCell;

use serde_json::json;
use zz_gpui::{AnyWindowHandle, App, AppContext as _, WindowOptions};
use zz_ui::{ActiveTheme as _, Colorize as _, Root, Theme, to_hex};

pub use knobs::Knobs;
pub use stories::STORIES;
pub use story::{Section, Story};

use host::SectionHost;

thread_local! {
    static KNOBS: RefCell<Knobs> = RefCell::new(Knobs::default());
    static OPEN: RefCell<Vec<AnyWindowHandle>> = const { RefCell::new(Vec::new()) };
}

fn launch(cx: &mut App) {
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
        .expect("failed to load the interface fonts");
    zz_ui::init(cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = "Inter Variable".into();
    theme.mono_font_family = "Lilex".into();
    KNOBS.with(|knobs| knobs.borrow().apply(cx));
}

pub(crate) fn knobs() -> Knobs {
    KNOBS.with(|knobs| *knobs.borrow())
}

pub fn find(id: &str) -> Option<&'static Story> {
    STORIES.iter().find(|story| story.id == id).copied()
}

pub fn stories_json() -> String {
    json!(
        STORIES
            .iter()
            .map(|story| json!({
                "id": story.id,
                "name": story.name,
                "group": story.group,
                "summary": story.summary,
                "sections": story.sections.iter().map(|section| json!({
                    "id": section.id,
                    "name": section.name,
                    "summary": section.summary,
                    "mount": mount_id(story, section),
                })).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>()
    )
    .to_string()
}

pub fn presets_json() -> String {
    json!(
        zz_ui::chrome_palette::CHROME_PRESETS
            .iter()
            .map(|preset| json!({
                "id": preset.id.as_str(),
                "name": preset.name,
                "dark": preset.dark,
            }))
            .collect::<Vec<_>>()
    )
    .to_string()
}

pub fn glass_presets_json() -> String {
    json!(
        zz_gpui::GlassMaterial::PRESETS
            .iter()
            .map(|(id, material)| {
                let material = material();
                let values = knobs::GLASS_KNOBS
                    .iter()
                    .map(|(name, get, _)| ((*name).to_owned(), json!(get(&material))))
                    .collect::<serde_json::Map<_, _>>();
                ((*id).to_owned(), serde_json::Value::Object(values))
            })
            .collect::<serde_json::Map<_, _>>()
    )
    .to_string()
}

pub fn mount_id(story: &Story, section: &Section) -> String {
    format!("section-{}-{}", story.id, section.id)
}

pub fn show_story(story: &'static Story, cx: &mut App) -> Result<(), String> {
    for handle in OPEN.with(|open| std::mem::take(&mut *open.borrow_mut())) {
        handle
            .update(cx, |_, window, _| window.remove_window())
            .ok();
    }
    let knobs = knobs();
    for section in story.sections {
        let mount = format!("#{}", mount_id(story, section));
        let handle = cx
            .open_window(
                WindowOptions {
                    mount: Some(mount.into()),
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title(section.name);
                    window.set_zoom(knobs.zoom);
                    window.set_default_corner_smoothing(knobs.smoothing);
                    window.set_adaptive_corner_fraction(Some(0.45));
                    let content = (section.build)(window, cx);
                    let floats = backdrop::floats(story.id, section.id);
                    let host = cx.new(|_| SectionHost::new(content, floats));
                    cx.new(|cx| Root::new(host, window, cx).bordered(false))
                },
            )
            .map_err(|error| format!("{}: {error:#}", section.id))?;
        OPEN.with(|open| open.borrow_mut().push(handle.into()));
    }
    Ok(())
}

pub fn set_knobs(query: &str, cx: &mut App) -> Result<(), String> {
    let knobs = Knobs::parse(query)?;
    KNOBS.with(|current| *current.borrow_mut() = knobs);
    knobs.apply(cx);
    Ok(())
}

pub fn theme_json(cx: &App) -> String {
    let theme = cx.theme();
    json!({
        "mode": theme.mode.name(),
        "background": to_hex(theme.background),
        "raised": to_hex(theme.background.raised(1)),
        "foreground": to_hex(theme.foreground),
        "muted": to_hex(theme.foreground.muted()),
        "border": to_hex(theme.border()),
        "accent": to_hex(theme.accent),
        "radius": f32::from(theme.radius),
    })
    .to_string()
}

#[cfg(target_family = "wasm")]
mod web {
    use std::cell::{Cell, RefCell};

    use wasm_bindgen::prelude::*;

    thread_local! {
        static APPLICATION: RefCell<Option<zz_gpui::ApplicationHandle>> = const { RefCell::new(None) };
        static READY: Cell<bool> = const { Cell::new(false) };
    }

    fn with_app<R>(f: impl FnOnce(&mut zz_gpui::App) -> R) -> Result<R, JsValue> {
        APPLICATION.with(|application| {
            application
                .borrow()
                .as_ref()
                .map(|handle| handle.update(f))
                .ok_or_else(|| JsValue::from_str("the storybook is not running"))
        })
    }

    #[wasm_bindgen]
    pub fn run() -> Result<(), JsValue> {
        if APPLICATION.with(|application| application.borrow().is_some()) {
            return Err(JsValue::from_str("the storybook is already running"));
        }
        zz_gpui_platform::web_init();
        let application = zz_gpui_platform::single_threaded_web().with_assets(zz_ui::Assets);
        let handle = application.run_embedded(|cx| {
            super::launch(cx);
            READY.with(|ready| ready.set(true));
        });
        APPLICATION.with(|application| application.replace(Some(handle)));
        Ok(())
    }

    #[wasm_bindgen]
    pub fn is_ready() -> bool {
        READY.with(Cell::get)
    }

    #[wasm_bindgen]
    pub fn stories() -> String {
        super::stories_json()
    }

    #[wasm_bindgen]
    pub fn presets() -> String {
        super::presets_json()
    }

    #[wasm_bindgen]
    pub fn glass_presets() -> String {
        super::glass_presets_json()
    }

    #[wasm_bindgen]
    pub fn show(story: &str) -> Result<(), JsValue> {
        let story = super::find(story).ok_or_else(|| JsValue::from_str("no such story"))?;
        with_app(|cx| super::show_story(story, cx))?.map_err(|error| JsValue::from_str(&error))
    }

    #[wasm_bindgen]
    pub fn set_knobs(query: &str) -> Result<(), JsValue> {
        with_app(|cx| super::set_knobs(query, cx))?.map_err(|error| JsValue::from_str(&error))
    }

    #[wasm_bindgen]
    pub fn theme() -> Result<String, JsValue> {
        with_app(|cx| super::theme_json(cx))
    }
}
