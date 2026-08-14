//! iPad application assembly over zz's public engine surface.

use std::path::PathBuf;

use gpui::{App, AppContext as _, WindowOptions};

/// Starts the iPad app, called by the `zz-ios` binary inside `ZZ.app`. Window
/// construction and nothing else: iOS forbids subprocesses, so the daemon is
/// always a peer over the socket, and the one window is fullscreen anyway.
pub(crate) fn run(profile: zz::AppProfile) {
    zz::engine::diagnostics::init();
    // The bundle has no argv for `--socket`, so the environment carries it
    // (`simctl launch` forwards `SIMCTL_CHILD_ZZ_SOCKET`).
    let socket_path =
        std::env::var_os("ZZ_SOCKET").map_or_else(zz::engine::default_socket_path, PathBuf::from);
    log::info!(
        target: "zz::diagnostics::lifecycle",
        "gpui_source={} socket={}",
        zz::engine::gpui_source(),
        socket_path.display()
    );
    let connect_profile = profile.clone();
    let application =
        gpui::Application::with_platform(std::rc::Rc::new(zz_gpui_ios::IosPlatform::new()))
            .with_assets(zz_ui::Assets);
    application.on_reopen(crate::chrome::nudge_reconnects);
    application.run(move |cx: &mut App| {
        cx.set_global(profile);
        zz::engine::diagnostics::start_main_thread_watchdog(cx);
        cx.set_reduce_motion(zz_gpui_ios::reduce_motion());
        zz::engine::config::init(cx);
        zz::engine::window::background::detect_compositor_support(cx);
        zz::engine::browser::recent_pages::init(cx);
        zz_ui::init(cx);
        zz::engine::ui_scale::init(cx);
        zz::engine::config::settings::init(cx);
        zz::engine::browser::view::init(cx);
        zz::engine::editor::init(cx);
        zz::engine::terminal::view::init(cx);
        zz::engine::workspace::init(cx);

        let controller = cx.new(zz::engine::browser::BrowserController::new);
        let agent_config = zz::engine::config::agent_config(cx);
        let preferences = zz::engine::agent::AgentPreferences::load_persistent();
        let agent_socket = socket_path.to_str().map(str::to_owned);
        if zz::engine::config::agent_pane_enabled(cx) {
            zz::engine::agent::warm_agent_adapter_cache(&agent_config);
        }
        let agent_controller = cx.new(|_| {
            zz::engine::agent::AgentController::with_preferences(
                agent_config,
                preferences,
                agent_socket,
            )
        });

        let main_window = cx
            .open_window(WindowOptions::default(), move |window, cx| {
                zz::engine::ui_scale::apply_to_new_window(window, cx);
                zz::engine::theme::sync_system_appearance(Some(window), cx);
                let color_scheme = zz::engine::terminal_color_scheme(window.appearance());
                let mux = zz::engine::connect_local(&connect_profile, &socket_path, color_scheme);
                let mux = cx.new(|cx| {
                    zz::engine::mux::MuxClient::new_with_color_scheme(
                        mux,
                        socket_path.clone(),
                        color_scheme,
                        cx,
                    )
                });
                cx.set_global(crate::chrome::IosMuxHandle(mux.downgrade()));

                zz::engine::diagnostics::start_app_state_sampler(
                    controller.clone(),
                    mux.clone(),
                    cx,
                );
                zz::engine::diagnostics::init_debug_mark(controller.clone(), mux.clone(), cx);

                let shutdown_mux = mux.clone();
                cx.on_app_quit(move |cx| {
                    // Detach, so the daemon's sessions survive a kill.
                    shutdown_mux.update(cx, |mux, _| mux.detach());
                    async {}
                })
                .detach();

                let appearance_mux = mux.clone();
                window
                    .observe_window_appearance(move |window, cx| {
                        zz::engine::theme::sync_system_appearance(Some(window), cx);
                        appearance_mux.update(cx, |mux, _| {
                            mux.set_color_scheme(zz::engine::terminal_color_scheme(
                                window.appearance(),
                            ));
                        });
                    })
                    .detach();

                let view = cx.new(|cx| {
                    zz::engine::workspace::AppView::new(
                        controller.clone(),
                        agent_controller.clone(),
                        mux,
                        window,
                        cx,
                    )
                });
                let shell = cx.new(|cx| {
                    zz::engine::AppShell::new(view, controller, agent_controller, window, cx)
                });
                let chrome = cx.new(|_| crate::chrome::IosChrome::new(shell.into()));
                cx.new(|cx| zz::engine::build_root(chrome, window, cx))
            })
            .expect("failed to open zz window");
        zz::engine::window::toast::set_host(main_window, cx);
    });
}
