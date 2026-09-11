use super::{SettingsSelectItem, StackPosition};
use crate::select::{SelectItem as _, SelectState};
use crate::{ActiveTheme as _, Colorize as _, ThemeColor, ThemeMode};
use gpui::{
    AnyElement, App, Entity, IntoElement, ScrollHandle, SharedString, Window, div, prelude::*, px,
};
use std::{rc::Rc, sync::Arc};

const THEME_PREVIEW_WIDTH: f32 = 84.0;
const THEME_PREVIEW_HEIGHT: f32 = 56.0;
const THEME_PREVIEW_SIDEBAR_WIDTH: f32 = 20.0;

const PAGE_ROW_GAP: f32 = 8.0;
const TILE_GAP: f32 = 6.0;
const STRIP_INSET: f32 = super::SETTINGS_STACK_PADDING;
const TILE_FRAME_PADDING: f32 = 3.0;
const TILE_FRAME: f32 = TILE_FRAME_PADDING + 1.0;
const TILE_HALO_GAP: f32 = 1.0;
const TILE_HALO: f32 = TILE_HALO_GAP + 1.0;

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
    UiFontFamily,
    UiZoom,
    AppIcon,
    Preset(ThemeMode),
    ChromeColor(C),
    ChromeContrast,
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
        AppearancePageItem::UiFontFamily,
        AppearancePageItem::UiZoom,
    ];
    if macos {
        items.push(AppearancePageItem::AppIcon);
    }
    items.extend([
        AppearancePageItem::Group {
            title: "Chroma Colors",
            description: Some(
                "Pick a palette for each appearance, or set the base colors yourself. Edges \
                 follow the background and foreground; status colors follow the palette.",
            ),
        },
        AppearancePageItem::Preset(ThemeMode::Light),
        AppearancePageItem::Preset(ThemeMode::Dark),
    ]);
    items.extend(colors.into_iter().map(AppearancePageItem::ChromeColor));
    items.push(AppearancePageItem::ChromeContrast);
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

pub struct AvailableFonts(pub Arc<dyn gpui::PlatformTextSystem>);

impl gpui::Global for AvailableFonts {}

pub fn ui_font_select(
    selected: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<SelectState<Vec<SettingsSelectItem>>> {
    let mut families = cx.try_global::<AvailableFonts>().map_or_else(
        || cx.text_system().all_font_names(),
        |fonts| fonts.0.all_font_names(),
    );
    families.retain(|family| !family.starts_with('.') && !family.trim().is_empty());
    families.sort_by_cached_key(|family| family.to_lowercase());
    families.dedup();
    let items: Vec<_> = std::iter::once(SettingsSelectItem::new("System default", ""))
        .chain(
            families
                .into_iter()
                .map(|family| SettingsSelectItem::new(family.clone(), family)),
        )
        .collect();
    let selected = selected.unwrap_or_default();
    let index = items.iter().position(|item| item.value() == selected);
    cx.new(|cx| SelectState::new(items, index.map(crate::IndexPath::new), window, cx))
}

pub fn picker_tile(
    id: SharedString,
    label: &'static str,
    preview: impl IntoElement,
    selected: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    tile(id, label, preview, selected, false, cx)
}

fn tile(
    id: SharedString,
    label: &'static str,
    preview: impl IntoElement,
    selected: bool,
    focused: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let ring = |on: bool| {
        if on {
            cx.theme().accent
        } else {
            gpui::transparent_black()
        }
    };
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
                .p(px(TILE_HALO_GAP))
                .rounded(cx.theme().radius + px(TILE_FRAME + TILE_HALO))
                .border_1()
                .border_color(ring(focused))
                .child(
                    div()
                        .p(px(TILE_FRAME_PADDING))
                        .rounded(cx.theme().radius + px(TILE_FRAME))
                        .border_1()
                        .border_color(ring(selected))
                        .child(preview),
                ),
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

/// A row of picker tiles that scrolls sideways once it outgrows the page and
/// bleeds to the row's edges. Clicking a tile or pressing the left and right
/// arrows while the strip has focus reports the tile's index to `on_select`.
#[derive(IntoElement)]
pub struct PickerStrip {
    id: &'static str,
    selected: usize,
    tiles: Vec<(&'static str, AnyElement)>,
    on_select: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
}

impl PickerStrip {
    pub fn new(id: &'static str, selected: usize) -> Self {
        Self {
            id,
            selected,
            tiles: Vec::new(),
            on_select: None,
        }
    }

    #[must_use]
    pub fn tile(mut self, label: &'static str, preview: impl IntoElement) -> Self {
        self.tiles.push((label, preview.into_any_element()));
        self
    }

    #[must_use]
    pub fn tiles<E: IntoElement>(
        mut self,
        tiles: impl IntoIterator<Item = (&'static str, E)>,
    ) -> Self {
        for (label, preview) in tiles {
            self = self.tile(label, preview);
        }
        self
    }

    #[must_use]
    pub fn on_select(mut self, on_select: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(on_select));
        self
    }
}

impl RenderOnce for PickerStrip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll = window
            .use_keyed_state((self.id, 0usize), cx, |_, _| ScrollHandle::default())
            .read(cx)
            .clone();
        let focus = window
            .use_keyed_state((self.id, 1usize), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let count = self.tiles.len();
        let selected = self.selected.min(count.saturating_sub(1));
        let on_select = self.on_select;
        let select = Rc::new(move |index: usize, window: &mut Window, cx: &mut App| {
            if let Some(on_select) = &on_select {
                on_select(index, window, cx);
            }
        });
        let id = self.id;
        let edge = || div().flex_none().w(px(STRIP_INSET - TILE_GAP - TILE_HALO));
        let key_scroll = scroll.clone();
        let key_select = Rc::clone(&select);
        div()
            .id(id)
            .track_focus(&focus)
            .track_scroll(&scroll)
            .flex()
            .gap(px(TILE_GAP))
            .mx(px(-STRIP_INSET))
            .overflow_x_scroll()
            .restrict_scroll_to_axis()
            .on_key_down(move |event, window, cx| {
                let next = match event.keystroke.key.as_str() {
                    "left" => selected.checked_sub(1),
                    "right" => (selected + 1 < count).then_some(selected + 1),
                    _ => return,
                };
                cx.stop_propagation();
                if let Some(next) = next {
                    key_scroll.scroll_to_item(next + 1);
                    key_select(next, window, cx);
                }
            })
            .child(edge())
            .children(
                self.tiles
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, preview))| {
                        let select = Rc::clone(&select);
                        tile(
                            format!("{id}-{index}").into(),
                            label,
                            preview,
                            index == selected,
                            focused && index == selected,
                            cx,
                        )
                        .on_click(move |_, window, cx| select(index, window, cx))
                    }),
            )
            .child(edge())
    }
}

pub fn theme_preview(
    mode: Option<ThemeMode>,
    light: &ThemeColor,
    dark: &ThemeColor,
    cx: &App,
) -> gpui::Div {
    match mode {
        Some(ThemeMode::Light) => palette_preview(light, cx),
        Some(ThemeMode::Dark) => palette_preview(dark, cx),
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

/// The window mockup the theme and palette tiles share, painted from `colors`.
pub fn palette_preview(colors: &ThemeColor, cx: &App) -> gpui::Div {
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
                .border_color(colors.border()),
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
                .child(text_bar(22.0))
                .child(
                    div()
                        .w(px(14.0))
                        .h(px(5.0))
                        .rounded_full()
                        .bg(colors.accent),
                ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Context, Modifiers, Render, ScrollDelta, ScrollWheelEvent, TestAppContext,
        VisualTestContext, point,
    };
    use std::cell::Cell;

    type Picked = Rc<Cell<Option<(u8, usize)>>>;

    struct StripsTest {
        picked: Picked,
    }

    fn strip(id: &'static str, which: u8, first: &'static str, picked: Picked) -> PickerStrip {
        PickerStrip::new(id, 1)
            .tiles((0..8).map(move |index| {
                (
                    "tile",
                    div().w(px(84.0)).h(px(56.0)).when(index == 0, |this| {
                        this.debug_selector(move || first.to_string())
                    }),
                )
            }))
            .on_select(move |index, _, _| picked.set(Some((which, index))))
    }

    impl Render for StripsTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(300.0))
                .h(px(400.0))
                .flex()
                .flex_col()
                .gap(px(20.0))
                .px(px(STRIP_INSET))
                .child(strip(
                    "first-strip",
                    0,
                    "first-strip-tile",
                    self.picked.clone(),
                ))
                .child(strip(
                    "second-strip",
                    1,
                    "second-strip-tile",
                    self.picked.clone(),
                ))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn strips_scroll_and_navigate_independently(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let picked: Picked = Rc::default();
        let (_, cx) = cx.add_window_view(|_, _| StripsTest {
            picked: picked.clone(),
        });
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let first_before = cx.debug_bounds("first-strip-tile").unwrap();
        let second_before = cx.debug_bounds("second-strip-tile").unwrap();
        assert_eq!(first_before.origin.x, px(STRIP_INSET + TILE_FRAME));
        cx.simulate_event(ScrollWheelEvent {
            position: first_before.center(),
            delta: ScrollDelta::Pixels(point(px(-120.0), px(0.0))),
            ..Default::default()
        });
        draw(cx);
        assert!(cx.debug_bounds("first-strip-tile").unwrap().origin.x < first_before.origin.x);
        assert_eq!(
            cx.debug_bounds("second-strip-tile").unwrap().origin.x,
            second_before.origin.x
        );
        cx.simulate_event(ScrollWheelEvent {
            position: second_before.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
            ..Default::default()
        });
        draw(cx);
        assert_eq!(
            cx.debug_bounds("second-strip-tile").unwrap().origin.x,
            second_before.origin.x
        );

        cx.simulate_click(second_before.center(), Modifiers::default());
        draw(cx);
        assert_eq!(picked.get(), Some((1, 0)));
        cx.simulate_keystrokes("right");
        assert_eq!(picked.get(), Some((1, 2)));
        cx.simulate_keystrokes("left");
        assert_eq!(picked.get(), Some((1, 0)));
    }
}
