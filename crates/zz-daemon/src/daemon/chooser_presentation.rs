use super::*;
use zz_mux::TmuxSortOrder;
use zz_protocol::{
    ChooserPresentation, ChooserPreview, ChooserPreviewSize, ChooserPreviewTile, ChooserRow,
};

const WINDOW_TREE_DEFAULT_FORMAT: &str = concat!(
    "#{?pane_format,",
    "#{?pane_marked,#[fg=thememagenta],}#{?pane_floating_flag,#[underscore],}",
    "#{pane_current_command}#[fg=themelightgrey]#{pane_flags}",
    "#{?#{&&:#{pane_title},#{!=:#{pane_title},#{host_short}}},: \"#{pane_title}\",}",
    ",window_format,",
    "#{?window_marked_flag,#[fg=thememagenta],}",
    "#{window_name}#[fg=themelightgrey]#{window_flags}",
    "#{?#{&&:#{==:#{window_panes},1},#{&&:#{pane_title},#{!=:#{pane_title},#{host_short}}}},: \"#{pane_title}\",}",
    ",",
    "#[fg=themelightgrey]#{session_windows} windows",
    "#{?session_grouped, (group #{session_group}: #{session_group_list}),}",
    "#{?session_attached, (attached),}",
    "}"
);
const WINDOW_BUFFER_DEFAULT_FORMAT: &str = "#{t/p:buffer_created}: #{buffer_sample}";
/// `WINDOW_CLIENT_DEFAULT_FORMAT`.
const WINDOW_CLIENT_DEFAULT_FORMAT: &str =
    "#[fg=themelightgrey]#{t/p:client_activity}: session #[default]#{session_name}";
/// `window_client_info_lines`, the `i` view, verbatim.
const WINDOW_CLIENT_INFO_LINES: &[&str] = &[
    concat!(
        "#[fg=themelightgrey]Client Name   #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{client_name} #[fg=themelightgrey]#[fg=themelightgrey](PID #{client_pid})#[default]"
    ),
    concat!(
        "#[fg=themelightgrey]Session       #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{session_name}"
    ),
    concat!(
        "#[fg=themelightgrey]Attach Time   #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{t:client_created} #[fg=themelightgrey](#{t/r:client_created})#[default]"
    ),
    concat!(
        "#[fg=themelightgrey]Activity Time #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{t:client_activity} #[fg=themelightgrey](#{t/r:client_activity})#[default]"
    ),
    concat!(
        "#[fg=themelightgrey]Terminal Type #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{?client_termtype,#{client_termtype},Unknown}"
    ),
    concat!(
        "#[fg=themelightgrey]TERM          #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{client_termname}"
    ),
    concat!(
        "#[fg=themelightgrey]Size          #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{client_width}x#{client_height} ",
        "#[fg=themelightgrey](cell #{client_cell_width}x#{client_cell_height})#[default]"
    ),
    concat!(
        "#[fg=themelightgrey]Bytes Written #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{client_written} #[fg=themelightgrey](#{client_discarded} discarded)#[default]"
    ),
    concat!(
        "#[fg=themelightgrey]prefix        #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{prefix}"
    ),
    concat!(
        "#[fg=themelightgrey]escape-time   #[#{E:tree-mode-border-style},acs]x#[default] ",
        "#{escape-time} ms"
    ),
];
const TREE_MODE_BORDER_STYLE: &str = "bg=themedarkgrey,fg=themelightgrey";
const TREE_MODE_SELECTION_STYLE: &str = "#{E:mode-style}";
const TREE_MODE_PREVIEW_FORMAT: &str =
    "#{?pane_format,#{pane_index}:#{pane_title},#{window_index}:#{window_name}}";
const TREE_MODE_PREVIEW_STYLE: &str = concat!(
    "fg=#{?#{||:",
    "#{&&:#{pane_format},#{pane_active}},",
    "#{&&:#{window_format},#{window_active}}},",
    "themered,",
    "themeblue}"
);
const MESSAGE_STYLE: &str = "bg=themeyellow,fg=themeblack";
const PREVIEW_TEXT_LINES: usize = 256;

fn scope_variables(session: bool, window: bool, pane: bool) -> BTreeMap<String, String> {
    let flag = |on: bool| if on { "1" } else { "0" }.to_owned();
    BTreeMap::from([
        ("session_format".to_owned(), flag(session)),
        ("window_format".to_owned(), flag(window)),
        ("pane_format".to_owned(), flag(pane)),
    ])
}

fn expand_row(
    engine: &MuxEngine,
    format: &str,
    context: &ExecutionContext,
    variables: &BTreeMap<String, String>,
    attached_session: Option<SessionId>,
    facts: &FormatHookFacts,
) -> String {
    let mut hooks =
        DaemonFormatHooks::command_with_variables(facts, variables).with_option_engine(engine);
    engine.expand_pane_format(
        format,
        context,
        attached_session,
        FormatClient::NoClient,
        &mut hooks,
    )
}

/// `window_client_build`: one row per attached client, with its shortcut key
/// column, its name and the row text expanded in that client's own format
/// tree, which is the only place the client formats resolve.
pub(super) fn client_chooser_rows(
    inner: &ServerState,
    format: Option<&str>,
    filter: Option<&str>,
) -> Vec<ClientChooserRow> {
    let format = format.unwrap_or(WINDOW_CLIENT_DEFAULT_FORMAT);
    let mut clients = inner
        .attached
        .iter()
        .flat_map(|(session, clients)| {
            clients
                .iter()
                .copied()
                .map(|client| (client, *session))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    clients.sort_by_key(|(client, _)| client.0);
    let base = format_hook_facts(inner);
    let mut rows = Vec::with_capacity(clients.len());
    for (line, (client, session_id)) in clients.into_iter().enumerate() {
        let Some(session) = inner.engine.state.sessions.get(&session_id) else {
            continue;
        };
        let focused = client_focused_window(inner, client, session);
        let mut context = inner.engine.format_status_context_for_client(
            Some(session_id),
            Some(focused),
            None,
            session_id,
        );
        context.config_files.clone_from(&inner.config_files);
        let mut client_facts = client_format_facts(inner, client, session_id);
        client_facts.line = line;
        let name = client_facts.name.clone();
        let width = client_facts.width.parse::<u16>().unwrap_or_default();
        let height = client_facts.height.parse::<u16>().unwrap_or_default();
        let facts = FormatHookFacts {
            client: Some(client_facts),
            ..base.clone()
        };
        let mut hooks = DaemonFormatHooks::command(&facts).with_option_engine(&inner.engine);
        let text = expand_format_values(format, &context, &mut hooks);
        let matches = filter.is_none_or(|filter| {
            let mut hooks = DaemonFormatHooks::command(&facts).with_option_engine(&inner.engine);
            format_true(&expand_format_values(filter, &context, &mut hooks))
        });
        rows.push(ClientChooserRow {
            client,
            name,
            text,
            activity: inner
                .client_activity
                .get(&client)
                .copied()
                .unwrap_or_default(),
            created: inner
                .client_created_times
                .get(&client)
                .copied()
                .unwrap_or_default(),
            width,
            height,
            matches,
            pane: inner
                .engine
                .state
                .windows
                .get(&focused)
                .map(|window| window.active_pane),
        });
    }
    rows
}

/// `window_client_draw_info`, expanded in the chosen client's format tree.
fn client_info_lines(inner: &ServerState, client: ClientId) -> Vec<String> {
    let Some(session_id) = client_attached_session(inner, client) else {
        return Vec::new();
    };
    let Some(session) = inner.engine.state.sessions.get(&session_id) else {
        return Vec::new();
    };
    let focused = client_focused_window(inner, client, session);
    let mut context = inner.engine.format_status_context_for_client(
        Some(session_id),
        Some(focused),
        None,
        session_id,
    );
    context.config_files.clone_from(&inner.config_files);
    let facts = FormatHookFacts {
        client: Some(client_format_facts(inner, client, session_id)),
        ..format_hook_facts(inner)
    };
    WINDOW_CLIENT_INFO_LINES
        .iter()
        .map(|line| {
            let mut hooks = DaemonFormatHooks::command(&facts).with_option_engine(&inner.engine);
            expand_format_values(line, &context, &mut hooks)
        })
        .collect()
}

pub(super) fn tree_rows(
    engine: &MuxEngine,
    items: &[ChooseTreeItem],
    formatted: bool,
    attached_session: Option<SessionId>,
    facts: &FormatHookFacts,
) -> Vec<ChooserRow> {
    let state = &engine.state;
    items
        .iter()
        .map(|item| {
            if matches!(item.target, ChooseTreeTarget::Client(_)) {
                return ChooserRow {
                    name: item.label.clone(),
                    text: item.text.clone(),
                    align: false,
                };
            }
            let (name, context, variables, align) = match item.target {
                ChooseTreeTarget::Session(session) => (
                    state
                        .sessions
                        .get(&session)
                        .map_or_else(String::new, |session| session.name.clone()),
                    Some(ExecutionContext::new(Some(session), None, None)),
                    scope_variables(true, false, false),
                    false,
                ),
                ChooseTreeTarget::Window(window) => (
                    state
                        .windows
                        .get(&window)
                        .map_or_else(String::new, |entry| entry.index.to_string()),
                    state.windows.get(&window).map(|entry| {
                        ExecutionContext::new(Some(entry.session), Some(window), None)
                    }),
                    scope_variables(false, true, false),
                    true,
                ),
                ChooseTreeTarget::Pane(pane) => (
                    state
                        .window_for_pane(pane)
                        .and_then(|window| engine.pane_index(window, pane))
                        .map_or_else(String::new, |index| index.to_string()),
                    ExecutionContext::for_pane(state, pane),
                    scope_variables(false, false, true),
                    true,
                ),
                ChooseTreeTarget::Client(_) => unreachable!("client rows return early"),
            };
            let text = if formatted {
                item.text.clone()
            } else {
                context.map_or_else(String::new, |context| {
                    expand_row(
                        engine,
                        WINDOW_TREE_DEFAULT_FORMAT,
                        &context,
                        &variables,
                        attached_session,
                        facts,
                    )
                })
            };
            ChooserRow { name, text, align }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn buffer_rows(
    engine: &MuxEngine,
    buffers: &[PasteBuffer],
    names: &[String],
    items: &[ChooseBufferItem],
    formatted: bool,
    source: Option<&ExecutionContext>,
    attached_session: Option<SessionId>,
    facts: &FormatHookFacts,
) -> Vec<ChooserRow> {
    let variables = scope_variables(false, false, true);
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let buffer = names
                .get(index)
                .and_then(|name| buffers.iter().find(|buffer| &buffer.name == name));
            let text = if formatted {
                item.text.clone()
            } else {
                match (buffer, source) {
                    (Some(buffer), Some(source)) => {
                        let facts = FormatHookFacts {
                            buffer: Some(buffer_format_facts(buffer)),
                            ..facts.clone()
                        };
                        expand_row(
                            engine,
                            WINDOW_BUFFER_DEFAULT_FORMAT,
                            source,
                            &variables,
                            attached_session,
                            &facts,
                        )
                    }
                    _ => String::new(),
                }
            };
            ChooserRow {
                name: item.name.clone(),
                text,
                align: false,
            }
        })
        .collect()
}

pub(super) const fn next_preview_size(size: ChooserPreviewSize) -> ChooserPreviewSize {
    match size {
        ChooserPreviewSize::Off => ChooserPreviewSize::Big,
        ChooserPreviewSize::Normal => ChooserPreviewSize::Off,
        ChooserPreviewSize::Big => ChooserPreviewSize::Normal,
    }
}

struct ScopedHooks<'a> {
    engine: &'a MuxEngine,
    variables: BTreeMap<String, String>,
}

impl StatusHooks for ScopedHooks<'_> {
    fn strftime(&mut self, literal: &str) -> String {
        literal.to_owned()
    }

    fn shell(&mut self, _command: &str, _tag: &zz_mux::FormatJobTag) -> String {
        String::new()
    }

    fn variable(&mut self, name: &str, context: &zz_mux::StatusContext) -> Option<String> {
        self.variables
            .get(name)
            .cloned()
            .or_else(|| self.engine.format_option_value(context, name))
    }
}

fn sort_label(sort: TmuxSort) -> String {
    let order = match sort.order() {
        Some(TmuxSortOrder::Activity) => "activity",
        Some(TmuxSortOrder::Creation) => "creation",
        Some(TmuxSortOrder::Index) => "index",
        Some(TmuxSortOrder::Modifier) => "modifier",
        Some(TmuxSortOrder::Name) => "name",
        Some(TmuxSortOrder::Order) => "order",
        Some(TmuxSortOrder::Size) => "size",
        Some(TmuxSortOrder::Z) => "z",
        None => "",
    };
    if sort.reversed() {
        format!("{order}, reversed")
    } else {
        order.to_owned()
    }
}

struct Styles<'a> {
    inner: &'a ServerState,
}

impl Styles<'_> {
    fn expand(
        &self,
        value: &str,
        session: Option<SessionId>,
        window: Option<WindowId>,
        pane: Option<PaneId>,
    ) -> String {
        let engine = &self.inner.engine;
        let context =
            server_format_context(engine, &self.inner.config_files, session, window, pane);
        let mut hooks = ScopedHooks {
            engine,
            variables: scope_variables(
                window.is_none() && pane.is_none(),
                window.is_some() && pane.is_none(),
                pane.is_some(),
            ),
        };
        expand_format_values(value, &context, &mut hooks)
    }

    fn selection(&self, pane: PaneId) -> String {
        let state = &self.inner.engine.state;
        let window = state.window_for_pane(pane);
        let session =
            window.and_then(|window| state.windows.get(&window).map(|entry| entry.session));
        self.expand(TREE_MODE_SELECTION_STYLE, session, window, Some(pane))
    }

    fn tile(
        &self,
        session: SessionId,
        window: WindowId,
        pane: Option<PaneId>,
        shown: PaneId,
    ) -> ChooserPreviewTile {
        ChooserPreviewTile {
            label: self.expand(TREE_MODE_PREVIEW_FORMAT, Some(session), Some(window), pane),
            label_style: self.expand(TREE_MODE_PREVIEW_STYLE, Some(session), Some(window), pane),
            border_style: TREE_MODE_BORDER_STYLE.to_owned(),
            viewport: pane_viewport(self.inner, shown),
        }
    }
}

fn pane_viewport(inner: &ServerState, pane: PaneId) -> Option<TerminalViewport> {
    inner
        .terminals
        .get(&pane)
        .map(|terminal| (*terminal.latest_viewport()).clone())
}

pub(super) fn chooser_presentation(
    inner: &ServerState,
    client: ClientId,
) -> Option<ChooserPresentation> {
    let styles = Styles { inner };
    if let Some(chooser) = inner.choose_trees.get(&client) {
        let selected = usize::try_from(chooser.rendered.selected).unwrap_or(usize::MAX);
        let preview = chooser
            .rendered
            .items
            .get(selected)
            .and_then(|item| tree_preview(&styles, chooser, item.target));
        return Some(ChooserPresentation {
            selected: chooser.rendered.selected,
            rows: chooser.presentation_rows.clone(),
            sort: sort_label(chooser.sort),
            view: "preview".to_owned(),
            filter: chooser.filter.is_some(),
            selection_style: styles.selection(chooser.source_pane),
            border_style: TREE_MODE_BORDER_STYLE.to_owned(),
            prompt_style: MESSAGE_STYLE.to_owned(),
            preview_size: chooser.preview_size,
            preview,
        });
    }
    let chooser = inner.choose_buffers.get(&client)?;
    let selected = usize::try_from(chooser.rendered.selected).unwrap_or(usize::MAX);
    let preview = chooser
        .names
        .get(selected)
        .and_then(|name| {
            inner
                .paste_buffers
                .iter()
                .find(|buffer| &buffer.name == name)
        })
        .map(|buffer| ChooserPreview::Text {
            lines: buffer
                .data
                .split(|byte| *byte == b'\n')
                .take(PREVIEW_TEXT_LINES)
                .map(crate::status::vis_escape)
                .collect(),
        });
    Some(ChooserPresentation {
        selected: chooser.rendered.selected,
        rows: chooser.presentation_rows.clone(),
        sort: sort_label(chooser.sort),
        view: String::new(),
        filter: chooser.filter.is_some(),
        selection_style: styles.selection(chooser.source_pane),
        border_style: TREE_MODE_BORDER_STYLE.to_owned(),
        prompt_style: MESSAGE_STYLE.to_owned(),
        preview_size: chooser.preview_size,
        preview,
    })
}

fn tree_preview(
    styles: &Styles<'_>,
    chooser: &ChooseTreeSession,
    target: ChooseTreeTarget,
) -> Option<ChooserPreview> {
    let state = &styles.inner.engine.state;
    match target {
        // `window_client_draw`: the chosen client's current pane, a rule, and
        // that client's own status rows underneath it.
        ChooseTreeTarget::Client(id) => {
            let inner = styles.inner;
            if chooser.info_preview {
                return Some(ChooserPreview::Markup {
                    lines: client_info_lines(inner, id),
                });
            }
            let row = chooser.clients.iter().find(|row| row.client == id)?;
            let pane = row
                .pane
                .filter(|pane| !(chooser.hide_source && *pane == chooser.source_pane));
            let (status, status_style) = inner
                .client_status_rows
                .get(&id)
                .cloned()
                .unwrap_or_default();
            Some(ChooserPreview::Client {
                viewport: pane.and_then(|pane| pane_viewport(inner, pane)),
                status,
                status_style,
                status_width: u32::from(row.width),
            })
        }
        ChooseTreeTarget::Session(session_id) => {
            let session = state.sessions.get(&session_id)?;
            let mut windows = session
                .windows
                .iter()
                .filter_map(|window| state.windows.get(window).map(|entry| (*window, entry)))
                .collect::<Vec<_>>();
            windows.sort_by_key(|(_, entry)| entry.index);
            let current = windows
                .iter()
                .position(|(window, _)| *window == session.active_window)
                .unwrap_or(0);
            let tiles = windows
                .iter()
                .map(|(window, entry)| styles.tile(session_id, *window, None, entry.active_pane))
                .collect();
            Some(ChooserPreview::Tiles {
                tiles,
                current: u32::try_from(current).unwrap_or(0),
            })
        }
        ChooseTreeTarget::Window(window_id) => {
            let window = state.windows.get(&window_id)?;
            let panes = window.pane_order();
            let current = panes
                .iter()
                .position(|pane| *pane == window.active_pane)
                .unwrap_or(0);
            let tiles = panes
                .iter()
                .map(|pane| styles.tile(window.session, window_id, Some(*pane), *pane))
                .collect();
            Some(ChooserPreview::Tiles {
                tiles,
                current: u32::try_from(current).unwrap_or(0),
            })
        }
        ChooseTreeTarget::Pane(pane) => {
            pane_viewport(styles.inner, pane).map(|viewport| ChooserPreview::Screen { viewport })
        }
    }
}
