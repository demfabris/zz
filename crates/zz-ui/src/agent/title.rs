use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, ElementId, Entity, FocusHandle, IntoElement, Render, Subscription,
    Window, div, prelude::*, px,
};

use crate::{
    Disableable as _, Sizable as _,
    input::{Enter, Escape, Input, InputEvent, InputState},
};

use super::controls::agent_thread_button;

const KEY_CONTEXT: &str = "AgentTitle";

pub fn agent_title_is_editing(window: &Window) -> bool {
    window
        .context_stack()
        .iter()
        .any(|context| context.contains(KEY_CONTEXT))
}

type RenameHandler = Rc<dyn Fn(&str, &mut App)>;

struct ThreadTitleEditor {
    title: String,
    enabled: bool,
    editing: bool,
    input: Entity<InputState>,
    previous_focus: Option<FocusHandle>,
    on_rename: RenameHandler,
    _input_events: Subscription,
}

impl ThreadTitleEditor {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Session name"));
        let input_events = cx.subscribe(&input, |this: &mut Self, _, event, cx| {
            if matches!(event, InputEvent::Blur) && this.editing {
                this.editing = false;
                this.previous_focus = None;
                cx.notify();
            }
        });
        Self {
            title: String::new(),
            enabled: false,
            editing: false,
            input,
            previous_focus: None,
            on_rename: Rc::new(|_, _| {}),
            _input_events: input_events,
        }
    }

    fn begin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        self.previous_focus = window.focused(cx);
        self.editing = true;
        self.input.update(cx, |input, cx| {
            input.set_value(self.title.clone(), window, cx);
            input.set_selected_range(0..self.title.len(), cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = false;
        if let Some(focus) = self.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    fn accept(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value();
        let title = value.trim();
        if !self.enabled || title.is_empty() || title.chars().any(char::is_control) {
            return;
        }
        if title != self.title {
            title.clone_into(&mut self.title);
            (self.on_rename)(title, cx);
        }
        self.finish(window, cx);
    }

    fn cancel(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(window, cx);
    }
}

impl Render for ThreadTitleEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.editing && self.enabled {
            div()
                .key_context(KEY_CONTEXT)
                .w(px(320.0))
                .max_w_full()
                .min_w_0()
                .h(px(24.0))
                .on_action(cx.listener(Self::accept))
                .on_action(cx.listener(Self::cancel))
                .child(
                    Input::new(&self.input)
                        .xsmall()
                        .w_full()
                        .h_full()
                        .px_2()
                        .focus_bordered(false),
                )
                .into_any_element()
        } else {
            agent_thread_button("agent-inline-title", &self.title, cx)
                .debug_selector(|| "agent-inline-title".into())
                .disabled(!self.enabled)
                .on_click(cx.listener(|this, _, window, cx| this.begin(window, cx)))
                .into_any_element()
        }
    }
}

pub fn agent_thread_title_editor(
    id: impl Into<ElementId>,
    title: &str,
    enabled: bool,
    on_rename: impl Fn(&str, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let editor = window.use_keyed_state(id, cx, ThreadTitleEditor::new);
    editor.update(cx, |editor, cx| {
        if editor.title != title || editor.enabled != enabled {
            title.clone_into(&mut editor.title);
            editor.enabled = enabled;
            if !enabled {
                editor.editing = false;
            }
            cx.notify();
        }
        editor.on_rename = Rc::new(on_rename);
    });
    div()
        .min_w_0()
        .flex_shrink_1()
        .child(editor)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};

    struct TitleTest {
        title: String,
        renamed: Vec<String>,
        composer: Entity<InputState>,
    }

    impl Render for TitleTest {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = cx.entity();
            crate::v_flex()
                .w(px(500.0))
                .child(agent_thread_title_editor(
                    "test-title",
                    &self.title,
                    true,
                    move |title, cx| {
                        view.update(cx, |view, cx| {
                            title.clone_into(&mut view.title);
                            view.renamed.push(title.to_owned());
                            cx.notify();
                        });
                    },
                    window,
                    cx,
                ))
                .child(Input::new(&self.composer))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    fn edit(cx: &mut VisualTestContext) {
        draw(cx);
        let title = cx.debug_bounds("agent-inline-title").unwrap();
        cx.simulate_click(title.center(), Modifiers::default());
        draw(cx);
    }

    #[gpui::test]
    fn inline_rename_saves_on_enter_and_discards_escape_or_blur(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let composer = cx.new(|cx| InputState::new(window, cx));
            composer.update(cx, |input, cx| input.focus(window, cx));
            TitleTest {
                title: "界".repeat(80),
                renamed: Vec::new(),
                composer,
            }
        });
        cx.update(|window, _| window.activate_window());
        edit(cx);
        assert!(cx.update(|window, _| agent_title_is_editing(window)));
        cx.simulate_input("New name");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(
            view.read_with(cx, |view, _| view.renamed.clone()),
            ["New name"]
        );
        assert!(cx.debug_bounds("agent-inline-title").is_some());
        assert!(!cx.update(|window, _| agent_title_is_editing(window)));

        edit(cx);
        cx.simulate_input("Discard this");
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(view.read_with(cx, |view, _| view.title.clone()), "New name");
        assert!(cx.debug_bounds("agent-inline-title").is_some());

        edit(cx);
        cx.simulate_input("Discard on blur");
        cx.update(|window, cx| {
            view.read(cx)
                .composer
                .clone()
                .update(cx, |input, cx| input.focus(window, cx));
        });
        draw(cx);
        assert_eq!(
            view.read_with(cx, |view, _| view.renamed.clone()),
            ["New name"]
        );
        assert!(cx.debug_bounds("agent-inline-title").is_some());
    }
}
