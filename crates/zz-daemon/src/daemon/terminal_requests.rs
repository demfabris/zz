use super::*;
use std::sync::atomic::AtomicUsize;
use zz_terminal::{TerminalRequest, TerminalRequestError};

type Completion<T> = Box<dyn FnOnce(&Arc<Shared>, Result<T, TerminalRequestError>) + Send>;

pub(super) trait Pending: Send {
    fn poll(&mut self, shared: &Arc<Shared>, now: Instant) -> bool;
    fn next(&self) -> Option<Instant>;
}

struct Request<T> {
    token: TerminalRequest<T>,
    finish: Option<Completion<T>>,
}

impl<T: Send> Pending for Request<T> {
    fn poll(&mut self, shared: &Arc<Shared>, now: Instant) -> bool {
        let Some(result) = self.token.poll(now) else {
            return false;
        };
        self.finish.take().unwrap()(shared, result);
        true
    }

    fn next(&self) -> Option<Instant> {
        Some(self.token.next_poll())
    }
}

struct Scheduled {
    deadline: Instant,
    finish: Option<Box<dyn FnOnce(&Arc<Shared>) + Send>>,
}

impl Pending for Scheduled {
    fn poll(&mut self, shared: &Arc<Shared>, now: Instant) -> bool {
        if now < self.deadline {
            return false;
        }
        self.finish.take().unwrap()(shared);
        true
    }

    fn next(&self) -> Option<Instant> {
        Some(self.deadline)
    }
}

pub(super) enum ReplyOutcome<T> {
    Ready(T),
    Closed,
    TimedOut,
}

type ReplyFinish<T> = Box<dyn FnOnce(&Arc<Shared>, ReplyOutcome<T>) + Send>;

struct ReplyRequest<T> {
    result: Arc<Mutex<Option<ReplyOutcome<T>>>>,
    deadline: Option<Instant>,
    continuation: cmdq::WaitContinuation,
    finish: Option<ReplyFinish<T>>,
}

impl<T: Send> Pending for ReplyRequest<T> {
    fn poll(&mut self, shared: &Arc<Shared>, now: Instant) -> bool {
        let result = if self.continuation.ready() || shared.stopping.load(Ordering::Acquire) {
            Some(ReplyOutcome::Closed)
        } else if let Some(result) = self.result.lock().take() {
            Some(result)
        } else if self.deadline.is_some_and(|deadline| now >= deadline) {
            Some(ReplyOutcome::TimedOut)
        } else {
            None
        };
        let Some(result) = result else {
            return false;
        };
        self.finish.take().unwrap()(shared, result);
        true
    }

    fn next(&self) -> Option<Instant> {
        self.deadline
    }
}

struct Wake {
    pending: AtomicBool,
    accept: AcceptWake,
    changed: Condvar,
    serial: Mutex<()>,
}

impl Wake {
    fn notify(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.accept.wake();
        }
        self.changed.notify_all();
    }
}

pub(super) struct Inbox {
    sender: crossbeam_channel::Sender<Box<dyn Pending>>,
    receiver: crossbeam_channel::Receiver<Box<dyn Pending>>,
    active: Mutex<Vec<Box<dyn Pending>>>,
    wake: Arc<Wake>,
}

impl Default for Inbox {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self {
            sender,
            receiver,
            active: Mutex::new(Vec::new()),
            wake: Arc::new(Wake {
                pending: AtomicBool::new(false),
                accept: AcceptWake::new(),
                changed: Condvar::new(),
                serial: Mutex::new(()),
            }),
        }
    }
}

impl Inbox {
    #[cfg(unix)]
    pub(super) fn install(&self, waker: Arc<mio::Waker>) {
        self.wake.accept.install(waker);
    }

    pub(super) fn notifier(&self) -> Arc<dyn Fn() + Send + Sync> {
        let wake = Arc::clone(&self.wake);
        Arc::new(move || wake.notify())
    }

    pub(super) fn submit<T: Send + 'static>(
        &self,
        token: TerminalRequest<T>,
        finish: impl FnOnce(&Arc<Shared>, Result<T, TerminalRequestError>) + Send + 'static,
    ) {
        let _ = self.sender.send(Box::new(Request {
            token,
            finish: Some(Box::new(finish)),
        }));
        self.wake.notify();
    }

    pub(super) fn push(&self, request: impl Pending + 'static) {
        let _ = self.sender.send(Box::new(request));
        self.wake.notify();
    }

    pub(super) fn reply<T: Send + 'static>(
        &self,
        deadline: Option<Instant>,
        continuation: cmdq::WaitContinuation,
        finish: impl FnOnce(&Arc<Shared>, ReplyOutcome<T>) + Send + 'static,
    ) -> cmdq::Reply<T> {
        let result = Arc::new(Mutex::new(None));
        let _ = self.sender.send(Box::new(ReplyRequest {
            result: Arc::clone(&result),
            deadline,
            continuation,
            finish: Some(Box::new(finish)),
        }));
        self.wake.notify();
        let wake = Arc::clone(&self.wake);
        cmdq::Reply::new(move |value| {
            *result.lock() = Some(match value {
                Some(value) => ReplyOutcome::Ready(value),
                None => ReplyOutcome::Closed,
            });
            wake.notify();
        })
    }

    pub(super) fn schedule(
        &self,
        deadline: Instant,
        finish: impl FnOnce(&Arc<Shared>) + Send + 'static,
    ) {
        let _ = self.sender.send(Box::new(Scheduled {
            deadline,
            finish: Some(Box::new(finish)),
        }));
        self.wake.notify();
    }

    pub(super) fn finish_off_loop(
        &self,
        shared: &Arc<Shared>,
        continuation: &cmdq::WaitContinuation,
    ) {
        #[cfg(unix)]
        if shared.loop_active.load(Ordering::Acquire) {
            return;
        }
        self.wait(shared, continuation);
    }

    pub(super) fn pending(&self) -> bool {
        self.wake.pending.load(Ordering::Acquire)
    }

    pub(super) fn turn(&self, shared: &Arc<Shared>) -> Option<Instant> {
        self.wake.pending.store(false, Ordering::Release);
        let mut active = std::mem::take(&mut *self.active.lock());
        active.extend(self.receiver.try_iter());
        let now = Instant::now();
        active.retain_mut(|request| !request.poll(shared, now));
        let next = active.iter().filter_map(|request| request.next()).min();
        self.active.lock().extend(active);
        next
    }

    fn wait(&self, shared: &Arc<Shared>, continuation: &cmdq::WaitContinuation) {
        while !continuation.ready() {
            let deadline = self.turn(shared);
            let mut serial = self.wake.serial.lock();
            if !continuation.ready() && !self.wake.pending.load(Ordering::Acquire) {
                self.wake.changed.wait_for(
                    &mut serial,
                    deadline
                        .unwrap_or_else(Instant::now)
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(10)),
                );
            }
        }
    }
}

#[derive(Default)]
pub(super) struct TapAck {
    state: Mutex<TapAckState>,
}

#[derive(Default)]
struct TapAckState {
    result: Option<Result<(), TerminalRequestError>>,
    waiters: Vec<Arc<CommandState>>,
}

impl TapAck {
    pub(super) fn ready(&self) -> bool {
        self.state.lock().result.is_some()
    }

    pub(super) fn complete(&self, result: Result<(), TerminalRequestError>) {
        let waiters = {
            let mut state = self.state.lock();
            if state.result.is_some() {
                return;
            }
            state.result = Some(result);
            std::mem::take(&mut state.waiters)
        };
        for waiter in waiters {
            if let Err(error) = result {
                *waiter.output.lock() = Some(Err(ServerError::Internal(format!(
                    "could not arm pane output pipe: {error}"
                ))
                .into()));
            }
            waiter.complete();
        }
    }

    fn subscribe(&self, waiter: Arc<CommandState>) {
        waiter.add();
        let mut state = self.state.lock();
        if let Some(result) = state.result {
            drop(state);
            if let Err(error) = result {
                *waiter.output.lock() = Some(Err(ServerError::Internal(format!(
                    "could not arm pane output pipe: {error}"
                ))
                .into()));
            }
            waiter.complete();
        } else {
            state.waiters.push(waiter);
        }
    }
}

impl Drop for TapAck {
    fn drop(&mut self) {
        self.complete(Err(TerminalRequestError::ActorStopped));
    }
}

pub(super) struct CommandState {
    remaining: AtomicUsize,
    pub(super) continuation: cmdq::WaitContinuation,
    output: Mutex<Option<Result<Execution, DaemonError>>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

impl CommandState {
    fn add(&self) {
        self.remaining.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn complete(&self) {
        if self.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.continuation.complete();
            (self.notify)();
        }
    }

    pub(super) fn take_failure(&self) -> Option<DaemonError> {
        let mut output = self.output.lock();
        if matches!(*output, Some(Err(_))) {
            output.take().unwrap().err()
        } else {
            None
        }
    }

    pub(super) fn resolve(&self, result: Result<Execution, DaemonError>) {
        *self.output.lock() = Some(result);
        self.complete();
    }

    pub(super) fn apply(&self, result: &mut Result<Execution, DaemonError>) {
        if let Some(output) = self.output.lock().take() {
            match output {
                Ok(output) => {
                    if let Ok(execution) = result {
                        let tail = std::mem::replace(&mut execution.output, output.output);
                        execution.effects.extend(output.effects);
                        append_inserted_output(&mut execution.output, &tail);
                    }
                }
                Err(error) => *result = Err(error),
            }
        }
    }
}

pub(super) struct CommandWait {
    pub(super) state: Arc<CommandState>,
    registered: bool,
}

impl CommandWait {
    pub(super) fn new(shared: &Arc<Shared>) -> Self {
        let mut item = shared.command_item.as_ref().map(|item| item.lock());
        let registered = item.as_ref().is_some_and(|item| {
            item.loop_wait || {
                #[cfg(unix)]
                {
                    item.loop_leaf
                }
                #[cfg(not(unix))]
                {
                    false
                }
            }
        });
        if registered
            && let Some(state) = item
                .as_ref()
                .and_then(|item| item.pending_wait.as_ref())
                .and_then(|wait| wait.terminal.as_ref())
        {
            state.add();
            return Self {
                state: Arc::clone(state),
                registered,
            };
        }
        let continuation = cmdq::WaitContinuation::new(
            registered.then(|| item.as_ref().unwrap().wait()).flatten(),
            None,
        );
        let state = Arc::new(CommandState {
            remaining: AtomicUsize::new(1),
            continuation: continuation.clone(),
            output: Mutex::new(None),
            notify: shared.terminal_requests.notifier(),
        });
        if registered {
            let item = item.as_mut().unwrap();
            assert!(item.pending_wait.is_none());
            item.pending_wait = Some(Box::new(RegisteredWait {
                name: String::new(),
                continuation,
                #[cfg(unix)]
                shell: None,
                popup: None,
                overlay: None,
                leaf: None,
                guard: None,
                terminal: Some(Arc::clone(&state)),
            }));
        }
        Self { state, registered }
    }

    pub(super) fn finish(
        self,
        shared: &Arc<Shared>,
        execution: Execution,
    ) -> Result<Execution, DaemonError> {
        let state = Arc::clone(&self.state);
        let registered = self.registered;
        drop(self);
        let mut result = Ok(execution);
        if !registered {
            shared.terminal_requests.wait(shared, &state.continuation);
            state.apply(&mut result);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn pane_output(
        &self,
        shared: &Arc<Shared>,
        pane: PaneId,
        terminal: Arc<TerminalSession>,
        format: String,
        active_session: Option<SessionId>,
        format_client: FormatClient,
        variables: BTreeMap<String, String>,
        command: String,
    ) {
        self.state.add();
        let state = Arc::clone(&self.state);
        let request =
            terminal.identity_request(Duration::from_secs(2), shared.terminal_requests.notifier());
        shared
            .terminal_requests
            .submit(request, move |shared, _ready| {
                let result = pane_output(
                    shared,
                    pane,
                    &terminal,
                    &format,
                    active_session,
                    format_client,
                    &variables,
                    &command,
                );
                *state.output.lock() = Some(result.map(|output| Execution {
                    output,
                    effects: Vec::new(),
                }));
                state.complete();
            });
    }

    pub(super) fn start(&self) -> Arc<CommandState> {
        self.state.add();
        Arc::clone(&self.state)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn read<T: Send + 'static>(
        &self,
        shared: &Arc<Shared>,
        client: ClientId,
        pane: PaneId,
        terminal: Arc<TerminalSession>,
        request: TerminalRequest<T>,
        finish: impl FnOnce(
            &Arc<Shared>,
            Result<T, TerminalRequestError>,
        ) -> Result<Execution, DaemonError>
        + Send
        + 'static,
    ) {
        let guard_client = client != ClientId(u64::MAX) && {
            let registered = shared.inner.lock().client(client).is_some();
            #[cfg(unix)]
            let registered = registered || shared.loop_active.load(Ordering::Acquire);
            registered
        };
        self.state.add();
        let state = Arc::clone(&self.state);
        shared
            .terminal_requests
            .submit(request, move |shared, result| {
                let valid = {
                    let inner = shared.inner.lock();
                    if guard_client
                        && (inner.client(client).is_none()
                            || shared.command_queue_cancelled(client))
                    {
                        None
                    } else {
                        Some(
                            inner
                                .terminals
                                .get(&pane)
                                .is_some_and(|current| Arc::ptr_eq(current, &terminal)),
                        )
                    }
                };
                let output = match valid {
                    None => Ok(Execution::default()),
                    Some(false) => Err(ServerError::PaneExited(pane).into()),
                    Some(true) => finish(shared, result),
                };
                *state.output.lock() = Some(output);
                state.complete();
            });
    }

    pub(super) fn tap(&self, tap: &TapAck) {
        tap.subscribe(Arc::clone(&self.state));
    }

    pub(super) fn effects(&self, shared: &Arc<Shared>, commands: Vec<DeferredTerminalCommand>) {
        self.state.add();
        run_effects(shared, commands.into(), Vec::new(), Arc::clone(&self.state));
    }
}

impl Drop for CommandWait {
    fn drop(&mut self) {
        self.state.complete();
    }
}

#[allow(clippy::too_many_arguments)]
fn pane_output(
    shared: &Arc<Shared>,
    pane: PaneId,
    terminal: &Arc<TerminalSession>,
    format: &str,
    active_session: Option<SessionId>,
    format_client: FormatClient,
    variables: &BTreeMap<String, String>,
    command: &str,
) -> Result<RawText, DaemonError> {
    let mut inner = shared.inner.lock();
    if !inner
        .terminals
        .get(&pane)
        .is_some_and(|current| Arc::ptr_eq(current, terminal))
    {
        return Err(ServerError::PaneExited(pane).into());
    }
    let mut runtime = inner
        .engine
        .pane_runtime_facts(pane)
        .cloned()
        .unwrap_or_default();
    runtime.pid = terminal.process_id();
    runtime.tty = terminal
        .tty()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    inner.engine.set_pane_runtime_facts(pane, runtime);
    let target = ExecutionContext::for_pane(&inner.engine.state, pane)
        .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
    let facts = borrowed_format_hook_facts(&inner);
    let mut hooks = DaemonFormatHooks::command_with_optional_variables(
        &facts,
        (!variables.is_empty()).then_some(variables),
    )
    .with_command_item(command);
    let mut output =
        inner
            .engine
            .expand_pane_format(format, &target, active_session, format_client, &mut hooks);
    output.push('\n');
    Ok(output.into())
}

struct Target {
    pane: Option<PaneId>,
    terminal: Arc<TerminalSession>,
}

impl Target {
    fn new(shared: &Shared, terminal: Arc<TerminalSession>) -> Self {
        let pane = shared
            .inner
            .lock()
            .terminals
            .iter()
            .find_map(|(pane, current)| Arc::ptr_eq(current, &terminal).then_some(*pane));
        Self { pane, terminal }
    }

    fn valid(&self, shared: &Shared) -> bool {
        self.pane.is_none_or(|pane| {
            shared
                .inner
                .lock()
                .terminals
                .get(&pane)
                .is_some_and(|current| Arc::ptr_eq(current, &self.terminal))
        })
    }
}

fn run_effects(
    shared: &Arc<Shared>,
    mut commands: VecDeque<DeferredTerminalCommand>,
    mut settle: Vec<Target>,
    state: Arc<CommandState>,
) {
    while let Some(command) = commands.pop_front() {
        if let DeferredTerminalCommand::ArmCopySource { terminal, source } = command {
            let target = Target::new(shared, terminal);
            let source = Target::new(shared, source);
            let request = source
                .terminal
                .copy_source_request(shared.terminal_requests.notifier());
            shared
                .terminal_requests
                .submit(request, move |shared, result| {
                    if target.valid(shared) && source.valid(shared) {
                        match result
                            .map_err(|error| error.to_string())
                            .and_then(|captured| captured.map_err(|error| error.to_string()))
                        {
                            Ok(captured) => target
                                .terminal
                                .set_pending_copy_source(Some(Box::new(captured))),
                            Err(error) => {
                                log::warn!("could not clone the copy-mode source screen: {error}");
                            }
                        }
                        run_effects(shared, commands, settle, state);
                    } else {
                        state.complete();
                    }
                });
            return;
        }
        if let Some(terminal) = command.copy_mode_terminal()
            && !settle
                .iter()
                .any(|target| Arc::ptr_eq(&target.terminal, terminal))
        {
            settle.push(Target::new(shared, Arc::clone(terminal)));
        }
        #[cfg(test)]
        let wrap_search = command.wrap_search();
        command.run();
        #[cfg(test)]
        if let Some(enabled) = wrap_search {
            shared.delivered_wrap_search_commands.lock().push(enabled);
        }
    }
    for target in settle {
        state.add();
        let finished = Arc::clone(&state);
        let request = target
            .terminal
            .settle_request(shared.terminal_requests.notifier());
        shared
            .terminal_requests
            .submit(request, move |_, _| finished.complete());
    }
    state.complete();
}

#[cfg(test)]
#[path = "terminal_requests_e04_tests.rs"]
mod tests;

#[cfg(unix)]
impl Shared {
    pub(super) fn start_control_output_tap(
        self: &Arc<Self>,
        pane: PaneId,
        terminal: &Arc<TerminalSession>,
    ) {
        let (output, receiver) = TerminalSession::raw_output_tap_channel();
        let wake = Arc::clone(&self.pipe_jobs.wake);
        output.set_notification(move || wake.wake());
        let token = {
            let mut inner = self.inner.lock();
            if self.stopping.load(Ordering::Acquire)
                || !inner
                    .terminals
                    .get(&pane)
                    .is_some_and(|current| Arc::ptr_eq(current, terminal))
                || inner.control_output_taps.contains_key(&pane)
            {
                return;
            }
            inner.next_pipe_token = inner.next_pipe_token.wrapping_add(1).max(1);
            let token = inner.next_pipe_token;
            inner.control_output_taps.insert(
                pane,
                ControlOutputTap {
                    receiver: Some(receiver),
                    pending: None,
                    output: output.clone(),
                    pane,
                    token,
                    terminal: Arc::clone(terminal),
                    requests: Arc::clone(&self.terminal_requests),
                    ack: Arc::default(),
                },
            );
            token
        };
        self.arm_control_output_tap(pane, token, terminal, output, true);
    }

    pub(super) fn arm_control_output_tap(
        self: &Arc<Self>,
        pane: PaneId,
        token: u64,
        terminal: &Arc<TerminalSession>,
        output: RawOutputTapSender,
        retry: bool,
    ) {
        let Some(ack) = self
            .inner
            .lock()
            .control_output_taps
            .get(&pane)
            .map(|tap| Arc::clone(&tap.ack))
        else {
            return;
        };
        let request = terminal.arm_raw_output_tap_request(
            token,
            output.clone(),
            self.terminal_requests.notifier(),
        );
        let terminal = Arc::clone(terminal);
        self.terminal_requests
            .submit(request, move |shared, result| {
                let valid = {
                    let inner = shared.inner.lock();
                    inner
                        .terminals
                        .get(&pane)
                        .is_some_and(|current| Arc::ptr_eq(current, &terminal))
                        && inner.control_output_taps.get(&pane).is_some_and(|tap| {
                            tap.token == token && Arc::ptr_eq(&tap.terminal, &terminal)
                        })
                };
                if !valid {
                    ack.complete(Err(TerminalRequestError::ActorStopped));
                    let request = terminal
                        .disarm_raw_output_tap_request(token, shared.terminal_requests.notifier());
                    shared.terminal_requests.submit(request, |_, _| {});
                    return;
                }
                if matches!(result, Ok(true)) {
                    ack.complete(Ok(()));
                    return;
                }
                if retry && matches!(result, Err(TerminalRequestError::TimedOut)) {
                    shared.arm_control_output_tap(pane, token, &terminal, output, false);
                    return;
                }
                ack.complete(Err(result
                    .err()
                    .unwrap_or(TerminalRequestError::ActorStopped)));
                log::warn!("control output tap arm failed for {pane}: {result:?}");
                let tap = {
                    let mut inner = shared.inner.lock();
                    inner
                        .control_output_taps
                        .get(&pane)
                        .is_some_and(|tap| {
                            tap.token == token && Arc::ptr_eq(&tap.terminal, &terminal)
                        })
                        .then(|| inner.control_output_taps.remove(&pane))
                        .flatten()
                };
                if let Some(tap) = tap {
                    stop_control_output_tap(tap);
                }
            });
    }
}
