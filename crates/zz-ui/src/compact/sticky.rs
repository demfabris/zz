use zpui::{App, Keystroke, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickyModifier {
    Control,
    Alt,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StickyModifiers {
    pub control: bool,
    pub alt: bool,
}

impl zpui::Global for StickyModifiers {}

impl StickyModifiers {
    pub fn get(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }

    pub fn toggle(modifier: StickyModifier, cx: &mut App) {
        let latched = cx.default_global::<Self>();
        match modifier {
            StickyModifier::Control => latched.control = !latched.control,
            StickyModifier::Alt => latched.alt = !latched.alt,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.control && !self.alt
    }

    pub fn is_latched(&self, modifier: StickyModifier) -> bool {
        match modifier {
            StickyModifier::Control => self.control,
            StickyModifier::Alt => self.alt,
        }
    }

    pub fn take(cx: &mut App) -> Self {
        if Self::get(cx).is_empty() {
            return Self::default();
        }
        std::mem::take(cx.default_global::<Self>())
    }

    pub fn apply(keystroke: &Keystroke, cx: &mut App) -> Keystroke {
        let latched = Self::take(cx);
        if latched.is_empty() {
            return keystroke.clone();
        }
        Keystroke {
            modifiers: Modifiers {
                control: keystroke.modifiers.control || latched.control,
                alt: keystroke.modifiers.alt || latched.alt,
                ..keystroke.modifiers
            },
            key: keystroke.key.clone(),
            key_char: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use zpui::TestAppContext;

    use super::*;

    #[zpui::test]
    fn toggle_latches_and_unlatches(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(StickyModifiers::get(cx).is_empty());
            StickyModifiers::toggle(StickyModifier::Control, cx);
            assert_eq!(
                StickyModifiers::get(cx),
                StickyModifiers {
                    control: true,
                    alt: false
                }
            );
            StickyModifiers::toggle(StickyModifier::Alt, cx);
            StickyModifiers::toggle(StickyModifier::Control, cx);
            assert_eq!(
                StickyModifiers::get(cx),
                StickyModifiers {
                    control: false,
                    alt: true
                }
            );
        });
    }

    #[zpui::test]
    fn take_reads_and_clears(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(StickyModifiers::take(cx).is_empty());
            StickyModifiers::toggle(StickyModifier::Alt, cx);
            let taken = StickyModifiers::take(cx);
            assert!(taken.alt);
            assert!(taken.is_latched(StickyModifier::Alt));
            assert!(!taken.is_latched(StickyModifier::Control));
            assert!(StickyModifiers::get(cx).is_empty());
        });
    }

    #[zpui::test]
    fn apply_merges_the_latch_once(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let plain = Keystroke::parse("c").unwrap();
            let plain = Keystroke {
                key_char: Some("c".into()),
                ..plain
            };
            assert_eq!(StickyModifiers::apply(&plain, cx), plain);

            StickyModifiers::toggle(StickyModifier::Control, cx);
            let applied = StickyModifiers::apply(&plain, cx);
            assert!(applied.modifiers.control);
            assert!(!applied.modifiers.alt);
            assert_eq!(applied.key, "c");
            assert_eq!(applied.key_char, None);
            assert!(StickyModifiers::get(cx).is_empty());
            assert_eq!(StickyModifiers::apply(&plain, cx), plain);

            StickyModifiers::toggle(StickyModifier::Alt, cx);
            let shifted = Keystroke::parse("shift-tab").unwrap();
            let applied = StickyModifiers::apply(&shifted, cx);
            assert!(applied.modifiers.alt && applied.modifiers.shift);
            assert!(!applied.modifiers.control);
        });
    }
}
