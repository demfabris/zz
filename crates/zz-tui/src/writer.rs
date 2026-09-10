use std::{
    collections::VecDeque,
    io::{self, Write as _},
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread,
    time::Duration,
};

/// Bytes allowed to sit unwritten before the queue is thrown away.
///
/// The pin's client never writes to its tty from the loop that reads its
/// stdin: tty.c hands every byte to a libevent buffer, so a terminal that
/// stops reading costs the client memory, not liveness. The raw TUI matches
/// that by handing whole paints to a writer thread, and this budget is where
/// the memory stops growing.
///
/// Past it the queue is DROPPED, never waited on, because that is what the pin
/// does: `tty_block_maybe` drains its whole out buffer once it holds more than
/// `TTY_BLOCK_START`, and every later write is thrown away until the block
/// clears. A client whose viewer is not reading owes that viewer nothing but a
/// correct screen once reading resumes, and [`Submission::Dropped`] is how the
/// renderer is told to repaint from the model instead of from what it thinks
/// the terminal already has.
const QUEUE_BUDGET: usize = 4 * 1024 * 1024;

/// How long a block lasts before the stop rule is applied to it.
///
/// tty.c `TTY_BLOCK_INTERVAL`, unchanged: 100 ms.
const BLOCK_INTERVAL: Duration = Duration::from_millis(100);

/// How little has to be dropped in one interval for the block to clear.
///
/// tty.c sets `TTY_BLOCK_START` to 1 + cells * 8 and `TTY_BLOCK_STOP` to
/// 1 + cells / 8, a ratio of 64 between the threshold that starts a block and
/// the one that ends it. The raw TUI's threshold is a whole-paint queue rather
/// than a per-cell buffer, so it keeps the pin's ratio against its own budget
/// rather than the pin's absolute byte counts.
const BLOCK_STOP: usize = QUEUE_BUDGET / 64;

pub(crate) type Sink = Box<dyn FnMut(&[u8]) -> io::Result<()> + Send>;
pub(crate) type Wake = Box<dyn Fn() + Send + Sync>;

/// What became of a paint handed to [`TerminalWriter::submit`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Submission {
    /// The bytes are queued and the terminal will get them.
    Queued,
    /// The bytes were dropped, and so was anything else still queued. What the
    /// caller believes the terminal is showing is now wrong.
    Dropped,
}

/// A terminal that writes on its own thread.
///
/// [`submit`](TerminalWriter::submit) never waits: the bytes are either queued
/// or dropped, so a terminal that has stopped reading never stops the event
/// loop from reading keys and acting on them, however long it stops for. The
/// write error, when there is one, is reported on the next submission.
pub(crate) struct TerminalWriter {
    shared: Arc<Shared>,
    threaded: bool,
}

struct Shared {
    sink: Mutex<Sink>,
    state: Mutex<State>,
    work: Condvar,
    wake: Mutex<Option<Wake>>,
}

struct State {
    queue: VecDeque<Vec<u8>>,
    queued: usize,
    failure: Option<(io::ErrorKind, String)>,
    closed: bool,
    /// Set while paints are being thrown away instead of queued.
    blocked: bool,
    /// Bytes dropped since the current interval began.
    discarded: usize,
    /// True once a block has cleared and the screen has not been repainted for
    /// it yet.
    unblocked: bool,
}

pub(crate) fn stdout_sink() -> Sink {
    Box::new(|bytes| {
        let mut stdout = io::stdout().lock();
        stdout.write_all(bytes)?;
        stdout.flush()
    })
}

impl TerminalWriter {
    /// Starts the writer thread, falling back to writing from the caller when
    /// the thread cannot be started at all.
    pub fn spawn(sink: Sink) -> Self {
        let shared = Arc::new(Shared {
            sink: Mutex::new(sink),
            state: Mutex::new(State {
                queue: VecDeque::new(),
                queued: 0,
                failure: None,
                closed: false,
                blocked: false,
                discarded: 0,
                unblocked: false,
            }),
            work: Condvar::new(),
            wake: Mutex::new(None),
        });
        let worker = Arc::clone(&shared);
        let threaded = thread::Builder::new()
            .name("zz-tui-writer".to_owned())
            .spawn(move || worker.pump())
            .is_ok();
        Self { shared, threaded }
    }

    /// Installs the callback that asks for a repaint when a block clears.
    ///
    /// Without one the screen is still correct, because the paint that follows
    /// a drop repaints everything; with one it is correct at the moment the
    /// terminal starts reading again rather than at the next event, which is
    /// what `tty_timer_callback`'s `CLIENT_ALLREDRAWFLAGS` buys the pin.
    pub fn set_wake(&self, wake: Wake) {
        *self
            .shared
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(wake);
    }

    /// Queues a paint, or drops it, and reports whichever write failed since
    /// the last call.
    pub fn submit(&self, bytes: Vec<u8>) -> io::Result<Submission> {
        if !self.threaded {
            self.shared.write_now(&bytes)?;
            return Ok(Submission::Queued);
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((kind, message)) = state.failure.take() {
            return Err(io::Error::new(kind, message));
        }
        if state.blocked {
            state.discarded += bytes.len();
            return Ok(Submission::Dropped);
        }
        // A queue with nothing in it takes the paint whatever it weighs: there
        // is no smaller thing to fall back on, and dropping it would leave the
        // terminal with a screen nothing will ever replace.
        if state.queued > 0 && state.queued + bytes.len() > QUEUE_BUDGET {
            state.discarded = state.queued + bytes.len();
            state.queue.clear();
            state.queued = 0;
            state.blocked = true;
            drop(state);
            self.start_block_timer();
            return Ok(Submission::Dropped);
        }
        state.queued += bytes.len();
        state.queue.push_back(bytes);
        self.shared.work.notify_one();
        Ok(Submission::Queued)
    }

    /// Reports, once, that a block has cleared since the last call.
    pub fn take_unblocked(&self) -> bool {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut state.unblocked)
    }

    /// The pin's timer, on a thread of its own because the writer thread is
    /// inside a write to a terminal that is not reading and will be for as
    /// long as the block lasts.
    fn start_block_timer(&self) {
        let shared = Arc::clone(&self.shared);
        if thread::Builder::new()
            .name("zz-tui-writer-block".to_owned())
            .spawn(move || shared.run_block_timer())
            .is_err()
        {
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.blocked = false;
            state.discarded = 0;
            state.unblocked = true;
        }
    }

    /// Drops every paint still waiting so a teardown cannot repaint the
    /// screen after the terminal has been restored.
    ///
    /// A queued paint is stale the moment the event loop stops: the client is
    /// leaving the alternate screen, so the bytes would land on the user's
    /// shell instead of the pane they were painted for.
    pub fn abandon(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.queue.clear();
        state.queued = 0;
        state.closed = true;
        state.blocked = false;
        self.shared.work.notify_all();
    }

    #[cfg(test)]
    pub fn queued(&self) -> usize {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .queued
    }
}

impl Shared {
    fn pump(&self) {
        while let Some(chunk) = self.take() {
            if let Err(error) = self.write_now(&chunk) {
                self.record(&error);
            }
        }
    }

    fn write_now(&self, bytes: &[u8]) -> io::Result<()> {
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        sink(bytes)
    }

    fn take(&self) -> Option<Vec<u8>> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(chunk) = state.queue.pop_front() {
                state.queued -= chunk.len();
                return Some(chunk);
            }
            if state.closed {
                return None;
            }
            state = self
                .work
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// `tty_timer_callback`: every interval, clear the block if this interval
    /// dropped little enough, and otherwise start the count again.
    fn run_block_timer(&self) {
        loop {
            thread::sleep(BLOCK_INTERVAL);
            {
                let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                if state.closed || !state.blocked {
                    return;
                }
                if state.discarded >= BLOCK_STOP {
                    state.discarded = 0;
                    continue;
                }
                state.blocked = false;
                state.discarded = 0;
                state.unblocked = true;
            }
            if let Some(wake) = self
                .wake
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                wake();
            }
            return;
        }
    }

    fn record(&self, error: &io::Error) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.failure.is_none() {
            state.failure = Some((error.kind(), error.to_string()));
        }
    }
}

impl Drop for TerminalWriter {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        self.shared.work.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_terminal_that_never_reads_does_not_stall_the_paint_that_feeds_it() {
        let (release, blocked) = mpsc::channel::<()>();
        let (wrote, written) = mpsc::channel::<usize>();
        let writer = TerminalWriter::spawn(Box::new(move |bytes| {
            blocked.recv().ok();
            wrote.send(bytes.len()).ok();
            Ok(())
        }));

        for _ in 0..64 {
            writer.submit(vec![b'x'; 1024]).expect("queued");
        }
        assert!(written.try_recv().is_err());

        for _ in 0..64 {
            release.send(()).expect("release one write");
        }
        for _ in 0..64 {
            assert_eq!(
                written
                    .recv_timeout(Duration::from_secs(5))
                    .expect("write landed"),
                1024
            );
        }
    }

    #[test]
    fn a_write_error_is_reported_on_the_next_paint() {
        let (wrote, written) = mpsc::channel::<()>();
        let writer = TerminalWriter::spawn(Box::new(move |_| {
            wrote.send(()).ok();
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "terminal went away",
            ))
        }));

        writer.submit(vec![b'x'; 8]).expect("first paint queues");
        written
            .recv_timeout(Duration::from_secs(5))
            .expect("sink ran");
        let mut reported = None;
        for _ in 0..200 {
            match writer.submit(vec![b'x'; 8]) {
                Ok(_) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    reported = Some(error);
                    break;
                }
            }
        }
        let error = reported.expect("the failed write surfaces on a later paint");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(error.to_string().contains("terminal went away"));
    }

    #[test]
    fn an_abandoned_queue_never_reaches_the_terminal() {
        let (release, blocked) = mpsc::channel::<()>();
        let (wrote, written) = mpsc::channel::<usize>();
        let writer = TerminalWriter::spawn(Box::new(move |bytes| {
            wrote.send(bytes.len()).ok();
            blocked.recv().ok();
            Ok(())
        }));

        writer
            .submit(vec![b'x'; 1])
            .expect("the first paint queues");
        assert_eq!(
            written
                .recv_timeout(Duration::from_secs(5))
                .expect("the first paint reaches the sink"),
            1
        );
        for _ in 0..8 {
            writer
                .submit(vec![b'x'; 4096])
                .expect("the stale paints queue");
        }

        writer.abandon();
        release.send(()).expect("release the write in flight");

        assert!(
            written.recv_timeout(Duration::from_millis(500)).is_err(),
            "a paint queued before the teardown still reached the terminal"
        );
    }

    // The pin's shape: past the threshold the queue goes, the paint that hit
    // the threshold goes with it, and nothing ever waits. tty.c
    // tty_block_maybe drains its whole out buffer and returns; it does not
    // stop the client.
    #[test]
    fn past_the_budget_the_queue_is_dropped_and_no_paint_ever_waits() {
        let (release, blocked) = mpsc::channel::<()>();
        let writer = TerminalWriter::spawn(Box::new(move |_| {
            blocked.recv().ok();
            Ok(())
        }));

        let chunk = QUEUE_BUDGET / 8;
        let mut dropped = None;
        let started = std::time::Instant::now();
        for attempt in 0..64 {
            match writer.submit(vec![b'x'; chunk]).expect("submitted") {
                Submission::Queued => {}
                Submission::Dropped => {
                    dropped = Some(attempt);
                    break;
                }
            }
        }
        let elapsed = started.elapsed();
        let attempt = dropped.expect("the queue is dropped once it passes the budget");
        assert!(attempt <= 9, "dropped after {attempt} paints");
        assert!(
            elapsed < Duration::from_secs(5),
            "a paint waited for the terminal: {elapsed:?}"
        );
        assert!(writer.queued() <= chunk, "{}", writer.queued());

        // Everything after the drop is dropped too, without waiting, for as
        // long as the block lasts.
        for _ in 0..4 {
            assert_eq!(
                writer.submit(vec![b'x'; chunk]).expect("submitted"),
                Submission::Dropped
            );
        }
        assert_eq!(writer.queued(), 0);

        release.send(()).ok();
    }

    // tty_timer_callback: an interval with little enough dropped in it clears
    // the block, sets the redraw flags and calls tty_invalidate. Here that is
    // the wake callback plus the one-shot take_unblocked.
    #[test]
    fn a_cleared_block_asks_for_one_repaint() {
        let (release, blocked) = mpsc::channel::<()>();
        let writer = TerminalWriter::spawn(Box::new(move |_| {
            blocked.recv().ok();
            Ok(())
        }));
        let (woke, wakes) = mpsc::channel::<()>();
        writer.set_wake(Box::new(move || {
            woke.send(()).ok();
        }));

        let chunk = QUEUE_BUDGET / 8;
        let mut dropped = false;
        for _ in 0..64 {
            if writer.submit(vec![b'x'; chunk]).expect("submitted") == Submission::Dropped {
                dropped = true;
                break;
            }
        }
        assert!(dropped, "the queue never passed the budget");
        assert!(
            !writer.take_unblocked(),
            "the block cleared before its timer"
        );

        wakes
            .recv_timeout(Duration::from_secs(5))
            .expect("the block timer asked for a repaint");
        assert!(writer.take_unblocked(), "the repaint request is readable");
        assert!(!writer.take_unblocked(), "the repaint request is one-shot");
        assert_eq!(
            writer.submit(vec![b'x'; 8]).expect("submitted"),
            Submission::Queued,
            "a cleared block queues again"
        );

        release.send(()).ok();
    }
}
