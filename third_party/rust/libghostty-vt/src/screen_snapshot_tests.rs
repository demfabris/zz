use crate::{
    ScreenSnapshot, Terminal,
    fmt::{Format, Formatter, FormatterOptions},
    terminal::{CompressionMode, Point, PointCoordinate},
};

fn snapshot_text(snapshot: &ScreenSnapshot) -> Vec<u8> {
    snapshot
        .format_alloc(
            FormatterOptions::new()
                .with_format(Format::Plain)
                .with_trim(true)
                .with_unwrap(true),
        )
        .expect("format snapshot")
        .to_vec()
}

fn snapshot_styled_text(snapshot: &ScreenSnapshot) -> Vec<u8> {
    snapshot
        .format_alloc(
            FormatterOptions::new()
                .with_format(Format::Vt)
                .with_trim(true)
                .with_style(true)
                .with_hyperlink(true),
        )
        .expect("format snapshot")
        .to_vec()
}

fn populated() -> Terminal<'static, 'static> {
    let mut terminal = Terminal::new(180, 24).expect("terminal");
    terminal
        .set_scrollback_max_bytes(None)
        .expect("history bytes");
    terminal
        .set_scrollback_max_lines(Some(2_000))
        .expect("history lines");
    for row in 0..1_000 {
        terminal.vt_write(format!("line-{row:04} \x1b[31mred\x1b[0m e\u{301} 界\r\n").as_bytes());
    }
    terminal
}

#[test]
fn frozen_resident_history_survives_output_pruning_clear_and_source_drop() {
    let mut terminal = populated();
    let snapshot = terminal.clone_screen().expect("snapshot");
    let expected = snapshot_text(&snapshot);
    let rows = snapshot.total_rows().expect("rows");
    for _ in 0..3_000 {
        terminal.vt_write(b"replaced\r\n");
    }
    terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hclear");
    drop(terminal);
    assert_eq!(snapshot.total_rows().expect("rows"), rows);
    assert_eq!(snapshot_text(&snapshot), expected);
}

#[test]
fn frozen_compressed_history_survives_source_drop_and_nested_clone() {
    let mut terminal = populated();
    terminal.compress(CompressionMode::Full).expect("compress");
    let snapshot = terminal.clone_screen().expect("snapshot");
    let nested = snapshot.clone_screen().expect("nested snapshot");
    drop(terminal);
    drop(snapshot);
    let expected = snapshot_text(&nested);
    assert!(expected.starts_with("line-0000 red e\u{301} 界".as_bytes()));
    assert!(
        expected
            .windows(b"line-0999".len())
            .any(|window| window == b"line-0999")
    );
}

#[test]
fn frozen_snapshot_resize_reflows_its_own_content() {
    let mut terminal = populated();
    let mut snapshot = terminal.clone_screen().expect("snapshot");
    let expected = snapshot_text(&snapshot);
    terminal.resize(90, 12, 0, 0).expect("live resize");
    terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hdifferent");
    let anchor = snapshot
        .resize_anchored(100, 20, PointCoordinate { x: 2, y: 4 })
        .expect("frozen resize");
    assert!(anchor.is_some());
    assert_eq!(snapshot_text(&snapshot), expected);
}

#[test]
fn frozen_snapshot_can_move_to_search_worker_after_live_source_drop() {
    let terminal = populated();
    let snapshot = terminal.clone_screen().expect("snapshot");
    drop(terminal);
    let captured = std::thread::spawn(move || snapshot_text(&snapshot))
        .join()
        .expect("search worker");
    assert!(captured.starts_with(b"line-0000"));
}

fn styled_text(terminal: &Terminal<'_, '_>) -> Vec<u8> {
    Formatter::new(
        terminal,
        FormatterOptions::new()
            .with_format(Format::Vt)
            .with_trim(true)
            .with_style(true)
            .with_hyperlink(true),
    )
    .expect("formatter")
    .format_alloc(None)
    .expect("format")
    .to_vec()
}

#[test]
fn seeded_frozen_clones_match_eager_formatted_reference() {
    for seed in 1_u64..=12 {
        let mut random = seed;
        let mut terminal = Terminal::new(80, 8).expect("terminal");
        terminal.set_scrollback_max_bytes(None).expect("bytes");
        terminal.set_scrollback_max_lines(Some(700)).expect("lines");
        for row in 0..600 {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let palette = random % 255;
            let value = format!(
                "\x1b]133;A\x07\x1b[38;5;{palette}m\x1b]8;id={row};https://example.test/{row}\x1b\\row-{row} e\u{301} 界\x1b]8;;\x1b\\\x1b[0m\x1b]133;B\x07\r\n"
            );
            terminal.vt_write(value.as_bytes());
        }
        if seed % 2 == 0 {
            terminal.compress(CompressionMode::Full).expect("compress");
        }
        let expected = styled_text(&terminal);
        let snapshot = terminal.clone_screen().expect("snapshot");
        let prompt = snapshot
            .grid_ref(Point::Screen(PointCoordinate { x: 0, y: 10 }))
            .expect("prompt")
            .cell()
            .expect("cell")
            .semantic_content()
            .expect("semantic content");
        for turn in 0..8 {
            let cols = 40 + u16::try_from((random >> turn) % 80).expect("bounded columns");
            terminal.resize(cols, 8 + turn, 0, 0).expect("resize");
            terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hreplacement\r\n");
        }
        drop(terminal);
        assert_eq!(snapshot_styled_text(&snapshot), expected, "seed {seed}");
        assert_eq!(
            snapshot
                .grid_ref(Point::Screen(PointCoordinate { x: 0, y: 10 }))
                .expect("prompt")
                .cell()
                .expect("cell")
                .semantic_content()
                .expect("semantic content"),
            prompt,
            "seed {seed}"
        );
    }
}

#[test]
fn frozen_clone_keeps_callback_state_in_source_and_reads_during_live_mutation() {
    let bells = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut terminal = populated();
    let observed = std::rc::Rc::clone(&bells);
    terminal
        .on_bell(move |_| observed.set(observed.get() + 1))
        .expect("bell");
    let snapshot = terminal.clone_screen().expect("snapshot");
    let expected = snapshot_text(&snapshot);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let entered = std::sync::Arc::clone(&barrier);
    let worker = std::thread::spawn(move || {
        entered.wait();
        for _ in 0..20 {
            assert_eq!(snapshot_text(&snapshot), expected);
        }
    });
    barrier.wait();
    for turn in 0..20 {
        terminal.resize(80 + turn, 10, 0, 0).expect("resize");
        terminal.vt_write(b"\x07\x1b[3J\x1b[2J\x1b[Hnew\r\n");
        terminal.compress(CompressionMode::Full).expect("compress");
    }
    worker.join().expect("worker");
    assert_eq!(bells.get(), 20);
}

#[test]
fn frozen_snapshot_updates_osc_color_overrides_without_live_content() {
    let mut terminal = Terminal::new(80, 8).expect("terminal");
    terminal.vt_write(b"old\x1b]10;#112233\x07\x1b]11;#223344\x07\x1b]4;5;#334455\x07");
    let mut snapshot = terminal.clone_screen().expect("snapshot");
    let expected = snapshot_text(&snapshot);
    terminal.vt_write(b"\rnew\x1b]10;#556677\x07\x1b]11;#667788\x07\x1b]4;5;#778899\x07");
    snapshot.set_colors_from(&terminal).expect("appearance");
    assert_eq!(
        snapshot.fg_color().expect("foreground"),
        terminal.fg_color().expect("foreground")
    );
    assert_eq!(
        snapshot.bg_color().expect("background"),
        terminal.bg_color().expect("background")
    );
    assert_eq!(
        snapshot.color_palette().expect("palette").0,
        terminal.color_palette().expect("palette").0
    );
    assert_eq!(snapshot_text(&snapshot), expected);
}

#[test]
fn frozen_grid_row_maps_columns_once_and_checks_bounds() {
    let terminal = populated();
    let snapshot = terminal.clone_screen().expect("snapshot");
    let row = snapshot
        .grid_row(Point::Screen(PointCoordinate { x: 0, y: 17 }))
        .expect("row");
    for column in 0..180 {
        let mapped = row.cell(column).expect("column");
        let direct = snapshot
            .grid_ref(Point::Screen(PointCoordinate { x: column, y: 17 }))
            .expect("direct");
        assert_eq!(
            mapped
                .cell()
                .expect("mapped cell")
                .codepoint()
                .expect("codepoint"),
            direct
                .cell()
                .expect("direct cell")
                .codepoint()
                .expect("codepoint")
        );
        assert_eq!(
            mapped.cell().expect("mapped cell").wide().expect("width"),
            direct.cell().expect("direct cell").wide().expect("width")
        );
    }
    assert!(row.cell(180).is_none());
    assert!(
        snapshot
            .grid_row(Point::Screen(PointCoordinate { x: 1, y: 17 }))
            .is_err()
    );
}

#[test]
fn frozen_narrow_reflow_retains_history_beyond_source_limits() {
    for compressed in [false, true] {
        let mut terminal = Terminal::new(180, 24).expect("terminal");
        terminal.set_scrollback_max_bytes(None).expect("bytes");
        terminal
            .set_scrollback_max_lines(Some(10_000))
            .expect("lines");
        for line in 0..10_040 {
            let text = format!("row-{line:05} {}\r\n", "x".repeat(168));
            terminal.vt_write(text.as_bytes());
        }
        if compressed {
            terminal.compress(CompressionMode::Full).expect("compress");
        }
        let mut snapshot = terminal.clone_screen().expect("snapshot");
        let captured_rows = snapshot.total_rows().expect("captured rows");
        assert!(captured_rows > 9_500);
        let expected = snapshot_text(&snapshot);
        let anchor = snapshot
            .resize_anchored(30, 12, PointCoordinate { x: 5, y: 0 })
            .expect("narrow reflow");
        assert!(anchor.is_some());
        assert!(snapshot.total_rows().expect("reflowed rows") > 50_000);
        assert_eq!(snapshot_text(&snapshot), expected);
        terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hreplacement");
        drop(terminal);
        snapshot.resize(180, 24).expect("wide reflow");
        assert_eq!(snapshot_text(&snapshot), expected);
    }
}
