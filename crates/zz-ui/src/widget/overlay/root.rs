//! The window's outermost view, and the overlay state it owns.

use std::rc::Rc;

use crate::window_border;
use gpui::{
    Anchor, AnyView, App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, StyleRefinement, Styled,
    WeakFocusHandle, Window, div, prelude::FluentBuilder as _,
};

use crate::{
    ActiveTheme as _, StyledExt as _,
    text::{self, SelectionScope, TextSelectionController, WindowTextSelection},
};

use super::{
    actions::{Tab, TabPrev},
    dialog::{ANIMATION_DURATION, Dialog},
    notification::{Notification, NotificationList},
};

pub(super) const CONTEXT: &str = text::ROOT_KEY_CONTEXT;

const MAX_FOCUS_STEPS: usize = 100;

#[derive(Clone)]
struct ActiveDialog {
    focus_handle: FocusHandle,
    previous_focused_handle: Option<WeakFocusHandle>,
    builder: Rc<dyn Fn(Dialog, &mut Window, &mut App) -> Dialog>,
}

/// The window's root view: the app's own view, plus the dialog and notification
/// state the overlay layers render from. Must be the window's first view;
/// [`Root::update`] and both `render_*_layer` helpers downcast `window.root()`.
pub struct Root {
    style: StyleRefinement,
    view: AnyView,
    bordered: bool,
    active_dialogs: Vec<ActiveDialog>,
    notification: Entity<NotificationList>,
    pending_focus_restore: Option<WeakFocusHandle>,
    pub(crate) text_selection: WindowTextSelection,
}

impl Root {
    #[must_use]
    pub fn new(view: impl Into<AnyView>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            style: StyleRefinement::default(),
            view: view.into(),
            bordered: true,
            active_dialogs: Vec::new(),
            notification: cx.new(|_| NotificationList::new()),
            pending_focus_restore: None,
            text_selection: WindowTextSelection::default(),
        }
    }

    /// Draw the Linux client-side window border, default `true`. Pass `false`
    /// for surfaces that must not carry a frame.
    #[must_use]
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    #[must_use]
    pub fn view(&self) -> &AnyView {
        &self.view
    }

    /// Run `f` against this window's root. Panics unless the window's root view
    /// is a [`Root`].
    pub fn update<F, R>(window: &mut Window, cx: &mut App, f: F) -> R
    where
        F: FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    {
        let root = window
            .root::<Self>()
            .flatten()
            .expect("the window's root view must be a zz_ui::Root");

        root.update(cx, |root, cx| f(root, window, cx))
    }

    /// Read this window's root. Panics unless the window's root view is a
    /// [`Root`].
    pub fn read<'a>(window: &'a Window, cx: &'a App) -> &'a Self {
        window
            .root::<Self>()
            .flatten()
            .expect("the window's root view must be a zz_ui::Root")
            .read(cx)
    }

    /// The notification layer, for the host view to mount above its chrome.
    pub fn render_notification_layer(
        window: &mut Window,
        cx: &mut App,
    ) -> Option<impl IntoElement + use<>> {
        let root = window.root::<Self>()??;
        let placement = cx.theme().notification.placement;

        Some(
            div()
                .absolute()
                .when(matches!(placement, Anchor::TopRight), |this| {
                    this.top_0().right_0()
                })
                .when(matches!(placement, Anchor::TopLeft), |this| {
                    this.top_0().left_0()
                })
                .when(matches!(placement, Anchor::TopCenter), |this| {
                    this.top_0().mx_auto()
                })
                .when(matches!(placement, Anchor::BottomRight), |this| {
                    this.bottom_0().right_0()
                })
                .when(matches!(placement, Anchor::BottomLeft), |this| {
                    this.bottom_0().left_0()
                })
                .when(matches!(placement, Anchor::BottomCenter), |this| {
                    this.bottom_0().mx_auto()
                })
                .child(root.read(cx).notification.clone()),
        )
    }

    /// The dialog layer, for the host view to mount above its chrome. `None`
    /// when nothing is open.
    pub fn render_dialog_layer(
        window: &mut Window,
        cx: &mut App,
    ) -> Option<impl IntoElement + use<>> {
        let root = window.root::<Self>()??;
        let active_dialogs = root.read(cx).active_dialogs.clone();
        let topmost = active_dialogs.len().checked_sub(1)?;

        let dialogs = active_dialogs
            .iter()
            .enumerate()
            .map(|(ix, active)| {
                let dialog = Dialog::new(cx);
                let mut dialog = (active.builder)(dialog, window, cx);

                dialog.focus_handle = active.focus_handle.clone();
                dialog.layer_ix = ix;
                dialog.props.overlay_visible = ix == topmost;
                dialog
            })
            .collect::<Vec<_>>();

        Some(div().children(dialogs))
    }

    /// Push a dialog onto the stack and move focus into it.
    pub fn open_dialog<F>(&mut self, build: F, window: &mut Window, cx: &mut Context<Self>)
    where
        F: Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
    {
        let previous_focused_handle = self
            .pending_focus_restore
            .take()
            .or_else(|| window.focused(cx).map(|handle| handle.downgrade()));

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        self.active_dialogs.push(ActiveDialog {
            focus_handle,
            previous_focused_handle,
            builder: Rc::new(build),
        });
        self.text_selection.clear(cx);
        cx.notify();
    }

    /// Close the topmost dialog, restoring focus immediately.
    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.pop_dialog() {
            window.focus(&handle, cx);
        }
        self.text_selection.clear(cx);
        cx.notify();
    }

    pub(super) fn defer_close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.pop_dialog() {
            let depth = self.active_dialogs.len();
            self.pending_focus_restore = Some(handle.downgrade());

            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(ANIMATION_DURATION).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.active_dialogs.len() == depth {
                        window.focus(&handle, cx);
                    }
                    this.pending_focus_restore = None;
                });
            })
            .detach();
        }
        self.text_selection.clear(cx);
        cx.notify();
    }

    /// Close every open dialog, restoring the focus the first one took.
    pub fn close_all_dialogs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self
            .active_dialogs
            .first()
            .and_then(|dialog| dialog.previous_focused_handle.clone());
        self.active_dialogs.clear();

        if let Some(handle) = restore.and_then(|handle| handle.upgrade()) {
            window.focus(&handle, cx);
        }
        self.text_selection.clear(cx);
        cx.notify();
    }

    #[must_use]
    pub fn has_active_dialog(&self) -> bool {
        !self.active_dialogs.is_empty()
    }

    fn top_dialog_focus(&self) -> Option<FocusHandle> {
        self.active_dialogs
            .last()
            .map(|dialog| dialog.focus_handle.clone())
    }

    fn pop_dialog(&mut self) -> Option<FocusHandle> {
        self.active_dialogs
            .pop()
            .and_then(|dialog| dialog.previous_focused_handle)
            .and_then(|handle| handle.upgrade())
    }

    pub fn push_notification(
        &mut self,
        notification: Notification,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notification
            .update(cx, |list, cx| list.push(notification, window, cx));
        cx.notify();
    }

    /// Retire the notifications tagged with `key`, playing their dismiss
    /// animation. Returns whether any were showing.
    pub fn dismiss_notification(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let dismissed = self
            .notification
            .update(cx, |list, cx| list.dismiss_key(key, window, cx));
        if dismissed {
            cx.notify();
        }
        dismissed
    }

    /// Drop every notification, without playing their dismiss animation.
    pub fn clear_notifications(&mut self, cx: &mut Context<Self>) {
        self.notification.update(cx, NotificationList::clear);
        cx.notify();
    }

    /// The live notifications, oldest first.
    #[must_use]
    pub fn notifications(&self, cx: &App) -> Vec<Entity<Notification>> {
        self.notification.read(cx).notifications()
    }

    fn on_action_tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(true, window, cx);
    }

    fn on_action_tab_prev(&mut self, _: &TabPrev, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(false, window, cx);
    }

    fn cycle_focus(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(container) = self.top_dialog_focus() else {
            Self::step_focus(forward, window, cx);
            return;
        };

        let before = window.focused(cx);
        Self::step_focus(forward, window, cx);

        let mut steps = 0;
        while !container.contains_focused(window, cx) && steps < MAX_FOCUS_STEPS {
            Self::step_focus(forward, window, cx);
            steps += 1;

            if window.focused(cx) == before {
                break;
            }
        }
    }

    fn step_focus(forward: bool, window: &mut Window, cx: &mut App) {
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
    }
}

impl Root {
    pub(crate) fn text_selection_scope(&self) -> SelectionScope {
        match self.active_dialogs.len() {
            0 => SelectionScope::Base,
            n => SelectionScope::Dialog(n - 1),
        }
    }

    /// The merged selected text across this window's selectable text views.
    #[must_use]
    pub fn window_selected_text(&self, cx: &App) -> String {
        self.text_selection
            .selected_text(self.text_selection_scope(), cx)
    }

    #[must_use]
    pub fn has_text_selection(&self, cx: &App) -> bool {
        self.text_selection.has_selection(cx)
    }

    /// Clear the window selection and every view-local selection.
    pub fn clear_text_selection(&mut self, cx: &mut App) {
        self.text_selection.clear(cx);
    }

    /// End an in-progress selection drag.
    pub fn end_text_selection(&mut self, cx: &mut App) {
        self.text_selection.end(cx);
    }

    fn on_action_copy(&mut self, _: &text::Copy, _: &mut Window, cx: &mut Context<Self>) {
        let selected_text = self.window_selected_text(cx);
        let text = text::clipboard_selection_text(&selected_text);
        if text.is_empty() {
            cx.propagate();
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    }
}

impl Styled for Root {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(cx.theme().font_size);

        let inner = div()
            .id("root")
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_action_tab))
            .on_action(cx.listener(Self::on_action_tab_prev))
            .on_action(cx.listener(Self::on_action_copy))
            .relative()
            .size_full()
            .font_family(cx.theme().font_family.clone())
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .refine_style(&self.style)
            // Must stay the first child: bubble-phase mouse listeners fire in
            // reverse registration order.
            .child(TextSelectionController)
            .child(self.view.clone());

        if self.bordered {
            window_border().child(inner).into_any_element()
        } else {
            inner.into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Dialog, Root};
    use gpui::{App, AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div};

    struct TestView;

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn noop_dialog(dialog: Dialog, _: &mut Window, _: &mut App) -> Dialog {
        dialog
    }

    #[gpui::test]
    fn dialogs_stack_and_unwind(cx: &mut TestAppContext) {
        cx.update(crate::init);

        let (root, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| TestView);
            Root::new(view, window, cx)
        });

        root.update_in(cx, |root, window, cx| {
            root.open_dialog(noop_dialog, window, cx);
            root.open_dialog(noop_dialog, window, cx);
        });
        assert_eq!(root.read_with(cx, |root, _| root.active_dialogs.len()), 2);

        root.update_in(cx, |root, window, cx| root.close_dialog(window, cx));
        assert_eq!(root.read_with(cx, |root, _| root.active_dialogs.len()), 1);

        root.update_in(cx, |root, window, cx| root.close_all_dialogs(window, cx));
        assert!(root.read_with(cx, |root, _| !root.has_active_dialog()));
    }
}
