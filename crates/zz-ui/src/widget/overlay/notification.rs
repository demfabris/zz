//! Transient toasts stacked in a corner of the window.

use std::{rc::Rc, time::Duration};

use crate::cubic_ease;
use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, App, AppContext as _, ClickEvent, Context,
    DismissEvent, ElementId, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};

use super::dialog::{CONTENT_PADDING, DEFAULT_WIDTH, DESCRIPTION_TEXT_SIZE, TITLE_TEXT_SIZE};
use crate::Colorize as _;
use crate::{
    ActiveTheme as _, Icon, IconName, Sizable as _, Size, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

const AUTOHIDE_DELAY: Duration = Duration::from_secs(5);
const DISMISS_ANIMATION: Duration = Duration::from_millis(150);
const SLIDE_ANIMATION: Duration = Duration::from_millis(250);
const SLIDE_DISTANCE: f32 = 45.;
const LINE_HEIGHT: Pixels = px(18.);

#[derive(Debug, Clone, Copy)]
enum NotificationType {
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationType {
    fn icon(self, cx: &App) -> Icon {
        match self {
            Self::Info => Icon::new(IconName::Info).text_color(cx.theme().foreground),
            Self::Success => Icon::new(IconName::CircleCheck).text_color(cx.theme().success),
            Self::Warning => Icon::new(IconName::TriangleAlert).text_color(cx.theme().warning),
            Self::Error => Icon::new(IconName::CircleX).text_color(cx.theme().danger),
        }
    }
}

type ContentBuilder =
    Rc<dyn Fn(&mut Notification, &mut Window, &mut Context<Notification>) -> AnyElement>;

/// A single toast. Push one with [`crate::WindowExt::push_notification`].
pub struct Notification {
    type_: Option<NotificationType>,
    key: Option<SharedString>,
    title: Option<SharedString>,
    message: Option<SharedString>,
    content_builder: Option<ContentBuilder>,
    autohide: Option<Duration>,
    closing: bool,
}

impl Notification {
    /// An untyped toast: no icon, no accent color. Give it a
    /// [`Self::message`], a [`Self::title`] or a [`Self::content`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            type_: None,
            key: None,
            title: None,
            message: None,
            content_builder: None,
            autohide: Some(AUTOHIDE_DELAY),
            closing: false,
        }
    }

    #[must_use]
    pub fn info(message: impl Into<SharedString>) -> Self {
        Self::new()
            .message(message)
            .with_type(NotificationType::Info)
    }

    #[must_use]
    pub fn success(message: impl Into<SharedString>) -> Self {
        Self::new()
            .message(message)
            .with_type(NotificationType::Success)
    }

    #[must_use]
    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self::new()
            .message(message)
            .with_type(NotificationType::Warning)
    }

    #[must_use]
    pub fn error(message: impl Into<SharedString>) -> Self {
        Self::new()
            .message(message)
            .with_type(NotificationType::Error)
    }

    /// Tag the toast so a later
    /// [`crate::WindowExt::dismiss_notification`] can retire exactly this one.
    #[must_use]
    pub fn key(mut self, key: impl Into<SharedString>) -> Self {
        self.key = Some(key.into());
        self
    }

    #[must_use]
    pub fn message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Set the bold line above the message.
    #[must_use]
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Keep the toast up until it is dismissed. Default is `true`: auto-hide
    /// after five seconds.
    #[must_use]
    pub fn autohide(mut self, autohide: bool) -> Self {
        self.autohide = autohide.then_some(AUTOHIDE_DELAY);
        self
    }

    #[must_use]
    pub fn autohide_after(mut self, delay: Duration) -> Self {
        self.autohide = Some(delay);
        self
    }

    /// Render arbitrary content below the title and the message.
    #[must_use]
    pub fn content(
        mut self,
        content: impl Fn(&mut Self, &mut Window, &mut Context<Self>) -> AnyElement + 'static,
    ) -> Self {
        self.content_builder = Some(Rc::new(content));
        self
    }

    fn with_type(mut self, type_: NotificationType) -> Self {
        self.type_ = Some(type_);
        self
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        cx.notify();

        cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(DISMISS_ANIMATION).await;
            let _ = view.update_in(cx, |view, _, cx| {
                view.closing = false;
                cx.emit(DismissEvent);
                cx.notify();
            });
        })
        .detach();
    }
}

impl Default for Notification {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitter<DismissEvent> for Notification {}

impl Render for Notification {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self
            .content_builder
            .clone()
            .map(|builder| builder(self, window, cx));
        let icon = self.type_.map(|type_| type_.icon(cx));
        let closing = self.closing;
        let placement = cx.theme().notification.placement;

        h_flex()
            .id("notification")
            .group("")
            .occlude()
            .relative()
            .w(DEFAULT_WIDTH)
            .items_start()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background.raised(1).opaque())
            .rounded(cx.theme().radius)
            .shadow_md()
            .p(CONTENT_PADDING)
            .gap_2()
            .when_some(icon, |this, icon| {
                this.child(
                    h_flex()
                        .h(LINE_HEIGHT)
                        .flex_shrink_0()
                        .debug_selector(|| "notification-icon".to_owned())
                        .child(icon.with_size(Size::Small)),
                )
            })
            .child(
                v_flex()
                    .flex_1()
                    .gap_0p5()
                    .overflow_hidden()
                    .line_height(LINE_HEIGHT)
                    .when_some(self.title.clone(), |this, title| {
                        this.child(
                            div()
                                .text_size(TITLE_TEXT_SIZE)
                                .font_semibold()
                                .debug_selector(|| "notification-title".to_owned())
                                .child(title),
                        )
                    })
                    .when_some(self.message.clone(), |this, message| {
                        this.child(
                            div()
                                .text_size(DESCRIPTION_TEXT_SIZE)
                                .debug_selector(|| "notification-message".to_owned())
                                .child(message),
                        )
                    })
                    .when_some(content, |this, content| this.child(content)),
            )
            .child(
                div()
                    .absolute()
                    .top_1()
                    .right_1()
                    .invisible()
                    .group_hover("", |this| this.visible())
                    .child(
                        Button::new("close")
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.dismiss(window, cx);
                            })),
                    ),
            )
            .on_aux_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                if event.is_middle_click() {
                    this.dismiss(window, cx);
                }
            }))
            .with_animation(
                ElementId::NamedInteger("slide".into(), u64::from(closing)),
                Animation::new(SLIDE_ANIMATION).with_easing(cubic_ease(0., 1.)),
                move |this, delta| slide(this, placement, closing, delta),
            )
    }
}

fn slide<E: Styled + IntoElement>(this: E, placement: Anchor, closing: bool, delta: f32) -> E {
    let opacity = if closing { 1. - delta } else { delta };
    let travel = px(if closing {
        delta * SLIDE_DISTANCE
    } else {
        SLIDE_DISTANCE - delta * SLIDE_DISTANCE
    });

    let this = this
        .opacity(opacity)
        .when(closing || opacity < 0.85, |this| this.shadow_none());

    if closing {
        match placement {
            Anchor::TopRight | Anchor::BottomRight => this.left(travel),
            Anchor::TopLeft | Anchor::BottomLeft => this.left(-travel),
            Anchor::TopCenter => this.top(-travel),
            Anchor::BottomCenter => this.top(travel),
            _ => this,
        }
    } else {
        match placement {
            Anchor::TopLeft | Anchor::TopRight | Anchor::TopCenter => this.top(-travel),
            Anchor::BottomLeft | Anchor::BottomRight | Anchor::BottomCenter => this.top(travel),
            _ => this,
        }
    }
}

struct Entry {
    view: Entity<Notification>,
    _dismissed: Subscription,
}

/// The stack of live toasts for one window.
pub struct NotificationList {
    entries: Vec<Entry>,
}

impl NotificationList {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub(super) fn push(
        &mut self,
        notification: Notification,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let autohide = notification.autohide;
        let view = cx.new(|_| notification);
        let id = view.entity_id();

        let dismissed = cx.subscribe(&view, move |list, _, _: &DismissEvent, cx| {
            list.entries.retain(|entry| entry.view.entity_id() != id);
            cx.notify();
        });

        self.entries.push(Entry {
            view: view.clone(),
            _dismissed: dismissed,
        });

        if let Some(delay) = autohide {
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(delay).await;
                let _ = view.update_in(cx, |note, window, cx| note.dismiss(window, cx));
            })
            .detach();
        }

        cx.notify();
    }

    /// Play the dismiss animation on every toast carrying `key`.
    pub(super) fn dismiss_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let matches = self
            .entries
            .iter()
            .filter(|entry| entry.view.read(cx).key.as_deref() == Some(key))
            .map(|entry| entry.view.clone())
            .collect::<Vec<_>>();
        for view in &matches {
            view.update(cx, |note, cx| note.dismiss(window, cx));
        }
        !matches.is_empty()
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        self.entries.clear();
        cx.notify();
    }

    pub(super) fn notifications(&self) -> Vec<Entity<Notification>> {
        self.entries
            .iter()
            .map(|entry| entry.view.clone())
            .collect()
    }
}

impl Render for NotificationList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = window.viewport_size().height;
        let placement = cx.theme().notification.placement;
        let max_items = cx.theme().notification.max_items;
        let (mt, mb, ml, mr) = {
            let m = &cx.theme().notification.margins;
            (m.top, m.bottom, m.left, m.right)
        };

        let items = self
            .entries
            .iter()
            .rev()
            .take(max_items)
            .rev()
            .map(|entry| entry.view.clone())
            .collect::<Vec<_>>();

        v_flex()
            .id("notification-list")
            .max_h(height)
            .pt(mt)
            .pb(mb)
            .gap_3()
            .when(matches!(placement, Anchor::TopRight), |this| this.pr(mr))
            .when(matches!(placement, Anchor::TopLeft), |this| this.pl(ml))
            .when(matches!(placement, Anchor::BottomRight), |this| {
                this.flex_col_reverse().pr(mr)
            })
            .when(matches!(placement, Anchor::BottomLeft), |this| {
                this.flex_col_reverse().pl(ml)
            })
            .when(matches!(placement, Anchor::BottomCenter), |this| {
                this.flex_col_reverse()
            })
            .children(items)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext};

    use super::Notification;

    fn icon_and_first_line(
        notification: Notification,
        first_line: &'static str,
        cx: &mut TestAppContext,
    ) -> (Bounds<Pixels>, Bounds<Pixels>) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| notification);
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        (
            cx.debug_bounds("notification-icon")
                .expect("a typed toast draws its icon"),
            cx.debug_bounds(first_line)
                .unwrap_or_else(|| panic!("the toast draws its {first_line}")),
        )
    }

    #[gpui::test]
    fn the_icon_centers_on_the_message(cx: &mut TestAppContext) {
        let (icon, message) = icon_and_first_line(
            Notification::warning("session ended"),
            "notification-message",
            cx,
        );
        assert_eq!(icon.center().y, message.center().y);
    }

    #[gpui::test]
    fn a_title_takes_the_icon_off_the_message(cx: &mut TestAppContext) {
        let (icon, title) = icon_and_first_line(
            Notification::error("The host refused the connection").title("Disconnected"),
            "notification-title",
            cx,
        );
        assert_eq!(icon.center().y, title.center().y);
    }
}
