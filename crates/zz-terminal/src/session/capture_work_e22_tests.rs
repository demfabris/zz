use super::*;

fn same_capture(actual: &str, expected: &str, options: CaptureOptions) {
    if actual != expected {
        let at = actual
            .bytes()
            .zip(expected.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(actual.len().min(expected.len()));
        let start = at.saturating_sub(24);
        panic!(
            "{options:?}: lengths {} / {}, mismatch at {at}, actual {:?}, expected {:?}",
            actual.len(),
            expected.len(),
            &actual.as_bytes()[start..(at + 50).min(actual.len())],
            &expected.as_bytes()[start..(at + 50).min(expected.len())]
        );
    }
}

#[test]
fn capture_chunks_keep_wrap_styles_numbering_and_one_revision() {
    let mut terminal = new_terminal(16, 8, 2048).expect("terminal");
    for row in 0..1100 {
        if row == 510 {
            terminal
                .vt_write(b"\x1b[31mwrapped words   wrapped words   wrapped words   \x1b[0m\r\n");
        } else {
            terminal.vt_write(format!("\x1b[{}m{row:04}\tend\x1b[0m\r\n", 31 + row % 7).as_bytes());
        }
    }
    for options in [
        CaptureOptions::default(),
        CaptureOptions {
            join_wrapped: true,
            ..CaptureOptions::default()
        },
        CaptureOptions {
            preserve_trailing: true,
            ..CaptureOptions::default()
        },
        CaptureOptions {
            number_lines: true,
            join_wrapped: true,
            ..CaptureOptions::default()
        },
        CaptureOptions {
            escape_sequences: true,
            ..CaptureOptions::default()
        },
        CaptureOptions {
            escape_sequences: true,
            join_wrapped: true,
            number_lines: true,
            escape_nonprintable: true,
            ..CaptureOptions::default()
        },
        CaptureOptions {
            line_flags: true,
            ..CaptureOptions::default()
        },
        CaptureOptions {
            line_flags: true,
            join_wrapped: true,
            number_lines: true,
            ..CaptureOptions::default()
        },
        CaptureOptions {
            line_flags: true,
            escape_sequences: true,
            ..CaptureOptions::default()
        },
    ] {
        let options = CaptureOptions {
            start: CaptureBoundary::HistoryStart,
            ..options
        };
        let expected = capture_terminal(&terminal, None, options).expect("whole capture");
        let (reply, receiver) = crossbeam_channel::bounded(1);
        let mut work = CaptureWork::start(
            &terminal,
            None,
            &EngineFilter::default(),
            CaptureRequest {
                options,
                reply: reply.into(),
            },
        )
        .expect("chunked capture");
        terminal.vt_write(b"changed after snapshot\r\n");
        let mut steps = 0;
        loop {
            let before = work.next;
            let complete = work.step().expect("capture step");
            assert!(work.next - before <= CAPTURE_ROWS_PER_STEP);
            steps += 1;
            if complete {
                break;
            }
        }
        assert!(steps >= 3);
        same_capture(&work.output, &expected, options);
        drop(work);
        assert!(receiver.try_recv().is_err());
    }
}

#[test]
fn capture_chunks_join_a_wrapped_line_longer_than_the_row_budget() {
    let mut terminal = new_terminal(8, 8, 2048).expect("terminal");
    terminal.vt_write("word    ".repeat(1100).as_bytes());
    let options = CaptureOptions {
        start: CaptureBoundary::HistoryStart,
        join_wrapped: true,
        ..CaptureOptions::default()
    };
    let expected = capture_terminal(&terminal, None, options).expect("whole capture");
    let (reply, _) = crossbeam_channel::bounded(1);
    let mut work = CaptureWork::start(
        &terminal,
        None,
        &EngineFilter::default(),
        CaptureRequest {
            options,
            reply: reply.into(),
        },
    )
    .expect("chunked capture");
    while !work.step().expect("capture step") {}
    same_capture(&work.output, &expected, options);
}
