use std::fmt::Write as _;

use super::mode_prompt::{ModeKey, ModeMouseKey, ModePrompt, PromptHistories, PromptOutcome};
use super::*;
use crate::tmux_option_metadata::TmuxOptionKind;
use zz_protocol::{
    ChooseTreeItem, ChooseTreeState, ChooseTreeTarget, ChooserPresentation, ChooserPreview,
    ChooserPreviewSize, ChooserRow, CommandPromptType,
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
    Environment {
        name: String,
        session: Option<SessionId>,
        hidden: bool,
    },
    AddOption {
        target: TmuxOptionTarget,
        hook: bool,
    },
    AddEnvironment {
        session: Option<SessionId>,
    },
    AddKey {
        table: String,
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
    changed_only: bool,
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

    #[must_use]
    pub fn prompt_kind(&self) -> (&'static str, &'static [&'static str]) {
        match &self.prompt {
            Some((prompt, _)) if prompt.is_single() => ("command", &["SINGLE", "NOFORMAT"]),
            Some((_, PromptPurpose::Search | PromptPurpose::Filter)) => ("search", &["NOFORMAT"]),
            _ => ("command", &["NOFORMAT"]),
        }
    }

    #[must_use]
    pub fn prompt_input(&self) -> String {
        self.prompt
            .as_ref()
            .map(|(prompt, _)| prompt.input())
            .unwrap_or_default()
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
        hook: bool,
        monitor: Option<String>,
    },
    Environment {
        name: String,
        session: Option<SessionId>,
        value: Option<String>,
        hidden: bool,
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

    fn search(&self, label: impl Into<String>, input: &str) -> ModePrompt {
        self.prompt(label, input)
            .with_history_type(CommandPromptType::Search)
    }
}

pub struct CustomizeResult {
    pub close: bool,
    pub commands: Vec<CommandInvocation>,
    pub menu: Option<CustomizeMenu>,
    pub edit: Option<CustomizeEdit>,
    pub stop_on_error: bool,
    pub message: Option<String>,
    pub remembered: Option<(CommandPromptType, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomizeEdit {
    pub value: String,
    kind: EditKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EditKind {
    Option {
        name: String,
        array_key: Option<String>,
        target: TmuxOptionTarget,
    },
    KeyCommand {
        table: String,
        key: String,
    },
    KeyNote {
        table: String,
        key: String,
    },
    Environment {
        name: String,
        session: Option<SessionId>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomizeMenu {
    pub line: usize,
    pub outside: bool,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomizeMenuItem {
    pub name: &'static str,
    pub key: &'static str,
    pub annotation: &'static str,
    pub feed: &'static str,
}

pub const CUSTOMIZE_MENU_ITEMS: [Option<CustomizeMenuItem>; 11] = [
    Some(CustomizeMenuItem {
        name: "Select",
        key: "Enter",
        annotation: "Enter",
        feed: "Enter",
    }),
    Some(CustomizeMenuItem {
        name: "Edit",
        key: "e",
        annotation: "e",
        feed: "e",
    }),
    Some(CustomizeMenuItem {
        name: "Expand",
        key: "Right",
        annotation: "Right",
        feed: "Right",
    }),
    None,
    Some(CustomizeMenuItem {
        name: "Tag",
        key: "t",
        annotation: "t",
        feed: "t",
    }),
    Some(CustomizeMenuItem {
        name: "Tag All",
        key: "[DC4]",
        annotation: "[DC4]",
        feed: "\x14",
    }),
    Some(CustomizeMenuItem {
        name: "Tag None",
        key: "T",
        annotation: "T",
        feed: "T",
    }),
    None,
    Some(CustomizeMenuItem {
        name: "Changed Only",
        key: "C",
        annotation: "C",
        feed: "C",
    }),
    None,
    Some(CustomizeMenuItem {
        name: "Cancel",
        key: "q",
        annotation: "q",
        feed: "q",
    }),
];

pub const CUSTOMIZE_OUTSIDE_MENU_ITEMS: [Option<CustomizeMenuItem>; 4] = [
    Some(CustomizeMenuItem {
        name: "Scroll Left",
        key: "<",
        annotation: "<",
        feed: "<",
    }),
    Some(CustomizeMenuItem {
        name: "Scroll Right",
        key: ">",
        annotation: ">",
        feed: ">",
    }),
    None,
    Some(CustomizeMenuItem {
        name: "Cancel",
        key: "q",
        annotation: "q",
        feed: "q",
    }),
];

#[must_use]
pub fn customize_menu_feed(outside: bool, index: usize) -> Option<&'static str> {
    let items: &[Option<CustomizeMenuItem>] = if outside {
        &CUSTOMIZE_OUTSIDE_MENU_ITEMS
    } else {
        &CUSTOMIZE_MENU_ITEMS
    };
    items.get(index).and_then(|item| item.map(|item| item.feed))
}

impl CustomizeResult {
    fn message(message: String) -> Self {
        Self {
            message: Some(message),
            ..Self::stay()
        }
    }

    const fn stay() -> Self {
        Self {
            close: false,
            commands: Vec::new(),
            menu: None,
            edit: None,
            stop_on_error: false,
            message: None,
            remembered: None,
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

    fn check_selected(&mut self, size: usize) {
        if self.height == 0 {
            return;
        }
        if size <= self.height {
            self.offset = 0;
        } else if self.offset > size - self.height {
            self.offset = size - self.height;
        }
        if self.current < self.offset {
            self.offset = self.current;
        } else if self.current >= self.offset + self.height {
            self.offset = self.current + 1 - self.height;
        }
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
            preview: match options.count("-N") {
                0 => Preview::Normal,
                1 => Preview::Off,
                _ => Preview::Big,
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
        self.customize_height(pane, mode);
        if let Some(found) = tag.and_then(|tag| lines.iter().position(|line| rows[*line].id == tag))
        {
            mode.current = found;
        } else if mode.current >= lines.len() && !lines.is_empty() {
            mode.current = lines.len() - 1;
        }
        mode.check_selected(lines.len());
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
        let session_targets = vec![
            TmuxOptionTarget::Session(window.session),
            TmuxOptionTarget::GlobalSession,
        ];
        let window_targets = vec![
            TmuxOptionTarget::Pane(pane),
            TmuxOptionTarget::Window(window.id),
            TmuxOptionTarget::GlobalWindow,
        ];
        for (index, title, targets) in [
            (0, "Server Options", vec![TmuxOptionTarget::Server]),
            (1, "Session Options", session_targets.clone()),
            (2, "Window & Pane Options", window_targets.clone()),
            (3, "Session Hooks", session_targets),
            (4, "Window & Pane Hooks", window_targets),
        ] {
            self.customize_option_rows(
                &mut rows,
                mode,
                index,
                title,
                &targets,
                format.as_deref(),
                expand,
            );
        }
        if !mode.changed_only {
            self.customize_environment_rows(&mut rows, mode, None, format.as_deref(), expand);
            self.customize_environment_rows(
                &mut rows,
                mode,
                Some(window.session),
                format.as_deref(),
                expand,
            );
        }
        self.customize_key_rows(&mut rows, mode, format.as_deref(), expand);
        for index in 0..rows.len() {
            rows[index].children = rows
                .get(index + 1)
                .is_some_and(|next| next.depth > rows[index].depth);
        }
        rows
    }

    #[allow(clippy::too_many_arguments)]
    fn customize_option_rows(
        &self,
        rows: &mut Vec<Row>,
        mode: &CustomizeMode,
        index: usize,
        title: &str,
        targets: &[TmuxOptionTarget],
        format: Option<&str>,
        expand: &mut CustomizeExpand<'_>,
    ) {
        let hooks = index >= 3;
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
            let mut names = self
                .user_options_at_target(*target)
                .map(|options| options.keys().cloned().collect::<BTreeSet<_>>())
                .unwrap_or_default();
            names.extend(
                self.format_monitors
                    .iter()
                    .filter(|monitor| monitor.target == *target)
                    .map(|monitor| monitor.name.clone()),
            );
            for name in names {
                if !user.contains(&name) {
                    user.push(name);
                }
            }
        }
        let table = tmux_options()
            .filter(|option| tmux_option_is_hook(option.name) == hooks)
            .filter(|option| match option.scope {
                TmuxOptionScope::Server => index == 0,
                TmuxOptionScope::Session => index == 1 || index == 3,
                TmuxOptionScope::Window | TmuxOptionScope::WindowPane => index == 2 || index == 4,
            })
            .map(|option| (option.name.to_owned(), option))
            .collect::<BTreeMap<_, _>>();
        let names = user
            .into_iter()
            .map(|name| (name, None))
            .chain(table.into_iter().map(|(name, option)| (name, Some(option))));
        let mut count = 0;
        for (name, metadata) in names {
            let (owner, value, entries) = self.customize_option_value(targets, &name, metadata);
            if metadata.is_none()
                && (self.hook_events.contains(&name)
                    || self.customize_monitor(owner, &name).is_some())
                    != hooks
            {
                continue;
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
            if mode.changed_only && !customize_option_changed(metadata, &value, &entries, None) {
                continue;
            }
            let array = metadata.is_some_and(|option| option.is_array);
            let scope = self.customize_scope(owner);
            let unit = metadata.map_or("", |option| option.metadata.unit);
            let monitor = metadata
                .is_none()
                .then(|| self.customize_monitor(owner, &name))
                .flatten()
                .map(|monitor| {
                    format_monitor_display(&monitor.name, monitor.scope, &monitor.format)
                });
            let mut vars = BTreeMap::from([
                ("is_option".to_owned(), "1".to_owned()),
                ("is_key".to_owned(), "0".to_owned()),
                ("is_environment".to_owned(), "0".to_owned()),
                ("option_name".to_owned(), name.clone()),
                ("option_is_global".to_owned(), u8::from(global).to_string()),
                ("option_is_array".to_owned(), u8::from(array).to_string()),
                (
                    "option_is_hook".to_owned(),
                    u8::from(hooks && metadata.is_some()).to_string(),
                ),
                (
                    "option_is_monitor".to_owned(),
                    u8::from(monitor.is_some()).to_string(),
                ),
                ("option_scope".to_owned(), scope.clone()),
                ("option_unit".to_owned(), unit.to_owned()),
                (
                    "option_monitor".to_owned(),
                    monitor.clone().unwrap_or_default(),
                ),
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
                format.map_or_else(
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
                    hook: hooks,
                    monitor: monitor.clone(),
                },
            });
            count += 1;
            if array {
                for (key, entry) in &entries {
                    if mode.changed_only
                        && !customize_option_changed(metadata, entry, &entries, Some(key))
                    {
                        continue;
                    }
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
                            array_key: Some(key.clone()),
                            target: owner,
                            metadata,
                            value: entry.clone(),
                            global,
                            hook: hooks,
                            monitor: None,
                        },
                    });
                    count += 1;
                }
            }
        }
        if mode.changed_only && count == 0 {
            rows.truncate(section);
        }
    }

    fn customize_option_value(
        &self,
        targets: &[TmuxOptionTarget],
        name: &str,
        metadata: Option<TmuxOption>,
    ) -> (TmuxOptionTarget, String, Vec<(String, String)>) {
        let last = *targets.last().expect("a section has a target");
        if let Some(option) = metadata
            && tmux_option_is_hook(option.name)
        {
            let owner = targets
                .iter()
                .copied()
                .find(|target| self.hook_array(*target, name).is_some())
                .unwrap_or(last);
            let entries = self.customize_array_entries(owner, name);
            let value = entries
                .iter()
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            return (owner, value, entries);
        }
        let mut owner = last;
        let mut value = None;
        for target in targets {
            let found = if let Some(option) = metadata {
                if option.is_array {
                    self.array_option(*target, name)
                        .map(|values| values.values().cloned().collect::<Vec<_>>().join(" "))
                } else {
                    self.tmux_option_readback(option, *target, false)
                        .ok()
                        .flatten()
                        .map(|(value, _)| value)
                }
            } else {
                self.user_option_at_target(*target, name)
                    .map(ToString::to_string)
                    .or_else(|| self.customize_monitor(*target, name).map(|_| String::new()))
            };
            if let Some(found) = found {
                owner = *target;
                value = Some(found);
                break;
            }
        }
        let array = metadata.is_some_and(|option| option.is_array);
        let entries = if array {
            self.customize_array_entries(owner, name)
        } else {
            Vec::new()
        };
        let value = value.unwrap_or_else(|| {
            metadata
                .and_then(|option| {
                    if option.is_array {
                        Some(
                            entries
                                .iter()
                                .map(|(_, value)| value.as_str())
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
        (owner, value, entries)
    }

    fn customize_option_chain(&self, target: TmuxOptionTarget) -> Vec<TmuxOptionTarget> {
        let window = |pane| {
            self.state
                .window_for_pane(pane)
                .map(TmuxOptionTarget::Window)
        };
        match target {
            TmuxOptionTarget::Pane(pane) => [Some(target), window(pane)]
                .into_iter()
                .flatten()
                .chain([TmuxOptionTarget::GlobalWindow])
                .collect(),
            TmuxOptionTarget::Window(_) => vec![target, TmuxOptionTarget::GlobalWindow],
            TmuxOptionTarget::Session(_) => vec![target, TmuxOptionTarget::GlobalSession],
            _ => vec![target],
        }
    }

    fn customize_holds(&self, name: &str, target: TmuxOptionTarget) -> bool {
        if self.customize_monitor(target, name).is_some()
            || self.user_option_at_target(target, name).is_some()
        {
            return true;
        }
        let Some(option) = tmux_options().find(|option| option.name == name) else {
            return false;
        };
        if tmux_option_is_hook(name) {
            return self.hook_array(target, name).is_some();
        }
        if option.is_array {
            return self.array_option(target, name).is_some();
        }
        self.tmux_option_readback(option, target, false)
            .ok()
            .flatten()
            .is_some()
    }

    fn customize_remove_option(
        &self,
        name: &str,
        target: TmuxOptionTarget,
    ) -> Vec<CommandInvocation> {
        let mut commands = Vec::new();
        if target != TmuxOptionTarget::Server && self.customize_monitor(target, name).is_some() {
            let mut args = vec!["-u".to_owned()];
            args.extend(customize_target_args(target));
            args.extend(["-B".to_owned(), name.to_owned()]);
            commands.push(CommandInvocation::new("set-hook", args));
        }
        commands.push(customize_unset_command(name, target));
        commands
    }

    fn customize_monitor(
        &self,
        target: TmuxOptionTarget,
        name: &str,
    ) -> Option<&FormatMonitorEntry> {
        self.format_monitors
            .iter()
            .find(|monitor| monitor.target == target && monitor.name == name)
    }

    fn customize_array_entries(
        &self,
        target: TmuxOptionTarget,
        name: &str,
    ) -> Vec<(String, String)> {
        if tmux_option_is_hook(name) {
            return self
                .hook_array(target, name)
                .map(|hook| {
                    hook.iter()
                        .map(|(key, commands)| {
                            (
                                key.display(),
                                commands
                                    .iter()
                                    .map(format_command)
                                    .collect::<Vec<_>>()
                                    .join(" ; "),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        self.customize_array_values(target, name)
            .into_iter()
            .map(|(key, value)| (key.display(), value))
            .collect()
    }

    fn customize_environment_entries(
        &self,
        session: Option<SessionId>,
    ) -> Vec<(String, Option<String>, bool)> {
        let collect = |environment: &Environment| {
            environment
                .iter()
                .map(|(name, entry)| {
                    (
                        name.to_string(),
                        entry.value.as_ref().map(ToString::to_string),
                        entry.hidden,
                    )
                })
                .collect::<Vec<_>>()
        };
        match session {
            None => collect(&self.global_environment),
            Some(session) => self
                .session_environments
                .get(&session)
                .map(|retained| collect(&retained.inner.lock()))
                .unwrap_or_default(),
        }
    }

    fn customize_environment_rows(
        &self,
        rows: &mut Vec<Row>,
        mode: &CustomizeMode,
        session: Option<SessionId>,
        format: Option<&str>,
        expand: &mut CustomizeExpand<'_>,
    ) {
        let section = rows.len();
        let (title, section_id) = match session {
            None => ("Global Environment", "environment:global"),
            Some(_) => ("Session Environment", "environment:session"),
        };
        rows.push(Row {
            id: section_id.to_owned(),
            name: title.to_owned(),
            text: None,
            depth: 0,
            parent: None,
            children: false,
            no_tag: true,
            item: Item::Section,
        });
        let scope = session.map_or_else(String::new, |session| {
            self.customize_scope(TmuxOptionTarget::Session(session))
        });
        let mut vars = BTreeMap::from([
            ("is_option".to_owned(), "0".to_owned()),
            ("is_key".to_owned(), "0".to_owned()),
            ("is_environment".to_owned(), "1".to_owned()),
            (
                "environment_is_global".to_owned(),
                u8::from(session.is_none()).to_string(),
            ),
            ("environment_scope".to_owned(), scope),
        ]);
        for (name, value, hidden) in self.customize_environment_entries(session) {
            vars.insert("environment_name".to_owned(), name.clone());
            vars.insert(
                "environment_hidden".to_owned(),
                u8::from(hidden).to_string(),
            );
            vars.insert(
                "environment_removed".to_owned(),
                u8::from(value.is_none()).to_string(),
            );
            vars.insert(
                "environment_value".to_owned(),
                value.clone().unwrap_or_default(),
            );
            if let Some(filter) = &mode.filter
                && !format_true(&expand(filter, &vars))
            {
                continue;
            }
            let (row_name, text) = match &value {
                None => (format!("-{name}"), None),
                Some(value) => (
                    name.clone(),
                    Some(format.map_or_else(
                        || format!("#[fg=themelightgrey]#[ignore]{value}"),
                        |format| expand(format, &vars),
                    )),
                ),
            };
            rows.push(Row {
                id: format!("{section_id}/{name}"),
                name: row_name,
                text,
                depth: 1,
                parent: Some(section),
                children: false,
                no_tag: false,
                item: Item::Environment {
                    name,
                    session,
                    value,
                    hidden,
                },
            });
        }
    }

    fn customize_key_rows(
        &self,
        rows: &mut Vec<Row>,
        mode: &CustomizeMode,
        format: Option<&str>,
        expand: &mut CustomizeExpand<'_>,
    ) {
        let defaults = mode.changed_only.then(KeyTables::default);
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
            let mut count = 0;
            for listed in bindings {
                if let Some(defaults) = &defaults
                    && !customize_key_changed(defaults.get(&table, &listed.key), listed.binding)
                {
                    continue;
                }
                let key = listed.key.clone();
                let mut vars = BTreeMap::from([
                    ("is_option".to_owned(), "0".to_owned()),
                    ("is_key".to_owned(), "1".to_owned()),
                    ("is_environment".to_owned(), "0".to_owned()),
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
                    name: format.map_or_else(|| key.clone(), |format| expand(format, &vars)),
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
                count += 1;
                let command = customize_command_print(listed.binding);
                for (field, text) in [
                    ("Command", format!("#[fg=themelightgrey]#[ignore]{command}")),
                    (
                        "Note",
                        listed
                            .binding
                            .note
                            .as_ref()
                            .map_or_else(String::new, |note| {
                                format!("#[fg=themelightgrey]#[ignore]{note}")
                            }),
                    ),
                    (
                        "Repeat",
                        format!(
                            "#[fg=themelightgrey]#[ignore]{}",
                            if listed.binding.repeat { "on" } else { "off" }
                        ),
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
            if defaults.is_some() && count == 0 {
                rows.truncate(section);
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
        let lines = customize_lines(&rows, mode);
        if let Some(position) = lines.iter().position(|line| rows[*line].id == found) {
            mode.current = position;
            mode.check_selected(lines.len());
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
                Item::Environment { name, session, .. } => {
                    self.customize_draw_environment(name, *session, &mut writer);
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
                prompt_cursor: zz_protocol::PromptCursor::default(),
                prompt_column: 0,
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
            || !writer.value("Repeat: ", if binding.repeat { "on" } else { "off" })
            || !writer.skip_line()
        {
            return;
        }
        let command = customize_command_print(binding);
        if !writer.value("Command: ", &command) {
            return;
        }
        if let Some(default) = KeyTables::default().get(table, key) {
            let default_command = customize_command_print(default);
            if default_command != command {
                writer.value("The default is: ", &default_command);
            }
        }
    }

    fn customize_draw_environment(
        &self,
        name: &str,
        session: Option<SessionId>,
        writer: &mut PreviewWriter,
    ) {
        let entries = self.customize_environment_entries(session);
        let Some((_, value, hidden)) = entries.iter().find(|(entry, _, _)| entry == name) else {
            return;
        };
        let scope = if session.is_none() {
            "global"
        } else {
            "session"
        };
        if !writer.text(
            false,
            "",
            &format!("This is a {scope} environment variable."),
        ) {
            return;
        }
        if *hidden && !writer.text(false, "", "This variable is hidden.") {
            return;
        }
        if !writer.skip_line() {
            return;
        }
        let shown = match value {
            None => writer.text(false, "", "Variable is removed."),
            Some(value) => writer.value("Variable value: ", value),
        };
        if !shown || session.is_none() {
            return;
        }
        let global = self.customize_environment_entries(None);
        let Some((_, parent, _)) = global.iter().find(|(entry, _, _)| entry == name) else {
            return;
        };
        match parent {
            None => writer.text(false, "", "Global variable is removed."),
            Some(parent) => writer.value("Global value: ", parent),
        };
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
            hook,
            monitor,
            ..
        } = &row.item
        else {
            return;
        };
        let is_hook = *hook && metadata.is_some();
        let is_user_hook = *hook && metadata.is_none();
        let fire = monitor
            .as_ref()
            .and_then(|_| self.customize_monitor(*target, name))
            .map_or_else(|| self.hook_fire(*target, name), |monitor| monitor.fire);
        let (space, unit) = match metadata.map(|option| option.metadata.unit) {
            Some(unit) if !unit.is_empty() => (" ", unit),
            _ => ("", ""),
        };
        let description = match metadata {
            Some(option) if !option.metadata.description.is_empty() => option.metadata.description,
            _ if monitor.is_some() => "This hook runs when a monitor changes.",
            _ if is_user_hook => "This hook doesn't have a description.",
            _ => "This option doesn't have a description.",
        };
        if !writer.text(false, "", description) || !writer.skip_line() {
            return;
        }
        let scope = metadata.map_or("user", |option| match option.scope {
            TmuxOptionScope::Server => "server",
            TmuxOptionScope::Session => "session",
            TmuxOptionScope::Window => "window",
            TmuxOptionScope::WindowPane => "window and pane",
        });
        let kind_line = if monitor.is_some() {
            "This is a monitor hook.".to_owned()
        } else if is_user_hook {
            "This is a user hook.".to_owned()
        } else if is_hook {
            format!("This is a {scope} hook.")
        } else {
            format!("This is a {scope} option.")
        };
        if !writer.text(false, "", &kind_line) {
            return;
        }
        if let Some(monitor) = monitor
            && !writer.value("Monitor: ", monitor)
        {
            return;
        }
        if metadata.is_some_and(|option| option.is_array) {
            if is_hook {
                if array_key.is_none() {
                    if writer.text(false, "", "This is an array hook.") {
                        customize_draw_hook_fire(fire, writer);
                    }
                    return;
                }
            } else {
                let line = array_key.as_ref().map_or_else(
                    || "This is an array option.".to_owned(),
                    |key| format!("This is an array option, key {key}."),
                );
                if !writer.text(false, "", &line) || array_key.is_none() {
                    return;
                }
            }
        }
        if !writer.skip_line() {
            return;
        }
        let default_value = metadata
            .filter(|_| array_key.is_none())
            .and_then(customize_default_value)
            .filter(|default| default != value);
        if is_hook || is_user_hook {
            if !writer.value("Hook command: ", &format!("{value}{space}{unit}"))
                || !customize_draw_hook_fire(fire, writer)
            {
                return;
            }
        } else if !writer.value("Option value: ", &format!("{value}{space}{unit}")) {
            return;
        }
        let kind = metadata.map(|option| option.metadata.kind);
        let expanded = expand(value, &BTreeMap::new());
        if matches!(kind, None | Some(TmuxOptionKind::String))
            && expanded != *value
            && !writer.value("This expands to: ", &expanded)
        {
            return;
        }
        if let Some(option) = metadata
            && option.metadata.kind == TmuxOptionKind::Choice
            && !writer.value(
                "Available values are: ",
                &option.metadata.choices.join(", "),
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
            && !writer.value("The default is: ", &format!("{default}{space}{unit}"))
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
            if !writer.value(
                &format!("Window value (from window {index}): "),
                &format!("{parent}{space}{unit}"),
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
            writer.value("Global value: ", &format!("{parent}{space}{unit}"));
        }
    }

    pub fn customize_key(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        name: &str,
        history: PromptHistories<'_>,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        let key = ModeKey::parse(name);
        let Some(lines) = self.customize_ready(pane, mode, expand) else {
            return CustomizeResult {
                close: true,
                commands: Vec::new(),
                menu: None,
                edit: None,
                stop_on_error: false,
                message: None,
                remembered: None,
            };
        };
        if let Some((prompt, _)) = &mut mode.prompt {
            let outcome = prompt.key_with_history(key, history.of(prompt.history_type()));
            let mut remembered = None;
            let (value, purpose) = match outcome {
                PromptOutcome::Done => {
                    let value = prompt.input();
                    remembered = prompt.remembered();
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
            let mut result = self.customize_answer(pane, mode, purpose, value, expand);
            result.remembered = remembered;
            return result;
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
                menu: None,
                edit: None,
                stop_on_error: false,
                message: None,
                remembered: None,
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
        let button = ModeMouseKey::parse(name);
        if x > columns || y > mode.height {
            if button != ModeMouseKey::Down3 {
                return CustomizeResult::stay();
            }
            let line = if mode.offset + y < size {
                mode.offset + y
            } else {
                mode.current
            };
            return self.customize_menu_result(pane, mode, line, true, expand);
        }
        if mode.offset + y >= size {
            if button != ModeMouseKey::Down3 {
                return CustomizeResult::stay();
            }
            return self.customize_menu_result(pane, mode, mode.current, false, expand);
        }
        if matches!(
            button,
            ModeMouseKey::Down1 | ModeMouseKey::Down3 | ModeMouseKey::DoubleClick1
        ) {
            mode.current = mode.offset + y;
        }
        if button == ModeMouseKey::DoubleClick1 {
            return self.customize_tree_key(pane, mode, ModeKey::Char('\r'), size, expand);
        }
        if button != ModeMouseKey::Down3 {
            return CustomizeResult::stay();
        }
        self.customize_menu_result(pane, mode, mode.current, false, expand)
    }

    pub fn customize_menu_choice(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        line: usize,
        name: &str,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        let Some(size) = self.customize_ready(pane, mode, expand) else {
            return CustomizeResult {
                close: true,
                commands: Vec::new(),
                menu: None,
                edit: None,
                stop_on_error: false,
                message: None,
                remembered: None,
            };
        };
        if line >= size {
            return CustomizeResult::stay();
        }
        mode.current = line;
        self.customize_key(pane, mode, name, PromptHistories::default(), expand)
    }

    fn customize_menu_result(
        &self,
        pane: PaneId,
        mode: &CustomizeMode,
        line: usize,
        outside: bool,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        let rows = self.customize_rows(pane, mode, expand);
        let lines = customize_lines(&rows, mode);
        let name = lines
            .get(line)
            .and_then(|index| rows.get(*index))
            .map_or_else(String::new, |row| row.name.clone());
        CustomizeResult {
            close: false,
            commands: Vec::new(),
            menu: Some(CustomizeMenu {
                line,
                outside,
                name,
            }),
            edit: None,
            stop_on_error: false,
            message: None,
            remembered: None,
        }
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
                menu: None,
                edit: None,
                stop_on_error: false,
                message: None,
                remembered: None,
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
                    menu: None,
                    edit: None,
                    stop_on_error: false,
                    message: None,
                    remembered: None,
                };
            }
            ModeKey::Char('e') => {
                if let Some(edit) = self.customize_start_edit(row_at(mode)) {
                    return CustomizeResult {
                        edit: Some(edit),
                        ..CustomizeResult::stay()
                    };
                }
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
                    self.customize_prompt_options(pane).search("(search) ", ""),
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
                        .search("(filter) ", &input),
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
                            self.customize_add_current(pane, mode, &row, &separators);
                            return CustomizeResult::stay();
                        }
                    }
                    Item::Environment { name, session, .. } => {
                        if matches!(key, ModeKey::Char('w')) {
                            return CustomizeResult::stay();
                        }
                        self.customize_set_environment(mode, name, *session, global, &separators);
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
                if matches!(row.item, Item::Section | Item::Environment { .. })
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
            ModeKey::Char('C') => {
                mode.changed_only = !mode.changed_only;
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
            menu: None,
            edit: None,
            stop_on_error: false,
            message: None,
            remembered: None,
        }
    }

    fn customize_start_edit(&self, row: &Row) -> Option<CustomizeEdit> {
        match &row.item {
            Item::Option {
                name,
                array_key,
                target,
                metadata,
                value,
                ..
            } => {
                if metadata.is_some_and(|option| {
                    matches!(
                        option.metadata.kind,
                        TmuxOptionKind::Flag | TmuxOptionKind::Choice
                    )
                }) {
                    return None;
                }
                Some(CustomizeEdit {
                    value: value.clone(),
                    kind: EditKind::Option {
                        name: name.clone(),
                        array_key: array_key.clone(),
                        target: *target,
                    },
                })
            }
            Item::KeyField { table, key } => {
                let binding = self.keys.get(table, key)?;
                let (value, kind) = match row.name.as_str() {
                    "Command" => (
                        customize_command_print(binding),
                        EditKind::KeyCommand {
                            table: table.clone(),
                            key: key.clone(),
                        },
                    ),
                    "Note" => (
                        binding.note.clone().unwrap_or_default(),
                        EditKind::KeyNote {
                            table: table.clone(),
                            key: key.clone(),
                        },
                    ),
                    _ => return None,
                };
                Some(CustomizeEdit { value, kind })
            }
            Item::Environment {
                name,
                session,
                value: Some(value),
                ..
            } => Some(CustomizeEdit {
                value: value.clone(),
                kind: EditKind::Environment {
                    name: name.clone(),
                    session: *session,
                },
            }),
            _ => None,
        }
    }

    pub fn customize_edited(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        edit: &CustomizeEdit,
        value: &str,
        expand: &mut CustomizeExpand<'_>,
    ) -> CustomizeResult {
        let rows = self.customize_rows(pane, mode, expand);
        let lines = customize_lines(&rows, mode);
        let tag = lines.get(mode.current).map(|line| rows[*line].id.clone());
        let command = match &edit.kind {
            EditKind::Option {
                name,
                array_key,
                target,
            } => self.customize_option_set_command(name, array_key.as_deref(), *target, value),
            EditKind::KeyCommand { table, key } => self.keys.get(table, key).map(|binding| {
                customize_bind(table, key, binding.repeat, binding.note.as_deref(), value)
            }),
            EditKind::KeyNote { table, key } => self.keys.get(table, key).map(|binding| {
                customize_bind(
                    table,
                    key,
                    binding.repeat,
                    (!value.is_empty()).then_some(value),
                    &customize_command_print(binding),
                )
            }),
            EditKind::Environment { name, session } => {
                Some(self.customize_environment_set(name, *session, value, false))
            }
        };
        self.customize_commands(pane, mode, command.into_iter().collect(), tag, expand)
    }

    fn customize_option_set_command(
        &self,
        name: &str,
        array_key: Option<&str>,
        target: TmuxOptionTarget,
        value: &str,
    ) -> Option<CommandInvocation> {
        let array = tmux_options().any(|option| option.name == name && option.is_array);
        if !array {
            return Some(customize_set_command(name, target, value));
        }
        let key = match array_key {
            Some(key) => key.to_owned(),
            None if tmux_option_is_hook(name) => {
                let used = self
                    .hook_array(target, name)
                    .map(|hook| hook.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                first_free_array_index(used.iter()).ok()?.to_string()
            }
            None => {
                let values = self.customize_array_values(target, name);
                first_free_array_index(values.keys()).ok()?.to_string()
            }
        };
        Some(customize_set_command(
            &format!("{name}[{key}]"),
            target,
            value,
        ))
    }

    fn customize_environment_set(
        &self,
        name: &str,
        session: Option<SessionId>,
        value: &str,
        hidden: bool,
    ) -> CommandInvocation {
        let hidden = self
            .customize_environment_entries(session)
            .iter()
            .find(|(entry, _, _)| entry == name)
            .map_or(hidden, |(_, _, hidden)| *hidden);
        let mut args = Vec::new();
        if hidden {
            args.push("-h".to_owned());
        }
        args.extend(customize_environment_target(session));
        args.extend([name.to_owned(), value.to_owned()]);
        CommandInvocation::new("set-environment", args)
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

    fn customize_add_current(
        &self,
        pane: PaneId,
        mode: &mut CustomizeMode,
        row: &Row,
        separators: &ModePromptOptions,
    ) {
        let Some(window) = self.state.window_for_pane(pane) else {
            return;
        };
        let session = self.state.windows[&window].session;
        let (label, input, purpose) = match row.name.as_str() {
            "Server Options" => (
                "New user option: ".to_owned(),
                "@",
                PromptPurpose::AddOption {
                    target: TmuxOptionTarget::Server,
                    hook: false,
                },
            ),
            "Session Options" | "Session Hooks" => {
                let hook = row.name == "Session Hooks";
                (
                    format!("New user {}: ", if hook { "hook" } else { "option" }),
                    "@",
                    PromptPurpose::AddOption {
                        target: TmuxOptionTarget::Session(session),
                        hook,
                    },
                )
            }
            "Window & Pane Options" | "Window & Pane Hooks" => {
                let hook = row.name == "Window & Pane Hooks";
                (
                    format!("New user {}: ", if hook { "hook" } else { "option" }),
                    "@",
                    PromptPurpose::AddOption {
                        target: TmuxOptionTarget::Pane(pane),
                        hook,
                    },
                )
            }
            "Global Environment" => (
                "New environment: ".to_owned(),
                "",
                PromptPurpose::AddEnvironment { session: None },
            ),
            "Session Environment" => (
                "New environment: ".to_owned(),
                "",
                PromptPurpose::AddEnvironment {
                    session: Some(session),
                },
            ),
            name => {
                let Some(table) = name.strip_prefix("Key Table - ") else {
                    return;
                };
                (
                    format!("New key in {table}: "),
                    "",
                    PromptPurpose::AddKey {
                        table: table.to_owned(),
                    },
                )
            }
        };
        mode.prompt = Some((separators.prompt(label, input), purpose));
    }

    fn customize_set_environment(
        &self,
        mode: &mut CustomizeMode,
        name: &str,
        session: Option<SessionId>,
        global: bool,
        separators: &ModePromptOptions,
    ) {
        let entries = self.customize_environment_entries(session);
        let Some((_, value, hidden)) = entries.iter().find(|(entry, _, _)| entry == name) else {
            return;
        };
        let session = if global { None } else { session };
        let label = match session {
            Some(session) => format!(
                "({name}, for {}) ",
                self.customize_scope(TmuxOptionTarget::Session(session))
            ),
            None => format!("({name}, global) "),
        };
        mode.prompt = Some((
            separators.prompt(label, value.as_deref().unwrap_or_default()),
            PromptPurpose::Environment {
                name: name.to_owned(),
                session,
                hidden: *hidden,
            },
        ));
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
                let Some(command) =
                    self.customize_option_set_command(&name, array_key.as_deref(), target, &value)
                else {
                    return CustomizeResult::stay();
                };
                self.customize_commands(pane, mode, vec![command], tag, expand)
            }
            PromptPurpose::ArrayKey {
                name,
                array_key,
                target,
            } => {
                let Some(new_key) = value.filter(|value| !value.is_empty()) else {
                    return CustomizeResult::stay();
                };
                let Some(new_key) = customize_array_key(&new_key) else {
                    return CustomizeResult::message(format!("Bad array key: {new_key}"));
                };
                let values = self.customize_array_entries(target, &name);
                if values.iter().any(|(key, _)| *key == new_key) {
                    return CustomizeResult::stay();
                }
                let Some(entry) = values
                    .iter()
                    .find(|(key, _)| *key == array_key)
                    .map(|(_, entry)| entry.clone())
                else {
                    return CustomizeResult::stay();
                };
                let mut result = self.customize_commands(
                    pane,
                    mode,
                    vec![
                        customize_set_command(&format!("{name}[{new_key}]"), target, &entry),
                        customize_unset_command(&format!("{name}[{array_key}]"), target),
                    ],
                    tag,
                    expand,
                );
                result.stop_on_error = true;
                result
            }
            PromptPurpose::Environment {
                name,
                session,
                hidden,
            } => {
                let Some(value) = value else {
                    return CustomizeResult::stay();
                };
                let command = self.customize_environment_set(&name, session, &value, hidden);
                self.customize_commands(pane, mode, vec![command], tag, expand)
            }
            PromptPurpose::AddOption { target, hook } => {
                let Some(value) = value.filter(|value| !value.is_empty()) else {
                    return CustomizeResult::stay();
                };
                match customize_add_option(&value, target, hook) {
                    Ok(command) => self.customize_commands(pane, mode, vec![command], tag, expand),
                    Err(message) => CustomizeResult::message(message),
                }
            }
            PromptPurpose::AddEnvironment { session } => {
                let Some(value) = value.filter(|value| !value.is_empty()) else {
                    return CustomizeResult::stay();
                };
                match customize_add_environment(&value, session) {
                    Ok(command) => self.customize_commands(pane, mode, vec![command], tag, expand),
                    Err(message) => CustomizeResult::message(message),
                }
            }
            PromptPurpose::AddKey { table } => {
                let Some(value) = value.filter(|value| !value.is_empty()) else {
                    return CustomizeResult::stay();
                };
                match customize_add_key(&value, &table) {
                    Ok(command) => self.customize_commands(pane, mode, vec![command], tag, expand),
                    Err(message) => CustomizeResult::message(message),
                }
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
            Item::Environment { name, session, .. } => {
                if reset {
                    return Vec::new();
                }
                if is_current {
                    mode.up(lines.len(), false);
                }
                let mut args = vec!["-u".to_owned()];
                args.extend(customize_environment_target(*session));
                args.push(name.clone());
                vec![CommandInvocation::new("set-environment", args)]
            }
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
                if let Some(key) = array_key {
                    return vec![customize_unset_command(&format!("{name}[{key}]"), *target)];
                }
                if !reset {
                    return self.customize_remove_option(name, *target);
                }
                self.customize_option_chain(*target)
                    .into_iter()
                    .filter(|target| self.customize_holds(name, *target))
                    .flat_map(|target| self.customize_remove_option(name, target))
                    .collect()
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

fn customize_draw_hook_fire(fire: HookFire, writer: &mut PreviewWriter) -> bool {
    if fire.time != 0 {
        let now = i64::try_from(unix_seconds()).unwrap_or(i64::MAX);
        let time = crate::formats::pretty_time(i64::try_from(fire.time).unwrap_or(i64::MAX), now);
        return writer.text(
            false,
            "",
            &format!(
                "This hook has been fired {} times, last {time}.",
                fire.count
            ),
        );
    }
    writer.text(
        false,
        "",
        &format!("This hook has been fired {} times.", fire.count),
    )
}

fn customize_option_changed(
    metadata: Option<TmuxOption>,
    value: &str,
    entries: &[(String, String)],
    key: Option<&str>,
) -> bool {
    let Some(option) = metadata else {
        return true;
    };
    if tmux_option_is_hook(option.name) {
        return key.is_some() || !entries.is_empty();
    }
    if option.is_array {
        let defaults = customize_default_array(option);
        if let Some(key) = key {
            let default = defaults
                .iter()
                .find(|(default_key, _)| default_key == key)
                .map(|(_, value)| value.as_str());
            return default != Some(value);
        }
        let printed = |entries: &[(String, String)]| {
            entries
                .iter()
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        return printed(entries) != printed(&defaults);
    }
    customize_default_value(option).is_none_or(|default| default != value)
}

fn customize_default_array(option: TmuxOption) -> Vec<(String, String)> {
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
            .customize_array_values(target, option.name)
            .into_iter()
            .map(|(key, value)| (key.display(), value))
            .collect()
    })
}

fn customize_key_changed(default: Option<&Binding>, binding: &Binding) -> bool {
    default.is_none_or(|default| {
        default.repeat != binding.repeat
            || default.note != binding.note
            || customize_command_print(default) != customize_command_print(binding)
    })
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
        Item::Option { name, .. } | Item::Environment { name, .. } => name.clone(),
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

fn customize_array_key(key: &str) -> Option<String> {
    if key.contains(']') {
        return None;
    }
    if !key.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(key.to_owned());
    }
    key.parse::<u32>().ok().map(|index| index.to_string())
}

fn customize_split_pair(value: &str) -> Option<(&str, &str)> {
    let end = value.find([' ', '\t'])?;
    let rest = value[end..].trim_start_matches([' ', '\t']);
    (end != 0 && !rest.is_empty()).then(|| (&value[..end], rest))
}

fn customize_add_option(
    value: &str,
    target: TmuxOptionTarget,
    hook: bool,
) -> Result<CommandInvocation, String> {
    let Some((name, value)) = customize_split_pair(value) else {
        return Err("User option must be @name value".to_owned());
    };
    if !name.starts_with('@') || name.contains('[') {
        return Err(format!(
            "User {} name must start with @",
            if hook { "hook" } else { "option" }
        ));
    }
    if !hook {
        return Ok(customize_set_command(name, target, value));
    }
    let mut args = customize_target_args(target);
    args.extend([name.to_owned(), value.to_owned()]);
    Ok(CommandInvocation::new("set-hook", args))
}

fn customize_add_environment(
    value: &str,
    session: Option<SessionId>,
) -> Result<CommandInvocation, String> {
    let mut args = customize_environment_target(session);
    if let Some(name) = value.strip_prefix('-') {
        if name.is_empty() || name.contains('=') {
            return Err(format!("Bad environment variable: {value}"));
        }
        args.extend(["-r".to_owned(), name.to_owned()]);
    } else {
        let Some((name, value)) = value.split_once('=').filter(|(name, _)| !name.is_empty()) else {
            return Err("Environment variable must be NAME=value".to_owned());
        };
        args.extend([name.to_owned(), value.to_owned()]);
    }
    Ok(CommandInvocation::new("set-environment", args))
}

fn customize_add_key(value: &str, table: &str) -> Result<CommandInvocation, String> {
    let Some((key, command)) = customize_split_pair(value) else {
        return Err("Key binding must be key command".to_owned());
    };
    if parse_tmux_key_details(key).is_none() {
        return Err(format!("Unknown key: {key}"));
    }
    Ok(CommandInvocation::new(
        "bind-key",
        [
            "-T".to_owned(),
            table.to_owned(),
            key.to_owned(),
            command.to_owned(),
        ],
    ))
}

fn customize_environment_target(session: Option<SessionId>) -> Vec<String> {
    match session {
        None => vec!["-g".to_owned()],
        Some(session) => vec!["-t".to_owned(), session.to_string()],
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

    fn value(&mut self, label: &str, value: &str) -> bool {
        if self.y >= self.height || !self.text(true, "", label) {
            return false;
        }
        if self.y >= self.height {
            return false;
        }
        self.text(false, "fg=themelightgrey", value)
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
        let result = engine.customize_key(pane, mode, key, PromptHistories::default(), &mut expand);
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
    fn customize_lists_hooks_and_environment_in_their_own_sections_like_3_8() {
        let (engine, _, pane) = engine_with_session();
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &CustomizeMode::default(), &mut expand);
        let sections = rows
            .iter()
            .filter(|row| row.parent.is_none())
            .map(|row| row.name.as_str())
            .take(7)
            .collect::<Vec<_>>();
        assert_eq!(
            sections,
            [
                "Server Options",
                "Session Options",
                "Window & Pane Options",
                "Session Hooks",
                "Window & Pane Hooks",
                "Global Environment",
                "Session Environment",
            ]
        );
        let in_section = |title: &str| {
            let section = rows.iter().position(|row| row.name == title).unwrap();
            rows.iter()
                .filter(|row| row.parent == Some(section))
                .collect::<Vec<_>>()
        };
        let options = ["Server Options", "Session Options", "Window & Pane Options"]
            .into_iter()
            .flat_map(in_section)
            .collect::<Vec<_>>();
        assert_eq!(
            options
                .iter()
                .filter(|row| matches!(
                    &row.item,
                    Item::Option { metadata: Some(option), .. } if option.is_array
                ))
                .count(),
            8
        );
        assert!(!options.iter().any(|row| tmux_option_is_hook(&row.name)));
        let session_hooks = in_section("Session Hooks");
        let window_hooks = in_section("Window & Pane Hooks");
        assert!(
            session_hooks
                .iter()
                .any(|row| row.name == "after-new-session")
        );
        assert!(window_hooks.iter().any(|row| row.name == "pane-exited"));
        assert!(window_hooks.iter().any(|row| row.name == "window-renamed"));
        assert_eq!(
            session_hooks.len() + window_hooks.len(),
            tmux_options()
                .filter(|option| tmux_option_is_hook(option.name))
                .count()
        );
        assert!(
            session_hooks
                .iter()
                .chain(&window_hooks)
                .all(
                    |row| matches!(row.item, Item::Option { hook: true, .. }) && row.text.is_none()
                )
        );
    }

    fn preview_text(engine: &MuxEngine, pane: PaneId, mode: &CustomizeMode) -> String {
        let mut expand = customize_expand;
        let (_, presentation, _) = engine.customize_presentation(pane, mode, &mut expand);
        match presentation.preview {
            Some(ChooserPreview::Markup { lines }) => lines.join("\n"),
            _ => String::new(),
        }
    }

    fn run(engine: &mut MuxEngine, context: &mut ExecutionContext, name: &str, args: &[&str]) {
        engine
            .execute(context, &CommandInvocation::new(name, args.iter().copied()))
            .unwrap();
    }

    fn output(
        engine: &mut MuxEngine,
        context: &mut ExecutionContext,
        name: &str,
        args: &[&str],
    ) -> String {
        engine
            .execute(context, &CommandInvocation::new(name, args.iter().copied()))
            .unwrap()
            .output
            .to_string()
    }

    #[test]
    fn customize_edits_hook_arrays_and_counts_their_fires_in_the_preview() {
        let (mut engine, mut context, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        mode.expanded.insert("options:3".to_owned());
        select(&engine, pane, &mut mode, "after-new-window");
        let preview = preview_text(&engine, pane, &mode);
        assert!(preview.contains("This is a session hook."), "{preview}");
        assert!(preview.contains("This is an array hook."), "{preview}");
        assert!(
            preview.contains("This hook has been fired 0 times."),
            "{preview}"
        );
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        assert_eq!(
            mode.prompt
                .as_ref()
                .map(|(prompt, _)| prompt.label.as_str()),
            Some("(after-new-window[+], global) ")
        );
        type_text(
            &mut engine,
            &mut context,
            pane,
            &mut mode,
            "set-option -g @fired yes",
        );
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        assert_eq!(
            output(
                &mut engine,
                &mut context,
                "show-hooks",
                &["-g", "after-new-window"]
            ),
            "after-new-window[0] set-option -g @fired yes"
        );
        assert_eq!(current_row(&engine, pane, &mode).name, "after-new-window");
        press(&mut engine, &mut context, pane, &mut mode, "Right");
        press(&mut engine, &mut context, pane, &mut mode, "Down");
        assert_eq!(
            current_row(&engine, pane, &mode).name,
            "after-new-window[0]"
        );
        let preview = preview_text(&engine, pane, &mode);
        assert!(
            preview.contains("Hook command: #[fg=themelightgrey]set-option -g @fired yes"),
            "{preview}"
        );
        engine.count_hook_fire(
            Some(TmuxOptionTarget::GlobalSession),
            "after-new-window",
            unix_seconds(),
        );
        let preview = preview_text(&engine, pane, &mode);
        assert!(
            preview.contains("This hook has been fired 1 times, last "),
            "{preview}"
        );
        press(&mut engine, &mut context, pane, &mut mode, "a");
        press(&mut engine, &mut context, pane, &mut mode, "C-u");
        type_text(&mut engine, &mut context, pane, &mut mode, "5");
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        assert_eq!(
            output(
                &mut engine,
                &mut context,
                "show-hooks",
                &["-g", "after-new-window"]
            ),
            "after-new-window[5] set-option -g @fired yes"
        );
        select(&engine, pane, &mut mode, "after-new-window[5]");
        press(&mut engine, &mut context, pane, &mut mode, "u");
        assert_eq!(
            mode.prompt
                .as_ref()
                .map(|(prompt, _)| prompt.label.as_str()),
            Some("Unset after-new-window[5]? ")
        );
        assert_eq!(mode.prompt_kind(), ("command", &["SINGLE", "NOFORMAT"][..]));
        press(&mut engine, &mut context, pane, &mut mode, "y");
        assert_eq!(
            output(
                &mut engine,
                &mut context,
                "show-hooks",
                &["-g", "after-new-window"]
            ),
            "after-new-window"
        );
    }

    fn press_lenient(
        engine: &mut MuxEngine,
        context: &mut ExecutionContext,
        pane: PaneId,
        mode: &mut CustomizeMode,
        key: &str,
    ) -> CustomizeResult {
        let mut expand = customize_expand;
        let result = engine.customize_key(pane, mode, key, PromptHistories::default(), &mut expand);
        for command in &result.commands {
            if engine.execute(context, command).is_err() && result.stop_on_error {
                break;
            }
        }
        engine.customize_finish(pane, mode, &mut expand);
        result
    }

    fn rename_hook_key(new_key: &str) -> (String, CustomizeResult) {
        let (mut engine, mut context, pane) = engine_with_session();
        run(
            &mut engine,
            &mut context,
            "set-hook",
            &["-g", "after-new-window", "display-message kept"],
        );
        let mut mode = CustomizeMode::default();
        mode.expanded.insert("options:3".to_owned());
        mode.expanded
            .insert("options:3/after-new-window".to_owned());
        select(&engine, pane, &mut mode, "after-new-window[0]");
        press(&mut engine, &mut context, pane, &mut mode, "a");
        press(&mut engine, &mut context, pane, &mut mode, "C-u");
        type_text(&mut engine, &mut context, pane, &mut mode, new_key);
        let result = press_lenient(&mut engine, &mut context, pane, &mut mode, "Enter");
        (
            output(
                &mut engine,
                &mut context,
                "show-hooks",
                &["-g", "after-new-window"],
            ),
            result,
        )
    }

    #[test]
    fn customize_array_key_rename_keeps_the_entry_when_the_new_key_is_invalid() {
        let (hooks, result) = rename_hook_key("4294967296");
        assert_eq!(hooks, "after-new-window[0] display-message kept");
        assert!(result.commands.is_empty(), "{:?}", result.commands);
        assert_eq!(result.message.as_deref(), Some("Bad array key: 4294967296"));
    }

    #[test]
    fn customize_array_key_rename_sees_an_occupied_key_in_any_spelling() {
        let (hooks, result) = rename_hook_key("00");
        assert_eq!(hooks, "after-new-window[0] display-message kept");
        assert!(result.commands.is_empty(), "{:?}", result.commands);
        assert_eq!(result.message, None);
        let (hooks, _) = rename_hook_key("007");
        assert_eq!(hooks, "after-new-window[7] display-message kept");
    }

    #[test]
    fn customize_array_key_rename_refuses_a_key_the_command_line_cannot_spell() {
        let (hooks, result) = rename_hook_key("a]b");
        assert_eq!(hooks, "after-new-window[0] display-message kept");
        assert!(result.commands.is_empty(), "{:?}", result.commands);
        assert_eq!(result.message.as_deref(), Some("Bad array key: a]b"));
    }

    #[test]
    fn customize_prompts_walk_the_shared_history_of_their_type() {
        let (engine, _, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        let mut expand = customize_expand;
        let command = ["set -g @x 1".to_owned()];
        let search = ["status".to_owned()];
        let history = PromptHistories {
            command: &command,
            search: &search,
        };
        engine.customize_key(pane, &mut mode, "/", history, &mut expand);
        engine.customize_key(pane, &mut mode, "Up", history, &mut expand);
        assert_eq!(
            mode.prompt.as_ref().map(|(prompt, _)| prompt.input()),
            Some("status".to_owned())
        );
        let result = engine.customize_key(pane, &mut mode, "Enter", history, &mut expand);
        assert!(mode.prompt.is_none());
        assert_eq!(
            result.remembered,
            Some((CommandPromptType::Search, "status".to_owned()))
        );
        engine.customize_key(pane, &mut mode, "f", history, &mut expand);
        engine.customize_key(pane, &mut mode, "C-p", history, &mut expand);
        assert_eq!(
            mode.prompt.as_ref().map(|(prompt, _)| prompt.input()),
            Some("status".to_owned()),
            "the filter prompt is a search prompt too"
        );
        let result = engine.customize_key(pane, &mut mode, "Escape", history, &mut expand);
        assert_eq!(result.remembered, None);
    }

    #[test]
    fn customize_add_prompt_errors_are_messages_not_commands() {
        let (engine, _, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        let mut expand = customize_expand;
        for (purpose, value, message) in [
            (
                PromptPurpose::AddOption {
                    target: TmuxOptionTarget::GlobalSession,
                    hook: false,
                },
                "mine",
                "User option must be @name value",
            ),
            (
                PromptPurpose::AddOption {
                    target: TmuxOptionTarget::GlobalSession,
                    hook: true,
                },
                "mine value",
                "User hook name must start with @",
            ),
            (
                PromptPurpose::AddEnvironment { session: None },
                "-",
                "Bad environment variable: -",
            ),
            (
                PromptPurpose::AddEnvironment { session: None },
                "NOVALUE",
                "Environment variable must be NAME=value",
            ),
            (
                PromptPurpose::AddKey {
                    table: "root".to_owned(),
                },
                "C-a",
                "Key binding must be key command",
            ),
            (
                PromptPurpose::AddKey {
                    table: "root".to_owned(),
                },
                "Nope display-message x",
                "Unknown key: Nope",
            ),
        ] {
            let result = engine.customize_answer(
                pane,
                &mut mode,
                purpose,
                Some(value.to_owned()),
                &mut expand,
            );
            assert!(result.commands.is_empty(), "{value}: {:?}", result.commands);
            assert_eq!(result.message.as_deref(), Some(message), "{value}");
        }
    }

    #[test]
    fn customize_array_key_rename_stops_on_error() {
        let (mut engine, mut context, pane) = engine_with_session();
        run(
            &mut engine,
            &mut context,
            "set-hook",
            &["-g", "after-new-window", "display-message kept"],
        );
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        let mut expand = customize_expand;
        let result = engine.customize_answer(
            pane,
            &mut mode,
            PromptPurpose::ArrayKey {
                name: "after-new-window".to_owned(),
                array_key: "0".to_owned(),
                target: TmuxOptionTarget::GlobalSession,
            },
            Some("1".to_owned()),
            &mut expand,
        );
        assert!(result.stop_on_error);
        assert_eq!(
            result.commands,
            [
                customize_set_command(
                    "after-new-window[1]",
                    TmuxOptionTarget::GlobalSession,
                    "display-message kept"
                ),
                customize_unset_command("after-new-window[0]", TmuxOptionTarget::GlobalSession),
            ]
        );
    }

    #[test]
    fn customize_clearing_a_hidden_variable_keeps_it_hidden() {
        let (mut engine, mut context, pane) = engine_with_session();
        run(
            &mut engine,
            &mut context,
            "set-environment",
            &["-h", "SECRET", "one"],
        );
        let mut mode = CustomizeMode::default();
        mode.expanded.insert("environment:session".to_owned());
        select(&engine, pane, &mut mode, "Session Environment");
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        type_text(&mut engine, &mut context, pane, &mut mode, "-SECRET");
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        select(&engine, pane, &mut mode, "-SECRET");
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        type_text(&mut engine, &mut context, pane, &mut mode, "two");
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        assert_eq!(
            output(
                &mut engine,
                &mut context,
                "show-environment",
                &["-h", "SECRET"]
            ),
            "SECRET=two"
        );
        assert!(!output(&mut engine, &mut context, "show-environment", &[]).contains("SECRET"));
    }

    #[test]
    fn customize_unset_removes_a_monitor_with_its_option() {
        let (mut engine, mut context, pane) = engine_with_session();
        run(
            &mut engine,
            &mut context,
            "set-hook",
            &["-g", "-B", "@mon:%*:#{pane_id}"],
        );
        let mut mode = CustomizeMode::default();
        mode.expanded.insert("options:3".to_owned());
        select(&engine, pane, &mut mode, "@mon");
        press_lenient(&mut engine, &mut context, pane, &mut mode, "u");
        press_lenient(&mut engine, &mut context, pane, &mut mode, "y");
        assert!(engine.format_monitors.is_empty());
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        assert!(!rows.iter().any(|row| row.name == "@mon"));
    }

    #[test]
    fn customize_reset_clears_a_hook_at_every_level_it_is_defined() {
        let (mut engine, mut context, pane) = engine_with_session();
        run(
            &mut engine,
            &mut context,
            "set-hook",
            &["-g", "after-new-window", "display-message global"],
        );
        run(
            &mut engine,
            &mut context,
            "set-hook",
            &[
                "-t",
                "customize",
                "after-new-window",
                "display-message local",
            ],
        );
        let mut mode = CustomizeMode::default();
        mode.expanded.insert("options:3".to_owned());
        select(&engine, pane, &mut mode, "after-new-window");
        assert!(matches!(
            current_row(&engine, pane, &mode).item,
            Item::Option {
                target: TmuxOptionTarget::Session(_),
                ..
            }
        ));
        press_lenient(&mut engine, &mut context, pane, &mut mode, "d");
        press_lenient(&mut engine, &mut context, pane, &mut mode, "y");
        assert_eq!(
            output(
                &mut engine,
                &mut context,
                "show-hooks",
                &["-g", "after-new-window"]
            ),
            "after-new-window"
        );
        assert_eq!(
            output(
                &mut engine,
                &mut context,
                "show-hooks",
                &["-t", "customize", "after-new-window"]
            ),
            ""
        );
    }

    #[test]
    fn customize_classifies_a_user_option_by_its_effective_entry() {
        let (mut engine, mut context, pane) = engine_with_session();
        run(
            &mut engine,
            &mut context,
            "set-hook",
            &["-g", "-B", "@mon:%*:#{pane_id}"],
        );
        run(
            &mut engine,
            &mut context,
            "set-option",
            &["-t", "customize", "@mon", "plain"],
        );
        let mut mode = CustomizeMode::default();
        for section in 0..5 {
            mode.expanded.insert(format!("options:{section}"));
        }
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let sections = rows
            .iter()
            .filter(|row| row.name == "@mon")
            .map(|row| rows[row.parent.unwrap()].name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(sections, ["Session Options"]);
    }

    #[test]
    fn customize_changed_only_compares_an_arrays_printed_values() {
        let (mut engine, mut context, pane) = engine_with_session();
        let values =
            engine.customize_array_entries(TmuxOptionTarget::GlobalSession, "update-environment");
        let (last_key, last_value) = values.last().cloned().unwrap();
        run(
            &mut engine,
            &mut context,
            "set-option",
            &["-g", "update-environment[100]", &last_value],
        );
        run(
            &mut engine,
            &mut context,
            "set-option",
            &["-gu", &format!("update-environment[{last_key}]")],
        );
        let mode = CustomizeMode {
            changed_only: true,
            ..CustomizeMode::default()
        };
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        assert!(!rows.iter().any(|row| row.name == "update-environment"));
    }

    #[test]
    fn customize_environment_rows_resolve_the_3_8_context_formats() {
        let (mut engine, mut context, pane) = engine_with_session();
        run(
            &mut engine,
            &mut context,
            "set-environment",
            &["-g", "ZZTEST_A", "one"],
        );
        run(
            &mut engine,
            &mut context,
            "set-environment",
            &["-g", "-r", "ZZTEST_R"],
        );
        run(
            &mut engine,
            &mut context,
            "set-environment",
            &["-h", "ZZTEST_H", "secret"],
        );
        let mut mode = CustomizeMode {
            filter: Some("#{&&:#{is_environment},#{m:ZZTEST*,#{environment_name}}}".to_owned()),
            format: Some(
                "#{environment_name}=#{environment_value}/#{environment_is_global}/#{environment_hidden}/#{environment_removed}/#{environment_scope}/#{is_option}#{is_key}".to_owned(),
            ),
            ..CustomizeMode::default()
        };
        mode.expanded.insert("environment:global".to_owned());
        mode.expanded.insert("environment:session".to_owned());
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let environment = rows
            .iter()
            .filter(|row| matches!(row.item, Item::Environment { .. }))
            .map(|row| (row.name.as_str(), row.text.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            environment,
            [
                ("ZZTEST_A", Some("ZZTEST_A=one/1/0/0//00")),
                ("-ZZTEST_R", None),
                (
                    "ZZTEST_H",
                    Some("ZZTEST_H=secret/0/1/0/session customize/00")
                ),
            ]
        );
        mode.filter = None;
        mode.format = None;
        select(&engine, pane, &mut mode, "ZZTEST_H");
        let preview = preview_text(&engine, pane, &mode);
        assert!(
            preview.contains("This is a session environment variable."),
            "{preview}"
        );
        assert!(preview.contains("This variable is hidden."), "{preview}");
        assert!(
            preview.contains("Variable value: #[fg=themelightgrey]secret"),
            "{preview}"
        );
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        assert_eq!(
            mode.prompt
                .as_ref()
                .map(|(prompt, _)| prompt.label.as_str()),
            Some("(ZZTEST_H, for session customize) ")
        );
        press(&mut engine, &mut context, pane, &mut mode, "C-u");
        type_text(&mut engine, &mut context, pane, &mut mode, "changed");
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        assert_eq!(
            output(
                &mut engine,
                &mut context,
                "show-environment",
                &["-h", "ZZTEST_H"]
            ),
            "ZZTEST_H=changed"
        );
        select(&engine, pane, &mut mode, "Global Environment");
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        assert_eq!(
            mode.prompt
                .as_ref()
                .map(|(prompt, _)| prompt.label.as_str()),
            Some("New environment: ")
        );
        type_text(
            &mut engine,
            &mut context,
            pane,
            &mut mode,
            "ZZTEST_NEW=fresh",
        );
        press(&mut engine, &mut context, pane, &mut mode, "Enter");
        assert_eq!(
            output(
                &mut engine,
                &mut context,
                "show-environment",
                &["-g", "ZZTEST_NEW"]
            ),
            "ZZTEST_NEW=fresh"
        );
    }

    #[test]
    fn customize_hook_and_monitor_rows_resolve_the_3_8_context_formats() {
        let (mut engine, mut context, pane) = engine_with_session();
        run(
            &mut engine,
            &mut context,
            "set-hook",
            &["-g", "-B", "@mon:%*:#{pane_id}"],
        );
        run(
            &mut engine,
            &mut context,
            "set-hook",
            &["-g", "@event", "display-message hi"],
        );
        let mut mode = CustomizeMode {
            filter: Some("#{||:#{option_is_hook},#{option_is_monitor}}".to_owned()),
            format: Some(
                "#{option_name}:#{option_is_hook}:#{option_is_monitor}:#{option_monitor}:#{is_environment}".to_owned(),
            ),
            ..CustomizeMode::default()
        };
        for section in 0..5 {
            mode.expanded.insert(format!("options:{section}"));
        }
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let texts = rows
            .iter()
            .filter(|row| matches!(row.item, Item::Option { .. }))
            .filter_map(|row| row.text.as_deref())
            .collect::<Vec<_>>();
        assert!(
            texts.contains(&"@mon:0:1:@mon:%*:#{pane_id}:0"),
            "{texts:?}"
        );
        assert!(
            rows.iter()
                .any(|row| row.name == "after-new-session" && row.text.is_none())
        );
        assert!(!rows.iter().any(|row| row.name == "@event"));
        mode.filter = None;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let event = rows.iter().find(|row| row.name == "@event").unwrap();
        assert_eq!(event.text.as_deref(), Some("@event:0:0::0"));
        let in_hooks = |name: &str| {
            let row = rows.iter().find(|row| row.name == name).unwrap();
            rows[row.parent.unwrap()].name.clone()
        };
        assert_eq!(in_hooks("@mon"), "Session Hooks");
        assert_eq!(in_hooks("@event"), "Session Hooks");
        mode.format = None;
        select(&engine, pane, &mut mode, "@mon");
        let preview = preview_text(&engine, pane, &mode);
        assert!(
            preview.contains("This hook runs when a monitor changes."),
            "{preview}"
        );
        assert!(preview.contains("This is a monitor hook."), "{preview}");
        assert!(
            preview.contains("Monitor: #[fg=themelightgrey]@mon:%*:##{pane_id}"),
            "{preview}"
        );
    }

    #[test]
    fn customize_changed_only_keeps_changed_rows_and_drops_environment() {
        let (mut engine, mut context, pane) = engine_with_session();
        engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-option", ["-g", "status-left", "changed"]),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &CommandInvocation::new(
                    "set-hook",
                    ["-g", "after-new-window", "display-message hi"],
                ),
            )
            .unwrap();
        engine
            .execute(
                &mut context,
                &CommandInvocation::new(
                    "bind-key",
                    ["-T", "prefix", "F7", "display-message seven"],
                ),
            )
            .unwrap();
        let mut mode = CustomizeMode::default();
        press(&mut engine, &mut context, pane, &mut mode, "C");
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let names = rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>();
        assert!(names.contains(&"status-left"));
        assert!(!names.contains(&"status-right"));
        assert!(names.contains(&"after-new-window"));
        assert!(names.contains(&"after-new-window[0]"));
        assert!(!names.contains(&"after-new-session"));
        assert!(names.contains(&"F7"));
        assert!(!names.contains(&"Global Environment"));
        assert!(!names.contains(&"Session Environment"));
        assert!(!names.contains(&"Server Options"));
        press(&mut engine, &mut context, pane, &mut mode, "C");
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        assert!(rows.iter().any(|row| row.name == "Global Environment"));
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
    fn customize_preview_flags_follow_mode_tree_start() {
        let (mut engine, mut context, _) = engine_with_session();
        for (flags, expected) in [
            (&[][..], Preview::Normal),
            (&["-N"][..], Preview::Off),
            (&["-NN"][..], Preview::Big),
            (&["-N", "-N"][..], Preview::Big),
        ] {
            let effects = engine
                .execute(
                    &mut context,
                    &CommandInvocation::new("customize-mode", flags.iter().copied()),
                )
                .unwrap()
                .effects;
            let [
                MuxEffect::PaneModeChanged {
                    mode: Some(PaneModeRequest::Customize(mode)),
                    ..
                },
            ] = effects.as_slice()
            else {
                panic!("customize-mode {flags:?} opened no mode: {effects:?}");
            };
            assert_eq!(mode.preview, expected, "{flags:?}");
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
    fn customize_menu_feed_answers_each_row() {
        assert_eq!(customize_menu_feed(false, 0), Some("Enter"));
        assert_eq!(customize_menu_feed(false, 1), Some("e"));
        assert_eq!(customize_menu_feed(false, 2), Some("Right"));
        assert_eq!(customize_menu_feed(false, 3), None);
        assert_eq!(customize_menu_feed(false, 4), Some("t"));
        assert_eq!(customize_menu_feed(false, 5), Some("\x14"));
        assert_eq!(customize_menu_feed(false, 6), Some("T"));
        assert_eq!(customize_menu_feed(false, 7), None);
        assert_eq!(customize_menu_feed(false, 8), Some("C"));
        assert_eq!(customize_menu_feed(false, 9), None);
        assert_eq!(customize_menu_feed(false, 10), Some("q"));
        assert_eq!(customize_menu_feed(false, 11), None);
        assert_eq!(customize_menu_feed(true, 0), Some("<"));
        assert_eq!(customize_menu_feed(true, 1), Some(">"));
        assert_eq!(customize_menu_feed(true, 2), None);
        assert_eq!(customize_menu_feed(true, 3), Some("q"));
        assert_eq!(customize_menu_feed(true, 4), None);
        let tag_all = CUSTOMIZE_MENU_ITEMS
            .iter()
            .flatten()
            .find(|item| item.name == "Tag All")
            .unwrap();
        assert_eq!(tag_all.key, "[DC4]");
        assert_eq!(tag_all.annotation, "[DC4]");
        assert_eq!(tag_all.feed, "\x14");
    }

    #[test]
    fn customize_mouse_down3_selects_and_requests_the_line_menu() {
        let (engine, _, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        let mut expand = customize_expand;
        let result = engine.customize_mouse(pane, &mut mode, "MouseDown1Pane", 3, 2, &mut expand);
        assert!(!result.close);
        assert!(result.menu.is_none());
        let result = engine.customize_mouse(pane, &mut mode, "MouseDown3Pane", 3, 2, &mut expand);
        assert!(!result.close);
        assert_eq!(mode.current, 2);
        let menu = result.menu.expect("a menu request");
        assert!(!menu.outside);
        assert_eq!(menu.line, 2);
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let lines = customize_lines(&rows, &mode);
        assert_eq!(menu.name, rows[lines[2]].name);
        let result = engine.customize_mouse(pane, &mut mode, "WheelUpPane", 3, 2, &mut expand);
        assert!(!result.close);
        assert!(result.menu.is_none());
    }

    #[test]
    fn customize_mouse_down3_outside_requests_the_outside_menu() {
        let (engine, _, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let size = customize_lines(&rows, &mode).len();
        let below = size.max(mode.height) + 5;
        let result =
            engine.customize_mouse(pane, &mut mode, "MouseDown3Pane", 3, below, &mut expand);
        assert!(!result.close);
        assert_eq!(mode.current, 0);
        let menu = result.menu.expect("a menu request");
        assert!(menu.outside);
        assert_eq!(menu.line, 0);
        let columns = engine.customize_screen_columns(pane);
        let result = engine.customize_mouse(
            pane,
            &mut mode,
            "MouseDown3Pane",
            columns + 1,
            1,
            &mut expand,
        );
        assert!(result.menu.is_some_and(|menu| menu.outside));
        let result =
            engine.customize_mouse(pane, &mut mode, "MouseDown1Pane", 3, below, &mut expand);
        assert!(result.menu.is_none());
    }

    #[test]
    fn customize_mouse_down3_below_the_lines_keeps_the_current_line() {
        let (engine, _, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let size = customize_lines(&rows, &mode).len();
        mode.height = size + 4;
        mode.current = 1;
        let result =
            engine.customize_mouse(pane, &mut mode, "MouseDown3Pane", 3, size, &mut expand);
        assert!(!result.close);
        assert_eq!(mode.current, 1);
        let menu = result.menu.expect("a menu request");
        assert!(!menu.outside);
        assert_eq!(menu.line, 1);
    }

    #[test]
    fn customize_menu_choice_runs_the_key_on_the_menu_line() {
        let (engine, _, pane) = engine_with_session();
        let mut mode = CustomizeMode::default();
        engine.customize_height(pane, &mut mode);
        mode.expanded.insert("options:0".to_owned());
        let mut expand = customize_expand;
        let rows = engine.customize_rows(pane, &mode, &mut expand);
        let lines = customize_lines(&rows, &mode);
        let option = 1;
        assert_eq!(rows[lines[option]].depth, 1);
        mode.current = 0;
        let result = engine.customize_menu_choice(pane, &mut mode, option, "t", &mut expand);
        assert!(!result.close);
        assert_eq!(mode.current, option);
        assert!(mode.tagged.contains(&rows[lines[option]].id));
        let tagged = mode.tagged.clone();
        let result = engine.customize_menu_choice(pane, &mut mode, option, "\x14", &mut expand);
        assert!(!result.close);
        assert_eq!(mode.current, option);
        assert_eq!(mode.tagged, tagged);
        let result = engine.customize_menu_choice(pane, &mut mode, option, "T", &mut expand);
        assert!(!result.close);
        assert!(mode.tagged.is_empty());
        let result =
            engine.customize_menu_choice(pane, &mut mode, lines.len() + 4, "t", &mut expand);
        assert!(!result.close);
        assert_eq!(mode.current, option);
        assert!(mode.tagged.is_empty());
        let closed = engine.customize_menu_choice(pane, &mut mode, option, "q", &mut expand);
        assert!(closed.close);
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
