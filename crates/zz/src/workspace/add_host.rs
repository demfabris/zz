//! The sidebar's `+ add host` dialog.

use gpui::{App, AppContext as _, Entity, Focusable as _, SharedString, Window};
use zz_daemon::Endpoint;
use zz_ui::{WindowExt as _, feedback::add_host_prompt_dialog, input::InputState};

use crate::config;

pub(crate) const PLACEHOLDER: &str = "user@desktop";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AddHostRequest {
    pub(crate) name: String,
    pub(crate) endpoint: String,
}

struct AddHostDialogState {
    error: Option<SharedString>,
}

pub fn open(window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(PLACEHOLDER));
    let state = cx.new(|_| AddHostDialogState { error: None });

    let dialog_input = input.clone();
    let dialog_state = state.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let error = dialog_state.read(cx).error.clone();
        let submit_input = dialog_input.clone();
        let submit_state = dialog_state.clone();
        add_host_prompt_dialog(dialog, &dialog_input, error, cx)
            .on_ok(move |_, window, cx| submit(&submit_input, &submit_state, window, cx))
    });
    input.read(cx).focus_handle(cx).focus(window, cx);
}

fn submit(
    input: &Entity<InputState>,
    state: &Entity<AddHostDialogState>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let value = input.read(cx).value();
    let existing = config::fleet_hosts(cx)
        .into_iter()
        .map(|host| host.name)
        .collect::<Vec<_>>();
    let request = match parse_add_host(&value, &existing) {
        Ok(request) => request,
        Err(message) => return fail(state, message, window, cx),
    };
    if let Err(error) = config::add_fleet_host(&request.name, &request.endpoint, cx) {
        return fail(
            state,
            format!("Could not write zz/config: {error}"),
            window,
            cx,
        );
    }
    log::info!(
        target: "zz::config",
        "added fleet host name={} endpoint={}",
        request.name,
        request.endpoint,
    );
    true
}

fn fail(
    state: &Entity<AddHostDialogState>,
    message: String,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    state.update(cx, |state, cx| {
        state.error = Some(message.into());
        cx.notify();
    });
    window.refresh();
    false
}

pub(crate) fn parse_add_host(input: &str, existing: &[String]) -> Result<AddHostRequest, String> {
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
        unreachable!("an ssh:// URI parses as an SSH endpoint");
    };
    let name = parsed.host;
    config::validate_fleet_host(&name, &endpoint)?;
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

    #[test]
    fn the_host_component_alone_names_the_entry() {
        assert_eq!(
            parse("  fabrico@arch-desktop  ").unwrap(),
            AddHostRequest {
                name: "arch-desktop".to_owned(),
                endpoint: "ssh://fabrico@arch-desktop".to_owned(),
            }
        );
        assert_eq!(
            parse("gpu-box:2222").unwrap(),
            AddHostRequest {
                name: "gpu-box".to_owned(),
                endpoint: "ssh://gpu-box:2222".to_owned(),
            }
        );
        assert_eq!(parse("ssh://gpu-box").unwrap().name, "gpu-box");
        assert!(
            parse("gpu-box:9922")
                .unwrap()
                .endpoint
                .starts_with("ssh://")
        );
    }

    #[test]
    fn empty_spaced_reserved_duplicate_and_malformed_entries_are_rejected() {
        assert_eq!(
            parse("   ").unwrap_err(),
            format!("Enter a host, like `{PLACEHOLDER}`.")
        );
        assert_eq!(
            parse("arch desktop").unwrap_err(),
            "A host destination must not contain spaces."
        );
        assert_eq!(
            parse("local").unwrap_err(),
            "invalid `host-local`: host name `local` is reserved"
        );
        assert_eq!(
            parse("fabrico@desktop").unwrap_err(),
            "A host named `desktop` already exists."
        );
        assert!(parse("fabrico@").is_err());
        assert!(parse("fabrico@@desktop").is_err());
        assert!(parse("desktop:0").is_err());
    }
}
