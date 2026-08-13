use portable_pty::CommandBuilder;

#[cfg(any(unix, windows))]
use std::{
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(unix)]
const BASH_INTEGRATION: &[u8] =
    include_bytes!("../assets/shell-integration/bash/zz-integration.bash");
#[cfg(unix)]
const ZSH_BOOTSTRAP: &[u8] = include_bytes!("../assets/shell-integration/zsh/.zshenv");
#[cfg(unix)]
const ZSH_INTEGRATION: &[u8] = include_bytes!("../assets/shell-integration/zsh/zz-integration.zsh");
#[cfg(windows)]
const POWERSHELL_INTEGRATION: &[u8] =
    include_bytes!("../assets/shell-integration/powershell/zz-integration.ps1");

#[cfg(any(unix, windows))]
static RESOURCE_ROOT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
#[cfg(any(unix, windows))]
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(any(unix, windows))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum IntegrationMode {
    #[default]
    Detect,
    None,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shell {
    Bash,
    Zsh,
}

pub(super) fn default_shell_command() -> CommandBuilder {
    let command = CommandBuilder::new_default_prog();
    #[cfg(any(unix, windows))]
    {
        configure_default_shell(command)
    }
    #[cfg(not(any(unix, windows)))]
    {
        command
    }
}

#[cfg(unix)]
fn configure_default_shell(mut command: CommandBuilder) -> CommandBuilder {
    let mode = integration_mode(std::env::var_os("ZZ_SHELL_INTEGRATION").as_deref());
    command.env_remove("ZZ_SHELL_INTEGRATION");
    if mode == IntegrationMode::None {
        log::debug!(target: "zz_terminal::shell_integration", "shell integration disabled");
        return command;
    }

    let shell_path = command.get_shell();
    let Some(shell) = detect_shell(&shell_path) else {
        log::debug!(
            target: "zz_terminal::shell_integration",
            "no automatic title integration for shell={shell_path:?}",
        );
        return command;
    };
    if shell == Shell::Bash && unsupported_apple_bash(&shell_path) {
        log::debug!(
            target: "zz_terminal::shell_integration",
            "automatic title integration unavailable for macOS system bash shell={shell_path:?}",
        );
        return command;
    }

    let Some(root) = resource_root() else {
        return command;
    };
    match shell {
        Shell::Bash => configure_bash(command, &shell_path, &root),
        Shell::Zsh => {
            configure_zsh(&mut command, &root);
            command
        }
    }
}

#[cfg(any(unix, windows))]
fn integration_mode(value: Option<&OsStr>) -> IntegrationMode {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return IntegrationMode::Detect;
    };
    match value.to_string_lossy().trim().to_ascii_lowercase().as_str() {
        "detect" | "true" | "1" => IntegrationMode::Detect,
        "none" | "false" | "0" => IntegrationMode::None,
        value => {
            log::warn!(
                target: "zz_terminal::shell_integration",
                "invalid ZZ_SHELL_INTEGRATION={value:?}; using detect",
            );
            IntegrationMode::Detect
        }
    }
}

#[cfg(unix)]
fn detect_shell(shell: &str) -> Option<Shell> {
    match Path::new(shell).file_name().and_then(OsStr::to_str) {
        Some("bash") => Some(Shell::Bash),
        Some("zsh") => Some(Shell::Zsh),
        _ => None,
    }
}

#[cfg(all(unix, target_os = "macos"))]
fn unsupported_apple_bash(shell: &str) -> bool {
    shell == "/bin/bash"
}

#[cfg(all(unix, not(target_os = "macos")))]
const fn unsupported_apple_bash(_: &str) -> bool {
    false
}

#[cfg(unix)]
fn configure_bash(mut command: CommandBuilder, shell: &str, root: &Path) -> CommandBuilder {
    command.get_argv_mut().push(OsString::from(shell));
    command.args(["--posix", "--login"]);
    if let Some(existing) = command.get_env("ENV").map(OsStr::to_owned) {
        command.env("ZZ_BASH_ENV", existing);
    }
    command.env("ENV", root.join("bash/zz-integration.bash"));
    command.env("ZZ_BASH_INJECT", "1");
    command.env("ZZ_SHELL_INTEGRATION_ACTIVE", "bash-title");

    if command.get_env("HISTFILE").is_none()
        && let Some(home) = command.get_env("HOME").map(PathBuf::from)
    {
        command.env("HISTFILE", home.join(".bash_history"));
        command.env("ZZ_BASH_UNEXPORT_HISTFILE", "1");
    }
    command
}

#[cfg(unix)]
fn configure_zsh(command: &mut CommandBuilder, root: &Path) {
    if let Some(existing) = command.get_env("ZDOTDIR").map(OsStr::to_owned) {
        command.env("ZZ_ZSH_ZDOTDIR", existing);
    }
    command.env("ZDOTDIR", root.join("zsh"));
    command.env("ZZ_SHELL_INTEGRATION_DIR", root);
    command.env("ZZ_SHELL_INTEGRATION_ACTIVE", "zsh-title");
}

#[cfg(any(unix, windows))]
fn resource_root() -> Option<PathBuf> {
    RESOURCE_ROOT
        .get_or_init(|| {
            let root = resource_cache_root().map_err(|error| error.to_string())?;
            materialize_resources(&root).map_err(|error| error.to_string())?;
            log::debug!(
                target: "zz_terminal::shell_integration",
                "prepared shell integration resources root={}",
                root.display(),
            );
            Ok(root)
        })
        .as_ref()
        .map_or_else(
            |error| {
                log::warn!(
                    target: "zz_terminal::shell_integration",
                    "shell integration unavailable: {error}",
                );
                None
            },
            |root| Some(root.clone()),
        )
}

#[cfg(unix)]
fn resource_cache_root() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(platform_cache_root)
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                "neither XDG_CACHE_HOME nor HOME can locate a shell integration cache",
            )
        })?;
    Ok(base.join("zz/shell-integration/v1"))
}

#[cfg(all(unix, target_os = "macos"))]
fn platform_cache_root() -> Option<PathBuf> {
    nonempty_home().map(|home| home.join("Library/Caches"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_cache_root() -> Option<PathBuf> {
    nonempty_home().map(|home| home.join(".cache"))
}

#[cfg(unix)]
fn nonempty_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

#[cfg(unix)]
fn materialize_resources(root: &Path) -> io::Result<()> {
    create_private_directory(root)?;
    write_resource(&root.join("bash/zz-integration.bash"), BASH_INTEGRATION)?;
    write_resource(&root.join("zsh/.zshenv"), ZSH_BOOTSTRAP)?;
    write_resource(&root.join("zsh/zz-integration.zsh"), ZSH_INTEGRATION)
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
fn write_resource(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "resource path has no parent"))?;
    create_private_directory(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "refusing non-file shell integration resource {}",
                    path.display()
                ),
            ));
        }
        if fs::read(path)? == contents {
            return fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
    }

    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "resource filename is not UTF-8"))?;
    let temporary = parent.join(format!(
        ".{file_name}.{}.{sequence}.tmp",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn configure_default_shell(mut command: CommandBuilder) -> CommandBuilder {
    let mode = integration_mode(std::env::var_os("ZZ_SHELL_INTEGRATION").as_deref());
    command.env_remove("ZZ_SHELL_INTEGRATION");
    if mode == IntegrationMode::None {
        log::debug!(target: "zz_terminal::shell_integration", "shell integration disabled");
        return command;
    }

    let Some(shell) = powershell_path(&command) else {
        log::debug!(
            target: "zz_terminal::shell_integration",
            "no automatic title integration for shell={:?}",
            command.get_shell(),
        );
        return command;
    };
    let Some(root) = resource_root() else {
        return command;
    };
    configure_powershell(command, &shell, &root)
}

#[cfg(windows)]
fn powershell_path(command: &CommandBuilder) -> Option<PathBuf> {
    let search = command
        .get_env("PATH")
        .map(OsStr::to_owned)
        .or_else(|| std::env::var_os("PATH"));
    for name in ["pwsh.exe", "powershell.exe"] {
        if let Some(found) = search
            .as_deref()
            .and_then(|search| lookup_in_path(search, name))
        {
            return Some(found);
        }
    }
    system_powershell_path(command).filter(|path| path.is_file())
}

#[cfg(windows)]
fn lookup_in_path(search: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(search)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn system_powershell_path(command: &CommandBuilder) -> Option<PathBuf> {
    let root = command
        .get_env("SystemRoot")
        .map(OsStr::to_owned)
        .or_else(|| std::env::var_os("SystemRoot"))?;
    Some(PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe"))
}

#[cfg(windows)]
fn configure_powershell(mut command: CommandBuilder, shell: &Path, root: &Path) -> CommandBuilder {
    let script = root.join("powershell/zz-integration.ps1");
    let Some(startup) = dot_source_command(&script) else {
        log::warn!(
            target: "zz_terminal::shell_integration",
            "shell integration path is not utf-8: {}",
            script.display(),
        );
        return command;
    };
    command.get_argv_mut().push(shell.as_os_str().to_owned());
    command.args([
        OsString::from("-NoLogo"),
        OsString::from("-NoExit"),
        OsString::from("-Command"),
        OsString::from(startup),
    ]);
    command.env("ZZ_SHELL_INTEGRATION_ACTIVE", "powershell-title");
    command
}

#[cfg(windows)]
fn dot_source_command(script: &Path) -> Option<String> {
    let script = script.to_str()?.replace('\'', "''");
    Some(format!(
        ". ([scriptblock]::Create([System.IO.File]::ReadAllText('{script}')))"
    ))
}

#[cfg(windows)]
fn resource_cache_root() -> io::Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            let temporary = std::env::temp_dir();
            temporary.is_absolute().then_some(temporary)
        })
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                "neither LOCALAPPDATA nor the temporary directory can locate a shell integration cache",
            )
        })?;
    Ok(base.join("zz/shell-integration/v1"))
}

#[cfg(windows)]
fn materialize_resources(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;
    write_resource(
        &root.join("powershell/zz-integration.ps1"),
        POWERSHELL_INTEGRATION,
    )
}

#[cfg(windows)]
fn write_resource(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "resource path has no parent"))?;
    fs::create_dir_all(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "refusing non-file shell integration resource {}",
                    path.display()
                ),
            ));
        }
        if fs::read(path)? == contents {
            return Ok(());
        }
    }

    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "resource filename is not UTF-8"))?;
    let temporary = parent.join(format!(
        ".{file_name}.{}.{sequence}.tmp",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        // Windows opens files without FILE_SHARE_DELETE, so close before renaming.
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn integration_mode_defaults_to_detect_and_accepts_an_off_switch() {
        assert_eq!(integration_mode(None), IntegrationMode::Detect);
        assert_eq!(
            integration_mode(Some(OsStr::new(""))),
            IntegrationMode::Detect
        );
        assert_eq!(
            integration_mode(Some(OsStr::new("detect"))),
            IntegrationMode::Detect
        );
        assert_eq!(
            integration_mode(Some(OsStr::new("none"))),
            IntegrationMode::None
        );
        assert_eq!(
            integration_mode(Some(OsStr::new("false"))),
            IntegrationMode::None
        );
    }

    #[cfg(unix)]
    #[test]
    fn detects_only_shells_with_owned_title_hooks() {
        assert_eq!(detect_shell("/opt/homebrew/bin/bash"), Some(Shell::Bash));
        assert_eq!(detect_shell("/bin/zsh"), Some(Shell::Zsh));
        assert_eq!(detect_shell("/usr/bin/fish"), None);
        assert_eq!(detect_shell("sh"), None);
    }

    #[cfg(unix)]
    #[test]
    fn materializes_private_versioned_resources() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().expect("temporary resource directory");
        let root = temporary.path().join("shell-integration/v1");
        materialize_resources(&root).expect("materialize shell integration");

        for relative in [
            "bash/zz-integration.bash",
            "zsh/.zshenv",
            "zsh/zz-integration.zsh",
        ] {
            let path = root.join(relative);
            assert!(path.is_file(), "missing {}", path.display());
            assert_eq!(
                fs::metadata(path)
                    .expect("resource metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        for relative in ["bash/zz-integration.bash", "zsh/zz-integration.zsh"] {
            let contents = fs::read_to_string(root.join(relative)).expect("integration contents");
            assert!(
                contents.contains("\\e[0 q"),
                "missing cursor reset in {relative}"
            );
            assert!(
                !(1..=6).any(|shape| contents.contains(&format!("\\e[{shape} q"))),
                "{relative} imposes a cursor shape instead of the configured default"
            );
        }
        assert_eq!(
            fs::metadata(root)
                .expect("resource root metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn configures_shell_specific_startup_without_losing_base_environment() {
        let temporary = tempfile::tempdir().expect("temporary resource directory");
        materialize_resources(temporary.path()).expect("materialize shell integration");

        let mut base = CommandBuilder::new_default_prog();
        base.env("HOME", "/tmp/zz-shell-home");
        base.env("SHELL", "/opt/homebrew/bin/bash");
        base.env("ZZ_FIXTURE", "preserved");
        let bash_command = configure_bash(base, "/opt/homebrew/bin/bash", temporary.path());
        assert_eq!(
            bash_command.get_argv(),
            &vec![
                OsString::from("/opt/homebrew/bin/bash"),
                OsString::from("--posix"),
                OsString::from("--login"),
            ]
        );
        assert_eq!(
            bash_command.get_env("ZZ_FIXTURE"),
            Some(OsStr::new("preserved"))
        );
        let bash_integration = temporary.path().join("bash/zz-integration.bash");
        assert_eq!(
            bash_command.get_env("ENV"),
            Some(bash_integration.as_os_str())
        );

        let mut zsh = CommandBuilder::new_default_prog();
        zsh.env("ZDOTDIR", "/tmp/custom-zdotdir");
        configure_zsh(&mut zsh, temporary.path());
        assert_eq!(
            zsh.get_env("ZZ_ZSH_ZDOTDIR"),
            Some(OsStr::new("/tmp/custom-zdotdir"))
        );
        let zsh_integration = temporary.path().join("zsh");
        assert_eq!(zsh.get_env("ZDOTDIR"), Some(zsh_integration.as_os_str()));
    }
}
