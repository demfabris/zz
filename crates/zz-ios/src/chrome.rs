//! iPad shell chrome: the status-bar inset, the key strip above the software
//! keyboard, the zoom taps, and the foreground reconnect nudge. The window
//! itself mounts the shared desktop `AppShell`.

use gpui::{
    AnyView, App, Context, Global, InteractiveElement, IntoElement, Keystroke, Modifiers,
    MouseButton, ParentElement, Render, Styled, WeakEntity, Window, div, px,
};
use zz::engine::{IosAccessory, mux::MuxClient, theme::chrome_background, ui_scale::scale_by};
use zz_gpui_ios::{keyboard_inset, take_content_size_scale, take_pinch_scale};

/// Height of the iPad status bar. gpui knows nothing about safe areas, so the
/// shell starts below it.
const STATUS_BAR_INSET: f32 = 24.0;

const STRIP_HEIGHT: f32 = 40.0;

/// How the platform's foreground hook finds the mux without owning it.
pub(crate) struct IosMuxHandle(pub WeakEntity<MuxClient>);

impl Global for IosMuxHandle {}

/// Retry every host the backoff ladder has parked. iOS freezes the timers in
/// background, so the return to foreground is when the ladder should fire.
pub(crate) fn nudge_reconnects(cx: &mut App) {
    let Some(mux) = cx
        .try_global::<IosMuxHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    mux.update(cx, |mux, cx| mux.retry_stalled_hosts(cx));
}

/// The root the iPad window renders: inset, the app shell, the key strip.
pub(crate) struct IosChrome {
    content: AnyView,
}

impl IosChrome {
    pub(crate) fn new(content: AnyView) -> Self {
        Self { content }
    }

    fn strip_key(
        &self,
        label: &'static str,
        key: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        strip_button(label, false).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, window, cx| {
                let sticky = cx.try_global::<IosAccessory>().copied().unwrap_or_default();
                let keystroke = Keystroke {
                    modifiers: Modifiers {
                        control: sticky.ctrl,
                        alt: sticky.alt,
                        ..Modifiers::default()
                    },
                    key: key.to_owned(),
                    key_char: None,
                };
                // A root listener must defer: dispatching synchronously
                // re-renders this view while it is leased, and panics.
                window.defer(cx, move |window, cx| {
                    window.dispatch_keystroke(keystroke, cx);
                });
                cx.set_global(IosAccessory::default());
                cx.notify();
            }),
        )
    }

    fn strip_modifier(
        &self,
        label: &'static str,
        ctrl: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sticky = cx.try_global::<IosAccessory>().copied().unwrap_or_default();
        let active = if ctrl { sticky.ctrl } else { sticky.alt };
        strip_button(label, active).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                let mut sticky = cx.try_global::<IosAccessory>().copied().unwrap_or_default();
                if ctrl {
                    sticky.ctrl = !sticky.ctrl;
                } else {
                    sticky.alt = !sticky.alt;
                }
                cx.set_global(sticky);
                cx.notify();
            }),
        )
    }
}

impl Render for IosChrome {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(scale) = take_pinch_scale() {
            scale_by(scale, cx);
        }
        if let Some(scale) = take_content_size_scale() {
            scale_by(scale, cx);
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(chrome_background(cx))
            .pt(px(STATUS_BAR_INSET))
            .pb(px(keyboard_inset()))
            .child(div().flex_1().min_h(px(0.)).child(self.content.clone()))
            .child(
                div()
                    .h(px(STRIP_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .child(self.strip_key("esc", "escape", cx))
                    .child(self.strip_key("tab", "tab", cx))
                    .child(self.strip_modifier("ctrl", true, cx))
                    .child(self.strip_modifier("alt", false, cx))
                    .child(self.strip_key("←", "left", cx))
                    .child(self.strip_key("↓", "down", cx))
                    .child(self.strip_key("↑", "up", cx))
                    .child(self.strip_key("→", "right", cx)),
            )
    }
}

fn strip_button(label: &'static str, active: bool) -> gpui::Div {
    let overlay = if active { 0.22 } else { 0.08 };
    div()
        .px(px(14.))
        .py(px(5.))
        .rounded(px(8.))
        .bg(gpui::white().opacity(overlay))
        .text_size(px(13.))
        .child(label)
}
