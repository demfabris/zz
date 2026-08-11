use gpui::{Context, SharedString, Window};

use super::{
    CodeEditorState, RopeExt as _,
    state::{Indent, IndentInline, Outdent, OutdentInline},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TabSize {
    /// Number of spaces represented by one indentation level.
    pub tab_size: usize,
    /// Insert a literal tab instead of spaces.
    pub hard_tabs: bool,
}

impl Default for TabSize {
    fn default() -> Self {
        Self {
            tab_size: 2,
            hard_tabs: false,
        }
    }
}

impl TabSize {
    pub(super) fn text(self) -> SharedString {
        if self.hard_tabs {
            "\t".into()
        } else {
            " ".repeat(self.tab_size).into()
        }
    }
}

impl CodeEditorState {
    pub fn tab_size(mut self, tab: TabSize) -> Self {
        self.mode.set_tab_size(tab);
        self
    }

    pub(super) fn indent_inline(
        &mut self,
        _: &IndentInline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vim_intercepts_text() {
            return;
        }
        self.indent_selection(false, cx);
    }

    pub(super) fn indent_block(&mut self, _: &Indent, _: &mut Window, cx: &mut Context<Self>) {
        self.indent_selection(true, cx);
    }

    pub(super) fn outdent_inline(
        &mut self,
        _: &OutdentInline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vim_intercepts_text() {
            return;
        }
        self.outdent_selection(false, cx);
    }

    pub(super) fn outdent_block(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        self.outdent_selection(true, cx);
    }

    pub(super) fn indent_selection(&mut self, force_block: bool, cx: &mut Context<Self>) {
        let indent = self.mode.tab_size().text();
        let selection = self.selected_range;
        if selection.is_empty() && !force_block {
            self.replace_range(selection.start..selection.end, indent.as_ref(), true, cx);
            return;
        }

        let start_line = self.text.offset_to_point(selection.start).row;
        let end_line = self
            .text
            .offset_to_point(
                selection
                    .end
                    .saturating_sub(usize::from(!selection.is_empty())),
            )
            .row;
        let start = self.text.line_start_offset(start_line);
        let end = self.text.line_end_offset(end_line);
        let source = self.text.slice(start..end).to_string();
        let replacement = source
            .split_inclusive('\n')
            .map(|line| format!("{indent}{line}"))
            .collect::<String>();
        self.replace_range(start..end, &replacement, false, cx);
        self.selected_range = (start..start + replacement.len()).into();
        cx.notify();
    }

    pub(super) fn outdent_selection(&mut self, force_block: bool, cx: &mut Context<Self>) {
        let selection = self.selected_range;
        let start_line = self.text.offset_to_point(selection.start).row;
        let end_line = if selection.is_empty() && !force_block {
            start_line
        } else {
            self.text
                .offset_to_point(
                    selection
                        .end
                        .saturating_sub(usize::from(!selection.is_empty())),
                )
                .row
        };
        let start = self.text.line_start_offset(start_line);
        let end = self.text.line_end_offset(end_line);
        let source = self.text.slice(start..end).to_string();
        let spaces = " ".repeat(self.mode.tab_size().tab_size);
        let replacement = source
            .split_inclusive('\n')
            .map(|line| {
                if let Some(rest) = line.strip_prefix('\t') {
                    rest.to_string()
                } else {
                    let count = line
                        .bytes()
                        .take_while(|byte| *byte == b' ')
                        .count()
                        .min(spaces.len());
                    line[count..].to_string()
                }
            })
            .collect::<String>();

        if replacement == source {
            return;
        }
        self.replace_range(start..end, &replacement, false, cx);
        let cursor = selection
            .start
            .saturating_sub(source.len() - replacement.len())
            .max(start);
        self.selected_range = (cursor..cursor).into();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_tab_default_is_two_soft_spaces() {
        let tab = TabSize::default();
        assert_eq!(tab.text().as_ref(), "  ");
    }
}
