#![allow(unsafe_code)]

use std::{
    ffi::{CStr, CString, c_char, c_int},
    io::{BufRead, BufReader},
    os::fd::FromRawFd,
};

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

const TAG: &CStr = c"zz";

struct Logcat;

impl log::Log for Logcat {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let priority = match record.level() {
            log::Level::Error => 6,
            log::Level::Warn => 5,
            log::Level::Info => 4,
            log::Level::Debug => 3,
            log::Level::Trace => 2,
        };
        write(priority, &format!("{}: {}", record.target(), record.args()));
    }

    fn flush(&self) {}
}

fn write(priority: c_int, text: &str) {
    let Ok(text) = CString::new(text.replace('\0', "")) else {
        return;
    };
    unsafe { __android_log_write(priority, TAG.as_ptr(), text.as_ptr()) };
}

/// Sends `log` records, and everything written to stdout and stderr (panic
/// messages included), to logcat under the `zz` tag.
pub fn init() {
    if log::set_logger(&Logcat).is_err() {
        return;
    }
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|level| level.parse().ok())
        .unwrap_or(log::LevelFilter::Info);
    log::set_max_level(level);
    let mut fds = [0; 2];
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        libc::dup2(fds[1], libc::STDOUT_FILENO);
        libc::dup2(fds[1], libc::STDERR_FILENO);
        libc::close(fds[1]);
    }
    let reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    std::thread::Builder::new()
        .name("logcat".into())
        .spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                write(4, &line);
            }
        })
        .ok();
}
