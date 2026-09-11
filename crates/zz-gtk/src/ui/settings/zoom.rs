//! Transient UI zoom, 50% to 300% in 10% steps.
//!
//! Deliberately not a config key: the desktop's zoom is transient too, so a
//! chord pressed to read one long line does not become a persisted preference.

use std::cell::Cell;

const MIN: f32 = 0.5;
const MAX: f32 = 3.0;
const STEP: f32 = 0.1;
const FALLBACK_POINTS: f32 = 11.0;

pub struct UiZoom {
    scale: Cell<f32>,
    animations_disabled: Cell<bool>,
}

impl Default for UiZoom {
    fn default() -> Self {
        Self {
            scale: Cell::new(1.0),
            animations_disabled: Cell::new(false),
        }
    }
}

impl UiZoom {
    pub fn scale(&self) -> f32 {
        self.scale.get()
    }

    pub fn percent(&self) -> u32 {
        (self.scale.get() * 100.0).round() as u32
    }

    /// True when the scale moved; a step past either end is a no-op rather
    /// than a repaint.
    pub fn step(&self, direction: i32) -> bool {
        self.set(stepped(self.scale.get(), direction))
    }

    pub fn reset(&self) -> bool {
        self.set(1.0)
    }

    fn set(&self, scale: f32) -> bool {
        if (scale - self.scale.get()).abs() < f32::EPSILON {
            return false;
        }
        self.scale.set(scale);
        true
    }

    pub fn css(&self) -> String {
        let config = crate::config::current();
        self.apply_animations(config.animations);
        let scale = self.scale.get();
        let points = if (scale - 1.0).abs() < f32::EPSILON {
            FALLBACK_POINTS
        } else {
            base_points()
        };
        chrome_css(scale, points, config.ui_font_family.as_deref())
    }

    fn apply_animations(&self, enabled: bool) {
        if self.animations_disabled.get() != enabled {
            return;
        }
        if let Some(settings) = gtk::Settings::default() {
            if enabled {
                settings.reset_property("gtk-enable-animations");
            } else {
                settings.set_gtk_enable_animations(false);
            }
            self.animations_disabled.set(!enabled);
        }
    }
}

fn chrome_css(scale: f32, points: f32, family: Option<&str>) -> String {
    let mut declarations = Vec::new();
    if (scale - 1.0).abs() >= f32::EPSILON {
        declarations.push(format!("font-size: {:.1}pt;", points * scale));
    }
    if let Some(family) = family.filter(|family| !family.is_empty() && *family != ".SystemUIFont") {
        let family = family.replace('\\', "\\\\").replace('"', "\\\"");
        declarations.push(format!("font-family: \"{family}\";"));
    }
    if declarations.is_empty() {
        String::new()
    } else {
        format!("window {{ {} }}\n", declarations.join(" "))
    }
}

fn stepped(scale: f32, direction: i32) -> f32 {
    let steps = (scale / STEP).round() + direction as f32;
    (((steps * STEP) * 100.0).round() / 100.0).clamp(MIN, MAX)
}

fn base_points() -> f32 {
    gtk::Settings::default()
        .and_then(|settings| settings.gtk_font_name())
        .and_then(|font| {
            font.rsplit_once(' ')
                .and_then(|(_, size)| size.parse::<f32>().ok())
        })
        .filter(|points| *points > 1.0)
        .unwrap_or(FALLBACK_POINTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_moves_in_whole_tenths() {
        assert_eq!(stepped(1.0, 1), 1.1);
        assert_eq!(stepped(1.0, -1), 0.9);
        assert_eq!(stepped(1.2, 1), 1.3);
        assert_eq!(stepped(0.7, -1), 0.6);
    }

    #[test]
    fn the_range_is_bounded_at_both_ends() {
        let zoom = UiZoom::default();
        for _ in 0..100 {
            zoom.step(-1);
        }
        assert_eq!(zoom.scale(), MIN);
        assert!(!zoom.step(-1));

        for _ in 0..100 {
            zoom.step(1);
        }
        assert_eq!(zoom.scale(), MAX);
        assert!(!zoom.step(1));
    }

    #[test]
    fn resetting_returns_to_a_hundred_percent_and_stops_styling() {
        let zoom = UiZoom::default();
        zoom.step(1);
        assert_eq!(zoom.percent(), 110);

        assert!(zoom.reset());
        assert_eq!(zoom.percent(), 100);
        assert!(chrome_css(zoom.scale(), 11.0, None).is_empty());
        assert!(!zoom.reset());
    }

    #[test]
    fn family_override_survives_reset_and_zoom_remains_transient() {
        let zoom = UiZoom::default();
        assert_eq!(
            chrome_css(zoom.scale(), 11.0, Some("Inter")),
            "window { font-family: \"Inter\"; }\n"
        );
        zoom.step(1);
        assert_eq!(
            chrome_css(zoom.scale(), 11.0, Some("Inter")),
            "window { font-size: 12.1pt; font-family: \"Inter\"; }\n"
        );
        zoom.reset();
        assert_eq!(
            chrome_css(zoom.scale(), 11.0, Some("Inter")),
            "window { font-family: \"Inter\"; }\n"
        );
        assert!(chrome_css(zoom.scale(), 11.0, Some(".SystemUIFont")).is_empty());
    }

    #[test]
    fn font_names_remain_single_css_strings() {
        assert_eq!(
            chrome_css(1.0, 11.0, Some("A\\B\"; color: red;")),
            "window { font-family: \"A\\\\B\\\"; color: red;\"; }\n"
        );
    }
}
