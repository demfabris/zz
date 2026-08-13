//! Toggle switch.

use std::{rc::Rc, time::Duration};

use gpui::{
    Animation, AnimationExt as _, App, Background, ElementId, Entity, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, RenderOnce, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::Colorize as _;
use crate::{ActiveTheme as _, Disableable, Sizable, Size, StyledExt as _, h_flex};

const TOGGLE_ANIMATION: Duration = Duration::from_millis(150);

type ClickHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

struct Metrics {
    track_w: Pixels,
    track_h: Pixels,
    thumb: Pixels,
    inset: Pixels,
}

impl Metrics {
    fn for_compact(compact: bool) -> Self {
        if compact {
            Self {
                track_w: px(28.),
                track_h: px(16.),
                thumb: px(12.),
                inset: px(2.),
            }
        } else {
            Self {
                track_w: px(36.),
                track_h: px(20.),
                thumb: px(16.),
                inset: px(2.),
            }
        }
    }

    fn travel(&self) -> Pixels {
        self.track_w - self.thumb - self.inset * 2
    }
}

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    style: StyleRefinement,
    checked: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
    size: Size,
    tooltip: Option<SharedString>,
}

impl Switch {
    /// A switch with the given stable id (needed for the thumb animation state).
    #[must_use]
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            checked: false,
            disabled: false,
            on_click: None,
            size: Size::Medium,
            tooltip: None,
        }
    }

    #[must_use]
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Handle a toggle. The bool argument is the *new* state.
    #[must_use]
    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }

    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

impl Styled for Switch {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Sizable for Switch {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Disableable for Switch {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

fn thumb(
    checked: bool,
    disabled: bool,
    fill: Background,
    m: &Metrics,
    toggle_state: &Entity<bool>,
    cx: &mut App,
) -> impl IntoElement {
    let travel = m.travel();
    div()
        .rounded_full()
        .bg(fill)
        .shadow_md()
        .size(m.thumb)
        .map(|this| {
            if disabled || *toggle_state.read(cx) == checked {
                let x = if checked { travel } else { px(0.) };
                return this.left(x).into_any_element();
            }

            cx.spawn({
                let toggle_state = toggle_state.clone();
                async move |cx| {
                    cx.background_executor().timer(TOGGLE_ANIMATION).await;
                    let () = toggle_state.update(cx, |this, _| *this = checked);
                }
            })
            .detach();

            this.with_animation(
                ElementId::NamedInteger("move".into(), u64::from(checked)),
                Animation::new(TOGGLE_ANIMATION),
                move |this, delta| {
                    let x = if checked {
                        travel * delta
                    } else {
                        travel - travel * delta
                    };
                    this.left(x)
                },
            )
            .into_any_element()
        })
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let checked = self.checked;
        let m = Metrics::for_compact(matches!(self.size, Size::XSmall | Size::Small));

        let toggle_state = window.use_keyed_state(self.id.clone(), cx, |_, _| checked);

        let base_track: Background = if checked {
            cx.theme().foreground.into()
        } else {
            cx.theme().background.raised(3).into()
        };
        let base_thumb: Background = cx.theme().background.into();
        let (track, thumb_fill) = if self.disabled {
            let track = if checked {
                base_track.opacity(0.5)
            } else {
                base_track
            };
            (track, base_thumb.opacity(0.35))
        } else {
            (base_track, base_thumb)
        };

        div().refine_style(&self.style).child(
            h_flex()
                .id(self.id.clone())
                .gap_2()
                .items_start()
                .child(
                    div()
                        .id(self.id.clone())
                        .w(m.track_w)
                        .h(m.track_h)
                        .rounded_full()
                        .flex()
                        .items_center()
                        .border(m.inset)
                        .border_color(cx.theme().transparent)
                        .bg(track)
                        .when_some(self.tooltip.clone(), |this, tooltip| {
                            this.tooltip(move |window, cx| {
                                crate::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                        })
                        .child(thumb(
                            checked,
                            self.disabled,
                            thumb_fill,
                            &m,
                            &toggle_state,
                            cx,
                        )),
                )
                .when_some(
                    self.on_click.clone().filter(|_| !self.disabled),
                    |this, on_click| {
                        let toggle_state = toggle_state.clone();
                        this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            let () = toggle_state.update(cx, |this, _| *this = checked);
                            on_click(&!checked, window, cx);
                        })
                    },
                ),
        )
    }
}
