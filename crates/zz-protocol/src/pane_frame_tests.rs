use std::sync::Arc;

use zz_terminal::{
    ATTR_BOLD, ATTR_HYPERLINK, ATTR_ITALIC, CellWidth, ColourClass, Cursor, CursorStyle,
    KittyLayer, KittyPlacement, OverlayKind, OverlaySpan, PackedCell, PackedStyle, ScrollbarState,
    SearchStatus, SessionStatus, TerminalDictionary, TerminalMode, TerminalPatchFields,
    TerminalViewport, TerminalViewportPatch, UnderlineStyle,
};

use super::*;
use crate::framing::{Lane, decode_enveloped, encode_enveloped};
use crate::{
    ChooserPreview, Event, EventPayload, ProtocolMessage, decode_protocol_frame,
    encode_protocol_message,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

fn narrow(glyph: char, style: u16) -> PackedCell {
    PackedCell::new(u32::from(glyph), style, CellWidth::Narrow)
}

fn text_row(viewport: &mut TerminalViewport, row: u16, column: u16, text: &str, style: u16) {
    let columns = usize::from(viewport.columns);
    let cells = Arc::make_mut(&mut viewport.cells);
    for (offset, glyph) in text.chars().enumerate() {
        cells[usize::from(row) * columns + usize::from(column) + offset] = narrow(glyph, style);
    }
}

fn event(payload: EventPayload) -> ProtocolMessage {
    ProtocolMessage::Event(Event {
        sequence: 3,
        payload,
    })
}

fn full(viewport: &TerminalViewport) -> Vec<u8> {
    encode_protocol_message(&event(EventPayload::TerminalViewport {
        pane: PaneId(4),
        viewport: viewport.clone(),
    }))
    .expect("full frame encodes")
}

fn patch_frame(patch: &TerminalViewportPatch) -> Vec<u8> {
    encode_protocol_message(&event(EventPayload::TerminalPatch {
        pane: PaneId(4),
        patch: patch.clone(),
    }))
    .expect("patch frame encodes")
}

fn decoded_viewport(frame: &[u8]) -> TerminalViewport {
    match decode_protocol_frame(frame).expect("full frame decodes") {
        ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalViewport { viewport, .. },
            ..
        }) => viewport,
        other => panic!("expected a full viewport, got {other:?}"),
    }
}

fn decoded_patch(frame: &[u8]) -> TerminalViewportPatch {
    match decode_protocol_frame(frame).expect("patch frame decodes") {
        ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalPatch { patch, .. },
            ..
        }) => patch,
        other => panic!("expected a patch, got {other:?}"),
    }
}

fn assert_patch_applies(previous: &TerminalViewport, current: &TerminalViewport) -> usize {
    let patch = TerminalViewport::diff(previous, current).expect("compatible frames");
    let mut direct = previous.clone();
    direct.apply_patch(patch.clone()).expect("the diff applies");
    assert_eq!(&direct, current);
    let frame = patch_frame(&patch);
    let mut retained = previous.clone();
    retained
        .apply_patch(decoded_patch(&frame))
        .expect("the decoded patch applies");
    assert_eq!(&retained, current);
    frame.len()
}

fn styled_dictionary(viewport: &mut TerminalViewport) {
    let mut styles = viewport.styles().to_vec();
    styles.push(PackedStyle::new(
        Color::rgb(200, 10, 20),
        viewport.background,
        None,
        ATTR_BOLD,
        UnderlineStyle::None,
    ));
    styles.push(
        PackedStyle::new(
            Color::rgb(1, 2, 3),
            Color::rgb(4, 5, 6),
            Some(Color::rgb(7, 8, 9)),
            ATTR_HYPERLINK | ATTR_ITALIC,
            UnderlineStyle::Curly,
        )
        .with_classes(ColourClass::Palette(12), ColourClass::Default),
    );
    styles.push(
        PackedStyle::new(
            Color::rgb(90, 90, 90),
            Color::rgb(0, 0, 80),
            None,
            0,
            UnderlineStyle::Dashed,
        )
        .with_classes(ColourClass::Rgb, ColourClass::Palette(255)),
    );
    let graphemes = [
        "e\u{301}",
        "\u{1f44d}\u{1f3fd}",
        "\u{1f468}\u{200d}\u{1f469}",
    ];
    let mut bytes = Vec::new();
    let mut offsets = vec![0_u32];
    for grapheme in graphemes {
        bytes.extend_from_slice(grapheme.as_bytes());
        offsets.push(u32::try_from(bytes.len()).expect("small arena"));
    }
    viewport.dictionary = Arc::new(TerminalDictionary::from_shared(
        styles.into(),
        offsets.into(),
        bytes.into(),
    ));
}

fn random_cell(rng: &mut Rng, styles: usize, graphemes: usize) -> [Option<PackedCell>; 2] {
    let style = if rng.chance(70) {
        0
    } else {
        u16::try_from(rng.below(styles)).expect("style fits")
    };
    match rng.below(12) {
        0..=3 => [
            Some(narrow(char::from(b' ' + rng.below(95) as u8), style)),
            None,
        ],
        4 => [Some(PackedCell::EMPTY), None],
        5 => [Some(PackedCell::new(0, style, CellWidth::Narrow)), None],
        6 => [
            Some(narrow(
                ['\u{e9}', '\u{2192}', '\u{2500}', '\u{3bb}'][rng.below(4)],
                style,
            )),
            None,
        ],
        7 => [
            Some(PackedCell::new(
                u32::from(['\u{4e2d}', '\u{6587}', '\u{1f600}'][rng.below(3)]),
                style,
                CellWidth::Wide,
            )),
            Some(PackedCell::new(0, style, CellWidth::SpacerTail)),
        ],
        8 if graphemes > 0 => [
            Some(PackedCell::new(
                GRAPHEME_TABLE_BIT | u32::try_from(rng.below(graphemes)).expect("index fits"),
                style,
                CellWidth::Narrow,
            )),
            None,
        ],
        9 if graphemes > 0 => [
            Some(PackedCell::new(
                GRAPHEME_TABLE_BIT | u32::try_from(rng.below(graphemes)).expect("index fits"),
                style,
                CellWidth::Wide,
            )),
            Some(PackedCell::new(0, style, CellWidth::SpacerTail)),
        ],
        10 => [Some(PackedCell::new(0, style, CellWidth::SpacerHead)), None],
        _ => [
            Some(PackedCell::from_raw(
                u32::from('x'),
                style,
                u16::try_from(rng.below(40)).expect("flags fit"),
            )),
            None,
        ],
    }
}

fn randomize_row(viewport: &mut TerminalViewport, rng: &mut Rng, row: usize) {
    let columns = usize::from(viewport.columns);
    let styles = viewport.styles().len();
    let graphemes = viewport.grapheme_offsets().len() - 1;
    let cells = Arc::make_mut(&mut viewport.cells);
    let line = &mut cells[row * columns..(row + 1) * columns];
    line.fill(PackedCell::EMPTY);
    let used = rng.below(columns + 1);
    let mut column = 0;
    while column < used {
        if rng.chance(10) {
            let run = rng.below(12).min(used - column);
            let cell = narrow(['-', '=', '\u{2500}'][rng.below(3)], 0);
            line[column..column + run].fill(cell);
            column += run;
            continue;
        }
        let [head, tail] = random_cell(rng, styles, graphemes);
        match (head, tail) {
            (Some(head), Some(tail)) if column + 1 < used => {
                line[column] = head;
                line[column + 1] = tail;
                column += 2;
            }
            (Some(head), None) => {
                line[column] = head;
                column += 1;
            }
            _ => column += 1,
        }
    }
}

#[test]
fn blank_screens_encode_in_a_few_dozen_bytes() {
    let viewport = TerminalViewport::blank(180, 50, SessionStatus::Running);
    let frame = full(&viewport);
    assert!(
        frame.len() <= 64,
        "blank 180x50 frame took {} bytes",
        frame.len()
    );
    assert!(72_000 / frame.len() >= 140);
    assert_eq!(decoded_viewport(&frame), viewport);
}

#[test]
fn echo_patches_stay_under_forty_bytes() {
    let mut previous = TerminalViewport::blank(120, 40, SessionStatus::Running);
    previous.generation = 90_210;
    previous.view_generation = 90_577;
    previous.dictionary_generation = 3;
    previous.scrollbar = ScrollbarState {
        total: 10_040,
        offset: 10_000,
        len: 40,
    };
    previous.set_title(Arc::from("user@host: ~/dev/zz"));
    previous.set_working_directory(Some(Arc::from("file://host/home/user/dev/zz")));
    text_row(&mut previous, 0, 0, "$ printf 'hello world'", 0);
    text_row(&mut previous, 1, 0, "hello world", 0);
    text_row(&mut previous, 23, 0, "$ ", 0);
    let cursor = |column| {
        Some(Cursor::new(
            column,
            23,
            true,
            false,
            false,
            CursorStyle::Block,
            Color::rgb(200, 200, 200),
        ))
    };
    previous.cursor = cursor(2);
    let mut current = previous.clone();
    current.generation += 1;
    current.view_generation += 1;
    text_row(&mut current, 23, 2, "\u{a7}001", 0);
    current.cursor = cursor(6);

    let patch = TerminalViewport::diff(&previous, &current).expect("compatible frames");
    assert_eq!(patch.fields, {
        let mut fields = TerminalPatchFields::ROWS;
        fields.insert(TerminalPatchFields::CURSOR_AT);
        fields
    });
    assert_eq!(patch.changed_rows.spans().len(), 1);
    assert_eq!(patch.changed_rows.cells().len(), 4);
    let bytes = assert_patch_applies(&previous, &current);
    assert!(bytes <= 40, "an echo took {bytes} bytes");
}

#[test]
fn patches_carry_only_the_metadata_that_changed() {
    let mut previous = TerminalViewport::blank(20, 4, SessionStatus::Running);
    previous.set_title(Arc::from("before"));
    previous.set_working_directory(Some(Arc::from("file://host/tmp")));
    let mut current = previous.clone();
    current.view_generation = 1;
    current.set_title(Arc::from("after"));
    let patch = TerminalViewport::diff(&previous, &current).expect("compatible frames");
    assert_eq!(patch.fields, TerminalPatchFields::PRESENTATION);
    let decoded = decoded_patch(&patch_frame(&patch));
    assert_eq!(decoded.fields, TerminalPatchFields::PRESENTATION);
    assert_eq!(decoded.title(), "after");
    assert_patch_applies(&previous, &current);

    let mut quiet = current.clone();
    quiet.view_generation = 2;
    let patch = TerminalViewport::diff(&current, &quiet).expect("compatible frames");
    assert_eq!(patch.fields, TerminalPatchFields::empty());
    let bytes = assert_patch_applies(&current, &quiet);
    assert!(bytes <= 20, "an empty patch took {bytes} bytes");

    let mut moved = quiet.clone();
    moved.view_generation = 3;
    moved.scrollbar = ScrollbarState {
        total: 30,
        offset: 26,
        len: 4,
    };
    moved.mode = TerminalMode::Copy {
        position: 3,
        total: 30,
        hide_position: true,
    };
    moved.search = Some(SearchStatus::new(1, 2).with_pending(true));
    moved.unseen_output = 7;
    moved.kitty_keyboard = true;
    moved.status = SessionStatus::exited(3, Some("TERM".to_owned()));
    moved.foreground = Color::rgb(1, 1, 1);
    moved.overlays = Arc::from([OverlaySpan::new(1, 2, 9, OverlayKind::SearchCurrent)]);
    let patch = TerminalViewport::diff(&quiet, &moved).expect("compatible frames");
    for field in [
        TerminalPatchFields::SCROLLBAR,
        TerminalPatchFields::MODE,
        TerminalPatchFields::SEARCH,
        TerminalPatchFields::UNSEEN,
        TerminalPatchFields::INPUT_MODES,
        TerminalPatchFields::STATUS,
        TerminalPatchFields::COLORS,
        TerminalPatchFields::OVERLAYS,
    ] {
        assert!(patch.carries(field), "missing {field:?}");
    }
    assert!(!patch.carries(TerminalPatchFields::PRESENTATION));
    assert_patch_applies(&quiet, &moved);
}

#[test]
fn cursor_moves_keep_the_base_appearance() {
    let mut previous = TerminalViewport::blank(10, 3, SessionStatus::Running);
    previous.cursor = Some(Cursor::new(
        0,
        0,
        true,
        true,
        false,
        CursorStyle::Underline,
        Color::rgb(9, 8, 7),
    ));
    let mut current = previous.clone();
    current.view_generation = 1;
    current.cursor = Some(Cursor::new(
        9,
        2,
        true,
        true,
        true,
        CursorStyle::Underline,
        Color::rgb(9, 8, 7),
    ));
    let patch = TerminalViewport::diff(&previous, &current).expect("compatible frames");
    assert_eq!(patch.fields, TerminalPatchFields::CURSOR_AT);
    assert_patch_applies(&previous, &current);

    let mut hidden = current.clone();
    hidden.view_generation = 2;
    hidden.cursor = None;
    let patch = TerminalViewport::diff(&current, &hidden).expect("compatible frames");
    assert_eq!(patch.fields, TerminalPatchFields::CURSOR);
    assert_patch_applies(&current, &hidden);

    let mut shown = hidden.clone();
    shown.view_generation = 3;
    shown.cursor = previous.cursor;
    assert_patch_applies(&hidden, &shown);

    let mut restyled = shown.clone();
    restyled.view_generation = 4;
    restyled.cursor = Some(Cursor::new(
        0,
        0,
        false,
        true,
        false,
        CursorStyle::BlockHollow,
        Color::rgb(9, 8, 7),
    ));
    let patch = TerminalViewport::diff(&shown, &restyled).expect("compatible frames");
    assert_eq!(patch.fields, TerminalPatchFields::CURSOR);
    assert_patch_applies(&shown, &restyled);

    let mut orphan = patch_frame(&TerminalViewport::diff(&previous, &current).expect("move"));
    let mut retained = hidden.clone();
    retained.generation = previous.generation;
    retained.view_generation = previous.view_generation;
    assert_eq!(
        retained.apply_patch(decoded_patch(&orphan)),
        Err(zz_terminal::PatchError::Metadata)
    );
    orphan.truncate(orphan.len() - 1);
    assert!(decode_protocol_frame(&orphan).is_err());
}

#[test]
fn spans_cover_changed_columns_and_clear_emptied_tails() {
    let mut previous = TerminalViewport::blank(40, 3, SessionStatus::Running);
    text_row(&mut previous, 0, 0, "the quick brown fox jumps", 0);
    text_row(&mut previous, 1, 0, "left", 0);
    text_row(&mut previous, 1, 30, "right side", 0);
    let mut current = previous.clone();
    current.generation = 1;
    current.view_generation = 1;
    text_row(&mut current, 0, 4, "QUICK", 1);
    let cells = Arc::make_mut(&mut current.cells);
    cells[40 + 2..80].fill(PackedCell::EMPTY);
    Arc::make_mut(&mut current.dictionary).styles = [
        current.styles()[0],
        PackedStyle::new(
            Color::rgb(5, 5, 5),
            current.background,
            None,
            ATTR_BOLD,
            UnderlineStyle::None,
        ),
    ]
    .into();
    let patch = TerminalViewport::diff(&previous, &current).expect("compatible frames");
    let spans = patch.changed_rows.spans();
    assert_eq!(spans.len(), 2);
    assert_eq!((spans[0].row, spans[0].start, spans[0].len), (0, 4, 5));
    assert!(!spans[0].clear);
    assert_eq!((spans[1].row, spans[1].start, spans[1].len), (1, 2, 0));
    assert!(spans[1].clear);
    assert_patch_applies(&previous, &current);
}

#[test]
fn scrolled_patches_replace_the_exposed_rows() {
    let mut previous = TerminalViewport::blank(12, 6, SessionStatus::Running);
    for row in 0..6 {
        text_row(&mut previous, row, 0, &format!("line {row}"), 0);
    }
    previous.scrollbar = ScrollbarState {
        total: 6,
        offset: 0,
        len: 6,
    };
    let mut current = previous.clone();
    current.generation = 1;
    current.view_generation = 1;
    let columns = 12;
    Arc::make_mut(&mut current.cells).copy_within(2 * columns..6 * columns, 0);
    Arc::make_mut(&mut current.cells)[4 * columns..].fill(PackedCell::EMPTY);
    text_row(&mut current, 4, 0, "line 6", 0);
    current.scrollbar = ScrollbarState {
        total: 8,
        offset: 2,
        len: 6,
    };
    let patch = TerminalViewport::diff(&previous, &current).expect("compatible frames");
    assert_eq!(patch.scroll, -2);
    assert_eq!(patch.changed_rows.row_indices().collect::<Vec<_>>(), [4, 5]);
    assert!(
        patch
            .changed_rows
            .spans()
            .iter()
            .all(|span| span.covers_row(12))
    );
    assert_patch_applies(&previous, &current);

    let mut broken = patch.clone();
    broken.changed_rows = zz_terminal::TerminalPatchRows::from_spans(
        patch.changed_rows.spans()[..1].iter().copied().collect(),
        patch.changed_rows.cells()[..usize::from(patch.changed_rows.spans()[0].len)].to_vec(),
    );
    assert!(matches!(
        encode_protocol_message(&event(EventPayload::TerminalPatch {
            pane: PaneId(1),
            patch: broken.clone(),
        })),
        Err(ProtocolError::InvalidTerminal(_))
    ));
    let mut retained = previous.clone();
    assert_eq!(
        retained.apply_patch(broken),
        Err(zz_terminal::PatchError::Row)
    );
    assert_eq!(retained, previous);
}

#[test]
fn random_grids_round_trip_through_full_and_patch_frames() {
    for seed in 1..=64_u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let columns = u16::try_from(1 + rng.below(140)).expect("columns fit");
        let rows = u16::try_from(1 + rng.below(40)).expect("rows fit");
        let mut previous = TerminalViewport::blank(columns, rows, SessionStatus::Running);
        previous.generation = rng.next() >> 20;
        previous.view_generation = previous.generation + rng.next() % 1000;
        previous.dictionary_generation = u32::try_from(rng.below(9)).expect("small");
        styled_dictionary(&mut previous);
        for row in 0..usize::from(rows) {
            if rng.chance(70) {
                randomize_row(&mut previous, &mut rng, row);
            }
        }
        let frame = full(&previous);
        assert_eq!(decoded_viewport(&frame), previous, "seed {seed}");
        assert!(
            frame.len() < previous.cells.len() * 8 / 2 + 256,
            "seed {seed}"
        );

        let mut current = previous.clone();
        for _ in 0..4 {
            current.generation = current.generation.wrapping_add(1);
            current.view_generation = current
                .view_generation
                .wrapping_add(1 + rng.below(3) as u64);
            if rng.chance(30) && rows > 2 {
                let shift = 1 + rng.below(usize::from(rows) - 1);
                let width = usize::from(columns);
                let cells = Arc::make_mut(&mut current.cells);
                cells.copy_within(shift * width.., 0);
                let len = cells.len();
                cells[len - shift * width..].fill(PackedCell::EMPTY);
            }
            if rng.chance(40) {
                let mut styles = current.styles().to_vec();
                styles.push(PackedStyle::new(
                    Color::rgb(rng.below(255) as u8, 3, 4),
                    Color::rgb(0, rng.below(255) as u8, 0),
                    None,
                    ATTR_BOLD,
                    UnderlineStyle::Single,
                ));
                let dictionary = Arc::make_mut(&mut current.dictionary);
                dictionary.styles = styles.into();
                let mut bytes = dictionary.grapheme_bytes.to_vec();
                bytes.extend_from_slice("a\u{308}".as_bytes());
                let mut offsets = dictionary.grapheme_offsets.to_vec();
                offsets.push(u32::try_from(bytes.len()).expect("small"));
                dictionary.grapheme_bytes = bytes.into();
                dictionary.grapheme_offsets = offsets.into();
            }
            for row in 0..usize::from(rows) {
                if rng.chance(25) {
                    randomize_row(&mut current, &mut rng, row);
                }
            }
            if rng.chance(30) {
                current.cursor = Some(Cursor::new(
                    u16::try_from(rng.below(usize::from(columns))).expect("fits"),
                    u16::try_from(rng.below(usize::from(rows))).expect("fits"),
                    true,
                    rng.chance(50),
                    false,
                    CursorStyle::Block,
                    Color::rgb(1, 2, 3),
                ));
            }
            if rng.chance(20) {
                current.overlays = Arc::from([OverlaySpan::new(
                    u16::try_from(rng.below(usize::from(rows))).expect("fits"),
                    0,
                    columns,
                    OverlayKind::Selection,
                )]);
            }
            if rng.chance(20) {
                current.set_title(Arc::from(format!("title {}", rng.below(100)).as_str()));
            }
            assert_patch_applies(&previous, &current);
            let frame = full(&current);
            assert_eq!(decoded_viewport(&frame), current, "seed {seed}");
            for cut in [1, frame.len() / 2, frame.len() - 1] {
                assert!(decode_protocol_frame(&frame[..cut]).is_err());
            }
            previous = current.clone();
        }
    }
}

#[test]
fn history_chunks_round_trip_with_blank_tails_dropped() {
    let columns = 180_u16;
    let mut dictionary_source = TerminalViewport::blank(1, 1, SessionStatus::Running);
    styled_dictionary(&mut dictionary_source);
    let dictionary = (*dictionary_source.dictionary).clone();
    let mut rng = Rng(77);
    let rows = (0..300)
        .map(|index| {
            let mut row = vec![PackedCell::EMPTY; usize::from(columns)];
            if index % 7 != 0 {
                for (column, glyph) in format!("history line {index}").chars().enumerate() {
                    row[column] = narrow(glyph, 0);
                }
                if rng.chance(20) {
                    row[40] = PackedCell::new(GRAPHEME_TABLE_BIT | 1, 2, CellWidth::Wide);
                    row[41] = PackedCell::new(0, 2, CellWidth::SpacerTail);
                }
            }
            row
        })
        .collect::<Vec<_>>();
    let message = event(EventPayload::HistoryChunk {
        pane: PaneId(9),
        start: 1000,
        total: 20_000,
        offset: 19_950,
        columns,
        rows: rows.clone(),
        dictionary,
    });
    let frame = encode_protocol_message(&message).expect("history chunk encodes");
    assert_eq!(frame[4], Lane::Terminal as u8);
    assert_eq!(frame[8], HISTORY_CHUNK);
    assert!(
        frame.len() < 300 * 20,
        "history chunk took {} bytes",
        frame.len()
    );
    assert_eq!(decode_protocol_frame(&frame).expect("decodes"), message);
    for cut in 0..frame.len() {
        assert!(decode_protocol_frame(&frame[..cut]).is_err());
    }

    let mut wide = rows;
    wide.push(vec![PackedCell::EMPTY; 3]);
    assert!(
        encode_protocol_message(&event(EventPayload::HistoryChunk {
            pane: PaneId(9),
            start: 0,
            total: 1,
            offset: 0,
            columns,
            rows: wide,
            dictionary: TerminalDictionary::default(),
        }))
        .is_err()
    );
}

#[test]
fn chooser_previews_carry_packed_viewports_through_postcard() {
    let mut viewport = TerminalViewport::blank(80, 24, SessionStatus::Running);
    styled_dictionary(&mut viewport);
    text_row(&mut viewport, 3, 2, "preview text", 1);
    let preview = ChooserPreview::Screen {
        viewport: viewport.clone(),
    };
    let bytes = postcard::to_stdvec(&preview).expect("preview encodes");
    assert!(bytes.len() < 200, "preview took {} bytes", bytes.len());
    assert_eq!(
        postcard::from_bytes::<ChooserPreview>(&bytes).expect("preview decodes"),
        preview
    );
    let client = ChooserPreview::Client {
        viewport: Some(viewport),
        status: vec!["[0] 0:bash*".to_owned()],
        status_style: String::new(),
        status_width: 80,
    };
    let bytes = postcard::to_stdvec(&client).expect("client preview encodes");
    assert_eq!(
        postcard::from_bytes::<ChooserPreview>(&bytes).expect("client preview decodes"),
        client
    );
    let empty = ChooserPreview::Client {
        viewport: None,
        status: Vec::new(),
        status_style: String::new(),
        status_width: 0,
    };
    let bytes = postcard::to_stdvec(&empty).expect("empty preview encodes");
    assert_eq!(
        postcard::from_bytes::<ChooserPreview>(&bytes).expect("empty preview decodes"),
        empty
    );
}

#[test]
fn decoders_reject_every_truncated_frame() {
    let mut viewport = TerminalViewport::blank(3, 2, SessionStatus::failed("cold status"));
    viewport.set_title(Arc::from("truncation fixture"));
    viewport.overlays = Arc::from([OverlaySpan::new(0, 0, 2, OverlayKind::Selection)]);
    styled_dictionary(&mut viewport);
    text_row(&mut viewport, 1, 0, "abc", 2);
    let frame = full(&viewport);
    for cut in 0..frame.len() {
        assert!(
            decode_protocol_frame(&frame[..cut]).is_err(),
            "full viewport prefix of {cut} bytes was accepted"
        );
    }

    let previous = TerminalViewport::blank(2, 2, SessionStatus::Running);
    let mut current = previous.clone();
    current.generation = 1;
    current.view_generation = 1;
    current.set_title(Arc::from("patch fixture"));
    current.status = SessionStatus::failed("patch status");
    styled_dictionary(&mut current);
    Arc::make_mut(&mut current.cells)[0] =
        PackedCell::new(GRAPHEME_TABLE_BIT, 1, CellWidth::Narrow);
    current.overlays = Arc::from([OverlaySpan::new(0, 0, 1, OverlayKind::SearchCurrent)]);
    let patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
    let frame = patch_frame(&patch);
    for cut in 0..frame.len() {
        assert!(
            decode_protocol_frame(&frame[..cut]).is_err(),
            "viewport patch prefix of {cut} bytes was accepted"
        );
    }
}

#[test]
fn decoders_reject_invalid_utf8_in_every_text_field() {
    let title = "title-utf8-unique-41";
    let working_directory = "file://localhost/tmp/utf8-unique-42";
    let uri = "https://utf8.example/unique-43";
    let status = "status-utf8-unique-44";
    let cells = "cells-utf8-unique-45";
    let mut viewport = TerminalViewport::blank(24, 1, SessionStatus::failed(status));
    viewport.set_title(Arc::from(title));
    viewport.set_working_directory(Some(Arc::from(working_directory)));
    viewport.set_hovered_uri(Some(Arc::from(uri)));
    text_row(&mut viewport, 0, 0, cells, 0);
    let full_frame = full(&viewport);

    let previous = TerminalViewport::blank(24, 1, SessionStatus::Running);
    let mut current = viewport.clone();
    current.generation = 1;
    current.view_generation = 1;
    let patch = patch_frame(&TerminalViewport::diff(&previous, &current).expect("compatible"));

    let markers: [(&str, &[u8]); 5] = [
        ("title", title.as_bytes()),
        ("working directory", working_directory.as_bytes()),
        ("hovered URI", uri.as_bytes()),
        ("status", status.as_bytes()),
        ("cells", cells.as_bytes()),
    ];
    for (kind, frame) in [("full viewport", full_frame), ("viewport patch", patch)] {
        for (field, marker) in markers {
            let mut malformed = frame.clone();
            let offset = malformed
                .windows(marker.len())
                .position(|window| window == marker)
                .unwrap_or_else(|| panic!("missing {field} marker in {kind}"));
            malformed[offset] = 0xff;
            assert!(
                matches!(
                    decode_protocol_frame(&malformed),
                    Err(ProtocolError::InvalidTerminal(_))
                ),
                "{kind} accepted invalid UTF-8 in {field}"
            );
        }
    }
}

fn terminal_frame(payload: &[u8]) -> Vec<u8> {
    encode_enveloped(Lane::Terminal, payload).expect("envelope")
}

#[test]
fn decoders_reject_hand_built_frames_that_lie() {
    let full_header = |fields: u8| vec![FULL_VIEWPORT, 1, 1, 5, 0, 0, 4, 2, fields];
    let dictionary = [1, 0, 0, 0, 0, 0, 0, 0, 0];

    let mut good = full_header(0);
    good.extend_from_slice(&dictionary);
    good.extend_from_slice(&[2, 2, 0x10, b'a', b'b', 0, 0]);
    let decoded = decode_protocol_frame(&terminal_frame(&good)).expect("hand-built frame");
    let ProtocolMessage::Event(Event {
        payload: EventPayload::TerminalViewport { viewport, .. },
        ..
    }) = decoded
    else {
        panic!("expected a viewport");
    };
    assert_eq!(viewport.cell(1, 1), Some(narrow('a', 0)));
    assert_eq!(viewport.cell(1, 2), Some(narrow('b', 0)));
    assert_eq!(viewport.cell(1, 3), Some(PackedCell::EMPTY));

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("row past the grid", {
            let mut frame = full_header(0);
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[3, 0, 0x08, b'a', 0, 0]);
            frame
        }),
        ("run past the row", {
            let mut frame = full_header(0);
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[1, 4, 0x18, b'a', b'b', b'c', 0, 0]);
            frame
        }),
        ("missing style", {
            let mut frame = full_header(0);
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[1, 0, 0x0c, 3, b'a', 0, 0]);
            frame
        }),
        ("missing grapheme", {
            let mut frame = full_header(0);
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[1, 0, 0x0b, 1, 0, 0, 0]);
            frame
        }),
        ("empty run", {
            let mut frame = full_header(0);
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[1, 0, 0x01, 0, 0]);
            frame
        }),
        ("unknown fields", {
            let mut frame = vec![FULL_VIEWPORT, 1, 1, 5, 0, 0, 4, 2, 0x80, 0x80, 0x02];
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[0]);
            frame
        }),
        ("patch-only field in a full frame", {
            let mut frame = full_header(1);
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[0]);
            frame
        }),
        ("overlay count past the frame", {
            let mut frame = full_header(0x20);
            frame.extend_from_slice(&[0x80, 0x80, 0x40]);
            frame
        }),
        ("style count past the frame", {
            let mut frame = full_header(0);
            frame.extend_from_slice(&[0x80, 0x80, 0x04]);
            frame
        }),
        ("varint overflow", {
            let mut frame = vec![FULL_VIEWPORT, 1];
            frame.extend_from_slice(&[0xff; 11]);
            frame
        }),
        ("a grid past the cell limit", {
            let mut frame = vec![
                FULL_VIEWPORT,
                1,
                1,
                5,
                0,
                0,
                0xff,
                0xff,
                0x03,
                0xff,
                0xff,
                0x03,
                0,
            ];
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[0]);
            frame
        }),
        ("a history chunk past the cell limit", {
            let mut frame = vec![HISTORY_CHUNK, 1, 1, 0, 0, 0, 0xff, 0xff, 0x03, 0x80, 0x04];
            frame.extend_from_slice(&dictionary);
            frame.extend_from_slice(&[0]);
            frame
        }),
        ("trailing bytes", {
            let mut frame = good.clone();
            frame.push(0);
            frame
        }),
        (
            "patch moves and replaces the cursor",
            vec![VIEWPORT_PATCH, 1, 1, 5, 0, 0, 0, 0, 4, 2, 0x42, 0, 0, 0],
        ),
        (
            "patch places images without a scrollbar",
            vec![
                VIEWPORT_PATCH,
                1,
                1,
                5,
                0,
                0,
                0,
                0,
                4,
                2,
                0x80,
                0x80,
                0x01,
                0,
            ],
        ),
        (
            "patch scroll without replacement rows",
            vec![VIEWPORT_PATCH, 1, 1, 5, 0, 0, 0, 0, 4, 2, 0x04, 1],
        ),
    ];
    for (name, payload) in cases {
        assert!(
            decode_protocol_frame(&terminal_frame(&payload)).is_err(),
            "accepted a frame with {name}"
        );
    }
    assert!(matches!(
        decode_protocol_frame(&terminal_frame(&{
            let mut frame = full_header(0x20);
            frame.extend_from_slice(&[0x80, 0x80, 0x40]);
            frame
        })),
        Err(ProtocolError::Truncated)
    ));
}

#[test]
fn encoders_reject_what_decoders_would() {
    let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
    Arc::make_mut(&mut viewport.cells)[0] = narrow('x', 8);
    assert!(matches!(
        encode_protocol_message(&event(EventPayload::TerminalViewport {
            pane: PaneId(1),
            viewport,
        })),
        Err(ProtocolError::InvalidTerminal(_))
    ));

    for (hovered, directory) in [
        (Some("https://example.com/not allowed"), None),
        (None, Some("file://localhost/tmp/not\nallowed")),
    ] {
        let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        viewport.set_hovered_uri(hovered.map(Arc::from));
        viewport.set_working_directory(directory.map(Arc::from));
        assert!(matches!(
            encode_protocol_message(&event(EventPayload::TerminalViewport {
                pane: PaneId(1),
                viewport,
            })),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }

    let previous = TerminalViewport::blank(1, 3, SessionStatus::Running);
    let mut current = previous.clone();
    current.generation = 1;
    current.view_generation = 1;
    let cells = Arc::make_mut(&mut current.cells);
    cells[0] = narrow('a', 0);
    cells[2] = narrow('c', 0);
    let mut patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
    let reversed = patch
        .changed_rows
        .spans()
        .iter()
        .rev()
        .copied()
        .collect::<zz_terminal::TerminalPatchSpans>();
    let cells = patch.changed_rows.cells().to_vec();
    patch.changed_rows = zz_terminal::TerminalPatchRows::from_spans(reversed, cells);
    assert!(matches!(
        encode_protocol_message(&event(EventPayload::TerminalPatch {
            pane: PaneId(1),
            patch,
        })),
        Err(ProtocolError::InvalidTerminal(_))
    ));
}

#[test]
fn statuses_modes_and_searches_round_trip() {
    for status in [
        SessionStatus::Starting,
        SessionStatus::Running,
        SessionStatus::exited(7, Some("TERM".to_owned())),
        SessionStatus::exited(0, Some(String::new())),
        SessionStatus::exited(u32::MAX, None),
        SessionStatus::failed("boom"),
    ] {
        let mut viewport = TerminalViewport::blank(2, 2, status);
        viewport.mode = TerminalMode::View {
            position: 1,
            total: u32::MAX,
        };
        viewport.search = Some(
            SearchStatus::new(2, 5)
                .with_pending(true)
                .with_invalid_pattern(true),
        );
        viewport.unseen_output = u32::MAX;
        viewport.mouse_tracking = true;
        viewport.set_working_directory(Some(Arc::from("")));
        assert_eq!(decoded_viewport(&full(&viewport)), viewport);
    }
    let mut exited = Vec::new();
    encode_status(
        &mut exited,
        &SessionStatus::exited(7, Some("TERM".to_owned())),
    )
    .expect("status encodes");
    assert_eq!(exited, b"\x02\x07\x05TERM");
}

#[test]
fn kitty_placements_round_trip_densely_and_reject_malformed_records() {
    for (columns, rows) in [(32, 17), (256, 256)] {
        let previous = TerminalViewport::blank(columns, rows, SessionStatus::Running);
        let mut current = previous.clone();
        current.generation = 1;
        current.view_generation = 1;
        current.scrollbar = ScrollbarState {
            total: u32::from(rows),
            offset: 0,
            len: u32::from(rows),
        };
        current.kitty_placements = (0..rows)
            .flat_map(|row| {
                (0..columns).map(move |column| KittyPlacement {
                    image_id: 42,
                    image_generation: 1,
                    layer: KittyLayer::AboveText,
                    viewport_col: i32::from(column),
                    viewport_row: i32::from(row),
                    absolute_row: u64::from(row),
                    cell_offset_x: 0,
                    cell_offset_y: 0,
                    grid_cols: 1,
                    grid_rows: 1,
                    pixel_width: 8,
                    pixel_height: 18,
                    source_rect: Some((u32::from(column) * 8, u32::from(row) * 18, 8, 18)),
                })
            })
            .collect();
        assert!(current.kitty_placements.len() <= MAX_KITTY_PLACEMENTS);
        assert_eq!(decoded_viewport(&full(&current)), current);
        assert_patch_applies(&previous, &current);
    }

    let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
    viewport.kitty_placements = Arc::from([KittyPlacement {
        image_id: 1,
        image_generation: 1,
        layer: KittyLayer::AboveText,
        viewport_col: 0,
        viewport_row: 0,
        absolute_row: 0,
        cell_offset_x: 0,
        cell_offset_y: 0,
        grid_cols: 1,
        grid_rows: 1,
        pixel_width: 1,
        pixel_height: 1,
        source_rect: None,
    }]);
    let mut oversized = viewport.clone();
    oversized.kitty_placements =
        vec![viewport.kitty_placements[0].clone(); MAX_KITTY_PLACEMENTS + 1].into();
    assert!(matches!(
        encode_protocol_message(&event(EventPayload::TerminalViewport {
            pane: PaneId(1),
            viewport: oversized,
        })),
        Err(ProtocolError::InvalidTerminal(error))
            if error == "kitty placement count exceeds its wire limit"
    ));
    let frame = full(&viewport);
    let (_, payload) = decode_enveloped(&frame).expect("envelope");
    let record = payload
        .windows(4)
        .position(|window| window == 1_u32.to_le_bytes())
        .expect("image id");
    let mut malformed = frame.clone();
    malformed[8 + record + 12] = 9;
    assert!(matches!(
        decode_protocol_frame(&malformed),
        Err(ProtocolError::InvalidTerminal(_))
    ));
}

#[test]
fn full_frames_round_trip_every_metadata_field() {
    let mut viewport = TerminalViewport::blank(2, 1, SessionStatus::Running);
    viewport.generation = 42;
    viewport.view_generation = 7;
    viewport.dictionary_generation = 3;
    viewport.scrollbar = ScrollbarState {
        total: u32::MAX,
        offset: u32::MAX - 1,
        len: 1,
    };
    viewport.mode = TerminalMode::Copy {
        position: u32::MAX,
        total: u32::MAX,
        hide_position: true,
    };
    viewport.unseen_output = u32::MAX;
    viewport.set_working_directory(Some(Arc::from("file://localhost/tmp/terminal-fixture")));
    viewport.set_hovered_uri(Some(Arc::from("https://example.com/terminal")));
    styled_dictionary(&mut viewport);
    Arc::make_mut(&mut viewport.cells)[0] = narrow('A', 1);
    viewport.search = Some(
        SearchStatus::new(2, 5)
            .with_pending(true)
            .with_invalid_pattern(true),
    );
    viewport.cursor = Some(Cursor::new(
        1,
        0,
        true,
        true,
        false,
        CursorStyle::Block,
        Color::rgb(7, 8, 9),
    ));
    viewport.overlays = Arc::from([OverlaySpan::new(0, 0, 1, OverlayKind::Selection)]);
    viewport.kitty_placements = Arc::from([KittyPlacement {
        image_id: 17,
        image_generation: 9,
        layer: KittyLayer::BelowText,
        viewport_col: 0,
        viewport_row: 0,
        absolute_row: u64::from(u32::MAX - 1),
        cell_offset_x: 2,
        cell_offset_y: 3,
        grid_cols: 1,
        grid_rows: 1,
        pixel_width: 20,
        pixel_height: 10,
        source_rect: Some((1, 2, 8, 6)),
    }]);
    viewport.status = SessionStatus::failed("renderer disconnected");
    viewport.kitty_keyboard = true;
    assert_eq!(decoded_viewport(&full(&viewport)), viewport);

    let mut output = viewport.clone();
    output.kitty_placements = Arc::from([]);
    let message = event(EventPayload::CommandOutput {
        pane: PaneId(12),
        output_id: 0x0123_4567_89ab_cdef,
        viewport: Some(output),
    });
    let frame = encode_protocol_message(&message).expect("command output encodes");
    assert_eq!(decode_protocol_frame(&frame).expect("decodes"), message);
}
