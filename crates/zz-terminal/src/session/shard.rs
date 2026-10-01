use super::*;
use std::{cmp::Reverse, collections::BinaryHeap};

static SHARDS: LazyLock<Result<Vec<ShardHandle>, String>> = LazyLock::new(|| {
    let count = shard_count(std::env::var("ZZ_PTY_SHARDS").ok().as_deref());
    (0..count)
        .map(|index| ShardHandle::start(index).map_err(|error| error.to_string()))
        .collect()
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
    let shards = SHARDS
        .as_ref()
        .map_err(|error| WorkerError::Thread(error.clone()))?;
    if shards.is_empty() {
        return Ok(None);
    }
    let index = NEXT_SHARD.fetch_add(1, Ordering::Relaxed) % shards.len();
    Ok(Some(shards[index].clone()))
}

#[derive(Clone)]
pub(super) struct ShardHandle {
    requests: Sender<PaneLaunch>,
    pub(super) wake: ActorWake,
}

pub(super) struct PaneLaunch {
    pub(super) control_rx: Receiver<Command>,
    pub(super) input_rx: InputReceiver,
    pub(super) slot: Arc<Mutex<ControlSlot>>,
    pub(super) publisher: Publisher,
    pub(super) max_scrollback: usize,
    pub(super) appearance: Arc<TerminalAppearance>,
    pub(super) spawn: TerminalSpawn,
    pub(super) alive: Sender<Infallible>,
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
                    pipe: Some(Arc::new(write)),
                },
            )
        };
        #[cfg(not(unix))]
        let (wake_rx, wake) = {
            let (send, receive) = crossbeam_channel::bounded(1);
            (
                receive,
                ActorWake {
                    channel: Some(send),
                },
            )
        };
        let worker_wake = wake.clone();
        thread::Builder::new()
            .name(format!("zz-pty-shard-{index}"))
            .spawn(move || {
                let mut shard = Shard {
                    incoming,
                    wake: worker_wake,
                    wake_rx,
                    actors: HashMap::new(),
                    deadlines: BinaryHeap::new(),
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
                if !actor.on_wake(Wake::Deadline)? {
                    return Ok(None);
                }
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
    generation: u64,
}

struct Shard {
    incoming: Receiver<PaneLaunch>,
    wake: ActorWake,
    #[cfg(unix)]
    wake_rx: std::os::fd::OwnedFd,
    #[cfg(not(unix))]
    wake_rx: Receiver<()>,
    actors: HashMap<usize, Entry>,
    deadlines: BinaryHeap<Reverse<(Instant, usize, u64)>>,
    next_id: usize,
    cursor: usize,
}

impl Shard {
    fn run(&mut self) -> Result<(), WorkerError> {
        loop {
            while let Ok(launch) = self.incoming.try_recv() {
                let publisher = launch.publisher.clone();
                #[cfg(all(unix, not(target_os = "linux")))]
                let wake_rx = None;
                #[cfg(any(target_os = "linux", not(unix)))]
                let wake_rx = ();
                match PaneActor::spawn(
                    launch.control_rx,
                    launch.input_rx,
                    launch.slot,
                    launch.publisher,
                    launch.max_scrollback,
                    &launch.appearance,
                    &launch.spawn,
                    &self.wake,
                    wake_rx,
                    true,
                ) {
                    Ok(actor) => {
                        let id = self.next_id;
                        self.next_id += 1;
                        self.actors.insert(
                            id,
                            Entry {
                                actor: Actor::Live(Box::new(actor)),
                                publisher,
                                _alive: launch.alive,
                                generation: 0,
                            },
                        );
                        self.dispatch(id, None);
                    }
                    Err(error) => report_worker_error(&publisher, &error),
                }
            }
            self.expire_deadlines();
            let mut ready = Vec::new();
            let mut failed = Vec::new();
            for (&id, entry) in &mut self.actors {
                match entry.actor.try_wake() {
                    Ok(Some(wake)) => ready.push((id, Some(wake), false, false)),
                    Ok(None) => {}
                    Err(error) => failed.push((id, error)),
                }
            }
            for (id, error) in failed {
                self.fail(id, &error);
            }
            let timeout = if ready.is_empty() {
                self.poll_timeout()
            } else {
                Duration::ZERO
            };
            #[cfg(unix)]
            self.poll(timeout, &mut ready)?;
            #[cfg(not(unix))]
            self.poll(timeout, &mut ready);
            ready.sort_unstable_by_key(|(id, _, _, _)| (*id < self.cursor, *id));
            let only_ready = ready
                .iter()
                .map(|(id, _, _, _)| *id)
                .collect::<HashSet<_>>()
                .len()
                == 1;
            for (id, wake, pty, child) in ready {
                if let Some(mut entry) = self.actors.remove(&id) {
                    let result = (|| {
                        #[cfg(unix)]
                        if let Actor::Live(actor) = &mut entry.actor {
                            if child {
                                actor.poll_child()?;
                            }
                            #[cfg(not(target_os = "linux"))]
                            if pty {
                                actor.on_pty_ready(only_ready)?;
                            }
                        }
                        #[cfg(any(target_os = "linux", not(unix)))]
                        let _ = (pty, only_ready);
                        #[cfg(not(unix))]
                        let _ = child;
                        let actor = if let Some(wake) = wake {
                            entry.actor.on_wake(wake)?
                        } else {
                            Some(entry.actor)
                        };
                        if let Some(actor) = actor {
                            actor.on_deadline()
                        } else {
                            Ok(None)
                        }
                    })();
                    self.store_result(id, entry.publisher, entry._alive, entry.generation, result);
                    self.cursor = id + 1;
                }
            }
        }
    }

    fn dispatch(&mut self, id: usize, wake: Option<Wake>) {
        if let Some(entry) = self.actors.remove(&id) {
            let result = if let Some(wake) = wake {
                entry.actor.on_wake(wake)
            } else {
                entry.actor.on_deadline()
            };
            self.store_result(id, entry.publisher, entry._alive, entry.generation, result);
        }
    }

    fn store_result(
        &mut self,
        id: usize,
        publisher: Publisher,
        alive: Sender<Infallible>,
        generation: u64,
        result: Result<Option<Actor>, WorkerError>,
    ) {
        match result {
            Ok(Some(actor)) => {
                let generation = generation.wrapping_add(1);
                self.deadlines
                    .push(Reverse((actor.next_deadline(), id, generation)));
                self.actors.insert(
                    id,
                    Entry {
                        actor,
                        publisher,
                        _alive: alive,
                        generation,
                    },
                );
            }
            Ok(None) => publisher.set_foreground_source(None),
            Err(error) => {
                report_worker_error(&publisher, &error);
                publisher.set_foreground_source(None);
            }
        }
        if self.deadlines.len() > self.actors.len() * 4 + 64 {
            self.deadlines = self
                .actors
                .iter()
                .map(|(&id, entry)| Reverse((entry.actor.next_deadline(), id, entry.generation)))
                .collect();
        }
    }

    fn fail(&mut self, id: usize, error: &WorkerError) {
        if let Some(entry) = self.actors.remove(&id) {
            report_worker_error(&entry.publisher, error);
            entry.publisher.set_foreground_source(None);
        }
    }

    fn discard_stale_deadlines(&mut self) {
        while let Some(Reverse((_, id, generation))) = self.deadlines.peek() {
            if self
                .actors
                .get(id)
                .is_some_and(|entry| entry.generation == *generation)
            {
                break;
            }
            self.deadlines.pop();
        }
    }

    fn expire_deadlines(&mut self) {
        let now = Instant::now();
        for _ in 0..self.actors.len() {
            self.discard_stale_deadlines();
            let Some(Reverse((due, id, _))) = self.deadlines.peek().copied() else {
                return;
            };
            if due > now {
                return;
            }
            self.deadlines.pop();
            self.dispatch(id, None);
        }
    }

    fn poll_timeout(&mut self) -> Duration {
        self.discard_stale_deadlines();
        self.deadlines
            .peek()
            .map_or(IDLE_SLEEP, |Reverse((due, _, _))| {
                due.saturating_duration_since(Instant::now())
            })
    }

    #[cfg(unix)]
    fn poll(
        &self,
        timeout: Duration,
        ready: &mut Vec<(usize, Option<Wake>, bool, bool)>,
    ) -> Result<(), WorkerError> {
        use rustix::event::{PollFd, PollFlags};
        let mut fds = vec![PollFd::new(&self.wake_rx, PollFlags::IN)];
        let mut sources = Vec::new();
        for (&id, entry) in &self.actors {
            if let Actor::Live(actor) = &entry.actor {
                #[cfg(target_os = "linux")]
                if let Some(fd) = actor.child_poll_fd() {
                    fds.push(PollFd::new(fd, PollFlags::IN));
                    sources.push((id, false));
                }
                #[cfg(not(target_os = "linux"))]
                {
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

    #[cfg(not(unix))]
    fn poll(&self, timeout: Duration, _: &mut Vec<(usize, Option<Wake>, bool, bool)>) {
        let _ = self.wake_rx.recv_timeout(timeout);
    }
}

#[cfg(all(test, unix))]
#[path = "shard_tests.rs"]
mod tests;
