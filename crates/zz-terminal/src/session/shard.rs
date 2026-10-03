use super::*;
use std::sync::OnceLock;

static SHARDS: LazyLock<Vec<OnceLock<Result<ShardHandle, String>>>> = LazyLock::new(|| {
    let count = shard_count(std::env::var("ZZ_PTY_SHARDS").ok().as_deref());
    (0..count).map(|_| OnceLock::new()).collect()
});
static NEXT_SHARD: AtomicUsize = AtomicUsize::new(0);

fn shard_count(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            thread::available_parallelism()
                .map_or(1, std::num::NonZeroUsize::get)
                .min(4)
        })
}

pub(super) fn choose() -> Result<Option<ShardHandle>, WorkerError> {
    let shards = &*SHARDS;
    if shards.is_empty() {
        return Ok(None);
    }
    let index = NEXT_SHARD.fetch_add(1, Ordering::Relaxed) % shards.len();
    shards[index]
        .get_or_init(|| ShardHandle::start(index).map_err(|error| error.to_string()))
        .as_ref()
        .cloned()
        .map(Some)
        .map_err(|error| WorkerError::Thread(error.clone()))
}

#[derive(Clone)]
pub(super) struct ShardHandle {
    requests: Sender<PaneLaunch>,
    pub(super) wake: ActorWake,
}

pub(super) struct PaneLaunch {
    pub(super) control_rx: Receiver<Command>,
    pub(super) slot: Arc<Mutex<ControlSlot>>,
    pub(super) publisher: Publisher,
    pub(super) max_scrollback: usize,
    pub(super) appearance: Arc<TerminalAppearance>,
    pub(super) kind: LaunchKind,
    pub(super) alive: Sender<Infallible>,
    pub(super) wake: ActorWake,
}

pub(super) enum LaunchKind {
    Pty {
        input_rx: InputReceiver,
        spawn: Box<TerminalSpawn>,
    },
    Surface {
        title: String,
        text: String,
        frozen: bool,
        geometry: Geometry,
    },
}

impl ShardHandle {
    fn start(index: usize) -> Result<Self, WorkerError> {
        let (requests, incoming) = crossbeam_channel::unbounded();
        #[cfg(unix)]
        let (wake_rx, wake) = {
            let (read, write) =
                configured_actor_wake_pipe().map_err(|error| WorkerError::Io(error.into()))?;
            (
                read,
                ActorWake {
                    ready: None,
                    pipe: Some(Arc::new(write)),
                    pending: Some(Arc::new(AtomicBool::new(false))),
                },
            )
        };
        #[cfg(not(unix))]
        let (wake_rx, wake) = {
            let (send, receive) = crossbeam_channel::bounded(1);
            (
                receive,
                ActorWake {
                    ready: None,
                    channel: Some(send),
                },
            )
        };
        #[cfg(unix)]
        let worker_wake = wake.clone();
        #[cfg(target_os = "macos")]
        let kqueue =
            rustix::event::kqueue::kqueue().map_err(|error| WorkerError::Io(error.into()))?;
        thread::Builder::new()
            .name(format!("zz-pty-shard-{index}"))
            .spawn(move || {
                let mut shard = Shard {
                    incoming,
                    #[cfg(unix)]
                    wake: worker_wake,
                    wake_rx,
                    #[cfg(target_os = "macos")]
                    kqueue,
                    #[cfg(target_os = "macos")]
                    wake_registered: false,
                    #[cfg(target_os = "macos")]
                    events: Vec::with_capacity(32),
                    #[cfg(target_os = "linux")]
                    index,
                    #[cfg(target_os = "linux")]
                    gather: None,
                    actors: HashMap::new(),
                    next_id: 0,
                    cursor: 0,
                };
                if let Err(error) = shard.run() {
                    log::error!("PTY shard stopped: {error}");
                    for entry in shard.actors.values() {
                        entry.publisher.fail(&error);
                    }
                }
            })
            .map_err(WorkerError::Io)?;
        Ok(Self { requests, wake })
    }

    pub(super) fn launch(&self, launch: PaneLaunch) -> Result<(), WorkerError> {
        self.requests
            .send(launch)
            .map_err(|_| WorkerError::Thread("PTY shard stopped".into()))?;
        self.wake.notify();
        Ok(())
    }
}

pub(super) enum Actor {
    Live(Box<PaneActor>),
    Dead(Box<pane_actor::DeadPane>),
    Surface(Box<surface_actor::SurfaceActor<'static, 'static>>),
}

impl Actor {
    fn echo_priority(&self, wake: Option<&Wake>) -> bool {
        matches!(wake, Some(Wake::Input(_)))
            || matches!(self, Self::Live(actor) if actor.echo_pending())
    }

    fn next_deadline(&self) -> Instant {
        match self {
            Self::Live(actor) => actor.next_deadline(),
            Self::Dead(actor) => actor.next_deadline(),
            Self::Surface(actor) => actor.next_deadline(),
        }
    }

    fn try_wake(&mut self) -> Result<Option<Wake>, WorkerError> {
        match self {
            Self::Live(actor) => actor.try_wake(),
            Self::Dead(actor) => Ok(actor.try_wake()),
            Self::Surface(actor) => actor.try_wake(),
        }
    }

    pub(super) fn wait_for_wake(&mut self) -> Result<Wake, WorkerError> {
        match self {
            Self::Live(actor) => actor.wait_for_wake(),
            Self::Dead(actor) => Ok(actor.wait_for_wake()),
            Self::Surface(actor) => actor.wait_for_wake(),
        }
    }

    pub(super) fn on_deadline(mut self) -> Result<Option<Self>, WorkerError> {
        match &mut self {
            Self::Live(actor) => {
                if !actor.on_deadline()? {
                    return Ok(None);
                }
                actor.on_parse_deadline();
            }
            Self::Dead(actor) => {
                if !actor.on_wake(Wake::Deadline)? {
                    return self.finish_dead();
                }
            }
            Self::Surface(actor) => actor.on_deadline()?,
        }
        self.finish_live()
    }

    pub(super) fn on_wake(mut self, wake: Wake) -> Result<Option<Self>, WorkerError> {
        let keep = match &mut self {
            Self::Live(actor) => actor.on_wake(wake)?,
            Self::Dead(actor) => actor.on_wake(wake)?,
            Self::Surface(actor) => actor.on_wake(wake)?,
        };
        if !keep {
            return if matches!(self, Self::Dead(_)) {
                self.finish_dead()
            } else {
                Ok(None)
            };
        }
        self.finish_live()
    }

    fn finish_live(self) -> Result<Option<Self>, WorkerError> {
        if let Self::Live(actor) = self {
            if actor.ready_to_finish() {
                return actor.finish().map(|dead| Some(Self::Dead(Box::new(dead))));
            }
            return Ok(Some(Self::Live(actor)));
        }
        Ok(Some(self))
    }

    fn finish_dead(self) -> Result<Option<Self>, WorkerError> {
        let Self::Dead(actor) = self else {
            unreachable!("only dead panes await a notice")
        };
        actor
            .into_surface()
            .map(|surface| surface.map(|surface| Self::Surface(Box::new(surface))))
    }
}

struct Entry {
    actor: Actor,
    publisher: Publisher,
    _alive: Sender<Infallible>,
    deadline: Instant,
    pending: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    poll_sources: [Option<i32>; 2],
}

struct Shard {
    incoming: Receiver<PaneLaunch>,
    #[cfg(unix)]
    wake: ActorWake,
    #[cfg(unix)]
    wake_rx: std::os::fd::OwnedFd,
    #[cfg(not(unix))]
    wake_rx: Receiver<()>,
    #[cfg(target_os = "macos")]
    kqueue: std::os::fd::OwnedFd,
    #[cfg(target_os = "macos")]
    wake_registered: bool,
    #[cfg(target_os = "macos")]
    events: Vec<rustix::event::kqueue::Event>,
    #[cfg(target_os = "linux")]
    index: usize,
    #[cfg(target_os = "linux")]
    gather: Option<PtyGather>,
    actors: HashMap<usize, Entry>,
    next_id: usize,
    cursor: usize,
}

impl Shard {
    fn run(&mut self) -> Result<(), WorkerError> {
        let mut ready = Vec::new();
        loop {
            ready.clear();
            while let Ok(launch) = self.incoming.try_recv() {
                let publisher = launch.publisher.clone();
                #[cfg(unix)]
                let wake_rx = None;
                #[cfg(not(unix))]
                let wake_rx = ();
                let actor = match launch.kind {
                    LaunchKind::Pty { input_rx, spawn } => {
                        #[cfg(target_os = "linux")]
                        let gather = match self.pty_gather() {
                            Ok(gather) => gather,
                            Err(error) => {
                                report_worker_error(&publisher, &error);
                                continue;
                            }
                        };
                        PaneActor::spawn(
                            launch.control_rx,
                            input_rx,
                            launch.slot,
                            launch.publisher,
                            launch.max_scrollback,
                            &launch.appearance,
                            &spawn,
                            &launch.wake,
                            wake_rx,
                            true,
                            #[cfg(target_os = "linux")]
                            gather,
                        )
                        .map(|actor| Actor::Live(Box::new(actor)))
                    }
                    LaunchKind::Surface {
                        title,
                        text,
                        frozen,
                        geometry,
                    } => new_output_view(
                        launch.control_rx,
                        launch.slot,
                        launch.publisher,
                        &title,
                        &text,
                        &launch.appearance,
                        launch.max_scrollback,
                        frozen,
                        geometry,
                        &launch.wake,
                    )
                    .map(|actor| Actor::Surface(Box::new(actor))),
                };
                match actor {
                    Ok(actor) => {
                        let id = self.next_id;
                        self.next_id += 1;
                        let deadline = actor.next_deadline();
                        self.actors.insert(
                            id,
                            Entry {
                                actor,
                                publisher,
                                _alive: launch.alive,
                                deadline,
                                pending: Arc::clone(
                                    launch
                                        .wake
                                        .ready
                                        .as_ref()
                                        .expect("pane wake tracks readiness"),
                                ),
                                #[cfg(target_os = "macos")]
                                poll_sources: [None; 2],
                            },
                        );
                        self.dispatch(id, None);
                    }
                    Err(error) => report_worker_error(&publisher, &error),
                }
            }
            self.collect_channels(&mut ready);
            let timeout = if ready.is_empty() {
                self.poll_timeout()
            } else {
                Duration::ZERO
            };
            #[cfg(unix)]
            self.poll(timeout, &mut ready)?;
            #[cfg(not(unix))]
            self.poll(timeout, &mut ready);
            self.collect_channels(&mut ready);
            self.collect_deadlines(&mut ready);
            ready.sort_unstable_by_key(|(id, wake, _, _)| {
                (
                    !self
                        .actors
                        .get(id)
                        .is_some_and(|entry| entry.actor.echo_priority(wake.as_ref())),
                    *id < self.cursor,
                    *id,
                )
            });
            #[cfg(unix)]
            let only_ready = ready.first().is_some_and(|(only_id, _, _, _)| {
                ready.iter().all(|(id, _, _, _)| id == only_id)
                    && self
                        .actors
                        .iter()
                        .all(|(id, entry)| id == only_id || entry.deadline > Instant::now())
            });
            for (id, wake, pty, child) in ready.drain(..) {
                if let Some(entry) = self.actors.remove(&id) {
                    let result = (|| {
                        let actor = if let Some(wake) = wake {
                            entry.actor.on_wake(wake)?
                        } else {
                            Some(entry.actor)
                        };
                        let Some(actor) = actor else {
                            return Ok(None);
                        };
                        #[cfg(unix)]
                        let mut actor = actor;
                        #[cfg(unix)]
                        if let Actor::Live(actor) = &mut actor {
                            if child {
                                actor.poll_child()?;
                            }
                            #[cfg(target_os = "macos")]
                            actor.retry_child()?;
                            if pty {
                                actor.on_readable_with_spin(only_ready, || {
                                    self.actors.values().any(|entry| {
                                        matches!(&entry.actor, Actor::Live(actor)
                                            if actor.echo_pending()
                                                || (entry.pending.load(Ordering::Acquire)
                                                    && actor.queued_input_ready()))
                                    })
                                })?;
                            }
                        }
                        #[cfg(not(unix))]
                        let _ = pty;
                        #[cfg(not(unix))]
                        let _ = child;
                        actor.on_deadline()
                    })();
                    self.store_result(
                        id,
                        entry.publisher,
                        entry._alive,
                        entry.pending,
                        result,
                        #[cfg(target_os = "macos")]
                        [
                            entry.poll_sources[0],
                            entry.poll_sources[1].filter(|_| !child),
                        ],
                    );
                    self.cursor = id + 1;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn pty_gather(&mut self) -> Result<Option<PtyGather>, WorkerError> {
        if !pty_gather_enabled() {
            return Ok(None);
        }
        if self.gather.is_none() {
            self.gather = Some(PtyGather::start(format!("zz-pty-gather-{}", self.index))?);
        }
        Ok(self.gather.clone())
    }

    fn collect_channels(&mut self, ready: &mut Vec<(usize, Option<Wake>, bool, bool)>) {
        let mut failed = Vec::new();
        for (&id, entry) in &mut self.actors {
            let pending = ready.iter_mut().find(|(ready_id, _, _, _)| *ready_id == id);
            if pending
                .as_ref()
                .is_some_and(|(_, wake, _, _)| wake.is_some())
            {
                continue;
            }
            if !entry.pending.load(Ordering::Acquire)
                || !entry.pending.swap(false, Ordering::AcqRel)
            {
                continue;
            }
            match entry.actor.try_wake() {
                Ok(Some(wake)) => {
                    entry.pending.store(true, Ordering::Release);
                    if let Some((_, pending_wake, _, _)) = pending {
                        *pending_wake = Some(wake);
                    } else {
                        ready.push((id, Some(wake), false, false));
                    }
                }
                Ok(None) => {}
                Err(error) => failed.push((id, error)),
            }
        }
        for (id, error) in failed {
            self.fail(id, &error);
        }
    }

    fn dispatch(&mut self, id: usize, wake: Option<Wake>) {
        if let Some(entry) = self.actors.remove(&id) {
            let result = if let Some(wake) = wake {
                entry.actor.on_wake(wake)
            } else {
                entry.actor.on_deadline()
            };
            self.store_result(
                id,
                entry.publisher,
                entry._alive,
                entry.pending,
                result,
                #[cfg(target_os = "macos")]
                entry.poll_sources,
            );
        }
    }

    fn store_result(
        &mut self,
        id: usize,
        publisher: Publisher,
        alive: Sender<Infallible>,
        pending: Arc<AtomicBool>,
        result: Result<Option<Actor>, WorkerError>,
        #[cfg(target_os = "macos")] poll_sources: [Option<i32>; 2],
    ) {
        match result {
            Ok(Some(actor)) => {
                let deadline = actor.next_deadline();
                #[cfg(target_os = "macos")]
                let recheck_channels =
                    !matches!(&actor, Actor::Live(actor) if !actor.queued_input_ready());
                #[cfg(not(target_os = "macos"))]
                let recheck_channels = true;
                if recheck_channels {
                    pending.store(true, Ordering::Release);
                }
                self.actors.insert(
                    id,
                    Entry {
                        actor,
                        publisher,
                        _alive: alive,
                        deadline,
                        pending,
                        #[cfg(target_os = "macos")]
                        poll_sources,
                    },
                );
            }
            Ok(None) => publisher.set_foreground_source(None),
            Err(error) => {
                report_worker_error(&publisher, &error);
                publisher.set_foreground_source(None);
            }
        }
    }

    fn fail(&mut self, id: usize, error: &WorkerError) {
        if let Some(entry) = self.actors.remove(&id) {
            report_worker_error(&entry.publisher, error);
            entry.publisher.set_foreground_source(None);
        }
    }

    fn collect_deadlines(&self, ready: &mut Vec<(usize, Option<Wake>, bool, bool)>) {
        let now = Instant::now();
        for (&id, entry) in &self.actors {
            if entry.deadline <= now && !ready.iter().any(|(ready_id, _, _, _)| *ready_id == id) {
                ready.push((id, None, false, false));
            }
        }
    }

    fn poll_timeout(&self) -> Duration {
        self.actors
            .values()
            .map(|entry| entry.deadline)
            .min()
            .map_or(IDLE_SLEEP, |due| {
                due.saturating_duration_since(Instant::now())
            })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn poll(
        &self,
        timeout: Duration,
        ready: &mut Vec<(usize, Option<Wake>, bool, bool)>,
    ) -> Result<(), WorkerError> {
        use rustix::event::{PollFd, PollFlags};
        let mut fds = SmallVec::<[PollFd<'_>; 9]>::new();
        fds.push(PollFd::new(&self.wake_rx, PollFlags::IN));
        let mut sources = SmallVec::<[(usize, bool); 8]>::new();
        for (&id, entry) in &self.actors {
            if let Actor::Live(actor) = &entry.actor {
                let (pty, child) = actor.poll_sources();
                if let Some(fd) = pty {
                    fds.push(PollFd::new(fd, PollFlags::IN));
                    sources.push((id, true));
                }
                if let Some(fd) = child {
                    fds.push(PollFd::new(fd, PollFlags::IN));
                    sources.push((id, false));
                }
            }
        }
        let timespec = rustix::event::Timespec::try_from(timeout.min(IDLE_SLEEP))
            .expect("bounded poll timeout");
        match rustix::event::poll(&mut fds, Some(&timespec)) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => return Ok(()),
            Err(error) => return Err(WorkerError::Io(error.into())),
        }
        if !fds[0].revents().is_empty() {
            drain_wake_pipe(&self.wake_rx)?;
            if let Some(pending) = &self.wake.pending {
                pending.store(false, Ordering::Release);
            }
        }
        for ((id, pty), fd) in sources.into_iter().zip(fds.iter().skip(1)) {
            if !fd.revents().is_empty() {
                if let Some((_, _, pty_ready, child_ready)) =
                    ready.iter_mut().find(|(ready_id, _, _, _)| *ready_id == id)
                {
                    if pty {
                        *pty_ready = true;
                    } else {
                        *child_ready = true;
                    }
                } else {
                    ready.push((id, None, pty, !pty));
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn poll(
        &mut self,
        timeout: Duration,
        ready: &mut Vec<(usize, Option<Wake>, bool, bool)>,
    ) -> Result<(), WorkerError> {
        use rustix::event::kqueue::{Event, EventFilter, EventFlags, ProcessEvents};
        use std::os::fd::AsRawFd;

        let filter = |index, source| {
            if index == 0 {
                EventFilter::Read(source)
            } else {
                EventFilter::Proc {
                    pid: rustix::process::Pid::from_raw(source).expect("pane child pid"),
                    flags: ProcessEvents::EXIT,
                }
            }
        };
        let mut changes = SmallVec::<[Event; 9]>::new();
        if !self.wake_registered {
            changes.push(Event::new(
                EventFilter::Read(self.wake_rx.as_raw_fd()),
                EventFlags::ADD,
                std::ptr::null_mut(),
            ));
            self.wake_registered = true;
        }
        for (&id, entry) in &mut self.actors {
            let sources = if let Actor::Live(actor) = &entry.actor {
                let (pty, _) = actor.poll_sources();
                [
                    pty.map(AsRawFd::as_raw_fd),
                    actor
                        .shard_child_pid()
                        .map(|pid| pid.as_raw_nonzero().get()),
                ]
            } else {
                [None; 2]
            };
            for (index, source) in sources.into_iter().enumerate() {
                if source == entry.poll_sources[index] {
                    continue;
                }
                if let Some(fd) = entry.poll_sources[index] {
                    changes.push(Event::new(
                        filter(index, fd),
                        EventFlags::DELETE,
                        usize::MAX as *mut _,
                    ));
                }
                if let Some(fd) = source {
                    changes.push(Event::new(
                        filter(index, fd),
                        if index == 0 {
                            EventFlags::ADD
                        } else {
                            EventFlags::ADD | EventFlags::ONESHOT
                        },
                        ((id + 1) * 2 + index) as *mut _,
                    ));
                }
                entry.poll_sources[index] = source;
            }
        }
        self.events.clear();
        #[allow(
            unsafe_code,
            reason = "the shard owns registered descriptors until their actors close them"
        )]
        let polled = unsafe {
            rustix::event::kqueue::kevent(
                &self.kqueue,
                &changes,
                rustix::buffer::spare_capacity(&mut self.events),
                Some(timeout.min(IDLE_SLEEP)),
            )
        };
        match polled {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => return Ok(()),
            Err(error) => return Err(WorkerError::Io(error.into())),
        }
        for event in &self.events {
            let token = event.udata() as usize;
            if event.flags().contains(EventFlags::ERROR) {
                if token == usize::MAX
                    && (event.data() == i64::from(rustix::io::Errno::BADF.raw_os_error())
                        || event.data() == i64::from(rustix::io::Errno::NOENT.raw_os_error())
                        || event.data() == i64::from(rustix::io::Errno::SRCH.raw_os_error()))
                {
                    continue;
                }
                if token.is_multiple_of(2)
                    || event.data() != i64::from(rustix::io::Errno::SRCH.raw_os_error())
                {
                    return Err(WorkerError::Io(std::io::Error::from_raw_os_error(
                        event.data() as i32,
                    )));
                }
            }
            if token == 0 {
                drain_wake_pipe(&self.wake_rx)?;
                if let Some(pending) = &self.wake.pending {
                    pending.store(false, Ordering::Release);
                }
                continue;
            }
            let id = token / 2 - 1;
            let pty = token.is_multiple_of(2);
            if let Some((_, _, pty_ready, child_ready)) =
                ready.iter_mut().find(|(ready_id, _, _, _)| *ready_id == id)
            {
                if pty {
                    *pty_ready = true;
                } else {
                    *child_ready = true;
                }
            } else {
                ready.push((id, None, pty, !pty));
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn poll(&self, timeout: Duration, _: &mut Vec<(usize, Option<Wake>, bool, bool)>) {
        let _ = self.wake_rx.recv_timeout(timeout);
    }
}

#[cfg(all(test, unix))]
#[path = "shard_tests.rs"]
mod tests;
