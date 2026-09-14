use super::*;
use zz_protocol::{MuxOptionKey, MuxOptions};

pub(super) fn file_settings(terminal: &str, mux: &str) -> Vec<Setting> {
    let appearance = TerminalAppearance::default();
    let terminal_rows = [
        ("font-family", "Font family", None, &[][..]),
        ("font-size", "Font size", Some((8.0, 72.0)), &[][..]),
        ("theme", "Theme", None, &[][..]),
        ("foreground", "Text color", None, &[][..]),
        ("background", "Background color", None, &[][..]),
        ("cursor-color", "Cursor color", None, &[][..]),
        (
            "cursor-style",
            "Cursor style",
            None,
            &["block", "bar", "underline", "block_hollow"][..],
        ),
        (
            "cursor-style-blink",
            "Cursor blink",
            None,
            &["on", "off", "terminal"][..],
        ),
        (
            "background-opacity",
            "Background opacity",
            Some((0.0, 1.0)),
            &[][..],
        ),
        (
            "window-padding-x",
            "Horizontal padding",
            Some((0.0, 64.0)),
            &[][..],
        ),
        (
            "window-padding-y",
            "Vertical padding",
            Some((0.0, 64.0)),
            &[][..],
        ),
        (
            "zz-cursor-blink-interval-ms",
            "Cursor blink interval (ms)",
            Some((100.0, 2000.0)),
            &[][..],
        ),
    ];
    let mut rows = Vec::new();
    for (name, title, range, choices) in terminal_rows {
        let key = AppearanceConfigKey::from_config_key(name).unwrap();
        let default = appearance_config_values(&appearance, key)
            .ok()
            .and_then(|values| values.into_iter().next())
            .unwrap_or_default();
        rows.push(row(
            name,
            title,
            "terminal",
            file_options::appearance_value(terminal, key),
            default,
            range,
            choices,
        ));
    }
    let mux_rows = [
        (MuxOptionKey::Prefix, "Prefix", None, &[][..]),
        (
            MuxOptionKey::ModeKeys,
            "Mode keys",
            None,
            &["vi", "emacs"][..],
        ),
        (MuxOptionKey::Mouse, "Mouse", None, &["on", "off"][..]),
        (
            MuxOptionKey::HistoryLimit,
            "History limit",
            Some((0.0, 1_000_000.0)),
            &[][..],
        ),
        (
            MuxOptionKey::SetClipboard,
            "Clipboard",
            None,
            &["on", "external", "off"][..],
        ),
        (
            MuxOptionKey::EscapeTime,
            "Escape time (ms)",
            Some((0.0, 5000.0)),
            &[][..],
        ),
    ];
    let defaults = MuxOptions::default();
    for (key, title, range, choices) in mux_rows {
        rows.push(row(
            key.as_str(),
            title,
            "multiplexer",
            file_options::mux_option_value(mux, key),
            defaults
                .get(key)
                .map(|v| v.value.clone())
                .unwrap_or_default(),
            range,
            choices,
        ));
    }
    rows
}

fn row(
    key: &'static str,
    title: &str,
    section: &'static str,
    value: Option<String>,
    default: String,
    range: Option<(f32, f32)>,
    choices: &[&str],
) -> Setting {
    let normalize = |value: String| {
        let value = if key == "cursor-style-blink" {
            match value.as_str() {
                "true" => "on".to_owned(),
                "false" => "off".to_owned(),
                _ => value,
            }
        } else {
            value
        };
        if range.is_some() {
            let scalar = value.split(',').next().unwrap_or(&value);
            scalar
                .parse::<f64>()
                .ok()
                .map_or(json!(value), |v| json!(v))
        } else {
            json!(value)
        }
    };
    Setting {
        key,
        title: title.to_owned(),
        section,
        overridden: value.is_some(),
        value: normalize(value.unwrap_or_else(|| default.clone())),
        default_value: normalize(default),
        enabled: true,
        control: if range.is_some() {
            "number"
        } else if !choices.is_empty() {
            "choice"
        } else if key.ends_with("color") || matches!(key, "foreground" | "background") {
            "color"
        } else {
            "text"
        },
        range,
        choices: choices
            .iter()
            .map(|v| Choice {
                value: (*v).to_owned(),
                title: (*v).to_owned(),
            })
            .collect(),
    }
}
