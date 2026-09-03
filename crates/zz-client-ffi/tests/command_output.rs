//! A printing command opens a client-local output view on the daemon, which
//! parks the client on the pane's copy-mode key table and swallows its
//! terminal input until the view is gone. A shell that never renders the view
//! has to close it itself, and Escape cannot: `copy-mode-vi` binds it to
//! `clear-selection`, which keeps the view open. The pane here reports the
//! first byte it reads as hex, so the assertion reads a real pty byte rather
//! than a rendered screen.

#![cfg(unix)]

use std::{
    ffi::CString,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use zz_client_ffi::{
    ZzClient, ZzEvent, ZzEventKind, zz_client_attach, zz_client_cancel_command_output,
    zz_client_connect, zz_client_execute, zz_client_free, zz_client_next_event, zz_client_send_key,
    zz_client_send_text, zz_client_terminal_panes,
};
use zz_daemon::{CommandClient, Daemon};
use zz_protocol::CommandInvocation;

const READY: &str = "ZZ_OUTPUT_READY";
const BYTES: &str = "ZZ_OUTPUT_BYTES=";
const PRINTED: &str = "printed-output";
const COPY_TABLE: &str = "copy-mode-vi";
const ESCAPE: u32 = 4;
const SETTLE: Duration = Duration::from_millis(20);
const PATIENCE: Duration = Duration::from_secs(30);

/// A pane that reports the first byte it reads as hex. Raw mode keeps the line
/// discipline from rewriting it and `-echo` keeps the report free of the input.
fn byte_probe() -> String {
    format!(
        "stty raw -echo; printf '{READY}\\r\\n'; \
         byte=$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \\n'); \
         printf '{BYTES}%s\\r\\n' \"$byte\"; exec /bin/cat"
    )
}

struct Fixture {
    scratch: PathBuf,
    commands: CommandClient,
    pane: u64,
    client: *mut ZzClient,
}

impl Fixture {
    fn boot() -> Self {
        let scratch = std::env::temp_dir().join(format!("zz-output-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("create scratch directory");
        let socket = scratch.join("output.sock");
        let _ = std::fs::remove_file(&socket);
        let daemon = Daemon::new(&socket).without_user_config();
        thread::Builder::new()
            .name("zz-output-cancel-daemon".to_owned())
            .spawn(move || {
                let _ = daemon.run_foreground();
            })
            .expect("spawn the output-cancel daemon");
        let mut fixture = Self {
            scratch,
            commands: connect_commands(&socket),
            pane: 0,
            client: std::ptr::null_mut(),
        };
        let printed = fixture.run(
            "new-session",
            &[
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
                "-x",
                "200",
                "-y",
                "24",
                "-s",
                "output",
                &byte_probe(),
            ],
        );
        fixture.pane = printed
            .trim()
            .strip_prefix('%')
            .and_then(|id| id.parse().ok())
            .unwrap_or_else(|| panic!("the probe pane reported no id: {printed}"));
        fixture.client = fixture.attach(&socket);
        fixture
    }

    fn run(&mut self, name: &str, args: &[&str]) -> String {
        self.commands
            .execute(CommandInvocation::new(name, args.iter().copied()))
            .unwrap_or_else(|error| panic!("{name} failed: {error}"))
    }

    /// Attach through the ABI and wait until the core reports the probe pane;
    /// the daemon refuses input for a pane the client is not attached to.
    fn attach(&self, socket: &Path) -> *mut ZzClient {
        let path = CString::new(socket.to_str().expect("socket path")).expect("socket string");
        let client = unsafe { zz_client_connect(path.as_ptr()) };
        assert!(!client.is_null(), "the ABI client connected");
        let session = CString::new("output").expect("session string");
        assert!(
            unsafe { zz_client_attach(client, session.as_ptr()) },
            "the attach request was sent"
        );
        let deadline = Instant::now() + PATIENCE;
        loop {
            let mut event = ZzEvent {
                kind: ZzEventKind::Other,
                flags: 0,
                pane: 0,
                row_start: 0,
                row_end: 0,
            };
            while unsafe { zz_client_next_event(client, &raw mut event) } {}
            let mut ids = [0_u64; 8];
            let count = unsafe { zz_client_terminal_panes(client, ids.as_mut_ptr(), ids.len()) };
            if ids[..count].contains(&self.pane) {
                return client;
            }
            assert!(
                Instant::now() < deadline,
                "the ABI client never saw the probe pane {}, only {:?}",
                self.pane,
                &ids[..count]
            );
            thread::sleep(SETTLE);
        }
    }

    fn wait_for(&mut self, needle: &str) -> String {
        let target = format!("%{}", self.pane);
        let deadline = Instant::now() + PATIENCE;
        loop {
            let captured = self.run("capture-pane", &["-p", "-t", &target]);
            if let Some(line) = captured.lines().find(|line| line.contains(needle)) {
                return line.to_owned();
            }
            assert!(
                Instant::now() < deadline,
                "{needle} never appeared in {target}:\n{captured}"
            );
            thread::sleep(SETTLE);
        }
    }

    fn pty_hex(&mut self) -> String {
        let line = self.wait_for(BYTES);
        line.split(BYTES)
            .nth(1)
            .expect("the hex report")
            .trim()
            .to_owned()
    }

    fn key_table(&mut self) -> String {
        self.run("list-clients", &["-F", "#{client_key_table}"])
            .trim()
            .to_owned()
    }

    fn wait_for_key_table(&mut self, expected: &str) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let table = self.key_table();
            if table == expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the client key table stayed {table} instead of {expected}"
            );
            thread::sleep(SETTLE);
        }
    }

    fn type_text(&mut self, text: &str) {
        let text = CString::new(text).expect("typed string");
        assert!(
            unsafe { zz_client_send_text(self.client, self.pane, text.as_ptr()) },
            "the typed text was sent"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.client.is_null() {
            unsafe { zz_client_free(self.client) };
        }
        let _ = self
            .commands
            .execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn connect_commands(socket: &Path) -> CommandClient {
    let deadline = Instant::now() + PATIENCE;
    loop {
        match CommandClient::connect(socket) {
            Ok(client) => return client,
            Err(error) => {
                assert!(Instant::now() < deadline, "daemon did not start: {error}");
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[test]
fn cancelling_the_output_view_gives_a_vi_pane_its_input_back() {
    let mut fixture = Fixture::boot();
    fixture.run("set-window-option", &["-g", "mode-keys", "vi"]);
    fixture.wait_for(READY);
    assert_eq!(fixture.key_table(), "root");

    let message = CString::new("display-message").expect("command string");
    let arguments =
        ["-p", "-l", PRINTED].map(|argument| CString::new(argument).expect("command argument"));
    let printed = arguments.each_ref().map(|argument| argument.as_ptr());
    assert!(
        unsafe {
            zz_client_execute(
                fixture.client,
                message.as_ptr(),
                printed.as_ptr(),
                printed.len(),
            )
        },
        "the printing command was sent"
    );
    fixture.wait_for_key_table(COPY_TABLE);

    fixture.type_text("a");
    assert!(
        unsafe {
            zz_client_send_key(
                fixture.client,
                fixture.pane,
                ESCAPE,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                false,
            )
        },
        "the escape workaround was sent"
    );
    fixture.type_text("b");

    assert!(
        unsafe { zz_client_cancel_command_output(fixture.client) },
        "the cancel was sent"
    );
    fixture.wait_for_key_table("root");
    fixture.type_text("c");

    assert_eq!(
        fixture.pty_hex(),
        "63",
        "everything typed while {PRINTED} held the output view open was \
         swallowed, escape included, and only the byte typed after the cancel \
         reached the pty"
    );
}
