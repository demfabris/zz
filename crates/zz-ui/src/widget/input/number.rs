//! [`NumberInput`]: a text field flanked by `-` and `+` steppers.

use gpui::{
    App, Context, Entity, InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _,
    RenderOnce, Role, StatefulInteractiveElement as _, StyleRefinement, Styled, Window, actions,
    prelude::FluentBuilder as _,
};

use crate::{
    ActiveTheme as _, Disableable, Sizable, Size, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    icon::IconName,
};

use super::{
    field::Input,
    state::{InputState, StepDirection},
};
use crate::Colorize as _;
use gpui::TextAlign;

actions!(zz_number_input, [Increment, Decrement]);

const CONTEXT: &str = "ZzNumberInput";

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", Increment, Some(CONTEXT)),
        KeyBinding::new("down", Decrement, Some(CONTEXT)),
    ]);
}

pub(super) fn is_number_like(value: &str) -> bool {
    let body = value.strip_prefix(['-', '+']).unwrap_or(value);
    let mut seen_dot = false;
    for c in body.chars() {
        if c == '.' {
            if seen_dot {
                return false;
            }
            seen_dot = true;
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    true
}

fn fraction_digits(value: &str) -> usize {
    value.split('.').nth(1).map_or(0, str::len)
}

pub(super) fn stepped(
    current: &str,
    direction: StepDirection,
    step: f64,
    min: Option<f64>,
    max: Option<f64>,
) -> Option<String> {
    let current = current.trim();
    let parsed = current.parse::<f64>().ok();
    let base = parsed.unwrap_or(0.0);

    let mut value = match direction {
        StepDirection::Increment => base + step,
        StepDirection::Decrement => base - step,
    };
    let mut digits = fraction_digits(current).max(fraction_digits(&step.to_string()));

    if let Some(min) = min
        && value < min
    {
        value = min;
        digits = digits.max(fraction_digits(&min.to_string()));
    }
    if let Some(max) = max
        && value > max
    {
        value = max;
        digits = digits.max(fraction_digits(&max.to_string()));
    }

    if let Some(base) = parsed {
        let moved = match direction {
            StepDirection::Increment => value > base,
            StepDirection::Decrement => value < base,
        };
        if !moved {
            return None;
        }
    }

    Some(format!("{value:.digits$}"))
}

impl InputState {
    fn on_increment(&mut self, _: &Increment, _: &mut Window, cx: &mut Context<Self>) {
        self.step_value(StepDirection::Increment, cx);
    }

    fn on_decrement(&mut self, _: &Decrement, _: &mut Window, cx: &mut Context<Self>) {
        self.step_value(StepDirection::Decrement, cx);
    }
}

/// A bounded, steppable number field bound to an [`InputState`], which carries
/// the step and the bounds.
#[derive(IntoElement)]
pub struct NumberInput {
    state: Entity<InputState>,
    style: StyleRefinement,
    size: Size,
    appearance: bool,
    disabled: bool,
}

impl NumberInput {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            size: Size::default(),
            appearance: true,
            disabled: false,
        }
    }

    /// Draw the group's background, radius and border.
    #[must_use]
    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    fn stepper(
        state: &Entity<InputState>,
        id: &'static str,
        icon: IconName,
        direction: StepDirection,
        size: Size,
        disabled: bool,
    ) -> Button {
        let state = state.clone();
        Button::new(id)
            .ghost()
            .flat()
            .with_size(size)
            .icon(icon)
            .compact()
            .tab_stop(false)
            .disabled(disabled)
            .on_click(move |_, window, cx| {
                state.update(cx, |this, cx| {
                    this.focus(window, cx);
                    this.step_value(direction, cx);
                });
            })
    }
}

impl Sizable for NumberInput {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Disableable for NumberInput {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for NumberInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NumberInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, InputState::mark_numeric);

        let focused = self.state.read(cx).focus_handle_ref().is_focused(window) && !self.disabled;

        h_flex()
            .id(("zz-number-input", self.state.entity_id()))
            .role(Role::SpinButton)
            .key_context(CONTEXT)
            .on_action(window.listener_for(&self.state, InputState::on_increment))
            .on_action(window.listener_for(&self.state, InputState::on_decrement))
            .w_full()
            .items_center()
            .when(self.appearance, |this| {
                this.bg(cx.theme().background.raised(1))
                    .rounded(cx.theme().radius)
                    .control_surface(cx)
                    .when(focused, |this| this.border_color(cx.theme().foreground))
            })
            .when(self.disabled, |this| this.opacity(0.5))
            .refine_style(&self.style)
            .child(Self::stepper(
                &self.state,
                "zz-number-decrement",
                IconName::Minus,
                StepDirection::Decrement,
                self.size,
                self.disabled,
            ))
            .child(
                Input::new(&self.state)
                    .with_size(self.size)
                    .disabled(self.disabled)
                    .appearance(false)
                    .text_align(TextAlign::Center)
                    .flex_1()
                    .min_w_0(),
            )
            .child(Self::stepper(
                &self.state,
                "zz-number-increment",
                IconName::Plus,
                StepDirection::Increment,
                self.size,
                self.disabled,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_shapes_are_accepted_including_partial_ones() {
        for value in ["", "-", "+", ".", "1", "-1", "1.", "1.5", "0.05", "256"] {
            assert!(is_number_like(value), "{value:?} should be accepted");
        }
        for value in ["a", "1a", "1.2.3", "1 2", "--1", "1-", "1e5"] {
            assert!(!is_number_like(value), "{value:?} should be rejected");
        }
    }

    #[test]
    fn stepping_keeps_the_widest_precision() {
        assert_eq!(
            stepped("0.1", StepDirection::Increment, 0.2, None, None).as_deref(),
            Some("0.3")
        );
        assert_eq!(
            stepped("1", StepDirection::Increment, 0.05, None, None).as_deref(),
            Some("1.05")
        );
        assert_eq!(
            stepped("4", StepDirection::Decrement, 1.0, None, None).as_deref(),
            Some("3")
        );
    }

    #[test]
    fn stepping_clamps_to_the_bounds() {
        assert_eq!(
            stepped("255", StepDirection::Increment, 4.0, Some(0.0), Some(256.0)).as_deref(),
            Some("256")
        );
        assert_eq!(
            stepped("2", StepDirection::Decrement, 4.0, Some(0.0), Some(256.0)).as_deref(),
            Some("0")
        );
    }

    #[test]
    fn a_step_that_cannot_move_the_value_does_nothing() {
        assert_eq!(
            stepped("256", StepDirection::Increment, 1.0, None, Some(256.0)),
            None
        );
        assert_eq!(
            stepped("0", StepDirection::Decrement, 1.0, Some(0.0), None),
            None
        );
        assert_eq!(
            stepped("-5", StepDirection::Decrement, 1.0, Some(0.0), None),
            None
        );
    }

    #[test]
    fn an_empty_value_steps_into_range() {
        assert_eq!(
            stepped("", StepDirection::Increment, 1.0, Some(10.0), None).as_deref(),
            Some("10")
        );
        assert_eq!(
            stepped("", StepDirection::Increment, 1.0, None, None).as_deref(),
            Some("1")
        );
    }
}
