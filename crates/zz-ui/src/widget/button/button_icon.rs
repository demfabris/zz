//! The leading glyph slot of a [`Button`](super::Button).

use gpui::{App, IntoElement, RenderOnce, Window};

use crate::{Icon, Sizable, Size, spinner::Spinner};

#[derive(IntoElement)]
pub(super) struct ButtonIcon {
    icon: Icon,
    loading: bool,
    size: Size,
}

impl ButtonIcon {
    pub(super) fn new(icon: impl Into<Icon>) -> Self {
        Self {
            icon: icon.into(),
            loading: false,
            size: Size::Medium,
        }
    }

    pub(super) fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }
}

impl Sizable for ButtonIcon {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for ButtonIcon {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        if self.loading {
            Spinner::new().with_size(self.size).into_any_element()
        } else {
            self.icon.with_size(self.size).into_any_element()
        }
    }
}
