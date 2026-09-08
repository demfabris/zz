use std::collections::BTreeSet;

use zz_protocol::{
    CommandInvocation, PaneKindSnapshot, PaneSnapshot, SessionId, WindowId, WindowSnapshot,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Session(SessionId),
    Window(WindowId),
}

pub fn rename_prompt_command(
    target: RenameTarget,
    current_name: &str,
) -> (&'static str, CommandInvocation) {
    let (label, prompt, template) = match target {
        RenameTarget::Session(session) => (
            "Rename Session…",
            "rename-session: ",
            format!("rename-session -t '{session}' -- '%%'"),
        ),
        RenameTarget::Window(window) => (
            "Rename Window…",
            "rename-window: ",
            format!("rename-window -t '{window}' -- '%%'"),
        ),
    };
    (
        label,
        CommandInvocation::new(
            "command-prompt",
            ["-p", prompt, "-I", current_name, &template],
        ),
    )
}

#[must_use]
pub fn ordered_panes(window: &WindowSnapshot) -> Vec<&PaneSnapshot> {
    let mut layout_order = Vec::with_capacity(window.panes.len());
    window.layout.panes(&mut layout_order);
    let mut seen = BTreeSet::new();
    let mut panes = Vec::with_capacity(window.panes.len());
    for id in layout_order {
        if seen.insert(id)
            && let Some(pane) = window.panes.get(&id)
        {
            panes.push(pane);
        }
    }
    for (id, pane) in &window.panes {
        if seen.insert(*id) {
            panes.push(pane);
        }
    }
    panes
}

#[must_use]
pub fn pane_label(pane: &PaneSnapshot) -> String {
    let title = pane.title.trim();
    if !title.is_empty() {
        return title.to_owned();
    }
    match &pane.kind {
        PaneKindSnapshot::Picker => "new pane".to_owned(),
        PaneKindSnapshot::Terminal => "terminal".to_owned(),
        PaneKindSnapshot::Browser(browser) if !browser.url().trim().is_empty() => {
            browser.url().trim().to_owned()
        }
        PaneKindSnapshot::Browser(_) => "browser".to_owned(),
        PaneKindSnapshot::Agent(_) => "agent".to_owned(),
        PaneKindSnapshot::Editor(_) => "editor".to_owned(),
    }
}

pub fn session_label(name: &str, id: SessionId) -> String {
    if name.trim().is_empty() {
        format!("session {id}")
    } else {
        name.to_owned()
    }
}
