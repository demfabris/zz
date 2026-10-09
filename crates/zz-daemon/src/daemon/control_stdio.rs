use std::io::Write as _;
use std::mem::MaybeUninit;

use rustix::fs::OFlags;
use zz_protocol::encode_protocol_message;

use super::*;

#[cfg(test)]
#[path = "control_stdio_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "control_stdio_burst_tests.rs"]
mod burst_tests;

#[cfg(test)]
#[path = "control_stdio_bytes2_tests.rs"]
mod bytes2_tests;

const STDOUT_HIGH: usize = 256 * 1024;
const INPUT_READ_LIMIT: usize = 256 * 1024;
const INPUT_CHUNK: usize = 8192;
const DEFERRED_OUTPUT: usize = 1024;

pub(super) fn receive(
    stream: &UnixStream,
    buffer: &mut [u8],
    fds: &mut Vec<OwnedFd>,
) -> io::Result<usize> {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut ancillary = rustix::net::RecvAncillaryBuffer::new(&mut space);
    let received = rustix::net::recvmsg(
        stream,
        &mut [io::IoSliceMut::new(buffer)],
        &mut ancillary,
        rustix::net::RecvFlags::empty(),
    )?;
    for message in ancillary.drain() {
        if let rustix::net::RecvAncillaryMessage::ScmRights(received) = message {
            for fd in received {
                let _ = rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC);
                fds.push(fd);
            }
        }
    }
    Ok(received.bytes)
}

pub(super) struct ControlStdio {
    registry: mio::Registry,
    stdin: OwnedFd,
    stdout: OwnedFd,
    pub(super) stdin_token: Token,
    pub(super) stdout_token: Token,
    flags: [OFlags; 2],
    reading: bool,
    stdout_registered: bool,
    input: Vec<u8>,
    consumed: usize,
    drained: bool,
    input_closed: bool,
    input_error: Option<String>,
    output: Vec<u8>,
    output_offset: usize,
    batch: Vec<OutboundFrame>,
    direct: bool,
    forwarded: u64,
    next_number: u64,
    unit: Option<Unit>,
    close_requested: bool,
    pub(super) released: bool,
}

struct Unit {
    line: String,
    frames: Vec<OutboundFrame>,
    begin: Option<(u64, u32)>,
    body: Option<Vec<u8>>,
    complete: bool,
}

enum NextInput {
    Wait,
    Line(String),
    Forward,
}

fn encode(message: &ProtocolMessage) -> OutboundFrame {
    OutboundFrame::Owned(encode_protocol_message(message).expect("control stdio frames encode"))
}

fn messages(frame: &OutboundFrame) -> Option<Vec<ProtocolMessage>> {
    if let OutboundFrame::Grouped { frames, .. } = frame {
        return frames
            .iter()
            .map(|frame| zz_protocol::decode_protocol_frame(frame).ok())
            .collect();
    }
    match zz_protocol::decode_protocol_frame(frame).ok()? {
        ProtocolMessage::Batch(batch) => batch.messages().ok(),
        message => Some(vec![message]),
    }
}

const fn escapes(byte: u8) -> bool {
    byte < 0x20 || byte == b'\\'
}

fn plain_run(bytes: &[u8]) -> usize {
    let mut run = 0;
    for block in bytes.chunks_exact(16) {
        if block
            .iter()
            .fold(false, |found, byte| found | escapes(*byte))
        {
            break;
        }
        run += 16;
    }
    run + bytes[run..]
        .iter()
        .position(|byte| escapes(*byte))
        .unwrap_or(bytes.len() - run)
}

fn append_output_bytes(line: &mut Vec<u8>, bytes: &[u8]) {
    line.reserve(bytes.len());
    let mut rest = bytes;
    loop {
        let run = plain_run(rest);
        line.extend_from_slice(&rest[..run]);
        let Some(&byte) = rest.get(run) else {
            return;
        };
        line.extend_from_slice(&[
            b'\\',
            b'0' + (byte >> 6),
            b'0' + ((byte >> 3) & 7),
            b'0' + (byte & 7),
        ]);
        rest = &rest[run + 1..];
    }
}

fn render_output(output: &mut Vec<u8>, message: &ProtocolMessage) -> bool {
    match message {
        ProtocolMessage::Event(Event {
            payload: EventPayload::PaneOutput { pane, bytes },
            ..
        }) => {
            let _ = write!(output, "%output {pane} ");
            append_output_bytes(output, bytes);
            output.push(b'\n');
            true
        }
        ProtocolMessage::Event(Event {
            payload:
                EventPayload::PaneOutputAged {
                    pane,
                    age_ms,
                    bytes,
                },
            ..
        }) => {
            let _ = write!(output, "%extended-output {pane} {age_ms} : ");
            append_output_bytes(output, bytes);
            output.push(b'\n');
            true
        }
        _ => false,
    }
}

impl Unit {
    fn absorb(&mut self, messages: Vec<ProtocolMessage>) -> bool {
        for message in messages {
            match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::ControlCommandStarted {
                            request_id: 1,
                            flags,
                            canonical_name,
                            guard: true,
                        },
                    ..
                }) if self.begin.is_none()
                    && u8::try_from(flags).is_ok()
                    && !matches!(
                        canonical_name.as_deref(),
                        Some("source-file" | "detach-client")
                    ) =>
                {
                    self.begin = Some((unix_timestamp(), flags));
                }
                ProtocolMessage::CommandResponse(CommandResponse::Success {
                    request_id: 1,
                    output,
                    ..
                }) if self.begin.is_some() && self.body.is_none() => {
                    self.body = Some(output.into_bytes());
                }
                ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                    outcome: zz_protocol::ExecOutcome::Ran,
                    ..
                }) if self.body.is_some() && !self.complete => self.complete = true,
                _ => return false,
            }
        }
        true
    }
}

impl ControlStdio {
    pub(super) fn open(
        fds: Vec<OwnedFd>,
        registry: &mio::Registry,
        next_token: &mut usize,
    ) -> io::Result<Self> {
        let Ok([stdin, stdout]) = <[OwnedFd; 2]>::try_from(fds) else {
            return Err(io::Error::from(ErrorKind::InvalidInput));
        };
        let flags = [
            rustix::fs::fcntl_getfl(&stdin)?,
            rustix::fs::fcntl_getfl(&stdout)?,
        ];
        let stdin_token = jobs::allocate_token(next_token)?;
        let stdout_token = jobs::allocate_token(next_token)?;
        rustix::fs::fcntl_setfl(&stdin, flags[0] | OFlags::NONBLOCK)?;
        let registered = rustix::fs::fcntl_setfl(&stdout, flags[1] | OFlags::NONBLOCK)
            .map_err(io::Error::from)
            .and_then(|()| {
                registry.register(
                    &mut SourceFd(&stdin.as_raw_fd()),
                    stdin_token,
                    Interest::READABLE,
                )
            });
        if let Err(error) = registered {
            let _ = rustix::fs::fcntl_setfl(&stdin, flags[0]);
            let _ = rustix::fs::fcntl_setfl(&stdout, flags[1]);
            return Err(error);
        }
        Ok(Self {
            registry: registry.try_clone()?,
            stdin,
            stdout,
            stdin_token,
            stdout_token,
            flags,
            reading: true,
            stdout_registered: false,
            input: Vec::new(),
            consumed: 0,
            drained: true,
            input_closed: false,
            input_error: None,
            output: Vec::new(),
            output_offset: 0,
            batch: Vec::new(),
            direct: false,
            forwarded: 0,
            next_number: 1,
            unit: None,
            close_requested: false,
            released: false,
        })
    }

    pub(super) fn start_marker() -> OutboundFrame {
        encode(&ProtocolMessage::ControlStdioSync { next_number: 0 })
    }

    pub(super) fn refusal() -> OutboundFrame {
        encode(&ProtocolMessage::ControlStdioClosed)
    }

    fn forward(&mut self, frame: OutboundFrame, frames: &mut Vec<OutboundFrame>) {
        self.forwarded += 1;
        frames.push(frame);
    }

    fn compact_input(&mut self) {
        self.input.drain(..std::mem::take(&mut self.consumed));
    }

    fn forward_input(&mut self, frames: &mut Vec<OutboundFrame>) {
        self.compact_input();
        if self.input.is_empty() && !self.input_closed && self.input_error.is_none() {
            return;
        }
        let message = ProtocolMessage::ControlStdin {
            bytes: std::mem::take(&mut self.input),
            submitted: false,
            closed: std::mem::take(&mut self.input_closed),
            error: self.input_error.take(),
        };
        self.forward(encode(&message), frames);
    }

    fn enter_forward(&mut self, frames: &mut Vec<OutboundFrame>) {
        if self.direct {
            let _ = self.flush();
            self.direct = false;
            let sync = encode(&ProtocolMessage::ControlStdioSync {
                next_number: self.next_number,
            });
            self.forward(sync, frames);
        }
    }

    pub(super) fn read_input(&mut self) -> bool {
        self.read_chunks(false)
    }

    fn read_chunks(&mut self, until_short: bool) -> bool {
        let mut read = 0;
        let mut arrived = false;
        self.compact_input();
        while self.reading && read < INPUT_READ_LIMIT {
            self.input.reserve(INPUT_CHUNK);
            let room = self.input.capacity() - self.input.len();
            match rustix::io::read(&self.stdin, rustix::buffer::spare_capacity(&mut self.input)) {
                Ok(0) => {
                    self.input_closed = true;
                    self.stop_reading();
                    arrived = true;
                }
                Ok(count) => {
                    read += count;
                    arrived = true;
                    if until_short && count < room && self.input.last() == Some(&b'\n') {
                        self.drained = false;
                        return true;
                    }
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => break,
                Err(error) => {
                    self.input_error = Some(io::Error::from(error).to_string());
                    self.stop_reading();
                    arrived = true;
                }
            }
        }
        self.drained = !self.reading || read < INPUT_READ_LIMIT;
        arrived
    }

    fn wants_input(&self) -> bool {
        !self.drained && !self.input[self.consumed..].contains(&b'\n')
    }

    fn drain_input(&mut self, frames: &mut Vec<OutboundFrame>) -> bool {
        if !self.wants_input() {
            return false;
        }
        let arrived = self.read_input();
        self.input_ready(frames);
        arrived
    }

    fn stop_reading(&mut self) {
        if self.reading {
            self.reading = false;
            let _ = self
                .registry
                .deregister(&mut SourceFd(&self.stdin.as_raw_fd()));
        }
    }

    pub(super) fn holds_connection(&self) -> bool {
        !self.released
    }

    pub(super) fn input_ready(&mut self, frames: &mut Vec<OutboundFrame>) {
        if !self.direct && !self.released {
            self.forward_input(frames);
        }
    }

    pub(super) fn wants_line(&self) -> bool {
        self.direct
            && !self.released
            && !self.close_requested
            && self.unit.is_none()
            && self.output.len() < STDOUT_HIGH
            && (self.consumed < self.input.len() || self.input_closed || self.input_error.is_some())
    }

    fn next_input(&self) -> NextInput {
        let input = &self.input[self.consumed..];
        if let Some(end) = input.iter().position(|byte| *byte == b'\n') {
            return std::str::from_utf8(&input[..end])
                .ok()
                .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .map_or(NextInput::Forward, |line| NextInput::Line(line.to_owned()));
        }
        if self.input_closed || self.input_error.is_some() {
            NextInput::Forward
        } else {
            NextInput::Wait
        }
    }

    pub(super) fn take_line(
        &mut self,
        eligible: bool,
        frames: &mut Vec<OutboundFrame>,
    ) -> Option<String> {
        match self.next_input() {
            NextInput::Wait => None,
            NextInput::Line(line) if eligible => {
                self.consumed += line.len() + 1;
                self.unit = Some(Unit {
                    line: line.clone(),
                    frames: Vec::new(),
                    begin: None,
                    body: None,
                    complete: false,
                });
                Some(line)
            }
            NextInput::Line(_) | NextInput::Forward => {
                self.enter_forward(frames);
                self.forward_input(frames);
                None
            }
        }
    }

    fn abort_unit(&mut self, frames: &mut Vec<OutboundFrame>) {
        let Some(unit) = self.unit.take() else {
            return;
        };
        self.enter_forward(frames);
        let mut bytes = unit.line.into_bytes();
        bytes.push(b'\n');
        let line = encode(&ProtocolMessage::ControlStdin {
            bytes,
            submitted: true,
            closed: false,
            error: None,
        });
        self.forward(line, frames);
        for frame in unit.frames {
            self.forward(frame, frames);
        }
        self.forward_input(frames);
    }

    fn render_unit(&mut self, unit: Unit) {
        let (Some((time, flags)), Some(body)) = (unit.begin, unit.body) else {
            return;
        };
        let number = self.next_number;
        self.next_number = self.next_number.saturating_add(1);
        let start = self.output.len() + "%begin".len();
        let _ = writeln!(self.output, "%begin {time} {number} {flags}");
        let tail = start..self.output.len();
        if !body.is_empty() {
            self.output.extend_from_slice(&body);
            if body.last() != Some(&b'\n') {
                self.output.push(b'\n');
            }
        }
        self.output.extend_from_slice(b"%end");
        self.output.extend_from_within(tail);
    }

    fn accept(&mut self, frame: OutboundFrame, frames: &mut Vec<OutboundFrame>) -> Option<usize> {
        if !self.direct || self.released {
            self.forward(frame, frames);
            return None;
        }
        let decoded = messages(&frame);
        if let Some(unit) = self.unit.as_mut() {
            unit.frames.push(frame);
            if decoded.is_some_and(|decoded| unit.absorb(decoded)) {
                if !unit.complete {
                    return Some(0);
                }
                let unit = self.unit.take().expect("complete unit");
                let length = unit.frames.iter().map(OutboundFrame::len).sum();
                self.render_unit(unit);
                return Some(length);
            }
            self.abort_unit(frames);
            return None;
        }
        if let Some(decoded) = &decoded {
            let start = self.output.len();
            if decoded
                .iter()
                .all(|message| render_output(&mut self.output, message))
            {
                return Some(frame.len());
            }
            self.output.truncate(start);
        }
        self.enter_forward(frames);
        self.forward(frame, frames);
        self.forward_input(frames);
        None
    }

    pub(super) fn client_write(&mut self, bytes: &[u8], idle: Option<(u64, u64)>, close: bool) {
        if self.released {
            return;
        }
        self.output.extend_from_slice(bytes);
        if let Some((received, next_number)) = idle
            && !self.direct
            && received == self.forwarded
        {
            self.direct = true;
            self.next_number = next_number;
        }
        if close {
            self.close_requested = true;
        }
    }

    fn defer_flush(&self) -> bool {
        self.direct
            && !self.close_requested
            && self.output.len() < DEFERRED_OUTPUT
            && (self.unit.is_some() || self.input[self.consumed..].contains(&b'\n'))
    }

    fn flush(&mut self) -> io::Result<bool> {
        while self.output_offset < self.output.len() {
            match rustix::io::write(&self.stdout, &self.output[self.output_offset..]) {
                Ok(written) => self.output_offset += written,
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => {
                    if !self.stdout_registered {
                        self.registry.register(
                            &mut SourceFd(&self.stdout.as_raw_fd()),
                            self.stdout_token,
                            Interest::WRITABLE,
                        )?;
                        self.stdout_registered = true;
                    }
                    return Ok(false);
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.output.clear();
        self.output_offset = 0;
        self.stop_writing();
        Ok(true)
    }

    fn stop_writing(&mut self) {
        if self.stdout_registered {
            self.stdout_registered = false;
            let _ = self
                .registry
                .deregister(&mut SourceFd(&self.stdout.as_raw_fd()));
        }
    }

    fn release(&mut self, frames: &mut Vec<OutboundFrame>) {
        if self.released {
            return;
        }
        self.deregister();
        self.restore();
        self.released = true;
        frames.push(encode(&ProtocolMessage::ControlStdioClosed));
    }

    fn restore(&self) {
        let _ = rustix::fs::fcntl_setfl(&self.stdin, self.flags[0]);
        let _ = rustix::fs::fcntl_setfl(&self.stdout, self.flags[1]);
    }

    pub(super) fn deregister(&mut self) {
        self.stop_reading();
        self.stop_writing();
    }

    pub(super) fn close(mut self) {
        self.deregister();
        if !self.released {
            self.restore();
        }
    }
}

impl Connection {
    fn takes_stdio_line(&self) -> bool {
        self.initialized
            && !self.busy
            && !self.read_closed
            && self.command.is_none()
            && self.pending.is_empty()
            && self.session.is_some()
    }

    pub(super) fn write_stdio_ready(&mut self) -> io::Result<()> {
        loop {
            if !self.frames.is_empty() {
                let mut slices = self.frames[self.frame_index..]
                    .iter()
                    .map(|frame| io::IoSlice::new(frame))
                    .collect::<Vec<_>>();
                slices[0] = io::IoSlice::new(&self.frames[self.frame_index][self.write_offset..]);
                match self.stream.write_vectored(&slices) {
                    Ok(0) => return Err(io::Error::from(ErrorKind::WriteZero)),
                    Ok(mut written) => {
                        self.outbound.record_write(written);
                        while written != 0 {
                            let remaining = self.frames[self.frame_index].len() - self.write_offset;
                            if written < remaining {
                                self.write_offset += written;
                                break;
                            }
                            written -= remaining;
                            self.frame_index += 1;
                            self.write_offset = 0;
                        }
                        if self.frame_index == self.frames.len() {
                            self.outbound.recycle_written_batch(&mut self.frames);
                            self.frame_index = 0;
                            self.write_offset = 0;
                        }
                        continue;
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                    Err(error) => return Err(error),
                }
            }
            if self.output_wait && !exec::output_pending(&self.outbound) {
                self.output_wait = false;
                let command = self.command.as_ref().expect("output continuation");
                if let Some(continuation) = command.wait() {
                    command.resume(continuation);
                }
                if let Some((waker, _)) = self.outbound.loop_waker.lock().as_ref() {
                    waker.wake()?;
                }
            }
            let Some(stdio) = self.stdio.as_mut() else {
                return Ok(());
            };
            if !stdio.released {
                if !stdio.defer_flush() && !stdio.flush()? {
                    return Ok(());
                }
                if stdio.close_requested && stdio.unit.is_none() {
                    stdio.release(&mut self.frames);
                    continue;
                }
                if stdio.output.len() >= STDOUT_HIGH {
                    return Ok(());
                }
            }
            let mut batch = std::mem::take(&mut stdio.batch);
            self.outbound
                .try_recv_batch(&mut batch, attach::MAX_BATCHED_WRITE_BYTES);
            if batch.is_empty() {
                stdio.batch = batch;
                if stdio.unit.is_some() {
                    stdio.abort_unit(&mut self.frames);
                    continue;
                }
                self.outbound.recycle_written_batch(&mut stdio.batch);
                return Ok(());
            }
            let mut consumed = 0;
            for frame in batch.drain(..) {
                if let Some(length) = stdio.accept(frame, &mut self.frames) {
                    consumed += length;
                }
            }
            stdio.batch = batch;
            if consumed != 0 {
                self.outbound.record_write(consumed);
            }
            if self.frames.is_empty() {
                self.outbound.recycle_written_batch(&mut self.frames);
            }
        }
    }
}

impl EventLoop {
    pub(super) fn close_stdio(&mut self, stdio: Box<ControlStdio>) {
        self.stdio_tokens.remove(&stdio.stdin_token);
        self.stdio_tokens.remove(&stdio.stdout_token);
        stdio.close();
    }

    pub(super) fn drop_stdio(&mut self, token: Token) {
        let Some(connection) = self.connections.get_mut(&token) else {
            return;
        };
        let Some(stdio) = connection.stdio.take() else {
            return;
        };
        if stdio.holds_connection() {
            connection.outbound.close();
        }
        self.close_stdio(stdio);
    }

    pub(super) fn open_stdio(&mut self, token: Token) {
        let connection = self.connections.get_mut(&token).unwrap();
        let fds = std::mem::take(&mut connection.received_fds);
        if connection.stdio.is_some() {
            return;
        }
        match ControlStdio::open(fds, self.poll.registry(), &mut self.next_token) {
            Ok(stdio) => {
                self.stdio_tokens.insert(stdio.stdin_token, token);
                self.stdio_tokens.insert(stdio.stdout_token, token);
                let mut state = connection.outbound.state.lock();
                state.quiet_socket = None;
                state.direct_socket = None;
                drop(state);
                connection.frames.push(ControlStdio::start_marker());
                connection.stdio = Some(Box::new(stdio));
            }
            Err(error) => {
                log::debug!("control stdio refused: {error}");
                let _ = connection
                    .outbound
                    .enqueue_encoded_reliable(ControlStdio::refusal());
            }
        }
    }

    pub(super) fn stdio_ready(&mut self, owner: Token, token: Token, shared: &Arc<Shared>) {
        let Some(connection) = self.connections.get_mut(&owner) else {
            return;
        };
        let Some(stdio) = connection.stdio.as_mut() else {
            return;
        };
        if token != stdio.stdin_token {
            return;
        }
        stdio.read_chunks(true);
        stdio.input_ready(&mut connection.frames);
        if !stdio.wants_line() || !connection.takes_stdio_line() {
            return;
        }
        shared.turn_control_output(
            self.control_output_deadline
                .is_some_and(|deadline| Instant::now() >= deadline),
        );
        if let Err(error) = connection.write_ready() {
            log::debug!("client write failed: {error}");
            self.remove(owner, shared);
            return;
        }
        self.pump_stdio(owner, shared);
    }

    pub(super) fn pump_stdio(&mut self, token: Token, shared: &Arc<Shared>) -> bool {
        let mut drain = true;
        loop {
            let Some(connection) = self.connections.get_mut(&token) else {
                return false;
            };
            let eligible = connection.takes_stdio_line();
            let Some(stdio) = connection.stdio.as_mut() else {
                return true;
            };
            if !stdio.wants_line() {
                if std::mem::take(&mut drain) && stdio.drain_input(&mut connection.frames) {
                    continue;
                }
                break;
            }
            let line = stdio.take_line(eligible, &mut connection.frames);
            let started = line.is_some();
            if let Some(line) = line {
                let hello = &connection.session.as_ref().expect("idle session").hello;
                let request = zz_protocol::ExecRequest {
                    protocol_version: PROTOCOL_VERSION,
                    flags: zz_protocol::ExecFlags::default(),
                    client_instance_id: hello.client_instance_id,
                    origin: None,
                    working_directory: None,
                    tty: None,
                    size: None,
                    features: 0,
                    startup_reentry: None,
                    spawned_server_id: None,
                    expect_server_id: Some(shared.server_id),
                    process_id: hello.process_id,
                    environment: ClientEnvironmentBlob::default(),
                    commands: Vec::new(),
                    raw_control_line: Some(line),
                };
                self.dispatch(token, shared, ProtocolMessage::Exec(request), 0);
            }
            let Some(connection) = self.connections.get_mut(&token) else {
                return false;
            };
            if let Err(error) = connection.write_ready() {
                log::debug!("client write failed: {error}");
                self.remove(token, shared);
                return false;
            }
            let connection = self
                .connections
                .get_mut(&token)
                .expect("written connection");
            let Some(stdio) = connection.stdio.as_mut() else {
                return true;
            };
            if !started || stdio.unit.is_some() {
                if std::mem::take(&mut drain)
                    && stdio.drain_input(&mut connection.frames)
                    && stdio.wants_line()
                {
                    continue;
                }
                break;
            }
        }
        if self.connections[&token]
            .stdio
            .as_ref()
            .is_some_and(|stdio| stdio.wants_input())
        {
            let _ = crate::wake::wake_loop(&self.waker);
        }
        true
    }
}
