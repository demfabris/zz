use std::borrow::Cow;

use zz_gpui::{App, AssetSource, Result, SharedString};
use zz_ui::ActiveTheme as _;

use crate::glyphs;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(svg) = glyphs::load(path) {
            return Ok(Some(Cow::Owned(svg.into_bytes())));
        }
        match path.strip_prefix("tabler/") {
            Some(icon) => zz_ui::Assets.load(&format!("icons/{icon}")),
            None => zz_ui::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        zz_ui::Assets.list(path)
    }
}

pub fn icon_source(path: &str, cx: &App) -> Option<SharedString> {
    let set = crate::knobs().icons.set()?;
    let name = path.strip_prefix("icons/")?.strip_suffix(".svg")?;
    let theme = cx.theme();
    glyphs::has(set, name).then(|| {
        glyphs::path(
            set,
            name,
            f32::from(theme.radius) * 0.5,
            theme.corner_smoothing,
        )
        .into()
    })
}
