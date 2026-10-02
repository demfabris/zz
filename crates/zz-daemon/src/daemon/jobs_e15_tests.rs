use super::*;
use crate::daemon::jobs::{Completion, JobRegistry, launch_status};
use mio::Token;
use std::{collections::BTreeMap, time::Instant};
use std::{
    ops::{Deref, DerefMut},
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

pub(crate) struct Driver {
    stop: Arc<AtomicBool>,
    wake: Arc<mio::Waker>,
    thread: Option<thread::JoinHandle<()>>,
    completed: Arc<Mutex<Vec<Completion>>>,
}

impl Driver {
    pub(crate) fn new(client: StatusClient) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(Mutex::new(Vec::new()));
        let (ready, started) = mpsc::sync_channel(1);
        let stopping = Arc::clone(&stop);
        let results = Arc::clone(&completed);
        let thread = thread::spawn(move || {
            let mut poll = mio::Poll::new().unwrap();
            let wake = Arc::new(mio::Waker::new(poll.registry(), Token(1)).unwrap());
            client.install(Arc::clone(&wake));
            ready.send(wake).unwrap();
            let mut jobs = JobRegistry::default();
            let mut active = BTreeMap::new();
            let mut next = 4;
            let mut events = mio::Events::with_capacity(64);
            let mut shutdown = None;
            loop {
                if stopping.load(Ordering::Acquire) && shutdown.is_none() {
                    client.stop();
                    jobs.cancel_all(poll.registry());
                    shutdown = Some(Instant::now() + Duration::from_secs(3));
                }
                if client.take_pending() {
                    active.retain(|_, id| jobs.contains(*id));
                    for request in client.requests() {
                        match request {
                            StatusRequest::Launch {
                                serial,
                                command,
                                output,
                            } => {
                                let mut launch = launch_status(*command, output).unwrap();
                                let results = Arc::clone(&results);
                                launch.complete =
                                    Box::new(move |result| results.lock().unwrap().push(result));
                                let id = jobs.register(poll.registry(), &mut next, launch).unwrap();
                                active.insert(serial, id);
                                if shutdown.is_some() {
                                    jobs.cancel(poll.registry(), id);
                                }
                            }
                            StatusRequest::Cancel(serial) => {
                                if let Some(id) = active.remove(&serial) {
                                    jobs.cancel(poll.registry(), id);
                                }
                            }
                        }
                    }
                }
                jobs.turn(poll.registry(), Instant::now());
                jobs.child_signal(poll.registry());
                if let Some(deadline) = shutdown {
                    if active.values().all(|id| !jobs.contains(*id)) {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "status jobs did not reap at shutdown"
                    );
                }
                match poll.poll(&mut events, Some(Duration::from_millis(5))) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => panic!("status poll failed: {error}"),
                }
                for event in &events {
                    jobs.ready(
                        poll.registry(),
                        event.token(),
                        event.is_readable() || event.is_read_closed(),
                        false,
                    );
                }
            }
        });
        Self {
            stop,
            wake: started.recv().unwrap(),
            thread: Some(thread),
            completed,
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.wake.wake().unwrap();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.shutdown();
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

fn assert_reaped_once(result: &Completion) {
    assert!(result.cancelled);
    assert!(result.status.is_some());
    let pid = rustix::process::Pid::from_raw(result.pid as i32).unwrap();
    loop {
        match rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG) {
            Err(rustix::io::Errno::INTR) => {}
            result => {
                assert_eq!(result.unwrap_err(), rustix::io::Errno::CHILD);
                break;
            }
        }
    }
}

#[test]
fn status_jobs_cancel_and_reap_once_on_eviction_and_shutdown() {
    for shutdown in [false, true] {
        let client = StatusClient::default();
        let mut driver = Driver::new(client.clone());
        let command = crate::shell_process("printf 'ready\\n'; exec sleep 30");
        let updates = client.launch(1, command, None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let update = updates
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if update.latest.as_deref() == Some("ready") {
                break;
            }
        }
        if !shutdown {
            client.cancel(1);
            client.cancel(1);
            while driver.completed.lock().unwrap().is_empty() {
                assert!(Instant::now() < deadline, "evicted job did not reap");
                thread::sleep(Duration::from_millis(1));
            }
        }
        driver.shutdown();
        let results = driver.completed.lock().unwrap();
        assert_eq!(results.len(), 1);
        assert_reaped_once(&results[0]);
    }
}

#[test]
fn status_output_coalesces_lines_and_preserves_cr_and_partial_tails() {
    let client = StatusClient::default();
    let mut driver = Driver::new(client.clone());
    let command = crate::shell_process("printf 'first\\r\\nsecond \\t\\ntail\\r'");
    let updates = client.launch(1, command, None).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while driver.completed.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let update = updates.try_recv().unwrap();
    assert_eq!(update.latest.as_deref(), Some("tail\r"));
    assert!(update.complete);
    assert!(update.streamed);
    driver.shutdown();
}
