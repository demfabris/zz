use super::*;

fn feed(terminal: &mut Terminal<'_, '_>, filter: &mut EngineFilter, bytes: &[u8]) {
    filter.write(
        bytes,
        EngineKnobs::default(),
        terminal,
        &mut Vec::new(),
        &mut None,
        &mut None,
    );
}

fn osc8(uri: &str, text: &str) -> String {
    format!("\x1b]8;;{uri}\x07{text}\x1b]8;;\x07")
}

fn capture(terminal: &Terminal<'_, '_>, filter: &EngineFilter, options: CaptureOptions) -> String {
    capture_terminal_marked(terminal, None, options, &filter.output_rows(terminal))
        .expect("capture")
}

fn marked_screen() -> (Terminal<'static, 'static>, EngineFilter) {
    let mut terminal = new_terminal(20, 8, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    let bytes = format!(
        "\x1b]133;A\x07$ {} {}\r\n\x1b]133;C\x07plain output\r\n{}{}\r\nabcdefghijklmnopqrstuvwxyz\r\né\r\n",
        osc8("http://a", "aa"),
        osc8("http://b", "bb"),
        osc8("http://a", "again"),
        osc8("http://c", "cc"),
    );
    feed(&mut terminal, &mut filter, bytes.as_bytes());
    (terminal, filter)
}

#[test]
fn capture_hyperlinks_match_the_pin() {
    let (terminal, filter) = marked_screen();
    let links = CaptureOptions {
        hyperlinks: true,
        ..CaptureOptions::default()
    };
    assert_eq!(
        capture(&terminal, &filter, links),
        "http://a http://b\nhttp://a http://c"
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                number_lines: true,
                ..links
            }
        ),
        "0 http://a http://b\n2 http://a http://c"
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                line_flags: true,
                ..links
            }
        ),
        "HP http://a http://b\nH http://a http://c"
    );
    for ignored in [
        CaptureOptions {
            escape_sequences: true,
            ..links
        },
        CaptureOptions {
            preserve_trailing: true,
            ..links
        },
        CaptureOptions {
            join_wrapped: true,
            preserve_trailing: true,
            ..links
        },
        CaptureOptions {
            escape_nonprintable: true,
            ..links
        },
    ] {
        assert_eq!(
            capture(&terminal, &filter, ignored),
            "http://a http://b\nhttp://a http://c",
            "{ignored:?}"
        );
    }
}

#[test]
fn capture_line_flags_match_the_pin_without_d_and_x() {
    let (terminal, filter) = marked_screen();
    let flags = CaptureOptions {
        line_flags: true,
        ..CaptureOptions::default()
    };
    assert_eq!(
        capture(&terminal, &filter, flags),
        "HP $ aa bb\nO plain output\nH againcc\nW abcdefghijklmnopqrst\n- uvwxyz\n- é\n- \n- "
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                number_lines: true,
                ..flags
            }
        ),
        "0 HP $ aa bb\n1 O plain output\n2 H againcc\n3 W abcdefghijklmnopqrst\n4 - uvwxyz\n5 - é\n6 - \n7 - "
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                join_wrapped: true,
                preserve_trailing: true,
                ..flags
            }
        ),
        "HP $ aa bb\nO plain output\nH againcc\nW abcdefghijklmnopqrst- uvwxyz\n- é\n- \n- "
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                preserve_trailing: true,
                ..flags
            }
        ),
        "HP $ aa bb   \nO plain output        \nH againcc   \nW abcdefghijklmnopqrst\n- uvwxyz    \n- é    \n- \n- "
    );
    for same in [
        CaptureOptions {
            trim_positions: true,
            ..flags
        },
        CaptureOptions {
            escape_nonprintable: true,
            ..flags
        },
    ] {
        assert_eq!(
            capture(&terminal, &filter, same),
            "HP $ aa bb\nO plain output\nH againcc\nW abcdefghijklmnopqrst\n- uvwxyz\n- é\n- \n- ",
            "{same:?}"
        );
    }
    let styled = capture(
        &terminal,
        &filter,
        CaptureOptions {
            escape_sequences: true,
            ..flags
        },
    );
    assert_eq!(
        styled.lines().map(|line| &line[..2]).collect::<Vec<_>>(),
        ["HP", "O ", "H ", "W ", "- ", "- ", "- ", "- "]
    );
}

#[test]
fn capture_links_follow_wraps_history_and_the_column_cap() {
    let mut terminal = new_terminal(10, 4, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    let bytes = format!(
        "{}\r\n{}{}{}\r\n\x1b]133;C\x07out\r\n\r\n\r\n",
        osc8("http://w", "LLLLLLLLLLLLLLL"),
        osc8("http://x\\y", "ab"),
        osc8("http://x", "cd"),
        osc8("http://y", "ef"),
    );
    feed(&mut terminal, &mut filter, bytes.as_bytes());
    let whole = CaptureOptions {
        start: CaptureBoundary::HistoryStart,
        ..CaptureOptions::default()
    };
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                hyperlinks: true,
                ..whole
            }
        ),
        "http://w\nhttp://x\\\\y http://x http://y"
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                hyperlinks: true,
                join_wrapped: true,
                preserve_trailing: true,
                ..whole
            }
        ),
        "http://whttp://x\\\\y http://x http://y"
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                hyperlinks: true,
                line_flags: true,
                ..whole
            }
        ),
        "HW http://w\nH http://x\\\\y http://x http://y"
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                line_flags: true,
                ..whole
            }
        ),
        "HW LLLLLLLLLL\nH LLLLL\nH abcdef\nO out\n- \n- \n- "
    );

    let mut narrow = new_terminal(4, 3, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    let bytes = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|name| osc8(&format!("http://{name}"), name))
        .collect::<String>();
    feed(&mut narrow, &mut filter, bytes.as_bytes());
    assert_eq!(
        capture(
            &narrow,
            &filter,
            CaptureOptions {
                hyperlinks: true,
                ..CaptureOptions::default()
            }
        ),
        "http://a http://b http://c http://d"
    );
    assert_eq!(
        capture(
            &narrow,
            &filter,
            CaptureOptions {
                hyperlinks: true,
                start: CaptureBoundary::Relative(2),
                ..CaptureOptions::default()
            }
        ),
        ""
    );
}

#[test]
fn output_marks_reset_with_the_terminal() {
    let mut terminal = new_terminal(10, 4, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    feed(&mut terminal, &mut filter, b"\x1b]133;C\x1b\\out\r\n");
    assert_eq!(filter.output_rows(&terminal), [0]);
    feed(&mut terminal, &mut filter, b"\x1bc");
    assert!(filter.output_rows(&terminal).is_empty());
}

fn frozen_screen() -> (Terminal<'static, 'static>, EngineFilter, CopyModeSlot) {
    let mut terminal = new_terminal(20, 6, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    let before = format!("{}\r\n\x1b]133;C\x07out\r\n", osc8("http://old", "old"));
    feed(&mut terminal, &mut filter, before.as_bytes());
    let mut selection = None;
    let mut copy_mode = None;
    enter_copy_mode(
        &mut terminal,
        &mut selection,
        &mut copy_mode,
        false,
        false,
        None,
        false,
    )
    .expect("copy mode");
    let mode = copy_mode.as_deref().expect("frozen revision");
    mode.revision
        .stamp_output_rows(|| filter.output_rows(&terminal));
    let after = format!(
        "plain\r\n{}\r\n\x1b]133;C\x07later\r\n",
        osc8("http://new", "new")
    );
    feed(&mut terminal, &mut filter, after.as_bytes());
    (terminal, filter, copy_mode)
}

#[test]
fn mode_capture_hyperlinks_print_nothing_like_the_pin_mode_screen() {
    let (terminal, filter, copy_mode) = frozen_screen();
    let options = CaptureOptions {
        mode: true,
        hyperlinks: true,
        ..CaptureOptions::default()
    };
    assert_eq!(
        capture_terminal_marked(
            &terminal,
            copy_mode.as_deref(),
            options,
            &filter.output_rows(&terminal)
        )
        .expect("mode capture"),
        ""
    );
}

#[test]
fn mode_capture_line_flags_keep_the_frozen_link_and_output_marks() {
    let (terminal, filter, copy_mode) = frozen_screen();
    let options = CaptureOptions {
        mode: true,
        line_flags: true,
        ..CaptureOptions::default()
    };
    assert_eq!(
        capture_terminal_marked(
            &terminal,
            copy_mode.as_deref(),
            options,
            &filter.output_rows(&terminal)
        )
        .expect("mode capture"),
        "H old\nO out\n- \n- \n- \n- "
    );
}

#[test]
fn a_full_row_erase_drops_the_output_mark_like_the_pin() {
    let mut terminal = new_terminal(20, 6, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    filter.write(
        b"\x1b]133;C\x07out\r\nmore\r\n\x1b[H\x1b[2Jafter\r\n\x1b]133;C\x07keep\r\nxy\x1b[1;3H\x1b[J\r\n",
        EngineKnobs {
            scroll_on_clear: false,
            ..EngineKnobs::default()
        },
        &mut terminal,
        &mut Vec::new(),
        &mut None,
        &mut None,
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                line_flags: true,
                ..CaptureOptions::default()
            }
        ),
        "- af\n- \n- \n- \n- \n- "
    );
}

#[test]
fn entering_the_alternate_screen_drops_its_old_output_marks() {
    let mut terminal = new_terminal(20, 4, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    feed(
        &mut terminal,
        &mut filter,
        b"\x1b[?1049h\x1b]133;C\x07alt\x1b[?1049l\x1b[?1049h",
    );
    assert!(filter.output_rows(&terminal).is_empty());
}

#[test]
fn output_marks_last_as_long_as_their_rows_are_retained() {
    let mut terminal = new_terminal(20, 6, 5000).expect("terminal");
    let mut filter = EngineFilter::default();
    let mut bytes = String::new();
    for row in 0..1030 {
        bytes.push_str("\x1b]133;C\x07row");
        bytes.push_str(&row.to_string());
        bytes.push_str("\r\n");
    }
    feed(&mut terminal, &mut filter, bytes.as_bytes());
    assert_eq!(filter.output_rows(&terminal).len(), 1030);
    let flags = capture(
        &terminal,
        &filter,
        CaptureOptions {
            line_flags: true,
            start: CaptureBoundary::HistoryStart,
            ..CaptureOptions::default()
        },
    );
    assert!(flags.starts_with("O row0\nO row1\n"), "{flags:.40}");
    assert_eq!(
        flags.lines().filter(|line| line.starts_with("O ")).count(),
        1030
    );
}
