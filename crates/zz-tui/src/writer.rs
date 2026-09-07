use std::{
    collections::VecDeque,
    io::{self, Write as _},
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread,
};

/// Bytes allowed to sit unwritten before a paint waits for the terminal.
///
/// The pin's client never writes to its tty from the loop that reads its
/// stdin: tty.c hands every byte to a libevent buffer, so a terminal that
/// stops reading costs the client memory, not liveness. The raw TUI matches
/// that by handing whole paints to a writer thread, and this budget is where
/// the memory stops growing: a raw TUI client filling its pty at the 150 KB a
/// second measured by compat/scenarios/smoke/tui-client-input-backpressure.txt
/// parks its paint loop after half a minute of a terminal that reads nothing,
/// rather than buying more heap for a viewer who is not looking.
const QUEUE_BUDGET: usize = 4 * 1024 * 1024;

pub(crate) type Sink = Box<dyn FnMut(&[u8]) -> io::Result<()> + Send>;

/// A terminal that writes on its own thread.
///
/// [`submit`](TerminalWriter::submit) returns as soon as the bytes are queued,
/// so a blocked terminal never stops the event loop from reading keys and
/// acting on them. The write error, when there is one, is reported on the next
/// submission.
pub(crate) struct TerminalWriter {
    shared: Arc<Shared>,
    threaded: bool,
}

struct Shared {
    sink: Mutex<Sink>,
    state: Mutex<State>,
    room: Condvar,
    work: Condvar,
}

struct State {
    queue: VecDeque<Vec<u8>>,
    queued: usize,
    failure: Option<(io::ErrorKind, String)>,
    closed: bool,
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
            }),
            room: Condvar::new(),
            work: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        let threaded = thread::Builder::new()
            .name("zz-tui-writer".to_owned())
            .spawn(move || worker.pump())
            .is_ok();
        Self { shared, threaded }
    }

    /// Queues a paint and reports whichever write failed since the last call.
    pub fn submit(&self, bytes: Vec<u8>) -> io::Result<()> {
        if !self.threaded {
            return self.shared.write_now(&bytes);
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while state.queued >= QUEUE_BUDGET && state.failure.is_none() {
            state = self
                .shared
                .room
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if let Some((kind, message)) = state.failure.take() {
            return Err(io::Error::new(kind, message));
        }
        state.queued += bytes.len();
        state.queue.push_back(bytes);
        self.shared.work.notify_one();
        Ok(())
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
                self.room.notify_all();
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

    fn record(&self, error: &io::Error) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.failure.is_none() {
            state.failure = Some((error.kind(), error.to_string()));
        }
        self.room.notify_all();
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
                Ok(()) => std::thread::sleep(Duration::from_millis(10)),
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
    fn the_queue_stops_growing_at_the_budget() {
        let (release, blocked) = mpsc::channel::<()>();
        let writer = Arc::new(TerminalWriter::spawn(Box::new(move |_| {
            blocked.recv().ok();
            Ok(())
        })));

        let chunk = QUEUE_BUDGET / 8;
        let producer = {
            let writer = Arc::clone(&writer);
            thread::spawn(move || {
                for _ in 0..16 {
                    if writer.submit(vec![b'x'; chunk]).is_err() {
                        return;
                    }
                }
            })
        };

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while writer.queued() < QUEUE_BUDGET && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(writer.queued() >= QUEUE_BUDGET, "{}", writer.queued());
        assert!(
            writer.queued() < QUEUE_BUDGET + chunk,
            "{}",
            writer.queued()
        );
        assert!(!producer.is_finished(), "the paint past the budget waits");

        for _ in 0..16 {
            release.send(()).ok();
        }
        producer.join().expect("producer thread");
    }
}
