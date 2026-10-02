use super::super::agent_publisher::Publisher;
use super::*;

fn fixture() -> (Arc<Shared>, AgentInbox, PaneId) {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    {
        let mut inner = shared.inner.lock();
        for command in [
            CommandInvocation::new("new-session", ["-s", "agent-inbox"]),
            CommandInvocation::new("set-option", ["-g", "experimental-agent-pane", "on"]),
            CommandInvocation::new("split-window", ["--kind", "picker"]),
            CommandInvocation::new("select-pane-kind", ["agent"]),
        ] {
            inner
                .engine
                .execute(&mut context, &command)
                .expect("mux setup");
        }
    }
    let inbox = AgentInbox::new(&shared);
    (shared, inbox, context.pane.expect("agent pane"))
}

fn runtime(shared: &Arc<Shared>, pane: PaneId) -> (Publisher, Arc<AgentRuntime>, u64) {
    let publisher = shared.agent_tx.publisher();
    let handle: Arc<dyn AgentPublisher> = Arc::new(publisher.clone());
    let runtime = Arc::new(AgentRuntime::new(
        &handle,
        AgentSpawnConfig::default(),
        None,
    ));
    runtime.set_runner_factory(Box::new(|_| Box::new(|_| Box::pin(std::future::pending()))));
    assert!(runtime.open(pane, shared.agent_pane_spec(pane).expect("pane spec")));
    let generation = runtime.pane_generation(pane).expect("generation");
    *shared.agent.lock() = Some(Arc::clone(&runtime));
    (publisher, runtime, generation)
}

#[test]
fn retired_runtime_incarnation_messages_are_dropped() {
    let (shared, mut inbox, pane) = fixture();
    let (old, retired, generation) = runtime(&shared, pane);
    {
        let _inner = shared.inner.lock();
        old.publish_agent_state(
            generation,
            pane,
            AgentPaneWire {
                phase: zz_protocol::AgentConnectionPhase::Running,
                title: Some("retired state".into()),
                ..AgentPaneWire::default()
            },
        );
        old.title_agent_pane(generation, pane, "retired title".into());
        old.adopt_agent_session(
            generation,
            pane,
            AgentProvider::Codex,
            "retired-session".into(),
            None,
        );
    }
    let (current, live, generation) = runtime(&shared, pane);
    retired.shutdown();
    inbox.turn(&shared).expect("agent inbox turn");
    {
        let inner = shared.inner.lock();
        assert_ne!(
            inner.engine.state.pane(pane).unwrap().title,
            "retired title"
        );
        let PaneKind::Agent(agent) = &inner.engine.state.pane(pane).unwrap().kind else {
            panic!("agent pane");
        };
        assert_ne!(agent.session_id.as_deref(), Some("retired-session"));
        assert_ne!(
            inner
                .agent_states
                .get(&pane)
                .and_then(|state| state.title.as_deref()),
            Some("retired state")
        );
    }
    current.publish_agent_state(
        generation,
        pane,
        AgentPaneWire {
            phase: zz_protocol::AgentConnectionPhase::Running,
            title: Some("current state".into()),
            ..AgentPaneWire::default()
        },
    );
    inbox.turn(&shared).expect("agent inbox turn");
    assert_eq!(
        shared.inner.lock().agent_states[&pane].title.as_deref(),
        Some("current state")
    );
    live.shutdown();
}

#[test]
fn restarted_pane_generation_messages_are_dropped() {
    let (shared, mut inbox, pane) = fixture();
    let (publisher, runtime, old_generation) = runtime(&shared, pane);
    publisher.title_agent_pane(old_generation, pane, "retired title".into());
    publisher.publish_agent_state(
        old_generation,
        pane,
        AgentPaneWire {
            title: Some("retired state".into()),
            ..AgentPaneWire::default()
        },
    );
    assert!(runtime.restart(pane, shared.agent_pane_spec(pane).unwrap()));
    let generation = runtime.pane_generation(pane).unwrap();
    assert_ne!(generation, old_generation);
    inbox.turn(&shared).expect("agent inbox turn");
    assert_ne!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "retired title"
    );
    assert_ne!(
        shared
            .inner
            .lock()
            .agent_states
            .get(&pane)
            .and_then(|state| state.title.as_deref()),
        Some("retired state")
    );
    publisher.title_agent_pane(generation, pane, "current title".into());
    inbox.turn(&shared).expect("agent inbox turn");
    assert_eq!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "current title"
    );
    runtime.shutdown();
}

#[test]
fn agent_inbox_bounds_each_turn_and_keeps_the_next_wake() {
    let (shared, mut inbox, pane) = fixture();
    let (publisher, runtime, generation) = runtime(&shared, pane);
    inbox.turn(&shared).expect("agent inbox turn");
    for index in 0..=DRAIN_BURST {
        publisher.publish_agent_state(
            generation,
            pane,
            AgentPaneWire {
                title: Some(index.to_string()),
                ..AgentPaneWire::default()
            },
        );
    }
    inbox.turn(&shared).expect("agent inbox turn");
    assert_eq!(
        shared.inner.lock().agent_states[&pane].title.as_deref(),
        Some((DRAIN_BURST - 1).to_string().as_str())
    );
    assert!(shared.agent_tx.pending.load(Ordering::Acquire));
    inbox.turn(&shared).expect("agent inbox turn");
    assert_eq!(
        shared.inner.lock().agent_states[&pane].title.as_deref(),
        Some(DRAIN_BURST.to_string().as_str())
    );
    assert!(!shared.agent_tx.pending.load(Ordering::Acquire));
    runtime.shutdown();
}

#[test]
fn a_shell_hook_keeps_the_agent_inbox_running() {
    let (shared, mut inbox, pane) = fixture();
    let (publisher, runtime, generation) = runtime(&shared, pane);
    shared
        .execute(
            ClientId(1),
            ClientKind::Command,
            &mut ExecutionContext::default(),
            &CommandInvocation::new(
                "set-hook",
                ["-g", "pane-mode-changed", "run-shell 'sleep 0.1'"],
            ),
        )
        .expect("parked hook");
    inbox.hooks.push_back(
        PendingHookEvent::live_pane("pane-mode-changed", pane, &shared.inner.lock().engine)
            .expect("live hook pane"),
    );
    inbox.turn(&shared).expect("agent state turn");
    publisher.title_agent_pane(generation, pane, "while hook waits".into());
    assert!(inbox.hook_running);
    inbox.turn(&shared).expect("inbox turn while hook runs");
    assert_eq!(
        shared.inner.lock().engine.state.pane(pane).unwrap().title,
        "while hook waits"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while inbox.hook_running {
        assert!(Instant::now() < deadline, "hook did not finish");
        inbox.turn(&shared).expect("hook completion turn");
        thread::sleep(Duration::from_millis(1));
    }
    runtime.shutdown();
}
