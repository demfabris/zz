use super::*;

#[test]
fn sharded_reader_turn_stops_at_the_byte_limit_and_preserves_eof() {
    let (output, queued) = crossbeam_channel::unbounded();
    output.send(ReaderMessage::Eof).expect("queue EOF");
    let mut consumed = 0;
    assert!(!drain_pty_output_burst(
        &queued,
        vec![0; PTY_DRAIN_TURN_BYTES],
        PTY_DRAIN_TURN_BYTES,
        true,
        PTY_BUFFER_POOL_SIZE,
        |_, length| consumed += length,
    ));
    assert_eq!(consumed, PTY_DRAIN_TURN_BYTES);
    assert!(matches!(queued.try_recv(), Ok(ReaderMessage::Eof)));
}

#[test]
fn sharded_reader_turn_yields_after_a_slow_batch() {
    let (output, queued) = crossbeam_channel::unbounded();
    output
        .send(ReaderMessage::Data {
            buffer: vec![2],
            length: 1,
        })
        .expect("queue next batch");
    let mut consumed = Vec::new();
    assert!(!drain_pty_output_burst(
        &queued,
        vec![1],
        1,
        true,
        PTY_BUFFER_POOL_SIZE,
        |buffer, _| {
            consumed.push(buffer);
            thread::sleep(PTY_DRAIN_TURN_TIME);
        },
    ));
    assert_eq!(consumed, [vec![1]]);
    assert_eq!(queued.len(), 1);
}

#[test]
fn sharded_reader_turn_consumes_eof_after_the_final_batch() {
    let (output, queued) = crossbeam_channel::unbounded();
    output
        .send(ReaderMessage::Data {
            buffer: vec![2],
            length: 1,
        })
        .expect("queue final batch");
    output.send(ReaderMessage::Eof).expect("queue EOF");
    let mut consumed = Vec::new();
    let mut buffer = vec![1];
    loop {
        let eof = drain_pty_output_burst(
            &queued,
            buffer,
            1,
            true,
            PTY_BUFFER_POOL_SIZE,
            |buffer, _| {
                consumed.push(buffer);
            },
        );
        if eof {
            break;
        }
        match queued.try_recv().expect("next reader event") {
            ReaderMessage::Data { buffer: next, .. } => buffer = next,
            ReaderMessage::Eof => break,
        }
    }
    assert_eq!(consumed, [vec![1], vec![2]]);
    assert!(queued.is_empty());
}

#[test]
fn blocking_reader_marks_its_pane_ready_for_data_and_eof() {
    let wake = ActorWake::none().for_actor();
    let ready = Arc::clone(wake.ready.as_ref().expect("pane readiness"));
    ready.store(false, Ordering::Release);
    let (output, queued) = crossbeam_channel::bounded(1);
    let (recycle, recycled) = crossbeam_channel::bounded(1);
    recycle
        .send(vec![0; PTY_READ_BUFFER_BYTES])
        .expect("seed reader buffer");
    let reader = thread::spawn(move || {
        read_pty(
            Box::new(std::io::Cursor::new(vec![1])),
            Box::new(|| 0),
            output,
            recycled,
            wake,
        );
    });
    let ReaderMessage::Data { buffer, length } = queued
        .recv_timeout(Duration::from_secs(2))
        .expect("reader data")
    else {
        panic!("reader EOF before data");
    };
    assert_eq!(&buffer[..length], &[1]);
    let wait_ready = || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ready.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "reader did not wake its pane");
            thread::yield_now();
        }
    };
    wait_ready();
    ready.store(false, Ordering::Release);
    recycle.send(buffer).expect("recycle reader buffer");
    assert!(matches!(
        queued
            .recv_timeout(Duration::from_secs(2))
            .expect("reader EOF"),
        ReaderMessage::Eof
    ));
    wait_ready();
    reader.join().expect("reader thread");
}
