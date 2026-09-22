use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gpui::{
    AnyElement, App, Context, Corners, Entity, FocusHandle, Focusable, IntoElement, ListAlignment,
    ListState, MouseButton, Pixels, Render, Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;
use zz_client::agent_completion::{
    AgentCommand, CommandCompletion, active_command_hint, bare_command_name, completion_query,
    completion_score, meaningful_command_description, ranked_completions,
};
use zz_client::agent_config::{
    AgentCatalogCache, AgentSettingsApply, AgentSettingsSelection, config_option_models,
    rendered_error,
};
use zz_client::agent_transcript::{
    AgentPermissionKind, AgentThreadEntry, AgentToolKindModel, AgentToolStatusModel,
    AgentTranscript, ToolPayload,
};
use zz_protocol::{
    AgentConnectionPhase, AgentDescriptor, AgentImage, AgentSessionOpKind, PaneId, ProtocolMessage,
    agent_stream::AgentSessionSummary,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, ElementExt as _, IconName, Sizable as _,
    StyledExt as _,
    agent::{
        AgentEntry, AgentMarkdown, AgentTimeline, AgentTimelineStore, AgentToolEntry,
        AgentToolKind, AgentToolPayload, AgentToolStatus, AgentToolText, COMPOSER_ATTACHMENT,
        MarkdownSlot, TimelineRow, TimelineStick, agent_attachment_thumbnail,
        agent_jump_to_bottom_button, agent_pane_header,
        composer::{AgentComposer, COMPOSER_OUTER_PADDING},
        controls::{
            AgentControlChoice, AgentControlSelection, ComposerAction, agent_chrome_button,
            agent_config_picker, agent_directory_button, agent_model_picker, composer_action,
            composer_action_button, context_usage_meter, git_summary_footer,
        },
        fold_timeline_rows,
        presentation::{
            empty_state, error_card, permission_card, permission_option, welcome_state,
        },
        title::{agent_thread_title_editor, agent_title_is_editing},
    },
    button::{Button, ButtonVariants as _},
    input::{IndentInline, InputEvent, InputState, MoveDown, MoveUp},
    pane::{PaneDrag, pane_drag_button, pane_header_icon_button},
    scroll::{ScrollableElement as _, Scrollbar},
};

use crate::connection::Connection;

pub(super) struct AgentPane {
    pane: PaneId,
    descriptor: AgentDescriptor,
    connection: Entity<Connection>,
    connected: bool,
    input: Entity<InputState>,
    commands: Arc<[AgentCommand]>,
    completions: Arc<[CommandCompletion]>,
    completion_selected: Option<usize>,
    completion_dismissed: bool,
    completion_scroll: gpui::UniformListScrollHandle,
    last_input: String,
    last_cursor: usize,
    transcript: Transcript,
    timeline: Entity<AgentTimelineStore>,
    rows: Arc<Vec<TimelineRow>>,
    scroll: ListState,
    stick: TimelineStick,
    last_sequence: u64,
    last_reclaim_id: u64,
    attachments: Vec<AgentImage>,
    choosing_images: bool,
    draft_error: Option<String>,
    settings_busy: bool,
    settings_apply: Option<AgentSettingsApply>,
    catalogs: AgentCatalogCache,
    settings_apply_generation: u64,
    control_sequence: u64,
    lifecycle_pending: bool,
    lifecycle_generation: u64,
    permission_request_id: Option<u64>,
    permission_selected: usize,
    permission_answered: HashSet<u64>,
    usage: Option<(u64, u64)>,
    history_open: bool,
    history_compact: bool,
    project_directory: Option<PathBuf>,
    project_hovered: Option<PathBuf>,
    project_focus: bool,
    project_scroll: gpui::UniformListScrollHandle,
    history_selected: usize,
    history_delete: Option<String>,
    picker_scroll: gpui::UniformListScrollHandle,
    history_supported: bool,
    history_input: Entity<InputState>,
    history_loading: bool,
    history_error: Option<String>,
    sessions: Vec<AgentSessionSummary>,
    next_cursor: Option<String>,
    transcript_dirty: bool,
    header_drag_handler: Option<Rc<dyn Fn(&PaneDrag, &mut Window, &mut App)>>,
    header_touch_drag_handler: Option<Rc<dyn Fn(&gpui::TouchDragEvent, &mut Window, &mut App)>>,
    corner_radii: Corners<Pixels>,
    _subscriptions: Vec<Subscription>,
}

impl AgentPane {
    pub(super) fn new(
        pane: PaneId,
        descriptor: AgentDescriptor,
        connection: Entity<Connection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Ask the agent…")
                .auto_grow(2, 8)
                .submit_on_enter(true)
                .context_menu(true)
        });
        let history_input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Search directories and sessions…"));
        let history_changes = cx.subscribe(&history_input, |this: &mut Self, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.history_selected = 0;
                this.reconcile_project(cx);
                cx.notify();
            }
        });
        let observation = cx.observe(&connection, |this, connection, cx| {
            let connected = connection.read(cx).connected;
            if this.connected != connected {
                this.connected = connected;
                this.settings_busy = false;
                this.settings_apply = None;
                this.catalogs = AgentCatalogCache::default();
                this.lifecycle_pending = false;
                this.permission_answered.clear();
                this.history_loading = false;
                cx.notify();
            }
            if this.settings_apply.is_some() {
                this.synchronize_controls(cx);
            }
        });
        let events = cx.subscribe(
            &connection,
            move |this, _, event: &zz_client::CoreEvent, cx| {
                if matches!(
                    event,
                    zz_client::CoreEvent::HelloReceived | zz_client::CoreEvent::Attached { .. }
                ) {
                    this.transcript = Transcript::default();
                    this.settings_busy = false;
                    this.settings_apply = None;
                    this.control_sequence = 0;
                    this.lifecycle_pending = false;
                    this.usage = None;
                    this.last_sequence = 0;
                    this.rows = Arc::new(Vec::new());
                    this.scroll.reset(0);
                    this.stick.engage_now(&this.scroll, cx.reduce_motion());
                    this.timeline.update(cx, |timeline, cx| {
                        timeline.clear(cx);
                    });
                    cx.notify();
                } else if let zz_client::CoreEvent::AgentSessions {
                    pane: changed,
                    result,
                    ..
                } = event
                {
                    if *changed == pane {
                        if let Ok(catalog) = serde_json::from_str::<
                            zz_protocol::agent_stream::AgentCatalogResult,
                        >(result)
                        {
                            this.catalogs.receive(catalog);
                        } else {
                            this.receive_sessions(result, cx);
                        }
                        cx.notify();
                    }
                } else if matches!(event,
                zz_client::CoreEvent::AgentUpdates { pane: changed, .. }
                | zz_client::CoreEvent::AgentStateChanged { pane: changed, .. } if *changed == pane)
                {
                    if matches!(event, zz_client::CoreEvent::AgentStateChanged { .. }) {
                        this.lifecycle_pending = false;
                    }
                    cx.notify();
                }
            },
        );
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } => this.enter(window, cx),
                InputEvent::Change => {
                    this.draft_error = None;
                    cx.notify();
                }
                InputEvent::PasteImages(images) => this.attach_images(
                    images
                        .iter()
                        .map(|image| AgentImage {
                            format: image.format.mime_type().to_owned(),
                            data: image.bytes.clone(),
                        })
                        .collect(),
                    cx,
                ),
                _ => {}
            });
        let input_observer = cx.observe(&input, |this, _, cx| {
            if this.synchronize_completions(cx) {
                cx.notify();
            }
        });
        let timeline = cx.new(|_| AgentTimelineStore::default());
        let timeline_observer = cx.observe(&timeline, |_, _, cx| cx.notify());
        let scroll = ListState::new(0, ListAlignment::Top, px(1_200.0));
        let stick = TimelineStick::new(&scroll, cx.reduce_motion());
        let scroll_view = cx.weak_entity();
        scroll.set_scroll_handler(move |_, _, cx| {
            let view = scroll_view.clone();
            cx.defer(move |cx| {
                let _ = view.update(cx, |this: &mut Self, cx| {
                    if this.stick.on_user_scroll(&this.scroll, cx.reduce_motion()) {
                        cx.notify();
                    }
                });
            });
        });
        Self {
            pane,
            descriptor,
            connection,
            connected: false,
            input,
            commands: Arc::from([]),
            completions: Arc::from([]),
            completion_selected: None,
            completion_dismissed: false,
            completion_scroll: gpui::UniformListScrollHandle::new(),
            last_input: String::new(),
            last_cursor: 0,
            transcript: Transcript::default(),
            timeline,
            rows: Arc::new(Vec::new()),
            scroll,
            stick,
            last_sequence: 0,
            last_reclaim_id: 0,
            attachments: Vec::new(),
            choosing_images: false,
            draft_error: None,
            settings_busy: false,
            settings_apply: None,
            catalogs: AgentCatalogCache::default(),
            settings_apply_generation: 0,
            control_sequence: 0,
            lifecycle_pending: false,
            lifecycle_generation: 0,
            permission_request_id: None,
            permission_selected: 0,
            permission_answered: HashSet::new(),
            usage: None,
            history_open: false,
            history_compact: true,
            project_directory: None,
            project_hovered: None,
            project_focus: false,
            project_scroll: gpui::UniformListScrollHandle::new(),
            history_selected: 0,
            history_delete: None,
            picker_scroll: gpui::UniformListScrollHandle::new(),
            history_supported: false,
            history_input,
            history_loading: false,
            history_error: None,
            sessions: Vec::new(),
            next_cursor: None,
            transcript_dirty: false,
            header_drag_handler: None,
            header_touch_drag_handler: None,
            corner_radii: Corners::default(),
            _subscriptions: vec![
                observation,
                events,
                subscription,
                history_changes,
                timeline_observer,
                input_observer,
            ],
        }
    }

    pub(super) fn set_header_drag_handler(
        &mut self,
        handler: impl Fn(&PaneDrag, &mut Window, &mut App) + 'static,
    ) {
        self.header_drag_handler = Some(Rc::new(handler));
    }

    pub(super) fn set_header_touch_drag_handler(
        &mut self,
        handler: impl Fn(&gpui::TouchDragEvent, &mut Window, &mut App) + 'static,
    ) {
        self.header_touch_drag_handler = Some(Rc::new(handler));
    }

    fn synchronize_timeline_context(&self, cx: &mut Context<Self>) {
        let running = self
            .connection
            .read(cx)
            .core
            .agent_state(self.pane)
            .is_some_and(|state| {
                matches!(
                    state.phase,
                    AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
                )
            });
        let streaming = running
            .then(|| self.transcript.entries.last().map(AgentEntry::id))
            .flatten();
        let cwd = self.descriptor.cwd.clone();
        self.timeline.update(cx, |timeline, cx| {
            timeline.set_streaming(streaming, cx);
            timeline.set_cwd(cwd, cx);
        });
    }

    pub(super) fn set_corner_radii(&mut self, radii: Corners<Pixels>, cx: &mut Context<Self>) {
        if self.corner_radii != radii {
            self.corner_radii = radii;
            cx.notify();
        }
    }

    pub(super) fn update_descriptor(
        &mut self,
        descriptor: &AgentDescriptor,
        cx: &mut Context<Self>,
    ) {
        if self.descriptor != *descriptor {
            if self.descriptor.cwd != descriptor.cwd {
                self.settings_apply = None;
            }
            if self.descriptor.provider != descriptor.provider {
                self.sessions.clear();
                self.next_cursor = None;
                self.history_loading = false;
                self.history_error = None;
            }
            self.descriptor = descriptor.clone();
            self.drive_settings_apply(cx);
            cx.notify();
        }
    }

    fn synchronize_completions(&mut self, cx: &mut Context<Self>) -> bool {
        let input = self.input.read(cx);
        let value = input.value().to_string();
        let cursor = input.cursor();
        let commands = self.connection.read(cx).agent_commands(self.pane);
        let input_changed = value != self.last_input || cursor != self.last_cursor;
        if !input_changed && self.commands == commands {
            return false;
        }
        if input_changed {
            self.last_input = value;
            self.last_cursor = cursor;
            self.completion_dismissed = false;
        }
        self.commands = commands;
        self.completions = if self.completion_dismissed || self.history_open {
            Arc::from([])
        } else {
            completion_query(&self.last_input, self.last_cursor).map_or_else(
                || Arc::from([]),
                |query| ranked_completions(&self.commands, &query).into(),
            )
        };
        self.completion_selected = (!self.completions.is_empty()).then_some(
            self.completion_selected
                .unwrap_or_default()
                .min(self.completions.len().saturating_sub(1)),
        );
        if let Some(selected) = self.completion_selected {
            self.completion_scroll
                .scroll_to_item(selected, gpui::ScrollStrategy::Nearest);
        }
        true
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.synchronize_completions(cx);
        if let Some(index) = self.completion_selected {
            self.accept_completion(index, window, cx);
        } else {
            self.submit(window, cx);
        }
    }

    fn accept_completion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(completion) = self.completions.get(index).cloned() else {
            return;
        };
        let insertion = completion.insertion();
        self.completion_dismissed = true;
        self.input.update(cx, |input, cx| {
            input.set_selected_range(completion.replacement, cx);
            input.replace(insertion, window, cx);
        });
        self.completions = Arc::from([]);
        self.completion_selected = None;
        self.input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn complete(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        if agent_title_is_editing(window) {
            return;
        }
        if let Some(index) = self.completion_selected {
            self.accept_completion(index, window, cx);
            cx.stop_propagation();
        }
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
            .scroll_to_item(selected, gpui::ScrollStrategy::Nearest);
        cx.notify();
    }

    fn move_completion_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        if agent_title_is_editing(window) {
            return;
        }
        if !self.completions.is_empty() {
            self.navigate_completion(-1, cx);
            cx.stop_propagation();
        }
    }

    fn move_completion_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        if agent_title_is_editing(window) {
            return;
        }
        if !self.completions.is_empty() {
            self.navigate_completion(1, cx);
            cx.stop_propagation();
        }
    }

    fn render_completions(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if self.completions.is_empty() {
            return None;
        }
        let completions = self.completions.clone();
        let selected = self.completion_selected;
        let view = cx.entity();
        let pane = self.pane;
        let rows = gpui::uniform_list(
            ("web-agent-completion-rows", pane.0),
            completions.len(),
            move |range, _, cx| {
                range
                    .filter_map(|index| {
                        let completion = completions.get(index)?;
                        let command = &completion.command;
                        let hover = view.clone();
                        let click = view.clone();
                        Some(
                            zz_ui::agent::slash::suggestion_row(
                                format!("web-agent-completion-{}-{index}", pane.0),
                                bare_command_name(&command.name),
                                meaningful_command_description(&command.description)
                                    .map(|description| description.to_owned().into()),
                                selected == Some(index),
                                cx,
                            )
                            .on_hover(move |hovered, _, cx| {
                                if *hovered {
                                    hover.update(cx, |this, cx| {
                                        if this.completion_selected != Some(index) {
                                            this.completion_selected = Some(index);
                                            cx.notify();
                                        }
                                    });
                                }
                            })
                            .on_click(move |_, window, cx| {
                                click.update(cx, |this, cx| {
                                    this.accept_completion(index, window, cx);
                                });
                                cx.stop_propagation();
                            }),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .w_full()
        .h(zz_ui::agent::slash::suggestion_list_height(
            self.completions.len(),
        ))
        .track_scroll(&self.completion_scroll);
        Some(zz_ui::agent::slash::suggestion_list(rows, cx).into_any_element())
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let connection = self.connection.read(cx);
        let allowed = self.settings_apply.is_none()
            && connection.connected
            && !connection.core.attached_read_only()
            && connection.core.agent_state(self.pane).is_some_and(|state| {
                matches!(
                    state.phase,
                    AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
                ) || (state.phase == AgentConnectionPhase::Ready
                    && state.pending_permission.is_none())
            });
        let text = self.input.read(cx).value().to_string();
        if !allowed || (text.trim().is_empty() && self.attachments.is_empty()) {
            return;
        }
        if let Err(error) = crate::attachments::validate_images(&text, &self.attachments) {
            self.draft_error = Some(error);
            cx.notify();
            return;
        }
        self.connection.update(cx, |connection, cx| {
            connection.send(
                ProtocolMessage::AgentPrompt {
                    pane: self.pane,
                    text: text.clone(),
                    images: self.attachments.clone(),
                },
                cx,
            );
        });
        if !self.connection.read(cx).connected {
            return;
        }
        let queueing = self
            .connection
            .read(cx)
            .core
            .agent_state(self.pane)
            .is_some_and(|state| {
                matches!(
                    state.phase,
                    AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
                )
            });
        if !queueing {
            self.transcript.local_prompt(&text, &self.attachments);
            self.transcript_dirty = true;
        }
        self.attachments.clear();
        self.draft_error = None;
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.stick.engage(&self.scroll, cx.reduce_motion());
        cx.notify();
    }

    fn request_catalog(&mut self, provider: zz_protocol::AgentProvider, cx: &mut Context<Self>) {
        let cwd = self.descriptor.cwd.clone().unwrap_or_default();
        let Some(request_id) = self.catalogs.request(provider, cwd) else {
            return;
        };
        self.connection.update(cx, |connection, cx| {
            connection.command(
                "agent-catalog",
                vec![
                    "-t".into(),
                    self.pane.to_string(),
                    provider.as_str().into(),
                    request_id.to_string(),
                ],
                cx,
            );
        });
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(45))
                .await;
            let _ = view.update(cx, |view, cx| {
                if view.catalogs.timeout(request_id) {
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn apply_settings(&mut self, selection: AgentSettingsSelection, cx: &mut Context<Self>) {
        let connection = self.connection.read(cx);
        if !connection.connected
            || connection.core.attached_read_only()
            || self.settings_busy
            || self.lifecycle_pending
            || self.settings_apply.is_some()
            || connection.core.agent_state(self.pane).is_some_and(|state| {
                matches!(
                    state.phase,
                    AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
                )
            })
        {
            return;
        }
        let provider = selection.provider;
        self.settings_apply = Some(AgentSettingsApply::new(selection));
        self.settings_apply_generation = self.settings_apply_generation.wrapping_add(1);
        let generation = self.settings_apply_generation;
        if provider != self.descriptor.provider {
            self.lifecycle_command(Some(provider), cx);
        }
        self.drive_settings_apply(cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(30))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.settings_apply_generation == generation
                    && this.settings_apply.take().is_some()
                {
                    this.draft_error = Some("Timed out applying agent settings.".into());
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn synchronize_controls(&mut self, cx: &mut Context<Self>) {
        let journal = self.connection.read(cx).agent_events.get(&self.pane);
        if journal
            .and_then(|events| events.last())
            .is_some_and(|(sequence, _)| *sequence < self.control_sequence)
        {
            self.control_sequence = 0;
        }
        let updates: Vec<_> = journal
            .into_iter()
            .flatten()
            .filter(|(sequence, _)| *sequence > self.control_sequence)
            .filter_map(|(sequence, bytes)| {
                serde_json::from_slice::<Value>(bytes)
                    .ok()
                    .map(|value| (*sequence, value))
            })
            .collect();
        for (sequence, update) in updates {
            self.apply_control_update(&update);
            self.control_sequence = sequence;
        }
        self.drive_settings_apply(cx);
    }

    fn drive_settings_apply(&mut self, cx: &mut Context<Self>) {
        let Some(apply) = &mut self.settings_apply else {
            return;
        };
        let connection = self.connection.read(cx);
        if !connection.connected || connection.core.attached_read_only() {
            self.settings_apply = None;
            return;
        }
        let Some(state) = connection.core.agent_state(self.pane) else {
            return;
        };
        if matches!(state.phase, AgentConnectionPhase::Failed { .. })
            && self.descriptor.provider == apply.selection.provider
        {
            self.settings_apply = None;
            return;
        }
        if self.settings_busy
            || self.lifecycle_pending
            || self.descriptor.provider != apply.selection.provider
            || state.phase != AgentConnectionPhase::Ready
            || state.pending_permission.is_some()
        {
            return;
        }
        let options = serde_json::from_str(&state.config_options)
            .map(config_option_models)
            .unwrap_or_default();
        match apply.next_setting(&options) {
            Ok(Some((id, value))) => {
                if let Some(option) = config_controls(&state.config_options, &state.modes)
                    .into_iter()
                    .find(|option| option.id == id)
                {
                    self.set_config(&option, &value, cx);
                    if !self.settings_busy {
                        self.settings_apply = None;
                    }
                }
            }
            Ok(None) => self.settings_apply = None,
            Err(error) => {
                self.settings_apply = None;
                self.draft_error = Some(error);
            }
        }
    }

    fn apply_control_update(&mut self, update: &Value) {
        if let Some((id, value)) = self
            .settings_apply
            .as_ref()
            .and_then(AgentSettingsApply::awaiting_setting)
        {
            match update.get("item").and_then(Value::as_str) {
                Some("configOptionsChanged")
                    if update.get("option_id").and_then(Value::as_str) != Some(id)
                        || update.get("value").and_then(Value::as_str) != Some(value) =>
                {
                    return;
                }
                Some("settingFailed")
                    if update.get("option_id").and_then(Value::as_str) != Some(id) =>
                {
                    return;
                }
                Some("modeChanged") => return,
                _ => {}
            }
        }
        match update.get("item").and_then(Value::as_str) {
            Some("configOptionsChanged" | "modeChanged") => self.settings_busy = false,
            Some("settingFailed") => {
                self.settings_apply = None;
                self.settings_busy = false;
                self.draft_error = update
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            Some("ready" | "sessionReady") => self.lifecycle_pending = false,
            Some("sessionReset" | "sessionSwitched") => {
                self.settings_busy = false;
                self.lifecycle_pending = false;
                self.usage = None;
                for update in update
                    .get("replay")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    self.apply_control_update(update);
                }
            }
            _ => {}
        }
        if let Some(usage) = usage_from_update(update) {
            self.usage = Some(usage);
        }
    }

    fn set_config(&mut self, option: &ConfigControl, value: &str, cx: &mut Context<Self>) {
        let connection = self.connection.read(cx);
        if self.settings_busy
            || !connection.connected
            || connection.core.attached_read_only()
            || !connection.core.agent_state(self.pane).is_some_and(|state| {
                state.phase == AgentConnectionPhase::Ready && state.pending_permission.is_none()
            })
        {
            return;
        }
        self.settings_busy = true;
        self.draft_error = None;
        self.connection.update(cx, |connection, cx| {
            connection.send(
                if option.legacy_mode {
                    ProtocolMessage::AgentSetMode {
                        pane: self.pane,
                        mode_id: value.to_owned(),
                    }
                } else {
                    ProtocolMessage::AgentSetConfigOption {
                        pane: self.pane,
                        option_id: option.id.clone(),
                        value: value.to_owned(),
                    }
                },
                cx,
            );
        });
        if !self.connection.read(cx).connected {
            self.settings_busy = false;
            self.draft_error = Some(self.connection.read(cx).status.clone());
        }
        cx.notify();
    }

    fn render_config_controls(
        &self,
        state: &zz_protocol::AgentPaneWire,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let connection = self.connection.read(cx);
        let enabled = connection.connected
            && !connection.core.attached_read_only()
            && state.phase == AgentConnectionPhase::Ready
            && state.pending_permission.is_none()
            && !self.settings_busy;
        config_controls(&state.config_options, &state.modes)
            .into_iter()
            .filter(|option| option.category == "mode")
            .take(1)
            .map(|option| {
                let view = cx.entity();
                let selected = option.clone();
                agent_config_picker(
                    format!("agent-config-picker-{}-{}", self.pane.0, option.id),
                    IconName::Check,
                    &option.current_value,
                    &option.name,
                    option.description.as_deref().unwrap_or(&option.name),
                    option.choices,
                    enabled,
                    move |value, _, cx| {
                        view.update(cx, |view, cx| view.set_config(&selected, value, cx));
                    },
                )
            })
            .collect()
    }

    fn synchronize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_supported = self.connection.read(cx).agent_history_supported(self.pane);
        let events = self.connection.read(cx).agent_events.get(&self.pane);
        let newest = events
            .and_then(|events| events.last())
            .map_or(0, |(sequence, _)| *sequence);
        if newest == self.last_sequence && !self.transcript_dirty {
            self.synchronize_timeline_context(cx);
            self.drive_settings_apply(cx);
            return;
        }
        let skipped = events
            .and_then(|events| events.first())
            .is_some_and(|(first, _)| {
                self.last_sequence > 0 && *first > self.last_sequence.saturating_add(1)
            });
        if newest < self.last_sequence || skipped {
            self.transcript = Transcript::default();
            self.timeline.update(cx, |timeline, cx| {
                timeline.clear(cx);
            });
            self.last_sequence = 0;
            self.control_sequence = 0;
        }
        let old_first = self.transcript.entries.first().map(AgentEntry::id);
        let old_count = self.rows.len();
        let journal = self.connection.read(cx).agent_events.get(&self.pane);
        let updates: Vec<_> = journal
            .map(|journal| {
                &journal[journal.partition_point(|(sequence, _)| *sequence <= self.last_sequence)..]
            })
            .into_iter()
            .flatten()
            .filter_map(|(sequence, bytes)| {
                serde_json::from_slice::<Value>(bytes)
                    .ok()
                    .map(|value| (*sequence, value))
            })
            .collect();
        let conversation_changed = updates.iter().any(|(_, update)| {
            matches!(
                update.get("item").and_then(Value::as_str),
                Some("sessionReset" | "sessionSwitched")
            )
        });
        if conversation_changed {
            self.permission_answered.clear();
        }
        for (sequence, update) in updates {
            self.restore_prompts(&update, window, cx);
            if sequence > self.control_sequence {
                self.apply_control_update(&update);
                self.control_sequence = sequence;
            }
            if matches!(
                update.get("item").and_then(Value::as_str),
                Some("sessionReset" | "sessionSwitched")
            ) {
                self.timeline.update(cx, |timeline, cx| {
                    timeline.clear(cx);
                });
            }
            self.transcript.apply(sequence, &update);
        }
        self.transcript.synchronize();
        self.last_sequence = newest;
        self.transcript_dirty = false;
        self.synchronize_timeline_context(cx);
        self.rows = fold_timeline_rows(&self.transcript.entries).rows;
        if conversation_changed
            || self.transcript.entries.first().map(AgentEntry::id) != old_first
            || self.rows.len() < old_count
        {
            self.scroll.reset(self.rows.len());
            self.stick.engage_now(&self.scroll, cx.reduce_motion());
        } else {
            self.scroll.remeasure_items(0..old_count);
            if self.rows.len() > old_count {
                self.scroll
                    .splice(old_count..old_count, self.rows.len() - old_count);
            }
        }
        self.stick.wake();
        self.timeline.update(cx, |timeline, cx| {
            for entry in &self.transcript.entries {
                match entry {
                    AgentEntry::User { id, markdown, .. }
                    | AgentEntry::Assistant { id, markdown }
                    | AgentEntry::Reasoning { id, markdown, .. }
                    | AgentEntry::Plan { id, markdown } => {
                        timeline.synchronize_markdown(
                            *id,
                            MarkdownSlot::Body,
                            markdown.clone(),
                            cx,
                        );
                    }
                    AgentEntry::Tool(tool) => timeline.synchronize_tool_content(
                        tool.id,
                        tool.location.clone(),
                        tool.input.clone(),
                        tool.output.clone(),
                        cx,
                    ),
                }
            }
        });
        self.drive_settings_apply(cx);
    }

    fn restore_prompts(&mut self, update: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let item = update.get("item").and_then(Value::as_str);
        if !matches!(item, Some("promptsReclaimed" | "promptsRestored")) {
            return;
        }
        let reclaim_id = update
            .get("reclaim_id")
            .or_else(|| update.get("reclaimId"))
            .and_then(Value::as_u64);
        if reclaim_id.is_some_and(|id| id <= self.last_reclaim_id) {
            return;
        }
        let client = self.connection.read(cx).client_instance_id();
        let prompts = restored_prompts(update, client);
        if prompts.is_empty() {
            return;
        }
        let mut draft = self.input.read(cx).value().to_string();
        for (text, images) in prompts {
            if !text.is_empty() {
                if !draft.is_empty() {
                    draft.push('\n');
                }
                draft.push_str(&text);
            }
            self.attachments.extend(images);
        }
        self.input
            .update(cx, |input, cx| input.set_value(draft, window, cx));
        if let Some(reclaim_id) = reclaim_id {
            self.last_reclaim_id = reclaim_id;
            self.connection.update(cx, |connection, cx| {
                connection.send(
                    ProtocolMessage::AgentAcknowledgePromptRestore {
                        pane: self.pane,
                        reclaim_id,
                    },
                    cx,
                );
            });
        }
        cx.notify();
    }

    fn choose_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.choosing_images {
            return;
        }
        self.choosing_images = true;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let images = crate::attachments::choose_images().await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.choosing_images = false;
                match images {
                    Ok(images) => this.attach_images(images, cx),
                    Err(error) => {
                        this.draft_error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn attach_images(&mut self, images: Vec<AgentImage>, cx: &mut Context<Self>) {
        let mut combined = self.attachments.clone();
        combined.extend(images);
        match crate::attachments::validate_images(&self.input.read(cx).value(), &combined) {
            Ok(()) => {
                self.attachments = combined;
                self.draft_error = None;
            }
            Err(error) => self.draft_error = Some(error),
        }
        cx.notify();
    }

    fn list_sessions(&mut self, replace: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.history_supported || self.history_loading {
            return;
        }
        self.history_delete = None;
        self.history_loading = true;
        self.history_error = None;
        let cursor = if replace {
            None
        } else {
            self.next_cursor.clone()
        };
        self.connection.update(cx, |connection, cx| {
            connection.send(
                ProtocolMessage::AgentSessionOp {
                    pane: self.pane,
                    op: AgentSessionOpKind::List {
                        cwd: None,
                        cursor,
                        replace,
                    },
                },
                cx,
            );
        });
        cx.notify();
    }

    fn receive_sessions(&mut self, result: &str, cx: &App) {
        let selected = self
            .history_results(cx)
            .get(self.history_selected)
            .and_then(|index| self.sessions.get(*index))
            .map(|session| session.session_id.clone());
        self.history_loading = false;
        let result = serde_json::from_str::<Value>(result).unwrap_or_default();
        if result.get("item").and_then(Value::as_str) == Some("sessionDeleted") {
            self.history_delete = None;
            if let Some(id) = result.get("session_id").and_then(Value::as_str) {
                self.sessions.retain(|session| session.session_id != id);
            }
            self.history_selected = self
                .history_selected
                .min(self.history_results(cx).len().saturating_sub(1));
            return;
        }
        if result.get("item").and_then(Value::as_str) != Some("sessionsListed") {
            self.history_delete = None;
            self.history_error = Some(
                result
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Could not load conversations")
                    .into(),
            );
            return;
        }
        if result
            .get("replace")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            self.sessions.clear();
        }
        for session in result
            .get("sessions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Ok(session) = serde_json::from_value::<AgentSessionSummary>(session.clone())
                && !self
                    .sessions
                    .iter()
                    .any(|existing| existing.session_id == session.session_id)
            {
                self.sessions.push(session);
            }
        }
        self.reconcile_project(cx);
        self.history_selected = selected
            .and_then(|id| {
                self.history_results(cx)
                    .iter()
                    .position(|index| self.sessions[*index].session_id == id)
            })
            .unwrap_or_default();
        self.next_cursor = result
            .get("next_cursor")
            .or_else(|| result.get("nextCursor"))
            .and_then(Value::as_str)
            .map(str::to_owned);
    }

    fn history_results(&self, cx: &App) -> Vec<usize> {
        ranked_session_indices(&self.sessions, &self.history_input.read(cx).value())
            .into_iter()
            .filter(|index| Some(&self.sessions[*index].cwd) == self.project_directory.as_ref())
            .collect()
    }

    fn session_is_open(&self, id: &str, cx: &App) -> bool {
        self.connection.read(cx).core.snapshot().sessions.iter().flat_map(|session| &session.windows).flat_map(|window| window.panes.values()).any(|pane| {
            matches!(&pane.kind, zz_protocol::PaneKindSnapshot::Agent(descriptor) if descriptor.provider == self.descriptor.provider && descriptor.session_id.as_deref() == Some(id))
        })
    }

    fn open_history_result(&mut self, result: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_change_session(cx)
            || !self
                .connection
                .read(cx)
                .agent_session_load_supported(self.pane)
        {
            return;
        }
        let Some(session) = self
            .history_results(cx)
            .get(result)
            .and_then(|index| self.sessions.get(*index))
            .cloned()
        else {
            return;
        };
        if self.session_is_open(&session.session_id, cx) {
            self.history_error = Some("This conversation is already open.".into());
            cx.notify();
            return;
        }
        self.connection.update(cx, |connection, cx| {
            connection.send(
                ProtocolMessage::AgentSessionOp {
                    pane: self.pane,
                    op: AgentSessionOpKind::Switch {
                        session_id: session.session_id,
                        cwd: session.cwd,
                        additional_directories: session.additional_directories,
                    },
                },
                cx,
            );
        });
        self.close_history(window, cx);
    }

    fn can_change_session(&self, cx: &App) -> bool {
        let connection = self.connection.read(cx);
        connection.connected
            && !connection.core.attached_read_only()
            && connection.core.agent_state(self.pane).is_some_and(|state| {
                matches!(state.phase, AgentConnectionPhase::Ready)
                    && state.pending_permission.is_none()
            })
    }

    fn delete_history(&mut self, cx: &mut Context<Self>) {
        if !self.can_change_session(cx)
            || !self
                .connection
                .read(cx)
                .agent_session_delete_supported(self.pane)
            || self.history_loading
        {
            return;
        }
        let Some(session_id) = self.history_delete.clone() else {
            return;
        };
        if self.session_is_open(&session_id, cx) {
            return;
        }
        self.history_loading = true;
        self.connection.update(cx, |connection, cx| {
            connection.send(
                ProtocolMessage::AgentSessionOp {
                    pane: self.pane,
                    op: AgentSessionOpKind::Delete { session_id },
                },
                cx,
            );
        });
        cx.notify();
    }

    fn history(&self, ready: bool, cx: &mut Context<Self>) -> AnyElement {
        use zz_ui::command::palette_shortcut_hint;
        use zz_ui::picker::{
            picker_empty, picker_footer, picker_header, picker_list, picker_modal_sized,
            picker_overlay, picker_row, picker_search,
        };
        let results = self.history_results(cx);
        let count = results.len();
        let sessions = self.sessions.clone();
        let selected = self.history_selected;
        let current = self
            .connection
            .read(cx)
            .core
            .agent_state(self.pane)
            .and_then(|state| state.session_id.clone());
        let can_delete = self
            .connection
            .read(cx)
            .agent_session_delete_supported(self.pane);
        let loading = self.history_loading;
        let compact = self.history_compact;
        let layout_view = cx.entity();
        let view = cx.entity();
        let pane = self.pane;
        let rows = gpui::uniform_list(
            ("agent-history-rows", pane.0),
            count,
            move |range, _, cx| {
                range
                    .filter_map(|index| {
                        let session = sessions.get(*results.get(index)?)?;
                        let current = current.as_deref() == Some(session.session_id.as_str());
                        let open = view.clone();
                        let hover = view.clone();
                        let delete = view.clone();
                        let session_id = session.session_id.clone();
                        let detail = if selected == index {
                            cx.theme().foreground
                        } else {
                            cx.theme().foreground.muted()
                        };
                        Some(
                            picker_row(("agent-history-row", index), selected == index, cx)
                                .h(px(26.0))
                                .menu_item_corners(px(26.0), cx)
                                .px_2()
                                .text_size(zz_ui::rems_from_px(12.0))
                                .line_height(px(16.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(
                                            session
                                                .title
                                                .clone()
                                                .unwrap_or_else(|| directory_label(&session.cwd)),
                                        ),
                                )
                                .when(current, |row| {
                                    row.child(
                                        zz_ui::Icon::new(IconName::Check)
                                            .size(px(12.0))
                                            .text_color(detail),
                                    )
                                })
                                .when(!(can_delete && !current && selected == index), |row| {
                                    row.when_some(
                                        session.updated_at.as_deref().map(history_timestamp),
                                        |row, timestamp| {
                                            row.child(
                                                div()
                                                    .flex_none()
                                                    .text_size(zz_ui::rems_from_px(11.0))
                                                    .text_color(detail)
                                                    .child(timestamp),
                                            )
                                        },
                                    )
                                })
                                .on_mouse_move(move |_, _, cx| {
                                    hover.update(cx, |this, cx| {
                                        if this.history_selected != index || this.project_focus {
                                            this.history_selected = index;
                                            this.project_focus = false;
                                            cx.notify();
                                        }
                                    });
                                })
                                .on_click(move |_, window, cx| {
                                    if ready {
                                        open.update(cx, |this, cx| {
                                            this.open_history_result(index, window, cx);
                                        });
                                    }
                                    cx.stop_propagation();
                                })
                                .when(can_delete && !current && selected == index, |row| {
                                    row.child(
                                        agent_chrome_button(("agent-history-delete", index))
                                            .icon(IconName::Xmark)
                                            .label("Delete")
                                            .tooltip("Delete this session")
                                            .disabled(loading || !ready)
                                            .on_click(move |_, _, cx| {
                                                delete.update(cx, |this, cx| {
                                                    if this.session_is_open(&session_id, cx) {
                                                        this.history_error = Some(
                                                            "This conversation is already open."
                                                                .into(),
                                                        );
                                                    } else {
                                                        this.history_delete =
                                                            Some(session_id.clone());
                                                    }
                                                    cx.notify();
                                                });
                                                cx.stop_propagation();
                                            }),
                                    )
                                }),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .flex_1()
        .w_full()
        .track_scroll(&self.picker_scroll);
        let footer = if self.history_delete.is_some() {
            picker_footer(cx)
                .min_h(px(52.0))
                .flex_wrap()
                .border_color(cx.theme().danger.outline())
                .bg(cx.theme().danger.fill())
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .when(compact, |this| this.w_full().flex_none())
                        .child("Permanently delete this session from the agent’s local store?"),
                )
                .child(
                    zz_ui::h_flex()
                        .flex_none()
                        .gap(px(zz_ui::CHROME_GAP))
                        .child(
                            agent_chrome_button("agent-history-delete-cancel")
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.history_delete = None;
                                    cx.notify();
                                    cx.stop_propagation();
                                })),
                        )
                        .child(
                            agent_chrome_button("agent-history-delete-confirm")
                                .danger()
                                .label("Delete")
                                .disabled(loading)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.delete_history(cx);
                                    cx.stop_propagation();
                                })),
                        ),
                )
        } else {
            picker_footer(cx)
                .flex_wrap()
                .when_some(self.history_error.as_deref(), |footer, error| {
                    footer.child(
                        div()
                            .min_w_0()
                            .text_color(cx.theme().danger)
                            .child(rendered_error(error)),
                    )
                })
                .when(loading, |footer| footer.child("Loading sessions…"))
                .when(
                    !compact && !loading && self.history_error.is_none(),
                    |footer| {
                        footer
                            .child(palette_shortcut_hint(["up", "down"], "select"))
                            .child(palette_shortcut_hint(["enter"], "open"))
                            .child(palette_shortcut_hint(["tab"], "column"))
                            .when(can_delete, |footer| {
                                footer.child(palette_shortcut_hint(["delete"], "delete"))
                            })
                            .child(palette_shortcut_hint(["escape"], "close"))
                    },
                )
                .child(div().flex_1())
                .when(self.next_cursor.is_some(), |footer| {
                    footer.child(
                        agent_chrome_button("agent-history-more")
                            .label("Load more")
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.list_sessions(false, window, cx);
                                cx.stop_propagation();
                            })),
                    )
                })
                .child(
                    agent_chrome_button("agent-project-new")
                        .accent()
                        .text_color(cx.theme().foreground)
                        .min_w_0()
                        .max_w_full()
                        .child(
                            div()
                                .min_w_0()
                                .max_w(px(240.0))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(self.project_directory.as_ref().map_or_else(
                                    || "New session".to_owned(),
                                    |path| format!("New session in {}", directory_label(path)),
                                )),
                        )
                        .disabled(!ready || self.project_directory.is_none())
                        .tooltip("Start a new session in the selected directory (Cmd/Ctrl+Enter)")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_new_session(window, cx);
                            cx.stop_propagation();
                        })),
                )
        };
        picker_overlay(("agent-history-overlay", pane.0), cx).p_2()
            .track_focus(&self.history_input.read(cx).focus_handle(cx))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                this.close_history(window, cx); cx.stop_propagation();
            }))
            .child(picker_modal_sized(("agent-history-modal", pane.0), 920.0, cx)
                .map_element(|modal| modal.w_full().min_w_0().min_h_0())
                .child(picker_header(cx).child(picker_search(&self.history_input, cx)))
                .child(zz_ui::h_flex().relative().flex_1().min_h_0().min_w_0().items_stretch()
                    .when(compact, gpui::Styled::flex_col)
                    .on_prepaint(move |bounds, _, cx| {
                        layout_view.update(cx, |this, cx| {
                            let compact = bounds.size.width < px(560.0);
                            if this.history_compact != compact { this.history_compact = compact; cx.notify(); }
                        });
                    })
                    .child(self.project_directories(cx))
                    .child(zz_ui::v_flex().flex_1().min_w_0().min_h_0().pt(px(zz_ui::CHROME_GAP))
                        .child(zz_ui::h_flex().h(px(26.0)).flex_none().gap_2().px_3()
                            .text_size(zz_ui::rems_from_px(11.0)).line_height(px(16.0)).text_color(cx.theme().foreground.muted())
                            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap()
                                .child(self.project_directory.as_ref().map_or_else(String::new, |path| path.display().to_string())))
                            .when(self.history_supported, |row| row.child(agent_chrome_button("agent-history-refresh").icon(IconName::Redo2)
                                .label("Refresh").disabled(loading)
                                .on_click(cx.listener(|this, _, window, cx| { this.list_sessions(true, window, cx); cx.stop_propagation(); })))))
                        .child(picker_list().p(px(zz_ui::CHROME_GAP)).pt_0().pr_0()
                            .child(zz_ui::v_flex().relative().flex_1().min_h_0().overflow_hidden()
                                .pr(zz_ui::scroll::GUTTER_WIDTH).child(rows).vertical_scrollbar(&self.picker_scroll))
                            .when(count == 0, |list| list.child(picker_empty(if loading {
                                "Loading sessions…"
                            } else if !self.history_supported { "This agent does not provide session history."
                            } else if self.project_directory.is_none() { "Choose a directory or enter a full path."
                            } else if self.history_input.read(cx).value().is_empty() { "No sessions in this directory."
                            } else { "No sessions match that search." }, cx))))))
                .child(footer)).into_any_element()
    }

    fn directory_results(&self, cx: &App) -> Vec<ProjectDirectory> {
        let paths = self
            .connection
            .read(cx)
            .core
            .snapshot()
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .flat_map(|window| window.panes.values())
            .filter_map(|pane| match &pane.kind {
                zz_protocol::PaneKindSnapshot::Agent(descriptor) => descriptor.cwd.clone(),
                _ => None,
            })
            .collect::<Vec<_>>();
        project_directory_rows(
            self.descriptor.cwd.as_deref(),
            &self.sessions,
            &paths,
            &self.history_input.read(cx).value(),
        )
    }

    fn reconcile_project(&mut self, cx: &App) {
        let directories = self.directory_results(cx);
        if !directories
            .iter()
            .any(|row| Some(&row.path) == self.project_directory.as_ref())
        {
            self.project_directory = directories.first().map(|row| row.path.clone());
            self.history_selected = 0;
        }
    }

    fn select_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.project_directory = Some(path);
        self.project_focus = true;
        self.history_selected = 0;
        self.history_delete = None;
        self.picker_scroll
            .scroll_to_item(0, gpui::ScrollStrategy::Nearest);
        cx.notify();
    }

    fn open_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_change_session(cx) {
            return;
        }
        self.history_open = true;
        self.project_hovered = None;
        self.project_directory = self.descriptor.cwd.clone();
        self.project_focus = false;
        self.completions = Arc::from([]);
        self.completion_selected = None;
        self.history_selected = 0;
        self.history_delete = None;
        self.history_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.reconcile_project(cx);
        self.history_input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        self.list_sessions(true, window, cx);
        cx.notify();
    }

    fn start_new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_change_session(cx) {
            return;
        }
        let Some(cwd) = self.project_directory.clone() else {
            return;
        };
        self.connection.update(cx, |connection, cx| {
            connection.send(
                ProtocolMessage::AgentSessionOp {
                    pane: self.pane,
                    op: AgentSessionOpKind::New { cwd },
                },
                cx,
            );
        });
        self.close_history(window, cx);
    }

    fn project_directories(&self, cx: &Context<Self>) -> AnyElement {
        let directories = self.directory_results(cx);
        let recent = directories.iter().take_while(|row| row.recent).count();
        let mut entries = Vec::new();
        for index in 0..directories.len() {
            if index == 0 || index == recent {
                entries.push(None);
            }
            entries.push(Some(index));
        }
        let selected = self.project_directory.clone();
        let hovered = self.project_hovered.clone();
        let view = cx.entity();
        let rows = gpui::uniform_list(
            ("agent-project-directories", self.pane.0),
            entries.len(),
            move |range, _, cx| {
                range
                    .map(|index| {
                        let Some(directory_index) = entries[index] else {
                            return div()
                                .h(px(26.0))
                                .flex()
                                .items_center()
                                .px_2p5()
                                .text_size(zz_ui::rems_from_px(11.0))
                                .line_height(px(16.0))
                                .text_color(cx.theme().foreground.muted())
                                .child(if index == 0 && recent > 0 {
                                    "Recent"
                                } else {
                                    "All directories"
                                })
                                .into_any_element();
                        };
                        let directory = &directories[directory_index];
                        let path = directory.path.clone();
                        let highlighted =
                            Some(&path) == selected.as_ref() || Some(&path) == hovered.as_ref();
                        let pointer_path = path.clone();
                        let hover = view.clone();
                        let click = view.clone();
                        zz_ui::picker::directory_row(
                            ("agent-project-directory", directory_index),
                            directory.label.clone(),
                            highlighted,
                            cx,
                        )
                        .h(px(26.0))
                        .menu_item_corners(px(26.0), cx)
                        .px_2()
                        .line_height(px(16.0))
                        .when(directory.sessions > 0, |row| {
                            row.child(
                                div()
                                    .flex_none()
                                    .text_size(zz_ui::rems_from_px(11.0))
                                    .text_color(if highlighted {
                                        cx.theme().foreground
                                    } else {
                                        cx.theme().foreground.muted()
                                    })
                                    .child(directory.sessions.to_string()),
                            )
                        })
                        .on_hover(move |hovered, _, cx| {
                            hover.update(cx, |this, cx| {
                                let next = if *hovered {
                                    Some(pointer_path.clone())
                                } else if this.project_hovered.as_ref() == Some(&pointer_path) {
                                    None
                                } else {
                                    return;
                                };
                                if this.project_hovered != next {
                                    this.project_hovered = next;
                                    cx.notify();
                                }
                            });
                        })
                        .on_click(move |_, _, cx| {
                            click.update(cx, |this, cx| this.select_project(path.clone(), cx));
                            cx.stop_propagation();
                        })
                        .into_any_element()
                    })
                    .collect::<Vec<_>>()
            },
        )
        .flex_1()
        .w_full()
        .track_scroll(&self.project_scroll);
        zz_ui::v_flex()
            .flex_none()
            .min_w_0()
            .min_h_0()
            .when(self.history_compact, |column| {
                column.w_full().h(gpui::relative(0.35)).border_b_1()
            })
            .when(!self.history_compact, |column| {
                column.w(gpui::relative(0.36)).border_r_1()
            })
            .border_color(cx.theme().border())
            .p(px(zz_ui::CHROME_GAP))
            .pr_0()
            .child(
                zz_ui::v_flex()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .pr(zz_ui::scroll::GUTTER_WIDTH)
                    .child(rows)
                    .vertical_scrollbar(&self.project_scroll),
            )
            .into_any_element()
    }

    fn picker_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if agent_title_is_editing(window) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if !self.history_open {
            if !self.completions.is_empty() {
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
                return;
            }
            self.permission_key_down(event, window, cx);
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                if self.history_delete.take().is_none() {
                    self.close_history(window, cx);
                }
            }
            "tab" => self.project_focus = !self.project_focus,
            "up" | "down"
                if self.history_delete.is_none() && !modifiers.platform && !modifiers.alt =>
            {
                let up = event.keystroke.key == "up";
                let results = self.history_results(cx).len();
                if self.project_focus || results == 0 {
                    let directories = self.directory_results(cx);
                    let count = directories.len();
                    if count > 0 {
                        let current = directories
                            .iter()
                            .position(|row| Some(&row.path) == self.project_directory.as_ref())
                            .unwrap_or_default();
                        let next = if up {
                            current.checked_sub(1).unwrap_or(count - 1)
                        } else {
                            (current + 1) % count
                        };
                        self.select_project(directories[next].path.clone(), cx);
                        let recent = directories.iter().take_while(|row| row.recent).count();
                        self.project_scroll.scroll_to_item(
                            next + 1 + usize::from(recent > 0 && next >= recent),
                            gpui::ScrollStrategy::Nearest,
                        );
                    }
                } else {
                    self.history_selected = if up {
                        self.history_selected.checked_sub(1).unwrap_or(results - 1)
                    } else {
                        (self.history_selected + 1) % results
                    };
                    self.picker_scroll
                        .scroll_to_item(self.history_selected, gpui::ScrollStrategy::Nearest);
                }
            }
            "enter" if self.history_delete.is_none() => {
                if modifiers.platform || self.history_results(cx).is_empty() {
                    self.start_new_session(window, cx);
                } else if self.project_focus {
                    self.project_focus = false;
                } else {
                    self.open_history_result(self.history_selected, window, cx);
                }
            }
            "delete" | "backspace"
                if !self.project_focus
                    && (event.keystroke.key == "delete"
                        || self.history_input.read(cx).value().is_empty()) =>
            {
                if self
                    .connection
                    .read(cx)
                    .agent_session_delete_supported(self.pane)
                    && let Some(session) = self
                        .history_results(cx)
                        .get(self.history_selected)
                        .and_then(|index| self.sessions.get(*index))
                    && !self.session_is_open(&session.session_id, cx)
                {
                    self.history_delete = Some(session.session_id.clone());
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn close_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_open = false;
        self.project_hovered = None;
        self.history_delete = None;
        self.input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn pending_permissions(&self, cx: &App) -> Vec<PendingPermission> {
        let mut requests = self
            .transcript
            .model
            .permissions()
            .iter()
            .map(|request| PendingPermission {
                request_id: request.request_id,
                title: request.title.clone(),
                options: request
                    .options
                    .iter()
                    .map(|option| PermissionChoice {
                        id: option.id.clone(),
                        name: option.name.clone(),
                        allow: Some(matches!(
                            option.kind,
                            AgentPermissionKind::AllowOnce | AgentPermissionKind::AllowAlways
                        )),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        if let Some(permission) = self
            .connection
            .read(cx)
            .core
            .agent_state(self.pane)
            .and_then(|state| state.pending_permission.as_ref())
            && !requests
                .iter()
                .any(|request| request.request_id == permission.request_id)
        {
            let payload = serde_json::from_str::<Value>(&permission.payload).unwrap_or_default();
            requests.push(PendingPermission {
                request_id: permission.request_id,
                title: payload
                    .pointer("/tool_call/title")
                    .or_else(|| payload.pointer("/toolCall/title"))
                    .and_then(Value::as_str)
                    .unwrap_or("This agent needs your permission")
                    .to_owned(),
                options: permission_choices(&payload),
            });
        }
        requests
    }

    fn current_permission(&self, cx: &App) -> Option<PendingPermission> {
        self.pending_permissions(cx)
            .into_iter()
            .find(|request| !self.permission_answered.contains(&request.request_id))
    }

    fn permissions(
        &self,
        permission: PendingPermission,
        index: usize,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let request_id = permission.request_id;
        let writable = self.connection.read(cx).connected
            && !self.connection.read(cx).core.attached_read_only()
            && !self.permission_answered.contains(&request_id);
        let options = permission
            .options
            .into_iter()
            .enumerate()
            .map(|(index, option)| {
                let id = option.id;
                let button = Button::new(format!(
                    "agent-permission-{}-{request_id}-{index}",
                    self.pane.0
                ))
                .small()
                .label(option.name)
                .disabled(!writable)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.respond_permission(request_id, Some(id.clone()), cx);
                    cx.stop_propagation();
                }));
                let button = match option.allow {
                    Some(true) => button.primary(),
                    Some(false) => button.danger(),
                    None => button,
                };
                permission_option(
                    format!(
                        "agent-permission-option-{}-{request_id}-{index}",
                        self.pane.0
                    ),
                    index,
                    index == self.permission_selected,
                    button,
                    cx,
                )
                .on_hover(cx.listener(move |this, hovered, _, cx| {
                    if *hovered && this.permission_selected != index {
                        this.permission_selected = index;
                        cx.notify();
                    }
                }))
                .into_any_element()
            })
            .collect();
        permission_card(
            permission.title,
            (count > 1).then(|| format!("{}/{count}", index + 1).into()),
            options,
            Button::new(format!(
                "agent-permission-cancel-{}-{request_id}",
                self.pane.0
            ))
            .small()
            .ghost()
            .disabled(!writable)
            .label("Cancel request")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.respond_permission(request_id, None, cx);
                cx.stop_propagation();
            })),
            cx,
        )
        .into_any_element()
    }

    fn respond_permission(
        &mut self,
        request_id: u64,
        option_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let connection = self.connection.read(cx);
        if !connection.connected
            || connection.core.attached_read_only()
            || self.permission_answered.contains(&request_id)
            || !self
                .pending_permissions(cx)
                .iter()
                .any(|request| request.request_id == request_id)
        {
            return;
        }
        self.permission_answered.insert(request_id);
        self.connection.update(cx, |connection, cx| {
            connection.send(
                ProtocolMessage::AgentRespondPermission {
                    pane: self.pane,
                    request_id,
                    option_id,
                },
                cx,
            );
        });
        cx.notify();
    }

    fn permission_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.alt || modifiers.control || modifiers.function {
            return;
        }
        let input = self.input.read(cx);
        if !input.value().trim().is_empty() && input.focus_handle(cx).is_focused(window) {
            return;
        }
        let Some(permission) = self.current_permission(cx) else {
            return;
        };
        let options = permission.options;
        match event.keystroke.key.as_str() {
            "escape" => self.respond_permission(permission.request_id, None, cx),
            "up" if !options.is_empty() => {
                self.permission_selected = self
                    .permission_selected
                    .checked_sub(1)
                    .unwrap_or(options.len() - 1);
            }
            "down" if !options.is_empty() => {
                self.permission_selected = (self.permission_selected + 1) % options.len();
            }
            key => {
                let index = if key == "enter" {
                    Some(self.permission_selected)
                } else {
                    key.parse::<usize>()
                        .ok()
                        .filter(|digit| (1..=9).contains(digit))
                        .map(|digit| digit - 1)
                };
                let Some(option) = index.and_then(|index| options.get(index)) else {
                    return;
                };
                self.respond_permission(permission.request_id, Some(option.id.clone()), cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn lifecycle_command(
        &mut self,
        provider: Option<zz_protocol::AgentProvider>,
        cx: &mut Context<Self>,
    ) {
        let connection = self.connection.read(cx);
        if !connection.connected
            || connection.core.attached_read_only()
            || self.lifecycle_pending
            || connection.core.agent_state(self.pane).is_some_and(|state| {
                matches!(
                    state.phase,
                    AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
                )
            })
            || provider == Some(self.descriptor.provider)
        {
            return;
        }
        if provider.is_none() {
            self.settings_apply = None;
        }
        self.lifecycle_pending = true;
        self.lifecycle_generation = self.lifecycle_generation.wrapping_add(1);
        let generation = self.lifecycle_generation;
        let mut args = vec!["-t".to_owned(), self.pane.to_string()];
        let command = if let Some(provider) = provider {
            args.push(provider.as_str().to_owned());
            "set-agent-provider"
        } else {
            "restart-agent-pane"
        };
        self.connection
            .update(cx, |connection, cx| connection.command(command, args, cx));
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(30))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.lifecycle_generation == generation && this.lifecycle_pending {
                    this.lifecycle_pending = false;
                    if this.settings_apply.take().is_some() {
                        this.draft_error = Some("Timed out switching agent providers.".into());
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn render_error(
        &self,
        state: &zz_protocol::AgentPaneWire,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let error = state
            .error
            .as_deref()
            .or(match &state.phase {
                AgentConnectionPhase::Failed { message } => Some(message.as_str()),
                _ => None,
            })
            .or(self.draft_error.as_deref())?;
        let writable = self.connection.read(cx).connected
            && !self.connection.read(cx).core.attached_read_only();
        let failed = matches!(state.phase, AgentConnectionPhase::Failed { .. });
        let methods = serde_json::from_str::<Value>(&state.auth_methods).unwrap_or_default();
        let auth = methods
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, method)| {
                let id = method.get("id")?.as_str()?.to_owned();
                let name = method
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_owned();
                let description = method
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("Authenticate with the agent")
                    .to_owned();
                Some(
                    Button::new(format!("web-agent-authenticate-{}-{index}", self.pane.0))
                        .secondary()
                        .small()
                        .label(name)
                        .tooltip(description)
                        .disabled(!writable || self.lifecycle_pending)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.connection.update(cx, |connection, cx| {
                                connection.send(
                                    ProtocolMessage::AgentAuthenticate {
                                        pane: this.pane,
                                        method_id: id.clone(),
                                    },
                                    cx,
                                );
                            });
                        })),
                )
            });
        Some(
            error_card(error, cx)
                .when(failed, |card| {
                    card.child(
                        zz_ui::h_flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                Button::new(("web-agent-retry", self.pane.0))
                                    .primary()
                                    .small()
                                    .icon(IconName::Redo2)
                                    .label(if self.lifecycle_pending {
                                        "Restarting…"
                                    } else {
                                        "Try again"
                                    })
                                    .disabled(!writable || self.lifecycle_pending)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.lifecycle_command(None, cx);
                                    })),
                            )
                            .children(auth),
                    )
                })
                .into_any_element(),
        )
    }

    fn drive_stick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.reduce_motion() || !self.stick.wants_frame(&self.scroll) {
            return;
        }
        self.stick.arm();
        let view = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            let _ = view.update(cx, |this: &mut Self, cx| {
                if this.stick.step(&this.scroll) {
                    cx.notify();
                }
            });
        });
    }
}

impl Focusable for AgentPane {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl Render for AgentPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.synchronize(window, cx);
        self.synchronize_completions(cx);
        let state = self
            .connection
            .read(cx)
            .core
            .agent_state(self.pane)
            .cloned()
            .unwrap_or_default();
        let connected = self.connection.read(cx).connected;
        let writable = connected && !self.connection.read(cx).core.attached_read_only();
        let running = matches!(
            state.phase,
            AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
        );
        let ready = writable
            && matches!(
                state.phase,
                AgentConnectionPhase::Ready | AgentConnectionPhase::Running
            );
        let permissions = self.pending_permissions(cx);
        self.permission_answered
            .retain(|id| permissions.iter().any(|request| request.request_id == *id));
        let permissions = permissions
            .into_iter()
            .filter(|request| !self.permission_answered.contains(&request.request_id))
            .collect::<Vec<_>>();
        let current_permission = (!permissions.is_empty()).then_some(0);
        let permission_id = current_permission.map(|index| permissions[index].request_id);
        if self.permission_request_id != permission_id {
            self.permission_request_id = permission_id;
            self.permission_selected = 0;
        }
        self.stick.set_bottom_padding(COMPOSER_OUTER_PADDING);
        self.drive_stick(window, cx);
        let show_jump = !self.rows.is_empty() && self.stick.shows_jump_button();
        let mut prefix = Vec::new();
        if let Some(index) = current_permission {
            prefix.push(self.permissions(permissions[index].clone(), index, permissions.len(), cx));
        }
        if state.queued_prompts > 0 {
            prefix.push(
                zz_ui::h_flex()
                    .w_full()
                    .justify_end()
                    .child(
                        Button::new(("agent-unqueue", self.pane.0))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Undo2)
                            .label(format!("{} queued", state.queued_prompts))
                            .tooltip("Return the queued prompts to the composer")
                            .text_color(cx.theme().foreground.muted())
                            .disabled(!writable)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.connection.update(cx, |connection, cx| {
                                    connection.send(
                                        ProtocolMessage::AgentUnqueue { pane: this.pane },
                                        cx,
                                    );
                                });
                                cx.stop_propagation();
                            })),
                    )
                    .into_any_element(),
            );
        }
        prefix.extend(self.render_error(&state, cx));
        prefix.extend(self.render_completions(cx));
        let has_content =
            !self.input.read(cx).value().trim().is_empty() || !self.attachments.is_empty();
        let action_kind = composer_action(running, has_content);
        let action = composer_action_button(
            ("agent-action", self.pane.0),
            action_kind,
            match action_kind {
                ComposerAction::Stop => writable,
                ComposerAction::Queue => writable && self.settings_apply.is_none(),
                ComposerAction::Send => {
                    ready
                        && state.pending_permission.is_none()
                        && has_content
                        && self.settings_apply.is_none()
                }
            },
        );
        let action = if action_kind == ComposerAction::Stop {
            let connection = self.connection.clone();
            let pane = self.pane;
            action.on_click(move |_, _, cx| {
                connection.update(cx, |connection, cx| {
                    connection.send(ProtocolMessage::AgentCancel { pane }, cx);
                });
            })
        } else {
            action.on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
        };
        let mut settings = vec![
            Button::compact_icon("web-agent-attach", IconName::Plus)
                .tooltip("Attach images")
                .disabled(!ready || self.choosing_images)
                .on_click(cx.listener(|this, _, window, cx| this.choose_images(window, cx)))
                .into_any_element(),
        ];
        if cfg!(target_os = "ios") {
            settings.clear();
        }
        settings.extend(self.render_config_controls(&state, cx));
        let controls = config_controls(&state.config_options, &state.modes);
        let model = controls
            .iter()
            .find(|option| option.category == "model")
            .cloned();
        let effort = controls
            .iter()
            .find(|option| option.category == "thought_level")
            .cloned();
        let provider_view = cx.entity();
        let catalog_view = cx.entity();
        settings.push(agent_model_picker(
            ("web-agent-model", self.pane.0),
            (
                format!("{:?}", self.connection.entity_id()),
                self.descriptor.cwd.clone().unwrap_or_default(),
            ),
            self.descriptor.provider,
            model
                .map(|option| AgentControlSelection {
                    current_value: option.current_value,
                    choices: option.choices,
                })
                .unwrap_or_default(),
            effort.map(|option| AgentControlSelection {
                current_value: option.current_value,
                choices: option.choices,
            }),
            writable
                && !running
                && !self.lifecycle_pending
                && !self.settings_busy
                && self.settings_apply.is_none(),
            state.phase == AgentConnectionPhase::Ready && !self.lifecycle_pending,
            self.catalogs.results().to_vec(),
            move |provider, cx| {
                catalog_view.update(cx, |view, cx| view.request_catalog(provider, cx));
            },
            move |selection, cx| {
                provider_view.update(cx, |view, cx| view.apply_settings(selection, cx));
            },
            window,
            cx,
        ));
        let usage = self.usage.map(|(used, size)| {
            context_usage_meter(("agent-context-usage", self.pane.0), used, size, cx)
        });
        let directory = agent_directory_button(
            "web-agent-directory",
            self.descriptor.cwd.as_ref().map_or_else(
                || "Daemon working directory".to_owned(),
                |path| directory_label(path),
            ),
            self.can_change_session(cx),
            cx,
        )
        .tooltip(self.descriptor.cwd.as_ref().map_or_else(
            || "Agent working directory".to_owned(),
            |path| path.to_string_lossy().into_owned(),
        ))
        .on_click(cx.listener(|this, _, window, cx| this.open_directory(window, cx)))
        .into_any_element();
        let pane = self.pane;
        let composer =
            AgentComposer {
                input: self.input.clone(),
                action: action.into_any_element(),
                settings,
                usage,
                footer_actions: vec![directory],
                git: state.git.as_ref().map(|git| {
                    git_summary_footer(
                        ("agent-git-summary", self.pane.0),
                        git.branch.clone().map(Into::into),
                        git.changed_files,
                        git.additions,
                        git.deletions,
                        cx,
                    )
                }),
                command_hint: active_command_hint(&self.last_input, &self.commands).map(Into::into),
                prefix,
                attachments: (!self.attachments.is_empty()).then(|| {
                    zz_ui::h_flex()
                        .w_full()
                        .flex_wrap()
                        .gap_2()
                        .px_3()
                        .pt_2()
                        .children(self.attachments.iter().enumerate().filter_map(
                            |(index, image)| {
                                let preview = preview_image(image)?;
                                Some(
                                    div()
                                        .relative()
                                        .child(agent_attachment_thumbnail(
                                            ("web-agent-image", index),
                                            preview,
                                            COMPOSER_ATTACHMENT,
                                            cx,
                                        ))
                                        .child(
                                            div().absolute().top(px(-6.0)).right(px(-6.0)).child(
                                                Button::compact_icon(
                                                    ("web-agent-remove-image", index),
                                                    IconName::Xmark,
                                                )
                                                .tooltip("Remove this image")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if index < this.attachments.len() {
                                                        this.attachments.remove(index);
                                                    }
                                                    this.draft_error = None;
                                                    cx.notify();
                                                    cx.stop_propagation();
                                                })),
                                            ),
                                        ),
                                )
                            },
                        ))
                        .into_any_element()
                }),
            };
        let provider = self.descriptor.provider;
        let title = self
            .connection
            .read(cx)
            .core
            .snapshot()
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .find_map(|window| window.panes.get(&self.pane))
            .map_or_else(|| "New session".to_owned(), |pane| pane.title.clone());
        let title_connection = self.connection.clone();
        let header_controls = zz_ui::h_flex()
            .min_w_0()
            .gap(px(zz_ui::CHROME_GAP))
            .child(zz_ui::agent::controls::agent_provider_label(provider, cx))
            .child(agent_thread_title_editor(
                ("web-agent-title", pane.0),
                &title,
                writable,
                move |title, cx| {
                    title_connection.update(cx, |connection, cx| {
                        connection.command(
                            "select-pane",
                            vec![
                                "-t".to_owned(),
                                pane.to_string(),
                                "-T".to_owned(),
                                title.to_owned(),
                            ],
                            cx,
                        );
                    });
                },
                window,
                cx,
            ));
        let can_drag = writable
            && self.header_drag_handler.is_some()
            && self
                .connection
                .read(cx)
                .core
                .snapshot()
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .any(|window| {
                    window.panes.contains_key(&pane)
                        && window.zoomed_pane.is_none()
                        && window.panes.len() > 1
                });
        let drag_handler = self.header_drag_handler.clone();
        let header_drag = pane_drag_button(
            ("agent-pane-drag", pane.0),
            pane,
            title,
            can_drag,
            move |drag, window, cx| {
                if let Some(handler) = &drag_handler {
                    handler(drag, window, cx);
                }
            },
            cx,
        )
        .relative()
        .when(can_drag, |grip| {
            grip.when_some(self.header_touch_drag_handler.clone(), |grip, handler| {
                grip.child(super::touch_drag_handle(move |event, window, cx| {
                    handler(event, window, cx);
                }))
            })
        });
        let empty = self.rows.is_empty().then(|| {
            let empty_message = if connected {
                match state.phase {
                    AgentConnectionPhase::Starting => format!("Starting {}…", provider.label()),
                    AgentConnectionPhase::Ready => return welcome_state(cx),
                    AgentConnectionPhase::Running => {
                        format!("Waiting for {}’s first update…", provider.label())
                    }
                    AgentConnectionPhase::AwaitingPermission => {
                        "The agent needs your permission.".to_owned()
                    }
                    AgentConnectionPhase::Failed { .. } => {
                        "The agent could not start this session.".to_owned()
                    }
                }
            } else {
                "The ACP agent is offline.".to_owned()
            };
            empty_state(
                empty_message,
                connected
                    && matches!(
                        state.phase,
                        AgentConnectionPhase::Starting | AgentConnectionPhase::Running
                    ),
                cx.entity_id(),
                cx,
            )
        });
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .rounded_tl(self.corner_radii.top_left)
            .rounded_tr(self.corner_radii.top_right)
            .rounded_bl(self.corner_radii.bottom_left)
            .rounded_br(self.corner_radii.bottom_right)
            .bg(cx
                .theme()
                .background
                .opaque()
                .opacity(cx.theme().pane_background_opacity))
            .child(agent_pane_header(
                self.connection
                    .read(cx)
                    .core
                    .snapshot()
                    .sessions
                    .iter()
                    .flat_map(|session| &session.windows)
                    .any(|window| window.active_pane == self.pane),
                header_controls,
                zz_ui::h_flex()
                    .gap(px(zz_ui::CHROME_GAP))
                    .children(
                        [
                            (
                                "web-agent-split-bottom",
                                IconName::PanelBottom,
                                "Split bottom",
                                "-v",
                            ),
                            (
                                "web-agent-split-right",
                                IconName::PanelRight,
                                "Split right",
                                "-h",
                            ),
                        ]
                        .into_iter()
                        .map(|(id, icon, label, flag)| {
                            pane_header_icon_button(id, icon, writable, cx)
                                .tooltip(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.connection.update(cx, |connection, cx| {
                                        connection.command(
                                            "split-window",
                                            vec![
                                                "--kind".into(),
                                                "picker".into(),
                                                flag.to_owned(),
                                                "-t".to_owned(),
                                                this.pane.to_string(),
                                            ],
                                            cx,
                                        );
                                    });
                                    cx.stop_propagation();
                                }))
                        }),
                    )
                    .child(header_drag)
                    .child(
                        pane_header_icon_button("web-agent-close", IconName::Xmark, writable, cx)
                            .tooltip("Close pane")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.connection.update(cx, |connection, cx| {
                                    connection.command(
                                        "kill-pane",
                                        vec!["-t".to_owned(), this.pane.to_string()],
                                        cx,
                                    );
                                });
                                cx.stop_propagation();
                            })),
                    ),
                !self.rows.is_empty(),
                cx,
            ))
            .child(
                div()
                    .id("web-agent-timeline")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .children(empty)
                    .when(!self.rows.is_empty(), |area| {
                        area.child(
                            AgentTimeline::new(
                                self.rows.clone(),
                                self.scroll.clone(),
                                self.timeline.clone(),
                            )
                            .active_turn(running)
                            .bottom_padding(COMPOSER_OUTER_PADDING),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .bottom(px(COMPOSER_OUTER_PADDING))
                                .child(Scrollbar::vertical(&self.scroll)),
                        )
                    })
                    .when(show_jump, |area| {
                        area.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom(px(2.0 * COMPOSER_OUTER_PADDING))
                                .flex()
                                .justify_center()
                                .child(
                                    agent_jump_to_bottom_button(
                                        ("web-agent-jump-to-end", self.pane.0),
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.stick.engage(&this.scroll, cx.reduce_motion());
                                            cx.notify();
                                            cx.stop_propagation();
                                        },
                                    )),
                                ),
                        )
                    }),
            )
            .child(composer)
            .when(self.history_open, |pane| {
                pane.child(self.history(ready && !running, cx))
            })
            .capture_action(cx.listener(Self::complete))
            .capture_action(cx.listener(Self::move_completion_up))
            .capture_action(cx.listener(Self::move_completion_down))
            .capture_key_down(cx.listener(Self::picker_key_down))
    }
}

#[derive(Clone)]
struct PendingPermission {
    request_id: u64,
    title: String,
    options: Vec<PermissionChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PermissionChoice {
    id: String,
    name: String,
    allow: Option<bool>,
}

fn permission_choices(payload: &Value) -> Vec<PermissionChoice> {
    payload
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let id = option
                .get("optionId")
                .or_else(|| option.get("id"))?
                .as_str()?
                .to_owned();
            Some(PermissionChoice {
                name: option
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_owned(),
                allow: match option.get("kind").and_then(Value::as_str) {
                    Some("allow_once" | "allow_always") => Some(true),
                    Some("reject_once" | "reject_always") => Some(false),
                    _ => None,
                },
                id,
            })
        })
        .collect()
}

fn decode_transcript_image(format: &str, data: Vec<u8>) -> Option<Arc<gpui::Image>> {
    let image = AgentImage {
        format: format.to_owned(),
        data,
    };
    crate::attachments::validate_images("", std::slice::from_ref(&image)).ok()?;
    preview_image(&image)
}

fn preview_image(image: &AgentImage) -> Option<Arc<gpui::Image>> {
    Some(Arc::new(gpui::Image::from_bytes(
        gpui::ImageFormat::from_mime_type(&image.format)?,
        image.data.clone(),
    )))
}

fn restored_prompts(
    update: &Value,
    client: Option<zz_protocol::ClientInstanceId>,
) -> Vec<(String, Vec<AgentImage>)> {
    update
        .get("prompts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|prompt| {
            let owner = match prompt.get("owner") {
                Some(owner) => {
                    serde_json::from_value::<zz_protocol::ClientInstanceId>(owner.clone()).ok()?
                }
                None => zz_protocol::ClientInstanceId::default(),
            };
            if owner.0 != 0 && Some(owner) != client {
                return None;
            }
            let text =
                String::from_utf8(BASE64.decode(prompt.get("text")?.as_str()?).ok()?).ok()?;
            let images = prompt
                .get("images")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|image| {
                    Some(AgentImage {
                        format: image.get("format")?.as_str()?.to_owned(),
                        data: BASE64.decode(image.get("data")?.as_str()?).ok()?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some((text, images))
        })
        .collect()
}

struct Transcript {
    model: AgentTranscript<Arc<gpui::Image>>,
    entries: Vec<AgentEntry>,
    markdown: HashMap<u64, AgentMarkdown>,
    tool_payloads: HashMap<(u64, usize), AgentToolPayload>,
    revision: u64,
    session_reset: bool,
}

impl Default for Transcript {
    fn default() -> Self {
        Self {
            model: AgentTranscript::new(decode_transcript_image),
            entries: Vec::new(),
            markdown: HashMap::new(),
            tool_payloads: HashMap::new(),
            revision: 0,
            session_reset: false,
        }
    }
}

impl Transcript {
    fn local_prompt(&mut self, text: &str, images: &[AgentImage]) {
        self.model.begin_prompt(
            text.to_owned(),
            images.iter().filter_map(preview_image).collect(),
        );
        self.synchronize();
    }

    fn synchronize(&mut self) {
        let changed = self.model.changed_entries(self.revision);
        let indices = changed.unwrap_or_else(|| (0..self.model.entries().len()).collect());
        for index in indices {
            let entry = ui_entry_with_markdown(
                &self.model.entries()[index],
                &mut self.markdown,
                &mut self.tool_payloads,
            );
            if index < self.entries.len() {
                self.entries[index] = entry;
            } else {
                self.entries.push(entry);
            }
        }
        self.revision = self.model.revision();
    }

    fn apply(&mut self, _sequence: u64, item: &Value) {
        match item.get("item").and_then(Value::as_str).unwrap_or_default() {
            "sessionReset" => {
                *self = Self::default();
                self.session_reset = true;
            }
            "sessionReady" => {
                self.session_reset = false;
                self.model.finish_replay();
                self.model.finish_turn();
            }
            "sessionSwitched" => {
                if !self.session_reset {
                    *self = Self::default();
                }
                for update in item
                    .get("replay")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Ok(update) = serde_json::from_value(update.clone()) {
                        self.model.apply_update(update);
                    }
                }
                self.session_reset = false;
                self.model.finish_replay();
            }
            "update" => {
                if let Some(update) = item.get("update")
                    && let Ok(update) = serde_json::from_value(update.clone())
                {
                    self.model.apply_update(update);
                }
            }
            "permissionRequested" => {
                if let (Some(request_id), Some(tool), Some(options)) = (
                    item.get("request_id").and_then(Value::as_u64),
                    item.get("tool_call"),
                    item.get("options"),
                ) && let (Ok(tool), Ok(options)) = (
                    serde_json::from_value(tool.clone()),
                    serde_json::from_value(options.clone()),
                ) {
                    self.model.request_permission(request_id, tool, options);
                }
            }
            "permissionResolved" => {
                if let Some(request_id) = item.get("request_id").and_then(Value::as_u64) {
                    self.model.resolve_permission(
                        request_id,
                        item.get("canceled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    );
                }
            }
            "promptFinished" => {
                if item.pointer("/outcome/outcome").and_then(Value::as_str) == Some("failed") {
                    self.model.fail_inflight();
                } else if item.pointer("/outcome/stop_reason").and_then(Value::as_str)
                    == Some("cancelled")
                {
                    self.model.cancel_inflight();
                } else {
                    self.model.finish_turn();
                }
                self.model.finish_replay();
            }
            "paneFailed" => self.model.fail_inflight(),
            _ => {}
        }
        self.synchronize();
    }
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
    entry: &AgentThreadEntry<Arc<gpui::Image>>,
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
            label: gpui::SharedString::from(label.clone()),
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
                label: gpui::SharedString::from(label.clone()),
                location: location.clone().map(gpui::SharedString::from),
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

#[cfg(test)]
mod tests {
    #[test]
    fn directory_queries_use_daemon_paths_without_browser_filesystem_support() {
        use super::absolute_daemon_directory;
        assert_eq!(
            absolute_daemon_directory(" /tmp/project "),
            Some(std::path::PathBuf::from("/tmp/project"))
        );
        assert_eq!(
            absolute_daemon_directory("/"),
            Some(std::path::PathBuf::from("/"))
        );
        for query in ["", "tmp", "~/dev", "/tmp\0project"] {
            assert_eq!(absolute_daemon_directory(query), None);
        }
        assert_eq!(
            absolute_daemon_directory(&format!("/{}", "x".repeat(64 * 1024))),
            None
        );
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn replayed_image_only_messages_and_text_chunks_share_the_user_row() {
        let content =
            json!({"type":"image","mimeType":"image/png","data":BASE64.encode([1, 2, 3])});
        let mut transcript = Transcript::default();
        transcript.apply(1, &json!({"item":"sessionSwitched","replay":[
            {"sessionUpdate":"user_message_chunk","messageId":"a","content":content},
            {"sessionUpdate":"user_message_chunk","messageId":"a","content":{"type":"text","text":"Look at this"}},
            {"sessionUpdate":"user_message_chunk","messageId":"b","content":content}
        ]}));
        assert_eq!(transcript.entries.len(), 2);
        let AgentEntry::User {
            markdown, images, ..
        } = &transcript.entries[0]
        else {
            panic!()
        };
        assert_eq!(markdown.full_text(), "Look at this");
        assert_eq!(images.len(), 1);
        let AgentEntry::User {
            markdown, images, ..
        } = &transcript.entries[1]
        else {
            panic!()
        };
        assert!(markdown.trim_is_empty());
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn local_image_echo_is_suppressed_but_a_later_clients_image_is_kept() {
        let image = AgentImage {
            format: "image/png".into(),
            data: vec![1, 2, 3],
        };
        let content =
            json!({"type":"image","mimeType":image.format,"data":BASE64.encode(&image.data)});
        let mut transcript = Transcript::default();
        transcript.local_prompt("", &[image]);
        transcript.apply(1, &json!({"item":"turnStarted"}));
        transcript.apply(2, &json!({"item":"update","update":{"sessionUpdate":"user_message_chunk","content":content}}));
        assert_eq!(transcript.entries.len(), 1);
        transcript.apply(3, &json!({"item":"promptFinished"}));
        transcript.apply(4, &json!({"item":"update","update":{"sessionUpdate":"user_message_chunk","content":content}}));
        assert_eq!(transcript.entries.len(), 2);
        for entry in &transcript.entries {
            let AgentEntry::User { images, .. } = entry else {
                panic!()
            };
            assert_eq!(images.len(), 1);
        }
    }

    #[test]
    fn plan_updates_replace_the_existing_row_and_keep_in_progress_markers() {
        let mut transcript = Transcript::default();
        for (sequence, status) in [(1, "in_progress"), (2, "completed")] {
            transcript.apply(sequence, &json!({"item":"update","update":{
                "sessionUpdate":"plan","entries":[{"content":"Ship shared UI","priority":"high","status":status}]
            }}));
            let AgentEntry::Plan { markdown, .. } = &transcript.entries[0] else {
                panic!()
            };
            assert_eq!(
                markdown.full_text(),
                if sequence == 1 {
                    "- [~] Ship shared UI"
                } else {
                    "- [x] Ship shared UI"
                }
            );
        }
        assert_eq!(transcript.entries.len(), 1);
    }

    #[test]
    fn structured_tool_output_permissions_and_cancellation_follow_the_shared_model() {
        let mut transcript = Transcript::default();
        transcript.apply(1, &json!({"item":"update","update":{
            "sessionUpdate":"tool_call","toolCallId":"t","title":"Run tests","kind":"execute","status":"in_progress",
            "locations":[{"path":"src/main.rs","line":12}],
            "content":[{"type":"terminal","terminalId":"pty-1"}],"rawOutput":{"ignored":true}
        }}));
        let AgentEntry::Tool(tool) = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(tool.location.as_deref(), Some("src/main.rs:12"));
        assert!(matches!(&tool.output[0], AgentToolPayload::Terminal(_)));
        transcript.apply(2, &json!({"item":"permissionRequested","request_id":7,
            "tool_call":{"toolCallId":"t"},"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"}]}));
        let AgentEntry::Tool(tool) = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(tool.status, AgentToolStatus::NeedsApproval);
        assert_eq!(transcript.model.permissions().len(), 1);
        transcript.apply(
            3,
            &json!({"item":"permissionResolved","request_id":7,"canceled":true}),
        );
        let AgentEntry::Tool(tool) = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(tool.status, AgentToolStatus::Canceled);
        assert!(transcript.model.permissions().is_empty());
    }

    #[test]
    fn failed_and_canceled_turns_settle_unfinished_tools() {
        for (outcome, expected) in [
            (
                json!({"outcome":"failed","message":"lost connection"}),
                AgentToolStatus::Failed,
            ),
            (
                json!({"outcome":"finished","stop_reason":"cancelled"}),
                AgentToolStatus::Canceled,
            ),
        ] {
            let mut transcript = Transcript::default();
            transcript.apply(1, &json!({"item":"update","update":{
                "sessionUpdate":"tool_call","toolCallId":"t","title":"Read file","kind":"read","status":"in_progress"
            }}));
            transcript.apply(2, &json!({"item":"promptFinished","outcome":outcome}));
            let AgentEntry::Tool(tool) = &transcript.entries[0] else {
                panic!()
            };
            assert_eq!(tool.status, expected);
        }
    }

    #[test]
    fn project_picker_searches_session_titles_ids_and_remote_paths() {
        let sessions = vec![
            AgentSessionSummary {
                session_id: "opaque-a".into(),
                cwd: "/work/api".into(),
                title: Some("Fix login".into()),
                additional_directories: vec![],
                updated_at: None,
            },
            AgentSessionSummary {
                session_id: "opaque-b".into(),
                cwd: "/other/api".into(),
                title: Some("Draft docs".into()),
                additional_directories: vec![],
                updated_at: None,
            },
        ];
        let rows = project_directory_rows(Some(Path::new("/work/api")), &sessions, &[], "");
        assert_eq!(
            rows.iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            ["/work/api", "/other/api"]
        );
        assert_eq!(rows[0].sessions, 1);
        let filtered =
            project_directory_rows(Some(Path::new("/work/api")), &sessions, &[], "login");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].path, PathBuf::from("/work/api"));
        assert_eq!(ranked_session_indices(&sessions, "opaque-b"), [1]);
        let typed = project_directory_rows(None, &[], &[], "/remote/project");
        assert_eq!(typed[0].path, PathBuf::from("/remote/project"));
        assert!(!typed[0].recent);
    }

    #[test]
    fn config_controls_preserve_grouped_values_and_composer_order() {
        let options = json!([
            {"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"high","options":[{"name":"Thinking","options":[{"value":"high","name":"High","description":"More reasoning"}]}]},
            {"id":"model","name":"Model","category":"model","type":"select","currentValue":"model-a","options":[{"value":"model-a","name":"Model A"}]},
            {"id":"permissions","name":"Permissions","category":"mode","type":"select","currentValue":"ask","options":[{"value":"ask","name":"Ask first"}]}
        ]);
        let controls = config_controls(&options.to_string(), "{}");
        assert_eq!(
            controls
                .iter()
                .map(|control| control.id.as_str())
                .collect::<Vec<_>>(),
            ["permissions", "model", "effort"]
        );
        assert_eq!(controls[2].current_value, "high");
        assert_eq!(
            controls[2].choices[0].description.as_deref(),
            Some("More reasoning")
        );
        assert!(controls.iter().all(|control| !control.legacy_mode));
    }

    #[test]
    fn legacy_mode_controls_keep_the_provider_selection() {
        let modes =
            json!({"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask first"}]})
                .to_string();
        let controls = config_controls("[]", &modes);
        assert_eq!(controls.len(), 1);
        assert!(controls[0].legacy_mode);
        assert_eq!(controls[0].current_value, "ask");
        let options =
            json!([{"id":"model","name":"Model","type":"select","options":[]}]).to_string();
        assert!(!config_controls(&options, &modes)[0].legacy_mode);
    }

    #[test]
    fn history_timestamps_use_compact_calendar_labels() {
        let date = |year, month, day| {
            chrono::NaiveDate::from_ymd_opt(year, month, day).expect("valid fixture date")
        };
        let today = date(2026, 7, 20);
        assert_eq!(history_timestamp_label(today, 18, 1, today), "Today, 18:01");
        assert_eq!(
            history_timestamp_label(date(2026, 7, 19), 9, 43, today),
            "Yesterday, 09:43"
        );
        assert_eq!(
            history_timestamp_label(date(2026, 6, 4), 7, 5, today),
            "Jun 4, 07:05"
        );
        assert_eq!(
            history_timestamp_label(date(2025, 12, 31), 23, 59, today),
            "Dec 31, 2025, 23:59"
        );
        assert_eq!(history_timestamp("not a date"), "not a date");
    }

    #[test]
    fn usage_reads_live_and_history_updates() {
        let update = json!({"sessionUpdate":"usage_update","used":400,"size":1000});
        assert_eq!(usage_from_update(&update), Some((400, 1000)));
        assert_eq!(
            usage_from_update(&json!({"item":"update","update":update})),
            Some((400, 1000))
        );
        assert_eq!(
            usage_from_update(&json!({"sessionUpdate":"usage_update","used":1})),
            None
        );
    }

    #[test]
    fn submitted_prompt_is_visible_without_a_provider_echo_and_not_duplicated_by_one() {
        let mut transcript = Transcript::default();
        transcript.local_prompt("hello world", &[]);
        assert_eq!(transcript.entries.len(), 1);
        transcript.apply(1, &json!({"item":"turnStarted"}));
        for (sequence, text) in [(2, "hello"), (3, " world")] {
            transcript.apply(sequence, &json!({"item":"update","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":text}}}));
        }
        assert_eq!(transcript.entries.len(), 1);
        transcript.apply(4, &json!({"item":"promptFinished"}));
        transcript.apply(5, &json!({"item":"update","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"another client"}}}));
        assert_eq!(transcript.entries.len(), 2);
    }

    #[test]
    fn restored_drafts_keep_unicode_images_and_only_the_owners_prompts() {
        let update = json!({"item":"promptsRestored","prompts":[
            {"owner":7,"text":BASE64.encode("héllo"),"images":[{"format":"image/png","data":BASE64.encode([1,2,3])}]},
            {"owner":8,"text":BASE64.encode("someone else's draft"),"images":[]},
            {"owner":0,"text":BASE64.encode("legacy"),"images":[]},
            {"owner":"bad","text":BASE64.encode("invalid owner"),"images":[]}
        ]});
        let prompts = restored_prompts(&update, Some(zz_protocol::ClientInstanceId(7)));
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].0, "héllo");
        assert_eq!(prompts[0].1[0].data, [1, 2, 3]);
        assert_eq!(prompts[1].0, "legacy");
        assert_eq!(restored_prompts(&update, None).len(), 1);
    }

    #[test]
    fn chunks_keep_message_identity_and_turn_boundaries() {
        let mut transcript = Transcript::default();
        for (sequence, text) in [(1, "hello"), (2, " world")] {
            transcript.apply(sequence, &json!({"item":"update","update":{"sessionUpdate":"agent_message_chunk","messageId":"a","content":{"type":"text","text":text}}}));
        }
        assert_eq!(transcript.entries.len(), 1);
        let AgentEntry::Assistant { markdown, .. } = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(markdown.full_text(), "hello world");
        transcript.apply(3, &json!({"item":"promptFinished"}));
        transcript.apply(4, &json!({"item":"update","update":{"sessionUpdate":"agent_message_chunk","messageId":"a","content":{"type":"text","text":"next"}}}));
        assert_eq!(transcript.entries.len(), 2);
    }

    #[test]
    fn session_switch_preserves_streamed_history_and_replaces_a_previous_session() {
        let mut transcript = Transcript::default();
        transcript.apply(1, &json!({"item":"sessionReset","restoring":true}));
        transcript.apply(2, &json!({"item":"update","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"restored"}}}));
        transcript.apply(3, &json!({"item":"sessionSwitched","replay":[]}));
        let AgentEntry::Assistant { markdown, .. } = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(markdown.full_text(), "restored");
        transcript.apply(4, &json!({"item":"sessionSwitched","replay":[{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"another session"}}]}));
        assert_eq!(transcript.entries.len(), 1);
        let AgentEntry::Assistant { markdown, .. } = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(markdown.full_text(), "another session");
    }

    #[test]
    fn session_reset_discards_old_messages_and_tools() {
        let mut transcript = Transcript::default();
        transcript.apply(1, &json!({"item":"update","update":{"sessionUpdate":"tool_call","toolCallId":"t","title":"Run tests"}}));
        transcript.apply(2, &json!({"item":"update","update":{"sessionUpdate":"tool_call_update","toolCallId":"t","status":"completed"}}));
        let AgentEntry::Tool(tool) = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(tool.status, AgentToolStatus::Completed);
        transcript.apply(3, &json!({"item":"sessionReset"}));
        assert!(transcript.entries.is_empty());
        assert!(transcript.model.permissions().is_empty());
    }
}

fn usage_from_update(update: &Value) -> Option<(u64, u64)> {
    let update = update.get("update").unwrap_or(update);
    (update.get("sessionUpdate").and_then(Value::as_str) == Some("usage_update"))
        .then(|| Some((update.get("used")?.as_u64()?, update.get("size")?.as_u64()?)))?
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConfigControl {
    id: String,
    name: String,
    description: Option<String>,
    category: String,
    current_value: String,
    choices: Vec<AgentControlChoice>,
    legacy_mode: bool,
}

fn config_controls(config_options: &str, modes: &str) -> Vec<ConfigControl> {
    let options: Value = serde_json::from_str(config_options).unwrap_or_default();
    let mut controls: Vec<_> = options
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| {
            if option.get("type").and_then(Value::as_str) != Some("select") {
                return None;
            }
            let id = option.get("id")?.as_str()?.to_owned();
            let choices = option
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(|choice| {
                    choice
                        .get("options")
                        .and_then(Value::as_array)
                        .map_or_else(|| vec![choice], |group| group.iter().collect())
                })
                .filter_map(|choice| {
                    let value = choice.get("value")?.as_str()?.to_owned();
                    Some(AgentControlChoice {
                        name: choice
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(&value)
                            .to_owned(),
                        description: choice
                            .get("description")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        value,
                    })
                })
                .collect();
            Some(ConfigControl {
                name: option
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_owned(),
                description: option
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                category: option
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                current_value: option
                    .get("currentValue")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                choices,
                id,
                legacy_mode: false,
            })
        })
        .collect();
    controls.sort_by_key(|option| match option.category.as_str() {
        "mode" => 0,
        "model" => 1,
        "thought_level" => 2,
        _ => 3,
    });
    if controls.is_empty() {
        let modes: Value = serde_json::from_str(modes).unwrap_or_default();
        let choices: Vec<_> = modes
            .get("availableModes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|mode| {
                let value = mode.get("id")?.as_str()?.to_owned();
                Some(AgentControlChoice {
                    name: mode
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&value)
                        .to_owned(),
                    description: mode
                        .get("description")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    value,
                })
            })
            .collect();
        if !choices.is_empty() {
            controls.push(ConfigControl {
                id: "legacy-session-mode".to_owned(),
                name: "Permissions".to_owned(),
                description: Some("Agent permission mode".to_owned()),
                category: "mode".to_owned(),
                current_value: modes
                    .get("currentModeId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                choices,
                legacy_mode: true,
            });
        }
    }
    controls
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectDirectory {
    path: PathBuf,
    label: String,
    sessions: usize,
    recent: bool,
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

fn project_directory_rows(
    cwd: Option<&Path>,
    sessions: &[AgentSessionSummary],
    directories: &[PathBuf],
    query: &str,
) -> Vec<ProjectDirectory> {
    let matching = ranked_session_indices(sessions, query)
        .into_iter()
        .map(|index| sessions[index].cwd.as_path())
        .collect::<BTreeSet<_>>();
    let needle = query.trim().to_lowercase();
    let mut counts = HashMap::<PathBuf, usize>::new();
    for session in sessions {
        *counts.entry(session.cwd.clone()).or_default() += 1;
    }
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for (path, recent) in cwd
        .into_iter()
        .chain(sessions.iter().map(|session| session.cwd.as_path()))
        .map(|path| (path, true))
        .chain(directories.iter().map(|path| (path.as_path(), false)))
    {
        if (needle.is_empty()
            || completion_score(&path.to_string_lossy().to_lowercase(), &needle).is_some()
            || matching.contains(path))
            && seen.insert(path.to_path_buf())
        {
            rows.push(ProjectDirectory {
                path: path.to_path_buf(),
                label: directory_label(path),
                sessions: counts.get(path).copied().unwrap_or_default(),
                recent,
            });
        }
    }
    if let Some(path) = absolute_daemon_directory(query)
        && seen.insert(path.clone())
    {
        rows.push(ProjectDirectory {
            label: directory_label(&path),
            path,
            sessions: 0,
            recent: false,
        });
    }
    let mut labels = HashMap::<String, usize>::new();
    for row in &rows {
        *labels.entry(row.label.clone()).or_default() += 1;
    }
    for row in &mut rows {
        if labels[&row.label] > 1 {
            row.label = row.path.display().to_string();
        }
    }
    rows
}

fn absolute_daemon_directory(query: &str) -> Option<PathBuf> {
    let query = query.trim();
    (query.starts_with('/') && query.len() <= 64 * 1024 && !query.contains('\0'))
        .then(|| PathBuf::from(query))
}

fn directory_label(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn history_timestamp(value: &str) -> String {
    #[cfg(target_family = "wasm")]
    {
        let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(value));
        let now = js_sys::Date::new_0();
        let day = |date: &js_sys::Date| {
            chrono::NaiveDate::from_ymd_opt(
                date.get_full_year() as i32,
                date.get_month() + 1,
                date.get_date(),
            )
        };
        match (day(&date), day(&now)) {
            (Some(day), Some(today)) if date.get_time().is_finite() => {
                history_timestamp_label(day, date.get_hours(), date.get_minutes(), today)
            }
            _ => value.into(),
        }
    }
    #[cfg(not(target_family = "wasm"))]
    {
        use chrono::Timelike as _;
        let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) else {
            return value.to_owned();
        };
        let local = timestamp.with_timezone(&chrono::Local);
        history_timestamp_label(
            local.date_naive(),
            local.hour(),
            local.minute(),
            chrono::Local::now().date_naive(),
        )
    }
}

fn history_timestamp_label(
    date: chrono::NaiveDate,
    hour: u32,
    minute: u32,
    today: chrono::NaiveDate,
) -> String {
    use chrono::Datelike as _;
    let time = format!("{hour:02}:{minute:02}");
    if date == today {
        format!("Today, {time}")
    } else if today.pred_opt() == Some(date) {
        format!("Yesterday, {time}")
    } else if date.year() == today.year() {
        format!("{} {}, {time}", date.format("%b"), date.day())
    } else {
        format!(
            "{} {}, {}, {time}",
            date.format("%b"),
            date.day(),
            date.year()
        )
    }
}
