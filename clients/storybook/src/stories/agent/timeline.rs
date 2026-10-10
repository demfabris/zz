use std::{cell::Cell, rc::Rc, sync::Arc};

use zz_gpui::{
    AnyElement, App, AppContext as _, Context, Div, Entity, IntoElement, ListAlignment, ListState,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, canvas, div,
    prelude::FluentBuilder as _, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _,
    agent::{
        AgentEntry, AgentTimeline, AgentTimelineStore, DisclosureKind, TimelineRow,
        fold_timeline_rows,
    },
};

use crate::story::states;

const BOTTOM_PADDING: f32 = 4.0;
const FIRST_HEIGHT: f32 = 120.0;
const MAX_HEIGHT: f32 = 1600.0;

pub fn pane_frame(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .relative()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border())
        .bg(theme
            .background
            .opaque()
            .opacity(theme.pane_background_opacity))
}

pub struct Case {
    label: SharedString,
    store: Entity<AgentTimelineStore>,
    rows: Arc<Vec<TimelineRow>>,
    list: ListState,
    height: Rc<Cell<f32>>,
    width: Option<f32>,
    active: bool,
    rewind: bool,
    open_output: bool,
    _observe: Subscription,
}

impl Case {
    pub fn new<V: 'static>(
        label: impl Into<SharedString>,
        entries: &[AgentEntry],
        cx: &mut Context<V>,
    ) -> Self {
        let rows = fold_timeline_rows(entries).rows;
        let store = cx.new(|_| AgentTimelineStore::default());
        let observe = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            label: label.into(),
            list: ListState::new(rows.len(), ListAlignment::Top, px(1200.0)),
            rows,
            store,
            height: Rc::new(Cell::new(FIRST_HEIGHT)),
            width: None,
            active: false,
            rewind: false,
            open_output: false,
            _observe: observe,
        }
    }

    pub fn active(mut self) -> Self {
        self.active = true;
        self
    }

    pub fn rewind(mut self) -> Self {
        self.rewind = true;
        self
    }

    pub fn open_output(mut self) -> Self {
        self.open_output = true;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn expand(self, id: u64, cx: &mut App) -> Self {
        self.store.update(cx, |store, cx| {
            store.set_expanded(id, DisclosureKind::Turn, true, cx);
        });
        self
    }

    pub fn streaming(self, id: u64, cx: &mut App) -> Self {
        self.store
            .update(cx, |store, cx| store.set_streaming(Some(id), cx));
        self
    }

    pub fn render(&self, cx: &App) -> AnyElement {
        let mut timeline =
            AgentTimeline::new(self.rows.clone(), self.list.clone(), self.store.clone())
                .active_turn(self.active)
                .bottom_padding(BOTTOM_PADDING);
        if self.rewind {
            timeline = timeline.rewind(true, |_, _, _| {});
        }
        if self.open_output {
            timeline = timeline.open_output(|_, _, _| {});
        }
        let list = self.list.clone();
        let height = Rc::clone(&self.height);
        let final_row = self.rows.len().checked_sub(1);
        pane_frame(cx)
            .when_some(self.width, |frame, width| frame.w(px(width)))
            .h(px(self.height.get()))
            .child(timeline)
            .child(
                canvas(
                    move |_, window, _| {
                        let viewport = list.viewport_bounds();
                        let wanted = match final_row.and_then(|row| list.bounds_for_item(row)) {
                            Some(item) => {
                                f32::from(item.bottom() - viewport.top()) + BOTTOM_PADDING + 2.0
                            }
                            None => {
                                height.get()
                                    + f32::from(list.max_offset_for_scrollbar().y).max(200.0)
                            }
                        }
                        .clamp(48.0, MAX_HEIGHT);
                        if (wanted - height.get()).abs() > 0.5 {
                            height.set(wanted);
                            window.refresh();
                        }
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .into_any_element()
    }
}

pub struct Cases {
    columns: u16,
    cases: Vec<Case>,
}

impl Cases {
    pub fn new(columns: u16, cases: Vec<Case>) -> Self {
        Self { columns, cases }
    }
}

impl Render for Cases {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.cases
            .iter()
            .fold(states().columns(self.columns), |states, case| {
                states.state(case.label.clone(), case.render(cx))
            })
    }
}
