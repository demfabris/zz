use gpui::{App, LayoutId, Style, Window, relative};

pub(crate) mod display;
pub(crate) mod layout;
pub(crate) mod picker;

pub(crate) fn fill_parent(window: &mut Window, cx: &mut App) -> LayoutId {
    let mut style = Style::default();
    style.size.width = relative(1.0).into();
    style.size.height = relative(1.0).into();
    window.request_layout(style, [], cx)
}
