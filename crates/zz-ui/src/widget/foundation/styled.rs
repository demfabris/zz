//! The shared styling vocabulary: one [`Size`] scale, one set of state traits,
//! one set of [`gpui::Styled`] extensions.

use super::ActiveTheme;
use super::Colorize as _;
use gpui::{
    App, BoxShadow, Corners, DefiniteLength, Div, Edges, Pixels, Refineable, StyleRefinement,
    Styled, div, point, px,
};

use super::rems_from_px;

/// A [`Div`] laid out as a horizontal flex row, vertically centered.
#[inline(always)]
pub fn h_flex() -> Div {
    div().h_flex()
}

#[inline(always)]
pub fn v_flex() -> Div {
    div().v_flex()
}

macro_rules! font_weight {
    ($fn:ident, $const:ident) => {
        /// [docs](https://tailwindcss.com/docs/font-weight)
        #[inline]
        fn $fn(self) -> Self {
            self.font_weight(gpui::FontWeight::$const)
        }
    };
}

/// Extends [`gpui::Styled`] with the shorthands this crate leans on.
pub trait StyledExt: Styled + Sized {
    fn refine_style(mut self, style: &StyleRefinement) -> Self {
        self.style().refine(style);
        self
    }

    /// Lay out as a horizontal flex row, vertically centered.
    #[inline(always)]
    fn h_flex(self) -> Self {
        self.flex().flex_row().items_center()
    }

    #[inline(always)]
    fn v_flex(self) -> Self {
        self.flex().flex_col()
    }

    fn paddings<L>(self, paddings: impl Into<Edges<L>>) -> Self
    where
        L: Into<DefiniteLength> + Clone + Default + std::fmt::Debug + PartialEq,
    {
        let paddings = paddings.into();
        self.pt(paddings.top.into())
            .pb(paddings.bottom.into())
            .pl(paddings.left.into())
            .pr(paddings.right.into())
    }

    fn margins<L>(self, margins: impl Into<Edges<L>>) -> Self
    where
        L: Into<DefiniteLength> + Clone + Default + std::fmt::Debug + PartialEq,
    {
        let margins = margins.into();
        self.mt(margins.top.into())
            .mb(margins.bottom.into())
            .ml(margins.left.into())
            .mr(margins.right.into())
    }

    /// A 1px border in the theme's ring color.
    #[inline]
    fn focused_border(self, cx: &App) -> Self {
        self.border_1().border_color(cx.theme().foreground)
    }

    fn control_highlight(self, cx: &App) -> Self {
        let this = self.border_color(cx.theme().foreground.opacity(0.1));
        if cx.theme().shadow {
            this.shadow(control_shadow(cx))
        } else {
            this
        }
    }

    fn control_surface(self, cx: &App) -> Self {
        self.border(px(0.5)).control_highlight(cx)
    }

    font_weight!(font_thin, THIN);
    font_weight!(font_extralight, EXTRA_LIGHT);
    font_weight!(font_light, LIGHT);
    font_weight!(font_normal, NORMAL);
    font_weight!(font_medium, MEDIUM);
    font_weight!(font_semibold, SEMIBOLD);
    font_weight!(font_bold, BOLD);
    font_weight!(font_extrabold, EXTRA_BOLD);
    font_weight!(font_black, BLACK);

    /// The floating-panel look: popover background, border, shadow and radius.
    #[inline]
    fn popover_style(self, cx: &App) -> Self {
        self.bg(cx.theme().background.raised(1).opaque())
            .text_color(cx.theme().foreground)
            .control_surface(cx)
            .rounded(cx.theme().radius)
    }

    fn corner_radii(self, radius: Corners<Pixels>) -> Self {
        self.rounded_tl(radius.top_left)
            .rounded_tr(radius.top_right)
            .rounded_bl(radius.bottom_left)
            .rounded_br(radius.bottom_right)
    }
}

impl<E: Styled> StyledExt for E {}

/// How far the ring reaches past the element's own edge.
pub const SURFACE_RING_OUTSET: Pixels = px(0.5);

const RING_SPREAD: Pixels = SURFACE_RING_OUTSET;

const RING_INK_FLOOR: f32 = 0.12;

fn ring(sink: Pixels, cx: &App) -> BoxShadow {
    let scrim = cx.theme().scrim;
    BoxShadow {
        color: scrim.divide(scrim.a.max(RING_INK_FLOOR)),
        offset: point(px(0.), sink),
        blur_radius: px(0.),
        spread_radius: RING_SPREAD,
        inset: false,
    }
}

pub fn control_shadow(cx: &App) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: cx.theme().scrim.alpha(0.2),
        offset: point(px(0.), px(1.)),
        blur_radius: px(2.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// The control hairline without the soft falloff. Apply it to a box expanded by
/// [`SURFACE_RING_OUTSET`], so the inset shadow occupies only that band and lays
/// no ink beneath translucent content.
pub fn surface_ring(cx: &App) -> Vec<BoxShadow> {
    let mut ring = ring(px(0.), cx);
    ring.inset = true;
    vec![ring]
}

/// The ring alone, for a surface assembled from stacked pieces that each paint
/// their own share of it. Pass `caps_top: false` on a piece that does not own the
/// top edge, so its stroke sinks under its own background instead of the piece
/// above.
pub fn stacked_ring(caps_top: bool, cx: &App) -> Vec<BoxShadow> {
    vec![ring(if caps_top { px(0.) } else { RING_SPREAD }, cx)]
}

/// The size scale shared by every widget in the crate.
#[derive(Clone, Default, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    /// An explicit pixel size, for the few places the scale does not fit.
    Size(Pixels),
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    fn as_f32(&self) -> f32 {
        match self {
            Size::Size(val) => val.as_f32(),
            Size::XSmall => 0.,
            Size::Small => 1.,
            Size::Medium => 2.,
            Size::Large => 3.,
        }
    }

    /// The short name: `xs`, `sm`, `md`, `lg`, or `custom`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Size::XSmall => "xs",
            Size::Small => "sm",
            Size::Medium => "md",
            Size::Large => "lg",
            Size::Size(_) => "custom",
        }
    }

    /// Parse a size name, case-insensitively: `xs`/`xsmall`, `sm`/`small`,
    /// `md`/`medium`, `lg`/`large`. Anything else is [`Size::Medium`].
    pub fn from_str(size: &str) -> Self {
        match size.to_lowercase().as_str() {
            "xs" | "xsmall" => Size::XSmall,
            "sm" | "small" => Size::Small,
            "md" | "medium" => Size::Medium,
            "lg" | "large" => Size::Large,
            _ => Size::Medium,
        }
    }

    pub fn smaller(&self) -> Self {
        match self {
            Size::XSmall => Size::XSmall,
            Size::Small => Size::XSmall,
            Size::Medium => Size::Small,
            Size::Large => Size::Medium,
            Size::Size(val) => Size::Size(*val * 0.2),
        }
    }

    pub fn larger(&self) -> Self {
        match self {
            Size::XSmall => Size::Small,
            Size::Small => Size::Medium,
            Size::Medium => Size::Large,
            Size::Large => Size::Large,
            Size::Size(val) => Size::Size(*val * 1.2),
        }
    }

    /// The *smaller* of two sizes, despite the name: the ordering is inverted
    /// (`Size::XSmall.max(Size::Small) == Size::XSmall`).
    pub fn max(&self, other: Self) -> Self {
        match (self, other) {
            (Size::Size(a), Size::Size(b)) => Size::Size(px(a.as_f32().min(b.as_f32()))),
            (Size::Size(a), _) => Size::Size(*a),
            (_, Size::Size(b)) => Size::Size(b),
            (a, b) if a.as_f32() < b.as_f32() => *a,
            _ => other,
        }
    }

    /// The *larger* of two sizes; see [`Size::max`] on the inverted naming.
    pub fn min(&self, other: Self) -> Self {
        match (self, other) {
            (Size::Size(a), Size::Size(b)) => Size::Size(px(a.as_f32().max(b.as_f32()))),
            (Size::Size(a), _) => Size::Size(*a),
            (_, Size::Size(b)) => Size::Size(b),
            (a, b) if a.as_f32() > b.as_f32() => *a,
            _ => other,
        }
    }

    /// Horizontal padding for input-shaped controls.
    pub fn input_px(&self) -> DefiniteLength {
        match self {
            Self::Large => rems_from_px(16.).into(),
            Self::Medium => rems_from_px(12.).into(),
            Self::Small => rems_from_px(8.).into(),
            Self::XSmall => rems_from_px(4.).into(),
            Self::Size(_) => px(8.).into(),
        }
    }

    /// Vertical padding for input-shaped controls.
    pub fn input_py(&self) -> DefiniteLength {
        match self {
            Size::Large => rems_from_px(10.).into(),
            Size::Medium => rems_from_px(8.).into(),
            Size::Small => rems_from_px(2.).into(),
            Size::XSmall => rems_from_px(0.).into(),
            Size::Size(_) => px(2.).into(),
        }
    }

    /// Height of every control that sits on a form row: text fields, selects,
    /// and the square of an icon-only button. `Size::Size` stays the caller's
    /// literal pixels.
    pub fn control_h(&self) -> DefiniteLength {
        match self {
            Size::Size(value) => (*value).into(),
            Size::XSmall => rems_from_px(24.).into(),
            Size::Small => rems_from_px(28.).into(),
            Size::Medium => rems_from_px(36.).into(),
            Size::Large => rems_from_px(40.).into(),
        }
    }
}

impl From<Pixels> for Size {
    fn from(size: Pixels) -> Self {
        Size::Size(size)
    }
}

#[allow(patterns_in_fns_without_body)]
pub trait Selectable: Sized {
    fn selected(mut self, selected: bool) -> Self;

    fn is_selected(&self) -> bool;

    /// Mark the element as right-click-selected. Does nothing by default.
    fn secondary_selected(self, _: bool) -> Self {
        self
    }
}

#[allow(patterns_in_fns_without_body)]
pub trait Disableable {
    fn disabled(mut self, disabled: bool) -> Self;
}

/// An element that takes a [`Size`]. [`Size::Medium`] is the default.
#[allow(patterns_in_fns_without_body)]
pub trait Sizable: Sized {
    /// Accepts a [`Pixels`] for a custom size: `.with_size(px(30.))`.
    fn with_size(mut self, size: impl Into<Size>) -> Self;

    #[inline(always)]
    fn xsmall(self) -> Self {
        self.with_size(Size::XSmall)
    }

    #[inline(always)]
    fn small(self) -> Self {
        self.with_size(Size::Small)
    }

    #[inline(always)]
    fn large(self) -> Self {
        self.with_size(Size::Large)
    }
}

/// Apply a [`Size`] to an element's own metrics, rather than to a child widget.
pub trait StyleSized<T: Styled> {
    fn input_text_size(self, size: Size) -> Self;
    fn input_size(self, size: Size) -> Self;
    fn input_px(self, size: Size) -> Self;
    fn input_py(self, size: Size) -> Self;
    fn input_h(self, size: Size) -> Self;
    fn list_size(self, size: Size) -> Self;
    fn list_px(self, size: Size) -> Self;
    fn list_py(self, size: Size) -> Self;
}

impl<T: Styled> StyleSized<T> for T {
    #[inline]
    fn input_text_size(self, size: Size) -> Self {
        match size {
            Size::XSmall => self.text_xs(),
            Size::Small => self.text_sm(),
            Size::Medium => self.text_sm(),
            Size::Large => self.text_base(),
            Size::Size(size) => self.text_size(size * 0.875),
        }
    }

    #[inline]
    fn input_size(self, size: Size) -> Self {
        self.input_px(size).input_py(size).input_h(size)
    }

    #[inline]
    fn input_px(self, size: Size) -> Self {
        self.px(size.input_px())
    }

    #[inline]
    fn input_py(self, size: Size) -> Self {
        self.py(size.input_py())
    }

    #[inline]
    fn input_h(self, size: Size) -> Self {
        self.h(size.control_h())
    }

    #[inline]
    fn list_size(self, size: Size) -> Self {
        self.list_px(size).list_py(size).input_text_size(size)
    }

    #[inline]
    fn list_px(self, size: Size) -> Self {
        match size {
            Size::Small => self.px_2(),
            _ => self.px_3(),
        }
    }

    #[inline]
    fn list_py(self, size: Size) -> Self {
        match size {
            Size::Large => self.py_2(),
            Size::Medium => self.py_1(),
            Size::Small => self.py_0p5(),
            _ => self.py_1(),
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::Size;

    #[test]
    fn size_max_min_are_inverted_upstream_and_stay_that_way() {
        assert_eq!(Size::Small.min(Size::XSmall), Size::Small);
        assert_eq!(Size::XSmall.min(Size::Small), Size::Small);
        assert_eq!(Size::Small.min(Size::Medium), Size::Medium);
        assert_eq!(Size::Medium.min(Size::Large), Size::Large);
        assert_eq!(Size::Large.min(Size::Small), Size::Large);
        assert_eq!(
            Size::Size(px(10.)).min(Size::Size(px(20.))),
            Size::Size(px(20.))
        );

        assert_eq!(Size::Small.max(Size::XSmall), Size::XSmall);
        assert_eq!(Size::XSmall.max(Size::Small), Size::XSmall);
        assert_eq!(Size::Small.max(Size::Medium), Size::Small);
        assert_eq!(Size::Medium.max(Size::Large), Size::Medium);
        assert_eq!(Size::Large.max(Size::Small), Size::Small);
        assert_eq!(
            Size::Size(px(10.)).max(Size::Size(px(20.))),
            Size::Size(px(10.))
        );
    }

    #[test]
    fn size_names_round_trip() {
        assert_eq!(Size::XSmall.as_str(), "xs");
        assert_eq!(Size::Small.as_str(), "sm");
        assert_eq!(Size::Medium.as_str(), "md");
        assert_eq!(Size::Large.as_str(), "lg");
        assert_eq!(Size::Size(px(15.)).as_str(), "custom");

        assert_eq!(Size::from_str("xs"), Size::XSmall);
        assert_eq!(Size::from_str("SMALL"), Size::Small);
        assert_eq!(Size::from_str("Md"), Size::Medium);
        assert_eq!(Size::from_str("large"), Size::Large);
        assert_eq!(Size::from_str("unknown"), Size::Medium);
    }

    #[test]
    fn size_steps_saturate_at_the_ends() {
        assert_eq!(Size::XSmall.smaller(), Size::XSmall);
        assert_eq!(Size::Large.larger(), Size::Large);
        assert_eq!(Size::Medium.smaller(), Size::Small);
        assert_eq!(Size::Medium.larger(), Size::Large);
    }
}
