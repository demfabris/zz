use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gpui::{
    AnyElement, App, Context, Corners, Entity, FocusHandle, Focusable, IntoElement, ListAlignment,
    ListState, MouseButton, Render, Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;
use zz_protocol::{
    AgentConnectionPhase, AgentDescriptor, AgentImage, AgentSessionOpKind, PaneId, ProtocolMessage,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, IconName, Sizable as _,
    agent::{
        AgentEntry, AgentMarkdown, AgentTimeline, AgentTimelineStore, AgentToolEntry,
        AgentToolKind, AgentToolPayload, AgentToolStatus, MarkdownSlot, TimelineRow,
        agent_pane_header,
        composer::{AgentComposer, composer_tail_clearance},
        controls::{
            AgentControlChoice, ComposerAction, agent_chrome_button, agent_config_picker,
            composer_action, composer_action_button, context_usage_meter, git_summary_footer,
        },
        fold_timeline_rows,
    },
    button::{Button, ButtonVariants as _},
    input::{InputEvent, InputState},
};

use crate::connection::Connection;

pub(super) struct AgentPane {
    pane: PaneId,
    descriptor: AgentDescriptor,
    connection: Entity<Connection>,
    connected: bool,
    input: Entity<InputState>,
    transcript: Transcript,
    timeline: Entity<AgentTimelineStore>,
    rows: Arc<Vec<TimelineRow>>,
    scroll: ListState,
    last_sequence: u64,
    last_reclaim_id: u64,
    attachments: Vec<AgentImage>,
    choosing_images: bool,
    draft_error: Option<String>,
    settings_busy: bool,
    usage: Option<(u64, u64)>,
    history_open: bool,
    history_all_projects: bool,
    history_selected: usize,
    history_delete: Option<String>,
    directory_open: bool,
    directory_input: Entity<InputState>,
    directory_selected: usize,
    picker_scroll: gpui::UniformListScrollHandle,
    history_supported: bool,
    history_input: Entity<InputState>,
    history_loading: bool,
    history_error: Option<String>,
    sessions: Vec<SessionSummary>,
    next_cursor: Option<String>,
    transcript_dirty: bool,
    local_sequence: u64,
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
                .placeholder("Ask anything…")
                .auto_grow(2, 8)
        });
        let history_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search by title or project…"));
        let history_changes = cx.subscribe(&history_input, |this: &mut Self, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.history_selected = 0;
                cx.notify();
            }
        });
        let directory_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Choose the agent's working directory")
        });
        let directory_changes = cx.subscribe(&directory_input, |this: &mut Self, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.directory_selected = 0;
                cx.notify();
            }
        });
        let observation = cx.observe(&connection, |this, connection, cx| {
            let connected = connection.read(cx).connected;
            if this.connected != connected {
                this.connected = connected;
                this.settings_busy = false;
                cx.notify();
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
                    this.usage = None;
                    this.last_sequence = 0;
                    this.rows = Arc::new(Vec::new());
                    this.scroll.reset(0);
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
                        this.receive_sessions(result);
                        cx.notify();
                    }
                } else if matches!(event,
                zz_client::CoreEvent::AgentUpdates { pane: changed, .. }
                | zz_client::CoreEvent::AgentStateChanged { pane: changed, .. } if *changed == pane)
                {
                    cx.notify();
                }
            },
        );
        let subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { shift: false }) {
                this.submit(window, cx);
            } else if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            pane,
            descriptor,
            connection,
            connected: false,
            input,
            transcript: Transcript::default(),
            timeline: cx.new(|_| AgentTimelineStore::default()),
            rows: Arc::new(Vec::new()),
            scroll: ListState::new(0, ListAlignment::Bottom, px(300.0)),
            last_sequence: 0,
            last_reclaim_id: 0,
            attachments: Vec::new(),
            choosing_images: false,
            draft_error: None,
            settings_busy: false,
            usage: None,
            history_open: false,
            history_all_projects: false,
            history_selected: 0,
            history_delete: None,
            directory_open: false,
            directory_input,
            directory_selected: 0,
            picker_scroll: gpui::UniformListScrollHandle::new(),
            history_supported: false,
            history_input,
            history_loading: false,
            history_error: None,
            sessions: Vec::new(),
            next_cursor: None,
            transcript_dirty: false,
            local_sequence: 0,
            _subscriptions: vec![
                observation,
                events,
                subscription,
                history_changes,
                directory_changes,
            ],
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let connection = self.connection.read(cx);
        let allowed = connection.connected
            && !connection.core.attached_read_only()
            && connection.core.agent_state(self.pane).is_some_and(|state| {
                matches!(
                    state.phase,
                    AgentConnectionPhase::Ready | AgentConnectionPhase::Running
                ) && state.pending_permission.is_none()
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
        self.local_sequence = self.local_sequence.wrapping_add(1);
        self.transcript
            .local_prompt(u64::MAX - self.local_sequence, &text, &self.attachments);
        self.transcript_dirty = true;
        self.attachments.clear();
        self.draft_error = None;
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn apply_control_update(&mut self, update: &Value) {
        match update.get("item").and_then(Value::as_str) {
            Some("configOptionsChanged" | "modeChanged") => self.settings_busy = false,
            Some("settingFailed") => {
                self.settings_busy = false;
                self.draft_error = update
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            Some("sessionReset" | "sessionSwitched") => {
                self.settings_busy = false;
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
            .map(|option| {
                let view = cx.entity();
                let selected = option.clone();
                agent_config_picker(
                    format!("agent-config-picker-{}-{}", self.pane.0, option.id),
                    match option.category.as_str() {
                        "mode" => IconName::Check,
                        "model" => IconName::Asterisk,
                        "thought_level" => IconName::Cpu,
                        _ => IconName::Settings,
                    },
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
        }
        let old_first = self.transcript.entries.first().map(AgentEntry::id);
        let old_count = self.rows.len();
        let following = self.scroll.is_scrolled_to_end().unwrap_or(true);
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
        for (sequence, update) in updates {
            self.restore_prompts(&update, window, cx);
            self.apply_control_update(&update);
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
        self.transcript.prune();
        self.last_sequence = newest;
        self.transcript_dirty = false;
        self.rows = fold_timeline_rows(&self.transcript.entries).rows;
        if self.transcript.entries.first().map(AgentEntry::id) != old_first
            || self.rows.len() < old_count
        {
            self.scroll.reset(self.rows.len());
        } else {
            self.scroll.remeasure_items(0..old_count);
            if self.rows.len() > old_count {
                self.scroll
                    .splice(old_count..old_count, self.rows.len() - old_count);
            }
        }
        if following {
            self.scroll.scroll_to_end();
        }
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
                    Ok(images) => {
                        let mut combined = this.attachments.clone();
                        combined.extend(images);
                        match crate::attachments::validate_images(
                            &this.input.read(cx).value(),
                            &combined,
                        ) {
                            Ok(()) => {
                                this.attachments = combined;
                                this.draft_error = None;
                            }
                            Err(error) => this.draft_error = Some(error),
                        }
                    }
                    Err(error) => this.draft_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn list_sessions(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.history_supported {
            return;
        }
        if !self.history_open {
            self.history_input
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.history_input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        }
        self.directory_open = false;
        self.history_open = true;
        self.history_delete = None;
        if replace {
            self.history_selected = 0;
        }
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
                        cwd: if self.history_all_projects {
                            None
                        } else {
                            self.descriptor.cwd.clone()
                        },
                        cursor,
                        replace,
                    },
                },
                cx,
            );
        });
        cx.notify();
    }

    fn receive_sessions(&mut self, result: &str) {
        self.history_loading = false;
        let result = serde_json::from_str::<Value>(result).unwrap_or_default();
        if result.get("item").and_then(Value::as_str) == Some("sessionDeleted") {
            self.history_delete = None;
            if let Some(id) = result.get("session_id").and_then(Value::as_str) {
                self.sessions.retain(|session| session.session_id != id);
            }
            self.history_selected = self
                .history_selected
                .min(self.sessions.len().saturating_sub(1));
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
            if let Ok(session) = serde_json::from_value::<SessionSummary>(session.clone())
                && !self
                    .sessions
                    .iter()
                    .any(|existing| existing.session_id == session.session_id)
            {
                self.sessions.push(session);
            }
        }
        self.next_cursor = result
            .get("next_cursor")
            .or_else(|| result.get("nextCursor"))
            .and_then(Value::as_str)
            .map(str::to_owned);
    }

    fn history_results(&self, cx: &App) -> Vec<usize> {
        let query = self.history_input.read(cx).value().to_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                query.is_empty()
                    || session
                        .title
                        .as_deref()
                        .unwrap_or(&session.session_id)
                        .to_lowercase()
                        .contains(&query)
                    || session
                        .cwd
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&query)
            })
            .map(|(index, _)| index)
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
        use zz_ui::agent::controls::agent_chrome_button;
        use zz_ui::command::palette_shortcut_hint;
        use zz_ui::picker::{
            history_row, picker_empty, picker_footer, picker_header, picker_list, picker_modal,
            picker_overlay, picker_search,
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
        let view = cx.entity();
        let rows = gpui::uniform_list("web-agent-history-rows", count, move |range, _, cx| {
            range
                .filter_map(|index| {
                    let session = sessions.get(*results.get(index)?)?;
                    let current = current.as_deref() == Some(session.session_id.as_str());
                    let open = view.clone();
                    let hover = view.clone();
                    let delete = view.clone();
                    let session_id = session.session_id.clone();
                    Some(
                        history_row(
                            ("web-agent-history-row", index),
                            session
                                .title
                                .clone()
                                .unwrap_or_else(|| directory_label(&session.cwd)),
                            directory_label(&session.cwd),
                            session
                                .updated_at
                                .as_deref()
                                .map(history_timestamp)
                                .map(Into::into),
                            selected == index,
                            current,
                            cx,
                        )
                        .on_mouse_move(move |_, _, cx| {
                            hover.update(cx, |this, cx| {
                                if this.history_selected != index {
                                    this.history_selected = index;
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
                        .when(can_delete && !current, |row| {
                            row.child(
                                Button::compact_icon(
                                    ("web-agent-history-delete", index),
                                    IconName::Xmark,
                                )
                                .tooltip("Delete this session")
                                .disabled(loading || !ready)
                                .on_click(move |_, _, cx| {
                                    delete.update(cx, |this, cx| {
                                        if this.session_is_open(&session_id, cx) {
                                            this.history_error =
                                                Some("This conversation is already open.".into());
                                        } else {
                                            this.history_delete = Some(session_id.clone());
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
        })
        .flex_1()
        .track_scroll(&self.picker_scroll);
        let footer = if self.history_delete.is_some() {
            picker_footer(cx)
                .min_h(px(52.0))
                .border_color(cx.theme().danger.outline())
                .bg(cx.theme().danger.fill())
                .child("Permanently delete this session from the agent’s local store?")
                .child(div().flex_1())
                .child(
                    agent_chrome_button("web-history-cancel-delete")
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.history_delete = None;
                            cx.notify();
                        })),
                )
                .child(
                    agent_chrome_button("web-history-confirm-delete")
                        .danger()
                        .label("Delete")
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.delete_history(cx))),
                )
        } else {
            picker_footer(cx)
                .when_some(self.history_error.clone(), |footer, error| {
                    footer.child(div().text_color(cx.theme().danger).child(error))
                })
                .when(loading, |footer| footer.child("Loading sessions…"))
                .when(!loading && self.history_error.is_none(), |footer| {
                    footer
                        .child(palette_shortcut_hint(["up", "down"], "select"))
                        .child(palette_shortcut_hint(["enter"], "open"))
                        .child(palette_shortcut_hint(["escape"], "close"))
                })
                .child(div().flex_1())
                .when(self.next_cursor.is_some(), |footer| {
                    footer.child(
                        agent_chrome_button("web-history-more")
                            .label("Load more")
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.list_sessions(false, window, cx);
                            })),
                    )
                })
        };
        picker_overlay("web-agent-history-overlay", cx)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_history(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                picker_modal("web-agent-history-modal", cx)
                    .child(
                        picker_header(cx)
                            .child(picker_search(&self.history_input, cx))
                            .child(
                                zz_ui::h_flex()
                                    .gap(px(zz_ui::CHROME_GAP))
                                    .child(
                                        agent_chrome_button("web-history-scope")
                                            .secondary()
                                            .icon(if self.history_all_projects {
                                                IconName::Globe
                                            } else {
                                                IconName::Folder
                                            })
                                            .label(if self.history_all_projects {
                                                "All projects"
                                            } else {
                                                "This project"
                                            })
                                            .disabled(loading)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.history_all_projects =
                                                    !this.history_all_projects;
                                                this.list_sessions(true, window, cx);
                                            })),
                                    )
                                    .child(
                                        agent_chrome_button("web-history-refresh")
                                            .icon(IconName::Redo2)
                                            .label("Refresh")
                                            .disabled(loading)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.list_sessions(true, window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(picker_list().child(rows).when(count == 0, |list| {
                        list.child(picker_empty(
                            if loading {
                                "Loading sessions…"
                            } else if self.history_input.read(cx).value().is_empty() {
                                "No sessions found for this scope."
                            } else {
                                "No sessions match that search."
                            },
                            cx,
                        ))
                    }))
                    .child(footer),
            )
            .into_any_element()
    }

    fn directory_results(&self, cx: &App) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        paths.extend(self.descriptor.cwd.clone());
        paths.extend(self.sessions.iter().map(|session| session.cwd.clone()));
        for pane in self
            .connection
            .read(cx)
            .core
            .snapshot()
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .flat_map(|window| window.panes.values())
        {
            if let zz_protocol::PaneKindSnapshot::Agent(descriptor) = &pane.kind {
                paths.extend(descriptor.cwd.clone());
            }
        }
        paths.sort();
        paths.dedup();
        let query = self.directory_input.read(cx).value().to_string();
        paths.retain(|path| {
            path.to_string_lossy()
                .to_lowercase()
                .contains(&query.to_lowercase())
        });
        let typed = PathBuf::from(query.trim());
        if typed.is_absolute() && !paths.contains(&typed) {
            paths.insert(0, typed);
        }
        paths
    }

    fn open_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_change_session(cx) {
            return;
        }
        self.history_open = false;
        self.directory_open = true;
        self.directory_selected = 0;
        self.directory_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.directory_input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    fn choose_directory(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_change_session(cx) {
            return;
        }
        let Some(cwd) = self.directory_results(cx).get(index).cloned() else {
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
        self.directory_open = false;
        self.input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn directory_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        use zz_ui::command::palette_shortcut_hint;
        use zz_ui::picker::{
            directory_row, picker_empty, picker_footer, picker_header, picker_list, picker_modal,
            picker_overlay, picker_search,
        };
        let paths = self.directory_results(cx);
        let count = paths.len();
        let selected = self.directory_selected;
        let view = cx.entity();
        let rows = gpui::uniform_list("web-directory-rows", count, move |range, _, cx| {
            range
                .filter_map(|index| {
                    let path = paths.get(index)?;
                    let click = view.clone();
                    let hover = view.clone();
                    Some(
                        directory_row(
                            ("web-directory-row", index),
                            path.to_string_lossy().into_owned(),
                            index == selected,
                            cx,
                        )
                        .on_mouse_move(move |_, _, cx| {
                            hover.update(cx, |this, cx| {
                                if this.directory_selected != index {
                                    this.directory_selected = index;
                                    cx.notify();
                                }
                            });
                        })
                        .on_click(move |_, window, cx| {
                            click.update(cx, |this, cx| this.choose_directory(index, window, cx));
                            cx.stop_propagation();
                        }),
                    )
                })
                .collect::<Vec<_>>()
        })
        .flex_1()
        .track_scroll(&self.picker_scroll);
        picker_overlay("web-directory-overlay", cx)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.directory_open = false;
                    this.input.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                    cx.stop_propagation();
                }),
            )
            .child(
                picker_modal("web-directory-modal", cx)
                    .child(picker_header(cx).child(picker_search(&self.directory_input, cx)))
                    .child(picker_list().child(rows).when(count == 0, |list| {
                        list.child(picker_empty(
                            "Enter an absolute directory on the daemon host.",
                            cx,
                        ))
                    }))
                    .child(
                        picker_footer(cx)
                            .child(palette_shortcut_hint(["up", "down"], "select"))
                            .child(palette_shortcut_hint(["enter"], "open"))
                            .child(palette_shortcut_hint(["escape"], "close")),
                    ),
            )
            .into_any_element()
    }

    fn picker_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.history_open && !self.directory_open {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                if self.history_delete.is_some() {
                    self.history_delete = None;
                } else {
                    self.directory_open = false;
                    self.close_history(window, cx);
                }
            }
            "up" | "down" if self.history_delete.is_none() => {
                let count = if self.history_open {
                    self.history_results(cx).len()
                } else {
                    self.directory_results(cx).len()
                };
                let selected = if self.history_open {
                    &mut self.history_selected
                } else {
                    &mut self.directory_selected
                };
                *selected = if event.keystroke.key == "up" {
                    selected.saturating_sub(1)
                } else {
                    (*selected + 1).min(count.saturating_sub(1))
                };
                self.picker_scroll
                    .scroll_to_item(*selected, gpui::ScrollStrategy::Nearest);
            }
            "enter" if self.history_delete.is_none() => {
                if self.history_open {
                    self.open_history_result(self.history_selected, window, cx);
                } else {
                    self.choose_directory(self.directory_selected, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn close_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_open = false;
        self.input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn permissions(&self, payload: &str, request_id: u64, cx: &mut Context<Self>) -> AnyElement {
        let writable = self.connection.read(cx).connected
            && !self.connection.read(cx).core.attached_read_only();
        let payload = serde_json::from_str::<Value>(payload).unwrap_or_default();
        let title = payload
            .pointer("/tool_call/title")
            .or_else(|| payload.pointer("/toolCall/title"))
            .and_then(Value::as_str)
            .unwrap_or("This agent needs your permission")
            .to_owned();
        let mut choices = Vec::new();
        for (index, option) in payload
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(id) = option
                .get("optionId")
                .or_else(|| option.get("id"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let id = id.to_owned();
            let label = option
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_owned();
            let connection = self.connection.clone();
            let pane = self.pane;
            choices.push(
                Button::new(("web-agent-permission", index))
                    .small()
                    .label(label)
                    .disabled(!writable)
                    .on_click(move |_, _, cx| {
                        connection.update(cx, |connection, cx| {
                            connection.send(
                                ProtocolMessage::AgentRespondPermission {
                                    pane,
                                    request_id,
                                    option_id: Some(id.clone()),
                                },
                                cx,
                            );
                        });
                    })
                    .into_any_element(),
            );
        }
        let connection = self.connection.clone();
        let pane = self.pane;
        choices.push(
            Button::new("web-agent-permission-cancel")
                .small()
                .ghost()
                .disabled(!writable)
                .label("Cancel")
                .on_click(move |_, _, cx| {
                    connection.update(cx, |connection, cx| {
                        connection.send(
                            ProtocolMessage::AgentRespondPermission {
                                pane,
                                request_id,
                                option_id: None,
                            },
                            cx,
                        );
                    });
                })
                .into_any_element(),
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .rounded(cx.theme().radius)
            .bg(cx.theme().background.raised(2))
            .child(div().text_size(px(12.0)).child(title))
            .child(div().flex().flex_wrap().gap(px(6.0)).children(choices))
            .into_any_element()
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
        let label = match self.descriptor.provider {
            zz_protocol::AgentProvider::Codex => "Codex",
            zz_protocol::AgentProvider::ClaudeCode => "Claude Code",
        };
        let icon = match self.descriptor.provider {
            zz_protocol::AgentProvider::Codex => IconName::Openai,
            zz_protocol::AgentProvider::ClaudeCode => IconName::Claude,
        };
        let mut prefix = Vec::new();
        let methods = serde_json::from_str::<Value>(&state.auth_methods).unwrap_or_default();
        for (index, method) in methods.as_array().into_iter().flatten().enumerate() {
            let Some(id) = method.get("id").and_then(Value::as_str) else {
                continue;
            };
            let id = id.to_owned();
            let name = method
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_owned();
            let connection = self.connection.clone();
            let pane = self.pane;
            prefix.push(
                Button::new(("web-agent-authenticate", index))
                    .small()
                    .label(format!("Sign in: {name}"))
                    .disabled(!writable || running)
                    .on_click(move |_, _, cx| {
                        connection.update(cx, |connection, cx| {
                            connection.send(
                                ProtocolMessage::AgentAuthenticate {
                                    pane,
                                    method_id: id.clone(),
                                },
                                cx,
                            );
                        });
                    })
                    .into_any_element(),
            );
        }
        if let Some(permission) = &state.pending_permission {
            prefix.push(self.permissions(&permission.payload, permission.request_id, cx));
        }
        if let Some(error) = &self.draft_error {
            prefix.push(
                div()
                    .text_size(px(12.0))
                    .text_color(cx.theme().danger)
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        if let AgentConnectionPhase::Failed { message } = &state.phase {
            prefix.push(
                div()
                    .text_size(px(12.0))
                    .text_color(cx.theme().danger)
                    .child(message.clone())
                    .into_any_element(),
            );
        }
        let has_content =
            !self.input.read(cx).value().trim().is_empty() || !self.attachments.is_empty();
        let action_kind = composer_action(running, has_content);
        let action = composer_action_button(
            ("agent-action", self.pane.0),
            action_kind,
            if action_kind == ComposerAction::Stop {
                writable
            } else {
                ready && state.pending_permission.is_none() && has_content
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
        settings.extend(self.render_config_controls(&state, cx));
        let queued = (state.queued_prompts > 0).then(|| {
            agent_chrome_button("web-agent-restore-queue")
                .label(format!("{} queued", state.queued_prompts))
                .tooltip("Restore queued prompts to draft")
                .disabled(!writable)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.connection.update(cx, |connection, cx| {
                        connection.send(ProtocolMessage::AgentUnqueue { pane: this.pane }, cx);
                    });
                }))
                .into_any_element()
        });
        let usage = self.usage.map(|(used, size)| {
            context_usage_meter(("agent-context-usage", self.pane.0), used, size, cx)
        });
        let usage = if queued.is_some() || usage.is_some() {
            Some(
                zz_ui::h_flex()
                    .items_center()
                    .gap_1()
                    .children(queued)
                    .children(usage)
                    .into_any_element(),
            )
        } else {
            None
        };
        let composer = AgentComposer {
            input: self.input.clone(),
            action: action.into_any_element(),
            settings,
            usage,
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
            directory: agent_chrome_button("web-agent-directory")
                .icon(IconName::Folder)
                .label(self.descriptor.cwd.as_ref().map_or_else(
                    || "Daemon working directory".to_owned(),
                    |path| directory_label(path),
                ))
                .disabled(!self.can_change_session(cx))
                .tooltip(self.descriptor.cwd.as_ref().map_or_else(
                    || "Agent working directory".to_owned(),
                    |path| path.to_string_lossy().into_owned(),
                ))
                .on_click(cx.listener(|this, _, window, cx| this.open_directory(window, cx)))
                .into_any_element(),
            command_hint: None,
            prefix,
            attachments: (!self.attachments.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .p(px(8.0))
                    .children(self.attachments.iter().enumerate().map(|(index, image)| {
                        let preview = preview_image(image);
                        div()
                            .relative()
                            .w(px(72.0))
                            .h(px(64.0))
                            .children(preview.map(|preview| {
                                let expanded = preview.clone();
                                div()
                                    .id(("web-agent-image", index))
                                    .size_full()
                                    .cursor_pointer()
                                    .child(
                                        gpui::img(preview)
                                            .size_full()
                                            .object_fit(gpui::ObjectFit::Contain),
                                    )
                                    .on_click(move |_, window, cx| {
                                        zz_ui::attachment::open_attachment_preview(
                                            expanded.clone(),
                                            window,
                                            cx,
                                        );
                                    })
                            }))
                            .child(
                                div().absolute().right_0().top_0().child(
                                    Button::compact_icon(
                                        ("web-agent-remove-image", index),
                                        IconName::Xmark,
                                    )
                                    .tooltip("Remove image")
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if index < this.attachments.len() {
                                                this.attachments.remove(index);
                                            }
                                            this.draft_error = None;
                                            cx.notify();
                                        },
                                    )),
                                ),
                            )
                    }))
                    .into_any_element()
            }),
            radii: Corners::all(px(0.0)),
            background: cx.theme().background.opaque(),
        };
        let pane = self.pane;
        let connection = self.connection.clone();
        let cwd = self.descriptor.cwd.clone();
        let new_session = Button::compact_icon("web-agent-new-session", IconName::ChatPlus)
            .tooltip("New conversation")
            .disabled(!ready || running || cwd.is_none())
            .on_click(move |_, _, cx| {
                if let Some(cwd) = cwd.clone() {
                    connection.update(cx, |connection, cx| {
                        connection.send(
                            ProtocolMessage::AgentSessionOp {
                                pane,
                                op: AgentSessionOpKind::New { cwd },
                            },
                            cx,
                        );
                    });
                }
            });
        let history = Button::compact_icon("web-agent-history-button", IconName::Clock)
            .tooltip(if self.history_supported {
                "Conversation history"
            } else {
                "This agent does not support conversation history"
            })
            .disabled(!ready || running || !self.history_supported)
            .on_click(cx.listener(|this, _, window, cx| this.list_sessions(true, window, cx)));
        let _ = window;
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background.opaque())
            .child(agent_pane_header(
                Button::new("web-agent-provider")
                    .ghost()
                    .small()
                    .icon(icon)
                    .label(label)
                    .disabled(true),
                div().flex().gap(px(4.0)).child(new_session).child(history),
                cx,
            ))
            .child(
                div()
                    .id("web-agent-timeline")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        AgentTimeline::new(
                            self.rows.clone(),
                            self.scroll.clone(),
                            self.timeline.clone(),
                        )
                        .active_turn(running)
                        .bottom_padding(composer_tail_clearance()),
                    ),
            )
            .child(composer)
            .when(self.history_open, |pane| {
                pane.child(self.history(ready && !running, cx))
            })
            .when(self.directory_open, |pane| {
                pane.child(self.directory_picker(cx))
            })
            .capture_key_down(cx.listener(Self::picker_key_down))
    }
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummary {
    session_id: String,
    cwd: PathBuf,
    #[serde(default)]
    additional_directories: Vec<PathBuf>,
    title: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

struct LocalPrompt {
    text: String,
    echoed: usize,
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

#[derive(Default)]
struct Transcript {
    entries: Vec<AgentEntry>,
    messages: HashMap<String, usize>,
    tools: HashMap<String, usize>,
    current: Option<(String, usize)>,
    generation: u64,
    local_prompts: VecDeque<LocalPrompt>,
    session_reset: bool,
}

impl Transcript {
    fn local_prompt(&mut self, id: u64, text: &str, images: &[AgentImage]) {
        self.entries.push(AgentEntry::User {
            id,
            markdown: AgentMarkdown::from(text),
            images: images
                .iter()
                .filter_map(preview_image)
                .collect::<Vec<_>>()
                .into(),
        });
        self.current = None;
        self.local_prompts.push_back(LocalPrompt {
            text: text.to_owned(),
            echoed: 0,
        });
    }

    fn prune(&mut self) {
        let remove = self.entries.len().saturating_sub(500);
        if remove == 0 {
            return;
        }
        self.entries.drain(..remove);
        self.messages.retain(|_, index| {
            if *index < remove {
                false
            } else {
                *index -= remove;
                true
            }
        });
        self.tools.retain(|_, index| {
            if *index < remove {
                false
            } else {
                *index -= remove;
                true
            }
        });
        self.current = self
            .current
            .take()
            .and_then(|(kind, index)| index.checked_sub(remove).map(|index| (kind, index)));
    }

    fn apply(&mut self, sequence: u64, item: &Value) {
        match item.get("item").and_then(Value::as_str).unwrap_or_default() {
            "sessionReset" => {
                *self = Self::default();
                self.session_reset = true;
            }
            "sessionReady" => self.session_reset = false,
            "sessionSwitched" => {
                if !self.session_reset {
                    *self = Self::default();
                }
                for (index, update) in item
                    .get("replay")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    self.update(
                        sequence.saturating_mul(1024).saturating_add(index as u64),
                        update,
                    );
                }
                self.session_reset = false;
                self.current = None;
            }
            "update" => {
                if let Some(update) = item.get("update") {
                    self.update(sequence, update);
                }
            }
            "turnStarted" | "promptFinished" => {
                if item.get("item").and_then(Value::as_str) == Some("promptFinished") {
                    self.local_prompts.pop_front();
                }
                self.current = None;
                self.generation = self.generation.wrapping_add(1);
            }
            _ => {}
        }
    }

    fn update(&mut self, sequence: u64, update: &Value) {
        let kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "agent_message_chunk" | "user_message_chunk" | "agent_thought_chunk" => {
                let Some(text) = update.pointer("/content/text").and_then(Value::as_str) else {
                    return;
                };
                if kind == "user_message_chunk"
                    && let Some(local) = self.local_prompts.front_mut()
                    && local
                        .text
                        .get(local.echoed..)
                        .is_some_and(|remaining| remaining.starts_with(text))
                {
                    local.echoed += text.len();
                    return;
                }
                let key = update
                    .get("messageId")
                    .and_then(Value::as_str)
                    .map(|id| format!("{}:{kind}:{id}", self.generation));
                let previous = key
                    .as_ref()
                    .and_then(|key| self.messages.get(key).copied())
                    .or_else(|| {
                        key.is_none()
                            .then(|| {
                                self.current
                                    .as_ref()
                                    .filter(|(previous, _)| previous == kind)
                                    .map(|(_, index)| *index)
                            })
                            .flatten()
                    });
                if let Some(index) = previous {
                    let (AgentEntry::User { markdown, .. }
                    | AgentEntry::Assistant { markdown, .. }
                    | AgentEntry::Reasoning { markdown, .. }) = &self.entries[index]
                    else {
                        return;
                    };
                    let mut combined = markdown.full_text();
                    if combined.len() >= 256 * 1024 {
                        return;
                    }
                    combined.push_str(text);
                    let mut end = combined.len().min(256 * 1024);
                    while !combined.is_char_boundary(end) {
                        end -= 1;
                    }
                    combined.truncate(end);
                    markdown.synchronize_append(&combined);
                    return;
                }
                let markdown = AgentMarkdown::from(text);
                let entry = match kind {
                    "user_message_chunk" => AgentEntry::User {
                        id: sequence,
                        markdown,
                        images: Arc::from([]),
                    },
                    "agent_thought_chunk" => AgentEntry::Reasoning {
                        id: sequence,
                        label: "Thinking".into(),
                        markdown,
                        default_expanded: false,
                    },
                    _ => AgentEntry::Assistant {
                        id: sequence,
                        markdown,
                    },
                };
                let index = self.entries.len();
                self.entries.push(entry);
                if let Some(key) = key {
                    self.messages.insert(key, index);
                }
                self.current = Some((kind.to_owned(), index));
            }
            "tool_call" | "tool_call_update" => {
                self.current = None;
                let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
                    return;
                };
                let index = *self.tools.entry(id.to_owned()).or_insert_with(|| {
                    let index = self.entries.len();
                    self.entries.push(AgentEntry::Tool(AgentToolEntry {
                        id: sequence,
                        kind: AgentToolKind::Other,
                        status: AgentToolStatus::Pending,
                        label: "Tool call".into(),
                        location: None,
                        input: None,
                        output: Arc::from([]),
                        default_expanded: false,
                    }));
                    index
                });
                let AgentEntry::Tool(tool) = &mut self.entries[index] else {
                    return;
                };
                if let Some(title) = update.get("title").and_then(Value::as_str) {
                    tool.label = title.to_owned().into();
                }
                if let Some(kind) = update.get("kind").and_then(Value::as_str) {
                    tool.kind = match kind {
                        "read" => AgentToolKind::Read,
                        "search" => AgentToolKind::Search,
                        "edit" => AgentToolKind::Edit,
                        "execute" => AgentToolKind::Execute,
                        "fetch" => AgentToolKind::Fetch,
                        "think" => AgentToolKind::Think,
                        _ => AgentToolKind::Other,
                    };
                }
                if let Some(status) = update.get("status").and_then(Value::as_str) {
                    tool.status = match status {
                        "in_progress" => AgentToolStatus::Running,
                        "completed" => AgentToolStatus::Completed,
                        "failed" => AgentToolStatus::Failed,
                        _ => AgentToolStatus::Pending,
                    };
                }
                if let Some(locations) = update.get("locations") {
                    tool.location = locations
                        .get(0)
                        .and_then(|location| location.get("path"))
                        .and_then(Value::as_str)
                        .map(|path| path.to_owned().into());
                }
                if let Some(input) = update.get("rawInput") {
                    tool.input = Some(AgentToolPayload::Json(
                        serde_json::to_string_pretty(input)
                            .unwrap_or_default()
                            .into(),
                    ));
                }
                if let Some(content) = update.get("content").and_then(Value::as_array) {
                    tool.output = content
                        .iter()
                        .filter_map(|content| {
                            if content.get("type").and_then(Value::as_str) == Some("diff") {
                                Some(AgentToolPayload::Diff {
                                    path: content
                                        .get("path")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_owned()
                                        .into(),
                                    old: content
                                        .get("oldText")
                                        .and_then(Value::as_str)
                                        .map(Into::into),
                                    new: content
                                        .get("newText")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .into(),
                                })
                            } else {
                                content
                                    .pointer("/content/text")
                                    .and_then(Value::as_str)
                                    .map(|text| AgentToolPayload::Text(text.into()))
                            }
                        })
                        .collect::<Vec<_>>()
                        .into();
                }
            }
            "plan" => {
                let markdown = update
                    .get("entries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|entry| {
                        let checked =
                            entry.get("status").and_then(Value::as_str) == Some("completed");
                        format!(
                            "- [{}] {}",
                            if checked { "x" } else { " " },
                            entry
                                .get("content")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.entries.push(AgentEntry::Plan {
                    id: sequence,
                    markdown: markdown.into(),
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        transcript.local_prompt(u64::MAX, "hello world", &[]);
        assert_eq!(transcript.entries.len(), 1);
        transcript.apply(1, &json!({"item":"turnStarted"}));
        for (sequence, text) in [(2, "hello"), (3, " world")] {
            transcript.apply(sequence, &json!({"item":"update","update":{"sessionUpdate":"user_message_chunk","content":{"text":text}}}));
        }
        assert_eq!(transcript.entries.len(), 1);
        transcript.apply(4, &json!({"item":"promptFinished"}));
        transcript.apply(5, &json!({"item":"update","update":{"sessionUpdate":"user_message_chunk","content":{"text":"another client"}}}));
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
            transcript.apply(sequence, &json!({"item":"update","update":{"sessionUpdate":"agent_message_chunk","messageId":"a","content":{"text":text}}}));
        }
        assert_eq!(transcript.entries.len(), 1);
        let AgentEntry::Assistant { markdown, .. } = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(markdown.full_text(), "hello world");
        transcript.apply(3, &json!({"item":"promptFinished"}));
        transcript.apply(4, &json!({"item":"update","update":{"sessionUpdate":"agent_message_chunk","messageId":"a","content":{"text":"next"}}}));
        assert_eq!(transcript.entries.len(), 2);
    }

    #[test]
    fn session_switch_preserves_streamed_history_and_replaces_a_previous_session() {
        let mut transcript = Transcript::default();
        transcript.apply(1, &json!({"item":"sessionReset","restoring":true}));
        transcript.apply(2, &json!({"item":"update","update":{"sessionUpdate":"agent_message_chunk","content":{"text":"restored"}}}));
        transcript.apply(3, &json!({"item":"sessionSwitched","replay":[]}));
        let AgentEntry::Assistant { markdown, .. } = &transcript.entries[0] else {
            panic!()
        };
        assert_eq!(markdown.full_text(), "restored");
        transcript.apply(4, &json!({"item":"sessionSwitched","replay":[{"sessionUpdate":"agent_message_chunk","content":{"text":"another session"}}]}));
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
        assert!(transcript.tools.is_empty());
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
        if !date.get_time().is_finite() {
            return value.into();
        }
        let now = js_sys::Date::new_0();
        let time = format!("{:02}:{:02}", date.get_hours(), date.get_minutes());
        let yesterday = js_sys::Date::new_0();
        yesterday.set_date(now.get_date().saturating_sub(1));
        let same_day = |other: &js_sys::Date| {
            date.get_full_year() == other.get_full_year()
                && date.get_month() == other.get_month()
                && date.get_date() == other.get_date()
        };
        if same_day(&now) {
            return format!("Today, {time}");
        }
        if same_day(&yesterday) {
            return format!("Yesterday, {time}");
        }
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let month = months[date.get_month() as usize];
        if date.get_full_year() == now.get_full_year() {
            format!("{month} {}, {time}", date.get_date())
        } else {
            format!(
                "{month} {}, {}, {time}",
                date.get_date(),
                date.get_full_year()
            )
        }
    }
    #[cfg(not(target_family = "wasm"))]
    value.into()
}
