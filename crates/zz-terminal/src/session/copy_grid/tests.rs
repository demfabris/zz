use std::cell::Cell;

use super::*;
use crate::session::SelectionMode;
use crate::session::mode_revision::{ModeRevision, ModeSelection};
use crate::session::{
    CaptureBoundary, CaptureOptions, HistorySearchSnapshot, SearchCase, SearchDirection,
    SearchMode, SearchQuery, capture_terminal, color, new_terminal,
};

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
        terminal.color_palette().expect("palette").0,
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
