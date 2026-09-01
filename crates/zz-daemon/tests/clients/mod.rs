//! A headless daemon with real attached clients, for the format loops that
//! only have rows once somebody is attached: `#{L:}` over the client roster and
//! `#{Vc:}` over the selected client's environment.

#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use zz_daemon::{CommandClient, Daemon, DaemonError, InteractiveClient};
use zz_protocol::{CommandInvocation, PaneId};
use zz_terminal::TerminalColorScheme;

const SETTLE: Duration = Duration::from_millis(20);

pub struct Clients {
    socket: PathBuf,
    daemon: Option<JoinHandle<Result<(), DaemonError>>>,
    pub commands: CommandClient,
    attached: Vec<InteractiveClient>,
}

impl Clients {
    /// Start a daemon carrying one detached session per name.
    pub fn start(name: &str, sessions: &[&str]) -> Self {
        let socket = PathBuf::from(format!("/tmp/zz-{name}-{}.sock", std::process::id()));
        let _ = fs::remove_file(&socket);
        let daemon = Daemon::new(&socket).without_user_config();
        let handle = thread::spawn(move || daemon.run_foreground());
        let mut commands = connect_command_retry(&socket);
        for session in sessions {
            commands
                .execute(CommandInvocation::new(
                    "new-session",
                    ["-d", "-s", session, "cat"],
                ))
                .expect("create a session");
        }
        Self {
            socket,
            daemon: Some(handle),
            commands,
            attached: Vec::new(),
        }
    }

    /// Attach an interactive client and wait for the roster to grow.
    pub fn attach_interactive(&mut self, session: &str) -> usize {
        let client = InteractiveClient::connect_with_color_scheme_and_terminal(
            &self.socket,
            TerminalColorScheme::Dark,
            true,
        )
        .expect("connect an interactive client");
        client.attach(session).expect("attach interactive");
        self.push_attached(client)
    }

    /// Attach a control-mode client and wait for the roster to grow.
    pub fn attach_control(&mut self, session: &str) -> usize {
        let client = InteractiveClient::connect_control(&self.socket).expect("connect control");
        client.attach(session).expect("attach control");
        self.push_attached(client)
    }

    fn push_attached(&mut self, client: InteractiveClient) -> usize {
        let expected = self.attached.len() + 1;
        self.attached.push(client);
        self.await_client_count(expected);
        expected - 1
    }

    /// Disconnect one attached client and wait for the roster to shrink.
    pub fn detach(&mut self, index: usize) {
        let client = self.attached.remove(index);
        let _ = client.detach();
        drop(client);
        self.await_client_count(self.attached.len());
    }

    /// Terminal input from an attached client, which is what moves the client's
    /// activity ahead of every other client's.
    pub fn note_activity(&mut self, index: usize, session: &str) {
        let pane = self.pane(session);
        self.attached[index]
            .send_input(zz_protocol::InputMessage::Text {
                pane,
                text: "x".to_owned(),
            })
            .expect("send input");
        thread::sleep(SETTLE);
    }

    pub fn pane(&mut self, session: &str) -> PaneId {
        self.commands
            .execute(CommandInvocation::new(
                "list-panes",
                ["-t", session, "-F", "#{pane_id}"],
            ))
            .expect("list panes")
            .lines()
            .find_map(|line| line.trim().parse::<PaneId>().ok())
            .expect("the session's pane")
    }

    /// `display-message -p`, the shortest path from a format to its expansion.
    pub fn format(&mut self, target: &str, format: &str) -> String {
        self.commands
            .execute(CommandInvocation::new(
                "display-message",
                ["-p", "-t", target, format],
            ))
            .expect("expand the format")
    }

    /// One line per attached client, in the roster's own order.
    pub fn listed(&mut self, format: &str) -> Vec<String> {
        self.commands
            .execute(CommandInvocation::new("list-clients", ["-F", format]))
            .expect("list clients")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub fn client_names(&mut self) -> Vec<String> {
        self.listed("#{client_name}")
    }

    fn await_client_count(&mut self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.client_names().len() == expected {
                thread::sleep(SETTLE);
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the daemon never reported {expected} attached clients"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Clients {
    fn drop(&mut self) {
        self.attached.clear();
        let _ = self
            .commands
            .execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        if let Some(daemon) = self.daemon.take() {
            let _ = daemon.join();
        }
        let _ = fs::remove_file(&self.socket);
    }
}

fn connect_command_retry(socket: &Path) -> CommandClient {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match CommandClient::connect(socket) {
            Ok(client) => return client,
            Err(error) if Instant::now() >= deadline => panic!("daemon did not start: {error}"),
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}
