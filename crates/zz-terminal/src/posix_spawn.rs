use std::ffi::CStr;
use std::io;
use std::os::fd::RawFd;

const POSIX_SPAWN_SETSID: libc::c_int = 0x0400;

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

struct Attributes(libc::posix_spawnattr_t);

impl Drop for Attributes {
    #[allow(unsafe_code, reason = "destroys attributes this value initialized")]
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawnattr_destroy(&raw mut self.0);
        }
    }
}

struct Actions(libc::posix_spawn_file_actions_t);

impl Drop for Actions {
    #[allow(unsafe_code, reason = "destroys file actions this value initialized")]
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawn_file_actions_destroy(&raw mut self.0);
        }
    }
}

fn check(code: libc::c_int) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

pub struct PosixSpawn {
    attributes: Attributes,
    actions: Actions,
    flags: libc::c_int,
}

#[allow(
    unsafe_code,
    reason = "every call passes attributes and file actions this value initialized"
)]
impl PosixSpawn {
    pub fn new() -> io::Result<Self> {
        let mut attributes = Attributes(std::ptr::null_mut());
        check(unsafe { libc::posix_spawnattr_init(&raw mut attributes.0) })?;
        let mut defaults: libc::sigset_t = 0;
        let empty: libc::sigset_t = 0;
        unsafe {
            libc::sigfillset(&raw mut defaults);
        }
        check(unsafe {
            libc::posix_spawnattr_setsigdefault(&raw mut attributes.0, &raw const defaults)
        })?;
        check(unsafe {
            libc::posix_spawnattr_setsigmask(&raw mut attributes.0, &raw const empty)
        })?;
        let mut actions = Actions(std::ptr::null_mut());
        check(unsafe { libc::posix_spawn_file_actions_init(&raw mut actions.0) })?;
        Ok(Self {
            attributes,
            actions,
            flags: libc::POSIX_SPAWN_SETSIGDEF
                | libc::POSIX_SPAWN_SETSIGMASK
                | libc::POSIX_SPAWN_CLOEXEC_DEFAULT,
        })
    }

    pub fn new_session(&mut self) {
        self.flags |= POSIX_SPAWN_SETSID;
    }

    pub fn process_group(&mut self, group: libc::pid_t) -> io::Result<()> {
        check(unsafe { libc::posix_spawnattr_setpgroup(&raw mut self.attributes.0, group) })?;
        self.flags |= libc::POSIX_SPAWN_SETPGROUP;
        Ok(())
    }

    pub fn dup2(&mut self, fd: RawFd, target: RawFd) -> io::Result<()> {
        check(unsafe {
            libc::posix_spawn_file_actions_adddup2(&raw mut self.actions.0, fd, target)
        })
    }

    pub fn open(&mut self, target: RawFd, path: &CStr, flags: libc::c_int) -> io::Result<()> {
        check(unsafe {
            libc::posix_spawn_file_actions_addopen(
                &raw mut self.actions.0,
                target,
                path.as_ptr(),
                flags,
                0,
            )
        })
    }

    pub fn chdir(&mut self, directory: &CStr) -> io::Result<()> {
        check(unsafe {
            posix_spawn_file_actions_addchdir_np(&raw mut self.actions.0, directory.as_ptr())
        })
    }

    pub fn spawn(
        &mut self,
        program: &CStr,
        argv: &[impl AsRef<CStr>],
        envp: &[impl AsRef<CStr>],
    ) -> io::Result<u32> {
        let argv = pointers(argv);
        let envp = pointers(envp);
        check(unsafe {
            libc::posix_spawnattr_setflags(
                &raw mut self.attributes.0,
                libc::c_short::try_from(self.flags).expect("spawn flags fit a short"),
            )
        })?;
        let mut pid: libc::pid_t = 0;
        check(unsafe {
            libc::posix_spawnp(
                &raw mut pid,
                program.as_ptr(),
                &raw const self.actions.0,
                &raw const self.attributes.0,
                argv.as_ptr().cast(),
                envp.as_ptr().cast(),
            )
        })?;
        Ok(pid.cast_unsigned())
    }
}

fn pointers(values: &[impl AsRef<CStr>]) -> Vec<*const libc::c_char> {
    values
        .iter()
        .map(|value| value.as_ref().as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}
