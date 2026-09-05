//! `load-buffer` reads through the client that asked for it.
//!
//! The pin expands the path once in the server — a leading `~/` against the
//! server's own home, anything else relative against the invoking client's
//! working directory — and then hands the absolute path to that client
//! (`file_get_path` and `file_read` in file.c), because a command client may
//! not share a filesystem with the server at all.

#![cfg(unix)]

mod client_file;

use client_file::Daemon;

#[test]
fn a_relative_path_reads_the_clients_own_directory() {
    let daemon = Daemon::start("load-relative");
    std::fs::write(daemon.daemon_directory().join("shared.txt"), b"daemon copy")
        .expect("decoy beside the daemon");
    std::fs::write(daemon.client_directory().join("shared.txt"), b"client copy")
        .expect("the file the client can see");

    let loaded = daemon.run_in_client_directory(&["load-buffer", "-b", "relative", "shared.txt"]);
    assert!(loaded.status.success(), "{loaded:?}");
    assert_eq!(
        daemon.buffer("relative"),
        "client copy",
        "the daemon's own directory holds a different file of the same name"
    );
}

#[test]
fn a_nested_relative_path_hangs_from_the_client_directory() {
    let daemon = Daemon::start("load-nested");
    let nested = daemon.client_directory().join("nested");
    std::fs::create_dir_all(&nested).expect("nested client directory");
    std::fs::write(nested.join("deep.txt"), b"nested payload").expect("nested file");

    let loaded =
        daemon.run_in_client_directory(&["load-buffer", "-b", "nested", "nested/deep.txt"]);
    assert!(loaded.status.success(), "{loaded:?}");
    assert_eq!(daemon.buffer("nested"), "nested payload");
}

#[test]
fn a_tilde_path_hangs_from_the_daemons_home() {
    let daemon = Daemon::start("load-tilde");
    std::fs::write(
        daemon.daemon_home().join("tilde.txt"),
        b"daemon home payload",
    )
    .expect("file in the daemon's home");
    std::fs::write(
        daemon.client_home().join("tilde.txt"),
        b"client home payload",
    )
    .expect("decoy in the client's home");

    let loaded = daemon.run_in_client_directory(&["load-buffer", "-b", "tilde", "~/tilde.txt"]);
    assert!(loaded.status.success(), "{loaded:?}");
    assert_eq!(
        daemon.buffer("tilde"),
        "daemon home payload",
        "the pin expands a leading ~/ with the server's own find_home"
    );
}

#[test]
fn an_absolute_path_is_read_as_written() {
    let daemon = Daemon::start("load-absolute");
    let file = daemon.daemon_directory().join("absolute.txt");
    std::fs::write(&file, b"absolute payload").expect("absolute file");

    let loaded =
        daemon.run_in_client_directory(&["load-buffer", "-b", "absolute", &file.to_string_lossy()]);
    assert!(loaded.status.success(), "{loaded:?}");
    assert_eq!(daemon.buffer("absolute"), "absolute payload");
}

#[test]
fn a_missing_file_reports_the_reason_then_the_expanded_path() {
    let daemon = Daemon::start("load-missing");
    let missing = daemon.run_in_client_directory(&["load-buffer", "-b", "missing", "nope.txt"]);
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&missing.stderr).trim_end(),
        format!(
            "No such file or directory: {}",
            daemon.client_directory().join("nope.txt").display()
        ),
        "the pin prints strerror then cf->path"
    );
}

#[test]
fn a_read_that_fails_after_the_open_reports_the_pins_io_error() {
    let daemon = Daemon::start("load-directory");
    let directory = daemon.client_directory().join("adirectory");
    std::fs::create_dir_all(&directory).expect("a directory to read");

    let failed = daemon.run_in_client_directory(&["load-buffer", "-b", "dir", "adirectory"]);
    assert_eq!(failed.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&failed.stderr).trim_end(),
        format!("Input/output error: {}", directory.display()),
        "the pin's client opens first and reports EIO for anything after that"
    );
}
