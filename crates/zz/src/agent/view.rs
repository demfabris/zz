use std::{
    collections::{BTreeSet, HashMap},
    f32::consts::PI,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Datelike as _, Local, NaiveDate, Timelike as _};
use gpui::{
    Anchor, AnyElement, Context, ElementId, Entity, EntityId, FocusHandle, Focusable, Hsla, Image,
    IntoElement, KeyDownEvent, ListAlignment, ListState, MouseButton, MouseDownEvent, PathBuilder,
    Render, Role, ScrollStrategy, SharedString, Subscription, Transformation,
    UniformListScrollHandle, Window, canvas, div, ease_in_out, percentage, point, prelude::*, px,
    relative, uniform_list,
};
use zz_protocol::{AgentDescriptor, AgentGitSummary, AgentProvider, CommandInvocation, PaneId};
#[cfg(all(test, not(target_os = "macos")))]
use zz_ui::agent::DisclosureKind;
use zz_ui::agent::{
    AGENT_CHROME_CONTROL_HEIGHT, AGENT_CONTENT_MAX_WIDTH, AgentEntry, AgentMarkdown, AgentTimeline,
    AgentTimelineStore, AgentToolEntry, AgentToolKind, AgentToolPayload, AgentToolStatus,
    AgentToolText, COMPOSER_ATTACHMENT, FoldedTimelineRows, MarkdownSlot, TimelineRow,
    TimelineStick, agent_attachment_thumbnail, agent_jump_to_bottom_button, agent_pane_header,
    append_timeline_row, fold_timeline_rows, timeline_group_kind,
};
use zz_ui::command::palette_shortcut_hint;
use zz_ui::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, Disableable as _, Icon, IconName, Sizable as _,
    Size,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{IndentInline, Input, InputEvent, InputState, MoveDown, MoveUp},
    menu::{DropdownMenu as _, PopupMenuItem},
    pulse::pulse_phase,
    scroll::Scrollbar,
    tag::Tag,
    tooltip::Tooltip,
    v_flex,
};

use crate::{
    agent::attachment as agent_attachment,
    agent::controller::{
        AgentCommand, AgentConfigCategory, AgentConfigOption, AgentConnectionState,
        AgentController, AgentPaneState, AgentPermissionKind, AgentPermissionRequest,
        AgentSessionCapabilities, AgentSessionHistoryState, AgentSessionSummary, AgentThreadEntry,
        AgentToolKindModel, AgentToolStatusModel, ToolPayload,
    },
    config::pane_content_radii,
    file_picker::{FilePickerEvent, FilePickerMode, FilePickerView, directory_picker_root},
    mux::{client::MuxClient, hosts::HostId},
    window::corners::{WindowCorners, round_div_radii},
};

const AGENT_KEY_CONTEXT: &str = "Agent";
const COMPLETION_ROW_HEIGHT: f32 = 52.0;
const MAX_VISIBLE_COMPLETION_ROWS: u8 = 6;
const MAX_COMPLETION_RESULTS: usize = 64;
const HISTORY_ROW_HEIGHT: f32 = 52.0;
const COMPOSER_MIN_HEIGHT: f32 = 86.0;
const COMPOSER_FOOTER_HEIGHT: f32 = 28.0;
const COMPOSER_OUTER_PADDING: f32 = 12.0;
const COMPOSER_SECTION_GAP: f32 = 8.0;
const COMPOSER_ACTION_SIZE: f32 = 32.0;
const COMPOSER_MAX_WIDTH: f32 = AGENT_CONTENT_MAX_WIDTH + 2.0;
const CHROME_BUTTON_HEIGHT: f32 = AGENT_CHROME_CONTROL_HEIGHT;
const CONTEXT_USAGE_RING_SIZE: f32 = 16.0;
const CONTEXT_USAGE_STROKE_WIDTH: f32 = 2.0;
const SPINNER_PERIOD: Duration = Duration::from_millis(800);
const MAX_RENDERED_ERROR_BYTES: usize = 1024;

const fn composer_total_height() -> f32 {
    COMPOSER_MIN_HEIGHT
        + 2.0 * COMPOSER_OUTER_PADDING
        + COMPOSER_SECTION_GAP
        + COMPOSER_FOOTER_HEIGHT
}

const fn composer_tail_clearance() -> f32 {
    composer_total_height() - COMPOSER_OUTER_PADDING
}

fn agent_spinner(size: Size, color: Hsla, view: EntityId, cx: &mut gpui::App) -> AnyElement {
    let phase = ease_in_out(pulse_phase(SPINNER_PERIOD, view, cx));
    Icon::new(IconName::Loader)
        .with_size(size)
        .text_color(color)
        .transform(Transformation::rotate(percentage(phase)))
        .into_any_element()
}

/// Whether the pane shows busy chrome. This reads the connection phase and
/// nothing else: an adapter that never sends a final tool update leaves rows
/// unsettled forever, so tool statuses cannot be allowed to decide whether the
/// pane looks alive.
const fn pane_is_busy(connection: AgentConnectionState) -> bool {
    matches!(
        connection,
        AgentConnectionState::Starting
            | AgentConnectionState::Restoring
            | AgentConnectionState::Running
            | AgentConnectionState::Cancelling
    )
}

const fn directory_picker_enabled(
    connection: AgentConnectionState,
    pending_permission: bool,
    local_host: bool,
) -> bool {
    local_host && connection.accepts_prompt() && !pending_permission
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandCompletion {
    command: AgentCommand,
    replacement: Range<usize>,
}

enum TimelineStoreUpdate {
    None,
    Clear,
    Synchronize(Vec<AgentEntry>),
}

enum TimelineModelUpdate {
    None,
    Rebuild,
    Incremental {
        store_entries: Vec<AgentEntry>,
        remeasure_rows: Vec<usize>,
        splice_start: usize,
        added_rows: usize,
    },
}

#[derive(Clone, Default)]
struct TimelineModel {
    rows: Arc<Vec<TimelineRow>>,
    entry_ids: Vec<u64>,
    entry_revisions: Vec<u64>,
    entry_to_row: Vec<usize>,
    markdown: HashMap<u64, AgentMarkdown>,
    tool_payloads: HashMap<(u64, usize), AgentToolPayload>,
}

impl TimelineModel {
    fn new(entries: &[AgentThreadEntry], revisions: &[u64]) -> Self {
        debug_assert_eq!(entries.len(), revisions.len());
        let mut markdown = HashMap::new();
        let mut tool_payloads = HashMap::new();
        let ui_entries = ui_entries_with_markdown(entries, &mut markdown, &mut tool_payloads);
        let FoldedTimelineRows { rows, entry_to_row } = fold_timeline_rows(&ui_entries);
        Self {
            rows,
            entry_ids: entries.iter().map(AgentThreadEntry::id).collect(),
            entry_revisions: revisions.to_vec(),
            entry_to_row,
            markdown,
            tool_payloads,
        }
    }

    fn clear(&mut self) {
        self.rows = Arc::new(Vec::new());
        self.entry_ids.clear();
        self.entry_revisions.clear();
        self.entry_to_row.clear();
        self.markdown.clear();
        self.tool_payloads.clear();
    }

    fn rebuild(&mut self, entries: &[AgentThreadEntry], revisions: &[u64]) {
        *self = Self::new(entries, revisions);
    }

    fn synchronize(
        &mut self,
        entries: &[AgentThreadEntry],
        revisions: &[u64],
        changed_entries: Option<&[usize]>,
    ) -> TimelineModelUpdate {
        debug_assert_eq!(entries.len(), revisions.len());
        if entries.len() != revisions.len() {
            return TimelineModelUpdate::None;
        }

        let old_entry_count = self.entry_ids.len();
        let append_only = old_entry_count <= entries.len()
            && if changed_entries.is_some() {
                old_entry_count == 0
                    || self.entry_ids.last().copied()
                        == entries.get(old_entry_count - 1).map(AgentThreadEntry::id)
            } else {
                self.entry_ids
                    .iter()
                    .zip(entries)
                    .all(|(id, entry)| *id == entry.id())
            };
        if !append_only {
            self.rebuild(entries, revisions);
            return TimelineModelUpdate::Rebuild;
        }

        let changed_existing = changed_entries.map_or_else(
            || {
                (0..old_entry_count)
                    .filter(|index| self.entry_revisions[*index] != revisions[*index])
                    .collect::<Vec<_>>()
            },
            |changes| {
                changes
                    .iter()
                    .copied()
                    .filter(|index| {
                        *index < old_entry_count
                            && self.entry_revisions[*index] != revisions[*index]
                    })
                    .collect::<Vec<_>>()
            },
        );
        if changed_existing.is_empty() && old_entry_count == entries.len() {
            return TimelineModelUpdate::None;
        }

        let mut replacements = Vec::with_capacity(changed_existing.len());
        for &entry_index in &changed_existing {
            let id = self.entry_ids[entry_index];
            let Some(&row_index) = self.entry_to_row.get(entry_index) else {
                self.rebuild(entries, revisions);
                return TimelineModelUpdate::Rebuild;
            };
            let next = ui_entry_with_markdown(
                &entries[entry_index],
                &mut self.markdown,
                &mut self.tool_payloads,
            );
            let Some(previous) = self.rows.get(row_index).and_then(|row| row.entry(id)) else {
                self.rebuild(entries, revisions);
                return TimelineModelUpdate::Rebuild;
            };
            if !replacement_preserves_folding(previous, &next) {
                self.rebuild(entries, revisions);
                return TimelineModelUpdate::Rebuild;
            }
            replacements.push((entry_index, row_index, id, next));
        }

        let appended = entries
            .iter()
            .skip(old_entry_count)
            .map(|entry| ui_entry_with_markdown(entry, &mut self.markdown, &mut self.tool_payloads))
            .collect::<Vec<_>>();
        let old_row_count = self.rows.len();
        let mut store_entries = Vec::with_capacity(replacements.len());
        let mut remeasure_rows = replacements
            .iter()
            .map(|(_, row_index, _, _)| *row_index)
            .collect::<Vec<_>>();
        let rows = Arc::make_mut(&mut self.rows);
        for (entry_index, row_index, id, entry) in replacements {
            store_entries.push(entry.clone());
            let replaced = rows[row_index].replace_entry(id, entry);
            debug_assert!(replaced);
            self.entry_revisions[entry_index] = revisions[entry_index];
        }

        for entry in appended {
            let (row_index, row_added) = append_timeline_row(rows, entry);
            self.entry_to_row.push(row_index);
            if !row_added && row_index < old_row_count {
                remeasure_rows.push(row_index);
            }
        }
        self.entry_ids
            .extend(entries[old_entry_count..].iter().map(AgentThreadEntry::id));
        self.entry_revisions
            .extend_from_slice(&revisions[old_entry_count..]);

        remeasure_rows.sort_unstable();
        remeasure_rows.dedup();
        TimelineModelUpdate::Incremental {
            store_entries,
            remeasure_rows,
            splice_start: old_row_count,
            added_rows: rows.len() - old_row_count,
        }
    }
}

fn replacement_preserves_folding(previous: &AgentEntry, next: &AgentEntry) -> bool {
    timeline_group_kind(previous) == timeline_group_kind(next)
}

/// What the composer's single action button does right now. A turn already
/// running turns Send into Queue — zz dispatches a queued prompt as the next
/// turn, it never injects into the live one — and an empty composer under a
/// live turn turns it into Stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComposerAction {
    Send,
    Queue,
    Stop,
}

const fn composer_action(active_turn: bool, has_content: bool) -> ComposerAction {
    match (active_turn, has_content) {
        (false, _) => ComposerAction::Send,
        (true, true) => ComposerAction::Queue,
        (true, false) => ComposerAction::Stop,
    }
}

#[allow(clippy::cast_precision_loss)]
fn context_usage_fraction(used: u64, size: u64) -> f64 {
    if size == 0 {
        0.0
    } else {
        (used.min(size) as f64 / size as f64).clamp(0.0, 1.0)
    }
}

fn context_usage_tooltip(used: u64, size: u64) -> String {
    if size == 0 {
        "Context window usage unavailable".to_owned()
    } else {
        format!(
            "{used} of {size} context tokens used ({:.0}%)",
            context_usage_fraction(used, size) * 100.0
        )
    }
}

#[allow(clippy::cast_possible_truncation)]
fn context_usage_meter(pane: PaneId, used: u64, size: u64, cx: &gpui::App) -> AnyElement {
    let progress = context_usage_fraction(used, size) as f32;
    let tooltip = context_usage_tooltip(used, size);
    let track_color = cx.theme().foreground.muted().opacity(0.22);
    let progress_color = cx.theme().foreground.muted();
    let ring = canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let stroke = px(CONTEXT_USAGE_STROKE_WIDTH);
            let radius = px((CONTEXT_USAGE_RING_SIZE - CONTEXT_USAGE_STROKE_WIDTH) / 2.0);
            let center_x = bounds.origin.x + bounds.size.width / 2.0;
            let center_y = bounds.origin.y + bounds.size.height / 2.0;

            let mut track = PathBuilder::stroke(stroke);
            track.move_to(point(center_x + radius, center_y));
            track.arc_to(
                point(radius, radius),
                px(0.0),
                false,
                true,
                point(center_x - radius, center_y),
            );
            track.arc_to(
                point(radius, radius),
                px(0.0),
                false,
                true,
                point(center_x + radius, center_y),
            );
            track.close();
            if let Ok(path) = track.build() {
                window.paint_path(path, track_color);
            }

            if progress <= 0.0 {
                return;
            }
            let mut fill = PathBuilder::stroke(stroke);
            if progress >= 0.999 {
                fill.move_to(point(center_x + radius, center_y));
                fill.arc_to(
                    point(radius, radius),
                    px(0.0),
                    false,
                    true,
                    point(center_x - radius, center_y),
                );
                fill.arc_to(
                    point(radius, radius),
                    px(0.0),
                    false,
                    true,
                    point(center_x + radius, center_y),
                );
                fill.close();
            } else {
                fill.move_to(point(center_x, center_y - radius));
                let angle = -PI / 2.0 + progress * 2.0 * PI;
                fill.arc_to(
                    point(radius, radius),
                    px(0.0),
                    progress > 0.5,
                    true,
                    point(
                        center_x + radius * angle.cos(),
                        center_y + radius * angle.sin(),
                    ),
                );
            }
            if let Ok(path) = fill.build() {
                window.paint_path(path, progress_color);
            }
        },
    )
    .size(px(CONTEXT_USAGE_RING_SIZE));
    let aria_value = format!("{:.0}%", f64::from(progress) * 100.0);
    let hover_tooltip = tooltip.clone();

    div()
        .id(("agent-context-usage", pane.0))
        .role(Role::ProgressIndicator)
        .aria_label(tooltip)
        .aria_value(aria_value)
        .flex()
        .flex_none()
        .size(px(28.0))
        .items_center()
        .justify_center()
        .tooltip(move |window, cx| Tooltip::new(hover_tooltip.clone()).build(window, cx))
        .child(ring)
        .into_any_element()
}

fn git_file_count_label(count: u32) -> String {
    if count == 1 {
        "1 file".to_owned()
    } else {
        format!("{count} files")
    }
}

fn git_summary_footer(pane: PaneId, git: &AgentGitSummary, cx: &gpui::App) -> AnyElement {
    let branch = git.branch.clone().map(SharedString::from);
    let files = git_file_count_label(git.changed_files);
    let tooltip = match git.branch.as_deref() {
        Some(branch) => format!(
            "{branch}: {files}, +{} additions, -{} deletions",
            git.additions, git.deletions
        ),
        None => format!(
            "Detached HEAD: {files}, +{} additions, -{} deletions",
            git.additions, git.deletions
        ),
    };
    let hover_tooltip = tooltip.clone();

    h_flex()
        .id(("agent-git-summary", pane.0))
        .min_w_0()
        .h(px(COMPOSER_FOOTER_HEIGHT))
        .items_center()
        .gap_2()
        .text_size(zz_ui::rems_from_px(11.0))
        .tooltip(move |window, cx| Tooltip::new(hover_tooltip.clone()).build(window, cx))
        .child(
            Icon::new(IconName::GitBranch)
                .xsmall()
                .flex_none()
                .text_color(cx.theme().foreground.muted()),
        )
        .when_some(branch, |this, branch| {
            this.child(
                div()
                    .min_w_0()
                    .max_w(px(220.0))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(cx.theme().foreground.muted())
                    .child(branch),
            )
        })
        .child(
            div()
                .flex_none()
                .text_color(cx.theme().foreground.muted())
                .child(files),
        )
        .child(
            div()
                .flex_none()
                .text_color(cx.theme().success)
                .child(format!("+{}", git.additions)),
        )
        .child(
            div()
                .flex_none()
                .text_color(cx.theme().danger)
                .child(format!("-{}", git.deletions)),
        )
        .into_any_element()
}

/// What a wizard interaction asks the controller to do.
#[derive(Clone, Debug, Eq, PartialEq)]
enum PermissionStep {
    Stay,
    /// `option_id` of `None` is the request's cancel path.
    Answer {
        request_id: u64,
        option_id: Option<String>,
    },
}

/// The pane's pending permission requests, one per page, in arrival order.
///
/// ACP v1 permission options are a closed, typed set, so a page is answered by
/// picking one of them: the free-text override comet's question wizard carries
/// would only apply to kindless, question-shaped requests, which v1 cannot
/// deliver (see `acp_v1_rejects_permission_options_with_an_unknown_kind`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PermissionWizard {
    page: usize,
    focused: Option<u64>,
    highlighted: usize,
    /// Requests already answered, held until the controller drops them from the
    /// pending list: a re-render must not re-show an answered page, and a pick
    /// must never be taken twice.
    answered: BTreeSet<u64>,
}

impl PermissionWizard {
    /// Re-seat the cursor over the pending requests. A request resolved
    /// elsewhere — auto-approved, or cancelled with the turn — loses its page
    /// while the cursor keeps its place among the rest.
    fn sync(&mut self, requests: &[AgentPermissionRequest]) {
        self.answered
            .retain(|id| requests.iter().any(|request| request.request_id == *id));
        let pages = self.pages(requests);
        let Some(last) = pages.len().checked_sub(1) else {
            self.page = 0;
            self.focused = None;
            self.highlighted = 0;
            return;
        };
        self.page = self
            .focused
            .and_then(|id| pages.iter().position(|request| request.request_id == id))
            .unwrap_or_else(|| self.page.min(last));
        let focused = pages[self.page].request_id;
        if self.focused != Some(focused) {
            self.focused = Some(focused);
            self.highlighted = 0;
        }
    }

    fn pages<'a>(&self, requests: &'a [AgentPermissionRequest]) -> Vec<&'a AgentPermissionRequest> {
        requests
            .iter()
            .filter(|request| !self.answered.contains(&request.request_id))
            .collect()
    }

    fn current<'a>(
        &self,
        requests: &'a [AgentPermissionRequest],
    ) -> Option<&'a AgentPermissionRequest> {
        self.pages(requests).get(self.page).copied()
    }

    /// `n/m`, only worth showing once a second request is waiting.
    fn page_label(&self, requests: &[AgentPermissionRequest]) -> Option<String> {
        let count = self.pages(requests).len();
        (count > 1).then(|| format!("{}/{count}", self.page + 1))
    }

    fn highlight(&mut self, option: usize) -> bool {
        let changed = self.highlighted != option;
        self.highlighted = option;
        changed
    }

    /// Answer the focused page and advance to the next pending request.
    fn answer(
        &mut self,
        requests: &[AgentPermissionRequest],
        option: Option<usize>,
    ) -> PermissionStep {
        let Some(request) = self.current(requests) else {
            return PermissionStep::Stay;
        };
        let request_id = request.request_id;
        let option_id = match option {
            Some(index) => match request.options.get(index) {
                Some(option) => Some(option.id.clone()),
                None => return PermissionStep::Stay,
            },
            None => None,
        };
        self.answered.insert(request_id);
        self.sync(requests);
        PermissionStep::Answer {
            request_id,
            option_id,
        }
    }

    /// Number keys 1-9. The composer wins the digit whenever the user is
    /// actually typing into it.
    fn press_digit(
        &mut self,
        requests: &[AgentPermissionRequest],
        digit: usize,
        composer_engaged: bool,
    ) -> PermissionStep {
        let Some(option) = digit.checked_sub(1) else {
            return PermissionStep::Stay;
        };
        if composer_engaged {
            return PermissionStep::Stay;
        }
        self.answer(requests, Some(option))
    }

    fn confirm(&mut self, requests: &[AgentPermissionRequest]) -> PermissionStep {
        self.answer(requests, Some(self.highlighted))
    }

    fn cancel(&mut self, requests: &[AgentPermissionRequest]) -> PermissionStep {
        self.answer(requests, None)
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent visibility, completion, and history UI states"
)]
pub(crate) struct AgentView {
    pane: PaneId,
    mux: Entity<MuxClient>,
    controller: Entity<AgentController>,
    input: Entity<InputState>,
    history_input: Entity<InputState>,
    visible: bool,
    pane_state: AgentPaneState,
    timeline: TimelineModel,
    timeline_store: Entity<AgentTimelineStore>,
    timeline_next_revision: u64,
    conversation_epoch: u64,
    timeline_scroll: ListState,
    stick: TimelineStick,
    completion_scroll: UniformListScrollHandle,
    submission_error: Option<Arc<str>>,
    permission_wizard: PermissionWizard,
    attachments: Vec<Arc<Image>>,
    completions: Arc<[CommandCompletion]>,
    completion_selected: Option<usize>,
    completion_dismissed: bool,
    last_input: String,
    last_cursor: usize,
    history_open: bool,
    history_all_projects: bool,
    history_results: Arc<[usize]>,
    history_selected: Option<usize>,
    history_scroll: UniformListScrollHandle,
    history_delete_confirmation: Option<Arc<str>>,
    last_history_query: String,
    directory_picker: Option<Entity<FilePickerView>>,
    window_corners: WindowCorners,
    _subscriptions: Vec<Subscription>,
}

impl AgentView {
    pub(crate) fn new(
        pane: PaneId,
        descriptor: &AgentDescriptor,
        controller: Entity<AgentController>,
        mux: Entity<MuxClient>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 8)
                .submit_on_enter(true)
                .placeholder("Ask the agent…")
                .context_menu(true)
        });
        let history_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search by title or project…"));
        let timeline_store = cx.new(|_| AgentTimelineStore::default());
        let timeline_store_observer = cx.observe(&timeline_store, |_, _, cx| cx.notify());
        let input_observer = cx.observe(&input, |view, input, cx| {
            view.synchronize_input(&input, cx);
        });
        let input_subscription = cx.subscribe_in(
            &input,
            window,
            |view, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } => view.enter(window, cx),
                InputEvent::Change if view.submission_error.take().is_some() => cx.notify(),
                InputEvent::PasteImages(images) => view.attach_images(images, cx),
                _ => {}
            },
        );
        let history_input_subscription = cx.subscribe_in(
            &history_input,
            window,
            |view, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => view.synchronize_history_input(input, cx),
                InputEvent::PressEnter { .. } => {
                    view.open_selected_history(window, cx);
                }
                _ => {}
            },
        );
        controller.update(cx, |controller, cx| {
            controller.ensure_pane(pane, descriptor, cx);
        });
        let (pane_state, timeline, timeline_next_revision, conversation_epoch) = {
            let controller = controller.read(cx);
            let pane_state = controller
                .pane_state(pane)
                .unwrap_or_else(disconnected_pane_state);
            let (entries, revisions, next_revision) =
                controller.pane_entries(pane).unwrap_or((&[], &[], 0));
            (
                pane_state,
                TimelineModel::new(entries, revisions),
                next_revision,
                controller.conversation_epoch(pane),
            )
        };
        let timeline_scroll = ListState::new(0, ListAlignment::Top, px(1_200.0));
        timeline_scroll.splice(0..0, timeline.rows.len());
        let stick = TimelineStick::new(&timeline_scroll, cx.reduce_motion());
        let scroll_view = cx.weak_entity();
        timeline_scroll.set_scroll_handler(move |_, _, cx| {
            // The list still holds its own borrow across this call, so reading
            // the scroll state back here panics; run after the effect cycle.
            let view = scroll_view.clone();
            cx.defer(move |cx| {
                view.update(cx, |view: &mut Self, cx| view.on_timeline_scroll(cx))
                    .ok();
            });
        });
        let controller_observer = cx.observe(&controller, |view, controller, cx| {
            if view.visible && view.synchronize_controller(&controller, cx) {
                cx.notify();
            }
        });
        Self {
            pane,
            mux,
            controller,
            input,
            history_input,
            visible: false,
            pane_state,
            timeline,
            timeline_store,
            timeline_next_revision,
            conversation_epoch,
            timeline_scroll,
            stick,
            completion_scroll: UniformListScrollHandle::new(),
            submission_error: None,
            permission_wizard: PermissionWizard::default(),
            attachments: Vec::new(),
            completions: Arc::from([]),
            completion_selected: None,
            completion_dismissed: false,
            last_input: String::new(),
            last_cursor: 0,
            history_open: false,
            history_all_projects: false,
            history_results: Arc::from([]),
            history_selected: None,
            history_scroll: UniformListScrollHandle::new(),
            history_delete_confirmation: None,
            last_history_query: String::new(),
            directory_picker: None,
            window_corners: WindowCorners::NONE,
            _subscriptions: vec![
                input_observer,
                timeline_store_observer,
                input_subscription,
                history_input_subscription,
                controller_observer,
            ],
        }
    }

    pub(crate) fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    pub(crate) fn set_window_corners(&mut self, corners: WindowCorners, cx: &mut Context<Self>) {
        if self.window_corners != corners {
            self.window_corners = corners;
            cx.notify();
        }
    }

    pub(crate) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible {
            let controller = self.controller.clone();
            self.synchronize_controller(&controller, cx);
        }
    }

    fn on_mouse_down(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.mux.read(cx).execute(CommandInvocation::new(
            "select-pane",
            ["-t", &self.pane.to_string()],
        ));
    }

    fn synchronize_input(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let input = input.read(cx);
        let value = input.value().to_string();
        let cursor = input.cursor();
        if value == self.last_input && cursor == self.last_cursor {
            return;
        }
        self.last_input.clone_from(&value);
        self.last_cursor = cursor;
        self.completion_dismissed = false;
        let commands = self.pane_state.available_commands.clone();
        self.recompute_completions(&commands);
        cx.notify();
    }

    fn synchronize_history_input(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let query = input.read(cx).value().to_string();
        if query == self.last_history_query {
            return;
        }
        self.last_history_query.clone_from(&query);
        self.recompute_history_results(&query);
        self.history_delete_confirmation = None;
        self.submission_error = None;
        cx.notify();
    }

    fn synchronize_controller(
        &mut self,
        controller: &Entity<AgentController>,
        cx: &mut Context<Self>,
    ) -> bool {
        let reduce_motion = cx.reduce_motion();
        let (changed, store_update) = {
            let controller = controller.read(cx);
            self.synchronize_controller_state(controller, reduce_motion)
        };
        // The tail entry of a live turn is the one still taking deltas, so it is
        // the only one whose display copy may be mended.
        let streaming = self
            .pane_state
            .connection
            .has_active_turn()
            .then(|| self.timeline.entry_ids.last().copied())
            .flatten();
        self.timeline_store
            .update(cx, |store, cx| store.set_streaming(streaming, cx));
        let store_changed = self.update_timeline_store(store_update, cx);
        let cwd = self.pane_state.cwd.clone();
        self.timeline_store
            .update(cx, |store, cx| store.set_cwd(Some(cwd), cx));
        changed || store_changed
    }

    fn update_timeline_store(
        &mut self,
        store_update: TimelineStoreUpdate,
        cx: &mut Context<Self>,
    ) -> bool {
        match store_update {
            TimelineStoreUpdate::None => false,
            TimelineStoreUpdate::Clear => self.timeline_store.update(cx, AgentTimelineStore::clear),
            TimelineStoreUpdate::Synchronize(entries) => {
                for entry in &entries {
                    synchronize_entry_store(&self.timeline_store, entry, cx);
                }
                false
            }
        }
    }

    fn synchronize_controller_state(
        &mut self,
        controller: &AgentController,
        reduce_motion: bool,
    ) -> (bool, TimelineStoreUpdate) {
        let next_state = controller
            .pane_state(self.pane)
            .unwrap_or_else(disconnected_pane_state);
        let state_changed = self.pane_state != next_state;
        let provider_changed = self.pane_state.provider != next_state.provider;
        let next_conversation_epoch = controller.conversation_epoch(self.pane);
        let conversation_changed = self.conversation_epoch != next_conversation_epoch
            || provider_changed
            || self.pane_state.session_id != next_state.session_id;
        self.conversation_epoch = next_conversation_epoch;
        let history_changed = provider_changed
            || self.pane_state.session_history != next_state.session_history
            || self.pane_state.session_id != next_state.session_id;
        let selected_history_session_id = (history_changed && !provider_changed)
            .then(|| self.selected_history_session_id())
            .flatten();
        let commands_changed = provider_changed
            || self.pane_state.available_commands.as_ref()
                != next_state.available_commands.as_ref();
        if state_changed {
            self.pane_state = next_state;
        }
        if history_changed {
            let query = self.last_history_query.clone();
            self.recompute_history_results_preserving(
                &query,
                selected_history_session_id.as_deref(),
            );
        }
        if commands_changed {
            let commands = self.pane_state.available_commands.clone();
            self.recompute_completions(&commands);
        }

        let Some((entries, revisions, next_revision)) = controller.pane_entries(self.pane) else {
            if !self.timeline.rows.is_empty() {
                self.timeline.clear();
                self.timeline_next_revision = 0;
                self.timeline_scroll.reset(0);
                self.stick.engage_now(&self.timeline_scroll, reduce_motion);
                return (true, TimelineStoreUpdate::Clear);
            }
            return (state_changed, TimelineStoreUpdate::Clear);
        };
        if conversation_changed {
            self.timeline.rebuild(entries, revisions);
            self.timeline_next_revision = next_revision;
            self.timeline_scroll.reset(self.timeline.rows.len());
            self.stick.engage_now(&self.timeline_scroll, reduce_motion);
            return (true, TimelineStoreUpdate::Clear);
        }
        if self.timeline.entry_ids.len() == entries.len()
            && self.timeline_next_revision == next_revision
        {
            return (state_changed, TimelineStoreUpdate::None);
        }
        let Some(changed_entries) =
            controller.pane_entry_changes(self.pane, self.timeline_next_revision)
        else {
            self.timeline.rebuild(entries, revisions);
            self.timeline_next_revision = next_revision;
            self.timeline_scroll.reset(self.timeline.rows.len());
            self.stick.engage_now(&self.timeline_scroll, reduce_motion);
            return (true, TimelineStoreUpdate::Clear);
        };
        self.timeline_next_revision = next_revision;
        let (timeline_changed, store_update) =
            self.synchronize_timeline(entries, revisions, &changed_entries, reduce_motion);
        (timeline_changed || state_changed, store_update)
    }

    fn recompute_history_results(&mut self, query: &str) {
        self.recompute_history_results_preserving(query, None);
    }

    fn recompute_history_results_preserving(
        &mut self,
        query: &str,
        preferred_session_id: Option<&str>,
    ) {
        let results = ranked_session_indices(&self.pane_state.session_history.sessions, query);
        let retained_selection = preferred_session_id.and_then(|session_id| {
            history_result_index_for_session(
                &self.pane_state.session_history.sessions,
                &results,
                session_id,
            )
        });
        let selected = retained_selection.or_else(|| (!results.is_empty()).then_some(0));
        self.history_results = results.into();
        self.history_selected = selected;
        if retained_selection.is_none()
            && let Some(selected) = selected
        {
            self.history_scroll
                .scroll_to_item(selected, ScrollStrategy::Top);
        }
    }

    fn selected_history_session_id(&self) -> Option<String> {
        let result_index = self.history_selected?;
        let session_index = *self.history_results.get(result_index)?;
        self.pane_state
            .session_history
            .sessions
            .get(session_index)
            .map(|session| session.session_id.clone())
    }

    fn synchronize_timeline(
        &mut self,
        entries: &[AgentThreadEntry],
        revisions: &[u64],
        changed_entries: &[usize],
        reduce_motion: bool,
    ) -> (bool, TimelineStoreUpdate) {
        match self
            .timeline
            .synchronize(entries, revisions, Some(changed_entries))
        {
            TimelineModelUpdate::None => (false, TimelineStoreUpdate::None),
            TimelineModelUpdate::Rebuild => {
                self.timeline_scroll.reset(self.timeline.rows.len());
                self.stick.engage_now(&self.timeline_scroll, reduce_motion);
                (true, TimelineStoreUpdate::Clear)
            }
            TimelineModelUpdate::Incremental {
                store_entries,
                remeasure_rows,
                splice_start,
                added_rows,
            } => {
                for row_index in remeasure_rows {
                    self.timeline_scroll
                        .remeasure_items(row_index..row_index + 1);
                }
                if added_rows > 0 {
                    self.timeline_scroll
                        .splice(splice_start..splice_start, added_rows);
                }
                // The grown rows are still unmeasured, so the distance to the
                // end reads as zero until the next layout: wake the driver
                // outright rather than waiting for a measurement to show up.
                if self.stick.is_pinned() {
                    self.stick.wake();
                }
                let store_update = if store_entries.is_empty() {
                    TimelineStoreUpdate::None
                } else {
                    TimelineStoreUpdate::Synchronize(store_entries)
                };
                (true, store_update)
            }
        }
    }

    fn recompute_completions(&mut self, commands: &[AgentCommand]) {
        let Some(query) = completion_query(&self.last_input, self.last_cursor) else {
            self.completions = Arc::from([]);
            self.completion_selected = None;
            return;
        };
        if self.completion_dismissed {
            self.completions = Arc::from([]);
            self.completion_selected = None;
            return;
        }
        self.completions = ranked_completions(commands, &query).into();
        self.completion_selected = (!self.completions.is_empty()).then_some(
            self.completion_selected
                .unwrap_or_default()
                .min(self.completions.len().saturating_sub(1)),
        );
        if let Some(selected) = self.completion_selected {
            self.completion_scroll
                .scroll_to_item(selected, ScrollStrategy::Nearest);
        }
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_open {
            self.open_selected_history(window, cx);
            return;
        }
        if !self.completions.is_empty() {
            self.accept_selected_completion(window, cx);
            return;
        }
        if self.confirm_permission(cx) {
            return;
        }
        self.submit(window, cx);
    }

    /// Enter over an empty composer confirms the highlighted permission option.
    /// It never reaches [`ComposerAction::Stop`]: a stray Enter right after
    /// sending must not kill the turn it just started.
    fn confirm_permission(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.input.read(cx).value().trim().is_empty() {
            return false;
        }
        let requests = self.pane_state.pending_permissions.clone();
        let step = self.permission_wizard.confirm(&requests);
        self.apply_permission_step(step, cx)
    }

    fn apply_permission_step(&mut self, step: PermissionStep, cx: &mut Context<Self>) -> bool {
        let PermissionStep::Answer {
            request_id,
            option_id,
        } = step
        else {
            return false;
        };
        let pane = self.pane;
        let sent = self.controller.update(cx, |controller, cx| {
            controller.respond_permission(pane, request_id, option_id, cx)
        });
        if !sent {
            self.permission_wizard.answered.remove(&request_id);
            self.permission_wizard
                .sync(&self.pane_state.pending_permissions);
        }
        cx.notify();
        true
    }

    /// The user is mid-sentence in the composer, so keys the wizard would claim
    /// belong to the draft instead.
    fn composer_engaged(&self, window: &Window, cx: &gpui::App) -> bool {
        let input = self.input.read(cx);
        !input.value().trim().is_empty() && input.focus_handle(cx).is_focused(window)
    }

    fn composer_has_content(&self) -> bool {
        !self.last_input.trim().is_empty() || !self.attachments.is_empty()
    }

    /// Digits 1-9 pick an option on the focused permission page, Escape takes
    /// its cancel path. Answered here rather than through a binding so the
    /// digit can be swallowed before the composer types it.
    fn handle_permission_key(
        &mut self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pane_state.pending_permissions.is_empty() {
            return false;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.alt || modifiers.control || modifiers.function {
            return false;
        }
        let engaged = self.composer_engaged(window, cx);
        let requests = self.pane_state.pending_permissions.clone();
        let step = match event.keystroke.key.as_str() {
            "escape" if !engaged => self.permission_wizard.cancel(&requests),
            key => match key.parse::<usize>() {
                Ok(digit) if (1..=9).contains(&digit) => self
                    .permission_wizard
                    .press_digit(&requests, digit, engaged),
                _ => PermissionStep::Stay,
            },
        };
        self.apply_permission_step(step, cx)
    }

    fn navigate_completion(&mut self, direction: isize, cx: &mut Context<Self>) {
        if self.completions.is_empty() {
            return;
        }
        let count = self.completions.len();
        let current = self.completion_selected.unwrap_or_default();
        let selected = if direction < 0 {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        };
        self.completion_selected = Some(selected);
        self.completion_scroll
            .scroll_to_item(selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pane_state.session_capabilities.list {
            self.submission_error = Some(Arc::from("this agent does not support session history"));
            cx.notify();
            return;
        }
        if self.pane_state.connection.has_active_turn() {
            self.submission_error = Some(Arc::from(
                "finish or cancel the current turn before opening session history",
            ));
            cx.notify();
            return;
        }
        self.history_open = true;
        self.history_all_projects = false;
        self.history_delete_confirmation = None;
        self.last_history_query.clear();
        self.completions = Arc::from([]);
        self.completion_selected = None;
        self.history_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.recompute_history_results("");
        let result = self.controller.update(cx, |controller, cx| {
            controller.list_sessions(self.pane, false, false, cx)
        });
        self.submission_error = result.err();
        self.history_input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    fn close_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_open = false;
        self.history_delete_confirmation = None;
        self.focus(cx).focus(window, cx);
        cx.notify();
    }

    fn refresh_history(&mut self, cx: &mut Context<Self>) {
        let result = self.controller.update(cx, |controller, cx| {
            controller.list_sessions(self.pane, self.history_all_projects, false, cx)
        });
        self.submission_error = result.err();
        cx.notify();
    }

    fn toggle_history_scope(&mut self, cx: &mut Context<Self>) {
        self.history_all_projects = !self.history_all_projects;
        self.history_delete_confirmation = None;
        self.refresh_history(cx);
    }

    fn load_more_history(&mut self, cx: &mut Context<Self>) {
        let result = self.controller.update(cx, |controller, cx| {
            controller.list_sessions(self.pane, self.history_all_projects, true, cx)
        });
        self.submission_error = result.err();
        cx.notify();
    }

    fn navigate_history(&mut self, direction: isize, cx: &mut Context<Self>) {
        if self.history_results.is_empty() {
            return;
        }
        let count = self.history_results.len();
        let current = self.history_selected.unwrap_or_default();
        let selected = if direction < 0 {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        };
        self.history_selected = Some(selected);
        self.history_scroll
            .scroll_to_item(selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn open_history_result(
        &mut self,
        result_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session_index) = self.history_results.get(result_index).copied() else {
            return;
        };
        let Some(session) = self
            .pane_state
            .session_history
            .sessions
            .get(session_index)
            .cloned()
        else {
            return;
        };
        let result = self.controller.update(cx, |controller, cx| {
            controller.switch_session(self.pane, session, cx)
        });
        match result {
            Ok(()) => {
                self.history_open = false;
                self.history_delete_confirmation = None;
                self.submission_error = None;
                self.focus(cx).focus(window, cx);
            }
            Err(error) => self.submission_error = Some(error),
        }
        cx.notify();
    }

    fn open_selected_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.history_open || self.history_delete_confirmation.is_some() {
            return;
        }
        if let Some(index) = self.history_selected {
            self.open_history_result(index, window, cx);
        }
    }

    fn start_new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .controller
            .update(cx, |controller, cx| controller.new_session(self.pane, cx));
        match result {
            Ok(()) => {
                self.history_open = false;
                self.history_delete_confirmation = None;
                self.submission_error = None;
                self.focus(cx).focus(window, cx);
            }
            Err(error) => self.submission_error = Some(error),
        }
        cx.notify();
    }

    fn confirm_delete_history(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let result = self.controller.update(cx, |controller, cx| {
            controller.delete_session(self.pane, session_id, cx)
        });
        self.submission_error = result.err();
        self.history_delete_confirmation = None;
        cx.notify();
    }

    fn accept_completion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(completion) = self.completions.get(index).cloned() else {
            return;
        };
        let name = bare_command_name(&completion.command.name);
        self.completion_dismissed = true;
        self.input.update(cx, |input, cx| {
            input.set_selected_range(completion.replacement, cx);
            input.replace(format!("/{name} "), window, cx);
        });
        self.completions = Arc::from([]);
        self.completion_selected = None;
        self.focus(cx).focus(window, cx);
        cx.notify();
    }

    fn accept_selected_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .completion_selected
            .or((!self.completions.is_empty()).then_some(0))
        {
            self.accept_completion(index, window, cx);
        }
    }

    fn complete(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_open {
            cx.stop_propagation();
            return;
        }
        if self.completions.is_empty() {
            return;
        }
        self.accept_selected_completion(window, cx);
        cx.stop_propagation();
    }

    fn move_completion_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.history_open {
            self.navigate_history(-1, cx);
            cx.stop_propagation();
        } else if !self.completions.is_empty() {
            self.navigate_completion(-1, cx);
            cx.stop_propagation();
        }
    }

    fn move_completion_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.history_open {
            self.navigate_history(1, cx);
            cx.stop_propagation();
        } else if !self.completions.is_empty() {
            self.navigate_completion(1, cx);
            cx.stop_propagation();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_open && event.keystroke.key.as_str() == "escape" {
            if self.history_delete_confirmation.take().is_some() {
                cx.notify();
            } else {
                self.close_history(window, cx);
            }
            cx.stop_propagation();
            return;
        }
        if self.completions.is_empty()
            && !self.history_open
            && self.handle_permission_key(event, window, cx)
        {
            cx.stop_propagation();
            return;
        }
        if self.completions.is_empty() {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            "up" if !modifiers.platform && !modifiers.alt => {
                self.navigate_completion(-1, cx);
            }
            "down" if !modifiers.platform && !modifiers.alt => {
                self.navigate_completion(1, cx);
            }
            "escape" => {
                self.completion_dismissed = true;
                self.completions = Arc::from([]);
                self.completion_selected = None;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn attach_images(&mut self, images: &[Arc<Image>], cx: &mut Context<Self>) {
        if !self.pane_state.session_capabilities.images {
            self.submission_error = Some(Arc::from("this agent does not accept images"));
            cx.notify();
            return;
        }
        for image in images {
            match agent_attachment::normalize(image) {
                Ok(normalized) => {
                    self.attachments.push(normalized);
                    self.submission_error = None;
                }
                Err(error) => self.submission_error = Some(Arc::from(error.as_ref())),
            }
        }
        cx.notify();
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            cx.notify();
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().to_string();
        if value.trim().is_empty() && self.attachments.is_empty() {
            return;
        }
        let attachments = self.attachments.clone();
        let result = self.controller.update(cx, |controller, cx| {
            controller.prompt(self.pane, &value, attachments, cx)
        });
        match result {
            Ok(()) => {
                self.input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
                self.attachments.clear();
                self.submission_error = None;
                self.completions = Arc::from([]);
                self.completion_selected = None;
                self.stick.engage(&self.timeline_scroll, cx.reduce_motion());
            }
            Err(error) => self.submission_error = Some(error),
        }
        cx.notify();
    }

    fn render_attachments(&self, view: &Entity<Self>, cx: &gpui::App) -> Option<impl IntoElement> {
        if self.attachments.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .w_full()
                .flex_wrap()
                .gap_2()
                .px_3()
                .pt_2()
                .children(self.attachments.iter().enumerate().map(|(index, image)| {
                    let remove_view = view.clone();
                    div()
                        .relative()
                        .child(agent_attachment_thumbnail(
                            ("agent-composer-attachment", index),
                            Arc::clone(image),
                            COMPOSER_ATTACHMENT,
                            cx,
                        ))
                        .child(
                            div().absolute().top(px(-6.0)).right(px(-6.0)).child(
                                Button::new(format!(
                                    "agent-attachment-remove-{}-{index}",
                                    self.pane.0
                                ))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::Close))
                                .tooltip("Remove this image")
                                .on_click(move |_, _, cx| {
                                    remove_view.update(cx, |view, cx| {
                                        view.remove_attachment(index, cx);
                                    });
                                    cx.stop_propagation();
                                }),
                            ),
                        )
                })),
        )
    }

    fn render_empty_state(
        state: &AgentPaneState,
        view: EntityId,
        cx: &mut gpui::App,
    ) -> impl IntoElement {
        let agent = state
            .agent_name
            .as_deref()
            .unwrap_or(state.provider.label());
        let message: SharedString = match state.connection {
            AgentConnectionState::Starting => format!("Starting {agent}…").into(),
            AgentConnectionState::Restoring => "Restoring the previous session…".into(),
            AgentConnectionState::Ready => "Ask the agent to work in this workspace.".into(),
            AgentConnectionState::Running => format!("Waiting for {agent}’s first update…").into(),
            AgentConnectionState::Cancelling => "Cancelling the current turn…".into(),
            AgentConnectionState::Failed => "The agent could not start this session.".into(),
            AgentConnectionState::Disconnected => "The ACP agent is offline.".into(),
        };
        let busy = pane_is_busy(state.connection);
        v_flex()
            .w_full()
            .py(px(48.0))
            .items_center()
            .gap_2()
            .text_size(zz_ui::rems_from_px(12.0))
            .text_color(cx.theme().foreground.muted())
            .when(busy, |this| {
                this.child(agent_spinner(
                    Size::Small,
                    cx.theme().foreground.muted(),
                    view,
                    cx,
                ))
            })
            .child(message)
    }

    /// One pending permission request at a time, with its page counter, its
    /// options numbered for the digit keys, and the cancel path.
    fn render_permission_wizard(
        &self,
        state: &AgentPaneState,
        view: &Entity<Self>,
        cx: &gpui::App,
    ) -> Option<AnyElement> {
        let permission = self.permission_wizard.current(&state.pending_permissions)?;
        let counter = self
            .permission_wizard
            .page_label(&state.pending_permissions);
        let highlighted = self.permission_wizard.highlighted;
        let request_id = permission.request_id;
        let options = permission
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let answer_view = view.clone();
                let hover_view = view.clone();
                let button = Button::new(format!(
                    "agent-permission-{}-{request_id}-{index}",
                    self.pane.0
                ))
                .small()
                .label(option.name.clone())
                .on_click(move |_, _, cx| {
                    answer_view.update(cx, |view, cx| {
                        view.answer_permission(index, cx);
                    });
                    cx.stop_propagation();
                });
                let button = match option.kind {
                    AgentPermissionKind::AllowOnce | AgentPermissionKind::AllowAlways => {
                        button.primary()
                    }
                    AgentPermissionKind::RejectOnce | AgentPermissionKind::RejectAlways => {
                        button.danger()
                    }
                };
                h_flex()
                    .id(format!(
                        "agent-permission-option-{}-{request_id}-{index}",
                        self.pane.0
                    ))
                    .w_full()
                    .items_center()
                    .gap_2()
                    .rounded(cx.theme().radius)
                    .px_1()
                    .py(px(2.0))
                    .when(index == highlighted, |this| {
                        this.bg(cx.theme().background.hover())
                    })
                    .on_hover(move |hovered, _, cx| {
                        if *hovered {
                            hover_view.update(cx, |view, cx| {
                                if view.permission_wizard.highlight(index) {
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .when(index < 9, |this| {
                        this.child(
                            div()
                                .flex_none()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().background.raised(2))
                                .px_2()
                                .py(px(2.0))
                                .text_size(zz_ui::rems_from_px(9.0))
                                .text_color(cx.theme().foreground.muted())
                                .child(format!("{}", index + 1)),
                        )
                    })
                    .child(button)
            })
            .collect::<Vec<_>>();
        let cancel_view = view.clone();
        Some(
            v_flex()
                .w_full()
                .gap_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().warning.outline())
                .bg(cx.theme().warning.fill())
                .p_3()
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .text_size(zz_ui::rems_from_px(12.0))
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .small()
                                .flex_none()
                                .text_color(cx.theme().warning),
                        )
                        .child(div().min_w_0().flex_1().child(permission.title.clone()))
                        .when_some(counter, |this, counter| {
                            this.child(
                                div()
                                    .flex_none()
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().background.raised(2))
                                    .px_2()
                                    .py(px(2.0))
                                    .text_size(zz_ui::rems_from_px(9.0))
                                    .text_color(cx.theme().foreground.muted())
                                    .child(counter),
                            )
                        }),
                )
                .child(v_flex().w_full().gap_1().children(options))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_size(zz_ui::rems_from_px(9.0))
                                .text_color(cx.theme().foreground.muted())
                                .child("1-9 picks · enter confirms · esc cancels"),
                        )
                        .child(
                            Button::new(format!(
                                "agent-permission-cancel-{}-{request_id}",
                                self.pane.0
                            ))
                            .ghost()
                            .small()
                            .label("Cancel request")
                            .on_click(move |_, _, cx| {
                                cancel_view.update(cx, |view, cx| {
                                    view.cancel_permission(cx);
                                });
                                cx.stop_propagation();
                            }),
                        ),
                )
                .into_any_element(),
        )
    }

    /// How many prompts are waiting behind the live turn, and the way back:
    /// clicking hands the whole queue to the composer draft.
    fn render_queue_chip(
        &self,
        state: &AgentPaneState,
        cx: &gpui::App,
    ) -> Option<impl IntoElement> {
        let queued = state.queued_prompts;
        if queued == 0 {
            return None;
        }
        let controller = self.controller.clone();
        let pane = self.pane;
        Some(
            h_flex().w_full().justify_end().child(
                Button::new(format!("agent-unqueue-{}", self.pane.0))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Undo2)
                    .label(format!("{queued} queued"))
                    .tooltip("Return the queued prompts to the composer")
                    .text_color(cx.theme().foreground.muted())
                    .on_click(move |_, _, cx| {
                        controller.update(cx, |controller, cx| {
                            controller.unqueue_prompts(pane, cx);
                        });
                        cx.stop_propagation();
                    }),
            ),
        )
    }

    fn answer_permission(&mut self, option: usize, cx: &mut Context<Self>) {
        let requests = self.pane_state.pending_permissions.clone();
        let step = self.permission_wizard.answer(&requests, Some(option));
        self.apply_permission_step(step, cx);
    }

    fn cancel_permission(&mut self, cx: &mut Context<Self>) {
        let requests = self.pane_state.pending_permissions.clone();
        let step = self.permission_wizard.cancel(&requests);
        self.apply_permission_step(step, cx);
    }

    fn render_error(&self, state: &AgentPaneState, cx: &gpui::App) -> Option<impl IntoElement> {
        let runtime_error = state.error.clone();
        let error = runtime_error
            .clone()
            .or_else(|| self.submission_error.clone())?;
        let retry_controller = self.controller.clone();
        let pane = self.pane;
        Some(
            v_flex()
                .w_full()
                .gap_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().danger.outline())
                .bg(cx.theme().danger.fill())
                .p_3()
                .text_size(zz_ui::rems_from_px(11.0))
                .child(rendered_error(&error))
                .when(
                    runtime_error.is_some() && state.connection == AgentConnectionState::Failed,
                    |this| {
                        let auth_buttons =
                            state
                                .auth_methods
                                .iter()
                                .enumerate()
                                .map(|(index, method)| {
                                    let controller = self.controller.clone();
                                    let method_id = method.id.clone();
                                    Button::new(format!("agent-auth-{}-{index}", self.pane.0))
                                        .secondary()
                                        .small()
                                        .label(method.name.clone())
                                        .tooltip(method.description.clone().unwrap_or_else(|| {
                                            "Authenticate with the agent".to_owned()
                                        }))
                                        .on_click(move |_, _, cx| {
                                            controller.update(cx, |controller, cx| {
                                                controller.authenticate(
                                                    pane,
                                                    method_id.clone(),
                                                    cx,
                                                );
                                            });
                                        })
                                });
                        this.child(
                            h_flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    Button::new(format!("agent-retry-{}", self.pane.0))
                                        .primary()
                                        .small()
                                        .icon(IconName::Redo2)
                                        .label(if state.lifecycle_pending {
                                            "Restarting…"
                                        } else {
                                            "Try again"
                                        })
                                        .disabled(state.lifecycle_pending)
                                        .on_click(move |_, _, cx| {
                                            retry_controller.update(cx, |controller, cx| {
                                                controller.retry(pane, cx);
                                            });
                                        }),
                                )
                                .children(auth_buttons),
                        )
                    },
                ),
        )
    }

    fn render_agent_picker(&self, state: &AgentPaneState, view: Entity<Self>) -> impl IntoElement {
        let selected = state.provider;
        let disabled = state.connection.has_active_turn() || state.lifecycle_pending;
        let tooltip = if state.lifecycle_pending {
            "Waiting for the daemon to switch agents".to_owned()
        } else if disabled {
            "Finish or cancel the current turn before switching agents".to_owned()
        } else {
            state.agent_name.as_deref().map_or_else(
                || "Choose an ACP agent".to_owned(),
                |name| format!("{name} · switch agent"),
            )
        };
        agent_chrome_button(("agent-provider-picker", self.pane.0))
            .icon(provider_icon(selected))
            .label(selected.label())
            .dropdown_caret(true)
            .tooltip(tooltip)
            .disabled(disabled)
            .dropdown_menu(move |menu, _, _| {
                AgentProvider::ALL
                    .iter()
                    .fold(menu.min_w(px(190.0)), |menu, provider| {
                        let provider = *provider;
                        let picker_view = view.clone();
                        menu.item(
                            PopupMenuItem::new(provider.label())
                                .icon(provider_icon(provider))
                                .checked(provider == selected)
                                .on_click(move |_, _, cx| {
                                    picker_view.update(cx, |view, cx| {
                                        let result =
                                            view.controller.update(cx, |controller, cx| {
                                                controller.select_provider(view.pane, provider, cx)
                                            });
                                        view.submission_error = result.err();
                                        cx.notify();
                                    });
                                }),
                        )
                    })
            })
            .anchor(Anchor::TopLeft)
    }

    fn render_history_button(
        &self,
        state: &AgentPaneState,
        view: Entity<Self>,
    ) -> impl IntoElement {
        let enabled = state.session_capabilities.list && state.connection.accepts_prompt();
        agent_chrome_icon_button(("agent-session-history", self.pane.0))
            .icon(IconName::History)
            .tooltip(if enabled {
                "Browse sessions stored by this agent"
            } else if state.session_capabilities.list {
                "Wait for the current turn to finish"
            } else {
                "This agent does not advertise session history"
            })
            .disabled(!enabled)
            .on_click(move |_, window, cx| {
                view.update(cx, |view, cx| view.open_history(window, cx));
                cx.stop_propagation();
            })
    }

    /// The jump-to-bottom pill, floated over the timeline just above the
    /// composer: an absolute child, so showing it never resizes the scroll
    /// viewport and moves the very tail it is offering to reveal.
    fn render_jump_to_end(&self, view: &Entity<Self>, cx: &gpui::App) -> impl IntoElement {
        let view = view.clone();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(CHROME_GAP))
            .flex()
            .justify_center()
            .child(
                agent_jump_to_bottom_button(("agent-jump-to-end", self.pane.0), cx).on_click(
                    move |_, _, cx| {
                        view.update(cx, AgentView::jump_to_timeline_end);
                        cx.stop_propagation();
                    },
                ),
            )
    }

    fn render_directory_picker(
        &self,
        state: &AgentPaneState,
        view: Entity<Self>,
        local_host: bool,
    ) -> impl IntoElement {
        let ready = directory_picker_enabled(
            state.connection,
            !state.pending_permissions.is_empty(),
            local_host,
        );
        let cwd = state.cwd.display().to_string();
        agent_chrome_button(("agent-working-directory", self.pane.0))
            .icon(IconName::FolderOpen)
            .label(session_directory_label(&state.cwd))
            .tooltip(if !local_host {
                format!("{cwd} · working directory is managed by the remote host")
            } else if state.connection.has_active_turn() || !state.pending_permissions.is_empty() {
                "Finish or cancel the current turn before changing the working directory".to_owned()
            } else if !state.connection.accepts_prompt() {
                "Wait for the agent to be ready before changing the working directory".to_owned()
            } else {
                format!("{cwd} · choose another workspace (starts a new session)")
            })
            .disabled(!ready)
            .on_click(move |_, window, cx| {
                view.update(cx, |view, cx| view.open_directory_picker(window, cx));
                cx.stop_propagation();
            })
    }

    fn open_directory_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.directory_picker.is_some() {
            return;
        }
        let root = directory_picker_root(&self.pane_state.cwd);
        let picker = cx.new(|cx| {
            FilePickerView::new(
                FilePickerMode::Directories,
                root,
                "Choose the agent's working directory",
                window,
                cx,
            )
        });
        cx.subscribe_in(
            &picker,
            window,
            |view, _, event: &FilePickerEvent, window, cx| {
                view.directory_picker = None;
                if let FilePickerEvent::Selected(path) = event {
                    let pane = view.pane;
                    let path = path.clone();
                    let result = view.controller.update(cx, |controller, cx| {
                        controller.set_working_directory(pane, &path, cx)
                    });
                    view.submission_error = result.err();
                }
                view.focus(cx).focus(window, cx);
                cx.notify();
            },
        )
        .detach();
        self.completions = Arc::from([]);
        self.completion_selected = None;
        self.directory_picker = Some(picker);
        cx.notify();
    }

    fn render_history_overlay(
        &self,
        state: &AgentPaneState,
        view: &Entity<Self>,
        cx: &gpui::App,
    ) -> impl IntoElement {
        let sessions = state.session_history.sessions.clone();
        let results = self.history_results.clone();
        let selected = self.history_selected;
        let current_session = state.session_id.clone();
        let can_delete = state.session_capabilities.delete;
        let loading = state.session_history.loading;
        let pane = self.pane;
        let today = Local::now().date_naive();
        let rows_view = view.clone();
        let rows = uniform_list(
            ("agent-history-rows", pane.0),
            results.len(),
            move |range, _, cx| {
                range
                    .filter_map(|result_index| {
                        let session_index = *results.get(result_index)?;
                        let session = sessions.get(session_index)?.clone();
                        let title = session_display_title(&session);
                        let directory = session_directory_label(&session.cwd);
                        let updated_at = session
                            .updated_at
                            .as_deref()
                            .map(|value| format_session_timestamp(value, today));
                        let is_selected = selected == Some(result_index);
                        let is_current =
                            current_session.as_deref() == Some(session.session_id.as_str());
                        let pointer_view = rows_view.clone();
                        let click_view = rows_view.clone();
                        let delete_view = rows_view.clone();
                        let delete_id: Arc<str> = Arc::from(session.session_id);
                        Some(
                            h_flex()
                                .id(format!("agent-history-row-{}-{result_index}", pane.0))
                                .w_full()
                                .h(px(HISTORY_ROW_HEIGHT))
                                .items_center()
                                .gap_2()
                                .rounded(cx.theme().radius)
                                .px_2p5()
                                .cursor_pointer()
                                .when(is_selected, |this| this.bg(cx.theme().background.hover()))
                                .when(!is_selected, |this| {
                                    this.hover(|this| this.bg(cx.theme().background.hover()))
                                })
                                .on_mouse_move(move |_, _, cx| {
                                    pointer_view.update(cx, |view, cx| {
                                        if view.history_selected != Some(result_index) {
                                            view.history_selected = Some(result_index);
                                            cx.notify();
                                        }
                                    });
                                })
                                .on_click(move |_, window, cx| {
                                    click_view.update(cx, |view, cx| {
                                        view.open_history_result(result_index, window, cx);
                                    });
                                    cx.stop_propagation();
                                })
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .w_full()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .text_size(zz_ui::rems_from_px(12.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .child(title),
                                        )
                                        .child(
                                            h_flex()
                                                .min_w_0()
                                                .gap_1()
                                                .text_size(zz_ui::rems_from_px(9.0))
                                                .text_color(cx.theme().foreground.muted())
                                                .child(
                                                    Tag::secondary()
                                                        .xsmall()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .text_size(zz_ui::rems_from_px(9.0))
                                                        .text_color(cx.theme().foreground.muted())
                                                        .child(directory),
                                                )
                                                .when(is_current, |this| {
                                                    this.child(
                                                        div()
                                                            .flex_none()
                                                            .rounded(px(999.0))
                                                            .bg(cx.theme().success.fill())
                                                            .px_1()
                                                            .text_size(zz_ui::rems_from_px(8.0))
                                                            .text_color(cx.theme().success)
                                                            .child("CURRENT"),
                                                    )
                                                })
                                                .child(div().flex_1())
                                                .when_some(updated_at, |this, updated_at| {
                                                    this.child(
                                                        div()
                                                            .flex_none()
                                                            .max_w(px(112.0))
                                                            .overflow_hidden()
                                                            .text_ellipsis()
                                                            .whitespace_nowrap()
                                                            .child(updated_at),
                                                    )
                                                }),
                                        ),
                                )
                                .when(can_delete && !is_current, |this| {
                                    this.child(
                                        Button::new(format!(
                                            "agent-history-delete-{}-{result_index}",
                                            pane.0
                                        ))
                                        .ghost()
                                        .xsmall()
                                        .icon(
                                            Icon::new(IconName::Delete)
                                                .text_color(cx.theme().danger),
                                        )
                                        .tooltip("Delete this session")
                                        .disabled(loading)
                                        .on_click(
                                            move |_, _, cx| {
                                                let delete_id = delete_id.clone();
                                                delete_view.update(cx, |view, cx| {
                                                    view.history_delete_confirmation =
                                                        Some(delete_id);
                                                    cx.notify();
                                                });
                                                cx.stop_propagation();
                                            },
                                        ),
                                    )
                                }),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .flex_1()
        .track_scroll(&self.history_scroll);

        let refresh_view = view.clone();
        let scope_view = view.clone();
        let new_view = view.clone();
        let backdrop_view = view.clone();
        let load_more_view = view.clone();
        let scope_label = if self.history_all_projects {
            "All projects"
        } else {
            "This project"
        };
        let scope_icon = if self.history_all_projects {
            IconName::Globe
        } else {
            IconName::Folder
        };
        let delete_confirmation = self.history_delete_confirmation.clone();
        let footer = if let Some(session_id) = delete_confirmation {
            let cancel_view = view.clone();
            let delete_view = view.clone();
            let confirm_id = session_id.clone();
            h_flex()
                .w_full()
                .min_h(px(52.0))
                .flex_none()
                .justify_between()
                .gap(px(CHROME_GAP))
                .border_t_1()
                .border_color(cx.theme().danger.outline())
                .bg(cx.theme().danger.fill())
                .py(px(CHROME_GAP))
                .pl_4()
                .pr(px(CHROME_GAP))
                .text_size(zz_ui::rems_from_px(11.0))
                .child("Permanently delete this session from the agent’s local store?")
                .child(
                    h_flex()
                        .flex_none()
                        .gap(px(CHROME_GAP))
                        .child(
                            agent_chrome_button(("agent-history-delete-cancel", pane.0))
                                .label("Cancel")
                                .on_click(move |_, _, cx| {
                                    cancel_view.update(cx, |view, cx| {
                                        view.history_delete_confirmation = None;
                                        cx.notify();
                                    });
                                    cx.stop_propagation();
                                }),
                        )
                        .child(
                            agent_chrome_button(("agent-history-delete-confirm", pane.0))
                                .danger()
                                .label("Delete")
                                .disabled(loading)
                                .on_click(move |_, _, cx| {
                                    delete_view.update(cx, |view, cx| {
                                        view.confirm_delete_history(&confirm_id, cx);
                                    });
                                    cx.stop_propagation();
                                }),
                        ),
                )
                .into_any_element()
        } else {
            let error = self
                .submission_error
                .clone()
                .or_else(|| state.session_history.error.clone());
            h_flex()
                .w_full()
                .min_h(px(40.0))
                .flex_none()
                .gap(px(CHROME_GAP))
                .border_t_1()
                .border_color(cx.theme().border)
                .py(px(CHROME_GAP))
                .pl_4()
                .pr(px(CHROME_GAP))
                .text_size(zz_ui::rems_from_px(10.0))
                .text_color(cx.theme().foreground.muted())
                .when_some(error, |this, error| {
                    this.child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(cx.theme().danger)
                            .child(rendered_error(&error)),
                    )
                })
                .when(state.session_history.loading, |this| {
                    this.child("Loading sessions…")
                })
                .when(
                    !state.session_history.loading && state.session_history.error.is_none(),
                    |this| {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap(px(CHROME_GAP))
                                .child(palette_shortcut_hint(["up", "down"], "select"))
                                .child(palette_shortcut_hint(["enter"], "open"))
                                .child(palette_shortcut_hint(["escape"], "close")),
                        )
                    },
                )
                .child(div().flex_1())
                .when(state.session_history.next_cursor.is_some(), |this| {
                    this.child(
                        agent_chrome_button(("agent-history-load-more", pane.0))
                            .label("Load more")
                            .disabled(loading)
                            .on_click(move |_, _, cx| {
                                load_more_view.update(cx, |view, cx| {
                                    view.load_more_history(cx);
                                });
                                cx.stop_propagation();
                            }),
                    )
                })
                .into_any_element()
        };

        div()
            .id(("agent-history-overlay", pane.0))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(cx.theme().scrim)
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                backdrop_view.update(cx, |view, cx| view.close_history(window, cx));
                cx.stop_propagation();
            })
            .child(
                v_flex()
                    .id(("agent-history-modal", pane.0))
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
                            .gap(px(CHROME_GAP))
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
                                        Input::new(&self.history_input)
                                            .small()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(zz_ui::rems_from_px(12.0))
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap(px(CHROME_GAP))
                                    .child(
                                        agent_chrome_button(("agent-history-scope", pane.0))
                                            .secondary()
                                            .icon(scope_icon)
                                            .label(scope_label)
                                            .disabled(loading)
                                            .on_click(move |_, _, cx| {
                                                scope_view.update(cx, |view, cx| {
                                                    view.toggle_history_scope(cx);
                                                });
                                                cx.stop_propagation();
                                            }),
                                    )
                                    .child(
                                        agent_chrome_button(("agent-history-refresh", pane.0))
                                            .icon(IconName::Redo2)
                                            .label("Refresh")
                                            .disabled(loading)
                                            .on_click(move |_, _, cx| {
                                                refresh_view.update(cx, |view, cx| {
                                                    view.refresh_history(cx);
                                                });
                                                cx.stop_propagation();
                                            }),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        agent_chrome_button(("agent-history-new", pane.0))
                                            .icon(IconName::Plus)
                                            .label("New")
                                            .tooltip("Start a new session")
                                            .disabled(loading)
                                            .on_click(move |_, window, cx| {
                                                new_view.update(cx, |view, cx| {
                                                    view.start_new_session(window, cx);
                                                });
                                                cx.stop_propagation();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .p(px(CHROME_GAP))
                            .child(rows)
                            .when(self.history_results.is_empty(), |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(zz_ui::rems_from_px(11.0))
                                        .text_color(cx.theme().foreground.muted())
                                        .child(if state.session_history.loading {
                                            "Loading sessions…"
                                        } else if self.history_input.read(cx).value().is_empty() {
                                            "No sessions found for this scope."
                                        } else {
                                            "No sessions match that search."
                                        }),
                                )
                            }),
                    )
                    .child(footer),
            )
    }

    fn render_config_picker(
        &self,
        option: &AgentConfigOption,
        icon: IconName,
        view: Entity<Self>,
        enabled: bool,
    ) -> gpui::AnyElement {
        let current_label = option
            .choices
            .iter()
            .find(|choice| choice.value == option.current_value)
            .map_or_else(|| option.name.clone(), |choice| choice.name.clone());
        let option_id = option.id.clone();
        let current_value = option.current_value.clone();
        let choices = option.choices.clone();
        let description = option
            .description
            .clone()
            .unwrap_or_else(|| option.name.clone());
        agent_chrome_button(format!("agent-config-picker-{}-{}", self.pane.0, option.id))
            .icon(icon)
            .label(current_label)
            .dropdown_caret(true)
            .tooltip(description)
            .disabled(!enabled || choices.is_empty())
            .dropdown_menu(move |menu, _, _| {
                choices.iter().fold(menu.min_w(px(250.0)), |menu, choice| {
                    let choice_name = choice.name.clone();
                    let choice_description = choice.description.clone();
                    let value = choice.value.clone();
                    let config_id = option_id.clone();
                    let picker_view = view.clone();
                    menu.item(
                        PopupMenuItem::element(move |_, cx| {
                            v_flex()
                                .min_w_0()
                                .ml_1()
                                .py_1()
                                .child(
                                    div()
                                        .text_size(zz_ui::rems_from_px(12.0))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(choice_name.clone()),
                                )
                                .when_some(choice_description.clone(), |this, description| {
                                    this.child(
                                        div()
                                            .max_w(px(300.0))
                                            .text_size(zz_ui::rems_from_px(10.0))
                                            .text_color(cx.theme().foreground.muted())
                                            .child(description),
                                    )
                                })
                        })
                        .checked(choice.value == current_value)
                        .on_click(move |_, _, cx| {
                            picker_view.update(cx, |view, cx| {
                                let result = view.controller.update(cx, |controller, cx| {
                                    controller.set_config_option(view.pane, &config_id, &value, cx)
                                });
                                view.submission_error = result.err();
                                cx.notify();
                            });
                        }),
                    )
                })
            })
            .anchor(Anchor::BottomLeft)
            .into_any_element()
    }

    fn render_legacy_mode_picker(
        &self,
        state: &AgentPaneState,
        view: Entity<Self>,
        enabled: bool,
    ) -> Option<gpui::AnyElement> {
        if !state.config_options.is_empty() || state.modes.is_empty() {
            return None;
        }
        let current = state.mode.clone();
        let current_label = state
            .modes
            .iter()
            .find(|mode| current.as_deref() == Some(mode.id.as_str()))
            .map_or("Permissions", |mode| mode.name.as_str())
            .to_owned();
        let modes = state.modes.clone();
        Some(
            agent_chrome_button(("agent-mode-picker", self.pane.0))
                .icon(IconName::Check)
                .label(current_label)
                .dropdown_caret(true)
                .tooltip("Agent permission mode")
                .disabled(!enabled)
                .dropdown_menu(move |menu, _, _| {
                    modes.iter().fold(menu.min_w(px(250.0)), |menu, mode| {
                        let mode_name = mode.name.clone();
                        let mode_description = mode.description.clone();
                        let mode_id = mode.id.clone();
                        let mode_view = view.clone();
                        menu.item(
                            PopupMenuItem::element(move |_, cx| {
                                v_flex()
                                    .ml_1()
                                    .py_1()
                                    .child(
                                        div()
                                            .text_size(zz_ui::rems_from_px(12.0))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(mode_name.clone()),
                                    )
                                    .when_some(mode_description.clone(), |this, description| {
                                        this.child(
                                            div()
                                                .max_w(px(300.0))
                                                .text_size(zz_ui::rems_from_px(10.0))
                                                .text_color(cx.theme().foreground.muted())
                                                .child(description),
                                        )
                                    })
                            })
                            .checked(current.as_deref() == Some(mode.id.as_str()))
                            .on_click(move |_, _, cx| {
                                mode_view.update(cx, |view, cx| {
                                    let result = view.controller.update(cx, |controller, cx| {
                                        controller.set_mode(view.pane, &mode_id, cx)
                                    });
                                    view.submission_error = result.err();
                                    cx.notify();
                                });
                            }),
                        )
                    })
                })
                .anchor(Anchor::BottomLeft)
                .into_any_element(),
        )
    }

    fn render_completions(&self, view: &Entity<Self>, cx: &gpui::App) -> Option<gpui::AnyElement> {
        if self.completions.is_empty() {
            return None;
        }
        let selected = self.completion_selected;
        let visible_rows = self
            .completions
            .len()
            .min(usize::from(MAX_VISIBLE_COMPLETION_ROWS));
        let visible_rows =
            f32::from(u8::try_from(visible_rows).unwrap_or(MAX_VISIBLE_COMPLETION_ROWS));
        let pane = self.pane;
        let completions = Arc::clone(&self.completions);
        let rows_view = view.clone();
        let rows = uniform_list(
            ("agent-completion-rows", pane.0),
            completions.len(),
            move |range, _, cx| {
                range
                    .filter_map(|index| {
                        let completion = completions.get(index)?.clone();
                        let command = completion.command.clone();
                        let name = bare_command_name(&command.name);
                        let description = meaningful_command_description(&command.description)
                            .map(ToOwned::to_owned);
                        let is_selected = selected == Some(index);
                        let hover_view = rows_view.clone();
                        let click_view = rows_view.clone();
                        Some(
                            h_flex()
                                .id(format!("agent-completion-{}-{index}", pane.0))
                                .w_full()
                                .h(px(COMPLETION_ROW_HEIGHT))
                                .items_center()
                                .gap_3()
                                .rounded(cx.theme().radius)
                                .px_3()
                                .cursor_pointer()
                                .when(is_selected, |this| this.bg(cx.theme().background.hover()))
                                .when(!is_selected, |this| {
                                    this.hover(|this| this.bg(cx.theme().background.hover()))
                                })
                                .on_hover(move |hovered, _, cx| {
                                    if *hovered {
                                        hover_view.update(cx, |view, cx| {
                                            if view.completion_selected != Some(index) {
                                                view.completion_selected = Some(index);
                                                cx.notify();
                                            }
                                        });
                                    }
                                })
                                .on_click(move |_, window, cx| {
                                    click_view.update(cx, |view, cx| {
                                        view.accept_completion(index, window, cx);
                                    });
                                    cx.stop_propagation();
                                })
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .w_full()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .text_size(zz_ui::rems_from_px(12.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .child(format!("/{name}")),
                                        )
                                        .when_some(description, |this, description| {
                                            this.child(
                                                div()
                                                    .w_full()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .whitespace_nowrap()
                                                    .text_size(zz_ui::rems_from_px(10.0))
                                                    .text_color(cx.theme().foreground.muted())
                                                    .child(description),
                                            )
                                        }),
                                ),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .w_full()
        .h(px(COMPLETION_ROW_HEIGHT * visible_rows))
        .track_scroll(&self.completion_scroll);
        Some(
            v_flex()
                .w_full()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background.raised(1))
                .p_1()
                .shadow_md()
                .overflow_hidden()
                .child(rows)
                .into_any_element(),
        )
    }

    #[allow(clippy::redundant_closure_for_method_calls)]
    fn render_composer(
        &self,
        state: &AgentPaneState,
        view: &Entity<Self>,
        local_host: bool,
        cx: &mut gpui::App,
    ) -> impl IntoElement {
        let can_submit = state.connection.accepts_prompt();
        let button = Button::new(format!("agent-action-{}", self.pane.0))
            .small()
            .size(px(COMPOSER_ACTION_SIZE))
            .rounded_full();
        let action = match composer_action(
            state.connection.has_active_turn(),
            self.composer_has_content(),
        ) {
            ComposerAction::Send => {
                let submit_view = view.clone();
                button
                    .primary()
                    .icon(IconName::ArrowUp)
                    .tooltip("Send message")
                    .disabled(!can_submit)
                    .on_click(move |_, window, cx| {
                        submit_view.update(cx, |view, cx| view.submit(window, cx));
                    })
            }
            ComposerAction::Queue => {
                let submit_view = view.clone();
                button
                    .secondary()
                    .icon(IconName::Plus)
                    .tooltip("Queue this as the next turn")
                    .on_click(move |_, window, cx| {
                        submit_view.update(cx, |view, cx| view.submit(window, cx));
                    })
            }
            ComposerAction::Stop => {
                let controller = self.controller.clone();
                let pane = self.pane;
                button
                    .danger()
                    .icon(IconName::Close)
                    .tooltip("Stop the current turn")
                    .on_click(move |_, _, cx| {
                        controller.update(cx, |controller, cx| controller.cancel(pane, cx));
                    })
            }
        };
        let settings_enabled = state.connection.accepts_prompt() && !state.settings_busy;
        let permission_option = state
            .config_options
            .iter()
            .find(|option| option.category == AgentConfigCategory::Mode);
        let effort_option = state
            .config_options
            .iter()
            .find(|option| option.category == AgentConfigCategory::ThoughtLevel);
        let model_option = state
            .config_options
            .iter()
            .find(|option| option.category == AgentConfigCategory::Model);
        let mut settings = Vec::with_capacity(3);
        if let Some(option) = permission_option {
            settings.push(self.render_config_picker(
                option,
                IconName::Check,
                view.clone(),
                settings_enabled,
            ));
        } else if let Some(mode) =
            self.render_legacy_mode_picker(state, view.clone(), settings_enabled)
        {
            settings.push(mode);
        }
        if let Some(option) = model_option {
            settings.push(self.render_config_picker(
                option,
                IconName::Asterisk,
                view.clone(),
                settings_enabled,
            ));
        }
        if let Some(option) = effort_option {
            settings.push(self.render_config_picker(
                option,
                IconName::Cpu,
                view.clone(),
                settings_enabled,
            ));
        }
        let usage = state
            .usage
            .map(|(used, size)| context_usage_meter(self.pane, used, size, cx));
        let git = state
            .git
            .as_ref()
            .map(|git| git_summary_footer(self.pane, git, cx));
        let directory = self.render_directory_picker(state, view.clone(), local_host);
        let command_hint = active_command_hint(&self.last_input, &state.available_commands);
        let completions = self.render_completions(view, cx);

        v_flex()
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .bottom(px(0.0))
            .w_full()
            .p(px(COMPOSER_OUTER_PADDING))
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(COMPOSER_MAX_WIDTH))
                    .mx_auto()
                    .gap(px(COMPOSER_SECTION_GAP))
                    .when_some(
                        self.render_permission_wizard(state, view, cx),
                        |this, wizard| this.child(wizard),
                    )
                    .when_some(self.render_queue_chip(state, cx), |this, chip| {
                        this.child(chip)
                    })
                    .when_some(self.render_error(state, cx), |this, error| {
                        this.child(error)
                    })
                    .when_some(completions, |this, completions| this.child(completions))
                    .child(
                        v_flex()
                            .w_full()
                            .min_h(px(COMPOSER_MIN_HEIGHT))
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background.raised(1))
                            .when(cx.theme().shadow, |this| this.shadow_xs())
                            .children(self.render_attachments(view, cx))
                            .child(
                                Input::new(&self.input)
                                    .w_full()
                                    .min_w_0()
                                    .text_size(zz_ui::rems_from_px(13.0))
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false),
                            )
                            .when_some(command_hint, |this, hint| {
                                this.child(
                                    div()
                                        .px_3()
                                        .pb_1()
                                        .text_size(zz_ui::rems_from_px(10.0))
                                        .text_color(cx.theme().foreground.muted())
                                        .child(hint),
                                )
                            })
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_h(px(40.0))
                                    .items_end()
                                    .justify_between()
                                    .gap(px(CHROME_GAP))
                                    .p(px(CHROME_GAP))
                                    .child(
                                        h_flex()
                                            .min_w_0()
                                            .flex_wrap()
                                            .gap(px(CHROME_GAP))
                                            .children(settings),
                                    )
                                    .child(h_flex().flex_none().gap(px(CHROME_GAP)).child(action)),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .h(px(COMPOSER_FOOTER_HEIGHT))
                            .items_center()
                            .justify_between()
                            .px_1()
                            .child(h_flex().min_w_0().flex_1().children(git))
                            .child(
                                h_flex()
                                    .flex_none()
                                    .gap(px(CHROME_GAP))
                                    .children(usage)
                                    .child(directory),
                            ),
                    ),
            )
    }
}

impl Focusable for AgentView {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.focus(cx)
    }
}

impl AgentView {
    fn drain_pending_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane;
        let (pending, images) = self.controller.update(cx, |controller, _| {
            (
                controller.take_pending_composer(pane),
                controller.take_pending_images(pane),
            )
        });
        if pending.is_none() && images.is_empty() {
            return;
        }
        self.attachments.extend(images);
        if let Some(pending) = pending {
            self.input.update(cx, |input, cx| {
                let current = input.value().to_string();
                let value = if current.trim().is_empty() {
                    pending
                } else {
                    format!("{}\n{pending}", current.trim_end_matches('\n'))
                };
                input.set_value(value, window, cx);
            });
        }
        self.submission_error = None;
        self.stick.engage(&self.timeline_scroll, cx.reduce_motion());
    }

    fn on_timeline_scroll(&mut self, cx: &mut Context<Self>) {
        let reduce_motion = cx.reduce_motion();
        if self
            .stick
            .on_user_scroll(&self.timeline_scroll, reduce_motion)
        {
            cx.notify();
        }
    }

    fn jump_to_timeline_end(&mut self, cx: &mut Context<Self>) {
        self.stick.engage(&self.timeline_scroll, cx.reduce_motion());
        cx.notify();
    }

    /// Ask for one spring frame, at most one callback in flight. Each step
    /// notifies while it still has travel left, which re-enters `render` and
    /// arms the next frame; the loop stops itself once the spring parks.
    fn drive_stick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.reduce_motion() || !self.stick.wants_frame(&self.timeline_scroll) {
            return;
        }
        self.stick.arm();
        let view = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            view.update(cx, |view: &mut Self, cx| {
                let list = view.timeline_scroll.clone();
                if view.stick.step(&list) {
                    cx.notify();
                }
            })
            .ok();
        });
    }
}

impl Render for AgentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_pending_composer(window, cx);
        self.permission_wizard
            .sync(&self.pane_state.pending_permissions);
        let state = self.pane_state.clone();
        let rows = self.timeline.rows.clone();
        let timeline_clearance = composer_tail_clearance();
        self.stick.set_bottom_padding(timeline_clearance);
        self.drive_stick(window, cx);
        let view = cx.entity();
        let local_host = self.mux.read(cx).attached_host() == HostId::LOCAL;
        let header_controls = self.render_agent_picker(&state, view.clone());
        let header_actions = h_flex()
            .flex_none()
            .gap(px(CHROME_GAP))
            .child(self.render_history_button(&state, view.clone()));
        let input_focus = self.focus(cx);
        let root = div()
            .id(("agent-pane", self.pane.0))
            .key_context(AGENT_KEY_CONTEXT)
            .track_focus(&input_focus)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .bg(crate::theme::app_pane_background(cx))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .capture_action(cx.listener(Self::move_completion_up))
            .capture_action(cx.listener(Self::move_completion_down))
            .capture_action(cx.listener(Self::complete))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(agent_pane_header(header_controls, header_actions, cx))
            .child(
                div()
                    .id(("agent-thread-scroll", self.pane.0))
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(rows.is_empty(), |this| {
                        this.child(
                            div().w_full().px_3().child(
                                div()
                                    .w_full()
                                    .max_w(px(AGENT_CONTENT_MAX_WIDTH))
                                    .mx_auto()
                                    .child(Self::render_empty_state(&state, cx.entity_id(), cx)),
                            ),
                        )
                    })
                    .when(!rows.is_empty(), |this| {
                        this.child(
                            AgentTimeline::new(
                                rows,
                                self.timeline_scroll.clone(),
                                self.timeline_store.clone(),
                            )
                            .active_turn(state.connection.has_active_turn())
                            .bottom_padding(timeline_clearance),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .bottom(px(timeline_clearance))
                                .child(Scrollbar::vertical(&self.timeline_scroll)),
                        )
                        .when(self.stick.shows_jump_button(), |this| {
                            this.child(self.render_jump_to_end(&view, cx))
                        })
                    }),
            )
            .child(self.render_composer(&state, &view, local_host, cx))
            .when(self.history_open, |this| {
                this.child(self.render_history_overlay(&state, &view, cx))
            })
            .when_some(self.directory_picker.clone(), |this, picker| {
                this.child(picker)
            });
        round_div_radii(root, pane_content_radii(cx, self.window_corners))
    }
}

fn rendered_error(error: &str) -> String {
    let mut rendered = String::with_capacity(MAX_RENDERED_ERROR_BYTES);
    let mut truncated = false;
    for character in error.trim().chars() {
        let character = if character.is_control() && !matches!(character, '\n' | '\t') {
            '�'
        } else {
            character
        };
        if rendered.len().saturating_add(character.len_utf8()) > MAX_RENDERED_ERROR_BYTES - 3 {
            truncated = true;
            break;
        }
        rendered.push(character);
    }
    if truncated {
        rendered.push('…');
    }
    rendered
}

fn disconnected_pane_state() -> AgentPaneState {
    AgentPaneState {
        provider: AgentProvider::Codex,
        connection: AgentConnectionState::Disconnected,
        pending_permissions: Arc::from([]),
        auth_methods: Arc::from([]),
        error: Some(Arc::from("agent pane is not registered")),
        agent_name: None,
        cwd: PathBuf::from("/"),
        session_id: None,
        session_capabilities: AgentSessionCapabilities::default(),
        session_history: AgentSessionHistoryState::default(),
        settings_busy: false,
        lifecycle_pending: false,
        mode: None,
        modes: Arc::from([]),
        config_options: Arc::from([]),
        available_commands: Arc::from([]),
        usage: None,
        git: None,
        pending_composer: None,
        queued_prompts: 0,
    }
}

#[cfg(test)]
fn ui_entries(entries: &[AgentThreadEntry]) -> Arc<[AgentEntry]> {
    ui_entries_with_markdown(entries, &mut HashMap::new(), &mut HashMap::new())
}

#[cfg(test)]
fn ui_entry(entry: &AgentThreadEntry) -> AgentEntry {
    ui_entry_with_markdown(entry, &mut HashMap::new(), &mut HashMap::new())
}

fn ui_entries_with_markdown(
    entries: &[AgentThreadEntry],
    markdown: &mut HashMap<u64, AgentMarkdown>,
    tool_payloads: &mut HashMap<(u64, usize), AgentToolPayload>,
) -> Arc<[AgentEntry]> {
    entries
        .iter()
        .map(|entry| ui_entry_with_markdown(entry, markdown, tool_payloads))
        .collect::<Vec<_>>()
        .into()
}

fn streaming_markdown(
    markdown: &mut HashMap<u64, AgentMarkdown>,
    id: u64,
    source: &str,
) -> AgentMarkdown {
    let markdown = markdown
        .entry(id)
        .or_insert_with(|| AgentMarkdown::new(source));
    markdown.synchronize_append(source);
    markdown.clone()
}

fn replaced_markdown(
    markdown: &mut HashMap<u64, AgentMarkdown>,
    id: u64,
    source: &str,
) -> AgentMarkdown {
    let markdown = markdown
        .entry(id)
        .or_insert_with(|| AgentMarkdown::new(source));
    markdown.replace(source);
    markdown.clone()
}

fn ui_entry_with_markdown(
    entry: &AgentThreadEntry,
    markdown_sources: &mut HashMap<u64, AgentMarkdown>,
    tool_payloads: &mut HashMap<(u64, usize), AgentToolPayload>,
) -> AgentEntry {
    match entry {
        AgentThreadEntry::User {
            id,
            markdown,
            images,
        } => AgentEntry::User {
            id: *id,
            markdown: streaming_markdown(markdown_sources, *id, markdown),
            images: images.clone().into(),
        },
        AgentThreadEntry::Assistant { id, markdown, .. } => AgentEntry::Assistant {
            id: *id,
            markdown: streaming_markdown(markdown_sources, *id, markdown),
        },
        AgentThreadEntry::Reasoning {
            id,
            label,
            markdown,
            default_expanded,
        } => AgentEntry::Reasoning {
            id: *id,
            label: SharedString::from(label.clone()),
            markdown: streaming_markdown(markdown_sources, *id, markdown),
            default_expanded: *default_expanded,
        },
        AgentThreadEntry::Tool {
            id,
            kind,
            status,
            label,
            location,
            input,
            output,
            default_expanded,
            ..
        } => {
            tool_payloads.retain(|(entry_id, slot), _| {
                *entry_id != *id
                    || (*slot == 0 && input.is_some())
                    || (*slot > 0 && *slot <= output.len())
            });
            AgentEntry::Tool(AgentToolEntry {
                id: *id,
                kind: match kind {
                    AgentToolKindModel::Read => AgentToolKind::Read,
                    AgentToolKindModel::Search => AgentToolKind::Search,
                    AgentToolKindModel::Edit
                    | AgentToolKindModel::Delete
                    | AgentToolKindModel::Move => AgentToolKind::Edit,
                    AgentToolKindModel::Execute => AgentToolKind::Execute,
                    AgentToolKindModel::Fetch => AgentToolKind::Fetch,
                    AgentToolKindModel::Think => AgentToolKind::Think,
                    AgentToolKindModel::SwitchMode | AgentToolKindModel::Other => {
                        AgentToolKind::Other
                    }
                },
                status: match status {
                    AgentToolStatusModel::Pending => AgentToolStatus::Pending,
                    AgentToolStatusModel::Running => AgentToolStatus::Running,
                    AgentToolStatusModel::NeedsApproval => AgentToolStatus::NeedsApproval,
                    AgentToolStatusModel::Completed => AgentToolStatus::Completed,
                    AgentToolStatusModel::Failed => AgentToolStatus::Failed,
                    AgentToolStatusModel::Canceled => AgentToolStatus::Canceled,
                },
                label: SharedString::from(label.clone()),
                location: location.clone().map(SharedString::from),
                input: input
                    .as_ref()
                    .map(|payload| retained_tool_payload(tool_payloads, *id, 0, payload)),
                output: output
                    .iter()
                    .enumerate()
                    .map(|(index, payload)| {
                        retained_tool_payload(tool_payloads, *id, index + 1, payload)
                    })
                    .collect::<Vec<_>>()
                    .into(),
                default_expanded: *default_expanded,
            })
        }
        AgentThreadEntry::Plan { id, markdown } => AgentEntry::Plan {
            id: *id,
            markdown: replaced_markdown(markdown_sources, *id, markdown),
        },
    }
}

fn synchronize_entry_store(
    store: &Entity<AgentTimelineStore>,
    entry: &AgentEntry,
    cx: &mut Context<AgentView>,
) {
    match entry {
        AgentEntry::User { id, markdown, .. }
        | AgentEntry::Assistant { id, markdown }
        | AgentEntry::Reasoning { id, markdown, .. }
        | AgentEntry::Plan { id, markdown } => {
            store.update(cx, |store, cx| {
                store.synchronize_markdown(*id, MarkdownSlot::Body, markdown.clone(), cx);
            });
        }
        AgentEntry::Tool(tool) => {
            store.update(cx, |store, cx| {
                store.synchronize_tool_content(
                    tool.id,
                    tool.location.clone(),
                    tool.input.clone(),
                    tool.output.clone(),
                    cx,
                );
            });
        }
    }
}

fn retained_tool_payload(
    retained: &mut HashMap<(u64, usize), AgentToolPayload>,
    entry_id: u64,
    slot: usize,
    payload: &ToolPayload,
) -> AgentToolPayload {
    let key = (entry_id, slot);
    let next = match (retained.get(&key), payload) {
        (
            Some(AgentToolPayload::Diff {
                old: retained_old,
                new: retained_new,
                ..
            }),
            ToolPayload::Diff { path, old, new },
        ) if retained_old.is_some() == old.is_some() => {
            if let (Some(retained_old), Some(old)) = (retained_old, old) {
                retained_old.synchronize(old);
            }
            retained_new.synchronize(new);
            AgentToolPayload::Diff {
                path: path.clone().into(),
                old: retained_old.clone(),
                new: retained_new.clone(),
            }
        }
        (Some(AgentToolPayload::Text(retained)), ToolPayload::Text(text)) => {
            retained.synchronize(text);
            AgentToolPayload::Text(retained.clone())
        }
        (Some(AgentToolPayload::Json(retained)), ToolPayload::Json(text)) => {
            retained.synchronize(text);
            AgentToolPayload::Json(retained.clone())
        }
        (Some(AgentToolPayload::Terminal(retained)), ToolPayload::Terminal(text)) => {
            retained.synchronize(text);
            AgentToolPayload::Terminal(retained.clone())
        }
        (_, ToolPayload::Diff { path, old, new }) => AgentToolPayload::Diff {
            path: path.clone().into(),
            old: old.as_deref().map(AgentToolText::new),
            new: AgentToolText::new(new),
        },
        (_, ToolPayload::Text(text)) => AgentToolPayload::Text(AgentToolText::new(text)),
        (_, ToolPayload::Json(text)) => AgentToolPayload::Json(AgentToolText::new(text)),
        (_, ToolPayload::Terminal(text)) => AgentToolPayload::Terminal(AgentToolText::new(text)),
    };
    retained.insert(key, next.clone());
    next
}

fn agent_chrome_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .h(px(CHROME_BUTTON_HEIGHT))
        .px_2p5()
}

fn agent_chrome_icon_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .size(px(CHROME_BUTTON_HEIGHT))
        .p_0()
}

fn session_directory_label(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| cwd.display().to_string(), ToOwned::to_owned)
}

fn session_display_title(session: &AgentSessionSummary) -> String {
    session
        .title
        .clone()
        .unwrap_or_else(|| session_directory_label(&session.cwd))
}

fn format_session_timestamp(value: &str, today: NaiveDate) -> String {
    let Ok(timestamp) = DateTime::parse_from_rfc3339(value) else {
        return value.to_owned();
    };
    let local = timestamp.with_timezone(&Local);
    session_timestamp_label(local.date_naive(), local.hour(), local.minute(), today)
}

fn session_timestamp_label(date: NaiveDate, hour: u32, minute: u32, today: NaiveDate) -> String {
    let time = format!("{hour:02}:{minute:02}");
    if date == today {
        return format!("Today, {time}");
    }
    if today.pred_opt() == Some(date) {
        return format!("Yesterday, {time}");
    }
    let month = date.format("%b");
    if date.year() == today.year() {
        format!("{month} {}, {time}", date.day())
    } else {
        format!("{month} {}, {}, {time}", date.day(), date.year())
    }
}

fn ranked_session_indices(sessions: &[AgentSessionSummary], query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    let mut ranked = sessions
        .iter()
        .enumerate()
        .filter_map(|(index, session)| {
            if needle.is_empty() {
                return Some((3, index));
            }
            [
                session.title.as_deref().unwrap_or_default().to_lowercase(),
                session.cwd.to_string_lossy().to_lowercase(),
                session.session_id.to_lowercase(),
            ]
            .iter()
            .filter_map(|candidate| completion_score(candidate, &needle))
            .min()
            .map(|score| (score, index))
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(score, index)| (*score, *index));
    ranked.into_iter().map(|(_, index)| index).collect()
}

fn history_result_index_for_session(
    sessions: &[AgentSessionSummary],
    results: &[usize],
    session_id: &str,
) -> Option<usize> {
    results.iter().position(|session_index| {
        sessions
            .get(*session_index)
            .is_some_and(|session| session.session_id == session_id)
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompletionQuery {
    needle: String,
    replacement: Range<usize>,
}

const fn provider_icon(provider: AgentProvider) -> IconName {
    match provider {
        AgentProvider::Codex => IconName::Openai,
        AgentProvider::ClaudeCode => IconName::Claude,
    }
}

fn completion_query(value: &str, cursor: usize) -> Option<CompletionQuery> {
    if cursor > value.len() || !value.is_char_boundary(cursor) {
        return None;
    }
    let before_cursor = &value[..cursor];
    let line_start = before_cursor.rfind('\n').map_or(0, |index| index + 1);
    let sigil_index = before_cursor[line_start..].rfind('/')? + line_start;
    if sigil_index > line_start
        && !value[..sigil_index]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        return None;
    }
    let tail = &value[sigil_index + 1..cursor];
    if tail.chars().any(char::is_whitespace) {
        return None;
    }
    Some(CompletionQuery {
        needle: tail.to_owned(),
        replacement: sigil_index..cursor,
    })
}

fn bare_command_name(name: &str) -> &str {
    name.trim_start_matches('/')
}

fn completion_score(candidate: &str, needle: &str) -> Option<u8> {
    if needle.is_empty() {
        return Some(3);
    }
    if candidate == needle {
        return Some(0);
    }
    if candidate.starts_with(needle) {
        return Some(1);
    }
    if candidate.contains(needle) {
        return Some(2);
    }
    let mut characters = candidate.chars();
    needle
        .chars()
        .all(|needle| characters.by_ref().any(|candidate| candidate == needle))
        .then_some(3)
}

fn ranked_completions(
    commands: &[AgentCommand],
    query: &CompletionQuery,
) -> Vec<CommandCompletion> {
    let needle = query.needle.to_ascii_lowercase();
    let mut ranked = commands
        .iter()
        .filter_map(|command| {
            let searchable = bare_command_name(&command.name).to_ascii_lowercase();
            completion_score(&searchable, &needle).map(|score| {
                (
                    score,
                    searchable,
                    CommandCompletion {
                        command: command.clone(),
                        replacement: query.replacement.clone(),
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
    ranked
        .into_iter()
        .take(MAX_COMPLETION_RESULTS)
        .map(|(_, _, completion)| completion)
        .collect()
}

fn meaningful_command_description(description: &str) -> Option<&str> {
    let description = description.trim();
    (!description.is_empty()
        && description
            .chars()
            .any(|character| character != '.' && character != '…'))
    .then_some(description)
}

fn active_command_hint(value: &str, commands: &[AgentCommand]) -> Option<String> {
    let command = value.trim_start().strip_prefix('/')?;
    let (name, arguments) = command
        .split_once(char::is_whitespace)
        .map_or((command, ""), |(name, arguments)| (name, arguments));
    if !arguments.trim().is_empty() {
        return None;
    }
    commands
        .iter()
        .find(|command| bare_command_name(&command.name).eq_ignore_ascii_case(name))
        .and_then(|command| command.input_hint.as_deref())
        .map(|hint| format!("Argument · {hint}"))
}

#[cfg(test)]
mod completion_tests {
    #[cfg(not(target_os = "macos"))]
    use std::{cell::RefCell, rc::Rc};

    #[cfg(not(target_os = "macos"))]
    use gpui::{TestAppContext, VisualTestContext};
    #[cfg(not(target_os = "macos"))]
    use zz_daemon::DaemonError;
    #[cfg(not(target_os = "macos"))]
    use zz_ui::Root;

    use super::*;

    fn command(name: &str) -> AgentCommand {
        AgentCommand {
            name: name.to_owned(),
            description: format!("Run {name}"),
            input_hint: None,
        }
    }

    fn permission(request_id: u64) -> AgentPermissionRequest {
        use crate::agent::controller::AgentPermissionOption;

        AgentPermissionRequest {
            request_id,
            tool_call_id: format!("tool-{request_id}"),
            title: format!("Run command {request_id}"),
            options: vec![
                AgentPermissionOption {
                    id: format!("allow-{request_id}"),
                    name: "Allow once".to_owned(),
                    kind: AgentPermissionKind::AllowOnce,
                },
                AgentPermissionOption {
                    id: format!("reject-{request_id}"),
                    name: "Reject".to_owned(),
                    kind: AgentPermissionKind::RejectOnce,
                },
            ],
        }
    }

    #[test]
    fn composer_action_follows_the_live_turn_and_the_draft() {
        assert_eq!(composer_action(false, false), ComposerAction::Send);
        assert_eq!(composer_action(false, true), ComposerAction::Send);
        assert_eq!(composer_action(true, true), ComposerAction::Queue);
        assert_eq!(composer_action(true, false), ComposerAction::Stop);
    }

    #[test]
    fn permission_wizard_answers_the_focused_page_then_advances() {
        let requests = vec![permission(1), permission(2)];
        let mut wizard = PermissionWizard::default();
        wizard.sync(&requests);

        assert_eq!(wizard.page_label(&requests).as_deref(), Some("1/2"));
        assert_eq!(
            wizard.answer(&requests, Some(1)),
            PermissionStep::Answer {
                request_id: 1,
                option_id: Some("reject-1".to_owned()),
            }
        );
        assert_eq!(
            wizard.current(&requests).map(|request| request.request_id),
            Some(2)
        );
        assert_eq!(wizard.page_label(&requests), None);
    }

    #[test]
    fn permission_wizard_latches_an_answered_page_until_the_controller_drops_it() {
        let requests = vec![permission(1), permission(2)];
        let mut wizard = PermissionWizard::default();
        wizard.sync(&requests);
        wizard.answer(&requests, Some(0));

        assert_eq!(
            wizard.answer(&requests, Some(0)),
            PermissionStep::Answer {
                request_id: 2,
                option_id: Some("allow-2".to_owned()),
            }
        );
        assert_eq!(wizard.current(&requests), None);

        wizard.sync(&[]);

        assert!(wizard.answered.is_empty());
    }

    #[test]
    fn permission_wizard_keeps_its_page_when_another_request_resolves_elsewhere() {
        let requests = vec![permission(1), permission(2), permission(3)];
        let mut wizard = PermissionWizard::default();
        wizard.sync(&requests);
        wizard.answer(&requests, Some(0));
        wizard.highlight(1);

        let remaining = vec![permission(2), permission(3)];
        wizard.sync(&remaining);

        assert_eq!(
            wizard.current(&remaining).map(|request| request.request_id),
            Some(2)
        );
        assert_eq!(wizard.highlighted, 1);

        let auto_approved = vec![permission(3)];
        wizard.sync(&auto_approved);

        assert_eq!(
            wizard
                .current(&auto_approved)
                .map(|request| request.request_id),
            Some(3)
        );
        assert_eq!(wizard.highlighted, 0);
    }

    #[test]
    fn permission_digits_defer_to_a_composer_the_user_is_typing_into() {
        let requests = vec![permission(1)];
        let mut wizard = PermissionWizard::default();
        wizard.sync(&requests);

        assert_eq!(wizard.press_digit(&requests, 1, true), PermissionStep::Stay);
        assert_eq!(
            wizard.press_digit(&requests, 0, false),
            PermissionStep::Stay
        );
        assert_eq!(
            wizard.press_digit(&requests, 7, false),
            PermissionStep::Stay
        );
        assert_eq!(
            wizard.press_digit(&requests, 2, false),
            PermissionStep::Answer {
                request_id: 1,
                option_id: Some("reject-1".to_owned()),
            }
        );
    }

    #[test]
    fn permission_wizard_confirms_the_highlight_and_cancels_the_page() {
        let requests = vec![permission(1), permission(2)];
        let mut wizard = PermissionWizard::default();
        wizard.sync(&requests);
        wizard.highlight(1);

        assert_eq!(
            wizard.confirm(&requests),
            PermissionStep::Answer {
                request_id: 1,
                option_id: Some("reject-1".to_owned()),
            }
        );
        assert_eq!(
            wizard.cancel(&requests),
            PermissionStep::Answer {
                request_id: 2,
                option_id: None,
            }
        );
    }

    fn session(id: &str, cwd: &str, title: Option<&str>) -> AgentSessionSummary {
        AgentSessionSummary {
            session_id: id.to_owned(),
            cwd: PathBuf::from(cwd),
            additional_directories: Vec::new(),
            title: title.map(ToOwned::to_owned),
            updated_at: None,
        }
    }

    #[test]
    fn session_history_searches_title_path_and_opaque_id_without_losing_source_order() {
        let sessions = vec![
            session("sess-alpha", "/work/one", Some("Release planning")),
            session("sess-beta", "/work/payments", Some("Fix retries")),
            session("opaque-needle-id", "/work/three", None),
        ];

        assert_eq!(ranked_session_indices(&sessions, ""), vec![0, 1, 2]);
        assert_eq!(ranked_session_indices(&sessions, "release"), vec![0]);
        assert_eq!(ranked_session_indices(&sessions, "payments"), vec![1]);
        assert_eq!(ranked_session_indices(&sessions, "needle"), vec![2]);
    }

    #[test]
    fn session_history_retains_selection_by_identity_after_catalog_updates() {
        let sessions = vec![
            session("sess-gamma", "/work/three", Some("Gamma")),
            session("sess-alpha", "/work/one", Some("Alpha")),
            session("sess-beta", "/work/two", Some("Beta")),
        ];
        let results = ranked_session_indices(&sessions, "");

        assert_eq!(
            history_result_index_for_session(&sessions, &results, "sess-beta"),
            Some(2)
        );
        assert_eq!(
            history_result_index_for_session(&sessions, &results, "missing"),
            None
        );
    }

    #[test]
    fn session_directories_are_clipped_to_their_last_component() {
        assert_eq!(
            session_directory_label(Path::new("/work/payments")),
            "payments"
        );
        assert_eq!(
            session_directory_label(Path::new("/work/payments/")),
            "payments"
        );
        assert_eq!(session_directory_label(Path::new("/")), "/");
    }

    #[test]
    fn session_timestamps_use_compact_calendar_labels() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid fixture date");

        assert_eq!(session_timestamp_label(today, 18, 1, today), "Today, 18:01");
        assert_eq!(
            session_timestamp_label(
                NaiveDate::from_ymd_opt(2026, 7, 19).expect("valid fixture date"),
                9,
                43,
                today,
            ),
            "Yesterday, 09:43"
        );
        assert_eq!(
            session_timestamp_label(
                NaiveDate::from_ymd_opt(2026, 6, 4).expect("valid fixture date"),
                7,
                5,
                today,
            ),
            "Jun 4, 07:05"
        );
        assert_eq!(
            session_timestamp_label(
                NaiveDate::from_ymd_opt(2025, 12, 31).expect("valid fixture date"),
                23,
                59,
                today,
            ),
            "Dec 31, 2025, 23:59"
        );
    }

    #[test]
    fn available_commands_use_standard_slash_semantics() {
        assert_eq!(bare_command_name("/review"), "review");
        assert_eq!(bare_command_name("$brainstorm"), "$brainstorm");
        assert_eq!(
            completion_query("/rev", 4),
            Some(CompletionQuery {
                needle: "rev".to_owned(),
                replacement: 0..4,
            })
        );
        assert_eq!(
            completion_query("please /rev", 11),
            Some(CompletionQuery {
                needle: "rev".to_owned(),
                replacement: 7..11,
            })
        );
        assert!(completion_query("$rev", 4).is_none());
        assert!(completion_query("https://zed.dev", 15).is_none());
        assert!(completion_query("/review branch", 14).is_none());
    }

    #[test]
    fn completion_matching_supports_bare_command_names() {
        assert_eq!(completion_score("brainstorm", "brain"), Some(1));
        assert_eq!(completion_score("gh-address-comments", "gac"), Some(3));
        assert_eq!(completion_score("review", "xyz"), None);
    }

    #[test]
    fn completion_results_keep_every_available_command() {
        let commands = (0..16)
            .map(|index| command(&format!("command-{index:02}")))
            .collect::<Vec<_>>();
        let query = completion_query("/", 1).expect("command completion query");

        let completions = ranked_completions(&commands, &query);

        assert_eq!(completions.len(), commands.len());
    }

    #[test]
    fn command_hints_follow_standard_slash_semantics() {
        let command = AgentCommand {
            input_hint: Some("optional context".to_owned()),
            ..command("review")
        };

        assert_eq!(
            active_command_hint("/review ", std::slice::from_ref(&command)),
            Some("Argument · optional context".to_owned())
        );
        assert_eq!(active_command_hint("$review ", &[command]), None);
    }

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn non_append_timeline_rebuild_clears_retained_store(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx));
            let history_input = cx.new(|cx| InputState::new(window, cx));
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let controller =
                cx.new(|_| AgentController::new(crate::config::AgentConfig::default()));
            let timeline_store = cx.new(|_| AgentTimelineStore::default());
            timeline_store.update(cx, |store, cx| {
                assert!(!store.expanded(1, DisclosureKind::Tool, false));
                assert!(!store.expanded(1, DisclosureKind::Group, false));
                store.toggle_expanded(1, DisclosureKind::Tool, false, cx);
                store.toggle_expanded(1, DisclosureKind::Group, false, cx);
                store.markdown(1, MarkdownSlot::Body, "old session".into(), cx);
            });
            let timeline_entries = [
                AgentThreadEntry::User {
                    id: 1,
                    markdown: "old session".to_owned(),
                    images: Vec::new(),
                },
                AgentThreadEntry::Assistant {
                    id: 2,
                    markdown: "old response".to_owned(),
                },
            ];

            let timeline_scroll = ListState::new(2, ListAlignment::Top, px(1_200.0));
            AgentView {
                pane: PaneId(7),
                mux,
                controller,
                input,
                history_input,
                visible: false,
                pane_state: disconnected_pane_state(),
                timeline: TimelineModel::new(&timeline_entries, &[1, 2]),
                timeline_store,
                timeline_next_revision: 2,
                conversation_epoch: 0,
                stick: TimelineStick::new(&timeline_scroll, false),
                timeline_scroll,
                completion_scroll: UniformListScrollHandle::new(),
                submission_error: None,
                permission_wizard: PermissionWizard::default(),
                attachments: Vec::new(),
                completions: Arc::from([]),
                completion_selected: None,
                completion_dismissed: false,
                last_input: String::new(),
                last_cursor: 0,
                history_open: false,
                history_all_projects: false,
                history_results: Arc::from([]),
                history_selected: None,
                history_scroll: UniformListScrollHandle::new(),
                history_delete_confirmation: None,
                last_history_query: String::new(),
                directory_picker: None,
                window_corners: WindowCorners::NONE,
                _subscriptions: Vec::new(),
            }
        });
        let replacement = [AgentThreadEntry::User {
            id: 1,
            markdown: "new session".to_owned(),
            images: Vec::new(),
        }];
        let (changed, cleared) = cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                let (changed, store_update) =
                    view.synchronize_timeline(&replacement, &[3], &[], false);
                let cleared = view.update_timeline_store(store_update, cx);
                (changed, cleared)
            })
        });

        assert!(changed);
        assert!(cleared);
        let timeline_store = cx.update(|_, cx| view.read(cx).timeline_store.clone());
        assert!(!cx.update(|_, cx| {
            timeline_store.update(cx, |store, _| {
                store.expanded(1, DisclosureKind::Tool, false)
            })
        }));
        assert!(!cx.update(|_, cx| {
            timeline_store.update(cx, |store, _| {
                store.expanded(1, DisclosureKind::Group, false)
            })
        }));
    }

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn arrow_actions_navigate_completions_before_the_multiline_input(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let view_slot = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&view_slot);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .auto_grow(2, 8)
                    .default_value("/")
            });
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let controller =
                cx.new(|_| AgentController::new(crate::config::AgentConfig::default()));
            let history_input = cx.new(|cx| InputState::new(window, cx));
            let timeline_scroll = ListState::new(0, ListAlignment::Top, px(1_200.0));
            let view = cx.new(|view_cx| AgentView {
                pane: PaneId(7),
                mux,
                controller,
                input,
                history_input,
                visible: false,
                pane_state: disconnected_pane_state(),
                timeline: TimelineModel::default(),
                timeline_store: view_cx.new(|_| AgentTimelineStore::default()),
                timeline_next_revision: 0,
                conversation_epoch: 0,
                stick: TimelineStick::new(&timeline_scroll, false),
                timeline_scroll,
                completion_scroll: UniformListScrollHandle::new(),
                submission_error: None,
                permission_wizard: PermissionWizard::default(),
                attachments: Vec::new(),
                completions: vec![
                    CommandCompletion {
                        command: command("first"),
                        replacement: 0..1,
                    },
                    CommandCompletion {
                        command: command("second"),
                        replacement: 0..1,
                    },
                ]
                .into(),
                completion_selected: Some(0),
                completion_dismissed: false,
                last_input: "/".to_owned(),
                last_cursor: 1,
                history_open: false,
                history_all_projects: false,
                history_results: Arc::from([]),
                history_selected: None,
                history_scroll: UniformListScrollHandle::new(),
                history_delete_confirmation: None,
                last_history_query: String::new(),
                directory_picker: None,
                window_corners: WindowCorners::NONE,
                _subscriptions: Vec::new(),
            });
            view.read(cx).focus(cx).focus(window, cx);
            captured.replace(Some(view.clone()));
            Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        let view = view_slot.borrow().clone().expect("captured agent view");

        cx.simulate_keystrokes("down");
        assert_eq!(
            cx.update(|_, cx| view.read(cx).completion_selected),
            Some(1)
        );

        cx.simulate_keystrokes("up");
        assert_eq!(
            cx.update(|_, cx| view.read(cx).completion_selected),
            Some(0)
        );

        cx.simulate_keystrokes("tab");
        assert_eq!(
            cx.update(|_, cx| view.read(cx).input.read(cx).value().to_string()),
            "/first "
        );
        assert!(cx.update(|_, cx| view.read(cx).completions.is_empty()));
    }

    #[test]
    fn placeholder_only_command_descriptions_are_hidden() {
        assert_eq!(meaningful_command_description("..."), None);
        assert_eq!(meaningful_command_description(" … "), None);
        assert_eq!(
            meaningful_command_description(" Review the current diff "),
            Some("Review the current diff")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_geometry_tracks_the_rendered_stack() {
        assert_eq!(composer_total_height(), 146.0);
        assert_eq!(composer_tail_clearance(), 134.0);
        assert_eq!(
            composer_total_height() - composer_tail_clearance(),
            COMPOSER_OUTER_PADDING
        );
    }

    #[test]
    fn context_usage_is_bounded_and_handles_an_unknown_window() {
        assert_eq!(context_usage_fraction(0, 0), 0.0);
        assert_eq!(context_usage_fraction(25, 100), 0.25);
        assert_eq!(context_usage_fraction(125, 100), 1.0);
    }

    #[test]
    fn context_usage_tooltip_keeps_the_exact_counts() {
        assert_eq!(
            context_usage_tooltip(32_000, 128_000),
            "32000 of 128000 context tokens used (25%)"
        );
        assert_eq!(
            context_usage_tooltip(4, 0),
            "Context window usage unavailable"
        );
    }

    #[test]
    fn git_footer_pluralizes_the_file_count() {
        assert_eq!(git_file_count_label(0), "0 files");
        assert_eq!(git_file_count_label(1), "1 file");
        assert_eq!(git_file_count_label(12), "12 files");
    }

    #[test]
    fn busy_chrome_follows_the_connection_and_not_a_tool_row() {
        for connection in [
            AgentConnectionState::Starting,
            AgentConnectionState::Restoring,
            AgentConnectionState::Running,
            AgentConnectionState::Cancelling,
        ] {
            assert!(pane_is_busy(connection));
        }
        for connection in [
            AgentConnectionState::Ready,
            AgentConnectionState::Failed,
            AgentConnectionState::Disconnected,
        ] {
            assert!(!pane_is_busy(connection));
        }
    }

    #[test]
    fn directory_picker_requires_a_ready_local_pane_without_a_permission() {
        for connection in [
            AgentConnectionState::Starting,
            AgentConnectionState::Restoring,
            AgentConnectionState::Running,
            AgentConnectionState::Cancelling,
            AgentConnectionState::Failed,
            AgentConnectionState::Disconnected,
        ] {
            assert!(!directory_picker_enabled(connection, false, true));
        }
        assert!(directory_picker_enabled(
            AgentConnectionState::Ready,
            false,
            true
        ));
        assert!(!directory_picker_enabled(
            AgentConnectionState::Ready,
            true,
            true
        ));
        assert!(!directory_picker_enabled(
            AgentConnectionState::Ready,
            false,
            false
        ));
    }

    #[test]
    fn rendered_errors_are_bounded_and_strip_control_bytes() {
        let rendered = rendered_error(&format!("bad\0{}", "x".repeat(4096)));
        assert!(rendered.len() <= MAX_RENDERED_ERROR_BYTES);
        assert!(rendered.starts_with("bad�"));
        assert!(rendered.ends_with('…'));
        assert!(!rendered.contains('\0'));
    }

    fn thread_tool(id: u64, label: &str, status: AgentToolStatusModel) -> AgentThreadEntry {
        AgentThreadEntry::Tool {
            id,
            protocol_id: format!("tool-{id}"),
            kind: AgentToolKindModel::Edit,
            status,
            label: label.to_owned(),
            location: None,
            input: None,
            output: Vec::new(),
            default_expanded: false,
        }
    }

    fn thread_reasoning(id: u64, markdown: &str) -> AgentThreadEntry {
        AgentThreadEntry::Reasoning {
            id,
            label: "Reasoning".to_owned(),
            markdown: markdown.to_owned(),
            default_expanded: false,
        }
    }

    #[test]
    fn reasoning_runs_fold_into_one_row_without_absorbing_other_kinds() {
        let mut entries = vec![
            thread_reasoning(1, "first"),
            thread_reasoning(2, "second"),
            thread_tool(3, "Editing files", AgentToolStatusModel::Completed),
        ];
        let mut revisions = vec![1, 1, 1];
        let mut timeline = TimelineModel::new(&entries, &revisions);
        assert_eq!(timeline.entry_to_row, [0, 0, 1]);
        assert!(matches!(
            &timeline.rows[0],
            TimelineRow::Group { entries, .. } if entries.len() == 2
        ));
        assert!(matches!(&timeline.rows[1], TimelineRow::Single(_)));

        entries.push(thread_reasoning(4, "third"));
        entries.push(thread_reasoning(5, "fourth"));
        revisions.extend([1, 1]);
        let TimelineModelUpdate::Incremental { splice_start, .. } =
            timeline.synchronize(&entries, &revisions, None)
        else {
            panic!("appending reasoning after a tool should splice a row");
        };
        assert_eq!(splice_start, 2);
        assert_eq!(timeline.entry_to_row, [0, 0, 1, 2, 2]);

        entries[3] = thread_reasoning(4, "third, continued");
        revisions[3] = 2;
        let TimelineModelUpdate::Incremental {
            remeasure_rows,
            added_rows,
            ..
        } = timeline.synchronize(&entries, &revisions, None)
        else {
            panic!("a member revision should update its owning group row");
        };
        assert_eq!(remeasure_rows, [2]);
        assert_eq!(added_rows, 0);
    }

    #[test]
    fn timeline_model_updates_trailing_groups_and_maps_member_revisions() {
        let mut entries = vec![
            thread_tool(1, "Editing files", AgentToolStatusModel::Completed),
            thread_tool(2, "Editing files", AgentToolStatusModel::Running),
            thread_tool(3, "Editing files", AgentToolStatusModel::Completed),
        ];
        let mut revisions = vec![1, 1, 1];
        let mut timeline = TimelineModel::new(&entries, &revisions);
        assert_eq!(timeline.rows.len(), 1);
        assert_eq!(timeline.entry_to_row, [0, 0, 0]);

        entries.push(thread_tool(
            4,
            "Editing files",
            AgentToolStatusModel::Pending,
        ));
        revisions.push(1);
        let TimelineModelUpdate::Incremental {
            store_entries,
            remeasure_rows,
            splice_start,
            added_rows,
        } = timeline.synchronize(&entries, &revisions, None)
        else {
            panic!("tool append should update the trailing group");
        };
        assert!(store_entries.is_empty());
        assert_eq!(remeasure_rows, [0]);
        assert_eq!(splice_start, 1);
        assert_eq!(added_rows, 0);
        assert_eq!(timeline.rows.len(), 1);
        assert_eq!(timeline.entry_to_row, [0, 0, 0, 0]);
        assert!(matches!(
            &timeline.rows[0],
            TimelineRow::Group { entries, .. } if entries.len() == 4
        ));

        entries.push(thread_tool(
            5,
            "Running command",
            AgentToolStatusModel::Completed,
        ));
        revisions.push(1);
        let TimelineModelUpdate::Incremental {
            remeasure_rows,
            splice_start,
            added_rows,
            ..
        } = timeline.synchronize(&entries, &revisions, None)
        else {
            panic!("different-label tool append should update the trailing group");
        };
        assert_eq!(remeasure_rows, [0]);
        assert_eq!(splice_start, 1);
        assert_eq!(added_rows, 0);
        assert_eq!(timeline.rows.len(), 1);
        assert_eq!(timeline.entry_to_row, [0, 0, 0, 0, 0]);

        entries[1] = thread_tool(2, "Editing files", AgentToolStatusModel::Failed);
        revisions[1] = 2;
        let TimelineModelUpdate::Incremental {
            store_entries,
            remeasure_rows,
            added_rows,
            ..
        } = timeline.synchronize(&entries, &revisions, None)
        else {
            panic!("member revision should update its owning row");
        };
        assert_eq!(remeasure_rows, [0]);
        assert_eq!(added_rows, 0);
        assert_eq!(store_entries.len(), 1);
        assert_eq!(store_entries[0].id(), 2);
        assert!(matches!(
            timeline.rows[0].entry(2),
            Some(AgentEntry::Tool(AgentToolEntry {
                status: AgentToolStatus::Failed,
                ..
            }))
        ));

        entries[1] = thread_tool(2, "Renamed tool", AgentToolStatusModel::Failed);
        revisions[1] = 3;
        let TimelineModelUpdate::Incremental {
            store_entries,
            remeasure_rows,
            added_rows,
            ..
        } = timeline.synchronize(&entries, &revisions, None)
        else {
            panic!("tool label changes should preserve contiguous grouping");
        };
        assert_eq!(remeasure_rows, [0]);
        assert_eq!(added_rows, 0);
        assert_eq!(store_entries.len(), 1);
        assert_eq!(timeline.entry_to_row, [0, 0, 0, 0, 0]);
    }

    #[test]
    fn tool_entry_adapter_preserves_typed_payloads() {
        let entry = ui_entry(&AgentThreadEntry::Tool {
            id: 1,
            protocol_id: "tool".to_owned(),
            kind: AgentToolKindModel::Execute,
            status: AgentToolStatusModel::Completed,
            label: "run".to_owned(),
            location: Some("/workspace/src/lib.rs:9".to_owned()),
            input: Some(ToolPayload::Json("{\n  \"check\": true\n}".to_owned())),
            output: vec![
                ToolPayload::Diff {
                    path: "/workspace/src/lib.rs".to_owned(),
                    old: Some("old".to_owned()),
                    new: "new".to_owned(),
                },
                ToolPayload::Text("done".to_owned()),
                ToolPayload::Terminal("$ cargo check\nok\n[exit status: 0]".to_owned()),
            ],
            default_expanded: false,
        });
        let AgentEntry::Tool(AgentToolEntry {
            location,
            input,
            output,
            ..
        }) = entry
        else {
            panic!("tool output should be present");
        };

        assert_eq!(location.as_deref(), Some("/workspace/src/lib.rs:9"));
        assert!(matches!(input, Some(AgentToolPayload::Json(json)) if json.contains("check")));
        assert!(matches!(
            output.as_ref(),
            [
                AgentToolPayload::Diff { path, old: Some(old), new },
                AgentToolPayload::Text(text),
                AgentToolPayload::Terminal(terminal),
            ]
                if path == "/workspace/src/lib.rs"
                    && old == "old"
                    && new == "new"
                    && text == "done"
                    && terminal.contains("exit status")
        ));
    }

    #[test]
    fn tool_entry_adapter_releases_replaced_payload_slots() {
        let mut entry = AgentThreadEntry::Tool {
            id: 7,
            protocol_id: "tool".to_owned(),
            kind: AgentToolKindModel::Execute,
            status: AgentToolStatusModel::Running,
            label: "run".to_owned(),
            location: None,
            input: Some(ToolPayload::Text("input".to_owned())),
            output: vec![
                ToolPayload::Text("one".to_owned()),
                ToolPayload::Text("two".to_owned()),
                ToolPayload::Text("three".to_owned()),
            ],
            default_expanded: false,
        };
        let mut markdown = HashMap::new();
        let mut tool_payloads = HashMap::new();

        ui_entry_with_markdown(&entry, &mut markdown, &mut tool_payloads);
        assert_eq!(tool_payloads.len(), 4);

        let AgentThreadEntry::Tool { input, output, .. } = &mut entry else {
            unreachable!();
        };
        *input = None;
        output.truncate(1);
        ui_entry_with_markdown(&entry, &mut markdown, &mut tool_payloads);

        assert_eq!(tool_payloads.len(), 1);
        assert!(tool_payloads.contains_key(&(7, 1)));
    }

    #[test]
    fn flat_entry_adapter_covers_exactly_five_acp_shapes() {
        let entries = ui_entries(&[
            AgentThreadEntry::User {
                id: 1,
                markdown: "user".to_owned(),
                images: Vec::new(),
            },
            AgentThreadEntry::Assistant {
                id: 2,
                markdown: "assistant".to_owned(),
            },
            AgentThreadEntry::Reasoning {
                id: 3,
                label: "reasoning".to_owned(),
                markdown: "thought".to_owned(),
                default_expanded: false,
            },
            AgentThreadEntry::Tool {
                id: 4,
                protocol_id: "tool".to_owned(),
                kind: AgentToolKindModel::Execute,
                status: AgentToolStatusModel::Running,
                label: "run".to_owned(),
                location: None,
                input: None,
                output: Vec::new(),
                default_expanded: false,
            },
            AgentThreadEntry::Plan {
                id: 5,
                markdown: "- [ ] plan".to_owned(),
            },
        ]);

        assert_eq!(entries.len(), 5);
        assert!(matches!(entries[0], AgentEntry::User { .. }));
        assert!(matches!(entries[1], AgentEntry::Assistant { .. }));
        assert!(matches!(entries[2], AgentEntry::Reasoning { .. }));
        assert!(matches!(entries[3], AgentEntry::Tool(_)));
        assert!(matches!(entries[4], AgentEntry::Plan { .. }));
    }

    #[test]
    fn user_attachments_reach_the_timeline_without_a_second_copy() {
        let image = Arc::new(Image::from_bytes(gpui::ImageFormat::Png, vec![0x89, 0x50]));
        let entries = ui_entries(&[AgentThreadEntry::User {
            id: 1,
            markdown: String::new(),
            images: vec![Arc::clone(&image)],
        }]);

        let AgentEntry::User { images, .. } = &entries[0] else {
            panic!("the adapter should keep this a user entry");
        };
        assert_eq!(images.len(), 1);
        assert!(
            Arc::ptr_eq(&images[0], &image),
            "the bytes should travel by handle, not by clone, so gpui decodes them once"
        );
    }
}
