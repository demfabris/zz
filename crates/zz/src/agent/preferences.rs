use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use zz_protocol::AgentProvider;

use crate::{
    config::atomic_write,
    user_data::{platform_data_dir, restrict_directory_to_current_user, restrict_to_current_user},
};

const PREFERENCES_FILE_NAME: &str = "agent-preferences.json";
const PREFERENCES_VERSION: u8 = 2;
const MAX_PREFERENCES_BYTES: u64 = 128 * 1024;
const MAX_PREFERENCES: usize = 128;
const MAX_AGENT_KEY_BYTES: usize = 256;
const MAX_OPTION_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AgentPreferenceKind {
    Model,
    Effort,
    Permission,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredAgentPreference {
    provider: AgentProvider,
    agent_key: String,
    kind: AgentPreferenceKind,
    option_id: String,
    value: String,
    revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredAgentPreferences {
    version: u8,
    revision: u64,
    entries: Vec<StoredAgentPreference>,
}

impl Default for StoredAgentPreferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            revision: 0,
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentPreferences {
    path: Option<PathBuf>,
    stored: StoredAgentPreferences,
}

impl AgentPreferences {
    pub fn load_persistent() -> Self {
        let path = match preferences_path() {
            Ok(path) => path,
            Err(error) => {
                log::warn!(target: "zz::agent::preferences", "agent preferences are not persisted: {error}");
                return Self::default();
            }
        };
        match Self::load_at(&path) {
            Ok(stored) => Self {
                path: Some(path),
                stored,
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self {
                path: Some(path),
                stored: StoredAgentPreferences::default(),
            },
            Err(error) => {
                log::warn!(
                    target: "zz::agent::preferences",
                    "could not load agent preferences path={} error={error}",
                    path.display(),
                );
                Self {
                    path: Some(path),
                    stored: StoredAgentPreferences::default(),
                }
            }
        }
    }

    pub(crate) fn desired(
        &self,
        provider: AgentProvider,
        agent_key: &str,
        kind: AgentPreferenceKind,
        option_id: &str,
    ) -> Option<&str> {
        self.stored
            .entries
            .iter()
            .rev()
            .find(|entry| {
                entry.provider == provider
                    && entry.agent_key == agent_key
                    && entry.kind == kind
                    && entry.option_id == option_id
            })
            .map(|entry| entry.value.as_str())
    }

    pub(crate) fn remember(
        &mut self,
        provider: AgentProvider,
        agent_key: &str,
        kind: AgentPreferenceKind,
        option_id: &str,
        value: &str,
    ) {
        if !valid_label(agent_key, MAX_AGENT_KEY_BYTES)
            || !valid_label(option_id, MAX_OPTION_BYTES)
            || !valid_label(value, MAX_OPTION_BYTES)
        {
            log::warn!(target: "zz::agent::preferences", "refusing to persist invalid agent preference");
            return;
        }
        self.stored.revision = self.stored.revision.saturating_add(1);
        self.stored.entries.retain(|entry| {
            !(entry.provider == provider
                && entry.agent_key == agent_key
                && entry.kind == kind
                && entry.option_id == option_id)
        });
        self.stored.entries.push(StoredAgentPreference {
            provider,
            agent_key: agent_key.to_owned(),
            kind,
            option_id: option_id.to_owned(),
            value: value.to_owned(),
            revision: self.stored.revision,
        });
        if self.stored.entries.len() > MAX_PREFERENCES {
            let excess = self.stored.entries.len() - MAX_PREFERENCES;
            self.stored.entries.drain(..excess);
        }
        if let Some(path) = &self.path
            && let Err(error) = self.save_at(path)
        {
            log::warn!(
                target: "zz::agent::preferences",
                "could not persist agent preferences path={} error={error}",
                path.display(),
            );
        }
    }

    fn load_at(path: &Path) -> io::Result<StoredAgentPreferences> {
        let metadata = fs::metadata(path)?;
        if metadata.len() > MAX_PREFERENCES_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "agent preferences file is too large",
            ));
        }
        let bytes = fs::read(path)?;
        let mut stored: StoredAgentPreferences = serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if stored.version != PREFERENCES_VERSION {
            return Ok(StoredAgentPreferences::default());
        }
        stored.entries.retain(valid_stored_preference);
        stored.entries.sort_by_key(|entry| entry.revision);
        if stored.entries.len() > MAX_PREFERENCES {
            let excess = stored.entries.len() - MAX_PREFERENCES;
            stored.entries.drain(..excess);
        }
        Ok(stored)
    }

    fn save_at(&self, path: &Path) -> io::Result<()> {
        let contents = serde_json::to_vec(&self.stored)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if u64::try_from(contents.len()).unwrap_or(u64::MAX) > MAX_PREFERENCES_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "agent preferences exceed their size limit",
            ));
        }
        prepare_parent_directory(path)?;
        atomic_write(path, &contents)?;
        restrict_to_current_user(path)
    }
}

fn prepare_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "agent preferences path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    restrict_directory_to_current_user(parent)
}

fn valid_stored_preference(entry: &StoredAgentPreference) -> bool {
    valid_label(&entry.agent_key, MAX_AGENT_KEY_BYTES)
        && valid_label(&entry.option_id, MAX_OPTION_BYTES)
        && valid_label(&entry.value, MAX_OPTION_BYTES)
}

fn valid_label(value: &str, max_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

fn preferences_path() -> io::Result<PathBuf> {
    let data = platform_data_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve the current user's application-data directory",
        )
    })?;
    Ok(data.join("zz").join(PREFERENCES_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn preferences_round_trip_and_keep_provider_scope() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join(PREFERENCES_FILE_NAME);
        let mut preferences = AgentPreferences {
            path: Some(path.clone()),
            stored: StoredAgentPreferences::default(),
        };

        preferences.remember(
            AgentProvider::Codex,
            "codex-acp",
            AgentPreferenceKind::Model,
            "model",
            "gpt-test",
        );

        let loaded = AgentPreferences {
            path: Some(path.clone()),
            stored: AgentPreferences::load_at(&path).expect("load preferences"),
        };
        assert_eq!(
            loaded.desired(
                AgentProvider::Codex,
                "codex-acp",
                AgentPreferenceKind::Model,
                "model"
            ),
            Some("gpt-test")
        );
        assert_eq!(
            loaded.desired(
                AgentProvider::ClaudeCode,
                "codex-acp",
                AgentPreferenceKind::Model,
                "model"
            ),
            None
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(directory.path())
                    .expect("directory metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn malformed_or_oversized_values_are_not_saved() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join(PREFERENCES_FILE_NAME);
        let mut preferences = AgentPreferences {
            path: Some(path),
            stored: StoredAgentPreferences::default(),
        };

        preferences.remember(
            AgentProvider::Codex,
            "codex-acp",
            AgentPreferenceKind::Permission,
            "mode",
            "bad\nmode",
        );

        assert!(preferences.stored.entries.is_empty());
    }
}
