use std::hash::{DefaultHasher, Hash, Hasher};

use zz_protocol::{
    AttachOperation, Batch, ClientView, Hello, KeySubscription, MouseBindings, TreeDelta,
    TreeSubscription, Welcome,
};

use super::*;

thread_local! {
    pub(super) static HOOK_NOTIFICATIONS_ONLY: Cell<bool> = const { Cell::new(false) };
}

pub(super) struct HookNotificationsOnlyScope(bool);

impl HookNotificationsOnlyScope {
    pub(super) fn new(enabled: bool) -> Self {
        Self(HOOK_NOTIFICATIONS_ONLY.with(|slot| slot.replace(slot.get() || enabled)))
    }
}

impl Drop for HookNotificationsOnlyScope {
    fn drop(&mut self) {
        HOOK_NOTIFICATIONS_ONLY.with(|slot| slot.set(self.0));
    }
}

static TREE_DELTA: LazyLock<bool> = LazyLock::new(|| {
    let enabled = std::env::var_os("ZZ_PERF_TREE_DELTA").is_none_or(|value| value != "0");
    if !enabled {
        log::info!("ZZ_PERF_TREE_DELTA=0: publish full subscribed trees");
    }
    enabled
});

pub(super) fn options_event(
    inner: &ServerState,
    client: ClientId,
    options: MuxOptions,
) -> Option<EventPayload> {
    let Some(subscription) = inner.ctrl_subscriptions.get(&client) else {
        return Some(EventPayload::MuxOptionsChanged { options });
    };
    (subscription.options != 0).then(|| EventPayload::MuxOptionsPatched {
        options: MuxOptions::from_entries(
            options
                .iter()
                .filter(|(key, _)| subscription.includes_option(*key))
                .map(|(key, value)| (key, value.clone())),
        ),
    })
}

fn scope(inner: &ServerState, client: ClientId) -> Option<(u8, Option<SessionId>)> {
    match inner.ctrl_subscriptions.get(&client)?.tree {
        TreeSubscription::None => None,
        TreeSubscription::Attached => Some((1, client_attached_session(inner, client))),
        TreeSubscription::All => Some((2, None)),
    }
}

fn scoped_snapshot(snapshot: &MuxSnapshot, scope: (u8, Option<SessionId>)) -> MuxSnapshot {
    let mut tree = snapshot.clone();
    tree.focused_window = None;
    if scope.0 == 1 {
        tree.sessions.retain(|session| Some(session.id) == scope.1);
    }
    tree
}

fn layout_generation(inner: &mut ServerState, client: ClientId) -> u64 {
    let window = client_focused_window_for_attachment(inner, client);
    let mut hash = DefaultHasher::new();
    window.hash(&mut hash);
    if let Some(window) = window.and_then(|window| inner.engine.state.windows.get(&window)) {
        window.layout.dump().hash(&mut hash);
        window.zoomed_pane.hash(&mut hash);
    }
    let digest = hash.finish();
    let state = inner.ctrl_layouts.entry(client).or_insert((digest, 1));
    if state.0 != digest {
        *state = (digest, state.1.saturating_add(1));
    }
    state.1
}

pub(super) fn normalize_resize(
    inner: &mut ServerState,
    client: ClientId,
    input: InputMessage,
) -> Option<InputMessage> {
    let reported = match &input {
        InputMessage::ResizeTerminalV2 {
            layout_generation, ..
        }
        | InputMessage::ClientTerminalSizeV2 {
            layout_generation, ..
        } => Some(*layout_generation),
        _ => None,
    };
    if let Some(reported) = reported
        && inner.ctrl_subscriptions.contains_key(&client)
        && reported != layout_generation(inner, client)
    {
        return None;
    }
    Some(match input {
        InputMessage::ResizeTerminalV2 {
            pane,
            columns,
            rows,
            cell_width_px,
            cell_height_px,
            ..
        } => InputMessage::ResizeTerminal {
            pane,
            columns,
            rows,
            cell_width_px,
            cell_height_px,
        },
        InputMessage::ClientTerminalSizeV2 { columns, rows, .. } => {
            InputMessage::ClientTerminalSize { columns, rows }
        }
        input => input,
    })
}

fn shared_tree(
    inner: &ServerState,
    snapshot: &MuxSnapshot,
    scope: (u8, Option<SessionId>),
    facts: &FormatHookFacts,
) -> MuxSnapshot {
    let mut tree = scoped_snapshot(snapshot, scope);
    let presence = snapshot_presence(inner);
    for session in &mut tree.sessions {
        session.viewers = presence
            .get(&session.id)
            .into_iter()
            .flatten()
            .map(|(_, viewer)| viewer.clone())
            .collect();
    }
    let contexts = inner.engine.format_context_snapshot(FormatClient::NoClient);
    expand_window_status_labels(
        &inner.engine,
        &inner.config_files,
        facts,
        &contexts,
        &mut tree,
    );
    stamp_pane_border_colours(
        &inner.engine,
        &inner.config_files,
        facts,
        &contexts,
        &mut tree,
    );
    stamp_pane_border_chrome(
        &inner.engine,
        &inner.config_files,
        facts,
        &contexts,
        &mut tree,
    );
    drop(contexts);
    stamp_pane_modes(inner, facts, &mut tree);
    tree
}

fn client_view(
    inner: &mut ServerState,
    client: ClientId,
    raw: &MuxSnapshot,
    facts: &FormatHookFacts,
) -> ClientView {
    let layout_generation = layout_generation(inner, client);
    let mut overlay = Vec::new();
    let contexts = (inner.client_kinds.get(&client) != Some(&ClientKind::Control)).then(|| {
        inner.engine.format_context_snapshot(
            client_attached_session(inner, client)
                .map_or(FormatClient::Unattached, FormatClient::Attached),
        )
    });
    let presence = snapshot_presence(inner);
    let engine = &inner.engine;
    for session in &raw.sessions {
        let viewers = presence
            .get(&session.id)
            .into_iter()
            .flatten()
            .map(|(viewer_client, viewer)| SessionViewer {
                is_self: *viewer_client == client,
                ..viewer.clone()
            })
            .collect::<Vec<_>>();
        if viewers != session.viewers {
            overlay.push(zz_protocol::TreeOp::SessionViewers {
                session: session.id,
                viewers,
            });
        }
        let Some(contexts) = contexts.as_ref() else {
            continue;
        };
        for window in &session.windows {
            let formats = engine.window_status_formats(window.id);
            let format = if window.id == session.active_window {
                &formats.current_format
            } else {
                &formats.format
            };
            let mut context = contexts.status_context(
                Some(session.id),
                Some(window.id),
                Some(window.active_pane),
            );
            inner.config_files.clone_into(&mut context.config_files);
            let mut hooks = DaemonFormatHooks::command(facts).with_option_engine(engine);
            let style = expand_window_status_style(&formats, &context, &mut hooks);
            let label = expand_status(format, &context, &mut hooks);
            let status_label =
                truncate_window_status_label(format!("#[{style}]#[push-default]{label}"));
            if status_label != window.status_label {
                overlay.push(zz_protocol::TreeOp::WindowPresentation {
                    session: session.id,
                    window: window.id,
                    status_label,
                    pane_border_status: window.pane_border_status,
                    pane_border_lines: window.pane_border_lines,
                    pane_border_indicators: window.pane_border_indicators,
                });
            }
            if !engine.has_pane_border_style_settings() && !window.pane_border_status.is_on() {
                continue;
            }
            for (pane, entry) in &window.panes {
                let mut context =
                    contexts.status_context(Some(session.id), Some(window.id), Some(*pane));
                inner.config_files.clone_into(&mut context.config_files);
                let mut hooks = DaemonFormatHooks::command(facts).with_option_engine(engine);
                let mut border_colour = entry.border_colour;
                let mut active_border_colour = entry.active_border_colour;
                if engine.has_pane_border_style_settings() {
                    let styles = engine.pane_border_style_values(*pane);
                    let mut colour = |value: Option<String>| {
                        let value = value?;
                        let expanded = if value.contains("#{") {
                            expand_format_values(&value, &context, &mut hooks)
                        } else {
                            value
                        };
                        zz_protocol::parse_style(&expanded)?.fg
                    };
                    border_colour = colour(styles.border);
                    active_border_colour = colour(styles.active_border);
                }
                let border_status_text = if window.pane_border_status.is_on() {
                    expand_status(&engine.pane_border_format(*pane), &context, &mut hooks)
                } else {
                    String::new()
                };
                if border_colour != entry.border_colour
                    || active_border_colour != entry.active_border_colour
                    || border_status_text != entry.border_status_text
                {
                    overlay.push(zz_protocol::TreeOp::PanePresentation {
                        session: session.id,
                        window: window.id,
                        pane: *pane,
                        border_colour,
                        active_border_colour,
                        border_status_text,
                        mode: entry.mode.clone(),
                    });
                }
            }
        }
    }
    let flags = inner.client_flags.get(client);
    ClientView {
        session: client_attached_session(inner, client),
        focused_window: client_focused_window_for_attachment(inner, client),
        read_only: flags.read_only,
        client_flags: flags.reconnect_flags(),
        layout_generation,
        attachment_generation: inner.ctrl_attachments.get(&client).copied().unwrap_or(0),
        overlay,
    }
}

impl OutboundMailbox {
    pub(super) fn enqueue_control_group(&self, frames: Vec<Vec<u8>>) -> bool {
        let message = ProtocolMessage::Batch(Batch {
            sequence: Shared::next_sequence(),
            frames,
        });
        let encoded = match self.encode_message(&message) {
            Ok(encoded) => encoded,
            Err(error) => {
                log::error!("failed to encode subscribed tree group: {error}");
                return false;
            }
        };
        let ProtocolMessage::Batch(Batch { frames, .. }) = message else {
            unreachable!()
        };
        self.enqueue_encoded_reliable(OutboundFrame::Grouped { encoded, frames })
    }

    pub(super) fn flush_control_batch(&self, preserve_welcome: bool) -> bool {
        let mut state = self.state.lock();
        let welcome = preserve_welcome
            .then(|| state.reliable.pop_front())
            .flatten();
        state.attach_batch = false;
        state.ctrl_collecting = false;
        state.terminals_held = false;
        let mut frames = Vec::new();
        while let Some(frame) = pop_ready_frame(&mut state) {
            match frame {
                OutboundFrame::Grouped {
                    frames: children, ..
                } => frames.extend(children),
                frame => frames.push(frame.into_vec()),
            }
        }
        let message = ProtocolMessage::Batch(Batch {
            sequence: Shared::next_sequence(),
            frames,
        });
        let mut encoded = Vec::new();
        let result = encode_protocol_message_into(&message, &mut encoded);
        if let Some(welcome) = welcome {
            state.reliable.push_back(welcome);
        }
        if result.is_err() || !reserve_outbound_bytes(&mut state, encoded.len(), 0) {
            close_outbound_too_far_behind(&mut state);
            drop(state);
            self.ready.notify_all();
            return false;
        }
        state.queued_bytes += encoded.len();
        let ProtocolMessage::Batch(Batch { frames, .. }) = message else {
            unreachable!()
        };
        state
            .reliable
            .push_back(OutboundFrame::Grouped { encoded, frames });
        drop(state);
        self.ready.notify_one();
        true
    }
}

pub(super) fn control_query_can_defer_wakeup(
    inner: &ServerState,
    context: &ExecutionContext,
    command: &PreparedCommand,
) -> bool {
    let Some(name) = command.canonical_name.as_deref() else {
        return false;
    };
    if !hook_events::command_is_read_only(name, &command.invocation.args)
        || !inner.deferred_event_hooks.is_empty()
    {
        return false;
    }
    ["command-error", &format!("after-{name}")]
        .into_iter()
        .all(|hook| {
            inner
                .engine
                .hook_commands(context.session, hook)
                .is_none_or(|commands| commands.is_empty())
        })
}

impl Shared {
    pub(super) fn register_welcome(&self, hello: &Hello) -> Option<(ClientId, Welcome)> {
        let client_hello = &hello.client;
        let client = self.register_identity(
            client_hello.kind,
            client_hello.client_instance_id,
            client_hello.device_name.clone(),
            client_hello.color_scheme,
            client_hello.kind == ClientKind::Interactive
                && client_hello
                    .capabilities
                    .iter()
                    .any(|capability| capability == ClientHello::CLIENT_TERMINAL_CAPABILITY),
            false,
        )?;
        let mut inner = self.inner.lock();
        if hello.subscriptions.keys == KeySubscription::Full
            && !inner.subscribers.keys().any(|client| {
                inner
                    .ctrl_subscriptions
                    .get(client)
                    .is_none_or(|subscription| subscription.keys == KeySubscription::Full)
            })
        {
            inner.key_table_generations = inner
                .engine
                .keys
                .table_generations()
                .map(|(name, generation)| (name.to_owned(), generation))
                .collect();
            if !inner.subscribers.keys().any(|client| {
                inner
                    .ctrl_subscriptions
                    .get(client)
                    .is_some_and(|subscription| subscription.keys == KeySubscription::Hash)
            }) {
                inner.key_tables_generation = inner.engine.keys.generation();
            }
        }
        inner.ctrl_subscriptions.insert(client, hello.subscriptions);
        inner.ctrl_initializing.insert(client);
        Some((
            client,
            Welcome {
                protocol_version: PROTOCOL_VERSION,
                server_id: self.server_id,
                client_id: client,
                client_instance_id: client_hello.client_instance_id,
                caps: (1 << zz_protocol::CAPABILITY_NAMES.len()) - 1,
            },
        ))
    }

    pub(super) fn compact_tree_messages(
        &self,
        client: ClientId,
        force: bool,
    ) -> Vec<ProtocolMessage> {
        let mut inner = self.inner.lock();
        let snapshot = inner.engine.state.snapshot();
        let facts = format_hook_facts(&inner);
        let mut messages = Vec::new();
        let raw = if let Some(scope) = scope(&inner, client) {
            let mut raw = shared_tree(&inner, &snapshot, scope, &facts);
            let before = inner.ctrl_trees.get(&scope).cloned().unwrap_or_default();
            raw.generation = before.generation.saturating_add(1);
            let delta = TreeDelta::between(&before, &raw);
            if delta.ops.is_empty() {
                raw.generation = before.generation;
            }
            let known = inner.ctrl_tree_versions.get(&client).copied();
            if force || known != Some(raw.generation) {
                let tree = if *TREE_DELTA {
                    if force || known != Some(before.generation) {
                        EventPayload::Snapshot(raw.clone())
                    } else {
                        EventPayload::TreeDelta(delta)
                    }
                } else {
                    EventPayload::Snapshot(raw.clone())
                };
                messages.push(Self::event(tree));
                inner.ctrl_tree_versions.insert(client, raw.generation);
            }
            inner.ctrl_trees.insert(scope, raw.clone());
            raw
        } else {
            MuxSnapshot {
                generation: 0,
                sessions: Vec::new(),
                focused_window: None,
            }
        };
        let view = client_view(&mut inner, client, &raw, &facts);
        if force || inner.ctrl_views.get(&client) != Some(&view) {
            inner.ctrl_views.insert(client, view.clone());
            messages.push(Self::event(EventPayload::ClientView(view)));
        }
        messages
    }

    pub(super) fn send_compact_state(
        &self,
        client: ClientId,
        outbound: &OutboundMailbox,
        force: bool,
    ) {
        let _order = self.snapshot_order.lock();
        let frames = self
            .compact_tree_messages(client, force)
            .into_iter()
            .map(|message| zz_protocol::encode_protocol_message(&message))
            .collect::<Result<Vec<_>, _>>();
        match frames {
            Ok(frames) if !frames.is_empty() => {
                let _ = outbound.enqueue_control_group(frames);
            }
            Ok(_) => {}
            Err(error) => log::error!("failed to encode subscribed tree state: {error}"),
        }
    }

    pub(super) fn publish_compact_trees(&self) {
        if self.shutdown_pending.load(Ordering::Acquire) || self.stopping.load(Ordering::Acquire) {
            return;
        }
        let sends = {
            let mut inner = self.inner.lock();
            if inner.ctrl_subscriptions.is_empty() {
                return;
            }
            let snapshot = inner.engine.state.snapshot();
            let facts = format_hook_facts(&inner);
            let clients = inner
                .ctrl_subscriptions
                .keys()
                .copied()
                .filter(|client| {
                    inner.subscribers.contains_key(client)
                        && !inner.ctrl_initializing.contains(client)
                })
                .collect::<Vec<_>>();
            let mut groups = BTreeMap::<(u8, Option<SessionId>), Vec<ClientId>>::new();
            let mut view_only = Vec::new();
            for client in clients {
                if let Some(scope) = scope(&inner, client) {
                    groups.entry(scope).or_default().push(client);
                } else {
                    view_only.push(client);
                }
            }
            let mut sends = BTreeMap::<ClientId, (Arc<OutboundMailbox>, Vec<Vec<u8>>)>::new();
            for (scope, clients) in groups {
                let before = inner.ctrl_trees.get(&scope).cloned().unwrap_or_default();
                let mut raw = shared_tree(&inner, &snapshot, scope, &facts);
                raw.generation = before.generation.saturating_add(1);
                let delta = TreeDelta::between(&before, &raw);
                if delta.ops.is_empty() {
                    raw.generation = before.generation;
                }
                let shared_delta = (!delta.ops.is_empty())
                    .then(|| {
                        if *TREE_DELTA {
                            Self::event(EventPayload::TreeDelta(delta))
                        } else {
                            Self::event(EventPayload::Snapshot(raw.clone()))
                        }
                    })
                    .and_then(|message| zz_protocol::encode_protocol_message(&message).ok())
                    .map(Arc::<[u8]>::from);
                let mut shared_full = None;
                for client in clients {
                    let outbound = Arc::clone(&inner.subscribers[&client]);
                    let known = inner.ctrl_tree_versions.get(&client).copied();
                    if known != Some(raw.generation) {
                        let encoded = if known == Some(before.generation) {
                            shared_delta.clone()
                        } else {
                            if shared_full.is_none() {
                                shared_full = zz_protocol::encode_protocol_message(&Self::event(
                                    EventPayload::Snapshot(raw.clone()),
                                ))
                                .ok()
                                .map(Arc::<[u8]>::from);
                            }
                            shared_full.clone()
                        };
                        if let Some(encoded) = encoded {
                            sends
                                .entry(client)
                                .or_insert_with(|| (Arc::clone(&outbound), Vec::new()))
                                .1
                                .push(encoded.to_vec());
                        }
                        inner.ctrl_tree_versions.insert(client, raw.generation);
                    }
                    let view = client_view(&mut inner, client, &raw, &facts);
                    if inner.ctrl_views.get(&client) != Some(&view) {
                        inner.ctrl_views.insert(client, view.clone());
                        if let Ok(encoded) = zz_protocol::encode_protocol_message(&Self::event(
                            EventPayload::ClientView(view),
                        )) {
                            sends
                                .entry(client)
                                .or_insert_with(|| (outbound, Vec::new()))
                                .1
                                .push(encoded);
                        }
                    }
                }
                inner.ctrl_trees.insert(scope, raw);
            }
            for client in view_only {
                let view = client_view(&mut inner, client, &MuxSnapshot::default(), &facts);
                if inner.ctrl_views.get(&client) != Some(&view) {
                    inner.ctrl_views.insert(client, view.clone());
                    if let Ok(encoded) = zz_protocol::encode_protocol_message(&Self::event(
                        EventPayload::ClientView(view),
                    )) {
                        sends
                            .entry(client)
                            .or_insert_with(|| {
                                (Arc::clone(&inner.subscribers[&client]), Vec::new())
                            })
                            .1
                            .push(encoded);
                    }
                }
            }
            sends
        };
        for (_, (outbound, frames)) in sends {
            let _ = outbound.enqueue_control_group(frames);
        }
    }

    pub(super) fn publish_key_table_changes(&self, payload: EventPayload) {
        let (full, hash, hash_payload) = {
            let inner = self.inner.lock();
            let mut full = Vec::new();
            let mut hash = Vec::new();
            for (client, outbound) in &inner.subscribers {
                match inner
                    .ctrl_subscriptions
                    .get(client)
                    .map_or(KeySubscription::Full, |subscription| subscription.keys)
                {
                    KeySubscription::Full => full.push(Arc::clone(outbound)),
                    KeySubscription::Hash => hash.push(Arc::clone(outbound)),
                    KeySubscription::None => {}
                }
            }
            let hash_payload = (!hash.is_empty()).then(|| EventPayload::KeyTablesHashChanged {
                hash: inner.engine.keys.generation(),
                mouse: MouseBindings::from_key_tables(&inner.engine.keys),
            });
            (full, hash, hash_payload)
        };
        let full_payload = (!matches!(&payload, EventPayload::KeyTablesPatched { tables, removed } if tables.is_empty() && removed.is_empty())).then_some(payload);
        for (outbounds, payload) in [(full, full_payload), (hash, hash_payload)] {
            let Some(payload) = payload else {
                continue;
            };
            if outbounds.is_empty() {
                continue;
            }
            let Ok(frame) =
                zz_protocol::encode_protocol_message(&Self::event(payload)).map(Arc::<[u8]>::from)
            else {
                continue;
            };
            for outbound in outbounds {
                let _ = outbound.enqueue_encoded_reliable(Arc::clone(&frame));
            }
        }
    }

    pub(super) fn send_compact_keys(&self, client: ClientId, outbound: &OutboundMailbox) {
        let payload = {
            let inner = self.inner.lock();
            let subscription = inner
                .ctrl_subscriptions
                .get(&client)
                .map_or(KeySubscription::Full, |subscription| subscription.keys);
            if subscription == KeySubscription::None {
                return;
            }
            match subscription {
                KeySubscription::Hash => EventPayload::KeyTablesHashChanged {
                    hash: inner.engine.keys.generation(),
                    mouse: MouseBindings::from_key_tables(&inner.engine.keys),
                },
                KeySubscription::Full => EventPayload::KeyTablesChanged {
                    tables: inner.engine.keys.snapshot(),
                },
                KeySubscription::None => return,
            }
        };
        Self::send_event(outbound, payload);
    }

    pub(super) fn send_compact_resync(
        &self,
        client: ClientId,
        outbound: &OutboundMailbox,
        full: bool,
    ) {
        self.send_compact_state(client, outbound, full);
        let (appearance, provenance, options, stream, terminals, overlays) = {
            let inner = self.inner.lock();
            let subscriptions = inner
                .ctrl_subscriptions
                .get(&client)
                .copied()
                .unwrap_or_default();
            let mut terminals = inner
                .streamed_terminals
                .get(&client)
                .into_iter()
                .flat_map(|terminals| terminals.keys())
                .filter_map(|pane| {
                    inner
                        .terminals
                        .get(pane)
                        .map(|terminal| (*pane, Arc::clone(terminal)))
                })
                .collect::<Vec<_>>();
            if let Some(popup) = inner.popups.get(&client) {
                terminals.push((popup.state.pane, Arc::clone(&popup.terminal)));
            }
            let mut overlays = vec![
                Self::event(EventPayload::CommandPrompt {
                    state: command_prompt_state(&inner, client),
                }),
                Self::event(EventPayload::ChooseTree {
                    state: inner
                        .choose_trees
                        .get(&client)
                        .map(|chooser| chooser.rendered.clone()),
                }),
                Self::event(EventPayload::ChooseBuffer {
                    state: inner
                        .choose_buffers
                        .get(&client)
                        .map(|chooser| chooser.rendered.clone()),
                }),
                Self::event(EventPayload::ChooserPresentation {
                    presentation: chooser_presentation::chooser_presentation(&inner, client)
                        .map(Box::new),
                }),
                Self::event(EventPayload::DisplayPanes {
                    state: inner
                        .display_panes
                        .get(&client)
                        .map(|overlay| overlay.state.clone()),
                }),
                Self::event(EventPayload::Popup {
                    state: inner.popups.get(&client).map(|popup| popup.state.clone()),
                }),
                Self::event(EventPayload::Menu {
                    state: inner.menus.get(&client).map(|menu| menu.state.clone()),
                }),
                Self::event(EventPayload::Confirm {
                    state: inner
                        .confirms
                        .get(&client)
                        .map(|confirm| confirm.state.clone()),
                }),
            ];
            if !full || inner.ctrl_initializing.contains(&client) {
                overlays.retain(|message| match message {
                    ProtocolMessage::Event(Event { payload, .. }) => match payload {
                        EventPayload::CommandPrompt { state } => state.is_some(),
                        EventPayload::ChooseTree { state } => state.is_some(),
                        EventPayload::ChooseBuffer { state } => state.is_some(),
                        EventPayload::ChooserPresentation { presentation } => {
                            presentation.is_some()
                        }
                        EventPayload::DisplayPanes { state } => state.is_some(),
                        EventPayload::Popup { state } => state.is_some(),
                        EventPayload::Menu { state } => state.is_some(),
                        EventPayload::Confirm { state } => state.is_some(),
                        _ => true,
                    },
                    _ => true,
                });
            }
            (
                (*published_appearance(&inner)).clone(),
                inner.appearance_provenance.clone(),
                (subscriptions.options != 0).then(|| {
                    let options = effective_mux_options(&inner, client);
                    MuxOptions::from_entries(
                        options
                            .iter()
                            .filter(|(key, _)| subscriptions.includes_option(*key))
                            .map(|(key, value)| (key, value.clone())),
                    )
                }),
                subscriptions.pane_stream,
                terminals,
                overlays,
            )
        };
        if full && self.inner.lock().client_kinds.get(&client) != Some(&ClientKind::Control) {
            Self::send_event(
                outbound,
                EventPayload::AppearanceChanged {
                    appearance: Box::new(appearance),
                    provenance,
                },
            );
        }
        if full {
            self.send_compact_keys(client, outbound);
        }
        if let Some(options) = options {
            let mut inner = self.inner.lock();
            let effective = effective_mux_options(&inner, client);
            inner.published_mux_options.insert(client, effective);
            drop(inner);
            Self::send_event(outbound, EventPayload::MuxOptionsPatched { options });
        }
        for message in overlays {
            let _ = outbound.enqueue_reliable(&message);
        }
        if stream {
            let _round_trips = zz_terminal::allow_actor_round_trips();
            for (pane, terminal) in terminals {
                let fresh = terminal.fresh_viewport();
                let viewport = terminal
                    .latest_viewport_for(TerminalViewId(client.0))
                    .unwrap_or(fresh);
                self.enqueue_kitty_images_for_viewport(outbound, pane, &terminal, &viewport);
                let _ = outbound.replace_terminal_viewport(pane, Self::next_sequence(), &viewport);
            }
        }
        #[cfg(feature = "agent")]
        self.send_agent_resync(client, outbound);
        if stream {
            let inner = self.inner.lock();
            if let Some(output) = inner.command_outputs.get(&client) {
                if let Some(viewport) = output
                    .terminal
                    .latest_viewport_for(TerminalViewId(client.0))
                {
                    let _ = outbound.replace_command_output(&Self::event(
                        EventPayload::CommandOutput {
                            output_id: output.output_id,
                            pane: output.pane,
                            viewport: Some((*viewport).clone()),
                        },
                    ));
                }
            } else if full && !inner.ctrl_initializing.contains(&client) {
                Self::send_event(
                    outbound,
                    EventPayload::CommandOutput {
                        output_id: 0,
                        pane: client_context_pane(&inner, client).unwrap_or(PaneId(0)),
                        viewport: None,
                    },
                );
            }
        }
        self.refresh_status_filtered(None, Some(&BTreeSet::from([client])));
    }

    pub(super) fn send_compact_attached(
        self: &Arc<Self>,
        client: ClientId,
        outbound: &Arc<OutboundMailbox>,
        _session: SessionId,
    ) -> bool {
        let initializing = {
            let mut inner = self.inner.lock();
            let generation = inner.ctrl_attachments.entry(client).or_default();
            *generation = generation.saturating_add(1);
            inner.ctrl_initializing.contains(&client)
        };
        outbound.hold_terminals();
        outbound.state.lock().ctrl_collecting = true;
        outbound.forget_delivered_terminals();
        outbound.reset_kitty_images();
        outbound.reset_pasted_images();
        if initializing {
            return true;
        }
        self.send_compact_resync(client, outbound, true);
        self.sync_key_table(client, false);
        let _ = self.try_deliver_startup_config_causes(client, outbound, false);
        initializing || outbound.flush_control_batch(false)
    }

    pub(super) fn execute_compact_request(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        request: zz_protocol::ExecRequest,
        outbound: &Arc<OutboundMailbox>,
    ) {
        if self.command_queue_cancelled(client) {
            return;
        }
        let commands = if let Some(line) = request.raw_control_line {
            let names = zz_mux::config_expansion_names("<control>", &line);
            let users = names.homes.into_iter().collect::<Vec<_>>();
            let homes = users
                .iter()
                .cloned()
                .zip(self.resolve_home_directories(&users))
                .filter_map(|(name, value)| value.map(|value| (name, value)))
                .collect::<BTreeMap<_, _>>();
            let names = names.variables.into_iter().collect::<Vec<_>>();
            let variables = names
                .iter()
                .cloned()
                .zip(self.resolve_environment(&names))
                .filter_map(|(name, value)| value.map(|value| (name, value)))
                .collect::<BTreeMap<_, _>>();
            let parsed =
                zz_mux::parse_config_with_expansions("<control>", &line, &homes, &variables);
            if let Some(error) = parsed.diagnostics.first() {
                let _ =
                    outbound.enqueue_reliable(&ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                        server_id: self.server_id,
                        outcome: zz_protocol::ExecOutcome::Rejected(ServerError::CommandParse(
                            error.message.clone(),
                        )),
                    }));
                return;
            }
            parsed.commands
        } else {
            request.commands
        };
        let prepared =
            Self::prepare_command_list_with_engine(&self.inner.lock().engine, commands, true);
        if let Some(error) = prepared.iter().find_map(|prepared| match &prepared.result {
            PreparedCommandResult::Error(error) => Some(error.clone()),
            PreparedCommandResult::Ready => None,
        }) {
            let _ = outbound.enqueue_reliable(&ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                server_id: self.server_id,
                outcome: zz_protocol::ExecOutcome::Rejected(error),
            }));
            return;
        }
        let command_count = prepared.len();
        for (index, prepared) in prepared.into_iter().enumerate() {
            if self.command_queue_cancelled(client) {
                break;
            }
            sync_context_with_attachment(&self.inner.lock(), client, context);
            let _park = (kind == ClientKind::Control)
                .then(|| CommandQueueParkScope::new(client, index as u64 + 1));
            if kind == ClientKind::Control {
                let wakeup =
                    !control_query_can_defer_wakeup(&self.inner.lock(), context, &prepared);
                let started = Self::event(EventPayload::ControlCommandStarted {
                    request_id: index as u64 + 1,
                    flags: u32::from(if prepared.invocation.source.is_some() {
                        CONTROL_COMMAND_FRAME_FLAGS_CONTROL
                    } else {
                        CONTROL_COMMAND_FRAME_FLAGS_NONE
                    }),
                    canonical_name: prepared.canonical_name.clone(),
                    guard: !MuxEngine::is_command_alias_group(&prepared.invocation),
                });
                let _ = outbound.enqueue_reliable_with_wakeup(&started, wakeup);
            }
            let response = match prepared.result {
                PreparedCommandResult::Ready => self.execute_command_request_with_prepared(
                    client,
                    kind,
                    context,
                    index as u64 + 1,
                    &prepared.invocation,
                    true,
                ),
                PreparedCommandResult::Error(error) => CommandResponse::Error {
                    request_id: index as u64 + 1,
                    error,
                    output: RawText::default(),
                },
            };
            let failed = matches!(response, CommandResponse::Error { .. });
            if kind == ClientKind::Control && (failed || index + 1 == command_count) {
                let completion = [
                    ProtocolMessage::CommandResponse(response),
                    ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                        server_id: self.server_id,
                        outcome: zz_protocol::ExecOutcome::Ran,
                    }),
                ];
                let frames = completion
                    .iter()
                    .map(|message| outbound.encode_message(message))
                    .collect::<Result<_, _>>();
                if !frames.is_ok_and(|frames| outbound.enqueue_control_group(frames)) {
                    for message in &completion {
                        let _ = outbound.enqueue_reliable(message);
                    }
                }
                return;
            }
            let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(response));
            if failed {
                break;
            }
        }
        let _ = outbound.enqueue_reliable(&ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            server_id: self.server_id,
            outcome: zz_protocol::ExecOutcome::Ran,
        }));
    }

    pub(super) fn initialize_compact(
        self: &Arc<Self>,
        client: ClientId,
        hello: &Hello,
        outbound: &Arc<OutboundMailbox>,
        context: &mut ExecutionContext,
    ) {
        let mut pending_errors = Vec::new();
        match &hello.attach {
            Some(AttachOperation::Session(target)) => {
                match self.attach_target(client, hello.client.kind, context, target) {
                    Ok((session, snapshot)) => {
                        self.send_attached(client, outbound, session, snapshot);
                        self.publish_snapshot();
                    }
                    Err(error) => {
                        let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                            CommandResponse::Error {
                                request_id: 0,
                                error,
                                output: RawText::default(),
                            },
                        ));
                    }
                }
            }
            Some(AttachOperation::Commands(commands)) => {
                let commands = commands
                    .iter()
                    .map(|command| {
                        if command.canonical_name.is_none()
                            && !command.alias_matched
                            && command.result == PreparedCommandResult::Ready
                        {
                            Self::prepare_command_list_with_engine(
                                &self.inner.lock().engine,
                                vec![command.invocation.clone()],
                                true,
                            )
                            .remove(0)
                        } else {
                            command.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                let rejected = commands.iter().enumerate().find_map(|(index, prepared)| {
                    if let PreparedCommandResult::Error(error) = &prepared.result {
                        Some((index as u64 + 1, error.clone()))
                    } else {
                        None
                    }
                });
                let reject = rejected.is_some();
                if let Some((request_id, error)) = rejected {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id,
                            error,
                            output: RawText::default(),
                        },
                    ));
                }
                for (index, prepared) in commands.iter().enumerate().filter(|_| !reject) {
                    if let PreparedCommandResult::Error(error) = &prepared.result {
                        let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                            CommandResponse::Error {
                                request_id: index as u64 + 1,
                                error: error.clone(),
                                output: RawText::default(),
                            },
                        ));
                        break;
                    }
                    let response = self.execute_command_request_with_prepared(
                        client,
                        hello.client.kind,
                        context,
                        index as u64 + 1,
                        &prepared.invocation,
                        prepared.canonical_name.is_some() || prepared.alias_matched,
                    );
                    let failed = matches!(response, CommandResponse::Error { .. });
                    if failed && client_attached_session(&self.inner.lock(), client).is_some() {
                        pending_errors.push(response);
                    } else {
                        let _ =
                            outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(response));
                    }
                    if failed {
                        break;
                    }
                }
            }
            None => {}
        }
        if client_attached_session(&self.inner.lock(), client).is_some() {
            self.sync_key_table(client, false);
            let _ = self.try_deliver_startup_config_causes(client, outbound, false);
        }
        self.send_compact_resync(client, outbound, true);
        for response in pending_errors {
            let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(response));
        }
        self.inner.lock().ctrl_initializing.remove(&client);
        outbound.flush_control_batch(true);
    }
}
