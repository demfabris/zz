use zpui::{App, Div, ParentElement as _, SharedString, Styled as _, div, prelude::*};

use crate::{ActiveTheme as _, StyledExt as _, rems_from_px};

pub fn compact_hud(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .debug_selector(|| "compact-hud".to_owned())
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex_none()
                .px(rems_from_px(16.0))
                .py(rems_from_px(8.0))
                .popover_style(cx)
                .rounded_full()
                .font_family(cx.theme().font_family.clone())
                .text_size(rems_from_px(17.0))
                .line_height(rems_from_px(22.0))
                .font_medium()
                .child(text.into()),
        )
}
