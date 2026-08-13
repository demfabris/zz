use std::{
    fs::{self, File},
    io::Read as _,
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding, MouseButton,
    MouseDownEvent, ParentElement as _, Render, Styled as _, Subscription, Window, div, prelude::*,
    px,
};
use zz_protocol::{CommandInvocation, EditorDescriptor, PaneId};
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    code_editor::{CodeEditor, CodeEditorEvent, CodeEditorState, VimMode},
    h_flex,
    tag::Tag,
    v_flex,
};

use crate::{
    config::{self, pane_content_radii},
    file_picker::{FilePickerEvent, FilePickerMode, FilePickerView},
    mux::client::MuxClient,
    window::corners::{WindowCorners, round_div_radii},
};

const EDITOR_KEY_CONTEXT: &str = "Editor";
const MAX_EDITOR_FILE_BYTES: u64 = 8 * 1024 * 1024;

gpui::actions!(editor_pane, [OpenFile, SaveFile]);

pub fn init(cx: &mut App) {
    cx.bind_keys(editor_key_bindings());
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn editor_key_bindings() -> [KeyBinding; 2] {
    [
        KeyBinding::new("cmd-o", OpenFile, Some(EDITOR_KEY_CONTEXT)),
        KeyBinding::new("cmd-s", SaveFile, Some(EDITOR_KEY_CONTEXT)),
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn editor_key_bindings() -> [KeyBinding; 2] {
    [
        KeyBinding::new("ctrl-o", OpenFile, Some(EDITOR_KEY_CONTEXT)),
        KeyBinding::new("ctrl-s", SaveFile, Some(EDITOR_KEY_CONTEXT)),
    ]
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
const OPEN_HINT: &str = "⌘O to open a file";
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
const OPEN_HINT: &str = "Ctrl+O to open a file";

#[derive(Clone)]
enum Retry {
    Open(PathBuf),
    Save(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
enum SyncAction {
    Ignore,
    ClearPending,
    Restore,
}

fn descriptor_sync_action(
    pending: Option<&str>,
    snapshot: Option<&str>,
    local: Option<&Path>,
) -> SyncAction {
    if let Some(pending) = pending {
        return if snapshot == Some(pending) {
            SyncAction::ClearPending
        } else {
            SyncAction::Ignore
        };
    }
    if snapshot.map(Path::new) == local {
        SyncAction::Ignore
    } else {
        SyncAction::Restore
    }
}

#[derive(Clone)]
struct EditorError {
    message: Arc<str>,
    retry: Retry,
}

pub(crate) struct EditorView {
    pane: PaneId,
    mux: Entity<MuxClient>,
    editor: Entity<CodeEditorState>,
    cwd: PathBuf,
    path: Option<PathBuf>,
    snapshot_path: Option<String>,
    pending_path: Option<String>,
    saved_contents: String,
    dirty: bool,
    last_title: String,
    error: Option<EditorError>,
    picker: Option<Entity<FilePickerView>>,
    window_corners: WindowCorners,
    _subscriptions: Vec<Subscription>,
}

impl EditorView {
    pub(crate) fn new(
        pane: PaneId,
        descriptor: &EditorDescriptor,
        mux: Entity<MuxClient>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            CodeEditorState::new(window, cx)
                .language("text")
                .soft_wrap(true)
        });
        let subscription = cx.subscribe_in(
            &editor,
            window,
            |view, editor, event: &CodeEditorEvent, _, cx| {
                if matches!(event, CodeEditorEvent::Change) {
                    let contents = editor.read(cx).value();
                    view.on_buffer_changed(contents.as_ref(), cx);
                }
            },
        );
        let mut view = Self {
            pane,
            mux,
            editor,
            cwd: PathBuf::from(&descriptor.cwd),
            path: None,
            snapshot_path: descriptor.path.clone(),
            pending_path: None,
            saved_contents: String::new(),
            dirty: false,
            last_title: String::new(),
            error: None,
            picker: None,
            window_corners: WindowCorners::NONE,
            _subscriptions: vec![subscription],
        };
        view.restore_descriptor(descriptor, window, cx);
        view
    }

    pub(crate) fn focus(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle(cx)
    }

    pub(crate) fn set_window_corners(&mut self, corners: WindowCorners, cx: &mut Context<Self>) {
        if self.window_corners != corners {
            self.window_corners = corners;
            cx.notify();
        }
    }

    pub(crate) fn synchronize_descriptor(
        &mut self,
        descriptor: &EditorDescriptor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cwd = PathBuf::from(&descriptor.cwd);
        self.snapshot_path.clone_from(&descriptor.path);
        match descriptor_sync_action(
            self.pending_path.as_deref(),
            descriptor.path.as_deref(),
            self.path.as_deref(),
        ) {
            SyncAction::Ignore => {}
            SyncAction::ClearPending => self.pending_path = None,
            SyncAction::Restore => self.restore_descriptor(descriptor, window, cx),
        }
    }

    fn restore_descriptor(
        &mut self,
        descriptor: &EditorDescriptor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cwd = PathBuf::from(&descriptor.cwd);
        let Some(path) = descriptor.path.as_deref().map(PathBuf::from) else {
            self.path = None;
            self.saved_contents.clear();
            self.dirty = false;
            self.error = None;
            self.editor.update(cx, |editor, cx| {
                editor.set_value("", window, cx);
                editor.set_language("text", window, cx);
            });
            self.publish_title(cx);
            return;
        };

        self.path = Some(path.clone());
        self.editor.update(cx, |editor, cx| {
            editor.set_language(language_for_path(&path), window, cx);
        });
        self.publish_title(cx);
        self.load_file(path, window, cx);
    }

    #[allow(
        clippy::unused_self,
        reason = "kept as a method so open, restore, and retry share one call shape"
    )]
    fn load_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let read_path = path.clone();
        let background = cx.background_executor().clone();
        let view = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            let result = background
                .spawn(async move { read_editor_file(&read_path) })
                .await;
            window
                .update(|window, cx| {
                    view.update(cx, |view, cx| {
                        view.finish_load(path, result, window, cx);
                    });
                })
                .ok()
        })
        .detach();
    }

    fn on_buffer_changed(&mut self, contents: &str, cx: &mut Context<Self>) {
        let dirty = contents != self.saved_contents.as_str();
        if self.dirty == dirty {
            return;
        }
        self.dirty = dirty;
        self.publish_title(cx);
        cx.notify();
    }

    fn open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            return;
        }
        let picker = cx.new(|cx| {
            FilePickerView::new(
                FilePickerMode::Files,
                self.cwd.clone(),
                "Open a file in the editor",
                window,
                cx,
            )
        });
        cx.subscribe_in(
            &picker,
            window,
            |view, _, event: &FilePickerEvent, window, cx| {
                view.picker = None;
                if let FilePickerEvent::Selected(path) = event {
                    view.load_file(path.clone(), window, cx);
                }
                view.focus(cx).focus(window, cx);
                cx.notify();
            },
        )
        .detach();
        self.error = None;
        self.picker = Some(picker);
        cx.notify();
    }

    fn finish_load(
        &mut self,
        path: PathBuf,
        result: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(contents) => self.apply_loaded_file(path, contents, window, cx),
            Err(message) => {
                self.error = Some(EditorError {
                    message: message.into(),
                    retry: Retry::Open(path),
                });
                cx.notify();
            }
        }
    }

    fn apply_loaded_file(
        &mut self,
        path: PathBuf,
        contents: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(path_string) = validated_descriptor_path(&path, &self.cwd) else {
            self.error = Some(EditorError {
                message: format!("{} cannot be stored as an editor path", path.display()).into(),
                retry: Retry::Open(path),
            });
            cx.notify();
            return;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_value(&contents, window, cx);
            editor.set_language(language_for_path(&path), window, cx);
        });
        self.path = Some(path);
        self.saved_contents = contents;
        self.dirty = false;
        self.error = None;
        self.publish_path(path_string, cx);
        self.publish_title(cx);
        cx.notify();
    }

    fn save_file(&mut self, _: &SaveFile, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.path.clone() {
            self.begin_save(path, window, cx);
            return;
        }

        let selected = cx.prompt_for_new_path(&self.cwd, Some("untitled"));
        let view = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            let path = selected.await.ok()?.ok()??;
            window
                .update(|window, cx| {
                    view.update(cx, |view, cx| view.begin_save(path, window, cx));
                })
                .ok()
        })
        .detach();
    }

    fn begin_save(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(message) = validated_descriptor_path(&path, &self.cwd) {
            self.error = Some(EditorError {
                message: message.into(),
                retry: Retry::Save(path),
            });
            cx.notify();
            return;
        }
        let contents = self.editor.read(cx).value().to_string();
        let write_path = path.clone();
        let write_contents = contents.clone();
        let background = cx.background_executor().clone();
        let view = cx.entity();
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |_, window| {
            let result = background
                .spawn(async move {
                    config::atomic_write(&write_path, write_contents.as_bytes()).map_err(|error| {
                        format!("Could not save {}: {error}", write_path.display())
                    })
                })
                .await;
            window
                .update(|_, cx| {
                    view.update(cx, |view, cx| {
                        view.finish_save(path, contents, result, cx);
                    });
                })
                .ok()
        })
        .detach();
    }

    fn finish_save(
        &mut self,
        path: PathBuf,
        saved_contents: String,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if let Err(message) = result {
            self.error = Some(EditorError {
                message: message.into(),
                retry: Retry::Save(path),
            });
            cx.notify();
            return;
        }
        let path_string = match validated_descriptor_path(&path, &self.cwd) {
            Ok(path) => path,
            Err(message) => {
                self.error = Some(EditorError {
                    message: message.into(),
                    retry: Retry::Save(path),
                });
                cx.notify();
                return;
            }
        };
        self.path = Some(path);
        self.saved_contents = saved_contents;
        self.dirty = self.editor.read(cx).value().as_ref() != self.saved_contents.as_str();
        self.error = None;
        self.publish_path(path_string, cx);
        self.publish_title(cx);
        cx.notify();
    }

    fn retry(&mut self, retry: Retry, window: &mut Window, cx: &mut Context<Self>) {
        match retry {
            Retry::Open(path) => self.load_file(path, window, cx),
            Retry::Save(path) => self.begin_save(path, window, cx),
        }
    }

    fn on_mouse_down(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus(cx).focus(window, cx);
        self.mux.read(cx).execute(CommandInvocation::new(
            "select-pane",
            ["-t", &self.pane.to_string()],
        ));
    }

    fn publish_path(&mut self, path: String, cx: &App) {
        if self.snapshot_path.as_deref() == Some(path.as_str()) {
            self.pending_path = None;
            return;
        }
        self.pending_path = Some(path.clone());
        self.mux.read(cx).execute(CommandInvocation::new(
            "set-editor-path",
            vec!["-t".to_owned(), self.pane.to_string(), path],
        ));
    }

    fn publish_title(&mut self, cx: &App) {
        let title = editor_title(self.path.as_deref(), self.dirty);
        if title == self.last_title {
            return;
        }
        self.last_title.clone_from(&title);
        self.mux.read(cx).execute(CommandInvocation::new(
            "select-pane",
            vec![
                "-t".to_owned(),
                self.pane.to_string(),
                "-T".to_owned(),
                title,
            ],
        ));
    }

    fn render_error(&self, error: EditorError, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let retry = error.retry;
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(cx.theme().background.opaque())
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(440.0))
                    .gap_3()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().danger.outline())
                    .bg(cx.theme().danger.fill())
                    .p_4()
                    .text_size(zz_ui::rems_from_px(12.0))
                    .child(error.message.to_string())
                    .child(
                        h_flex().child(
                            Button::new(format!("editor-retry-{}", self.pane.0))
                                .primary()
                                .small()
                                .icon(IconName::Redo2)
                                .label("Try again")
                                .on_click(move |_, window, cx| {
                                    view.update(cx, |view, cx| {
                                        view.retry(retry.clone(), window, cx);
                                    });
                                    cx.stop_propagation();
                                }),
                        ),
                    ),
            )
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.focus(cx)
    }
}

impl Render for EditorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus(cx);
        let scratch_hint = self.path.is_none()
            && self.editor.read(cx).value().is_empty()
            && self.error.is_none()
            && self.picker.is_none();
        let error = self.error.clone();
        let line_numbers = config::editor_line_numbers(cx);
        let relative_line_numbers = config::editor_relative_line_numbers(cx);
        let soft_wrap = config::editor_soft_wrap(cx);
        let vim = config::editor_vim_mode(cx);
        let radii = pane_content_radii(cx, self.window_corners);
        self.editor.update(cx, |editor, cx| {
            editor.set_line_numbers(line_numbers, cx);
            editor.set_relative_line_numbers(relative_line_numbers, cx);
            editor.set_soft_wrap(soft_wrap, cx);
            editor.set_vim_enabled(vim, cx);
            editor.set_corner_radii(radii, cx);
        });
        let vim_mode = self.editor.read(cx).vim_mode();
        let root = div()
            .id(("editor-pane", self.pane.0))
            .key_context(EDITOR_KEY_CONTEXT)
            .track_focus(&focus)
            .relative()
            .flex()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(config::editor_font_size(cx))
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::save_file))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .child(CodeEditor::new(&self.editor))
            .when(scratch_hint, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(cx.theme().foreground.muted())
                        .child(OPEN_HINT),
                )
            })
            .when_some(vim_mode, |this, mode| this.child(vim_mode_badge(mode, cx)))
            .when_some(error, |this, error| {
                this.child(self.render_error(error, cx))
            })
            .when_some(self.picker.clone(), gpui::ParentElement::child);
        round_div_radii(root, radii)
    }
}

fn vim_mode_badge(mode: VimMode, cx: &App) -> Tag {
    let badge = if matches!(mode, VimMode::Normal) {
        Tag::secondary().text_color(cx.theme().foreground.muted())
    } else {
        Tag::primary()
    };
    badge
        .absolute()
        .right(px(8.0))
        .bottom(px(8.0))
        .text_size(zz_ui::rems_from_px(11.0))
        .child(mode.label())
}

fn validated_descriptor_path(path: &Path, cwd: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or_else(|| "Editor paths must be valid UTF-8".to_owned())?
        .to_owned();
    let cwd = cwd
        .to_str()
        .ok_or_else(|| "The editor working directory must be valid UTF-8".to_owned())?
        .to_owned();
    EditorDescriptor {
        path: Some(path.clone()),
        cwd,
    }
    .validate()
    .map_err(|error| error.to_string())?;
    Ok(path)
}

fn read_editor_file(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    if metadata.len() > MAX_EDITOR_FILE_BYTES {
        return Err(format!(
            "{} is larger than the 8 MiB editor limit",
            path.display()
        ));
    }
    let mut file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or_default());
    file.by_ref()
        .take(MAX_EDITOR_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_EDITOR_FILE_BYTES {
        return Err(format!(
            "{} is larger than the 8 MiB editor limit",
            path.display()
        ));
    }
    if bytes.contains(&0) {
        return Err(format!(
            "{} contains NUL bytes and appears to be binary",
            path.display()
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))
}

fn language_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("rs") => "rust",
        Some("md" | "markdown" | "mdx") => "markdown",
        Some("json" | "jsonc") => "json",
        Some("toml") => "toml",
        _ => "text",
    }
}

fn editor_title(path: Option<&Path>, dirty: bool) -> String {
    let mut title = path
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("editor")
        .to_owned();
    if dirty {
        title.push_str(" •");
    }
    title
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn language_and_dirty_titles_follow_the_file_name() {
        assert_eq!(language_for_path(Path::new("/tmp/main.rs")), "rust");
        assert_eq!(language_for_path(Path::new("/tmp/README.md")), "markdown");
        assert_eq!(language_for_path(Path::new("/tmp/data.json")), "json");
        assert_eq!(language_for_path(Path::new("/tmp/config.toml")), "toml");
        assert_eq!(language_for_path(Path::new("/tmp/notes.txt")), "text");
        assert_eq!(editor_title(None, false), "editor");
        assert_eq!(
            editor_title(Some(Path::new("/tmp/main.rs")), true),
            "main.rs •"
        );
    }

    #[test]
    fn stale_snapshots_never_restore_while_a_publish_is_in_flight() {
        let pending = Some("/workspace/new.rs");
        let local = Some(Path::new("/workspace/new.rs"));

        assert_eq!(
            descriptor_sync_action(pending, None, local),
            SyncAction::Ignore
        );
        assert_eq!(
            descriptor_sync_action(pending, Some("/workspace/old.rs"), local),
            SyncAction::Ignore
        );
        assert_eq!(
            descriptor_sync_action(pending, Some("/workspace/new.rs"), local),
            SyncAction::ClearPending
        );
        assert_eq!(
            descriptor_sync_action(None, Some("/workspace/new.rs"), local),
            SyncAction::Ignore
        );
        assert_eq!(
            descriptor_sync_action(None, Some("/workspace/other.rs"), local),
            SyncAction::Restore
        );
        assert_eq!(
            descriptor_sync_action(None, None, local),
            SyncAction::Restore
        );
    }

    #[test]
    fn bounded_reader_rejects_binary_invalid_utf8_and_oversized_files() {
        let directory = tempfile::tempdir().expect("temporary directory");

        let binary = directory.path().join("binary");
        fs::write(&binary, b"a\0b").expect("binary fixture");
        assert!(read_editor_file(&binary).unwrap_err().contains("NUL"));

        let invalid = directory.path().join("invalid");
        fs::write(&invalid, [0xff]).expect("invalid UTF-8 fixture");
        assert!(
            read_editor_file(&invalid)
                .unwrap_err()
                .contains("valid UTF-8")
        );

        let oversized = directory.path().join("oversized");
        let mut file = File::create(&oversized).expect("oversized fixture");
        file.write_all(b"x").expect("fixture byte");
        file.set_len(MAX_EDITOR_FILE_BYTES + 1)
            .expect("extend fixture");
        assert!(read_editor_file(&oversized).unwrap_err().contains("8 MiB"));
    }
}
