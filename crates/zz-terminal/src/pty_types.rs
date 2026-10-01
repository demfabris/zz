use std::collections::BTreeMap;
use std::ffi::{CStr, OsStr, OsString};
use std::os::unix::process::ExitStatusExt;

#[derive(Clone, Debug)]
pub(crate) struct CommandBuilder {
    argv: Vec<OsString>,
    environment: BTreeMap<OsString, OsString>,
    cwd: Option<OsString>,
}

impl CommandBuilder {
    pub(crate) fn new(program: impl AsRef<OsStr>) -> Self {
        let mut command = Self::new_default_prog();
        command.argv.push(program.as_ref().to_owned());
        command
    }

    pub(crate) fn new_default_prog() -> Self {
        let mut environment = std::env::vars_os().collect::<BTreeMap<_, _>>();
        environment
            .entry(OsString::from("SHELL"))
            .or_insert_with(|| passwd_shell().into());
        Self {
            argv: Vec::new(),
            environment,
            cwd: None,
        }
    }

    pub(crate) fn is_default_prog(&self) -> bool {
        self.argv.is_empty()
    }

    pub(crate) fn args<I, S>(&mut self, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            assert!(
                !self.is_default_prog(),
                "attempted to add args to a default_prog builder"
            );
            self.argv.push(arg.as_ref().to_owned());
        }
    }

    pub(crate) fn get_argv(&self) -> &Vec<OsString> {
        &self.argv
    }

    pub(crate) fn get_argv_mut(&mut self) -> &mut Vec<OsString> {
        &mut self.argv
    }

    pub(crate) fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
        self.environment
            .insert(key.as_ref().to_owned(), value.as_ref().to_owned());
    }

    pub(crate) fn env_remove(&mut self, key: impl AsRef<OsStr>) {
        self.environment.remove(key.as_ref());
    }

    pub(crate) fn get_env(&self, key: impl AsRef<OsStr>) -> Option<&OsStr> {
        self.environment.get(key.as_ref()).map(OsString::as_os_str)
    }

    pub(crate) fn iter_full_env_as_str(&self) -> impl Iterator<Item = (&str, &str)> {
        self.environment
            .iter()
            .filter_map(|(key, value)| Some((key.to_str()?, value.to_str()?)))
    }

    pub(crate) fn cwd(&mut self, directory: impl AsRef<OsStr>) {
        self.cwd = Some(directory.as_ref().to_owned());
    }

    pub(crate) fn get_cwd(&self) -> Option<&OsString> {
        self.cwd.as_ref()
    }

    pub(crate) fn get_shell(&self) -> String {
        if let Some(shell) = self.get_env("SHELL").and_then(OsStr::to_str) {
            match rustix::fs::access(shell, rustix::fs::Access::EXEC_OK) {
                Ok(()) => return shell.to_owned(),
                Err(error) => log::warn!(
                    "$SHELL -> {shell:?} which is not executable ({error:#}), falling back to password db lookup"
                ),
            }
        }
        passwd_shell()
    }
}

#[allow(
    unsafe_code,
    reason = "the passwd entry is read while its C strings are valid"
)]
fn passwd_shell() -> String {
    let entry = unsafe { libc::getpwuid(libc::getuid()) };
    if !entry.is_null() {
        let shell = unsafe { CStr::from_ptr((*entry).pw_shell) };
        match shell.to_str() {
            Ok(shell) => match rustix::fs::access(shell, rustix::fs::Access::EXEC_OK) {
                Ok(()) => return shell.to_owned(),
                Err(error) => log::warn!(
                    "passwd database shell={shell:?} which is not executable ({error:#}), falling back to /bin/sh"
                ),
            },
            Err(error) => log::warn!(
                "passwd database shell could not be represented as utf-8: {error:#}, falling back to /bin/sh"
            ),
        }
    }
    "/bin/sh".to_owned()
}

#[derive(Clone, Debug)]
pub(crate) struct ExitStatus {
    code: u32,
    signal: Option<String>,
}

impl ExitStatus {
    pub(crate) fn exit_code(&self) -> u32 {
        self.code
    }

    pub(crate) fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }
}

impl From<std::process::ExitStatus> for ExitStatus {
    #[allow(
        unsafe_code,
        reason = "strsignal returns a C string read before the next call"
    )]
    fn from(status: std::process::ExitStatus) -> Self {
        let signal = status.signal().map(|signal| {
            let name = unsafe { libc::strsignal(signal) };
            if name.is_null() {
                format!("Signal {signal}")
            } else {
                unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .into_owned()
            }
        });
        Self {
            code: status
                .code()
                .map_or_else(|| u32::from(!status.success()), |code| code as u32),
            signal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PtySize {
    pub(crate) rows: u16,
    pub(crate) cols: u16,
    pub(crate) pixel_width: u16,
    pub(crate) pixel_height: u16,
}
