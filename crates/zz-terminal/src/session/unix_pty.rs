use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CString, OsStr, OsString};
use std::io::{self, ErrorKind};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

use portable_pty::{CommandBuilder, PtySize};
use rustix::fs::{Access, Mode, OFlags};
use rustix::io::FdFlags;
use rustix::pty::OpenptFlags;

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

pub(super) fn spawn(
    builder: &CommandBuilder,
    environment: Vec<(OsString, OsString)>,
    slave: &OwnedFd,
) -> io::Result<u32> {
    let plan = ExecPlan::new(builder, environment)?;
    let pointers = ExecPointers {
        argv: pointers(&plan.argv),
        envp: pointers(&plan.envp),
        fallback: pointers(&plan.fallback),
    };
    let mut descriptors = DescriptorScratch::new();
    #[allow(
        unsafe_code,
        reason = "the child only makes async-signal-safe calls on memory prepared before the fork"
    )]
    match unsafe { libc::fork() } {
        -1 => Err(io::Error::last_os_error()),
        0 => unsafe { exec_child(&plan, &pointers, slave.as_raw_fd(), &mut descriptors) },
        pid => Ok(pid.cast_unsigned()),
    }
}

struct ExecPointers {
    argv: Vec<*const libc::c_char>,
    envp: Vec<*const libc::c_char>,
    fallback: Vec<*const libc::c_char>,
}

struct ExecPlan {
    program: CString,
    directory: CString,
    argv: Vec<CString>,
    envp: Vec<CString>,
    fallback: Vec<CString>,
}

impl ExecPlan {
    fn new(builder: &CommandBuilder, environment: Vec<(OsString, OsString)>) -> io::Result<Self> {
        let shell = builder.get_shell();
        let mut variables = BTreeMap::from([(OsString::from("SHELL"), OsString::from(&shell))]);
        variables.extend(environment);
        let home = variables
            .get(OsStr::new("HOME"))
            .map(PathBuf::from)
            .or_else(passwd_home)
            .unwrap_or_else(|| PathBuf::from("/"));
        let directory = builder
            .get_cwd()
            .map(PathBuf::from)
            .filter(|directory| directory.is_dir())
            .unwrap_or(home);
        let path = variables.get(OsStr::new("PATH")).map(OsString::as_os_str);
        let (program, argv) = if builder.is_default_prog() {
            let basename = shell.rsplit('/').next().unwrap_or(&shell);
            (
                search_path(OsStr::new(&shell), &directory, path)?,
                vec![OsString::from(format!("-{basename}"))],
            )
        } else {
            let argv = builder.get_argv();
            (search_path(&argv[0], &directory, path)?, argv.clone())
        };
        let program = c_string(program.as_os_str())?;
        let mut fallback = vec![CString::from(c"sh"), program.clone()];
        for argument in argv.iter().skip(1) {
            fallback.push(c_string(argument)?);
        }
        Ok(Self {
            program,
            directory: c_string(directory.as_os_str())?,
            argv: argv
                .iter()
                .map(|argument| c_string(argument))
                .collect::<io::Result<_>>()?,
            envp: variables
                .into_iter()
                .filter_map(|(key, value)| {
                    let mut entry = key.into_vec();
                    entry.push(b'=');
                    entry.extend_from_slice(value.as_bytes());
                    CString::new(entry).ok()
                })
                .collect(),
            fallback,
        })
    }
}

fn c_string(value: &OsStr) -> io::Result<CString> {
    CString::new(value.as_bytes()).map_err(|_| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("{} contains a NUL byte", value.display()),
        )
    })
}

fn pointers(values: &[CString]) -> Vec<*const libc::c_char> {
    values
        .iter()
        .map(|value| value.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}

#[cfg(target_os = "macos")]
struct DescriptorScratch(Vec<libc::proc_fdinfo>);

#[cfg(target_os = "macos")]
impl DescriptorScratch {
    fn new() -> Self {
        let open = rustix::process::getrlimit(rustix::process::Resource::Nofile)
            .current
            .unwrap_or(4096)
            .clamp(256, 65_536);
        Self(Vec::with_capacity(usize::try_from(open).unwrap_or(4096)))
    }

    #[allow(
        unsafe_code,
        reason = "proc_pidinfo writes into capacity reserved before the fork"
    )]
    unsafe fn close_inherited(&mut self) {
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
                    libc::close(descriptor);
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
struct DescriptorScratch(libc::c_int);

#[cfg(not(target_os = "macos"))]
impl DescriptorScratch {
    fn new() -> Self {
        let open = rustix::process::getrlimit(rustix::process::Resource::Nofile)
            .current
            .unwrap_or(4096)
            .clamp(256, 65_536);
        Self(libc::c_int::try_from(open).unwrap_or(4096))
    }

    #[allow(
        unsafe_code,
        reason = "close_range and close are async-signal-safe syscalls"
    )]
    unsafe fn close_inherited(&mut self) {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if unsafe { libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 0_u32) } == 0 {
            return;
        }
        for descriptor in 3..self.0 {
            unsafe {
                libc::close(descriptor);
            }
        }
    }
}

#[allow(
    unsafe_code,
    reason = "runs in the forked child between fork and exec, on memory prepared before the fork"
)]
unsafe fn exec_child(
    plan: &ExecPlan,
    pointers: &ExecPointers,
    slave: RawFd,
    descriptors: &mut DescriptorScratch,
) -> ! {
    unsafe {
        for signal in [
            libc::SIGCHLD,
            libc::SIGHUP,
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGTERM,
            libc::SIGALRM,
            libc::SIGPIPE,
        ] {
            libc::signal(signal, libc::SIG_DFL);
        }
        let empty: libc::sigset_t = std::mem::zeroed();
        libc::sigprocmask(libc::SIG_SETMASK, &raw const empty, std::ptr::null_mut());
        if libc::setsid() == -1
            || rustix::process::ioctl_tiocsctty(std::os::fd::BorrowedFd::borrow_raw(slave)).is_err()
        {
            libc::_exit(1);
        }
        for target in 0..=2 {
            if libc::dup2(slave, target) == -1 {
                libc::_exit(1);
            }
        }
        if libc::chdir(plan.directory.as_ptr()) == -1 {
            libc::_exit(1);
        }
        descriptors.close_inherited();
        libc::execve(
            plan.program.as_ptr(),
            pointers.argv.as_ptr(),
            pointers.envp.as_ptr(),
        );
        if io::Error::last_os_error().raw_os_error() == Some(libc::ENOEXEC) {
            libc::execve(
                c"/bin/sh".as_ptr(),
                pointers.fallback.as_ptr(),
                pointers.envp.as_ptr(),
            );
        }
        libc::_exit(1)
    }
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
