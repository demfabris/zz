#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests spawn helper processes from threads with an empty signal mask"
    )
)]

#[cfg(not(windows))]
extern crate mimalloc;
#[cfg(all(test, not(target_os = "ios")))]
use zz_daemon::{CommandStdinSink, append_stdin_payload};
mod control_mode;
pub mod diagnostics;
mod events;
mod fleet;

#[cfg(all(test, not(target_os = "ios")))]
use std::borrow::Cow;
#[cfg(not(target_os = "ios"))]
use std::{
    cell::RefCell,
    io::{self, ErrorKind, IsTerminal as _, Write as _},
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

#[cfg(not(target_os = "ios"))]
use zz_daemon::{
    CommandClient, CommandOutcome, Daemon, Endpoint, ExecChain, ExecChainEnd, ExecClassifier,
    classify_local_connect_error, terminate_incompatible_daemon, unmasked::SpawnUnmasked as _,
};
use zz_daemon::{DaemonError, InteractiveClient};
#[cfg(not(target_os = "ios"))]
use zz_mux::MuxEngine;
#[cfg(not(target_os = "ios"))]
use zz_protocol::{
    CommandInvocation, ExecResume, ExecResumeKind, MAX_CLIENT_WORKING_DIRECTORY_BYTES,
    PROTOCOL_VERSION, PreparedCommand, RawText, ServerError, ServerHello, StdoutClaim,
    canonical_command, catalog_command_spec,
};
use zz_terminal::TerminalColorScheme;

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
pub const APP_STARTUP_DIRECTORY_ENV: &str = "ZZ_APP_STARTUP_DIRECTORY";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandLineOrigin {
    Launcher,
    Application,
}

pub type BrowserProvider = fn() -> Option<Box<dyn zz_tui::browser::BrowserFrameProvider>>;

#[derive(Clone, Copy)]
pub struct StartupOptions {
    pub origin: CommandLineOrigin,
    pub browser_provider: Option<BrowserProvider>,
}

const DAEMON_BOOTSTRAP_SERVER_ID_ARGUMENT: &str = "--bootstrap-server-id";
#[cfg(not(target_os = "ios"))]
const DAEMON_BOOTSTRAP_READY_FD_ARGUMENT: &str = "--bootstrap-ready-fd";
#[cfg(not(target_os = "ios"))]
const DAEMON_BOOTSTRAP_CLIENT_CWD_ARGUMENT: &str = "--bootstrap-client-cwd";
#[cfg(not(target_os = "ios"))]
const DAEMON_READY_DEADLINE: Duration = Duration::from_secs(6);
#[cfg(not(target_os = "ios"))]
static DAEMON_SPAWN_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(not(target_os = "ios"))]
pub enum Startup {
    Application(PathBuf),
    Exit(ExitCode),
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, Default, PartialEq, Eq)]
struct DaemonBootstrapArguments {
    server_id: Option<u64>,
    ready_fd: Option<i32>,
    client_working_directory: Option<PathBuf>,
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, PartialEq, Eq)]
enum DaemonBootstrapArgumentError {
    ServerId,
    ReadyFd,
    ClientWorkingDirectory,
}

#[cfg(not(target_os = "ios"))]
impl DaemonBootstrapArgumentError {
    fn message(&self) -> &'static str {
        match self {
            Self::ServerId => "invalid bootstrap server id",
            Self::ReadyFd => "invalid bootstrap ready fd",
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
    let (ready_fd, remaining) = match remaining {
        [flag, fd, remaining @ ..] if flag == DAEMON_BOOTSTRAP_READY_FD_ARGUMENT => (
            Some(
                fd.parse::<i32>()
                    .ok()
                    .filter(|fd| *fd > 2)
                    .ok_or(DaemonBootstrapArgumentError::ReadyFd)?,
            ),
            remaining,
        ),
        [flag] if flag == DAEMON_BOOTSTRAP_READY_FD_ARGUMENT => {
            return Err(DaemonBootstrapArgumentError::ReadyFd);
        }
        remaining => (None, remaining),
    };
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
        ready_fd,
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
    NativeUsage,
    Command(ServerError),
}

#[cfg(not(target_os = "ios"))]
struct TmuxLabelCreationError {
    message: String,
    kind: ErrorKind,
}

#[cfg(not(target_os = "ios"))]
#[must_use]
pub fn run_startup(socket_path: &Path, options: StartupOptions) -> Startup {
    diagnostics::init(bare_command_line_opens_application(
        options.origin,
        launched_by_launch_services(),
    ));
    let arguments =
        match application_arguments(diagnostics::application_args(), socket_path.to_path_buf()) {
            Ok(arguments) => arguments,
            Err(ApplicationArgumentError::Message(error)) => {
                eprintln!("zz: {error}");
                return Startup::Exit(exit_code_for(CliFailure::Usage));
            }
            Err(ApplicationArgumentError::Raw(error)) => {
                eprintln!("{error}");
                return Startup::Exit(exit_code_for(CliFailure::TmuxUsage));
            }
            Err(ApplicationArgumentError::Usage) => {
                eprintln!("{TMUX_USAGE}");
                return Startup::Exit(exit_code_for(CliFailure::TmuxUsage));
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
        early_output,
        client_utf8,
        client_features,
    } = arguments;
    zz_daemon::set_client_terminal_flags(zz_daemon::ClientTerminalFlags {
        utf8: client_utf8,
        features: client_features,
    });
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
            return Startup::Exit(exit_code_for(CliFailure::Usage));
        }
        if implicit_tmux_conflict {
            eprintln!("{FOREIGN_TMUX_ERROR}");
            return Startup::Exit(exit_code_for(CliFailure::Runtime));
        }
        return Startup::Exit(control_mode::run(
            &socket_path,
            socket_source,
            &mux_config_files,
            no_start_server,
            control_mode,
            &remaining,
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
        &options,
        implicit_tmux_conflict,
    ) {
        Startup::Exit(exit)
    } else {
        if options.origin == CommandLineOrigin::Application
            || std::env::args_os()
                .skip(1)
                .ne([std::ffi::OsString::from("app")])
        {
            configure_application_working_directory();
        }
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
#[cfg(not(windows))]
const MI_OPTION_PURGE_DELAY: std::ffi::c_int = 15;
#[cfg(target_os = "macos")]
const MI_OPTION_OS_TAG: std::ffi::c_int = 18;
#[cfg(target_os = "macos")]
const MI_DEFAULT_OS_TAG: std::ffi::c_long = 100;
#[cfg(target_os = "macos")]
const ALLOCATOR_OS_TAG: std::ffi::c_long = 241;
#[cfg(not(windows))]
const DAEMON_PURGE_DELAY_MS: std::ffi::c_long = 0;

#[cfg(not(windows))]
#[allow(unsafe_code)]
unsafe extern "C" {
    #[cfg(target_os = "macos")]
    fn mi_option_get(option: std::ffi::c_int) -> std::ffi::c_long;
    fn mi_option_set(option: std::ffi::c_int, value: std::ffi::c_long);
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
pub extern "C" fn tag_allocator_memory() {
    if unsafe { mi_option_get(MI_OPTION_OS_TAG) } == MI_DEFAULT_OS_TAG {
        unsafe { mi_option_set(MI_OPTION_OS_TAG, ALLOCATOR_OS_TAG) };
    }
}

#[cfg(not(windows))]
#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
fn purge_freed_memory_promptly() {
    if std::env::var_os("MIMALLOC_PURGE_DELAY").is_none() {
        unsafe { mi_option_set(MI_OPTION_PURGE_DELAY, DAEMON_PURGE_DELAY_MS) };
    }
}

#[cfg(target_os = "windows")]
pub fn attach_parent_console() {
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
#[must_use]
pub fn run_askpass_mode() -> Option<ExitCode> {
    let socket = std::env::var_os(zz_daemon::ASKPASS_SOCKET_ENV)?;
    let prompt = std::env::args_os().nth(1).unwrap_or_default();
    Some(zz_daemon::run_helper(
        Path::new(&socket),
        &prompt.to_string_lossy(),
    ))
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, PartialEq, Eq)]
pub struct ApplicationArguments {
    pub socket_path: PathBuf,
    socket_source: SocketSelectionSource,
    host: Option<String>,
    remaining: Vec<RawText>,
    mux_config_files: Vec<PathBuf>,
    no_start_server: bool,
    control_mode: u8,
    shell_command: Option<String>,
    login_shell: bool,
    early_output: Option<&'static str>,
    client_utf8: bool,
    client_features: Vec<String>,
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, PartialEq, Eq)]
pub enum ApplicationArgumentError {
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
pub fn application_arguments(
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
    let mut client_utf8 = false;
    let mut client_features = Vec::new();
    let mut parsing_tmux_options = true;
    let mut arguments = arguments.into_iter().collect::<Vec<_>>().into_iter();
    while let Some(argument) = arguments.next() {
        if argument == diagnostics::SOCKET_ARGUMENT {
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
        } else if parsing_tmux_options
            && matches!(argument.as_str(), "--version" | "--kill-server" | "--help")
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
                    '2' => client_features.push("256".to_owned()),
                    'q' | 'v' => {}
                    'u' => client_utf8 = true,
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
                            client_utf8: false,
                            client_features: Vec::new(),
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
                        client_features.push(value(&mut arguments)?.to_string());
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
                            client_utf8: false,
                            client_features: Vec::new(),
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
        early_output: None,
        client_utf8,
        client_features,
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
    startup_options: &StartupOptions,
    implicit_tmux_conflict: bool,
) -> Option<ExitCode> {
    if let Some(shell_command) = shell_command {
        if implicit_tmux_conflict && host.is_none() {
            eprintln!("{FOREIGN_TMUX_ERROR}");
            return Some(exit_code_for(CliFailure::Runtime));
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
    if command_chain.is_empty()
        && host.is_none()
        && !bare_command_line_opens_application(
            startup_options.origin,
            launched_by_launch_services(),
        )
    {
        command_chain =
            default_client_command_chain(socket_path, mux_config_files, no_start_server);
    }
    let Some(invocation) = command_chain.first().cloned() else {
        if host.is_some() {
            eprintln!("zz: --host requires a command");
            return Some(exit_code_for(CliFailure::Usage));
        }
        return None;
    };
    let command = invocation.name.clone();
    if command == "--help" || command == "help" {
        print!("{}", top_level_help());
        return Some(ExitCode::SUCCESS);
    }
    if invocation.args.iter().any(|argument| argument == "--help") {
        return Some(match command_help(&command) {
            Ok(help) => {
                print!("{help}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{}", server_error_message(&error));
                exit_code_for(CliFailure::Server(&error))
            }
        });
    }
    let progress_target = if command == "agent-send" {
        match events::progress_target(&invocation.args) {
            Ok(target) => target,
            Err(message) => {
                eprintln!("{message}");
                return Some(exit_code_for(CliFailure::Usage));
            }
        }
    } else {
        None
    };
    if progress_target.is_some() && host.is_some() {
        eprintln!("agent-send: --progress is local only");
    }
    if command == "events" {
        if host.is_some() || command_chain.len() != 1 {
            eprintln!("events: use a single local command: zz events [-t target]");
            return Some(exit_code_for(CliFailure::Usage));
        }
        if implicit_tmux_conflict {
            eprintln!("{FOREIGN_TMUX_ERROR}");
            return Some(exit_code_for(CliFailure::Runtime));
        }
        return Some(events::run(
            socket_path,
            socket_source,
            mux_config_files,
            no_start_server,
            &invocation.args,
        ));
    }
    if command == "app" {
        if host.is_some() || !invocation.args.is_empty() || command_chain.len() != 1 {
            eprintln!("{NATIVE_APP_USAGE}");
            return Some(exit_code_for(CliFailure::Usage));
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
                    exit_code_for(CliFailure::Usage)
                }
            },
        );
    }

    if host.is_some() && matches!(command.as_str(), "daemon" | "proxy" | "fleet") {
        eprintln!("zz: --host is not supported for `{command}`");
        return Some(exit_code_for(CliFailure::Usage));
    }

    if command == "daemon" {
        let bootstrap = match parse_daemon_bootstrap_arguments(&invocation.args) {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                eprintln!("zz daemon: {}", error.message());
                return Some(exit_code_for(CliFailure::Usage));
            }
        };
        #[cfg(not(windows))]
        purge_freed_memory_promptly();
        let mut daemon =
            Daemon::new(socket_path).with_mux_config_files(mux_config_files.iter().cloned());
        if let Some(server_id) = bootstrap.server_id {
            daemon = daemon.with_server_id(server_id);
        }
        if let Some(client_working_directory) = bootstrap.client_working_directory {
            daemon = daemon.with_initial_client_working_directory(client_working_directory);
        }
        #[cfg(unix)]
        if let Some(fd) = bootstrap.ready_fd {
            daemon = daemon.with_bootstrap_ready_fd(fd);
        }
        return Some(match daemon.run_foreground() {
            Ok(()) | Err(DaemonError::AlreadyRunning(_)) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("zz daemon: {error}");
                exit_code_for(CliFailure::Runtime)
            }
        });
    }

    if command == "proxy" {
        return Some(match zz_daemon::run_socket_proxy(socket_path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("zz proxy: {error}");
                exit_code_for(CliFailure::Runtime)
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
                    exit_code_for(
                        if error.starts_with("usage:")
                            || error == "ssh destination must not start with `-`"
                        {
                            CliFailure::Usage
                        } else {
                            CliFailure::Runtime
                        },
                    )
                }
            },
        );
    }

    if implicit_tmux_conflict && host.is_none() {
        eprintln!("{FOREIGN_TMUX_ERROR}");
        return Some(exit_code_for(CliFailure::Runtime));
    }

    if command == "--kill-server" {
        return Some(match host {
            Some(host) => run_host_kill_server(host, invocation.args),
            None => run_kill_server(socket_path, invocation.args),
        });
    }

    let start_server = !no_start_server && tmux_command_starts_server(&command);
    let routing = ChainRouting {
        socket_path,
        host,
        mux_config_files,
        startup_options,
        command: &command,
        start_server,
    };
    if let Some(host) = host {
        return Some(run_host_command_chain(
            &routing,
            host,
            socket_source,
            command_chain,
        ));
    }

    let mut resume = None;
    let failure = match CommandClient::connect(socket_path) {
        Ok(client) => {
            match run_local_command_chain(
                &routing,
                client,
                &command_chain,
                progress_target.as_deref(),
                None,
            ) {
                LocalChain::Done(exit) => return Some(exit),
                LocalChain::Resume(prepared, client) => {
                    resume = Some((prepared, *client));
                    None
                }
                LocalChain::Unanswered(error) => Some(error),
            }
        }
        Err(error) => Some(error),
    };
    let failure = failure.map(|error| classify_local_connect_error(socket_path, error));

    if failure.is_some() {
        let static_commands = if matches!(command.as_str(), "attach" | "attach-session") {
            if let Err(error) = parse_native_attach_arguments(command_chain[0].args.clone()) {
                return Some(print_native_attach_argument_error(error));
            }
            &command_chain[1..]
        } else {
            &command_chain
        };
        if let Err(error) = zz_mux::validate_static_command_chain(static_commands) {
            eprintln!("{}", server_error_message(&error));
            return Some(exit_code_for(CliFailure::Server(&error)));
        }
    }
    let failure = match failure {
        Some(error)
            if !start_server
                && tmux_command_starts_server(&command)
                && daemon_is_spawnable(&error) =>
        {
            eprintln!("{}", format_local_command_error(socket_path, error));
            return Some(exit_code_for(CliFailure::Runtime));
        }
        failure => failure,
    };

    let failure = if start_server && failure.as_ref().is_some_and(daemon_is_spawnable) {
        if let Some(error) = tmux_label_creation_error(socket_path, socket_source, start_server) {
            eprintln!("{}", error.message);
            let nested_label_new_session =
                canonical_command(&command) == "new-session" && error.kind == ErrorKind::NotFound;
            return Some(if nested_label_new_session {
                ExitCode::SUCCESS
            } else {
                exit_code_for(CliFailure::Runtime)
            });
        }
        let client = match spawn_and_connect_command_client(socket_path, mux_config_files) {
            Ok(connected) => connected,
            Err(error) => {
                eprintln!("{}", format_local_command_error(socket_path, error));
                return Some(exit_code_for(CliFailure::Runtime));
            }
        };
        let (client, spawned_server_id) = client;
        match run_local_command_chain(
            &routing,
            client,
            &command_chain,
            progress_target.as_deref(),
            Some(spawned_server_id),
        ) {
            LocalChain::Done(exit) => return Some(exit),
            LocalChain::Resume(prepared, client) => {
                resume = Some((prepared, *client));
                None
            }
            LocalChain::Unanswered(error) => {
                eprintln!("{}", format_local_command_error(socket_path, error));
                return Some(exit_code_for(CliFailure::Runtime));
            }
        }
    } else {
        failure
    };

    if command == "kill-server" && resume.is_none() {
        return Some(run_kill_server(socket_path, invocation.args));
    }

    let (new_session_tui, native_attach) = match &resume {
        Some((resume, _)) => (
            resume.kind == ExecResumeKind::NewSession,
            resume.kind == ExecResumeKind::NativeAttach,
        ),
        None => (
            command_chain_uses_tui(&command_chain) || attach_prefix_uses_tui(&command),
            matches!(command.as_str(), "attach" | "attach-session"),
        ),
    };
    let (prepared, connection) = match resume {
        Some((resume, client)) => (Some(resume.commands), Some(client)),
        None => (None, None),
    };
    if (prepared.is_some() || start_server) && new_session_tui {
        return Some(run_tui_chain(&routing, prepared, command_chain, connection));
    }
    if native_attach {
        return Some(run_native_attach(
            &routing,
            prepared,
            command_chain,
            connection,
        ));
    }

    if let Some(error) = tmux_label_creation_error(socket_path, socket_source, start_server) {
        eprintln!("{}", error.message);
        let nested_label_new_session =
            canonical_command(&command) == "new-session" && error.kind == ErrorKind::NotFound;
        return Some(if nested_label_new_session {
            ExitCode::SUCCESS
        } else {
            exit_code_for(CliFailure::Runtime)
        });
    }
    let failure = failure.unwrap_or_else(|| {
        DaemonError::Server(ServerError::Internal(
            "daemon handed back a chain it cannot resume".to_owned(),
        ))
    });
    eprintln!("{}", format_local_command_error(socket_path, failure));
    Some(exit_code_for(CliFailure::Runtime))
}

#[cfg(not(target_os = "ios"))]
struct ChainRouting<'a> {
    socket_path: &'a Path,
    host: Option<&'a str>,
    mux_config_files: &'a [PathBuf],
    startup_options: &'a StartupOptions,
    command: &'a str,
    start_server: bool,
}

#[cfg(not(target_os = "ios"))]
enum LocalChain {
    Done(ExitCode),
    Resume(ExecResume, Box<CommandClient>),
    Unanswered(DaemonError),
}

#[cfg(not(target_os = "ios"))]
fn prepare_command_client(client: &mut CommandClient) {
    client.enable_stdin();
    client.set_stderr_handler(print_command_error);
    client.set_stdout_handler(print_released_command_output);
}

#[cfg(not(target_os = "ios"))]
fn emit_chain_outcome(outcome: &CommandOutcome) -> u8 {
    let status = print_chain_output(&outcome.stdout, raw_command_output(outcome.stdout_claim));
    print_command_error(&outcome.stderr);
    status
}

#[cfg(not(target_os = "ios"))]
fn run_local_command_chain(
    routing: &ChainRouting<'_>,
    mut client: CommandClient,
    command_chain: &[CommandInvocation],
    progress_target: Option<&str>,
    spawned_server_id: Option<u64>,
) -> LocalChain {
    let _progress = if command_chain.len() == 1
        && let Some(pane) = progress_target
    {
        match events::Progress::start(routing.socket_path, pane.to_owned()) {
            Ok(progress) => Some(progress),
            Err(error) => {
                eprintln!("agent-send: --progress: {error}");
                return LocalChain::Done(exit_code_for(CliFailure::Runtime));
            }
        }
    } else {
        None
    };
    prepare_command_client(&mut client);
    let classify: ExecClassifier<'_> = &zz_daemon::exec_resume_kind;
    let chain = ExecChain {
        commands: command_chain.to_vec(),
        spawned_server_id,
        expect_server_id: None,
        resume: Some(classify),
        prepared: false,
        last: true,
    };
    match client.exec_chain(chain, emit_chain_outcome) {
        Ok(ExecChainEnd::Ran { exit_code }) => LocalChain::Done(ExitCode::from(exit_code)),
        Ok(ExecChainEnd::Resume(resume)) => LocalChain::Resume(resume, Box::new(client)),
        Ok(ExecChainEnd::Rejected(error)) => {
            eprintln!("{}", server_error_message(&error));
            LocalChain::Done(exit_code_for(CliFailure::Server(&error)))
        }
        Ok(ExecChainEnd::Failed(error))
            if routing.command == "kill-server" && daemon_transport_failure(&error) =>
        {
            LocalChain::Done(recover_kill_server_failure(routing.socket_path, &error))
        }
        Ok(ExecChainEnd::Failed(error)) => LocalChain::Done(print_chain_failure(error)),
        Ok(ExecChainEnd::ServerMismatch) => LocalChain::Done(exit_code_for(CliFailure::Runtime)),
        Err(error) => LocalChain::Unanswered(error),
    }
}

#[cfg(not(target_os = "ios"))]
fn print_chain_failure(error: DaemonError) -> ExitCode {
    match error {
        DaemonError::CommandFailed { output, error } => {
            print_chain_output(&output, false);
            eprintln!("{}", command_error_message(&error));
            exit_code_for(CliFailure::Runtime)
        }
        error => {
            eprintln!("{}", command_error_message(&error));
            exit_code_for(CliFailure::Daemon(&error))
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn run_host_command_chain(
    routing: &ChainRouting<'_>,
    host: &str,
    socket_source: SocketSelectionSource,
    command_chain: Vec<CommandInvocation>,
) -> ExitCode {
    if command_chain_uses_tui(&command_chain) || attach_prefix_uses_tui(routing.command) {
        return run_tui_chain(routing, None, command_chain, None);
    }
    if matches!(routing.command, "attach" | "attach-session") {
        return run_native_attach(routing, None, command_chain, None);
    }
    if let Some(error) =
        tmux_label_creation_error(routing.socket_path, socket_source, routing.start_server)
    {
        eprintln!("{}", error.message);
        let nested_label_new_session = canonical_command(routing.command) == "new-session"
            && error.kind == ErrorKind::NotFound;
        return if nested_label_new_session {
            ExitCode::SUCCESS
        } else {
            exit_code_for(CliFailure::Runtime)
        };
    }
    let mut client = match connect_host_command_client(host) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("zz: {error}");
            return exit_code_for(CliFailure::Runtime);
        }
    };
    prepare_command_client(&mut client);
    match client.exec_chain(ExecChain::new(command_chain), emit_chain_outcome) {
        Ok(ExecChainEnd::Ran { exit_code }) => ExitCode::from(exit_code),
        Ok(ExecChainEnd::Rejected(error)) => {
            eprintln!("{}", server_error_message(&error));
            exit_code_for(CliFailure::Server(&error))
        }
        Ok(ExecChainEnd::Failed(error)) => print_chain_failure(error),
        Ok(ExecChainEnd::Resume(_) | ExecChainEnd::ServerMismatch) => {
            exit_code_for(CliFailure::Runtime)
        }
        Err(error) => {
            eprintln!("{}", command_error_message(&error));
            exit_code_for(CliFailure::Daemon(&error))
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn tui_request<'a>(
    routing: &ChainRouting<'a>,
    options: &'a zz_tui::RunOptions,
    reconnect: &'a dyn Fn(
        &Path,
        bool,
        Option<zz_protocol::AttachOperation>,
    ) -> Result<InteractiveClient, DaemonError>,
) -> zz_tui::RunRequest<'a> {
    match routing.startup_options.browser_provider {
        Some(provider) => options.with_browser_provider(provider),
        None => zz_tui::RunRequest::from(options),
    }
    .with_local_reconnect(reconnect)
}

#[cfg(not(target_os = "ios"))]
fn run_tui_chain(
    routing: &ChainRouting<'_>,
    prepared: Option<Vec<PreparedCommand>>,
    command_chain: Vec<CommandInvocation>,
    connection: Option<CommandClient>,
) -> ExitCode {
    let options = zz_tui::RunOptions {
        socket_path: routing.socket_path.to_path_buf(),
        host: routing.host.map(str::to_owned),
        session: None,
        restart_daemon: false,
        detach_others: false,
        read_only: false,
        client_flags: None,
    };
    let mux_config_files = routing.mux_config_files;
    let reconnect =
        |path: &Path, client_has_terminal, attach: Option<zz_protocol::AttachOperation>| {
            connect_terminal_surface_client_with_config(
                path,
                TerminalColorScheme::Dark,
                mux_config_files,
                client_has_terminal,
                attach.as_ref(),
            )
        };
    let request = tui_request(routing, &options, &reconnect);
    let result = match (connection, prepared) {
        (Some(client), Some(commands)) => {
            match client.into_interactive(zz_protocol::AttachOperation::Commands(commands)) {
                Ok(initial) => zz_tui::run_connected(request, initial),
                Err(error) => {
                    eprintln!("{}", command_error_message(&error));
                    return exit_code_for(CliFailure::Daemon(&error));
                }
            }
        }
        (_, Some(commands)) => zz_tui::run_prepared_new_session(request, commands),
        (_, None) => zz_tui::run_new_session(request, command_chain),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            exit_code_for(CliFailure::Runtime)
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn run_native_attach(
    routing: &ChainRouting<'_>,
    prepared: Option<Vec<PreparedCommand>>,
    mut command_chain: Vec<CommandInvocation>,
    connection: Option<CommandClient>,
) -> ExitCode {
    let options = match parse_native_attach_arguments(command_chain[0].args.clone()) {
        Ok(options) => options,
        Err(error) => {
            return print_native_attach_argument_error(error);
        }
    };
    if options.restart_daemon && routing.host.is_some() {
        eprintln!("zz: --restart-daemon is only supported for the local daemon");
        return exit_code_for(CliFailure::Usage);
    }
    // -x has no RunOptions field: route it through the real attach command so
    // the daemon sees the flag that picks the parent-hangup eviction.
    let needs_command_attach = options.no_update_environment
        || options.working_directory.is_some()
        || options.detach_others_hangup;
    let default_attach =
        !needs_command_attach && command_chain.len() == 1 && options.session.is_none();
    let attach_command = native_attach_command(&options);
    let options = zz_tui::RunOptions {
        socket_path: routing.socket_path.to_path_buf(),
        host: routing.host.map(str::to_owned),
        session: options.session,
        restart_daemon: options.restart_daemon,
        detach_others: options.detach_others,
        read_only: options.read_only,
        client_flags: options.client_flags,
    };
    let mux_config_files = routing.mux_config_files;
    let reconnect =
        |path: &Path, client_has_terminal, attach: Option<zz_protocol::AttachOperation>| {
            connect_terminal_surface_client_with_config(
                path,
                TerminalColorScheme::Dark,
                mux_config_files,
                client_has_terminal,
                attach.as_ref(),
            )
        };
    let request = tui_request(routing, &options, &reconnect);
    let result = if let (Some(client), Some(mut commands)) = (connection, prepared.clone()) {
        commands[0].invocation = attach_command;
        match client.into_interactive(zz_protocol::AttachOperation::Commands(commands)) {
            Ok(initial) => zz_tui::run_connected(request, initial),
            Err(error) => {
                eprintln!("{}", command_error_message(&error));
                return exit_code_for(CliFailure::Daemon(&error));
            }
        }
    } else if command_chain.len() > 1 || needs_command_attach {
        if let Some(mut commands) = prepared {
            commands[0].invocation = attach_command;
            zz_tui::run_prepared_new_session(request, commands)
        } else {
            command_chain[0] = attach_command;
            zz_tui::run_new_session(request, command_chain)
        }
    } else {
        zz_tui::run(request)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if default_attach && error.to_string() == "can't find session: current session" {
                eprintln!("no sessions");
            } else {
                eprintln!("{error}");
            }
            exit_code_for(CliFailure::Runtime)
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn top_level_help() -> String {
    use std::fmt::Write as _;

    let mut help = String::from("usage: zz [global flags] <command> [flags]\n\nzz verbs\n");
    for name in zz_protocol::NATIVE_COMMAND_NAMES {
        if let Some(spec) = catalog_command_spec(name)
            && !spec.is_internal()
        {
            let _ = writeln!(help, "  {}  {}", spec.name, spec.description);
        }
    }
    help.push_str("\ntmux commands\n");
    for spec in zz_protocol::command_specs().filter(|spec| {
        !spec.is_internal() && !zz_protocol::NATIVE_COMMAND_NAMES.contains(&spec.name)
    }) {
        let _ = writeln!(help, "  {}{}", spec.name, help_aliases(spec));
    }
    help.push_str("\nnot implemented\n");
    for name in zz_protocol::CommandSpec::UNIMPLEMENTED_TMUX_COMMANDS {
        let _ = writeln!(help, "  {name}");
    }
    help.push_str("\nRun zz tools for the agent guide.\n");
    help
}

#[cfg(not(target_os = "ios"))]
fn help_aliases(spec: &zz_protocol::CommandSpec) -> String {
    if spec.aliases.is_empty() {
        String::new()
    } else {
        format!(" ({})", spec.aliases.join(", "))
    }
}

#[cfg(not(target_os = "ios"))]
fn help_value(kind: zz_protocol::CommandValueKind) -> &'static str {
    use zz_protocol::CommandValueKind;

    match kind {
        CommandValueKind::FreeForm => "value",
        CommandValueKind::Session => "session",
        CommandValueKind::Window => "window",
        CommandValueKind::Pane => "pane",
        CommandValueKind::Layout => "layout",
        CommandValueKind::PaneKind => "pane-kind",
        CommandValueKind::KeyTable => "key-table",
        CommandValueKind::SetOption => "option",
        CommandValueKind::Boolean => "boolean",
    }
}

#[cfg(not(target_os = "ios"))]
fn command_help(command: &str) -> Result<String, ServerError> {
    use std::fmt::Write as _;

    let spec = match zz_protocol::resolve_command(command) {
        zz_protocol::CommandResolution::Canonical(name) => catalog_command_spec(name),
        zz_protocol::CommandResolution::Unimplemented(name) => {
            zz_protocol::unimplemented_tmux_command_spec(name)
        }
        zz_protocol::CommandResolution::Ambiguous(message) => {
            return Err(ServerError::NativeCommandParse(message));
        }
        zz_protocol::CommandResolution::Unknown => None,
    }
    .ok_or_else(|| ServerError::NativeCommandParse(format!("unknown command: {command}")))?;
    let mut help = format!(
        "{}{}\n{}\nusage: zz {} {}\n",
        spec.name,
        help_aliases(spec),
        spec.description,
        spec.name,
        spec.usage,
    );
    if !spec.options.is_empty() {
        help.push_str("\noptions\n");
    }
    for option in spec.options {
        let value = if option.optional_value {
            " [value]".to_owned()
        } else {
            option
                .value
                .map(|kind| format!(" <{}>", help_value(kind)))
                .unwrap_or_default()
        };
        let unsupported = if option.unsupported {
            " (not supported)"
        } else {
            ""
        };
        let _ = writeln!(
            help,
            "  {}{value}  {}{unsupported}",
            option.name, option.description
        );
    }
    if !spec.positionals.is_empty() || spec.variadic.is_some() {
        help.push_str("\npositionals\n");
        let names = usage_positional_names(spec.usage);
        for (index, kind) in spec.positionals.iter().enumerate() {
            match names.as_ref().and_then(|names| names.get(index)) {
                Some(name) => {
                    let _ = writeln!(help, "  <{name}>");
                }
                None => {
                    let _ = writeln!(help, "  <{}>", help_value(*kind));
                }
            }
        }
        if let Some(kind) = spec.variadic {
            let _ = writeln!(help, "  <{}>...", help_value(kind));
        }
    }
    Ok(help)
}

fn usage_positional_names(usage: &str) -> Option<Vec<&str>> {
    let mut depth = 0usize;
    let mut names = Vec::new();
    for token in usage.split_whitespace() {
        let opens = token.matches('[').count();
        let closes = token.matches(']').count();
        if depth == 0 && opens == 0 && !token.starts_with('-') && !token.ends_with("...") {
            names.push(token);
        }
        depth = (depth + opens).saturating_sub(closes);
    }
    (!names.is_empty()).then_some(names)
}

#[cfg(all(test, not(target_os = "ios")))]
fn prepared_command_invocations(command: &PreparedCommand) -> Option<Cow<'_, [CommandInvocation]>> {
    if command.result != zz_protocol::PreparedCommandResult::Ready {
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

#[cfg(all(test, not(target_os = "ios")))]
fn prepared_command_reads_stdin(command: &PreparedCommand) -> Option<CommandStdinSink> {
    prepared_command_invocations(command)?
        .iter()
        .find_map(|invocation| {
            let canonical_name = command
                .canonical_name
                .as_deref()
                .unwrap_or_else(|| canonical_command(&invocation.name));
            zz_daemon::command_stdin_sink(canonical_name, &invocation.args)
        })
}

#[cfg(all(test, not(target_os = "ios")))]
fn append_prepared_command_stdin_payload(
    command: &mut PreparedCommand,
    payload: impl Into<RawText>,
) {
    if let Some(canonical_name) = command.canonical_name.clone() {
        append_stdin_payload(&canonical_name, &mut command.invocation.args, payload);
        return;
    }
    command.invocation.set_stdin(payload);
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

#[cfg(all(test, not(target_os = "ios")))]
fn command_reads_stdin(invocation: &CommandInvocation) -> Option<CommandStdinSink> {
    zz_daemon::command_stdin_sink(canonical_command(&invocation.name), &invocation.args)
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
        return exit_code_for(CliFailure::Runtime);
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
            return exit_code_for(CliFailure::Runtime);
        }
    };
    let shell = match client.execute(CommandInvocation::new(
        "show-options",
        ["-gqv", "default-shell"],
    )) {
        Ok(shell) => shell.trim_end_matches('\n').to_owned(),
        Err(error) => {
            eprintln!("{}", command_error_message(&error));
            return exit_code_for(CliFailure::Daemon(&error));
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
        .status_unmasked()
    {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(exit_code_for(CliFailure::Runtime), ExitCode::from),
        Err(error) => {
            eprintln!("zz: could not run {shell}: {error}");
            exit_code_for(CliFailure::Runtime)
        }
    }
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
                return Err(NativeAttachArgumentError::NativeUsage);
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
fn print_native_attach_argument_error(error: NativeAttachArgumentError) -> ExitCode {
    let status = match &error {
        NativeAttachArgumentError::Usage => exit_code_for(CliFailure::TmuxUsage),
        NativeAttachArgumentError::NativeUsage => exit_code_for(CliFailure::Usage),
        NativeAttachArgumentError::Command(error) => exit_code_for(CliFailure::Server(error)),
    };
    match error {
        NativeAttachArgumentError::Usage | NativeAttachArgumentError::NativeUsage => {
            eprintln!("{NATIVE_ATTACH_USAGE}");
        }
        NativeAttachArgumentError::Command(error) => {
            eprintln!("{}", command_error_message(&DaemonError::Server(error)));
        }
    }
    status
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

/// Whether the daemon says a `file_write` on `-` claimed this client's stdout
/// for the run that produced these bytes. The pin's claim is a property of the
/// writer: `show-buffer` on a buffer whose own last byte is a newline is still
/// a raw write, and reading the claim off the bytes turned that into a print.
#[cfg(not(target_os = "ios"))]
const fn raw_command_output(claim: StdoutClaim) -> bool {
    matches!(claim, StdoutClaim::Raw)
}

#[cfg(not(target_os = "ios"))]
#[derive(Default)]
struct CommandOutputWriter {
    raw_owner: Option<bool>,
}

#[cfg(not(target_os = "ios"))]
impl CommandOutputWriter {
    #[cfg(unix)]
    const OWNED_STREAM_ERROR: i32 = libc::EBADF;
    #[cfg(windows)]
    const OWNED_STREAM_ERROR: i32 = 6;

    fn write(
        &mut self,
        output: &RawText,
        raw: bool,
        stdout: &mut impl io::Write,
    ) -> io::Result<()> {
        let output = output.as_bytes();
        if output.is_empty() {
            return Ok(());
        }
        if raw && self.raw_owner.is_some() {
            return Err(io::Error::from_raw_os_error(Self::OWNED_STREAM_ERROR));
        }
        if self.raw_owner == Some(true) {
            return Ok(());
        }
        self.raw_owner = Some(raw);
        stdout.write_all(output)?;
        if !raw && !output.ends_with(b"\n") {
            stdout.write_all(b"\n")?;
        }
        stdout.flush()
    }

    fn print(&mut self, output: &RawText, raw: bool) -> u8 {
        if let Err(error) = self.write(output, raw, &mut io::stdout().lock())
            && raw
            && error.raw_os_error() == Some(Self::OWNED_STREAM_ERROR)
        {
            eprintln!("{}: -", os_error_text(&error));
            return 1;
        }
        0
    }
}

#[cfg(not(target_os = "ios"))]
thread_local! {
    static CHAIN_OUTPUT_WRITER: RefCell<CommandOutputWriter> =
        const { RefCell::new(CommandOutputWriter { raw_owner: None }) };
}

#[cfg(not(target_os = "ios"))]
fn print_chain_output(output: &RawText, raw: bool) -> u8 {
    CHAIN_OUTPUT_WRITER.with(|writer| writer.borrow_mut().print(output, raw))
}

/// The daemon released a `cmdq_print` line while the command was still running.
#[cfg(not(target_os = "ios"))]
fn print_released_command_output(output: &RawText) {
    let _ = print_chain_output(output, false);
}

#[cfg(not(target_os = "ios"))]
fn print_command_output(output: &RawText) {
    CommandOutputWriter::default().print(output, false);
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
#[derive(Clone, Copy)]
enum CliFailure<'a> {
    Usage,
    TmuxUsage,
    Runtime,
    Server(&'a ServerError),
    Daemon(&'a DaemonError),
}

#[cfg(not(target_os = "ios"))]
fn exit_code_for(error: CliFailure<'_>) -> ExitCode {
    ExitCode::from(match error {
        CliFailure::Usage => 2,
        CliFailure::Runtime | CliFailure::TmuxUsage => 1,
        CliFailure::Server(error) => error.exit_code(),
        CliFailure::Daemon(error) => match error {
            DaemonError::Server(error) => error.exit_code(),
            DaemonError::CommandExit { exit_code, .. }
            | DaemonError::ReportedCommandExit { exit_code, .. } => *exit_code,
            _ => 1,
        },
    })
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
fn run_kill_server(path: &Path, args: impl IntoIterator<Item = RawText>) -> ExitCode {
    let invocation = CommandInvocation::new("kill-server", args);
    let failure = match CommandClient::connect(path) {
        Ok(mut client) => match client.execute_prepared_streams(invocation) {
            Ok(outcome) => {
                print_command_output(&outcome.stdout);
                print_command_error(&outcome.stderr);
                return ExitCode::from(outcome.exit_code);
            }
            Err(error) => error,
        },
        Err(error) if daemon_is_missing(&error) => {
            eprintln!("{}", format_local_command_error(path, error));
            return exit_code_for(CliFailure::Runtime);
        }
        Err(error) => error,
    };

    if let DaemonError::Server(error) = &failure {
        eprintln!("{}", server_error_message(error));
        return exit_code_for(CliFailure::Server(error));
    }
    recover_kill_server_failure(path, &failure)
}

#[cfg(not(target_os = "ios"))]
fn run_host_kill_server(host: &str, args: impl IntoIterator<Item = RawText>) -> ExitCode {
    let mut client = match connect_host_command_client(host) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("zz: {error}");
            return exit_code_for(CliFailure::Runtime);
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
            exit_code_for(CliFailure::Runtime)
        }
        Err(error) => {
            eprintln!("{}", command_error_message(&error));
            exit_code_for(CliFailure::Daemon(&error))
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
            exit_code_for(CliFailure::Runtime)
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
fn tmux_import_hint(interactive: bool, mux_exists: bool, donor: Option<&Path>) -> Option<String> {
    if !interactive || mux_exists {
        return None;
    }
    donor.map(|path| {
        format!(
            "zz does not read {}; run `zz import-tmux-config` to import it.",
            path.display()
        )
    })
}

#[cfg(not(target_os = "ios"))]
fn spawn_daemon(
    path: &Path,
    color_scheme: Option<TerminalColorScheme>,
    mux_config_files: &[PathBuf],
) -> Result<u64, DaemonError> {
    let server_id = next_spawn_server_id();
    let executable = daemon_executable()?;
    let mut command = Command::new(&executable);
    command
        .env_remove("ZZ_TMUX_EXECUTABLE")
        .env_remove(APP_STARTUP_DIRECTORY_ENV);
    command.arg(diagnostics::SOCKET_ARGUMENT).arg(path);
    for config in mux_config_files {
        command.arg("-f").arg(config);
    }
    command
        .arg("daemon")
        .arg(DAEMON_BOOTSTRAP_SERVER_ID_ARGUMENT)
        .arg(server_id.to_string());
    #[cfg(unix)]
    let ready = readiness::Pipe::new()?;
    #[cfg(unix)]
    command
        .arg(DAEMON_BOOTSTRAP_READY_FD_ARGUMENT)
        .arg(ready.child_fd().to_string());
    if let Some(color_scheme) = color_scheme {
        command.env("ZZ_COLOR_SCHEME", color_scheme.as_str());
    }
    diagnostics::configure_spawned_process(&mut command);
    #[cfg(unix)]
    detach_daemon_session(&mut command, ready.child_fd());
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
    if let Some(hint) = tmux_import_hint(
        std::io::stderr().is_terminal(),
        zz_daemon::mux_config_write_path().is_some_and(|path| path.exists()),
        zz_daemon::discover_tmux_config().as_deref(),
    ) {
        eprintln!("{hint}");
    }
    reap_when_exited(command.spawn_unmasked()?);
    log::debug!(
        target: "zz::diagnostics::process",
        "spawned daemon path={} child_verbose_flag_applied={}",
        path.display(),
        diagnostics::enabled(),
    );
    #[cfg(unix)]
    ready.wait(DAEMON_READY_DEADLINE);
    Ok(server_id)
}

#[cfg(not(target_os = "ios"))]
fn reap_when_exited(mut child: std::process::Child) {
    let _ = thread::Builder::new()
        .name("zz-daemon-reaper".to_owned())
        .spawn(move || child.wait());
}

#[cfg(all(unix, not(target_os = "ios")))]
mod readiness {
    use std::{
        io,
        os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd},
        time::{Duration, Instant},
    };

    pub(super) struct Pipe {
        reader: OwnedFd,
        child: Option<OwnedFd>,
    }

    #[allow(
        unsafe_code,
        reason = "pipe, fcntl and poll on descriptors this function owns"
    )]
    impl Pipe {
        pub(super) fn new() -> io::Result<Self> {
            let mut fds = [0; 2];
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let (reader, child) =
                unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
            for fd in [&reader, &child] {
                if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(Self {
                reader,
                child: Some(child),
            })
        }

        pub(super) fn child_fd(&self) -> RawFd {
            self.child
                .as_ref()
                .map_or(-1, std::os::fd::AsRawFd::as_raw_fd)
        }

        pub(super) fn wait(mut self, limit: Duration) {
            drop(self.child.take());
            let deadline = Instant::now() + limit;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return;
                }
                let mut poll = libc::pollfd {
                    fd: self.reader.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let timeout = i32::try_from(remaining.as_millis())
                    .unwrap_or(i32::MAX)
                    .max(1);
                match unsafe { libc::poll(&raw mut poll, 1, timeout) } {
                    0 => return,
                    count if count > 0 => {
                        let mut byte = 0_u8;
                        let _ = unsafe {
                            libc::read(self.reader.as_raw_fd(), (&raw mut byte).cast(), 1)
                        };
                        return;
                    }
                    _ if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => {}
                    _ => return,
                }
            }
        }
    }
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
fn detach_daemon_session(command: &mut Command, ready_fd: std::os::fd::RawFd) {
    use std::os::unix::process::CommandExt as _;

    let mut inherited = InheritedDescriptors::new();
    // SAFETY: the hook only calls setsid, fcntl, and close_range or
    // proc_pidinfo into memory reserved before the fork, which are
    // async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            let _ = rustix::process::setsid();
            inherited.mark_cloexec();
            if ready_fd >= 0 {
                libc::fcntl(ready_fd, libc::F_SETFD, 0);
            }
            Ok(())
        });
    }
}

#[cfg(target_os = "macos")]
struct InheritedDescriptors(Vec<libc::proc_fdinfo>);

#[cfg(all(unix, not(target_os = "macos")))]
struct InheritedDescriptors(libc::c_int);

#[cfg(unix)]
fn descriptor_limit() -> usize {
    let open = rustix::process::getrlimit(rustix::process::Resource::Nofile)
        .current
        .unwrap_or(4096)
        .clamp(256, 65_536);
    usize::try_from(open).unwrap_or(4096)
}

#[cfg(target_os = "macos")]
impl InheritedDescriptors {
    fn new() -> Self {
        Self(Vec::with_capacity(descriptor_limit()))
    }

    #[allow(
        unsafe_code,
        reason = "proc_pidinfo writes into capacity reserved before the fork"
    )]
    fn mark_cloexec(&mut self) {
        let capacity = self.0.capacity();
        let bytes =
            i32::try_from(capacity * std::mem::size_of::<libc::proc_fdinfo>()).unwrap_or(i32::MAX);
        let written = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDLISTFDS,
                0,
                self.0.as_mut_ptr().cast(),
                bytes,
            )
        };
        let Ok(written) = usize::try_from(written) else {
            return;
        };
        let count = (written / std::mem::size_of::<libc::proc_fdinfo>()).min(capacity);
        for index in 0..count {
            let descriptor = unsafe { (*self.0.as_ptr().add(index)).proc_fd };
            if descriptor > 2 {
                unsafe {
                    libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC);
                }
            }
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl InheritedDescriptors {
    fn new() -> Self {
        Self(libc::c_int::try_from(descriptor_limit()).unwrap_or(4096))
    }

    #[allow(
        unsafe_code,
        reason = "close_range and fcntl are async-signal-safe syscalls"
    )]
    fn mark_cloexec(&mut self) {
        #[cfg(target_os = "linux")]
        if unsafe {
            libc::syscall(
                libc::SYS_close_range,
                3_u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            )
        } == 0
        {
            return;
        }
        for descriptor in 3..self.0 {
            unsafe {
                libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }
    }
}

#[cfg(not(target_os = "ios"))]
pub fn connect_or_spawn_daemon<T>(
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
pub fn connect_or_spawn_daemon_with_provenance<T>(
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
            error if daemon_is_spawnable(&error) => {}
            error => return Err(error),
        },
    }

    let spawned_server_id = spawn_daemon(path, color_scheme, mux_config_files)?;
    let client = connect_spawned(path, || connect(true))?;
    let provenance =
        (server_hello(&client).server_id == spawned_server_id).then_some(spawned_server_id);
    Ok((client, provenance))
}

#[cfg(not(target_os = "ios"))]
fn connect_spawned<T>(
    path: &Path,
    connect: impl Fn() -> Result<T, DaemonError>,
) -> Result<T, DaemonError> {
    let deadline = Instant::now() + DAEMON_READY_DEADLINE;
    let mut backoff = [500, 1_000, 2_000].into_iter();
    loop {
        match connect() {
            Ok(client) => return Ok(client),
            Err(error) if Instant::now() >= deadline || !daemon_is_spawnable(&error) => {
                return Err(classify_local_connect_error(path, error));
            }
            Err(_) => thread::sleep(Duration::from_micros(backoff.next().unwrap_or(5_000))),
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn connect_command_client(
    path: &Path,
    mux_config_files: &[PathBuf],
    start_server: bool,
) -> Result<CommandClient, DaemonError> {
    match CommandClient::connect(path) {
        Ok(client) => Ok(client),
        Err(error) if start_server && daemon_is_spawnable(&error) => {
            spawn_and_connect_command_client(path, mux_config_files).map(|(client, _)| client)
        }
        Err(error) if start_server => Err(classify_local_connect_error(path, error)),
        Err(error) => Err(error),
    }
}

#[cfg(not(target_os = "ios"))]
fn spawn_and_connect_command_client(
    path: &Path,
    mux_config_files: &[PathBuf],
) -> Result<(CommandClient, u64), DaemonError> {
    let spawned_server_id = spawn_daemon(path, None, mux_config_files)?;
    let client = connect_spawned(path, || CommandClient::connect(path))?;
    Ok((client, spawned_server_id))
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
#[cfg(not(target_os = "ios"))]
fn bare_command_line_opens_application(
    origin: CommandLineOrigin,
    launched_by_launch_services: bool,
) -> bool {
    origin == CommandLineOrigin::Application && (cfg!(windows) || launched_by_launch_services)
}

#[cfg(target_os = "macos")]
fn launched_by_launch_services() -> bool {
    std::os::unix::process::parent_id() == 1
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn launched_by_launch_services() -> bool {
    false
}

fn is_launcher_default_client_command(commands: &[CommandInvocation]) -> bool {
    matches!(commands, [command] if command.name == "new-session" && command.args.is_empty())
}

#[cfg(not(target_os = "ios"))]
fn stored_default_client_command(
    socket_path: &Path,
    mux_config_files: &[PathBuf],
    no_start_server: bool,
) -> Option<String> {
    let mut client =
        connect_command_client(socket_path, mux_config_files, !no_start_server).ok()?;
    let output = client
        .execute(CommandInvocation::new(
            "show-options",
            ["-sqv", "default-client-command"],
        ))
        .ok()?;
    Some(output.trim_end_matches('\n').to_owned())
}

#[cfg(not(target_os = "ios"))]
fn connect_host_command_client(name: &str) -> Result<CommandClient, String> {
    let endpoint = configured_host_endpoint(name)?;
    CommandClient::connect_endpoint(&endpoint).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "ios"))]
fn configured_host_endpoint(name: &str) -> Result<Endpoint, String> {
    let (hosts, _) = zz_daemon::configured_fleet_hosts()
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
pub fn connect_interactive_client_with_config(
    path: &Path,
    color_scheme: TerminalColorScheme,
    mux_config_files: &[PathBuf],
    capabilities: &[&str],
    attach: Option<&zz_protocol::AttachOperation>,
) -> Result<InteractiveClient, DaemonError> {
    connect_or_spawn_daemon(
        path,
        Some(color_scheme),
        mux_config_files,
        |_| {
            InteractiveClient::connect_endpoint_with_prompts_and_attach(
                &Endpoint::Local(path.to_path_buf()),
                Some(color_scheme),
                None,
                capabilities,
                attach.cloned(),
                false,
            )
        },
        InteractiveClient::server_hello,
    )
}

#[cfg(not(target_os = "ios"))]
pub fn connect_interactive_client_with_config_and_terminal(
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
pub fn connect_terminal_surface_client_with_config(
    path: &Path,
    color_scheme: TerminalColorScheme,
    mux_config_files: &[PathBuf],
    _client_has_terminal: bool,
    attach: Option<&zz_protocol::AttachOperation>,
) -> Result<InteractiveClient, DaemonError> {
    connect_or_spawn_daemon(
        path,
        Some(color_scheme),
        mux_config_files,
        |_| {
            InteractiveClient::connect_endpoint_with_prompts_and_attach(
                &Endpoint::Local(path.to_path_buf()),
                None,
                None,
                &[
                    zz_daemon::CLIENT_EXITS_ON_DETACH_CAPABILITY,
                    zz_protocol::TTY_INPUT_CAPABILITY,
                ],
                attach.cloned(),
                true,
            )
        },
        InteractiveClient::server_hello,
    )
}

fn startup_directory() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .filter(|directory| directory.is_dir())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_dir())
        })
}

#[cfg(target_os = "macos")]
fn startup_directory_environment(directory: &Path) -> std::ffi::OsString {
    let mut environment = std::ffi::OsString::from(APP_STARTUP_DIRECTORY_ENV);
    environment.push("=");
    environment.push(directory);
    environment
}

#[cfg(target_os = "macos")]
#[must_use]
pub fn launch_application(socket_path: &Path) -> ExitCode {
    let launcher = match std::env::current_exe().and_then(|executable| executable.canonicalize()) {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("zz: could not resolve the launcher path: {error}");
            return ExitCode::FAILURE;
        }
    };

    let Some(bundle) = launcher
        .ancestors()
        .find(|path| path.extension() == Some(std::ffi::OsStr::new("app")))
    else {
        eprintln!(
            "zz app: the desktop app is not installed beside {}; this is a headless install",
            launcher.display(),
        );
        return ExitCode::FAILURE;
    };
    let mut command = Command::new("/usr/bin/open");
    command.args(["--env", "TMUX=", "--env", "TMUX_PANE="]);
    if let Some(directory) = startup_directory() {
        command.args([
            std::ffi::OsString::from("--env"),
            startup_directory_environment(&directory),
        ]);
    }
    if let Some(socket) = std::env::var_os("ZZ_SOCKET").filter(|socket| !socket.is_empty()) {
        let mut socket_environment = std::ffi::OsString::from("ZZ_SOCKET=");
        socket_environment.push(socket);
        command.args([std::ffi::OsString::from("--env"), socket_environment]);
    }
    command
        .arg(bundle)
        .args(["--args", "app", diagnostics::SOCKET_ARGUMENT])
        .arg(socket_path)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE");
    diagnostics::configure_spawned_process(&mut command);
    match command.status_unmasked() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("zz: could not open {}: {status}", bundle.display());
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("zz: could not open {}: {error}", bundle.display());
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn launch_application(socket_path: &Path) -> ExitCode {
    let launcher = match std::env::current_exe().and_then(|executable| executable.canonicalize()) {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("zz: could not resolve the launcher path: {error}");
            return ExitCode::FAILURE;
        }
    };

    let executable = launcher.with_file_name(if cfg!(windows) { "zz.exe" } else { "zz" });
    if !executable.is_file() {
        eprintln!(
            "zz app: the desktop app is not installed beside {}; this is a headless install",
            launcher.display(),
        );
        return ExitCode::FAILURE;
    }

    let mut command = Command::new(executable);
    command
        .args(["app", diagnostics::SOCKET_ARGUMENT])
        .arg(socket_path)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(directory) = startup_directory() {
        command.env(APP_STARTUP_DIRECTORY_ENV, directory);
    }
    diagnostics::configure_spawned_process(&mut command);
    match command.spawn_unmasked() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zz: could not open the application: {error}");
            ExitCode::FAILURE
        }
    }
}

pub fn daemon_executable() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    match executable.canonicalize() {
        Ok(executable) => Ok(daemon_executable_from(&executable)),
        #[cfg(target_os = "linux")]
        Err(_) => Ok(PathBuf::from("/proc/self/exe")),
        #[cfg(not(target_os = "linux"))]
        Err(error) => Err(error),
    }
}

#[must_use]
fn daemon_executable_from(executable: &Path) -> PathBuf {
    let name = if cfg!(windows) { "cli.exe" } else { "cli" };
    if executable.file_name() != Some(std::ffi::OsStr::new(name)) {
        let cli = executable.with_file_name(name);
        if cli.is_file() {
            return cli;
        }
    }
    executable.to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::{Path, PathBuf},
    };

    use zz_protocol::RawText;

    use super::{
        ApplicationArgumentError, CommandOutputWriter, DaemonBootstrapArgumentError,
        DaemonBootstrapArguments, NativeAttachArgumentError, TMUX_USAGE, TMUX_VERSION_OUTPUT,
        append_prepared_command_stdin_payload, append_stdin_payload, application_arguments,
        application_working_directory, attach_prefix_uses_tui, command_chain_uses_tui,
        command_error_message, command_reads_stdin, daemon_is_missing, daemon_transport_failure,
        implicit_tmux_endpoint_conflict, native_attach_command, new_session_uses_tui,
        parse_daemon_bootstrap_arguments, parse_native_attach_arguments,
        prepared_command_reads_stdin, protocol_version_output, run_command_mode,
        split_command_chain, tmux_command_starts_server,
        validated_bootstrap_client_working_directory,
    };
    #[cfg(unix)]
    use super::{tmux_label_socket_path, tmux_socket_root};
    use zz_daemon::DaemonError;
    use zz_mux::{CommandAliasResolution, ExecutionContext, MuxEngine};
    use zz_protocol::{
        CommandInvocation, ExecResumeKind, PreparedCommand, PreparedCommandResult, ServerError,
    };

    fn prepared_attach_uses_tui(typed: &str, prepared: &PreparedCommand) -> bool {
        zz_daemon::exec_resume_kind(
            &[CommandInvocation::new(typed, [] as [&str; 0])],
            std::slice::from_ref(prepared),
        ) == Some(ExecResumeKind::NewSession)
    }

    fn prepared_command_chain_uses_tui(
        typed: &[CommandInvocation],
        prepared: &[PreparedCommand],
    ) -> bool {
        zz_daemon::exec_resume_kind(typed, prepared) == Some(ExecResumeKind::NewSession)
    }

    fn prepared_native_attach(typed: &str, prepared: &PreparedCommand) -> bool {
        zz_daemon::exec_resume_kind(
            &[CommandInvocation::new(typed, [] as [&str; 0])],
            std::slice::from_ref(prepared),
        ) == Some(ExecResumeKind::NativeAttach)
    }

    #[test]
    fn cli_exit_contract_preserves_explicit_status_and_classifies_errors() {
        use super::{CliFailure, exit_code_for};
        use std::process::ExitCode;

        for code in [0, 1, 2, 3, 124, 125, 255] {
            let error = DaemonError::CommandExit {
                output: RawText::default(),
                exit_code: code,
            };
            assert_eq!(
                exit_code_for(CliFailure::Daemon(&error)),
                ExitCode::from(code)
            );
        }
        let usage = ServerError::CommandParse("command list-panes: invalid flag --".to_owned());
        assert_eq!(exit_code_for(CliFailure::Server(&usage)), ExitCode::from(1));
        let unsupported = ServerError::UnsupportedCommand("unknown command: bogus".to_owned());
        assert_eq!(
            exit_code_for(CliFailure::Server(&unsupported)),
            ExitCode::from(1)
        );
        let native = ServerError::NativeCommandParse("native usage".to_owned());
        assert_eq!(
            exit_code_for(CliFailure::Server(&native)),
            ExitCode::from(2)
        );
        let runtime = ServerError::InvalidCommand("duplicate session: dup".to_owned());
        assert_eq!(
            exit_code_for(CliFailure::Server(&runtime)),
            ExitCode::from(1)
        );
        let missing = ServerError::InvalidTarget("can't find window: 9".to_owned());
        assert_eq!(
            exit_code_for(CliFailure::Server(&missing)),
            ExitCode::from(1)
        );
        let failed = DaemonError::CommandFailed {
            output: "partial output".into(),
            error: Box::new(DaemonError::Server(usage)),
        };
        assert_eq!(
            exit_code_for(CliFailure::Daemon(&failed)),
            ExitCode::from(1)
        );
        let disconnected = DaemonError::Io(io::Error::from(io::ErrorKind::BrokenPipe));
        assert_eq!(
            exit_code_for(CliFailure::Daemon(&disconnected)),
            ExitCode::from(1)
        );
    }

    #[test]
    fn tmux_import_hint_only_when_cli_spawns_without_mux_config() {
        let donor = Path::new("/home/u/.tmux.conf");
        assert!(
            super::tmux_import_hint(true, false, Some(donor))
                .unwrap()
                .contains("zz import-tmux-config")
        );
        assert!(super::tmux_import_hint(false, false, Some(donor)).is_none());
        assert!(super::tmux_import_hint(true, true, Some(donor)).is_none());
        assert!(super::tmux_import_hint(true, false, None).is_none());
    }

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
                ready_fd: None,
                client_working_directory: None,
            }
        );
        assert_eq!(
            parse_daemon_bootstrap_arguments(&[
                "--bootstrap-server-id".into(),
                "42".into(),
                "--bootstrap-ready-fd".into(),
                "9".into(),
                "--bootstrap-client-cwd".into(),
                path_string.clone().into(),
            ])
            .unwrap(),
            DaemonBootstrapArguments {
                server_id: Some(42),
                ready_fd: Some(9),
                client_working_directory: Some(path.clone()),
            }
        );
        for ready_fd in [
            vec!["--bootstrap-ready-fd"],
            vec!["--bootstrap-ready-fd", "1"],
        ] {
            let mut arguments = vec![RawText::from("--bootstrap-server-id"), "42".into()];
            arguments.extend(ready_fd.into_iter().map(RawText::from));
            assert_eq!(
                parse_daemon_bootstrap_arguments(&arguments),
                Err(DaemonBootstrapArgumentError::ReadyFd)
            );
        }
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
                ready_fd: None,
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
    fn agent_send_progress_usage_errors_exit_two_before_connecting() {
        for args in [
            vec!["agent-send", "--progress", "-t", "%1", "hi"],
            vec!["agent-send", "--progress", "--wait", "hi"],
        ] {
            let args = args.into_iter().map(RawText::from).collect::<Vec<_>>();
            assert_eq!(
                run_command_mode(
                    &args,
                    std::path::Path::new("/tmp/zz-progress-missing.sock"),
                    super::SocketSelectionSource::Default,
                    None,
                    &[],
                    true,
                    None,
                    false,
                    &super::StartupOptions {
                        origin: super::CommandLineOrigin::Launcher,
                        browser_provider: None
                    },
                    false,
                ),
                Some(std::process::ExitCode::from(2))
            );
        }
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
                &super::StartupOptions {
                    origin: super::CommandLineOrigin::Launcher,
                    browser_provider: None,
                },
                true,
            )
            .is_none()
        );
    }

    #[test]
    fn a_bare_command_line_is_a_client_unless_launch_services_or_windows_opened_it() {
        use super::{CommandLineOrigin, bare_command_line_opens_application};

        assert!(!bare_command_line_opens_application(
            CommandLineOrigin::Launcher,
            true
        ));
        assert!(!bare_command_line_opens_application(
            CommandLineOrigin::Launcher,
            false
        ));
        assert_eq!(
            bare_command_line_opens_application(CommandLineOrigin::Application, false),
            cfg!(windows)
        );
        assert!(bare_command_line_opens_application(
            CommandLineOrigin::Application,
            true
        ));
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
            assert!(command_reads_stdin(&CommandInvocation::new(command, ["--submit"])).is_some());
            assert!(command_reads_stdin(&CommandInvocation::new(command, ["text"])).is_none());
        }
        for command in ["send-text", "send-t"] {
            assert!(command_reads_stdin(&CommandInvocation::new(command, ["-t", "%1"])).is_some());
            assert!(command_reads_stdin(&CommandInvocation::new(command, ["hello"])).is_none());
        }
        assert!(
            command_reads_stdin(&CommandInvocation::new("list-sessions", [] as [&str; 0]))
                .is_none()
        );
    }

    #[test]
    fn agent_permission_commands_pass_preflight_and_route_stdin() {
        let send = CommandInvocation::new("agent-send", ["--wait", "--on-block", "fail"]);
        let inspect = CommandInvocation::new("inspect", ["-t", "%1", "--json"]);
        let respond = CommandInvocation::new("agent-respond", ["-t", "%1", "--allow"]);
        assert!(command_reads_stdin(&send).is_some());
        assert!(command_reads_stdin(&inspect).is_none());
        assert!(command_reads_stdin(&respond).is_none());
        zz_mux::validate_static_command_chain(&[send, inspect, respond]).expect("native preflight");
        assert!(
            command_reads_stdin(&CommandInvocation::new(
                "agent-send",
                ["--wait", "--on-block=fail", "hello"]
            ))
            .is_none()
        );
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
        assert!(prepared_command_reads_stdin(&live_send).is_some());
        let shadowed_send = prepared("agent-send", "display-message", true, &["-p", "shadow"]);
        assert!(prepared_command_reads_stdin(&shadowed_send).is_none());

        let plain_attach = prepared("attach", "attach-session", false, &[]);
        assert!(prepared_native_attach("attach", &plain_attach));
        assert!(!prepared_attach_uses_tui("attach", &plain_attach));
    }

    #[test]
    fn prepared_cli_routing_uses_the_first_stream_sink_in_alias_groups() {
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
            assert!(prepared_command_reads_stdin(&send_text).is_some());
            append_prepared_command_stdin_payload(&mut send_text, payload.to_owned());
            let commands = MuxEngine::command_alias_group_commands(&send_text.invocation)
                .expect("parse prepared alias group")
                .expect("prepared command is an alias group");
            assert_eq!(commands[0].name, "display-message");
            assert_eq!(commands[1].name, "send-text");
            assert_eq!(commands[1].args, ["-t", "%1"]);
            assert_eq!(send_text.invocation.stdin(), Some(&RawText::from(payload)));
        }

        let mut bounded = prepared_alias("display-message -p before ; send-text -t %1 --", &[]);
        assert!(prepared_command_reads_stdin(&bounded).is_some());
        append_prepared_command_stdin_payload(&mut bounded, "piped".to_owned());
        let commands = MuxEngine::command_alias_group_commands(&bounded.invocation)
            .expect("parse prepared alias group")
            .expect("prepared command is an alias group");
        assert_eq!(commands[1].args, ["-t", "%1", "--"]);
        assert_eq!(bounded.invocation.stdin(), Some(&RawText::from("piped")));

        for (body, payload, expected) in [
            (
                "display-message -p before ; send-text -t --",
                "--no-enter",
                vec!["-t", "--"],
            ),
            (
                "display-message -p before ; agent-send --context --",
                "--submit",
                vec!["--context", "--"],
            ),
        ] {
            let mut prepared = prepared_alias(body, &[]);
            assert!(prepared_command_reads_stdin(&prepared).is_some());
            append_prepared_command_stdin_payload(&mut prepared, payload.to_owned());
            let commands = MuxEngine::command_alias_group_commands(&prepared.invocation)
                .expect("parse prepared alias group")
                .expect("prepared command is an alias group");
            assert_eq!(commands[1].args, expected);
            assert_eq!(prepared.invocation.stdin(), Some(&RawText::from(payload)));
        }

        let mut binary = prepared_alias("load-buffer -b alias - ; source-file -", &[]);
        assert_eq!(
            prepared_command_reads_stdin(&binary),
            Some(super::CommandStdinSink::Argument { binary: true })
        );
        let body = binary.invocation.args.clone();
        let payload = RawText::from_bytes(b"a\xff\0z\n".to_vec());
        append_prepared_command_stdin_payload(&mut binary, payload.clone());
        assert_eq!(binary.invocation.args, body);
        assert_eq!(binary.invocation.stdin(), Some(&payload));

        let agent_send = prepared_alias("display-message -p before ; agent-send --submit", &[]);
        assert!(prepared_command_reads_stdin(&agent_send).is_some());
        let nonfinal_agent_send =
            prepared_alias("agent-send --submit ; display-message -p after", &[]);
        assert!(prepared_command_reads_stdin(&nonfinal_agent_send).is_some());

        let empty = prepared_alias("", &["agent-send", "--submit"]);
        assert!(!prepared_attach_uses_tui("route", &empty));
        assert!(!prepared_command_chain_uses_tui(
            &[CommandInvocation::new("route", [] as [&str; 0])],
            std::slice::from_ref(&empty)
        ));
        assert!(prepared_command_reads_stdin(&empty).is_none());
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
                "-fread-only,no-detach-on-destroy",
                "work",
            ]
            .map(RawText::from),
        )
        .unwrap();
        assert_eq!(
            flags.client_flags.as_deref(),
            Some("read-only,no-detach-on-destroy")
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
    fn command_output_file_stream_owns_stdout_after_its_first_write() {
        let mut writer = CommandOutputWriter::default();
        let mut stdout = Vec::new();
        writer.write(&"hello".into(), true, &mut stdout).unwrap();
        writer.write(&"\n".into(), false, &mut stdout).unwrap();
        writer.write(&"AFTER\n".into(), false, &mut stdout).unwrap();
        assert_eq!(stdout, b"hello");
        assert_eq!(
            writer
                .write(&"again".into(), true, &mut stdout)
                .unwrap_err()
                .raw_os_error(),
            Some(CommandOutputWriter::OWNED_STREAM_ERROR)
        );
        assert_eq!(stdout, b"hello");
    }

    #[test]
    fn command_output_print_stream_survives_a_later_file_collision() {
        let mut writer = CommandOutputWriter::default();
        let mut stdout = Vec::new();
        writer.write(&"\n".into(), false, &mut stdout).unwrap();
        assert_eq!(
            writer
                .write(&"hello".into(), true, &mut stdout)
                .unwrap_err()
                .raw_os_error(),
            Some(CommandOutputWriter::OWNED_STREAM_ERROR)
        );
        writer.write(&"AFTER\n".into(), false, &mut stdout).unwrap();
        assert_eq!(stdout, b"\nAFTER\n");
    }

    #[test]
    fn command_output_empty_results_do_not_claim_stdout() {
        for raw in [false, true] {
            let mut writer = CommandOutputWriter::default();
            let mut stdout = Vec::new();
            writer.write(&RawText::default(), raw, &mut stdout).unwrap();
            writer.write(&"hello".into(), true, &mut stdout).unwrap();
            assert_eq!(stdout, b"hello");
        }
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

    #[test]
    fn daemon_executable_selects_the_installed_cli_and_keeps_a_lone_cli() {
        let directory = tempfile::tempdir().expect("temporary installation");
        let application = directory
            .path()
            .join(if cfg!(windows) { "zz.exe" } else { "zz" });
        let cli = directory
            .path()
            .join(if cfg!(windows) { "cli.exe" } else { "cli" });
        std::fs::write(&application, b"").expect("write application");
        assert_eq!(super::daemon_executable_from(&application), application);
        std::fs::create_dir(&cli).expect("create non-file CLI path");
        assert_eq!(super::daemon_executable_from(&application), application);
        std::fs::remove_dir(&cli).expect("remove non-file CLI path");
        std::fs::write(&cli, b"").expect("write CLI");
        assert_eq!(super::daemon_executable_from(&application), cli);
        std::fs::remove_file(&application).expect("remove application");
        assert_eq!(super::daemon_executable_from(&cli), cli);
    }

    #[cfg(unix)]
    #[test]
    fn a_spawned_daemon_that_exits_is_reaped_while_the_spawner_lives() {
        use zz_daemon::unmasked::SpawnUnmasked as _;

        let child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn_unmasked()
            .expect("spawn a short-lived child");
        let pid = rustix::process::Pid::from_child(&child);
        super::reap_when_exited(child);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while rustix::process::test_kill_process(pid).is_ok() {
            assert!(
                std::time::Instant::now() < deadline,
                "the exited child is still a zombie"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_launch_preserves_the_callers_working_directory() {
        assert_eq!(
            super::startup_directory_environment(Path::new("/tmp/a project")),
            std::ffi::OsString::from("ZZ_APP_STARTUP_DIRECTORY=/tmp/a project")
        );
    }

    #[cfg(not(windows))]
    #[test]
    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn purge_delay_names_mimallocs_v3_option() {
        if std::env::var_os("MIMALLOC_PURGE_DELAY").is_some() {
            return;
        }
        unsafe extern "C" {
            fn mi_option_get(option: std::ffi::c_int) -> std::ffi::c_long;
        }
        assert_eq!(unsafe { mi_option_get(super::MI_OPTION_PURGE_DELAY) }, 1000);
    }
}
