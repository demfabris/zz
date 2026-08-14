use gpui::{
    AnyElement, App, Context, DragMoveEvent, Entity, IntoElement, KeyUpEvent, MouseButton, Render,
    Window, div, prelude::*, px,
};
use zz_ui::navigation::{
    WORKSPACE_STRIP_GAP, WORKSPACE_TREE_ACTION_INSET, WORKSPACE_TREE_CONTENT_INSET,
    workspace_titlebar_strip,
};
use zz_ui::shell::{app_shell_surface, app_titlebar_strip};
use zz_ui::{ActiveTheme as _, Root, TITLE_BAR_HEIGHT, WindowControls, draws_window_controls};

#[cfg(target_os = "macos")]
use crate::macos_app::{CloseWindow, Minimize, Zoom};
#[cfg(target_os = "linux")]
use crate::window::frame::rounded_window_frame;
use crate::{
    agent::AgentController,
    browser::controller::BrowserController,
    config::{frame_content_corner_radius, resolved_config, settings::OpenSettings},
    diagnostics::fps::app_fps_overlay,
    request_window_close,
    window::{corners::WindowCorners, drag::window_drag_handle},
    workspace::{
        AppView, ClosePane, WindowOverviewChanged,
        overview::{ToggleWindowOverview, overview_titlebar_height},
        sidebar::{
            ChromeMode, SidebarModeChanged, SidebarResizeDrag, SidebarRouteChanged, WorkspaceRoute,
            WorkspaceSidebar,
        },
    },
};

pub use crate::diagnostics::fps::AppFpsMeter;

pub struct AppShell {
    workspace: Entity<AppView>,
    controller: Entity<BrowserController>,
    agent_controller: Entity<AgentController>,
    sidebar: Entity<WorkspaceSidebar>,
    app_fps_meter: Entity<AppFpsMeter>,
}

impl AppShell {
    pub fn new(
        workspace: Entity<AppView>,
        controller: Entity<BrowserController>,
        agent_controller: Entity<AgentController>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sidebar = workspace.read(cx).sidebar();
        cx.subscribe(&sidebar, |_, _, _: &SidebarModeChanged, cx| cx.notify())
            .detach();
        cx.subscribe(&sidebar, |_, _, _: &SidebarRouteChanged, cx| cx.notify())
            .detach();
        cx.subscribe(&workspace, |_, _, _: &WindowOverviewChanged, cx| {
            cx.notify();
        })
        .detach();
        let app_fps_meter = cx.new(|cx| AppFpsMeter::new(window, cx));
        Self {
            workspace,
            controller,
            agent_controller,
            sidebar,
            app_fps_meter,
        }
    }

    fn on_sidebar_resize_drag_move(
        &mut self,
        event: &DragMoveEvent<SidebarResizeDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pointer_offset = f32::from(event.event.position.x - event.bounds.origin.x);
        let available_width = f32::from(event.bounds.size.width);
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.resize_to(pointer_offset, available_width, cx);
        });
        cx.stop_propagation();
    }

    fn window_controls(&self, scale: f32) -> WindowControls {
        let close_controller = self.controller.clone();
        let close_agent_controller = self.agent_controller.clone();
        WindowControls::new()
            .scale(scale)
            .on_close_window(move |_, window, cx| {
                if request_window_close(&close_controller, &close_agent_controller, window, cx) {
                    window.remove_window();
                }
            })
    }

    fn render_control_strip(
        &self,
        window: &mut Window,
        cx: &mut App,
        force: bool,
        paint_background: bool,
        scale: f32,
    ) -> Option<AnyElement> {
        let draws_controls = draws_window_controls(window);
        (draws_controls || force).then(|| {
            let title_corners = WindowCorners::for_window(window).top();
            let controls = if draws_controls {
                self.window_controls(scale).into_any_element()
            } else {
                div().into_any_element()
            };
            let strip = app_titlebar_strip("app-titlebar", controls)
                .h(TITLE_BAR_HEIGHT * scale)
                .when(paint_background, |strip| {
                    strip.bg(crate::theme::chrome_background(cx))
                })
                .when(title_corners.top_right(), |strip| {
                    strip.rounded_tr(frame_content_corner_radius(cx) * scale)
                });
            if crate::profile::profile(cx).fixed_window {
                strip.into_any_element()
            } else {
                window_drag_handle("app-titlebar-drag", strip, window, cx).into_any_element()
            }
        })
    }

    fn render_workspace_strip(
        &self,
        window: &mut Window,
        cx: &mut App,
        paint_background: bool,
    ) -> AnyElement {
        let leading = self
            .sidebar
            .read(cx)
            .render_strip_controls(&self.sidebar, cx);
        let (content, status) = self.sidebar.read(cx).render_strip_content(cx);
        let trailing = div()
            .flex()
            .flex_none()
            .h_full()
            .items_center()
            .gap(px(WORKSPACE_STRIP_GAP))
            .child(status)
            .map(|cluster| {
                if draws_window_controls(window) {
                    cluster.child(self.window_controls(1.0))
                } else {
                    cluster.pr(px(
                        WORKSPACE_TREE_CONTENT_INSET + WORKSPACE_TREE_ACTION_INSET
                    ))
                }
            });
        let strip = WindowCorners::for_window(window).top().round_div(
            workspace_titlebar_strip("workspace-strip", leading, content, trailing, cx)
                .when(paint_background, |strip| {
                    strip.bg(crate::theme::chrome_background(cx))
                }),
            frame_content_corner_radius(cx),
        );
        if crate::profile::profile(cx).fixed_window {
            strip.into_any_element()
        } else {
            window_drag_handle("workspace-strip-drag", strip, window, cx).into_any_element()
        }
    }

    fn render_slideover(&self, cx: &App) -> AnyElement {
        let dismiss = self.sidebar.clone();
        div()
            .id("workspace-slideover-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_start()
            .bg(cx.theme().scrim)
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                dismiss.update(cx, WorkspaceSidebar::dismiss_slideover);
                cx.stop_propagation();
            })
            .child(
                div()
                    .h_full()
                    .flex()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.sidebar.clone()),
            )
            .into_any_element()
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_fps = resolved_config(cx).show_fps.value;
        self.app_fps_meter
            .update(cx, |meter, cx| meter.set_enabled(show_fps, cx));
        let (route, mode) = {
            let sidebar = self.sidebar.read(cx);
            (sidebar.route(), sidebar.mode())
        };
        let overview_open = self.workspace.read(cx).window_overview_open();
        let overview_titlebar_height = overview_titlebar_height(mode, window, cx);
        let overview_titlebar = (overview_open && overview_titlebar_height > px(0.0)).then(|| {
            let scale = f32::from(overview_titlebar_height) / f32::from(TITLE_BAR_HEIGHT);
            self.render_control_strip(window, cx, true, false, scale)
                .expect("overview titlebar policy requested a control strip")
        });
        let (sidebar, titlebar) = if overview_open {
            (div().into_any_element(), None)
        } else {
            match route {
                WorkspaceRoute::Settings => (
                    self.sidebar.clone().into_any_element(),
                    self.render_control_strip(window, cx, false, true, 1.0),
                ),
                WorkspaceRoute::App => match mode {
                    ChromeMode::Sidebar => (
                        self.sidebar.clone().into_any_element(),
                        self.render_control_strip(window, cx, false, true, 1.0),
                    ),
                    ChromeMode::Titlebar => (
                        div().into_any_element(),
                        Some(self.render_workspace_strip(window, cx, true)),
                    ),
                },
            }
        };
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        let slideover = (route == WorkspaceRoute::App
            && !overview_open
            && self.sidebar.read(cx).slideover_open())
        .then(|| self.render_slideover(cx));
        let overview_titlebar = overview_titlebar.map(|titlebar| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .child(titlebar)
                .into_any_element()
        });
        let overlays = overview_titlebar
            .into_iter()
            .chain(show_fps.then(|| app_fps_overlay(self.app_fps_meter.clone()).into_any_element()))
            .chain(slideover)
            .chain(dialog_layer.into_iter().map(IntoElement::into_any_element))
            .chain(
                notification_layer
                    .into_iter()
                    .map(IntoElement::into_any_element),
            )
            .collect::<Vec<_>>();
        let shell = app_shell_surface(
            "app-shell",
            sidebar,
            titlebar,
            self.workspace.clone(),
            overlays,
        )
        .when(overview_open, |shell| {
            shell.bg(crate::theme::chrome_background(cx))
        })
        .on_drag_move::<SidebarResizeDrag>(cx.listener(Self::on_sidebar_resize_drag_move))
        .capture_key_up(cx.listener(|shell, event: &KeyUpEvent, window, cx| {
            shell.workspace.update(cx, |workspace, cx| {
                workspace.on_claim_key_up(event, window, cx);
            });
        }));

        let overview_workspace = self.workspace.clone();
        let shell = shell.on_action(move |_: &ToggleWindowOverview, window, cx| {
            if !overview_workspace.update(cx, |workspace, cx| {
                workspace.toggle_window_overview(window, cx)
            }) {
                cx.propagate();
            }
        });

        let settings_sidebar = self.sidebar.clone();
        let shell = shell.on_action(move |_: &OpenSettings, window, cx| {
            settings_sidebar.update(cx, |sidebar, cx| sidebar.open_settings(window, cx));
        });

        let workspace = self.workspace.clone();
        let sidebar = self.sidebar.clone();
        let shell = shell.on_action(move |_: &ClosePane, window, cx| {
            if sidebar.read(cx).route() == WorkspaceRoute::Settings {
                sidebar.update(cx, |sidebar, cx| {
                    sidebar.close_settings(window, cx);
                });
            } else if !workspace.update(cx, AppView::close_active_pane) {
                cx.propagate();
            }
        });

        #[cfg(target_os = "macos")]
        let shell = {
            let controller = self.controller.clone();
            let agent_controller = self.agent_controller.clone();
            shell
                .on_action(move |_: &CloseWindow, window, cx| {
                    if request_window_close(&controller, &agent_controller, window, cx) {
                        window.remove_window();
                    }
                })
                .on_action(|_: &Minimize, window, _| window.minimize_window())
                .on_action(|_: &Zoom, window, _| window.zoom_window())
        };

        #[cfg(target_os = "linux")]
        {
            rounded_window_frame().child(shell)
        }
        #[cfg(not(target_os = "linux"))]
        {
            shell
        }
    }
}

// gpui-component's macOS Root constructor requires a native NSView; GPUI test windows have none.
#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use gpui::{TestAppContext, VisualTestContext, div};
    use zz_browser::BrowserError;
    use zz_daemon::DaemonError;
    use zz_protocol::ClientMessageKind;
    use zz_ui::{WindowExt as _, notification::Notification};

    use super::*;
    use crate::mux::client::MuxClient;

    #[gpui::test]
    fn mux_notifications_reach_the_mounted_root_layer(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let mux_slot = Rc::new(RefCell::new(None));
        let captured_mux = Rc::clone(&mux_slot);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let controller =
                cx.new(|cx| BrowserController::new(Err(BrowserError::AlreadyShutdown), cx));
            let agent_controller = cx.new(|_| {
                AgentController::new(crate::config::AgentConfig {
                    command: "unused-test-agent".to_owned(),
                    claude_code_command: "unused-test-claude-agent".to_owned(),
                    working_directory: None,
                })
            });
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            captured_mux.replace(Some(mux.clone()));
            let workspace = cx.new(|cx| {
                AppView::new(
                    controller.clone(),
                    agent_controller.clone(),
                    mux,
                    window,
                    cx,
                )
            });
            let shell =
                cx.new(|cx| AppShell::new(workspace, controller, agent_controller, window, cx));
            crate::build_root(shell, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        let mux = mux_slot.borrow().clone().expect("captured mux");

        mux.update(cx, |_, cx| {
            MuxClient::emit_notification(ClientMessageKind::Success, "configuration reloaded", cx);
        });
        cx.run_until_parked();
        assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 1);

        let rendered = Rc::new(Cell::new(false));
        let rendered_probe = Rc::clone(&rendered);
        cx.update(|window, cx| {
            window.push_notification(
                Notification::new().autohide(false).content(move |_, _, _| {
                    rendered_probe.set(true);
                    div().into_any_element()
                }),
                cx,
            );
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(rendered.get(), "notification content was not rendered");
    }
}
