pub mod floating;
mod palette;
mod palette_model;
mod palette_view;

pub use palette::{
    PalettePill, PaletteRow, PaletteStatus, command_palette_empty, command_palette_entry,
    command_palette_section, command_palette_tree_entry, unified_command_palette_input,
};
pub use palette_model::{
    PaletteHostId, PaletteMode, PaletteSettings, PaletteTarget, PaletteTree, PaletteTreeHost,
    PaletteTreePane, PaletteTreeSession, PaletteTreeWindow,
};
pub use palette_view::{CommandPaletteEvent, CommandPaletteView, PaletteBackend};

use crate::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, Sizable as _, StyledExt as _,
    input::{Input, InputState},
    kbd::Kbd,
    list::ListItem,
    tag::Tag,
};
use gpui::{
    AnyElement, App, ElementId, Entity, IntoElement, Keystroke, ParentElement as _, Pixels,
    RenderOnce, SharedString, Styled as _, div, prelude::*, px,
};

pub const COMMAND_PALETTE_MAX_WIDTH: f32 = 560.0;
pub const COMMAND_PALETTE_INSET: f32 = 8.0;
pub const COMMAND_PALETTE_ROW_HEIGHT: f32 = 28.0;

/// The palette surface's radius: the theme's, opened by
/// [`COMMAND_PALETTE_INSET`] to stay concentric with the children's corners.
pub fn command_palette_radius(cx: &App) -> Pixels {
    cx.theme().radius + px(COMMAND_PALETTE_INSET)
}

#[derive(Clone, Copy)]
pub struct PaletteHint {
    pub key: &'static str,
    pub label: &'static str,
}

pub fn command_palette_input(
    input: &Entity<InputState>,
    prompt: impl Into<SharedString>,
    font_family: impl Into<SharedString>,
    cx: &App,
) -> Input {
    let font_family = font_family.into();
    Input::new(input)
        .w_full()
        .appearance(false)
        .h(px(40.0))
        .px(px(14.0))
        .py(px(8.0))
        .gap(px(8.0))
        .font_family(font_family.clone())
        .text_size(crate::rems_from_px(12.0))
        .line_height(px(16.0))
        .prefix(
            div()
                .font_family(font_family)
                .text_size(crate::rems_from_px(12.0))
                .text_color(cx.theme().foreground)
                .child(prompt.into()),
        )
}

/// A completion row. The caller attaches event handlers to the returned
/// `ListItem`.
pub fn command_palette_row(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    badge: Option<AnyElement>,
    selected: bool,
    cx: &App,
) -> ListItem {
    palette::palette_entry(
        id,
        &PaletteRow {
            label: label.into(),
            detail: detail.into(),
            ..Default::default()
        },
        selected,
        badge,
        None,
        cx,
    )
}

pub fn command_kind_badge(
    label: impl Into<SharedString>,
    font_family: impl Into<SharedString>,
) -> Tag {
    Tag::secondary()
        .small()
        .font_family(font_family.into())
        .text_size(crate::rems_from_px(9.0))
        .child(label.into())
}

/// The palette surface. Input and virtualized rows are caller-supplied slots.
#[derive(IntoElement)]
pub struct CommandPaletteSurface {
    input: AnyElement,
    usage: Option<SharedString>,
    rows: Option<AnyElement>,
    hints: Vec<PaletteHint>,
    revision: u64,
}

impl CommandPaletteSurface {
    pub fn new(input: impl IntoElement, revision: u64) -> Self {
        Self {
            input: input.into_any_element(),
            usage: None,
            rows: None,
            hints: Vec::new(),
            revision,
        }
    }

    #[must_use]
    pub fn rows(mut self, rows: impl IntoElement) -> Self {
        self.rows = Some(rows.into_any_element());
        self
    }

    #[must_use]
    pub fn hints(mut self, hints: impl IntoIterator<Item = PaletteHint>) -> Self {
        self.hints = hints.into_iter().collect();
        self
    }

    #[must_use]
    pub fn usage(mut self, usage: impl Into<SharedString>) -> Self {
        let usage = usage.into();
        self.usage = (!usage.is_empty()).then_some(usage);
        self
    }
}

impl RenderOnce for CommandPaletteSurface {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        let surface = div()
            .id("command-palette-surface")
            .relative()
            .w_full()
            .max_w(px(COMMAND_PALETTE_MAX_WIDTH))
            .flex()
            .flex_col()
            .overflow_hidden()
            .popover_style(cx)
            .rounded(command_palette_radius(cx))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.input)
            .children(self.usage.map(|usage| {
                div()
                    .h(px(24.0))
                    .min_w_0()
                    .flex_none()
                    .px(px(18.0))
                    .pb(px(8.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(crate::rems_from_px(10.0))
                    .line_height(px(16.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(usage)
            }))
            .children(self.rows.map(|rows| {
                div()
                    .px(px(COMMAND_PALETTE_INSET))
                    .pt(px(2.0))
                    .pb(px(COMMAND_PALETTE_INSET))
                    .child(rows)
            }))
            .child(
                div()
                    .min_h(px(32.0))
                    .flex_none()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap(px(14.0))
                    .px(px(18.0))
                    .py(px(8.0))
                    .border_t(px(0.5))
                    .border_color(cx.theme().foreground.opacity(0.1))
                    .text_size(crate::rems_from_px(10.0))
                    .line_height(px(16.0))
                    .text_color(cx.theme().foreground.muted())
                    .children(self.hints.into_iter().map(|hint| palette_hint(hint, cx))),
            );
        crate::widget::foundation::surface_enter(
            surface,
            ElementId::NamedInteger("command-palette-open".into(), self.revision),
            px(0.0),
        )
    }
}

fn palette_hint(hint: PaletteHint, cx: &App) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(5.0))
        .whitespace_nowrap()
        .child(
            div()
                .text_color(cx.theme().foreground)
                .child(match hint.key {
                    "up down" | "up/down" => "↑↓".to_owned(),
                    "left right" | "left/right" => "←→".to_owned(),
                    "escape" => "esc".to_owned(),
                    "enter" => "↵".to_owned(),
                    key => Keystroke::parse(key)
                        .map_or_else(|_| key.to_owned(), |key| Kbd::format(&key)),
                }),
        )
        .child(hint.label)
}

/// Keyboard shortcut hint styling, shared with palette-adjacent surfaces.
///
/// # Panics
///
/// If a key is not a parseable [`Keystroke`].
pub fn palette_shortcut_hint(
    keys: impl IntoIterator<Item = &'static str>,
    label: impl Into<SharedString>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(CHROME_GAP))
        .children(
            keys.into_iter()
                .map(|key| Kbd::new(Keystroke::parse(key).expect("static palette keystroke"))),
        )
        .child(label.into())
}
