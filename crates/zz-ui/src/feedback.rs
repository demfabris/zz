use crate::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
    button::ButtonVariant,
    dialog::{AlertDialog, Dialog, DialogButtonProps},
    input::{Input, InputContentType, InputState},
    overlay::dialog_description,
    rems_from_px, v_flex,
};
use gpui::{
    App, Div, Entity, ParentElement as _, SharedString, Styled as _, div,
    prelude::FluentBuilder as _,
};

#[cfg(target_os = "ios")]
const ADD_HOST_DESCRIPTION: &str = "Connects over SSH with this device's built-in key.";
#[cfg(not(target_os = "ios"))]
const ADD_HOST_DESCRIPTION: &str = "Connects over SSH · your ~/.ssh/config still applies.";

/// Browser clear-data confirmation. The caller appends the completion
/// callback with `on_ok`.
pub fn browser_clear_site_data_alert(alert: AlertDialog, cx: &App) -> AlertDialog {
    alert
        .icon(Icon::new(IconName::TriangleAlert).text_color(cx.theme().danger))
        .title("Clear site data?")
        .description(
            "This clears cookies and persistent storage owned by the current site. You may be signed out. This cannot be undone.",
        )
        .button_props(
            DialogButtonProps::default()
                .ok_variant(ButtonVariant::Danger)
                .ok_text("Clear data")
                .cancel_text("Cancel")
                .show_cancel(true),
        )
}

/// Confirmation for importing Ghostty and tmux configuration into zz-owned
/// files. The caller supplies the path-bearing description.
pub fn import_configuration_alert(
    alert: AlertDialog,
    description: impl Into<SharedString>,
) -> AlertDialog {
    import_configuration_file_alert(alert, "Import from Ghostty and tmux?", description)
}

/// Confirmation for importing one donor into one zz-owned configuration file.
pub fn import_configuration_file_alert(
    alert: AlertDialog,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
) -> AlertDialog {
    alert
        .title(title.into())
        .description(description.into())
        .button_props(
            DialogButtonProps::default()
                .ok_variant(ButtonVariant::Warning)
                .ok_text("Import")
                .cancel_text("Cancel")
                .show_cancel(true),
        )
}

/// Add-host prompt. The caller appends the submit handler with `on_ok`.
pub fn add_host_prompt_dialog(
    dialog: Dialog,
    input: &Entity<InputState>,
    error: Option<SharedString>,
    cx: &App,
) -> Dialog {
    prompt_dialog(dialog, "Add host")
        .button_props(
            DialogButtonProps::default()
                .ok_text("Add")
                .cancel_text("Cancel")
                .show_cancel(true),
        )
        .child(
            prompt_body()
                .child(dialog_description(cx).child(ADD_HOST_DESCRIPTION))
                .child(Input::new(input).small())
                .when_some(error, |this, error| {
                    this.child(
                        div()
                            .text_size(rems_from_px(11.0))
                            .text_color(cx.theme().warning)
                            .child(error),
                    )
                }),
        )
}

/// Prompt for a secret ssh asked for: a password, a key passphrase, or a
/// keyboard-interactive question. The caller answers the channel from
/// `on_ok`/`on_cancel`.
pub fn ssh_secret_prompt_dialog(
    dialog: Dialog,
    title: impl Into<SharedString>,
    question: &str,
    input: &Entity<InputState>,
    cx: &App,
) -> Dialog {
    prompt_dialog(dialog, title)
        .button_props(
            DialogButtonProps::default()
                .ok_text("Continue")
                .cancel_text("Cancel")
                .show_cancel(true),
        )
        .child(
            prompt_body().child(prompt_question(question, cx)).child(
                Input::new(input)
                    .small()
                    .content_type(InputContentType::Password),
            ),
        )
}

/// Confirmation for ssh's yes/no questions: an unrecognised host key, or an
/// agent asking permission to sign.
pub fn ssh_confirm_prompt_dialog(
    dialog: Dialog,
    title: impl Into<SharedString>,
    question: &str,
    cx: &App,
) -> Dialog {
    prompt_dialog(dialog, title)
        .button_props(
            DialogButtonProps::default()
                .ok_variant(ButtonVariant::Warning)
                .ok_text("Yes, connect")
                .cancel_text("No")
                .show_cancel(true),
        )
        .child(prompt_question(question, cx))
}

fn prompt_dialog(dialog: Dialog, title: impl Into<SharedString>) -> Dialog {
    dialog
        .title(title.into())
        .close_button(false)
        .overlay_closable(false)
}

fn prompt_body() -> Div {
    v_flex().gap_2()
}

fn prompt_question(question: &str, cx: &App) -> Div {
    dialog_description(cx)
        .v_flex()
        .gap_1()
        .children(question.lines().map(|line| div().child(line.to_owned())))
}
