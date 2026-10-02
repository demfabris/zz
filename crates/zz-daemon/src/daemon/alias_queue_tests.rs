use super::*;

#[test]
fn three_level_alias_suspension_preserves_output_and_runs_an_unrelated_queue() {
    let shared = Arc::new(Shared::new(303));
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let (unrelated, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let mut context = ExecutionContext::default();
    for (index, alias) in [
        "level-three=display-message -p three-before ; wait-for alias-resume ; display-message -p three-after",
        "level-two=display-message -p two-before ; if-shell -F 1 'level-three' ; display-message -p two-after",
        "level-one=display-message -p one-before ; run-shell -C 'level-two' ; display-message -p one-after",
    ].into_iter().enumerate() {
        shared.execute(
            client, ClientKind::Command, &mut context,
            &CommandInvocation::new("set-option", ["-s".to_owned(), format!("command-alias[{}]", 100 + index), alias.to_owned()]),
        ).unwrap();
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || {
        let result = worker_shared.execute_command_request(
            client,
            ClientKind::Command,
            &mut context,
            1,
            &CommandInvocation::new("level-one", [] as [&str; 0]),
        );
        sender.send(result).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(response) = receiver.try_recv() {
            worker.join().unwrap();
            panic!("alias returned before suspension: {response:?}");
        }
        if shared
            .inner
            .lock()
            .wait_channels
            .get("alias-resume")
            .is_some_and(|channel| !channel.waiters.is_empty())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "three-level alias did not suspend"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let mut unrelated_context = ExecutionContext::default();
    let response = shared.execute_command_request(
        unrelated,
        ClientKind::Command,
        &mut unrelated_context,
        2,
        &CommandInvocation::new("display-message", ["-p", "unrelated"]),
    );
    assert!(matches!(response, CommandResponse::Success { output, .. } if output == "unrelated"));
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    shared
        .execute(
            unrelated,
            ClientKind::Command,
            &mut unrelated_context,
            &CommandInvocation::new("wait-for", ["-S", "alias-resume"]),
        )
        .unwrap();
    let response = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("alias completion");
    worker.join().unwrap();
    let CommandResponse::Success {
        output, exit_code, ..
    } = response
    else {
        panic!("alias response: {response:?}");
    };
    assert_eq!(exit_code, 0);
    let expected = [
        "one-before",
        "two-before",
        "three-before",
        "three-after",
        "two-after",
        "one-after",
    ];
    let output = output.to_string();
    let lines = output.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), expected.len());
    let reordered = lines
        .iter()
        .zip(expected)
        .filter(|(actual, expected)| **actual != *expected)
        .count();
    assert_eq!(reordered, 0, "reordered output lines: {output}");
}
