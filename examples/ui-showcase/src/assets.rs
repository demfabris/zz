use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub(crate) const INTER_FONT_FAMILY: &str = "Inter Variable";

pub(crate) fn inter_fonts() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/inter/InterVariable.ttf")),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/inter/InterVariable-Italic.ttf"
        )),
    ]
}

#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
pub(crate) struct ShowcaseAssets;

impl AssetSource for ShowcaseAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        Ok(Self::get(path).map(|asset| asset.data))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter_map(|asset| asset.starts_with(path).then(|| asset.into()))
            .collect())
    }
}
