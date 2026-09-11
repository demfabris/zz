use crate::{
    config::{file, import::MAX_MUX_CONFIG_BYTES},
    engine::{Engine, HostId},
};
use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use zz_mux::settings_bindings::{
    SplitDirection, SplitPaneKind, preferred_split_binding, split_binding_kind,
    update_split_binding,
};
use zz_protocol::{CommandInvocation, KeyBindingSnapshot};

const DIRECTIONS: [SplitDirection; 2] = [SplitDirection::Vertical, SplitDirection::Horizontal];
const KINDS: [SplitPaneKind; 3] = [
    SplitPaneKind::Picker,
    SplitPaneKind::Terminal,
    SplitPaneKind::Browser,
];

struct SplitRow {
    row: adw::EntryRow,
    kind: gtk::DropDown,
    binding: RefCell<Option<KeyBindingSnapshot>>,
    preferred: RefCell<Option<String>>,
}

struct Pending {
    key: String,
    removed: Option<String>,
    kind: SplitPaneKind,
    direction: SplitDirection,
    started: Instant,
}

pub struct MuxEditor {
    engine: Arc<Engine>,
    groups: [adw::PreferencesGroup; 3],
    view: gtk::TextView,
    status: gtk::Label,
    sources: gtk::Label,
    copied: adw::ActionRow,
    path: Option<PathBuf>,
    saved: RefCell<Option<String>>,
    save: gtk::Button,
    reload: gtk::Button,
    splits: [SplitRow; 2],
    syncing: Cell<bool>,
    pending: RefCell<Option<Pending>>,
}

impl MuxEditor {
    pub fn new(engine: Arc<Engine>) -> Rc<Self> {
        let path = zz_daemon::mux_config_write_path();
        let view = gtk::TextView::builder()
            .monospace(true)
            .editable(false)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(8)
            .right_margin(8)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&view)
            .min_content_height(300)
            .vexpand(true)
            .build();
        scroller.add_css_class("card");
        let status = gtk::Label::builder().xalign(0.0).wrap(true).build();
        status.add_css_class("caption");
        let sources = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .build();
        let files = adw::PreferencesGroup::builder()
            .title("Configuration files")
            .description("zz reads existing tmux files in order, then applies zz/mux.conf.")
            .build();
        files.add(&sources);
        let copied = adw::ActionRow::builder()
            .title("Copied tmux configuration")
            .build();
        copied.set_subtitle_lines(0);
        let remove = gtk::Button::with_label("Remove copied lines");
        remove.set_valign(gtk::Align::Center);
        copied.add_suffix(&remove);
        files.add(&copied);
        let split_group = adw::PreferencesGroup::builder()
            .title("Split panes")
            .description(
                "Choose the pane type and prefix shortcut. Custom commands stay in the editor.",
            )
            .build();
        let splits = ["Split below", "Split right"].map(|title| {
            let row = adw::EntryRow::builder().title(title).build();
            let kind =
                gtk::DropDown::from_strings(&["Pane picker", "Terminal", "Browser", "Custom"]);
            kind.set_valign(gtk::Align::Center);
            row.add_prefix(&gtk::Label::new(Some("Prefix +")));
            row.add_suffix(&kind);
            split_group.add(&row);
            SplitRow {
                row,
                kind,
                binding: RefCell::new(None),
                preferred: RefCell::new(None),
            }
        });
        let save = gtk::Button::with_label("Save");
        save.add_css_class("suggested-action");
        let reload = gtk::Button::with_label("Reload");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_valign(gtk::Align::Center);
        buttons.append(&reload);
        buttons.append(&save);
        let group = adw::PreferencesGroup::builder()
            .title("Overrides")
            .description(path.as_ref().map_or_else(
                || "No writable zz/mux.conf location.".into(),
                |path| path.display().to_string(),
            ))
            .header_suffix(&buttons)
            .build();
        group.add(&scroller);
        group.add(&status);
        let editor = Rc::new(Self {
            engine,
            groups: [files, split_group, group],
            view,
            status,
            sources,
            copied,
            path,
            saved: RefCell::new(None),
            save,
            reload,
            splits,
            syncing: Cell::new(false),
            pending: RefCell::new(None),
        });
        let weak = Rc::downgrade(&editor);
        editor.save.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.save_text(&editor.text());
            }
        });
        let weak = Rc::downgrade(&editor);
        editor.reload.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.pending.replace(None);
                editor.refresh();
                editor.request_reload();
            }
        });
        let weak = Rc::downgrade(&editor);
        editor.view.buffer().connect_changed(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.sync_controls();
            }
        });
        let weak = Rc::downgrade(&editor);
        remove.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade()
                && let Some((_, rest)) = copied_prefix(&editor.text())
            {
                editor.view.buffer().set_text(&rest);
                editor
                    .status
                    .set_text("Copied lines removed from the draft. Save to apply.");
            }
        });
        for index in 0..2 {
            let weak = Rc::downgrade(&editor);
            editor.splits[index].row.connect_entry_activated(move |_| {
                if let Some(editor) = weak.upgrade() {
                    editor.commit_split(index);
                }
            });
            let weak = Rc::downgrade(&editor);
            editor.splits[index].kind.connect_selected_notify(move |_| {
                if let Some(editor) = weak.upgrade()
                    && !editor.syncing.get()
                {
                    editor.commit_split(index);
                }
            });
        }
        editor.refresh();
        editor
    }

    pub fn groups(&self) -> &[adw::PreferencesGroup; 3] {
        &self.groups
    }

    fn text(&self) -> String {
        let buffer = self.view.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string()
    }

    fn dirty(&self) -> bool {
        self.saved
            .borrow()
            .as_ref()
            .is_some_and(|saved| *saved != self.text())
    }

    fn local(&self) -> bool {
        self.engine.active_host() == HostId::LOCAL
            && self.engine.hosts().iter().any(|host| {
                host.id == HostId::LOCAL && host.state.is_connected() && host.attached.is_some()
            })
    }

    pub fn refresh(&self) {
        if !self.dirty()
            && let Some(path) = &self.path
        {
            match file::read_editor_source(path, MAX_MUX_CONFIG_BYTES) {
                Ok(source) => {
                    self.saved.replace(Some(source.clone()));
                    self.view.set_editable(true);
                    if self.text() != source {
                        self.view.buffer().set_text(&source);
                    }
                }
                Err(error) => {
                    self.saved.replace(None);
                    self.view.set_editable(false);
                    self.status
                        .set_text(&format!("Could not read configuration: {error}"));
                }
            }
        }
        let mut paths = zz_daemon::tmux_config_candidates()
            .into_iter()
            .filter(|p| p.is_file())
            .collect::<Vec<_>>();
        if let Some(path) = &self.path {
            paths.push(path.clone());
        }
        self.sources.set_text(
            &paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        self.sync_controls();
    }

    pub fn sync_controls(&self) {
        let bindings = self.engine.prefix_bindings();
        let mut pending = self.pending.borrow_mut();
        if let Some(waiting) = pending.as_ref() {
            let confirmed = bindings.iter().any(|b| {
                b.key == waiting.key
                    && split_binding_kind(b, waiting.direction) == Some(waiting.kind)
            }) && waiting
                .removed
                .as_ref()
                .is_none_or(|key| !bindings.iter().any(|b| b.key == *key));
            if confirmed || !self.local() {
                *pending = None;
            } else if waiting.started.elapsed() >= Duration::from_secs(5) {
                *pending = None;
                self.status.set_text("The saved shortcut has not appeared. Review the configuration or retry Reload.");
            }
        }
        let blocked = pending.is_some();
        drop(pending);
        let dirty = self.dirty();
        let writable = self.local() && self.saved.borrow().is_some() && self.path.is_some();
        self.save.set_sensitive(writable && dirty);
        self.reload.set_sensitive(writable && !dirty);
        let copy = copied_prefix(&self.text());
        self.copied.set_visible(copy.is_some());
        if let Some((path, _)) = copy {
            self.copied.set_subtitle(&format!(
                "These lines duplicate {} and can hide later edits to that file.",
                path.display()
            ));
        }
        self.syncing.set(true);
        for (row, direction) in self.splits.iter().zip(DIRECTIONS) {
            row.row.set_sensitive(writable && !dirty && !blocked);
            if blocked {
                continue;
            }
            let preferred = row.preferred.borrow();
            let binding = preferred
                .as_ref()
                .and_then(|key| bindings.iter().find(|b| b.key == *key))
                .filter(|b| preferred_split_binding(std::slice::from_ref(*b), direction).is_some())
                .or_else(|| preferred_split_binding(&bindings, direction))
                .cloned();
            if *row.binding.borrow() != binding
                || !row
                    .row
                    .root()
                    .and_then(|root| root.focus())
                    .is_some_and(|focus| focus == row.row || focus.is_ancestor(&row.row))
            {
                let key = binding.as_ref().map_or(
                    match direction {
                        SplitDirection::Vertical => "\"",
                        SplitDirection::Horizontal => "%",
                    },
                    |b| b.key.as_str(),
                );
                if row.row.text() != key {
                    row.row.set_text(key);
                }
                let kind = binding.as_ref().map_or(Some(SplitPaneKind::Picker), |b| {
                    split_binding_kind(b, direction)
                });
                row.kind.set_selected(
                    kind.and_then(|kind| KINDS.iter().position(|k| *k == kind))
                        .unwrap_or(3) as u32,
                );
                row.binding.replace(binding);
            }
        }
        self.syncing.set(false);
    }

    fn save_text(&self, source: &str) -> bool {
        if !self.local() {
            self.status
                .set_text("Connect to a local session to save and reload configuration.");
            return false;
        }
        let Some(path) = &self.path else {
            return false;
        };
        match file::read_editor_source(path, MAX_MUX_CONFIG_BYTES) {
            Ok(current) if self.saved.borrow().as_ref() == Some(&current) => {}
            Ok(_) => {
                self.status.set_text("Configuration changed on disk. Your draft was kept; reopen after reconciling the file.");
                return false;
            }
            Err(error) => {
                self.status
                    .set_text(&format!("Could not read configuration: {error}"));
                return false;
            }
        }
        match file::write_editor_source(path, source, MAX_MUX_CONFIG_BYTES) {
            Ok(()) => {
                self.saved.replace(Some(source.to_owned()));
                if self.text() != source {
                    self.view.buffer().set_text(source);
                }
                self.pending.replace(None);
                self.request_reload();
                self.status
                    .set_text("Saved. The local daemon is reloading configuration.");
                self.sync_controls();
                true
            }
            Err(error) => {
                self.status
                    .set_text(&format!("Could not save configuration: {error}"));
                false
            }
        }
    }

    fn request_reload(&self) {
        if self.local() {
            self.engine.execute_on(
                HostId::LOCAL,
                CommandInvocation::new("reload-config", [] as [&str; 0]),
            );
        }
    }

    fn commit_split(&self, index: usize) {
        if self.syncing.get() || self.dirty() || !self.local() || self.pending.borrow().is_some() {
            return;
        }
        let row = &self.splits[index];
        let Some(kind) = KINDS.get(row.kind.selected() as usize).copied() else {
            return;
        };
        let bindings = self.engine.prefix_bindings();
        let original = row.binding.borrow().clone();
        if original
            .as_ref()
            .is_some_and(|b| bindings.iter().find(|live| live.key == b.key) != Some(b))
        {
            self.status
                .set_text("The shortcut changed. Review it and try again.");
            self.sync_controls();
            return;
        }
        let key = row.row.text();
        let saved = self.saved.borrow().clone().unwrap_or_default();
        let source = match update_split_binding(
            &saved,
            &bindings,
            original.as_ref(),
            DIRECTIONS[index],
            &key,
            kind,
        ) {
            Ok(source) => source,
            Err(error) => {
                self.status.set_text(&error);
                self.sync_controls();
                return;
            }
        };
        if source == saved {
            return;
        }
        if self.save_text(&source) {
            let key = zz_mux::parse_tmux_key(key.trim()).expect("validated shortcut");
            row.preferred.replace(Some(key.clone()));
            self.pending.replace(Some(Pending {
                removed: original.map(|b| b.key).filter(|old| *old != key),
                key,
                kind,
                direction: DIRECTIONS[index],
                started: Instant::now(),
            }));
            self.sync_controls();
        }
    }
}

fn copied_prefix(source: &str) -> Option<(PathBuf, String)> {
    zz_daemon::tmux_config_candidates()
        .into_iter()
        .find_map(|path| {
            let donor = file::read_editor_source(&path, MAX_MUX_CONFIG_BYTES).ok()?;
            (!donor.trim().is_empty()).then_some(())?;
            source
                .strip_prefix(&donor)
                .map(|rest| (path, rest.to_owned()))
        })
}
