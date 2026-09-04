use super::AGENT_CONTENT_MAX_WIDTH;
use crate::input::{Input, InputState};
use crate::{ActiveTheme as _, CHROME_GAP, Colorize as _, h_flex, v_flex};
use gpui::{
    AnyElement, App, Corners, Entity, Hsla, IntoElement, Pixels, RenderOnce, SharedString, Window,
    div, prelude::*, px,
};

pub const COMPOSER_MIN_HEIGHT: f32 = 86.0;
pub const COMPOSER_FOOTER_HEIGHT: f32 = 28.0;
pub const COMPOSER_OUTER_PADDING: f32 = 12.0;
pub const COMPOSER_SECTION_GAP: f32 = 8.0;
pub const COMPOSER_INPUT_PADDING_TOP: f32 = 12.0;
pub const COMPOSER_INPUT_PADDING_X: f32 = 14.0;
pub const COMPOSER_MAX_WIDTH: f32 = AGENT_CONTENT_MAX_WIDTH + 2.0;

pub const fn composer_total_height() -> f32 {
    COMPOSER_MIN_HEIGHT
        + 2.0 * COMPOSER_OUTER_PADDING
        + COMPOSER_SECTION_GAP
        + COMPOSER_FOOTER_HEIGHT
}

pub const fn composer_tail_clearance() -> f32 {
    composer_total_height() + COMPOSER_OUTER_PADDING
}

#[derive(IntoElement)]
pub struct AgentComposer {
    pub input: Entity<InputState>,
    pub action: AnyElement,
    pub settings: Vec<AnyElement>,
    pub usage: Option<AnyElement>,
    pub git: Option<AnyElement>,
    pub directory: AnyElement,
    pub command_hint: Option<SharedString>,
    pub prefix: Vec<AnyElement>,
    pub attachments: Option<AnyElement>,
    pub radii: Corners<Pixels>,
    pub background: Hsla,
}

impl RenderOnce for AgentComposer {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .bottom(px(0.0))
            .w_full()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(
                        px(COMPOSER_FOOTER_HEIGHT + COMPOSER_OUTER_PADDING + COMPOSER_SECTION_GAP)
                            + cx.theme().radius,
                    )
                    .bg(self.background)
                    .rounded_bl(self.radii.bottom_left)
                    .rounded_br(self.radii.bottom_right),
            )
            .child(
                v_flex()
                    .w_full()
                    .px(px(COMPOSER_OUTER_PADDING))
                    .pt(px(COMPOSER_OUTER_PADDING))
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(COMPOSER_MAX_WIDTH))
                            .mx_auto()
                            .gap(px(COMPOSER_SECTION_GAP))
                            .children(self.prefix)
                            .child(
                                v_flex()
                                    .w_full()
                                    .min_h(px(COMPOSER_MIN_HEIGHT))
                                    .rounded(cx.theme().radius)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().background.raised(1))
                                    .when(cx.theme().shadow, gpui::Styled::shadow_xs)
                                    .children(self.attachments)
                                    .child(
                                        Input::new(&self.input)
                                            .w_full()
                                            .min_w_0()
                                            .pt(px(COMPOSER_INPUT_PADDING_TOP))
                                            .px(px(COMPOSER_INPUT_PADDING_X))
                                            .text_size(crate::rems_from_px(13.0))
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false),
                                    )
                                    .when_some(self.command_hint, |this, hint| {
                                        this.child(
                                            div()
                                                .px_3()
                                                .pb_1()
                                                .text_size(crate::rems_from_px(10.0))
                                                .text_color(cx.theme().foreground.muted())
                                                .child(hint),
                                        )
                                    })
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .min_h(px(40.0))
                                            .items_end()
                                            .justify_between()
                                            .gap(px(CHROME_GAP))
                                            .p(px(CHROME_GAP))
                                            .child(
                                                h_flex()
                                                    .min_w_0()
                                                    .flex_wrap()
                                                    .gap(px(CHROME_GAP))
                                                    .children(self.settings),
                                            )
                                            .child(
                                                h_flex()
                                                    .flex_none()
                                                    .gap(px(CHROME_GAP))
                                                    .child(self.action),
                                            ),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .bg(self.background)
                    .rounded_bl(self.radii.bottom_left)
                    .rounded_br(self.radii.bottom_right)
                    .px(px(COMPOSER_OUTER_PADDING))
                    .pt(px(COMPOSER_SECTION_GAP))
                    .pb(px(COMPOSER_OUTER_PADDING))
                    .child(
                        h_flex()
                            .w_full()
                            .max_w(px(COMPOSER_MAX_WIDTH))
                            .mx_auto()
                            .h(px(COMPOSER_FOOTER_HEIGHT))
                            .items_center()
                            .justify_between()
                            .px_1()
                            .child(h_flex().min_w_0().flex_1().children(self.git))
                            .child(
                                h_flex()
                                    .flex_none()
                                    .gap(px(CHROME_GAP))
                                    .children(self.usage)
                                    .child(self.directory),
                            ),
                    ),
            )
    }
}
