use super::*;

fn terminal_read_command(name: &str) -> bool {
    matches!(
        name,
        "capture-pane"
            | "show-last-output"
            | "send-last-output"
            | "send-text"
            | "wait-pane"
            | "run-pane"
            | "agent-send"
            | "agent-respond"
            | "new-agent-session"
            | "capture-browser"
    )
}

pub(super) fn queue_command(command: &CommandInvocation) -> bool {
    let name = canonical_command(&command.name);
    command_stdin_sink(name, &command.args).is_some()
        || terminal_read_command(name)
        || MuxEngine::is_command_alias_group(command)
        || matches!(
            name,
            "split-window"
                | "wait-for"
                | "run-shell"
                | "if-shell"
                | "display-panes"
                | "display-menu"
                | "display-popup"
                | "command-prompt"
                | "confirm-before"
                | "load-buffer"
                | "save-buffer"
                | "source-file"
        )
}

pub(super) fn task_command(command: &CommandInvocation) -> bool {
    let name = canonical_command(&command.name);
    command_stdin_sink(name, &command.args).is_some()
        || MuxEngine::is_command_alias_group(command)
        || terminal_read_command(name)
        || matches!(
            name,
            "split-window"
                | "wait-for"
                | "run-shell"
                | "if-shell"
                | "display-panes"
                | "display-menu"
                | "display-popup"
                | "command-prompt"
                | "confirm-before"
                | "load-buffer"
                | "save-buffer"
                | "source-file"
        )
}

pub(super) fn can_run_inline(
    shared: &Shared,
    context: &ExecutionContext,
    command: &CommandInvocation,
) -> bool {
    if queue_command(command)
        || resolve_and_prepare_command(&shared.inner.lock().engine, command)
            .is_ok_and(|(command, _)| queue_command(&command))
    {
        return true;
    }
    if task_command(command) {
        return false;
    }
    let prepared = PreparedCommand {
        invocation: command.clone(),
        canonical_name: Some(canonical_command(&command.name).to_owned()),
        alias_matched: false,
        result: PreparedCommandResult::Ready,
    };
    connection::inline_query(shared, context, &prepared)
}

pub(super) enum Progress {
    Done,
    Waiting,
    Worker,
    Ready,
}

pub(super) struct CommandTask {
    shared: Arc<Shared>,
    admission: Option<ResponseAdmissionGuard>,
    client: ClientId,
    kind: ClientKind,
    request_id: u64,
    command: CommandInvocation,
    client_name: String,
    previous_control_target: Option<(ClientId, u8)>,
    frames: Vec<InsertedQueueFrame<Box<CommandQueueExecution>>>,
    finished: Option<InsertedQueueFrame<Box<CommandQueueExecution>>>,
}

impl CommandTask {
    pub(super) fn new(
        shared: &Arc<Shared>,
        client: ClientId,
        kind: ClientKind,
        context: &ExecutionContext,
        request_id: u64,
        command: &CommandInvocation,
        prepared: bool,
    ) -> Result<Self, CommandResponse> {
        Self::new_with_log(
            shared, client, kind, context, request_id, command, prepared, true,
        )
    }

    pub(super) fn default_attach(
        shared: &Arc<Shared>,
        client: ClientId,
        context: &ExecutionContext,
    ) -> Result<Self, CommandResponse> {
        Self::new_with_log(
            shared,
            client,
            ClientKind::Command,
            context,
            0,
            &CommandInvocation::new("new-session", ["-A", "-d"]),
            true,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_log(
        shared: &Arc<Shared>,
        client: ClientId,
        kind: ClientKind,
        context: &ExecutionContext,
        request_id: u64,
        command: &CommandInvocation,
        prepared: bool,
        log_command: bool,
    ) -> Result<Self, CommandResponse> {
        let Some(admission) = ResponseAdmissionGuard::new(shared) else {
            let ProtocolMessage::CommandResponse(response) = server_stopping_response(request_id)
            else {
                unreachable!()
            };
            return Err(response);
        };
        let item =
            shared.command_item((kind == ClientKind::Control).then_some((client, request_id)));
        {
            let mut context = item.command_item.as_ref().unwrap().lock();
            context.loop_wait = true;
            #[cfg(unix)]
            if kind == ClientKind::Command {
                context.exec_writer = shared
                    .client_writers
                    .lock()
                    .get(&client)
                    .map(Arc::downgrade);
            }
        }
        let stdin_available = kind == ClientKind::Command && command.stdin_available();
        let (command, client_name) = {
            let mut inner = shared.inner.lock();
            let command = match prepare_command_request(&mut inner, client, command, prepared) {
                Ok((command, false)) => command,
                Ok((_, true)) => {
                    return Err(CommandResponse::Error {
                        request_id,
                        error: ServerError::InvalidCommand("client is read-only".to_owned()),
                        output: RawText::default(),
                    });
                }
                Err(error) => {
                    return Err(CommandResponse::Error {
                        request_id,
                        error,
                        output: RawText::default(),
                    });
                }
            };
            let client_name = server_log_client_name(&inner, client);
            if log_command {
                push_server_message(
                    &mut inner,
                    format!("{client_name} command: {}", command_log_line(&command)),
                );
            }
            if kind == ClientKind::Command
                || kind == ClientKind::Control && command.source.is_none()
            {
                inner
                    .client_entry(client)
                    .command_streams
                    .replace(CommandStreams {
                        stdin_available,
                        stdin: (kind == ClientKind::Command)
                            .then(|| command.stdin().cloned().map(SourceStream::Bytes))
                            .flatten(),
                        ..CommandStreams::default()
                    });
            }
            (command, client_name)
        };
        let mut context = context.clone();
        let previous_control_target = context.control_command_target();
        if kind == ClientKind::Control {
            context.set_control_command_target(Some((
                client,
                if command.source.is_none() {
                    CONTROL_COMMAND_FRAME_FLAGS_NONE
                } else {
                    CONTROL_COMMAND_FRAME_FLAGS_CONTROL
                },
            )));
        }
        let execution = item.inserted_child_execution(None, false, None);
        execution.frame_active.set(true);
        let root = InsertedQueueFrame {
            request_root: true,
            request_result: None,
            wait_boundary: None,
            execution: Box::new(execution),
            context,
            commands: vec![command.clone()].into_iter(),
            prepared: true,
            control_target: None,
            mux_source: MuxOptionSource::RuntimeCommand,
            alias_terminal: None,
            stdin: None,
            carried_a_stream: false,
            label: "<request>".to_owned(),
            result: InsertedCommandResult::default(),
            stdout_claim: StdoutClaim::None,
            first_error: None,
            failed_group: None,
            boundary: None,
            terminal_error: None,
            parked_boundary: None,
            hook: None,
            events: None,
        };
        Ok(Self {
            shared: item,
            admission: Some(admission),
            client,
            kind,
            request_id,
            command,
            client_name,
            previous_control_target,
            frames: vec![root],
            finished: None,
        })
    }

    pub(super) fn ready(&self) -> bool {
        self.shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .as_ref()
            .is_none_or(|wait| wait.continuation.ready())
    }

    pub(super) fn run(&mut self, inline: bool) -> Progress {
        let _deferral =
            (self.kind == ClientKind::Control).then(|| self.shared.defer_control_notifications());
        let finished = self.shared.run_inserted_queue_frames(
            self.client,
            self.kind,
            &mut self.frames,
            inline,
            true,
        );
        if self.kind == ClientKind::Control {
            self.shared.publish_deferred_control_notifications();
        }
        if let Some((mut frame, _)) = finished {
            if frame.request_result.is_none() {
                frame.request_result = Some(Ok(Execution::default()));
            }
            self.shared
                .finish_command_queue_execution(&frame.execution, None);
            self.finished = Some(frame);
            Progress::Done
        } else if self
            .shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .is_some()
        {
            Progress::Waiting
        } else if inline {
            Progress::Worker
        } else {
            Progress::Ready
        }
    }

    pub(super) fn finish(
        mut self,
    ) -> (
        CommandResponse,
        bool,
        ExecutionContext,
        Option<ResponseAdmissionGuard>,
    ) {
        let mut frame = self.finished.take().expect("finished command task");
        frame
            .context
            .set_control_command_target(self.previous_control_target);
        let (response, client_exit) = self.shared.finish_command_request_segment(
            self.client,
            self.kind,
            &frame.context,
            self.request_id,
            &self.command,
            &self.client_name,
            frame.request_result.take().unwrap(),
        );
        let mut item = self.shared.command_item.as_ref().unwrap().lock();
        item.result = Some((response.clone(), client_exit));
        item.finish();
        (response, client_exit, frame.context, self.admission.take())
    }
}

impl Drop for CommandTask {
    fn drop(&mut self) {
        let pending = self
            .shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .take();
        if let Some(wait) = pending {
            pane_exit::cancel_token(&mut self.shared.inner.lock(), wait.continuation.token);
            self.shared.wake_wait_items([wait.continuation.clone()]);
            (self.shared.terminal_requests.notifier())();
            let next = remove_wait_item(
                &mut self.shared.inner.lock(),
                &wait.name,
                wait.continuation.token,
                true,
            );
            self.shared.wake_wait_items(next);
        }
    }
}

pub(super) type InsertedCompletion = Box<
    dyn FnOnce(&Arc<Shared>, &ExecutionContext, Result<InsertedCommandResult, DaemonError>) + Send,
>;

pub(super) struct InsertedTask {
    shared: Arc<Shared>,
    client: ClientId,
    kind: ClientKind,
    frames: Vec<InsertedQueueFrame<Box<CommandQueueExecution>>>,
    completion: Option<InsertedCompletion>,
}

impl InsertedTask {
    pub(super) fn new(
        shared: &Arc<Shared>,
        client: ClientId,
        kind: ClientKind,
        context: ExecutionContext,
        source: &InsertedCommandSource,
        label: &str,
        control_target: Option<(ClientId, u8)>,
        detached: bool,
        blocker: Option<ShutdownBlocker>,
        completion: InsertedCompletion,
    ) -> Result<Self, (DaemonError, InsertedCompletion)> {
        let shared = shared.command_item(None);
        shared.command_item.as_ref().unwrap().lock().loop_wait = true;
        let execution = shared.inserted_child_execution(None, detached, blocker);
        let frame = match shared.prepare_inserted_queue_frame(
            context,
            source,
            label,
            control_target,
            MuxOptionSource::RuntimeCommand,
            Box::new(execution),
            None,
            None,
        ) {
            Ok(frame) => frame,
            Err(error) => return Err((error, completion)),
        };
        Ok(Self {
            shared,
            client,
            kind,
            frames: vec![frame],
            completion: Some(completion),
        })
    }

    pub(super) fn ready(&self) -> bool {
        self.shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .as_ref()
            .is_none_or(|wait| wait.continuation.ready())
    }

    pub(super) fn run(&mut self, inline: bool) -> Progress {
        if self.frames.is_empty() {
            return Progress::Done;
        }
        if let Some((frame, result)) = self.shared.run_inserted_queue_frames(
            self.client,
            self.kind,
            &mut self.frames,
            inline,
            true,
        ) {
            let reported = frame.execution.reported_failures.get();
            self.shared
                .finish_command_queue_execution(&frame.execution, None);
            let result = if reported
                && !frame.execution.detached
                && frame.execution.deferred_shutdown.get() != DeferredShutdown::Force
                && matches!(self.kind, ClientKind::Command | ClientKind::Control)
            {
                result.and_then(|result| {
                    Err(DaemonError::ReportedCommandExit {
                        output: result.output,
                        exit_code: 1,
                    })
                })
            } else {
                result
            };
            self.completion.take().unwrap()(&self.shared.server_owner(), &frame.context, result);
            Progress::Done
        } else if self
            .shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .is_some()
        {
            Progress::Waiting
        } else if inline {
            Progress::Worker
        } else {
            Progress::Ready
        }
    }
}

impl Drop for InsertedTask {
    fn drop(&mut self) {
        let pending = self
            .shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .take();
        if let Some(wait) = pending {
            pane_exit::cancel_token(&mut self.shared.inner.lock(), wait.continuation.token);
            self.shared.wake_wait_items([wait.continuation.clone()]);
            (self.shared.terminal_requests.notifier())();
            let next = remove_wait_item(
                &mut self.shared.inner.lock(),
                &wait.name,
                wait.continuation.token,
                true,
            );
            self.shared.wake_wait_items(next);
        }
    }
}

#[cfg(unix)]
impl Shared {
    pub(super) fn enqueue_inserted_task(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &ExecutionContext,
        source: &InsertedCommandSource,
        label: &str,
        control_target: Option<(ClientId, u8)>,
        detached: bool,
        blocker: Option<ShutdownBlocker>,
        completion: wait_queue::InsertedCompletion,
    ) {
        match wait_queue::InsertedTask::new(
            self,
            client,
            kind,
            context.clone(),
            source,
            label,
            control_target,
            detached,
            blocker,
            completion,
        ) {
            Ok(task) => {
                self.pending_wait_queues.lock().push(task);
                self.accept_wake.wake();
            }
            Err((error, completion)) => completion(self, context, Err(error)),
        }
    }
}
