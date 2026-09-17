use super::mode_prompt::{ModeKey, ModePrompt, PromptOutcome};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchMode {
    pub windows: bool,
    pub format: Option<String>,
    pub template: Option<String>,
    pub kill_source: bool,
    pub zoom: bool,
    pub prompt: ModePrompt,
    pub filter: String,
    pub current: usize,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchAction {
    Redraw,
    Run,
    Exit,
}

impl SwitchMode {
    #[must_use]
    pub fn new(
        windows: bool,
        format: Option<String>,
        template: Option<String>,
        kill_source: bool,
        zoom: bool,
        word_separators: &str,
    ) -> Self {
        Self {
            windows,
            format,
            template,
            kill_source,
            zoom,
            prompt: ModePrompt::incremental("(search) ", "", word_separators),
            filter: String::new(),
            current: 0,
            offset: 0,
        }
    }

    pub fn set_current(&mut self, current: usize, size: usize, visible: usize) {
        if size == 0 {
            self.current = 0;
            self.offset = 0;
            return;
        }
        self.current = current.min(size - 1);
        if self.current < self.offset {
            self.offset = self.current;
        } else if visible != 0 && self.current >= self.offset + visible {
            self.offset = self.current + 1 - visible;
        }
    }

    pub fn key(&mut self, name: &str, size: usize, visible: usize) -> SwitchAction {
        let key = match ModeKey::parse(name) {
            ModeKey::Ctrl('p' | 'k') => ModeKey::Up,
            ModeKey::Ctrl('n' | 'j') => ModeKey::Down,
            key => key,
        };
        if key == ModeKey::Char('\r') {
            return SwitchAction::Run;
        }
        if key.is_cancel() {
            return SwitchAction::Exit;
        }
        match self.prompt.key(key) {
            PromptOutcome::Changed(_) => {
                self.filter = self.prompt.input();
                self.current = 0;
                self.offset = 0;
                return SwitchAction::Redraw;
            }
            PromptOutcome::Move => {}
            _ => return SwitchAction::Redraw,
        }
        let current = self.current;
        match key {
            ModeKey::Up if size != 0 => {
                self.set_current(current.checked_sub(1).unwrap_or(size - 1), size, visible);
            }
            ModeKey::Down if size != 0 => {
                let next = if current + 1 >= size { 0 } else { current + 1 };
                self.set_current(next, size, visible);
            }
            ModeKey::PageUp => {
                self.set_current(current.saturating_sub(visible), size, visible);
            }
            ModeKey::PageDown => self.set_current(current + visible, size, visible),
            _ => {}
        }
        SwitchAction::Redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_wraps_and_a_filter_edit_returns_to_the_top() {
        let mut mode = SwitchMode::new(false, None, None, false, false, " ");
        assert_eq!(mode.key("Down", 3, 22), SwitchAction::Redraw);
        assert_eq!(mode.current, 1);
        mode.key("C-p", 3, 22);
        mode.key("Up", 3, 22);
        assert_eq!(mode.current, 2);
        mode.key("PPage", 3, 22);
        assert_eq!(mode.current, 0);
        mode.key("NPage", 3, 22);
        assert_eq!(mode.current, 2);
        mode.key("z", 3, 22);
        assert_eq!((mode.filter.as_str(), mode.current), ("z", 0));
        mode.key("Home", 1, 22);
        assert_eq!(mode.prompt.draw(80).1, 9);
        assert_eq!(mode.key("Enter", 1, 22), SwitchAction::Run);
        assert_eq!(mode.key("C-g", 1, 22), SwitchAction::Exit);
    }
}
