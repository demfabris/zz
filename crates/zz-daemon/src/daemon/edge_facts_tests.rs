use std::cell::Cell;

thread_local! {
    static LOOKUPS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn count_lookup() {
    LOOKUPS.with(|lookups| lookups.set(lookups.get() + 1));
}

#[cfg(unix)]
mod unix {
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::super::*;
    use super::{Cell, LOOKUPS};

    fn lookups() -> usize {
        LOOKUPS.with(Cell::get)
    }

    fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !ready() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn printing_pane(shared: &Arc<Shared>, name: &str) -> (PaneId, Arc<TerminalSession>) {
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new(
                    "new-session",
                    ["-d", "-s", name, "while :; do printf x; sleep 0.005; done"],
                ),
            )
            .expect("printing session");
        let pane = context.pane.expect("session pane");
        let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
        wait_until("the pane's first output", || {
            terminal.last_output().is_some()
        });
        wait_until("the watcher's first name check", || {
            shared
                .inner
                .lock()
                .engine
                .pane_runtime_facts(pane)
                .is_some_and(|facts| !facts.current_command.is_empty())
        });
        (pane, terminal)
    }

    #[test]
    fn a_printing_pane_reads_its_names_once_per_interval() {
        let shared = Arc::new(Shared::new(1));
        let (pane, terminal) = printing_pane(&shared, "edge-names");
        let start = Instant::now() + Duration::from_hours(1);
        let before = lookups();
        let mut checks = Vec::new();
        for frame in 0..100u32 {
            let now = start + Duration::from_millis(u64::from(frame) * 10);
            let seen = lookups();
            shared.run_due_name_checks(now);
            shared.note_pane_output(pane, &terminal, now);
            if lookups() != seen {
                checks.push((frame, lookups() - seen));
            }
            thread::sleep(Duration::from_millis(2));
        }
        shared.run_due_name_checks(start + Duration::from_secs(1));
        assert_eq!(checks, vec![(0, 1), (50, 1)]);
        assert_eq!(lookups() - before, 3);
        shared.request_shutdown();
    }

    #[test]
    fn a_frame_without_a_due_check_reads_no_process_facts() {
        let shared = Arc::new(Shared::new(1));
        let (pane, terminal) = printing_pane(&shared, "edge-frames");
        let start = Instant::now() + Duration::from_hours(1);
        shared.note_pane_output(pane, &terminal, start);
        let before = lookups();
        for frame in 1..50u64 {
            shared.note_pane_output(pane, &terminal, start + Duration::from_millis(frame * 9));
        }
        assert_eq!(lookups(), before);
        shared.request_shutdown();
    }

    #[test]
    fn monitor_silence_waits_out_output_after_the_armed_deadline() {
        let shared = Arc::new(Shared::new(1));
        let (pane, terminal) = printing_pane(&shared, "edge-silence");
        let window = shared
            .inner
            .lock()
            .engine
            .state
            .window_for_pane(pane)
            .expect("window");
        let mut context = ExecutionContext::default();
        shared
            .execute(
                ClientId(u64::MAX),
                ClientKind::Command,
                &mut context,
                &CommandInvocation::new("set-window-option", ["-g", "monitor-silence", "1"]),
            )
            .expect("monitor silence");
        let armed = shared.inner.lock().silence_deadlines[&window];
        let set = Instant::now();
        wait_until("output after the deadline was armed", || {
            terminal.last_output().is_some_and(|last| last > set)
        });
        shared.expire_window_silence(armed, armed.deadline);
        let rearmed = shared.inner.lock().silence_deadlines[&window];
        assert_ne!(rearmed.token, armed.token);
        assert!(rearmed.deadline > armed.deadline);
        assert!(
            rearmed.deadline <= terminal.last_output().expect("output") + Duration::from_secs(1)
        );
        assert!(!shared.inner.lock().engine.state.windows[&window].silence_flag);
        shared.request_shutdown();
    }
}
