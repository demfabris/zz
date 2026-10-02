use super::*;
use terminal_requests::{CommandState, CommandWait, ReplyOutcome};

pub(super) fn reply<T: Send + 'static>(
    shared: &Arc<Shared>,
    timeout: Option<Duration>,
    finish: impl FnOnce(&Arc<Shared>, ReplyOutcome<T>) -> Result<Execution, DaemonError>
    + Send
    + 'static,
) -> (CommandWait, cmdq::Reply<T>) {
    let wait = CommandWait::new(shared);
    let state = wait.start();
    let reply = shared.terminal_requests.reply(
        timeout.map(|timeout| Instant::now() + timeout),
        state.continuation.clone(),
        move |shared, result| {
            let result = finish(shared, result);
            if !state.continuation.ready() {
                state.resolve(result);
            }
        },
    );
    (wait, reply)
}

pub(super) struct TerminalWait {
    client: ClientId,
    pane: PaneId,
    state: Arc<CommandState>,
    parsed: ParsedAgentSend,
    progress: Mutex<TerminalProgress>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

struct TerminalProgress {
    started: Option<Instant>,
    seen_working: bool,
    result: Option<Result<&'static str, DaemonError>>,
}

impl TerminalWait {
    pub(super) fn subscribe(
        shared: &Arc<Shared>,
        client: ClientId,
        pane: PaneId,
        parsed: &ParsedAgentSend,
        state: Arc<CommandState>,
        initial: &str,
    ) -> Arc<Self> {
        let wait = Arc::new(Self {
            client,
            pane,
            state,
            parsed: parsed.clone(),
            progress: Mutex::new(TerminalProgress {
                started: None,
                seen_working: initial != "idle",
                result: None,
            }),
            notify: shared.terminal_requests.notifier(),
        });
        shared
            .agent_state_waits
            .lock()
            .entry(pane)
            .or_default()
            .push(Arc::downgrade(&wait));
        shared
            .terminal_requests
            .push(TerminalRequest(Arc::clone(&wait)));
        wait
    }

    pub(super) fn submitted(&self, result: Result<(), DaemonError>) {
        let mut progress = self.progress.lock();
        progress.started = Some(Instant::now());
        if let Err(error) = result {
            progress.result = Some(Err(error));
        }
        drop(progress);
        (self.notify)();
    }

    fn observe(&self, value: &str) {
        let mut progress = self.progress.lock();
        if progress.result.is_some() {
            return;
        }
        match value {
            "idle" if progress.seen_working => progress.result = Some(Ok("end_turn")),
            "blocked" if self.parsed.on_block == AgentBlockPolicy::Fail => {
                progress.result = Some(Ok("blocked"));
            }
            "failed" => progress.result = Some(Ok("failed")),
            "" | "idle" => {}
            _ => progress.seen_working = true,
        }
        drop(progress);
        (self.notify)();
    }
}

struct TerminalRequest(Arc<TerminalWait>);

impl terminal_requests::Pending for TerminalRequest {
    fn poll(&mut self, shared: &Arc<Shared>, now: Instant) -> bool {
        let wait = &self.0;
        let mut progress = wait.progress.lock();
        let started = progress.started.unwrap_or(now);
        let result = if wait.state.continuation.ready()
            || shared.command_queue_cancelled(wait.client)
            || shared.stopping.load(Ordering::Acquire)
        {
            Some(Ok(Execution::default()))
        } else if shared.inner.lock().engine.state.pane(wait.pane).is_none() {
            Some(Err(ServerError::PaneExited(wait.pane).into()))
        } else if let Some(error) = wait.state.take_failure() {
            Some(Err(error))
        } else if progress.started.is_none() {
            None
        } else if let Some(result) = progress.result.take() {
            Some(result.and_then(|reason| match reason {
                "end_turn" => {
                    shared.record_command_stderr(wait.client, &wait.pane.to_string());
                    Ok(Execution {
                        output: agent_terminal_reply_output(
                            wait.pane,
                            "",
                            started.elapsed(),
                            reason,
                            &wait.parsed,
                        )
                        .into(),
                        effects: Vec::new(),
                    })
                }
                "blocked" => {
                    shared.record_command_stderr(wait.client, &wait.pane.to_string());
                    Err(DaemonError::CommandExit {
                        output: if wait.parsed.json {
                            agent_terminal_reply_output(
                                wait.pane,
                                "",
                                started.elapsed(),
                                reason,
                                &wait.parsed,
                            )
                        } else {
                            format!("{}: blocked (agent_state=blocked)", wait.pane)
                        }
                        .into(),
                        exit_code: 3,
                    })
                }
                _ => Err(DaemonError::CommandExit {
                    output: format!("{}: agent_state=failed", wait.pane).into(),
                    exit_code: 1,
                }),
            }))
        } else if wait
            .parsed
            .wait_timeout()
            .is_some_and(|timeout| now >= started + timeout)
        {
            Some(Err(DaemonError::CommandExit {
                output: format!(
                    "{}: no idle state within {} seconds; the turn may still be running",
                    wait.pane,
                    wait.parsed.wait_timeout().unwrap().as_secs()
                )
                .into(),
                exit_code: 124,
            }))
        } else if !progress.seen_working && now >= started + AGENT_STATE_START_GRACE {
            Some(Err(DaemonError::CommandExit {
                output: format!(
                    "{}: agent_state stayed idle for {} seconds after the send",
                    wait.pane,
                    AGENT_STATE_START_GRACE.as_secs()
                )
                .into(),
                exit_code: 124,
            }))
        } else {
            None
        };
        drop(progress);
        let Some(result) = result else {
            return false;
        };
        wait.state.resolve(result);
        let mut subscriptions = shared.agent_state_waits.lock();
        if let Some(waits) = subscriptions.get_mut(&wait.pane) {
            waits.retain(|current| {
                current
                    .upgrade()
                    .is_some_and(|current| !Arc::ptr_eq(&current, wait))
            });
            if waits.is_empty() {
                subscriptions.remove(&wait.pane);
            }
        }
        true
    }

    fn next(&self) -> Option<Instant> {
        let progress = self.0.progress.lock();
        let started = progress.started?;
        self.0
            .parsed
            .wait_timeout()
            .map(|timeout| started + timeout)
            .into_iter()
            .chain((!progress.seen_working).then_some(started + AGENT_STATE_START_GRACE))
            .min()
    }
}

pub(super) fn state_changed(shared: &Arc<Shared>, pane: PaneId) {
    let waits = shared
        .agent_state_waits
        .lock()
        .get(&pane)
        .cloned()
        .unwrap_or_default();
    if waits.is_empty() {
        return;
    }
    let value = shared.pane_agent_state(pane);
    for wait in waits.into_iter().filter_map(|wait| wait.upgrade()) {
        wait.observe(&value);
    }
}

#[cfg(all(feature = "agent", unix))]
pub(super) fn watch_peer(
    shared: &Arc<Shared>,
    pid: u32,
    reply: Weak<cmdq::Reply<Result<String, String>>>,
    continuation: cmdq::WaitContinuation,
) {
    shared
        .terminal_requests
        .schedule(Instant::now() + Duration::from_millis(100), move |shared| {
            if continuation.ready() {
                return;
            }
            let Some(sender) = reply.upgrade() else {
                return;
            };
            if crate::agent::claude_peers::pid_alive(pid) {
                watch_peer(shared, pid, reply, continuation);
            } else {
                sender.close();
            }
        });
}

#[cfg(all(test, unix))]
#[path = "agent_waits_tests.rs"]
mod tests;
