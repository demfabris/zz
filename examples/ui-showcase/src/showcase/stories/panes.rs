//! Pane-level indicators and the overlays layered over a terminal grid.

use gpui::{
    AnyElement, App, Context, Corners, Keystroke, ParentElement as _, Pixels, div, prelude::*, px,
};
use zz_ui::ActiveTheme as _;
use zz_ui::kbd::Kbd;
use zz_ui::pane::{
    PaneChrome, PaneDragOverlayState, PaneOverlayCorner, PaneSplitAxis, pane_drag_chip,
    pane_drag_overlay, pane_drop_preview, pane_indicator_card, pane_indicator_overlay,
    pane_overlay_stack, pane_split_hit_target, pane_split_surface, pane_surface, pane_sync_badge,
    pane_unzoom_control, pane_waiting_state, terminal_link_popup, terminal_mode_indicator,
    terminal_search_prompt, terminal_status_popup,
};
use zz_ui::shell::app_connection_state;

use super::{
    Showcase, gallery, mock_terminal, specimen, specimen_block, specimen_over_terminal, specimens,
    story_stack,
};

pub(super) fn render(cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Pane chrome",
                "The real pane surface in flush and gapped states. Chrome is neutral everywhere: the active pane is the one at full strength, and every other pane fades behind a scrim.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "flush · no chrome",
                        pane_chrome_fixture("pane-flush", 0.0, 0.0, 0.0, true, cx),
                        cx,
                    ))
                    .child(specimen(
                        "gapped · inactive · dimmed",
                        pane_chrome_fixture(
                            "pane-gapped-inactive",
                            8.0,
                            6.0,
                            1.0,
                            false,
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen(
                        "gapped · active",
                        pane_chrome_fixture(
                            "pane-gapped-active",
                            8.0,
                            6.0,
                            1.0,
                            true,
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen(
                        "gapped · borderless",
                        pane_chrome_fixture(
                            "pane-gapped-minimal",
                            8.0,
                            6.0,
                            0.0,
                            true,
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen("split · divider", pane_split_fixture(false, cx), cx))
                    .child(specimen("split · gaps", pane_split_fixture(true, cx), cx)),
            ),
        )
        .child(
            gallery(
                "Prefix-armed pane dragging",
                "The real layout stays live for the whole drag: panes wear the armed tint, the pane in flight recedes behind the inactive scrim, a chip trails the pointer, and one preview rect shows where a release would land.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen_over_terminal(
                        "armed",
                        240.0,
                        150.0,
                        pane_drag_overlay(
                            "pane-drag-armed",
                            PaneDragOverlayState::Armed,
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "in flight",
                        240.0,
                        150.0,
                        pane_drag_overlay(
                            "pane-drag-source",
                            PaneDragOverlayState::Source,
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen(
                        "cursor chip",
                        pane_drag_chip("%3", "terminal · project", cx),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "drop preview · split",
                        240.0,
                        150.0,
                        pane_drop_preview(px(6.0), px(1.0), cx)
                            .left(px(0.0))
                            .top(px(0.0))
                            .w(px(120.0))
                            .h(px(150.0)),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "drop preview · swap",
                        240.0,
                        150.0,
                        pane_drop_preview(px(6.0), px(1.0), cx)
                            .left(px(0.0))
                            .top(px(0.0))
                            .w(px(240.0))
                            .h(px(150.0)),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Display-panes labels",
                "Bind+q overlays every pane with its index and its selection shortcut, rendered through the shared Kbd pill. The active pane uses danger semantics; inactive panes use the neutral list surface.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen_over_terminal(
                        "active pane · key 1",
                        240.0,
                        150.0,
                        pane_indicator_overlay(pane_indicator_card(
                            "pane-ind-active",
                            "1",
                            key_pill("1"),
                            true,
                            cx.theme().mono_font_family.clone(),
                            cx,
                        )),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "inactive pane · key 2",
                        240.0,
                        150.0,
                        pane_indicator_overlay(pane_indicator_card(
                            "pane-ind-idle",
                            "2",
                            key_pill("2"),
                            false,
                            cx.theme().mono_font_family.clone(),
                            cx,
                        )),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Pane status controls",
                "Synchronized-input caution, the zoom release, and the pending-entity placeholder all reuse the Tag treatment and share one top-right stack so they never overlap.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen_over_terminal(
                        "synchronized input",
                        240.0,
                        150.0,
                        top_right(pane_sync_badge(cx)),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "zoomed pane",
                        240.0,
                        150.0,
                        top_right(pane_unzoom_control()),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "pending entity",
                        240.0,
                        150.0,
                        top_right(pane_waiting_state("waiting for %7")),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "stacked",
                        240.0,
                        150.0,
                        pane_overlay_stack(
                            PaneOverlayCorner::TopRight,
                            [
                                pane_sync_badge(cx).into_any_element(),
                                pane_unzoom_control().into_any_element(),
                            ],
                        ),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Terminal mode indicators",
                "Copy mode, view mode, and unseen output reuse the Tag treatment in the top-right corner.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen_over_terminal(
                        "copy mode",
                        280.0,
                        120.0,
                        top_right(mode(Some("COPY MODE"), "284/1200 · +14 output")),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "view mode",
                        280.0,
                        120.0,
                        top_right(mode(Some("VIEW MODE"), "96/420 · q close")),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "unseen output",
                        280.0,
                        120.0,
                        top_right(mode(None, "+27 output")),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Search, status & link overlays",
                "Find prompt, transient status, and hovered-URI previews all reuse the Tag treatment and share the pane's bottom-right stack.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen_over_terminal(
                        "search prompt",
                        420.0,
                        150.0,
                        bottom_right(terminal_search_prompt(
                            "Find: renderer  3/8  [forward, literal, smart-case]",
                            cx,
                        )),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "invalid search",
                        420.0,
                        150.0,
                        bottom_right(terminal_search_prompt(
                            "Find: [unterminated  invalid pattern  [forward, regex]",
                            cx,
                        )),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "status popup",
                        300.0,
                        150.0,
                        bottom_right(terminal_status_popup("Copied 3 lines to clipboard", cx)),
                        cx,
                    ))
                    .child(specimen_over_terminal(
                        "URI preview",
                        360.0,
                        150.0,
                        bottom_right(terminal_link_popup(
                            "https://gpui.rs/docs/components/input",
                            cx,
                        )),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Workspace connection state",
                "What the workspace shows instead of panes while there is nothing to attach to: the dial-up line, and whatever the daemon or ssh said when it failed. The same treatment a pane uses for an entity it is still waiting on.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "connecting",
                        connection_stage(app_connection_state("connecting to zz daemon…", cx), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "failed",
                        connection_stage(
                            app_connection_state(
                                "ssh: connect to host builder port 22: connection refused",
                                cx,
                            ),
                            cx,
                        ),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

fn connection_stage(message: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .flex()
        .w_full()
        .h(px(96.0))
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(message)
        .into_any_element()
}

fn pane_chrome_fixture(
    id: &'static str,
    margin: f32,
    radius: f32,
    border_width: f32,
    active: bool,
    cx: &App,
) -> gpui::Div {
    div()
        .w(px(240.0))
        .h(px(150.0))
        .p(px(margin))
        .child(pane_leaf(
            id,
            radius,
            border_width,
            margin > 0.0,
            active,
            cx,
        ))
}

fn pane_split_fixture(gaps: bool, cx: &App) -> gpui::Div {
    let (surface_id, first_id, second_id, hit_id) = if gaps {
        (
            "pane-split-gapped",
            "pane-split-gapped-first",
            "pane-split-gapped-second",
            "pane-split-gapped-hit",
        )
    } else {
        (
            "pane-split-flush",
            "pane-split-flush-first",
            "pane-split-flush-second",
            "pane-split-flush-hit",
        )
    };
    let margin = if gaps { 8.0 } else { 0.0 };
    let radius = if gaps { 6.0 } else { 0.0 };
    let border_width = if gaps { 1.0 } else { 0.0 };
    let first = pane_leaf(first_id, radius, border_width, margin > 0.0, true, cx);
    let second = pane_leaf(second_id, radius, border_width, margin > 0.0, false, cx);

    div()
        .w(px(480.0))
        .h(px(180.0))
        .p(px(margin))
        .child(pane_split_surface(
            surface_id,
            PaneSplitAxis::Horizontal,
            0.5,
            false,
            gaps,
            px(margin),
            None,
            None,
            first,
            second,
            pane_split_hit_target(hit_id, PaneSplitAxis::Horizontal, 0.5, px(margin)),
            cx.theme().background,
            cx,
        ))
}

fn pane_leaf(
    id: &'static str,
    radius: f32,
    border_width: f32,
    shadow: bool,
    active: bool,
    cx: &App,
) -> gpui::Div {
    div().flex().size_full().child(pane_surface(
        id,
        mock_terminal(cx),
        std::iter::empty::<AnyElement>(),
        PaneChrome::new(
            uniform_radii(radius),
            px(border_width),
            cx.theme().border,
            cx.theme().background,
            shadow,
        )
        .dimmed(!active, 0.7),
        cx,
    ))
}

fn uniform_radii(radius: f32) -> Corners<Pixels> {
    let radius = px(radius);
    Corners {
        top_left: radius,
        top_right: radius,
        bottom_right: radius,
        bottom_left: radius,
    }
}

fn mode(label: Option<&'static str>, detail: &'static str) -> impl IntoElement {
    terminal_mode_indicator(label, detail)
}

fn key_pill(key: &'static str) -> Kbd {
    Kbd::new(Keystroke::parse(key).expect("static showcase keystroke"))
}

fn top_right(tag: impl IntoElement) -> impl IntoElement {
    pane_overlay_stack(PaneOverlayCorner::TopRight, [tag.into_any_element()])
}

fn bottom_right(tag: impl IntoElement) -> impl IntoElement {
    pane_overlay_stack(PaneOverlayCorner::BottomRight, [tag.into_any_element()])
}
