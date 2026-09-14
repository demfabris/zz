use crate::agent_completion::AgentCommand;
use agent_client_protocol_schema::v1::{
    AvailableCommand, AvailableCommandInput, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory,
};
use std::path::Path;
use zz_protocol::{MAX_AGENT_CONFIG_CHOICES, MAX_AGENT_CONFIG_OPTIONS};
use zz_protocol::{
    MAX_AGENT_SESSION_DIRECTORIES, MAX_GUI_TEXT_BYTES, agent_stream::AgentSessionSummary,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum AgentConfigCategory {
    Mode,
    Model,
    ModelConfig,
    ThoughtLevel,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AgentConfigChoice {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AgentConfigOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: AgentConfigCategory,
    pub current_value: String,
    pub choices: Vec<AgentConfigChoice>,
}

#[derive(Clone, Debug, Default)]
pub struct AgentCatalogCache {
    results: Vec<zz_protocol::agent_stream::AgentCatalogResult>,
    next_request: u64,
}

impl AgentCatalogCache {
    pub fn results(&self) -> &[zz_protocol::agent_stream::AgentCatalogResult] {
        &self.results
    }

    pub fn request(
        &mut self,
        provider: zz_protocol::AgentProvider,
        cwd: std::path::PathBuf,
    ) -> Option<u64> {
        if self.results.iter().any(|result| {
            result.catalog_provider == provider
                && (result.cwd == cwd || cwd.as_os_str().is_empty())
                && result.error.is_none()
        }) {
            return None;
        }
        self.next_request = self.next_request.wrapping_add(1);
        self.results
            .retain(|result| result.catalog_provider != provider);
        self.results
            .push(zz_protocol::agent_stream::AgentCatalogResult {
                catalog_provider: provider,
                cwd,
                request_id: self.next_request,
                config_options: None,
                error: None,
            });
        Some(self.next_request)
    }

    pub fn receive(&mut self, result: zz_protocol::agent_stream::AgentCatalogResult) -> bool {
        let Some(pending) = self.results.iter_mut().find(|pending| {
            pending.catalog_provider == result.catalog_provider
                && (pending.cwd == result.cwd || pending.cwd.as_os_str().is_empty())
                && pending.request_id == result.request_id
        }) else {
            return false;
        };
        *pending = result;
        true
    }

    pub fn timeout(&mut self, request_id: u64) -> bool {
        let Some(pending) = self.results.iter_mut().find(|pending| {
            pending.request_id == request_id
                && pending.config_options.is_none()
                && pending.error.is_none()
        }) else {
            return false;
        };
        pending.error = Some("Timed out loading models.".into());
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSettingsSelection {
    pub provider: zz_protocol::AgentProvider,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AgentSettingsApply {
    pub selection: AgentSettingsSelection,
    awaiting: Option<(String, String)>,
}

impl AgentSettingsApply {
    pub fn new(selection: AgentSettingsSelection) -> Self {
        Self {
            selection,
            awaiting: None,
        }
    }

    pub fn awaiting_setting(&self) -> Option<(&str, &str)> {
        self.awaiting
            .as_ref()
            .map(|(id, value)| (id.as_str(), value.as_str()))
    }

    pub fn next_setting(
        &mut self,
        options: &[AgentConfigOption],
    ) -> Result<Option<(String, String)>, String> {
        if let Some((id, value)) = self.awaiting.take()
            && !options
                .iter()
                .any(|option| option.id == id && option.current_value == value)
        {
            return Err("The agent did not apply the selected setting.".into());
        }
        for (category, requested) in [
            (AgentConfigCategory::Model, &mut self.selection.model),
            (
                AgentConfigCategory::ThoughtLevel,
                &mut self.selection.effort,
            ),
        ] {
            let Some(value) = requested.take() else {
                continue;
            };
            let option = options.iter().find(|option| option.category == category);
            let Some(option) =
                option.filter(|option| option.choices.iter().any(|choice| choice.value == value))
            else {
                return Err(match category {
                    AgentConfigCategory::Model => "This agent no longer offers the selected model.",
                    _ => "This model no longer offers the selected effort.",
                }
                .into());
            };
            if option.current_value != value {
                let setting = (option.id.clone(), value);
                self.awaiting = Some(setting.clone());
                return Ok(Some(setting));
            }
        }
        Ok(None)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct AgentMode {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

pub fn agent_command_model(command: AvailableCommand) -> AgentCommand {
    let input_hint = command.input.and_then(|input| match input {
        AvailableCommandInput::Unstructured(input) => Some(input.hint),
        _ => None,
    });
    AgentCommand {
        name: command.name,
        description: command.description,
        input_hint,
    }
}

pub fn config_option_models(options: Vec<SessionConfigOption>) -> Vec<AgentConfigOption> {
    options
        .into_iter()
        .take(MAX_AGENT_CONFIG_OPTIONS)
        .filter_map(config_option_model)
        .collect()
}

fn config_option_model(option: SessionConfigOption) -> Option<AgentConfigOption> {
    let SessionConfigKind::Select(select) = option.kind else {
        return None;
    };
    let choices = match select.options {
        agent_client_protocol_schema::v1::SessionConfigSelectOptions::Ungrouped(options) => options,
        agent_client_protocol_schema::v1::SessionConfigSelectOptions::Grouped(groups) => {
            groups.into_iter().flat_map(|group| group.options).collect()
        }
        _ => Vec::new(),
    }
    .into_iter()
    .take(MAX_AGENT_CONFIG_CHOICES)
    .map(|choice| AgentConfigChoice {
        value: choice.value.0.to_string(),
        name: choice.name,
        description: choice.description,
    })
    .collect();
    let category = match option.category {
        Some(SessionConfigOptionCategory::Mode) => AgentConfigCategory::Mode,
        Some(SessionConfigOptionCategory::Model) => AgentConfigCategory::Model,
        Some(SessionConfigOptionCategory::ModelConfig) => AgentConfigCategory::ModelConfig,
        Some(SessionConfigOptionCategory::ThoughtLevel) => AgentConfigCategory::ThoughtLevel,
        _ => AgentConfigCategory::Other,
    };
    Some(AgentConfigOption {
        id: option.id.0.to_string(),
        name: option.name,
        description: option.description,
        category,
        current_value: select.current_value.0.to_string(),
        choices,
    })
}

pub const MAX_SESSION_ID_BYTES: usize = 16 * 1024;
const MAX_SESSION_TITLE_BYTES: usize = 4 * 1024;
const MAX_SESSION_TIMESTAMP_BYTES: usize = 256;
pub const MAX_SESSION_CURSOR_BYTES: usize = 16 * 1024;

pub fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_BYTES
        && !session_id.chars().any(char::is_control)
}

pub fn valid_session_cursor(cursor: &str) -> bool {
    !cursor.is_empty()
        && cursor.len() <= MAX_SESSION_CURSOR_BYTES
        && !cursor.chars().any(char::is_control)
}

pub fn valid_session_summary(session: &AgentSessionSummary) -> bool {
    valid_session_id(&session.session_id)
        && valid_session_directory(&session.cwd)
        && session.additional_directories.len() <= MAX_AGENT_SESSION_DIRECTORIES
        && session
            .additional_directories
            .iter()
            .all(|directory| valid_session_directory(directory))
        && session.title.as_deref().is_none_or(|title| {
            title.len() <= MAX_SESSION_TITLE_BYTES && !title.chars().any(char::is_control)
        })
        && session.updated_at.as_deref().is_none_or(|timestamp| {
            timestamp.len() <= MAX_SESSION_TIMESTAMP_BYTES
                && !timestamp.chars().any(char::is_control)
        })
}

pub fn valid_session_directory(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.as_os_str().as_encoded_bytes().len() <= MAX_GUI_TEXT_BYTES
}

pub const MAX_RENDERED_ERROR_BYTES: usize = 1024;

pub fn rendered_error(error: &str) -> String {
    let mut rendered = String::with_capacity(MAX_RENDERED_ERROR_BYTES);
    let mut truncated = false;
    for character in error.trim().chars() {
        let character = if character.is_control() && !matches!(character, '\n' | '\t') {
            '�'
        } else {
            character
        };
        if rendered.len().saturating_add(character.len_utf8()) > MAX_RENDERED_ERROR_BYTES - 3 {
            truncated = true;
            break;
        }
        rendered.push(character);
    }
    if truncated {
        rendered.push('…');
    }
    rendered
}

#[cfg(test)]
mod settings_apply_tests {
    use super::*;

    fn option(
        id: &str,
        category: AgentConfigCategory,
        current: &str,
        choices: &[&str],
    ) -> AgentConfigOption {
        AgentConfigOption {
            id: id.into(),
            name: id.into(),
            description: None,
            category,
            current_value: current.into(),
            choices: choices
                .iter()
                .map(|value| AgentConfigChoice {
                    value: (*value).into(),
                    name: (*value).into(),
                    description: None,
                })
                .collect(),
        }
    }

    fn selection() -> AgentSettingsSelection {
        AgentSettingsSelection {
            provider: zz_protocol::AgentProvider::Codex,
            model: Some("large".into()),
            effort: Some("high".into()),
        }
    }

    #[test]
    fn catalog_cache_deduplicates_requests_and_rejects_stale_scope_replies() {
        let mut cache = AgentCatalogCache::default();
        let provider = zz_protocol::AgentProvider::ClaudeCode;
        let first = cache.request(provider, "/first".into()).unwrap();
        assert!(cache.request(provider, "/first".into()).is_none());
        let second = cache.request(provider, "/second".into()).unwrap();
        assert!(
            !cache.receive(zz_protocol::agent_stream::AgentCatalogResult {
                catalog_provider: provider,
                cwd: "/first".into(),
                request_id: first,
                config_options: Some(serde_json::json!([])),
                error: None,
            })
        );
        assert!(cache.timeout(second));
        assert!(cache.request(provider, "/second".into()).is_some());
    }

    #[test]
    fn model_acknowledgement_resolves_effort_using_the_refreshed_option_id() {
        let mut apply = AgentSettingsApply::new(selection());
        let model = option(
            "model",
            AgentConfigCategory::Model,
            "small",
            &["small", "large"],
        );
        assert_eq!(
            apply.next_setting(&[model]).unwrap(),
            Some(("model".into(), "large".into()))
        );
        let model = option(
            "model",
            AgentConfigCategory::Model,
            "large",
            &["small", "large"],
        );
        let effort = option(
            "new-effort-id",
            AgentConfigCategory::ThoughtLevel,
            "low",
            &["low", "high"],
        );
        assert_eq!(
            apply.next_setting(&[model.clone(), effort]).unwrap(),
            Some(("new-effort-id".into(), "high".into()))
        );
        let effort = option(
            "new-effort-id",
            AgentConfigCategory::ThoughtLevel,
            "high",
            &["low", "high"],
        );
        assert_eq!(apply.next_setting(&[model, effort]).unwrap(), None);
    }

    #[test]
    fn unapplied_model_and_stale_cached_effort_stop_the_sequence() {
        let mut apply = AgentSettingsApply::new(selection());
        let model = option(
            "model",
            AgentConfigCategory::Model,
            "small",
            &["small", "large"],
        );
        apply.next_setting(std::slice::from_ref(&model)).unwrap();
        assert!(apply.next_setting(&[model]).is_err());
        let mut apply = AgentSettingsApply::new(selection());
        let model = option("model", AgentConfigCategory::Model, "large", &["large"]);
        let effort = option("effort", AgentConfigCategory::ThoughtLevel, "low", &["low"]);
        assert!(apply.next_setting(&[model, effort]).is_err());
    }
}
