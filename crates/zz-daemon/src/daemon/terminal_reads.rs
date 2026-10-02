use super::*;
use terminal_requests::CommandState;

struct Target {
    client: ClientId,
    guard_client: bool,
    pane: PaneId,
    terminal: Arc<TerminalSession>,
    state: Arc<CommandState>,
    observation: Arc<Observation>,
}

impl Target {
    fn new(
        shared: &Shared,
        client: ClientId,
        pane: PaneId,
        terminal: Arc<TerminalSession>,
        state: Arc<CommandState>,
    ) -> Arc<Self> {
        let mut inner = shared.inner.lock();
        let registered = inner.client(client).is_some();
        let observation = inner
            .pane_read_observations
            .get(&pane)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let observation = Arc::new(Observation {
                    generation: AtomicU64::new(0),
                    notify: shared.terminal_requests.notifier(),
                });
                inner
                    .pane_read_observations
                    .insert(pane, Arc::downgrade(&observation));
                observation
            });
        #[cfg(unix)]
        let registered = registered || shared.loop_active.load(Ordering::Acquire);
        Arc::new(Self {
            client,
            guard_client: client != ClientId(u64::MAX) && registered,
            pane,
            terminal,
            state,
            observation,
        })
    }

    fn generation(&self) -> u64 {
        self.observation.generation.load(Ordering::Acquire)
    }

    fn check(&self, shared: &Shared) -> bool {
        if self.state.continuation.ready() {
            return false;
        }
        if shared.command_queue_cancelled(self.client)
            || shared.stopping.load(Ordering::Acquire)
            || self.guard_client && shared.inner.lock().client(self.client).is_none()
        {
            self.state.resolve(Ok(Execution::default()));
            return false;
        }
        if !shared
            .inner
            .lock()
            .terminals
            .get(&self.pane)
            .is_some_and(|current| Arc::ptr_eq(current, &self.terminal))
        {
            self.state
                .resolve(Err(ServerError::PaneExited(self.pane).into()));
            return false;
        }
        true
    }
}

pub(super) struct Observation {
    generation: AtomicU64,
    notify: Arc<dyn Fn() + Send + Sync>,
}

pub(super) fn pane_changed(inner: &mut ServerState, pane: PaneId) {
    if let Some(observation) = inner.pane_read_observations.get(&pane) {
        if let Some(observation) = observation.upgrade() {
            observation.generation.fetch_add(1, Ordering::Release);
            (observation.notify)();
        } else {
            inner.pane_read_observations.remove(&pane);
        }
    }
}

type ObserveFinish = Box<dyn FnOnce(&Arc<Shared>, Arc<Target>) + Send>;

struct OutputWait {
    target: Arc<Target>,
    generation: u64,
    deadline: Option<Instant>,
    finish: Option<ObserveFinish>,
}

impl terminal_requests::Pending for OutputWait {
    fn poll(&mut self, shared: &Arc<Shared>, now: Instant) -> bool {
        if self.target.state.continuation.ready() {
            return true;
        }
        let invalid = {
            let inner = shared.inner.lock();
            !inner
                .terminals
                .get(&self.target.pane)
                .is_some_and(|terminal| Arc::ptr_eq(terminal, &self.target.terminal))
                || self.target.guard_client && inner.client(self.target.client).is_none()
        };
        if self.target.generation() == self.generation
            && self.deadline.is_none_or(|deadline| now < deadline)
            && !invalid
            && !shared.command_queue_cancelled(self.target.client)
            && !shared.stopping.load(Ordering::Acquire)
        {
            return false;
        }
        self.finish.take().unwrap()(shared, Arc::clone(&self.target));
        true
    }

    fn next(&self) -> Option<Instant> {
        self.deadline
    }
}

fn observe_output(
    shared: &Arc<Shared>,
    target: Arc<Target>,
    generation: u64,
    deadline: Option<Instant>,
    finish: impl FnOnce(&Arc<Shared>, Arc<Target>) + Send + 'static,
) {
    shared.terminal_requests.push(OutputWait {
        target,
        generation,
        deadline,
        finish: Some(Box::new(finish)),
    });
}

fn timeout_deadline(started: Instant, timeout: Duration) -> Option<Instant> {
    (!timeout.is_zero()).then(|| started + timeout)
}

fn capture_screen(
    shared: &Arc<Shared>,
    target: Arc<Target>,
    options: CaptureOptions,
    finish: impl FnOnce(&Arc<Shared>, Arc<Target>, Result<String, DaemonError>) + Send + 'static,
) {
    if !target.check(shared) {
        return;
    }
    let request = target
        .terminal
        .capture_request(options, shared.terminal_requests.notifier());
    shared
        .terminal_requests
        .submit(request, move |shared, result| {
            if !target.check(shared) {
                return;
            }
            let result = result
                .map_err(TerminalCaptureError::from)
                .and_then(|result| result)
                .map_err(|error| match error {
                    TerminalCaptureError::ActorStopped => ServerError::PaneExited(target.pane),
                    TerminalCaptureError::TimedOut => {
                        ServerError::Internal("terminal capture timed out".to_owned())
                    }
                    error => ServerError::Internal(error.to_string()),
                })
                .map_err(Into::into);
            finish(shared, target, result);
        });
}

type PasteFinish = Box<dyn FnOnce(&Arc<Shared>, Arc<Target>, Result<(), DaemonError>) + Send>;

struct Paste {
    sinks: Vec<Arc<TerminalSession>>,
    bytes: Arc<[u8]>,
    tail: String,
    collapsed_before: usize,
    generation: u64,
    started: Instant,
    timeout: Duration,
    enter: bool,
    finish: PasteFinish,
}

fn paste(
    shared: &Arc<Shared>,
    target: Arc<Target>,
    text: &str,
    timeout: Duration,
    enter: bool,
    finish: PasteFinish,
) -> Result<(), DaemonError> {
    let sinks = {
        let inner = shared.inner.lock();
        resolve_input_sinks(&inner, target.pane)?
            .into_iter()
            .filter_map(|sink| match sink {
                PaneSink::Terminal(terminal) => Some(terminal),
                PaneSink::Browser(_) => None,
            })
            .collect::<Vec<_>>()
    };
    if sinks.is_empty() {
        return Err(
            ServerError::InvalidCommand(format!("{}: pane input is off", target.pane)).into(),
        );
    }
    let bytes = prepare_paste_buffer(text.as_bytes(), b"\r", true)
        .map_err(|error| ServerError::InvalidCommand(format!("send-text: {error}")))?;
    let mut paste = Paste {
        sinks,
        bytes: bytes.into(),
        tail: echo_tail(text),
        collapsed_before: 0,
        generation: target.generation(),
        started: Instant::now(),
        timeout,
        enter,
        finish,
    };
    capture_screen(
        shared,
        target,
        CaptureOptions {
            join_wrapped: true,
            ..CaptureOptions::default()
        },
        move |shared, target, result| {
            let screen = match result {
                Ok(screen) => screen,
                Err(error) => {
                    (paste.finish)(shared, target, Err(error));
                    return;
                }
            };
            paste.collapsed_before = screen.matches(PASTE_COLLAPSE_MARKER).count();
            paste.generation = target.generation();
            for sink in &paste.sinks {
                sink.paste_prepared_bytes(None, Arc::clone(&paste.bytes), true);
            }
            paste.started = Instant::now();
            poll_paste(shared, target, paste);
        },
    );
    Ok(())
}

fn poll_paste(shared: &Arc<Shared>, target: Arc<Target>, paste: Paste) {
    let generation = target.generation();
    capture_screen(
        shared,
        target,
        CaptureOptions {
            join_wrapped: true,
            ..CaptureOptions::default()
        },
        move |shared, target, result| {
            let screen = match result {
                Ok(screen) => screen,
                Err(error) => {
                    (paste.finish)(shared, target, Err(error));
                    return;
                }
            };
            if (paste.tail.is_empty() || generation != paste.generation)
                && (collapse_whitespace(&screen).contains(&paste.tail)
                    || screen.matches(PASTE_COLLAPSE_MARKER).count() > paste.collapsed_before)
            {
                let result = if paste.enter
                    && !send_tokens(
                        &paste.sinks,
                        &[zz_protocol::KeyToken::Named("Enter".to_owned())],
                    ) {
                    Err(ServerError::InvalidCommand(format!(
                        "{}: input queue is full; text delivered but Enter not sent",
                        target.pane
                    ))
                    .into())
                } else {
                    Ok(())
                };
                (paste.finish)(shared, target, result);
            } else if !paste.timeout.is_zero() && paste.started.elapsed() >= paste.timeout {
                (paste.finish)(
                    shared,
                    Arc::clone(&target),
                    Err(ServerError::InvalidCommand(format!(
                        "{}: text not echoed within {} seconds; nothing submitted",
                        target.pane,
                        paste.timeout.as_secs_f64()
                    ))
                    .into()),
                );
            } else {
                observe_output(
                    shared,
                    target,
                    generation,
                    timeout_deadline(paste.started, paste.timeout),
                    move |shared, target| {
                        if target.generation() != generation {
                            poll_paste(shared, target, paste);
                        } else if target.check(shared) {
                            (paste.finish)(
                                shared,
                                Arc::clone(&target),
                                Err(ServerError::InvalidCommand(format!(
                                    "{}: text not echoed within {} seconds; nothing submitted",
                                    target.pane,
                                    paste.timeout.as_secs_f64()
                                ))
                                .into()),
                            );
                        }
                    },
                );
            }
        },
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn send_text(
    shared: &Arc<Shared>,
    client: ClientId,
    pane: PaneId,
    terminal: Arc<TerminalSession>,
    text: &str,
    timeout: Duration,
    enter: bool,
) -> Result<Execution, DaemonError> {
    let wait = terminal_requests::CommandWait::new(shared);
    let target = Target::new(shared, client, pane, terminal, wait.start());
    if let Err(error) = paste(
        shared,
        Arc::clone(&target),
        text,
        timeout,
        enter,
        Box::new(|_, target, result| {
            target.state.resolve(result.map(|()| Execution::default()));
        }),
    ) {
        target.state.resolve(Err(error));
    }
    wait.finish(shared, Execution::default())
}

pub(super) fn submit_agent_text(
    shared: &Arc<Shared>,
    client: ClientId,
    pane: PaneId,
    terminal: Arc<TerminalSession>,
    text: &str,
    finish: impl FnOnce(&Arc<Shared>, Result<(), DaemonError>) + Send + 'static,
) {
    let wait = terminal_requests::CommandWait::new(shared);
    let target = Target::new(shared, client, pane, terminal, wait.start());
    let finish = Arc::new(Mutex::new(Some(finish)));
    let completed = Arc::clone(&finish);
    if let Err(error) = paste(
        shared,
        Arc::clone(&target),
        text,
        SEND_TEXT_TIMEOUT,
        true,
        Box::new(move |shared, target, result| {
            completed.lock().take().unwrap()(shared, result);
            target.state.complete();
        }),
    ) {
        finish.lock().take().unwrap()(shared, Err(error));
        target.state.complete();
    }
}

struct Wait {
    parsed: ParsedWaitPane,
    started: Instant,
    scan_start: Option<usize>,
    first: bool,
}

pub(super) fn wait_pane(
    shared: &Arc<Shared>,
    client: ClientId,
    pane: PaneId,
    terminal: Arc<TerminalSession>,
    parsed: ParsedWaitPane,
) -> Result<Execution, DaemonError> {
    let wait = terminal_requests::CommandWait::new(shared);
    let target = Target::new(shared, client, pane, terminal, wait.start());
    let scan_start = (!matches!(
        parsed.condition,
        PaneWaitCondition::Until(_) | PaneWaitCondition::Regex(_)
    ))
    .then_some(0);
    poll_wait(
        shared,
        target,
        Wait {
            parsed,
            started: Instant::now(),
            scan_start,
            first: true,
        },
    );
    wait.finish(shared, Execution::default())
}

fn poll_wait(shared: &Arc<Shared>, target: Arc<Target>, mut wait: Wait) {
    let generation = target.generation();
    if !target.check(shared) {
        return;
    }
    match wait.parsed.condition {
        PaneWaitCondition::Exit => unreachable!(),
        PaneWaitCondition::Idle(dwell) => {
            let last_output = shared
                .inner
                .lock()
                .last_output
                .get(&target.pane)
                .copied()
                .unwrap_or(wait.started)
                .max(wait.started);
            if !wait.first && last_output.elapsed() >= dwell {
                target.state.resolve(Ok(Execution::default()));
            } else {
                finish_wait_poll(shared, target, wait, generation);
            }
        }
        _ => capture_screen(
            shared,
            target,
            CaptureOptions {
                start: CaptureBoundary::HistoryStart,
                join_wrapped: true,
                preserve_trailing: true,
                ..CaptureOptions::default()
            },
            move |shared, target, result| {
                let screen = match result {
                    Ok(screen) => screen,
                    Err(error) => {
                        target.state.resolve(Err(error));
                        return;
                    }
                };
                let scan_start = *wait.scan_start.get_or_insert_with(|| {
                    screen
                        .lines()
                        .count()
                        .saturating_sub(usize::from(target.terminal.latest_viewport().rows))
                });
                if let Some(line) = screen
                    .lines()
                    .rev()
                    .take(
                        screen
                            .lines()
                            .count()
                            .saturating_sub(scan_start)
                            .min(10_000),
                    )
                    .skip_while(|line| line.trim().is_empty())
                    .take(wait.parsed.tail.unwrap_or(usize::MAX))
                    .find(|line| match &wait.parsed.condition {
                        PaneWaitCondition::Until(text) => line.contains(text),
                        PaneWaitCondition::Regex(regex) => regex.is_match(line),
                        _ => false,
                    })
                {
                    target.state.resolve(Ok(Execution {
                        output: format!("{line}\n").into(),
                        effects: Vec::new(),
                    }));
                } else {
                    finish_wait_poll(shared, target, wait, generation);
                }
            },
        ),
    }
}

fn finish_wait_poll(shared: &Arc<Shared>, target: Arc<Target>, mut wait: Wait, generation: u64) {
    if !wait.parsed.timeout.is_zero() && wait.started.elapsed() >= wait.parsed.timeout {
        let condition = match &wait.parsed.condition {
            PaneWaitCondition::Exit => "--exit".to_owned(),
            PaneWaitCondition::Idle(dwell) => format!("--idle {}", dwell.as_millis()),
            PaneWaitCondition::Until(text) => format!("--until {text:?}"),
            PaneWaitCondition::Regex(regex) => format!("--regex {:?}", regex.as_str()),
        };
        target.state.resolve(Err(DaemonError::CommandExit {
            output: format!(
                "wait-pane: timed out waiting for {condition} on {}\n",
                target.pane
            )
            .into(),
            exit_code: 124,
        }));
    } else {
        if wait.first {
            shared.report_command_queue_park();
        }
        wait.first = false;
        let idle = if let PaneWaitCondition::Idle(dwell) = wait.parsed.condition {
            Some(
                shared
                    .inner
                    .lock()
                    .last_output
                    .get(&target.pane)
                    .copied()
                    .unwrap_or(wait.started)
                    .max(wait.started)
                    + dwell,
            )
        } else {
            None
        };
        let deadline = timeout_deadline(wait.started, wait.parsed.timeout)
            .into_iter()
            .chain(idle)
            .min();
        observe_output(
            shared,
            target,
            generation,
            deadline,
            move |shared, target| {
                if target.generation() != generation
                    || matches!(
                        wait.parsed.condition,
                        PaneWaitCondition::Idle(_) | PaneWaitCondition::Exit
                    )
                {
                    poll_wait(shared, target, wait);
                } else if target.check(shared) {
                    finish_wait_poll(shared, target, wait, generation);
                }
            },
        );
    }
}

struct Run {
    parsed: ParsedRunPane,
    marker: String,
    started: Instant,
    output: String,
    collecting: bool,
}

pub(super) fn run_pane(
    shared: &Arc<Shared>,
    client: ClientId,
    pane: PaneId,
    terminal: Arc<TerminalSession>,
    parsed: ParsedRunPane,
) -> Result<Execution, DaemonError> {
    let mut nonce = [0_u8; 8];
    getrandom::fill(&mut nonce).map_err(std::io::Error::other)?;
    let marker = format!("ZZRUN-{:016x}", u64::from_ne_bytes(nonce));
    let line = format!(
        "printf '\\n%s\\n' '{marker}-BEGIN'; {}; printf '\\n{marker}-RC=%d=END\\n' $?",
        parsed.command
    );
    let run = Run {
        parsed,
        marker,
        started: Instant::now(),
        output: String::new(),
        collecting: false,
    };
    let wait = terminal_requests::CommandWait::new(shared);
    let target = Target::new(shared, client, pane, terminal, wait.start());
    shared.report_command_queue_park();
    if let Err(error) = paste(
        shared,
        Arc::clone(&target),
        &line,
        run.parsed.timeout,
        true,
        Box::new(move |shared, target, result| {
            if let Err(error) = result {
                if !run.parsed.timeout.is_zero() && run.started.elapsed() >= run.parsed.timeout {
                    timeout_run(shared, &target, run);
                } else {
                    target.state.resolve(Err(error));
                }
            } else {
                poll_run(shared, target, run);
            }
        }),
    ) {
        target.state.resolve(Err(error));
    }
    wait.finish(shared, Execution::default())
}

fn timeout_run(shared: &Shared, target: &Target, run: Run) {
    shared.record_command_stderr(
        target.client,
        &format!(
            "run-pane: timed out after {}s on {}; the command keeps running",
            run.parsed.timeout.as_secs_f64(),
            target.pane
        ),
    );
    target.state.resolve(Err(DaemonError::CommandExit {
        output: run.output.into(),
        exit_code: 125,
    }));
}

fn poll_run(shared: &Arc<Shared>, target: Arc<Target>, mut run: Run) {
    let generation = target.generation();
    capture_screen(
        shared,
        target,
        CaptureOptions {
            start: CaptureBoundary::HistoryStart,
            join_wrapped: true,
            preserve_trailing: true,
            ..CaptureOptions::default()
        },
        move |shared, target, result| {
            let screen = match result {
                Ok(screen) => screen,
                Err(error) => {
                    target.state.resolve(Err(error));
                    return;
                }
            };
            let (captured, exit_code) = run_pane_result(&screen, &run.marker, run.collecting);
            if let Some(captured) = captured {
                run.collecting = true;
                captured.clone_into(&mut run.output);
            }
            if let Some(exit_code) = exit_code {
                target.state.resolve(if exit_code == 0 {
                    Ok(Execution {
                        output: run.output.into(),
                        effects: Vec::new(),
                    })
                } else {
                    Err(DaemonError::CommandExit {
                        output: run.output.into(),
                        exit_code,
                    })
                });
            } else if !run.parsed.timeout.is_zero() && run.started.elapsed() >= run.parsed.timeout {
                timeout_run(shared, &target, run);
            } else {
                observe_output(
                    shared,
                    target,
                    generation,
                    timeout_deadline(run.started, run.parsed.timeout),
                    move |shared, target| {
                        if target.generation() != generation {
                            poll_run(shared, target, run);
                        } else if target.check(shared) {
                            timeout_run(shared, &target, run);
                        }
                    },
                );
            }
        },
    );
}

#[cfg(all(test, unix))]
#[path = "terminal_reads_e10_tests.rs"]
mod e10_tests;

#[cfg(test)]
#[path = "terminal_reads_e05_tests.rs"]
mod tests;

#[cfg(unix)]
pub(super) struct MouseBinding {
    pub(super) client: ClientId,
    pub(super) kind: ClientKind,
    pub(super) pane: PaneId,
    pub(super) terminal: Option<Arc<TerminalSession>>,
    pub(super) context: ExecutionContext,
    pub(super) commands: Vec<CommandInvocation>,
    pub(super) repeat_binding: bool,
    pub(super) done: cmdq::WaitContinuation,
}

#[cfg(unix)]
impl Shared {
    pub(super) fn resume_mouse_binding(self: &Arc<Self>, mut binding: MouseBinding) {
        let valid = {
            let inner = self.inner.lock();
            inner.client(binding.client).is_some()
                && client_is_attached_to_pane(&inner, binding.client, binding.pane)
                && binding.terminal.as_ref().is_none_or(|terminal| {
                    inner
                        .terminals
                        .get(&binding.pane)
                        .is_some_and(|current| Arc::ptr_eq(current, terminal))
                })
        };
        if !valid {
            binding.done.complete();
            return;
        }
        binding.context.set_repeat_binding(binding.repeat_binding);
        let client = binding.client;
        let kind = binding.kind;
        let title = binding.commands.first().map_or_else(
            || "command output".to_owned(),
            |command| {
                if binding.commands.len() == 1 {
                    command.name.clone()
                } else {
                    "command output".to_owned()
                }
            },
        );
        self.enqueue_inserted_task(
            client,
            kind,
            &binding.context,
            &InsertedCommandSource::Commands(binding.commands),
            "<mouse-binding>",
            None,
            false,
            None,
            Box::new(move |shared, context, result| {
                match result {
                    Ok(result) => shared.route_background_inserted_output(
                        client,
                        kind,
                        context,
                        title,
                        &result.output,
                    ),
                    Err(error) => {
                        shared.publish_background_command_error(client, context, &error, false);
                    }
                }
                shared.sync_key_table(client, false);
                binding.done.complete();
                shared.accept_wake.wake();
            }),
        );
    }
}
