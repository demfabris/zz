//! The lane between an agent pane's runtime and the clients watching it:
//! per-pane coalescing, the wire sequence, the in-memory replay ring, and the
//! small typed pane state.
//!
//! The host stamps its own per-pane counter, but the wire sequence is minted
//! here: request replies (session listings, turn diffs) leave the stream
//! entirely, and a replay that outruns the ring synthesizes fresh items, so
//! only this side can promise the numbering a client replays against.
//!
//! Lock order is fanout-then-daemon, never the reverse: a publisher call may
//! take the daemon's state lock, so nothing holding that lock may reach in
//! here.

use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};
use serde_json::Value;
use zz_protocol::{
    AgentPaneWire, AgentPermissionWire, ClientId, MAX_AGENT_UPDATES_BYTES, PaneId,
    agent_update_batch_bytes,
};

use crate::agent::{
    host::{AgentConnectionPhase, AgentHost, AgentPaneSpec, AgentPaneState, HostCommand},
    journal::AgentJournal,
    runtime::AgentSpawnConfig,
    stream::{AgentPrompt, AgentStreamItem, AgentStreamPayload},
};

/// How long items are gathered before one frame leaves. An ACP turn bursts
/// hundreds of small updates; a client only ever needs them at frame rate.
const BATCH_WINDOW: Duration = Duration::from_millis(25);
/// What one pane keeps replayable in memory. Past it a reattaching client is
/// served from the journal instead.
const MAX_REPLAY_RING_BYTES: usize = 16 * 1024 * 1024;
/// A derived pane title is the opening words of the first prompt: enough to
/// tell agent panes apart in the tree without wrapping the pane header.
const MAX_TITLE_WORDS: usize = 7;
const MAX_TITLE_CHARS: usize = 48;
/// The title an agent pane is born with. A pane still wearing it has never
/// been named — by the user, by a rename, or by an earlier prompt — so it is
/// the one title the daemon may overwrite.
const DEFAULT_AGENT_PANE_TITLE: &str = "agent";

/// A reply to one client request, correlated by the identifier that client
/// sent. Session listings carry no client identifier on the wire, so they ride
/// [`AgentRequestReply::request_id`] zero and reach everyone on the pane.
pub(crate) enum AgentRequestReply {
    Sessions { request_id: u64, result: String },
    TurnDiff { request_id: u64, result: String },
}

/// What the daemon does with everything an agent pane produces. The fanout
/// knows nothing about mailboxes, sessions, or visibility; this is the whole
/// surface it reaches the daemon through.
pub(crate) trait AgentPublisher: Send + Sync + 'static {
    /// One coalesced frame to every client the pane is visible to, plus
    /// `also` — the client whose replay produced it, visible or not.
    fn publish_agent_updates(
        &self,
        pane: PaneId,
        first_seq: u64,
        items: Vec<Vec<u8>>,
        also: Option<ClientId>,
    );
    fn send_agent_updates(
        &self,
        client: ClientId,
        pane: PaneId,
        first_seq: u64,
        items: Vec<Vec<u8>>,
    );
    fn publish_agent_state(&self, pane: PaneId, state: AgentPaneWire);
    fn publish_agent_reply(&self, pane: PaneId, reply: AgentRequestReply);
    /// The adapter named the session this pane is now speaking to. The daemon
    /// owns that metadata, so it lands in the mux state, not just the stream.
    fn adopt_agent_session(&self, pane: PaneId, session_id: String, cwd: Option<PathBuf>);
    fn title_agent_pane(&self, pane: PaneId, title: String);
}

/// The daemon's handle on the agent runtime: one host, one lane per pane.
pub(crate) struct AgentRuntime {
    host: AgentHost,
    fanout: Arc<AgentFanout>,
}

impl AgentRuntime {
    pub(crate) fn new(
        publisher: &Arc<dyn AgentPublisher>,
        config: AgentSpawnConfig,
        journal: Option<Arc<AgentJournal>>,
    ) -> Self {
        let fanout = Arc::new(AgentFanout::new(publisher, journal.clone()));
        let sink_fanout = Arc::downgrade(&fanout);
        let host = AgentHost::with_journal(
            config,
            Box::new(move |pane, state, item| {
                if let Some(fanout) = sink_fanout.upgrade() {
                    fanout.accept(pane, &state, item);
                }
            }),
            journal,
        );
        Self { host, fanout }
    }

    pub(crate) fn open(&self, pane: PaneId, spec: AgentPaneSpec) -> bool {
        self.fanout.open_lane(pane, spec.resume_session.clone());
        if self.host.open(pane, spec) {
            return true;
        }
        self.fanout.close_lane(pane);
        false
    }

    pub(crate) fn close(&self, pane: PaneId) {
        let _ = self.host.close(pane);
        self.fanout.close_lane(pane);
    }

    pub(crate) fn command(&self, pane: PaneId, command: HostCommand) -> bool {
        self.host.command(pane, command)
    }

    /// Dispatch a prompt and, on the pane's first one, name it after what was
    /// asked.
    pub(crate) fn prompt(&self, pane: PaneId, prompt: AgentPrompt) -> bool {
        let title = self.fanout.propose_title(pane, &prompt.text);
        let sent = self.host.command(pane, HostCommand::Prompt(prompt));
        if !sent {
            return false;
        }
        if let Some((title, publisher)) = title.zip(self.fanout.publisher.upgrade()) {
            publisher.title_agent_pane(pane, title);
        }
        true
    }

    pub(crate) fn prewarm(&self) {
        // No test downloads an adapter package to warm a cache it never uses.
        if cfg!(test) {
            return;
        }
        self.host.prewarm();
    }

    pub(crate) fn reconfigure(&self, config: AgentSpawnConfig) {
        self.host.reconfigure(config);
    }

    /// What the next pane this runtime opens would be spawned with.
    #[cfg(test)]
    pub(crate) fn spawn_config(&self) -> AgentSpawnConfig {
        self.host.config()
    }

    /// What a client needs to render the pane without the stream.
    pub(crate) fn wire_state(&self, pane: PaneId) -> Option<AgentPaneWire> {
        self.fanout.published_state(pane).or_else(|| {
            self.host
                .snapshot_state(pane)
                .map(|state| wire(&state, None))
        })
    }

    pub(crate) fn replay(&self, client: ClientId, pane: PaneId, from_seq: u64) {
        self.fanout.replay(client, pane, from_seq);
    }

    pub(crate) fn shutdown(&self) {
        self.host.shutdown();
        self.fanout.shutdown();
    }

    #[cfg(test)]
    pub(crate) fn set_runner_factory(&self, factory: crate::agent::host::PaneRunnerFactory) {
        self.host.set_runner_factory(factory);
    }
}

impl Drop for AgentRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Everything one pane's stream carries between the host and the wire.
struct PaneLane {
    next_seq: u64,
    /// Highest sequence the ring has dropped, so a replay knows when it has
    /// fallen behind what memory still holds.
    evicted_seq: u64,
    batch: Vec<Vec<u8>>,
    batch_first_seq: u64,
    deadline: Option<Instant>,
    ring: VecDeque<(u64, Vec<u8>)>,
    ring_bytes: usize,
    session_id: Option<String>,
    title: Option<String>,
    titled: bool,
    modes: String,
    config_options: String,
    /// Bumped whenever a blob the pane state carries is replaced, so the
    /// per-item comparison never copies a quarter-megabyte of JSON.
    blobs: u64,
    fingerprint: Option<StateFingerprint>,
    state: Option<AgentPaneWire>,
}

impl PaneLane {
    fn new(session_id: Option<String>) -> Self {
        Self {
            next_seq: 1,
            evicted_seq: 0,
            batch: Vec::new(),
            batch_first_seq: 0,
            deadline: None,
            ring: VecDeque::new(),
            ring_bytes: 0,
            session_id,
            title: None,
            titled: false,
            modes: String::new(),
            config_options: String::new(),
            blobs: 0,
            fingerprint: None,
            state: None,
        }
    }

    fn push(&mut self, seq: u64, encoded: Vec<u8>) {
        if self.batch.is_empty() {
            self.batch_first_seq = seq;
            self.deadline = Some(Instant::now() + BATCH_WINDOW);
        }
        self.batch.push(encoded.clone());
        self.ring_bytes = self.ring_bytes.saturating_add(encoded.len());
        self.ring.push_back((seq, encoded));
        while self.ring_bytes > MAX_REPLAY_RING_BYTES {
            let Some((seq, dropped)) = self.ring.pop_front() else {
                break;
            };
            self.ring_bytes = self.ring_bytes.saturating_sub(dropped.len());
            self.evicted_seq = seq;
        }
    }

    /// Everything gathered so far, split so no frame outgrows the wire bound.
    fn take_batch(&mut self) -> Vec<(u64, Vec<Vec<u8>>)> {
        self.deadline = None;
        let first_seq = self.batch_first_seq;
        split_frames(first_seq, std::mem::take(&mut self.batch))
    }
}

/// One pane's state as the wire sees it, cheap enough to compare per item.
#[derive(PartialEq, Eq)]
struct StateFingerprint {
    phase: AgentConnectionPhase,
    queued: usize,
    permission: Option<u64>,
    session_id: Option<String>,
    error: Option<String>,
    auth_methods: usize,
    blobs: u64,
}

struct AgentFanout {
    publisher: Weak<dyn AgentPublisher>,
    journal: Option<Arc<AgentJournal>>,
    lanes: Mutex<BTreeMap<PaneId, PaneLane>>,
    wake: Condvar,
    stopped: AtomicBool,
    flusher: Mutex<Option<JoinHandle<()>>>,
}

impl AgentFanout {
    fn new(publisher: &Arc<dyn AgentPublisher>, journal: Option<Arc<AgentJournal>>) -> Self {
        Self {
            publisher: Arc::downgrade(publisher),
            journal,
            lanes: Mutex::new(BTreeMap::new()),
            wake: Condvar::new(),
            stopped: AtomicBool::new(false),
            flusher: Mutex::new(None),
        }
    }

    fn open_lane(self: &Arc<Self>, pane: PaneId, session_id: Option<String>) {
        self.lanes.lock().insert(pane, PaneLane::new(session_id));
        self.ensure_flusher();
    }

    fn close_lane(&self, pane: PaneId) {
        self.lanes.lock().remove(&pane);
    }

    fn published_state(&self, pane: PaneId) -> Option<AgentPaneWire> {
        self.lanes.lock().get(&pane)?.state.clone()
    }

    /// One item from a pane, plus the pane state it left behind. A state-only
    /// call (no item) is how a queued prompt reaches the badges.
    fn accept(&self, pane: PaneId, state: &AgentPaneState, item: Option<AgentStreamItem>) {
        let Some(publisher) = self.publisher.upgrade() else {
            return;
        };
        let mut adoption = None;
        let mut reply = None;
        let next_state = {
            let mut lanes = self.lanes.lock();
            let Some(lane) = lanes.get_mut(&pane) else {
                return;
            };
            if let Some(item) = item {
                match &item.payload {
                    AgentStreamPayload::SessionReady {
                        session_id,
                        modes,
                        config_options,
                    } => {
                        lane.session_id = Some(session_id.clone());
                        lane.modes = blob(modes.as_ref());
                        lane.config_options = blob(config_options.as_ref());
                        lane.blobs = lane.blobs.saturating_add(1);
                        adoption = Some((session_id.clone(), None));
                    }
                    AgentStreamPayload::SessionSwitched {
                        session_id,
                        cwd,
                        modes,
                        config_options,
                        ..
                    } => {
                        lane.session_id = Some(session_id.clone());
                        lane.modes = blob(modes.as_ref());
                        lane.config_options = blob(config_options.as_ref());
                        lane.blobs = lane.blobs.saturating_add(1);
                        adoption = Some((session_id.clone(), Some(cwd.clone())));
                    }
                    AgentStreamPayload::ConfigOptionsChanged { config_options, .. } => {
                        lane.config_options = blob(Some(config_options));
                        lane.blobs = lane.blobs.saturating_add(1);
                    }
                    AgentStreamPayload::SessionsListed { .. } => {
                        reply =
                            encode_reply(&item.payload).map(|result| AgentRequestReply::Sessions {
                                request_id: 0,
                                result,
                            });
                    }
                    AgentStreamPayload::TurnDiff { request_id, .. } => {
                        reply =
                            encode_reply(&item.payload).map(|result| AgentRequestReply::TurnDiff {
                                request_id: *request_id,
                                result,
                            });
                    }
                    _ => {}
                }
                if reply.is_none() {
                    lane.enqueue(pane, item.payload);
                }
            }
            lane.refresh_state(state)
        };
        self.wake.notify_all();
        if let Some(reply) = reply {
            publisher.publish_agent_reply(pane, reply);
        }
        if let Some((session_id, cwd)) = adoption {
            publisher.adopt_agent_session(pane, session_id, cwd);
        }
        if let Some(next_state) = next_state {
            publisher.publish_agent_state(pane, next_state);
        }
    }

    /// Name the pane after its opening prompt, once.
    fn propose_title(&self, pane: PaneId, prompt: &str) -> Option<String> {
        let title = derive_pane_title(prompt)?;
        let mut lanes = self.lanes.lock();
        let lane = lanes.get_mut(&pane)?;
        if lane.titled || lane.title.is_some() {
            return None;
        }
        lane.title = Some(title.clone());
        lane.titled = true;
        lane.blobs = lane.blobs.saturating_add(1);
        Some(title)
    }

    fn replay(&self, client: ClientId, pane: PaneId, from_seq: u64) {
        let Some(publisher) = self.publisher.upgrade() else {
            return;
        };
        // The lane stays locked across the sends: a concurrent flush would
        // otherwise land newer sequences ahead of the replay they follow.
        let mut lanes = self.lanes.lock();
        let Some(lane) = lanes.get_mut(&pane) else {
            return;
        };
        let from = from_seq.max(1);
        if from > lane.evicted_seq {
            let items = lane
                .ring
                .iter()
                .filter(|(seq, _)| *seq >= from)
                .map(|(seq, encoded)| (*seq, encoded.clone()))
                .collect::<Vec<_>>();
            let Some(first_seq) = items.first().map(|(seq, _)| *seq) else {
                return;
            };
            for (first_seq, items) in split_frames(
                first_seq,
                items.into_iter().map(|(_, encoded)| encoded).collect(),
            ) {
                publisher.send_agent_updates(client, pane, first_seq, items);
            }
            return;
        }
        for (first_seq, items) in lane.take_batch() {
            publisher.publish_agent_updates(pane, first_seq, items, Some(client));
        }
        let session_id = lane.session_id.clone();
        let replay = session_id
            .as_deref()
            .zip(self.journal.as_ref())
            .and_then(|(session_id, journal)| journal.replay(session_id).ok())
            .unwrap_or_default();
        log::info!(
            target: "zz::agent",
            "replaying pane {pane} for client {client} out of the journal: \
             asked for {from_seq}, memory starts at {}, {} journalled updates",
            lane.evicted_seq.saturating_add(1),
            replay.len(),
        );
        lane.enqueue(pane, AgentStreamPayload::SessionReset { restoring: true });
        for (_, update) in replay {
            lane.enqueue(pane, AgentStreamPayload::Update { update });
        }
        for (first_seq, items) in lane.take_batch() {
            publisher.publish_agent_updates(pane, first_seq, items, Some(client));
        }
        if let Some(state) = lane.state.clone() {
            drop(lanes);
            publisher.publish_agent_state(pane, state);
        }
    }

    fn ensure_flusher(self: &Arc<Self>) {
        let mut flusher = self.flusher.lock();
        if flusher.is_some() {
            return;
        }
        let fanout = Arc::downgrade(self);
        *flusher = std::thread::Builder::new()
            .name("zz-agent-flush".to_owned())
            .spawn(move || run_flusher(&fanout))
            .map_err(|error| {
                log::error!(target: "zz::agent", "could not start the agent flush thread: {error}");
            })
            .ok();
    }

    fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
        if let Some(flusher) = self.flusher.lock().take() {
            let _ = flusher.join();
        }
        self.lanes.lock().clear();
    }
}

impl PaneLane {
    /// Stamp, encode, and queue one payload. An item too large for a single
    /// frame is dropped rather than stamped: the wire cannot carry it, and a
    /// sequence spent on it would look like loss to every client.
    fn enqueue(&mut self, pane: PaneId, payload: AgentStreamPayload) {
        let item = AgentStreamItem {
            seq: self.next_seq,
            payload,
        };
        let encoded = match serde_json::to_vec(&item) {
            Ok(encoded) => encoded,
            Err(error) => {
                log::error!(target: "zz::agent", "could not encode a stream item for pane {pane}: {error}");
                return;
            }
        };
        if encoded.len() > MAX_AGENT_UPDATES_BYTES {
            log::warn!(
                target: "zz::agent",
                "dropping a {} byte agent update for pane {pane}: one item may not exceed {MAX_AGENT_UPDATES_BYTES} bytes",
                encoded.len(),
            );
            return;
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.push(seq, encoded);
    }

    /// The pane state to publish, or `None` when nothing a client renders
    /// moved.
    fn refresh_state(&mut self, state: &AgentPaneState) -> Option<AgentPaneWire> {
        let fingerprint = StateFingerprint {
            phase: state.phase,
            queued: state.queued_prompts,
            permission: state
                .pending_permissions
                .first()
                .map(|permission| permission.request_id),
            session_id: state.session_id.clone(),
            error: state.error.clone(),
            auth_methods: state.auth_methods.len(),
            blobs: self.blobs,
        };
        if self.fingerprint.as_ref() == Some(&fingerprint) {
            return None;
        }
        self.fingerprint = Some(fingerprint);
        let mut next = wire(state, self.title.as_deref());
        next.modes.clone_from(&self.modes);
        next.config_options.clone_from(&self.config_options);
        if let Err(error) = next.validate() {
            log::warn!(target: "zz::agent", "trimming the pane state for {:?}: {error}", state.session_id);
            next.modes.clear();
            next.config_options.clear();
            next.auth_methods.clear();
            next.pending_permission = None;
            next.validate().ok()?;
        }
        self.state = Some(next.clone());
        Some(next)
    }
}

/// The one clock the lane runs. Every window it closes is decided under the
/// lane lock, so an item queued while a frame is in flight cannot be missed,
/// and nothing ticks while no pane has anything gathered.
fn run_flusher(fanout: &Weak<AgentFanout>) {
    loop {
        let Some(fanout) = fanout.upgrade() else {
            return;
        };
        let mut lanes = fanout.lanes.lock();
        if fanout.stopped.load(Ordering::Acquire) {
            return;
        }
        let now = Instant::now();
        let mut due = Vec::new();
        let mut next: Option<Instant> = None;
        for (pane, lane) in lanes.iter_mut() {
            let Some(deadline) = lane.deadline else {
                continue;
            };
            if deadline > now {
                next = Some(next.map_or(deadline, |current| current.min(deadline)));
                continue;
            }
            for (first_seq, items) in lane.take_batch() {
                due.push((*pane, first_seq, items));
            }
        }
        if due.is_empty() {
            match next {
                Some(deadline) => {
                    fanout
                        .wake
                        .wait_for(&mut lanes, deadline.saturating_duration_since(now));
                }
                None => fanout.wake.wait(&mut lanes),
            }
            continue;
        }
        drop(lanes);
        let Some(publisher) = fanout.publisher.upgrade() else {
            return;
        };
        for (pane, first_seq, items) in due {
            publisher.publish_agent_updates(pane, first_seq, items, None);
        }
    }
}

/// Greedily pack encoded items into frames inside the wire bound. The items
/// are consecutive, so each frame's first sequence follows from the split.
fn split_frames(first_seq: u64, items: Vec<Vec<u8>>) -> Vec<(u64, Vec<Vec<u8>>)> {
    let mut frames = Vec::new();
    let mut current: Vec<Vec<u8>> = Vec::new();
    let mut seq = first_seq;
    let mut start = first_seq;
    for item in items {
        if !current.is_empty()
            && agent_update_batch_bytes(&current).saturating_add(item.len())
                > MAX_AGENT_UPDATES_BYTES
        {
            frames.push((start, std::mem::take(&mut current)));
            start = seq;
        }
        current.push(item);
        seq = seq.saturating_add(1);
    }
    if !current.is_empty() {
        frames.push((start, current));
    }
    frames
}

fn blob(value: Option<&Value>) -> String {
    value.map(ToString::to_string).unwrap_or_default()
}

fn encode_reply(payload: &AgentStreamPayload) -> Option<String> {
    serde_json::to_string(payload)
        .map_err(
            |error| log::error!(target: "zz::agent", "could not encode an agent reply: {error}"),
        )
        .ok()
}

/// The host's pane state as a client renders it.
fn wire(state: &AgentPaneState, title: Option<&str>) -> AgentPaneWire {
    let phase = match state.phase {
        AgentConnectionPhase::Starting | AgentConnectionPhase::Restoring => {
            zz_protocol::AgentConnectionPhase::Starting
        }
        AgentConnectionPhase::Ready => zz_protocol::AgentConnectionPhase::Ready,
        AgentConnectionPhase::Running | AgentConnectionPhase::Cancelling => {
            if state.pending_permissions.is_empty() {
                zz_protocol::AgentConnectionPhase::Running
            } else {
                zz_protocol::AgentConnectionPhase::AwaitingPermission
            }
        }
        AgentConnectionPhase::Failed | AgentConnectionPhase::Disconnected => {
            zz_protocol::AgentConnectionPhase::Failed {
                message: state
                    .error
                    .clone()
                    .unwrap_or_else(|| "the agent disconnected".to_owned()),
            }
        }
    };
    AgentPaneWire {
        phase,
        queued_prompts: u32::try_from(state.queued_prompts).unwrap_or(u32::MAX),
        session_id: state.session_id.clone(),
        title: title.map(ToOwned::to_owned),
        auth_methods: serde_json::to_string(&state.auth_methods).unwrap_or_default(),
        config_options: String::new(),
        modes: String::new(),
        pending_permission: state.pending_permissions.first().map(|permission| {
            AgentPermissionWire {
                request_id: permission.request_id,
                payload: serde_json::json!({
                    "toolCall": permission.tool_call,
                    "options": permission.options,
                })
                .to_string(),
            }
        }),
    }
}

fn derive_pane_title(prompt: &str) -> Option<String> {
    let first_line = prompt.trim().lines().next().unwrap_or_default();
    let cleaned = first_line
        .trim_start_matches(['"', '\'', '#', '>', '`', '*', ' ', '\t'])
        .trim_end_matches(['"', '\'', '`', '*', ' ', '\t']);
    let words = cleaned
        .split_whitespace()
        .take(MAX_TITLE_WORDS)
        .collect::<Vec<_>>()
        .join(" ");
    let title = words.chars().take(MAX_TITLE_CHARS).collect::<String>();
    let title = title.trim_end();
    (!title.is_empty()).then(|| title.to_owned())
}

/// Whether a pane still wears the title it was created with.
pub(crate) fn is_default_agent_title(title: &str) -> bool {
    title == DEFAULT_AGENT_PANE_TITLE
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent::stream::{AgentSessionSummary, AgentTurnDiffOutcome};

    const DEADLINE: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct Recorder {
        broadcast: Mutex<Vec<(PaneId, u64, Vec<Vec<u8>>, Option<ClientId>)>>,
        direct: Mutex<Vec<(ClientId, PaneId, u64, Vec<Vec<u8>>)>>,
        states: Mutex<Vec<(PaneId, AgentPaneWire)>>,
        replies: Mutex<Vec<(PaneId, u64, String)>>,
        sessions: Mutex<Vec<(PaneId, String)>>,
        titles: Mutex<Vec<(PaneId, String)>>,
    }

    impl Recorder {
        fn delivered(&self) -> usize {
            let broadcast = self
                .broadcast
                .lock()
                .iter()
                .map(|(_, _, items, _)| items.len())
                .sum::<usize>();
            let direct = self
                .direct
                .lock()
                .iter()
                .map(|(_, _, _, items)| items.len())
                .sum::<usize>();
            broadcast + direct
        }

        /// Every item any client was sent, in delivery order.
        fn payloads(&self) -> Vec<AgentStreamItem> {
            let mut items = self
                .broadcast
                .lock()
                .iter()
                .flat_map(|(_, _, items, _)| items.clone())
                .collect::<Vec<_>>();
            items.extend(
                self.direct
                    .lock()
                    .iter()
                    .flat_map(|(_, _, _, items)| items.clone()),
            );
            items
                .iter()
                .map(|item| serde_json::from_slice(item).expect("decode stream item"))
                .collect()
        }

        fn wait_for_items(&self, count: usize) -> Vec<AgentStreamItem> {
            let deadline = Instant::now() + DEADLINE;
            while Instant::now() < deadline {
                if self.delivered() >= count {
                    return self.payloads();
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            panic!(
                "timed out waiting for {count} items, saw {}",
                self.delivered()
            );
        }
    }

    impl AgentPublisher for Recorder {
        fn publish_agent_updates(
            &self,
            pane: PaneId,
            first_seq: u64,
            items: Vec<Vec<u8>>,
            also: Option<ClientId>,
        ) {
            self.broadcast.lock().push((pane, first_seq, items, also));
        }

        fn send_agent_updates(
            &self,
            client: ClientId,
            pane: PaneId,
            first_seq: u64,
            items: Vec<Vec<u8>>,
        ) {
            self.direct.lock().push((client, pane, first_seq, items));
        }

        fn publish_agent_state(&self, pane: PaneId, state: AgentPaneWire) {
            self.states.lock().push((pane, state));
        }

        fn publish_agent_reply(&self, pane: PaneId, reply: AgentRequestReply) {
            let (request_id, result) = match reply {
                AgentRequestReply::Sessions { request_id, result }
                | AgentRequestReply::TurnDiff { request_id, result } => (request_id, result),
            };
            self.replies.lock().push((pane, request_id, result));
        }

        fn adopt_agent_session(&self, pane: PaneId, session_id: String, _cwd: Option<PathBuf>) {
            self.sessions.lock().push((pane, session_id));
        }

        fn title_agent_pane(&self, pane: PaneId, title: String) {
            self.titles.lock().push((pane, title));
        }
    }

    struct Fixture {
        fanout: Arc<AgentFanout>,
        recorder: Arc<Recorder>,
        pane: PaneId,
    }

    impl Fixture {
        fn open(journal: Option<Arc<AgentJournal>>, session_id: Option<&str>) -> Self {
            let recorder = Arc::new(Recorder::default());
            let publisher: Arc<dyn AgentPublisher> = Arc::<Recorder>::clone(&recorder);
            let fanout = Arc::new(AgentFanout::new(&publisher, journal));
            let pane = PaneId(4);
            fanout.open_lane(pane, session_id.map(ToOwned::to_owned));
            Self {
                fanout,
                recorder,
                pane,
            }
        }

        fn accept(&self, payload: AgentStreamPayload) {
            self.fanout.accept(
                self.pane,
                &state(AgentConnectionPhase::Running),
                Some(AgentStreamItem { seq: 0, payload }),
            );
        }

        fn chunk(&self, text: &str) {
            self.accept(AgentStreamPayload::Update {
                update: json!({ "sessionUpdate": "agent_message_chunk", "content": { "text": text } }),
            });
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.fanout.shutdown();
        }
    }

    fn state(phase: AgentConnectionPhase) -> AgentPaneState {
        let mut state = AgentPaneState::for_test();
        state.phase = phase;
        state
    }

    fn text_of(item: &AgentStreamItem) -> Option<String> {
        let AgentStreamPayload::Update { update } = &item.payload else {
            return None;
        };
        update
            .get("content")?
            .get("text")?
            .as_str()
            .map(ToOwned::to_owned)
    }

    #[test]
    fn a_window_of_items_leaves_as_one_frame_numbered_from_the_first() {
        let fixture = Fixture::open(None, Some("s-1"));
        for text in ["a", "b", "c"] {
            fixture.chunk(text);
        }
        let items = fixture.recorder.wait_for_items(3);

        assert_eq!(
            items.iter().map(|item| item.seq).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(
            items.iter().filter_map(text_of).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        let broadcast = fixture.recorder.broadcast.lock();
        assert_eq!(broadcast.len(), 1, "one window, one frame: {broadcast:?}");
        assert_eq!(broadcast[0].1, 1);
    }

    #[test]
    fn a_replay_inside_the_ring_serves_only_what_the_client_missed() {
        let fixture = Fixture::open(None, Some("s-1"));
        for text in ["a", "b", "c"] {
            fixture.chunk(text);
        }
        fixture.recorder.wait_for_items(3);

        fixture.fanout.replay(ClientId(9), fixture.pane, 2);
        let direct = fixture.recorder.direct.lock().clone();
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].0, ClientId(9));
        assert_eq!(direct[0].2, 2, "the replay starts where the client stopped");
        let items = direct[0]
            .3
            .iter()
            .map(|item| serde_json::from_slice::<AgentStreamItem>(item).expect("decode"))
            .collect::<Vec<_>>();
        assert_eq!(
            items.iter().filter_map(text_of).collect::<Vec<_>>(),
            ["b", "c"]
        );

        fixture.fanout.replay(ClientId(9), fixture.pane, 9);
        assert_eq!(
            fixture.recorder.direct.lock().len(),
            1,
            "a client already ahead of the ring is sent nothing"
        );
    }

    #[test]
    fn a_replay_older_than_the_ring_resets_and_replays_the_journal() {
        let directory = tempfile::tempdir().expect("journal directory");
        let journal = Arc::new(AgentJournal::open(directory.path()).expect("open journal"));
        for text in ["one", "two"] {
            journal
                .append(
                    "s-1",
                    &json!({ "sessionUpdate": "agent_message_chunk", "content": { "text": text } }),
                )
                .expect("append");
        }
        let fixture = Fixture::open(Some(journal), Some("s-1"));

        // Overrun the ring: every item is stamped, so the first ones are the
        // ones memory drops.
        let filler = "x".repeat(900 * 1024);
        for _ in 0..20 {
            fixture.chunk(&filler);
        }
        fixture.recorder.wait_for_items(20);
        let last_live = fixture.fanout.lanes.lock()[&fixture.pane]
            .next_seq
            .saturating_sub(1);

        fixture.fanout.replay(ClientId(3), fixture.pane, 1);
        let replayed = fixture
            .recorder
            .broadcast
            .lock()
            .iter()
            .filter(|(_, _, _, also)| *also == Some(ClientId(3)))
            .flat_map(|(_, _, items, _)| items.clone())
            .map(|item| serde_json::from_slice::<AgentStreamItem>(&item).expect("decode"))
            .collect::<Vec<_>>();

        assert!(
            matches!(
                replayed.first().map(|item| &item.payload),
                Some(AgentStreamPayload::SessionReset { restoring: true })
            ),
            "the fallback resets before it replays: {replayed:?}"
        );
        assert_eq!(
            replayed.iter().filter_map(text_of).collect::<Vec<_>>(),
            ["one", "two"]
        );
        assert!(
            replayed[0].seq > last_live,
            "the synthesized replay is stamped with fresh sequences"
        );
        assert_eq!(
            replayed.iter().map(|item| item.seq).collect::<Vec<_>>(),
            (replayed[0].seq..replayed[0].seq + 3).collect::<Vec<_>>(),
            "and stays contiguous for every client on the pane"
        );
    }

    #[test]
    fn request_replies_leave_the_stream_without_spending_a_sequence() {
        let fixture = Fixture::open(None, Some("s-1"));
        fixture.accept(AgentStreamPayload::SessionsListed {
            sessions: vec![AgentSessionSummary {
                session_id: "s-2".to_owned(),
                cwd: PathBuf::from("/work"),
                additional_directories: Vec::new(),
                title: None,
                updated_at: None,
            }],
            next_cursor: None,
            cwd_filter: None,
            replace: true,
        });
        fixture.accept(AgentStreamPayload::TurnDiff {
            request_id: 7,
            outcome: AgentTurnDiffOutcome::Failed {
                message: "no turn".to_owned(),
            },
        });
        fixture.chunk("after");
        let items = fixture.recorder.wait_for_items(1);

        assert_eq!(
            items.iter().map(|item| item.seq).collect::<Vec<_>>(),
            [1],
            "a reply is not part of the transcript the client replays"
        );
        let replies = fixture.recorder.replies.lock();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0].1, 0, "session listings carry no client request");
        assert_eq!(replies[1].1, 7);
        assert!(replies[1].2.contains("no turn"));
    }

    #[test]
    fn the_first_prompt_names_the_pane_and_later_ones_leave_it_alone() {
        let fixture = Fixture::open(None, None);

        assert_eq!(
            fixture.fanout.propose_title(
                fixture.pane,
                "  # Fix the flaky reconcile test  \nand then some"
            ),
            Some("Fix the flaky reconcile test".to_owned())
        );
        assert_eq!(
            fixture.fanout.propose_title(fixture.pane, "and again"),
            None
        );
        assert_eq!(derive_pane_title("   \n"), None);
    }

    #[test]
    fn the_pane_state_is_published_only_when_something_a_client_renders_moves() {
        let fixture = Fixture::open(None, Some("s-1"));
        fixture.chunk("a");
        fixture.chunk("b");
        assert_eq!(
            fixture.recorder.states.lock().len(),
            1,
            "a steady turn republishes nothing"
        );

        let mut running = state(AgentConnectionPhase::Running);
        running.queued_prompts = 1;
        fixture.fanout.accept(fixture.pane, &running, None);
        let states = fixture.recorder.states.lock();
        assert_eq!(states.len(), 2);
        assert_eq!(states[1].1.queued_prompts, 1);
    }
}
