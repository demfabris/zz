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
        fmt::Write as _,
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

    struct CargoLauncher {
        _directory: tempfile::TempDir,
        path: PathBuf,
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

        fn command_from(&self, executable: &Path) -> Command {
            let mut command = Command::new(executable);
            command
                .arg("-f")
                .arg(&self.config)
                .arg("-S")
                .arg(&self.socket);
            command
        }

        fn command(&self) -> Command {
            self.command_from(Path::new(env!("CARGO_BIN_EXE_zz")))
        }

        fn run(&self, arguments: &[&str]) -> Output {
            self.command()
                .args(arguments)
                .output()
                .expect("run zz command")
        }

        fn run_with_configs(&self, configs: &[&Path], arguments: &[&str]) -> Output {
            let mut command = Command::new(Path::new(env!("CARGO_BIN_EXE_zz")));
            for config in configs {
                command.arg("-f").arg(config);
            }
            command
                .arg("-S")
                .arg(&self.socket)
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

        fn assert_stopped(&self) {
            let deadline = Instant::now() + Duration::from_secs(2);
            let identity = identity_path(&self.socket);
            while (self.socket.exists() || identity.exists()) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            self.assert_not_started();
        }
    }

    impl CargoLauncher {
        fn new() -> Self {
            let directory = tempfile::Builder::new()
                .prefix("zz launcher fixture ")
                .tempdir_in("/tmp")
                .expect("temporary launcher directory");
            let install = directory.path().join("installed zz with spaces");
            std::fs::create_dir_all(&install).expect("create launcher install directory");
            let path = install.join("cli");
            std::fs::copy(env!("CARGO_BIN_EXE_zz_cli"), &path).expect("copy Cargo launcher");
            std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_zz"), install.join("zz"))
                .expect("link Cargo zz executable");
            assert!(path.to_string_lossy().contains(' '));
            Self {
                _directory: directory,
                path,
            }
        }

        fn command(&self, fixture: &Fixture) -> Command {
            let mut command = Command::new(&self.path);
            let home = fixture.config.parent().expect("fixture config directory");
            command
                .env("ZZ_SOCKET", &fixture.socket)
                .env("XDG_CONFIG_HOME", home)
                .env("HOME", home);
            command
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.run(&["--kill-server"]);
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
        assert!(killed.stdout.is_empty());
        assert!(killed.stderr.is_empty());
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
            "--kill-server",
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
    fn cold_autospawn_uses_the_launching_client_cwd_for_startup_sources() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let caller_cwd = source_directory(&fixture, "startup client cwd [literal]*? with spaces");
        let caller_entry_directory = caller_cwd.join("a");
        std::fs::create_dir(&caller_entry_directory).expect("caller entry directory");
        std::fs::write(
            caller_cwd.join("startup-top.conf"),
            b"set-option -g @startup_client_cwd_top caller-root\n",
        )
        .expect("caller top-level startup source");
        std::fs::write(
            caller_entry_directory.join("entry.conf"),
            b"set-option -g @startup_client_cwd_entry caller-entry\nsource-file startup-leaf.conf\n",
        )
        .expect("caller nested startup entry");
        std::fs::write(
            caller_cwd.join("startup-leaf.conf"),
            b"set-option -g @startup_client_cwd_leaf caller-root\n",
        )
        .expect("caller-root nested startup source");
        std::fs::write(
            caller_entry_directory.join("startup-leaf.conf"),
            b"set-option -g @startup_client_cwd_leaf containing-file-decoy\n",
        )
        .expect("containing-file startup decoy");

        let config_directory = fixture.config.parent().expect("fixture config directory");
        let config_entry_directory = config_directory.join("a");
        std::fs::create_dir(&config_entry_directory).expect("config entry directory");
        std::fs::write(
            config_directory.join("startup-top.conf"),
            b"set-option -g @startup_client_cwd_top config-decoy\n",
        )
        .expect("config top-level startup decoy");
        std::fs::write(
            config_entry_directory.join("entry.conf"),
            b"set-option -g @startup_client_cwd_entry config-decoy\n",
        )
        .expect("config nested startup decoy");
        std::fs::write(
            &fixture.config,
            b"source-file startup-top.conf\nsource-file a/entry.conf\n",
        )
        .expect("startup source config");

        let started = fixture
            .command()
            .current_dir(&caller_cwd)
            .args(["new-session", "-d", "-s", "startup-client-cwd"])
            .output()
            .expect("cold autospawn from startup client cwd");
        assert_eq!(
            started.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&started.stderr)
        );
        assert!(started.stdout.is_empty());
        assert!(started.stderr.is_empty());

        for (option, expected) in [
            ("@startup_client_cwd_top", b"caller-root\n" as &[u8]),
            ("@startup_client_cwd_entry", b"caller-entry\n"),
            ("@startup_client_cwd_leaf", b"caller-root\n"),
        ] {
            let shown = fixture.run(&["show-options", "-gqv", option]);
            assert_eq!(shown.status.code(), Some(0), "{option}");
            assert_eq!(shown.stdout, expected, "{option}");
            assert!(shown.stderr.is_empty(), "{option}");
        }
    }

    #[test]
    fn startup_config_shares_a_fifty_source_command_budget() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let directory = fixture
            .config
            .parent()
            .expect("fixture config directory")
            .join("startup-sources");
        std::fs::create_dir_all(&directory).expect("startup source directory");
        let source = |first: usize, last: usize| {
            let mut config = String::new();
            for index in first..=last {
                let leaf = directory.join(format!("leaf{index}.conf"));
                std::fs::write(&leaf, format!("set-option -g @startup{index} yes\n"))
                    .expect("startup source leaf");
                let _ = writeln!(config, "source-file '{}'", leaf.display());
            }
            config
        };
        std::fs::write(&fixture.config, source(1, 45)).expect("first startup config");
        let second = directory.join("second-root.conf");
        let mut tail = source(46, 60);
        tail.push_str("set-option -g @startup-after yes\n");
        std::fs::write(&second, tail).expect("second startup config");

        let started = fixture.run_with_configs(
            &[fixture.config.as_path(), second.as_path()],
            &["new-session", "-d", "-s", "startup-depth"],
        );
        assert_eq!(started.status.code(), Some(0));
        assert!(started.stdout.is_empty());
        assert!(started.stderr.is_empty());
        assert_eq!(
            fixture.run(&["show-options", "-gqv", "@startup45"]).stdout,
            b"yes\n"
        );
        assert_eq!(
            fixture.run(&["show-options", "-gqv", "@startup50"]).stdout,
            b"yes\n"
        );
        assert!(
            fixture
                .run(&["show-options", "-gqv", "@startup51"])
                .stdout
                .is_empty()
        );
        assert_eq!(
            fixture
                .run(&["show-options", "-gqv", "@startup-after"])
                .stdout,
            b"yes\n"
        );
    }

    #[test]
    fn event_hooks_are_clientless_and_command_hooks_retain_the_command_client() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        std::fs::write(
            &fixture.config,
            b"set-hook -g session-created 'new-session -s fromevent'\nset-hook -g after-new-session 'new-session -s fromcommand'\n",
        )
        .expect("write hook client config");

        let created = fixture.run(&["new-session", "-d", "-s", "base"]);
        assert_eq!(created.status.code(), Some(0));
        assert!(created.stdout.is_empty());
        assert!(created.stderr.is_empty());

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"base\nfromevent\n");
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

    fn source_directory(fixture: &Fixture, name: &str) -> PathBuf {
        let directory = fixture.config.with_file_name(name);
        std::fs::create_dir_all(&directory).expect("source fixture directory");
        directory
    }

    fn write_source(directory: &Path, name: &str, body: &str) -> String {
        let path = directory.join(name);
        std::fs::write(&path, body).expect("write source fixture");
        path.to_str().expect("UTF-8 source fixture path").to_owned()
    }

    #[test]
    fn source_file_resolves_relative_paths_from_the_command_client_cwd() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let daemon_cwd = std::fs::canonicalize(source_directory(&fixture, "daemon cwd"))
            .expect("canonical daemon cwd");
        let caller_cwd = std::fs::canonicalize(source_directory(
            &fixture,
            "caller cwd [literal]*? with spaces",
        ))
        .expect("canonical caller cwd");
        let daemon_home = std::fs::canonicalize(source_directory(
            &fixture,
            "daemon home [literal]*? with spaces",
        ))
        .expect("canonical daemon home");
        assert_ne!(daemon_cwd, caller_cwd);

        let literal_tilde_name = "literal-source.conf";
        std::fs::write(
            daemon_home.join(literal_tilde_name),
            b"set-option -g @literal_tilde_source decoy-home\n",
        )
        .expect("write home-expansion decoy source");
        let literal_tilde_directory = caller_cwd.join("~");
        std::fs::create_dir(&literal_tilde_directory).expect("literal tilde directory");
        std::fs::write(
            literal_tilde_directory.join(literal_tilde_name),
            b"set-option -g @literal_tilde_source caller-literal\n",
        )
        .expect("write literal tilde source");

        let relative_directory = "relative configs with spaces";
        let sources = caller_cwd.join(relative_directory);
        std::fs::create_dir_all(&sources).expect("relative source directory");
        std::fs::write(
            sources.join("source-file-w-0-0-10.conf"),
            b"set-option -g @cwd_order ten\n",
        )
        .expect("write first relative source");
        std::fs::write(
            sources.join("source-file-w-0-0-20.conf"),
            b"set-option -g @cwd_order twenty\n",
        )
        .expect("write second relative source");
        let nested_entry_directory = caller_cwd.join("a");
        std::fs::create_dir(&nested_entry_directory).expect("nested source entry directory");
        std::fs::write(
            caller_cwd.join("leaf.conf"),
            b"set-option -g @nested_client_cwd caller-root\n",
        )
        .expect("write caller-root nested source");
        std::fs::write(
            nested_entry_directory.join("leaf.conf"),
            b"set-option -g @nested_client_cwd containing-file-decoy\n",
        )
        .expect("write containing-file nested source decoy");
        std::fs::write(
            nested_entry_directory.join("entry.conf"),
            b"set-option -g @nested_replay_started yes\nsource-file leaf.conf\n",
        )
        .expect("write nested source entry");
        assert!(!daemon_cwd.join(relative_directory).exists());

        let started = fixture
            .command()
            .env("HOME", &daemon_home)
            .current_dir(&daemon_cwd)
            .args(["new-session", "-d", "-s", "w"])
            .output()
            .expect("start daemon from its cwd");
        assert_eq!(
            started.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&started.stderr)
        );

        let run_from_caller = |arguments: &[&str]| {
            fixture
                .command()
                .env("HOME", &daemon_home)
                .current_dir(&caller_cwd)
                .args(arguments)
                .output()
                .expect("run command from caller cwd")
        };
        let format_prefix = format!(
            "{relative_directory}/source-file-#{{session_name}}-#{{window_index}}-#{{pane_index}}"
        );
        let glob = format!("{format_prefix}-[12]0.conf");
        let ten = format!("{format_prefix}-10.conf");
        let twenty = format!("{format_prefix}-20.conf");
        let missing = format!("{relative_directory}/source-file-#{{session_name}}-missing.conf");

        let literal_tilde = format!("~/{literal_tilde_name}");
        let sourced_literal_tilde = run_from_caller(&["source-file", &literal_tilde]);
        assert_eq!(sourced_literal_tilde.status.code(), Some(0));
        assert!(sourced_literal_tilde.stdout.is_empty());
        assert!(sourced_literal_tilde.stderr.is_empty());
        let literal_tilde_value =
            run_from_caller(&["show-options", "-gqv", "@literal_tilde_source"]);
        assert_eq!(literal_tilde_value.status.code(), Some(0));
        assert_eq!(literal_tilde_value.stdout, b"caller-literal\n");
        assert!(literal_tilde_value.stderr.is_empty());

        let nested = run_from_caller(&["source-file", "a/entry.conf"]);
        assert_eq!(nested.status.code(), Some(0));
        assert!(nested.stdout.is_empty());
        assert!(nested.stderr.is_empty());
        let nested_replay = run_from_caller(&["show-options", "-gqv", "@nested_replay_started"]);
        assert_eq!(nested_replay.status.code(), Some(0));
        assert_eq!(nested_replay.stdout, b"yes\n");
        assert!(nested_replay.stderr.is_empty());
        let nested_base = run_from_caller(&["show-options", "-gqv", "@nested_client_cwd"]);
        assert_eq!(nested_base.status.code(), Some(0));
        assert_eq!(nested_base.stdout, b"caller-root\n");
        assert!(nested_base.stderr.is_empty());

        let globbed = run_from_caller(&["source-file", "-F", &glob]);
        assert_eq!(globbed.status.code(), Some(0));
        assert!(globbed.stdout.is_empty());
        assert!(globbed.stderr.is_empty());
        let glob_order = run_from_caller(&["show-options", "-gqv", "@cwd_order"]);
        assert_eq!(glob_order.status.code(), Some(0));
        assert_eq!(glob_order.stdout, b"twenty\n");
        assert!(glob_order.stderr.is_empty());

        let explicit = run_from_caller(&["source-file", "-F", &twenty, &ten]);
        assert_eq!(explicit.status.code(), Some(0));
        assert!(explicit.stdout.is_empty());
        assert!(explicit.stderr.is_empty());
        let explicit_order = run_from_caller(&["show-options", "-gqv", "@cwd_order"]);
        assert_eq!(explicit_order.status.code(), Some(0));
        assert_eq!(explicit_order.stdout, b"ten\n");
        assert!(explicit_order.stderr.is_empty());

        let quiet = run_from_caller(&["source-file", "-Fq", &missing, &twenty]);
        assert_eq!(quiet.status.code(), Some(0));
        assert!(quiet.stdout.is_empty());
        assert!(quiet.stderr.is_empty());
        let quiet_continued = run_from_caller(&["show-options", "-gqv", "@cwd_order"]);
        assert_eq!(quiet_continued.status.code(), Some(0));
        assert_eq!(quiet_continued.stdout, b"twenty\n");
        assert!(quiet_continued.stderr.is_empty());

        let loud = run_from_caller(&["source-file", "-F", &missing]);
        assert_eq!(loud.status.code(), Some(1));
        assert!(loud.stdout.is_empty());
        assert_eq!(
            loud.stderr,
            format!(
                "No such file or directory: {}\n",
                Path::new(relative_directory)
                    .join("source-file-w-missing.conf")
                    .display()
            )
            .into_bytes()
        );
    }

    #[test]
    fn source_file_diagnostics_split_stdout_stderr_and_the_exit_code() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert_eq!(
            fixture
                .run(&["new-session", "-d", "-s", "diagnostics"])
                .status
                .code(),
            Some(0)
        );
        let directory = source_directory(&fixture, "diagnostics");
        let bad = write_source(&directory, "bad.conf", "wibble\n");
        let missing = directory
            .join("missing.conf")
            .to_str()
            .expect("UTF-8 missing path")
            .to_owned();

        let pattern = directory.join("bad*.conf");
        let globbed = fixture.run(&[
            "source-file",
            pattern.to_str().expect("UTF-8 source glob pattern"),
        ]);
        assert_eq!(globbed.status.code(), Some(1));
        assert_eq!(
            globbed.stdout,
            format!("{bad}:1: unknown command: wibble\n").into_bytes()
        );
        assert!(globbed.stderr.is_empty());

        let quiet_miss = fixture.run(&["source-file", "-q", &missing]);
        assert_eq!(quiet_miss.status.code(), Some(0));
        assert!(quiet_miss.stdout.is_empty());
        assert!(quiet_miss.stderr.is_empty());

        let loud_miss = fixture.run(&["source-file", &missing]);
        assert_eq!(loud_miss.status.code(), Some(1));
        assert!(loud_miss.stdout.is_empty());
        assert_eq!(
            loud_miss.stderr,
            format!("No such file or directory: {missing}\n").into_bytes()
        );

        let mixed = fixture.run(&["source-file", &bad, &missing]);
        assert_eq!(mixed.status.code(), Some(1));
        assert_eq!(
            mixed.stdout,
            format!("{bad}:1: unknown command: wibble\n").into_bytes()
        );
        assert_eq!(
            mixed.stderr,
            format!("No such file or directory: {missing}\n").into_bytes()
        );

        let quiet_mixed = fixture.run(&["source-file", "-q", &bad, &missing]);
        assert_eq!(quiet_mixed.status.code(), Some(1));
        assert_eq!(
            quiet_mixed.stdout,
            format!("{bad}:1: unknown command: wibble\n").into_bytes()
        );
        assert!(quiet_mixed.stderr.is_empty());

        let leaf = write_source(&directory, "leaf.conf", "wibble\nwibble\nblorp\n");
        let entry = write_source(&directory, "entry.conf", &format!("source-file {leaf}\n"));
        let second = write_source(&directory, "second.conf", "flurb\n");
        let nested = fixture.run(&["source-file", &entry, &second]);
        assert_eq!(nested.status.code(), Some(1));
        assert_eq!(
            nested.stdout,
            format!(
                "{leaf}:1: unknown command: wibble\n\
                 {second}:1: unknown command: flurb\n"
            )
            .into_bytes()
        );
        assert!(nested.stderr.is_empty());

        let verbose_leaf = write_source(
            &directory,
            "verbose-leaf.conf",
            "set-option -g @verbose-loaded yes\n",
        );
        let verbose_entry = write_source(
            &directory,
            "verbose.conf",
            &format!("source-file -v {verbose_leaf}\nset-option -g @loaded yes\n"),
        );
        let verbose = fixture.run(&["source-file", &verbose_entry]);
        assert_eq!(verbose.status.code(), Some(0));
        assert_eq!(
            verbose.stdout,
            format!("{verbose_leaf}:1: set-option -g @verbose-loaded yes\n").into_bytes()
        );
        assert!(verbose.stderr.is_empty());
        let loaded = fixture.run(&["show-options", "-gqv", "@loaded"]);
        assert_eq!(loaded.status.code(), Some(0));
        assert_eq!(loaded.stdout, b"yes\n");

        let chained = fixture.run(&[
            "source-file",
            &bad,
            ";",
            "display-message",
            "-p",
            "after-the-diagnostic",
        ]);
        assert_eq!(chained.status.code(), Some(1));
        assert_eq!(
            chained.stdout,
            format!("{bad}:1: unknown command: wibble\nafter-the-diagnostic\n").into_bytes()
        );
        assert!(chained.stderr.is_empty());

        let stopped = fixture.run(&[
            "kill-window",
            "-t",
            "nosuchwindow",
            ";",
            "display-message",
            "-p",
            "never-runs",
        ]);
        assert_eq!(stopped.status.code(), Some(1));
        assert!(stopped.stdout.is_empty());
        assert_eq!(stopped.stderr, b"can't find window: nosuchwindow\n");
    }

    #[test]
    fn source_file_replayed_output_batches_top_level_paths() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert_eq!(
            fixture
                .run(&["new-session", "-d", "-s", "output"])
                .status
                .code(),
            Some(0)
        );
        let directory = source_directory(&fixture, "replayed-output");
        let child = write_source(
            &directory,
            "10-child.conf",
            "display-message -p CHILD_ONE\nlist-sessions -F CHILD_TWO\n",
        );
        let root = write_source(
            &directory,
            "20-root.conf",
            &format!(
                "display-message -p ROOT_ONE\n\
                 source-file -v {child}\n\
                 display-message -p ROOT_TWO\n"
            ),
        );
        let child_verbose = format!(
            "{child}:1: display-message -p CHILD_ONE\n\
             {child}:2: list-sessions -F CHILD_TWO"
        );
        let root_verbose = format!(
            "{root}:1: display-message -p ROOT_ONE\n\
             {root}:2: source-file -v {child}\n\
             {root}:3: display-message -p ROOT_TWO"
        );
        let child_replay = "CHILD_ONE\nCHILD_TWO";
        let root_replay = format!("ROOT_ONE\n{child_verbose}\n{child_replay}\nROOT_TWO");
        let nested_expected = format!("{root_verbose}\n{root_replay}\n");
        let aggregate_expected =
            format!("{child_verbose}\n{root_verbose}\n{child_replay}\n{root_replay}\n");

        let nested = fixture.run(&["source-file", "-v", &root]);
        assert_eq!(nested.status.code(), Some(0));
        assert_eq!(nested.stdout, nested_expected.into_bytes());
        assert!(nested.stderr.is_empty());

        let declared = fixture.run(&["source-file", "-v", &child, &root]);
        assert_eq!(declared.status.code(), Some(0));
        assert_eq!(declared.stdout, aggregate_expected.as_bytes());
        assert!(declared.stderr.is_empty());

        let glob = directory.join("*.conf").display().to_string();
        let globbed = fixture.run(&["source-file", "-v", &glob]);
        assert_eq!(globbed.status.code(), Some(0));
        assert_eq!(globbed.stdout, aggregate_expected.as_bytes());
        assert!(globbed.stderr.is_empty());

        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[90]",
                    &format!("indirect=source-file -v {child}"),
                ])
                .status
                .success()
        );
        let alias_root = write_source(
            &directory,
            "alias-root.conf",
            "display-message -p ALIAS_BEFORE\nindirect\ndisplay-message -p ALIAS_AFTER\n",
        );
        let aliased = fixture.run(&["source-file", &alias_root]);
        assert_eq!(aliased.status.code(), Some(0));
        assert_eq!(
            aliased.stdout,
            format!("ALIAS_BEFORE\n{child_verbose}\n{child_replay}\nALIAS_AFTER\n").into_bytes()
        );
        assert!(aliased.stderr.is_empty());

        let conditional_root = write_source(
            &directory,
            "conditional-root.conf",
            &format!(
                "display-message -p CONDITIONAL_BEFORE\n\
                 if-shell -F 1 'source-file -v {child}'\n\
                 display-message -p CONDITIONAL_AFTER\n"
            ),
        );
        let conditional = fixture.run(&["source-file", &conditional_root]);
        assert_eq!(conditional.status.code(), Some(0));
        assert_eq!(
            conditional.stdout,
            format!("CONDITIONAL_BEFORE\n{child_verbose}\n{child_replay}\nCONDITIONAL_AFTER\n")
                .into_bytes()
        );
        assert!(conditional.stderr.is_empty());

        let hook_child = write_source(
            &directory,
            "hook-child.conf",
            "list-sessions -F HOOK_CHILD\n",
        );
        let hook_root = write_source(
            &directory,
            "hook-root.conf",
            "display-message -p HOOK_TRIGGER\nlist-sessions -F HOOK_LATER\n",
        );
        assert!(
            fixture
                .run(&[
                    "set-hook",
                    "-g",
                    "after-display-message",
                    &format!("source-file {hook_child}"),
                ])
                .status
                .success()
        );
        let hooked = fixture.run(&["source-file", &hook_root]);
        assert_eq!(hooked.status.code(), Some(0));
        assert_eq!(hooked.stdout, b"HOOK_TRIGGER\nHOOK_CHILD\nHOOK_LATER\n");
        assert!(hooked.stderr.is_empty());
        assert!(
            fixture
                .run(&["set-hook", "-gu", "after-display-message"])
                .status
                .success()
        );

        let indirect_error_child = write_source(
            &directory,
            "indirect-error-child.conf",
            "display-message -p BEFORE_CHILD\n\
             kill-session -t missing-indirect\n\
             display-message -p AFTER_CHILD\n",
        );
        let indirect_error_root = write_source(
            &directory,
            "indirect-error-root.conf",
            &format!(
                "display-message -p ROOT_BEFORE\n\
                 if-shell -F 1 'source-file {indirect_error_child}'\n\
                 display-message -p ROOT_AFTER\n"
            ),
        );
        let indirect_error = fixture.run(&["source-file", &indirect_error_root]);
        assert_eq!(indirect_error.status.code(), Some(1));
        assert_eq!(
            indirect_error.stdout,
            b"ROOT_BEFORE\nBEFORE_CHILD\nAFTER_CHILD\nROOT_AFTER\n"
        );
        assert_eq!(
            indirect_error.stderr,
            b"can't find session: missing-indirect\n"
        );

        let ordered_error_child = write_source(
            &directory,
            "ordered-error-child.conf",
            "kill-session -t missing-B\n",
        );
        let ordered_error_root = write_source(
            &directory,
            "ordered-error-root.conf",
            &format!(
                "kill-session -t missing-A\n\
                 if-shell -F 1 'source-file {ordered_error_child}'\n\
                 kill-session -t missing-C\n"
            ),
        );
        let ordered_error = fixture.run(&["source-file", &ordered_error_root]);
        assert_eq!(ordered_error.status.code(), Some(1));
        assert!(ordered_error.stdout.is_empty());
        assert_eq!(
            ordered_error.stderr,
            b"can't find session: missing-A\n\
              can't find session: missing-B\n\
              can't find session: missing-C\n"
        );

        let invalid_parse = write_source(
            &directory,
            "invalid-parse.conf",
            "display-message -p CHILD_SHOULD_NOT_RUN\nset @bad \\400\n",
        );
        let diagnostic = format!("{invalid_parse}:2: invalid octal escape");
        let direct_parse = write_source(
            &directory,
            "direct-parse.conf",
            &format!(
                "display-message -p ROOT_BEFORE\n\
                 source-file {invalid_parse}\n\
                 display-message -p ROOT_AFTER\n"
            ),
        );
        let direct_diagnostic = fixture.run(&["source-file", "-v", &direct_parse]);
        assert_eq!(direct_diagnostic.status.code(), Some(1));
        assert_eq!(
            direct_diagnostic.stdout,
            format!(
                "{direct_parse}:1: display-message -p ROOT_BEFORE\n\
                 {direct_parse}:2: source-file {invalid_parse}\n\
                 {direct_parse}:3: display-message -p ROOT_AFTER\n\
                 ROOT_BEFORE\n{diagnostic}\nROOT_AFTER\n"
            )
            .into_bytes()
        );
        assert!(direct_diagnostic.stderr.is_empty());

        let conditional_parse = write_source(
            &directory,
            "conditional-parse.conf",
            &format!(
                "display-message -p ROOT_BEFORE\n\
                 if-shell -F 1 'source-file {invalid_parse}'\n\
                 display-message -p ROOT_AFTER\n"
            ),
        );
        let conditional_diagnostic = fixture.run(&["source-file", &conditional_parse]);
        assert_eq!(conditional_diagnostic.status.code(), Some(1));
        assert_eq!(
            conditional_diagnostic.stdout,
            format!("ROOT_BEFORE\n{diagnostic}\nROOT_AFTER\n").into_bytes()
        );
        assert!(conditional_diagnostic.stderr.is_empty());

        let diagnostic_good = write_source(
            &directory,
            "diagnostic-good.conf",
            "display-message -p GOOD_DIAGNOSTIC_OUTPUT\n",
        );
        let diagnostic_later = write_source(
            &directory,
            "diagnostic-later.conf",
            "display-message -p LATER_DIAGNOSTIC_OUTPUT\n",
        );
        let top_level_diagnostic = fixture.run(&[
            "source-file",
            "-v",
            &diagnostic_good,
            &invalid_parse,
            &diagnostic_later,
        ]);
        assert_eq!(top_level_diagnostic.status.code(), Some(1));
        assert_eq!(
            top_level_diagnostic.stdout,
            format!(
                "{diagnostic_good}:1: display-message -p GOOD_DIAGNOSTIC_OUTPUT\n\
                 {diagnostic_later}:1: display-message -p LATER_DIAGNOSTIC_OUTPUT\n\
                 GOOD_DIAGNOSTIC_OUTPUT\nLATER_DIAGNOSTIC_OUTPUT\n{diagnostic}\n"
            )
            .into_bytes()
        );
        assert!(top_level_diagnostic.stderr.is_empty());

        let nested_multi_parse = write_source(
            &directory,
            "nested-multi-parse.conf",
            &format!(
                "display-message -p ROOT_BEFORE\n\
                 source-file -v {diagnostic_good} {invalid_parse} {diagnostic_later}\n\
                 display-message -p ROOT_AFTER\n"
            ),
        );
        let nested_multi_diagnostic = fixture.run(&["source-file", "-v", &nested_multi_parse]);
        assert_eq!(nested_multi_diagnostic.status.code(), Some(1));
        assert_eq!(
            nested_multi_diagnostic.stdout,
            format!(
                "{nested_multi_parse}:1: display-message -p ROOT_BEFORE\n\
                 {nested_multi_parse}:2: source-file -v {diagnostic_good} {invalid_parse} {diagnostic_later}\n\
                 {nested_multi_parse}:3: display-message -p ROOT_AFTER\n\
                 ROOT_BEFORE\n\
                 {diagnostic_good}:1: display-message -p GOOD_DIAGNOSTIC_OUTPUT\n\
                 {diagnostic_later}:1: display-message -p LATER_DIAGNOSTIC_OUTPUT\n\
                 GOOD_DIAGNOSTIC_OUTPUT\nLATER_DIAGNOSTIC_OUTPUT\n{diagnostic}\nROOT_AFTER\n"
            )
            .into_bytes()
        );
        assert!(nested_multi_diagnostic.stderr.is_empty());

        let unknown_command = write_source(&directory, "unknown-command.conf", "wibble\n");
        let nested_unknown = write_source(
            &directory,
            "nested-unknown.conf",
            &format!(
                "display-message -p ROOT_BEFORE\n\
                 source-file {unknown_command}\n\
                 display-message -p ROOT_AFTER\n"
            ),
        );
        let unknown_diagnostic = fixture.run(&["source-file", "-v", &nested_unknown]);
        assert_eq!(unknown_diagnostic.status.code(), Some(1));
        assert_eq!(
            unknown_diagnostic.stdout,
            format!(
                "{nested_unknown}:1: display-message -p ROOT_BEFORE\n\
                 {nested_unknown}:2: source-file {unknown_command}\n\
                 {nested_unknown}:3: display-message -p ROOT_AFTER\n\
                 ROOT_BEFORE\n\
                 {unknown_command}:1: unknown command: wibble\n\
                 ROOT_AFTER\n"
            )
            .into_bytes()
        );
        assert!(unknown_diagnostic.stderr.is_empty());

        let good = write_source(
            &directory,
            "30-good.conf",
            "display-message -p GOOD_OUTPUT\n",
        );
        let unreadable = directory.join("middle-directory");
        std::fs::create_dir(&unreadable).expect("unreadable source directory");
        let later = write_source(
            &directory,
            "40-later.conf",
            "display-message -p LATER_OUTPUT\n",
        );
        let read_error = std::fs::read_to_string(&unreadable)
            .expect_err("reading the source directory must fail");
        let continued = fixture.run(&[
            "source-file",
            &good,
            unreadable.to_str().expect("UTF-8 unreadable path"),
            &later,
        ]);
        assert_eq!(continued.status.code(), Some(1));
        assert_eq!(continued.stdout, b"GOOD_OUTPUT\nLATER_OUTPUT\n");
        assert_eq!(
            continued.stderr,
            format!("{read_error}: {}\n", unreadable.display()).into_bytes()
        );
    }

    #[test]
    fn source_file_replayed_runtime_errors_are_bare_and_propagate_outward() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert_eq!(
            fixture
                .run(&["new-session", "-d", "-s", "replayed-errors"])
                .status
                .code(),
            Some(0)
        );
        let directory = source_directory(&fixture, "replayed-errors");
        let runtime = write_source(
            &directory,
            "runtime.conf",
            "kill-session -t missing-runtime\n\
             set-option -g nonexistent-option value\n\
             set-environment -g \"\" value\n\
             set-option -g @runtime-after yes\n",
        );

        let replayed = fixture.run(&["source-file", &runtime]);
        assert_eq!(replayed.status.code(), Some(1));
        assert!(replayed.stdout.is_empty());
        assert_eq!(
            replayed.stderr,
            b"can't find session: missing-runtime\ninvalid option: nonexistent-option\nempty variable name\n"
        );
        let after = fixture.run(&["show-options", "-gqv", "@runtime-after"]);
        assert_eq!(after.status.code(), Some(0));
        assert_eq!(after.stdout, b"yes\n");

        let inner = write_source(
            &directory,
            "inner.conf",
            "kill-session -t missing-inner\nset-option -g @inner-after yes\n",
        );
        let outer = write_source(
            &directory,
            "outer.conf",
            &format!("source-file '{inner}'\nset-option -g @outer-after yes\n"),
        );
        let nested = fixture.run(&["source-file", &outer]);
        assert_eq!(nested.status.code(), Some(1));
        assert!(nested.stdout.is_empty());
        assert_eq!(nested.stderr, b"can't find session: missing-inner\n");
        for option in ["@inner-after", "@outer-after"] {
            let after = fixture.run(&["show-options", "-gqv", option]);
            assert_eq!(after.status.code(), Some(0));
            assert_eq!(after.stdout, b"yes\n");
        }

        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[100]",
                    "source=new-window -t missing:",
                ])
                .status
                .success()
        );
        let alias_root = write_source(
            &directory,
            "source-alias.conf",
            "source ignored.conf\nset-option -g @source-alias-after yes\n",
        );
        let aliased = fixture.run(&["source-file", &alias_root]);
        assert_eq!(aliased.status.code(), Some(1));
        assert!(aliased.stdout.is_empty());
        assert_eq!(aliased.stderr, b"can't find session: missing\n");
        assert_eq!(
            fixture
                .run(&["show-options", "-gqv", "@source-alias-after"])
                .stdout,
            b"yes\n"
        );

        let hook_runtime = write_source(
            &directory,
            "hook-runtime.conf",
            "display-message -p BEFORE\n\
             kill-session -t missing-runtime\n\
             display-message -p AFTER\n\
             list-sessions -F 'LIST_#{session_name}'\n",
        );
        assert!(
            fixture
                .run(&["set-hook", "-g", "command-error", "display-message -p HOOK",])
                .status
                .success()
        );
        let hooked = fixture.run(&["source-file", &hook_runtime]);
        assert_eq!(hooked.status.code(), Some(1));
        assert_eq!(
            hooked.stdout,
            b"BEFORE\nHOOK\nAFTER\nLIST_replayed-errors\n"
        );
        assert_eq!(hooked.stderr, b"can't find session: missing-runtime\n");
    }

    fn write_source_chain(directory: &Path, invocations: usize) -> String {
        let leaf = directory.join("leaf.conf");
        write_source_chain_with_deepest(
            directory,
            invocations,
            &format!(
                "source-file '{}'\nsource-file -q '{}'\n",
                leaf.display(),
                leaf.display()
            ),
        )
    }

    fn write_source_chain_with_deepest(
        directory: &Path,
        invocations: usize,
        deepest: &str,
    ) -> String {
        for level in 1..=invocations {
            let nested = if level < invocations {
                format!(
                    "source-file '{}'\n",
                    directory.join(format!("f{}.conf", level + 1)).display()
                )
            } else {
                deepest.to_owned()
            };
            let body =
                format!("set-option -g @depth {level}\n{nested}set-option -g @after{level} yes\n");
            write_source(directory, &format!("f{level}.conf"), &body);
        }
        write_source(
            directory,
            "leaf.conf",
            &format!("set-option -g @leaf{invocations} yes\n"),
        );
        directory
            .join("f1.conf")
            .to_str()
            .expect("UTF-8 chain entry")
            .to_owned()
    }

    #[test]
    fn source_file_allows_fifty_invocations_and_refuses_the_fifty_first() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert_eq!(
            fixture
                .run(&["new-session", "-d", "-s", "depth"])
                .status
                .code(),
            Some(0)
        );

        let allowed = source_directory(&fixture, "depth-allowed");
        let entry = write_source_chain(&allowed, 49);
        let sourced = fixture.run(&["source-file", &entry]);
        assert_eq!(sourced.status.code(), Some(0));
        assert!(sourced.stdout.is_empty());
        assert!(sourced.stderr.is_empty());
        assert_eq!(
            fixture.run(&["show-options", "-gv", "@depth"]).stdout,
            b"49\n"
        );
        assert_eq!(
            fixture.run(&["show-options", "-gv", "@leaf49"]).stdout,
            b"yes\n"
        );

        let refused = source_directory(&fixture, "depth-refused");
        let entry = write_source_chain(&refused, 50);
        let sourced = fixture.run(&["source-file", &entry]);
        assert_eq!(sourced.status.code(), Some(1));
        assert!(sourced.stdout.is_empty());
        assert_eq!(
            sourced.stderr,
            b"too many nested files\ntoo many nested files\n"
        );
        assert_eq!(
            fixture.run(&["show-options", "-gv", "@depth"]).stdout,
            b"50\n"
        );
        assert_eq!(
            fixture.run(&["show-options", "-gv", "@after50"]).stdout,
            b"yes\n"
        );
        assert!(
            fixture
                .run(&["show-options", "-gqv", "@leaf50"])
                .stdout
                .is_empty()
        );
    }

    #[test]
    fn source_file_diagnoses_a_malformed_refused_invocation_instead_of_the_depth_limit() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert_eq!(
            fixture
                .run(&["new-session", "-d", "-s", "malformed-depth"])
                .status
                .code(),
            Some(0)
        );

        for (name, deepest, message) in [
            (
                "depth-no-path",
                "source-file\n",
                "command source-file: too few arguments (need at least 1)",
            ),
            (
                "depth-bad-flag",
                "source-file -z leaf.conf\n",
                "command source-file: unknown flag -z",
            ),
        ] {
            let directory = source_directory(&fixture, name);
            let entry = write_source_chain_with_deepest(&directory, 50, deepest);
            let deepest_path = directory
                .join("f50.conf")
                .to_str()
                .expect("UTF-8 chain leaf")
                .to_owned();
            let sourced = fixture.run(&["source-file", &entry]);
            assert_eq!(sourced.status.code(), Some(1));
            assert_eq!(
                sourced.stdout,
                format!("{deepest_path}:2: {message}\n").into_bytes()
            );
            assert!(sourced.stderr.is_empty());
        }
    }

    #[test]
    fn source_file_treats_the_default_config_as_an_ordinary_ordered_path() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let config_home = source_directory(&fixture, "runtime-default-xdg");
        let isolated_home = source_directory(&fixture, "runtime-default-home");
        let default_config = config_home.join("zz").join("mux.conf");
        std::fs::create_dir_all(default_config.parent().expect("default config parent"))
            .expect("default config directory");
        std::fs::write(&default_config, b"wibble\n").expect("write invalid default mux config");
        let started = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .arg("start-server")
            .output()
            .expect("run zz start-server");
        assert_eq!(started.status.code(), Some(0));

        let sourced = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .arg("source-file")
            .arg(&default_config)
            .output()
            .expect("run zz source-file on the default config");
        assert_eq!(sourced.status.code(), Some(1));
        assert_eq!(
            sourced.stdout,
            format!("{}:1: unknown command: wibble\n", default_config.display()).into_bytes()
        );
        assert!(sourced.stderr.is_empty());

        std::fs::write(&default_config, b"set-option -ag @default_order D\n")
            .expect("write ordered default mux config");
        let after = isolated_home.join("after.conf");
        std::fs::write(&after, b"set-option -ag @default_order A\n")
            .expect("write ordered after config");
        let initialized = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .args(["set-option", "-g", "@default_order", ""])
            .output()
            .expect("initialize source order");
        assert_eq!(initialized.status.code(), Some(0));
        assert!(initialized.stdout.is_empty());
        assert!(initialized.stderr.is_empty());
        let missing = isolated_home.join("missing.conf");
        let ordered = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .arg("source-file")
            .arg("-v")
            .arg(&default_config)
            .arg(&missing)
            .arg(&after)
            .arg(&default_config)
            .output()
            .expect("run ordered default source");
        assert_eq!(ordered.status.code(), Some(1));
        assert_eq!(
            ordered.stdout,
            format!(
                "{}:1: set-option -ag @default_order D\n\
                 {}:1: set-option -ag @default_order A\n\
                 {}:1: set-option -ag @default_order D\n",
                default_config.display(),
                after.display(),
                default_config.display(),
            )
            .into_bytes()
        );
        assert_eq!(
            ordered.stderr,
            format!("No such file or directory: {}\n", missing.display()).into_bytes()
        );
        let order = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .args(["show-options", "-gqv", "@default_order"])
            .output()
            .expect("show default source order");
        assert_eq!(order.status.code(), Some(0));
        assert_eq!(order.stdout, b"DAD\n");
        assert!(order.stderr.is_empty());
    }

    #[test]
    fn reload_config_of_the_default_config_stays_silent_and_resets_keys() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let config_home = source_directory(&fixture, "reload-default-xdg");
        let isolated_home = source_directory(&fixture, "reload-default-home");
        let default_config = config_home.join("zz").join("mux.conf");
        std::fs::create_dir_all(default_config.parent().expect("default config parent"))
            .expect("default config directory");
        let caller_cwd = std::fs::canonicalize(source_directory(
            &fixture,
            "default caller cwd [literal]*? with spaces",
        ))
        .expect("canonical default source caller cwd");
        std::fs::write(
            caller_cwd.join("leaf.conf"),
            b"set-option -g @default_nested_client_cwd caller-root\n",
        )
        .expect("write default source caller-root leaf");
        std::fs::write(
            default_config
                .parent()
                .expect("default config parent")
                .join("leaf.conf"),
            b"set-option -g @default_nested_client_cwd containing-file-decoy\n",
        )
        .expect("write default source containing-file decoy");
        std::fs::write(
            &default_config,
            b"set-option -g @default_nested_replay_started yes\nsource-file leaf.conf\nbind-key -T reload-loaded z display-message loaded\nwibble\n",
        )
        .expect("write default mux config");
        let started = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .arg("start-server")
            .output()
            .expect("run zz start-server");
        assert_eq!(started.status.code(), Some(0));
        let bound = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .args([
                "bind-key",
                "-T",
                "reload-stale",
                "z",
                "display-message",
                "stale",
            ])
            .output()
            .expect("bind stale reload key");
        assert_eq!(bound.status.code(), Some(0));
        assert!(bound.stdout.is_empty());
        assert!(bound.stderr.is_empty());

        let reloaded = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .current_dir(&caller_cwd)
            .arg("reload-config")
            .output()
            .expect("run zz reload-config from the caller cwd");
        assert_eq!(reloaded.status.code(), Some(0));
        assert!(reloaded.stdout.is_empty());
        assert!(reloaded.stderr.is_empty());
        let replay_started = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .current_dir(&caller_cwd)
            .args(["show-options", "-gqv", "@default_nested_replay_started"])
            .output()
            .expect("show direct reload replay marker");
        assert_eq!(replay_started.status.code(), Some(0));
        assert!(replay_started.stdout.is_empty());
        assert!(replay_started.stderr.is_empty());
        let nested_base = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .current_dir(&caller_cwd)
            .args(["show-options", "-gqv", "@default_nested_client_cwd"])
            .output()
            .expect("show direct reload nested base");
        assert_eq!(nested_base.status.code(), Some(0));
        assert!(nested_base.stdout.is_empty());
        assert!(nested_base.stderr.is_empty());
        let stale = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .args(["list-keys", "-T", "reload-stale", "z"])
            .output()
            .expect("query stale reload key");
        assert_eq!(stale.status.code(), Some(1));
        assert!(stale.stdout.is_empty());
        assert_eq!(stale.stderr, b"table reload-stale doesn't exist\n");
        let loaded = fixture
            .command()
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &isolated_home)
            .args([
                "list-keys",
                "-1",
                "-T",
                "reload-loaded",
                "-F",
                "#{key_table}:#{key_string}",
                "z",
            ])
            .output()
            .expect("query reloaded key");
        assert_eq!(loaded.status.code(), Some(1));
        assert!(loaded.stdout.is_empty());
        assert_eq!(loaded.stderr, b"table reload-loaded doesn't exist\n");
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
    fn attaching_new_session_inside_a_later_alias_enters_the_alternate_screen() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "seed"])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[40]",
                    "go=new-session -s pty-attached ; split-window -h",
                ])
                .status
                .success()
        );
        let Ok((mut master, slave)) = open_pty() else {
            return;
        };
        let stdin = slave.try_clone().expect("clone pty stdin");
        let stdout = slave.try_clone().expect("clone pty stdout");
        let mut command = fixture.command();
        command
            .args(["new-session", "-d", "-s", "chain-before", ";", "go"])
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
        assert_eq!(listed.stdout, b"chain-before\npty-attached\nseed\n");
        assert!(listed.stderr.is_empty());
        let panes = fixture.run(&["list-panes", "-t", "pty-attached", "-F", "#{pane_index}"]);
        assert_eq!(panes.status.code(), Some(0));
        assert_eq!(panes.stdout, b"0\n1\n");
        assert!(panes.stderr.is_empty());
    }

    #[test]
    fn attached_tui_renders_daemon_authored_styled_status_labels() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        for arguments in [
            ["new-session", "-d", "-s", "styled", "-n", "main"].as_slice(),
            ["set", "-t", "styled", "status-left", "#[fg=red,bold]LEFT"].as_slice(),
            ["set", "-t", "styled", "status-right", "#[bg=blue]RIGHT"].as_slice(),
            [
                "setw",
                "-t",
                "styled:0",
                "window-status-current-format",
                "#[underscore]CUSTOM",
            ]
            .as_slice(),
        ] {
            let output = fixture.run(arguments);
            assert_eq!(
                output.status.code(),
                Some(0),
                "stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let Ok((mut master, slave)) = open_pty() else {
            return;
        };
        rustix::io::ioctl_fionbio(&master, true).expect("set pty master nonblocking");
        let stdin = slave.try_clone().expect("clone pty stdin");
        let stdout = slave.try_clone().expect("clone pty stdout");
        let mut child = fixture
            .command()
            .args(["attach-session", "-t", "styled"])
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(slave))
            .spawn()
            .expect("spawn styled TUI attach");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut captured = Vec::new();
        let mut early_status = None;
        let rendered = loop {
            let mut buffer = [0_u8; 4096];
            match master.read(&mut buffer) {
                Ok(0) => {}
                Ok(count) => captured.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
            if [
                b"LEFT".as_slice(),
                b"RIGHT".as_slice(),
                b"CUSTOM".as_slice(),
            ]
            .into_iter()
            .all(|needle| {
                captured
                    .windows(needle.len())
                    .any(|window| window == needle)
            }) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            if let Some(status) = child.try_wait().expect("poll styled TUI attach") {
                early_status = Some(status);
                break false;
            }
            thread::sleep(Duration::from_millis(5));
        };

        let _ = child.kill();
        let _ = child.wait();
        drop(master);

        assert!(
            rendered,
            "child exited early={early_status:?}; pty output={}",
            String::from_utf8_lossy(&captured),
        );
        assert!(!captured.windows(2).any(|window| window == b"#["));
        assert!(!captured.windows(6).any(|window| window == b"0:main"));
    }

    fn capture_tui_until(
        fixture: &Fixture,
        attach: &[&str],
        needles: &[&[u8]],
    ) -> (bool, Vec<u8>, Option<std::process::ExitStatus>) {
        let mut command = fixture.command();
        command.args(attach);
        capture_command_until(command, needles)
    }

    fn capture_command_until(
        mut command: Command,
        needles: &[&[u8]],
    ) -> (bool, Vec<u8>, Option<std::process::ExitStatus>) {
        let Ok((mut master, slave)) = open_pty() else {
            return (true, Vec::new(), None);
        };
        rustix::io::ioctl_fionbio(&master, true).expect("set pty master nonblocking");
        let stdin = slave.try_clone().expect("clone pty stdin");
        let stdout = slave.try_clone().expect("clone pty stdout");
        let mut child = command
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(slave))
            .spawn()
            .expect("spawn TUI attach");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut captured = Vec::new();
        let mut early_status = None;
        let rendered = loop {
            let mut buffer = [0_u8; 4096];
            match master.read(&mut buffer) {
                Ok(0) => {}
                Ok(count) => captured.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
            if needles.iter().all(|needle| {
                captured
                    .windows(needle.len())
                    .any(|window| window == *needle)
            }) {
                drain_settled_output(&mut master, &mut captured);
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            if let Some(status) = child.try_wait().expect("poll TUI attach") {
                early_status = Some(status);
                break false;
            }
            thread::sleep(Duration::from_millis(5));
        };
        let _ = child.kill();
        let _ = child.wait();
        drop(master);
        (rendered, captured, early_status)
    }

    fn drain_settled_output(master: &mut File, captured: &mut Vec<u8>) {
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut quiet_since = Instant::now();
        while Instant::now() < deadline && quiet_since.elapsed() < Duration::from_millis(150) {
            let mut buffer = [0_u8; 4096];
            match master.read(&mut buffer) {
                Ok(count) if count > 0 => {
                    captured.extend_from_slice(&buffer[..count]);
                    quiet_since = Instant::now();
                }
                _ => thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    fn visible_text_after(captured: &[u8], cursor: &[u8]) -> Vec<String> {
        let mut collected = Vec::new();
        let mut search = 0;
        while let Some(offset) = captured[search..]
            .windows(cursor.len())
            .position(|window| window == cursor)
        {
            let mut index = search + offset + cursor.len();
            search = index;
            let mut text = Vec::new();
            while index < captured.len() && text.len() < 24 {
                if captured[index] == 0x1b {
                    let Some(end) = captured[index..].iter().position(u8::is_ascii_alphabetic)
                    else {
                        break;
                    };
                    if captured[index + end] == b'H' {
                        break;
                    }
                    index += end + 1;
                    continue;
                }
                text.push(captured[index]);
                index += 1;
            }
            collected.push(String::from_utf8_lossy(&text).into_owned());
        }
        collected
    }

    #[test]
    fn styled_multi_row_status_renders_two_rows_without_literal_markers() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        for arguments in [
            ["new-session", "-d", "-s", "multirow", "-n", "main"].as_slice(),
            ["set", "-g", "status", "2"].as_slice(),
            ["set", "-g", "status-format[1]", "#[fg=red,bold]ROWTWO"].as_slice(),
        ] {
            let output = fixture.run(arguments);
            assert_eq!(
                output.status.code(),
                Some(0),
                "stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let (rendered, captured, early_status) = capture_tui_until(
            &fixture,
            &["attach-session", "-t", "multirow"],
            &[b"[multirow]", b"ROWTWO"],
        );
        assert!(
            rendered,
            "child exited early={early_status:?}; pty output={}",
            String::from_utf8_lossy(&captured),
        );
        assert!(!captured.windows(2).any(|window| window == b"#["));
        for row in [b"\x1b[23;30H".as_slice(), b"\x1b[24;30H".as_slice()] {
            assert!(
                captured.windows(row.len()).any(|window| window == row),
                "both status rows paint above the last line: {}",
                String::from_utf8_lossy(&captured),
            );
        }
        assert!(
            visible_text_after(&captured, b"\x1b[24;30H")
                .iter()
                .any(|text| text.starts_with("ROWTWO")),
            "row 1 carries the second status-format row: {}",
            String::from_utf8_lossy(&captured),
        );
    }

    #[test]
    fn status_position_top_puts_the_status_block_at_row_zero() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        for arguments in [
            ["new-session", "-d", "-s", "toppos", "-n", "main"].as_slice(),
            ["set", "-t", "toppos", "status-position", "top"].as_slice(),
            ["set", "-t", "toppos", "status-left", "TOPMARK"].as_slice(),
        ] {
            let output = fixture.run(arguments);
            assert_eq!(
                output.status.code(),
                Some(0),
                "stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let (rendered, captured, early_status) =
            capture_tui_until(&fixture, &["attach-session", "-t", "toppos"], &[b"TOPMARK"]);
        assert!(
            rendered,
            "child exited early={early_status:?}; pty output={}",
            String::from_utf8_lossy(&captured),
        );
        assert!(!captured.windows(2).any(|window| window == b"#["));
        assert!(
            visible_text_after(&captured, b"\x1b[1;30H")
                .iter()
                .any(|text| text.starts_with("TOPMARK")),
            "the status block owns row zero of the main columns: {}",
            String::from_utf8_lossy(&captured),
        );
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
    fn native_attach_accepts_targets_and_stops_options_at_positional_sessions() {
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

            let positional = fixture.run(&[command, "named", "-@"]);
            assert_eq!(positional.status.code(), Some(1));
            assert!(positional.stdout.is_empty());
            assert_eq!(
                positional.stderr,
                b"zz: usage: zz [--host <name>] attach [--restart-daemon] [-dEr] [-c working-directory] [-f flags] [session]\n"
            );
        }
    }

    #[test]
    fn exact_native_attach_executes_a_valid_command_chain_tail_after_a_positional_session() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "named"]);
        assert_eq!(created.status.code(), Some(0));
        for (command, variable) in [
            ("attach", "NATIVE_ATTACH_TAIL"),
            ("attach-session", "NATIVE_ATTACH_SESSION_TAIL"),
        ] {
            let (rendered, captured, early_status) = capture_tui_until(
                &fixture,
                &[
                    command,
                    "named",
                    ";",
                    "set-environment",
                    "-g",
                    variable,
                    "yes",
                ],
                &[b"\x1b[?1049h"],
            );
            assert!(
                rendered,
                "{command}: child exited early={early_status:?}; pty output={}",
                String::from_utf8_lossy(&captured)
            );

            let value = fixture.run(&["show-environment", "-g", variable]);
            assert_eq!(value.status.code(), Some(0));
            assert_eq!(value.stdout, format!("{variable}=yes\n").as_bytes());
            assert!(value.stderr.is_empty());
        }
    }

    #[test]
    fn native_attach_flag_errors_match_tmux_for_both_spellings() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        for command in ["attach", "attach-session"] {
            for (arguments, expected) in [
                (
                    &["-0"][..],
                    b"command attach-session: unknown flag -0\n".as_slice(),
                ),
                (
                    &["-@"][..],
                    b"command attach-session: invalid flag -@\n".as_slice(),
                ),
                (
                    &["--bogus"][..],
                    b"command attach-session: invalid flag --\n".as_slice(),
                ),
                (
                    &["-?"][..],
                    b"usage: attach-session [-dErx] [-c working-directory] [-f flags] [-t target-session]\n"
                        .as_slice(),
                ),
                (
                    &["-t"][..],
                    b"command attach-session: -t expects an argument\n".as_slice(),
                ),
                (
                    &["-x0"][..],
                    b"command attach-session: unknown flag -0\n".as_slice(),
                ),
                (
                    &["-x"][..],
                    b"unsupported command: attach-session -x\n".as_slice(),
                ),
            ] {
                let mut invocation = vec![command];
                invocation.extend_from_slice(arguments);
                let output = fixture.run(&invocation);
                assert_eq!(output.status.code(), Some(1), "{invocation:?}");
                assert!(output.stdout.is_empty(), "{invocation:?}");
                assert_eq!(output.stderr, expected, "{invocation:?}");
            }
        }
    }

    #[test]
    fn attach_prefix_defers_live_user_aliases_to_the_daemon() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "named"]);
        assert_eq!(created.status.code(), Some(0));

        let unaliased = fixture.run(&["a"]);
        assert_eq!(unaliased.status.code(), Some(1));
        assert!(unaliased.stdout.is_empty());
        assert_eq!(unaliased.stderr, b"open terminal failed: not a terminal\n");

        let configured = fixture.run(&[
            "set-option",
            "-s",
            "command-alias[40]",
            "a=list-sessions -F '#{session_name}'",
        ]);
        assert_eq!(configured.status.code(), Some(0));
        let aliased = fixture.run(&["a"]);
        assert_eq!(aliased.status.code(), Some(0));
        assert_eq!(aliased.stdout, b"named\n");
        assert!(aliased.stderr.is_empty());
    }

    #[test]
    fn exact_attach_aliases_bypass_the_native_attach_wrapper() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "named"])
                .status
                .success()
        );
        for (index, command) in ["attach", "attach-session"].into_iter().enumerate() {
            let marker = format!("{command}-shadow");
            let alias = format!("{command}=display-message -p {marker}");
            let option = format!("command-alias[{}]", 40 + index);
            assert!(
                fixture
                    .run(&["set-option", "-s", &option, &alias])
                    .status
                    .success()
            );
            let output = fixture.run(&[command]);
            assert_eq!(output.status.code(), Some(0));
            assert_eq!(output.stdout, format!("{marker}\n").as_bytes());
            assert!(output.stderr.is_empty());
        }
    }

    #[test]
    fn arbitrary_attach_alias_uses_the_tui_path() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "named"])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[40]",
                    "go=display-message -p before ; attach-session -t named",
                ])
                .status
                .success()
        );
        let output = fixture.run(&["go"]);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"before\n");
        assert_eq!(output.stderr, b"open terminal failed: not a terminal\n");
    }

    #[test]
    fn live_agent_send_aliases_control_stdin_capture() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "agent-stdin"])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[40]",
                    "pipe=display-message -p before ; agent-send -t %0",
                ])
                .status
                .success()
        );
        let aliased = fixture.run_with_stdin(&["pipe"], b"review this\n");
        assert_eq!(aliased.status.code(), Some(1));
        assert_eq!(aliased.stdout, b"before\n");
        assert_eq!(
            aliased.stderr,
            b"target not found: no agent pane in the window holding %0\n"
        );

        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[41]",
                    "agent-send=display-message -p shadow",
                ])
                .status
                .success()
        );
        let shadowed = fixture.run_with_stdin(&["agent-send"], b"must stay unread\n");
        assert_eq!(shadowed.status.code(), Some(0));
        assert_eq!(shadowed.stdout, b"shadow\n");
        assert!(shadowed.stderr.is_empty());
    }

    #[test]
    fn failing_kill_server_alias_does_not_enter_recovery() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "alive"])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[40]",
                    "kill-server=has-session -t missing",
                ])
                .status
                .success()
        );
        let killed = fixture.run(&["kill-server"]);
        assert_eq!(killed.status.code(), Some(1));
        assert!(fixture.socket.exists());
        let alive = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(alive.status.code(), Some(0));
        assert_eq!(alive.stdout, b"alive\n");
        assert!(
            fixture
                .run(&["set-option", "-su", "command-alias[40]"])
                .status
                .success()
        );
    }

    #[test]
    fn prepared_kill_server_stops_the_daemon_without_recovery() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "alive"])
                .status
                .success()
        );
        let killed = fixture.run(&["kill-server"]);
        assert_eq!(killed.status.code(), Some(0));
        assert!(killed.stdout.is_empty());
        assert!(killed.stderr.is_empty());
        let deadline = Instant::now() + Duration::from_secs(2);
        while fixture.socket.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!fixture.socket.exists());
    }

    #[test]
    fn raw_kill_server_flag_ignores_the_live_alias_table() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "alive"])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[40]",
                    "kill-server=display-message -p shadow",
                ])
                .status
                .success()
        );
        let killed = fixture.run(&["--kill-server"]);
        assert_eq!(killed.status.code(), Some(0));
        assert!(killed.stdout.is_empty());
        assert!(killed.stderr.is_empty());
        let deadline = Instant::now() + Duration::from_secs(2);
        while fixture.socket.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!fixture.socket.exists());
    }

    #[test]
    fn cli_prepare_freezes_aliases_for_the_whole_command_chain() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "snapshot"])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[40]",
                    "live=display-message -p old",
                ])
                .status
                .success()
        );
        let frozen = fixture.run(&[
            "set-option",
            "-s",
            "command-alias[40]",
            "live=display-message -p new",
            ";",
            "live",
        ]);
        assert_eq!(frozen.status.code(), Some(0));
        assert_eq!(frozen.stdout, b"old\n");
        assert!(frozen.stderr.is_empty());
        let next = fixture.run(&["live"]);
        assert_eq!(next.status.code(), Some(0));
        assert_eq!(next.stdout, b"new\n");
    }

    #[test]
    fn prepared_cli_chain_rejects_later_alias_parse_errors_before_effects() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "atomic"])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[40]",
                    "broken=display-message \\",
                ])
                .status
                .success()
        );

        let rejected = fixture.run(&[
            "set-environment",
            "-g",
            "CLI_CHAIN_BEFORE",
            "mutated",
            ";",
            "broken",
        ]);
        assert_eq!(rejected.status.code(), Some(1));
        assert!(rejected.stdout.is_empty());
        assert_eq!(rejected.stderr, b"unknown command: broken\n");

        let marker = fixture.run(&["show-environment", "-g", "CLI_CHAIN_BEFORE"]);
        assert_eq!(marker.status.code(), Some(1));
        assert!(marker.stdout.is_empty());
        assert_eq!(marker.stderr, b"unknown variable: CLI_CHAIN_BEFORE\n");

        let alive = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(alive.status.code(), Some(0));
        assert_eq!(alive.stdout, b"atomic\n");
        assert!(alive.stderr.is_empty());
    }

    #[test]
    fn prepared_cli_chain_rejects_later_unaliased_argument_errors_before_effects() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "atomic-unaliased"])
                .status
                .success()
        );

        let cases: &[(&str, &[&str], &[u8])] = &[
            (
                "CLI_CHAIN_INVALID_FLAG",
                &["list-sessions", "-Z"],
                b"command list-sessions: unknown flag -Z\n",
            ),
            (
                "CLI_CHAIN_TOO_MANY",
                &["list-sessions", "extra"],
                b"command list-sessions: too many arguments (need at most 0)\n",
            ),
            (
                "CLI_CHAIN_MISSING_VALUE",
                &["list-sessions", "-F"],
                b"command list-sessions: -F expects an argument\n",
            ),
            (
                "CLI_CHAIN_LATE_ATTACH",
                &["attach", "atomic-unaliased"],
                b"command attach-session: too many arguments (need at most 0)\n",
            ),
            (
                "CLI_CHAIN_LATE_ATTACH_SESSION",
                &["attach-session", "atomic-unaliased"],
                b"command attach-session: too many arguments (need at most 0)\n",
            ),
        ];
        for &(marker, later, expected) in cases {
            let mut arguments = vec!["set-environment", "-g", marker, "mutated", ";"];
            arguments.extend_from_slice(later);
            let rejected = fixture.run(&arguments);
            assert_eq!(rejected.status.code(), Some(1), "{marker}");
            assert!(rejected.stdout.is_empty(), "{marker}");
            assert_eq!(rejected.stderr, expected, "{marker}");

            let marker_output = fixture.run(&["show-environment", "-g", marker]);
            assert_eq!(marker_output.status.code(), Some(1), "{marker}");
            assert!(marker_output.stdout.is_empty(), "{marker}");
            assert_eq!(
                marker_output.stderr,
                format!("unknown variable: {marker}\n").as_bytes(),
                "{marker}"
            );
        }

        let runtime = fixture.run(&[
            "set-environment",
            "-g",
            "CLI_RUNTIME_BEFORE",
            "kept",
            ";",
            "has-session",
            "-t",
            "missing",
            ";",
            "set-environment",
            "-g",
            "CLI_RUNTIME_AFTER",
            "bad",
        ]);
        assert_eq!(runtime.status.code(), Some(1));
        assert!(runtime.stdout.is_empty());
        assert_eq!(runtime.stderr, b"can't find session: missing\n");

        let before = fixture.run(&["show-environment", "-g", "CLI_RUNTIME_BEFORE"]);
        assert_eq!(before.status.code(), Some(0));
        assert_eq!(before.stdout, b"CLI_RUNTIME_BEFORE=kept\n");
        assert!(before.stderr.is_empty());

        let after = fixture.run(&["show-environment", "-g", "CLI_RUNTIME_AFTER"]);
        assert_eq!(after.status.code(), Some(1));
        assert!(after.stdout.is_empty());
        assert_eq!(after.stderr, b"unknown variable: CLI_RUNTIME_AFTER\n");
    }

    #[test]
    fn cold_cli_parse_errors_do_not_start_or_mutate_a_daemon() {
        let cases: &[&[&str]] = &[
            &["new-session", "-d", "-s", "before", ";", "frobnicate"],
            &[
                "new-session",
                "-d",
                "-s",
                "before",
                ";",
                "list-sessions",
                "-Z",
            ],
            &[
                "new-session",
                "-d",
                "-s",
                "before",
                ";",
                "list-sessions",
                "-F",
            ],
            &[
                "new-session",
                "-d",
                "-s",
                "before",
                ";",
                "list-sessions",
                "extra",
            ],
            &["new", "-d", "-s", "before", ";", "lscm", "-Z"],
            &["new-session", "-s", "before", ";", "frobnicate"],
            &["-N", "new-session", "-s", "before", ";", "frobnicate"],
            &["-N", "attach"],
            &["-N", "attach-session"],
            &["attach", ";", "frobnicate"],
            &["attach-session", ";", "frobnicate"],
            &["new-session", "-d", "-s", "before", ";", "clock-mode", "-Z"],
            &[
                "new-session",
                "-d",
                "-s",
                "before",
                ";",
                "suspendc",
                "extra",
            ],
        ];
        for arguments in cases {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let output = fixture.run(arguments);
            assert_missing(&output, &fixture.missing_message());
            fixture.assert_not_started();
        }
    }

    #[test]
    fn cold_cli_valid_builtin_alias_chain_still_starts_and_executes() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let output = fixture.run(&[
            "new",
            "-d",
            "-s",
            "before",
            ";",
            "ls",
            "-F",
            "#{session_name}",
        ]);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"before\n");
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn arbitrary_startup_alias_cannot_trigger_cold_autostart() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        std::fs::write(
            &fixture.config,
            b"set-option -s command-alias[0] 'go=new-session -d -s from-alias'\n",
        )
        .expect("write startup alias");
        let output = fixture.run(&["go"]);
        assert_missing(&output, &fixture.missing_message());
        fixture.assert_not_started();
    }

    #[test]
    fn canonical_startup_alias_shadow_is_prepared_after_config() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        std::fs::write(
            &fixture.config,
            b"set-option -s command-alias[0] 'list-sessions=display-message -p STARTUP_VALID'\n",
        )
        .expect("write startup alias");
        let output = fixture.run(&["new-session", "-d", "-s", "before", ";", "list-sessions"]);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"STARTUP_VALID\n");
        assert!(output.stderr.is_empty());
        let session = fixture.run(&["has-session", "-t", "=before"]);
        assert_eq!(session.status.code(), Some(0));
        assert!(session.stdout.is_empty());
        assert!(session.stderr.is_empty());
    }

    #[test]
    fn canonical_attach_startup_alias_shadow_is_prepared_after_config() {
        for command in ["attach", "attach-session"] {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            std::fs::write(
                &fixture.config,
                format!(
                    "set-option -s command-alias[0] '{command}=display-message -p STARTUP_ATTACH'\n"
                ),
            )
            .expect("write startup alias");
            let output = fixture.run(&[command]);
            assert_eq!(output.status.code(), Some(0));
            assert_eq!(output.stdout, b"STARTUP_ATTACH\n");
            assert!(output.stderr.is_empty());
        }
    }

    #[test]
    fn failed_preflight_handshake_still_runs_the_cold_static_gate() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let listener = std::os::unix::net::UnixListener::bind(&fixture.socket)
            .expect("bind fake preflight listener");
        let socket = fixture.socket.clone();
        let fake = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept preflight client");
            std::fs::remove_file(&socket).expect("unlink fake listener");
            drop(stream);
        });
        let output = fixture.run(&["new-session", "-d", "-s", "before-reset", ";", "frobnicate"]);
        fake.join().expect("join fake listener");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        fixture.assert_not_started();
    }

    #[test]
    fn invalid_startup_alias_shadows_abort_before_cold_effects() {
        for alias in [
            "list-sessions=frobnicate",
            "list-sessions=confirm-before { frobnicate }",
        ] {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            std::fs::write(
                &fixture.config,
                format!("set-option -s command-alias[0] '{alias}'\n"),
            )
            .expect("write startup alias");
            let output = fixture.run(&["new-session", "-d", "-s", "before", ";", "list-sessions"]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, b"unknown command: frobnicate\n");
            fixture.assert_stopped();
        }
    }

    #[test]
    fn tui_handoff_executes_the_prepared_alias_snapshot() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&["new-session", "-d", "-s", "seed"])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-s",
                    "command-alias[40]",
                    "go=display-message -p before ; new-session -s frozen-old",
                ])
                .status
                .success()
        );
        let frozen = fixture.run(&[
            "set-option",
            "-s",
            "command-alias[40]",
            "go=display-message -p changed",
            ";",
            "go",
        ]);
        assert_eq!(frozen.status.code(), Some(1));
        assert_eq!(frozen.stdout, b"before\n");
        assert_eq!(frozen.stderr, b"open terminal failed: not a terminal\n");
        let sessions = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(sessions.status.code(), Some(0));
        assert!(
            !String::from_utf8_lossy(&sessions.stdout)
                .lines()
                .any(|session| session == "frozen-old")
        );
        let changed = fixture.run(&["go"]);
        assert_eq!(changed.status.code(), Some(0));
        assert_eq!(changed.stdout, b"changed\n");
    }

    #[test]
    fn agent_send_prefix_reads_stdin_before_dispatch() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "agent-stdin"]);
        assert_eq!(created.status.code(), Some(0));
        let sent = fixture.run_with_stdin(&["agent-s", "-t", "%0"], b"review this\n");
        assert_eq!(sent.status.code(), Some(1));
        assert!(sent.stdout.is_empty());
        assert_eq!(
            sent.stderr,
            b"target not found: no agent pane in the window holding %0\n"
        );
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
    fn explicit_targetless_attach_autostarts_then_reports_no_sessions() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        std::fs::write(&fixture.config, b"set-environment -g started YES\n")
            .expect("write autostart marker config");
        for command in ["attach", "attach-session"] {
            let output = fixture.run(&[command]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, b"no sessions\n");
            assert!(fixture.socket.exists());
        }

        let started = fixture.run(&["show-environment", "-g", "started"]);
        assert_eq!(started.status.code(), Some(0));
        assert_eq!(started.stdout, b"started=YES\n");
        assert!(started.stderr.is_empty());
    }

    #[test]
    fn attach_restart_daemon_survives_missing_preflight_daemon() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let output = fixture.run(&["attach", "--restart-daemon"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"no sessions\n");
        assert!(fixture.socket.exists());
    }

    #[test]
    fn cargo_launcher_pair_routes_bare_new_and_attach_across_empty_and_existing_daemons() {
        let Ok((master, slave)) = open_pty() else {
            return;
        };
        drop((master, slave));
        let launcher = CargoLauncher::new();
        let cases = [
            ("bare-empty", &[][..], false, &["0"][..]),
            ("bare-existing", &[][..], true, &["existing"][..]),
            (
                "new-empty",
                &["new", "-s", "created"][..],
                false,
                &["created"][..],
            ),
            (
                "new-existing",
                &["new", "-s", "created"][..],
                true,
                &["created", "existing"][..],
            ),
            (
                "attach-existing",
                &["attach", "-t", "existing"][..],
                true,
                &["existing"][..],
            ),
        ];

        for (name, arguments, existing, expected) in cases {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            if existing {
                let created = fixture.run(&["new-session", "-d", "-s", "existing"]);
                assert_eq!(
                    created.status.code(),
                    Some(0),
                    "{name}: {}",
                    String::from_utf8_lossy(&created.stderr)
                );
            }
            let mut command = launcher.command(&fixture);
            command.args(arguments);
            let (rendered, captured, early_status) =
                capture_command_until(command, &[b"\x1b[?1049h"]);
            assert!(
                rendered,
                "{name}: child exited early={early_status:?}; pty output={}",
                String::from_utf8_lossy(&captured)
            );

            let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
            assert_eq!(
                listed.status.code(),
                Some(0),
                "{name}: {}",
                String::from_utf8_lossy(&listed.stderr)
            );
            let output = String::from_utf8_lossy(&listed.stdout);
            let mut actual = output.lines().collect::<Vec<_>>();
            actual.sort_unstable();
            assert_eq!(actual, expected, "{name}");
        }

        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let explicit = launcher.command(&fixture).arg("attach").output().unwrap();
        assert_eq!(explicit.status.code(), Some(1));
        assert!(explicit.stdout.is_empty());
        assert_eq!(explicit.stderr, b"no sessions\n");
        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert!(listed.stdout.is_empty());
    }

    #[test]
    fn explicit_attach_can_fall_back_to_new_session() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(concat!(
                "\"$ZZ_BIN\" -f \"$ZZ_CONF\" -S \"$ZZ_TEST_SOCKET\" attach || ",
                "exec \"$ZZ_BIN\" -f \"$ZZ_CONF\" -S \"$ZZ_TEST_SOCKET\" new-session -s fallback"
            ))
            .env("ZZ_BIN", env!("CARGO_BIN_EXE_zz"))
            .env("ZZ_CONF", &fixture.config)
            .env("ZZ_TEST_SOCKET", &fixture.socket);
        let (rendered, captured, early_status) = capture_command_until(command, &[b"\x1b[?1049h"]);
        assert!(
            rendered,
            "child exited early={early_status:?}; pty output={}",
            String::from_utf8_lossy(&captured)
        );
        assert!(captured.windows(11).any(|window| window == b"no sessions"));

        let listed = fixture.run(&["list-sessions", "-F", "#{session_name}"]);
        assert_eq!(listed.status.code(), Some(0));
        assert_eq!(listed.stdout, b"fallback\n");
    }

    #[test]
    fn native_attach_rejects_unsupported_x_like_the_engine() {
        let fixture = Fixture::new();
        let invocation = zz_protocol::CommandInvocation::new("attach-session", ["-x"]);
        let error = zz_mux::MuxEngine::default()
            .execute(&mut zz_mux::ExecutionContext::default(), &invocation)
            .expect_err("engine rejects the unsupported attach option");
        let expected = format!("{}\n", error.tmux_message());
        for command in ["attach", "attach-session"] {
            let output = fixture
                .command()
                .args([command, "-x"])
                .output()
                .expect("run native attach rejection");
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, expected.as_bytes());
        }
    }

    #[test]
    fn native_attach_e_preserves_the_session_environment() {
        const NAME: &str = "ZZ_NATIVE_ATTACH_E";

        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        assert!(
            fixture
                .run(&[
                    "new-session",
                    "-d",
                    "-s",
                    "native-attach-e",
                    "printf 'ZZ_NATIVE_ATTACH_E_READY\\r\\n'; exec /bin/cat",
                ])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-option",
                    "-t",
                    "=native-attach-e:",
                    "update-environment",
                    NAME,
                ])
                .status
                .success()
        );
        assert!(
            fixture
                .run(&[
                    "set-environment",
                    "-t",
                    "=native-attach-e",
                    NAME,
                    "before-E",
                ])
                .status
                .success()
        );

        let mut preserved = fixture.command();
        preserved
            .env(NAME, "ignored")
            .args(["attach-session", "-E", "-t", "=native-attach-e"]);
        let (rendered, captured, early_status) =
            capture_command_until(preserved, &[b"ZZ_NATIVE_ATTACH_E_READY"]);
        if rendered && captured.is_empty() {
            return;
        }
        assert!(
            rendered,
            "native attach -E exited early={early_status:?}; output={}",
            String::from_utf8_lossy(&captured)
        );
        let environment = fixture.run(&["show-environment", "-t", "=native-attach-e", NAME]);
        assert_eq!(environment.status.code(), Some(0));
        assert_eq!(environment.stdout, format!("{NAME}=before-E\n").as_bytes());
        assert!(environment.stderr.is_empty());

        let mut refreshed = fixture.command();
        refreshed
            .env(NAME, "refreshed")
            .args(["attach-session", "-t", "=native-attach-e"]);
        let (rendered, captured, early_status) =
            capture_command_until(refreshed, &[b"ZZ_NATIVE_ATTACH_E_READY"]);
        assert!(
            rendered,
            "ordinary native attach exited early={early_status:?}; output={}",
            String::from_utf8_lossy(&captured)
        );
        let environment = fixture.run(&["show-environment", "-t", "=native-attach-e", NAME]);
        assert_eq!(environment.status.code(), Some(0));
        assert_eq!(environment.stdout, format!("{NAME}=refreshed\n").as_bytes());
        assert!(environment.stderr.is_empty());
    }

    #[test]
    fn mouse_option_gates_the_outer_terminal_mouse_modes() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "mousey"]);
        assert_eq!(created.status.code(), Some(0));

        let (rendered, captured, early_status) = capture_tui_until(
            &fixture,
            &["attach-session", "-t", "mousey"],
            &[b"[mousey]"],
        );
        assert!(
            rendered,
            "child exited early={early_status:?}; pty output={}",
            String::from_utf8_lossy(&captured)
        );
        assert!(
            captured
                .windows(b"\x1b[?1003h".len())
                .any(|window| window == b"\x1b[?1003h"),
            "the pinned default keeps mouse on: {}",
            String::from_utf8_lossy(&captured)
        );

        let disabled = fixture.run(&["set", "-g", "mouse", "off"]);
        assert_eq!(disabled.status.code(), Some(0));
        let (rendered, captured, early_status) = capture_tui_until(
            &fixture,
            &["attach-session", "-t", "mousey"],
            &[b"[mousey]"],
        );
        assert!(
            rendered,
            "child exited early={early_status:?}; pty output={}",
            String::from_utf8_lossy(&captured)
        );
        assert!(
            !captured
                .windows(b"[?1003".len())
                .any(|window| window == b"[?1003"),
            "mouse off with no app-requested tracking must emit no outer mouse mode: {}",
            String::from_utf8_lossy(&captured)
        );
    }

    #[test]
    fn nested_attach_inside_a_pane_prints_the_pinned_refusal() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&["new-session", "-d", "-s", "outer"]);
        assert_eq!(created.status.code(), Some(0));

        let nested_attach = format!(
            "{} -f {} -S {} attach",
            env!("CARGO_BIN_EXE_zz"),
            fixture.config.display(),
            fixture.socket.display(),
        );
        let sent = fixture.run(&["send-keys", "-t", "outer", &nested_attach, "Enter"]);
        assert_eq!(
            sent.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&sent.stderr)
        );

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let captured = fixture.run(&["capture-pane", "-p", "-t", "outer"]);
            assert_eq!(captured.status.code(), Some(0));
            let pane_text = String::from_utf8_lossy(&captured.stdout).into_owned();
            if pane_text.contains("sessions should be nested with care, unset $TMUX to force") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "nested refusal did not appear; pane content: {pane_text}"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn read_only_attach_renders_but_typed_keys_never_reach_the_pane() {
        let fixture = Fixture::new();
        if !local_socket_bind_available(&fixture.socket) {
            return;
        }
        let created = fixture.run(&[
            "new-session",
            "-d",
            "-s",
            "ro",
            "printf 'ZZ_ATTACH_READY\\r\\n'; exec /bin/cat",
        ]);
        assert_eq!(created.status.code(), Some(0));

        let attach_and_type = |attach: &[&str],
                               text: &[u8],
                               expected: Option<&str>|
         -> (bool, Vec<u8>, String) {
            let Ok((mut master, slave)) = open_pty() else {
                return (true, Vec::new(), String::new());
            };
            rustix::io::ioctl_fionbio(&master, true).expect("set pty master nonblocking");
            let stdin = slave.try_clone().expect("clone pty stdin");
            let stdout = slave.try_clone().expect("clone pty stdout");
            let mut child = fixture
                .command()
                .args(attach)
                .stdin(Stdio::from(stdin))
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(slave))
                .spawn()
                .expect("spawn TUI attach");
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut captured = Vec::new();
            let rendered = loop {
                let mut buffer = [0_u8; 4096];
                match master.read(&mut buffer) {
                    Ok(0) => {}
                    Ok(count) => captured.extend_from_slice(&buffer[..count]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(_) => {}
                }
                if captured
                    .windows(b"ZZ_ATTACH_READY".len())
                    .any(|window| window == b"ZZ_ATTACH_READY")
                {
                    break true;
                }
                if Instant::now() >= deadline || child.try_wait().expect("poll attach").is_some() {
                    break false;
                }
                thread::sleep(Duration::from_millis(5));
            };
            let mut flags = String::new();
            if rendered {
                master.write_all(text).expect("type into the attach pty");
                let settle = Instant::now() + Duration::from_millis(600);
                while Instant::now() < settle {
                    let mut buffer = [0_u8; 4096];
                    match master.read(&mut buffer) {
                        Ok(count) if count > 0 => captured.extend_from_slice(&buffer[..count]),
                        _ => thread::sleep(Duration::from_millis(10)),
                    }
                }
                if let Some(marker) = expected {
                    let deadline = Instant::now() + Duration::from_secs(10);
                    loop {
                        let pane = fixture.run(&["capture-pane", "-p", "-t", "ro"]);
                        if String::from_utf8_lossy(&pane.stdout).contains(marker) {
                            break;
                        }
                        assert!(
                            Instant::now() < deadline,
                            "typed {marker} never reached the pane"
                        );
                        let mut buffer = [0_u8; 4096];
                        if let Ok(count) = master.read(&mut buffer)
                            && count > 0
                        {
                            captured.extend_from_slice(&buffer[..count]);
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                }
                let listed = fixture.run(&["list-clients", "-F", "#{client_flags}"]);
                flags = String::from_utf8_lossy(&listed.stdout).into_owned();
            }
            let _ = child.kill();
            let _ = child.wait();
            drop(master);
            (rendered, captured, flags)
        };

        let (rendered, captured, flags) =
            attach_and_type(&["attach-session", "-r", "-t", "ro"], b"echo ZZRO\r", None);
        if captured.is_empty() && flags.is_empty() && rendered {
            return;
        }
        assert!(
            rendered,
            "read-only attach did not render: {}",
            String::from_utf8_lossy(&captured)
        );
        assert!(
            flags.lines().any(|line| {
                line.split(',').any(|flag| flag == "attached")
                    && line.split(',').any(|flag| flag == "read-only")
                    && !line.split(',').any(|flag| flag == "ignore-size")
            }),
            "client_flags must report read-only without ignore-size: {flags}"
        );

        let (rendered, writable_attach, flags) = attach_and_type(
            &["attach-session", "-t", "ro"],
            b"echo ZZRW\r",
            Some("ZZRW"),
        );
        assert!(
            rendered,
            "writable attach did not render: {}",
            String::from_utf8_lossy(&writable_attach)
        );
        assert!(
            flags.lines().any(|line| {
                line.split(',').any(|flag| flag == "attached")
                    && !line.split(',').any(|flag| flag == "read-only")
                    && !line.split(',').any(|flag| flag == "ignore-size")
            }),
            "plain attach reports no read-only flag: {flags}"
        );

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let captured = fixture.run(&["capture-pane", "-p", "-t", "ro"]);
            assert_eq!(captured.status.code(), Some(0));
            let pane_text = String::from_utf8_lossy(&captured.stdout).into_owned();
            assert!(
                !pane_text.contains("ZZRO"),
                "read-only keystrokes reached the pane: {pane_text}"
            );
            if pane_text.contains("ZZRW") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "writable keystrokes never reached the pane: {pane_text}; client flags: {flags}; attach output: {}",
                String::from_utf8_lossy(&writable_attach)
            );
            thread::sleep(Duration::from_millis(50));
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

        fn parse_stream_impl(output: &[u8], double: bool, contiguous: bool) -> Stream {
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
                if contiguous {
                    assert_eq!(block.number, index as u64 + 1);
                }
                assert!(block.time > 0);
            }
            Stream { blocks, outside }
        }

        fn parse_stream(output: &[u8], double: bool) -> Stream {
            parse_stream_impl(output, double, true)
        }

        fn parse_stream_allow_gaps(output: &[u8], double: bool) -> Stream {
            parse_stream_impl(output, double, false)
        }

        fn os_error_text(error: &std::io::Error) -> String {
            let message = error.to_string();
            error.raw_os_error().map_or(message.clone(), |code| {
                message
                    .strip_suffix(&format!(" (os error {code})"))
                    .unwrap_or(&message)
                    .to_owned()
            })
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

        fn next_block_guard<'a>(lines: impl Iterator<Item = &'a str>) -> &'a str {
            for line in lines {
                if line.starts_with("%begin ")
                    || line.starts_with("%end ")
                    || line.starts_with("%error ")
                {
                    return line;
                }
                assert!(
                    line.starts_with('%'),
                    "raw line {line:?} between the error and its guard"
                );
            }
            panic!("no block guard next to the raw error");
        }

        fn assert_attached_startup(outside: &[String], name: &str) {
            let settled = outside
                .iter()
                .filter(|line| {
                    !line.starts_with("%window-renamed @0 ") && !line.starts_with("%output %0 ")
                })
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

        fn wait_for_control_marker(path: &Path, child: &mut Child, label: &str) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !path.exists() {
                if let Some(status) = child.try_wait().expect("poll control marker process") {
                    panic!("control process exited before {label}: {status}");
                }
                if Instant::now() >= deadline {
                    child.kill().expect("kill stalled control marker process");
                    panic!("control process did not reach {label}");
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn wait_for_control_output_marker(
            path: &Path,
            marker: &str,
            child: &mut Child,
            label: &str,
        ) {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let output = std::fs::read(path).expect("read live control output");
                let mut marker_seen = false;
                let completed = String::from_utf8_lossy(&output).lines().any(|line| {
                    if line == marker {
                        marker_seen = true;
                        false
                    } else {
                        marker_seen && line.starts_with("%end ")
                    }
                });
                if completed {
                    return;
                }
                if let Some(status) = child.try_wait().expect("poll control block process") {
                    panic!("control process exited before {label}: {status}");
                }
                if Instant::now() >= deadline {
                    child.kill().expect("kill stalled control block process");
                    panic!("control process did not reach {label}");
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn wait_for_control_error_marker(
            path: &Path,
            marker: &str,
            child: &mut Child,
            label: &str,
        ) {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let output = std::fs::read(path).expect("read live control output");
                let mut marker_seen = false;
                let completed = String::from_utf8_lossy(&output).lines().any(|line| {
                    if line == marker {
                        marker_seen = true;
                        false
                    } else {
                        marker_seen && line.starts_with("%error ")
                    }
                });
                if completed {
                    return;
                }
                if let Some(status) = child.try_wait().expect("poll control error process") {
                    panic!("control process exited before {label}: {status}");
                }
                if Instant::now() >= deadline {
                    child.kill().expect("kill stalled control error process");
                    panic!("control process did not reach {label}");
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn spawn_control_to_file(
            fixture: &Fixture,
            arguments: &[&str],
            output_path: &Path,
        ) -> (Child, ChildStdin) {
            let output_file = File::create(output_path).expect("create live control output");
            let mut child = fixture
                .command()
                .args(arguments)
                .stdin(Stdio::piped())
                .stdout(Stdio::from(output_file))
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn control with live output");
            let stdin = child.stdin.take().expect("piped control stdin");
            (child, stdin)
        }

        fn wait_for_control_clients(
            fixture: &Fixture,
            expected: usize,
            label: &str,
        ) -> Vec<String> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let listed =
                    fixture.run(&["list-clients", "-F", "#{client_name}\t#{client_flags}"]);
                let names = String::from_utf8_lossy(&listed.stdout)
                    .lines()
                    .filter_map(|line| {
                        let (name, flags) = line.split_once('\t')?;
                        flags.contains("control-mode").then(|| name.to_owned())
                    })
                    .collect::<Vec<_>>();
                if listed.status.success() && names.len() == expected {
                    return names;
                }
                assert!(
                    Instant::now() < deadline,
                    "control clients did not settle for {label}: status={} stdout={} stderr={}",
                    listed.status,
                    String::from_utf8_lossy(&listed.stdout),
                    String::from_utf8_lossy(&listed.stderr)
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn prime_control_return_code(
            child: &mut Child,
            stdin: &mut ChildStdin,
            output_path: &Path,
            marker: &str,
            missing: &str,
        ) {
            writeln!(stdin, "kill-session -t {missing}").expect("write retval failure");
            writeln!(stdin, "display-message -p {marker}").expect("write retval marker");
            stdin.flush().expect("flush retval commands");
            wait_for_control_output_marker(output_path, marker, child, marker);
        }

        fn collect_control_process(
            mut child: Child,
            stdin: Option<ChildStdin>,
            label: &str,
        ) -> Output {
            let deadline = Instant::now() + Duration::from_secs(10);
            let status = loop {
                if let Some(status) = child.try_wait().expect("poll control process") {
                    break status;
                }
                if Instant::now() >= deadline {
                    child.kill().expect("kill stalled control process");
                    panic!("control process did not exit for {label}");
                }
                thread::sleep(Duration::from_millis(10));
            };
            drop(stdin);
            let output = child.wait_with_output().expect("collect control output");
            assert_eq!(output.status, status, "{label}");
            output
        }

        fn run_control_until_return(
            fixture: &Fixture,
            arguments: &[&str],
            input: &str,
            marker: &Path,
            label: &str,
        ) -> Output {
            let (mut child, mut stdin) = fixture.spawn_with_open_stdin(arguments);
            stdin
                .write_all(input.as_bytes())
                .expect("write control commands");
            if !input.ends_with('\n') {
                writeln!(stdin).expect("terminate control command line");
            }
            writeln!(stdin, "run-shell 'touch \"{}\"'", marker.display())
                .expect("write control completion marker");
            stdin.flush().expect("flush control commands");
            wait_for_control_marker(marker, &mut child, label);
            writeln!(stdin).expect("write control return");
            stdin.flush().expect("flush control return");
            collect_control_process(child, Some(stdin), label)
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
        fn control_initial_command_uses_a_live_daemon_alias() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", "aliased"])
                    .status
                    .success()
            );
            assert!(
                fixture
                    .run(&[
                        "set-option",
                        "-s",
                        "command-alias[40]",
                        "live=list-sessions -F 'alias-#{session_name}'",
                    ])
                    .status
                    .success()
            );
            let output = fixture.run(&["-C", "live"]);
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 1);
            assert_block(&stream.blocks[0], 1, 0, &["alias-aliased"], false);
            assert_eq!(stream.outside, ["%exit"]);
        }

        #[test]
        fn control_stdin_prepares_each_complete_alias_chain_once() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", "alias-chain"])
                    .status
                    .success()
            );
            assert!(
                fixture
                    .run(&[
                        "set-option",
                        "-s",
                        "command-alias[40]",
                        "live=display-message -p old",
                    ])
                    .status
                    .success()
            );
            let marker = fixture._directory.path().join("alias-chain.complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "attach-session", "-t", "alias-chain"],
                "set-option -s command-alias[40] 'live=display-message -p new' ; live\nlive\n",
                &marker,
                "alias chain completion",
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 5);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(&stream.blocks[2], 3, 1, &["old"], false);
            assert_block(&stream.blocks[3], 4, 1, &["new"], false);
            assert_block(&stream.blocks[4], 5, 1, &[], false);
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
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
            let complete = fixture._directory.path().join("chain.complete");
            let (mut child, mut stdin) =
                fixture.spawn_with_open_stdin(&["-C", "new-session", "-s", "chain"]);
            writeln!(stdin, "display-message -p one ; display-message -p two")
                .expect("write successful chain");
            writeln!(stdin, "kill-session -t nosuch ; display-message -p skipped")
                .expect("write failing chain");
            writeln!(stdin, "display-message -p fresh").expect("write fresh command");
            writeln!(stdin, "run-shell 'touch \"{}\"'", complete.display())
                .expect("write chain completion marker");
            stdin.flush().expect("flush chain commands");
            wait_for_control_marker(&complete, &mut child, "chain completion");
            writeln!(stdin).expect("write chain return");
            stdin.flush().expect("flush chain return");
            let output = collect_control_process(child, Some(stdin), "chain return");
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 6);
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
            assert_block(&stream.blocks[5], 6, 1, &[], false);
            assert_attached_startup(&stream.outside, "chain");
        }

        #[test]
        fn control_parse_and_generic_nonzero_results_do_not_set_retval() {
            let cases: &[(&str, &[u8], i32, &[&str], bool)] = &[
                (
                    "parse-return-status",
                    b"wibble\n\n",
                    0,
                    &["parse error: unknown command: wibble"],
                    true,
                ),
                (
                    "flag-parse-return-status",
                    b"list-sessions -Z\n\n",
                    0,
                    &["command list-sessions: unknown flag -Z"],
                    true,
                ),
                (
                    "arity-parse-eof-status",
                    b"rename-session\n",
                    0,
                    &["command rename-session: too few arguments (need at least 1)"],
                    true,
                ),
            ];
            for (session, input, expected_status, expected_payload, expected_error) in cases {
                let fixture = Fixture::new();
                if !local_socket_bind_available(&fixture.socket) {
                    return;
                }
                let output = fixture.run_with_stdin(
                    &["-C", "new-session", "-s", session, "exec /bin/cat"],
                    input,
                );
                assert_eq!(output.status.code(), Some(*expected_status), "{session}");
                assert!(output.stderr.is_empty(), "{session}");
                let stream = parse_stream(&output.stdout, false);
                assert_eq!(stream.blocks.len(), 2, "{session}");
                assert_block(&stream.blocks[0], 1, 0, &[], false);
                assert_block(&stream.blocks[1], 2, 1, expected_payload, *expected_error);
                assert_attached_startup(&stream.outside, session);
            }
        }

        #[cfg(unix)]
        #[test]
        fn control_foreground_run_shell_closes_before_raw_output_and_continues() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let marker_path = fixture._directory.path().join("run-shell-order-complete");
            let output = run_control_until_return(
                &fixture,
                &[
                    "-C",
                    "new-session",
                    "-s",
                    "run-shell-order",
                    "exec /bin/cat",
                ],
                "run-shell 'printf foreground; exit 3' ; display-message -p AFTER_FOREGROUND\n\
                 run-shell 'kill -TERM $$'\n",
                &marker_path,
                "foreground run-shell completion",
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let text = String::from_utf8(output.stdout.clone()).expect("UTF-8 control output");
            let lines = text.lines().collect::<Vec<_>>();
            let index = |expected: &str| {
                lines
                    .iter()
                    .position(|line| *line == expected)
                    .unwrap_or_else(|| panic!("missing {expected:?}: {text}"))
            };
            let frame = |marker_name: &str, number: u64| {
                lines
                    .iter()
                    .position(|line| {
                        marker(line, marker_name)
                            .is_some_and(|(_, candidate, _)| candidate == number)
                    })
                    .unwrap_or_else(|| panic!("missing {marker_name} for {number}: {text}"))
            };
            assert!(frame("%end", 2) < index("foreground"));
            assert!(index("foreground") < index("'printf foreground; exit 3' returned 3"));
            assert!(index("'printf foreground; exit 3' returned 3") < frame("%begin", 3));
            assert!(frame("%end", 4) < index("'kill -TERM $$' terminated by signal 15"));

            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 5, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(&stream.blocks[2], 3, 1, &["AFTER_FOREGROUND"], false);
            assert_block(&stream.blocks[3], 4, 1, &[], false);
            assert_block(&stream.blocks[4], 5, 1, &[], false);
            assert!(stream.outside.iter().any(|line| line == "foreground"));
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == "'printf foreground; exit 3' returned 3")
            );
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == "'kill -TERM $$' terminated by signal 15")
            );
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
        }

        #[test]
        fn control_alias_groups_inherit_flags_and_continue_after_shell_status() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let created =
                fixture.run(&["new-session", "-d", "-s", "control-alias", "exec /bin/cat"]);
            assert_eq!(created.status.code(), Some(0));
            let configured = fixture.run(&[
                "set-option",
                "-s",
                "command-alias[90]",
                "auditmulti=display-message -p first ; display-message -p",
            ]);
            assert_eq!(configured.status.code(), Some(0));

            let initial = fixture.run_with_stdin(&["-C", "auditmulti", "tail"], b"");
            assert_eq!(initial.status.code(), Some(0));
            assert!(initial.stderr.is_empty());
            let stream = parse_stream(&initial.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &["first"], false);
            assert_block(&stream.blocks[1], 2, 0, &["tail"], false);
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));

            let configured = fixture.run(&[
                "set-option",
                "-s",
                "command-alias[91]",
                "auditfail=display-message -p before ; run-shell 'exit 3' ; display-message -p after",
            ]);
            assert_eq!(configured.status.code(), Some(0));
            let marker_path = fixture._directory.path().join("alias-shell-complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "attach-session", "-t", "control-alias"],
                "auditfail ; display-message -p same-line\n",
                &marker_path,
                "control alias shell completion",
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 6, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &["before"], false);
            assert_block(&stream.blocks[2], 3, 1, &[], false);
            assert_block(&stream.blocks[3], 4, 1, &["after"], false);
            assert_block(&stream.blocks[4], 5, 1, &["same-line"], false);
            assert_block(&stream.blocks[5], 6, 1, &[], false);
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == "'exit 3' returned 3")
            );

            let source = write_source(
                fixture._directory.path(),
                "alias-source.conf",
                "display-message -p CHILD_ONE\ndisplay-message -p CHILD_TWO\n",
            );
            let configured = fixture.run(&[
                "set-option",
                "-s",
                "command-alias[92]",
                &format!("auditsource=source-file '{source}'"),
            ]);
            assert_eq!(configured.status.code(), Some(0));
            let initial = fixture.run_with_stdin(&["-C", "auditsource"], b"");
            assert_eq!(initial.status.code(), Some(0));
            let stream = parse_stream(&initial.stdout, false);
            assert_eq!(stream.blocks.len(), 3, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 0, &["CHILD_ONE"], false);
            assert_block(&stream.blocks[2], 3, 0, &["CHILD_TWO"], false);

            let configured = fixture.run(&[
                "set-option",
                "-s",
                "command-alias[94]",
                &format!(
                    "auditmiddle=display-message -p BEFORE ; source-file '{source}' ; display-message -p OUTER"
                ),
            ]);
            assert_eq!(configured.status.code(), Some(0));
            let initial = fixture.run_with_stdin(&["-C", "auditmiddle"], b"");
            assert_eq!(initial.status.code(), Some(0));
            let stream = parse_stream_allow_gaps(&initial.stdout, false);
            assert_eq!(stream.blocks.len(), 5, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &["BEFORE"], false);
            assert_block(&stream.blocks[1], 2, 0, &[], false);
            assert_block(&stream.blocks[2], 3, 0, &["CHILD_ONE"], false);
            assert_block(&stream.blocks[3], 4, 0, &["CHILD_TWO"], false);
            assert_block(&stream.blocks[4], 6, 0, &["OUTER"], false);

            let missing = fixture._directory.path().join("missing-alias-source.conf");
            let configured = fixture.run(&[
                "set-option",
                "-s",
                "command-alias[93]",
                &format!(
                    "auditmissing=display-message -p before-missing ; source-file '{}' ; display-message -p after-missing",
                    missing.display()
                ),
            ]);
            assert_eq!(configured.status.code(), Some(0));
            let marker_path = fixture
                ._directory
                .path()
                .join("alias-source-failure-complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "attach-session", "-t", "control-alias"],
                "auditmissing ; display-message -p same-line-missing\n",
                &marker_path,
                "control alias source failure",
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert!(
                stream
                    .blocks
                    .iter()
                    .any(|block| block.payload == ["before-missing"])
            );
            assert!(stream.blocks.iter().any(|block| {
                block.error
                    && block
                        .payload
                        .iter()
                        .any(|line| line.contains("missing-alias-source.conf"))
            }));
            assert!(!stream.blocks.iter().any(|block| {
                block
                    .payload
                    .iter()
                    .any(|line| matches!(line.as_str(), "after-missing" | "same-line-missing"))
            }));

            let configured = fixture.run(&[
                "set-option",
                "-s",
                "command-alias[95]",
                &format!(
                    "auditpartial=display-message -p before-partial ; source-file '{}' '{source}' ; display-message -p after-partial",
                    missing.display()
                ),
            ]);
            assert_eq!(configured.status.code(), Some(0));
            let marker_path = fixture
                ._directory
                .path()
                .join("alias-partial-source-complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "attach-session", "-t", "control-alias"],
                "auditpartial\n",
                &marker_path,
                "control alias partial source failure",
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            let payloads = stream
                .blocks
                .iter()
                .flat_map(|block| block.payload.iter().map(String::as_str))
                .collect::<Vec<_>>();
            for expected in ["before-partial", "CHILD_ONE", "CHILD_TWO", "after-partial"] {
                assert!(payloads.contains(&expected), "{stream:?}");
            }
            assert!(stream.blocks.iter().any(|block| {
                !block.error
                    && block
                        .payload
                        .iter()
                        .any(|line| line.contains("missing-alias-source.conf"))
            }));

            let state_source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../compat/scenarios/source-file-output-child.conf");
            let configured = fixture.run(&[
                "set-option",
                "-s",
                "command-alias[96]",
                &format!(
                    "auditshutdown=kill-server ; source-file '{}' ; display-message -p OUTER",
                    state_source.display()
                ),
            ]);
            assert_eq!(configured.status.code(), Some(0));
            let output = fixture.run_with_stdin(&["-C", "auditshutdown"], b"");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let lines = std::str::from_utf8(&output.stdout)
                .expect("UTF-8 shutdown alias output")
                .lines()
                .collect::<Vec<_>>();
            let exits = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| **line == "%exit")
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            assert_eq!(exits.len(), 1, "{lines:?}");
            let child = lines
                .iter()
                .position(|line| *line == "CHILD_ONE")
                .expect("sourced child after shutdown");
            assert!(exits[0] < child, "{lines:?}");
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            let child = stream
                .blocks
                .iter()
                .position(|block| block.payload == ["CHILD_ONE"])
                .expect("sourced child guard after shutdown");
            assert!(stream.blocks[child + 1].payload.is_empty(), "{stream:?}");
            assert!(
                !stream
                    .blocks
                    .iter()
                    .any(|block| block.payload == ["CHILD_TWO"])
            );
            assert!(
                stream
                    .blocks
                    .iter()
                    .skip(child + 2)
                    .any(|block| block.payload == ["OUTER"])
            );
            fixture.assert_stopped();
        }

        #[test]
        fn shutdown_hook_source_yields_locally_without_replaying_the_after_hook() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let created = fixture.run(&["new-session", "-d", "-s", "hookprobe", "exec /bin/cat"]);
            assert_eq!(created.status.code(), Some(0));
            let scenarios = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/scenarios");
            let root = scenarios.join("source-file-output-hook-root.conf");
            let child = scenarios.join("source-file-output-child.conf");
            let hook = format!("source-file '{}'", child.display());
            let configured =
                fixture.run(&["set-hook", "-g", "after-display-message", hook.as_str()]);
            assert_eq!(configured.status.code(), Some(0));
            let callback = format!(
                "kill-server ; source-file '{}' ; display-message -p OUTER",
                root.display()
            );
            let output = fixture.run_with_stdin(&["-C", "run-shell", "-C", callback.as_str()], b"");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 3, "{stream:?}");
            let payloads = stream
                .blocks
                .iter()
                .flat_map(|block| block.payload.iter().map(String::as_str))
                .collect::<Vec<_>>();
            assert_eq!(payloads, ["OUTER", "HOOK_TRIGGER"]);
            assert_eq!(
                stream
                    .blocks
                    .iter()
                    .filter(|block| block.payload.is_empty())
                    .count(),
                1
            );
            for suppressed in ["HOOK_LATER", "CHILD_ONE", "CHILD_TWO"] {
                assert!(!payloads.contains(&suppressed), "{stream:?}");
            }
            let lines = std::str::from_utf8(&output.stdout)
                .expect("UTF-8 shutdown hook output")
                .lines()
                .collect::<Vec<_>>();
            assert_eq!(lines.iter().filter(|line| **line == "%exit").count(), 1);
            assert_eq!(lines.last().copied(), Some("%exit"));
            fixture.assert_stopped();
        }

        #[test]
        fn foreground_callback_error_prevents_later_shutdown_without_losing_guards() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let created = fixture.run(&["new-session", "-d", "-s", "callback-error"]);
            assert_eq!(created.status.code(), Some(0));
            let output = fixture.run_with_stdin(
                &[
                    "-C",
                    "run-shell",
                    "-C",
                    "display-message -F MATCH one ; kill-server",
                ],
                b"",
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(
                &stream.blocks[1],
                2,
                0,
                &["only one of -F or argument must be given"],
                true,
            );
            assert_eq!(stream.outside, ["%exit"]);

            let listed = fixture.run(&["list-sessions"]);
            assert_eq!(listed.status.code(), Some(0));
            let stopped = fixture.run(&["kill-server"]);
            assert_eq!(stopped.status.code(), Some(0));
            fixture.assert_stopped();
        }

        #[test]
        fn draining_alias_shutdown_tears_down_panes_and_rejects_late_commands_cleanly() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let ready = fixture._directory.path().join("shutdown-pane-ready");
            let trigger = fixture._directory.path().join("shutdown-pane-trigger");
            let marker = fixture._directory.path().join("shutdown-pane-marker");
            let pane_command = format!(
                "printf ready > '{}'; while [ ! -e '{}' ]; do sleep 0.01; done; sleep 0.2; printf pane > '{}'; sleep 5",
                ready.display(),
                trigger.display(),
                marker.display()
            );
            let created = fixture.run(&[
                "new-session",
                "-d",
                "-s",
                "draining-shutdown",
                &pane_command,
            ]);
            assert_eq!(created.status.code(), Some(0));
            let deadline = Instant::now() + Duration::from_secs(2);
            while !ready.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(ready.exists(), "pane process did not start");
            let configured = fixture.run(&[
                "set-option",
                "-s",
                "command-alias[97]",
                "slowshutdown=kill-server ; run-shell 'sleep 0.8'",
            ]);
            assert_eq!(configured.status.code(), Some(0));

            let shutdown = fixture.run(&["slowshutdown"]);
            assert_eq!(shutdown.status.code(), Some(0));
            assert!(shutdown.stdout.is_empty());
            assert!(shutdown.stderr.is_empty());
            let late = fixture.run(&["list-sessions"]);
            assert_eq!(late.status.code(), Some(1));
            assert!(late.stdout.is_empty());
            assert_eq!(late.stderr, b"server exited unexpectedly\n");
            std::fs::write(&trigger, b"go").expect("release pane process");
            thread::sleep(Duration::from_millis(400));
            assert!(!marker.exists());
            fixture.assert_stopped();
            assert!(!marker.exists());
        }

        #[cfg(unix)]
        #[test]
        fn control_sourced_run_shell_closes_before_raw_output_and_same_line_continues() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("sourced run-shell");
            std::fs::create_dir(&directory).expect("create sourced run-shell directory");
            let source = write_source(
                &directory,
                "run-shell.conf",
                "run-shell 'printf CHILD; exit 3' ; display-message -p SAME\n",
            );
            let marker_path = directory.join("complete");
            let output = run_control_until_return(
                &fixture,
                &[
                    "-C",
                    "new-session",
                    "-s",
                    "sourced-run-shell",
                    "exec /bin/cat",
                ],
                &format!("source-file '{source}'\n"),
                &marker_path,
                "sourced run-shell completion",
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let text = String::from_utf8(output.stdout.clone()).expect("UTF-8 control output");
            let lines = text.lines().collect::<Vec<_>>();
            let child = lines
                .iter()
                .position(|line| *line == "CHILD")
                .unwrap_or_else(|| panic!("missing raw child output: {text}"));
            assert_eq!(lines[child + 1], "'printf CHILD; exit 3' returned 3");
            let end = marker(lines[child - 1], "%end").expect("guard before raw child output");
            assert_eq!(end.2, 1);
            let begin = marker(lines[child + 2], "%begin").expect("guard after raw diagnostic");
            assert_eq!(begin.2, 1);

            let stream = parse_stream_allow_gaps(&output.stdout, false);
            let shell = stream
                .blocks
                .iter()
                .find(|block| block.number == end.1)
                .expect("sourced run-shell guard");
            assert!(shell.payload.is_empty());
            assert!(!shell.error);
            let same = stream
                .blocks
                .iter()
                .find(|block| block.number == begin.1)
                .expect("same-line continuation guard");
            assert_eq!(same.payload, ["SAME"]);
            assert!(!same.error);
            assert!(stream.outside.iter().any(|line| line == "CHILD"));
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == "'printf CHILD; exit 3' returned 3")
            );
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
            let output =
                fixture.run_with_stdin(&["-C", "new-session", "-s", "parse-error"], b"wibble\n\n");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(
                &stream.blocks[1],
                2,
                1,
                &["parse error: unknown command: wibble"],
                true,
            );
            assert_attached_startup(&stream.outside, "parse-error");
        }

        #[test]
        fn control_stdin_preflight_errors_abort_the_whole_line() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let marker = fixture._directory.path().join("preflight.complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "new-session", "-s", "preflight-error"],
                "set-option -g @should-not-run yes ; bogus-command\nshow-options -gqv @should-not-run\n",
                &marker,
                "preflight completion",
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 4);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(
                &stream.blocks[1],
                2,
                1,
                &["parse error: unknown command: bogus-command"],
                true,
            );
            assert_block(&stream.blocks[2], 3, 1, &[], false);
            assert_block(&stream.blocks[3], 4, 1, &[], false);
            assert_attached_startup(&stream.outside, "preflight-error");
        }

        #[test]
        fn control_config_error_keeps_the_source_line_and_inner_message() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("a: b");
            std::fs::create_dir(&directory).expect("create config error directory");
            let source = directory.join("mux.conf");
            std::fs::write(&source, "wibble\ndisplay-message -p after-config-error\n")
                .expect("write invalid config");
            let input = format!("source-file '{}'\n\n", source.display());
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
            let diagnostic = format!(
                "%config-error {}:1: unknown command: wibble",
                source.display()
            );
            assert!(stream.outside.contains(&diagnostic));
            let outside = stream
                .outside
                .into_iter()
                .filter(|line| line != &diagnostic)
                .collect::<Vec<_>>();
            assert_attached_startup(&outside, "config-error");
        }

        #[test]
        fn control_first_sourced_construction_failure_uses_config_error() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let source = fixture._directory.path().join("classification.conf");
            std::fs::write(
                &source,
                "set-option -s command-alias[90] badalias=\n\
                 set-option -s command-alias[91] source=\n\
                 source ignored.conf\n\
                 wibble\n\
                 kill-s\n\
                 badalias\n\
                 set-environment -g\n\
                 new-pane\n\
                 display-message -p after-classification\n",
            )
            .expect("write sourced classification fixture");
            let input = format!("source-file '{}'\n\n", source.display());
            let output = fixture.run_with_stdin(
                &["-C", "new-session", "-s", "source-classification"],
                input.as_bytes(),
            );
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stream = parse_stream(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 2);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            let diagnostics = [format!(
                "%config-error {}:4: unknown command: wibble",
                source.display()
            )];
            for diagnostic in &diagnostics {
                assert!(stream.outside.contains(diagnostic), "{diagnostic}");
            }
            let outside = stream
                .outside
                .into_iter()
                .filter(|line| !diagnostics.contains(line))
                .collect::<Vec<_>>();
            assert_attached_startup(&outside, "source-classification");
        }

        #[test]
        fn control_sourced_success_quiet_and_partial_guards_keep_fifo_status() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("control sourced guards");
            std::fs::create_dir(&directory).expect("create sourced guard directory");
            let quiet = write_source(
                &directory,
                "quiet.conf",
                "set-option -g @guard-success yes\nsource-file -q quiet-missing.conf\n",
            );
            let quiet_input = format!("source-file '{quiet}'\ndisplay-message -p quiet-fresh\n");
            let quiet_marker = directory.join("quiet.complete");
            let quiet_output = run_control_until_return(
                &fixture,
                &["-C", "new-session", "-s", "quiet-guards"],
                &quiet_input,
                &quiet_marker,
                "quiet guard completion",
            );
            assert_eq!(quiet_output.status.code(), Some(0));
            assert!(quiet_output.stderr.is_empty());
            let quiet_stream = parse_stream_allow_gaps(&quiet_output.stdout, false);
            assert_eq!(quiet_stream.blocks.len(), 6);
            assert_block(&quiet_stream.blocks[0], 1, 0, &[], false);
            assert_block(&quiet_stream.blocks[1], 2, 1, &[], false);
            assert_block(&quiet_stream.blocks[2], 3, 1, &[], false);
            assert_block(&quiet_stream.blocks[3], 4, 1, &[], false);
            assert_block(&quiet_stream.blocks[4], 7, 1, &["quiet-fresh"], false);
            assert_block(&quiet_stream.blocks[5], 8, 1, &[], false);
            assert_attached_startup(&quiet_stream.outside, "quiet-guards");

            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("control partial guards");
            std::fs::create_dir(&directory).expect("create partial guard directory");
            let leaf = write_source(
                &directory,
                "leaf.conf",
                "set-option -g @guard-partial yes\n",
            );
            let entry = write_source(
                &directory,
                "entry.conf",
                &format!(
                    "source-file partial-missing.conf '{leaf}'\n\
                     display-message -p sourced-output\n"
                ),
            );
            let partial_input =
                format!("source-file '{entry}'\ndisplay-message -p partial-fresh\n");
            let partial_marker = directory.join("partial.complete");
            let partial_output = run_control_until_return(
                &fixture,
                &["-C", "new-session", "-s", "partial-guards"],
                &partial_input,
                &partial_marker,
                "partial guard completion",
            );
            assert_eq!(partial_output.status.code(), Some(1));
            assert!(partial_output.stderr.is_empty());
            let partial_stream = parse_stream_allow_gaps(&partial_output.stdout, false);
            assert_eq!(partial_stream.blocks.len(), 7);
            assert_block(&partial_stream.blocks[0], 1, 0, &[], false);
            assert_block(&partial_stream.blocks[1], 2, 1, &[], false);
            assert_block(
                &partial_stream.blocks[2],
                3,
                1,
                &["No such file or directory: partial-missing.conf"],
                false,
            );
            assert_block(&partial_stream.blocks[3], 4, 1, &[], false);
            assert_block(&partial_stream.blocks[4], 6, 1, &["sourced-output"], false);
            assert_block(&partial_stream.blocks[5], 8, 1, &["partial-fresh"], false);
            assert_block(&partial_stream.blocks[6], 9, 1, &[], false);
            assert_attached_startup(&partial_stream.outside, "partial-guards");
        }

        #[test]
        fn control_source_read_error_follows_the_parent_guard_and_sets_retval() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let unreadable = fixture._directory.path().join("source read directory");
            std::fs::create_dir(&unreadable).expect("create unreadable source directory");
            let entry = write_source(
                fixture._directory.path(),
                "read-entry.conf",
                &format!("source-file '{}'\n", unreadable.display()),
            );
            let read_error = std::fs::read_to_string(&unreadable)
                .expect_err("reading the source directory must fail");
            let expected = format!("{}: {}", os_error_text(&read_error), unreadable.display());
            let input = format!("source-file '{entry}'\ndisplay-message -p after-read-error\n");
            let marker = fixture._directory.path().join("source-read.complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "new-session", "-s", "source-read-error"],
                &input,
                &marker,
                "source read completion",
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let lines = std::str::from_utf8(&output.stdout)
                .expect("UTF-8 source read output")
                .lines()
                .collect::<Vec<_>>();
            let error_index = lines
                .iter()
                .position(|line| line == &expected)
                .expect("raw source read error");
            let before = next_block_guard(lines[..error_index].iter().rev().copied());
            assert!(
                before.starts_with("%end ") && before.ends_with(" 3 1"),
                "{lines:?}"
            );
            let after = next_block_guard(lines[error_index + 1..].iter().copied());
            assert!(
                after.starts_with("%begin ") && after.ends_with(" 6 1"),
                "{lines:?}"
            );
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 5);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(&stream.blocks[2], 3, 1, &[], false);
            assert_block(&stream.blocks[3], 6, 1, &["after-read-error"], false);
            assert_block(&stream.blocks[4], 7, 1, &[], false);
            assert!(stream.outside.contains(&expected));
            let outside = stream
                .outside
                .into_iter()
                .filter(|line| line != &expected)
                .collect::<Vec<_>>();
            assert_attached_startup(&outside, "source-read-error");
        }

        #[test]
        fn control_source_diagnostics_keep_tmux_chain_semantics() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("nested control");
            std::fs::create_dir(&directory).expect("create nested source directory");
            let loud = directory.join("loud entry.conf");
            let quiet = directory.join("quiet entry.conf");
            let invalid = directory.join("invalid entry.conf");
            std::fs::write(&loud, "source-file nested-missing.conf\n")
                .expect("write loud nested source");
            std::fs::write(&quiet, "source-file -q nested-missing.conf\n")
                .expect("write quiet nested source");
            std::fs::write(&invalid, "wibble\n").expect("write invalid source");
            let input = format!(
                "source-file '{}' ; display-message -p same-nested\nsource-file top-missing.conf ; display-message -p same-top-missing\nsource-file '{}' ; display-message -p same-invalid\nsource-file '{}'\ndisplay-message -p fresh\n",
                loud.display(),
                invalid.display(),
                quiet.display()
            );
            let marker = directory.join("source-diagnostics.complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "new-session", "-s", "nested-source-error"],
                &input,
                &marker,
                "source diagnostic completion",
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 11);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(
                &stream.blocks[2],
                3,
                1,
                &["No such file or directory: nested-missing.conf"],
                true,
            );
            assert_block(&stream.blocks[3], 6, 1, &["same-nested"], false);
            assert_block(
                &stream.blocks[4],
                7,
                1,
                &["No such file or directory: top-missing.conf"],
                true,
            );
            assert_block(&stream.blocks[5], 9, 1, &[], false);
            assert_block(&stream.blocks[6], 11, 1, &["same-invalid"], false);
            assert_block(&stream.blocks[7], 12, 1, &[], false);
            assert_block(&stream.blocks[8], 13, 1, &[], false);
            assert_block(&stream.blocks[9], 16, 1, &["fresh"], false);
            assert_block(&stream.blocks[10], 17, 1, &[], false);
            let diagnostic = format!(
                "%config-error {}:1: unknown command: wibble",
                invalid.display()
            );
            assert!(stream.outside.contains(&diagnostic));
            let outside = stream
                .outside
                .into_iter()
                .filter(|line| line != &diagnostic)
                .collect::<Vec<_>>();
            assert_attached_startup(&outside, "nested-source-error");
        }

        #[test]
        fn control_replayed_runtime_errors_use_bare_error_blocks_and_continue() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("control replay errors");
            std::fs::create_dir(&directory).expect("create control replay directory");
            let source = write_source(
                &directory,
                "runtime.conf",
                "kill-session -t missing-runtime\n\
                 set-option -g nonexistent-option value\n\
                 set-environment -g \"\" value\n\
                 set-option -g @runtime-after yes\n",
            );
            let input = format!(
                "source-file '{source}'\nshow-options -gqv @runtime-after\ndisplay-message -p fresh\n"
            );
            let marker = directory.join("runtime.complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "new-session", "-s", "control-replay-errors"],
                &input,
                &marker,
                "runtime replay completion",
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 9);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(
                &stream.blocks[2],
                3,
                1,
                &["can't find session: missing-runtime"],
                true,
            );
            assert_block(
                &stream.blocks[3],
                4,
                1,
                &["invalid option: nonexistent-option"],
                true,
            );
            assert_block(&stream.blocks[4], 5, 1, &["empty variable name"], true);
            assert_block(&stream.blocks[5], 6, 1, &[], false);
            assert_block(&stream.blocks[6], 8, 1, &["yes"], false);
            assert_block(&stream.blocks[7], 9, 1, &["fresh"], false);
            assert_block(&stream.blocks[8], 10, 1, &[], false);
            assert_attached_startup(&stream.outside, "control-replay-errors");
        }

        #[test]
        fn control_return_and_explicit_detach_follow_the_full_retval_matrix() {
            #[derive(Clone, Copy)]
            enum ExitPath {
                Eof,
                Blank,
                DetachCompleted,
                DetachQueuedOpen,
                DetachQueuedEof,
            }

            struct Row {
                name: &'static str,
                command: String,
                return_code: i32,
                payload: &'static str,
                error: bool,
            }

            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("control return matrix");
            std::fs::create_dir(&directory).expect("create control return matrix directory");
            let runtime_source = write_source(
                &directory,
                "runtime.conf",
                "kill-session -t matrix-sourced-runtime\n",
            );
            let source_failure = write_source(
                &directory,
                "source-failure.conf",
                "source-file matrix-nested-missing.conf\n",
            );
            let rows = [
                Row {
                    name: "direct-runtime",
                    command: "kill-session -t matrix-direct-runtime".to_owned(),
                    return_code: 1,
                    payload: "can't find session: matrix-direct-runtime",
                    error: true,
                },
                Row {
                    name: "sourced-runtime",
                    command: format!("source-file '{runtime_source}'"),
                    return_code: 1,
                    payload: "can't find session: matrix-sourced-runtime",
                    error: true,
                },
                Row {
                    name: "sourced-command",
                    command: format!("source-file '{source_failure}'"),
                    return_code: 1,
                    payload: "No such file or directory: matrix-nested-missing.conf",
                    error: true,
                },
                Row {
                    name: "generic-nonzero",
                    command: "run-shell 'exit 3'".to_owned(),
                    return_code: 0,
                    payload: "'exit 3' returned 3",
                    error: false,
                },
            ];
            let exits = [
                ("eof", ExitPath::Eof),
                ("blank", ExitPath::Blank),
                ("detach-completed", ExitPath::DetachCompleted),
                ("detach-queued-open", ExitPath::DetachQueuedOpen),
                ("detach-queued-eof", ExitPath::DetachQueuedEof),
            ];

            for (row_index, row) in rows.iter().enumerate() {
                for (exit_index, (exit_name, exit_path)) in exits.iter().enumerate() {
                    let label = format!("{}-{exit_name}", row.name);
                    let session = format!("matrix-{row_index}-{exit_index}");
                    let ready = directory.join(format!("{label}.ready"));
                    let release = directory.join(format!("{label}.release"));
                    let output_path = directory.join(format!("{label}.output"));
                    let (mut child, mut stdin) = spawn_control_to_file(
                        &fixture,
                        &["-C", "new-session", "-s", &session, "exec /bin/cat"],
                        &output_path,
                    );
                    writeln!(stdin, "{}", row.command).expect("write matrix command");

                    match exit_path {
                        ExitPath::Eof | ExitPath::Blank | ExitPath::DetachCompleted => {
                            let complete = format!("MATRIX_COMPLETE_{row_index}_{exit_index}");
                            writeln!(stdin, "display-message -p {complete}")
                                .expect("write completion marker command");
                            stdin.flush().expect("flush completed matrix commands");
                            wait_for_control_output_marker(
                                &output_path,
                                &complete,
                                &mut child,
                                &label,
                            );
                        }
                        ExitPath::DetachQueuedOpen | ExitPath::DetachQueuedEof => {
                            writeln!(
                                stdin,
                                "run-shell 'touch \"{}\"; while [ ! -e \"{}\" ]; do sleep 0.01; done'",
                                ready.display(),
                                release.display()
                            )
                            .expect("write held matrix command");
                            stdin.flush().expect("flush held matrix commands");
                            wait_for_control_marker(&ready, &mut child, &label);
                        }
                    }

                    let output = match exit_path {
                        ExitPath::Eof => {
                            drop(stdin);
                            collect_control_process(child, None, &label)
                        }
                        ExitPath::Blank => {
                            writeln!(stdin).expect("write matrix blank return");
                            stdin.flush().expect("flush matrix blank return");
                            collect_control_process(child, Some(stdin), &label)
                        }
                        ExitPath::DetachCompleted => {
                            writeln!(stdin, "detach-client").expect("write completed detach");
                            stdin.flush().expect("flush completed detach");
                            collect_control_process(child, Some(stdin), &label)
                        }
                        ExitPath::DetachQueuedOpen => {
                            writeln!(stdin, "detach-client").expect("queue held-open detach");
                            stdin.flush().expect("flush held-open detach");
                            std::fs::write(&release, b"").expect("release held-open command");
                            collect_control_process(child, Some(stdin), &label)
                        }
                        ExitPath::DetachQueuedEof => {
                            writeln!(stdin, "detach-client").expect("queue detach before EOF");
                            stdin.flush().expect("flush detach before EOF");
                            drop(stdin);
                            thread::sleep(Duration::from_millis(100));
                            std::fs::write(&release, b"").expect("release EOF-held command");
                            collect_control_process(child, None, &label)
                        }
                    };
                    let expected_status = match exit_path {
                        ExitPath::Eof | ExitPath::Blank | ExitPath::DetachQueuedEof => {
                            row.return_code
                        }
                        ExitPath::DetachCompleted | ExitPath::DetachQueuedOpen => 0,
                    };
                    assert_eq!(output.status.code(), Some(expected_status), "{label}");
                    assert!(output.stderr.is_empty(), "{label}");
                    let stdout = std::fs::read(&output_path).expect("read matrix control output");
                    let hidden = match row.name {
                        "sourced-runtime" => 1,
                        "sourced-command" => 2,
                        _ => 0,
                    };
                    let stream = if hidden == 0 {
                        parse_stream(&stdout, false)
                    } else {
                        parse_stream_allow_gaps(&stdout, false)
                    };
                    assert_eq!(
                        stream.blocks.last().expect("matrix block").number
                            - stream.blocks.len() as u64,
                        hidden,
                        "{label}: {stream:?}"
                    );
                    if row.name == "generic-nonzero" {
                        assert!(
                            stream.outside.iter().any(|line| line == row.payload),
                            "{label}: {stream:?}"
                        );
                        assert!(
                            stream.blocks.iter().any(|block| {
                                block.flags == 1 && !block.error && block.payload.is_empty()
                            }),
                            "{label}: {stream:?}"
                        );
                    } else {
                        assert!(
                            stream.blocks.iter().any(|block| {
                                block.error == row.error
                                    && block.payload.iter().any(|line| line == row.payload)
                            }),
                            "{label}: {stream:?}"
                        );
                    }
                    assert_eq!(
                        stream.outside.last().map(String::as_str),
                        Some("%exit"),
                        "{label}"
                    );
                }
            }
        }

        #[test]
        fn control_immediate_eof_keeps_a_direct_source_failure_status() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let missing = fixture._directory.path().join("immediate-eof-missing.conf");
            let output = fixture.run_with_stdin(
                &[
                    "-C",
                    "new-session",
                    "-s",
                    "immediate-eof-source",
                    "exec /bin/cat",
                ],
                format!("source-file '{}'\n", missing.display()).as_bytes(),
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert!(stream.blocks.iter().any(|block| {
                block.error
                    && block
                        .payload
                        .iter()
                        .any(|line| line.contains("immediate-eof-missing.conf"))
            }));
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
        }

        #[test]
        fn control_attached_eof_keeps_the_current_admitted_failure_status() {
            for kind in ["runtime", "source", "run-shell", "confirm-before"] {
                let fixture = Fixture::new();
                if !local_socket_bind_available(&fixture.socket) {
                    return;
                }
                let session = format!("attached-eof-{kind}");
                assert!(
                    fixture
                        .run(&["new-session", "-d", "-s", &session, "exec /bin/cat"])
                        .status
                        .success()
                );
                let output_path = fixture
                    ._directory
                    .path()
                    .join(format!("attached-eof-{kind}.output"));
                let missing = fixture
                    ._directory
                    .path()
                    .join("attached-eof-source-missing.conf");
                let (failure, expected) = match kind {
                    "runtime" => (
                        "kill-session -t attached-eof-runtime-missing".to_owned(),
                        "can't find session: attached-eof-runtime-missing",
                    ),
                    "source" => (
                        format!("source-file '{}'", missing.display()),
                        "attached-eof-source-missing.conf",
                    ),
                    "run-shell" => (
                        "run-shell -d not-a-number 'true'".to_owned(),
                        "invalid delay time: not-a-number",
                    ),
                    "confirm-before" => (
                        "confirm-before -c xx { display-message -p no }".to_owned(),
                        "invalid confirm key",
                    ),
                    _ => unreachable!(),
                };
                let blocker_ready = fixture
                    ._directory
                    .path()
                    .join(format!("attached-eof-{kind}.ready"));
                let blocker_release = fixture
                    ._directory
                    .path()
                    .join(format!("attached-eof-{kind}.release"));
                let (mut child, mut stdin) = spawn_control_to_file(
                    &fixture,
                    &["-C", "attach-session", "-t", &format!("={session}")],
                    &output_path,
                );
                wait_for_control_clients(&fixture, 1, &format!("attached EOF {kind}"));
                writeln!(stdin, "display-message -p ATTACHED_EOF_READY")
                    .expect("write attached EOF readiness command");
                stdin.flush().expect("flush attached EOF readiness command");
                wait_for_control_output_marker(
                    &output_path,
                    "ATTACHED_EOF_READY",
                    &mut child,
                    &format!("attached EOF {kind} readiness"),
                );
                writeln!(
                    stdin,
                    "run-shell 'touch \"{}\"; while [ ! -e \"{}\" ]; do sleep 0.01; done' ; {failure}",
                    blocker_ready.display(),
                    blocker_release.display(),
                )
                .expect("write attached EOF failure");
                stdin.flush().expect("flush attached EOF failure");
                wait_for_control_marker(
                    &blocker_ready,
                    &mut child,
                    &format!("attached EOF {kind} blocker"),
                );
                drop(stdin);
                thread::sleep(Duration::from_millis(100));
                std::fs::write(&blocker_release, b"").expect("release attached EOF blocker");

                let output =
                    collect_control_process(child, None, &format!("attached EOF {kind} failure"));
                assert_eq!(output.status.code(), Some(1));
                assert!(output.stderr.is_empty());
                let stdout = std::fs::read(&output_path).expect("read attached EOF output");
                let stream = parse_stream_allow_gaps(&stdout, false);
                assert!(
                    stream.blocks.iter().any(|block| {
                        block.error && block.payload.iter().any(|line| line.contains(expected))
                    }),
                    "{stream:?}"
                );
                assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
            }
        }

        #[test]
        fn control_nonself_detach_stays_attached_and_preserves_queued_return() {
            for (label, detach, create_other, blank_return) in [
                ("detach-others", "detach-client -a", false, true),
                (
                    "detach-other-session",
                    "detach-client -s =scope-other",
                    true,
                    false,
                ),
                (
                    "detach-missing-session",
                    "detach-client -s =scope-missing",
                    false,
                    false,
                ),
            ] {
                let fixture = Fixture::new();
                if !local_socket_bind_available(&fixture.socket) {
                    return;
                }
                assert!(
                    fixture
                        .run(&["new-session", "-d", "-s", "scope-caller", "exec /bin/cat",])
                        .status
                        .success()
                );
                if create_other {
                    assert!(
                        fixture
                            .run(&["new-session", "-d", "-s", "scope-other", "exec /bin/cat",])
                            .status
                            .success()
                    );
                }
                let output_path = fixture._directory.path().join(format!("{label}.output"));
                let (mut child, mut stdin) = spawn_control_to_file(
                    &fixture,
                    &["-C", "attach-session", "-t", "=scope-caller"],
                    &output_path,
                );
                wait_for_control_clients(&fixture, 1, label);
                let ready = format!("{}_READY", label.replace('-', "_").to_uppercase());
                prime_control_return_code(
                    &mut child,
                    &mut stdin,
                    &output_path,
                    &ready,
                    &format!("{label}-missing"),
                );
                let after = format!("{}_AFTER", label.replace('-', "_").to_uppercase());
                writeln!(stdin, "{detach}").expect("write nonself detach");
                writeln!(stdin, "display-message -p {after}")
                    .expect("write command after nonself detach");
                stdin.flush().expect("flush nonself detach commands");
                wait_for_control_output_marker(&output_path, &after, &mut child, label);
                let output = if blank_return {
                    writeln!(stdin).expect("write nonself blank return");
                    stdin.flush().expect("flush nonself blank return");
                    collect_control_process(child, Some(stdin), label)
                } else {
                    drop(stdin);
                    collect_control_process(child, None, label)
                };
                assert_eq!(output.status.code(), Some(1), "{label}");
                assert!(output.stderr.is_empty(), "{label}");
                let stdout = std::fs::read(&output_path).expect("read nonself detach output");
                let stream = parse_stream(&stdout, false);
                assert!(
                    stream
                        .blocks
                        .iter()
                        .any(|block| block.payload == [after.as_str()]),
                    "{label}: {stream:?}"
                );
                assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
            }

            for (label, blank_return, continue_after) in [
                ("target-other-eof", false, false),
                ("target-other-blank", true, false),
                ("target-other-continue", true, true),
            ] {
                let fixture = Fixture::new();
                if !local_socket_bind_available(&fixture.socket) {
                    return;
                }
                assert!(
                    fixture
                        .run(&["new-session", "-d", "-s", "target-other", "exec /bin/cat",])
                        .status
                        .success()
                );
                let (peer, peer_stdin) =
                    fixture.spawn_with_open_stdin(&["-C", "attach-session", "-t", "=target-other"]);
                let peer_name = wait_for_control_clients(&fixture, 1, label)
                    .into_iter()
                    .next()
                    .expect("peer control client");
                let output_path = fixture._directory.path().join(format!("{label}.output"));
                let (mut child, mut stdin) = spawn_control_to_file(
                    &fixture,
                    &["-C", "attach-session", "-t", "=target-other"],
                    &output_path,
                );
                let clients = wait_for_control_clients(&fixture, 2, label);
                assert!(clients.contains(&peer_name), "{label}: {clients:?}");
                let ready = format!("{}_READY", label.replace('-', "_").to_uppercase());
                prime_control_return_code(
                    &mut child,
                    &mut stdin,
                    &output_path,
                    &ready,
                    &format!("{label}-missing"),
                );
                writeln!(stdin, "detach-client -t '{peer_name}'")
                    .expect("write target-other detach");
                let output = if continue_after {
                    writeln!(stdin, "display-message -p TARGET_OTHER_AFTER")
                        .expect("write command after target-other detach");
                    stdin.flush().expect("flush target-other continuation");
                    wait_for_control_output_marker(
                        &output_path,
                        "TARGET_OTHER_AFTER",
                        &mut child,
                        label,
                    );
                    writeln!(stdin).expect("write target-other continuation return");
                    stdin
                        .flush()
                        .expect("flush target-other continuation return");
                    collect_control_process(child, Some(stdin), label)
                } else if blank_return {
                    writeln!(stdin).expect("write target-other blank return");
                    stdin.flush().expect("flush target-other blank return");
                    collect_control_process(child, Some(stdin), label)
                } else {
                    stdin.flush().expect("flush target-other detach");
                    drop(stdin);
                    collect_control_process(child, None, label)
                };
                assert_eq!(output.status.code(), Some(1), "{label}");
                assert!(output.stderr.is_empty(), "{label}");
                let peer_output = collect_control_process(peer, Some(peer_stdin), label);
                assert_eq!(peer_output.status.code(), Some(0), "{label}");
                assert!(peer_output.stderr.is_empty(), "{label}");
            }
        }

        #[test]
        fn control_detach_aliases_follow_the_authoritative_self_victim() {
            for (label, command, install_alias) in [
                ("bare-self-detach", "detach-client".to_owned(), false),
                ("built-in-alias-self-detach", "detach".to_owned(), false),
                ("alias-self-detach", "dc".to_owned(), true),
                (
                    "scoped-self-detach",
                    "detach-client -s =scoped-self-detach".to_owned(),
                    false,
                ),
            ] {
                let fixture = Fixture::new();
                if !local_socket_bind_available(&fixture.socket) {
                    return;
                }
                assert!(
                    fixture
                        .run(&["new-session", "-d", "-s", label, "exec /bin/cat",])
                        .status
                        .success()
                );
                if install_alias {
                    assert!(
                        fixture
                            .run(&["set-option", "-s", "command-alias[40]", "dc=detach-client",])
                            .status
                            .success()
                    );
                }
                let output_path = fixture._directory.path().join(format!("{label}.output"));
                let (mut child, mut stdin) = spawn_control_to_file(
                    &fixture,
                    &["-C", "attach-session", "-t", &format!("={label}")],
                    &output_path,
                );
                wait_for_control_clients(&fixture, 1, label);
                let ready = format!("{}_READY", label.replace('-', "_").to_uppercase());
                prime_control_return_code(
                    &mut child,
                    &mut stdin,
                    &output_path,
                    &ready,
                    &format!("{label}-missing"),
                );
                writeln!(stdin, "{command}").expect("write self detach");
                stdin.flush().expect("flush self detach");
                drop(stdin);
                let output = collect_control_process(child, None, label);
                assert_eq!(output.status.code(), Some(0), "{label}");
                assert!(output.stderr.is_empty(), "{label}");
                let stdout = std::fs::read(&output_path).expect("read self-detach output");
                let stream = parse_stream(&stdout, false);
                assert_block(
                    stream.blocks.last().expect("self-detach response block"),
                    4,
                    1,
                    &[],
                    false,
                );
                assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
            }

            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            assert!(
                fixture
                    .run(&[
                        "new-session",
                        "-d",
                        "-s",
                        "alias-other-detach",
                        "exec /bin/cat",
                    ])
                    .status
                    .success()
            );
            assert!(
                fixture
                    .run(&["set-option", "-s", "command-alias[40]", "dc=detach-client",])
                    .status
                    .success()
            );
            let output_path = fixture._directory.path().join("alias-other-detach.output");
            let (mut child, mut stdin) = spawn_control_to_file(
                &fixture,
                &["-C", "attach-session", "-t", "=alias-other-detach"],
                &output_path,
            );
            wait_for_control_clients(&fixture, 1, "alias other detach");
            prime_control_return_code(
                &mut child,
                &mut stdin,
                &output_path,
                "ALIAS_OTHER_READY",
                "alias-other-missing",
            );
            writeln!(stdin, "dc -a").expect("write aliased nonself detach");
            writeln!(stdin, "display-message -p ALIAS_OTHER_AFTER")
                .expect("write command after aliased nonself detach");
            stdin.flush().expect("flush aliased nonself detach");
            wait_for_control_output_marker(
                &output_path,
                "ALIAS_OTHER_AFTER",
                &mut child,
                "alias other detach",
            );
            writeln!(stdin).expect("write aliased nonself blank return");
            stdin.flush().expect("flush aliased nonself blank return");
            let output =
                collect_control_process(child, Some(stdin), "aliased nonself detach return");
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
        }

        #[test]
        fn control_return_snapshot_precedes_later_failure_and_detach_eof_is_zero() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("control return precedence");
            std::fs::create_dir(&directory).expect("create return precedence directory");
            for wrapper in ["if-shell", "if-shell-format", "run-shell"] {
                let ready = directory.join(format!("pre-{wrapper}-failure.ready"));
                let release = directory.join(format!("pre-{wrapper}-failure.release"));
                let missing = format!("pre-{wrapper}-failure-missing");
                let blocker = format!(
                    "run-shell 'touch \"{}\"; while [ ! -e \"{}\" ]; do sleep 0.01; done'",
                    ready.display(),
                    release.display()
                );
                let delayed_command = match wrapper {
                    "if-shell" => format!(
                        "if-shell 'touch \"{}\"; while [ ! -e \"{}\" ]; do sleep 0.01; done; true' 'kill-session -t {missing}'",
                        ready.display(),
                        release.display()
                    ),
                    "if-shell-format" => {
                        format!("if-shell -F 1 {{ {blocker} ; kill-session -t {missing} }}")
                    }
                    "run-shell" => {
                        format!("run-shell -C {{ {blocker} ; kill-session -t {missing} }}")
                    }
                    _ => unreachable!(),
                };
                let session = format!("pre-{wrapper}-failure-return");
                let (mut child, mut stdin) = fixture.spawn_with_open_stdin(&[
                    "-C",
                    "new-session",
                    "-s",
                    &session,
                    "exec /bin/cat",
                ]);
                writeln!(stdin, "{delayed_command}").expect("write delayed command");
                stdin.flush().expect("flush delayed command");
                wait_for_control_marker(
                    &ready,
                    &mut child,
                    &format!("pre-{wrapper}-failure command wait"),
                );
                drop(stdin);
                thread::sleep(Duration::from_millis(100));
                std::fs::write(&release, b"").expect("release delayed source");
                let output = collect_control_process(
                    child,
                    None,
                    &format!("pre-{wrapper}-failure EOF snapshot"),
                );
                let stream = parse_stream(&output.stdout, false);
                assert_eq!(output.status.code(), Some(0), "{stream:?}");
                assert!(output.stderr.is_empty());
                assert!(
                    stream.blocks.iter().any(|block| {
                        block.error && block.payload.iter().any(|line| line.contains(&missing))
                    }),
                    "{stream:?}"
                );
                assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
            }

            let runtime_source = write_source(
                &directory,
                "detach-runtime.conf",
                "kill-session -t post-detach-missing\n",
            );
            let complete = "POST_DETACH_COMPLETE";
            let output_path = directory.join("post-detach.output");
            let output_file = File::create(&output_path).expect("create live control output");
            let mut child = fixture
                .command()
                .args([
                    "-C",
                    "new-session",
                    "-s",
                    "post-detach-eof",
                    "exec /bin/cat",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::from(output_file))
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn post-detach control");
            let mut stdin = child.stdin.take().expect("piped control stdin");
            writeln!(stdin, "source-file '{runtime_source}'").expect("write detach source");
            writeln!(stdin, "display-message -p {complete}")
                .expect("write detach completion marker");
            stdin.flush().expect("flush detach completion commands");
            wait_for_control_output_marker(
                &output_path,
                complete,
                &mut child,
                "post-detach completed frame",
            );
            writeln!(stdin, "detach-client").expect("write post-completion detach");
            stdin.flush().expect("flush post-completion detach");
            drop(stdin);
            let output = collect_control_process(child, None, "post-detach immediate EOF");
            assert_eq!(
                output.status.code(),
                Some(0),
                "stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&std::fs::read(&output_path).expect("read control output")),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            let stdout = std::fs::read(output_path).expect("read completed control output");
            let stream = parse_stream_allow_gaps(&stdout, false);
            assert_eq!(
                stream.blocks.last().expect("post-detach block").number
                    - stream.blocks.len() as u64,
                1
            );
            assert!(stream.blocks.iter().any(|block| {
                block.error && block.payload == ["can't find session: post-detach-missing"]
            }));
        }

        #[test]
        fn control_server_stop_preserves_the_completed_return_code() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("control server stop status");
            std::fs::create_dir(&directory).expect("create server stop status directory");
            let source = write_source(
                &directory,
                "runtime.conf",
                "kill-session -t server-stop-missing\n",
            );
            let complete = directory.join("runtime.complete");
            let (mut child, mut stdin) = fixture.spawn_with_open_stdin(&[
                "-C",
                "new-session",
                "-s",
                "server-stop-status",
                "exec /bin/cat",
            ]);
            writeln!(stdin, "source-file '{source}'").expect("write server stop source");
            writeln!(stdin, "run-shell 'touch \"{}\"'", complete.display())
                .expect("write server stop completion marker");
            stdin.flush().expect("flush server stop setup");
            wait_for_control_marker(&complete, &mut child, "server stop completion");
            writeln!(stdin, "kill-server").expect("write server stop command");
            stdin.flush().expect("flush server stop command");
            let output = collect_control_process(child, Some(stdin), "server stop status");
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert_eq!(
                stream.blocks.last().expect("server-stop block").number
                    - stream.blocks.len() as u64,
                1
            );
            assert!(stream.blocks.iter().any(|block| {
                block.error && block.payload == ["can't find session: server-stop-missing"]
            }));
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
        }

        #[test]
        fn control_nested_source_depth_limit_uses_sourced_command_guards() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let directory = fixture._directory.path().join("control depth");
            std::fs::create_dir(&directory).expect("create control depth directory");
            let entry = write_source_chain(&directory, 50);
            let input = format!("source-file '{entry}' ; display-message -p after-the-limit\n");
            let marker = directory.join("depth.complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "new-session", "-s", "control-depth", "exec /bin/cat"],
                &input,
                &marker,
                "nested source depth completion",
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 155);
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            let errors = stream
                .blocks
                .iter()
                .filter(|block| block.error)
                .collect::<Vec<_>>();
            assert_eq!(errors.len(), 2);
            assert!(
                errors
                    .iter()
                    .all(|block| block.payload == ["too many nested files"])
            );
            assert_block(&stream.blocks[153], 204, 1, &["after-the-limit"], false);
            assert_block(&stream.blocks[154], 205, 1, &[], false);
            assert_eq!(stream.blocks[154].number - stream.blocks.len() as u64, 50);
            assert_attached_startup(&stream.outside, "control-depth");
        }

        #[test]
        fn control_background_inserted_commands_use_flags_zero_after_later_input() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let session = "control-background-frames";
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", session, "exec /bin/cat"])
                    .status
                    .success()
            );
            let directory = fixture._directory.path().join("control background frames");
            std::fs::create_dir(&directory).expect("create background frame directory");
            let child_source = write_source(
                &directory,
                "child.conf",
                "display-message -p BACKGROUND_CHILD\n",
            );
            let missing_source = directory.join("missing.conf");
            let output_path = directory.join("control.raw");
            let (mut child, mut stdin) = spawn_control_to_file(
                &fixture,
                &["-C", "attach-session", "-t", &format!("={session}")],
                &output_path,
            );
            writeln!(
                stdin,
                "run-shell -bC -d 0.3 'source-file \"{child_source}\"'"
            )
            .expect("write background source command");
            writeln!(
                stdin,
                "run-shell -bC -d 0.6 'source-file \"{}\"'",
                missing_source.display()
            )
            .expect("write background missing source command");
            writeln!(
                stdin,
                "run-shell -bC -d 0.9 'kill-session -t background-runtime-missing'"
            )
            .expect("write background runtime command");
            writeln!(
                stdin,
                "run-shell -bC -d 1.2 'display-message -p BACKGROUND_RUN'"
            )
            .expect("write background success command");
            writeln!(
                stdin,
                "if-shell -b 'sleep 1.5; false' 'display-message -p WRONG_BRANCH' 'display-message -p BACKGROUND_ELSE'"
            )
            .expect("write background false branch");
            writeln!(stdin, "display-message -p BACKGROUND_LATER")
                .expect("write later flags-one command");
            stdin.flush().expect("flush background commands");

            wait_for_control_output_marker(
                &output_path,
                "BACKGROUND_LATER",
                &mut child,
                "later flags-one frame",
            );
            wait_for_control_output_marker(
                &output_path,
                "BACKGROUND_CHILD",
                &mut child,
                "background child frame",
            );
            wait_for_control_error_marker(
                &output_path,
                &format!("No such file or directory: {}", missing_source.display()),
                &mut child,
                "background missing source frame",
            );
            wait_for_control_error_marker(
                &output_path,
                "can't find session: background-runtime-missing",
                &mut child,
                "background runtime frame",
            );
            wait_for_control_output_marker(
                &output_path,
                "BACKGROUND_RUN",
                &mut child,
                "background run-shell frame",
            );
            wait_for_control_output_marker(
                &output_path,
                "BACKGROUND_ELSE",
                &mut child,
                "background false branch frame",
            );
            writeln!(stdin, "display-message -p BACKGROUND_STICKY_LATER")
                .expect("write sticky-status later command");
            stdin.flush().expect("flush sticky-status later command");
            wait_for_control_output_marker(
                &output_path,
                "BACKGROUND_STICKY_LATER",
                &mut child,
                "sticky-status later frame",
            );
            writeln!(stdin).expect("write background return");
            stdin.flush().expect("flush background return");
            let output = collect_control_process(child, Some(stdin), "background flags zero");
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let stdout = std::fs::read(&output_path).expect("read background control output");
            let stream = parse_stream_allow_gaps(&stdout, false);
            assert_eq!(stream.blocks.len(), 14, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(&stream.blocks[2], 3, 1, &[], false);
            assert_block(&stream.blocks[3], 4, 1, &[], false);
            assert_block(&stream.blocks[4], 5, 1, &[], false);
            assert_block(&stream.blocks[5], 6, 1, &[], false);
            assert_block(&stream.blocks[6], 7, 1, &["BACKGROUND_LATER"], false);
            assert_block(&stream.blocks[7], 8, 0, &[], false);
            assert_block(&stream.blocks[8], 9, 0, &["BACKGROUND_CHILD"], false);
            assert_block(
                &stream.blocks[9],
                11,
                0,
                &[&format!(
                    "No such file or directory: {}",
                    missing_source.display()
                )],
                true,
            );
            assert_block(
                &stream.blocks[10],
                13,
                0,
                &["can't find session: background-runtime-missing"],
                true,
            );
            assert_block(&stream.blocks[11], 14, 0, &["BACKGROUND_RUN"], false);
            assert_block(&stream.blocks[12], 15, 0, &["BACKGROUND_ELSE"], false);
            assert_block(
                &stream.blocks[13],
                16,
                1,
                &["BACKGROUND_STICKY_LATER"],
                false,
            );
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == &format!("%session-changed $0 {session}"))
            );
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
        }

        #[test]
        fn held_control_detached_shutdown_drains_callback_guards_before_exit() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let session = "control-background-shutdown";
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", session, "exec /bin/cat"])
                    .status
                    .success()
            );
            let output_path = fixture._directory.path().join("control.raw");
            let (child, mut stdin) = spawn_control_to_file(
                &fixture,
                &["-C", "attach-session", "-t", &format!("={session}")],
                &output_path,
            );
            writeln!(
                stdin,
                "run-shell -bC 'kill-server ; display-message -p AFTER'"
            )
            .expect("write detached shutdown command");
            stdin.flush().expect("flush detached shutdown command");

            let output = collect_control_process(child, Some(stdin), "detached shutdown callback");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stdout = std::fs::read(&output_path).expect("read detached shutdown output");
            let stream = parse_stream(&stdout, false);
            assert_eq!(stream.blocks.len(), 4, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(&stream.blocks[2], 3, 0, &[], false);
            assert_block(&stream.blocks[3], 4, 0, &["AFTER"], false);
            let lines = std::str::from_utf8(&stdout)
                .expect("UTF-8 detached shutdown output")
                .lines()
                .collect::<Vec<_>>();
            assert_eq!(lines.iter().filter(|line| **line == "%exit").count(), 1);
            assert_eq!(lines.last().copied(), Some("%exit"));

            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", session, "exec /bin/cat"])
                    .status
                    .success()
            );
            let scenarios = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/scenarios");
            let root = scenarios.join("source-file-output-hook-root.conf");
            let child = scenarios.join("source-file-output-child.conf");
            let hook = format!("source-file '{}'", child.display());
            assert!(
                fixture
                    .run(&["set-hook", "-g", "after-display-message", hook.as_str()])
                    .status
                    .success()
            );
            let output_path = fixture._directory.path().join("nested-control.raw");
            let (child, mut stdin) = spawn_control_to_file(
                &fixture,
                &["-C", "attach-session", "-t", &format!("={session}")],
                &output_path,
            );
            writeln!(
                stdin,
                "run-shell -bC 'kill-server ; source-file \"{}\" ; display-message -p OUTER'",
                root.display()
            )
            .expect("write nested detached shutdown command");
            stdin
                .flush()
                .expect("flush nested detached shutdown command");

            let output =
                collect_control_process(child, Some(stdin), "nested detached shutdown callback");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stdout = std::fs::read(&output_path).expect("read nested detached shutdown output");
            let stream = parse_stream_allow_gaps(&stdout, false);
            assert_eq!(stream.blocks.len(), 7, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(&stream.blocks[2], 3, 0, &[], false);
            assert_block(&stream.blocks[3], 4, 0, &[], false);
            assert_block(&stream.blocks[4], 5, 0, &["HOOK_TRIGGER"], false);
            assert_block(&stream.blocks[5], 6, 0, &[], false);
            assert_block(&stream.blocks[6], 9, 0, &["OUTER"], false);
            let payloads = stream
                .blocks
                .iter()
                .flat_map(|block| block.payload.iter().map(String::as_str))
                .collect::<Vec<_>>();
            for suppressed in ["HOOK_LATER", "CHILD_ONE", "CHILD_TWO"] {
                assert!(!payloads.contains(&suppressed), "{stream:?}");
            }
            let lines = std::str::from_utf8(&stdout)
                .expect("UTF-8 nested detached shutdown output")
                .lines()
                .collect::<Vec<_>>();
            assert_eq!(lines.iter().filter(|line| **line == "%exit").count(), 1);
            assert_eq!(lines.last().copied(), Some("%exit"));
        }

        #[test]
        fn control_background_malformed_lists_and_shell_jobs_stay_unframed() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let session = "control-background-silent";
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", session, "exec /bin/cat"])
                    .status
                    .success()
            );
            let directory = fixture._directory.path().join("control background silent");
            std::fs::create_dir(&directory).expect("create silent background directory");
            let finished = directory.join("condition.finished");
            let output_path = directory.join("control.raw");
            let (mut child, mut stdin) = spawn_control_to_file(
                &fixture,
                &["-C", "attach-session", "-t", &format!("={session}")],
                &output_path,
            );
            writeln!(
                stdin,
                "if-shell -b 'sleep 0.1; touch \"{}\"; true' 'if -x {{'",
                finished.display()
            )
            .expect("write malformed background command");
            writeln!(stdin, "run-shell -bC -d 0.15 'if -x {{'")
                .expect("write malformed background run-shell command");
            writeln!(stdin, "run-shell -b 'sleep 0.05; printf ordinary'")
                .expect("write ordinary background shell command");
            writeln!(stdin, "display-message -p MALFORMED_LATER")
                .expect("write malformed later command");
            stdin.flush().expect("flush malformed commands");

            let deadline = Instant::now() + Duration::from_secs(10);
            while !finished.exists() {
                if let Some(status) = child.try_wait().expect("poll malformed process") {
                    panic!("control process exited before malformed callback: {status}");
                }
                assert!(
                    Instant::now() < deadline,
                    "malformed background condition did not finish"
                );
                thread::sleep(Duration::from_millis(10));
            }
            thread::sleep(Duration::from_millis(200));
            writeln!(stdin, "display-message -p MALFORMED_DONE")
                .expect("write malformed completion command");
            stdin.flush().expect("flush malformed completion command");
            wait_for_control_output_marker(
                &output_path,
                "MALFORMED_DONE",
                &mut child,
                "malformed completion frame",
            );
            writeln!(stdin).expect("write malformed return");
            stdin.flush().expect("flush malformed return");
            let output = collect_control_process(child, Some(stdin), "malformed background list");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stdout = std::fs::read(&output_path).expect("read malformed control output");
            let stream = parse_stream(&stdout, false);
            assert_eq!(stream.blocks.len(), 6, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(&stream.blocks[2], 3, 1, &[], false);
            assert_block(&stream.blocks[3], 4, 1, &[], false);
            assert_block(&stream.blocks[4], 5, 1, &["MALFORMED_LATER"], false);
            assert_block(&stream.blocks[5], 6, 1, &["MALFORMED_DONE"], false);
            assert!(!stream.outside.iter().any(|line| line == "ordinary"));
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == &format!("%session-changed $0 {session}"))
            );
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
        }

        #[test]
        fn control_disconnect_cancels_background_inserted_side_effects() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let session = "control-background-disconnect";
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", session, "exec /bin/cat"])
                    .status
                    .success()
            );
            let wait_marker = fixture._directory.path().join("disconnect.waited");
            assert!(
                fixture
                    .run(&[
                        "run-shell",
                        "-b",
                        &format!("sleep 0.8; touch '{}'", wait_marker.display()),
                    ])
                    .status
                    .success()
            );
            let output_path = fixture._directory.path().join("disconnect.raw");
            let (mut child, mut stdin) = spawn_control_to_file(
                &fixture,
                &["-C", "attach-session", "-t", &format!("={session}")],
                &output_path,
            );
            writeln!(
                stdin,
                "run-shell -bC -d 0.5 'set-option -g @disconnected-run yes'"
            )
            .expect("write disconnected run-shell command");
            writeln!(
                stdin,
                "if-shell -b 'sleep 0.5; true' 'set-option -g @disconnected-if yes'"
            )
            .expect("write disconnected if-shell command");
            writeln!(stdin, "display-message -p DISCONNECT_READY")
                .expect("write disconnect readiness command");
            stdin.flush().expect("flush disconnect commands");
            wait_for_control_output_marker(
                &output_path,
                "DISCONNECT_READY",
                &mut child,
                "disconnect readiness frame",
            );
            drop(stdin);
            let output = collect_control_process(child, None, "background disconnect");
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stderr.is_empty());
            let stdout = std::fs::read(&output_path).expect("read disconnect control output");
            let stream = parse_stream(&stdout, false);
            assert_eq!(stream.blocks.len(), 4, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &[], false);
            assert_block(&stream.blocks[2], 3, 1, &[], false);
            assert_block(&stream.blocks[3], 4, 1, &["DISCONNECT_READY"], false);
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == &format!("%session-changed $0 {session}"))
            );
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));

            let deadline = Instant::now() + Duration::from_secs(10);
            while !wait_marker.exists() {
                assert!(
                    Instant::now() < deadline,
                    "disconnect wait marker did not arrive"
                );
                thread::sleep(Duration::from_millis(10));
            }
            for option in ["@disconnected-run", "@disconnected-if"] {
                let shown = fixture.run(&["show-options", "-gqv", option]);
                assert_eq!(shown.status.code(), Some(0), "{option}");
                assert!(shown.stdout.is_empty(), "{option}");
                assert!(shown.stderr.is_empty(), "{option}");
            }
        }

        #[test]
        fn control_command_hook_frames_are_inserted_and_retain_failure_status() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let session = "control-hook-frames";
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", session, "exec /bin/cat"])
                    .status
                    .success()
            );
            let directory = fixture._directory.path().join("control hook frames");
            std::fs::create_dir(&directory).expect("create hook frame directory");
            let hook_source = write_source(
                &directory,
                "hook.conf",
                "display-message -p HOOK_CHILD\n\
                 kill-session -t hook-runtime-missing\n\
                 display-message -p HOOK_AFTER_ERROR\n",
            );
            assert!(
                fixture
                    .run(&[
                        "set-hook",
                        "-g",
                        "after-display-message[0]",
                        &format!("source-file '{hook_source}'"),
                    ])
                    .status
                    .success()
            );
            assert!(
                fixture
                    .run(&[
                        "set-hook",
                        "-g",
                        "after-display-message[1]",
                        "display-message -p HOOK_LATER_ARRAY",
                    ])
                    .status
                    .success()
            );

            let marker = directory.join("complete");
            let output = run_control_until_return(
                &fixture,
                &["-C", "attach-session", "-t", &format!("={session}")],
                "display-message -p TRIGGER\n\
                 set-hook -gu after-display-message\n\
                 display-message -p LATER_FLAGS_ONE\n",
                &marker,
                "control command hook frames",
            );
            assert_eq!(
                output.status.code(),
                Some(1),
                "stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            assert!(output.stderr.is_empty());
            let stream = parse_stream_allow_gaps(&output.stdout, false);
            assert_eq!(stream.blocks.len(), 10, "{stream:?}");
            assert_block(&stream.blocks[0], 1, 0, &[], false);
            assert_block(&stream.blocks[1], 2, 1, &["TRIGGER"], false);
            assert_block(&stream.blocks[2], 3, 0, &[], false);
            assert_block(&stream.blocks[3], 4, 0, &["HOOK_CHILD"], false);
            assert_block(
                &stream.blocks[4],
                5,
                0,
                &["can't find session: hook-runtime-missing"],
                true,
            );
            assert_block(&stream.blocks[5], 6, 0, &["HOOK_AFTER_ERROR"], false);
            assert_block(&stream.blocks[6], 8, 0, &["HOOK_LATER_ARRAY"], false);
            assert_block(&stream.blocks[7], 9, 1, &[], false);
            assert_block(&stream.blocks[8], 10, 1, &["LATER_FLAGS_ONE"], false);
            assert_block(&stream.blocks[9], 11, 1, &[], false);
            assert!(
                stream
                    .outside
                    .iter()
                    .any(|line| line == &format!("%session-changed $0 {session}"))
            );
            assert_eq!(stream.outside.last().map(String::as_str), Some("%exit"));
        }

        #[test]
        fn control_hook_source_diagnostics_keep_trigger_frames_and_status() {
            let fixture = Fixture::new();
            if !local_socket_bind_available(&fixture.socket) {
                return;
            }
            let session = "control-hook-source-diagnostics";
            assert!(
                fixture
                    .run(&["new-session", "-d", "-s", session, "exec /bin/cat"])
                    .status
                    .success()
            );
            let directory = fixture
                ._directory
                .path()
                .join("control hook source diagnostics");
            std::fs::create_dir(&directory).expect("create hook source diagnostic directory");
            let hit = write_source(&directory, "hit.conf", "display-message -p MIXED_HIT\n");
            let missing = directory.join("missing.conf");
            assert!(
                fixture
                    .run(&[
                        "set-hook",
                        "-g",
                        "after-display-message",
                        &format!("source-file '{}' '{hit}'", missing.display()),
                    ])
                    .status
                    .success()
            );

            let mixed_marker = directory.join("mixed-complete");
            let mixed = run_control_until_return(
                &fixture,
                &["-C", "attach-session", "-t", &format!("={session}")],
                "display-message -p MIXED_TRIGGER\n\
                 set-hook -gu after-display-message\n\
                 display-message -p MIXED_LATER\n",
                &mixed_marker,
                "control mixed hook source",
            );
            assert_eq!(mixed.status.code(), Some(1));
            assert!(mixed.stderr.is_empty());
            let mixed = parse_stream_allow_gaps(&mixed.stdout, false);
            assert_eq!(mixed.blocks.len(), 7, "{mixed:?}");
            assert_block(&mixed.blocks[0], 1, 0, &[], false);
            assert_block(&mixed.blocks[1], 2, 1, &["MIXED_TRIGGER"], false);
            assert_block(
                &mixed.blocks[2],
                3,
                0,
                &[&format!("No such file or directory: {}", missing.display())],
                false,
            );
            assert_block(&mixed.blocks[3], 4, 0, &["MIXED_HIT"], false);
            assert_block(&mixed.blocks[4], 6, 1, &[], false);
            assert_block(&mixed.blocks[5], 7, 1, &["MIXED_LATER"], false);
            assert_block(&mixed.blocks[6], 8, 1, &[], false);

            assert!(
                fixture
                    .run(&[
                        "set-hook",
                        "-g",
                        "after-display-message",
                        &format!("source-file '{}'", directory.display()),
                    ])
                    .status
                    .success()
            );
            let read_error = std::fs::read_to_string(&directory)
                .expect_err("source directory must fail as a file read");
            let read_error = format!("{}: {}", os_error_text(&read_error), directory.display());
            let read_marker = directory.join("read-complete");
            let read = run_control_until_return(
                &fixture,
                &["-C", "attach-session", "-t", &format!("={session}")],
                "display-message -p READ_TRIGGER\n\
                 set-hook -gu after-display-message\n\
                 display-message -p READ_LATER\n",
                &read_marker,
                "control hook source read error",
            );
            assert_eq!(read.status.code(), Some(1));
            assert!(read.stderr.is_empty());
            let read_lines = std::str::from_utf8(&read.stdout)
                .expect("UTF-8 hook source read output")
                .lines()
                .collect::<Vec<_>>();
            let read_error_index = read_lines
                .iter()
                .position(|line| line == &read_error)
                .expect("raw hook source read error");
            let before = next_block_guard(read_lines[..read_error_index].iter().rev().copied());
            assert!(
                before.starts_with("%end ") && before.ends_with(" 3 0"),
                "{read_lines:?}"
            );
            let after = next_block_guard(read_lines[read_error_index + 1..].iter().copied());
            assert!(
                after.starts_with("%begin ") && after.ends_with(" 5 1"),
                "{read_lines:?}"
            );
            let read = parse_stream_allow_gaps(&read.stdout, false);
            assert_eq!(read.blocks.len(), 6, "{read:?}");
            assert_block(&read.blocks[0], 1, 0, &[], false);
            assert_block(&read.blocks[1], 2, 1, &["READ_TRIGGER"], false);
            assert_block(&read.blocks[2], 3, 0, &[], false);
            assert_block(&read.blocks[3], 5, 1, &[], false);
            assert_block(&read.blocks[4], 6, 1, &["READ_LATER"], false);
            assert_block(&read.blocks[5], 7, 1, &[], false);
            assert!(read.outside.contains(&read_error));
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
            assert_eq!(
                stream
                    .outside
                    .iter()
                    .filter(|line| line.as_str() == "%exit")
                    .count(),
                1
            );
            assert!(
                !stream
                    .outside
                    .iter()
                    .any(|line| line.contains("server exited unexpectedly"))
            );
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
                .filter(|line| {
                    !line.starts_with("%output ")
                        && !line.starts_with("%window-renamed @")
                        && line.as_str() != "%exit"
                })
                .collect::<Vec<_>>();
            let position = |predicate: &dyn Fn(&str) -> bool| {
                notifications
                    .iter()
                    .position(|line| predicate(line))
                    .unwrap_or_else(|| panic!("missing startup notification: {notifications:?}"))
            };
            let added = position(&|line| line.starts_with("%window-add @"));
            let listed = position(&|line| line == "%sessions-changed");
            let switched = position(&|line| {
                line.starts_with("%session-changed $") && line.ends_with(" watched")
            });
            assert!(added < listed && listed < switched, "{notifications:?}");
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
                stream
                    .outside
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
            let target = wait_for_control_clients(&fixture, 1, "refresh-client sizing")
                .into_iter()
                .next()
                .expect("control client list row");
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
            let expected_width = format!("{target}:100");
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let width = fixture.run(&["list-clients", "-F", "#{client_name}:#{client_width}"]);
                if String::from_utf8_lossy(&width.stdout)
                    .lines()
                    .any(|line| line == expected_width)
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
            let deadline = Instant::now() + Duration::from_secs(30);
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
