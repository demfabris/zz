//! The native rope-backed code editor.

use gpui::{AnyElement, Context, ParentElement as _, Styled as _, div, prelude::*, px};
use zz_ui::{ActiveTheme as _, code_editor::CodeEditor};

use super::{Showcase, gallery, story_stack};

pub(super) fn render(showcase: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Rust buffer",
                "The native rope editor with line numbers, soft wrapping, selection, undo, and syntax highlighting.",
                cx,
            )
            .child(
                div()
                    .w_full()
                    .h(px(420.0))
                    .overflow_hidden()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(CodeEditor::new(&showcase.code_editor)),
            ),
        )
        .into_any_element()
}
