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

#[test]
fn a_repeated_alternate_enable_keeps_its_output_marks() {
    let mut terminal = new_terminal(20, 6, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    feed(
        &mut terminal,
        &mut filter,
        b"\x1b[?47h\x1b]133;C\x07alt\r\n\x1b[?47hsecond\r\n",
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
        "O alt\n- second\n- \n- \n- \n- "
    );
}

#[test]
fn a_resized_or_recoloured_frozen_revision_keeps_its_output_marks() {
    let mut terminal = new_terminal(20, 6, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    feed(
        &mut terminal,
        &mut filter,
        b"top\r\n\x1b]133;C\x07out\r\nnext\r\n",
    );
    let revision = ModeRevision::capture(&terminal).expect("revision");
    revision.stamp_output_rows(|| filter.output_rows(&terminal));
    assert_eq!(revision.output_rows(), [1]);
    let (resized, _) = revision
        .resized(12, 5, PointCoordinate { x: 0, y: 0 })
        .expect("resized revision");
    assert_eq!(resized.output_rows(), [1]);
    let flags = capture_revision(
        &resized,
        0,
        CaptureOptions {
            start: CaptureBoundary::HistoryStart,
            line_flags: true,
            ..CaptureOptions::default()
        },
    )
    .expect("frozen capture");
    assert!(flags.lines().any(|line| line == "O out"), "{flags:?}");
    let recoloured = resized
        .with_appearance(&mut terminal)
        .expect("recoloured revision");
    assert_eq!(recoloured.output_rows(), [1]);

    let mut narrow = new_terminal(10, 4, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    feed(
        &mut narrow,
        &mut filter,
        b"aaaaaaaaaaaaaaa\r\n\x1b]133;C\x07out\r\n",
    );
    let revision = ModeRevision::capture(&narrow).expect("revision");
    revision.stamp_output_rows(|| filter.output_rows(&narrow));
    assert_eq!(revision.output_rows(), [2]);
    let (wide, _) = revision
        .resized(20, 4, PointCoordinate { x: 0, y: 0 })
        .expect("reflowed revision");
    assert_eq!(wide.output_rows(), [1]);
}

fn e_links_screen() -> Terminal<'static, 'static> {
    let mut terminal = new_terminal(80, 20, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    let bytes = [
        format!("{} {}\r\n", osc8("http://a", "aa"), osc8("http://b", "bb")),
        format!(
            "{}{}\r\n",
            osc8("http://a", "again"),
            osc8("http://c", "cc")
        ),
        format!("\x1b[1;31m{}\x1b[0m tail\r\n", osc8("http://s", "red")),
        "\x1b]8;;http://m\x07x\x1b[4my\x1b[24mz\x1b]8;;\x07\x1b[32mgreen\x1b[0m\r\n".to_owned(),
        "\x1b]8;;http://q\x07a\x1b[1mb\x1b]8;;\x07\x1b[0m\r\n".to_owned(),
        format!(
            "{} {}\r\n",
            osc8("http://x\\y", "ab"),
            osc8("http://y", "cd")
        ),
        format!("pre {} post\r\n", osc8("http://w", &"0".repeat(90))),
        format!("{}\r\n", osc8("http://t", "ab  ")),
        format!("x{}\r\n", osc8("http://z", "y")),
        format!("{}z\r\n", osc8("http://u", "一二")),
        "\x1b]8;;http://n\x07one\r\ntwo\x1b]8;;\x07 three\r\n".to_owned(),
        format!("\x1b[7m{}\r\nNEXT", osc8("http://r", "rev")),
    ]
    .concat();
    feed(&mut terminal, &mut filter, bytes.as_bytes());
    terminal
}

#[test]
fn escaped_capture_wraps_links_in_osc_8_like_the_pin() {
    let terminal = e_links_screen();
    let filter = EngineFilter::default();
    let rows = CaptureOptions {
        start: CaptureBoundary::Relative(0),
        end: CaptureBoundary::Relative(14),
        ..CaptureOptions::default()
    };
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                escape_sequences: true,
                ..rows
            }
        ),
        [
            "\x1b]8;;http://a\x1b\\aa\x1b]8;;\x1b\\ \x1b]8;;http://b\x1b\\bb\x1b]8;;\x1b\\",
            "\x1b]8;;http://a\x1b\\again\x1b]8;;http://c\x1b\\cc\x1b]8;;\x1b\\",
            "\x1b[1m\x1b[31m\x1b]8;;http://s\x1b\\red\x1b[0m\x1b]8;;\x1b\\ tail",
            "\x1b]8;;http://m\x1b\\x\x1b[4my\x1b[0mz\x1b[32m\x1b]8;;\x1b\\green\x1b[39m",
            "\x1b]8;;http://q\x1b\\a\x1b[1mb\x1b[0m\x1b]8;;\x1b\\",
            "\x1b]8;;http://x\\\\y\x1b\\ab\x1b]8;;\x1b\\ \x1b]8;;http://y\x1b\\cd\x1b]8;;\x1b\\",
            "pre \x1b]8;;http://w\x1b\\0000000000000000000000000000000000000000000000000000000000000000000000000000\x1b]8;;\x1b\\",
            "00000000000000 post",
            "\x1b]8;;http://t\x1b\\ab  \x1b]8;;\x1b\\",
            "x\x1b]8;;http://z\x1b\\y\x1b]8;;\x1b\\",
            "\x1b]8;;http://u\x1b\\一二\x1b]8;;\x1b\\z",
            "\x1b]8;;http://n\x1b\\one\x1b]8;;\x1b\\",
            "\x1b]8;;http://n\x1b\\two\x1b]8;;\x1b\\ three",
            "\x1b[7m\x1b]8;;http://r\x1b\\rev\x1b[0m\x1b]8;;\x1b\\",
            "\x1b[7mNEXT\x1b[0m",
        ]
        .join("\n")
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                escape_sequences: true,
                join_wrapped: true,
                preserve_trailing: true,
                ..rows
            }
        ),
        [
            "\x1b]8;;http://a\x1b\\aa\x1b]8;;\x1b\\ \x1b]8;;http://b\x1b\\bb\x1b]8;;\x1b\\",
            "\x1b]8;;http://a\x1b\\again\x1b]8;;http://c\x1b\\cc\x1b]8;;\x1b\\",
            "\x1b[1m\x1b[31m\x1b]8;;http://s\x1b\\red\x1b[0m\x1b]8;;\x1b\\ tail",
            "\x1b]8;;http://m\x1b\\x\x1b[4my\x1b[0mz\x1b[32m\x1b]8;;\x1b\\green",
            "\x1b[39m\x1b]8;;http://q\x1b\\a\x1b[1mb\x1b[1m\x1b]8;;\x1b\\",
            "\x1b[0m\x1b]8;;http://x\\\\y\x1b\\ab\x1b]8;;\x1b\\ \x1b]8;;http://y\x1b\\cd\x1b]8;;\x1b\\",
            "pre \x1b]8;;http://w\x1b\\0000000000000000000000000000000000000000000000000000000000000000000000000000\x1b]8;;\x1b\\00000000000000 post",
            "\x1b]8;;http://t\x1b\\ab  \x1b]8;;\x1b\\",
            "x\x1b]8;;http://z\x1b\\y\x1b]8;;http://z\x1b\\\x1b]8;;\x1b\\",
            "\x1b]8;;http://u\x1b\\一二\x1b]8;;\x1b\\z",
            "\x1b]8;;http://n\x1b\\one\x1b]8;;\x1b\\",
            "two three",
            "\x1b[7m\x1b]8;;http://r\x1b\\rev\x1b]8;;\x1b\\",
            "NEXT",
        ]
        .join("\n")
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                escape_sequences: true,
                trim_positions: true,
                ..rows
            }
        ),
        [
            "\x1b]8;;http://a\x1b\\aa\x1b]8;;\x1b\\ \x1b]8;;http://b\x1b\\bb\x1b]8;;\x1b\\",
            "\x1b]8;;http://a\x1b\\again\x1b]8;;http://c\x1b\\cc\x1b]8;;\x1b\\",
            "\x1b[1m\x1b[31m\x1b]8;;http://s\x1b\\red\x1b[0m\x1b]8;;\x1b\\ tail",
            "\x1b]8;;http://m\x1b\\x\x1b[4my\x1b[0mz\x1b[32m\x1b]8;;\x1b\\green",
            "\x1b[39m\x1b]8;;http://q\x1b\\a\x1b[1mb\x1b[1m\x1b]8;;\x1b\\",
            "\x1b[0m\x1b]8;;http://x\\\\y\x1b\\ab\x1b]8;;\x1b\\ \x1b]8;;http://y\x1b\\cd\x1b]8;;\x1b\\",
            "pre \x1b]8;;http://w\x1b\\0000000000000000000000000000000000000000000000000000000000000000000000000000\x1b]8;;\x1b\\",
            "00000000000000 post",
            "\x1b]8;;http://t\x1b\\ab  \x1b]8;;\x1b\\",
            "x\x1b]8;;http://z\x1b\\y\x1b]8;;http://z\x1b\\\x1b]8;;\x1b\\",
            "\x1b]8;;http://u\x1b\\一二\x1b]8;;\x1b\\z",
            "\x1b]8;;http://n\x1b\\one\x1b]8;;\x1b\\",
            "two three",
            "\x1b[7m\x1b]8;;http://r\x1b\\rev\x1b]8;;\x1b\\",
            "NEXT",
        ]
        .join("\n")
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                escape_sequences: true,
                preserve_trailing: true,
                ..rows
            }
        ),
        [
            "\x1b]8;;http://a\x1b\\aa\x1b]8;;\x1b\\ \x1b]8;;http://b\x1b\\bb\x1b]8;;\x1b\\               ",
            "\x1b]8;;http://a\x1b\\again\x1b]8;;http://c\x1b\\cc\x1b]8;;\x1b\\             ",
            "\x1b[1m\x1b[31m\x1b]8;;http://s\x1b\\red\x1b[0m\x1b]8;;\x1b\\ tail            ",
            "\x1b]8;;http://m\x1b\\x\x1b[4my\x1b[0mz\x1b[32m\x1b]8;;\x1b\\green\x1b[39m            ",
            "\x1b]8;;http://q\x1b\\a\x1b[1mb\x1b[0m\x1b]8;;\x1b\\                  ",
            "\x1b]8;;http://x\\\\y\x1b\\ab\x1b]8;;\x1b\\ \x1b]8;;http://y\x1b\\cd\x1b]8;;\x1b\\               ",
            "pre \x1b]8;;http://w\x1b\\0000000000000000000000000000000000000000000000000000000000000000000000000000\x1b]8;;\x1b\\",
            "00000000000000 post ",
            "\x1b]8;;http://t\x1b\\ab  \x1b]8;;\x1b\\                ",
            "x\x1b]8;;http://z\x1b\\y\x1b]8;;\x1b\\                  ",
            "\x1b]8;;http://u\x1b\\一二\x1b]8;;\x1b\\z               ",
            "\x1b]8;;http://n\x1b\\one\x1b]8;;\x1b\\                 ",
            "\x1b]8;;http://n\x1b\\two\x1b]8;;\x1b\\ three           ",
            "\x1b[7m\x1b]8;;http://r\x1b\\rev\x1b[0m\x1b]8;;\x1b\\                 ",
            "\x1b[7mNEXT\x1b[0m                ",
        ]
        .join("\n")
    );
    assert_eq!(
        capture(
            &terminal,
            &filter,
            CaptureOptions {
                escape_sequences: true,
                escape_nonprintable: true,
                ..rows
            }
        ),
        [
            "\\033]8;;http://a\\033\\\\aa\\033]8;;\\033\\\\ \\033]8;;http://b\\033\\\\bb\\033]8;;\\033\\\\",
            "\\033]8;;http://a\\033\\\\again\\033]8;;http://c\\033\\\\cc\\033]8;;\\033\\\\",
            "\\033[1m\\033[31m\\033]8;;http://s\\033\\\\red\\033[0m\\033]8;;\\033\\\\ tail",
            "\\033]8;;http://m\\033\\\\x\\033[4my\\033[0mz\\033[32m\\033]8;;\\033\\\\green\\033[39m",
            "\\033]8;;http://q\\033\\\\a\\033[1mb\\033[0m\\033]8;;\\033\\\\",
            "\\033]8;;http://x\\\\y\\033\\\\ab\\033]8;;\\033\\\\ \\033]8;;http://y\\033\\\\cd\\033]8;;\\033\\\\",
            "pre \\033]8;;http://w\\033\\\\0000000000000000000000000000000000000000000000000000000000000000000000000000\\033]8;;\\033\\\\",
            "00000000000000 post",
            "\\033]8;;http://t\\033\\\\ab  \\033]8;;\\033\\\\",
            "x\\033]8;;http://z\\033\\\\y\\033]8;;\\033\\\\",
            "\\033]8;;http://u\\033\\\\一二\\033]8;;\\033\\\\z",
            "\\033]8;;http://n\\033\\\\one\\033]8;;\\033\\\\",
            "\\033]8;;http://n\\033\\\\two\\033]8;;\\033\\\\ three",
            "\\033[7m\\033]8;;http://r\\033\\\\rev\\033[0m\\033]8;;\\033\\\\",
            "\\033[7mNEXT\\033[0m",
        ]
        .join("\n")
    );
}

#[test]
fn mode_capture_ranges_count_from_the_frozen_screen_not_the_scroll_position() {
    let mut terminal = new_terminal(10, 4, 64).expect("terminal");
    let mut filter = EngineFilter::default();
    let lines = (1..=12)
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\r\n");
    feed(&mut terminal, &mut filter, lines.as_bytes());
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
    copy_mode
        .as_deref_mut()
        .expect("frozen revision")
        .viewport_offset = 3;
    let capture = |start, end| {
        capture_terminal_marked(
            &terminal,
            copy_mode.as_deref(),
            CaptureOptions {
                mode: true,
                start,
                end,
                ..CaptureOptions::default()
            },
            &[],
        )
        .expect("mode capture")
        .lines()
        .collect::<Vec<_>>()
        .join(",")
    };
    assert_eq!(
        capture(CaptureBoundary::Relative(-3), CaptureBoundary::Relative(1)),
        "6,7,8,9,10"
    );
    assert_eq!(
        capture(CaptureBoundary::Relative(0), CaptureBoundary::VisibleEnd),
        "9,10,11,12"
    );
}
