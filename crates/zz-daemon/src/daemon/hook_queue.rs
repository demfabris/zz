use super::*;

struct Queue {
    shared: Arc<Shared>,
    frames: Vec<InsertedQueueFrame<Box<CommandQueueExecution>>>,
}

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
            queue
                .shared
                .command_item
                .as_ref()
                .expect("loop hook item")
                .lock()
                .pending_wait
                .as_ref()
                .is_none_or(|wait| wait.continuation.ready())
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

    pub(super) fn shutdown_events(&mut self, shared: &Arc<Shared>, events: Vec<PendingHookEvent>) {
        if events.is_empty() {
            return;
        }
        self.enqueue(
            shared,
            ExecutionContext::default(),
            &InsertedCommandSource::Events(Box::new(EventQueueSource {
                events: RefCell::new(events),
                publish_control: false,
                shutdown_already_blocked: shared.active_shutdown_blockers() != 0,
            })),
        );
    }

    pub(super) fn pending(&self) -> bool {
        !self.queues.is_empty()
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
        {
            let mut item = shared.command_item.as_ref().expect("loop hook item").lock();
            item.loop_leaf = true;
            item.loop_wait = true;
        }
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
                self.queues.push_back(Queue {
                    shared,
                    frames: vec![frame],
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
            if !queue.frames.is_empty() {
                self.queues.push_front(queue);
                break;
            }
        }
        Ok(())
    }
}

impl Queue {
    fn turn(&mut self, waker: &Arc<mio::Waker>) -> Result<(), DaemonError> {
        let shared = Arc::clone(&self.shared);
        if shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .as_ref()
            .is_some_and(|wait| !wait.continuation.ready())
        {
            return Ok(());
        }
        let finished = shared.advance_inserted_frames(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut self.frames,
            64,
            false,
            false,
            |frame, command, target| {
                Some(shared.execute_inserted_frame_command(
                    ClientId(u64::MAX),
                    ClientKind::Command,
                    frame,
                    command,
                    target,
                ))
            },
        );
        if let Some((finished, result)) = finished {
            shared.finish_command_queue_execution(&finished.execution, None);
            if let Err(error) = result {
                log::debug!("queued hook failed: {error}");
            }
        } else if shared
            .command_item
            .as_ref()
            .unwrap()
            .lock()
            .pending_wait
            .is_none()
        {
            waker.wake()?;
        }
        Ok(())
    }
}
