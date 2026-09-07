//! Where toasts land: always the workspace window, whichever window raised them.

use gpui::{App, Global, WindowHandle};
use zz_ui::{Root, notification::Notification};

struct ToastHost(WindowHandle<Root>);

impl Global for ToastHost {}

/// Names the window every toast is shown on. Call once, as the workspace window
/// opens.
pub fn set_host(handle: WindowHandle<Root>, cx: &mut App) {
    cx.set_global(ToastHost(handle));
}

/// Shows a toast on the workspace window; a no-op when no host is set.
pub(crate) fn push(notification: Notification, cx: &mut App) {
    let Some(host) = cx.try_global::<ToastHost>().map(|host| host.0) else {
        return;
    };
    cx.defer(move |cx| {
        let _ = host.update(cx, |root, window, cx| {
            root.push_notification(notification, window, cx);
        });
    });
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, Context, Render, TestAppContext, Window, div};
    use zz_ui::WindowExt as _;

    use super::*;

    struct Content;

    impl Render for Content {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn notifications_survive_an_active_host_window_update(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let content = cx.new(|_| Content);
            Root::new(content, window, cx)
        });
        cx.update(|window, cx| set_host(window.window_handle().downcast().unwrap(), cx));
        cx.update(|_, cx| push(Notification::info("Settings feedback"), cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 1);
    }
}
