use super::*;
use serde::Deserialize;
use std::{
    io::ErrorKind,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeConnectOptions {
    endpoint: String,
    #[serde(default)]
    helper_path: PathBuf,
    #[serde(default)]
    start_if_missing: bool,
    #[serde(default)]
    restart_incompatible: bool,
    dark: bool,
    working_directory: Option<PathBuf>,
    mux_config_path: Option<PathBuf>,
}

fn connect(
    options: NativeConnectOptions,
    callback: Option<ZzSshPromptCallback>,
    context: *mut c_void,
) -> Result<*mut ZzClient, ConnectFailure> {
    let endpoint = Endpoint::parse(&options.endpoint)
        .map_err(|error| ConnectFailure::configuration(error.to_string()))?;
    let scheme = if options.dark {
        TerminalColorScheme::Dark
    } else {
        TerminalColorScheme::Light
    };
    let dial = || match &endpoint {
        Endpoint::Local(path) => InteractiveClient::connect_terminal_surface_with_timeout(
            path,
            scheme,
            Duration::from_secs(5),
        )
        .map_err(|error| zz_daemon::classify_local_connect_error(path, error)),
        Endpoint::Ssh(_) => InteractiveClient::connect_terminal_surface_endpoint_with_prompts(
            &endpoint,
            scheme,
            native_interactive_prompts(options.helper_path.clone(), callback, context),
        ),
    };
    let client = match dial() {
        Ok(client) => client,
        Err(error) => {
            let Endpoint::Local(path) = &endpoint else {
                return Err(failure(error));
            };
            if !options.start_if_missing {
                return Err(failure(error));
            }
            let restart = match &error {
                DaemonError::IncompatibleDaemon { .. } if options.restart_incompatible => true,
                DaemonError::Io(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::NotFound | ErrorKind::ConnectionRefused
                    ) =>
                {
                    false
                }
                _ => return Err(failure(error)),
            };
            if !options.helper_path.is_absolute()
                || !options.helper_path.metadata().is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                })
            {
                return Err(ConnectFailure::configuration(
                    "Native daemon helper is unavailable.",
                ));
            }
            if options
                .working_directory
                .as_ref()
                .is_some_and(|path| !path.is_dir())
            {
                return Err(ConnectFailure::configuration(
                    "Native daemon working directory is unavailable.",
                ));
            }
            if restart {
                zz_daemon::terminate_incompatible_daemon(path).map_err(|error| ConnectFailure {
                    kind: ZzConnectFailure::Incompatible,
                    message: error.to_string(),
                })?;
            }
            let mut command = Command::new(&options.helper_path);
            command
                .arg("daemon")
                .arg("--socket")
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .env(
                    "ZZ_COLOR_SCHEME",
                    if options.dark { "dark" } else { "light" },
                );
            if let Some(cwd) = &options.working_directory {
                command.arg("--client-cwd").arg(cwd).current_dir(cwd);
            }
            if let Some(config) = &options.mux_config_path {
                command.arg("--mux-config").arg(config);
            }
            let mut child = command.spawn().map_err(|error| failure(error.into()))?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match dial() {
                    Ok(client) => {
                        thread::spawn(move || {
                            let _ = child.wait();
                        });
                        break client;
                    }
                    Err(error) => {
                        if child
                            .try_wait()
                            .map_err(|error| failure(error.into()))?
                            .is_some()
                        {
                            return Err(failure(error));
                        }
                        if Instant::now() >= deadline {
                            thread::spawn(move || {
                                let _ = child.wait();
                            });
                            return Err(failure(error));
                        }
                    }
                }
                thread::sleep(Duration::from_millis(40));
            }
        }
    };
    start_client(client).map_err(|message| ConnectFailure {
        kind: ZzConnectFailure::Retryable,
        message,
    })
}

fn failure(error: DaemonError) -> ConnectFailure {
    ConnectFailure {
        kind: classify_connect_error(&error),
        message: error.to_string(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_connect_native(
    options_json: *const c_char,
    callback: Option<ZzSshPromptCallback>,
    context: *mut c_void,
    failure: *mut ZzConnectFailure,
    error: *mut c_char,
    error_capacity: usize,
) -> *mut ZzClient {
    unsafe {
        write_c_string("", error, error_capacity);
        if let Some(failure) = failure.as_mut() {
            *failure = ZzConnectFailure::None;
        }
    }
    let result = (|| {
        if options_json.is_null() {
            return Err(ConnectFailure::configuration(
                "Native connection options are required.",
            ));
        }
        let options = unsafe { CStr::from_ptr(options_json) }.to_bytes();
        let options = serde_json::from_slice(options)
            .map_err(|error| ConnectFailure::configuration(error.to_string()))?;
        connect(options, callback, context)
    })();
    match result {
        Ok(client) => client,
        Err(connect_error) => {
            unsafe {
                if let Some(failure) = failure.as_mut() {
                    *failure = connect_error.kind;
                }
                write_c_string(&connect_error.message, error, error_capacity);
            }
            std::ptr::null_mut()
        }
    }
}
