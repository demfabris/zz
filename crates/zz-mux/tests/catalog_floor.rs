use zz_mux::{COMMAND_SPECS, ExecutionContext, MuxEngine};
use zz_protocol::{CommandInvocation, ServerError};

fn command(name: &str, args: &[&str]) -> CommandInvocation {
    CommandInvocation::new(name, args.iter().copied())
}

fn assert_unknown_flag(command: &str, flag: &str, error: &ServerError) {
    assert_eq!(
        error,
        &ServerError::InvalidCommand(format!("{command} does not support {flag}"))
    );
}

#[test]
fn every_catalog_command_rejects_a_mechanically_absent_flag() {
    assert_eq!(COMMAND_SPECS.len(), 59);
    for spec in COMMAND_SPECS {
        let candidate = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .find(|candidate| {
                let flag = format!("-{candidate}");
                spec.option(&flag).is_none()
            })
            .expect("every command leaves at least one short flag unused");
        let flag = format!("-{candidate}");
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        let error = engine
            .execute(
                &mut context,
                &CommandInvocation::new(spec.name, [flag.clone()]),
            )
            .unwrap_err();
        assert_unknown_flag(spec.name, &flag, &error);
    }
}

#[test]
fn kill_server_rejects_unknown_flag_before_shutdown() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let error = engine
        .execute(&mut context, &command("kill-server", &["-q"]))
        .unwrap_err();
    assert_unknown_flag("kill-server", "-q", &error);
}

#[test]
fn last_window_rejects_unknown_flag_before_switching() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    engine
        .execute(&mut context, &command("new-window", &["-n", "second"]))
        .unwrap();
    let current = context.clone();

    let error = engine
        .execute(&mut context, &command("last-window", &["-q"]))
        .unwrap_err();

    assert_unknown_flag("last-window", "-q", &error);
    assert_eq!(context, current);
}

#[test]
fn send_prefix_rejects_unknown_flag_before_emitting_input() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();

    let error = engine
        .execute(&mut context, &command("send-prefix", &["-q"]))
        .unwrap_err();

    assert_unknown_flag("send-prefix", "-q", &error);
}

#[test]
fn catalogued_unsupported_value_keeps_the_unsupported_error_shape() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let error = engine
        .execute(&mut context, &command("new-session", &["-x", "80"]))
        .unwrap_err();

    assert_eq!(
        error,
        ServerError::UnsupportedCommand("new-session -x".to_owned())
    );
}
