# zz UI showcase

A browser-based, WASM inventory of the UI that zz owns and renders today. Each
page renders one kind of piece at a time, in the states that occur in the app
(no assembled screens), so a piece can be evaluated on its own.

The visual definitions live in the shared `crates/zz-ui` (`zz-ui`) crate and are
imported by both `crates/zz` and this WASM catalog, never copied into the
showcase. Desktop-only controllers remain in `crates/zz`; stories replace their
CEF, PTY, daemon, and filesystem inputs with deterministic state and then render
the same components.

## First run

```bash
just showcase-setup
just showcase
```

`just showcase` performs an initial debug build, starts Vite at
<http://localhost:3131>, and keeps `cargo watch` running. Editing Rust under
`crates/zz-ui/src/` or `examples/ui-showcase/src/` rebuilds the WASM bindings; Vite
then reloads the page.
Only the browser build uses nightly, matching the pinned GPUI web gallery; the
rest of the workspace continues to use the repository's stable toolchain. Set
`SHOWCASE_TOOLCHAIN` to override it when testing another nightly.

The showcase is a small, isolated Cargo workspace. Its lockfile pins a
web-tested GPUI revision, while the desktop workspace keeps its native renderer
patch stack. Both targets compile the same `zz-ui` source; a small backend-only
pixel difference is still possible between the native and web renderers.

## Other commands

```bash
just showcase-build          # debug WASM + JS bindings
just showcase-build-release  # optimized WASM + JS bindings
```

The generated bindings live under `web/src/wasm/` and are ignored. The checked-in
icons and Inter v4.1 variable fonts are embedded into both debug and release WASM
binaries so the showcase works without an asset CDN and remains visually stable
during `cargo watch` rebuilds. Inter is registered and selected only by the
showcase; it does not change the desktop application's UI or terminal fonts.

## Catalog scope

`StoryId::ALL` in `src/showcase.rs` is the page list: an overview, then one page
per bundle of pieces, grouped by which layer owns them.

Primitives (the `zz-ui` widget layer):

- buttons: every variant, size, state, and the icon-only form;
- tags & badges: tag variants and the status badges built on them;
- inputs & selects: the text field (including the masked one), the bounded
  number input, and dropdowns;
- toggles, keys & feedback: switches, Kbd pills, spinners, separators.

Compositions (the pieces `crates/zz` assembles from them):

- navigation: host-tree rows and their disclosure, the titlebar strip that
  stands in for the sidebar, the sidebar's tmux status section, and the
  titlebar status label;
- panes & terminal: display-panes labels, pane status tags, the mode, search,
  status, and link overlays, and the workspace's connection states;
- commands & choosers: palette input, completion rows, tree and paste-buffer
  rows, and the chooser footers;
- browser: toolbar controls, the address bar, start page, and recovery states;
- code editor: the rope-backed buffer with syntax highlighting;
- agent: the pane header, and the thread timeline's entry, tool-call,
  tool-payload, subagent, and notification rows;
- settings: navigation buttons, setting cards, mux rows, provenance and reset;
- dialogs & notifications: the shared confirmations, the prompt dialogs, and
  the four toast tones.

The toolbar carries the two window-wide knobs a specimen is read against: the
theme, and the root rem the app's `cmd-=`/`cmd--`/`cmd-0` actions drive. Named
metrics are declared with `rems_from_px`, so scaling the rem is how a piece is
checked at a size other than the default one.

Each page's toolbar tags its group and the source it renders from
(`crates/zz-ui/src/pane.rs`, `zz_ui::button`).

A page is a stack of galleries; a gallery is a row of labeled specimens; and a
specimen calls the same `zz_ui` constructor `crates/zz` calls, at the size the
app gives it and against the live `Theme` . `specimen("gapped · inactive ·
dimmed", pane_chrome_fixture(…))` in `stories/panes.rs`. Native-only inputs (PTY
pixels, CEF frames, and daemon snapshots) use deterministic fixture data so the
UI stays interactive and repeatable in WASM.

When the app adds or removes a rendered surface, put its visual composition in
`crates/zz-ui`, consume it from `crates/zz`, then add or remove the fixture-backed
story in `StoryId::ALL`. Do not reproduce app markup under `stories/`; story
modules should only assemble shared components and provide state, callbacks, or
native-data fixtures.
