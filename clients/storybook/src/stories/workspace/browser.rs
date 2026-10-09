use zpui::{
    AnyElement, App, AppContext as _, Entity, Focusable as _, IntoElement, ParentElement as _,
    Styled as _, Window, div, prelude::*, px,
};
use zz_protocol::PaneId;
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, IconName, Sizable as _,
    browser::{
        BrowserActionMenuState, BrowserEmptyHint, BrowserErrorPanel, BrowserHeader,
        BrowserMenuActions, BrowserMenuProfile, BrowserPickStatus, BrowserProfileDiscoveryState,
        BrowserSiteMenuState, BrowserTabInfo, BrowserTabStrip, BrowserToolbar, browser_action_menu,
        browser_address, browser_omnibox_panel, browser_omnibox_row, browser_recent_row,
        browser_site_controls_button, browser_site_menu, browser_start_surface,
        browser_toolbar_button,
    },
    button::{Button, ButtonVariants as _},
    input::InputState,
    menu::{DropdownMenu as _, PopupMenu},
    pane::{pane_drag_button, pane_header_icon_button, pane_surface},
};

use super::fixtures::{chrome, radii, stateful};
use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "browser",
    name: "Browser chrome",
    group: "Browser",
    summary: "Everything a browser pane draws around the Chromium page: the tab row, the toolbar and address field, the omnibox, the start page, error and picker states, and the two menus.",
    sections: &[
        Section {
            id: "header",
            name: "Header",
            summary: "BrowserHeader: the tab row with the pane's split, drag and close actions, over the navigation toolbar. An inactive pane hides its actions until hover.",
            build: |window, cx| stateful(window, cx, inputs::<4>, headers),
        },
        Section {
            id: "toolbar",
            name: "Toolbar",
            summary: "browser_toolbar_button states, and BrowserToolbar with history disabled, the element picker on, and the plain-text address the web client shows without Chromium.",
            build: |window, cx| stateful(window, cx, inputs::<2>, toolbars),
        },
        Section {
            id: "address",
            name: "Address field",
            summary: "browser_address: the site controls button inside a borderless URL input. Long URLs scroll inside the field.",
            build: |window, cx| stateful(window, cx, inputs::<4>, addresses),
        },
        Section {
            id: "address-focused",
            name: "Address field, focused",
            summary: "With focus the field fills and takes the control highlight.",
            build: |window, cx| stateful(window, cx, focused_input, focused_address),
        },
        Section {
            id: "tabs",
            name: "Tab strip",
            summary: "BrowserTabStrip. Tabs share one width between 112 and 180px and scroll sideways once they run out of room, keeping the active tab in view. Close buttons appear on hover when there is more than one tab.",
            build: |_, cx| stateless(tabs, cx),
        },
        Section {
            id: "omnibox",
            name: "Omnibox",
            summary: "browser_omnibox_panel floats under the header with suggestions from history. The keyboard selection uses the row highlight.",
            build: |window, cx| stateful(window, cx, inputs::<1>, omnibox),
        },
        Section {
            id: "start-page",
            name: "Start page",
            summary: "browser_start_surface covers a blank tab: a hint before any history exists, then the recent pages.",
            build: |_, cx| stateless(start_page, cx),
        },
        Section {
            id: "error-and-picker",
            name: "Error and element picker",
            summary: "BrowserErrorPanel when a page fails, with a retry when the failure is recoverable, and BrowserPickStatus while the element picker is on.",
            build: |_, cx| stateless(error_and_picker, cx),
        },
        Section {
            id: "action-menu",
            name: "Action menu",
            summary: "browser_action_menu from the toolbar's more button: open and copy the URL, switch profile, page zoom, Chrome import, site data, the picker and developer tools.",
            build: |window, cx| stateful(window, cx, action_menus, menus),
        },
        Section {
            id: "site-menu",
            name: "Site menu",
            summary: "browser_site_menu from the button at the start of the address field: connection security, sound, and clearing site data.",
            build: |window, cx| stateful(window, cx, site_menus, menus),
        },
    ],
};

const URLS: [&str; 4] = [
    "https://zed.dev/docs/key-bindings",
    "https://github.com/demfabris/zz/pull/412",
    "http://localhost:4321/",
    "https://docs.rs/zpui/latest/zpui/struct.Window.html#method.use_keyed_state",
];

fn inputs<const N: usize>(window: &mut Window, cx: &mut App) -> [Entity<InputState>; N] {
    std::array::from_fn(|index| address_input(URLS[index % URLS.len()], window, cx))
}

fn address_input(url: &str, window: &mut Window, cx: &mut App) -> Entity<InputState> {
    let url = url.to_owned();
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Search or enter address")
            .default_value(url)
    })
}

fn focused_input(window: &mut Window, cx: &mut App) -> Entity<InputState> {
    let input = address_input(URLS[0], window, cx);
    input.read(cx).focus_handle(cx).focus(window, cx);
    input
}

fn site_controls(cx: &App) -> impl IntoElement + use<> {
    browser_site_controls_button(cx).dropdown_menu(|menu, _, _| {
        browser_site_menu(
            menu,
            site_state(Some(true), Some(false), true),
            |_, _| {},
            |_, _| {},
        )
    })
}

fn menu_state(
    discovery: BrowserProfileDiscoveryState,
    profiles: bool,
    picker_active: bool,
) -> BrowserActionMenuState {
    BrowserActionMenuState {
        current_profile_label: "Default zz profile".into(),
        selected_profile: "default".into(),
        default_profile: "default".into(),
        profiles: if profiles {
            vec![
                BrowserMenuProfile::new("chrome-default", "Personal · Default"),
                BrowserMenuProfile::new("chrome-profile-1", "Work · Profile 1"),
            ]
        } else {
            Vec::new()
        },
        profile_discovery: discovery,
        zoom_percent: 110,
        can_import_chrome_data: true,
        can_clear_site_data: true,
        picker_active,
    }
}

fn site_state(secure: Option<bool>, muted: Option<bool>, clear: bool) -> BrowserSiteMenuState {
    BrowserSiteMenuState {
        site: "zed.dev".into(),
        connection_secure: secure,
        audio_muted: muted,
        can_clear_site_data: clear,
    }
}

pub fn toolbar(
    address: AnyElement,
    back: bool,
    forward: bool,
    picking: bool,
    cx: &App,
) -> BrowserToolbar {
    BrowserToolbar::new(
        browser_toolbar_button(
            cx,
            "browser-back",
            IconName::ArrowLeft,
            "Back",
            !back,
            false,
        ),
        browser_toolbar_button(
            cx,
            "browser-forward",
            IconName::ArrowRight,
            "Forward",
            !forward,
            false,
        ),
        browser_toolbar_button(
            cx,
            "browser-reload",
            IconName::Redo2,
            "Reload",
            false,
            false,
        ),
        address,
        browser_toolbar_button(
            cx,
            "browser-element-picker",
            IconName::Inspector,
            if picking {
                "Cancel element picker"
            } else {
                "Pick an element"
            },
            false,
            picking,
        ),
        browser_toolbar_button(
            cx,
            "browser-more",
            IconName::EllipsisVertical,
            "More browser actions",
            false,
            false,
        )
        .dropdown_menu_with_anchor(zpui::Anchor::TopRight, move |menu, window, cx| {
            browser_action_menu(
                menu,
                window,
                cx,
                menu_state(BrowserProfileDiscoveryState::Ready, true, picking),
                BrowserMenuActions::new(),
            )
        }),
    )
}

fn pane_actions(pane: u64, cx: &App) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .child(
            pane_header_icon_button("Split bottom", IconName::PanelBottom, true, cx)
                .tooltip("Split bottom"),
        )
        .child(
            pane_header_icon_button("Split right", IconName::PanelRight, true, cx)
                .tooltip("Split right"),
        )
        .child(pane_drag_button(
            ("browser-drag", pane),
            PaneId(pane),
            "zed.dev".into(),
            true,
            |_, _, _| {},
            cx,
        ))
        .child(
            pane_header_icon_button("Close pane", IconName::Xmark, true, cx).tooltip("Close pane"),
        )
}

fn tab_infos(count: usize) -> Vec<BrowserTabInfo> {
    [
        ("zed.dev", "Key bindings - Zed"),
        (
            "github.com",
            "Flatten zpui into crates/ · Pull Request #412",
        ),
        ("localhost:4321", "zz, a terminal multiplexer"),
        ("docs.rs", "Window in zpui - Rust"),
        ("news.ycombinator.com", "Hacker News"),
        ("crates.io", "crates.io: Rust Package Registry"),
        ("New tab", "about:blank"),
        ("wgpu.rs", "wgpu: portable graphics"),
        ("ghostty.org", "Ghostty"),
    ]
    .into_iter()
    .take(count)
    .enumerate()
    .map(|(index, (label, detail))| BrowserTabInfo::new(index as u64 + 1, label, detail))
    .collect()
}

pub fn header(
    active: bool,
    tabs: usize,
    active_tab: usize,
    address: &Entity<InputState>,
    show_separator: bool,
    cx: &App,
) -> BrowserHeader {
    BrowserHeader::new(
        active,
        BrowserTabStrip::new(tab_infos(tabs), active_tab),
        pane_actions(1, cx),
        toolbar(
            browser_address(address, site_controls(cx), cx).into_any_element(),
            true,
            false,
            false,
            cx,
        )
        .show_separator(show_separator),
    )
}

pub fn browser_pane(
    id: &'static str,
    address: &Entity<InputState>,
    active: bool,
    gaps: bool,
    cx: &App,
) -> zpui::Stateful<zpui::Div> {
    let radius = radii(gaps).top_left;
    pane_surface(
        id,
        div()
            .flex()
            .flex_col()
            .size_full()
            .rounded(radius)
            .bg(cx.theme().background.opaque())
            .text_color(cx.theme().foreground)
            .child(header(active, 2, 0, address, false, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(browser_start_surface(BrowserEmptyHint)),
            ),
        Vec::new(),
        chrome(active, true, gaps, cx),
        cx,
    )
}

fn framed(id: &'static str, content: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .id(id)
        .w_full()
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .bg(cx.theme().background.opaque())
        .child(content)
        .into_any_element()
}

fn headers(inputs: &[Entity<InputState>; 4], _: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "active pane, three tabs",
            framed(
                "header-active",
                header(true, 3, 1, &inputs[0], true, cx),
                cx,
            ),
        )
        .state(
            "inactive pane: actions appear on hover",
            framed(
                "header-inactive",
                header(false, 3, 0, &inputs[1], true, cx),
                cx,
            ),
        )
        .state(
            "one tab: no close button",
            framed(
                "header-single",
                header(true, 1, 0, &inputs[2], true, cx),
                cx,
            ),
        )
        .state(
            "over the start page: no separator",
            framed(
                "header-start",
                header(true, 2, 1, &inputs[3], false, cx),
                cx,
            ),
        )
        .into_any_element()
}

fn toolbars(inputs: &[Entity<InputState>; 2], _: &mut Window, cx: &mut App) -> AnyElement {
    let plain_address = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .text_size(px(12.0))
        .text_color(cx.theme().foreground.muted())
        .child(URLS[0])
        .into_any_element();
    let disabled = |id: &'static str, icon: IconName| {
        Button::compact_icon(id, icon)
            .disabled(true)
            .tooltip("Requires the desktop browser runtime")
    };
    states()
        .state(
            "buttons: rest, disabled, selected",
            row()
                .child(browser_toolbar_button(
                    cx,
                    "rest",
                    IconName::ArrowLeft,
                    "Back",
                    false,
                    false,
                ))
                .child(browser_toolbar_button(
                    cx,
                    "disabled",
                    IconName::ArrowRight,
                    "Forward",
                    true,
                    false,
                ))
                .child(browser_toolbar_button(
                    cx,
                    "selected",
                    IconName::Inspector,
                    "Cancel element picker",
                    false,
                    true,
                ))
                .child(browser_toolbar_button(
                    cx,
                    "loading",
                    IconName::Xmark,
                    "Stop",
                    false,
                    false,
                )),
        )
        .state(
            "back available, forward not",
            framed(
                "toolbar-history",
                toolbar(
                    browser_address(&inputs[0], site_controls(cx), cx).into_any_element(),
                    true,
                    false,
                    false,
                    cx,
                ),
                cx,
            ),
        )
        .state(
            "element picker on, no separator",
            framed(
                "toolbar-picking",
                toolbar(
                    browser_address(&inputs[1], site_controls(cx), cx).into_any_element(),
                    false,
                    false,
                    true,
                    cx,
                )
                .show_separator(false),
                cx,
            ),
        )
        .state(
            "web client without Chromium: controls disabled, address as text",
            framed(
                "toolbar-web",
                BrowserToolbar::new(
                    disabled("web-back", IconName::ArrowLeft),
                    disabled("web-forward", IconName::ArrowRight),
                    disabled("web-reload", IconName::Redo2),
                    plain_address,
                    disabled("web-picker", IconName::Inspector),
                    disabled("web-more", IconName::Ellipsis),
                ),
                cx,
            ),
        )
        .into_any_element()
}

fn address_row(id: &'static str, input: &Entity<InputState>, cx: &App) -> AnyElement {
    div()
        .id(id)
        .flex()
        .w(px(520.0))
        .h(px(28.0))
        .child(browser_address(input, site_controls(cx), cx))
        .into_any_element()
}

fn addresses(inputs: &[Entity<InputState>; 4], _: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state("url", address_row("address-url", &inputs[0], cx))
        .state("long url", address_row("address-long", &inputs[3], cx))
        .state(
            "site controls alone",
            row().child(browser_site_controls_button(cx)),
        )
        .into_any_element()
}

fn focused_address(input: &Entity<InputState>, _: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state("focused", address_row("address-focused", input, cx))
        .into_any_element()
}

fn strip(id: &'static str, width: f32, count: usize, active: usize) -> AnyElement {
    div()
        .id(id)
        .flex()
        .w(px(width))
        .h(px(28.0))
        .child(BrowserTabStrip::new(tab_infos(count), active))
        .into_any_element()
}

fn tabs(_: &mut Window, _: &mut App) -> AnyElement {
    states()
        .state("one tab", strip("tabs-one", 640.0, 1, 0))
        .state(
            "three tabs, second active",
            strip("tabs-three", 640.0, 3, 1),
        )
        .state(
            "five tabs squeezed into 520px",
            strip("tabs-squeezed", 520.0, 5, 0),
        )
        .state(
            "nine tabs, the eighth active, scrolled into view",
            strip("tabs-scrolled", 640.0, 9, 7),
        )
        .into_any_element()
}

fn omnibox(inputs: &[Entity<InputState>; 1], _: &mut Window, cx: &mut App) -> AnyElement {
    let rows = [
        ("Key bindings - Zed", "zed.dev/docs/key-bindings", true),
        ("Zed - The editor for what's next", "zed.dev", false),
        ("", "zed.dev/releases/stable", false),
        (
            "Configuring Zed - a very long page title that keeps going past the half way mark",
            "zed.dev/docs/configuring-zed#theme-overrides",
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (title, url, selected))| {
        browser_omnibox_row(index, title, url, selected, None, cx).into_any_element()
    })
    .collect();
    states()
        .state(
            "suggestions for \"zed\"",
            div()
                .id("omnibox")
                .relative()
                .w_full()
                .h(BrowserHeader::HEIGHT + px(160.0))
                .overflow_hidden()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border())
                .bg(cx.theme().background.opaque())
                .child(header(true, 2, 0, &inputs[0], true, cx))
                .child(browser_omnibox_panel(rows, cx)),
        )
        .into_any_element()
}

fn page(id: &'static str, height: f32, content: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .id(id)
        .relative()
        .w_full()
        .h(px(height))
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .bg(cx.theme().background.opaque())
        .child(content)
        .into_any_element()
}

fn start_page(_: &mut Window, cx: &mut App) -> AnyElement {
    let recents = div()
        .flex()
        .flex_col()
        .w_full()
        .max_w(px(360.0))
        .gap(px(4.0))
        .children(
            [
                "zed.dev/docs/key-bindings",
                "github.com/demfabris/zz/pull/412",
                "localhost:4321",
                "docs.rs/zpui/latest/zpui/struct.Window.html#method.use_keyed_state",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, url)| browser_recent_row(("recent", index), url, None, cx)),
        );
    states()
        .columns(2)
        .state(
            "no history yet",
            page(
                "start-empty",
                260.0,
                browser_start_surface(BrowserEmptyHint),
                cx,
            ),
        )
        .state(
            "recent pages",
            page("start-recent", 260.0, browser_start_surface(recents), cx),
        )
        .into_any_element()
}

fn error_and_picker(_: &mut Window, cx: &mut App) -> AnyElement {
    let centered = |panel: BrowserErrorPanel| {
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(20.0))
            .child(panel)
    };
    let retry = Button::new("retry")
        .primary()
        .small()
        .icon(IconName::Redo2)
        .label("Try again");
    states()
        .columns(2)
        .state(
            "recoverable",
            page(
                "error-retry",
                260.0,
                centered(
                    BrowserErrorPanel::new("net::ERR_CONNECTION_REFUSED while loading http://localhost:4321/")
                        .retry(retry),
                ),
                cx,
            ),
        )
        .state(
            "not recoverable",
            page(
                "error-final",
                260.0,
                centered(BrowserErrorPanel::new(
                    "The browser process for this profile exited. Close the pane and open a new one.",
                )),
                cx,
            ),
        )
        .state(
            "element picker on",
            page(
                "picking",
                160.0,
                BrowserPickStatus::new("Select an element · Esc to cancel"),
                cx,
            ),
        )
        .state(
            "picker status that wraps",
            page(
                "picking-long",
                160.0,
                BrowserPickStatus::new(
                    "Copied the element's context and a screenshot to the clipboard · paste it into an agent pane",
                ),
                cx,
            ),
        )
        .into_any_element()
}

struct Menus {
    columns: u16,
    menus: Vec<(&'static str, Entity<PopupMenu>)>,
}

fn action_menus(window: &mut Window, cx: &mut App) -> Menus {
    let menu = |state: BrowserActionMenuState, window: &mut Window, cx: &mut App| {
        PopupMenu::build(window, cx, move |menu, window, cx| {
            browser_action_menu(menu, window, cx, state, BrowserMenuActions::new())
        })
    };
    Menus {
        columns: 2,
        menus: vec![
            (
                "profiles found",
                menu(
                    menu_state(BrowserProfileDiscoveryState::Ready, true, false),
                    window,
                    cx,
                ),
            ),
            (
                "profile discovery failed, picker on",
                menu(
                    menu_state(BrowserProfileDiscoveryState::Failed, false, true),
                    window,
                    cx,
                ),
            ),
        ],
    }
}

fn site_menus(window: &mut Window, cx: &mut App) -> Menus {
    let menu = |state: BrowserSiteMenuState, window: &mut Window, cx: &mut App| {
        PopupMenu::build(window, cx, move |menu, _, _| {
            browser_site_menu(menu, state, |_, _| {}, |_, _| {})
        })
    };
    Menus {
        columns: 3,
        menus: vec![
            (
                "secure, sound on",
                menu(site_state(Some(true), Some(false), true), window, cx),
            ),
            (
                "not secure, muted",
                menu(site_state(Some(false), Some(true), true), window, cx),
            ),
            (
                "file page: nothing to report",
                menu(
                    BrowserSiteMenuState {
                        site: "This page".into(),
                        ..site_state(None, None, false)
                    },
                    window,
                    cx,
                ),
            ),
        ],
    }
}

fn menus(menus: &Menus, _: &mut Window, _: &mut App) -> AnyElement {
    menus
        .menus
        .iter()
        .fold(states().columns(menus.columns), |grid, (label, menu)| {
            grid.state(*label, div().flex().items_start().child(menu.clone()))
        })
        .into_any_element()
}
