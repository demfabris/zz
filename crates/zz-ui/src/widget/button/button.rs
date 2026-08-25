//! The [`Button`] element and its variant/state color matrix.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Corners, Div, Edges, ElementId, Hsla, InteractiveElement,
    Interactivity, IntoElement, MouseButton, ParentElement, Pixels, RenderOnce, Role, SharedString,
    Stateful, StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, px, relative, transparent_white,
};

use crate::{
    ActiveTheme as _, Colorize as _, Disableable, Icon, IconName, Selectable, Sizable, Size,
    StyledExt as _, control_shadow, h_flex, rems_from_px, tooltip::Tooltip,
};

use super::button_icon::ButtonIcon;

pub const COMPACT_ICON_BUTTON_SIZE: f32 = 24.0;
const COMPACT_ICON_DROP: Pixels = px(0.5);

/// Corner radius of a [`Button`]: the theme radius, or an explicit override.
#[derive(Default, Clone, Copy)]
pub enum ButtonRounded {
    #[default]
    Medium,
    Size(Pixels),
}

impl From<Pixels> for ButtonRounded {
    fn from(px: Pixels) -> Self {
        ButtonRounded::Size(px)
    }
}

/// A caller-supplied palette for [`ButtonVariant::Custom`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ButtonCustomVariant {
    color: Hsla,
    shadow: bool,
    active: Hsla,
}

/// The variant selectors, as a trait so wrappers around a Button can forward
/// them (see [`ButtonVariant`] for the variants themselves).
pub trait ButtonVariants: Sized {
    fn with_variant(self, variant: ButtonVariant) -> Self;

    fn primary(self) -> Self {
        self.with_variant(ButtonVariant::Primary)
    }

    fn secondary(self) -> Self {
        self.with_variant(ButtonVariant::Secondary)
    }

    fn danger(self) -> Self {
        self.with_variant(ButtonVariant::Danger)
    }

    fn warning(self) -> Self {
        self.with_variant(ButtonVariant::Warning)
    }

    fn success(self) -> Self {
        self.with_variant(ButtonVariant::Success)
    }

    fn ghost(self) -> Self {
        self.with_variant(ButtonVariant::Ghost)
    }

    fn link(self) -> Self {
        self.with_variant(ButtonVariant::Link)
    }

    /// Unpadded, styled as plain text.
    fn text(self) -> Self {
        self.with_variant(ButtonVariant::Text)
    }

    fn custom(self, style: ButtonCustomVariant) -> Self {
        self.with_variant(ButtonVariant::Custom(style))
    }
}

impl ButtonCustomVariant {
    pub fn new(cx: &App) -> Self {
        Self {
            color: cx.theme().transparent,
            active: cx.theme().transparent,
            shadow: false,
        }
    }

    /// Background and text color. Defaults to transparent.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = color;
        self
    }

    pub fn active(mut self, color: Hsla) -> Self {
        self.active = color;
        self
    }

    pub fn shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ButtonVariant {
    #[default]
    Default,
    Primary,
    Secondary,
    Danger,
    Success,
    Warning,
    Ghost,
    Link,
    Text,
    Custom(ButtonCustomVariant),
}

impl ButtonVariant {
    #[inline]
    pub fn is_link(&self) -> bool {
        matches!(self, Self::Link)
    }

    #[inline]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text)
    }

    #[inline]
    fn no_padding(&self) -> bool {
        self.is_link() || self.is_text()
    }

    #[inline]
    fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    base: Stateful<Div>,
    style: StyleRefinement,
    icon: Option<ButtonIcon>,
    label: Option<SharedString>,
    children: Vec<AnyElement>,
    disabled: bool,
    selected: bool,
    variant: ButtonVariant,
    hover_bg: Option<Hsla>,
    rounded: ButtonRounded,
    outline: bool,
    dropdown_caret: bool,
    size: Size,
    compact: bool,
    tooltip: Option<SharedString>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    on_hover: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
    loading: bool,

    tab_index: isize,
    tab_stop: bool,
}

impl From<Button> for AnyElement {
    fn from(button: Button) -> Self {
        button.into_any_element()
    }
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();

        Self {
            id: id.clone(),
            base: div().flex_shrink_0().id(id),
            style: StyleRefinement::default(),
            icon: None,
            label: None,
            disabled: false,
            selected: false,
            variant: ButtonVariant::default(),
            hover_bg: None,
            rounded: ButtonRounded::Medium,
            size: Size::Medium,
            tooltip: None,
            on_click: None,
            on_hover: None,
            loading: false,
            compact: false,
            outline: false,
            children: Vec::new(),
            dropdown_caret: false,
            tab_index: 0,
            tab_stop: true,
        }
    }

    pub fn compact_icon(id: impl Into<ElementId>, icon: impl Into<Icon>) -> Self {
        Self::new(id)
            .ghost()
            .small()
            .compact()
            .size(rems_from_px(COMPACT_ICON_BUTTON_SIZE))
            .icon(icon.into().relative().top(COMPACT_ICON_DROP))
    }

    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }

    /// Replace the variant's hovered fill. Setting `hover` from outside panics,
    /// because the variant matrix already claimed it.
    #[must_use]
    pub fn hover_bg(mut self, bg: Hsla) -> Self {
        self.hover_bg = Some(bg);
        self
    }

    pub fn rounded(mut self, rounded: impl Into<ButtonRounded>) -> Self {
        self.rounded = rounded.into();
        self
    }

    /// Set the label. A button with no label renders as an icon button.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the leading icon.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(ButtonIcon::new(icon));
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Called with `true` when the pointer enters, `false` when it leaves.
    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Rc::new(handler));
        self
    }

    /// Defaults to 0.
    pub fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = tab_index;
        self
    }

    /// Defaults to true.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn dropdown_caret(mut self, dropdown_caret: bool) -> Self {
        self.dropdown_caret = dropdown_caret;
        self
    }

    #[inline]
    fn clickable(&self) -> bool {
        !(self.disabled || self.loading) && self.on_click.is_some()
    }

    #[inline]
    fn hoverable(&self) -> bool {
        !(self.disabled || self.loading) && self.on_hover.is_some()
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Selectable for Button {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl Sizable for Button {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl ButtonVariants for Button {
    fn with_variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

#[inline]
fn button_text_size<T: Styled>(this: T, size: Size) -> T {
    match size {
        Size::XSmall => this.text_xs(),
        Size::Small => this.text_sm(),
        _ => this.text_base(),
    }
}

fn focus_ring<T: ParentElement + Styled + Sized>(
    mut this: T,
    is_focused: bool,
    margins: Pixels,
    window: &Window,
    cx: &App,
) -> T {
    if !is_focused {
        return this;
    }

    const RING_BORDER_WIDTH: Pixels = px(1.5);
    let rem_size = window.rem_size();
    let style = this.style();

    let border_widths = Edges::<Pixels> {
        top: style
            .border_widths
            .top
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        bottom: style
            .border_widths
            .bottom
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        left: style
            .border_widths
            .left
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        right: style
            .border_widths
            .right
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
    };

    let radius = Corners::<Pixels> {
        top_left: style
            .corner_radii
            .top_left
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        top_right: style
            .corner_radii
            .top_right
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        bottom_left: style
            .corner_radii
            .bottom_left
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
        bottom_right: style
            .corner_radii
            .bottom_right
            .map(|v| v.to_pixels(rem_size))
            .unwrap_or_default(),
    }
    .map(|v| *v + RING_BORDER_WIDTH);

    let mut inner_style = StyleRefinement::default();
    inner_style.corner_radii.top_left = Some(radius.top_left.into());
    inner_style.corner_radii.top_right = Some(radius.top_right.into());
    inner_style.corner_radii.bottom_left = Some(radius.bottom_left.into());
    inner_style.corner_radii.bottom_right = Some(radius.bottom_right.into());

    let inset = RING_BORDER_WIDTH + margins;

    this.child(
        div()
            .flex_none()
            .absolute()
            .top(-(inset + border_widths.top))
            .left(-(inset + border_widths.left))
            .right(-(inset + border_widths.right))
            .bottom(-(inset + border_widths.bottom))
            .border(RING_BORDER_WIDTH)
            .border_color(cx.theme().foreground.alpha(0.2))
            .refine_style(&inner_style),
    )
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style: ButtonVariant = self.variant;
        let clickable = self.clickable();
        let is_disabled = self.disabled;
        let hoverable = self.hoverable();
        let normal_style = style.normal(self.outline, cx);
        let icon_size = match self.size {
            Size::Size(v) => Size::Size(v * 0.75),
            _ => self.size,
        };

        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);

        let rounding = match self.rounded {
            ButtonRounded::Medium => cx.theme().radius,
            ButtonRounded::Size(px) => px,
        };

        let element = self
            .base
            .role(if self.variant.is_link() {
                Role::Link
            } else {
                Role::Button
            })
            .when_some(self.label.as_ref(), |this, label| {
                this.aria_label(label.clone())
            })
            .aria_selected(self.selected)
            .when(!self.disabled, |this| {
                this.track_focus(
                    &focus_handle
                        .tab_index(self.tab_index)
                        .tab_stop(self.tab_stop),
                )
            })
            .cursor_default()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .cursor_default()
            .when(self.variant.is_link(), |this| this.cursor_pointer())
            .when(cx.theme().shadow && normal_style.shadow, |this| {
                this.shadow(control_shadow(cx))
            })
            .when(!style.no_padding(), |this| {
                let height = self.size.control_h();
                if self.label.is_none() && self.children.is_empty() {
                    this.size(height)
                } else {
                    let this = this.h(height);
                    match self.size {
                        Size::Size(size) => this.px(size * 0.2),
                        Size::XSmall => this.px_1().when(self.compact, |this| this.min_w(height)),
                        Size::Small => this
                            .px_3()
                            .when(self.compact, |this| this.min_w(height).px_1p5()),
                        _ => this
                            .px_4()
                            .when(self.compact, |this| this.min_w(height).px_2()),
                    }
                }
            })
            .rounded(rounding)
            .when(self.variant.is_default() || self.outline, |this| {
                this.border_1()
            })
            .text_color(normal_style.fg)
            .when(self.selected, |this| {
                let selected_style = style.selected(self.outline, cx);
                this.bg(selected_style.bg)
                    .border_color(selected_style.border)
                    .text_color(selected_style.fg)
            })
            .when(!self.disabled && !self.selected, |this| {
                this.border_color(normal_style.border)
                    .bg(normal_style.bg)
                    .when(normal_style.underline, |this| this.text_decoration_1())
                    .hover(|this| {
                        let hover_style = style.hovered(self.outline, cx);
                        this.bg(self.hover_bg.unwrap_or(hover_style.bg))
                            .border_color(hover_style.border)
                            .text_color(hover_style.fg)
                    })
                    .active(|this| {
                        let active_style = style.active(self.outline, cx);
                        this.bg(active_style.bg)
                            .border_color(active_style.border)
                            .text_color(active_style.fg)
                    })
            })
            .when(self.disabled, |this| {
                let disabled_style = style.disabled(self.outline, cx);
                this.bg(disabled_style.bg)
                    .text_color(disabled_style.fg)
                    .border_color(disabled_style.border)
                    .shadow_none()
            })
            .refine_style(&self.style)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if is_disabled {
                    cx.stop_propagation();
                    return;
                }

                window.prevent_default();

                crate::text::suppress_text_selection(cx);
            })
            .when_some(self.on_click, |this, on_click| {
                this.on_click(move |event, window, cx| {
                    if !clickable {
                        cx.stop_propagation();
                        return;
                    }

                    on_click(event, window, cx);
                })
            })
            .when_some(self.on_hover.filter(|_| hoverable), |this, on_hover| {
                this.on_hover(move |hovered, window, cx| {
                    on_hover(hovered, window, cx);
                })
            })
            .child({
                let label = h_flex()
                    .id("label")
                    .size_full()
                    .items_center()
                    .justify_center();

                button_text_size(label, self.size)
                    .map(|this| match self.size {
                        Size::XSmall => this.gap_1(),
                        Size::Small => this.gap_1(),
                        _ => this.gap_2(),
                    })
                    .when_some(self.icon, |this, icon| {
                        this.child(icon.loading(self.loading).with_size(icon_size))
                    })
                    .when_some(self.label, |this, label| {
                        this.child(div().flex_none().line_height(relative(1.)).child(label))
                    })
                    .children(self.children)
                    .when(self.dropdown_caret, |this| {
                        this.justify_between().child(
                            Icon::new(IconName::ChevronDown).xsmall().text_color(
                                match self.disabled {
                                    true => normal_style.fg.opacity(0.3),
                                    false => normal_style.fg.opacity(0.5),
                                },
                            ),
                        )
                    })
            })
            .when(self.loading && !self.disabled, |this| {
                this.bg(normal_style.bg.opacity(0.8))
                    .border_color(normal_style.border.opacity(0.8))
                    .text_color(normal_style.fg.opacity(0.8))
            })
            .when_some(self.tooltip, |this, tooltip| {
                this.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            });

        focus_ring(element, is_focused, px(0.), window, cx)
    }
}

struct ButtonVariantStyle {
    bg: Hsla,
    border: Hsla,
    fg: Hsla,
    underline: bool,
    shadow: bool,
}

#[derive(Clone, Copy)]
enum ButtonStyleState {
    Normal,
    Hovered,
    Active,
}

impl ButtonVariant {
    fn outline_background(&self, state: ButtonStyleState, cx: &mut App) -> Hsla {
        match (self, state) {
            (Self::Default, ButtonStyleState::Normal) => cx.theme().background.raised(1).into(),
            (Self::Default, ButtonStyleState::Hovered) => cx
                .theme()
                .border
                .mix_oklab(cx.theme().transparent, 0.5)
                .into(),
            (Self::Default, ButtonStyleState::Active) => cx
                .theme()
                .border
                .mix_oklab(cx.theme().transparent, 0.7)
                .into(),
            (Self::Primary, ButtonStyleState::Normal) => cx.theme().foreground.opacity(0.1),
            (Self::Primary, ButtonStyleState::Hovered) => {
                cx.theme().foreground.hover().opacity(0.2)
            }
            (Self::Primary, ButtonStyleState::Active) => {
                cx.theme().foreground.active().opacity(0.4)
            }
            (Self::Secondary, ButtonStyleState::Normal) => {
                cx.theme().background.raised(2).opacity(0.1)
            }
            (Self::Secondary, ButtonStyleState::Hovered) => {
                cx.theme().background.raised(2).hover().opacity(0.2)
            }
            (Self::Secondary, ButtonStyleState::Active) => {
                cx.theme().background.raised(2).active().opacity(0.4)
            }
            (Self::Danger, ButtonStyleState::Normal) => cx.theme().danger.opacity(0.1),
            (Self::Danger, ButtonStyleState::Hovered) => cx.theme().danger.hover().opacity(0.2),
            (Self::Danger, ButtonStyleState::Active) => cx.theme().danger.active().opacity(0.4),
            (Self::Warning, ButtonStyleState::Normal) => cx.theme().warning.opacity(0.1),
            (Self::Warning, ButtonStyleState::Hovered) => cx.theme().warning.hover().opacity(0.2),
            (Self::Warning, ButtonStyleState::Active) => cx.theme().warning.active().opacity(0.4),
            (Self::Success, ButtonStyleState::Normal) => cx.theme().success.opacity(0.1),
            (Self::Success, ButtonStyleState::Hovered) => cx.theme().success.hover().opacity(0.2),
            (Self::Success, ButtonStyleState::Active) => cx.theme().success.active().opacity(0.4),
            (Self::Ghost | Self::Link | Self::Text, _) => cx.theme().transparent.into(),
            (Self::Custom(colors), _) => colors.color.mix_oklab(cx.theme().transparent, 0.2).into(),
        }
    }

    fn bg_color(&self, outline: bool, cx: &mut App) -> Hsla {
        if outline {
            return self.outline_background(ButtonStyleState::Normal, cx);
        }

        match self {
            Self::Default => cx.theme().background.raised(1).into(),
            Self::Primary => cx.theme().foreground.into(),
            Self::Secondary => cx.theme().background.raised(2).into(),
            Self::Danger => cx.theme().danger.fill().into(),
            Self::Warning => cx.theme().warning.fill().into(),
            Self::Success => cx.theme().success.fill().into(),
            Self::Ghost | Self::Link | Self::Text => cx.theme().transparent.into(),
            Self::Custom(colors) => colors.color.mix_oklab(cx.theme().transparent, 0.2).into(),
        }
    }

    fn text_color(&self, outline: bool, cx: &mut App) -> Hsla {
        match self {
            Self::Default => cx.theme().foreground,
            Self::Primary => {
                if outline {
                    cx.theme().foreground
                } else {
                    cx.theme().foreground.on()
                }
            }
            Self::Secondary => {
                if outline {
                    cx.theme().foreground
                } else {
                    cx.theme().foreground
                }
            }
            Self::Ghost => cx.theme().foreground,
            Self::Danger => {
                if outline {
                    cx.theme().danger
                } else {
                    cx.theme().danger
                }
            }
            Self::Warning => {
                if outline {
                    cx.theme().warning
                } else {
                    cx.theme().warning
                }
            }
            Self::Success => {
                if outline {
                    cx.theme().success
                } else {
                    cx.theme().success
                }
            }
            Self::Link => cx.theme().foreground,
            Self::Text => cx.theme().foreground,
            Self::Custom(colors) => colors.color,
        }
    }

    fn border_color(&self, outline: bool, cx: &mut App) -> Hsla {
        match self {
            Self::Default => cx.theme().border,
            Self::Secondary => cx.theme().border,
            Self::Primary => cx.theme().foreground,
            Self::Danger => {
                if outline {
                    cx.theme().danger.mix_oklab(transparent_white(), 0.4)
                } else {
                    cx.theme().danger.fill()
                }
            }
            Self::Warning => {
                if outline {
                    cx.theme().warning.mix_oklab(transparent_white(), 0.4)
                } else {
                    cx.theme().warning.fill()
                }
            }
            Self::Success => {
                if outline {
                    cx.theme().success.mix_oklab(transparent_white(), 0.4)
                } else {
                    cx.theme().success.fill()
                }
            }
            Self::Ghost | Self::Link | Self::Text => cx.theme().transparent,
            Self::Custom(colors) => {
                if outline {
                    colors.color.mix_oklab(transparent_white(), 0.4)
                } else {
                    colors.color
                }
            }
        }
    }

    fn underline(&self, _: &App) -> bool {
        match self {
            Self::Link => true,
            _ => false,
        }
    }

    fn shadow(&self, outline: bool, _: &App) -> bool {
        match self {
            Self::Default => true,
            Self::Primary | Self::Secondary | Self::Danger => outline,
            Self::Custom(c) => c.shadow,
            _ => false,
        }
    }

    fn normal(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        let bg = self.bg_color(outline, cx);
        let border = self.border_color(outline, cx);
        let fg = self.text_color(outline, cx);
        let underline = self.underline(cx);
        let shadow = self.shadow(outline, cx);

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }

    fn hovered(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        let bg: Hsla = match self {
            Self::Default => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().background.raised(1).hover().into()
                }
            }
            Self::Primary => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().foreground.hover().into()
                }
            }
            Self::Secondary => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().background.raised(2).hover().into()
                }
            }
            Self::Danger => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().danger.fill().hover().into()
                }
            }
            Self::Warning => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().warning.fill().hover().into()
                }
            }
            Self::Success => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().success.fill().hover().into()
                }
            }
            Self::Custom(colors) => if outline {
                colors.color.mix_oklab(cx.theme().transparent, 0.2)
            } else {
                colors.color.mix_oklab(cx.theme().transparent, 0.3)
            }
            .into(),
            Self::Ghost => if cx.theme().mode.is_dark() {
                cx.theme().background.raised(2).lighten(0.1).opacity(0.8)
            } else {
                cx.theme().background.raised(2).darken(0.1).opacity(0.8)
            }
            .into(),
            Self::Link => cx.theme().transparent.into(),
            Self::Text => cx.theme().transparent.into(),
        };

        let border = self.border_color(outline, cx);
        let fg = match self {
            Self::Link => cx.theme().foreground,
            _ => self.text_color(outline, cx),
        };

        let underline = self.underline(cx);
        let shadow = self.shadow(outline, cx);

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }

    fn active(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        let bg = match self {
            Self::Default => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().background.raised(1).active().into()
                }
            }
            Self::Primary => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().foreground.active().into()
                }
            }
            Self::Secondary => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().background.raised(2).active().into()
                }
            }
            Self::Ghost => if cx.theme().mode.is_dark() {
                cx.theme().background.raised(2).lighten(0.2).opacity(0.8)
            } else {
                cx.theme().background.raised(2).darken(0.2).opacity(0.8)
            }
            .into(),
            Self::Danger => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().danger.fill().active().into()
                }
            }
            Self::Warning => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().warning.fill().active().into()
                }
            }
            Self::Success => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().success.fill().active().into()
                }
            }
            Self::Custom(colors) => colors.color.mix_oklab(cx.theme().transparent, 0.4).into(),
            Self::Link => cx.theme().transparent.into(),
            Self::Text => cx.theme().transparent.into(),
        };
        let border = self.border_color(outline, cx);
        let fg = match self {
            Self::Link => cx.theme().foreground,
            Self::Text => cx.theme().foreground.opacity(0.7),
            _ => self.text_color(outline, cx),
        };
        let underline = self.underline(cx);
        let shadow = self.shadow(outline, cx);

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }

    fn selected(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        if outline {
            let active_style = self.active(outline, cx);

            return ButtonVariantStyle {
                fg: self.text_color(outline, cx),
                ..active_style
            };
        }

        let bg = match self {
            Self::Default => cx.theme().background.raised(1).active().into(),
            Self::Primary => cx.theme().foreground.active().into(),
            Self::Secondary => cx.theme().background.raised(2).active().into(),
            Self::Ghost => if cx.theme().mode.is_dark() {
                cx.theme().background.raised(2).lighten(0.2).opacity(0.8)
            } else {
                cx.theme().background.raised(2).darken(0.2).opacity(0.8)
            }
            .into(),
            Self::Danger => cx.theme().danger.fill().active().into(),
            Self::Warning => cx.theme().warning.fill().active().into(),
            Self::Success => cx.theme().success.fill().active().into(),
            Self::Link => cx.theme().transparent.into(),
            Self::Text => cx.theme().transparent.into(),
            Self::Custom(colors) => colors.active.into(),
        };

        let border = self.border_color(outline, cx);
        let fg = match self {
            Self::Link => cx.theme().foreground,
            Self::Text => cx.theme().foreground.opacity(0.7),
            _ => self.text_color(false, cx),
        };
        let underline = self.underline(cx);
        let shadow = self.shadow(outline, cx);

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }

    fn disabled(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        let bg = match self {
            Self::Default | Self::Link | Self::Ghost | Self::Text => cx.theme().transparent.into(),
            Self::Primary => cx.theme().foreground.opacity(0.15),
            Self::Danger => cx.theme().danger.fill().opacity(0.15),
            Self::Warning => cx.theme().warning.fill().opacity(0.15),
            Self::Success => cx.theme().success.fill().opacity(0.15),
            Self::Secondary => cx.theme().background.raised(2).opacity(1.5),
            Self::Custom(style) => style.color.opacity(0.15).into(),
        };
        let fg = cx.theme().foreground.muted().opacity(0.5);
        let (bg, border) = if outline {
            (
                self.outline_background(ButtonStyleState::Normal, cx)
                    .opacity(0.5),
                self.border_color(true, cx).opacity(0.5),
            )
        } else if let Self::Default = self {
            (
                cx.theme().background.raised(1).opacity(0.5).into(),
                cx.theme().border.opacity(0.5),
            )
        } else {
            let border = match self {
                Self::Primary => cx.theme().foreground.opacity(0.15),
                Self::Secondary => cx.theme().background.raised(2).opacity(1.5),
                Self::Danger => cx.theme().danger.fill().opacity(0.15),
                Self::Warning => cx.theme().warning.fill().opacity(0.15),
                Self::Success => cx.theme().success.fill().opacity(0.15),
                Self::Custom(style) => style.color.opacity(0.15),
                Self::Default | Self::Link | Self::Ghost | Self::Text => cx.theme().transparent,
            };
            (bg, border)
        };

        let underline = self.underline(cx);
        let shadow = false;

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn test_button_clickable_logic(_cx: &mut gpui::TestAppContext) {
        let clickable = Button::new("test").on_click(|_, _, _| {});
        assert!(clickable.clickable());

        let disabled = Button::new("test").disabled(true).on_click(|_, _, _| {});
        assert!(!disabled.clickable());

        let loading = Button::new("test").loading(true).on_click(|_, _, _| {});
        assert!(!loading.clickable());
    }

    #[gpui::test]
    fn test_button_hoverable_logic(_cx: &mut gpui::TestAppContext) {
        assert!(!Button::new("test").hoverable());

        assert!(Button::new("test").on_hover(|_, _, _| {}).hoverable());

        assert!(
            !Button::new("test")
                .disabled(true)
                .on_hover(|_, _, _| {})
                .hoverable()
        );

        assert!(
            !Button::new("test")
                .loading(true)
                .on_hover(|_, _, _| {})
                .hoverable()
        );
    }

    #[gpui::test]
    fn test_button_variant_methods(_cx: &mut gpui::TestAppContext) {
        assert!(ButtonVariant::Link.is_link());
        assert!(ButtonVariant::Text.is_text());

        assert!(ButtonVariant::Link.no_padding());
        assert!(ButtonVariant::Text.no_padding());
        assert!(!ButtonVariant::Ghost.no_padding());

        assert!(ButtonVariant::Default.is_default());
        assert!(!ButtonVariant::Primary.is_default());
    }

    #[gpui::test]
    fn test_outline_selected_uses_outline_active_style(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_empty_window();
        window.update(|_, cx| {
            let variant = ButtonVariant::Danger;
            let active_style = variant.active(true, cx);
            let selected_style = variant.selected(true, cx);

            assert_eq!(selected_style.bg, active_style.bg);
            assert_eq!(selected_style.border, active_style.border);
            assert_eq!(selected_style.fg, cx.theme().danger);
            assert_ne!(selected_style.bg, cx.theme().danger.active().into());
        });
    }
}
