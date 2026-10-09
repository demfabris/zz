use std::cell::Cell;

use zpui::{
    AnyView, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, KeyBinding, MouseButton, ParentElement as _, Render, Styled as _, Subscription,
    Window, actions, div, prelude::FluentBuilder as _, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName,
    button::Button,
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
};

use super::support::{Probe, after_first_frame, dispatch, hover, press, when_visible};
use crate::story::{Section, Story, row, states};

actions!(
    storybook_menu,
    [
        NewTab, SplitRight, SplitDown, ClosePane, CopyPath, Rename, Detach
    ]
);

thread_local! {
    static BOUND: Cell<bool> = const { Cell::new(false) };
}

fn bind_keys(cx: &mut App) {
    if BOUND.with(|bound| bound.replace(true)) {
        return;
    }
    cx.bind_keys([
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("cmd-d", SplitRight, None),
        KeyBinding::new("cmd-shift-d", SplitDown, None),
        KeyBinding::new("cmd-w", ClosePane, None),
        KeyBinding::new("cmd-alt-c", CopyPath, None),
    ]);
}

pub const STORY: Story = Story {
    id: "menus",
    name: "Menus",
    group: "Kit",
    summary: "PopupMenu is the one menu surface. Dropdowns, context menus and the Select all open it; key hints come from the keymap.",
    sections: &[
        Section {
            id: "popup-menu",
            name: "Popup menu",
            summary: "Every row kind rendered inline: label, icon, checked, disabled, key hint, separator, link, stepper and a closed submenu.",
            build: |window, cx| Menus::view(Highlight::None, window, cx),
        },
        Section {
            id: "highlighted-row",
            name: "Highlighted row",
            summary: "The keyboard moved the highlight down two rows. A highlighted row brightens its key hint.",
            build: |window, cx| Menus::view(Highlight::Row, window, cx),
        },
        Section {
            id: "submenu-open",
            name: "Submenu open",
            summary: "Highlighting a submenu row opens it beside the parent; Right moves the highlight into it.",
            build: |window, cx| Menus::view(Highlight::Submenu, window, cx),
        },
        Section {
            id: "triggers",
            name: "Triggers",
            summary: "A dropdown Button and a context-menu region at rest. Both open the same PopupMenu, only on interaction.",
            build: |window, cx| Triggers::view(Open::None, window, cx),
        },
        Section {
            id: "dropdown-open",
            name: "Dropdown open",
            summary: "A press on the trigger opens the menu below it, focused, with the checked row marked.",
            build: |window, cx| Triggers::view(Open::Dropdown, window, cx),
        },
        Section {
            id: "context-menu-open",
            name: "Context menu open",
            summary: "A right press opens the menu at the pointer.",
            build: |window, cx| Triggers::view(Open::Context, window, cx),
        },
    ],
};

#[derive(Clone, Copy, PartialEq)]
enum Highlight {
    None,
    Row,
    Submenu,
}

fn pane_menu(menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    menu.label("Pane")
        .item(
            PopupMenuItem::new("New tab")
                .icon(IconName::Plus)
                .action(Box::new(NewTab)),
        )
        .item(
            PopupMenuItem::new("Split right")
                .icon(IconName::PanelRight)
                .action(Box::new(SplitRight)),
        )
        .item(
            PopupMenuItem::new("Split down")
                .icon(IconName::PanelBottom)
                .action(Box::new(SplitDown)),
        )
        .separator()
        .item(PopupMenuItem::new("Synchronize input").checked(true))
        .item(PopupMenuItem::new("Monitor activity"))
        .submenu_with_icon(
            Some(Icon::new(IconName::Layers)),
            "Move to",
            window,
            cx,
            |menu, _, _| {
                menu.item(PopupMenuItem::new("work"))
                    .item(PopupMenuItem::new("scratch").checked(true))
                    .item(PopupMenuItem::new("logs"))
                    .separator()
                    .item(PopupMenuItem::new("New session").icon(IconName::Plus))
            },
        )
        .separator()
        .item(
            PopupMenuItem::new("Copy path")
                .icon(IconName::Copy)
                .action(Box::new(CopyPath)),
        )
        .menu_with_disabled("Rename", Box::new(Rename), true)
        .item(
            PopupMenuItem::new("Close pane")
                .icon(IconName::Xmark)
                .action(Box::new(ClosePane)),
        )
}

fn view_menu(menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>) -> PopupMenu {
    menu.label("View")
        .item(PopupMenuItem::stepper(
            "Zoom",
            |_| "100%".into(),
            |_, _| {},
            |_, _| {},
            |_, _| {},
        ))
        .item(PopupMenuItem::stepper(
            "Font size",
            |_| "13".into(),
            |_, _| {},
            |_, _| {},
            |_, _| {},
        ))
        .separator()
        .item(PopupMenuItem::new("Light").icon(IconName::Sun))
        .item(
            PopupMenuItem::new("Dark")
                .icon(IconName::Moon)
                .checked(true),
        )
        .separator()
        .link("Documentation", "https://zzmux.sh")
        .link_with_disabled("Changelog", "https://zzmux.sh", true)
        .item(
            PopupMenuItem::element(|_, _, cx| {
                div()
                    .text_xs()
                    .text_color(cx.theme().foreground.muted())
                    .child("zz 0.16.0")
            })
            .disabled(true),
        )
}

struct Menus {
    highlight: Highlight,
    pane: Entity<PopupMenu>,
    view: Entity<PopupMenu>,
}

impl Menus {
    fn view(highlight: Highlight, window: &mut Window, cx: &mut App) -> AnyView {
        bind_keys(cx);
        let pane = PopupMenu::build(window, cx, pane_menu);
        let view = PopupMenu::build(window, cx, view_menu);
        if highlight != Highlight::None {
            let target = pane.clone();
            after_first_frame(window, move |window, cx| {
                target.read(cx).focus_handle(cx).focus(window, cx);
                let steps = if highlight == Highlight::Row { 3 } else { 7 };
                for _ in 0..steps {
                    dispatch("zz_menu::SelectDown", window, cx);
                }
                if highlight == Highlight::Submenu {
                    after_first_frame(window, |window, cx| {
                        dispatch("zz_menu::SelectRight", window, cx);
                    });
                }
            });
        }
        cx.new(|_| Self {
            highlight,
            pane,
            view,
        })
        .into()
    }
}

impl Render for Menus {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let menus = h_flex()
            .items_start()
            .gap_6()
            .child(self.pane.clone())
            .when(self.highlight == Highlight::None, |this| {
                this.child(self.view.clone())
            });
        let caption = match self.highlight {
            Highlight::None => "pane menu, view menu",
            Highlight::Row => "second row highlighted",
            Highlight::Submenu => "submenu open, first row highlighted",
        };
        states().state(caption, div().min_h(px(380.0)).child(menus))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Open {
    None,
    Dropdown,
    Context,
}

struct Triggers {
    open: Open,
    dropdown: Probe,
    region: Probe,
    _bounds: Subscription,
}

impl Triggers {
    fn view(open: Open, window: &mut Window, cx: &mut App) -> AnyView {
        bind_keys(cx);
        let dropdown = Probe::default();
        let region = Probe::default();
        let (dropdown_probe, region_probe) = (dropdown.clone(), region.clone());
        match open {
            Open::None => {}
            Open::Dropdown => when_visible(&dropdown, window, move |window, cx| {
                press(&dropdown_probe, MouseButton::Left, window, cx);
            }),
            Open::Context => when_visible(&region, window, move |window, cx| {
                press(&region_probe, MouseButton::Right, window, cx);
            }),
        }
        cx.new(|cx| Self {
            open,
            dropdown,
            region,
            _bounds: cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                match this.open {
                    Open::None => {}
                    Open::Dropdown => hover(&this.dropdown, window, cx),
                    Open::Context => hover(&this.region, window, cx),
                }
            }),
        })
        .into()
    }
}

impl Render for Triggers {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let trigger = div()
            .relative()
            .child(
                Button::new("layout-trigger")
                    .label("Layout")
                    .dropdown_caret(true)
                    .dropdown_menu(|menu, _, _| {
                        menu.item(PopupMenuItem::new("Even horizontal").checked(true))
                            .item(PopupMenuItem::new("Even vertical"))
                            .item(PopupMenuItem::new("Main vertical"))
                            .item(PopupMenuItem::new("Tiled"))
                            .separator()
                            .item(
                                PopupMenuItem::new("Split right")
                                    .icon(IconName::PanelRight)
                                    .action(Box::new(SplitRight)),
                            )
                    }),
            )
            .child(self.dropdown.measure());
        let region = div()
            .id("context-region")
            .relative()
            .w(px(280.0))
            .h(px(96.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.radius)
            .border_1()
            .border_dashed()
            .border_color(theme.border())
            .text_sm()
            .text_color(theme.foreground.muted())
            .child("Right click here")
            .child(self.region.measure())
            .context_menu(|menu, _, _| {
                menu.item(
                    PopupMenuItem::new("Copy")
                        .icon(IconName::Copy)
                        .action(Box::new(CopyPath)),
                )
                .item(PopupMenuItem::new("Paste"))
                .separator()
                .item(PopupMenuItem::new("Detach").action(Box::new(Detach)))
                .item(
                    PopupMenuItem::new("Close pane")
                        .icon(IconName::Xmark)
                        .action(Box::new(ClosePane)),
                )
            });
        let content = match self.open {
            Open::None => row().child(trigger).child(region).into_any_element(),
            Open::Dropdown => div()
                .h(px(240.0))
                .child(h_flex().child(trigger))
                .into_any_element(),
            Open::Context => div().h(px(260.0)).child(region).into_any_element(),
        };
        let caption = match self.open {
            Open::None => "dropdown button, context-menu region",
            Open::Dropdown => "dropdown open",
            Open::Context => "context menu open",
        };
        states().state(caption, content)
    }
}
