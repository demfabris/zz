use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use portable_pty::{CommandBuilder, PtySize};
use rustix::fs::{Access, Mode, OFlags};
use rustix::io::FdFlags;
use rustix::pty::OpenptFlags;

/// The master side of a pane's PTY. The pair comes from `posix_openpt`, and
/// the slave's name from `ptsname` (`TIOCPTYGNAME` on macOS), where
/// portable-pty's `openpty` asked `ttyname_r`, which walks `/dev`.
pub(super) struct UnixMaster(OwnedFd);

impl UnixMaster {
    pub(super) fn resize(&self, size: PtySize) -> io::Result<()> {
        rustix::termios::tcsetwinsize(
            &self.0,
            rustix::termios::Winsize {
                ws_row: size.rows,
                ws_col: size.cols,
                ws_xpixel: size.pixel_width,
                ws_ypixel: size.pixel_height,
            },
        )
        .map_err(io::Error::from)
    }

    pub(super) fn process_group_leader(&self) -> Option<i32> {
        rustix::termios::tcgetpgrp(&self.0)
            .ok()
            .map(rustix::process::Pid::as_raw_nonzero)
            .map(std::num::NonZeroI32::get)
    }

    pub(super) fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

pub(super) struct UnixPty {
    pub(super) master: UnixMaster,
    pub(super) slave: OwnedFd,
    pub(super) tty: PathBuf,
}

pub(super) fn open(size: PtySize) -> io::Result<UnixPty> {
    let master = rustix::pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
    rustix::io::fcntl_setfd(&master, FdFlags::CLOEXEC)?;
    rustix::pty::grantpt(&master)?;
    rustix::pty::unlockpt(&master)?;
    let name = rustix::pty::ptsname(&master, Vec::new())?;
    let slave = rustix::fs::open(
        name.as_c_str(),
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let master = UnixMaster(master);
    master.resize(size)?;
    Ok(UnixPty {
        master,
        slave,
        tty: PathBuf::from(OsStr::from_bytes(name.as_bytes())),
    })
}

/// The whole environment a builder will hand its child, read back value by
/// value so a non-UTF-8 entry survives: the inherited keys, every key the
/// builder was told about in UTF-8, and the keys the caller names.
pub(super) fn command_environment<'a>(
    builder: &CommandBuilder,
    named: impl IntoIterator<Item = &'a OsStr>,
) -> Vec<(OsString, OsString)> {
    let mut keys = std::env::vars_os()
        .map(|(key, _)| key)
        .collect::<BTreeSet<_>>();
    keys.extend(
        builder
            .iter_extra_env_as_str()
            .map(|(key, _)| OsString::from(key)),
    );
    keys.extend(named.into_iter().map(OsStr::to_owned));
    keys.insert(OsString::from("SHELL"));
    keys.into_iter()
        .filter_map(|key| {
            let value = builder.get_env(&key)?.to_owned();
            Some((key, value))
        })
        .collect()
}

/// Starts the builder's program on the PTY slave the way portable-pty's
/// `spawn_command` does: the same program resolution, working directory,
/// login-shell `argv[0]`, environment and child setup.
pub(super) fn spawn(
    builder: &CommandBuilder,
    environment: Vec<(OsString, OsString)>,
    slave: &OwnedFd,
) -> io::Result<u32> {
    let mut command = std_command(builder, environment)?;
    let stdio = || slave.try_clone().map(Stdio::from);
    command.stdin(stdio()?).stdout(stdio()?).stderr(stdio()?);
    #[allow(
        unsafe_code,
        reason = "the child setup runs between fork and exec and only makes syscalls"
    )]
    unsafe {
        command.pre_exec(child_setup);
    }
    let child = command.spawn()?;
    Ok(child.id())
}

fn std_command(
    builder: &CommandBuilder,
    environment: Vec<(OsString, OsString)>,
) -> io::Result<std::process::Command> {
    let home = environment
        .iter()
        .find(|(key, _)| key == "HOME")
        .map(|(_, value)| PathBuf::from(value))
        .or_else(passwd_home)
        .unwrap_or_else(|| PathBuf::from("/"));
    let directory = builder
        .get_cwd()
        .map(PathBuf::from)
        .filter(|directory| directory.is_dir())
        .unwrap_or(home);
    let shell = builder.get_shell();
    let mut command = if builder.is_default_prog() {
        let mut command = std::process::Command::new(&shell);
        let basename = shell.rsplit('/').next().unwrap_or(&shell);
        command.arg0(format!("-{basename}"));
        command
    } else {
        let argv = builder.get_argv();
        let path = environment
            .iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| value.as_os_str());
        let resolved = search_path(&argv[0], &directory, path)?;
        let mut command = std::process::Command::new(resolved);
        command.arg0(&argv[0]);
        command.args(&argv[1..]);
        command
    };
    command.current_dir(&directory);
    command.env_clear();
    command.env("SHELL", &shell);
    command.envs(environment);
    Ok(command)
}

#[allow(
    deprecated,
    reason = "home_dir reads the password database the way portable-pty's getpwuid lookup did"
)]
fn passwd_home() -> Option<PathBuf> {
    std::env::home_dir()
}

fn executable(path: &Path) -> bool {
    rustix::fs::access(path, Access::EXEC_OK).is_ok()
}

fn exists(path: &Path) -> bool {
    rustix::fs::access(path, Access::EXISTS).is_ok()
}

fn search_path(exe: &OsStr, cwd: &Path, path: Option<&OsStr>) -> io::Result<PathBuf> {
    let unable = |message: String| io::Error::new(ErrorKind::NotFound, message);
    let exe_path = Path::new(exe);
    if exe_path.is_relative() {
        if matches!(
            exe_path.components().next(),
            Some(Component::CurDir | Component::ParentDir)
        ) {
            let absolute = cwd.join(exe_path);
            if absolute.is_dir() {
                return Err(unable(format!(
                    "Unable to spawn {} because it is a directory",
                    absolute.display()
                )));
            }
            if executable(&absolute) {
                return Ok(absolute);
            }
            if exists(&absolute) {
                return Err(unable(format!(
                    "Unable to spawn {} because it is not executable",
                    absolute.display()
                )));
            }
            return Err(unable(format!(
                "Unable to spawn {} because it does not exist",
                absolute.display()
            )));
        }
        let mut errors = Vec::new();
        if let Some(path) = path {
            for entry in std::env::split_paths(path) {
                let candidate = cwd.join(&entry).join(exe);
                if candidate.is_dir() {
                    errors.push(format!("{} exists but is a directory", candidate.display()));
                } else if executable(&candidate) {
                    return Ok(candidate);
                } else if exists(&candidate) {
                    errors.push(format!(
                        "{} exists but is not executable",
                        candidate.display()
                    ));
                }
            }
            errors.push(format!("No viable candidates found in PATH {path:?}"));
        } else {
            errors.push("Unable to resolve the PATH".to_owned());
        }
        return Err(unable(format!(
            "Unable to spawn {} because:\n{}",
            exe_path.display(),
            errors.join(".\n")
        )));
    }
    if exe_path.is_dir() {
        return Err(unable(format!(
            "Unable to spawn {} because it is a directory",
            exe_path.display()
        )));
    }
    if !executable(exe_path) {
        return Err(unable(if exists(exe_path) {
            format!(
                "Unable to spawn {} because it is not executable",
                exe_path.display()
            )
        } else {
            format!(
                "Unable to spawn {} because it doesn't exist on the filesystem",
                exe_path.display()
            )
        }));
    }
    Ok(exe_path.to_owned())
}

#[allow(
    unsafe_code,
    reason = "signal dispositions and the controlling terminal are set with raw libc calls in the forked child"
)]
fn child_setup() -> io::Result<()> {
    for signal in [
        libc::SIGCHLD,
        libc::SIGHUP,
        libc::SIGINT,
        libc::SIGQUIT,
        libc::SIGTERM,
        libc::SIGALRM,
    ] {
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
        }
    }
    unsafe {
        let empty: libc::sigset_t = std::mem::zeroed();
        libc::sigprocmask(libc::SIG_SETMASK, &raw const empty, std::ptr::null_mut());
    }
    rustix::process::setsid()?;
    rustix::process::ioctl_tiocsctty(std::io::stdin())?;
    close_inherited_descriptors();
    Ok(())
}

/// Closes every descriptor above the standard three, the way portable-pty's
/// `close_random_fds` does, so nothing the daemon inherited leaks to a pane.
fn close_inherited_descriptors() {
    let Ok(entries) = std::fs::read_dir("/dev/fd") else {
        return;
    };
    let descriptors = entries
        .filter_map(|entry| {
            entry
                .ok()?
                .file_name()
                .into_string()
                .ok()?
                .parse::<RawFd>()
                .ok()
        })
        .filter(|descriptor| *descriptor > 2)
        .collect::<Vec<_>>();
    for descriptor in descriptors {
        #[allow(
            unsafe_code,
            reason = "closing a raw descriptor number listed in /dev/fd right before exec"
        )]
        unsafe {
            libc::close(descriptor);
        }
    }
}
