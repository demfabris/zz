use std::rc::Rc;

use zpui::{
    AnyElement, App, Div, ElementId, Entity, IntoElement, Keystroke, Modifiers, MouseButton,
    ParentElement as _, RenderOnce, ScrollHandle, SharedString, Stateful, Styled, Window, div,
    prelude::*,
};

use super::{
    arrow_pad::ArrowPad,
    popover_key::{PopoverKey, PopoverKeyItem, PopoverKeyTap, alt_chords, control_chords},
    sticky::{StickyModifier, StickyModifiers},
};
use crate::{ActiveTheme as _, Colorize as _, Icon, IconName, StyledExt as _, rems_from_px};

pub const KEY_ROW_HEIGHT: f32 = 44.0;
pub(super) const KEY_HEIGHT: f32 = 32.0;

pub(super) const KEY_WIDTH: f32 = 52.0;
const ROW_PADDING_X: f32 = 6.0;
const KEY_GAP: f32 = 3.0;
const KEY_GROUP: &str = "key-row-key";
pub(super) const KEY_SLOP: (f32, f32) = (KEY_GAP / 2.0, (KEY_ROW_HEIGHT - KEY_HEIGHT) / 2.0);
const LATCHED_WASH: u8 = 6;
const PRESSED_WASH: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyLook {
    Rest,
    Latched,
    Inverted,
}

pub(super) fn key_surface<E: Styled>(element: E, look: KeyLook, cx: &App) -> E {
    let theme = cx.theme();
    let element = element
        .h(rems_from_px(KEY_HEIGHT))
        .rounded(theme.control_radius())
        .control_surface(cx);
    match look {
        KeyLook::Rest => element
            .bg(theme.background.raised(2))
            .text_color(theme.foreground),
        KeyLook::Latched => element
            .bg(theme.background.washed(LATCHED_WASH))
            .text_color(theme.foreground),
        KeyLook::Inverted => element
            .bg(theme.foreground)
            .text_color(theme.background.opaque())
            .font_semibold(),
    }
}

#[derive(Clone)]
pub enum KeyRowKey {
    Hide,
    Key {
        label: SharedString,
        keystroke: Keystroke,
    },
    Modifier(StickyModifier),
    Popover(Entity<PopoverKey>),
    Arrows(Entity<ArrowPad>),
}

impl KeyRowKey {
    pub fn text(label: impl Into<SharedString>) -> Self {
        let label = label.into();
        Self::Key {
            keystroke: Keystroke {
                modifiers: Modifiers::default(),
                key: label.to_string(),
                key_char: Some(label.to_string()),
            },
            label,
        }
    }

    pub fn named(label: impl Into<SharedString>, key: impl Into<String>) -> Self {
        Self::Key {
            label: label.into(),
            keystroke: Keystroke {
                modifiers: Modifiers::default(),
                key: key.into(),
                key_char: None,
            },
        }
    }
}

#[derive(Clone)]
pub struct ToolKeys {
    pub arrows: Entity<ArrowPad>,
    pub control: Entity<PopoverKey>,
    pub alt: Entity<PopoverKey>,
    pub prefix: Entity<PopoverKey>,
    pub scroll: ScrollHandle,
}

impl ToolKeys {
    pub fn new(prefix: Vec<PopoverKeyItem>, footer: Option<PopoverKeyItem>, cx: &mut App) -> Self {
        let scroll = ScrollHandle::new();
        let latch = |label: &'static str, modifier, items: Vec<PopoverKeyItem>, cx: &mut App| {
            cx.new(|cx| {
                PopoverKey::new(label, PopoverKeyTap::Latch(modifier), cx)
                    .items(items)
                    .columns(2)
                    .scrolls(scroll.clone())
            })
        };
        Self {
            arrows: cx.new(ArrowPad::new),
            control: latch("ctrl", StickyModifier::Control, control_chords(), cx),
            alt: latch("alt", StickyModifier::Alt, alt_chords(), cx),
            prefix: cx.new(|cx| {
                let key = PopoverKey::new("prefix", PopoverKeyTap::Open, cx)
                    .items(prefix)
                    .scrolls(scroll.clone());
                match footer {
                    Some(footer) => key.footer(footer),
                    None => key,
                }
            }),
            scroll,
        }
    }

    pub fn row(&self) -> Vec<KeyRowKey> {
        vec![
            KeyRowKey::Popover(self.prefix.clone()),
            KeyRowKey::Popover(self.control.clone()),
            KeyRowKey::Popover(self.alt.clone()),
            KeyRowKey::named("tab", "tab"),
            KeyRowKey::named("esc", "escape"),
            KeyRowKey::text("/"),
            KeyRowKey::text("@"),
            KeyRowKey::Arrows(self.arrows.clone()),
            KeyRowKey::Hide,
        ]
    }

    pub fn close(&self, cx: &mut App) {
        for key in [&self.control, &self.alt, &self.prefix] {
            if key.read(cx).is_open() {
                key.update(cx, PopoverKey::close);
            }
        }
    }
}

type KeyHandler = Rc<dyn Fn(&Keystroke, &mut Window, &mut App)>;
type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct KeyRow {
    keys: Vec<KeyRowKey>,
    scroll: Option<ScrollHandle>,
    on_key: Option<KeyHandler>,
    on_hide: Option<Handler>,
}

impl KeyRow {
    pub fn new(keys: Vec<KeyRowKey>) -> Self {
        Self {
            keys,
            scroll: None,
            on_key: None,
            on_hide: None,
        }
    }

    #[must_use]
    pub fn track_scroll(mut self, scroll: &ScrollHandle) -> Self {
        self.scroll = Some(scroll.clone());
        self
    }

    #[must_use]
    pub fn on_key(mut self, f: impl Fn(&Keystroke, &mut Window, &mut App) + 'static) -> Self {
        self.on_key = Some(Rc::new(f));
        self
    }

    #[must_use]
    pub fn on_hide(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_hide = Some(Rc::new(f));
        self
    }

    fn slot(id: impl Into<ElementId>) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .h_full()
            .px(rems_from_px(KEY_SLOP.0))
    }

    fn cap(index: usize, look: KeyLook, content: impl IntoElement, cx: &App) -> Stateful<Div> {
        let selector = format!("key-row-key-{index}");
        Self::slot(ElementId::named_usize("key-row-key", index))
            .group(KEY_GROUP)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(
                key_surface(div().id("cap"), look, cx)
                    .debug_selector(move || selector)
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(rems_from_px(KEY_WIDTH))
                    .whitespace_nowrap()
                    .relative()
                    .child(crate::touch::instant_press_highlight(
                        ElementId::named_usize("key-row-press", index),
                        cx.theme().background.washed(PRESSED_WASH),
                        cx.theme().control_radius(),
                    ))
                    .child(content),
            )
    }

    fn key(&self, index: usize, key: KeyRowKey, sticky: StickyModifiers, cx: &App) -> AnyElement {
        match key {
            KeyRowKey::Hide => {
                let on_hide = self.on_hide.clone();
                Self::cap(
                    index,
                    KeyLook::Rest,
                    Icon::new(IconName::ChevronDown).size(rems_from_px(16.0)),
                    cx,
                )
                .on_click(move |_, window, cx| {
                    if let Some(on_hide) = &on_hide {
                        on_hide(window, cx);
                    }
                })
                .into_any_element()
            }
            KeyRowKey::Key { label, keystroke } => {
                let on_key = self.on_key.clone();
                Self::cap(index, KeyLook::Rest, label, cx)
                    .on_click(move |_, window, cx| {
                        let keystroke = StickyModifiers::apply(&keystroke, cx);
                        window.refresh();
                        if let Some(on_key) = &on_key {
                            on_key(&keystroke, window, cx);
                        }
                    })
                    .into_any_element()
            }
            KeyRowKey::Modifier(modifier) => {
                let look = if sticky.is_latched(modifier) {
                    KeyLook::Latched
                } else {
                    KeyLook::Rest
                };
                let label = match modifier {
                    StickyModifier::Control => "ctrl",
                    StickyModifier::Alt => "alt",
                };
                Self::cap(index, look, label, cx)
                    .on_click(move |_, window, cx| {
                        StickyModifiers::toggle(modifier, cx);
                        window.refresh();
                    })
                    .into_any_element()
            }
            KeyRowKey::Popover(key) => Self::slot(ElementId::named_usize("key-row-slot", index))
                .child(key)
                .into_any_element(),
            KeyRowKey::Arrows(pad) => Self::slot(ElementId::named_usize("key-row-slot", index))
                .child(pad)
                .into_any_element(),
        }
    }
}

impl RenderOnce for KeyRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let sticky = StickyModifiers::get(cx);
        let keys: Vec<AnyElement> = self
            .keys
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, key)| self.key(index, key, sticky, cx))
            .collect();
        div()
            .id("key-row")
            .debug_selector(|| "key-row".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(rems_from_px(KEY_ROW_HEIGHT))
            .px(rems_from_px(ROW_PADDING_X - KEY_SLOP.0))
            .overflow_x_scroll()
            .restrict_scroll_to_axis()
            .when_some(
                self.scroll.as_ref(),
                StatefulInteractiveElement::track_scroll,
            )
            .font_family(cx.theme().font_family.clone())
            .text_size(rems_from_px(15.0))
            .line_height(rems_from_px(20.0))
            .children(keys)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use zpui::{Context, Render, TestAppContext, VisualTestContext};

    use super::*;

    struct Host {
        keys: ToolKeys,
        sent: Rc<RefCell<Vec<String>>>,
        width: Option<zpui::Pixels>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let sent = Rc::clone(&self.sent);
            let hide = Rc::clone(&self.sent);
            div().size_full().child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .when_some(self.width, Styled::w)
                    .when(self.width.is_none(), Styled::right_0)
                    .child(
                        KeyRow::new(self.keys.row())
                            .track_scroll(&self.keys.scroll)
                            .on_key(move |keystroke, _, _| {
                                sent.borrow_mut().push(keystroke.unparse());
                            })
                            .on_hide(move |_, _| hide.borrow_mut().push("hide".into())),
                    ),
            )
        }
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    fn host(cx: &mut TestAppContext) -> (zpui::Entity<Host>, &mut VisualTestContext) {
        host_with_width(cx, None)
    }

    fn host_with_width(
        cx: &mut TestAppContext,
        width: Option<zpui::Pixels>,
    ) -> (zpui::Entity<Host>, &mut VisualTestContext) {
        cx.update(crate::init);
        let (host, cx) = cx.add_window_view(move |_, cx| Host {
            keys: ToolKeys::new(vec![PopoverKeyItem::new("new-pane", "New pane")], None, cx),
            sent: Rc::default(),
            width,
        });
        redraw(cx);
        (host, cx)
    }

    fn tap(cx: &mut VisualTestContext, selector: &'static str) {
        let center = cx.debug_bounds(selector).expect(selector).center();
        cx.simulate_click(center, Modifiers::none());
        redraw(cx);
    }

    #[zpui::test]
    fn keys_send_through_the_sticky_latch(cx: &mut TestAppContext) {
        let (host, cx) = host(cx);
        assert!(cx.debug_bounds("arrow-pad").is_some());

        tap(cx, "key-row-key-4");
        tap(cx, "popover-key-ctrl");
        assert!(cx.update(|_, cx| StickyModifiers::get(cx).control));
        tap(cx, "key-row-key-5");
        assert!(cx.update(|_, cx| StickyModifiers::get(cx).is_empty()));
        tap(cx, "key-row-key-6");
        tap(cx, "key-row-key-8");
        let sent = host.read_with(cx, |host, _| host.sent.borrow().clone());
        assert_eq!(sent, ["escape", "ctrl-/", "@", "hide"]);

        tap(cx, "popover-key-prefix");
        assert!(cx.debug_bounds("popover-key-item-new-pane").is_some());
    }

    #[zpui::test]
    fn keys_take_taps_off_the_cap(cx: &mut TestAppContext) {
        let (host, cx) = host(cx);
        let row = cx.debug_bounds("key-row").expect("row");
        let esc = cx.debug_bounds("key-row-key-4").expect("esc");
        assert_eq!(row.size.height, zpui::px(KEY_ROW_HEIGHT));
        assert_eq!(esc.size.height, zpui::px(KEY_HEIGHT));
        for at in [
            zpui::point(esc.center().x, row.top() + zpui::px(1.0)),
            zpui::point(esc.center().x, row.bottom() - zpui::px(1.0)),
            zpui::point(esc.right() + zpui::px(KEY_SLOP.0 - 0.5), esc.center().y),
            zpui::point(
                esc.left() - zpui::px(KEY_SLOP.0 - 0.5),
                row.top() + zpui::px(1.0),
            ),
        ] {
            cx.simulate_click(at, Modifiers::none());
            redraw(cx);
        }
        let sent = host.read_with(cx, |host, _| host.sent.borrow().clone());
        assert_eq!(sent, ["escape", "escape", "escape", "escape"]);

        let ctrl = cx.debug_bounds("popover-key-ctrl").expect("ctrl");
        let above = zpui::point(ctrl.center().x, row.top() + zpui::px(1.0));
        for phase in [zpui::TouchPhase::Started, zpui::TouchPhase::Ended] {
            cx.simulate_event(zpui::TouchDragEvent {
                phase,
                start_position: above,
                position: above,
            });
            redraw(cx);
        }
        assert!(cx.update(|_, cx| StickyModifiers::get(cx).control));
    }

    #[zpui::test]
    fn keys_share_one_width_in_the_set_order(cx: &mut TestAppContext) {
        let (_, cx) = host(cx);
        let keys = [
            "popover-key-prefix",
            "popover-key-ctrl",
            "popover-key-alt",
            "key-row-key-3",
            "key-row-key-4",
            "key-row-key-5",
            "key-row-key-6",
            "arrow-pad",
            "key-row-key-8",
        ]
        .map(|selector| cx.debug_bounds(selector).expect(selector));
        for key in &keys {
            assert_eq!(key.size.width, zpui::px(KEY_WIDTH));
        }
        for pair in keys.windows(2) {
            assert!(pair[0].right() < pair[1].left());
        }
    }

    #[zpui::test]
    fn a_sideways_drag_on_a_card_key_scrolls_the_row(cx: &mut TestAppContext) {
        let (host, cx) = host_with_width(cx, Some(zpui::px(200.0)));
        let scroll = host.read_with(cx, |host, _| host.keys.scroll.clone());
        assert!(scroll.max_offset().x > zpui::px(0.0));
        let alt = cx.debug_bounds("popover-key-alt").expect("alt").center();
        let drag = |phase, x: f32, cx: &mut VisualTestContext| {
            cx.simulate_event(zpui::TouchDragEvent {
                phase,
                start_position: alt,
                position: zpui::point(alt.x + zpui::px(x), alt.y),
            });
            redraw(cx);
        };
        drag(zpui::TouchPhase::Started, 0.0, cx);
        drag(zpui::TouchPhase::Moved, -20.0, cx);
        drag(zpui::TouchPhase::Moved, -60.0, cx);
        drag(zpui::TouchPhase::Ended, -60.0, cx);
        assert_eq!(scroll.offset().x, zpui::px(-40.0));
        assert!(cx.debug_bounds("popover-key-item-alt-b").is_none());
        assert!(cx.update(|_, cx| StickyModifiers::get(cx).is_empty()));
    }
}
