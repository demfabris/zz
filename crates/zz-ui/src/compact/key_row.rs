use std::rc::Rc;

use gpui::{
    AnyElement, App, Div, ElementId, Entity, IntoElement, Keystroke, Modifiers, MouseButton,
    ParentElement as _, RenderOnce, SharedString, Stateful, Styled, Window, div, prelude::*,
};

use super::{
    arrow_pad::ArrowPad,
    sticky::{StickyModifier, StickyModifiers},
};
use crate::{ActiveTheme as _, Colorize as _, Icon, IconName, StyledExt as _, rems_from_px};

pub const KEY_ROW_HEIGHT: f32 = 44.0;
pub(super) const KEY_HEIGHT: f32 = 32.0;

const KEY_MIN_WIDTH: f32 = 30.0;
const KEY_PADDING_X: f32 = 4.0;
const ROW_PADDING_X: f32 = 8.0;
const KEY_GAP: f32 = 4.0;
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
    Prefix,
    Arrows,
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

    pub fn defaults() -> Vec<Self> {
        vec![
            Self::Hide,
            Self::named("esc", "escape"),
            Self::named("tab", "tab"),
            Self::Modifier(StickyModifier::Control),
            Self::Modifier(StickyModifier::Alt),
            Self::text("|"),
            Self::text("~"),
            Self::text("/"),
            Self::Prefix,
            Self::Arrows,
        ]
    }
}

type KeyHandler = Rc<dyn Fn(&Keystroke, &mut Window, &mut App)>;
type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct KeyRow {
    arrow_pad: Entity<ArrowPad>,
    keys: Vec<KeyRowKey>,
    prefix_armed: bool,
    prefix_label: SharedString,
    on_key: Option<KeyHandler>,
    on_prefix: Option<Handler>,
    on_hide: Option<Handler>,
}

impl KeyRow {
    pub fn new(arrow_pad: Entity<ArrowPad>) -> Self {
        Self {
            arrow_pad,
            keys: KeyRowKey::defaults(),
            prefix_armed: false,
            prefix_label: "C-b".into(),
            on_key: None,
            on_prefix: None,
            on_hide: None,
        }
    }

    #[must_use]
    pub fn keys(mut self, keys: Vec<KeyRowKey>) -> Self {
        self.keys = keys;
        self
    }

    #[must_use]
    pub fn prefix_armed(mut self, armed: bool) -> Self {
        self.prefix_armed = armed;
        self
    }

    #[must_use]
    pub fn prefix_label(mut self, label: impl Into<SharedString>) -> Self {
        self.prefix_label = label.into();
        self
    }

    #[must_use]
    pub fn on_key(mut self, f: impl Fn(&Keystroke, &mut Window, &mut App) + 'static) -> Self {
        self.on_key = Some(Rc::new(f));
        self
    }

    #[must_use]
    pub fn on_prefix(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_prefix = Some(Rc::new(f));
        self
    }

    #[must_use]
    pub fn on_hide(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_hide = Some(Rc::new(f));
        self
    }

    fn cap(index: usize, look: KeyLook, cx: &App) -> Stateful<Div> {
        let selector = format!("key-row-key-{index}");
        key_surface(
            div().id(ElementId::named_usize("key-row-key", index)),
            look,
            cx,
        )
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(rems_from_px(KEY_MIN_WIDTH))
        .px(rems_from_px(KEY_PADDING_X))
        .whitespace_nowrap()
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .when(look == KeyLook::Rest, |cap| {
            let pressed = cx.theme().background.washed(PRESSED_WASH);
            cap.active(move |style| style.bg(pressed))
        })
    }

    fn key(&self, index: usize, key: KeyRowKey, sticky: StickyModifiers, cx: &App) -> AnyElement {
        match key {
            KeyRowKey::Hide => {
                let on_hide = self.on_hide.clone();
                Self::cap(index, KeyLook::Rest, cx)
                    .child(Icon::new(IconName::ChevronDown).size(rems_from_px(16.0)))
                    .on_click(move |_, window, cx| {
                        if let Some(on_hide) = &on_hide {
                            on_hide(window, cx);
                        }
                    })
                    .into_any_element()
            }
            KeyRowKey::Key { label, keystroke } => {
                let on_key = self.on_key.clone();
                Self::cap(index, KeyLook::Rest, cx)
                    .child(label)
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
                Self::cap(index, look, cx)
                    .child(label)
                    .on_click(move |_, window, cx| {
                        StickyModifiers::toggle(modifier, cx);
                        window.refresh();
                    })
                    .into_any_element()
            }
            KeyRowKey::Prefix => {
                let on_prefix = self.on_prefix.clone();
                let look = if self.prefix_armed {
                    KeyLook::Inverted
                } else {
                    KeyLook::Rest
                };
                Self::cap(index, look, cx)
                    .child(self.prefix_label.clone())
                    .on_click(move |_, window, cx| {
                        if let Some(on_prefix) = &on_prefix {
                            on_prefix(window, cx);
                        }
                    })
                    .into_any_element()
            }
            KeyRowKey::Arrows => div()
                .flex()
                .flex_none()
                .ml_auto()
                .child(self.arrow_pad.clone())
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
            .px(rems_from_px(ROW_PADDING_X))
            .gap(rems_from_px(KEY_GAP))
            .overflow_x_scroll()
            .restrict_scroll_to_axis()
            .font_family(cx.theme().font_family.clone())
            .text_size(rems_from_px(13.0))
            .line_height(rems_from_px(16.0))
            .children(keys)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{Context, Render, TestAppContext, VisualTestContext};

    use super::*;

    struct Host {
        pad: Entity<ArrowPad>,
        armed: bool,
        sent: Rc<RefCell<Vec<String>>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let sent = Rc::clone(&self.sent);
            let prefix = Rc::clone(&self.sent);
            let hide = Rc::clone(&self.sent);
            div().size_full().child(
                div().absolute().bottom_0().left_0().right_0().child(
                    KeyRow::new(self.pad.clone())
                        .prefix_armed(self.armed)
                        .on_key(move |keystroke, _, _| {
                            sent.borrow_mut().push(keystroke.unparse());
                        })
                        .on_prefix(move |_, _| prefix.borrow_mut().push("prefix".into()))
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

    const KEYS: [&str; 9] = [
        "key-row-key-0",
        "key-row-key-1",
        "key-row-key-2",
        "key-row-key-3",
        "key-row-key-4",
        "key-row-key-5",
        "key-row-key-6",
        "key-row-key-7",
        "key-row-key-8",
    ];

    fn tap(cx: &mut VisualTestContext, index: usize) {
        let selector = KEYS[index];
        let center = cx.debug_bounds(selector).expect(selector).center();
        cx.simulate_click(center, Modifiers::none());
        redraw(cx);
    }

    #[gpui::test]
    fn keys_send_through_the_sticky_latch(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let sent = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&sent);
        let (_, cx) = cx.add_window_view(move |_, cx| Host {
            pad: cx.new(ArrowPad::new),
            armed: false,
            sent: log,
        });
        redraw(cx);
        assert!(cx.debug_bounds("arrow-pad").is_some());

        tap(cx, 1);
        tap(cx, 3);
        assert!(cx.update(|_, cx| StickyModifiers::get(cx).control));
        tap(cx, 7);
        assert!(cx.update(|_, cx| StickyModifiers::get(cx).is_empty()));
        tap(cx, 8);
        tap(cx, 0);
        assert_eq!(
            sent.borrow().as_slice(),
            ["escape", "ctrl-/", "prefix", "hide"]
        );
    }

    #[gpui::test]
    fn the_arrows_sit_at_the_right_edge(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(move |_, cx| Host {
            pad: cx.new(ArrowPad::new),
            armed: true,
            sent: Rc::default(),
        });
        redraw(cx);
        let row = cx.debug_bounds("key-row").expect("row");
        let pad = cx.debug_bounds("arrow-pad").expect("pad");
        let prefix = cx.debug_bounds("key-row-key-8").expect("prefix");
        assert!(pad.left() > prefix.right());
        assert!(row.right() - pad.right() <= gpui::px(ROW_PADDING_X + 0.5));
    }
}
