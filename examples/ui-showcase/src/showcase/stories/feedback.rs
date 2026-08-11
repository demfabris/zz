//! The shared confirmation dialogs and the notification tones.

use gpui::{AnyElement, Context, ParentElement as _, prelude::*};
use zz_ui::feedback::{
    add_host_prompt_dialog, browser_clear_site_data_alert, import_configuration_alert,
    import_configuration_file_alert, ssh_confirm_prompt_dialog, ssh_secret_prompt_dialog,
};
use zz_ui::{
    IconName, WindowExt as _,
    button::{Button, ButtonVariants as _},
    notification::Notification,
};

use super::{Showcase, gallery, specimen, specimens, story_stack};

const HOST_KEY_QUESTION: &str = "The authenticity of host 'desktop (192.168.1.42)' can't be established.\nED25519 key fingerprint is SHA256:OoeOvr/b8tnPfD45Xm9k8hS2nLd0a3yKC2YoAuw0KJM.\nThis key is not known by any other names.\nAre you sure you want to continue connecting (yes/no/[fingerprint])?";

pub(super) fn render(showcase: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Confirmation dialogs",
                "These open the exact shared alert definitions used by BrowserView and SettingsView, mounted through the real Root dialog layer.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "destructive · clear data",
                        Button::new("fb-clear-data")
                            .danger()
                            .icon(IconName::Delete)
                            .label("Clear site data…")
                            .on_click(|_, window, cx| {
                                window.open_alert_dialog(cx, |alert, _, cx| {
                                    browser_clear_site_data_alert(alert, cx).on_ok(|_, window, cx| {
                                        window.push_notification(
                                            Notification::success("Site data cleared"),
                                            cx,
                                        );
                                        true
                                    })
                                });
                            }),
                        cx,
                    ))
                    .child(specimen(
                        "warning · import configuration",
                        Button::new("fb-import-config")
                            .warning()
                            .icon(IconName::File)
                            .label("Import configuration…")
                            .on_click(|_, window, cx| {
                                window.open_alert_dialog(cx, |alert, _, _| {
                                    import_configuration_alert(
                                        alert,
                                        "This copies your Ghostty appearance into ~/.config/zz/config, overwriting previously imported keys, and replaces ~/.config/zz/mux.conf with a copy of your tmux configuration. Your Ghostty and tmux files are not modified.",
                                    )
                                });
                            }),
                        cx,
                    ))
                    .child(specimen(
                        "warning · import one file",
                        Button::new("fb-import-file")
                            .warning()
                            .icon(IconName::File)
                            .label("Import mux.conf…")
                            .on_click(|_, window, cx| {
                                window.open_alert_dialog(cx, |alert, _, _| {
                                    import_configuration_file_alert(
                                        alert,
                                        "Import from tmux?",
                                        "This replaces ~/.config/zz/mux.conf with a copy of ~/.tmux.conf. Your tmux configuration is not modified.",
                                    )
                                });
                            }),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Prompt dialogs",
                "A dialog that asks for something rather than confirming it: a title, a body a shared builder owns, and the standard action row. Custom dialogs show no action row until they ask for one (declaring buttons or an on-ok handler is the opt-in), so a dialog that answers itself keeps a clean foot. Like the alerts above, these open the exact definitions the app opens: the sidebar's add-host dialog, and the two questions ssh asks through the askpass helper. Only the callbacks differ here.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "field · default action row",
                        Button::new("fb-add-host")
                            .secondary()
                            .icon(IconName::Plus)
                            .label("Add host…")
                            .on_click({
                                let input = showcase.host_input.clone();
                                move |_, window, cx| {
                                    let input = input.clone();
                                    window.open_dialog(cx, move |dialog, _, cx| {
                                        add_host_prompt_dialog(dialog, &input, None, cx)
                                    });
                                }
                            }),
                        cx,
                    ))
                    .child(specimen(
                        "secret · masked field",
                        Button::new("fb-askpass")
                            .secondary()
                            .icon(IconName::Asterisk)
                            .label("Password prompt…")
                            .on_click({
                                let input = showcase.secret_input.clone();
                                move |_, window, cx| {
                                    let input = input.clone();
                                    window.open_dialog(cx, move |dialog, _, cx| {
                                        ssh_secret_prompt_dialog(
                                            dialog,
                                            "Sign in to desktop",
                                            "fab@desktop's password:",
                                            &input,
                                            cx,
                                        )
                                    });
                                }
                            }),
                        cx,
                    ))
                    .child(specimen(
                        "confirmation · host key",
                        Button::new("fb-host-key")
                            .secondary()
                            .icon(IconName::TriangleAlert)
                            .label("Host key prompt…")
                            .on_click(|_, window, cx| {
                                window.open_dialog(cx, |dialog, _, cx| {
                                    ssh_confirm_prompt_dialog(
                                        dialog,
                                        "Unrecognised host key for desktop",
                                        HOST_KEY_QUESTION,
                                        cx,
                                    )
                                });
                            }),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Notifications",
                "The four notification tones, each pushed into the mounted Root notification layer.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "info",
                        Button::new("fb-info").secondary().label("Info").on_click(|_, window, cx| {
                            window.push_notification(
                                Notification::info("Browser profile loaded")
                                    .title("Ready")
                                    .autohide(false),
                                cx,
                            );
                        }),
                        cx,
                    ))
                    .child(specimen(
                        "success",
                        Button::new("fb-success").success().label("Success").on_click(|_, window, cx| {
                            window.push_notification(
                                Notification::success("Configuration reloaded")
                                    .title("Saved")
                                    .autohide(false),
                                cx,
                            );
                        }),
                        cx,
                    ))
                    .child(specimen(
                        "warning",
                        Button::new("fb-warning").warning().label("Warning").on_click(|_, window, cx| {
                            window.push_notification(
                                Notification::warning("The browser connection is unstable")
                                    .title("Reconnecting")
                                    .autohide(false),
                                cx,
                            );
                        }),
                        cx,
                    ))
                    .child(specimen(
                        "error",
                        Button::new("fb-error").danger().label("Error").on_click(|_, window, cx| {
                            window.push_notification(
                                Notification::error("Could not clear site data")
                                    .title("Request failed")
                                    .autohide(false),
                                cx,
                            );
                        }),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}
