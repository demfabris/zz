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
