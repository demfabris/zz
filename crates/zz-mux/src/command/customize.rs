use std::fmt::Write as _;

use super::mode_prompt::{ModeKey, ModeMouseKey, ModePrompt, PromptOutcome};
use super::*;
use crate::tmux_option_metadata::TmuxOptionKind;
use zz_protocol::{
    ChooseTreeItem, ChooseTreeState, ChooseTreeTarget, ChooserPresentation, ChooserPreview,
    ChooserPreviewSize, ChooserRow,
};

const CUSTOMIZE_COLOUR_FLAG_OPTIONS: &[&str] = &[
    "clock-mode-colour",
    "cursor-colour",
    "dark-theme-black",
    "dark-theme-blue",
    "dark-theme-cyan",
    "dark-theme-dark-grey",
    "dark-theme-green",
    "dark-theme-light-grey",
    "dark-theme-magenta",
    "dark-theme-red",
    "dark-theme-white",
    "dark-theme-yellow",
    "display-panes-active-colour",
    "display-panes-colour",
    "light-theme-black",
    "light-theme-blue",
    "light-theme-cyan",
    "light-theme-dark-grey",
    "light-theme-green",
    "light-theme-light-grey",
    "light-theme-magenta",
    "light-theme-red",
    "light-theme-white",
    "light-theme-yellow",
    "prompt-command-cursor-colour",
    "prompt-cursor-colour",
];

pub type CustomizeExpand<'a> = dyn FnMut(&str, &BTreeMap<String, String>) -> String + 'a;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Preview {
    #[default]
    Normal,
    Off,
    Big,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PromptPurpose {
    Search,
    Filter,
    Option {
        name: String,
        array_key: Option<String>,
        target: TmuxOptionTarget,
    },
    ArrayKey {
        name: String,
        array_key: String,
        target: TmuxOptionTarget,
    },
    Command {
        table: String,
        key: String,
    },
    Note {
        table: String,
        key: String,
    },
    Reset,
    Unset,
    ResetTagged,
    UnsetTagged,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomizeMode {
    current: usize,
    offset: usize,
    height: usize,
    expanded: BTreeSet<String>,
    tagged: BTreeSet<String>,
    preview: Preview,
    filter: Option<String>,
    search: Option<String>,
    prompt: Option<(ModePrompt, PromptPurpose)>,
    help: bool,
    format: Option<String>,
    hide_global: bool,
    accept: bool,
    rebuild: bool,
    rebuild_tag: Option<String>,
    pub kill_source: bool,
    pub zoom: bool,
}

impl CustomizeMode {
    /// `PROMPT_COMMANDMODE`: `prompt_draw` paints the mode's prompt row with
    /// `message-command-style` while a vi prompt sits in command mode.
    #[must_use]
    pub fn prompt_command_mode(&self) -> bool {
        self.prompt
            .as_ref()
            .is_some_and(|(prompt, _)| prompt.command_mode())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Item {
    Section,
    Option {
        name: String,
        array_key: Option<String>,
        target: TmuxOptionTarget,
        metadata: Option<TmuxOption>,
        value: String,
        global: bool,
    },
    Key {
        table: String,
        key: String,
    },
    KeyField {
        table: String,
        key: String,
    },
}

#[derive(Clone, Debug)]
struct Row {
    id: String,
    name: String,
    text: Option<String>,
    depth: u8,
    parent: Option<usize>,
    children: bool,
    no_tag: bool,
    item: Item,
}

/// The prompt facts `prompt_set_options` copies off the session, kept for the
/// whole life of every prompt the mode raises.
struct ModePromptOptions {
    vi: bool,
    separators: String,
}

impl ModePromptOptions {
    fn prompt(&self, label: impl Into<String>, input: &str) -> ModePrompt {
        ModePrompt::new(label, input, &self.separators).with_status_keys(self.vi)
    }

    fn single(&self, label: impl Into<String>) -> ModePrompt {
        ModePrompt::single(label).with_status_keys(self.vi)
    }
}

pub struct CustomizeResult {
    pub close: bool,
    pub commands: Vec<CommandInvocation>,
}

impl CustomizeResult {
    const fn stay() -> Self {
        Self {
            close: false,
            commands: Vec::new(),
        }
    }
}

fn customize_lines(rows: &[Row], mode: &CustomizeMode) -> Vec<usize> {
    let mut hidden_below: Option<u8> = None;
    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if hidden_below.is_some_and(|depth| row.depth > depth) {
            continue;
        }
        hidden_below = (!mode.expanded.contains(&row.id)).then_some(row.depth);
        lines.push(index);
    }
    lines
}

fn below(height: usize, current: usize) -> bool {
    height.checked_sub(1).is_some_and(|last| current > last)
}

impl CustomizeMode {
    fn up(&mut self, size: usize, wrap: bool) {
        if size == 0 {
            return;
        }
        if self.current == 0 {
            if wrap {
                self.current = size - 1;
                if size >= self.height {
                    self.offset = size - self.height;
                }
            }
        } else {
            self.current -= 1;
            if self.current < self.offset {
                self.offset -= 1;
            }
        }
    }

    fn down(&mut self, size: usize, wrap: bool) -> bool {
        if size == 0 {
            return false;
        }
        if self.current == size - 1 {
            if !wrap {
                return false;
            }
            self.current = 0;
            self.offset = 0;
        } else {
            self.current += 1;
            if self.current + 1 > self.offset + self.height {
                self.offset += 1;
            }
        }
        true
    }

    fn offset_for_current(&mut self) {
        self.offset = if below(self.height, self.current) {
            self.current + 1 - self.height
        } else {
            0
        };
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
        let mut mode = CustomizeMode {
            format: options.value("-F").map(str::to_owned),
            filter: options.value("-f").map(str::to_owned),
            preview: if options.has("-N") {
                Preview::Off
            } else {
                Preview::Normal
            },
            accept: options.has("-y"),
            kill_source: options.has("-k"),
            zoom: options.has("-Z"),
            ..CustomizeMode::default()
        };
        self.customize_height(pane, &mut mode);
        Ok(Execution::effect(MuxEffect::PaneModeChanged {
            pane,
            mode: Some(PaneModeRequest::Customize(Box::new(mode))),
        }))
    }

    fn customize_screen_rows(&self, pane: PaneId) -> usize {
        self.pane_geometry(pane)
            .map_or(24, |(_, rows)| usize::from(rows))
    }

    fn customize_screen_columns(&self, pane: PaneId) -> usize {
        self.pane_geometry(pane)
            .map_or(80, |(columns, _)| usize::from(columns))
    }

    fn customize_height(&self, pane: PaneId, mode: &mut CustomizeMode) {
        let sy = self.customize_screen_rows(pane);
        if mode.preview == Preview::Off {
            mode.height = sy;
        } else if 12 < sy {
            mode.height = sy - 12;
        }
        if sy.checked_sub(mode.height).is_some_and(|rest| rest < 2) {
            mode.height = sy;
        }
    }

    fn customize_array_values(&self, target: TmuxOptionTarget, name: &str) -> StringArray {
        self.array_option_readback(target, name, true)
            .map(|(values, _)| values.clone())
            .unwrap_or_default()
    }

    fn customize_build(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        tag: Option<String>,
        expand: &mut CustomizeExpand<'_>,
    ) {
        let rows = self.customize_rows(pane, mode, expand);
        mode.tagged.retain(|id| {
            rows.iter().find(|row| row.id == *id).is_some_and(|row| {
                row.parent
                    .is_none_or(|parent| mode.expanded.contains(&rows[parent].id))
            })
        });
        let lines = customize_lines(&rows, mode);
        if let Some(found) = tag.and_then(|tag| lines.iter().position(|line| rows[*line].id == tag))
        {
            mode.current = found;
            mode.offset_for_current();
        } else if mode.current >= lines.len() && !lines.is_empty() {
            mode.current = lines.len() - 1;
            mode.offset_for_current();
        }
        self.customize_height(pane, mode);
        if below(mode.height, mode.current) {
            mode.offset = mode.current + 1 - mode.height;
        }
    }

    pub fn customize_finish(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        expand: &mut CustomizeExpand<'_>,
    ) {
        if std::mem::take(&mut mode.rebuild) {
            let tag = mode.rebuild_tag.take();
            self.customize_build(pane, mode, tag, expand);
        }
    }

    fn customize_rows(
        &self,
        pane: PaneId,
        mode: &CustomizeMode,
        expand: &mut CustomizeExpand<'_>,
    ) -> Vec<Row> {
        let Some(window) = self
            .state
            .window_for_pane(pane)
            .and_then(|id| self.state.windows.get(&id))
        else {
            return Vec::new();
        };
        let format = mode.format.clone();
        let mut rows = Vec::new();
        for (index, title, targets) in [
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
            let section = rows.len();
            let section_id = format!("options:{index}");
            rows.push(Row {
                id: section_id.clone(),
                name: title.to_owned(),
                text: None,
                depth: 0,
                parent: None,
                children: false,
                no_tag: true,
                item: Item::Section,
            });
            let mut user = Vec::<String>::new();
            for target in targets.iter().rev() {
                if let Some(options) = self.user_options_at_target(*target) {
                    for name in options.keys() {
                        if !user.contains(name) {
                            user.push(name.clone());
                        }
                    }
                }
            }
            let table = tmux_options()
                .filter(|option| !tmux_option_is_hook(option.name))
                .filter(|option| match option.scope {
                    TmuxOptionScope::Server => index == 0,
                    TmuxOptionScope::Session => index == 1,
                    TmuxOptionScope::Window | TmuxOptionScope::WindowPane => index == 2,
                })
                .map(|option| (option.name.to_owned(), option))
                .collect::<BTreeMap<_, _>>();
            let names = user
                .into_iter()
                .map(|name| (name, None))
                .chain(table.into_iter().map(|(name, option)| (name, Some(option))));
            for (name, metadata) in names {
                let mut owner = *targets.last().expect("a section has a target");
                let mut value = None;
                for target in &targets {
                    let found = if let Some(option) = metadata {
                        if option.is_array {
                            self.array_option(*target, &name).map(|values| {
                                values.values().cloned().collect::<Vec<_>>().join(" ")
                            })
                        } else {
                            self.tmux_option_readback(option, *target, false)
                                .ok()
                                .flatten()
                                .map(|(value, _)| value)
                        }
                    } else {
                        self.user_option_at_target(*target, &name)
                            .map(str::to_owned)
                    };
                    if let Some(found) = found {
                        owner = *target;
                        value = Some(found);
                        break;
                    }
                }
                let value = value.unwrap_or_else(|| {
                    metadata
                        .and_then(|option| {
                            if option.is_array {
                                Some(
                                    self.customize_array_values(owner, &name)
                                        .into_values()
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                )
                            } else {
                                self.tmux_option_readback(option, owner, true)
                                    .ok()
                                    .flatten()
                                    .map(|(value, _)| value)
                            }
                        })
                        .unwrap_or_default()
                });
                let global = matches!(
                    owner,
                    TmuxOptionTarget::Server
                        | TmuxOptionTarget::GlobalSession
                        | TmuxOptionTarget::GlobalWindow
                );
                if mode.hide_global && global {
                    continue;
                }
                let array = metadata.is_some_and(|option| option.is_array);
                let scope = self.customize_scope(owner);
                let unit = metadata.map_or("", |option| option.metadata.unit);
                let mut vars = BTreeMap::from([
                    ("is_option".to_owned(), "1".to_owned()),
                    ("is_key".to_owned(), "0".to_owned()),
                    ("option_name".to_owned(), name.clone()),
                    ("option_is_global".to_owned(), u8::from(global).to_string()),
                    ("option_is_array".to_owned(), u8::from(array).to_string()),
                    ("option_scope".to_owned(), scope.clone()),
                    ("option_unit".to_owned(), unit.to_owned()),
                ]);
                if !array {
                    vars.insert("option_value".to_owned(), value.clone());
                }
                if let Some(filter) = &mode.filter
                    && !format_true(&expand(filter, &vars))
                {
                    continue;
                }
                let text = |vars: &BTreeMap<String, String>, expand: &mut CustomizeExpand<'_>| {
                    format.as_ref().map_or_else(
                        || {
                            format!(
                                "{}#[fg=themelightgrey]#[ignore]{}{}",
                                if global {
                                    String::new()
                                } else {
                                    format!("#[reverse]({scope})#[default] ")
                                },
                                vars.get("option_value").map_or("", String::as_str),
                                if unit.is_empty() {
                                    String::new()
                                } else {
                                    format!(" {unit}")
                                }
                            )
                        },
                        |format| expand(format, vars),
                    )
                };
                let option_row = rows.len();
                let row_id = format!("{section_id}/{name}");
                rows.push(Row {
                    id: row_id.clone(),
                    name: name.clone(),
                    text: (!array).then(|| text(&vars, expand)),
                    depth: 1,
                    parent: Some(section),
                    children: false,
                    no_tag: false,
                    item: Item::Option {
                        name: name.clone(),
                        array_key: None,
                        target: owner,
                        metadata,
                        value: value.clone(),
                        global,
                    },
                });
                if array {
                    for (key, entry) in self.customize_array_values(owner, &name) {
                        let key = key.display();
                        let full_name = format!("{name}[{key}]");
                        vars.insert("option_name".to_owned(), full_name.clone());
                        vars.insert("option_value".to_owned(), entry.clone());
                        rows.push(Row {
                            id: format!("{row_id}/{key}"),
                            name: full_name,
                            text: Some(text(&vars, expand)),
                            depth: 2,
                            parent: Some(option_row),
                            children: false,
                            no_tag: false,
                            item: Item::Option {
                                name: name.clone(),
                                array_key: Some(key),
                                target: owner,
                                metadata,
                                value: entry,
                                global,
                            },
                        });
                    }
                }
            }
        }
        let tables = self
            .keys
            .table_names()
            .filter(|table| !matches!(*table, "choose-tree" | "choose-buffer" | "choose-client"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for table in tables {
            let bindings = listed_keys(self.keys.list(Some(&table)));
            if bindings.is_empty() {
                continue;
            }
            let section = rows.len();
            let id = format!("keys:{table}");
            rows.push(Row {
                id: id.clone(),
                name: format!("Key Table - {table}"),
                text: None,
                depth: 0,
                parent: None,
                children: false,
                no_tag: true,
                item: Item::Section,
            });
            for listed in bindings {
                let key = listed.key.clone();
                let mut vars = BTreeMap::from([
                    ("is_option".to_owned(), "0".to_owned()),
                    ("is_key".to_owned(), "1".to_owned()),
                    ("key".to_owned(), key.clone()),
                ]);
                if let Some(note) = &listed.binding.note {
                    vars.insert("key_note".to_owned(), note.clone());
                }
                if let Some(filter) = &mode.filter
                    && !format_true(&expand(filter, &vars))
                {
                    continue;
                }
                let key_row = rows.len();
                let key_id = format!("{id}\u{0}{key}");
                rows.push(Row {
                    id: key_id.clone(),
                    name: format
                        .as_ref()
                        .map_or_else(|| key.clone(), |format| expand(format, &vars)),
                    text: None,
                    depth: 1,
                    parent: Some(section),
                    children: false,
                    no_tag: false,
                    item: Item::Key {
                        table: table.clone(),
                        key: key.clone(),
                    },
                });
                let command = customize_command_print(listed.binding);
                for (field, text) in [
                    ("Command", format!("#[ignore]{command}")),
                    (
                        "Note",
                        listed
                            .binding
                            .note
                            .as_ref()
                            .map_or_else(String::new, |note| format!("#[ignore]{note}")),
                    ),
                    (
                        "Repeat",
                        if listed.binding.repeat { "on" } else { "off" }.to_owned(),
                    ),
                ] {
                    rows.push(Row {
                        id: format!("{key_id}\u{0}{field}"),
                        name: field.to_owned(),
                        text: Some(text),
                        depth: 2,
                        parent: Some(key_row),
                        children: false,
                        no_tag: true,
                        item: Item::KeyField {
                            table: table.clone(),
                            key: key.clone(),
                        },
                    });
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

    fn customize_search_set(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        backward: bool,
        expand: &mut CustomizeExpand<'_>,
    ) {
        let Some(search) = mode.search.clone() else {
            return;
        };
        let rows = self.customize_rows(pane, mode, expand);
        let lines = customize_lines(&rows, mode);
        let Some(&start) = lines.get(mode.current) else {
            return;
        };
        let icase = search.chars().all(|character| !character.is_uppercase());
        let needle = search.to_lowercase();
        let count = rows.len();
        let found = (1..count)
            .map(|step| {
                if backward {
                    (start + count - step) % count
                } else {
                    (start + step) % count
                }
            })
            .find(|index| {
                if icase {
                    rows[*index].name.to_lowercase().contains(&needle)
                } else {
                    rows[*index].name.contains(&search)
                }
            });
        let Some(found) = found else {
            return;
        };
        let mut parent = rows[found].parent;
        while let Some(index) = parent {
            mode.expanded.insert(rows[index].id.clone());
            parent = rows[index].parent;
        }
        let tag = rows[lines[mode.current]].id.clone();
        let found = rows[found].id.clone();
        self.customize_build(pane, mode, Some(tag), expand);
        let rows = self.customize_rows(pane, mode, expand);
        if let Some(position) = customize_lines(&rows, mode)
            .iter()
            .position(|line| rows[*line].id == found)
        {
            mode.current = position;
            mode.offset_for_current();
        }
    }

    pub fn customize_presentation(
        &self,
        pane: PaneId,
        mode: &CustomizeMode,
        expand: &mut CustomizeExpand<'_>,
    ) -> (
        ChooseTreeState,
        ChooserPresentation,
        Option<(String, u16, bool)>,
    ) {
        let rows = self.customize_rows(pane, mode, expand);
        let lines = customize_lines(&rows, mode);
        let selected = mode.current.min(lines.len().saturating_sub(1));
        let columns = self.customize_screen_columns(pane);
        let screen_rows = self.customize_screen_rows(pane);
        let preview = lines.get(selected).map(|line| {
            let row = &rows[*line];
            let row = if matches!(row.item, Item::KeyField { .. }) {
                row.parent.map_or(row, |parent| &rows[parent])
            } else {
                row
            };
            let width = columns.saturating_sub(4);
            let height = screen_rows.saturating_sub(mode.height).saturating_sub(2);
            let mut writer = PreviewWriter::new(width, height);
            match &row.item {
                Item::Option { .. } => self.customize_draw_option(pane, row, &mut writer, expand),
                Item::Key { table, key } | Item::KeyField { table, key } => {
                    self.customize_draw_key(table, key, &mut writer);
                }
                Item::Section => {}
            }
            ChooserPreview::Markup {
                lines: writer.finish(),
            }
        });
        let prompt = mode.prompt.as_ref().map(|(prompt, _)| {
            let (text, cursor) = prompt.draw(u16::try_from(columns).unwrap_or(u16::MAX));
            (text, cursor, self.customize_prompt_top(pane))
        });
        let presentation_rows = lines
            .iter()
            .map(|line| {
                let row = &rows[*line];
                ChooserRow {
                    name: row.name.clone(),
                    text: match &row.text {
                        None => String::new(),
                        Some(text) if text.is_empty() => "#[default]".to_owned(),
                        Some(text) => text.clone(),
                    },
                    align: false,
                }
            })
            .collect::<Vec<_>>();
        (
            ChooseTreeState {
                items: lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| {
                        let row = &rows[*line];
                        let title = if matches!(row.item, Item::KeyField { .. }) {
                            row.parent.map_or(&row.name, |parent| &rows[parent].name)
                        } else {
                            &row.name
                        };
                        ChooseTreeItem {
                            label: row.name.clone(),
                            detail: title.clone(),
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
                        }
                    })
                    .collect(),
                search: None,
                selected: selected as u32,
                kind: ChooseTreeKind::Panes,
                filter_no_matches: false,
                prompt: String::new(),
                help: mode.help,
            },
            ChooserPresentation {
                selected: selected as u32,
                rows: presentation_rows,
                sort: String::new(),
                view: String::new(),
                filter: mode.filter.is_some(),
                selection_style: String::new(),
                border_style: String::new(),
                prompt_style: String::new(),
                preview_size: match mode.preview {
                    Preview::Normal => ChooserPreviewSize::Normal,
                    Preview::Off => ChooserPreviewSize::Off,
                    Preview::Big => ChooserPreviewSize::Big,
                },
                preview,
            },
            prompt,
        )
    }

    #[must_use]
    pub fn customize_offset(mode: &CustomizeMode) -> u32 {
        u32::try_from(mode.offset).unwrap_or(u32::MAX)
    }

    fn customize_prompt_top(&self, pane: PaneId) -> bool {
        self.state
            .window_for_pane(pane)
            .and_then(|window| self.state.windows.get(&window))
            .is_some_and(|window| {
                self.status_formats_for_session(Some(window.session))
                    .position
                    == crate::StatusPosition::Top
            })
    }

    fn customize_draw_key(&self, table: &str, key: &str, writer: &mut PreviewWriter) {
        let Some(binding) = self.keys.get(table, key) else {
            return;
        };
        let note = binding
            .note
            .as_deref()
            .unwrap_or("There is no note for this key.");
        let period = if !note.is_empty() && !note.ends_with('.') {
            "."
        } else {
            ""
        };
        if !writer.text(false, "", &format!("{note}{period}")) || !writer.skip_line() {
            return;
        }
        if !writer.text(false, "", &format!("This key is in the {table} table."))
            || !writer.text(
                false,
                "",
                &format!(
                    "This key {} repeat.",
                    if binding.repeat { "does" } else { "does not" }
                ),
            )
            || !writer.skip_line()
        {
            return;
        }
        let command = customize_command_print(binding);
        if !writer.text(false, "", &format!("Command: {command}")) {
            return;
        }
        if let Some(default) = KeyTables::default().get(table, key) {
            let default_command = customize_command_print(default);
            if default_command != command {
                writer.text(false, "", &format!("The default is: {default_command}"));
            }
        }
    }

    fn customize_draw_option(
        &self,
        pane: PaneId,
        row: &Row,
        writer: &mut PreviewWriter,
        expand: &mut CustomizeExpand<'_>,
    ) {
        let Item::Option {
            name,
            array_key,
            target,
            metadata,
            value,
            ..
        } = &row.item
        else {
            return;
        };
        let (space, unit) = match metadata.map(|option| option.metadata.unit) {
            Some(unit) if !unit.is_empty() => (" ", unit),
            _ => ("", ""),
        };
        let description = metadata.map_or("This option doesn't have a description.", |option| {
            option.metadata.description
        });
        if !writer.text(false, "", description) || !writer.skip_line() {
            return;
        }
        let scope = metadata.map_or("user", |option| match option.scope {
            TmuxOptionScope::Server => "server",
            TmuxOptionScope::Session => "session",
            TmuxOptionScope::Window => "window",
            TmuxOptionScope::WindowPane => "window and pane",
        });
        if !writer.text(false, "", &format!("This is a {scope} option.")) {
            return;
        }
        if metadata.is_some_and(|option| option.is_array) {
            let line = array_key.as_ref().map_or_else(
                || "This is an array option.".to_owned(),
                |key| format!("This is an array option, key {key}."),
            );
            if !writer.text(false, "", &line) || array_key.is_none() {
                return;
            }
        }
        if !writer.skip_line() {
            return;
        }
        let default_value = metadata
            .filter(|_| array_key.is_none())
            .and_then(customize_default_value)
            .filter(|default| default != value);
        if !writer.text(false, "", &format!("Option value: {value}{space}{unit}")) {
            return;
        }
        let kind = metadata.map(|option| option.metadata.kind);
        let expanded = expand(value, &BTreeMap::new());
        if matches!(kind, None | Some(TmuxOptionKind::String))
            && expanded != *value
            && !writer.text(false, "", &format!("This expands to: {expanded}"))
        {
            return;
        }
        if let Some(option) = metadata
            && option.metadata.kind == TmuxOptionKind::Choice
            && !writer.text(
                false,
                "",
                &format!(
                    "Available values are: {}",
                    option.metadata.choices.join(", ")
                ),
            )
        {
            return;
        }
        if matches!(kind, Some(TmuxOptionKind::Colour))
            || CUSTOMIZE_COLOUR_FLAG_OPTIONS.contains(&name.as_str())
        {
            let style = if matches!(kind, Some(TmuxOptionKind::Colour)) {
                format!("fg={value}")
            } else {
                format!("fg={expanded}")
            };
            if !writer.text(true, "", "This is a colour option: ")
                || !writer.text(false, &customize_example_style(&style), "EXAMPLE")
            {
                return;
            }
        }
        if matches!(kind, Some(TmuxOptionKind::Style))
            && (!writer.text(true, "", "This is a style option: ")
                || !writer.text(false, &customize_example_style(&expanded), "EXAMPLE"))
        {
            return;
        }
        if let Some(default) = default_value
            && !writer.text(
                false,
                "",
                &format!("The default is: {default}{space}{unit}"),
            )
        {
            return;
        }
        if !writer.skip_line_strict() || metadata.is_some_and(|option| option.is_array) {
            return;
        }
        let option = match metadata {
            Some(option) => *option,
            None => return,
        };
        let window = self.state.window_for_pane(pane);
        if let TmuxOptionTarget::Pane(_) = target
            && let Some(window) = window
            && let Ok(Some((parent, _))) =
                self.tmux_option_readback(option, TmuxOptionTarget::Window(window), false)
        {
            let index = self
                .state
                .windows
                .get(&window)
                .map_or(0, |entry| entry.index);
            if !writer.text(
                false,
                "",
                &format!("Window value (from window {index}): {parent}{space}{unit}"),
            ) {
                return;
            }
        }
        let global = match target {
            TmuxOptionTarget::Pane(_) | TmuxOptionTarget::Window(_) => {
                Some(TmuxOptionTarget::GlobalWindow)
            }
            TmuxOptionTarget::Session(_) => Some(TmuxOptionTarget::GlobalSession),
            _ => None,
        };
        if let Some(global) = global
            && let Ok(Some((parent, _))) = self.tmux_option_readback(option, global, false)
        {
            writer.text(false, "", &format!("Global value: {parent}{space}{unit}"));
        }
    }

    pub fn customize_key(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        name: &str,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        let key = ModeKey::parse(name);
        let Some(lines) = self.customize_ready(pane, mode, expand) else {
            return CustomizeResult {
                close: true,
                commands: Vec::new(),
            };
        };
        if let Some((prompt, _)) = &mut mode.prompt {
            let outcome = prompt.key(key);
            let (value, purpose) = match outcome {
                PromptOutcome::Done => {
                    let value = prompt.input();
                    (Some(value), mode.prompt.take().map(|(_, purpose)| purpose))
                }
                PromptOutcome::Cancelled => (None, mode.prompt.take().map(|(_, purpose)| purpose)),
                PromptOutcome::Closed => {
                    mode.prompt = None;
                    return CustomizeResult::stay();
                }
                _ => return CustomizeResult::stay(),
            };
            let Some(purpose) = purpose else {
                return CustomizeResult::stay();
            };
            return self.customize_answer(pane, mode, purpose, value, expand);
        }
        if mode.help {
            mode.help = false;
            return CustomizeResult::stay();
        }
        self.customize_tree_key(pane, mode, key, lines, expand)
    }

    /// `mode_tree_build`'s line list with `mode_tree_check_selected` applied,
    /// or nothing when the tree is empty and `mode_tree_key` closes the mode.
    fn customize_ready(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        expand: &mut CustomizeExpand<'_>,
    ) -> Option<usize> {
        let rows = self.customize_rows(pane, mode, expand);
        let lines = customize_lines(&rows, mode);
        if lines.is_empty() {
            return None;
        }
        mode.current = mode.current.min(lines.len() - 1);
        Some(lines.len())
    }

    /// `mode_tree_key`'s pointer half and `window_pane_key`'s mode branch: the
    /// pin forwards a mouse key to the pane, the pane hands it to the mode, and
    /// the mode answers it before any of its own key handling runs.
    pub fn customize_mouse(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        name: &str,
        x: usize,
        y: usize,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        let Some(size) = self.customize_ready(pane, mode, expand) else {
            return CustomizeResult {
                close: true,
                commands: Vec::new(),
            };
        };
        let columns = self.customize_screen_columns(pane);
        let screen_rows = self.customize_screen_rows(pane);
        let prompt_top = self.customize_prompt_top(pane);
        if let Some((prompt, _)) = &mut mode.prompt {
            let row = if prompt_top {
                0
            } else {
                screen_rows.saturating_sub(1)
            };
            let handled = ModeMouseKey::is_press1(name)
                && y == row
                && prompt.mouse(x, columns) != PromptOutcome::NotHandled;
            if handled {
                return CustomizeResult::stay();
            }
        }
        if mode.help {
            return CustomizeResult::stay();
        }
        if x > columns || y > mode.height || mode.offset + y >= size {
            return CustomizeResult::stay();
        }
        let button = ModeMouseKey::parse(name);
        if matches!(
            button,
            ModeMouseKey::Down1 | ModeMouseKey::Down3 | ModeMouseKey::DoubleClick1
        ) {
            mode.current = mode.offset + y;
        }
        if button == ModeMouseKey::DoubleClick1 {
            return self.customize_tree_key(pane, mode, ModeKey::Char('\r'), size, expand);
        }
        CustomizeResult::stay()
    }

    fn customize_tree_key(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        key: ModeKey,
        size: usize,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        let rows = self.customize_rows(pane, mode, expand);
        let lines = customize_lines(&rows, mode);
        if lines.is_empty() {
            return CustomizeResult {
                close: true,
                commands: Vec::new(),
            };
        }
        let mut key = key;
        if let Some(choice) = (0..size.min(36)).find(|index| {
            key == if *index < 10 {
                ModeKey::Char(char::from(b'0' + *index as u8))
            } else {
                ModeKey::Meta(char::from(b'a' + (*index - 10) as u8))
            }
        }) {
            mode.current = choice;
            key = ModeKey::Char('\r');
        }
        let row_at = |mode: &CustomizeMode| &rows[lines[mode.current]];
        match key {
            ModeKey::Char('q' | '\u{1b}') | ModeKey::Ctrl('[' | 'g') => {
                return CustomizeResult {
                    close: true,
                    commands: Vec::new(),
                };
            }
            ModeKey::F1 | ModeKey::Ctrl('h') => mode.help = true,
            ModeKey::Up | ModeKey::Char('k') | ModeKey::Ctrl('p') => mode.up(size, true),
            ModeKey::Down | ModeKey::Char('j') | ModeKey::Ctrl('n') => {
                mode.down(size, true);
            }
            ModeKey::PageUp | ModeKey::Ctrl('b') => {
                for _ in 0..mode.height {
                    if mode.current == 0 {
                        break;
                    }
                    mode.up(size, true);
                }
            }
            ModeKey::PageDown | ModeKey::Ctrl('f') => {
                for _ in 0..mode.height {
                    if mode.current == size - 1 {
                        break;
                    }
                    mode.down(size, true);
                }
            }
            ModeKey::Char('g') | ModeKey::Home => {
                mode.current = 0;
                mode.offset = 0;
            }
            ModeKey::Char('G') | ModeKey::End => {
                mode.current = size - 1;
                mode.offset_for_current();
            }
            ModeKey::Char('t') => {
                let row = row_at(mode);
                if !row.no_tag {
                    if mode.tagged.contains(&row.id) {
                        mode.tagged.remove(&row.id);
                    } else {
                        let mut parent = row.parent;
                        while let Some(index) = parent {
                            mode.tagged.remove(&rows[index].id);
                            parent = rows[index].parent;
                        }
                        let id = row.id.clone();
                        mode.tagged
                            .retain(|tagged| !customize_is_descendant(&rows, &id, tagged));
                        mode.tagged.insert(id);
                    }
                }
            }
            ModeKey::Char('T') => {
                for line in &lines {
                    mode.tagged.remove(&rows[*line].id);
                }
            }
            ModeKey::Ctrl('t') => {
                for line in &lines {
                    let row = &rows[*line];
                    let tag = match row.parent {
                        None => !row.no_tag,
                        Some(parent) => rows[parent].no_tag,
                    };
                    if tag {
                        mode.tagged.insert(row.id.clone());
                    } else {
                        mode.tagged.remove(&row.id);
                    }
                }
            }
            ModeKey::Char('O' | 'r') => {
                let tag = row_at(mode).id.clone();
                self.customize_build(pane, mode, Some(tag), expand);
            }
            ModeKey::Left | ModeKey::Char('h' | '-') => {
                let line = lines[mode.current];
                let flat = customize_flat(&rows, line);
                let mut target = Some(line);
                if flat || !mode.expanded.contains(&rows[line].id) {
                    target = rows[line].parent;
                }
                match target {
                    None => mode.up(size, false),
                    Some(target) => {
                        mode.expanded.remove(&rows[target].id);
                        if let Some(position) = lines.iter().position(|line| *line == target) {
                            mode.current = position;
                        }
                        let tag = row_at(mode).id.clone();
                        self.customize_build(pane, mode, Some(tag), expand);
                    }
                }
            }
            ModeKey::Right | ModeKey::Char('l' | '+') => {
                let line = lines[mode.current];
                if customize_flat(&rows, line) || mode.expanded.contains(&rows[line].id) {
                    mode.down(size, false);
                } else {
                    mode.expanded.insert(rows[line].id.clone());
                    let tag = rows[line].id.clone();
                    self.customize_build(pane, mode, Some(tag), expand);
                }
            }
            ModeKey::Meta('-') => {
                let tag = row_at(mode).id.clone();
                for row in rows.iter().filter(|row| row.parent.is_none()) {
                    mode.expanded.remove(&row.id);
                }
                self.customize_build(pane, mode, Some(tag), expand);
            }
            ModeKey::Meta('+') => {
                let tag = row_at(mode).id.clone();
                for row in rows.iter().filter(|row| row.parent.is_none()) {
                    mode.expanded.insert(row.id.clone());
                }
                self.customize_build(pane, mode, Some(tag), expand);
            }
            ModeKey::Char('?' | '/') | ModeKey::Ctrl('s') => {
                mode.prompt = Some((
                    self.customize_prompt_options(pane).prompt("(search) ", ""),
                    PromptPurpose::Search,
                ));
            }
            ModeKey::Char(direction @ ('n' | 'N')) => {
                self.customize_search_set(pane, mode, direction == 'N', expand);
            }
            ModeKey::Char('f') => {
                let input = mode.filter.clone().unwrap_or_default();
                mode.prompt = Some((
                    self.customize_prompt_options(pane)
                        .prompt("(filter) ", &input),
                    PromptPurpose::Filter,
                ));
            }
            ModeKey::Char('c') => {
                mode.prompt = None;
                mode.filter = None;
                let tag = row_at(mode).id.clone();
                self.customize_build(pane, mode, Some(tag), expand);
            }
            ModeKey::Char('v') => {
                mode.preview = match mode.preview {
                    Preview::Off => Preview::Big,
                    Preview::Normal => Preview::Off,
                    Preview::Big => Preview::Normal,
                };
                let tag = row_at(mode).id.clone();
                self.customize_build(pane, mode, Some(tag), expand);
            }
            _ => {}
        }
        let rows = self.customize_rows(pane, mode, expand);
        let lines = customize_lines(&rows, mode);
        let Some(row) = lines.get(mode.current).map(|line| rows[*line].clone()) else {
            return CustomizeResult::stay();
        };
        let tag = Some(row.id.clone());
        let separators = self.customize_prompt_options(pane);
        match key {
            ModeKey::Char('a') => {
                if let Item::Option {
                    name,
                    array_key: Some(array_key),
                    target,
                    ..
                } = &row.item
                {
                    mode.prompt = Some((
                        separators.prompt(format!("({name}[{array_key}]) "), array_key),
                        PromptPurpose::ArrayKey {
                            name: name.clone(),
                            array_key: array_key.clone(),
                            target: *target,
                        },
                    ));
                }
            }
            ModeKey::Char('\r' | 's' | 'w' | 'S' | 'W') => {
                let global = matches!(key, ModeKey::Char('S' | 'W'));
                let pane_scope = matches!(key, ModeKey::Char('\r' | 's'));
                let mut commands = Vec::new();
                match &row.item {
                    Item::Section => {
                        if matches!(key, ModeKey::Char('\r' | 's')) {
                            return CustomizeResult::stay();
                        }
                    }
                    Item::Key {
                        table,
                        key: binding,
                    }
                    | Item::KeyField {
                        table,
                        key: binding,
                    } => {
                        if !matches!(key, ModeKey::Char('\r' | 's')) {
                            return CustomizeResult::stay();
                        }
                        if let Some(command) =
                            self.customize_set_key(mode, &row, table, binding, &separators)
                        {
                            commands.push(command);
                        }
                    }
                    Item::Option { .. } => {
                        if let Some(command) = self.customize_set_option(
                            pane,
                            mode,
                            &row,
                            global,
                            pane_scope,
                            &separators,
                        ) {
                            commands.push(command);
                        }
                    }
                }
                return self.customize_commands(pane, mode, commands, tag, expand);
            }
            ModeKey::Char('d') => {
                if matches!(row.item, Item::Section)
                    || matches!(
                        row.item,
                        Item::Option {
                            array_key: Some(_),
                            ..
                        }
                    )
                {
                    return CustomizeResult::stay();
                }
                let name = customize_item_name(&row);
                return self.customize_confirm(
                    pane,
                    mode,
                    format!("Reset {name} to default? "),
                    PromptPurpose::Reset,
                    expand,
                );
            }
            ModeKey::Char('u') => {
                if matches!(row.item, Item::Section) {
                    return CustomizeResult::stay();
                }
                let label = match &row.item {
                    Item::Option {
                        name,
                        array_key: Some(array_key),
                        ..
                    } => format!("Unset {name}[{array_key}]? "),
                    _ => format!("Unset {}? ", customize_item_name(&row)),
                };
                return self.customize_confirm(pane, mode, label, PromptPurpose::Unset, expand);
            }
            ModeKey::Char(change @ ('D' | 'U')) => {
                let count = lines
                    .iter()
                    .filter(|line| mode.tagged.contains(&rows[**line].id))
                    .count();
                if count == 0 {
                    return CustomizeResult::stay();
                }
                let (label, purpose) = if change == 'D' {
                    (
                        format!("Reset {count} tagged to default? "),
                        PromptPurpose::ResetTagged,
                    )
                } else {
                    (
                        format!("Unset {count} tagged? "),
                        PromptPurpose::UnsetTagged,
                    )
                };
                return self.customize_confirm(pane, mode, label, purpose, expand);
            }
            ModeKey::Char('H') => {
                mode.hide_global = !mode.hide_global;
                self.customize_build(pane, mode, tag, expand);
            }
            _ => {}
        }
        CustomizeResult::stay()
    }

    /// `prompt_set_options`: every mode-tree prompt is created through it, so
    /// each one keeps the session's `status-keys` and `word-separators`.
    fn customize_prompt_options(&self, pane: PaneId) -> ModePromptOptions {
        let session = self
            .state
            .window_for_pane(pane)
            .map(|window| self.state.windows[&window].session);
        let (vi, separators) = self.prompt_key_options(session);
        ModePromptOptions { vi, separators }
    }

    fn customize_confirm(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        label: String,
        purpose: PromptPurpose,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        if mode.accept {
            return self.customize_answer(pane, mode, purpose, Some("y".to_owned()), expand);
        }
        mode.prompt = Some((self.customize_prompt_options(pane).single(label), purpose));
        CustomizeResult::stay()
    }

    fn customize_commands(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        commands: Vec<CommandInvocation>,
        tag: Option<String>,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        if commands.is_empty() {
            self.customize_build(pane, mode, tag, expand);
        } else {
            mode.rebuild = true;
            mode.rebuild_tag = tag;
        }
        CustomizeResult {
            close: false,
            commands,
        }
    }

    fn customize_set_key(
        &self,
        mode: &mut CustomizeMode,
        row: &Row,
        table: &str,
        key: &str,
        separators: &ModePromptOptions,
    ) -> Option<CommandInvocation> {
        let binding = self.keys.get(table, key)?;
        match row.name.as_str() {
            "Repeat" if matches!(row.item, Item::KeyField { .. }) => Some(customize_bind(
                table,
                key,
                !binding.repeat,
                binding.note.as_deref(),
                &customize_command_print(binding),
            )),
            "Command" if matches!(row.item, Item::KeyField { .. }) => {
                mode.prompt = Some((
                    separators.prompt(format!("({key}) "), &customize_command_print(binding)),
                    PromptPurpose::Command {
                        table: table.to_owned(),
                        key: key.to_owned(),
                    },
                ));
                None
            }
            "Note" if matches!(row.item, Item::KeyField { .. }) => {
                mode.prompt = Some((
                    separators.prompt(
                        format!("({key}) "),
                        binding.note.as_deref().unwrap_or_default(),
                    ),
                    PromptPurpose::Note {
                        table: table.to_owned(),
                        key: key.to_owned(),
                    },
                ));
                None
            }
            _ => None,
        }
    }

    fn customize_set_option(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        row: &Row,
        global: bool,
        pane_scope: bool,
        separators: &ModePromptOptions,
    ) -> Option<CommandInvocation> {
        let Item::Option {
            name,
            array_key,
            target: owner,
            metadata,
            ..
        } = &row.item
        else {
            return None;
        };
        let window = self.state.window_for_pane(pane)?;
        let session = self.state.windows.get(&window)?.session;
        let pane_scope =
            pane_scope && metadata.is_none_or(|option| option.scope == TmuxOptionScope::WindowPane);
        let array = metadata.is_some_and(|option| option.is_array);
        let local_window = if pane_scope {
            TmuxOptionTarget::Pane(pane)
        } else {
            TmuxOptionTarget::Window(window)
        };
        let target = if array {
            *owner
        } else if global {
            match owner {
                TmuxOptionTarget::Session(_) => TmuxOptionTarget::GlobalSession,
                TmuxOptionTarget::Window(_) | TmuxOptionTarget::Pane(_) => {
                    TmuxOptionTarget::GlobalWindow
                }
                other => *other,
            }
        } else {
            match owner {
                TmuxOptionTarget::GlobalSession => TmuxOptionTarget::Session(session),
                TmuxOptionTarget::Window(_)
                | TmuxOptionTarget::Pane(_)
                | TmuxOptionTarget::GlobalWindow => local_window,
                other => *other,
            }
        };
        if let Some(option) = metadata {
            let current = || {
                self.tmux_option_readback(*option, target, true)
                    .ok()
                    .flatten()
                    .map(|(value, _)| value)
                    .unwrap_or_default()
            };
            match option.metadata.kind {
                TmuxOptionKind::Flag => {
                    return Some(customize_set_command(
                        name,
                        target,
                        if current() == "on" { "off" } else { "on" },
                    ));
                }
                TmuxOptionKind::Choice if !option.metadata.choices.is_empty() => {
                    let choices = option.metadata.choices;
                    let value = current();
                    let index = choices.iter().position(|choice| *choice == value);
                    let next = index.map_or(0, |index| (index + 1) % choices.len());
                    return Some(customize_set_command(name, target, choices[next]));
                }
                _ => {}
            }
        }
        let scope = self.customize_scope(target);
        let space = if !scope.is_empty() {
            ", for "
        } else if target == TmuxOptionTarget::Server {
            ""
        } else {
            ", global"
        };
        let label = match (array, array_key) {
            (true, None) => format!("({name}[+]{space}{scope}) "),
            (true, Some(key)) => format!("({name}[{key}]{space}{scope}) "),
            (false, _) => format!("({name}{space}{scope}) "),
        };
        let value = match &row.item {
            Item::Option { value, .. } => value.clone(),
            _ => String::new(),
        };
        mode.prompt = Some((
            separators.prompt(label, &value),
            PromptPurpose::Option {
                name: name.clone(),
                array_key: array_key.clone(),
                target,
            },
        ));
        None
    }

    fn customize_answer(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        purpose: PromptPurpose,
        value: Option<String>,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        let rows = self.customize_rows(pane, mode, expand);
        let lines = customize_lines(&rows, mode);
        let current = lines.get(mode.current).map(|line| &rows[*line]);
        let tag = current.map(|row| row.id.clone());
        match purpose {
            PromptPurpose::Search => {
                mode.search = value.filter(|value| !value.is_empty());
                if mode.search.is_some() {
                    self.customize_search_set(pane, mode, false, expand);
                }
                CustomizeResult::stay()
            }
            PromptPurpose::Filter => {
                mode.filter = value.filter(|value| !value.is_empty());
                self.customize_build(pane, mode, tag, expand);
                CustomizeResult::stay()
            }
            PromptPurpose::Option {
                name,
                array_key,
                target,
            } => {
                let Some(value) = value.filter(|value| !value.is_empty()) else {
                    return CustomizeResult::stay();
                };
                let full = if tmux_options().any(|option| option.name == name && option.is_array) {
                    let key = if let Some(key) = array_key {
                        key
                    } else {
                        let values = self.customize_array_values(target, &name);
                        let Ok(index) = first_free_array_index(values.keys()) else {
                            return CustomizeResult::stay();
                        };
                        index.to_string()
                    };
                    format!("{name}[{key}]")
                } else {
                    name
                };
                self.customize_commands(
                    pane,
                    mode,
                    vec![customize_set_command(&full, target, &value)],
                    tag,
                    expand,
                )
            }
            PromptPurpose::ArrayKey {
                name,
                array_key,
                target,
            } => {
                let Some(new_key) = value.filter(|value| !value.is_empty()) else {
                    return CustomizeResult::stay();
                };
                let values = self.customize_array_values(target, &name);
                if values.keys().any(|key| key.display() == new_key) {
                    return CustomizeResult::stay();
                }
                let Some(entry) = values
                    .iter()
                    .find(|(key, _)| key.display() == array_key)
                    .map(|(_, entry)| entry.clone())
                else {
                    return CustomizeResult::stay();
                };
                self.customize_commands(
                    pane,
                    mode,
                    vec![
                        customize_set_command(&format!("{name}[{new_key}]"), target, &entry),
                        customize_unset_command(&format!("{name}[{array_key}]"), target),
                    ],
                    tag,
                    expand,
                )
            }
            PromptPurpose::Command { table, key } => {
                let Some(command) = value.filter(|value| !value.is_empty()) else {
                    return CustomizeResult::stay();
                };
                let Some(binding) = self.keys.get(&table, &key) else {
                    return CustomizeResult::stay();
                };
                let bind = customize_bind(
                    &table,
                    &key,
                    binding.repeat,
                    binding.note.as_deref(),
                    &command,
                );
                self.customize_commands(pane, mode, vec![bind], tag, expand)
            }
            PromptPurpose::Note { table, key } => {
                let Some(note) = value.filter(|value| !value.is_empty()) else {
                    return CustomizeResult::stay();
                };
                let Some(binding) = self.keys.get(&table, &key) else {
                    return CustomizeResult::stay();
                };
                let bind = customize_bind(
                    &table,
                    &key,
                    binding.repeat,
                    Some(&note),
                    &customize_command_print(binding),
                );
                self.customize_commands(pane, mode, vec![bind], tag, expand)
            }
            PromptPurpose::Reset
            | PromptPurpose::Unset
            | PromptPurpose::ResetTagged
            | PromptPurpose::UnsetTagged => {
                if !value.is_some_and(|value| value.eq_ignore_ascii_case("y")) {
                    return CustomizeResult::stay();
                }
                let reset = matches!(purpose, PromptPurpose::Reset | PromptPurpose::ResetTagged);
                let targets = if matches!(purpose, PromptPurpose::Reset | PromptPurpose::Unset) {
                    current.map(|row| vec![row.clone()]).unwrap_or_default()
                } else {
                    lines
                        .iter()
                        .map(|line| &rows[*line])
                        .filter(|row| mode.tagged.contains(&row.id))
                        .cloned()
                        .collect()
                };
                let current_id = current.map(|row| row.id.clone());
                let mut commands = Vec::new();
                for row in targets {
                    let is_current = current_id.as_ref() == Some(&row.id)
                        || current.is_some_and(|current| customize_same_key(current, &row));
                    commands.extend(
                        self.customize_change(mode, &rows, &lines, &row, reset, is_current),
                    );
                }
                let tag = lines.get(mode.current).map(|line| rows[*line].id.clone());
                self.customize_commands(pane, mode, commands, tag, expand)
            }
        }
    }

    fn customize_change(
        &self,
        mode: &mut CustomizeMode,
        rows: &[Row],
        lines: &[usize],
        row: &Row,
        reset: bool,
        is_current: bool,
    ) -> Vec<CommandInvocation> {
        match &row.item {
            Item::Section => Vec::new(),
            Item::Option {
                name,
                array_key,
                target,
                ..
            } => {
                if reset && array_key.is_some() {
                    return Vec::new();
                }
                if !reset && array_key.is_some() && is_current {
                    mode.up(lines.len(), false);
                }
                let full = array_key
                    .as_ref()
                    .map_or_else(|| name.clone(), |key| format!("{name}[{key}]"));
                vec![customize_unset_command(&full, *target)]
            }
            Item::Key { table, key } | Item::KeyField { table, key } => {
                let Some(binding) = self.keys.get(table, key) else {
                    return Vec::new();
                };
                let default = KeyTables::default().get(table, key).cloned();
                if reset && let Some(default) = &default {
                    if default.commands == binding.commands {
                        return Vec::new();
                    }
                    return vec![customize_bind(
                        table,
                        key,
                        default.repeat,
                        default.note.as_deref(),
                        &customize_command_print(default),
                    )];
                }
                if is_current {
                    if let Some(line) = lines.get(mode.current) {
                        mode.expanded.remove(&rows[*line].id);
                    }
                    mode.up(lines.len(), false);
                }
                vec![CommandInvocation::new(
                    "unbind-key",
                    ["-T".to_owned(), table.clone(), key.clone()],
                )]
            }
        }
    }
}

fn customize_default_value(option: TmuxOption) -> Option<String> {
    thread_local! {
        static DEFAULTS: MuxEngine = MuxEngine::default();
    }
    let target = match option.scope {
        TmuxOptionScope::Server => TmuxOptionTarget::Server,
        TmuxOptionScope::Session => TmuxOptionTarget::GlobalSession,
        TmuxOptionScope::Window | TmuxOptionScope::WindowPane => TmuxOptionTarget::GlobalWindow,
    };
    DEFAULTS.with(|engine| {
        engine
            .tmux_option_readback(option, target, false)
            .ok()
            .flatten()
            .map(|(value, _)| value)
    })
}

fn customize_is_descendant(rows: &[Row], ancestor: &str, id: &str) -> bool {
    let Some(mut row) = rows.iter().find(|row| row.id == id) else {
        return false;
    };
    while let Some(parent) = row.parent {
        if rows[parent].id == ancestor {
            return true;
        }
        row = &rows[parent];
    }
    false
}

fn customize_same_key(left: &Row, right: &Row) -> bool {
    match (&left.item, &right.item) {
        (
            Item::Key { table, key } | Item::KeyField { table, key },
            Item::Key {
                table: other_table,
                key: other_key,
            }
            | Item::KeyField {
                table: other_table,
                key: other_key,
            },
        ) => table == other_table && key == other_key,
        _ => false,
    }
}

fn customize_flat(rows: &[Row], line: usize) -> bool {
    let parent = rows[line].parent;
    let depth = rows[line].depth;
    !rows
        .iter()
        .any(|row| row.parent == parent && row.depth == depth && row.children)
}

fn customize_item_name(row: &Row) -> String {
    match &row.item {
        Item::Option { name, .. } => name.clone(),
        Item::Key { key, .. } | Item::KeyField { key, .. } => key.clone(),
        Item::Section => row.name.clone(),
    }
}

fn customize_command_print(binding: &Binding) -> String {
    binding
        .commands
        .iter()
        .flat_map(|command| match parse_command_alias_group(command) {
            Ok(Some(commands)) => commands.iter().map(tmux_command_print).collect::<Vec<_>>(),
            _ => vec![tmux_command_print(command)],
        })
        .collect::<Vec<_>>()
        .join(" ; ")
}

fn customize_example_style(value: &str) -> String {
    value
        .split([' ', ','])
        .filter(|token| {
            zz_protocol::parse_style(token).is_some_and(|style| {
                style.align.is_none()
                    && style.fill.is_none()
                    && style.list.is_none()
                    && style.range.is_none()
                    && style.width.is_none()
                    && style.pad.is_none()
                    && style.default_type.is_none()
                    && style.ignore.is_none()
                    && style.link.is_none()
            })
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn customize_bind(
    table: &str,
    key: &str,
    repeat: bool,
    note: Option<&str>,
    command: &str,
) -> CommandInvocation {
    let mut args = Vec::new();
    if repeat {
        args.push("-r".to_owned());
    }
    if let Some(note) = note {
        args.extend(["-N".to_owned(), note.to_owned()]);
    }
    args.extend([
        "-T".to_owned(),
        table.to_owned(),
        key.to_owned(),
        command.to_owned(),
    ]);
    CommandInvocation::new("bind-key", args)
}

fn customize_target_args(target: TmuxOptionTarget) -> Vec<String> {
    match target {
        TmuxOptionTarget::Server => vec!["-s".to_owned()],
        TmuxOptionTarget::GlobalSession => vec!["-g".to_owned()],
        TmuxOptionTarget::GlobalWindow => vec!["-gw".to_owned()],
        TmuxOptionTarget::Session(id) => vec!["-t".to_owned(), id.to_string()],
        TmuxOptionTarget::Window(id) => vec!["-w".to_owned(), "-t".to_owned(), id.to_string()],
        TmuxOptionTarget::Pane(id) => vec!["-p".to_owned(), "-t".to_owned(), id.to_string()],
    }
}

fn customize_set_command(name: &str, target: TmuxOptionTarget, value: &str) -> CommandInvocation {
    let mut args = customize_target_args(target);
    args.extend([name.to_owned(), value.to_owned()]);
    CommandInvocation::new("set-option", args)
}

fn customize_unset_command(name: &str, target: TmuxOptionTarget) -> CommandInvocation {
    let mut args = vec!["-u".to_owned()];
    args.extend(customize_target_args(target));
    args.push(name.to_owned());
    CommandInvocation::new("set-option", args)
}

struct PreviewWriter {
    width: usize,
    height: usize,
    lines: Vec<String>,
    x: usize,
    y: usize,
}

impl PreviewWriter {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            lines: Vec::new(),
            x: 0,
            y: 0,
        }
    }

    fn put(&mut self, style: &str, text: &str) {
        if text.is_empty() || self.y >= self.height {
            return;
        }
        while self.lines.len() <= self.y {
            self.lines.push(String::new());
        }
        let line = &mut self.lines[self.y];
        let escaped = text.replace('#', "##");
        if style.is_empty() {
            line.push_str(&escaped);
        } else {
            let _ = write!(line, "#[{style}]{escaped}#[default]");
        }
    }

    fn text(&mut self, more: bool, style: &str, text: &str) -> bool {
        use unicode_width::UnicodeWidthChar as _;
        if self.height == 0 || self.width == 0 {
            return false;
        }
        let characters = text.chars().collect::<Vec<_>>();
        let start_row = self.y;
        let lines = self.height.saturating_sub(start_row);
        let mut index = 0;
        let mut left = self.width.saturating_sub(self.x);
        loop {
            let mut at = 0;
            let mut end = index;
            while end < characters.len() {
                if characters[end] == '\n' {
                    break;
                }
                let width = characters[end].width().unwrap_or(0);
                if at + width > left {
                    break;
                }
                at += width;
                end += 1;
            }
            let next = if end == characters.len() {
                end
            } else if characters[end] == '\n' || characters[end] == ' ' {
                end + 1
            } else {
                match (index + 1..=end)
                    .rev()
                    .find(|position| characters[*position] == ' ')
                {
                    Some(space) => {
                        end = space;
                        space + 1
                    }
                    None => end,
                }
            };
            let piece = characters[index..end].iter().collect::<String>();
            let width = piece
                .chars()
                .map(|character| character.width().unwrap_or(0))
                .sum::<usize>();
            self.put(style, &piece);
            self.x += width;
            index = next;
            if self.y + 1 == start_row + lines || index == characters.len() {
                break;
            }
            self.y += 1;
            self.x = 0;
            left = self.width;
        }
        if (self.y + 1 == start_row + lines && (!more || self.x == self.width))
            || index != characters.len()
        {
            return false;
        }
        if !more || self.x == self.width {
            self.y += 1;
            self.x = 0;
        }
        true
    }

    fn skip_line(&mut self) -> bool {
        self.y += 1;
        self.x = 0;
        self.y + 1 < self.height
    }

    fn skip_line_strict(&mut self) -> bool {
        self.y += 1;
        self.x = 0;
        self.y < self.height
    }

    fn finish(self) -> Vec<String> {
        self.lines
    }
}

#[cfg(test)]
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

    fn engine_with_session() -> (MuxEngine, ExecutionContext, PaneId) {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(
                &mut context,
                &CommandInvocation::new("new-session", ["-s", "customize"]),
            )
            .unwrap();
        let pane = context.pane.unwrap();
        (engine, context, pane)
    }

    fn press(
        engine: &mut MuxEngine,
        context: &mut ExecutionContext,
        pane: PaneId,
        mode: &mut CustomizeMode,
        key: &str,
    ) -> bool {
        let mut expand = customize_expand;
        let result = engine.customize_key(pane, mode, key, &mut expand);
        for command in &result.commands {
            engine.execute(context, command).unwrap();
        }
        engine.customize_finish(pane, mode, &mut expand);
        result.close
    }

    fn current_row(engine: &MuxEngine, pane: PaneId, mode: &CustomizeMode) -> Row {
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, mode, &mut expand);
        let lines = customize_lines(&rows, mode);
        rows[lines[mode.current]].clone()
    }

    fn select(engine: &MuxEngine, pane: PaneId, mode: &mut CustomizeMode, name: &str) {
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, mode, &mut expand);
        mode.current = customize_lines(&rows, mode)
            .iter()
            .position(|line| rows[*line].name == name)
            .unwrap();
    }

    fn type_text(
        engine: &mut MuxEngine,
        context: &mut ExecutionContext,
        pane: PaneId,
        mode: &mut CustomizeMode,
        text: &str,
    ) {
        for character in text.chars() {
            press(engine, context, pane, mode, &character.to_string());
        }
    }

    #[test]
    fn customize_array_edits_preserve_entries_and_fill_the_first_hole() {
        let (mut engine, mut context, pane) = engine_with_session();
        for option in
            tmux_options().filter(|option| option.is_array && !tmux_option_is_hook(option.name))
        {
            let target = match option.scope {
                TmuxOptionScope::Server => TmuxOptionTarget::Server,
                TmuxOptionScope::Session => TmuxOptionTarget::GlobalSession,
                TmuxOptionScope::Window | TmuxOptionScope::WindowPane => {
                    TmuxOptionTarget::GlobalWindow
                }
            };
            let value = match option.name {
                "pane-colours" => "red",
                "command-alias" => "review=display-message review",
                "codepoint-widths" => "U+0041=1",
                _ => "review-value",
            };
            let indexed = format!("{}[100]", option.name);
            engine
                .execute(
                    &mut context,
                    &customize_set_command(&indexed, target, value),
                )
                .unwrap();
            let mut expected = engine.customize_array_values(target, option.name);
            let hole = first_free_array_index(expected.keys()).unwrap();
            let mut mode = CustomizeMode::default();
            mode.expanded
                .extend((0..3).map(|index| format!("options:{index}")));
            select(&engine, pane, &mut mode, option.name);
            press(&mut engine, &mut context, pane, &mut mode, "W");
            assert!(
                matches!(&mode.prompt, Some((prompt, _)) if prompt.label.contains("[+]")),
                "{}",
                option.name
            );
            press(&mut engine, &mut context, pane, &mut mode, "C-u");
            type_text(&mut engine, &mut context, pane, &mut mode, value);
            press(&mut engine, &mut context, pane, &mut mode, "Enter");
            expected.insert(ArrayIndex::Numeric(hole), value.to_owned());
            assert_eq!(
                engine.customize_array_values(target, option.name),
                expected,
                "{} root",
                option.name
            );
            mode.expanded.insert(current_row(&engine, pane, &mode).id);
            select(&engine, pane, &mut mode, &indexed);
            press(&mut engine, &mut context, pane, &mut mode, "Enter");
            let replacement = match option.name {
                "pane-colours" => "blue",
                "codepoint-widths" => "U+0042=2",
                "command-alias" => "changed=display-message changed",
                _ => "changed-value",
            };
            press(&mut engine, &mut context, pane, &mut mode, "C-u");
            type_text(&mut engine, &mut context, pane, &mut mode, replacement);
            press(&mut engine, &mut context, pane, &mut mode, "Enter");
            expected.insert(ArrayIndex::Numeric(100), replacement.to_owned());
            assert_eq!(
                engine.customize_array_values(target, option.name),
                expected,
                "{} child",
                option.name
            );
        }
    }

    #[test]
    fn customize_hides_hook_arrays_like_the_pin() {
        let (engine, _, pane) = engine_with_session();
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &CustomizeMode::default(), &mut expand);
        assert_eq!(
            rows.iter()
                .filter(|row| matches!(
                    &row.item,
                    Item::Option {
                        metadata: Some(option),
                        array_key: None,
                        ..
                    } if option.is_array
                ))
                .count(),
            8
        );
        assert!(!rows.iter().any(|row| tmux_option_is_hook(&row.name)));
    }

    #[test]
    fn customize_only_the_mode_tree_exit_keys_close_the_mode() {
        let (mut engine, mut context, pane) = engine_with_session();
        for key in ["C-c", "C-d", "C-z", "C-j", "Space", "M-<", "M->", "x"] {
            let mut mode = CustomizeMode::default();
            let original = mode.clone();
            assert!(
                !press(&mut engine, &mut context, pane, &mut mode, key),
                "{key}"
            );
            assert_eq!(mode, original, "{key}");
        }
        for key in ["q", "Escape", "C-[", "C-g", "\u{1b}"] {
            assert!(
                press(
                    &mut engine,
                    &mut context,
                    pane,
                    &mut CustomizeMode::default(),
                    key
                ),
                "{key}"
            );
        }
    }

    #[test]
    fn customize_global_edit_keys_keep_an_arrays_existing_scope() {
        let (mut engine, mut context, pane) = engine_with_session();
        for (name, target, value) in [
            (
                "update-environment",
                TmuxOptionTarget::Session(context.session.unwrap()),
                "LOCAL",
            ),
            ("pane-colours", TmuxOptionTarget::Pane(pane), "red"),
            (
                "pane-colours",
                TmuxOptionTarget::Window(context.window.unwrap()),
                "blue",
            ),
        ] {
            engine
                .execute(
                    &mut context,
                    &customize_set_command(&format!("{name}[100]"), target, value),
                )
                .unwrap();
            if matches!(target, TmuxOptionTarget::Window(_)) {
                engine
                    .execute(
                        &mut context,
                        &CommandInvocation::new("set-option", ["-pu", "pane-colours"]),
                    )
                    .unwrap();
            }
            for key in ["S", "W"] {
                let mut mode = CustomizeMode::default();
                mode.expanded
                    .extend((0..3).map(|index| format!("options:{index}")));
                select(&engine, pane, &mut mode, name);
                press(&mut engine, &mut context, pane, &mut mode, key);
                assert!(
                    matches!(&mode.prompt, Some((_, PromptPurpose::Option { target: actual, .. })) if *actual == target),
                    "{name}: {key}"
                );
            }
        }
    }

    #[test]
    fn customize_edits_a_number_and_preserves_the_selected_option() {
        let (mut engine, mut context, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        press(&mut engine, &mut context, pane, &mut mode, "Right");
        select(&engine, pane, &mut mode, "buffer-limit");
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        press(&mut engine, &mut context, pane, &mut mode, "C-u");
        press(&mut engine, &mut context, pane, &mut mode, "7");
        assert!(!press(&mut engine, &mut context, pane, &mut mode, "Enter"));
        let row = current_row(&engine, pane, &mode);
        assert_eq!(row.name, "buffer-limit");
        assert!(matches!(row.item, Item::Option { ref value, .. } if value == "7"));
        assert!(press(&mut engine, &mut context, pane, &mut mode, "q"));
    }

    #[test]
    fn customize_cycles_flags_and_choices_in_the_local_scope() {
        let (mut engine, mut context, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        mode.expanded.insert("options:1".to_owned());
        engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "mouse", "off"]),
            )
            .unwrap();
        for (name, expected) in [("mouse", "on"), ("status-position", "top")] {
            select(&engine, pane, &mut mode, name);
            press(&mut engine, &mut context, pane, &mut mode, "Enter");
            let row = current_row(&engine, pane, &mode);
            assert!(
                matches!(&row.item, Item::Option { value, target, .. }
                    if value == expected && *target == TmuxOptionTarget::Session(context.session.unwrap())),
                "{name}"
            );
        }
    }

    #[test]
    fn customize_tree_keys_follow_mode_tree_key() {
        let (mut engine, mut context, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        press(&mut engine, &mut context, pane, &mut mode, "Right");
        assert!(mode.expanded.contains("options:0"));
        assert_eq!(mode.current, 0);
        press(&mut engine, &mut context, pane, &mut mode, "Right");
        assert_eq!(mode.current, 1);
        press(&mut engine, &mut context, pane, &mut mode, "Left");
        assert_eq!(mode.current, 0);
        assert!(!mode.expanded.contains("options:0"));
        press(&mut engine, &mut context, pane, &mut mode, "Down");
        press(&mut engine, &mut context, pane, &mut mode, "Left");
        assert_eq!(mode.current, 0);
        press(&mut engine, &mut context, pane, &mut mode, "M-+");
        press(&mut engine, &mut context, pane, &mut mode, "Down");
        press(&mut engine, &mut context, pane, &mut mode, "Down");
        press(&mut engine, &mut context, pane, &mut mode, "M--");
        assert_eq!(mode.current, 2);
        press(&mut engine, &mut context, pane, &mut mode, "t");
        assert_eq!(mode.current, 2);
        assert!(mode.tagged.is_empty());
        for expected in [Preview::Off, Preview::Big, Preview::Normal] {
            press(&mut engine, &mut context, pane, &mut mode, "v");
            assert_eq!(mode.preview, expected);
        }
    }

    #[test]
    fn customize_search_wraps_backward_into_key_tables() {
        let (mut engine, mut context, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        for key in ["/", "m", "o", "u", "s", "e", "Enter", "n", "N", "N", "N"] {
            press(&mut engine, &mut context, pane, &mut mode, key);
            let mut expand = customize_expand;
            let _ = engine.customize_presentation(pane, &mode, &mut expand);
        }
        assert!(
            current_row(&engine, pane, &mode)
                .name
                .to_lowercase()
                .contains("mouse")
        );
    }

    #[test]
    fn customize_unset_removes_one_array_index_and_moves_up() {
        let (mut engine, mut context, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        mode.expanded.insert("options:0".to_owned());
        mode.expanded.insert("options:0/command-alias".to_owned());
        select(&engine, pane, &mut mode, "command-alias[0]");
        let before = engine.customize_array_values(TmuxOptionTarget::Server, "command-alias");
        press(&mut engine, &mut context, pane, &mut mode, "u");
        assert!(
            matches!(&mode.prompt, Some((prompt, _)) if prompt.label == "Unset command-alias[0]? ")
        );
        press(&mut engine, &mut context, pane, &mut mode, "y");
        let after = engine.customize_array_values(TmuxOptionTarget::Server, "command-alias");
        assert_eq!(after.len() + 1, before.len());
        assert!(!after.contains_key(&ArrayIndex::Numeric(0)));
        assert_eq!(current_row(&engine, pane, &mode).name, "command-alias");
    }
}
