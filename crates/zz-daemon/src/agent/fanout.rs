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
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use agent_client_protocol::schema::v1::SessionUpdate;
use parking_lot::{Condvar, Mutex};
use serde_json::Value;
use zz_protocol::{
    AgentPaneWire, AgentPermissionWire, AgentProvider, ClientId, ClientInstanceId,
    MAX_AGENT_PROMPT_BYTES, MAX_AGENT_QUEUED_PROMPTS, MAX_AGENT_RESULT_BYTES,
    MAX_AGENT_UPDATES_BYTES, PaneId,
};

use crate::agent::{
    host::{AgentConnectionPhase, AgentHost, AgentPaneSpec, AgentPaneState, HostCommand},
    journal::{AgentJournal, JournalEntry},
    runtime::AgentSpawnConfig,
    stream::{AgentPrompt, AgentStreamItem, AgentStreamPayload},
};

/// How long items are gathered before one frame leaves. An ACP turn bursts
/// hundreds of small updates; a client only ever needs them at frame rate.
const BATCH_WINDOW: Duration = Duration::from_millis(25);
/// What one pane keeps replayable in memory. Past it a reattaching client is
/// served from the journal instead.
const MAX_REPLAY_RING_BYTES: usize = 2 * MAX_AGENT_UPDATES_BYTES;
const MAX_AGENT_FRAME_BYTES: usize = MAX_AGENT_RESULT_BYTES;
const MAX_RECLAIMED_PROMPTS: usize = MAX_AGENT_QUEUED_PROMPTS + 1;
/// A derived pane title is the opening words of the first prompt: enough to
/// tell agent panes apart in the tree without wrapping the pane header.
const MAX_TITLE_WORDS: usize = 7;
const MAX_TITLE_CHARS: usize = 48;
/// The title an agent pane is born with. A pane still wearing it has never
/// been named — by the user, by a rename, or by an earlier prompt — so it is
/// the one title the daemon may overwrite.
const DEFAULT_AGENT_PANE_TITLE: &str = "agent";

pub(crate) enum AgentRequestReply {
    Sessions { client: ClientId, result: String },
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
    fn send_agent_replay(&self, client: ClientId, pane: PaneId, frames: Vec<(u64, Vec<Vec<u8>>)>);
    fn publish_agent_replay(
        &self,
        pane: PaneId,
        frames: Vec<(u64, Vec<Vec<u8>>)>,
        also: Option<ClientId>,
    );
    fn publish_agent_state(&self, pane: PaneId, state: AgentPaneWire);
    fn send_agent_reply(&self, pane: PaneId, reply: AgentRequestReply);
    /// The adapter named the session this pane is now speaking to. The daemon
    /// owns that metadata, so it lands in the mux state, not just the stream.
    fn adopt_agent_session(
        &self,
        pane: PaneId,
        provider: AgentProvider,
        session_id: String,
        cwd: Option<PathBuf>,
    );
    fn title_agent_pane(&self, pane: PaneId, title: String);
}

/// The daemon's handle on the agent runtime: one host, one lane per pane.
pub(crate) struct AgentRuntime {
    host: AgentHost,
    fanout: Arc<AgentFanout>,
    generation: AtomicU64,
    lifecycle: Arc<Mutex<()>>,
}

impl AgentRuntime {
    pub(crate) fn new(
        publisher: &Arc<dyn AgentPublisher>,
        config: AgentSpawnConfig,
        journal: Option<Arc<AgentJournal>>,
    ) -> Self {
        let lifecycle = Arc::new(Mutex::new(()));
        let fanout = Arc::new(AgentFanout::new(
            publisher,
            journal.clone(),
            Arc::clone(&lifecycle),
        ));
        let sink_fanout = Arc::downgrade(&fanout);
        let host = AgentHost::with_journal(
            config,
            Box::new(move |pane, generation, state, item| {
                if let Some(fanout) = sink_fanout.upgrade() {
                    fanout.accept(pane, generation, &state, item);
                }
            }),
            journal,
        );
        Self {
            host,
            fanout,
            generation: AtomicU64::new(1),
            lifecycle,
        }
    }

    pub(crate) fn open(&self, pane: PaneId, spec: AgentPaneSpec) -> bool {
        let lifecycle = self.lifecycle.lock();
        if self.host.contains(pane) {
            return true;
        }
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        self.fanout
            .open_lane(pane, generation, spec.provider, spec.resume_session.clone());
        if self.host.open(pane, generation, spec) {
            let state = self.host.snapshot_state(pane);
            drop(lifecycle);
            if let Some(state) = state {
                self.fanout.accept(pane, generation, &state, None);
            }
            return true;
        }
        self.fanout.close_lane(pane);
        false
    }

    pub(crate) fn restart(&self, pane: PaneId, spec: AgentPaneSpec) -> bool {
        let lifecycle = self.lifecycle.lock();
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let _ = self.host.close(pane);
        self.fanout
            .restart_lane(pane, generation, spec.provider, spec.resume_session.clone());
        if self.host.open(pane, generation, spec) {
            let state = self.host.snapshot_state(pane);
            drop(lifecycle);
            if let Some(state) = state {
                self.fanout.accept(pane, generation, &state, None);
            }
            true
        } else {
            self.fanout.close_lane(pane);
            false
        }
    }

    pub(crate) fn close(&self, pane: PaneId) {
        let _lifecycle = self.lifecycle.lock();
        let _ = self.host.close(pane);
        self.fanout.close_lane(pane);
    }

    pub(crate) fn command(&self, pane: PaneId, command: HostCommand) -> bool {
        let _lifecycle = self.lifecycle.lock();
        match self.host.command(pane, command) {
            Ok(()) => true,
            Err(command) => self.fanout.reject_command(pane, command),
        }
    }

    /// Dispatch a prompt and, on the pane's first one, name it after what was
    /// asked.
    pub(crate) fn prompt(&self, pane: PaneId, prompt: AgentPrompt) -> bool {
        let _lifecycle = self.lifecycle.lock();
        let title = self.fanout.propose_title(pane, &prompt.text);
        let sent = match self.host.command(pane, HostCommand::Prompt(prompt)) {
            Ok(()) => true,
            Err(HostCommand::Prompt(prompt)) => self.fanout.reclaim_prompt(pane, prompt),
            Err(_) => unreachable!(),
        };
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
        let _lifecycle = self.lifecycle.lock();
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

    pub(crate) fn acknowledge_prompt_restore(
        &self,
        owner: ClientInstanceId,
        pane: PaneId,
        reclaim_id: u64,
    ) {
        let _lifecycle = self.lifecycle.lock();
        self.fanout
            .acknowledge_prompt_restore(owner, pane, reclaim_id);
    }

    pub(crate) fn shutdown(&self) {
        let lifecycle = self.lifecycle.lock();
        self.fanout.shutdown();
        drop(lifecycle);
        self.host.shutdown();
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
    generation: u64,
    provider: AgentProvider,
    next_seq: u64,
    /// Highest sequence the ring has dropped, so a replay knows when it has
    /// fallen behind what memory still holds.
    evicted_seq: u64,
    batch: Vec<Vec<u8>>,
    batch_bytes: usize,
    batch_first_seq: u64,
    deadline: Option<Instant>,
    ring: VecDeque<(u64, Vec<u8>)>,
    ring_bytes: usize,
    session_id: Option<String>,
    title: Option<String>,
    titled: bool,
    modes: String,
    config_options: String,
    ready: Option<AgentStreamPayload>,
    modes_value: Option<Value>,
    config_options_value: Option<Value>,
    turn_id: Option<u64>,
    reclaimed: VecDeque<ReclaimedPrompt>,
    reclaimed_bytes: usize,
    next_reclaim_id: u64,
    /// Bumped whenever a blob the pane state carries is replaced, so the
    /// per-item comparison never copies a quarter-megabyte of JSON.
    blobs: u64,
    fingerprint: Option<StateFingerprint>,
    state: Option<AgentPaneWire>,
    sync_next_state: bool,
}

#[derive(Clone)]
struct ReclaimedPrompt {
    reclaim_id: u64,
    last_seq: u64,
    prompt: AgentPrompt,
}

impl PaneLane {
    fn new(generation: u64, provider: AgentProvider, session_id: Option<String>) -> Self {
        Self {
            generation,
            provider,
            next_seq: 1,
            evicted_seq: 0,
            batch: Vec::new(),
            batch_bytes: 0,
            batch_first_seq: 0,
            deadline: None,
            ring: VecDeque::new(),
            ring_bytes: 0,
            session_id,
            title: None,
            titled: false,
            modes: String::new(),
            config_options: String::new(),
            ready: None,
            modes_value: None,
            config_options_value: None,
            turn_id: None,
            reclaimed: VecDeque::new(),
            reclaimed_bytes: 0,
            next_reclaim_id: 1,
            blobs: 0,
            fingerprint: None,
            state: None,
            sync_next_state: false,
        }
    }

    fn push(&mut self, seq: u64, encoded: Vec<u8>) {
        if self.batch_bytes.saturating_add(encoded.len()) > MAX_AGENT_UPDATES_BYTES {
            self.batch.clear();
            self.batch_bytes = 0;
        }
        if self.batch.is_empty() {
            self.batch_first_seq = seq;
            self.deadline = Some(Instant::now() + BATCH_WINDOW);
        }
        self.batch.push(encoded.clone());
        self.batch_bytes = self.batch_bytes.saturating_add(encoded.len());
        self.push_ring(seq, encoded);
    }

    fn push_ring(&mut self, seq: u64, encoded: Vec<u8>) {
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
        self.batch_bytes = 0;
        let first_seq = self.batch_first_seq;
        split_frames(first_seq, std::mem::take(&mut self.batch))
    }

    fn restart(&mut self, generation: u64, provider: AgentProvider, session_id: Option<String>) {
        if self.provider != provider {
            self.title = None;
            self.titled = false;
        }
        self.generation = generation;
        self.provider = provider;
        self.session_id = session_id;
        self.modes.clear();
        self.config_options.clear();
        self.ready = None;
        self.modes_value = None;
        self.config_options_value = None;
        self.turn_id = None;
        self.blobs = self.blobs.saturating_add(1);
        self.fingerprint = None;
        self.state = None;
        self.sync_next_state = true;
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
    git: Option<zz_protocol::AgentGitSummary>,
    blobs: u64,
}

struct AgentFanout {
    publisher: Weak<dyn AgentPublisher>,
    journal: Option<Arc<AgentJournal>>,
    lifecycle: Arc<Mutex<()>>,
    lanes: Mutex<BTreeMap<PaneId, PaneLane>>,
    wake: Condvar,
    stopped: AtomicBool,
    flusher: Mutex<Option<JoinHandle<()>>>,
}

impl AgentFanout {
    fn new(
        publisher: &Arc<dyn AgentPublisher>,
        journal: Option<Arc<AgentJournal>>,
        lifecycle: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            publisher: Arc::downgrade(publisher),
            journal,
            lifecycle,
            lanes: Mutex::new(BTreeMap::new()),
            wake: Condvar::new(),
            stopped: AtomicBool::new(false),
            flusher: Mutex::new(None),
        }
    }

    fn open_lane(
        self: &Arc<Self>,
        pane: PaneId,
        generation: u64,
        provider: AgentProvider,
        session_id: Option<String>,
    ) {
        self.lanes
            .lock()
            .insert(pane, PaneLane::new(generation, provider, session_id));
        self.ensure_flusher();
    }

    fn restart_lane(
        self: &Arc<Self>,
        pane: PaneId,
        generation: u64,
        provider: AgentProvider,
        session_id: Option<String>,
    ) {
        let mut lanes = self.lanes.lock();
        if let Some(lane) = lanes.get_mut(&pane) {
            lane.restart(generation, provider, session_id);
        } else {
            lanes.insert(pane, PaneLane::new(generation, provider, session_id));
        }
        drop(lanes);
        self.ensure_flusher();
    }

    fn close_lane(&self, pane: PaneId) {
        self.lanes.lock().remove(&pane);
    }

    fn published_state(&self, pane: PaneId) -> Option<AgentPaneWire> {
        self.lanes.lock().get(&pane)?.state.clone()
    }

    fn reclaim_prompt(&self, pane: PaneId, prompt: AgentPrompt) -> bool {
        let mut lanes = self.lanes.lock();
        let Some(lane) = lanes.get_mut(&pane) else {
            return false;
        };
        lane.enqueue_reclaimed(pane, vec![prompt]);
        drop(lanes);
        self.wake.notify_all();
        true
    }

    fn reject_command(&self, pane: PaneId, command: HostCommand) -> bool {
        let Some(publisher) = self.publisher.upgrade() else {
            return false;
        };
        if !self.lanes.lock().contains_key(&pane) {
            return false;
        }
        let payload = match command {
            HostCommand::Authenticate { .. } => AgentStreamPayload::AuthenticationFailed {
                message: "agent command queue is busy".to_owned(),
            },
            HostCommand::SetConfigOption { option_id, .. } => AgentStreamPayload::SettingFailed {
                option_id,
                message: "agent command queue is busy".to_owned(),
            },
            HostCommand::SetMode { .. } => AgentStreamPayload::SettingFailed {
                option_id: "legacy-session-mode".to_owned(),
                message: "agent command queue is busy".to_owned(),
            },
            HostCommand::ListSessions { client, .. } => {
                let payload = AgentStreamPayload::SessionListFailed {
                    client,
                    message: "agent command queue is busy".to_owned(),
                };
                let Some(result) = encode_reply(&payload) else {
                    return false;
                };
                publisher.send_agent_reply(pane, AgentRequestReply::Sessions { client, result });
                return true;
            }
            HostCommand::DeleteSession { client, .. } => {
                let payload = AgentStreamPayload::SessionDeleteFailed {
                    client,
                    message: "agent command queue is busy".to_owned(),
                };
                let Some(result) = encode_reply(&payload) else {
                    return false;
                };
                publisher.send_agent_reply(pane, AgentRequestReply::Sessions { client, result });
                return true;
            }
            HostCommand::NewSession { .. } | HostCommand::SwitchSession { .. } => {
                AgentStreamPayload::SessionSwitchFailed {
                    message: "agent command queue is busy".to_owned(),
                }
            }
            HostCommand::Prompt(prompt) => {
                return self.reclaim_prompt(pane, prompt);
            }
            HostCommand::Cancel | HostCommand::Unqueue | HostCommand::RespondPermission { .. } => {
                AgentStreamPayload::PaneFailed {
                    message: "agent control queue is busy".to_owned(),
                }
            }
        };
        let mut lanes = self.lanes.lock();
        let Some(lane) = lanes.get_mut(&pane) else {
            return false;
        };
        lane.enqueue(pane, payload);
        drop(lanes);
        self.wake.notify_all();
        true
    }

    /// One item from a pane, plus the pane state it left behind. A state-only
    /// call (no item) is how a queued prompt reaches the badges.
    fn accept(
        &self,
        pane: PaneId,
        generation: u64,
        state: &AgentPaneState,
        item: Option<AgentStreamItem>,
    ) {
        let _lifecycle = self.lifecycle.lock();
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let Some(publisher) = self.publisher.upgrade() else {
            return;
        };
        let mut adoption = None;
        let mut reply = None;
        let mut lanes = self.lanes.lock();
        let Some(lane) = lanes.get_mut(&pane) else {
            return;
        };
        if lane.generation != generation {
            if let Some(AgentStreamItem {
                payload: AgentStreamPayload::PromptsReclaimed { prompts },
                ..
            }) = item
            {
                lane.enqueue_reclaimed(pane, prompts);
                drop(lanes);
                self.wake.notify_all();
            }
            return;
        }
        if let Some(item) = item {
            match &item.payload {
                AgentStreamPayload::Ready { .. } => {
                    lane.ready = Some(item.payload.clone());
                }
                AgentStreamPayload::SessionReady {
                    session_id,
                    modes,
                    config_options,
                } => {
                    if lane.session_id.as_deref() != Some(session_id) {
                        lane.title = None;
                        lane.titled = false;
                        lane.turn_id = None;
                    }
                    lane.session_id = Some(session_id.clone());
                    lane.modes = blob(modes.as_ref());
                    lane.config_options = blob(config_options.as_ref());
                    lane.modes_value.clone_from(modes);
                    lane.config_options_value.clone_from(config_options);
                    lane.blobs = lane.blobs.saturating_add(1);
                    adoption = Some((lane.provider, session_id.clone(), None));
                }
                AgentStreamPayload::SessionSwitched {
                    session_id,
                    cwd,
                    modes,
                    config_options,
                    ..
                } => {
                    lane.turn_id = None;
                    lane.title = None;
                    lane.titled = false;
                    lane.session_id = Some(session_id.clone());
                    lane.modes = blob(modes.as_ref());
                    lane.config_options = blob(config_options.as_ref());
                    lane.modes_value.clone_from(modes);
                    lane.config_options_value.clone_from(config_options);
                    lane.blobs = lane.blobs.saturating_add(1);
                    adoption = Some((lane.provider, session_id.clone(), Some(cwd.clone())));
                }
                AgentStreamPayload::ConfigOptionsChanged { config_options, .. } => {
                    lane.config_options = blob(Some(config_options));
                    lane.config_options_value = Some(config_options.clone());
                    lane.blobs = lane.blobs.saturating_add(1);
                }
                AgentStreamPayload::ModeChanged { mode_id } => {
                    let changed = lane
                        .modes_value
                        .as_mut()
                        .and_then(Value::as_object_mut)
                        .map(|modes| {
                            modes
                                .insert("currentModeId".to_owned(), Value::String(mode_id.clone()));
                        })
                        .is_some();
                    if changed {
                        lane.modes = blob(lane.modes_value.as_ref());
                        lane.blobs = lane.blobs.saturating_add(1);
                    }
                }
                AgentStreamPayload::Update { update } => {
                    match serde_json::from_value::<SessionUpdate>(update.clone()) {
                        Ok(SessionUpdate::CurrentModeUpdate(update)) => {
                            let changed = lane
                                .modes_value
                                .as_mut()
                                .and_then(Value::as_object_mut)
                                .map(|modes| {
                                    modes.insert(
                                        "currentModeId".to_owned(),
                                        Value::String(update.current_mode_id.0.to_string()),
                                    );
                                })
                                .is_some();
                            if changed {
                                lane.modes = blob(lane.modes_value.as_ref());
                                lane.blobs = lane.blobs.saturating_add(1);
                            }
                        }
                        Ok(SessionUpdate::ConfigOptionUpdate(update)) => {
                            if let Ok(value) = serde_json::to_value(update.config_options) {
                                lane.config_options = blob(Some(&value));
                                lane.config_options_value = Some(value);
                                lane.blobs = lane.blobs.saturating_add(1);
                            }
                        }
                        _ => {}
                    }
                }
                AgentStreamPayload::SessionReset { .. } => lane.turn_id = None,
                AgentStreamPayload::TurnStarted { turn_id } => lane.turn_id = Some(*turn_id),
                AgentStreamPayload::SessionsListed { client, .. }
                | AgentStreamPayload::SessionListFailed { client, .. }
                | AgentStreamPayload::SessionDeleted { client, .. }
                | AgentStreamPayload::SessionDeleteFailed { client, .. } => {
                    let result = encode_reply(&item.payload).or_else(|| {
                        encode_reply(&AgentStreamPayload::SessionListFailed {
                            client: *client,
                            message: "agent returned too much session history".to_owned(),
                        })
                    });
                    reply = result.map(|result| AgentRequestReply::Sessions {
                        client: *client,
                        result,
                    });
                }
                _ => {}
            }
            if reply.is_none() {
                match item.payload {
                    AgentStreamPayload::PromptsReclaimed { prompts } => {
                        lane.enqueue_reclaimed(pane, prompts);
                    }
                    payload => lane.enqueue(pane, payload),
                }
            }
        }
        let next_state = lane.refresh_state(state);
        if lane.sync_next_state
            && let Some(state) = next_state.as_ref()
        {
            lane.enqueue(
                pane,
                AgentStreamPayload::StateSynced {
                    state: state.clone(),
                },
            );
            lane.sync_next_state = false;
        }
        drop(lanes);
        self.wake.notify_all();
        if let Some(reply) = reply {
            publisher.send_agent_reply(pane, reply);
        }
        if let Some((provider, session_id, cwd)) = adoption {
            publisher.adopt_agent_session(pane, provider, session_id, cwd);
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
            let frames = split_frames(
                first_seq,
                items.into_iter().map(|(_, encoded)| encoded).collect(),
            );
            for (first_seq, items) in lane.take_batch() {
                publisher.publish_agent_updates(pane, first_seq, items, None);
            }
            publisher.send_agent_replay(client, pane, frames);
            return;
        }
        lane.take_batch();
        let session_id = lane.session_id.clone();
        let replay = session_id
            .as_deref()
            .zip(self.journal.as_ref())
            .and_then(|(session_id, journal)| journal.replay_for(lane.provider, session_id).ok())
            .unwrap_or_default();
        log::info!(
            target: "zz::agent",
            "replaying pane {pane} for client {client} out of the journal: \
             asked for {from_seq}, memory starts at {}, {} journalled updates",
            lane.evicted_seq.saturating_add(1),
            replay.len(),
        );
        let first_seq = lane.next_seq;
        let mut synthesized = Vec::new();
        lane.synthesize(
            pane,
            AgentStreamPayload::SessionReset { restoring: true },
            &mut synthesized,
        );
        if let Some(ready) = lane.ready.clone() {
            lane.synthesize(pane, ready, &mut synthesized);
        }
        for (_, entry) in replay {
            let payload = match entry {
                JournalEntry::Update(update) => AgentStreamPayload::Update { update },
            };
            lane.synthesize(pane, payload, &mut synthesized);
        }
        if let Some(session_id) = session_id {
            lane.synthesize(
                pane,
                AgentStreamPayload::SessionReady {
                    session_id,
                    modes: lane.modes_value.clone(),
                    config_options: lane.config_options_value.clone(),
                },
                &mut synthesized,
            );
        }
        if let Some(turn_id) = lane.turn_id {
            lane.synthesize(
                pane,
                AgentStreamPayload::TurnStarted { turn_id },
                &mut synthesized,
            );
        }
        for reclaimed in lane.reclaimed.clone() {
            let seq = lane.synthesize_with_seq(
                pane,
                AgentStreamPayload::PromptsRestored {
                    reclaim_id: reclaimed.reclaim_id,
                    prompts: vec![reclaimed.prompt],
                },
                &mut synthesized,
            );
            if let Some(seq) = seq
                && let Some(cached) = lane
                    .reclaimed
                    .iter_mut()
                    .find(|cached| cached.reclaim_id == reclaimed.reclaim_id)
            {
                cached.last_seq = seq;
            }
        }
        let state = lane.state.clone();
        if let Some(state) = state {
            lane.synthesize(
                pane,
                AgentStreamPayload::StateSynced { state },
                &mut synthesized,
            );
        }
        let frames = split_frames(first_seq, synthesized);
        publisher.publish_agent_replay(pane, frames, Some(client));
    }

    fn acknowledge_prompt_restore(&self, owner: ClientInstanceId, pane: PaneId, reclaim_id: u64) {
        let mut lanes = self.lanes.lock();
        if let Some(lane) = lanes.get_mut(&pane) {
            lane.acknowledge_reclaimed(owner, reclaim_id);
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
        let lanes = self.lanes.lock();
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
        drop(lanes);
        if let Some(flusher) = self.flusher.lock().take() {
            let _ = flusher.join();
        }
        self.lanes.lock().clear();
    }
}

impl PaneLane {
    fn enqueue_reclaimed(&mut self, pane: PaneId, prompts: Vec<AgentPrompt>) {
        for prompt in prompts {
            let reclaim_id = self.next_reclaim_id;
            self.next_reclaim_id = self.next_reclaim_id.saturating_add(1);
            let Some(last_seq) = self.enqueue_with_seq(
                pane,
                AgentStreamPayload::PromptsRestored {
                    reclaim_id,
                    prompts: vec![prompt.clone()],
                },
            ) else {
                continue;
            };
            self.reclaimed_bytes = self.reclaimed_bytes.saturating_add(prompt.byte_len());
            self.reclaimed.push_back(ReclaimedPrompt {
                reclaim_id,
                last_seq,
                prompt,
            });
            while self.reclaimed_bytes > MAX_AGENT_PROMPT_BYTES
                || self.reclaimed.len() > MAX_RECLAIMED_PROMPTS
            {
                let Some(evicted) = self.reclaimed.pop_front() else {
                    break;
                };
                self.reclaimed_bytes = self
                    .reclaimed_bytes
                    .saturating_sub(evicted.prompt.byte_len());
            }
        }
    }

    fn acknowledge_reclaimed(&mut self, owner: ClientInstanceId, reclaim_id: u64) {
        let Some(index) = self.reclaimed.iter().position(|reclaimed| {
            reclaimed.reclaim_id == reclaim_id && reclaimed.prompt.owner == owner
        }) else {
            return;
        };
        let Some(reclaimed) = self.reclaimed.remove(index) else {
            return;
        };
        self.reclaimed_bytes = self
            .reclaimed_bytes
            .saturating_sub(reclaimed.prompt.byte_len());
        while self
            .ring
            .front()
            .is_some_and(|(seq, _)| *seq <= reclaimed.last_seq)
        {
            let Some((seq, encoded)) = self.ring.pop_front() else {
                break;
            };
            self.ring_bytes = self.ring_bytes.saturating_sub(encoded.len());
            self.evicted_seq = self.evicted_seq.max(seq);
        }
        self.evicted_seq = self.evicted_seq.max(reclaimed.last_seq);
    }

    /// Stamp, encode, and queue one payload. An item too large for a single
    /// frame is dropped rather than stamped: the wire cannot carry it, and a
    /// sequence spent on it would look like loss to every client.
    fn enqueue(&mut self, pane: PaneId, payload: AgentStreamPayload) {
        _ = self.enqueue_with_seq(pane, payload);
    }

    fn enqueue_with_seq(&mut self, pane: PaneId, payload: AgentStreamPayload) -> Option<u64> {
        let (seq, encoded) = self.stamp(pane, payload)?;
        self.push(seq, encoded);
        Some(seq)
    }

    fn synthesize(&mut self, pane: PaneId, payload: AgentStreamPayload, items: &mut Vec<Vec<u8>>) {
        _ = self.synthesize_with_seq(pane, payload, items);
    }

    fn synthesize_with_seq(
        &mut self,
        pane: PaneId,
        payload: AgentStreamPayload,
        items: &mut Vec<Vec<u8>>,
    ) -> Option<u64> {
        let (seq, encoded) = self.stamp(pane, payload)?;
        self.push_ring(seq, encoded.clone());
        items.push(encoded);
        Some(seq)
    }

    fn stamp(&mut self, pane: PaneId, payload: AgentStreamPayload) -> Option<(u64, Vec<u8>)> {
        let item = AgentStreamItem {
            seq: self.next_seq,
            payload,
        };
        let encoded = match serde_json::to_vec(&item) {
            Ok(encoded) => encoded,
            Err(error) => {
                log::error!(target: "zz::agent", "could not encode a stream item for pane {pane}: {error}");
                return None;
            }
        };
        if encoded.len() > MAX_AGENT_UPDATES_BYTES {
            log::warn!(
                target: "zz::agent",
                "dropping a {} byte agent update for pane {pane}: one item may not exceed {MAX_AGENT_UPDATES_BYTES} bytes",
                encoded.len(),
            );
            return None;
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        Some((seq, encoded))
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
            git: state.git.clone(),
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
            next.git = None;
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
    let mut current_bytes = 0usize;
    let mut seq = first_seq;
    let mut start = first_seq;
    for item in items {
        if !current.is_empty() && current_bytes.saturating_add(item.len()) > MAX_AGENT_FRAME_BYTES {
            frames.push((start, std::mem::take(&mut current)));
            start = seq;
            current_bytes = 0;
        }
        current_bytes = current_bytes.saturating_add(item.len());
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
    let encoded = serde_json::to_string(payload)
        .map_err(
            |error| log::error!(target: "zz::agent", "could not encode an agent reply: {error}"),
        )
        .ok()?;
    if encoded.len() > MAX_AGENT_RESULT_BYTES {
        log::warn!(target: "zz::agent", "dropping an oversized {} byte agent reply", encoded.len());
        return None;
    }
    Some(encoded)
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
        error: state.error.clone(),
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
        git: state.git.clone(),
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
    use std::sync::mpsc::{self, Receiver, Sender};

    use serde_json::json;

    use super::*;
    use crate::agent::{
        fixture::{Behavior, fixture_runner},
        stream::{AgentImage, AgentSessionCapabilities, AgentSessionSummary},
    };

    const DEADLINE: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct Recorder {
        broadcast: Mutex<Vec<(PaneId, u64, Vec<Vec<u8>>, Option<ClientId>)>>,
        direct: Mutex<Vec<(ClientId, PaneId, u64, Vec<Vec<u8>>)>>,
        states: Mutex<Vec<(PaneId, AgentPaneWire)>>,
        replies: Mutex<Vec<(ClientId, PaneId, u64, String)>>,
        sessions: Mutex<Vec<(PaneId, String)>>,
        titles: Mutex<Vec<(PaneId, String)>>,
        update_gate: Mutex<Option<(Sender<()>, Receiver<()>)>>,
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
            if let Some((entered, release)) = self.update_gate.lock().take() {
                let _ = entered.send(());
                let _ = release.recv();
            }
            self.broadcast.lock().push((pane, first_seq, items, also));
        }

        fn send_agent_replay(
            &self,
            client: ClientId,
            pane: PaneId,
            frames: Vec<(u64, Vec<Vec<u8>>)>,
        ) {
            self.direct.lock().extend(
                frames
                    .into_iter()
                    .map(|(first_seq, items)| (client, pane, first_seq, items)),
            );
        }

        fn publish_agent_replay(
            &self,
            pane: PaneId,
            frames: Vec<(u64, Vec<Vec<u8>>)>,
            also: Option<ClientId>,
        ) {
            self.broadcast.lock().extend(
                frames
                    .into_iter()
                    .map(|(first_seq, items)| (pane, first_seq, items, also)),
            );
        }

        fn publish_agent_state(&self, pane: PaneId, state: AgentPaneWire) {
            self.states.lock().push((pane, state));
        }

        fn send_agent_reply(&self, pane: PaneId, reply: AgentRequestReply) {
            let AgentRequestReply::Sessions { client, result } = reply;
            let request_id = 0;
            self.replies.lock().push((client, pane, request_id, result));
        }

        fn adopt_agent_session(
            &self,
            pane: PaneId,
            _provider: AgentProvider,
            session_id: String,
            _cwd: Option<PathBuf>,
        ) {
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
            let fanout = Arc::new(AgentFanout::new(
                &publisher,
                journal,
                Arc::new(Mutex::new(())),
            ));
            let pane = PaneId(4);
            fanout.open_lane(
                pane,
                1,
                AgentProvider::Codex,
                session_id.map(ToOwned::to_owned),
            );
            Self {
                fanout,
                recorder,
                pane,
            }
        }

        fn accept(&self, payload: AgentStreamPayload) {
            self.accept_generation(1, payload);
        }

        fn accept_generation(&self, generation: u64, payload: AgentStreamPayload) {
            self.fanout.accept(
                self.pane,
                generation,
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
    fn a_flush_settles_and_drains_before_replay_admission() {
        let fixture = Fixture::open(None, Some("s-1"));
        let (entered_send, entered_recv) = mpsc::channel();
        let (release_send, release_recv) = mpsc::channel();
        *fixture.recorder.update_gate.lock() = Some((entered_send, release_recv));

        fixture.chunk("a");
        entered_recv
            .recv_timeout(DEADLINE)
            .expect("flusher should enter publication");
        assert!(fixture.fanout.lanes.try_lock().is_none());

        release_send.send(()).expect("release flusher");
        let deadline = Instant::now() + DEADLINE;
        let settled = loop {
            if fixture.fanout.lanes.try_lock().is_some() {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::yield_now();
        };
        assert!(settled, "flusher should release the lane");
        fixture.chunk("b");
        assert!(
            fixture
                .fanout
                .lanes
                .lock()
                .get(&fixture.pane)
                .is_some_and(|lane| !lane.batch.is_empty())
        );
        fixture.fanout.replay(ClientId(8), fixture.pane, 0);
        assert!(
            fixture
                .fanout
                .lanes
                .lock()
                .get(&fixture.pane)
                .is_some_and(|lane| lane.batch.is_empty())
        );
        assert_eq!(fixture.recorder.direct.lock().len(), 1);
    }

    #[test]
    fn an_unflushed_batch_stays_inside_its_byte_bound() {
        let mut lane = PaneLane::new(1, AgentProvider::Codex, None);
        let item = vec![b'x'; MAX_AGENT_RESULT_BYTES];
        for seq in 1..=32 {
            lane.push(seq, item.clone());
        }
        assert!(lane.batch_bytes <= MAX_AGENT_UPDATES_BYTES);
        assert!(lane.ring_bytes <= MAX_REPLAY_RING_BYTES);
        assert!(lane.batch.len() <= MAX_AGENT_UPDATES_BYTES / MAX_AGENT_RESULT_BYTES);
    }

    #[test]
    fn zero_byte_reclaimed_prompts_stay_inside_the_count_bound() {
        let mut lane = PaneLane::new(1, AgentProvider::Codex, None);
        for _ in 0..MAX_RECLAIMED_PROMPTS + 8 {
            lane.enqueue_reclaimed(
                PaneId(1),
                vec![AgentPrompt {
                    owner: ClientInstanceId::default(),
                    text: String::new(),
                    images: vec![AgentImage {
                        format: "image/png".to_owned(),
                        data: Vec::new(),
                    }],
                }],
            );
        }

        assert_eq!(lane.reclaimed.len(), MAX_RECLAIMED_PROMPTS);
        assert_eq!(lane.reclaimed_bytes, 0);
    }

    #[test]
    fn only_the_prompt_owner_can_retire_a_restored_prompt() {
        let mut lane = PaneLane::new(1, AgentProvider::Codex, None);
        lane.enqueue_reclaimed(
            PaneId(1),
            vec![AgentPrompt {
                owner: ClientInstanceId(7),
                text: "keep me".to_owned(),
                images: Vec::new(),
            }],
        );
        let reclaim_id = lane.reclaimed[0].reclaim_id;
        let last_seq = lane.reclaimed[0].last_seq;

        lane.acknowledge_reclaimed(ClientInstanceId(8), reclaim_id);
        assert_eq!(lane.reclaimed.len(), 1);

        lane.acknowledge_reclaimed(ClientInstanceId(7), reclaim_id);
        assert!(lane.reclaimed.is_empty());
        assert_eq!(lane.reclaimed_bytes, 0);
        assert!(lane.evicted_seq >= last_seq);
        assert!(lane.ring.iter().all(|(seq, _)| *seq > last_seq));
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
                    &json!({
                        "sessionUpdate": "agent_message_chunk",
                        "messageId": text,
                        "content": { "text": text }
                    }),
                )
                .expect("append");
        }
        let fixture = Fixture::open(Some(journal), Some("s-1"));
        fixture.accept(AgentStreamPayload::Ready {
            agent_name: "Codex".to_owned(),
            agent_key: "codex".to_owned(),
            auth_methods: Vec::new(),
            capabilities: AgentSessionCapabilities::default(),
        });
        fixture.accept(AgentStreamPayload::SessionReady {
            session_id: "s-1".to_owned(),
            modes: Some(json!({ "currentModeId": "plan", "availableModes": [] })),
            config_options: None,
        });
        fixture.recorder.wait_for_items(2);

        // Overrun the ring: every item is stamped, so the first ones are the
        // ones memory drops.
        let filler = "x".repeat(MAX_AGENT_UPDATES_BYTES - 4096);
        let filler_count = MAX_REPLAY_RING_BYTES / filler.len() + 2;
        for _ in 0..filler_count {
            fixture.chunk(&filler);
        }
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
            "the fallback starts with an explicit stream reset: {replayed:?}"
        );
        assert!(matches!(
            replayed.get(1).map(|item| &item.payload),
            Some(AgentStreamPayload::Ready { agent_name, .. }) if agent_name == "Codex"
        ));
        assert!(matches!(
            replayed.get(replayed.len().saturating_sub(2)).map(|item| &item.payload),
            Some(AgentStreamPayload::SessionReady {
                session_id,
                modes: Some(_),
                ..
            }) if session_id == "s-1"
        ));
        assert!(matches!(
            replayed.last().map(|item| &item.payload),
            Some(AgentStreamPayload::StateSynced { .. })
        ));
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
            (replayed[0].seq..replayed[0].seq + 6).collect::<Vec<_>>(),
            "and stays contiguous for every client on the pane"
        );
    }

    #[test]
    fn a_large_journal_replay_keeps_its_reset_and_every_frame() {
        let directory = tempfile::tempdir().expect("journal directory");
        let journal = Arc::new(AgentJournal::open(directory.path()).expect("open journal"));
        let body = "x".repeat(850 * 1024);
        for index in 0..12 {
            journal
                .append(
                    "s-large",
                    &json!({
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "text": format!("{index}:{body}") }
                    }),
                )
                .expect("append");
        }
        let fixture = Fixture::open(Some(journal), Some("s-large"));
        fixture.accept(AgentStreamPayload::Ready {
            agent_name: "Codex".to_owned(),
            agent_key: "codex".to_owned(),
            auth_methods: Vec::new(),
            capabilities: AgentSessionCapabilities::default(),
        });
        fixture.accept(AgentStreamPayload::SessionReady {
            session_id: "s-large".to_owned(),
            modes: None,
            config_options: None,
        });
        fixture.recorder.wait_for_items(2);
        fixture
            .fanout
            .lanes
            .lock()
            .get_mut(&fixture.pane)
            .expect("lane")
            .evicted_seq = 1;

        fixture.fanout.replay(ClientId(3), fixture.pane, 1);
        let frames = fixture
            .recorder
            .broadcast
            .lock()
            .iter()
            .filter(|(_, _, _, also)| *also == Some(ClientId(3)))
            .cloned()
            .collect::<Vec<_>>();
        assert!(frames.len() > 1);
        let replayed = frames
            .into_iter()
            .flat_map(|(_, _, items, _)| items)
            .map(|item| serde_json::from_slice::<AgentStreamItem>(&item).expect("decode"))
            .collect::<Vec<_>>();
        assert!(matches!(
            replayed.first().map(|item| &item.payload),
            Some(AgentStreamPayload::SessionReset { restoring: true })
        ));
        assert!(matches!(
            replayed.last().map(|item| &item.payload),
            Some(AgentStreamPayload::StateSynced { .. })
        ));
        assert_eq!(
            replayed
                .iter()
                .filter(|item| matches!(item.payload, AgentStreamPayload::Update { .. }))
                .count(),
            12
        );
        assert!(
            replayed
                .windows(2)
                .all(|items| items[1].seq == items[0].seq + 1)
        );
    }

    #[test]
    fn restarting_a_lane_keeps_sequences_monotonic_and_drops_the_old_runtime_tail() {
        let fixture = Fixture::open(None, None);
        fixture.chunk("before");
        fixture.recorder.wait_for_items(1);

        fixture
            .fanout
            .restart_lane(fixture.pane, 2, AgentProvider::Codex, None);
        fixture.fanout.accept(
            fixture.pane,
            2,
            &state(AgentConnectionPhase::Starting),
            None,
        );
        fixture.accept_generation(
            1,
            AgentStreamPayload::Update {
                update: json!({ "sessionUpdate": "agent_message_chunk", "content": { "text": "stale" } }),
            },
        );
        fixture.accept_generation(
            2,
            AgentStreamPayload::Update {
                update: json!({ "sessionUpdate": "agent_message_chunk", "content": { "text": "after" } }),
            },
        );

        let items = fixture.recorder.wait_for_items(3);
        assert_eq!(
            items.iter().filter_map(text_of).collect::<Vec<_>>(),
            ["before", "after"]
        );
        assert_eq!(
            items.iter().map(|item| item.seq).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(matches!(
            items.get(1).map(|item| &item.payload),
            Some(AgentStreamPayload::StateSynced { state })
                if state.phase == zz_protocol::AgentConnectionPhase::Starting
        ));
    }

    #[test]
    fn concurrent_restarts_leave_one_live_current_generation() {
        let recorder = Arc::new(Recorder::default());
        let publisher: Arc<dyn AgentPublisher> = Arc::<Recorder>::clone(&recorder);
        let runtime = Arc::new(AgentRuntime::new(
            &publisher,
            AgentSpawnConfig::default(),
            None,
        ));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Arc::new(Mutex::new(Some(release_rx)));
        runtime.set_runner_factory(Box::new(move |spec| {
            let call = calls.fetch_add(1, Ordering::Relaxed);
            if call == 1 {
                entered_tx.send(()).expect("mark first restart");
                release_rx
                    .lock()
                    .take()
                    .expect("first restart release")
                    .recv()
                    .expect("release first restart");
            }
            fixture_runner(spec.provider, Behavior::Chunk, false, true)
        }));
        let pane = PaneId(44);
        let spec = AgentPaneSpec {
            provider: AgentProvider::Codex,
            cwd: PathBuf::from("/"),
            resume_session: None,
            workspace: crate::agent::environment::AgentWorkspaceEnvironment::default(),
        };
        assert!(runtime.open(pane, spec.clone()));

        let first_runtime = Arc::clone(&runtime);
        let first_spec = spec.clone();
        let first = std::thread::spawn(move || first_runtime.restart(pane, first_spec));
        entered_rx
            .recv_timeout(DEADLINE)
            .expect("first restart reached open");
        let second_runtime = Arc::clone(&runtime);
        let second_spec = spec.clone();
        let (second_started_tx, second_started_rx) = std::sync::mpsc::channel();
        let second = std::thread::spawn(move || {
            second_started_tx.send(()).expect("mark second restart");
            second_runtime.restart(pane, second_spec)
        });
        second_started_rx
            .recv_timeout(DEADLINE)
            .expect("second restart started");
        std::thread::sleep(Duration::from_millis(20));
        release_tx.send(()).expect("release restart");

        assert!(first.join().expect("first restart thread"));
        assert!(second.join().expect("second restart thread"));
        assert_eq!(runtime.host.pane_count(), 1);
        assert!(runtime.fanout.lanes.lock().contains_key(&pane));
        assert!(runtime.prompt(
            pane,
            AgentPrompt {
                owner: ClientInstanceId::default(),
                text: "probe".to_owned(),
                images: Vec::new(),
            },
        ));
        let deadline = Instant::now() + DEADLINE;
        while Instant::now() < deadline {
            if recorder
                .payloads()
                .iter()
                .filter_map(text_of)
                .any(|text| text == "turn 0")
            {
                runtime.shutdown();
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("the surviving runtime did not deliver its prompt");
    }

    #[test]
    fn opening_an_existing_pane_leaves_its_live_lane_intact() {
        let recorder = Arc::new(Recorder::default());
        let publisher: Arc<dyn AgentPublisher> = Arc::<Recorder>::clone(&recorder);
        let runtime = AgentRuntime::new(&publisher, AgentSpawnConfig::default(), None);
        runtime.set_runner_factory(Box::new(|spec| {
            fixture_runner(spec.provider, Behavior::Chunk, false, true)
        }));
        let pane = PaneId(45);
        let spec = AgentPaneSpec {
            provider: AgentProvider::Codex,
            cwd: PathBuf::from("/"),
            resume_session: None,
            workspace: crate::agent::environment::AgentWorkspaceEnvironment::default(),
        };

        assert!(runtime.open(pane, spec.clone()));
        let generation = runtime.fanout.lanes.lock()[&pane].generation;
        assert!(runtime.open(pane, spec));
        assert_eq!(runtime.host.pane_count(), 1);
        assert_eq!(runtime.fanout.lanes.lock()[&pane].generation, generation);
        assert!(runtime.prompt(
            pane,
            AgentPrompt {
                owner: ClientInstanceId::default(),
                text: "probe".to_owned(),
                images: Vec::new(),
            },
        ));
        let deadline = Instant::now() + DEADLINE;
        while Instant::now() < deadline {
            if recorder
                .payloads()
                .iter()
                .filter_map(text_of)
                .any(|text| text == "turn 0")
            {
                runtime.shutdown();
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("the original lane stopped delivering after a duplicate open");
    }

    #[test]
    fn restarting_a_lane_reclaims_prompts_from_the_retiring_runtime() {
        let fixture = Fixture::open(None, None);
        fixture.chunk("before");
        fixture.recorder.wait_for_items(1);

        fixture
            .fanout
            .restart_lane(fixture.pane, 2, AgentProvider::Codex, None);
        fixture.accept_generation(
            1,
            AgentStreamPayload::PromptsReclaimed {
                prompts: vec![AgentPrompt {
                    owner: ClientInstanceId::default(),
                    text: "keep me".to_owned(),
                    images: Vec::new(),
                }],
            },
        );
        fixture.accept_generation(
            1,
            AgentStreamPayload::Update {
                update: json!({ "sessionUpdate": "agent_message_chunk", "content": { "text": "stale" } }),
            },
        );
        fixture.accept_generation(
            2,
            AgentStreamPayload::Update {
                update: json!({ "sessionUpdate": "agent_message_chunk", "content": { "text": "after" } }),
            },
        );

        let items = fixture.recorder.wait_for_items(4);
        assert!(items.iter().any(|item| matches!(
            &item.payload,
            AgentStreamPayload::PromptsRestored { prompts, .. }
                if prompts.first().is_some_and(|prompt| prompt.text == "keep me")
        )));
        assert_eq!(
            items.iter().filter_map(text_of).collect::<Vec<_>>(),
            ["before", "after"]
        );
        assert_eq!(
            items.iter().map(|item| item.seq).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn restarting_a_lane_reclaims_a_near_limit_image() {
        let fixture = Fixture::open(None, None);
        fixture
            .fanout
            .restart_lane(fixture.pane, 2, AgentProvider::Codex, None);
        let image_bytes = zz_protocol::MAX_AGENT_PROMPT_BYTES - 1024;
        fixture.accept_generation(
            1,
            AgentStreamPayload::PromptsReclaimed {
                prompts: vec![AgentPrompt {
                    owner: ClientInstanceId::default(),
                    text: "keep the screenshot".to_owned(),
                    images: vec![crate::agent::stream::AgentImage {
                        format: "image/png".to_owned(),
                        data: vec![7; image_bytes],
                    }],
                }],
            },
        );

        let items = fixture.recorder.wait_for_items(1);
        assert!(items.iter().any(|item| matches!(
            &item.payload,
            AgentStreamPayload::PromptsRestored { prompts, .. }
                if prompts.first().is_some_and(|prompt|
                    prompt.text == "keep the screenshot"
                        && prompt.images.first().is_some_and(|image| image.data.len() == image_bytes)
                )
        )));
    }

    #[test]
    fn journal_replay_restores_the_latest_mode_and_live_state() {
        let fixture = Fixture::open(None, Some("s-1"));
        fixture.accept(AgentStreamPayload::SessionReady {
            session_id: "s-1".to_owned(),
            modes: Some(json!({
                "currentModeId": "plan",
                "availableModes": [
                    { "id": "plan", "name": "Plan" },
                    { "id": "ask", "name": "Ask" }
                ]
            })),
            config_options: None,
        });
        fixture.accept(AgentStreamPayload::Update {
            update: serde_json::to_value(SessionUpdate::CurrentModeUpdate(
                agent_client_protocol::schema::v1::CurrentModeUpdate::new("ask"),
            ))
            .expect("encode mode update"),
        });
        fixture.recorder.wait_for_items(2);
        {
            let mut lanes = fixture.fanout.lanes.lock();
            let lane = lanes.get_mut(&fixture.pane).expect("pane lane");
            lane.evicted_seq = lane.next_seq.saturating_sub(1);
        }

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
        let modes = replayed.iter().find_map(|item| match &item.payload {
            AgentStreamPayload::SessionReady {
                modes: Some(modes), ..
            } => Some(modes),
            _ => None,
        });
        assert_eq!(
            modes
                .and_then(|modes| modes.get("currentModeId"))
                .and_then(Value::as_str),
            Some("ask")
        );
        let state = replayed.last().and_then(|item| match &item.payload {
            AgentStreamPayload::StateSynced { state } => Some(state),
            _ => None,
        });
        assert!(matches!(
            state.map(|state| &state.phase),
            Some(zz_protocol::AgentConnectionPhase::Running)
        ));
        assert_eq!(
            state
                .and_then(|state| serde_json::from_str::<Value>(&state.modes).ok())
                .and_then(|modes| modes.get("currentModeId").cloned())
                .and_then(|mode| mode.as_str().map(ToOwned::to_owned))
                .as_deref(),
            Some("ask")
        );
    }

    #[test]
    fn journal_replay_restores_raw_config_option_updates() {
        use agent_client_protocol::schema::v1::{ConfigOptionUpdate, SessionConfigOption};

        let fixture = Fixture::open(None, Some("s-1"));
        fixture.accept(AgentStreamPayload::SessionReady {
            session_id: "s-1".to_owned(),
            modes: None,
            config_options: Some(
                serde_json::to_value(vec![SessionConfigOption::boolean(
                    "sandbox", "Sandbox", false,
                )])
                .expect("encode initial config"),
            ),
        });
        fixture.accept(AgentStreamPayload::Update {
            update: serde_json::to_value(SessionUpdate::ConfigOptionUpdate(
                ConfigOptionUpdate::new(vec![SessionConfigOption::boolean(
                    "sandbox", "Sandbox", true,
                )]),
            ))
            .expect("encode config update"),
        });
        fixture.recorder.wait_for_items(2);
        {
            let mut lanes = fixture.fanout.lanes.lock();
            let lane = lanes.get_mut(&fixture.pane).expect("pane lane");
            lane.evicted_seq = lane.next_seq.saturating_sub(1);
        }

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
        let config = replayed.iter().find_map(|item| match &item.payload {
            AgentStreamPayload::SessionReady {
                config_options: Some(config),
                ..
            } => Some(config),
            _ => None,
        });
        assert_eq!(
            config
                .and_then(Value::as_array)
                .and_then(|options| options.first())
                .and_then(|option| option.get("currentValue"))
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn session_replies_leave_the_stream_without_spending_a_sequence() {
        let fixture = Fixture::open(None, Some("s-1"));
        fixture.accept(AgentStreamPayload::SessionsListed {
            client: ClientId(8),
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
        fixture.accept(AgentStreamPayload::SessionDeleted {
            client: ClientId(10),
            session_id: "s-3".to_owned(),
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
        assert_eq!(replies[0].0, ClientId(8));
        assert_eq!(replies[0].2, 0);
        assert_eq!(replies[1].0, ClientId(10));
        assert!(replies[1].3.contains("s-3"));
    }

    #[test]
    fn an_oversized_session_page_returns_a_targeted_failure() {
        let fixture = Fixture::open(None, Some("s-1"));
        fixture.accept(AgentStreamPayload::SessionsListed {
            client: ClientId(8),
            sessions: (0..300)
                .map(|index| AgentSessionSummary {
                    session_id: format!("s-{index}"),
                    cwd: PathBuf::from("/work"),
                    additional_directories: Vec::new(),
                    title: Some("x".repeat(4 * 1024)),
                    updated_at: None,
                })
                .collect(),
            next_cursor: None,
            cwd_filter: None,
            replace: true,
        });

        let replies = fixture.recorder.replies.lock();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].0, ClientId(8));
        assert!(matches!(
            serde_json::from_str::<AgentStreamPayload>(&replies[0].3),
            Ok(AgentStreamPayload::SessionListFailed { client, message })
                if client == ClientId(8) && message.contains("too much session history")
        ));
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
    fn switching_provider_allows_the_new_conversation_to_name_the_pane() {
        let fixture = Fixture::open(None, None);
        assert!(
            fixture
                .fanout
                .propose_title(fixture.pane, "old provider turn")
                .is_some()
        );

        fixture
            .fanout
            .restart_lane(fixture.pane, 2, AgentProvider::ClaudeCode, None);
        assert_eq!(
            fixture
                .fanout
                .propose_title(fixture.pane, "new provider turn"),
            Some("new provider turn".to_owned())
        );

        fixture
            .fanout
            .restart_lane(fixture.pane, 3, AgentProvider::ClaudeCode, None);
        assert_eq!(
            fixture
                .fanout
                .propose_title(fixture.pane, "same provider retry"),
            None
        );
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
        fixture.fanout.accept(fixture.pane, 1, &running, None);
        running.git = Some(zz_protocol::AgentGitSummary {
            branch: Some("main".to_owned()),
            changed_files: 2,
            additions: 8,
            deletions: 3,
        });
        fixture.fanout.accept(fixture.pane, 1, &running, None);
        let states = fixture.recorder.states.lock();
        assert_eq!(states.len(), 3);
        assert_eq!(states[1].1.queued_prompts, 1);
        assert_eq!(states[2].1.git, running.git);
    }
}
