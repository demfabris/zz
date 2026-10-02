use super::*;

fn finish<T>(mut request: TerminalRequest<T>) -> Result<T, TerminalRequestError> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(result) = request.poll(Instant::now()) {
            return result;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn asynchronous_capture_and_history_preserve_the_canonical_rows_and_flags() {
    let terminal =
        TerminalSession::spawn_empty_with_appearance(64, Arc::new(TerminalAppearance::default()));
    terminal.resize(8, 3, 8, 16);
    assert!(terminal.feed(Arc::from(
        b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n".as_slice()
    )));
    let options = CaptureOptions {
        start: CaptureBoundary::HistoryStart,
        join_wrapped: true,
        preserve_trailing: true,
        ..CaptureOptions::default()
    };
    let expected = terminal.capture(options).unwrap();
    let history = terminal.history(0, 10).unwrap();
    let _round_trips = forbid_actor_round_trips();
    assert_eq!(
        finish(terminal.capture_request(options, Arc::new(|| {})))
            .unwrap()
            .unwrap(),
        expected
    );
    let captured = finish(terminal.history_request(0, 10, Arc::new(|| {})))
        .unwrap()
        .unwrap();
    assert_eq!(captured.0, history.0);
    assert_eq!(captured.1, history.1);
    assert_eq!(captured.2, history.2);
    assert_eq!(captured.3, history.3);
    assert_eq!(captured.4, history.4);
}

#[test]
fn asynchronous_semantic_and_pointer_reads_use_the_actor_owned_grid() {
    let terminal =
        TerminalSession::spawn_empty_with_appearance(64, Arc::new(TerminalAppearance::default()));
    assert!(terminal.feed(Arc::from(b"\x1b]133;A\x07$ \x1b]133;B\x07echo answer\x1b]133;C\x07\r\nanswer\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ".as_slice())));
    let expected = terminal.capture_last_command().unwrap();
    let pointer = terminal.pointer_context(TerminalViewId(0), 3, 0).unwrap();
    let _round_trips = forbid_actor_round_trips();
    let captured = finish(terminal.capture_last_command_request(Arc::new(|| {})))
        .unwrap()
        .unwrap();
    assert_eq!(captured.command, expected.command);
    assert_eq!(captured.output, expected.output);
    assert_eq!(
        finish(terminal.pointer_context_request(TerminalViewId(0), 3, 0, Arc::new(|| {}))).unwrap(),
        pointer
    );
}

#[test]
fn asynchronous_kitty_reads_keep_pixel_and_storage_generations_together() {
    let terminal =
        TerminalSession::spawn_empty_with_appearance(64, Arc::new(TerminalAppearance::default()));
    let _round_trips = forbid_actor_round_trips();
    assert!(terminal.feed(Arc::from(
        b"\x1b_Ga=T,f=24,s=1,v=1,i=77;/wAA\x1b\\".as_slice()
    )));
    let image = finish(terminal.kitty_image_request(77, Arc::new(|| {})))
        .unwrap()
        .unwrap();
    assert_eq!(image.bgra, [0, 0, 255, 255]);
    assert_eq!(
        finish(terminal.kitty_image_generation_request(77, Arc::new(|| {}))).unwrap(),
        Some(image.generation)
    );
    assert!(terminal.feed(Arc::from(
        b"\x1b_Ga=T,f=24,s=1,v=1,i=77;AP8A\x1b\\".as_slice()
    )));
    let replacement = finish(terminal.kitty_image_request(77, Arc::new(|| {})))
        .unwrap()
        .unwrap();
    assert_ne!(replacement.generation, image.generation);
    assert_eq!(replacement.bgra, [0, 255, 0, 255]);
    assert_eq!(
        finish(terminal.kitty_image_generation_request(77, Arc::new(|| {}))).unwrap(),
        Some(replacement.generation)
    );
}
