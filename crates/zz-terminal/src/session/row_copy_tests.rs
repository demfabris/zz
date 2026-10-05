use super::*;
use crate::GRAPHEME_TABLE_BIT;

type CellFacts = (String, PackedStyle, u16);

fn per_cell_facts(terminal: &Terminal<'_, '_>) -> Vec<CellFacts> {
    let mut render_state = RenderState::new().expect("render state");
    let mut rows = RowIterator::new().expect("rows");
    let mut cells = CellIterator::new().expect("cells");
    let snapshot = render_state.update(terminal).expect("update");
    let colors = snapshot.colors().expect("colors");
    let default_palette = terminal.default_color_palette().expect("palette").0;
    let hints = ClassHints::default();
    let classes = Classifier::new(
        &hints,
        &colors.palette,
        &default_palette,
        [colors.foreground, colors.background],
    );
    let columns = usize::from(snapshot.cols().expect("cols"));
    let mut facts = Vec::new();
    let mut text = String::new();
    let mut row_iteration = rows.update(&snapshot).expect("rows");
    while let Some(row) = row_iteration.next() {
        let mut cell_iteration = cells.update(row).expect("cells");
        let mut column = 0;
        while let Some(cell) = cell_iteration.next() {
            if column >= columns {
                break;
            }
            let raw_style = cell.style().expect("style");
            let mut foreground = color(cell.fg_color().expect("fg").unwrap_or(colors.foreground));
            let mut background = color(cell.bg_color().expect("bg").unwrap_or(colors.background));
            if raw_style.inverse {
                std::mem::swap(&mut foreground, &mut background);
            }
            text.clear();
            cell.graphemes_utf8(&mut text).expect("text");
            let raw_cell = cell.raw_cell().expect("raw");
            let class = if raw_style.inverse {
                (ColourClass::Resolved, ColourClass::Resolved)
            } else {
                (
                    classes.ground(raw_style.fg_color, 0),
                    match raw_cell.content_tag().expect("tag") {
                        CellContentTag::BgColorPalette => {
                            classes.entry(raw_cell.bg_color_palette().expect("palette").0)
                        }
                        CellContentTag::BgColorRgb => ColourClass::Rgb,
                        CellContentTag::Codepoint | CellContentTag::CodepointGrapheme => {
                            classes.ground(raw_style.bg_color, 1)
                        }
                    },
                )
            };
            let width = match raw_cell.wide().expect("wide") {
                CellWide::Narrow => CellWidth::Narrow,
                CellWide::Wide => CellWidth::Wide,
                CellWide::SpacerTail => CellWidth::SpacerTail,
                CellWide::SpacerHead => CellWidth::SpacerHead,
            };
            let explicit_rgb = matches!(
                if raw_style.inverse {
                    raw_style.bg_color
                } else {
                    raw_style.fg_color
                },
                StyleColor::Rgb(_)
            );
            let style = PackedStyle::new(
                foreground,
                background,
                resolve_style_color(raw_style.underline_color, &colors.palette),
                style_attributes(
                    &raw_style,
                    explicit_rgb,
                    raw_cell.has_hyperlink().expect("hyperlink"),
                ),
                underline_style(raw_style.underline),
            )
            .with_classes(class.0, class.1);
            facts.push((text.clone(), style, PackedCell::new(0, 0, width).flags()));
            column += 1;
        }
    }
    facts
}

fn frame_facts(viewport: &TerminalViewport) -> Vec<CellFacts> {
    let dictionary = &viewport.dictionary;
    viewport
        .cells
        .iter()
        .map(|cell| {
            let glyph = cell.glyph();
            let text = if glyph & GRAPHEME_TABLE_BIT != 0 {
                let index = usize::try_from(glyph & !GRAPHEME_TABLE_BIT).expect("index");
                let start = usize::try_from(dictionary.grapheme_offsets[index]).expect("start");
                let end = usize::try_from(dictionary.grapheme_offsets[index + 1]).expect("end");
                std::str::from_utf8(&dictionary.grapheme_bytes[start..end])
                    .expect("utf8")
                    .to_owned()
            } else {
                char::from_u32(glyph)
                    .filter(|_| glyph != 0)
                    .map(String::from)
                    .unwrap_or_default()
            };
            (
                text,
                dictionary.styles[usize::from(cell.style_id())],
                cell.flags(),
            )
        })
        .collect()
}

const STYLED: &[u8] = b"\x1b[1;31mbold red\x1b[0m \x1b[3;38;5;200mitalic\x1b[0m \x1b[4:3;58;2;9;8;7mcurly\x1b[0m\r\n\
\x1b[7;38;2;1;2;3;48;5;17minverse rgb\x1b[0m \x1b[2;9;53mfaint strike over\x1b[0m \x1b[5;8mhidden\x1b[0m\r\n\
\x1b[48;5;4m\x1b[K\x1b[0mpalette erase\r\n\
\x1b[48;2;20;40;60m\x1b[K\x1b[0m\x1b[38;5;9mrgb erase\x1b[0m\r\n\
\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ \xe4\xb8\xad\xe6\x96\x87 e\xcc\x81 \x1b[?2027h\xf0\x9f\x91\xa8\xe2\x80\x8d\xf0\x9f\x91\xa9\x1b[?2027l\r\n\
\x1b[44m          \x1b[0m\x1b[45;1m  \x1b[0m end";

struct Fixture<'alloc> {
    render_state: RenderState<'alloc>,
    rows: RowIterator<'alloc>,
    cells: CellIterator<'alloc>,
    generations: ViewportGenerations,
    dictionary: ViewportDictionary,
}

impl<'alloc> Fixture<'alloc> {
    fn new() -> Self {
        Self {
            render_state: RenderState::new().expect("render state"),
            rows: RowIterator::new().expect("rows"),
            cells: CellIterator::new().expect("cells"),
            generations: ViewportGenerations::default(),
            dictionary: ViewportDictionary::default(),
        }
    }

    fn frame(&mut self, terminal: &Terminal<'alloc, '_>) -> TerminalViewport {
        self.frame_within(terminal, None)
    }

    fn frame_within(
        &mut self,
        terminal: &Terminal<'alloc, '_>,
        bound: Option<(u16, u16)>,
    ) -> TerminalViewport {
        snapshot(
            terminal,
            &mut self.render_state,
            &mut self.rows,
            &mut self.cells,
            &mut self.generations,
            SnapshotChange::Content,
            &mut self.dictionary,
            None,
            SessionStatus::Running,
            bound,
        )
        .expect("snapshot")
    }
}

fn top_left(facts: &[CellFacts], columns: u16, (width, height): (u16, u16)) -> Vec<CellFacts> {
    facts
        .chunks(usize::from(columns))
        .take(usize::from(height))
        .flat_map(|row| row[..usize::from(width)].iter().cloned())
        .collect()
}

#[test]
fn a_bounded_frame_is_the_top_left_region_of_the_terminal() {
    let mut terminal = new_terminal(40, 8, 100).expect("terminal");
    terminal.vt_write(STYLED);
    let full = Fixture::new().frame(&terminal);
    let mut fixture = Fixture::new();
    let bounded = fixture.frame_within(&terminal, Some((12, 3)));
    assert_eq!((bounded.columns, bounded.rows), (12, 3));
    assert_eq!(bounded.cells.len(), 36);
    assert_eq!(
        frame_facts(&bounded),
        top_left(&frame_facts(&full), 40, (12, 3))
    );
    assert!(full.cursor.is_some_and(|cursor| cursor.row() == 5));
    assert!(bounded.cursor.is_none());
    assert_eq!(bounded.scrollbar, full.scrollbar);

    terminal.vt_write(b"\x1b[2;3H");
    let moved = fixture.frame_within(&terminal, Some((12, 3)));
    assert_eq!(
        moved.cursor.map(|cursor| (cursor.column(), cursor.row())),
        Some((2, 1))
    );
    let roomy = fixture.frame_within(&terminal, Some((400, 300)));
    assert_eq!((roomy.columns, roomy.rows), (40, 8));
    assert_eq!(frame_facts(&roomy), frame_facts(&full));
}

#[test]
fn a_grown_bound_copies_the_rows_and_columns_it_skipped() {
    let mut terminal = new_terminal(40, 8, 100).expect("terminal");
    terminal.vt_write(STYLED);
    let mut fixture = Fixture::new();
    fixture.frame_within(&terminal, Some((10, 4)));
    terminal.vt_write(b"\x1b[7;30H\x1b[44mbeyond\x1b[0m\x1b[2;1H\x1b[1;32mnear\x1b[0m");
    let small = fixture.frame_within(&terminal, Some((10, 4)));
    let facts = frame_facts(&Fixture::new().frame(&terminal));
    assert_eq!(frame_facts(&small), top_left(&facts, 40, (10, 4)));
    for bound in [(20, 2), (40, 8), (10, 4), (40, 8), (3, 8)] {
        let frame = fixture.frame_within(&terminal, Some(bound));
        assert_eq!((frame.columns, frame.rows), bound);
        assert_eq!(
            frame_facts(&frame),
            top_left(&facts, 40, bound),
            "{bound:?}"
        );
    }
}

#[test]
fn row_copy_frames_match_the_per_cell_reads() {
    let mut terminal = new_terminal(40, 8, 100).expect("terminal");
    terminal.vt_write(STYLED);
    let mut fixture = Fixture::new();

    let full = fixture.frame(&terminal);
    assert_eq!(frame_facts(&full), per_cell_facts(&terminal));

    terminal.vt_write(b"\x1b[2;5H\x1b[42;30mpatch\x1b[0m\x1b[6;1H\x1b[2K\x1b[48;5;160m\x1b[K");
    let partial = fixture.frame(&terminal);
    assert_eq!(frame_facts(&partial), per_cell_facts(&terminal));

    terminal.vt_write(b"\r\nscroll one\r\nscroll two\r\n\x1b[1mscroll three\x1b[0m");
    let scrolled = fixture.frame(&terminal);
    assert_eq!(frame_facts(&scrolled), per_cell_facts(&terminal));
}

#[test]
fn row_copy_frames_keep_a_rainbow_row_exact() {
    let mut terminal = new_terminal(64, 2, 10).expect("terminal");
    let mut input = Vec::new();
    for index in 0..64_u8 {
        input.extend_from_slice(
            format!("\x1b[38;2;{};{};{}m#", index * 3, 255 - index, index).as_bytes(),
        );
    }
    terminal.vt_write(&input);
    let viewport = Fixture::new().frame(&terminal);
    assert_eq!(frame_facts(&viewport), per_cell_facts(&terminal));
    assert!(viewport.dictionary.styles.len() >= 65);
}
