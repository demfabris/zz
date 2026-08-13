use std::rc::Rc;

use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Selectable as _,
    Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputContentType, InputState},
    menu::{PopupMenu, PopupMenuItem},
    tag::Tag,
    tooltip::Tooltip,
};
use gpui::{
    AnyElement, App, BoxShadow, Context, Entity, Focusable as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, RenderOnce, SharedString, Styled as _, Window, div, point,
    prelude::*, px,
};

/// The 40px browser toolbar. Behavior comes through the child slots.
#[derive(IntoElement)]
pub struct BrowserToolbar {
    back: AnyElement,
    forward: AnyElement,
    reload: AnyElement,
    address: AnyElement,
    picker: AnyElement,
    more: AnyElement,
}

impl BrowserToolbar {
    /// Fixed toolbar height. A host mounting the toolbar behind a cached-view
    /// boundary must give the wrapper this exact height.
    pub const HEIGHT: Pixels = px(40.0);

    pub fn new(
        back: impl IntoElement,
        forward: impl IntoElement,
        reload: impl IntoElement,
        address: impl IntoElement,
        picker: impl IntoElement,
        more: impl IntoElement,
    ) -> Self {
        Self {
            back: back.into_any_element(),
            forward: forward.into_any_element(),
            reload: reload.into_any_element(),
            address: address.into_any_element(),
            picker: picker.into_any_element(),
            more: more.into_any_element(),
        }
    }
}

impl RenderOnce for BrowserToolbar {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        div()
            .h(Self::HEIGHT)
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .text_color(cx.theme().foreground)
            .child(
                browser_toolbar_cluster()
                    .child(self.back)
                    .child(self.forward)
                    .child(self.reload),
            )
            .child(self.address)
            .child(
                browser_toolbar_cluster()
                    .child(self.picker)
                    .child(self.more),
            )
    }
}

fn browser_toolbar_cluster() -> gpui::Div {
    div().flex().items_center().gap_1()
}

pub fn browser_address(address: &Entity<InputState>, cx: &App) -> gpui::Div {
    let focus_handle = address.read(cx).focus_handle(cx);
    let input = Input::new(address)
        .xsmall()
        .flex_1()
        .min_w_0()
        .px_2()
        .rounded(cx.theme().radius)
        .bg(cx.theme().background.washed(1))
        .bordered(false)
        .focus_bordered(false)
        .content_type(InputContentType::Url);

    div()
        .h_full()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            focus_handle.focus(window, cx);
        })
        .child(input)
}

/// One tab in the compact tab strip. The active tab renders as the address bar
/// itself, Safari-compact style, so its `label`/`detail` go unused.
#[derive(Clone, PartialEq, Eq)]
pub struct BrowserTabInfo {
    /// Stable per-pane tab id, chosen by the caller.
    pub id: u64,
    /// Short pill text, typically the page host.
    pub label: SharedString,
    /// Pill tooltip, typically the page title or the full URL.
    pub detail: SharedString,
}

impl BrowserTabInfo {
    pub fn new(id: u64, label: impl Into<SharedString>, detail: impl Into<SharedString>) -> Self {
        Self {
            id,
            label: label.into(),
            detail: detail.into(),
        }
    }
}

type BrowserTabAction = Rc<dyn Fn(u64, &mut Window, &mut App)>;

/// Safari-compact tab row for the toolbar's address slot: the address bar is
/// the active tab, every other tab collapses into a pill beside it, and a
/// new-tab button closes the row.
#[derive(IntoElement)]
pub struct BrowserTabStrip {
    address: Entity<InputState>,
    tabs: Vec<BrowserTabInfo>,
    active: usize,
    on_address_mouse_down: BrowserMenuAction,
    on_activate: BrowserTabAction,
    on_close: BrowserTabAction,
    on_new_tab: BrowserMenuAction,
}

impl BrowserTabStrip {
    pub fn new(address: &Entity<InputState>, tabs: Vec<BrowserTabInfo>, active: usize) -> Self {
        Self {
            address: address.clone(),
            tabs,
            active,
            on_address_mouse_down: Rc::new(|_, _| {}),
            on_activate: Rc::new(|_, _, _| {}),
            on_close: Rc::new(|_, _, _| {}),
            on_new_tab: Rc::new(|_, _| {}),
        }
    }

    #[must_use]
    pub fn on_address_mouse_down(
        mut self,
        action: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_address_mouse_down = Rc::new(action);
        self
    }

    #[must_use]
    pub fn on_activate(mut self, action: impl Fn(u64, &mut Window, &mut App) + 'static) -> Self {
        self.on_activate = Rc::new(action);
        self
    }

    #[must_use]
    pub fn on_close(mut self, action: impl Fn(u64, &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Rc::new(action);
        self
    }

    #[must_use]
    pub fn on_new_tab(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_new_tab = Rc::new(action);
        self
    }
}

impl RenderOnce for BrowserTabStrip {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        let Self {
            address,
            tabs,
            active,
            on_address_mouse_down,
            on_activate,
            on_close,
            on_new_tab,
        } = self;
        let new_tab = browser_toolbar_button(
            cx,
            "browser-new-tab",
            IconName::Plus,
            "New tab",
            false,
            false,
        )
        .on_click(move |_, window, cx| on_new_tab(window, cx));
        let closable = tabs.len() > 1;
        let mut address = Some(address);
        let mut row = div()
            .h_full()
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .items_center()
            .gap_1();
        for (index, tab) in tabs.into_iter().enumerate() {
            let tab_id = tab.id;
            row = if index == active {
                row.children(address.take().map(|address| {
                    browser_active_tab(
                        &address,
                        tab_id,
                        closable,
                        &on_address_mouse_down,
                        &on_close,
                        cx,
                    )
                }))
            } else {
                row.child(browser_tab_pill(tab, &on_activate, &on_close, cx))
            };
        }
        row.children(address.take().map(|address| browser_address(&address, cx)))
            .child(new_tab)
    }
}

fn browser_active_tab(
    address: &Entity<InputState>,
    id: u64,
    closable: bool,
    on_address_mouse_down: &BrowserMenuAction,
    on_close: &BrowserTabAction,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let focus_handle = address.read(cx).focus_handle(cx);
    let mouse_down = Rc::clone(on_address_mouse_down);
    let input = Input::new(address)
        .xsmall()
        .flex_1()
        .min_w_0()
        .px_0()
        .appearance(false)
        .content_type(InputContentType::Url);
    let content = div()
        .h_full()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            mouse_down(window, cx);
            focus_handle.focus(window, cx);
            cx.stop_propagation();
        })
        .child(input);
    browser_tab_shell(id, content, closable, on_close, cx)
}

fn browser_tab_pill(
    tab: BrowserTabInfo,
    on_activate: &BrowserTabAction,
    on_close: &BrowserTabAction,
    cx: &App,
) -> impl IntoElement {
    let BrowserTabInfo { id, label, detail } = tab;
    let activate = Rc::clone(on_activate);
    let close_middle = Rc::clone(on_close);
    let content = div()
        .flex_1()
        .min_w_0()
        .text_xs()
        .text_color(cx.theme().foreground.muted())
        .whitespace_nowrap()
        .text_ellipsis()
        .overflow_hidden()
        .child(label);
    browser_tab_shell(id, content, true, on_close, cx)
        .cursor_pointer()
        .when(!detail.is_empty(), |this| {
            this.tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
        })
        .on_click(move |_, window, cx| activate(id, window, cx))
        .on_mouse_down(MouseButton::Middle, move |_, window, cx| {
            close_middle(id, window, cx);
        })
}

fn browser_tab_shell(
    id: u64,
    content: impl IntoElement,
    closable: bool,
    on_close: &BrowserTabAction,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let rest = cx.theme().background.washed(1);
    div()
        .id(("browser-tab", id))
        .debug_selector(move || format!("browser-tab-{id}"))
        .group(browser_tab_group(id))
        .h(px(24.0))
        .flex()
        .flex_1()
        .items_center()
        .gap(px(2.0))
        .pl(px(10.0))
        .pr(px(4.0))
        .min_w(px(40.0))
        .rounded(cx.theme().radius)
        .bg(rest)
        .hover(move |style| style.bg(rest.hover()))
        .child(content)
        .when(closable, |this| {
            this.child(browser_tab_close_button(id, on_close))
        })
}

fn browser_tab_group(id: u64) -> SharedString {
    format!("browser-tab-{id}").into()
}

fn browser_tab_close_button(id: u64, on_close: &BrowserTabAction) -> impl IntoElement {
    let close = Rc::clone(on_close);
    div()
        .flex_none()
        .invisible()
        .group_hover(browser_tab_group(id), gpui::Styled::visible)
        .child(
            Button::new(("browser-tab-close", id))
                .ghost()
                .with_size(px(18.0))
                .icon(IconName::Close)
                .debug_selector(move || format!("browser-tab-close-{id}"))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(id, window, cx);
                }),
        )
}

/// The compact toolbar button: a ghost icon button that is its own 24px hover
/// target, with every state owned by the theme.
pub fn browser_toolbar_button(
    cx: &App,
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    disabled: bool,
    selected: bool,
) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(icon)
        .when(!disabled, |this| {
            this.text_color(cx.theme().foreground.muted())
        })
        .disabled(disabled)
        .selected(selected)
        .tooltip(tooltip)
}

/// Blank-browser hint shown before any recent pages exist.
#[derive(IntoElement)]
pub struct BrowserEmptyHint;

impl RenderOnce for BrowserEmptyHint {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        div()
            .max_w(px(440.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.0))
            .p(px(20.0))
            .child(
                div()
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(cx.theme().background.washed(2))
                    .child(
                        Icon::new(IconName::Globe)
                            .xsmall()
                            .text_color(cx.theme().foreground.muted()),
                    ),
            )
            .child(
                div()
                    .text_center()
                    .text_size(crate::rems_from_px(13.0))
                    .font_semibold()
                    .child("Where to?"),
            )
            .child(
                div()
                    .text_center()
                    .text_size(crate::rems_from_px(11.0))
                    .text_color(cx.theme().foreground.muted())
                    .child("Type a URL to get started. Pages you visit will show up here."),
            )
    }
}

pub fn browser_recent_row(
    id: impl Into<gpui::ElementId>,
    url: impl Into<SharedString>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let rest = cx.theme().background.washed(1);
    let highlight = crate::navigation::workspace_row_highlight(cx);
    let url = url.into();
    div()
        .id(id)
        .flex()
        .w_full()
        .h(px(32.0))
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .rounded(cx.theme().radius)
        .bg(rest)
        .cursor_pointer()
        .hover(move |style| style.bg(highlight))
        .text_size(crate::rems_from_px(12.0))
        .font_medium()
        .child(
            Icon::new(IconName::Globe)
                .xsmall()
                .flex_none()
                .text_color(cx.theme().foreground.muted()),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .whitespace_nowrap()
                .text_ellipsis()
                .overflow_hidden()
                .child(url),
        )
}

#[derive(Clone)]
pub struct BrowserMenuProfile {
    pub id: SharedString,
    pub label: SharedString,
}

impl BrowserMenuProfile {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum BrowserProfileDiscoveryState {
    Loading,
    #[default]
    Ready,
    Failed,
}

#[derive(Clone)]
pub struct BrowserActionMenuState {
    pub current_profile_label: SharedString,
    pub selected_profile: SharedString,
    pub default_profile: SharedString,
    pub profiles: Vec<BrowserMenuProfile>,
    pub profile_discovery: BrowserProfileDiscoveryState,
    pub zoom_percent: u16,
    pub can_import_chrome_data: bool,
    pub can_clear_site_data: bool,
    pub picker_active: bool,
}

type BrowserMenuAction = Rc<dyn Fn(&mut Window, &mut App)>;
type BrowserProfileAction = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

#[derive(Clone)]
#[must_use]
pub struct BrowserMenuActions {
    open_url: BrowserMenuAction,
    copy_url: BrowserMenuAction,
    switch_profile: BrowserProfileAction,
    refresh_profiles: BrowserMenuAction,
    zoom_in: BrowserMenuAction,
    zoom_out: BrowserMenuAction,
    reset_zoom: BrowserMenuAction,
    import_chrome_data: BrowserProfileAction,
    import_cookies: BrowserMenuAction,
    clear_site_data: BrowserMenuAction,
    reload: BrowserMenuAction,
    toggle_picker: BrowserMenuAction,
    dev_tools: BrowserMenuAction,
}

impl Default for BrowserMenuActions {
    fn default() -> Self {
        let noop: BrowserMenuAction = Rc::new(|_, _| {});
        Self {
            open_url: Rc::clone(&noop),
            copy_url: Rc::clone(&noop),
            switch_profile: Rc::new(|_, _, _| {}),
            refresh_profiles: Rc::clone(&noop),
            zoom_in: Rc::clone(&noop),
            zoom_out: Rc::clone(&noop),
            reset_zoom: Rc::clone(&noop),
            import_chrome_data: Rc::new(|_, _, _| {}),
            import_cookies: Rc::clone(&noop),
            clear_site_data: Rc::clone(&noop),
            reload: Rc::clone(&noop),
            toggle_picker: Rc::clone(&noop),
            dev_tools: noop,
        }
    }
}

impl BrowserMenuActions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_url(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.open_url = Rc::new(action);
        self
    }

    pub fn copy_url(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.copy_url = Rc::new(action);
        self
    }

    pub fn switch_profile(
        mut self,
        action: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.switch_profile = Rc::new(action);
        self
    }

    pub fn refresh_profiles(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.refresh_profiles = Rc::new(action);
        self
    }

    pub fn zoom_in(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.zoom_in = Rc::new(action);
        self
    }

    pub fn zoom_out(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.zoom_out = Rc::new(action);
        self
    }

    pub fn reset_zoom(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.reset_zoom = Rc::new(action);
        self
    }

    pub fn import_chrome_data(
        mut self,
        action: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.import_chrome_data = Rc::new(action);
        self
    }

    pub fn import_cookies(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.import_cookies = Rc::new(action);
        self
    }

    pub fn clear_site_data(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.clear_site_data = Rc::new(action);
        self
    }

    pub fn reload(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.reload = Rc::new(action);
        self
    }

    pub fn toggle_picker(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.toggle_picker = Rc::new(action);
        self
    }

    pub fn dev_tools(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.dev_tools = Rc::new(action);
        self
    }
}

/// The browser action-menu hierarchy. The caller provides state and callbacks.
// Both by value: their fields move into the item callbacks.
#[allow(clippy::needless_pass_by_value)]
pub fn browser_action_menu(
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    state: BrowserActionMenuState,
    actions: BrowserMenuActions,
) -> PopupMenu {
    let open_url = Rc::clone(&actions.open_url);
    let copy_url = Rc::clone(&actions.copy_url);
    let menu = menu
        .min_w(px(250.0))
        .item(
            PopupMenuItem::new("Open in default browser")
                .icon(IconName::ExternalLink)
                .on_click(move |_, window, cx| open_url(window, cx)),
        )
        .item(
            PopupMenuItem::new("Copy URL")
                .icon(IconName::Copy)
                .on_click(move |_, window, cx| copy_url(window, cx)),
        )
        .separator()
        .item(
            PopupMenuItem::new(format!("Profile · {}", state.current_profile_label))
                .icon(IconName::CircleUser)
                .disabled(true),
        );

    let selected_profile = state.selected_profile.clone();
    let default_profile = state.default_profile.clone();
    let profiles = state.profiles.clone();
    let profile_discovery = state.profile_discovery;
    let profile_action = Rc::clone(&actions.switch_profile);
    let refresh_profiles = Rc::clone(&actions.refresh_profiles);
    let menu = menu.submenu_with_icon(
        Some(Icon::new(IconName::User)),
        "Switch profile",
        window,
        cx,
        move |profile_menu, _, _| {
            let default_action = Rc::clone(&profile_action);
            let default_id = default_profile.clone();
            let mut profile_menu = profile_menu
                .min_w(px(310.0))
                .item(
                    PopupMenuItem::new("Default zz profile")
                        .checked(selected_profile == default_profile)
                        .disabled(selected_profile == default_profile)
                        .on_click(move |_, window, cx| {
                            default_action(default_id.clone(), window, cx);
                        }),
                )
                .separator()
                .item(PopupMenuItem::label(
                    "Chrome profiles · isolated zz storage",
                ));

            if profile_discovery == BrowserProfileDiscoveryState::Loading {
                profile_menu = profile_menu
                    .item(PopupMenuItem::new("Finding Chrome profiles…").disabled(true));
            } else if profiles.is_empty()
                && profile_discovery == BrowserProfileDiscoveryState::Ready
            {
                profile_menu = profile_menu
                    .item(PopupMenuItem::new("No Chrome profiles found").disabled(true));
            } else {
                for profile in &profiles {
                    let action = Rc::clone(&profile_action);
                    let id = profile.id.clone();
                    profile_menu = profile_menu.item(
                        PopupMenuItem::new(profile.label.clone())
                            .checked(selected_profile == profile.id)
                            .disabled(selected_profile == profile.id)
                            .on_click(move |_, window, cx| {
                                action(id.clone(), window, cx);
                            }),
                    );
                }
            }
            if profile_discovery == BrowserProfileDiscoveryState::Failed {
                let refresh_profiles = Rc::clone(&refresh_profiles);
                profile_menu = profile_menu.separator().item(
                    PopupMenuItem::new("Retry Chrome profile discovery")
                        .icon(IconName::Redo2)
                        .on_click(move |_, window, cx| refresh_profiles(window, cx)),
                );
            }
            profile_menu
        },
    );

    let zoom_in = Rc::clone(&actions.zoom_in);
    let zoom_out = Rc::clone(&actions.zoom_out);
    let reset_zoom = Rc::clone(&actions.reset_zoom);
    let import_chrome_data = Rc::clone(&actions.import_chrome_data);
    let import_cookies = Rc::clone(&actions.import_cookies);
    let clear_site_data = Rc::clone(&actions.clear_site_data);
    let reload = Rc::clone(&actions.reload);
    let toggle_picker = Rc::clone(&actions.toggle_picker);
    let dev_tools = Rc::clone(&actions.dev_tools);
    let menu = menu
        .separator()
        .item(PopupMenuItem::new(format!("Page zoom · {}%", state.zoom_percent)).disabled(true))
        .item(
            PopupMenuItem::new("Zoom in")
                .icon(IconName::Plus)
                .on_click(move |_, window, cx| zoom_in(window, cx)),
        )
        .item(
            PopupMenuItem::new("Zoom out")
                .icon(IconName::Minus)
                .on_click(move |_, window, cx| zoom_out(window, cx)),
        )
        .item(
            PopupMenuItem::new("Reset zoom")
                .disabled(state.zoom_percent == 100)
                .on_click(move |_, window, cx| reset_zoom(window, cx)),
        );

    let import_profiles = state.profiles.clone();
    let import_profile_discovery = state.profile_discovery;
    let can_import_chrome_data = state.can_import_chrome_data;
    let refresh_profiles = Rc::clone(&actions.refresh_profiles);
    let menu = menu.separator().submenu_with_icon(
        Some(Icon::new(IconName::Globe)),
        "Import Chrome data",
        window,
        cx,
        move |import_menu, _, _| {
            let mut import_menu = import_menu
                .min_w(px(310.0))
                .item(PopupMenuItem::label("Cookies and history · source profile"));
            if !can_import_chrome_data {
                return import_menu
                    .item(PopupMenuItem::new("Not supported on this platform").disabled(true));
            }
            if import_profile_discovery == BrowserProfileDiscoveryState::Loading {
                import_menu =
                    import_menu.item(PopupMenuItem::new("Finding Chrome profiles…").disabled(true));
            } else if import_profiles.is_empty()
                && import_profile_discovery == BrowserProfileDiscoveryState::Ready
            {
                import_menu =
                    import_menu.item(PopupMenuItem::new("No Chrome profiles found").disabled(true));
            } else {
                for profile in &import_profiles {
                    let action = Rc::clone(&import_chrome_data);
                    let id = profile.id.clone();
                    import_menu = import_menu.item(
                        PopupMenuItem::new(profile.label.clone()).on_click(move |_, window, cx| {
                            action(id.clone(), window, cx);
                        }),
                    );
                }
            }
            if import_profile_discovery == BrowserProfileDiscoveryState::Failed {
                let refresh_profiles = Rc::clone(&refresh_profiles);
                import_menu = import_menu.separator().item(
                    PopupMenuItem::new("Retry Chrome profile discovery")
                        .icon(IconName::Redo2)
                        .on_click(move |_, window, cx| refresh_profiles(window, cx)),
                );
            }
            import_menu
        },
    );

    menu.item(
        PopupMenuItem::new("Import cookie file…")
            .icon(IconName::File)
            .on_click(move |_, window, cx| import_cookies(window, cx)),
    )
    .item(
        PopupMenuItem::new("Clear site data…")
            .icon(IconName::Delete)
            .disabled(!state.can_clear_site_data)
            .on_click(move |_, window, cx| clear_site_data(window, cx)),
    )
    .separator()
    .item(
        PopupMenuItem::new("Reload")
            .icon(IconName::Redo2)
            .on_click(move |_, window, cx| reload(window, cx)),
    )
    .item(
        PopupMenuItem::new(if state.picker_active {
            "Cancel element picker"
        } else {
            "Pick an element"
        })
        .icon(IconName::Inspector)
        .on_click(move |_, window, cx| toggle_picker(window, cx)),
    )
    .item(
        PopupMenuItem::new("Developer tools")
            .icon(IconName::SquareTerminal)
            .on_click(move |_, window, cx| dev_tools(window, cx)),
    )
}

/// Browser recovery card. The caller supplies the optional retry control.
#[derive(IntoElement)]
pub struct BrowserErrorPanel {
    message: SharedString,
    retry: Option<AnyElement>,
}

impl BrowserErrorPanel {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            retry: None,
        }
    }

    #[must_use]
    pub fn retry(mut self, retry: impl IntoElement) -> Self {
        self.retry = Some(retry.into_any_element());
        self
    }
}

impl RenderOnce for BrowserErrorPanel {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        div()
            .max_w(px(440.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.0))
            .p(px(20.0))
            .rounded(cx.theme().radius)
            .bg(cx.theme().background.washed(1))
            .text_color(cx.theme().foreground)
            .shadow(browser_surface_shadow(cx))
            .child(
                div()
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(cx.theme().danger.fill())
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .xsmall()
                            .text_color(cx.theme().danger),
                    ),
            )
            .child(
                div()
                    .text_center()
                    .text_size(crate::rems_from_px(13.0))
                    .font_semibold()
                    .child("This page couldn’t load"),
            )
            .child(
                div()
                    .max_w(px(400.0))
                    .text_center()
                    .text_size(crate::rems_from_px(11.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(self.message),
            )
            .children(self.retry)
    }
}

/// Status pill overlaid while the browser element picker is active.
#[derive(IntoElement)]
pub struct BrowserPickStatus {
    message: SharedString,
}

impl BrowserPickStatus {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl RenderOnce for BrowserPickStatus {
    fn render(self, _: &mut gpui::Window, _: &mut App) -> impl IntoElement {
        div()
            .absolute()
            .left(px(16.0))
            .right(px(16.0))
            .bottom(px(14.0))
            .flex()
            .justify_center()
            .child(
                Tag::primary()
                    .outline()
                    .small()
                    .rounded_full()
                    .gap(px(6.0))
                    .child(Icon::new(IconName::Inspector).xsmall())
                    .child(self.message),
            )
    }
}

pub fn browser_surface_shadow(cx: &App) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: cx.theme().border.subtle(),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(1.0),
            inset: false,
        },
        BoxShadow {
            color: cx.theme().scrim,
            offset: point(px(0.0), px(8.0)),
            blur_radius: px(24.0),
            spread_radius: px(-8.0),
            inset: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gpui::{Context, Modifiers, Render, TestAppContext, VisualTestContext};

    use super::*;

    struct BrowserTabStripTest {
        address: Entity<InputState>,
        active: usize,
        activated: Arc<Mutex<Vec<u64>>>,
        closed: Arc<Mutex<Vec<u64>>>,
    }

    impl Render for BrowserTabStripTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let activated = Arc::clone(&self.activated);
            let closed = Arc::clone(&self.closed);
            BrowserTabStrip::new(
                &self.address,
                vec![
                    BrowserTabInfo::new(1, "Active", "Active tab"),
                    BrowserTabInfo::new(2, "Inactive", "Inactive tab"),
                ],
                self.active,
            )
            .on_activate(move |id, _, _| activated.lock().unwrap().push(id))
            .on_close(move |id, _, _| closed.lock().unwrap().push(id))
        }
    }

    #[gpui::test]
    fn active_and_inactive_tabs_reveal_working_close_buttons_on_hover(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let activated = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(Mutex::new(Vec::new()));
        let activated_for_view = Arc::clone(&activated);
        let closed_for_view = Arc::clone(&closed);
        let (_, cx) = cx.add_window_view(move |window, cx| BrowserTabStripTest {
            address: cx.new(|cx| InputState::new(window, cx)),
            active: 0,
            activated: activated_for_view,
            closed: closed_for_view,
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let active_tab = cx
            .debug_bounds("browser-tab-1")
            .expect("the active tab renders");
        let inactive_tab = cx
            .debug_bounds("browser-tab-2")
            .expect("the inactive tab renders");

        cx.simulate_mouse_move(active_tab.center(), None, Modifiers::none());
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let active = cx
            .debug_bounds("browser-tab-close-1")
            .expect("the active tab reveals its close button on hover");
        assert!(
            active.origin.x >= active_tab.origin.x
                && active.right() <= active_tab.right()
                && active.origin.y >= active_tab.origin.y
                && active.bottom() <= active_tab.bottom(),
            "the active close button must stay inside its tab surface"
        );
        cx.simulate_click(active.center(), Modifiers::none());

        cx.simulate_mouse_move(inactive_tab.center(), None, Modifiers::none());
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let inactive = cx
            .debug_bounds("browser-tab-close-2")
            .expect("the inactive tab reveals its close button on hover");
        assert_eq!(active.size, inactive.size);
        assert_eq!(
            active_tab.right() - active.right(),
            inactive_tab.right() - inactive.right()
        );
        assert_eq!(
            active.origin.y - active_tab.origin.y,
            inactive.origin.y - inactive_tab.origin.y
        );
        cx.simulate_click(inactive.center(), Modifiers::none());

        assert_eq!(*closed.lock().unwrap(), [1, 2]);
        assert!(activated.lock().unwrap().is_empty());
    }

    #[gpui::test]
    fn switching_active_tab_preserves_equal_tab_widths(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(move |window, cx| BrowserTabStripTest {
            address: cx.new(|cx| InputState::new(window, cx)),
            active: 0,
            activated: Arc::default(),
            closed: Arc::default(),
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let first_before = cx.debug_bounds("browser-tab-1").expect("first tab renders");
        let second_before = cx
            .debug_bounds("browser-tab-2")
            .expect("second tab renders");
        assert_eq!(first_before.size.width, second_before.size.width);

        view.update(cx, |view, cx| {
            view.active = 1;
            cx.notify();
        });
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        let first_after = cx.debug_bounds("browser-tab-1").expect("first tab renders");
        let second_after = cx
            .debug_bounds("browser-tab-2")
            .expect("second tab renders");
        assert_eq!(first_after.size.width, second_after.size.width);
        assert_eq!(first_before.size.width, first_after.size.width);
        assert_eq!(second_before.size.width, second_after.size.width);
    }
}
