use super::*;

impl Shared {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn hand_off_from_loop(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &mut ExecutionContext,
        command: &CommandInvocation,
        name: &str,
        mux_source: MuxOptionSource,
        client_terminal: ClientTerminal,
        queue_execution: Option<&CommandQueueExecution>,
    ) -> Option<Result<Execution, DaemonError>> {
        if !matches!(
            name,
            "source-file"
                | "run-shell"
                | "if-shell"
                | "load-buffer"
                | "save-buffer"
                | "command-prompt"
                | "show-prompt-history"
                | "clear-prompt-history"
                | "reload-config"
                | "import-tmux-config"
        ) || !self.helpers.on_loop_thread()
        {
            return None;
        }
        let (loop_wait, loop_leaf) = self.command_item.as_ref().map_or((false, false), |item| {
            let item = item.lock();
            (item.loop_wait, item.loop_leaf)
        });
        if !matches!(name, "reload-config" | "import-tmux-config") {
            let framed = queue_execution.is_some_and(|queue| queue.frame_active.get());
            let suspendable = match name {
                "source-file" => loop_wait && queue_execution.is_some(),
                "run-shell" | "if-shell" => (loop_wait || loop_leaf) && framed,
                _ => loop_wait || loop_leaf,
            };
            if suspendable
                || kind != ClientKind::Interactive
                || !self.parks_on_loop(name, &command.args)
            {
                return None;
            }
            let title = name.to_owned();
            self.enqueue_inserted_task(
                client,
                kind,
                context,
                &InsertedCommandSource::Commands(vec![command.clone()]),
                &format!("<{name}>"),
                None,
                false,
                None,
                Box::new(move |shared, context, result| {
                    shared.finish_handed_off(
                        client,
                        kind,
                        context,
                        title,
                        result.map(|result| result.output),
                    );
                }),
            );
            return Some(Ok(Execution::default()));
        }
        let owner = self.server_owner();
        let mut job_context = context.clone();
        let command = command.clone();
        let run = move |owner: &Arc<Self>, context: &mut ExecutionContext| {
            owner
                .command_item(None)
                .execute_with_mux_source_routed_for_terminal_in_queue_in_item(
                    client,
                    kind,
                    context,
                    &command,
                    mux_source,
                    client_terminal,
                    None,
                )
        };
        if !(loop_wait || loop_leaf) {
            let title = name.to_owned();
            let worker = Arc::clone(&owner);
            return Some(
                owner
                    .connection_threads
                    .run(Box::new(move || {
                        let result = run(&worker, &mut job_context);
                        worker.finish_handed_off(
                            client,
                            kind,
                            &job_context,
                            title,
                            result.map(|execution| execution.output),
                        );
                    }))
                    .map(|()| Execution::default())
                    .map_err(Into::into),
            );
        }
        let wait = terminal_requests::CommandWait::new(self);
        let state = wait.start();
        let failed = Arc::clone(&state);
        let slot = Arc::new(Mutex::new(None));
        let delivered = Arc::clone(&slot);
        let worker = Arc::clone(&owner);
        if let Err(error) = owner.connection_threads.run(Box::new(move || {
            let result = run(&worker, &mut job_context);
            *delivered.lock() = Some((result, job_context));
            state.resolve(Ok(Execution::default()));
        })) {
            failed.resolve(Err(error.into()));
        }
        let _ = wait.finish(self, Execution::default());
        if let Some(wait) = self
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .as_mut()
        {
            wait.file = Some(Box::new(move |_, context, _| {
                let Some((result, worker_context)) = slot.lock().take() else {
                    return Ok(Execution::default());
                };
                *context = worker_context;
                result
            }));
        }
        Some(Ok(Execution::default()))
    }

    pub(super) fn hand_off_key_commands(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &ExecutionContext,
        commands: &[CommandInvocation],
        repeat_binding: bool,
    ) -> bool {
        if !self.helpers.on_loop_thread()
            || self.command_item.as_ref().is_some_and(|item| {
                let item = item.lock();
                item.loop_wait || item.loop_leaf
            })
            || !commands.iter().any(|command| {
                let name = canonical_command(&command.name);
                matches!(
                    name,
                    "source-file"
                        | "run-shell"
                        | "if-shell"
                        | "load-buffer"
                        | "save-buffer"
                        | "command-prompt"
                        | "show-prompt-history"
                        | "clear-prompt-history"
                        | "reload-config"
                        | "import-tmux-config"
                ) && self.parks_on_loop(name, &command.args)
            })
        {
            return false;
        }
        let mut context = context.clone();
        context.set_repeat_binding(repeat_binding);
        let title = match commands {
            [command] => command.name.clone(),
            _ => "command output".to_owned(),
        };
        self.enqueue_inserted_task(
            client,
            kind,
            &context,
            &InsertedCommandSource::Commands(commands.to_vec()),
            "<key-binding>",
            None,
            false,
            None,
            Box::new(move |shared, context, result| {
                shared.finish_handed_off(
                    client,
                    kind,
                    context,
                    title,
                    result.map(|result| result.output),
                );
            }),
        );
        true
    }

    fn parks_on_loop(&self, name: &str, args: &[RawText]) -> bool {
        match name {
            "run-shell" => parse_run_shell_args(args).is_ok_and(|parsed| !parsed.background),
            "if-shell" => {
                parse_if_shell_args(args).is_ok_and(|parsed| !parsed.background && !parsed.format)
            }
            "command-prompt" | "show-prompt-history" | "clear-prompt-history" => {
                !self.prompt_history_settled.load(Ordering::Acquire)
            }
            _ => true,
        }
    }

    fn finish_handed_off(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        context: &ExecutionContext,
        title: String,
        result: Result<RawText, DaemonError>,
    ) {
        match result {
            Ok(output) => {
                self.route_background_inserted_output(client, kind, context, title, &output);
            }
            Err(error) => {
                if let Some(output) = daemon_error_output(&error) {
                    self.route_background_inserted_output(client, kind, context, title, output);
                }
                if !matches!(
                    error,
                    DaemonError::CommandExit { .. } | DaemonError::ReportedCommandExit { .. }
                ) {
                    self.publish_background_command_error(client, context, &error, true);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "loopfix_tests.rs"]
mod tests;
