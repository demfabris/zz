use zz_gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, ElementId, Entity, IntoElement, Keystroke,
    ParentElement as _, Render, SharedString, Styled as _, Window, div, prelude::*, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName, IndexPath, Sizable as _,
    button::{Button, ButtonVariants as _},
    command::{
        CommandPaletteSurface, PaletteHint, PalettePill, PaletteRow, PaletteStatus,
        command_palette_entry, command_palette_row_height, command_palette_section,
        unified_command_palette_input,
    },
    h_flex,
    input::{Input, InputState, NumberInput},
    kbd::Kbd,
    menu::PopupMenu,
    notification::Notification,
    overlay::{dialog_description, dialog_footer, dialog_gutter, dialog_surface, dialog_title},
    select::{Select, SelectState},
    slider::DiscreteSlider,
    switch::Switch,
    tag::Tag,
    tooltip::Tooltip,
    v_flex,
};

use super::{
    agent::cards::{COMMAND, permission},
    kit::menus::{SplitRight, bind_keys, pane_menu, view_menu},
    workspace::{
        shell::{Shell, with_sidebar},
        status_bar::{Bar, titlebar},
    },
};
use crate::story::{Section, Story};

pub const ID: &str = "style-creator";

pub const STORY: Story = Story {
    id: ID,
    name: "Style creator",
    group: "Foundation",
    summary: "Everything a style touches on one canvas. Tune the knobs, then copy the look as JSON.",
    sections: &[Section {
        id: "canvas",
        name: "Canvas",
        summary: "zz's chrome, controls and floating surfaces side by side, in one window.",
        build: Showcase::view,
    }],
};

const SHELLS: [&str; 4] = ["zsh", "fish", "bash", "nu"];
const SCROLLBACK: [&str; 5] = ["1k", "10k", "50k", "100k", "∞"];
const WIDE: f32 = 1040.0;
const MEDIUM: f32 = 680.0;
const GAP: f32 = 16.0;

pub struct Showcase {
    shell: Shell,
    host: Entity<InputState>,
    user: Entity<InputState>,
    port: Entity<InputState>,
    login: Entity<SelectState<Vec<&'static str>>>,
    pane_menu: Entity<PopupMenu>,
    view_menu: Entity<PopupMenu>,
    search: Entity<InputState>,
    toasts: [Entity<Notification>; 2],
    tooltip: Entity<Tooltip>,
    gaps: bool,
    animations: bool,
    sound: bool,
    scrollback: usize,
}

impl Showcase {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        bind_keys(cx);
        let shell = Shell::new(window, cx);
        let field = |value: &'static str, window: &mut Window, cx: &mut App| {
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        };
        let host = field("devbox.local", window, cx);
        let user = field("fabrico", window, cx);
        let port = cx.new(|cx| {
            InputState::new(window, cx)
                .step(1.0)
                .min(1.0)
                .max(65535.0)
                .default_value("22")
        });
        let login =
            cx.new(|cx| SelectState::new(SHELLS.to_vec(), Some(IndexPath::new(0)), window, cx));
        let pane_menu = PopupMenu::build(window, cx, pane_menu);
        let view_menu = PopupMenu::build(window, cx, view_menu);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Jump to a window"));
        let toast = |notification: Notification, cx: &mut App| {
            cx.new(|_| notification.autohide(false).width(px(320.0)))
        };
        let toasts = [
            toast(
                Notification::success("3 windows and 7 panes came back.").title("Session restored"),
                cx,
            ),
            toast(
                Notification::error("ssh devbox exited with status 255.").title("Connection lost"),
                cx,
            ),
        ];
        let tooltip = cx.new(|_| Tooltip::new("Split right").action(&SplitRight, None));
        cx.new(|_| Self {
            shell,
            host,
            user,
            port,
            login,
            pane_menu,
            view_menu,
            search,
            toasts,
            tooltip,
            gaps: false,
            animations: true,
            sound: false,
            scrollback: 2,
        })
        .into()
    }

    fn hero(&self, cx: &App) -> AnyElement {
        v_flex()
            .id("showcase-app")
            .w_full()
            .h(px(360.0))
            .overflow_hidden()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().divider_color())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(with_sidebar(self.gaps, &self.shell, 0, cx)),
            )
            .into_any_element()
    }

    fn status_bar(&self, cx: &App) -> AnyElement {
        div()
            .id("showcase-status-bar")
            .w_full()
            .overflow_hidden()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().divider_color())
            .bg(cx.theme().background)
            .child(titlebar(
                &Bar {
                    gaps: self.gaps,
                    ..Bar::FULL
                },
                cx,
            ))
            .into_any_element()
    }

    fn connect(&self, cx: &App) -> AnyElement {
        card("showcase-connect", cx)
            .child(heading("New host", "Panes on this host open over ssh.", cx))
            .child(labeled("Host", Input::new(&self.host), cx))
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .child(labeled("User", Input::new(&self.user), cx)),
                    )
                    .child(div().w(px(132.0)).child(labeled(
                        "Port",
                        NumberInput::new(&self.port),
                        cx,
                    ))),
            )
            .child(labeled("Login shell", Select::new(&self.login), cx))
            .child(
                h_flex()
                    .pt_1()
                    .gap_2()
                    .justify_end()
                    .child(Button::new("showcase-cancel").label("Cancel"))
                    .child(
                        Button::new("showcase-connect-button")
                            .primary()
                            .label("Connect"),
                    ),
            )
            .into_any_element()
    }

    fn preferences(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        card("showcase-preferences", cx)
            .child(heading("Panes", "How the canvas lays out and behaves.", cx))
            .child(toggle(
                "showcase-gaps",
                "Pane gaps",
                "Float panes apart with rounded corners",
                self.gaps,
                cx.listener(|this, checked: &bool, _, cx| {
                    this.gaps = *checked;
                    cx.notify();
                }),
                cx,
            ))
            .child(toggle(
                "showcase-animations",
                "Animations",
                "Menus and dialogs ease in",
                self.animations,
                cx.listener(|this, checked: &bool, _, cx| {
                    this.animations = *checked;
                    cx.notify();
                }),
                cx,
            ))
            .child(toggle(
                "showcase-sound",
                "Bell sound",
                "Play a sound when a pane rings",
                self.sound,
                cx.listener(|this, checked: &bool, _, cx| {
                    this.sound = *checked;
                    cx.notify();
                }),
                cx,
            ))
            .child(DiscreteSlider::new(
                "showcase-scrollback",
                "Scrollback",
                SCROLLBACK
                    .iter()
                    .map(|label| SharedString::from(*label))
                    .collect(),
                self.scrollback,
                move |index, _, cx| {
                    this.update(cx, |this, cx| {
                        this.scrollback = index;
                        cx.notify();
                    });
                },
            ))
            .into_any_element()
    }

    fn buttons(&self, cx: &App) -> AnyElement {
        let keys = ["cmd-k", "cmd-shift-d", "ctrl-a"];
        card("showcase-buttons", cx)
            .child(heading("Actions", "Buttons, tags and key hints.", cx))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("showcase-primary")
                            .primary()
                            .label("New session"),
                    )
                    .child(Button::new("showcase-default").label("Detach"))
                    .child(Button::new("showcase-ghost").ghost().label("Rename"))
                    .child(Button::new("showcase-danger").danger().label("Kill")),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("showcase-split-right").icon(IconName::PanelRight))
                    .child(Button::new("showcase-split-down").icon(IconName::PanelBottom))
                    .child(
                        Button::new("showcase-search")
                            .ghost()
                            .icon(IconName::Search),
                    )
                    .child(
                        Button::new("showcase-settings")
                            .ghost()
                            .icon(IconName::Settings),
                    )
                    .child(div().flex_1())
                    .child(Button::new("showcase-small").small().label("Small")),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(Tag::success().child("connected"))
                    .child(Tag::primary().child("main"))
                    .child(Tag::secondary().child("3 panes"))
                    .children(
                        keys.into_iter()
                            .map(|key| Kbd::new(Keystroke::parse(key).unwrap_or_default())),
                    ),
            )
            .into_any_element()
    }

    fn dialog(&self, cx: &App) -> AnyElement {
        dialog_surface(cx)
            .w_full()
            .max_w(px(360.0))
            .child(dialog_gutter().child(dialog_title().child("Kill session \"work\"?")))
            .child(dialog_gutter().child(
                dialog_description(cx).child(
                    "Its 3 windows and 7 panes stop, and their scrollback is gone for good.",
                ),
            ))
            .child(
                dialog_gutter().child(
                    dialog_footer(cx)
                        .child(Button::new("showcase-dialog-cancel").label("Cancel"))
                        .child(
                            Button::new("showcase-dialog-kill")
                                .danger()
                                .label("Kill session"),
                        ),
                ),
            )
            .into_any_element()
    }

    fn palette(&self, cx: &App) -> AnyElement {
        let row =
            |label: &str, detail: &str, right: &str, icon: IconName, running: bool| PaletteRow {
                label: label.to_owned().into(),
                detail: detail.to_owned().into(),
                right: right.to_owned().into(),
                icon: Some(icon),
                running,
                ..Default::default()
            };
        let rows = [
            row(
                "editor",
                "2 panes",
                "current",
                IconName::PanelsTopLeft,
                false,
            ),
            row("agents", "2 panes", "", IconName::PanelsTopLeft, true),
            row("logs", "1 pane", "", IconName::PanelsTopLeft, false),
            row("train.py", "gpu-box", "", IconName::SquareTerminal, true),
        ];
        let height = px(command_palette_row_height(cx));
        CommandPaletteSurface::new(
            unified_command_palette_input(
                &self.search,
                [PalettePill::Host {
                    label: "devbox".into(),
                    status: PaletteStatus::Online,
                }],
                cx,
            ),
            1,
        )
        .rows(
            v_flex()
                .child(
                    div()
                        .h(height)
                        .child(command_palette_section("Windows", "zz", cx)),
                )
                .children(rows.iter().enumerate().map(|(index, row)| {
                    div().h(height).child(command_palette_entry(
                        ("showcase-palette-row", index),
                        row,
                        index == 0,
                        cx,
                    ))
                })),
        )
        .hints([
            PaletteHint {
                key: "up down",
                label: "navigate",
            },
            PaletteHint {
                key: "enter",
                label: "open",
            },
            PaletteHint {
                key: "escape",
                label: "close",
            },
        ])
        .into_any_element()
    }

    fn agent(&self, cx: &App) -> AnyElement {
        card("showcase-agent", cx)
            .child(heading("Agent", "A tool call waiting for approval.", cx))
            .child(permission(
                "showcase-permission",
                "cargo test -p zz-ui",
                None,
                &COMMAND,
                0,
                true,
                cx,
            ))
            .into_any_element()
    }
}

impl Render for Showcase {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = window.viewport_size().width.as_f32();
        let menus = plate(
            "showcase-menus",
            h_flex().items_start().gap_4().child(self.pane_menu.clone()),
            cx,
        );
        let view = plate("showcase-view-menu", self.view_menu.clone(), cx);
        let palette = plate("showcase-palette", self.palette(cx), cx);
        let dialog = plate("showcase-dialog", self.dialog(cx), cx);
        let toasts = plate(
            "showcase-toasts",
            v_flex()
                .gap_3()
                .items_center()
                .children(self.toasts.iter().cloned())
                .child(self.tooltip.clone()),
            cx,
        );
        let tiles = [
            vec![self.connect(cx), self.preferences(cx), view],
            vec![menus, self.buttons(cx), toasts],
            vec![palette, dialog, self.agent(cx)],
        ];
        let columns = if width >= WIDE {
            3
        } else if width >= MEDIUM {
            2
        } else {
            1
        };
        let mut stacks: Vec<Vec<AnyElement>> = (0..columns).map(|_| Vec::new()).collect();
        for (index, tile) in tiles.into_iter().flatten().enumerate() {
            let column = if columns == 3 {
                index / 3
            } else {
                index % columns
            };
            stacks[column].push(tile);
        }
        v_flex()
            .w_full()
            .gap(px(GAP))
            .child(self.hero(cx))
            .child(self.status_bar(cx))
            .child(
                h_flex().w_full().items_start().gap(px(GAP)).children(
                    stacks
                        .into_iter()
                        .map(|stack| v_flex().flex_1().min_w_0().gap(px(GAP)).children(stack)),
                ),
            )
    }
}

fn card(id: &'static str, cx: &App) -> zz_gpui::Stateful<zz_gpui::Div> {
    v_flex()
        .id(id)
        .w_full()
        .p(px(16.0))
        .gap_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().divider_color())
        .bg(cx.theme().background)
}

/// Floating surfaces sit on a plate that shows the backdrop knob behind
/// them, the way they float over panes in zz.
fn plate(id: &'static str, content: impl IntoElement, cx: &App) -> AnyElement {
    let backdrop = crate::backdrop::backdrop(crate::knobs().backdrop, cx);
    div()
        .id(id)
        .relative()
        .w_full()
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().divider_color())
        .bg(cx.theme().background.raised(1))
        .children(backdrop)
        .child(
            h_flex()
                .relative()
                .w_full()
                .p(px(20.0))
                .justify_center()
                .child(content),
        )
        .into_any_element()
}

fn heading(title: &'static str, description: &'static str, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_0p5()
        .child(
            div()
                .text_sm()
                .font_weight(zz_gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().foreground.muted())
                .child(description),
        )
}

fn labeled(label: &'static str, control: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().foreground.muted())
                .child(label),
        )
        .child(control)
}

fn toggle(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    checked: bool,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .gap_3()
        .justify_between()
        .child(
            v_flex()
                .min_w_0()
                .child(div().text_sm().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().foreground.muted())
                        .child(description),
                ),
        )
        .child(
            Switch::new(ElementId::from(id))
                .checked(checked)
                .on_click(on_click),
        )
}
