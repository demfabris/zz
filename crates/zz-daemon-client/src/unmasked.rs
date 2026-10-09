use std::{
    io,
    process::{Child, Command, ExitStatus, Output},
};

pub trait SpawnUnmasked {
    fn spawn_unmasked(&mut self) -> io::Result<Child>;
    fn output_unmasked(&mut self) -> io::Result<Output>;
    fn status_unmasked(&mut self) -> io::Result<ExitStatus>;
}

#[allow(
    clippy::disallowed_methods,
    reason = "the one place that spawns a Command, with the calling thread's signal mask cleared"
)]
impl SpawnUnmasked for Command {
    fn spawn_unmasked(&mut self) -> io::Result<Child> {
        unmasked(|| self.spawn())
    }

    fn output_unmasked(&mut self) -> io::Result<Output> {
        unmasked(|| self.output())
    }

    fn status_unmasked(&mut self) -> io::Result<ExitStatus> {
        self.spawn_unmasked()?.wait()
    }
}

#[cfg(unix)]
struct RestoreMask(libc::sigset_t);

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "restore the mask this thread had before unmasked"
)]
impl Drop for RestoreMask {
    fn drop(&mut self) {
        unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &raw const self.0, std::ptr::null_mut());
        }
    }
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "pthread_sigmask on the calling thread with masks this function owns"
)]
fn unmasked<T>(spawn: impl FnOnce() -> T) -> T {
    let _restore = unsafe {
        let mut empty: libc::sigset_t = std::mem::zeroed();
        let mut previous: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&raw mut empty);
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const empty, &raw mut previous);
        RestoreMask(previous)
    };
    spawn()
}

#[cfg(not(unix))]
fn unmasked<T>(spawn: impl FnOnce() -> T) -> T {
    spawn()
}

#[cfg(all(test, unix))]
mod tests {
    use std::{os::unix::process::CommandExt as _, process::Command};

    use super::SpawnUnmasked as _;

    const PROBE: &str = "ZZ_SIGNAL_MASK_PROBE";
    const PROBE_TEST: &str =
        "unmasked::tests::children_spawned_from_a_masked_thread_start_with_nothing_blocked";
    const MARKER: &str = "blocked-signals:";

    #[allow(unsafe_code, reason = "read this thread's signal mask")]
    fn blocked_signals() -> Vec<libc::c_int> {
        let mut current: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &raw mut current);
        }
        (1..32)
            .filter(|&signal| unsafe { libc::sigismember(&raw const current, signal) } == 1)
            .collect()
    }

    #[allow(unsafe_code, reason = "set this test thread's signal mask")]
    fn mask(signals: &[libc::c_int]) {
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&raw mut set);
            for &signal in signals {
                libc::sigaddset(&raw mut set, signal);
            }
            libc::pthread_sigmask(libc::SIG_SETMASK, &raw const set, std::ptr::null_mut());
        }
    }

    fn probe_command(fork: bool) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("test binary"));
        command
            .args([PROBE_TEST, "--exact", "--nocapture", "--test-threads=1"])
            .env(PROBE, "1");
        if fork {
            #[allow(
                unsafe_code,
                reason = "an empty pre_exec hook moves std onto fork and exec"
            )]
            unsafe {
                command.pre_exec(|| Ok(()));
            }
        }
        command
    }

    fn reported(output: &std::process::Output) -> Vec<libc::c_int> {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "probe failed: {stdout}");
        let line = stdout
            .lines()
            .find_map(|line| line.split_once(MARKER).map(|(_, signals)| signals))
            .unwrap_or_else(|| panic!("probe printed no mask: {stdout}"));
        line.split_whitespace()
            .map(|signal| signal.parse().expect("signal number"))
            .collect()
    }

    #[test]
    fn children_spawned_from_a_masked_thread_start_with_nothing_blocked() {
        if std::env::var_os(PROBE).is_some() {
            let signals = blocked_signals()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            println!("{MARKER}{}", signals.join(" "));
            return;
        }
        let masked = [libc::SIGINT, libc::SIGTERM, libc::SIGCHLD, libc::SIGWINCH];
        let mut expected = masked.to_vec();
        expected.sort_unstable();
        std::thread::spawn(move || {
            mask(&masked);
            assert_eq!(blocked_signals(), expected);
            for fork in [false, true] {
                let bypassed = probe_command(fork).output().expect("plain probe");
                assert_eq!(reported(&bypassed), expected, "fork={fork}");
                let routed = probe_command(fork).output_unmasked().expect("probe");
                assert_eq!(reported(&routed), Vec::<libc::c_int>::new(), "fork={fork}");
                let spawned = probe_command(fork)
                    .stdout(std::process::Stdio::piped())
                    .spawn_unmasked()
                    .expect("spawned probe")
                    .wait_with_output()
                    .expect("probe exit");
                assert_eq!(reported(&spawned), Vec::<libc::c_int>::new(), "fork={fork}");
                assert_eq!(blocked_signals(), expected);
            }
        })
        .join()
        .expect("masked thread");
    }
}
