#![cfg(all(unix, feature = "daemon"))]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use zz_daemon::{CommandClient, Daemon};
use zz_protocol::CommandInvocation;

fn connect(socket: &Path) -> CommandClient {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match CommandClient::connect(socket) {
            Ok(client) => return client,
            Err(error) if Instant::now() >= deadline => {
                panic!("daemon never accepted a client: {error}")
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn send_sigterm() {
    rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::TERM)
        .expect("send SIGTERM to this process");
}

#[cfg(target_os = "linux")]
fn signal_listener_running() -> bool {
    fs::read_dir("/proc/self/task")
        .expect("list this process's threads")
        .flatten()
        .filter_map(|task| fs::read_to_string(task.path().join("comm")).ok())
        .any(|name| name.starts_with("zz-daemon-sig"))
}

#[test]
fn sigterm_stops_a_daemon_held_open_by_a_foreground_job() {
    let id = std::process::id();
    let socket = PathBuf::from(format!("/tmp/zz-sigterm-{id}.sock"));
    let started = PathBuf::from(format!("/tmp/zz-sigterm-{id}.started"));
    let _ = fs::remove_file(&socket);
    let _ = fs::remove_file(&started);

    let daemon = Daemon::new(&socket).without_user_config();
    let (stopped_sender, stopped) = mpsc::channel();
    thread::spawn(move || {
        let _ = stopped_sender.send(daemon.run_foreground());
    });
    let mut client = connect(&socket);
    let command = format!("printf started > {}; sleep 30", started.display());
    let job = thread::spawn(move || client.execute(CommandInvocation::new("run-shell", [command])));
    wait_for_file(&started);

    let signalled = Instant::now();
    send_sigterm();
    assert!(
        stopped.recv_timeout(Duration::from_millis(300)).is_err(),
        "the first SIGTERM waits for the foreground job during its grace"
    );
    #[cfg(target_os = "linux")]
    assert!(
        signal_listener_running(),
        "the signal listener keeps running after its first signal"
    );

    stopped
        .recv_timeout(Duration::from_secs(15))
        .expect("the daemon stopped")
        .expect("the daemon stopped cleanly");
    assert!(signalled.elapsed() < Duration::from_secs(15));
    assert!(
        job.join().expect("run-shell client").is_err(),
        "the stopped job reports failure"
    );

    let _ = fs::remove_file(&started);
    let _ = fs::remove_file(&socket);
}
