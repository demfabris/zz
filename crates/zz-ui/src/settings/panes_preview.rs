use crate::{
    ActiveTheme as _, Colorize as _, Icon, IconName,
    pane::{
        PaneChrome, PaneSplitAxis, PaneSplitHighlight, PaneSplitSide, pane_border_color,
        pane_split_surface, pane_surface,
    },
};
use gpui::{App, Corners, IntoElement, Pixels, RenderOnce, Window, div, prelude::*, px};

#[derive(Clone, Copy, IntoElement)]
pub struct PanesPreview {
    pub gaps: bool,
    pub margin: f32,
    pub radius: f32,
    pub border_width: f32,
    pub inactive_opacity: f32,
}

impl RenderOnce for PanesPreview {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let selection =
            window.use_keyed_state("settings-pane-preview-selection", cx, |_, _| 0_usize);
        let selected = *selection.read(cx);
        let margin = px(if self.gaps { self.margin } else { 0.0 });
        let radius = px(if self.gaps { self.radius } else { 0.0 });
        let border = px(if self.gaps { self.border_width } else { 0.0 });
        let content_radius = (radius - border).max(px(0.0));
        let height = (f32::from(window.viewport_size().height) * 0.32)
            .clamp(180.0, 280.0)
            .max(f32::from(margin) * 3.0 + f32::from(border) * 4.0 + 112.0);
        let pane = |index: usize, content: gpui::Div| {
            let active = selected == index;
            pane_surface(
                ("settings-preview-pane", index),
                content
                    .rounded(content_radius)
                    .debug_selector(move || format!("settings-preview-active-{index}-{active}")),
                [],
                PaneChrome::new(
                    Corners::all(radius),
                    border,
                    pane_border_color(active, cx),
                    self.gaps,
                )
                .active(active)
                .dimmed(!active, self.inactive_opacity),
                cx,
            )
            .min_w_0()
            .min_h_0()
            .cursor_pointer()
            .debug_selector(move || {
                [
                    "settings-preview-terminal",
                    "settings-preview-agent",
                    "settings-preview-browser",
                ][index]
                    .to_owned()
            })
            .on_click(window.listener_for(&selection, move |selected, _, _, cx| {
                if *selected != index {
                    *selected = index;
                    cx.notify();
                }
            }))
        };
        let terminal = pane(0_usize, terminal_sample(cx));
        let agent = pane(1_usize, agent_sample(cx));
        let browser = pane(2_usize, browser_sample(content_radius, cx));
        let horizontal_highlight = match selected {
            0 => PaneSplitHighlight::new(0.0, 1.0, PaneSplitSide::First, cx.theme().accent),
            1 => PaneSplitHighlight::new(0.0, 0.5, PaneSplitSide::Second, cx.theme().accent),
            _ => PaneSplitHighlight::new(0.5, 0.5, PaneSplitSide::Second, cx.theme().accent),
        };
        let vertical_highlight = match selected {
            1 => Some(PaneSplitHighlight::new(
                0.0,
                1.0,
                PaneSplitSide::First,
                cx.theme().accent,
            )),
            2 => Some(PaneSplitHighlight::new(
                0.0,
                1.0,
                PaneSplitSide::Second,
                cx.theme().accent,
            )),
            _ => None,
        };
        let right = pane_split_surface(
            "settings-preview-vertical-split",
            PaneSplitAxis::Vertical,
            0.5,
            false,
            self.gaps,
            margin,
            None,
            vertical_highlight,
            agent,
            browser,
            div().absolute(),
            cx,
        );
        div()
            .id("settings-panes-preview")
            .w_full()
            .h(px(height))
            .flex_none()
            .overflow_hidden()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border())
            .bg(cx.theme().background.raised(2))
            .text_color(cx.theme().foreground)
            .text_size(px(11.0))
            .line_height(px(16.0))
            .p(margin)
            .child(pane_split_surface(
                "settings-preview-horizontal-split",
                PaneSplitAxis::Horizontal,
                0.55,
                false,
                self.gaps,
                margin,
                None,
                Some(horizontal_highlight),
                terminal,
                right,
                div().absolute(),
                cx,
            ))
    }
}

fn sample_surface(cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .bg(cx
            .theme()
            .background
            .opaque()
            .opacity(cx.theme().pane_background_opacity))
}

fn sample_header(icon: IconName, title: &'static str, cx: &App) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .flex_none()
        .h(px(28.0))
        .px(px(10.0))
        .gap(px(6.0))
        .text_color(cx.theme().foreground.muted())
        .child(
            div()
                .relative()
                .top(px(0.5))
                .child(Icon::new(icon).size(px(12.0))),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(title),
        )
}

fn terminal_sample(cx: &App) -> gpui::Div {
    sample_surface(cx)
        .child(sample_header(IconName::SquareTerminal, "Terminal", cx))
        .child(
            div()
                .flex()
                .flex_col()
                .min_h_0()
                .overflow_hidden()
                .p(px(12.0))
                .gap(px(4.0))
                .font_family(cx.theme().mono_font_family.clone())
                .whitespace_nowrap()
                .child(
                    div()
                        .text_color(cx.theme().foreground.muted())
                        .child("~/workspace"),
                )
                .child("❯ cargo run")
                .child(
                    div()
                        .text_color(cx.theme().success)
                        .child("Finished in 0.42s"),
                )
                .child("Hello, workspace.")
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .pt(px(8.0))
                        .child("❯")
                        .child(div().w(px(6.0)).h(px(12.0)).bg(cx.theme().foreground)),
                ),
        )
}

fn agent_sample(cx: &App) -> gpui::Div {
    sample_surface(cx)
        .child(sample_header(IconName::Bot, "Agent", cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .px(px(10.0))
                .child("Ready when you are."),
        )
        .child(
            div()
                .flex_none()
                .h(px(24.0))
                .mx(px(6.0))
                .mb(px(6.0))
                .px(px(6.0))
                .flex()
                .items_center()
                .rounded(cx.theme().radius)
                .bg(cx.theme().background.opaque())
                .border_1()
                .border_color(cx.theme().border())
                .text_color(cx.theme().foreground.muted())
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child("Ask anything…"),
                ),
        )
}

fn browser_sample(content_radius: Pixels, cx: &App) -> gpui::Div {
    sample_surface(cx)
        .child(sample_header(IconName::Globe, "Browser", cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .px(px(10.0))
                .py(px(6.0))
                .rounded_bl(content_radius)
                .rounded_br(content_radius)
                .bg(cx.theme().background.opaque())
                .child("Welcome to zz")
                .child(
                    div()
                        .text_color(cx.theme().foreground.muted())
                        .child("Your workspace, together."),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Modifiers, Render, TestAppContext, VisualTestContext};

    struct PreviewTest(PanesPreview);

    impl Render for PreviewTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(500.0)).child(self.0)
        }
    }

    fn preview() -> PanesPreview {
        PanesPreview {
            gaps: true,
            margin: 8.0,
            radius: 12.0,
            border_width: 1.0,
            inactive_opacity: 0.6,
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn clicking_samples_changes_selection_and_preserves_it_on_settings_updates(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, _| PreviewTest(preview()));
        draw(cx);
        assert!(cx.debug_bounds("settings-preview-active-0-true").is_some());
        for (pane, active) in [
            ("settings-preview-agent", "settings-preview-active-1-true"),
            ("settings-preview-browser", "settings-preview-active-2-true"),
            (
                "settings-preview-terminal",
                "settings-preview-active-0-true",
            ),
        ] {
            let bounds = cx.debug_bounds(pane).unwrap();
            cx.simulate_click(bounds.center(), Modifiers::none());
            draw(cx);
            assert!(cx.debug_bounds(active).is_some());
        }
        let bounds = cx.debug_bounds("settings-preview-agent").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::none());
        view.update(cx, |view, cx| {
            view.0.margin = 16.0;
            cx.notify();
        });
        draw(cx);
        assert!(cx.debug_bounds("settings-preview-active-1-true").is_some());
        assert!(cx.debug_bounds("settings-preview-active-0-true").is_none());
    }

    #[gpui::test]
    fn spacing_and_border_updates_use_workspace_pixel_geometry(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, _| PreviewTest(preview()));
        for (gaps, margin, border) in [(true, 8.0, 1.0), (true, 32.0, 8.0), (false, 32.0, 8.0)] {
            view.update(cx, |view, cx| {
                view.0.gaps = gaps;
                view.0.margin = margin;
                view.0.border_width = border;
                cx.notify();
            });
            draw(cx);
            let terminal = cx.debug_bounds("settings-preview-terminal").unwrap();
            let agent = cx.debug_bounds("settings-preview-agent").unwrap();
            let browser = cx.debug_bounds("settings-preview-browser").unwrap();
            let terminal_content = cx.debug_bounds("settings-preview-active-0-true").unwrap();
            let gap = px(if gaps { margin } else { 1.0 });
            assert_eq!(agent.origin.x - terminal.right(), gap);
            assert_eq!(browser.origin.y - agent.bottom(), gap);
            assert_eq!(terminal.origin.x, px(1.0 + if gaps { margin } else { 0.0 }));
            assert_eq!(
                terminal_content.origin.x - terminal.origin.x,
                px(if gaps { border } else { 0.0 })
            );
            assert!(browser.size.height > px(40.0));
        }
    }
}
