use gpui::SharedString;

use super::TabSize;

#[derive(Clone)]
pub(crate) struct EditorMode {
    language: SharedString,
    tab: TabSize,
    line_numbers: bool,
    relative_line_numbers: bool,
    indent_guides: bool,
}

impl Default for EditorMode {
    fn default() -> Self {
        Self {
            language: "text".into(),
            tab: TabSize::default(),
            line_numbers: true,
            relative_line_numbers: false,
            indent_guides: true,
        }
    }
}

impl EditorMode {
    pub(crate) fn language(&self) -> &SharedString {
        &self.language
    }

    pub(crate) fn set_language(&mut self, language: impl Into<SharedString>) {
        self.language = language.into();
    }

    pub(crate) const fn tab_size(&self) -> TabSize {
        self.tab
    }

    pub(crate) fn set_tab_size(&mut self, tab: TabSize) {
        self.tab = tab;
    }

    pub(crate) const fn line_numbers(&self) -> bool {
        self.line_numbers
    }

    pub(crate) fn set_line_numbers(&mut self, enabled: bool) {
        self.line_numbers = enabled;
    }

    pub(crate) const fn relative_line_numbers(&self) -> bool {
        self.relative_line_numbers
    }

    pub(crate) fn set_relative_line_numbers(&mut self, enabled: bool) {
        self.relative_line_numbers = enabled;
    }

    pub(crate) const fn indent_guides(&self) -> bool {
        self.indent_guides
    }
}
