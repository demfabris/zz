use zz_gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, IntoElement, ParentElement, Render,
    RenderOnce, SharedString, Styled as _, Window, div, px,
};
use zz_ui::{ActiveTheme as _, Colorize as _, h_flex, v_flex};

pub struct Story {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub summary: &'static str,
    pub sections: &'static [Section],
}

#[derive(Clone, Copy)]
pub struct Section {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub build: fn(&mut Window, &mut App) -> AnyView,
}

pub struct Stateless(pub fn(&mut Window, &mut App) -> AnyElement);

impl Render for Stateless {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        (self.0)(window, cx)
    }
}

pub fn stateless(render: fn(&mut Window, &mut App) -> AnyElement, cx: &mut App) -> AnyView {
    cx.new(|_| Stateless(render)).into()
}

#[derive(IntoElement)]
pub struct States {
    columns: u16,
    items: Vec<(SharedString, AnyElement)>,
}

pub fn states() -> States {
    States {
        columns: 1,
        items: Vec::new(),
    }
}

impl States {
    pub fn columns(mut self, columns: u16) -> Self {
        self.columns = columns.max(1);
        self
    }

    pub fn state(mut self, label: impl Into<SharedString>, content: impl IntoElement) -> Self {
        self.items.push((label.into(), content.into_any_element()));
        self
    }
}

impl RenderOnce for States {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().foreground.muted();
        let mono = cx.theme().mono_font_family.clone();
        let cells = self.items.into_iter().map(|(label, content)| {
            v_flex()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .text_size(px(11.0))
                        .line_height(px(14.0))
                        .font_family(mono.clone())
                        .text_color(muted)
                        .child(label),
                )
                .child(content)
        });
        if self.columns == 1 {
            v_flex().w_full().gap_6().children(cells).into_any_element()
        } else {
            div()
                .w_full()
                .grid()
                .grid_cols(self.columns)
                .gap_x_6()
                .gap_y_6()
                .children(cells)
                .into_any_element()
        }
    }
}

#[derive(IntoElement)]
pub struct Row {
    children: Vec<AnyElement>,
}

pub fn row() -> Row {
    Row {
        children: Vec::new(),
    }
}

impl ParentElement for Row {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Row {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .children(self.children)
    }
}
