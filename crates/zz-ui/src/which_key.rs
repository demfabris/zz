use std::sync::Arc;

use gpui::{
    AnyElement, App, IntoElement, Keystroke, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::*, px,
};

use crate::{ActiveTheme as _, Colorize as _, StyledExt as _, kbd::Kbd};

pub const WHICH_KEY_YOURS: &str = "Yours";

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
}

struct Column<'a> {
    title: Option<SharedString>,
    rows: Vec<&'a WhichKeyRow>,
}

fn columns(rows: &[WhichKeyRow]) -> Vec<Column<'_>> {
    if rows.iter().all(|row| row.group.is_none() && !row.yours) {
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

impl WhichKeyView {
    #[must_use]
    pub fn new(header: WhichKeyHeader, rows: impl Into<Arc<[WhichKeyRow]>>) -> Self {
        Self {
            header,
            rows: rows.into(),
        }
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

    fn column(column: &Column<'_>, flat: bool, cx: &App) -> impl IntoElement {
        let body = div()
            .flex()
            .gap_x(px(16.0))
            .when(flat, |body| body.flex_row().flex_wrap())
            .when(!flat, gpui::Styled::flex_col)
            .children(column.rows.iter().map(|row| {
                div()
                    .when(flat, |cell| cell.w(px(240.0)))
                    .child(Self::row(row, cx))
            }));
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .min_w(px(200.0))
            .when(flat, gpui::Styled::flex_1)
            .children(column.title.clone().map(|title| {
                let selector = format!("which-key-group-{title}");
                div()
                    .debug_selector(move || selector)
                    .h(px(20.0))
                    .pl(px(64.0))
                    .text_size(crate::rems_from_px(11.0))
                    .line_height(px(16.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(title)
            }))
            .child(body)
    }
}

impl RenderOnce for WhichKeyView {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let columns = columns(&self.rows);
        let flat = columns.len() == 1 && columns[0].title.is_none();
        let header = &self.header;
        div()
            .id("which-key")
            .debug_selector(|| "which-key".to_owned())
            .w_full()
            .occlude()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .px(px(16.0))
            .py(px(12.0))
            .popover_style(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(crate::rems_from_px(12.0))
                    .line_height(px(16.0))
                    .child(Self::cap(header.prefix.as_ref(), &header.prefix_raw, cx))
                    .child(header.table.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_x(px(24.0))
                    .gap_y(px(12.0))
                    .children(columns.iter().map(|column| Self::column(column, flat, cx))),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Render, TestAppContext};

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
    }

    struct Host {
        focus: gpui::FocusHandle,
        rows: Arc<[WhichKeyRow]>,
        keys: usize,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|host, _: &gpui::KeyDownEvent, _, _| host.keys += 1))
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .child(WhichKeyView::new(
                            WhichKeyHeader {
                                table: "prefix".into(),
                                prefix: Keystroke::parse("ctrl-b").ok(),
                                prefix_raw: "C-b".into(),
                            },
                            Arc::clone(&self.rows),
                        )),
                )
        }
    }

    #[gpui::test]
    fn sheet_renders_rows_without_taking_focus_or_keys(cx: &mut TestAppContext) {
        cx.update(crate::init);
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
        let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
        let captured = std::rc::Rc::clone(&slot);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let host = cx.new(|cx| Host {
                focus: cx.focus_handle(),
                rows,
                keys: 0,
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
}
