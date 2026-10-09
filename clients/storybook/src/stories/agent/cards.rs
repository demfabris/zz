use std::sync::Arc;

use zpui::{
    AnyElement, AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div, px, uniform_list,
};
use zz_client::agent_completion::{
    AgentCommand, CommandCompletion, bare_command_name, completion_query,
    meaningful_command_description, pane_commands, ranked_completions,
};
use zz_protocol::{AgentTaskWire, MAX_AGENT_OPTION_BYTES, agent_stream::AgentQuestion};
use zz_ui::{
    Disableable as _, Sizable as _,
    agent::{
        composer::COMPOSER_MAX_WIDTH,
        presentation::{permission_card, permission_option, spinner_phase},
        question::QuestionCardState,
        slash::{suggestion_list, suggestion_list_height, suggestion_row},
        tasks::{TaskTrayAction, TrayPanel, task_tray},
    },
    button::{Button, ButtonVariants as _},
    input::InputEvent,
};

use super::fixtures::{PLAN, PLAN_DONE, branch_question, checks_question, question, tasks};
use crate::story::{stateless, states};

fn prefix_width(element: impl IntoElement) -> impl IntoElement {
    div().w_full().max_w(px(COMPOSER_MAX_WIDTH)).child(element)
}

pub fn permissions(_: &mut Window, cx: &mut App) -> AnyView {
    stateless(render_permissions, cx)
}

fn permission(
    id: &str,
    title: &str,
    counter: Option<&str>,
    options: &[(&str, Option<bool>)],
    highlighted: usize,
    enabled: bool,
    cx: &App,
) -> AnyElement {
    let options = options
        .iter()
        .enumerate()
        .map(|(index, (name, allow))| {
            let button = Button::new(SharedString::from(format!("{id}-{index}")))
                .small()
                .label(*name)
                .disabled(!enabled);
            let button = match allow {
                Some(true) => button.primary(),
                Some(false) => button.danger(),
                None => button,
            };
            permission_option(
                (SharedString::from(format!("{id}-option")), index),
                index,
                index == highlighted,
                button,
                cx,
            )
            .into_any_element()
        })
        .collect();
    permission_card(
        title.to_owned(),
        counter.map(|counter| counter.to_owned().into()),
        options,
        Button::new(SharedString::from(format!("{id}-cancel")))
            .small()
            .ghost()
            .disabled(!enabled)
            .label("Cancel request"),
        cx,
    )
    .into_any_element()
}

const COMMAND: [(&str, Option<bool>); 3] = [
    ("Allow", Some(true)),
    ("Always allow this session", Some(true)),
    ("Reject", Some(false)),
];

const PLAN_EXIT: [(&str, Option<bool>); 3] = [
    ("Yes, and auto-accept edits", Some(true)),
    ("Yes, and manually approve edits", Some(true)),
    ("No, keep planning", Some(false)),
];

const TEN: [(&str, Option<bool>); 10] = [
    ("Tokyo Night", Some(true)),
    ("Catppuccin", Some(true)),
    ("Nord", Some(true)),
    ("Gruvbox", Some(true)),
    ("Solarized", Some(true)),
    ("Rosé Pine", Some(true)),
    ("Dracula", Some(true)),
    ("One Dark", Some(true)),
    ("Kanagawa", Some(true)),
    ("Everforest", Some(true)),
];

fn render_permissions(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "command approval, first option highlighted",
            prefix_width(permission(
                "permission-command",
                "cargo test -p zz-ui browser",
                None,
                &COMMAND,
                0,
                true,
                cx,
            )),
        )
        .state(
            "plan approval, first of two requests",
            prefix_width(permission(
                "permission-plan",
                "Ready to code?",
                Some("1/2"),
                &PLAN_EXIT,
                1,
                true,
                cx,
            )),
        )
        .state(
            "long title",
            prefix_width(permission(
                "permission-long",
                "Edit crates/zz-ui/src/browser/tab_strip.rs, crates/zz-ui/src/browser/drag.rs and four more files to give pinned tabs a width floor",
                None,
                &[("Allow", Some(true)), ("Reject", Some(false))],
                0,
                true,
                cx,
            )),
        )
        .state(
            "answered, waiting for the agent",
            prefix_width(permission(
                "permission-answered",
                "cargo test -p zz-ui browser",
                None,
                &COMMAND,
                0,
                false,
                cx,
            )),
        )
        .state(
            "ten choices, digits stop at nine",
            prefix_width(permission(
                "permission-ten",
                "Which palette should the storybook use?",
                None,
                &TEN,
                9,
                true,
                cx,
            )),
        )
        .into_any_element()
}

struct QuestionCase {
    label: &'static str,
    state: QuestionCardState,
    counter: Option<SharedString>,
    enabled: bool,
}

pub struct Questions {
    cases: Vec<QuestionCase>,
}

fn card(
    index: usize,
    questions: Vec<AgentQuestion>,
    window: &mut Window,
    cx: &mut Context<Questions>,
) -> QuestionCardState {
    QuestionCardState::new(
        index as u64 + 1,
        questions,
        window,
        cx,
        move |view: &mut Questions, question, event, _, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(case) = view.cases.get_mut(index)
                && case.state.input_changed(question, cx)
            {
                cx.notify();
            }
        },
    )
}

fn type_other(
    state: &mut QuestionCardState,
    question: usize,
    text: &str,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(input) = state.other_input(question) {
        input.update(cx, |input, cx| input.set_value(text.to_owned(), window, cx));
    }
    state.input_changed(question, cx);
}

pub fn questions(window: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| {
        let mut cases = Vec::new();
        let mut add = |label, state, counter: Option<&str>, enabled| {
            cases.push(QuestionCase {
                label,
                state,
                counter: counter.map(|counter| counter.to_owned().into()),
                enabled,
            });
        };

        add(
            "one question, a pick submits it",
            card(0, vec![branch_question()], window, cx),
            None,
            true,
        );

        let mut headed = branch_question();
        headed.header = Some("Base".into());
        add(
            "with a header chip",
            card(1, vec![headed], window, cx),
            None,
            true,
        );

        let mut multi = card(2, vec![checks_question()], window, cx);
        multi.card.pick(0, 0);
        multi.card.pick(0, 1);
        add("pick any, two picked", multi, None, true);

        let mut several = card(3, vec![branch_question(), checks_question()], window, cx);
        several.card.pick(0, 0);
        add(
            "several questions, second in focus, 2 of 3",
            several,
            Some("2/3"),
            true,
        );

        let mut typed = card(4, vec![branch_question()], window, cx);
        typed.card.select_other(0);
        type_other(&mut typed, 0, "release/0.15", window, cx);
        add("typed answer replaces the choice", typed, None, true);

        let mut free = card(
            5,
            vec![question(
                "name",
                "What should the release be called?",
                Vec::new(),
            )],
            window,
            cx,
        );
        type_other(&mut free, 0, "0.16 Pinned", window, cx);
        add("free text", free, None, true);

        let mut secret = card(
            6,
            vec![AgentQuestion {
                secret: true,
                ..question("token", "Paste the deploy token for staging", Vec::new())
            }],
            window,
            cx,
        );
        type_other(&mut secret, 0, "not-a-real-token-1234", window, cx);
        add("secret", secret, None, true);

        let mut complete = card(7, vec![branch_question(), checks_question()], window, cx);
        complete.card.pick(0, 1);
        complete.card.pick(1, 0);
        complete.card.pick(1, 2);
        add("complete, Submit enabled", complete, None, true);

        let unsendable = AgentQuestion {
            id: "q".repeat(MAX_AGENT_OPTION_BYTES + 1),
            ..branch_question()
        };
        add(
            "a card zz cannot answer",
            card(8, vec![unsendable], window, cx),
            None,
            true,
        );

        add(
            "read-only",
            card(9, vec![branch_question()], window, cx),
            None,
            false,
        );

        Questions { cases }
    })
    .into()
}

impl Render for Questions {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        self.cases
            .iter()
            .enumerate()
            .fold(states().columns(2), |states, (index, case)| {
                let entity = entity.clone();
                states.state(
                    case.label,
                    case.state.render(
                        &format!("question-{index}"),
                        case.counter.clone(),
                        case.enabled,
                        move |action, window, cx| {
                            entity.update(cx, |view, cx| {
                                if let Some(case) = view.cases.get_mut(index) {
                                    case.state.action(action, window, cx);
                                    cx.notify();
                                }
                            });
                        },
                        cx,
                    ),
                )
            })
    }
}

pub fn slash_commands(query: &str) -> Vec<CommandCompletion> {
    let vendor = [
        (
            "compact",
            "Clear conversation history but keep a summary in context",
            None,
        ),
        ("review", "Review a pull request", Some("PR number")),
        (
            "init",
            "Initialize a new CLAUDE.md file with codebase documentation",
            None,
        ),
        (
            "pr-comments",
            "Get comments from a GitHub pull request",
            None,
        ),
        (
            "security-review",
            "Complete a security review of the pending changes on the current branch",
            None,
        ),
        ("statusline", "…", None),
    ]
    .into_iter()
    .map(
        |(name, description, hint): (&str, &str, Option<&str>)| AgentCommand {
            name: name.to_owned(),
            description: description.to_owned(),
            input_hint: hint.map(str::to_owned),
        },
    )
    .collect::<Vec<_>>();
    let commands = pane_commands(&vendor, true);
    let value = format!("/{query}");
    completion_query(&value, value.len())
        .map(|query| ranked_completions(&commands, &query))
        .unwrap_or_default()
}

pub fn slash_list(
    id: &str,
    completions: &[CommandCompletion],
    selected: Option<usize>,
    cx: &App,
) -> AnyElement {
    let completions: Arc<[CommandCompletion]> = completions.into();
    let count = completions.len();
    let prefix = id.to_owned();
    let rows = uniform_list(SharedString::from(format!("{id}-rows")), count, {
        move |range, _, cx| {
            range
                .filter_map(|index| {
                    let command = &completions.get(index)?.command;
                    Some(suggestion_row(
                        SharedString::from(format!("{prefix}-{index}")),
                        bare_command_name(&command.name),
                        meaningful_command_description(&command.description)
                            .map(|description| description.to_owned().into()),
                        selected == Some(index),
                        cx,
                    ))
                })
                .collect::<Vec<_>>()
        }
    })
    .w_full()
    .h(suggestion_list_height(count));
    suggestion_list(rows, cx).into_any_element()
}

pub fn slash_menu(_: &mut Window, cx: &mut App) -> AnyView {
    stateless(render_slash_menu, cx)
}

fn render_slash_menu(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "every command, first selected, scrolls past six",
            prefix_width(slash_list("slash-all", &slash_commands(""), Some(0), cx)),
        )
        .state(
            "filtered by /re, second selected",
            prefix_width(slash_list("slash-re", &slash_commands("re"), Some(1), cx)),
        )
        .state(
            "nothing selected yet",
            prefix_width(slash_list("slash-none", &slash_commands("fo"), None, cx)),
        )
        .state(
            "a command without a description",
            prefix_width(slash_list(
                "slash-bare",
                &slash_commands("statusl"),
                Some(0),
                cx,
            )),
        )
        .into_any_element()
}

struct TrayCase {
    label: &'static str,
    plan: Option<&'static str>,
    tasks: Vec<AgentTaskWire>,
    open: Option<TrayPanel>,
    enabled: bool,
}

pub struct Trays {
    cases: Vec<TrayCase>,
}

pub fn trays(_: &mut Window, cx: &mut App) -> AnyView {
    let case = |label, plan, tasks, open, enabled| TrayCase {
        label,
        plan,
        tasks,
        open,
        enabled,
    };
    let cases = vec![
        case("plan, closed", Some(PLAN), Vec::new(), None, true),
        case(
            "plan, open",
            Some(PLAN),
            Vec::new(),
            Some(TrayPanel::Plan),
            true,
        ),
        case("background tasks, closed", None, tasks(), None, true),
        case(
            "background tasks, open",
            None,
            tasks(),
            Some(TrayPanel::Tasks),
            true,
        ),
        case("plan and tasks", Some(PLAN), tasks(), None, true),
        case(
            "finished plan, open",
            Some(PLAN_DONE),
            Vec::new(),
            Some(TrayPanel::Plan),
            true,
        ),
        case(
            "read-only, tasks open",
            None,
            tasks(),
            Some(TrayPanel::Tasks),
            false,
        ),
    ];
    cx.new(|_| Trays { cases }).into()
}

impl Render for Trays {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let view = cx.entity_id();
        let phase = spinner_phase(view, cx);
        self.cases
            .iter()
            .enumerate()
            .fold(states(), |states, (index, case)| {
                let entity = entity.clone();
                let tray = task_tray(
                    &format!("tray-{index}"),
                    case.plan,
                    &case.tasks,
                    case.open,
                    case.enabled,
                    phase,
                    move |action, _, cx| {
                        if let TaskTrayAction::Toggle(panel) = action {
                            entity.update(cx, |view, cx| {
                                if let Some(case) = view.cases.get_mut(index) {
                                    case.open = (case.open != Some(panel)).then_some(panel);
                                    cx.notify();
                                }
                            });
                        }
                    },
                    cx,
                );
                states.state(case.label, prefix_width(div().children(tray)))
            })
    }
}
