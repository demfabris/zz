use std::cell::RefCell;

use gpui::{TestAppContext, VisualTestContext};
use zz_protocol::{ChooseTreeItem, ChooseTreeKind, SessionId, WindowId};

use super::*;
use crate::{
    ActiveTheme as _, Colorize as _, IconName, Root,
    command::{
        PaletteStatus, PaletteTreeHost, PaletteTreePane, PaletteTreeSession, PaletteTreeWindow,
    },
};

const LOCAL: PaletteHostId = PaletteHostId(0);

#[derive(Debug, PartialEq)]
enum Call {
    Input(InputMessage),
    Key(PaneId, String),
    Execute(PaletteHostId, CommandInvocation),
    Connect(PaletteHostId),
    Activate(PaletteHostId, PaletteTarget),
}

type Calls = Rc<RefCell<Vec<Call>>>;

struct FakeBackend {
    calls: Calls,
    snapshot: Arc<MuxSnapshot>,
    tree: PaletteTree,
    availability: PaneKindAvailability,
}

impl PaletteBackend for FakeBackend {
    fn snapshot(&self, _: &App) -> Arc<MuxSnapshot> {
        Arc::clone(&self.snapshot)
    }

    fn host_snapshot<'a>(&self, _: PaletteHostId, _: &'a App) -> Option<&'a MuxSnapshot> {
        None
    }

    fn tree(&self, _: &App) -> PaletteTree {
        self.tree.clone()
    }

    fn settings(&self, _: &App) -> PaletteSettings {
        PaletteSettings::default()
    }

    fn availability(&self, _: &App) -> PaneKindAvailability {
        self.availability
    }

    fn command_shortcut(&self, _: &str, _: &App) -> Option<SharedString> {
        None
    }

    fn mono_font(&self, _: &App) -> SharedString {
        "Lilex".into()
    }

    fn send_input(&self, input: InputMessage, _: &mut App) {
        self.calls.borrow_mut().push(Call::Input(input));
    }

    fn send_key(&self, pane: PaneId, event: &KeyDownEvent, _: &mut App) {
        self.calls
            .borrow_mut()
            .push(Call::Key(pane, event.keystroke.key.clone()));
    }

    fn active_pane(&self, _: &App) -> Option<PaneId> {
        Some(PaneId(0))
    }

    fn execute(&self, host: PaletteHostId, command: CommandInvocation, _: &mut App) {
        self.calls.borrow_mut().push(Call::Execute(host, command));
    }

    fn connect_host(&self, host: PaletteHostId, _: &mut App) {
        self.calls.borrow_mut().push(Call::Connect(host));
    }

    fn activate(&self, host: PaletteHostId, target: PaletteTarget, _: &mut App) -> bool {
        self.calls.borrow_mut().push(Call::Activate(host, target));
        true
    }
}

fn fake_backend(
    tree: PaletteTree,
    availability: PaneKindAvailability,
) -> (Rc<dyn PaletteBackend>, Calls) {
    let calls = Calls::default();
    let backend = FakeBackend {
        calls: Rc::clone(&calls),
        snapshot: Arc::default(),
        tree,
        availability,
    };
    (Rc::new(backend), calls)
}

fn backend(tree: PaletteTree) -> (Rc<dyn PaletteBackend>, Calls) {
    fake_backend(
        tree,
        PaneKindAvailability {
            browser: true,
            agent: true,
            editor: true,
        },
    )
}

fn local_host(status: PaletteStatus, sessions: Vec<PaletteTreeSession>) -> PaletteTree {
    PaletteTree {
        attached: LOCAL,
        hosts: vec![PaletteTreeHost {
            id: LOCAL,
            name: "local".to_owned(),
            detail: "This computer".to_owned(),
            right: String::new(),
            status,
            sessions,
        }],
    }
}

fn offline_tree() -> PaletteTree {
    local_host(PaletteStatus::Offline, Vec::new())
}

fn online_tree() -> PaletteTree {
    local_host(
        PaletteStatus::Online,
        vec![PaletteTreeSession {
            id: SessionId(1),
            name: "dev".to_owned(),
            active: true,
            windows: vec![PaletteTreeWindow {
                id: WindowId(1),
                name: "editor".to_owned(),
                active: true,
                active_pane: PaneId(10),
                panes: vec![PaletteTreePane {
                    id: PaneId(10),
                    label: "nvim".to_owned(),
                    detail: "Terminal".into(),
                    icon: IconName::SquareTerminal,
                    running: false,
                }],
            }],
        }],
    )
}

fn sent(calls: &Calls, action: &ChooseTreeAction) -> bool {
    calls.borrow().iter().any(|call| {
        matches!(call, Call::Input(InputMessage::ChooseTree { action: sent }) if sent == action)
    })
}

fn mount(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<CommandPaletteView>) -> CommandPaletteView,
) -> (Entity<CommandPaletteView>, &mut VisualTestContext) {
    let slot = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&slot);
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let palette = cx.new(|cx| build(window, cx));
        palette.read(cx).focus(cx).focus(window, cx);
        captured.replace(Some(palette.clone()));
        Root::new(palette, window, cx)
    });
    let palette = slot.borrow_mut().take().expect("palette captured");
    (palette, cx)
}

fn prompt_state(input: &str, mode: CommandPromptMode) -> CommandPromptState {
    CommandPromptState {
        prompt: ":".to_owned(),
        input: input.to_owned(),
        cursor: u32::try_from(input.chars().count()).unwrap_or(u32::MAX),
        kind: CommandPromptKind::Command,
        history: Vec::new(),
        prompt_type: CommandPromptType::Command,
        mode,
        no_freeze: false,
        pane: None,
    }
}

#[test]
fn chooser_search_expands_descendants_and_restores_only_its_own_branches() {
    let item = |target, flags| ChooseTreeItem {
        label: String::new(),
        detail: String::new(),
        target,
        depth: 0,
        flags,
        pane_kind: None,
        key: String::new(),
        text: String::new(),
    };
    let mut state = ChooseTreeState {
        items: vec![
            item(
                ChooseTreeTarget::Session(SessionId(0)),
                ChooseTreeItem::HAS_CHILDREN | ChooseTreeItem::EXPANDED,
            ),
            item(
                ChooseTreeTarget::Window(WindowId(0)),
                ChooseTreeItem::HAS_CHILDREN,
            ),
            item(
                ChooseTreeTarget::Session(SessionId(1)),
                ChooseTreeItem::HAS_CHILDREN,
            ),
        ],
        search: None,
        selected: 0,
        kind: ChooseTreeKind::Windows,
        filter_no_matches: false,
        prompt: String::new(),
        help: false,
    };
    let mut search = ChooserSearch::default();
    assert_eq!(search.advance(&state, true), Some((1, true)));
    state.selected = 1;
    assert_eq!(search.advance(&state, true), None);
    state.items[1].flags |= ChooseTreeItem::EXPANDED;
    state
        .items
        .insert(2, item(ChooseTreeTarget::Pane(PaneId(0)), 0));
    assert_eq!(search.advance(&state, true), Some((3, true)));
    state.items[3].flags |= ChooseTreeItem::EXPANDED;
    state.items.push(item(
        ChooseTreeTarget::Window(WindowId(1)),
        ChooseTreeItem::HAS_CHILDREN,
    ));
    assert_eq!(search.advance(&state, true), Some((4, true)));
    state.items[4].flags |= ChooseTreeItem::EXPANDED;
    assert_eq!(search.advance(&state, true), None);
    assert_eq!(search.advance(&state, false), Some((4, false)));
    assert_eq!(search.advance(&state, false), None);
    state.items[4].flags &= !ChooseTreeItem::EXPANDED;
    assert_eq!(search.advance(&state, false), Some((3, false)));
    state.items[3].flags &= !ChooseTreeItem::EXPANDED;
    state.items.pop();
    assert_eq!(search.advance(&state, false), Some((1, false)));
    state.items[1].flags &= !ChooseTreeItem::EXPANDED;
    state.items.remove(2);
    assert_eq!(search.advance(&state, false), None);
    assert!(search.expanded.is_empty());
    assert!(state.items[0].expanded());
}

#[gpui::test]
fn daemon_tree_navigation_and_search_preserve_pane_activation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::init(cx);
        cx.set_reduce_motion(false);
    });
    let session = ChooseTreeItem {
        label: "workspace".to_owned(),
        detail: "1 window".to_owned(),
        target: ChooseTreeTarget::Session(SessionId(0)),
        depth: 0,
        flags: ChooseTreeItem::HAS_CHILDREN | ChooseTreeItem::ACTIVE,
        pane_kind: None,
        key: String::new(),
        text: String::new(),
    };
    let mut state = ChooseTreeState {
        items: vec![session.clone()],
        search: None,
        selected: 0,
        kind: ChooseTreeKind::Windows,
        filter_no_matches: false,
        prompt: String::new(),
        help: false,
    };
    let initial = state.clone();
    let (backend, messages) = backend(offline_tree());
    let (palette, cx) = cx.add_window_view(move |window, cx| {
        let palette = CommandPaletteView::new_window_chooser(backend, &initial, 1, window, cx);
        palette.focus(cx).focus(window, cx);
        palette
    });
    messages.borrow_mut().clear();
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    std::thread::sleep(Duration::from_millis(200));
    cx.update(Window::simulate_next_frame);
    cx.run_until_parked();
    let surface_top = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
            window
                .painted_quads()
                .into_iter()
                .find(|quad| {
                    quad.background
                        == gpui::solid_background(cx.theme().background.raised(2).opaque())
                        && quad.bounds.size.width.0 > 300.0 * window.scale_factor()
                })
                .expect("palette stays fully opaque after chooser updates")
                .bounds
                .origin
                .y
        })
    };
    let settled_top = surface_top(cx);
    palette.read_with(cx, |palette, _| {
        assert_eq!(palette.unified.as_ref().unwrap().rows.len(), 1);
        assert_eq!(palette.selected, Some(0));
    });
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!(sent(&messages, &ChooseTreeAction::Expand));
    state.items[0].flags |= ChooseTreeItem::EXPANDED;
    let mut child = session;
    child.label = "editor".to_owned();
    child.target = ChooseTreeTarget::Window(WindowId(0));
    child.depth = 1;
    state.items.push(child.clone());
    palette.update_in(cx, |palette, window, cx| {
        palette.synchronize_window_chooser(&state, 2, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(surface_top(cx), settled_top);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(
        palette.read_with(cx, |palette, _| palette.selected),
        Some(1)
    );
    messages.borrow_mut().clear();
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!(sent(&messages, &ChooseTreeAction::Select(1)));
    state.items[1].flags |= ChooseTreeItem::EXPANDED;
    child.label = "shell".to_owned();
    child.target = ChooseTreeTarget::Pane(PaneId(0));
    child.depth = 2;
    child.flags = ChooseTreeItem::ACTIVE;
    state.items.push(child);
    palette.update_in(cx, |palette, window, cx| {
        palette.synchronize_window_chooser(&state, 3, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(surface_top(cx), settled_top);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(
        palette.read_with(cx, |palette, _| palette.selected),
        Some(2)
    );
    cx.simulate_keystrokes("left");
    cx.run_until_parked();
    assert_eq!(
        palette.read_with(cx, |palette, _| palette.selected),
        Some(1)
    );
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!(cx.update(|window, cx| palette.read(cx).focus(cx).is_focused(window)));
    let mut other = state.items[0].clone();
    other.target = ChooseTreeTarget::Session(SessionId(1));
    other.flags = ChooseTreeItem::HAS_CHILDREN;
    other.label = "other".to_owned();
    state.items.insert(0, other);
    palette.update_in(cx, |palette, window, cx| {
        palette.synchronize_window_chooser(&state, 4, window, cx);
    });
    messages.borrow_mut().clear();
    cx.simulate_input("shell");
    cx.run_until_parked();
    assert!(sent(&messages, &ChooseTreeAction::Select(0)));
    cx.simulate_keystrokes("enter");
    assert!(!messages.borrow().iter().any(|message| matches!(
        message,
        Call::Input(InputMessage::ChooseTree {
            action: ChooseTreeAction::ActivateIndex(_)
        })
    )));
    state.items[0].flags |= ChooseTreeItem::EXPANDED;
    let mut other_window = state.items[2].clone();
    other_window.target = ChooseTreeTarget::Window(WindowId(1));
    other_window.flags = 0;
    state.items.insert(1, other_window);
    palette.update_in(cx, |palette, window, cx| {
        palette.synchronize_window_chooser(&state, 5, window, cx);
    });
    assert!(sent(&messages, &ChooseTreeAction::ActivateIndex(4)));
}

#[gpui::test]
fn palette_navigation_scrolls_at_most_one_row_per_step(cx: &mut TestAppContext) {
    cx.update(crate::init);
    for unified in [true, false] {
        let (backend, _) = backend(offline_tree());
        let (palette, cx) = mount(cx, move |window, cx| {
            let mut palette =
                CommandPaletteView::new_unified(backend, Some(PaletteMode::Command), window, cx);
            if !unified {
                palette.unified = None;
            }
            palette
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let mut previous_offset = px(0.0);
        let mut scrolled = false;
        for direction in [1, -1] {
            for _ in 0..24 {
                cx.update(|_, cx| {
                    palette.update(cx, |palette, cx| {
                        palette.navigate(direction, cx);
                    });
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    let _ = window.draw(cx);
                });
                let (offset, selected, row_count) = palette.read_with(cx, |palette, _| {
                    (
                        palette.scroll_handle.0.borrow().base_handle.offset().y,
                        palette.selected.expect("selected command"),
                        palette
                            .unified
                            .as_ref()
                            .map_or(palette.suggestions.len(), |state| state.rows.len()),
                    )
                });
                if direction < 0 && selected == row_count - 1 {
                    break;
                }
                let delta = (offset - previous_offset).abs();
                assert!(
                    delta <= px(COMMAND_PALETTE_ROW_HEIGHT + 0.01),
                    "unified={unified}, direction={direction}, selected={selected}, delta={delta:?}"
                );
                scrolled |= delta > px(0.0);
                previous_offset = offset;
            }
        }
        assert!(scrolled, "the command list should exceed the viewport");
    }
}

#[gpui::test]
fn unified_palette_modes_targets_and_backspace_keep_input_focus(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (backend, _) = backend(offline_tree());
    let (palette, cx) = mount(cx, move |window, cx| {
        CommandPaletteView::new_unified(backend, None, window, cx)
    });
    cx.simulate_input(":");
    cx.run_until_parked();
    assert_eq!(
        palette.read_with(cx, |palette, _| palette.unified.as_ref().unwrap().mode),
        Some(PaletteMode::Command)
    );
    cx.simulate_input("join");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    palette.read_with(cx, |palette, cx| {
        let state = palette.unified.as_ref().unwrap();
        assert_eq!(state.command.unwrap().name, "join-pane");
        assert_eq!(state.placeholder(), "Target window");
        assert!(palette.input.read(cx).value().is_empty());
    });
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    palette.read_with(cx, |palette, _| {
        let state = palette.unified.as_ref().unwrap();
        assert!(state.command.is_none());
        assert_eq!(state.mode, Some(PaletteMode::Command));
    });
    cx.simulate_keystrokes("up");
    cx.run_until_parked();
    palette.read_with(cx, |palette, _| {
        let state = palette.unified.as_ref().unwrap();
        assert!(state.rows.len() > 11);
        assert_eq!(palette.selected, Some(state.rows.len() - 1));
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    palette.read_with(cx, |palette, _| {
        assert!(palette.scroll_handle.is_scrollable());
        assert!(palette.scroll_handle.0.borrow().base_handle.offset().y < px(0.0));
    });
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert_eq!(
        palette.read_with(cx, |palette, _| palette.selected),
        Some(0)
    );
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(
        palette.read_with(cx, |palette, _| palette
            .scroll_handle
            .0
            .borrow()
            .base_handle
            .offset()
            .y),
        px(0.0)
    );
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    for (prefix, mode) in [
        ("@", PaletteMode::Window),
        ("%", PaletteMode::Pane),
        ("~", PaletteMode::Host),
    ] {
        cx.simulate_input(prefix);
        cx.run_until_parked();
        assert_eq!(
            palette.read_with(cx, |palette, _| palette.unified.as_ref().unwrap().mode),
            Some(mode)
        );
        if mode == PaletteMode::Host {
            cx.simulate_keystrokes("left");
            cx.run_until_parked();
            assert!(palette.read_with(cx, |palette, _| {
                palette.unified.as_ref().unwrap().collapsed.is_empty()
            }));
            cx.simulate_keystrokes("right");
            cx.run_until_parked();
            assert!(palette.read_with(cx, |palette, _| {
                palette.unified.as_ref().unwrap().collapsed.is_empty()
            }));
        }
        cx.simulate_input("x");
        cx.run_until_parked();
        cx.simulate_keystrokes("backspace");
        cx.run_until_parked();
        assert_eq!(
            palette.read_with(cx, |palette, _| palette.unified.as_ref().unwrap().mode),
            Some(mode)
        );
        cx.simulate_keystrokes("backspace");
        cx.run_until_parked();
        assert_eq!(
            palette.read_with(cx, |palette, _| palette.unified.as_ref().unwrap().mode),
            None
        );
        assert!(cx.update(|window, cx| palette.read(cx).focus(cx).is_focused(window)));
    }
    cx.update(|window, cx| {
        palette.update(cx, |palette, cx| {
            palette.unified.as_mut().unwrap().host = Some(LOCAL);
            palette.refresh(window, cx);
        });
    });
    cx.simulate_input(":");
    cx.run_until_parked();
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert!(palette.read_with(cx, |palette, _| {
        palette.unified.as_ref().unwrap().host.is_some()
    }));
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert!(palette.read_with(cx, |palette, _| {
        palette.unified.as_ref().unwrap().host.is_none()
    }));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(palette.read_with(cx, |palette, _| palette.is_finished()));
}

#[gpui::test]
fn local_palette_activates_targets_through_the_backend(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (backend, calls) = backend(online_tree());
    let (palette, cx) = mount(cx, move |window, cx| {
        CommandPaletteView::new_unified(backend, None, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(palette.read_with(cx, |palette, _| palette.is_finished()));
    assert_eq!(
        *calls.borrow(),
        [Call::Activate(LOCAL, PaletteTarget::Window(WindowId(1)))]
    );
}

#[gpui::test]
fn local_palette_executes_commands_on_the_attached_host(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (backend, calls) = backend(online_tree());
    let (palette, cx) = mount(cx, move |window, cx| {
        CommandPaletteView::new_unified(backend, Some(PaletteMode::Command), window, cx)
    });
    cx.simulate_input("new-window");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(palette.read_with(cx, |palette, _| palette.is_finished()));
    assert_eq!(
        *calls.borrow(),
        [Call::Execute(
            LOCAL,
            CommandInvocation::new("new-window", [] as [&str; 0])
        )]
    );
}

#[gpui::test]
fn palette_keeps_local_edits_for_the_same_revision_and_disables_value_completions(
    cx: &mut TestAppContext,
) {
    cx.update(crate::init);
    let mut initial = prompt_state("", CommandPromptMode::Text);
    initial.history = vec!["list-panes".to_owned()];
    let stale = initial.clone();
    let (backend, _) = backend(offline_tree());
    let (palette, cx) = mount(cx, move |window, cx| {
        CommandPaletteView::new(
            backend,
            &initial,
            1,
            Arc::new(MuxSnapshot::default()),
            window,
            cx,
        )
    });

    cx.update(|window, cx| {
        assert!(palette.read(cx).focus(cx).is_focused(window));
        let input = palette.read(cx).input.clone();
        input.update(cx, |input, cx| input.insert("ren", window, cx));
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| palette.read(cx).last_input.clone()),
        "ren"
    );
    assert!(cx.update(|_, cx| !palette.read(cx).suggestions.is_empty()));

    cx.update(|window, cx| {
        palette.update(cx, |palette, cx| {
            palette.synchronize(&stale, 1, &Arc::new(MuxSnapshot::default()), window, cx);
        });
    });
    assert_eq!(
        cx.update(|_, cx| palette.read(cx).input.read(cx).value().to_string()),
        "ren"
    );

    let value = CommandPromptState {
        prompt: "rename-window: ".to_owned(),
        kind: CommandPromptKind::Value,
        ..prompt_state("notes", CommandPromptMode::Text)
    };
    cx.update(|window, cx| {
        palette.update(cx, |palette, cx| {
            palette.synchronize(&value, 2, &Arc::new(MuxSnapshot::default()), window, cx);
        });
    });
    assert!(cx.update(|_, cx| palette.read(cx).suggestions.is_empty()));
    assert_eq!(
        cx.update(|_, cx| palette.read(cx).input.read(cx).value().to_string()),
        "notes"
    );
}

#[test]
fn key_reading_prompts_relay_instead_of_editing() {
    for mode in [
        CommandPromptMode::Single,
        CommandPromptMode::Numeric,
        CommandPromptMode::Key,
    ] {
        assert!(CommandPaletteView::relays_keys(mode), "{mode:?}");
    }
    for mode in [
        CommandPromptMode::Text,
        CommandPromptMode::Incremental,
        CommandPromptMode::BackspaceExit,
    ] {
        assert!(!CommandPaletteView::relays_keys(mode), "{mode:?}");
    }
}

#[test]
fn only_a_known_command_followed_by_whitespace_has_arguments() {
    assert!(has_command_arguments("split-window -h"));
    assert!(!has_command_arguments("split-window"));
}

#[gpui::test]
fn key_reading_prompt_relays_keystrokes_to_the_active_pane(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let state = CommandPromptState {
        prompt: "(key) ".to_owned(),
        ..prompt_state("", CommandPromptMode::Key)
    };
    let (backend, calls) = backend(offline_tree());
    let (palette, cx) = mount(cx, move |window, cx| {
        CommandPaletteView::new(
            backend,
            &state,
            1,
            Arc::new(MuxSnapshot::default()),
            window,
            cx,
        )
    });
    cx.simulate_keystrokes("a");
    cx.run_until_parked();
    assert_eq!(*calls.borrow(), [Call::Key(PaneId(0), "a".to_owned())]);
    assert!(cx.update(|_, cx| palette.read(cx).input.read(cx).value().is_empty()));
}

#[gpui::test]
fn history_suggestions_skip_commands_the_backend_cannot_run(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let mut state = prompt_state("", CommandPromptMode::Text);
    state.history = vec![
        "set-editor-path /tmp/file".to_owned(),
        "set-browser-tabs []".to_owned(),
        "agent-send hello".to_owned(),
        "list-panes".to_owned(),
    ];
    let (backend, _) = fake_backend(
        offline_tree(),
        PaneKindAvailability {
            browser: false,
            agent: false,
            editor: false,
        },
    );
    let (palette, cx) = mount(cx, move |window, cx| {
        CommandPaletteView::new(
            backend,
            &state,
            1,
            Arc::new(MuxSnapshot::default()),
            window,
            cx,
        )
    });
    let history = palette.read_with(cx, |palette, _| {
        palette
            .suggestions
            .iter()
            .filter(|suggestion| suggestion.kind == CompletionKind::History)
            .map(|suggestion| suggestion.insertion.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(history, ["list-panes"]);
}

#[gpui::test]
fn a_key_reading_prompt_drops_the_completion_list(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let initial = prompt_state("ren", CommandPromptMode::Text);
    let (backend, _) = backend(offline_tree());
    let (palette, cx) = mount(cx, move |window, cx| {
        CommandPaletteView::new(
            backend,
            &initial,
            1,
            Arc::new(MuxSnapshot::default()),
            window,
            cx,
        )
    });
    assert!(cx.update(|_, cx| !palette.read(cx).suggestions.is_empty()));

    for (revision, mode) in [
        (2, CommandPromptMode::Key),
        (3, CommandPromptMode::Numeric),
        (4, CommandPromptMode::Incremental),
    ] {
        let state = prompt_state("ren", mode);
        cx.update(|window, cx| {
            palette.update(cx, |palette, cx| {
                palette.synchronize(
                    &state,
                    revision,
                    &Arc::new(MuxSnapshot::default()),
                    window,
                    cx,
                );
            });
        });
        assert_eq!(cx.update(|_, cx| palette.read(cx).mode), mode);
        assert!(
            cx.update(|_, cx| palette.read(cx).suggestions.is_empty()),
            "{mode:?}"
        );
    }
}

#[gpui::test]
fn tab_accepts_completion_without_leaving_the_palette(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let initial = prompt_state("new-w", CommandPromptMode::Text);
    let (backend, _) = backend(offline_tree());
    let (palette, cx) = mount(cx, move |window, cx| {
        CommandPaletteView::new(
            backend,
            &initial,
            1,
            Arc::new(MuxSnapshot::default()),
            window,
            cx,
        )
    });

    cx.simulate_keystrokes("tab");

    assert_eq!(
        cx.update(|_, cx| palette.read(cx).input.read(cx).value().to_string()),
        "new-window "
    );
    assert!(!palette.read_with(cx, |palette, _| palette.finishing));
    assert!(cx.update(|window, cx| palette.read(cx).focus(cx).is_focused(window)));
}

#[test]
fn unicode_scalar_cursor_conversion_is_boundary_safe() {
    assert_eq!(byte_index_for_char("aα界", 0), Some(0));
    assert_eq!(byte_index_for_char("aα界", 2), Some(3));
    assert_eq!(byte_index_for_char("aα界", 3), Some(6));
    assert_eq!(byte_index_for_char("aα界", 4), None);
    let value = "a🦀日本";
    assert_eq!(byte_index_for_char(value, 2), Some(5));
    assert_eq!(byte_index_for_char(value, 4), Some(value.len()));
    assert_eq!(byte_index_for_char(value, 5), None);
}
