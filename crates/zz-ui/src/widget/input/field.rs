//! [`Input`]: the chrome around a text field, rebuilt every frame.

use gpui::{
    AnyElement, App, DefiniteLength, Entity, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, RenderOnce, Role, StatefulInteractiveElement as _,
    StyleRefinement, Styled, TextAlign, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, Disableable, Sizable, Size, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    icon::{Icon, IconName},
    spinner::Spinner,
};

use super::{actions, state::InputState};
use crate::Colorize as _;

const LINE_HEIGHT: DefiniteLength = DefiniteLength::Fraction(1.25);
const SMALL_TEXT_SIZE: f32 = 13.;

/// A semantic hint about what a field holds, for the accessibility role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContentType {
    Url,
    EmailAddress,
    /// A password: the rendered text is masked, single-line only. The value is
    /// untouched, so copy still yields the real text.
    Password,
}

impl InputContentType {
    const fn role(self) -> Role {
        match self {
            Self::Url => Role::UrlInput,
            Self::EmailAddress => Role::EmailInput,
            Self::Password => Role::PasswordInput,
        }
    }

    const fn masks(self) -> bool {
        matches!(self, Self::Password)
    }
}

/// A text field bound to an [`InputState`].
#[derive(IntoElement)]
pub struct Input {
    state: Entity<InputState>,
    style: StyleRefinement,
    size: Size,
    prefix: Option<AnyElement>,
    suffix: Option<AnyElement>,
    appearance: bool,
    bordered: bool,
    focus_bordered: bool,
    cleanable: bool,
    disabled: bool,
    tab_index: isize,
    content_type: Option<InputContentType>,
    align: TextAlign,
}

impl Input {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            size: Size::default(),
            prefix: None,
            suffix: None,
            appearance: true,
            bordered: true,
            focus_bordered: true,
            cleanable: false,
            disabled: false,
            tab_index: 0,
            content_type: None,
            align: TextAlign::Left,
        }
    }

    /// Place the text horizontally within the field. Hit-testing, the caret and
    /// selection quads all follow the alignment.
    #[must_use]
    pub fn text_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    #[must_use]
    pub fn prefix(mut self, prefix: impl IntoElement) -> Self {
        self.prefix = Some(prefix.into_any_element());
        self
    }

    #[must_use]
    pub fn suffix(mut self, suffix: impl IntoElement) -> Self {
        self.suffix = Some(suffix.into_any_element());
        self
    }

    /// Draw the field's background, radius and border. With `false` the field
    /// is bare text, for a field embedded in chrome that already has an edge.
    #[must_use]
    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    /// Draw the resting border. Only meaningful with [`Self::appearance`].
    #[must_use]
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Draw the focus ring. Only meaningful with [`Self::bordered`].
    #[must_use]
    pub fn focus_bordered(mut self, bordered: bool) -> Self {
        self.focus_bordered = bordered;
        self
    }

    /// Show a clear button while the field is non-empty. Single-line only.
    #[must_use]
    pub fn cleanable(mut self, cleanable: bool) -> Self {
        self.cleanable = cleanable;
        self
    }

    #[must_use]
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = index;
        self
    }

    /// What the field holds, for assistive technology.
    #[must_use]
    pub fn content_type(mut self, content_type: InputContentType) -> Self {
        self.content_type = Some(content_type);
        self
    }

    fn sized<E: Styled>(element: E, size: Size) -> E {
        let element = element.px(size.input_px()).py(size.input_py());
        match size {
            Size::XSmall => element.text_xs(),
            Size::Small => element.text_size(crate::rems_from_px(SMALL_TEXT_SIZE)),
            Size::Medium => element.text_sm(),
            Size::Large => element.text_base(),
            Size::Size(value) => element.text_size(value * 0.875),
        }
    }

    fn height<E: Styled>(element: E, size: Size) -> E {
        element.h(size.control_h())
    }

    fn colors(disabled: bool, cx: &App) -> (Hsla, Hsla) {
        if disabled {
            (
                cx.theme().background.raised(1).opacity(0.5),
                cx.theme().foreground.muted(),
            )
        } else {
            (cx.theme().background.raised(1), cx.theme().foreground)
        }
    }

    fn clear_button(state: &Entity<InputState>, cx: &App) -> Button {
        let state = state.clone();
        Button::new("zz-input-clear")
            .icon(Icon::new(IconName::CircleX))
            .ghost()
            .flat()
            .xsmall()
            .tab_stop(false)
            .text_color(cx.theme().foreground.muted())
            .on_click(move |_, window, cx| {
                state.update(cx, |this, cx| {
                    this.clear(cx);
                    this.focus(window, cx);
                });
            })
    }
}

impl Sizable for Input {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Disableable for Input {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for Input {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Input {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, _| {
            state.disabled = self.disabled;
            state.size = self.size;
            state.align = self.align;
            state.masked = self.content_type.is_some_and(InputContentType::masks);
        });

        let (multi_line, focused, empty, loading, focus_handle) = {
            let state = self.state.read(cx);
            (
                state.mode().is_multi_line(),
                state.focus_handle_ref().is_focused(window) && !self.disabled,
                state.text().is_empty(),
                state.loading,
                state.focus_handle_ref().clone(),
            )
        };

        let role = if multi_line {
            Role::MultilineTextInput
        } else {
            self.content_type
                .map_or(Role::TextInput, InputContentType::role)
        };

        let gap = match self.size {
            Size::XSmall | Size::Small => px(4.),
            Size::Large => px(8.),
            Size::Medium | Size::Size(_) => px(6.),
        };
        let (background, foreground) = Self::colors(self.disabled, cx);
        let show_clear = self.cleanable && !self.disabled && !loading && !empty && !multi_line;
        let clear_button = show_clear.then(|| Self::clear_button(&self.state, cx));
        let editable = !self.disabled;
        let disabled = self.disabled;
        let spinner_color = cx.theme().foreground.muted();
        let prefix = self.prefix;
        let suffix = self.suffix;
        let has_suffix = suffix.is_some() || loading || show_clear;

        let mut element = div()
            .id(("zz-input", self.state.entity_id()))
            .role(role)
            .key_context(actions::CONTEXT)
            .track_focus(&focus_handle)
            .tab_index(self.tab_index)
            .flex()
            .items_center()
            .gap(gap)
            .w_full()
            .line_height(LINE_HEIGHT)
            .text_color(foreground)
            .when(editable, gpui::Styled::cursor_text);

        element = Self::sized(element, self.size);
        if !multi_line {
            element = Self::height(element, self.size);
        }

        element
            .when(self.appearance, |this| {
                this.bg(background)
                    .rounded(cx.theme().radius)
                    .when(self.bordered, |this| {
                        this.control_surface(cx)
                            .when(self.disabled, |this| {
                                this.border_color(cx.theme().foreground.opacity(0.05))
                                    .shadow_none()
                            })
                            .when(focused && self.focus_bordered, |this| {
                                this.border_color(cx.theme().foreground)
                            })
                    })
            })
            .refine_style(&self.style)
            .when(editable, |this| {
                this.on_action(window.listener_for(&self.state, InputState::backspace))
                    .on_action(window.listener_for(&self.state, InputState::delete))
                    .on_action(
                        window.listener_for(&self.state, InputState::delete_to_previous_word_start),
                    )
                    .on_action(
                        window.listener_for(&self.state, InputState::delete_to_next_word_end),
                    )
                    .on_action(
                        window.listener_for(&self.state, InputState::delete_to_beginning_of_line),
                    )
                    .on_action(window.listener_for(&self.state, InputState::delete_to_end_of_line))
                    .on_action(window.listener_for(&self.state, InputState::enter))
                    .on_action(window.listener_for(&self.state, InputState::cut))
                    .on_action(window.listener_for(&self.state, InputState::paste))
                    .on_action(window.listener_for(&self.state, InputState::undo))
                    .on_action(window.listener_for(&self.state, InputState::redo))
                    .on_action(window.listener_for(&self.state, InputState::show_character_palette))
            })
            .on_action(window.listener_for(&self.state, InputState::move_left))
            .on_action(window.listener_for(&self.state, InputState::move_right))
            .on_action(window.listener_for(&self.state, InputState::select_left))
            .on_action(window.listener_for(&self.state, InputState::select_right))
            .on_action(window.listener_for(&self.state, InputState::move_home))
            .on_action(window.listener_for(&self.state, InputState::move_end))
            .on_action(window.listener_for(&self.state, InputState::select_to_start_of_line))
            .on_action(window.listener_for(&self.state, InputState::select_to_end_of_line))
            .on_action(window.listener_for(&self.state, InputState::move_to_start))
            .on_action(window.listener_for(&self.state, InputState::move_to_end))
            .on_action(window.listener_for(&self.state, InputState::select_to_start))
            .on_action(window.listener_for(&self.state, InputState::select_to_end))
            .on_action(window.listener_for(&self.state, InputState::move_to_previous_word))
            .on_action(window.listener_for(&self.state, InputState::move_to_next_word))
            .on_action(window.listener_for(&self.state, InputState::select_to_previous_word_start))
            .on_action(window.listener_for(&self.state, InputState::select_to_next_word_end))
            .on_action(window.listener_for(&self.state, InputState::select_all))
            .on_action(window.listener_for(&self.state, InputState::copy))
            .on_action(window.listener_for(&self.state, InputState::escape))
            .when(multi_line, |this| {
                this.on_action(window.listener_for(&self.state, InputState::move_up))
                    .on_action(window.listener_for(&self.state, InputState::move_down))
                    .on_action(window.listener_for(&self.state, InputState::select_up))
                    .on_action(window.listener_for(&self.state, InputState::select_down))
            })
            .on_key_down(window.listener_for(&self.state, InputState::on_key_down))
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&self.state, InputState::on_mouse_down),
            )
            .on_mouse_down(
                MouseButton::Right,
                window.listener_for(&self.state, InputState::on_mouse_down),
            )
            .on_scroll_wheel(window.listener_for(&self.state, InputState::on_scroll_wheel))
            .children(prefix.map(|prefix| {
                div()
                    .flex_none()
                    .when(disabled, |this| this.opacity(0.5))
                    .child(prefix)
            }))
            .child(self.state.clone())
            .when(has_suffix, |this| {
                this.child(
                    h_flex()
                        .flex_none()
                        .gap(gap)
                        .items_center()
                        .when(disabled, |this| this.opacity(0.5))
                        .when(loading, |this| {
                            this.child(
                                Spinner::new()
                                    .with_size(self.size.smaller())
                                    .color(spinner_color),
                            )
                        })
                        .children(clear_button)
                        .children(suffix),
                )
            })
    }
}
