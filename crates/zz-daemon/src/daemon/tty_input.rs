use super::*;

#[cfg(test)]
#[path = "tty_input_tests.rs"]
mod tests;

const READ_CHUNK: usize = 4096;
const READ_LIMIT: usize = 64 * 1024;

pub(super) struct TtyInput {
    registry: mio::Registry,
    fd: OwnedFd,
    pub(super) token: Token,
    handoff: u64,
    forwarded: u64,
    direct: Option<PaneId>,
    again: bool,
    input: Vec<u8>,
}

enum TtyRead {
    Open,
    Closed,
}

impl TtyInput {
    fn open(
        fds: Vec<OwnedFd>,
        handoff: u64,
        registry: &mio::Registry,
        next_token: &mut usize,
    ) -> io::Result<Self> {
        let Ok([handed]) = <[OwnedFd; 1]>::try_from(fds) else {
            return Err(io::Error::from(ErrorKind::InvalidInput));
        };
        #[cfg(target_vendor = "apple")]
        let name = rustix::fs::getpath(&handed)?;
        #[cfg(not(target_vendor = "apple"))]
        let name = rustix::termios::ttyname(&handed, Vec::new())?;
        let fd = rustix::fs::open(
            name.as_c_str(),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::NOCTTY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?;
        if rustix::fs::fstat(&handed)?.st_rdev != rustix::fs::fstat(&fd)?.st_rdev {
            return Err(io::Error::from(ErrorKind::InvalidInput));
        }
        let token = jobs::allocate_token(next_token)?;
        registry.register(&mut SourceFd(&fd.as_raw_fd()), token, Interest::READABLE)?;
        Ok(Self {
            registry: registry.try_clone()?,
            fd,
            token,
            handoff,
            forwarded: 0,
            direct: None,
            again: false,
            input: Vec::new(),
        })
    }

    fn read(&mut self) -> TtyRead {
        self.input.clear();
        self.again = false;
        while self.input.len() < READ_LIMIT {
            self.input.reserve(READ_CHUNK);
            let room = self.input.capacity() - self.input.len();
            match rustix::io::read(&self.fd, rustix::buffer::spare_capacity(&mut self.input)) {
                Ok(0) => return TtyRead::Closed,
                Ok(count) if count < room => return TtyRead::Open,
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => return TtyRead::Open,
                Err(_) => return TtyRead::Closed,
            }
        }
        self.again = true;
        TtyRead::Open
    }

    pub(super) fn ready(&mut self, received: u64, pane: Option<PaneId>) {
        if received == self.forwarded {
            self.direct = pane;
        }
    }

    pub(super) fn again(&self) -> bool {
        self.again
    }

    pub(super) fn close(self) {
        let _ = self
            .registry
            .deregister(&mut SourceFd(&self.fd.as_raw_fd()));
    }
}

impl Connection {
    fn takes_tty_key(&self) -> bool {
        self.initialized
            && !self.initializing
            && !self.busy
            && !self.read_closed
            && !self.exec_mode
            && self.command.is_none()
            && self.pending.is_empty()
            && self.session.is_some()
    }
}

impl EventLoop {
    pub(super) fn open_tty(&mut self, token: Token, handoff: u64) {
        self.close_tty(token, false);
        let connection = self.connections.get_mut(&token).unwrap();
        let fds = std::mem::take(&mut connection.received_fds);
        let reply = match TtyInput::open(fds, handoff, self.poll.registry(), &mut self.next_token) {
            Ok(tty) => {
                self.tty_tokens.insert(tty.token, token);
                connection.tty = Some(Box::new(tty));
                ProtocolMessage::TtyInputStarted { handoff }
            }
            Err(error) => {
                log::debug!("tty input refused: {error}");
                ProtocolMessage::TtyInputClosed { handoff }
            }
        };
        let _ = connection.outbound.enqueue_reliable(&reply);
    }

    pub(super) fn release_tty(&mut self, token: Token, handoff: u64) {
        self.close_tty(token, false);
        if let Some(connection) = self.connections.get_mut(&token) {
            let _ = connection
                .outbound
                .enqueue_reliable(&ProtocolMessage::TtyInputClosed { handoff });
        }
    }

    pub(super) fn close_tty(&mut self, token: Token, notify: bool) {
        let Some(connection) = self.connections.get_mut(&token) else {
            return;
        };
        let Some(tty) = connection.tty.take() else {
            return;
        };
        self.tty_tokens.remove(&tty.token);
        if notify {
            let _ = connection
                .outbound
                .enqueue_reliable(&ProtocolMessage::TtyInputClosed {
                    handoff: tty.handoff,
                });
        }
        tty.close();
    }

    pub(super) fn ttys_ready(&mut self, ready: &[(Token, bool, bool)], shared: &Arc<Shared>) {
        for (token, ..) in ready {
            if let Some(&owner) = self.tty_tokens.get(token) {
                self.tty_ready(owner, shared);
            }
        }
    }

    pub(super) fn tty_ready(&mut self, token: Token, shared: &Arc<Shared>) {
        let Some(connection) = self.connections.get_mut(&token) else {
            return;
        };
        let Some(tty) = connection.tty.as_mut() else {
            return;
        };
        if connection
            .client
            .is_some_and(|client| shared.tty_input_left(client))
        {
            self.close_tty(token, true);
            self.flush_tty_owner(token, shared);
            return;
        }
        let closed = matches!(tty.read(), TtyRead::Closed);
        let mut input = std::mem::take(&mut tty.input);
        let mut start = 0;
        let mut wrote = closed;
        if let (Some(client), Some(pane)) = (connection.client, tty.direct) {
            while start < input.len() {
                if !connection.takes_tty_key() {
                    break;
                }
                let Some((key, width)) = zz_protocol::tty_input_key(&input[start..]) else {
                    break;
                };
                let Some(generation) =
                    shared.tty_key_generation(client, pane, &key, &input[start..])
                else {
                    break;
                };
                if let Err(error) = shared.input_plain_key(client, pane, key, generation) {
                    wrote = true;
                    let _ =
                        connection
                            .outbound
                            .enqueue_reliable(&ProtocolMessage::CommandResponse(
                                CommandResponse::Error {
                                    request_id: 0,
                                    error: daemon_server_error(error),
                                    output: RawText::default(),
                                },
                            ));
                }
                start += width;
            }
        }
        let tty = connection.tty.as_mut().expect("tty input");
        if start < input.len() {
            wrote = true;
            tty.direct = None;
            tty.forwarded += 1;
            let bytes = input.split_off(start);
            let _ = connection
                .outbound
                .enqueue_reliable(&ProtocolMessage::TtyInputBytes { bytes });
        }
        input.clear();
        tty.input = input;
        if tty.again {
            let _ = crate::transport::wake_loop(&self.waker);
        }
        if closed {
            self.close_tty(token, true);
        }
        if wrote {
            self.flush_tty_owner(token, shared);
        }
    }

    fn flush_tty_owner(&mut self, token: Token, shared: &Arc<Shared>) {
        if let Some(connection) = self.connections.get_mut(&token)
            && let Err(error) = connection.write_ready()
        {
            log::debug!("client write failed: {error}");
            connection.outbound.close();
            self.remove(token, shared);
        }
    }
}
