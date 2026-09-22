use serde::{Deserialize, Serialize};
use zz_ui::{
    ThemeMode,
    chrome_palette::{ChromeColor, ChromePresetId, ThemeModeSetting},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub sidebar: bool,
    pub sidebar_width: f32,
    pub status_show_session: bool,
    pub status_badges: bool,
    pub status_agents: bool,
    pub animations: bool,
    pub shadow_strength: f32,
    pub gaps: bool,
    pub agent_enabled: bool,
    pub dark: bool,
    #[serde(default)]
    pub mode: Option<String>,
    pub preset_light: Option<String>,
    pub preset_dark: Option<String>,
    pub colors: [Option<String>; ChromeColor::ALL.len()],
    pub ui_font_family: String,
    pub terminal_font_family: Option<String>,
    pub terminal_font_scale: f32,
    pub zoom: f32,
    pub radius: f32,
    pub contrast: f32,
    pub pane_background_opacity: f32,
    pub pane_inactive_opacity: f32,
    pub pane_glow_strength: f32,
    pub pane_margin: f32,
    pub pane_radius: f32,
    pub pane_border_width: f32,
    pub palette_grouped: bool,
    pub palette_host_prefix: String,
    pub palette_show_keys: bool,
    pub extend_bottom_safe_area: bool,
    pub keep_screen_awake: bool,
    pub system_text_size: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            sidebar: true,
            sidebar_width: zz_ui::navigation::WORKSPACE_SIDEBAR_DEFAULT_WIDTH,
            status_show_session: true,
            status_badges: true,
            status_agents: true,
            animations: true,
            shadow_strength: 1.0,
            gaps: false,
            agent_enabled: true,
            dark: true,
            mode: Some("system".into()),
            preset_light: None,
            preset_dark: None,
            colors: Default::default(),
            ui_font_family: String::new(),
            terminal_font_family: None,
            terminal_font_scale: 1.0,
            zoom: 1.0,
            radius: 6.0,
            contrast: 1.0,
            pane_background_opacity: 0.5,
            pane_inactive_opacity: 0.7,
            pane_glow_strength: 1.0,
            pane_margin: 6.0,
            pane_radius: 13.5,
            pane_border_width: 0.5,
            palette_grouped: true,
            palette_host_prefix: "~".into(),
            palette_show_keys: true,
            extend_bottom_safe_area: false,
            keep_screen_awake: false,
            system_text_size: true,
        }
    }
}

impl Preferences {
    pub fn sanitized(mut self) -> Self {
        self.sidebar_width = bounded(
            self.sidebar_width,
            160.0,
            640.0,
            zz_ui::navigation::WORKSPACE_SIDEBAR_DEFAULT_WIDTH,
        );
        self.shadow_strength = bounded(self.shadow_strength, 0.0, 1.0, 1.0);
        self.zoom = bounded(self.zoom, 0.5, 3.0, 1.0);
        self.terminal_font_scale = bounded(self.terminal_font_scale, 0.5, 3.0, 1.0);
        self.radius = bounded(self.radius, 0.0, 25.0, 6.0);
        self.contrast = bounded(self.contrast, 0.5, 2.0, 1.0);
        for (value, min, max, fallback) in [
            (&mut self.pane_background_opacity, 0., 1., 0.5),
            (&mut self.pane_inactive_opacity, 0., 1., 0.7),
            (&mut self.pane_glow_strength, 0., 2., 1.),
            (&mut self.pane_margin, 0., 32., 6.),
            (&mut self.pane_radius, 0., 32., 13.5),
            (&mut self.pane_border_width, 0., 8., 0.5),
        ] {
            *value = bounded(*value, min, max, fallback);
        }
        self.mode = Some(self.theme_mode().as_str().into());
        self.preset_light = self
            .preset(ThemeMode::Light)
            .filter(|id| !id.dark())
            .map(|id| id.as_str().into());
        self.preset_dark = self
            .preset(ThemeMode::Dark)
            .filter(|id| id.dark())
            .map(|id| id.as_str().into());
        self.colors = self
            .colors
            .map(|value| value.filter(|value| zz_ui::parse_hex(value).is_ok()));
        if self.palette_host_prefix != "#" {
            self.palette_host_prefix = "~".into();
        }
        self.ui_font_family = self.ui_font_family.trim().to_owned();
        self.terminal_font_family = self
            .terminal_font_family
            .map(|family| family.trim().to_owned())
            .filter(|family| !family.is_empty());
        self
    }

    pub fn status_bar_settings(&self) -> zz_client::StatusBarSettings {
        zz_client::StatusBarSettings {
            show_session: self.status_show_session,
            badges: self.status_badges,
            show_agents: self.status_agents,
            show_host: false,
            show_update: false,
        }
    }

    pub fn palette_settings(&self) -> zz_ui::command::PaletteSettings {
        zz_ui::command::PaletteSettings {
            grouped: self.palette_grouped,
            host_prefix: if self.palette_host_prefix == "#" {
                "#"
            } else {
                "~"
            },
            show_keys: self.palette_show_keys,
        }
    }

    pub fn sidebar_width(&self, available_width: f32) -> f32 {
        self.sidebar_width
            .clamp(160.0, (available_width * 0.5).clamp(160.0, 640.0))
    }

    pub fn preset(&self, mode: ThemeMode) -> Option<ChromePresetId> {
        if mode.is_dark() {
            &self.preset_dark
        } else {
            &self.preset_light
        }
        .as_deref()
        .and_then(ChromePresetId::parse)
    }

    pub fn theme_mode(&self) -> ThemeModeSetting {
        self.mode.as_deref().map_or(
            if self.dark {
                ThemeModeSetting::Dark
            } else {
                ThemeModeSetting::Light
            },
            |mode| ThemeModeSetting::parse(mode).unwrap_or(ThemeModeSetting::System),
        )
    }

    #[cfg(any(target_os = "ios", target_family = "wasm", test))]
    pub fn decode(value: &str) -> serde_json::Result<Self> {
        let source: serde_json::Value = serde_json::from_str(value)?;
        let mut preferences: Self = serde_json::from_value(source.clone())?;
        if source.get("mode").is_none() && source.get("dark").is_none() {
            preferences.mode = Some("system".into());
        }
        Ok(preferences.sanitized())
    }

    #[cfg(any(target_os = "ios", test))]
    pub fn load_path(path: &std::path::Path) -> std::io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(value) => Self::decode(&value)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    #[cfg(any(target_os = "ios", test))]
    pub fn save_path(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let value = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, value)?;
        std::fs::rename(temporary, path)
    }

    #[cfg(target_os = "ios")]
    fn path() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var_os("HOME").expect("iOS application home directory"))
            .join("Library/Application Support/zz-gpui/preferences.json")
    }

    pub fn load() -> Self {
        #[cfg(target_family = "wasm")]
        if let Some(value) = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item(Self::STORAGE_KEY).ok().flatten())
            .and_then(|value| Self::decode(&value).ok())
        {
            return value;
        }
        #[cfg(target_os = "ios")]
        {
            Self::load_path(&Self::path()).unwrap_or_default()
        }
        #[cfg(not(target_os = "ios"))]
        Self::default().sanitized()
    }

    pub fn save(&self) {
        #[cfg(target_family = "wasm")]
        if let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
            && let Ok(value) = serde_json::to_string(self)
        {
            let _ = storage.set_item(Self::STORAGE_KEY, &value);
        }
        #[cfg(target_os = "ios")]
        if let Err(error) = self.save_path(&Self::path()) {
            log::warn!("Could not save preferences: {error}");
        }
    }

    #[cfg(target_family = "wasm")]
    const STORAGE_KEY: &str = if zz_protocol::app_identity::DEVELOPMENT {
        "zz-dev-web-preferences"
    } else {
        "zz-web-preferences"
    };
}

fn bounded(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::Preferences;
    use zz_ui::chrome_palette::ThemeModeSetting;

    #[test]
    fn preferences_persist_and_missing_fields_keep_defaults() {
        let directory = std::env::temp_dir().join(format!(
            "zz-ios-preferences-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = directory.join("preferences.json");
        assert_eq!(
            Preferences::load_path(&path).unwrap(),
            Preferences::default()
        );
        let expected = Preferences {
            mode: Some("light".into()),
            animations: false,
            ui_font_family: "Inter Variable".into(),
            terminal_font_family: Some("Lilex".into()),
            terminal_font_scale: 1.25,
            pane_margin: 7.5,
            pane_radius: 10.5,
            pane_border_width: 1.5,
            ..Preferences::default()
        };
        expected.save_path(&path).unwrap();
        assert_eq!(Preferences::load_path(&path).unwrap(), expected);
        std::fs::write(&path, r#"{"mode":"dark"}"#).unwrap();
        assert_eq!(
            Preferences::load_path(&path).unwrap(),
            Preferences {
                mode: Some("dark".into()),
                ..Preferences::default()
            }
        );
        std::fs::write(&path, "broken json").unwrap();
        assert_eq!(
            Preferences::load_path(&path).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fresh_preferences_follow_system_and_legacy_dark_choice_survives() {
        assert_eq!(
            Preferences::decode("{}").unwrap().theme_mode(),
            ThemeModeSetting::System
        );
        assert_eq!(
            Preferences::decode(r#"{"dark":true}"#)
                .unwrap()
                .theme_mode(),
            ThemeModeSetting::Dark
        );
        assert_eq!(
            Preferences::decode(r#"{"dark":false}"#)
                .unwrap()
                .theme_mode(),
            ThemeModeSetting::Light
        );
        assert_eq!(
            Preferences::decode(r#"{"mode":"system","dark":true}"#)
                .unwrap()
                .theme_mode(),
            ThemeModeSetting::System
        );
    }

    #[test]
    fn older_preferences_gain_status_and_appearance_defaults() {
        let defaults = serde_json::from_str::<Preferences>(r#"{"sidebar":false}"#).unwrap();
        assert!(!defaults.sidebar);
        assert!(defaults.animations);
        assert_eq!(defaults.shadow_strength, 1.0);
        assert_eq!(
            defaults.sidebar_width,
            zz_ui::navigation::WORKSPACE_SIDEBAR_DEFAULT_WIDTH
        );
        let status = defaults.status_bar_settings();
        assert!(status.show_session && status.badges && status.show_agents);
        assert!(!status.show_host && !status.show_update);
        let changed = Preferences {
            status_show_session: false,
            status_badges: false,
            status_agents: false,
            animations: false,
            shadow_strength: 0.35,
            sidebar_width: 320.0,
            ..defaults
        };
        let saved = serde_json::to_string(&changed).unwrap();
        let restored = serde_json::from_str::<Preferences>(&saved)
            .unwrap()
            .sanitized();
        assert_eq!(
            restored.status_bar_settings(),
            changed.status_bar_settings()
        );
        assert!(!restored.animations);
        assert_eq!(restored.shadow_strength, 0.35);
        assert_eq!(restored.sidebar_width, 320.0);
    }

    #[test]
    fn sidebar_and_shadow_values_stay_within_supported_limits() {
        for (width, available, expected) in [
            (0.0, 1000.0, 160.0),
            (300.0, 1000.0, 300.0),
            (800.0, 1000.0, 500.0),
            (800.0, 2000.0, 640.0),
            (300.0, 200.0, 160.0),
        ] {
            let preferences = Preferences {
                sidebar_width: width,
                ..Preferences::default()
            }
            .sanitized();
            assert_eq!(preferences.sidebar_width(available), expected);
        }
        let invalid = Preferences {
            sidebar_width: f32::NAN,
            shadow_strength: f32::INFINITY,
            ..Preferences::default()
        }
        .sanitized();
        assert_eq!(invalid.sidebar_width, Preferences::default().sidebar_width);
        assert_eq!(invalid.shadow_strength, 1.0);
        for (value, expected) in [(-0.1, 0.0), (1.5, 1.0), (0.4, 0.4)] {
            assert_eq!(
                Preferences {
                    shadow_strength: value,
                    ..Preferences::default()
                }
                .sanitized()
                .shadow_strength,
                expected
            );
        }
    }

    #[test]
    fn contrast_defaults_clamps_and_round_trips_with_preferences() {
        let defaults = serde_json::from_str::<Preferences>("{}").unwrap();
        assert_eq!(defaults.contrast, 1.0);
        for (value, expected) in [(0.1, 0.5), (3.0, 2.0), (1.5, 1.5), (f32::NAN, 1.0)] {
            let preferences = Preferences {
                contrast: value,
                ..Preferences::default()
            }
            .sanitized();
            assert_eq!(preferences.contrast, expected);
            let saved = serde_json::to_string(&preferences).unwrap();
            assert_eq!(
                serde_json::from_str::<Preferences>(&saved)
                    .unwrap()
                    .contrast,
                expected
            );
        }
    }

    #[test]
    fn full_widget_radius_survives_saved_preferences() {
        let preferences = Preferences {
            radius: 25.0,
            ..Preferences::default()
        };
        let saved = serde_json::to_string(&preferences).unwrap();
        let loaded = serde_json::from_str::<Preferences>(&saved)
            .unwrap()
            .sanitized();
        assert_eq!(loaded.radius, 25.0);
    }

    #[test]
    fn older_preferences_gain_pane_controls_and_values_remain_bounded() {
        let preferences = serde_json::from_str::<Preferences>(r#"{"gaps":true,"radius":8}"#)
            .unwrap()
            .sanitized();
        assert!(preferences.gaps);
        assert_eq!(preferences.radius, 8.0);
        assert_eq!(preferences.pane_margin, 6.0);
        assert_eq!(preferences.pane_radius, 13.5);
        assert_eq!(preferences.pane_background_opacity, 0.5);
        assert_eq!(preferences.pane_glow_strength, 1.0);
        let preferences = Preferences {
            pane_margin: -1.0,
            pane_radius: 200.0,
            pane_border_width: f32::NAN,
            pane_inactive_opacity: 2.0,
            pane_background_opacity: -1.0,
            pane_glow_strength: 3.0,
            ..preferences
        }
        .sanitized();
        assert_eq!(preferences.pane_margin, 0.0);
        assert_eq!(preferences.pane_radius, 32.0);
        assert_eq!(preferences.pane_border_width, 0.5);
        assert_eq!(preferences.pane_inactive_opacity, 1.0);
        assert_eq!(preferences.pane_background_opacity, 0.0);
        assert_eq!(preferences.pane_glow_strength, 2.0);
        for (value, expected) in [(2.0, 1.0), (f32::NAN, 0.5)] {
            assert_eq!(
                Preferences {
                    pane_background_opacity: value,
                    ..Preferences::default()
                }
                .sanitized()
                .pane_background_opacity,
                expected,
            );
        }
    }
}
