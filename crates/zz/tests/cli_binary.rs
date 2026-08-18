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
mod daemon_autostart {
    use std::{
        ffi::OsString,
        io::Write as _,
        path::{Path, PathBuf},
        process::{Child, ChildStdin, Command, Output, Stdio},
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
            assert_eq!(
                stream.outside,
                ["%unlinked-window-add @0", "%sessions-changed", "%exit"]
            );
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
            assert_eq!(
                stream.outside,
                ["%unlinked-window-add @0", "%sessions-changed", "%exit"]
            );
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

        fn checked_layout(layout: &str) -> bool {
            layout.split_once(',').is_some_and(|(checksum, body)| {
                checksum.len() == 4
                    && checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && !body.is_empty()
            })
        }
    }
}
