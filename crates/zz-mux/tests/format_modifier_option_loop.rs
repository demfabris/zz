use zz_mux::{ExecutionContext, MuxEngine};
use zz_protocol::CommandInvocation;

fn command(name: &str, args: &[&str]) -> CommandInvocation {
    CommandInvocation::new(name, args.iter().copied())
}

struct Probe {
    engine: MuxEngine,
    context: ExecutionContext,
}

impl Probe {
    fn new() -> Self {
        let mut engine = MuxEngine::default();
        let mut context = ExecutionContext::default();
        engine
            .execute(&mut context, &command("new-session", &["-d", "-s", "o"]))
            .expect("session");
        Self { engine, context }
    }

    fn run(&mut self, name: &str, args: &[&str]) {
        self.engine
            .execute(&mut self.context, &command(name, args))
            .unwrap_or_else(|error| panic!("{name} {args:?}: {error:?}"));
    }

    fn format(&mut self, template: &str) -> String {
        self.engine
            .execute(
                &mut self.context,
                &command("display-message", &["-p", template]),
            )
            .expect("display-message")
            .output
            .to_string()
    }

    fn rows(&mut self, template: &str) -> usize {
        self.format(&format!("#{{n:{template}}}"))
            .parse()
            .expect("row count")
    }
}

#[test]
fn option_loop_rows_carry_the_pinned_fields_and_order() {
    let mut probe = Probe::new();
    probe.run("set-option", &["-t", "o", "status-left", "L"]);
    probe.run("set-option", &["-t", "o", "@user", "U"]);
    probe.run("set-option", &["-t", "o", "status-format[0]", "A"]);
    probe.run("set-option", &["-t", "o", "status-format[1]", "B"]);
    probe.run(
        "set-hook",
        &["-t", "o", "after-new-window", "display-message x"],
    );

    let template = concat!(
        "#{O:#{loop_index}|#{option_name}|#{option_value}|#{option_is_array}",
        "|#{option_array_key}|#{option_array_index}|#{option_array_first}",
        "|#{option_array_last}|#{option_array_count}|#{option_is_hook}",
        "|#{option_is_user}|#{loop_last_flag};}"
    );
    assert_eq!(
        probe.format(template),
        concat!(
            "0|@user|U|0|||0|0|0|0|1|0;",
            "1|after-new-window|display-message x|1|0|0|1|1|1|1|0|0;",
            "2|status-format|A|1|0|0|1|0|2|0|0|0;",
            "3|status-format|B|1|1|1|0|1|2|0|0|0;",
            "4|status-left|L|0|||0|0|0|0|0|1;"
        )
    );
}

#[test]
fn option_loop_gives_an_empty_array_one_row() {
    let mut probe = Probe::new();
    probe.run("set-option", &["-t", "o", "update-environment", ""]);
    assert_eq!(
        probe.format(
            "#{O:#{option_name}=(#{option_value})#{option_array_count}#{option_array_first}#{option_array_last};}"
        ),
        "update-environment=()011;"
    );
}

#[test]
fn option_loop_selects_the_stored_scope_per_flag() {
    let mut probe = Probe::new();
    probe.run("set-option", &["-t", "o", "@session", "S"]);
    probe.run("set-option", &["-w", "-t", "o:0", "@window", "W"]);
    probe.run("set-option", &["-p", "-t", "o:0.0", "@pane", "P"]);

    for (template, expected) in [
        ("#{O:#{option_name};}", "@session;"),
        ("#{Os:#{option_name};}", "@session;"),
        ("#{Ow:#{option_name};}", "@window;"),
        ("#{Op:#{option_name};}", "@pane;"),
    ] {
        assert_eq!(probe.format(template), expected, "{template}");
    }

    let server = probe.rows("#{Ov:#{l:x}}");
    let global_session = probe.rows("#{Ogs:#{l:x}}");
    let global_window = probe.rows("#{Ogw:#{l:x}}");
    assert_eq!(probe.rows("#{Og:#{l:x}}"), global_session);
    assert!(server > 0 && global_session > 0 && global_window > 0);
    assert_ne!(server, global_session);
    assert_ne!(global_session, global_window);
}

#[test]
fn option_loop_prints_an_empty_line_for_invalid_and_unreachable_scopes() {
    let mut probe = Probe::new();
    probe.run("set-option", &["-t", "o", "@session", "S"]);
    for template in [
        "#{Oz:#{option_name};}",
        "#{Ogp:#{option_name};}",
        "#{Ow:#{option_name};}",
        "#{Op:#{option_name};}",
    ] {
        assert_eq!(probe.format(template), "\n", "{template}");
    }
}

#[test]
fn option_loop_flag_precedence_matches_the_pin() {
    let mut probe = Probe::new();
    probe.run("set-option", &["-t", "o", "@session", "S"]);
    probe.run("set-option", &["-w", "-t", "o:0", "@window", "W"]);
    probe.run("set-option", &["-p", "-t", "o:0.0", "@pane", "P"]);

    assert_eq!(probe.format("#{Osw:#{option_name};}"), "@window;");
    assert_eq!(probe.format("#{Osp:#{option_name};}"), "@session;");
    assert_eq!(probe.format("#{Owp:#{option_name};}"), "@window;");
    let server = probe.rows("#{Ov:#{l:x}}");
    assert_eq!(probe.rows("#{Ovgw:#{l:x}}"), server);
    assert_eq!(probe.rows("#{Ogsw:#{l:x}}"), probe.rows("#{Ogw:#{l:x}}"));
}
