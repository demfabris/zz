mod agent;
mod app_icon;
mod app_shell;
mod browser;
mod chooser;
mod command;
mod config;
mod diagnostics;
mod editor;
#[cfg(any(feature = "agent-pane", feature = "editor-pane"))]
mod file_picker;
mod keymap;
#[cfg(target_os = "macos")]
mod macos_app;
mod menus;
mod mux;
mod pane;
mod profile;
mod status_bar;
mod terminal;
mod theme;
/// A desktop menu bar / notification area; the iPad has neither.
#[cfg(not(target_os = "ios"))]
mod tray;
mod ui_scale;
#[cfg(not(target_os = "ios"))]
mod update;
mod user_data;
mod window;
mod workspace;

use std::path::Path;
#[cfg(not(target_os = "ios"))]
use std::{path::PathBuf, process::ExitCode};
#[cfg(target_os = "ios")]
use std::{
    thread,
    time::{Duration, Instant},
};

use gpui::Styled as _;
use gpui::{AnyView, App, Context, Entity, Window, WindowAppearance};
#[cfg(not(target_os = "ios"))]
use gpui::{AppContext, WindowOptions, px, size};
#[cfg(not(target_os = "ios"))]
use zz_browser::{BrowserBootstrap, BrowserError, BrowserRuntime};
#[cfg(not(target_os = "ios"))]
pub(crate) use zz_cli::application_arguments;
#[cfg(not(target_os = "ios"))]
use zz_cli::{CommandLineOrigin, Startup, StartupOptions};
#[cfg(not(target_os = "ios"))]
use zz_daemon::default_socket_path;
use zz_daemon::{DaemonError, InteractiveClient};
#[cfg(not(target_os = "ios"))]
use zz_protocol::CommandInvocation;
use zz_terminal::TerminalColorScheme;
#[cfg(not(target_os = "ios"))]
use zz_ui::Assets;
use zz_ui::Root;

use agent::AgentController;
#[cfg(not(target_os = "ios"))]
use agent::AgentPreferences;
#[cfg(not(target_os = "ios"))]
use app_shell::AppShell;
use browser::controller::BrowserController;
#[cfg(not(target_os = "ios"))]
use workspace::AppView;

pub use profile::{AppProfile, LocalHostPolicy, SettingsSection};

/// Windows runs the application out of `zz.dll`, where `main.rs` never executes.
#[cfg(windows)]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const GPUI_SOURCE: &str = env!("ZZ_GPUI_SOURCE");

/// Start the native executable on Linux and macOS.
#[cfg(not(any(target_os = "windows", target_os = "ios")))]
#[must_use]
pub fn run() -> ExitCode {
    #[cfg(unix)]
    if let Some(exit) = zz_cli::run_askpass_mode() {
        return exit;
    }
    let mut socket_path = default_socket_path();
    if !is_cef_subprocess() {
        socket_path = match zz_cli::run_startup(
            &socket_path,
            StartupOptions {
                origin: CommandLineOrigin::Application,
                browser_provider: Some(tui_browser_provider),
            },
        ) {
            Startup::Application(socket_path) => socket_path,
            Startup::Exit(exit) => return exit,
        };
        #[cfg(target_os = "macos")]
        if !macos_cef_framework_is_available() {
            eprintln!(
                "zz: the macOS app must be launched from its CEF bundle\n\
                 build it with `cargo xtask bundle-cef --release --output dist/zz`, then run \
                 `open dist/zz/zz.app`"
            );
            return ExitCode::FAILURE;
        }
    }
    ExitCode::from(finish_bootstrap(
        zz_browser::bootstrap(),
        socket_path,
        AppProfile::desktop(),
    ))
}

/// Serve the Windows command line. Only the bundled `zz.exe` can open the window.
#[cfg(target_os = "windows")]
#[must_use]
pub fn run() -> ExitCode {
    if let Some(exit) = zz_cli::run_askpass_mode() {
        return exit;
    }
    match zz_cli::run_startup(
        &default_socket_path(),
        StartupOptions {
            origin: CommandLineOrigin::Application,
            browser_provider: Some(tui_browser_provider),
        },
    ) {
        Startup::Exit(exit) => exit,
        Startup::Application(_) => {
            eprintln!(
                "zz: the Windows app must be launched from its CEF bundle\n\
                 build it with `cargo xtask bundle-cef --release --output dist\\zz`, then run \
                 `dist\\zz\\zz.exe`"
            );
            ExitCode::FAILURE
        }
    }
}

/// The entry point CEF's bootstrap executable calls in `zz.dll`, for the browser
/// process and for every `--type=` subprocess it relaunches itself as.
#[cfg(target_os = "windows")]
#[allow(
    unsafe_code,
    reason = "CEF's Windows sandbox bootstrap requires an unmangled DLL export"
)]
#[unsafe(no_mangle)]
#[allow(non_snake_case, reason = "name is fixed by CEF's bootstrap ABI")]
pub extern "C" fn RunWinMain(
    instance: cef::sys::HINSTANCE,
    _command_line: *mut u16,
    _show_command: i32,
    sandbox_info: *mut core::ffi::c_void,
    _version_info: *mut core::ffi::c_void,
) -> i32 {
    if let Some(exit) = zz_cli::run_askpass_mode() {
        return if exit == ExitCode::SUCCESS { 0 } else { 1 };
    }
    zz_cli::attach_parent_console();
    let mut socket_path = default_socket_path();
    if !is_cef_subprocess() {
        socket_path = match zz_cli::run_startup(
            &socket_path,
            StartupOptions {
                origin: CommandLineOrigin::Application,
                browser_provider: Some(tui_browser_provider),
            },
        ) {
            Startup::Application(socket_path) => socket_path,
            Startup::Exit(exit) => return if exit == ExitCode::SUCCESS { 0 } else { 1 },
        };
    }
    let result = zz_browser::bootstrap_windows(instance, sandbox_info.cast());
    i32::from(finish_bootstrap(result, socket_path, AppProfile::desktop()))
}

#[cfg(not(target_os = "ios"))]
fn is_cef_subprocess() -> bool {
    std::env::args_os()
        .skip(1)
        .any(|argument| argument == "--type" || argument.to_string_lossy().starts_with("--type="))
}

#[cfg(target_os = "macos")]
fn macos_cef_framework_is_available() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(std::path::Path::to_owned))
        .is_some_and(|directory| {
            directory
                .join("../Frameworks/Chromium Embedded Framework.framework")
                .join("Chromium Embedded Framework")
                .is_file()
        })
}

#[cfg(not(target_os = "ios"))]
fn finish_bootstrap(
    bootstrap: Result<BrowserBootstrap, BrowserError>,
    socket_path: PathBuf,
    profile: AppProfile,
) -> u8 {
    let runtime = match bootstrap {
        Ok(BrowserBootstrap::SubprocessExit(code)) => {
            return u8::try_from(code.clamp(0, 255)).unwrap_or_default();
        }
        Ok(BrowserBootstrap::Runtime(mut runtime)) => {
            runtime.set_log_file(diagnostics::cef_log_file());
            Ok(runtime)
        }
        Err(error) => Err(error),
    };
    run_app(runtime, socket_path, profile);
    0
}

#[cfg(not(target_os = "ios"))]
fn run_app(
    runtime: Result<BrowserRuntime, BrowserError>,
    socket_path: PathBuf,
    profile: AppProfile,
) {
    log::info!(
        target: "zz::diagnostics::appearance",
        "gpui_source={GPUI_SOURCE}"
    );
    let platform = gpui_platform::current_platform(false);
    let fonts = zz_ui::settings::appearance::AvailableFonts(platform.text_system());
    let application = gpui::Application::with_platform(platform);
    #[cfg(target_os = "macos")]
    application.on_reopen(|cx| {
        if let Some(window) = cx
            .try_global::<tray::DesktopTray>()
            .and_then(|tray| tray.window)
        {
            tray::show(window, cx);
        }
    });
    application
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            cx.set_global(fonts);
            cx.set_global(profile);
            diagnostics::start_main_thread_watchdog(cx);
            #[cfg(target_os = "macos")]
            cx.activate(true);
            config::init(cx);
            cx.set_global(tray::DesktopTray::default());
            window::background::detect_compositor_support(cx);
            browser::recent_pages::init(cx);
            zz_ui::init(cx);
            ui_scale::init(cx);
            browser::view::init(cx);
            editor::init(cx);
            terminal::view::init(cx);
            workspace::init(cx);
            #[cfg(target_os = "macos")]
            macos_app::init(cx);
            #[cfg(not(target_os = "macos"))]
            menus::install(cx);
            let controller = cx.new(|cx| BrowserController::new(runtime, cx));
            let agent_config = config::agent_config(cx);
            let preferences = AgentPreferences::load_persistent();
            let agent_controller =
                cx.new(|_| AgentController::with_preferences(agent_config, preferences));
            let window_state = window::state::MainWindowState::load_persistent();

            cx.on_window_closed(|cx, _window_id| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let minimum_window_size = size(px(480.0), px(320.0));
            let restored_window = window_state.restored_window(
                cx,
                size(px(1080.0), px(720.0)),
                minimum_window_size,
            );
            let window_decorations = config::window_decorations(cx);
            let mut titlebar = config::titlebar_options();
            if cfg!(target_os = "linux") {
                titlebar.title = Some(zz_protocol::app_identity::DISPLAY_NAME.into());
            }
            let main_window = cx.open_window(
                WindowOptions {
                    window_bounds: Some(restored_window.bounds),
                    titlebar: Some(titlebar),
                    app_owns_titlebar_drag: true,
                    window_background: config::window_background_appearance(cx),
                    display_id: restored_window.display_id,
                    window_min_size: Some(minimum_window_size),
                    window_decorations: Some(window_decorations),
                    app_id: Some(zz_protocol::app_identity::DIRECTORY.into()),
                    #[cfg(target_os = "linux")]
                    icon: Some(app_icon::x11_window_icon()),
                    ..Default::default()
                },
                move |window, cx| {
                    ui_scale::apply_to_new_window(window, cx);
                    theme::sync_system_appearance(Some(window), cx);
                    let color_scheme = terminal_color_scheme(window.appearance());
                    let mux = cx.new(|cx| {
                        mux::client::MuxClient::new_connecting(
                            socket_path.clone(),
                            color_scheme,
                            cx,
                        )
                    });
                    tray::init_desktop(&mux, window.window_handle(), cx);

                    diagnostics::start_app_state_sampler(controller.clone(), mux.clone(), cx);
                    diagnostics::init_debug_mark(controller.clone(), mux.clone(), cx);
                    update::start(mux.clone(), cx);

                    let shutdown_controller = controller.clone();
                    let shutdown_agent_controller = agent_controller.clone();
                    let shutdown_mux = mux.clone();
                    let shutdown_window_state = window_state.clone();
                    let shutdown_window_handle = window.window_handle();
                    cx.on_app_quit(move |cx| {
                        if shutdown_window_handle
                            .update(cx, |_, window, cx| {
                                shutdown_window_state.capture_and_flush(window, cx);
                            })
                            .is_err()
                        {
                            shutdown_window_state.flush();
                        }
                        #[cfg(target_os = "macos")]
                        mark_macos_app_as_background_only();
                        let verbose = diagnostics::enabled();
                        if verbose {
                            log::info!(target: "zz::diagnostics::lifecycle", "application shutdown requested");
                            shutdown_mux.read(cx).log_diagnostic_snapshot("shutdown");
                            shutdown_controller
                                .read(cx)
                                .log_diagnostic_snapshot("shutdown");
                        }
                        if config::quit_daemon_on_exit(cx)
                            && !cx.global::<tray::DesktopTray>().stopping_daemon
                        {
                            shutdown_mux
                                .read(cx)
                                .execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
                        }
                        shutdown_mux.update(cx, |mux, _| mux.detach());
                        let shutdown =
                            shutdown_controller.update(cx, BrowserController::shutdown);
                        let agent_shutdown = shutdown_agent_controller
                            .update(cx, AgentController::shutdown);
                        async move {
                            let agent_clean = agent_shutdown.await;
                            let clean = shutdown.await && agent_clean;
                            if verbose {
                                log::info!(
                                    target: "zz::diagnostics::lifecycle",
                                    "application shutdown complete clean={clean}"
                                );
                                log::logger().flush();
                            }
                        }
                    })
                    .detach();

                    let controller = controller.clone();
                    let mux = mux.clone();
                    let appearance_mux = mux.clone();
                    window
                        .observe_window_appearance(move |window, cx| {
                            theme::sync_system_appearance(Some(window), cx);
                            appearance_mux
                                .update(cx, |mux, _| {
                                    mux.set_color_scheme(terminal_color_scheme(window.appearance()));
                                });
                        })
                        .detach();
                    let close_controller = controller.clone();
                    let close_agent_controller = agent_controller.clone();
                    let close_window_state = window_state.clone();
                    let close_mux = mux.clone();
                    window.on_window_should_close(cx, move |window, cx| {
                        close_window_state.capture_and_flush(window, cx);
                        if tray::hide_or_quit(&close_mux, window.window_handle(), cx) {
                            return false;
                        }
                        request_window_close(
                            &close_controller,
                            &close_agent_controller,
                            window,
                            cx,
                        )
                    });
                    let view = cx.new(|cx| {
                        AppView::new(
                            controller.clone(),
                            agent_controller.clone(),
                            mux.clone(),
                            window,
                            cx,
                        )
                    });
                    let shell = cx
                        .new(|cx| AppShell::new(view, controller, agent_controller, window, cx));
                    let observed_window_state = window_state.clone();
                    window
                        .subscribe(
                            &mux,
                            cx,
                            |mux, _: &mux::client::InitialConnectionFinished, window, cx| {
                                window.defer(cx, move |window, cx| {
                                    if !workspace::maybe_prompt_stale_daemon(&mux, window, cx) {
                                        config::import_prompt::maybe_prompt(window, cx);
                                    }
                                });
                            },
                        )
                        .detach();
                    cx.new(|cx| {
                        let root = build_root(shell, window, cx);
                        cx.observe_window_activation(window, |_, window, cx| {
                            tray::set_active(window.is_window_active(), cx);
                        }).detach();
                        window::state::observe(observed_window_state, window, cx);
                        root
                    })
                },
            )
            .expect("failed to open zz window");
            window::toast::set_host(main_window, cx);
            cx.activate(true);
        });
}

#[cfg(not(target_os = "ios"))]
fn toggle_from_tray(main_window: gpui::AnyWindowHandle, cx: &mut App) {
    let (visible, active) = main_window
        .update(cx, |_, window, _| {
            (window.is_window_visible(), window.is_window_active())
        })
        .unwrap_or((true, false));
    match tray::toggle_action(visible, active) {
        tray::ToggleAction::Hide => tray::hide(main_window, cx),
        tray::ToggleAction::Raise | tray::ToggleAction::Show => tray::show(main_window, cx),
    }
}

pub fn build_root(view: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Root>) -> Root {
    config::observe_window_background(window, cx);
    let root = Root::new(view, window, cx).bg(gpui::transparent_black());
    #[cfg(target_os = "linux")]
    {
        root.bordered(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        root
    }
}

#[must_use]
fn request_window_close(
    controller: &Entity<BrowserController>,
    agent_controller: &Entity<AgentController>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let browser_complete = controller.read(cx).is_shutdown_complete();
    let agent_complete = agent_controller.read(cx).is_shutdown_complete();
    if browser_complete && agent_complete {
        return true;
    }
    let browser_shutting_down = controller.read(cx).is_shutting_down();
    let agent_shutting_down = agent_controller.read(cx).is_shutting_down();
    if !browser_shutting_down || !agent_shutting_down {
        let browser_shutdown = controller.update(cx, BrowserController::shutdown);
        let agent_shutdown = agent_controller.update(cx, AgentController::shutdown);
        let window_handle = window.window_handle();
        cx.spawn(async move |cx| {
            let agent_clean = agent_shutdown.await;
            if browser_shutdown.await && agent_clean {
                let _ = window_handle.update(cx, |_, window, _| {
                    window.remove_window();
                });
            }
        })
        .detach();
    }
    false
}

pub fn terminal_color_scheme(appearance: WindowAppearance) -> TerminalColorScheme {
    match appearance {
        WindowAppearance::Light | WindowAppearance::VibrantLight => TerminalColorScheme::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => TerminalColorScheme::Dark,
    }
}

#[cfg(target_os = "macos")]
fn mark_macos_app_as_background_only() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(main_thread) = MainThreadMarker::new() else {
        log::warn!("could not hide macOS Dock activity outside the main thread");
        return;
    };
    let app = NSApplication::sharedApplication(main_thread);
    if !app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited) {
        log::warn!("macOS rejected the background-only activation policy");
    }
}

#[cfg(not(target_os = "ios"))]
fn connect_interactive_client(
    path: &Path,
    color_scheme: TerminalColorScheme,
) -> Result<InteractiveClient, DaemonError> {
    zz_cli::connect_interactive_client_with_config(path, color_scheme, &[])
}
#[cfg(target_os = "ios")]
fn connect_interactive_client(
    path: &Path,
    color_scheme: TerminalColorScheme,
) -> Result<InteractiveClient, DaemonError> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match InteractiveClient::connect_with_color_scheme(path, color_scheme) {
            Ok(client) => {
                log::info!(
                    target: "zz::diagnostics::process",
                    "connected to daemon path={} server_hello={:#?}",
                    path.display(),
                    client.server_hello(),
                );
                return Ok(client);
            }
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[cfg(all(not(target_os = "ios"), not(target_os = "windows")))]
fn tui_browser_provider() -> Option<Box<dyn zz_tui::browser::BrowserFrameProvider>> {
    #[cfg(target_os = "macos")]
    if !macos_cef_framework_is_available() {
        log::warn!(target: "zz::browser::tui", "the bundled CEF framework is unavailable");
        eprintln!("zz attach: browser panes unavailable: the bundled CEF framework is missing");
        return None;
    }

    let profile_paths = match zz_browser::resolve_profile_paths() {
        Ok(paths) => browser::tui::tui_profile_paths(&paths),
        Err(error) => {
            log::error!(target: "zz::browser::tui", "could not resolve the TUI browser cache: {error}");
            eprintln!("zz attach: browser panes unavailable: could not prepare the browser cache");
            return None;
        }
    };
    match zz_browser::bootstrap_with_profile_paths(profile_paths) {
        Ok(BrowserBootstrap::Runtime(mut runtime)) => {
            runtime.set_log_file(diagnostics::cef_log_file());
            Some(Box::new(browser::tui::TuiBrowserProvider::new(runtime)))
        }
        Ok(BrowserBootstrap::SubprocessExit(code)) => {
            log::error!(
                target: "zz::browser::tui",
                "unexpected CEF subprocess exit while attaching: {code}"
            );
            eprintln!("zz attach: browser panes unavailable: CEF exited during startup");
            None
        }
        Err(error) => {
            log::error!(target: "zz::browser::tui", "could not prepare CEF for TUI browsers: {error}");
            eprintln!("zz attach: browser panes unavailable: CEF could not start");
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn tui_browser_provider() -> Option<Box<dyn zz_tui::browser::BrowserFrameProvider>> {
    None
}

#[cfg(test)]
mod tests {
    use gpui::WindowAppearance;
    use zz_terminal::TerminalColorScheme;

    use super::terminal_color_scheme;

    #[test]
    fn gpui_appearance_maps_to_terminal_theme_variants() {
        assert_eq!(
            terminal_color_scheme(WindowAppearance::Light),
            TerminalColorScheme::Light
        );
        assert_eq!(
            terminal_color_scheme(WindowAppearance::VibrantLight),
            TerminalColorScheme::Light
        );
        assert_eq!(
            terminal_color_scheme(WindowAppearance::Dark),
            TerminalColorScheme::Dark
        );
        assert_eq!(
            terminal_color_scheme(WindowAppearance::VibrantDark),
            TerminalColorScheme::Dark
        );
    }
}
