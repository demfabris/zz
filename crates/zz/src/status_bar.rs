use std::ops::Range;

use gpui::{
    AnyElement, App, Entity, IntoElement, MouseButton, Pixels, SharedString, Stateful, Window,
    WindowControlArea, div, prelude::*, px,
};
use zz_mux::parse_styled_segments;
use zz_protocol::{MuxSnapshot, SessionId, StatusLine, TmuxAlign, TmuxList, WindowId};
use zz_ui::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, TITLE_BAR_HEIGHT,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    navigation::{
        WORKSPACE_STATUS_CONTENT_HEIGHT, WorkspaceStatusWindowState,
        workspace_controls_leading_inset, workspace_status_item, workspace_status_window,
    },
};

use crate::{
    mux::{
        client::MuxClient,
        nav::{
            MuxTreeModel, TreeNode, TreeTarget, activate_nav, kill_target_command,
            select_window_command,
        },
    },
    theme::chrome_background,
};

const TITLEBAR_CONTROLS_GAP: Pixels = px(6.0);
const STATUS_GAP: Pixels = px(2.0);
const MAX_VISIBLE_WINDOWS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuiStatusPlacement {
    Bottom,
    Titlebar,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum NativeStatusAlignment {
    #[default]
    Left,
    Centre,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeStatusWindow {
    id: WindowId,
    index: u32,
    name: String,
    active: bool,
    zoomed: bool,
    bell: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeStatusChunk {
    icon: Option<IconName>,
    text: String,
}

pub(crate) fn render_gui_status_bar(
    placement: GuiStatusPlacement,
    status: &StatusLine,
    mux: &Entity<MuxClient>,
    titlebar_controls: Option<(AnyElement, Pixels)>,
    window_controls: Option<AnyElement>,
    _window: &mut Window,
    cx: &mut App,
) -> Stateful<gpui::Div> {
    let background = chrome_background(cx);
    let foreground = cx.theme().foreground;
    let status_enabled = !status.is_empty();
    let (snapshot, attached_host, attached, connected) = {
        let mux = mux.read(cx);
        (
            mux.snapshot(),
            mux.attached_host(),
            mux.attached_session(),
            mux.is_connected(),
        )
    };
    let model = MuxTreeModel::from_mux(mux.read(cx));
    let windows = if status_enabled {
        native_status_windows(&snapshot, attached)
    } else {
        Vec::new()
    };
    let alignment = native_status_alignment(status);
    let active_index = windows.iter().position(|window| window.active).unwrap_or(0);
    let visible_range = visible_window_range(windows.len(), active_index, MAX_VISIBLE_WINDOWS);
    let visible_windows = windows[visible_range]
        .iter()
        .map(|window| render_status_window(window, connected, attached_host, &model, mux, cx))
        .collect::<Vec<_>>();
    let overflow = (windows.len() > MAX_VISIBLE_WINDOWS)
        .then(|| render_window_overflow(&windows, connected, mux, cx));
    let left = status_enabled
        .then(|| render_status_chunks("gui-status-left", &status.left, cx))
        .flatten();
    let right = status_enabled
        .then(|| render_status_chunks("gui-status-right", &status.right, cx))
        .flatten();
    let window_strip = div()
        .flex()
        .flex_1()
        .min_w_0()
        .h(WORKSPACE_STATUS_CONTENT_HEIGHT)
        .items_center()
        .gap(STATUS_GAP)
        .overflow_hidden()
        .when(alignment == NativeStatusAlignment::Centre, |strip| {
            strip.justify_center()
        })
        .when(alignment == NativeStatusAlignment::Right, |strip| {
            strip.justify_end()
        })
        .children(visible_windows)
        .children(overflow);
    let content = div()
        .flex()
        .flex_1()
        .min_w_0()
        .h(TITLE_BAR_HEIGHT)
        .items_center()
        .gap(px(6.0))
        .px(px(6.0))
        .when(placement == GuiStatusPlacement::Titlebar, |content| {
            content.window_control_area(WindowControlArea::Drag)
        })
        .children(left)
        .child(window_strip)
        .children(right);
    let leading = (placement == GuiStatusPlacement::Titlebar).then(|| {
        div()
            .flex_none()
            .w(workspace_controls_leading_inset(cx))
            .h(TITLE_BAR_HEIGHT)
            .window_control_area(WindowControlArea::Drag)
    });
    let titlebar_controls = titlebar_controls.map(|(controls, width)| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .w(width + TITLEBAR_CONTROLS_GAP)
            .h(TITLE_BAR_HEIGHT)
            .pr(TITLEBAR_CONTROLS_GAP)
            .child(controls)
            .into_any_element()
    });
    let window_controls = window_controls.map(|controls| {
        div()
            .flex_none()
            .h(TITLE_BAR_HEIGHT)
            .child(controls)
            .into_any_element()
    });

    div()
        .id(match placement {
            GuiStatusPlacement::Bottom => "gui-status-bottom",
            GuiStatusPlacement::Titlebar => "gui-status-titlebar",
        })
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .w_full()
        .h(TITLE_BAR_HEIGHT)
        .overflow_hidden()
        .bg(background)
        .text_color(foreground)
        .when(placement == GuiStatusPlacement::Bottom, |bar| {
            bar.border_t_1().border_color(cx.theme().border)
        })
        .when(placement == GuiStatusPlacement::Titlebar, |bar| {
            bar.border_b_1().border_color(cx.theme().border)
        })
        .children(leading)
        .children(titlebar_controls)
        .child(content)
        .children(window_controls)
}

fn render_status_chunks(id: &'static str, value: &str, cx: &App) -> Option<AnyElement> {
    let chunks = native_status_chunks(value);
    if chunks.is_empty() {
        return None;
    }
    Some(
        div()
            .flex()
            .flex_shrink_1()
            .min_w_0()
            .h(WORKSPACE_STATUS_CONTENT_HEIGHT)
            .items_center()
            .gap(px(10.0))
            .overflow_hidden()
            .children(chunks.into_iter().enumerate().map(|(index, chunk)| {
                workspace_status_item((id, index), chunk.icon, chunk.text.into(), cx)
            }))
            .into_any_element(),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_status_window(
    window: &NativeStatusWindow,
    connected: bool,
    attached_host: crate::mux::hosts::HostId,
    model: &MuxTreeModel,
    mux: &Entity<MuxClient>,
    cx: &App,
) -> AnyElement {
    let tooltip: SharedString = format!("{}:{}", window.index, window.name).into();
    let id = window.id;
    let select_mux = mux.clone();
    let rename = model.rename_activation_for_node(
        TreeNode::Target(attached_host, TreeTarget::Window(id)),
        attached_host,
    );
    let rename_mux = mux.clone();
    let close_mux = mux.clone();
    let item = workspace_status_window(
        ("gui-status-window", id.0),
        window.index.to_string().into(),
        window.name.clone().into(),
        tooltip,
        WorkspaceStatusWindowState {
            connected,
            active: window.active,
            zoomed: window.zoomed,
            bell: window.bell,
        },
        cx,
    )
    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        if connected {
            select_mux.read(cx).execute(select_window_command(id));
        }
    });
    if !connected {
        return item.into_any_element();
    }
    item.context_menu(move |menu, _, _| {
        let menu = if let Some((label, activation)) = rename.clone() {
            let rename_mux = rename_mux.clone();
            menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                activate_nav(&rename_mux, activation.clone(), cx);
            }))
        } else {
            menu
        };
        let close_mux = close_mux.clone();
        menu.item(
            PopupMenuItem::new("Close Window")
                .icon(IconName::Xmark)
                .on_click(move |_, _, cx| {
                    close_mux
                        .read(cx)
                        .execute(kill_target_command(TreeTarget::Window(id)));
                }),
        )
    })
    .into_any_element()
}

fn render_window_overflow(
    windows: &[NativeStatusWindow],
    connected: bool,
    mux: &Entity<MuxClient>,
    cx: &App,
) -> AnyElement {
    let windows = windows.to_vec();
    let menu_mux = mux.clone();
    Button::new("gui-status-window-overflow")
        .ghost()
        .xsmall()
        .compact()
        .icon(IconName::Ellipsis)
        .hover_bg(zz_ui::navigation::workspace_row_highlight(cx))
        .tooltip("All windows")
        .disabled(!connected)
        .dropdown_menu(move |menu, _, _| {
            windows.iter().fold(menu, |menu, window| {
                let id = window.id;
                let select_mux = menu_mux.clone();
                menu.item(
                    PopupMenuItem::new(native_window_label(window))
                        .icon(if window.active {
                            IconName::Check
                        } else {
                            IconName::AppWindow
                        })
                        .on_click(move |_, _, cx| {
                            select_mux.read(cx).execute(select_window_command(id));
                        }),
                )
            })
        })
        .into_any_element()
}

fn native_status_windows(
    snapshot: &MuxSnapshot,
    attached: Option<SessionId>,
) -> Vec<NativeStatusWindow> {
    let Some(session) = snapshot
        .sessions
        .iter()
        .find(|session| Some(session.id) == attached)
    else {
        return Vec::new();
    };
    let focused_window = snapshot.focused_window_for(session);
    session
        .windows
        .iter()
        .map(|window| NativeStatusWindow {
            id: window.id,
            index: window.index,
            name: window.name.clone(),
            active: window.id == focused_window,
            zoomed: window.zoomed_pane.is_some(),
            bell: window.panes.values().any(|pane| pane.bell),
        })
        .collect()
}

fn native_status_alignment(status: &StatusLine) -> NativeStatusAlignment {
    status
        .rows
        .iter()
        .flat_map(|row| parse_styled_segments(row))
        .find_map(|segment| {
            (segment.style.list == Some(TmuxList::On))
                .then_some(segment.style.align)
                .flatten()
        })
        .map_or(NativeStatusAlignment::Left, |align| match align {
            TmuxAlign::Centre | TmuxAlign::AbsoluteCentre => NativeStatusAlignment::Centre,
            TmuxAlign::Right => NativeStatusAlignment::Right,
            TmuxAlign::Default | TmuxAlign::Left => NativeStatusAlignment::Left,
        })
}

fn visible_window_range(total: usize, active: usize, limit: usize) -> Range<usize> {
    if total <= limit || limit == 0 {
        return 0..total;
    }
    let mut start = active.saturating_sub(limit / 2);
    start = start.min(total - limit);
    start..start + limit
}

fn native_status_chunks(value: &str) -> Vec<NativeStatusChunk> {
    let text = parse_styled_segments(value)
        .into_iter()
        .map(|segment| segment.text)
        .collect::<String>();
    text.split(is_powerline_separator)
        .filter_map(native_status_chunk)
        .collect()
}

fn native_status_chunk(value: &str) -> Option<NativeStatusChunk> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let first = chars.next()?;
    if let Some(icon) = native_status_icon(first) {
        return Some(NativeStatusChunk {
            icon: Some(icon),
            text: chars.as_str().trim_start().to_owned(),
        });
    }
    Some(NativeStatusChunk {
        icon: None,
        text: normalized,
    })
}

fn native_status_icon(value: char) -> Option<IconName> {
    match value {
        '\u{f120}' => Some(IconName::SquareTerminal),
        '\u{e0a0}' => Some(IconName::GitBranch),
        '\u{f017}' => Some(IconName::Clock),
        '\u{f073}' => Some(IconName::Calendar),
        '\u{f00e}' => Some(IconName::ZoomIn),
        _ => None,
    }
}

fn is_powerline_separator(value: char) -> bool {
    ('\u{e0b0}'..='\u{e0d4}').contains(&value)
}

fn native_window_label(window: &NativeStatusWindow) -> SharedString {
    format!("{} {}", window.index, window.name).into()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use zz_protocol::{LayoutNode, PaneId, SessionSnapshot, WindowSnapshot};

    fn window(id: u64, active_pane: u64) -> WindowSnapshot {
        WindowSnapshot {
            id: WindowId(id),
            index: u32::try_from(id).expect("fixture window index"),
            name: format!("window-{id}"),
            automatic_rename: true,
            active_pane: PaneId(active_pane),
            zoomed_pane: None,
            layout: LayoutNode::Pane(PaneId(active_pane)),
            panes: BTreeMap::new(),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
            status_label: format!("{id}:window-{id}"),
        }
    }

    #[test]
    fn native_windows_follow_the_attached_session_and_focused_window() {
        let mut second = window(2, 12);
        second.zoomed_pane = Some(PaneId(12));
        let snapshot = MuxSnapshot {
            generation: 1,
            sessions: vec![SessionSnapshot {
                id: SessionId(4),
                name: "work".to_owned(),
                active_window: WindowId(1),
                windows: vec![window(1, 11), second],
                viewers: Vec::new(),
            }],
            focused_window: Some(WindowId(2)),
        };
        let windows = native_status_windows(&snapshot, Some(SessionId(4)));
        assert_eq!(windows.len(), 2);
        assert!(!windows[0].active);
        assert!(windows[1].active);
        assert!(windows[1].zoomed);
        assert!(native_status_windows(&snapshot, Some(SessionId(9))).is_empty());
    }

    #[test]
    fn native_chunks_ignore_tmux_styles_and_promote_known_icons() {
        let styled = native_status_chunks(
            "#[fg=black,bg=white,bold]  0 #[fg=red]#[reverse] macbook #[none] \
             #[fg=blue]  main #[fg=yellow]#[italics]  20:57 #[underscore]  23 Aug ",
        );

        assert_eq!(
            styled,
            vec![
                NativeStatusChunk {
                    icon: Some(IconName::SquareTerminal),
                    text: "0".to_owned(),
                },
                NativeStatusChunk {
                    icon: None,
                    text: "macbook".to_owned(),
                },
                NativeStatusChunk {
                    icon: Some(IconName::GitBranch),
                    text: "main".to_owned(),
                },
                NativeStatusChunk {
                    icon: Some(IconName::Clock),
                    text: "20:57".to_owned(),
                },
                NativeStatusChunk {
                    icon: Some(IconName::Calendar),
                    text: "23 Aug".to_owned(),
                },
            ]
        );
        assert_eq!(
            native_status_chunks("#[fg=black,bg=black]same"),
            native_status_chunks("#[fg=white,bg=white,reverse,bold]same")
        );
    }

    #[test]
    fn native_chunks_preserve_unknown_content() {
        assert_eq!(
            native_status_chunks("  λ   custom output  "),
            vec![NativeStatusChunk {
                icon: None,
                text: "λ custom output".to_owned(),
            }]
        );
    }

    #[test]
    fn native_alignment_reads_the_status_list_marker() {
        for (align, expected) in [
            ("left", NativeStatusAlignment::Left),
            ("centre", NativeStatusAlignment::Centre),
            ("absolute-centre", NativeStatusAlignment::Centre),
            ("right", NativeStatusAlignment::Right),
        ] {
            let status = StatusLine {
                rows: vec![format!("#[list=on align={align}]window")],
                ..StatusLine::default()
            };
            assert_eq!(native_status_alignment(&status), expected);
        }
    }

    #[test]
    fn visible_windows_stay_centered_on_the_active_window() {
        assert_eq!(visible_window_range(3, 1, 5), 0..3);
        assert_eq!(visible_window_range(9, 0, 5), 0..5);
        assert_eq!(visible_window_range(9, 4, 5), 2..7);
        assert_eq!(visible_window_range(9, 8, 5), 4..9);
    }
}
