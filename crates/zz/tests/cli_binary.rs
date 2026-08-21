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

#[cfg(unix)]
#[test]
fn tmux_without_a_zz_socket_fails_before_dialing_a_foreign_server() {
    let output = Command::new(env!("CARGO_BIN_EXE_zz"))
        .env("TMUX", "/tmp/real-tmux.sock,123,0")
        .env_remove("ZZ_SOCKET")
        .arg("list-sessions")
        .output()
        .expect("run zz inside a foreign tmux environment");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"zz: TMUX is set but ZZ_SOCKET is not; refusing to treat a tmux server as zz\nUse `zz app` to open the GUI, or pass `-S` / set `ZZ_SOCKET` to target a zz daemon.\n"
    );
}

#[cfg(unix)]
mod daemon_autostart {
    use std::{
        ffi::OsString,
        fs::File,
        io::{self, Read as _, Write as _},
        os::fd::FromRawFd as _,
        path::{Path, PathBuf},
        process::{Child, ChildStdin, Command, Output, Stdio},
        sync::mpsc,
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

        fn run_with_stdin(&self, arguments: &[&str], input: &[u8]) -> Output {
            let mut child = self
                .command()
                .args(arguments)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn zz command");
            let mut stdin = child.stdin.take().expect("piped stdin");
            stdin.write_all(input).expect("write command stdin");
            drop(stdin);
            child.wait_with_output().expect("wait for zz command")
        }

        fn spawn_with_open_stdin(&self, arguments: &[&str]) -> (Child, ChildStdin) {
            let mut child = self
                .command()
                .args(arguments)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn zz command");
            let stdin = child.stdin.take().expect("piped stdin");
            (child, stdin)
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

    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn open_pty() -> io::Result<(File, File)> {
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut size,
            )
        } == -1
        {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) })
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
    fn startup_config_new_session_is_forced_detached() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        std::fs::write(
            &fixture.config,
            b"new-session -s fromconfig\nnew-session -A -s fromconfig\nattach-session -t bogus\nnew-session -d -s after-clientless-attach\nnew-session -d -s configured-extra\nif-shell -F 1 'new-session -s fromconditional'\n",
        )
        .expect("write new-session startup config");

        let created = fixture.run(&["new-session", "-d", "-s", "command-extra"]);
        assert_eq!(created.status.code(), Some(0));
        assert!(created.stdout.is_empty());
        assert!(created.stderr.is_empty());

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(
            listed.stdout,
            b"after-clientless-attach\ncommand-extra\nconfigured-extra\nfromconditional\nfromconfig\n"
        );
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn event_and_command_hooks_create_sessions_without_a_client_terminal() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        std::fs::write(
            &fixture.config,
            b"set-hook -g session-created 'new-session -s fromevent'\nset-hook -g after-new-session 'new-session -s fromcommand'\n",
        )
        .expect("write clientless hook config");

        let created = fixture.run(&["new-session", "-d", "-s", "base"]);
        assert_eq!(created.status.code(), Some(0));
        assert!(created.stdout.is_empty());
        assert!(created.stderr.is_empty());

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"base\nfromcommand\nfromevent\n");
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn switch_client_from_a_plain_shell_reports_the_pinned_client_errors() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "w"]);
        assert_eq!(created.status.code(), Some(0));

        let clientless = fixture.run(&["switch-client", "-t", "w"]);
        assert_eq!(clientless.status.code(), Some(1));
        assert_eq!(clientless.stderr, b"no current client\n");

        let unknown = fixture.run(&["switch-client", "-c", "bogus:", "-t", "w"]);
        assert_eq!(unknown.status.code(), Some(1));
        assert_eq!(unknown.stderr, b"can't find client: bogus\n");
    }

    #[test]
    fn runtime_config_commands_keep_the_command_client_terminal_state() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let started = fixture.run(&["start-server"]);
        assert_eq!(started.status.code(), Some(0));
        let runtime_config = fixture.config.with_file_name("runtime.conf");
        std::fs::write(&runtime_config, b"new-session -s sourced\n")
            .expect("write runtime source config");

        let _ = fixture.run(&[
            "source-file",
            runtime_config.to_str().expect("UTF-8 runtime config path"),
        ]);

        let conditional = fixture.run(&["if-shell", "-F", "1", "new-session -s conditional"]);
        assert_eq!(conditional.status.code(), Some(1));
        assert_eq!(
            conditional.stderr,
            b"open terminal failed: not a terminal\n"
        );

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert!(listed.stdout.is_empty());
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn detached_new_session_accepts_dash_dimensions() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }

        let created = fixture.run(&["new-session", "-d", "-s", "dash-size", "-x", "-", "-y", "-"]);
        assert_eq!(created.status.code(), Some(0));
        assert!(created.stdout.is_empty());
        assert!(created.stderr.is_empty());

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"dash-size\n");
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn headless_attaching_new_session_fails_without_creating_the_session() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }

        let created = fixture.run(&["new-session", "-s", "headless"]);
        assert_eq!(created.status.code(), Some(1));
        assert!(created.stdout.is_empty());
        assert_eq!(created.stderr, b"open terminal failed: not a terminal\n");

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert!(listed.stdout.is_empty());
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn tty_new_session_error_is_bare_and_does_not_start_the_browser() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let Ok((mut master, slave)) = open_pty() else {
            return;
        };
        rustix::io::ioctl_fionbio(&master, true).expect("set pty master nonblocking");
        let stdin = slave.try_clone().expect("clone pty stdin");
        let mut child = fixture
            .command()
            .args(["new-session", "-s", "tty-width", "-x", "0"])
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(slave))
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn TTY new-session error");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stdout = Vec::new();
        let status = loop {
            let mut buffer = [0_u8; 4096];
            match master.read(&mut buffer) {
                Ok(0) => {}
                Ok(count) => stdout.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    drop(master);
                    panic!("poll TTY new-session error: {error}");
                }
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                drop(master);
                panic!("TTY new-session error did not exit before deadline");
            }
            thread::sleep(Duration::from_millis(5));
        };
        drop(master);
        let mut stderr = Vec::new();
        child
            .stderr
            .take()
            .expect("piped TTY error stderr")
            .read_to_end(&mut stderr)
            .expect("read TTY error stderr");

        assert_eq!(status.code(), Some(1));
        assert!(stdout.is_empty());
        assert_eq!(stderr, b"width too small\n");

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert!(listed.stdout.is_empty());
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn duplicate_new_session_error_is_exact_and_precedes_the_terminal_check() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }

        let first = fixture.run(&["new-session", "-d", "-s", "dup"]);
        assert_eq!(first.status.code(), Some(0));

        let detached_duplicate = fixture.run(&["new-session", "-d", "-s", "dup"]);
        assert_eq!(detached_duplicate.status.code(), Some(1));
        assert!(detached_duplicate.stdout.is_empty());
        assert_eq!(detached_duplicate.stderr, b"duplicate session: dup\n");

        let attaching_duplicate = fixture.run(&["new-session", "-s", "dup"]);
        assert_eq!(attaching_duplicate.status.code(), Some(1));
        assert!(attaching_duplicate.stdout.is_empty());
        assert_eq!(attaching_duplicate.stderr, b"duplicate session: dup\n");
    }

    #[test]
    fn headless_dash_a_ignores_detach_for_existing_but_not_new_sessions() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }

        let existing = fixture.run(&["new-session", "-d", "-s", "existing"]);
        assert_eq!(existing.status.code(), Some(0));

        let attaching = fixture.run(&["new-session", "-A", "-d", "-s", "existing"]);
        assert_eq!(attaching.status.code(), Some(1));
        assert!(attaching.stdout.is_empty());
        assert_eq!(attaching.stderr, b"open terminal failed: not a terminal\n");

        let fresh = fixture.run(&["new-session", "-A", "-d", "-s", "fresh"]);
        assert_eq!(fresh.status.code(), Some(0));
        assert!(fresh.stdout.is_empty());
        assert!(fresh.stderr.is_empty());

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"existing\nfresh\n");
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn detached_new_session_prints_default_and_custom_formats() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }

        let default = fixture.run(&["new-session", "-P", "-d", "-s", "printed"]);
        assert_eq!(default.status.code(), Some(0));
        assert_eq!(default.stdout, b"printed:\n");
        assert!(default.stderr.is_empty());

        let formatted = fixture.run(&[
            "new-session",
            "-P",
            "-d",
            "-F",
            "#{session_name}/#{window_index}",
            "-s",
            "formatted",
        ]);
        assert_eq!(formatted.status.code(), Some(0));
        assert_eq!(formatted.stdout, b"formatted/0\n");
        assert!(formatted.stderr.is_empty());

        let empty = fixture.run(&["new-window", "-d", "-t", "formatted:", "-P", "-F", ""]);
        assert_eq!(empty.status.code(), Some(0));
        assert_eq!(empty.stdout, b"\n");
        assert!(empty.stderr.is_empty());

        let trailing_newline =
            fixture.run(&["new-window", "-d", "-t", "formatted:", "-P", "-F", "X\n"]);
        assert_eq!(trailing_newline.status.code(), Some(0));
        assert_eq!(trailing_newline.stdout, b"X\n\n");
        assert!(trailing_newline.stderr.is_empty());

        let split = fixture.run(&["split-window", "-d", "-t", "formatted:0"]);
        assert_eq!(split.status.code(), Some(0));
        assert!(split.stdout.is_empty());
        assert!(split.stderr.is_empty());

        let broken = fixture.run(&["break-pane", "-d", "-P", "-s", "formatted:0.1", "-F", "X\n"]);
        assert_eq!(broken.status.code(), Some(0));
        assert_eq!(broken.stdout, b"X\n\n");
        assert!(broken.stderr.is_empty());
    }

    #[test]
    fn attaching_new_session_later_in_a_chain_enters_the_alternate_screen() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let Ok((mut master, slave)) = open_pty() else {
            return;
        };
        let stdin = slave.try_clone().expect("clone pty stdin");
        let stdout = slave.try_clone().expect("clone pty stdout");
        let mut command = fixture.command();
        command
            .args([
                "new-session",
                "-d",
                "-s",
                "chain-before",
                ";",
                "new-session",
                "-s",
                "pty-attached",
                ";",
                "split-window",
                "-h",
            ])
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(slave));
        let mut child = command.spawn().expect("spawn attaching new-session");

        let (bytes_sender, bytes_receiver) = mpsc::channel();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let (reader_done_sender, reader_done_receiver) = mpsc::channel();
        rustix::io::ioctl_fionbio(&master, true).expect("set pty master nonblocking");
        let reader = thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match master.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        if bytes_sender.send(buffer[..count].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(_) => break,
                }
                if stop_receiver.try_recv().is_ok() {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            drop(master);
            let _ = reader_done_sender.send(());
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut captured = Vec::new();
        let mut early_status = None;
        let entered_alternate_screen = loop {
            if captured
                .windows(b"\x1b[?1049h".len())
                .any(|window| window == b"\x1b[?1049h")
            {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            if let Some(status) = child.try_wait().expect("poll attaching new-session") {
                early_status = Some(status);
                break false;
            }
            match bytes_receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(bytes) => captured.extend_from_slice(&bytes),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break false,
            }
        };

        let _ = child.kill();
        let _ = child.wait();
        let _ = stop_sender.send(());
        reader_done_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("pty reader stopped before deadline");
        reader.join().expect("join pty reader");

        assert!(
            entered_alternate_screen,
            "child exited early={early_status:?}; pty output={}",
            String::from_utf8_lossy(&captured),
        );
        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"chain-before\npty-attached\n");
        assert!(listed.stderr.is_empty());
        let panes = fixture.run(&["list-panes", "-t", "pty-attached", "-F", "#{pane_index}"]);
        assert_eq!(panes.status.code(), Some(0));
        assert_eq!(panes.stdout, b"0\n1\n");
        assert!(panes.stderr.is_empty());
    }

    #[test]
    fn chain_error_after_attachment_is_rendered_inside_the_tui() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let Ok((mut master, slave)) = open_pty() else {
            return;
        };
        rustix::io::ioctl_fionbio(&master, true).expect("set pty master nonblocking");
        let stdin = slave.try_clone().expect("clone pty stdin");
        let stdout = slave.try_clone().expect("clone pty stdout");
        let mut child = fixture
            .command()
            .args([
                "new-session",
                "-s",
                "pty-error",
                ";",
                "split-window",
                "-t",
                ":99",
            ])
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(slave))
            .spawn()
            .expect("spawn attaching chain error");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut captured = Vec::new();
        let mut early_status = None;
        let rendered_error = loop {
            let mut buffer = [0_u8; 4096];
            match master.read(&mut buffer) {
                Ok(0) => {}
                Ok(count) => captured.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
            let alternate_screen = captured
                .windows(b"\x1b[?1049h".len())
                .any(|window| window == b"\x1b[?1049h");
            let error_text = captured
                .windows(b"can't find win".len())
                .any(|window| window == b"can't find win");
            if alternate_screen && error_text {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    early_status = Some(status);
                    break false;
                }
                Ok(None) => {}
                Err(_) => break false,
            }
            thread::sleep(Duration::from_millis(5));
        };
        let still_running = matches!(child.try_wait(), Ok(None));
        let _ = child.kill();
        let _ = child.wait();
        drop(master);

        assert!(
            rendered_error,
            "child exited early={early_status:?}; pty output={}",
            String::from_utf8_lossy(&captured),
        );
        assert!(still_running, "TUI exited after rendering the chain error");
        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"pty-error\n");
        assert!(listed.stderr.is_empty());
    }

    #[test]
    fn startup_config_reenters_through_the_private_tmux_shim() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        std::fs::write(
            &fixture.config,
            b"run-shell \"tmux set-option -g @boot-shim ready\"\nset-option -g @after-shim loaded\n",
        )
        .expect("write reentrant startup config");

        let created = fixture.run(&["new-session", "-d", "-s", "shim"]);
        assert_eq!(
            created.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&created.stderr)
        );
        let values = fixture.run(&[
            "show-options",
            "-gqv",
            "@boot-shim",
            ";",
            "show-options",
            "-gqv",
            "@after-shim",
        ]);
        assert_eq!(values.status.code(), Some(0));
        assert_eq!(values.stdout, b"ready\nloaded\n");
        assert!(values.stderr.is_empty());
    }

    #[test]
    fn gui_style_default_attach_uses_its_working_directory_with_an_empty_daemon() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let started = fixture.run(&["start-server"]);
        assert_eq!(
            started.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&started.stderr)
        );
        let working_directory = tempfile::Builder::new()
            .prefix("zz app cwd ")
            .tempdir_in("/tmp")
            .expect("temporary app working directory");
        let client = zz_daemon::InteractiveClient::connect(&fixture.socket)
            .expect("connect GUI-style interactive client");
        client
            .attach_default_in(working_directory.path())
            .expect("attach from app working directory");
        let expected = std::fs::canonicalize(working_directory.path())
            .expect("canonical app working directory");

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let panes = fixture.run(&["list-panes", "-a", "-F", "#{pane_current_path}"]);
            if panes.status.success()
                && panes.stdout == format!("{}\n", expected.display()).as_bytes()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "pane cwd never matched {}; stdout={} stderr={}",
                expected.display(),
                String::from_utf8_lossy(&panes.stdout),
                String::from_utf8_lossy(&panes.stderr),
            );
            thread::sleep(Duration::from_millis(20));
        }
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
            assert_eq!(output.stderr, b"open terminal failed: not a terminal\n");
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
            assert_eq!(output.stderr, b"can't find session: bogus\n");
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
        assert_eq!(output.stderr, b"no sessions\n");
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
            let expected = format!("{}\n", error.tmux_message());
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
    fn option_value_errors_match_pinned_tmux_stderr() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "error-shapes"]);
        assert_eq!(created.status.code(), Some(0));
        let cases: &[(&[&str], &str)] = &[
            (
                &["set", "-g", "display-time", "-5"],
                "value is too small: -5",
            ),
            (
                &["set", "-g", "display-time", "abc"],
                "value is invalid: abc",
            ),
            (&["set", "-g", "display-time"], "empty value"),
            (&["set", "-g", "@novalue"], "empty value"),
            (
                &["set", "-g", "status-keys", "bogus"],
                "unknown value: bogus",
            ),
            (&["set", "-g", "focus-events", "maybe"], "bad value: maybe"),
            (&["set", "-g", "status-bg", "xxxyyy"], "bad colour: xxxyyy"),
            (
                &["set", "-g", "status-style", "bg=xxxyyy"],
                "invalid style: bg=xxxyyy",
            ),
            (&["set", "-g", "prefix", "boguskey"], "bad key: boguskey"),
            (
                &["set", "-g", "default-shell", "/not/a/shell"],
                "not a suitable shell: /not/a/shell",
            ),
            (
                &["set", "-g", "default-client-command", "if -x {"],
                "syntax error",
            ),
        ];
        for (arguments, expected) in cases {
            let output = fixture.run(arguments);
            assert_eq!(output.status.code(), Some(1), "{arguments:?}");
            assert!(output.stdout.is_empty(), "{arguments:?}");
            assert_eq!(
                output.stderr,
                format!("{expected}\n").as_bytes(),
                "{arguments:?}"
            );
        }

        let first = fixture.run(&["set", "-g", "@once", "first"]);
        assert_eq!(first.status.code(), Some(0));
        assert!(first.stdout.is_empty());
        assert!(first.stderr.is_empty());
        let duplicate = fixture.run(&["set", "-go", "@once", "second"]);
        assert_eq!(duplicate.status.code(), Some(1));
        assert!(duplicate.stdout.is_empty());
        assert_eq!(duplicate.stderr, b"already set: @once\n");
    }

    #[test]
    fn target_and_unknown_command_errors_match_pinned_tmux_stderr() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "error-shapes"]);
        assert_eq!(created.status.code(), Some(0));
        for (arguments, expected) in [
            (
                &["kill-session", "-t", "bogus"] as &[&str],
                b"can't find session: bogus\n" as &[u8],
            ),
            (&["wibble"], b"unknown command: wibble\n"),
        ] {
            let output = fixture.run(arguments);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, expected);
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

    mod control_mode {
        use super::*;

        #[derive(Debug)]
        struct Block {
            time: u64,
            number: u64,
            flags: u8,
            payload: Vec<String>,
            error: bool,
        }

        #[derive(Debug)]
        struct Stream {
            blocks: Vec<Block>,
            outside: Vec<String>,
        }

        fn marker(line: &str, expected: &str) -> Option<(u64, u64, u8)> {
            let mut fields = line.split(' ');
            if fields.next()? != expected {
                return None;
            }
            let time = fields.next()?.parse().ok()?;
            let number = fields.next()?.parse().ok()?;
            let flags = fields.next()?.parse().ok()?;
            fields.next().is_none().then_some((time, number, flags))
        }

        fn parse_stream(output: &[u8], double: bool) -> Stream {
            let output = if double {
                assert!(output.starts_with(b"\x1bP1000p"));
                assert!(output.ends_with(b"\x1b\\"));
                &output[b"\x1bP1000p".len()..output.len() - b"\x1b\\".len()]
            } else {
                assert!(!output.starts_with(b"\x1bP1000p"));
                assert!(!output.ends_with(b"\x1b\\"));
                output
            };
            let text = std::str::from_utf8(output).expect("UTF-8 control output");
            let text = text.strip_suffix('\n').unwrap_or(text);
            let lines = text.split('\n').collect::<Vec<_>>();
            let mut blocks = Vec::new();
            let mut outside = Vec::new();
            let mut index = 0;
            while index < lines.len() {
                let Some((time, number, flags)) = marker(lines[index], "%begin") else {
                    outside.push(lines[index].to_owned());
                    index += 1;
                    continue;
                };
                index += 1;
                let mut payload = Vec::new();
                let error = loop {
                    assert!(index < lines.len(), "unterminated block {number}");
                    if let Some(end) = marker(lines[index], "%end") {
                        assert_eq!(end, (time, number, flags));
                        index += 1;
                        break false;
                    }
                    if let Some(end) = marker(lines[index], "%error") {
                        assert_eq!(end, (time, number, flags));
                        index += 1;
                        break true;
                    }
                    payload.push(lines[index].to_owned());
                    index += 1;
                };
                blocks.push(Block {
                    time,
                    number,
                    flags,
                    payload,
                    error,
                });
            }
            for (index, block) in blocks.iter().enumerate() {
                assert_eq!(block.number, index as u64 + 1);
                assert!(block.time > 0);
            }
            Stream { blocks, outside }
        }

        fn assert_block(block: &Block, number: u64, flags: u8, payload: &[&str], error: bool) {
            assert_eq!(block.number, number);
            assert_eq!(block.flags, flags);
            assert_eq!(
                block.payload,
                payload
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect::<Vec<_>>()
            );
            assert_eq!(block.error, error);
        }

        fn assert_attached_startup(outside: &[String], name: &str) {
            let settled = outside
                .iter()
                .filter(|line| !line.starts_with("%window-renamed @0 "))
                .map(String::as_str)
                .collect::<Vec<_>>();
            assert_eq!(
                settled,
                [
                    "%window-add @0",
                    "%sessions-changed",
                    &format!("%session-changed $0 {name}"),
                    "%exit",
                ]
            );
        }

        #[test]
        fn control_read_only_connect_failure_has_no_framing_or_exit() {
            let fixture = Fixture::new();
            let output = fixture.run(&["-C", "ls"]);
            assert_missing(&output, &fixture.missing_message());
            fixture.assert_not_started();
        }

        #[test]
        fn control_list_commands_autostarts_and_frames_the_initial_command() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output = fixture.run(&["-C", "list-commands"]);
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 1);
            assert_eq!(stream.blocks[0].flags, 0);
            assert!(!stream.blocks[0].payload.is_empty());
            assert!(!stream.blocks[0].error);
            assert_eq!(stream.outside, ["%exit"]);
            assert!(fixture.socket.exists());
        }

        #[test]
        fn detached_control_new_session_exits_without_reading_stdin() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let (mut child, stdin) =
                fixture.spawn_with_open_stdin(&["-C", "new-session", "-d", "-s", "x"]);
            let deadline = Instant::now() + Duration::from_secs(5);
            let status = loop {
                if let Some(status) = child.try_wait().expect("poll control process") {
                    break status;
                }
                if Instant::now() >= deadline {
                    child.kill().expect("kill stalled control process");
                    panic!("detached control client read stdin");
                }
                thread::sleep(Duration::from_millis(10));
            };
            drop(stdin);
            let output = child.wait_with_output().expect("collect control output");
            assert_eq!(output.status, status);
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 1);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_eq!(stream.outside, ["%exit"]);
        }

        #[test]
        fn control_ls_with_a_daemon_frames_session_output() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let created = fixture.run(&["new-session", "-d", "-s", "listed"]);
            assert_eq!(created.status.code(), Some(0));
            let output = fixture.run(&["-C", "ls"]);
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 1);
            assert_eq!(stream.blocks[0].flags, 0);
            assert!(!stream.blocks[0].error);
            assert!(stream.blocks[0].payload[0].starts_with("listed:"));
            assert_eq!(stream.outside, ["%exit"]);
        }

        #[test]
        fn control_unknown_initial_command_is_bare_after_connect() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let created = fixture.run(&["new-session", "-d"]);
            assert_eq!(created.status.code(), Some(0));
            let output = fixture.run(&["-C", "frobnicate"]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert!(stream.blocks.is_empty());
            assert_eq!(stream.outside, ["unknown command: frobnicate", "%exit"]);
        }

        #[test]
        fn control_command_error_uses_the_inner_server_message() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output = fixture.run(&["-C", "list-commands", "bogus-command"]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 1);
            assert_block(
                &stream.blocks[0],
                1,
                0,
                &["unknown command: bogus-command"],
                true,
            );
            assert_eq!(stream.outside, ["%exit"]);
        }

        #[test]
        fn attached_control_executes_stdin_until_empty_line() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output =
                fixture.run_with_stdin(&["-C", "new-session", "-s", "attached"], b"ls\n\n");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_eq!(stream.blocks[1].flags, 1);
            assert!(!stream.blocks[1].error);
            assert!(stream.blocks[1].payload[0].starts_with("attached:"));
            assert!(stream.blocks[1].payload[0].ends_with(" (attached)"));
            assert_attached_startup(&stream.outside, "attached");
        }

        #[test]
        fn control_chains_frame_each_command_and_abort_only_the_failing_line() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output = fixture.run_with_stdin(
                &["-C", "new-session", "-s", "chain"],
                b"display-message -p one ; display-message -p two\nkill-session -t nosuch ; display-message -p skipped\ndisplay-message -p fresh\n\n",
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 5);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &["one"], false);
            assert_block(&stream.blocks[2], 3, 1, &["two"], false);
            assert_block(
                &stream.blocks[3],
                4,
                1,
                &["can't find session: nosuch"],
                true,
            );
            assert_block(&stream.blocks[4], 5, 1, &["fresh"], false);
            assert_attached_startup(&stream.outside, "chain");
        }

        #[test]
        fn bare_control_defaults_to_attached_new_session() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output = fixture.run_with_stdin(&["-C"], b"ls\n\n");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_eq!(stream.blocks[1].flags, 1);
            assert!(!stream.blocks[1].payload.is_empty());
            assert!(!stream.blocks[1].error);
            assert_attached_startup(&stream.outside, "0");
        }

        #[test]
        fn double_control_wraps_stdout_in_dcs_and_st() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output = fixture.run(&["-CC", "new-session", "-d"]);
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, true);
            assert_eq!(stream.blocks.len(), 1);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_eq!(stream.outside, ["%exit"]);
        }

        #[test]
        fn control_stdin_parse_error_is_framed_and_the_loop_continues() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output = fixture.run_with_stdin(
                &["-C", "new-session", "-s", "parse-error"],
                b"bogus-command\n\n",
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(
                &stream.blocks[1],
                2,
                1,
                &["parse error: unknown command: bogus-command"],
                true,
            );
            assert_attached_startup(&stream.outside, "parse-error");
        }

        #[test]
        fn control_config_error_keeps_the_source_line_and_inner_message() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let source = fixture._directory.path().join("config-error.conf");
            std::fs::write(&source, "wibble\n").expect("write invalid config");
            let input = format!("source-file {}\n\n", source.display());
            let output = fixture.run_with_stdin(
                &["-C", "new-session", "-s", "config-error"],
                input.as_bytes(),
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert!(stream.outside.contains(&format!(
                "%config-error {}:1: unknown command: wibble",
                source.display()
            )));
            assert_attached_startup(
                &stream
                    .outside
                    .iter()
                    .filter(|line| !line.starts_with("%config-error "))
                    .cloned()
                    .collect::<Vec<_>>(),
                "config-error",
            );
        }

        #[test]
        fn control_kill_server_closes_its_block_before_exit() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output =
                fixture.run_with_stdin(&["-C", "new-session", "-s", "stopping"], b"kill-server\n");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_attached_startup(&stream.outside, "stopping");
        }

        #[test]
        fn control_notifications_layout_and_output_follow_the_live_socket_stream() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let (child, mut stdin) = fixture.spawn_with_open_stdin(&[
                "-C",
                "new-session",
                "-s",
                "watched",
                "exec /bin/cat",
            ]);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let ready = fixture.run(&["has-session", "-t", "watched"]);
                if ready.status.success() {
                    break;
                }
                assert!(Instant::now() < deadline, "control session did not start");
                thread::sleep(Duration::from_millis(10));
            }

            let added = fixture.run(&[
                "new-window",
                "-d",
                "-t",
                "watched",
                "-n",
                "added",
                "exec /bin/cat",
            ]);
            assert_eq!(added.status.code(), Some(0));
            let foreign = fixture.run(&["new-session", "-d", "-s", "foreign", "exec /bin/cat"]);
            assert_eq!(foreign.status.code(), Some(0));
            let split = fixture.run(&[
                "split-window",
                "-h",
                "-d",
                "-t",
                "watched:0",
                "exec /bin/cat",
            ]);
            assert_eq!(split.status.code(), Some(0));
            let typed = fixture.run(&["send-keys", "-l", "-t", "watched:0.0", "CONTROL_OUTPUT"]);
            assert_eq!(typed.status.code(), Some(0));
            let entered = fixture.run(&["send-keys", "-t", "watched:0.0", "Enter"]);
            assert_eq!(entered.status.code(), Some(0));

            stdin
                .write_all(b"run-shell \"sleep 1\"\n")
                .expect("start slow control command");
            stdin.flush().expect("flush slow control command");
            thread::sleep(Duration::from_millis(100));
            let renamed = fixture.run(&["rename-window", "-t", "watched:0", "during"]);
            assert_eq!(renamed.status.code(), Some(0));
            thread::sleep(Duration::from_millis(1100));
            stdin.write_all(b"\n").expect("end control input");
            drop(stdin);

            let output = child.wait_with_output().expect("wait for control stream");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert!(
                stream
                    .blocks
                    .iter()
                    .all(|block| { block.payload.iter().all(|line| !line.starts_with('%')) })
            );

            let notifications = stream
                .outside
                .iter()
                .filter(|line| !line.starts_with("%output ") && line.as_str() != "%exit")
                .skip_while(|line| line.starts_with("%window-renamed @0 "))
                .collect::<Vec<_>>();
            assert!(notifications[0].starts_with("%window-add @"));
            assert_eq!(notifications[1], "%sessions-changed");
            assert!(notifications[2].starts_with("%session-changed $"));
            assert!(notifications[2].ends_with(" watched"));
            assert!(
                notifications
                    .iter()
                    .any(|line| line.starts_with("%window-add @"))
            );
            assert!(
                notifications
                    .iter()
                    .any(|line| line.starts_with("%unlinked-window-add @"))
            );
            assert!(notifications.iter().any(|line| {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                fields.first() == Some(&"%layout-change")
                    && fields.get(2).is_some_and(|layout| checked_layout(layout))
                    && fields.get(3).is_some_and(|layout| checked_layout(layout))
            }));
            assert!(
                notifications
                    .iter()
                    .any(|line| line.starts_with("%window-renamed @") && line.ends_with(" during"))
            );
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line.starts_with("%output %") && line.contains("CONTROL_OUTPUT"))
            );
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
        }

        #[test]
        fn refresh_client_pane_states_gate_the_live_control_stream() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let (child, mut stdin) = fixture.spawn_with_open_stdin(&[
                "-C",
                "new-session",
                "-s",
                "flow-state",
                "exec /bin/cat",
            ]);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let ready = fixture.run(&["has-session", "-t", "flow-state"]);
                if ready.status.success() {
                    break;
                }
                assert!(Instant::now() < deadline, "control session did not start");
                thread::sleep(Duration::from_millis(10));
            }
            let pane_output = fixture.run(&["list-panes", "-t", "flow-state", "-F", "#{pane_id}"]);
            assert!(pane_output.status.success());
            let pane = String::from_utf8(pane_output.stdout)
                .expect("pane id")
                .trim()
                .to_owned();
            fn control(stdin: &mut ChildStdin, pane: &str, state: &str) {
                writeln!(stdin, "refresh-client -A {pane}:{state}").expect("write pane state");
                stdin.flush().expect("flush pane state");
                thread::sleep(Duration::from_millis(100));
            }
            fn type_line(stdin: &mut ChildStdin, pane: &str, text: &str) {
                writeln!(stdin, "send-keys -l -t {pane} {text}").expect("write typed text");
                writeln!(stdin, "send-keys -t {pane} Enter").expect("write typed enter");
                stdin.flush().expect("flush typed text");
                thread::sleep(Duration::from_millis(150));
            }

            control(&mut stdin, &pane, "off");
            type_line(&mut stdin, &pane, "HIDDEN_WHILE_OFF");
            control(&mut stdin, &pane, "on");
            type_line(&mut stdin, &pane, "VISIBLE_AFTER_ON");
            thread::sleep(Duration::from_millis(400));
            control(&mut stdin, &pane, "pause");
            control(&mut stdin, &pane, "pause");
            type_line(&mut stdin, &pane, "HIDDEN_WHILE_PAUSED");
            control(&mut stdin, &pane, "continue");
            control(&mut stdin, &pane, "continue");
            type_line(&mut stdin, &pane, "VISIBLE_AFTER_CONTINUE");
            thread::sleep(Duration::from_millis(400));
            stdin.write_all(b"\n").expect("end control input");
            drop(stdin);

            let output = child.wait_with_output().expect("wait for control stream");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            let output_lines = stream
                .outside
                .iter()
                .filter(|line| line.starts_with("%output "))
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!output_lines.contains("HIDDEN_WHILE_OFF"));
            assert!(!output_lines.contains("HIDDEN_WHILE_PAUSED"));
            assert!(output_lines.contains("VISIBLE_AFTER_ON"));
            assert!(output_lines.contains("VISIBLE_AFTER_CONTINUE"));
            assert_eq!(
                stream
                    .outside
                    .iter()
                    .filter(|line| line.as_str() == format!("%pause {pane}"))
                    .count(),
                1
            );
            assert_eq!(
                stream
                    .outside
                    .iter()
                    .filter(|line| line.as_str() == format!("%continue {pane}"))
                    .count(),
                1
            );
        }

        #[test]
        fn refresh_client_c_sizes_a_control_target_for_menu_gating() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let (control, mut stdin) = fixture.spawn_with_open_stdin(&[
                "-C",
                "new-session",
                "-s",
                "sizing",
                "exec /bin/cat",
            ]);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let listed = fixture.run(&["list-clients", "-F", "#{client_name}:#{client_flags}"]);
                if listed.status.success()
                    && String::from_utf8_lossy(&listed.stdout).contains("control-mode")
                {
                    break;
                }
                assert!(Instant::now() < deadline, "control client did not attach");
                thread::sleep(Duration::from_millis(10));
            }
            let listed = fixture.run(&["list-clients", "-F", "#{client_name}:#{client_flags}"]);
            let line = String::from_utf8(listed.stdout)
                .expect("client list")
                .lines()
                .find(|line| line.ends_with("control-mode"))
                .expect("control client list row")
                .to_owned();
            let target = line
                .split_once(':')
                .expect("client row fields")
                .0
                .to_owned();
            let menu_args = [
                "display-menu",
                "-c",
                target.as_str(),
                "Item",
                "i",
                "display-message chosen",
            ];
            let no_size = fixture.run(&menu_args);
            assert_eq!(no_size.status.code(), Some(0));

            stdin
                .write_all(b"refresh-client -C 100x50\n")
                .expect("set control size");
            stdin.flush().expect("flush control size");
            let expected_dimensions = format!("{target}:100x50");
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let dimensions = fixture.run(&[
                    "list-clients",
                    "-F",
                    "#{client_name}:#{client_width}x#{client_height}",
                ]);
                if String::from_utf8_lossy(&dimensions.stdout)
                    .lines()
                    .any(|line| line == expected_dimensions)
                {
                    break;
                }
                assert!(Instant::now() < deadline, "control size did not settle");
                thread::sleep(Duration::from_millis(10));
            }

            let mut menu = fixture
                .command()
                .args(menu_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn sized menu");
            let hold_deadline = Instant::now() + Duration::from_millis(300);
            while Instant::now() < hold_deadline {
                assert!(
                    menu.try_wait().expect("poll sized menu").is_none(),
                    "sized menu silently no-op'd"
                );
                thread::sleep(Duration::from_millis(10));
            }
            stdin.write_all(b"\n").expect("detach sized control");
            drop(stdin);
            let control_output = control.wait_with_output().expect("wait for control client");
            assert_eq!(control_output.status.code(), Some(0));
            let menu_output = menu.wait_with_output().expect("wait for sized menu");
            assert_eq!(menu_output.status.code(), Some(0));
        }

        #[test]
        fn refresh_client_b_reports_initial_change_and_exact_removal() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let (child, mut stdin) = fixture.spawn_with_open_stdin(&[
                "-C",
                "new-session",
                "-s",
                "subscribed",
                "exec /bin/cat",
            ]);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if fixture
                    .run(&["has-session", "-t", "subscribed"])
                    .status
                    .success()
                {
                    break;
                }
                assert!(Instant::now() < deadline, "control session did not start");
                thread::sleep(Duration::from_millis(10));
            }

            stdin
                .write_all(b"refresh-client -B watch::#{session_name}\n")
                .expect("add subscription");
            stdin.flush().expect("flush subscription");
            thread::sleep(Duration::from_millis(1200));
            assert_eq!(
                fixture
                    .run(&["rename-session", "-t", "subscribed", "renamed"])
                    .status
                    .code(),
                Some(0)
            );
            thread::sleep(Duration::from_millis(1200));
            stdin
                .write_all(b"refresh-client -B watch\nrename-session -t renamed removed\n")
                .expect("remove subscription and rename session");
            stdin.flush().expect("flush removal");
            thread::sleep(Duration::from_millis(1200));
            stdin.write_all(b"\n").expect("end control input");
            drop(stdin);

            let output = child.wait_with_output().expect("wait for control stream");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            let subscriptions = stream
                .outside
                .iter()
                .filter(|line| line.starts_with("%subscription-changed watch "))
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(subscriptions.len(), 2);
            assert!(subscriptions[0].ends_with(" - - - : subscribed"));
            assert!(subscriptions[1].ends_with(" - - - : renamed"));
            assert!(
                subscriptions
                    .iter()
                    .all(|line| !line.ends_with(" : removed"))
            );
        }

        #[test]
        fn pause_after_emits_extended_output_then_pauses_a_slow_control_client() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let (child, mut stdin) = fixture.spawn_with_open_stdin(&[
                "-C",
                "new-session",
                "-s",
                "pause-after",
                "exec /bin/sh",
            ]);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let ready = fixture.run(&["has-session", "-t", "pause-after"]);
                if ready.status.success() {
                    break;
                }
                assert!(Instant::now() < deadline, "control session did not start");
                thread::sleep(Duration::from_millis(10));
            }
            let pane_output = fixture.run(&["list-panes", "-t", "pause-after", "-F", "#{pane_id}"]);
            assert!(pane_output.status.success());
            let pane = String::from_utf8(pane_output.stdout)
                .expect("pane id")
                .trim()
                .to_owned();
            stdin
                .write_all(b"refresh-client -f pause-after=1\n")
                .expect("enable pause-after");
            stdin.flush().expect("flush pause-after");
            thread::sleep(Duration::from_millis(100));
            let marker = fixture.socket.with_extension("flooded");
            let flood = format!("yes x | head -c 1048576; touch {}", marker.display());
            assert!(
                fixture
                    .run(&["send-keys", "-l", "-t", &pane, &flood])
                    .status
                    .success()
            );
            assert!(
                fixture
                    .run(&["send-keys", "-t", &pane, "Enter"])
                    .status
                    .success()
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            while !marker.exists() {
                assert!(Instant::now() < deadline, "flood never completed");
                thread::sleep(Duration::from_millis(20));
            }
            thread::sleep(Duration::from_millis(1500));
            stdin.write_all(b"\n").expect("end control input");
            drop(stdin);

            let output = child
                .wait_with_output()
                .expect("wait for paused control stream");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert!(stream.outside.iter().any(|line| {
                let fields = line.splitn(5, ' ').collect::<Vec<_>>();
                fields.first() == Some(&"%extended-output")
                    && fields.get(1) == Some(&pane.as_str())
                    && fields.get(2).is_some_and(|age| age.parse::<u64>().is_ok())
                    && fields.get(3) == Some(&":")
            }));
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == &format!("%pause {pane}"))
            );
        }

        #[test]
        fn wait_exit_holds_the_control_process_until_a_second_blank_line() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let (mut child, mut stdin) = fixture.spawn_with_open_stdin(&[
                "-C",
                "new-session",
                "-s",
                "wait-exit",
                "exec /bin/cat",
            ]);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let ready = fixture.run(&["has-session", "-t", "wait-exit"]);
                if ready.status.success() {
                    break;
                }
                assert!(Instant::now() < deadline, "control session did not start");
                thread::sleep(Duration::from_millis(10));
            }
            stdin
                .write_all(b"refresh-client -f wait-exit\n")
                .expect("enable wait-exit");
            stdin.flush().expect("flush wait-exit flag");
            thread::sleep(Duration::from_millis(100));
            stdin.write_all(b"\n").expect("detach control client");
            stdin.flush().expect("flush detach");
            let hold_deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < hold_deadline {
                assert!(
                    child.try_wait().expect("poll wait-exit process").is_none(),
                    "wait-exit process exited before its acknowledgement"
                );
                thread::sleep(Duration::from_millis(10));
            }
            stdin.write_all(b"\n").expect("acknowledge exit");
            drop(stdin);

            let output = child
                .wait_with_output()
                .expect("wait for wait-exit control");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
        }

        fn checked_layout(layout: &str) -> bool {
            layout.split_once(',').is_some_and(|(checksum, body)| {
                checksum.len() == 4
                    && checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && !body.is_empty()
            })
        }
    }
}
