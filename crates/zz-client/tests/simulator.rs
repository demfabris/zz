//! Randomized convergence harness: a real in-process daemon, two `ClientCore`
//! reducers, seed-driven traffic, and content oracles.
//!
//! Every failure prints its seed; rerun with the same seed for the same client
//! action sequence. PTY output timing is real, so the oracles are quiescence
//! properties (retained state equals freshly-requested state) rather than
//! byte-identical transcripts.

#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use zz_client::{ClientCore, Outbound};
use zz_daemon::{CommandClient, Daemon, InteractiveClient};
use zz_protocol::{
    CommandInvocation, Event, EventPayload, InputMessage, MuxSnapshot, PaneId, PaneKindSnapshot,
    ProtocolMessage,
};
use zz_terminal::{GRAPHEME_TABLE_BIT, PackedStyle, TerminalViewport};

const COLUMNS: u16 = 80;
const ROWS: u16 = 24;
const CELL_WIDTH_PX: u32 = 8;
const CELL_HEIGHT_PX: u32 = 16;
const STEPS: usize = 40;
const MAX_PANES: usize = 4;
const QUIESCENT_WINDOW: Duration = Duration::from_millis(400);
const QUIESCENCE_DEADLINE: Duration = Duration::from_secs(20);
const FULL_FRAME_DEADLINE: Duration = Duration::from_secs(10);

struct Rng(u64);

impl Rng {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }
}

struct SimClient {
    client: Arc<InteractiveClient>,
    core: ClientCore,
    inbox: mpsc::Receiver<ProtocolMessage>,
}

impl SimClient {
    fn connect(socket: &Path) -> Self {
        let deadline = Instant::now() + Duration::from_secs(30);
        let client = loop {
            match InteractiveClient::connect(socket) {
                Ok(client) => break Arc::new(client),
                Err(error) => {
                    assert!(Instant::now() < deadline, "daemon did not start: {error}");
                    thread::sleep(Duration::from_millis(10));
                }
            }
        };
        let (sender, inbox) = mpsc::channel();
        let reader = Arc::clone(&client);
        thread::Builder::new()
            .name("zz-sim-reader".to_owned())
            .spawn(move || {
                while let Ok(message) = reader.recv() {
                    if sender.send(message).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn simulator reader");
        let mut core = ClientCore::new();
        core.handle_message(ProtocolMessage::ServerHello(client.server_hello().clone()));
        Self {
            client,
            core,
            inbox,
        }
    }

    /// Feed every already-received message into the core; true if anything
    /// beyond the daemon's periodic status clock arrived.
    fn pump(&mut self) -> bool {
        let mut received = false;
        while let Ok(message) = self.inbox.try_recv() {
            let significant = !matches!(
                &message,
                ProtocolMessage::Event(Event {
                    payload: EventPayload::StatusChanged { .. },
                    ..
                })
            );
            if significant && std::env::var_os("ZZ_SIM_TRACE").is_some() {
                match &message {
                    ProtocolMessage::Event(event) => {
                        let name = format!("{:?}", event.payload);
                        let name = name.split([' ', '(', '{']).next().unwrap_or("?");
                        eprintln!("sim: event {name}");
                    }
                    other => {
                        let name = format!("{other:?}");
                        let name = name.split([' ', '(', '{']).next().unwrap_or("?");
                        eprintln!("sim: message {name}");
                    }
                }
            }
            received |= significant;
            self.core.handle_message(message);
            while let Some(outbound) = self.core.poll_outbound() {
                match outbound {
                    Outbound::RequestFull(pane) => {
                        self.client.request_full(pane).expect("request full");
                    }
                }
            }
        }
        while self.core.poll_event().is_some() {}
        received
    }

    fn resize_all_terminals(&self) {
        for pane in terminal_panes(self.core.snapshot()) {
            self.client
                .send_input(InputMessage::ResizeTerminal {
                    pane,
                    columns: COLUMNS,
                    rows: ROWS,
                    cell_width_px: CELL_WIDTH_PX,
                    cell_height_px: CELL_HEIGHT_PX,
                })
                .expect("resize terminal");
        }
    }
}

fn terminal_panes(snapshot: &MuxSnapshot) -> Vec<PaneId> {
    let mut panes: Vec<PaneId> = snapshot
        .sessions
        .iter()
        .flat_map(|session| &session.windows)
        .flat_map(|window| &window.panes)
        .filter(|(_, pane)| matches!(pane.kind, PaneKindSnapshot::Terminal))
        .map(|(pane, _)| *pane)
        .collect();
    panes.sort_unstable();
    panes
}

/// Per-client stamps (focused window, viewer identity, generation) stripped.
fn normalized_snapshot(
    snapshot: &MuxSnapshot,
) -> Vec<(String, Vec<(String, String, Vec<String>)>)> {
    snapshot
        .sessions
        .iter()
        .map(|session| {
            (
                format!("{} {} @{}", session.id, session.name, session.active_window),
                session
                    .windows
                    .iter()
                    .map(|window| {
                        let mut panes: Vec<String> = window
                            .panes
                            .iter()
                            .map(|(pane, snapshot)| format!("{pane} {:?}", snapshot.kind))
                            .collect();
                        panes.sort();
                        (
                            format!("{} {}", window.id, window.name),
                            format!("{:?}", window.layout),
                            panes,
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

/// Rendering-equivalent content: resolved glyph text and style values per
/// cell, plus geometry and cursor. Style ids and dictionary layout renumber
/// between a patched viewport and a fresh full frame, so equality must
/// compare resolved values, never raw ids.
fn content_signature(
    viewport: &TerminalViewport,
) -> (u16, u16, Vec<(String, Option<PackedStyle>)>, String) {
    let cells = viewport
        .cells
        .iter()
        .map(|cell| {
            let glyph = cell.glyph();
            let text = if glyph == 0 {
                String::new()
            } else if glyph & GRAPHEME_TABLE_BIT == 0 {
                char::from_u32(glyph).map_or_else(String::new, String::from)
            } else {
                let index = (glyph & !GRAPHEME_TABLE_BIT) as usize;
                let offsets = &viewport.dictionary.grapheme_offsets;
                match (offsets.get(index), offsets.get(index + 1)) {
                    (Some(&start), Some(&end)) => std::str::from_utf8(
                        &viewport.dictionary.grapheme_bytes[start as usize..end as usize],
                    )
                    .unwrap_or("?")
                    .to_owned(),
                    _ => "?".to_owned(),
                }
            };
            let style = viewport
                .dictionary
                .styles
                .get(usize::from(cell.style_id()))
                .copied();
            (text, style)
        })
        .collect();
    (
        viewport.columns,
        viewport.rows,
        cells,
        format!("{:?}", viewport.cursor),
    )
}

struct Simulation {
    socket: PathBuf,
    clients: Vec<SimClient>,
}

impl Simulation {
    fn boot(seed: u64) -> Self {
        let socket =
            std::env::temp_dir().join(format!("zz-sim-{}-{seed}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let daemon = Daemon::new(&socket).without_user_config();
        thread::Builder::new()
            .name("zz-sim-daemon".to_owned())
            .spawn(move || {
                let _ = daemon.run_foreground();
            })
            .expect("spawn simulator daemon");

        let mut commands = connect_commands(&socket);
        commands
            .execute(CommandInvocation::new(
                "new-session",
                ["-d", "-s", "sim", fixture_command()],
            ))
            .expect("create the simulated session");

        let mut clients = vec![SimClient::connect(&socket), SimClient::connect(&socket)];
        for client in &mut clients {
            client.client.attach("sim").expect("attach");
        }
        let mut simulation = Self { socket, clients };
        simulation.quiesce();
        for client in &simulation.clients {
            client.resize_all_terminals();
        }
        simulation.quiesce();
        simulation
    }

    fn pump_all(&mut self) -> bool {
        let mut received = false;
        for client in &mut self.clients {
            received |= client.pump();
        }
        received
    }

    /// Pump until no client received anything for [`QUIESCENT_WINDOW`].
    fn quiesce(&mut self) {
        let deadline = Instant::now() + QUIESCENCE_DEADLINE;
        let mut quiet_since = Instant::now();
        loop {
            if self.pump_all() {
                quiet_since = Instant::now();
            } else if quiet_since.elapsed() >= QUIESCENT_WINDOW {
                return;
            }
            assert!(Instant::now() < deadline, "the simulation never went quiet");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn run_traffic(&mut self, rng: &mut Rng) {
        for _ in 0..STEPS {
            let driver = &self.clients[usize::try_from(rng.next(2)).unwrap_or(0) % 2].client;
            let panes = terminal_panes(self.clients[0].core.snapshot());
            match rng.next(100) {
                0..=49 => {
                    if let Some(pane) = pick(rng, &panes) {
                        let word = format!("w{}", rng.next(10_000));
                        let _ = driver.execute(CommandInvocation::new(
                            "send-keys",
                            ["-t", &pane.to_string(), "-l", &word],
                        ));
                        let _ = driver.execute(CommandInvocation::new(
                            "send-keys",
                            ["-t", &pane.to_string(), "Enter"],
                        ));
                    }
                }
                50..=64 => {
                    if panes.len() < MAX_PANES {
                        let _ = driver.execute(CommandInvocation::new(
                            "split-window",
                            ["-t", "sim", fixture_command()],
                        ));
                    }
                }
                65..=74 => {
                    if panes.len() > 1
                        && let Some(pane) = pick(rng, &panes)
                    {
                        let _ = driver.execute(CommandInvocation::new(
                            "kill-pane",
                            ["-t", &pane.to_string()],
                        ));
                    }
                }
                75..=89 => {
                    let _ = driver.execute(CommandInvocation::new("select-pane", ["-t", ":.+"]));
                }
                _ => {
                    for client in &self.clients {
                        client.resize_all_terminals();
                    }
                }
            }
            self.pump_all();
            thread::sleep(Duration::from_millis(2));
        }
        self.quiesce();
        for client in &self.clients {
            client.resize_all_terminals();
        }
        self.quiesce();
    }

    /// Every client's normalized snapshot matches every other's.
    fn assert_snapshots_converged(&self) {
        let reference = normalized_snapshot(self.clients[0].core.snapshot());
        for (index, client) in self.clients.iter().enumerate().skip(1) {
            assert_eq!(
                reference,
                normalized_snapshot(client.core.snapshot()),
                "client {index} snapshot diverged"
            );
        }
    }

    /// Every retained pane viewport matches across clients.
    fn assert_viewports_converged(&self) {
        let panes = terminal_panes(self.clients[0].core.snapshot());
        for pane in panes {
            let Some(reference) = self.clients[0].core.viewport(pane) else {
                continue;
            };
            for (index, client) in self.clients.iter().enumerate().skip(1) {
                let Some(viewport) = client.core.viewport(pane) else {
                    continue;
                };
                assert_eq!(
                    content_signature(reference),
                    content_signature(viewport),
                    "client {index} viewport for {pane} diverged"
                );
            }
        }
    }

    /// The patch-accumulated viewport equals a freshly requested full frame.
    fn assert_patch_full_duality(&mut self) {
        let panes = terminal_panes(self.clients[0].core.snapshot());
        for pane in panes {
            let Some(retained) = self.clients[0].core.viewport(pane) else {
                continue;
            };
            let before = content_signature(retained);
            self.clients[0]
                .client
                .request_full(pane)
                .expect("request full frame");
            let deadline = Instant::now() + FULL_FRAME_DEADLINE;
            loop {
                self.pump_all();
                let fresh = self.clients[0].core.viewport(pane).map(content_signature);
                if fresh.as_ref() == Some(&before) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "pane {pane}: retained viewport diverged from the fresh full frame:\nretained: {before:?}\nfresh: {fresh:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// A resync converges back to the same normalized snapshot.
    fn assert_resync_soundness(&mut self) {
        let before = normalized_snapshot(self.clients[0].core.snapshot());
        self.clients[0]
            .client
            .request_resync()
            .expect("request resync");
        self.quiesce();
        assert_eq!(
            before,
            normalized_snapshot(self.clients[0].core.snapshot()),
            "resync changed the converged snapshot"
        );
    }

    fn shutdown(self) {
        if let Ok(mut commands) = CommandClient::connect(&self.socket) {
            let _ = commands.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn connect_commands(socket: &Path) -> CommandClient {
    let deadline = Instant::now() + Duration::from_secs(30);
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

fn fixture_command() -> &'static str {
    "printf 'zz-sim-ready\\r\\n'; exec /bin/cat"
}

fn pick(rng: &mut Rng, panes: &[PaneId]) -> Option<PaneId> {
    if panes.is_empty() {
        return None;
    }
    let index = usize::try_from(rng.next(panes.len() as u64)).unwrap_or(0);
    panes.get(index).copied()
}

fn run_simulation(seed: u64) {
    let mut simulation = Simulation::boot(seed);
    let mut rng = Rng(seed);
    simulation.run_traffic(&mut rng);
    simulation.assert_snapshots_converged();
    simulation.assert_viewports_converged();
    simulation.assert_patch_full_duality();
    simulation.assert_resync_soundness();
    simulation.shutdown();
}

#[test]
fn seeded_convergence_seed_5eed() {
    run_simulation(0x5eed);
}

#[test]
fn seeded_convergence_seed_c0ffee() {
    run_simulation(0xc0f_fee);
}
