use std::collections::HashSet;

use zz_daemon::{Endpoint, HostEntry};
use zz_protocol::{
    MAX_COMMAND_PROMPT_BYTES, MuxSnapshot, PaneId, PaneKindSnapshot, PaneSnapshot, SessionId,
    WindowId, WindowSnapshot,
};

use crate::layout::Rect;

pub(crate) const WIDTH: u16 = 28;
pub(crate) const BORDER_WIDTH: u16 = 1;
pub(crate) const AUTO_HIDE_COLUMNS: u16 = 80;
pub(crate) const MIN_MANUAL_COLUMNS: u16 = 50;
pub(crate) const STATUS_ROWS: u16 = 3;
const FULL_WIDTH_STATUS_ROWS: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Visibility {
    Auto,
    Shown,
    Hidden,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct State {
    pub focused: bool,
    pub selected: usize,
    pub scroll: usize,
    visibility: Visibility,
}

impl Default for State {
    fn default() -> Self {
        Self {
            focused: false,
            selected: 0,
            scroll: 0,
            visibility: Visibility::Auto,
        }
    }
}

impl State {
    pub const fn visible(&self, columns: u16) -> bool {
        columns >= MIN_MANUAL_COLUMNS
            && match self.visibility {
                Visibility::Auto => columns >= AUTO_HIDE_COLUMNS,
                Visibility::Shown => true,
                Visibility::Hidden => false,
            }
    }

    pub fn focus(&mut self, columns: u16) {
        if columns < MIN_MANUAL_COLUMNS {
            self.focused = false;
            return;
        }
        if !self.visible(columns) {
            self.visibility = Visibility::Shown;
        }
        self.focused = true;
    }

    pub fn toggle_focus(&mut self, columns: u16) {
        if self.focused {
            self.focused = false;
        } else {
            self.focus(columns);
        }
    }

    pub fn hide(&mut self) {
        self.focused = false;
        self.visibility = Visibility::Hidden;
    }

    pub fn reconcile_width(&mut self, columns: u16) {
        if !self.visible(columns) {
            self.focused = false;
        }
    }

    pub fn clamp(&mut self, row_count: usize, viewport_height: u16) {
        self.selected = self.selected.min(row_count.saturating_sub(1));
        let max_scroll = row_count.saturating_sub(usize::from(viewport_height));
        self.scroll = self.scroll.min(max_scroll);
        self.reveal_selection(viewport_height, row_count);
    }

    pub fn move_selection(&mut self, delta: isize, row_count: usize, viewport_height: u16) {
        if row_count == 0 {
            self.selected = 0;
            self.scroll = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(row_count.saturating_sub(1));
        self.reveal_selection(viewport_height, row_count);
    }

    pub fn select(&mut self, selected: usize, row_count: usize, viewport_height: u16) {
        self.selected = selected.min(row_count.saturating_sub(1));
        self.reveal_selection(viewport_height, row_count);
    }

    pub fn scroll(&mut self, delta: isize, row_count: usize, viewport_height: u16) {
        let max_scroll = row_count.saturating_sub(usize::from(viewport_height));
        self.scroll = self.scroll.saturating_add_signed(delta).min(max_scroll);
    }

    fn reveal_selection(&mut self, viewport_height: u16, row_count: usize) {
        let height = usize::from(viewport_height);
        if height == 0 {
            self.scroll = 0;
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll.saturating_add(height) {
            self.scroll = self.selected.saturating_add(1).saturating_sub(height);
        }
        self.scroll = self.scroll.min(row_count.saturating_sub(height));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Target {
    Session(SessionId),
    Window(WindowId),
    Pane(PaneId),
    NewPane(PaneId),
    LocalHost,
    FleetHost(usize),
    AddHost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditKind {
    RenameSession(SessionId),
    RenameWindow(WindowId),
    AddHost,
}

impl EditKind {
    pub const fn target(self) -> Target {
        match self {
            Self::RenameSession(session) => Target::Session(session),
            Self::RenameWindow(window) => Target::Window(window),
            Self::AddHost => Target::AddHost,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Edit {
    pub kind: EditKind,
    pub buffer: String,
    pub cursor: usize,
}

impl Edit {
    pub fn new(kind: EditKind, buffer: String) -> Self {
        let cursor = buffer.chars().count();
        Self {
            kind,
            buffer,
            cursor,
        }
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.cursor = self
            .cursor
            .saturating_add(1)
            .min(self.buffer.chars().count());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.buffer.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let end = scalar_byte_index(&self.buffer, self.cursor);
        let start = scalar_byte_index(&self.buffer, self.cursor - 1);
        self.buffer.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        let start = scalar_byte_index(&self.buffer, self.cursor);
        let end = scalar_byte_index(&self.buffer, self.cursor.saturating_add(1));
        if start < end {
            self.buffer.replace_range(start..end, "");
        }
    }

    pub fn insert_char(&mut self, character: char) {
        if self.buffer.len().saturating_add(character.len_utf8()) > MAX_COMMAND_PROMPT_BYTES {
            return;
        }
        let index = scalar_byte_index(&self.buffer, self.cursor);
        self.buffer.insert(index, character);
        self.cursor = self.cursor.saturating_add(1);
    }

    pub fn insert_text(&mut self, text: &str) {
        let available = MAX_COMMAND_PROMPT_BYTES.saturating_sub(self.buffer.len());
        let mut end = text.len().min(available);
        while !text.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        let inserted = &text[..end];
        let index = scalar_byte_index(&self.buffer, self.cursor);
        self.buffer.insert_str(index, inserted);
        self.cursor = self.cursor.saturating_add(inserted.chars().count());
    }

    pub fn viewport(&self, width: u16) -> (String, u16) {
        let width = usize::from(width);
        if width == 0 {
            return (String::new(), 0);
        }
        let cursor = self.cursor.min(self.buffer.chars().count());
        let start = cursor.saturating_sub(width.saturating_sub(1));
        let text = self.buffer.chars().skip(start).take(width).collect();
        let cursor_column = u16::try_from(cursor.saturating_sub(start))
            .unwrap_or(u16::MAX)
            .min(u16::try_from(width.saturating_sub(1)).unwrap_or(u16::MAX));
        (text, cursor_column)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Row {
    pub text: String,
    pub target: Option<Target>,
}

pub(crate) fn flatten(
    snapshot: &MuxSnapshot,
    attached_session: Option<SessionId>,
    host_label: &str,
    fleet_hosts: &[HostEntry],
    local_endpoint: &Endpoint,
    current_endpoint: &Endpoint,
) -> Vec<Row> {
    let mut rows = vec![Row {
        text: format!("zz at {host_label}"),
        target: None,
    }];
    let show_sessions = snapshot.sessions.len() > 1;
    for session in &snapshot.sessions {
        if show_sessions {
            let marker = if Some(session.id) == attached_session {
                '*'
            } else {
                ' '
            };
            rows.push(Row {
                text: format!("  {marker} {}", session.name),
                target: Some(Target::Session(session.id)),
            });
        }
        let window_indent = if show_sessions { "    " } else { "  " };
        let pane_indent = if show_sessions { "      " } else { "    " };
        let focused_window = snapshot.focused_window_for(session);
        for window in &session.windows {
            let window_active = Some(session.id) == attached_session && window.id == focused_window;
            rows.push(Row {
                text: format!(
                    "{window_indent}{} # {} {}",
                    if window_active { '*' } else { ' ' },
                    window.index,
                    window.name
                ),
                target: Some(Target::Window(window.id)),
            });
            for pane in ordered_panes(window) {
                let active_marker = if window_active && pane.id == window.active_pane {
                    '▸'
                } else {
                    ' '
                };
                let bell = if pane.bell { " ●" } else { "" };
                rows.push(Row {
                    text: format!(
                        "{pane_indent}{active_marker} {} {}{bell}",
                        pane_glyph(&pane.kind),
                        pane_title(pane)
                    ),
                    target: Some(Target::Pane(pane.id)),
                });
            }
            rows.push(Row {
                text: format!("{pane_indent}  + new pane"),
                target: Some(Target::NewPane(window.active_pane)),
            });
        }
    }
    rows.push(Row {
        text: "hosts".to_owned(),
        target: None,
    });
    rows.push(Row {
        text: format!(
            "  {} local",
            if current_endpoint == local_endpoint {
                '*'
            } else {
                ' '
            }
        ),
        target: Some(Target::LocalHost),
    });
    for (index, host) in fleet_hosts.iter().enumerate() {
        rows.push(Row {
            text: format!(
                "  {} {}",
                if current_endpoint == &host.endpoint {
                    '*'
                } else {
                    ' '
                },
                host.name
            ),
            target: Some(Target::FleetHost(index)),
        });
    }
    rows.push(Row {
        text: "    + add host".to_owned(),
        target: Some(Target::AddHost),
    });
    rows
}

pub(crate) fn canvas_rect(columns: u16, rows: u16, sidebar_visible: bool) -> Rect {
    if sidebar_visible {
        let x = WIDTH.saturating_add(BORDER_WIDTH).min(columns);
        Rect {
            x,
            y: 0,
            width: columns.saturating_sub(x),
            height: rows,
        }
    } else {
        Rect {
            x: 0,
            y: 0,
            width: columns,
            height: rows.saturating_sub(FULL_WIDTH_STATUS_ROWS),
        }
    }
}

pub(crate) const fn tree_height(rows: u16) -> u16 {
    rows.saturating_sub(STATUS_ROWS)
}

fn scalar_byte_index(text: &str, scalar: usize) -> usize {
    text.char_indices()
        .nth(scalar)
        .map_or(text.len(), |(index, _)| index)
}

fn ordered_panes(window: &WindowSnapshot) -> Vec<&PaneSnapshot> {
    let mut layout_order = Vec::with_capacity(window.panes.len());
    window.layout.panes(&mut layout_order);
    let mut seen = HashSet::new();
    let mut panes = Vec::with_capacity(window.panes.len());
    for id in layout_order {
        if seen.insert(id)
            && let Some(pane) = window.panes.get(&id)
        {
            panes.push(pane);
        }
    }
    for (id, pane) in &window.panes {
        if seen.insert(*id) {
            panes.push(pane);
        }
    }
    panes
}

fn pane_title(pane: &PaneSnapshot) -> &str {
    let title = pane.title.trim();
    if !title.is_empty() {
        return title;
    }
    match pane.kind {
        PaneKindSnapshot::Picker => "new pane",
        PaneKindSnapshot::Terminal => "terminal",
        PaneKindSnapshot::Browser(_) => "browser",
        PaneKindSnapshot::Agent(_) => "agent",
        PaneKindSnapshot::Editor(_) => "editor",
    }
}

const fn pane_glyph(kind: &PaneKindSnapshot) -> char {
    match kind {
        PaneKindSnapshot::Picker => '+',
        PaneKindSnapshot::Terminal => '$',
        PaneKindSnapshot::Browser(_) => 'B',
        PaneKindSnapshot::Agent(_) => 'A',
        PaneKindSnapshot::Editor(_) => 'E',
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use zz_protocol::{Axis, LayoutNode, SplitId, WindowSnapshot};

    fn pane(id: u64, title: &str, kind: PaneKindSnapshot, bell: bool) -> PaneSnapshot {
        PaneSnapshot {
            id: PaneId(id),
            title: title.to_owned(),
            kind,
            synchronized_input: false,
            bell,
            dead: false,
            dead_status: None,
        }
    }

    fn window(id: u64, index: u32, name: &str) -> WindowSnapshot {
        let terminal = PaneId(id * 10 + 1);
        let browser = PaneId(id * 10 + 2);
        WindowSnapshot {
            id: WindowId(id),
            index,
            name: name.to_owned(),
            automatic_rename: true,
            active_pane: browser,
            zoomed_pane: None,
            layout: LayoutNode::Split {
                id: SplitId(id),
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(terminal)),
                second: Box::new(LayoutNode::Pane(browser)),
            },
            panes: BTreeMap::from([
                (
                    terminal,
                    pane(terminal.0, "shell", PaneKindSnapshot::Terminal, false),
                ),
                (
                    browser,
                    pane(
                        browser.0,
                        "docs",
                        PaneKindSnapshot::Browser(zz_protocol::BrowserDescriptor::single(
                            "https://example.com".to_owned(),
                            "default".to_owned(),
                        )),
                        true,
                    ),
                ),
            ]),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
            status_label: String::new(),
        }
    }

    fn session(id: u64, name: &str, window: WindowSnapshot) -> zz_protocol::SessionSnapshot {
        zz_protocol::SessionSnapshot {
            id: SessionId(id),
            name: name.to_owned(),
            active_window: window.id,
            windows: vec![window],
            viewers: Vec::new(),
        }
    }

    fn local_endpoint() -> Endpoint {
        Endpoint::parse("unix:///tmp/zz.sock").unwrap()
    }

    #[test]
    fn single_session_flattens_windows_directly_under_host_with_markers() {
        let window = window(1, 3, "work");
        let snapshot = MuxSnapshot {
            generation: 1,
            sessions: vec![session(7, "main", window)],
            focused_window: Some(WindowId(1)),
        };

        let local = local_endpoint();
        let rows = flatten(
            &snapshot,
            Some(SessionId(7)),
            "macbook",
            &[],
            &local,
            &local,
        );
        let text = rows.iter().map(|row| row.text.as_str()).collect::<Vec<_>>();

        assert_eq!(text[0], "zz at macbook");
        assert!(text.iter().all(|row| !row.contains("main")));
        assert!(text.iter().any(|row| row.contains("* # 3 work")));
        assert!(text.iter().any(|row| row.contains("▸ B docs ●")));
        assert!(
            rows.iter()
                .any(|row| matches!(row.target, Some(Target::NewPane(_))))
        );
    }

    #[test]
    fn multiple_sessions_include_every_session_node() {
        let snapshot = MuxSnapshot {
            generation: 1,
            sessions: vec![
                session(7, "main", window(1, 0, "one")),
                session(8, "other", window(2, 1, "two")),
            ],
            focused_window: Some(WindowId(1)),
        };

        let local = local_endpoint();
        let rows = flatten(&snapshot, Some(SessionId(7)), "box", &[], &local, &local);

        assert!(rows.iter().any(|row| row.text == "  * main"));
        assert!(rows.iter().any(|row| row.text == "    other"));
        assert_eq!(
            rows.iter()
                .filter(|row| matches!(row.target, Some(Target::NewPane(_))))
                .count(),
            2
        );
    }

    #[test]
    fn visibility_thresholds_and_canvas_arithmetic_match_the_chrome_contract() {
        let mut state = State::default();
        assert!(state.visible(80));
        assert!(!state.visible(79));
        state.focus(60);
        assert!(state.visible(60));
        state.hide();
        assert!(!state.visible(120));

        assert_eq!(
            canvas_rect(100, 30, true),
            Rect {
                x: 29,
                y: 0,
                width: 71,
                height: 30,
            }
        );
        assert_eq!(
            canvas_rect(70, 20, false),
            Rect {
                x: 0,
                y: 0,
                width: 70,
                height: 19,
            }
        );
    }

    #[test]
    fn hosts_section_marks_the_current_endpoint_and_ends_with_add_host() {
        let local = local_endpoint();
        let box_endpoint = Endpoint::parse("ssh://box").unwrap();
        let hosts = [
            HostEntry {
                name: "box".to_owned(),
                endpoint: box_endpoint.clone(),
            },
            HostEntry {
                name: "gpu".to_owned(),
                endpoint: Endpoint::parse("ssh://gpu").unwrap(),
            },
        ];

        let rows = flatten(
            &MuxSnapshot::default(),
            None,
            "box",
            &hosts,
            &local,
            &box_endpoint,
        );
        let hosts_header = rows.iter().position(|row| row.text == "hosts").unwrap();

        assert_eq!(rows[hosts_header + 1].text, "    local");
        assert_eq!(rows[hosts_header + 2].text, "  * box");
        assert_eq!(rows[hosts_header + 3].text, "    gpu");
        assert_eq!(rows.last().unwrap().text, "    + add host");
        assert_eq!(rows.last().unwrap().target, Some(Target::AddHost));
    }

    #[test]
    fn editor_operations_track_multibyte_scalar_positions_and_scroll_the_cursor() {
        let mut edit = Edit::new(EditKind::AddHost, "aéz".to_owned());
        edit.move_left();
        edit.backspace();
        assert_eq!(edit.buffer, "az");
        assert_eq!(edit.cursor, 1);

        edit.insert_text("界x");
        assert_eq!(edit.buffer, "a界xz");
        assert_eq!(edit.cursor, 3);
        edit.delete();
        assert_eq!(edit.buffer, "a界x");
        edit.move_home();
        edit.insert_char('é');
        assert_eq!(edit.buffer, "éa界x");

        edit.move_end();
        let (visible, cursor) = edit.viewport(3);
        assert_eq!(visible, "界x");
        assert_eq!(cursor, 2);
    }
}
