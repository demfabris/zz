use std::sync::{Arc, LazyLock};

use gpui::{App, RenderImage, WindowAppearance};
use image::{Frame, RgbaImage, imageops::FilterType};
use smallvec::smallvec;
use zz_ui::ThemeMode;

const APP_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/linux/hicolor/256x256/apps/zz.png"
));

const APP_ICON_LIGHT_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/zz-light-512.png"
));
const APP_ICON_DARK_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/zz-dark-512.png"
));

const SETTINGS_PREVIEW_RASTER_SIZE: u32 = 96;

const ABOUT_LOGO_RASTER_SIZE: u32 = 176;

/// What `app-icon` selects.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppIconSetting {
    /// Follow the OS appearance.
    #[default]
    Automatic,
    Light,
    Dark,
}

impl AppIconSetting {
    pub(crate) const ALL: [Self; 3] = [Self::Automatic, Self::Light, Self::Dark];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|setting| setting.as_str() == value.trim())
    }

    pub(crate) fn variant(self, appearance: WindowAppearance) -> ThemeMode {
        match self {
            Self::Automatic => ThemeMode::from(appearance),
            Self::Light => ThemeMode::Light,
            Self::Dark => ThemeMode::Dark,
        }
    }
}

fn decode_png(png: &[u8]) -> RgbaImage {
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .expect("embedded zz icon must be a valid PNG")
        .into_rgba8()
}

fn icon_pixels(variant: ThemeMode) -> &'static RgbaImage {
    static PIXELS: LazyLock<[RgbaImage; 2]> = LazyLock::new(|| {
        [
            decode_png(APP_ICON_LIGHT_PNG),
            decode_png(APP_ICON_DARK_PNG),
        ]
    });

    &PIXELS[usize::from(variant.is_dark())]
}

fn render_image(pixels: RgbaImage) -> Arc<RenderImage> {
    let mut image = pixels;
    for pixel in image.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(smallvec![Frame::new(image)]))
}

fn resize_preview(pixels: &RgbaImage, size: u32) -> RgbaImage {
    let mut premultiplied = pixels.clone();
    for pixel in premultiplied.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }

    let mut resized = image::imageops::resize(&premultiplied, size, size, FilterType::Lanczos3);
    for pixel in resized.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
    resized
}

pub(crate) fn icon_preview(variant: ThemeMode) -> Arc<RenderImage> {
    static PREVIEWS: LazyLock<[Arc<RenderImage>; 2]> = LazyLock::new(|| {
        [ThemeMode::Light, ThemeMode::Dark].map(|variant| {
            render_image(resize_preview(
                icon_pixels(variant),
                SETTINGS_PREVIEW_RASTER_SIZE,
            ))
        })
    });

    Arc::clone(&PREVIEWS[usize::from(variant.is_dark())])
}

pub(crate) fn about_logo(variant: ThemeMode) -> Arc<RenderImage> {
    static LOGOS: LazyLock<[Arc<RenderImage>; 2]> = LazyLock::new(|| {
        [ThemeMode::Light, ThemeMode::Dark].map(|variant| {
            render_image(resize_preview(icon_pixels(variant), ABOUT_LOGO_RASTER_SIZE))
        })
    });

    Arc::clone(&LOGOS[usize::from(variant.is_dark())])
}

pub(crate) fn sidebar_logo() -> Arc<RenderImage> {
    static LOGO: LazyLock<Arc<RenderImage>> =
        LazyLock::new(|| render_image(decode_png(APP_ICON_PNG)));
    Arc::clone(&LOGO)
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn x11_window_icon() -> Arc<RgbaImage> {
    static APP_ICON: LazyLock<Arc<RgbaImage>> =
        LazyLock::new(|| Arc::new(decode_png(APP_ICON_PNG)));
    Arc::clone(&APP_ICON)
}

pub(crate) fn apply(cx: &App) {
    #[cfg(target_os = "macos")]
    {
        let setting = crate::config::app_icon_setting(cx);
        if setting == AppIconSetting::Automatic && macos::bundle_declares_icon() {
            macos::reset_dock_icon();
        } else {
            macos::set_dock_icon(icon_pixels(setting.variant(cx.window_appearance())));
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::io::Cursor;

    use image::{ImageFormat, RgbaImage};
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{NSBundle, NSData, ns_string};

    pub(super) fn bundle_declares_icon() -> bool {
        NSBundle::mainBundle()
            .objectForInfoDictionaryKey(ns_string!("CFBundleIconName"))
            .is_some()
    }

    #[allow(
        unsafe_code,
        reason = "`setApplicationIconImage:` has no safe objc2 binding"
    )]
    pub(super) fn reset_dock_icon() {
        let Some(main_thread) = MainThreadMarker::new() else {
            log::warn!(target: "zz::app_icon", "could not reset the Dock icon outside the main thread");
            return;
        };
        // SAFETY: nil is the documented "use the bundle icon" value, and the
        // marker proves this is the main thread NSApplication requires.
        unsafe {
            NSApplication::sharedApplication(main_thread).setApplicationIconImage(None);
        }
    }

    #[allow(
        unsafe_code,
        reason = "`setApplicationIconImage:` has no safe objc2 binding"
    )]
    pub(super) fn set_dock_icon(icon: &RgbaImage) {
        let Some(main_thread) = MainThreadMarker::new() else {
            log::warn!(target: "zz::app_icon", "could not set the Dock icon outside the main thread");
            return;
        };
        let mut png = Vec::new();
        if let Err(error) = icon.write_to(&mut Cursor::new(&mut png), ImageFormat::Png) {
            log::warn!(target: "zz::app_icon", "could not encode the Dock icon: {error}");
            return;
        }
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png)) else {
            log::warn!(target: "zz::app_icon", "AppKit rejected the encoded Dock icon");
            return;
        };
        // SAFETY: the setter accepts any NSImage, and the marker proves this is
        // the main thread NSApplication requires.
        unsafe {
            NSApplication::sharedApplication(main_thread).setApplicationIconImage(Some(&image));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_is_square_and_large_enough_for_x11() {
        let icon = x11_window_icon();
        assert_eq!(icon.dimensions(), (256, 256));
    }

    #[test]
    fn the_two_dock_renders_are_square_twins_with_different_artwork() {
        let light = icon_pixels(ThemeMode::Light);
        let dark = icon_pixels(ThemeMode::Dark);
        assert_eq!(light.dimensions(), (512, 512));
        assert_eq!(light.dimensions(), dark.dimensions());
        assert_ne!(light.as_raw(), dark.as_raw());
    }

    #[test]
    fn settings_renders_are_prefiltered_at_retina_size() {
        for variant in [ThemeMode::Light, ThemeMode::Dark] {
            for (render, expected) in [
                (icon_preview(variant), SETTINGS_PREVIEW_RASTER_SIZE as i32),
                (about_logo(variant), ABOUT_LOGO_RASTER_SIZE as i32),
            ] {
                assert_eq!(render.size(0).width.0, expected);
                assert_eq!(render.size(0).height.0, expected);
            }
        }
    }

    #[test]
    fn preview_filter_keeps_straight_alpha_edges_bright() {
        let mut source = RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 0]));
        for y in 2..6 {
            for x in 2..6 {
                source.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }

        let resized = resize_preview(&source, SETTINGS_PREVIEW_RASTER_SIZE);
        let translucent = resized
            .pixels()
            .filter(|pixel| pixel[3] > 0 && pixel[3] < 255)
            .collect::<Vec<_>>();
        assert!(!translucent.is_empty());
        assert!(
            translucent
                .iter()
                .all(|pixel| pixel[0] >= 250 && pixel[1] >= 250 && pixel[2] >= 250)
        );
    }

    #[test]
    fn only_automatic_follows_the_os_appearance() {
        for appearance in [
            WindowAppearance::Light,
            WindowAppearance::VibrantLight,
            WindowAppearance::Dark,
            WindowAppearance::VibrantDark,
        ] {
            assert_eq!(AppIconSetting::Light.variant(appearance), ThemeMode::Light);
            assert_eq!(AppIconSetting::Dark.variant(appearance), ThemeMode::Dark);
        }
        assert_eq!(
            AppIconSetting::Automatic.variant(WindowAppearance::VibrantLight),
            ThemeMode::Light
        );
        assert_eq!(
            AppIconSetting::Automatic.variant(WindowAppearance::Dark),
            ThemeMode::Dark
        );
    }

    #[test]
    fn every_setting_round_trips_through_its_config_value() {
        for setting in AppIconSetting::ALL {
            assert_eq!(AppIconSetting::from_str(setting.as_str()), Some(setting));
        }
        assert_eq!(AppIconSetting::from_str("rainbow"), None);
    }
}
