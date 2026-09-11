use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc, time::Duration};

use gpui::{Context, IntoElement, Render, Task, TextRun, Window, canvas, div, prelude::*, px};
use zz_terminal::{
    ATTR_BOLD, ATTR_ITALIC, AppearanceLoad, CellWidth, Cursor, PackedCell, PackedStyle,
    SessionStatus, TerminalAppearance, TerminalColorScheme, TerminalViewport, UnderlineStyle,
    apply_appearance_overrides,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, StyledExt as _,
    terminal::{RowRenderCache, TerminalRenderInput, cursor_should_blink, terminal_background},
};

use crate::terminal::view::{terminal_font, terminal_font_size, terminal_line_height};

const COLUMNS: u16 = 72;
const ROWS: u16 = 8;

pub(super) struct TerminalPreview {
    appearance: Arc<TerminalAppearance>,
    viewport: Arc<TerminalViewport>,
    cache: Rc<RefCell<RowRenderCache>>,
    #[cfg(test)]
    painted_geometry: Rc<std::cell::Cell<Option<zz_ui::terminal::TerminalGeometry>>>,
    cursor_visible: bool,
    blink_task: Task<()>,
    source_task: Task<()>,
    source: Option<(String, TerminalColorScheme, Option<PathBuf>)>,
}

impl TerminalPreview {
    pub(super) fn new(appearance: TerminalAppearance, cx: &mut Context<Self>) -> Self {
        let viewport = Arc::new(sample_viewport(&appearance));
        let mut preview = Self {
            appearance: Arc::new(appearance),
            viewport,
            cache: Rc::default(),
            #[cfg(test)]
            painted_geometry: Rc::default(),
            cursor_visible: true,
            blink_task: Task::ready(()),
            source_task: Task::ready(()),
            source: None,
        };
        preview.restart_blink(cx);
        preview
    }

    pub(super) fn set_source(
        &mut self,
        source: String,
        color_scheme: TerminalColorScheme,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let key = (source, color_scheme, path);
        if self.source.as_ref() == Some(&key) {
            return;
        }
        self.source = Some(key.clone());
        self.source_task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
            let appearance = cx
                .background_executor()
                .spawn(async move {
                    let (source, color_scheme, path) = key;
                    resolve_appearance(&source, color_scheme, path)
                })
                .await;
            let _ = this.update(cx, |preview, cx| preview.set_appearance(appearance, cx));
        });
    }

    fn set_appearance(&mut self, appearance: TerminalAppearance, cx: &mut Context<Self>) {
        if *self.appearance == appearance {
            return;
        }
        self.viewport = Arc::new(sample_viewport(&appearance));
        self.appearance = Arc::new(appearance);
        self.cache = Rc::default();
        self.restart_blink(cx);
        cx.notify();
    }

    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_task = Task::ready(());
        self.cursor_visible = true;
        if !cursor_should_blink(
            self.viewport.cursor,
            self.appearance.cursor_blink_policy,
            true,
        ) {
            return;
        }
        let interval =
            Duration::from_millis(u64::from(self.appearance.cursor_blink_interval_ms).max(16));
        self.blink_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                if this
                    .update(cx, |preview, cx| {
                        preview.cursor_visible = !preview.cursor_visible;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
    }
}

impl Render for TerminalPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let appearance = Arc::clone(&self.appearance);
        let viewport = Arc::clone(&self.viewport);
        let prepaint_cache = Rc::clone(&self.cache);
        let paint_cache = Rc::clone(&self.cache);
        let cursor_visible = self.cursor_visible;
        #[cfg(test)]
        let painted_geometry = Rc::clone(&self.painted_geometry);
        let background = cx
            .theme()
            .background
            .opaque()
            .blend(terminal_background(
                appearance.background,
                appearance.background_opacity,
            ))
            .opacity(cx.theme().pane_background_opacity);
        let font = terminal_font(&appearance);
        let font_size = terminal_font_size(&appearance);
        let line_height = terminal_line_height(&appearance);
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
        let natural_height =
            probe.ascent + probe.descent + window.text_system().line_gap(font_id, font_size);
        let scale = window.scale_factor();
        let cell_height = (appearance
            .cell_height_adjustment
            .apply(f32::from(natural_height))
            .max(1.0)
            * scale)
            .round()
            / scale;
        let height = cell_height * f32::from(ROWS)
            + appearance.padding_top
            + appearance.padding_bottom
            + 2.0;
        div()
            .w_full()
            .h(px(height.max(190.0)))
            .flex_none()
            .control_surface(cx)
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
                    .line_height(line_height)
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                let paint = prepaint_cache.borrow_mut().prepaint(
                                    TerminalRenderInput {
                                        viewport: &viewport,
                                        row_revisions: &[1, 2, 3, 4, 5, 6, 7, 8],
                                        revision_epoch: 0,
                                        history: None,
                                        images: None,
                                        local_scroll_target: None,
                                        command_output: true,
                                        appearance: &appearance,
                                        appearance_hash: appearance.stable_hash(),
                                        text_opacity: 1.0,
                                        focused: true,
                                        cursor_blink_visible: cursor_visible,
                                        marked_text: None,
                                    },
                                    bounds,
                                    window,
                                    cx,
                                );
                                #[cfg(test)]
                                painted_geometry.set(Some(paint.geometry));
                                paint
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

fn resolve_appearance(
    source: &str,
    color_scheme: TerminalColorScheme,
    path: Option<PathBuf>,
) -> TerminalAppearance {
    let parsed = zz_config::parse_config(source, gpui::Font::default().family.as_ref());
    let mut defaults = AppearanceLoad::defaults_for(color_scheme);
    defaults.root = path;
    let load = apply_appearance_overrides(defaults, &parsed.daemon_entries);
    load.appearance
}

fn sample_viewport(appearance: &TerminalAppearance) -> TerminalViewport {
    let mut viewport =
        TerminalViewport::blank_with_appearance(COLUMNS, ROWS, SessionStatus::Running, appearance);
    let mut styles = vec![PackedStyle::new(
        appearance.foreground,
        appearance.background,
        None,
        0,
        UnderlineStyle::None,
    )];
    for index in 0..16 {
        styles.push(PackedStyle::new(
            appearance.palette[index],
            appearance.background,
            None,
            0,
            UnderlineStyle::None,
        ));
    }
    styles.push(PackedStyle::new(
        appearance.foreground,
        appearance.background,
        None,
        ATTR_BOLD,
        UnderlineStyle::None,
    ));
    styles.push(PackedStyle::new(
        appearance.foreground,
        appearance.background,
        None,
        ATTR_ITALIC,
        UnderlineStyle::None,
    ));
    let cells = Arc::make_mut(&mut viewport.cells);
    let mut write = |row: usize, column: usize, text: &str, style: u16| {
        for (offset, character) in text.chars().take(usize::from(COLUMNS) - column).enumerate() {
            cells[row * usize::from(COLUMNS) + column + offset] =
                PackedCell::new(character as u32, style, CellWidth::Narrow);
        }
    };
    write(0, 0, "~/projects/zz", 5);
    write(0, 15, "main", 3);
    write(1, 0, ">", 3);
    write(1, 2, "printf 'hello, terminal\\n'", 0);
    write(2, 0, "hello, terminal", 17);
    write(3, 0, "Regular", 0);
    write(3, 10, "Bold", 17);
    write(3, 17, "Italic", 18);
    for index in 0..8 {
        write(5, index * 4, "██", u16::try_from(index + 1).unwrap());
        write(6, index * 4, "██", u16::try_from(index + 9).unwrap());
    }
    write(7, 0, ">", 3);
    Arc::make_mut(&mut viewport.dictionary).styles = styles.into();
    viewport.cursor = Some(Cursor::new(
        2,
        7,
        true,
        true,
        false,
        appearance.cursor_style,
        appearance.cursor_color,
    ));
    viewport
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn rendered_font_metrics_follow_appearance_changes(cx: &mut gpui::TestAppContext) {
        cx.update(zz_ui::init);
        let mut appearance = TerminalAppearance {
            cursor_blink_policy: zz_terminal::CursorBlinkPolicy::Off,
            font_size_points: 12.0,
            ..TerminalAppearance::default()
        };
        let (preview, cx) =
            cx.add_window_view(|_, cx| TerminalPreview::new(appearance.clone(), cx));
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let before = preview.read_with(cx, |preview, _| preview.painted_geometry.get().unwrap());
        appearance.font_size_points = 24.0;
        appearance.cursor_style = zz_terminal::CursorStyle::Bar;
        preview.update(cx, |preview, cx| {
            preview.set_appearance(appearance.clone(), cx);
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let after = preview.read_with(cx, |preview, _| preview.painted_geometry.get().unwrap());
        assert!(after.cell_width > before.cell_width * 1.5);
        assert!(after.line_height > before.line_height * 1.5);
        assert!(after.grid.rows >= ROWS);
    }

    #[test]
    fn invalid_draft_fields_do_not_freeze_valid_appearance_changes() {
        let appearance = resolve_appearance(
            "font-size = 24\nwindow-padding-y = -6\ncursor-style = bar\n",
            TerminalColorScheme::Dark,
            None,
        );
        assert_eq!(appearance.font_size_points, 24.0);
        assert_eq!(appearance.cursor_style, zz_terminal::CursorStyle::Bar);
        assert_eq!(
            appearance.padding_top,
            TerminalAppearance::default().padding_top
        );
        let appearance = resolve_appearance(
            "this is not a config entry\ncursor-style = underline\n",
            TerminalColorScheme::Dark,
            None,
        );
        assert_eq!(appearance.cursor_style, zz_terminal::CursorStyle::Underline);
        let appearance = resolve_appearance(
            "theme = /definitely-missing-zz-preview-theme\nfont-size = 24\n",
            TerminalColorScheme::Dark,
            None,
        );
        assert_eq!(appearance.font_size_points, 24.0);
    }

    #[test]
    fn sample_uses_appearance_palette_and_cursor() {
        let mut appearance = TerminalAppearance::default();
        appearance.palette[1] = appearance.palette[6];
        appearance.cursor_style = zz_terminal::CursorStyle::Underline;
        appearance.cursor_color = appearance.palette[3];
        let viewport = sample_viewport(&appearance);
        let swatch = viewport.cell(5, 4).unwrap();
        assert_eq!(
            viewport.style(swatch).unwrap().foreground(),
            appearance.palette[1]
        );
        assert_eq!(
            viewport
                .style(viewport.cell(3, 10).unwrap())
                .unwrap()
                .attributes()
                & ATTR_BOLD,
            ATTR_BOLD
        );
        assert_eq!(
            viewport
                .style(viewport.cell(3, 17).unwrap())
                .unwrap()
                .attributes()
                & ATTR_ITALIC,
            ATTR_ITALIC
        );
        let cursor = viewport.cursor.unwrap();
        assert_eq!(cursor.style(), appearance.cursor_style);
        assert_eq!(cursor.color(), appearance.cursor_color);
        assert!(cursor.column() < viewport.columns);
        assert!(cursor.row() < viewport.rows);
    }
}
