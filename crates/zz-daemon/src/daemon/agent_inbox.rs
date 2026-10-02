use super::agent_publisher::{Message, Payload};
use super::*;

const DRAIN_BURST: usize = 128;

pub(super) struct AgentInbox {
    pub(super) receiver: Option<crossbeam_channel::Receiver<Message>>,
    hooks: VecDeque<PendingHookEvent>,
}

impl AgentInbox {
    pub(super) fn new(shared: &Shared) -> Self {
        Self {
            receiver: shared.agent_rx.lock().take(),
            hooks: VecDeque::new(),
        }
    }

    pub(super) fn turn(&mut self, shared: &Arc<Shared>) {
        if shared.agent_tx.pending.swap(false, Ordering::AcqRel)
            && let Some(receiver) = self.receiver.clone()
        {
            for message in receiver.try_iter().take(DRAIN_BURST) {
                self.apply(shared, message);
            }
            if !receiver.is_empty() {
                shared.agent_tx.notify_loop();
            }
        }
        if !self.hooks.is_empty() {
            shared.enqueue_event_hooks(self.hooks.drain(..).collect());
        }
    }

    pub(super) fn apply(&mut self, shared: &Arc<Shared>, message: Message) {
        let event = {
            let _effects = shared.agent_effects.lock();
            if shared.agent_stopped.load(Ordering::Acquire)
                || shared.agent_tx.incarnation.load(Ordering::Acquire) != message.incarnation
                || shared.open_agent_runtime().is_none_or(|runtime| {
                    runtime.pane_generation(message.pane) != Some(message.generation)
                })
                || !shared
                    .inner
                    .lock()
                    .engine
                    .state
                    .pane(message.pane)
                    .is_some_and(|pane| matches!(pane.kind, PaneKind::Agent(_)))
            {
                return;
            }
            let pane = message.pane;
            match message.payload {
                Payload::Barrier(reply) => {
                    let _ = reply.try_send(());
                    None
                }
                Payload::Updates {
                    first_seq,
                    items,
                    also,
                } => {
                    shared.publish_agent_updates(pane, first_seq, items, also);
                    None
                }
                Payload::Replay { client, frames } => {
                    shared.send_agent_replay(client, pane, frames);
                    None
                }
                Payload::BroadcastReplay { frames, also } => {
                    shared.publish_agent_replay(pane, frames, also);
                    None
                }
                Payload::State(state) => shared.apply_agent_state(pane, state),
                Payload::ToolCall(call) => shared.agent_tool_call_event(pane, call),
                Payload::Reply(reply) => {
                    shared.send_agent_reply(pane, reply);
                    None
                }
                Payload::Session {
                    provider,
                    session_id,
                    cwd,
                } => {
                    shared.adopt_agent_session(pane, provider, session_id, cwd);
                    None
                }
                Payload::Title(title) => {
                    shared.title_agent_pane(pane, title);
                    None
                }
                Payload::Text(bytes) => {
                    shared.feed_agent_pane_text(pane, bytes);
                    None
                }
            }
        };
        if let Some(event) = event {
            self.hooks.push_back(event);
        }
    }
}

#[cfg(test)]
#[path = "agent_inbox_tests.rs"]
mod tests;
