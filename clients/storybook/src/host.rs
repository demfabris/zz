use zpui::{
    AnyView, Context, IntoElement, ParentElement as _, Render, Styled as _, Window, canvas, div,
    px, size,
};
use zz_ui::{ActiveTheme as _, Root};

const PADDING: f32 = 24.0;

pub struct SectionHost {
    content: AnyView,
}

impl SectionHost {
    pub fn new(content: AnyView) -> Self {
        Self { content }
    }
}

impl Render for SectionHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);
        let theme = cx.theme();
        div()
            .relative()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex_none()
                    .p(px(PADDING))
                    .child(self.content.clone())
                    .child(
                        canvas(
                            |bounds, window, _| {
                                let height = (bounds.size.height * window.zoom()).ceil();
                                let current = window.bounds().size;
                                if (current.height - height).abs() > px(0.5) {
                                    window.resize(size(current.width, height));
                                }
                            },
                            |_, (), _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    ),
            )
            .children(dialog_layer)
            .children(notification_layer)
    }
}
