//! Randomized command interleavings from several connections at once.

use zz_mux::{ExecutionContext, MuxEngine};
use zz_protocol::{CommandInvocation, PaneId, SessionId, WindowId};

const ITERATIONS: usize = 2_000;
const CONNECTIONS: usize = 3;
const MAX_SESSIONS: usize = 6;

// The 64-bit LCG constants from Knuth, taking the high bits as the output.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("bound fits u64");
        usize::try_from(self.next() % bound).expect("a remainder below bound fits usize")
    }

    fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }

    fn pick<T: Copy>(&mut self, values: &[T]) -> Option<T> {
        (!values.is_empty()).then(|| values[self.below(values.len())])
    }
}

struct Ids {
    sessions: Vec<SessionId>,
    windows: Vec<WindowId>,
    panes: Vec<PaneId>,
}

fn live_ids(engine: &MuxEngine) -> Ids {
    Ids {
        sessions: engine.state.sessions.keys().copied().collect(),
        windows: engine.state.windows.keys().copied().collect(),
        panes: engine
            .state
            .windows
            .values()
            .flat_map(|window| window.panes.keys().copied())
            .collect(),
    }
}

fn target(rng: &mut Rng, id: Option<impl ToString>) -> Vec<String> {
    id.map_or_else(Vec::new, |id| {
        if rng.chance(2) {
            vec!["-t".to_owned(), id.to_string()]
        } else {
            Vec::new()
        }
    })
}

fn next_command(rng: &mut Rng, engine: &MuxEngine, names: &mut u32) -> CommandInvocation {
    let ids = live_ids(engine);
    let session = rng.pick(&ids.sessions);
    let window = rng.pick(&ids.windows);
    let pane = rng.pick(&ids.panes);
    *names += 1;
    let unique = *names;
    match rng.below(21) {
        0 if engine.state.sessions.len() >= MAX_SESSIONS => CommandInvocation::new(
            "kill-session",
            session.map_or_else(Vec::new, |session| {
                vec!["-t".to_owned(), session.to_string()]
            }),
        ),
        0 => CommandInvocation::new("new-session", ["-d", "-s", &format!("chaos-{unique}")]),
        1 | 2 => CommandInvocation::new("new-window", target(rng, session)),
        3..=5 => CommandInvocation::new(
            "split-window",
            [if rng.chance(2) { "-h" } else { "-v" }.to_owned()]
                .into_iter()
                .chain(target(rng, pane)),
        ),
        6..=9 => CommandInvocation::new(
            "select-pane",
            [["-U", "-D", "-L", "-R"][rng.below(4)].to_owned()]
                .into_iter()
                .chain(target(rng, pane)),
        ),
        10 => CommandInvocation::new("select-window", target(rng, window)),
        11 => CommandInvocation::new("next-window", [] as [String; 0]),
        12 => CommandInvocation::new("kill-pane", target(rng, pane)),
        13 => CommandInvocation::new("kill-window", target(rng, window)),
        14 => CommandInvocation::new("kill-session", target(rng, session)),
        15 => CommandInvocation::new(
            "rename-window",
            target(rng, window)
                .into_iter()
                .chain([format!("chaos-window-{unique}")]),
        ),
        16 => CommandInvocation::new("last-pane", target(rng, window)),
        17 => CommandInvocation::new("rotate-window", target(rng, window)),
        18 => CommandInvocation::new("attach-session", target(rng, session)),
        19 => CommandInvocation::new("list-windows", target(rng, session)),
        _ => CommandInvocation::new("list-panes", target(rng, window)),
    }
}

fn external_kill(rng: &mut Rng, engine: &MuxEngine) -> Option<CommandInvocation> {
    let ids = live_ids(engine);
    match rng.below(3) {
        0 => rng
            .pick(&ids.panes)
            .map(|pane| CommandInvocation::new("kill-pane", ["-t", &pane.to_string()])),
        1 => rng
            .pick(&ids.windows)
            .map(|window| CommandInvocation::new("kill-window", ["-t", &window.to_string()])),
        _ => rng
            .pick(&ids.sessions)
            .map(|session| CommandInvocation::new("kill-session", ["-t", &session.to_string()])),
    }
}

fn assert_invariants(engine: &MuxEngine, step: usize, command: &CommandInvocation) {
    let state = &engine.state;
    for (id, session) in &state.sessions {
        assert!(
            !session.windows.is_empty(),
            "step {step}: session {id} has no windows after {command:?}"
        );
        for window in &session.windows {
            assert!(
                state.windows.contains_key(window),
                "step {step}: session {id} lists dead window {window} after {command:?}"
            );
        }
        assert!(
            state.windows.contains_key(&session.active_window),
            "step {step}: session {id} has a dead active window after {command:?}"
        );
    }
    for (id, window) in &state.windows {
        assert!(
            !window.panes.is_empty(),
            "step {step}: window {id} has no panes after {command:?}"
        );
        assert!(
            window.panes.contains_key(&window.active_pane),
            "step {step}: window {id} has a dead active pane after {command:?}"
        );
    }
    assert_eq!(
        state.validate(),
        Ok(()),
        "step {step}: state is inconsistent after {command:?}"
    );
}

#[test]
fn interleaved_connections_never_panic_and_always_heal() {
    let mut rng = Rng(0x5eed_1e55_c0ff_ee01);
    let mut engine = MuxEngine::default();
    let mut contexts = vec![ExecutionContext::default(); CONNECTIONS];
    let mut external = ExecutionContext::default();
    let mut names = 0_u32;

    for step in 0..ITERATIONS {
        if rng.chance(9)
            && let Some(kill) = external_kill(&mut rng, &engine)
        {
            let _ = engine.execute(&mut external, &kill);
            assert_invariants(&engine, step, &kill);
        }

        let connection = rng.below(CONNECTIONS);
        let command = next_command(&mut rng, &engine, &mut names);
        let failed = engine.execute(&mut contexts[connection], &command).is_err();
        assert_invariants(&engine, step, &command);

        if failed {
            names += 1;
            let heal =
                CommandInvocation::new("new-session", ["-d", "-s", &format!("heal-{names}")]);
            engine
                .execute(&mut contexts[connection], &heal)
                .unwrap_or_else(|error| {
                    panic!(
                        "step {step}: connection {connection} cannot heal after {command:?}: {error}"
                    )
                });
            assert_invariants(&engine, step, &heal);
        }
    }

    names += 1;
    engine
        .execute(
            &mut external,
            &CommandInvocation::new("new-session", ["-d", "-s", &format!("final-{names}")]),
        )
        .expect("a uniquely named session always creates state");
    for (connection, context) in contexts.iter_mut().enumerate() {
        engine
            .execute(
                context,
                &CommandInvocation::new("list-panes", [] as [String; 0]),
            )
            .unwrap_or_else(|error| panic!("connection {connection} is stranded: {error}"));
    }
}
