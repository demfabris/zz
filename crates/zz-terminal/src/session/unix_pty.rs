use std::collections::BTreeMap;
use std::ffi::{CString, OsStr, OsString};
use std::io::{self, ErrorKind};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::pty_types::{CommandBuilder, PtySize};
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

    #[allow(
        unsafe_code,
        reason = "tcgetpgrp only reads the foreground group of a descriptor this value owns"
    )]
    pub(super) fn process_group_leader(&self) -> Option<i32> {
        let group = unsafe { libc::tcgetpgrp(self.0.as_raw_fd()) };
        (group > 0).then_some(group)
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
    let mut environment = builder
        .iter_full_env_as_str()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect::<BTreeMap<_, _>>();
    for key in base_environment_keys()
        .iter()
        .map(OsString::as_os_str)
        .chain(named)
        .chain(std::iter::once(OsStr::new("SHELL")))
    {
        if !environment.contains_key(key)
            && let Some(value) = builder.get_env(key)
        {
            environment.insert(key.to_owned(), value.to_owned());
        }
    }
    environment.into_iter().collect()
}

fn base_environment_keys() -> &'static [OsString] {
    static KEYS: OnceLock<Vec<OsString>> = OnceLock::new();
    KEYS.get_or_init(|| std::env::vars_os().map(|(key, _)| key).collect())
}

const PTY_EXEC_ARGUMENT: &str = "--zz-pty-exec";
const PTY_EXEC_FENCE: RawFd = 3;
static PTY_EXEC_HOST: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
static THP_DISABLED_HERE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "getenv and prctl run in a constructor before the allocator starts and touch no memory the process owns"
)]
pub(super) fn disable_transparent_huge_pages() {
    unsafe {
        let knob = libc::getenv(c"ZZ_PERF_THP".as_ptr());
        if !knob.is_null() && *knob == b'1'.cast_signed() {
            return;
        }
        if libc::prctl(libc::PR_GET_THP_DISABLE, 0, 0, 0, 0) == 0
            && libc::prctl(libc::PR_SET_THP_DISABLE, 1, 0, 0, 0) == 0
        {
            THP_DISABLED_HERE.store(true, Ordering::Relaxed);
        }
    }
}

pub(super) fn run_pty_exec_mode() -> Option<ExitCode> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(OsStr::new(PTY_EXEC_ARGUMENT)) {
        PTY_EXEC_HOST.store(true, Ordering::Release);
        return None;
    }
    let Ok(arguments) = arguments
        .map(|argument| CString::new(argument.into_vec()))
        .collect::<Result<Vec<_>, _>>()
    else {
        return Some(ExitCode::from(127));
    };
    Some(exec_pty_program(&arguments))
}

#[allow(
    unsafe_code,
    reason = "resets signals, claims the controlling terminal and execs, all on memory this function owns"
)]
fn exec_pty_program(arguments: &[CString]) -> ExitCode {
    let Some((program, argv)) = arguments.split_first() else {
        return ExitCode::from(127);
    };
    if argv.is_empty() {
        return ExitCode::from(127);
    }
    let argv_pointers = pointers(argv);
    let mut fallback = vec![CString::from(c"sh"), program.clone()];
    fallback.extend(argv.iter().skip(1).cloned());
    let fallback_pointers = pointers(&fallback);
    unsafe {
        for signal in 1..SIGNAL_LIMIT {
            if signal != libc::SIGKILL && signal != libc::SIGSTOP {
                libc::signal(signal, libc::SIG_DFL);
            }
        }
        let empty: libc::sigset_t = std::mem::zeroed();
        libc::sigprocmask(libc::SIG_SETMASK, &raw const empty, std::ptr::null_mut());
        let _ = rustix::process::ioctl_tiocsctty(std::os::fd::BorrowedFd::borrow_raw(0));
        libc::fcntl(PTY_EXEC_FENCE, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::execv(program.as_ptr(), argv_pointers.as_ptr());
        if io::Error::last_os_error().raw_os_error() == Some(libc::ENOEXEC) {
            libc::execv(c"/bin/sh".as_ptr(), fallback_pointers.as_ptr());
        }
    }
    let error = io::Error::last_os_error();
    eprintln!("zz: cannot run {}: {error}", program.to_string_lossy());
    ExitCode::from(127)
}

#[cfg(target_os = "macos")]
fn pty_exec_helper() -> Option<&'static CString> {
    static HELPER: OnceLock<Option<CString>> = OnceLock::new();
    if !PTY_EXEC_HOST.load(Ordering::Acquire) {
        return None;
    }
    HELPER
        .get_or_init(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| CString::new(path.into_os_string().into_vec()).ok())
        })
        .as_ref()
}

pub(super) struct Spawned {
    pub(super) pid: u32,
    pub(super) exec_fence: OwnedFd,
}

impl Spawned {
    pub(super) fn wait_for_exec(self, limit: Duration) {
        let mut fds = [rustix::event::PollFd::new(
            &self.exec_fence,
            rustix::event::PollFlags::IN | rustix::event::PollFlags::HUP,
        )];
        let deadline = std::time::Instant::now() + limit;
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            let timeout = rustix::event::Timespec::try_from(remaining)
                .expect("a bounded wait fits in a timespec");
            match rustix::event::poll(&mut fds, Some(&timeout)) {
                Err(rustix::io::Errno::INTR) => {}
                Ok(_) | Err(_) => break,
            }
        }
    }
}

fn exec_fence() -> io::Result<(OwnedFd, OwnedFd)> {
    let (read, write) = rustix::pipe::pipe()?;
    rustix::io::fcntl_setfd(&read, FdFlags::CLOEXEC)?;
    rustix::io::fcntl_setfd(&write, FdFlags::CLOEXEC)?;
    Ok((read, write))
}

pub(super) fn spawn(
    builder: &CommandBuilder,
    environment: Vec<(OsString, OsString)>,
    slave: &OwnedFd,
) -> io::Result<Spawned> {
    let plan = ExecPlan::new(builder, environment)?;
    let (exec_fence, fence_write) = exec_fence()?;
    let pointers = ExecPointers {
        argv: pointers(&plan.argv),
        envp: pointers(&plan.envp),
        fallback: pointers(&plan.fallback),
    };
    #[cfg(target_os = "macos")]
    if let Some(helper) = pty_exec_helper() {
        match spawn_through_helper(helper, &plan, slave.as_raw_fd(), fence_write.as_raw_fd()) {
            Ok(pid) => return Ok(Spawned { pid, exec_fence }),
            Err(error) => log::warn!(
                target: "zz_terminal::spawn",
                "pane helper {} failed, forking instead: {error}",
                helper.to_string_lossy()
            ),
        }
    }
    let mut descriptors = DescriptorScratch::new();
    #[allow(
        unsafe_code,
        reason = "the child only makes async-signal-safe calls on memory prepared before the fork, \
                  and every signal stays blocked until it has reset their dispositions"
    )]
    unsafe {
        let mut blocked: libc::sigset_t = std::mem::zeroed();
        let mut previous: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&raw mut blocked);
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const blocked, &raw mut previous);
        let pid = libc::fork();
        if pid == 0 {
            exec_child(
                &plan,
                &pointers,
                slave.as_raw_fd(),
                fence_write.as_raw_fd(),
                &mut descriptors,
            );
        }
        let error = io::Error::last_os_error();
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const previous, std::ptr::null_mut());
        if pid == -1 {
            Err(error)
        } else {
            Ok(Spawned {
                pid: pid.cast_unsigned(),
                exec_fence,
            })
        }
    }
}

#[cfg(target_os = "macos")]
const POSIX_SPAWN_SETSID: libc::c_int = 0x0400;

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "posix_spawn_file_actions_addchdir_np has no binding in the libc crate"
)]
unsafe extern "C" {
    fn posix_spawn_file_actions_addchdir_np(
        actions: *mut libc::posix_spawn_file_actions_t,
        path: *const libc::c_char,
    ) -> libc::c_int;
}

#[cfg(target_os = "macos")]
struct SpawnAttributes(libc::posix_spawnattr_t);

#[cfg(target_os = "macos")]
impl Drop for SpawnAttributes {
    #[allow(unsafe_code, reason = "destroys attributes this value initialized")]
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawnattr_destroy(&raw mut self.0);
        }
    }
}

#[cfg(target_os = "macos")]
struct SpawnActions(libc::posix_spawn_file_actions_t);

#[cfg(target_os = "macos")]
impl Drop for SpawnActions {
    #[allow(unsafe_code, reason = "destroys file actions this value initialized")]
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawn_file_actions_destroy(&raw mut self.0);
        }
    }
}

#[cfg(target_os = "macos")]
fn spawn_result(code: libc::c_int) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "posix_spawn reads argv, envp and the actions from memory that outlives the call"
)]
fn spawn_through_helper(
    helper: &CString,
    plan: &ExecPlan,
    slave: RawFd,
    fence: RawFd,
) -> io::Result<u32> {
    let marker = CString::new(PTY_EXEC_ARGUMENT).expect("the marker has no NUL byte");
    let argv = [helper.as_ptr(), marker.as_ptr(), plan.program.as_ptr()]
        .into_iter()
        .chain(plan.argv.iter().map(|argument| argument.as_ptr()))
        .chain(std::iter::once(std::ptr::null()))
        .collect::<Vec<_>>();
    let envp = pointers(&plan.envp);
    let mut attributes = SpawnAttributes(std::ptr::null_mut());
    spawn_result(unsafe { libc::posix_spawnattr_init(&raw mut attributes.0) })?;
    let flags = libc::POSIX_SPAWN_SETSIGDEF
        | libc::POSIX_SPAWN_SETSIGMASK
        | libc::POSIX_SPAWN_CLOEXEC_DEFAULT
        | POSIX_SPAWN_SETSID;
    spawn_result(unsafe {
        libc::posix_spawnattr_setflags(
            &raw mut attributes.0,
            libc::c_short::try_from(flags).expect("spawn flags fit a short"),
        )
    })?;
    let mut defaults: libc::sigset_t = 0;
    let empty: libc::sigset_t = 0;
    unsafe {
        libc::sigfillset(&raw mut defaults);
    }
    spawn_result(unsafe {
        libc::posix_spawnattr_setsigdefault(&raw mut attributes.0, &raw const defaults)
    })?;
    spawn_result(unsafe {
        libc::posix_spawnattr_setsigmask(&raw mut attributes.0, &raw const empty)
    })?;
    let mut actions = SpawnActions(std::ptr::null_mut());
    spawn_result(unsafe { libc::posix_spawn_file_actions_init(&raw mut actions.0) })?;
    for target in 0..=2 {
        spawn_result(unsafe {
            libc::posix_spawn_file_actions_adddup2(&raw mut actions.0, slave, target)
        })?;
    }
    spawn_result(unsafe {
        libc::posix_spawn_file_actions_adddup2(&raw mut actions.0, fence, PTY_EXEC_FENCE)
    })?;
    spawn_result(unsafe {
        posix_spawn_file_actions_addchdir_np(&raw mut actions.0, plan.directory.as_ptr())
    })?;
    let mut pid: libc::pid_t = 0;
    spawn_result(unsafe {
        libc::posix_spawn(
            &raw mut pid,
            helper.as_ptr(),
            &raw const actions.0,
            &raw const attributes.0,
            argv.as_ptr().cast(),
            envp.as_ptr().cast(),
        )
    })?;
    Ok(pid.cast_unsigned())
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
    unsafe fn close_inherited(&mut self, keep: RawFd) {
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
            if descriptor > 2 && descriptor != keep {
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
    unsafe fn close_inherited(&mut self, keep: RawFd) {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Ok(keep) = u32::try_from(keep)
            && keep >= 3
            && (keep == 3
                || unsafe { libc::syscall(libc::SYS_close_range, 3_u32, keep - 1, 0_u32) } == 0)
            && unsafe { libc::syscall(libc::SYS_close_range, keep + 1, u32::MAX, 0_u32) } == 0
        {
            return;
        }
        for descriptor in 3..self.0 {
            if descriptor != keep {
                unsafe {
                    libc::close(descriptor);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
const SIGNAL_LIMIT: libc::c_int = 32;
#[cfg(not(target_os = "macos"))]
const SIGNAL_LIMIT: libc::c_int = 65;

#[allow(
    unsafe_code,
    reason = "runs in the forked child between fork and exec, on memory prepared before the fork"
)]
unsafe fn exec_child(
    plan: &ExecPlan,
    pointers: &ExecPointers,
    slave: RawFd,
    fence: RawFd,
    descriptors: &mut DescriptorScratch,
) -> ! {
    unsafe {
        for signal in 1..SIGNAL_LIMIT {
            if signal != libc::SIGKILL && signal != libc::SIGSTOP {
                libc::signal(signal, libc::SIG_DFL);
            }
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
        descriptors.close_inherited(fence);
        #[cfg(target_os = "linux")]
        if THP_DISABLED_HERE.load(Ordering::Relaxed) {
            libc::prctl(libc::PR_SET_THP_DISABLE, 0, 0, 0, 0);
        }
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
            errors.push(format!(
                "No viable candidates found in PATH {}",
                path.display()
            ));
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
