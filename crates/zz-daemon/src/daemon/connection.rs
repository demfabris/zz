use super::*;
use zz_protocol::Hello;

pub(super) struct Session {
    pub(super) client: ClientId,
    pub(super) hello: ClientHello,
    cancel: Arc<AtomicBool>,
    compact_hello: Option<Hello>,
    context: Option<ExecutionContext>,
    path_list: Option<(u64, Arc<AtomicBool>)>,
    path_list_turn: Arc<Mutex<()>>,
    registration: ClientRegistrationGuard,
    pub(super) released: Arc<AtomicBool>,
    writer_registration: ClientWriterRegistrationGuard,
}

impl Session {
    pub(super) fn register(
        shared: &Arc<Shared>,
        first: ProtocolMessage,
        outbound: &Arc<OutboundMailbox>,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Option<Self>, DaemonError> {
        let (hello, compact_hello) = match first {
            ProtocolMessage::Hello(hello) => (hello.clone().into_client(), Some(hello)),
            ProtocolMessage::ClientHello(hello) => (hello, None),
            _ => {
                return Err(ServerError::InvalidCommand(
                    "first protocol message must be ClientHello".to_owned(),
                )
                .into());
            }
        };
        if let Err(error) = validate_hello(&hello) {
            let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                CommandResponse::Error {
                    request_id: 0,
                    error: ServerError::ProtocolMismatch {
                        client: hello.protocol_version,
                        server: PROTOCOL_VERSION,
                    },
                    output: RawText::default(),
                },
            ));
            return Err(error);
        }
        let startup_reentry_capability = format!(
            "{}{}",
            crate::STARTUP_REENTRY_CAPABILITY_PREFIX,
            shared.server_id
        );
        let startup_reentry = hello.kind == ClientKind::Command
            && hello.capabilities.contains(&startup_reentry_capability);
        if !startup_reentry && !shared.wait_for_startup() {
            return Ok(None);
        }

        let registration = if let Some(hello) = &compact_hello {
            shared
                .register_welcome(hello)
                .map(|(client, welcome)| (client, ProtocolMessage::Welcome(welcome)))
        } else {
            shared
                .register(
                    hello.kind,
                    hello.client_instance_id,
                    hello.device_name.clone(),
                    hello.color_scheme,
                    hello.kind == ClientKind::Interactive
                        && hello.capabilities.iter().any(|capability| {
                            capability == ClientHello::CLIENT_TERMINAL_CAPABILITY
                        }),
                    startup_reentry,
                )
                .map(|(client, hello)| (client, ProtocolMessage::ServerHello(Box::new(hello))))
        };
        let Some((client, greeting)) = registration else {
            let _ = outbound.enqueue_reliable(&server_stopping_response(0));
            return Ok(None);
        };
        {
            let mut inner = shared.inner.lock();
            if hello.kind == ClientKind::Interactive
                && hello.capabilities.iter().any(|capability| {
                    capability == ClientHello::CLIENT_NATIVE_TERMINAL_SEARCH_CAPABILITY
                })
            {
                inner.client_entry(client).native_terminal_search = true;
            }
            if hello.kind == ClientKind::Interactive
                && hello
                    .capabilities
                    .iter()
                    .any(|capability| capability == ClientHello::CLIENT_NATIVE_CHOOSER_CAPABILITY)
            {
                inner.client_entry(client).native_chooser = true;
            }
            if hello.kind == ClientKind::Interactive
                && hello
                    .capabilities
                    .iter()
                    .any(|capability| capability == ClientHello::CLIENT_PATH_PICKER_CAPABILITY)
            {
                inner.client_entry(client).path_picker = true;
            }
            let registered = inner.client_mut(client).expect("registered client");
            registered.origin = hello.origin;
            registered.nested = client_nested_fact(&hello.capabilities);
            registered.utf8 = client_utf8_fact(&hello.capabilities);
            let features = client_features_fact(&hello.capabilities);
            registered.features = (features != 0).then_some(features);
            registered.tty = client_tty_fact(&hello.capabilities);
            registered.size = client_size_fact(&hello.capabilities);
            registered.cell_pixels = attach::client_cell_fact(&hello.capabilities);
            if let Some(viewport) = compact_hello.as_ref().and_then(|hello| hello.viewport) {
                registered.size = Some((viewport.columns, viewport.rows));
                registered.cell_pixels = Some((viewport.cell_width_px, viewport.cell_height_px));
            }
            registered.pid = Some(hello.process_id);
            registered.working_directory =
                client_working_directory_fact(hello.working_directory.as_ref());
            registered.environment = Some(client_environment_fact(&hello.environment));
        }
        warm_terminfo_entries(&hello.environment);
        let registration = ClientRegistrationGuard::new(shared, client);
        log::debug!(
            target: "zz_daemon::diagnostics::connection",
            "registered client={client} kind={:?} hello={hello:#?}",
            hello.kind,
        );
        let writer_registration =
            ClientWriterRegistrationGuard::new(shared, client, Arc::clone(outbound));
        if compact_hello.is_some() {
            let mut state = outbound.state.lock();
            state.attach_batch = true;
            state.ctrl_collecting = ControlCollection::Attach;
            state.terminals_held = true;
        }
        let _ = outbound.enqueue_reliable(&greeting);
        if matches!(hello.kind, ClientKind::Interactive | ClientKind::Control) {
            shared.subscribe(client, Arc::clone(outbound));
        }
        if hello.kind == ClientKind::Control
            && hello
                .capabilities
                .iter()
                .any(|capability| capability == ClientHello::STARTUP_CONFIG_OWNER_CAPABILITY)
        {
            shared.try_deliver_startup_config_causes(client, outbound, true);
        }

        let context = {
            let inner = shared.inner.lock();
            hello
                .origin
                .and_then(|pane| ExecutionContext::for_pane(&inner.engine.state, pane))
                .or_else(|| {
                    let fallback =
                        if matches!(hello.kind, ClientKind::Command | ClientKind::Control) {
                            inner.engine.state.most_recent_context()
                        } else {
                            inner.engine.state.default_context()
                        };
                    fallback.map(|(session, window, pane)| {
                        ExecutionContext::new(Some(session), Some(window), Some(pane))
                    })
                })
                .unwrap_or_default()
        };

        shared
            .command_queue_cancels
            .lock()
            .insert(client, Arc::clone(cancel));
        Ok(Some(Self {
            client,
            hello,
            cancel: Arc::clone(cancel),
            compact_hello,
            context: Some(context),
            path_list: None,
            path_list_turn: Arc::new(Mutex::new(())),
            registration,
            released: Arc::new(AtomicBool::new(false)),
            writer_registration,
        }))
    }

    pub(super) fn initialize(&mut self, shared: &Arc<Shared>, outbound: &Arc<OutboundMailbox>) {
        if self.cancel.load(Ordering::Acquire) || self.released.load(Ordering::Acquire) {
            return;
        }
        if let Some(hello) = &self.compact_hello {
            shared.initialize_compact(
                self.client,
                hello,
                outbound,
                self.context.as_mut().expect("client context"),
            );
        }
    }

    pub(super) fn try_inline_message(
        &mut self,
        shared: &Arc<Shared>,
        outbound: &Arc<OutboundMailbox>,
        message: ProtocolMessage,
    ) -> Option<ProtocolMessage> {
        let context = self.context.as_mut().expect("client context");
        match message {
            ProtocolMessage::Exec(mut request) => {
                if let Some(line) = &request.raw_control_line {
                    let names = zz_mux::config_expansion_names("<control>", line);
                    if !names.homes.is_empty() || !names.variables.is_empty() {
                        return Some(ProtocolMessage::Exec(request));
                    }
                    let parsed = zz_mux::parse_config_with_expansions(
                        "<control>",
                        line,
                        &BTreeMap::new(),
                        &BTreeMap::new(),
                    );
                    if !parsed.diagnostics.is_empty() {
                        return Some(ProtocolMessage::Exec(request));
                    }
                    request.commands = parsed.commands;
                    request.raw_control_line = None;
                }
                let prepared = {
                    let inner = shared.inner.lock();
                    Shared::prepare_command_list_with_engine(
                        &inner.engine,
                        request.commands.clone(),
                        self.hello.kind == ClientKind::Command,
                    )
                };
                sync_context_with_attachment(&shared.inner.lock(), self.client, context);
                if !prepared
                    .iter()
                    .all(|command| inline_query(shared, context, command))
                {
                    return Some(ProtocolMessage::Exec(request));
                }
                shared.execute_prepared_compact_request(
                    self.client,
                    self.hello.kind,
                    context,
                    prepared,
                    outbound,
                );
                None
            }
            ProtocolMessage::CommandRequest(request) => {
                if request.prepared {
                    return Some(ProtocolMessage::CommandRequest(request));
                }
                let prepared = {
                    let inner = shared.inner.lock();
                    Shared::prepare_command_list_with_engine(
                        &inner.engine,
                        vec![request.command.clone()],
                        self.hello.kind == ClientKind::Command,
                    )
                };
                sync_context_with_attachment(&shared.inner.lock(), self.client, context);
                if !prepared
                    .iter()
                    .all(|command| inline_query(shared, context, command))
                {
                    return Some(ProtocolMessage::CommandRequest(request));
                }
                self.message(shared, outbound, ProtocolMessage::CommandRequest(request));
                None
            }
            message => Some(message),
        }
    }

    pub(super) fn message(
        &mut self,
        shared: &Arc<Shared>,
        outbound: &Arc<OutboundMailbox>,
        message: ProtocolMessage,
    ) {
        let client = self.client;
        let hello = &self.hello;
        let context = &mut self.context;
        let path_list = &mut self.path_list;
        let path_list_turn = &self.path_list_turn;
        let message_started = diagnostic_timer();
        if shared.shutdown_pending.load(Ordering::Acquire)
            && !(hello.kind == ClientKind::Command
                && matches!(
                    message,
                    ProtocolMessage::CommandRequest(_)
                        | ProtocolMessage::PrepareCommandList { .. }
                        | ProtocolMessage::ClientFileResponse(_)
                ))
        {
            return;
        }
        match message {
            ProtocolMessage::Exec(request) => {
                shared.execute_compact_request(
                    client,
                    hello.kind,
                    context.as_mut().expect("client context"),
                    request,
                    outbound,
                );
            }
            ProtocolMessage::TreeSync => {
                shared.send_compact_state(client, outbound, true);
            }
            ProtocolMessage::GetKeyTables => {
                let tables = shared.inner.lock().engine.keys.snapshot();
                Shared::send_event(outbound, EventPayload::KeyTablesChanged { tables });
            }
            ProtocolMessage::CommandRequest(CommandRequest {
                request_id,
                command,
                prepared,
            }) => {
                shared.inner.lock().cold_bootstrap.command(client);
                let context = context.as_mut().expect("client context");
                sync_context_with_attachment(&shared.inner.lock(), client, context);
                let item = shared.command_item(
                    (hello.kind == ClientKind::Control).then_some((client, request_id)),
                );
                let _ = item.execute_command_request_with_prepared_into(
                    outbound, client, hello.kind, context, request_id, &command, prepared,
                );
            }
            ProtocolMessage::ClientFileResponse(response) => {
                shared.complete_client_file(client, response);
            }
            ProtocolMessage::HomeDirectoryRequest { request_id, users } => {
                let homes = shared.resolve_home_directories(&users);
                let _ = outbound.enqueue_reliable(&ProtocolMessage::HomeDirectoryResponse {
                    request_id,
                    homes,
                });
            }
            ProtocolMessage::PathListRequest {
                request_id,
                pane,
                dir,
            } => {
                if let Some((_, cancel)) = path_list.take() {
                    cancel.store(true, Ordering::Release);
                }
                let cancel = Arc::new(AtomicBool::new(false));
                *path_list = Some((request_id, Arc::clone(&cancel)));
                shared.start_path_list(
                    client,
                    hello.kind,
                    (request_id, pane, dir),
                    outbound,
                    (&cancel, path_list_turn),
                );
            }
            ProtocolMessage::PathListCancel { request_id } => {
                if path_list
                    .as_ref()
                    .is_some_and(|(active, _)| *active == request_id)
                    && let Some((_, cancel)) = path_list.take()
                {
                    cancel.store(true, Ordering::Release);
                }
            }
            ProtocolMessage::EnvironmentRequest { request_id, names } => {
                let values = shared.resolve_environment(&names);
                let _ = outbound
                    .enqueue_reliable(&ProtocolMessage::EnvironmentResponse { request_id, values });
            }
            ProtocolMessage::PrepareCommandList {
                request_id,
                commands,
            } => {
                let commands = shared.prepare_command_list_for_request(client, commands);
                let _ = outbound.enqueue_reliable(&ProtocolMessage::PreparedCommandList {
                    request_id,
                    commands,
                });
            }
            ProtocolMessage::Attach { session } => {
                if hello.kind == ClientKind::Command {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error: ServerError::InvalidCommand(
                                "command client cannot attach".to_owned(),
                            ),
                            output: RawText::default(),
                        },
                    ));
                    return;
                }
                let Some(context) = context.as_mut() else {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error: ServerError::InvalidCommand(
                                "command client cannot attach".to_owned(),
                            ),
                            output: RawText::default(),
                        },
                    ));
                    return;
                };
                outbound.hold_terminals();
                match shared.attach_target(client, hello.kind, context, &session) {
                    Ok((session, snapshot)) => {
                        shared.clear_pending_committed_text(client);
                        shared.send_attached(client, outbound, session, snapshot);
                        shared.publish_snapshot();
                        outbound.release_terminals();
                    }
                    Err(error) => {
                        outbound.release_terminals();
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
            ProtocolMessage::Detach => {
                let control_session = (hello.kind == ClientKind::Control)
                    .then(|| {
                        shared
                            .inner
                            .lock()
                            .attached
                            .iter()
                            .find_map(|(session, clients)| {
                                clients.contains(&client).then_some(*session)
                            })
                    })
                    .flatten();
                shared.detach(client);
                if let Some(session) = control_session {
                    shared
                        .publish_to_client(client, EventPayload::detached_requested(session, None));
                }
            }
            ProtocolMessage::SetColorScheme(color_scheme) => {
                if hello.kind == ClientKind::Interactive {
                    shared.set_client_color_scheme(client, color_scheme);
                }
            }
            ProtocolMessage::SetConfigOverrides { entries } => {
                shared.set_config_overrides(client, hello.kind, &entries);
            }
            ProtocolMessage::SetTerminalPreview { enabled } => {
                shared.set_terminal_preview(client, hello.kind, enabled);
            }
            ProtocolMessage::ClientTerminalFeatures { features } => {
                shared.add_client_terminal_features(client, hello.kind, &features);
            }
            ProtocolMessage::ClientTerminalType { term_type } => {
                shared.set_client_terminal_type(client, hello.kind, &term_type);
            }
            ProtocolMessage::Input(input) => {
                if let Some(context) = context.as_mut() {
                    sync_context_with_attachment(&shared.inner.lock(), client, context);
                }
                if hello.kind != ClientKind::Interactive {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error: ServerError::InvalidCommand(
                                "command client cannot send input".to_owned(),
                            ),
                            output: RawText::default(),
                        },
                    ));
                    return;
                }
                let Some(context) = context.as_mut() else {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error: ServerError::InvalidCommand(
                                "command client cannot send input".to_owned(),
                            ),
                            output: RawText::default(),
                        },
                    ));
                    return;
                };
                if let Err(error) = shared.input(client, hello.kind, context, input) {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error: daemon_server_error(error),
                            output: RawText::default(),
                        },
                    ));
                }
            }
            ProtocolMessage::GuiResponse(response) => {
                shared.complete_gui_request(client, response);
            }
            ProtocolMessage::Resync => shared.send_resync(client, outbound),
            ProtocolMessage::RequestFull { pane } => {
                shared.request_full(client, pane, outbound);
            }
            ProtocolMessage::HistoryRequest { pane, start, count } => {
                shared.send_history(client, pane, start, count, outbound);
            }
            ProtocolMessage::PasteUploadBegin {
                upload_id,
                pane,
                purpose,
                extension,
                total_bytes,
            } => {
                shared.begin_paste_upload(
                    client,
                    hello.kind,
                    upload_id,
                    pane,
                    purpose,
                    extension,
                    total_bytes,
                );
            }
            ProtocolMessage::PasteUploadChunk { upload_id, bytes } => {
                shared.extend_paste_upload(client, upload_id, &bytes);
            }
            ProtocolMessage::FetchPastedImage { pane, number }
                if hello.kind == ClientKind::Interactive =>
            {
                shared.fetch_pasted_image(client, pane, number);
            }
            message @ (ProtocolMessage::AgentPrompt { .. }
            | ProtocolMessage::AgentCancel { .. }
            | ProtocolMessage::AgentUnqueue { .. }
            | ProtocolMessage::AgentRespondPermission { .. }
            | ProtocolMessage::AgentSetConfigOption { .. }
            | ProtocolMessage::AgentSetMode { .. }
            | ProtocolMessage::AgentAuthenticate { .. }
            | ProtocolMessage::AgentSessionOp { .. }
            | ProtocolMessage::AgentReplay { .. }
            | ProtocolMessage::AgentAcknowledgePromptRestore { .. }) => {
                let read_only_blocked = !matches!(
                    message,
                    ProtocolMessage::AgentReplay { .. }
                        | ProtocolMessage::AgentAcknowledgePromptRestore { .. }
                ) && shared.inner.lock().client_flags.contains(client);
                if read_only_blocked {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error: ServerError::InvalidCommand("client is read-only".to_owned()),
                            output: RawText::default(),
                        },
                    ));
                } else if let Err(error) = handle_agent_message(shared, client, message) {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error,
                            output: RawText::default(),
                        },
                    ));
                }
            }
            _ => {}
        }
        log::trace!(
            target: "zz_daemon::diagnostics::connection",
            "message end client={client} elapsed_us={} context={context:#?}",
            diagnostic_elapsed_us(message_started),
        );
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some((_, cancel)) = self.path_list.take() {
            cancel.store(true, Ordering::Release);
        }
        if self.released.swap(true, Ordering::AcqRel) {
            self.registration.armed = false;
            self.writer_registration.armed = false;
        } else {
            self.registration.shared.detach(self.client);
            self.registration.unregister();
            self.writer_registration.unregister();
        }
        self.registration
            .shared
            .command_queue_cancels
            .lock()
            .remove(&self.client);
    }
}

pub(super) fn inline_query(
    shared: &Shared,
    context: &ExecutionContext,
    command: &PreparedCommand,
) -> bool {
    command.result == PreparedCommandResult::Ready
        && command.canonical_name.as_deref() != Some("capture-pane")
        && ctrl::control_query_can_defer_wakeup(&shared.inner.lock(), context, command)
}
