use zpui::{
    AnyElement, App, IntoElement, Keystroke, ParentElement as _, Styled as _, Window, div,
    prelude::*, px, relative,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName,
    kbd::Kbd,
    pane::{
        FloatingSurface, PaneDragOverlayState, PaneOverlayCorner, PaneSplitAxis,
        PaneSplitHighlight, PaneSplitSide, TERMINAL_HEADER_HEIGHT, pane_drag_chip,
        pane_drag_overlay, pane_drop_preview, pane_indicator_card, pane_indicator_overlay,
        pane_overlay_stack, pane_picker_choices, pane_picker_row, pane_split_surface,
        pane_status_badge, pane_sync_badge, pane_unzoom_control, pane_waiting_state,
        terminal_link_popup, terminal_mode_indicator, terminal_search_prompt,
        terminal_status_popup,
    },
    settings::panes_preview::PanesPreview,
};

use super::fixtures::{
    BUILD, INACTIVE_OPACITY, LOGS, PANE_BORDER, PANE_MARGIN, PANE_RADIUS, SERVER, TerminalPane,
    gap, pane_background, placeholder_pane, radii, terminal_body,
};
use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "panes",
    name: "Panes",
    group: "Workspace",
    summary: "The frame around every pane, its header and corner tags, splits, the display-panes picker, drag and drop, floating popups and the new-pane picker.",
    sections: &[
        Section {
            id: "frames",
            name: "Frames",
            summary: "pane_surface with and without pane gaps. Terminals dim their text when inactive; other panes take the inactive scrim. Try the pane-glow and pane-opacity knobs.",
            build: |_, cx| stateless(frames, cx),
        },
        Section {
            id: "terminal-header",
            name: "Terminal header",
            summary: "terminal_pane_header with its split, drag and close actions. An inactive header hides its actions until the pointer is over it.",
            build: |_, cx| stateless(terminal_header, cx),
        },
        Section {
            id: "tags",
            name: "Status tags",
            summary: "The tags a pane stacks in its top-right and bottom-right corners: dead, waiting, sync, zoom, copy and view modes, search, status lines and link previews.",
            build: |_, cx| stateless(tags, cx),
        },
        Section {
            id: "display-panes",
            name: "Display panes",
            summary: "pane_indicator_card for tmux display-panes. The active pane takes danger colors; the others the neutral wash.",
            build: |_, cx| stateless(display_panes, cx),
        },
        Section {
            id: "splits",
            name: "Splits",
            summary: "pane_split_surface. Without gaps the slot is a 1px hairline, and the active pane's side of it turns accent; with gaps the slot is the margin.",
            build: |_, cx| stateless(splits, cx),
        },
        Section {
            id: "drag-and-drop",
            name: "Drag and drop",
            summary: "With the prefix armed every pane becomes a handle. While a pane is in flight its source recedes, the drop target shows where it lands, and a chip follows the pointer.",
            build: |_, cx| stateless(drag_and_drop, cx),
        },
        Section {
            id: "floating",
            name: "Floating surface",
            summary: "FloatingSurface hosts display-popup and command output over the panes: bordered with a title in the frame, borderless, and recolored from a popup style.",
            build: |_, cx| stateless(floating, cx),
        },
        Section {
            id: "picker",
            name: "New pane picker",
            summary: "A freshly split pane asks what it should become. pane_picker_row in its selected, resting and read-only states.",
            build: |_, cx| stateless(picker, cx),
        },
        Section {
            id: "preview",
            name: "Settings preview",
            summary: "PanesPreview, the three-pane sample on the Panes settings page, at the app defaults with gaps on and off. Click a pane to move focus.",
            build: |_, cx| stateless(preview, cx),
        },
    ],
};

fn sized(id: &'static str, height: f32, content: impl IntoElement) -> impl IntoElement {
    div()
        .id(id)
        .relative()
        .flex()
        .w_full()
        .h(px(height))
        .child(content)
}

fn frames(_: &mut Window, cx: &mut App) -> AnyElement {
    let mut grid = states().columns(3);
    for gaps in [true, false] {
        let suffix = if gaps { "gaps" } else { "no gaps" };
        grid = grid
            .state(
                format!("active terminal · {suffix}"),
                sized(
                    if gaps { "frame-a-gaps" } else { "frame-a" },
                    190.0,
                    TerminalPane::new(1, "cargo test", BUILD)
                        .active(true)
                        .gaps(gaps)
                        .render("pane", Vec::new(), cx),
                ),
            )
            .state(
                format!("inactive terminal · {suffix}"),
                sized(
                    if gaps { "frame-b-gaps" } else { "frame-b" },
                    190.0,
                    TerminalPane::new(2, "tail -f daemon.log", LOGS)
                        .gaps(gaps)
                        .render("pane", Vec::new(), cx),
                ),
            )
            .state(
                format!("inactive agent, scrim · {suffix}"),
                sized(
                    if gaps { "frame-c-gaps" } else { "frame-c" },
                    190.0,
                    placeholder_pane(
                        "pane",
                        IconName::Claude,
                        "Review pane titles",
                        "Agent pane content",
                        false,
                        gaps,
                        Vec::new(),
                        cx,
                    ),
                ),
            );
    }
    grid.into_any_element()
}

fn header_box(
    id: &'static str,
    width: Option<f32>,
    header: impl IntoElement,
    cx: &App,
) -> AnyElement {
    div()
        .id(id)
        .when_some(width, |this, width| this.w(px(width)))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .bg(pane_background(cx))
        .overflow_hidden()
        .child(header)
        .into_any_element()
}

fn header(
    id: u64,
    title: &'static str,
    active: bool,
    draggable: bool,
    cx: &App,
) -> zpui::Stateful<zpui::Div> {
    zz_ui::pane::terminal_pane_header(
        active,
        title,
        zz_ui::pane::pane_drag_button(
            ("header-drag", id),
            zz_protocol::PaneId(id),
            title.into(),
            draggable,
            |_, _, _| {},
            cx,
        ),
        |_, _, _| {},
        cx,
    )
}

fn terminal_header(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "active",
            header_box(
                "header-active",
                None,
                header(1, "cargo test -p zz-ui", true, true, cx),
                cx,
            ),
        )
        .state(
            "inactive (actions appear on hover)",
            header_box(
                "header-inactive",
                None,
                div().opacity(INACTIVE_OPACITY).child(header(
                    2,
                    "tail -f daemon.log",
                    false,
                    true,
                    cx,
                )),
                cx,
            ),
        )
        .state(
            "drag disabled: the only pane in its window, or a read-only client",
            header_box("header-solo", None, header(3, "htop", true, false, cx), cx),
        )
        .state(
            "long title at 320px",
            header_box(
                "header-long",
                Some(320.0),
                header(
                    4,
                    "ssh devbox -t 'cd ~/src/zz && cargo build --release --features agent'",
                    true,
                    true,
                    cx,
                ),
                cx,
            ),
        )
        .into_any_element()
}

fn tags(_: &mut Window, cx: &mut App) -> AnyElement {
    let top = vec![
        terminal_mode_indicator(Some("Copy mode"), "12 / 340", cx).into_any_element(),
        pane_sync_badge(cx).into_any_element(),
        pane_unzoom_control().into_any_element(),
    ];
    let bottom = vec![
        terminal_link_popup("https://zed.dev/docs/key-bindings", cx).into_any_element(),
        terminal_status_popup("Copied 3 lines", cx).into_any_element(),
    ];
    let composed = TerminalPane::new(1, "cargo test", BUILD)
        .active(true)
        .gaps(true)
        .render(
            "pane",
            vec![
                pane_overlay_stack(PaneOverlayCorner::TopRight, top)
                    .top(px(TERMINAL_HEADER_HEIGHT + 8.0))
                    .into_any_element(),
                pane_overlay_stack(PaneOverlayCorner::BottomRight, bottom).into_any_element(),
            ],
            cx,
        );
    states()
        .state(
            "pane status",
            row()
                .child(pane_status_badge(IconName::CircleX, "Dead · exit 1", cx))
                .child(pane_waiting_state("Waiting for %3", cx))
                .child(pane_sync_badge(cx))
                .child(pane_unzoom_control()),
        )
        .state(
            "terminal modes",
            row()
                .child(terminal_mode_indicator(Some("Copy mode"), "12 / 340", cx))
                .child(terminal_mode_indicator(Some("Copy mode"), "", cx))
                .child(terminal_mode_indicator(Some("View mode"), "3 / 120", cx))
                .child(terminal_mode_indicator(None::<&str>, "+42 output", cx)),
        )
        .state(
            "search, status and link previews",
            row()
                .child(terminal_search_prompt("/cargo test", 11, |_, _, _| {}, cx))
                .child(terminal_status_popup("No matches for \"panic\"", cx))
                .child(terminal_link_popup(
                    "https://github.com/demfabris/zz/pull/412",
                    cx,
                )),
        )
        .state(
            "stacked in a terminal: modes top-right below the header, previews bottom-right",
            sized("tags-composed", 230.0, composed),
        )
        .into_any_element()
}

fn indicator_key(key: &str) -> AnyElement {
    Kbd::new(Keystroke::parse(key).expect("static display-panes key")).into_any_element()
}

fn display_panes(_: &mut Window, cx: &mut App) -> AnyElement {
    let mono = cx.theme().mono_font_family.clone();
    let card = |id: &'static str, index: &'static str, key: AnyElement, active: bool| {
        pane_indicator_card(id, index, key, active, mono.clone(), cx)
    };
    let pane = |index: usize, active: bool, cx: &App| {
        let key = index.to_string();
        let overlay = pane_indicator_overlay(pane_indicator_card(
            ("indicator", index),
            key.clone(),
            indicator_key(&key),
            active,
            cx.theme().mono_font_family.clone(),
            cx,
        ))
        .into_any_element();
        let (title, lines) = [
            ("cargo test", BUILD),
            ("daemon.log", LOGS),
            ("just site", SERVER),
        ][index];
        TerminalPane::new(index as u64 + 1, title, lines)
            .active(active)
            .render(("display-pane", index), vec![overlay], cx)
    };
    let layout = pane_split_surface(
        "display-split",
        PaneSplitAxis::Horizontal,
        0.5,
        false,
        false,
        px(0.0),
        None,
        Some(PaneSplitHighlight::new(
            0.0,
            1.0,
            PaneSplitSide::First,
            cx.theme().accent,
        )),
        pane(0, true, cx),
        pane_split_surface(
            "display-split-right",
            PaneSplitAxis::Vertical,
            0.5,
            false,
            false,
            px(0.0),
            None,
            None,
            pane(1, false, cx),
            pane(2, false, cx),
            div().absolute(),
            cx,
        ),
        div().absolute(),
        cx,
    );
    states()
        .state(
            "cards",
            row()
                .child(card("card-active", "0", indicator_key("0"), true))
                .child(card("card-1", "1", indicator_key("1"), false))
                .child(card("card-2", "2", indicator_key("2"), false))
                .child(card(
                    "card-click",
                    "12",
                    div()
                        .text_xs()
                        .text_color(cx.theme().foreground.muted())
                        .child("click")
                        .into_any_element(),
                    false,
                )),
        )
        .state("over a window", sized("display-layout", 300.0, layout))
        .into_any_element()
}

fn split_pane(
    id: &'static str,
    label: &'static str,
    active: bool,
    gaps: bool,
    cx: &App,
) -> AnyElement {
    placeholder_pane(
        id,
        if active {
            IconName::SquareTerminal
        } else {
            IconName::Globe
        },
        label,
        if active { "active" } else { "inactive" },
        active,
        gaps,
        Vec::new(),
        cx,
    )
    .into_any_element()
}

fn split(
    id: &'static str,
    axis: PaneSplitAxis,
    gaps: bool,
    resizing: bool,
    side: PaneSplitSide,
    cx: &App,
) -> impl IntoElement {
    let first_active = side == PaneSplitSide::First;
    pane_split_surface(
        id,
        axis,
        0.5,
        resizing,
        gaps,
        gap(gaps),
        None,
        Some(PaneSplitHighlight::new(0.0, 1.0, side, cx.theme().accent)),
        split_pane("first", "First", first_active, gaps, cx),
        split_pane("second", "Second", !first_active, gaps, cx),
        div().absolute(),
        cx,
    )
}

fn nested(id: &'static str, gaps: bool, cx: &App) -> impl IntoElement {
    let right = pane_split_surface(
        "nested-right",
        PaneSplitAxis::Vertical,
        0.5,
        false,
        gaps,
        gap(gaps),
        None,
        Some(PaneSplitHighlight::new(
            0.0,
            1.0,
            PaneSplitSide::Second,
            cx.theme().accent,
        )),
        split_pane("top", "Top", false, gaps, cx),
        split_pane("bottom", "Bottom", true, gaps, cx),
        div().absolute(),
        cx,
    );
    div()
        .size_full()
        .when(gaps, |this| this.p(gap(gaps)))
        .child(pane_split_surface(
            id,
            PaneSplitAxis::Horizontal,
            0.55,
            false,
            gaps,
            gap(gaps),
            None,
            Some(PaneSplitHighlight::new(
                0.5,
                0.5,
                PaneSplitSide::Second,
                cx.theme().accent,
            )),
            split_pane("left", "Left", false, gaps, cx),
            right,
            div().absolute(),
            cx,
        ))
}

fn splits(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .columns(2)
        .state(
            "horizontal · no gaps · left active",
            sized(
                "split-h",
                170.0,
                split(
                    "s",
                    PaneSplitAxis::Horizontal,
                    false,
                    false,
                    PaneSplitSide::First,
                    cx,
                ),
            ),
        )
        .state(
            "vertical · no gaps · bottom active",
            sized(
                "split-v",
                170.0,
                split(
                    "s",
                    PaneSplitAxis::Vertical,
                    false,
                    false,
                    PaneSplitSide::Second,
                    cx,
                ),
            ),
        )
        .state(
            "resizing: the hairline washes and drops the highlight",
            sized(
                "split-resizing",
                170.0,
                split(
                    "s",
                    PaneSplitAxis::Horizontal,
                    false,
                    true,
                    PaneSplitSide::First,
                    cx,
                ),
            ),
        )
        .state(
            "horizontal · 6px gaps",
            sized(
                "split-gaps",
                170.0,
                split(
                    "s",
                    PaneSplitAxis::Horizontal,
                    true,
                    false,
                    PaneSplitSide::First,
                    cx,
                ),
            ),
        )
        .state(
            "nested · no gaps: the highlight covers only the active pane's span",
            sized("split-nested", 220.0, nested("s", false, cx)),
        )
        .state(
            "nested · gaps",
            sized("split-nested-gaps", 220.0, nested("s", true, cx)),
        )
        .into_any_element()
}

fn drag_and_drop(_: &mut Window, cx: &mut App) -> AnyElement {
    let armed = |index: u64, active: bool, cx: &App| {
        let (title, lines) = if active {
            ("cargo test", BUILD)
        } else {
            ("daemon.log", LOGS)
        };
        TerminalPane::new(index, title, lines)
            .active(active)
            .gaps(true)
            .render(
                ("armed", index),
                vec![
                    pane_drag_overlay(
                        ("armed-overlay", index),
                        PaneDragOverlayState::Armed,
                        radii(true),
                        cx,
                    )
                    .into_any_element(),
                ],
                cx,
            )
    };
    let armed_layout = div()
        .size_full()
        .p(px(PANE_MARGIN))
        .child(pane_split_surface(
            "armed-split",
            PaneSplitAxis::Horizontal,
            0.5,
            false,
            true,
            px(PANE_MARGIN),
            None,
            None,
            armed(1, true, cx),
            armed(2, false, cx),
            div().absolute(),
            cx,
        ));
    let source = TerminalPane::new(1, "cargo test", BUILD)
        .active(true)
        .gaps(true)
        .render(
            "source",
            vec![
                pane_drag_overlay(
                    "source-overlay",
                    PaneDragOverlayState::Source,
                    radii(true),
                    cx,
                )
                .into_any_element(),
            ],
            cx,
        );
    let target = TerminalPane::new(2, "daemon.log", LOGS).gaps(true).render(
        "target",
        vec![
            pane_drag_overlay(
                "target-overlay",
                PaneDragOverlayState::Armed,
                radii(true),
                cx,
            )
            .into_any_element(),
        ],
        cx,
    );
    let dragging = div()
        .relative()
        .size_full()
        .p(px(PANE_MARGIN))
        .child(pane_split_surface(
            "dragging-split",
            PaneSplitAxis::Horizontal,
            0.5,
            false,
            true,
            px(PANE_MARGIN),
            None,
            None,
            source,
            target,
            div().absolute(),
            cx,
        ))
        .child(
            pane_drop_preview(px(PANE_RADIUS), px(PANE_BORDER.max(1.0)), cx)
                .top(px(PANE_MARGIN))
                .bottom(px(PANE_MARGIN))
                .right(px(PANE_MARGIN))
                .w(relative(0.24)),
        )
        .child(
            div()
                .absolute()
                .left(relative(0.62))
                .top(px(96.0))
                .child(pane_drag_chip("%1", "cargo test", cx)),
        );
    states()
        .state(
            "prefix armed: every pane is a handle",
            sized("dnd-armed", 200.0, armed_layout),
        )
        .state(
            "dragging %1 onto the right edge of %2",
            sized("dnd-dragging", 200.0, dragging),
        )
        .state(
            "drag chips",
            row()
                .child(pane_drag_chip("%1", "cargo test", cx))
                .child(pane_drag_chip(
                    "%12",
                    "ssh devbox -t 'cd ~/src/zz && cargo build --release --features agent'",
                    cx,
                )),
        )
        .state(
            "drop previews at the pane radius and at zero radius",
            row()
                .child(
                    div()
                        .relative()
                        .w(px(160.0))
                        .h(px(90.0))
                        .child(pane_drop_preview(px(PANE_RADIUS), px(1.0), cx).inset_0()),
                )
                .child(
                    div()
                        .relative()
                        .w(px(160.0))
                        .h(px(90.0))
                        .child(pane_drop_preview(px(0.0), px(1.0), cx).inset_0()),
                ),
        )
        .into_any_element()
}

fn floating_box(id: &'static str, surface: impl IntoElement) -> impl IntoElement {
    div().id(id).w(px(380.0)).h(px(190.0)).child(surface)
}

fn floating(_: &mut Window, cx: &mut App) -> AnyElement {
    let body = |cx: &App| terminal_body(SERVER, false, cx).bg(zpui::transparent_black());
    states()
        .columns(2)
        .state(
            "bordered, titled",
            floating_box(
                "floating-titled",
                FloatingSurface::new("popup", body(cx), cx)
                    .title("display-popup · just site")
                    .content_inset(px(8.0), px(14.0)),
            ),
        )
        .state(
            "borderless",
            floating_box(
                "floating-borderless",
                FloatingSurface::new("popup", body(cx), cx)
                    .bordered(false)
                    .content_inset(px(0.0), px(6.0)),
            ),
        )
        .state(
            "colored by the popup style",
            floating_box(
                "floating-styled",
                FloatingSurface::new("popup", body(cx), cx)
                    .title("lazygit")
                    .content_inset(px(8.0), px(14.0))
                    .colors(
                        cx.theme().background.raised(2).opaque(),
                        cx.theme().foreground,
                        cx.theme().accent,
                    ),
            ),
        )
        .state(
            "long title clipped by the frame",
            floating_box(
                "floating-long",
                FloatingSurface::new("popup", body(cx), cx)
                    .title("display-popup -E 'cargo watch -x \"test -p zz-daemon --all-features\"'")
                    .content_inset(px(8.0), px(14.0)),
            ),
        )
        .into_any_element()
}

const CHOICES: [(&str, IconName, &str); 3] = [
    ("Terminal", IconName::SquareTerminal, "t"),
    ("Agent", IconName::RobotFace, "a"),
    ("Browser", IconName::Globe, "b"),
];

fn picker_pane(id: &'static str, selected: usize, enabled: bool, cx: &App) -> impl IntoElement {
    let rows = CHOICES
        .iter()
        .enumerate()
        .map(|(index, (title, icon, shortcut))| {
            pane_picker_row(
                ("choice", index),
                title,
                icon.clone(),
                shortcut,
                selected == index,
                enabled,
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();
    sized(
        id,
        220.0,
        zz_ui::pane::pane_surface(
            "pane",
            div()
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .px(px(12.0))
                .rounded(px(PANE_RADIUS))
                .bg(pane_background(cx))
                .text_color(cx.theme().foreground)
                .child(pane_picker_choices(rows)),
            Vec::new(),
            super::fixtures::chrome(true, false, true, cx),
            cx,
        ),
    )
}

fn picker(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .columns(2)
        .state(
            "terminal selected",
            picker_pane("picker-terminal", 0, true, cx),
        )
        .state("agent selected", picker_pane("picker-agent", 1, true, cx))
        .state(
            "read-only client: rows dim and stop taking clicks",
            picker_pane("picker-readonly", 0, false, cx),
        )
        .state(
            "rows at full width",
            div()
                .w_full()
                .child(pane_picker_choices(CHOICES.iter().enumerate().map(
                    |(index, (title, icon, shortcut))| {
                        pane_picker_row(
                            ("bare-choice", index),
                            title,
                            icon.clone(),
                            shortcut,
                            index == 2,
                            true,
                            cx,
                        )
                        .into_any_element()
                    },
                ))),
        )
        .into_any_element()
}

fn preview(_: &mut Window, _: &mut App) -> AnyElement {
    let preview = |gaps: bool| PanesPreview {
        gaps,
        margin: PANE_MARGIN,
        radius: PANE_RADIUS,
        border_width: PANE_BORDER,
        inactive_opacity: INACTIVE_OPACITY,
    };
    states()
        .state(
            "gaps on: 6px margin, 13.5px radius, 0.5px border",
            div().id("preview-gaps").child(preview(true)),
        )
        .state(
            "gaps off, the app default",
            div().id("preview-flush").child(preview(false)),
        )
        .state(
            "wide margin and thick border",
            div().id("preview-wide").child(PanesPreview {
                gaps: true,
                margin: 16.0,
                radius: 20.0,
                border_width: 3.0,
                inactive_opacity: 0.4,
            }),
        )
        .into_any_element()
}
