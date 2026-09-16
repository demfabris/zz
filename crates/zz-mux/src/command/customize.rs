use super::*;
use crate::tmux_option_metadata::TmuxOptionKind;
use zz_protocol::{
    ChooseTreeItem, ChooseTreeState, ChooseTreeTarget, ChooserPresentation, ChooserPreview,
    ChooserPreviewSize, ChooserRow,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomizeMode {
    pub selected: usize,
    pub offset: usize,
    expanded: BTreeSet<String>,
    tagged: BTreeSet<String>,
    query: String,
    prompt: Option<CustomizePrompt>,
    format: Option<String>,
    filter: Option<String>,
    preview_off: bool,
    hide_global: bool,
    accept: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CustomizePrompt {
    Search,
    Filter,
    Edit {
        name: String,
        target: TmuxOptionTarget,
        prefix: String,
        value: String,
    },
}

#[derive(Clone)]
struct Row {
    id: String,
    name: String,
    text: String,
    depth: u8,
    children: bool,
    option: Option<(String, TmuxOptionTarget, Option<TmuxOption>)>,
    value: String,
}

impl Row {
    fn section(name: String, id: String, depth: u8) -> Self {
        Self {
            id,
            name,
            text: String::new(),
            depth,
            children: true,
            option: None,
            value: String::new(),
        }
    }
}

impl MuxEngine {
    pub(super) fn customize_mode(
        &self,
        context: &ExecutionContext,
        args: &[RawText],
    ) -> Result<Execution, ServerError> {
        let (options, positional) = parse_command_options("customize-mode", args)?;
        reject_positionals("customize-mode", &positional)?;
        let pane = self.resolve_pane(options.value("-t"), context.window, context.pane)?;
        if options.has("-k") || options.has("-Z") {
            return Err(ServerError::InvalidCommand(
                "customize-mode -k and -Z are not implemented (TUI-014)".to_owned(),
            ));
        }
        Ok(Execution::effect(MuxEffect::PaneModeChanged {
            pane,
            mode: Some(PaneModeRequest::Customize(Box::new(CustomizeMode {
                format: options.value("-F").map(str::to_owned),
                filter: options.value("-f").map(str::to_owned),
                preview_off: options.has("-N"),
                accept: options.has("-y"),
                ..CustomizeMode::default()
            }))),
        }))
    }

    fn customize_rows(&self, pane: PaneId, mode: &CustomizeMode) -> Vec<Row> {
        let all = self.customize_all_rows(pane, mode);
        let mut hidden_below = None;
        all.into_iter()
            .filter(|row| {
                if hidden_below.is_some_and(|depth| row.depth > depth) {
                    return false;
                }
                hidden_below = if row.children && !mode.expanded.contains(&row.id) {
                    Some(row.depth)
                } else {
                    None
                };
                true
            })
            .collect()
    }

    fn customize_all_rows(&self, pane: PaneId, mode: &CustomizeMode) -> Vec<Row> {
        let Some(window) = self
            .state
            .window_for_pane(pane)
            .and_then(|id| self.state.windows.get(&id))
        else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (index, name, targets) in [
            (0, "Server Options", vec![TmuxOptionTarget::Server]),
            (
                1,
                "Session Options",
                vec![
                    TmuxOptionTarget::Session(window.session),
                    TmuxOptionTarget::GlobalSession,
                ],
            ),
            (
                2,
                "Window & Pane Options",
                vec![
                    TmuxOptionTarget::Pane(pane),
                    TmuxOptionTarget::Window(window.id),
                    TmuxOptionTarget::GlobalWindow,
                ],
            ),
        ] {
            let id = format!("options:{index}");
            rows.push(Row::section(name.to_owned(), id.clone(), 0));
            let mut options = tmux_options()
                .filter(|option| match option.scope {
                    TmuxOptionScope::Server => index == 0,
                    TmuxOptionScope::Session => index == 1,
                    TmuxOptionScope::Window | TmuxOptionScope::WindowPane => index == 2,
                })
                .map(|option| (option.name.to_owned(), Some(option)))
                .collect::<BTreeMap<_, _>>();
            for target in &targets {
                if let Some(user) = self.user_options_at_target(*target) {
                    options.extend(user.keys().map(|name| (name.clone(), None)));
                }
            }
            for (name, metadata) in options {
                let mut owner = *targets.last().unwrap();
                let mut value = String::new();
                for target in &targets {
                    let found = if let Some(option) = metadata {
                        self.tmux_option_readback(option, *target, false)
                            .ok()
                            .flatten()
                            .map(|(v, _)| v)
                    } else {
                        self.user_option_at_target(*target, &name)
                            .map(str::to_owned)
                    };
                    if let Some(found) = found {
                        owner = *target;
                        value = found;
                        break;
                    }
                }
                if value.is_empty()
                    && let Some(option) = metadata
                {
                    value = self
                        .tmux_option_readback(option, owner, true)
                        .ok()
                        .flatten()
                        .map(|(v, _)| v)
                        .unwrap_or_default();
                }
                let global = matches!(
                    owner,
                    TmuxOptionTarget::Server
                        | TmuxOptionTarget::GlobalSession
                        | TmuxOptionTarget::GlobalWindow
                );
                if mode.hide_global && global {
                    continue;
                }
                let scope = self.customize_scope(owner);
                let array = metadata.is_some_and(|option| option.is_array);
                let unit = metadata.map_or("", |option| option.metadata.unit);
                let vars = BTreeMap::from([
                    ("is_option".to_owned(), "1".to_owned()),
                    ("is_key".to_owned(), "0".to_owned()),
                    ("option_name".to_owned(), name.clone()),
                    ("option_value".to_owned(), value.clone()),
                    ("option_is_global".to_owned(), u8::from(global).to_string()),
                    ("option_is_array".to_owned(), u8::from(array).to_string()),
                    ("option_scope".to_owned(), scope.clone()),
                    ("option_unit".to_owned(), unit.to_owned()),
                ]);
                if mode
                    .filter
                    .as_ref()
                    .is_some_and(|filter| !format_true(&customize_expand(filter, &vars)))
                {
                    continue;
                }
                let text = if array {
                    String::new()
                } else if let Some(format) = &mode.format {
                    customize_expand(format, &vars)
                } else {
                    format!(
                        "{}#[fg=themelightgrey]#[ignore]{value}{}",
                        if global {
                            String::new()
                        } else {
                            format!("#[reverse]({scope})#[default] ")
                        },
                        if unit.is_empty() {
                            String::new()
                        } else {
                            format!(" {unit}")
                        }
                    )
                };
                let row_id = format!("{id}/{name}");
                rows.push(Row {
                    id: row_id.clone(),
                    name: name.clone(),
                    text,
                    depth: 1,
                    children: array
                        && self
                            .array_option_readback(owner, &name, true)
                            .is_some_and(|(values, _)| !values.is_empty()),
                    option: Some((name.clone(), owner, metadata)),
                    value,
                });
                if array && let Some((values, _)) = self.array_option_readback(owner, &name, true) {
                    for (key, value) in values {
                        let key = key.display();
                        let full_name = format!("{name}[{key}]");
                        rows.push(Row {
                            id: format!("{row_id}/{key}"),
                            name: key,
                            text: format!("#[fg=themelightgrey]#[ignore]{value}"),
                            depth: 2,
                            children: false,
                            option: Some((full_name, owner, metadata)),
                            value: value.clone(),
                        });
                    }
                }
            }
        }
        for table in self
            .keys
            .table_names()
            .filter(|table| !matches!(*table, "choose-tree" | "choose-buffer" | "choose-client"))
        {
            let bindings = self.keys.list(Some(table)).collect::<Vec<_>>();
            if bindings.is_empty() {
                continue;
            }
            let id = format!("keys:{table}");
            rows.push(Row::section(format!("Key Table - {table}"), id.clone(), 0));
            for (_, key, binding) in bindings {
                let key_id = format!("{id}/{key}");
                rows.push(Row::section(key.to_owned(), key_id.clone(), 1));
                {
                    for (name, value) in [
                        (
                            "Command",
                            binding
                                .commands
                                .iter()
                                .map(format_command)
                                .collect::<Vec<_>>()
                                .join(" ; "),
                        ),
                        ("Note", binding.note.clone().unwrap_or_default()),
                        (
                            "Repeat",
                            if binding.repeat { "on" } else { "off" }.to_owned(),
                        ),
                    ] {
                        rows.push(Row {
                            id: format!("{key_id}/{name}"),
                            name: name.to_owned(),
                            text: format!("#[ignore]{value}"),
                            depth: 2,
                            children: false,
                            option: None,
                            value,
                        });
                    }
                }
            }
        }
        for index in 0..rows.len() {
            rows[index].children = rows
                .get(index + 1)
                .is_some_and(|next| next.depth > rows[index].depth);
        }
        rows
    }

    fn customize_search(&self, pane: PaneId, mode: &mut CustomizeMode, reverse: bool) {
        let rows = self.customize_all_rows(pane, mode);
        let selected = self
            .customize_rows(pane, mode)
            .get(mode.selected)
            .map(|row| row.id.clone());
        let start = rows
            .iter()
            .position(|row| Some(&row.id) == selected.as_ref())
            .unwrap_or(0);
        for step in 1..=rows.len() {
            let index = if reverse {
                (start + rows.len() - step) % rows.len()
            } else {
                (start + step) % rows.len()
            };
            let row = &rows[index];
            if row.name.to_lowercase().contains(&mode.query.to_lowercase()) {
                let mut parent = row.id.as_str();
                while let Some((id, _)) = parent.rsplit_once('/') {
                    mode.expanded.insert(id.to_owned());
                    parent = id;
                }
                mode.selected = self
                    .customize_rows(pane, mode)
                    .iter()
                    .position(|found| found.id == row.id)
                    .unwrap_or(0);
                break;
            }
        }
    }

    fn customize_scope(&self, target: TmuxOptionTarget) -> String {
        match target {
            TmuxOptionTarget::Session(id) => format!(
                "session {}",
                self.state
                    .sessions
                    .get(&id)
                    .map_or("", |session| session.name.as_str())
            ),
            TmuxOptionTarget::Window(id) => format!(
                "window {}",
                self.state.windows.get(&id).map_or(0, |window| window.index)
            ),
            TmuxOptionTarget::Pane(id) => format!(
                "pane {}",
                self.state
                    .window_for_pane(id)
                    .and_then(|window| self.pane_index(window, id))
                    .unwrap_or(0)
            ),
            _ => String::new(),
        }
    }

    pub fn customize_presentation(
        &self,
        pane: PaneId,
        mode: &CustomizeMode,
    ) -> (ChooseTreeState, ChooserPresentation) {
        let rows = self.customize_rows(pane, mode);
        let selected = mode.selected.min(rows.len().saturating_sub(1));
        let preview = rows
            .get(selected)
            .filter(|row| row.option.is_some())
            .map(|row| {
                let (_, owner, metadata) = row.option.as_ref().unwrap();
                let scope = metadata.map_or("user", |option| match option.scope {
                    TmuxOptionScope::Server => "server",
                    TmuxOptionScope::Session => "session",
                    TmuxOptionScope::Window => "window",
                    TmuxOptionScope::WindowPane => "window and pane",
                });
                let description = metadata
                    .map_or("This option doesn't have a description.", |option| {
                        option.metadata.description
                    });
                let mut lines = vec![
                    description.to_owned(),
                    String::new(),
                    format!("This is a {scope} option."),
                ];
                if metadata.is_some_and(|option| option.is_array) {
                    lines.push("This is an array option.".to_owned());
                    if row.children {
                        return ChooserPreview::Text { lines };
                    }
                }
                let unit = metadata.map_or("", |option| option.metadata.unit);
                let unit = if unit.is_empty() {
                    String::new()
                } else {
                    format!(" {unit}")
                };
                lines.extend([String::new(), format!("Option value: {}{unit}", row.value)]);
                if let Some(option) = metadata {
                    if !option.metadata.choices.is_empty() {
                        lines.push(format!(
                            "Available values are: {}",
                            option.metadata.choices.join(", ")
                        ));
                    }
                    if let Some(default) = option.default
                        && default.value() != row.value
                    {
                        lines.push(format!("The default is: {}{unit}", default.value()));
                    }
                    if matches!(
                        owner,
                        TmuxOptionTarget::Session(_)
                            | TmuxOptionTarget::Window(_)
                            | TmuxOptionTarget::Pane(_)
                    ) {
                        let global = if option.scope == TmuxOptionScope::Session {
                            TmuxOptionTarget::GlobalSession
                        } else {
                            TmuxOptionTarget::GlobalWindow
                        };
                        if let Ok(Some((value, _))) =
                            self.tmux_option_readback(*option, global, true)
                        {
                            lines.extend([String::new(), format!("Global value: {value}{unit}")]);
                        }
                    }
                }
                ChooserPreview::Text { lines }
            });
        let prompt = match &mode.prompt {
            Some(CustomizePrompt::Search) => format!("(search) {}", mode.query),
            Some(CustomizePrompt::Filter) => format!("(filter) {}", mode.query),
            Some(CustomizePrompt::Edit { prefix, value, .. }) => format!("{prefix}{value}"),
            None => String::new(),
        };
        (
            ChooseTreeState {
                items: rows
                    .iter()
                    .enumerate()
                    .map(|(index, row)| ChooseTreeItem {
                        label: row.name.clone(),
                        detail: row.text.clone(),
                        target: ChooseTreeTarget::Pane(pane),
                        depth: row.depth,
                        flags: if row.children {
                            ChooseTreeItem::HAS_CHILDREN
                        } else {
                            0
                        } | if mode.expanded.contains(&row.id) {
                            ChooseTreeItem::EXPANDED
                        } else {
                            0
                        } | if mode.tagged.contains(&row.id) {
                            ChooseTreeItem::TAGGED
                        } else {
                            0
                        },
                        pane_kind: None,
                        key: if index < 10 {
                            index.to_string()
                        } else if index < 36 {
                            format!("M-{}", char::from(b'a' + (index - 10) as u8))
                        } else {
                            String::new()
                        },
                        text: String::new(),
                    })
                    .collect(),
                search: None,
                selected: selected as u32,
                kind: ChooseTreeKind::Panes,
                filter_no_matches: false,
                prompt,
                help: false,
            },
            ChooserPresentation {
                selected: selected as u32,
                rows: rows
                    .iter()
                    .map(|row| ChooserRow {
                        name: row.name.clone(),
                        text: row.text.clone(),
                        align: false,
                    })
                    .collect(),
                sort: String::new(),
                view: String::new(),
                filter: mode.filter.is_some(),
                selection_style: String::new(),
                border_style: String::new(),
                prompt_style: String::new(),
                preview_size: if mode.preview_off {
                    ChooserPreviewSize::Off
                } else {
                    ChooserPreviewSize::Normal
                },
                preview,
            },
        )
    }

    pub fn customize_key(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        key: &str,
    ) -> (bool, Option<CommandInvocation>) {
        if let Some(prompt) = &mut mode.prompt {
            if matches!(key, "Escape" | "C-c" | "C-g" | "\u{1b}" | "\u{3}" | "\u{7}") {
                mode.prompt = None;
                return (false, None);
            }
            if matches!(key, "Enter" | "C-m" | "\r") {
                let prompt = mode.prompt.take().unwrap();
                match prompt {
                    CustomizePrompt::Edit {
                        name,
                        target,
                        value,
                        ..
                    } => {
                        return (
                            false,
                            (!value.is_empty())
                                .then(|| customize_set_command(&name, target, &value)),
                        );
                    }
                    CustomizePrompt::Filter => {
                        mode.filter = (!mode.query.is_empty()).then(|| mode.query.clone());
                        mode.selected = 0;
                    }
                    CustomizePrompt::Search => self.customize_search(pane, mode, false),
                }
                return (false, None);
            }
            let value = if let CustomizePrompt::Edit { value, .. } = prompt {
                value
            } else {
                &mut mode.query
            };
            if matches!(key, "BSpace" | "C-h" | "\u{7f}") {
                value.pop();
            } else if key == "C-u" {
                value.clear();
            } else if key == "Space" {
                value.push(' ');
            } else if key.chars().count() == 1 {
                value.push_str(key);
            }
            return (false, None);
        }
        let rows = self.customize_rows(pane, mode);
        if rows.is_empty() {
            return (true, None);
        }
        mode.selected = mode.selected.min(rows.len() - 1);
        let row = &rows[mode.selected];
        match key {
            "q" | "Escape" | "C-c" | "C-g" | "\u{1b}" | "\u{3}" | "\u{7}" => return (true, None),
            "Up" | "k" | "C-p" => {
                mode.selected = if mode.selected == 0 {
                    rows.len() - 1
                } else {
                    mode.selected - 1
                }
            }
            "Down" | "j" | "C-n" => mode.selected = (mode.selected + 1) % rows.len(),
            "Home" | "g" | "M-<" => mode.selected = 0,
            "End" | "G" | "M->" => mode.selected = rows.len() - 1,
            "NPage" | "C-f" => mode.selected = (mode.selected + 10).min(rows.len() - 1),
            "PPage" | "C-b" => mode.selected = mode.selected.saturating_sub(10),
            "Right" | "+" | "l" => {
                if row.children {
                    mode.expanded.insert(row.id.clone());
                }
            }
            "Left" | "-" | "h" => {
                if !mode.expanded.remove(&row.id) && row.depth > 0 {
                    mode.selected = rows[..mode.selected]
                        .iter()
                        .rposition(|parent| parent.depth < row.depth)
                        .unwrap_or(0);
                }
            }
            "M--" => {
                mode.expanded.clear();
                mode.selected = 0;
            }
            "M-+" => {
                mode.expanded.extend(
                    rows.iter()
                        .filter(|row| row.depth == 0)
                        .map(|row| row.id.clone()),
                );
            }
            "t" => {
                if row.depth > 0 && !mode.tagged.remove(&row.id) {
                    mode.tagged.insert(row.id.clone());
                }
                mode.selected = (mode.selected + 1) % rows.len();
            }
            "T" => mode.tagged.clear(),
            "v" => mode.preview_off = !mode.preview_off,
            "H" => {
                mode.hide_global = !mode.hide_global;
                mode.selected = 0;
            }
            "/" | "C-s" => {
                mode.query.clear();
                mode.prompt = Some(CustomizePrompt::Search);
            }
            "n" | "N" => self.customize_search(pane, mode, key == "N"),
            "f" => {
                mode.query = mode.filter.clone().unwrap_or_default();
                mode.prompt = Some(CustomizePrompt::Filter);
            }
            "Enter" | "C-m" | "\r" | "s" | "S" | "w" | "W" => {
                if let Some((name, owner, metadata)) = &row.option {
                    let window = &self.state.windows[&self.state.window_for_pane(pane).unwrap()];
                    let target = if matches!(key, "S" | "W") {
                        match owner {
                            TmuxOptionTarget::Session(_) => TmuxOptionTarget::GlobalSession,
                            TmuxOptionTarget::Window(_) | TmuxOptionTarget::Pane(_) => {
                                TmuxOptionTarget::GlobalWindow
                            }
                            _ => *owner,
                        }
                    } else if metadata.is_some_and(|option| option.is_array) {
                        *owner
                    } else {
                        match owner {
                            TmuxOptionTarget::GlobalSession => {
                                TmuxOptionTarget::Session(window.session)
                            }
                            TmuxOptionTarget::GlobalWindow
                            | TmuxOptionTarget::Window(_)
                            | TmuxOptionTarget::Pane(_) => {
                                if metadata.is_some_and(|option| {
                                    option.scope == TmuxOptionScope::WindowPane
                                }) && key != "w"
                                {
                                    TmuxOptionTarget::Pane(pane)
                                } else {
                                    TmuxOptionTarget::Window(window.id)
                                }
                            }
                            _ => *owner,
                        }
                    };
                    match metadata.map(|option| option.metadata.kind) {
                        Some(TmuxOptionKind::Flag) => {
                            return (
                                false,
                                Some(customize_set_command(
                                    name,
                                    target,
                                    if row.value == "on" { "off" } else { "on" },
                                )),
                            );
                        }
                        Some(TmuxOptionKind::Choice) => {
                            let choices = metadata.unwrap().metadata.choices;
                            if !choices.is_empty() {
                                let index = choices
                                    .iter()
                                    .position(|value| *value == row.value)
                                    .unwrap_or(0);
                                return (
                                    false,
                                    Some(customize_set_command(
                                        name,
                                        target,
                                        choices[(index + 1) % choices.len()],
                                    )),
                                );
                            }
                        }
                        _ => {}
                    }
                    let scope = self.customize_scope(target);
                    let suffix = if !scope.is_empty() {
                        format!(", for {scope}")
                    } else if target != TmuxOptionTarget::Server {
                        ", global".to_owned()
                    } else {
                        String::new()
                    };
                    mode.prompt = Some(CustomizePrompt::Edit {
                        name: name.clone(),
                        target,
                        prefix: format!("({name}{suffix}) "),
                        value: row.value.clone(),
                    });
                }
            }
            _ => {
                let index = key
                    .parse::<usize>()
                    .ok()
                    .filter(|index| *index < 10)
                    .or_else(|| {
                        key.strip_prefix("M-")
                            .and_then(|value| value.bytes().next())
                            .filter(u8::is_ascii_lowercase)
                            .map(|value| usize::from(value - b'a') + 10)
                    });
                if let Some(index) = index.filter(|index| *index < rows.len()) {
                    mode.selected = index;
                    return self.customize_key(pane, mode, "Enter");
                }
            }
        }
        let height = self
            .state
            .window_for_pane(pane)
            .and_then(|id| self.state.windows.get(&id))
            .map_or(11, |window| {
                usize::from(window.layout.extent().1)
                    .saturating_sub(if mode.preview_off { 0 } else { 12 })
                    .max(1)
            });
        if mode.selected < mode.offset {
            mode.offset = mode.selected;
        }
        if mode.selected >= mode.offset + height {
            mode.offset = mode.selected + 1 - height;
        }
        (false, None)
    }
}

fn customize_set_command(name: &str, target: TmuxOptionTarget, value: &str) -> CommandInvocation {
    let mut args: Vec<String> = match target {
        TmuxOptionTarget::Server => vec!["-s".to_owned()],
        TmuxOptionTarget::GlobalSession => vec!["-g".to_owned()],
        TmuxOptionTarget::GlobalWindow => vec!["-gw".to_owned()],
        TmuxOptionTarget::Session(id) => vec!["-t".to_owned(), id.to_string()],
        TmuxOptionTarget::Window(id) => vec!["-w".to_owned(), "-t".to_owned(), id.to_string()],
        TmuxOptionTarget::Pane(id) => vec!["-p".to_owned(), "-t".to_owned(), id.to_string()],
    };
    args.extend([name.to_owned(), value.to_owned()]);
    CommandInvocation::new("set-option", args)
}

fn customize_expand(format: &str, variables: &BTreeMap<String, String>) -> String {
    struct Hooks<'a>(&'a BTreeMap<String, String>);
    impl StatusHooks for Hooks<'_> {
        fn strftime(&mut self, value: &str) -> String {
            value.to_owned()
        }
        fn shell(&mut self, _command: &str, _tag: &FormatJobTag) -> String {
            String::new()
        }
        fn variable(&mut self, name: &str, _context: &StatusContext) -> Option<String> {
            self.0.get(name).cloned()
        }
    }
    crate::expand_format_values(format, &StatusContext::default(), &mut Hooks(variables))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customize_edits_a_number_and_preserves_the_selected_option() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "customize"]),
            )
            .unwrap();
        let pane = context.pane.unwrap();
        let mut mode = CustomizeMode::default();
        assert_eq!(
            engine.customize_presentation(pane, &mode).0.items[0].label,
            "Server Options"
        );
        engine.customize_key(pane, &mut mode, "Right");
        let rows = engine.customize_rows(pane, &mode);
        mode.selected = rows
            .iter()
            .position(|row| row.name == "buffer-limit")
            .unwrap();
        engine.customize_key(pane, &mut mode, "Enter");
        engine.customize_key(pane, &mut mode, "C-u");
        engine.customize_key(pane, &mut mode, "7");
        let (close, command) = engine.customize_key(pane, &mut mode, "Enter");
        assert!(!close);
        engine.execute(&mut context, &command.unwrap()).unwrap();
        let rows = engine.customize_rows(pane, &mode);
        assert_eq!(rows[mode.selected].name, "buffer-limit");
        assert_eq!(rows[mode.selected].value, "7");
        assert!(engine.customize_key(pane, &mut mode, "q").0);
    }

    #[test]
    fn customize_cycles_flags_and_choices_in_the_local_scope() {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "customize"]),
            )
            .unwrap();
        let pane = context.pane.unwrap();
        let mut mode = CustomizeMode::default();
        mode.expanded.insert("options:1".to_owned());
        engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "mouse", "off"]),
            )
            .unwrap();
        for (name, expected) in [("mouse", "on"), ("status-position", "top")] {
            mode.selected = engine
                .customize_rows(pane, &mode)
                .iter()
                .position(|row| row.name == name)
                .unwrap();
            let (_, command) = engine.customize_key(pane, &mut mode, "Enter");
            engine.execute(&mut context, &command.unwrap()).unwrap();
            let rows = engine.customize_rows(pane, &mode);
            assert_eq!(rows[mode.selected].value, expected);
            assert_eq!(
                rows[mode.selected].option.as_ref().unwrap().1,
                TmuxOptionTarget::Session(context.session.unwrap())
            );
        }
    }
}
