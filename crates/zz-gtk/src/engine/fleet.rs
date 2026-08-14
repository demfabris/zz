//! The fleet: the local daemon, plus one connection per configured
//! `host-<name>`.
//!
//! Every host owns a [`Link`] and a reader thread of its own, so a machine that
//! goes away takes nothing with it — its frozen frames stay in its own core, its
//! ladder runs on its own thread, and the local daemon never sees any of it.
//! Exactly one host is *active* at a time: the one whose session the workspace
//! is rendering, which is what every display-facing [`Engine`](super::Engine)
//! accessor answers about.

use std::{path::PathBuf, sync::Arc};

use async_channel::Sender;
use zz_daemon::{AskpassPrompt, AskpassReply, Endpoint, HostEntry, SshPrompts};
use zz_protocol::{MuxSnapshot, SessionId};

use super::Link;

/// Which daemon a connection, a tree row, or an event belongs to. An id is
/// handed out once per host name and never reused, so a menu target built
/// before a config reload still names the host it was built for rather than
/// whatever moved into its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostId(pub u32);

impl HostId {
    pub const LOCAL: Self = Self(0);
}

/// Where a host's connection stands. The desktop's `HostState`, minus the
/// protocol-mismatch split it can only make by downcasting the daemon's error
/// type; an incompatible daemon lands in [`Self::Unreachable`] with the same
/// sentence the daemon wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HostState {
    #[default]
    Connecting,
    Connected,
    Reconnecting {
        attempt: u32,
    },
    /// The ladder ran out. Nothing retries until the host is reconnected by
    /// hand, which is what the row's Reconnect item is for.
    Unreachable {
        reason: String,
    },
    /// ssh asked a question and the answer was dismissed. Parked rather than
    /// retried: dialling again would only re-open the same dialog.
    Parked {
        reason: String,
    },
}

impl HostState {
    /// The short state a host row shows after its name, or `None` for the state
    /// with nothing to say.
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Connected => None,
            Self::Connecting => Some("connecting…".to_owned()),
            Self::Reconnecting { attempt } => Some(format!("retrying ({attempt})")),
            Self::Unreachable { .. } => Some("unreachable".to_owned()),
            Self::Parked { .. } => Some("signed out".to_owned()),
        }
    }

    /// What to tell someone looking at a host that is not connected.
    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::Unreachable { reason } | Self::Parked { reason } => Some(reason),
            Self::Connecting | Self::Connected | Self::Reconnecting { .. } => None,
        }
    }

    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// One host, flattened for the surfaces that draw it. The snapshot is behind an
/// `Arc`, so listing the whole fleet costs refcount bumps rather than trees.
pub struct HostView {
    pub id: HostId,
    pub name: String,
    pub state: HostState,
    pub snapshot: Arc<MuxSnapshot>,
    pub attached: Option<SessionId>,
}

/// A question ssh asked while dialling `host`, waiting on an answer. The
/// connect thread is blocked until `reply` carries one; dropping it cancels.
pub struct SshPromptRequest {
    pub host: HostId,
    /// The ssh destination, so two hosts dialling at once stay apart.
    pub label: String,
    pub prompt: AskpassPrompt,
    pub reply: Sender<AskpassReply>,
}

pub const AUTH_DECLINED_REASON: &str =
    "Authentication was cancelled. Pick Reconnect when you are ready to sign in again.";

pub struct Host {
    pub id: HostId,
    pub name: String,
    pub endpoint: Endpoint,
    pub link: Arc<Link>,
}

/// The registry: the local daemon at index zero, every configured host after
/// it in the file's own order.
pub struct Fleet {
    hosts: Vec<Host>,
    active: HostId,
    next_id: u32,
}

impl Fleet {
    pub fn new(local: Arc<Link>, endpoint: Endpoint) -> Self {
        Self {
            hosts: vec![Host {
                id: HostId::LOCAL,
                name: "local".to_owned(),
                endpoint,
                link: local,
            }],
            active: HostId::LOCAL,
            next_id: 1,
        }
    }

    pub const fn active(&self) -> HostId {
        self.active
    }

    pub fn set_active(&mut self, host: HostId) {
        self.active = host;
        for entry in &self.hosts {
            entry.link.set_active(entry.id == host);
        }
    }

    pub fn get(&self, host: HostId) -> Option<&Host> {
        self.hosts.iter().find(|entry| entry.id == host)
    }

    pub fn link(&self, host: HostId) -> Option<Arc<Link>> {
        self.get(host).map(|entry| Arc::clone(&entry.link))
    }

    /// The active host's link, falling back to the local one. The fallback is
    /// reachable only between a host being removed and the active id moving,
    /// and it is deliberately the local daemon rather than another host.
    pub fn active_link(&self) -> Arc<Link> {
        self.link(self.active)
            .unwrap_or_else(|| Arc::clone(&self.hosts[0].link))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Host> {
        self.hosts.iter()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.hosts.iter().any(|entry| entry.name == name)
    }

    /// Claim the next id. Ids are never reused, so a menu target built for a
    /// host that has since been closed resolves to nothing rather than to
    /// whichever host took its place in the file.
    pub fn reserve(&mut self) -> HostId {
        let id = HostId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub fn push(&mut self, id: HostId, name: String, endpoint: Endpoint, link: Arc<Link>) {
        self.hosts.push(Host {
            id,
            name,
            endpoint,
            link,
        });
    }

    /// Drop a host from the registry, handing back its link so the caller can
    /// close it. The local daemon is not removable.
    pub fn remove(&mut self, host: HostId) -> Option<Host> {
        if host == HostId::LOCAL {
            return None;
        }
        let index = self.hosts.iter().position(|entry| entry.id == host)?;
        let removed = self.hosts.remove(index);
        if self.active == host {
            self.active = HostId::LOCAL;
            self.hosts[0].link.set_active(true);
        }
        Some(removed)
    }

    /// Hosts the file no longer lists, or whose destination it changed. Both
    /// are closed and, for a changed destination, dialled again from scratch:
    /// a live connection cannot be repointed.
    pub fn stale(&self, configured: &[HostEntry]) -> Vec<HostId> {
        self.hosts
            .iter()
            .skip(1)
            .filter(|entry| {
                !configured
                    .iter()
                    .any(|host| host.name == entry.name && host.endpoint == entry.endpoint)
            })
            .map(|entry| entry.id)
            .collect()
    }
}

/// Where ssh's questions go while a host is dialling. The connect thread blocks
/// inside `connect_endpoint_with_prompts` until the main loop answers, which is
/// exactly the shape `SshPrompts` is built for.
pub struct PromptRoute {
    host: HostId,
    label: String,
    helper: PathBuf,
    requests: Sender<SshPromptRequest>,
}

impl PromptRoute {
    /// `None` when this endpoint never involves ssh, or when the executable
    /// cannot name itself — ssh needs a real path to run as `SSH_ASKPASS`.
    pub fn new(
        host: HostId,
        endpoint: &Endpoint,
        requests: Sender<SshPromptRequest>,
    ) -> Option<Self> {
        let Endpoint::Ssh(ssh) = endpoint else {
            return None;
        };
        Some(Self {
            host,
            label: ssh_destination_label(ssh),
            helper: std::env::current_exe().ok()?,
            requests,
        })
    }

    /// A fresh responder for one dial. It runs on the connect thread and blocks
    /// there until the main loop answers — which is the whole point: ssh is
    /// waiting on the other end of that reply. Declining parks the host, but
    /// the surface that showed the dialog is what says so, because only it
    /// knows that a host-key `no` is a decline rather than an answer.
    pub fn prompts(&self) -> SshPrompts {
        let host = self.host;
        let label = self.label.clone();
        let requests = self.requests.clone();
        SshPrompts::new(self.helper.clone(), move |prompt: &AskpassPrompt| {
            let (reply, answers) = async_channel::bounded(1);
            let request = SshPromptRequest {
                host,
                label: label.clone(),
                prompt: prompt.clone(),
                reply,
            };
            if requests.send_blocking(request).is_err() {
                return AskpassReply::Cancel;
            }
            answers.recv_blocking().unwrap_or(AskpassReply::Cancel)
        })
    }
}

fn ssh_destination_label(endpoint: &zz_daemon::SshEndpoint) -> String {
    let mut label = String::new();
    if let Some(user) = &endpoint.user {
        label.push_str(user);
        label.push('@');
    }
    label.push_str(&endpoint.host);
    if let Some(port) = endpoint.port {
        label.push(':');
        label.push_str(&port.to_string());
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_stopped_host_has_something_to_say() {
        assert_eq!(HostState::Connected.detail(), None);
        assert_eq!(
            HostState::Reconnecting { attempt: 3 }.detail().as_deref(),
            Some("retrying (3)")
        );
        assert_eq!(
            HostState::Unreachable {
                reason: "no route to host".to_owned()
            }
            .hint(),
            Some("no route to host")
        );
        assert_eq!(HostState::Connecting.hint(), None);
    }

    #[test]
    fn a_destination_label_keeps_the_user_and_the_port() {
        let endpoint = zz_daemon::SshEndpoint {
            user: Some("fabrico".to_owned()),
            host: "desktop".to_owned(),
            port: Some(2222),
            remote_socket: None,
        };

        assert_eq!(ssh_destination_label(&endpoint), "fabrico@desktop:2222");
        assert_eq!(
            ssh_destination_label(&zz_daemon::SshEndpoint {
                user: None,
                port: None,
                ..endpoint
            }),
            "desktop"
        );
    }
}
