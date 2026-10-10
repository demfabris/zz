//! zz's storybook: every piece of `zz-ui` in its states, on one web page.
//!
//! The page (`web/`) renders the navigation, prose and knobs as HTML from the
//! registry in [`stories`]. Each section of the story on screen is its own
//! zz-gpui window, mounted into that section's element and sized to its content,
//! so people and agents get real page structure around live zz UI. The
//! sections' accessibility trees are mirrored into the page, and
//! `globalThis.zzGpui` drives them.

mod assets;
mod backdrop;
mod glyphs;
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
    theme.font_family = knobs::UI_FONT.into();
    theme.mono_font_family = "Lilex".into();
    cx.set_global(zz_ui::IconSource(assets::icon_source));
    KNOBS.with(|knobs| knobs.borrow().apply(cx));
}

pub(crate) fn knobs() -> Knobs {
    KNOBS.with(|knobs| knobs.borrow().clone())
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

pub fn interface_styles_json() -> String {
    json!(
        zz_ui::interface_style::InterfaceStyle::ALL
            .into_iter()
            .map(|style| {
                let look = zz_ui::interface_style::look(style);
                let mut values = knobs::LOOK_KNOBS
                    .iter()
                    .map(|(name, get, _)| ((*name).to_owned(), json!(get(&look))))
                    .collect::<serde_json::Map<_, _>>();
                values.insert("selection".to_owned(), json!(look.selection));
                values.insert("font".to_owned(), json!(look.font));
                values.insert(
                    "glass".to_owned(),
                    json!(look.glass.map(|material| {
                        knobs::GLASS_KNOBS
                            .iter()
                            .map(|(name, get, _)| ((*name).to_owned(), json!(get(&material))))
                            .collect::<serde_json::Map<_, _>>()
                    })),
                );
                (style.as_str().to_owned(), serde_json::Value::Object(values))
            })
            .collect::<serde_json::Map<_, _>>()
    )
    .to_string()
}

/// The look the knobs describe, as JSON a style preset can be built from.
pub fn look_json() -> String {
    serde_json::to_string_pretty(&knobs().look).unwrap_or_default()
}

/// The knob values a look JSON describes; see [`knobs::look_knobs`].
/// Keys the JSON leaves out keep `style`'s value.
pub fn import_look(json: &str, style: &str) -> Result<String, String> {
    use zz_ui::interface_style::{InterfaceStyle, Look, look};
    let style = InterfaceStyle::parse(style).unwrap_or(InterfaceStyle::DEFAULT);
    let serde_json::Value::Object(changes) =
        serde_json::from_str(json).map_err(|error| format!("not JSON: {error}"))?
    else {
        return Err("a look is a JSON object".to_owned());
    };
    let serde_json::Value::Object(mut merged) =
        serde_json::to_value(look(style)).map_err(|error| error.to_string())?
    else {
        return Err("a look is a JSON object".to_owned());
    };
    merged.extend(changes);
    let look = serde_json::from_value::<Look>(serde_json::Value::Object(merged))
        .map_err(|error| format!("not a look: {error}"))?;
    Ok(serde_json::Value::Object(knobs::look_knobs(&look)).to_string())
}

/// Loads a font file and names the families it added.
pub fn add_font(bytes: Vec<u8>, cx: &mut App) -> Result<Vec<String>, String> {
    let before = cx.text_system().all_font_names();
    cx.text_system()
        .add_fonts(vec![Cow::Owned(bytes)])
        .map_err(|error| format!("{error:#}"))?;
    Ok(cx
        .text_system()
        .all_font_names()
        .into_iter()
        .filter(|name| !before.contains(name))
        .collect())
}

pub fn fonts_json(cx: &App) -> String {
    let mut names = cx.text_system().all_font_names();
    names.sort();
    names.dedup();
    json!(names).to_string()
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
                    window.set_default_corner_smoothing(knobs.look.corner_smoothing);
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
    knobs.apply(cx);
    KNOBS.with(|current| *current.borrow_mut() = knobs);
    Ok(())
}

pub fn theme_json(cx: &App) -> String {
    let theme = cx.theme();
    json!({
        "mode": theme.mode.name(),
        "background": to_hex(theme.background),
        "raised": to_hex(theme.background.raised(1)),
        "raised2": to_hex(theme.background.raised(2)),
        "raised3": to_hex(theme.background.raised(3)),
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
        let application =
            zz_gpui_platform::single_threaded_web().with_assets(super::assets::Assets);
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
    pub fn interface_styles() -> String {
        super::interface_styles_json()
    }

    #[wasm_bindgen]
    pub fn look() -> String {
        super::look_json()
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
    pub fn import_look(json: &str, style: &str) -> Result<String, JsValue> {
        super::import_look(json, style).map_err(|error| JsValue::from_str(&error))
    }

    #[wasm_bindgen]
    pub fn add_font(bytes: Vec<u8>) -> Result<String, JsValue> {
        with_app(|cx| super::add_font(bytes, cx))?
            .map(|names| serde_json::json!(names).to_string())
            .map_err(|error| JsValue::from_str(&error))
    }

    #[wasm_bindgen]
    pub fn fonts() -> Result<String, JsValue> {
        with_app(|cx| super::fonts_json(cx))
    }

    #[wasm_bindgen]
    pub fn theme() -> Result<String, JsValue> {
        with_app(|cx| super::theme_json(cx))
    }
}
