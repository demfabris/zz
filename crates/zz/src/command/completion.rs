use std::{collections::BTreeSet, ops::Range};

use zz_mux::LayoutPreset;
use zz_protocol::MuxSnapshot;
use zz_protocol::{COMMAND_SPECS, CommandSpec, CommandValueKind, command_spec};

const MAX_COMPLETIONS: usize = 64;
const KEY_TABLES: &[&str] = &[
    "prefix",
    "root",
    "copy-mode",
    "copy-mode-vi",
    "choose-tree",
    "choose-buffer",
];
const BROWSER_COMMANDS: &[&str] = &[
    "new-browser",
    "split-browser",
    "set-browser-url",
    "set-browser-tabs",
    "set-browser-profile",
];
const SET_OPTIONS: &[&str] = &[
    "buffer-limit",
    "copy-command",
    "history-limit",
    "history-trickle",
    "mode-keys",
    "prefix",
    "set-clipboard",
    "synchronize-panes",
    "word-separators",
];

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CompletionKind {
    History,
    Command,
    Option,
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompletionSuggestion {
    pub(crate) kind: CompletionKind,
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) replacement: Range<usize>,
    pub(crate) insertion: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    range: Range<usize>,
    value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    class: u8,
    length: usize,
    order: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PaneKindAvailability {
    pub browser: bool,
    pub agent: bool,
    pub editor: bool,
}

impl Default for PaneKindAvailability {
    fn default() -> Self {
        Self {
            browser: true,
            agent: false,
            editor: false,
        }
    }
}

pub(crate) fn complete_command(
    input: &str,
    cursor: usize,
    history: &[String],
    snapshot: &MuxSnapshot,
    availability: PaneKindAvailability,
) -> Vec<CompletionSuggestion> {
    let cursor = floor_char_boundary(input, cursor.min(input.len()));
    let tokens = tokenize(input);
    let segment_start = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.range.start < cursor && token.value == ";")
        .map(|(index, _)| index)
        .next_back()
        .map_or(0, |index| index + 1);
    let segment = &tokens[segment_start..];
    let (active_index, replacement, query) = active_token(segment, input, cursor);

    let mut ranked = Vec::new();
    if segment_start == 0 {
        add_history(&mut ranked, input, cursor, history, 0..input.len());
    }

    if active_index == 0 {
        add_commands(&mut ranked, &query, replacement, availability);
        return finish(ranked);
    }

    let Some(command_token) = segment.first() else {
        add_commands(&mut ranked, &query, replacement, availability);
        return finish(ranked);
    };
    let Some(spec) = command_spec(&command_token.value) else {
        let discovery_query = input[command_token.range.start..cursor].trim();
        let discovery_end = replacement.end.max(cursor);
        add_commands(
            &mut ranked,
            discovery_query,
            command_token.range.start..discovery_end,
            availability,
        );
        return finish(ranked);
    };

    let previous = &segment[1..active_index.min(segment.len())];
    let context = argument_context(spec, previous);
    if let Some(kind) = context.value_kind {
        add_values(
            &mut ranked,
            kind,
            &query,
            replacement,
            snapshot,
            spec,
            previous,
            availability,
        );
    } else if query.starts_with('-') || query.is_empty() {
        add_options(
            &mut ranked,
            spec,
            &query,
            replacement.clone(),
            &context.used_options,
        );
        if query.is_empty()
            && let Some(kind) = spec.positional_kind(context.positional_index)
        {
            add_values(
                &mut ranked,
                kind,
                &query,
                replacement,
                snapshot,
                spec,
                previous,
                availability,
            );
        }
    } else if let Some(kind) = spec.positional_kind(context.positional_index) {
        add_values(
            &mut ranked,
            kind,
            &query,
            replacement,
            snapshot,
            spec,
            previous,
            availability,
        );
    }

    finish(ranked)
}

pub(crate) fn apply_completion(input: &str, suggestion: &CompletionSuggestion) -> (String, usize) {
    let start = floor_char_boundary(input, suggestion.replacement.start.min(input.len()));
    let end = floor_char_boundary(input, suggestion.replacement.end.clamp(start, input.len()));
    let insertion = completion_insertion(input, suggestion);
    let mut completed = String::with_capacity(input.len() - (end - start) + insertion.len());
    completed.push_str(&input[..start]);
    completed.push_str(&insertion);
    let cursor = completed.len();
    completed.push_str(&input[end..]);
    (completed, cursor)
}

pub(crate) fn completion_insertion(input: &str, suggestion: &CompletionSuggestion) -> String {
    let suffix_starts_with_space = input
        .get(suggestion.replacement.end..)
        .and_then(|suffix| suffix.chars().next())
        .is_some_and(char::is_whitespace);
    if suffix_starts_with_space && suggestion.insertion.ends_with(' ') {
        suggestion.insertion.trim_end_matches(' ').to_owned()
    } else {
        suggestion.insertion.clone()
    }
}

#[derive(Default)]
struct ArgumentContext {
    used_options: BTreeSet<&'static str>,
    value_kind: Option<CommandValueKind>,
    positional_index: usize,
}

fn argument_context(spec: &'static CommandSpec, tokens: &[Token]) -> ArgumentContext {
    let mut context = ArgumentContext::default();
    let mut expecting_value = None;
    for token in tokens {
        if expecting_value.take().is_some() {
            continue;
        }
        if let Some(option) = spec.option(&token.value) {
            context.used_options.insert(option.name);
            expecting_value = option.value;
        } else if token.value.starts_with('-') {
            // Unknown options remain parser-owned and do not change positional context.
        } else {
            context.positional_index += 1;
        }
    }
    context.value_kind = expecting_value;
    context
}

fn add_history(
    ranked: &mut Vec<(Rank, CompletionSuggestion)>,
    input: &str,
    cursor: usize,
    history: &[String],
    replacement: Range<usize>,
) {
    let query = input[..cursor].trim();
    let mut seen = BTreeSet::new();
    for (order, entry) in history.iter().rev().enumerate() {
        if entry.is_empty() || !seen.insert(entry.as_str()) {
            continue;
        }
        let Some(mut rank) = text_rank(entry, query, order) else {
            continue;
        };
        rank.class = rank.class.saturating_sub(1);
        rank.length = 0;
        ranked.push((
            rank,
            CompletionSuggestion {
                kind: CompletionKind::History,
                label: entry.clone(),
                detail: "Recent command".to_owned(),
                replacement: replacement.clone(),
                insertion: entry.clone(),
            },
        ));
    }
}

fn add_commands(
    ranked: &mut Vec<(Rank, CompletionSuggestion)>,
    query: &str,
    replacement: Range<usize>,
    availability: PaneKindAvailability,
) {
    for (order, spec) in COMMAND_SPECS.iter().enumerate() {
        if !availability.browser && BROWSER_COMMANDS.contains(&spec.name) {
            continue;
        }
        let Some((rank, matched_alias)) = command_rank(spec, query, order) else {
            continue;
        };
        let detail = matched_alias.map_or_else(
            || spec.description.to_owned(),
            |alias| format!("{} · alias {alias}", spec.description),
        );
        ranked.push((
            rank,
            CompletionSuggestion {
                kind: CompletionKind::Command,
                label: spec.name.to_owned(),
                detail,
                replacement: replacement.clone(),
                insertion: format!("{} ", spec.name),
            },
        ));
    }
}

fn add_options(
    ranked: &mut Vec<(Rank, CompletionSuggestion)>,
    spec: &'static CommandSpec,
    query: &str,
    replacement: Range<usize>,
    used: &BTreeSet<&'static str>,
) {
    for (order, option) in spec.options.iter().enumerate() {
        if !option.completable || used.contains(option.name) {
            continue;
        }
        let Some(rank) = text_rank(option.name, query, order) else {
            continue;
        };
        ranked.push((
            rank,
            CompletionSuggestion {
                kind: CompletionKind::Option,
                label: option.name.to_owned(),
                detail: option.description.to_owned(),
                replacement: replacement.clone(),
                insertion: format!("{} ", option.name),
            },
        ));
    }
}

fn add_values(
    ranked: &mut Vec<(Rank, CompletionSuggestion)>,
    kind: CommandValueKind,
    query: &str,
    replacement: Range<usize>,
    snapshot: &MuxSnapshot,
    spec: &CommandSpec,
    previous: &[Token],
    availability: PaneKindAvailability,
) {
    let values = values_for_kind(kind, snapshot, spec, previous, availability);
    for (order, (value, label, detail)) in values.into_iter().enumerate() {
        let Some(rank) = text_rank(&format!("{value} {label} {detail}"), query, order) else {
            continue;
        };
        ranked.push((
            rank,
            CompletionSuggestion {
                kind: CompletionKind::Value,
                label,
                detail,
                replacement: replacement.clone(),
                insertion: format!("{} ", quote_argument(&value)),
            },
        ));
    }
}

fn values_for_kind(
    kind: CommandValueKind,
    snapshot: &MuxSnapshot,
    spec: &CommandSpec,
    previous: &[Token],
    availability: PaneKindAvailability,
) -> Vec<(String, String, String)> {
    match kind {
        CommandValueKind::FreeForm => Vec::new(),
        CommandValueKind::Session => snapshot
            .sessions
            .iter()
            .map(|session| {
                let id = session.id.to_string();
                (id.clone(), id, session.name.clone())
            })
            .collect(),
        CommandValueKind::Window => snapshot
            .sessions
            .iter()
            .flat_map(|session| {
                session.windows.iter().map(move |window| {
                    let id = window.id.to_string();
                    (
                        id.clone(),
                        id,
                        format!("{} · {}:{}", session.name, window.index, window.name),
                    )
                })
            })
            .collect(),
        CommandValueKind::Pane => snapshot
            .sessions
            .iter()
            .flat_map(|session| {
                session.windows.iter().flat_map(move |window| {
                    window.panes.values().map(move |pane| {
                        let id = pane.id.to_string();
                        (
                            id.clone(),
                            id,
                            format!(
                                "{} · {}:{} · {}",
                                session.name, window.index, window.name, pane.title
                            ),
                        )
                    })
                })
            })
            .collect(),
        CommandValueKind::Layout => LayoutPreset::ALL
            .into_iter()
            .map(|layout| {
                let name = layout.name().to_owned();
                (name.clone(), name, "Layout".to_owned())
            })
            .collect(),
        CommandValueKind::PaneKind => ["terminal", "browser", "agent", "editor"]
            .into_iter()
            .filter(|kind| match *kind {
                "browser" => availability.browser,
                "agent" => availability.agent,
                "editor" => availability.editor,
                _ => true,
            })
            .map(|kind| (kind.to_owned(), kind.to_owned(), "Pane kind".to_owned()))
            .collect(),
        CommandValueKind::KeyTable => KEY_TABLES
            .iter()
            .map(|table| {
                (
                    (*table).to_owned(),
                    (*table).to_owned(),
                    "Key table".to_owned(),
                )
            })
            .collect(),
        CommandValueKind::SetOption => SET_OPTIONS
            .iter()
            .filter(|option| {
                spec.name != "set-window-option"
                    || matches!(**option, "mode-keys" | "synchronize-panes")
            })
            .map(|option| {
                (
                    (*option).to_owned(),
                    (*option).to_owned(),
                    "Option".to_owned(),
                )
            })
            .collect(),
        CommandValueKind::Boolean => set_option_values(previous),
    }
}

fn set_option_values(previous: &[Token]) -> Vec<(String, String, String)> {
    let option = previous
        .iter()
        .rfind(|token| !token.value.starts_with('-'))
        .map(|token| token.value.as_str());
    let values: &[&str] = match option {
        Some("mode-keys") => &["vi", "emacs"],
        Some("set-clipboard") => &["on", "external", "off"],
        Some("synchronize-panes") => &["on", "off"],
        _ => &[],
    };
    values
        .iter()
        .map(|value| ((*value).to_owned(), (*value).to_owned(), "Value".to_owned()))
        .collect()
}

fn finish(mut ranked: Vec<(Rank, CompletionSuggestion)>) -> Vec<CompletionSuggestion> {
    ranked.sort_by_key(|(rank, _)| *rank);
    let mut seen = BTreeSet::new();
    ranked
        .into_iter()
        .filter_map(|(_, suggestion)| {
            seen.insert((
                suggestion.kind,
                suggestion.label.clone(),
                suggestion.replacement.start,
                suggestion.replacement.end,
            ))
            .then_some(suggestion)
        })
        .take(MAX_COMPLETIONS)
        .collect()
}

fn command_rank(
    spec: &CommandSpec,
    query: &str,
    order: usize,
) -> Option<(Rank, Option<&'static str>)> {
    let query = query.to_ascii_lowercase();
    let name = spec.name.to_ascii_lowercase();
    if name == query {
        return Some((
            Rank {
                class: 1,
                length: name.len(),
                order,
            },
            None,
        ));
    }
    if name.starts_with(&query) {
        return Some((
            Rank {
                class: 3,
                length: name.len(),
                order,
            },
            None,
        ));
    }
    if let Some(alias) = spec
        .aliases
        .iter()
        .find(|alias| alias.to_ascii_lowercase().starts_with(&query))
    {
        return Some((
            Rank {
                class: 5,
                length: alias.len(),
                order,
            },
            Some(alias),
        ));
    }
    if is_ordered_match(&name, &query) {
        return Some((
            Rank {
                class: 7,
                length: name.len(),
                order,
            },
            None,
        ));
    }
    spec.description
        .to_ascii_lowercase()
        .contains(&query)
        .then_some((
            Rank {
                class: 9,
                length: name.len(),
                order,
            },
            None,
        ))
}

fn text_rank(candidate: &str, query: &str, order: usize) -> Option<Rank> {
    let candidate = candidate.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    if query.is_empty() {
        return Some(Rank {
            class: 3,
            length: candidate.len(),
            order,
        });
    }
    if candidate == query {
        Some(Rank {
            class: 1,
            length: candidate.len(),
            order,
        })
    } else if candidate.starts_with(&query) {
        Some(Rank {
            class: 3,
            length: candidate.len(),
            order,
        })
    } else if candidate.contains(&query) {
        Some(Rank {
            class: 5,
            length: candidate.len(),
            order,
        })
    } else if is_ordered_match(&candidate, &query) {
        Some(Rank {
            class: 7,
            length: candidate.len(),
            order,
        })
    } else {
        None
    }
}

fn is_ordered_match(candidate: &str, query: &str) -> bool {
    let mut candidate = candidate.chars();
    query
        .chars()
        .all(|needle| candidate.by_ref().any(|character| character == needle))
}

fn active_token(tokens: &[Token], input: &str, cursor: usize) -> (usize, Range<usize>, String) {
    for (index, token) in tokens.iter().enumerate() {
        if cursor >= token.range.start && cursor <= token.range.end {
            let prefix_end = floor_char_boundary(input, cursor.min(token.range.end));
            let query = decode_token(&input[token.range.start..prefix_end]);
            return (index, token.range.clone(), query);
        }
        if cursor < token.range.start {
            return (index, cursor..cursor, String::new());
        }
    }
    (tokens.len(), cursor..cursor, String::new())
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            start.get_or_insert(offset);
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            start.get_or_insert(offset);
            quote = Some(character);
        } else if character == ';' {
            if let Some(token_start) = start.take() {
                tokens.push(Token {
                    range: token_start..offset,
                    value: decode_token(&input[token_start..offset]),
                });
            }
            let end = offset + character.len_utf8();
            tokens.push(Token {
                range: offset..end,
                value: ";".to_owned(),
            });
        } else if character.is_whitespace() {
            if let Some(token_start) = start.take() {
                tokens.push(Token {
                    range: token_start..offset,
                    value: decode_token(&input[token_start..offset]),
                });
            }
        } else {
            start.get_or_insert(offset);
        }
    }
    if let Some(token_start) = start {
        tokens.push(Token {
            range: token_start..input.len(),
            value: decode_token(&input[token_start..]),
        });
    }
    tokens
}

fn decode_token(token: &str) -> String {
    let mut decoded = String::with_capacity(token.len());
    let mut quote = None;
    let mut escaped = false;
    for character in token.chars() {
        if escaped {
            decoded.push(character);
            escaped = false;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else {
                decoded.push(character);
            }
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else {
            decoded.push(character);
        }
    }
    if escaped {
        decoded.push('\\');
    }
    decoded
}

fn quote_argument(value: &str) -> String {
    if value
        .chars()
        .all(|character| !character.is_whitespace() && !matches!(character, '\'' | '"' | '\\'))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zz_protocol::{
        LayoutNode, PaneId, PaneKindSnapshot, PaneSnapshot, SessionId, SessionSnapshot, WindowId,
        WindowSnapshot,
    };

    use super::*;

    fn snapshot() -> MuxSnapshot {
        let pane = PaneId(3);
        MuxSnapshot {
            generation: 1,
            focused_window: None,
            sessions: vec![SessionSnapshot {
                id: SessionId(1),
                name: "work".to_owned(),
                active_window: WindowId(2),
                windows: vec![WindowSnapshot {
                    id: WindowId(2),
                    index: 0,
                    name: "editor".to_owned(),
                    active_pane: pane,
                    zoomed_pane: None,
                    layout: LayoutNode::Pane(pane),
                    panes: BTreeMap::from([(
                        pane,
                        PaneSnapshot {
                            id: pane,
                            title: "shell".to_owned(),
                            kind: PaneKindSnapshot::Terminal,
                            synchronized_input: false,
                            bell: false,
                        },
                    )]),
                }],
                viewers: Vec::new(),
            }],
        }
    }

    #[test]
    fn command_alias_and_history_ranking_are_deterministic() {
        let history = vec![
            "split-window -h".to_owned(),
            "rename-window notes".to_owned(),
        ];
        let completions = complete_command(
            "ren",
            3,
            &history,
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert_eq!(completions[0].kind, CompletionKind::History);
        assert_eq!(completions[0].label, "rename-window notes");
        assert!(
            completions
                .iter()
                .any(|item| item.label == "rename-session")
        );
        assert!(completions.iter().any(|item| item.label == "rename-window"));

        let alias = complete_command("neww", 4, &[], &snapshot(), PaneKindAvailability::default());
        assert_eq!(alias[0].label, "new-window");
        assert!(alias[0].detail.contains("alias neww"));
    }

    #[test]
    fn options_and_live_targets_follow_command_context() {
        let options = complete_command(
            "rename-window -",
            15,
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert_eq!(options[0].label, "-t");

        let targets = complete_command(
            "rename-window -t ",
            17,
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert_eq!(targets[0].label, "@2");
        assert!(targets[0].detail.contains("editor"));

        let panes = complete_command(
            "select-pane ",
            12,
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert!(panes.iter().any(|item| item.label == "%3"));
    }

    #[test]
    fn quoted_unicode_tokens_have_valid_replacement_ranges() {
        let input = "rename-window -t '@2' café";
        let completions = complete_command(
            input,
            "rename-window -t '@".len(),
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert_eq!(completions[0].replacement, 17..21);
        let (completed, cursor) = apply_completion(input, &completions[0]);
        assert!(completed.is_char_boundary(cursor));
        assert!(completed.ends_with(" café"));
    }

    #[test]
    fn used_flags_are_not_suggested_twice() {
        let completions = complete_command(
            "select-pane -Z -",
            16,
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert!(!completions.iter().any(|item| item.label == "-Z"));
        assert!(completions.iter().any(|item| item.label == "-t"));
    }

    #[test]
    fn empty_input_deduplicates_history_in_recency_order() {
        let history = vec![
            "list-panes".to_owned(),
            "split-window -h".to_owned(),
            "list-panes".to_owned(),
        ];
        let completions = complete_command(
            "",
            0,
            &history,
            &snapshot(),
            PaneKindAvailability::default(),
        );
        let history = completions
            .iter()
            .filter(|item| item.kind == CompletionKind::History)
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(history, ["list-panes", "split-window -h"]);
        assert_eq!(
            completions
                .iter()
                .filter(|item| item.label == "list-panes")
                .count(),
            2,
            "history and the catalog command remain distinct suggestion kinds"
        );
    }

    #[test]
    fn static_values_and_description_discovery_are_contextual() {
        let layouts = complete_command(
            "select-layout e",
            15,
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert!(layouts.iter().any(|item| item.label == "even-horizontal"));

        let tables = complete_command(
            "bind-key -T copy",
            16,
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert!(tables.iter().any(|item| item.label == "copy-mode"));
        assert!(tables.iter().any(|item| item.label == "copy-mode-vi"));

        let discovery = complete_command(
            "destroy a session",
            17,
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert_eq!(discovery[0].label, "kill-session");

        let pane_kinds = complete_command(
            "select-pane-kind ",
            17,
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        assert_eq!(
            pane_kinds
                .iter()
                .filter(|completion| completion.detail == "Pane kind")
                .map(|completion| completion.label.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["browser", "terminal"]),
            "gated pane kinds must not be advertised"
        );

        let pane_kinds = complete_command(
            "select-pane-kind ",
            17,
            &[],
            &snapshot(),
            PaneKindAvailability {
                browser: true,
                agent: true,
                editor: true,
            },
        );
        assert_eq!(
            pane_kinds
                .iter()
                .filter(|completion| completion.detail == "Pane kind")
                .map(|completion| completion.label.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["agent", "browser", "editor", "terminal"])
        );

        let unavailable = PaneKindAvailability {
            browser: false,
            ..PaneKindAvailability::default()
        };
        let pane_kinds = complete_command("select-pane-kind ", 17, &[], &snapshot(), unavailable);
        assert_eq!(
            pane_kinds
                .iter()
                .filter(|completion| completion.detail == "Pane kind")
                .map(|completion| completion.label.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["terminal"])
        );
        for command in BROWSER_COMMANDS {
            let commands = complete_command(command, command.len(), &[], &snapshot(), unavailable);
            assert!(
                !commands
                    .iter()
                    .any(|completion| completion.label == *command)
            );
        }
    }

    #[test]
    fn completion_acceptance_preserves_text_after_the_replaced_token() {
        let input = "rename-window -t @ café";
        let suggestions = complete_command(
            input,
            "rename-window -t @".len(),
            &[],
            &snapshot(),
            PaneKindAvailability::default(),
        );
        let target = suggestions
            .iter()
            .find(|item| item.label == "@2")
            .expect("window target");
        let (completed, cursor) = apply_completion(input, target);
        assert_eq!(completed, "rename-window -t @2 café");
        assert_eq!(&completed[cursor..], " café");
    }
}
