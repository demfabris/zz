use std::{cell::Cell, rc::Rc};

use zpui::{
    AnyView, App, AppContext as _, Context, Entity, Focusable as _, IntoElement, Keystroke,
    ParentElement as _, Render, Styled as _, Window, div, prelude::*, px,
};
use zz_protocol::{GitMark, InsertStyle, PathEntry, PathKind, PathListRoot, ShellKind};
use zz_ui::path_picker::{PathPickerBackend, PathPickerView};

use crate::story::{Section, Story, states};

pub const STORY: Story = Story {
    id: "path-picker",
    name: "Path picker",
    group: "Commands",
    summary: "Inserts a path into the focused pane. It browses one folder at a time, ranks a query across the whole listing, and marks files git has seen change.",
    sections: &[
        Section {
            id: "browse",
            name: "Browse",
            summary: "A folder at rest: folders first, then files, with git letters on the right.",
            build: |window, cx| single(window, cx, Fixture::Browse, &[]),
        },
        Section {
            id: "query",
            name: "Query",
            summary: "A typed query ranks paths from every depth, highlights the matched letters and mutes the folders in front. The only focused picker on this page.",
            build: |window, cx| single(window, cx, Fixture::Browse, &["p", "a", "l"]),
        },
        Section {
            id: "nested",
            name: "Nested folder",
            summary: "A listing that started in a deeper folder shows the full prefix in the prompt.",
            build: |window, cx| single(window, cx, Fixture::Nested, &[]),
        },
        Section {
            id: "states",
            name: "Listing states",
            summary: "What the picker says while it waits, when the host refuses, when a folder is empty, and when the listing stopped early.",
            build: |window, cx| cx.new(|cx| ListingStates::new(window, cx)).into(),
        },
    ],
};

#[derive(Default)]
struct StoryPaths {
    next: Cell<u64>,
}

impl PathPickerBackend for StoryPaths {
    fn list(&self, _: Option<&str>, _: &mut App) -> Option<u64> {
        let id = self.next.get() + 1;
        self.next.set(id);
        Some(id)
    }

    fn cancel(&self, _: u64, _: &mut App) {}

    fn insert(&self, _: &PathListRoot, _: &PathEntry, _: bool, _: &mut App) {}
}

#[derive(Clone, Copy)]
enum Fixture {
    Browse,
    Nested,
    Listing,
    Refused,
    Empty,
    Truncated,
}

fn root(display: &str) -> PathListRoot {
    PathListRoot {
        root: "/Users/fabrico/dev/zz".to_owned(),
        display_root: display.to_owned(),
        cwd: Some("/Users/fabrico/dev/zz".to_owned()),
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

const DIRS: &[&str] = &[
    ".github",
    "clients",
    "crates",
    "knowledge",
    "scripts",
    "site",
    "third_party",
    "crates/zz-ui",
    "crates/zz-ui/src",
    "crates/zz-ui/src/command",
    "clients/storybook",
    "clients/storybook/src",
    "knowledge/designs",
];

const FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "Justfile",
    "README.md",
    "NOTES.md",
    "mise.toml",
    "rust-toolchain.toml",
    "crates/zz-ui/src/path_picker.rs",
    "crates/zz-ui/src/command/palette.rs",
    "crates/zz-ui/src/command/palette_view.rs",
    "crates/zz-ui/src/command/palette_model.rs",
    "clients/storybook/src/story.rs",
    "knowledge/designs/command-palette.md",
    "scripts/build-storybook.sh",
];

const NESTED_DIRS: &[&str] = &["compact", "command", "pane", "settings"];

const NESTED_FILES: &[&str] = &[
    "chooser.rs",
    "feedback.rs",
    "lib.rs",
    "path_picker.rs",
    "terminal.rs",
    "tmux_style.rs",
    "which_key.rs",
];

fn listing(dirs: &[&str], files: &[&str]) -> Vec<PathEntry> {
    dirs.iter()
        .map(|rel| entry(rel, PathKind::Dir))
        .chain(files.iter().map(|rel| entry(rel, PathKind::File)))
        .collect()
}

fn marks() -> Vec<(String, GitMark)> {
    vec![
        ("Cargo.toml".to_owned(), GitMark::Modified),
        ("rust-toolchain.toml".to_owned(), GitMark::Added),
        ("NOTES.md".to_owned(), GitMark::Untracked),
        ("Justfile".to_owned(), GitMark::Conflicted),
        ("crates/".to_owned(), GitMark::Modified),
        ("clients/storybook/".to_owned(), GitMark::Added),
    ]
}

fn picker(window: &mut Window, cx: &mut App, fixture: Fixture) -> Entity<PathPickerView> {
    let backend: Rc<dyn PathPickerBackend> = Rc::new(StoryPaths::default());
    let picker = cx.new(|cx| PathPickerView::new(backend, None, window, cx));
    let Some(request) = picker.read(cx).request_id() else {
        return picker;
    };
    picker.update(cx, |picker, cx| match fixture {
        Fixture::Browse => {
            picker.apply_begin(request, Ok(root("~/dev/zz/")), cx);
            picker.apply_chunk(request, listing(DIRS, FILES), true, false, cx);
            picker.apply_git(request, marks(), cx);
        }
        Fixture::Nested => {
            picker.apply_begin(request, Ok(root("~/dev/zz/crates/zz-ui/src/")), cx);
            picker.apply_chunk(request, listing(NESTED_DIRS, NESTED_FILES), true, false, cx);
            picker.apply_git(
                request,
                vec![
                    ("path_picker.rs".to_owned(), GitMark::Modified),
                    ("command/".to_owned(), GitMark::Modified),
                ],
                cx,
            );
        }
        Fixture::Listing => picker.apply_begin(request, Ok(root("~/dev/zz/")), cx),
        Fixture::Refused => {
            picker.apply_begin(
                request,
                Err("Permission denied: ~/Library/Mail".to_owned()),
                cx,
            );
        }
        Fixture::Empty => {
            picker.apply_begin(request, Ok(root("~/dev/zz/target/")), cx);
            picker.apply_chunk(request, Vec::new(), true, false, cx);
        }
        Fixture::Truncated => {
            picker.apply_begin(request, Ok(root("/nfs/archive/")), cx);
            picker.apply_chunk(request, Vec::new(), true, true, cx);
        }
    });
    picker
}

struct Single {
    picker: Entity<PathPickerView>,
}

impl Render for Single {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w_full().child(self.picker.clone())
    }
}

fn single(
    window: &mut Window,
    cx: &mut App,
    fixture: Fixture,
    keys: &'static [&'static str],
) -> AnyView {
    let picker = picker(window, cx, fixture);
    if !keys.is_empty() {
        let focus = picker.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        window.on_next_frame(move |window, cx| {
            for key in keys {
                if let Ok(keystroke) = Keystroke::parse(key) {
                    window.dispatch_keystroke(keystroke, cx);
                }
            }
        });
    }
    cx.new(|_| Single { picker }).into()
}

struct ListingStates {
    pickers: Vec<(&'static str, Entity<PathPickerView>)>,
}

impl ListingStates {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pickers = [
            ("listing", Fixture::Listing),
            ("refused", Fixture::Refused),
            ("empty folder", Fixture::Empty),
            ("stopped early", Fixture::Truncated),
        ]
        .into_iter()
        .map(|(caption, fixture)| (caption, picker(window, cx, fixture)))
        .collect();
        Self { pickers }
    }
}

impl Render for ListingStates {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.pickers
            .iter()
            .enumerate()
            .fold(states(), |states, (index, (caption, picker))| {
                states.state(
                    *caption,
                    div()
                        .id(("path-picker-state", index))
                        .w(px(560.0))
                        .child(picker.clone()),
                )
            })
    }
}
