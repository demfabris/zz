use crate::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _, StyledExt as _, kbd::Kbd,
    list::ListItem, tag::Tag,
};
use gpui::{
    AnyElement, App, BoxShadow, CursorStyle, IntoElement, Keystroke, ParentElement as _,
    RenderOnce, SharedString, Styled as _, div, point, prelude::*, px, relative,
};

pub const CHOOSER_ROW_HEIGHT: f32 = 40.0;

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
            selection_background: crate::navigation::workspace_row_highlight(cx),
            primary: cx.theme().foreground,
            foreground: cx.theme().foreground,
            secondary_foreground: cx.theme().foreground,
            muted_foreground: cx.theme().foreground.muted(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct ChooserDimensions {
    pub max_width: f32,
    pub row_count: usize,
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
    prompt: Option<SharedString>,
    help: bool,
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
            prompt: None,
            help: false,
            search: None,
            hints: &[],
            font_family: font_family.into(),
        }
    }

    /// The single-key confirmation the mode owns, drawn above the rows the way
    /// `mode_tree_draw` puts `mtd->prompt` there. Rows keep answering keys
    /// while it stands, so a client that does not draw it kills silently.
    #[must_use]
    pub fn prompt(mut self, prompt: Option<SharedString>) -> Self {
        self.prompt = prompt;
        self
    }

    /// `mtd->help`: the mode's help screen stands until the next key of any
    /// kind, and that key does nothing else. A client that draws nothing here
    /// eats a key its viewer meant for a row.
    #[must_use]
    pub const fn help(mut self, help: bool) -> Self {
        self.help = help;
        self
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
    fn render(self, window: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        let rows = f32::from(u8::try_from(self.dimensions.row_count.min(10)).unwrap_or(10));
        let notices = u8::from(self.help) + u8::from(self.prompt.is_some());
        let height = 98.0 + rows * CHOOSER_ROW_HEIGHT + f32::from(notices) * 36.0;
        div()
            .id(self.id)
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(self.dimensions.max_width))
            .h(px(height))
            .max_h((window.viewport_size().height - px(44.0)).max(px(0.0)))
            .overflow_hidden()
            .rounded(cx.theme().radius + px(4.0))
            .control_surface(cx)
            .bg(cx.theme().background.raised(1).opaque())
            .text_color(cx.theme().foreground)
            .shadow(chooser_shadow(cx))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(56.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(12.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(crate::rems_from_px(13.0))
                                    .font_medium()
                                    .text_color(cx.theme().foreground)
                                    .child(self.title),
                            )
                            .child(
                                div()
                                    .text_size(crate::rems_from_px(11.0))
                                    .truncate()
                                    .text_color(cx.theme().foreground.muted())
                                    .child(self.subtitle),
                            ),
                    )
                    .child(div().flex_1())
                    .child(self.close),
            )
            .when(self.help, |modal| {
                modal.child(
                    div()
                        .flex_none()
                        .px(px(12.0))
                        .py(px(9.0))
                        .border_b_1()
                        .border_color(cx.theme().border())
                        .bg(cx.theme().background.raised(2))
                        .font_family(self.font_family.clone())
                        .text_size(crate::rems_from_px(11.0))
                        .text_color(cx.theme().foreground)
                        .child("Help: the next key closes this and does nothing else"),
                )
            })
            .children(self.prompt.map(|prompt| {
                div()
                    .flex_none()
                    .px(px(12.0))
                    .py(px(9.0))
                    .border_b_1()
                    .border_color(cx.theme().border())
                    .bg(cx.theme().background.raised(2))
                    .font_family(self.font_family.clone())
                    .text_size(crate::rems_from_px(11.0))
                    .text_color(cx.theme().foreground)
                    .child(prompt)
            }))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .overflow_hidden()
                    .min_h_0()
                    .p(px(4.0))
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
        .h(px(34.0))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(12.0))
        .px(px(12.0))
        .border_t(px(0.5))
        .border_color(cx.theme().foreground.opacity(0.1))
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
        .h(px(CHOOSER_ROW_HEIGHT))
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
        .w(px(30.0))
        .flex_none()
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(font_family)
        .text_size(crate::rems_from_px(10.0))
        .text_color(theme.muted_foreground)
        .child(chooser_key_label(key))
}

#[allow(clippy::fn_params_excessive_bools)]
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
    tagged: bool,
    selected: bool,
    theme: ChooserRowTheme,
    font_family: impl Into<SharedString>,
) -> ListItem {
    let font_family = font_family.into();
    let key = key.into();
    let target = target.into();
    let disclosure = disclosure.into();
    let icon = match pane_kind {
        Some(ChooserPaneKind::Terminal) => IconName::SquareTerminal,
        Some(ChooserPaneKind::Browser) => IconName::Globe,
        Some(ChooserPaneKind::Agent) => IconName::Bot,
        Some(ChooserPaneKind::Editor) => IconName::File,
        None if target.starts_with('$') => IconName::Folder,
        None => IconName::PanelsTopLeft,
    };
    chooser_row(id, index, selected, theme.selection_background)
        .px(px(8.0))
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .when(show_key_gutter, |row| {
                    row.child(chooser_key_cell(&key, theme, font_family.clone()))
                })
                .child(div().w(px(f32::from(depth) * 16.0)).flex_none())
                .child(
                    div()
                        .w(px(16.0))
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .when(!disclosure.is_empty(), |slot| {
                            slot.child(
                                Icon::new(if disclosure == "▾" {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(px(12.0)),
                            )
                        }),
                )
                .child(
                    Icon::new(icon)
                        .size(px(14.0))
                        .flex_none()
                        .text_color(if active {
                            theme.foreground
                        } else {
                            theme.muted_foreground
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .ml(px(8.0))
                        .truncate()
                        .text_size(crate::rems_from_px(13.0))
                        .text_color(theme.foreground)
                        .when(active, crate::StyledExt::font_medium)
                        .child(label.into()),
                )
                .child(
                    div()
                        .max_w(relative(0.3))
                        .min_w_0()
                        .ml(px(12.0))
                        .truncate()
                        .text_size(crate::rems_from_px(11.0))
                        .text_color(theme.muted_foreground)
                        .child(detail.into()),
                )
                .child(
                    div()
                        .ml(px(12.0))
                        .flex_none()
                        .font_family(font_family.clone())
                        .text_size(crate::rems_from_px(10.0))
                        .text_color(theme.muted_foreground)
                        .child(target),
                )
                .when(tagged, |row| {
                    row.child(
                        Tag::secondary()
                            .small()
                            .ml(px(8.0))
                            .text_size(crate::rems_from_px(9.0))
                            .child("Tagged"),
                    )
                })
                .child(
                    div()
                        .w(px(22.0))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .when(active, |slot| {
                            slot.child(
                                Icon::new(IconName::Check)
                                    .size(px(12.0))
                                    .text_color(theme.foreground),
                            )
                        }),
                ),
        )
}

#[allow(clippy::fn_params_excessive_bools)]
pub fn buffer_chooser_row(
    id: &'static str,
    index: usize,
    key: impl Into<SharedString>,
    show_key_gutter: bool,
    name: impl Into<SharedString>,
    preview: impl Into<SharedString>,
    size: impl Into<SharedString>,
    age: impl Into<SharedString>,
    tagged: bool,
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
                        .font_family(font_family.clone())
                        .text_size(crate::rems_from_px(9.0))
                        .text_color(theme.foreground.muted())
                        .child(age.into()),
                )
                .when(tagged, |row| {
                    row.child(
                        Tag::primary()
                            .small()
                            .ml(px(8.0))
                            .font_family(font_family)
                            .text_size(crate::rems_from_px(9.0))
                            .child("TAGGED"),
                    )
                }),
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

fn chooser_shadow(cx: &App) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: cx.theme().border().subtle(),
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
