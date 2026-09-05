use crate::ActiveTheme as _;
use crate::Colorize as _;
use gpui::{
    AnyElement, App, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Stateful, Styled as _, WindowControlArea, div, prelude::FluentBuilder as _, px,
};

pub fn chrome_background(background: Hsla, blur: bool) -> Hsla {
    if blur {
        background.opacity(0.93)
    } else {
        background
    }
}

pub fn app_shell_surface(
    id: impl Into<ElementId>,
    sidebar: impl IntoElement,
    titlebar: Option<AnyElement>,
    workspace: impl IntoElement,
    overlays: impl IntoIterator<Item = AnyElement>,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_col()
        .size_full()
        .overflow_hidden()
        .child(
            div()
                .flex()
                .flex_row()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .child(sidebar)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .overflow_hidden()
                        .children(titlebar)
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .overflow_hidden()
                                .child(workspace),
                        ),
                ),
        )
        .children(overlays)
}

/// Titlebar-height strip above the content column, carrying the client-side
/// window controls at its trailing end and a drag region left of them. Mount
/// only when [`crate::draws_window_controls`] says the buttons are ours.
pub fn app_titlebar_strip(
    id: impl Into<ElementId>,
    controls: impl IntoElement,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .w_full()
        .h(crate::TITLE_BAR_HEIGHT)
        .items_center()
        .child(
            div()
                .flex_1()
                .h_full()
                .min_w_0()
                .window_control_area(WindowControlArea::Drag),
        )
        .child(controls)
}

/// Workspace composition beneath the native title bar. Native callers attach
/// drag handlers and window-corner clipping to the returned element. `content`
/// owns its own background; this draws no workspace tint under it.
pub fn app_workspace_surface(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    overlays: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .size_full()
        .overflow_hidden()
        .text_color(cx.theme().foreground)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(div().flex().flex_1().overflow_hidden().child(content)),
        )
        .children(overlays)
}

pub fn app_connection_state(message: impl IntoElement, cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .bg(cx.theme().background)
        .text_size(crate::rems_from_px(12.0))
        .text_color(cx.theme().foreground.muted())
        .child(message)
}

#[derive(Default)]
pub struct WorkspaceStatusSlots {
    pub session: Option<AnyElement>,
    pub windows: Vec<AnyElement>,
    pub right: Vec<AnyElement>,
    pub titlebar_controls: Option<(AnyElement, Pixels)>,
    pub window_controls: Option<AnyElement>,
}

pub fn workspace_status_bar(
    centered: bool,
    gaps: bool,
    background: Hsla,
    leading_inset: Pixels,
    slots: WorkspaceStatusSlots,
    cx: &App,
) -> Stateful<gpui::Div> {
    use crate::TITLE_BAR_HEIGHT;
    let window_strip = div()
        .flex()
        .flex_1()
        .min_w_0()
        .h(TITLE_BAR_HEIGHT)
        .items_center()
        .gap(px(2.0))
        .px(px(2.0))
        .overflow_hidden()
        .when(centered, gpui::Styled::justify_center)
        .children(slots.windows);
    let content = div()
        .flex()
        .flex_1()
        .min_w_0()
        .h(TITLE_BAR_HEIGHT)
        .items_center()
        .gap(px(6.0))
        .px(px(6.0))
        .window_control_area(WindowControlArea::Drag)
        .children(slots.session)
        .child(window_strip)
        .children(slots.right);
    let leading = div()
        .flex_none()
        .w(leading_inset)
        .h(TITLE_BAR_HEIGHT)
        .window_control_area(WindowControlArea::Drag);
    let titlebar_controls = slots.titlebar_controls.map(|(controls, width)| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .w(width + px(6.0))
            .h(TITLE_BAR_HEIGHT)
            .pr(px(6.0))
            .child(controls)
            .into_any_element()
    });
    let window_controls = slots.window_controls.map(|controls| {
        div()
            .flex_none()
            .h(TITLE_BAR_HEIGHT)
            .child(controls)
            .into_any_element()
    });

    div()
        .id("gui-status-titlebar")
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .w_full()
        .h(TITLE_BAR_HEIGHT)
        .overflow_hidden()
        .bg(background)
        .text_color(cx.theme().foreground)
        .when(!gaps, |bar| {
            bar.border_b_1().border_color(cx.theme().border)
        })
        .child(leading)
        .children(titlebar_controls)
        .child(content)
        .children(window_controls)
}
