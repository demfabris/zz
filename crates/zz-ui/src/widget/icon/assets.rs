//! The asset source backing [`IconName`](super::IconName).

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// zz's embedded SVG icon set, read from `crates/zz-ui/assets/icons/`. Native
/// debug builds read from disk, so an edited SVG needs no rebuild; wasm builds
/// always embed because the browser has no disk to read.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
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
