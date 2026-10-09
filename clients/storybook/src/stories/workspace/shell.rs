use zpui::{
    AnyElement, App, AppContext as _, Entity, IntoElement, ParentElement as _, Styled as _,
    UniformListScrollHandle, Window, div, prelude::*, px,
};
use zz_ui::{
    ActiveTheme as _, IconName,
    input::InputState,
    pane::{PaneSplitAxis, PaneSplitHighlight, PaneSplitSide, pane_split_surface},
    shell::{app_connection_state, app_shell_surface, app_titlebar_strip, app_workspace_surface},
};

use super::{
    browser::browser_pane,
    fixtures::{BUILD, TerminalPane, gap, placeholder_pane, stateful},
    status_bar::{Bar, titlebar},
    tree::{TreeState, chrome_controls, host_only, sidebar, tree_list, workspace_tree},
};
use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "shell",
    name: "App shell",
    group: "Workspace",
    summary: "The pieces put together the way the thin client does: sidebar or status bar, the pane canvas with its splits, and the connection states.",
    sections: &[
        Section {
            id: "app",
            name: "App",
            summary: "app_shell_surface with a terminal, an agent and a browser pane. With the sidebar open the status bar goes away; with it hidden the status bar takes the titlebar and the canvas drops its top margin.",
            build: |window, cx| stateful(window, cx, Shell::new, app),
        },
        Section {
            id: "connection",
            name: "Connection",
            summary: "Before the first snapshot arrives the sidebar shows only the host, with a spinner or a warning, and the workspace shows app_connection_state.",
            build: |window, cx| stateful(window, cx, Shell::new, connection),
        },
        Section {
            id: "titlebar-strip",
            name: "Titlebar strip",
            summary: "app_titlebar_strip: a titlebar-height drag region with controls at its trailing end, mounted where zz draws its own window controls.",
            build: |_, cx| stateless(titlebar_strip, cx),
        },
    ],
};

struct Shell {
    scrolls: [UniformListScrollHandle; 2],
    addresses: [Entity<InputState>; 2],
}

impl Shell {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            scrolls: std::array::from_fn(|_| UniformListScrollHandle::new()),
            addresses: std::array::from_fn(|_| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Search or enter address")
                        .default_value("https://zed.dev/docs/key-bindings")
                })
            }),
        }
    }
}

fn canvas(
    gaps: bool,
    inline_sidebar: bool,
    address: &Entity<InputState>,
    cx: &App,
) -> impl IntoElement {
    let terminal = TerminalPane::new(1, "cargo test", BUILD)
        .active(true)
        .gaps(gaps)
        .render("shell-terminal", Vec::new(), cx);
    let agent = placeholder_pane(
        "shell-agent",
        IconName::Claude,
        "Review sidebar",
        "Agent pane content",
        false,
        gaps,
        Vec::new(),
        cx,
    );
    let browser = browser_pane("shell-browser", address, false, gaps, cx);
    let right = pane_split_surface(
        "shell-right",
        PaneSplitAxis::Vertical,
        0.4,
        false,
        gaps,
        gap(gaps),
        None,
        None,
        agent,
        browser,
        div().absolute(),
        cx,
    );
    let layout = pane_split_surface(
        "shell-split",
        PaneSplitAxis::Horizontal,
        0.5,
        false,
        gaps,
        gap(gaps),
        None,
        Some(PaneSplitHighlight::new(
            0.0,
            1.0,
            PaneSplitSide::First,
            cx.theme().accent,
        )),
        terminal,
        right,
        div().absolute(),
        cx,
    );
    div().id("shell-canvas").relative().size_full().child(
        div()
            .size_full()
            .p(gap(gaps))
            .when(!inline_sidebar, zpui::Styled::pt_0)
            .child(layout),
    )
}

fn window_frame(id: &'static str, height: f32, shell: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .id(id)
        .w_full()
        .h(px(height))
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .child(shell)
        .into_any_element()
}

fn with_sidebar(gaps: bool, state: &Shell, index: usize, cx: &App) -> AnyElement {
    let sidebar = sidebar(
        tree_list(
            workspace_tree(),
            TreeState::CONNECTED,
            &state.scrolls[index],
            cx,
        ),
        false,
        cx,
    )
    .when(gaps, |surface| {
        surface.border_color(zpui::transparent_black())
    });
    app_shell_surface(
        "shell",
        cx.theme().background,
        sidebar,
        None,
        app_workspace_surface(
            "workspace",
            canvas(gaps, true, &state.addresses[index], cx),
            Vec::new(),
            cx,
        ),
        Vec::new(),
    )
    .into_any_element()
}

fn with_status_bar(gaps: bool, state: &Shell, index: usize, cx: &App) -> AnyElement {
    app_shell_surface(
        "shell",
        cx.theme().background,
        div(),
        Some(titlebar(&Bar { gaps, ..Bar::FULL }, cx).into_any_element()),
        app_workspace_surface(
            "workspace",
            canvas(gaps, false, &state.addresses[index], cx),
            Vec::new(),
            cx,
        ),
        Vec::new(),
    )
    .into_any_element()
}

fn app(state: &Shell, _: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "sidebar open, no gaps (the default)",
            window_frame(
                "shell-sidebar",
                540.0,
                with_sidebar(false, state, 0, cx),
                cx,
            ),
        )
        .state(
            "sidebar hidden, pane gaps on",
            window_frame(
                "shell-status",
                540.0,
                with_status_bar(true, state, 1, cx),
                cx,
            ),
        )
        .into_any_element()
}

fn connecting_shell(
    connecting: bool,
    message: &'static str,
    state: &Shell,
    index: usize,
    cx: &App,
) -> AnyElement {
    let tree = tree_list(
        host_only(),
        TreeState {
            connected: false,
            connecting,
            ..TreeState::CONNECTED
        },
        &state.scrolls[index],
        cx,
    );
    app_shell_surface(
        "shell",
        cx.theme().background,
        sidebar(tree, false, cx),
        None,
        app_workspace_surface(
            "workspace",
            app_connection_state(message, cx),
            Vec::new(),
            cx,
        ),
        Vec::new(),
    )
    .into_any_element()
}

fn connection(state: &Shell, _: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "connecting",
            window_frame(
                "shell-connecting",
                260.0,
                connecting_shell(true, "Connecting to devbox…", state, 0, cx),
                cx,
            ),
        )
        .state(
            "connection lost",
            window_frame(
                "shell-lost",
                260.0,
                connecting_shell(false, "Connection lost. Reconnecting in 4s…", state, 1, cx),
                cx,
            ),
        )
        .into_any_element()
}

fn titlebar_strip(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "controls at the trailing end",
            div()
                .id("strip")
                .w_full()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border())
                .child(app_titlebar_strip(
                    "titlebar-strip",
                    chrome_controls(false).pr(px(8.0)),
                )),
        )
        .into_any_element()
}
