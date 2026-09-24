use std::{
    io::{self, Read},
    os::{fd::IntoRawFd, unix::net::UnixStream},
    sync::atomic::{AtomicI32, Ordering},
    thread,
    time::Duration,
};

use gpui::App;

const QUIT_SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];
const FORCED_EXIT_AFTER: Duration = Duration::from_secs(2);

static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

pub(crate) fn init(cx: &mut App) {
    let (mut reader, writer) = match UnixStream::pair() {
        Ok(pair) => pair,
        Err(error) => {
            log::error!("could not create the quit signal socket pair: {error}");
            return;
        }
    };
    let (requests, requested) = async_channel::bounded(1);
    if let Err(error) = thread::Builder::new()
        .name("zz-quit-signal".to_owned())
        .spawn(move || {
            let mut signal = [0];
            if reader.read_exact(&mut signal).is_err() {
                return;
            }
            let _ = requests.send_blocking(signal[0]);
            thread::sleep(FORCED_EXIT_AFTER);
            force_exit(signal[0]);
        })
    {
        log::error!("could not start the quit signal thread: {error}");
        return;
    }
    WAKE_FD.store(writer.into_raw_fd(), Ordering::Relaxed);
    cx.spawn(async move |cx| {
        if let Ok(signal) = requested.recv().await {
            log::info!(target: "zz::diagnostics::lifecycle", "quit requested by signal {signal}");
            cx.update(|cx| cx.quit());
        }
    })
    .detach();
    reclaim();
}

#[allow(unsafe_code, reason = "sigaction has no safe binding")]
pub(crate) fn reclaim() {
    if WAKE_FD.load(Ordering::Relaxed) < 0 {
        return;
    }
    for signal in QUIT_SIGNALS {
        let installed = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = request_quit as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&raw mut action.sa_mask);
            libc::sigaction(signal, &raw const action, std::ptr::null_mut()) == 0
        };
        if !installed {
            log::warn!(
                "could not handle signal {signal}: {}",
                io::Error::last_os_error()
            );
        }
    }
}

#[allow(
    unsafe_code,
    reason = "a signal handler may only make async-signal-safe calls"
)]
extern "C" fn request_quit(signal: libc::c_int) {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = libc::SIG_DFL;
        libc::sigaction(signal, &raw const action, std::ptr::null_mut());
        let byte = u8::try_from(signal).unwrap_or(u8::MAX);
        libc::write(WAKE_FD.load(Ordering::Relaxed), (&raw const byte).cast(), 1);
    }
}

#[allow(
    unsafe_code,
    reason = "_exit skips atexit handlers that a stuck shutdown may be holding"
)]
fn force_exit(signal: u8) -> ! {
    unsafe { libc::_exit(128 + i32::from(signal)) }
}
