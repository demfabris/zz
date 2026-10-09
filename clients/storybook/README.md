# zz storybook

Every piece of `zz-ui` and `zpui-kit` in its states, on one web page. The page is plain HTML:
navigation, prose and knobs. Each section of the story on screen is its own zpui window, mounted
into the section's element and sized to its content.

```bash
just storybook run      # build, serve on http://127.0.0.1:8097, rebuild on save
just storybook build    # build into clients/storybook/dist (add --release for an optimized build)
```

URLs carry the story, the section and the knobs: `#/agent/composer?theme=dark&radius=12`.

## Writing a story

A story is a `Story` constant in `src/stories/<id>.rs`, listed in `src/stories/mod.rs`:

```rust
pub const STORY: Story = Story {
    id: "buttons",
    name: "Buttons",
    group: "Kit",
    summary: "One sentence for the page header.",
    sections: &[Section {
        id: "variants",
        name: "Variants",
        summary: "One sentence under the section title.",
        build: |_, cx| stateless(variants, cx),
    }],
};

fn variants(_: &mut Window, _: &mut App) -> AnyElement {
    states()
        .state("default", Button::new("a").label("Save"))
        .state("disabled", Button::new("b").label("Save").disabled(true))
        .into_any_element()
}
```

- One section per component piece. Each section shows that piece's states with `states()`:
  a labelled list, or a grid with `.columns(n)`. `row()` lays small items out side by side.
- `build` runs once when the story opens. A section that needs entities (an `InputState`, a
  `SelectState`, a `ListState`) builds a view with `cx.new(..)` and returns it as an `AnyView`;
  stateless ones use `stateless(fn, cx)`, which re-renders on every frame.
- Build fixtures from the same types the app uses. Never reach a daemon, the network or the
  filesystem: everything must render in a browser tab from constants.
- Ids must be lowercase with hyphens; a test checks they are unique.
- Element ids inside one section must be unique: the same id twice shares keyed state.
- Use `web_time::Instant`, never `std::time::Instant`, which panics on wasm.
- One focused element exists per window. A state that needs focus or an open menu goes in its
  own section.

## For agents

The page exposes `window.storybook`:

```js
storybook.stories();                       // the registry: stories, sections and mount ids
await storybook.show("agent", "composer"); // open a story, scroll to a section
await storybook.setKnobs({ theme: "dark", radius: 12 });
```

The live UI is reachable through `globalThis.zpui` (see
`knowledge/references/zpui-web-agents.md`): every section window's accessibility tree is
mirrored into the page, so `zpui.find({ role: "button", name: "Send" })`, `zpui.click(ref)` and
`zpui.capture(windowId)` work, and so do the browser's own accessibility snapshots.
