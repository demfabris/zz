use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use zz_daemon::Endpoint;

use crate::{config::HostEntry, profile::LocalHostPolicy};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HostId(u32);

impl HostId {
    pub const LOCAL: Self = Self(0);

    fn from_index(index: usize) -> Self {
        Self(u32::try_from(index).expect("host registry exceeds u32 host ids"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostState {
    Disconnected,
    Connecting,
    Reconnecting { attempt: u32 },
    Connected,
    Unreachable { reason: String },
    Incompatible { local: u16, remote: u16 },
}

impl HostState {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Reconnecting { .. } => "reconnecting",
            Self::Connected => "connected",
            Self::Unreachable { .. } => "unreachable",
            Self::Incompatible { .. } => "incompatible",
        }
    }

    /// What to tell someone looking at a failed host row, or `None` for the
    /// states with nothing to explain.
    pub fn failure_detail(&self) -> Option<String> {
        match self {
            Self::Unreachable { reason } => Some(reason.clone()),
            Self::Incompatible { local, remote } => Some(format!(
                "This zz speaks protocol v{local}; that machine speaks v{remote}.\nUpgrade \
                 whichever side is older, then reconnect."
            )),
            Self::Disconnected | Self::Connecting | Self::Reconnecting { .. } | Self::Connected => {
                None
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct HostRegistry {
    entries: Vec<HostEntry>,
    configured_count: usize,
    ids_by_name: HashMap<String, HostId>,
    has_local: bool,
    local_socket_path: PathBuf,
}

impl HostRegistry {
    pub(crate) fn new(
        local_socket_path: PathBuf,
        configured: &[HostEntry],
        policy: LocalHostPolicy,
    ) -> Self {
        Self::with_local(local_socket_path, configured, policy.synthesize_local())
    }

    fn with_local(local_socket_path: PathBuf, configured: &[HostEntry], has_local: bool) -> Self {
        let mut entries = Vec::with_capacity(configured.len() + usize::from(has_local));
        if has_local {
            entries.push(HostEntry {
                name: "local".to_owned(),
                endpoint: Endpoint::Local(local_socket_path.clone()),
            });
        }
        entries.extend_from_slice(configured);

        let first_id = usize::from(!has_local);
        let ids_by_name = entries
            .iter()
            .enumerate()
            .map(|(index, host)| (host.name.clone(), HostId::from_index(index + first_id)))
            .collect();
        Self {
            entries,
            configured_count: configured.len(),
            ids_by_name,
            has_local,
            local_socket_path,
        }
    }

    pub(crate) fn configured(&self) -> &[HostEntry] {
        let start = usize::from(self.has_local);
        &self.entries[start..start + self.configured_count]
    }

    pub(crate) fn local_socket_path(&self) -> &Path {
        &self.local_socket_path
    }

    pub(crate) fn push_retained(&mut self, host: HostEntry) -> HostId {
        debug_assert_ne!(host.name, "local");
        debug_assert!(!self.ids_by_name.contains_key(&host.name));
        let id = HostId::from_index(self.entries.len() + usize::from(!self.has_local));
        self.ids_by_name.insert(host.name.clone(), id);
        self.entries.push(host);
        id
    }

    pub(crate) fn is_retained(&self, id: HostId) -> bool {
        let index = id.0 as usize;
        index > self.configured_count && self.get(id).is_some()
    }

    pub(crate) fn get(&self, id: HostId) -> Option<&HostEntry> {
        let index = id.0 as usize;
        let index = if self.has_local {
            index
        } else {
            index.checked_sub(1)?
        };
        self.entries.get(index)
    }

    pub(crate) fn get_by_name(&self, name: &str) -> Option<(HostId, &HostEntry)> {
        let id = *self.ids_by_name.get(name)?;
        Some((id, self.get(id)?))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (HostId, &HostEntry)> {
        let first_id = usize::from(!self.has_local);
        self.entries
            .iter()
            .enumerate()
            .map(move |(index, host)| (HostId::from_index(index + first_id), host))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_keeps_local_at_zero_and_configured_hosts_in_order() {
        let configured = vec![
            HostEntry {
                name: "desktop".to_owned(),
                endpoint: Endpoint::parse("ssh://desktop").expect("desktop endpoint"),
            },
            HostEntry {
                name: "server".to_owned(),
                endpoint: Endpoint::parse("unix:///tmp/server.sock").expect("server endpoint"),
            },
        ];
        let registry = HostRegistry::new(
            PathBuf::from("/tmp/local.sock"),
            &configured,
            LocalHostPolicy::Always,
        );

        assert_eq!(
            registry.get(HostId::LOCAL),
            Some(&HostEntry {
                name: "local".to_owned(),
                endpoint: Endpoint::Local(PathBuf::from("/tmp/local.sock")),
            })
        );
        assert_eq!(
            registry.get_by_name("local").map(|(id, _)| id),
            Some(HostId::LOCAL)
        );
        assert_eq!(
            registry.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            [HostId(0), HostId(1), HostId(2)]
        );
        assert_eq!(
            registry
                .iter()
                .map(|(_, host)| host.name.as_str())
                .collect::<Vec<_>>(),
            ["local", "desktop", "server"]
        );
        assert_eq!(
            registry.get_by_name("desktop").map(|(id, _)| id),
            Some(HostId(1))
        );
        assert_eq!(
            registry.get_by_name("server").map(|(id, _)| id),
            Some(HostId(2))
        );
        assert!(registry.get_by_name("missing").is_none());
        assert!(registry.get(HostId(3)).is_none());
    }

    #[test]
    fn registry_omits_local_without_reusing_its_id() {
        let configured = vec![HostEntry {
            name: "desktop".to_owned(),
            endpoint: Endpoint::parse("ssh://desktop").expect("desktop endpoint"),
        }];
        let registry =
            HostRegistry::with_local(PathBuf::from("/tmp/local.sock"), &configured, false);

        assert!(registry.get(HostId::LOCAL).is_none());
        assert_eq!(registry.configured(), configured);
        assert_eq!(
            registry
                .iter()
                .map(|(id, host)| (id, host.name.as_str()))
                .collect::<Vec<_>>(),
            [(HostId(1), "desktop")]
        );
        assert_eq!(registry.local_socket_path(), Path::new("/tmp/local.sock"));
    }

    #[test]
    fn only_failed_states_carry_a_detail_and_it_survives_the_typed_reason() {
        assert_eq!(
            HostState::Unreachable {
                reason: "zz is not installed on ssh://desk.\nInstall it there.".to_owned()
            }
            .failure_detail()
            .as_deref(),
            Some("zz is not installed on ssh://desk.\nInstall it there.")
        );
        assert_eq!(
            HostState::Incompatible {
                local: 33,
                remote: 31,
            }
            .failure_detail()
            .as_deref(),
            Some(
                "This zz speaks protocol v33; that machine speaks v31.\nUpgrade whichever side is \
                 older, then reconnect."
            )
        );
        assert!(HostState::Disconnected.failure_detail().is_none());
        assert!(HostState::Connecting.failure_detail().is_none());
        assert!(
            HostState::Reconnecting { attempt: 2 }
                .failure_detail()
                .is_none()
        );
        assert!(HostState::Connected.failure_detail().is_none());
    }
}
