use super::*;

fn pane(shared: &Arc<Shared>) -> PaneId {
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "e04", ""]),
        )
        .unwrap();
    context.pane.unwrap()
}

fn queued(shared: &Arc<Shared>) -> Arc<Shared> {
    let item = shared.command_item(None);
    item.command_item.as_ref().unwrap().lock().loop_wait = true;
    item
}

fn drain(shared: &Arc<Shared>, state: &CommandState) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !state.continuation.ready() {
        assert!(Instant::now() < deadline);
        shared.terminal_requests.turn(shared);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn identity_reply_from_a_respawned_pane_is_rejected() {
    let shared = Arc::new(Shared::new(404));
    let pane = pane(&shared);
    let old = Arc::clone(&shared.inner.lock().terminals[&pane]);
    let item = queued(&shared);
    let wait = CommandWait::new(&item);
    let state = Arc::clone(&wait.state);
    {
        let _round_trips = zz_terminal::forbid_actor_round_trips();
        wait.pane_output(
            &item,
            pane,
            old,
            "#{pane_pid}".to_owned(),
            None,
            FormatClient::NoClient,
            BTreeMap::new(),
            "new-window".to_owned(),
        );
        assert!(!state.continuation.ready());
    }
    let mut context = ExecutionContext::for_pane(&shared.inner.lock().engine.state, pane).unwrap();
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("respawn-pane", ["-k", "-t", &pane.to_string(), ""]),
        )
        .unwrap();
    wait.finish(&item, Execution::default()).unwrap();
    drain(&shared, &state);
    let mut result = Ok(Execution::default());
    state.apply(&mut result);
    assert!(matches!(result, Err(DaemonError::Server(ServerError::PaneExited(id))) if id == pane));
    shared.request_shutdown();
}

#[test]
fn copy_source_and_settle_park_without_an_execution_worker_wait() {
    let shared = Arc::new(Shared::new(405));
    let pane = pane(&shared);
    let target = Arc::clone(&shared.inner.lock().terminals[&pane]);
    let source = Arc::new(TerminalSession::spawn_output_view(
        "source".to_owned(),
        "source text".to_owned(),
    ));
    let item = queued(&shared);
    let wait = CommandWait::new(&item);
    let state = Arc::clone(&wait.state);
    {
        let _round_trips = zz_terminal::forbid_actor_round_trips();
        wait.effects(
            &item,
            vec![
                DeferredTerminalCommand::ArmCopySource {
                    terminal: Arc::clone(&target),
                    source,
                },
                DeferredTerminalCommand::ViewAction {
                    terminal: target,
                    view: TerminalViewId(405),
                    action: zz_terminal::TerminalViewAction::EnterCopyMode,
                },
            ],
        );
        assert!(!shared.terminal_requests.receiver.is_empty());
        assert!(!state.continuation.ready());
        wait.finish(&item, Execution::default()).unwrap();
    }
    drain(&shared, &state);
    let mut result = Ok(Execution::default());
    state.apply(&mut result);
    assert!(result.is_ok());
    shared.request_shutdown();
}

#[test]
fn replaced_copy_destination_discards_the_remaining_effects() {
    let shared = Arc::new(Shared::new(406));
    let pane = pane(&shared);
    let old = Arc::clone(&shared.inner.lock().terminals[&pane]);
    let item = queued(&shared);
    let wait = CommandWait::new(&item);
    let state = Arc::clone(&wait.state);
    wait.effects(
        &item,
        vec![
            DeferredTerminalCommand::ArmCopySource {
                terminal: Arc::clone(&old),
                source: Arc::new(TerminalSession::spawn_output_view(
                    "source".to_owned(),
                    "captured".to_owned(),
                )),
            },
            DeferredTerminalCommand::SetWrapSearch {
                terminal: old,
                enabled: true,
            },
        ],
    );
    Arc::make_mut(&mut shared.inner.lock().terminals).insert(
        pane,
        Arc::new(TerminalSession::spawn_output_view(
            "replacement".to_owned(),
            String::new(),
        )),
    );
    wait.finish(&item, Execution::default()).unwrap();
    drain(&shared, &state);
    assert!(shared.delivered_wrap_search_commands.lock().is_empty());
    shared.request_shutdown();
}
