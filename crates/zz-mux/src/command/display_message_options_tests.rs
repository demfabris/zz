use super::*;

fn parsed(args: &[&str]) -> String {
    let args = CommandInvocation::new("display-message", args.iter().copied()).args;
    format!("{:?}", parse_display_message_options(&args))
}

fn generic(args: &[&str]) -> String {
    let args = CommandInvocation::new("display-message", args.iter().copied()).args;
    format!("{:?}", parse_command_options("display-message", &args))
}

#[test]
fn printed_display_message_options_match_the_generic_parse() {
    for args in [
        &["-p", "x"][..],
        &["-p", ""],
        &["-p", "#{session_name}"],
        &["-p", "a b"],
        &["-p", "-x"],
        &["-p", "--"],
        &["-p", "-"],
        &["-pl", "x"],
        &["-l", "x"],
        &["-p"],
        &["-p", "x", "y"],
        &["x"],
        &[],
    ] {
        assert_eq!(parsed(args), generic(args), "{args:?}");
    }
}

#[test]
fn display_message_j_parses_the_expanded_message_as_json_like_3_8() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    let run = |engine: &mut MuxEngine, context: &mut ExecutionContext, args: &[&str]| {
        engine.execute(
            context,
            &CommandInvocation::new("display-message", args.iter().copied()),
        )
    };
    engine
        .execute(
            &mut context,
            &CommandInvocation::new("new-session", ["-s", "work"]),
        )
        .unwrap();

    assert_eq!(
        run(
            &mut engine,
            &mut context,
            &[
                "-pj",
                "{ \"s\": \"#{session_name}\", \"n\": #{window_index}, \"a\": true }"
            ],
        )
        .unwrap()
        .output,
        "{\"a\":true,\"n\":0,\"s\":\"work\"}"
    );
    assert_eq!(
        run(
            &mut engine,
            &mut context,
            &["-pjl", "{\"b\":\"#{x}\",\"a\":[{}]}"]
        )
        .unwrap()
        .output,
        "{\"a\":[{}],\"b\":\"#{x}\"}"
    );
    assert_eq!(
        run(&mut engine, &mut context, &["-pj", "-F", "{\"z\":1}"])
            .unwrap()
            .output,
        "{\"z\":1}"
    );
    assert_eq!(
        run(&mut engine, &mut context, &["-pj"]),
        Err(ServerError::InvalidCommand("empty input".to_owned()))
    );
    assert_eq!(
        run(&mut engine, &mut context, &["-pj", "[#{session_name}]"]),
        Err(ServerError::InvalidCommand(
            "tokenization error: work]".to_owned()
        ))
    );
    assert_eq!(
        run(&mut engine, &mut context, &["-pj", "[]"]),
        Err(ServerError::InvalidCommand(
            "expected object: []".to_owned()
        ))
    );
    assert_eq!(
        run(&mut engine, &mut context, &["-paj", "{\"k\":\"v\"}"])
            .unwrap()
            .output,
        "{\"k\":\"v\"}"
    );
    assert_eq!(
        run(&mut engine, &mut context, &["-pIj", "{\"k\":\"v\"}"])
            .unwrap()
            .output,
        "{\"k\":\"v\"}"
    );
    let displayed = run(&mut engine, &mut context, &["-j", "{\"k\":2}"]).unwrap();
    assert!(
        displayed.effects.iter().any(|effect| matches!(
            effect,
            MuxEffect::DisplayMessage { text, .. } if text == "{\"k\":2}"
        )),
        "{displayed:?}"
    );
}
