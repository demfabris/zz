#[allow(dead_code)]
#[path = "../../../clients/gpui-shared/src/preferences.rs"]
mod preferences;

use preferences::Preferences;
use zz_ui::chrome_palette::ThemeModeSetting;

#[test]
fn preferences_reject_invalid_colors_modes_and_numeric_values() {
    let preferences = Preferences {
        mode: Some("unknown".into()),
        preset_light: Some("unknown".into()),
        preset_dark: Some("unknown".into()),
        colors: [Some("invalid".into()), Some("#112233".into()), None],
        zoom: f32::NAN,
        contrast: f32::INFINITY,
        radius: -20.0,
        shadow_strength: 10.0,
        terminal_font_family: Some(" ".into()),
        terminal_font_scale: f32::NAN,
        ..Preferences::default()
    }
    .sanitized();
    assert_eq!(preferences.theme_mode(), ThemeModeSetting::System);
    assert_eq!(
        (preferences.preset_light, preferences.preset_dark),
        (None, None)
    );
    assert_eq!(preferences.colors, [None, Some("#112233".into()), None]);
    assert_eq!(
        (
            preferences.zoom,
            preferences.contrast,
            preferences.radius,
            preferences.shadow_strength
        ),
        (1.0, 1.0, 0.0, 1.0)
    );
    assert_eq!(preferences.terminal_font_family, None);
    assert_eq!(preferences.terminal_font_scale, 1.0);
}

#[test]
fn pane_preferences_bound_values_and_preserve_fractional_frames() {
    let preferences = Preferences {
        gaps: true,
        pane_background_opacity: -1.0,
        pane_inactive_opacity: 3.0,
        pane_glow_strength: f32::NAN,
        pane_margin: 33.0,
        pane_radius: 13.5,
        pane_border_width: 0.5,
        ..Preferences::default()
    }
    .sanitized();
    assert!(preferences.gaps);
    assert_eq!(preferences.pane_background_opacity, 0.0);
    assert_eq!(preferences.pane_inactive_opacity, 1.0);
    assert_eq!(preferences.pane_glow_strength, 1.0);
    assert_eq!(preferences.pane_margin, 32.0);
    assert_eq!(preferences.pane_radius, 13.5);
    assert_eq!(preferences.pane_border_width, 0.5);
    let restored = Preferences::decode(&serde_json::to_string(&preferences).unwrap()).unwrap();
    assert_eq!(restored, preferences);
}
