use super::actions::{Cancel, Confirm, SelectDown, SelectLeft, SelectRight, SelectUp};
use super::menu_item::MenuItemElement;
use crate::button::{Button, ButtonVariants as _};
use crate::scroll::ScrollableElement;
use crate::{ActiveTheme, Icon, IconName, Sizable as _, Size, StyledExt, h_flex, kbd::Kbd, v_flex};
use crate::{ElementExt, Side};
use gpui::{
    Action, Anchor, AnyElement, App, AppContext, Bounds, Context, DismissEvent, Edges, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyBinding,
    ParentElement, Pixels, Render, Role, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, WeakEntity, Window, anchored, div, prelude::FluentBuilder, px, rems,
};
use gpui::{ClickEvent, MouseDownEvent, Point, Subscription};

use crate::Colorize as _;
use std::rc::Rc;

const CONTEXT: &str = "ZzPopupMenu";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm { secondary: false }, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
    ]);
}

pub enum PopupMenuItem {
    Separator,
    Label(SharedString),
    Stepper {
        label: SharedString,
        value: Rc<dyn Fn(&App) -> SharedString>,
        decrement: Rc<dyn Fn(&mut Window, &mut App)>,
        reset: Rc<dyn Fn(&mut Window, &mut App)>,
        increment: Rc<dyn Fn(&mut Window, &mut App)>,
    },
    Item {
        icon: Option<Icon>,
        label: SharedString,
        disabled: bool,
        checked: bool,
        is_link: bool,
        action: Option<Box<dyn Action>>,
        handler: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    },
    ElementItem {
        icon: Option<Icon>,
        disabled: bool,
        checked: bool,
        action: Option<Box<dyn Action>>,
        render: Box<dyn Fn(bool, &mut Window, &mut App) -> AnyElement + 'static>,
        handler: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    },
    /// Only supported when the parent menu is not `scrollable`.
    Submenu {
        icon: Option<Icon>,
        label: SharedString,
        disabled: bool,
        menu: Entity<PopupMenu>,
    },
}

impl FluentBuilder for PopupMenuItem {}
impl PopupMenuItem {
    #[inline]
    pub fn new(label: impl Into<SharedString>) -> Self {
        PopupMenuItem::Item {
            icon: None,
            label: label.into(),
            disabled: false,
            checked: false,
            action: None,
            is_link: false,
            handler: None,
        }
    }

    #[inline]
    pub fn element<F, E>(builder: F) -> Self
    where
        F: Fn(bool, &mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        PopupMenuItem::ElementItem {
            icon: None,
            disabled: false,
            checked: false,
            action: None,
            render: Box::new(move |highlighted, window, cx| {
                builder(highlighted, window, cx).into_any_element()
            }),
            handler: None,
        }
    }

    pub fn stepper(
        label: impl Into<SharedString>,
        value: impl Fn(&App) -> SharedString + 'static,
        decrement: impl Fn(&mut Window, &mut App) + 'static,
        reset: impl Fn(&mut Window, &mut App) + 'static,
        increment: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Stepper {
            label: label.into(),
            value: Rc::new(value),
            decrement: Rc::new(decrement),
            reset: Rc::new(reset),
            increment: Rc::new(increment),
        }
    }

    #[inline]
    pub fn submenu(label: impl Into<SharedString>, menu: Entity<PopupMenu>) -> Self {
        PopupMenuItem::Submenu {
            icon: None,
            label: label.into(),
            disabled: false,
            menu,
        }
    }

    #[inline]
    pub fn separator() -> Self {
        PopupMenuItem::Separator
    }

    #[inline]
    pub fn label(label: impl Into<SharedString>) -> Self {
        PopupMenuItem::Label(label.into())
    }

    /// Only works for [`PopupMenuItem::Item`], [`PopupMenuItem::ElementItem`] and [`PopupMenuItem::Submenu`].
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        match &mut self {
            PopupMenuItem::Item { icon: i, .. } => {
                *i = Some(icon.into());
            }
            PopupMenuItem::ElementItem { icon: i, .. } => {
                *i = Some(icon.into());
            }
            PopupMenuItem::Submenu { icon: i, .. } => {
                *i = Some(icon.into());
            }
            _ => {}
        }
        self
    }

    /// Only works for [`PopupMenuItem::Item`] and [`PopupMenuItem::ElementItem`].
    pub fn action(mut self, action: Box<dyn Action>) -> Self {
        match &mut self {
            PopupMenuItem::Item { action: a, .. } => {
                *a = Some(action);
            }
            PopupMenuItem::ElementItem { action: a, .. } => {
                *a = Some(action);
            }
            _ => {}
        }
        self
    }

    /// Only works for [`PopupMenuItem::Item`], [`PopupMenuItem::ElementItem`] and [`PopupMenuItem::Submenu`].
    pub fn disabled(mut self, disabled: bool) -> Self {
        match &mut self {
            PopupMenuItem::Item { disabled: d, .. } => {
                *d = disabled;
            }
            PopupMenuItem::ElementItem { disabled: d, .. } => {
                *d = disabled;
            }
            PopupMenuItem::Submenu { disabled: d, .. } => {
                *d = disabled;
            }
            _ => {}
        }
        self
    }

    /// A checked item renders a check icon in place of its left icon.
    pub fn checked(mut self, checked: bool) -> Self {
        match &mut self {
            PopupMenuItem::Item { checked: c, .. } => {
                *c = checked;
            }
            PopupMenuItem::ElementItem { checked: c, .. } => {
                *c = checked;
            }
            _ => {}
        }
        self
    }

    /// Only works for [`PopupMenuItem::Item`] and [`PopupMenuItem::ElementItem`].
    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        match &mut self {
            PopupMenuItem::Item { handler: h, .. } => {
                *h = Some(Rc::new(handler));
            }
            PopupMenuItem::ElementItem { handler: h, .. } => {
                *h = Some(Rc::new(handler));
            }
            _ => {}
        }
        self
    }

    #[inline]
    pub fn link(label: impl Into<SharedString>, href: impl Into<String>) -> Self {
        let href = href.into();
        PopupMenuItem::Item {
            icon: None,
            label: label.into(),
            disabled: false,
            checked: false,
            action: None,
            is_link: true,
            handler: Some(Rc::new(move |_, _, cx| cx.open_url(&href))),
        }
    }

    #[inline]
    fn is_clickable(&self) -> bool {
        !matches!(self, PopupMenuItem::Separator)
            && matches!(
                self,
                PopupMenuItem::Item {
                    disabled: false,
                    ..
                } | PopupMenuItem::ElementItem {
                    disabled: false,
                    ..
                } | PopupMenuItem::Submenu {
                    disabled: false,
                    ..
                } | PopupMenuItem::Stepper { .. }
            )
    }

    #[inline]
    fn is_separator(&self) -> bool {
        matches!(self, PopupMenuItem::Separator)
    }

    fn has_left_icon(&self) -> bool {
        match self {
            PopupMenuItem::Item { icon, checked, .. } => icon.is_some() || *checked,
            PopupMenuItem::ElementItem { icon, checked, .. } => icon.is_some() || *checked,
            PopupMenuItem::Submenu { icon, .. } => icon.is_some(),
            _ => false,
        }
    }

    #[inline]
    fn is_checked(&self) -> bool {
        match self {
            PopupMenuItem::Item { checked, .. } => *checked,
            PopupMenuItem::ElementItem { checked, .. } => *checked,
            _ => false,
        }
    }

    fn a11y_label(&self) -> Option<SharedString> {
        match self {
            PopupMenuItem::Item { label, .. }
            | PopupMenuItem::Label(label)
            | PopupMenuItem::Submenu { label, .. }
            | PopupMenuItem::Stepper { label, .. } => Some(label.clone()),
            PopupMenuItem::Separator | PopupMenuItem::ElementItem { .. } => None,
        }
    }
}

#[derive(Clone, Copy)]
enum StepperAction {
    Decrement,
    Reset,
    Increment,
}

pub struct PopupMenu {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) menu_items: Vec<PopupMenuItem>,
    pub(crate) action_context: Option<FocusHandle>,
    pub(crate) previous_focus_handle: Option<FocusHandle>,
    selected_index: Option<usize>,
    min_width: Option<Pixels>,
    max_width: Option<Pixels>,
    max_height: Option<Pixels>,
    bounds: Bounds<Pixels>,
    size: Size,

    parent_menu: Option<WeakEntity<Self>>,
    scrollable: bool,
    scroll_handle: ScrollHandle,
    submenu_anchor: (Anchor, Pixels),

    _subscriptions: Vec<Subscription>,
}

impl PopupMenu {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            action_context: None,
            previous_focus_handle: None,
            parent_menu: None,
            menu_items: Vec::new(),
            selected_index: None,
            min_width: None,
            max_width: None,
            max_height: None,
            bounds: Bounds::default(),
            scrollable: false,
            scroll_handle: ScrollHandle::default(),
            size: Size::default(),
            submenu_anchor: (Anchor::TopLeft, Pixels::ZERO),
            _subscriptions: vec![],
        }
    }

    pub fn build(
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(Self, &mut Window, &mut Context<PopupMenu>) -> Self,
    ) -> Entity<Self> {
        cx.new(|cx| f(Self::new(cx), window, cx))
    }

    /// Focus returns to this handle on dismiss and before an action fires, and
    /// the action is dispatched there.
    pub fn action_context(mut self, handle: FocusHandle) -> Self {
        self.action_context = Some(handle);
        self
    }

    pub(crate) fn set_previous_focus(
        &mut self,
        handle: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.previous_focus_handle = handle.clone();

        for item in &self.menu_items {
            if let PopupMenuItem::Submenu { menu, .. } = item {
                menu.update(cx, |menu, cx| {
                    menu.set_previous_focus(handle.clone(), cx);
                });
            }
        }
    }

    /// Set the menu's min width. Defaults to 8rem.
    pub fn min_w(mut self, width: impl Into<Pixels>) -> Self {
        self.min_width = Some(width.into());
        self
    }

    /// Set the menu's max width. Defaults to 500px.
    pub fn max_w(mut self, width: impl Into<Pixels>) -> Self {
        self.max_width = Some(width.into());
        self
    }

    /// Set the menu's max height. Defaults to half the window height, capped
    /// at 450px.
    pub fn max_h(mut self, height: impl Into<Pixels>) -> Self {
        self.max_height = Some(height.into());
        self
    }

    /// A scrollable menu cannot carry submenus.
    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    pub fn menu(self, label: impl Into<SharedString>, action: Box<dyn Action>) -> Self {
        self.menu_with_disabled(label, action, false)
    }

    pub fn menu_with_disabled(
        mut self,
        label: impl Into<SharedString>,
        action: Box<dyn Action>,
        disabled: bool,
    ) -> Self {
        self.add_menu_item(label, None, action, disabled, false);
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.menu_items.push(PopupMenuItem::label(label.into()));
        self
    }

    pub fn link(self, label: impl Into<SharedString>, href: impl Into<String>) -> Self {
        self.link_with_disabled(label, href, false)
    }

    pub fn link_with_disabled(
        mut self,
        label: impl Into<SharedString>,
        href: impl Into<String>,
        disabled: bool,
    ) -> Self {
        let href = href.into();
        self.menu_items
            .push(PopupMenuItem::link(label, href).disabled(disabled));
        self
    }

    pub fn separator(mut self) -> Self {
        if self.menu_items.is_empty() {
            return self;
        }

        if let Some(PopupMenuItem::Separator) = self.menu_items.last() {
            return self;
        }

        self.menu_items.push(PopupMenuItem::separator());
        self
    }

    pub fn submenu_with_icon(
        mut self,
        icon: Option<Icon>,
        label: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        let submenu = PopupMenu::build(window, cx, f);
        let parent_menu = cx.entity().downgrade();
        submenu.update(cx, |view, _| {
            view.parent_menu = Some(parent_menu);
        });

        self.menu_items.push(
            PopupMenuItem::submenu(label, submenu).when_some(icon, |this, icon| this.icon(icon)),
        );
        self
    }

    pub fn item(mut self, item: impl Into<PopupMenuItem>) -> Self {
        let item: PopupMenuItem = item.into();
        self.menu_items.push(item);
        self
    }

    fn add_menu_item(
        &mut self,
        label: impl Into<SharedString>,
        icon: Option<Icon>,
        action: Box<dyn Action>,
        disabled: bool,
        checked: bool,
    ) -> &mut Self {
        self.menu_items.push(
            PopupMenuItem::new(label)
                .when_some(icon, |item, icon| item.icon(icon))
                .disabled(disabled)
                .checked(checked)
                .action(action),
        );
        self
    }

    pub(crate) fn active_submenu(&self) -> Option<Entity<PopupMenu>> {
        if let Some(ix) = self.selected_index {
            if let Some(item) = self.menu_items.get(ix) {
                return match item {
                    PopupMenuItem::Submenu { menu, .. } => Some(menu.clone()),
                    _ => None,
                };
            }
        }

        None
    }

    pub fn is_empty(&self) -> bool {
        self.menu_items.is_empty()
    }

    fn clickable_menu_items(&self) -> impl Iterator<Item = (usize, &PopupMenuItem)> {
        self.menu_items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.is_clickable())
    }

    fn on_click(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        window.prevent_default();
        self.selected_index = Some(ix);
        self.confirm(&Confirm { secondary: false }, window, cx);
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.adjust_stepper(StepperAction::Reset, window, cx) {
            return;
        }
        match self.selected_index {
            Some(index) => {
                let item = self.menu_items.get(index);
                match item {
                    Some(PopupMenuItem::Item {
                        handler, action, ..
                    }) => {
                        if let Some(handler) = handler {
                            handler(&ClickEvent::default(), window, cx);
                        } else if let Some(action) = action.as_ref() {
                            self.dispatch_confirm_action(action, window, cx);
                        }

                        self.dismiss(&Cancel, window, cx)
                    }
                    Some(PopupMenuItem::ElementItem {
                        handler, action, ..
                    }) => {
                        if let Some(handler) = handler {
                            handler(&ClickEvent::default(), window, cx);
                        } else if let Some(action) = action.as_ref() {
                            self.dispatch_confirm_action(action, window, cx);
                        }
                        self.dismiss(&Cancel, window, cx)
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn adjust_stepper(
        &mut self,
        action: StepperAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(PopupMenuItem::Stepper {
            decrement,
            reset,
            increment,
            ..
        }) = self
            .selected_index
            .and_then(|index| self.menu_items.get(index))
        else {
            return false;
        };
        cx.stop_propagation();
        match action {
            StepperAction::Decrement => decrement(window, cx),
            StepperAction::Reset => reset(window, cx),
            StepperAction::Increment => increment(window, cx),
        }
        cx.notify();
        true
    }

    fn dispatch_confirm_action(
        &self,
        action: &Box<dyn Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(context) = self.action_context.as_ref() {
            context.focus(window, cx);
        }

        window.dispatch_action(action.boxed_clone(), cx);
    }

    pub(crate) fn set_selected_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.selected_index != Some(ix) {
            self.selected_index = Some(ix);
            self.scroll_handle.scroll_to_item(ix);
            cx.notify();
        }
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let ix = self.selected_index.unwrap_or(0);

        if let Some((prev_ix, _)) = self
            .menu_items
            .iter()
            .enumerate()
            .rev()
            .find(|(i, item)| *i < ix && item.is_clickable())
        {
            self.set_selected_index(prev_ix, cx);
            return;
        }

        let last_clickable_ix = self.clickable_menu_items().last().map(|(ix, _)| ix);
        self.set_selected_index(last_clickable_ix.unwrap_or(0), cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let Some(ix) = self.selected_index else {
            self.set_selected_index(0, cx);
            return;
        };

        if let Some((next_ix, _)) = self
            .menu_items
            .iter()
            .enumerate()
            .find(|(i, item)| *i > ix && item.is_clickable())
        {
            self.set_selected_index(next_ix, cx);
            return;
        }

        self.set_selected_index(0, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        if self.adjust_stepper(StepperAction::Decrement, window, cx) {
            return;
        }
        let handled = if matches!(self.submenu_anchor.0, Anchor::TopLeft | Anchor::BottomLeft) {
            self._unselect_submenu(window, cx)
        } else {
            self._select_submenu(window, cx)
        };

        if self.parent_side(cx).is_left() {
            self._focus_parent_menu(window, cx);
        }

        if handled {
            return;
        }

        if self.parent_menu.is_none() {
            cx.propagate();
        }
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        if self.adjust_stepper(StepperAction::Increment, window, cx) {
            return;
        }
        let handled = if matches!(self.submenu_anchor.0, Anchor::TopLeft | Anchor::BottomLeft) {
            self._select_submenu(window, cx)
        } else {
            self._unselect_submenu(window, cx)
        };

        if self.parent_side(cx).is_right() {
            self._focus_parent_menu(window, cx);
        }

        if handled {
            return;
        }

        if self.parent_menu.is_none() {
            cx.propagate();
        }
    }

    fn _select_submenu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(active_submenu) = self.active_submenu() {
            active_submenu.update(cx, |view, cx| {
                view.set_selected_index(0, cx);
                view.focus_handle.focus(window, cx);
            });
            cx.notify();
            return true;
        }

        return false;
    }

    fn _unselect_submenu(&mut self, _: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(active_submenu) = self.active_submenu() {
            active_submenu.update(cx, |view, cx| {
                view.selected_index = None;
                cx.notify();
            });
            return true;
        }

        return false;
    }

    fn _focus_parent_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = self.parent_menu.as_ref() else {
            return;
        };
        let Some(parent) = parent.upgrade() else {
            return;
        };

        self.selected_index = None;
        parent.update(cx, |view, cx| {
            view.focus_handle.focus(window, cx);
            cx.notify();
        });
    }

    fn parent_side(&self, cx: &App) -> Side {
        let Some(parent) = self.parent_menu.as_ref() else {
            return Side::Left;
        };

        let Some(parent) = parent.upgrade() else {
            return Side::Left;
        };

        match parent.read(cx).submenu_anchor.0 {
            Anchor::TopLeft | Anchor::BottomLeft => Side::Left,
            Anchor::TopRight | Anchor::BottomRight => Side::Right,
            _ => Side::Left,
        }
    }

    fn dismiss(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_index = None;
        cx.emit(DismissEvent);

        let focus_moved_away =
            window.focused(cx).is_some() && !self.focus_handle.contains_focused(window, cx);
        if !focus_moved_away {
            if let Some(handle) = self
                .previous_focus_handle
                .as_ref()
                .or(self.action_context.as_ref())
            {
                window.focus(handle, cx);
            }
        }

        let Some(parent_menu) = self.parent_menu.clone() else {
            return;
        };

        _ = parent_menu.update(cx, |view, cx| {
            view.dismiss(&Cancel, window, cx);
        });
    }

    fn handle_dismiss(
        &mut self,
        position: &Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(parent) = self.parent_menu.as_ref() {
            if let Some(parent) = parent.upgrade() {
                if parent.read(cx).bounds.contains(position) {
                    return;
                }
            }
        }

        if self.active_submenu().is_some() {
            return;
        }

        self.dismiss(&Cancel, window, cx);
    }

    fn on_mouse_down_out(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_dismiss(&e.position, window, cx);
    }

    fn render_key_binding(
        &self,
        action: Option<Box<dyn Action>>,
        opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Kbd> {
        let action = action?;

        match self
            .action_context
            .as_ref()
            .or(self.previous_focus_handle.as_ref())
            .and_then(|handle| Kbd::binding_for_action_in(action.as_ref(), handle, window))
        {
            Some(kbd) => Some(kbd),
            None => Kbd::binding_for_action(action.as_ref(), None, window),
        }
        .map(|this| {
            this.p_0()
                .flex_nowrap()
                .border_0()
                .bg(gpui::transparent_white())
                .text_color(cx.theme().foreground)
                .opacity(opacity)
        })
    }

    fn render_icon(
        has_icon: bool,
        checked: bool,
        icon: Option<Icon>,
        opacity: f32,
    ) -> Option<impl IntoElement> {
        if !has_icon {
            return None;
        }

        let icon = if let Some(icon) = icon {
            icon.clone()
        } else if checked {
            Icon::new(IconName::Check)
        } else {
            Icon::empty()
        };

        Some(
            div()
                .relative()
                .top(px(0.5))
                .flex_none()
                .child(icon.size(px(14.0)).opacity(opacity)),
        )
    }

    #[inline]
    fn max_width(&self) -> Pixels {
        self.max_width.unwrap_or(px(500.))
    }

    fn update_submenu_menu_anchor(&mut self, window: &Window) {
        let bounds = self.bounds;
        let max_width = self.max_width();
        let (anchor, left) = if max_width + bounds.origin.x > window.bounds().size.width {
            (Anchor::TopRight, -px(16.))
        } else {
            (Anchor::TopLeft, bounds.size.width - px(8.))
        };

        let is_bottom_pos = bounds.origin.y + bounds.size.height > window.bounds().size.height;
        self.submenu_anchor = if is_bottom_pos {
            (anchor.other_side_along(gpui::Axis::Vertical), left)
        } else {
            (anchor, left)
        };
    }

    fn render_item(
        &self,
        ix: usize,
        item: &PopupMenuItem,
        options: RenderOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> MenuItemElement {
        let has_left_icon = options.has_left_icon;
        let is_left_check = item.is_checked();

        let selected = self.selected_index == Some(ix);
        let detail_opacity = if selected && item.is_clickable() {
            1.0
        } else {
            0.8
        };
        const EDGE_PADDING: Pixels = px(4.);
        const INNER_PADDING: Pixels = px(8.);

        let is_submenu = matches!(item, PopupMenuItem::Submenu { .. });
        let group_name = format!("{}:item-{}", cx.entity().entity_id(), ix);

        let item_height = match self.size {
            Size::Small => px(20.),
            _ => px(26.),
        };

        let this = MenuItemElement::new(ix, &group_name)
            .relative()
            .text_size(crate::rems_from_px(12.0))
            .line_height(px(16.0))
            .py_0()
            .px(INNER_PADDING)
            .menu_item_corners(item_height, cx)
            .items_center()
            .selected(selected)
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                if *hovered {
                    this.selected_index = Some(ix);
                } else if !is_submenu && this.selected_index == Some(ix) {
                    // TODO: Better handle the submenu unselection when hover out
                    this.selected_index = None;
                }

                cx.notify();
            }))
            .when_some(item.a11y_label(), |this, label| this.aria_label(label));

        match item {
            PopupMenuItem::Separator => this
                .h_auto()
                .p_0()
                .my(px(2.0))
                .mx(px(4.0))
                .rounded(px(0.0))
                .border_0()
                .border_b(px(0.5))
                .border_color(cx.theme().foreground.opacity(0.1))
                .disabled(true),
            PopupMenuItem::Label(label) => this
                .disabled(true)
                .cursor_default()
                .py_0()
                .text_size(crate::rems_from_px(11.0))
                .child(
                    h_flex()
                        .cursor_default()
                        .items_center()
                        .gap(px(8.0))
                        .children(Self::render_icon(
                            has_left_icon,
                            false,
                            None,
                            detail_opacity,
                        ))
                        .child(div().flex_1().child(label.clone())),
                ),
            PopupMenuItem::Stepper { label, value, .. } => {
                let value = value(cx);
                let label_for = |action| match action {
                    StepperAction::Decrement => format!("Decrease {label}"),
                    StepperAction::Reset => format!("Reset {label}"),
                    StepperAction::Increment => format!("Increase {label}"),
                };
                let button = |id, action| {
                    Button::new((id, ix))
                        .debug_selector(move || format!("{id}-{ix}"))
                        .ghost()
                        .flat()
                        .xsmall()
                        .tab_stop(false)
                        .tooltip(label_for(action))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.selected_index = Some(ix);
                            this.adjust_stepper(action, window, cx);
                        }))
                };
                this.h(item_height.max(px(26.0)))
                    .aria_label(format!(
                        "{label}: {value}; Left and Right adjust, Enter resets"
                    ))
                    .gap(px(10.0))
                    .on_click(cx.listener(move |this, _, window, cx| this.on_click(ix, window, cx)))
                    .children(Self::render_icon(
                        has_left_icon,
                        false,
                        None,
                        detail_opacity,
                    ))
                    .child(div().flex_1().child(label.clone()))
                    .child(
                        h_flex()
                            .flex_none()
                            .rounded(cx.theme().control_radius())
                            .bg(cx.theme().background.raised(2).opaque())
                            .control_surface(cx)
                            .child(
                                button("menu-step-down", StepperAction::Decrement)
                                    .icon(IconName::Minus),
                            )
                            .child(
                                button("menu-step-reset", StepperAction::Reset)
                                    .label(value)
                                    .min_w(px(48.0)),
                            )
                            .child(
                                button("menu-step-up", StepperAction::Increment)
                                    .icon(IconName::Plus),
                            ),
                    )
            }
            PopupMenuItem::ElementItem {
                render,
                icon,
                disabled,
                ..
            } => this
                .when(!disabled, |this| {
                    this.on_click(
                        cx.listener(move |this, _, window, cx| this.on_click(ix, window, cx)),
                    )
                })
                .disabled(*disabled)
                .child(
                    h_flex()
                        .flex_1()
                        .min_h(item_height)
                        .items_center()
                        .gap(px(8.0))
                        .children(Self::render_icon(
                            has_left_icon,
                            is_left_check,
                            icon.clone(),
                            detail_opacity,
                        ))
                        .child((render)(selected && !disabled, window, cx)),
                ),
            PopupMenuItem::Item {
                icon,
                label,
                action,
                disabled,
                is_link,
                ..
            } => {
                let show_link_icon = *is_link;
                let action = action.as_ref().map(|action| action.boxed_clone());
                let key = self.render_key_binding(action, detail_opacity, window, cx);

                this.when(!disabled, |this| {
                    this.on_click(
                        cx.listener(move |this, _, window, cx| this.on_click(ix, window, cx)),
                    )
                })
                .disabled(*disabled)
                .h(item_height)
                .gap(px(8.0))
                .children(Self::render_icon(
                    has_left_icon,
                    is_left_check,
                    icon.clone(),
                    detail_opacity,
                ))
                .child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .items_center()
                        .justify_between()
                        .when(!show_link_icon, |this| this.child(label.clone()))
                        .when(show_link_icon, |this| {
                            this.child(
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .gap_1p5()
                                    .child(label.clone())
                                    .child(
                                        Icon::new(IconName::ExternalLink)
                                            .xsmall()
                                            .opacity(detail_opacity),
                                    ),
                            )
                        })
                        .children(key),
                )
            }
            PopupMenuItem::Submenu {
                icon,
                label,
                menu,
                disabled,
            } => this
                .selected(selected)
                .submenu_open(selected)
                .disabled(*disabled)
                .items_start()
                .child(
                    h_flex()
                        .min_h(item_height)
                        .size_full()
                        .items_center()
                        .gap(px(8.0))
                        .children(Self::render_icon(
                            has_left_icon,
                            false,
                            icon.clone(),
                            detail_opacity,
                        ))
                        .child(
                            h_flex()
                                .flex_1()
                                .gap_2()
                                .items_center()
                                .justify_between()
                                .child(label.clone())
                                .child(
                                    Icon::new(IconName::ChevronRight)
                                        .xsmall()
                                        .opacity(detail_opacity),
                                ),
                        ),
                )
                .when(selected, |this| {
                    this.child({
                        let (anchor, left) = self.submenu_anchor;
                        let is_bottom_pos =
                            matches!(anchor, Anchor::BottomLeft | Anchor::BottomRight);
                        anchored()
                            .anchor(anchor)
                            .child(
                                div()
                                    .id("submenu")
                                    .occlude()
                                    .when(is_bottom_pos, |this| this.bottom_0())
                                    .when(!is_bottom_pos, |this| this.top_neg_1())
                                    .left(left)
                                    .child(menu.clone()),
                            )
                            .snap_to_window_with_margin(Edges::all(EDGE_PADDING))
                    })
                }),
        }
    }
}

impl FluentBuilder for PopupMenu {}
impl EventEmitter<DismissEvent> for PopupMenu {}
impl Focusable for PopupMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Clone, Copy)]
struct RenderOptions {
    has_left_icon: bool,
}

impl Render for PopupMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_submenu_menu_anchor(window);

        let view = cx.entity().clone();
        let items_count = self.menu_items.len();

        let max_height = self.max_height.unwrap_or_else(|| {
            let window_half_height = window.window_bounds().get_bounds().size.height * 0.5;
            window_half_height.min(px(450.))
        });

        let has_left_icon = self.menu_items.iter().any(|item| item.has_left_icon());

        let max_width = self.max_width();
        let options = RenderOptions { has_left_icon };

        let surface = v_flex()
            .id("popup-menu")
            .role(Role::Menu)
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .on_mouse_down_out(cx.listener(Self::on_mouse_down_out))
            .popover_style(cx)
            .text_color(cx.theme().foreground)
            .relative()
            .occlude()
            .child(
                v_flex()
                    .id("items")
                    .p(px(4.0))
                    .gap_y_0p5()
                    .min_w(rems(8.))
                    .when_some(self.min_width, |this, min_width| this.min_w(min_width))
                    .max_w(max_width)
                    .when(self.scrollable, |this| {
                        this.max_h(max_height)
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_handle)
                    })
                    .children(
                        self.menu_items
                            .iter()
                            .enumerate()
                            .filter(|(ix, item)| !(*ix + 1 == items_count && item.is_separator()))
                            .map(|(ix, item)| self.render_item(ix, item, options, window, cx)),
                    )
                    .on_prepaint(move |bounds, _, cx| {
                        view.update(cx, |menu, cx| {
                            let first_layout = menu.bounds.size.height == px(0.0);
                            menu.bounds = bounds;
                            if first_layout
                                && menu.scrollable
                                && let Some(ix) = menu.selected_index
                            {
                                menu.scroll_handle.scroll_to_item(ix);
                                cx.notify();
                            }
                        })
                    }),
            )
            .when(self.scrollable, |this| {
                // TODO: When the menu is limited by `overflow_y_scroll`, the sub-menu will cannot be displayed.
                this.vertical_scrollbar(&self.scroll_handle)
            });
        crate::widget::foundation::surface_enter(surface, "menu-open", px(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn custom_content_tracks_keyboard_and_pointer_highlights(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let highlights = Rc::new(std::cell::RefCell::new([false; 2]));
        let (_, cx) = cx.add_window_view(|window, cx| {
            let mut menu = PopupMenu::new(cx);
            for index in 0..2 {
                let highlights = highlights.clone();
                menu = menu.item(
                    PopupMenuItem::element(move |highlighted, _, _| {
                        highlights.borrow_mut()[index] = highlighted;
                        div()
                            .id(("custom-content", index))
                            .debug_selector(move || format!("custom-content-{index}"))
                            .child("Description")
                    })
                    .checked(index == 0),
                );
            }
            menu.focus_handle.focus(window, cx);
            menu
        });
        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| _ = window.draw(cx));
        };
        draw(cx);
        assert_eq!(*highlights.borrow(), [false, false]);
        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(*highlights.borrow(), [true, false]);
        let second = cx.debug_bounds("custom-content-1").unwrap();
        cx.simulate_mouse_move(second.center(), None, gpui::Modifiers::default());
        draw(cx);
        assert_eq!(*highlights.borrow(), [false, true]);
    }

    #[gpui::test]
    fn popup_highlights_use_exact_tuned_corners_inside_the_surface(cx: &mut gpui::TestAppContext) {
        struct Preview(Entity<PopupMenu>);
        impl Render for Preview {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                v_flex().size_full().items_start().child(self.0.clone())
            }
        }
        for (configured, expected) in [(12.0, 8.0), (14.0, 10.0), (25.0, 10.4)] {
            cx.update(|cx| {
                crate::init(cx);
                cx.set_reduce_motion(true);
                crate::Theme::global_mut(cx).radius = px(configured);
            });
            let (_, cx) = cx.add_window_view(|window, cx| {
                window.set_adaptive_corner_fraction(Some(0.45));
                window.set_default_corner_smoothing(4.0);
                Preview(cx.new(|cx| {
                    let mut menu =
                        PopupMenu::new(cx).item(PopupMenuItem::new("System default").checked(true));
                    menu.selected_index = Some(0);
                    menu
                }))
            });
            cx.update(|window, cx| {
                _ = window.draw(cx);
                let quads = window.painted_quads();
                let surface = quads
                    .iter()
                    .find(|quad| {
                        quad.background
                            == gpui::solid_background(cx.theme().background.raised(2).opaque())
                    })
                    .expect("menu surface");
                let highlight = quads
                    .iter()
                    .find(|quad| {
                        quad.background == gpui::solid_background(cx.theme().selection_background())
                    })
                    .expect("selected menu entry");
                assert_eq!(highlight.corner_smoothing, 2.5);
                assert_eq!(surface.corner_smoothing, 4.0);
                assert!(
                    (highlight.corner_radii.top_left.0 / window.scale_factor() - expected).abs()
                        < 0.001
                );
                assert!(highlight.corner_radii.top_left.0 < highlight.bounds.size.height.0 / 2.0);
                assert!(surface.bounds.contains(&highlight.bounds.origin));
                assert!(surface.bounds.contains(&highlight.bounds.bottom_right()));
            });
        }
    }

    #[gpui::test]
    fn open_submenu_parent_stays_neutral_while_leaf_stays_accented(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let mut menu = PopupMenu::new(cx).submenu_with_icon(
                None,
                "More",
                window,
                cx,
                |mut submenu, _, _| {
                    submenu = submenu.item(PopupMenuItem::new("Open"));
                    submenu.selected_index = Some(0);
                    submenu
                },
            );
            menu.selected_index = Some(0);
            menu.focus_handle.focus(window, cx);
            menu
        });
        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| {
                _ = window.draw(cx);
                let quads = window.painted_quads();
                let parent = quads
                    .iter()
                    .find(|quad| {
                        quad.background == gpui::solid_background(cx.theme().background.washed(2))
                    })
                    .expect("open submenu parent has a neutral background");
                let child = quads
                    .iter()
                    .find(|quad| {
                        quad.background == gpui::solid_background(cx.theme().selection_background())
                    })
                    .expect("selected submenu leaf has an accent background");
                let position = |quad: &gpui::Quad| {
                    let center = quad.bounds.center();
                    gpui::point(
                        px(center.x.0 / window.scale_factor()),
                        px(center.y.0 / window.scale_factor()),
                    )
                };
                (position(parent), position(child))
            })
        };
        let (parent, _) = draw(cx);
        cx.simulate_mouse_move(parent, None, gpui::Modifiers::default());
        let (_, child) = draw(cx);
        cx.simulate_mouse_move(child, None, gpui::Modifiers::default());
        draw(cx);
    }

    fn menu_with_open_submenu(
        clicks: Rc<std::cell::Cell<usize>>,
        cx: &mut gpui::TestAppContext,
    ) -> (
        Entity<PopupMenu>,
        Rc<std::cell::Cell<usize>>,
        &mut gpui::VisualTestContext,
    ) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        struct Host(Entity<PopupMenu>);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                v_flex().size_full().items_start().child(self.0.clone())
            }
        }
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host(cx.new(|cx| {
                let mut menu = PopupMenu::new(cx).submenu_with_icon(
                    None,
                    "More",
                    window,
                    cx,
                    move |mut submenu, _, _| {
                        let clicks = Rc::clone(&clicks);
                        submenu = submenu.item(
                            PopupMenuItem::new("Open")
                                .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                        );
                        submenu.selected_index = Some(0);
                        submenu
                    },
                );
                menu.selected_index = Some(0);
                menu.focus_handle.focus(window, cx);
                menu
            }))
        });
        let menu = host.read_with(cx, |host, _| host.0.clone());
        let dismissed = Rc::new(std::cell::Cell::new(0));
        let count = Rc::clone(&dismissed);
        cx.update(|window, cx| {
            window
                .subscribe(&menu, cx, move |_, _: &DismissEvent, _, _| {
                    count.set(count.get() + 1)
                })
                .detach();
        });
        (menu, dismissed, cx)
    }

    #[gpui::test]
    fn escape_closes_the_chain_while_a_hovered_submenu_is_open(cx: &mut gpui::TestAppContext) {
        let clicks = Rc::new(std::cell::Cell::new(0));
        let (menu, dismissed, cx) = menu_with_open_submenu(Rc::clone(&clicks), cx);
        assert!(menu.read_with(cx, |menu, _| menu.active_submenu().is_some()));

        cx.simulate_keystrokes("escape");

        assert_eq!(dismissed.get(), 1);
        assert!(menu.read_with(cx, |menu, _| menu.active_submenu().is_none()));
        assert_eq!(clicks.get(), 0);
    }

    #[gpui::test]
    fn clicking_a_submenu_item_runs_it_before_the_chain_closes(cx: &mut gpui::TestAppContext) {
        let clicks = Rc::new(std::cell::Cell::new(0));
        let (menu, dismissed, cx) = menu_with_open_submenu(Rc::clone(&clicks), cx);
        cx.run_until_parked();
        let leaf = cx.update(|window, cx| {
            _ = window.draw(cx);
            let quad = window
                .painted_quads()
                .into_iter()
                .find(|quad| {
                    quad.background == gpui::solid_background(cx.theme().selection_background())
                })
                .expect("selected submenu leaf");
            let center = quad.bounds.center();
            gpui::point(
                px(center.x.0 / window.scale_factor()),
                px(center.y.0 / window.scale_factor()),
            )
        });

        assert!(!menu.read_with(cx, |menu, _| menu.bounds.contains(&leaf)));

        cx.simulate_click(leaf, gpui::Modifiers::default());

        assert_eq!(clicks.get(), 1);
        assert_eq!(dismissed.get(), 1);
    }

    #[gpui::test]
    fn dismissal_preserves_focus_moved_by_menu_handler(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);

        for moves_focus in [false, true] {
            let previous_focus = cx.update(|cx| cx.focus_handle());
            let target_focus = cx.update(|cx| cx.focus_handle());
            let (menu, cx) = cx.add_window_view(|window, cx| {
                let target_focus = target_focus.clone();
                let mut menu = PopupMenu::new(cx).item(PopupMenuItem::new("Open").on_click(
                    move |_, window, cx| {
                        if moves_focus {
                            target_focus.focus(window, cx);
                        }
                    },
                ));
                menu.previous_focus_handle = Some(previous_focus.clone());
                menu.focus_handle.focus(window, cx);
                menu
            });

            cx.update(|window, cx| {
                menu.update(cx, |menu, cx| {
                    menu.selected_index = Some(0);
                    menu.confirm(&Confirm { secondary: false }, window, cx);
                });
                assert_eq!(
                    window.focused(cx),
                    Some(if moves_focus {
                        target_focus
                    } else {
                        previous_focus
                    })
                );
            });
        }
    }

    #[gpui::test]
    fn stepper_updates_live_with_keyboard_and_clicks_without_dismissing(
        cx: &mut gpui::TestAppContext,
    ) {
        use std::cell::Cell;
        cx.update(crate::init);
        let value = Rc::new(Cell::new(100));
        let read = Rc::clone(&value);
        let decrement = Rc::clone(&value);
        let reset = Rc::clone(&value);
        let increment = Rc::clone(&value);
        let (menu, cx) = cx.add_window_view(|window, cx| {
            let mut menu = PopupMenu::new(cx).item(PopupMenuItem::stepper(
                "Page zoom",
                move |_| format!("{}%", read.get()).into(),
                move |_, _| decrement.set(decrement.get() - 10),
                move |_, _| reset.set(100),
                move |_, _| increment.set(increment.get() + 10),
            ));
            menu.selected_index = Some(0);
            menu.focus_handle.focus(window, cx);
            menu
        });
        let dismissed = Rc::new(Cell::new(false));
        let did_dismiss = Rc::clone(&dismissed);
        let _subscription = cx.update(|window, cx| {
            window.subscribe(&menu, cx, move |_, _: &DismissEvent, _, _| {
                did_dismiss.set(true)
            })
        });
        cx.simulate_keystrokes("right");
        cx.simulate_keystrokes("right");
        assert_eq!(value.get(), 120);
        cx.simulate_keystrokes("left");
        assert_eq!(value.get(), 110);
        cx.simulate_keystrokes("enter");
        assert_eq!(value.get(), 100);
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let bounds = cx.debug_bounds("menu-step-up-0").expect("increment button");
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        assert_eq!(value.get(), 110);
        assert!(!dismissed.get());
        menu.read_with(cx, |menu, cx| {
            let PopupMenuItem::Stepper { value, .. } = &menu.menu_items[0] else {
                panic!("stepper row")
            };
            assert_eq!(value(cx).as_ref(), "110%");
        });
        cx.simulate_keystrokes("escape");
        assert!(dismissed.get());
    }

    #[gpui::test]
    fn popup_menu_item_a11y_label_uses_visible_label(cx: &mut gpui::TestAppContext) {
        let submenu = cx.update(|cx| cx.new(|cx| PopupMenu::new(cx)));

        assert_eq!(PopupMenuItem::new("Open").a11y_label(), Some("Open".into()));
        assert_eq!(
            PopupMenuItem::link("Docs", "https://example.com").a11y_label(),
            Some("Docs".into())
        );
        assert_eq!(
            PopupMenuItem::label("Recent files").a11y_label(),
            Some("Recent files".into())
        );
        assert_eq!(
            PopupMenuItem::submenu("More", submenu).a11y_label(),
            Some("More".into())
        );
        assert_eq!(PopupMenuItem::separator().a11y_label(), None);
        assert_eq!(PopupMenuItem::element(|_, _, _| div()).a11y_label(), None);
    }
}
