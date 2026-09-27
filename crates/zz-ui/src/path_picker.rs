use std::{collections::HashMap, ops::Range, rc::Rc, sync::Arc};

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render,
    ScrollStrategy, SharedString, Task, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use zz_client::path_rank;
use zz_protocol::{GitMark, PathEntry, PathKind, PathListRoot};

use crate::{
    ActiveTheme as _, Colorize as _, IconName,
    command::{
        COMMAND_PALETTE_MAX_WIDTH, COMMAND_PALETTE_ROW_HEIGHT, CommandPaletteSurface, PaletteHint,
        PaletteRow, command_palette_entry, command_palette_input,
    },
    input::{Backspace, Enter, Escape, IndentInline, InputEvent, InputState, MoveDown, MoveUp},
};

pub const PATH_PICKER_MAX_ROWS: usize = 500;
pub const PATH_PICKER_VISIBLE_ROWS: usize = 10;

pub trait PathPickerBackend {
    fn list(&self, dir: Option<&str>, cx: &mut App) -> Option<u64>;
    fn cancel(&self, request_id: u64, cx: &mut App);
    fn insert(&self, root: &PathListRoot, entry: &PathEntry, absolute: bool, cx: &mut App);
}

pub enum PathPickerEvent {
    Dismissed,
}

impl EventEmitter<PathPickerEvent> for PathPickerView {}

#[derive(Clone, Debug, Default)]
struct GitMarks {
    exact: HashMap<String, GitMark>,
    dirs: Vec<(String, GitMark)>,
}

impl GitMarks {
    fn get(&self, rel: &str) -> Option<GitMark> {
        self.exact.get(rel).copied().or_else(|| {
            self.dirs
                .iter()
                .find(|(prefix, _)| {
                    rel.starts_with(prefix.as_str()) || rel == prefix.trim_end_matches('/')
                })
                .map(|(_, mark)| *mark)
        })
    }
}

const fn mark_letter(mark: GitMark) -> &'static str {
    match mark {
        GitMark::Modified => "M",
        GitMark::Added => "A",
        GitMark::Untracked => "U",
        GitMark::Conflicted => "C",
    }
}

#[derive(Clone, Debug)]
struct PickerRow {
    entry: PathEntry,
    label: SharedString,
    matches: Vec<Range<usize>>,
    mark: Option<GitMark>,
}

struct Snapshot {
    chunks: Vec<Arc<[PathEntry]>>,
    base: String,
    query: String,
    marks: Arc<GitMarks>,
}

struct Ranked {
    query: String,
    rows: Arc<[PickerRow]>,
}

fn rank_rows(snapshot: &Snapshot) -> Arc<[PickerRow]> {
    let base = snapshot.base.as_str();
    let entries = snapshot
        .chunks
        .iter()
        .flat_map(|chunk| chunk.iter())
        .filter(|entry| entry.rel.len() > base.len() && entry.rel.starts_with(base))
        .collect::<Vec<_>>();
    let label = |entry: &PathEntry| entry.rel[base.len()..].trim_end_matches('/').to_owned();
    let row = |entry: &PathEntry, matches| PickerRow {
        entry: entry.clone(),
        label: label(entry).into(),
        matches,
        mark: snapshot.marks.get(&entry.rel),
    };
    let query = snapshot.query.as_str();
    if query.is_empty() {
        let mut shown = entries
            .into_iter()
            .filter(|entry| !entry.rel[base.len()..].trim_end_matches('/').contains('/'))
            .collect::<Vec<_>>();
        shown.sort_by(|left, right| {
            path_rank::browse_order(
                (left.rel.as_str(), left.kind == PathKind::Dir),
                (right.rel.as_str(), right.kind == PathKind::Dir),
            )
        });
        shown.truncate(PATH_PICKER_MAX_ROWS);
        return shown
            .into_iter()
            .map(|entry| row(entry, Vec::new()))
            .collect();
    }
    let labels = entries
        .iter()
        .map(|entry| entry.rel[base.len()..].trim_end_matches('/'))
        .collect::<Vec<_>>();
    let kept = path_rank::rank(
        query,
        &labels,
        |index| {
            path_rank::path_prior(
                query,
                labels[index],
                snapshot.marks.get(&entries[index].rel).is_some(),
            )
        },
        PATH_PICKER_MAX_ROWS,
    );
    let kept_labels = kept.iter().map(|index| labels[*index]).collect::<Vec<_>>();
    let highlights = path_rank::highlights(query, &kept_labels);
    kept.into_iter()
        .zip(highlights)
        .map(|(index, matches)| row(entries[index], matches))
        .collect()
}

fn parent_base(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    trimmed
        .rfind('/')
        .map_or_else(String::new, |index| trimmed[..=index].to_owned())
}

fn reroot_split(query: &str) -> Option<(&str, &str)> {
    if !(query.starts_with("~/") || query.starts_with('/') || query.starts_with("../")) {
        return None;
    }
    let cut = query.rfind('/')? + 1;
    Some((&query[..cut], &query[cut..]))
}

pub struct PathPickerView {
    backend: Rc<dyn PathPickerBackend>,
    input: Entity<InputState>,
    request: Option<u64>,
    root: Option<PathListRoot>,
    error: Option<SharedString>,
    base: String,
    chunks: Vec<Arc<[PathEntry]>>,
    done: bool,
    truncated: bool,
    marks: Arc<GitMarks>,
    query: String,
    rows: Arc<[PickerRow]>,
    shown_query: String,
    selected: usize,
    generation: u64,
    dirty: bool,
    ranking: Option<Task<()>>,
    scroll_handle: UniformListScrollHandle,
    finished: bool,
    typed_root: bool,
}

impl PathPickerView {
    pub fn new(
        backend: Rc<dyn PathPickerBackend>,
        start_dir: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search paths…"));
        cx.subscribe_in(
            &input,
            window,
            |picker, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    picker.query_changed(window, cx);
                }
            },
        )
        .detach();
        let request = backend.list(start_dir, cx);
        Self {
            backend,
            input,
            request,
            root: None,
            error: None,
            base: String::new(),
            chunks: Vec::new(),
            done: false,
            truncated: false,
            marks: Arc::default(),
            query: String::new(),
            rows: Arc::from([]),
            shown_query: String::new(),
            selected: 0,
            generation: 0,
            dirty: false,
            ranking: None,
            scroll_handle: UniformListScrollHandle::new(),
            finished: false,
            typed_root: false,
        }
    }

    pub const fn request_id(&self) -> Option<u64> {
        self.request
    }

    pub fn apply_begin(
        &mut self,
        request_id: u64,
        result: Result<PathListRoot, String>,
        cx: &mut Context<Self>,
    ) {
        if self.finished || self.request != Some(request_id) {
            return;
        }
        self.chunks.clear();
        self.marks = Arc::default();
        self.base.clear();
        self.truncated = false;
        self.rows = Arc::from([]);
        self.selected = 0;
        self.generation += 1;
        match result {
            Ok(root) => {
                self.root = Some(root);
                self.error = None;
                self.done = false;
            }
            Err(message) => {
                self.error = Some(message.into());
                self.done = true;
            }
        }
        self.refresh(cx);
        cx.notify();
    }

    pub fn apply_chunk(
        &mut self,
        request_id: u64,
        entries: Vec<PathEntry>,
        done: bool,
        truncated: bool,
        cx: &mut Context<Self>,
    ) {
        if self.finished || self.request != Some(request_id) {
            return;
        }
        if !entries.is_empty() {
            self.chunks.push(Arc::from(entries));
        }
        self.done |= done;
        self.truncated |= truncated;
        self.refresh(cx);
        cx.notify();
    }

    pub fn apply_git(
        &mut self,
        request_id: u64,
        marks: Vec<(String, GitMark)>,
        cx: &mut Context<Self>,
    ) {
        if self.finished || self.request != Some(request_id) {
            return;
        }
        let merged = Arc::make_mut(&mut self.marks);
        for (rel, mark) in marks {
            if rel.ends_with('/') {
                merged.dirs.push((rel, mark));
            } else {
                merged.exact.insert(rel, mark);
            }
        }
        self.refresh(cx);
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.ranking = None;
        if let Some(request) = self.request
            && !self.done
        {
            self.backend.cancel(request, cx);
        }
        cx.emit(PathPickerEvent::Dismissed);
        cx.notify();
    }

    fn request(&mut self, dir: &str, cx: &mut Context<Self>) {
        let previous = self.request;
        let Some(request) = self.backend.list(Some(dir), cx) else {
            return;
        };
        if let Some(previous) = previous
            && !self.done
        {
            self.backend.cancel(previous, cx);
        }
        self.request = Some(request);
        self.done = false;
        self.base.clear();
        self.error = None;
        self.chunks.clear();
        self.rows = Arc::from([]);
        self.selected = 0;
        self.generation += 1;
        cx.notify();
    }

    fn set_query(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value(value.to_owned(), window, cx);
        });
        value.trim().clone_into(&mut self.query);
    }

    fn query_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().to_string();
        if let Some((dir, rest)) = reroot_split(&value) {
            let dir = if dir.starts_with("../") {
                format!("{}{dir}", self.base)
            } else {
                dir.to_owned()
            };
            let rest = rest.to_owned();
            self.typed_root = true;
            self.set_query(&rest, window, cx);
            self.request(&dir, cx);
            return;
        }
        if self.typed_root
            && let Some(entry) = value.strip_suffix('/').and_then(|dir| self.named_dir(dir))
        {
            self.open_dir(&entry, window, cx);
            return;
        }
        value.trim().clone_into(&mut self.query);
        self.refresh(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        if self.ranking.is_none() {
            self.spawn_rank(cx);
        }
    }

    fn spawn_rank(&mut self, cx: &mut Context<Self>) {
        self.dirty = false;
        let generation = self.generation;
        let snapshot = Snapshot {
            chunks: self.chunks.clone(),
            base: self.base.clone(),
            query: self.query.clone(),
            marks: Arc::clone(&self.marks),
        };
        let ranking = cx.background_executor().spawn(async move {
            Ranked {
                rows: rank_rows(&snapshot),
                query: snapshot.query,
            }
        });
        self.ranking = Some(cx.spawn(async move |picker, cx| {
            let ranked = ranking.await;
            picker
                .update(cx, |picker, cx| picker.land(generation, ranked, cx))
                .ok();
        }));
    }

    fn land(&mut self, generation: u64, ranked: Ranked, cx: &mut Context<Self>) {
        self.ranking = None;
        if self.finished {
            return;
        }
        if generation == self.generation {
            let selected = if ranked.query == self.shown_query {
                self.rows.get(self.selected).and_then(|row| {
                    ranked
                        .rows
                        .iter()
                        .position(|candidate| candidate.entry.rel == row.entry.rel)
                })
            } else {
                None
            };
            self.selected = selected.unwrap_or(0);
            self.rows = ranked.rows;
            self.shown_query = ranked.query;
            self.scroll_handle
                .scroll_to_item(self.selected, ScrollStrategy::Nearest);
            cx.notify();
        }
        if self.dirty {
            self.spawn_rank(cx);
        }
    }

    fn navigate(&mut self, direction: isize, cx: &mut Context<Self>) {
        let count = self.rows.len();
        if count == 0 {
            return;
        }
        self.selected = if direction < 0 {
            self.selected.checked_sub(1).unwrap_or(count - 1)
        } else {
            (self.selected + 1) % count
        };
        self.scroll_handle
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn insert(&mut self, index: usize, absolute: bool, cx: &mut Context<Self>) {
        if self.finished {
            return;
        }
        let (Some(root), Some(row)) = (self.root.as_ref(), self.rows.get(index)) else {
            return;
        };
        self.backend.insert(root, &row.entry, absolute, cx);
        self.close(cx);
    }

    fn named_dir(&self, dir: &str) -> Option<PathEntry> {
        if dir.is_empty() {
            return None;
        }
        let rel = format!("{}{dir}", self.base);
        self.chunks
            .iter()
            .flat_map(|chunk| chunk.iter())
            .find(|entry| entry.kind == PathKind::Dir && entry.rel.trim_end_matches('/') == rel)
            .cloned()
    }

    fn enter_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self
            .rows
            .get(self.selected)
            .map(|row| row.entry.clone())
            .filter(|entry| entry.kind == PathKind::Dir)
        else {
            return;
        };
        self.open_dir(&entry, window, cx);
    }

    fn open_dir(&mut self, entry: &PathEntry, window: &mut Window, cx: &mut Context<Self>) {
        let base = format!("{}/", entry.rel.trim_end_matches('/'));
        let local = self.done
            && !self.truncated
            && self.error.is_none()
            && !entry.symlink
            && self
                .chunks
                .iter()
                .flat_map(|chunk| chunk.iter())
                .any(|candidate| {
                    candidate.rel.len() > base.len() && candidate.rel.starts_with(&base)
                });
        self.set_query("", window, cx);
        if local {
            self.enter_base(base, cx);
        } else {
            self.request(&entry.rel, cx);
        }
    }

    fn enter_base(&mut self, base: String, cx: &mut Context<Self>) {
        self.base = base;
        self.generation += 1;
        self.rows = Arc::from([]);
        self.selected = 0;
        self.refresh(cx);
        cx.notify();
    }

    fn prefix(&self) -> SharedString {
        let Some(root) = &self.root else {
            return SharedString::default();
        };
        let display = root.display_root.as_str();
        let separator = if display.ends_with('/') { "" } else { "/" };
        format!("{display}{separator}{}", self.base).into()
    }

    fn empty_message(&self) -> SharedString {
        if let Some(error) = &self.error {
            return error.clone();
        }
        if !self.done || self.ranking.is_some() {
            return "Listing…".into();
        }
        if self.query.is_empty() {
            "This folder is empty".into()
        } else {
            "No matches".into()
        }
    }

    fn complete(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        self.enter_dir(window, cx);
        cx.stop_propagation();
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(-1, cx);
        cx.stop_propagation();
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(1, cx);
        cx.stop_propagation();
    }

    fn leave_dir(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if !self.input.read(cx).value().is_empty() {
            return;
        }
        if self.base.is_empty() {
            self.request("..", cx);
        } else {
            let base = parent_base(&self.base);
            self.enter_base(base, cx);
        }
        cx.stop_propagation();
    }

    fn dismiss(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        cx.stop_propagation();
    }

    fn accept(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        self.insert(self.selected, false, cx);
        cx.stop_propagation();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        let plain_control =
            modifiers.control && !modifiers.alt && !modifiers.shift && !modifiers.platform;
        match event.keystroke.key.as_str() {
            "enter" if modifiers.alt && !modifiers.control && !modifiers.platform => {
                self.insert(self.selected, true, cx);
            }
            "n" if plain_control => self.navigate(1, cx),
            "p" if plain_control => self.navigate(-1, cx),
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    fn row(
        row: &PickerRow,
        index: usize,
        selected: bool,
        picker: Entity<Self>,
        cx: &App,
    ) -> gpui::Div {
        let hover = picker.clone();
        let click = picker;
        let muted_prefix = row.label.rfind('/').map_or(0, |index| index + 1);
        command_palette_entry(
            ("path-picker-row", index),
            &PaletteRow {
                label: row.label.clone(),
                matches: row.matches.clone(),
                muted_prefix,
                right: row.mark.map(mark_letter).unwrap_or_default().into(),
                icon: Some(match row.entry.kind {
                    PathKind::Dir => IconName::Folder,
                    PathKind::File => IconName::File,
                }),
                ..Default::default()
            },
            selected,
            cx,
        )
        .on_mouse_enter(move |_, _, cx| {
            hover.update(cx, |picker, cx| {
                if picker.selected != index {
                    picker.selected = index;
                    cx.notify();
                }
            });
        })
        .on_click(move |_, _, cx| {
            click.update(cx, |picker, cx| picker.insert(index, false, cx));
            cx.stop_propagation();
        })
        .map(|row| div().h(px(COMMAND_PALETTE_ROW_HEIGHT)).child(row))
    }
}

impl Focusable for PathPickerView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for PathPickerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Arc::clone(&self.rows);
        let selected = self.selected;
        let picker = cx.entity();
        let visible = rows.len().min(PATH_PICKER_VISIBLE_ROWS);
        let visible = f32::from(u8::try_from(visible).unwrap_or(u8::MAX));
        let list = uniform_list(
            "path-picker-rows",
            rows.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        rows.get(index)
                            .map(|row| Self::row(row, index, selected == index, picker.clone(), cx))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .h(px(COMMAND_PALETTE_ROW_HEIGHT * visible))
        .track_scroll(&self.scroll_handle);
        let input = command_palette_input(
            &self.input,
            self.prefix(),
            cx.theme().mono_font_family.clone(),
            cx,
        );
        let hints = [
            PaletteHint {
                key: "up down",
                label: "navigate",
            },
            PaletteHint {
                key: "tab",
                label: "open folder",
            },
            PaletteHint {
                key: "enter",
                label: "insert",
            },
            PaletteHint {
                key: "alt-enter",
                label: "absolute",
            },
            PaletteHint {
                key: "escape",
                label: "close",
            },
        ];
        let surface = CommandPaletteSurface::new(input, cx.entity_id().as_u64()).hints(hints);
        let surface = if self.rows.is_empty() {
            surface.rows(
                div()
                    .debug_selector(|| "path-picker-empty".to_owned())
                    .py(px(16.0))
                    .text_center()
                    .text_size(crate::rems_from_px(12.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(self.empty_message()),
            )
        } else {
            surface.rows(list)
        };
        div()
            .id("path-picker")
            .debug_selector(|| "path-picker".to_owned())
            .w(px(COMMAND_PALETTE_MAX_WIDTH))
            .occlude()
            .capture_action(cx.listener(Self::complete))
            .capture_action(cx.listener(Self::move_up))
            .capture_action(cx.listener(Self::move_down))
            .capture_action(cx.listener(Self::leave_dir))
            .capture_action(cx.listener(Self::dismiss))
            .capture_action(cx.listener(Self::accept))
            .capture_key_down(cx.listener(Self::on_key_down))
            .on_key_up(|_, _, cx| cx.stop_propagation())
            .child(surface)
    }
}

#[cfg(test)]
#[path = "path_picker_tests.rs"]
mod tests;
