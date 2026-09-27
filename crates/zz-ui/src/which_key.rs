use std::sync::Arc;

use gpui::{
    AnyElement, App, IntoElement, Keystroke, ParentElement as _, Pixels, RenderOnce, SharedString,
    Size, Styled as _, Window, div, prelude::*, px,
};

use crate::{ActiveTheme as _, Colorize as _, StyledExt as _, kbd::Kbd};

pub const WHICH_KEY_YOURS: &str = "Yours";

const ROW_HEIGHT: f32 = 22.0;
const COLUMN_WIDTH: f32 = 240.0;
const COLUMN_GAP: f32 = 24.0;
const LINE_GAP: f32 = 12.0;
const TITLE_HEIGHT: f32 = 24.0;
const PADDING_X: f32 = 16.0;
const WIDTH_SLACK: f32 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct WhichKeyRow {
    pub key: Option<Keystroke>,
    pub raw: SharedString,
    pub label: SharedString,
    pub group: Option<SharedString>,
    pub repeat: bool,
    pub yours: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WhichKeyHeader {
    pub table: SharedString,
    pub prefix: Option<Keystroke>,
    pub prefix_raw: SharedString,
}

#[derive(IntoElement)]
pub struct WhichKeyView {
    header: WhichKeyHeader,
    rows: Arc<[WhichKeyRow]>,
    available: Option<Size<Pixels>>,
}

struct Column<'a> {
    title: Option<SharedString>,
    rows: Vec<&'a WhichKeyRow>,
}

fn columns(rows: &[WhichKeyRow]) -> Vec<Column<'_>> {
    if rows.iter().all(|row| row.group.is_none()) {
        return vec![Column {
            title: None,
            rows: rows.iter().collect(),
        }];
    }
    let mut columns: Vec<Column<'_>> = Vec::new();
    let mut yours = Vec::new();
    for row in rows {
        if row.yours {
            yours.push(row);
            continue;
        }
        let title = row.group.clone();
        match columns.iter_mut().find(|column| column.title == title) {
            Some(column) => column.rows.push(row),
            None => columns.push(Column {
                title,
                rows: vec![row],
            }),
        }
    }
    if !yours.is_empty() {
        columns.push(Column {
            title: Some(WHICH_KEY_YOURS.into()),
            rows: yours,
        });
    }
    columns
}

struct Fit {
    rows_per_column: usize,
    width: Option<Pixels>,
}

fn per_line(width: Pixels) -> usize {
    let inner = f32::from(width) - 2.0 * PADDING_X - WIDTH_SLACK;
    let fits = ((inner + COLUMN_GAP) / (COLUMN_WIDTH + COLUMN_GAP)).floor();
    if fits.is_finite() && fits >= 1.0 {
        fits as usize
    } else {
        1
    }
}

fn line_width(columns: usize) -> Pixels {
    let columns = columns.max(1) as f32;
    px(columns * COLUMN_WIDTH + (columns - 1.0) * COLUMN_GAP + 2.0 * PADDING_X + WIDTH_SLACK)
}

fn fit(columns: &[Column<'_>], flat: bool, available: Option<Size<Pixels>>) -> Fit {
    let tallest = columns
        .iter()
        .map(|column| column.rows.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let Some(available) = available else {
        return Fit {
            rows_per_column: tallest,
            width: None,
        };
    };
    let per_line = per_line(available.width);
    if flat {
        return Fit {
            rows_per_column: tallest,
            width: Some(line_width(tallest.min(per_line))),
        };
    }
    let mut best = (f32::INFINITY, tallest, 1);
    for rows_per_column in 1..=tallest {
        let split = columns
            .iter()
            .map(|column| column.rows.len().div_ceil(rows_per_column))
            .sum::<usize>();
        let lines = split.div_ceil(per_line) as f32;
        let height =
            lines * (TITLE_HEIGHT + rows_per_column as f32 * ROW_HEIGHT) + (lines - 1.0) * LINE_GAP;
        if height <= best.0 {
            best = (height, rows_per_column, split);
        }
    }
    Fit {
        rows_per_column: best.1,
        width: Some(line_width(best.2.min(per_line))),
    }
}

impl WhichKeyView {
    #[must_use]
    pub fn new(header: WhichKeyHeader, rows: impl Into<Arc<[WhichKeyRow]>>) -> Self {
        Self {
            header,
            rows: rows.into(),
            available: None,
        }
    }

    #[must_use]
    pub const fn fit(mut self, available: Size<Pixels>) -> Self {
        self.available = Some(available);
        self
    }

    fn cap(key: Option<&Keystroke>, raw: &SharedString, cx: &App) -> AnyElement {
        match key {
            Some(key) => Kbd::new(key.clone()).lowercase().into_any_element(),
            None => div()
                .flex_shrink_0()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(crate::rems_from_px(11.0))
                .text_color(cx.theme().foreground.muted())
                .child(raw.clone())
                .into_any_element(),
        }
    }

    fn row(row: &WhichKeyRow, cx: &App) -> impl IntoElement {
        let selector = format!("which-key-row-{}", row.raw);
        div()
            .debug_selector(move || selector)
            .h(px(22.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .min_w_0()
            .child(
                div()
                    .w(px(56.0))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .child(Self::cap(row.key.as_ref(), &row.raw, cx)),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(crate::rems_from_px(12.0))
                    .line_height(px(16.0))
                    .child(row.label.clone()),
            )
            .when(row.repeat, |element| {
                let selector = format!("which-key-repeat-{}", row.raw);
                element.child(
                    div()
                        .debug_selector(move || selector)
                        .flex_none()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(crate::rems_from_px(10.0))
                        .text_color(cx.theme().foreground.muted())
                        .child("repeat"),
                )
            })
    }

    fn title(title: Option<&SharedString>, cx: &App) -> impl IntoElement {
        let selector = title.map(|title| format!("which-key-group-{title}"));
        div()
            .when_some(selector, |element, selector| {
                element.debug_selector(move || selector)
            })
            .h(px(TITLE_HEIGHT - 4.0))
            .pl(px(64.0))
            .text_size(crate::rems_from_px(11.0))
            .line_height(px(16.0))
            .text_color(cx.theme().foreground.muted())
            .children(title.cloned())
    }

    fn flat(column: &Column<'_>, cx: &App) -> AnyElement {
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_x(px(COLUMN_GAP))
            .children(
                column
                    .rows
                    .iter()
                    .map(|row| div().w(px(COLUMN_WIDTH)).child(Self::row(row, cx))),
            )
            .into_any_element()
    }

    fn grouped(column: &Column<'_>, rows_per_column: usize, cx: &App) -> Vec<AnyElement> {
        column
            .rows
            .chunks(rows_per_column.max(1))
            .enumerate()
            .map(|(index, rows)| {
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(px(4.0))
                    .w(px(COLUMN_WIDTH))
                    .child(Self::title(
                        column.title.as_ref().filter(|_| index == 0),
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .children(rows.iter().map(|row| Self::row(row, cx))),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

impl RenderOnce for WhichKeyView {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let columns = columns(&self.rows);
        let flat = columns.len() == 1 && columns[0].title.is_none();
        let fit = fit(&columns, flat, self.available);
        let header = &self.header;
        let body = if flat {
            vec![Self::flat(&columns[0], cx)]
        } else {
            columns
                .iter()
                .flat_map(|column| Self::grouped(column, fit.rows_per_column, cx))
                .collect()
        };
        div()
            .id("which-key")
            .debug_selector(|| "which-key".to_owned())
            .w_full()
            .when_some(fit.width, gpui::Styled::max_w)
            .when_some(self.available, |element, available| {
                element.max_h(available.height)
            })
            .occlude()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .px(px(PADDING_X))
            .py(px(12.0))
            .popover_style(cx)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(crate::rems_from_px(12.0))
                    .line_height(px(16.0))
                    .child(Self::cap(header.prefix.as_ref(), &header.prefix_raw, cx))
                    .child(header.table.clone()),
            )
            .child(
                div()
                    .id("which-key-body")
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_x(px(COLUMN_GAP))
                    .gap_y(px(LINE_GAP))
                    .children(body),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Entity, Render, TestAppContext};

    use super::*;
    use crate::Root;

    fn row(key: &str, label: &str, group: Option<&str>, yours: bool) -> WhichKeyRow {
        WhichKeyRow {
            key: Keystroke::parse(key).ok(),
            raw: key.to_owned().into(),
            label: label.to_owned().into(),
            group: group.map(|group| group.to_owned().into()),
            repeat: false,
            yours,
        }
    }

    #[test]
    fn yours_goes_last_and_groups_keep_first_seen_order() {
        let rows = [
            row("x", "Mine", Some("Panes"), true),
            row("c", "New window", Some("Windows"), false),
            row("o", "Next pane", Some("Panes"), false),
            row("n", "Next window", Some("Windows"), false),
        ];
        let titles = columns(&rows)
            .iter()
            .map(|column| column.title.clone().map(|title| title.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            [
                Some("Windows".to_owned()),
                Some("Panes".to_owned()),
                Some("Yours".to_owned())
            ]
        );
        let flat = [row("a", "One", None, false), row("b", "Two", None, false)];
        let flat = columns(&flat);
        assert_eq!(flat.len(), 1);
        assert!(flat[0].title.is_none());
        assert_eq!(flat[0].rows.len(), 2);
        let custom = [row("a", "One", None, true), row("b", "Two", None, true)];
        let custom = columns(&custom);
        assert_eq!(custom.len(), 1);
        assert!(custom[0].title.is_none());
        assert_eq!(custom[0].rows.len(), 2);
    }

    struct Host {
        focus: gpui::FocusHandle,
        rows: Arc<[WhichKeyRow]>,
        keys: usize,
        available: Option<gpui::Size<gpui::Pixels>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|host, _: &gpui::KeyDownEvent, _, _| host.keys += 1))
                .child(div().absolute().bottom_0().left_0().right_0().child({
                    let view = WhichKeyView::new(
                        WhichKeyHeader {
                            table: "prefix".into(),
                            prefix: Keystroke::parse("ctrl-b").ok(),
                            prefix_raw: "C-b".into(),
                        },
                        Arc::clone(&self.rows),
                    );
                    match self.available {
                        Some(available) => view.fit(available),
                        None => view,
                    }
                }))
        }
    }

    fn host(
        rows: Arc<[WhichKeyRow]>,
        available: Option<gpui::Size<gpui::Pixels>>,
        cx: &mut TestAppContext,
    ) -> (Entity<Host>, &mut gpui::VisualTestContext) {
        cx.update(crate::init);
        let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
        let captured = std::rc::Rc::clone(&slot);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let host = cx.new(|cx| Host {
                focus: cx.focus_handle(),
                rows,
                keys: 0,
                available,
            });
            let focus = host.read(cx).focus.clone();
            focus.focus(window, cx);
            captured.replace(Some(host.clone()));
            Root::new(host, window, cx)
        });
        let host = slot.borrow_mut().take().expect("host");
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        (host, cx)
    }

    #[gpui::test]
    fn sheet_renders_rows_without_taking_focus_or_keys(cx: &mut TestAppContext) {
        let rows: Arc<[WhichKeyRow]> = Arc::from(vec![
            row("c", "New window", Some("Windows"), false),
            WhichKeyRow {
                repeat: true,
                ..row(
                    "up",
                    "Select the pane above the active pane",
                    Some("Panes"),
                    false,
                )
            },
            WhichKeyRow {
                key: None,
                raw: "M-F13".into(),
                ..row("x", "Odd key", Some("Other"), false)
            },
            row("x", "Mine", None, true),
        ]);
        let (host, cx) = host(rows, None, cx);
        assert!(cx.debug_bounds("which-key").is_some());
        for selector in [
            "which-key-row-c",
            "which-key-row-up",
            "which-key-row-M-F13",
            "which-key-row-x",
            "which-key-repeat-up",
            "which-key-group-Windows",
            "which-key-group-Panes",
            "which-key-group-Other",
            "which-key-group-Yours",
        ] {
            assert!(cx.debug_bounds(selector).is_some(), "{selector} missing");
        }
        assert!(cx.debug_bounds("which-key-repeat-c").is_none());
        let windows = cx.debug_bounds("which-key-group-Windows").unwrap();
        let yours = cx.debug_bounds("which-key-group-Yours").unwrap();
        assert!(yours.origin.x > windows.origin.x || yours.origin.y > windows.origin.y);
        cx.simulate_keystrokes("a b");
        cx.run_until_parked();
        assert_eq!(host.read_with(cx, |host, _| host.keys), 2);
        assert!(cx.update(|window, cx| host.read(cx).focus.is_focused(window)));
    }

    #[gpui::test]
    fn a_tall_group_splits_into_columns_that_fit_the_box(cx: &mut TestAppContext) {
        let rows: Arc<[WhichKeyRow]> = (0..40)
            .map(|index| {
                row(
                    &format!("F{}", index + 1),
                    "Pane thing",
                    Some("Panes"),
                    false,
                )
            })
            .chain((0..10).map(|index| {
                row(
                    &format!("M-F{}", index + 1),
                    "Window thing",
                    Some("Windows"),
                    false,
                )
            }))
            .collect();
        let available = gpui::size(px(1100.0), px(360.0));
        let (_, cx) = host(rows, Some(available), cx);
        let sheet = cx.debug_bounds("which-key").unwrap();
        assert!(sheet.size.height <= available.height, "{sheet:?}");
        assert!(sheet.size.width <= available.width, "{sheet:?}");
        let first = cx.debug_bounds("which-key-row-F1").unwrap();
        let last = cx.debug_bounds("which-key-row-F40").unwrap();
        assert!(last.origin.x > first.origin.x);
        assert!(last.bottom() <= sheet.bottom());
        let windows = cx.debug_bounds("which-key-group-Windows").unwrap();
        assert!(windows.bottom() <= sheet.bottom());
    }
}
