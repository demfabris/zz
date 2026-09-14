use gpui::{
    AnyElement, App, Context, DragMoveEvent, Entity, IntoElement, KeyUpEvent, MouseButton, Render,
    Window, div, prelude::*,
};
use zz_ui::shell::{app_shell_surface, app_titlebar_strip};
use zz_ui::{
    ActiveTheme as _, Root, WindowControls, draws_window_controls,
    navigation::workspace_chrome_controls_width,
};

#[cfg(target_os = "macos")]
use crate::macos_app::{CloseWindow, Minimize, Zoom};
#[cfg(target_os = "linux")]
use crate::window::frame::rounded_window_frame;
use crate::{
    agent::AgentController,
    browser::controller::BrowserController,
    config::{frame_content_corner_radius, resolved_config, settings::OpenSettings},
    diagnostics::fps::app_fps_overlay,
    mux::client::MuxClient,
    request_window_close,
    status_bar::render_gui_status_bar,
    window::{corners::WindowCorners, drag::window_drag_handle},
    workspace::{
        AppView, ClosePane,
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
    mux: Entity<MuxClient>,
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
        let mux = workspace.read(cx).mux();
        #[cfg(not(target_os = "ios"))]
        crate::menus::observe_mux(&mux, cx);
        cx.subscribe(&sidebar, |_, _, _: &SidebarModeChanged, cx| cx.notify())
            .detach();
        cx.subscribe(&sidebar, |_, _, _: &SidebarRouteChanged, cx| cx.notify())
            .detach();
        let mut snapshot_generation = mux.read(cx).snapshot().generation;
        let mut attachment = (
            mux.read(cx).attached_host(),
            mux.read(cx).attached_session(),
            mux.read(cx).is_connected(),
        );
        cx.observe(&mux, move |_, mux, cx| {
            let mux = mux.read(cx);
            let next_snapshot_generation = mux.snapshot().generation;
            let next_attachment = (
                mux.attached_host(),
                mux.attached_session(),
                mux.is_connected(),
            );
            if next_snapshot_generation != snapshot_generation || next_attachment != attachment {
                snapshot_generation = next_snapshot_generation;
                attachment = next_attachment;
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(
            &mux,
            |_, _, _: &crate::mux::client::AgentStateChanged, cx| {
                cx.notify();
            },
        )
        .detach();
        cx.observe_global::<crate::config::AppConfig>(|_, cx| cx.notify())
            .detach();
        cx.observe_global::<crate::update::UpdateState>(|_, cx| cx.notify())
            .detach();
        let app_fps_meter = cx.new(|cx| AppFpsMeter::new(window, cx));
        Self {
            workspace,
            controller,
            agent_controller,
            sidebar,
            mux,
            app_fps_meter,
        }
    }

    #[cfg(not(target_os = "ios"))]
    fn active_menu_target(
        &self,
        session: bool,
        cx: &App,
    ) -> Option<(crate::mux::nav::TreeTarget, String)> {
        let mux = self.mux.read(cx);
        let snapshot = mux.snapshot();
        let attached = mux.attached_session()?;
        let current = snapshot
            .sessions
            .iter()
            .find(|entry| entry.id == attached)?;
        if session {
            Some((
                crate::mux::nav::TreeTarget::Session(current.id),
                current.name.clone(),
            ))
        } else {
            let focused = snapshot.focused_window_for(current);
            let window = current.windows.iter().find(|entry| entry.id == focused)?;
            Some((
                crate::mux::nav::TreeTarget::Window(window.id),
                window.name.clone(),
            ))
        }
    }

    #[cfg(not(target_os = "ios"))]
    fn rename_menu_target(&self, session: bool, cx: &App) {
        if let Some((target, name)) = self.active_menu_target(session, cx)
            && let Some((_, command)) = crate::mux::nav::rename_prompt_command(target, &name)
        {
            self.mux.read(cx).execute(command);
        }
    }

    #[cfg(not(target_os = "ios"))]
    fn split_menu_pane(&self, axis: zz_protocol::Axis, cx: &App) {
        let mux = self.mux.read(cx);
        if let Some(pane) = mux.active_pane() {
            mux.execute(crate::mux::nav::split_picker_command(pane, axis));
        }
    }

    #[cfg(not(target_os = "ios"))]
    fn pane_menu_command(&self, command: &str, flags: &[&str], cx: &App) {
        let mux = self.mux.read(cx);
        if let Some(pane) = mux.active_pane() {
            let mut args = flags
                .iter()
                .map(|flag| (*flag).to_owned())
                .collect::<Vec<_>>();
            args.extend(["-t".to_owned(), pane.to_string()]);
            mux.execute(zz_protocol::CommandInvocation::new(command, args));
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

    fn window_controls(&self) -> WindowControls {
        let close_controller = self.controller.clone();
        let close_agent_controller = self.agent_controller.clone();
        WindowControls::new().on_close_window(move |_, window, cx| {
            if request_window_close(&close_controller, &close_agent_controller, window, cx) {
                window.remove_window();
            }
        })
    }

    fn render_control_strip(&self, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        draws_window_controls(window).then(|| {
            let title_corners = WindowCorners::for_window(window).top();
            let strip = app_titlebar_strip("app-titlebar", self.window_controls())
                .when(title_corners.top_right(), |strip| {
                    strip.rounded_tr(frame_content_corner_radius(cx))
                });
            if crate::profile::profile(cx).fixed_window {
                strip.into_any_element()
            } else {
                window_drag_handle("app-titlebar-drag", strip, window, cx).into_any_element()
            }
        })
    }

    fn render_status_bar(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let has_layout = !crate::profile::profile(cx).fixed_window;
        let width = workspace_chrome_controls_width(has_layout, window);
        let controls = self.sidebar.read(cx).render_controls(&self.sidebar, cx);
        let bar = render_gui_status_bar(
            &self.mux,
            Some((controls, width)),
            Some(self.window_controls().into_any_element()),
            window,
            cx,
        );
        let bar = WindowCorners::for_window(window)
            .top()
            .round_div(bar, frame_content_corner_radius(cx));
        if crate::profile::profile(cx).fixed_window {
            return bar.into_any_element();
        }
        window_drag_handle("gui-status-titlebar-drag", bar, window, cx).into_any_element()
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
                    .bg(crate::theme::chrome_background(cx))
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
        let (sidebar, titlebar) = match (route, mode) {
            (WorkspaceRoute::Settings, _) | (WorkspaceRoute::App, ChromeMode::Sidebar) => (
                self.sidebar.clone().into_any_element(),
                self.render_control_strip(window, cx),
            ),
            (WorkspaceRoute::App, ChromeMode::Titlebar) => (
                div().into_any_element(),
                Some(self.render_status_bar(window, cx)),
            ),
        };
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        let slideover = (route == WorkspaceRoute::App && self.sidebar.read(cx).slideover_open())
            .then(|| self.render_slideover(cx));
        let overlays = show_fps
            .then(|| app_fps_overlay(self.app_fps_meter.clone()).into_any_element())
            .into_iter()
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
            if cfg!(target_os = "linux")
                && matches!(
                    window.window_decorations(),
                    gpui::Decorations::Client { .. }
                )
            {
                gpui::transparent_black()
            } else {
                crate::theme::chrome_background(cx)
            },
            sidebar,
            titlebar,
            self.workspace.clone(),
            overlays,
        )
        .map(|shell| {
            WindowCorners::for_window(window).round_div(shell, frame_content_corner_radius(cx))
        })
        .on_drag_move::<SidebarResizeDrag>(cx.listener(Self::on_sidebar_resize_drag_move))
        .capture_key_up(cx.listener(|shell, event: &KeyUpEvent, window, cx| {
            shell.workspace.update(cx, |workspace, cx| {
                workspace.on_claim_key_up(event, window, cx);
            });
        }));

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

        #[cfg(not(target_os = "ios"))]
        let shell = {
            use crate::menus;
            use crate::mux::nav::{TreeTarget, kill_target_command, new_window_command};
            use zz_protocol::{Axis, CommandInvocation};

            shell
                .on_action(cx.listener(|shell, _: &menus::NewSession, _, cx| {
                    let mux = shell.mux.read(cx);
                    mux.new_session(mux.attached_host());
                }))
                .on_action(cx.listener(|shell, _: &menus::NewWindow, _, cx| {
                    let mux = shell.mux.read(cx);
                    if let Some(session) = mux.attached_session() {
                        mux.execute(new_window_command(session));
                    }
                }))
                .on_action(cx.listener(|shell, _: &menus::SplitRight, _, cx| {
                    shell.split_menu_pane(Axis::Horizontal, cx);
                }))
                .on_action(cx.listener(|shell, _: &menus::SplitDown, _, cx| {
                    shell.split_menu_pane(Axis::Vertical, cx);
                }))
                .when(crate::browser::controller::is_available(cx), |shell| {
                    shell.on_action(cx.listener(|shell, _: &menus::NewBrowserPane, _, cx| {
                        shell.pane_menu_command("split-browser", &[], cx);
                    }))
                })
                .when(crate::config::agent_pane_enabled(cx), |shell| {
                    shell.on_action(cx.listener(|shell, _: &menus::NewAgentPane, _, cx| {
                        shell.pane_menu_command("split-agent", &[], cx);
                    }))
                })
                .on_action(cx.listener(|shell, action: &menus::SwitchSession, _, cx| {
                    let session = shell
                        .mux
                        .read(cx)
                        .snapshot()
                        .sessions
                        .iter()
                        .find(|session| session.name == action.name)
                        .map(|session| session.id);
                    if let Some(session) = session {
                        shell.mux.update(cx, |mux, _| mux.attach(session));
                    }
                }))
                .on_action(cx.listener(|shell, _: &menus::RenameSession, _, cx| {
                    shell.rename_menu_target(true, cx);
                }))
                .on_action(cx.listener(|shell, _: &menus::RenameWindow, _, cx| {
                    shell.rename_menu_target(false, cx);
                }))
                .on_action(cx.listener(|shell, _: &menus::KillWindow, _, cx| {
                    if let Some((target, _)) = shell.active_menu_target(false, cx) {
                        shell.mux.read(cx).execute(kill_target_command(target));
                    }
                }))
                .on_action(cx.listener(|shell, _: &menus::KillSession, _, cx| {
                    if let Some((TreeTarget::Session(session), name)) =
                        shell.active_menu_target(true, cx)
                    {
                        shell.mux.read(cx).execute(CommandInvocation::new(
                            "confirm-before",
                            [
                                "-p".to_owned(),
                                format!("Kill session {name}? (y/n)"),
                                format!("kill-session -t {session}"),
                            ],
                        ));
                    }
                }))
                .on_action(cx.listener(|shell, _: &menus::Detach, _, cx| {
                    shell
                        .mux
                        .read(cx)
                        .execute(CommandInvocation::new("detach-client", [] as [&str; 0]));
                }))
                .on_action(cx.listener(|shell, _: &menus::ZoomPane, _, cx| {
                    shell.pane_menu_command("resize-pane", &["-Z"], cx);
                }))
                .on_action(cx.listener(|shell, action: &menus::SelectPane, _, cx| {
                    let direction = match action.direction.as_str() {
                        "U" => "-U",
                        "D" => "-D",
                        "L" => "-L",
                        "R" => "-R",
                        _ => return,
                    };
                    shell.pane_menu_command("select-pane", &[direction], cx);
                }))
                .on_action(cx.listener(|shell, _: &menus::ToggleSidebar, _, cx| {
                    shell.sidebar.update(cx, WorkspaceSidebar::toggle_mode);
                }))
                .on_action(|_: &menus::ShowFps, _, cx| {
                    let enabled = !resolved_config(cx).show_fps.value;
                    if let Err(error) = crate::config::set_config_key(
                        crate::config::ConfigKey::ShowFps,
                        if enabled { "true" } else { "false" },
                    ) {
                        crate::window::toast::push(
                            zz_ui::notification::Notification::error(format!(
                                "Could not change Show FPS: {error}"
                            )),
                            cx,
                        );
                    }
                })
                .on_action(|_: &menus::CheckForUpdates, _, cx| crate::update::check_now(cx))
                .on_action(|_: &menus::OpenLogs, _, cx| crate::diagnostics::open_logs(cx))
                .on_action(|_: &menus::ImportTmuxConfig, window, cx| {
                    crate::config::import_prompt::choose_tmux_config(window, cx);
                })
                .on_action(cx.listener(|shell, _: &menus::About, window, cx| {
                    shell
                        .sidebar
                        .update(cx, |sidebar, cx| sidebar.open_settings(window, cx));
                    if let Some(settings) = shell.sidebar.read(cx).settings_view() {
                        settings.update(cx, |settings, cx| {
                            settings.set_section(
                                zz_ui::settings::SettingsSection::About,
                                window,
                                cx,
                            );
                        });
                    }
                }))
                .on_action(|action: &menus::OpenUrl, _, cx| {
                    if action.url == menus::RELEASE_NOTES_URL
                        && let Some(crate::update::Status {
                            check: crate::update::CheckState::Available(release),
                            ..
                        }) = crate::update::status(cx)
                    {
                        cx.open_url(&release.url);
                    } else {
                        cx.open_url(&action.url);
                    }
                })
        };

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
            let agent_controller =
                cx.new(|_| AgentController::new(crate::config::AgentConfig::default()));
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
