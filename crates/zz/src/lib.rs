mod agent;
mod app_icon;
mod app_shell;
mod browser;
mod chooser;
mod command;
mod config;
#[cfg(not(target_os = "ios"))]
mod control_mode;
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

#[cfg(not(target_os = "ios"))]
use std::{
    borrow::Cow,
    io::{self, ErrorKind, Write as _},
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
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
    CommandClient, CommandOutcome, Daemon, Endpoint, classify_local_connect_error,
    terminate_incompatible_daemon,
};
use zz_daemon::{DaemonError, InteractiveClient};
#[cfg(not(target_os = "ios"))]
use zz_mux::{MuxEngine, format_command};
#[cfg(not(target_os = "ios"))]
use zz_protocol::{
    CommandInvocation, MAX_AGENT_SEND_BYTES, MAX_CLIENT_WORKING_DIRECTORY_BYTES, PROTOCOL_VERSION,
    PreparedCommand, PreparedCommandResult, RawText, ServerError, ServerHello, canonical_command,
    catalog_command_spec,
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
const TMUX_VERSION_OUTPUT: &str = zz_protocol::CommandSpec::TMUX_VERSION_OUTPUT;
#[cfg(not(target_os = "ios"))]
const TMUX_USAGE: &str = concat!(
    "usage: zz [-2CDhlNuVv] [-c shell-command] [-f file] [-L socket-name]\n",
    "            [-S socket-path] [-T features] [command [flags]]"
);
#[cfg(not(target_os = "ios"))]
const NATIVE_ATTACH_USAGE: &str = "zz: usage: zz [--host <name>] attach [--restart-daemon] [-dEr] [-c working-directory] [-f flags] [session]";
#[cfg(not(target_os = "ios"))]
const NATIVE_APP_USAGE: &str = "zz: usage: zz app";
#[cfg(not(target_os = "ios"))]
const FOREIGN_TMUX_ERROR: &str = "zz: TMUX is set but ZZ_SOCKET is not; refusing to treat a tmux server as zz\nUse `zz app` to open the GUI, or pass `-S` / set `ZZ_SOCKET` to target a zz daemon.";
#[cfg(not(target_os = "ios"))]
const APP_STARTUP_DIRECTORY_ENV: &str = "ZZ_APP_STARTUP_DIRECTORY";
#[cfg(not(target_os = "ios"))]
/// Private handshake from the `zz` launcher on `PATH`: this process is a
/// command-line client, so an empty command line runs `default-client-command`
/// the way the pin's `server_client_default_command` does instead of opening
/// the desktop application.
const LAUNCHER_CLIENT_ARGUMENT: &str = "--bootstrap-launcher-client";

/// Who produced this command line. Only the launcher's empty command line is a
/// client with no command of its own; the application's own is the desktop app.
#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandLineOrigin {
    Launcher,
    Application,
}

const DAEMON_BOOTSTRAP_SERVER_ID_ARGUMENT: &str = "--bootstrap-server-id";
#[cfg(not(target_os = "ios"))]
const DAEMON_BOOTSTRAP_CLIENT_CWD_ARGUMENT: &str = "--bootstrap-client-cwd";
#[cfg(not(target_os = "ios"))]
static DAEMON_SPAWN_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(not(target_os = "ios"))]
enum Startup {
    Application(PathBuf),
    Exit(ExitCode),
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, Default, PartialEq, Eq)]
struct DaemonBootstrapArguments {
    server_id: Option<u64>,
    client_working_directory: Option<PathBuf>,
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, PartialEq, Eq)]
enum DaemonBootstrapArgumentError {
    ServerId,
    ClientWorkingDirectory,
}

#[cfg(not(target_os = "ios"))]
impl DaemonBootstrapArgumentError {
    fn message(&self) -> &'static str {
        match self {
            Self::ServerId => "invalid bootstrap server id",
            Self::ClientWorkingDirectory => "invalid bootstrap client cwd",
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn validated_bootstrap_client_working_directory(path: PathBuf) -> Option<PathBuf> {
    let value = path.to_str()?;
    (path.is_absolute() && value.len() <= MAX_CLIENT_WORKING_DIRECTORY_BYTES).then_some(path)
}

#[cfg(not(target_os = "ios"))]
fn parse_daemon_bootstrap_arguments(
    arguments: &[RawText],
) -> Result<DaemonBootstrapArguments, DaemonBootstrapArgumentError> {
    if arguments.is_empty() {
        return Ok(DaemonBootstrapArguments::default());
    }
    let [server_flag, server_id, remaining @ ..] = arguments else {
        return Err(DaemonBootstrapArgumentError::ServerId);
    };
    if server_flag != DAEMON_BOOTSTRAP_SERVER_ID_ARGUMENT {
        return Err(DaemonBootstrapArgumentError::ServerId);
    }
    let server_id = server_id
        .parse::<u64>()
        .map_err(|_| DaemonBootstrapArgumentError::ServerId)?;
    let client_working_directory = match remaining {
        [] => None,
        [cwd_flag, cwd] if cwd_flag == DAEMON_BOOTSTRAP_CLIENT_CWD_ARGUMENT => Some(
            validated_bootstrap_client_working_directory(PathBuf::from(cwd.to_os_string()))
                .ok_or(DaemonBootstrapArgumentError::ClientWorkingDirectory)?,
        ),
        _ => return Err(DaemonBootstrapArgumentError::ClientWorkingDirectory),
    };
    Ok(DaemonBootstrapArguments {
        server_id: Some(server_id),
        client_working_directory,
    })
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, PartialEq, Eq)]
struct NativeAttachArguments {
    restart_daemon: bool,
    detach_others: bool,
    detach_others_hangup: bool,
    no_update_environment: bool,
    read_only: bool,
    client_flags: Option<String>,
    working_directory: Option<String>,
    session: Option<String>,
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, PartialEq, Eq)]
enum NativeAttachArgumentError {
    Usage,
    Command(ServerError),
}

#[cfg(not(target_os = "ios"))]
struct TmuxLabelCreationError {
    message: String,
    kind: ErrorKind,
}

#[cfg(not(target_os = "ios"))]
fn run_startup(socket_path: PathBuf) -> Startup {
    diagnostics::init();
    let arguments = match application_arguments(diagnostics::application_args(), socket_path) {
        Ok(arguments) => arguments,
        Err(ApplicationArgumentError::Message(error)) => {
            eprintln!("zz: {error}");
            return Startup::Exit(ExitCode::FAILURE);
        }
        Err(ApplicationArgumentError::Raw(error)) => {
            eprintln!("{error}");
            return Startup::Exit(ExitCode::FAILURE);
        }
        Err(ApplicationArgumentError::Usage) => {
            eprintln!("{TMUX_USAGE}");
            return Startup::Exit(ExitCode::FAILURE);
        }
    };
    let ApplicationArguments {
        socket_path,
        socket_source,
        host,
        remaining,
        mux_config_files,
        no_start_server,
        control_mode,
        shell_command,
        login_shell,
        origin,
        early_output,
    } = arguments;
    let implicit_tmux_conflict = implicit_tmux_endpoint_conflict(
        socket_source,
        std::env::var_os("ZZ_SOCKET").as_deref(),
        std::env::var_os("TMUX").as_deref(),
    );
    if let Some(output) = early_output {
        println!("{output}");
        return Startup::Exit(ExitCode::SUCCESS);
    }
    if control_mode != 0 && shell_command.is_none() {
        if host.is_some() {
            eprintln!("zz: --host is not supported with control mode");
            return Startup::Exit(ExitCode::FAILURE);
        }
        if implicit_tmux_conflict {
            eprintln!("{FOREIGN_TMUX_ERROR}");
            return Startup::Exit(ExitCode::FAILURE);
        }
        return Startup::Exit(control_mode::run(
            &socket_path,
            socket_source,
            &mux_config_files,
            no_start_server,
            control_mode,
            remaining,
        ));
    }
    if let Some(exit) = run_command_mode(
        &remaining,
        &socket_path,
        socket_source,
        host.as_deref(),
        &mux_config_files,
        no_start_server,
        shell_command.as_deref(),
        login_shell,
        origin,
        implicit_tmux_conflict,
    ) {
        Startup::Exit(exit)
    } else {
        configure_application_working_directory();
        Startup::Application(socket_path)
    }
}

#[cfg(not(target_os = "ios"))]
fn application_working_directory(
    launched: Option<&Path>,
    current: Option<&Path>,
    home: Option<&Path>,
    home_from_root: bool,
) -> Option<PathBuf> {
    launched
        .filter(|directory| directory.is_dir())
        .or_else(|| {
            current.filter(|directory| {
                directory.is_dir() && (!home_from_root || *directory != Path::new("/"))
            })
        })
        .or_else(|| home.filter(|directory| directory.is_dir()))
        .or_else(|| current.filter(|directory| directory.is_dir()))
        .map(Path::to_owned)
}

#[cfg(not(target_os = "ios"))]
fn configure_application_working_directory() {
    let launched = std::env::var_os(APP_STARTUP_DIRECTORY_ENV).map(PathBuf::from);
    let current = std::env::current_dir().ok();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let selected = application_working_directory(
        launched.as_deref(),
        current.as_deref(),
        home.as_deref(),
        cfg!(target_os = "macos"),
    );
    let Some(selected) = selected.filter(|selected| current.as_deref() != Some(selected.as_path()))
    else {
        return;
    };
    if let Err(error) = std::env::set_current_dir(&selected) {
        log::warn!(
            target: "zz::diagnostics::process",
            "could not use application working directory path={} error={error}",
            selected.display(),
        );
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
    socket_source: SocketSelectionSource,
    host: Option<String>,
    remaining: Vec<RawText>,
    mux_config_files: Vec<PathBuf>,
    no_start_server: bool,
    control_mode: u8,
    shell_command: Option<String>,
    login_shell: bool,
    origin: CommandLineOrigin,
    early_output: Option<&'static str>,
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, PartialEq, Eq)]
enum ApplicationArgumentError {
    Message(String),
    Raw(String),
    Usage,
}

#[cfg(not(target_os = "ios"))]
enum SocketSelection {
    Path(PathBuf),
    Label(String),
}

#[cfg(not(target_os = "ios"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SocketSelectionSource {
    Default,
    Path,
    Label,
}

#[cfg(not(target_os = "ios"))]
impl SocketSelectionSource {
    fn is_overridden(self) -> bool {
        self != Self::Default
    }
}

#[cfg(not(target_os = "ios"))]
fn application_arguments(
    arguments: impl IntoIterator<Item = RawText>,
    default_path: PathBuf,
) -> Result<ApplicationArguments, ApplicationArgumentError> {
    let mut socket_selection = None;
    let mut host = None;
    let mut remaining = Vec::new();
    let mut mux_config_files = Vec::new();
    let mut no_start_server = false;
    let mut shell_command = None;
    let mut login_shell = false;
    let mut control_mode = 0_u8;
    let mut foreground_server = false;
    let mut origin = CommandLineOrigin::Application;
    let mut parsing_tmux_options = true;
    let mut arguments = arguments.into_iter().collect::<Vec<_>>().into_iter();
    while let Some(argument) = arguments.next() {
        if parsing_tmux_options && argument == LAUNCHER_CLIENT_ARGUMENT {
            origin = CommandLineOrigin::Launcher;
        } else if argument == diagnostics::SOCKET_ARGUMENT {
            let path = arguments.next().ok_or_else(|| {
                ApplicationArgumentError::Message("--socket requires a path".to_owned())
            })?;
            if path.is_empty() {
                return Err(ApplicationArgumentError::Message(
                    "--socket requires a non-empty path".to_owned(),
                ));
            }
            socket_selection = Some(SocketSelection::Path(PathBuf::from(path.to_os_string())));
        } else if let Some(path) = argument
            .strip_prefix(diagnostics::SOCKET_ARGUMENT)
            .and_then(|argument| argument.strip_prefix('='))
        {
            if path.is_empty() {
                return Err(ApplicationArgumentError::Message(
                    "--socket requires a non-empty path".to_owned(),
                ));
            }
            socket_selection = Some(SocketSelection::Path(PathBuf::from(path)));
        } else if argument == "--host" {
            let name = arguments.next().ok_or_else(|| {
                ApplicationArgumentError::Message("--host requires a name".to_owned())
            })?;
            if name.is_empty() {
                return Err(ApplicationArgumentError::Message(
                    "--host requires a non-empty name".to_owned(),
                ));
            }
            host = Some(name.to_string());
        } else if let Some(name) = argument.strip_prefix("--host=") {
            if name.is_empty() {
                return Err(ApplicationArgumentError::Message(
                    "--host requires a non-empty name".to_owned(),
                ));
            }
            host = Some(name.into());
        } else if parsing_tmux_options && argument == "--" {
            parsing_tmux_options = false;
        } else if parsing_tmux_options && matches!(argument.as_str(), "--version" | "--kill-server")
        {
            parsing_tmux_options = false;
            remaining.push(argument);
        } else if parsing_tmux_options && argument.starts_with("--") {
            return Err(ApplicationArgumentError::Usage);
        } else if parsing_tmux_options && argument.starts_with('-') && argument != "-" {
            let options = &argument[1..];
            for (index, option) in options.char_indices() {
                let value = |arguments: &mut std::vec::IntoIter<RawText>| {
                    let value_index = index + option.len_utf8();
                    if value_index < options.len() {
                        Ok(RawText::from(&options[value_index..]))
                    } else {
                        arguments.next().ok_or_else(|| {
                            ApplicationArgumentError::Raw(format!(
                                "zz: option requires an argument -- {option}\n{TMUX_USAGE}"
                            ))
                        })
                    }
                };
                match option {
                    '2' | 'q' | 'u' | 'v' => {}
                    'c' => {
                        shell_command = Some(value(&mut arguments)?.to_string());
                        break;
                    }
                    'C' => control_mode = control_mode.saturating_add(1),
                    'D' => foreground_server = true,
                    'f' => {
                        mux_config_files.push(PathBuf::from(value(&mut arguments)?.to_os_string()));
                        break;
                    }
                    'h' => {
                        return Ok(ApplicationArguments {
                            socket_path: default_path,
                            socket_source: SocketSelectionSource::Default,
                            host: None,
                            remaining: Vec::new(),
                            mux_config_files: Vec::new(),
                            no_start_server: false,
                            control_mode: 0,
                            shell_command: None,
                            login_shell: false,
                            origin: CommandLineOrigin::Application,
                            early_output: Some(TMUX_USAGE),
                        });
                    }
                    'l' => login_shell = true,
                    'L' => {
                        socket_selection =
                            Some(SocketSelection::Label(value(&mut arguments)?.to_string()));
                        break;
                    }
                    'N' => no_start_server = true,
                    'S' => {
                        socket_selection = Some(SocketSelection::Path(PathBuf::from(
                            value(&mut arguments)?.to_os_string(),
                        )));
                        break;
                    }
                    'T' => {
                        let _ = value(&mut arguments)?;
                        break;
                    }
                    'V' => {
                        return Ok(ApplicationArguments {
                            socket_path: default_path,
                            socket_source: SocketSelectionSource::Default,
                            host: None,
                            remaining: Vec::new(),
                            mux_config_files: Vec::new(),
                            no_start_server: false,
                            control_mode: 0,
                            shell_command: None,
                            login_shell: false,
                            origin: CommandLineOrigin::Application,
                            early_output: Some(TMUX_VERSION_OUTPUT),
                        });
                    }
                    _ => {
                        return Err(ApplicationArgumentError::Raw(format!(
                            "zz: unknown option -- {option}\n{TMUX_USAGE}"
                        )));
                    }
                }
            }
        } else {
            parsing_tmux_options = false;
            remaining.push(argument);
        }
    }
    if shell_command.is_some() && !remaining.is_empty() {
        return Err(ApplicationArgumentError::Usage);
    }
    if foreground_server && !remaining.is_empty() {
        return Err(ApplicationArgumentError::Usage);
    }
    if foreground_server {
        return Err(ApplicationArgumentError::Message(
            "-D foreground server mode is not supported; use `zz daemon`".to_owned(),
        ));
    }
    let socket_source = match &socket_selection {
        Some(SocketSelection::Path(_)) => SocketSelectionSource::Path,
        Some(SocketSelection::Label(_)) => SocketSelectionSource::Label,
        None => SocketSelectionSource::Default,
    };
    if socket_source.is_overridden() && host.is_some() {
        return Err(ApplicationArgumentError::Message(
            "--host cannot be used together with a socket selector".to_owned(),
        ));
    }
    let socket_path = match socket_selection {
        Some(SocketSelection::Path(path)) => path,
        Some(SocketSelection::Label(label)) => {
            tmux_label_socket_path(&label, std::env::var_os("TMUX_TMPDIR").as_deref())
                .map_err(ApplicationArgumentError::Raw)?
        }
        None => default_path,
    };
    Ok(ApplicationArguments {
        socket_path,
        socket_source,
        host,
        remaining,
        mux_config_files,
        no_start_server,
        control_mode,
        shell_command,
        login_shell,
        origin,
        early_output: None,
    })
}

#[cfg(not(target_os = "ios"))]
fn implicit_tmux_endpoint_conflict(
    socket_source: SocketSelectionSource,
    zz_socket: Option<&std::ffi::OsStr>,
    tmux: Option<&std::ffi::OsStr>,
) -> bool {
    socket_source == SocketSelectionSource::Default
        && zz_socket.is_none_or(std::ffi::OsStr::is_empty)
        && tmux.is_some_and(|value| !value.is_empty())
}

#[cfg(not(target_os = "ios"))]
#[cfg(unix)]
fn tmux_label_socket_path(
    label: &str,
    tmux_tmpdir: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};

    let root = tmux_socket_root(tmux_tmpdir).ok_or_else(|| "no suitable socket path".to_owned())?;
    let uid = rustix::process::getuid().as_raw();
    let base = root.join(format!("tmux-{uid}"));
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    if let Err(error) = builder.create(&base)
        && error.kind() != ErrorKind::AlreadyExists
    {
        return Err(format!(
            "couldn't create directory {} ({error})",
            base.display()
        ));
    }
    let metadata = std::fs::symlink_metadata(&base)
        .map_err(|error| format!("couldn't read directory {} ({error})", base.display()))?;
    if !metadata.file_type().is_dir() {
        return Err(format!("{} is not a directory", base.display()));
    }
    if metadata.uid() != uid || metadata.mode() & 0o007 != 0 {
        return Err(format!(
            "directory {} has unsafe permissions",
            base.display()
        ));
    }
    Ok(base.join(label))
}

#[cfg(not(target_os = "ios"))]
#[cfg(unix)]
fn tmux_socket_root(tmux_tmpdir: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    tmux_tmpdir
        .filter(|path| !path.is_empty())
        .and_then(|path| std::fs::canonicalize(Path::new(path)).ok())
        .or_else(|| std::fs::canonicalize("/tmp").ok())
}

#[cfg(not(target_os = "ios"))]
#[cfg(not(unix))]
fn tmux_label_socket_path(
    label: &str,
    tmux_tmpdir: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    let root = tmux_tmpdir
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let base = root.join("tmux-0");
    std::fs::create_dir_all(&base)
        .map_err(|error| format!("couldn't create directory {} ({error})", base.display()))?;
    Ok(base.join(label))
}

#[cfg(not(target_os = "ios"))]
fn run_command_mode(
    arguments: &[RawText],
    socket_path: &Path,
    socket_source: SocketSelectionSource,
    host: Option<&str>,
    mux_config_files: &[PathBuf],
    no_start_server: bool,
    shell_command: Option<&str>,
    login_shell: bool,
    origin: CommandLineOrigin,
    implicit_tmux_conflict: bool,
) -> Option<ExitCode> {
    if let Some(shell_command) = shell_command {
        if implicit_tmux_conflict && host.is_none() {
            eprintln!("{FOREIGN_TMUX_ERROR}");
            return Some(ExitCode::FAILURE);
        }
        return Some(run_tmux_shell_command(
            socket_path,
            socket_source,
            host,
            mux_config_files,
            no_start_server,
            shell_command,
            login_shell,
        ));
    }
    let mut command_chain = split_command_chain(arguments);
    if command_chain.is_empty() && origin == CommandLineOrigin::Launcher && host.is_none() {
        command_chain =
            default_client_command_chain(socket_path, mux_config_files, no_start_server);
    }
    let Some(invocation) = command_chain.first().cloned() else {
        if host.is_some() {
            eprintln!("zz: --host requires a command");
            return Some(ExitCode::FAILURE);
        }
        return None;
    };
    let command = invocation.name.clone();
    if command == "app" {
        if host.is_some() || !invocation.args.is_empty() || command_chain.len() != 1 {
            eprintln!("{NATIVE_APP_USAGE}");
            return Some(ExitCode::FAILURE);
        }
        return None;
    }
    if is_version_command(&command) {
        println!("zz {}", env!("CARGO_PKG_VERSION"));
        return Some(ExitCode::SUCCESS);
    }
    if command == "protocol-version" {
        return Some(
            match protocol_version_output(
                invocation.args.iter().map(ToString::to_string),
                host,
                socket_source.is_overridden(),
            ) {
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
        let bootstrap = match parse_daemon_bootstrap_arguments(&invocation.args) {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                eprintln!("zz daemon: {}", error.message());
                return Some(ExitCode::FAILURE);
            }
        };
        let mut daemon =
            Daemon::new(socket_path).with_mux_config_files(mux_config_files.iter().cloned());
        if let Some(server_id) = bootstrap.server_id {
            daemon = daemon.with_server_id(server_id);
        }
        if let Some(client_working_directory) = bootstrap.client_working_directory {
            daemon = daemon.with_initial_client_working_directory(client_working_directory);
        }
        return Some(match daemon.run_foreground() {
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
        return Some(
            match fleet::run(
                invocation
                    .args
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<String>>(),
            ) {
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
            },
        );
    }

    if implicit_tmux_conflict && host.is_none() {
        eprintln!("{FOREIGN_TMUX_ERROR}");
        return Some(ExitCode::FAILURE);
    }

    if command == "--kill-server" {
        return Some(match host {
            Some(host) => run_host_kill_server(host, invocation.args),
            None => run_kill_server(socket_path, invocation.args, true),
        });
    }

    let start_server = !no_start_server && tmux_command_starts_server(&command);
    let native_attach_spelling = matches!(command.as_str(), "attach" | "attach-session");
    let mut preparation_error = None;
    let mut prepared = if host.is_none() {
        match prepare_cli_command_chain(socket_path, &command_chain) {
            Ok(prepared) => Some(prepared),
            Err(error) => {
                preparation_error = Some(classify_local_connect_error(socket_path, error));
                None
            }
        }
    } else {
        None
    };

    if prepared.is_none() && host.is_none() {
        let static_commands = if native_attach_spelling {
            if let Err(error) = parse_native_attach_arguments(command_chain[0].args.clone()) {
                print_native_attach_argument_error(error);
                return Some(ExitCode::FAILURE);
            }
            &command_chain[1..]
        } else {
            &command_chain
        };
        if zz_mux::validate_static_command_chain(static_commands).is_err() {
            let error = preparation_error.expect("missing preparation error");
            eprintln!("{}", format_local_command_error(socket_path, error));
            return Some(ExitCode::FAILURE);
        }
    }

    if prepared.is_none()
        && !start_server
        && tmux_command_starts_server(&command)
        && preparation_error.as_ref().is_some_and(daemon_is_spawnable)
    {
        let error = preparation_error.expect("missing preparation error");
        eprintln!("{}", format_local_command_error(socket_path, error));
        return Some(ExitCode::FAILURE);
    }

    if prepared.is_none()
        && start_server
        && preparation_error.as_ref().is_some_and(daemon_is_spawnable)
    {
        if let Some(error) = tmux_label_creation_error(socket_path, socket_source, start_server) {
            eprintln!("{}", error.message);
            let nested_label_new_session =
                canonical_command(&command) == "new-session" && error.kind == ErrorKind::NotFound;
            return Some(if nested_label_new_session {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        let (mut client, spawned_server_id) =
            match connect_command_client_with_spawn_provenance(socket_path, mux_config_files) {
                Ok(connected) => connected,
                Err(error) => {
                    eprintln!("{}", format_local_command_error(socket_path, error));
                    return Some(ExitCode::FAILURE);
                }
            };
        let commands = match spawned_server_id {
            Some(server_id) => {
                client.prepare_commands_or_stop_empty(command_chain.clone(), server_id)
            }
            None => client.prepare_commands(command_chain.clone()),
        };
        let commands = match commands {
            Ok(commands) => commands,
            Err(error) => {
                eprintln!("{}", command_error_message(&error));
                return Some(ExitCode::FAILURE);
            }
        };
        prepared = Some(PreparedCliCommandChain { client, commands });
    }

    if let Some(error) = prepared
        .as_ref()
        .and_then(|prepared| prepared_command_error(&prepared.commands))
        .map(server_error_message)
    {
        eprintln!("{error}");
        return Some(ExitCode::FAILURE);
    }

    if command == "kill-server" && host.is_none() && prepared.is_none() {
        return Some(run_kill_server(socket_path, invocation.args, false));
    }

    let reads_stdin = prepared.as_ref().map_or_else(
        || command_reads_stdin(&command_chain[0]),
        |prepared| {
            prepared
                .commands
                .first()
                .is_some_and(prepared_command_reads_stdin)
        },
    );
    if reads_stdin {
        match read_stdin_payload() {
            Ok(payload) => {
                if let Some(prepared) = prepared.as_mut() {
                    append_prepared_command_stdin_payload(&mut prepared.commands[0], payload);
                } else {
                    let canonical_name = canonical_command(&command_chain[0].name).to_owned();
                    append_stdin_payload(&canonical_name, &mut command_chain[0].args, payload);
                }
            }
            Err(error) => {
                eprintln!("zz: {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    let new_session_tui = prepared.as_ref().map_or_else(
        || command_chain_uses_tui(&command_chain),
        |prepared| prepared_command_chain_uses_tui(&command_chain, &prepared.commands),
    );
    let attach_tui = prepared.as_ref().map_or_else(
        || attach_prefix_uses_tui(&command),
        |prepared| {
            prepared
                .commands
                .first()
                .is_some_and(|prepared| prepared_attach_uses_tui(&command, prepared))
        },
    );
    let tui_connection_allowed = host.is_some() || prepared.is_some() || start_server;
    if tui_connection_allowed && (new_session_tui || attach_tui) {
        let options = zz_tui::RunOptions {
            socket_path: socket_path.to_path_buf(),
            host: host.map(str::to_owned),
            session: None,
            restart_daemon: false,
            detach_others: false,
            read_only: false,
            client_flags: None,
        };
        let reconnect = |path: &Path, client_has_terminal| {
            connect_terminal_surface_client_with_config(
                path,
                TerminalColorScheme::Dark,
                mux_config_files,
                client_has_terminal,
            )
        };
        let request = options
            .with_browser_provider(tui_browser_provider)
            .with_local_reconnect(&reconnect);
        return Some(match prepared {
            Some(PreparedCliCommandChain { client, commands }) => {
                drop(client);
                match zz_tui::run_prepared_new_session(request, commands) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("{error}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => match zz_tui::run_new_session(request, command_chain) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            },
        });
    }

    let native_attach = prepared.as_ref().map_or_else(
        || matches!(command.as_str(), "attach" | "attach-session"),
        |prepared| {
            prepared
                .commands
                .first()
                .is_some_and(|prepared| prepared_native_attach(&command, prepared))
        },
    );
    if native_attach {
        let options = match parse_native_attach_arguments(command_chain[0].args.clone()) {
            Ok(options) => options,
            Err(error) => {
                print_native_attach_argument_error(error);
                return Some(ExitCode::FAILURE);
            }
        };
        if options.restart_daemon && host.is_some() {
            eprintln!("zz: --restart-daemon is only supported for the local daemon");
            return Some(ExitCode::FAILURE);
        }
        // -x has no RunOptions field: route it through the real attach command so
        // the daemon sees the flag that picks the parent-hangup eviction.
        let needs_command_attach = options.no_update_environment
            || options.working_directory.is_some()
            || options.detach_others_hangup;
        let attach_command = native_attach_command(&options);
        let options = zz_tui::RunOptions {
            socket_path: socket_path.to_path_buf(),
            host: host.map(str::to_owned),
            session: options.session,
            restart_daemon: options.restart_daemon,
            detach_others: options.detach_others,
            read_only: options.read_only,
            client_flags: options.client_flags,
        };
        let reconnect = |path: &Path, client_has_terminal| {
            connect_terminal_surface_client_with_config(
                path,
                TerminalColorScheme::Dark,
                mux_config_files,
                client_has_terminal,
            )
        };
        let request = options
            .with_browser_provider(tui_browser_provider)
            .with_local_reconnect(&reconnect);
        let result = if command_chain.len() > 1 || needs_command_attach {
            if let Some(PreparedCliCommandChain {
                client,
                mut commands,
            }) = prepared.take()
            {
                drop(client);
                commands[0].invocation = attach_command;
                zz_tui::run_prepared_new_session(request, commands)
            } else {
                command_chain[0] = attach_command;
                zz_tui::run_new_session(request, command_chain)
            }
        } else {
            if let Some(PreparedCliCommandChain { client, .. }) = prepared.take() {
                drop(client);
            }
            zz_tui::run(request)
        };
        return Some(match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        });
    }

    if let Some(error) = tmux_label_creation_error(socket_path, socket_source, start_server) {
        eprintln!("{}", error.message);
        let nested_label_new_session =
            canonical_command(&command) == "new-session" && error.kind == ErrorKind::NotFound;
        return Some(if nested_label_new_session {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    let connected = match host {
        Some(host) => connect_host_command_client(host)
            .map(|client| (client, None))
            .map_err(|error| format!("zz: {error}")),
        None => match prepared {
            Some(PreparedCliCommandChain { client, commands }) => Ok((client, Some(commands))),
            None => connect_command_client(socket_path, mux_config_files, start_server)
                .map(|client| (client, None))
                .map_err(|error| format_local_command_error(socket_path, error)),
        },
    };
    let (mut client, prepared_commands) = match connected {
        Ok(connected) => connected,
        Err(error) => {
            eprintln!("{error}");
            return Some(ExitCode::FAILURE);
        }
    };
    if let Some(prepared_commands) = prepared_commands {
        let recover_kill = prepared_commands
            .first()
            .is_some_and(|prepared| prepared_kill_server_recovery(&command, prepared));
        return match execute_command_chain(
            prepared_commands.into_iter().enumerate(),
            |(index, command)| {
                execute_prepared_command(&mut client, command).map_err(|error| (index, error))
            },
            |outcome| {
                print_command_output(&outcome.stdout);
                print_command_error(&outcome.stderr);
            },
        ) {
            Ok(exit_code) => Some(ExitCode::from(exit_code)),
            Err((0, error)) if recover_kill && daemon_transport_failure(&error) => {
                Some(recover_kill_server_failure(socket_path, &error))
            }
            Err((_, DaemonError::CommandFailed { output, error })) => {
                print_command_output(&output);
                eprintln!("{}", command_error_message(&error));
                Some(ExitCode::FAILURE)
            }
            Err((_, error)) => {
                eprintln!("{}", command_error_message(&error));
                Some(ExitCode::FAILURE)
            }
        };
    }
    match execute_command_chain(
        command_chain,
        |command| client.execute_streams(command),
        |outcome| {
            print_command_output(&outcome.stdout);
            print_command_error(&outcome.stderr);
        },
    ) {
        Ok(exit_code) => Some(ExitCode::from(exit_code)),
        Err(DaemonError::CommandFailed { output, error }) => {
            print_command_output(&output);
            eprintln!("{}", command_error_message(&error));
            Some(ExitCode::FAILURE)
        }
        Err(error) => {
            eprintln!("{}", command_error_message(&error));
            Some(ExitCode::FAILURE)
        }
    }
}

#[cfg(not(target_os = "ios"))]
struct PreparedCliCommandChain {
    client: CommandClient,
    commands: Vec<PreparedCommand>,
}

#[cfg(not(target_os = "ios"))]
fn prepare_cli_command_chain(
    socket_path: &Path,
    commands: &[CommandInvocation],
) -> Result<PreparedCliCommandChain, DaemonError> {
    let mut client = CommandClient::connect(socket_path)?;
    let commands = client.prepare_commands(commands.to_vec())?;
    Ok(PreparedCliCommandChain { client, commands })
}

#[cfg(not(target_os = "ios"))]
fn prepared_command_is(command: &PreparedCommand, canonical_name: &str) -> bool {
    command.result == PreparedCommandResult::Ready
        && command.canonical_name.as_deref() == Some(canonical_name)
}

#[cfg(not(target_os = "ios"))]
fn prepared_command_invocations(command: &PreparedCommand) -> Option<Cow<'_, [CommandInvocation]>> {
    if command.result != PreparedCommandResult::Ready {
        return None;
    }
    if command.canonical_name.is_some() {
        return Some(Cow::Borrowed(std::slice::from_ref(&command.invocation)));
    }
    MuxEngine::command_alias_group_commands(&command.invocation)
        .ok()
        .flatten()
        .map(Cow::Owned)
}

#[cfg(not(target_os = "ios"))]
fn prepared_command_tail(command: &PreparedCommand) -> Option<Cow<'_, CommandInvocation>> {
    match prepared_command_invocations(command)? {
        Cow::Borrowed(commands) => commands.last().map(Cow::Borrowed),
        Cow::Owned(mut commands) => commands.pop().map(Cow::Owned),
    }
}

#[cfg(not(target_os = "ios"))]
fn prepared_command_any(
    command: &PreparedCommand,
    mut matches: impl FnMut(&CommandInvocation, &str) -> bool,
) -> bool {
    prepared_command_invocations(command).is_some_and(|commands| {
        commands.iter().any(|invocation| {
            let canonical_name = command
                .canonical_name
                .as_deref()
                .unwrap_or_else(|| canonical_command(&invocation.name));
            matches(invocation, canonical_name)
        })
    })
}

#[cfg(not(target_os = "ios"))]
fn prepared_command_error(commands: &[PreparedCommand]) -> Option<&ServerError> {
    commands.iter().find_map(|command| match &command.result {
        PreparedCommandResult::Ready => None,
        PreparedCommandResult::Error(error) => Some(error),
    })
}

#[cfg(not(target_os = "ios"))]
fn prepared_command_reads_stdin(command: &PreparedCommand) -> bool {
    let Some(tail) = prepared_command_tail(command) else {
        return false;
    };
    let canonical_name = command
        .canonical_name
        .as_deref()
        .unwrap_or_else(|| canonical_command(&tail.name));
    match canonical_name {
        "agent-send" => zz_daemon::agent_send_reads_stdin(&tail.args),
        "send-text" => zz_daemon::send_text_reads_stdin(&tail.args),
        _ => false,
    }
}

#[cfg(not(target_os = "ios"))]
fn stdin_payload_has_argument_boundary(canonical_name: &str, arguments: &[RawText]) -> bool {
    let Some(spec) = catalog_command_spec(canonical_name) else {
        return false;
    };
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        if argument == "--" {
            return true;
        }
        if !argument.starts_with('-') || argument == "-" {
            return false;
        }
        let consumes_next = spec
            .option(argument)
            .is_some_and(|option| option.value.is_some());
        index += if consumes_next { 2 } else { 1 };
    }
    false
}

#[cfg(not(target_os = "ios"))]
fn append_stdin_payload(canonical_name: &str, arguments: &mut Vec<RawText>, payload: String) {
    if !stdin_payload_has_argument_boundary(canonical_name, arguments) {
        arguments.push("--".into());
    }
    arguments.push(payload.into());
}

#[cfg(not(target_os = "ios"))]
fn append_prepared_command_stdin_payload(command: &mut PreparedCommand, payload: String) {
    if let Some(canonical_name) = command.canonical_name.clone() {
        append_stdin_payload(&canonical_name, &mut command.invocation.args, payload);
        return;
    }
    let has_argument_boundary = prepared_command_tail(command).is_some_and(|tail| {
        stdin_payload_has_argument_boundary(canonical_command(&tail.name), &tail.args)
    });
    let body = MuxEngine::command_alias_group_body(&command.invocation)
        .expect("stdin-reading prepared command must be an alias group");
    let marker = "__zz-stdin-payload";
    let mut suffix_arguments = Vec::with_capacity(2);
    if !has_argument_boundary {
        suffix_arguments.push("--".to_owned());
    }
    suffix_arguments.push(payload);
    let suffix = format_command(&CommandInvocation::new(marker, suffix_arguments));
    let suffix = suffix
        .strip_prefix(marker)
        .expect("stdin payload formatter must preserve an unknown command name");
    command.invocation.args[0] = format!("{{ {body}{suffix} }}").into();
}

#[cfg(not(target_os = "ios"))]
fn prepared_command_chain_uses_tui(
    typed: &[CommandInvocation],
    prepared: &[PreparedCommand],
) -> bool {
    typed.iter().zip(prepared).any(|(typed, prepared)| {
        !matches!(typed.name.as_str(), "attach" | "attach-session")
            && prepared_command_any(prepared, |command, canonical_name| {
                canonical_name == "new-session"
                    && MuxEngine::new_session_attaches(&command.args).unwrap_or(false)
            })
    })
}

#[cfg(not(target_os = "ios"))]
fn prepared_attach_uses_tui(typed_name: &str, prepared: &PreparedCommand) -> bool {
    !matches!(typed_name, "attach" | "attach-session")
        && prepared_command_any(prepared, |_, canonical_name| {
            canonical_name == "attach-session"
        })
}

#[cfg(not(target_os = "ios"))]
fn prepared_native_attach(typed_name: &str, prepared: &PreparedCommand) -> bool {
    matches!(typed_name, "attach" | "attach-session")
        && !prepared.alias_matched
        && prepared_command_is(prepared, "attach-session")
}

#[cfg(not(target_os = "ios"))]
fn prepared_kill_server_recovery(typed_name: &str, prepared: &PreparedCommand) -> bool {
    typed_name == "kill-server"
        && !prepared.alias_matched
        && prepared_command_is(prepared, "kill-server")
}

#[cfg(not(target_os = "ios"))]
fn execute_prepared_command(
    client: &mut CommandClient,
    command: PreparedCommand,
) -> Result<CommandOutcome, DaemonError> {
    match command.result {
        PreparedCommandResult::Ready => client.execute_prepared_streams(command.invocation),
        PreparedCommandResult::Error(error) => Err(DaemonError::Server(error)),
    }
}

#[cfg(not(target_os = "ios"))]
fn daemon_transport_failure(error: &DaemonError) -> bool {
    match error {
        DaemonError::Io(_) | DaemonError::Protocol(_) | DaemonError::IncompatibleDaemon { .. } => {
            true
        }
        DaemonError::CommandFailed { error, .. } => daemon_transport_failure(error),
        DaemonError::Server(_)
        | DaemonError::AlreadyRunning(_)
        | DaemonError::Thread(_)
        | DaemonError::InsertedCommandParse(_)
        | DaemonError::CommandExit { .. }
        | DaemonError::ReportedCommandExit { .. } => false,
    }
}

#[cfg(not(target_os = "ios"))]
fn new_session_uses_tui(invocation: &CommandInvocation) -> bool {
    canonical_command(&invocation.name) == "new-session"
        && MuxEngine::new_session_attaches(&invocation.args).unwrap_or(false)
}

#[cfg(not(target_os = "ios"))]
fn attach_prefix_uses_tui(command: &str) -> bool {
    canonical_command(command) == "attach-session"
        && !matches!(command, "attach" | "attach-session")
}

#[cfg(not(target_os = "ios"))]
fn command_reads_stdin(invocation: &CommandInvocation) -> bool {
    match canonical_command(&invocation.name) {
        "agent-send" => zz_daemon::agent_send_reads_stdin(&invocation.args),
        "send-text" => zz_daemon::send_text_reads_stdin(&invocation.args),
        _ => false,
    }
}

#[cfg(not(target_os = "ios"))]
fn command_chain_uses_tui(invocations: &[CommandInvocation]) -> bool {
    invocations.iter().any(new_session_uses_tui)
}

#[cfg(not(target_os = "ios"))]
fn split_command_chain(arguments: &[RawText]) -> Vec<CommandInvocation> {
    zz_protocol::split_command_words(arguments.iter().cloned())
        .into_iter()
        .filter_map(|words| {
            let mut words = words.into_iter();
            let name = words.next()?;
            Some(CommandInvocation::new(name, words))
        })
        .collect()
}

/// Run every member of a `\;` chain, emitting each one's streams as it lands.
/// The pin stops a chain only when a command itself fails (`cmdq_next` drops
/// the rest of the group on `CMD_RETURN_ERROR`), never merely because the
/// client's exit status went nonzero, and the last nonzero status wins.
#[cfg(not(target_os = "ios"))]
fn execute_command_chain<T, E>(
    commands: impl IntoIterator<Item = T>,
    mut execute: impl FnMut(T) -> Result<CommandOutcome, E>,
    mut emit: impl FnMut(&CommandOutcome),
) -> Result<u8, E> {
    let mut exit_code = 0;
    for command in commands {
        let outcome = execute(command)?;
        emit(&outcome);
        if outcome.exit_code != 0 {
            exit_code = outcome.exit_code;
        }
    }
    Ok(exit_code)
}

#[cfg(not(target_os = "ios"))]
fn run_tmux_shell_command(
    socket_path: &Path,
    socket_source: SocketSelectionSource,
    host: Option<&str>,
    mux_config_files: &[PathBuf],
    no_start_server: bool,
    shell_command: &str,
    login_shell: bool,
) -> ExitCode {
    let start_server = !no_start_server;
    if let Some(error) = tmux_label_creation_error(socket_path, socket_source, start_server) {
        eprintln!("{}", error.message);
        return ExitCode::FAILURE;
    }
    let mut client = match host.map_or_else(
        || {
            connect_command_client(socket_path, mux_config_files, start_server)
                .map_err(|error| format_local_command_error(socket_path, error))
        },
        |host| connect_host_command_client(host).map_err(|error| format!("zz: {error}")),
    ) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let shell = match client.execute(CommandInvocation::new(
        "show-options",
        ["-gqv", "default-shell"],
    )) {
        Ok(shell) => shell.trim_end_matches('\n').to_owned(),
        Err(error) => {
            eprintln!("{}", command_error_message(&error));
            return ExitCode::FAILURE;
        }
    };
    let mut process = Command::new(&shell);
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::process::CommandExt as _};

        let name = Path::new(&shell)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new(&shell));
        let mut argv0 = OsString::new();
        if login_shell {
            argv0.push("-");
        }
        argv0.push(name);
        process.arg0(argv0);
    }
    #[cfg(not(unix))]
    let _ = login_shell;
    match process
        .arg("-c")
        .arg(shell_command)
        .env("SHELL", &shell)
        .status()
    {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(error) => {
            eprintln!("zz: could not run {shell}: {error}");
            ExitCode::FAILURE
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
fn is_version_command(command: &str) -> bool {
    command == "--version"
}

#[cfg(not(target_os = "ios"))]
fn tmux_command_starts_server(command: &str) -> bool {
    matches!(
        canonical_command(command),
        "attach-session" | "list-commands" | "list-keys" | "new-session" | "start-server"
    )
}

#[cfg(not(target_os = "ios"))]
fn parse_native_attach_arguments(
    arguments: impl IntoIterator<Item = RawText>,
) -> Result<NativeAttachArguments, NativeAttachArgumentError> {
    let spec = catalog_command_spec("attach-session").expect("attach-session catalog");
    let mut restart_daemon = false;
    let mut detach_others = false;
    let mut detach_others_hangup = false;
    let mut no_update_environment = false;
    let mut read_only = false;
    let mut client_flags = None;
    let mut working_directory = None;
    let mut target = None;
    let mut positional = None;
    let mut options_done = false;
    let mut explicit_boundary = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if !explicit_boundary && argument == "--restart-daemon" {
            if restart_daemon {
                return Err(NativeAttachArgumentError::Usage);
            }
            restart_daemon = true;
            continue;
        }
        if options_done || !argument.starts_with('-') || argument == "-" {
            if positional.is_some() {
                return Err(NativeAttachArgumentError::Usage);
            }
            positional = Some(argument);
            options_done = true;
            continue;
        }
        if argument == "--" {
            options_done = true;
            explicit_boundary = true;
            continue;
        }
        if argument.starts_with("--") {
            return Err(NativeAttachArgumentError::Command(
                ServerError::CommandParse("command attach-session: invalid flag --".to_owned()),
            ));
        }
        let options = &argument[1..];
        for (index, option) in options.char_indices() {
            let name = format!("-{option}");
            if option == '?' {
                return Err(NativeAttachArgumentError::Command(
                    ServerError::CommandParse(format!(
                        "usage: attach-session {}",
                        spec.pinned_tmux_usage()
                    )),
                ));
            }
            if !option.is_ascii_alphanumeric() {
                return Err(NativeAttachArgumentError::Command(
                    ServerError::CommandParse(format!(
                        "command attach-session: invalid flag {name}"
                    )),
                ));
            }
            match option {
                'd' => detach_others = true,
                'E' => no_update_environment = true,
                'r' => read_only = true,
                't' | 'c' | 'f' => {
                    let value_index = index + option.len_utf8();
                    let value = if value_index < options.len() {
                        options[value_index..].to_owned()
                    } else {
                        arguments
                            .next()
                            .ok_or_else(|| {
                                NativeAttachArgumentError::Command(ServerError::CommandParse(
                                    format!("command attach-session: {name} expects an argument"),
                                ))
                            })?
                            .to_string()
                    };
                    match option {
                        't' => target = Some(value),
                        'c' => working_directory = Some(value),
                        'f' => client_flags = Some(value),
                        _ => unreachable!(),
                    }
                    break;
                }
                'x' => detach_others_hangup = true,
                _ => {
                    return Err(NativeAttachArgumentError::Command(
                        ServerError::CommandParse(format!(
                            "command attach-session: unknown flag {name}"
                        )),
                    ));
                }
            }
        }
    }
    if target.is_some() && positional.is_some() {
        return Err(NativeAttachArgumentError::Usage);
    }
    Ok(NativeAttachArguments {
        restart_daemon,
        detach_others,
        detach_others_hangup,
        no_update_environment,
        read_only,
        client_flags,
        working_directory,
        session: target.or(positional.map(|value| value.to_string())),
    })
}

#[cfg(not(target_os = "ios"))]
fn print_native_attach_argument_error(error: NativeAttachArgumentError) {
    match error {
        NativeAttachArgumentError::Usage => eprintln!("{NATIVE_ATTACH_USAGE}"),
        NativeAttachArgumentError::Command(error) => {
            eprintln!("{}", command_error_message(&DaemonError::Server(error)));
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn native_attach_command(options: &NativeAttachArguments) -> CommandInvocation {
    let mut args = Vec::new();
    if options.detach_others {
        args.push("-d".to_owned());
    }
    if options.detach_others_hangup {
        args.push("-x".to_owned());
    }
    if options.no_update_environment {
        args.push("-E".to_owned());
    }
    if options.read_only {
        args.push("-r".to_owned());
    }
    if let Some(client_flags) = &options.client_flags {
        args.extend(["-f".to_owned(), client_flags.clone()]);
    }
    if let Some(working_directory) = &options.working_directory {
        args.extend(["-c".to_owned(), working_directory.clone()]);
    }
    if let Some(session) = &options.session {
        args.extend(["-t".to_owned(), session.clone()]);
    }
    CommandInvocation::new("attach-session", args)
}

#[cfg(not(target_os = "ios"))]
fn tmux_label_creation_error(
    path: &Path,
    socket_source: SocketSelectionSource,
    start_server: bool,
) -> Option<TmuxLabelCreationError> {
    if socket_source != SocketSelectionSource::Label || !start_server {
        return None;
    }
    let parent = path.parent()?;
    match std::fs::metadata(parent) {
        Ok(metadata) if metadata.is_dir() => None,
        Ok(_) => Some(TmuxLabelCreationError {
            message: format!("error connecting to {} (Not a directory)", path.display()),
            kind: ErrorKind::NotADirectory,
        }),
        Err(error) => Some(TmuxLabelCreationError {
            message: format!(
                "error creating {} ({})",
                path.display(),
                os_error_text(&error)
            ),
            kind: error.kind(),
        }),
    }
}

#[cfg(not(target_os = "ios"))]
fn os_error_text(error: &io::Error) -> String {
    let message = error.to_string();
    error.raw_os_error().map_or(message.clone(), |code| {
        message
            .strip_suffix(&format!(" (os error {code})"))
            .unwrap_or(&message)
            .to_owned()
    })
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
fn format_local_command_error(path: &Path, error: DaemonError) -> String {
    match error {
        DaemonError::Server(ServerError::InvalidCommand(message))
            if message == "server exited unexpectedly" =>
        {
            message
        }
        DaemonError::Io(error) if error.kind() == ErrorKind::ConnectionRefused => {
            format!("no server running on {}", path.display())
        }
        DaemonError::Io(error) => format!(
            "error connecting to {} ({})",
            path.display(),
            os_error_text(&error)
        ),
        error => format!("zz: {}", format_local_daemon_error(error)),
    }
}

#[cfg(not(target_os = "ios"))]
/// Command output is a byte string, so it reaches stdout exactly as the daemon
/// stored it, `show-environment`'s non-UTF-8 values included.
fn print_command_output(output: &RawText) {
    let output = output.as_bytes();
    if output.is_empty() {
        return;
    }
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(output);
    if !output.ends_with(b"\n") {
        let _ = stdout.write_all(b"\n");
    }
    let _ = stdout.flush();
}

#[cfg(not(target_os = "ios"))]
fn print_command_error(output: &str) {
    if output.is_empty() {
        return;
    }
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(output.as_bytes());
    if !output.ends_with('\n') {
        let _ = stderr.write_all(b"\n");
    }
    let _ = stderr.flush();
}

#[cfg(not(target_os = "ios"))]
fn command_error_message(error: &DaemonError) -> String {
    match error {
        DaemonError::CommandFailed { error, .. } => command_error_message(error),
        DaemonError::Server(error) => server_error_message(error),
        error => format!("zz: {error}"),
    }
}

#[cfg(not(target_os = "ios"))]
fn server_error_message(error: &ServerError) -> String {
    error.tmux_message()
}

#[cfg(not(target_os = "ios"))]
fn run_kill_server(
    path: &Path,
    args: impl IntoIterator<Item = RawText>,
    prepared: bool,
) -> ExitCode {
    let invocation = CommandInvocation::new("kill-server", args);
    let failure = match CommandClient::connect(path) {
        Ok(mut client) => {
            if prepared {
                match client.execute_prepared_streams(invocation) {
                    Ok(outcome) => {
                        print_command_output(&outcome.stdout);
                        print_command_error(&outcome.stderr);
                        return ExitCode::from(outcome.exit_code);
                    }
                    Err(error) => error,
                }
            } else {
                match client.execute(invocation) {
                    Ok(output) => {
                        if !output.is_empty() {
                            println!("{output}");
                        }
                        return ExitCode::SUCCESS;
                    }
                    Err(error) => error,
                }
            }
        }
        Err(error) if daemon_is_missing(&error) => {
            eprintln!("{}", format_local_command_error(path, error));
            return ExitCode::FAILURE;
        }
        Err(error) => error,
    };

    recover_kill_server_failure(path, &failure)
}

#[cfg(not(target_os = "ios"))]
fn run_host_kill_server(host: &str, args: impl IntoIterator<Item = RawText>) -> ExitCode {
    let mut client = match connect_host_command_client(host) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("zz: {error}");
            return ExitCode::FAILURE;
        }
    };
    match client.execute_prepared_streams(CommandInvocation::new("kill-server", args)) {
        Ok(outcome) => {
            print_command_output(&outcome.stdout);
            print_command_error(&outcome.stderr);
            ExitCode::from(outcome.exit_code)
        }
        Err(DaemonError::CommandFailed { output, error }) => {
            print_command_output(&output);
            eprintln!("{}", command_error_message(&error));
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("{}", command_error_message(&error));
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn recover_kill_server_failure(path: &Path, failure: &DaemonError) -> ExitCode {
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
fn daemon_is_spawnable(error: &DaemonError) -> bool {
    matches!(
        error,
        DaemonError::Io(error)
            if matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset
            )
    )
}

#[cfg(not(target_os = "ios"))]
fn next_spawn_server_id() -> u64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let folded = u64::try_from(timestamp & u128::from(u64::MAX)).unwrap_or_default();
    folded
        ^ u64::from(std::process::id()).rotate_left(32)
        ^ DAEMON_SPAWN_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[cfg(not(target_os = "ios"))]
fn spawn_daemon(
    path: &Path,
    color_scheme: Option<TerminalColorScheme>,
    mux_config_files: &[PathBuf],
) -> Result<u64, DaemonError> {
    let server_id = next_spawn_server_id();
    let executable = std::env::current_exe()?;
    let mut command = Command::new(&executable);
    command
        .env("ZZ_TMUX_EXECUTABLE", &executable)
        .env_remove(APP_STARTUP_DIRECTORY_ENV);
    command.arg(diagnostics::SOCKET_ARGUMENT).arg(path);
    for config in mux_config_files {
        command.arg("-f").arg(config);
    }
    command
        .arg("daemon")
        .arg(DAEMON_BOOTSTRAP_SERVER_ID_ARGUMENT)
        .arg(server_id.to_string());
    if let Some(color_scheme) = color_scheme {
        command.env("ZZ_COLOR_SCHEME", color_scheme.as_str());
    }
    diagnostics::configure_spawned_process(&mut command);
    #[cfg(unix)]
    detach_daemon_session(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(client_working_directory) = std::env::current_dir()
        .ok()
        .and_then(validated_bootstrap_client_working_directory)
    {
        command
            .arg(DAEMON_BOOTSTRAP_CLIENT_CWD_ARGUMENT)
            .arg(client_working_directory);
    }
    command.spawn()?;
    log::debug!(
        target: "zz::diagnostics::process",
        "spawned daemon path={} child_verbose_flag_applied={}",
        path.display(),
        diagnostics::enabled(),
    );
    Ok(server_id)
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
    mux_config_files: &[PathBuf],
    connect: impl Fn(bool) -> Result<T, DaemonError>,
    server_hello: impl for<'a> Fn(&'a T) -> &'a ServerHello,
) -> Result<T, DaemonError> {
    connect_or_spawn_daemon_with_provenance(
        path,
        color_scheme,
        mux_config_files,
        connect,
        server_hello,
    )
    .map(|(client, _)| client)
}

#[cfg(not(target_os = "ios"))]
fn connect_or_spawn_daemon_with_provenance<T>(
    path: &Path,
    color_scheme: Option<TerminalColorScheme>,
    mux_config_files: &[PathBuf],
    connect: impl Fn(bool) -> Result<T, DaemonError>,
    server_hello: impl for<'a> Fn(&'a T) -> &'a ServerHello,
) -> Result<(T, Option<u64>), DaemonError> {
    match connect(false) {
        Ok(client) => {
            log::debug!(
                target: "zz::diagnostics::process",
                "connected to existing daemon path={} server_hello={:#?}",
                path.display(),
                server_hello(&client),
            );
            return Ok((client, None));
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

    let spawned_server_id = spawn_daemon(path, color_scheme, mux_config_files)?;
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        match connect(true) {
            Ok(client) => {
                let provenance = (server_hello(&client).server_id == spawned_server_id)
                    .then_some(spawned_server_id);
                return Ok((client, provenance));
            }
            Err(error) if Instant::now() >= deadline => {
                return Err(classify_local_connect_error(path, error));
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn connect_command_client(
    path: &Path,
    mux_config_files: &[PathBuf],
    start_server: bool,
) -> Result<CommandClient, DaemonError> {
    if !start_server {
        return CommandClient::connect(path);
    }
    connect_or_spawn_daemon(
        path,
        None,
        mux_config_files,
        |_| CommandClient::connect(path),
        CommandClient::server_hello,
    )
}

/// The command list the pin's `server_client_default_command` runs for a client
/// that arrives with no command of its own: `default-client-command`, parsed as
/// a command list. `new-session` alone is the option's own default, and zz's
/// launcher keeps answering that one with the create-or-attach `new-session -A`
/// it has always used.
#[cfg(not(target_os = "ios"))]
fn default_client_command_chain(
    socket_path: &Path,
    mux_config_files: &[PathBuf],
    no_start_server: bool,
) -> Vec<CommandInvocation> {
    stored_default_client_command(socket_path, mux_config_files, no_start_server)
        .map(|stored| zz_mux::parse_config("default-client-command", &stored))
        .filter(|parsed| parsed.diagnostics.is_empty() && !parsed.commands.is_empty())
        .map(|parsed| parsed.commands)
        .filter(|commands| !is_launcher_default_client_command(commands))
        .unwrap_or_else(|| vec![CommandInvocation::new("new-session", ["-A"])])
}

#[cfg(not(target_os = "ios"))]
fn is_launcher_default_client_command(commands: &[CommandInvocation]) -> bool {
    matches!(commands, [command] if command.name == "new-session" && command.args.is_empty())
}

#[cfg(not(target_os = "ios"))]
fn stored_default_client_command(
    socket_path: &Path,
    mux_config_files: &[PathBuf],
    no_start_server: bool,
) -> Option<String> {
    let mut client = if no_start_server {
        CommandClient::connect(socket_path).ok()?
    } else {
        connect_command_client_with_spawn_provenance(socket_path, mux_config_files)
            .ok()?
            .0
    };
    let output = client
        .execute(CommandInvocation::new(
            "show-options",
            ["-sqv", "default-client-command"],
        ))
        .ok()?;
    Some(output.trim_end_matches('\n').to_owned())
}

#[cfg(not(target_os = "ios"))]
fn connect_command_client_with_spawn_provenance(
    path: &Path,
    mux_config_files: &[PathBuf],
) -> Result<(CommandClient, Option<u64>), DaemonError> {
    connect_or_spawn_daemon_with_provenance(
        path,
        None,
        mux_config_files,
        |_| CommandClient::connect(path),
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
    connect_interactive_client_with_config(path, color_scheme, &[])
}

#[cfg(not(target_os = "ios"))]
fn connect_interactive_client_with_config(
    path: &Path,
    color_scheme: TerminalColorScheme,
    mux_config_files: &[PathBuf],
) -> Result<InteractiveClient, DaemonError> {
    connect_interactive_client_with_config_and_terminal(path, color_scheme, mux_config_files, true)
}

#[cfg(not(target_os = "ios"))]
fn connect_interactive_client_with_config_and_terminal(
    path: &Path,
    color_scheme: TerminalColorScheme,
    mux_config_files: &[PathBuf],
    client_has_terminal: bool,
) -> Result<InteractiveClient, DaemonError> {
    connect_or_spawn_daemon(
        path,
        Some(color_scheme),
        mux_config_files,
        |_| {
            InteractiveClient::connect_with_color_scheme_and_terminal(
                path,
                color_scheme,
                client_has_terminal,
            )
        },
        InteractiveClient::server_hello,
    )
}

#[cfg(not(target_os = "ios"))]
fn connect_terminal_surface_client_with_config(
    path: &Path,
    color_scheme: TerminalColorScheme,
    mux_config_files: &[PathBuf],
    client_has_terminal: bool,
) -> Result<InteractiveClient, DaemonError> {
    connect_or_spawn_daemon(
        path,
        Some(color_scheme),
        mux_config_files,
        |_| InteractiveClient::connect_terminal_surface(path, color_scheme, client_has_terminal),
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
    use std::{
        io,
        path::{Path, PathBuf},
    };

    use gpui::WindowAppearance;
    use zz_protocol::RawText;
    use zz_terminal::TerminalColorScheme;

    use super::{
        ApplicationArgumentError, CommandOutcome, DaemonBootstrapArgumentError,
        DaemonBootstrapArguments, NativeAttachArgumentError, TMUX_USAGE, TMUX_VERSION_OUTPUT,
        append_prepared_command_stdin_payload, append_stdin_payload, application_arguments,
        application_working_directory, attach_prefix_uses_tui, command_chain_uses_tui,
        command_error_message, command_reads_stdin, daemon_is_missing, daemon_transport_failure,
        execute_command_chain, implicit_tmux_endpoint_conflict, native_attach_command,
        new_session_uses_tui, parse_daemon_bootstrap_arguments, parse_native_attach_arguments,
        prepared_attach_uses_tui, prepared_command_chain_uses_tui, prepared_command_reads_stdin,
        prepared_kill_server_recovery, prepared_native_attach, protocol_version_output,
        run_command_mode, split_command_chain, terminal_color_scheme, tmux_command_starts_server,
        validated_bootstrap_client_working_directory,
    };
    #[cfg(unix)]
    use super::{tmux_label_socket_path, tmux_socket_root};
    use zz_daemon::DaemonError;
    use zz_mux::{CommandAliasResolution, ExecutionContext, MuxEngine};
    use zz_protocol::{CommandInvocation, PreparedCommand, PreparedCommandResult, ServerError};

    #[test]
    fn daemon_bootstrap_arguments_accept_only_the_ordered_private_grammar() {
        let path = std::env::temp_dir().join("client cwd [literal]*? with spaces");
        let path_string = path.to_str().expect("UTF-8 temporary path").to_owned();

        assert_eq!(
            parse_daemon_bootstrap_arguments(&[]).unwrap(),
            DaemonBootstrapArguments::default()
        );
        assert_eq!(
            parse_daemon_bootstrap_arguments(&["--bootstrap-server-id".into(), "42".into(),])
                .unwrap(),
            DaemonBootstrapArguments {
                server_id: Some(42),
                client_working_directory: None,
            }
        );
        assert_eq!(
            parse_daemon_bootstrap_arguments(&[
                "--bootstrap-server-id".into(),
                "42".into(),
                "--bootstrap-client-cwd".into(),
                path_string.clone().into(),
            ])
            .unwrap(),
            DaemonBootstrapArguments {
                server_id: Some(42),
                client_working_directory: Some(path),
            }
        );

        for arguments in [
            vec![RawText::from("extra")],
            vec![RawText::from("--bootstrap-server-id")],
            vec![RawText::from("--bootstrap-server-id"), "nope".into()],
            vec![
                RawText::from("--bootstrap-client-cwd"),
                RawText::from(path_string.clone()),
            ],
        ] as [Vec<RawText>; 4]
        {
            assert_eq!(
                parse_daemon_bootstrap_arguments(&arguments),
                Err(DaemonBootstrapArgumentError::ServerId),
                "{arguments:?}"
            );
        }
        for arguments in [
            vec![
                "--bootstrap-server-id".into(),
                "42".into(),
                "--bootstrap-client-cwd".into(),
            ],
            vec![
                "--bootstrap-server-id".into(),
                "42".into(),
                "--other".into(),
                path_string.clone().into(),
            ],
            vec![
                "--bootstrap-server-id".into(),
                "42".into(),
                "--bootstrap-client-cwd".into(),
                "relative".into(),
            ],
            vec![
                "--bootstrap-server-id".into(),
                "42".into(),
                "--bootstrap-client-cwd".into(),
                path_string.clone().into(),
                "extra".into(),
            ],
        ] as [Vec<RawText>; 4]
        {
            assert_eq!(
                parse_daemon_bootstrap_arguments(&arguments),
                Err(DaemonBootstrapArgumentError::ClientWorkingDirectory),
                "{arguments:?}"
            );
        }
        assert_eq!(
            DaemonBootstrapArgumentError::ServerId.message(),
            "invalid bootstrap server id"
        );
        assert_eq!(
            DaemonBootstrapArgumentError::ClientWorkingDirectory.message(),
            "invalid bootstrap client cwd"
        );
    }

    #[test]
    fn daemon_bootstrap_client_cwd_filter_accepts_only_bounded_absolute_utf8() {
        #[cfg(unix)]
        let prefix = "/";
        #[cfg(windows)]
        let prefix = "C:\\";
        let path_with_length = |length: usize| {
            PathBuf::from(format!(
                "{prefix}{}",
                "x".repeat(length.saturating_sub(prefix.len()))
            ))
        };
        let boundary = path_with_length(zz_protocol::MAX_CLIENT_WORKING_DIRECTORY_BYTES);
        assert_eq!(
            validated_bootstrap_client_working_directory(boundary.clone()),
            Some(boundary)
        );
        assert!(
            validated_bootstrap_client_working_directory(path_with_length(
                zz_protocol::MAX_CLIENT_WORKING_DIRECTORY_BYTES + 1
            ))
            .is_none()
        );
        assert!(
            validated_bootstrap_client_working_directory(PathBuf::from("relative/path")).is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn daemon_bootstrap_client_cwd_filter_rejects_non_utf8() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt as _};

        let path = PathBuf::from(OsString::from_vec(b"/tmp/client-\xff".to_vec()));
        assert!(validated_bootstrap_client_working_directory(path).is_none());
    }

    #[test]
    fn app_is_an_exact_native_gui_verb_even_inside_tmux() {
        assert!(
            run_command_mode(
                &["app".into()],
                std::path::Path::new("/tmp/zz.sock"),
                super::SocketSelectionSource::Default,
                None,
                &[],
                false,
                None,
                false,
                super::CommandLineOrigin::Launcher,
                true,
            )
            .is_none()
        );
    }

    #[test]
    fn the_option_default_keeps_the_launcher_create_or_attach_and_a_set_list_wins() {
        assert!(super::is_launcher_default_client_command(&[
            CommandInvocation::new("new-session", [] as [&str; 0])
        ]));
        assert!(!super::is_launcher_default_client_command(&[
            CommandInvocation::new("new-session", ["-A"])
        ]));
        assert!(!super::is_launcher_default_client_command(&[
            CommandInvocation::new("new-session", [] as [&str; 0]),
            CommandInvocation::new("new-session", [] as [&str; 0]),
        ]));
        assert!(!super::is_launcher_default_client_command(&[]));
    }

    #[test]
    fn app_working_directory_prefers_the_launcher_and_uses_home_for_launch_services() {
        let home = tempfile::tempdir().expect("temporary home");
        let project = tempfile::tempdir().expect("temporary project");

        assert_eq!(
            application_working_directory(
                Some(project.path()),
                Some(Path::new("/")),
                Some(home.path()),
                true,
            ),
            Some(project.path().to_owned())
        );
        assert_eq!(
            application_working_directory(None, Some(Path::new("/")), Some(home.path()), true),
            Some(home.path().to_owned())
        );
        assert_eq!(
            application_working_directory(None, Some(project.path()), Some(home.path()), true),
            Some(project.path().to_owned())
        );
    }

    #[test]
    fn tmux_start_server_policy_matches_the_pin() {
        for command in [
            "attach-session",
            "attach",
            "list-commands",
            "lscm",
            "list-keys",
            "lsk",
            "new-session",
            "new",
            "start-server",
            "start",
        ] {
            assert!(tmux_command_starts_server(command), "{command}");
        }
        for command in [
            "list-sessions",
            "ls",
            "list-panes",
            "show-options",
            "has-session",
            "source-file",
            "source",
            "kill-server",
        ] {
            assert!(!tmux_command_starts_server(command), "{command}");
        }
    }

    #[test]
    fn new_session_tui_routing_resolves_prefixes_and_tmux_argv_edges() {
        let routes = |name: &str, args: &[&str]| {
            new_session_uses_tui(&CommandInvocation::new(name, args.iter().copied()))
        };

        assert!(routes("new-session", &[]));
        assert!(routes("new", &["-s", "work"]));
        assert!(routes("new-s", &["-dA", "-s", "work"]));
        assert!(routes("new-session", &["-t", "group"]));
        assert!(routes("new", &["-At", "work"]));
        assert!(routes("new-session", &["-s", "a", "/usr/bin/true", "-d"]));
        assert!(routes("new-session", &["-s", "b", "--", "-d"]));
        assert!(!routes("new-session", &["-dsfoo"]));
        assert!(!routes("new-session", &["-d", "-t", "group"]));
        assert!(!routes("new-session", &["-s"]));
        assert!(!routes("list-sessions", &[]));
    }

    #[test]
    fn attach_prefix_routing_keeps_exact_native_attach_commands() {
        for command in ["a", "att", "attach-", "attach-s"] {
            assert!(attach_prefix_uses_tui(command), "{command}");
        }
        for command in ["attach", "attach-session", "list-sessions"] {
            assert!(!attach_prefix_uses_tui(command), "{command}");
        }
    }

    #[test]
    fn agent_send_stdin_routing_uses_the_canonical_static_command() {
        for command in ["agent-send", "agent-s"] {
            assert!(command_reads_stdin(&CommandInvocation::new(
                command,
                ["--submit"]
            )));
            assert!(!command_reads_stdin(&CommandInvocation::new(
                command,
                ["text"]
            )));
        }
        for command in ["send-text", "send-t"] {
            assert!(command_reads_stdin(&CommandInvocation::new(
                command,
                ["-t", "%1"]
            )));
            assert!(!command_reads_stdin(&CommandInvocation::new(
                command,
                ["hello"]
            )));
        }
        assert!(!command_reads_stdin(&CommandInvocation::new(
            "list-sessions",
            [] as [&str; 0]
        )));
    }

    #[test]
    fn stdin_payload_boundary_distinguishes_terminators_from_option_values() {
        let mut send_text = ["-t", "--"].map(RawText::from).to_vec();
        append_stdin_payload("send-text", &mut send_text, "--no-enter".to_owned());
        assert_eq!(send_text, ["-t", "--", "--", "--no-enter"]);

        let mut agent_send = ["--context", "--"].map(RawText::from).to_vec();
        append_stdin_payload("agent-send", &mut agent_send, "--submit".to_owned());
        assert_eq!(agent_send, ["--context", "--", "--", "--submit"]);

        let mut bounded = ["-t", "%1", "--"].map(RawText::from).to_vec();
        append_stdin_payload("send-text", &mut bounded, "--no-enter".to_owned());
        assert_eq!(bounded, ["-t", "%1", "--", "--no-enter"]);
    }

    #[test]
    fn prepared_cli_routing_uses_canonical_identity_and_alias_match() {
        let prepared =
            |typed: &str, canonical: &str, alias_matched: bool, args: &[&str]| PreparedCommand {
                invocation: CommandInvocation::new(typed, args.iter().copied()),
                canonical_name: Some(canonical.to_owned()),
                alias_matched,
                result: PreparedCommandResult::Ready,
            };

        let exact_shadow = prepared("attach", "attach-session", true, &[]);
        assert!(!prepared_native_attach("attach", &exact_shadow));
        assert!(!prepared_attach_uses_tui("attach", &exact_shadow));

        let live_attach = prepared("go", "attach-session", true, &["-t", "work"]);
        assert!(prepared_attach_uses_tui("go", &live_attach));

        let live_new = prepared("work", "new-session", true, &["-t", "group"]);
        assert!(prepared_command_chain_uses_tui(
            &[CommandInvocation::new("work", ["-t", "group"])],
            &[live_new]
        ));

        let live_send = prepared("pipe", "agent-send", true, &["-t", "%0"]);
        assert!(prepared_command_reads_stdin(&live_send));
        let shadowed_send = prepared("agent-send", "display-message", true, &["-p", "shadow"]);
        assert!(!prepared_command_reads_stdin(&shadowed_send));

        let plain_kill = prepared("kill-server", "kill-server", false, &[]);
        let aliased_kill = prepared("kill-server", "kill-server", true, &[]);
        assert!(prepared_kill_server_recovery("kill-server", &plain_kill));
        assert!(!prepared_kill_server_recovery("kill-server", &aliased_kill));
    }

    #[test]
    fn prepared_cli_routing_scans_alias_groups_but_stdin_uses_the_final_command() {
        let prepared_alias = |body: &str, args: &[&str]| {
            let mut engine = MuxEngine::default();
            let mut context = ExecutionContext::default();
            engine
                .execute(
                    &mut context,
                    &CommandInvocation::new(
                        "set-option",
                        [
                            "-s".to_owned(),
                            "command-alias[90]".to_owned(),
                            format!("route={body}"),
                        ],
                    ),
                )
                .expect("set command alias");
            let CommandAliasResolution::Expanded(invocation) = engine
                .resolve_command_alias(&CommandInvocation::new("route", args.iter().copied()))
            else {
                panic!("command alias must expand");
            };
            PreparedCommand {
                invocation,
                canonical_name: None,
                alias_matched: true,
                result: PreparedCommandResult::Ready,
            }
        };

        let attach = prepared_alias("display-message -p before ; attach-session -t work", &[]);
        assert!(prepared_attach_uses_tui("route", &attach));
        let attach_first = prepared_alias("attach-session -t work ; display-message -p after", &[]);
        assert!(prepared_attach_uses_tui("route", &attach_first));
        let group_name = attach.invocation.name.clone();
        let forged = prepared_alias(
            &format!(
                "display-message -p outer ; {group_name} {{ display-message -p inner ; attach-session -t work }}"
            ),
            &[],
        );
        let forged_commands = MuxEngine::command_alias_group_commands(&forged.invocation)
            .expect("parse alias group")
            .expect("prepared command is an alias group");
        assert_eq!(forged_commands.len(), 2);
        assert_eq!(forged_commands[1].name, group_name);
        assert!(!MuxEngine::is_command_alias_group(&forged_commands[1]));
        assert!(!prepared_attach_uses_tui("route", &forged));

        let new_session = prepared_alias("display-message -p before ; new-session -s work", &[]);
        assert!(prepared_command_chain_uses_tui(
            &[CommandInvocation::new("route", [] as [&str; 0])],
            &[new_session]
        ));
        let new_session_first =
            prepared_alias("new-session -s work ; display-message -p after", &[]);
        assert!(prepared_command_chain_uses_tui(
            &[CommandInvocation::new("route", [] as [&str; 0])],
            &[new_session_first]
        ));
        let detached = prepared_alias("display-message -p before ; new-session -d -s work", &[]);
        assert!(!prepared_command_chain_uses_tui(
            &[CommandInvocation::new("route", [] as [&str; 0])],
            &[detached]
        ));
        let detached_first =
            prepared_alias("new-session -d -s work ; display-message -p after", &[]);
        assert!(!prepared_command_chain_uses_tui(
            &[CommandInvocation::new("route", [] as [&str; 0])],
            &[detached_first]
        ));

        for payload in [
            "",
            "a 'quoted' \"value\" $x ; { y } \\",
            "first\nsecond",
            "--no-enter",
        ] {
            let mut send_text =
                prepared_alias("display-message -p before ; send-text", &["-t", "%1"]);
            assert!(prepared_command_reads_stdin(&send_text));
            append_prepared_command_stdin_payload(&mut send_text, payload.to_owned());
            let commands = MuxEngine::command_alias_group_commands(&send_text.invocation)
                .expect("parse prepared alias group")
                .expect("prepared command is an alias group");
            assert_eq!(commands[0].name, "display-message");
            assert_eq!(commands[1].name, "send-text");
            assert_eq!(&commands[1].args[..3], ["-t", "%1", "--"]);
            assert_eq!(commands[1].args[3], payload);
        }

        let mut bounded = prepared_alias("display-message -p before ; send-text -t %1 --", &[]);
        assert!(prepared_command_reads_stdin(&bounded));
        append_prepared_command_stdin_payload(&mut bounded, "piped".to_owned());
        let commands = MuxEngine::command_alias_group_commands(&bounded.invocation)
            .expect("parse prepared alias group")
            .expect("prepared command is an alias group");
        assert_eq!(commands[1].args, ["-t", "%1", "--", "piped"]);

        for (body, payload, expected) in [
            (
                "display-message -p before ; send-text -t --",
                "--no-enter",
                vec!["-t", "--", "--", "--no-enter"],
            ),
            (
                "display-message -p before ; agent-send --context --",
                "--submit",
                vec!["--context", "--", "--", "--submit"],
            ),
        ] {
            let mut prepared = prepared_alias(body, &[]);
            assert!(prepared_command_reads_stdin(&prepared));
            append_prepared_command_stdin_payload(&mut prepared, payload.to_owned());
            let commands = MuxEngine::command_alias_group_commands(&prepared.invocation)
                .expect("parse prepared alias group")
                .expect("prepared command is an alias group");
            assert_eq!(commands[1].args, expected);
        }

        let agent_send = prepared_alias("display-message -p before ; agent-send --submit", &[]);
        assert!(prepared_command_reads_stdin(&agent_send));
        let nonfinal_agent_send =
            prepared_alias("agent-send --submit ; display-message -p after", &[]);
        assert!(!prepared_command_reads_stdin(&nonfinal_agent_send));

        let empty = prepared_alias("", &["agent-send", "--submit"]);
        assert!(!prepared_attach_uses_tui("route", &empty));
        assert!(!prepared_command_chain_uses_tui(
            &[CommandInvocation::new("route", [] as [&str; 0])],
            std::slice::from_ref(&empty)
        ));
        assert!(!prepared_command_reads_stdin(&empty));
    }

    #[test]
    fn kill_recovery_accepts_only_transport_and_handshake_failures() {
        assert!(daemon_transport_failure(&DaemonError::Io(io::Error::from(
            io::ErrorKind::BrokenPipe
        ))));
        assert!(!daemon_transport_failure(&DaemonError::Server(
            ServerError::InvalidCommand("no".to_owned())
        )));
        assert!(!daemon_transport_failure(&DaemonError::CommandExit {
            output: RawText::default(),
            exit_code: 7,
        }));
        assert!(daemon_transport_failure(&DaemonError::CommandFailed {
            output: RawText::default(),
            error: Box::new(DaemonError::IncompatibleDaemon {
                daemon: Some(73),
                client: 74,
            }),
        }));
    }

    #[test]
    fn new_session_tui_routing_scans_the_complete_command_chain() {
        let attaching_later = split_command_chain(
            &[
                "new-session",
                "-d",
                "-s",
                "first",
                ";",
                "new-session",
                "-s",
                "later",
            ]
            .map(RawText::from),
        );
        assert!(command_chain_uses_tui(&attaching_later));

        let target_later = split_command_chain(
            &[
                "new-session",
                "-d",
                "-s",
                "first",
                ";",
                "new-session",
                "-t",
                "group",
                ";",
                "new-session",
                "-d",
                "-s",
                "never",
            ]
            .map(RawText::from),
        );
        assert!(command_chain_uses_tui(&target_later));

        let detached_only = split_command_chain(
            &["new-session", "-d", "-s", "first", ";", "list-sessions"].map(RawText::from),
        );
        assert!(!command_chain_uses_tui(&detached_only));
    }

    #[test]
    fn native_attach_parser_keeps_the_zz_superset_and_tmux_target() {
        let target = parse_native_attach_arguments(
            ["-d", "-t", "work", "--restart-daemon"].map(RawText::from),
        )
        .unwrap();
        assert!(target.detach_others);
        assert!(target.restart_daemon);
        assert!(!target.no_update_environment);
        assert!(!target.read_only);
        assert_eq!(target.working_directory, None);
        assert_eq!(target.session.as_deref(), Some("work"));
        assert_eq!(native_attach_command(&target).args, ["-d", "-t", "work"]);

        let positional =
            parse_native_attach_arguments(["work", "--restart-daemon"].map(RawText::from)).unwrap();
        assert!(!positional.detach_others);
        assert!(positional.restart_daemon);
        assert_eq!(positional.session.as_deref(), Some("work"));
        assert_eq!(
            parse_native_attach_arguments(["work", "-@"].map(RawText::from)),
            Err(NativeAttachArgumentError::Usage)
        );

        let read_only =
            parse_native_attach_arguments(["-dr", "-t", "work"].map(RawText::from)).unwrap();
        assert!(read_only.detach_others);
        assert!(read_only.read_only);
        assert_eq!(read_only.client_flags, None);
        assert_eq!(read_only.session.as_deref(), Some("work"));

        let no_update =
            parse_native_attach_arguments(["-E", "-t", "work"].map(RawText::from)).unwrap();
        assert!(no_update.no_update_environment);
        assert_eq!(no_update.session.as_deref(), Some("work"));
        assert_eq!(native_attach_command(&no_update).args, ["-E", "-t", "work"]);

        let flags = parse_native_attach_arguments(
            [
                "-f",
                "ignore-size",
                "-factive-pane,no-detach-on-destroy",
                "work",
            ]
            .map(RawText::from),
        )
        .unwrap();
        assert_eq!(
            flags.client_flags.as_deref(),
            Some("active-pane,no-detach-on-destroy")
        );

        let cwd = parse_native_attach_arguments(["-dc/tmp/work", "-t", "work"].map(RawText::from))
            .unwrap();
        assert!(cwd.detach_others);
        assert_eq!(cwd.working_directory.as_deref(), Some("/tmp/work"));
        assert_eq!(cwd.session.as_deref(), Some("work"));
        assert_eq!(
            native_attach_command(&cwd).args,
            ["-d", "-c", "/tmp/work", "-t", "work"]
        );

        let bundled = parse_native_attach_arguments(
            ["-dEr", "-fignore-size", "-c/tmp/work", "-twork"].map(RawText::from),
        )
        .unwrap();
        assert!(bundled.detach_others);
        assert!(bundled.no_update_environment);
        assert!(bundled.read_only);
        let command = native_attach_command(&bundled);
        assert_eq!(command.name, "attach-session");
        assert_eq!(
            command.args,
            [
                "-d",
                "-E",
                "-r",
                "-f",
                "ignore-size",
                "-c",
                "/tmp/work",
                "-t",
                "work"
            ]
        );

        assert_eq!(
            native_attach_command(&read_only).args,
            ["-d", "-r", "-t", "work"]
        );

        assert!(matches!(
            parse_native_attach_arguments(["-t", "one", "two"].map(RawText::from)),
            Err(super::NativeAttachArgumentError::Usage)
        ));

        for (arguments, expected) in [
            (
                vec!["-0"],
                ServerError::CommandParse(
                    "command attach-session: unknown flag -0".to_owned(),
                ),
            ),
            (
                vec!["-@"],
                ServerError::CommandParse(
                    "command attach-session: invalid flag -@".to_owned(),
                ),
            ),
            (
                vec!["--bogus"],
                ServerError::CommandParse(
                    "command attach-session: invalid flag --".to_owned(),
                ),
            ),
            (
                vec!["-?"],
                ServerError::CommandParse(
                    "usage: attach-session [-dErx] [-c working-directory] [-f flags] [-t target-session]"
                        .to_owned(),
                ),
            ),
            (
                vec!["-t"],
                ServerError::CommandParse(
                    "command attach-session: -t expects an argument".to_owned(),
                ),
            ),
            (
                vec!["-x0"],
                ServerError::CommandParse(
                    "command attach-session: unknown flag -0".to_owned(),
                ),
            ),
        ] {
            assert_eq!(
                parse_native_attach_arguments(arguments.into_iter().map(RawText::from)),
                Err(NativeAttachArgumentError::Command(expected))
            );
        }
        // -x is -d with the parent-hangup exit action, so the native parser
        // forwards it the way it forwards -d.
        let hangup = parse_native_attach_arguments(["-x"].map(RawText::from))
            .expect("attach-session -x parses");
        assert!(hangup.detach_others_hangup);
        assert!(!hangup.detach_others);
        assert_eq!(native_attach_command(&hangup).args, ["-x".to_owned()]);
        assert_eq!(
            parse_native_attach_arguments(["-t", "-?"].map(RawText::from))
                .unwrap()
                .session
                .as_deref(),
            Some("-?")
        );
        assert_eq!(
            parse_native_attach_arguments(["--", "work"].map(RawText::from))
                .unwrap()
                .session
                .as_deref(),
            Some("work")
        );
        let literal_restart =
            parse_native_attach_arguments(["--", "--restart-daemon"].map(RawText::from)).unwrap();
        assert!(!literal_restart.restart_daemon);
        assert_eq!(literal_restart.session.as_deref(), Some("--restart-daemon"));
    }

    #[test]
    fn bare_semicolons_split_commands_and_escaped_semicolons_stay_arguments() {
        let commands = split_command_chain(
            &[
                "start-server;",
                "show-environment",
                "-g",
                "TMUX_PLUGIN_MANAGER_PATH",
                ";",
                "display-message",
                r"a\;",
                r"\;",
            ]
            .map(RawText::from),
        );
        assert_eq!(
            commands,
            [
                zz_protocol::CommandInvocation::new("start-server", std::iter::empty::<&str>()),
                zz_protocol::CommandInvocation::new(
                    "show-environment",
                    ["-g", "TMUX_PLUGIN_MANAGER_PATH"]
                ),
                zz_protocol::CommandInvocation::new("display-message", ["a;", ";"])
            ]
        );
    }

    #[test]
    fn command_chains_preserve_output_and_abort_on_the_first_error() {
        let commands =
            split_command_chain(&["first", ";", "fail", ";", "never"].map(RawText::from));
        let mut seen = Vec::new();
        let mut output = Vec::new();
        let result = execute_command_chain(
            commands,
            |command| {
                seen.push(command.name.clone());
                match command.name.as_str() {
                    "first" => Ok(CommandOutcome {
                        stdout: "first output\n".into(),
                        ..CommandOutcome::default()
                    }),
                    "fail" => Err(17_u8),
                    _ => panic!("command after the failure executed"),
                }
            },
            |outcome| output.push(outcome.stdout.clone()),
        );
        assert_eq!(result, Err(17));
        assert_eq!(seen, ["first", "fail"]);
        assert_eq!(output, ["first output\n"]);
    }

    /// The pin keeps running a chain after a nonzero exit and reports the last
    /// nonzero status: `cmdq_next` drops the rest of the group only on
    /// `CMD_RETURN_ERROR`, while `c->retval` is simply overwritten.
    #[test]
    fn command_chains_continue_past_a_nonzero_exit_and_keep_the_last_status() {
        let commands = split_command_chain(&["three", ";", "zero", ";", "five"].map(RawText::from));
        let mut seen = Vec::new();
        let mut streams = Vec::new();
        let result: Result<u8, u8> = execute_command_chain(
            commands,
            |command| {
                seen.push(command.name.clone());
                Ok(match command.name.as_str() {
                    "three" => CommandOutcome {
                        stdout: "three out\n".into(),
                        stderr: "three err\n".to_owned(),
                        exit_code: 3,
                    },
                    "zero" => CommandOutcome {
                        stdout: "zero out\n".into(),
                        ..CommandOutcome::default()
                    },
                    _ => CommandOutcome {
                        exit_code: 5,
                        ..CommandOutcome::default()
                    },
                })
            },
            |outcome| streams.push((outcome.stdout.clone(), outcome.stderr.clone())),
        );
        assert_eq!(result, Ok(5));
        assert_eq!(seen, ["three", "zero", "five"]);
        assert_eq!(
            streams,
            [
                (RawText::from("three out\n"), "three err\n".to_owned()),
                ("zero out\n".into(), String::new()),
                (RawText::default(), String::new()),
            ]
        );
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
            "other"
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
                "--socket".into(),
                "/tmp/forwarded.sock".into(),
                "list-sessions".into(),
            ],
            PathBuf::from("/tmp/zz-env.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/forwarded.sock"));
        assert_eq!(parsed.host, None);
        assert_eq!(parsed.remaining, ["list-sessions"]);

        let parsed = application_arguments(
            ["daemon".into(), "--socket=/tmp/daemon.sock".into()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/daemon.sock"));
        assert_eq!(parsed.host, None);
        assert_eq!(parsed.remaining, ["daemon"]);
    }

    #[test]
    fn tmux_environment_never_becomes_an_implicit_zz_endpoint() {
        let tmux = std::ffi::OsStr::new("tmux.sock,123,4");
        assert!(implicit_tmux_endpoint_conflict(
            super::SocketSelectionSource::Default,
            None,
            Some(tmux),
        ));
        assert!(!implicit_tmux_endpoint_conflict(
            super::SocketSelectionSource::Default,
            Some(std::ffi::OsStr::new("/tmp/zz.sock")),
            Some(tmux),
        ));
        assert!(!implicit_tmux_endpoint_conflict(
            super::SocketSelectionSource::Path,
            None,
            Some(tmux),
        ));
        assert!(!implicit_tmux_endpoint_conflict(
            super::SocketSelectionSource::Default,
            None,
            None,
        ));
    }

    #[test]
    fn tmux_version_and_help_are_exact_early_outputs() {
        let version = application_arguments(
            ["-2uV".into(), "ignored".into()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(version.early_output, Some(TMUX_VERSION_OUTPUT));

        let help = application_arguments(
            ["-vh".into(), "ignored".into()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(help.early_output, Some(TMUX_USAGE));
    }

    #[test]
    fn tmux_flags_compose_before_the_command_word() {
        let parsed = application_arguments(
            [
                "-2u".into(),
                "-lN".into(),
                "-f".into(),
                "/tmp/first.conf".into(),
                "-f/tmp/second.conf".into(),
                "-S/tmp/tmux.sock".into(),
                "new-session".into(),
                "-d".into(),
                "-f".into(),
                "pane-command".into(),
            ],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/tmux.sock"));
        assert!(parsed.socket_source.is_overridden());
        assert!(parsed.no_start_server);
        assert!(parsed.login_shell);
        assert_eq!(
            parsed.mux_config_files,
            [
                PathBuf::from("/tmp/first.conf"),
                PathBuf::from("/tmp/second.conf")
            ]
        );
        assert_eq!(
            parsed.remaining,
            ["new-session", "-d", "-f", "pane-command"]
        );
    }

    #[test]
    fn the_last_zz_or_tmux_socket_selector_wins() {
        let parsed = application_arguments(
            [
                "-S".into(),
                "/tmp/tmux.sock".into(),
                "--socket=/tmp/zz.sock".into(),
                "list-sessions".into(),
            ],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/zz.sock"));

        let parsed = application_arguments(
            [
                "--socket".into(),
                "/tmp/zz.sock".into(),
                "-S/tmp/tmux.sock".into(),
                "list-sessions".into(),
            ],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/tmux.sock"));
    }

    #[test]
    fn tmux_shell_command_is_exclusive_and_preserves_login_mode() {
        let parsed = application_arguments(
            ["-lc".into(), "printf ok".into()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.shell_command.as_deref(), Some("printf ok"));
        assert!(parsed.login_shell);
        assert!(parsed.remaining.is_empty());

        assert_eq!(
            application_arguments(
                ["-cprintf ok".into(), "list-sessions".into()],
                PathBuf::from("/tmp/default.sock")
            ),
            Err(ApplicationArgumentError::Usage)
        );
    }

    #[test]
    fn unsupported_and_unknown_tmux_flags_fail_loudly() {
        for flag in ["-8", "-d", "-U", "-x"] {
            let expected = format!("zz: unknown option -- {}\n{TMUX_USAGE}", &flag[1..2]);
            assert!(
                matches!(
                    application_arguments([RawText::from(flag)], PathBuf::from("/tmp/default.sock")),
                    Err(ApplicationArgumentError::Raw(message)) if message == expected
                ),
                "{flag}"
            );
        }
        assert_eq!(
            application_arguments(["--unknown".into()], PathBuf::from("/tmp/default.sock")),
            Err(ApplicationArgumentError::Usage)
        );
        assert!(matches!(
            application_arguments(["-L".into()], PathBuf::from("/tmp/default.sock")),
            Err(ApplicationArgumentError::Raw(message))
                if message == format!("zz: option requires an argument -- L\n{TMUX_USAGE}")
        ));
        assert!(matches!(
            application_arguments(["-D".into()], PathBuf::from("/tmp/default.sock")),
            Err(ApplicationArgumentError::Message(message))
                if message == "-D foreground server mode is not supported; use `zz daemon`"
        ));
        assert_eq!(
            application_arguments(
                ["-D".into(), "list-sessions".into()],
                PathBuf::from("/tmp/default.sock")
            ),
            Err(ApplicationArgumentError::Usage)
        );
    }

    #[test]
    fn control_flags_count_and_compose_with_tmux_options() {
        for (flag, expected) in [("-C", 1), ("-CC", 2), ("-CCC", 3)] {
            let parsed = application_arguments(
                [RawText::from(flag), "list-sessions".into()],
                PathBuf::from("/tmp/default.sock"),
            )
            .unwrap();
            assert_eq!(parsed.control_mode, expected);
            assert_eq!(parsed.remaining, ["list-sessions"]);
        }
        let parsed = application_arguments(
            [
                "-2CulN".into(),
                "-f/tmp/control.conf".into(),
                "-S/tmp/control.sock".into(),
                "new-session".into(),
            ],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.control_mode, 1);
        assert!(parsed.login_shell);
        assert!(parsed.no_start_server);
        assert_eq!(
            parsed.mux_config_files,
            [PathBuf::from("/tmp/control.conf")]
        );
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/control.sock"));
        assert_eq!(parsed.remaining, ["new-session"]);
        let shell = application_arguments(
            ["-Cc".into(), "printf ok".into()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(shell.control_mode, 1);
        assert_eq!(shell.shell_command.as_deref(), Some("printf ok"));
    }

    #[cfg(unix)]
    #[test]
    fn tmux_label_uses_tmpdir_then_the_tmp_fallback() {
        use std::os::unix::fs::MetadataExt as _;

        let directory = tempfile::tempdir().expect("temporary TMUX_TMPDIR");
        let root = std::fs::canonicalize(directory.path()).expect("canonical TMUX_TMPDIR");
        assert_eq!(
            tmux_socket_root(Some(directory.path().as_os_str())),
            Some(root.clone())
        );
        assert_eq!(
            tmux_socket_root(Some(directory.path().join("missing").as_os_str())),
            std::fs::canonicalize("/tmp").ok()
        );
        assert_eq!(tmux_socket_root(None), std::fs::canonicalize("/tmp").ok());

        let uid = rustix::process::getuid().as_raw();
        let path = tmux_label_socket_path("work", Some(directory.path().as_os_str())).unwrap();
        assert_eq!(path, root.join(format!("tmux-{uid}/work")));
        let metadata = std::fs::symlink_metadata(path.parent().unwrap()).unwrap();
        assert_eq!(metadata.uid(), uid);
        assert_eq!(metadata.mode() & 0o007, 0);
    }

    #[test]
    fn host_flag_accepts_both_spellings_and_conflicts_with_socket() {
        let parsed = application_arguments(
            ["--host".into(), "desktop".into(), "list-sessions".into()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.socket_path, PathBuf::from("/tmp/default.sock"));
        assert_eq!(parsed.host.as_deref(), Some("desktop"));
        assert_eq!(parsed.remaining, ["list-sessions"]);

        let parsed = application_arguments(
            ["list-panes".into(), "--host=gpu".into()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.host.as_deref(), Some("gpu"));
        assert_eq!(parsed.remaining, ["list-panes"]);

        for arguments in [
            vec![
                "--host".into(),
                "desktop".into(),
                "--socket=/tmp/daemon.sock".into(),
                "list-sessions".into(),
            ],
            vec![
                "--socket".into(),
                "/tmp/daemon.sock".into(),
                "--host=desktop".into(),
                "list-sessions".into(),
            ],
        ] as [Vec<RawText>; 2]
        {
            assert!(application_arguments(arguments, PathBuf::from("/tmp/default.sock")).is_err());
        }
    }

    #[test]
    fn a_prompt_would_reach_the_command_dispatcher_if_askpass_mode_did_not_come_first() {
        let parsed = application_arguments(
            ["demfabris@xps's password: ".into()],
            PathBuf::from("/tmp/default.sock"),
        )
        .unwrap();
        assert_eq!(parsed.remaining, ["demfabris@xps's password: "]);
    }

    #[test]
    fn socket_flag_requires_a_non_empty_path() {
        assert!(
            application_arguments(["--socket".into()], PathBuf::from("/tmp/default.sock")).is_err()
        );
        assert!(
            application_arguments(["--socket=".into()], PathBuf::from("/tmp/default.sock"))
                .is_err()
        );
        assert!(
            application_arguments(["--host".into()], PathBuf::from("/tmp/default.sock")).is_err()
        );
        assert!(
            application_arguments(["--host=".into()], PathBuf::from("/tmp/default.sock")).is_err()
        );
    }
}
