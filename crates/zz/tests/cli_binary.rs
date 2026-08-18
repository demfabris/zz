use std::process::Command;

const TMUX_USAGE: &str = concat!(
    "usage: zz [-2CDhlNuVv] [-c shell-command] [-f file] [-L socket-name]\n",
    "            [-S socket-path] [-T features] [command [flags]]\n"
);

#[test]
fn tmux_version_is_exact() {
    let output = Command::new(env!("CARGO_BIN_EXE_zz"))
        .args(["-2uV", "ignored"])
        .output()
        .expect("run zz -V");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"tmux 3.8-zz\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn unknown_tmux_flag_uses_tmux_usage_shape() {
    let output = Command::new(env!("CARGO_BIN_EXE_zz"))
        .arg("-8")
        .output()
        .expect("run zz with an unknown tmux flag");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        format!("zz: unknown option -- 8\n{TMUX_USAGE}").as_bytes()
    );
}

#[test]
fn unsupported_control_mode_fails_on_one_line() {
    let output = Command::new(env!("CARGO_BIN_EXE_zz"))
        .arg("-CC")
        .output()
        .expect("run zz control mode rejection");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"zz: -C and -CC control mode are not supported\n"
    );
}

#[cfg(unix)]
mod daemon_autostart {
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
        process::{Command, Output},
        thread,
        time::{Duration, Instant},
    };

    struct Fixture {
        _directory: tempfile::TempDir,
        socket: PathBuf,
        config: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::Builder::new()
                .prefix("zz-cli-")
                .tempdir_in("/tmp")
                .expect("temporary CLI directory");
            let socket = directory.path().join("daemon.sock");
            let config = directory.path().join("empty.conf");
            std::fs::write(&config, b"").expect("empty mux config");
            Self {
                _directory: directory,
                socket,
                config,
            }
        }

        fn command(&self) -> Command {
            let mut command = Command::new(env!("CARGO_BIN_EXE_zz"));
            command
                .arg("-f")
                .arg(&self.config)
                .arg("-S")
                .arg(&self.socket);
            command
        }

        fn run(&self, arguments: &[&str]) -> Output {
            self.command()
                .args(arguments)
                .output()
                .expect("run zz command")
        }

        fn missing_message(&self) -> Vec<u8> {
            format!(
                "error connecting to {} (No such file or directory)\n",
                self.socket.display()
            )
            .into_bytes()
        }

        fn assert_not_started(&self) {
            assert!(!self.socket.exists(), "socket was created");
            assert!(
                !identity_path(&self.socket).exists(),
                "identity was created"
            );
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.run(&["kill-server"]);
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.socket.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn identity_path(socket: &Path) -> PathBuf {
        let mut path = OsString::from(socket.as_os_str());
        path.push(".identity");
        PathBuf::from(path)
    }

    fn local_socket_bind_available(path: &Path) -> bool {
        match std::os::unix::net::UnixListener::bind(path) {
            Ok(listener) => {
                drop(listener);
                std::fs::remove_file(path).expect("remove socket capability probe");
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => false,
            Err(error) => panic!("probe Unix socket binding: {error}"),
        }
    }

    fn assert_missing(output: &Output, expected: &[u8]) {
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, expected);
    }

    #[test]
    fn read_only_command_without_a_daemon_uses_the_pin_connect_error() {
        let fixture = Fixture::new();
        let output = fixture.run(&["list-sessions"]);
        assert_missing(&output, &fixture.missing_message());
        fixture.assert_not_started();
    }

    #[test]
    fn new_session_immediately_after_kill_server_starts_a_fresh_daemon() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let first = fixture.run(&["new-session", "-d", "-s", "a"]);
        assert_eq!(first.status.code(), Some(0));
        let killed = fixture.run(&["kill-server"]);
        assert_eq!(killed.status.code(), Some(0));
        let second = fixture.run(&["new-session", "-d", "-s", "b"]);
        assert_eq!(
            second.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&second.stderr)
        );
        let sessions = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(sessions.status.code(), Some(0));
        assert_eq!(sessions.stdout, b"b\n");
    }

    #[test]
    fn non_start_server_commands_do_not_spawn_a_daemon() {
        let fixture = Fixture::new();
        for command in [
            "ls",
            "list-panes",
            "show-options",
            "source-file",
            "kill-server",
        ] {
            let output = fixture.run(&[command]);
            assert_missing(&output, &fixture.missing_message());
            fixture.assert_not_started();
        }
    }

    #[test]
    fn detached_new_session_still_autostarts_the_daemon() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "autostart"]);
        assert_eq!(created.status.code(), Some(0));
        assert!(created.stdout.is_empty());
        assert!(created.stderr.is_empty());
        assert!(fixture.socket.exists());

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"autostart\n");
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn native_attach_accepts_the_tmux_target_for_both_spellings() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "named"]);
        assert_eq!(created.status.code(), Some(0));
        for command in ["attach", "attach-session"] {
            let output = fixture.run(&[command, "-t", "named"]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(
                output.stderr,
                b"zz attach: attach requires an interactive terminal\n"
            );
        }
    }

    #[test]
    fn headless_attach_resolves_the_target_before_rejecting_the_terminal() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "named"]);
        assert_eq!(created.status.code(), Some(0));
        for command in ["attach", "attach-session"] {
            let output = fixture.run(&[command, "-t", "bogus"]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, b"zz attach: can't find session: bogus\n");
        }
    }

    #[test]
    fn headless_attach_autostarts_before_reporting_no_sessions() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        std::fs::write(&fixture.config, b"set-environment -g started YES\n")
            .expect("write autostart marker config");
        let output = fixture.run(&["attach"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"zz attach: no sessions\n");
        assert!(fixture.socket.exists());

        let started = fixture.run(&["show-environment", "-g", "started"]);
        assert_eq!(started.status.code(), Some(0));
        assert_eq!(started.stdout, b"started=YES\n");
        assert!(started.stderr.is_empty());
    }

    #[test]
    fn native_attach_rejections_match_the_engine() {
        let fixture = Fixture::new();
        for args in [
            &["-r"][..],
            &["-x"][..],
            &["-E"][..],
            &["-c", "/tmp"][..],
            &["-f", "flags"][..],
        ] {
            let invocation =
                zz_protocol::CommandInvocation::new("attach-session", args.iter().copied());
            let error = zz_mux::MuxEngine::default()
                .execute(&mut zz_mux::ExecutionContext::default(), &invocation)
                .expect_err("engine rejects the unsupported attach option");
            let expected = format!("zz: mux command failed: {error}\n");
            for command in ["attach", "attach-session"] {
                let output = fixture
                    .command()
                    .arg(command)
                    .args(args)
                    .output()
                    .expect("run native attach rejection");
                assert_eq!(output.status.code(), Some(1));
                assert!(output.stdout.is_empty());
                assert_eq!(output.stderr, expected.as_bytes());
            }
        }
    }

    #[test]
    fn ls_or_new_session_idiom_starts_one_fresh_daemon() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(r#""$1" -f "$2" -S "$3" ls || "$1" -f "$2" -S "$3" new-session -d"#)
            .arg("zz-ls-or-new")
            .arg(env!("CARGO_BIN_EXE_zz"))
            .arg(&fixture.config)
            .arg(&fixture.socket)
            .output()
            .expect("run ls-or-new-session idiom");
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, fixture.missing_message());
        assert!(fixture.socket.exists());

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"0\n");
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn no_start_server_suppresses_new_session_autostart() {
        let fixture = Fixture::new();
        let output = fixture.run(&["-N", "new-session", "-d"]);
        assert_missing(&output, &fixture.missing_message());
        fixture.assert_not_started();
    }

    #[test]
    fn has_session_on_a_fresh_socket_leaves_no_daemon() {
        let fixture = Fixture::new();
        let output = fixture.run(&["has-session"]);
        assert_missing(&output, &fixture.missing_message());
        fixture.assert_not_started();
    }

    #[test]
    fn slash_label_never_creates_intermediate_directories() {
        let directory = tempfile::Builder::new()
            .prefix("zz-label-")
            .tempdir_in("/tmp")
            .expect("temporary label directory");
        let root = std::fs::canonicalize(directory.path()).expect("canonical label root");
        let config = directory.path().join("empty.conf");
        std::fs::write(&config, b"").expect("empty mux config");
        let base = root.join(format!("tmux-{}", rustix::process::getuid().as_raw()));
        let socket = base.join("a/b");
        let run = |label: &str, arguments: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_zz"))
                .env("TMUX_TMPDIR", directory.path())
                .arg("-f")
                .arg(&config)
                .args(["-L", label])
                .args(arguments)
                .output()
                .expect("run slash-label command")
        };

        let listed = run("a/b", &["list-sessions"]);
        assert_missing(
            &listed,
            format!(
                "error connecting to {} (No such file or directory)\n",
                socket.display()
            )
            .as_bytes(),
        );
        assert!(base.is_dir());
        assert!(!base.join("a").exists());

        let created = run("a/b", &["new-session", "-d"]);
        assert_eq!(created.status.code(), Some(0));
        assert!(created.stdout.is_empty());
        assert_eq!(
            created.stderr,
            format!(
                "error creating {} (No such file or directory)\n",
                socket.display()
            )
            .as_bytes()
        );
        assert!(!base.join("a").exists());
        assert!(!socket.exists());
        assert!(!identity_path(&socket).exists());

        let file = base.join("file");
        std::fs::write(&file, b"").expect("non-directory label parent");
        let not_a_directory = run("file/socket", &["new-session", "-d"]);
        assert_eq!(not_a_directory.status.code(), Some(1));
        assert!(not_a_directory.stdout.is_empty());
        assert_eq!(
            not_a_directory.stderr,
            format!(
                "error connecting to {} (Not a directory)\n",
                file.join("socket").display()
            )
            .as_bytes()
        );
    }
}
