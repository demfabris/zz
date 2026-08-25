use crate::{ActiveTheme as _, Colorize as _, Sizable as _, kbd::Kbd, list::ListItem, tag::Tag};
use gpui::{
    AnyElement, App, BoxShadow, CursorStyle, IntoElement, Keystroke, ParentElement as _,
    RenderOnce, SharedString, Styled as _, div, point, prelude::*, px, relative,
};

const SEARCH_HINTS: &[ChooserHint] = &[
    ChooserHint {
        keys: &["enter"],
        label: "accept",
    },
    ChooserHint {
        keys: &["escape"],
        label: "cancel",
    },
];

#[derive(Clone, Copy)]
pub struct ChooserHint {
    pub keys: &'static [&'static str],
    pub label: &'static str,
}

#[derive(Clone)]
pub struct ChooserSearch {
    pub prefix: SharedString,
    pub value: SharedString,
}

#[derive(Clone, Copy)]
pub struct ChooserRowTheme {
    pub selection_background: gpui::Hsla,
    pub primary: gpui::Hsla,
    pub foreground: gpui::Hsla,
    pub secondary_foreground: gpui::Hsla,
    pub muted_foreground: gpui::Hsla,
}

impl ChooserRowTheme {
    pub fn from_theme(cx: &App) -> Self {
        Self {
            selection_background: cx.theme().background.hover(),
            primary: cx.theme().foreground,
            foreground: cx.theme().foreground,
            secondary_foreground: cx.theme().foreground,
            muted_foreground: cx.theme().foreground.muted(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct ChooserDimensions {
    pub width: f32,
    pub max_width: f32,
    pub height: f32,
    pub min_height: f32,
    pub max_height: f32,
}

/// The chooser modal, shared by the native choosers and the WASM fixtures.
#[derive(IntoElement)]
pub struct ChooserModal {
    id: &'static str,
    title: SharedString,
    subtitle: SharedString,
    dimensions: ChooserDimensions,
    rows: AnyElement,
    close: AnyElement,
    search: Option<ChooserSearch>,
    hints: &'static [ChooserHint],
    font_family: SharedString,
}

impl ChooserModal {
    pub fn new(
        id: &'static str,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        dimensions: ChooserDimensions,
        rows: impl IntoElement,
        close: impl IntoElement,
        font_family: impl Into<SharedString>,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            subtitle: subtitle.into(),
            dimensions,
            rows: rows.into_any_element(),
            close: close.into_any_element(),
            search: None,
            hints: &[],
            font_family: font_family.into(),
        }
    }

    #[must_use]
    pub fn search(mut self, search: Option<ChooserSearch>) -> Self {
        self.search = search;
        self
    }

    #[must_use]
    pub fn hints(mut self, hints: &'static [ChooserHint]) -> Self {
        self.hints = hints;
        self
    }
}

impl RenderOnce for ChooserModal {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .relative()
            .flex()
            .flex_col()
            .w(relative(self.dimensions.width))
            .max_w(px(self.dimensions.max_width))
            .h(relative(self.dimensions.height))
            .min_h(px(self.dimensions.min_height))
            .max_h(px(self.dimensions.max_height))
            .overflow_hidden()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background.raised(1).opaque())
            .text_color(cx.theme().foreground)
            .shadow(chooser_shadow(cx))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(64.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(16.0))
                    .rounded_t(band_radius(cx))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background.raised(2))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(crate::rems_from_px(14.0))
                                    .text_color(cx.theme().foreground)
                                    .child(self.title),
                            )
                            .child(
                                div()
                                    .text_size(crate::rems_from_px(10.0))
                                    .text_color(cx.theme().foreground.muted())
                                    .child(self.subtitle),
                            ),
                    )
                    .child(div().flex_1())
                    .child(self.close),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .overflow_hidden()
                    .p(px(8.0))
                    .child(self.rows),
            )
            .child(chooser_footer(
                self.search,
                self.hints,
                self.font_family,
                cx,
            ))
    }
}

pub fn chooser_footer(
    search: Option<ChooserSearch>,
    hints: &'static [ChooserHint],
    font_family: impl Into<SharedString>,
    cx: &App,
) -> gpui::Div {
    let searching = search.is_some();
    let font_family = font_family.into();
    let search = search.map(|search| {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(7.0))
            .font_family(font_family)
            .text_size(crate::rems_from_px(11.0))
            .child(div().text_color(cx.theme().foreground).child(search.prefix))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(cx.theme().foreground)
                    .child(search.value),
            )
    });
    let hints = if searching { SEARCH_HINTS } else { hints };

    div()
        .h(px(44.0))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(12.0))
        .px(px(14.0))
        .rounded_b(band_radius(cx))
        .border_t_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.raised(2))
        .children(search)
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(crate::rems_from_px(9.0))
                .text_color(cx.theme().foreground.muted())
                .children(hints.iter().copied().map(chooser_hint)),
        )
}

#[must_use]
pub fn chooser_row(
    id: &'static str,
    index: usize,
    selected: bool,
    selection_background: gpui::Hsla,
) -> ListItem {
    ListItem::new((id, index))
        .h(px(42.0))
        .w_full()
        .cursor(CursorStyle::PointingHand)
        .when(selected, |row| row.bg(selection_background))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChooserPaneKind {
    Terminal,
    Browser,
    Agent,
    Editor,
}

fn chooser_key_label(key: &str) -> SharedString {
    if key.is_empty() {
        SharedString::default()
    } else {
        SharedString::from(format!("({key})"))
    }
}

pub fn chooser_has_key_gutter<'a>(keys: impl IntoIterator<Item = &'a str>) -> bool {
    keys.into_iter().any(|key| !key.is_empty())
}

pub fn chooser_subtitle(summary: impl Into<String>, filter_no_matches: bool) -> String {
    let summary = summary.into();
    if filter_no_matches {
        format!("{summary} · filter: no matches")
    } else {
        summary
    }
}

fn chooser_key_cell(
    key: &SharedString,
    theme: ChooserRowTheme,
    font_family: SharedString,
) -> impl IntoElement {
    div()
        .w(px(46.0))
        .flex_none()
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(font_family)
        .text_size(crate::rems_from_px(10.0))
        .text_color(theme.muted_foreground)
        .child(chooser_key_label(key))
}

pub fn tree_chooser_row(
    id: &'static str,
    index: usize,
    key: impl Into<SharedString>,
    show_key_gutter: bool,
    target: impl Into<SharedString>,
    label: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    depth: u8,
    disclosure: impl Into<SharedString>,
    pane_kind: Option<ChooserPaneKind>,
    active: bool,
    selected: bool,
    theme: ChooserRowTheme,
    font_family: impl Into<SharedString>,
) -> ListItem {
    let font_family = font_family.into();
    let key = key.into();
    chooser_row(id, index, selected, theme.selection_background)
        .pr(px(10.0))
        .pl(px(10.0))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .when(show_key_gutter, |row| {
                    row.child(chooser_key_cell(&key, theme, font_family.clone()))
                })
                .child(div().w(px(f32::from(depth) * 18.0)).flex_none())
                .child(
                    div()
                        .w(px(18.0))
                        .flex_none()
                        .text_size(crate::rems_from_px(12.0))
                        .text_color(theme.foreground.muted())
                        .child(disclosure.into()),
                )
                .child(
                    div()
                        .w(px(48.0))
                        .flex_none()
                        .font_family(font_family.clone())
                        .text_size(crate::rems_from_px(10.0))
                        .text_color(if selected {
                            theme.foreground
                        } else {
                            theme.foreground.muted()
                        })
                        .child(target.into()),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap(px(9.0))
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(crate::rems_from_px(12.0))
                                .text_color(theme.foreground)
                                .child(label.into()),
                        )
                        .child(
                            div()
                                .max_w(px(310.0))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(crate::rems_from_px(10.0))
                                .text_color(theme.foreground.muted())
                                .child(detail.into()),
                        ),
                )
                .children(pane_kind.map(|kind| {
                    let tag = match kind {
                        ChooserPaneKind::Browser => Tag::primary(),
                        ChooserPaneKind::Terminal
                        | ChooserPaneKind::Agent
                        | ChooserPaneKind::Editor => Tag::secondary(),
                    };
                    tag.small()
                        .outline()
                        .ml(px(8.0))
                        .font_family(font_family.clone())
                        .text_size(crate::rems_from_px(9.0))
                        .child(match kind {
                            ChooserPaneKind::Terminal => "TERM",
                            ChooserPaneKind::Browser => "WEB",
                            ChooserPaneKind::Agent => "AGENT",
                            ChooserPaneKind::Editor => "EDIT",
                        })
                }))
                .when(active, |row| {
                    row.child(
                        Tag::success()
                            .small()
                            .ml(px(8.0))
                            .font_family(font_family)
                            .text_size(crate::rems_from_px(9.0))
                            .child("ACTIVE"),
                    )
                }),
        )
}

pub fn buffer_chooser_row(
    id: &'static str,
    index: usize,
    key: impl Into<SharedString>,
    show_key_gutter: bool,
    name: impl Into<SharedString>,
    preview: impl Into<SharedString>,
    size: impl Into<SharedString>,
    age: impl Into<SharedString>,
    selected: bool,
    theme: ChooserRowTheme,
    font_family: impl Into<SharedString>,
) -> ListItem {
    let font_family = font_family.into();
    let key = key.into();
    chooser_row(id, index, selected, theme.selection_background)
        .gap(px(12.0))
        .px(px(10.0))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(12.0))
                .when(show_key_gutter, |row| {
                    row.child(chooser_key_cell(&key, theme, font_family.clone()))
                })
                .child(
                    div()
                        .w(px(142.0))
                        .flex_none()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_family(font_family.clone())
                        .text_size(crate::rems_from_px(11.0))
                        .text_color(theme.foreground)
                        .child(name.into()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(crate::rems_from_px(11.0))
                        .text_color(theme.foreground)
                        .child(preview.into()),
                )
                .child(
                    div()
                        .w(px(76.0))
                        .flex_none()
                        .text_right()
                        .font_family(font_family.clone())
                        .text_size(crate::rems_from_px(9.0))
                        .text_color(theme.foreground.muted())
                        .child(size.into()),
                )
                .child(
                    div()
                        .w(px(54.0))
                        .flex_none()
                        .text_right()
                        .font_family(font_family)
                        .text_size(crate::rems_from_px(9.0))
                        .text_color(theme.foreground.muted())
                        .child(age.into()),
                ),
        )
}

fn chooser_hint(hint: ChooserHint) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .children(
            hint.keys
                .iter()
                .map(|key| Kbd::new(Keystroke::parse(key).expect("static chooser keystroke"))),
        )
        .child(hint.label)
}

fn band_radius(cx: &App) -> gpui::Pixels {
    (cx.theme().radius - px(1.0)).max(px(0.0))
}

fn chooser_shadow(cx: &App) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: cx.theme().border.subtle(),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(1.0),
            inset: false,
        },
        BoxShadow {
            color: cx.theme().scrim,
            offset: point(px(0.0), px(14.0)),
            blur_radius: px(36.0),
            spread_radius: px(0.0),
            inset: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{chooser_has_key_gutter, chooser_key_label, chooser_subtitle};

    #[test]
    fn row_shortcuts_wear_the_parentheses_mode_tree_draws() {
        assert_eq!(chooser_key_label("0").as_ref(), "(0)");
        assert_eq!(chooser_key_label("M-a").as_ref(), "(M-a)");
        assert_eq!(chooser_key_label("").as_ref(), "");
    }

    #[test]
    fn key_gutter_and_filter_subtitle_follow_list_level_state() {
        assert!(chooser_has_key_gutter(["", "M-a", ""]));
        assert!(!chooser_has_key_gutter(["", ""]));
        assert_eq!(chooser_subtitle("2 buffers", false), "2 buffers");
        assert_eq!(
            chooser_subtitle("2 buffers", true),
            "2 buffers · filter: no matches"
        );
    }
}
