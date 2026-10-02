use std::sync::LazyLock;

use super::*;
use zz_protocol::{
    ClientEnvironmentBlob, ExecExit, ExecFlags, ExecOutcome, ExecRequest, ExecResume,
    ExecResumeKind,
};

#[cfg(windows)]
thread_local! {
    static EXEC_CLIENT: Cell<Option<ClientId>> = const { Cell::new(None) };
}

type ExecJob = Box<dyn FnOnce() + Send>;

const IDLE_CONNECTION_THREADS: usize = 2;
#[cfg(windows)]
const EXEC_FLUSH_FRAMES: usize = 64;
#[cfg(windows)]
const EXEC_FLUSH_BYTES: usize = 64 * 1024;
const CONNECTION_THREAD_IDLE: Duration = Duration::from_secs(1);

static SPAWN_PER_CONNECTION: LazyLock<bool> = LazyLock::new(|| {
    std::env::var_os("ZZ_PERF_CONNECTION_THREADS").is_some_and(|value| value == "0")
});

#[derive(Default)]
pub(super) struct ConnectionThreads {
    idle: Mutex<Vec<crossbeam_channel::Sender<ExecJob>>>,
    #[cfg(test)]
    pub(super) fail_next: AtomicBool,
    #[cfg(test)]
    live_workers: Arc<std::sync::atomic::AtomicUsize>,
}

impl ConnectionThreads {
    #[cfg(windows)]
    pub(super) fn log_knob() {
        if *SPAWN_PER_CONNECTION {
            log::info!("ZZ_PERF_CONNECTION_THREADS=0: every connection starts a new thread");
        }
    }

    pub(super) fn run(self: &Arc<Self>, job: ExecJob) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fail_next.swap(false, Ordering::AcqRel) {
            return Err(std::io::Error::other("injected worker start failure"));
        }
        let mut job = job;
        loop {
            let Some(worker) = self.idle.lock().pop() else {
                break;
            };
            match worker.send(job) {
                Ok(()) => return Ok(()),
                Err(returned) => job = returned.into_inner(),
            }
        }
        let threads = Arc::downgrade(self);
        #[cfg(test)]
        let lifetime = WorkerLifetime::new(Arc::clone(&self.live_workers));
        thread::Builder::new()
            .name("zz-client".to_owned())
            .spawn(move || {
                #[cfg(test)]
                let _lifetime = lifetime;
                connection_worker(&threads, job);
            })
            .map(drop)
    }

    #[cfg(test)]
    pub(super) fn idle_count(&self) -> usize {
        self.idle.lock().len()
    }

    #[cfg(test)]
    pub(super) fn worker_count(&self) -> usize {
        self.live_workers.load(Ordering::Acquire)
    }

    fn park(&self, worker: &crossbeam_channel::Sender<ExecJob>) -> bool {
        let mut idle = self.idle.lock();
        if *SPAWN_PER_CONNECTION || idle.len() >= IDLE_CONNECTION_THREADS {
            return false;
        }
        idle.push(worker.clone());
        true
    }

    fn retire(&self, worker: &crossbeam_channel::Sender<ExecJob>) -> bool {
        let mut idle = self.idle.lock();
        let Some(index) = idle.iter().position(|parked| parked.same_channel(worker)) else {
            return false;
        };
        idle.swap_remove(index);
        true
    }
}

#[cfg(test)]
struct WorkerLifetime(Arc<std::sync::atomic::AtomicUsize>);

#[cfg(test)]
impl WorkerLifetime {
    fn new(workers: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        workers.fetch_add(1, Ordering::AcqRel);
        Self(workers)
    }
}

#[cfg(test)]
impl Drop for WorkerLifetime {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn connection_worker(threads: &Weak<ConnectionThreads>, first: ExecJob) {
    let (worker, jobs) = crossbeam_channel::bounded::<ExecJob>(1);
    let mut job = first;
    loop {
        job();
        let Some(owner) = threads.upgrade() else {
            return;
        };
        if !owner.park(&worker) {
            return;
        }
        drop(owner);
        job = match jobs.recv_timeout(CONNECTION_THREAD_IDLE) {
            Ok(next) => next,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                let Some(owner) = threads.upgrade() else {
                    return;
                };
                if owner.retire(&worker) {
                    return;
                }
                match jobs.recv() {
                    Ok(next) => next,
                    Err(_) => return,
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
        };
    }
}

#[cfg(windows)]
pub(super) struct ExecLink {
    starter: Mutex<Option<ExecStarter>>,
    live: Mutex<Option<ExecLive>>,
}

#[cfg(windows)]
struct ExecLive {
    writer: thread::JoinHandle<()>,
    reader: thread::JoinHandle<()>,
    requests: crossbeam_channel::Receiver<ExecRequest>,
}

#[cfg(windows)]
type ExecStarter = Box<dyn FnOnce() -> Option<ExecLive> + Send>;

#[cfg(windows)]
impl ExecLink {
    fn go_live(&self) {
        let mut starter = self.starter.lock();
        let Some(start) = starter.take() else {
            return;
        };
        if let Some(live) = start() {
            *self.live.lock() = Some(live);
        }
    }

    fn is_live(&self) -> bool {
        self.live.lock().is_some()
    }

    fn direct_writes(&self) -> Option<parking_lot::MutexGuard<'_, Option<ExecStarter>>> {
        let starter = self.starter.lock();
        (!self.is_live()).then_some(starter)
    }
}

struct ExecRegistration {
    shared: Arc<Shared>,
    client: ClientId,
    released: Arc<AtomicBool>,
}

impl ExecRegistration {
    fn release(&self) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }
        #[cfg(unix)]
        if self.shared.loop_active.load(Ordering::Acquire) {
            self.shared.lifecycle.release(self.client, true);
            self.shared.accept_wake.wake();
            return;
        }
        if !detach_is_inert(&self.shared.inner.lock(), self.client) {
            self.shared.detach(self.client);
        }
        self.shared.unregister(self.client);
    }
}

#[cfg(windows)]
struct ExecClientScope;

#[cfg(windows)]
impl ExecClientScope {
    fn new(client: ClientId) -> Self {
        EXEC_CLIENT.with(|current| current.set(Some(client)));
        Self
    }
}

#[cfg(windows)]
impl Drop for ExecClientScope {
    fn drop(&mut self) {
        EXEC_CLIENT.with(|current| current.set(None));
    }
}

#[must_use]
pub fn exec_resume_kind(
    typed: &[CommandInvocation],
    prepared: &[PreparedCommand],
) -> Option<ExecResumeKind> {
    let first_typed = typed.first()?.name.as_str();
    let first = prepared.first()?;
    let new_session = typed.iter().zip(prepared).any(|(typed, prepared)| {
        !matches!(typed.name.as_str(), "attach" | "attach-session")
            && prepared_command_any(prepared, |command, canonical_name| {
                canonical_name == "new-session"
                    && MuxEngine::new_session_attaches(&command.args).unwrap_or(false)
            })
    });
    let attach_prefix = !matches!(first_typed, "attach" | "attach-session")
        && prepared_command_any(first, |_, canonical_name| {
            canonical_name == "attach-session"
        });
    if new_session || attach_prefix {
        return Some(ExecResumeKind::NewSession);
    }
    (matches!(first_typed, "attach" | "attach-session")
        && !first.alias_matched
        && first.result == PreparedCommandResult::Ready
        && first.canonical_name.as_deref() == Some("attach-session"))
    .then_some(ExecResumeKind::NativeAttach)
}

fn prepared_command_invocations(command: &PreparedCommand) -> Option<Cow<'_, [CommandInvocation]>> {
    if command.result != PreparedCommandResult::Ready {
        return None;
    }
    if command.canonical_name.is_some() {
        return Some(Cow::Borrowed(std::slice::from_ref(&command.invocation)));
    }
    MuxEngine::command_alias_group_commands(&command.invocation)
        .ok()
        .flatten()
        .map(Cow::Owned)
}

fn prepared_command_any(
    command: &PreparedCommand,
    mut matches: impl FnMut(&CommandInvocation, &str) -> bool,
) -> bool {
    prepared_command_invocations(command).is_some_and(|commands| {
        commands.iter().any(|invocation| {
            let canonical_name = command
                .canonical_name
                .as_deref()
                .unwrap_or_else(|| canonical_command(&invocation.name));
            matches(invocation, canonical_name)
        })
    })
}

impl Shared {
    #[cfg(windows)]
    pub(super) fn go_live_exec(&self, client: ClientId) {
        let link = self.exec_links.lock().get(&client).cloned();
        if let Some(link) = link {
            link.go_live();
        }
    }

    #[cfg(windows)]
    pub(super) fn go_live_current_exec(&self) {
        if let Some(client) = EXEC_CLIENT.with(Cell::get) {
            self.go_live_exec(client);
        }
    }

    #[cfg(windows)]
    fn defer_exec_until_startup(&self, job: ExecJob) -> Option<ExecJob> {
        let ready = self.startup_ready.lock();
        if *ready || self.stopping.load(Ordering::Acquire) {
            return Some(job);
        }
        self.pending_execs.lock().push(job);
        drop(ready);
        None
    }

    pub(super) fn resume_pending_execs(&self) {
        self.accept_wake.wake();
        #[cfg(windows)]
        let pending = std::mem::take(&mut *self.pending_execs.lock());
        #[cfg(windows)]
        for job in pending {
            if let Err(error) = self.connection_threads.run(job) {
                log::warn!("could not resume a parked command connection: {error}");
            }
        }
    }

    pub(super) fn drop_pending_execs(&self) {
        drop(std::mem::take(&mut *self.pending_execs.lock()));
    }

    pub(super) fn register_exec(
        &self,
        request: &ExecRequest,
        environment: ClientEnvironmentBlob,
    ) -> Option<(ClientId, ExecutionContext)> {
        let mut inner = self.inner.lock();
        if self.stopping.load(Ordering::Acquire) || self.shutdown_pending.load(Ordering::Acquire) {
            return None;
        }
        let client = ClientId(inner.next_client_id);
        inner.next_client_id = inner.next_client_id.saturating_add(1);
        inner
            .cold_bootstrap
            .register(client, request.startup_reentry == Some(self.server_id));
        let now = unix_timestamp();
        inner.activity_sequence = inner.activity_sequence.saturating_add(1);
        let activity = inner.activity_sequence;
        let mut entry = Box::<Client>::default();
        entry.instance_id = Some(request.client_instance_id);
        entry.kind = Some(ClientKind::Command);
        entry.activity = Some(activity);
        entry.activity_time = Some(now);
        entry.created_time = Some(now);
        entry.focused = Some(true);
        entry.origin = request.origin;
        entry.nested = request.flags.contains(ExecFlags::NESTED);
        entry.utf8 = request.flags.contains(ExecFlags::UTF8);
        entry.features = (request.features != 0).then_some(request.features);
        entry.tty = request.tty.as_ref().filter(|tty| !tty.is_empty()).cloned();
        entry.size = request
            .size
            .filter(|(columns, rows)| *columns > 0 && *rows > 0);
        entry.pid = Some(request.process_id);
        entry.working_directory = client_working_directory_fact(request.working_directory.as_ref());
        entry.environment = Some(Arc::new(environment));
        inner.clients.insert(client, entry);
        let context = request
            .origin
            .and_then(|pane| ExecutionContext::for_pane(&inner.engine.state, pane))
            .or_else(|| {
                inner
                    .engine
                    .state
                    .most_recent_context()
                    .map(|(session, window, pane)| {
                        ExecutionContext::new(Some(session), Some(window), Some(pane))
                    })
            })
            .unwrap_or_default();
        Some((client, context))
    }
}

#[cfg(windows)]
pub(super) fn serve_exec<S: TransportStream>(
    mut stream: S,
    shared: &Arc<Shared>,
    request: ExecRequest,
) -> Result<(), DaemonError> {
    if request.protocol_version != PROTOCOL_VERSION {
        best_effort_protocol_mismatch_reply(&mut stream, request.protocol_version);
        return Err(ServerError::ProtocolMismatch {
            client: request.protocol_version,
            server: PROTOCOL_VERSION,
        }
        .into());
    }
    let mut request = request;
    let mut frame = Vec::new();
    while request.commands.is_empty() {
        if shared.stopping.load(Ordering::Acquire)
            || shared.shutdown_pending.load(Ordering::Acquire)
        {
            best_effort_server_stopping_reply(&mut stream);
            return Ok(());
        }
        let outcome = if request
            .expect_server_id
            .is_some_and(|expected| expected != shared.server_id)
        {
            ExecOutcome::ServerMismatch
        } else {
            ExecOutcome::Ran
        };
        encode_protocol_message_into(
            &ProtocolMessage::ExecExit(ExecExit {
                server_id: shared.server_id,
                outcome,
            }),
            &mut frame,
        )?;
        stream.write_all(&frame)?;
        stream.flush()?;
        if request.flags.contains(ExecFlags::LAST) {
            return Ok(());
        }
        request = loop {
            match read_protocol_message_into(&mut stream, &mut frame) {
                Ok(ProtocolMessage::Exec(next)) => break next,
                Ok(_) => {}
                Err(_) => return Ok(()),
            }
        };
    }
    let reentry = request.startup_reentry == Some(shared.server_id);
    let job_shared = Arc::clone(shared);
    let job: ExecJob = Box::new(move || serve_exec_ready(stream, &job_shared, request));
    if reentry {
        job();
        return Ok(());
    }
    if let Some(job) = shared.defer_exec_until_startup(job) {
        job();
    }
    Ok(())
}

#[cfg(windows)]
fn serve_exec_ready<S: TransportStream>(
    mut stream: S,
    shared: &Arc<Shared>,
    mut request: ExecRequest,
) {
    let started = diagnostic_timer();
    let environment = std::mem::take(&mut request.environment);
    let Some((client, context)) = shared.register_exec(&request, environment) else {
        best_effort_server_stopping_reply(&mut stream);
        return;
    };
    log::debug!(
        target: "zz_daemon::diagnostics::connection",
        "registered exec client={client} request={request:#?}",
    );
    let cancel = Arc::new(AtomicBool::new(false));
    shared
        .command_queue_cancels
        .lock()
        .insert(client, Arc::clone(&cancel));
    let registration = Arc::new(ExecRegistration {
        shared: Arc::clone(shared),
        client,
        released: Arc::new(AtomicBool::new(false)),
    });
    let mut connection = ExecConnection {
        shared: Arc::clone(shared),
        stream: Arc::new(Mutex::new(Some(stream))),
        frame: Vec::new(),
        client,
        context,
        cancel,
        registration,
        live: None,
        resumed: false,
    };
    loop {
        let last = request.flags.contains(ExecFlags::LAST);
        connection.run(request);
        if last && !connection.resumed {
            break;
        }
        match connection.next_request() {
            Some(ProtocolMessage::Exec(next)) => request = next,
            Some(first @ ProtocolMessage::Hello(_)) => {
                let stream = connection.stream.lock().take();
                connection.finish();
                if let Some(stream) = stream {
                    let _ = handle_connection_message(stream, shared, first);
                }
                return;
            }
            _ => break,
        }
    }
    connection.finish();
    log::debug!(
        target: "zz_daemon::diagnostics::connection",
        "unregistered exec client={client} connection_elapsed_us={}",
        diagnostic_elapsed_us(started),
    );
}

#[cfg(windows)]
struct ExecConnection<S: TransportStream> {
    shared: Arc<Shared>,
    stream: Arc<Mutex<Option<S>>>,
    frame: Vec<u8>,
    client: ClientId,
    context: ExecutionContext,
    cancel: Arc<AtomicBool>,
    registration: Arc<ExecRegistration>,
    live: Option<(Arc<OutboundMailbox>, Arc<ExecLink>)>,
    resumed: bool,
}

#[cfg(windows)]
impl<S: TransportStream> ExecConnection<S> {
    fn run(&mut self, request: ExecRequest) {
        let (mailbox, link) = if let Some((mailbox, link)) = &self.live {
            (Arc::clone(mailbox), Arc::clone(link))
        } else {
            let mailbox = OutboundMailbox::buffered();
            let link = Arc::new(ExecLink {
                starter: Mutex::new(Some(self.starter(&mailbox))),
                live: Mutex::new(None),
            });
            self.shared
                .client_writers
                .lock()
                .insert(self.client, Arc::clone(&mailbox));
            self.shared
                .exec_links
                .lock()
                .insert(self.client, Arc::clone(&link));
            (mailbox, link)
        };
        let admission = ResponseAdmissionGuard::new(&self.shared);
        let outcome = if admission.is_some() {
            self.execute(&mailbox, &link, request)
        } else {
            let _ = mailbox.enqueue_reliable(&server_stopping_response(1));
            ExecOutcome::Ran
        };
        self.resumed = matches!(outcome, ExecOutcome::Resume(_));
        let exit = ProtocolMessage::ExecExit(ExecExit {
            server_id: self.shared.server_id,
            outcome,
        });
        let Some(direct) = link.direct_writes() else {
            let _ = mailbox.enqueue_reliable(&exit);
            drop(admission);
            if self.live.is_none() {
                self.live = Some((mailbox, link));
            }
            return;
        };
        drop(admission);
        let mut output = Vec::new();
        mailbox.drain_reliable_into(&mut output);
        let mut frame = Vec::new();
        if encode_protocol_message_into(&exit, &mut frame).is_ok() {
            output.extend_from_slice(&frame);
        }
        let _ = self.write_direct(&output);
        drop(direct);
        mailbox.mark_writer_finished();
        self.shared.exec_links.lock().remove(&self.client);
        let mut writers = self.shared.client_writers.lock();
        if writers
            .get(&self.client)
            .is_some_and(|current| Arc::ptr_eq(current, &mailbox))
        {
            writers.remove(&self.client);
        }
    }

    fn execute(
        &mut self,
        mailbox: &Arc<OutboundMailbox>,
        link: &ExecLink,
        request: ExecRequest,
    ) -> ExecOutcome {
        let shared = Arc::clone(&self.shared);
        if request
            .expect_server_id
            .is_some_and(|expected| expected != shared.server_id)
        {
            return ExecOutcome::ServerMismatch;
        }
        if request.commands.is_empty() {
            return ExecOutcome::Ran;
        }
        let stdin_available = request.flags.contains(ExecFlags::STDIN_AVAILABLE);
        if request.flags.contains(ExecFlags::PREPARED) {
            self.run_commands(mailbox, link, request.commands, stdin_available);
            return ExecOutcome::Ran;
        }
        let resume = request.flags.contains(ExecFlags::RESUME);
        let typed = resume.then(|| {
            request
                .commands
                .iter()
                .map(|command| CommandInvocation::new(command.name.clone(), Vec::<String>::new()))
                .collect::<Vec<_>>()
        });
        let prepared = {
            let mut inner = shared.inner.lock();
            let prepared =
                Shared::prepare_command_list_with_engine(&inner.engine, request.commands, true);
            if request.spawned_server_id == Some(shared.server_id) {
                let failed = prepared
                    .iter()
                    .any(|command| matches!(command.result, PreparedCommandResult::Error(_)));
                inner.cold_bootstrap.prepare(self.client, failed);
            }
            prepared
        };
        if let Some(error) = prepared.iter().find_map(|command| match &command.result {
            PreparedCommandResult::Ready => None,
            PreparedCommandResult::Error(error) => Some(error.clone()),
        }) {
            return ExecOutcome::Rejected(error);
        }
        if let Some(typed) = typed
            && let Some(kind) = exec_resume_kind(&typed, &prepared)
        {
            return ExecOutcome::Resume(ExecResume {
                kind,
                commands: prepared,
            });
        }
        self.run_commands(
            mailbox,
            link,
            prepared.into_iter().map(|command| command.invocation),
            stdin_available,
        );
        ExecOutcome::Ran
    }

    fn run_commands(
        &mut self,
        mailbox: &Arc<OutboundMailbox>,
        link: &ExecLink,
        commands: impl IntoIterator<Item = CommandInvocation>,
        stdin_available: bool,
    ) {
        let shared = Arc::clone(&self.shared);
        let _scope = ExecClientScope::new(self.client);
        for (index, mut invocation) in commands.into_iter().enumerate() {
            let request_id = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
            if self.cancel.load(Ordering::Acquire) {
                break;
            }
            if shared.shutdown_pending.load(Ordering::Acquire) {
                let _ = mailbox.enqueue_reliable(&server_stopping_response(request_id));
                break;
            }
            invocation.set_stdin_available(stdin_available);
            let (response, client_exit) = shared.execute_command_request_with_streams(
                self.client,
                ClientKind::Command,
                &mut self.context,
                request_id,
                &invocation,
                true,
            );
            let failed = matches!(response, CommandResponse::Error { .. });
            let admitted = mailbox.enqueue_reliable(&ProtocolMessage::CommandResponse(response));
            if failed || client_exit || !admitted {
                break;
            }
            self.flush_buffered(mailbox, link);
        }
    }

    fn flush_buffered(&mut self, mailbox: &OutboundMailbox, link: &ExecLink) {
        if mailbox
            .queued_reliable()
            .is_none_or(|(bytes, frames)| bytes < EXEC_FLUSH_BYTES && frames < EXEC_FLUSH_FRAMES)
        {
            return;
        }
        let Some(direct) = link.direct_writes() else {
            return;
        };
        let mut output = Vec::new();
        mailbox.drain_reliable_into(&mut output);
        let written = self.write_direct(&output);
        drop(direct);
        if written.is_err() {
            mailbox.close();
        }
    }

    fn write_direct(&self, output: &[u8]) -> std::io::Result<()> {
        let mut stream = self.stream.lock();
        let Some(stream) = stream.as_mut() else {
            return Err(ErrorKind::NotConnected.into());
        };
        stream.write_all(output).and_then(|()| stream.flush())
    }

    fn starter(&self, mailbox: &Arc<OutboundMailbox>) -> ExecStarter {
        let slot = Arc::clone(&self.stream);
        let mailbox = Arc::clone(mailbox);
        let shared = Arc::downgrade(&self.shared);
        let client = self.client;
        let cancel = Arc::clone(&self.cancel);
        let registration = Arc::clone(&self.registration);
        Box::new(move || {
            let mut reader = slot.lock().as_ref()?.try_clone().ok()?;
            let mut stream = slot.lock().take()?;
            let mut buffered = Vec::new();
            mailbox.drain_reliable_into(&mut buffered);
            if !buffered.is_empty()
                && stream
                    .write_all(&buffered)
                    .and_then(|()| stream.flush())
                    .is_err()
            {
                mailbox.close();
            }
            mailbox.stop_buffering();
            let (requests_tx, requests) = crossbeam_channel::unbounded();
            let writer_mailbox = Arc::clone(&mailbox);
            let writer_shared = shared.clone();
            let writer = thread::Builder::new()
                .name(format!("zz-client-writer-{}", client.0))
                .spawn(move || write_outbound(&mut stream, &writer_mailbox, &writer_shared, client))
                .ok()?;
            let reader = thread::Builder::new()
                .name(format!("zz-client-reader-{}", client.0))
                .spawn(move || {
                    let mut frame = Vec::new();
                    loop {
                        match read_protocol_message_into(&mut reader, &mut frame) {
                            Ok(ProtocolMessage::Exec(request)) => {
                                if requests_tx.send(request).is_err() {
                                    break;
                                }
                            }
                            Ok(ProtocolMessage::ClientFileResponse(response)) => {
                                if let Some(shared) = shared.upgrade() {
                                    shared.complete_client_file(client, response);
                                }
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                    cancel.store(true, Ordering::Release);
                    registration.release();
                })
                .ok();
            let Some(reader) = reader else {
                mailbox.close();
                let _ = writer.join();
                return None;
            };
            Some(ExecLive {
                writer,
                reader,
                requests,
            })
        })
    }

    fn next_request(&mut self) -> Option<ProtocolMessage> {
        if let Some((_, link)) = &self.live {
            let requests = link.live.lock().as_ref()?.requests.clone();
            return requests.recv().ok().map(ProtocolMessage::Exec);
        }
        let mut stream = self.stream.lock();
        let stream = stream.as_mut()?;
        loop {
            match read_protocol_message_into(stream, &mut self.frame) {
                Ok(message @ (ProtocolMessage::Exec(_) | ProtocolMessage::Hello(_))) => {
                    return Some(message);
                }
                Ok(ProtocolMessage::ClientFileResponse(response)) => {
                    self.shared.complete_client_file(self.client, response);
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }

    fn finish(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.registration.release();
        self.shared
            .command_queue_cancels
            .lock()
            .remove(&self.client);
        let Some((mailbox, link)) = self.live.take() else {
            return;
        };
        self.shared.exec_links.lock().remove(&self.client);
        mailbox.close_after_flush();
        let live = link.live.lock().take();
        if let Some(live) = live {
            if live.writer.join().is_err() {
                log::error!(
                    target: "zz_daemon::diagnostics::connection",
                    "writer thread panicked for client={}",
                    self.client,
                );
            }
            let _ = live.reader.join();
        }
        let mut writers = self.shared.client_writers.lock();
        if writers
            .get(&self.client)
            .is_some_and(|current| Arc::ptr_eq(current, &mailbox))
        {
            writers.remove(&self.client);
        }
    }
}

#[cfg(unix)]
pub(super) struct LoopExec {
    shared: Arc<Shared>,
    pub(super) client: ClientId,
    context: ExecutionContext,
    cancel: Arc<AtomicBool>,
    registration: ExecRegistration,
    mailbox: Arc<OutboundMailbox>,
}

#[cfg(unix)]
pub(super) struct PreparedExec {
    commands: std::vec::IntoIter<PreparedCommand>,
    next_request: u64,
    stdin_available: bool,
    outcome: ExecOutcome,
    pub(super) last: bool,
    pub(super) waiting_output: bool,
    pub(super) waiting_command: bool,
    pub(super) ready_on_loop: bool,
    task: Option<Box<wait_queue::CommandTask>>,
    admission: Option<ResponseAdmissionGuard>,
}

#[cfg(unix)]
impl PreparedExec {
    pub(super) fn command_ready(&self) -> bool {
        self.task.as_ref().is_none_or(|task| task.ready())
    }
}

#[cfg(unix)]
impl LoopExec {
    pub(super) fn register(
        shared: &Arc<Shared>,
        request: &mut ExecRequest,
        mailbox: &Arc<OutboundMailbox>,
        cancel: &Arc<AtomicBool>,
    ) -> Option<Self> {
        let environment = std::mem::take(&mut request.environment);
        let (client, context) = shared.register_exec(request, environment)?;
        shared
            .command_queue_cancels
            .lock()
            .insert(client, Arc::clone(cancel));
        shared
            .client_writers
            .lock()
            .insert(client, Arc::clone(mailbox));
        Some(Self {
            shared: Arc::clone(shared),
            client,
            context,
            cancel: Arc::clone(cancel),
            registration: ExecRegistration {
                shared: Arc::clone(shared),
                client,
                released: Arc::new(AtomicBool::new(false)),
            },
            mailbox: Arc::clone(mailbox),
        })
    }

    pub(super) fn released(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.registration.released)
    }

    pub(super) fn prepare(&mut self, request: ExecRequest) -> PreparedExec {
        let stdin_available = request.flags.contains(ExecFlags::STDIN_AVAILABLE);
        let mut prepared = PreparedExec {
            commands: Vec::new().into_iter(),
            next_request: 1,
            stdin_available,
            outcome: ExecOutcome::Ran,
            last: request.flags.contains(ExecFlags::LAST),
            waiting_output: false,
            waiting_command: false,
            ready_on_loop: false,
            task: None,
            admission: ResponseAdmissionGuard::new(&self.shared),
        };
        if prepared.admission.is_none() {
            return prepared;
        }
        if request
            .expect_server_id
            .is_some_and(|id| id != self.shared.server_id)
        {
            prepared.outcome = ExecOutcome::ServerMismatch;
            return prepared;
        }
        if request.commands.is_empty() {
            return prepared;
        }
        if request.flags.contains(ExecFlags::PREPARED) {
            prepared.commands = request
                .commands
                .into_iter()
                .map(|invocation| {
                    let canonical_name = (!MuxEngine::is_command_alias_group(&invocation))
                        .then(|| canonical_command(&invocation.name).to_owned());
                    PreparedCommand {
                        invocation,
                        canonical_name,
                        alias_matched: false,
                        result: PreparedCommandResult::Ready,
                    }
                })
                .collect::<Vec<_>>()
                .into_iter();
            return prepared;
        }
        let typed = request.flags.contains(ExecFlags::RESUME).then(|| {
            request
                .commands
                .iter()
                .map(|command| CommandInvocation::new(command.name.clone(), Vec::<String>::new()))
                .collect::<Vec<_>>()
        });
        let commands = {
            let mut inner = self.shared.inner.lock();
            let commands =
                Shared::prepare_command_list_with_engine(&inner.engine, request.commands, true);
            if request.spawned_server_id == Some(self.shared.server_id) {
                let failed = commands
                    .iter()
                    .any(|command| matches!(command.result, PreparedCommandResult::Error(_)));
                inner.cold_bootstrap.prepare(self.client, failed);
            }
            commands
        };
        if let Some(error) = commands.iter().find_map(|command| match &command.result {
            PreparedCommandResult::Ready => None,
            PreparedCommandResult::Error(error) => Some(error.clone()),
        }) {
            prepared.outcome = ExecOutcome::Rejected(error);
            return prepared;
        }
        if let Some(typed) = typed
            && let Some(kind) = exec_resume_kind(&typed, &commands)
        {
            prepared.outcome = ExecOutcome::Resume(ExecResume { kind, commands });
            return prepared;
        }
        prepared.commands = commands.into_iter();
        prepared
    }

    pub(super) fn run(&mut self, prepared: &mut PreparedExec, inline: bool) -> Option<bool> {
        prepared.waiting_output = false;
        prepared.waiting_command = false;
        prepared.ready_on_loop = false;
        let mailbox = Arc::clone(&self.mailbox);
        if prepared.admission.is_some() {
            let shared = Arc::clone(&self.shared);
            while prepared.task.is_some() || !prepared.commands.as_slice().is_empty() {
                let mut task_command = None;
                if inline {
                    if self.output_pending() {
                        prepared.waiting_output = true;
                        return None;
                    }
                    if prepared.task.is_none() {
                        let command = &prepared.commands.as_slice()[0];
                        let task = wait_queue::task_command(&command.invocation);
                        if !task && !self.can_inline(command) {
                            return None;
                        }
                        task_command = Some(task);
                    }
                }
                let (response, client_exit) = if let Some(mut task) = prepared.task.take() {
                    match task.run(inline) {
                        wait_queue::Progress::Done => {
                            let (response, client_exit, context, _admission) = task.finish();
                            self.context = context;
                            (response, client_exit)
                        }
                        wait_queue::Progress::Waiting => {
                            prepared.waiting_command = true;
                            prepared.task = Some(task);
                            return None;
                        }
                        wait_queue::Progress::Worker => {
                            prepared.task = Some(task);
                            return None;
                        }
                        wait_queue::Progress::Ready => {
                            prepared.ready_on_loop = true;
                            prepared.task = Some(task);
                            return None;
                        }
                    }
                } else {
                    let mut invocation = prepared.commands.next().unwrap().invocation;
                    let request_id = prepared.next_request;
                    prepared.next_request = request_id.saturating_add(1);
                    if self.cancel.load(Ordering::Acquire) {
                        break;
                    }
                    if shared.shutdown_pending.load(Ordering::Acquire) {
                        let _ = mailbox.enqueue_reliable(&server_stopping_response(request_id));
                        break;
                    }
                    invocation.set_stdin_available(prepared.stdin_available);
                    if !inline
                        || task_command.unwrap_or_else(|| wait_queue::task_command(&invocation))
                    {
                        match wait_queue::CommandTask::new(
                            &shared,
                            self.client,
                            ClientKind::Command,
                            &self.context,
                            request_id,
                            &invocation,
                            true,
                        ) {
                            Ok(task) => {
                                prepared.task = Some(Box::new(task));
                                continue;
                            }
                            Err(response) => (response, false),
                        }
                    } else {
                        let item = shared.command_item(None);
                        item.command_item
                            .as_ref()
                            .expect("Exec command item")
                            .lock()
                            .exec_writer = Some(Arc::downgrade(&mailbox));
                        item.execute_command_request_with_streams(
                            self.client,
                            ClientKind::Command,
                            &mut self.context,
                            request_id,
                            &invocation,
                            true,
                        )
                    }
                };
                let failed = matches!(response, CommandResponse::Error { .. });
                let admitted = mailbox.enqueue_reliable_with_wakeup(
                    &ProtocolMessage::CommandResponse(response),
                    false,
                );
                if failed || client_exit || !admitted {
                    prepared.commands = Vec::new().into_iter();
                    break;
                }
                if !inline && !prepared.commands.as_slice().is_empty() {
                    return None;
                }
            }
        } else {
            let _ = mailbox.enqueue_reliable(&server_stopping_response(1));
            prepared.outcome = ExecOutcome::Ran;
        }
        let outcome = std::mem::replace(&mut prepared.outcome, ExecOutcome::Ran);
        let resumed = matches!(outcome, ExecOutcome::Resume(_));
        if !self.cancel.load(Ordering::Acquire) {
            let _ = mailbox.enqueue_reliable_with_wakeup(
                &ProtocolMessage::ExecExit(ExecExit {
                    server_id: self.shared.server_id,
                    outcome,
                }),
                false,
            );
        }
        drop(prepared.admission.take());
        Some(resumed)
    }

    pub(super) fn finish(&mut self) {
        if self.registration.released.swap(true, Ordering::AcqRel) {
            return;
        }
        if !detach_is_inert(&self.shared.inner.lock(), self.client) {
            self.shared.detach(self.client);
        }
        self.shared.unregister(self.client);
    }

    pub(super) fn can_finish_inline(&self) -> bool {
        let inner = self.shared.inner.lock();
        detach_is_inert(&inner, self.client) && !inner.engine.destroy_unattached_explicit_anywhere()
    }

    fn can_inline(&self, command: &PreparedCommand) -> bool {
        if command.result == PreparedCommandResult::Ready
            && ctrl::control_query_can_defer_wakeup(
                &self.shared.inner.lock(),
                &self.context,
                command,
            )
        {
            return true;
        }
        let Some(name @ ("select-pane" | "send-keys")) = command.canonical_name.as_deref() else {
            return false;
        };
        let Some(spec) = zz_protocol::catalog_command_spec(name) else {
            return false;
        };
        let Ok(parsed) = zz_protocol::parse_tmux_options(spec, &command.invocation.args) else {
            return false;
        };
        if parsed.options.iter().any(|option| {
            !(matches!(option, zz_protocol::TmuxOption::Value("-t", _))
                || (name == "send-keys"
                    && matches!(option, zz_protocol::TmuxOption::Flag("-l" | "-H" | "-R"))))
        }) {
            return false;
        }
        let inner = self.shared.inner.lock();
        if !inner.deferred_event_hooks.is_empty()
            || !inner.pane_modes.is_empty()
            || inner
                .clients
                .values()
                .any(|client| client.copy_session.is_some() || client.command_output.is_some())
            || inner
                .engine
                .has_hook_commands(self.context.session, "command-error")
            || MuxEngine::after_command_hook(name)
                .is_some_and(|hook| inner.engine.has_hook_commands(self.context.session, hook))
        {
            return false;
        }
        let target = parsed.options.iter().rev().find_map(|option| match option {
            zz_protocol::TmuxOption::Value("-t", value) => Some(*value),
            _ => None,
        });
        if name == "select-pane"
            && matches!(target, Some(":.+" | ".+" | ":+" | ":.-" | ".-" | ":-"))
        {
            return false;
        }
        let Ok(pane) = inner
            .engine
            .resolve_pane(target, self.context.window, self.context.pane)
        else {
            return false;
        };
        if name == "send-keys" {
            inner.attached.is_empty()
        } else {
            inner
                .engine
                .state
                .window_for_pane(pane)
                .and_then(|window| inner.engine.state.windows.get(&window))
                .is_some_and(|window| window.active_pane == pane && window.zoomed_pane.is_none())
        }
    }

    pub(super) fn output_pending(&self) -> bool {
        output_pending(&self.mailbox)
    }
}

#[cfg(unix)]
pub(super) fn output_pending(mailbox: &OutboundMailbox) -> bool {
    let state = mailbox.state.lock();
    !state.closed
        && (state
            .queued_bytes
            .saturating_add(state.writer_inflight_bytes)
            >= 64 * 1024
            || state
                .reliable
                .len()
                .saturating_add(state.writer_inflight_messages)
                >= 64)
}

#[cfg(unix)]
impl Drop for LoopExec {
    fn drop(&mut self) {
        if !self.registration.released.load(Ordering::Acquire) {
            self.registration.release();
        }
        self.shared
            .command_queue_cancels
            .lock()
            .remove(&self.client);
    }
}
