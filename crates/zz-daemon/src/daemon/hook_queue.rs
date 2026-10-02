use super::*;

struct Queue {
    shared: Arc<Shared>,
    frames: Vec<InsertedQueueFrame<Box<CommandQueueExecution>>>,
    waiting: bool,
    sender: crossbeam_channel::Sender<LeafCompletion>,
    completed: crossbeam_channel::Receiver<LeafCompletion>,
}

type LeafCompletion = (
    Box<CommandQueueExecution>,
    ExecutionContext,
    InsertedCommandStep,
);

pub(super) struct LoopHooks {
    queues: VecDeque<Queue>,
}

impl LoopHooks {
    pub(super) fn new() -> Self {
        Self {
            queues: VecDeque::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.queues.is_empty()
    }

    pub(super) fn ready(&self) -> bool {
        self.queues.front().is_some_and(|queue| {
            (!queue.waiting
                && queue
                    .shared
                    .command_item
                    .as_ref()
                    .expect("loop hook item")
                    .lock()
                    .pending_wait
                    .as_ref()
                    .is_none_or(|wait| wait.continuation.ready()))
                || !queue.completed.is_empty()
        })
    }

    pub(super) fn events(&mut self, shared: &Arc<Shared>, events: Vec<PendingHookEvent>) {
        self.enqueue(
            shared,
            ExecutionContext::default(),
            &InsertedCommandSource::Events(Box::new(EventQueueSource {
                events: RefCell::new(events),
                publish_control: true,
                shutdown_already_blocked: shared.active_shutdown_blockers() != 0,
            })),
        );
    }

    pub(super) fn monitor(
        &mut self,
        shared: &Arc<Shared>,
        context: ExecutionContext,
        commands: Vec<Vec<CommandInvocation>>,
        variables: BTreeMap<String, String>,
    ) {
        self.enqueue(
            shared,
            context,
            &InsertedCommandSource::Hooks(Box::new(HookQueueSource {
                commands: RefCell::new(commands),
                variables,
                skip_resolution_errors: true,
                initial_draining: false,
                replaying: false,
            })),
        );
    }

    #[cfg(feature = "agent")]
    pub(super) fn command(
        &mut self,
        shared: &Arc<Shared>,
        context: ExecutionContext,
        command: CommandInvocation,
    ) {
        if self.enqueue(
            shared,
            context,
            &InsertedCommandSource::Block(String::new()),
        ) {
            let queue = self.queues.back_mut().expect("enqueued peer command");
            queue.frames[0].commands = vec![command].into_iter();
        }
    }

    fn enqueue(
        &mut self,
        shared: &Arc<Shared>,
        context: ExecutionContext,
        source: &InsertedCommandSource,
    ) -> bool {
        let shared = shared.command_item(None);
        shared
            .command_item
            .as_ref()
            .expect("loop hook item")
            .lock()
            .loop_leaf = true;
        let mut execution = shared.inserted_child_execution(None, false, None);
        execution.item.yield_boundary = true;
        match shared.prepare_inserted_queue_frame(
            context,
            source,
            "<loop-hooks>",
            None,
            MuxOptionSource::RuntimeCommand,
            Box::new(execution),
            None,
            None,
        ) {
            Ok(frame) => {
                let (sender, completed) = crossbeam_channel::unbounded();
                self.queues.push_back(Queue {
                    shared,
                    frames: vec![frame],
                    waiting: false,
                    sender,
                    completed,
                });
                true
            }
            Err(error) => {
                log::debug!("could not prepare queued hook: {error}");
                false
            }
        }
    }

    pub(super) fn turn(
        &mut self,
        _shared: &Arc<Shared>,
        waker: &Arc<mio::Waker>,
    ) -> Result<(), DaemonError> {
        let count = self.queues.len().min(32);
        for _ in 0..count {
            let mut queue = self.queues.pop_front().expect("hook queue");
            queue.turn(waker)?;
            if !queue.frames.is_empty() || queue.waiting {
                self.queues.push_front(queue);
                break;
            }
        }
        Ok(())
    }
}

fn blocking_leaf(command: &CommandInvocation) -> bool {
    if canonical_command(&command.name) == "run-shell" {
        return parse_run_shell_args(&command.args).is_err();
    }
    if canonical_command(&command.name) == "if-shell" {
        return parse_if_shell_args(&command.args).is_err();
    }
    matches!(
        canonical_command(&command.name),
        "run-shell"
            | "if-shell"
            | "wait-for"
            | "source-file"
            | "display-menu"
            | "display-popup"
            | "agent-send"
            | "new-session"
            | "new-window"
            | "split-window"
            | "respawn-pane"
            | "respawn-window"
            | "load-buffer"
            | "save-buffer"
            | "show-buffer"
    )
}

impl Queue {
    fn turn(&mut self, waker: &Arc<mio::Waker>) -> Result<(), DaemonError> {
        let shared = Arc::clone(&self.shared);
        let client = ClientId(u64::MAX);
        let kind = ClientKind::Command;
        if self.waiting {
            let Ok((execution, context, step)) = self.completed.try_recv() else {
                return Ok(());
            };
            self.waiting = false;
            shared
                .command_item
                .as_ref()
                .expect("loop hook item")
                .lock()
                .loop_leaf = true;
            let frame = self.frames.last_mut().expect("waiting frame");
            frame.execution = execution;
            frame.context = context;
            let boundary = frame.parked_boundary.take().expect("parked leaf boundary");
            let child = shared.settle_inserted_frame_step(client, kind, frame, boundary, step);
            if let Some(child) = child {
                self.frames.push(child);
            }
        }
        let sender = self.sender.clone();
        let waiting = &mut self.waiting;
        let mut spawn_error = None;
        let finished = shared.advance_inserted_frames(
            client,
            kind,
            &mut self.frames,
            64,
            false,
            false,
            |frame, command, target| {
                if !blocking_leaf(command) {
                    let step =
                        shared.execute_inserted_frame_command(client, kind, frame, command, target);
                    return Some(step);
                }
                if let Some(hook) = &frame.hook {
                    frame.context.no_hooks = true;
                    frame.context.format_variables.clone_from(&hook.variables);
                }
                shared
                    .command_item
                    .as_ref()
                    .expect("loop hook item")
                    .lock()
                    .loop_leaf = false;
                let placeholder = shared.inserted_child_execution(None, false, None);
                let execution = std::mem::replace(&mut frame.execution, Box::new(placeholder));
                let mut context = frame.context.clone();
                let prepared = frame.prepared;
                let mux_source = frame.mux_source;
                let alias_terminal = frame.alias_terminal;
                let command = command.clone();
                let owner = Arc::clone(&shared);
                let sender = sender.clone();
                let wake = Arc::clone(waker);
                if let Err(error) = shared.connection_threads.run(Box::new(move || {
                    let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        owner.execute_inserted_leaf(
                            client,
                            kind,
                            &mut context,
                            &command,
                            target,
                            prepared,
                            mux_source,
                            inserted_frame_mode(&execution, alias_terminal),
                        )
                    }))
                    .unwrap_or_else(|_| {
                        (
                            Err(DaemonError::Thread("command leaf panicked".to_owned())),
                            None,
                            false,
                            None,
                            false,
                        )
                    });
                    let _ = sender.send((execution, context, step));
                    let _ = wake.wake();
                })) {
                    spawn_error = Some(error);
                }
                *waiting = true;
                None
            },
        );
        if let Some(error) = spawn_error {
            return Err(error.into());
        }
        if let Some((finished, result)) = finished {
            shared.finish_command_queue_execution(&finished.execution, None);
            if let Err(error) = result {
                log::debug!("queued hook failed: {error}");
            }
        }
        Ok(())
    }
}
