use std::sync::Arc;

use gpui::{
    AnyElement, App, IntoElement, Keystroke, Modifiers, ParentElement as _, Pixels, RenderOnce,
    SharedString, Size, Styled as _, TextRun, Window, div, font, prelude::*, px,
};

use crate::{ActiveTheme as _, Colorize as _, StyledExt as _, kbd::Kbd};

const ROW_HEIGHT: f32 = 22.0;
const TITLE_HEIGHT: f32 = 24.0;
const LABEL_WIDTH: f32 = 150.0;
const CAP_GAP: f32 = 8.0;
const CAP_MIN: f32 = 24.0;
const CAP_MAX: f32 = 112.0;
const SET_GAP: f32 = 6.0;
const COLUMN_GAP: f32 = 24.0;
const LINE_GAP: f32 = 12.0;
const STACK_GAP: f32 = 10.0;
const PADDING_X: f32 = 16.0;
const CHROME_HEIGHT: f32 = 56.0;
const WIDTH_SLACK: f32 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct WhichKeyCap {
    pub keys: Vec<Keystroke>,
    pub raw: SharedString,
    pub yours: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WhichKeyRow {
    pub id: SharedString,
    pub caps: Vec<WhichKeyCap>,
    pub label: SharedString,
    pub group: Option<SharedString>,
    pub repeat: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WhichKeyHeader {
    pub table: SharedString,
    pub prefix: Option<Keystroke>,
    pub prefix_raw: SharedString,
    pub more: Option<WhichKeyCap>,
}

#[derive(IntoElement)]
pub struct WhichKeyView {
    header: WhichKeyHeader,
    rows: Arc<[WhichKeyRow]>,
    available: Option<Size<Pixels>>,
}

struct Group<'a> {
    title: Option<SharedString>,
    rows: Vec<&'a WhichKeyRow>,
}

struct Block<'a> {
    title: Option<&'a SharedString>,
    titled: bool,
    rows: &'a [&'a WhichKeyRow],
}

fn groups(rows: &[WhichKeyRow]) -> Vec<Group<'_>> {
    let mut groups: Vec<Group<'_>> = Vec::new();
    for row in rows {
        match groups.iter_mut().find(|group| group.title == row.group) {
            Some(group) => group.rows.push(row),
            None => groups.push(Group {
                title: row.group.clone(),
                rows: vec![row],
            }),
        }
    }
    groups
}

fn block_height(rows: usize, titled: bool) -> f32 {
    rows as f32 * ROW_HEIGHT + if titled { TITLE_HEIGHT } else { 0.0 }
}

fn stacks<'a>(groups: &'a [Group<'a>], per_column: usize) -> Vec<Vec<Block<'a>>> {
    let per_column = per_column.max(1);
    let mut stacks: Vec<Vec<Block<'a>>> = Vec::new();
    let mut open: Option<f32> = None;
    for group in groups {
        let titled = group.title.is_some();
        let whole = group.rows.len() <= per_column;
        let height = block_height(group.rows.len(), titled);
        if whole
            && let Some(used) = open
            && used + STACK_GAP + height <= block_height(per_column, titled)
            && let Some(stack) = stacks.last_mut()
        {
            stack.push(Block {
                title: group.title.as_ref(),
                titled,
                rows: &group.rows,
            });
            open = Some(used + STACK_GAP + height);
            continue;
        }
        for (index, chunk) in group.rows.chunks(per_column).enumerate() {
            stacks.push(vec![Block {
                title: group.title.as_ref().filter(|_| index == 0),
                titled,
                rows: chunk,
            }]);
        }
        open = whole.then_some(height);
    }
    stacks
}

fn stack_height(stack: &[Block<'_>]) -> f32 {
    stack
        .iter()
        .map(|block| block_height(block.rows.len(), block.titled))
        .sum::<f32>()
        + stack.len().saturating_sub(1) as f32 * STACK_GAP
}

fn layout_height(stacks: &[Vec<Block<'_>>], per_line: usize) -> f32 {
    let lines = stacks.chunks(per_line.max(1));
    let count = lines.len();
    lines
        .map(|line| {
            line.iter()
                .map(|stack| stack_height(stack))
                .fold(0.0, f32::max)
        })
        .sum::<f32>()
        + count.saturating_sub(1) as f32 * LINE_GAP
}

fn per_line(width: Pixels, column_width: f32) -> usize {
    let inner = f32::from(width) - 2.0 * PADDING_X - WIDTH_SLACK;
    let fits = ((inner + COLUMN_GAP) / (column_width + COLUMN_GAP)).floor();
    if fits.is_finite() && fits >= 1.0 {
        fits as usize
    } else {
        1
    }
}

fn line_width(columns: usize, column_width: f32) -> Pixels {
    let columns = columns.max(1) as f32;
    px(columns * column_width + (columns - 1.0) * COLUMN_GAP + 2.0 * PADDING_X + WIDTH_SLACK)
}

struct Fit {
    per_column: usize,
    width: Option<Pixels>,
}

fn fit(groups: &[Group<'_>], column_width: f32, available: Option<Size<Pixels>>) -> Fit {
    let tallest = groups
        .iter()
        .map(|group| group.rows.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let Some(available) = available else {
        return Fit {
            per_column: tallest,
            width: None,
        };
    };
    let per_line = per_line(available.width, column_width);
    let budget = f32::from(available.height) - CHROME_HEIGHT;
    let mut best = (f32::INFINITY, tallest, 1);
    for per_column in (1..=tallest).rev() {
        let stacks = stacks(groups, per_column);
        let height = layout_height(&stacks, per_line);
        if height <= budget {
            best = (height, per_column, stacks.len());
            break;
        }
        if height < best.0 {
            best = (height, per_column, stacks.len());
        }
    }
    Fit {
        per_column: best.1,
        width: Some(line_width(best.2.min(per_line), column_width)),
    }
}

fn cap_text(cap: &WhichKeyCap) -> SharedString {
    let [first, rest @ ..] = cap.keys.as_slice() else {
        return cap.raw.clone();
    };
    let full = |key: &Keystroke| Kbd::format_key(key, true);
    if rest.is_empty() {
        return full(first).into();
    }
    let spelled = || {
        cap.keys
            .iter()
            .map(full)
            .collect::<Vec<_>>()
            .join(" ")
            .into()
    };
    if rest.iter().any(|key| key.modifiers != first.modifiers) {
        return spelled();
    }
    let symbols: Vec<String> = cap
        .keys
        .iter()
        .map(|key| {
            Kbd::format_key(
                &Keystroke {
                    modifiers: Modifiers::default(),
                    ..key.clone()
                },
                true,
            )
        })
        .collect();
    let head = full(first);
    let modifiers = head.strip_suffix(symbols[0].as_str()).unwrap_or_default();
    let digits: Option<Vec<u32>> = symbols
        .iter()
        .map(|symbol| symbol.parse().ok().filter(|_| symbol.len() == 1))
        .collect();
    if let Some(digits) = digits
        && digits.len() >= 3
        && digits.windows(2).all(|pair| pair[1] == pair[0] + 1)
    {
        return format!("{modifiers}{}–{}", symbols[0], symbols[symbols.len() - 1]).into();
    }
    let single = symbols.iter().all(|symbol| symbol.chars().count() == 1);
    if single
        && symbols
            .iter()
            .all(|symbol| !symbol.chars().all(char::is_alphanumeric))
    {
        return format!("{modifiers}{}", symbols.concat()).into();
    }
    if modifiers.is_empty() {
        return symbols.join(" ").into();
    }
    spelled()
}

fn text_width(text: &str, window: &Window, cx: &App) -> f32 {
    let run = TextRun {
        len: text.len(),
        font: font(cx.theme().mono_font_family.clone()),
        color: cx.theme().foreground,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let size = crate::rems_from_px(11.0).to_pixels(window.rem_size());
    f32::from(
        window
            .text_system()
            .shape_line(text.to_owned().into(), size, &[run], None)
            .width,
    )
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

    fn cap(cap: &WhichKeyCap, cx: &App) -> impl IntoElement {
        div()
            .flex_none()
            .whitespace_nowrap()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(crate::rems_from_px(11.0))
            .text_color(if cap.yours {
                cx.theme().accent
            } else {
                cx.theme().foreground.muted()
            })
            .child(cap_text(cap))
    }

    fn row(row: &WhichKeyRow, cap_width: f32, cx: &App) -> impl IntoElement {
        let selector = format!("which-key-row-{}", row.id);
        div()
            .debug_selector(move || selector)
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .gap(px(CAP_GAP))
            .min_w_0()
            .child(
                div()
                    .w(px(cap_width))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .gap(px(SET_GAP))
                    .overflow_hidden()
                    .children(row.caps.iter().map(|cap| Self::cap(cap, cx))),
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
                let selector = format!("which-key-repeat-{}", row.id);
                element.child(
                    div()
                        .debug_selector(move || selector)
                        .flex_none()
                        .text_size(crate::rems_from_px(11.0))
                        .text_color(cx.theme().foreground.muted())
                        .child("↻"),
                )
            })
    }

    fn block(block: &Block<'_>, cap_width: f32, cx: &App) -> impl IntoElement {
        let selector = block.title.map(|title| format!("which-key-group-{title}"));
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .when(block.titled, |element| {
                element.child(
                    div()
                        .when_some(selector, |element, selector| {
                            element.debug_selector(move || selector)
                        })
                        .h(px(TITLE_HEIGHT - 4.0))
                        .pl(px(cap_width + CAP_GAP))
                        .text_size(crate::rems_from_px(11.0))
                        .line_height(px(16.0))
                        .text_color(cx.theme().foreground.muted())
                        .children(block.title.cloned()),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(block.rows.iter().map(|row| Self::row(row, cap_width, cx))),
            )
    }
}

impl RenderOnce for WhichKeyView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let cap_width = self
            .rows
            .iter()
            .map(|row| {
                row.caps
                    .iter()
                    .map(|cap| text_width(&cap_text(cap), window, cx))
                    .sum::<f32>()
                    + row.caps.len().saturating_sub(1) as f32 * SET_GAP
            })
            .fold(CAP_MIN, f32::max)
            .min(CAP_MAX)
            .ceil();
        let column_width = cap_width + CAP_GAP + LABEL_WIDTH;
        let groups = groups(&self.rows);
        let fit = fit(&groups, column_width, self.available);
        let header = &self.header;
        let body = stacks(&groups, fit.per_column)
            .into_iter()
            .map(|stack| {
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(px(STACK_GAP))
                    .w(px(column_width))
                    .children(
                        stack
                            .iter()
                            .map(|block| Self::block(block, cap_width, cx).into_any_element()),
                    )
                    .into_any_element()
            })
            .collect::<Vec<AnyElement>>();
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
                    .child(match header.prefix.clone() {
                        Some(key) => Kbd::new(key).lowercase().into_any_element(),
                        None => Self::cap(
                            &WhichKeyCap {
                                keys: Vec::new(),
                                raw: header.prefix_raw.clone(),
                                yours: false,
                            },
                            cx,
                        )
                        .into_any_element(),
                    })
                    .child(header.table.clone())
                    .when_some(header.more.as_ref(), |element, more| {
                        element.child(
                            div()
                                .debug_selector(|| "which-key-more".to_owned())
                                .ml_auto()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .text_color(cx.theme().foreground.muted())
                                .child(Self::cap(more, cx))
                                .child("all keys"),
                        )
                    }),
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

    fn cap(keys: &[&str], yours: bool) -> WhichKeyCap {
        WhichKeyCap {
            keys: keys
                .iter()
                .filter_map(|key| Keystroke::parse(key).ok())
                .collect(),
            raw: keys.join(" ").into(),
            yours,
        }
    }

    fn row(key: &str, label: &str, group: Option<&str>) -> WhichKeyRow {
        WhichKeyRow {
            id: key.to_owned().into(),
            caps: vec![cap(&[key], false)],
            label: label.to_owned().into(),
            group: group.map(|group| group.to_owned().into()),
            repeat: false,
        }
    }

    fn titles(stack: &[Block<'_>]) -> Vec<String> {
        stack
            .iter()
            .map(|block| block.title.map(ToString::to_string).unwrap_or_default())
            .collect()
    }

    #[test]
    fn small_groups_stack_under_each_other() {
        let rows: Vec<_> = [
            ("Panes", 7),
            ("Windows", 7),
            ("Sessions", 3),
            ("Copy", 2),
            ("Other", 2),
        ]
        .into_iter()
        .flat_map(|(group, count)| {
            (0..count).map(move |index| row(&format!("f{index}"), "Thing", Some(group)))
        })
        .collect();
        let groups = groups(&rows);
        let packed: Vec<_> = stacks(&groups, 7)
            .iter()
            .map(|stack| titles(stack))
            .collect();
        assert_eq!(
            packed,
            [
                vec!["Panes"],
                vec!["Windows"],
                vec!["Sessions", "Copy"],
                vec!["Other"]
            ]
        );
        let split = stacks(&groups, 4);
        assert_eq!(titles(&split[0]), ["Panes"]);
        assert_eq!(titles(&split[1]), [""]);
        assert!(split[1][0].titled);
    }

    #[test]
    fn caps_fold_families_and_ranges() {
        let text = |keys: &[&str]| cap_text(&cap(keys, false)).to_string();
        let ctrl = Kbd::format(&Keystroke::parse("ctrl-a").unwrap());
        let ctrl = ctrl.trim_end_matches('A');
        assert_eq!(text(&["0", "1", "2", "3"]), "0–3");
        assert_eq!(text(&["h", "j", "k", "l"]), "h j k l");
        assert_eq!(text(&["{", "}"]), "{}");
        let arrows = ["ctrl-left", "ctrl-down"];
        let expected = if cfg!(any(target_os = "macos", target_os = "ios")) {
            format!(
                "{ctrl}{}{}",
                Kbd::format(&Keystroke::parse("left").unwrap()),
                Kbd::format(&Keystroke::parse("down").unwrap())
            )
        } else {
            arrows
                .map(|key| Kbd::format_key(&Keystroke::parse(key).unwrap(), true))
                .join(" ")
        };
        assert_eq!(text(&arrows), expected);
        assert_eq!(
            cap_text(&WhichKeyCap {
                keys: Vec::new(),
                raw: "M-F13".into(),
                yours: false
            }),
            "M-F13"
        );
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
                            more: Some(cap(&["?"], false)),
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
            row("c", "New window", Some("Windows")),
            WhichKeyRow {
                caps: vec![cap(&["left", "down", "up", "right"], false)],
                repeat: true,
                ..row("left", "Focus pane", Some("Panes"))
            },
            WhichKeyRow {
                caps: vec![WhichKeyCap {
                    keys: Vec::new(),
                    raw: "M-F13".into(),
                    yours: false,
                }],
                ..row("M-F13", "Odd key", Some("Other"))
            },
            WhichKeyRow {
                caps: vec![cap(&["x"], true)],
                ..row("x", "Mine", Some("Yours"))
            },
        ]);
        let (host, cx) = host(rows, None, cx);
        assert!(cx.debug_bounds("which-key").is_some());
        for selector in [
            "which-key-row-c",
            "which-key-row-left",
            "which-key-row-M-F13",
            "which-key-row-x",
            "which-key-repeat-left",
            "which-key-group-Windows",
            "which-key-group-Panes",
            "which-key-group-Other",
            "which-key-group-Yours",
            "which-key-more",
        ] {
            assert!(cx.debug_bounds(selector).is_some(), "{selector} missing");
        }
        assert!(cx.debug_bounds("which-key-repeat-c").is_none());
        cx.simulate_keystrokes("a b");
        cx.run_until_parked();
        assert_eq!(host.read_with(cx, |host, _| host.keys), 2);
        assert!(cx.update(|window, cx| host.read(cx).focus.is_focused(window)));
    }

    #[gpui::test]
    fn a_tall_group_splits_into_columns_that_fit_the_box(cx: &mut TestAppContext) {
        let rows: Arc<[WhichKeyRow]> = (0..40)
            .map(|index| row(&format!("f{}", index + 1), "Pane thing", Some("Panes")))
            .chain((0..10).map(|index| {
                row(
                    &format!("alt-f{}", index + 1),
                    "Window thing",
                    Some("Windows"),
                )
            }))
            .collect();
        let available = gpui::size(px(1100.0), px(360.0));
        let (_, cx) = host(rows, Some(available), cx);
        let sheet = cx.debug_bounds("which-key").unwrap();
        assert!(sheet.size.height <= available.height, "{sheet:?}");
        assert!(sheet.size.width <= available.width, "{sheet:?}");
        let first = cx.debug_bounds("which-key-row-f1").unwrap();
        let last = cx.debug_bounds("which-key-row-f40").unwrap();
        assert!(last.origin.x > first.origin.x);
        assert!(last.bottom() <= sheet.bottom());
        let windows = cx.debug_bounds("which-key-row-alt-f10").unwrap();
        assert!(windows.bottom() <= sheet.bottom());
    }
}
