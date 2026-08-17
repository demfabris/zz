//! The window title bar: a draggable strip that owns the top of a window.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Decorations, Div, Hsla, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, RenderOnce, Stateful, StatefulInteractiveElement as _,
    StyleRefinement, Styled, TitlebarOptions, Window, WindowControlArea, div, point,
    prelude::FluentBuilder as _, px,
};
use smallvec::SmallVec;

use crate::Colorize as _;
#[cfg(target_os = "macos")]
use crate::UiZoom;
use crate::{
    ActiveTheme as _, Icon, IconName, InteractiveElementExt as _, Sizable as _, StyledExt as _,
    h_flex,
};

/// Height of the title bar, and of every strip that has to line up with it
/// (the sidebar headers in [`crate::navigation`] and [`crate::settings`]).
pub const TITLE_BAR_HEIGHT: Pixels = px(38.);

/// Side of a native macOS window-button frame, and the width of all three
/// plus their gaps. Measured on macOS 27: each frame is 14x14, origins sit 23
/// apart, so the cluster spans 60 from the first left edge to the last right
/// edge. The platform layer centres the buttons in a container of
/// `glyph + 2 * traffic_light_position.y`, which is what ties them to
/// [`TITLE_BAR_HEIGHT`]: keep the two in step or the lights stop sharing a
/// centre line with the strip's own controls.
pub const MACOS_TRAFFIC_LIGHT_GLYPH: f32 = 14.;
pub const MACOS_TRAFFIC_LIGHT_SPAN: f32 = 60.;

/// Leading margin the macOS traffic lights keep from the window edge. The
/// strip's own controls keep the same margin from the cluster's far edge, so
/// the gap on either side of the lights reads as one measurement.
pub const MACOS_TRAFFIC_LIGHT_INSET: f32 = 14.;

/// Whether the app draws the window's minimize / maximize / close buttons:
/// Windows outside fullscreen, and Linux under [`Decorations::Client`]. macOS
/// keeps its native traffic lights, and WASM has no window to control.
#[must_use]
pub fn draws_window_controls(window: &Window) -> bool {
    (cfg!(target_os = "windows") && !window.is_fullscreen())
        || (cfg!(target_os = "linux")
            && matches!(window.window_decorations(), Decorations::Client { .. }))
}

/// Left inset that clears the native macOS traffic lights, in window points.
#[cfg(target_os = "macos")]
fn title_bar_left_padding(cx: &App) -> Pixels {
    UiZoom::unzoomed(
        px(2. * MACOS_TRAFFIC_LIGHT_INSET + MACOS_TRAFFIC_LIGHT_SPAN),
        cx,
    )
}
#[cfg(not(target_os = "macos"))]
fn title_bar_left_padding(_cx: &App) -> Pixels {
    px(12.)
}

type CloseHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The window title bar. Children are laid out in the draggable region, to the
/// left of the window controls.
#[derive(IntoElement)]
pub struct TitleBar {
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 1]>,
    on_close_window: Option<CloseHandler>,
}

impl TitleBar {
    #[must_use]
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: SmallVec::new(),
            on_close_window: None,
        }
    }

    /// The [`TitlebarOptions`] a window must open with for [`TitleBar`] to sit
    /// correctly in it: transparent, untitled, traffic lights placed to match
    /// this bar's height and left padding.
    #[must_use]
    pub fn title_bar_options() -> TitlebarOptions {
        TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(
                px(MACOS_TRAFFIC_LIGHT_INSET),
                (TITLE_BAR_HEIGHT - px(MACOS_TRAFFIC_LIGHT_GLYPH)) / 2.,
            )),
            ..TitlebarOptions::default()
        }
    }

    /// Run `f` instead of `Window::remove_window` when the close button is
    /// clicked. Linux only: elsewhere the button is absent or hit-tested by the
    /// OS, so there is no click to intercept.
    #[must_use]
    pub fn on_close_window(
        mut self,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        if cfg!(target_os = "linux") {
            self.on_close_window = Some(Rc::new(f));
        }
        self
    }
}

impl Default for TitleBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for TitleBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for TitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

#[cfg(not(target_os = "windows"))]
const CONTROL_ICON_WIDTH: Pixels = TITLE_BAR_HEIGHT;
/// The Windows 11 shell's own caption button width.
#[cfg(target_os = "windows")]
const CONTROL_ICON_WIDTH: Pixels = px(46.);

#[derive(IntoElement, Clone)]
enum ControlIcon {
    Minimize,
    Restore,
    Maximize,
    Close {
        on_close_window: Option<CloseHandler>,
    },
}

impl ControlIcon {
    fn minimize() -> Self {
        Self::Minimize
    }

    fn restore() -> Self {
        Self::Restore
    }

    fn maximize() -> Self {
        Self::Maximize
    }

    fn close(on_close_window: Option<CloseHandler>) -> Self {
        Self::Close { on_close_window }
    }

    fn id(&self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Restore => "restore",
            Self::Maximize => "maximize",
            Self::Close { .. } => "close",
        }
    }

    fn icon(&self) -> IconName {
        match self {
            Self::Minimize => IconName::WindowMinimize,
            Self::Restore => IconName::WindowRestore,
            Self::Maximize => IconName::WindowMaximize,
            Self::Close { .. } => IconName::WindowClose,
        }
    }

    fn window_control_area(&self) -> WindowControlArea {
        match self {
            Self::Minimize => WindowControlArea::Min,
            Self::Restore | Self::Maximize => WindowControlArea::Max,
            Self::Close { .. } => WindowControlArea::Close,
        }
    }

    fn is_close(&self) -> bool {
        matches!(self, Self::Close { .. })
    }

    fn hover_fg(&self, cx: &App) -> Hsla {
        if self.is_close() {
            cx.theme().danger.on()
        } else {
            cx.theme().foreground
        }
    }

    fn hover_bg(&self, cx: &App) -> Hsla {
        if self.is_close() {
            cx.theme().danger
        } else {
            cx.theme().background.raised(2).hover()
        }
    }

    fn active_bg(&self, cx: &App) -> Hsla {
        if self.is_close() {
            cx.theme().danger.active()
        } else {
            cx.theme().background.raised(2).active()
        }
    }
}

impl RenderOnce for ControlIcon {
    #[allow(
        clippy::similar_names,
        reason = "hover_fg/hover_bg is the clearest naming for a foreground/background pair"
    )]
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let is_linux = cfg!(target_os = "linux");
        let is_windows = cfg!(target_os = "windows");
        let hover_fg = self.hover_fg(cx);
        let hover_bg = self.hover_bg(cx);
        let active_bg = self.active_bg(cx);
        let icon = self.clone();
        let on_close_window = match &self {
            Self::Close { on_close_window } => on_close_window.clone(),
            _ => None,
        };

        div()
            .id(self.id())
            .flex()
            .w(CONTROL_ICON_WIDTH)
            .h_full()
            .flex_shrink_0()
            .justify_center()
            .content_center()
            .items_center()
            .text_color(cx.theme().foreground)
            .hover(|style| style.bg(hover_bg).text_color(hover_fg))
            .active(|style| style.bg(active_bg).text_color(hover_fg))
            .when(is_windows, |this| {
                this.window_control_area(self.window_control_area())
            })
            .when(is_linux, |this| {
                this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    match icon {
                        Self::Minimize => window.minimize_window(),
                        Self::Restore | Self::Maximize => window.zoom_window(),
                        Self::Close { .. } => {
                            if let Some(f) = on_close_window.clone() {
                                f(&ClickEvent::default(), window, cx);
                            } else {
                                window.remove_window();
                            }
                        }
                    }
                })
            })
            .child(Icon::new(self.icon()).small())
    }
}

/// The trailing minimize / maximize / close cluster. Empty wherever
/// [`draws_window_controls`] is false. Mountable on its own, without a
/// [`TitleBar`].
#[derive(IntoElement, Default)]
pub struct WindowControls {
    on_close_window: Option<CloseHandler>,
}

impl WindowControls {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `f` instead of `Window::remove_window` when close is clicked. Linux
    /// only, as with [`TitleBar::on_close_window`]; on Windows the veto belongs
    /// on `Window::on_window_should_close`.
    #[must_use]
    pub fn on_close_window(
        mut self,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        if cfg!(target_os = "linux") {
            self.on_close_window = Some(Rc::new(f));
        }
        self
    }
}

impl RenderOnce for WindowControls {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        if !draws_window_controls(window) {
            return div().id("window-controls");
        }

        h_flex()
            .id("window-controls")
            .items_center()
            .flex_shrink_0()
            .h_full()
            .child(ControlIcon::minimize())
            .child(if window.is_maximized() {
                ControlIcon::restore()
            } else {
                ControlIcon::maximize()
            })
            .child(ControlIcon::close(self.on_close_window))
    }
}

struct TitleBarState {
    should_move: bool,
}

fn arm_window_move(bar: Stateful<Div>, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    if cfg!(target_os = "windows") {
        return bar;
    }
    let state = window.use_state(cx, |_, _| TitleBarState { should_move: false });
    bar.on_mouse_down_out(window.listener_for(&state, |state, _, _, _| {
        state.should_move = false;
    }))
    .on_mouse_down(
        MouseButton::Left,
        window.listener_for(&state, |state, _, _, _| {
            state.should_move = true;
        }),
    )
    .on_mouse_up(
        MouseButton::Left,
        window.listener_for(&state, |state, _, _, _| {
            state.should_move = false;
        }),
    )
    .on_mouse_move(window.listener_for(&state, |state, _, window, _| {
        if state.should_move {
            state.should_move = false;
            window.start_window_move();
        }
    }))
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let is_client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let is_web = cfg!(target_family = "wasm");
        let is_linux = cfg!(target_os = "linux");
        let is_macos = cfg!(target_os = "macos");
        let is_fullscreen = window.is_fullscreen();

        div().flex_shrink_0().child(
            div()
                .id("title-bar")
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .h(TITLE_BAR_HEIGHT)
                .pl(title_bar_left_padding(cx))
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .refine_style(&self.style)
                .when(is_linux, |this| {
                    this.on_double_click(|_, window, _| window.zoom_window())
                })
                .when(is_macos, |this| {
                    this.on_double_click(|_, window, _| window.titlebar_double_click())
                })
                .map(|bar| arm_window_move(bar, window, cx))
                .child(
                    h_flex()
                        .id("bar")
                        .h_full()
                        .justify_between()
                        .flex_shrink_0()
                        .flex_1()
                        .when(!is_web, |this| {
                            this.window_control_area(WindowControlArea::Drag)
                                .when(is_fullscreen, gpui::Styled::pl_3)
                                .when(is_linux && is_client_decorated, |this| {
                                    this.child(
                                        div()
                                            .top_0()
                                            .left_0()
                                            .absolute()
                                            .size_full()
                                            .h_full()
                                            .on_mouse_down(
                                                MouseButton::Right,
                                                move |ev, window, _| {
                                                    window.show_window_menu(ev.position);
                                                },
                                            ),
                                    )
                                })
                        })
                        .children(self.children),
                )
                .child(WindowControls {
                    on_close_window: self.on_close_window,
                }),
        )
    }
}
