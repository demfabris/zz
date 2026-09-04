//! [`ColorPicker`]: a swatch that opens a hex field and a grid of presets.

use gpui::{
    Anchor, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled as _, Subscription,
    Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, Colorize as _, Disableable, Sizable, Size,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    popover::Popover,
    v_flex,
};

const SWATCH_SIZE: Pixels = px(18.0);
const TRIGGER_SWATCH_SIZE: Pixels = px(16.0);

const SWATCH_COLUMNS: usize = 10;
const SWATCHES: [&str; 40] = [
    "#000000", "#0a0a0a", "#141414", "#1e1e1e", "#2d2d2d", "#454545", "#6b6b6b", "#9a9a9a",
    "#cccccc", "#ffffff", "#1a1b26", "#16161e", "#1e1e2e", "#181825", "#282828", "#1d2021",
    "#2e3440", "#3b4252", "#191724", "#1f1d2e", "#7aa2f7", "#7dcfff", "#9ece6a", "#e0af68",
    "#f7768e", "#bb9af7", "#89b4fa", "#a6e3a1", "#f9e2af", "#f38ba8", "#89dceb", "#cba6f7",
    "#fab387", "#94e2d5", "#b4befe", "#83a598", "#fabd2f", "#fb4934", "#d3869b", "#8ec07c",
];

pub enum ColorPickerEvent {
    /// The user committed a color, or cleared it back to inherited (`None`).
    Change(Option<Hsla>),
}

pub struct ColorPickerState {
    focus_handle: FocusHandle,
    hex: Entity<InputState>,
    color: Option<Hsla>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ColorPickerEvent> for ColorPickerState {}

impl Focusable for ColorPickerState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ColorPickerState {
    /// A picker starting on `color`, or inherited when it is `None`.
    pub fn new(color: Option<Hsla>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hex = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("#rrggbb")
                .default_value(color.map(crate::to_hex).unwrap_or_default())
        });
        let subscriptions =
            vec![
                cx.subscribe_in(&hex, window, |picker, hex, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        picker.commit_hex(hex, cx);
                    }
                }),
            ];
        Self {
            focus_handle: cx.focus_handle(),
            hex,
            color,
            _subscriptions: subscriptions,
        }
    }

    /// The committed color, or `None` while inherited.
    #[must_use]
    pub fn color(&self) -> Option<Hsla> {
        self.color
    }

    /// Push a color in without emitting, for echoing an external source back.
    pub fn set_color(&mut self, color: Option<Hsla>, window: &mut Window, cx: &mut Context<Self>) {
        if self.color == color {
            return;
        }
        self.color = color;
        let text = color.map(crate::to_hex).unwrap_or_default();
        self.hex
            .update(cx, |hex, cx| hex.set_value(text, window, cx));
        cx.notify();
    }

    fn commit_hex(&mut self, hex: &Entity<InputState>, cx: &mut Context<Self>) {
        let value = hex.read(cx).value();
        let color = if value.trim().is_empty() {
            None
        } else {
            match crate::parse_hex(&value) {
                Ok(color) => Some(color),
                Err(_) => return,
            }
        };
        self.emit(color, cx);
    }

    fn emit(&mut self, color: Option<Hsla>, cx: &mut Context<Self>) {
        self.color = color;
        cx.emit(ColorPickerEvent::Change(color));
        cx.notify();
    }
}

/// A swatch button that opens a hex field and a grid of preset colors.
#[derive(IntoElement)]
pub struct ColorPicker {
    state: Entity<ColorPickerState>,
    inherited: Hsla,
    label: Option<SharedString>,
    style: StyleRefinement,
    size: Size,
    disabled: bool,
}

impl ColorPicker {
    #[must_use]
    pub fn new(state: &Entity<ColorPickerState>, inherited: Hsla) -> Self {
        Self {
            state: state.clone(),
            inherited,
            label: None,
            style: StyleRefinement::default(),
            size: Size::Small,
            disabled: false,
        }
    }

    /// Heading shown above the hex field, naming what is being colored.
    #[must_use]
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn swatch_grid(state: &Entity<ColorPickerState>, cx: &App) -> impl IntoElement {
        let radius = cx.theme().radius;
        v_flex()
            .gap(px(4.0))
            .children(
                SWATCHES
                    .chunks(SWATCH_COLUMNS)
                    .enumerate()
                    .map(|(row, colors)| {
                        h_flex()
                            .gap(px(4.0))
                            .children(colors.iter().enumerate().map(|(column, hex)| {
                                let color = crate::parse_hex(hex).unwrap_or_default();
                                let state = state.clone();
                                div()
                                    .id(("zz-swatch", row * SWATCH_COLUMNS + column))
                                    .size(SWATCH_SIZE)
                                    .flex_none()
                                    .rounded(radius)
                                    .bg(color)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .cursor_pointer()
                                    .hover(|this| this.border_color(cx.theme().foreground))
                                    .tooltip(move |window, cx| {
                                        crate::tooltip::Tooltip::new(*hex).build(window, cx)
                                    })
                                    .on_click(move |_: &ClickEvent, window, cx| {
                                        state.update(cx, |picker, cx| {
                                            picker.set_color(Some(color), window, cx);
                                            picker.emit(Some(color), cx);
                                        });
                                    })
                            }))
                    }),
            )
    }
}

impl Sizable for ColorPicker {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Disableable for ColorPicker {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl gpui::Styled for ColorPicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ColorPicker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let committed = self.state.read(cx).color;
        let shown = committed.unwrap_or(self.inherited);
        let state = self.state.clone();
        let hex = self.state.read(cx).hex.clone();
        let label = self.label.clone();
        let size = self.size;

        let trigger = Button::new(("zz-color-picker", self.state.entity_id()))
            .ghost()
            .flat()
            .with_size(size)
            .compact()
            .disabled(self.disabled)
            .child(
                div()
                    .size(TRIGGER_SWATCH_SIZE)
                    .rounded(cx.theme().radius)
                    .bg(shown)
                    .border_1()
                    .border_color(cx.theme().border),
            );

        Popover::new(("zz-color-picker-popover", self.state.entity_id()))
            .anchor(Anchor::TopRight)
            .trigger(trigger)
            .content(move |_, _, cx| {
                v_flex()
                    .gap(px(8.0))
                    .w(px(248.0))
                    .when_some(label.clone(), |this, label| {
                        this.child(
                            div()
                                .text_size(crate::rems_from_px(11.0))
                                .text_color(cx.theme().foreground.muted())
                                .child(label),
                        )
                    })
                    .child(
                        h_flex()
                            .gap(px(6.0))
                            .child(Input::new(&hex).small().flex_1().min_w_0())
                            .child(
                                Button::new("zz-color-picker-clear")
                                    .ghost()
                                    .flat()
                                    .small()
                                    .compact()
                                    .icon(crate::IconName::Undo2)
                                    .tooltip("Clear the override")
                                    .on_click({
                                        let state = state.clone();
                                        move |_, window, cx| {
                                            state.update(cx, |picker, cx| {
                                                picker.set_color(None, window, cx);
                                                picker.emit(None, cx);
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(Self::swatch_grid(&state, cx))
            })
            .into_any_element()
    }
}
