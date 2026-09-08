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
