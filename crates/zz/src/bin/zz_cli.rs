//! The launcher every `zz` on `PATH` points at: it canonicalizes its own path
//! into the installed bundle and execs the real `zz` beside it.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

const APP_STARTUP_DIRECTORY_ENV: &str = "ZZ_APP_STARTUP_DIRECTORY";

/// Private handshake with the bundled executable: a `zz` typed on the command
/// line is a client, so a command line that carries no command word runs the
/// stored `default-client-command` instead of opening the application.
const LAUNCHER_CLIENT_ARGUMENT: &str = "--bootstrap-launcher-client";

fn main() -> ExitCode {
    let launcher = match env::current_exe().and_then(|launcher| launcher.canonicalize()) {
        Ok(launcher) => launcher,
        Err(error) => {
            eprintln!("zz: could not resolve the launcher path: {error}");
            return ExitCode::FAILURE;
        }
    };
    let executable = match bundled_executable(&launcher) {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("zz: {error}");
            return ExitCode::FAILURE;
        }
    };
    let arguments = env::args_os().skip(1).collect::<Vec<OsString>>();
    if arguments == [OsString::from("app")] {
        return launch_application(&launcher, &executable);
    }
    let arguments = cli_arguments(arguments);
    launch(&executable, &arguments)
}

fn cli_arguments(arguments: Vec<OsString>) -> Vec<OsString> {
    let mut marked = Vec::with_capacity(arguments.len() + 1);
    marked.push(OsString::from(LAUNCHER_CLIENT_ARGUMENT));
    marked.extend(arguments);
    marked
}

fn startup_directory() -> Option<PathBuf> {
    env::current_dir()
        .ok()
        .filter(|directory| directory.is_dir())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_dir())
        })
}

#[cfg(target_os = "macos")]
fn startup_directory_environment(directory: &Path) -> OsString {
    let mut environment = OsString::from(APP_STARTUP_DIRECTORY_ENV);
    environment.push("=");
    environment.push(directory);
    environment
}

#[cfg(target_os = "macos")]
fn launch_application(launcher: &Path, _executable: &Path) -> ExitCode {
    let Some(bundle) = launcher
        .ancestors()
        .find(|path| path.extension() == Some(std::ffi::OsStr::new("app")))
    else {
        eprintln!("zz: the launcher is not inside an application bundle");
        return ExitCode::FAILURE;
    };
    let mut command = Command::new("/usr/bin/open");
    command
        .arg("-n")
        .args(["--env", "TMUX=", "--env", "TMUX_PANE="]);
    if let Some(directory) = startup_directory() {
        command.args([
            OsString::from("--env"),
            startup_directory_environment(&directory),
        ]);
    }
    if let Some(socket) = env::var_os("ZZ_SOCKET").filter(|socket| !socket.is_empty()) {
        let mut socket_environment = OsString::from("ZZ_SOCKET=");
        socket_environment.push(socket);
        command.args([OsString::from("--env"), socket_environment]);
    }
    match command
        .arg(bundle)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .status()
    {
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
fn launch_application(_launcher: &Path, executable: &Path) -> ExitCode {
    use std::process::Stdio;

    let mut command = Command::new(executable);
    command
        .arg("app")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(directory) = startup_directory() {
        command.env(APP_STARTUP_DIRECTORY_ENV, directory);
    }
    match command.spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zz: could not open the application: {error}");
            ExitCode::FAILURE
        }
    }
}

fn bundled_executable(launcher: &Path) -> Result<PathBuf, String> {
    let executable = launcher
        .parent()
        .ok_or("the launcher path has no parent directory")?
        .join("zz");
    if !executable.is_file() {
        return Err(format!(
            "the zz executable is missing from the installed bundle: {}",
            executable.display()
        ));
    }
    Ok(executable)
}

#[cfg(unix)]
fn launch(executable: &Path, arguments: &[OsString]) -> ExitCode {
    use std::os::unix::process::CommandExt as _;

    let error = Command::new(executable).args(arguments).exec();
    eprintln!("zz: could not run {}: {error}", executable.display());
    ExitCode::FAILURE
}

#[cfg(not(unix))]
fn launch(executable: &Path, arguments: &[OsString]) -> ExitCode {
    match Command::new(executable).args(arguments).status() {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(error) => {
            eprintln!("zz: could not run {}: {error}", executable.display());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    #[cfg(target_os = "macos")]
    use std::path::Path;

    use super::{bundled_executable, cli_arguments};

    #[cfg(target_os = "macos")]
    use super::startup_directory_environment;

    #[test]
    fn resolves_the_executable_beside_the_launcher() {
        let bundle = tempfile::Builder::new()
            .prefix("zz launcher bundle ")
            .tempdir()
            .expect("temporary bundle");
        let directory = bundle.path().join("path with spaces");
        std::fs::create_dir(&directory).expect("create bundle directory");
        let executable = directory.join("zz");
        std::fs::write(&executable, b"").expect("write executable");

        assert_eq!(bundled_executable(&directory.join("cli")), Ok(executable));
    }

    #[test]
    fn reports_a_bundle_without_the_executable() {
        let bundle = tempfile::tempdir().expect("temporary bundle");

        let error = bundled_executable(&bundle.path().join("cli"))
            .expect_err("a bundle without zz cannot be launched");

        assert!(
            error.contains("missing from the installed bundle"),
            "{error}"
        );
    }

    #[test]
    fn every_cli_launch_carries_the_client_marker_ahead_of_its_own_words() {
        assert_eq!(
            cli_arguments(Vec::new()),
            [OsString::from(super::LAUNCHER_CLIENT_ARGUMENT)]
        );
        for arguments in [
            vec![
                OsString::from("new"),
                OsString::from("-s"),
                OsString::from("work"),
            ],
            vec![
                OsString::from("attach"),
                OsString::from("-t"),
                OsString::from("work"),
            ],
            vec![OsString::from("-L"), OsString::from("probe")],
            vec![OsString::from("list-sessions")],
        ] {
            let mut expected = vec![OsString::from(super::LAUNCHER_CLIENT_ARGUMENT)];
            expected.extend(arguments.clone());
            assert_eq!(cli_arguments(arguments), expected);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_launch_preserves_the_callers_working_directory() {
        assert_eq!(
            startup_directory_environment(Path::new("/tmp/a project")),
            OsString::from("ZZ_APP_STARTUP_DIRECTORY=/tmp/a project")
        );
    }
}
