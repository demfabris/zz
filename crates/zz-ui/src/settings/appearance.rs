use super::{SettingsSelectItem, StackPosition};
use crate::scroll::{GUTTER_WIDTH, Scrollbar, ScrollbarShow};
use crate::select::{SelectItem as _, SelectState};
use crate::{ActiveTheme as _, Colorize as _, ThemeColor, ThemeMode};
use std::{rc::Rc, sync::Arc};
use zz_gpui::{
    AnyElement, App, Entity, IntoElement, ScrollHandle, SharedString, Window, div, prelude::*, px,
};

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
    InterfaceStyle,
    Advanced {
        expanded: bool,
    },
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
        !matches!(
            self,
            Self::Description | Self::Group { .. } | Self::Advanced { .. }
        )
    }
}

/// The Interface page: theme, style and palettes up front, every other knob
/// behind the Advanced heading, listed only while `advanced` is expanded.
pub fn appearance_page_items<C>(
    colors: impl IntoIterator<Item = C>,
    macos: bool,
    has_window_blur: bool,
    advanced: bool,
) -> Vec<AppearancePageItem<C>> {
    let mut items = vec![
        AppearancePageItem::Description,
        AppearancePageItem::Group {
            title: "Theme",
            description: None,
        },
        AppearancePageItem::ThemeMode,
        AppearancePageItem::InterfaceStyle,
        AppearancePageItem::Group {
            title: "Chroma Colors",
            description: Some(
                "Pick a palette for each appearance, or set the base colors under Advanced.",
            ),
        },
        AppearancePageItem::Preset(ThemeMode::Light),
        AppearancePageItem::Preset(ThemeMode::Dark),
        AppearancePageItem::Advanced { expanded: advanced },
    ];
    if !advanced {
        return items;
    }
    items.extend([AppearancePageItem::UiFontFamily, AppearancePageItem::UiZoom]);
    if macos {
        items.push(AppearancePageItem::AppIcon);
    }
    items.extend(colors.into_iter().map(AppearancePageItem::ChromeColor));
    items.extend([
        AppearancePageItem::ChromeContrast,
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

pub struct AvailableFonts(pub Arc<dyn zz_gpui::PlatformTextSystem>);

impl zz_gpui::Global for AvailableFonts {}

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
                .map(|family| SettingsSelectItem::new(family.clone(), family).preview_font()),
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
) -> zz_gpui::Stateful<zz_gpui::Div> {
    tile(id, label, preview, selected, false, cx)
}

fn tile(
    id: SharedString,
    label: &'static str,
    preview: impl IntoElement,
    selected: bool,
    focused: bool,
    cx: &App,
) -> zz_gpui::Stateful<zz_gpui::Div> {
    let ring = |on: bool| {
        if on {
            cx.theme().accent
        } else {
            zz_gpui::transparent_black()
        }
    };
    div()
        .id(zz_gpui::ElementId::Name(id))
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .child(
            div()
                .p(px(TILE_HALO_GAP))
                .rounded(cx.theme().outer_radius(px(TILE_FRAME + TILE_HALO)))
                .border_1()
                .border_color(ring(focused))
                .child(
                    div()
                        .p(px(TILE_FRAME_PADDING))
                        .rounded(cx.theme().outer_radius(px(TILE_FRAME)))
                        .border_1()
                        .border_color(ring(selected))
                        .child(preview),
                ),
        )
        .child(
            div()
                .w_0()
                .min_w_full()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_center()
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
        let tiles = div()
            .id(id)
            .track_focus(&focus)
            .track_scroll(&scroll)
            .flex()
            .gap(px(TILE_GAP))
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
            .child(edge());

        let coarse = crate::touch::CoarsePointer::get(cx);
        div()
            .flex()
            .flex_col()
            .mx(px(-STRIP_INSET))
            .when(!coarse, |strip| strip.mb(px(-8.0)))
            .child(
                div()
                    .relative()
                    .child(tiles)
                    .child(crate::compact::yield_back_swipe((id, 3usize), &scroll)),
            )
            .when(!coarse, |strip| {
                strip.child(
                    div()
                        .relative()
                        .flex_none()
                        .h(GUTTER_WIDTH)
                        .mx(px(STRIP_INSET))
                        .child(
                            Scrollbar::horizontal(&scroll)
                                .id((id, 2usize))
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                )
            })
    }
}

pub fn theme_preview(
    mode: Option<ThemeMode>,
    light: &ThemeColor,
    dark: &ThemeColor,
    cx: &App,
) -> zz_gpui::Div {
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

/// A small menu drawn the way `style` draws one, for the style tiles.
pub fn interface_style_preview(
    style: crate::interface_style::InterfaceStyle,
    cx: &App,
) -> zz_gpui::Div {
    use crate::interface_style::InterfaceStyle;
    let theme = cx.theme();
    let (surface_radius, row_radius, inset) = match style {
        InterfaceStyle::Flat => (0.0, 0.0, 0.0),
        InterfaceStyle::Modern => (8.0, 4.0, 3.0),
        InterfaceStyle::Full => (10.0, 5.0, 3.0),
    };
    let flat = style == InterfaceStyle::Flat;
    let text = theme.foreground.muted();
    let row = |width: f32, selected: bool| {
        div()
            .h(px(10.0))
            .flex()
            .items_center()
            .px(px(5.0))
            .rounded(px(row_radius))
            .when(selected, |row| row.bg(theme.selection_background()))
            .child(div().w(px(width)).h(px(3.0)).rounded_full().bg(text))
    };
    let surface = theme.background.raised(2).opaque();
    let menu = div()
        .relative()
        .w(px(52.0))
        .flex()
        .flex_col()
        .py(px(3.0))
        .px(px(inset))
        .when(!flat, |menu| menu.gap(px(1.0)))
        .rounded(px(surface_radius))
        .bg(if style == InterfaceStyle::Full {
            surface.opacity(0.6)
        } else {
            surface
        })
        .when(!flat, |menu| {
            menu.border(px(0.5))
                .border_color(theme.foreground.opacity(0.15))
                .shadow_md()
        })
        .children([row(30.0, false), row(22.0, true), row(26.0, false)]);
    div()
        .relative()
        .w(px(THEME_PREVIEW_WIDTH))
        .h(px(THEME_PREVIEW_HEIGHT))
        .rounded(theme.radius)
        .overflow_hidden()
        .bg(theme.background)
        .flex()
        .items_center()
        .justify_center()
        .when(style == InterfaceStyle::Full, |preview| {
            preview.child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .left(px(46.0))
                    .size(px(22.0))
                    .rounded_full()
                    .bg(theme.accent),
            )
        })
        .child(menu)
}

/// The window mockup the theme and palette tiles share, painted from `colors`.
pub fn palette_preview(colors: &ThemeColor, cx: &App) -> zz_gpui::Div {
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

fn theme_preview_contents(colors: &ThemeColor) -> zz_gpui::Div {
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
    left: zz_gpui::Hsla,
    right: zz_gpui::Hsla,
    split: f32,
) -> zz_gpui::Background {
    zz_gpui::linear_gradient(
        90.0,
        zz_gpui::linear_color_stop(left, split),
        zz_gpui::linear_color_stop(right, split),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use zz_gpui::{
        Context, Modifiers, Render, ScrollDelta, ScrollWheelEvent, TestAppContext,
        VisualTestContext, point,
    };

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
                .child(
                    div()
                        .debug_selector(|| "first-strip-container".to_string())
                        .child(strip(
                            "first-strip",
                            0,
                            "first-strip-tile",
                            self.picked.clone(),
                        )),
                )
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

    #[zz_gpui::test]
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

        let first_scrolled = cx.debug_bounds("first-strip-tile").unwrap();
        let first_strip = cx.debug_bounds("first-strip-container").unwrap();
        cx.simulate_click(
            point(first_strip.left() + px(4.0), first_strip.bottom() - px(1.0)),
            Modifiers::default(),
        );
        draw(cx);
        assert!(cx.debug_bounds("first-strip-tile").unwrap().left() > first_scrolled.left());
        assert_eq!(picked.get(), Some((1, 0)));
    }
}
