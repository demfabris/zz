//! A headless daemon with one attached interactive client, for the overlay
//! surfaces that only exist while a client is attached: `display-menu` and
//! `display-popup`.
//!
//! Overlay commands from a command client block until the overlay closes, so
//! every invocation runs on its own connection and its own thread while the
//! attached client reads the published descriptor.

#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use zz_daemon::{CommandClient, Daemon, DaemonError, InteractiveClient};
use zz_protocol::{
    CommandInvocation, Event, EventPayload, InputMessage, MenuState, PaneId, PopupState,
    ProtocolMessage,
};
use zz_terminal::TerminalColorScheme;

pub const SESSION: &str = "overlay";
pub const COLUMNS: u16 = 80;
pub const ROWS: u16 = 24;

pub struct Overlays {
    socket: PathBuf,
    daemon: Option<JoinHandle<Result<(), DaemonError>>>,
    pub commands: CommandClient,
    pub client: InteractiveClient,
    pub client_name: String,
    pub pane: PaneId,
}

impl Overlays {
    pub fn start(name: &str) -> Self {
        let socket = PathBuf::from(format!("/tmp/zz-{name}-{}.sock", std::process::id()));
        let _ = fs::remove_file(&socket);
        let daemon = Daemon::new(&socket).without_user_config();
        let handle = thread::spawn(move || daemon.run_foreground());
        let mut commands = connect_command_retry(&socket);
        commands
            .execute(CommandInvocation::new(
                "new-session",
                ["-d", "-s", SESSION, "cat"],
            ))
            .expect("create the overlay session");
        let pane = commands
            .execute(CommandInvocation::new(
                "list-panes",
                ["-t", SESSION, "-F", "#{pane_id}"],
            ))
            .expect("list panes")
            .lines()
            .find_map(|line| line.trim().parse::<PaneId>().ok())
            .expect("the session's pane");

        let client = InteractiveClient::connect_with_color_scheme_and_terminal(
            &socket,
            TerminalColorScheme::Dark,
            true,
        )
        .expect("attach an interactive client");
        client.attach(SESSION).expect("attach to the session");
        let mut overlays = Self {
            socket,
            daemon: Some(handle),
            commands,
            client,
            client_name: String::new(),
            pane,
        };
        overlays.resize(COLUMNS, ROWS);
        overlays.client_name = overlays.await_client_name();
        overlays
    }

    pub fn resize(&self, columns: u16, rows: u16) {
        self.client
            .send_input(InputMessage::ResizeTerminal {
                pane: self.pane,
                columns,
                rows,
                cell_width_px: 8,
                cell_height_px: 16,
            })
            .expect("resize the attached client");
    }

    fn await_client_name(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let listed = self
                .commands
                .execute(CommandInvocation::new(
                    "list-clients",
                    ["-F", "#{client_name}"],
                ))
                .expect("list clients");
            if let Some(name) = listed.lines().map(str::trim).find(|name| !name.is_empty()) {
                return name.to_owned();
            }
            assert!(Instant::now() < deadline, "no client attached in time");
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Run one command on its own connection, so an overlay command that waits
    /// for the overlay to close does not block the test thread.
    pub fn spawn_command(&self, command: CommandInvocation) -> JoinHandle<Result<String, String>> {
        let socket = self.socket.clone();
        thread::spawn(move || {
            let mut client = connect_command_retry(&socket);
            client.execute(command).map_err(|error| error.to_string())
        })
    }

    /// Read published events until a menu descriptor arrives.
    pub fn await_menu(&self) -> MenuState {
        self.await_event(|payload| match payload {
            EventPayload::Menu { state } => state,
            _ => None,
        })
    }

    pub fn await_menu_closed(&self) {
        self.await_event(|payload| match payload {
            EventPayload::Menu { state: None } => Some(()),
            _ => None,
        });
    }

    pub fn await_popup(&self) -> PopupState {
        self.await_event(|payload| match payload {
            EventPayload::Popup { state } => state,
            _ => None,
        })
    }

    pub fn await_popup_matching(&self, accept: impl Fn(&PopupState) -> bool) -> PopupState {
        self.await_event(|payload| match payload {
            EventPayload::Popup { state: Some(state) } if accept(&state) => Some(state),
            _ => None,
        })
    }

    pub fn await_menu_matching(&self, accept: impl Fn(&MenuState) -> bool) -> MenuState {
        self.await_event(|payload| match payload {
            EventPayload::Menu { state: Some(state) } if accept(&state) => Some(state),
            _ => None,
        })
    }

    fn await_event<T>(&self, mut accept: impl FnMut(EventPayload) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_mins(1);
        loop {
            assert!(Instant::now() < deadline, "no matching event arrived");
            let message = self.client.recv().expect("read a daemon message");
            if let ProtocolMessage::Event(Event { payload, .. }) = message
                && let Some(value) = accept(payload)
            {
                return value;
            }
        }
    }
}

impl Drop for Overlays {
    fn drop(&mut self) {
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
