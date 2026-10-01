use super::*;

#[test]
fn fanout_keeps_one_shared_encoded_frame() {
    let first = OutboundMailbox::new();
    let second = OutboundMailbox::new();
    let frame: Arc<[u8]> = Arc::from(
        zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync).expect("encode"),
    );
    assert!(first.enqueue_encoded_reliable(Arc::clone(&frame)));
    assert!(second.enqueue_encoded_reliable(Arc::clone(&frame)));
    let first = first.state.lock();
    let second = second.state.lock();
    let (OutboundFrame::Shared(left), OutboundFrame::Shared(right)) =
        (&first.reliable[0], &second.reliable[0])
    else {
        panic!("fanout copied its shared bytes")
    };
    assert!(Arc::ptr_eq(left, right));
}

#[test]
fn scoped_tree_group_keeps_shared_children_through_flush_and_writer_completion() {
    let shared = Arc::new(Shared::new(53));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "shared-tree"]),
        )
        .expect("model session");
    let subscriptions = zz_protocol::Subscriptions {
        tree: zz_protocol::TreeSubscription::All,
        status: false,
        options: 0,
        keys: zz_protocol::KeySubscription::None,
        pane_stream: false,
    };
    let (first_client, first) = compact_registered(&shared, subscriptions);
    let (second_client, second) = compact_registered(&shared, subscriptions);
    for (client, mailbox) in [(first_client, &first), (second_client, &second)] {
        shared.send_compact_state(client, mailbox, true);
        reliable_children(mailbox);
    }
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("rename-session", ["-t", "shared-tree", "renamed"]),
        )
        .expect("rename session");
    shared.publish_compact_trees();
    let children = [&first, &second].map(|mailbox| {
        let state = mailbox.state.lock();
        let Some(OutboundFrame::Grouped { frames, .. }) = state.reliable.front() else {
            panic!("tree publication was not grouped")
        };
        let OutboundFrame::Shared(frame) = &frames[0] else {
            panic!("tree child was copied")
        };
        Arc::clone(frame)
    });
    assert!(Arc::ptr_eq(&children[0], &children[1]));
    assert!(first.flush_control_batch(false));
    {
        let state = first.state.lock();
        let Some(OutboundFrame::Grouped { frames, .. }) = state.reliable.front() else {
            panic!("tree flush was not grouped")
        };
        let OutboundFrame::Shared(frame) = &frames[0] else {
            panic!("tree flush copied its child")
        };
        assert!(Arc::ptr_eq(frame, &children[1]));
    }
    let mut pending = Vec::new();
    for mailbox in [&first, &second] {
        assert!(mailbox.recv_batch(&mut pending, MAX_OUTBOUND_BYTES));
        let ProtocolMessage::Batch(batch) =
            zz_protocol::decode_protocol_frame(&pending[0]).expect("decode tree batch")
        else {
            panic!("missing tree batch")
        };
        assert!(matches!(
            batch.messages().expect("flat tree").as_slice(),
            [ProtocolMessage::Event(Event {
                payload: EventPayload::TreeDelta(_) | EventPayload::Snapshot(_),
                ..
            })]
        ));
        mailbox.finish_batch(&mut pending);
        let state = mailbox.state.lock();
        assert_eq!(state.recycled_frames.len(), 1);
        assert!(state.recycled_capacity <= MAX_RECYCLED_FRAME_CAPACITY);
    }
    assert_eq!(Arc::strong_count(&children[0]), 2);
}

#[test]
fn compact_batch_flush_keeps_existing_groups_flat() {
    let mailbox = OutboundMailbox::new();
    let buffer = Vec::with_capacity(4096);
    let allocation = buffer.as_ptr();
    mailbox.recycle_frame(buffer);
    let child = zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync).expect("encode");
    assert!(mailbox.enqueue_control_group(vec![child.into()]));
    assert!(mailbox.flush_control_batch(false));
    assert!(mailbox.flush_control_batch(false));
    assert!(matches!(
        mailbox.state.lock().reliable.front(),
        Some(OutboundFrame::Grouped { encoded, .. }) if encoded.as_ptr() == allocation
    ));
    let messages = tests::take_reliable_messages(&mailbox);
    let [ProtocolMessage::Batch(batch)] = messages.as_slice() else {
        panic!("expected one flat batch: {messages:?}")
    };
    assert!(matches!(
        batch.messages().expect("flat group").as_slice(),
        [ProtocolMessage::TreeSync]
    ));
}

#[test]
fn completed_group_reuses_its_buffers_after_the_writer_finishes() {
    let mailbox = OutboundMailbox::new();
    let outer = Vec::with_capacity(4096);
    let outer_capacity = outer.capacity();
    mailbox.recycle_frame(outer);
    let mut child = Vec::with_capacity(512);
    zz_protocol::encode_protocol_message_into(&ProtocolMessage::TreeSync, &mut child)
        .expect("encode child");
    let allocation = child.as_ptr();
    let child_capacity = child.capacity();
    assert!(mailbox.enqueue_control_group(vec![child.into()]));
    let mut frames = Vec::new();
    assert!(mailbox.recv_batch(&mut frames, MAX_OUTBOUND_BYTES));
    let wire_bytes = frames[0].len();
    {
        let state = mailbox.state.lock();
        assert_eq!(state.queued_bytes, 0);
        assert_eq!(state.written_bytes, 0);
        assert!(state.recycled_frames.is_empty());
    }
    let ProtocolMessage::Batch(batch) =
        zz_protocol::decode_protocol_frame(&frames[0]).expect("decode group")
    else {
        panic!("expected written group")
    };
    assert_eq!(
        batch.messages().expect("flat group"),
        [ProtocolMessage::TreeSync]
    );
    mailbox.finish_batch(&mut frames);
    assert!(frames.is_empty());
    {
        let state = mailbox.state.lock();
        assert_eq!(state.written_bytes, wire_bytes as u64);
        assert_eq!(state.recycled_frames.len(), 2);
        assert_eq!(state.recycled_capacity, outer_capacity + child_capacity);
    }
    let reused = mailbox
        .encode_message(&ProtocolMessage::TreeSync)
        .expect("reuse child");
    assert_eq!(reused.as_ptr(), allocation);
    assert_eq!(
        zz_protocol::decode_protocol_frame(&reused).expect("decode reused"),
        ProtocolMessage::TreeSync
    );
    assert_eq!(mailbox.state.lock().recycled_capacity, outer_capacity);
}

#[test]
fn grouped_buffer_recycling_keeps_limits_and_shared_ownership() {
    let mailbox = OutboundMailbox::new();
    let shared: Arc<[u8]> = Arc::from([1_u8, 2]);
    {
        let mut state = mailbox.state.lock();
        recycle_outbound_frame(&mut state, Arc::clone(&shared));
        assert_eq!(Arc::strong_count(&shared), 1);
        assert!(state.recycled_frames.is_empty());
        recycle_outbound_frame(
            &mut state,
            OutboundFrame::Grouped {
                encoded: Vec::with_capacity(MAX_RECYCLED_FRAME_CAPACITY + 1),
                frames: (0..=MAX_RECYCLED_FRAME_BUFFERS)
                    .map(|_| Vec::with_capacity(16).into())
                    .collect(),
            },
        );
        assert_eq!(state.recycled_frames.len(), MAX_RECYCLED_FRAME_BUFFERS);
        assert_eq!(state.recycled_capacity, MAX_RECYCLED_FRAME_BUFFERS * 16);
    }
    let mut bounded = OutboundState::default();
    recycle_outbound_frame(
        &mut bounded,
        OutboundFrame::Grouped {
            encoded: Vec::with_capacity(MAX_RECYCLED_FRAME_CAPACITY / 2),
            frames: vec![
                Vec::with_capacity(MAX_RECYCLED_FRAME_CAPACITY / 2).into(),
                Vec::with_capacity(MAX_RECYCLED_FRAME_CAPACITY / 2).into(),
            ],
        },
    );
    assert_eq!(bounded.recycled_frames.len(), 2);
    assert_eq!(bounded.recycled_capacity, MAX_RECYCLED_FRAME_CAPACITY);
    mailbox.close();
    let mut state = mailbox.state.lock();
    recycle_outbound_frame(
        &mut state,
        OutboundFrame::Grouped {
            encoded: Vec::with_capacity(16),
            frames: vec![Vec::with_capacity(16).into()],
        },
    );
    assert!(state.recycled_frames.is_empty());
    assert_eq!(state.recycled_capacity, 0);
}

#[test]
fn quiet_group_retains_its_children_without_an_intermediate_encoding() {
    let mailbox = OutboundMailbox::new();
    let buffer = Vec::with_capacity(4096);
    let allocation = buffer.as_ptr();
    mailbox.recycle_frame(buffer);
    let child = zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync).expect("child");
    let shared: Arc<[u8]> = Arc::from(child.clone());
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_control_group(vec![child.into(), Arc::clone(&shared).into()]));
    {
        let mut state = mailbox.state.lock();
        let Some(OutboundFrame::DeferredGrouped {
            sequence,
            encoded_len,
            frames,
        }) = state.reliable.front()
        else {
            panic!("quiet group was encoded before collection")
        };
        let mut encoded = Vec::new();
        zz_protocol::encode_batch_frames_into(*sequence, frames, &mut encoded).expect("wire");
        assert_eq!(*encoded_len, encoded.len());
        assert_eq!(state.queued_bytes, encoded.len());
        assert_eq!(state.reliable.len(), 1);
        assert_eq!(state.recycled_frames.len(), 1);
        let OutboundFrame::Shared(retained) = &frames[1] else {
            panic!("shared child copied")
        };
        assert!(Arc::ptr_eq(retained, &shared));
        assert!(pop_ready_frame(&mut state).is_none());
    }
    mailbox.release_control_query();
    let mut pending = Vec::new();
    assert!(mailbox.recv_batch(&mut pending, MAX_OUTBOUND_BYTES));
    let OutboundFrame::Grouped { encoded, frames } = &pending[0] else {
        panic!("group missing")
    };
    assert_eq!(encoded.as_ptr(), allocation);
    let OutboundFrame::Shared(retained) = &frames[1] else {
        panic!("shared child copied on flush")
    };
    assert!(Arc::ptr_eq(retained, &shared));
    let ProtocolMessage::Batch(batch) = zz_protocol::decode_protocol_frame(encoded).expect("batch")
    else {
        panic!("batch missing")
    };
    assert_eq!(
        batch.messages().expect("flat group"),
        [ProtocolMessage::TreeSync, ProtocolMessage::TreeSync]
    );
    assert_eq!(Arc::strong_count(&shared), 2);
    assert!(mailbox.state.lock().recycled_frames.is_empty());
    mailbox.finish_batch(&mut pending);
    assert_eq!(Arc::strong_count(&shared), 1);
    assert_eq!(mailbox.state.lock().recycled_frames.len(), 2);
}

#[test]
fn buffered_groups_and_direct_drains_keep_their_original_wire() {
    let child = zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync).expect("child");
    let mailbox = OutboundMailbox::buffered();
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_control_group(vec![child.clone().into()]));
    assert!(matches!(
        mailbox.state.lock().reliable.front(),
        Some(OutboundFrame::Grouped { .. })
    ));
    let mut output = Vec::new();
    mailbox.drain_reliable_into(&mut output);
    let ProtocolMessage::Batch(batch) =
        zz_protocol::decode_protocol_frame(&output).expect("buffered group")
    else {
        panic!("batch missing")
    };
    assert_eq!(
        batch.messages().expect("flat buffered group"),
        [ProtocolMessage::TreeSync]
    );

    let mailbox = OutboundMailbox::new();
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_control_group(vec![child.clone().into()]));
    let sequence = match mailbox.state.lock().reliable.front() {
        Some(OutboundFrame::DeferredGrouped { sequence, .. }) => *sequence,
        _ => panic!("group was not deferred"),
    };
    let expected =
        zz_protocol::encode_protocol_message(&ProtocolMessage::Batch(zz_protocol::Batch {
            sequence,
            frames: vec![child.clone()],
        }))
        .expect("expected drain");
    let mut output = Vec::new();
    mailbox.drain_reliable_into(&mut output);
    assert_eq!(output, expected);
    assert_eq!(mailbox.state.lock().queued_bytes, 0);
    assert_eq!(mailbox.state.lock().recycled_frames.len(), 2);

    assert!(mailbox.enqueue_control_group(vec![child.into()]));
    let messages = tests::take_reliable_messages(&mailbox);
    assert!(matches!(messages.as_slice(), [ProtocolMessage::Batch(_)]));
    assert_eq!(mailbox.state.lock().queued_bytes, 0);
}

#[test]
fn deferred_groups_keep_encoded_byte_and_message_admission_limits() {
    let child = zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync).expect("child");
    let encoded =
        zz_protocol::encode_protocol_message(&ProtocolMessage::Batch(zz_protocol::Batch {
            sequence: 128,
            frames: vec![child.clone(), child.clone()],
        }))
        .expect("group");
    for (queued_bytes, reliable_count, admitted) in [
        (
            MAX_OUTBOUND_BYTES - encoded.len(),
            MAX_RELIABLE_MESSAGES - 1,
            true,
        ),
        (MAX_OUTBOUND_BYTES - encoded.len() + 1, 0, false),
        (0, MAX_RELIABLE_MESSAGES, false),
    ] {
        for deferred in [false, true] {
            let mailbox = OutboundMailbox::new();
            assert!(mailbox.collect_control_query());
            {
                let mut state = mailbox.state.lock();
                state.queued_bytes = queued_bytes;
                state
                    .reliable
                    .extend((0..reliable_count).map(|_| child.clone().into()));
            }
            let frame = if deferred {
                OutboundFrame::DeferredGrouped {
                    sequence: 128,
                    encoded_len: zz_protocol::batch_frames_encoded_len(128, &[&child, &child])
                        .expect("length"),
                    frames: vec![child.clone().into(), child.clone().into()],
                }
            } else {
                OutboundFrame::Grouped {
                    encoded: encoded.clone(),
                    frames: vec![child.clone().into(), child.clone().into()],
                }
            };
            assert_eq!(mailbox.enqueue_encoded_reliable(frame), admitted);
            let mut state = mailbox.state.lock();
            if admitted {
                assert_eq!(state.queued_bytes, queued_bytes + encoded.len());
                assert_eq!(state.reliable.len(), reliable_count + 1);
                assert!(!state.closed);
            } else {
                assert!(state.closed);
                assert_eq!(state.reliable.len(), 1);
                let reason = pop_ready_frame(&mut state).expect("overflow reason");
                assert!(
                    matches!(zz_protocol::decode_protocol_frame(&reason).expect("reason"),
                    ProtocolMessage::Event(Event { payload: EventPayload::ControlExit { reason }, .. }) if reason == "too far behind")
                );
            }
        }
    }
}

#[test]
fn quiet_control_query_sends_one_flat_completion_batch() {
    let shared = Arc::new(Shared::new(50));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .inner
        .lock()
        .client_entry(client)
        .kind
        .replace(ClientKind::Control);
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut ExecutionContext::default(),
        compact_exec_request(vec![CommandInvocation::new(
            "display-message",
            ["-p", "quiet result"],
        )]),
        &mailbox,
    );
    let messages = tests::take_reliable_messages(&mailbox);
    if !*hook_events::READONLY_SKIP {
        assert_eq!(messages.len(), 2);
        return;
    }
    let [ProtocolMessage::Batch(batch)] = messages.as_slice() else {
        panic!("expected one completion batch: {messages:?}")
    };
    assert!(matches!(
        batch.messages().expect("flat completion").as_slice(),
        [
            ProtocolMessage::Event(Event {
                payload: EventPayload::ControlCommandStarted { request_id: 1, .. },
                ..
            }),
            ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }),
            ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                outcome: zz_protocol::ExecOutcome::Ran,
                ..
            }),
        ] if output.as_bytes() == b"quiet result"
    ));
}

#[test]
fn quiet_control_release_cannot_flush_an_attach_collector() {
    let shared = Arc::new(Shared::new(51));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .client_writers
        .lock()
        .insert(client, Arc::clone(&mailbox));
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_reliable_with_wakeup(&ProtocolMessage::TreeSync, false));
    assert!(mailbox.enqueue_control_group(vec![
            zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync)
                .expect("deferred child")
                .into(),
        ]));
    assert!(matches!(
        mailbox.state.lock().reliable.back(),
        Some(OutboundFrame::DeferredGrouped { .. })
    ));
    mailbox.hold_terminals();
    mailbox.release_terminals();
    {
        let mut state = mailbox.state.lock();
        assert!(state.ctrl_collecting == ControlCollection::Quiet);
        assert!(pop_ready_frame(&mut state).is_none());
    }
    mailbox.collect_control_attach();
    assert!(!mailbox.collect_control_query());
    for index in 0..MAX_RELIABLE_MESSAGES / 2 {
        let admitted = if index % 2 == 0 {
            mailbox.enqueue_reliable(&ProtocolMessage::TreeSync)
        } else {
            mailbox.enqueue_control_group(vec![
                zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync)
                    .expect("encode")
                    .into(),
            ])
        };
        assert!(admitted);
    }
    mailbox.hold_terminals();
    mailbox.release_terminals();
    shared.wake_control_queue(client, ClientKind::Control);
    mailbox.release_control_query();
    {
        let mut state = mailbox.state.lock();
        assert!(state.ctrl_collecting == ControlCollection::Attach);
        assert!(pop_ready_frame(&mut state).is_none());
    }
    assert!(mailbox.flush_control_batch(false));
    let messages = tests::take_reliable_messages(&mailbox);
    let [ProtocolMessage::Batch(batch)] = messages.as_slice() else {
        panic!("attach collection was split: {messages:?}")
    };
    let children = batch.messages().expect("flat attach collection");
    assert_eq!(children.len(), MAX_RELIABLE_MESSAGES / 2 + 2);
    assert!(
        children
            .iter()
            .all(|message| *message == ProtocolMessage::TreeSync)
    );
}

#[test]
fn quiet_control_close_and_overflow_release_the_held_frames() {
    let mailbox = OutboundMailbox::new();
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_reliable_with_wakeup(&ProtocolMessage::TreeSync, false));
    assert!(mailbox.enqueue_control_group(vec![
            zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync)
                .expect("deferred child")
                .into(),
        ]));
    mailbox.close_after_flush();
    let encoded = mailbox.recv().expect("closed collection drains");
    let ProtocolMessage::Batch(batch) =
        zz_protocol::decode_protocol_frame(&encoded).expect("decode")
    else {
        panic!("expected final collection")
    };
    assert_eq!(
        batch.messages().expect("flat group"),
        [ProtocolMessage::TreeSync, ProtocolMessage::TreeSync]
    );
    assert!(mailbox.recv().is_none());

    let mailbox = OutboundMailbox::new();
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_control_group(vec![
            zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync)
                .expect("overflow child")
                .into(),
        ]));
    for _ in 1..MAX_RELIABLE_MESSAGES {
        assert!(mailbox.enqueue_reliable_with_wakeup(&ProtocolMessage::TreeSync, false));
    }
    assert!(!mailbox.enqueue_reliable(&ProtocolMessage::TreeSync));
    let encoded = mailbox.recv().expect("overflow collection drains");
    assert!(matches!(
        zz_protocol::decode_protocol_frame(&encoded).expect("decode"),
        ProtocolMessage::Event(Event {
            payload: EventPayload::ControlExit { reason },
            ..
        }) if reason == "too far behind"
    ));
    assert!(mailbox.recv().is_none());
}

#[test]
fn compact_callback_guard_preserves_raw_output() {
    let shared = Arc::new(Shared::new(38));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared.publish_control_command_guard(
        Some((client, 1)),
        RawText::from_bytes(vec![0xff, b'\n']),
        false,
        false,
    );
    let messages = reliable_children(&mailbox);
    assert!(
        matches!(messages.as_slice(), [ProtocolMessage::Event(Event { payload: EventPayload::ControlCommandGuardRaw { output, flags: 1, .. }, .. })] if output.as_bytes() == [0xff, b'\n'])
    );
}

#[test]
fn control_query_wakeup_excludes_hooks_and_pending_events() {
    let shared = Shared::new(47);
    let mut context = ExecutionContext::default();
    let command = |name: &str, args: &[&str]| PreparedCommand {
        invocation: CommandInvocation::new(name, args.iter().copied()),
        canonical_name: Some(name.to_owned()),
        alias_matched: false,
        result: PreparedCommandResult::Ready,
    };
    let query = command("display-message", &["-p", "#{session_name}"]);
    let mut inner = shared.inner.lock();
    for name in [
        "has-session",
        "list-buffers",
        "list-clients",
        "list-commands",
        "list-keys",
        "list-panes",
        "list-sessions",
        "list-windows",
        "show-buffer",
        "show-environment",
        "show-hooks",
        "show-messages",
        "show-options",
        "show-prompt-history",
        "show-window-options",
        "start-server",
    ] {
        assert_eq!(
            ctrl::control_query_can_defer_wakeup(&inner, &context, &command(name, &[])),
            *hook_events::READONLY_SKIP,
            "{name}"
        );
    }
    assert_eq!(
        ctrl::control_query_can_defer_wakeup(&inner, &context, &query),
        *hook_events::READONLY_SKIP
    );
    assert_eq!(
        ctrl::control_query_can_defer_wakeup(&inner, &context, &command("capture-pane", &["-p"]),),
        *hook_events::READONLY_SKIP
    );
    for (name, args) in [
        ("display-message", &["-I"][..]),
        ("display-message", &["-d", "100", "x"][..]),
        ("capture-pane", &[][..]),
        ("wait-for", &["release"][..]),
        ("run-shell", &["sleep 30"][..]),
        ("agent-send", &["--wait", "x"][..]),
        ("capture-browser", &[][..]),
    ] {
        assert!(!ctrl::control_query_can_defer_wakeup(
            &inner,
            &context,
            &command(name, args),
        ));
    }
    for hook in ["after-display-message", "command-error"] {
        inner
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-hook", ["-g", hook, "wait-for release"]),
            )
            .expect("set blocking hook");
        assert!(!ctrl::control_query_can_defer_wakeup(
            &inner, &context, &query,
        ));
        inner
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new("set-hook", ["-gu", hook]),
            )
            .expect("unset hook");
    }
    inner.deferred_event_hooks.push(PendingHookEvent {
        name: "session-renamed",
        context: context.clone(),
        variables: BTreeMap::new(),
        exclude_client: None,
    });
    assert!(!ctrl::control_query_can_defer_wakeup(
        &inner, &context, &query,
    ));
}

#[cfg(unix)]
#[test]
fn control_query_started_is_visible_before_a_blocking_after_hook_releases() {
    use std::os::unix::net::UnixStream;

    let shared = Arc::new(Shared::new(48));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .client_writers
        .lock()
        .insert(client, Arc::clone(&mailbox));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new(
                "set-hook",
                ["-g", "after-display-message", "wait-for ctrl-hook-release"],
            ),
        )
        .expect("blocking after hook");
    let (mut reader, mut server) = UnixStream::pair().expect("pair");
    reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read deadline");
    let writer = {
        let mailbox = Arc::clone(&mailbox);
        thread::spawn(move || write_outbound(&mut server, &mailbox, &Weak::new(), client))
    };
    let (finished, completion) = crossbeam_channel::bounded(1);
    let worker = {
        let shared = Arc::clone(&shared);
        let mailbox = Arc::clone(&mailbox);
        thread::spawn(move || {
            shared.execute_compact_request(
                client,
                ClientKind::Control,
                &mut context,
                compact_exec_request(vec![CommandInvocation::new(
                    "display-message",
                    ["-p", "query result"],
                )]),
                &mailbox,
            );
            let _ = finished.send(());
        })
    };
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut reader).expect("visible Started"),
        ProtocolMessage::Event(Event {
            payload: EventPayload::ControlCommandStarted { request_id: 1, .. },
            ..
        })
    ));
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut reader).expect("after hook parked"),
        ProtocolMessage::CommandQueueParked { request_id: 1 }
    ));
    assert!(matches!(
        completion.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
    shared.signal_wait_channel("ctrl-hook-release");
    completion
        .recv_timeout(Duration::from_secs(2))
        .expect("query completed after release");
    worker.join().expect("query worker");
    mailbox.close_after_flush();
    writer.join().expect("socket writer");
}

#[test]
fn a_late_gui_hook_wakes_quiet_started_before_its_reply() {
    let shared = Arc::new(Shared::new(49));
    let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .client_writers
        .lock()
        .insert(client, Arc::clone(&mailbox));
    let gui_mailbox = OutboundMailbox::new();
    let (gui_client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        None,
        None,
        Arc::clone(&gui_mailbox),
    );
    let (session, pane, mut context) = {
        let mut inner = shared.inner.lock();
        let (session, _, pane) = inner
            .engine
            .state
            .create_session("late-gui-hook")
            .expect("model session");
        inner.engine.state.pane_mut(pane).expect("pane").kind = PaneKind::Browser(
            zz_protocol::BrowserDescriptor::single("about:blank".to_owned(), "default".to_owned()),
        );
        let context = ExecutionContext::for_pane(&inner.engine.state, pane).expect("context");
        (session, pane, context)
    };
    shared.attach(gui_client, session).expect("GUI attach");
    reliable_children(&gui_mailbox);
    reliable_children(&mailbox);
    let query = PreparedCommand {
        invocation: CommandInvocation::new("display-message", ["-p", "query result"]),
        canonical_name: Some("display-message".to_owned()),
        alias_matched: false,
        result: PreparedCommandResult::Ready,
    };
    assert!(ctrl::control_query_can_defer_wakeup(
        &shared.inner.lock(),
        &context,
        &query,
    ));
    let (armed, waiting) = crossbeam_channel::bounded(1);
    let (observable, visible) = crossbeam_channel::bounded(1);
    let observer = {
        let mailbox = Arc::clone(&mailbox);
        thread::spawn(move || {
            let mut state = mailbox.state.lock();
            let _ = armed.send(());
            let frame = if mailbox
                .ready
                .wait_for(&mut state, Duration::from_secs(2))
                .timed_out()
            {
                None
            } else {
                pop_ready_frame(&mut state)
            };
            let _ = observable.send(frame);
        })
    };
    waiting
        .recv_timeout(Duration::from_secs(2))
        .expect("observer waiting");
    assert!(mailbox.collect_control_query());
    assert!(mailbox.enqueue_reliable_with_wakeup(
        &Shared::event(EventPayload::ControlCommandStarted {
            request_id: 1,
            flags: 1,
            canonical_name: query.canonical_name.clone(),
            guard: true,
        }),
        false,
    ));
    let hook = format!("capture-browser -t {pane} -o /tmp/zz-ctrl-late-hook.png");
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("set-hook", ["-g", "after-display-message", &hook]),
        )
        .expect("install hook after eligibility");
    let worker = {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            shared.run_command_hook(
                client,
                ClientKind::Control,
                &context,
                &query.invocation,
                "after-display-message",
                None,
            )
        })
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let request_id = loop {
        let request =
            reliable_children(&gui_mailbox)
                .into_iter()
                .find_map(|message| match message {
                    ProtocolMessage::Event(Event {
                        payload:
                            EventPayload::BrowserCommand {
                                command: BrowserCommand::Screenshot { request_id, .. },
                                ..
                            },
                        ..
                    }) => Some(request_id),
                    _ => None,
                });
        if request.is_some() || Instant::now() >= deadline {
            break request;
        }
        thread::yield_now();
    };
    let started = visible.recv_timeout(Duration::from_secs(2)).ok().flatten();
    let pending = request_id.is_some_and(|request_id| {
        shared
            .inner
            .lock()
            .pending_gui_requests
            .contains_key(&request_id)
    });
    if let Some(request_id) = request_id {
        shared.complete_gui_request(
            gui_client,
            GuiResponse::Success {
                request_id,
                output: "saved".to_owned(),
            },
        );
    } else {
        shared.fail_gui_requests_for(gui_client);
    }
    worker.join().expect("hook worker");
    observer.join().expect("Started observer");
    assert!(pending, "hook was waiting for its GUI reply");
    let Some(ProtocolMessage::Batch(batch)) =
        started.map(|frame| zz_protocol::decode_protocol_frame(&frame).expect("decode Started"))
    else {
        panic!("late hook did not release the quiet batch")
    };
    assert!(matches!(
        batch.messages().expect("flat Started").as_slice(),
        [ProtocolMessage::Event(Event {
            payload: EventPayload::ControlCommandStarted { request_id: 1, .. },
            ..
        })]
    ));
}

#[test]
fn compact_resize_validates_generation_before_skipping_its_exact_report() {
    let shared = Arc::new(Shared::new(59));
    let (client, _) = compact_registered(&shared, zz_protocol::Subscriptions::terminal());
    let pane = PaneId(1);
    let geometry = TerminalGeometry {
        columns: 97,
        rows: 31,
        cell_width_px: 8,
        cell_height_px: 16,
    };
    let mut inner = shared.inner.lock();
    inner
        .terminal_geometries
        .entry(pane)
        .or_default()
        .insert(client, geometry);
    assert!(inner.client(client).is_none_or(|c| c.ctrl_layout.is_none()));
    let input = InputMessage::ResizeTerminalV2 {
        pane,
        columns: geometry.columns,
        rows: geometry.rows,
        cell_width_px: geometry.cell_width_px,
        cell_height_px: geometry.cell_height_px,
        layout_generation: 1,
    };
    let normalized = ctrl::normalize_resize(&mut inner, client, input);
    assert_eq!(inner.clients[&client].ctrl_layout.as_ref().unwrap().1, 1);
    if *attach::ATTACH_PRESIZE {
        assert!(normalized.is_none());
    } else {
        assert!(matches!(
            normalized,
            Some(InputMessage::ResizeTerminal { .. })
        ));
    }
    assert_eq!(inner.terminal_geometries[&pane][&client], geometry);
    assert!(matches!(
        ctrl::normalize_resize(
            &mut inner,
            client,
            InputMessage::ResizeTerminal {
                pane,
                columns: geometry.columns,
                rows: geometry.rows,
                cell_width_px: geometry.cell_width_px,
                cell_height_px: geometry.cell_height_px,
            },
        ),
        Some(InputMessage::ResizeTerminal { .. })
    ));
}

#[test]
fn compact_resize_forwards_each_changed_geometry_field_and_unknown_reports() {
    let shared = Arc::new(Shared::new(61));
    let (client, _) = compact_registered(&shared, zz_protocol::Subscriptions::terminal());
    let pane = PaneId(1);
    let geometry = TerminalGeometry {
        columns: 97,
        rows: 31,
        cell_width_px: 8,
        cell_height_px: 16,
    };
    let mut inner = shared.inner.lock();
    inner
        .terminal_geometries
        .entry(pane)
        .or_default()
        .insert(client, geometry);
    for (columns, rows, cell_width_px, cell_height_px) in [
        (98, 31, 8, 16),
        (97, 32, 8, 16),
        (97, 31, 9, 16),
        (97, 31, 8, 17),
    ] {
        assert!(matches!(
            ctrl::normalize_resize(
                &mut inner,
                client,
                InputMessage::ResizeTerminalV2 {
                    pane,
                    columns,
                    rows,
                    cell_width_px,
                    cell_height_px,
                    layout_generation: 1,
                },
            ),
            Some(InputMessage::ResizeTerminal {
                columns: next_columns,
                rows: next_rows,
                cell_width_px: next_width,
                cell_height_px: next_height,
                ..
            }) if (next_columns, next_rows, next_width, next_height)
                == (columns, rows, cell_width_px, cell_height_px)
        ));
        assert_eq!(inner.terminal_geometries[&pane][&client], geometry);
    }
    inner.terminal_geometries.remove(&pane);
    assert!(matches!(
        ctrl::normalize_resize(
            &mut inner,
            client,
            InputMessage::ResizeTerminalV2 {
                pane,
                columns: geometry.columns,
                rows: geometry.rows,
                cell_width_px: geometry.cell_width_px,
                cell_height_px: geometry.cell_height_px,
                layout_generation: 1,
            },
        ),
        Some(InputMessage::ResizeTerminal { .. })
    ));
}

#[test]
fn legacy_resize_accepts_v2_reports_without_a_compact_generation() {
    let shared = Arc::new(Shared::new(43));
    let mailbox = OutboundMailbox::new();
    let (client, _) = shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
    let mut inner = shared.inner.lock();
    inner
        .terminal_geometries
        .entry(PaneId(1))
        .or_default()
        .insert(
            client,
            TerminalGeometry {
                columns: 97,
                rows: 31,
                cell_width_px: 8,
                cell_height_px: 16,
            },
        );
    assert!(matches!(
        ctrl::normalize_resize(
            &mut inner,
            client,
            InputMessage::ClientTerminalSizeV2 {
                columns: 97,
                rows: 31,
                layout_generation: 0,
            },
        ),
        Some(InputMessage::ClientTerminalSize {
            columns: 97,
            rows: 31
        })
    ));
    assert!(matches!(
        ctrl::normalize_resize(
            &mut inner,
            client,
            InputMessage::ResizeTerminalV2 {
                pane: PaneId(1),
                columns: 97,
                rows: 31,
                cell_width_px: 8,
                cell_height_px: 16,
                layout_generation: 0,
            },
        ),
        Some(InputMessage::ResizeTerminal {
            pane: PaneId(1),
            columns: 97,
            rows: 31,
            cell_width_px: 8,
            cell_height_px: 16,
        })
    ));
    assert!(inner.client(client).is_none_or(|c| c.ctrl_layout.is_none()));
}

#[test]
fn stale_layout_size_report_is_dropped_before_geometry_changes() {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-resize", "sleep 30"]),
        )
        .expect("session");
    let mailbox = OutboundMailbox::new();
    let (client, _) = shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
    shared
        .attach_target(client, ClientKind::Interactive, &mut context, "ctrl-resize")
        .expect("attach");
    let pane = context.pane.expect("pane");
    {
        let mut inner = shared.inner.lock();
        inner
            .client_entry(client)
            .ctrl_subscriptions
            .replace(zz_protocol::Subscriptions::terminal());
        ctrl::normalize_resize(
            &mut inner,
            client,
            InputMessage::ClientTerminalSizeV2 {
                columns: 80,
                rows: 24,
                layout_generation: 1,
            },
        )
        .expect("initial generation");
    }
    shared
        .execute(
            client,
            ClientKind::Interactive,
            &mut context,
            &CommandInvocation::new("split-window", ["-d", "sleep 30"]),
        )
        .expect("split");
    compact_command(
        &shared,
        &mut context,
        "resize-pane",
        &["-Z", "-t", &pane.to_string()],
    );
    shared.compact_tree_messages(client, true);
    let generation = shared.inner.lock().clients[&client]
        .ctrl_layout
        .as_ref()
        .unwrap()
        .1;
    compact_command(
        &shared,
        &mut context,
        "resize-pane",
        &["-Z", "-t", &pane.to_string()],
    );
    let geometry = shared.inner.lock().engine.pane_geometry(pane);
    shared
        .input(
            client,
            ClientKind::Interactive,
            &mut context,
            InputMessage::ResizeTerminalV2 {
                pane,
                columns: 1,
                rows: 1,
                cell_width_px: 8,
                cell_height_px: 16,
                layout_generation: generation,
            },
        )
        .expect("stale report");
    assert_eq!(shared.inner.lock().engine.pane_geometry(pane), geometry);
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("kill-session", ["-t", "ctrl-resize"]),
        )
        .expect("cleanup");
}

fn resize_application_fixture() -> (Arc<Shared>, ClientId, PaneId, ExecutionContext, u64) {
    let shared = Arc::new(Shared::new(62));
    let (client, _) = compact_registered(&shared, zz_protocol::Subscriptions::terminal());
    let mut context = ExecutionContext::default();
    let session = {
        let mut inner = shared.inner.lock();
        for command in [
            CommandInvocation::new("new-session", ["-d", "-s", "resize-admission"]),
            CommandInvocation::new("split-window", ["-h"]),
        ] {
            inner
                .engine
                .execute(&mut context, &command)
                .expect("model layout");
        }
        let pane = context.pane.expect("active pane");
        inner.terminals_mut().insert(
            pane,
            Arc::new(TerminalSession::spawn_output_view(
                "resize admission".to_owned(),
                String::new(),
            )),
        );
        context.session.expect("session")
    };
    shared
        .attach(client, session)
        .expect("attach model terminal");
    let pane = context.pane.expect("pane");
    {
        let mut inner = shared.inner.lock();
        let (columns, rows) = inner.engine.pane_geometry(pane).expect("pane geometry");
        inner.terminal_geometries.entry(pane).or_default().insert(
            client,
            TerminalGeometry {
                columns,
                rows,
                cell_width_px: 8,
                cell_height_px: 16,
            },
        );
        inner.client_entry(client).size.replace((80, 24));
    }
    context.no_hooks = true;
    shared.compact_tree_messages(client, true);
    let generation = shared.inner.lock().clients[&client]
        .ctrl_layout
        .as_ref()
        .unwrap()
        .1;
    (shared, client, pane, context, generation)
}

#[test]
fn terminal_resize_is_revalidated_after_initial_admission() {
    let (shared, client, pane, _, generation) = resize_application_fixture();
    let report = TerminalGeometry {
        columns: 1,
        rows: 1,
        cell_width_px: 9,
        cell_height_px: 17,
    };
    let (stored, laid_out, terminal, viewport) = {
        let mut inner = shared.inner.lock();
        assert!(
            ctrl::normalize_resize(
                &mut inner,
                client,
                InputMessage::ResizeTerminalV2 {
                    pane,
                    columns: report.columns,
                    rows: report.rows,
                    cell_width_px: report.cell_width_px,
                    cell_height_px: report.cell_height_px,
                    layout_generation: generation,
                },
            )
            .is_some()
        );
        inner
            .engine
            .state
            .toggle_zoom(pane)
            .expect("layout changed after admission");
        let terminal = Arc::clone(&inner.terminals[&pane]);
        (
            inner.terminal_geometries[&pane][&client],
            inner.engine.pane_geometry(pane),
            Arc::clone(&terminal),
            terminal.fresh_viewport(),
        )
    };
    assert!(
        !shared
            .apply_terminal_size_report(client, pane, report, Some(generation))
            .expect("stale application")
    );
    {
        let inner = shared.inner.lock();
        assert_eq!(inner.terminal_geometries[&pane][&client], stored);
        assert_eq!(inner.engine.pane_geometry(pane), laid_out);
    }
    let current = terminal.fresh_viewport();
    assert_eq!(
        (current.columns, current.rows),
        (viewport.columns, viewport.rows)
    );
    let generation = shared.inner.lock().clients[&client]
        .ctrl_layout
        .as_ref()
        .unwrap()
        .1;
    let current = TerminalGeometry {
        columns: 82,
        rows: 26,
        ..report
    };
    assert!(
        shared
            .apply_terminal_size_report(client, pane, current, Some(generation))
            .expect("current application")
    );
    assert_eq!(
        shared.inner.lock().terminal_geometries[&pane][&client],
        current
    );
    shared
        .inner
        .lock()
        .client_mut(client)
        .and_then(|c| c.ctrl_subscriptions.take());
    assert!(
        shared
            .apply_terminal_size_report(client, pane, report, Some(0))
            .expect("legacy application")
    );
    assert_eq!(
        shared.inner.lock().terminal_geometries[&pane][&client],
        report
    );
}

#[test]
fn client_resize_is_revalidated_after_initial_admission() {
    let (shared, client, pane, context, generation) = resize_application_fixture();
    let (stored, laid_out) = {
        let mut inner = shared.inner.lock();
        assert!(
            ctrl::normalize_resize(
                &mut inner,
                client,
                InputMessage::ClientTerminalSizeV2 {
                    columns: 1,
                    rows: 1,
                    layout_generation: generation,
                },
            )
            .is_some()
        );
        inner
            .engine
            .state
            .toggle_zoom(pane)
            .expect("layout changed after admission");
        (
            inner.clients[&client].size.unwrap(),
            inner.engine.pane_geometry(pane),
        )
    };
    assert!(!shared.apply_client_size_report(
        client,
        ClientKind::Interactive,
        &context,
        1,
        1,
        Some(generation),
    ));
    {
        let inner = shared.inner.lock();
        assert_eq!(inner.clients[&client].size.unwrap(), stored);
        assert_eq!(inner.engine.pane_geometry(pane), laid_out);
    }
    let generation = shared.inner.lock().clients[&client]
        .ctrl_layout
        .as_ref()
        .unwrap()
        .1;
    assert!(shared.apply_client_size_report(
        client,
        ClientKind::Interactive,
        &context,
        81,
        25,
        Some(generation),
    ));
    assert_eq!(shared.inner.lock().clients[&client].size.unwrap(), (81, 25));
    shared
        .inner
        .lock()
        .client_mut(client)
        .and_then(|c| c.ctrl_subscriptions.take());
    assert!(shared.apply_client_size_report(
        client,
        ClientKind::Interactive,
        &context,
        1,
        1,
        Some(0)
    ));
    assert_eq!(shared.inner.lock().clients[&client].size.unwrap(), (1, 1));
}

fn compact_hello(kind: ClientKind) -> zz_protocol::Hello {
    zz_protocol::Hello::from_client(ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: ClientInstanceId(919),
        kind,
        device_name: None,
        capabilities: vec![
            zz_protocol::PANE_FRAME_CAPABILITY.to_owned(),
            ClientHello::CLIENT_TERMINAL_CAPABILITY.to_owned(),
        ],
        color_scheme: None,
        origin: None,
        working_directory: None,
        environment: Vec::new(),
        process_id: std::process::id(),
    })
}

fn compact_command(
    shared: &Arc<Shared>,
    context: &mut ExecutionContext,
    name: &str,
    args: &[&str],
) {
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            context,
            &CommandInvocation::new(name, args.iter().copied()),
        )
        .expect(name);
}

#[test]
fn initializing_status_waits_for_explicit_refresh_while_existing_clients_update() {
    let shared = Arc::new(Shared::new(61));
    let mut context = ExecutionContext::default();
    {
        let mut inner = shared.inner.lock();
        for command in [
            CommandInvocation::new("new-session", ["-d", "-s", "initial-status"]),
            CommandInvocation::new(
                "set-option",
                ["-t", "initial-status", "status-format[0]", "FIRST"],
            ),
        ] {
            inner
                .engine
                .execute(&mut context, &command)
                .expect("model command");
        }
    }
    let session = context.session.expect("session");
    let existing = OutboundMailbox::new();
    let (existing_client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, Arc::clone(&existing));
    let hello = compact_hello(ClientKind::Interactive);
    let (initial_client, _) = shared.register_welcome(&hello).expect("welcome");
    let initial = OutboundMailbox::new();
    shared.subscribe(initial_client, Arc::clone(&initial));
    {
        let mut inner = shared.inner.lock();
        inner
            .attached
            .entry(session)
            .or_default()
            .extend([existing_client, initial_client]);
        inner.client_entry(existing_client).size.replace((80, 24));
        inner.client_entry(initial_client).size.replace((97, 31));
    }
    for label in ["FIRST", "SECOND"] {
        shared
            .inner
            .lock()
            .engine
            .execute(
                &mut context,
                &CommandInvocation::new(
                    "set-option",
                    ["-t", "initial-status", "status-format[0]", label],
                ),
            )
            .expect("status format");
        shared.publish_mux_snapshots_except(true, true, None);
        shared.refresh_status_for_sessions(Some(&BTreeSet::from([session])));
        assert!(
            reliable_children(&existing)
                .iter()
                .any(|message| matches!(message,
            ProtocolMessage::Event(Event { payload: EventPayload::StatusChanged { status }, .. })
                if status.rows == [label]))
        );
        assert!(!reliable_children(&initial).iter().any(|message| matches!(
            message,
            ProtocolMessage::Event(Event {
                payload: EventPayload::StatusChanged { .. },
                ..
            })
        )));
        assert!(shared.read_client(initial_client, |c| {
            c.is_none_or(|c| c.status_rows.is_none())
        }));
    }
    shared.refresh_status_filtered(None, Some(&BTreeSet::from([initial_client])));
    let statuses = reliable_children(&initial)
        .into_iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::StatusChanged { status },
                ..
            }) => Some(status),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].rows, ["SECOND"]);
    assert!(shared.read_client(initial_client, |c| {
        c.is_some_and(|c| c.status_rows.is_some())
    }));
}

#[test]
fn caller_input_flags_keep_packed_escaped_and_false_positive_spellings() {
    for name in ["display-message", "split-window"] {
        for arguments in [vec!["-I"], vec!["-It%7"]] {
            let command = CommandInvocation::new(name, arguments);
            assert_eq!(
                command_stdin_sink(name, &command.args),
                Some(CommandStdinSink::PaneInput)
            );
        }
        for arguments in [vec!["--", "-I"], vec!["-t", "targetI"], vec!["textI"]] {
            let command = CommandInvocation::new(name, arguments);
            assert_eq!(command_stdin_sink(name, &command.args), None);
        }
    }
    let command = CommandInvocation::new("display-message", ["-pI"]);
    assert_eq!(
        command_stdin_sink(&command.name, &command.args),
        Some(CommandStdinSink::PaneInput)
    );
    for line in [
        r"display-message -\111; split-window -\111",
        r#"display-message '-I'; split-window "-I""#,
    ] {
        let parsed = zz_mux::parse_config_with_expansions(
            "<control>",
            line,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert_eq!(parsed.commands.len(), 2);
        for command in parsed.commands {
            assert_eq!(
                command_stdin_sink(&command.name, &command.args),
                Some(CommandStdinSink::PaneInput)
            );
        }
    }
}

#[test]
fn display_alias_absence_preserves_packed_targets_literal_markers_and_errors() {
    let shared = Arc::new(Shared::new(63));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "alias-absence"]),
        )
        .expect("model session");
    let session = context.session.expect("session");
    let pane = context.pane.expect("pane");
    let (client, _) = compact_registered(&shared, zz_protocol::Subscriptions::terminal());
    shared
        .inner
        .lock()
        .attached
        .entry(session)
        .or_default()
        .insert(client);
    for target in ["@", "{active}", "{current}"] {
        let command = CommandInvocation::new(
            "display-message",
            [format!("-pt{target}"), "#{pane_id}".to_owned()],
        );
        let route = shared
            .display_message_client_alias(client, &command)
            .expect("alias route");
        assert!(route.command.is_some());
        let output = shared
            .execute(client, ClientKind::Interactive, &mut context, &command)
            .expect("packed alias");
        assert_eq!(output.output, pane.to_string());
    }
    for text in ["ordinary", "literal @ {active} I"] {
        let command = CommandInvocation::new("display-message", ["-pl", text]);
        assert!(
            shared
                .display_message_client_alias(client, &command)
                .is_none()
        );
        assert_eq!(
            shared
                .execute(client, ClientKind::Interactive, &mut context, &command)
                .expect("literal display")
                .output,
            text
        );
    }
    for text in ["ordinary", "literal @ {active} I"] {
        let error = shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("display-message", ["-pZ", text]),
            )
            .expect_err("unknown option");
        assert!(error.to_string().contains("unknown flag -Z"), "{error}");
    }
}

fn compact_registered(
    shared: &Arc<Shared>,
    subscriptions: zz_protocol::Subscriptions,
) -> (ClientId, Arc<OutboundMailbox>) {
    let mut hello = compact_hello(ClientKind::Interactive);
    hello.subscriptions = subscriptions;
    let (client, _) = shared.register_welcome(&hello).expect("register");
    let mailbox = OutboundMailbox::new();
    shared.subscribe(client, Arc::clone(&mailbox));
    if let Some(client) = shared.inner.lock().client_mut(client) {
        client.ctrl_initializing = false;
    }
    (client, mailbox)
}

fn reliable_children(mailbox: &OutboundMailbox) -> Vec<ProtocolMessage> {
    tests::take_reliable_messages(mailbox)
        .into_iter()
        .flat_map(|message| match message {
            ProtocolMessage::Batch(batch) => batch.messages().expect("children"),
            message => vec![message],
        })
        .collect()
}

fn bounded_reliable_children(mailbox: &OutboundMailbox, bound: usize) -> Vec<ProtocolMessage> {
    let envelopes = tests::take_reliable_messages(mailbox);
    let bytes = envelopes
        .iter()
        .map(|message| {
            zz_protocol::encode_protocol_message(message)
                .expect("encode")
                .len()
        })
        .sum::<usize>();
    eprintln!("CTRL publication total_wire_bytes={bytes} bound={bound}");
    assert!(
        bytes <= bound,
        "publication {bytes} bytes including every batch and view"
    );
    envelopes
        .into_iter()
        .flat_map(|message| match message {
            ProtocolMessage::Batch(batch) => batch.messages().expect("children"),
            message => vec![message],
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn compact_startup_actor_follows_attachment_in_its_initial_batch() {
    use std::os::unix::net::UnixStream;

    let shared = Arc::new(Shared::new(71));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "startup-actor"]),
        )
        .expect("model session");
    *shared.startup_config_causes.lock() = Some(vec![
        "root.conf:1: INTERACTIVE_DIRECT".to_owned(),
        "child.conf:1: INTERACTIVE_NESTED".to_owned(),
    ]);
    let (mut client, server) = UnixStream::pair().expect("socket pair");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read deadline");
    let server_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || handle_connection(server, &server_shared));
    let mut hello = compact_hello(ClientKind::Interactive);
    hello.subscriptions = zz_protocol::Subscriptions::terminal();
    hello.viewport = Some(zz_protocol::ClientViewport {
        columns: 97,
        rows: 31,
        cell_width_px: 8,
        cell_height_px: 16,
    });
    hello.attach = Some(zz_protocol::AttachOperation::Session(
        "startup-actor".to_owned(),
    ));
    zz_protocol::write_protocol_message(&mut client, &ProtocolMessage::Hello(hello))
        .expect("compact hello");
    let welcome = zz_protocol::read_protocol_message(&mut client).expect("welcome");
    let initial = zz_protocol::read_protocol_message(&mut client).expect("initial batch");
    drop(client);
    worker
        .join()
        .expect("connection thread")
        .expect("connection cleanup");
    assert!(matches!(welcome, ProtocolMessage::Welcome(_)));
    let ProtocolMessage::Batch(batch) = &initial else {
        panic!("initial state must be one batch")
    };
    let messages = batch.messages().expect("flat initial batch");
    let viewport = messages
        .iter()
        .find_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload:
                    EventPayload::CommandOutput {
                        viewport: Some(viewport),
                        ..
                    },
                ..
            }) => Some(viewport),
            _ => None,
        })
        .expect("initial startup actor viewport");
    let text = viewport
        .cells
        .iter()
        .map(|cell| viewport.cell_text(*cell))
        .collect::<String>();
    assert_eq!(text.matches("root.conf:1: INTERACTIVE_DIRECT").count(), 1);
    assert_eq!(text.matches("child.conf:1: INTERACTIVE_NESTED").count(), 1);
    assert!(
        text.find("INTERACTIVE_DIRECT").expect("direct row")
            < text.find("INTERACTIVE_NESTED").expect("nested row")
    );
    let attached = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientView(zz_protocol::ClientView {
                        session: Some(_),
                        ..
                    }),
                    ..
                })
            )
        })
        .expect("attached view");
    let output_positions = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::CommandOutput {
                        viewport: Some(_),
                        ..
                    },
                    ..
                })
            )
            .then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(output_positions.len(), 1);
    assert!(attached < output_positions[0]);
    assert!(shared.startup_config_causes.lock().is_none());
}

#[cfg(unix)]
#[test]
fn compact_attach_sends_one_initial_batch_with_every_final_viewport() {
    assert_initial_compact_attach(false);
    assert_initial_compact_attach(true);
}

#[cfg(unix)]
fn assert_initial_compact_attach(commands: bool) {
    use std::os::unix::net::UnixStream;
    let shared = Arc::new(Shared::new(31));
    let mut context = ExecutionContext::default();
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &[
            "-d",
            "-s",
            "ctrl-initial",
            "printf CTRL_INITIAL_MARKER; exec sleep 30",
        ],
    );
    for _ in 0..3 {
        compact_command(
            &shared,
            &mut context,
            "split-window",
            &[
                "-d",
                "-t",
                "ctrl-initial",
                "printf CTRL_INITIAL_MARKER; exec sleep 30",
            ],
        );
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ready = shared.inner.lock().terminals.values().all(|terminal| {
            let viewport = terminal.latest_viewport();
            viewport
                .cells
                .iter()
                .map(|cell| viewport.cell_text(*cell))
                .collect::<String>()
                .contains("CTRL_INITIAL_MARKER")
        });
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "initial pane content did not arrive"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let (mut client, server) = UnixStream::pair().expect("pair");
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let worker_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || handle_connection(server, &worker_shared));
    let mut hello = compact_hello(ClientKind::Interactive);
    hello.subscriptions = zz_protocol::Subscriptions::terminal();
    hello.viewport = Some(zz_protocol::ClientViewport {
        columns: 97,
        rows: 31,
        cell_width_px: 8,
        cell_height_px: 16,
    });
    hello.attach = Some(if commands {
        zz_protocol::AttachOperation::Commands(
            [
                CommandInvocation::new("attach-session", ["-t", "ctrl-initial"]),
                CommandInvocation::new("set-option", ["-t", "ctrl-initial", "status", "2"]),
                CommandInvocation::new(
                    "set-option",
                    [
                        "-t",
                        "ctrl-initial",
                        "status-format[0]",
                        "CTRL_FINAL_#{client_width}x#{client_height}:#{pane_height}",
                    ],
                ),
                CommandInvocation::new(
                    "set-option",
                    ["-t", "ctrl-initial", "status-format[1]", "CTRL_SECOND"],
                ),
            ]
            .into_iter()
            .map(|invocation| PreparedCommand {
                canonical_name: Some(invocation.name.clone()),
                invocation,
                alias_matched: false,
                result: PreparedCommandResult::Ready,
            })
            .collect(),
        )
    } else {
        zz_protocol::AttachOperation::Session("ctrl-initial".to_owned())
    });
    zz_protocol::write_protocol_message(&mut client, &ProtocolMessage::Hello(hello))
        .expect("hello");
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut client).expect("welcome"),
        ProtocolMessage::Welcome(_)
    ));
    let ProtocolMessage::Batch(batch) =
        zz_protocol::read_protocol_message(&mut client).expect("initial batch")
    else {
        panic!("initial state is not a batch")
    };
    let messages = batch.messages().expect("messages");
    let statuses = messages
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::StatusChanged { status },
                ..
            }) => Some(status),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        statuses.len(),
        1,
        "commands={commands}, initial status was rendered before the final command: {statuses:?}"
    );
    if commands {
        let inner = shared.inner.lock();
        let client = inner
            .clients
            .iter()
            .find_map(|(id, client)| client.subscriber.as_ref().map(|_| id))
            .copied()
            .expect("attached client");
        let pane = client_context_pane(&inner, client).expect("active pane");
        let (_, height) = inner.engine.pane_geometry(pane).expect("active geometry");
        assert_eq!(
            statuses[0].rows,
            [
                format!("CTRL_FINAL_97x31:{height}"),
                "CTRL_SECOND".to_owned()
            ]
        );
    }
    assert_eq!(
        messages
            .iter()
            .filter(|message| matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(_),
                    ..
                })
            ))
            .count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientView(zz_protocol::ClientView {
                        session: Some(_),
                        ..
                    }),
                    ..
                })
            ))
            .count(),
        1,
        "{messages:?}"
    );
    let viewports = messages
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TerminalViewport { pane, viewport },
                ..
            }) => Some((*pane, viewport)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        viewports.len(),
        4,
        "initial batch must include all visible pane frames: {messages:?}"
    );
    for (pane, viewport) in viewports {
        let geometry = shared
            .inner
            .lock()
            .engine
            .pane_geometry(pane)
            .expect("geometry");
        assert_eq!((viewport.columns, viewport.rows), geometry);
        assert!(
            viewport
                .cells
                .iter()
                .map(|cell| viewport.cell_text(*cell))
                .collect::<String>()
                .contains("CTRL_INITIAL_MARKER")
        );
    }
    drop(client);
    worker
        .join()
        .expect("connection thread")
        .expect("connection");
    compact_command(
        &shared,
        &mut context,
        "kill-session",
        &["-t", "ctrl-initial"],
    );
}

#[test]
fn forced_attachment_stays_after_an_older_pending_publication() {
    let shared = Arc::new(Shared::new(39));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-order"]),
        )
        .expect("model session");
    let session = context.session.expect("session");
    let (client, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            tree: zz_protocol::TreeSubscription::All,
            ..zz_protocol::Subscriptions::control()
        },
    );
    shared
        .inner
        .lock()
        .client_entry(client)
        .kind
        .replace(ClientKind::Control);
    let publication_lock = shared.snapshot_order.lock();
    let older = shared.compact_tree_messages(client, true);
    {
        let mut inner = shared.inner.lock();
        inner.attached.entry(session).or_default().insert(client);
        inner.client_entry(client).ctrl_attachment.replace(1);
    }
    let (started, ready) = crossbeam_channel::bounded(1);
    let (finished, completion) = crossbeam_channel::bounded(1);
    let force = {
        let shared = Arc::clone(&shared);
        let mailbox = Arc::clone(&mailbox);
        thread::spawn(move || {
            started.send(()).expect("start forced attachment");
            shared.send_compact_state(client, &mailbox, true);
            finished.send(()).expect("finish forced attachment");
        })
    };
    ready
        .recv_timeout(Duration::from_secs(2))
        .expect("forced sender started");
    thread::sleep(Duration::from_millis(20));
    let still_unattached = shared.inner.lock().clients[&client]
        .ctrl_view
        .as_ref()
        .unwrap()
        .session
        .is_none();
    let older = older
        .iter()
        .map(|message| {
            zz_protocol::encode_protocol_message(message)
                .expect("old publication")
                .into()
        })
        .collect();
    assert!(mailbox.enqueue_control_group(older));
    drop(publication_lock);
    completion
        .recv_timeout(Duration::from_secs(2))
        .expect("forced attachment completed");
    force.join().expect("forced attachment");
    let views = reliable_children(&mailbox)
        .into_iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::ClientView(view),
                ..
            }) => Some(view),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        still_unattached,
        "forced attachment advanced the cursor before the older publication queued"
    );
    assert_eq!(views.len(), 2, "{views:?}");
    assert!(views[0].session.is_none());
    assert!(
        views[1..]
            .iter()
            .all(|view| view.session == Some(session) && view.attachment_generation == 1),
        "{views:?}"
    );
}

#[test]
fn compact_subscription_rename_is_bounded_and_empty_diff_sends_nothing() {
    let shared = Arc::new(Shared::new(32));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-tree"]),
        )
        .expect("model session");
    let session = context.session.expect("session");
    let (client, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            tree: zz_protocol::TreeSubscription::Attached,
            status: false,
            options: 0,
            keys: zz_protocol::KeySubscription::None,
            pane_stream: false,
        },
    );
    shared
        .attach_target(client, ClientKind::Interactive, &mut context, "ctrl-tree")
        .expect("attach");
    shared.compact_tree_messages(client, true);
    reliable_children(&mailbox);
    compact_command(
        &shared,
        &mut context,
        "rename-session",
        &["-t", "ctrl-tree", "renamed"],
    );
    shared.publish_compact_trees();
    let messages = bounded_reliable_children(&mailbox, 100);
    let deltas = messages
        .iter()
        .filter_map(|message| match message {
            ProtocolMessage::Event(Event {
                payload: EventPayload::TreeDelta(delta),
                ..
            }) => Some(delta),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(deltas.len(), 1, "{messages:?}");
    assert!(deltas[0].ops.iter().any(|op| matches!(op, zz_protocol::TreeOp::SessionName { session: changed, name } if *changed == session && name == "renamed")));
    let encoded = zz_protocol::encode_protocol_message(&Shared::event(EventPayload::TreeDelta(
        deltas[0].clone(),
    )))
    .expect("encode");
    assert!(encoded.len() <= 100, "rename frame {} bytes", encoded.len());
    shared.publish_compact_trees();
    assert!(reliable_children(&mailbox).is_empty());
    compact_command(&shared, &mut context, "kill-session", &["-t", "renamed"]);
}

#[test]
fn all_subscriber_unattached_session_rename_is_bounded_without_a_view_update() {
    let shared = Arc::new(Shared::new(40));
    let mut context = ExecutionContext::default();
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-own"]),
        )
        .expect("model session");
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "ctrl-other"]),
        )
        .expect("model session");
    let (client, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            tree: zz_protocol::TreeSubscription::All,
            status: false,
            options: 0,
            keys: zz_protocol::KeySubscription::None,
            pane_stream: false,
        },
    );
    shared
        .attach_target(client, ClientKind::Interactive, &mut context, "ctrl-own")
        .expect("attach own session");
    shared.send_compact_state(client, &mailbox, true);
    reliable_children(&mailbox);
    compact_command(
        &shared,
        &mut context,
        "rename-session",
        &["-t", "ctrl-other", "ctrl-else"],
    );
    shared.publish_snapshot_state();
    let messages = bounded_reliable_children(&mailbox, 100);
    assert!(
        matches!(messages.as_slice(), [ProtocolMessage::Event(Event { payload: EventPayload::TreeDelta(delta), .. })] if !delta.ops.is_empty())
    );
    shared.publish_snapshot_state();
    assert!(reliable_children(&mailbox).is_empty());
    compact_command(&shared, &mut context, "kill-session", &["-t", "ctrl-else"]);
    compact_command(&shared, &mut context, "kill-session", &["-t", "ctrl-own"]);
}

#[test]
fn disconnected_control_exec_cannot_detach_the_next_active_client() {
    let shared = Arc::new(Shared::new(41));
    let mut context = ExecutionContext::default();
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "ctrl-cancel", "sleep 30"],
    );
    let (old, old_mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .inner
        .lock()
        .client_entry(old)
        .kind
        .replace(ClientKind::Control);
    shared
        .attach_target(old, ClientKind::Control, &mut context, "ctrl-cancel")
        .expect("attach old control");
    let cancel = Arc::new(AtomicBool::new(false));
    shared
        .command_queue_cancels
        .lock()
        .insert(old, Arc::clone(&cancel));
    let directory = tempfile::Builder::new()
        .prefix("zz-ctrl-cancel-")
        .tempdir_in("/tmp")
        .expect("barrier directory");
    let ready = directory.path().join("ready");
    let release = directory.path().join("release");
    let script = format!(
        "touch {}; while test ! -e {}; do sleep 0.01; done",
        ready.display(),
        release.display()
    );
    let request = compact_exec_request(vec![
        CommandInvocation::new("run-shell", [script]),
        CommandInvocation::new("detach-client", [] as [&str; 0]),
    ]);
    let worker = {
        let shared = Arc::clone(&shared);
        let old_mailbox = Arc::clone(&old_mailbox);
        thread::spawn(move || {
            shared.execute_compact_request(
                old,
                ClientKind::Control,
                &mut context,
                request,
                &old_mailbox,
            );
        })
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "foreground control command did not start"
        );
        thread::sleep(Duration::from_millis(1));
    }
    cancel.store(true, Ordering::Release);
    shared.detach(old);
    shared.unregister(old);
    let (next, next_mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::control());
    shared
        .inner
        .lock()
        .client_entry(next)
        .kind
        .replace(ClientKind::Control);
    let mut next_context = ExecutionContext::default();
    let (session, _) = shared
        .attach_target(next, ClientKind::Control, &mut next_context, "ctrl-cancel")
        .expect("attach next control");
    reliable_children(&next_mailbox);
    fs::write(release, []).expect("release foreground command");
    worker.join().expect("cancelled execution");
    shared.execute_compact_request(
        old,
        ClientKind::Control,
        &mut ExecutionContext::default(),
        compact_exec_request(vec![CommandInvocation::new(
            "detach-client",
            [] as [&str; 0],
        )]),
        &old_mailbox,
    );
    assert_eq!(
        client_attached_session(&shared.inner.lock(), next),
        Some(session)
    );
    assert!(
        !reliable_children(&next_mailbox)
            .iter()
            .any(|message| matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Detached { .. },
                    ..
                })
            ))
    );
    shared.command_queue_cancels.lock().remove(&old);
    compact_command(
        &shared,
        &mut next_context,
        "kill-session",
        &["-t", "ctrl-cancel"],
    );
}

#[test]
fn new_full_subscriber_does_not_hide_pending_key_patch() {
    let shared = Arc::new(Shared::new(33));
    let (first, first_mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::default());
    shared.send_compact_keys(first, &first_mailbox);
    shared.publish_key_tables_if_changed();
    reliable_children(&first_mailbox);
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut ExecutionContext::default(),
            &CommandInvocation::new(
                "bind-key",
                ["-T", "ctrl-test", "x", "display-message", "changed"],
            ),
        )
        .expect("bind");
    let (second, second_mailbox) =
        compact_registered(&shared, zz_protocol::Subscriptions::default());
    shared.send_compact_keys(second, &second_mailbox);
    shared.publish_key_tables_if_changed();
    assert!(reliable_children(&first_mailbox).iter().any(|message| matches!(message, ProtocolMessage::Event(Event { payload: EventPayload::KeyTablesPatched { tables, .. }, .. }) if tables.iter().any(|table| table.name == "ctrl-test"))));
}

#[test]
fn first_full_subscriber_does_not_hide_a_pending_hash_update() {
    let shared = Arc::new(Shared::new(42));
    let (hash_client, hash_mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            keys: zz_protocol::KeySubscription::Hash,
            ..zz_protocol::Subscriptions::control()
        },
    );
    shared.send_compact_keys(hash_client, &hash_mailbox);
    shared.publish_key_tables_if_changed();
    reliable_children(&hash_mailbox);
    let generation = {
        let mut inner = shared.inner.lock();
        inner
            .engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new(
                    "bind-key",
                    ["-T", "ctrl-hash-pending", "x", "display-message", "changed"],
                ),
            )
            .expect("pending bind");
        inner.engine.keys.generation()
    };
    let (full_client, full_mailbox) =
        compact_registered(&shared, zz_protocol::Subscriptions::default());
    shared.send_compact_keys(full_client, &full_mailbox);
    reliable_children(&full_mailbox);
    shared.publish_key_tables_if_changed();
    let messages = bounded_reliable_children(&hash_mailbox, 64);
    assert!(
        matches!(messages.as_slice(), [ProtocolMessage::Event(Event { payload: EventPayload::KeyTablesHashChanged { hash, .. }, .. })] if *hash == generation)
    );
    assert!(reliable_children(&full_mailbox).is_empty());
}

#[test]
fn compact_raw_control_preflights_line_and_resolves_daemon_environment() {
    let shared = Arc::new(Shared::new(34));
    let mut hello = compact_hello(ClientKind::Control);
    hello.subscriptions = zz_protocol::Subscriptions::control();
    let (client, _) = shared.register_welcome(&hello).expect("register");
    let mailbox = OutboundMailbox::new();
    shared.subscribe(client, Arc::clone(&mailbox));
    if let Some(client) = shared.inner.lock().client_mut(client) {
        client.ctrl_initializing = false;
    }
    let mut context = ExecutionContext::default();
    let request = |line: &str| zz_protocol::ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: zz_protocol::ExecFlags::default(),
        client_instance_id: ClientInstanceId(919),
        origin: None,
        working_directory: None,
        tty: None,
        size: None,
        features: 0,
        startup_reentry: None,
        spawned_server_id: None,
        expect_server_id: None,
        process_id: std::process::id(),
        environment: ClientEnvironmentBlob::default(),
        commands: Vec::new(),
        raw_control_line: Some(line.to_owned()),
    };
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        request("set-environment -g CTRL_MUST_NOT_RUN yes ; unknown-ctrl-command"),
        &mailbox,
    );
    assert!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("CTRL_MUST_NOT_RUN")
            .is_none()
    );
    assert!(matches!(
        reliable_children(&mailbox).as_slice(),
        [ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            outcome: zz_protocol::ExecOutcome::Rejected(_),
            ..
        })]
    ));
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        request(r"set-environment -g CTRL_MUST_NOT_RUN yes ; display-message -p \400"),
        &mailbox,
    );
    assert!(matches!(
        reliable_children(&mailbox).as_slice(),
        [ProtocolMessage::ExecExit(zz_protocol::ExecExit {
            outcome: zz_protocol::ExecOutcome::Rejected(ServerError::CommandParse(message)),
            ..
        })] if message == "invalid octal escape"
    ));
    assert!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("CTRL_MUST_NOT_RUN")
            .is_none()
    );
    shared
        .inner
        .lock()
        .engine
        .seed_global_environment([("CTRL_EXPANSION", "server-value"), ("HOME", "/server-home")]);
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        request("display-message -p ~/$CTRL_EXPANSION"),
        &mailbox,
    );
    assert!(matches!(
        mailbox.state.lock().reliable.back(),
        Some(OutboundFrame::Grouped { frames, .. }) if frames.len() == if *hook_events::READONLY_SKIP { 3 } else { 2 }
    ));
    let messages = reliable_children(&mailbox);
    let started = messages.iter().position(|message| matches!(message, ProtocolMessage::Event(Event { payload: EventPayload::ControlCommandStarted { request_id: 1, flags: 1, guard: true, canonical_name: Some(name) }, .. }) if name == "display-message")).unwrap_or_else(|| panic!("command start missing: {messages:?}"));
    let response = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }) if output.as_bytes() == b"/server-home/server-value"
            )
        })
        .expect("response");
    assert!(started < response);
    assert!(matches!(
        messages.last(),
        Some(ProtocolMessage::ExecExit(_))
    ));
    shared
        .inner
        .lock()
        .engine
        .execute(
            &mut context,
            &CommandInvocation::new(
                "set-option",
                [
                    "-s",
                    "command-alias[90]",
                    "ctrlalias=display-message -p \"~/$CTRL_EXPANSION\"",
                ],
            ),
        )
        .expect("alias whose body needs expansion");
    for (line, expected) in [
        ("ctrlalias", "/server-home/server-value"),
        (
            "set-environment -g CTRL_EXPANSION changed ; ctrlalias",
            "/server-home/server-value",
        ),
        ("ctrlalias", "/server-home/changed"),
    ] {
        assert!(!line.contains(['$', '~']));
        shared.execute_compact_request(
            client,
            ClientKind::Control,
            &mut context,
            request(line),
            &mailbox,
        );
        assert!(
            reliable_children(&mailbox).iter().any(|message| matches!(
                message,
                ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. })
                    if output.as_bytes() == expected.as_bytes()
            )),
            "{line}"
        );
    }
}

#[test]
fn empty_control_expansion_resolvers_complete_without_the_state_lock() {
    let shared = Arc::new(Shared::new(52));
    let inner = shared.inner.lock();
    let (finished, completion) = crossbeam_channel::bounded(1);
    let worker = {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let values = shared.resolve_environment(&[]);
            let homes = shared.resolve_home_directories(&[]);
            let _ = finished.send((values, homes));
        })
    };
    let result = completion.recv_timeout(Duration::from_secs(2));
    drop(inner);
    worker.join().expect("empty expansion worker");
    let (values, homes) = result.expect("empty resolvers do not acquire the state lock");
    assert!(values.is_empty() && homes.is_empty());
}

fn compact_exec_request(commands: Vec<CommandInvocation>) -> zz_protocol::ExecRequest {
    zz_protocol::ExecRequest {
        protocol_version: PROTOCOL_VERSION,
        flags: zz_protocol::ExecFlags::default(),
        client_instance_id: ClientInstanceId(919),
        origin: None,
        working_directory: None,
        tty: None,
        size: None,
        features: 0,
        startup_reentry: None,
        spawned_server_id: None,
        expect_server_id: None,
        process_id: std::process::id(),
        environment: ClientEnvironmentBlob::default(),
        commands,
        raw_control_line: None,
    }
}

#[cfg(unix)]
#[test]
fn exec_resume_upgrades_the_same_socket_and_places_tail_error_after_attachment() {
    use std::os::unix::net::UnixStream;
    let shared = Arc::new(Shared::new(35));
    let (mut client, server) = UnixStream::pair().expect("pair");
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let worker_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || handle_connection(server, &worker_shared));
    let mut request = compact_exec_request(vec![
        CommandInvocation::new("display-message", ["-p", "prefix"]),
        CommandInvocation::new("new-session", ["-s", "ctrl-upgrade", "sleep 30"]),
        CommandInvocation::new("select-window", ["-t", "missing-ctrl-window"]),
    ]);
    request.flags.set(zz_protocol::ExecFlags::RESUME, true);
    request.flags.set(zz_protocol::ExecFlags::LAST, true);
    zz_protocol::write_protocol_message(&mut client, &ProtocolMessage::Exec(request))
        .expect("exec");
    let ProtocolMessage::ExecExit(zz_protocol::ExecExit {
        outcome: zz_protocol::ExecOutcome::Resume(resume),
        ..
    }) = zz_protocol::read_protocol_message(&mut client).expect("resume")
    else {
        panic!("expected resume")
    };
    let mut hello = compact_hello(ClientKind::Interactive);
    hello.subscriptions = zz_protocol::Subscriptions::terminal();
    hello.viewport = Some(zz_protocol::ClientViewport {
        columns: 80,
        rows: 24,
        cell_width_px: 8,
        cell_height_px: 16,
    });
    hello.attach = Some(zz_protocol::AttachOperation::Commands(resume.commands));
    zz_protocol::write_protocol_message(&mut client, &ProtocolMessage::Hello(hello))
        .expect("upgrade hello");
    assert!(matches!(
        zz_protocol::read_protocol_message(&mut client).expect("welcome"),
        ProtocolMessage::Welcome(_)
    ));
    let ProtocolMessage::Batch(batch) =
        zz_protocol::read_protocol_message(&mut client).expect("batch")
    else {
        panic!("expected batch")
    };
    let messages = batch.messages().expect("children");
    let prefix = messages.iter().position(|message| matches!(message, ProtocolMessage::CommandResponse(CommandResponse::Success { request_id: 1, output, .. }) if output.as_bytes() == b"prefix")).expect("prefix output");
    let attached = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientView(zz_protocol::ClientView {
                        session: Some(_),
                        ..
                    }),
                    ..
                })
            )
        })
        .expect("attachment");
    let tail = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::CommandResponse(CommandResponse::Error { request_id: 3, .. })
            )
        })
        .expect("tail error");
    assert!(prefix < attached);
    assert!(attached < tail);
    assert_eq!(shared.inner.lock().clients.len(), 1);
    drop(client);
    worker.join().expect("worker").expect("connection");
    compact_command(
        &shared,
        &mut ExecutionContext::default(),
        "kill-session",
        &["-t", "ctrl-upgrade"],
    );
}

#[test]
fn hook_body_control_notification_follows_output_guard_without_recursive_hooks() {
    let shared = Arc::new(Shared::new(36));
    let mut context = ExecutionContext::default();
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "ctrl-hook", "-n", "initial", "sleep 30"],
    );
    compact_command(
        &shared,
        &mut context,
        "set-hook",
        &[
            "-g",
            "after-display-message",
            "rename-window -t ctrl-hook:0 hook-renamed",
        ],
    );
    compact_command(
        &shared,
        &mut context,
        "set-hook",
        &[
            "-g",
            "window-renamed",
            "set-environment -g CTRL_RECURSIVE_HOOK bad",
        ],
    );
    let (client, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            tree: zz_protocol::TreeSubscription::All,
            ..zz_protocol::Subscriptions::control()
        },
    );
    shared
        .inner
        .lock()
        .client_entry(client)
        .kind
        .replace(ClientKind::Control);
    shared
        .attach_target(client, ClientKind::Control, &mut context, "ctrl-hook")
        .expect("attach");
    reliable_children(&mailbox);
    shared.execute_compact_request(
        client,
        ClientKind::Control,
        &mut context,
        compact_exec_request(vec![CommandInvocation::new(
            "display-message",
            ["-p", "outer"],
        )]),
        &mailbox,
    );
    let messages = reliable_children(&mailbox);
    let started = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ControlCommandStarted { guard: true, .. },
                    ..
                })
            )
        })
        .unwrap_or_else(|| panic!("command start missing: {messages:?}"));
    let notification = messages.iter().position(|message| matches!(message, ProtocolMessage::Event(Event { payload: EventPayload::HookEvent { name, .. }, .. }) if name == "window-renamed")).unwrap_or_else(|| panic!("rename notification missing: {messages:?}"));
    assert!(started < notification, "{messages:?}");
    assert!(
        shared
            .inner
            .lock()
            .engine
            .global_environment_variable("CTRL_RECURSIVE_HOOK")
            .is_none()
    );
    compact_command(&shared, &mut context, "kill-session", &["-t", "ctrl-hook"]);
}

#[test]
fn hash_subscriber_receives_bounded_mouse_hash_without_full_bindings() {
    let shared = Arc::new(Shared::new(37));
    let (_, mailbox) = compact_registered(
        &shared,
        zz_protocol::Subscriptions {
            keys: zz_protocol::KeySubscription::Hash,
            tree: zz_protocol::TreeSubscription::None,
            status: false,
            options: 0,
            pane_stream: false,
        },
    );
    compact_command(
        &shared,
        &mut ExecutionContext::default(),
        "bind-key",
        &["-T", "root", "x", "display-message", "changed"],
    );
    let messages = bounded_reliable_children(&mailbox, 64);
    assert!(!messages.iter().any(|message| matches!(
        message,
        ProtocolMessage::Event(Event {
            payload: EventPayload::KeyTablesChanged { .. } | EventPayload::KeyTablesPatched { .. },
            ..
        })
    )));
    let hash = messages
        .iter()
        .find(|message| {
            matches!(
                message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::KeyTablesHashChanged { .. },
                    ..
                })
            )
        })
        .expect("hash");
    assert!(
        zz_protocol::encode_protocol_message(hash)
            .expect("encode")
            .len()
            <= 64
    );
}

#[test]
fn personalized_view_materializes_the_existing_stamped_tree() {
    let shared = Arc::new(Shared::new(38));
    let mut context = ExecutionContext::default();
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "ctrl-view-a", "-n", "first", "sleep 30"],
    );
    compact_command(
        &shared,
        &mut context,
        "new-session",
        &["-d", "-s", "ctrl-view-b", "-n", "second", "sleep 30"],
    );
    compact_command(
        &shared,
        &mut context,
        "set-option",
        &[
            "-gw",
            "window-status-format",
            "#{client_session}|#{pane_title}",
        ],
    );
    compact_command(
        &shared,
        &mut context,
        "set-option",
        &[
            "-gw",
            "window-status-current-format",
            "#{client_session}|#{pane_title}",
        ],
    );
    compact_command(
        &shared,
        &mut context,
        "set-option",
        &["-gw", "pane-border-status", "top"],
    );
    compact_command(
        &shared,
        &mut context,
        "set-option",
        &[
            "-gw",
            "pane-border-format",
            "#{client_session}|#{pane_title}",
        ],
    );
    let mut clients = Vec::new();
    for session in ["ctrl-view-a", "ctrl-view-b"] {
        let (client, mailbox) = compact_registered(&shared, zz_protocol::Subscriptions::default());
        shared
            .attach_target(
                client,
                ClientKind::Interactive,
                &mut ExecutionContext::default(),
                session,
            )
            .expect("attach");
        clients.push((client, mailbox));
    }
    for (client, _) in &clients {
        let messages = shared.compact_tree_messages(*client, true);
        let mut raw = messages
            .iter()
            .find_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::Snapshot(snapshot),
                    ..
                }) => Some(snapshot.clone()),
                _ => None,
            })
            .expect("tree");
        let view = messages
            .iter()
            .find_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ClientView(view),
                    ..
                }) => Some(view),
                _ => None,
            })
            .expect("view");
        view.apply(&mut raw).expect("overlay");
        let inner = shared.inner.lock();
        let mut expected = inner.engine.state.snapshot();
        stamp_snapshot_for_client(&inner, *client, &mut expected, &snapshot_presence(&inner));
        expected.generation = raw.generation;
        assert_eq!(raw, expected);
    }
    compact_command(
        &shared,
        &mut context,
        "kill-session",
        &["-t", "ctrl-view-a"],
    );
    compact_command(
        &shared,
        &mut context,
        "kill-session",
        &["-t", "ctrl-view-b"],
    );
}

#[cfg(unix)]
mod quiet_socket {
    use super::*;
    use std::os::{fd::AsFd as _, unix::net::UnixStream};

    fn socket_pair(mailbox: &OutboundMailbox) -> (UnixStream, UnixStream) {
        let (reader, server) = UnixStream::pair().expect("socket pair");
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("reader deadline");
        server
            .set_write_timeout(Some(Duration::from_secs(2)))
            .expect("writer deadline");
        rustix::net::sockopt::set_socket_send_buffer_size(&server, 4096)
            .expect("small send buffer");
        assert!(
            !rustix::fs::fcntl_getfl(&server)
                .expect("descriptor flags")
                .contains(rustix::fs::OFlags::NONBLOCK)
        );
        mailbox.state.lock().quiet_socket = Some(
            server
                .as_fd()
                .try_clone_to_owned()
                .expect("socket duplicate"),
        );
        (reader, server)
    }

    fn completion(output: Vec<u8>) -> [ProtocolMessage; 3] {
        [
            ProtocolMessage::Event(Event {
                sequence: 1,
                payload: EventPayload::ControlCommandStarted {
                    request_id: 1,
                    flags: 0,
                    canonical_name: Some("display-message".to_owned()),
                    guard: true,
                },
            }),
            ProtocolMessage::CommandResponse(CommandResponse::Success {
                request_id: 1,
                output: RawText::from_bytes(output),
                exit_code: 0,
                stderr: String::new(),
                stdout_claim: zz_protocol::StdoutClaim::Raw,
            }),
            ProtocolMessage::ExecExit(zz_protocol::ExecExit {
                server_id: 1,
                outcome: zz_protocol::ExecOutcome::Ran,
            }),
        ]
    }

    fn queue_completion(mailbox: &OutboundMailbox, messages: &[ProtocolMessage; 3]) -> Arc<[u8]> {
        assert!(mailbox.collect_control_query());
        let started: Arc<[u8]> = Arc::from(
            zz_protocol::encode_protocol_message(&messages[0]).expect("Started encoding"),
        );
        assert!(mailbox.enqueue_encoded_reliable(Arc::clone(&started)));
        assert!(
            mailbox.enqueue_control_group(
                messages[1..]
                    .iter()
                    .map(|message| {
                        zz_protocol::encode_protocol_message(message)
                            .expect("completion encoding")
                            .into()
                    })
                    .collect()
            )
        );
        started
    }

    fn read_frame(reader: &mut UnixStream) -> (ProtocolMessage, Vec<u8>) {
        let mut following = Vec::new();
        let message = zz_protocol::read_protocol_message_into(reader, &mut following)
            .expect("complete socket frame");
        let mut wire = u32::try_from(following.len())
            .expect("frame length")
            .to_le_bytes()
            .to_vec();
        wire.extend(following);
        assert_eq!(
            wire,
            zz_protocol::encode_protocol_message(&message).expect("owned wire encoding")
        );
        (message, wire)
    }

    fn no_socket_bytes(reader: &UnixStream) {
        assert_eq!(
            rustix::net::recv(reader, &mut [0_u8; 1], rustix::net::RecvFlags::DONTWAIT)
                .expect_err("no direct socket write"),
            rustix::io::Errno::AGAIN
        );
    }

    fn partial_wire(mailbox: &OutboundMailbox) -> (Vec<u8>, usize) {
        let state = mailbox.state.lock();
        let Some(OutboundFrame::Partial { frame, offset }) = state.reliable.front() else {
            panic!("expected a real partial socket write")
        };
        let wire = frame.as_ref().as_ref().to_vec();
        assert!(*offset > 0 && *offset < wire.len());
        assert_eq!(
            state.queued_bytes,
            state.reliable.iter().map(OutboundFrame::len).sum::<usize>()
        );
        assert_eq!(state.written_bytes, *offset as u64);
        assert_eq!(state.discarded_bytes, 0);
        assert_eq!(state.writer_inflight_bytes, 0);
        (wire, *offset)
    }

    fn writer(mailbox: &Arc<OutboundMailbox>, mut server: UnixStream) -> thread::JoinHandle<()> {
        let mailbox = Arc::clone(mailbox);
        thread::spawn(move || {
            write_outbound(&mut server, &mailbox, &Weak::new(), ClientId(0));
        })
    }

    fn writer_finished(mailbox: &OutboundMailbox) {
        let state = mailbox.state.lock();
        assert!(state.writer_finished);
        assert_eq!(state.writer_inflight_bytes, 0);
        assert!(state.quiet_socket.is_none());
    }

    #[test]
    fn full_completion_keeps_exact_wire_raw_output_and_recycled_buffers() {
        assert!(*attach::BATCHED_WRITES);
        let mailbox = OutboundMailbox::new();
        let outer = Vec::with_capacity(4096);
        let allocation = outer.as_ptr();
        mailbox.recycle_frame(outer);
        let (mut reader, server) = socket_pair(&mailbox);
        let flags = rustix::fs::fcntl_getfl(&server).expect("blocking flags");
        let messages = completion(vec![0xff, b'\n']);
        let started = queue_completion(&mailbox, &messages);
        mailbox.finish_control_query();
        assert_eq!(
            rustix::fs::fcntl_getfl(&server).expect("flags after send"),
            flags
        );
        let (ProtocolMessage::Batch(batch), wire) = read_frame(&mut reader) else {
            panic!("expected a single flat completion")
        };
        assert_eq!(batch.messages().expect("flat completion"), messages);
        no_socket_bytes(&reader);
        assert_eq!(Arc::strong_count(&started), 1);
        let state = mailbox.state.lock();
        assert!(state.reliable.is_empty());
        assert_eq!(state.queued_bytes, 0);
        assert_eq!(state.written_bytes, wire.len() as u64);
        assert_eq!(state.discarded_bytes, 0);
        assert_eq!(state.writer_inflight_bytes, 0);
        assert_eq!(state.recycled_frames.len(), 3);
        assert!(
            state
                .recycled_frames
                .iter()
                .any(|frame| frame.as_ptr() == allocation)
        );
        assert!(state.recycled_capacity <= MAX_RECYCLED_FRAME_CAPACITY);
    }

    #[test]
    fn would_block_keeps_the_complete_group_for_the_normal_writer() {
        assert!(*attach::BATCHED_WRITES);
        let mailbox = OutboundMailbox::new();
        let (mut reader, server) = socket_pair(&mailbox);
        let flags = rustix::fs::fcntl_getfl(&server).expect("blocking flags");
        let filler = [0xa5_u8; 4096];
        let mut occupied_bytes = 0;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            assert!(Instant::now() < deadline && occupied_bytes < 4 * 1024 * 1024);
            match rustix::net::send(&server, &filler, rustix::net::SendFlags::DONTWAIT) {
                Ok(written) => {
                    assert!(written > 0);
                    occupied_bytes += written;
                }
                Err(error) if error == rustix::io::Errno::INTR => {}
                Err(error) if error == rustix::io::Errno::AGAIN => break,
                Err(error) => panic!("send-buffer fill failed: {error}"),
            }
        }
        let messages = completion(b"blocked result".to_vec());
        let started = queue_completion(&mailbox, &messages);
        mailbox.finish_control_query();
        assert_eq!(
            rustix::fs::fcntl_getfl(&server).expect("flags after block"),
            flags
        );
        let expected = {
            let state = mailbox.state.lock();
            let Some(frame @ OutboundFrame::Grouped { .. }) = state.reliable.front() else {
                panic!("blocked send changed the queued wire")
            };
            assert_eq!(state.written_bytes, 0);
            assert_eq!(state.queued_bytes, frame.len());
            assert!(state.recycled_frames.is_empty());
            frame.as_ref().to_vec()
        };
        let mut drained = vec![0; occupied_bytes];
        reader.read_exact(&mut drained).expect("drain filler");
        assert!(drained.iter().all(|byte| *byte == 0xa5));
        let writing = writer(&mailbox, server);
        let (ProtocolMessage::Batch(batch), wire) = read_frame(&mut reader) else {
            panic!("expected preserved completion")
        };
        assert_eq!(wire, expected);
        assert_eq!(batch.messages().expect("completion"), messages);
        mailbox.close_after_flush();
        writing.join().expect("normal writer");
        writer_finished(&mailbox);
        assert_eq!(mailbox.stats(), (wire.len() as u64, 0));
        assert_eq!(Arc::strong_count(&started), 1);
    }

    #[test]
    fn partial_tail_precedes_quiet_attach_and_sync_without_child_replay() {
        assert!(*attach::BATCHED_WRITES);
        let mailbox = OutboundMailbox::new();
        let (mut reader, mut server) = socket_pair(&mailbox);
        let flags = rustix::fs::fcntl_getfl(&server).expect("blocking flags");
        let first = completion(vec![b'p'; 256 * 1024]);
        let retained = queue_completion(&mailbox, &first);
        mailbox.finish_control_query();
        assert_eq!(
            rustix::fs::fcntl_getfl(&server).expect("flags after partial"),
            flags
        );
        let (original, offset) = partial_wire(&mailbox);
        assert_eq!(Arc::strong_count(&retained), 2);
        let next = completion(b"next result".to_vec());
        let _next_started = queue_completion(&mailbox, &next);
        mailbox.finish_control_query();
        assert_eq!(partial_wire(&mailbox), (original.clone(), offset));
        mailbox.collect_control_attach();
        mailbox.state.lock().attach_batch = true;
        let snapshot = Shared::event(EventPayload::Snapshot(MuxSnapshot {
            generation: 7,
            ..MuxSnapshot::default()
        }));
        let view = Shared::event(EventPayload::ClientView(zz_protocol::ClientView {
            attachment_generation: 1,
            layout_generation: 1,
            ..zz_protocol::ClientView::default()
        }));
        assert!(mailbox.enqueue_reliable(&snapshot));
        assert!(mailbox.enqueue_reliable(&view));
        let mut in_flight = Vec::new();
        assert!(mailbox.recv_batch(&mut in_flight, MAX_OUTBOUND_BYTES));
        assert_eq!(in_flight.len(), 1);
        assert_eq!(in_flight[0].as_ref(), &original[offset..]);
        {
            let state = mailbox.state.lock();
            assert_eq!(state.writer_inflight_bytes, original.len() - offset);
            assert!(state.ctrl_collecting == ControlCollection::Attach);
            assert!(state.attach_batch);
        }
        assert!(mailbox.flush_control_batch(false));
        let sending = {
            let mailbox = Arc::clone(&mailbox);
            thread::spawn(move || {
                attach::write_frames(&mut server, &in_flight).expect("partial remainder");
                mailbox.finish_batch(&mut in_flight);
                write_outbound(&mut server, &mailbox, &Weak::new(), ClientId(0));
            })
        };
        let (ProtocolMessage::Batch(batch), wire) = read_frame(&mut reader) else {
            panic!("expected the original batch")
        };
        assert_eq!(wire, original);
        assert_eq!(batch.messages().expect("original children"), first);
        let (ProtocolMessage::Batch(batch), final_wire) = read_frame(&mut reader) else {
            panic!("expected the later flat batch")
        };
        let mut expected = next.to_vec();
        expected.extend([snapshot, view]);
        assert_eq!(batch.messages().expect("new children only"), expected);
        mailbox.close_after_flush();
        sending.join().expect("partial writer");
        writer_finished(&mailbox);
        assert_eq!(mailbox.stats(), ((wire.len() + final_wire.len()) as u64, 0));
        assert_eq!(Arc::strong_count(&retained), 1);
    }

    #[test]
    fn an_empty_queue_with_an_inflight_writer_refuses_direct_completion() {
        let mailbox = OutboundMailbox::new();
        let (mut reader, mut server) = socket_pair(&mailbox);
        assert!(mailbox.enqueue_reliable(&ProtocolMessage::TreeSync));
        let mut in_flight = Vec::new();
        assert!(mailbox.recv_batch(&mut in_flight, MAX_OUTBOUND_BYTES));
        assert!(mailbox.state.lock().reliable.is_empty());
        assert_eq!(
            mailbox.state.lock().writer_inflight_bytes,
            in_flight.iter().map(OutboundFrame::len).sum::<usize>()
        );
        let messages = completion(b"after old frame".to_vec());
        let _started = queue_completion(&mailbox, &messages);
        mailbox.finish_control_query();
        no_socket_bytes(&reader);
        assert!(matches!(
            mailbox.state.lock().reliable.front(),
            Some(OutboundFrame::Grouped { .. })
        ));
        attach::write_frames(&mut server, &in_flight).expect("old writer bytes");
        mailbox.finish_batch(&mut in_flight);
        assert_eq!(mailbox.state.lock().writer_inflight_bytes, 0);
        mailbox.close_after_flush();
        write_outbound(&mut server, &mailbox, &Weak::new(), ClientId(0));
        let (first, old_wire) = read_frame(&mut reader);
        assert_eq!(first, ProtocolMessage::TreeSync);
        let (ProtocolMessage::Batch(batch), wire) = read_frame(&mut reader) else {
            panic!("expected queued completion")
        };
        assert_eq!(batch.messages().expect("completion"), messages);
        assert_eq!(mailbox.stats(), ((old_wire.len() + wire.len()) as u64, 0));
        writer_finished(&mailbox);
    }

    #[test]
    fn closing_a_partial_frame_accounts_only_its_remaining_bytes() {
        assert!(*attach::BATCHED_WRITES);
        let mailbox = OutboundMailbox::new();
        let (mut reader, mut server) = socket_pair(&mailbox);
        let messages = completion(vec![b'c'; 256 * 1024]);
        let started = queue_completion(&mailbox, &messages);
        mailbox.finish_control_query();
        let (original, offset) = partial_wire(&mailbox);
        mailbox.close();
        let mut received = Vec::new();
        reader.read_to_end(&mut received).expect("hard-close EOF");
        assert_eq!(received, original[..offset]);
        assert_eq!(
            mailbox.stats(),
            (offset as u64, (original.len() - offset) as u64)
        );
        {
            let state = mailbox.state.lock();
            assert!(state.closed && state.quiet_socket.is_none());
            assert!(state.reliable.is_empty());
            assert_eq!(state.queued_bytes, 0);
        }
        write_outbound(&mut server, &mailbox, &Weak::new(), ClientId(0));
        assert_eq!(Arc::strong_count(&started), 1);
        writer_finished(&mailbox);
    }

    #[test]
    fn overflow_preserves_a_partial_tail_before_its_exit_marker() {
        assert!(*attach::BATCHED_WRITES);
        let mailbox = OutboundMailbox::new();
        let (mut reader, server) = socket_pair(&mailbox);
        let messages = completion(vec![b'o'; 256 * 1024]);
        let started = queue_completion(&mailbox, &messages);
        mailbox.finish_control_query();
        let (original, offset) = partial_wire(&mailbox);
        let child = zz_protocol::encode_protocol_message(&ProtocolMessage::TreeSync)
            .expect("overflow filler");
        for _ in 1..MAX_RELIABLE_MESSAGES {
            assert!(mailbox.enqueue_encoded_reliable(child.clone()));
        }
        assert!(!mailbox.enqueue_encoded_reliable(child.clone()));
        {
            let state = mailbox.state.lock();
            assert!(state.closed);
            assert_eq!(state.reliable.len(), 2);
            assert!(state.reliable.front().expect("tail").is_partial());
            assert_eq!(state.written_bytes, offset as u64);
            assert_eq!(
                state.discarded_bytes,
                ((MAX_RELIABLE_MESSAGES - 1) * child.len()) as u64
            );
            assert!(state.queued_bytes <= MAX_OUTBOUND_BYTES);
        }
        let sending = writer(&mailbox, server);
        let (ProtocolMessage::Batch(batch), wire) = read_frame(&mut reader) else {
            panic!("expected completed partial group")
        };
        assert_eq!(wire, original);
        assert_eq!(batch.messages().expect("no replay"), messages);
        let (exit, exit_wire) = read_frame(&mut reader);
        assert!(matches!(exit, ProtocolMessage::Event(Event {
            payload: EventPayload::ControlExit { reason }, ..
        }) if reason == "too far behind"));
        assert_eq!(reader.read(&mut [0_u8; 1]).expect("exit EOF"), 0);
        sending.join().expect("overflow writer");
        writer_finished(&mailbox);
        assert_eq!(mailbox.stats().0, (wire.len() + exit_wire.len()) as u64);
        assert_eq!(mailbox.state.lock().queued_bytes, 0);
        assert_eq!(Arc::strong_count(&started), 1);
    }

    #[test]
    fn an_exit_marker_that_cannot_fit_shuts_down_without_appending_wire() {
        let mailbox = OutboundMailbox::new();
        let (mut reader, mut server) = socket_pair(&mailbox);
        server.write_all(&[0]).expect("already sent prefix");
        {
            let mut state = mailbox.state.lock();
            state.written_bytes = 1;
            state.queued_bytes = MAX_OUTBOUND_BYTES - 1;
            state.reliable.push_back(OutboundFrame::Partial {
                frame: Box::new(OutboundFrame::Grouped {
                    encoded: vec![0; MAX_OUTBOUND_BYTES],
                    frames: Vec::new(),
                }),
                offset: 1,
            });
        }
        assert!(!mailbox.enqueue_reliable(&ProtocolMessage::TreeSync));
        let mut received = Vec::new();
        reader
            .read_to_end(&mut received)
            .expect("capacity shutdown");
        assert_eq!(received, [0]);
        let state = mailbox.state.lock();
        assert!(state.closed && state.quiet_socket.is_none());
        assert!(state.reliable.is_empty());
        assert_eq!(state.queued_bytes, 0);
        assert_eq!(state.written_bytes, 1);
        assert_eq!(state.discarded_bytes, (MAX_OUTBOUND_BYTES - 1) as u64);
    }

    #[test]
    fn disabled_buffered_and_unsupported_sockets_keep_existing_drains() {
        for (buffered, socket) in [(true, true), (false, false), (false, true)] {
            if !buffered && socket && *attach::BATCHED_WRITES {
                continue;
            }
            let mailbox = if buffered {
                OutboundMailbox::buffered()
            } else {
                OutboundMailbox::new()
            };
            let (reader, _server) = socket_pair(&mailbox);
            if !socket {
                mailbox.state.lock().quiet_socket = None;
            }
            let messages = completion(b"normal drain".to_vec());
            let _started = queue_completion(&mailbox, &messages);
            mailbox.finish_control_query();
            no_socket_bytes(&reader);
            assert_eq!(mailbox.stats(), (0, 0));
            let mut wire = Vec::new();
            mailbox.drain_reliable_into(&mut wire);
            let ProtocolMessage::Batch(batch) =
                zz_protocol::decode_protocol_frame(&wire).expect("existing direct drain")
            else {
                panic!("expected an encoded group")
            };
            assert_eq!(batch.messages().expect("drained completion"), messages);
            assert_eq!(mailbox.state.lock().queued_bytes, 0);
            assert_eq!(mailbox.state.lock().writer_inflight_bytes, 0);
        }
    }

    #[test]
    fn hook_release_never_uses_the_final_completion_socket_attempt() {
        let mailbox = OutboundMailbox::new();
        let (reader, _server) = socket_pair(&mailbox);
        let messages = completion(b"hook yield".to_vec());
        let _started = queue_completion(&mailbox, &messages);
        mailbox.release_control_query();
        no_socket_bytes(&reader);
        assert_eq!(mailbox.stats(), (0, 0));
        assert_eq!(reliable_children(&mailbox), messages);
    }

    #[test]
    fn a_writer_error_releases_a_partial_frame_without_counting_its_prefix_twice() {
        assert!(*attach::BATCHED_WRITES);
        let mailbox = OutboundMailbox::new();
        let (reader, mut server) = socket_pair(&mailbox);
        let messages = completion(vec![b'e'; 256 * 1024]);
        let started = queue_completion(&mailbox, &messages);
        mailbox.finish_control_query();
        let (original, offset) = partial_wire(&mailbox);
        drop(reader);
        write_outbound(&mut server, &mailbox, &Weak::new(), ClientId(0));
        writer_finished(&mailbox);
        assert_eq!(
            mailbox.stats(),
            (offset as u64, (original.len() - offset) as u64)
        );
        let state = mailbox.state.lock();
        assert!(state.closed && state.reliable.is_empty());
        assert_eq!(state.queued_bytes, 0);
        assert_eq!(Arc::strong_count(&started), 1);
    }

    #[test]
    fn panic_after_writer_dequeue_closes_the_socket_and_releases_ownership() {
        assert!(*attach::BATCHED_WRITES);
        let mailbox = OutboundMailbox::new();
        let (mut reader, _server) = socket_pair(&mailbox);
        let messages = completion(vec![b'p'; 256 * 1024]);
        let started = queue_completion(&mailbox, &messages);
        mailbox.finish_control_query();
        let (original, offset) = partial_wire(&mailbox);
        let writing = {
            let mailbox = Arc::clone(&mailbox);
            thread::spawn(move || {
                let _finished = OutboundWriterGuard(&mailbox);
                let mut batch = Vec::new();
                assert!(mailbox.recv_batch(&mut batch, MAX_OUTBOUND_BYTES));
                assert_eq!(
                    mailbox.state.lock().writer_inflight_bytes,
                    batch.iter().map(OutboundFrame::len).sum::<usize>()
                );
                panic!("controlled writer panic");
            })
        };
        assert!(writing.join().is_err());
        let mut received = Vec::new();
        reader.read_to_end(&mut received).expect("panic EOF");
        assert_eq!(received, original[..offset]);
        writer_finished(&mailbox);
        let state = mailbox.state.lock();
        assert!(state.closed && state.reliable.is_empty());
        assert_eq!(state.queued_bytes, 0);
        assert_eq!(state.written_bytes, offset as u64);
        assert_eq!(state.discarded_bytes, (original.len() - offset) as u64);
        assert_eq!(Arc::strong_count(&started), 1);
    }

    #[test]
    fn closing_after_a_successful_write_does_not_discard_the_inflight_bytes() {
        let mailbox = OutboundMailbox::new();
        let (mut reader, mut server) = socket_pair(&mailbox);
        let messages = completion(b"completed before close".to_vec());
        let started = queue_completion(&mailbox, &messages);
        mailbox.release_control_query();
        let finished = OutboundWriterGuard(&mailbox);
        let mut batch = Vec::new();
        assert!(mailbox.recv_batch(&mut batch, MAX_OUTBOUND_BYTES));
        attach::write_frames(&mut server, &batch).expect("successful writer bytes");
        mailbox.close();
        mailbox.finish_batch(&mut batch);
        drop(finished);
        let (ProtocolMessage::Batch(batch), wire) = read_frame(&mut reader) else {
            panic!("expected completed writer frame")
        };
        assert_eq!(batch.messages().expect("completed children"), messages);
        assert_eq!(reader.read(&mut [0_u8; 1]).expect("closed EOF"), 0);
        assert_eq!(mailbox.stats(), (wire.len() as u64, 0));
        writer_finished(&mailbox);
        assert_eq!(Arc::strong_count(&started), 1);
    }

    #[test]
    fn compact_control_installs_the_actual_local_stream_socket() {
        let directory = tempfile::Builder::new()
            .prefix("zzqf.")
            .tempdir_in("/tmp")
            .expect("local socket directory");
        let path = directory.path().join("s");
        let listener = LocalTransport::bind(&path).expect("local listener");
        let shared = Arc::new(Shared::new(61));
        let worker = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                let server = listener.accept().expect("local connection");
                server
                    .set_timeout(Some(Duration::from_secs(2)))
                    .expect("server deadline");
                handle_connection(server, &shared)
            })
        };
        let mut client = LocalTransport::connect(&path).expect("local client");
        client
            .set_timeout(Some(Duration::from_secs(2)))
            .expect("client deadline");
        let mut hello = compact_hello(ClientKind::Control);
        hello.subscriptions = zz_protocol::Subscriptions::control();
        zz_protocol::write_protocol_message(&mut client, &ProtocolMessage::Hello(hello))
            .expect("compact hello");
        let ProtocolMessage::Welcome(welcome) =
            zz_protocol::read_protocol_message(&mut client).expect("local welcome")
        else {
            panic!("expected compact welcome")
        };
        assert!(matches!(
            zz_protocol::read_protocol_message(&mut client).expect("initial batch"),
            ProtocolMessage::Batch(_)
        ));
        let mailbox = shared
            .client_writers
            .lock()
            .get(&welcome.client_id)
            .cloned()
            .expect("registered local writer");
        assert_eq!(
            mailbox.state.lock().quiet_socket.is_some(),
            *attach::BATCHED_WRITES
        );
        #[cfg(target_vendor = "apple")]
        if *attach::BATCHED_WRITES {
            let state = mailbox.state.lock();
            let socket = state.quiet_socket.as_ref().expect("installed Apple socket");
            assert!(rustix::net::sockopt::socket_nosigpipe(socket).expect("SIGPIPE option"));
            assert!(
                !rustix::fs::fcntl_getfl(socket)
                    .expect("installed blocking flags")
                    .contains(rustix::fs::OFlags::NONBLOCK)
            );
        }
        zz_protocol::write_protocol_message(
            &mut client,
            &ProtocolMessage::Exec(compact_exec_request(vec![CommandInvocation::new(
                "display-message",
                ["-p", "local socket output"],
            )])),
        )
        .expect("local query");
        let ProtocolMessage::Batch(batch) =
            zz_protocol::read_protocol_message(&mut client).expect("local completion")
        else {
            panic!("expected joined quiet completion")
        };
        let messages = batch.messages().expect("local children");
        assert!(matches!(
            messages.as_slice(),
            [
                ProtocolMessage::Event(Event { payload: EventPayload::ControlCommandStarted { .. }, .. }),
                ProtocolMessage::CommandResponse(CommandResponse::Success { output, .. }),
                ProtocolMessage::ExecExit(_),
            ] if output.as_bytes() == b"local socket output"
        ));
        drop(client);
        let _ = worker.join().expect("local connection cleanup");
        writer_finished(&mailbox);
    }
}
