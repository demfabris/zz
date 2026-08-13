//! Window-level text selection: one drag, many `TextView`s. Endpoints are
//! anchored to a view's content, so a selection stays put when layout shifts.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

use std::collections::HashMap;

use gpui::{
    App, Bounds, Element, ElementId, Entity, EntityId, GlobalElementId, Hitbox, InspectorElementId,
    IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollWheelEvent, Style, WeakEntity, Window,
};

use crate::Root;

use super::{auto_scroll::AutoScroll, global::TextGlobal, state::TextViewState};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SelectionScope {
    Base,
    Dialog(usize),
}

pub(super) fn register_selectable_text_view(
    state: &Entity<TextViewState>,
    hitbox: &Hitbox,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(root) = window.root::<Root>().flatten() else {
        return;
    };
    let id = state.entity_id();
    let weak = state.downgrade();
    let hitbox = hitbox.clone();
    let scope = TextGlobal::current_scope(cx);
    root.update(cx, |root, _| {
        let selection = &mut root.text_selection;
        selection
            .views
            .retain(|_, (view, _, _)| view.upgrade().is_some());
        selection.views.insert(id, (weak, hitbox, scope));
        selection.inlines.remove(&id);
    });
}

pub(super) fn register_selectable_text_inline(
    state: &Entity<TextViewState>,
    text_bounds: Vec<Bounds<Pixels>>,
    window: &mut Window,
    cx: &mut App,
) {
    if text_bounds.is_empty() {
        return;
    }
    let Some(root) = window.root::<Root>().flatten() else {
        return;
    };
    let id = state.entity_id();
    root.update(cx, |root, _| {
        root.text_selection
            .inlines
            .entry(id)
            .or_default()
            .extend(text_bounds);
    });
}

/// Window-level text selection state, owned by [`Root`]. Drives all text
/// selection, including a drag inside a single `TextView`.
#[derive(Default)]
pub struct WindowTextSelection {
    anchor: Option<SelectionEndpoint>,
    cursor: Option<SelectionEndpoint>,
    is_selecting: bool,
    did_hit_text: bool,
    views: HashMap<EntityId, (WeakEntity<TextViewState>, Hitbox, SelectionScope)>,
    inlines: HashMap<EntityId, Vec<Bounds<Pixels>>>,
}

#[derive(Clone)]
struct SelectionEndpoint {
    view: Option<WeakEntity<TextViewState>>,
    point: Point<Pixels>,
    inside: bool,
    inside_text: bool,
}

impl SelectionEndpoint {
    fn resolve(&self, cx: &App) -> Option<Point<Pixels>> {
        match &self.view {
            Some(view) => {
                let state = view.upgrade()?;
                let state = state.read(cx);
                Some(self.point + state.scroll_offset() + state.bounds().origin)
            }
            None => Some(self.point),
        }
    }

    fn view_id(&self) -> Option<EntityId> {
        self.view.as_ref().map(|view| view.entity_id())
    }
}

impl WindowTextSelection {
    pub(super) fn resolved_points(&self, cx: &App) -> Option<(Point<Pixels>, Point<Pixels>)> {
        if !self.did_hit_text {
            return None;
        }
        let start = self.anchor.as_ref()?.resolve(cx)?;
        let end = self.cursor.as_ref()?.resolve(cx)?;
        if start == end {
            return None;
        }
        Some((start, end))
    }

    pub(super) fn single_view(&self) -> Option<EntityId> {
        let anchor = self.anchor.as_ref()?.view_id()?;
        let cursor = self.cursor.as_ref()?.view_id()?;
        (anchor == cursor).then_some(anchor)
    }

    fn involves(&self, view_id: EntityId) -> bool {
        self.anchor.as_ref().and_then(|e| e.view_id()) == Some(view_id)
            || self.cursor.as_ref().and_then(|e| e.view_id()) == Some(view_id)
    }

    /// Whether there is an active text selection (window-level or view-local).
    pub fn has_selection(&self, cx: &App) -> bool {
        if self.resolved_points(cx).is_some() {
            return true;
        }
        self.views.values().any(|(view, _, _)| {
            view.upgrade()
                .is_some_and(|view| view.read(cx).has_view_selection())
        })
    }

    /// The merged selected text across the views in `scope`, in document order.
    /// Reflects the last painted frame.
    pub fn selected_text(&self, scope: SelectionScope, cx: &App) -> String {
        let resolved = self.resolved_points(cx);
        let single_view = self.single_view();
        let mut items: Vec<(Point<Pixels>, String)> = Vec::new();
        for (id, (view, _, view_scope)) in self.views.iter() {
            let Some(view) = view.upgrade() else { continue };
            let state = view.read(cx);
            let in_window_selection = resolved.is_some()
                && state.is_selectable()
                && *view_scope == scope
                && single_view.map_or(true, |v| v == *id);
            if !state.has_view_selection() && !in_window_selection {
                continue;
            }
            let text = state.selected_text();
            if text.trim().is_empty() {
                continue;
            }
            items.push((state.bounds().origin, text));
        }

        items.sort_by(|a, b| {
            a.0.y
                .partial_cmp(&b.0.y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    a.0.x
                        .partial_cmp(&b.0.x)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });

        items
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clear the window selection and all view-local selections.
    pub fn clear(&mut self, cx: &mut App) {
        let had_window_selection = self.anchor.is_some();
        self.anchor = None;
        self.cursor = None;
        self.is_selecting = false;
        self.did_hit_text = false;
        self.views.retain(|_, (view, _, _)| {
            let Some(view) = view.upgrade() else {
                return false;
            };
            if had_window_selection || view.read(cx).has_view_selection() {
                view.update(cx, |state, cx| {
                    state.is_selecting = false;
                    state.clear_selection(cx);
                });
            }
            true
        });
        self.inlines.retain(|id, _| self.views.contains_key(id));
    }

    pub(super) fn clear_for_resized_view(&mut self, view_id: EntityId, cx: &mut App) {
        if self.is_selecting {
            return;
        }
        if self.involves(view_id) {
            self.clear(cx);
        }
    }

    pub(super) fn start(
        &mut self,
        position: Point<Pixels>,
        scope: SelectionScope,
        window: &mut Window,
        cx: &mut App,
    ) {
        let endpoint = self.endpoint(position, scope, window, cx);
        if endpoint.inside {
            if let Some(view) = endpoint.view.as_ref().and_then(|v| v.upgrade()) {
                view.update(cx, |state, cx| {
                    state.is_selecting = true;
                    state.focus_handle.focus(window, cx);
                });
            }
        }
        self.did_hit_text = endpoint.inside_text;
        self.anchor = Some(endpoint.clone());
        self.cursor = Some(endpoint);
        self.is_selecting = true;
    }

    pub(super) fn update(
        &mut self,
        position: Point<Pixels>,
        scope: SelectionScope,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !self.is_selecting {
            return;
        }
        if cx.has_active_drag() {
            return;
        }

        let old_points = self.resolved_points(cx);
        let endpoint = self.endpoint(position, scope, window, cx);
        self.did_hit_text |= endpoint.inside_text;
        self.cursor = Some(endpoint);
        let new_points = self.resolved_points(cx);

        if let Some(view) = self
            .anchor
            .as_ref()
            .filter(|e| e.inside)
            .and_then(|e| e.view.as_ref())
            .and_then(|v| v.upgrade())
        {
            view.update(cx, |state, cx| {
                if state.scrollable {
                    let delta = AutoScroll::compute_delta(position.y, state.bounds());
                    state.set_auto_scroll(delta, cx);
                }
            });
        }

        self.notify_selection_band(old_points, new_points, cx);
    }

    pub fn end(&mut self, cx: &mut App) {
        if !self.is_selecting {
            return;
        }
        self.is_selecting = false;
        if !self.did_hit_text {
            self.anchor = None;
            self.cursor = None;
            return;
        }
        if let Some(view) = self
            .anchor
            .as_ref()
            .filter(|e| e.inside)
            .and_then(|e| e.view.as_ref())
            .and_then(|v| v.upgrade())
        {
            view.update(cx, |state, cx| {
                state.is_selecting = false;
                state.stop_auto_scroll();
                cx.notify();
            });
        }
        self.notify_selectable_text_views(cx);
    }

    fn endpoint(
        &self,
        position: Point<Pixels>,
        scope: SelectionScope,
        window: &Window,
        cx: &App,
    ) -> SelectionEndpoint {
        let mut best: Option<(WeakEntity<TextViewState>, f32)> = None;
        for (view, hitbox, view_scope) in self.views.values() {
            if *view_scope != scope {
                continue;
            }
            if view.upgrade().is_none() {
                continue;
            }
            if !hitbox.is_hovered(window) {
                continue;
            }
            let area = f32::from(hitbox.bounds.size.width) * f32::from(hitbox.bounds.size.height);
            if best.as_ref().map_or(true, |(_, a)| area < *a) {
                best = Some((view.clone(), area));
            }
        }

        if let Some((view, entity)) =
            best.and_then(|(view, _)| view.upgrade().map(|entity| (view, entity)))
        {
            let state = entity.read(cx);
            let inside_text = self
                .inlines
                .get(&state.entity_id)
                .is_some_and(|bounds| bounds.iter().any(|bounds| bounds.contains(&position)));
            return SelectionEndpoint {
                point: position - state.bounds().origin - state.scroll_offset(),
                view: Some(view),
                inside: true,
                inside_text,
            };
        }

        let mut predecessor: Option<(WeakEntity<TextViewState>, Pixels)> = None;
        let mut first: Option<(WeakEntity<TextViewState>, Pixels)> = None;
        for (view, _, view_scope) in self.views.values() {
            if *view_scope != scope {
                continue;
            }
            let Some(entity) = view.upgrade() else {
                continue;
            };
            let top = entity.read(cx).bounds().top();
            if top <= position.y {
                if predecessor.as_ref().map_or(true, |(_, t)| top > *t) {
                    predecessor = Some((view.clone(), top));
                }
            }
            if first.as_ref().map_or(true, |(_, t)| top < *t) {
                first = Some((view.clone(), top));
            }
        }

        match predecessor.or(first) {
            Some((view, _)) => match view.upgrade() {
                Some(entity) => {
                    let state = entity.read(cx);
                    SelectionEndpoint {
                        point: position - state.bounds().origin - state.scroll_offset(),
                        view: Some(view),
                        inside: false,
                        inside_text: false,
                    }
                }
                None => SelectionEndpoint {
                    view: None,
                    point: position,
                    inside: false,
                    inside_text: false,
                },
            },
            None => SelectionEndpoint {
                view: None,
                point: position,
                inside: false,
                inside_text: false,
            },
        }
    }

    fn notify_selectable_text_views(&mut self, cx: &mut App) {
        self.views.retain(|_, (view, _, _)| {
            let Some(view) = view.upgrade() else {
                return false;
            };
            view.update(cx, |_, cx| cx.notify());
            true
        });
    }

    fn notify_selection_band(
        &mut self,
        old_points: Option<(Point<Pixels>, Point<Pixels>)>,
        new_points: Option<(Point<Pixels>, Point<Pixels>)>,
        cx: &mut App,
    ) {
        if old_points.is_none() {
            if let Some(id) = self.single_view() {
                if let Some((view, _, _)) = self.views.get(&id) {
                    if let Some(view) = view.upgrade() {
                        view.update(cx, |_, cx| cx.notify());
                    }
                }
                return;
            }
        }

        let band = |points: Option<(Point<Pixels>, Point<Pixels>)>| {
            points.map(|(a, b)| {
                let (lo, hi) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
                (lo, hi)
            })
        };
        let (band_min, band_max) = match (band(old_points), band(new_points)) {
            (Some((lo_a, hi_a)), Some((lo_b, hi_b))) => (lo_a.min(lo_b), hi_a.max(hi_b)),
            (Some(b), None) | (None, Some(b)) => b,
            (None, None) => return,
        };

        self.views.retain(|_, (view, _, _)| {
            let Some(view) = view.upgrade() else {
                return false;
            };
            let bounds = view.read(cx).bounds();
            if bounds.top() <= band_max && bounds.bottom() >= band_min {
                view.update(cx, |_, cx| cx.notify());
            }
            true
        });
    }

    #[cfg(test)]
    pub(crate) fn inline_bounds(&self) -> impl Iterator<Item = &Vec<Bounds<Pixels>>> {
        self.inlines.values()
    }
}

/// Zero-size element driving window text selection. Must be `Root`'s first
/// child: bubble listeners fire in reverse registration order, so registering
/// earliest is what makes this one run after every interactive widget.
pub(crate) struct TextSelectionController;

impl IntoElement for TextSelectionController {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextSelectionController {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if event.button != MouseButton::Left {
                return;
            }
            if phase.capture() {
                TextGlobal::clear_suppressed(cx);
                Root::update(window, cx, |root, _, cx| root.text_selection.clear(cx));
            } else if event.click_count == 1 {
                if TextGlobal::is_suppressed(cx) {
                    return;
                }
                Root::update(window, cx, |root, window, cx| {
                    let scope = root.text_selection_scope();
                    root.text_selection.start(event.position, scope, window, cx);
                });
            }
        });

        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }
            Root::update(window, cx, |root, window, cx| {
                let scope = root.text_selection_scope();
                root.text_selection
                    .update(event.position, scope, window, cx);
            });
        });

        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }
            Root::update(window, cx, |root, _, cx| root.text_selection.end(cx));
        });

        window.on_mouse_event(move |_: &ScrollWheelEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }
            let position = window.mouse_position();
            Root::update(window, cx, |root, window, cx| {
                let scope = root.text_selection_scope();
                root.text_selection.update(position, scope, window, cx);
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Root,
        text::{TextView, TextViewState, suppress_text_selection},
    };
    use gpui::{
        AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
        Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement as _, Render,
        Styled as _, TestAppContext, VisualTestContext, Window, div, point, px,
    };
    use std::cell::Cell;
    use std::rc::Rc;

    struct ChatTestView {
        focus_handle: FocusHandle,
        first: Entity<TextViewState>,
        second: Entity<TextViewState>,
        second_selectable: bool,
        top_offset: gpui::Pixels,
        mid_gap: gpui::Pixels,
    }

    impl ChatTestView {
        fn new(second_selectable: bool, cx: &mut Context<Self>) -> Self {
            Self {
                focus_handle: cx.focus_handle(),
                first: cx.new(|cx| TextViewState::markdown("Hello world", cx)),
                second: cx.new(|cx| TextViewState::markdown("Second message", cx)),
                second_selectable,
                top_offset: px(10.),
                mid_gap: px(0.),
            }
        }
    }

    impl Render for ChatTestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .track_focus(&self.focus_handle)
                .size_full()
                .pt(self.top_offset)
                .child(
                    div()
                        .h(px(40.))
                        .child(TextView::new(&self.first).selectable(true)),
                )
                .child(div().h(self.mid_gap))
                .child(
                    div()
                        .h(px(40.))
                        .child(TextView::new(&self.second).selectable(self.second_selectable)),
                )
                .child(
                    div()
                        .h(px(20.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            suppress_text_selection(cx);
                        }),
                )
        }
    }

    fn setup(
        second_selectable: bool,
        cx: &mut TestAppContext,
    ) -> (Entity<ChatTestView>, &mut VisualTestContext) {
        cx.update(crate::init);
        let (root, cx) = cx.add_window_view(|window, cx| {
            let chat = cx.new(|cx| ChatTestView::new(second_selectable, cx));
            Root::new(chat, window, cx)
        });
        let chat = root.read_with(cx, |root, _| {
            root.view().clone().downcast::<ChatTestView>().unwrap()
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        (chat, cx)
    }

    fn drag(
        cx: &mut VisualTestContext,
        from: gpui::Point<gpui::Pixels>,
        to: gpui::Point<gpui::Pixels>,
    ) {
        drag_through(cx, &[from, to]);
    }

    fn drag_through(cx: &mut VisualTestContext, points: &[gpui::Point<gpui::Pixels>]) {
        assert!(points.len() >= 2);
        let from = points[0];
        let to = *points.last().unwrap();

        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        for point in &points[1..] {
            cx.simulate_mouse_move(*point, Some(MouseButton::Left), Modifiers::default());
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
        }

        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    fn window_selected_text(cx: &mut VisualTestContext) -> String {
        use crate::WindowExt as _;
        cx.update(|window, cx| window.selected_text(cx))
    }

    #[gpui::test]
    fn cross_view_drag_merges_text_top_to_bottom(cx: &mut TestAppContext) {
        let (_, cx) = setup(true, cx);

        drag(cx, point(px(0.), px(15.)), point(px(300.), px(70.)));

        let text = window_selected_text(cx);
        let first = text.find("Hello world").expect("first view text missing");
        let second = text
            .find("Second message")
            .expect("second view text missing");
        assert!(first < second, "wrong order: {text:?}");
        assert!(text.contains('\n'), "expected newline separator: {text:?}");
    }

    #[gpui::test]
    fn drag_from_blank_space_selects_views_below(cx: &mut TestAppContext) {
        let (_, cx) = setup(true, cx);

        drag_through(
            cx,
            &[
                point(px(5.), px(2.)),
                point(px(20.), px(70.)),
                point(px(300.), px(70.)),
            ],
        );

        let text = window_selected_text(cx);
        assert!(text.contains("Hello world"), "got: {text:?}");
        assert!(text.contains("Second message"), "got: {text:?}");
    }

    #[gpui::test]
    fn drag_entirely_in_blank_gap_selects_nothing(cx: &mut TestAppContext) {
        let (chat, cx) = setup(true, cx);

        chat.update(cx, |chat, cx| {
            chat.mid_gap = px(60.);
            cx.notify();
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        drag(cx, point(px(5.), px(70.)), point(px(300.), px(90.)));

        let text = window_selected_text(cx);
        assert_eq!(text, "", "blank-only drag selected text: {text:?}");
    }

    #[gpui::test]
    fn drag_entirely_in_right_gutter_selects_nothing(cx: &mut TestAppContext) {
        let (_, cx) = setup(true, cx);

        drag(cx, point(px(300.), px(2.)), point(px(300.), px(70.)));

        let text = window_selected_text(cx);
        assert_eq!(text, "", "right-gutter drag selected text: {text:?}");
    }

    #[gpui::test]
    fn selection_follows_content_when_layout_shifts(cx: &mut TestAppContext) {
        let (chat, cx) = setup(true, cx);

        chat.update(cx, |chat, cx| {
            chat.mid_gap = px(60.);
            cx.notify();
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        drag_through(
            cx,
            &[
                point(px(0.), px(80.)),
                point(px(20.), px(120.)),
                point(px(300.), px(120.)),
            ],
        );
        let before = window_selected_text(cx);
        assert!(
            before.contains("Second message") && !before.contains("Hello world"),
            "expected only the second view selected, got: {before:?}"
        );

        chat.update(cx, |chat, cx| {
            chat.top_offset = px(90.);
            cx.notify();
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let after = window_selected_text(cx);
        assert_eq!(before, after, "selection drifted after layout shift");
    }

    #[gpui::test]
    fn suppressed_mouse_down_does_not_start_selection(cx: &mut TestAppContext) {
        let (_, cx) = setup(true, cx);

        drag(cx, point(px(20.), px(100.)), point(px(20.), px(15.)));

        let text = window_selected_text(cx);
        assert!(text.is_empty(), "expected no selection, got: {text:?}");
    }

    #[gpui::test]
    fn non_selectable_view_is_excluded(cx: &mut TestAppContext) {
        let (_, cx) = setup(false, cx);

        drag_through(
            cx,
            &[
                point(px(5.), px(2.)),
                point(px(20.), px(15.)),
                point(px(300.), px(15.)),
            ],
        );

        let text = window_selected_text(cx);
        assert!(text.contains("Hello world"), "got: {text:?}");
        assert!(!text.contains("Second message"), "got: {text:?}");
    }

    #[gpui::test]
    fn drag_within_single_view_excludes_others(cx: &mut TestAppContext) {
        let (_, cx) = setup(true, cx);

        drag(cx, point(px(5.), px(15.)), point(px(60.), px(15.)));

        let text = window_selected_text(cx);
        assert!(!text.contains("Second message"), "got: {text:?}");
        assert!(!text.trim().is_empty(), "expected some selection");
    }

    #[gpui::test]
    fn mouse_down_clears_previous_selection(cx: &mut TestAppContext) {
        let (_, cx) = setup(true, cx);

        drag(cx, point(px(5.), px(15.)), point(px(300.), px(70.)));
        assert!(!window_selected_text(cx).is_empty());

        cx.simulate_click(point(px(300.), px(100.)), Modifiers::default());
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        assert_eq!(window_selected_text(cx), "");
    }

    #[gpui::test]
    fn double_click_selects_word_under_root(cx: &mut TestAppContext) {
        let (_, cx) = setup(true, cx);

        let position = point(px(10.), px(15.));
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let text = window_selected_text(cx);
        assert_eq!(text.trim(), "Hello", "expected word selection: {text:?}");
        assert!(!text.contains("Second message"), "got: {text:?}");
    }

    #[gpui::test]
    fn drag_back_into_anchor_view_clears_other_views(cx: &mut TestAppContext) {
        let (chat, cx) = setup(true, cx);
        let second = chat.read_with(cx, |chat, _| chat.second.clone());

        cx.simulate_mouse_down(
            point(px(0.), px(15.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.simulate_mouse_move(
            point(px(300.), px(70.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let text = second.read_with(cx, |state, _| state.selected_text());
        assert!(
            text.contains("Second message"),
            "precondition: B should be selected, got {text:?}"
        );

        let b_notified = Rc::new(Cell::new(false));
        let _subscription = cx.update({
            let b_notified = b_notified.clone();
            let second = second.clone();
            move |_, cx| cx.observe(&second, move |_, _| b_notified.set(true))
        });
        b_notified.set(false);

        cx.simulate_mouse_move(
            point(px(60.), px(15.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.run_until_parked();

        assert!(
            b_notified.get(),
            "view B was not notified when the drag returned to the anchor view, \
             so its stale highlight would never be repainted away",
        );
    }
}
