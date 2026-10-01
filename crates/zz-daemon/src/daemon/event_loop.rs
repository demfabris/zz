use mio::{Events, Interest, Poll, Token, Waker, unix::SourceFd};

use super::*;

const LISTENER: Token = Token(0);
const WAKE: Token = Token(1);

pub(super) struct EventLoop {
    poll: Poll,
    events: Events,
    startup_finished: mpsc::Receiver<()>,
    startup_sender: mpsc::Sender<()>,
    waker: Arc<Waker>,
}

pub(super) struct StartupNotifier {
    sender: mpsc::Sender<()>,
    waker: Arc<Waker>,
}

impl Drop for StartupNotifier {
    fn drop(&mut self) {
        let _ = self.sender.send(());
        if let Err(error) = self.waker.wake() {
            log::warn!("could not wake the mux loop after startup: {error}");
        }
    }
}

impl EventLoop {
    pub(super) fn new<T: Transport>(
        listener: &T::Listener,
        shared: &Shared,
    ) -> Result<Self, DaemonError> {
        let poll = Poll::new()?;
        poll.registry().register(
            &mut SourceFd(&listener.raw_fd()),
            LISTENER,
            Interest::READABLE,
        )?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKE)?);
        shared.accept_wake.install(Arc::clone(&waker));
        let (startup_sender, startup_finished) = mpsc::channel();
        Ok(Self {
            poll,
            events: Events::with_capacity(8),
            startup_finished,
            startup_sender,
            waker,
        })
    }

    pub(super) fn startup_notifier(&self) -> StartupNotifier {
        StartupNotifier {
            sender: self.startup_sender.clone(),
            waker: Arc::clone(&self.waker),
        }
    }

    pub(super) fn run<T: Transport>(
        &mut self,
        listener: &T::Listener,
        shared: &Arc<Shared>,
        initialized: impl FnOnce(),
    ) -> Result<(), DaemonError> {
        let mut initialized = Some(initialized);
        loop {
            while self.startup_finished.try_recv().is_ok() {
                if let Some(initialized) = initialized.take() {
                    initialized();
                }
            }
            if shared.stopping.load(Ordering::Acquire) {
                return Ok(());
            }
            match self.poll.poll(&mut self.events, None) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
            for event in &self.events {
                if event.token() == LISTENER {
                    accept_ready::<T>(listener, shared)?;
                }
            }
        }
    }
}

fn accept_ready<T: Transport>(
    listener: &T::Listener,
    shared: &Arc<Shared>,
) -> Result<(), DaemonError> {
    while !shared.stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok(stream) => {
                let connection_shared = Arc::clone(shared);
                if let Err(error) = shared.connection_threads.run(Box::new(move || {
                    if let Err(error) = handle_connection(stream, &connection_shared) {
                        log::debug!("client disconnected: {error}");
                    }
                })) {
                    log::warn!("could not start client connection thread: {error}");
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(super) fn join_startup(
    startup: thread::JoinHandle<Result<(), DaemonError>>,
) -> Result<(), DaemonError> {
    startup
        .join()
        .map_err(|_| DaemonError::Thread("daemon startup thread panicked".to_owned()))?
}
