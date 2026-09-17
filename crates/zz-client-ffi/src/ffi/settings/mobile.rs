use super::*;
use crate::ffi::ZzViewport;
use std::{path::Path, sync::Arc};
use zz_terminal::{
    AppearanceConfigKey, AppearanceLoad, Color, ColourClass, Cursor, CursorBlinkPolicy,
    PackedStyle, TerminalAppearance, TerminalColorScheme, TerminalDictionary, TerminalViewport,
};

fn scheme(dark: bool) -> TerminalColorScheme {
    if dark {
        TerminalColorScheme::Dark
    } else {
        TerminalColorScheme::Light
    }
}

struct CachedAppearance {
    revision: u64,
    directory: String,
    dark: bool,
    base_hash: u64,
    value: Arc<AppearanceLoad>,
}

#[derive(Default)]
pub(super) struct MobileCache {
    resolved: Option<CachedAppearance>,
    applied_client: Option<(u64, u64)>,
    mux_underlay: std::collections::BTreeMap<zz_protocol::MuxOptionKey, String>,
}

fn resolve_cached(
    model: &ZzSettingsModel,
    directory: &str,
    dark: bool,
    base: Option<&TerminalAppearance>,
) -> Arc<AppearanceLoad> {
    let base_hash = base.map_or(0, TerminalAppearance::stable_hash);
    let mut cache = lock(&model.2);
    if let Some(cached) = &cache.resolved
        && cached.revision == model.0.revision
        && cached.directory == directory
        && cached.dark == dark
        && cached.base_hash == base_hash
    {
        return Arc::clone(&cached.value);
    }
    let value = Arc::new(resolve(&model.0, directory, dark, base));
    cache.resolved = Some(CachedAppearance {
        revision: model.0.revision,
        directory: directory.to_owned(),
        dark,
        base_hash,
        value: Arc::clone(&value),
    });
    value
}

fn resolve(
    model: &SettingsModel,
    directory: &str,
    dark: bool,
    base: Option<&TerminalAppearance>,
) -> AppearanceLoad {
    let mut load = AppearanceLoad::defaults_for(scheme(dark));
    if let Some(base) = base {
        load.appearance = base.clone();
    }
    load.appearance.color_scheme = scheme(dark);
    load.root = model.config_path().ok();
    let entries = model
        .parsed
        .daemon_entries
        .iter()
        .filter(|(key, _)| AppearanceConfigKey::from_config_key(key).is_some())
        .map(|(key, value)| {
            let path = Path::new(directory).join(value);
            let value = if key == "theme" && !Path::new(value).is_absolute() && path.is_file() {
                path.to_string_lossy().into_owned()
            } else {
                value.clone()
            };
            (key.clone(), value)
        })
        .collect::<Vec<_>>();
    zz_terminal::apply_appearance_overrides(load, &entries)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_themes_json(
    model: *const ZzSettingsModel,
    directory: *const c_char,
    dark: bool,
) -> *mut ZzJson {
    if unsafe { model.as_ref() }.is_none() {
        return std::ptr::null_mut();
    }
    let Some(directory) = (unsafe { string(directory) }) else {
        return std::ptr::null_mut();
    };
    let mut themes = std::collections::BTreeMap::new();
    if let Ok(files) = std::fs::read_dir(directory) {
        for file in files.flatten().filter(|file| file.path().is_file()) {
            let path = file.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let load = zz_terminal::apply_appearance_overrides(
                AppearanceLoad::defaults_for(scheme(dark)),
                &[("theme".to_owned(), path.to_string_lossy().into_owned())],
            );
            if load.fatal || load.invalid > 0 {
                continue;
            }
            themes.insert(
                name.to_owned(),
                json!({"name":name,"path":path,
                "background":format!("#{:06x}",load.appearance.background.packed()),
                "foreground":format!("#{:06x}",load.appearance.foreground.packed())}),
            );
        }
    }
    Box::into_raw(Box::new(ZzJson::new(json!(
        themes.into_values().collect::<Vec<_>>()
    ))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_terminal_appearance_json(
    model: *const ZzSettingsModel,
    directory: *const c_char,
    dark: bool,
) -> *mut ZzJson {
    let (Some(model), Some(directory)) = (unsafe { (model.as_ref(), string(directory)) }) else {
        return std::ptr::null_mut();
    };
    let load = resolve_cached(model, directory, dark, None);
    let a = &load.appearance;
    let entry = |key: AppearanceConfigKey| {
        zz_config::appearance_config_values(a, key)
            .ok()
            .and_then(|values| values.into_iter().next())
    };
    Box::into_raw(Box::new(ZzJson::new(json!({
        "font_size":a.font_size_points,"font_weight":a.font_weight,
        "background_opacity":a.background_opacity,"padding":[a.padding_top,a.padding_right,a.padding_bottom,a.padding_left],
        "cursor_blink_ms":a.cursor_blink_interval_ms,"cursor_style":entry(AppearanceConfigKey::CursorStyle),
        "cursor_blink_policy":entry(AppearanceConfigKey::CursorStyleBlink),
        "foreground":a.foreground.packed(),"background":a.background.packed(),"cursor_color":a.cursor_color.packed(),
        "selection_background":(u32::from(a.selection_background.a) << 24 | a.selection_background.rgb().packed()),"selection_foreground":a.selection_foreground.packed(),
        "search_match_color":(u32::from(a.search_match_color.a) << 24 | a.search_match_color.rgb().packed()),"search_current_color":(u32::from(a.search_current_color.a) << 24 | a.search_current_color.rgb().packed()),
        "copy_cursor_color":(u32::from(a.copy_cursor_color.a) << 24 | a.copy_cursor_color.rgb().packed()),
        "palette":a.palette.as_array().iter().map(|color| color.packed()).collect::<Vec<_>>()
    }))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_mobile_action(
    model: *mut ZzSettingsModel,
    client: *const ZzClient,
    action: *const c_char,
) -> bool {
    let (Some(model), Some(action)) = (unsafe { (model.as_mut(), string(action)) }) else {
        return false;
    };
    let bindings = unsafe { client.as_ref() }
        .map(|client| lock(&client.core).prefix_bindings().to_vec())
        .unwrap_or_default();
    let result = serde_json::from_str::<SettingsAction>(action)
        .map_err(|error| error.to_string())
        .and_then(|action| {
            model
                .0
                .action(action, &bindings)
                .map_err(|error| error.to_string())
        });
    if let Err(error) = result {
        model.0.error = Some(error);
        return false;
    }
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_mobile_apply(
    model: *mut ZzSettingsModel,
    client: *const ZzClient,
) -> bool {
    let (Some(model), Some(client)) = (unsafe { (model.as_mut(), client.as_ref()) }) else {
        return false;
    };
    let commands = match model.0.mux_commands() {
        Ok(commands) => commands,
        Err(error) => {
            model.0.error = Some(error.to_string());
            return false;
        }
    };
    let hello = client.client.server_hello();
    let identity = (hello.server_id, 0);
    let mux_options = lock(&client.core).mux_options().clone();
    let current_keys = commands
        .iter()
        .filter(|command| {
            matches!(
                command.name.as_str(),
                "set-option" | "set" | "set-window-option" | "setw"
            )
        })
        .flat_map(|command| {
            command
                .args
                .iter()
                .filter_map(|arg| zz_protocol::MuxOptionKey::from_config_key(arg))
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut cache = lock(&model.2);
    if cache.applied_client != Some(identity) {
        cache.mux_underlay.clear();
        cache.applied_client = Some(identity);
    }
    let removed = cache
        .mux_underlay
        .keys()
        .filter(|key| !current_keys.contains(key))
        .copied()
        .collect::<Vec<_>>();
    for key in removed {
        if let Some(value) = cache.mux_underlay.get(&key)
            && let Err(error) = client.client.execute(CommandInvocation::new(
                "set-option",
                ["-g", key.as_str(), value],
            ))
        {
            model.0.error = Some(error.to_string());
            return false;
        }
        cache.mux_underlay.remove(&key);
    }
    {
        for key in current_keys {
            if let Some(value) = mux_options.get(key) {
                cache
                    .mux_underlay
                    .entry(key)
                    .or_insert_with(|| value.value.clone());
            }
        }
    }
    drop(cache);
    for command in commands {
        if let Err(error) = client.client.execute(command) {
            model.0.error = Some(error.to_string());
            return false;
        }
    }
    true
}

fn restyle(
    viewport: &TerminalViewport,
    appearance: &TerminalAppearance,
    base: Option<&TerminalAppearance>,
    cursor_style: bool,
    cursor_blink: bool,
) -> TerminalViewport {
    let resolve_color = |class: ColourClass, color: Color, default: Color| match class {
        ColourClass::Default => default,
        ColourClass::Palette(index) | ColourClass::IndexedLow(index) => {
            appearance.palette.as_array()[index as usize]
        }
        ColourClass::Resolved | ColourClass::Rgb => color,
    };
    let styles = viewport
        .dictionary
        .styles
        .iter()
        .map(|style| {
            PackedStyle::from_raw(
                resolve_color(
                    style.foreground_class(),
                    style.foreground(),
                    appearance.foreground,
                )
                .packed(),
                resolve_color(
                    style.background_class(),
                    style.background(),
                    appearance.background,
                )
                .packed(),
                style.underline_color_raw(),
                style.attributes(),
                style.underline_kind_raw(),
            )
            .with_classes(style.foreground_class(), style.background_class())
        })
        .collect::<Vec<_>>();
    let mut viewport = viewport.clone();
    if base.is_some_and(|base| viewport.foreground == base.foreground) {
        viewport.foreground = appearance.foreground;
    }
    if base.is_some_and(|base| viewport.background == base.background) {
        viewport.background = appearance.background;
    }
    viewport.dictionary = Arc::new(TerminalDictionary::from_shared(
        styles.into(),
        Arc::clone(&viewport.dictionary.grapheme_offsets),
        Arc::clone(&viewport.dictionary.grapheme_bytes),
    ));
    viewport.cursor = viewport.cursor.map(|cursor| {
        Cursor::new(
            cursor.column(),
            cursor.row(),
            cursor.visible(),
            if cursor_blink {
                match appearance.cursor_blink_policy {
                    CursorBlinkPolicy::On => true,
                    CursorBlinkPolicy::Off => false,
                    CursorBlinkPolicy::Terminal => cursor.blinking(),
                }
            } else {
                cursor.blinking()
            },
            cursor.at_wide_tail(),
            if cursor_style {
                appearance.cursor_style
            } else {
                cursor.style()
            },
            if base.is_some_and(|base| cursor.color() == base.cursor_color) {
                appearance.cursor_color
            } else {
                cursor.color()
            },
        )
    });
    viewport
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_settings_model_viewport_acquire(
    model: *const ZzSettingsModel,
    client: *const ZzClient,
    pane: u64,
    directory: *const c_char,
    dark: bool,
) -> *mut ZzViewport {
    let (Some(model), Some(client), Some(directory)) =
        (unsafe { (model.as_ref(), client.as_ref(), string(directory)) })
    else {
        return std::ptr::null_mut();
    };
    let core = lock(&client.core);
    let Some(viewport) = core.viewport(zz_protocol::PaneId(pane)) else {
        return std::ptr::null_mut();
    };
    if model.0.parsed.daemon_entries.is_empty() {
        return Box::into_raw(Box::new(ZzViewport(viewport.clone())));
    }
    let load = resolve_cached(model, directory, dark, core.appearance());
    let has = |key: &str| {
        model
            .0
            .parsed
            .daemon_entries
            .iter()
            .any(|entry| entry.0 == key)
    };
    Box::into_raw(Box::new(ZzViewport(restyle(
        viewport,
        &load.appearance,
        core.appearance(),
        has("cursor-style"),
        has("cursor-style-blink"),
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_terminal::{TerminalPalette, UnderlineStyle};

    #[test]
    fn local_palette_preserves_rgb_cells_and_shared_cell_storage() {
        let mut viewport = TerminalViewport::blank(2, 1, zz_terminal::SessionStatus::Running);
        let literal = Color::from_packed(0x123456);
        let palette = PackedStyle::new(literal, literal, None, 0, UnderlineStyle::None)
            .with_classes(ColourClass::Palette(1), ColourClass::Default);
        let rgb = PackedStyle::new(literal, literal, None, 0, UnderlineStyle::None)
            .with_classes(ColourClass::Rgb, ColourClass::Rgb);
        viewport.dictionary = Arc::new(TerminalDictionary::from_shared(
            vec![palette, rgb].into(),
            Arc::from([0]),
            Arc::from([]),
        ));
        let mut appearance = TerminalAppearance::default();
        let mut colors = *appearance.palette.as_array();
        colors[1] = Color::from_packed(0xff0000);
        appearance.palette = TerminalPalette::new(colors);
        appearance.background = Color::from_packed(0xabcdef);
        let changed = restyle(
            &viewport,
            &appearance,
            Some(&TerminalAppearance::default()),
            false,
            false,
        );
        assert_eq!(changed.dictionary.styles[0].foreground(), colors[1]);
        assert_eq!(
            changed.dictionary.styles[0].background(),
            appearance.background
        );
        assert_eq!(changed.dictionary.styles[1].foreground(), literal);
        assert!(Arc::ptr_eq(&viewport.cells, &changed.cells));
        assert_eq!(viewport.dictionary.styles[0].foreground(), literal);
    }

    #[test]
    fn local_theme_preserves_terminal_osc_colors() {
        let base = TerminalAppearance::default();
        let appearance = TerminalAppearance {
            foreground: Color::from_packed(0x111111),
            background: Color::from_packed(0x222222),
            cursor_color: Color::from_packed(0x333333),
            ..base.clone()
        };
        let mut viewport = TerminalViewport::blank(1, 1, zz_terminal::SessionStatus::Running);
        viewport.foreground = Color::from_packed(0xaabbcc);
        viewport.background = Color::from_packed(0x112233);
        let cursor_color = Color::from_packed(0xabcdef);
        viewport.cursor = Some(Cursor::new(
            0,
            0,
            true,
            false,
            false,
            base.cursor_style,
            cursor_color,
        ));
        let changed = restyle(&viewport, &appearance, Some(&base), false, false);
        assert_eq!(changed.foreground, viewport.foreground);
        assert_eq!(changed.background, viewport.background);
        assert_eq!(changed.cursor.unwrap().color(), cursor_color);
    }

    #[test]
    fn local_theme_restyles_configured_defaults_and_preserves_resolved_inverse_cells() {
        let base = TerminalAppearance::default();
        let appearance = TerminalAppearance {
            foreground: Color::from_packed(0x111111),
            background: Color::from_packed(0x222222),
            cursor_color: Color::from_packed(0x333333),
            ..base.clone()
        };
        let mut viewport = TerminalViewport::blank(1, 1, zz_terminal::SessionStatus::Running);
        viewport.foreground = base.foreground;
        viewport.background = base.background;
        viewport.cursor = Some(Cursor::new(
            0,
            0,
            true,
            true,
            false,
            base.cursor_style,
            base.cursor_color,
        ));
        let inverse = PackedStyle::new(
            base.background,
            base.foreground,
            None,
            0,
            UnderlineStyle::None,
        );
        viewport.dictionary = Arc::new(TerminalDictionary::from_shared(
            vec![inverse].into(),
            Arc::from([0]),
            Arc::from([]),
        ));
        let changed = restyle(&viewport, &appearance, Some(&base), false, false);
        assert_eq!(changed.foreground, appearance.foreground);
        assert_eq!(changed.background, appearance.background);
        assert_eq!(changed.cursor.unwrap().color(), appearance.cursor_color);
        assert_eq!(changed.dictionary.styles[0], inverse);
    }
}
