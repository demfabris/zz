//! The browser-pane pieces: toolbar controls, the address bar, recent rows,
//! and the recovery/status states.

use gpui::{AnyElement, App, Context, ParentElement as _, Styled as _, div, prelude::*, px};
use zz_ui::browser::{
    BrowserActionMenuState, BrowserEmptyHint, BrowserErrorPanel, BrowserMenuActions,
    BrowserMenuProfile, BrowserPickStatus, BrowserProfileDiscoveryState, BrowserTabInfo,
    BrowserTabStrip, browser_action_menu as shared_browser_action_menu, browser_address,
    browser_recent_row, browser_toolbar_button,
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
                "The 24px ghost toolbar controls, in their enabled, disabled, active, and action-menu forms.",
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
                "The borderless URL field that fills the toolbar between the controls, at rest and while a navigation is loading.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "URL input",
                        div()
                            .w(px(520.0))
                            .child(browser_address(&showcase.browser_address, cx)),
                        cx,
                    ))
                    .child(specimen_block(
                        "loading",
                        div()
                            .w(px(520.0))
                            .child(browser_address(&showcase.browser_address_loading, cx)),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Tab strip",
                "Safari-compact tabs in the address slot. The address bar forms the active tab; the rest collapse into hostname pills. Each tab keeps its close control visible, followed by the new-tab button.",
                cx,
            )
            .child(
                specimens().w_full().child(specimen_block(
                    "four tabs · active second",
                    div()
                        .w(px(680.0))
                        .h(px(40.0))
                        .flex()
                        .child(
                            BrowserTabStrip::new(
                                browser_address(&showcase.browser_tab_address, cx),
                                vec![
                                    BrowserTabInfo::new(1, "gpui.rs", "GPUI"),
                                    BrowserTabInfo::new(2, "github.com", "zz: a terminal for the 2020s"),
                                    BrowserTabInfo::new(3, "crates.io", "crates.io: Rust package registry"),
                                    BrowserTabInfo::new(4, "news.ycombinator.com", "Hacker News"),
                                ],
                                1,
                            ),
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
                            .border_color(cx.theme().border)
                            .child(mock_browser_page(cx))
                            .child(BrowserPickStatus::new("Select an element · Esc to cancel")),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

fn recent_row(url: &'static str, cx: &App) -> AnyElement {
    browser_recent_row(format!("br-recent-{url}"), url, cx).into_any_element()
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
