use gpui::{AppContext as _, Context, Focusable as _, Window};
use zz_ui::{Root, WindowExt as _, input::InputState};

use super::AppShell;

impl AppShell {
    pub(super) fn sync_authentication(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.connection.read(cx).pending_auth_prompt().cloned() else {
            self.auth_prompt_id = None;
            return;
        };
        if self.auth_prompt_id == Some(prompt.id)
            || window
                .root::<Root>()
                .flatten()
                .is_some_and(|root| root.read(cx).has_active_dialog())
        {
            return;
        }
        self.auth_prompt_id = Some(prompt.id);
        let connection = self.connection.clone();
        let input = (prompt.kind == zz_daemon::AskpassPromptKind::Secret)
            .then(|| cx.new(|cx| InputState::new(window, cx)));
        let focus = input.as_ref().map(|input| input.focus_handle(cx));
        window.open_dialog(cx, move |dialog, _, cx| {
            let confirm = connection.clone();
            let cancel = connection.clone();
            let dialog = if let Some(input) = &input {
                let answer = input.clone();
                zz_ui::feedback::ssh_text_prompt_dialog(
                    dialog,
                    "Sign in to host",
                    &prompt.text,
                    input,
                    prompt.echo,
                    cx,
                )
                .on_ok(move |_, _, cx| {
                    let value = answer.read(cx).value().to_string();
                    confirm.update(cx, |connection, cx| {
                        connection.answer_auth_prompt(prompt.id, Some(value), cx)
                    });
                    true
                })
            } else {
                let title = if prompt.kind == zz_daemon::AskpassPromptKind::HostKey {
                    "Trust host key?"
                } else {
                    "Allow authentication?"
                };
                zz_ui::feedback::ssh_confirm_prompt_dialog(dialog, title, &prompt.text, cx).on_ok(
                    move |_, _, cx| {
                        confirm.update(cx, |connection, cx| {
                            connection.answer_auth_prompt(prompt.id, Some("yes".into()), cx)
                        });
                        true
                    },
                )
            };
            dialog.on_cancel(move |_, _, cx| {
                cancel.update(cx, |connection, cx| {
                    connection.answer_auth_prompt(prompt.id, None, cx)
                });
                true
            })
        });
        if let Some(focus) = focus {
            focus.focus(window, cx);
        }
    }
}
