//! Pasted images on their way into an ACP prompt.

use std::{borrow::Cow, io::Cursor, sync::Arc};

use gpui::{Image, ImageFormat};

/// Formats sent as-is: Anthropic's API takes exactly these four.
const NATIVE_FORMATS: [ImageFormat; 4] = [
    ImageFormat::Png,
    ImageFormat::Jpeg,
    ImageFormat::Gif,
    ImageFormat::Webp,
];

/// Long edge past which the image is scaled down: Claude resizes anything
/// larger before it reaches the model.
const MAX_EDGE: u32 = 1568;

/// Ceiling on the encoded payload, matching the per-image API limit.
const MAX_BYTES: usize = 5 * 1024 * 1024;

/// Prepare a clipboard image for a prompt, transcoding or scaling if needed.
///
/// The error is a message for the composer.
pub(crate) fn normalize(image: &Image) -> Result<Arc<Image>, Cow<'static, str>> {
    if image.format == ImageFormat::Svg {
        return Err(Cow::Borrowed("SVG images cannot be sent to an agent"));
    }
    let format = decoder_format(image.format)
        .ok_or_else(|| Cow::from(format!("cannot read {} images", image.format.mime_type())))?;
    let (width, height) = image::ImageReader::with_format(Cursor::new(&image.bytes), format)
        .into_dimensions()
        .map_err(|error| Cow::from(format!("could not read the pasted image: {error}")))?;

    let oversized = width.max(height) > MAX_EDGE;
    if !oversized && NATIVE_FORMATS.contains(&image.format) && image.bytes.len() <= MAX_BYTES {
        return Ok(Arc::new(image.clone()));
    }

    let decoded = image::load_from_memory_with_format(&image.bytes, format)
        .map_err(|error| Cow::from(format!("could not decode the pasted image: {error}")))?;
    let decoded = if oversized {
        decoded.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::CatmullRom)
    } else {
        decoded
    };
    let mut png = Cursor::new(Vec::new());
    decoded
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| Cow::from(format!("could not re-encode the pasted image: {error}")))?;
    let png = png.into_inner();
    if png.len() > MAX_BYTES {
        return Err(Cow::from(format!(
            "image is too large to send ({} MB after scaling)",
            png.len() / (1024 * 1024)
        )));
    }
    Ok(Arc::new(Image::from_bytes(ImageFormat::Png, png)))
}

const fn decoder_format(format: ImageFormat) -> Option<image::ImageFormat> {
    Some(match format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Webp => image::ImageFormat::WebP,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        ImageFormat::Tiff => image::ImageFormat::Tiff,
        ImageFormat::Ico => image::ImageFormat::Ico,
        ImageFormat::Pnm => image::ImageFormat::Pnm,
        ImageFormat::Svg => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let buffer = image::RgbaImage::from_fn(width, height, |x, y| {
            let channel = |value: u32| u8::try_from(value % 256).unwrap_or(0xff);
            image::Rgba([channel(x), channel(y), 0x40, 0xff])
        });
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut bytes, format)
            .expect("fixture should encode");
        bytes.into_inner()
    }

    #[test]
    fn a_small_png_is_sent_untouched() {
        let bytes = encode(32, 24, image::ImageFormat::Png);
        let pasted = Image::from_bytes(ImageFormat::Png, bytes.clone());
        let normalized = normalize(&pasted).expect("a small PNG should pass through");

        assert_eq!(normalized.format, ImageFormat::Png);
        assert_eq!(normalized.bytes, bytes);
    }

    #[test]
    fn a_tiff_becomes_a_png_agents_can_read() {
        let pasted = Image::from_bytes(ImageFormat::Tiff, encode(20, 10, image::ImageFormat::Tiff));
        let normalized = normalize(&pasted).expect("TIFF should transcode");

        assert_eq!(normalized.format, ImageFormat::Png);
        assert_eq!(normalized.format.mime_type(), "image/png");
        let decoded = image::ImageReader::with_format(
            Cursor::new(&normalized.bytes),
            image::ImageFormat::Png,
        )
        .into_dimensions()
        .expect("transcoded PNG should decode");
        assert_eq!(decoded, (20, 10));
    }

    #[test]
    fn an_oversized_screenshot_is_scaled_to_the_long_edge() {
        let pasted = Image::from_bytes(
            ImageFormat::Png,
            encode(MAX_EDGE * 2, MAX_EDGE, image::ImageFormat::Png),
        );
        let normalized = normalize(&pasted).expect("an oversized PNG should scale");

        let decoded = image::ImageReader::with_format(
            Cursor::new(&normalized.bytes),
            image::ImageFormat::Png,
        )
        .into_dimensions()
        .expect("scaled PNG should decode");
        assert_eq!(
            decoded,
            (MAX_EDGE, MAX_EDGE / 2),
            "the aspect ratio should survive"
        );
    }

    #[test]
    fn svg_is_refused_with_a_reason() {
        let pasted = Image::from_bytes(ImageFormat::Svg, b"<svg/>".to_vec());
        assert!(normalize(&pasted).is_err());
    }
}
