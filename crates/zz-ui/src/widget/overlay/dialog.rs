//! Modal dialogs, and the alert built on top of one.

use std::{rc::Rc, time::Duration};

use crate::{cubic_ease, window_paddings};
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, BoxShadow, ClickEvent, Div, FocusHandle,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement, Pixels, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled, Window,
    WindowControlArea, anchored, div, point, prelude::FluentBuilder as _, px, relative, size,
};

use crate::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, TITLE_BAR_HEIGHT,
    button::{Button, ButtonVariant, ButtonVariants as _},
    h_flex,
    scroll::ScrollableElement as _,
    v_flex,
};

use super::{
    Root,
    actions::{CancelDialog, ConfirmDialog},
    window_ext::WindowExt as _,
};
use crate::Colorize as _;

pub(super) const CONTEXT: &str = "ZzDialog";

pub(super) const ANIMATION_DURATION: Duration = Duration::from_millis(250);

pub(super) const CONTENT_PADDING: Pixels = px(12.);
const MIN_HEIGHT: Pixels = px(80.);
const LAYER_OFFSET: f32 = 16.;
pub(super) const DEFAULT_WIDTH: Pixels = px(400.);
pub(super) const TITLE_TEXT_SIZE: Pixels = px(13.);
pub(super) const DESCRIPTION_TEXT_SIZE: Pixels = px(12.);

const OK_LABEL: &str = "OK";
const CANCEL_LABEL: &str = "Cancel";

type ConfirmHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) -> bool>;
type CloseHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The text, variants and callbacks of a dialog's OK and Cancel buttons.
#[derive(Clone)]
pub struct DialogButtonProps {
    ok_text: Option<SharedString>,
    ok_variant: ButtonVariant,
    cancel_text: Option<SharedString>,
    show_cancel: bool,
    on_ok: ConfirmHandler,
    on_cancel: ConfirmHandler,
    on_close: CloseHandler,
}

impl Default for DialogButtonProps {
    fn default() -> Self {
        Self {
            ok_text: None,
            ok_variant: ButtonVariant::Primary,
            cancel_text: None,
            show_cancel: false,
            on_ok: Rc::new(|_, _, _| true),
            on_cancel: Rc::new(|_, _, _| true),
            on_close: Rc::new(|_, _, _| {}),
        }
    }
}

impl DialogButtonProps {
    /// Label for the OK button. Default: `OK`.
    #[must_use]
    pub fn ok_text(mut self, ok_text: impl Into<SharedString>) -> Self {
        self.ok_text = Some(ok_text.into());
        self
    }

    /// Variant for the OK button. Default: [`ButtonVariant::Primary`].
    #[must_use]
    pub fn ok_variant(mut self, ok_variant: ButtonVariant) -> Self {
        self.ok_variant = ok_variant;
        self
    }

    /// Label for the Cancel button. Default: `Cancel`.
    #[must_use]
    pub fn cancel_text(mut self, cancel_text: impl Into<SharedString>) -> Self {
        self.cancel_text = Some(cancel_text.into());
        self
    }

    /// Show the Cancel button. Default: `false`.
    #[must_use]
    pub fn show_cancel(mut self, show_cancel: bool) -> Self {
        self.show_cancel = show_cancel;
        self
    }

    /// Called when the dialog is confirmed. Return `true` to close it.
    #[must_use]
    pub fn on_ok(
        mut self,
        on_ok: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_ok = Rc::new(on_ok);
        self
    }

    /// Called when the dialog is cancelled. Return `true` to close it.
    #[must_use]
    pub fn on_cancel(
        mut self,
        on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_cancel = Rc::new(on_cancel);
        self
    }

    fn render_ok(&self) -> AnyElement {
        let on_ok = self.on_ok.clone();
        let on_close = self.on_close.clone();
        let label = self.ok_text.clone().unwrap_or_else(|| OK_LABEL.into());

        Button::new("ok")
            .label(label)
            .small()
            .with_variant(self.ok_variant)
            .on_click(move |_, window, cx| {
                if on_ok(&ClickEvent::default(), window, cx) {
                    window.close_dialog(cx);
                    on_close(&ClickEvent::default(), window, cx);
                }
            })
            .into_any_element()
    }

    fn render_cancel(&self) -> AnyElement {
        let on_cancel = self.on_cancel.clone();
        let on_close = self.on_close.clone();
        let label = self
            .cancel_text
            .clone()
            .unwrap_or_else(|| CANCEL_LABEL.into());

        Button::new("cancel")
            .label(label)
            .small()
            .on_click(move |_, window, cx| {
                if on_cancel(&ClickEvent::default(), window, cx) {
                    window.close_dialog(cx);
                    on_close(&ClickEvent::default(), window, cx);
                }
            })
            .into_any_element()
    }
}

#[derive(Clone)]
pub(crate) struct DialogProps {
    width: Pixels,
    close_button: bool,
    overlay_closable: bool,
    pub(crate) overlay_visible: bool,
    keyboard: bool,
}

impl Default for DialogProps {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            close_button: true,
            overlay_closable: true,
            overlay_visible: false,
            keyboard: true,
        }
    }
}

/// A modal box centered over a scrim. Opened with
/// [`crate::WindowExt::open_dialog`]; most callers want [`AlertDialog`].
#[derive(IntoElement)]
pub struct Dialog {
    style: StyleRefinement,
    children: Vec<AnyElement>,
    title: Option<AnyElement>,
    header: Option<AnyElement>,
    footer: Option<AnyElement>,
    button_props: DialogButtonProps,
    show_button_row: bool,
    a11y_role: Role,
    pub(crate) props: DialogProps,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) layer_ix: usize,
}

impl Dialog {
    /// A dialog with a close button, an overlay that closes it, and escape /
    /// enter bound to cancel / confirm.
    #[must_use]
    pub fn new(cx: &mut App) -> Self {
        Self {
            style: StyleRefinement::default(),
            children: Vec::new(),
            title: None,
            header: None,
            footer: None,
            button_props: DialogButtonProps::default(),
            show_button_row: false,
            a11y_role: Role::Dialog,
            props: DialogProps::default(),
            focus_handle: cx.focus_handle(),
            layer_ix: 0,
        }
    }

    #[must_use]
    pub fn title(mut self, title: impl IntoElement) -> Self {
        self.title = Some(title.into_any_element());
        self
    }

    /// Set the footer. Doing so replaces the OK/Cancel row that
    /// [`Self::button_props`] would render.
    #[must_use]
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Set the OK/Cancel button configuration, and show the action row. An
    /// explicit [`Self::footer`] still wins.
    #[must_use]
    pub fn button_props(mut self, button_props: DialogButtonProps) -> Self {
        self.button_props = button_props;
        self.show_button_row = true;
        self
    }

    /// Called after the dialog closes, whichever way it closed.
    #[must_use]
    pub fn on_close(
        mut self,
        on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.button_props.on_close = Rc::new(on_close);
        self
    }

    /// Called when the dialog is confirmed. Return `true` to close it.
    #[must_use]
    pub fn on_ok(
        mut self,
        on_ok: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.button_props = self.button_props.on_ok(on_ok);
        self.show_button_row = true;
        self
    }

    /// Called when the dialog is cancelled. Return `true` to close it.
    #[must_use]
    pub fn on_cancel(
        mut self,
        on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.button_props = self.button_props.on_cancel(on_cancel);
        self.show_button_row = true;
        self
    }

    /// Show the corner close button. Default: `true`.
    #[must_use]
    pub fn close_button(mut self, close_button: bool) -> Self {
        self.props.close_button = close_button;
        self
    }

    /// Close the dialog when the scrim is clicked. Default: `true`.
    #[must_use]
    pub fn overlay_closable(mut self, overlay_closable: bool) -> Self {
        self.props.overlay_closable = overlay_closable;
        self
    }

    /// Bind escape to cancel and enter to confirm. Default: `true`.
    #[must_use]
    pub fn keyboard(mut self, keyboard: bool) -> Self {
        self.props.keyboard = keyboard;
        self
    }

    /// Set the width. Default: 400px.
    #[must_use]
    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.props.width = width.into();
        self
    }

    #[must_use]
    fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    #[must_use]
    fn alert_role(mut self) -> Self {
        self.a11y_role = Role::AlertDialog;
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Dialog {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let layer_ix = self.layer_ix;
        let on_ok = self.button_props.on_ok.clone();
        let on_cancel = self.button_props.on_cancel.clone();
        let on_close = self.button_props.on_close.clone();
        let button_props = self.button_props.clone();
        let show_default_footer = self.show_button_row && self.footer.is_none();
        let overlay_closable = self.props.overlay_closable;

        let is_topmost = self.props.overlay_visible;

        let paddings = window_paddings(window);
        let view_size = window.viewport_size()
            - size(
                paddings.left + paddings.right,
                paddings.top + paddings.bottom,
            );
        let x = view_size.width / 2. - self.props.width / 2.;
        #[allow(clippy::cast_precision_loss)]
        let y = view_size.height / 10. + px(layer_ix as f32 * LAYER_OFFSET);

        let animation = Animation::new(ANIMATION_DURATION).with_easing(cubic_ease(0.72, 1.));
        let shadow_color = cx.theme().scrim;

        let body = v_flex()
            .id(layer_ix)
            .role(self.a11y_role)
            .track_focus(&self.focus_handle)
            .bg(cx.theme().background.opaque())
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .min_h(MIN_HEIGHT)
            .pt(CONTENT_PADDING)
            .pb(CONTENT_PADDING)
            .gap(CONTENT_PADDING)
            .refine_style(&self.style)
            .px_0()
            .key_context(CONTEXT)
            .when(self.props.keyboard, |this| {
                this.on_action({
                    let on_cancel = on_cancel.clone();
                    let on_close = on_close.clone();
                    move |_: &CancelDialog, window, cx| {
                        if on_cancel(&ClickEvent::default(), window, cx) {
                            window.close_dialog(cx);
                            on_close(&ClickEvent::default(), window, cx);
                        }
                    }
                })
                .on_action({
                    let on_ok = on_ok.clone();
                    let on_close = on_close.clone();
                    move |_: &ConfirmDialog, window, cx| {
                        if on_ok(&ClickEvent::default(), window, cx) {
                            Root::update(window, cx, |root, window, cx| {
                                root.defer_close_dialog(window, cx);
                            });
                            on_close(&ClickEvent::default(), window, cx);
                        }
                    }
                })
            })
            .absolute()
            .occlude()
            .relative()
            .left(x)
            .top(y)
            .w(self.props.width)
            .child(
                v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .gap_y_2()
                    .when_some(self.header, |this, header| {
                        this.child(gutter().child(header))
                    })
                    .when_some(self.title, |this, title| {
                        this.child(gutter().child(dialog_title().child(title)))
                    })
                    .when(!self.children.is_empty(), |this| {
                        this.child(
                            div().flex_1().overflow_hidden().child(
                                v_flex()
                                    .size_full()
                                    .overflow_y_scrollbar()
                                    .pl(CONTENT_PADDING)
                                    .pr(CONTENT_PADDING)
                                    .children(self.children),
                            ),
                        )
                    }),
            )
            .when_some(self.footer, |this, footer| {
                this.child(gutter().child(footer))
            })
            .when(show_default_footer, |this| {
                this.child(
                    gutter().child(
                        dialog_footer(cx)
                            .when(button_props.show_cancel, |this| {
                                this.child(button_props.render_cancel())
                            })
                            .child(button_props.render_ok()),
                    ),
                )
            })
            .children(self.props.close_button.then(|| {
                let on_cancel = on_cancel.clone();
                let on_close = on_close.clone();
                Button::new("close")
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .small()
                    .ghost()
                    .icon(IconName::Close)
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        on_cancel(&ClickEvent::default(), window, cx);
                        on_close(&ClickEvent::default(), window, cx);
                    })
            }))
            .with_animation("slide-down", animation.clone(), move |this, delta| {
                let shadow = shadow_color.opacity(shadow_color.a * delta);
                this.top(y * delta).shadow(vec![
                    BoxShadow {
                        color: shadow,
                        offset: point(px(0.), px(20.)),
                        blur_radius: px(25.),
                        spread_radius: px(-5.),
                        inset: false,
                    },
                    BoxShadow {
                        color: shadow,
                        offset: point(px(0.), px(8.)),
                        blur_radius: px(10.),
                        spread_radius: px(-6.),
                        inset: false,
                    },
                ])
            });

        anchored()
            .position(point(paddings.left, paddings.top))
            .snap_to_window()
            .child(
                div()
                    .id("dialog")
                    .occlude()
                    .w(view_size.width)
                    .h(view_size.height)
                    .when(is_topmost, |this| {
                        this.bg(cx.theme().scrim)
                            .window_control_area(WindowControlArea::Drag)
                            .on_any_mouse_down(move |event, window, cx| {
                                if event.position.y < TITLE_BAR_HEIGHT {
                                    return;
                                }

                                cx.stop_propagation();
                                if overlay_closable
                                    && event.button == MouseButton::Left
                                    && on_cancel(&ClickEvent::default(), window, cx)
                                {
                                    on_close(&ClickEvent::default(), window, cx);
                                    window.close_dialog(cx);
                                }
                            })
                    })
                    .child(body)
                    .with_animation("fade-in", animation, |this, delta| this.opacity(delta)),
            )
    }
}

fn gutter() -> Div {
    div().pl(CONTENT_PADDING).pr(CONTENT_PADDING)
}

fn dialog_title() -> Div {
    div()
        .text_size(TITLE_TEXT_SIZE)
        .font_semibold()
        .line_height(relative(1.))
}

pub(crate) fn dialog_description(cx: &App) -> Div {
    div()
        .text_size(DESCRIPTION_TEXT_SIZE)
        .text_color(cx.theme().foreground.muted())
}

fn dialog_footer(cx: &App) -> Div {
    h_flex()
        .gap_2()
        .justify_end()
        .line_height(relative(1.))
        .rounded_b(cx.theme().radius)
}

/// A modal that interrupts with one question and expects an answer. An optional
/// icon, a title, a description, and the OK/Cancel row from its
/// [`DialogButtonProps`]. Open it with [`crate::WindowExt::open_alert_dialog`].
pub struct AlertDialog {
    base: Dialog,
    icon: Option<AnyElement>,
    title: Option<AnyElement>,
    description: Option<AnyElement>,
    button_props: DialogButtonProps,
}

impl AlertDialog {
    /// An alert with an OK button, no Cancel, no close button, and a scrim that
    /// does not dismiss it.
    #[must_use]
    pub fn new(cx: &mut App) -> Self {
        Self {
            base: Dialog::new(cx).overlay_closable(false).close_button(false),
            icon: None,
            title: None,
            description: None,
            button_props: DialogButtonProps::default(),
        }
    }

    /// Leading icon, rendered to the left of the title block.
    #[must_use]
    pub fn icon(mut self, icon: impl IntoElement) -> Self {
        self.icon = Some(icon.into_any_element());
        self
    }

    /// The question, in bold.
    #[must_use]
    pub fn title(mut self, title: impl IntoElement) -> Self {
        self.title = Some(title.into_any_element());
        self
    }

    /// The consequences, in muted body text.
    #[must_use]
    pub fn description(mut self, description: impl IntoElement) -> Self {
        self.description = Some(description.into_any_element());
        self
    }

    #[must_use]
    pub fn button_props(mut self, button_props: DialogButtonProps) -> Self {
        self.button_props = button_props;
        self
    }

    /// Called when the alert is confirmed. Return `true` to close it.
    #[must_use]
    pub fn on_ok(
        mut self,
        on_ok: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.button_props = self.button_props.on_ok(on_ok);
        self
    }

    /// Called when the alert is cancelled. Return `true` to close it.
    #[must_use]
    pub fn on_cancel(
        mut self,
        on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.button_props = self.button_props.on_cancel(on_cancel);
        self
    }

    /// Called after the alert closes, whichever way it closed.
    #[must_use]
    pub fn on_close(
        mut self,
        on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.button_props.on_close = Rc::new(on_close);
        self
    }

    pub(super) fn into_dialog(self, cx: &App) -> Dialog {
        let props = self.button_props;
        let has_header = self.icon.is_some() || self.title.is_some() || self.description.is_some();

        self.base
            .button_props(props.clone())
            .alert_role()
            .when(has_header, |this| {
                this.header(
                    h_flex()
                        .gap_2()
                        .items_start()
                        .when_some(self.icon, |row, icon| row.child(icon))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .when_some(self.title, |this, title| {
                                    this.child(dialog_title().child(title))
                                })
                                .when_some(self.description, |this, description| {
                                    this.child(dialog_description(cx).child(description))
                                }),
                        ),
                )
            })
            .footer(
                dialog_footer(cx)
                    .when(props.show_cancel, |this| this.child(props.render_cancel()))
                    .child(props.render_ok()),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::{Dialog, DialogButtonProps};

    #[gpui::test]
    fn custom_dialog_actions_enable_the_visible_default_footer(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(!Dialog::new(cx).show_button_row);
            assert!(
                Dialog::new(cx)
                    .button_props(DialogButtonProps::default().show_cancel(true))
                    .show_button_row
            );
            assert!(Dialog::new(cx).on_ok(|_, _, _| false).show_button_row);
        });
    }
}
