use super::*;

struct Subscriber {
    client: ClientId,
    state: Arc<terminal_requests::CommandState>,
    split: Option<Arc<AtomicU8>>,
}

#[derive(Default)]
struct State {
    completed: bool,
    killed: bool,
    subscribers: Vec<Subscriber>,
}

pub(super) struct Completion {
    terminal: Weak<TerminalSession>,
    exit_code: AtomicU8,
    state: Mutex<State>,
}

impl Completion {
    pub(super) fn new(terminal: &Arc<TerminalSession>) -> Arc<Self> {
        Arc::new(Self {
            terminal: Arc::downgrade(terminal),
            exit_code: AtomicU8::new(0),
            state: Mutex::new(State::default()),
        })
    }

    pub(super) fn subscribe(
        &self,
        client: ClientId,
        state: Arc<terminal_requests::CommandState>,
        split: Option<Arc<AtomicU8>>,
    ) {
        let subscriber = Subscriber {
            client,
            state,
            split,
        };
        let mut state = self.state.lock();
        if state.completed {
            Self::resolve(
                subscriber,
                self.exit_code.load(Ordering::Acquire),
                state.killed,
            );
        } else {
            state.subscribers.push(subscriber);
        }
    }

    fn resolve(subscriber: Subscriber, exit_code: u8, killed: bool) {
        if let Some(status) = subscriber.split {
            status.store(if killed { 129 } else { exit_code }, Ordering::Release);
            subscriber.state.complete();
        } else {
            subscriber.state.resolve(if exit_code == 0 {
                Ok(Execution::default())
            } else {
                Err(DaemonError::CommandExit {
                    output: RawText::default(),
                    exit_code,
                })
            });
        }
    }

    pub(super) fn complete(&self, exit_code: u8) {
        self.finish(exit_code, false);
    }

    /// `window_pane_wait_finish` on a pane destroyed before its status was
    /// ready: a `-W` waiter answers `128 + SIGHUP`, a `wait-pane --exit` 0.
    pub(super) fn kill(&self) {
        self.finish(0, true);
    }

    fn finish(&self, exit_code: u8, killed: bool) {
        let mut state = self.state.lock();
        if state.completed {
            return;
        }
        state.completed = true;
        state.killed = killed;
        self.exit_code.store(exit_code, Ordering::Release);
        for subscriber in state.subscribers.drain(..) {
            Self::resolve(subscriber, exit_code, killed);
        }
    }

    fn cancel(&self, mut matches: impl FnMut(&Subscriber) -> bool) {
        self.state.lock().subscribers.retain(|subscriber| {
            if matches(subscriber) {
                Self::resolve_cancelled(subscriber);
                false
            } else {
                true
            }
        });
    }

    fn resolve_cancelled(subscriber: &Subscriber) {
        if subscriber.split.is_some() {
            subscriber.state.complete();
        } else {
            subscriber.state.resolve(Ok(Execution::default()));
        }
    }

    #[cfg(test)]
    pub(super) fn waiters(&self) -> usize {
        self.state.lock().subscribers.len()
    }
}

pub(super) fn remove_unused(inner: &mut ServerState, pane: PaneId) {
    if inner
        .pane_exit_waits
        .get(&pane)
        .is_some_and(|entry| entry.command_wait.is_none() && entry.current.state.lock().completed)
    {
        inner.pane_exit_waits.remove(&pane);
    }
}

fn cancel_matching(inner: &mut ServerState, mut matches: impl FnMut(&Subscriber) -> bool) {
    for entry in inner.pane_exit_waits.values() {
        entry.current.cancel(&mut matches);
        if let Some(command) = &entry.command_wait
            && !Arc::ptr_eq(command, &entry.current)
        {
            command.cancel(&mut matches);
        }
    }
}

pub(super) fn cancel_client(inner: &mut ServerState, client: ClientId) {
    cancel_matching(inner, |subscriber| subscriber.client == client);
}

pub(super) fn cancel_token(inner: &mut ServerState, token: cmdq::ContinuationToken) {
    cancel_matching(inner, |subscriber| {
        subscriber.state.continuation.token == token
    });
}

pub(super) fn cancel_all(inner: &mut ServerState) {
    cancel_matching(inner, |_| true);
    inner.pane_exit_waits.clear();
}

pub(super) fn wait(
    shared: &Arc<Shared>,
    client: ClientId,
    pane: PaneId,
    terminal: &Arc<TerminalSession>,
    timeout: Duration,
) -> Result<Execution, DaemonError> {
    let wait = terminal_requests::CommandWait::new(shared);
    let state = wait.start();
    let completion = {
        let mut inner = shared.inner.lock();
        let entry = inner
            .pane_exit_waits
            .entry(pane)
            .or_insert_with(|| PaneExitWait::new(terminal));
        if !entry.current.terminal.ptr_eq(&Arc::downgrade(terminal)) {
            entry.current.complete(0);
            entry.current = Completion::new(terminal);
        }
        Arc::clone(&entry.current)
    };
    completion.subscribe(client, Arc::clone(&state), None);
    if shared.command_queue_cancelled(client) || shared.stopping.load(Ordering::Acquire) {
        completion
            .cancel(|subscriber| subscriber.state.continuation.token == state.continuation.token);
    } else if terminal.completion().is_some() {
        completion.complete(pane_wait_exit_code(
            terminal,
            &terminal.latest_viewport().status,
        ));
        remove_unused(&mut shared.inner.lock(), pane);
    } else if !timeout.is_zero() {
        let completed = Arc::downgrade(&completion);
        let token = state.continuation.token;
        shared.terminal_requests.schedule_until(
            Instant::now() + timeout,
            state.continuation.clone(),
            move |shared| {
                if let Some(completion) = completed.upgrade() {
                    completion.state.lock().subscribers.retain(|subscriber| {
                        if subscriber.state.continuation.token == token {
                            subscriber.state.resolve(Err(DaemonError::CommandExit {
                                output: format!(
                                    "wait-pane: timed out waiting for --exit on {pane}\n"
                                )
                                .into(),
                                exit_code: 124,
                            }));
                            false
                        } else {
                            true
                        }
                    });
                }
                remove_unused(&mut shared.inner.lock(), pane);
            },
        );
    }
    shared.report_command_queue_park();
    wait.finish(shared, Execution::default())
}
