# zz UI preview

A browser workspace for iterating on zz's Rust interface. It renders GPUI through
WASM/WebGPU and imports `crates/zz-ui`, using the same patched GPUI revision as
the desktop app. JavaScript hosts the canvas and preview controls; the app UI is
Rust.

## Run

```bash
just showcase-setup   # first run
just showcase         # build, open http://127.0.0.1:3131, watch for changes
```

On macOS, setup also uses Python 3 to prepare local SF text faces. FontTools is
installed in an isolated environment under `target/ui-showcase-fonts`; the
generated faces are cached there. This runs once, or when the system fonts or
preparation script change, and adds no work to normal Rust edits.

Choose Workspace, Browser, Agent, Settings, or Components. The workspace combines
a sidebar, split terminal/browser/agent panes, and status bar. Settings currently
provides Interface and Panes fixtures; the other settings sections are disabled.
Components opens the existing catalog, including editor, chooser, menu, dialog,
notification, and agent timeline states.

The controls outside the app set its viewport width/height in logical pixels,
window zoom, theme, sidebar, and pane gaps. The viewport is not CSS-scaled. Zoom
uses GPUI's `Window::set_zoom`, just like desktop. Scene and appearance choices
survive rebuilds. Copy link includes those choices and the viewport size.

Use **Point** to mark a location; Copy link includes its coordinates. **Background**
loads a local image as the webpage's fixed, full-size wallpaper, behind the app.
Background opacity controls only that image. The image and opacity are saved
locally in the browser and survive rebuilds; Clear background removes the saved
image. Images are not uploaded or included in copied links.

Interface → Window blur makes the chrome translucent and blurs the wallpaper
behind the app. The preview and desktop share the 93% chrome-opacity rule;
workspace pane content stays opaque, while Settings uses translucent chrome
throughout. The browser uses a 160px CSS backdrop blur to simulate macOS's
background blur. CSS and the native compositor use different filters, so the
blur kernel is an approximation. The blur setting persists and is included in
copied links. A background image is needed to see the effect on a flat page.

## Shared source

The desktop and preview both call these implementations:

| Interface | Source |
| --- | --- |
| App shell and status rail | `crates/zz-ui/src/shell.rs` |
| Sidebar, tree rows, controls | `crates/zz-ui/src/navigation.rs` |
| Tree indentation guides | `crates/zz-ui/src/navigation/tree.rs` |
| Pane frames and splits | `crates/zz-ui/src/pane.rs` |
| Browser toolbar and tabs | `crates/zz-ui/src/browser.rs` |
| Agent header and timeline | `crates/zz-ui/src/agent.rs` |
| Agent composer layout | `crates/zz-ui/src/agent/composer.rs` |
| Settings rows, page layout, theme tiles | `crates/zz-ui/src/settings.rs`, `settings/appearance.rs` |

`src/preview.rs` supplies deterministic workspace data. `src/preview/settings.rs`
supplies settings values and local callbacks. Desktop adapters still own daemon,
CEF, PTY, filesystem, and agent behavior. The preview does not connect to those
services or write the user's zz configuration. Tab selection/closing, tree
navigation, input editing, theme/zoom, pane controls, and widget interactions can
be exercised locally. Fixture choices are not a live copy of an attached session.

Edit shared Rust to change both surfaces. If a needed composition still lives in
`crates/zz`, move its rendering into `zz-ui`, retain the desktop's state/actions in
its adapter, and supply fixture state here. Avoid maintaining a second visual
implementation of a desktop component.

## Dev loop

Changes under `crates/zz-ui/src`, `examples/ui-showcase/src`, their manifests, and
shared/fixture assets rebuild WASM and bindings. Vite reloads the canvas. This is
still a Rust rebuild, not code hot replacement; it avoids compiling the daemon,
CEF integration, and desktop application, and avoids repackaging/relaunching zz.
A changed shared UI rebuild measured about 8 seconds on the development Mac,
including bindings. Cold builds take longer; timings depend on the machine.

Only the browser build uses nightly. `SHOWCASE_TOOLCHAIN` can select another
installed nightly. The standalone workspace has its own lockfile; keep its GPUI
patch revision aligned with the root workspace. The watcher includes shared
icons, so it no longer depends on separately copied catalog icons.

```bash
just showcase-build          # debug WASM + bindings
just showcase-build-release  # optimized WASM + bindings
just showcase-native         # the same fixture in a native GPUI window
just showcase-capture /tmp/zz-preview.png  # save native GPUI pixels, then exit
```

The native window defaults to 1200×760. For example:

```bash
ZZ_PREVIEW_OPTIONS='{"scene":"agent","dark":true,"zoom":1,"gaps":true}' just showcase-native
ZZ_PREVIEW_OPTIONS='{"width":1200,"height":600}' just showcase-capture /tmp/zz-preview.png
```

The native preview is also isolated from the daemon and normal app configuration.
`showcase-capture` enables GPUI's capture support only for that native build and
saves a PNG at the display's pixel density. It captures the GPUI scene, excluding
OS decorations. Compare it with the browser at a matching viewport and zoom;
native captures do not include the desktop behind the window.

## Fidelity boundaries

The browser uses the same layout and painting code for the shared components.
It is a useful visual development surface, but native rendering remains the final
check for pixel-sensitive changes:

- On macOS, the preview prepares static SF faces from `/System/Library/Fonts`
  at optical size 17, matching CoreText's selection for the current small chrome
  text. Regular, medium, semibold, bold, and italic faces use the corresponding
  CoreText weight coordinates. The local Vite server only serves these fixed
  cached files and Menlo. System fonts are never committed or bundled for
  distribution. The axes and preparation live in `scripts/prepare-showcase-fonts.py`,
  using [FontTools' instancer](https://fonttools.readthedocs.io/en/latest/varLib/instancer.html).
- Where local fonts are unavailable, the preview explicitly reports its Inter
  fallback. Inter and the bundled monospace fallback keep it usable elsewhere.
- Web and native GPUI use different text shaping/rasterization backends. Matching
  the current chrome's font instances does not guarantee identical text pixels
  or wrapping. Larger text and different font choices still need native checks.
  The current Cosmic/Harfrust path uses 12pt when reading SF's size-dependent
  tracking table; a long 13px line can consequently be about 5px wider in the
  browser. The generated font's advances matched CoreText in direct comparisons.
  Keep shared spacing unchanged for this difference and check native captures.
- Native-only tree-sitter highlighting is not compiled into the WASM fixture.
- Native window decorations, the exact compositor blur filter, CEF page pixels,
  and terminal frames are outside this preview. macOS's native control area is
  reserved in the layout.
- Settings fixtures cover Interface and Panes, with local values rather than the
  complete native configuration model. Runtime-specific controls and unimplemented
  actions are specimens; they do not perform desktop operations.

Use `just showcase-native` at the same size/zoom to check a suspected renderer
difference before changing shared spacing to compensate for it.

The web package also builds both the host and canvas with `npm run build` from
`web/`. Use release WASM for a distributable build; debug WASM is intentionally
large. Static builds use the fallback fonts because the local-font endpoint only
exists in the development server.
