use zz_gpui::{AnyElement, Context, IntoElement, div, prelude::*};
#[cfg(target_os = "ios")]
use zz_gpui::{Entity, SharedString, Subscription, Window, px};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _,
    button::Button,
    h_flex, rems_from_px,
    settings::{SettingEntry, SettingsStack, settings_scroll_column},
};
#[cfg(target_os = "ios")]
use zz_ui::{
    button::ButtonVariants as _,
    input::{Input, InputEvent, InputState},
    settings::settings_control_fill,
};

use super::{
    AppShell,
    settings::{settings_heading, with_control},
};
use crate::connection::Connection;

#[cfg(target_os = "ios")]
const DESCRIPTION: &str = "Pick a machine running zz. The app signs in over ssh, so the machine needs Remote Login and zz on its PATH.";
#[cfg(not(target_os = "ios"))]
const DESCRIPTION: &str =
    "The browser client reaches the daemon through the zz web gateway on this computer.";

#[cfg(any(target_os = "ios", test))]
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct SavedHosts {
    endpoints: Vec<String>,
}

#[cfg(any(target_os = "ios", test))]
impl SavedHosts {
    pub(super) fn endpoints(&self) -> &[String] {
        &self.endpoints
    }

    pub(super) fn most_recent(&self) -> Option<&str> {
        self.endpoints.first().map(String::as_str)
    }

    pub(super) fn touch(&mut self, endpoint: &str) {
        self.endpoints.retain(|saved| saved != endpoint);
        self.endpoints.insert(0, endpoint.to_owned());
    }

    pub(super) fn remove(&mut self, endpoint: &str) {
        self.endpoints.retain(|saved| saved != endpoint);
    }

    fn load_from(directory: &std::path::Path) -> Self {
        if let Ok(value) = std::fs::read_to_string(directory.join("hosts.json")) {
            return serde_json::from_str(&value).unwrap_or_default();
        }
        let mut hosts = Self::default();
        if let Ok(endpoint) = std::fs::read_to_string(directory.join("endpoint"))
            && !endpoint.trim().is_empty()
        {
            hosts.touch(endpoint.trim());
        }
        hosts
    }

    fn save_to(&self, directory: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(directory)?;
        let value = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let temporary = directory.join("hosts.json.tmp");
        std::fs::write(&temporary, value)?;
        std::fs::rename(temporary, directory.join("hosts.json"))
    }
}

#[cfg(target_os = "ios")]
impl SavedHosts {
    fn directory() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var_os("HOME").expect("iOS application home directory"))
            .join("Library/Application Support/zz-gpui")
    }

    pub(super) fn load() -> Self {
        Self::load_from(&Self::directory())
    }

    pub(super) fn save(&self) {
        if let Err(error) = self.save_to(&Self::directory()) {
            log::warn!("Could not save hosts: {error}");
        }
    }
}

#[cfg(any(target_os = "ios", test))]
pub(super) fn destination_endpoint(input: &str) -> Result<String, String> {
    let destination = input.trim();
    if destination.is_empty() {
        return Err("Enter a host, like user@my-mac.local.".into());
    }
    if destination.chars().any(char::is_whitespace) {
        return Err("A host must not contain spaces.".into());
    }
    Ok(
        if destination.contains("://") || destination.starts_with('/') {
            destination.to_owned()
        } else {
            format!("ssh://{destination}")
        },
    )
}

#[cfg(any(target_os = "ios", test))]
pub(super) fn host_title(endpoint: &str) -> String {
    let Some(rest) = endpoint.strip_prefix("ssh://") else {
        return "Local socket".into();
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    }
    .to_owned()
}

#[cfg(target_os = "ios")]
pub(super) struct Hosts {
    saved: SavedHosts,
    destination: Entity<InputState>,
    error: Option<SharedString>,
    _destination_events: Subscription,
}

#[cfg(target_os = "ios")]
impl Hosts {
    pub(super) fn new(window: &mut Window, cx: &mut Context<AppShell>) -> Self {
        let destination = cx.new(|cx| InputState::new(window, cx).placeholder("user@my-mac.local"));
        let destination_events = cx.subscribe_in(
            &destination,
            window,
            |this: &mut AppShell, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_host(window, cx);
                }
            },
        );
        Self {
            saved: SavedHosts::load(),
            destination,
            error: None,
            _destination_events: destination_events,
        }
    }
}

impl AppShell {
    pub(super) fn hosts_page(
        &self,
        id: &'static str,
        title: Option<&'static str>,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entries = self.host_entries(narrow, cx);
        let page = settings_scroll_column(id)
            .children(settings_heading(title, DESCRIPTION, cx))
            .when(!entries.is_empty(), |page| {
                page.child(SettingsStack::titled("Hosts").children(entries))
            });
        #[cfg(target_os = "ios")]
        let page = page.child(self.add_host_stack(narrow, cx));
        page.into_any_element()
    }

    #[cfg(target_os = "ios")]
    pub(super) fn remember_host(&mut self, cx: &mut Context<Self>) {
        if let Some(endpoint) = self.connection.read(cx).endpoint() {
            let endpoint = endpoint.to_owned();
            self.hosts.saved.touch(&endpoint);
            self.hosts.saved.save();
        }
    }

    #[cfg(target_os = "ios")]
    pub(super) fn connect_recent_host(&mut self, cx: &mut Context<Self>) {
        let connection = self.connection.read(cx);
        if !connection.connected
            && !connection.busy()
            && let Some(endpoint) = self.hosts.saved.most_recent()
        {
            let endpoint = endpoint.to_owned();
            self.connection
                .update(cx, |connection, cx| connection.connect_to(endpoint, cx));
        }
    }

    #[cfg(target_os = "ios")]
    fn add_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.hosts.destination.read(cx).value().to_string();
        let endpoint = destination_endpoint(&value).and_then(|endpoint| {
            zz_daemon_client::Endpoint::parse(&endpoint)
                .map(|endpoint| endpoint.to_string())
                .map_err(|error| error.to_string())
        });
        match endpoint {
            Ok(endpoint) => {
                self.hosts.error = None;
                self.hosts
                    .destination
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.hosts.saved.touch(&endpoint);
                self.hosts.saved.save();
                self.connection
                    .update(cx, |connection, cx| connection.connect_to(endpoint, cx));
                super::settings::finish_field(window, cx);
            }
            Err(error) => self.hosts.error = Some(error.into()),
        }
        cx.notify();
    }

    #[cfg(target_os = "ios")]
    fn host_entries(&self, narrow: bool, cx: &mut Context<Self>) -> Vec<SettingEntry> {
        let connection = self.connection.read(cx);
        let current = connection.endpoint().map(ToOwned::to_owned);
        let connected = connection.connected;
        let busy = connection.busy();
        let status = if connected {
            "Connected".to_owned()
        } else {
            connection.status.clone()
        };
        let muted = cx.theme().foreground.muted();
        self.hosts
            .saved
            .endpoints()
            .iter()
            .enumerate()
            .map(|(index, endpoint)| {
                let active = current.as_deref() == Some(endpoint.as_str());
                let color = if !active {
                    muted
                } else if connected {
                    cx.theme().success
                } else if busy {
                    cx.theme().warning
                } else {
                    cx.theme().danger
                };
                let icon = if endpoint.starts_with("ssh://") {
                    IconName::Globe
                } else {
                    IconName::HardDrive
                };
                let entry = SettingEntry::new(host_title(endpoint), endpoint.clone())
                    .title_icon(Icon::new(icon).text_color(color))
                    .when(active, |entry| {
                        entry.child(status_line(
                            status.clone(),
                            if connected || busy {
                                muted
                            } else {
                                cx.theme().warning
                            },
                        ))
                    });
                let action = if active && (connected || busy) {
                    Button::new(("hosts-disconnect", index))
                        .small()
                        .label(if connected { "Disconnect" } else { "Cancel" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.connection.update(cx, Connection::disconnect);
                        }))
                } else {
                    let endpoint = endpoint.clone();
                    Button::new(("hosts-connect", index))
                        .small()
                        .when(
                            index == 0 && current.is_none(),
                            zz_ui::button::ButtonVariants::accent,
                        )
                        .label("Connect")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let endpoint = endpoint.clone();
                            this.connection
                                .update(cx, |connection, cx| connection.connect_to(endpoint, cx));
                        }))
                };
                let removed = endpoint.clone();
                let remove = Button::new(("hosts-remove", index))
                    .small()
                    .ghost()
                    .icon(IconName::Xmark)
                    .tooltip("Remove host")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.hosts.saved.remove(&removed);
                        this.hosts.saved.save();
                        cx.notify();
                    }));
                with_control(
                    entry,
                    h_flex().gap_2().flex_none().child(action).child(remove),
                    narrow,
                )
            })
            .collect()
    }

    #[cfg(not(target_os = "ios"))]
    fn host_entries(&self, narrow: bool, cx: &mut Context<Self>) -> Vec<SettingEntry> {
        let connection = self.connection.read(cx);
        let connected = connection.connected;
        let muted = cx.theme().foreground.muted();
        let address = gateway_address();
        let entry = SettingEntry::new(
            "This computer",
            if address.is_empty() {
                "zz web gateway".to_owned()
            } else {
                format!("zz web gateway at {address}")
            },
        )
        .title_icon(Icon::new(IconName::HardDrive).text_color(if connected {
            cx.theme().success
        } else {
            cx.theme().warning
        }))
        .child(status_line(
            if connected {
                "Connected".to_owned()
            } else {
                connection.status.clone()
            },
            if connected { muted } else { cx.theme().warning },
        ));
        vec![with_control(
            entry,
            h_flex().child(
                Button::new("hosts-reconnect")
                    .small()
                    .label(if connected { "Reconnect" } else { "Retry" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.connection.update(cx, Connection::reconnect);
                    })),
            ),
            narrow,
        )]
    }

    #[cfg(target_os = "ios")]
    fn add_host_stack(&self, narrow: bool, cx: &mut Context<Self>) -> SettingsStack {
        let destination = SettingEntry::new(
            "Destination",
            "Enter user@host, or user@host:port for a custom ssh port.",
        )
        .when_some(self.hosts.error.clone(), |entry, error| {
            entry.child(status_line(error, cx.theme().warning))
        });
        SettingsStack::titled("Add host")
            .child(with_control(
                destination,
                h_flex()
                    .gap_2()
                    .when(narrow, zz_gpui::Styled::w_full)
                    .child(
                        div()
                            .when(narrow, |field| field.flex_1().min_w_0())
                            .when(!narrow, |field| field.w(px(220.0)).flex_none())
                            .child(
                                Input::new(&self.hosts.destination)
                                    .small()
                                    .bg(settings_control_fill(cx)),
                            ),
                    )
                    .child(
                        Button::new("hosts-add")
                            .small()
                            .when(self.hosts.saved.endpoints().is_empty(), |button| {
                                button.accent()
                            })
                            .label("Connect")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_host(window, cx)),
                            ),
                    ),
                narrow,
            ))
            .child(with_control(
                SettingEntry::new(
                    "SSH key",
                    "Add this key to ~/.ssh/authorized_keys on the host to sign in without a password.",
                ),
                h_flex().child(Button::new("hosts-copy-ssh-key").small().label("Copy").on_click(
                    |_, window, cx| {
                        use zz_ui::{WindowExt as _, notification::Notification};
                        let notification = match zz_daemon_client::ios_ssh_public_key() {
                            Ok(key) => {
                                cx.write_to_clipboard(zz_gpui::ClipboardItem::new_string(key));
                                Notification::success("SSH key copied.")
                            }
                            Err(error) => Notification::error(error.to_string()),
                        };
                        window.push_notification(notification, cx);
                    },
                )),
                narrow,
            ))
    }
}

fn status_line(text: impl Into<zz_gpui::SharedString>, color: zz_gpui::Hsla) -> impl IntoElement {
    div()
        .text_size(rems_from_px(11.0))
        .text_color(color)
        .child(text.into())
}

#[cfg(target_family = "wasm")]
fn gateway_address() -> String {
    web_sys::window()
        .and_then(|window| window.location().host().ok())
        .unwrap_or_default()
}

#[cfg(not(any(target_family = "wasm", target_os = "ios")))]
fn gateway_address() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::{SavedHosts, destination_endpoint, host_title};

    #[test]
    fn destinations_default_to_ssh_and_reject_blank_or_spaced_input() {
        assert_eq!(
            destination_endpoint("  fabrico@mac.local ").unwrap(),
            "ssh://fabrico@mac.local"
        );
        assert_eq!(
            destination_endpoint("ssh://box:2222").unwrap(),
            "ssh://box:2222"
        );
        assert_eq!(
            destination_endpoint("/tmp/zz.sock").unwrap(),
            "/tmp/zz.sock"
        );
        assert!(destination_endpoint("   ").is_err());
        assert!(destination_endpoint("my mac").is_err());
    }

    #[test]
    fn titles_name_the_host_without_user_port_or_socket() {
        assert_eq!(host_title("ssh://fabrico@mac.local"), "mac.local");
        assert_eq!(host_title("ssh://box:2222/run/zz.sock"), "box");
        assert_eq!(host_title("ssh://me@[::1]:22"), "::1");
        assert_eq!(host_title("/tmp/zz.sock"), "Local socket");
    }

    #[test]
    fn recent_hosts_come_first_and_the_old_endpoint_file_seeds_the_list() {
        let directory = std::env::temp_dir().join(format!(
            "zz-thin-hosts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        assert_eq!(SavedHosts::load_from(&directory), SavedHosts::default());
        std::fs::write(directory.join("endpoint"), "ssh://me@old\n").unwrap();
        let mut hosts = SavedHosts::load_from(&directory);
        assert_eq!(hosts.endpoints(), ["ssh://me@old"]);
        hosts.touch("ssh://me@new");
        hosts.touch("ssh://me@other");
        hosts.touch("ssh://me@new");
        assert_eq!(
            hosts.endpoints(),
            ["ssh://me@new", "ssh://me@other", "ssh://me@old"]
        );
        hosts.remove("ssh://me@other");
        hosts.save_to(&directory).unwrap();
        std::fs::write(directory.join("endpoint"), "ssh://me@ignored").unwrap();
        let restored = SavedHosts::load_from(&directory);
        assert_eq!(restored, hosts);
        assert_eq!(restored.most_recent(), Some("ssh://me@new"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
