pub(crate) fn canvas_size(
    css_size: (f64, f64),
    device_pixel_size: Option<(f64, f64)>,
    device_pixel_ratio: f64,
) -> (u32, u32, f32, f32) {
    let (css_width, css_height) = css_size;
    let (device_width, device_height) = device_pixel_size.unwrap_or((
        css_width * device_pixel_ratio,
        css_height * device_pixel_ratio,
    ));
    (
        device_width.round() as u32,
        device_height.round() as u32,
        css_width as f32,
        css_height as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::canvas_size;

    #[test]
    fn fractional_device_pixel_sizes_round_to_the_nearest_pixel() {
        assert_eq!(
            canvas_size((1535.984, 863.992), Some((1919.98, 1079.99)), 1.25),
            (1920, 1080, 1535.984, 863.992)
        );
        for device_pixel_size in [(1919.9999, 1080.0001), (1920.0001, 1079.9999)] {
            let (width, height, _, _) = canvas_size((1280.0, 720.0), Some(device_pixel_size), 1.5);
            assert_eq!((width, height), (1920, 1080));
        }
    }

    #[test]
    fn css_pixel_fallback_scales_by_the_device_pixel_ratio() {
        assert_eq!(
            canvas_size((1535.98, 863.99), None, 1.25),
            (1920, 1080, 1535.98, 863.99)
        );
        assert_eq!(
            canvas_size((800.0, 600.0), None, 1.0),
            (800, 600, 800.0, 600.0)
        );
    }
}
