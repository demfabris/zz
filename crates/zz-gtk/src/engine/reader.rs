use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

use async_channel::Sender;
use zz_client::{ClientCore, CoreEvent, Outbound};
use zz_daemon::InteractiveClient;
use zz_protocol::{BrowserCommand, CommandResponse, GuiResponse, ServerError};

use super::{EngineEvent, Link};

/// The first retry fires almost immediately — a daemon that was restarted under
/// the client is usually back before a human notices — and the ladder doubles up
/// to [`MAX_DELAY`] so a machine that is really gone is not hammered.
const FIRST_DELAY: Duration = Duration::from_millis(100);
const MAX_DELAY: Duration = Duration::from_secs(2);
/// How long the engine keeps the frozen frames on screen before giving up.
const RETRY_WINDOW: Duration = Duration::from_secs(30);
/// Retry naps are slept in slices so a closed UI is noticed promptly.
const NAP_SLICE: Duration = Duration::from_millis(50);

/// Reduces one connection at a time and re-establishes the next one itself:
/// decoded messages in, wire requests straight back out, frames into the
/// coalescing inbox, everything else to the UI in stream order.
pub fn spawn(link: Arc<Link>, events: Sender<EngineEvent>) -> Result<(), String> {
    thread::Builder::new()
        .name("zz-gtk-protocol".to_owned())
        .spawn(move || supervise(&link, &events))
        .map(drop)
        .map_err(|error| format!("failed to start the protocol reader: {error}"))
}

fn supervise(link: &Link, events: &Sender<EngineEvent>) {
    while let Some(reason) = pump(link, events) {
        if !reconnect(link, events, reason) {
            return;
        }
    }
}

/// Drain the live connection until it fails, returning why. `None` means the UI
/// hung up and the thread is done.
fn pump(link: &Link, events: &Sender<EngineEvent>) -> Option<String> {
    let client = link.client();
    loop {
        let message = match client.recv() {
            Ok(message) => message,
            Err(error) => return Some(error.to_string()),
        };
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
            events.send_blocking(event).ok()?;
        }
    }
}

/// Retry the endpoint on a bounded ladder, keeping every accessor answering
/// with the state the dead connection left behind. True when a fresh connection
/// took over; false when the window elapsed or the UI hung up.
fn reconnect(link: &Link, events: &Sender<EngineEvent>, reason: String) -> bool {
    log::warn!("zz-gtk lost the daemon connection: {reason}");
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
        match link.dial() {
            Ok(()) => {
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
    let _ = events.send_blocking(EngineEvent::Disconnected(last));
    false
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
            forwarded.push(EngineEvent::Attached(session));
        }
        CoreEvent::PaneRemoved { pane } => {
            link.frames.forget(pane);
            forwarded.push(EngineEvent::SnapshotChanged);
        }
        CoreEvent::SnapshotChanged => forwarded.push(EngineEvent::SnapshotChanged),
        CoreEvent::StatusChanged => forwarded.push(EngineEvent::StatusChanged),
        CoreEvent::AppearanceChanged => forwarded.push(EngineEvent::AppearanceChanged),
        CoreEvent::FocusSidebar => forwarded.push(EngineEvent::FocusSidebar),
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
