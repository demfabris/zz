use std::{
    ffi::{OsStr, OsString},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use zz_daemon::{DaemonError, Endpoint, InteractiveClient, classify_local_connect_error};
use zz_protocol::{MAX_CLIENT_WORKING_DIRECTORY_BYTES, PROTOCOL_VERSION};
use zz_terminal::TerminalColorScheme;

pub(super) fn connect(
    endpoint: &Endpoint,
    color_scheme: TerminalColorScheme,
) -> Result<InteractiveClient, String> {
    let path = match InteractiveClient::connect_endpoint(endpoint, color_scheme) {
        Ok(client) => return Ok(client),
        Err(error) => match endpoint {
            Endpoint::Local(path) => {
                let error = classify_local_connect_error(path, error);
                if !spawnable(&error) {
                    return Err(error.to_string());
                }
                path
            }
            Endpoint::Ssh(_) => return Err(error.to_string()),
        },
    };
    let executable = resolve_executable()?;
    let mut command = daemon_command(&executable, path, color_scheme);
    detach(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start {}: {error}", executable.display()))?;
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut exit_status = None;
    loop {
        match InteractiveClient::connect_endpoint(endpoint, color_scheme) {
            Ok(client) => {
                reap(child);
                return Ok(client);
            }
            Err(error) => {
                let error = classify_local_connect_error(path, error);
                if !spawnable(&error) || Instant::now() >= deadline {
                    reap(child);
                    return Err(format!(
                        "Could not connect to the daemon at {} after starting {}{}: {error}",
                        path.display(),
                        executable.display(),
                        exit_status.map_or_else(String::new, |status| format!(" ({status})")),
                    ));
                }
            }
        }
        if exit_status.is_none() {
            exit_status = child.try_wait().ok().flatten();
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn spawnable(error: &DaemonError) -> bool {
    matches!(error, DaemonError::Io(error) if matches!(error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset
    ))
}

fn resolve_executable() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("ZZ_GTK_DAEMON_EXECUTABLE") {
        return verified_executable(Path::new(&path));
    }
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let candidates = executable_candidates(&current, std::env::var_os("PATH").as_deref());
    let mut failures = Vec::new();
    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        match verified_executable(&candidate) {
            Ok(executable) => return Ok(executable),
            Err(error) => failures.push(error),
        }
    }
    let detail = if failures.is_empty() {
        String::new()
    } else {
        format!(" {}", failures.join("; "))
    };
    Err(format!(
        "No compatible zz daemon executable was found beside zz-gtk or on PATH. Set ZZ_GTK_DAEMON_EXECUTABLE to a zz executable using protocol v{PROTOCOL_VERSION}.{detail}"
    ))
}

fn executable_candidates(current: &Path, path: Option<&OsStr>) -> Vec<PathBuf> {
    let name = format!("zz{}", std::env::consts::EXE_SUFFIX);
    let mut candidates = vec![current.with_file_name(&name)];
    if let Some(path) = path {
        for directory in std::env::split_paths(path) {
            let candidate = directory.join(&name);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn verified_executable(path: &Path) -> Result<PathBuf, String> {
    let executable = path
        .canonicalize()
        .map_err(|error| format!("Could not resolve {}: {error}", path.display()))?;
    let mut child = executable_command(&executable)
        .arg("protocol-version")
        .env_remove("ZZ_SOCKET")
        .env_remove("TMUX")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not inspect {}: {error}", executable.display()))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Could not inspect {}: {}",
                    executable.display(),
                    result.err().map_or_else(
                        || "protocol-version timed out".to_owned(),
                        |error| error.to_string()
                    ),
                ));
            }
        }
    };
    let mut output = String::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(128).read_to_string(&mut output);
    }
    if !status.success() || output.trim().parse::<u16>() != Ok(PROTOCOL_VERSION) {
        return Err(format!(
            "{} does not report protocol v{PROTOCOL_VERSION} (status {status}, reported {:?})",
            executable.display(),
            output.trim(),
        ));
    }
    Ok(executable)
}

fn daemon_command(executable: &Path, socket: &Path, scheme: TerminalColorScheme) -> Command {
    let mut command = executable_command(executable);
    command
        .args([
            OsString::from("-S"),
            socket.as_os_str().to_owned(),
            OsString::from("daemon"),
        ])
        .env("ZZ_TMUX_EXECUTABLE", executable)
        .env("ZZ_COLOR_SCHEME", scheme.as_str())
        .env_remove("ZZ_APP_STARTUP_DIRECTORY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(directory) = std::env::current_dir()
        && directory.is_absolute()
        && directory.is_dir()
        && directory
            .to_str()
            .is_some_and(|value| value.len() <= MAX_CLIENT_WORKING_DIRECTORY_BYTES)
    {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let server_id = ((timestamp as u64) ^ u64::from(std::process::id()).rotate_left(32)).max(1);
        command
            .arg("--bootstrap-server-id")
            .arg(server_id.to_string())
            .arg("--bootstrap-client-cwd")
            .arg(directory);
    }
    command
}

fn executable_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    #[cfg(target_os = "linux")]
    command.env(
        "LD_LIBRARY_PATH",
        library_path(executable, std::env::var_os("LD_LIBRARY_PATH").as_deref()),
    );
    command
}

#[cfg(target_os = "linux")]
fn library_path(executable: &Path, inherited: Option<&OsStr>) -> OsString {
    let mut path = executable
        .parent()
        .unwrap_or(Path::new("."))
        .as_os_str()
        .to_owned();
    if let Some(inherited) = inherited.filter(|value| !value.is_empty()) {
        path.push(":");
        path.push(inherited);
    }
    path
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(not(unix))]
fn detach(_: &mut Command) {}

fn reap(mut child: Child) {
    thread::spawn(move || {
        let _ = child.wait();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_missing_local_transport_errors_allow_startup() {
        for kind in [
            io::ErrorKind::NotFound,
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::ConnectionReset,
        ] {
            assert!(spawnable(&DaemonError::Io(io::Error::from(kind))));
        }
        assert!(!spawnable(&DaemonError::Io(io::Error::from(
            io::ErrorKind::PermissionDenied
        ))));
        assert!(!spawnable(&DaemonError::IncompatibleDaemon {
            daemon: Some(PROTOCOL_VERSION - 1),
            client: PROTOCOL_VERSION
        }));
    }

    #[test]
    #[cfg(unix)]
    fn discovery_prefers_sibling_then_path_without_duplicates() {
        let path = std::env::join_paths(["/app", "/usr/local/bin", "/usr/bin"]).unwrap();
        assert_eq!(
            executable_candidates(Path::new("/app/zz-gtk"), Some(&path)),
            ["/app/zz", "/usr/local/bin/zz", "/usr/bin/zz"].map(PathBuf::from)
        );
    }

    #[test]
    #[cfg(unix)]
    fn daemon_launch_preserves_socket_and_cli_identity() {
        let command = daemon_command(
            Path::new("/app/zz"),
            Path::new("/tmp/gtk socket"),
            TerminalColorScheme::Light,
        );
        let arguments = command.get_args().collect::<Vec<_>>();
        assert_eq!(&arguments[..3], ["-S", "/tmp/gtk socket", "daemon"]);
        assert!(arguments.contains(&OsStr::new("--bootstrap-client-cwd")));
        let environment = command.get_envs().collect::<Vec<_>>();
        assert!(environment.contains(&(
            OsStr::new("ZZ_TMUX_EXECUTABLE"),
            Some(OsStr::new("/app/zz"))
        )));
        assert!(environment.contains(&(OsStr::new("ZZ_COLOR_SCHEME"), Some(OsStr::new("light")))));
        assert!(environment.contains(&(OsStr::new("ZZ_APP_STARTUP_DIRECTORY"), None)));
        #[cfg(target_os = "linux")]
        {
            let expected = library_path(
                Path::new("/app/zz"),
                std::env::var_os("LD_LIBRARY_PATH").as_deref(),
            );
            assert!(
                environment.contains(&(OsStr::new("LD_LIBRARY_PATH"), Some(expected.as_os_str())))
            );
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn daemon_libraries_precede_inherited_gtk_libraries() {
        let executable = Path::new("/opt/zz release/zz");
        assert_eq!(
            library_path(executable, Some(OsStr::new("/gtk/cef:/vendor/lib"))),
            "/opt/zz release:/gtk/cef:/vendor/lib"
        );
        assert_eq!(
            library_path(executable, Some(OsStr::new(""))),
            "/opt/zz release"
        );
        assert_eq!(library_path(executable, None), "/opt/zz release");
    }

    #[test]
    #[cfg(unix)]
    fn executable_probe_requires_success_and_the_current_protocol() {
        use std::os::unix::fs::PermissionsExt;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("zz-gtk-probe-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let executable = directory.join("zz executable");
        for (version, status, accepted) in [
            (PROTOCOL_VERSION, 0, true),
            (PROTOCOL_VERSION - 1, 0, false),
            (PROTOCOL_VERSION, 1, false),
        ] {
            let library_check = if cfg!(target_os = "linux") {
                "[ \"${LD_LIBRARY_PATH%%:*}\" = \"${0%/*}\" ] || exit 8\n"
            } else {
                ""
            };
            std::fs::write(&executable, format!("#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = protocol-version ] || exit 9\n{library_check}printf '%s\\n' '{version}'\nexit {status}\n")).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let result = verified_executable(&executable);
            assert_eq!(result.is_ok(), accepted, "{result:?}");
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
