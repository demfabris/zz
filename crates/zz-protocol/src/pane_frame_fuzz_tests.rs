use std::sync::Arc;

use zz_terminal::{
    ATTR_BOLD, ATTR_HYPERLINK, CellWidth, ColourClass, Cursor, CursorStyle, KittyLayer,
    KittyPlacement, OverlayKind, OverlaySpan, PackedCell, PackedStyle, ScrollbarState,
    SearchStatus, SessionStatus, TerminalDictionary, TerminalDiffScratch, TerminalMode,
    TerminalViewport, TerminalViewportPatch, UnderlineStyle,
};

use super::*;
use crate::framing::decode_enveloped;
use crate::{Event, EventPayload, ProtocolMessage, decode_protocol_frame, encode_protocol_message};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next() % bound as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

fn event(payload: EventPayload) -> ProtocolMessage {
    ProtocolMessage::Event(Event {
        sequence: 9,
        payload,
    })
}

fn full_frame(viewport: &TerminalViewport) -> Vec<u8> {
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

fn decode_full(frame: &[u8]) -> TerminalViewport {
    match decode_protocol_frame(frame).expect("full decodes") {
        ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalViewport { viewport, .. },
            ..
        }) => viewport,
        other => panic!("expected full, got {other:?}"),
    }
}

fn decode_patch(frame: &[u8]) -> TerminalViewportPatch {
    match decode_protocol_frame(frame).expect("patch decodes") {
        ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalPatch { patch, .. },
            ..
        }) => patch,
        other => panic!("expected patch, got {other:?}"),
    }
}

fn dictionary_with(styles: usize, graphemes: &[&str]) -> TerminalDictionary {
    let mut list = vec![PackedStyle::new(
        Color::rgb(200, 200, 200),
        Color::rgb(0, 0, 0),
        None,
        0,
        UnderlineStyle::None,
    )];
    for index in 1..styles {
        let byte = u8::try_from(index % 250).expect("small");
        let style = PackedStyle::new(
            Color::rgb(byte, 3, 4),
            Color::rgb(4, byte, 9),
            (index % 3 == 0).then(|| Color::rgb(1, 2, byte)),
            if index % 2 == 0 {
                ATTR_HYPERLINK
            } else {
                ATTR_BOLD
            },
            if index % 4 == 0 {
                UnderlineStyle::Curly
            } else {
                UnderlineStyle::None
            },
        );
        list.push(if index % 5 == 0 {
            style.with_classes(ColourClass::Palette(byte), ColourClass::Rgb)
        } else {
            style
        });
    }
    let mut bytes = Vec::new();
    let mut offsets = vec![0_u32];
    for grapheme in graphemes {
        bytes.extend_from_slice(grapheme.as_bytes());
        offsets.push(u32::try_from(bytes.len()).expect("small"));
    }
    TerminalDictionary::from_shared(list.into(), offsets.into(), bytes.into())
}

fn random_cells(rng: &mut Rng, styles: usize, graphemes: usize, line: &mut [PackedCell]) {
    line.fill(PackedCell::EMPTY);
    let width = line.len();
    let used = if rng.chance(20) {
        width
    } else {
        rng.below(width + 1)
    };
    let mut column = if rng.chance(20) { rng.below(width) } else { 0 };
    while column < used {
        let style = if rng.chance(60) {
            0
        } else {
            u16::try_from(rng.below(styles)).expect("fits")
        };
        match rng.below(16) {
            0..=4 => {
                line[column] = PackedCell::new(
                    u32::from(char::from(
                        b' ' + u8::try_from(rng.below(95)).expect("ascii"),
                    )),
                    style,
                    CellWidth::Narrow,
                );
                column += 1;
            }
            5 => {
                line[column] = PackedCell::new(
                    0,
                    style.max(1).min(u16::try_from(styles - 1).unwrap_or(0)),
                    CellWidth::Narrow,
                );
                column += 1;
            }
            6 => {
                let run = rng.below(20).min(used - column);
                line[column..column + run].fill(PackedCell::new(
                    u32::from('\u{2500}'),
                    style,
                    CellWidth::Narrow,
                ));
                column += run.max(1);
            }
            7 if column + 1 < width => {
                line[column] = PackedCell::new(u32::from('\u{4e2d}'), style, CellWidth::Wide);
                line[column + 1] = PackedCell::new(0, style, CellWidth::SpacerTail);
                column += 2;
            }
            8 if column + 1 < width && graphemes > 0 => {
                line[column] = PackedCell::new(
                    GRAPHEME_TABLE_BIT | u32::try_from(rng.below(graphemes)).expect("fits"),
                    style,
                    CellWidth::Wide,
                );
                line[column + 1] = PackedCell::new(0, style, CellWidth::SpacerTail);
                column += 2;
            }
            9 if graphemes > 0 => {
                line[column] = PackedCell::new(
                    GRAPHEME_TABLE_BIT | u32::try_from(rng.below(graphemes)).expect("fits"),
                    style,
                    CellWidth::Narrow,
                );
                column += 1;
            }
            10 => {
                line[column] = PackedCell::new(0, style, CellWidth::SpacerHead);
                column += 1;
            }
            11 => {
                line[column] = PackedCell::new(u32::from('\u{1f600}'), style, CellWidth::Wide);
                column += 1;
            }
            12 => {
                line[column] = PackedCell::new(0, style, CellWidth::SpacerTail);
                column += 1;
            }
            13 => {
                line[column] = PackedCell::from_raw(
                    u32::from('\u{10ffff}'),
                    style,
                    u16::try_from(rng.below(0x1_0000)).expect("fits"),
                );
                column += 1;
            }
            14 => {
                let run = (6 + rng.below(30)).min(used - column);
                line[column..column + run].fill(PackedCell::new(0, style, CellWidth::Narrow));
                column += run.max(1);
            }
            _ => {
                line[column] = PackedCell::new(u32::from('\u{7f}'), style, CellWidth::Narrow);
                column += 1;
            }
        }
    }
}

fn random_row(viewport: &mut TerminalViewport, rng: &mut Rng, row: usize) {
    let columns = usize::from(viewport.columns);
    let styles = viewport.styles().len();
    let graphemes = viewport.grapheme_offsets().len() - 1;
    let cells = Arc::make_mut(&mut viewport.cells);
    random_cells(
        rng,
        styles,
        graphemes,
        &mut cells[row * columns..(row + 1) * columns],
    );
}

fn random_cursor(rng: &mut Rng, viewport: &TerminalViewport) -> Cursor {
    Cursor::new(
        u16::try_from(rng.below(usize::from(viewport.columns))).expect("fits"),
        u16::try_from(rng.below(usize::from(viewport.rows))).expect("fits"),
        rng.chance(80),
        rng.chance(50),
        rng.chance(10),
        [
            CursorStyle::Bar,
            CursorStyle::Block,
            CursorStyle::Underline,
            CursorStyle::BlockHollow,
        ][rng.below(4)],
        Color::rgb(
            u8::try_from(rng.below(256)).expect("byte"),
            7,
            u8::try_from(rng.below(256)).expect("byte"),
        ),
    )
}

fn mutate_metadata(viewport: &mut TerminalViewport, rng: &mut Rng) {
    let rows = viewport.rows;
    let columns = viewport.columns;
    if rng.chance(35) {
        viewport.cursor = match (viewport.cursor, rng.below(4)) {
            (_, 0) => None,
            (Some(cursor), 1 | 2) => Some(Cursor::new(
                u16::try_from(rng.below(usize::from(columns))).expect("fits"),
                u16::try_from(rng.below(usize::from(rows))).expect("fits"),
                cursor.visible(),
                cursor.blinking(),
                rng.chance(20),
                cursor.style(),
                cursor.color(),
            )),
            _ => Some(random_cursor(rng, viewport)),
        };
    }
    if rng.chance(15) {
        viewport.overlays = if rng.chance(30) {
            Arc::from([])
        } else {
            (0..rng.below(4))
                .map(|_| {
                    let start = u16::try_from(rng.below(usize::from(columns))).expect("fits");
                    OverlaySpan::new(
                        u16::try_from(rng.below(usize::from(rows))).expect("fits"),
                        start,
                        start
                            + u16::try_from(rng.below(usize::from(columns - start) + 1))
                                .expect("fits"),
                        [
                            OverlayKind::Selection,
                            OverlayKind::SearchCurrent,
                            OverlayKind::CopyCursor,
                        ][rng.below(3)],
                    )
                })
                .collect()
        };
    }
    if rng.chance(15) {
        viewport.set_title(Arc::from(if rng.chance(20) {
            String::new()
        } else {
            format!("t\u{e9}tle {}", rng.below(9))
        }));
    }
    if rng.chance(10) {
        viewport.set_working_directory(if rng.chance(30) {
            None
        } else {
            Some(Arc::from(format!("file://h/tmp/{}", rng.below(5)).as_str()))
        });
    }
    if rng.chance(10) {
        viewport.set_hovered_uri(if rng.chance(50) {
            None
        } else {
            Some(Arc::from(
                format!("https://x.test/{}", rng.below(5)).as_str(),
            ))
        });
    }
    if rng.chance(8) {
        if rng.chance(30) {
            viewport.foreground = Color::default();
            viewport.background = Color::default();
        } else {
            viewport.foreground = Color::rgb(u8::try_from(rng.below(256)).expect("b"), 1, 2);
        }
    }
    if rng.chance(12) {
        viewport.mode = match rng.below(3) {
            0 => TerminalMode::Live,
            1 => {
                let total = 1 + u32::try_from(rng.below(500)).expect("fits");
                TerminalMode::Copy {
                    position: 1 + u32::try_from(rng.below(total as usize)).expect("fits"),
                    total,
                    hide_position: rng.chance(50),
                }
            }
            _ => {
                let total = 1 + u32::try_from(rng.below(500)).expect("fits");
                TerminalMode::View {
                    position: 1 + u32::try_from(rng.below(total as usize)).expect("fits"),
                    total,
                }
            }
        };
    }
    if rng.chance(12) {
        viewport.search = if rng.chance(40) {
            None
        } else {
            let total = u32::try_from(rng.below(40)).expect("fits");
            Some(
                SearchStatus::new(
                    u32::try_from(rng.below(total as usize + 1)).expect("fits"),
                    total,
                )
                .with_pending(rng.chance(50))
                .with_invalid_pattern(rng.chance(20)),
            )
        };
    }
    if rng.chance(12) {
        viewport.unseen_output = if rng.chance(40) {
            0
        } else {
            u32::try_from(rng.below(1000)).expect("fits")
        };
    }
    if rng.chance(8) {
        viewport.kitty_keyboard = rng.chance(50);
        viewport.mouse_tracking = rng.chance(50);
    }
    if rng.chance(6) {
        viewport.status = match rng.below(4) {
            0 => SessionStatus::Starting,
            1 => SessionStatus::Running,
            2 => SessionStatus::exited(
                u32::try_from(rng.below(256)).expect("fits"),
                rng.chance(50).then(|| "TERM".to_owned()),
            ),
            _ => SessionStatus::failed("boom"),
        };
    }
    if rng.chance(15) {
        let total = u32::from(rows) + u32::try_from(rng.below(300)).expect("fits");
        let offset =
            u32::try_from(rng.below((total - u32::from(rows)) as usize + 1)).expect("fits");
        viewport.scrollbar = if rng.chance(20) {
            ScrollbarState::default()
        } else {
            ScrollbarState {
                total,
                offset,
                len: u32::from(rows),
            }
        };
        viewport.kitty_placements = Arc::from([]);
    }
    if rng.chance(8) {
        let offset = viewport.scrollbar.offset;
        viewport.kitty_placements = if rng.chance(30) {
            Arc::from([])
        } else {
            (0..=rng.below(3))
                .map(|index| {
                    let row = i32::try_from(rng.below(usize::from(rows))).expect("fits");
                    KittyPlacement {
                        image_id: 1 + u32::try_from(index).expect("fits"),
                        image_generation: 1 + rng.next() % 5,
                        layer: [
                            KittyLayer::BelowBg,
                            KittyLayer::BelowText,
                            KittyLayer::AboveText,
                        ][rng.below(3)],
                        viewport_col: i32::try_from(rng.below(usize::from(columns))).expect("fits"),
                        viewport_row: row,
                        absolute_row: u64::from(offset) + u64::try_from(row).expect("fits"),
                        cell_offset_x: 0,
                        cell_offset_y: 1,
                        grid_cols: 2,
                        grid_rows: 1,
                        pixel_width: 16,
                        pixel_height: 18,
                        source_rect: rng.chance(50).then_some((0, 0, 16, 18)),
                    }
                })
                .collect()
        };
    }
}

fn grow_dictionary(viewport: &mut TerminalViewport, rng: &mut Rng) {
    let dictionary = Arc::make_mut(&mut viewport.dictionary);
    if rng.chance(70) {
        let mut styles = dictionary.styles.to_vec();
        for _ in 0..=rng.below(3) {
            styles.push(PackedStyle::new(
                Color::rgb(u8::try_from(rng.below(256)).expect("b"), 9, 9),
                Color::rgb(9, u8::try_from(rng.below(256)).expect("b"), 9),
                rng.chance(30).then(|| Color::rgb(5, 5, 5)),
                if rng.chance(50) { ATTR_HYPERLINK } else { 0 },
                UnderlineStyle::Dotted,
            ));
        }
        dictionary.styles = styles.into();
    }
    if rng.chance(60) {
        let mut bytes = dictionary.grapheme_bytes.to_vec();
        let mut offsets = dictionary.grapheme_offsets.to_vec();
        for _ in 0..=rng.below(2) {
            bytes.extend_from_slice(
                ["o\u{308}", "\u{1f1e7}\u{1f1f7}", "\u{2764}\u{fe0f}"][rng.below(3)].as_bytes(),
            );
            offsets.push(u32::try_from(bytes.len()).expect("small"));
        }
        dictionary.grapheme_bytes = bytes.into();
        dictionary.grapheme_offsets = offsets.into();
    }
}

fn scroll(viewport: &mut TerminalViewport, rng: &mut Rng) {
    let rows = usize::from(viewport.rows);
    if rows < 2 {
        return;
    }
    let width = usize::from(viewport.columns);
    let shift = 1 + rng.below(rows - 1);
    let cells = Arc::make_mut(&mut viewport.cells);
    if rng.chance(50) {
        cells.copy_within(shift * width.., 0);
        let len = cells.len();
        cells[len - shift * width..].fill(PackedCell::EMPTY);
    } else {
        let len = cells.len();
        cells.copy_within(..len - shift * width, shift * width);
        cells[..shift * width].fill(PackedCell::EMPTY);
    }
}

fn fresh(rng: &mut Rng, generation: u64) -> TerminalViewport {
    let columns = u16::try_from(1 + rng.below(160)).expect("fits");
    let rows = u16::try_from(1 + rng.below(50)).expect("fits");
    let mut viewport = TerminalViewport::blank(columns, rows, SessionStatus::Running);
    viewport.dictionary = Arc::new(dictionary_with(
        1 + rng.below(12),
        &[
            "e\u{301}",
            "\u{1f44d}\u{1f3fd}",
            "\u{1f468}\u{200d}\u{1f469}",
        ][..rng.below(4)],
    ));
    viewport.generation = generation;
    viewport.view_generation = generation;
    viewport.dictionary_generation = u32::try_from(rng.below(1000)).expect("fits");
    for row in 0..usize::from(rows) {
        if rng.chance(60) {
            random_row(&mut viewport, rng, row);
        }
    }
    mutate_metadata(&mut viewport, rng);
    viewport
}

fn payload(frame: &[u8]) -> Vec<u8> {
    let (_, payload) = decode_enveloped(frame).expect("envelope");
    payload.as_ref().to_vec()
}

#[test]
fn chained_patches_keep_client_state_equal_to_daemon_state() {
    let mut patches = 0_usize;
    let mut fulls = 0_usize;
    for seed in 1..=400_u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_f491_4f6c_dd1d) | 1);
        let mut daemon = fresh(&mut rng, seed << 20);
        let mut client = decode_full(&full_frame(&daemon));
        assert_eq!(client, daemon, "seed {seed} initial");
        let mut shared_client = client.clone();
        let mut shared = TerminalDiffScratch::default();
        let mut tail = PatchTail::default();
        let mut shared_frame = Vec::new();
        for step in 0..80 {
            let mut current = daemon.clone();
            current.view_generation = current
                .view_generation
                .wrapping_add(1 + rng.below(3) as u64);
            if rng.chance(70) {
                current.generation = current.generation.wrapping_add(1);
            }
            let reset = rng.below(100);
            if reset < 3 {
                current = fresh(&mut rng, current.generation.wrapping_add(1));
            } else if reset < 6 {
                current.dictionary_generation = current.dictionary_generation.wrapping_add(1);
                current.dictionary = Arc::new(dictionary_with(1 + rng.below(6), &["a\u{308}"]));
                let styles = current.styles().len();
                for cell in Arc::make_mut(&mut current.cells) {
                    if usize::from(cell.style_id()) >= styles
                        || cell.glyph() & GRAPHEME_TABLE_BIT != 0
                    {
                        *cell = PackedCell::EMPTY;
                    }
                }
            }
            if rng.chance(25) {
                grow_dictionary(&mut current, &mut rng);
            }
            if rng.chance(25) {
                scroll(&mut current, &mut rng);
            }
            let rows = usize::from(current.rows);
            for row in 0..rows {
                if rng.chance(15) {
                    random_row(&mut current, &mut rng, row);
                }
            }
            if rng.chance(20) && current.columns > 2 {
                let columns = usize::from(current.columns);
                let row = rng.below(rows);
                let column = rng.below(columns - 1);
                let cells = Arc::make_mut(&mut current.cells);
                cells[row * columns + column] =
                    PackedCell::new(u32::from('Z'), 0, CellWidth::Narrow);
            }
            mutate_metadata(&mut current, &mut rng);
            if let Some(patch) = TerminalViewport::diff(&daemon, &current) {
                patches += 1;
                let frame = patch_frame(&patch);
                let decoded = decode_patch(&frame);
                client
                    .apply_patch(decoded)
                    .unwrap_or_else(|error| panic!("seed {seed} step {step}: {error:?}"));
                assert_eq!(client, current, "seed {seed} step {step} patch");
                let borrowed = TerminalViewport::diff_shared(&daemon, &current, &mut shared)
                    .expect("a borrowed diff agrees with the owned one");
                crate::encode_terminal_patch_event_into(
                    PaneId(4),
                    9,
                    &borrowed,
                    &mut tail,
                    &mut shared_frame,
                )
                .expect("a borrowed patch encodes");
                shared.release_shared();
                assert_eq!(shared_frame, frame, "seed {seed} step {step} borrowed");
                shared_client
                    .apply_patch(decode_patch(&shared_frame))
                    .unwrap_or_else(|error| panic!("seed {seed} step {step} shared: {error:?}"));
                assert_eq!(shared_client, current, "seed {seed} step {step} shared");
            } else {
                fulls += 1;
                client = decode_full(&full_frame(&current));
                assert_eq!(client, current, "seed {seed} step {step} full");
                shared_client = client.clone();
                assert!(TerminalViewport::diff_shared(&daemon, &current, &mut shared).is_none());
            }
            daemon = current;
        }
    }
    assert!(patches > 10_000, "{patches} patches, {fulls} fulls");
}

#[test]
fn decoders_and_apply_never_panic_on_mutated_frames() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut accepted = 0_usize;
    for seed in 1..=120_u64 {
        let mut local = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let base = fresh(&mut local, 77);
        let mut current = base.clone();
        current.view_generation += 1;
        current.generation += 1;
        grow_dictionary(&mut current, &mut local);
        scroll(&mut current, &mut local);
        for row in 0..usize::from(current.rows) {
            if local.chance(30) {
                random_row(&mut current, &mut local, row);
            }
        }
        mutate_metadata(&mut current, &mut local);
        let full_payload = payload(&full_frame(&base));
        let patch_payload =
            TerminalViewport::diff(&base, &current).map(|patch| payload(&patch_frame(&patch)));
        let history_rows = (0..usize::from(base.rows))
            .map(|row| {
                base.row(u16::try_from(row).expect("fits"))
                    .expect("row")
                    .to_vec()
            })
            .collect::<Vec<_>>();
        let mut history = Vec::new();
        encode_history_frame(
            &mut history,
            PaneId(3),
            5,
            &HistoryChunkRef {
                start: 1,
                total: 99,
                offset: 40,
                columns: base.columns,
                rows: &history_rows,
                dictionary: &base.dictionary,
            },
        )
        .expect("history encodes");
        let history_payload = payload(&history);
        for original in [Some(full_payload), patch_payload, Some(history_payload)]
            .into_iter()
            .flatten()
        {
            for _ in 0..400 {
                let mut mutated = original.clone();
                match rng.below(5) {
                    0 => {
                        let index = rng.below(mutated.len());
                        mutated[index] ^= 1 << rng.below(8);
                    }
                    1 => {
                        let index = rng.below(mutated.len());
                        mutated[index] = u8::try_from(rng.below(256)).expect("byte");
                    }
                    2 => {
                        let index = rng.below(mutated.len() + 1);
                        mutated.insert(index, u8::try_from(rng.below(256)).expect("byte"));
                    }
                    3 if mutated.len() > 1 => {
                        let index = 1 + rng.below(mutated.len() - 1);
                        mutated.remove(index);
                    }
                    _ => {
                        for _ in 0..=rng.below(6) {
                            let index = rng.below(mutated.len());
                            mutated[index] = 0xff;
                        }
                    }
                }
                match mutated[0] {
                    FULL_VIEWPORT | COMMAND_OUTPUT_VIEWPORT => {
                        if let Ok((_, _, _, viewport)) = decode_viewport_frame(&mutated) {
                            accepted += 1;
                            let mut out = Vec::new();
                            let _ = encode_viewport_body(&mut out, &viewport);
                        }
                    }
                    VIEWPORT_PATCH => {
                        if let Ok((_, _, patch)) = decode_patch_frame(&mutated) {
                            accepted += 1;
                            let mut retained = base.clone();
                            let _ = retained.apply_patch(patch.clone());
                            let mut other = current.clone();
                            other.generation = patch.base_generation;
                            other.view_generation = patch.base_view_generation;
                            other.dictionary_generation = patch.dictionary_generation;
                            let _ = other.apply_patch(patch);
                        }
                    }
                    HISTORY_CHUNK if decode_history_frame(&mutated).is_ok() => {
                        accepted += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(accepted > 100, "{accepted}");
}

#[test]
fn random_bytes_never_panic_any_decoder() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    for _ in 0..200_000 {
        let len = rng.below(64);
        let mut bytes = (0..len)
            .map(|_| u8::try_from(rng.below(256)).expect("byte"))
            .collect::<Vec<_>>();
        if !bytes.is_empty() {
            bytes[0] = u8::try_from(rng.below(4)).expect("kind");
        }
        let _ = decode_viewport_frame(&bytes);
        let _ = decode_patch_frame(&bytes);
        let _ = decode_history_frame(&bytes);
        let mut reader = Reader::new(&bytes);
        let _ = decode_viewport_body(&mut reader);
    }
}

#[test]
fn edge_grids_round_trip() {
    for (columns, rows) in [
        (1, 1),
        (2, 1),
        (1, 2),
        (0, 0),
        (0, 3),
        (3, 0),
        (u16::MAX, 1),
        (1, 300),
    ] {
        let mut viewport = TerminalViewport::blank(columns, rows, SessionStatus::Running);
        viewport.dictionary = Arc::new(dictionary_with(3, &["e\u{301}"]));
        if columns > 0 && rows > 0 {
            let width = usize::from(columns);
            let cells = Arc::make_mut(&mut viewport.cells);
            cells[width - 1] = PackedCell::new(0, 2, CellWidth::Narrow);
            if width > 1 {
                cells[width - 2] = PackedCell::new(u32::from('\u{4e2d}'), 1, CellWidth::Wide);
                cells[width - 1] = PackedCell::new(0, 1, CellWidth::SpacerTail);
            }
            let last = cells.len() - 1;
            cells[last] = PackedCell::new(GRAPHEME_TABLE_BIT, 2, CellWidth::Narrow);
        }
        let frame = full_frame(&viewport);
        assert_eq!(decode_full(&frame), viewport, "{columns}x{rows}");
        let mut next = viewport.clone();
        next.view_generation += 1;
        if columns > 0 && rows > 0 {
            Arc::make_mut(&mut next.cells)[0] =
                PackedCell::new(u32::from('q'), 1, CellWidth::Narrow);
        }
        if let Some(patch) = TerminalViewport::diff(&viewport, &next) {
            let mut client = viewport.clone();
            client
                .apply_patch(decode_patch(&patch_frame(&patch)))
                .expect("applies");
            assert_eq!(client, next, "{columns}x{rows}");
        }
    }
}
