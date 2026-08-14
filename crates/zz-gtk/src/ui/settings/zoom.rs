//! Transient UI zoom, 50% to 300% in 10% steps.
//!
//! Deliberately not a config key: the desktop's zoom is transient too, so a
//! chord pressed to read one long line does not become a persisted preference.
//!
//! Two mechanisms, because GTK has no single knob for both halves. Chrome text
//! is scaled by an app-level CSS `font-size` on `window`, computed from the
//! desktop's own font size so the family stays the theme's. The terminal grid
//! is scaled by multiplying the point size the daemon resolved, because the
//! pane caches its cell metrics and only recomputes them when the appearance
//! value it was handed actually changes. Driving the terminal through the CSS
//! rule instead would leave those metrics stale and the grid misaligned.

use std::cell::Cell;

const MIN: f32 = 0.5;
const MAX: f32 = 3.0;
const STEP: f32 = 0.1;
const FALLBACK_POINTS: f32 = 11.0;

pub struct UiZoom {
    scale: Cell<f32>,
}

impl Default for UiZoom {
    fn default() -> Self {
        Self {
            scale: Cell::new(1.0),
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

    /// The chrome half, as a CSS rule. Empty at 100% so the default look is
    /// exactly the platform's, with no rule of ours in the cascade.
    pub fn css(&self) -> String {
        let scale = self.scale.get();
        if (scale - 1.0).abs() < f32::EPSILON {
            return String::new();
        }
        format!("window {{ font-size: {:.1}pt; }}\n", base_points() * scale)
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
        assert!(zoom.css().is_empty());
        assert!(!zoom.reset());
    }
}
