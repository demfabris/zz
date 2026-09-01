use zz_mux::{ExecutionContext, MuxEngine};
use zz_protocol::CommandInvocation;

const CONTEXT_FORMATS: &[&str] = &[
    "loop_index",
    "loop_last_flag",
    "option_array_count",
    "option_array_first",
    "option_array_index",
    "option_array_key",
    "option_array_last",
    "option_is_array",
    "option_is_hook",
    "option_is_user",
    "option_name",
    "option_value",
];

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
    }
}

fn stocked() -> Probe {
    let mut probe = Probe::new();
    probe.run("set-option", &["-t", "o", "@a", "A"]);
    probe.run("set-option", &["-t", "o", "status-format[0]", "X"]);
    probe.run("set-option", &["-t", "o", "status-format[1]", "Y"]);
    probe
}

#[test]
fn option_loop_context_formats_are_empty_outside_a_loop() {
    let mut probe = stocked();
    for name in CONTEXT_FORMATS {
        assert_eq!(probe.format(&format!("#{{{name}}}")), "", "{name}");
    }
    assert_eq!(probe.format("#{?option_is_array,arr,notarr}"), "notarr");
}

#[test]
fn both_option_loop_producers_own_every_context_format() {
    let mut probe = stocked();
    let template = concat!(
        "#{O:<#{loop_index}#{loop_last_flag}#{option_array_count}",
        "#{option_array_first}#{option_array_index}>}"
    );
    assert_eq!(probe.format(template), "<0000><10210><21201>");

    let full = CONTEXT_FORMATS
        .iter()
        .map(|name| format!("#{{{name}}}"))
        .collect::<Vec<_>>()
        .join("|");
    assert_eq!(
        probe.format(&format!("#{{O:{full};}}")),
        concat!(
            "0|0|0|0|||0|0|0|1|@a|A;",
            "1|0|2|1|0|0|0|1|0|0|status-format|X;",
            "2|1|2|0|1|1|1|1|0|0|status-format|Y;"
        )
    );
}

#[test]
fn the_empty_array_producer_keeps_its_own_context_row() {
    let mut probe = Probe::new();
    probe.run("set-option", &["-t", "o", "update-environment", ""]);
    let full = CONTEXT_FORMATS
        .iter()
        .map(|name| format!("#{{{name}}}"))
        .collect::<Vec<_>>()
        .join("|");
    assert_eq!(
        probe.format(&format!("#{{O:{full};}}")),
        "0|1|0|1|||1|1|0|0|update-environment|;"
    );
}

#[test]
fn nested_loops_rebind_the_option_loop_context() {
    let mut probe = stocked();
    assert_eq!(
        probe.format("#{S:#{O:<#{loop_index}:#{option_name}>}}"),
        "<0:@a><1:status-format><2:status-format>"
    );
    assert_eq!(probe.format("#{O:#{S:[#{loop_index}]}}"), "[0][0][0]");
    assert_eq!(
        probe.format("#{O:#{?option_is_array,arr,notarr};}"),
        "notarr;arr;arr;"
    );
}
