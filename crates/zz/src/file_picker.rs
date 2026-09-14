//! Shared in-app fuzzy path picker.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(any(feature = "editor-pane", test))]
use fff_search::FilePickerOptions;
use fff_search::{FilePicker, FuzzySearchOptions, PaginationArgs, QueryParser};
#[cfg(any(feature = "editor-pane", test))]
use gpui::{
    App, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton,
    Render, ScrollStrategy, Subscription, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use gpui::{Context, SharedString, Task};
use ignore::WalkBuilder;
#[cfg(any(feature = "editor-pane", test))]
use zz_ui::command::palette_shortcut_hint;
#[cfg(any(feature = "editor-pane", test))]
use zz_ui::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, IconName, h_flex,
    input::{InputEvent, InputState, MoveDown, MoveUp},
    scroll::ScrollableElement as _,
    v_flex,
};

const MAX_PICKER_ROWS: usize = 500;
const WORKSPACE_PRIOR: u32 = 1 << 10;
const DIRECTORY_SCAN_LIMIT: usize = 50_000;
const DIRECTORY_SCAN_DEPTH: usize = 8;
const DIRECTORY_SCAN_BUDGET: Duration = Duration::from_secs(5);
const DIRECTORY_BATCH_SIZE: usize = 64;

#[cfg(any(feature = "editor-pane", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FilePickerMode {
    #[cfg_attr(
        not(feature = "editor-pane"),
        allow(
            dead_code,
            reason = "only the editor pane constructs a file-mode picker"
        )
    )]
    Files,
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the agent picker uses DirectoryCatalog directly")
    )]
    Directories,
}

#[cfg(any(feature = "editor-pane", test))]
impl FilePickerMode {
    const fn icon(self) -> IconName {
        match self {
            Self::Files => IconName::File,
            Self::Directories => IconName::Folder,
        }
    }

    const fn empty_label(self) -> &'static str {
        match self {
            Self::Files => "No files under this folder.",
            Self::Directories => "No folders under this folder.",
        }
    }
}

#[cfg(any(feature = "editor-pane", test))]
#[derive(Clone, Debug)]
#[cfg_attr(
    not(feature = "editor-pane"),
    allow(dead_code, reason = "only the editor pane consumes picker events")
)]
pub(crate) enum FilePickerEvent {
    Selected(PathBuf),
    Dismissed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PickerEntry {
    relative: SharedString,
    absolute: Arc<Path>,
    prior: u32,
}

fn entry_prior(depth: usize, is_workspace: bool) -> u32 {
    let shallowness = u32::try_from(depth).map_or(0, |depth| 64_u32.saturating_sub(depth));
    if is_workspace {
        WORKSPACE_PRIOR + shallowness
    } else {
        shallowness
    }
}

#[cfg(all(target_os = "macos", feature = "agent-pane"))]
fn home_directory() -> Option<PathBuf> {
    let home = objc2_foundation::NSHomeDirectory().to_string();
    (!home.is_empty()).then(|| PathBuf::from(home))
}

#[cfg(all(not(target_os = "macos"), feature = "agent-pane"))]
fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

#[cfg(feature = "agent-pane")]
pub(crate) fn directory_picker_root(fallback: &Path) -> PathBuf {
    match home_directory() {
        Some(home) if fallback.starts_with(&home) => home,
        _ => fallback.parent().unwrap_or(fallback).to_path_buf(),
    }
}

fn scan_directories(
    root: &Path,
    limit: usize,
    budget: Duration,
    cancelled: &dyn Fn() -> bool,
    emit: &mut dyn FnMut(Vec<PickerEntry>) -> bool,
) -> Result<bool, String> {
    if cancelled() {
        return Ok(false);
    }
    std::fs::read_dir(root).map_err(|error| format!("Cannot read {}: {error}", root.display()))?;
    let start = Instant::now();
    let mut last_batch = start;
    let mut pending = VecDeque::from([(root.to_path_buf(), 0)]);
    let mut batch = Vec::new();
    let mut limited = false;
    let mut published = false;
    let mut visited = 0;
    'scan: while let Some((directory, depth)) = pending.pop_front() {
        if cancelled() {
            return Ok(false);
        }
        if visited >= limit || start.elapsed() >= budget {
            limited = true;
            break;
        }
        let mut builder = WalkBuilder::new(&directory);
        builder
            .follow_links(false)
            .require_git(false)
            .max_depth(Some(1));
        for result in builder.build() {
            if cancelled() {
                return Ok(false);
            }
            if visited >= limit || start.elapsed() >= budget {
                limited = true;
                break 'scan;
            }
            let Ok(entry) = result else { continue };
            if entry.depth() == 0 {
                continue;
            }
            visited += 1;
            if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
                continue;
            }
            let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
            let name = entry.file_name().to_str().unwrap_or_default();
            if matches!(name, "node_modules" | "target" | "__pycache__" | "venv")
                || relative.ends_with("go/pkg/mod")
            {
                continue;
            }
            let Some(label) = relative.to_str() else {
                continue;
            };
            batch.push(PickerEntry {
                relative: SharedString::from(label.to_owned()),
                absolute: Arc::from(entry.path()),
                prior: entry_prior(depth + 1, entry.path().join(".git").exists()),
            });
            let media_root = depth == 0
                && matches!(
                    name,
                    "Applications"
                        | "Library"
                        | "Movies"
                        | "Music"
                        | "Pictures"
                        | "Public"
                        | "Templates"
                        | "Videos"
                        | "snap"
                );
            if depth + 1 < DIRECTORY_SCAN_DEPTH && !media_root {
                pending.push_back((entry.path().to_path_buf(), depth + 1));
            }
            if !published
                || batch.len() >= DIRECTORY_BATCH_SIZE
                || last_batch.elapsed() >= Duration::from_millis(30)
            {
                if !emit(std::mem::take(&mut batch)) {
                    return Ok(false);
                }
                published = true;
                last_batch = Instant::now();
            }
        }
    }
    if !batch.is_empty() {
        emit(batch);
    }
    Ok(limited)
}

struct PickerIndex {
    picker: Option<FilePicker>,
    directories: Vec<PickerEntry>,
}

impl PickerIndex {
    #[cfg(any(feature = "editor-pane", test))]
    fn files(root: &Path) -> Result<Self, String> {
        std::fs::read_dir(root)
            .map_err(|error| format!("Cannot read {}: {error}", root.display()))?;
        let base_path = root
            .to_str()
            .ok_or_else(|| format!("Cannot search a non-UTF-8 path: {}", root.display()))?;
        let mut picker = FilePicker::new(FilePickerOptions {
            base_path: base_path.to_owned(),
            watch: false,
            enable_home_dir_scanning: true,
            enable_fs_root_scanning: true,
            ..FilePickerOptions::default()
        })
        .map_err(|error| format!("Cannot search {}: {error}", root.display()))?;
        picker
            .collect_files()
            .map_err(|error| format!("Cannot search {}: {error}", root.display()))?;
        Ok(Self {
            picker: Some(picker),
            directories: Vec::new(),
        })
    }

    fn search(&self, query: &str) -> Vec<PickerEntry> {
        if let Some(picker) = &self.picker {
            let parser = QueryParser::default();
            let query = parser.parse(query);
            return picker
                .fuzzy_search(
                    &query,
                    None,
                    FuzzySearchOptions {
                        max_threads: 2,
                        pagination: PaginationArgs {
                            offset: 0,
                            limit: MAX_PICKER_ROWS,
                        },
                        ..FuzzySearchOptions::default()
                    },
                )
                .items
                .iter()
                .map(|file| {
                    let relative = file.relative_path(picker);
                    PickerEntry {
                        absolute: Arc::from(picker.base_path().join(&relative)),
                        relative: SharedString::from(relative),
                        prior: 0,
                    }
                })
                .collect();
        }
        let query = query.trim().trim_end_matches(['/', '\\']);
        let mut ranked = if query.is_empty() {
            self.directories
                .iter()
                .enumerate()
                .map(|(index, _)| (index, 0))
                .collect::<Vec<_>>()
        } else {
            let candidates = self
                .directories
                .iter()
                .map(|entry| entry.relative.as_ref())
                .collect::<Vec<_>>();
            let config = neo_frizbee::Config {
                max_typos: Some(u16::try_from(query.chars().count() / 4).unwrap_or(6).min(6)),
                casing: neo_frizbee::CaseMatching::Smart,
                sort: false,
                ..neo_frizbee::Config::default()
            };
            neo_frizbee::match_list(query, &candidates, &config)
                .into_iter()
                .map(|matched| (matched.index as usize, matched.score))
                .collect()
        };
        ranked.sort_unstable_by(|(left, left_score), (right, right_score)| {
            right_score
                .cmp(left_score)
                .then(
                    self.directories[*right]
                        .prior
                        .cmp(&self.directories[*left].prior),
                )
                .then(
                    self.directories[*left]
                        .relative
                        .cmp(&self.directories[*right].relative),
                )
        });
        ranked
            .into_iter()
            .take(MAX_PICKER_ROWS)
            .map(|(index, _)| self.directories[index].clone())
            .collect()
    }
}

#[cfg(feature = "agent-pane")]
pub(crate) struct DirectoryCatalog {
    index: Arc<PickerIndex>,
    pub(crate) entries: Vec<(SharedString, PathBuf)>,
    pub(crate) scanning: bool,
    pub(crate) limited: bool,
    pub(crate) error: Option<SharedString>,
    query: String,
    generation: u64,
    _walk: Task<()>,
    _rank: Task<()>,
}

#[cfg(feature = "agent-pane")]
impl DirectoryCatalog {
    pub(crate) fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        let (sender, receiver) = async_channel::bounded(8);
        cx.background_executor()
            .spawn(async move {
                let result = scan_directories(
                    &root,
                    DIRECTORY_SCAN_LIMIT,
                    DIRECTORY_SCAN_BUDGET,
                    &|| sender.is_closed(),
                    &mut |batch| {
                        sender
                            .send_blocking(ScanMessage::Directories(batch))
                            .is_ok()
                    },
                );
                sender.send_blocking(ScanMessage::Finished(result)).ok();
            })
            .detach();
        let walk = cx.spawn(async move |catalog, cx| {
            while let Ok(message) = receiver.recv().await {
                if catalog
                    .update(cx, |catalog, cx| {
                        match message {
                            ScanMessage::Directories(batch) => {
                                let mut directories = catalog.index.directories.clone();
                                directories.extend(batch);
                                catalog.index = Arc::new(PickerIndex {
                                    picker: None,
                                    directories,
                                });
                                catalog.rank(cx);
                            }
                            ScanMessage::Finished(result) => {
                                catalog.scanning = false;
                                match result {
                                    Ok(limited) => catalog.limited = limited,
                                    Err(error) => catalog.error = Some(error.into()),
                                }
                            }
                            #[cfg(any(feature = "editor-pane", test))]
                            ScanMessage::Files(_) => {}
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            index: Arc::new(PickerIndex {
                picker: None,
                directories: Vec::new(),
            }),
            entries: Vec::new(),
            scanning: true,
            limited: false,
            error: None,
            query: String::new(),
            generation: 0,
            _walk: walk,
            _rank: Task::ready(()),
        }
    }

    pub(crate) fn search(&mut self, query: &str, cx: &mut Context<Self>) {
        query.clone_into(&mut self.query);
        self.entries.clear();
        self.rank(cx);
        cx.notify();
    }

    fn rank(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let index = self.index.clone();
        let query = self.query.clone();
        let ranking = cx.background_executor().spawn(async move {
            let mut entries = index
                .search(&query)
                .into_iter()
                .map(|entry| (entry.relative, entry.absolute.to_path_buf()))
                .collect::<Vec<_>>();
            let typed = PathBuf::from(query.trim());
            if typed.is_absolute()
                && typed.is_dir()
                && !entries.iter().any(|(_, path)| *path == typed)
            {
                entries.insert(0, (typed.display().to_string().into(), typed));
            }
            entries
        });
        self._rank = cx.spawn(async move |catalog, cx| {
            let entries = ranking.await;
            catalog
                .update(cx, |catalog, cx| {
                    if catalog.generation == generation {
                        catalog.entries = entries;
                        cx.notify();
                    }
                })
                .ok();
        });
    }
}

enum ScanMessage {
    Directories(Vec<PickerEntry>),
    #[cfg(any(feature = "editor-pane", test))]
    Files(Arc<PickerIndex>),
    Finished(Result<bool, String>),
}

#[cfg(any(feature = "editor-pane", test))]
fn preserved_selection(entries: &[PickerEntry], preferred: Option<&Path>) -> Option<usize> {
    preferred
        .and_then(|path| {
            entries
                .iter()
                .position(|entry| entry.absolute.as_ref() == path)
        })
        .or_else(|| (!entries.is_empty()).then_some(0))
}

#[cfg(any(feature = "editor-pane", test))]
fn validate_selection(path: &Path, mode: FilePickerMode) -> Result<(), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
    if match mode {
        FilePickerMode::Files => metadata.is_file(),
        FilePickerMode::Directories => metadata.is_dir(),
    } {
        Ok(())
    } else {
        Err(format!(
            "{} is not a {}.",
            path.display(),
            match mode {
                FilePickerMode::Files => "file",
                FilePickerMode::Directories => "folder",
            }
        ))
    }
}

#[cfg(any(feature = "editor-pane", test))]
pub(crate) struct FilePickerView {
    mode: FilePickerMode,
    input: Entity<InputState>,
    entries: Vec<PickerEntry>,
    index: Option<Arc<PickerIndex>>,
    rows: Arc<[SharedString]>,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    query: String,
    scanning: bool,
    limited: bool,
    error: Option<SharedString>,
    rank_generation: u64,
    accept_generation: u64,
    _rank: Task<()>,
    _accept: Task<()>,
    _walk: Task<()>,
    _subscriptions: Vec<Subscription>,
}

#[cfg(any(feature = "editor-pane", test))]
impl FilePickerView {
    pub(crate) fn new(
        mode: FilePickerMode,
        root: PathBuf,
        prompt: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(prompt));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |picker, input, event: &InputEvent, _, cx| match event {
                InputEvent::Change => picker.on_query_changed(input, cx),
                InputEvent::PressEnter { .. } => picker.accept_selected(cx),
                _ => {}
            },
        );
        input.read(cx).focus_handle(cx).focus(window, cx);
        Self {
            mode,
            input,
            entries: Vec::new(),
            index: None,
            rows: Arc::from([]),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            query: String::new(),
            scanning: true,
            limited: false,
            error: None,
            rank_generation: 0,
            accept_generation: 0,
            _rank: Task::ready(()),
            _accept: Task::ready(()),
            _walk: Self::spawn_walk(mode, root, cx),
            _subscriptions: vec![subscription],
        }
    }

    pub(crate) fn focus(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    fn spawn_walk(mode: FilePickerMode, root: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        let (sender, receiver) = async_channel::bounded(8);
        cx.background_executor()
            .spawn(async move {
                let result = match mode {
                    FilePickerMode::Directories => scan_directories(
                        &root,
                        DIRECTORY_SCAN_LIMIT,
                        DIRECTORY_SCAN_BUDGET,
                        &|| sender.is_closed(),
                        &mut |batch| {
                            sender
                                .send_blocking(ScanMessage::Directories(batch))
                                .is_ok()
                        },
                    ),
                    FilePickerMode::Files => PickerIndex::files(&root).map(|index| {
                        sender
                            .send_blocking(ScanMessage::Files(Arc::new(index)))
                            .ok();
                        false
                    }),
                };
                sender.send_blocking(ScanMessage::Finished(result)).ok();
            })
            .detach();
        cx.spawn(async move |picker, cx| {
            while let Ok(message) = receiver.recv().await {
                if picker
                    .update(cx, |picker, cx| picker.apply_scan(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    fn apply_scan(&mut self, message: ScanMessage, cx: &mut Context<Self>) {
        match message {
            ScanMessage::Directories(batch) => {
                let mut directories = self
                    .index
                    .as_ref()
                    .map(|index| index.directories.clone())
                    .unwrap_or_default();
                directories.extend(batch);
                self.index = Some(Arc::new(PickerIndex {
                    picker: None,
                    directories,
                }));
                if self.rows.is_empty() && self.query.trim().is_empty() {
                    self.set_entries(self.index.as_ref().unwrap().search(""), None);
                }
                self.search(false, cx);
            }
            ScanMessage::Files(index) => {
                self.index = Some(index);
                self.search(false, cx);
            }
            ScanMessage::Finished(result) => {
                self.scanning = false;
                match result {
                    Ok(limited) => self.limited = limited,
                    Err(error) => self.error = Some(SharedString::from(error)),
                }
            }
        }
        cx.notify();
    }

    fn on_query_changed(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let query = input.read(cx).value().to_string();
        if query == self.query {
            return;
        }
        self.query = query;
        if self.scanning || self.index.is_some() {
            self.error = None;
        }
        self.accept_generation = self.accept_generation.saturating_add(1);
        self.search(true, cx);
    }

    fn search(&mut self, clear: bool, cx: &mut Context<Self>) {
        let Some(index) = self.index.clone() else {
            return;
        };
        let preferred = self.selected_path();
        if clear {
            self.entries.clear();
            self.rows = Arc::from([]);
            self.selected = None;
        }
        self.rank_generation = self.rank_generation.saturating_add(1);
        let generation = self.rank_generation;
        let query = self.query.clone();
        let ranking = cx
            .background_executor()
            .spawn(async move { index.search(&query) });
        self._rank = cx.spawn(async move |picker, cx| {
            let entries = ranking.await;
            picker
                .update(cx, |picker, cx| {
                    if picker.rank_generation != generation {
                        return;
                    }
                    let preferred = picker.selected_path().or(preferred);
                    picker.set_entries(entries, preferred.as_deref());
                    cx.notify();
                })
                .ok();
        });
        cx.notify();
    }

    fn set_entries(&mut self, entries: Vec<PickerEntry>, preferred: Option<&Path>) {
        self.rows = entries.iter().map(|entry| entry.relative.clone()).collect();
        self.selected = preserved_selection(&entries, preferred);
        self.entries = entries;
        if let Some(selected) = self.selected {
            self.scroll
                .scroll_to_item(selected, ScrollStrategy::Nearest);
        }
    }

    fn selected_path(&self) -> Option<Arc<Path>> {
        self.entries
            .get(self.selected?)
            .map(|entry| entry.absolute.clone())
    }

    fn navigate(&mut self, direction: isize, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            return;
        }
        let count = self.entries.len();
        let current = self.selected.unwrap_or_default();
        let selected = if direction < 0 {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        };
        self.selected = Some(selected);
        self.scroll
            .scroll_to_item(selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn accept(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        self.accept_path(entry.absolute.to_path_buf(), cx);
    }

    fn accept_selected(&mut self, cx: &mut Context<Self>) {
        let typed = PathBuf::from(self.query.trim());
        if typed.is_absolute() {
            self.accept_path(typed, cx);
        } else if let Some(index) = self.selected {
            self.accept(index, cx);
        }
    }

    fn accept_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.accept_generation = self.accept_generation.saturating_add(1);
        let generation = self.accept_generation;
        let mode = self.mode;
        let validation = cx.background_executor().spawn({
            let path = path.clone();
            async move { validate_selection(&path, mode) }
        });
        self._accept = cx.spawn(async move |picker, cx| {
            let result = validation.await;
            picker
                .update(cx, |picker, cx| {
                    if picker.accept_generation != generation {
                        return;
                    }
                    match result {
                        Ok(()) => cx.emit(FilePickerEvent::Selected(path)),
                        Err(error) => {
                            picker.error = Some(SharedString::from(error));
                            cx.notify();
                        }
                    }
                })
                .ok();
        });
    }

    #[allow(
        clippy::unused_self,
        reason = "kept as a method so the backdrop and Escape share one call shape"
    )]
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(FilePickerEvent::Dismissed);
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(-1, cx);
        cx.stop_propagation();
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(1, cx);
        cx.stop_propagation();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key.as_str() == "escape" {
            self.dismiss(cx);
            cx.stop_propagation();
        }
    }

    fn render_rows(&self, cx: &Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let selected = self.selected;
        let icon = self.mode.icon();
        let view = cx.entity();
        let list = uniform_list("file-picker-rows", rows.len(), move |range, _, cx| {
            range
                .filter_map(|index| {
                    let label = rows.get(index)?.clone();
                    let is_selected = selected == Some(index);
                    let pointer_view = view.clone();
                    let click_view = view.clone();
                    Some(
                        zz_ui::picker::path_row(
                            ("file-picker-row", index),
                            icon.clone(),
                            label,
                            is_selected,
                            cx,
                        )
                        .on_mouse_move(move |_, _, cx| {
                            pointer_view.update(cx, |picker, cx| {
                                if picker.selected != Some(index) {
                                    picker.selected = Some(index);
                                    cx.notify();
                                }
                            });
                        })
                        .on_click(move |_, _, cx| {
                            click_view.update(cx, |picker, cx| picker.accept(index, cx));
                            cx.stop_propagation();
                        }),
                    )
                })
                .collect::<Vec<_>>()
        })
        .size_full()
        .track_scroll(&self.scroll);
        div()
            .flex_1()
            .min_h_0()
            .child(list)
            .vertical_scrollbar(&self.scroll)
    }

    fn render_notes(&self, cx: &Context<Self>) -> impl IntoElement {
        let listed = !self.rows.is_empty();
        h_flex()
            .w_full()
            .flex_none()
            .gap(px(CHROME_GAP))
            .px_2p5()
            .text_size(zz_ui::rems_from_px(10.0))
            .text_color(cx.theme().foreground.muted())
            .when(listed && self.scanning, |this| this.child("Scanning…"))
            .when(self.limited, |this| {
                this.child("Search limited. Enter a full path to open another folder.")
            })
    }

    fn empty_message(&self, cx: &App) -> SharedString {
        if self.scanning {
            return SharedString::from("Scanning…");
        }
        if let Some(error) = &self.error {
            return error.clone();
        }
        if self.input.read(cx).value().is_empty() {
            SharedString::from(self.mode.empty_label())
        } else {
            SharedString::from("Nothing matches that search.")
        }
    }
}

#[cfg(any(feature = "editor-pane", test))]
impl EventEmitter<FilePickerEvent> for FilePickerView {}

#[cfg(any(feature = "editor-pane", test))]
impl Focusable for FilePickerView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.focus(cx)
    }
}

#[cfg(any(feature = "editor-pane", test))]
impl Render for FilePickerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus(cx);
        let backdrop_view = cx.entity();
        let empty = self.rows.is_empty();
        let empty_message = self.empty_message(cx);
        zz_ui::picker::picker_overlay("file-picker-overlay", cx)
            .track_focus(&focus)
            .capture_action(cx.listener(Self::move_up))
            .capture_action(cx.listener(Self::move_down))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                backdrop_view.update(cx, FilePickerView::dismiss);
                cx.stop_propagation();
            })
            .child(
                zz_ui::picker::picker_modal("file-picker-modal", cx)
                    .child(
                        zz_ui::picker::picker_header(cx)
                            .child(zz_ui::picker::picker_search(&self.input, cx)),
                    )
                    .child(
                        v_flex()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .p(px(CHROME_GAP))
                            .child(self.render_rows(cx))
                            .child(self.render_notes(cx))
                            .when(empty, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(zz_ui::rems_from_px(11.0))
                                        .text_color(cx.theme().foreground.muted())
                                        .child(empty_message),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .min_h(px(40.0))
                            .flex_none()
                            .items_center()
                            .gap(px(CHROME_GAP))
                            .border_t_1()
                            .border_color(cx.theme().border())
                            .py(px(CHROME_GAP))
                            .pl_4()
                            .pr(px(CHROME_GAP))
                            .text_size(zz_ui::rems_from_px(10.0))
                            .text_color(cx.theme().foreground.muted())
                            .child(palette_shortcut_hint(["up", "down"], "select"))
                            .child(palette_shortcut_hint(["enter"], "open"))
                            .child(palette_shortcut_hint(["escape"], "close"))
                            .child(div().flex_1())
                            .when_some(self.error.clone(), |this, error| {
                                this.child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(error),
                                )
                            }),
                    ),
            )
    }
}
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn build_index(root: &Path, mode: FilePickerMode) -> Result<PickerIndex, String> {
        if mode == FilePickerMode::Files {
            return PickerIndex::files(root);
        }
        let mut directories = Vec::new();
        scan_directories(
            root,
            DIRECTORY_SCAN_LIMIT,
            DIRECTORY_SCAN_BUDGET,
            &|| false,
            &mut |batch| {
                directories.extend(batch);
                true
            },
        )?;
        Ok(PickerIndex {
            picker: None,
            directories,
        })
    }

    fn labels(entries: &[PickerEntry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.relative.as_ref())
            .collect()
    }

    #[test]
    fn directories_include_empty_and_deep_folders_but_not_files() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("dev/empty")).unwrap();
        fs::create_dir_all(root.path().join("a/b/c/d/e/f/g/workspace")).unwrap();
        fs::write(root.path().join("dev/file.rs"), "").unwrap();
        let index = build_index(root.path(), FilePickerMode::Directories).unwrap();

        let entries = index.search("");

        assert!(labels(&entries).contains(&"dev/empty"));
        assert!(labels(&entries).contains(&"a/b/c/d/e/f/g/workspace"));
        assert!(entries.iter().all(|entry| entry.absolute.is_dir()));
        assert!(
            entries
                .iter()
                .all(|entry| entry.absolute.starts_with(root.path()))
        );
    }

    #[test]
    fn empty_directory_query_puts_workspaces_then_shallow_folders_first() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("dev/zz/.git")).unwrap();
        fs::create_dir_all(root.path().join("other/deeper/folder")).unwrap();
        fs::create_dir(root.path().join("Desktop")).unwrap();
        let index = build_index(root.path(), FilePickerMode::Directories).unwrap();

        let entries = index.search("  ");
        let names = labels(&entries);

        assert_eq!(names[0], "dev/zz");
        assert!(
            names.iter().position(|name| *name == "Desktop")
                < names.iter().position(|name| *name == "other/deeper/folder")
        );
    }

    #[test]
    fn fff_directory_search_tolerates_typos_and_trailing_separators() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("dev/terminal-workspace")).unwrap();
        fs::create_dir_all(root.path().join("Documents/recipes")).unwrap();
        let index = build_index(root.path(), FilePickerMode::Directories).unwrap();

        for query in ["termnal-workspace", "terminal-workspace/"] {
            let entries = index.search(query);
            assert_eq!(
                labels(&entries).first(),
                Some(&"dev/terminal-workspace"),
                "{query}"
            );
        }
    }

    #[test]
    fn file_search_honors_gitignore_and_prefers_filename_matches() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::create_dir(root.path().join("readme")).unwrap();
        fs::write(root.path().join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(root.path().join("ignored.rs"), "").unwrap();
        fs::write(root.path().join("docs/readme.md"), "").unwrap();
        fs::write(root.path().join("readme/notes.md"), "").unwrap();
        let index = build_index(root.path(), FilePickerMode::Files).unwrap();

        let entries = index.search("readme");

        assert_eq!(labels(&entries).first(), Some(&"docs/readme.md"));
        assert!(
            index.search("").iter().all(|entry| {
                entry.absolute.is_file() && entry.relative.as_ref() != "ignored.rs"
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_child_links_do_not_fail_a_readable_root() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("workspace")).unwrap();
        std::os::unix::fs::symlink(root.path().join("missing"), root.path().join("broken"))
            .unwrap();

        let index = build_index(root.path(), FilePickerMode::Directories).unwrap();

        assert_eq!(labels(&index.search("")), ["workspace"]);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_children_do_not_turn_results_into_a_scan_error() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempfile::tempdir().unwrap();
        let protected = root.path().join("protected");
        fs::create_dir_all(protected.join("secret")).unwrap();
        fs::create_dir(root.path().join("workspace")).unwrap();
        fs::set_permissions(&protected, fs::Permissions::from_mode(0o0)).unwrap();
        let unreadable = fs::read_dir(&protected).is_err();

        let result = build_index(root.path(), FilePickerMode::Directories);

        fs::set_permissions(&protected, fs::Permissions::from_mode(0o700)).unwrap();
        let index = result.unwrap();
        let entries = index.search("");
        assert!(labels(&entries).contains(&"workspace"));
        if unreadable {
            assert!(!labels(&entries).contains(&"protected/secret"));
        }
    }

    #[test]
    fn missing_root_reports_the_path_that_failed() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");

        let error = build_index(&missing, FilePickerMode::Directories)
            .err()
            .unwrap();

        assert!(error.starts_with("Cannot read "));
        assert!(error.contains(missing.to_str().unwrap()));
    }

    #[test]
    fn selections_revalidate_deleted_paths_and_the_requested_kind() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workspace");
        fs::create_dir(&folder).unwrap();
        let index = build_index(root.path(), FilePickerMode::Directories).unwrap();
        let entries = index.search("");
        let selected = &entries[0].absolute;
        assert!(validate_selection(selected, FilePickerMode::Directories).is_ok());
        assert!(validate_selection(selected, FilePickerMode::Files).is_err());
        fs::remove_dir(&folder).unwrap();

        let error = validate_selection(selected, FilePickerMode::Directories).unwrap_err();

        assert!(error.contains(folder.to_str().unwrap()));
    }

    #[test]
    fn results_are_capped_and_selection_follows_the_path() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..MAX_PICKER_ROWS + 10 {
            fs::create_dir(root.path().join(format!("folder{index:04}"))).unwrap();
        }
        let index = build_index(root.path(), FilePickerMode::Directories).unwrap();
        let mut entries = index.search("folder");
        assert_eq!(entries.len(), MAX_PICKER_ROWS);
        let selected = entries[1].absolute.clone();
        entries.swap(0, 1);

        assert_eq!(preserved_selection(&entries, Some(&selected)), Some(0));
        assert_eq!(preserved_selection(&entries, Some(root.path())), Some(0));
        assert_eq!(preserved_selection(&[], Some(&selected)), None);
    }

    #[test]
    fn first_batch_is_searchable_before_the_walk_finishes_and_can_stop_it() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..2_000 {
            fs::create_dir_all(root.path().join(format!("workspace{index}/src"))).unwrap();
        }
        let mut batches = 0;
        scan_directories(
            root.path(),
            DIRECTORY_SCAN_LIMIT,
            DIRECTORY_SCAN_BUDGET,
            &|| false,
            &mut |batch| {
                batches += 1;
                assert_eq!(batch.len(), 1);
                let index = PickerIndex {
                    picker: None,
                    directories: batch,
                };
                assert!(!index.search("workspace").is_empty());
                false
            },
        )
        .unwrap();
        assert_eq!(batches, 1);
    }

    #[test]
    fn discovery_honors_entry_time_and_cancellation_limits() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..20 {
            fs::create_dir(root.path().join(format!("folder{index}"))).unwrap();
        }
        let mut found = Vec::new();
        let limited = scan_directories(
            root.path(),
            3,
            DIRECTORY_SCAN_BUDGET,
            &|| false,
            &mut |batch| {
                found.extend(batch);
                true
            },
        )
        .unwrap();
        assert!(limited);
        assert_eq!(found.len(), 3);
        assert!(
            scan_directories(
                root.path(),
                DIRECTORY_SCAN_LIMIT,
                Duration::ZERO,
                &|| false,
                &mut |_| panic!("expired scan published results")
            )
            .unwrap()
        );
        assert!(
            !scan_directories(
                &root.path().join("missing"),
                DIRECTORY_SCAN_LIMIT,
                DIRECTORY_SCAN_BUDGET,
                &|| true,
                &mut |_| panic!("cancelled scan published results")
            )
            .unwrap()
        );
    }

    #[test]
    fn directory_discovery_prunes_caches_media_and_excess_depth() {
        let root = tempfile::tempdir().unwrap();
        for folder in [
            "Library/cache/hidden",
            "Music/Media/hidden",
            "dev/project/node_modules/package",
            "dev/project/target/debug",
            "dev/project/src",
            "go/pkg/mod/cache",
            "a/b/c/d/e/f/g/h/i",
        ] {
            fs::create_dir_all(root.path().join(folder)).unwrap();
        }
        let index = build_index(root.path(), FilePickerMode::Directories).unwrap();
        let entries = index.search("");
        let names = labels(&entries);
        assert!(names.contains(&"Library"));
        assert!(names.contains(&"Music"));
        assert!(names.contains(&"dev/project/src"));
        assert!(!names.iter().any(|name| name.contains("cache")
            || name.contains("Media")
            || name.contains("node_modules")
            || name.contains("target")));
        assert!(!names.contains(&"a/b/c/d/e/f/g/h/i"));
    }

    #[gpui::test]
    fn streamed_rows_stay_visible_before_scan_completion(cx: &mut gpui::TestAppContext) {
        use std::{cell::RefCell, rc::Rc};
        cx.update(zz_ui::init);
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_path_buf();
        let slot = Rc::new(RefCell::new(None));
        let captured = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let picker = cx.new(|cx| {
                let mut picker = FilePickerView::new(
                    FilePickerMode::Directories,
                    root_path,
                    "Folder",
                    window,
                    cx,
                );
                picker._walk = Task::ready(());
                picker
            });
            captured.replace(Some(picker.clone()));
            zz_ui::Root::new(picker, window, cx)
        });
        let picker = slot.borrow().clone().unwrap();
        cx.update(|_, cx| {
            picker.update(cx, |picker, cx| {
                let entry = PickerEntry {
                    relative: "workspace".into(),
                    absolute: Arc::from(root.path().join("workspace")),
                    prior: 0,
                };
                picker.apply_scan(ScanMessage::Directories(vec![entry]), cx);
                assert!(picker.scanning);
                assert_eq!(picker.rows.as_ref(), &[SharedString::from("workspace")]);
                assert_eq!(picker.selected, Some(0));
                let other = PickerEntry {
                    relative: "another".into(),
                    absolute: Arc::from(root.path().join("another")),
                    prior: 0,
                };
                picker.apply_scan(ScanMessage::Directories(vec![other]), cx);
                assert_eq!(picker.rows.as_ref(), &[SharedString::from("workspace")]);
                assert!(picker.scanning);
                picker.apply_scan(ScanMessage::Finished(Ok(true)), cx);
                assert!(!picker.scanning);
                assert!(picker.limited);
                assert!(!picker.rows.is_empty());
            });
        });
    }

    #[cfg(feature = "agent-pane")]
    #[test]
    #[ignore = "measures folder discovery against the real home directory"]
    fn real_home_directory_discovery() {
        let root = home_directory().unwrap();
        let start = Instant::now();
        let mut first = None;
        let mut count = 0;
        let limited = scan_directories(
            &root,
            DIRECTORY_SCAN_LIMIT,
            DIRECTORY_SCAN_BUDGET,
            &|| false,
            &mut |batch| {
                first.get_or_insert_with(|| start.elapsed());
                count += batch.len();
                true
            },
        )
        .unwrap();
        eprintln!(
            "first batch: {:?}; total: {:?}; folders: {count}; limited: {limited}",
            first.unwrap(),
            start.elapsed()
        );
        assert!(count > 0);
        assert!(start.elapsed() < DIRECTORY_SCAN_BUDGET + Duration::from_secs(2));
    }
}
