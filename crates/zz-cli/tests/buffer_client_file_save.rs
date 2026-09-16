//! `save-buffer` writes through the client that asked for it.
//!
//! `file_write` in file.c is `file_read`'s twin: the server expands the path
//! once, then hands the absolute path and the bytes to the invoking client
//! unless that client is attached, so the file lands on the caller's host and
//! `-a` appends there.

#![cfg(unix)]

mod client_file;

use client_file::Daemon;

#[test]
fn a_relative_path_writes_the_clients_own_directory() {
    let daemon = Daemon::start("save-relative");
    std::fs::write(daemon.daemon_directory().join("out.txt"), b"untouched")
        .expect("decoy beside the daemon");
    daemon.set_buffer("saved", "client payload");

    let saved = daemon.run_in_client_directory(&["save-buffer", "-b", "saved", "out.txt"]);
    assert!(saved.status.success(), "{saved:?}");
    assert_eq!(
        std::fs::read_to_string(daemon.client_directory().join("out.txt"))
            .expect("the client's own file"),
        "client payload"
    );
    assert_eq!(
        std::fs::read_to_string(daemon.daemon_directory().join("out.txt"))
            .expect("the daemon's own file"),
        "untouched",
        "the daemon's directory holds a file of the same name and keeps it"
    );
}

#[test]
fn a_second_save_truncates_and_dash_a_appends() {
    let daemon = Daemon::start("save-append");
    let file = daemon.client_directory().join("appended.txt");
    daemon.set_buffer("saved", "first");

    let saved = daemon.run_in_client_directory(&["save-buffer", "-b", "saved", "appended.txt"]);
    assert!(saved.status.success(), "{saved:?}");
    daemon.set_buffer("saved", "second");
    let appended =
        daemon.run_in_client_directory(&["save-buffer", "-a", "-b", "saved", "appended.txt"]);
    assert!(appended.status.success(), "{appended:?}");
    assert_eq!(
        std::fs::read_to_string(&file).expect("appended file"),
        "firstsecond"
    );

    let truncated = daemon.run_in_client_directory(&["save-buffer", "-b", "saved", "appended.txt"]);
    assert!(truncated.status.success(), "{truncated:?}");
    assert_eq!(
        std::fs::read_to_string(&file).expect("truncated file"),
        "second",
        "a save without -a truncates what was there"
    );
}

#[test]
fn a_tilde_path_hangs_from_the_daemons_home() {
    let daemon = Daemon::start("save-tilde");
    daemon.set_buffer("saved", "home payload");

    let saved =
        daemon.run_in_client_directory(&["save-buffer", "-b", "saved", "~/saved-tilde.txt"]);
    assert!(saved.status.success(), "{saved:?}");
    assert_eq!(
        std::fs::read_to_string(daemon.daemon_home().join("saved-tilde.txt"))
            .expect("file in the daemon's home"),
        "home payload"
    );
    assert!(
        !daemon.client_home().join("saved-tilde.txt").exists(),
        "the pin expands a leading ~/ with the server's own find_home"
    );
}

#[test]
fn an_absolute_path_is_written_as_written() {
    let daemon = Daemon::start("save-absolute");
    let file = daemon.daemon_directory().join("absolute.txt");
    daemon.set_buffer("saved", "absolute payload");

    let saved =
        daemon.run_in_client_directory(&["save-buffer", "-b", "saved", &file.to_string_lossy()]);
    assert!(saved.status.success(), "{saved:?}");
    assert_eq!(
        std::fs::read_to_string(&file).expect("absolute file"),
        "absolute payload"
    );
}

#[test]
fn a_directory_that_is_not_there_reports_the_reason_then_the_expanded_path() {
    let daemon = Daemon::start("save-missing");
    daemon.set_buffer("saved", "payload");

    let failed = daemon.run_in_client_directory(&["save-buffer", "-b", "saved", "nodir/out.txt"]);
    assert_eq!(failed.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&failed.stderr).trim_end(),
        format!(
            "No such file or directory: {}",
            daemon.client_directory().join("nodir/out.txt").display()
        ),
        "the pin prints strerror then cf->path"
    );
}
