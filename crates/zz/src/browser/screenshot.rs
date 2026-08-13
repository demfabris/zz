//! PNG encoding for `capture-browser`.

use std::path::Path;

use image::{ImageBuffer, ImageFormat, Rgba};

const MAX_SCREENSHOT_PIXELS: u64 = 64 * 1024 * 1024;

/// One decoded frame ready to encode: tightly packed RGBA8.
pub(crate) struct Screenshot {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Screenshot {
    /// Build a screenshot from tightly packed premultiplied BGRA.
    pub(crate) fn from_bgra(width: u32, height: u32, bgra: &[u8]) -> Result<Self, String> {
        Self::from_bgra_rows(
            width,
            height,
            usize::try_from(width)
                .unwrap_or(usize::MAX)
                .saturating_mul(4),
            bgra,
        )
    }

    /// Build a screenshot from BGRA whose rows may be padded, as an `IOSurface`'s are.
    pub(crate) fn from_bgra_rows(
        width: u32,
        height: u32,
        stride: usize,
        bgra: &[u8],
    ) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("the browser pane has not rendered a frame yet".to_owned());
        }
        if u64::from(width).saturating_mul(u64::from(height)) > MAX_SCREENSHOT_PIXELS {
            return Err(format!("frame is too large to encode: {width}x{height}"));
        }
        let row_bytes = usize::try_from(width)
            .unwrap_or(usize::MAX)
            .saturating_mul(4);
        if stride < row_bytes {
            return Err("frame stride is narrower than its width".to_owned());
        }
        let rows = usize::try_from(height).unwrap_or(usize::MAX);
        if bgra.len() < stride.saturating_mul(rows.saturating_sub(1)) + row_bytes {
            return Err("frame buffer is shorter than its dimensions".to_owned());
        }
        let mut rgba = vec![0_u8; row_bytes.saturating_mul(rows)];
        for row in 0..rows {
            let source = &bgra[row * stride..row * stride + row_bytes];
            let target = &mut rgba[row * row_bytes..(row + 1) * row_bytes];
            for (pixel, out) in source.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
                out[0] = pixel[2];
                out[1] = pixel[1];
                out[2] = pixel[0];
                out[3] = pixel[3];
            }
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub(crate) fn write_png(self, path: &Path) -> Result<(), String> {
        let buffer: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(self.width, self.height, self.rgba)
                .ok_or_else(|| "could not build an image from the browser frame".to_owned())?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.is_dir()
        {
            return Err(format!("{} is not a directory", parent.display()));
        }
        buffer
            .save_with_format(path, ImageFormat::Png)
            .map_err(|error| format!("could not write {}: {error}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::Screenshot;

    #[test]
    fn bgra_is_swizzled_to_rgba_and_padding_is_dropped() {
        let bgra = [
            0x11, 0x22, 0x33, 0x44, 0xff, 0xff, 0xff, 0xff, //
            0x55, 0x66, 0x77, 0x88, 0x00, 0x00, 0x00, 0x00,
        ];
        let screenshot = Screenshot::from_bgra_rows(1, 2, 8, &bgra).expect("screenshot");
        assert_eq!(
            screenshot.rgba,
            [0x33, 0x22, 0x11, 0x44, 0x77, 0x66, 0x55, 0x88]
        );
    }

    #[test]
    fn short_or_empty_frames_are_refused() {
        assert!(Screenshot::from_bgra(0, 4, &[]).is_err());
        assert!(Screenshot::from_bgra(4, 4, &[0; 8]).is_err());
        assert!(Screenshot::from_bgra_rows(4, 1, 2, &[0; 16]).is_err());
    }

    #[test]
    fn png_round_trips_through_the_image_crate() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("frame.png");
        Screenshot::from_bgra(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255])
            .expect("screenshot")
            .write_png(&path)
            .expect("write png");

        let decoded = image::open(&path).expect("decode png").to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.as_raw()[..4], [3, 2, 1, 255]);
    }
}
