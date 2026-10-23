use ndk_sys::{
    ALOOPER_EVENT_INPUT, ALOOPER_POLL_CALLBACK, ALooper_acquire, ALooper_addFd, ALooper_forThread,
};
use std::{
    cell::RefCell,
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, VecDeque},
    ffi::{c_int, c_void},
    sync::mpsc,
    thread::{self, ThreadId},
    time::{Duration, Instant},
};
use zz_gpui::{
    PlatformDispatcher, Priority, PriorityQueueReceiver, PriorityQueueSender, RunnableVariant,
    profiler,
};

const MIN_THREADS: usize = 2;

thread_local! {
    static DEFERRED: RefCell<VecDeque<Box<dyn FnOnce()>>> = const { RefCell::new(VecDeque::new()) };
    static WAKE_FD: std::cell::Cell<c_int> = const { std::cell::Cell::new(-1) };
}

pub(crate) struct AndroidDispatcher {
    main_sender: PriorityQueueSender<RunnableVariant>,
    wake_fd: c_int,
    background_sender: PriorityQueueSender<RunnableVariant>,
    timer_sender: mpsc::Sender<(Instant, RunnableVariant)>,
    main_thread: ThreadId,
    _threads: Vec<thread::JoinHandle<()>>,
}

fn run(runnable: RunnableVariant) {
    let location = runnable.metadata().location;
    let spawned = runnable.metadata().spawned;
    profiler::update_running_task(spawned, location);
    runnable.run();
    profiler::save_task_timing();
}

fn wake(fd: c_int) {
    let one: u64 = 1;
    unsafe {
        libc::write(fd, (&one as *const u64).cast(), std::mem::size_of::<u64>());
    }
}

/// Runs `task` on the main thread on a later turn of its looper, outside any
/// GPUI update that may be in progress.
pub(crate) fn defer(task: impl FnOnce() + 'static) {
    DEFERRED.with_borrow_mut(|tasks| tasks.push_back(Box::new(task)));
    let fd = WAKE_FD.get();
    if fd >= 0 {
        wake(fd);
    }
}

struct MainQueue {
    receiver: PriorityQueueReceiver<RunnableVariant>,
}

unsafe extern "C" fn drain(fd: c_int, _events: c_int, data: *mut c_void) -> c_int {
    let mut counter = 0u64;
    libc::read(
        fd,
        (&mut counter as *mut u64).cast(),
        std::mem::size_of::<u64>(),
    );
    let queue = &*(data as *const MainQueue);
    for runnable in queue.receiver.clone().try_iter() {
        match runnable {
            Ok(runnable) => run(runnable),
            Err(_) => break,
        }
    }
    while let Some(task) = DEFERRED.with_borrow_mut(VecDeque::pop_front) {
        task();
    }
    1
}

impl AndroidDispatcher {
    /// Must be created on the main thread, whose looper then runs main-thread work.
    pub fn new() -> Self {
        let (main_sender, main_receiver) = PriorityQueueReceiver::new();
        let wake_fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        assert!(wake_fd >= 0, "eventfd failed");
        WAKE_FD.set(wake_fd);
        unsafe {
            let looper = ALooper_forThread();
            assert!(!looper.is_null(), "the main thread has no looper");
            ALooper_acquire(looper);
            let queue = Box::into_raw(Box::new(MainQueue {
                receiver: main_receiver,
            }));
            ALooper_addFd(
                looper,
                wake_fd,
                ALOOPER_POLL_CALLBACK,
                ALOOPER_EVENT_INPUT as c_int,
                Some(drain),
                queue.cast(),
            );
        }

        let (background_sender, background_receiver) = PriorityQueueReceiver::new();
        let count = thread::available_parallelism()
            .map_or(MIN_THREADS, |count| count.get().max(MIN_THREADS));
        let mut threads = (0..count)
            .map(|index| {
                let receiver: PriorityQueueReceiver<RunnableVariant> = background_receiver.clone();
                thread::Builder::new()
                    .name(format!("Worker-{index}"))
                    .spawn(move || {
                        for runnable in receiver.iter() {
                            run(runnable);
                        }
                    })
                    .expect("spawn worker thread")
            })
            .collect::<Vec<_>>();
        let (timer_sender, timer_receiver) = mpsc::channel();
        threads.push(
            thread::Builder::new()
                .name("Timer".into())
                .spawn(move || timers(timer_receiver))
                .expect("spawn timer thread"),
        );

        Self {
            main_sender,
            wake_fd,
            background_sender,
            timer_sender,
            main_thread: thread::current().id(),
            _threads: threads,
        }
    }
}

fn timers(receiver: mpsc::Receiver<(Instant, RunnableVariant)>) {
    let mut due: BinaryHeap<Reverse<(Instant, u64)>> = BinaryHeap::new();
    let mut pending = HashMap::new();
    let mut next_id = 0u64;
    loop {
        let now = Instant::now();
        while let Some(Reverse((at, id))) = due.peek().copied() {
            if at > now {
                break;
            }
            due.pop();
            if let Some(runnable) = pending.remove(&id) {
                run(runnable);
            }
        }
        let message = match due.peek() {
            Some(Reverse((at, _))) => {
                match receiver.recv_timeout(at.saturating_duration_since(Instant::now())) {
                    Ok(message) => Some(message),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            None => match receiver.recv() {
                Ok(message) => Some(message),
                Err(_) => return,
            },
        };
        if let Some((at, runnable)) = message {
            next_id += 1;
            pending.insert(next_id, runnable);
            due.push(Reverse((at, next_id)));
        }
    }
}

impl PlatformDispatcher for AndroidDispatcher {
    fn is_main_thread(&self) -> bool {
        thread::current().id() == self.main_thread
    }

    fn dispatch(&self, runnable: RunnableVariant, priority: Priority) {
        self.background_sender
            .send(priority, runnable)
            .unwrap_or_else(|_| panic!("background queue closed"));
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, priority: Priority) {
        match self.main_sender.send(priority, runnable) {
            Ok(()) => wake(self.wake_fd),
            Err(error) => std::mem::forget(error),
        }
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        if let Err(error) = self
            .timer_sender
            .send((Instant::now() + duration, runnable))
        {
            std::mem::forget(error);
        }
    }

    fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
        thread::spawn(f);
    }
}
