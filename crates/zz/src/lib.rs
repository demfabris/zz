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
/// A CLI verb, so it belongs to the platforms that have a command line.
#[cfg(not(target_os = "ios"))]
mod fleet;
mod keymap;
#[cfg(target_os = "macos")]
mod macos_app;
mod mux;
mod pane;
mod profile;
mod terminal;
mod theme;
/// A desktop menu bar / notification area; the iPad has neither.
#[cfg(not(target_os = "ios"))]
mod tray;
mod ui_scale;
mod user_data;
mod window;
mod workspace;

#[cfg(not(target_os = "ios"))]
use std::{
    io::ErrorKind,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
};
use std::{
    path::Path,
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
use zz_daemon::default_socket_path;
#[cfg(not(target_os = "ios"))]
use zz_daemon::{
    CommandClient, Daemon, Endpoint, classify_local_connect_error, terminate_incompatible_daemon,
};
use zz_daemon::{DaemonError, InteractiveClient};
#[cfg(not(target_os = "ios"))]
use zz_protocol::{
    CommandInvocation, MAX_AGENT_SEND_BYTES, PROTOCOL_VERSION, ServerError, ServerHello,
};
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

#[cfg(not(target_os = "ios"))]
enum Startup {
    Application(PathBuf),
    Exit(ExitCode),
}

#[cfg(not(target_os = "ios"))]
fn run_startup(socket_path: PathBuf) -> Startup {
    diagnostics::init();
    let arguments = match application_arguments(diagnostics::application_args(), socket_path) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("zz: {error}");
            return Startup::Exit(ExitCode::FAILURE);
        }
    };
    let ApplicationArguments {
        socket_path,
        socket_overridden,
        host,
        remaining,
    } = arguments;
    match run_command_mode(remaining, &socket_path, socket_overridden, host.as_deref()) {
        Some(exit) => Startup::Exit(exit),
        None => Startup::Application(socket_path),
    }
}

/// Start the native executable on Linux and macOS.
#[cfg(not(any(target_os = "windows", target_os = "ios")))]
#[must_use]
pub fn run() -> ExitCode {
    #[cfg(unix)]
    if let Some(exit) = run_askpass_mode() {
        return exit;
    }
    let mut socket_path = default_socket_path();
    if !is_cef_subprocess() {
        socket_path = match run_startup(socket_path) {
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
    if let Some(exit) = run_askpass_mode() {
        return exit;
    }
    match run_startup(default_socket_path()) {
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
    if let Some(exit) = run_askpass_mode() {
        return if exit == ExitCode::SUCCESS { 0 } else { 1 };
    }
    attach_parent_console();
    let mut socket_path = default_socket_path();
    if !is_cef_subprocess() {
        socket_path = match run_startup(socket_path) {
            Startup::Application(socket_path) => socket_path,
            Startup::Exit(exit) => return if exit == ExitCode::SUCCESS { 0 } else { 1 },
        };
    }
    let result = zz_browser::bootstrap_windows(instance, sandbox_info.cast());
    i32::from(finish_bootstrap(result, socket_path, AppProfile::desktop()))
}

#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    #[allow(
        unsafe_code,
        reason = "AttachConsole is a raw Win32 entry point with no safe wrapper"
    )]
    // SAFETY: no pointers are involved; the call attaches this process to its
    // parent's console or fails when there is none.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Answer one ssh prompt and exit, when `zz` was run as ssh's askpass helper.
///
/// Runs before everything else and must start nothing: any process that outlives
/// it holds ssh's answer pipe open forever.
#[cfg(all(any(unix, windows), not(target_os = "ios")))]
fn run_askpass_mode() -> Option<ExitCode> {
    let socket = std::env::var_os(zz_daemon::ASKPASS_SOCKET_ENV)?;
    let prompt = std::env::args_os().nth(1).unwrap_or_default();
    Some(zz_daemon::run_helper(
        Path::new(&socket),
        &prompt.to_string_lossy(),
    ))
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
#[derive(Debug, PartialEq, Eq)]
struct ApplicationArguments {
    socket_path: PathBuf,
    socket_overridden: bool,
    host: Option<String>,
    remaining: Vec<String>,
}

#[cfg(not(target_os = "ios"))]
fn application_arguments(
    arguments: impl IntoIterator<Item = String>,
    default_path: PathBuf,
) -> Result<ApplicationArguments, String> {
    let mut socket_path = default_path;
    let mut socket_overridden = false;
    let mut host = None;
    let mut remaining = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == diagnostics::SOCKET_ARGUMENT {
            let path = arguments
                .next()
                .ok_or_else(|| "--socket requires a path".to_owned())?;
            if path.is_empty() {
                return Err("--socket requires a non-empty path".to_owned());
            }
            socket_path = PathBuf::from(path);
            socket_overridden = true;
        } else if let Some(path) = argument
            .strip_prefix(diagnostics::SOCKET_ARGUMENT)
            .and_then(|argument| argument.strip_prefix('='))
        {
            if path.is_empty() {
                return Err("--socket requires a non-empty path".to_owned());
            }
            socket_path = PathBuf::from(path);
            socket_overridden = true;
        } else if argument == "--host" {
            let name = arguments
                .next()
                .ok_or_else(|| "--host requires a name".to_owned())?;
            if name.is_empty() {
                return Err("--host requires a non-empty name".to_owned());
            }
            host = Some(name);
        } else if let Some(name) = argument.strip_prefix("--host=") {
            if name.is_empty() {
                return Err("--host requires a non-empty name".to_owned());
            }
            host = Some(name.to_owned());
        } else {
            remaining.push(argument);
        }
    }
    if socket_overridden && host.is_some() {
        return Err("--host cannot be used together with --socket".to_owned());
    }
    Ok(ApplicationArguments {
        socket_path,
        socket_overridden,
        host,
        remaining,
    })
}

#[cfg(not(target_os = "ios"))]
fn run_command_mode(
    arguments: Vec<String>,
    socket_path: &Path,
    socket_overridden: bool,
    host: Option<&str>,
) -> Option<ExitCode> {
    let mut args = arguments.into_iter();
    let Some(command) = args.next() else {
        if host.is_some() {
            eprintln!("zz: --host requires a command");
            return Some(ExitCode::FAILURE);
        }
        return None;
    };
    if is_version_command(&command) {
        println!("zz {}", env!("CARGO_PKG_VERSION"));
        return Some(ExitCode::SUCCESS);
    }
    if command == "protocol-version" {
        return Some(
            match protocol_version_output(args, host, socket_overridden) {
                Ok(version) => {
                    println!("{version}");
                    ExitCode::SUCCESS
                }
                Err(usage) => {
                    eprintln!("zz: {usage}");
                    ExitCode::FAILURE
                }
            },
        );
    }

    if host.is_some() && matches!(command.as_str(), "daemon" | "proxy" | "fleet") {
        eprintln!("zz: --host is not supported for `{command}`");
        return Some(ExitCode::FAILURE);
    }

    if command == "daemon" {
        return Some(match Daemon::new(socket_path).run_foreground() {
            Ok(()) | Err(DaemonError::AlreadyRunning(_)) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("zz daemon: {error}");
                ExitCode::FAILURE
            }
        });
    }

    if command == "proxy" {
        return Some(match zz_daemon::run_socket_proxy(socket_path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("zz proxy: {error}");
                ExitCode::FAILURE
            }
        });
    }

    if command == "fleet" {
        return Some(match fleet::run(args) {
            Ok(output) => {
                if !output.is_empty() {
                    println!("{output}");
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("zz: {error}");
                ExitCode::FAILURE
            }
        });
    }

    if is_kill_server_command(&command) && host.is_none() {
        return Some(run_kill_server(socket_path, args));
    }

    if command == "attach" {
        let mut restart_daemon = false;
        let mut session = None;
        for argument in args {
            if argument == "--restart-daemon" && !restart_daemon {
                restart_daemon = true;
            } else if session.is_none() && !argument.starts_with('-') {
                session = Some(argument);
            } else {
                eprintln!("zz: usage: zz [--host <name>] attach [--restart-daemon] [session]");
                return Some(ExitCode::FAILURE);
            }
        }
        if restart_daemon && host.is_some() {
            eprintln!("zz: --restart-daemon is only supported for the local daemon");
            return Some(ExitCode::FAILURE);
        }
        let options = zz_tui::RunOptions {
            socket_path: socket_path.to_path_buf(),
            host: host.map(str::to_owned),
            session,
            restart_daemon,
        };
        let browser_provider = tui_browser_provider();
        let reconnect = |path: &Path| connect_interactive_client(path, TerminalColorScheme::Dark);
        let request = options
            .with_browser_provider(browser_provider)
            .with_local_reconnect(&reconnect);
        return Some(match zz_tui::run(request) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("zz attach: {error}");
                ExitCode::FAILURE
            }
        });
    }

    if host.is_some() && matches!(command.as_str(), "attach" | "attach-session") {
        eprintln!("zz: --host attach is not supported");
        return Some(ExitCode::FAILURE);
    }

    let mut args = args.collect::<Vec<_>>();
    if command == "agent-send" && zz_daemon::agent_send_reads_stdin(&args) {
        match read_stdin_payload() {
            Ok(payload) => {
                if !args.iter().any(|argument| argument == "--") {
                    args.push("--".to_owned());
                }
                args.push(payload);
            }
            Err(error) => {
                eprintln!("zz: {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    let mut client = match host.map_or_else(
        || connect_command_client(socket_path).map_err(format_local_daemon_error),
        connect_host_command_client,
    ) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("zz: {error}");
            return Some(ExitCode::FAILURE);
        }
    };
    let command = if host.is_some() && command == "--kill-server" {
        "kill-server".to_owned()
    } else {
        command
    };
    match client.execute(CommandInvocation::new(command, args)) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            Some(ExitCode::SUCCESS)
        }
        Err(DaemonError::CommandExit { output, exit_code }) => {
            if !output.is_empty() {
                println!("{output}");
            }
            Some(ExitCode::from(exit_code))
        }
        Err(error) => {
            eprintln!("{}", command_error_message(&error));
            Some(ExitCode::FAILURE)
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn read_stdin_payload() -> Result<String, String> {
    use std::io::Read as _;

    let limit = u64::try_from(MAX_AGENT_SEND_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut payload = String::new();
    std::io::stdin()
        .lock()
        .take(limit)
        .read_to_string(&mut payload)
        .map_err(|error| format!("could not read standard input: {error}"))?;
    Ok(payload)
}

#[cfg(not(target_os = "ios"))]
fn is_kill_server_command(command: &str) -> bool {
    matches!(command, "kill-server" | "--kill-server")
}

#[cfg(not(target_os = "ios"))]
fn is_version_command(command: &str) -> bool {
    matches!(command, "--version" | "-V")
}

#[cfg(not(target_os = "ios"))]
fn protocol_version_output(
    mut args: impl Iterator<Item = String>,
    host: Option<&str>,
    socket_overridden: bool,
) -> Result<String, &'static str> {
    if host.is_some() || socket_overridden || args.next().is_some() {
        return Err("usage: zz protocol-version");
    }
    Ok(PROTOCOL_VERSION.to_string())
}

#[cfg(not(target_os = "ios"))]
fn format_local_daemon_error(error: DaemonError) -> String {
    match error {
        error @ DaemonError::IncompatibleDaemon { .. } => {
            format!("{error}\nrun 'zz kill-server' to restart it (sessions will be lost)")
        }
        error => error.to_string(),
    }
}

#[cfg(not(target_os = "ios"))]
fn command_error_message(error: &DaemonError) -> String {
    match error {
        DaemonError::Server(ServerError::InvalidCommand(message))
            if message == "no current client" =>
        {
            message.clone()
        }
        error => format!("zz: {error}"),
    }
}

#[cfg(not(target_os = "ios"))]
fn run_kill_server(path: &Path, args: impl IntoIterator<Item = String>) -> ExitCode {
    let invocation = CommandInvocation::new("kill-server", args);
    let failure = match CommandClient::connect(path) {
        Ok(mut client) => match client.execute(invocation) {
            Ok(output) => {
                if !output.is_empty() {
                    println!("{output}");
                }
                return ExitCode::SUCCESS;
            }
            Err(error) => error,
        },
        Err(error) if daemon_is_missing(&error) => {
            eprintln!("zz: no daemon is running at {}", path.display());
            return ExitCode::FAILURE;
        }
        Err(error) => error,
    };

    log::warn!(
        target: "zz::diagnostics::process",
        "graceful kill-server failed path={} error={failure}; attempting verified recovery",
        path.display(),
    );
    match terminate_incompatible_daemon(path) {
        Ok(recovered) => {
            eprintln!("zz: terminated incompatible daemon pid {}", recovered.pid());
            ExitCode::SUCCESS
        }
        Err(recovery) => {
            eprintln!("zz: {failure}; recovery failed: {recovery}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn daemon_is_missing(error: &DaemonError) -> bool {
    matches!(
        error,
        DaemonError::Io(error)
            if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused)
    )
}

#[cfg(not(target_os = "ios"))]
fn spawn_daemon(path: &Path, color_scheme: Option<TerminalColorScheme>) -> Result<(), DaemonError> {
    let executable = std::env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .arg(diagnostics::SOCKET_ARGUMENT)
        .arg(path)
        .arg("daemon");
    if let Some(color_scheme) = color_scheme {
        command.env("ZZ_COLOR_SCHEME", color_scheme.as_str());
    }
    diagnostics::configure_spawned_process(&mut command);
    #[cfg(unix)]
    detach_daemon_session(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    log::debug!(
        target: "zz::diagnostics::process",
        "spawned daemon path={} child_verbose_flag_applied={}",
        path.display(),
        diagnostics::enabled(),
    );
    Ok(())
}

/// A new process group alone is not enough: it stays in the launching
/// terminal's session with a controlling tty, and the daemon's own children
/// (the interactive login-shell PATH probe) can then stop the daemon's whole
/// process group through tty job control. A new session drops the tty.
#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "Command::pre_exec is the only way to give the daemon its own session"
)]
fn detach_daemon_session(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    // SAFETY: the hook only calls setsid, which is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            let _ = rustix::process::setsid();
            Ok(())
        });
    }
}

#[cfg(not(target_os = "ios"))]
fn connect_or_spawn_daemon<T>(
    path: &Path,
    color_scheme: Option<TerminalColorScheme>,
    connect: impl Fn() -> Result<T, DaemonError>,
    server_hello: impl for<'a> Fn(&'a T) -> &'a ServerHello,
) -> Result<T, DaemonError> {
    match connect() {
        Ok(client) => {
            log::debug!(
                target: "zz::diagnostics::process",
                "connected to existing daemon path={} server_hello={:#?}",
                path.display(),
                server_hello(&client),
            );
            return Ok(client);
        }
        Err(error) => match classify_local_connect_error(path, error) {
            DaemonError::Io(error)
                if matches!(
                    error.kind(),
                    ErrorKind::NotFound | ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset
                ) => {}
            error => return Err(error),
        },
    }

    spawn_daemon(path, color_scheme)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match connect() {
            Ok(client) => return Ok(client),
            Err(error) if Instant::now() >= deadline => {
                return Err(classify_local_connect_error(path, error));
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn connect_command_client(path: &Path) -> Result<CommandClient, DaemonError> {
    connect_or_spawn_daemon(
        path,
        None,
        || CommandClient::connect(path),
        CommandClient::server_hello,
    )
}

#[cfg(not(target_os = "ios"))]
fn connect_host_command_client(name: &str) -> Result<CommandClient, String> {
    let endpoint = configured_host_endpoint(name)?;
    CommandClient::connect_endpoint(&endpoint).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "ios"))]
fn configured_host_endpoint(name: &str) -> Result<Endpoint, String> {
    let (hosts, _) = config::configured_fleet_hosts()
        .map_err(|error| format!("could not read zz/config: {error}"))?;
    if let Some(host) = hosts.iter().find(|host| host.name == name) {
        return Ok(host.endpoint.clone());
    }
    let known = if hosts.is_empty() {
        "(none)".to_owned()
    } else {
        hosts
            .iter()
            .map(|host| host.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Err(format!("unknown fleet host `{name}`; known hosts: {known}"))
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
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            cx.set_global(profile);
            diagnostics::start_main_thread_watchdog(cx);
            #[cfg(target_os = "macos")]
            cx.activate(true);
            config::init(cx);
            window::background::detect_compositor_support(cx);
            browser::recent_pages::init(cx);
            zz_ui::init(cx);
            ui_scale::init(cx);
            config::settings::init(cx);
            browser::view::init(cx);
            editor::init(cx);
            #[cfg(target_os = "macos")]
            macos_app::init(cx);
            terminal::view::init(cx);
            workspace::init(cx);
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

            let tray = if config::tray_enabled(cx) {
                let (sender, receiver) = async_channel::unbounded();
                tray::spawn(sender).map(|tray| (tray, receiver))
            } else {
                None
            };
            let close_to_tray = tray.is_some();

            let minimum_window_size = size(px(480.0), px(320.0));
            let restored_window = window_state.restored_window(
                cx,
                size(px(1080.0), px(720.0)),
                minimum_window_size,
            );
            let window_decorations = config::window_decorations(cx);
            let mut titlebar = config::titlebar_options();
            if cfg!(target_os = "linux") {
                titlebar.title = Some("zz".into());
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
                    app_id: Some("zz".into()),
                    #[cfg(target_os = "linux")]
                    icon: Some(app_icon::x11_window_icon()),
                    ..Default::default()
                },
                move |window, cx| {
                    ui_scale::apply_to_new_window(window, cx);
                    theme::sync_system_appearance(Some(window), cx);
                    let color_scheme = terminal_color_scheme(window.appearance());
                    let mux = connect_interactive_client(&socket_path, color_scheme);
                    let mux = cx.new(|cx| {
                        mux::client::MuxClient::new_with_color_scheme(
                            mux,
                            socket_path.clone(),
                            color_scheme,
                            cx,
                        )
                    });

                    diagnostics::start_app_state_sampler(controller.clone(), mux.clone(), cx);
                    diagnostics::init_debug_mark(controller.clone(), mux.clone(), cx);

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
                        if config::quit_daemon_on_exit(cx) {
                            shutdown_mux
                                .read(cx)
                                .execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
                        } else {
                            shutdown_mux.update(cx, |mux, _| mux.detach());
                        }
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
                    window.on_window_should_close(cx, move |window, cx| {
                        close_window_state.capture_and_flush(window, cx);
                        if close_to_tray {
                            #[cfg(target_os = "macos")]
                            cx.hide();
                            #[cfg(not(target_os = "macos"))]
                            window.set_window_visible(false);
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
                    window.defer(cx, move |window, cx| {
                        if !workspace::maybe_prompt_stale_daemon(&mux, window, cx) {
                            config::import_prompt::maybe_prompt(window, cx);
                        }
                    });
                    cx.new(|cx| {
                        let root = build_root(shell, window, cx);
                        window::state::observe(observed_window_state, window, cx);
                        root
                    })
                },
            )
            .expect("failed to open zz window");
            window::toast::set_host(main_window, cx);
            if let Some((tray, receiver)) = tray {
                drain_tray(tray, receiver, main_window, cx);
            }
            cx.activate(true);
        });
}

#[cfg(not(target_os = "ios"))]
fn drain_tray(
    tray: tray::Tray,
    receiver: async_channel::Receiver<tray::TrayEvent>,
    main_window: gpui::WindowHandle<Root>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        let _tray = tray;
        while let Ok(event) = receiver.recv().await {
            cx.update(|cx| match event {
                tray::TrayEvent::Toggle => toggle_from_tray(main_window, cx),
                tray::TrayEvent::Quit => cx.quit(),
            });
        }
    })
    .detach();
}

#[cfg(not(target_os = "ios"))]
fn toggle_from_tray(main_window: gpui::WindowHandle<Root>, cx: &mut App) {
    let (visible, active) = main_window
        .update(cx, |_, window, _| {
            (window.is_window_visible(), window.is_window_active())
        })
        .unwrap_or((true, false));
    match tray::toggle_action(visible, active) {
        #[cfg(target_os = "macos")]
        tray::ToggleAction::Hide => cx.hide(),
        #[cfg(target_os = "macos")]
        tray::ToggleAction::Raise | tray::ToggleAction::Show => cx.activate(true),
        #[cfg(not(target_os = "macos"))]
        tray::ToggleAction::Hide => {
            let _ = main_window.update(cx, |_, window, _| window.set_window_visible(false));
        }
        #[cfg(not(target_os = "macos"))]
        tray::ToggleAction::Raise | tray::ToggleAction::Show => {
            let _ = main_window.update(cx, |_, window, _| {
                window.set_window_visible(true);
                window.activate_window();
            });
        }
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
    connect_or_spawn_daemon(
        path,
        Some(color_scheme),
        || InteractiveClient::connect_with_color_scheme(path, color_scheme),
        InteractiveClient::server_hello,
    )
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
    use std::{io, path::PathBuf};

    use gpui::WindowAppearance;
    use zz_terminal::TerminalColorScheme;

    use super::{
        application_arguments, command_error_message, daemon_is_missing, is_kill_server_command,
        protocol_version_output, terminal_color_scheme,
    };
    use zz_daemon::DaemonError;

    #[test]
    fn kill_server_accepts_command_and_flag_spellings_only() {
        assert!(is_kill_server_command("kill-server"));
        assert!(is_kill_server_command("--kill-server"));
        assert!(!is_kill_server_command("kill-session"));
        assert!(!is_kill_server_command("--kill-session"));
    }

    #[test]
    fn protocol_version_command_prints_only_the_wire_version() {
        assert_eq!(
            protocol_version_output(std::iter::empty(), None, false).unwrap(),
            zz_protocol::PROTOCOL_VERSION.to_string()
        );
        assert_eq!(
            protocol_version_output(["extra".to_owned()].into_iter(), None, false),
            Err("usage: zz protocol-version")
        );
        assert_eq!(
            protocol_version_output(std::iter::empty(), Some("remote"), false),
            Err("usage: zz protocol-version")
        );
        assert_eq!(
            protocol_version_output(std::iter::empty(), None, true),
            Err("usage: zz protocol-version")
        );
    }

    #[test]
    fn kill_server_only_classifies_absent_endpoints_as_missing() {
        assert!(daemon_is_missing(&DaemonError::Io(io::Error::from(
            io::ErrorKind::NotFound
        ))));
        assert!(daemon_is_missing(&DaemonError::Io(io::Error::from(
            io::ErrorKind::ConnectionRefused
        ))));
        assert!(!daemon_is_missing(&DaemonError::Io(io::Error::from(
            io::ErrorKind::ConnectionReset
        ))));
    }

    #[test]
    fn refresh_client_detached_error_has_no_zz_prefix() {
        assert_eq!(
            command_error_message(&DaemonError::Server(
                zz_protocol::ServerError::InvalidCommand("no current client".to_owned())
            )),
            "no current client"
        );
        assert_eq!(
            command_error_message(&DaemonError::Server(
                zz_protocol::ServerError::InvalidCommand("other".to_owned())
            )),
            "zz: mux command failed: invalid command: other"
        );
    }

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

    #[test]
    fn socket_flag_overrides_the_environment_resolved_path() {
        let parsed = application_arguments(
            [
                "--socket".to_owned(),
                "/tmp/forwarded.sock".to_owned(),
                "list-sessions".to_owned(),
            ],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/forwarded.sock"));
        assert_eq!(parsed.host, None);
        assert_eq!(parsed.remaining, ["list-sessions"]);

        let parsed = application_arguments(
            ["daemon".to_owned(), "--socket=/tmp/daemon.sock".to_owned()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/daemon.sock"));
        assert_eq!(parsed.host, None);
        assert_eq!(parsed.remaining, ["daemon"]);
    }

    #[test]
    fn host_flag_accepts_both_spellings_and_conflicts_with_socket() {
        let parsed = application_arguments(
            [
                "--host".to_owned(),
                "desktop".to_owned(),
                "list-sessions".to_owned(),
            ],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/default.sock"));
        assert_eq!(parsed.host.as_deref(), Some("desktop"));
        assert_eq!(parsed.remaining, ["list-sessions"]);

        let parsed = application_arguments(
            ["list-panes".to_owned(), "--host=gpu".to_owned()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.host.as_deref(), Some("gpu"));
        assert_eq!(parsed.remaining, ["list-panes"]);

        for arguments in [
            vec![
                "--host".to_owned(),
                "desktop".to_owned(),
                "--socket=/tmp/daemon.sock".to_owned(),
                "list-sessions".to_owned(),
            ],
            vec![
                "--socket".to_owned(),
                "/tmp/daemon.sock".to_owned(),
                "--host=desktop".to_owned(),
                "list-sessions".to_owned(),
            ],
        ] {
            assert!(application_arguments(arguments, PathBuf::from("/tmp/default.sock")).is_err());
        }
    }

    #[test]
    fn a_prompt_would_reach_the_command_dispatcher_if_askpass_mode_did_not_come_first() {
        let parsed = application_arguments(
            ["demfabris@xps's password: ".to_owned()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.remaining, ["demfabris@xps's password: "]);
    }

    #[test]
    fn socket_flag_requires_a_non_empty_path() {
        assert!(
            application_arguments(["--socket".to_owned()], PathBuf::from("/tmp/default.sock"))
                .is_err()
        );
        assert!(
            application_arguments(["--socket=".to_owned()], PathBuf::from("/tmp/default.sock"))
                .is_err()
        );
        assert!(
            application_arguments(["--host".to_owned()], PathBuf::from("/tmp/default.sock"))
                .is_err()
        );
        assert!(
            application_arguments(["--host=".to_owned()], PathBuf::from("/tmp/default.sock"))
                .is_err()
        );
    }
}
