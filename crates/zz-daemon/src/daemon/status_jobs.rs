use std::{
    io,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
pub(crate) struct StatusUpdate {
    pub(crate) latest: Option<String>,
    pub(crate) complete: bool,
    pub(crate) streamed: bool,
    pub(crate) failed: bool,
    pub(crate) fd: i32,
    pub(crate) pid: u32,
}

pub(super) struct StatusOutput {
    updates: crossbeam_channel::Sender<StatusUpdate>,
    stale: crossbeam_channel::Receiver<StatusUpdate>,
    waker: Option<std::task::Waker>,
    pending: Vec<u8>,
    latest: Option<String>,
    updated: bool,
    had_line: bool,
    pub(super) fd: i32,
    pub(super) pid: u32,
}

impl StatusOutput {
    pub(super) fn publish(&mut self, complete: bool, failed: bool) {
        let mut update = StatusUpdate {
            latest: self.latest.take(),
            complete,
            streamed: self.updated,
            failed,
            fd: self.fd,
            pid: self.pid,
        };
        self.updated = false;
        loop {
            match self.updates.try_send(update) {
                Ok(()) => break,
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => return,
                Err(crossbeam_channel::TrySendError::Full(mut next)) => {
                    if let Ok(previous) = self.stale.try_recv() {
                        next.latest = next.latest.or(previous.latest);
                        next.streamed |= previous.streamed;
                        next.complete |= previous.complete;
                        next.failed |= previous.failed;
                    }
                    update = next;
                }
            }
        }
        if let Some(waker) = &self.waker {
            waker.wake_by_ref();
        }
    }

    pub(super) fn read(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if byte == b'\n' {
                if self.pending.last() == Some(&b'\r') {
                    self.pending.pop();
                }
                self.latest = Some(String::from_utf8_lossy(&self.pending).into_owned());
                self.pending.clear();
                self.updated = true;
                self.had_line = true;
            } else {
                self.pending.push(byte);
            }
        }
        if self.updated {
            self.publish(false, false);
        }
    }

    pub(super) fn eof(&mut self) {
        if !self.pending.is_empty() || !self.had_line {
            self.latest = Some(String::from_utf8_lossy(&self.pending).into_owned());
        }
        self.pending.clear();
        self.publish(false, false);
    }
}

#[derive(Clone)]
pub(crate) struct StatusClient {
    requests: crossbeam_channel::Sender<StatusRequest>,
    receiver: crossbeam_channel::Receiver<StatusRequest>,
    wake: Arc<super::AcceptWake>,
    pending: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    #[cfg(windows)]
    notification: Arc<parking_lot::Mutex<Option<std::task::Waker>>>,
}

pub(super) enum StatusRequest {
    Launch {
        serial: u64,
        command: Box<Command>,
        output: StatusOutput,
    },
    Cancel(u64),
}

impl Default for StatusClient {
    fn default() -> Self {
        let (requests, receiver) = crossbeam_channel::unbounded();
        Self {
            requests,
            receiver,
            wake: Arc::new(super::AcceptWake::new()),
            pending: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            #[cfg(windows)]
            notification: Arc::new(parking_lot::Mutex::new(None)),
        }
    }
}

impl StatusClient {
    fn post(&self, request: StatusRequest) -> io::Result<()> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(io::Error::other("status jobs stopped"));
        }
        self.requests.send(request).map_err(io::Error::other)?;
        self.notify();
        Ok(())
    }

    fn notify(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.wake.wake();
            #[cfg(windows)]
            if let Some(waker) = self.notification.lock().as_ref() {
                waker.wake_by_ref();
            }
        }
    }

    pub(crate) fn launch(
        &self,
        serial: u64,
        command: Command,
        waker: Option<std::task::Waker>,
    ) -> io::Result<crossbeam_channel::Receiver<StatusUpdate>> {
        let (updates, receiver) = crossbeam_channel::bounded(1);
        self.post(StatusRequest::Launch {
            serial,
            command: Box::new(command),
            output: StatusOutput {
                updates,
                stale: receiver.clone(),
                waker,
                pending: Vec::new(),
                latest: None,
                updated: false,
                had_line: false,
                fd: -1,
                pid: 0,
            },
        })?;
        Ok(receiver)
    }

    pub(crate) fn cancel(&self, serial: u64) {
        let _ = self.post(StatusRequest::Cancel(serial));
    }

    #[cfg(unix)]
    pub(super) fn install(&self, waker: Arc<mio::Waker>) {
        self.wake.install(waker);
    }

    #[cfg(windows)]
    pub(crate) fn set_notification(&self, waker: std::task::Waker) {
        *self.notification.lock() = Some(waker);
    }

    pub(super) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }

    pub(super) fn take_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire) && self.pending.swap(false, Ordering::AcqRel)
    }

    pub(super) fn requests(&self) -> impl Iterator<Item = StatusRequest> {
        let requests = self.receiver.try_iter().take(64).collect::<Vec<_>>();
        if !self.receiver.is_empty() {
            self.notify();
        }
        requests.into_iter()
    }
}

#[cfg(all(test, unix))]
#[path = "jobs_e15_tests.rs"]
pub(crate) mod tests;

#[cfg(all(test, windows))]
pub(crate) mod tests {
    use super::*;
    use std::{
        ops::{Deref, DerefMut},
        thread,
        time::Duration,
    };

    pub(crate) struct Driver {
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl Driver {
        pub(crate) fn new(client: StatusClient) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = Arc::clone(&stop);
            let thread = thread::spawn(move || {
                let mut registry = windows::Registry::new(client);
                while !stopping.load(Ordering::Acquire) {
                    registry.turn();
                    thread::sleep(Duration::from_millis(5));
                }
            });
            Self {
                stop,
                thread: Some(thread),
            }
        }
    }

    impl Drop for Driver {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    pub(crate) struct Renderer {
        renderer: crate::status::StatusRenderer,
        _driver: Driver,
    }

    pub(crate) fn renderer() -> Renderer {
        let renderer = crate::status::StatusRenderer::default();
        let driver = Driver::new(renderer.job_client());
        Renderer {
            renderer,
            _driver: driver,
        }
    }

    impl Deref for Renderer {
        type Target = crate::status::StatusRenderer;
        fn deref(&self) -> &Self::Target {
            &self.renderer
        }
    }

    impl DerefMut for Renderer {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.renderer
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        collections::BTreeMap,
        io::Read as _,
        os::windows::io::AsRawHandle as _,
        process::{Child, ChildStdout, Stdio},
        time::{Duration, Instant},
    };
    use zz_daemon_client::unmasked::SpawnUnmasked as _;

    struct Job {
        child: Option<Child>,
        stdout: Option<ChildStdout>,
        output: StatusOutput,
        cancelled: bool,
    }

    impl Job {
        fn cancel(&mut self) {
            if self.cancelled {
                return;
            }
            self.cancelled = true;
            if let Some(child) = &mut self.child {
                let pid = child.id().to_string();
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid, "/T", "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status_unmasked();
                let _ = child.kill();
            }
            self.stdout = None;
        }

        #[allow(unsafe_code)]
        fn read(&mut self) -> io::Result<()> {
            let Some(stdout) = &mut self.stdout else {
                return Ok(());
            };
            let mut left = 256 * 1024;
            let mut buffer = [0; 8192];
            while left != 0 {
                let mut available = 0;
                let ready = unsafe {
                    windows_sys::Win32::System::Pipes::PeekNamedPipe(
                        stdout.as_raw_handle(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        &mut available,
                        std::ptr::null_mut(),
                    )
                };
                if ready == 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error()
                        == Some(windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE as i32)
                    {
                        self.output.eof();
                        self.stdout = None;
                        return Ok(());
                    }
                    return Err(error);
                }
                if available == 0 {
                    return Ok(());
                }
                let length = left.min(buffer.len()).min(available as usize);
                match stdout.read(&mut buffer[..length]) {
                    Ok(0) => {
                        self.output.eof();
                        self.stdout = None;
                        return Ok(());
                    }
                    Ok(count) => {
                        self.output.read(&buffer[..count]);
                        left -= count;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }

        fn turn(&mut self) -> bool {
            if self.read().is_err() {
                self.cancel();
            }
            if let Some(child) = &mut self.child {
                match super::super::try_reap_shell_job_child(child) {
                    Ok(Some(_)) => {
                        self.child = None;
                    }
                    Ok(None) => {}
                    Err(_) => self.cancel(),
                }
            }
            self.child.is_none() && self.stdout.is_none()
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            if self.child.is_some() {
                self.cancel();
            }
            if let Some(mut child) = self.child.take() {
                loop {
                    match child.wait() {
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        _ => break,
                    }
                }
            }
            self.output.publish(true, false);
        }
    }

    pub(in crate::daemon) struct Registry {
        client: StatusClient,
        jobs: BTreeMap<u64, Job>,
    }

    impl Registry {
        pub(in crate::daemon) fn new(client: StatusClient) -> Self {
            Self {
                client,
                jobs: BTreeMap::new(),
            }
        }

        pub(in crate::daemon) fn turn(&mut self) {
            if self.client.take_pending() {
                for request in self.client.requests() {
                    match request {
                        StatusRequest::Launch {
                            serial,
                            mut command,
                            mut output,
                        } => {
                            command
                                .stdin(Stdio::null())
                                .stdout(Stdio::piped())
                                .stderr(Stdio::null());
                            let child = loop {
                                match command.spawn_unmasked() {
                                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                                    result => break result,
                                }
                            };
                            match child {
                                Ok(mut child) => {
                                    output.pid = child.id();
                                    output.publish(false, false);
                                    let stdout = child.stdout.take();
                                    self.jobs.insert(
                                        serial,
                                        Job {
                                            child: Some(child),
                                            stdout,
                                            output,
                                            cancelled: false,
                                        },
                                    );
                                }
                                Err(_) => output.publish(true, true),
                            }
                        }
                        StatusRequest::Cancel(serial) => {
                            if let Some(job) = self.jobs.get_mut(&serial) {
                                job.cancel();
                            }
                        }
                    }
                }
            }
            self.jobs.retain(|_, job| !job.turn());
        }

        pub(in crate::daemon) fn next(&self) -> Option<Instant> {
            (!self.jobs.is_empty()).then(|| Instant::now() + Duration::from_millis(10))
        }
    }

    impl Drop for Registry {
        fn drop(&mut self) {
            self.client.stop();
        }
    }
}

#[cfg(windows)]
pub(super) use windows::Registry as WindowsRegistry;
