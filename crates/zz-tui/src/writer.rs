use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

const QUEUE_BUDGET: usize = 4 * 1024 * 1024;
const BLOCK_INTERVAL: Duration = Duration::from_millis(100);
const BLOCK_STOP: usize = QUEUE_BUDGET / 64;
const WRITE_TURN_BYTES: usize = 256 * 1024;

#[cfg(test)]
pub(crate) type Sink = Box<dyn FnMut(&[u8]) -> io::Result<()> + Send>;
type PartialSink = Box<dyn FnMut(&[u8]) -> io::Result<usize>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Submission {
    Queued,
    Dropped,
}

pub(crate) struct TerminalWriter {
    queue: VecDeque<Vec<u8>>,
    offset: usize,
    recycled: Vec<u8>,
    queued: usize,
    sink: PartialSink,
    #[cfg(unix)]
    fd: Option<OwnedFd>,
    paused: bool,
    closed: bool,
    block_deadline: Option<Instant>,
    discarded: usize,
    unblocked: bool,
}

impl TerminalWriter {
    fn new(sink: PartialSink) -> Self {
        Self {
            queue: VecDeque::new(),
            offset: 0,
            recycled: Vec::new(),
            queued: 0,
            sink,
            #[cfg(unix)]
            fd: None,
            paused: false,
            closed: false,
            block_deadline: None,
            discarded: 0,
            unblocked: false,
        }
    }

    #[cfg(unix)]
    pub fn terminal() -> io::Result<Self> {
        let name = rustix::termios::ttyname(io::stdout(), Vec::new())?;
        let fd = rustix::fs::open(
            name.as_c_str(),
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::NOCTTY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?;
        let sink_fd = fd.try_clone()?;
        let mut writer = Self::new(Box::new(move |bytes| {
            rustix::io::write(&sink_fd, bytes).map_err(io::Error::from)
        }));
        writer.fd = Some(fd);
        Ok(writer)
    }

    #[cfg(test)]
    pub fn with_sink(mut sink: Sink) -> Self {
        Self::new(Box::new(move |bytes| {
            sink(bytes)?;
            Ok(bytes.len())
        }))
    }

    pub fn submit(&mut self, bytes: &mut Vec<u8>) -> io::Result<Submission> {
        if self.paused || self.closed {
            bytes.clear();
            return Ok(Submission::Dropped);
        }
        if self.block_deadline.is_some() {
            self.discarded = self.discarded.saturating_add(bytes.len());
            bytes.clear();
            return Ok(Submission::Dropped);
        }
        if self.queued > 0 && self.queued.saturating_add(bytes.len()) > QUEUE_BUDGET {
            self.discarded = self.queued.saturating_add(bytes.len());
            if self.offset == 0 {
                self.clear_queue();
            } else {
                self.queue.truncate(1);
                self.queued = self
                    .queue
                    .front()
                    .map_or(0, |chunk| chunk.len() - self.offset);
            }
            self.block_deadline = Some(Instant::now() + BLOCK_INTERVAL);
            bytes.clear();
            return Ok(Submission::Dropped);
        }
        self.enqueue(bytes);
        self.flush()?;
        Ok(Submission::Queued)
    }

    fn enqueue(&mut self, bytes: &mut Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        self.queued += bytes.len();
        self.queue
            .push_back(std::mem::replace(bytes, std::mem::take(&mut self.recycled)));
    }

    pub fn control(&mut self, mut bytes: Vec<u8>) -> io::Result<()> {
        self.enqueue(&mut bytes);
        self.flush()
    }

    pub fn flush(&mut self) -> io::Result<()> {
        let mut written = 0;
        while let Some(chunk) = self.queue.front() {
            let end = chunk.len().min(self.offset + WRITE_TURN_BYTES - written);
            match (self.sink)(&chunk[self.offset..end]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => {
                    self.offset += count;
                    self.queued -= count;
                    written += count;
                    if self.offset == chunk.len() {
                        let mut completed = self.queue.pop_front().expect("front exists");
                        completed.clear();
                        if completed.capacity() <= QUEUE_BUDGET
                            && completed.capacity() > self.recycled.capacity()
                        {
                            self.recycled = completed;
                        }
                        self.offset = 0;
                    }
                    if written >= WRITE_TURN_BYTES {
                        break;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.block_deadline
    }

    pub fn tick(&mut self, now: Instant) {
        if self.block_deadline.is_none_or(|deadline| deadline > now) {
            return;
        }
        if self.discarded >= BLOCK_STOP {
            self.discarded = 0;
            self.block_deadline = Some(now + BLOCK_INTERVAL);
        } else {
            self.block_deadline = None;
            self.discarded = 0;
            self.unblocked = true;
        }
    }

    pub fn take_unblocked(&mut self) -> bool {
        std::mem::take(&mut self.unblocked)
    }

    #[cfg(unix)]
    pub fn pending_fd(&self) -> Option<BorrowedFd<'_>> {
        (self.queued > 0)
            .then(|| self.fd.as_ref().map(AsFd::as_fd))
            .flatten()
    }

    fn clear_queue(&mut self) {
        self.queue.clear();
        self.offset = 0;
        self.queued = 0;
    }

    pub fn abandon(&mut self) {
        self.clear_queue();
        self.closed = true;
        self.block_deadline = None;
    }

    pub fn pause(&mut self, paused: bool) {
        self.paused = paused;
        if paused {
            self.clear_queue();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[test]
    fn partial_writes_resume_without_repeating_bytes() {
        let written = Rc::new(RefCell::new(Vec::new()));
        let ready = Rc::new(RefCell::new(2));
        let mut writer = TerminalWriter::new(Box::new({
            let written = Rc::clone(&written);
            let ready = Rc::clone(&ready);
            move |bytes| {
                let count = bytes.len().min(*ready.borrow());
                if count == 0 {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                written.borrow_mut().extend_from_slice(&bytes[..count]);
                *ready.borrow_mut() -= count;
                Ok(count)
            }
        }));
        writer.submit(&mut b"abcdef".to_vec()).unwrap();
        assert_eq!(&*written.borrow(), b"ab");
        assert_eq!(writer.queued, 4);
        *ready.borrow_mut() = 8;
        writer.flush().unwrap();
        assert_eq!(&*written.borrow(), b"abcdef");
        assert_eq!(writer.queued, 0);
    }

    #[test]
    fn overflow_keeps_the_partially_written_chunk_and_recovers_after_quiet() {
        let ready = Rc::new(RefCell::new(true));
        let written = Rc::new(RefCell::new(Vec::new()));
        let mut writer = TerminalWriter::new(Box::new({
            let ready = Rc::clone(&ready);
            let written = Rc::clone(&written);
            move |bytes| {
                if !*ready.borrow() {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                *ready.borrow_mut() = false;
                written.borrow_mut().extend_from_slice(&bytes[..1]);
                Ok(1)
            }
        }));
        writer.submit(&mut b"abcd".to_vec()).unwrap();
        assert_eq!(
            writer.submit(&mut vec![b'x'; QUEUE_BUDGET]).unwrap(),
            Submission::Dropped
        );
        assert_eq!(writer.queued, 3);
        let deadline = writer.deadline().unwrap();
        writer.tick(deadline);
        assert!(!writer.take_unblocked());
        writer.tick(deadline + BLOCK_INTERVAL);
        assert!(writer.take_unblocked());
        assert!(!writer.take_unblocked());
        for _ in 0..3 {
            *ready.borrow_mut() = true;
            writer.flush().unwrap();
        }
        assert_eq!(&*written.borrow(), b"abcd");
        assert_eq!(writer.queued, 0);
    }

    #[test]
    fn blocked_output_and_abandonment_never_wait_for_a_reader() {
        let mut writer = TerminalWriter::new(Box::new(|_| Err(io::ErrorKind::WouldBlock.into())));
        writer.submit(&mut vec![b'x'; QUEUE_BUDGET]).unwrap();
        assert_eq!(
            writer.submit(&mut vec![b'y'; 1]).unwrap(),
            Submission::Dropped
        );
        assert_eq!(writer.queued, 0);
        assert_eq!(
            writer.submit(&mut b"stale".to_vec()).unwrap(),
            Submission::Dropped
        );
        writer.abandon();
        assert_eq!(
            writer.submit(&mut b"late".to_vec()).unwrap(),
            Submission::Dropped
        );
    }

    #[test]
    fn suspension_drops_paints_but_preserves_terminal_controls() {
        let written = Rc::new(RefCell::new(Vec::new()));
        let mut writer = TerminalWriter::new(Box::new({
            let written = Rc::clone(&written);
            move |bytes| {
                written.borrow_mut().extend_from_slice(bytes);
                Ok(bytes.len())
            }
        }));
        writer.pause(true);
        assert_eq!(
            writer.submit(&mut b"stale".to_vec()).unwrap(),
            Submission::Dropped
        );
        writer.control(b"restore".to_vec()).unwrap();
        writer.pause(false);
        writer.submit(&mut b"fresh".to_vec()).unwrap();
        assert_eq!(&*written.borrow(), b"restorefresh");
    }

    #[test]
    fn a_write_error_surfaces_in_the_same_turn() {
        let mut writer = TerminalWriter::new(Box::new(|_| Err(io::ErrorKind::BrokenPipe.into())));
        assert_eq!(
            writer.submit(&mut vec![b'x']).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn each_flush_yields_after_its_byte_budget() {
        let mut writer = TerminalWriter::new(Box::new(|bytes| Ok(bytes.len())));
        writer
            .submit(&mut vec![b'x'; WRITE_TURN_BYTES * 2])
            .unwrap();
        assert_eq!(writer.queued, WRITE_TURN_BYTES);
        writer.flush().unwrap();
        assert_eq!(writer.queued, 0);
    }
}
