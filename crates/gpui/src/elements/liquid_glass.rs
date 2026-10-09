use scheduler::Instant;
use std::{cell::RefCell, rc::Rc, time::Duration};

use crate::{
    AbsoluteLength, AnyElement, App, Bounds, Corners, DispatchPhase, Div, Element, ElementId,
    GlassMaterial, GlassShape, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
    InteractiveElement, Interactivity, Interpolate, IntoElement, LayoutId, LiquidRect, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, SpringConfig,
    SpringState, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, div, point,
    size,
};

/// Glass that answers the pointer: it swells and lights up from where it is
/// pressed, brightens under the pointer, and lenses in when it first
/// appears. Every change rides a spring, so interrupted motion stays smooth.
///
/// Style it and give it children like a `div`; the glass takes the
/// element's corner radii.
pub fn liquid_glass(id: impl Into<ElementId>, material: GlassMaterial) -> LiquidGlass {
    let id = id.into();
    LiquidGlass {
        id: id.clone(),
        content: Some(div().id(id)),
        material,
        pressed: None,
        hovered: None,
        press_scale: 1.12,
        hover_scale: 1.0,
        spring: SpringConfig::new(320., 20., 1.),
        appear: true,
        shown: true,
        morph: None,
        lift: None,
        lift_scale: 1.0,
        corner_radii: Corners::default(),
    }
}

/// See [`liquid_glass`].
pub struct LiquidGlass {
    id: ElementId,
    content: Option<Stateful<Div>>,
    material: GlassMaterial,
    pressed: Option<GlassMaterial>,
    hovered: Option<GlassMaterial>,
    press_scale: f32,
    hover_scale: f32,
    spring: SpringConfig,
    appear: bool,
    shown: bool,
    morph: Option<SpringConfig>,
    lift: Option<GlassMaterial>,
    lift_scale: f32,
    corner_radii: Corners<AbsoluteLength>,
}

impl LiquidGlass {
    /// The material while pressed. By default the resting material, lensing
    /// a little harder and glowing from where the press landed.
    pub fn pressed_material(mut self, material: GlassMaterial) -> Self {
        self.pressed = Some(material);
        self
    }

    /// The material under the pointer. By default the resting material with
    /// a brighter glint.
    pub fn hovered_material(mut self, material: GlassMaterial) -> Self {
        self.hovered = Some(material);
        self
    }

    /// How much a press swells the glass; 1 keeps its size.
    pub fn press_scale(mut self, scale: f32) -> Self {
        self.press_scale = scale;
        self
    }

    /// How much hovering swells the glass; 1 keeps its size.
    pub fn hover_scale(mut self, scale: f32) -> Self {
        self.hover_scale = scale;
        self
    }

    /// The spring every change follows.
    pub fn spring(mut self, spring: SpringConfig) -> Self {
        self.spring = spring;
        self
    }

    /// Whether the glass lenses in from nothing when first painted, rather
    /// than starting fully there.
    pub fn appear(mut self, appear: bool) -> Self {
        self.appear = appear;
        self
    }

    /// Whether the glass is there. Hiding it lenses it away and fades its
    /// content; showing it again lenses it back in.
    pub fn shown(mut self, shown: bool) -> Self {
        self.shown = shown;
        self
    }

    /// Makes the glass follow its element on springs when layout moves or
    /// resizes it, stretching along its motion like a drop, instead of
    /// jumping there.
    pub fn morph(mut self, spring: SpringConfig) -> Self {
        self.morph = Some(spring);
        self
    }

    /// What morphing glass turns into while it travels, as a tab bar's pill
    /// lifts into a stronger lens when it slides to another tab.
    pub fn lift_material(mut self, material: GlassMaterial) -> Self {
        self.lift = Some(material);
        self
    }

    /// How much morphing glass swells while it travels; 1 keeps its size.
    pub fn lift_scale(mut self, scale: f32) -> Self {
        self.lift_scale = scale;
        self
    }

    fn content(&mut self) -> &mut Stateful<Div> {
        self.content
            .as_mut()
            .expect("liquid glass content is taken only when laid out")
    }
}

impl Styled for LiquidGlass {
    fn style(&mut self) -> &mut StyleRefinement {
        self.content().style()
    }
}

impl ParentElement for LiquidGlass {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.content().extend(elements);
    }
}

impl InteractiveElement for LiquidGlass {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.content().interactivity()
    }
}

impl StatefulInteractiveElement for LiquidGlass {}

impl IntoElement for LiquidGlass {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// A spring toward 0 or 1.
#[derive(Clone, Copy, Default)]
struct Toggle {
    spring: SpringState,
    on: bool,
}

impl Toggle {
    fn step(&mut self, config: SpringConfig, delta: f32, snap: bool) -> bool {
        let target = if self.on { 1. } else { 0. };
        if !snap {
            self.spring = config.step(self.spring, target, delta);
        }
        if snap || config.is_settled(self.spring, target, 0.001) {
            self.spring = SpringState {
                position: target,
                velocity: 0.,
            };
            return false;
        }
        true
    }

    fn phase(&self) -> f32 {
        self.spring.position
    }
}

struct LiquidGlassState {
    press: Toggle,
    hover: Toggle,
    presence: Toggle,
    /// Where the press landed, as a fraction of the glass's bounds.
    touch: Point<f32>,
    /// Where morphing glass is on its way to the element's bounds.
    body: Option<LiquidRect>,
    lift: Toggle,
    updated_at: Instant,
}

/// What a laid out [`LiquidGlass`] paints this frame.
pub struct LiquidGlassFrame {
    state: Rc<RefCell<LiquidGlassState>>,
    hitbox: Hitbox,
    shape: GlassShape,
    material: GlassMaterial,
    presence: f32,
    /// Whether an enclosing group paints this glass as part of its body.
    grouped: bool,
}

impl Element for LiquidGlass {
    type RequestLayoutState = AnyElement;
    type PrepaintState = LiquidGlassFrame;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut content = self.content.take().expect("liquid glass is laid out once");
        let radii = &content.style().corner_radii;
        self.corner_radii = Corners {
            top_left: radii.top_left.unwrap_or_default(),
            top_right: radii.top_right.unwrap_or_default(),
            bottom_right: radii.bottom_right.unwrap_or_default(),
            bottom_left: radii.bottom_left.unwrap_or_default(),
        };
        let mut content = content.into_any_element();
        (content.request_layout(window, cx), content)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        content: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        content.prepaint(window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        let state = window.with_element_state(
            id.expect("liquid glass has an id"),
            |state: Option<Rc<RefCell<LiquidGlassState>>>, _| {
                let state = state.unwrap_or_else(|| {
                    Rc::new(RefCell::new(LiquidGlassState {
                        press: Toggle::default(),
                        hover: Toggle::default(),
                        presence: Toggle {
                            spring: SpringState {
                                position: if self.appear { 0. } else { 1. },
                                velocity: 0.,
                            },
                            on: true,
                        },
                        touch: point(0.5, 0.5),
                        body: None,
                        lift: Toggle::default(),
                        updated_at: cx.background_executor().now(),
                    }))
                });
                (state.clone(), state)
            },
        );

        let (press, hover, presence, touch, lift, body) = {
            let mut state = state.borrow_mut();
            let now = cx.background_executor().now();
            let delta = now.duration_since(state.updated_at).as_secs_f32().min(0.1);
            state.updated_at = now;
            state.hover.on = hitbox.is_hovered(window);
            state.presence.on = self.shown;
            let snap = cx.reduce_motion();
            let config = self.spring;
            let mut moving = state.press.step(config, delta, snap);
            moving |= state.hover.step(config, delta, snap);
            moving |= state.presence.step(config, delta, snap);
            let body = match self.morph {
                Some(spring) => {
                    let body = state
                        .body
                        .get_or_insert_with(|| LiquidRect::new(bounds, spring));
                    body.config = spring;
                    body.set_target(bounds);
                    if snap {
                        body.snap(bounds);
                    } else {
                        moving |= body.step(Duration::from_secs_f32(delta));
                    }
                    let velocity = body.velocity();
                    let speed = velocity.x.as_f32().hypot(velocity.y.as_f32());
                    let stretched = body.stretched(0.12, 0.35);
                    state.lift.on = speed > 120.;
                    moving |= state.lift.step(config, delta, snap);
                    stretched
                }
                None => {
                    state.body = None;
                    bounds
                }
            };
            if moving {
                window.request_animation_frame();
            }
            (
                state.press.phase(),
                state.hover.phase(),
                state.presence.phase(),
                state.touch,
                state.lift.phase(),
                body,
            )
        };

        let rest = Interpolate::interpolate(self.material.vanished(), self.material, presence);
        let hovered = self.hovered.unwrap_or_else(|| {
            rest.specular(rest.specular * 1.4)
                .brightness(rest.brightness + 0.03)
        });
        let longest = bounds.size.width.max(bounds.size.height);
        let pressed = self.pressed.unwrap_or_else(|| {
            rest.glow(0.22)
                .glow_radius(longest * 0.9)
                .refraction(rest.refraction * 1.2)
        });
        let pressed = pressed.glow_center(touch);
        let mut material = Interpolate::interpolate(
            Interpolate::interpolate(rest, hovered, hover),
            pressed,
            press,
        );
        if let Some(lifted) = self.lift {
            material = Interpolate::interpolate(material, lifted, lift);
        }

        let scale = (1. + (self.hover_scale - 1.) * hover + (self.press_scale - 1.) * press)
            * (1. + (self.lift_scale - 1.) * lift)
            * (0.92 + 0.08 * presence);
        let grown = size(body.size.width * scale, body.size.height * scale);
        let radii = self
            .corner_radii
            .to_pixels(window.rem_size())
            .clamp_radii_for_quad_size(bounds.size);
        let shape = GlassShape {
            bounds: Bounds::new(
                body.center() - point(grown.width / 2., grown.height / 2.),
                grown,
            ),
            corner_radii: Corners {
                top_left: radii.top_left * scale,
                top_right: radii.top_right * scale,
                bottom_right: radii.bottom_right * scale,
                bottom_left: radii.bottom_left * scale,
            },
        };

        let grouped = presence > 0.001
            && window.glass_groups.last().is_some_and(|group| {
                group.borrow_mut().push(GroupedGlass {
                    shape,
                    press,
                    touch,
                });
                true
            });

        LiquidGlassFrame {
            state,
            hitbox,
            shape,
            material,
            presence,
            grouped,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        content: &mut Self::RequestLayoutState,
        frame: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let presence = frame.presence;
        if presence > 0.001 && !frame.grouped {
            window.paint_glass(
                frame.shape.bounds,
                frame.shape.corner_radii,
                &frame.material,
            );
        }
        // Gone glass paints nothing, so its content takes no input either.
        if self.shown || presence > 0.001 {
            window.with_element_opacity(Some(presence.clamp(0., 1.)), |window| {
                content.paint(window, cx)
            });
        }

        let down_state = frame.state.clone();
        let down_hitbox = frame.hitbox.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && down_hitbox.is_hovered(window)
                && down_state.borrow().presence.on
            {
                let mut state = down_state.borrow_mut();
                state.press.on = true;
                state.touch = point(
                    ((event.position.x - bounds.origin.x) / bounds.size.width).clamp(0., 1.),
                    ((event.position.y - bounds.origin.y) / bounds.size.height).clamp(0., 1.),
                );
                window.refresh();
            }
        });
        let up_state = frame.state.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let mut state = up_state.borrow_mut();
                if state.press.on {
                    state.press.on = false;
                    window.refresh();
                }
            }
        });
        let move_state = frame.state.clone();
        let move_hitbox = frame.hitbox.clone();
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble
                && move_hitbox.is_hovered(window) != move_state.borrow().hover.on
            {
                window.refresh();
            }
        });
    }
}

/// The glass shapes the [`liquid_glass`] children of a group registered while
/// it prepainted them.
pub type GlassGroupShapes = Rc<RefCell<Vec<GroupedGlass>>>;

/// One child's shape in a [`GlassGroup`], and how it is being pressed.
#[derive(Clone, Copy, Debug)]
pub struct GroupedGlass {
    shape: GlassShape,
    press: f32,
    touch: Point<f32>,
}

/// A container whose [`liquid_glass`] children, at any depth, are painted as
/// one glass body: wherever two come closer than the material's merge
/// radius they melt into each other, so a button swelling under a press
/// bleeds into its neighbors. The group's material replaces the children's
/// own; their press glow carries over.
///
/// Style it and give it children like a `div`. One body holds up to
/// [`GLASS_MAX_SHAPES`](crate::GLASS_MAX_SHAPES) shapes; more are drawn as
/// further bodies that do not merge with the first.
pub fn glass_group(id: impl Into<ElementId>, material: GlassMaterial) -> GlassGroup {
    let id = id.into();
    GlassGroup {
        id: id.clone(),
        content: Some(div().id(id)),
        material,
    }
}

/// See [`glass_group`].
pub struct GlassGroup {
    id: ElementId,
    content: Option<Stateful<Div>>,
    material: GlassMaterial,
}

impl GlassGroup {
    fn content(&mut self) -> &mut Stateful<Div> {
        self.content
            .as_mut()
            .expect("glass group content is taken only when laid out")
    }
}

impl Styled for GlassGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        self.content().style()
    }
}

impl ParentElement for GlassGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.content().extend(elements);
    }
}

impl InteractiveElement for GlassGroup {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.content().interactivity()
    }
}

impl StatefulInteractiveElement for GlassGroup {}

impl IntoElement for GlassGroup {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for GlassGroup {
    type RequestLayoutState = AnyElement;
    type PrepaintState = Vec<GroupedGlass>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut content = self
            .content
            .take()
            .expect("glass group is laid out once")
            .into_any_element();
        (content.request_layout(window, cx), content)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        content: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        window.glass_groups.push(GlassGroupShapes::default());
        content.prepaint(window, cx);
        let shapes = window
            .glass_groups
            .pop()
            .expect("the group pushed its own shapes");
        shapes.take()
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        content: &mut Self::RequestLayoutState,
        grouped: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for body in grouped.chunks(crate::GLASS_MAX_SHAPES) {
            let shapes: Vec<GlassShape> = body.iter().map(|glass| glass.shape).collect();
            let mut material = self.material;
            if let Some(pressed) = body
                .iter()
                .max_by(|a, b| a.press.total_cmp(&b.press))
                .filter(|glass| glass.press > 0.001)
            {
                let union = shapes
                    .iter()
                    .map(|shape| shape.bounds)
                    .reduce(|a, b| a.union(&b))
                    .unwrap_or_default();
                let bounds = pressed.shape.bounds;
                let at = bounds.origin
                    + point(
                        bounds.size.width * pressed.touch.x,
                        bounds.size.height * pressed.touch.y,
                    );
                let longest = bounds.size.width.max(bounds.size.height);
                material = Interpolate::interpolate(
                    material,
                    material
                        .glow(0.22)
                        .glow_radius(longest * 0.9)
                        .glow_center(point(
                            (at.x - union.origin.x) / union.size.width.max(Pixels(1.)),
                            (at.y - union.origin.y) / union.size.height.max(Pixels(1.)),
                        )),
                    pressed.press,
                );
            }
            window.paint_glass_shapes(&shapes, &material);
        }
        content.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{Context, Modifiers, Render, TestAppContext, VisualTestContext, px};

    struct Button;

    impl Render for Button {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                liquid_glass("button", GlassMaterial::regular())
                    .appear(false)
                    .absolute()
                    .left(px(20.))
                    .top(px(20.))
                    .size(px(40.))
                    .rounded_full(),
            )
        }
    }

    fn glass_width(cx: &mut VisualTestContext) -> f32 {
        cx.update(|window, _| {
            window
                .painted_glasses()
                .iter()
                .map(|glass| glass.shape_bounds().size.width.0)
                .fold(0., f32::max)
        })
    }

    fn next_frame(cx: &mut VisualTestContext, after: Duration) {
        cx.executor().advance_clock(after);
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
    }

    #[crate::test]
    fn pressing_swells_the_glass_and_releasing_settles_it(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Button);
        cx.run_until_parked();
        let rest = glass_width(cx);
        let scale = cx.update(|window, _| window.scale_factor());
        assert_eq!(rest, 40. * scale);

        let center = point(px(40.), px(40.));
        cx.simulate_mouse_move(center, None, Modifiers::none());
        cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
        for _ in 0..30 {
            next_frame(cx, Duration::from_millis(16));
        }
        let pressed = glass_width(cx);
        assert!(pressed > rest * 1.08, "pressed glass is {pressed} wide");

        cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
        for _ in 0..120 {
            next_frame(cx, Duration::from_millis(16));
        }
        assert_eq!(glass_width(cx), rest);
    }

    struct Row {
        grouped: bool,
    }

    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let buttons = (0..3usize).map(|index| {
                liquid_glass(("button", index), GlassMaterial::regular())
                    .appear(false)
                    .size(px(40.))
                    .rounded_full()
            });
            let row = div().flex().gap(px(4.));
            if self.grouped {
                glass_group("row", GlassMaterial::regular())
                    .child(row.children(buttons))
                    .into_any_element()
            } else {
                row.children(buttons).into_any_element()
            }
        }
    }

    #[crate::test]
    fn groups_paint_their_glass_children_as_one_body(cx: &mut TestAppContext) {
        let bodies = |grouped: bool, cx: &mut TestAppContext| {
            let (_, cx) = cx.add_window_view(move |_, _| Row { grouped });
            cx.run_until_parked();
            cx.update(|window, _| {
                window
                    .painted_glasses()
                    .iter()
                    .map(|glass| glass.shape_count)
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(bodies(false, cx), vec![1, 1, 1]);
        assert_eq!(bodies(true, cx), vec![3]);
    }
}
