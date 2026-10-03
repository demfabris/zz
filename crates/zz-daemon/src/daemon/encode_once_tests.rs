use zz_protocol::decode_protocol_frame;

use super::*;

struct Stream {
    shared: Arc<Shared>,
    pane: PaneId,
    terminal: Arc<TerminalSession>,
    clients: Vec<(ClientId, Arc<OutboundMailbox>)>,
    fanout: PaneFrameFanout,
}

impl Stream {
    fn new(server: u64, kinds: &[TerminalStreamKind]) -> Self {
        let shared = Arc::new(Shared::new(server));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "exec sleep 1000000"]),
            )
            .expect("new-session");
        let pane = context.pane.expect("pane");
        let session = context.session.expect("session");
        let mut inner = shared.inner.lock();
        let clients = kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let client = ClientId(9000 + index as u64);
                let mailbox = OutboundMailbox::new();
                let entry = inner.client_entry(client);
                entry.subscriber = Some(Arc::clone(&mailbox));
                entry.streamed_terminals = Some(BTreeMap::from([(pane, *kind)]));
                entry.visible_terminals = Some(BTreeSet::from([pane]));
                inner.attached.entry(session).or_default().insert(client);
                (client, mailbox)
            })
            .collect();
        let terminal = Arc::clone(&inner.terminals[&pane]);
        drop(inner);
        Self {
            shared,
            pane,
            terminal,
            clients,
            fanout: PaneFrameFanout::new(),
        }
    }

    fn publish(
        &mut self,
        base: Option<&TerminalViewport>,
        current: &TerminalViewport,
    ) -> (usize, usize) {
        let before = terminal_encodes();
        for (client, _) in &self.clients {
            self.shared.publish_terminal_for_pane(
                self.pane,
                *client,
                base,
                current,
                &self.terminal,
                &mut self.fanout,
            );
        }
        self.fanout.release();
        let after = terminal_encodes();
        (after.0 - before.0, after.1 - before.1)
    }

    fn pending(&self) -> Vec<Option<Arc<[u8]>>> {
        self.clients
            .iter()
            .map(|(_, mailbox)| {
                mailbox
                    .state
                    .lock()
                    .terminals
                    .get(&self.pane)
                    .map(|pending| Arc::clone(&pending.encoded))
            })
            .collect()
    }

    fn deliver(&self) -> Vec<Event> {
        self.clients
            .iter()
            .filter_map(|(_, mailbox)| pop_ready_frame(&mut mailbox.state.lock()))
            .map(
                |frame| match decode_protocol_frame(&frame).expect("decode terminal frame") {
                    ProtocolMessage::Event(event) => event,
                    other => panic!("not a terminal event: {other:?}"),
                },
            )
            .collect()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.shared.request_shutdown();
    }
}

fn viewport(base: &TerminalViewport, generation: u64) -> TerminalViewport {
    let mut viewport = base.clone();
    viewport.generation = generation;
    viewport.view_generation = generation;
    viewport
}

fn one_shared_frame(pending: &[Option<Arc<[u8]>>]) -> Arc<[u8]> {
    let first = pending[0].clone().expect("a queued frame");
    for frame in pending {
        let frame = frame.as_ref().expect("every client has a queued frame");
        assert!(Arc::ptr_eq(frame, &first), "clients hold different encodes");
    }
    first
}

#[test]
fn two_clients_streaming_one_pane_share_one_encode_per_frame() {
    let mut stream = Stream::new(
        3901,
        &[
            TerminalStreamKind::Foreground,
            TerminalStreamKind::Foreground,
        ],
    );
    let blank = TerminalViewport::blank(40, 10, zz_terminal::SessionStatus::Running);
    let first = viewport(&blank, 1 << 50);
    let second = viewport(&first, (1 << 50) + 1);
    let third = viewport(&first, (1 << 50) + 2);
    let fourth = viewport(&first, (1 << 50) + 3);

    assert_eq!(stream.publish(None, &first), (0, 1));
    one_shared_frame(&stream.pending());
    let events = stream.deliver();
    assert_eq!(events.len(), 2);
    for event in &events {
        assert_eq!(event.sequence, first.view_generation);
        assert!(matches!(
            event.payload,
            EventPayload::TerminalViewport { .. }
        ));
    }

    assert_eq!(stream.publish(Some(&first), &second), (1, 0));
    one_shared_frame(&stream.pending());
    let events = stream.deliver();
    assert_eq!(events.len(), 2);
    for event in &events {
        assert_eq!(event.sequence, second.view_generation);
        assert!(matches!(event.payload, EventPayload::TerminalPatch { .. }));
    }

    assert_eq!(stream.publish(Some(&second), &third), (1, 0));
    assert_eq!(stream.publish(Some(&third), &fourth), (0, 1));
    let full = one_shared_frame(&stream.pending());
    let events = stream.deliver();
    assert_eq!(events.len(), 2);
    for event in &events {
        assert_eq!(event.sequence, fourth.view_generation);
        assert!(matches!(
            event.payload,
            EventPayload::TerminalViewport { .. }
        ));
    }

    let late = OutboundMailbox::new();
    let before = terminal_encodes();
    assert!(late.replace_terminal_viewport(stream.pane, &fourth, &stream.shared.terminal_frames));
    assert_eq!(terminal_encodes(), before);
    let queued = late.state.lock().terminals[&stream.pane].encoded.clone();
    assert!(Arc::ptr_eq(&queued, &full));
}

#[test]
fn a_preview_stream_and_a_lagging_client_keep_their_own_decisions() {
    let mut stream = Stream::new(
        3902,
        &[TerminalStreamKind::Foreground, TerminalStreamKind::Preview],
    );
    let blank = TerminalViewport::blank(40, 10, zz_terminal::SessionStatus::Running);
    let first = viewport(&blank, 1 << 51);
    let second = viewport(&first, (1 << 51) + 1);
    let third = viewport(&first, (1 << 51) + 2);

    assert_eq!(stream.publish(None, &first), (0, 1));
    assert_eq!(stream.deliver().len(), 2);

    assert_eq!(stream.publish(Some(&first), &second), (1, 0));
    one_shared_frame(&stream.pending());
    let (_, foreground) = &stream.clients[0];
    pop_ready_frame(&mut foreground.state.lock()).expect("foreground patch");

    assert_eq!(stream.publish(Some(&second), &third), (1, 0));
    let pending = stream.pending();
    let patch = pending[0]
        .clone()
        .expect("the foreground client takes the patch");
    let preview = pending[1]
        .clone()
        .expect("the preview keeps its older frame");
    assert!(!Arc::ptr_eq(&patch, &preview));
    let preview_state = stream.clients[1].1.state.lock();
    assert!(preview_state.preview_refreshes.contains(&stream.pane));
    assert_eq!(
        preview_state.terminals[&stream.pane].current,
        viewport_generation(&second)
    );
}

#[test]
fn every_client_frame_for_one_pane_carries_the_pane_stream_sequence() {
    let mut stream = Stream::new(
        3903,
        &[
            TerminalStreamKind::Foreground,
            TerminalStreamKind::Foreground,
        ],
    );
    let blank = TerminalViewport::blank(20, 4, zz_terminal::SessionStatus::Running);
    let mut previous: Option<TerminalViewport> = None;
    let mut sequences = Vec::new();
    for step in 0..5 {
        let current = viewport(&blank, (1 << 52) + step);
        stream.publish(previous.as_ref(), &current);
        let events = stream.deliver();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, events[1].sequence);
        sequences.push(events[0].sequence);
        previous = Some(current);
    }
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "{sequences:?}"
    );
}

#[test]
fn the_full_frame_cache_keeps_one_frame_per_pane_within_its_bounds() {
    let frames = TerminalFrames::default();
    let blank = TerminalViewport::blank(20, 4, zz_terminal::SessionStatus::Running);
    let older = viewport(&blank, 1 << 53);
    let newer = viewport(&blank, (1 << 53) + 1);
    let first = frames.full(PaneId(1), &older).expect("encode");
    let second = frames.full(PaneId(1), &newer).expect("encode");
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(frames.full.lock().frames.len(), 1);
    let before = terminal_encodes();
    assert!(Arc::ptr_eq(
        &frames.full(PaneId(1), &newer).expect("cached"),
        &second
    ));
    assert_eq!(terminal_encodes(), before);
    for pane in 2..40 {
        frames.full(PaneId(pane), &newer).expect("encode");
    }
    let cached = frames.full.lock();
    assert_eq!(cached.frames.len(), MAX_CACHED_FULL_FRAMES);
    assert_eq!(
        cached.bytes,
        cached
            .frames
            .iter()
            .map(|cached| cached.frame.len())
            .sum::<usize>()
    );
    assert!(cached.bytes <= MAX_CACHED_FULL_FRAME_BYTES);
}
