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
            .and_then(|item| tree_preview(&styles, item.target));
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

fn tree_preview(styles: &Styles<'_>, target: ChooseTreeTarget) -> Option<ChooserPreview> {
    let state = &styles.inner.engine.state;
    match target {
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
