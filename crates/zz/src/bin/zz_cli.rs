//! The launcher every `zz` on `PATH` points at: it canonicalizes its own path
//! into the macOS bundle and execs the real `zz` beside it.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

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
    launch(&executable, &arguments)
}

fn bundled_executable(launcher: &Path) -> Result<PathBuf, String> {
    let executable = launcher
        .parent()
        .ok_or("the launcher path has no parent directory")?
        .join("zz");
    if !executable.is_file() {
        return Err(format!(
            "the zz executable is missing from the application bundle: {}",
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
    use super::bundled_executable;

    #[test]
    fn resolves_the_executable_beside_the_launcher() {
        let bundle = tempfile::tempdir().expect("temporary bundle");
        let executable = bundle.path().join("zz");
        std::fs::write(&executable, b"").expect("write executable");

        assert_eq!(
            bundled_executable(&bundle.path().join("cli")),
            Ok(executable)
        );
    }

    #[test]
    fn reports_a_bundle_without_the_executable() {
        let bundle = tempfile::tempdir().expect("temporary bundle");

        let error = bundled_executable(&bundle.path().join("cli"))
            .expect_err("a bundle without zz cannot be launched");

        assert!(
            error.contains("missing from the application bundle"),
            "{error}"
        );
    }
}
