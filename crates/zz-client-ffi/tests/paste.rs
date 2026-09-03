//! What `zz_client_paste` actually delivers to a pty, byte for byte: newlines
//! arrive as carriage returns, bracketed-paste markers appear only when the
//! program asked for them, and nothing is reduced through the key tables.
//! Each fixture reads a fixed byte count in raw mode and prints it as hex, so
//! the assertions read real pty bytes rather than a rendered screen.

#![cfg(unix)]

use std::{
    ffi::CString,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use zz_client_ffi::{
    ZzClient, ZzEvent, ZzEventKind, zz_client_attach, zz_client_connect, zz_client_free,
    zz_client_next_event, zz_client_paste, zz_client_send_text, zz_client_terminal_panes,
};
use zz_daemon::{CommandClient, Daemon};
use zz_protocol::CommandInvocation;

const READY: &str = "ZZ_PASTE_READY";
const BYTES: &str = "ZZ_PASTE_BYTES=";
const PASTED: &str = "ab\ncd";
const SETTLE: Duration = Duration::from_millis(20);
const PATIENCE: Duration = Duration::from_secs(30);

/// A pane that reports the next `expected` bytes it reads as hex. Raw mode
/// keeps the line discipline from rewriting them and `-echo` keeps the report
/// free of the input itself.
fn byte_probe(bracketed: bool, expected: usize) -> String {
    let mode = if bracketed {
        "printf '\\033[?2004h'; "
    } else {
        ""
    };
    format!(
        "stty raw -echo; {mode}printf '{READY}\\r\\n'; \
         bytes=$(dd bs=1 count={expected} 2>/dev/null | od -An -tx1 | tr -d ' \\n'); \
         printf '{BYTES}%s\\r\\n' \"$bytes\"; exec /bin/cat"
    )
}

struct Fixture {
    scratch: PathBuf,
    commands: CommandClient,
    panes: Vec<u64>,
    client: *mut ZzClient,
}

impl Fixture {
    fn boot(name: &str, probes: &[String]) -> Self {
        let scratch = std::env::temp_dir().join(format!("zz-paste-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("create scratch directory");
        let socket = scratch.join("paste.sock");
        let _ = std::fs::remove_file(&socket);
        let daemon = Daemon::new(&socket).without_user_config();
        thread::Builder::new()
            .name("zz-paste-daemon".to_owned())
            .spawn(move || {
                let _ = daemon.run_foreground();
            })
            .expect("spawn the paste daemon");
        let mut fixture = Self {
            scratch,
            commands: connect_commands(&socket),
            panes: Vec::new(),
            client: std::ptr::null_mut(),
        };
        for (index, probe) in probes.iter().enumerate() {
            let printed = if index == 0 {
                fixture.run(
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
                        "paste",
                        probe,
                    ],
                )
            } else {
                fixture.run(
                    "new-window",
                    &["-d", "-P", "-F", "#{pane_id}", "-t", "paste", probe],
                )
            };
            let pane = printed
                .trim()
                .strip_prefix('%')
                .and_then(|id| id.parse().ok())
                .unwrap_or_else(|| panic!("the probe pane reported no id: {printed}"));
            fixture.panes.push(pane);
        }
        fixture.client = fixture.attach(&socket);
        fixture
    }

    fn run(&mut self, name: &str, args: &[&str]) -> String {
        self.commands
            .execute(CommandInvocation::new(name, args.iter().copied()))
            .unwrap_or_else(|error| panic!("{name} failed: {error}"))
    }

    /// Attach through the ABI and wait until the core reports every probe
    /// pane; the daemon refuses a paste into a pane the client is not
    /// attached to.
    fn attach(&self, socket: &Path) -> *mut ZzClient {
        let path = CString::new(socket.to_str().expect("socket path")).expect("socket string");
        let client = unsafe { zz_client_connect(path.as_ptr()) };
        assert!(!client.is_null(), "the ABI client connected");
        let session = CString::new("paste").expect("session string");
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
            if self.panes.iter().all(|pane| ids[..count].contains(pane)) {
                return client;
            }
            assert!(
                Instant::now() < deadline,
                "the ABI client never saw the probe panes {:?}, only {:?}",
                self.panes,
                &ids[..count]
            );
            thread::sleep(SETTLE);
        }
    }

    fn wait_for(&mut self, pane: u64, needle: &str) -> String {
        let target = format!("%{pane}");
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

    fn pty_hex(&mut self, pane: u64) -> String {
        let line = self.wait_for(pane, BYTES);
        line.split(BYTES)
            .nth(1)
            .expect("the hex report")
            .trim()
            .to_owned()
    }

    fn paste(&mut self, pane: u64, text: &str) {
        let text = CString::new(text).expect("paste string");
        assert!(
            unsafe { zz_client_paste(self.client, pane, text.as_ptr()) },
            "the paste was sent"
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
fn paste_translates_newlines_and_brackets_only_when_the_program_asked() {
    let mut fixture = Fixture::boot("modes", &[byte_probe(false, 5), byte_probe(true, 17)]);
    let (plain, bracketed) = (fixture.panes[0], fixture.panes[1]);
    fixture.wait_for(plain, READY);
    fixture.wait_for(bracketed, READY);

    fixture.paste(plain, PASTED);
    fixture.paste(bracketed, PASTED);

    assert_eq!(
        fixture.pty_hex(plain),
        "61620d6364",
        "a program without DECSET 2004 must see a carriage return, never the \
         line feed a shell would run as a second command"
    );
    assert_eq!(
        fixture.pty_hex(bracketed),
        "1b5b3230307e61620a63641b5b3230317e",
        "a program that enabled DECSET 2004 must see the paste wrapped, with \
         its newline left alone inside the brackets"
    );
}

#[test]
fn pasted_bytes_skip_the_key_tables_that_swallow_typed_text() {
    let mut fixture = Fixture::boot("prefix", &[byte_probe(false, 1)]);
    fixture.run("set-option", &["-g", "prefix", "q"]);
    let pane = fixture.panes[0];
    fixture.wait_for(pane, READY);

    let typed = CString::new("q").expect("typed string");
    assert!(
        unsafe { zz_client_send_text(fixture.client, pane, typed.as_ptr()) },
        "the typed prefix byte was sent"
    );
    fixture.paste(pane, "z");

    assert_eq!(
        fixture.pty_hex(pane),
        "7a",
        "the typed prefix byte armed the prefix and never reached the pty, \
         while the pasted byte went straight through"
    );
}
