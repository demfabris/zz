//! The asset source backing [`IconName`](super::IconName).

use std::borrow::Cow;

use zz_gpui::{AssetSource, Result, SharedString};

use super::glyphs;

/// zz's icons: the set [`glyphs`] draws for any `icons/…` path it names, and
/// the brand marks embedded from `crates/zz-gpui-kit/assets/icons/`. Native
/// debug builds read the marks from disk, so an edited SVG needs no rebuild;
/// wasm builds always embed because the browser has no disk to read.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(svg) = glyphs::load(path) {
            return Ok(Some(Cow::Owned(svg.into_bytes())));
        }

        Ok(Self::get(path).map(|asset| asset.data))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter(|asset| asset.starts_with(path))
            .map(Into::into)
            .collect())
    }
}
