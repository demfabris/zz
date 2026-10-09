use super::*;
use zz_protocol::{AttachOperation, Hello};

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
    #[cfg(unix)]
    request: Option<PendingMessage>,
    initializing: bool,
    task: Option<Box<wait_queue::CommandTask>>,
    task_after: TaskAfter,
    input_wait: Option<cmdq::WaitContinuation>,
}

#[derive(Default)]
enum TaskAfter {
    #[default]
    Request,
    Compact {
        last: bool,
    },
    Initialize,
    Attach(Box<PendingMessage>),
}

#[cfg(unix)]
#[expect(
    clippy::large_enum_variant,
    reason = "keep ready requests inline without per-command allocations"
)]
enum PendingMessage {
    Message(ProtocolMessage),
    Initialize {
        commands: std::vec::IntoIter<PreparedCommand>,
        request_id: u64,
        pending_errors: Vec<CommandResponse>,
    },
    InitializeFinish(Vec<CommandResponse>),
    InitializeAttach,
    Compact {
        commands: std::vec::IntoIter<PreparedCommand>,
        request_id: u64,
    },
}

#[cfg(unix)]
pub(super) enum MessageProgress {
    Done,
    Output,
    Wait,
    Worker,
    Ready,
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
        if !startup_reentry && !*shared.startup_ready.lock() {
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
            registered.exits_on_detach = attach::client_exits_on_detach_fact(&hello.capabilities);
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
        shared.client_lifecycle_hook("client-created", client);
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
            #[cfg(unix)]
            request: None,
            initializing: false,
            task: None,
            task_after: TaskAfter::Request,
            input_wait: None,
        }))
    }

    #[cfg(test)]
    pub(super) fn initialize(&mut self, shared: &Arc<Shared>, outbound: &Arc<OutboundMailbox>) {
        self.start_initialize(shared, outbound);
        while !matches!(
            self.run_message(shared, outbound, false),
            MessageProgress::Done
        ) {}
    }

    pub(super) fn start_initialize(
        &mut self,
        shared: &Arc<Shared>,
        outbound: &Arc<OutboundMailbox>,
    ) {
        if self.cancel.load(Ordering::Acquire) || self.released.load(Ordering::Acquire) {
            return;
        }
        let Some(hello) = &self.compact_hello else {
            return;
        };
        self.initializing = true;
        self.request = match &hello.attach {
            Some(AttachOperation::Commands(commands)) => {
                let commands = shared.prepare_initial_commands(commands);
                let rejected = commands.iter().enumerate().find_map(|(index, prepared)| {
                    match &prepared.result {
                        PreparedCommandResult::Error(error) => {
                            Some((index as u64 + 1, error.clone()))
                        }
                        PreparedCommandResult::Ready => None,
                    }
                });
                if let Some((request_id, error)) = rejected {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id,
                            error,
                            output: RawText::default(),
                        },
                    ));
                    Some(PendingMessage::InitializeFinish(Vec::new()))
                } else {
                    Some(PendingMessage::Initialize {
                        commands: commands.into_iter(),
                        request_id: 1,
                        pending_errors: Vec::new(),
                    })
                }
            }
            Some(AttachOperation::Session(_)) => Some(PendingMessage::InitializeAttach),
            None => Some(PendingMessage::InitializeFinish(Vec::new())),
        };
    }

    pub(super) fn initializing(&self) -> bool {
        self.initializing
    }

    #[cfg(unix)]
    fn finishing_initialize(&self) -> bool {
        match &self.request {
            Some(PendingMessage::Initialize { commands, .. }) => commands.as_slice().is_empty(),
            Some(PendingMessage::InitializeFinish(_)) => true,
            _ => false,
        }
    }

    #[cfg(unix)]
    pub(super) fn start_message(&mut self, message: ProtocolMessage) {
        assert!(self.request.is_none());
        self.request = Some(PendingMessage::Message(message));
    }

    #[cfg(unix)]
    pub(super) fn message_pending(&self) -> bool {
        self.request.is_some() || self.task.is_some() || self.input_wait.is_some()
    }

    pub(super) fn command_wait_ready(&self) -> bool {
        self.task.as_ref().is_some_and(|task| task.ready())
            || self
                .input_wait
                .as_ref()
                .is_some_and(cmdq::WaitContinuation::ready)
    }

    #[cfg(unix)]
    pub(super) fn run_message(
        &mut self,
        shared: &Arc<Shared>,
        outbound: &Arc<OutboundMailbox>,
        inline: bool,
    ) -> MessageProgress {
        if let Some(wait) = &self.input_wait {
            if !wait.ready() {
                return MessageProgress::Wait;
            }
            self.input_wait = None;
            return MessageProgress::Done;
        }
        let wakes = (!inline && self.initializing).then(zz_terminal::hold_actor_wakes);
        let mut step_again = wakes.is_some();
        loop {
            if let Some(mut task) = self.task.take() {
                match task.run(inline) {
                    wait_queue::Progress::Waiting => {
                        self.task = Some(task);
                        return MessageProgress::Wait;
                    }
                    wait_queue::Progress::Worker => {
                        self.task = Some(task);
                        return MessageProgress::Worker;
                    }
                    wait_queue::Progress::Ready => {
                        self.task = Some(task);
                        if std::mem::take(&mut step_again) {
                            continue;
                        }
                        return MessageProgress::Ready;
                    }
                    wait_queue::Progress::Done => {
                        let (response, client_exit, context, _admission) = task.finish();
                        self.context = Some(context);
                        let failed = matches!(response, CommandResponse::Error { .. });
                        match std::mem::take(&mut self.task_after) {
                            TaskAfter::Request => {
                                #[cfg(test)]
                                if let Some(hook) = shared.response_admission_hook.lock().take() {
                                    let _ = hook.reached.send(());
                                    let _ = hook.release.recv();
                                }
                                let _ = outbound
                                    .enqueue_reliable(&ProtocolMessage::CommandResponse(response));
                                return MessageProgress::Done;
                            }
                            TaskAfter::Compact { last } => {
                                let _ = outbound
                                    .enqueue_reliable(&ProtocolMessage::CommandResponse(response));
                                if failed || last || client_exit {
                                    self.request = None;
                                    let _ = outbound.enqueue_reliable(&ProtocolMessage::ExecExit(
                                        zz_protocol::ExecExit {
                                            server_id: shared.server_id,
                                            outcome: zz_protocol::ExecOutcome::Ran,
                                        },
                                    ));
                                    return MessageProgress::Done;
                                }
                            }
                            TaskAfter::Attach(request) => {
                                if failed {
                                    let _ = outbound.enqueue_reliable(
                                        &ProtocolMessage::CommandResponse(response),
                                    );
                                    if matches!(*request, PendingMessage::InitializeAttach) {
                                        self.request =
                                            Some(PendingMessage::InitializeFinish(Vec::new()));
                                    } else {
                                        return MessageProgress::Done;
                                    }
                                } else {
                                    self.request = Some(*request);
                                }
                            }
                            TaskAfter::Initialize => {
                                let Some(PendingMessage::Initialize { pending_errors, .. }) =
                                    self.request.as_mut()
                                else {
                                    unreachable!()
                                };
                                if failed
                                    && client_attached_session(&shared.inner.lock(), self.client)
                                        .is_some()
                                {
                                    pending_errors.push(response);
                                } else {
                                    let _ = outbound.enqueue_reliable(
                                        &ProtocolMessage::CommandResponse(response),
                                    );
                                }
                                if failed {
                                    let Some(PendingMessage::Initialize { pending_errors, .. }) =
                                        self.request.take()
                                    else {
                                        unreachable!()
                                    };
                                    self.request =
                                        Some(PendingMessage::InitializeFinish(pending_errors));
                                }
                            }
                        }
                        if !inline && !self.finishing_initialize() {
                            return MessageProgress::Ready;
                        }
                    }
                }
            }
            let Some(request) = self.request.take() else {
                break;
            };
            if self.cancel.load(Ordering::Acquire) {
                return MessageProgress::Done;
            }
            let default_attach = match &request {
                PendingMessage::InitializeAttach => self.compact_hello.as_ref().is_some_and(|hello|
                    matches!(&hello.attach, Some(AttachOperation::Session(target)) if target.is_empty())),
                PendingMessage::Message(ProtocolMessage::Attach { session }) => session.is_empty(),
                _ => false,
            };
            if default_attach
                && matches!(
                    self.hello.kind,
                    ClientKind::Interactive | ClientKind::Control
                )
                && shared.inner.lock().engine.state.sessions.is_empty()
            {
                match wait_queue::CommandTask::default_attach(
                    shared,
                    self.client,
                    self.context.as_ref().expect("client context"),
                ) {
                    Ok(task) => {
                        self.task = Some(Box::new(task));
                        self.task_after = TaskAfter::Attach(Box::new(request));
                        continue;
                    }
                    Err(response) => {
                        let _ =
                            outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(response));
                        return MessageProgress::Done;
                    }
                }
            }
            match request {
                PendingMessage::Message(ProtocolMessage::Exec(request)) => {
                    if inline
                        && request.raw_control_line.as_ref().is_some_and(|line| {
                            let names = zz_mux::config_expansion_names("<control>", line);
                            !names.homes.is_empty() || !names.variables.is_empty()
                        })
                    {
                        self.request =
                            Some(PendingMessage::Message(ProtocolMessage::Exec(request)));
                        return MessageProgress::Worker;
                    }
                    match shared.prepare_compact_request(self.hello.kind, request, !inline) {
                        Ok(commands) => {
                            self.request = Some(PendingMessage::Compact {
                                commands: commands.into_iter(),
                                request_id: 1,
                            });
                        }
                        Err(error) => {
                            let _ = outbound.enqueue_reliable(&ProtocolMessage::ExecExit(
                                zz_protocol::ExecExit {
                                    server_id: shared.server_id,
                                    outcome: zz_protocol::ExecOutcome::Rejected(error),
                                },
                            ));
                            return MessageProgress::Done;
                        }
                    }
                    if !inline {
                        return MessageProgress::Ready;
                    }
                }
                PendingMessage::Initialize {
                    mut commands,
                    request_id,
                    mut pending_errors,
                } => {
                    if commands.as_slice().is_empty() {
                        self.request = Some(PendingMessage::InitializeFinish(pending_errors));
                        continue;
                    }
                    let context = self.context.as_mut().expect("client context");
                    let task = wait_queue::task_command(&commands.as_slice()[0].invocation);
                    if inline {
                        let progress = if exec::output_pending(outbound) {
                            if outbound.state.lock().ctrl_collecting == ControlCollection::Attach {
                                outbound.flush_control_batch(true);
                            }
                            Some(MessageProgress::Output)
                        } else if !inline_task_or_query(
                            shared,
                            context,
                            &commands.as_slice()[0],
                            task,
                        ) {
                            Some(MessageProgress::Worker)
                        } else {
                            None
                        };
                        if let Some(progress) = progress {
                            self.request = Some(PendingMessage::Initialize {
                                commands,
                                request_id,
                                pending_errors,
                            });
                            return progress;
                        }
                    }
                    let prepared = commands.next().unwrap();
                    if task || !inline {
                        self.task = wait_queue::CommandTask::new(
                            shared,
                            self.client,
                            self.hello.kind,
                            context,
                            request_id,
                            &prepared.invocation,
                            prepared.canonical_name.is_some() || prepared.alias_matched,
                        )
                        .map(Box::new)
                        .ok();
                        if self.task.is_some() {
                            self.task_after = TaskAfter::Initialize;
                            self.request = Some(PendingMessage::Initialize {
                                commands,
                                request_id: request_id.saturating_add(1),
                                pending_errors,
                            });
                            continue;
                        }
                    }
                    let response = shared.execute_command_request_with_prepared(
                        self.client,
                        self.hello.kind,
                        context,
                        request_id,
                        &prepared.invocation,
                        prepared.canonical_name.is_some() || prepared.alias_matched,
                    );
                    let failed = matches!(response, CommandResponse::Error { .. });
                    if failed
                        && client_attached_session(&shared.inner.lock(), self.client).is_some()
                    {
                        pending_errors.push(response);
                    } else {
                        let _ =
                            outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(response));
                    }
                    self.request = if failed {
                        Some(PendingMessage::InitializeFinish(pending_errors))
                    } else {
                        Some(PendingMessage::Initialize {
                            commands,
                            request_id: request_id.saturating_add(1),
                            pending_errors,
                        })
                    };
                    if !inline {
                        return MessageProgress::Ready;
                    }
                }
                PendingMessage::InitializeFinish(pending_errors) => {
                    if inline {
                        self.request = Some(PendingMessage::InitializeFinish(pending_errors));
                        return MessageProgress::Worker;
                    }
                    shared.finish_initialize_compact(self.client, outbound, pending_errors);
                    self.initializing = false;
                    return MessageProgress::Done;
                }
                PendingMessage::InitializeAttach => {
                    if inline {
                        self.request = Some(PendingMessage::InitializeAttach);
                        return MessageProgress::Worker;
                    }
                    shared.initialize_compact(
                        self.client,
                        self.compact_hello.as_ref().expect("compact hello"),
                        outbound,
                        self.context.as_mut().expect("client context"),
                    );
                    self.initializing = false;
                    return MessageProgress::Done;
                }
                PendingMessage::Compact {
                    mut commands,
                    request_id,
                } => {
                    if commands.as_slice().is_empty() || shared.command_queue_cancelled(self.client)
                    {
                        let _ = outbound.enqueue_reliable(&ProtocolMessage::ExecExit(
                            zz_protocol::ExecExit {
                                server_id: shared.server_id,
                                outcome: zz_protocol::ExecOutcome::Ran,
                            },
                        ));
                        return MessageProgress::Done;
                    }
                    let context = self.context.as_mut().expect("client context");
                    sync_context_with_attachment(&shared.inner.lock(), self.client, context);
                    let task = wait_queue::task_command(&commands.as_slice()[0].invocation);
                    if inline {
                        let progress = if exec::output_pending(outbound) {
                            Some(MessageProgress::Output)
                        } else if !inline_task_or_query(
                            shared,
                            context,
                            &commands.as_slice()[0],
                            task,
                        ) {
                            Some(MessageProgress::Worker)
                        } else {
                            None
                        };
                        if let Some(progress) = progress {
                            self.request = Some(PendingMessage::Compact {
                                commands,
                                request_id,
                            });
                            return progress;
                        }
                    }
                    let command = commands.next().unwrap();
                    let last = commands.as_slice().is_empty();
                    if (task || !inline) && command.result == PreparedCommandResult::Ready {
                        if self.hello.kind == ClientKind::Control {
                            let _ = outbound.enqueue_reliable(&Shared::event(
                                EventPayload::ControlCommandStarted {
                                    request_id,
                                    flags: u32::from(if command.invocation.source.is_some() {
                                        CONTROL_COMMAND_FRAME_FLAGS_CONTROL
                                    } else {
                                        CONTROL_COMMAND_FRAME_FLAGS_NONE
                                    }),
                                    canonical_name: command.canonical_name.clone(),
                                    guard: !MuxEngine::is_command_alias_group(&command.invocation),
                                },
                            ));
                        }
                        match wait_queue::CommandTask::new(
                            shared,
                            self.client,
                            self.hello.kind,
                            context,
                            request_id,
                            &command.invocation,
                            true,
                        ) {
                            Ok(task) => {
                                self.task = Some(Box::new(task));
                                self.task_after = TaskAfter::Compact { last };
                                self.request = Some(PendingMessage::Compact {
                                    commands,
                                    request_id: request_id.saturating_add(1),
                                });
                                continue;
                            }
                            Err(response) => {
                                let _ = outbound
                                    .enqueue_reliable(&ProtocolMessage::CommandResponse(response));
                                let _ = outbound.enqueue_reliable(&ProtocolMessage::ExecExit(
                                    zz_protocol::ExecExit {
                                        server_id: shared.server_id,
                                        outcome: zz_protocol::ExecOutcome::Ran,
                                    },
                                ));
                                return MessageProgress::Done;
                            }
                        }
                    }
                    if shared.execute_compact_command(
                        self.client,
                        self.hello.kind,
                        context,
                        command,
                        (request_id, last),
                        outbound,
                        inline.then_some(true),
                    ) {
                        return MessageProgress::Done;
                    }
                    self.request = Some(PendingMessage::Compact {
                        commands,
                        request_id: request_id.saturating_add(1),
                    });
                    if !inline {
                        return MessageProgress::Ready;
                    }
                }
                PendingMessage::Message(message) => {
                    if let ProtocolMessage::CommandRequest(request) = &message {
                        let prepared = if request.prepared {
                            Ok(request.command.clone())
                        } else {
                            resolve_and_prepare_command(
                                &shared.inner.lock().engine,
                                &request.command,
                            )
                            .map(|(command, _)| command)
                        };
                        if let Ok(command) = prepared
                            && (wait_queue::task_command(&command) || !inline)
                        {
                            match wait_queue::CommandTask::new(
                                shared,
                                self.client,
                                self.hello.kind,
                                self.context.as_ref().expect("client context"),
                                request.request_id,
                                &command,
                                true,
                            ) {
                                Ok(task) => {
                                    self.task = Some(Box::new(task));
                                    self.task_after = TaskAfter::Request;
                                    continue;
                                }
                                Err(response) => {
                                    let _ = outbound.enqueue_reliable(
                                        &ProtocolMessage::CommandResponse(response),
                                    );
                                    return MessageProgress::Done;
                                }
                            }
                        }
                    }
                    if inline {
                        let ready = if let ProtocolMessage::CommandRequest(request) = &message {
                            if exec::output_pending(outbound) {
                                self.request = Some(PendingMessage::Message(message));
                                return MessageProgress::Output;
                            }
                            let context = self.context.as_mut().expect("client context");
                            sync_context_with_attachment(
                                &shared.inner.lock(),
                                self.client,
                                context,
                            );
                            let prepared = if request.prepared {
                                vec![PreparedCommand {
                                    invocation: request.command.clone(),
                                    canonical_name: (!MuxEngine::is_command_alias_group(
                                        &request.command,
                                    ))
                                    .then(|| canonical_command(&request.command.name).to_owned()),
                                    alias_matched: false,
                                    result: PreparedCommandResult::Ready,
                                }]
                            } else {
                                Shared::prepare_command_list_with_engine(
                                    &shared.inner.lock().engine,
                                    vec![request.command.clone()],
                                    self.hello.kind == ClientKind::Command,
                                )
                            };
                            prepared
                                .iter()
                                .all(|command| inline_query(shared, context, command))
                        } else {
                            matches!(
                                message,
                                ProtocolMessage::HistoryRequest { .. }
                                    | ProtocolMessage::Input(InputMessage::MouseKey { .. })
                            )
                        };
                        if !ready {
                            self.request = Some(PendingMessage::Message(message));
                            return MessageProgress::Worker;
                        }
                    }
                    self.message(shared, outbound, message);
                    return if self.input_wait.as_ref().is_some_and(|wait| !wait.ready()) {
                        MessageProgress::Wait
                    } else {
                        self.input_wait = None;
                        MessageProgress::Done
                    };
                }
            }
        }
        MessageProgress::Done
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
                let item = if matches!(&input, InputMessage::MouseKey { .. }) {
                    shared.command_item(None)
                } else {
                    Arc::clone(shared)
                };
                if let Err(error) = item.input(client, hello.kind, context, input) {
                    let _ = outbound.enqueue_reliable(&ProtocolMessage::CommandResponse(
                        CommandResponse::Error {
                            request_id: 0,
                            error: daemon_server_error(error),
                            output: RawText::default(),
                        },
                    ));
                }
                self.input_wait = item.command_item.as_ref().and_then(|item| {
                    item.lock()
                        .pending_wait
                        .take()
                        .map(|wait| wait.continuation)
                });
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
            | ProtocolMessage::AgentAcknowledgePromptRestore { .. }
            | ProtocolMessage::AgentAnswerQuestion { .. }
            | ProtocolMessage::AgentStopTask { .. }) => {
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
            if self.registration.shared.loop_active.load(Ordering::Acquire) {
                self.registration
                    .shared
                    .lifecycle
                    .release(self.client, false);
                self.registration.shared.accept_wake.wake();
                self.registration.armed = false;
            } else {
                self.registration.shared.detach(self.client);
                self.registration.unregister();
            }
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
    inline_task_or_query(
        shared,
        context,
        command,
        wait_queue::task_command(&command.invocation),
    )
}

fn inline_task_or_query(
    shared: &Shared,
    context: &ExecutionContext,
    command: &PreparedCommand,
    task: bool,
) -> bool {
    command.result == PreparedCommandResult::Ready
        && (task || ctrl::control_query_can_defer_wakeup(&shared.inner.lock(), context, command))
}
