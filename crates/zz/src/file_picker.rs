//! Shared in-app fuzzy path picker.

use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    MouseButton, Render, ScrollStrategy, SharedString, Subscription, Task, UniformListScrollHandle,
    Window, div, prelude::*, px, relative, uniform_list,
};
use ignore::WalkBuilder;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use zz_ui::command::palette_shortcut_hint;
use zz_ui::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, Icon, IconName, Sizable as _, h_flex,
    input::{Input, InputEvent, InputState, MoveDown, MoveUp},
    scroll::ScrollableElement as _,
    v_flex,
};

const MAX_PICKER_ENTRIES: usize = 50_000;
const MAX_PICKER_ROWS: usize = 500;
const WALK_BATCH: usize = 1024;
const WALK_BATCH_QUEUE: usize = 8;
const DIRECTORY_MAX_DEPTH: usize = 6;
const NO_DESCEND_HOME_ROOTS: [&str; 9] = [
    "Applications",
    "Library",
    "Movies",
    "Music",
    "Pictures",
    "Public",
    "Templates",
    "Videos",
    "snap",
];
const WORKSPACE_PRIOR: u32 = 1 << 10;
const PICKER_ROW_HEIGHT: f32 = 26.0;

const TRUNCATED_NOTE: &str = "truncated: showing the first 50,000 entries";
const _: () = assert!(
    MAX_PICKER_ENTRIES == 50_000,
    "TRUNCATED_NOTE spells the cap out"
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FilePickerMode {
    Files,
    Directories,
}

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

#[derive(Clone, Debug)]
pub(crate) enum FilePickerEvent {
    Selected(PathBuf),
    Dismissed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PickerEntry {
    relative: SharedString,
    absolute: PathBuf,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Ranked {
    score: u32,
    prior: u32,
    entry: usize,
}

fn ranked_order(left: &Ranked, right: &Ranked) -> Ordering {
    right
        .score
        .cmp(&left.score)
        .then(right.prior.cmp(&left.prior))
        .then(left.entry.cmp(&right.entry))
}

enum WalkMessage {
    Batch(Vec<PickerEntry>),
    Finished {
        truncated: bool,
        error: Option<String>,
    },
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
    home_directory().unwrap_or_else(|| fallback.to_path_buf())
}

fn walk_root(
    root: &Path,
    mode: FilePickerMode,
    limit: usize,
    batch_size: usize,
    emit: &mut dyn FnMut(WalkMessage) -> bool,
) {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .require_git(false)
        .sort_by_file_name(|left: &std::ffi::OsStr, right: &std::ffi::OsStr| left.cmp(right));
    if mode == FilePickerMode::Directories {
        builder.max_depth(Some(DIRECTORY_MAX_DEPTH));
        builder.filter_entry(|entry| {
            !(entry.depth() == 2
                && entry
                    .path()
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| NO_DESCEND_HOME_ROOTS.iter().any(|root| name == *root)))
        });
    }

    let mut batch = Vec::with_capacity(batch_size);
    let mut produced = 0_usize;
    let mut truncated = false;
    let mut error = None;
    for result in builder.build() {
        let entry = match result {
            Ok(entry) => entry,
            Err(failure) => {
                if error.is_none() {
                    error = Some(failure.to_string());
                }
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        let is_directory = entry.file_type().is_some_and(|kind| kind.is_dir());
        if is_directory != (mode == FilePickerMode::Directories) {
            continue;
        }
        let Some(relative) = entry
            .path()
            .strip_prefix(root)
            .ok()
            .and_then(Path::to_str)
            .filter(|relative| !relative.is_empty())
        else {
            continue;
        };
        let is_workspace = is_directory && entry.path().join(".git").exists();
        batch.push(PickerEntry {
            relative: SharedString::from(relative.to_owned()),
            absolute: entry.path().to_path_buf(),
            prior: entry_prior(entry.depth(), is_workspace),
        });
        produced += 1;
        if batch.len() >= batch_size {
            let full = std::mem::replace(&mut batch, Vec::with_capacity(batch_size));
            if !emit(WalkMessage::Batch(full)) {
                return;
            }
        }
        if produced >= limit {
            truncated = true;
            break;
        }
    }
    if !batch.is_empty() && !emit(WalkMessage::Batch(batch)) {
        return;
    }
    emit(WalkMessage::Finished { truncated, error });
}

fn rank_entries(
    entries: &[PickerEntry],
    offset: usize,
    query: &str,
    matcher: &mut Matcher,
) -> Vec<Ranked> {
    if query.trim().is_empty() {
        let mut ranked = (offset..entries.len())
            .map(|entry| Ranked {
                score: 0,
                prior: entries[entry].prior,
                entry,
            })
            .collect::<Vec<_>>();
        ranked.sort_by(ranked_order);
        return ranked;
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut buffer = Vec::new();
    let mut ranked = entries
        .iter()
        .enumerate()
        .skip(offset)
        .filter_map(|(entry, candidate)| {
            let path = candidate.relative.as_ref();
            let score = pattern.score(Utf32Str::new(path, &mut buffer), matcher)?;
            let name = path
                .rsplit(std::path::MAIN_SEPARATOR)
                .next()
                .unwrap_or(path);
            let name_bonus = pattern
                .score(Utf32Str::new(name, &mut buffer), matcher)
                .unwrap_or(0);
            Some(Ranked {
                score: score + name_bonus,
                prior: candidate.prior,
                entry,
            })
        })
        .collect::<Vec<_>>();
    ranked.sort_by(ranked_order);
    ranked
}

fn merge_ranked(mut ranked: Vec<Ranked>, incoming: Vec<Ranked>, limit: usize) -> Vec<Ranked> {
    if incoming.is_empty() {
        return ranked;
    }
    ranked.extend(incoming);
    ranked.sort_by(ranked_order);
    ranked.truncate(limit);
    ranked
}

fn preserved_selection(ranked: &[Ranked], preferred: Option<usize>) -> Option<usize> {
    preferred
        .and_then(|entry| ranked.iter().position(|row| row.entry == entry))
        .or_else(|| (!ranked.is_empty()).then_some(0))
}

pub(crate) struct FilePickerView {
    mode: FilePickerMode,
    input: Entity<InputState>,
    entries: Vec<PickerEntry>,
    ranked: Vec<Ranked>,
    rows: Arc<[SharedString]>,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    matcher: Matcher,
    query: String,
    scanning: bool,
    truncated: bool,
    error: Option<SharedString>,
    _walk: Task<()>,
    _subscriptions: Vec<Subscription>,
}

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
            ranked: Vec::new(),
            rows: Arc::from([]),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            query: String::new(),
            scanning: true,
            truncated: false,
            error: None,
            _walk: Self::spawn_walk(mode, root, cx),
            _subscriptions: vec![subscription],
        }
    }

    pub(crate) fn focus(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    fn spawn_walk(mode: FilePickerMode, root: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        let (sender, receiver) = async_channel::bounded::<WalkMessage>(WALK_BATCH_QUEUE);
        cx.background_executor()
            .spawn(async move {
                walk_root(
                    &root,
                    mode,
                    MAX_PICKER_ENTRIES,
                    WALK_BATCH,
                    &mut |message| sender.send_blocking(message).is_ok(),
                );
            })
            .detach();
        cx.spawn(async move |picker, cx| {
            while let Ok(message) = receiver.recv().await {
                if picker
                    .update(cx, |picker, cx| picker.apply_walk(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    fn apply_walk(&mut self, message: WalkMessage, cx: &mut Context<Self>) {
        match message {
            WalkMessage::Batch(batch) => self.append(batch),
            WalkMessage::Finished { truncated, error } => {
                self.scanning = false;
                self.truncated = truncated;
                self.error = error.map(SharedString::from);
            }
        }
        cx.notify();
    }

    fn append(&mut self, batch: Vec<PickerEntry>) {
        if batch.is_empty() {
            return;
        }
        let offset = self.entries.len();
        self.entries.extend(batch);
        let incoming = rank_entries(&self.entries, offset, &self.query, &mut self.matcher);
        let preferred = self.selected_entry();
        let ranked = merge_ranked(std::mem::take(&mut self.ranked), incoming, MAX_PICKER_ROWS);
        self.set_ranked(ranked, preferred);
    }

    fn on_query_changed(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let query = input.read(cx).value().to_string();
        if query == self.query {
            return;
        }
        self.query = query;
        let mut ranked = rank_entries(&self.entries, 0, &self.query, &mut self.matcher);
        ranked.truncate(MAX_PICKER_ROWS);
        let preferred = self.selected_entry();
        self.set_ranked(ranked, preferred);
        cx.notify();
    }

    fn set_ranked(&mut self, ranked: Vec<Ranked>, preferred: Option<usize>) {
        self.rows = ranked
            .iter()
            .filter_map(|row| self.entries.get(row.entry))
            .map(|entry| entry.relative.clone())
            .collect();
        let previous = self.selected;
        self.selected = preserved_selection(&ranked, preferred);
        self.ranked = ranked;
        if self.selected != previous
            && let Some(selected) = self.selected
        {
            self.scroll
                .scroll_to_item(selected, ScrollStrategy::Nearest);
        }
    }

    fn selected_entry(&self) -> Option<usize> {
        self.ranked.get(self.selected?).map(|row| row.entry)
    }

    fn navigate(&mut self, direction: isize, cx: &mut Context<Self>) {
        if self.ranked.is_empty() {
            return;
        }
        let count = self.ranked.len();
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
        let Some(entry) = self
            .ranked
            .get(index)
            .and_then(|row| self.entries.get(row.entry))
        else {
            return;
        };
        cx.emit(FilePickerEvent::Selected(entry.absolute.clone()));
    }

    fn accept_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.accept(index, cx);
        }
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
                        h_flex()
                            .id(("file-picker-row", index))
                            .w_full()
                            .h(px(PICKER_ROW_HEIGHT))
                            .items_center()
                            .gap_2()
                            .rounded(cx.theme().radius)
                            .px_2p5()
                            .cursor_pointer()
                            .when(is_selected, |this| {
                                this.bg(cx.theme().background.raised(2).wash())
                            })
                            .when(!is_selected, |this| {
                                this.hover(|this| this.bg(cx.theme().background.hover()))
                            })
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
                            })
                            .child(
                                Icon::new(icon.clone())
                                    .xsmall()
                                    .text_color(cx.theme().foreground.muted()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_size(zz_ui::rems_from_px(12.0))
                                    .child(label),
                            ),
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
            .when(listed && self.truncated, |this| this.child(TRUNCATED_NOTE))
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

impl EventEmitter<FilePickerEvent> for FilePickerView {}

impl Focusable for FilePickerView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.focus(cx)
    }
}

impl Render for FilePickerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus(cx);
        let backdrop_view = cx.entity();
        let empty = self.rows.is_empty();
        let empty_message = self.empty_message(cx);
        div()
            .id("file-picker-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(cx.theme().scrim)
            .occlude()
            .track_focus(&focus)
            .capture_action(cx.listener(Self::move_up))
            .capture_action(cx.listener(Self::move_down))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                backdrop_view.update(cx, FilePickerView::dismiss);
                cx.stop_propagation();
            })
            .child(
                v_flex()
                    .id("file-picker-modal")
                    .relative()
                    .w(relative(0.92))
                    .max_w(px(660.0))
                    .h(relative(0.82))
                    .min_h(px(240.0))
                    .max_h(px(720.0))
                    .overflow_hidden()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background.raised(1))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        v_flex()
                            .flex_none()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .p(px(CHROME_GAP))
                            .child(
                                h_flex()
                                    .w_full()
                                    .h(px(32.0))
                                    .rounded(cx.theme().radius)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().background.raised(1))
                                    .px_2p5()
                                    .child(
                                        Icon::new(IconName::Search)
                                            .xsmall()
                                            .text_color(cx.theme().foreground.muted()),
                                    )
                                    .child(
                                        Input::new(&self.input)
                                            .small()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(zz_ui::rems_from_px(12.0))
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false),
                                    ),
                            ),
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
                            .border_color(cx.theme().border)
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

    fn entry_with_prior(relative: &str, prior: u32) -> PickerEntry {
        PickerEntry {
            relative: SharedString::from(relative.to_owned()),
            absolute: Path::new("/root").join(relative),
            prior,
        }
    }

    fn entry(relative: &str) -> PickerEntry {
        entry_with_prior(relative, 0)
    }

    fn matcher() -> Matcher {
        Matcher::new(Config::DEFAULT.match_paths())
    }

    fn labels(entries: &[PickerEntry], ranked: &[Ranked]) -> Vec<String> {
        ranked
            .iter()
            .map(|row| entries[row.entry].relative.to_string())
            .collect()
    }

    fn collect_walk(
        root: &Path,
        mode: FilePickerMode,
        limit: usize,
    ) -> (Vec<PickerEntry>, bool, Option<String>) {
        let mut entries = Vec::new();
        let mut truncated = false;
        let mut error = None;
        walk_root(root, mode, limit, 2, &mut |message| {
            match message {
                WalkMessage::Batch(batch) => entries.extend(batch),
                WalkMessage::Finished {
                    truncated: capped,
                    error: failure,
                } => {
                    truncated = capped;
                    error = failure;
                }
            }
            true
        });
        (entries, truncated, error)
    }

    #[test]
    fn ranking_puts_the_path_the_query_abbreviates_first() {
        let entries = vec![entry("Cargo.lock"), entry("src/main.rs"), entry("moon.rs")];

        let ranked = rank_entries(&entries, 0, "mn", &mut matcher());

        assert_eq!(
            labels(&entries, &ranked).first().map(String::as_str),
            Some("src/main.rs"),
            "an fzf-style abbreviation ranks the file it abbreviates first"
        );
        assert!(
            !labels(&entries, &ranked).contains(&"Cargo.lock".to_owned()),
            "a path without the query's letters is not a match at all"
        );
    }

    #[test]
    fn an_empty_query_keeps_walk_order_between_equal_priors() {
        let entries = vec![entry("z.rs"), entry("a.rs"), entry("m.rs")];

        let ranked = rank_entries(&entries, 0, "  ", &mut matcher());

        assert_eq!(labels(&entries, &ranked), ["z.rs", "a.rs", "m.rs"]);
    }

    #[test]
    fn an_empty_query_ranks_workspaces_then_shallow_then_deep() {
        let entries = vec![
            entry_with_prior("go/pkg/mod/gopkg.in", entry_prior(4, false)),
            entry_with_prior("Desktop", entry_prior(1, false)),
            entry_with_prior("dev/zz", entry_prior(2, true)),
        ];

        let ranked = rank_entries(&entries, 0, "", &mut matcher());

        assert_eq!(
            labels(&entries, &ranked),
            ["dev/zz", "Desktop", "go/pkg/mod/gopkg.in"],
            "a checkout beats a shallower plain folder, which beats cache depth"
        );
    }

    #[test]
    fn a_name_match_outranks_an_equal_path_match() {
        let entries = vec![entry("readme/notes.md"), entry("docs/readme.md")];

        let ranked = rank_entries(&entries, 0, "readme", &mut matcher());

        assert_eq!(
            labels(&entries, &ranked).first().map(String::as_str),
            Some("docs/readme.md"),
        );
    }

    #[test]
    fn a_score_tie_falls_to_the_prior() {
        let entries = vec![
            entry_with_prior("dev/zz", entry_prior(2, false)),
            entry_with_prior("dev/zz", entry_prior(2, true)),
        ];

        let ranked = rank_entries(&entries, 0, "zz", &mut matcher());

        assert_eq!(
            ranked.first().map(|row| row.entry),
            Some(1),
            "identical text, so only the workspace prior separates them"
        );
    }

    #[test]
    fn streamed_batches_rank_the_same_as_one_pass() {
        let entries = (0..64)
            .map(|index| entry(&format!("src/item{index}/main.rs")))
            .collect::<Vec<_>>();

        let one_pass = {
            let mut ranked = rank_entries(&entries, 0, "main", &mut matcher());
            ranked.truncate(MAX_PICKER_ROWS);
            ranked
        };
        let streamed =
            (0..entries.len())
                .step_by(8)
                .fold(Vec::new(), |ranked: Vec<Ranked>, offset: usize| {
                    let visible = &entries[..(offset + 8).min(entries.len())];
                    let incoming = rank_entries(visible, offset, "main", &mut matcher());
                    merge_ranked(ranked, incoming, MAX_PICKER_ROWS)
                });

        assert_eq!(streamed, one_pass);
    }

    #[test]
    fn the_row_cap_keeps_the_best_rows_only() {
        let entries = (0..MAX_PICKER_ROWS + 100)
            .map(|index| entry(&format!("file{index}.rs")))
            .collect::<Vec<_>>();

        let ranked = merge_ranked(
            Vec::new(),
            rank_entries(&entries, 0, "", &mut matcher()),
            MAX_PICKER_ROWS,
        );

        assert_eq!(ranked.len(), MAX_PICKER_ROWS);
        assert_eq!(ranked[0].entry, 0);
    }

    #[test]
    fn the_cursor_holds_its_entry_across_a_rerank_and_otherwise_resets() {
        let ranked = vec![
            Ranked {
                score: 9,
                prior: 0,
                entry: 4,
            },
            Ranked {
                score: 5,
                prior: 0,
                entry: 1,
            },
        ];

        assert_eq!(preserved_selection(&ranked, Some(1)), Some(1));
        assert_eq!(
            preserved_selection(&ranked, Some(7)),
            Some(0),
            "an entry the new results dropped sends the cursor to the top"
        );
        assert_eq!(preserved_selection(&ranked, None), Some(0));
        assert_eq!(preserved_selection(&[], Some(1)), None);
    }

    #[test]
    fn the_walk_lists_files_relative_to_the_root_and_honors_gitignore() {
        let root = tempfile::tempdir().expect("temporary directory");
        fs::write(root.path().join(".gitignore"), "ignored.rs\n").expect("gitignore fixture");
        fs::write(root.path().join("kept.rs"), "").expect("kept fixture");
        fs::write(root.path().join("ignored.rs"), "").expect("ignored fixture");
        fs::create_dir(root.path().join("src")).expect("nested directory");
        fs::write(root.path().join("src/main.rs"), "").expect("nested fixture");

        let (entries, truncated, error) =
            collect_walk(root.path(), FilePickerMode::Files, MAX_PICKER_ENTRIES);

        let mut relatives = entries
            .iter()
            .map(|entry| entry.relative.to_string())
            .collect::<Vec<_>>();
        relatives.sort();
        assert_eq!(
            relatives,
            [
                "kept.rs".to_owned(),
                format!("src{}main.rs", std::path::MAIN_SEPARATOR)
            ]
        );
        assert!(!truncated);
        assert_eq!(error, None);
        assert!(
            entries
                .iter()
                .all(|entry| entry.absolute.starts_with(root.path())),
            "a selection returns an absolute path"
        );
    }

    #[test]
    fn directories_mode_lists_only_directories_within_the_depth_bound() {
        let root = tempfile::tempdir().expect("temporary directory");
        let mut deep = root.path().to_path_buf();
        for level in 0..DIRECTORY_MAX_DEPTH + 2 {
            deep = deep.join(format!("level{level}"));
        }
        fs::create_dir_all(&deep).expect("deep directory fixture");
        fs::write(root.path().join("level0/file.rs"), "").expect("file fixture");

        let (entries, _, _) =
            collect_walk(root.path(), FilePickerMode::Directories, MAX_PICKER_ENTRIES);

        let relatives = entries
            .iter()
            .map(|entry| entry.relative.to_string())
            .collect::<Vec<_>>();
        assert!(
            relatives
                .iter()
                .all(|relative| !relative.ends_with("file.rs")),
            "directories mode never offers a file: {relatives:?}"
        );
        assert_eq!(
            relatives.len(),
            DIRECTORY_MAX_DEPTH,
            "the walk stops at the depth bound: {relatives:?}"
        );
    }

    #[test]
    fn directories_mode_lists_a_media_root_without_descending_into_it() {
        let root = tempfile::tempdir().expect("temporary directory");
        fs::create_dir_all(root.path().join("Music/Music/Media.localized")).expect("media fixture");
        fs::create_dir_all(root.path().join("dev/project")).expect("project fixture");

        let (entries, _, _) =
            collect_walk(root.path(), FilePickerMode::Directories, MAX_PICKER_ENTRIES);

        let relatives = entries
            .iter()
            .map(|entry| entry.relative.to_string())
            .collect::<Vec<_>>();
        assert!(relatives.contains(&"Music".to_owned()), "{relatives:?}");
        assert!(
            !relatives.iter().any(|relative| relative.contains("Media")),
            "nothing below a media root is listed: {relatives:?}"
        );
        assert!(
            relatives.contains(&format!("dev{}project", std::path::MAIN_SEPARATOR)),
            "ordinary folders still descend: {relatives:?}"
        );
    }

    #[test]
    fn the_walk_marks_a_git_checkout_with_the_workspace_prior() {
        let root = tempfile::tempdir().expect("temporary directory");
        fs::create_dir_all(root.path().join("zebra/.git")).expect("checkout fixture");
        fs::create_dir(root.path().join("apple")).expect("plain fixture");

        let (entries, _, _) =
            collect_walk(root.path(), FilePickerMode::Directories, MAX_PICKER_ENTRIES);

        let prior_of = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.relative.as_ref() == name)
                .unwrap_or_else(|| panic!("{name} is listed"))
                .prior
        };
        assert_eq!(prior_of("zebra"), entry_prior(1, true));
        assert_eq!(prior_of("apple"), entry_prior(1, false));
        let ranked = rank_entries(&entries, 0, "", &mut matcher());
        assert_eq!(
            labels(&entries, &ranked).first().map(String::as_str),
            Some("zebra"),
            "the checkout outranks the alphabetically earlier plain folder"
        );
    }

    #[test]
    fn the_entry_cap_stops_the_walk_and_says_so() {
        let root = tempfile::tempdir().expect("temporary directory");
        for index in 0..8 {
            fs::write(root.path().join(format!("file{index}.rs")), "").expect("fixture");
        }

        let (entries, truncated, _) = collect_walk(root.path(), FilePickerMode::Files, 3);

        assert_eq!(entries.len(), 3);
        assert!(truncated);
    }

    #[test]
    fn a_hung_up_receiver_ends_the_walk() {
        let root = tempfile::tempdir().expect("temporary directory");
        for index in 0..64 {
            fs::write(root.path().join(format!("file{index}.rs")), "").expect("fixture");
        }

        let mut emitted = 0_usize;
        walk_root(
            root.path(),
            FilePickerMode::Files,
            MAX_PICKER_ENTRIES,
            2,
            &mut |_| {
                emitted += 1;
                false
            },
        );

        assert_eq!(emitted, 1, "the first refusal ends the walk");
    }

    #[test]
    fn an_unreadable_root_reports_instead_of_listing() {
        let root = tempfile::tempdir().expect("temporary directory");
        let missing = root.path().join("nowhere");

        let (entries, truncated, error) =
            collect_walk(&missing, FilePickerMode::Files, MAX_PICKER_ENTRIES);

        assert!(entries.is_empty());
        assert!(!truncated);
        assert!(error.is_some(), "the overlay has a message to show");
    }
}
