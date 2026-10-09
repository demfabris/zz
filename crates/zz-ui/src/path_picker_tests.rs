use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use zpui::{TestAppContext, VisualTestContext};
use zz_protocol::{InsertStyle, ShellKind};

use super::*;
use crate::Root;

#[derive(Debug, PartialEq)]
enum Call {
    List(Option<String>, u64),
    Cancel(u64),
    Insert(String, String, bool),
}

type Calls = Rc<RefCell<Vec<Call>>>;

struct FakeBackend {
    calls: Calls,
    next: Cell<u64>,
}

impl PathPickerBackend for FakeBackend {
    fn list(&self, dir: Option<&str>, _: &mut App) -> Option<u64> {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.calls
            .borrow_mut()
            .push(Call::List(dir.map(str::to_owned), id));
        Some(id)
    }

    fn cancel(&self, request_id: u64, _: &mut App) {
        self.calls.borrow_mut().push(Call::Cancel(request_id));
    }

    fn insert(&self, root: &PathListRoot, entry: &PathEntry, absolute: bool, _: &mut App) {
        self.calls
            .borrow_mut()
            .push(Call::Insert(root.root.clone(), entry.rel.clone(), absolute));
    }
}

struct Mounted<'a> {
    picker: Entity<PathPickerView>,
    calls: Calls,
    dismissed: Rc<Cell<usize>>,
    cx: &'a mut VisualTestContext,
}

fn mount<'a>(cx: &'a mut TestAppContext, start_dir: Option<&'static str>) -> Mounted<'a> {
    cx.update(crate::init);
    let calls = Calls::default();
    let backend: Rc<dyn PathPickerBackend> = Rc::new(FakeBackend {
        calls: Rc::clone(&calls),
        next: Cell::new(0),
    });
    let slot = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&slot);
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let picker = cx.new(|cx| PathPickerView::new(backend, start_dir, window, cx));
        picker.read(cx).focus_handle(cx).focus(window, cx);
        captured.replace(Some(picker.clone()));
        Root::new(picker, window, cx)
    });
    let picker: Entity<PathPickerView> = slot.borrow_mut().take().expect("picker captured");
    let dismissed = Rc::new(Cell::new(0));
    let counter = Rc::clone(&dismissed);
    cx.update(|_, cx| {
        cx.subscribe(&picker, move |_, event: &PathPickerEvent, _| {
            let PathPickerEvent::Dismissed = event;
            counter.set(counter.get() + 1);
        })
        .detach();
    });
    Mounted {
        picker,
        calls,
        dismissed,
        cx,
    }
}

fn root(path: &str) -> PathListRoot {
    PathListRoot {
        root: path.to_owned(),
        display_root: "~/work/".to_owned(),
        cwd: Some(path.to_owned()),
        insert: InsertStyle::Shell(ShellKind::Posix),
    }
}

fn entry(rel: &str, kind: PathKind) -> PathEntry {
    PathEntry {
        rel: rel.to_owned(),
        kind,
        symlink: false,
    }
}

fn tree() -> Vec<PathEntry> {
    vec![
        entry("b.txt", PathKind::File),
        entry("src", PathKind::Dir),
        entry("a.txt", PathKind::File),
        entry("docs", PathKind::Dir),
        entry("node_modules", PathKind::Dir),
        entry("src/main.rs", PathKind::File),
        entry("src/lib", PathKind::Dir),
        entry("src/lib/mod.rs", PathKind::File),
        entry("docs/guide.md", PathKind::File),
    ]
}

impl Mounted<'_> {
    fn feed(&mut self, request: u64, entries: Vec<PathEntry>, truncated: bool) {
        self.picker.update(self.cx, |picker, cx| {
            picker.apply_begin(request, Ok(root("/home/me/work")), cx);
            picker.apply_chunk(request, entries, true, truncated, cx);
        });
        self.cx.run_until_parked();
    }

    fn labels(&mut self) -> Vec<String> {
        self.picker.read_with(self.cx, |picker, _| {
            picker
                .rows
                .iter()
                .map(|row| row.label.to_string())
                .collect()
        })
    }

    fn selected(&mut self) -> String {
        self.picker.read_with(self.cx, |picker, _| {
            picker.rows[picker.selected].entry.rel.clone()
        })
    }

    fn select(&mut self, rel: &str) {
        self.picker.update(self.cx, |picker, _| {
            picker.selected = picker
                .rows
                .iter()
                .position(|row| row.entry.rel == rel)
                .expect("row present");
        });
    }

    fn lists(&self) -> Vec<(Option<String>, u64)> {
        self.calls
            .borrow()
            .iter()
            .filter_map(|call| match call {
                Call::List(dir, id) => Some((dir.clone(), *id)),
                _ => None,
            })
            .collect()
    }
}

#[zpui::test]
fn opens_with_the_start_dir_and_browses_depth_one_dirs_first(cx: &mut TestAppContext) {
    let mut picker = mount(cx, Some("/tmp/start"));
    assert_eq!(picker.lists(), [(Some("/tmp/start".to_owned()), 1)]);
    assert_eq!(
        picker
            .picker
            .read_with(picker.cx, |picker, _| picker.request_id()),
        Some(1)
    );
    picker.feed(1, tree(), false);
    assert_eq!(
        picker.labels(),
        ["docs", "node_modules", "src", "a.txt", "b.txt"]
    );
    assert_eq!(
        picker
            .picker
            .read_with(picker.cx, |picker, _| picker.prefix()),
        SharedString::from("~/work/")
    );
}

#[zpui::test]
fn stale_request_ids_are_ignored(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.picker.update(picker.cx, |picker, cx| {
        picker.apply_begin(7, Ok(root("/elsewhere")), cx);
        picker.apply_chunk(7, vec![entry("stale", PathKind::File)], true, false, cx);
        picker.apply_git(7, vec![("stale".to_owned(), GitMark::Modified)], cx);
    });
    picker.cx.run_until_parked();
    assert!(picker.labels().is_empty());
    assert!(
        picker
            .picker
            .read_with(picker.cx, |picker, _| picker.root.is_none())
    );
    picker.feed(1, tree(), true);
    picker.select("src");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().last(), Some(&(Some("src".to_owned()), 2)));
    picker.picker.update(picker.cx, |picker, cx| {
        picker.apply_chunk(1, vec![entry("late", PathKind::File)], true, false, cx);
    });
    picker.cx.run_until_parked();
    assert!(!picker.labels().contains(&"late".to_owned()));
}

#[zpui::test]
fn tab_reroots_locally_inside_a_complete_walk(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.select("src");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().len(), 1);
    assert_eq!(picker.labels(), ["lib", "main.rs"]);
    assert_eq!(
        picker
            .picker
            .read_with(picker.cx, |picker, _| picker.prefix()),
        SharedString::from("~/work/src/")
    );
    picker.cx.simulate_input("mod");
    picker.cx.run_until_parked();
    assert_eq!(picker.labels(), ["lib/mod.rs"]);
    picker.cx.simulate_keystrokes("enter");
    picker.cx.run_until_parked();
    assert!(picker.calls.borrow().contains(&Call::Insert(
        "/home/me/work".to_owned(),
        "src/lib/mod.rs".to_owned(),
        false
    )));
}

#[zpui::test]
fn tab_asks_the_daemon_for_unentered_or_truncated_dirs(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.select("node_modules");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.run_until_parked();
    assert_eq!(
        picker.lists().last(),
        Some(&(Some("node_modules".to_owned()), 2))
    );
    picker.feed(2, tree(), true);
    picker.select("docs");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().last(), Some(&(Some("docs".to_owned()), 3)));
    let before = picker.lists().len();
    picker.feed(3, tree(), false);
    picker.select("a.txt");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().len(), before);
    assert_eq!(picker.dismissed.get(), 0);
}

#[zpui::test]
fn backspace_on_an_empty_query_goes_to_the_parent(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.select("src");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.run_until_parked();
    picker.select("src/lib");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.run_until_parked();
    assert_eq!(picker.labels(), ["mod.rs"]);
    picker.cx.simulate_input("m");
    picker.cx.simulate_keystrokes("backspace");
    picker.cx.run_until_parked();
    assert_eq!(picker.labels(), ["mod.rs"]);
    picker.cx.simulate_keystrokes("backspace");
    picker.cx.run_until_parked();
    assert_eq!(picker.labels(), ["lib", "main.rs"]);
    picker.cx.simulate_keystrokes("backspace");
    picker.cx.run_until_parked();
    assert_eq!(picker.labels()[0], "docs");
    assert_eq!(picker.lists().len(), 1);
    picker.cx.simulate_keystrokes("backspace");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().last(), Some(&(Some("..".to_owned()), 2)));
}

#[zpui::test]
fn enter_inserts_relative_and_alt_enter_absolute(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.cx.simulate_keystrokes("down ctrl-n ctrl-p down");
    picker.cx.run_until_parked();
    assert_eq!(picker.selected(), "src");
    picker.cx.simulate_keystrokes("alt-enter");
    picker.cx.run_until_parked();
    assert_eq!(
        picker.calls.borrow().last(),
        Some(&Call::Insert(
            "/home/me/work".to_owned(),
            "src".to_owned(),
            true
        ))
    );
    assert_eq!(picker.dismissed.get(), 1);
    assert!(picker.picker.read_with(picker.cx, |picker, cx| {
        picker.input.read(cx).value().is_empty()
    }));
    picker.cx.simulate_keystrokes("enter");
    picker.cx.run_until_parked();
    assert_eq!(picker.dismissed.get(), 1);
    assert_eq!(
        picker
            .calls
            .borrow()
            .iter()
            .filter(|call| matches!(call, Call::Insert(..)))
            .count(),
        1
    );
}

#[zpui::test]
fn enter_inserts_the_top_ranked_match(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.cx.simulate_input("guide");
    picker.cx.run_until_parked();
    assert_eq!(picker.labels(), ["docs/guide.md"]);
    let matches = picker
        .picker
        .read_with(picker.cx, |picker, _| picker.rows[0].matches.clone());
    assert_eq!(matches, vec![5..10]);
    picker.cx.simulate_keystrokes("enter");
    picker.cx.run_until_parked();
    assert_eq!(
        picker.calls.borrow().last(),
        Some(&Call::Insert(
            "/home/me/work".to_owned(),
            "docs/guide.md".to_owned(),
            false
        ))
    );
    assert_eq!(picker.dismissed.get(), 1);
}

#[zpui::test]
fn home_absolute_and_parent_queries_reroot_through_the_backend(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.cx.simulate_input("~/dev");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists()[1], (Some("~/".to_owned()), 2));
    assert_eq!(
        picker
            .picker
            .read_with(picker.cx, |picker, cx| picker.input.read(cx).value()),
        SharedString::from("dev")
    );
    picker
        .cx
        .simulate_keystrokes("backspace backspace backspace");
    picker.feed(2, tree(), false);
    picker.cx.simulate_input("../");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().last(), Some(&(Some("../".to_owned()), 3)));
    picker.feed(3, tree(), false);
    picker.select("src");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.simulate_input("../");
    picker.cx.run_until_parked();
    assert_eq!(
        picker.lists().last(),
        Some(&(Some("src/../".to_owned()), 4))
    );
    picker.feed(4, tree(), false);
    picker.cx.simulate_input("/");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().last(), Some(&(Some("/".to_owned()), 5)));
    assert!(
        !picker
            .calls
            .borrow()
            .iter()
            .any(|call| matches!(call, Call::Cancel(_)))
    );
}

#[zpui::test]
fn escape_dismisses_and_cancels_an_unfinished_walk(cx: &mut TestAppContext) {
    let picker = mount(cx, None);
    picker.picker.update(picker.cx, |picker, cx| {
        picker.apply_begin(1, Ok(root("/home/me/work")), cx);
        picker.apply_chunk(1, tree(), false, false, cx);
    });
    picker.cx.run_until_parked();
    picker.cx.simulate_keystrokes("escape");
    picker.cx.run_until_parked();
    assert_eq!(picker.dismissed.get(), 1);
    assert_eq!(picker.calls.borrow().last(), Some(&Call::Cancel(1)));
}

#[zpui::test]
fn git_marks_apply_by_prefix_and_rank_changed_files_first(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(
        1,
        vec![
            entry("a/x.rs", PathKind::File),
            entry("b/x.rs", PathKind::File),
            entry("new", PathKind::Dir),
            entry("new/x.rs", PathKind::File),
        ],
        false,
    );
    picker.picker.update(picker.cx, |picker, cx| {
        picker.apply_git(
            1,
            vec![
                ("b/x.rs".to_owned(), GitMark::Modified),
                ("new/".to_owned(), GitMark::Untracked),
            ],
            cx,
        );
    });
    picker.cx.simulate_input("x.rs");
    picker.cx.run_until_parked();
    let rows = picker.picker.read_with(picker.cx, |picker, _| {
        picker
            .rows
            .iter()
            .map(|row| (row.entry.rel.clone(), row.mark))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        rows,
        [
            ("b/x.rs".to_owned(), Some(GitMark::Modified)),
            ("new/x.rs".to_owned(), Some(GitMark::Untracked)),
            ("a/x.rs".to_owned(), None),
        ]
    );
    picker
        .cx
        .simulate_keystrokes("backspace backspace backspace backspace");
    picker.cx.run_until_parked();
    let dir = picker.picker.read_with(picker.cx, |picker, _| {
        picker
            .rows
            .iter()
            .find(|row| row.entry.rel == "new")
            .and_then(|row| row.mark)
    });
    assert_eq!(dir, Some(GitMark::Untracked));
}

#[zpui::test]
fn enter_and_tab_act_on_the_current_query_before_its_rank_lands(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    assert_eq!(picker.selected(), "docs");
    picker.picker.update(picker.cx, |picker, cx| {
        "guide".clone_into(&mut picker.query);
        picker.refresh(cx);
    });
    picker.cx.simulate_keystrokes("enter");
    picker.cx.run_until_parked();
    assert_eq!(
        picker.calls.borrow().last(),
        Some(&Call::Insert(
            "/home/me/work".to_owned(),
            "docs/guide.md".to_owned(),
            false
        ))
    );

    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.picker.update(picker.cx, |picker, cx| {
        "lib".clone_into(&mut picker.query);
        picker.refresh(cx);
    });
    picker.cx.simulate_keystrokes("tab");
    picker.cx.run_until_parked();
    assert_eq!(
        picker
            .picker
            .read_with(picker.cx, |picker, _| picker.prefix()),
        SharedString::from("~/work/src/lib/")
    );
}

#[zpui::test]
fn a_truncated_listing_says_so_when_nothing_shows(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, Vec::new(), true);
    let message = |picker: &mut Mounted<'_>| {
        picker
            .picker
            .read_with(picker.cx, |picker, _| picker.empty_message())
    };
    assert_eq!(
        message(&mut picker),
        SharedString::from("The listing stopped early, try again")
    );
    picker.feed(1, tree(), true);
    picker.cx.simulate_input("zzz");
    picker.cx.run_until_parked();
    assert_eq!(
        message(&mut picker),
        SharedString::from("No matches in the partial listing")
    );
}

#[zpui::test]
fn an_error_begin_shows_the_message(cx: &mut TestAppContext) {
    let picker = mount(cx, None);
    picker.picker.update(picker.cx, |picker, cx| {
        picker.apply_begin(1, Err("Not a directory".to_owned()), cx);
    });
    picker.cx.run_until_parked();
    assert_eq!(
        picker
            .picker
            .read_with(picker.cx, |picker, _| picker.empty_message()),
        SharedString::from("Not a directory")
    );
    picker.cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(picker.cx.debug_bounds("path-picker-empty").is_some());
}

#[zpui::test]
fn a_typed_path_reroots_at_every_slash(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.cx.simulate_input("src/");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().len(), 1);
    picker
        .cx
        .simulate_keystrokes("backspace backspace backspace backspace");
    picker.cx.simulate_input("/");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().last(), Some(&(Some("/".to_owned()), 2)));
    picker.feed(
        2,
        vec![entry("etc", PathKind::Dir), entry("usr", PathKind::Dir)],
        false,
    );
    picker.cx.simulate_input("etc/");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().last(), Some(&(Some("etc".to_owned()), 3)));
    assert!(picker.picker.read_with(picker.cx, |picker, cx| {
        picker.input.read(cx).value().is_empty()
    }));
    picker.feed(
        3,
        vec![
            entry("hosts", PathKind::File),
            entry("ssh", PathKind::Dir),
            entry("ssh/config", PathKind::File),
        ],
        false,
    );
    picker.cx.simulate_input("ssh/");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().len(), 3);
    assert_eq!(picker.labels(), ["config"]);
    assert_eq!(
        picker
            .picker
            .read_with(picker.cx, |picker, _| picker.prefix()),
        SharedString::from("~/work/ssh/")
    );
}

#[zpui::test]
fn a_pending_request_resets_the_local_base(cx: &mut TestAppContext) {
    let mut picker = mount(cx, None);
    picker.feed(1, tree(), false);
    picker.select("src");
    picker.cx.simulate_keystrokes("tab");
    picker.cx.simulate_input("../");
    picker.cx.run_until_parked();
    assert_eq!(
        picker.lists().last(),
        Some(&(Some("src/../".to_owned()), 2))
    );
    picker.cx.simulate_input("../");
    picker.cx.run_until_parked();
    assert_eq!(picker.lists().last(), Some(&(Some("../".to_owned()), 3)));
}
