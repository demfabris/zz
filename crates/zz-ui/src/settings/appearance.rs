use super::StackPosition;
use crate::{ActiveTheme as _, Colorize as _, ThemeColor, ThemeMode};
use gpui::{AnyElement, App, IntoElement, SharedString, Window, div, prelude::*, px};

const THEME_PREVIEW_WIDTH: f32 = 84.0;
const THEME_PREVIEW_HEIGHT: f32 = 56.0;
const THEME_PREVIEW_SIDEBAR_WIDTH: f32 = 20.0;

const PAGE_ROW_GAP: f32 = 8.0;

pub fn run_position<T: Copy>(
    items: &[T],
    index: usize,
    is_entry: impl Fn(T) -> bool,
) -> StackPosition {
    let bounded = |neighbour: Option<usize>| {
        neighbour.is_none_or(|at| items.get(at).copied().is_none_or(|it| !is_entry(it)))
    };
    StackPosition::new(bounded(index.checked_sub(1)), bounded(Some(index + 1)))
}

pub fn page_row(element: AnyElement, ends_run: bool) -> AnyElement {
    div()
        .w_full()
        .pb(px(if ends_run { PAGE_ROW_GAP } else { 0.0 }))
        .child(element)
        .into_any_element()
}

#[derive(Clone, Copy)]
pub enum AppearancePageItem<C> {
    Description,
    Group {
        title: &'static str,
        description: Option<&'static str>,
    },
    ThemeMode,
    UiZoom,
    AppIcon,
    Preset,
    ChromeColor(C),
    StatusShowSession,
    StatusBadges,
    StatusAlignment,
    StatusAgents,
    StatusHost,
    StatusUpdate,
    StatusClock,
    Animations,
    WidgetCornerRadius,
    ShadowStrength,
    WindowBackgroundBlur,
    #[cfg(target_os = "linux")]
    WindowCornerRadius,
    #[cfg(target_os = "linux")]
    UseSystemTitlebar,
}

impl<C: Copy> AppearancePageItem<C> {
    pub const fn is_entry(self) -> bool {
        !matches!(self, Self::Description | Self::Group { .. })
    }
}

pub fn appearance_page_items<C>(
    colors: impl IntoIterator<Item = C>,
    macos: bool,
    has_window_blur: bool,
) -> Vec<AppearancePageItem<C>> {
    let mut items = vec![
        AppearancePageItem::Description,
        AppearancePageItem::Group {
            title: "Theme",
            description: None,
        },
        AppearancePageItem::ThemeMode,
        AppearancePageItem::UiZoom,
    ];
    if macos {
        items.push(AppearancePageItem::AppIcon);
    }
    items.extend([
        AppearancePageItem::Group {
            title: "Chroma Colors",
            description: Some(
                "Recolors the application chrome. Every panel, hover state, muted label and focus \
                 ring is derived from these six, so nothing else needs setting.",
            ),
        },
        AppearancePageItem::Preset,
    ]);
    items.extend(colors.into_iter().map(AppearancePageItem::ChromeColor));
    items.extend([
        AppearancePageItem::Group {
            title: "Status bar",
            description: None,
        },
        AppearancePageItem::StatusShowSession,
        AppearancePageItem::StatusBadges,
        AppearancePageItem::StatusAlignment,
        AppearancePageItem::StatusAgents,
        AppearancePageItem::StatusHost,
        AppearancePageItem::StatusUpdate,
        AppearancePageItem::StatusClock,
    ]);
    items.extend([
        AppearancePageItem::Group {
            title: "Tweaks",
            description: None,
        },
        AppearancePageItem::Animations,
        AppearancePageItem::WidgetCornerRadius,
        AppearancePageItem::ShadowStrength,
    ]);
    if has_window_blur {
        items.push(AppearancePageItem::WindowBackgroundBlur);
    }
    #[cfg(target_os = "linux")]
    items.extend([
        AppearancePageItem::WindowCornerRadius,
        AppearancePageItem::UseSystemTitlebar,
    ]);
    items
}

pub fn appearance_page<C: Copy + 'static>(
    items: Vec<AppearancePageItem<C>>,
    mut render: impl FnMut(AppearancePageItem<C>, StackPosition, &mut Window, &mut App) -> AnyElement
    + 'static,
) -> super::SettingsVirtualColumn {
    super::settings_virtual_column(
        "settings-appearance",
        items.len(),
        move |index, window, cx| {
            let Some(item) = items.get(index).copied() else {
                return div().into_any_element();
            };
            let position = run_position(&items, index, AppearancePageItem::is_entry);
            let row = match item {
                AppearancePageItem::Description => {
                    super::settings_page_description(super::SettingsSection::Appearance, cx)
                        .into_any_element()
                }
                AppearancePageItem::Group { title, description } => {
                    super::settings_list_group_header(title, description, cx).into_any_element()
                }
                _ => render(item, position, window, cx),
            };
            page_row(row, !item.is_entry() || position.ends_run())
        },
    )
}

pub fn picker_tile(
    id: SharedString,
    label: &'static str,
    preview: impl IntoElement,
    selected: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(gpui::ElementId::Name(id))
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .child(
            div()
                .p(px(3.0))
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(if selected {
                    cx.theme().foreground
                } else {
                    gpui::transparent_black()
                })
                .child(preview),
        )
        .child(
            div()
                .text_size(crate::rems_from_px(11.0))
                .text_color(if selected {
                    cx.theme().foreground
                } else {
                    cx.theme().foreground.muted()
                })
                .child(label),
        )
}

pub fn theme_preview(
    mode: Option<ThemeMode>,
    light: &ThemeColor,
    dark: &ThemeColor,
    cx: &App,
) -> gpui::Div {
    match mode {
        Some(ThemeMode::Light) => theme_preview_window(light, cx),
        Some(ThemeMode::Dark) => theme_preview_window(dark, cx),
        None => {
            let split = THEME_PREVIEW_WIDTH / 2.0;
            div()
                .relative()
                .w(px(THEME_PREVIEW_WIDTH))
                .h(px(THEME_PREVIEW_HEIGHT))
                .rounded(cx.theme().radius)
                .bg(theme_preview_split_background(
                    light.background.raised(2),
                    dark.background,
                    0.5,
                ))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(THEME_PREVIEW_SIDEBAR_WIDTH))
                        .h_full()
                        .w(px(split - THEME_PREVIEW_SIDEBAR_WIDTH))
                        .bg(light.background),
                )
                .child(theme_preview_contents(light))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .h_full()
                        .w(px(split))
                        .overflow_hidden()
                        .child(theme_preview_contents(dark).absolute().top_0().right_0()),
                )
        }
    }
}

fn theme_preview_window(colors: &ThemeColor, cx: &App) -> gpui::Div {
    let sidebar_fraction = THEME_PREVIEW_SIDEBAR_WIDTH / THEME_PREVIEW_WIDTH;
    div()
        .w(px(THEME_PREVIEW_WIDTH))
        .h(px(THEME_PREVIEW_HEIGHT))
        .rounded(cx.theme().radius)
        .bg(theme_preview_split_background(
            colors.background.raised(2),
            colors.background,
            sidebar_fraction,
        ))
        .child(theme_preview_contents(colors))
}

fn theme_preview_contents(colors: &ThemeColor) -> gpui::Div {
    let text = colors.foreground.muted();
    let text_bar = move |width: f32| div().w(px(width)).h(px(3.0)).rounded_full().bg(text);
    div()
        .flex()
        .w(px(THEME_PREVIEW_WIDTH))
        .h(px(THEME_PREVIEW_HEIGHT))
        .child(
            div()
                .w(px(THEME_PREVIEW_SIDEBAR_WIDTH))
                .flex_none()
                .border_r_1()
                .border_color(colors.border),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(6.0))
                .p(px(7.0))
                .child(
                    div().flex().items_center().gap(px(3.0)).children(
                        [colors.danger, colors.warning, colors.success]
                            .map(|light| div().size(px(4.0)).rounded_full().bg(light)),
                    ),
                )
                .child(text_bar(36.0))
                .child(text_bar(22.0)),
        )
}

fn theme_preview_split_background(
    left: gpui::Hsla,
    right: gpui::Hsla,
    split: f32,
) -> gpui::Background {
    gpui::linear_gradient(
        90.0,
        gpui::linear_color_stop(left, split),
        gpui::linear_color_stop(right, split),
    )
}
