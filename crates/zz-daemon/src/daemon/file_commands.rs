use super::*;

pub(super) type Completion = Box<dyn FnOnce(&Arc<Shared>, Result<Vec<u8>, DaemonError>) + Send>;
pub(super) type Resume = Box<
    dyn FnOnce(
            &Arc<Shared>,
            &mut ExecutionContext,
            &CommandQueueExecution,
        ) -> Result<Execution, DaemonError>
        + Send,
>;

#[allow(clippy::too_many_arguments)]
pub(super) fn load_buffer(
    shared: &Arc<Shared>,
    client: Option<ClientId>,
    context: &ExecutionContext,
    path: &Path,
    name: Option<String>,
    target: Option<String>,
    clipboard: bool,
) -> Result<Execution, DaemonError> {
    let wait = terminal_requests::CommandWait::new(shared);
    let state = wait.start();
    let failed = Arc::clone(&state);
    let no_hooks = context.no_hooks;
    let submitted = shared.client_file_operation(
        client,
        path,
        ClientFileOperation::Read,
        None,
        Box::new(move |shared, result| {
            let result = result.and_then(|data| {
                validate_paste_buffer_size(data.len())?;
                if data.is_empty() {
                    return Ok(Execution::default());
                }
                if let Some(name) = &name {
                    validate_paste_buffer_name(name)?;
                }
                let copied = clipboard.then(|| data.clone());
                let events = insert_paste_buffer(
                    &mut shared.inner.lock(),
                    name.as_deref(),
                    "buffer",
                    data,
                    true,
                )?;
                if let Some(data) = copied {
                    shared.deliver_buffer_clipboard_write(client, target.as_deref(), &data);
                }
                if !no_hooks {
                    shared.run_event_hooks(events);
                }
                shared.refresh_choose_buffers();
                Ok(Execution::default())
            });
            state.resolve(result);
        }),
    );
    if let Err(error) = submitted {
        failed.resolve(Err(error));
    }
    wait.finish(shared, Execution::default())
}

pub(super) fn save_buffer(
    shared: &Arc<Shared>,
    client: Option<ClientId>,
    path: &Path,
    data: &Arc<[u8]>,
    append: bool,
) -> Result<Execution, DaemonError> {
    let wait = terminal_requests::CommandWait::new(shared);
    let state = wait.start();
    let failed = Arc::clone(&state);
    if let Err(error) = shared.client_file_operation(
        client,
        path,
        ClientFileOperation::Write {
            append,
            data: data.to_vec(),
        },
        None,
        Box::new(move |_, result| {
            state.resolve(result.map(|_| Execution::default()));
        }),
    ) {
        failed.resolve(Err(error));
    }
    wait.finish(shared, Execution::default())
}

pub(super) fn read_stdin(
    shared: &Arc<Shared>,
    client: ClientId,
    sink: CommandStdinSink,
    command: &CommandInvocation,
) -> Result<(), DaemonError> {
    let wait = terminal_requests::CommandWait::new(shared);
    let state = wait.start();
    let failed = Arc::clone(&state);
    let load_buffer = canonical_command(&command.name) == "load-buffer";
    if let Some(item) = &shared.command_item
        && let Some(wait) = item.lock().pending_wait.as_mut()
    {
        wait.file = Some(Box::new(|_, _, _| Ok(Execution::default())));
    }
    if let Err(error) = shared.client_file_operation(
        Some(client),
        Path::new("-"),
        ClientFileOperation::ReadStdin {
            binary: sink.accepts_binary(),
        },
        None,
        Box::new(move |shared, result| {
            let result = result
                .or_else(|error| {
                    if (sink == CommandStdinSink::Config || load_buffer)
                        && caller_stdin_read_failure(&daemon_error_text(&error))
                    {
                        if let Some(streams) = shared
                            .inner
                            .lock()
                            .client_mut(client)
                            .and_then(|c| c.command_streams.as_mut())
                        {
                            streams.stdin_error = Some(daemon_error_text(&error));
                        }
                        Ok(Vec::new())
                    } else {
                        Err(error)
                    }
                })
                .map(|bytes| {
                    if let Some(streams) = shared
                        .inner
                        .lock()
                        .client_mut(client)
                        .and_then(|c| c.command_streams.as_mut())
                    {
                        streams.stdin = Some(if streams.stdin_error.is_some() {
                            SourceStream::Spent
                        } else {
                            SourceStream::Bytes(RawText::from_bytes(bytes))
                        });
                    }
                    Execution::default()
                });
            state.resolve(result);
        }),
    ) {
        failed.resolve(Err(error));
    }
    wait.finish(shared, Execution::default()).map(|_| ())
}

pub(super) fn stream_stdin(shared: &Arc<Shared>, client: ClientId, pane: PaneId) {
    let wait = terminal_requests::CommandWait::new(shared);
    chunk(shared, client, pane, wait.start());
    let _ = wait.finish(shared, Execution::default());
}

fn chunk(
    shared: &Arc<Shared>,
    client: ClientId,
    pane: PaneId,
    state: Arc<terminal_requests::CommandState>,
) {
    if shared.pane_stream_target_lost(pane) {
        shared.request_command_client_exit(client);
        state.resolve(Ok(Execution::default()));
        return;
    }
    let failed = Arc::clone(&state);
    if let Err(error) = shared.client_file_operation(
        Some(client),
        Path::new("-"),
        ClientFileOperation::ReadStdinChunk,
        Some(pane),
        Box::new(move |shared, result| {
            if shared.pane_stream_target_lost(pane) {
                shared.request_command_client_exit(client);
                state.resolve(Ok(Execution::default()));
            } else if let Ok(bytes) = result
                && !bytes.is_empty()
                && shared.feed_pane_stream_input(pane, &bytes)
            {
                chunk(shared, client, pane, state);
            } else {
                state.resolve(Ok(Execution::default()));
            }
        }),
    ) {
        failed.resolve(Err(error));
    }
}

pub(super) fn root(
    context: ExecutionContext,
    command: CommandInvocation,
    execution: Box<CommandQueueExecution>,
    terminal: ClientTerminal,
    mux_source: MuxOptionSource,
) -> InsertedQueueFrame<Box<CommandQueueExecution>> {
    execution.frame_active.set(true);
    InsertedQueueFrame {
        request_root: true,
        request_result: None,
        wait_boundary: None,
        execution,
        context,
        commands: vec![command].into_iter(),
        prepared: true,
        control_target: None,
        mux_source,
        alias_terminal: Some(terminal),
        stdin: None,
        carried_a_stream: false,
        label: "<config-command>".to_owned(),
        result: InsertedCommandResult::default(),
        stdout_claim: StdoutClaim::None,
        first_error: None,
        failed_group: None,
        boundary: None,
        terminal_error: None,
        parked_boundary: None,
        hook: None,
        events: None,
    }
}
