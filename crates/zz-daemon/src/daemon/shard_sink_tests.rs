use zz_protocol::decode_protocol_frame;
use zz_terminal::{KittyLayer, KittyPlacement, SessionStatus, TerminalFrameSink, ViewFrame};

use super::shard_sink::PaneSink;
use super::*;

struct Fixture {
    shared: Arc<Shared>,
    session: SessionId,
    pane: PaneId,
    terminal: Arc<TerminalSession>,
}

impl Fixture {
    fn new(server: u64, command: &str) -> Self {
        let shared = Arc::new(Shared::new(server));
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", command]),
            )
            .expect("new-session");
        let pane = context.pane.expect("pane");
        let session = context.session.expect("session");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        Self {
            shared,
            session,
            pane,
            terminal,
        }
    }

    fn client(&self, id: u64) -> (ClientId, Arc<OutboundMailbox>) {
        let client = ClientId(id);
        let mailbox = OutboundMailbox::new();
        let mut inner = self.shared.inner.lock();
        let entry = inner.client_entry(client);
        entry.subscriber = Some(Arc::clone(&mailbox));
        entry.streamed_terminals = Some(BTreeMap::from([(
            self.pane,
            TerminalStreamKind::Foreground,
        )]));
        entry.visible_terminals = Some(BTreeSet::from([self.pane]));
        inner
            .attached
            .entry(self.session)
            .or_default()
            .insert(client);
        sync_client_pane_sink(&inner, client, self.pane);
        drop(inner);
        (client, mailbox)
    }

    fn sink(&self) -> &PaneSink {
        PaneSink::of(&self.terminal).expect("watched panes carry a sink")
    }

    fn deliver(&self, frames: &[ViewFrame]) -> Vec<TerminalViewId> {
        let mut sunk = Vec::new();
        self.sink().deliver(frames, &mut sunk);
        self.sink().published(false);
        sunk
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.shared.request_shutdown();
    }
}

fn frame(base: &TerminalViewport, generation: u64) -> Arc<TerminalViewport> {
    let mut viewport = base.clone();
    viewport.generation = generation;
    viewport.view_generation = generation;
    Arc::new(viewport)
}

fn blank() -> TerminalViewport {
    TerminalViewport::blank(20, 4, SessionStatus::Running)
}

fn view_of(client: ClientId) -> TerminalViewId {
    TerminalViewId(client.0)
}

fn pending(mailbox: &OutboundMailbox, pane: PaneId) -> Option<(TerminalGeneration, bool)> {
    mailbox
        .state
        .lock()
        .terminals
        .get(&pane)
        .map(|pending| (pending.current, pending.full))
}

fn pop_event(mailbox: &OutboundMailbox) -> Option<EventPayload> {
    let frame = pop_ready_frame(&mut mailbox.state.lock())?;
    match decode_protocol_frame(&frame).expect("decode terminal frame") {
        ProtocolMessage::Event(event) => Some(event.payload),
        other => panic!("not an event: {other:?}"),
    }
}

#[test]
fn a_detach_racing_a_shard_publish_never_leaves_a_frame_behind() {
    let fixture = Fixture::new(4101, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9101);
    let view = view_of(client);
    let pane = fixture.pane;
    let terminal = Arc::clone(&fixture.terminal);
    let halt = Arc::new(AtomicBool::new(false));
    let publisher = thread::spawn({
        let halt = Arc::clone(&halt);
        move || {
            let sink = PaneSink::of(&terminal).expect("sink");
            let base = blank();
            let mut offset = 0;
            while !halt.load(Ordering::Acquire) {
                let mut taken = Vec::new();
                sink.deliver(
                    &[(view, frame(&base, (1 << 40) + offset), Some(1))],
                    &mut taken,
                );
                sink.published(false);
                offset += 1;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while pending(&mailbox, pane).is_none() {
        assert!(Instant::now() < deadline, "the shard never delivered");
        thread::yield_now();
    }
    {
        let mut inner = fixture.shared.inner.lock();
        if let Some(clients) = inner.attached.get_mut(&fixture.session) {
            clients.remove(&client);
        }
        let streamed = inner
            .client_entry(client)
            .streamed_terminals
            .take()
            .unwrap_or_default();
        apply_view_streams(&inner, view, &streamed, &BTreeMap::new());
    }
    mailbox.suspend_terminal(pane);
    thread::sleep(Duration::from_millis(20));
    halt.store(true, Ordering::Release);
    publisher.join().expect("publisher thread");
    assert!(fixture.sink().views().is_empty());
    assert_eq!(pending(&mailbox, pane), None);
    assert!(
        fixture
            .deliver(&[(view, frame(&blank(), 1 << 41), Some(1))])
            .is_empty()
    );
    assert_eq!(pending(&mailbox, pane), None);
}

#[test]
fn a_window_switch_moves_the_sink_record_to_the_new_window() {
    let fixture = Fixture::new(4102, "exec sleep 1000000");
    let mut context =
        ExecutionContext::for_pane(&fixture.shared.inner.lock().engine.state, fixture.pane)
            .expect("pane context");
    fixture
        .shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-window", ["-d", "exec sleep 1000000"]),
        )
        .expect("new-window");
    let mailbox = OutboundMailbox::new();
    let (client, _) = fixture.shared.register_subscribed(
        ClientKind::Interactive,
        Some("sink-switch".to_owned()),
        None,
        Arc::clone(&mailbox),
    );
    fixture
        .shared
        .attach(client, fixture.session)
        .expect("attach");
    let second = {
        let inner = fixture.shared.inner.lock();
        let session = &inner.engine.state.sessions[&fixture.session];
        session
            .windows
            .iter()
            .flat_map(|window| inner.engine.state.windows[window].panes.keys())
            .copied()
            .find(|pane| *pane != fixture.pane)
            .expect("second pane")
    };
    let second_terminal = Arc::clone(&fixture.shared.inner.lock().terminals[&second]);
    let view = view_of(client);
    assert_eq!(fixture.sink().views(), vec![view]);
    assert!(
        PaneSink::of(&second_terminal)
            .expect("sink")
            .views()
            .is_empty()
    );

    fixture
        .shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("next-window", Vec::<&str>::new()),
        )
        .expect("next-window");
    assert!(fixture.sink().views().is_empty());
    assert_eq!(
        PaneSink::of(&second_terminal).expect("sink").views(),
        vec![view]
    );
    let stale = frame(&blank(), 1 << 42);
    assert!(fixture.deliver(&[(view, stale, Some(1))]).is_empty());
    assert!(
        mailbox
            .state
            .lock()
            .terminals
            .get(&fixture.pane)
            .is_none_or(|pending| pending.current.view != 1 << 42)
    );
}

#[test]
fn a_frozen_client_falls_back_to_the_loop_path_until_the_freeze_ends() {
    let fixture = Fixture::new(4103, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9103);
    let view = view_of(client);
    let pane = fixture.pane;
    {
        let mut inner = fixture.shared.inner.lock();
        let _ = arm_client_message(&mut inner, client, 1, 0, true);
        assert!(client_terminal_publication_frozen(&inner, client));
    }
    assert!(mailbox.terminals_frozen());
    let base = blank();
    let first = frame(&base, 1 << 43);
    assert!(
        fixture
            .deliver(&[(view, Arc::clone(&first), Some(1))])
            .is_empty()
    );
    assert_eq!(pending(&mailbox, pane), None);
    let mut fanout = PaneFrameFanout::new();
    fixture.shared.publish_terminal_for_pane(
        pane,
        client,
        None,
        &first,
        &fixture.terminal,
        &mut fanout,
    );
    assert_eq!(pending(&mailbox, pane), None);

    {
        let mut inner = fixture.shared.inner.lock();
        take_client_message(&mut inner, client).expect("armed message");
    }
    assert!(!mailbox.terminals_frozen());
    let second = frame(&base, (1 << 43) + 1);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&second), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&second), true))
    );
}

#[test]
fn a_slot_holding_another_base_takes_a_full_frame() {
    let fixture = Fixture::new(4104, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9104);
    let view = view_of(client);
    let pane = fixture.pane;
    let base = blank();
    let generation = 1 << 44;
    let first = frame(&base, generation);
    assert_eq!(fixture.deliver(&[(view, first, Some(1))]), vec![view]);
    assert!(matches!(
        pop_event(&mailbox),
        Some(EventPayload::TerminalViewport { .. })
    ));

    let second = frame(&base, generation + 1);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&second), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&second), false))
    );

    let third = frame(&base, generation + 2);
    assert!(
        fixture
            .deliver(&[(view, Arc::clone(&third), Some(1))])
            .is_empty()
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&second), false))
    );
    let mut fanout = PaneFrameFanout::new();
    fixture.shared.publish_terminal_for_pane(
        pane,
        client,
        Some(&second),
        &third,
        &fixture.terminal,
        &mut fanout,
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&third), true))
    );
    assert!(matches!(
        pop_event(&mailbox),
        Some(EventPayload::TerminalViewport { .. })
    ));

    let fourth = frame(&base, generation + 3);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&fourth), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&fourth), true))
    );
    pop_event(&mailbox).expect("full frame after the loop");

    let loop_frame = frame(&base, generation + 4);
    assert!(mailbox.replace_terminal_viewport(pane, &loop_frame, &fixture.shared.terminal_frames));
    pop_event(&mailbox).expect("loop frame");
    let sixth = frame(&base, generation + 5);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&sixth), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&sixth), true))
    );
    pop_event(&mailbox).expect("full frame against the loop base");
}

#[test]
fn a_frame_with_kitty_placements_falls_back_to_the_loop() {
    let fixture = Fixture::new(4105, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9105);
    let view = view_of(client);
    let pane = fixture.pane;
    let base = blank();
    let generation = 1 << 45;
    let mut placed = (*frame(&base, generation)).clone();
    placed.kitty_placements = Arc::from(vec![KittyPlacement {
        image_id: 1,
        image_generation: 1,
        layer: KittyLayer::AboveText,
        viewport_col: 0,
        viewport_row: 0,
        absolute_row: 0,
        cell_offset_x: 0,
        cell_offset_y: 0,
        grid_cols: 1,
        grid_rows: 1,
        pixel_width: 8,
        pixel_height: 16,
        source_rect: None,
    }]);
    assert!(
        fixture
            .deliver(&[(view, Arc::new(placed), Some(1))])
            .is_empty()
    );
    assert_eq!(pending(&mailbox, pane), None);

    let cleared = frame(&base, generation + 1);
    assert!(fixture.deliver(&[(view, cleared, Some(1))]).is_empty());
    assert_eq!(pending(&mailbox, pane), None);

    let plain = frame(&base, generation + 2);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&plain), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&plain), true))
    );
}

#[test]
fn copy_mode_takes_the_view_off_the_sink_until_the_loop_ends_it() {
    let fixture = Fixture::new(4106, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9106);
    let view = view_of(client);
    let pane = fixture.pane;
    {
        let mut inner = fixture.shared.inner.lock();
        enter_copy_session(&mut inner, client, pane).expect("enter copy session");
    }
    assert!(fixture.sink().views().is_empty());
    let base = blank();
    assert!(
        fixture
            .deliver(&[(view, frame(&base, 1 << 46), Some(1))])
            .is_empty()
    );
    assert_eq!(pending(&mailbox, pane), None);

    let mut copy = (*frame(&base, (1 << 46) + 1)).clone();
    copy.mode = TerminalMode::Copy {
        position: 0,
        total: 0,
        hide_position: false,
    };
    {
        let mut inner = fixture.shared.inner.lock();
        reconcile_copy_session(&mut inner, pane, client, copy.mode);
        assert!(fixture.sink().views().is_empty());
        reconcile_copy_session(&mut inner, pane, client, TerminalMode::Live);
        assert!(
            inner
                .client(client)
                .is_some_and(|c| c.copy_session.is_none())
        );
    }
    assert_eq!(fixture.sink().views(), vec![view]);
    assert!(
        fixture
            .deliver(&[(view, Arc::new(copy), Some(1))])
            .is_empty()
    );
    let live = frame(&base, (1 << 46) + 2);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&live), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&live), true))
    );
}

#[test]
fn two_sinks_on_one_pane_share_one_encode_per_frame() {
    let fixture = Fixture::new(4107, "exec sleep 1000000");
    let (first_client, first) = fixture.client(9107);
    let (second_client, second) = fixture.client(9108);
    let views = [view_of(first_client), view_of(second_client)];
    let base = blank();
    let generation = 1 << 47;
    for step in 0..4 {
        let current = frame(&base, generation + step);
        let frames = views
            .iter()
            .map(|view| (*view, Arc::clone(&current), Some(1)))
            .collect::<Vec<_>>();
        let before = terminal_encodes();
        assert_eq!(fixture.deliver(&frames), views.to_vec());
        let after = terminal_encodes();
        let encodes = (after.0 - before.0) + (after.1 - before.1);
        assert_eq!(encodes, 1, "step {step}");
        let shared_frame = |mailbox: &OutboundMailbox| {
            Arc::clone(&mailbox.state.lock().terminals[&fixture.pane].encoded)
        };
        assert!(Arc::ptr_eq(&shared_frame(&first), &shared_frame(&second)));
        let kinds = [&first, &second].map(|mailbox| pop_event(mailbox).expect("frame"));
        for kind in kinds {
            match (step, kind) {
                (0, EventPayload::TerminalViewport { .. })
                | (1.., EventPayload::TerminalPatch { .. }) => {}
                (step, other) => panic!("step {step}: {other:?}"),
            }
        }
    }
}

#[test]
fn a_streamed_pane_reaches_its_client_from_the_shard_and_still_syncs_its_title() {
    let fixture = Fixture::new(
        4108,
        if cfg!(windows) {
            "ping -n 2 127.0.0.1 >nul& echo \u{1b}]2;sink-title\u{7}& for /L %i in (0,0,1) do @(echo tick& ping -n 1 127.0.0.1 >nul)"
        } else {
            "sleep 0.3; printf '\\033]2;sink-title\\007'; while :; do echo tick; sleep 0.02; done"
        },
    );
    let mailbox = OutboundMailbox::new();
    let (client, _) = fixture.shared.register_subscribed(
        ClientKind::Interactive,
        Some("sink-stream".to_owned()),
        None,
        Arc::clone(&mailbox),
    );
    fixture
        .shared
        .attach(client, fixture.session)
        .expect("attach");
    let view = view_of(client);
    assert_eq!(fixture.sink().views(), vec![view]);
    let drain = || {
        mailbox.release_terminals();
        while pop_ready_frame(&mut mailbox.state.lock()).is_some() {}
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let wait_sunk = || loop {
        drain();
        if fixture.terminal.latest_frames().1.contains(&view) {
            break;
        }
        assert!(Instant::now() < deadline, "the shard never sank a frame");
        thread::sleep(Duration::from_millis(5));
    };
    wait_sunk();
    loop {
        drain();
        let title = fixture
            .shared
            .inner
            .lock()
            .engine
            .state
            .pane(fixture.pane)
            .map(|pane| pane.title.clone());
        if title.as_deref() == Some("sink-title") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the sink-delivered title did not sync: {title:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
    wait_sunk();
}

#[test]
fn a_retired_terminal_stops_delivering_to_its_clients() {
    let fixture = Fixture::new(4109, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9109);
    let view = view_of(client);
    assert_eq!(fixture.sink().views(), vec![view]);
    retire_terminal(&fixture.terminal);
    assert!(fixture.sink().views().is_empty());
    assert!(
        fixture
            .deliver(&[(view, frame(&blank(), 1 << 48), Some(1))])
            .is_empty()
    );
    assert_eq!(pending(&mailbox, fixture.pane), None);
}

fn copy_frame(base: &TerminalViewport, generation: u64) -> Arc<TerminalViewport> {
    let mut viewport = (*frame(base, generation)).clone();
    viewport.mode = TerminalMode::Copy {
        position: 1,
        total: 1,
        hide_position: false,
    };
    Arc::new(viewport)
}

fn publish_on_loop(fixture: &Fixture, view: TerminalViewId, viewport: &Arc<TerminalViewport>) {
    let mut fanout = PaneFrameFanout::new();
    shard_sink::publish_loop_view(
        &fixture.shared,
        &fixture.terminal,
        fixture.pane,
        &(view, Arc::clone(viewport), Some(1)),
        None,
        &mut fanout,
    );
}

#[test]
fn a_copy_mode_round_trip_keeps_patching_across_the_sink_and_the_loop() {
    let fixture = Fixture::new(4110, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9110);
    let view = view_of(client);
    let pane = fixture.pane;
    let base = blank();
    let generation = 1 << 49;
    let first = frame(&base, generation);
    assert_eq!(fixture.deliver(&[(view, first, Some(1))]), vec![view]);
    pop_event(&mailbox).expect("first full frame");
    let live = frame(&base, generation + 1);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&live), Some(1))]),
        vec![view]
    );
    pop_event(&mailbox).expect("live patch");

    {
        let mut inner = fixture.shared.inner.lock();
        enter_copy_session(&mut inner, client, pane).expect("enter copy session");
    }
    let copy = copy_frame(&base, generation + 2);
    assert!(
        fixture
            .deliver(&[(view, Arc::clone(&copy), Some(1))])
            .is_empty()
    );
    publish_on_loop(&fixture, view, &copy);
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&copy), false))
    );
    assert!(
        fixture
            .sink()
            .previous(view)
            .is_some_and(|previous| Arc::ptr_eq(&previous, &copy))
    );
    pop_event(&mailbox).expect("copy patch");

    {
        let mut inner = fixture.shared.inner.lock();
        exit_copy_session(&mut inner, client);
    }
    let after = frame(&base, generation + 3);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&after), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&after), false))
    );
}

#[test]
fn a_frame_the_loop_took_while_the_slot_was_full_keeps_the_patch_chain() {
    let fixture = Fixture::new(4111, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9111);
    let view = view_of(client);
    let pane = fixture.pane;
    let base = blank();
    let generation = 1 << 50;
    assert_eq!(
        fixture.deliver(&[(view, frame(&base, generation), Some(1))]),
        vec![view]
    );
    pop_event(&mailbox).expect("first full frame");
    let queued = frame(&base, generation + 1);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&queued), Some(1))]),
        vec![view]
    );
    let held = frame(&base, generation + 2);
    assert!(
        fixture
            .deliver(&[(view, Arc::clone(&held), Some(1))])
            .is_empty()
    );
    assert!(
        fixture
            .sink()
            .previous(view)
            .is_some_and(|previous| Arc::ptr_eq(&previous, &queued))
    );
    pop_event(&mailbox).expect("queued patch");
    publish_on_loop(&fixture, view, &held);
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&held), false))
    );
    pop_event(&mailbox).expect("loop patch");
    let next = frame(&base, generation + 3);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&next), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&next), false))
    );
}

#[test]
fn a_frozen_frame_still_moves_the_base_the_sink_resumes_from() {
    let fixture = Fixture::new(4112, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9112);
    let view = view_of(client);
    let pane = fixture.pane;
    let base = blank();
    let generation = 1 << 51;
    assert_eq!(
        fixture.deliver(&[(view, frame(&base, generation), Some(1))]),
        vec![view]
    );
    pop_event(&mailbox).expect("first full frame");
    {
        let mut inner = fixture.shared.inner.lock();
        let _ = arm_client_message(&mut inner, client, 1, 0, true);
    }
    let frozen = frame(&base, generation + 1);
    assert!(
        fixture
            .deliver(&[(view, Arc::clone(&frozen), Some(1))])
            .is_empty()
    );
    publish_on_loop(&fixture, view, &frozen);
    assert_eq!(pending(&mailbox, pane), None);
    {
        let mut inner = fixture.shared.inner.lock();
        take_client_message(&mut inner, client).expect("armed message");
    }
    assert!(mailbox.replace_terminal_viewport(pane, &frozen, &fixture.shared.terminal_frames));
    pop_event(&mailbox).expect("resume full frame");
    let resumed = frame(&base, generation + 2);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&resumed), Some(1))]),
        vec![view]
    );
    assert_eq!(
        pending(&mailbox, pane),
        Some((viewport_generation(&resumed), false))
    );
}

#[test]
fn a_loop_frame_older_than_the_sink_frame_is_never_delivered() {
    let fixture = Fixture::new(4113, "exec sleep 1000000");
    let (client, mailbox) = fixture.client(9113);
    let view = view_of(client);
    let pane = fixture.pane;
    let base = blank();
    let generation = 1 << 52;
    assert_eq!(
        fixture.deliver(&[(view, frame(&base, generation), Some(1))]),
        vec![view]
    );
    pop_event(&mailbox).expect("first full frame");
    let newer = frame(&base, generation + 2);
    assert_eq!(
        fixture.deliver(&[(view, Arc::clone(&newer), Some(1))]),
        vec![view]
    );
    pop_event(&mailbox).expect("newer patch");
    publish_on_loop(&fixture, view, &frame(&base, generation + 1));
    assert_eq!(pending(&mailbox, pane), None);
    assert!(
        fixture
            .sink()
            .previous(view)
            .is_some_and(|previous| Arc::ptr_eq(&previous, &newer))
    );
}

#[test]
fn an_output_watcher_makes_every_sunk_frame_urgent() {
    let fixture = Fixture::new(4114, "exec sleep 1000000");
    let (client, _mailbox) = fixture.client(9114);
    let view = view_of(client);
    let base = blank();
    let mut sunk = Vec::new();
    assert!(!fixture.sink().deliver(&[], &mut sunk));
    fixture.sink().published(false);
    let observer = Arc::new(());
    fixture.sink().observe(&observer);
    assert!(
        fixture
            .sink()
            .deliver(&[(view, frame(&base, 1 << 53), Some(1))], &mut sunk)
    );
    fixture.sink().published(false);
    assert_eq!(sunk, vec![view]);
    drop(observer);
    assert!(!fixture.sink().deliver(&[], &mut sunk));
    fixture.sink().published(false);
}

#[cfg(unix)]
#[test]
fn the_loop_wake_waits_for_the_publish_and_skips_a_notified_one() {
    let fixture = Fixture::new(4115, "exec sleep 1000000");
    let view = view_of(ClientId(9115));
    let pane = fixture.pane;
    let pane_sink = PaneSink::new(
        pane,
        Arc::clone(&fixture.shared.terminal_frames),
        fixture.terminal.output_wake(),
    );
    let mailbox = OutboundMailbox::new();
    pane_sink.set_view(view, Some((Arc::clone(&mailbox), true)));
    let mut poll = mio::Poll::new().expect("poll");
    let waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(7)).expect("waker"));
    let owner = thread::spawn(|| thread::current().id())
        .join()
        .expect("owner thread");
    *mailbox.loop_waker.lock() = Some((Arc::clone(&waker), owner));
    let mut events = mio::Events::with_capacity(4);
    let mut woke = || {
        poll.poll(&mut events, Some(Duration::ZERO)).expect("poll");
        !events.is_empty()
    };
    let base = blank();
    let generation = 1 << 54;
    let mut sunk = Vec::new();
    assert!(!pane_sink.deliver(&[(view, frame(&base, generation), Some(1))], &mut sunk));
    assert_eq!(sunk, vec![view]);
    assert!(pending(&mailbox, pane).is_some());
    assert!(!woke());
    pane_sink.published(false);
    assert!(woke());
    pop_event(&mailbox).expect("first frame");

    sunk.clear();
    pane_sink.deliver(&[(view, frame(&base, generation + 1), Some(1))], &mut sunk);
    assert_eq!(sunk, vec![view]);
    pane_sink.published(true);
    assert!(!woke());
    mailbox.notify_one();
    assert!(woke());
}
