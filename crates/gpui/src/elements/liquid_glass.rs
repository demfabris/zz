use scheduler::Instant;
use std::{cell::RefCell, rc::Rc};

use crate::{
    AbsoluteLength, AnyElement, App, Bounds, Corners, DispatchPhase, Div, Element, ElementId,
    GlassMaterial, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement,
    Interactivity, Interpolate, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, SpringConfig, SpringState, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div, point, size,
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
    updated_at: Instant,
}

impl Element for LiquidGlass {
    type RequestLayoutState = AnyElement;
    type PrepaintState = Hitbox;

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
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        content: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        content.prepaint(window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        content: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
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
                        updated_at: Instant::now(),
                    }))
                });
                (state.clone(), state)
            },
        );

        let (press, hover, presence, touch) = {
            let mut state = state.borrow_mut();
            let now = Instant::now();
            let delta = now.duration_since(state.updated_at).as_secs_f32().min(0.1);
            state.updated_at = now;
            state.hover.on = hitbox.is_hovered(window);
            state.presence.on = self.shown;
            let snap = cx.reduce_motion();
            let config = self.spring;
            let mut moving = state.press.step(config, delta, snap);
            moving |= state.hover.step(config, delta, snap);
            moving |= state.presence.step(config, delta, snap);
            if moving {
                window.request_animation_frame();
            }
            (
                state.press.phase(),
                state.hover.phase(),
                state.presence.phase(),
                state.touch,
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
        let material = Interpolate::interpolate(
            Interpolate::interpolate(rest, hovered, hover),
            pressed,
            press,
        );

        let scale = (1. + (self.hover_scale - 1.) * hover + (self.press_scale - 1.) * press)
            * (0.92 + 0.08 * presence);
        let grown = size(bounds.size.width * scale, bounds.size.height * scale);
        let glass_bounds = Bounds::new(
            bounds.center() - point(grown.width / 2., grown.height / 2.),
            grown,
        );
        let rem_size = window.rem_size();
        let radii = self
            .corner_radii
            .to_pixels(rem_size)
            .clamp_radii_for_quad_size(bounds.size);
        let radii = Corners {
            top_left: radii.top_left * scale,
            top_right: radii.top_right * scale,
            bottom_right: radii.bottom_right * scale,
            bottom_left: radii.bottom_left * scale,
        };
        if presence > 0.001 {
            window.paint_glass(glass_bounds, radii, &material);
        }
        // Gone glass paints nothing, so its content takes no input either.
        if self.shown || presence > 0.001 {
            window.with_element_opacity(Some(presence.clamp(0., 1.)), |window| {
                content.paint(window, cx)
            });
        }

        let down_state = state.clone();
        let down_hitbox = hitbox.clone();
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
        let up_state = state.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let mut state = up_state.borrow_mut();
                if state.press.on {
                    state.press.on = false;
                    window.refresh();
                }
            }
        });
        let move_hitbox = hitbox.clone();
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble
                && move_hitbox.is_hovered(window) != state.borrow().hover.on
            {
                window.refresh();
            }
        });
    }
}
