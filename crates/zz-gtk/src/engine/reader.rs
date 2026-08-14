use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

use async_channel::Sender;
use zz_client::{ClientCore, CoreEvent, Outbound};
use zz_daemon::InteractiveClient;
use zz_protocol::{BrowserCommand, CommandResponse, GuiResponse, ServerError, TerminalUiCommand};

use super::{AUTH_DECLINED_REASON, EngineEvent, HistoryChunk, HostState, Ladder, Link};

/// The first retry fires almost immediately — a daemon that was restarted under
/// the client is usually back before a human notices — and the ladder doubles up
/// to [`MAX_DELAY`] so a machine that is really gone is not hammered.
const FIRST_DELAY: Duration = Duration::from_millis(100);
const MAX_DELAY: Duration = Duration::from_secs(2);
/// How long the engine keeps the frozen frames on screen before giving up.
const RETRY_WINDOW: Duration = Duration::from_secs(30);
/// Retry naps are slept in slices so a closed UI is noticed promptly.
const NAP_SLICE: Duration = Duration::from_millis(50);

/// How many rungs a host nobody is watching climbs before it is left alone. The
/// desktop's number: a host on screen retries until it comes back, and one in a
/// collapsed corner of the tree stops asking and offers Reconnect instead.
const MAX_UNWATCHED_ATTEMPTS: u32 = 3;

/// The desktop's fleet ladder, in seconds.
fn host_delay(attempt: u32) -> Duration {
    Duration::from_secs(match attempt {
        0 | 1 => 1,
        2 => 2,
        3 => 4,
        4 => 8,
        5 => 16,
        _ => 30,
    })
}

/// Reduces one connection at a time and re-establishes the next one itself:
/// decoded messages in, wire requests straight back out, frames into the
/// coalescing inbox, everything else to the UI in stream order.
pub fn spawn(link: Arc<Link>, events: Sender<EngineEvent>) -> Result<(), String> {
    link.running.store(true, Ordering::Release);
    let guard = Arc::clone(&link);
    thread::Builder::new()
        .name("zz-gtk-protocol".to_owned())
        .spawn(move || {
            supervise(&link, &events);
            link.running.store(false, Ordering::Release);
        })
        .map(drop)
        .map_err(|error| {
            guard.running.store(false, Ordering::Release);
            format!("failed to start the protocol reader: {error}")
        })
}

fn supervise(link: &Link, events: &Sender<EngineEvent>) {
    if link.client().is_none() && !first_dial(link, events) {
        return;
    }
    while let Some(reason) = pump(link, events) {
        if link.is_closed() || !reconnect(link, events, reason) {
            return;
        }
    }
}

/// A host is dialled by its own reader thread rather than by whoever added it:
/// ssh can take seconds to answer, and the main loop cannot wait for that.
fn first_dial(link: &Link, events: &Sender<EngineEvent>) -> bool {
    match link.dial() {
        Ok(()) => publish_state(link, events, HostState::Connected),
        Err(reason) => reconnect(link, events, reason),
    }
}

/// Drain the live connection until it fails, returning why. `None` means the UI
/// hung up and the thread is done.
fn pump(link: &Link, events: &Sender<EngineEvent>) -> Option<String> {
    let client = link.client()?;
    loop {
        let message = match client.recv() {
            Ok(message) => message,
            Err(error) => return Some(error.to_string()),
        };
        if link.is_closed() {
            return None;
        }
        let forwarded = {
            let mut core = link.core();
            core.handle_message(message);
            while let Some(Outbound::RequestFull(pane)) = core.poll_outbound() {
                if let Err(error) = client.request_full(pane) {
                    log::warn!("zz-gtk failed to request a full viewport for {pane}: {error}");
                }
            }
            let mut forwarded = Vec::new();
            while let Some(event) = core.poll_event() {
                reduce(link, &core, &client, event, &mut forwarded);
            }
            forwarded
        };
        if forwarded
            .iter()
            .any(|event| matches!(event, EngineEvent::Attached(_)))
        {
            link.replay_geometry();
        }
        for event in forwarded {
            events.send_blocking(link.tag(event)).ok()?;
        }
    }
}

/// Retry the endpoint on a bounded ladder, keeping every accessor answering
/// with the state the dead connection left behind. True when a fresh connection
/// took over; false when the ladder gave up or the UI hung up.
fn reconnect(link: &Link, events: &Sender<EngineEvent>, reason: String) -> bool {
    log::warn!("zz-gtk lost the daemon connection: {reason}");
    match link.ladder {
        Ladder::Local => reconnect_local(link, events, reason),
        Ladder::Fleet => reconnect_host(link, events, reason),
    }
}

/// The local daemon: retry hard for half a minute, then close the window. There
/// is no workspace without it.
fn reconnect_local(link: &Link, events: &Sender<EngineEvent>, reason: String) -> bool {
    let started = Instant::now();
    let mut delay = FIRST_DELAY;
    let mut attempt = 1;
    let mut last = reason;
    while started.elapsed() < RETRY_WINDOW {
        if events
            .send_blocking(EngineEvent::Reconnecting { attempt })
            .is_err()
            || !nap(events, delay)
        {
            return false;
        }
        link.set_state(HostState::Reconnecting { attempt });
        match link.dial() {
            Ok(()) => {
                link.set_state(HostState::Connected);
                return [
                    EngineEvent::Reconnected,
                    EngineEvent::AppearanceChanged,
                    EngineEvent::StatusChanged,
                ]
                .into_iter()
                .all(|event| events.send_blocking(event).is_ok());
            }
            Err(error) => last = error,
        }
        delay = (delay * 2).min(MAX_DELAY);
        attempt += 1;
    }
    link.set_state(HostState::Unreachable {
        reason: last.clone(),
    });
    let _ = events.send_blocking(EngineEvent::Disconnected(last));
    false
}

/// A fleet host: the desktop's 1/2/4/8/16/30s ladder, forever while the host is
/// the one on screen and for three rungs while it is not. A host that stops
/// retrying keeps every frame it had — nothing here ever reaches for the local
/// daemon instead, because the panes on screen belong to a different machine.
fn reconnect_host(link: &Link, events: &Sender<EngineEvent>, reason: String) -> bool {
    let mut attempt = 1;
    let mut last = reason;
    loop {
        if link.parked.load(Ordering::Acquire) {
            publish_state(
                link,
                events,
                HostState::Parked {
                    reason: AUTH_DECLINED_REASON.to_owned(),
                },
            );
            return false;
        }
        if !link.is_active() && attempt > MAX_UNWATCHED_ATTEMPTS {
            publish_state(link, events, HostState::Unreachable { reason: last });
            return false;
        }
        if !publish_state(link, events, HostState::Reconnecting { attempt })
            || !nap(events, host_delay(attempt))
            || link.is_closed()
        {
            return false;
        }
        match link.dial() {
            Ok(()) => return publish_state(link, events, HostState::Connected),
            Err(error) => last = error,
        }
        attempt = attempt.saturating_add(1);
    }
}

/// Record a state and tell the UI, unless the UI is gone. False also when the
/// channel closed, which is every caller's cue to stop.
fn publish_state(link: &Link, events: &Sender<EngineEvent>, state: HostState) -> bool {
    let moved = link.set_state(state);
    let Some(host) = link.tag else {
        return !events.is_closed();
    };
    if !moved {
        return !events.is_closed();
    }
    events.send_blocking(EngineEvent::HostState(host)).is_ok()
}

/// False once the UI closed the event channel; the caller stops there.
fn nap(events: &Sender<EngineEvent>, delay: Duration) -> bool {
    let deadline = Instant::now() + delay;
    loop {
        if events.is_closed() {
            return false;
        }
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return true;
        };
        thread::sleep(left.min(NAP_SLICE));
    }
}

/// Event sequence gaps are never checked — the daemon supersedes stale frames
/// under backpressure, so a healthy stream skips numbers. A stopping daemon is
/// likewise not an exit: the socket drops right behind that event and the retry
/// ladder takes it from there.
fn reduce(
    link: &Link,
    core: &ClientCore,
    client: &InteractiveClient,
    event: CoreEvent,
    forwarded: &mut Vec<EngineEvent>,
) {
    match event {
        CoreEvent::ViewportChanged { pane, damage } => {
            if let Some(viewport) = core.viewport(pane)
                && link.frames.publish(pane, viewport.clone(), damage)
            {
                forwarded.push(EngineEvent::FramesReady);
            }
        }
        CoreEvent::Attached { session } => {
            link.remembered_reattach.store(false, Ordering::Relaxed);
            link.frames.clear();
            link.clear_history();
            forwarded.push(EngineEvent::Attached(session));
        }
        CoreEvent::PaneRemoved { pane } => {
            link.frames.forget(pane);
            link.forget_history(pane);
            forwarded.push(EngineEvent::SnapshotChanged);
        }
        CoreEvent::HistoryChunk {
            pane,
            start,
            total,
            offset,
            columns,
            rows,
            dictionary,
        } => {
            let chunk = HistoryChunk {
                start,
                total,
                offset,
                columns,
                rows,
                dictionary,
            };
            let absorbed = core
                .viewport(pane)
                .is_some_and(|viewport| link.absorb_history(pane, chunk, viewport));
            if let Some((start, count)) = link.next_history_request(pane)
                && let Err(error) = client.request_history(pane, start, count)
            {
                log::warn!("zz-gtk failed to continue the scrollback walk for {pane}: {error}");
            }
            if absorbed {
                forwarded.push(EngineEvent::HistoryChanged(pane));
            }
        }
        CoreEvent::CommandOutputChanged => forwarded.push(EngineEvent::CommandOutputChanged),
        CoreEvent::TerminalUiCommand {
            pane,
            command: TerminalUiCommand::BeginSearch { direction },
        } => forwarded.push(EngineEvent::BeginSearch { pane, direction }),
        CoreEvent::OpenUri { pane, uri } => forwarded.push(EngineEvent::OpenUri { pane, uri }),
        CoreEvent::SnapshotChanged => forwarded.push(EngineEvent::SnapshotChanged),
        CoreEvent::StatusChanged => forwarded.push(EngineEvent::StatusChanged),
        CoreEvent::AppearanceChanged => forwarded.push(EngineEvent::AppearanceChanged),
        CoreEvent::FocusSidebar => forwarded.push(EngineEvent::FocusSidebar),
        CoreEvent::MuxOptionsChanged => forwarded.push(EngineEvent::MuxOptionsChanged),
        CoreEvent::PrefixArmed { .. }
        | CoreEvent::CommandPromptChanged
        | CoreEvent::ChooseTreeChanged
        | CoreEvent::ChooseBufferChanged
        | CoreEvent::DisplayPanesChanged => forwarded.push(EngineEvent::OverlaysChanged),
        CoreEvent::Clipboard { target, text, .. } => {
            forwarded.push(EngineEvent::Clipboard { target, text });
        }
        CoreEvent::ClientMessage { text, .. } => forwarded.push(EngineEvent::Notice(text)),
        CoreEvent::CommandResponse(CommandResponse::Error { error, .. }) => {
            if matches!(error, ServerError::MissingTarget(_)) && link.retry_default_attach(client) {
                return;
            }
            forwarded.push(EngineEvent::Notice(error.to_string()));
        }
        CoreEvent::Detached { .. } => forwarded.push(EngineEvent::Detached),
        CoreEvent::ServerStopping => {
            forwarded.push(EngineEvent::Notice("the zz daemon stopped".to_owned()));
        }
        CoreEvent::AgentCommand { request_id, .. } => {
            reject_gui_request(client, request_id, "agent commands require the zz app");
        }
        CoreEvent::BrowserCommand {
            command: BrowserCommand::Screenshot { request_id, .. },
            ..
        } => reject_gui_request(client, request_id, "browser panes require the zz app"),
        _ => {}
    }
}

fn reject_gui_request(client: &InteractiveClient, request_id: u64, message: &str) {
    if let Err(error) = client.send_gui_response(GuiResponse::Error {
        request_id,
        message: message.to_owned(),
    }) {
        log::warn!("zz-gtk failed to answer a GUI request: {error}");
    }
}
