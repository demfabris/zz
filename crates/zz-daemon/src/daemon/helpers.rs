use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use parking_lot::{Condvar, Mutex};

use super::{ClientId, PaneId, path_listing};

const MAX_WORKERS: usize = 2;
const MAX_PENDING: usize = 64;
const IDLE_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) enum Task {
    SourceRead {
        path: std::path::PathBuf,
        complete: super::file_commands::Completion,
    },
    File {
        path: std::path::PathBuf,
        operation: zz_protocol::ClientFileOperation,
        complete: super::file_commands::Completion,
    },
    Path(Box<path_listing::Task>),
    #[cfg(feature = "agent")]
    Catalog {
        client: ClientId,
        pane: PaneId,
        config: Box<crate::agent::runtime::AgentSpawnConfig>,
        result: zz_protocol::agent_stream::AgentCatalogResult,
    },
    Terminfo {
        environment: Vec<zz_protocol::RawText>,
        #[cfg(unix)]
        jobs: Option<JobClient>,
        reply: std::sync::mpsc::SyncSender<()>,
    },
    ImportRead {
        path: std::path::PathBuf,
        reply: std::sync::mpsc::SyncSender<std::result::Result<String, super::DaemonError>>,
    },
    Read {
        path: std::path::PathBuf,
        reply: std::sync::mpsc::SyncSender<io::Result<Vec<u8>>>,
    },
    #[cfg(all(feature = "agent", unix))]
    Peers {
        panes: Vec<(PaneId, String, Option<u32>)>,
        always: bool,
        reply: Option<std::sync::mpsc::SyncSender<PeerResult>>,
        completed: Option<crossbeam_channel::Sender<super::timers::TimerCompletion>>,
    },
    HistoryLoad {
        path: std::path::PathBuf,
        limit: usize,
        complete: Completion<(Vec<String>, Vec<String>)>,
    },
    HistorySave {
        path: std::path::PathBuf,
        command: Vec<String>,
        search: Vec<String>,
        complete: Completion<()>,
    },
    #[cfg(test)]
    Hold(
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::SyncSender<()>,
    ),
}

#[cfg(all(feature = "agent", unix))]
pub(super) type PeerResult = io::Result<Vec<(PaneId, Option<u32>, Option<String>)>>;

pub(super) type Completion<T> = Box<dyn FnOnce(&Arc<super::Shared>, T) + Send>;

pub(super) enum Result {
    Complete(Box<dyn FnOnce(&Arc<super::Shared>) + Send>),
    File {
        complete: super::file_commands::Completion,
        result: std::result::Result<Vec<u8>, super::DaemonError>,
    },
    #[cfg(all(feature = "agent", unix))]
    Peers(PeerResult),
    Path {
        result: path_listing::PathResult,
        applied: std::sync::mpsc::SyncSender<()>,
    },
    #[cfg(feature = "agent")]
    Catalog {
        client: ClientId,
        pane: PaneId,
        result: zz_protocol::agent_stream::AgentCatalogResult,
    },
}

#[derive(Default)]
struct Queue {
    tasks: VecDeque<Task>,
    workers: usize,
    busy: usize,
    stopped: bool,
    #[cfg(test)]
    starts: usize,
    #[cfg(test)]
    submitted: usize,
}

struct State {
    queue: Mutex<Queue>,
    available: Condvar,
    results: crossbeam_channel::Sender<Result>,
    wake: super::AcceptWake,
    pending: Arc<AtomicBool>,
    #[cfg(unix)]
    loop_thread: Mutex<Option<thread::ThreadId>>,
    #[cfg(all(feature = "agent", unix))]
    peers: Mutex<crate::agent::claude_peers::RegistryCache>,
}

pub(super) struct Pool {
    state: Arc<State>,
    pub(super) results: crossbeam_channel::Receiver<Result>,
    #[cfg(unix)]
    pub(super) jobs: JobClient,
    #[cfg(unix)]
    pub(super) job_requests: crossbeam_channel::Receiver<JobRequest>,
}

impl Default for Pool {
    fn default() -> Self {
        let (results, receiver) = crossbeam_channel::bounded(MAX_PENDING + MAX_WORKERS);
        #[cfg(unix)]
        let (requests, job_requests) = crossbeam_channel::bounded(MAX_PENDING);
        let pending = Arc::new(AtomicBool::new(false));
        Self {
            state: Arc::new(State {
                queue: Mutex::new(Queue::default()),
                available: Condvar::new(),
                results,
                wake: super::AcceptWake::new(),
                pending: Arc::clone(&pending),
                #[cfg(unix)]
                loop_thread: Mutex::new(None),
                #[cfg(all(feature = "agent", unix))]
                peers: Mutex::new(crate::agent::claude_peers::RegistryCache::default()),
            }),
            results: receiver,
            #[cfg(unix)]
            jobs: JobClient {
                requests,
                wake: Arc::new(super::AcceptWake::new()),
                pending,
            },
            #[cfg(unix)]
            job_requests,
        }
    }
}

impl Pool {
    #[cfg(unix)]
    pub(super) fn take_pending(&self) -> bool {
        self.state.pending.load(Ordering::Acquire)
            && self.state.pending.swap(false, Ordering::AcqRel)
    }

    #[cfg(unix)]
    pub(super) fn notify_loop(&self) {
        self.state.pending.store(true, Ordering::Release);
        self.state.wake.wake();
    }

    #[cfg(unix)]
    pub(super) fn install(&self, waker: Arc<mio::Waker>) {
        *self.state.loop_thread.lock() = Some(thread::current().id());
        self.state.wake.install(Arc::clone(&waker));
        self.jobs.wake.install(waker);
    }

    #[cfg(all(test, unix))]
    pub(super) fn set_loop_thread(&self, owner: Option<thread::ThreadId>) {
        *self.state.loop_thread.lock() = owner;
    }

    pub(super) fn active(&self) -> bool {
        self.state.queue.lock().workers != 0
    }

    pub(super) fn on_loop_thread(&self) -> bool {
        #[cfg(unix)]
        {
            *self.state.loop_thread.lock() == Some(thread::current().id())
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    pub(super) fn require_off_loop(&self) -> io::Result<()> {
        if self.on_loop_thread() {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "synchronous helper reply on mux loop",
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn import_source(
        &self,
        path: &std::path::Path,
    ) -> std::result::Result<String, super::DaemonError> {
        self.require_off_loop()?;
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.submit_wait(Task::ImportRead {
            path: path.to_owned(),
            reply,
        })?;
        result
            .recv()
            .map_err(|_| io::Error::other("import helper stopped"))?
    }

    pub(super) fn read(&self, path: &std::path::Path) -> io::Result<Vec<u8>> {
        self.require_off_loop()?;
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.submit_wait(Task::Read {
            path: path.to_owned(),
            reply,
        })?;
        result
            .recv()
            .map_err(|_| io::Error::other("file helper stopped"))?
    }

    pub(super) fn submit_wait(&self, task: Task) -> io::Result<()> {
        self.enqueue(task, true)
    }

    pub(super) fn submit(&self, task: Task) -> io::Result<()> {
        self.enqueue(task, false)
    }

    fn enqueue(&self, task: Task, wait: bool) -> io::Result<()> {
        #[cfg(unix)]
        let wait = wait && *self.state.loop_thread.lock() != Some(thread::current().id());
        let mut queue = self.state.queue.lock();
        while wait && !queue.stopped && queue.tasks.len() >= MAX_PENDING {
            self.state.available.wait(&mut queue);
        }
        if queue.stopped || queue.tasks.len() >= MAX_PENDING {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "helper queue is full or stopped",
            ));
        }
        queue.tasks.push_back(task);
        #[cfg(test)]
        {
            queue.submitted += 1;
        }
        if queue.workers < MAX_WORKERS && queue.tasks.len() > queue.workers - queue.busy {
            let state = Arc::clone(&self.state);
            match thread::Builder::new()
                .name("zz-helper".into())
                .spawn(move || worker(&state))
            {
                Ok(_) => {
                    queue.workers += 1;
                    #[cfg(test)]
                    {
                        queue.starts += 1;
                    }
                }
                Err(error) if queue.workers == 0 => {
                    queue.tasks.pop_back();
                    return Err(error);
                }
                Err(error) => log::warn!("could not start a second helper: {error}"),
            }
        }
        self.state.available.notify_one();
        Ok(())
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        let mut queue = self.state.queue.lock();
        queue.stopped = true;
        queue.tasks.clear();
        self.state.available.notify_all();
    }
}

fn worker(state: &State) {
    loop {
        let task = {
            let mut queue = state.queue.lock();
            while queue.tasks.is_empty() && !queue.stopped {
                if state
                    .available
                    .wait_for(&mut queue, IDLE_TIMEOUT)
                    .timed_out()
                    && queue.tasks.is_empty()
                {
                    queue.workers -= 1;
                    return;
                }
            }
            if queue.stopped {
                queue.workers -= 1;
                return;
            }
            queue.busy += 1;
            let task = queue.tasks.pop_front().unwrap();
            state.available.notify_all();
            task
        };
        #[cfg(all(feature = "agent", unix))]
        let retire = matches!(&task, Task::Peers { .. });
        #[cfg(not(all(feature = "agent", unix)))]
        let retire = false;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(task, state)));
        if let Ok(Some(result)) = result {
            let _ = state.results.send(result);
            state.pending.store(true, Ordering::Release);
            state.wake.wake();
        } else if result.is_err() {
            log::error!("helper task panicked");
        }
        let mut queue = state.queue.lock();
        queue.busy -= 1;
        if retire && queue.tasks.is_empty() {
            queue.workers -= 1;
            state.available.notify_all();
            return;
        }
    }
}

fn run(task: Task, _state: &State) -> Option<Result> {
    match task {
        Task::SourceRead { path, complete } => Some(Result::File {
            complete,
            result: std::fs::read(path).map_err(Into::into),
        }),
        Task::File {
            path,
            operation,
            complete,
        } => {
            let result = match operation {
                zz_protocol::ClientFileOperation::Read => super::read_paste_buffer_file(&path),
                zz_protocol::ClientFileOperation::Write { append, data } => {
                    super::write_paste_buffer_file(&path, &data, append).map(|()| Vec::new())
                }
                _ => Err(super::client_file_failure(
                    "no invoking stdin client",
                    &path,
                )),
            }
            .map_err(Into::into);
            Some(Result::File { complete, result })
        }
        Task::Path(task) => {
            task.run(&|result| {
                let cancel = Arc::clone(&result.cancel);
                let (applied, ready) = std::sync::mpsc::sync_channel(1);
                if _state
                    .results
                    .send(Result::Path { result, applied })
                    .is_err()
                {
                    return false;
                }
                _state.pending.store(true, Ordering::Release);
                _state.wake.wake();
                ready.recv().is_ok() && !cancel.load(Ordering::Acquire)
            });
            None
        }
        #[cfg(feature = "agent")]
        Task::Catalog {
            client,
            pane,
            config,
            mut result,
        } => {
            match futures_lite::future::block_on(crate::agent::catalog::load(
                *config,
                result.catalog_provider,
                result.cwd.clone(),
            )) {
                Ok(options) => result.config_options = Some(options),
                Err(error) => result.error = Some(error),
            }
            Some(Result::Catalog {
                client,
                pane,
                result,
            })
        }
        Task::Terminfo {
            environment,
            #[cfg(unix)]
            jobs,
            reply,
        } => {
            crate::status::warm_terminfo_entries_using(&environment, &|mut command| {
                #[cfg(unix)]
                if let Some(jobs) = &jobs {
                    return jobs
                        .output(
                            command,
                            1024 * 1024,
                            std::time::Instant::now() + Duration::from_secs(2),
                            Arc::new(AtomicBool::new(false)),
                        )
                        .ok();
                }
                command.output().ok()
            });
            let _ = reply.send(());
            None
        }
        Task::ImportRead { path, reply } => {
            let _ = reply.send(super::read_mux_import_source(&path));
            None
        }
        Task::Read { path, reply } => {
            let _ = reply.send(std::fs::read(path));
            None
        }
        #[cfg(all(feature = "agent", unix))]
        Task::Peers {
            panes,
            always,
            reply,
            completed,
        } => {
            let result = scan_peers(_state, panes, always);
            let result = if let Some(reply) = reply {
                let _ = reply.send(result);
                None
            } else {
                Some(Result::Peers(result))
            };
            if let Some(completed) = completed {
                let _ = completed.send(super::timers::TimerCompletion::Peer);
            }
            result
        }
        Task::HistoryLoad {
            path,
            limit,
            complete,
        } => {
            let history = super::load_command_prompt_history(&path, limit);
            Some(Result::Complete(Box::new(move |shared| {
                complete(shared, history);
            })))
        }
        Task::HistorySave {
            path,
            command,
            search,
            complete,
        } => {
            super::save_command_prompt_history(&path, &command, &search);
            Some(Result::Complete(Box::new(move |shared| {
                complete(shared, ());
            })))
        }
        #[cfg(test)]
        Task::Hold(wait, started) => {
            let _ = started.send(());
            let _ = wait.recv();
            None
        }
    }
}

#[cfg(all(feature = "agent", unix))]
fn scan_peers(
    state: &State,
    panes: Vec<(PaneId, String, Option<u32>)>,
    always: bool,
) -> PeerResult {
    use crate::agent::claude_peers;
    let mut records = if always {
        claude_peers::read_records()?
    } else {
        let mut registry = state.peers.lock();
        registry.refresh()?;
        registry.records().cloned().collect()
    };
    records.retain(|record| record.zz.is_none());
    let now = super::SystemTime::now()
        .duration_since(super::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let parents = std::cell::RefCell::new(std::collections::BTreeMap::new());
    Ok(panes
        .into_iter()
        .map(|(pane, target, pid)| {
            let value = claude_peers::record_for_pane_with_parents(&records, &target, pid, |pid| {
                *parents
                    .borrow_mut()
                    .entry(pid)
                    .or_insert_with(|| crate::process_info::parent(pid))
            })
            .filter(|record| {
                let updated = if record.status_updated_at == 0 {
                    record.updated_at
                } else {
                    record.status_updated_at
                };
                !record.status.is_empty()
                    && now
                        .checked_sub(updated)
                        .is_some_and(|age| u128::from(age) <= Duration::from_mins(10).as_millis())
            })
            .map(|record| {
                if record.status == "busy" {
                    "working"
                } else {
                    "idle"
                }
                .to_owned()
            });
            (pane, pid, value)
        })
        .collect())
}

#[cfg(unix)]
#[derive(Clone)]
pub(super) struct JobClient {
    requests: crossbeam_channel::Sender<JobRequest>,
    wake: Arc<super::AcceptWake>,
    pending: Arc<AtomicBool>,
}

#[cfg(unix)]
pub(super) struct JobRequest {
    pub(super) command: std::process::Command,
    pub(super) limit: usize,
    pub(super) deadline: std::time::Instant,
    pub(super) cancel: Arc<std::sync::atomic::AtomicBool>,
    pub(super) reply:
        std::sync::mpsc::SyncSender<std::result::Result<std::process::Output, String>>,
}

#[cfg(unix)]
impl JobClient {
    pub(super) fn notify(&self) {
        self.pending.store(true, Ordering::Release);
        self.wake.wake();
    }

    pub(super) fn output(
        &self,
        command: std::process::Command,
        limit: usize,
        deadline: std::time::Instant,
        cancel: Arc<AtomicBool>,
    ) -> std::result::Result<std::process::Output, String> {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.requests
            .try_send(JobRequest {
                command,
                limit,
                deadline,
                cancel,
                reply,
            })
            .map_err(|error| error.to_string())?;
        self.notify();
        receiver
            .recv()
            .map_err(|_| "discovery job stopped".to_owned())?
    }
}

#[cfg(test)]
#[path = "helpers_e19_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "helpers_e19fix_tests.rs"]
mod e19fix_tests;

#[cfg(test)]
#[path = "helpers_helperwait_tests.rs"]
mod helperwait_tests;
