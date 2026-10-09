use zz_terminal::{KeyCode, KeyInput, Modifiers};

use super::tests::take_reliable_messages;
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
    outbound: Arc<OutboundMailbox>,
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
    let outbound = OutboundMailbox::new();
    let (client, _) = shared.register_subscribed(
        ClientKind::Interactive,
        Some("chooser".to_owned()),
        None,
        Arc::clone(&outbound),
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
        outbound,
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
        self.send(input, buffer).unwrap();
    }

    fn send(&mut self, input: KeyInput, buffer: bool) -> Result<(), DaemonError> {
        let message = if buffer {
            InputMessage::ChooseBuffer {
                action: ChooseBufferAction::Key(input),
            }
        } else {
            InputMessage::ChooseTree {
                action: ChooseTreeAction::Key(input),
            }
        };
        self.shared.input(
            self.client,
            ClientKind::Interactive,
            &mut self.context,
            message,
        )
    }

    fn search_append(&mut self, text: &str) {
        self.shared
            .input(
                self.client,
                ClientKind::Interactive,
                &mut self.context,
                InputMessage::ChooseTree {
                    action: ChooseTreeAction::SearchAppend(text.to_owned()),
                },
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

    fn split(&self) -> (PaneId, PaneId) {
        let mut inner = self.shared.inner.lock();
        let source = inner.engine.state.windows[&self.windows[0]].active_pane;
        let other = inner
            .engine
            .state
            .split_pane(source, zz_protocol::Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        inner.engine.state.select_pane(source).unwrap();
        (source, other)
    }

    fn active_pane(&self) -> PaneId {
        self.shared.inner.lock().engine.state.windows[&self.windows[0]].active_pane
    }

    fn tree_events(&self) -> Vec<Option<ChooseTreeState>> {
        take_reliable_messages(&self.outbound)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload: EventPayload::ChooseTree { state },
                    ..
                }) => Some(state),
                _ => None,
            })
            .collect()
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

#[test]
fn a_window_switch_leaves_the_tree_behind_until_the_client_returns() {
    let mut scene = attached_scene();
    scene.open("choose-tree", &["-Zw"]);
    let selected = scene.tree_selected().expect("tree chooser open");
    scene.press(key('b', control()), false);
    scene.press(key('n', Modifiers::default()), false);
    assert_eq!(scene.current_window(), scene.windows[1]);
    let _ = scene.send(key('j', Modifiers::default()), false);
    assert_eq!(scene.tree_selected(), Some(selected));

    scene.press(key('b', control()), false);
    scene.press(key('p', Modifiers::default()), false);
    assert_eq!(scene.current_window(), scene.windows[0]);
    scene.press(key('j', Modifiers::default()), false);
    assert_ne!(scene.tree_selected(), Some(selected));
}

#[test]
fn another_pane_in_the_window_takes_keys_until_the_tree_pane_is_back() {
    let mut scene = attached_scene();
    let (source, other) = scene.split();
    scene.open("choose-tree", &["-Zw"]);
    let selected = scene.tree_selected().expect("tree chooser open");
    scene.tree_events();
    scene.press(key('b', control()), false);
    scene.press(key('o', Modifiers::default()), false);
    assert_eq!(scene.active_pane(), other);
    assert_eq!(scene.tree_events().last(), Some(&None));
    let _ = scene.send(key('x', Modifiers::default()), false);
    let _ = scene.send(key('y', Modifiers::default()), false);
    {
        let inner = scene.shared.inner.lock();
        assert!(inner.clients[&scene.client].command_prompt.is_none());
        assert_eq!(inner.engine.state.sessions[&scene.session].windows.len(), 2);
        assert_eq!(
            inner.engine.state.windows[&scene.windows[0]]
                .pane_order()
                .len(),
            2
        );
    }
    assert_eq!(scene.tree_selected(), Some(selected));

    scene.press(key('b', control()), false);
    scene.press(key('o', Modifiers::default()), false);
    assert_eq!(scene.active_pane(), source);
    assert!(matches!(scene.tree_events().last(), Some(Some(_))));
    scene.press(key('j', Modifiers::default()), false);
    assert_ne!(scene.tree_selected(), Some(selected));
}

#[test]
fn display_panes_raised_in_the_tree_takes_keys_and_leaves_the_tree_open() {
    let mut scene = attached_scene();
    scene.open("choose-tree", &["-Zw"]);
    let selected = scene.tree_selected().expect("tree chooser open");
    scene.press(key('b', control()), false);
    scene.press(key('q', Modifiers::default()), false);
    let display_panes = |scene: &Scene| {
        scene.shared.inner.lock().clients[&scene.client]
            .display_panes
            .is_some()
    };
    assert!(display_panes(&scene));
    assert_eq!(scene.tree_selected(), Some(selected));
    scene.press(key('0', Modifiers::default()), false);
    assert!(!display_panes(&scene));
    assert_eq!(scene.tree_selected(), Some(selected));
    assert_eq!(scene.current_window(), scene.windows[0]);
}

#[test]
fn send_prefix_inside_the_tree_hands_the_prefix_to_the_tree() {
    let mut scene = attached_scene();
    scene.open("choose-tree", &["-Zw"]);
    scene.press(key('j', Modifiers::default()), false);
    scene.press(key('j', Modifiers::default()), false);
    let moved = scene.tree_selected().expect("tree chooser open");
    scene.press(key('b', control()), false);
    scene.press(key('b', control()), false);
    assert_eq!(scene.active_table(), None);
    assert_ne!(scene.tree_selected(), Some(moved));
}

#[test]
fn search_text_typed_after_the_prefix_runs_the_prefix_binding() {
    let mut scene = attached_scene();
    scene.open("choose-tree", &["-Zs"]);
    scene.press(key('/', Modifiers::default()), false);
    scene.press(key('b', control()), false);
    assert_eq!(scene.active_table().as_deref(), Some("prefix"));
    scene.search_append("d");
    assert!(!scene.attached());
}

impl Scene {
    fn presentation_sizes(&self) -> Vec<(ChooserPreviewSize, bool)> {
        take_reliable_messages(&self.outbound)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::ChooserPresentation {
                            presentation: Some(presentation),
                        },
                    ..
                }) => Some((presentation.preview_size, presentation.preview.is_some())),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn preview_flags_open_each_chooser_off_or_big_and_v_cycles_from_there() {
    for (command, flags, opened, cycled) in [
        (
            "choose-tree",
            &[][..],
            ChooserPreviewSize::Normal,
            ChooserPreviewSize::Off,
        ),
        (
            "choose-tree",
            &["-N"][..],
            ChooserPreviewSize::Off,
            ChooserPreviewSize::Big,
        ),
        (
            "choose-tree",
            &["-NN"][..],
            ChooserPreviewSize::Big,
            ChooserPreviewSize::Normal,
        ),
        (
            "choose-client",
            &["-N"][..],
            ChooserPreviewSize::Off,
            ChooserPreviewSize::Big,
        ),
        (
            "choose-client",
            &["-NN"][..],
            ChooserPreviewSize::Big,
            ChooserPreviewSize::Normal,
        ),
        (
            "choose-buffer",
            &["-N"][..],
            ChooserPreviewSize::Off,
            ChooserPreviewSize::Big,
        ),
        (
            "choose-buffer",
            &["-NN"][..],
            ChooserPreviewSize::Big,
            ChooserPreviewSize::Normal,
        ),
    ] {
        let mut scene = attached_scene();
        scene.open("set-buffer", &["-b", "alpha", "alpha"]);
        scene.presentation_sizes();
        let buffer = command == "choose-buffer";
        scene.open(command, flags);
        let (size, preview) = *scene.presentation_sizes().last().expect("opened");
        assert_eq!(size, opened, "{command} {flags:?} opens");
        assert_eq!(
            preview,
            opened != ChooserPreviewSize::Off,
            "{command} {flags:?} builds a preview only when it draws one"
        );
        scene.press(key('v', Modifiers::default()), buffer);
        let (size, preview) = *scene.presentation_sizes().last().expect("cycled");
        assert_eq!(size, cycled, "{command} {flags:?} then v");
        assert_eq!(preview, cycled != ChooserPreviewSize::Off);
    }
}

#[test]
fn snapshot_publishes_resend_a_chooser_only_when_it_changed() {
    let chooser_events = |scene: &Scene| {
        take_reliable_messages(&scene.outbound)
            .into_iter()
            .filter(|message| {
                matches!(
                    message,
                    ProtocolMessage::Event(Event {
                        payload: EventPayload::ChooseTree { .. }
                            | EventPayload::ChooseTreeUpdate { .. }
                            | EventPayload::ChooseBuffer { .. }
                            | EventPayload::ChooseBufferUpdate { .. }
                            | EventPayload::ChooserPresentation { .. },
                        ..
                    })
                )
            })
            .count()
    };
    for (command, buffer) in [("choose-tree", false), ("choose-buffer", true)] {
        let mut scene = attached_scene();
        scene.open("set-buffer", &["-b", "alpha", "alpha"]);
        scene.open(command, &[]);
        scene.shared.publish_snapshot();
        chooser_events(&scene);
        for _ in 0..3 {
            scene.shared.publish_snapshot();
        }
        assert_eq!(chooser_events(&scene), 0, "{command} unchanged");
        if buffer {
            scene.open("set-buffer", &["-b", "beta", "beta"]);
        } else {
            scene.open("rename-window", &["-t", "two", "renamed"]);
        }
        scene.shared.publish_snapshot();
        assert!(chooser_events(&scene) > 0, "{command} changed");
        scene.shared.publish_snapshot();
        assert_eq!(chooser_events(&scene), 0, "{command} settled");
        scene.press(key('j', Modifiers::default()), buffer);
        assert!(chooser_events(&scene) > 0, "{command} moved");
    }
}

#[test]
fn chooser_prompts_take_the_session_message_style_and_prompt_cursor() {
    let mut scene = attached_scene();
    scene.open("choose-tree", &[]);
    let presentation = |scene: &Scene| {
        take_reliable_messages(&scene.outbound)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::ChooserPresentation {
                            presentation: Some(presentation),
                        },
                    ..
                }) => Some(presentation),
                _ => None,
            })
            .last()
            .expect("a presentation")
    };
    let default = presentation(&scene);
    assert!(
        default
            .prompt_style
            .starts_with("bg=themeyellow,fg=themeblack")
    );
    assert!(!default.prompt_style.contains("fill="));
    assert_eq!(default.prompt_cursor, zz_protocol::PromptCursor::default());
    scene.press(key('q', Modifiers::default()), false);
    for (name, value) in [
        ("message-style", "bg=blue,fg=white"),
        ("prompt-cursor-style", "bar"),
        ("prompt-cursor-colour", "red"),
    ] {
        scene.open("set-option", &["-t", "keys", name, value]);
    }
    scene.open("choose-tree", &[]);
    let styled = presentation(&scene);
    assert_eq!(styled.prompt_style, "bg=blue,fg=white");
    assert_eq!(styled.prompt_cursor.style, 6);
    assert_eq!(
        styled.prompt_cursor.colour,
        zz_protocol::parse_tmux_colour("red")
    );
}

#[test]
fn a_full_chooser_state_is_always_followed_by_its_presentation() {
    for (command, buffer) in [("choose-tree", false), ("choose-buffer", true)] {
        let mut scene = attached_scene();
        scene.open("set-buffer", &["-b", "alpha", "alpha"]);
        scene.open(command, &["-N"]);
        take_reliable_messages(&scene.outbound);
        scene.press(key('f', Modifiers::default()), buffer);
        let mut cleared = false;
        let mut full = false;
        for message in take_reliable_messages(&scene.outbound) {
            match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::ChooseTree { state: Some(_) }
                        | EventPayload::ChooseBuffer { state: Some(_) },
                    ..
                }) => {
                    cleared = true;
                    full = true;
                }
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::ChooserPresentation {
                            presentation: Some(_),
                        },
                    ..
                }) => cleared = false,
                _ => {}
            }
        }
        assert!(full, "{command} f sends the full state");
        assert!(!cleared, "{command} f leaves the client a presentation");
    }
}

#[test]
fn chooser_prompt_styles_see_the_prompt_type_and_input() {
    for (typed, style) in [('x', "bg=blue"), ('/', "bg=red")] {
        let mut scene = attached_scene();
        scene.open(
            "set-option",
            &[
                "-t",
                "keys",
                "message-style",
                "bg=#{?#{==:#{prompt_type},search},red,blue}",
            ],
        );
        scene.open("choose-tree", &[]);
        scene.press(key(typed, Modifiers::default()), false);
        let last = take_reliable_messages(&scene.outbound)
            .into_iter()
            .filter_map(|message| match message {
                ProtocolMessage::Event(Event {
                    payload:
                        EventPayload::ChooserPresentation {
                            presentation: Some(presentation),
                        },
                    ..
                }) => Some(presentation.prompt_style),
                _ => None,
            })
            .last();
        assert_eq!(last.as_deref(), Some(style), "{typed}");
    }
}
