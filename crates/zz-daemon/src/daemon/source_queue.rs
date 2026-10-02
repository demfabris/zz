use super::*;

pub(super) struct CommandFinish {
    pub(super) command: CommandInvocation,
    pub(super) routed: CommandInvocation,
    pub(super) group: Option<(String, u32)>,
    pub(super) previous_replay_client: Option<ClientId>,
    pub(super) previous_control_target: Option<(ClientId, u8)>,
    pub(super) early_shell_guard: bool,
    pub(super) guard_capture: Option<ControlCommandEventCapture>,
    pub(super) callback_parse_failures_start: usize,
    pub(super) deferred_replay_issues_start: usize,
    pub(super) stdout_sequence: Option<usize>,
    pub(super) alias_group: bool,
    pub(super) caller_source_stream: bool,
}

pub(super) struct SourceRead {
    pub(super) pending: VecDeque<PendingConfigFile>,
    pub(super) parsed: VecDeque<(PendingConfigFile, PreparedConfig, Vec<String>)>,
    pub(super) command: CommandInvocation,
    pub(super) group: Option<(String, u32)>,
    pub(super) source_error_group: Option<usize>,
    pub(super) source_error: Option<DaemonError>,
    pub(super) source_command_error: bool,
    pub(super) source_has_file: bool,
}

struct Frame {
    execution: Option<Box<CommandQueueExecution>>,
    path: PathBuf,
    context: ExecutionContext,
    depth: usize,
    options: SourceFileLoadOptions,
    commands: VecDeque<PreparedConfigCommand>,
    finish_report: bool,
    failed_group: Option<(String, u32)>,
    source: Option<Box<ConfigSourceBoundary>>,
    read: Option<Box<SourceRead>>,
    suppressed_control_capture: Option<ControlCommandEventCapture>,
}

struct Active {
    commands: Vec<InsertedQueueFrame<Box<CommandQueueExecution>>>,
    finish: CommandFinish,
}

pub(super) struct Read {
    result: Arc<Mutex<Option<Result<Vec<u8>, DaemonError>>>>,
}

impl Read {
    pub(super) fn start(shared: &Arc<Shared>, path: &Path) -> Self {
        let result = Arc::new(Mutex::new(None));
        let delivered = Arc::clone(&result);
        let wait = terminal_requests::CommandWait::new(shared);
        let state = wait.start();
        let failed = Arc::clone(&state);
        if let Err(error) = shared.submit_helper(helpers::Task::SourceRead {
            path: path.to_owned(),
            complete: Box::new(move |_, bytes| {
                *delivered.lock() = Some(bytes);
                state.resolve(Ok(Execution::default()));
            }),
        }) {
            *result.lock() = Some(Err(error.into()));
            failed.resolve(Ok(Execution::default()));
        }
        let _ = wait.finish(shared, Execution::default());
        Self { result }
    }

    pub(super) fn finish(&self, shared: &Arc<Shared>) -> Result<Vec<u8>, DaemonError> {
        if let Some(item) = &shared.command_item {
            let pending = item.lock().pending_wait.take();
            if let Some(wait) = pending {
                assert!(wait.continuation.ready());
                item.lock().resume(wait.continuation.token);
            }
        }
        self.result.lock().take().expect("completed config read")
    }
}

pub(super) fn parse_read(
    shared: &Arc<Shared>,
    pending: &PendingConfigFile,
    bytes: Result<Vec<u8>, DaemonError>,
    report: &mut ConfigLoadReport,
) -> Result<Option<PreparedConfig>, DaemonError> {
    report.control_guarded |= pending.options.control_target.is_some();
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(DaemonError::Io(error))
            if error.kind() == ErrorKind::NotFound && pending.options.control_target.is_none() =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    shared
        .parse_config_input(
            &pending.path,
            ConfigInput::sourced(&bytes),
            report,
            pending.options,
            true,
        )
        .map(Some)
        .ok_or_else(|| non_utf8_config_error().into())
}

pub(super) struct Replay {
    frames: Vec<Frame>,
    active: Option<Active>,
    read: Option<Read>,
    pub(super) report: ConfigLoadReport,
    pub(super) invocations: SourceInvocationAccounting,
    pub(super) warnings: Vec<DeferredControlConfigWarning>,
    terminal: ClientTerminal,
    base: Option<PathBuf>,
    completed_context: Option<ExecutionContext>,
}

impl Replay {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        shared: &Arc<Shared>,
        pending: PendingConfigFile,
        parsed: PreparedConfig,
        mut report: ConfigLoadReport,
        invocations: SourceInvocationAccounting,
        mut warnings: Vec<DeferredControlConfigWarning>,
        terminal: ClientTerminal,
        base: Option<PathBuf>,
        parent: &CommandQueueExecution,
    ) -> Self {
        let mut context = pending.context;
        let (commands, finish_report) = shared.prepare_config_queue_frame(
            &pending.path,
            parsed,
            &mut context,
            &mut report,
            pending.options,
            &mut warnings,
        );
        let frame = Frame {
            execution: Some(Box::new(shared.config_child_execution(Some(parent)))),
            path: pending.path,
            context,
            depth: 0,
            options: pending.options,
            commands,
            finish_report,
            failed_group: None,
            source: None,
            read: None,
            suppressed_control_capture: shared.config_frame_capture(pending.options),
        };
        Self {
            frames: vec![frame],
            active: None,
            read: None,
            report,
            invocations,
            warnings,
            terminal,
            base,
            completed_context: None,
        }
    }

    pub(super) fn run(
        &mut self,
        shared: &Arc<Shared>,
        parent: &CommandQueueExecution,
    ) -> Option<Result<(), DaemonError>> {
        let _key_table_hold = timers::KeyTablePublishHold::enter(shared);
        loop {
            let frame = self.frames.last_mut().expect("config frame");
            let mut action = None;
            if let Some(mut active) = self.active.take() {
                let Some((mut finished, _)) = shared.run_inserted_queue_frames(
                    ClientId(u64::MAX),
                    ClientKind::Command,
                    &mut active.commands,
                    false,
                    false,
                ) else {
                    self.active = Some(active);
                    return None;
                };
                frame.context = finished.context;
                frame.execution = Some(finished.execution);
                action = Some(
                    shared.finish_config_frame_command(
                        &frame.path,
                        &mut frame.context,
                        &mut self.report,
                        frame.options,
                        frame.execution.as_ref().unwrap(),
                        &mut frame.failed_group,
                        active.finish,
                        finished
                            .request_result
                            .take()
                            .unwrap_or(Ok(Execution::default())),
                    ),
                );
            }
            if let Some(mut sources) = frame.read.take() {
                if let Some(read) = self.read.take() {
                    let pending = sources.pending.pop_front().unwrap();
                    let diagnostics_start = self.report.diagnostics().len();
                    match parse_read(shared, &pending, read.finish(shared), &mut self.report) {
                        Ok(Some(parsed)) => sources.parsed.push_back((
                            pending,
                            parsed,
                            self.report.diagnostics()[diagnostics_start..].to_vec(),
                        )),
                        Err(DaemonError::Io(error)) => {
                            let warning = if frame.options.control_target.is_some()
                                || self.invocations.is_startup()
                            {
                                source_read_error_warning(&pending.path, &error)
                            } else {
                                source_glob_error_warning(&pending.path, &error.to_string())
                            };
                            self.report.note_located_source_error(
                                &sources.command,
                                &mut sources.source_error_group,
                                &warning,
                            );
                            if let Some(target) = frame.options.control_target {
                                shared.publish_control_source_read_error(
                                    target,
                                    pending.context.pane,
                                    error.kind(),
                                    warning,
                                );
                            }
                        }
                        Err(error) if sources.source_error.is_none() => {
                            sources.source_error = Some(error);
                        }
                        Ok(None) => self.report.note_startup_command_cause(
                            &sources.command,
                            &missing_source_error(&pending.path),
                        ),
                        Err(_) => {}
                    }
                }
                if let Some(pending) = sources.pending.front() {
                    self.read = Some(Read::start(shared, &pending.path));
                    frame.read = Some(sources);
                    return None;
                }
                frame.source = Some(Box::new(ConfigSourceBoundary {
                    token: frame
                        .execution
                        .as_ref()
                        .unwrap()
                        .item
                        .wait()
                        .expect("source token"),
                    children: sources.parsed,
                    command: sources.command,
                    group: sources.group,
                    source_error_group: sources.source_error_group,
                    source_error: sources.source_error,
                    source_command_error: sources.source_command_error,
                    source_has_file: sources.source_has_file,
                    warnings: Vec::new(),
                }));
            }
            if let Some(mut source) = frame.source.take() {
                if let Some((pending, parsed, diagnostics)) = source.children.pop_front() {
                    if let Some((client, _)) = pending.options.control_target {
                        source.warnings.extend(
                            diagnostics
                                .into_iter()
                                .map(|text| DeferredControlConfigWarning { client, text }),
                        );
                    }
                    let execution =
                        shared.config_child_execution(Some(frame.execution.as_ref().unwrap()));
                    let mut context = pending.context;
                    let (commands, finish_report) = shared.prepare_config_queue_frame(
                        &pending.path,
                        parsed,
                        &mut context,
                        &mut self.report,
                        pending.options,
                        &mut source.warnings,
                    );
                    let child = Frame {
                        execution: Some(Box::new(execution)),
                        path: pending.path,
                        context,
                        depth: frame.depth + 1,
                        options: pending.options,
                        commands,
                        finish_report,
                        failed_group: None,
                        source: None,
                        read: None,
                        suppressed_control_capture: shared.config_frame_capture(pending.options),
                    };
                    frame.source = Some(source);
                    hook_events::release_input_change_window(shared);
                    self.frames.push(child);
                    continue;
                }
                assert!(frame.execution.as_ref().unwrap().item.resume(source.token));
                self.report.pop_stdout_frame();
                shared.publish_deferred_control_config_warnings(source.warnings);
                shared.publish_control_source_complete(frame.options.control_target);
                if source.source_command_error && !source.source_has_file {
                    frame.failed_group = source.group;
                }
                if let Some(error) = source.source_error {
                    action = Some(Err(error));
                }
            }
            let action = action.unwrap_or_else(|| {
                frame
                    .commands
                    .pop_front()
                    .map_or(Ok(ConfigFrameAction::Finish), |prepared| {
                        let execution = frame.execution.as_ref().unwrap();
                        execution.frame_active.set(true);
                        shared.execute_config_frame_command(
                            &frame.path,
                            &mut frame.context,
                            frame.depth,
                            &mut self.report,
                            self.terminal,
                            self.base.as_deref(),
                            &mut self.invocations,
                            frame.options,
                            execution,
                            &mut frame.failed_group,
                            prepared,
                        )
                    })
            });
            match action {
                Ok(ConfigFrameAction::Command(started)) => {
                    let (finish, result) = *started;
                    let execution = frame.execution.take().unwrap();
                    let mut root = file_commands::root(
                        std::mem::take(&mut frame.context),
                        finish.routed.clone(),
                        execution,
                        self.terminal,
                        MuxOptionSource::TmuxConfig,
                    );
                    let command = root.commands.next().unwrap();
                    let group = command
                        .source
                        .as_ref()
                        .map_or(InsertedPhysicalGroup::Unlocated, |source| {
                            InsertedPhysicalGroup::Source(source.source.clone(), source.line)
                        });
                    let boundary = InsertedCommandBoundary {
                        command,
                        group,
                        callback_failures_start: finish.callback_parse_failures_start,
                        command_control_target: None,
                        stdout_sequence: finish.stdout_sequence.unwrap_or(0),
                    };
                    let step = (result, None, false, None, false);
                    let child = if shared
                        .command_item
                        .as_ref()
                        .unwrap()
                        .lock()
                        .pending_wait
                        .is_some()
                    {
                        root.wait_boundary = Some((boundary, step));
                        None
                    } else {
                        shared.settle_inserted_frame_step(
                            ClientId(u64::MAX),
                            ClientKind::Command,
                            &mut root,
                            boundary,
                            step,
                        )
                    };
                    let mut commands = vec![root];
                    commands.extend(child);
                    self.active = Some(Active { commands, finish });
                    continue;
                }
                Ok(ConfigFrameAction::ReadSource(sources)) => {
                    frame.read = Some(sources);
                    continue;
                }
                Ok(ConfigFrameAction::Continue) => continue,
                Ok(ConfigFrameAction::Source(source)) => {
                    frame.source = Some(source);
                    continue;
                }
                Ok(ConfigFrameAction::Finish) | Err(_) => {}
            }
            if action.is_ok() && frame.finish_report {
                shared.finish_config_queue_frame(&frame.context, frame.options, &mut self.report);
            }
            let mut finished = self.frames.pop().unwrap();
            finished.suppressed_control_capture.take();
            let execution = finished.execution.take().unwrap();
            self.report.reported_failure |= execution.reported_failures.get();
            if let Some(parent_frame) = self.frames.last_mut() {
                shared.finish_command_queue_execution(
                    &execution,
                    Some(parent_frame.execution.as_ref().unwrap()),
                );
                if let Err(error) = action {
                    shared.record_config_child_error(
                        &finished.path,
                        &finished.context,
                        parent_frame.options,
                        &self.invocations,
                        &mut self.report,
                        parent_frame.source.as_mut().unwrap(),
                        error,
                    );
                }
            } else {
                shared.finish_command_queue_execution(&execution, Some(parent));
                self.completed_context = Some(finished.context);
                return Some(action.map(|_| ()));
            }
        }
    }
}

pub(super) struct SourceExecution {
    pub(super) client: ClientId,
    pub(super) kind: ClientKind,
    pub(super) execution: Execution,
    pub(super) source_client: ClientId,
    pub(super) source_kind: ClientKind,
    pub(super) source_client_terminal: ClientTerminal,
    pub(super) source_client_base: Option<PathBuf>,
    pub(super) control_target: Option<(ClientId, u8)>,
    pub(super) control_client: Option<ClientId>,
    pub(super) source_file_error: Option<DaemonError>,
    pub(super) source_path_error: bool,
    pub(super) source_path_matched: bool,
    pub(super) control_source_errors: Vec<String>,
    pub(super) control_source_matched: bool,
    pub(super) source_verbose_output: RawText,
    pub(super) source_replay_output: RawText,
    pub(super) source_diagnostics_output: RawText,
    pub(super) source_invocations: SourceInvocationAccounting,
    pub(super) source_invocation: bool,
    pub(super) reported_source_failure: bool,
    pub(super) reported_source_callback_failure: bool,
    pub(super) control_source_invocation: bool,
    pub(super) suppress_source_replay_output: bool,
    pub(super) reload_config: bool,
    pub(super) read_only: bool,
    pub(super) client_timers_changed: bool,
    pub(super) status_formats_changed: bool,
    pub(super) status_refresh_sessions: BTreeSet<SessionId>,
    pub(super) pending_hook_events: Vec<PendingHookEvent>,
    pub(super) notifications_only: bool,
    pub(super) incremental_start: Option<CommandPromptSubmission>,
    pub(super) terminal_wait: Option<terminal_requests::CommandWait>,
    pub(super) pending: VecDeque<PendingConfigFile>,
    pub(super) parsed: VecDeque<(PendingConfigFile, Option<PreparedConfig>, ConfigLoadReport)>,
    pub(super) read: Option<Read>,
    pub(super) replay: Option<Replay>,
    pub(super) replay_pending: Option<PendingConfigFile>,
    pub(super) deferred_control_config_warnings: Vec<DeferredControlConfigWarning>,
}

impl SourceExecution {
    pub(super) fn run(
        mut self,
        shared: &Arc<Shared>,
        context: &mut ExecutionContext,
        queue_execution: Option<&CommandQueueExecution>,
    ) -> Result<Execution, DaemonError> {
        loop {
            if let Some(pending) = self.pending.front() {
                let mut report =
                    if self.control_target.is_some() || self.source_client == ClientId(u64::MAX) {
                        ConfigLoadReport::default()
                    } else {
                        ConfigLoadReport::with_stdout_transcript()
                    };
                let parsed = if let Some(stream) = &pending.stdin {
                    shared
                        .parse_config_input(
                            &pending.path,
                            ConfigInput::sourced(stream.as_bytes()),
                            &mut report,
                            pending.options,
                            true,
                        )
                        .map(Some)
                        .ok_or_else(|| DaemonError::from(non_utf8_config_error()))
                } else if let Some(read) = self.read.take() {
                    parse_read(shared, pending, read.finish(shared), &mut report)
                } else {
                    self.read = Some(Read::start(shared, &pending.path));
                    return Ok(self.suspend(shared));
                };
                let pending = self.pending.pop_front().unwrap();
                match parsed {
                    Ok(parsed) => self.parsed.push_back((pending, parsed, report)),
                    Err(DaemonError::Io(error)) => {
                        if !pending
                            .context
                            .format_variables
                            .contains_key(HOOK_CONTEXT_FORMAT)
                        {
                            self.reported_source_failure = true;
                        }
                        let warning = if self.control_target.is_some() {
                            source_read_error_warning(&pending.path, &error)
                        } else {
                            source_glob_error_warning(&pending.path, &error.to_string())
                        };
                        if let Some(target) = self.control_target {
                            shared.publish_control_source_read_error(
                                target,
                                pending.context.pane,
                                error.kind(),
                                warning,
                            );
                        } else {
                            shared.route_source_error(
                                self.source_client,
                                self.source_kind,
                                pending.context.pane,
                                &warning,
                            );
                        }
                    }
                    Err(error) if self.source_file_error.is_none() => {
                        self.source_file_error = Some(error);
                    }
                    Err(_) => {}
                }
                continue;
            }
            if let Some(mut replay) = self.replay.take() {
                let Some(replayed) = replay.run(shared, queue_execution.unwrap()) else {
                    self.replay = Some(replay);
                    return Ok(self.suspend(shared));
                };
                self.source_invocations = replay.invocations;
                self.deferred_control_config_warnings = replay.warnings;
                let mut pending = self.replay_pending.take().unwrap();
                if let Some(context) = replay.completed_context.take() {
                    pending.context = context;
                }
                self.finish_file(shared, queue_execution, pending, replay.report, replayed);
                continue;
            }
            if let Some((pending, parsed, report)) = self.parsed.pop_front() {
                if let Some((client, _)) = self.control_target {
                    self.deferred_control_config_warnings
                        .extend(report.diagnostics().iter().map(|diagnostic| {
                            DeferredControlConfigWarning {
                                client,
                                text: diagnostic.clone(),
                            }
                        }));
                }
                if let Some(parsed) = parsed {
                    self.replay_pending = Some(PendingConfigFile {
                        path: pending.path.clone(),
                        context: pending.context.clone(),
                        options: pending.options,
                        stdin: None,
                    });
                    self.replay = Some(Replay::new(
                        shared,
                        pending,
                        parsed,
                        report,
                        std::mem::take(&mut self.source_invocations),
                        std::mem::take(&mut self.deferred_control_config_warnings),
                        self.source_client_terminal,
                        self.source_client_base.clone(),
                        queue_execution.unwrap(),
                    ));
                } else {
                    self.finish_file(shared, queue_execution, pending, report, Ok(()));
                }
                continue;
            }
            return self.finish(shared, context, queue_execution);
        }
    }

    fn suspend(self, shared: &Arc<Shared>) -> Execution {
        let original = shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .take()
            .expect("config wait");
        let wrapper = RegisteredWait {
            name: original.name.clone(),
            continuation: original.continuation.clone(),
            #[cfg(unix)]
            shell: None,
            overlay: None,
            leaf: None,
            guard: None,
            terminal: None,
            file: Some(Box::new(move |shared, context, queue| {
                shared.command_item.as_ref().unwrap().lock().pending_wait = Some(original);
                self.run(shared, context, Some(queue))
            })),
        };
        shared.command_item.as_ref().unwrap().lock().pending_wait = Some(Box::new(wrapper));
        Execution::default()
    }

    fn finish_file(
        &mut self,
        shared: &Arc<Shared>,
        queue_execution: Option<&CommandQueueExecution>,
        pending: PendingConfigFile,
        mut report: ConfigLoadReport,
        replayed: Result<(), DaemonError>,
    ) {
        let PendingConfigFile {
            path,
            context,
            options,
            stdin: _,
        } = pending;
        let defer_command_error_hook_replay_issues = self.control_target.is_none()
            && queue_execution.is_some()
            && context.replay_client().is_some()
            && context
                .format_variables
                .get(HOOK_CONTEXT_FORMAT)
                .is_some_and(|hook| hook == "command-error");
        match replayed {
            Ok(()) => {}
            Err(DaemonError::Io(error)) => {
                if !context.format_variables.contains_key(HOOK_CONTEXT_FORMAT) {
                    self.reported_source_failure = true;
                }
                let warning = if self.control_target.is_some() {
                    source_read_error_warning(&path, &error)
                } else {
                    source_glob_error_warning(&path, &error.to_string())
                };
                if !options.suppress_replay_output {
                    if let Some(target) = self.control_target {
                        shared.publish_control_source_read_error(
                            target,
                            context.pane,
                            error.kind(),
                            warning,
                        );
                    } else {
                        shared.route_source_error(
                            self.source_client,
                            self.source_kind,
                            context.pane,
                            &warning,
                        );
                    }
                }
            }
            Err(error) => {
                if self.source_file_error.is_none() {
                    self.source_file_error = Some(error);
                }
            }
        }
        let hook_owned_replay = context.format_variables.contains_key(HOOK_CONTEXT_FORMAT);
        self.reported_source_callback_failure |= report.source_callback_failure;
        if report.reported_failure || !hook_owned_replay && !report.replay_issues().is_empty() {
            self.reported_source_failure = true;
            if let Some((client, _)) = self.control_target
                && let Some(streams) = shared
                    .inner
                    .lock()
                    .client_mut(client)
                    .and_then(|c| c.command_streams.as_mut())
            {
                streams.exit_code = 1;
            }
        }
        if !options.parse_only {
            shared.apply_stored_mux_config_overrides("source-file-replay");
        }
        if !options.suppress_replay_output {
            if let Some((verbose, replay)) = report.stdout_transcript() {
                if self.control_target.is_none() && self.source_kind == ClientKind::Command {
                    let claim = if report.stdout_raw_claimed() {
                        Some(StdoutClaim::Raw)
                    } else if verbose.is_empty() && replay.is_empty() {
                        None
                    } else {
                        Some(StdoutClaim::Print)
                    };
                    if let Some(claim) = claim {
                        shared.record_command_stdout_claim(self.source_client, claim);
                    }
                }
                append_inserted_output(&mut self.source_verbose_output, verbose);
                append_inserted_output(&mut self.source_replay_output, replay);
            }
            if let Some(diagnostics) = report.stdout_diagnostics() {
                append_inserted_output(&mut self.source_diagnostics_output, diagnostics);
            }
            if self.control_target.is_none() && self.source_kind == ClientKind::Command {
                for (index, diagnostic) in report.diagnostics().iter().enumerate() {
                    if !report.replayed_diagnostics.contains(&index) {
                        shared.record_command_stdout(self.source_client, diagnostic);
                    }
                    shared.record_command_failure(self.source_client);
                }
            }
            if defer_command_error_hook_replay_issues {
                queue_execution
                    .expect("deferred replay issue queue")
                    .deferred_config_replay_issues
                    .borrow_mut()
                    .extend(
                        report
                            .take_undelivered_replay_issues()
                            .into_iter()
                            .map(DeferredConfigReplayIssue::Replay),
                    );
            } else {
                if let Some((client, flags)) = self.control_target {
                    if flags == CONTROL_COMMAND_FRAME_FLAGS_CONTROL {
                        shared.route_config_replay_errors(
                            client,
                            ClientKind::Control,
                            context.pane,
                            &mut report,
                        );
                    } else {
                        report.delivered_replay_issues = report.replay_issues().len();
                    }
                } else {
                    shared.route_config_replay_errors(
                        self.source_client,
                        self.source_kind,
                        context.pane,
                        &mut report,
                    );
                }
                if self.control_target.is_none()
                    && self.source_kind == ClientKind::Command
                    && let Some(skipped) = report.skipped_summary()
                {
                    shared.record_command_stderr(self.source_client, &skipped);
                }
                let summary = if self.control_target.is_some() && report.control_guarded {
                    report.skipped_summary()
                } else {
                    report.message()
                };
                if let Some(summary) = summary {
                    shared.publish_to_client(
                        self.control_client.unwrap_or(self.source_client),
                        EventPayload::ClientMessage {
                            pane: context.pane,
                            kind: ClientMessageKind::Warning,
                            text: summary,
                        },
                    );
                }
            }
        }
    }

    fn finish(
        mut self,
        shared: &Arc<Shared>,
        context: &mut ExecutionContext,
        queue_execution: Option<&CommandQueueExecution>,
    ) -> Result<Execution, DaemonError> {
        shared.publish_deferred_control_config_warnings(self.deferred_control_config_warnings);
        if self.control_source_invocation {
            shared.publish_control_source_complete(self.control_target);
        }
        append_inserted_output(&mut self.execution.output, &self.source_verbose_output);
        append_inserted_output(&mut self.execution.output, &self.source_replay_output);
        append_inserted_output(&mut self.execution.output, &self.source_diagnostics_output);
        if !self.control_source_errors.is_empty() {
            let mut inner = shared.inner.lock();
            let target = self
                .control_target
                .expect("Control source errors have a target");
            let streams = inner
                .client_entry(target.0)
                .command_streams
                .get_or_insert_default();
            if self.control_source_matched {
                for error in self.control_source_errors {
                    streams.stdout.push_str(&error);
                    streams.stdout.push('\n');
                }
                streams.exit_code = 1;
            } else {
                streams.control_error = self.control_source_errors.join("\n");
            }
        }
        if self.reload_config
            && let Err(error) = shared.reload_user_config_with_source_base(
                self.source_client,
                context,
                self.source_client_base.as_deref(),
                SourceFileLoadOptions::default(),
            )
            && self.source_file_error.is_none()
        {
            self.source_file_error = Some(error);
        }
        if !self.read_only {
            shared.publish_key_tables_if_changed();
        }
        if self.client_timers_changed
            || self.status_formats_changed
            || !self.status_refresh_sessions.is_empty()
        {
            shared.nudge_client_timers();
        }
        self.pending_hook_events.extend(std::mem::take(
            &mut shared.inner.lock().deferred_event_hooks,
        ));
        if !self.pending_hook_events.is_empty() {
            shared.wake_control_queue(self.client, self.kind);
        }
        if std::mem::take(&mut shared.inner.lock().deferred_control_refresh) {
            shared.refresh_control_output_taps();
        }
        if self.notifications_only {
            shared.run_event_hooks(self.pending_hook_events);
        } else if let Some(queue_execution) = queue_execution {
            queue_execution
                .pending_event_hooks
                .borrow_mut()
                .extend(self.pending_hook_events);
        } else {
            shared.run_event_hooks(self.pending_hook_events);
        }
        if let Some(submission) = self.incremental_start {
            shared.submit_command_prompt(
                self.source_client,
                self.source_kind,
                context,
                &submission,
            );
        }
        if self.suppress_source_replay_output
            && self.source_path_matched
            && let Some(queue_execution) = queue_execution
        {
            queue_execution.suppress_output.set(true);
        }
        let report_sets_request_status = self.reported_source_failure
            && (self.source_kind == ClientKind::Command
                || self.control_target.is_some_and(|(_, flags)| {
                    flags == CONTROL_COMMAND_FRAME_FLAGS_NONE
                        || flags == CONTROL_COMMAND_FRAME_FLAGS_CONTROL
                            && self.reported_source_callback_failure
                }));
        if report_sets_request_status && let Some(queue_execution) = queue_execution {
            queue_execution.note_reported_failure();
        }
        match self.source_file_error {
            Some(error) => Err(prepend_command_output(self.execution.output, error)),
            None if self.source_invocation
                && report_sets_request_status
                && queue_execution.is_none()
                && matches!(self.source_kind, ClientKind::Command | ClientKind::Control) =>
            {
                Err(DaemonError::ReportedCommandExit {
                    output: self.execution.output,
                    exit_code: 1,
                })
            }
            None if self.source_path_error && !self.source_path_matched => {
                Err(DaemonError::CommandExit {
                    output: self.execution.output,
                    exit_code: 1,
                })
            }
            None => match self.terminal_wait {
                Some(wait) => wait.finish(shared, self.execution),
                None => Ok(self.execution),
            },
        }
    }
}
