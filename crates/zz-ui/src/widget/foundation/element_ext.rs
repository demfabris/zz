//! Extensions on gpui's element traits.

use gpui::{
    App, Bounds, ClickEvent, InteractiveElement, ParentElement, Pixels, Stateful, Styled as _,
    Window, canvas,
};

pub trait ElementExt: ParentElement + Sized {
    /// Run `f` during prepaint with this element's resolved bounds.
    fn on_prepaint<F>(self, f: F) -> Self
    where
        F: FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    {
        self.child(
            canvas(
                move |bounds, window, cx| f(bounds, window, cx),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
    }
}

impl<T: ParentElement> ElementExt for T {}

/// Extends [`gpui::InteractiveElement`] with events gpui does not surface.
pub trait InteractiveElementExt: InteractiveElement {
    fn on_double_click(
        mut self,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_click(move |event, window, cx| {
            if event.click_count() == 2 {
                listener(event, window, cx);
            }
        });
        self
    }
}

impl<E: InteractiveElement> InteractiveElementExt for Stateful<E> {}
