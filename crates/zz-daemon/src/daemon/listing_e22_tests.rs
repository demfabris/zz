use super::*;

#[test]
fn uncached_listing_parks_between_rows_and_resumes_its_command_queue() {
    let shared = Arc::new(Shared::new(522));
    let (client, _) =
        shared.register_subscribed(ClientKind::Command, None, None, OutboundMailbox::new());
    let context = ExecutionContext::default();
    let command = CommandInvocation::new(
        "list-keys",
        ["-F", "#{key_table} #{key_string} #{key_command}"],
    );
    let mut task = wait_queue::CommandTask::new(
        &shared,
        client,
        ClientKind::Command,
        &context,
        522,
        &command,
        false,
    )
    .unwrap_or_else(|_| panic!("listing task"));
    assert!(matches!(task.run(true), wait_queue::Progress::Waiting));
    assert!(!task.ready());
    shared.terminal_requests.turn(&shared);
    assert!(!task.ready());
    let mut turns = 1;
    while !task.ready() {
        assert!(turns < 20);
        shared.terminal_requests.turn(&shared);
        turns += 1;
    }
    assert!(turns > 1);
    assert!(matches!(task.run(true), wait_queue::Progress::Done));
    let response = task.finish().0;
    assert!(matches!(response, CommandResponse::Success { .. }));
    shared.request_shutdown();
}
