use zz_client::{CHROME_TABLES, ChromeAction, ChromeKey};

/// A `zz/config` chrome override, validated while the file is parsed so the
/// keymap only ever sees chords and actions it can honour.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChromeOverride {
    Bind {
        table: &'static str,
        key: String,
        action: String,
    },
    Unbind {
        table: &'static str,
        key: String,
    },
}

pub fn parse_bind(value: &str) -> Result<ChromeOverride, String> {
    let (target, action) = value
        .rsplit_once('=')
        .ok_or_else(|| "expected `<table>:<key>=<action>`".to_owned())?;
    let (table, key) = parse_target(target)?;
    let action = action.trim();
    if ChromeAction::from_name(action).is_none() {
        return Err(format!("unknown chrome action `{action}`"));
    }
    Ok(ChromeOverride::Bind {
        table,
        key,
        action: action.to_owned(),
    })
}

pub fn parse_unbind(value: &str) -> Result<ChromeOverride, String> {
    let (table, key) = parse_target(value)?;
    Ok(ChromeOverride::Unbind { table, key })
}

fn parse_target(target: &str) -> Result<(&'static str, String), String> {
    let (table, key) = target
        .split_once(':')
        .ok_or_else(|| "expected `<table>:<key>`".to_owned())?;
    let table = table.trim();
    let table = CHROME_TABLES
        .into_iter()
        .find(|known| *known == table)
        .ok_or_else(|| {
            format!(
                "unknown chrome table `{table}`; expected one of {}",
                CHROME_TABLES.join(", "),
            )
        })?;
    let chord = ChromeKey::parse(key)
        .filter(|chord| gpui_source(chord).is_some())
        .ok_or_else(|| format!("`{key}` is not a chord zz can bind"))?;
    Ok((table, chord.to_string()))
}

pub fn gpui_source(key: &ChromeKey) -> Option<String> {
    let mut source = String::new();
    if key.command {
        source.push_str("cmd-");
    }
    if key.control {
        source.push_str("ctrl-");
    }
    if key.alt {
        source.push_str("alt-");
    }
    if key.shift {
        source.push_str("shift-");
    }
    let base = match key.base.as_str() {
        " " => "space",
        named => gpui_key_name(named).unwrap_or(named),
    };
    let mut characters = base.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) if character.is_ascii_uppercase() => {
            if key.shift {
                return None;
            }
            source.push_str("shift-");
            source.extend(character.to_lowercase());
        }
        (Some(_), _) => source.push_str(base),
        (None, _) => return None,
    }
    parse_keystroke(&source).map(|_| source)
}

pub fn gpui_key_name(name: &str) -> Option<&'static str> {
    KEY_NAMES
        .iter()
        .find(|(tmux, _)| *tmux == name)
        .map(|(_, gpui)| *gpui)
}

pub fn tmux_key_name(name: &str) -> Option<&'static str> {
    KEY_NAMES
        .iter()
        .find(|(_, gpui)| *gpui == name)
        .map(|(tmux, _)| *tmux)
}
const KEY_NAMES: [(&str, &str); 26] = [
    ("Enter", "enter"),
    ("Escape", "escape"),
    ("Tab", "tab"),
    ("BSpace", "backspace"),
    ("Up", "up"),
    ("Down", "down"),
    ("Left", "left"),
    ("Right", "right"),
    ("Home", "home"),
    ("End", "end"),
    ("PPage", "pageup"),
    ("NPage", "pagedown"),
    ("DC", "delete"),
    ("IC", "insert"),
    ("F1", "f1"),
    ("F2", "f2"),
    ("F3", "f3"),
    ("F4", "f4"),
    ("F5", "f5"),
    ("F6", "f6"),
    ("F7", "f7"),
    ("F8", "f8"),
    ("F9", "f9"),
    ("F10", "f10"),
    ("F11", "f11"),
    ("F12", "f12"),
];

#[derive(Clone, Debug)]
pub struct ParsedKeystroke {
    pub key: ChromeKey,
    pub function: bool,
}
impl ParsedKeystroke {
    pub fn spelling(&self) -> String {
        let mut value = String::new();
        for (enabled, name) in [
            (self.key.control, "ctrl-"),
            (self.key.alt, "alt-"),
            (self.key.command, "cmd-"),
            (self.key.shift, "shift-"),
            (self.function, "fn-"),
        ] {
            if enabled {
                value.push_str(name);
            }
        }
        value.push_str(&self.key.base);
        value
    }
}

pub fn parse_keystroke(source: &str) -> Option<ParsedKeystroke> {
    let mut chord = ParsedKeystroke {
        key: ChromeKey::default(),
        function: false,
    };
    let mut base = None;
    let mut components = source.split('-').peekable();
    while let Some(component) = components.next() {
        match component.to_ascii_lowercase().as_str() {
            "ctrl" => {
                chord.key.control = true;
                continue;
            }
            "alt" => {
                chord.key.alt = true;
                continue;
            }
            "shift" => {
                chord.key.shift = true;
                continue;
            }
            "fn" => {
                chord.function = true;
                continue;
            }
            "secondary" => {
                if cfg!(target_os = "macos") {
                    chord.key.command = true;
                } else {
                    chord.key.control = true;
                }
                continue;
            }
            "cmd" | "super" | "win" => {
                chord.key.command = true;
                continue;
            }
            _ => {}
        }
        if let Some(next) = components.peek() {
            if next.is_empty() && source.ends_with('-') {
                base = Some("-".to_owned());
                break;
            }
            if next.len() > 1 && next.starts_with('>') {
                base = Some(component.to_owned());
                components.next();
                continue;
            }
            return None;
        }
        if component.len() == 1 && component.as_bytes()[0].is_ascii_uppercase() {
            chord.key.shift = true;
        }
        base = Some(component.to_ascii_lowercase());
    }
    if base.is_none() {
        for (enabled, name) in [
            (&mut chord.key.shift, "shift"),
            (&mut chord.key.control, "control"),
            (&mut chord.key.alt, "alt"),
            (&mut chord.key.command, "platform"),
            (&mut chord.function, "function"),
        ] {
            if std::mem::take(enabled) {
                base = Some(name.to_owned());
                break;
            }
        }
    }
    chord.key.base = base.filter(|key| !key.is_empty())?;
    Some(chord)
}

pub fn configured_keymap(overrides: &[ChromeOverride], hotkey: &str) -> zz_client::ChromeKeymap {
    use zz_client::{BROWSER_TABLE, ChromeKeymap, ChromeProfile};
    let mut keymap = ChromeKeymap::for_profile(ChromeProfile::DESKTOP);
    if let Some(chord) = parse_keystroke(hotkey).filter(|chord| !chord.function) {
        for (bound, action) in keymap.table_bindings(BROWSER_TABLE) {
            if action == ChromeAction::BrowserElementSelector && bound != chord.key {
                keymap.unbind(BROWSER_TABLE, &bound.to_string());
            }
        }
        let _ = keymap.bind(
            BROWSER_TABLE,
            &chord.key.to_string(),
            ChromeAction::BrowserElementSelector.name(),
        );
    } else {
        log::warn!(target: "zz::config", "keeping the built-in element selector chord: `{hotkey}` is not a chord zz can bind");
    }
    for entry in overrides {
        match entry {
            ChromeOverride::Bind { table, key, action } => {
                if let Err(error) = keymap.bind(table, key, action) {
                    log::warn!(target: "zz::config", "ignoring chrome binding for unknown action `{}`", error.0);
                }
            }
            ChromeOverride::Unbind { table, key } => {
                keymap.unbind(table, key);
            }
        }
    }
    keymap
}
