use super::tests::{key_table_fixture, test_key};
use super::*;

fn run(
    shared: &Arc<Shared>,
    client: ClientId,
    context: &mut ExecutionContext,
    args: &[&str],
) -> Result<Execution, DaemonError> {
    shared.execute(
        client,
        ClientKind::Command,
        context,
        &CommandInvocation::new(args[0], args[1..].iter().copied()),
    )
}

fn letter(character: char) -> zz_terminal::KeyInput {
    test_key(
        zz_terminal::KeyCode::Character(character),
        zz_terminal::Modifiers::default(),
        Some(&character.to_string()),
    )
}

fn press(shared: &Arc<Shared>, client: ClientId, pane: PaneId, input: zz_terminal::KeyInput) {
    shared
        .input(
            client,
            ClientKind::Interactive,
            &mut ExecutionContext::default(),
            InputMessage::Key {
                pane,
                input,
                text_follows: false,
            },
        )
        .expect("key input");
}

fn wait_for_screen(
    shared: &Arc<Shared>,
    client: ClientId,
    context: &mut ExecutionContext,
    target: &str,
    accept: impl Fn(&str) -> bool,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let screen = run(
            shared,
            client,
            context,
            &["capture-pane", "-p", "-t", target],
        )
        .expect("capture-pane")
        .output;
        if accept(&screen) {
            return screen.to_string();
        }
        assert!(Instant::now() < deadline, "{screen:?}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_unbound_letter_takes_the_short_path_to_the_pane() {
    let (shared, client, mut context, pane, _) = key_table_fixture("plain");
    assert!(
        shared
            .plain_key_generation(client, pane, &letter('q'))
            .is_some()
    );
    press(&shared, client, pane, letter('q'));
    press(&shared, client, pane, letter('w'));
    wait_for_screen(&shared, client, &mut context, "plain", |screen| {
        screen.contains("qw")
    });
}

#[test]
fn bindings_prefixes_and_releases_take_the_full_path() {
    let (shared, client, mut context, pane, _) = key_table_fixture("bound");
    run(
        &shared,
        client,
        &mut context,
        &["bind-key", "-n", "z", "display-message", "zed"],
    )
    .expect("bind");
    assert!(
        shared
            .plain_key_generation(client, pane, &letter('z'))
            .is_none()
    );
    assert!(
        shared
            .plain_key_generation(client, pane, &letter('a'))
            .is_some()
    );
    let prefix = test_key(
        zz_terminal::KeyCode::Character('b'),
        zz_terminal::Modifiers::new(false, true, false, false),
        None,
    );
    assert!(shared.plain_key_generation(client, pane, &prefix).is_none());
    let release = zz_terminal::KeyInput {
        action: zz_terminal::KeyAction::Release,
        ..letter('a')
    };
    assert!(
        shared
            .plain_key_generation(client, pane, &release)
            .is_none()
    );
    press(&shared, client, pane, prefix);
    assert!(
        shared
            .plain_key_generation(client, pane, &letter('a'))
            .is_none()
    );
    run(
        &shared,
        client,
        &mut context,
        &["bind-key", "-n", "Any", "display-message", "any"],
    )
    .expect("bind any");
    run(&shared, client, &mut context, &["unbind-key", "-n", "z"]).expect("unbind");
    assert!(
        shared
            .plain_key_generation(client, pane, &letter('y'))
            .is_none()
    );
}

#[test]
fn a_binding_added_mid_session_runs_on_the_next_key() {
    let (shared, client, mut context, pane, _) = key_table_fixture("late");
    press(&shared, client, pane, letter('a'));
    wait_for_screen(&shared, client, &mut context, "late", |screen| {
        screen.contains('a')
    });
    run(
        &shared,
        client,
        &mut context,
        &["bind-key", "-n", "x", "set-option", "-g", "@pressed", "yes"],
    )
    .expect("bind");
    press(&shared, client, pane, letter('x'));
    let shown = run(
        &shared,
        client,
        &mut context,
        &["show-options", "-gv", "@pressed"],
    )
    .expect("show")
    .output;
    assert_eq!(shown.trim(), "yes");
    let screen = run(
        &shared,
        client,
        &mut context,
        &["capture-pane", "-p", "-t", "late"],
    )
    .expect("capture")
    .output;
    assert!(!screen.contains('x'), "{screen:?}");
}

#[test]
fn copy_mode_takes_the_full_path() {
    let (shared, client, mut context, pane, _) = key_table_fixture("modes");
    assert!(
        shared
            .plain_key_generation(client, pane, &letter('a'))
            .is_some()
    );
    run(&shared, client, &mut context, &["copy-mode", "-t", "modes"]).expect("copy-mode");
    assert!(
        shared
            .plain_key_generation(client, pane, &letter('a'))
            .is_none()
    );
}

#[test]
fn messages_read_only_and_unattached_clients_take_the_full_path() {
    let (shared, client, mut context, pane, _) = key_table_fixture("clients");
    let plain = |shared: &Arc<Shared>| shared.plain_key_generation(client, pane, &letter('a'));
    assert!(plain(&shared).is_some());
    shared.inner.lock().client_flags.insert(client);
    assert!(plain(&shared).is_none());
    shared.inner.lock().client_flags.remove(client);
    assert!(plain(&shared).is_some());
    run(
        &shared,
        client,
        &mut context,
        &[
            "display-message",
            "-c",
            &client.to_string(),
            "-d",
            "0",
            "shown",
        ],
    )
    .expect("display-message");
    let shown = shared
        .inner
        .lock()
        .client(client)
        .is_some_and(|registered| registered.message.is_some());
    assert_eq!(plain(&shared).is_none(), shown);
    let (other, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());
    assert!(
        shared
            .plain_key_generation(other, pane, &letter('a'))
            .is_none()
    );
}
