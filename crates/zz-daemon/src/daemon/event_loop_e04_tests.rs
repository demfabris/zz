use super::*;

#[test]
fn formatted_split_wait_resumes_its_pane_wait_on_the_loop() {
    let shared = Arc::new(Shared::new(408));
    let mailbox = OutboundMailbox::new();
    let (client, _) = shared.register_subscribed(ClientKind::Command, None, None, mailbox);
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "e04wait", "exec sleep 30"]),
        )
        .unwrap();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let mut task = wait_queue::CommandTask::new(
        &shared,
        client,
        ClientKind::Command,
        &context,
        408,
        &CommandInvocation::new(
            "split-window",
            [
                "-d",
                "-P",
                "-F",
                "#{pane_index}",
                "-W",
                "-t",
                "e04wait:",
                "sleep 0.2; exit 5",
            ],
        ),
        false,
    )
    .unwrap_or_else(|_| panic!("split wait task"));
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !task.ready() {
        assert!(Instant::now() < deadline);
        event_loop.turn(&shared).unwrap();
        let mut query = context.clone();
        let output = shared
            .execute(
                client,
                ClientKind::Command,
                &mut query,
                &CommandInvocation::new("display-message", ["-p", "responsive"]),
            )
            .unwrap()
            .output;
        assert_eq!(output, "responsive");
        thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    let (response, _, _, _) = task.finish();
    assert!(
        matches!(response, CommandResponse::Success { output, exit_code: 5, .. } if output == "1\n")
    );
    shared.request_shutdown();
}

#[test]
fn formatted_hook_split_wait_keeps_the_loop_running() {
    let shared = Arc::new(Shared::new(409));
    let mailbox = OutboundMailbox::new();
    let (client, _) = shared.register_subscribed(ClientKind::Command, None, None, mailbox);
    let mut context = ExecutionContext::default();
    shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("new-session", ["-d", "-s", "e04hook", "sleep 30"]),
        )
        .unwrap();
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let mut hooks = hook_queue::LoopHooks::new();
    hooks.monitor(
        &shared,
        context.clone(),
        vec![vec![
            CommandInvocation::new(
                "split-window",
                ["-d", "-P", "-W", "-t", "e04hook:", "sleep 0.2"],
            ),
            CommandInvocation::new("set-option", ["-g", "@e04after", "completed"]),
        ]],
        BTreeMap::new(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !hooks.is_empty() {
        assert!(Instant::now() < deadline);
        hooks.turn(&shared, &event_loop.waker).unwrap();
        event_loop.turn(&shared).unwrap();
        thread::sleep(Duration::from_millis(1));
    }
    let output = shared
        .execute(
            client,
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("show-options", ["-gqv", "@e04after"]),
        )
        .unwrap()
        .output;
    assert_eq!(output, "completed");
    shared.request_shutdown();
}
