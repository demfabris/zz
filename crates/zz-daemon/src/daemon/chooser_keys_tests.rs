use zz_terminal::{KeyCode, KeyInput, Modifiers};

use super::*;

fn key(character: char, modifiers: Modifiers) -> KeyInput {
    let text = character.to_string();
    tests::test_key(
        KeyCode::Character(character),
        modifiers,
        (modifiers == Modifiers::default()).then_some(text.as_str()),
    )
}

fn control() -> Modifiers {
    Modifiers::new(false, true, false, false)
}

struct Scene {
    shared: Arc<Shared>,
    client: ClientId,
    context: ExecutionContext,
    session: SessionId,
    windows: [WindowId; 2],
}

fn attached_scene() -> Scene {
    let shared = Arc::new(Shared::new(7));
    let (session, windows, pane) = {
        let mut inner = shared.inner.lock();
        let (session, first, pane) = inner.engine.state.create_session("keys").unwrap();
        let (second, _) = inner
            .engine
            .state
            .create_window(session, Some("two".to_owned()), PaneKind::Terminal)
            .unwrap();
        (session, [first, second], pane)
    };
    let context = ExecutionContext::for_pane(&shared.inner.lock().engine.state, pane).unwrap();
    let (client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some("chooser".to_owned()),
        None,
        OutboundMailbox::new(),
    );
    shared.attach(client, session).unwrap();
    shared
        .inner
        .lock()
        .engine
        .state
        .select_window(session, windows[0])
        .unwrap();
    Scene {
        shared,
        client,
        context,
        session,
        windows,
    }
}

impl Scene {
    fn open(&mut self, command: &str, args: &[&str]) {
        self.shared
            .execute(
                self.client,
                ClientKind::Interactive,
                &mut self.context,
                &CommandInvocation::new(command, args.iter().copied()),
            )
            .unwrap();
    }

    fn press(&mut self, input: KeyInput, buffer: bool) {
        let message = if buffer {
            InputMessage::ChooseBuffer {
                action: ChooseBufferAction::Key(input),
            }
        } else {
            InputMessage::ChooseTree {
                action: ChooseTreeAction::Key(input),
            }
        };
        self.shared
            .input(
                self.client,
                ClientKind::Interactive,
                &mut self.context,
                message,
            )
            .unwrap();
    }

    fn attached(&self) -> bool {
        client_attached_session(&self.shared.inner.lock(), self.client) == Some(self.session)
    }

    fn current_window(&self) -> WindowId {
        self.shared.inner.lock().engine.state.sessions[&self.session].active_window
    }

    fn active_table(&self) -> Option<String> {
        self.shared.inner.lock().clients[&self.client]
            .key_engine
            .as_ref()
            .and_then(KeyEngine::active_table)
            .map(str::to_owned)
    }

    fn tree_selected(&self) -> Option<u32> {
        self.shared.inner.lock().clients[&self.client]
            .choose_tree
            .as_ref()
            .map(|chooser| chooser.rendered.selected)
    }
}

#[test]
fn prefix_bindings_run_while_the_tree_chooser_is_open() {
    let mut scene = attached_scene();
    scene.open("choose-tree", &["-Zw"]);
    let selected = scene.tree_selected().expect("tree chooser open");
    scene.press(key('j', Modifiers::default()), false);
    let moved = scene.tree_selected().expect("tree chooser still open");
    assert_ne!(moved, selected);

    scene.press(key('b', control()), false);
    assert_eq!(scene.active_table().as_deref(), Some("prefix"));
    assert_eq!(scene.tree_selected(), Some(moved));
    scene.press(key('n', Modifiers::default()), false);
    assert_eq!(scene.current_window(), scene.windows[1]);
    assert_eq!(scene.active_table(), None);
    assert_eq!(scene.tree_selected(), Some(moved));

    scene.press(key('b', control()), false);
    scene.press(key('d', Modifiers::default()), false);
    assert!(!scene.attached());
}

#[test]
fn prefix_bindings_run_while_a_tree_prompt_is_open() {
    let mut scene = attached_scene();
    scene.open("choose-tree", &["-Zs"]);
    scene.press(key('/', Modifiers::default()), false);
    scene.press(key('k', Modifiers::default()), false);
    assert!(
        scene.shared.inner.lock().clients[&scene.client]
            .choose_tree
            .as_ref()
            .is_some_and(|chooser| chooser.search.is_some())
    );
    scene.press(key('b', control()), false);
    scene.press(key('d', Modifiers::default()), false);
    assert!(!scene.attached());
}

#[test]
fn prefix_bindings_run_while_the_buffer_chooser_is_open() {
    let mut scene = attached_scene();
    scene.open("set-buffer", &["-b", "alpha", "alpha"]);
    scene.open("choose-buffer", &["-Z"]);
    assert!(
        scene.shared.inner.lock().clients[&scene.client]
            .choose_buffer
            .is_some()
    );
    scene.press(key('b', control()), true);
    assert_eq!(scene.active_table().as_deref(), Some("prefix"));
    scene.press(key('d', Modifiers::default()), true);
    assert!(!scene.attached());
}
