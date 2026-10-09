#![cfg_attr(target_family = "wasm", no_main)]

//! Liquid glass playground: a draggable lens, a toolbar that lights up under
//! the pointer, a tab bar whose pill slides like a drop, blobs that melt into
//! each other, and a panel with every knob of the material they share.

#[path = "example_support/fonts.rs"]
mod example_support;

use std::{cell::RefCell, f32::consts::PI, rc::Rc, time::Duration};

use gpui::{
    App, Bounds, Context, GlassMaterial, GlassShape, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, SharedString, SpringConfig, Window, WindowBounds, WindowOptions,
    canvas, div, glass_group, hsla, linear_color_stop, linear_gradient, liquid_glass, point,
    prelude::*, px, rgb, size,
};
use gpui_platform::application;
use web_time::Instant;

const PANEL_WIDTH: f32 = 300.;
const TABS: [&str; 4] = ["Home", "Browse", "Radio", "Library"];
const TOOLS: [&str; 4] = ["◀", "▶", "+", "•••"];

struct Knob {
    name: &'static str,
    range: (f32, f32),
    get: fn(&GlassMaterial) -> f32,
    set: fn(&mut GlassMaterial, f32),
}

const KNOBS: &[Knob] = &[
    Knob {
        name: "blur",
        range: (0., 40.),
        get: |m| m.blur.as_f32(),
        set: |m, v| m.blur = px(v),
    },
    Knob {
        name: "bezel",
        range: (0., 60.),
        get: |m| m.bezel.as_f32(),
        set: |m, v| m.bezel = px(v),
    },
    Knob {
        name: "refraction",
        range: (-60., 120.),
        get: |m| m.refraction.as_f32(),
        set: |m, v| m.refraction = px(v),
    },
    Knob {
        name: "dispersion",
        range: (0., 1.),
        get: |m| m.dispersion,
        set: |m, v| m.dispersion = v,
    },
    Knob {
        name: "tint",
        range: (0., 1.),
        get: |m| m.tint.a,
        set: |m, v| m.tint.a = v,
    },
    Knob {
        name: "tint lightness",
        range: (0., 1.),
        get: |m| m.tint.l,
        set: |m, v| m.tint.l = v,
    },
    Knob {
        name: "saturation",
        range: (0., 3.),
        get: |m| m.saturation,
        set: |m, v| m.saturation = v,
    },
    Knob {
        name: "brightness",
        range: (-0.5, 0.5),
        get: |m| m.brightness,
        set: |m, v| m.brightness = v,
    },
    Knob {
        name: "contrast",
        range: (0., 2.),
        get: |m| m.contrast,
        set: |m, v| m.contrast = v,
    },
    Knob {
        name: "specular",
        range: (0., 2.),
        get: |m| m.specular,
        set: |m, v| m.specular = v,
    },
    Knob {
        name: "glint width",
        range: (0., 6.),
        get: |m| m.glint_width.as_f32(),
        set: |m, v| m.glint_width = px(v),
    },
    Knob {
        name: "light angle",
        range: (-PI, PI),
        get: |m| m.light_angle,
        set: |m, v| m.light_angle = v,
    },
    Knob {
        name: "fresnel",
        range: (0., 1.),
        get: |m| m.fresnel,
        set: |m, v| m.fresnel = v,
    },
    Knob {
        name: "edge shadow",
        range: (0., 1.),
        get: |m| m.edge_shadow,
        set: |m, v| m.edge_shadow = v,
    },
    Knob {
        name: "edge width",
        range: (0., 4.),
        get: |m| m.edge_width.as_f32(),
        set: |m, v| m.edge_width = px(v),
    },
    Knob {
        name: "noise",
        range: (0., 0.2),
        get: |m| m.noise,
        set: |m, v| m.noise = v,
    },
    Knob {
        name: "merge",
        range: (0., 80.),
        get: |m| m.merge.as_f32(),
        set: |m, v| m.merge = px(v),
    },
    Knob {
        name: "opacity",
        range: (0., 1.),
        get: |m| m.opacity,
        set: |m, v| m.opacity = v,
    },
];

const PRESETS: [(&str, fn() -> GlassMaterial); 5] = [
    ("regular", GlassMaterial::regular),
    ("clear", GlassMaterial::clear),
    ("frosted", GlassMaterial::frosted),
    ("bubble", GlassMaterial::bubble),
    ("smoked", GlassMaterial::smoked),
];

struct LiquidGlassDemo {
    material: GlassMaterial,
    preset: usize,
    animate_background: bool,
    started: Instant,
    last_frame: Instant,
    frame_times: Vec<f32>,
    lens: Bounds<Pixels>,
    lens_grab: Option<Point<Pixels>>,
    tab: usize,
    card_shown: bool,
    knob_drag: Option<usize>,
    knob_tracks: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    viewport: Bounds<Pixels>,
}

impl LiquidGlassDemo {
    fn new() -> Self {
        let now = Instant::now();
        let lens = Bounds::new(point(px(180.), px(260.)), size(px(150.), px(150.)));
        Self {
            material: GlassMaterial::regular(),
            preset: 0,
            animate_background: true,
            started: now,
            last_frame: now,
            frame_times: Vec::new(),
            lens,
            lens_grab: None,
            tab: 0,
            card_shown: true,
            knob_drag: None,
            knob_tracks: Rc::new(RefCell::new(vec![Bounds::default(); KNOBS.len()])),
            viewport: Bounds::default(),
        }
    }

    fn tab_bar(&self) -> Bounds<Pixels> {
        let width = px(420.);
        let height = px(64.);
        let center = self.stage().center().x;
        Bounds::new(
            point(
                center - width / 2.,
                self.viewport.size.height - height - px(28.),
            ),
            size(width, height),
        )
    }

    fn tab_slot(&self, index: usize) -> Bounds<Pixels> {
        let bar = self.tab_bar();
        let inset = px(6.);
        let width = (bar.size.width - inset * 2.) / TABS.len() as f32;
        Bounds::new(
            point(
                bar.origin.x + inset + width * index as f32,
                bar.origin.y + inset,
            ),
            size(width, bar.size.height - inset * 2.),
        )
    }

    fn toolbar(&self) -> Bounds<Pixels> {
        let width = px(252.);
        let center = self.stage().center().x;
        Bounds::new(point(center - width / 2., px(28.)), size(width, px(56.)))
    }

    fn tool_slot(&self, index: usize) -> Bounds<Pixels> {
        let bar = self.toolbar();
        let side = px(44.);
        let gap = (bar.size.width - side * TOOLS.len() as f32) / (TOOLS.len() as f32 + 1.);
        Bounds::new(
            point(
                bar.origin.x + gap + (side + gap) * index as f32,
                bar.origin.y + (bar.size.height - side) / 2.,
            ),
            size(side, side),
        )
    }

    /// The area left of the knob panel.
    fn stage(&self) -> Bounds<Pixels> {
        Bounds::new(
            self.viewport.origin,
            size(
                (self.viewport.size.width - px(PANEL_WIDTH + 32.)).max(px(200.)),
                self.viewport.size.height,
            ),
        )
    }

    fn tick(&mut self) -> bool {
        let now = Instant::now();
        let delta = now
            .duration_since(self.last_frame)
            .min(Duration::from_millis(100));
        self.last_frame = now;
        self.frame_times.push(delta.as_secs_f32());
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }

        self.animate_background
    }

    fn fps(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.;
        }
        let total: f32 = self.frame_times.iter().sum();
        self.frame_times.len() as f32 / total.max(1e-3)
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(grab) = self.lens_grab {
            self.lens.origin = event.position - grab;
            cx.notify();
        }
        if let Some(knob) = self.knob_drag {
            self.set_knob(knob, event.position.x);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.lens_grab = None;
        if self.knob_drag.take().is_some() {
            log::info!("{}", material_snippet(&self.material));
        }
        cx.notify();
    }

    fn set_knob(&mut self, index: usize, x: Pixels) {
        let track = self.knob_tracks.borrow()[index];
        if track.size.width <= px(0.) {
            return;
        }
        let fraction = ((x - track.origin.x) / track.size.width).clamp(0., 1.);
        let knob = &KNOBS[index];
        (knob.set)(
            &mut self.material,
            knob.range.0 + (knob.range.1 - knob.range.0) * fraction,
        );
    }

    fn background(&self, time: f32) -> impl IntoElement {
        let stage = self.viewport;
        let mut root = div()
            .absolute()
            .size_full()
            .bg(linear_gradient(
                135.,
                linear_color_stop(rgb(0x1d2b64), 0.),
                linear_color_stop(rgb(0xf8cdda), 1.),
            ))
            .overflow_hidden();

        let blobs = [
            (0xff5f6d, 0.0, 260.),
            (0xffc371, 1.7, 220.),
            (0x24c6dc, 3.1, 300.),
            (0x7f00ff, 4.4, 240.),
            (0x00f260, 5.2, 180.),
        ];
        for (index, (color, phase, radius)) in blobs.into_iter().enumerate() {
            let t = time * 0.25 + phase;
            let x =
                stage.size.width.as_f32() * (0.5 + 0.38 * (t * (0.7 + index as f32 * 0.13)).sin());
            let y =
                stage.size.height.as_f32() * (0.5 + 0.36 * (t * (0.9 - index as f32 * 0.07)).cos());
            root = root.child(
                div()
                    .absolute()
                    .left(px(x - radius / 2.))
                    .top(px(y - radius / 2.))
                    .size(px(radius))
                    .rounded_full()
                    .bg(rgb(color)),
            );
        }

        let stripes = div()
            .absolute()
            .left(px(40.))
            .top(px(120.))
            .flex()
            .gap(px(14.))
            .children((0..18).map(|index| {
                let hue = index as f32 / 18.;
                div()
                    .w(px(10.))
                    .h(px(stage.size.height.as_f32() - 240.))
                    .bg(hsla(hue, 0.9, 0.6, 0.85))
            }));

        let copy = div()
            .absolute()
            .left(px(64.))
            .top(px(140.))
            .w(px(560.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .text_color(gpui::white())
            .child(
                div()
                    .text_size(px(64.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Liquid Glass"),
            )
            .child(div().text_size(px(18.)).child(
                "Glass bends what lies under its rim, frosts it a little, and catches \
                 the light along its edge. Drag the lens around, press the toolbar, \
                 switch tabs, and turn the knobs on the right.",
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .children((0..24).map(|index| {
                        let hue = (index as f32 * 0.071) % 1.;
                        div()
                            .size(px(48.))
                            .rounded(px(12.))
                            .bg(linear_gradient(
                                160.,
                                linear_color_stop(hsla(hue, 0.85, 0.65, 1.), 0.),
                                linear_color_stop(hsla((hue + 0.1) % 1., 0.9, 0.45, 1.), 1.),
                            ))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(gpui::white())
                            .child(SharedString::from(format!(
                                "{}",
                                (b'A' + index as u8) as char
                            )))
                    })),
            );

        root.child(stripes).child(copy)
    }

    fn knob_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tracks = self.knob_tracks.clone();
        let rows = KNOBS.iter().enumerate().map(|(index, knob)| {
            let value = (knob.get)(&self.material);
            let fraction = ((value - knob.range.0) / (knob.range.1 - knob.range.0)).clamp(0., 1.);
            let tracks = tracks.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .text_size(px(12.))
                        .child(knob.name)
                        .child(SharedString::from(format!("{value:.2}"))),
                )
                .child(
                    div()
                        .id(("knob", index))
                        .relative()
                        .h(px(14.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                this.knob_drag = Some(index);
                                this.set_knob(index, event.position.x);
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(4.))
                                .rounded_full()
                                .bg(hsla(0., 0., 1., 0.25)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(gpui::relative(fraction))
                                .ml(px(-7.))
                                .size(px(14.))
                                .rounded_full()
                                .bg(gpui::white())
                                .shadow_sm(),
                        )
                        .child(
                            canvas(
                                move |bounds, _, _| tracks.borrow_mut()[index] = bounds,
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        ),
                )
        });

        let presets =
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .children(PRESETS.iter().enumerate().map(|(index, (name, make))| {
                    let make = *make;
                    let selected = index == self.preset;
                    div()
                        .id(("preset", index))
                        .px(px(10.))
                        .py(px(4.))
                        .rounded_full()
                        .text_size(px(12.))
                        .cursor_pointer()
                        .bg(if selected {
                            hsla(0., 0., 1., 0.35)
                        } else {
                            hsla(0., 0., 1., 0.1)
                        })
                        .child(*name)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.preset = index;
                            this.material = make();
                            cx.notify();
                        }))
                }));

        let toggle = div()
            .id("animate")
            .px(px(10.))
            .py(px(4.))
            .rounded_full()
            .text_size(px(12.))
            .cursor_pointer()
            .bg(hsla(0., 0., 1., 0.1))
            .child(if self.animate_background {
                "pause background"
            } else {
                "animate background"
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.animate_background = !this.animate_background;
                cx.notify();
            }));

        div()
            .id("panel")
            .absolute()
            .top(px(16.))
            .right(px(16.))
            .bottom(px(16.))
            .w(px(PANEL_WIDTH))
            .rounded(px(28.))
            .glass(GlassMaterial::frosted().merge(px(0.)))
            .shadow_lg()
            .p(px(18.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_color(gpui::white())
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Material"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .child(SharedString::from(format!("{:.0} fps", self.fps()))),
                    ),
            )
            .child(presets)
            .child(toggle)
            .children(rows)
            .child(
                div()
                    .mt(px(8.))
                    .p(px(10.))
                    .rounded(px(12.))
                    .bg(hsla(0., 0., 0., 0.25))
                    .font_family("Lilex")
                    .text_size(px(11.))
                    .whitespace_normal()
                    .child(SharedString::from(material_snippet(&self.material))),
            )
    }
}

impl Render for LiquidGlassDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.viewport = Bounds::new(point(px(0.), px(0.)), window.viewport_size());
        if self.tick() {
            window.request_animation_frame();
        }
        let time = self.started.elapsed().as_secs_f32();
        let material = self.material;

        // Toolbar: one capsule, and a glass button per tool that swells and
        // lights up from where it was pressed.
        let toolbar = self.toolbar();
        let mut layer = div().absolute().size_full();
        layer = layer.child(
            div()
                .absolute()
                .left(toolbar.origin.x)
                .top(toolbar.origin.y)
                .w(toolbar.size.width)
                .h(toolbar.size.height)
                .rounded_full()
                .glass(material)
                .shadow(vec![gpui::BoxShadow {
                    color: hsla(0., 0., 0., 0.18),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }]),
        );
        for (index, label) in TOOLS.iter().enumerate() {
            let slot = self.tool_slot(index);
            layer = layer.child(
                liquid_glass(("tool", index), material)
                    .drag_flex(px(10.))
                    .light_follows_pointer(true)
                    .absolute()
                    .left(slot.origin.x)
                    .top(slot.origin.y)
                    .w(slot.size.width)
                    .h(slot.size.height)
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(gpui::white())
                    .text_size(px(18.))
                    .cursor_pointer()
                    .child(*label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match index {
                            0 | 1 => {
                                let step = if index == 0 { PRESETS.len() - 1 } else { 1 };
                                this.preset = (this.preset + step) % PRESETS.len();
                                this.material = (PRESETS[this.preset].1)();
                            }
                            2 => this.card_shown = !this.card_shown,
                            _ => this.animate_background = !this.animate_background,
                        }
                        cx.notify();
                    })),
            );
        }

        // A row of buttons painted as one body: they sit just apart, and a
        // press swells one into its neighbors.
        let bar = self.tab_bar();
        layer = layer.child(
            glass_group("row", material.merge(px(18.)))
                .absolute()
                .left(bar.origin.x + px(40.))
                .top(bar.origin.y - px(86.))
                .flex()
                .gap(px(12.))
                .children(["A", "B", "C", "D", "E"].into_iter().enumerate().map(
                    |(index, label)| {
                        liquid_glass(("row", index), material)
                            .press_scale(1.22)
                            .drag_flex(px(8.))
                            .glass_shadow(soft_shadow(0.16))
                            .size(px(52.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(gpui::white())
                            .text_size(px(16.))
                            .cursor_pointer()
                            .child(label)
                    },
                )),
        );

        // A card that lenses in and out with the "+" button.
        let stage = self.stage();
        layer = layer.child(
            liquid_glass(
                "card",
                GlassMaterial::frosted().light_angle(material.light_angle),
            )
            .shown(self.card_shown)
            .drag_flex(px(14.))
            .light_follows_pointer(true)
            .glass_shadow(soft_shadow(0.22))
            .press_scale(1.03)
            .absolute()
            .left(stage.size.width - px(320.))
            .top(px(110.))
            .w(px(280.))
            .h(px(150.))
            .rounded(px(30.))
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_color(gpui::white())
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Now playing"),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(hsla(0., 0., 1., 0.8))
                    .child("Glass appears by lensing in, not by fading. Press it."),
            ),
        );

        // Tab bar: the bar and the pill riding in it. The pill morphs to the
        // selected tab, stretching as it slides and lifting into a stronger
        // lens until it settles.
        let bar = self.tab_bar();
        let pill = self.tab_slot(self.tab);
        layer = layer
            .child(
                div()
                    .absolute()
                    .left(bar.origin.x)
                    .top(bar.origin.y)
                    .w(bar.size.width)
                    .h(bar.size.height)
                    .rounded_full()
                    .glass(material)
                    .shadow(vec![gpui::BoxShadow {
                        color: hsla(0., 0., 0., 0.2),
                        offset: point(px(0.), px(10.)),
                        blur_radius: px(28.),
                        spread_radius: px(0.),
                        inset: false,
                    }]),
            )
            .child(
                liquid_glass("pill", material.tint(hsla(0., 0., 1., 0.14)).blur(px(0.)))
                    .appear(false)
                    .morph(SpringConfig::new(380., 30., 1.))
                    .lift_material(GlassMaterial::bubble())
                    .lift_scale(1.25)
                    .press_scale(1.)
                    .absolute()
                    .left(pill.origin.x)
                    .top(pill.origin.y)
                    .w(pill.size.width)
                    .h(pill.size.height)
                    .rounded_full(),
            );
        for (index, label) in TABS.iter().enumerate() {
            let slot = self.tab_slot(index);
            let selected = index == self.tab;
            layer = layer.child(
                div()
                    .id(("tab", index))
                    .absolute()
                    .left(slot.origin.x)
                    .top(slot.origin.y)
                    .w(slot.size.width)
                    .h(slot.size.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .text_color(if selected {
                        gpui::white()
                    } else {
                        hsla(0., 0., 1., 0.7)
                    })
                    .child(*label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.tab = index;
                            cx.notify();
                        }),
                    ),
            );
        }

        // Blobs that orbit close enough to melt into one body.
        let center = stage.center();
        let blobs = (0..3)
            .map(|index| {
                let t = time * 0.8 + index as f32 * 2.1;
                let radius = px(46. + 10. * index as f32);
                let orbit = 70. + 40. * (time * 0.5 + index as f32).sin();
                let position = center
                    + point(px(orbit * t.cos()), px(orbit * 0.55 * (t * 1.3).sin()))
                    + point(px(stage.size.width.as_f32() * 0.18), px(40.));
                GlassShape::capsule(centered(position, size(radius * 2., radius * 2.), 1.))
            })
            .collect::<Vec<_>>();
        let blob_material = material;
        layer = layer.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| window.paint_glass_shapes(&blobs, &blob_material),
            )
            .absolute()
            .size_full(),
        );

        // The lens, dragged by its element; the glass follows on a spring and
        // stretches with its own speed.
        let lens = self.lens;
        layer = layer.child(
            liquid_glass("lens", material)
                .appear(false)
                .morph(SpringConfig::new(260., 22., 1.))
                .press_scale(1.06)
                .glass_shadow(soft_shadow(0.22))
                .absolute()
                .left(lens.origin.x)
                .top(lens.origin.y)
                .w(lens.size.width)
                .h(lens.size.height)
                .rounded_full()
                .cursor_grab()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.lens_grab = Some(event.position - this.lens.origin);
                        cx.notify();
                    }),
                ),
        );

        div()
            .id("root")
            .size_full()
            .relative()
            .font_family("IBM Plex Sans")
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(self.background(if self.animate_background { time } else { 0. }))
            .child(layer)
            .child(self.knob_panel(cx))
    }
}

/// The material as the builder calls that make it, to paste into code.
fn material_snippet(material: &GlassMaterial) -> String {
    let tint = material.tint;
    let mut lines = vec!["GlassMaterial::regular()".to_string()];
    for knob in KNOBS {
        let value = (knob.get)(material);
        match knob.name {
            "tint" | "tint lightness" => {}
            "blur" | "bezel" | "refraction" | "glint width" | "edge width" | "merge" => {
                lines.push(format!(".{}(px({value:.2}))", knob.name.replace(' ', "_")))
            }
            name => lines.push(format!(".{}({value:.3})", name.replace(' ', "_"))),
        }
    }
    lines.push(format!(
        ".tint(hsla({:.3}, {:.3}, {:.3}, {:.3}))",
        tint.h, tint.s, tint.l, tint.a
    ));
    lines.join("\n    ")
}

fn soft_shadow(alpha: f32) -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: hsla(0., 0., 0., alpha),
        offset: point(px(0.), px(10.)),
        blur_radius: px(26.),
        spread_radius: px(-2.),
        inset: false,
    }]
}

fn centered(center: Point<Pixels>, extent: gpui::Size<Pixels>, scale: f32) -> Bounds<Pixels> {
    let extent = size(extent.width * scale, extent.height * scale);
    Bounds::new(
        center - point(extent.width / 2., extent.height / 2.),
        extent,
    )
}

fn run_example() {
    application().run(|cx: &mut App| {
        if !example_support::load_fonts(cx) {
            return;
        }
        let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| LiquidGlassDemo::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_example();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_example();
}
