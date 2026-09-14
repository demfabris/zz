//! The browser-pane pieces: toolbar controls, the address bar, recent rows,
//! and the recovery/status states.

use gpui::{AnyElement, App, Context, ParentElement as _, Styled as _, div, prelude::*, px};
use zz_ui::browser::{
    BrowserActionMenuState, BrowserEmptyHint, BrowserErrorPanel, BrowserHeader, BrowserMenuActions,
    BrowserMenuProfile, BrowserPickStatus, BrowserProfileDiscoveryState, BrowserSiteMenuState,
    BrowserTabInfo, BrowserTabStrip, BrowserToolbar,
    browser_action_menu as shared_browser_action_menu, browser_address, browser_recent_row,
    browser_site_controls_button, browser_site_menu, browser_toolbar_button,
};
use zz_ui::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::DropdownMenu as _,
};

use super::{
    Showcase, gallery, mock_browser_page, specimen, specimen_block, specimens, story_stack,
};

pub(super) fn render(showcase: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Toolbar buttons",
                "The 28px ghost toolbar controls, in their enabled, disabled, active, and action-menu forms.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "back",
                        browser_toolbar_button(cx, "br-back", IconName::ArrowLeft, "Back", false, false),
                        cx,
                    ))
                    .child(specimen(
                        "forward · disabled",
                        browser_toolbar_button(cx, "br-forward", IconName::ArrowRight, "Forward", true, false),
                        cx,
                    ))
                    .child(specimen(
                        "reload",
                        browser_toolbar_button(cx, "br-reload", IconName::Redo2, "Reload", false, false),
                        cx,
                    ))
                    .child(specimen(
                        "picker · active",
                        browser_toolbar_button(cx, "br-picker", IconName::Inspector, "Cancel element picker", false, true),
                        cx,
                    ))
                    .child(specimen(
                        "action menu",
                        browser_toolbar_button(cx, "br-more", IconName::EllipsisVertical, "More browser actions", false, false)
                            .dropdown_menu(browser_action_menu),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Address bar",
                "The URL field fills the navigation row between the browser controls.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "URL input",
                        div()
                            .w(px(520.0))
                            .child(browser_address(&showcase.browser_address, site_controls(cx), cx)),
                        cx,
                    ))
                    .child(specimen_block(
                        "loading",
                        div()
                            .w(px(520.0))
                            .child(browser_address(&showcase.browser_address_loading, site_controls(cx), cx)),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Browser header",
                "Page tabs sit above the navigation row. Hover a tab to reveal its close button; use the plus button to open another tab.",
                cx,
            )
            .child(
                specimens().w_full().child(specimen_block(
                    "four tabs · active second",
                    div()
                        .w(px(680.0))
                        .h(BrowserHeader::HEIGHT)
                        .flex()
                        .child(
                            BrowserHeader::new(true, BrowserTabStrip::new(
                                vec![
                                    BrowserTabInfo::new(1, "gpui.rs", "GPUI"),
                                    BrowserTabInfo::new(2, "github.com", "zz: a terminal for the 2020s"),
                                    BrowserTabInfo::new(3, "crates.io", "crates.io: Rust package registry"),
                                    BrowserTabInfo::new(4, "news.ycombinator.com", "Hacker News"),
                                ],
                                1,
                            ),
                            div().flex().flex_none().items_center().gap_1().children(
                                [(IconName::PanelBottom, "Split bottom"), (IconName::PanelRight, "Split right")]
                                    .into_iter()
                                    .map(|(icon, label)| zz_ui::pane::pane_header_icon_button(label, icon, true, cx).tooltip(label)),
                            ).child(zz_ui::pane::pane_drag_button(
                                "gallery-browser-drag", zz_protocol::PaneId(3), "Browser".into(), true, |_, _, _| {}, cx,
                            )).child(zz_ui::pane::pane_header_icon_button("gallery-browser-close", IconName::Xmark, true, cx).tooltip("Close pane")),
                            BrowserToolbar::new(
                                browser_toolbar_button(cx, "header-back", IconName::ArrowLeft, "Back", false, false),
                                browser_toolbar_button(cx, "header-forward", IconName::ArrowRight, "Forward", true, false),
                                browser_toolbar_button(cx, "header-reload", IconName::Redo2, "Reload", false, false),
                                browser_address(&showcase.browser_tab_address, site_controls(cx), cx),
                                browser_toolbar_button(cx, "header-picker", IconName::Inspector, "Pick an element", false, false),
                                browser_toolbar_button(cx, "header-more", IconName::EllipsisVertical, "More browser actions", false, false).dropdown_menu(browser_action_menu),
                            )),
                        ),
                    cx,
                )),
            ),
        )
        .child(
            gallery(
                "Start page",
                "The first-run hint and the inline washed URL rows shown after pages have been visited.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block("empty hint", BrowserEmptyHint, cx))
                    .child(specimen_block("recent URLs", recent_list(cx), cx)),
            ),
        )
        .child(
            gallery(
                "Recovery & status",
                "A recoverable failure offers a retry; a terminal failure drops it. Element picking shows a compact status pill over live content.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "recoverable error",
                        BrowserErrorPanel::new("The Chromium renderer exited before producing a frame.")
                            .retry(
                                Button::new("br-retry")
                                    .primary()
                                    .small()
                                    .icon(IconName::Redo2)
                                    .label("Try again"),
                            ),
                        cx,
                    ))
                    .child(specimen(
                        "terminal error",
                        BrowserErrorPanel::new("CEF runtime is unavailable in this bundle."),
                        cx,
                    ))
                    .child(specimen(
                        "picker status",
                        div()
                            .relative()
                            .w(px(320.0))
                            .h(px(150.0))
                            .overflow_hidden()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border())
                            .child(mock_browser_page(cx))
                            .child(BrowserPickStatus::new("Select an element · Esc to cancel")),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

fn recent_row(url: &'static str, cx: &App) -> AnyElement {
    browser_recent_row(format!("br-recent-{url}"), url, None, cx).into_any_element()
}

fn recent_list(cx: &App) -> impl IntoElement {
    div()
        .w(px(360.0))
        .max_w_full()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .children([
            recent_row("gpui.rs", cx),
            recent_row("zed.dev", cx),
            recent_row("doc.rust-lang.org", cx),
        ])
}

fn browser_action_menu(
    menu: zz_ui::menu::PopupMenu,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<zz_ui::menu::PopupMenu>,
) -> zz_ui::menu::PopupMenu {
    shared_browser_action_menu(
        menu,
        window,
        cx,
        BrowserActionMenuState {
            current_profile_label: "Work · work@example.com".into(),
            selected_profile: "chrome-work".into(),
            default_profile: "zz-default".into(),
            profiles: vec![
                BrowserMenuProfile::new("chrome-work", "Work · work@example.com"),
                BrowserMenuProfile::new("chrome-personal", "Personal · me@example.com"),
            ],
            profile_discovery: BrowserProfileDiscoveryState::Ready,
            zoom_percent: 110,
            can_import_chrome_data: true,
            can_clear_site_data: true,
            picker_active: false,
        },
        BrowserMenuActions::default(),
    )
}

fn site_controls(cx: &App) -> impl IntoElement {
    browser_site_controls_button(cx).dropdown_menu(|menu, _, _| {
        browser_site_menu(
            menu,
            BrowserSiteMenuState {
                site: "Example page".into(),
                connection_secure: None,
                audio_muted: None,
                can_clear_site_data: false,
            },
            |_, _| {},
            |_, _| {},
        )
    })
}
