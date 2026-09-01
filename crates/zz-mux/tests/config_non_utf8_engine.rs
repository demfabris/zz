use zz_mux::{ExecutionContext, MuxEngine, ParsedConfig};
use zz_protocol::CommandInvocation;

fn command(name: &str, args: &[&str]) -> CommandInvocation {
    CommandInvocation::new(name, args.iter().copied())
}

fn engine() -> MuxEngine {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(
            &mut context,
            &command("set-environment", &["-g", "ZZ_BYTE_ENGINE", "expanded"]),
        )
        .expect("global environment assignment");
    engine
}

fn arguments(command: &zz_mux::ConfigCommandBytes) -> Vec<&[u8]> {
    command.args.iter().map(Vec::as_slice).collect()
}

#[test]
fn file_byte_input_carries_engine_variables_and_conditions() {
    let engine = engine();
    let input = b"set-environment -g MIX pre\xff$ZZ_BYTE_ENGINE\n\
        %if #{==:#{l:a b},a b}\n\
        set-environment -g TAKEN yes\n\
        %endif\n\
        %if #{==:x,y}\n\
        set-environment -g SKIPPED yes\n\
        %endif\n";

    let parsed = engine.parse_config_file_bytes("engine.conf", input);
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.commands.len(), 2);
    assert_eq!(parsed.commands[0].name, b"set-environment");
    assert_eq!(
        arguments(&parsed.commands[0]),
        [
            b"-g".as_slice(),
            b"MIX".as_slice(),
            b"pre\xffexpanded".as_slice()
        ]
    );
    assert_eq!(
        arguments(&parsed.commands[1]),
        [b"-g".as_slice(), b"TAKEN".as_slice(), b"yes".as_slice()]
    );

    let contextless = ParsedConfig::parse_file_bytes("engine.conf", input);
    assert_eq!(contextless.commands.len(), 1);
    assert_eq!(
        arguments(&contextless.commands[0]),
        [b"-g".as_slice(), b"MIX".as_slice(), b"pre\xff".as_slice()]
    );
}

#[test]
fn file_byte_input_keeps_assignment_overlay_bytes_out_of_the_parse_only_adapter() {
    let engine = engine();
    let input = b"ZZOVR=over\xfflay\nset-environment -g MIX $ZZOVR\n";

    let parsed = engine.parse_config_file_bytes("overlay.conf", input);
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.environment.len(), 1);
    assert_eq!(parsed.environment[0].name, b"ZZOVR");
    assert_eq!(parsed.environment[0].value, b"over\xfflay");
    assert!(!parsed.environment[0].hidden);
    assert_eq!(parsed.commands.len(), 1);
    assert_eq!(
        arguments(&parsed.commands[0]),
        [
            b"-g".as_slice(),
            b"MIX".as_slice(),
            b"over\xfflay".as_slice()
        ]
    );

    let parse_only = engine.parse_config_file_bytes_parse_only("overlay.conf", input);
    assert!(parse_only.diagnostics.is_empty());
    assert_eq!(parse_only.environment.len(), 1);
    assert_eq!(parse_only.environment[0].value, b"over\xfflay");
    assert_eq!(parse_only.commands.len(), 1);
    assert_eq!(
        arguments(&parse_only.commands[0]),
        [b"-g".as_slice(), b"MIX".as_slice(), b"".as_slice()]
    );
}

#[test]
fn parse_only_byte_input_still_evaluates_engine_conditions() {
    let engine = engine();
    let input = b"%if #{==:#{l:a b},a b}\n\
        set-environment -g TAKEN pre\xff$ZZ_BYTE_ENGINE\n\
        %else\n\
        set-environment -g SKIPPED yes\n\
        %endif\n";

    let parsed = engine.parse_config_file_bytes_parse_only("condition.conf", input);
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.commands.len(), 1);
    assert_eq!(
        arguments(&parsed.commands[0]),
        [
            b"-g".as_slice(),
            b"TAKEN".as_slice(),
            b"pre\xffexpanded".as_slice()
        ]
    );
}

#[test]
fn buffer_byte_input_keeps_signed_eof_while_carrying_engine_context() {
    let engine = engine();
    let input = b"set-environment -g MIX pre\xff$ZZ_BYTE_ENGINE\nset-environment -g AFTER $ZZ_BYTE_ENGINE\n";

    let parsed = engine.parse_config_buffer_bytes("buffer.conf", input);
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.commands.len(), 2);
    assert_eq!(
        arguments(&parsed.commands[0]),
        [
            b"-g".as_slice(),
            b"MIX".as_slice(),
            b"pre".as_slice(),
            b"expanded".as_slice()
        ]
    );
    assert_eq!(
        arguments(&parsed.commands[1]),
        [
            b"-g".as_slice(),
            b"AFTER".as_slice(),
            b"expanded".as_slice()
        ]
    );

    let file = engine.parse_config_file_bytes("buffer.conf", input);
    assert_eq!(
        arguments(&file.commands[0]),
        [
            b"-g".as_slice(),
            b"MIX".as_slice(),
            b"pre\xffexpanded".as_slice()
        ]
    );
}

#[test]
fn ascii_byte_input_agrees_with_the_string_adapters() {
    let engine = engine();
    let text = "ZZOVR=overlay\n\
        set-environment -g MIX $ZZOVR-$ZZ_BYTE_ENGINE\n\
        %if #{==:#{l:a b},a b}\n\
        set-environment -g TAKEN yes\n\
        %endif\n";

    for (bytes, strings) in [
        (
            engine.parse_config_file_bytes("ascii.conf", text.as_bytes()),
            engine.parse_config("ascii.conf", text),
        ),
        (
            engine.parse_config_file_bytes_parse_only("ascii.conf", text.as_bytes()),
            engine.parse_config_parse_only("ascii.conf", text),
        ),
    ] {
        assert_eq!(bytes.commands.len(), strings.commands.len());
        for (byte_command, string_command) in bytes.commands.iter().zip(&strings.commands) {
            assert_eq!(byte_command.name, string_command.name.as_bytes());
            assert_eq!(
                arguments(byte_command),
                string_command
                    .args
                    .iter()
                    .map(String::as_bytes)
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(bytes.environment.len(), strings.environment.len());
        for (byte_assignment, string_assignment) in
            bytes.environment.iter().zip(&strings.environment)
        {
            assert_eq!(byte_assignment.name, string_assignment.name.as_bytes());
            assert_eq!(byte_assignment.value, string_assignment.value.as_bytes());
            assert_eq!(byte_assignment.hidden, string_assignment.hidden);
        }
        assert_eq!(bytes.diagnostics, strings.diagnostics);
    }
}
