//! Vim layer: a pure grammar, motion and object core, plus a thin executor over the editor state.

// The editor root blanket-allows dead code; the vim layer opts back in.
#![warn(dead_code)]

mod executor;
mod motion;
mod parser;
mod text_object;

use motion::FindChar;
use parser::Pending;

pub(super) use parser::Key as VimKey;

/// Which mode the vim layer is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl VimMode {
    /// Mode-line text, in vim's own vocabulary.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::VisualLine => "V-LINE",
        }
    }

    pub(super) const fn is_visual(self) -> bool {
        matches!(self, Self::Visual | Self::VisualLine)
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Register {
    pub text: String,
    pub linewise: bool,
}

/// Everything the vim layer remembers between keystrokes.
#[derive(Debug, Default)]
pub struct VimState {
    mode: VimMode,
    pending: Pending,
    register: Register,
    last_find: Option<FindChar>,
    visual_anchor: usize,
}

impl VimState {
    pub(super) const fn mode(&self) -> VimMode {
        self.mode
    }

    pub(super) fn reset(&mut self) {
        self.mode = VimMode::Normal;
        self.pending = Pending::default();
        self.visual_anchor = 0;
    }
}
