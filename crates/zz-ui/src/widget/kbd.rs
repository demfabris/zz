//! Keyboard-shortcut pill: one [`Keystroke`] as a platform-appropriate glyph
//! string, ⌘⇧A on Apple platforms and Ctrl+Shift+A elsewhere.

use gpui::{
    Action, AsKeystroke as _, FocusHandle, IntoElement, KeyContext, Keystroke, ParentElement as _,
    RenderOnce, StyleRefinement, Styled, Window, div, relative,
};

use crate::Colorize as _;
use crate::{ActiveTheme as _, StyledExt as _};

#[cfg(any(target_os = "macos", target_os = "ios"))]
const SEPARATOR: &str = "";
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
const SEPARATOR: &str = "+";

#[cfg(any(target_os = "macos", target_os = "ios"))]
const MODIFIERS: [&str; 4] = ["⌃", "⌥", "⇧", "⌘"];
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
const MODIFIERS: [&str; 4] = ["Ctrl", "Alt", "Shift", "Win"];

#[derive(IntoElement, Clone, Debug)]
pub struct Kbd {
    style: StyleRefinement,
    stroke: Keystroke,
    lowercase: bool,
}

impl Kbd {
    #[must_use]
    pub fn new(stroke: Keystroke) -> Self {
        Self {
            style: StyleRefinement::default(),
            stroke,
            lowercase: false,
        }
    }

    /// Render the key glyph in lower case, leaving modifier glyphs alone.
    #[must_use]
    pub fn lowercase(mut self) -> Self {
        self.lowercase = true;
        self
    }

    /// The highest-precedence binding for `action` as a pill, optionally scoped
    /// to a key context. `None` when the action is unbound.
    #[must_use]
    pub fn binding_for_action(
        action: &dyn Action,
        context: Option<&str>,
        window: &Window,
    ) -> Option<Self> {
        let binding = match context.and_then(|context| KeyContext::parse(context).ok()) {
            Some(context) => {
                window.highest_precedence_binding_for_action_in_context(action, context)
            }
            None => window.highest_precedence_binding_for_action(action),
        }?;
        binding
            .keystrokes()
            .first()
            .map(|key| Self::new(key.as_keystroke().clone()))
    }

    /// The highest-precedence binding for `action` in `focus_handle`'s context,
    /// as a pill. `None` when the action is unbound.
    #[must_use]
    pub fn binding_for_action_in(
        action: &dyn Action,
        focus_handle: &FocusHandle,
        window: &Window,
    ) -> Option<Self> {
        let binding = window.highest_precedence_binding_for_action_in(action, focus_handle)?;
        binding
            .keystrokes()
            .first()
            .map(|key| Self::new(key.as_keystroke().clone()))
    }

    /// Render a keystroke as its platform-specific display string.
    #[must_use]
    pub fn format(key: &Keystroke) -> String {
        Self::format_key(key, false)
    }

    fn format_key(key: &Keystroke, lowercase: bool) -> String {
        let m = &key.modifiers;
        let mut parts: Vec<&str> = [
            m.control.then_some(MODIFIERS[0]),
            m.alt.then_some(MODIFIERS[1]),
            m.shift.then_some(MODIFIERS[2]),
            m.platform.then_some(MODIFIERS[3]),
        ]
        .into_iter()
        .flatten()
        .collect();

        let key = key_symbol(key.key.as_str());
        let key = if lowercase { key.to_lowercase() } else { key };
        parts.push(&key);
        parts.join(SEPARATOR)
    }
}

fn key_symbol(key: &str) -> String {
    macro_rules! platform {
        ($mac:literal, $other:literal) => {{
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            {
                $mac.to_string()
            }
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            {
                $other.to_string()
            }
        }};
    }

    match key {
        "ctrl" => platform!("⌃", "Ctrl"),
        "alt" => platform!("⌥", "Alt"),
        "shift" => platform!("⇧", "Shift"),
        "cmd" => platform!("⌘", "Win"),
        "backspace" => platform!("⌫", "Backspace"),
        "delete" => platform!("⌫", "Delete"),
        "escape" => platform!("⎋", "Esc"),
        "enter" => platform!("⏎", "Enter"),
        "left" => platform!("←", "Left"),
        "right" => platform!("→", "Right"),
        "up" => platform!("↑", "Up"),
        "down" => platform!("↓", "Down"),
        "space" => "Space".to_string(),
        "pagedown" => "Page Down".to_string(),
        "pageup" => "Page Up".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

impl Styled for Kbd {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        div()
            .text_color(cx.theme().foreground.muted())
            .bg(cx.theme().background.raised(2))
            .py_0p5()
            .px_1()
            .min_w_5()
            .text_center()
            .rounded(cx.theme().radius)
            .line_height(relative(1.))
            .text_xs()
            .whitespace_normal()
            .flex_shrink_0()
            .refine_style(&self.style)
            .child(Self::format_key(&self.stroke, self.lowercase))
    }
}

#[cfg(test)]
mod tests {
    use super::Kbd;
    use gpui::Keystroke;

    #[test]
    fn lowercase_touches_only_the_key_glyph() {
        let key = |s: &str| Keystroke::parse(s).unwrap();
        assert_eq!(Kbd::format(&key("t")), "T");
        assert_eq!(Kbd::format_key(&key("t"), true), "t");
        let modified = Kbd::format_key(&key("ctrl-a"), true);
        assert!(modified.ends_with('a'), "{modified}");
        assert_eq!(
            modified.trim_end_matches('a'),
            Kbd::format(&key("ctrl-a")).trim_end_matches('A')
        );
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn formats_apple_glyphs() {
        let f = |s: &str| Kbd::format(&Keystroke::parse(s).unwrap());
        assert_eq!(f("cmd-a"), "⌘A");
        assert_eq!(f("cmd--"), "⌘-");
        assert_eq!(f("cmd-+"), "⌘+");
        assert_eq!(f("cmd-enter"), "⌘⏎");
        assert_eq!(f("secondary-f12"), "⌘F12");
        assert_eq!(f("shift-pagedown"), "⇧Page Down");
        assert_eq!(f("shift-pageup"), "⇧Page Up");
        assert_eq!(f("shift-space"), "⇧Space");
        assert_eq!(f("cmd-ctrl-a"), "⌃⌘A");
        assert_eq!(f("cmd-alt-backspace"), "⌥⌘⌫");
        assert_eq!(f("shift-delete"), "⇧⌫");
        assert_eq!(f("cmd-ctrl-shift-a"), "⌃⇧⌘A");
        assert_eq!(f("cmd-ctrl-shift-alt-a"), "⌃⌥⇧⌘A");
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    #[test]
    fn formats_named_keys() {
        let f = |s: &str| Kbd::format(&Keystroke::parse(s).unwrap());
        assert_eq!(f("a"), "A");
        assert_eq!(f("ctrl-a"), "Ctrl+A");
        assert_eq!(f("shift-space"), "Shift+Space");
        assert_eq!(f("ctrl-alt-a"), "Ctrl+Alt+A");
        assert_eq!(f("ctrl-alt-shift-a"), "Ctrl+Alt+Shift+A");
        assert_eq!(f("ctrl-alt-shift-win-a"), "Ctrl+Alt+Shift+Win+A");
        assert_eq!(f("ctrl-shift-backspace"), "Ctrl+Shift+Backspace");
        assert_eq!(f("alt-delete"), "Alt+Delete");
        assert_eq!(f("alt-tab"), "Alt+Tab");
    }
}
