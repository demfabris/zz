pub(crate) const CHROMIUM_ZOOM_STEP: f64 = 1.2;

const PAGE_ZOOM_FACTORS: &[f64] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

#[must_use]
pub(crate) fn chromium_zoom_level(factor: f64) -> f64 {
    factor.ln() / CHROMIUM_ZOOM_STEP.ln()
}

#[must_use]
pub(crate) fn next_page_zoom_factor(current: f64, direction: i8) -> f64 {
    const EPSILON: f64 = 0.000_001;
    if direction > 0 {
        PAGE_ZOOM_FACTORS
            .iter()
            .copied()
            .find(|factor| *factor > current + EPSILON)
            .unwrap_or_else(|| *PAGE_ZOOM_FACTORS.last().expect("zoom levels are non-empty"))
    } else {
        PAGE_ZOOM_FACTORS
            .iter()
            .rev()
            .copied()
            .find(|factor| *factor < current - EPSILON)
            .unwrap_or(PAGE_ZOOM_FACTORS[0])
    }
}

#[must_use]
pub(crate) fn sanitized_page_zoom_factor(factor: f64) -> f64 {
    if !factor.is_finite() || factor <= 0.0 {
        return 1.0;
    }
    factor.clamp(
        PAGE_ZOOM_FACTORS[0],
        *PAGE_ZOOM_FACTORS.last().expect("zoom levels are non-empty"),
    )
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "page zoom factors are clamped to a small positive percentage range"
)]
#[must_use]
pub(crate) fn page_zoom_percent(factor: f64) -> u16 {
    (factor * 100.0).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_factor(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn uses_chromes_percentage_steps_and_limits() {
        assert_factor(next_page_zoom_factor(1.0, 1), 1.1);
        assert_factor(next_page_zoom_factor(1.0, -1), 0.9);
        assert_factor(next_page_zoom_factor(5.0, 1), 5.0);
        assert_factor(next_page_zoom_factor(0.25, -1), 0.25);
        assert_eq!(page_zoom_percent(next_page_zoom_factor(1.1, 1)), 125);
        assert_factor(sanitized_page_zoom_factor(f64::NAN), 1.0);
    }

    #[test]
    fn converts_factors_to_cef_zoom_levels() {
        for factor in PAGE_ZOOM_FACTORS {
            let round_trip = CHROMIUM_ZOOM_STEP.powf(chromium_zoom_level(*factor));
            assert!((round_trip - factor).abs() < 1e-12);
        }
    }
}
