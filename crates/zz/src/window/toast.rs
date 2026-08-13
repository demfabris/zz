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
    let _ = host.update(cx, |root, window, cx| {
        root.push_notification(notification, window, cx);
    });
}
