//! The dialog ssh's password and host-key questions appear in.
//!
//! ssh runs this executable as its `SSH_ASKPASS` helper (see the askpass branch
//! in `main`), the helper hands the question back over a socket, and the
//! connect thread blocks inside `connect_endpoint_with_prompts` until this
//! dialog answers. Dismissing is not "try again": it parks the host, exactly as
//! the desktop does, so a cancelled password does not reopen one rung later.

use std::{cell::Cell, rc::Rc, sync::Arc};

use adw::prelude::*;
use zz_daemon::{AskpassPromptKind, AskpassReply};

use crate::engine::{Engine, SshPromptRequest};

pub fn present(parent: &impl IsA<gtk::Widget>, engine: &Arc<Engine>, request: &SshPromptRequest) {
    match request.prompt.kind() {
        AskpassPromptKind::Secret => secret(parent, engine, request),
        AskpassPromptKind::HostKey | AskpassPromptKind::AgentConfirm => {
            confirmation(parent, engine, request);
        }
    }
}

/// ssh is waiting on exactly one answer. The dialog can fire its response more
/// than once — a default response and then a close — so the first one wins and
/// the rest are dropped.
struct Answer {
    reply: async_channel::Sender<AskpassReply>,
    sent: Rc<Cell<bool>>,
}

impl Answer {
    fn new(reply: &async_channel::Sender<AskpassReply>) -> Self {
        Self {
            reply: reply.clone(),
            sent: Rc::new(Cell::new(false)),
        }
    }

    fn handle(&self) -> Self {
        Self {
            reply: self.reply.clone(),
            sent: Rc::clone(&self.sent),
        }
    }

    fn send(&self, reply: AskpassReply) {
        if self.sent.replace(true) {
            return;
        }
        let _ = self.reply.try_send(reply);
        self.reply.close();
    }
}

fn secret(parent: &impl IsA<gtk::Widget>, engine: &Arc<Engine>, request: &SshPromptRequest) {
    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::new(
        Some(&format!("Sign in to {}", request.label)),
        Some(request.prompt.text().trim_end()),
    );
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("send", "Sign in");
    dialog.set_response_appearance("send", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("send"));
    dialog.set_close_response("cancel");

    let answer = Answer::new(&request.reply);
    let engine = Arc::clone(engine);
    let host = request.host;
    dialog.connect_response(None, move |_, response| {
        if response == "send" {
            answer.handle().send(AskpassReply::answer(entry.text()));
            return;
        }
        engine.park_host(host);
        answer.handle().send(AskpassReply::Cancel);
    });
    dialog.present(Some(parent));
    dialog.grab_focus();
}

/// Host keys and agent confirmations are the same dialog with different words,
/// and they differ in how a decline has to be spelled: a host key wants a
/// literal `no`, while an agent confirmation wants a non-zero exit.
fn confirmation(parent: &impl IsA<gtk::Widget>, engine: &Arc<Engine>, request: &SshPromptRequest) {
    let host_key = request.prompt.kind() == AskpassPromptKind::HostKey;
    let title = if host_key {
        format!("Unrecognised host key for {}", request.label)
    } else {
        format!("{} is asking for permission", request.label)
    };

    let dialog = adw::AlertDialog::new(Some(&title), Some(request.prompt.text().trim_end()));
    dialog.add_response("no", "No");
    dialog.add_response("yes", "Yes");
    dialog.set_response_appearance("yes", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("no"));
    dialog.set_close_response("no");

    let answer = Answer::new(&request.reply);
    let engine = Arc::clone(engine);
    let host = request.host;
    dialog.connect_response(None, move |_, response| {
        if response == "yes" {
            answer.handle().send(AskpassReply::answer("yes"));
            return;
        }
        engine.park_host(host);
        answer.handle().send(if host_key {
            AskpassReply::answer("no")
        } else {
            AskpassReply::Cancel
        });
    });
    dialog.present(Some(parent));
}
