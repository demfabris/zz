use std::cell::Cell;

use super::*;
use crate::session::SelectionMode;
use crate::session::mode_revision::{ModeRevision, ModeSelection};
use crate::session::{
    CaptureBoundary, CaptureOptions, CopyModeSearch, HistorySearchSnapshot, SearchCase,
    SearchDirection, SearchMatch, SearchMode, SearchQuery, SearchWorker, SnapshotChange,
    TerminalViewId, TerminalViewState, ViewportGenerations, capture_terminal, color,
    copy_mode_search_match, copy_mode_snapshot, enter_copy_mode, new_terminal,
    run_copy_mode_search, snapshot,
};
use crate::{OverlayKind, OverlaySpan, SessionStatus};

fn point(x: u16, y: u32) -> PointCoordinate {
    PointCoordinate { x, y }
}

fn style(revision: &ModeRevision, point: PointCoordinate) -> PackedStyle {
    let cell = revision.cell(point);
    revision.shared_dictionary().styles[usize::from(cell.style_id())]
}

fn grid(terminal: &libghostty_vt::Terminal<'_, '_>) -> CopyGrid {
    CopyGrid::new(
        terminal.clone_screen().expect("frozen screen"),
        terminal
            .fg_color()
            .expect("foreground")
            .map_or(Color::rgb(255, 255, 255), color),
        terminal
            .bg_color()
            .expect("background")
            .map_or(Color::rgb(0, 0, 0), color),
        &terminal.color_palette().expect("palette").0,
        terminal.cols().expect("columns"),
    )
}

#[test]
fn paged_copy_entry_keeps_cells_and_search_offsets_lazy() {
    if ModeRevision::clone_enabled() {
        return;
    }
    let mut terminal = new_terminal(180, 24, 2000).expect("terminal");
    for row in 0..1000 {
        terminal.vt_write(format!("row-{row:04} target\r\n").as_bytes());
    }
    let revision = ModeRevision::capture(&mut terminal).expect("revision");
    assert!(revision.total_rows() >= 1000);
    assert!(revision.cells.is_empty());
    assert!(revision.rows.is_empty());
    assert!(revision.search.text.is_empty());
    assert!(revision.search.rows.is_empty());
    assert!(revision.search.offsets.is_empty());
    assert!(revision.search.terminal.is_some());
    assert_eq!(
        revision.capture_rows(0, 0, false, false, false),
        "row-0000 target"
    );
    assert!(revision.cells.is_empty());
    assert!(revision.search.offsets.is_empty());
}

#[test]
fn lazy_rows_keep_semantics_styles_wide_cells_and_graphemes_after_pruning_and_ed3() {
    let mut terminal = new_terminal(80, 4, 32).expect("terminal");
    let cluster = format!("e{}", "\u{301}".repeat(12));
    let seed = format!(
        "\x1b]133;A\x1b\\\x1b[1;4;38;2;11;22;33m$ \x1b]133;B\x1b\\{cluster}界 input-old\x1b[0m\x1b]133;C\x1b\\\r\noutput-old"
    );
    terminal.vt_write(seed.as_bytes());
    let revision = ModeRevision::capture(&mut terminal).expect("revision");
    let old_rows = revision.total_rows();
    for row in 0..4096 {
        terminal.vt_write(format!("replacement-{row}\r\n").as_bytes());
    }
    let live = capture_terminal(
        &terminal,
        None,
        CaptureOptions {
            start: CaptureBoundary::HistoryStart,
            ..CaptureOptions::default()
        },
    )
    .expect("live history");
    assert!(!live.contains("input-old"));
    terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hcleared-live");
    drop(terminal);

    assert_eq!(revision.total_rows(), old_rows);
    assert!(revision.row(0).prompt());
    assert!(revision.is_prompt(point(0, 0)));
    assert!(revision.is_input(point(2, 0)));
    assert!(revision.is_output(point(0, 1)));
    assert!(revision.cell_matches_text(point(2, 0), &cluster));
    assert_eq!(revision.cell(point(3, 0)).width(), CellWidth::Wide);
    assert_eq!(revision.cell(point(4, 0)).width(), CellWidth::SpacerTail);
    let input_style = style(&revision, point(2, 0));
    assert_eq!(input_style.foreground(), Color::rgb(11, 22, 33));
    assert!(input_style.bold());
    assert_eq!(input_style.underline(), crate::UnderlineStyle::Single);
    assert_eq!(
        revision.capture_rows(0, 1, false, false, false),
        format!("$ {cluster}界 input-old\noutput-old")
    );
    let selected = revision.format_selection(
        ModeSelection {
            anchor: point(2, 0),
            focus: point(5, 0),
            mode: SelectionMode::Cell,
            rectangle: false,
        },
        false,
    );
    assert_eq!(selected, format!("{cluster}界"));
}

#[test]
fn row_cache_stays_bounded_when_scanning_and_revisiting_history() {
    let mut terminal = new_terminal(16, 4, 256).expect("terminal");
    for row in 0..100 {
        terminal.vt_write(format!("row-{row:03}\r\n").as_bytes());
    }
    let mut grid = grid(&terminal);
    let first = grid.row(0).expect("first row");
    for row in 1..100 {
        grid.row(row).expect("history row");
        assert!(grid.cached.len() <= CACHED_ROWS);
        assert!(grid.order.len() <= CACHED_ROWS);
    }
    assert_eq!(grid.cached.len(), CACHED_ROWS);
    assert!(!grid.cached.contains_key(&0));
    let revisited = grid.row(0).expect("evicted row");
    assert!(!Arc::ptr_eq(&first, &revisited));
    assert_eq!(first.cells, revisited.cells);
    assert_eq!(first.semantics, revisited.semantics);
    assert_eq!(first.cells[0].glyph(), u32::from('r'));
    let resident = grid.row(0).expect("resident row");
    assert!(Arc::ptr_eq(&revisited, &resident));
    assert_eq!(grid.cached.len(), CACHED_ROWS);
}

#[test]
fn paged_search_maps_unicode_and_cancels_between_rows_without_flat_history() {
    if ModeRevision::clone_enabled() {
        return;
    }
    let mut terminal = new_terminal(32, 4, 256).expect("terminal");
    for row in 0..100 {
        terminal.vt_write(format!("e\u{301}界 TARGET{row:02}\r\n").as_bytes());
    }
    let snapshot = HistorySearchSnapshot::capture(&terminal).expect("search snapshot");
    let query = SearchQuery {
        text: r"TARGET\d{2}".to_owned(),
        mode: SearchMode::Regex,
        case: SearchCase::Sensitive,
        direction: SearchDirection::Forward,
    };
    let mut matches = Vec::with_capacity(128);
    let allocation = matches.as_ptr();
    let result = snapshot
        .search_reusing(&query, 7, &mut matches, || false)
        .expect("completed search");
    assert_eq!(result.matches.len(), 100);
    assert_eq!(result.request_id, 7);
    assert_eq!(result.matches.as_ptr(), allocation);
    assert!(
        result
            .matches
            .iter()
            .all(|found| found.start == 4 && found.end == 12)
    );
    matches = result.matches;
    let polls = Cell::new(0);
    assert!(
        snapshot
            .search_reusing(&query, 8, &mut matches, || {
                polls.set(polls.get() + 1);
                polls.get() == 8
            })
            .is_none()
    );
    assert_eq!(polls.get(), 8);
    assert_eq!(matches.as_ptr(), allocation);
    assert!(!matches.is_empty());
    assert!(matches.len() < 100);
    assert!(snapshot.text.is_empty());
    assert!(snapshot.rows.is_empty());
    assert!(snapshot.offsets.is_empty());
    terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hno-targets");
    drop(terminal);
    let after = snapshot.search(&query, 9, || false).expect("frozen search");
    assert_eq!(after.matches.len(), 100);
}

#[test]
fn wrapped_literal_and_regex_are_one_match_with_physical_endpoints() {
    if ModeRevision::clone_enabled() {
        return;
    }
    let mut terminal = new_terminal(12, 4, 64).expect("terminal");
    terminal.vt_write("prefix e\u{301}界 target-12345 end\r\nbar\r\nbaz".as_bytes());
    let snapshot = HistorySearchSnapshot::capture(&terminal).expect("snapshot");
    let literal = SearchQuery::literal("e\u{301}界 target-12345 end");
    let expected = SearchMatch {
        row: 0,
        end_row: 2,
        start: 7,
        end: 3,
    };
    let found = snapshot.search(&literal, 1, || false).expect("literal");
    assert_eq!(found.matches, [expected]);
    let regex = SearchQuery {
        text: "e\u{301}界 target-\\d{5} end".to_owned(),
        mode: SearchMode::Regex,
        case: SearchCase::Sensitive,
        direction: SearchDirection::Forward,
    };
    assert_eq!(
        snapshot.search(&regex, 2, || false).expect("regex").matches,
        [expected]
    );
    assert!(
        snapshot
            .search(&SearchQuery::literal("barbaz"), 3, || false)
            .expect("hard line break")
            .matches
            .is_empty()
    );
    assert_eq!(expected.span(0, 12), Some((7, 12)));
    assert_eq!(expected.span(1, 12), Some((0, 12)));
    assert_eq!(expected.span(2, 12), Some((0, 3)));
    assert!(expected.contains(point(1, 1), 12));
    assert!(!expected.contains(point(3, 2), 12));
    terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hreplacement");
    drop(terminal);
    assert_eq!(
        snapshot
            .search(&literal, 4, || false)
            .expect("frozen")
            .matches,
        [expected]
    );
    assert!(snapshot.text.is_empty());
    assert!(snapshot.rows.is_empty());
    assert!(snapshot.offsets.is_empty());
}

#[test]
fn wrapped_copy_search_places_emacs_at_end_and_vi_at_start() {
    if ModeRevision::clone_enabled() {
        return;
    }
    for vi in [false, true] {
        let mut terminal = new_terminal(12, 4, 64).expect("terminal");
        terminal.vt_write("prefix e\u{301}界 target-12345 end".as_bytes());
        let mut mode = None;
        enter_copy_mode(&mut terminal, &mut None, &mut mode, false, false, None, vi)
            .expect("copy mode");
        mode.as_mut().expect("mode").cursor = point(0, 0);
        let (jobs, job_rx) = crossbeam_channel::bounded(1);
        let mut worker = SearchWorker {
            jobs,
            discard_jobs: job_rx,
            latest_requests: std::collections::HashMap::new(),
            next_request: 0,
            match_scratch: Vec::new(),
            idle: None,
        };
        let mut search = None;
        let spec = CopyModeSearch {
            text: "e\u{301}界 target-12345 end".to_owned(),
            direction: SearchDirection::Forward,
            regex: false,
            incremental: false,
        };
        assert!(run_copy_mode_search(
            TerminalViewId(1),
            &mut mode,
            &mut search,
            &mut None,
            &mut worker,
            &spec,
            1,
            vi,
            true,
            &mut None,
        ));
        let mode = mode.expect("searched mode");
        assert_eq!(mode.cursor, if vi { point(7, 0) } else { point(3, 2) });
        assert_eq!(mode.search_count, Some((1, false)));
        assert_eq!(
            copy_mode_search_match(&mode, search.as_deref(), mode.cursor),
            spec.text
        );
        let mut view = TerminalViewState::for_screen(libghostty_vt::screen::Screen::Primary);
        view.search = search;
        let mut generations = ViewportGenerations::default();
        let mut dictionary = ViewportDictionary::default();
        let frozen = copy_mode_snapshot(
            &mut generations,
            SnapshotChange::Content,
            &mut dictionary,
            &view,
            &mode,
            SessionStatus::Running,
        );
        let live = snapshot(
            &terminal,
            &mut libghostty_vt::RenderState::new().expect("render state"),
            &mut libghostty_vt::render::RowIterator::new().expect("rows"),
            &mut libghostty_vt::render::CellIterator::new().expect("cells"),
            &mut generations,
            SnapshotChange::Content,
            &mut ViewportDictionary::default(),
            Some(&view),
            SessionStatus::Running,
        )
        .expect("live search overlays");
        for frame in [live, frozen] {
            let marked = frame
                .overlays
                .iter()
                .copied()
                .filter(|overlay| overlay.kind() == OverlayKind::SearchCurrent)
                .collect::<Vec<_>>();
            assert_eq!(
                marked,
                [
                    OverlaySpan::new(0, 7, 12, OverlayKind::SearchCurrent),
                    OverlaySpan::new(1, 0, 12, OverlayKind::SearchCurrent),
                    OverlaySpan::new(2, 0, 3, OverlayKind::SearchCurrent),
                ]
            );
        }
    }
}

#[test]
fn wrapped_history_search_reuses_matches_and_cancels_without_flat_arrays() {
    if ModeRevision::clone_enabled() {
        return;
    }
    let mut terminal = new_terminal(12, 4, 4096).expect("terminal");
    for _ in 0..600 {
        terminal.vt_write("prefix e\u{301}界 target-12345 end\r\n".as_bytes());
    }
    let snapshot = HistorySearchSnapshot::capture(&terminal).expect("snapshot");
    let query = SearchQuery::literal("e\u{301}界 target-12345 end");
    let mut scratch = Vec::with_capacity(640);
    let allocation = scratch.as_ptr();
    let result = snapshot
        .search_reusing(&query, 1, &mut scratch, || false)
        .expect("wrapped search");
    assert_eq!(result.matches.len(), 600);
    assert_eq!(result.matches.as_ptr(), allocation);
    assert!(
        result
            .matches
            .iter()
            .all(|found| { found.end_row == found.row + 2 && found.start == 7 && found.end == 3 })
    );
    scratch = result.matches;
    let polls = Cell::new(0);
    assert!(
        snapshot
            .search_reusing(&query, 2, &mut scratch, || {
                polls.set(polls.get() + 1);
                polls.get() == 600
            })
            .is_none()
    );
    assert_eq!(polls.get(), 600);
    assert_eq!(scratch.as_ptr(), allocation);
    assert!(!scratch.is_empty());
    assert!(scratch.len() < 600);
    assert!(snapshot.text.is_empty());
    assert!(snapshot.rows.is_empty());
    assert!(snapshot.offsets.is_empty());
    assert_eq!(std::mem::size_of::<crate::session::SearchCellOffset>(), 12);
}

#[test]
fn appearance_refresh_keeps_frozen_text_and_explicit_colors() {
    if ModeRevision::clone_enabled() {
        return;
    }
    let mut terminal = new_terminal(32, 4, 32).expect("terminal");
    terminal.vt_write(b"frozen \x1b[38;2;9;8;7mRGB\x1b[0m");
    let original = ModeRevision::capture(&mut terminal).expect("revision");
    let original_default = style(&original, point(0, 0));
    terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hreplacement-live");
    terminal
        .set_default_fg_color(Some(RgbColor { r: 1, g: 2, b: 3 }))
        .expect("foreground");
    terminal
        .set_default_bg_color(Some(RgbColor { r: 4, g: 5, b: 6 }))
        .expect("background");
    let updated = original
        .with_appearance(&mut terminal)
        .expect("frozen appearance");
    assert_eq!(
        updated.capture_rows(0, 0, false, false, false),
        "frozen RGB"
    );
    assert_eq!(
        original.capture_rows(0, 0, false, false, false),
        "frozen RGB"
    );
    assert_eq!(style(&original, point(0, 0)), original_default);
    assert_eq!(
        style(&updated, point(0, 0)).foreground(),
        Color::rgb(1, 2, 3)
    );
    assert_eq!(
        style(&updated, point(0, 0)).background(),
        Color::rgb(4, 5, 6)
    );
    assert_eq!(
        style(&updated, point(7, 0)).foreground(),
        Color::rgb(9, 8, 7)
    );
}

#[test]
fn frozen_resize_tracks_the_wide_cell_through_a_width_round_trip() {
    if ModeRevision::clone_enabled() {
        return;
    }
    let mut terminal = new_terminal(80, 4, 64).expect("terminal");
    terminal.vt_write(format!("{}界{}", "a".repeat(73), "b".repeat(45)).as_bytes());
    let original = ModeRevision::capture(&mut terminal).expect("revision");
    let original_cursor = point(73, 0);
    terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hlive-replacement");
    let (narrow, narrow_cursor) = original
        .resized(40, 4, original_cursor)
        .expect("narrow backing");
    assert_eq!(narrow_cursor.x, 33);
    assert!(narrow.cell_matches_text(narrow_cursor, "界"));
    assert_eq!(narrow.cell(narrow_cursor).width(), CellWidth::Wide);
    assert!(original.cell_matches_text(original_cursor, "界"));
    let (restored, restored_cursor) = narrow
        .resized(80, 4, narrow_cursor)
        .expect("restored backing");
    assert_eq!(restored_cursor, original_cursor);
    assert!(restored.cell_matches_text(restored_cursor, "界"));
    assert_eq!(
        restored
            .capture_rows(0, restored.total_rows() - 1, true, false, false)
            .trim_end(),
        original
            .capture_rows(0, original.total_rows() - 1, true, false, false)
            .trim_end()
    );
}

#[test]
fn row_conversion_keeps_style_links_semantics_and_mixed_unicode() {
    let mut terminal = new_terminal(32, 4, 64).expect("terminal");
    let cluster = format!("e{}", "\u{301}".repeat(12));
    terminal.vt_write(
        format!(
            "\x1b]133;A\x1b\\\x1b[38;2;11;22;33mAA\x1b[1mBB\x1b[22mCC\x1b]8;;https://example.test\x1b\\DD\x1b]8;;\x1b\\EE\x1b[0mF🦀{cluster}界\x1b]133;B\x1b\\input\x1b]133;C\x1b\\\r\n\x1b[38;2;44;55;66mGG\x1b[0mHH"
        )
        .as_bytes(),
    );
    let revision = ModeRevision::capture(&mut terminal).expect("revision");
    terminal.vt_write(b"\x1b[3J\x1b[2J\x1b[Hreplacement");
    drop(terminal);

    for column in [0, 1, 4, 5, 8, 9] {
        let captured = style(&revision, point(column, 0));
        assert_eq!(captured.foreground(), Color::rgb(11, 22, 33));
        assert!(!captured.bold());
        assert!(!captured.hyperlink());
    }
    for column in [2, 3] {
        assert!(style(&revision, point(column, 0)).bold());
    }
    for column in [6, 7] {
        let captured = style(&revision, point(column, 0));
        assert_eq!(captured.foreground(), Color::rgb(11, 22, 33));
        assert!(captured.hyperlink());
    }
    assert!(revision.row(0).prompt());
    assert!(revision.is_prompt(point(0, 0)));
    assert!(revision.is_input(point(16, 0)));
    assert!(revision.is_output(point(0, 1)));
    assert_eq!(
        style(&revision, point(0, 1)).foreground(),
        Color::rgb(44, 55, 66)
    );
    assert_eq!(revision.cell(point(10, 0)).glyph(), u32::from('F'));
    assert_eq!(revision.cell(point(11, 0)).glyph(), u32::from('🦀'));
    assert_eq!(revision.cell(point(11, 0)).width(), CellWidth::Wide);
    assert_eq!(revision.cell(point(12, 0)).width(), CellWidth::SpacerTail);
    assert!(revision.cell_matches_text(point(13, 0), &cluster));
    assert_eq!(revision.cell(point(14, 0)).glyph(), u32::from('界'));
    assert_eq!(revision.cell(point(15, 0)).width(), CellWidth::SpacerTail);
    assert_eq!(
        revision.capture_rows(0, 1, false, false, false),
        format!("AABBCCDDEEF🦀{cluster}界input\nGGHH")
    );
}

#[test]
fn formatting_reader_keeps_its_row_dictionary_after_other_rows_compact() {
    let mut terminal = new_terminal(16, 4, 5000).expect("terminal");
    let cluster = "e\u{301}";
    terminal.vt_write(format!("{cluster} initial\r\n").as_bytes());
    for index in 0..4200 {
        terminal.vt_write(
            format!(
                "\x1b[38;2;{};{};99mrow-{index:04}\r\n",
                index / 256,
                index % 256,
            )
            .as_bytes(),
        );
    }
    let revision = ModeRevision::capture(&mut terminal).expect("revision");
    let reader = revision.reader();
    let original = reader.cell(point(0, 0));
    assert_eq!(reader.first_char(point(0, 0)), Some('e'));
    let generation = revision.dictionary_generation();
    for row in 1..revision.total_rows() {
        revision.cell(point(0, row));
    }
    if !ModeRevision::clone_enabled() {
        assert_ne!(revision.dictionary_generation(), generation);
    }
    assert_eq!(reader.cell(point(0, 0)), original);
    let mut text = String::new();
    reader.push_text(point(0, 0), &mut text);
    assert_eq!(text, cluster);
    reader.push_text(point(0, 1), &mut text);
    assert_eq!(text, format!("{cluster}r"));
    text.clear();
    reader.push_text(point(0, 0), &mut text);
    assert_eq!(text, cluster);
}
