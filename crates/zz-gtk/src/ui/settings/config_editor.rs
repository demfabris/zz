use std::{cell::RefCell, path::PathBuf, rc::Rc};

use adw::prelude::*;

use crate::config::file;

pub struct ConfigEditor {
    group: adw::PreferencesGroup,
    view: gtk::TextView,
    save: gtk::Button,
    status: gtk::Label,
    document: RefCell<Option<Document>>,
    saved: RefCell<Option<Rc<dyn Fn()>>>,
}

struct Document {
    path: PathBuf,
    source: String,
}

impl Document {
    fn load(path: PathBuf) -> Result<Self, String> {
        let source = file::read_editor_source(&path, file::MAX_CONFIG_BYTES)
            .map_err(|error| error.to_string())?;
        Ok(Self { path, source })
    }

    fn write(&mut self, source: &str) -> Result<(), String> {
        let current = file::read_editor_source(&self.path, file::MAX_CONFIG_BYTES)
            .map_err(|error| error.to_string())?;
        if current != self.source {
            return Err(
                "The configuration changed on disk. Copy your edits before reloading the file."
                    .to_owned(),
            );
        }
        file::write_editor_source(&self.path, source, file::MAX_CONFIG_BYTES)
            .map_err(|error| error.to_string())?;
        source.clone_into(&mut self.source);
        Ok(())
    }
}

impl ConfigEditor {
    pub fn new() -> Rc<Self> {
        let view = gtk::TextView::builder()
            .monospace(true)
            .editable(false)
            .wrap_mode(gtk::WrapMode::None)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(8)
            .right_margin(8)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&view)
            .min_content_height(360)
            .vexpand(true)
            .build();
        let status = gtk::Label::builder().xalign(0.0).wrap(true).build();
        status.add_css_class("caption");
        let save = gtk::Button::with_label("Save");
        save.add_css_class("suggested-action");
        save.set_sensitive(false);
        let reload = gtk::Button::with_label("Reload");
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.set_valign(gtk::Align::Center);
        actions.append(&reload);
        actions.append(&save);
        let group = adw::PreferencesGroup::builder()
            .title("Configuration file")
            .header_suffix(&actions)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&scroller);
        content.append(&status);
        group.add(&content);
        let editor = Rc::new(Self {
            group,
            view,
            save,
            status,
            document: RefCell::new(None),
            saved: RefCell::new(None),
        });
        let target = Rc::downgrade(&editor);
        editor.view.buffer().connect_changed(move |_| {
            if let Some(editor) = target.upgrade() {
                editor.update_save();
            }
        });
        let target = Rc::downgrade(&editor);
        editor.save.connect_clicked(move |_| {
            if let Some(editor) = target.upgrade() {
                editor.save();
            }
        });
        let target = Rc::downgrade(&editor);
        reload.connect_clicked(move |_| {
            if let Some(editor) = target.upgrade() {
                editor.confirm_reload();
            }
        });
        editor.refresh();
        editor
    }

    pub fn group(&self) -> &adw::PreferencesGroup {
        &self.group
    }

    pub fn connect_saved(&self, callback: Rc<dyn Fn()>) {
        self.saved.replace(Some(callback));
    }

    pub fn refresh(&self) {
        if !self.is_dirty() {
            self.reload();
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.document
            .borrow()
            .as_ref()
            .is_some_and(|document| self.text() != document.source)
    }

    fn text(&self) -> String {
        let buffer = self.view.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string()
    }

    fn update_save(&self) {
        let valid_size = self.text().len() <= file::MAX_CONFIG_BYTES;
        self.save.set_sensitive(valid_size && self.is_dirty());
        if !valid_size {
            self.status
                .set_text("The configuration exceeds the 64 KiB limit.");
        }
    }

    fn reload(&self) {
        match file::path_for_write()
            .map_err(|error| error.to_string())
            .and_then(Document::load)
        {
            Ok(document) => {
                self.group.set_description(Some(&format!(
                    "Shared zz/config: {}",
                    document.path.display()
                )));
                let source = document.source.clone();
                self.document.replace(Some(document));
                self.view.buffer().set_text(&source);
                self.view.set_editable(true);
                self.status.set_text("");
            }
            Err(error) => self
                .status
                .set_text(&format!("Could not read zz/config: {error}")),
        }
        self.update_save();
    }

    fn save(&self) {
        let source = self.text();
        let result = {
            let mut slot = self.document.borrow_mut();
            let Some(document) = slot.as_mut() else {
                return;
            };
            file::path_for_write().map_err(|error| error.to_string()).and_then(|path| {
                if path != document.path {
                    return Err("The active configuration file changed. Copy your edits before reloading.".to_owned());
                }
                document.write(&source)
            })
        };
        match result {
            Ok(()) => {
                self.status.set_text("Saved");
                let callback = self.saved.borrow().clone();
                if let Some(callback) = callback {
                    callback();
                }
            }
            Err(error) => self
                .status
                .set_text(&format!("Could not save zz/config: {error}")),
        }
        self.update_save();
    }

    fn confirm_reload(self: &Rc<Self>) {
        if !self.is_dirty() {
            self.reload();
            return;
        }
        let dialog = adw::AlertDialog::new(
            Some("Discard unsaved changes?"),
            Some("Reloading replaces your draft with the configuration on disk."),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("reload", "Discard and Reload")]);
        dialog.set_response_appearance("reload", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let target = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            if response == "reload"
                && let Some(editor) = target.upgrade()
            {
                editor.reload();
            }
        });
        dialog.present(Some(&self.group));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn saving_detects_external_changes_and_keeps_them() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("zz-config-editor-{}-{unique}", std::process::id()));
        std::fs::write(&path, "font-size = 13\n").unwrap();
        let mut document = Document::load(path.clone()).unwrap();
        std::fs::write(&path, "font-size = 17\n").unwrap();
        assert!(document.write("font-size = 15\n").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "font-size = 17\n");
        let mut document = Document::load(path.clone()).unwrap();
        document.write("font-size = 15\n").unwrap();
        assert_eq!(document.source, "font-size = 15\n");
        assert!(
            document
                .write(&"x".repeat(file::MAX_CONFIG_BYTES + 1))
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "font-size = 15\n");
        std::fs::remove_file(path).unwrap();
    }
}
