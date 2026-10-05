use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    os::fd::{AsRawFd, OwnedFd},
    process::{Command, ExitStatus},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use mio::{Interest, Registry, Token, unix::SourceFd};

use super::status_jobs::StatusOutput;
pub(super) use super::status_jobs::{StatusClient, StatusRequest};
use super::{
    DaemonError, Shared, ShellJobPermit, ShellJobResult, configure_shell_job_environment,
    existing_job_working_directory, shell_process,
};
use std::path::Path;
use zz_protocol::RawText;

const IO_BURST: usize = 256 * 1024;

pub(super) fn allocate_token(next: &mut usize) -> io::Result<Token> {
    let token = Token(*next);
    *next = next
        .checked_add(1)
        .ok_or_else(|| io::Error::other("mux tokens exhausted"))?;
    Ok(token)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct JobId(u64);

#[derive(Clone, Copy)]
pub(super) enum CompletionPolicy {
    ChildExit,
    ChildExitAndEof,
}

pub(super) struct Descriptor {
    pub(super) fd: OwnedFd,
    pub(super) read: bool,
    pub(super) input: Option<Vec<u8>>,
    pub(super) socket: bool,
}

pub(super) struct PipeIo {
    terminal: Arc<parking_lot::Mutex<Arc<zz_terminal::TerminalSession>>>,
    input: Option<super::shard_sink::PipeReader>,
    waker: std::task::Waker,
}

impl PipeIo {
    pub(super) fn new(
        terminal: Arc<parking_lot::Mutex<Arc<zz_terminal::TerminalSession>>>,
        input: Option<super::shard_sink::PipeReader>,
        waker: std::task::Waker,
    ) -> Self {
        Self {
            terminal,
            input,
            waker,
        }
    }
}

pub(super) struct Launch {
    pub(super) child: JobChild,
    pub(super) descriptors: Vec<Descriptor>,
    pub(super) policy: CompletionPolicy,
    pub(super) deadline: Option<Instant>,
    pub(super) process_group: bool,
    pub(super) output_limit: Option<usize>,
    pub(super) stream: Option<StatusOutput>,
    pub(super) pipe: Option<PipeIo>,
    pub(super) cancel: Option<Arc<AtomicBool>>,
    pub(super) complete: Box<dyn FnOnce(Completion) + Send>,
}

pub(super) struct Completion {
    pub(super) id: JobId,
    pub(super) pid: u32,
    pub(super) status: Option<ExitStatus>,
    pub(super) output: Vec<Vec<u8>>,
    pub(super) error: Option<io::Error>,
    pub(super) cancelled: bool,
}

struct Port {
    descriptor: Descriptor,
    index: usize,
    written: usize,
    eof: bool,
    registered: bool,
    drain_left: Option<usize>,
    pending_output: Option<Arc<[u8]>>,
}

impl Port {
    fn interest(&self) -> Option<Interest> {
        let read = self.descriptor.read
            && !self.eof
            && self.drain_left != Some(0)
            && self.pending_output.is_none();
        let write = self.descriptor.input.is_some();
        match (read, write) {
            (true, true) => Some(Interest::READABLE | Interest::WRITABLE),
            (true, false) => Some(Interest::READABLE),
            (false, true) => Some(Interest::WRITABLE),
            (false, false) => None,
        }
    }

    fn sync(&mut self, registry: &Registry, token: Token) -> io::Result<()> {
        let fd = self.descriptor.fd.as_raw_fd();
        match (self.registered, self.interest()) {
            (true, Some(interest)) => registry.reregister(&mut SourceFd(&fd), token, interest),
            (false, Some(interest)) => {
                registry.register(&mut SourceFd(&fd), token, interest)?;
                self.registered = true;
                Ok(())
            }
            (true, None) => {
                registry.deregister(&mut SourceFd(&fd))?;
                self.registered = false;
                Ok(())
            }
            (false, None) => Ok(()),
        }
    }
}

struct Job {
    child: Option<JobChild>,
    pid: u32,
    ports: BTreeMap<Token, Port>,
    output: Vec<Vec<u8>>,
    policy: CompletionPolicy,
    deadline: Option<Instant>,
    process_group: bool,
    output_limit: Option<usize>,
    stream: Option<StatusOutput>,
    pipe: Option<PipeIo>,
    cancel_flag: Option<Arc<AtomicBool>>,
    status: Option<ExitStatus>,
    error: Option<io::Error>,
    cancelled: bool,
    complete: Option<Box<dyn FnOnce(Completion) + Send>>,
}

impl Job {
    fn reap(&mut self) {
        let Some(child) = &mut self.child else { return };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.status = Some(status);
                self.child.take();
                if matches!(self.policy, CompletionPolicy::ChildExit) {
                    for port in self
                        .ports
                        .values_mut()
                        .filter(|port| port.descriptor.read && !port.eof)
                    {
                        match rustix::io::ioctl_fionread(&port.descriptor.fd) {
                            Ok(remaining) => port.drain_left = Some(remaining as usize),
                            Err(error) => {
                                self.error.get_or_insert(error.into());
                                port.drain_left = Some(0);
                            }
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
                self.error.get_or_insert(error);
                self.child.take();
            }
            Err(error) => {
                self.error.get_or_insert(error);
                self.cancel();
            }
        }
    }

    fn cancel(&mut self) {
        if self.cancelled {
            return;
        }
        self.cancelled = true;
        self.deadline = None;
        if self.process_group
            && let Some(pid) = rustix::process::Pid::from_raw(self.pid as i32)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        if let Some(child) = &mut self.child
            && let Err(error) = child.kill()
        {
            self.error.get_or_insert(error);
        }
    }

    fn finished(&self) -> bool {
        self.child.is_none()
            && (self.cancelled
                || match self.policy {
                    CompletionPolicy::ChildExit => self.ports.values().all(|port| {
                        port.pending_output.is_none()
                            && port.drain_left.is_none_or(|left| left == 0)
                    }),
                    CompletionPolicy::ChildExitAndEof => self
                        .ports
                        .values()
                        .all(|port| !port.descriptor.read || port.eof),
                })
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if self.child.is_some() || !self.finished() {
            self.cancel();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            loop {
                match child.wait() {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    _ => break,
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct JobRegistry {
    jobs: BTreeMap<JobId, Job>,
    tokens: BTreeMap<Token, JobId>,
    pending: BTreeSet<Token>,
    next_id: u64,
}

impl JobRegistry {
    pub(super) fn register(
        &mut self,
        registry: &Registry,
        next_token: &mut usize,
        launch: Launch,
    ) -> io::Result<JobId> {
        let Launch {
            child,
            descriptors,
            policy,
            deadline,
            process_group,
            output_limit,
            stream,
            pipe,
            cancel,
            complete,
        } = launch;
        let mut job = Job {
            pid: child.id(),
            child: Some(child),
            ports: BTreeMap::new(),
            output: descriptors.iter().map(|_| Vec::new()).collect(),
            policy,
            deadline,
            process_group,
            output_limit,
            stream,
            pipe,
            cancel_flag: cancel,
            status: None,
            error: None,
            cancelled: false,
            complete: Some(complete),
        };
        let attached = (|| {
            self.next_id = self
                .next_id
                .checked_add(1)
                .ok_or_else(|| io::Error::other("job ids exhausted"))?;
            for (index, descriptor) in descriptors.into_iter().enumerate() {
                let flags = rustix::fs::fcntl_getfl(&descriptor.fd)?;
                rustix::fs::fcntl_setfl(&descriptor.fd, flags | rustix::fs::OFlags::NONBLOCK)?;
                let token = allocate_token(next_token)?;
                let mut port = Port {
                    descriptor,
                    index,
                    written: 0,
                    eof: false,
                    registered: false,
                    drain_left: None,
                    pending_output: None,
                };
                port.sync(registry, token)?;
                job.ports.insert(token, port);
            }
            Ok::<_, io::Error>(())
        })();
        if let Err(error) = attached {
            if let Some(stream) = &mut job.stream {
                stream.publish(true, true);
            }
            for port in job.ports.values_mut() {
                if port.registered {
                    let _ = registry.deregister(&mut SourceFd(&port.descriptor.fd.as_raw_fd()));
                }
            }
            job.cancel();
            if let Some(mut child) = job.child.take() {
                let _ = child.wait();
            }
            if let Some(complete) = job.complete.take() {
                complete(Completion {
                    id: JobId(self.next_id),
                    pid: job.pid,
                    status: None,
                    output: std::mem::take(&mut job.output),
                    error: Some(io::Error::other(error.to_string())),
                    cancelled: false,
                });
            }
            return Err(error);
        }
        let id = JobId(self.next_id);
        let tokens = job.ports.keys().copied().collect::<Vec<_>>();
        for token in &tokens {
            self.tokens.insert(*token, id);
        }
        self.jobs.insert(id, job);
        for token in tokens {
            self.ready(registry, token, true, true);
        }
        if let Some(job) = self.jobs.get_mut(&id) {
            job.reap();
            if job.child.is_none() {
                self.pending
                    .extend(job.ports.iter().filter_map(|(&token, port)| {
                        port.drain_left.filter(|left| *left > 0).map(|_| token)
                    }));
            }
        }
        self.finish(registry, id);
        Ok(id)
    }

    pub(super) fn contains(&self, id: JobId) -> bool {
        self.jobs.contains_key(&id)
    }

    pub(super) fn contains_token(&self, token: Token) -> bool {
        self.tokens.contains_key(&token)
    }

    pub(super) fn ready(
        &mut self,
        registry: &Registry,
        token: Token,
        readable: bool,
        writable: bool,
    ) {
        let Some(&id) = self.tokens.get(&token) else {
            return;
        };
        let job = self.jobs.get_mut(&id).unwrap();
        let port = job.ports.get_mut(&token).unwrap();
        let result = (|| {
            if readable
                && port.descriptor.read
                && !port.eof
                && port.drain_left != Some(0)
                && port.pending_output.is_none()
            {
                let mut left = IO_BURST.min(port.drain_left.unwrap_or(usize::MAX));
                let mut buffer = [0; 8192];
                while left != 0 {
                    match rustix::io::read(&port.descriptor.fd, &mut buffer[..left.min(8192)]) {
                        Ok(0) => {
                            if let Some(stream) = &mut job.stream {
                                stream.eof();
                            }
                            port.eof = true;
                            port.drain_left = Some(0);
                            break;
                        }
                        Ok(count) => {
                            if job.output_limit.is_some_and(|limit| {
                                job.output
                                    .iter()
                                    .map(Vec::len)
                                    .sum::<usize>()
                                    .saturating_add(count)
                                    > limit
                            }) {
                                return Err(io::Error::other("job output limit exceeded"));
                            }
                            if let Some(pipe) = &job.pipe {
                                let bytes = Arc::<[u8]>::from(&buffer[..count]);
                                if !pipe
                                    .terminal
                                    .lock()
                                    .send_raw_input_notified(Arc::clone(&bytes), &pipe.waker)
                                {
                                    port.pending_output = Some(bytes);
                                }
                            } else if let Some(stream) = &mut job.stream {
                                stream.read(&buffer[..count]);
                            } else {
                                job.output[port.index].extend_from_slice(&buffer[..count]);
                            }
                            left -= count;
                            if let Some(remaining) = &mut port.drain_left {
                                *remaining = remaining.saturating_sub(count);
                            }
                            if port.pending_output.is_some() {
                                break;
                            }
                        }
                        Err(rustix::io::Errno::INTR) => {}
                        Err(rustix::io::Errno::AGAIN) => {
                            if port.drain_left.is_some() {
                                port.drain_left = Some(0);
                            }
                            break;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                if left == 0 && port.drain_left != Some(0) && port.pending_output.is_none() {
                    self.pending.insert(token);
                }
            }
            if writable && let Some(input) = &port.descriptor.input {
                let end = input.len().min(port.written.saturating_add(IO_BURST));
                let mut broken_pipe = false;
                while port.written < end {
                    match rustix::io::write(&port.descriptor.fd, &input[port.written..end]) {
                        Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                        Ok(count) => port.written += count,
                        Err(rustix::io::Errno::INTR) => {}
                        Err(rustix::io::Errno::AGAIN) => break,
                        Err(rustix::io::Errno::PIPE) => {
                            broken_pipe = true;
                            if let Some(pipe) = &mut job.pipe {
                                pipe.input = None;
                            }
                            port.written = input.len();
                            break;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                if port.written == input.len() {
                    port.descriptor.input = None;
                    if port.descriptor.socket && !broken_pipe {
                        rustix::net::shutdown(&port.descriptor.fd, rustix::net::Shutdown::Write)?;
                    }
                } else if port.written == end {
                    self.pending.insert(token);
                }
            }
            port.sync(registry, token)?;
            Ok::<_, io::Error>(())
        })();
        let remove = !port.registered && !port.descriptor.read && job.pipe.is_none();
        if let Err(error) = result {
            job.error.get_or_insert(error);
            job.cancel();
        }
        if remove {
            job.ports.remove(&token);
            self.tokens.remove(&token);
            self.pending.remove(&token);
        }
        self.finish(registry, id);
    }

    pub(super) fn child_signal(&mut self, registry: &Registry) {
        let ids = self.jobs.keys().copied().collect::<Vec<_>>();
        for id in ids {
            let job = self.jobs.get_mut(&id).unwrap();
            let tokens = job.ports.keys().copied().collect::<Vec<_>>();
            for token in tokens {
                self.ready(registry, token, true, false);
            }
            if let Some(job) = self.jobs.get_mut(&id) {
                job.reap();
                if job.child.is_none() {
                    self.pending
                        .extend(job.ports.iter().filter_map(|(&token, port)| {
                            port.drain_left.filter(|left| *left > 0).map(|_| token)
                        }));
                }
            }
            self.finish(registry, id);
        }
    }

    fn finish(&mut self, registry: &Registry, id: JobId) {
        if !self.jobs.get(&id).is_some_and(Job::finished) {
            return;
        }
        if self
            .pending
            .iter()
            .any(|token| self.tokens.get(token) == Some(&id))
            && !self.jobs[&id].cancelled
        {
            return;
        }
        let mut job = self.jobs.remove(&id).unwrap();
        for (&token, port) in &job.ports {
            self.tokens.remove(&token);
            self.pending.remove(&token);
            if port.registered {
                let _ = registry.deregister(&mut SourceFd(&port.descriptor.fd.as_raw_fd()));
            }
        }
        let complete = job.complete.take().unwrap();
        if let Some(stream) = &mut job.stream {
            stream.publish(true, false);
        }
        complete(Completion {
            id,
            pid: job.pid,
            status: job.status,
            output: std::mem::take(&mut job.output),
            error: job.error.take(),
            cancelled: job.cancelled,
        });
    }

    pub(super) fn cancel(&mut self, registry: &Registry, id: JobId) {
        if let Some(job) = self.jobs.get_mut(&id) {
            job.cancel();
            job.reap();
        }
        self.finish(registry, id);
    }

    pub(super) fn cancel_all(&mut self, registry: &Registry) {
        for id in self.jobs.keys().copied().collect::<Vec<_>>() {
            self.cancel(registry, id);
        }
    }

    pub(super) fn next(&self, now: Instant) -> Option<Instant> {
        if !self.pending.is_empty() {
            return Some(now);
        }
        self.jobs.values().filter_map(|job| job.deadline).min()
    }

    pub(super) fn turn(&mut self, registry: &Registry, now: Instant) {
        let ids = self.jobs.keys().copied().collect::<Vec<_>>();
        for id in ids {
            if self.jobs[&id]
                .cancel_flag
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::Acquire))
            {
                self.cancel(registry, id);
                continue;
            }
            let job = self.jobs.get_mut(&id).unwrap();
            let Some(pipe) = &mut job.pipe else {
                continue;
            };
            for (&token, port) in &mut job.ports {
                if let Some(bytes) = &port.pending_output
                    && pipe
                        .terminal
                        .lock()
                        .send_raw_input_notified(Arc::clone(bytes), &pipe.waker)
                {
                    port.pending_output = None;
                    self.pending.insert(token);
                }
                if !port.descriptor.read
                    && port.descriptor.input.is_none()
                    && let Some(input) = &pipe.input
                {
                    match input.try_recv() {
                        Ok(bytes) => {
                            port.descriptor.input = Some(bytes.to_vec());
                            port.written = 0;
                            self.pending.insert(token);
                        }
                        Err(crossbeam_channel::TryRecvError::Disconnected) => {
                            pipe.input = None;
                        }
                        Err(crossbeam_channel::TryRecvError::Empty) => {}
                    }
                }
            }
        }
        let due = self
            .jobs
            .iter()
            .filter_map(|(&id, job)| job.deadline.filter(|deadline| *deadline <= now).map(|_| id))
            .collect::<Vec<_>>();
        for id in due {
            self.cancel(registry, id);
        }
        for token in std::mem::take(&mut self.pending) {
            self.ready(registry, token, true, true);
        }
    }
}

#[cfg(test)]
#[path = "jobs_e12_tests.rs"]
mod e12_tests;

#[cfg(test)]
#[path = "jobs_e16fix_tests.rs"]
mod e16fix_tests;

#[cfg(test)]
#[path = "jobs_reviewfixes_tests.rs"]
mod reviewfixes_tests;

pub(super) struct JobChild {
    pid: u32,
    status: Option<ExitStatus>,
}

#[cfg(test)]
impl From<std::process::Child> for JobChild {
    fn from(child: std::process::Child) -> Self {
        Self {
            pid: child.id(),
            status: None,
        }
    }
}

impl JobChild {
    pub(super) fn id(&self) -> u32 {
        self.pid
    }

    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.reap(rustix::process::WaitOptions::NOHANG)
    }

    pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
        self.reap(rustix::process::WaitOptions::empty())?
            .ok_or_else(|| io::Error::other("waitpid returned without a status"))
    }

    pub(super) fn kill(&mut self) -> io::Result<()> {
        if self.status.is_some() {
            return Ok(());
        }
        rustix::process::kill_process(self.process(), rustix::process::Signal::KILL)
            .map_err(Into::into)
    }

    pub(super) fn terminate(&mut self) {
        if self.status.is_none() {
            let _ =
                rustix::process::kill_process_group(self.process(), rustix::process::Signal::KILL);
        }
        let _ = self.kill();
        let _ = self.wait();
    }

    fn reap(&mut self, options: rustix::process::WaitOptions) -> io::Result<Option<ExitStatus>> {
        use std::os::unix::process::ExitStatusExt as _;
        while self.status.is_none() {
            match rustix::process::waitpid(Some(self.process()), options) {
                Err(rustix::io::Errno::INTR) => {}
                Err(error) => return Err(error.into()),
                Ok(None) => break,
                Ok(Some((_, status))) => self.status = Some(ExitStatus::from_raw(status.as_raw())),
            }
        }
        Ok(self.status)
    }

    fn process(&self) -> rustix::process::Pid {
        rustix::process::Pid::from_raw(self.pid.cast_signed())
            .expect("a spawned child has a positive pid")
    }
}

pub(super) enum Io {
    Null,
    Piped,
    Fd(OwnedFd),
}

#[derive(Clone, Copy)]
pub(super) enum Environment {
    Inherited,
    Cleared,
}

pub(super) struct Spawned {
    pub(super) child: JobChild,
    pub(super) stdin: Option<OwnedFd>,
    pub(super) stdout: Option<OwnedFd>,
    pub(super) stderr: Option<OwnedFd>,
}

#[cfg(target_os = "macos")]
pub(super) fn spawn(
    command: &mut Command,
    environment: Environment,
    streams: [Io; 3],
) -> io::Result<Spawned> {
    use std::{
        ffi::{CString, OsStr},
        os::unix::ffi::{OsStrExt as _, OsStringExt as _},
    };
    let c_string = |value: &OsStr| {
        CString::new(value.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    };
    let program = command.get_program();
    let argv = std::iter::once(Path::new(program).file_name().unwrap_or(program))
        .chain(command.get_args())
        .map(c_string)
        .collect::<io::Result<Vec<_>>>()?;
    let mut variables = match environment {
        Environment::Inherited => std::env::vars_os().collect(),
        Environment::Cleared => BTreeMap::new(),
    };
    for (name, value) in command.get_envs() {
        match value {
            Some(value) => variables.insert(name.to_owned(), value.to_owned()),
            None => variables.remove(name),
        };
    }
    let envp = variables
        .into_iter()
        .map(|(name, value)| {
            let mut entry = name.into_vec();
            entry.push(b'=');
            entry.extend_from_slice(value.as_bytes());
            CString::new(entry).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut spawn = zz_terminal::posix_spawn::PosixSpawn::new()?;
    spawn.process_group(0)?;
    if let Some(directory) = command.get_current_dir() {
        spawn.chdir(&c_string(directory.as_os_str())?)?;
    }
    let mut child_ends = Vec::new();
    let mut attach = |target, io| -> io::Result<Option<OwnedFd>> {
        match io {
            Io::Null => {
                let access = if target == 0 {
                    libc::O_RDONLY
                } else {
                    libc::O_WRONLY
                };
                spawn.open(target, c"/dev/null", access)?;
                Ok(None)
            }
            Io::Piped => {
                let (read, write) = rustix::pipe::pipe()?;
                rustix::io::fcntl_setfd(&read, rustix::io::FdFlags::CLOEXEC)?;
                rustix::io::fcntl_setfd(&write, rustix::io::FdFlags::CLOEXEC)?;
                let (parent, child) = if target == 0 {
                    (write, read)
                } else {
                    (read, write)
                };
                spawn.dup2(child.as_raw_fd(), target)?;
                child_ends.push(child);
                Ok(Some(parent))
            }
            Io::Fd(fd) => {
                spawn.dup2(fd.as_raw_fd(), target)?;
                child_ends.push(fd);
                Ok(None)
            }
        }
    };
    let [stdin, stdout, stderr] = streams;
    let stdin = attach(0, stdin)?;
    let stdout = attach(1, stdout)?;
    let stderr = attach(2, stderr)?;
    let program = c_string(program)?;
    let pid = loop {
        match spawn.spawn(&program, &argv, &envp) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            result => break result?,
        }
    };
    drop(child_ends);
    Ok(Spawned {
        child: JobChild { pid, status: None },
        stdin,
        stdout,
        stderr,
    })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn spawn(
    command: &mut Command,
    _: Environment,
    streams: [Io; 3],
) -> io::Result<Spawned> {
    use crate::unmasked::SpawnUnmasked as _;
    use std::{os::unix::process::CommandExt as _, process::Stdio};
    if let Some(name) = Path::new(command.get_program()).file_name() {
        let name = name.to_owned();
        command.arg0(name);
    }
    let [stdin, stdout, stderr] = streams.map(|io| match io {
        Io::Null => Stdio::null(),
        Io::Piped => Stdio::piped(),
        Io::Fd(fd) => Stdio::from(fd),
    });
    command
        .process_group(0)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr);
    let mut child = loop {
        match command.spawn_unmasked() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            result => break result?,
        }
    };
    Ok(Spawned {
        stdin: child.stdin.take().map(OwnedFd::from),
        stdout: child.stdout.take().map(OwnedFd::from),
        stderr: child.stderr.take().map(OwnedFd::from),
        child: JobChild {
            pid: child.id(),
            status: None,
        },
    })
}

pub(super) fn launch_status(mut command: Command, mut output: StatusOutput) -> io::Result<Launch> {
    let spawned = match spawn(
        &mut command,
        Environment::Cleared,
        [Io::Null, Io::Piped, Io::Null],
    ) {
        Ok(spawned) => spawned,
        Err(error) => {
            output.publish(true, true);
            return Err(error);
        }
    };
    let stdout = spawned.stdout.unwrap();
    output.fd = stdout.as_raw_fd();
    output.pid = spawned.child.id();
    output.publish(false, false);
    Ok(Launch {
        child: spawned.child,
        descriptors: vec![Descriptor {
            fd: stdout,
            read: true,
            input: None,
            socket: false,
        }],
        policy: CompletionPolicy::ChildExitAndEof,
        deadline: None,
        process_group: true,
        output_limit: None,
        stream: Some(output),
        pipe: None,
        cancel: None,
        complete: Box::new(|_| {}),
    })
}

pub(super) fn launch_shell(
    shared: &Arc<Shared>,
    command: &str,
    cwd: &Path,
    tmux: &str,
    environment: &[(RawText, Option<RawText>)],
    default_terminal: &str,
    startup_reentry: Option<String>,
    tmux_shim: Option<&Path>,
    zz_executable: Option<&Path>,
    show_stderr: bool,
    permit: ShellJobPermit,
    callback: impl FnOnce(Result<ShellJobResult, ()>) + Send + 'static,
) -> Result<(), DaemonError> {
    use std::os::unix::net::UnixStream;
    let (output, child_socket) = UnixStream::pair()?;
    let cwd = existing_job_working_directory(cwd);
    let mut process = shell_process(command);
    configure_shell_job_environment(
        &mut process,
        environment,
        default_terminal,
        startup_reentry.is_some(),
        tmux,
        shared.socket_path.as_os_str(),
        tmux_shim,
        zz_executable,
    );
    process.current_dir(&cwd).env("PWD", cwd.as_os_str());
    if let Some(startup_reentry) = startup_reentry {
        process.env(crate::STARTUP_REENTRY_ENVIRONMENT_VARIABLE, startup_reentry);
    }
    let socket = || child_socket.try_clone().map(|socket| Io::Fd(socket.into()));
    let stdio = [
        socket()?,
        socket()?,
        if show_stderr { socket()? } else { Io::Null },
    ];
    output.shutdown(std::net::Shutdown::Write)?;
    let child = spawn(&mut process, Environment::Cleared, stdio)?.child;
    *permit.process.lock() = Some(super::ShellJobProcess {
        pid: child.id(),
        cancel: Arc::clone(&permit.cancel),
    });
    drop(process);
    drop(child_socket);
    shared.pipe_jobs.launch(Launch {
        child,
        descriptors: vec![Descriptor {
            fd: output.into(),
            read: true,
            input: None,
            socket: true,
        }],
        policy: CompletionPolicy::ChildExit,
        deadline: None,
        process_group: true,
        output_limit: None,
        stream: None,
        pipe: None,
        cancel: Some(Arc::clone(&permit.cancel)),
        complete: Box::new(move |completion| {
            drop(permit);
            let result = if completion.error.is_some() || completion.cancelled {
                Err(())
            } else if let Some(status) = completion.status {
                Ok(ShellJobResult {
                    output: completion.output.into_iter().next().unwrap_or_default(),
                    status,
                })
            } else {
                Err(())
            };
            callback(result);
        }),
    })
}
