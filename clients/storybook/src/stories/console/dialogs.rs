use zz_gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, Render, Styled as _, Window, div,
    px,
};
use zz_ui::{
    WindowExt as _,
    feedback::{
        add_host_prompt_dialog, browser_clear_site_data_alert, import_configuration_file_alert,
        ssh_confirm_prompt_dialog, ssh_offer_prompt_dialog, ssh_secret_prompt_dialog,
        ssh_text_prompt_dialog,
    },
    input::InputState,
};

use crate::story::{Section, Story};

pub const STORY: Story = Story {
    id: "feedback-dialogs",
    name: "Feedback dialogs",
    group: "Commands",
    summary: "The confirmations and prompts zz raises over the window: destructive alerts, the add-host prompt, and the questions ssh asks while it connects.",
    sections: &[
        Section {
            id: "clear-site-data",
            name: "Clear site data",
            summary: "A destructive alert: danger icon and a danger answer.",
            build: |window, cx| {
                host(window, cx, |window, cx| {
                    window.open_alert_dialog(cx, |alert, _, cx| {
                        browser_clear_site_data_alert(alert, cx)
                    });
                })
            },
        },
        Section {
            id: "import-config",
            name: "Import configuration",
            summary: "A warning alert that names the file it writes.",
            build: |window, cx| {
                host(window, cx, |window, cx| {
                    window.open_alert_dialog(cx, |alert, _, _| {
                        import_configuration_file_alert(
                            alert,
                            "Import tmux.conf?",
                            "Copies 42 bindings and 9 options from ~/.tmux.conf into a marked block in ~/.config/zz/zz.conf. Running it again replaces the block.",
                        )
                    });
                })
            },
        },
        Section {
            id: "add-host",
            name: "Add host",
            summary: "The add-host prompt with a destination typed in.",
            build: |window, cx| add_host(window, cx, None),
        },
        Section {
            id: "add-host-error",
            name: "Add host, refused",
            summary: "The same prompt after the destination failed, with the reason under the field.",
            build: |window, cx| {
                add_host(
                    window,
                    cx,
                    Some("ssh: Could not resolve hostname gpu-bx: nodename nor servname provided"),
                )
            },
        },
        Section {
            id: "ssh-password",
            name: "ssh password",
            summary: "A secret ssh asked for: the field never echoes.",
            build: |window, cx| {
                let input = text_input(window, cx, "correct horse");
                host(window, cx, move |window, cx| {
                    let input = input.clone();
                    window.open_dialog(cx, move |dialog, _, cx| {
                        ssh_secret_prompt_dialog(
                            dialog,
                            "Password for gpu-box",
                            "fabrico@gpu-box's password:",
                            &input,
                            cx,
                        )
                    });
                })
            },
        },
        Section {
            id: "ssh-question",
            name: "ssh question",
            summary: "A keyboard-interactive question that echoes, with a multi-line prompt.",
            build: |window, cx| {
                let input = text_input(window, cx, "1");
                host(window, cx, move |window, cx| {
                    let input = input.clone();
                    window.open_dialog(cx, move |dialog, _, cx| {
                        ssh_text_prompt_dialog(
                            dialog,
                            "Two-factor login",
                            "Duo two-factor login for fabrico\n\nEnter a passcode or select one of the following options:\n 1. Duo Push to iPhone (iOS)\n 2. Phone call to XXX-XXX-1234",
                            &input,
                            true,
                            cx,
                        )
                    });
                })
            },
        },
        Section {
            id: "ssh-host-key",
            name: "ssh host key",
            summary: "A yes or no question: an unknown host key, answered with a warning button.",
            build: |window, cx| {
                host(window, cx, |window, cx| {
                    window.open_dialog(cx, |dialog, _, cx| {
                        ssh_confirm_prompt_dialog(
                            dialog,
                            "Unknown host key",
                            "The authenticity of host 'gpu-box (10.0.0.7)' can't be established.\nED25519 key fingerprint is SHA256:Wm4C3bq9tOe8n1QpXk2s7yZf0Lr5uHvJdA6gB+TcEiY.\nThis key is not known by any other names.\nAre you sure you want to continue connecting?",
                            cx,
                        )
                    });
                })
            },
        },
        Section {
            id: "ssh-offer",
            name: "ssh offer",
            summary: "An offer the user can turn down at no cost.",
            build: |window, cx| {
                host(window, cx, |window, cx| {
                    window.open_dialog(cx, |dialog, _, cx| {
                        ssh_offer_prompt_dialog(
                            dialog,
                            "Save a key for gpu-box?",
                            "zz can create a key and add it to gpu-box's authorized_keys, so the next connection skips the password.",
                            "Save key",
                            cx,
                        )
                    });
                })
            },
        },
    ],
};

struct DialogHost;

impl Render for DialogHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w_full().h(px(420.0))
    }
}

fn host(
    window: &mut Window,
    cx: &mut App,
    open: impl FnOnce(&mut Window, &mut App) + 'static,
) -> AnyView {
    window.defer(cx, open);
    cx.new(|_| DialogHost).into()
}

fn text_input(window: &mut Window, cx: &mut App, value: &'static str) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).default_value(value))
}

fn add_host(window: &mut Window, cx: &mut App, error: Option<&'static str>) -> AnyView {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("user@host or an ssh config alias")
            .default_value(if error.is_some() {
                "gpu-bx"
            } else {
                "fabrico@gpu-box"
            })
    });
    host(window, cx, move |window, cx| {
        window.open_dialog(cx, move |dialog, _, cx| {
            add_host_prompt_dialog(dialog, &input, error.map(Into::into), cx)
        });
    })
}
