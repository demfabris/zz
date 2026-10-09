use zpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Window, div, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::{Dialog, DialogButtonProps},
    h_flex,
    input::{Input, InputState},
    overlay::dialog_description,
    v_flex,
};

use super::support::after_first_frame;
use crate::story::{Section, Story};

pub const STORY: Story = Story {
    id: "dialogs",
    name: "Dialogs",
    group: "Kit",
    summary: "Dialogs render into the window's Root layer over a scrim. Each section opens its own after the window exists; the builder runs on every frame.",
    sections: &[
        Section {
            id: "dialog",
            name: "Dialog",
            summary: "A title, a body and the default footer with Cancel and OK. The close button sits in the top right corner.",
            build: |window, cx| Backdrop::view(Kind::Form, window, cx),
        },
        Section {
            id: "alert",
            name: "Alert dialog",
            summary: "An icon, a title, a description and a destructive answer. AlertDialog is a preset over Dialog.",
            build: |window, cx| Backdrop::view(Kind::Alert, window, cx),
        },
        Section {
            id: "custom-footer",
            name: "Custom footer",
            summary: "footer() replaces the button row; close_button(false) drops the corner X.",
            build: |window, cx| Backdrop::view(Kind::Footer, window, cx),
        },
        Section {
            id: "scrolling-body",
            name: "Scrolling body",
            summary: "A body taller than the window scrolls between a fixed title and footer.",
            build: |window, cx| Backdrop::view(Kind::Long, window, cx),
        },
        Section {
            id: "stacked",
            name: "Stacked",
            summary: "A second dialog opens over the first. Only the topmost draws the scrim; lower layers step down.",
            build: |window, cx| Backdrop::view(Kind::Stacked, window, cx),
        },
    ],
};

#[derive(Clone, Copy)]
enum Kind {
    Form,
    Alert,
    Footer,
    Long,
    Stacked,
}

struct Backdrop {
    kind: Kind,
    _name: Entity<InputState>,
}

impl Backdrop {
    fn view(kind: Kind, window: &mut Window, cx: &mut App) -> AnyView {
        let name = cx.new(|cx| InputState::new(window, cx).default_value("scratch"));
        let field = name.clone();
        after_first_frame(window, move |window, cx| open(kind, &field, window, cx));
        cx.new(|_| Self { kind, _name: name }).into()
    }
}

fn open(kind: Kind, field: &Entity<InputState>, window: &mut Window, cx: &mut App) {
    match kind {
        Kind::Form => {
            let field = field.clone();
            window.open_dialog(cx, move |dialog, _, cx| rename_dialog(dialog, &field, cx));
        }
        Kind::Alert => window.open_alert_dialog(cx, |alert, _, cx| {
            alert
                .icon(
                    Icon::new(IconName::TriangleAlert)
                        .large()
                        .text_color(cx.theme().danger),
                )
                .title("Kill session work?")
                .description("Three windows and seven panes close. Running processes get SIGHUP.")
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Kill session")
                        .ok_variant(ButtonVariant::Danger)
                        .show_cancel(true),
                )
        }),
        Kind::Footer => window.open_dialog(cx, |dialog, _, cx| {
            dialog
                .title("Daemon update")
                .width(px(460.0))
                .close_button(false)
                .child(
                    dialog_description(cx)
                        .child("zz 0.16.1 is installed. Sessions keep running on the old daemon until it restarts."),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .child(Button::new("notes").label("Release notes").link().small())
                        .child(
                            h_flex()
                                .gap_2()
                                .child(Button::new("later").label("Later").ghost().small())
                                .child(
                                    Button::new("restart")
                                        .label("Restart daemon")
                                        .accent()
                                        .small(),
                                ),
                        ),
                )
        }),
        Kind::Long => window.open_dialog(cx, |dialog, _, cx| {
            let muted = cx.theme().foreground.muted();
            dialog
                .title("Key bindings")
                .width(px(480.0))
                .child(v_flex().gap_2().children(BINDINGS.iter().map(|(keys, command)| {
                    h_flex()
                        .justify_between()
                        .text_sm()
                        .child(*command)
                        .child(
                            div()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_color(muted)
                                .child(*keys),
                        )
                })))
                .button_props(DialogButtonProps::default().ok_text("Done"))
        }),
        Kind::Stacked => {
            let field = field.clone();
            window.open_dialog(cx, move |dialog, _, cx| rename_dialog(dialog, &field, cx));
            window.open_alert_dialog(cx, |alert, _, _| {
                alert
                    .title("Discard the new name?")
                    .description("The session keeps its old name.")
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text("Discard")
                            .ok_variant(ButtonVariant::Danger)
                            .show_cancel(true),
                    )
            });
        }
    }
}

fn rename_dialog(dialog: Dialog, field: &Entity<InputState>, cx: &App) -> Dialog {
    dialog
        .title("Rename session")
        .width(px(420.0))
        .child(
            v_flex()
                .gap_2()
                .child(
                    dialog_description(cx).child("Names show in the sidebar and the status bar."),
                )
                .child(Input::new(field)),
        )
        .button_props(
            DialogButtonProps::default()
                .ok_text("Rename")
                .ok_variant(ButtonVariant::Accent)
                .show_cancel(true),
        )
}

const BINDINGS: [(&str, &str); 18] = [
    ("C-a c", "New window"),
    ("C-a n", "Next window"),
    ("C-a p", "Previous window"),
    ("C-a %", "Split right"),
    ("C-a \"", "Split down"),
    ("C-a x", "Kill pane"),
    ("C-a z", "Zoom pane"),
    ("C-a d", "Detach"),
    ("C-a [", "Copy mode"),
    ("C-a ]", "Paste buffer"),
    ("C-a ,", "Rename window"),
    ("C-a $", "Rename session"),
    ("C-a s", "Choose session"),
    ("C-a w", "Choose window"),
    ("C-a q", "Display panes"),
    ("C-a o", "Next pane"),
    ("C-a ;", "Last pane"),
    ("C-a ?", "List keys"),
];

impl Render for Backdrop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.foreground.muted();
        let height = match self.kind {
            Kind::Long | Kind::Stacked => 440.0,
            Kind::Form | Kind::Alert | Kind::Footer => 300.0,
        };
        v_flex()
            .h(px(height))
            .gap_2()
            .text_sm()
            .text_color(muted)
            .children(
                ["work", "scratch", "logs", "devbox", "notes"]
                    .into_iter()
                    .map(|name| {
                        h_flex()
                            .gap_2()
                            .child(Icon::new(IconName::SquareTerminal).small())
                            .child(name)
                    }),
            )
    }
}
