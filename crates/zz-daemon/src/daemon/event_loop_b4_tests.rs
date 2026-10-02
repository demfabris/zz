use super::io_tests::messages;
use super::*;
use crate::daemon::tests::{
    key_table_events, key_table_fixture, table_active, take_reliable_messages,
};

#[test]
fn empty_timer_heap_allows_indefinite_poll_and_control_output_uses_its_age_deadline() {
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let now = Instant::now();
    assert_eq!(event_loop.poll_timeout(now), None);
    event_loop.control_output_deadline = Some(now + COPY_PIPE_POLL_INTERVAL);
    assert_eq!(event_loop.poll_timeout(now), Some(COPY_PIPE_POLL_INTERVAL));
    let soon = now + Duration::from_micros(1);
    shared
        .timer_tx
        .send(timers::TimerInput::Timer(timers::TimerCommand::Rename(
            soon,
        )))
        .unwrap();
    assert_eq!(event_loop.poll_timeout(now), Some(Duration::ZERO));
}

#[test]
fn publication_flush_and_prefix_expiry_run_without_a_worker() {
    let (shared, client, _, _, mailbox) = key_table_fixture("b4-worker");
    let workers = shared.connection_threads.worker_count();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let (server, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let token = event_loop.insert(server.receive_fd().unwrap()).unwrap();
    let output = ProtocolMessage::TreeSync;
    let outbound = Arc::clone(&event_loop.connections[&token].outbound);
    let now = Instant::now();
    {
        let mut inner = shared.inner.lock();
        inner.client_entry(client).key_table_deadline = Some(now);
        inner.client_entry(client).published_key_table = Some((Some("prefix".to_owned()), false));
    }
    take_reliable_messages(&mailbox);
    shared.request_publish(timers::PublishReason::Tree);
    shared.request_publish(timers::PublishReason::Tree);
    shared
        .connection_threads
        .fail_next
        .store(true, Ordering::Release);
    shared
        .timer_tx
        .send(timers::TimerInput::Timer(
            timers::TimerCommand::PublishFlush(now),
        ))
        .unwrap();
    event_loop.turn(&shared).unwrap();
    shared
        .timer_tx
        .send(timers::TimerInput::KeyTable(
            KeyTableDeadlineCommand::Schedule(client, Some(now)),
        ))
        .unwrap();
    assert!(outbound.enqueue_reliable(&output));
    let started = Instant::now();
    event_loop.turn(&shared).unwrap();
    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(
        key_table_events(&mailbox),
        vec![
            EventPayload::PrefixArmed { armed: false },
            table_active(None, false)
        ]
    );
    assert_eq!(messages(&mut peer, &mut Inbound::default()), vec![output]);
    assert!(
        shared
            .connection_threads
            .fail_next
            .swap(false, Ordering::AcqRel)
    );
    assert_eq!(shared.connection_threads.worker_count(), workers);
    event_loop.remove(token, &shared);
    shared.request_shutdown();
}
