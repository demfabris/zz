# Welcome to GPUI!

GPUI is a hybrid immediate and retained mode, GPU accelerated, UI framework
for Rust, designed to support a wide variety of applications.

## Getting Started

GPUI is still in active development as we work on the Zed code editor, and is still pre-1.0. There will often be breaking changes between versions. You'll also need to use the latest version of stable Rust. Add `gpui`, and optionally `zpui_platform`, to your `Cargo.toml`:

```toml
gpui = { version = "*" }
zpui_platform = { version = "*", features = ["font-kit", "wayland", "x11"] }
```

Everything in a standalone GPUI app starts with an `Application`. You can create one with `zpui_platform::application()`, which picks the windowing and text backends for the host OS, and kick off your application by passing a callback to `Application::run()`. Inside this callback, you can create a new window with `App::open_window()` and register your first root view.

```rust,no_run
use zpui::*;

fn main() {
    zpui_platform::application().run(|cx: &mut App| {
        // ..
    });
}
```

### `zpui_platform`

The features on `zpui_platform` are platform-specific, so the list above is a safe cross-platform default. If you build for a single platform, you can trim it:

- **macOS** — Rendering uses Metal and is always available, but glyph rasterization needs `font-kit`. Without it, GPUI falls back to a placeholder text system that lays text out but renders no glyphs.

    ```toml
    zpui_platform = { version = "*", features = ["font-kit"] }
    ```

- **Linux / FreeBSD** — enable at least one windowing backend for desktop windows: `wayland`, `x11`, or both. These features also compile the renderer and text system, so no separate text feature is needed.

    ```toml
    zpui_platform = { version = "*", features = ["wayland", "x11"] }
    ```

- **Windows** — no features are required. Windowing uses Win32 and text uses DirectWrite. `font-kit` has no effect here.

### Additional Topics

- [Ownership and data flow](_ownership_and_data_flow)
- [Accessibility](_accessibility)

### Dependencies

GPUI has various system dependencies that it needs in order to work.

#### macOS

On macOS, GPUI uses Metal for rendering. In order to use Metal, you need to do the following:

- Install [Xcode](https://apps.apple.com/us/app/xcode/id497799835?mt=12) from the macOS App Store, or from the [Apple Developer](https://developer.apple.com/download/all/) website. Note this requires a developer account.

> Ensure you launch Xcode after installing, and install the macOS components, which is the default option.

- Install [Xcode command line tools](https://developer.apple.com/xcode/resources/)

  ```sh
  xcode-select --install
  ```

- Ensure that the Xcode command line tools are using your newly installed copy of Xcode:

  ```sh
  sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
  ```

## The Big Picture

GPUI offers three different [registers](<https://en.wikipedia.org/wiki/Register_(sociolinguistics)>) depending on your needs:

- State management and communication with `Entity`'s. Whenever you need to store application state that communicates between different parts of your application, you'll want to use GPUI's entities. Entities are owned by GPUI and are only accessible through an owned smart pointer similar to an `Rc`. See the `app::context` module for more information.

- High level, declarative UI with views. All UI in GPUI starts with a view. A view is simply an `Entity` that can be rendered, by implementing the `Render` trait. At the start of each frame, GPUI will call this render method on the root view of a given window. Views build a tree of `elements`, lay them out and style them with a tailwind-style API, and then give them to GPUI to turn into pixels. See the `div` element for an all purpose swiss-army knife of rendering.

- Low level, imperative UI with Elements. Elements are the building blocks of UI in GPUI, and they provide a nice wrapper around an imperative API that provides as much flexibility and control as you need. Elements have total control over how they and their child elements are rendered and can be used for making efficient views into large lists, implement custom layouting for a code editor, and anything else you can think of. See the `element` module for more information.

Each of these registers has one or more corresponding contexts that can be accessed from all GPUI services. This context is your main interface to GPUI, and is used extensively throughout the framework.

## Other Resources

In addition to the systems above, GPUI provides a range of smaller services that are useful for building complex applications:

- Actions are user-defined structs that are used for converting keystrokes into logical operations in your UI. Use this for implementing keyboard shortcuts, such as cmd-q. See the `action` module for more information.

- Platform services, such as `quit the app` or `open a URL` are available as methods on the `app::App`.

- An async executor that is integrated with the platform's event loop. See the `executor` module for more information.,

- The `[zpui::test]` macro provides a convenient way to write tests for your GPUI applications. Tests also have their own kind of context, a `TestAppContext` which provides ways of simulating common platform input. See `app::test_context` and `test` modules for more details.

Currently, the best way to learn about these APIs is to read the Zed source code or drop a question in the [Zed Discord](https://zed.dev/community-links). We're working on improving the documentation, creating more examples, and will be publishing more guides to GPUI on our [blog](https://zed.dev/blog).

## Liquid glass

Glass shows what was painted under it through a lens: the rim pulls the backdrop inward like the
thick edge of a drop, the face is frosted and tinted, a thin glint runs along the edge facing the
light, and shapes painted as one body melt into each other.

```rust,ignore
// Any element, in its own shape.
div().size(px(64.)).rounded_full().glass(GlassMaterial::regular())

// Glass that swells and glows where it is pressed, stretches like gel when a
// held press is dragged, and lenses in and out.
liquid_glass("play", GlassMaterial::regular())
    .drag_flex(px(10.))
    .light_follows_pointer(true)
    .glass_shadow(shadows)
    .size(px(44.))
    .rounded_full()
    .child("▶")

// A tab bar pill that slides to the selected tab like a drop, lifting as it goes.
liquid_glass("pill", GlassMaterial::regular())
    .morph(SpringConfig::new(380., 30., 1.))
    .lift_material(GlassMaterial::bubble())
    .lift_scale(1.25)
    .absolute()
    .left(selected.origin.x)

// Buttons that melt into their neighbors when a press swells them.
glass_group("toolbar", GlassMaterial::regular().merge(px(18.)))
    .flex()
    .gap(px(12.))
    .children(buttons)

// Materials interpolate, so any spring or animation can carry one into another.
div().with_spring("frost", SpringAnimation::new(spring).to(focused), |el, phase| {
    el.glass(phase.interpolate(GlassMaterial::clear(), GlassMaterial::frosted()))
})

// Or paint it yourself, one body made of up to eight shapes.
window.paint_glass_shapes(&shapes, &material);
```

`GlassMaterial` holds every knob: blur (frost), bezel and refraction (the lens), dispersion,
tint, saturation, brightness, contrast, specular and glint width, light angle, fresnel, edge shadow
and width, grain, touch glow, merge radius, and opacity. The presets `regular`, `clear`,
`frosted`, `bubble`, `smoked`, and `tinted(color)` are starting points, and materials
interpolate, so springs and animations can carry one into another; `vanished()` is the identity
glass appears from.
`LiquidRect` moves a shape on springs and stretches it along its velocity.

Metal and wgpu draw it from the same WGSL (`crates/zpui/src/glass.wgsl`; Metal translates it with
naga). Each batch of glass costs one render pass break, a copy of only the glass regions, a dual
Kawase blur over just those regions down to half resolution, and one analytic draw per body. At
5344x2964 on Apple silicon the first glass in a frame costs about 0.03 ms and a frosted sidebar
0.16 ms (`cargo test -p zpui-apple --release bench_glass -- --ignored --nocapture`). Renderers
that cannot read back their frame (DirectX, WebGL) paint a translucent fill instead.

Try it with `cargo run -p zpui --example liquid_glass`, or in a browser from the web gallery
(`/liquid-glass`).

