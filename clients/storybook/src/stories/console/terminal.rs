use std::{cell::RefCell, rc::Rc, sync::Arc};

use zpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Pixels,
    Render, RenderImage, Styled as _, TextRun, Window, canvas, div, px,
};
use zz_terminal::{
    ATTR_BOLD, ATTR_FAINT, ATTR_HYPERLINK, ATTR_INVISIBLE, ATTR_ITALIC, ATTR_OVERLINE,
    ATTR_STRIKETHROUGH, CellWidth, Color, Cursor, CursorStyle, GRAPHEME_TABLE_BIT, KittyLayer,
    KittyPlacement, OVERLAY_RECTANGLE, OverlayKind, OverlaySpan, PackedCell, PackedStyle,
    ScrollbarState, SearchStatus, SessionStatus, TerminalAppearance, TerminalDictionary,
    TerminalMode, TerminalPresentation, TerminalViewport, UnderlineStyle,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _,
    terminal::{
        RowRenderCache, TerminalImageSource, TerminalRenderInput, terminal_background,
        terminal_font_for_style,
    },
};

use crate::story::{Section, Story, states};

pub const STORY: Story = Story {
    id: "terminal-grid",
    name: "Terminal grid",
    group: "Terminal",
    summary: "The terminal renderer painting hand-built viewports: the same cells, styles and overlays the daemon ships, with no VT parser in the page.",
    sections: &[
        Section {
            id: "shell",
            name: "Shell session",
            summary: "A prompt, ls --color and a failing cargo test, with the block cursor waiting at the last prompt.",
            build: |_, cx| gallery(cx, 1, vec![("focused", shell())]),
        },
        Section {
            id: "colours",
            name: "Colours",
            summary: "The 16 palette colours as foreground and background, then a run of the 256-colour cube and a truecolor ramp.",
            build: |_, cx| gallery(cx, 1, vec![("default appearance", colours())]),
        },
        Section {
            id: "attributes",
            name: "Text attributes",
            summary: "Bold, italic, faint, strikethrough, overline and invisible cells, alone and combined.",
            build: |_, cx| gallery(cx, 1, vec![("attributes", attributes())]),
        },
        Section {
            id: "underlines",
            name: "Underlines",
            summary: "Every UnderlineStyle, in the text colour and in an underline colour of its own.",
            build: |_, cx| gallery(cx, 1, vec![("underline styles", underlines())]),
        },
        Section {
            id: "cursor",
            name: "Cursor",
            summary: "Each cursor style focused and unfocused. An unfocused pane draws a hollow block whatever the style.",
            build: |_, cx| gallery(cx, 2, cursors()),
        },
        Section {
            id: "selection",
            name: "Selection",
            summary: "A line selection with rounded outer corners, the same with rounding off, and a rectangle selection.",
            build: |_, cx| gallery(cx, 1, selections()),
        },
        Section {
            id: "copy-mode",
            name: "Copy mode",
            summary: "Scrolled back in copy mode: the copy cursor, its line tint and a selection growing from it.",
            build: |_, cx| gallery(cx, 1, vec![("copy mode", copy_mode())]),
        },
        Section {
            id: "search",
            name: "Search",
            summary: "Search matches with the current one brighter.",
            build: |_, cx| gallery(cx, 1, vec![("gpu-box, 2 of 3", search())]),
        },
        Section {
            id: "links",
            name: "Links",
            summary: "OSC 8 hyperlinks are underlined in the text colour. The one under the pointer also gets the link tint and a link-coloured rule.",
            build: |_, cx| gallery(cx, 1, vec![("hovering the second link", links())]),
        },
        Section {
            id: "ime",
            name: "Input method",
            summary: "Text an input method is still composing, drawn at the cursor before the shell sees it.",
            build: |_, cx| gallery(cx, 2, ime()),
        },
        Section {
            id: "scrollbar",
            name: "Scrollbar",
            summary: "The thumb shows only when history is longer than the screen.",
            build: |_, cx| gallery(cx, 3, scrollbars()),
        },
        Section {
            id: "wide",
            name: "Wide cells and graphemes",
            summary: "Two-cell characters with their spacer tails, and clusters stored in the grapheme table. The page only loads Inter and Lilex, so glyphs outside them fall back to whatever the browser text system finds.",
            build: |_, cx| gallery(cx, 1, vec![("wide and clustered", wide())]),
        },
        Section {
            id: "box-drawing",
            name: "Box drawing and blocks",
            summary: "Light lines, rounded corners and solid blocks are painted as quads that meet across cells. Heavy and double lines come from the font.",
            build: |_, cx| gallery(cx, 1, vec![("a TUI frame", box_drawing())]),
        },
        Section {
            id: "image",
            name: "Inline image",
            summary: "A kitty graphics placement above the text, anchored to cells.",
            build: |_, cx| image_section(cx),
        },
    ],
};

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="144" viewBox="0 0 240 144"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#7aa2f7"/><stop offset="1" stop-color="#bb9af7"/></linearGradient></defs><rect width="240" height="144" rx="10" fill="url(#g)"/><circle cx="62" cy="72" r="34" fill="#1a1b26" opacity="0.85"/><rect x="118" y="38" width="90" height="16" rx="8" fill="#1a1b26" opacity="0.7"/><rect x="118" y="64" width="64" height="16" rx="8" fill="#1a1b26" opacity="0.5"/><rect x="118" y="90" width="78" height="16" rx="8" fill="#1a1b26" opacity="0.35"/><path d="M48 60h28l-28 24h28" stroke="#c0caf5" stroke-width="6" fill="none" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

fn appearance() -> TerminalAppearance {
    TerminalAppearance {
        font_families: vec!["Lilex".into()],
        ..TerminalAppearance::default()
    }
}

struct Screen {
    columns: u16,
    rows: u16,
    cells: Vec<PackedCell>,
    styles: Vec<PackedStyle>,
    offsets: Vec<u32>,
    bytes: Vec<u8>,
    appearance: TerminalAppearance,
}

#[derive(Clone, Copy, Default)]
struct Ink {
    fg: Option<Color>,
    bg: Option<Color>,
    attrs: u16,
    underline: UnderlineStyle,
    underline_color: Option<Color>,
}

const fn is_wide(character: char) -> bool {
    matches!(
        character as u32,
        0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F680..=0x1F6FF
            | 0x1F900..=0x1F9FF
    )
}

impl Screen {
    fn new(columns: u16, rows: u16) -> Self {
        let appearance = appearance();
        let base = PackedStyle::new(
            appearance.foreground,
            appearance.background,
            None,
            0,
            UnderlineStyle::None,
        );
        Self {
            columns,
            rows,
            cells: vec![PackedCell::EMPTY; usize::from(columns) * usize::from(rows)],
            styles: vec![base],
            offsets: vec![0],
            bytes: Vec::new(),
            appearance,
        }
    }

    fn palette(&self, index: usize) -> Color {
        self.appearance.palette[index]
    }

    fn ink(&mut self, ink: Ink) -> u16 {
        let style = PackedStyle::new(
            ink.fg.unwrap_or(self.appearance.foreground),
            ink.bg.unwrap_or(self.appearance.background),
            ink.underline_color,
            ink.attrs,
            ink.underline,
        );
        if let Some(index) = self.styles.iter().position(|known| *known == style) {
            return index as u16;
        }
        self.styles.push(style);
        (self.styles.len() - 1) as u16
    }

    fn fg(&mut self, index: usize) -> u16 {
        let fg = self.palette(index);
        self.ink(Ink {
            fg: Some(fg),
            ..Ink::default()
        })
    }

    fn bold(&mut self, index: usize) -> u16 {
        let fg = self.palette(index);
        self.ink(Ink {
            fg: Some(fg),
            attrs: ATTR_BOLD,
            ..Ink::default()
        })
    }

    fn set(&mut self, row: u16, column: u16, cell: PackedCell) {
        if row < self.rows && column < self.columns {
            self.cells[usize::from(row) * usize::from(self.columns) + usize::from(column)] = cell;
        }
    }

    fn write(&mut self, row: u16, column: u16, text: &str, style: u16) -> u16 {
        let mut column = column;
        for character in text.chars() {
            if is_wide(character) {
                self.set(
                    row,
                    column,
                    PackedCell::new(u32::from(character), style, CellWidth::Wide),
                );
                self.set(
                    row,
                    column + 1,
                    PackedCell::new(0, style, CellWidth::SpacerTail),
                );
                column += 2;
            } else {
                self.set(
                    row,
                    column,
                    PackedCell::new(u32::from(character), style, CellWidth::Narrow),
                );
                column += 1;
            }
        }
        column
    }

    fn cluster(&mut self, row: u16, column: u16, cluster: &str, wide: bool, style: u16) -> u16 {
        let index = (self.offsets.len() - 1) as u32;
        self.bytes.extend_from_slice(cluster.as_bytes());
        self.offsets.push(self.bytes.len() as u32);
        let glyph = GRAPHEME_TABLE_BIT | index;
        if wide {
            self.set(row, column, PackedCell::new(glyph, style, CellWidth::Wide));
            self.set(
                row,
                column + 1,
                PackedCell::new(0, style, CellWidth::SpacerTail),
            );
            column + 2
        } else {
            self.set(
                row,
                column,
                PackedCell::new(glyph, style, CellWidth::Narrow),
            );
            column + 1
        }
    }

    fn fill(&mut self, row: u16, from: u16, to: u16, style: u16) {
        for column in from..to {
            self.set(
                row,
                column,
                PackedCell::new(u32::from(' '), style, CellWidth::Narrow),
            );
        }
    }

    fn prompt(&mut self, row: u16, command: &str) -> u16 {
        let path = self.bold(4);
        let branch = self.fg(5);
        let arrow = self.bold(2);
        let mut column = self.write(row, 0, "~/dev/zz ", path);
        column = self.write(row, column, "main* ", branch);
        column = self.write(row, column, "> ", arrow);
        self.write(row, column, command, 0)
    }

    fn viewport(self) -> TerminalViewport {
        let mut viewport = TerminalViewport::blank_with_appearance(
            self.columns,
            self.rows,
            SessionStatus::Running,
            &self.appearance,
        );
        viewport.cells = self.cells.into();
        viewport.dictionary = Arc::new(TerminalDictionary::from_shared(
            self.styles.into(),
            self.offsets.into(),
            self.bytes.into(),
        ));
        viewport
    }
}

fn cursor(column: u16, row: u16, style: CursorStyle) -> Cursor {
    Cursor::new(
        column,
        row,
        true,
        true,
        false,
        style,
        appearance().cursor_color,
    )
}

struct StoryImage(Option<Arc<RenderImage>>);

impl TerminalImageSource for StoryImage {
    fn image(&self, image_id: u32, _: u64) -> Option<Arc<RenderImage>> {
        (image_id == 1).then(|| self.0.clone()).flatten()
    }
}

struct Sample {
    viewport: Arc<TerminalViewport>,
    appearance: Arc<TerminalAppearance>,
    focused: bool,
    blink_visible: bool,
    marked: Option<&'static str>,
    images: Option<Rc<StoryImage>>,
}

impl Sample {
    fn new(viewport: TerminalViewport) -> Self {
        Self {
            viewport: Arc::new(viewport),
            appearance: Arc::new(appearance()),
            focused: true,
            blink_visible: true,
            marked: None,
            images: None,
        }
    }

    fn unfocused(mut self) -> Self {
        self.focused = false;
        self
    }

    fn appearance(mut self, appearance: TerminalAppearance) -> Self {
        self.appearance = Arc::new(appearance);
        self
    }
}

struct SampleView {
    sample: Sample,
    revisions: Arc<[u64]>,
    cache: Rc<RefCell<RowRenderCache>>,
}

impl SampleView {
    fn new(sample: Sample) -> Self {
        let revisions = (1..=u64::from(sample.viewport.rows)).collect();
        Self {
            sample,
            revisions,
            cache: Rc::default(),
        }
    }
}

fn cell_metrics(
    appearance: &TerminalAppearance,
    window: &mut Window,
    cx: &App,
) -> (Pixels, Pixels) {
    let font = terminal_font_for_style(appearance, &cx.theme().mono_font_family, false, false);
    let font_size = px(appearance.font_size_points);
    let probe = window.text_system().shape_line(
        "m".into(),
        font_size,
        &[TextRun {
            len: 1,
            font: font.clone(),
            color: cx.theme().foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    let font_id = probe.runs.first().map_or_else(
        || window.text_system().resolve_font(&font),
        |run| run.font_id,
    );
    let scale = window.scale_factor();
    let natural = probe.ascent + probe.descent + window.text_system().line_gap(font_id, font_size);
    let height = (appearance
        .cell_height_adjustment
        .apply(f32::from(natural))
        .max(1.0)
        * scale)
        .round()
        / scale;
    let width = (f32::from(probe.width).max(1.0) * scale).round() / scale;
    (px(width), px(height))
}

impl Render for SampleView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = Arc::clone(&self.sample.viewport);
        let appearance = Arc::clone(&self.sample.appearance);
        let revisions = Arc::clone(&self.revisions);
        let prepaint_cache = Rc::clone(&self.cache);
        let paint_cache = Rc::clone(&self.cache);
        let focused = self.sample.focused;
        let blink_visible = self.sample.blink_visible;
        let marked = self.sample.marked;
        let images = self.sample.images.clone();
        let (cell_width, cell_height) = cell_metrics(&appearance, window, cx);
        let font = terminal_font_for_style(&appearance, &cx.theme().mono_font_family, false, false);
        let font_size = px(appearance.font_size_points);
        let width = cell_width * f32::from(viewport.columns)
            + px(appearance.padding_left + appearance.padding_right + 1.0);
        let height = cell_height * f32::from(viewport.rows)
            + px(appearance.padding_top + appearance.padding_bottom + 1.0);
        let background = cx
            .theme()
            .background
            .opaque()
            .blend(terminal_background(
                appearance.background,
                appearance.background_opacity,
            ))
            .opacity(cx.theme().pane_background_opacity);
        div()
            .w(width)
            .h(height)
            .flex_none()
            .rounded(cx.theme().radius)
            .overflow_hidden()
            .bg(background)
            .child(
                div()
                    .size_full()
                    .pl(px(appearance.padding_left))
                    .pr(px(appearance.padding_right))
                    .pt(px(appearance.padding_top))
                    .pb(px(appearance.padding_bottom))
                    .font(font)
                    .text_size(font_size)
                    .line_height(px(appearance
                        .cell_height_adjustment
                        .apply(f32::from(font_size))
                        .max(1.0)))
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                prepaint_cache.borrow_mut().prepaint(
                                    TerminalRenderInput {
                                        viewport: &viewport,
                                        row_revisions: &revisions,
                                        revision_epoch: 0,
                                        history: None,
                                        images: images
                                            .as_deref()
                                            .map(|images| images as &dyn TerminalImageSource),
                                        local_scroll_target: None,
                                        scroll_pixel_offset: px(0.0),
                                        overscroll: px(0.0),
                                        extra_height: px(0.0),
                                        command_output: false,
                                        appearance: &appearance,
                                        appearance_hash: appearance.stable_hash(),
                                        text_opacity: 1.0,
                                        focused,
                                        cursor_blink_visible: blink_visible,
                                        marked_text: marked,
                                        rows_above: None,
                                    },
                                    bounds,
                                    window,
                                    cx,
                                )
                            },
                            move |bounds, mut paint, window, cx| {
                                paint_cache
                                    .borrow_mut()
                                    .paint(&mut paint, bounds, window, cx);
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}

struct Gallery {
    columns: u16,
    items: Vec<(&'static str, Entity<SampleView>)>,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.items.iter().fold(
            states().columns(self.columns),
            |states, (caption, sample)| states.state(*caption, sample.clone()),
        )
    }
}

fn gallery(cx: &mut App, columns: u16, items: Vec<(&'static str, Sample)>) -> AnyView {
    let items = items
        .into_iter()
        .map(|(caption, sample)| (caption, cx.new(|_| SampleView::new(sample))))
        .collect();
    cx.new(|_| Gallery { columns, items }).into()
}

fn shell() -> Sample {
    let mut screen = Screen::new(92, 22);
    let dir = screen.bold(4);
    let exec = screen.bold(2);
    let symlink = screen.fg(6);
    let green = screen.bold(2);
    let ok = screen.fg(2);
    let failed = screen.bold(1);
    let ignored = screen.fg(3);
    let dim = screen.ink(Ink {
        attrs: ATTR_FAINT,
        ..Ink::default()
    });
    let path = screen.fg(4);
    screen.prompt(0, "ls --color");
    let mut column = 0;
    for (name, style) in [
        ("Cargo.lock", 0),
        ("Cargo.toml", 0),
        ("Justfile", 0),
        ("clients/", dir),
        ("crates/", dir),
        ("scripts/", dir),
        ("site", symlink),
        ("target/", dir),
        ("wt.sh", exec),
    ] {
        screen.write(1, column, name, style);
        column += name.chars().count() as u16 + 2;
    }
    screen.prompt(2, "cargo test -p zz-ui palette");
    let line = |screen: &mut Screen, row: u16, head: &str, head_style: u16, rest: &str| {
        let column = screen.write(row, 0, head, head_style);
        screen.write(row, column, rest, 0);
    };
    line(
        &mut screen,
        3,
        "   Compiling",
        green,
        " zz-ui v0.15.0 (/Users/fabrico/dev/zz/crates/zz-ui)",
    );
    line(
        &mut screen,
        4,
        "    Finished",
        green,
        " `test` profile [unoptimized + debuginfo] target(s) in 41.87s",
    );
    line(
        &mut screen,
        5,
        "     Running",
        green,
        " unittests src/lib.rs (target/debug/deps/zz_ui-3f2c9a1b8e7d6c5a)",
    );
    screen.write(7, 0, "running 6 tests", 0);
    let tests = [
        (
            "command::palette::tests::disclosure_toggles_without_activating",
            "ok",
            ok,
        ),
        (
            "command::palette_view::tests::window_chooser_keeps_selection",
            "ok",
            ok,
        ),
        (
            "command::palette_model::tests::fuzzy_match_ranks_prefixes",
            "ok",
            ok,
        ),
        (
            "command::palette_view::tests::prompt_completes_options",
            "FAILED",
            failed,
        ),
        (
            "command::palette::tests::palette_fits_its_container",
            "ok",
            ok,
        ),
        (
            "command::palette_model::tests::host_mode_lists_offline",
            "ignored",
            ignored,
        ),
    ];
    for (index, (name, verdict, style)) in tests.iter().enumerate() {
        let row = 8 + index as u16;
        let column = screen.write(row, 0, "test ", 0);
        let column = screen.write(row, column, name, 0);
        let column = screen.write(row, column, " ... ", dim);
        screen.write(row, column, verdict, *style);
    }
    screen.write(15, 0, "failures:", 0);
    screen.write(
        17,
        0,
        "---- command::palette_view::tests::prompt_completes_options stdout ----",
        0,
    );
    let column = screen.write(18, 0, "thread 'main' panicked at ", 0);
    let column = screen.write(
        18,
        column,
        "crates/zz-ui/src/command/palette_view_tests.rs:212:9",
        path,
    );
    screen.write(18, column, ":", 0);
    let column = screen.write(20, 0, "test result: ", 0);
    let column = screen.write(20, column, "FAILED", failed);
    screen.write(
        20,
        column,
        ". 4 passed; 1 failed; 1 ignored; 0 measured; 132 filtered out",
        0,
    );
    let end = screen.prompt(21, "");
    let mut viewport = screen.viewport();
    viewport.cursor = Some(cursor(end, 21, CursorStyle::Block));
    viewport.presentation = Arc::new(TerminalPresentation::new(
        Arc::from("cargo test"),
        Some(Arc::from("/Users/fabrico/dev/zz")),
        None,
    ));
    Sample::new(viewport)
}

fn colours() -> Sample {
    let mut screen = Screen::new(88, 9);
    for index in 0..16_usize {
        let fg = screen.fg(index);
        let column = (index % 8) as u16 * 11;
        let row = (index / 8) as u16;
        screen.write(row, column, &format!("colour {index:>2}"), fg);
        let bg = screen.palette(index);
        let contrast = if index == 0 || index == 8 { 15 } else { 0 };
        let fg = screen.palette(contrast);
        let block = screen.ink(Ink {
            fg: Some(fg),
            bg: Some(bg),
            ..Ink::default()
        });
        screen.fill(row + 3, column, column + 10, block);
        screen.write(row + 3, column + 1, &format!("bg {index:>2}"), block);
    }
    for index in 0..88_u16 {
        let colour = screen.palette(usize::from(16 + index * 2 + index / 22));
        let block = screen.ink(Ink {
            bg: Some(colour),
            ..Ink::default()
        });
        screen.fill(6, index, index + 1, block);
    }
    for index in 0..88_u16 {
        let level = (index * 255 / 87) as u8;
        let block = screen.ink(Ink {
            bg: Some(Color::rgb(level, 64, 255 - level)),
            ..Ink::default()
        });
        screen.fill(8, index, index + 1, block);
    }
    Sample::new(screen.viewport())
}

fn attributes() -> Sample {
    let mut screen = Screen::new(92, 4);
    let entries = [
        ("regular", 0),
        ("bold", ATTR_BOLD),
        ("italic", ATTR_ITALIC),
        ("bold italic", ATTR_BOLD | ATTR_ITALIC),
        ("faint", ATTR_FAINT),
        ("strikethrough", ATTR_STRIKETHROUGH),
        ("overline", ATTR_OVERLINE),
        ("invisible", ATTR_INVISIBLE),
    ];
    let mut column = 0;
    for (label, attrs) in entries {
        let style = screen.ink(Ink {
            attrs,
            ..Ink::default()
        });
        screen.write(0, column, label, style);
        let red = screen.palette(1);
        let coloured = screen.ink(Ink {
            fg: Some(red),
            attrs,
            ..Ink::default()
        });
        screen.write(2, column, label, coloured);
        column += label.len() as u16 + 3;
    }
    let base = screen.ink(Ink::default());
    screen.write(
        3,
        0,
        "faint and invisible keep their cell, so the columns above still line up",
        base,
    );
    Sample::new(screen.viewport())
}

fn underlines() -> Sample {
    let mut screen = Screen::new(92, 3);
    let kinds = [
        ("single", UnderlineStyle::Single),
        ("double", UnderlineStyle::Double),
        ("curly", UnderlineStyle::Curly),
        ("dotted", UnderlineStyle::Dotted),
        ("dashed", UnderlineStyle::Dashed),
    ];
    let mut column = 0;
    for (label, underline) in kinds {
        let plain = screen.ink(Ink {
            underline,
            ..Ink::default()
        });
        screen.write(0, column, label, plain);
        let red = screen.palette(9);
        let coloured = screen.ink(Ink {
            underline,
            underline_color: Some(red),
            ..Ink::default()
        });
        screen.write(2, column, label, coloured);
        column += label.len() as u16 + 4;
    }
    let caption = screen.fg(8);
    screen.write(0, 60, "text colour", caption);
    screen.write(2, 60, "underline colour 9", caption);
    Sample::new(screen.viewport())
}

fn cursor_sample(style: CursorStyle, focused: bool) -> Sample {
    let mut screen = Screen::new(40, 2);
    screen.prompt(0, "cargo build");
    let end = screen.prompt(1, "git st");
    let mut viewport = screen.viewport();
    viewport.cursor = Some(cursor(end - 2, 1, style));
    let sample = Sample::new(viewport);
    if focused { sample } else { sample.unfocused() }
}

fn cursors() -> Vec<(&'static str, Sample)> {
    let mut blink_off = cursor_sample(CursorStyle::Block, true);
    blink_off.blink_visible = false;
    let mut wide = Screen::new(40, 2);
    let end = wide.prompt(0, "echo ");
    let column = wide.write(0, end, "日本語", 0);
    wide.prompt(1, "");
    let mut wide_viewport = wide.viewport();
    wide_viewport.cursor = Some(Cursor::new(
        column - 4,
        0,
        true,
        true,
        false,
        CursorStyle::Block,
        appearance().cursor_color,
    ));
    vec![
        ("block, focused", cursor_sample(CursorStyle::Block, true)),
        ("block, unfocused", cursor_sample(CursorStyle::Block, false)),
        ("bar, focused", cursor_sample(CursorStyle::Bar, true)),
        ("bar, unfocused", cursor_sample(CursorStyle::Bar, false)),
        (
            "underline, focused",
            cursor_sample(CursorStyle::Underline, true),
        ),
        (
            "underline, unfocused",
            cursor_sample(CursorStyle::Underline, false),
        ),
        (
            "hollow block style",
            cursor_sample(CursorStyle::BlockHollow, true),
        ),
        ("blink phase off", blink_off),
        ("block on a wide character", Sample::new(wide_viewport)),
    ]
}

fn log_screen(columns: u16, rows: u16) -> Screen {
    let mut screen = Screen::new(columns, rows);
    let time = screen.fg(8);
    let info = screen.fg(2);
    let warn = screen.fg(3);
    let error = screen.bold(1);
    let lines = [
        (
            "14:31:58",
            "INFO",
            info,
            "daemon listening on /tmp/zz-501/default",
        ),
        (
            "14:31:58",
            "INFO",
            info,
            "client 3 attached to session zz (100x30)",
        ),
        (
            "14:32:01",
            "WARN",
            warn,
            "pane %11 wrote 4.2 MB in 80 ms, coalescing frames",
        ),
        (
            "14:32:03",
            "INFO",
            info,
            "agent pane %14 started claude (acp 0.4)",
        ),
        (
            "14:32:07",
            "ERROR",
            error,
            "ssh gpu-box: connection reset by peer",
        ),
        ("14:32:07", "INFO", info, "reconnecting to gpu-box in 2s"),
        ("14:32:09", "INFO", info, "gpu-box: 1 session restored"),
        (
            "14:32:12",
            "WARN",
            warn,
            "error rate limit reached for copy-mode search",
        ),
    ];
    for (row, (stamp, level, style, message)) in lines.iter().enumerate() {
        let row = row as u16;
        if row >= rows {
            break;
        }
        let column = screen.write(row, 0, stamp, time);
        let column = screen.write(row, column + 1, level, *style);
        screen.write(row, column + 1, message, 0);
    }
    screen
}

fn selections() -> Vec<(&'static str, Sample)> {
    let line = |rounded: bool| {
        let mut viewport = log_screen(90, 6).viewport();
        viewport.overlays = Arc::from(vec![
            OverlaySpan::new(1, 24, 90, OverlayKind::Selection),
            OverlaySpan::new(2, 0, 90, OverlayKind::Selection),
            OverlaySpan::new(3, 0, 31, OverlayKind::Selection),
        ]);
        let mut appearance = appearance();
        appearance.rounded_selection = rounded;
        Sample::new(viewport).appearance(appearance).unfocused()
    };
    let mut rectangle = log_screen(90, 6).viewport();
    rectangle.overlays = Arc::from(
        (1..5)
            .map(|row| {
                OverlaySpan::with_flags(row, 9, 15, OverlayKind::Selection, OVERLAY_RECTANGLE)
            })
            .collect::<Vec<_>>(),
    );
    vec![
        ("three lines, rounded", line(true)),
        ("three lines, rounding off", line(false)),
        ("rectangle", Sample::new(rectangle).unfocused()),
    ]
}

fn copy_mode() -> Sample {
    let mut viewport = log_screen(90, 8).viewport();
    viewport.mode = TerminalMode::Copy {
        position: 212,
        total: 640,
        hide_position: false,
    };
    viewport.scrollbar = ScrollbarState {
        total: 640,
        offset: 205,
        len: 8,
    };
    viewport.overlays = Arc::from(vec![
        OverlaySpan::new(4, 9, 61, OverlayKind::Selection),
        OverlaySpan::new(4, 60, 61, OverlayKind::CopyCursor),
    ]);
    Sample::new(viewport)
}

fn search() -> Sample {
    let mut viewport = log_screen(90, 8).viewport();
    viewport.search = Some(SearchStatus::new(2, 3));
    viewport.overlays = Arc::from(vec![
        OverlaySpan::new(4, 19, 26, OverlayKind::SearchMatch),
        OverlaySpan::new(5, 30, 37, OverlayKind::SearchCurrent),
        OverlaySpan::new(6, 14, 21, OverlayKind::SearchMatch),
    ]);
    Sample::new(viewport)
}

fn links() -> Sample {
    let mut screen = Screen::new(90, 4);
    let link = screen.ink(Ink {
        attrs: ATTR_HYPERLINK,
        ..Ink::default()
    });
    let blue = screen.palette(4);
    let styled_link = screen.ink(Ink {
        fg: Some(blue),
        attrs: ATTR_HYPERLINK,
        underline: UnderlineStyle::Single,
        ..Ink::default()
    });
    screen.prompt(0, "gh pr view 214");
    let column = screen.write(1, 0, "Docs: ", 0);
    screen.write(1, column, "https://zzmux.sh/docs/terminal", link);
    let column = screen.write(2, 0, "PR:   ", 0);
    screen.write(2, column, "https://github.com/demfabris/zz/pull/214", link);
    let column = screen.write(3, 0, "Styled by the program: ", 0);
    screen.write(3, column, "storybook", styled_link);
    let mut viewport = screen.viewport();
    viewport.overlays = Arc::from(vec![OverlaySpan::new(2, 6, 46, OverlayKind::LinkHover)]);
    viewport.presentation = Arc::new(TerminalPresentation::new(
        Arc::from("gh"),
        None,
        Some(Arc::from("https://github.com/demfabris/zz/pull/214")),
    ));
    Sample::new(viewport)
}

fn ime() -> Vec<(&'static str, Sample)> {
    let composing = |marked: &'static str| {
        let mut screen = Screen::new(40, 2);
        screen.prompt(0, "git commit -m 'wip'");
        let end = screen.prompt(1, "echo ");
        let mut viewport = screen.viewport();
        viewport.cursor = Some(cursor(end, 1, CursorStyle::Bar));
        let mut sample = Sample::new(viewport);
        sample.marked = Some(marked);
        sample
    };
    vec![
        ("dead key", composing("´")),
        ("kana", composing("にほんご")),
    ]
}

fn scrollbars() -> Vec<(&'static str, Sample)> {
    let with = |total: u32, offset: u32| {
        let mut viewport = log_screen(26, 8).viewport();
        viewport.scrollbar = ScrollbarState {
            total,
            offset,
            len: 8,
        };
        Sample::new(viewport).unfocused()
    };
    vec![
        ("no history", with(8, 0)),
        ("at the bottom", with(400, 392)),
        ("scrolled back", with(400, 120)),
    ]
}

fn wide() -> Sample {
    let mut screen = Screen::new(90, 5);
    let caption = screen.fg(8);
    screen.write(0, 0, "CJK      ", caption);
    screen.write(0, 9, "日本語のテキスト | 한국어 | 中文", 0);
    screen.write(1, 0, "emoji    ", caption);
    screen.write(1, 9, "🦀 rust  🚀 ship  🔥 hot", 0);
    screen.write(2, 0, "ZWJ      ", caption);
    let mut column = 9;
    for cluster in ["👩‍💻", "👨‍👩‍👧", "🏳️‍🌈"] {
        column = screen.cluster(2, column, cluster, true, 0);
        column += 1;
    }
    screen.write(3, 0, "flags    ", caption);
    let mut column = 9;
    for cluster in ["🇧🇷", "🇯🇵", "🇺🇦"] {
        column = screen.cluster(3, column, cluster, true, 0);
        column += 1;
    }
    screen.write(4, 0, "combining", caption);
    let mut column = 10;
    for cluster in ["e\u{301}", "n\u{303}", "a\u{30a}", "o\u{308}"] {
        column = screen.cluster(4, column, cluster, false, 0);
    }
    Sample::new(screen.viewport())
}

fn box_drawing() -> Sample {
    let mut screen = Screen::new(90, 12);
    let border = screen.fg(8);
    let title = screen.bold(6);
    let bar = screen.fg(2);
    let hot = screen.fg(3);
    let rows = [
        "┌─ htop ──────────────────────┬──────────────────────────┐",
        "│ CPU ████████████▌     62%   │ ╭──────────────────────╮ │",
        "│ MEM ██████▏           31%   │ │ rounded, light lines │ │",
        "│ SWP ▏                  0%   │ ╰──────────────────────╯ │",
        "├─────────────────────────────┼──────────────────────────┤",
        "│ ▁▂▃▄▅▆▇█▇▆▅▄▃▂▁ load        │ ░░▒▒▓▓██ shades          │",
        "│ ▀▀▀▀ ▄▄▄▄ ▌▐ ▖▗▘▝ quadrants │ ╱╲╳ diagonals            │",
        "└─────────────────────────────┴──────────────────────────┘",
        "┏━━━━━━━━━━━┓ ╔═══════════╗ ┍━━━━━━━━━━━┑",
        "┃ heavy     ┃ ║ double    ║ │ mixed     │",
        "┗━━━━━━━━━━━┛ ╚═══════════╝ ┕━━━━━━━━━━━┙",
        "─│┼ joined across cells, no gaps between rows",
    ];
    for (row, text) in rows.iter().enumerate() {
        screen.write(row as u16, 0, text, border);
    }
    screen.write(0, 3, "htop", title);
    screen.write(1, 6, "████████████▌", bar);
    screen.write(2, 6, "██████▏", bar);
    screen.write(5, 2, "▁▂▃▄▅▆▇█▇▆▅▄▃▂▁", hot);
    Sample::new(screen.viewport())
}

fn image_section(cx: &mut App) -> AnyView {
    let image = cx
        .svg_renderer()
        .render_single_frame(SVG.as_bytes(), 2.0)
        .ok();
    let mut screen = Screen::new(90, 12);
    screen.prompt(0, "kitten icat zz.svg");
    let caption = screen.fg(8);
    for row in 2..9 {
        screen.write(row, 4, "covered by the image", 0);
    }
    screen.write(4, 33, "28 × 9 cells, above the text layer", caption);
    screen.write(5, 33, "the covered cells keep their text", caption);
    screen.prompt(11, "");
    let mut viewport = screen.viewport();
    viewport.kitty_placements = Arc::from(vec![KittyPlacement {
        image_id: 1,
        image_generation: 1,
        layer: KittyLayer::AboveText,
        viewport_col: 2,
        viewport_row: 1,
        absolute_row: 1,
        cell_offset_x: 0,
        cell_offset_y: 0,
        grid_cols: 28,
        grid_rows: 9,
        pixel_width: 240,
        pixel_height: 144,
        source_rect: None,
    }]);
    let mut sample = Sample::new(viewport);
    sample.images = Some(Rc::new(StoryImage(image)));
    gallery(cx, 1, vec![("kitty placement above text", sample)])
}
