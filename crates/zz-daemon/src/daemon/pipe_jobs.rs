use super::*;

#[derive(Clone)]
pub(super) struct Client {
    sender: crossbeam_channel::Sender<Request>,
    pub(super) receiver: crossbeam_channel::Receiver<Request>,
    pub(super) wake: Arc<AcceptWake>,
    pending: Arc<AtomicBool>,
}

impl Default for Client {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self {
            sender,
            receiver,
            wake: Arc::new(AcceptWake::new()),
            pending: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Client {
    pub(super) fn take_pending(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }

    pub(super) fn notify(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.wake.wake();
        }
    }

    pub(super) fn launch(&self, launch: jobs::Launch) -> Result<(), DaemonError> {
        self.sender
            .send(Request(Some(launch)))
            .map_err(|_| DaemonError::Thread("pipe job loop stopped".into()))?;
        self.notify();
        Ok(())
    }
}

#[cfg(test)]
pub(super) struct Driver {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

#[cfg(test)]
impl Driver {
    pub(super) fn new(shared: &Arc<Shared>) -> Self {
        let mut event_loop = event_loop::EventLoop::empty(shared).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let shared = Arc::clone(shared);
        let worker = thread::Builder::new()
            .name("zz-test-mux".into())
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    event_loop.pipe_test_turn(&shared);
                    thread::sleep(Duration::from_millis(2));
                }
                event_loop.pipe_test_stop(&shared);
            })
            .unwrap();
        Self {
            stop,
            worker: Some(worker),
        }
    }
}

#[cfg(test)]
impl Drop for Driver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

impl std::task::Wake for AcceptWake {
    fn wake(self: Arc<Self>) {
        AcceptWake::wake(&self);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        AcceptWake::wake(self);
    }
}

pub(super) struct Request(Option<jobs::Launch>);

impl Request {
    pub(super) fn into_launch(mut self) -> jobs::Launch {
        self.0.take().unwrap()
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        if let Some(mut launch) = self.0.take() {
            terminate_copy_pipe(&mut launch.child);
        }
    }
}
