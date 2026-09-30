#![cfg(unix)]

use super::*;

struct CopyFixture {
    shared: Arc<Shared>,
    client: ClientId,
    context: ExecutionContext,
    pane: PaneId,
    terminal: Arc<TerminalSession>,
}

fn empty_pane(shared: &Arc<Shared>, name: &str) -> (SessionId, PaneId, Arc<TerminalSession>) {
    let (session, _, pane) = shared
        .inner
        .lock()
        .engine
        .state
        .create_session(name)
        .expect("session");
    let terminal = Arc::new(TerminalSession::spawn_empty_with_appearance(
        32,
        Arc::new(TerminalAppearance::default()),
    ));
    shared
        .inner
        .lock()
        .terminals_mut()
        .insert(pane, Arc::clone(&terminal));
    shared.watch_terminal(pane, &terminal).expect("watch pane");
    (session, pane, terminal)
}

impl CopyFixture {
    fn new(name: &str) -> Self {
        let shared = Arc::new(Shared::new(1));
        let (session, pane, terminal) = empty_pane(&shared, name);
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());
        shared.attach(client, session).expect("attach copy client");
        terminal.resize(80, 24, 8, 18);
        assert!(terminal.settle());
        let context = ExecutionContext::for_pane(&shared.inner.lock().engine.state, pane)
            .expect("pane context");
        Self {
            shared,
            client,
            context,
            pane,
            terminal,
        }
    }

    fn run(&mut self, command: &[&str]) -> String {
        self.shared
            .execute(
                self.client,
                ClientKind::Interactive,
                &mut self.context,
                &CommandInvocation::new(command[0], command[1..].iter().copied()),
            )
            .unwrap_or_else(|error| panic!("{command:?}: {error:?}"))
            .output
            .to_string()
    }

    fn action(&mut self, action: &str) {
        let target = self.pane.to_string();
        self.run(&["send-keys", "-t", &target, "-X", action]);
        assert!(self.terminal.settle());
    }

    fn facts(&self) -> Arc<zz_terminal::CopyModeFacts> {
        self.terminal
            .copy_mode_facts(TerminalViewId(self.client.0))
            .expect("copy facts")
    }

    fn enter(&mut self, needle: &str) {
        let target = self.pane.to_string();
        self.run(&["copy-mode", "-t", &target]);
        self.run(&["send-keys", "-t", &target, "-X", "search-backward", needle]);
        self.action("start-of-line");
    }

    fn select_prefix(&mut self) {
        self.action("begin-selection");
        let target = self.pane.to_string();
        self.run(&["send-keys", "-t", &target, "-N", "5", "-X", "cursor-right"]);
        assert!(self.terminal.settle());
        assert!(self.facts().selection.is_some());
    }

    fn capture(&mut self) -> String {
        let target = self.pane.to_string();
        self.run(&["capture-pane", "-p", "-t", &target, "-S", "-"])
    }

    fn copy_prefix(&mut self, expected: &str) {
        let previous = self.shared.inner.lock().next_buffer_id;
        self.action("copy-selection-no-clear");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let ready = {
                let inner = self.shared.inner.lock();
                inner.next_buffer_id != previous
                    && inner
                        .paste_buffers
                        .first()
                        .is_some_and(|buffer| buffer.data.as_ref() == expected.as_bytes())
            };
            if ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "copy event never created buffer {expected:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(self.run(&["show-buffer"]), expected);
    }

    fn resize_narrow(&self) {
        let native = self.terminal.fresh_viewport();
        let columns = if native.columns == 40 { 41 } else { 40 };
        self.terminal.resize(columns, 16, 8, 18);
        assert!(self.terminal.settle());
        let actual = self.terminal.fresh_viewport();
        assert_eq!((actual.columns, actual.rows), (columns, 16));
        let frozen = self
            .terminal
            .latest_viewport_for(TerminalViewId(self.client.0))
            .expect("resized copy viewport");
        assert_eq!(
            (frozen.columns, frozen.rows),
            (columns, 16),
            "copy backing did not follow native resize from {}x{}",
            native.columns,
            native.rows,
        );
    }
}

fn feed(terminal: &TerminalSession, bytes: impl Into<Vec<u8>>) {
    assert!(terminal.feed(Arc::from(bytes.into())));
    assert!(terminal.settle());
}

fn lines(start: usize, end: usize) -> String {
    (start..=end)
        .map(|number| {
            format!(
                "F{number:04} payload-{number:04} abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n"
            )
        })
        .collect()
}

#[test]
fn copy_pages_keep_cursor_selection_and_bytes_after_pruning_ed3_and_resize() {
    let mut fixture = CopyFixture::new("copy-page-prune");
    feed(&fixture.terminal, lines(1, 128));
    fixture.enter("F0100");
    fixture.select_prefix();
    let original = fixture.facts();
    assert!(original.cursor_line.starts_with("F0100 "));

    feed(&fixture.terminal, lines(129, 4096));
    assert!(!fixture.capture().contains("F0100 "));
    assert_eq!(fixture.facts(), original);
    feed(&fixture.terminal, b"\x1b[3JCLEARED-HISTORY\r\n".to_vec());
    assert_eq!(fixture.facts(), original);
    fixture.copy_prefix("F0100");
    assert_eq!(fixture.facts(), original);

    fixture.resize_narrow();
    let resized = fixture.facts();
    assert!(resized.cursor_line.starts_with("F0100 "));
    assert_eq!(resized.cursor_word, "payload");
    assert!(resized.selection.is_none());
    fixture.action("start-of-line");
    fixture.select_prefix();
    fixture.copy_prefix("F0100");

    let target = fixture.pane.to_string();
    fixture.run(&["clear-history", "-t", &target]);
    assert!(fixture.terminal.settle());
    assert!(
        fixture
            .terminal
            .copy_mode_facts(TerminalViewId(fixture.client.0))
            .is_none()
    );
}

#[test]
fn sourced_copy_pages_survive_source_pruning_ed3_and_resize() {
    let mut fixture = CopyFixture::new("copy-page-target");
    let (_, source_pane, source) = empty_pane(&fixture.shared, "copy-page-source");
    source.resize(80, 24, 8, 18);
    feed(&source, lines(1, 128));
    feed(&fixture.terminal, b"TARGET-CONTENT\r\n".to_vec());
    let target = fixture.pane.to_string();
    let source_target = source_pane.to_string();
    fixture.run(&["copy-mode", "-s", &source_target, "-t", &target]);
    fixture.run(&["send-keys", "-t", &target, "-X", "search-backward", "F0100"]);
    fixture.action("start-of-line");
    fixture.select_prefix();
    let original = fixture.facts();
    assert!(original.cursor_line.starts_with("F0100 "));

    feed(&source, lines(129, 4096));
    feed(&source, b"\x1b[H\x1b[2J\x1b[3JSOURCE-REPLACED\r\n".to_vec());
    source.resize(40, 16, 8, 18);
    assert!(source.settle());
    assert_eq!(fixture.facts(), original);
    assert!(
        source
            .copy_mode_facts(TerminalViewId(fixture.client.0))
            .is_none()
    );
    fixture.copy_prefix("F0100");
    fixture.action("cancel");
    assert!(fixture.capture().contains("TARGET-CONTENT"));
    assert!(!fixture.capture().contains("SOURCE-REPLACED"));
}

fn retained_fixture(name: &str) -> CopyFixture {
    let shared = Arc::new(Shared::new(1));
    let mut context = ExecutionContext::default();
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new("set-option", ["-g", "remain-on-exit", "on"]),
        )
        .expect("retain dead pane");
    shared
        .execute(
            ClientId(u64::MAX),
            ClientKind::Command,
            &mut context,
            &CommandInvocation::new(
                "new-session",
                [
                    "-d",
                    "-s",
                    name,
                    "n=1; while [ $n -le 200 ]; do printf 'D%04d retained\\n' $n; n=$((n + 1)); done; exit 9",
                ],
            ),
        )
        .expect("exiting pane");
    let pane = context.pane.expect("dead pane");
    let terminal = Arc::clone(&shared.inner.lock().terminals[&pane]);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !shared
        .inner
        .lock()
        .engine
        .state
        .pane(pane)
        .is_some_and(|pane| pane.dead && pane.dead_status == Some(9))
    {
        assert!(Instant::now() < deadline, "pane never became retained-dead");
        thread::sleep(Duration::from_millis(10));
    }
    let (client, _) =
        shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());
    shared
        .attach(client, context.session.expect("session"))
        .expect("attach retained pane");
    CopyFixture {
        shared,
        client,
        context,
        pane,
        terminal,
    }
}

#[test]
fn retained_dead_pages_allow_history_capture_search_copy_and_resize() {
    let mut fixture = retained_fixture("copy-page-retained");
    assert!(fixture.capture().contains("D0180 retained"));
    fixture.enter("D0180");
    assert_eq!(fixture.facts().cursor_word, "D0180");
    fixture.select_prefix();
    fixture.copy_prefix("D0180");
    fixture.resize_narrow();
    assert!(fixture.facts().cursor_line.starts_with("D0180 retained"));
    fixture.action("cancel");
    assert!(fixture.capture().contains("D0180 retained"));
}

#[test]
fn retained_dead_target_keeps_source_pages_and_its_own_search_state() {
    let mut fixture = retained_fixture("copy-page-retained-source-target");
    let (_, source_pane, source) = empty_pane(&fixture.shared, "copy-page-retained-source");
    source.resize(80, 24, 8, 18);
    feed(&source, lines(1, 128));
    let target = fixture.pane.to_string();
    let source_target = source_pane.to_string();
    fixture.run(&["copy-mode", "-s", &source_target, "-t", &target]);
    assert!(fixture.terminal.settle());
    assert!(fixture.facts().cursor_line.starts_with("F0128 "));
    fixture.run(&["send-keys", "-t", &target, "-X", "search-backward", "F0100"]);
    fixture.action("start-of-line");
    assert!(fixture.facts().cursor_line.starts_with("F0100 "));
    assert_eq!(fixture.terminal.pane_search_string(), "F0100");
    assert_eq!(source.pane_search_string(), "");
    fixture.select_prefix();
    let original = fixture.facts();
    feed(&source, lines(129, 4096));
    feed(&source, b"\x1b[H\x1b[2J\x1b[3JSOURCE-REPLACED\r\n".to_vec());
    source.resize(40, 16, 8, 18);
    assert!(source.settle());
    assert_eq!(fixture.facts(), original);
    fixture.copy_prefix("F0100");
    fixture.resize_narrow();
    assert!(fixture.facts().cursor_line.starts_with("F0100 "));
    assert!(fixture.facts().selection.is_none());
    fixture.action("cancel");
    assert_eq!(fixture.terminal.pane_search_string(), "F0100");
    assert!(fixture.capture().contains("D0180 retained"));
    assert!(!fixture.capture().contains("SOURCE-REPLACED"));
}
