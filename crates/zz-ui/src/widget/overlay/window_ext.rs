use gpui::{App, Entity, Window};

use super::{
    dialog::{AlertDialog, Dialog},
    notification::Notification,
    root::Root,
};

/// Dialog and notification verbs on [`Window`].
pub trait WindowExt: Sized {
    /// Open a dialog. `build` runs on every frame the dialog is up.
    fn open_dialog<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static;

    /// Open an [`AlertDialog`]: a title, a description, and an answer.
    ///
    /// ```ignore
    /// window.open_alert_dialog(cx, |alert, _, _| {
    ///     alert
    ///         .title("Unsaved changes")
    ///         .description("Leaving now discards them.")
    ///         .button_props(DialogButtonProps::default().show_cancel(true))
    ///         .on_ok(|_, _, _| true)
    /// });
    /// ```
    fn open_alert_dialog<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(AlertDialog, &mut Window, &mut App) -> AlertDialog + 'static;

    fn has_active_dialog(&mut self, cx: &mut App) -> bool;

    /// Close the topmost dialog.
    fn close_dialog(&mut self, cx: &mut App);

    fn close_all_dialogs(&mut self, cx: &mut App);

    fn push_notification(&mut self, notification: Notification, cx: &mut App);

    /// Retire the notifications pushed with [`Notification::key`] equal to
    /// `key`. Returns whether any were showing.
    fn dismiss_notification(&mut self, key: &str, cx: &mut App) -> bool;

    fn clear_notifications(&mut self, cx: &mut App);

    /// The live toasts, oldest first.
    fn notifications(&mut self, cx: &mut App) -> Vec<Entity<Notification>>;

    /// The merged selected text across this window's selectable text views.
    fn selected_text(&mut self, cx: &mut App) -> String;

    fn has_text_selection(&mut self, cx: &mut App) -> bool;

    fn clear_text_selection(&mut self, cx: &mut App);

    /// End an in-progress text-selection drag.
    fn end_text_selection(&mut self, cx: &mut App);
}

impl WindowExt for Window {
    fn open_dialog<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
    {
        Root::update(self, cx, move |root, window, cx| {
            root.open_dialog(build, window, cx);
        });
    }

    fn open_alert_dialog<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(AlertDialog, &mut Window, &mut App) -> AlertDialog + 'static,
    {
        self.open_dialog(cx, move |_, window, cx| {
            build(AlertDialog::new(cx), window, cx).into_dialog(cx)
        });
    }

    fn has_active_dialog(&mut self, cx: &mut App) -> bool {
        Root::read(self, cx).has_active_dialog()
    }

    fn close_dialog(&mut self, cx: &mut App) {
        Root::update(self, cx, |root, window, cx| root.close_dialog(window, cx));
    }

    fn close_all_dialogs(&mut self, cx: &mut App) {
        Root::update(self, cx, |root, window, cx| {
            root.close_all_dialogs(window, cx);
        });
    }

    fn push_notification(&mut self, notification: Notification, cx: &mut App) {
        Root::update(self, cx, |root, window, cx| {
            root.push_notification(notification, window, cx);
        });
    }

    fn dismiss_notification(&mut self, key: &str, cx: &mut App) -> bool {
        Root::update(self, cx, |root, window, cx| {
            root.dismiss_notification(key, window, cx)
        })
    }

    fn clear_notifications(&mut self, cx: &mut App) {
        Root::update(self, cx, |root, _, cx| root.clear_notifications(cx));
    }

    fn notifications(&mut self, cx: &mut App) -> Vec<Entity<Notification>> {
        Root::read(self, cx).notifications(cx)
    }

    fn selected_text(&mut self, cx: &mut App) -> String {
        Root::read(self, cx).window_selected_text(cx)
    }

    fn has_text_selection(&mut self, cx: &mut App) -> bool {
        Root::read(self, cx).has_text_selection(cx)
    }

    fn clear_text_selection(&mut self, cx: &mut App) {
        Root::update(self, cx, |root, _, cx| root.clear_text_selection(cx));
    }

    fn end_text_selection(&mut self, cx: &mut App) {
        Root::update(self, cx, |root, _, cx| root.end_text_selection(cx));
    }
}
