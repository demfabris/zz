use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    Anchor, AnyElement, Bounds, Context, ElementId, EventEmitter, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _, Task,
    Window, anchored, canvas, deferred, div, point, prelude::*, px, relative,
};
use web_time::Duration;

use super::{
    key_row::{KEY_HEIGHT, KEY_MIN_WIDTH, KEY_PADDING_X, KeyLook, key_surface},
    press::{Grip, Press, press_listeners},
    sticky::{StickyModifier, StickyModifiers},
};
use crate::{ActiveTheme as _, Colorize as _, Icon, IconName, StyledExt as _, rems_from_px};

const HOLD: Duration = Duration::from_millis(280);
const SLIDE: f32 = 12.0;
const GAP: f32 = 8.0;
const WINDOW_MARGIN: f32 = 8.0;
const CARD_PADDING: f32 = 6.0;
const ITEM_HEIGHT: f32 = 40.0;
const ITEM_GAP: f32 = 2.0;
const LIST_WIDTH: f32 = 216.0;
const CELL_WIDTH: f32 = 124.0;
const CAP_WIDTH: f32 = 26.0;
const HINT: f32 = 3.0;

pub(super) fn popover_above(card: impl IntoElement, window: &Window) -> impl IntoElement {
    let gap = rems_from_px(GAP).to_pixels(window.rem_size());
    div().absolute().top_0().left(relative(0.5)).child(
        deferred(
            anchored()
                .anchor(Anchor::BottomCenter)
                .offset(point(px(0.0), -gap))
                .snap_to_window_with_margin(px(WINDOW_MARGIN))
                .child(card),
        )
        .with_priority(2),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopoverKeyItem {
    pub id: SharedString,
    pub label: SharedString,
    pub cap: Option<SharedString>,
    pub icon: Option<IconName>,
    pub danger: bool,
}

impl PopoverKeyItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            cap: None,
            icon: None,
            danger: false,
        }
    }

    #[must_use]
    pub fn cap(mut self, cap: impl Into<SharedString>) -> Self {
        self.cap = Some(cap.into());
        self
    }

    #[must_use]
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    #[must_use]
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

fn chords(chords: &[(&'static str, &'static str, &'static str)]) -> Vec<PopoverKeyItem> {
    chords
        .iter()
        .map(|(keystroke, cap, label)| PopoverKeyItem::new(*keystroke, *label).cap(*cap))
        .collect()
}

pub fn control_chords() -> Vec<PopoverKeyItem> {
    chords(&[
        ("ctrl-c", "^C", "Interrupt"),
        ("ctrl-d", "^D", "End input"),
        ("ctrl-z", "^Z", "Suspend"),
        ("ctrl-l", "^L", "Clear"),
        ("ctrl-r", "^R", "History"),
        ("ctrl-u", "^U", "Kill line"),
        ("ctrl-w", "^W", "Kill word"),
        ("ctrl-k", "^K", "Kill to end"),
        ("ctrl-a", "^A", "Line start"),
        ("ctrl-e", "^E", "Line end"),
    ])
}

pub fn alt_chords() -> Vec<PopoverKeyItem> {
    chords(&[
        ("alt-b", "M-b", "Word back"),
        ("alt-f", "M-f", "Word ahead"),
        ("alt-d", "M-d", "Kill word"),
        ("alt-.", "M-.", "Last arg"),
    ])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverKeyTap {
    Latch(StickyModifier),
    Open,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopoverKeyEvent {
    Pick(SharedString),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    Held,
    Pinned,
}

#[derive(Clone, Copy, Debug)]
struct Touch {
    origin: Point<Pixels>,
    grip: Grip,
    slid: bool,
}

pub struct PopoverKey {
    label: SharedString,
    tap: PopoverKeyTap,
    columns: u16,
    items: Vec<PopoverKeyItem>,
    footer: Option<PopoverKeyItem>,
    armed: bool,
    touch: Option<Touch>,
    open: Option<Open>,
    hover: Option<usize>,
    slots: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    key: Rc<Cell<Bounds<Pixels>>>,
    hold: Option<Task<()>>,
}

impl EventEmitter<PopoverKeyEvent> for PopoverKey {}

impl PopoverKey {
    pub fn new(label: impl Into<SharedString>, tap: PopoverKeyTap, _: &mut Context<Self>) -> Self {
        Self {
            label: label.into(),
            tap,
            columns: 1,
            items: Vec::new(),
            footer: None,
            armed: false,
            touch: None,
            open: None,
            hover: None,
            slots: Rc::default(),
            key: Rc::default(),
            hold: None,
        }
    }

    #[must_use]
    pub fn items(mut self, items: Vec<PopoverKeyItem>) -> Self {
        self.items = items;
        self
    }

    #[must_use]
    pub fn footer(mut self, footer: PopoverKeyItem) -> Self {
        self.footer = Some(footer);
        self
    }

    #[must_use]
    pub fn columns(mut self, columns: u16) -> Self {
        self.columns = columns.max(1);
        self
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn set_armed(&mut self, armed: bool, cx: &mut Context<Self>) {
        if self.armed != armed {
            self.armed = armed;
            cx.notify();
        }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open.take().is_some() || self.touch.take().is_some() {
            self.hover = None;
            self.hold = None;
            cx.notify();
        }
    }

    fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        let item = self
            .items
            .get(index)
            .or_else(|| self.footer.as_ref().filter(|_| index == self.items.len()));
        if let Some(item) = item {
            cx.emit(PopoverKeyEvent::Pick(item.id.clone()));
        }
        self.open = None;
        self.hover = None;
        cx.notify();
    }

    fn item(&self, index: usize, item: &PopoverKeyItem, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let hovered = self.hover == Some(index);
        let pressed = theme.background.washed(4);
        let color = if hovered {
            theme.background.opaque()
        } else if item.danger {
            theme.danger
        } else {
            theme.foreground
        };
        let muted = if hovered {
            color
        } else {
            theme.foreground.muted()
        };
        let cap = |cap: SharedString| {
            div()
                .flex_none()
                .min_w(rems_from_px(CAP_WIDTH))
                .whitespace_nowrap()
                .font_family(theme.mono_font_family.clone())
                .text_size(rems_from_px(12.0))
                .text_color(muted)
                .child(cap)
        };
        let leading = match (&item.icon, &item.cap) {
            (Some(icon), _) => Some(
                Icon::new(icon.clone())
                    .size(rems_from_px(16.0))
                    .into_any_element(),
            ),
            (None, Some(text)) => Some(cap(text.clone()).into_any_element()),
            (None, None) => None,
        };
        let trailing = item.icon.as_ref().and(item.cap.clone()).map(cap);
        let slots = Rc::clone(&self.slots);
        let selector = format!("popover-key-item-{}", item.id);
        div()
            .id(ElementId::named_usize("popover-key-item", index))
            .debug_selector(move || selector)
            .relative()
            .flex()
            .items_center()
            .gap(rems_from_px(8.0))
            .min_w_0()
            .h(rems_from_px(ITEM_HEIGHT))
            .px(rems_from_px(8.0))
            .rounded(theme.menu_radius())
            .text_color(color)
            .when(hovered, |row| row.bg(theme.foreground))
            .when(!hovered, |row| row.active(move |style| style.bg(pressed)))
            .children(leading)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(item.label.clone()),
            )
            .children(trailing)
            .child(
                canvas(
                    move |bounds, _, _| {
                        if let Some(slot) = slots.borrow_mut().get_mut(index) {
                            *slot = bounds;
                        }
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(move |this, _, _, cx| this.pick(index, cx)))
            .into_any_element()
    }

    fn card(&self, cx: &mut Context<Self>) -> AnyElement {
        let columns = self.columns;
        let width = if columns > 1 {
            CELL_WIDTH * f32::from(columns) + ITEM_GAP * f32::from(columns - 1)
        } else {
            LIST_WIDTH
        };
        {
            let mut slots = self.slots.borrow_mut();
            slots.clear();
            slots.resize(
                self.items.len() + usize::from(self.footer.is_some()),
                Bounds::default(),
            );
        }
        let items: Vec<AnyElement> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| self.item(index, item, cx))
            .collect();
        let footer = self
            .footer
            .as_ref()
            .map(|item| self.item(self.items.len(), item, cx));
        let theme = cx.theme();
        let rule = theme.border();
        div()
            .id("popover-key-card")
            .debug_selector(|| "popover-key-card".to_owned())
            .occlude()
            .flex()
            .flex_col()
            .w(rems_from_px(width + 2.0 * CARD_PADDING))
            .p(rems_from_px(CARD_PADDING))
            .gap(rems_from_px(ITEM_GAP))
            .popover_style(cx)
            .font_family(theme.font_family.clone())
            .text_size(rems_from_px(14.0))
            .line_height(rems_from_px(18.0))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if this.open == Some(Open::Pinned) && !this.key.get().contains(&event.position) {
                    this.close(cx);
                }
            }))
            .child(
                div()
                    .grid()
                    .grid_cols(columns)
                    .gap(rems_from_px(ITEM_GAP))
                    .children(items),
            )
            .when_some(footer, |card, footer| {
                card.child(
                    div()
                        .h(px(1.0))
                        .mx(rems_from_px(8.0))
                        .my(rems_from_px(2.0))
                        .bg(rule),
                )
                .child(footer)
            })
            .into_any_element()
    }
}

impl Press for PopoverKey {
    fn grip(&self) -> Option<Grip> {
        self.touch.map(|touch| touch.grip)
    }

    fn grab(&mut self, at: Point<Pixels>, grip: Grip, cx: &mut Context<Self>) {
        self.touch = Some(Touch {
            origin: at,
            grip,
            slid: false,
        });
        self.hover = None;
        self.hold = self.open.is_none().then(|| {
            cx.spawn(async move |key, cx| {
                cx.background_executor().timer(HOLD).await;
                let _ = key.update(cx, |key, cx| {
                    if key.touch.is_some() && key.open.is_none() {
                        key.open = Some(Open::Held);
                        cx.notify();
                    }
                });
            })
        });
        cx.notify();
    }

    fn track(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(touch) = self.touch.as_mut() else {
            return;
        };
        if !touch.slid && (at - touch.origin).magnitude() > f64::from(SLIDE) {
            touch.slid = true;
            if self.open.is_none() {
                self.open = Some(Open::Held);
                self.hold = None;
                cx.notify();
            }
        }
        let hover = if self.open.is_some() {
            self.slots
                .borrow()
                .iter()
                .position(|slot| slot.contains(&at))
        } else {
            None
        };
        if hover != self.hover {
            self.hover = hover;
            cx.notify();
        }
    }

    fn release(&mut self, cancelled: bool, cx: &mut Context<Self>) {
        let Some(touch) = self.touch.take() else {
            return;
        };
        self.hold = None;
        let hover = self.hover.take();
        if cancelled {
            self.open = None;
        } else {
            match (self.open, hover) {
                (Some(_), Some(index)) => self.pick(index, cx),
                (Some(Open::Held), None) if !touch.slid => self.open = Some(Open::Pinned),
                (Some(_), None) => self.open = None,
                (None, _) => match self.tap {
                    PopoverKeyTap::Latch(modifier) => StickyModifiers::toggle(modifier, cx),
                    PopoverKeyTap::Open => self.open = Some(Open::Pinned),
                },
            }
        }
        cx.notify();
    }
}

impl Render for PopoverKey {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let latched = match self.tap {
            PopoverKeyTap::Latch(modifier) => StickyModifiers::get(cx).is_latched(modifier),
            PopoverKeyTap::Open => false,
        };
        let look = if self.touch.is_some() || self.open.is_some() || self.armed {
            KeyLook::Inverted
        } else if latched {
            KeyLook::Latched
        } else {
            KeyLook::Rest
        };
        let theme = cx.theme();
        let hint = if look == KeyLook::Inverted {
            theme.background.opaque().opacity(0.5)
        } else {
            theme.foreground.opacity(0.35)
        };
        let key = Rc::clone(&self.key);
        let selector = format!("popover-key-{}", self.label);
        let open = self.open.is_some();
        key_surface(div().id(ElementId::Name(self.label.clone())), look, cx)
            .debug_selector(move || selector)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .min_w(rems_from_px(KEY_MIN_WIDTH))
            .h(rems_from_px(KEY_HEIGHT))
            .px(rems_from_px(KEY_PADDING_X + 2.0))
            .whitespace_nowrap()
            .child(self.label.clone())
            .child(
                div()
                    .absolute()
                    .top(rems_from_px(HINT))
                    .right(rems_from_px(HINT))
                    .size(rems_from_px(HINT))
                    .rounded_full()
                    .bg(hint),
            )
            .child(
                canvas(move |bounds, _, _| key.set(bounds), |_, (), _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(press_listeners(cx.entity().downgrade()))
            .when(open, |key| {
                let card = self.card(cx);
                key.child(popover_above(card, window))
            })
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        Entity, Modifiers, Subscription, TestAppContext, TouchDragEvent, TouchPhase,
        VisualTestContext,
    };

    use super::*;

    struct Host {
        key: Entity<PopoverKey>,
        picks: Vec<SharedString>,
        _subscription: Subscription,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .absolute()
                    .bottom(px(20.0))
                    .left(px(200.0))
                    .child(self.key.clone()),
            )
        }
    }

    fn host(tap: PopoverKeyTap, cx: &mut TestAppContext) -> (Entity<Host>, &mut VisualTestContext) {
        cx.update(crate::init);
        let (host, cx) = cx.add_window_view(move |_, cx| {
            let key = cx.new(|cx| {
                PopoverKey::new("ctrl", tap, cx)
                    .items(control_chords())
                    .columns(2)
                    .footer(PopoverKeyItem::new("more", "More").icon(IconName::ChevronRight))
            });
            let subscription = cx.subscribe(&key, |host: &mut Host, _, event, _| {
                let PopoverKeyEvent::Pick(id) = event;
                host.picks.push(id.clone());
            });
            Host {
                key,
                picks: Vec::new(),
                _subscription: subscription,
            }
        });
        redraw(cx);
        (host, cx)
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    fn touch(
        cx: &mut VisualTestContext,
        phase: TouchPhase,
        start: Point<Pixels>,
        at: Point<Pixels>,
    ) {
        cx.simulate_event(TouchDragEvent {
            phase,
            start_position: start,
            position: at,
        });
        redraw(cx);
    }

    fn picks(host: &Entity<Host>, cx: &mut VisualTestContext) -> Vec<SharedString> {
        host.read_with(cx, |host, _| host.picks.clone())
    }

    fn open(host: &Entity<Host>, cx: &mut VisualTestContext) -> bool {
        host.read_with(cx, |host, _| host.key.clone())
            .read_with(cx, |key, _| key.is_open())
    }

    #[gpui::test]
    fn a_tap_latches_and_a_hold_opens_the_card(cx: &mut TestAppContext) {
        let (host, cx) = host(PopoverKeyTap::Latch(StickyModifier::Control), cx);
        let center = cx.debug_bounds("popover-key-ctrl").expect("key").center();

        touch(cx, TouchPhase::Started, center, center);
        touch(cx, TouchPhase::Ended, center, center);
        assert!(cx.update(|_, cx| StickyModifiers::get(cx).control));
        assert!(!open(&host, cx));
        cx.update(|_, cx| StickyModifiers::toggle(StickyModifier::Control, cx));

        touch(cx, TouchPhase::Started, center, center);
        assert!(cx.debug_bounds("popover-key-card").is_none());
        cx.executor().advance_clock(HOLD);
        redraw(cx);
        let card = cx.debug_bounds("popover-key-card").expect("card");
        let key = cx.debug_bounds("popover-key-ctrl").expect("key");
        assert!(card.bottom() <= key.top());
        touch(cx, TouchPhase::Ended, center, center);
        assert!(open(&host, cx));
        assert!(cx.update(|_, cx| StickyModifiers::get(cx).is_empty()));

        let item = cx
            .debug_bounds("popover-key-item-ctrl-z")
            .expect("item")
            .center();
        cx.simulate_click(item, Modifiers::none());
        redraw(cx);
        assert_eq!(picks(&host, cx), ["ctrl-z"]);
        assert!(!open(&host, cx));
    }

    #[gpui::test]
    fn sliding_onto_an_item_picks_it_on_release(cx: &mut TestAppContext) {
        let (host, cx) = host(PopoverKeyTap::Open, cx);
        let center = cx.debug_bounds("popover-key-ctrl").expect("key").center();
        touch(cx, TouchPhase::Started, center, center);
        let above = center - point(px(0.0), px(20.0));
        touch(cx, TouchPhase::Moved, center, above);
        assert!(open(&host, cx));
        let item = cx
            .debug_bounds("popover-key-item-ctrl-c")
            .expect("item")
            .center();
        touch(cx, TouchPhase::Moved, center, item);
        touch(cx, TouchPhase::Ended, center, item);
        assert_eq!(picks(&host, cx), ["ctrl-c"]);
        assert!(!open(&host, cx));

        touch(cx, TouchPhase::Started, center, center);
        touch(cx, TouchPhase::Moved, center, above);
        touch(cx, TouchPhase::Ended, center, above);
        assert!(!open(&host, cx));
        assert_eq!(picks(&host, cx).len(), 1);
    }

    #[gpui::test]
    fn a_pinned_card_closes_on_a_tap_elsewhere_or_on_the_key(cx: &mut TestAppContext) {
        let (host, cx) = host(PopoverKeyTap::Open, cx);
        let center = cx.debug_bounds("popover-key-ctrl").expect("key").center();
        touch(cx, TouchPhase::Started, center, center);
        touch(cx, TouchPhase::Ended, center, center);
        assert!(open(&host, cx));
        let footer = cx
            .debug_bounds("popover-key-item-more")
            .expect("footer")
            .center();
        cx.simulate_click(footer, Modifiers::none());
        redraw(cx);
        assert_eq!(picks(&host, cx), ["more"]);

        touch(cx, TouchPhase::Started, center, center);
        touch(cx, TouchPhase::Ended, center, center);
        assert!(open(&host, cx));
        cx.simulate_click(point(px(5.0), px(5.0)), Modifiers::none());
        redraw(cx);
        assert!(!open(&host, cx));

        touch(cx, TouchPhase::Started, center, center);
        touch(cx, TouchPhase::Ended, center, center);
        touch(cx, TouchPhase::Started, center, center);
        touch(cx, TouchPhase::Ended, center, center);
        assert!(!open(&host, cx));
    }
}
