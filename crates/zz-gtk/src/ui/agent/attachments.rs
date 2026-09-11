use std::rc::Rc;

use gtk::{gio, glib, prelude::*};
use zz_protocol::{AgentImage, MAX_AGENT_PROMPT_BYTES, MAX_AGENT_PROMPT_IMAGES};

use super::AgentPane;

enum Attachment {
    Image(AgentImage),
    Text(String),
}

impl AgentPane {
    pub(super) fn choose_attachments(self: &Rc<Self>) {
        if !self.available() {
            return;
        }
        let parent = self.root.root().and_downcast::<gtk::Window>();
        let dialog = gtk::FileDialog::builder()
            .title("Attach images or text files")
            .accept_label("Attach")
            .modal(true)
            .build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Images and text files"));
        for mime in [
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "text/*",
            "application/json",
            "application/javascript",
            "application/xml",
        ] {
            filter.add_mime_type(mime);
        }
        let all = gtk::FileFilter::new();
        all.set_name(Some("All files"));
        all.add_pattern("*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        filters.append(&all);
        dialog.set_filters(Some(&filters));
        dialog.set_default_filter(Some(&filter));
        let weak = Rc::downgrade(self);
        let session = self
            .state
            .borrow()
            .as_ref()
            .and_then(|state| state.session_id.clone());
        glib::MainContext::default().spawn_local(async move {
            let files = match dialog.open_multiple_future(parent.as_ref()).await {
                Ok(files) => files,
                Err(error) => {
                    if !error.matches(gtk::DialogError::Dismissed)
                        && let Some(owner) = weak.upgrade().filter(|owner| owner.available())
                    {
                        owner.show_error(&format!("Files could not be selected: {error}"));
                    }
                    return;
                }
            };
            if files.n_items() > MAX_AGENT_PROMPT_IMAGES as u32 {
                if let Some(owner) = weak.upgrade().filter(|owner| owner.available()) {
                    owner.show_error("Select no more than 64 files at once.");
                }
                return;
            }
            for index in 0..files.n_items() {
                let Some(file) = files.item(index).and_downcast::<gio::File>() else {
                    continue;
                };
                let Some(owner) = weak.upgrade().filter(|owner| owner.available()) else {
                    return;
                };
                if owner
                    .state
                    .borrow()
                    .as_ref()
                    .and_then(|state| state.session_id.clone())
                    != session
                {
                    return;
                }
                let name = file.basename().map_or_else(
                    || "Attachment".into(),
                    |name| name.to_string_lossy().into_owned(),
                );
                let result = read_file(&file).await.and_then(classify);
                if !owner.available()
                    || owner
                        .state
                        .borrow()
                        .as_ref()
                        .and_then(|state| state.session_id.clone())
                        != session
                {
                    return;
                }
                match result {
                    Ok(Attachment::Image(image)) => {
                        if !owner.images_supported.get() {
                            owner.show_error("This Agent does not accept image attachments.");
                            continue;
                        }
                        if owner.attachments.borrow().len() >= MAX_AGENT_PROMPT_IMAGES {
                            owner.show_error("Too many image attachments.");
                            break;
                        }
                        if owner.prompt_bytes().saturating_add(image.data.len())
                            > MAX_AGENT_PROMPT_BYTES
                        {
                            owner.show_error(
                                "The message and attachments must total no more than 6 MiB.",
                            );
                            continue;
                        }
                        owner.attachments.borrow_mut().push(image);
                        owner.attachment_names.borrow_mut().push(name);
                        owner.update_actions();
                    }
                    Ok(Attachment::Text(text)) => {
                        let text = file_text(&name, &text);
                        if owner
                            .prompt_bytes()
                            .saturating_add(text.len())
                            .saturating_add(2)
                            > MAX_AGENT_PROMPT_BYTES
                        {
                            owner.show_error(
                                "The message and attachments must total no more than 6 MiB.",
                            );
                            continue;
                        }
                        let mut draft = owner.draft();
                        if !draft.is_empty() {
                            draft.push_str("\n\n");
                        }
                        draft.push_str(&text);
                        owner.composer.buffer().set_text(&draft);
                    }
                    Err(error) => owner.show_error(&format!("{name}: {error}")),
                }
            }
        });
    }

    pub(super) fn prompt_bytes(&self) -> usize {
        self.attachments
            .borrow()
            .iter()
            .fold(self.draft().len(), |bytes, image| {
                bytes.saturating_add(image.data.len())
            })
    }
}

async fn read_file(file: &gio::File) -> Result<Vec<u8>, String> {
    if file.path().is_none() {
        return Err("Choose a file available on this computer.".into());
    }
    let info = file
        .query_info_future(
            "standard::type,standard::size",
            gio::FileQueryInfoFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await
        .map_err(|error| error.to_string())?;
    if info.file_type() != gio::FileType::Regular {
        return Err("Choose a regular image or text file.".into());
    }
    if info.size() > MAX_AGENT_PROMPT_BYTES as i64 {
        return Err("The file is larger than 6 MiB.".into());
    }
    let stream = file
        .read_future(glib::Priority::DEFAULT)
        .await
        .map_err(|error| error.to_string())?;
    let mut result = Vec::new();
    loop {
        let bytes = stream
            .read_bytes_future(
                (MAX_AGENT_PROMPT_BYTES + 1 - result.len()).min(64 * 1024),
                glib::Priority::DEFAULT,
            )
            .await
            .map_err(|error| error.to_string())?;
        if bytes.is_empty() {
            return Ok(result);
        }
        result.extend_from_slice(&bytes);
        if result.len() > MAX_AGENT_PROMPT_BYTES {
            return Err("The file is larger than 6 MiB.".into());
        }
    }
}

fn classify(bytes: Vec<u8>) -> Result<Attachment, String> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        Some("image/webp")
    } else {
        None
    };
    if let Some(mime) = mime {
        return Ok(Attachment::Image(AgentImage {
            format: mime.into(),
            data: bytes,
        }));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        "Only PNG, JPEG, GIF, WebP, and UTF-8 text files can be attached.".to_owned()
    })?;
    if text.contains('\0') {
        return Err("This file contains binary data. Choose an image or text file.".into());
    }
    Ok(Attachment::Text(text))
}

fn file_text(name: &str, text: &str) -> String {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.saturating_add(1).max(3));
    format!("File: {name}\n{fence}\n{text}\n{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_types_come_from_content_and_preserve_encoded_bytes() {
        let bytes = b"\x89PNG\r\n\x1a\npayload".to_vec();
        let Attachment::Image(image) = classify(bytes.clone()).unwrap() else {
            panic!("expected image");
        };
        assert_eq!(image.format, "image/png");
        assert_eq!(image.data, bytes);
        assert!(classify(vec![0xff, 0xfe, 0]).is_err());
        assert!(classify(b"hello\0world".to_vec()).is_err());
    }

    #[test]
    fn text_files_are_visible_content_with_balanced_fences() {
        let text = "```rust\nfn main() {}\n```";
        let attached = file_text("README.md", text);
        assert!(attached.starts_with("File: README.md\n````\n"));
        assert!(attached.ends_with("\n````"));
        assert!(attached.contains(text));
    }
}
