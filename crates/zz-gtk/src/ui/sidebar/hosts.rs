//! The sidebar's Add host dialog, and the shared rules behind it.
//!
//! The parsing is the desktop's `parse_add_host`, restated against the same
//! validator the daemon exports: both clients write the same `host-<name>` line
//! into the same file, so what one accepts the other has to.

use std::sync::Arc;

use adw::prelude::*;

use zz_daemon::{Endpoint, validate_fleet_host};

use crate::{config, engine::Engine};

pub const PLACEHOLDER: &str = "user@desktop";

#[derive(Debug, PartialEq, Eq)]
pub struct AddHostRequest {
    pub name: String,
    pub endpoint: String,
}

/// Ask for a destination and write the line. Nothing dials from here: the
/// settings poll reads the file back and the fleet follows it, which is the
/// same path a hand edit takes.
pub fn add(parent: &impl IsA<gtk::Widget>, engine: &Arc<Engine>) {
    let entry = gtk::Entry::builder()
        .placeholder_text(PLACEHOLDER)
        .activates_default(true)
        .build();
    let error = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .build();
    error.add_css_class("error");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&entry);
    content.append(&error);

    let dialog = adw::AlertDialog::new(
        Some("Add a host"),
        Some("zz reaches a host over plain ssh. Give it a destination the way ssh would take one."),
    );
    dialog.set_extra_child(Some(&content));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("add", "Add");
    dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("add"));
    dialog.set_close_response("cancel");

    // The dialog is already closing by the time a response arrives, so the
    // destination is judged as it is typed and Add stays disabled until it
    // would be accepted. Nothing here re-implements the rules: it is the same
    // parse the write goes through.
    dialog.set_response_enabled("add", false);
    let existing = configured_names(engine);
    let validated = dialog.clone();
    let message = error;
    let typing = entry.clone();
    entry.connect_changed(move |entry| {
        let typed = entry.text();
        match parse_add_host(&typed, &existing) {
            Ok(_) => {
                validated.set_response_enabled("add", true);
                message.set_visible(false);
            }
            Err(problem) => {
                validated.set_response_enabled("add", false);
                message.set_text(&problem);
                message.set_visible(!typed.trim().is_empty());
            }
        }
    });

    let engine = Arc::clone(engine);
    dialog.connect_response(None, move |_, response| {
        if response != "add" {
            return;
        }
        let Ok(request) = parse_add_host(&entry.text(), &configured_names(&engine)) else {
            return;
        };
        match config::write_host(&request.name, Some(&request.endpoint)) {
            Ok(()) => log::info!(
                target: "zz_gtk::config",
                "added fleet host name={} endpoint={}",
                request.name,
                request.endpoint,
            ),
            Err(problem) => engine.notify(format!("Could not write zz/config: {problem}")),
        }
    });
    dialog.present(Some(parent));
    typing.grab_focus();
}

/// The hosts already in the file, local excluded — it is not one of them and
/// its name is reserved anyway.
fn configured_names(engine: &Engine) -> Vec<String> {
    engine
        .hosts()
        .into_iter()
        .skip(1)
        .map(|host| host.name)
        .collect()
}

/// The destination as typed, resolved into the line the file will carry. The
/// host component alone names the entry, so `fabrico@desktop:2222` is the host
/// `desktop`.
pub fn parse_add_host(input: &str, existing: &[String]) -> Result<AddHostRequest, String> {
    let destination = input.trim();
    let destination = destination.strip_prefix("ssh://").unwrap_or(destination);
    if destination.is_empty() {
        return Err(format!("Enter a host, like `{PLACEHOLDER}`."));
    }
    if destination.chars().any(char::is_whitespace) {
        return Err("A host destination must not contain spaces.".to_owned());
    }

    let endpoint = format!("ssh://{destination}");
    let Endpoint::Ssh(parsed) = Endpoint::parse(&endpoint)
        .map_err(|error| format!("{destination} is not a host: {error}"))?
    else {
        return Err(format!("{destination} is not an ssh destination."));
    };
    let name = parsed.host;
    validate_fleet_host(&name, &endpoint)?;
    if existing.contains(&name) {
        return Err(format!("A host named `{name}` already exists."));
    }
    Ok(AddHostRequest { name, endpoint })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<AddHostRequest, String> {
        parse_add_host(input, &["desktop".to_owned()])
    }

    /// The desktop's own cases, so the two clients cannot drift on what they
    /// will write into the file they share.
    #[test]
    fn the_host_component_alone_names_the_entry() {
        assert_eq!(
            parse("  fabrico@arch-desktop  ").expect("a destination"),
            AddHostRequest {
                name: "arch-desktop".to_owned(),
                endpoint: "ssh://fabrico@arch-desktop".to_owned(),
            }
        );
        assert_eq!(
            parse("gpu-box:2222").expect("a destination with a port"),
            AddHostRequest {
                name: "gpu-box".to_owned(),
                endpoint: "ssh://gpu-box:2222".to_owned(),
            }
        );
        assert_eq!(parse("ssh://gpu-box").expect("an ssh uri").name, "gpu-box");
    }

    #[test]
    fn empty_spaced_reserved_duplicate_and_malformed_entries_are_rejected() {
        assert_eq!(
            parse("   ").expect_err("empty"),
            format!("Enter a host, like `{PLACEHOLDER}`.")
        );
        assert_eq!(
            parse("arch desktop").expect_err("spaced"),
            "A host destination must not contain spaces."
        );
        assert_eq!(
            parse("local").expect_err("reserved"),
            "invalid `host-local`: host name `local` is reserved"
        );
        assert_eq!(
            parse("fabrico@desktop").expect_err("duplicate"),
            "A host named `desktop` already exists."
        );
        assert!(parse("fabrico@").is_err());
        assert!(parse("desktop:0").is_err());
    }
}
