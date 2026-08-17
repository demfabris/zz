---
name: gpui-interface
description: >-
  GPUI layout, hover, and optical alignment rules for zz chrome. Use when
  building or tweaking GPUI UI — buttons, rows, labels, icons, chevrons,
  hover/fg highlight, padding, composer chrome, agent timeline rows, or
  anything that "looks a few pixels off". Reach for it before putting
  `.hover()` on a nested div or trusting `items_center` to line up an icon
  with text.
---

# GPUI interface

Theme tokens, clippy's ban on raw `rgb`/`hsla`, and widget conventions live in
`knowledge/configuration/ui-conventions.md`. This skill is the GPUI mechanics
that file does not cover. Source of the worked example:
`crates/zz-ui/src/agent.rs` (`activity_row`, `activity_row_glyph`).

## Hover lives on the id'd row

GPUI only tracks hover on a **stateful** element — one with `.id()`. A nested
`div()` with `.hover()` or `group_hover` never sees the pointer.

Put color and hover on the same element that already has the id (usually the
clickable row). Children **inherit** `text_color` if they do not set their own.

```rust
let foreground = cx.theme().foreground;
h_flex()
    .id(("some-toggle", id))
    .text_color(cx.theme().foreground.muted())
    .cursor_pointer()
    .hover(move |this| this.text_color(foreground))
    .child(Icon::new(icon).size(px(13.0))) // inherits, no own color
    .child(div().child(label))             // inherits
```

Do not:

- `.hover()` on a child without `.id()`
- `group` / `group_hover` as a workaround when the row is already stateful
- set `text_color` on the label and then hope parent hover overrides it

If icons must stay muted while text highlights, set color on the icon
explicitly. If the whole row should go to `foreground` (text, icon, chevron),
set color only on the row.

## `items_center` centres boxes, not ink

GPUI centres a glyph on its **ascent/descent box**. The ink a reader sees sits
below that centre — at 13px, about 0.33px for caps and 1.49px for x-height.
Flex `items_center` will look like the icon sits high and the text is sunk.

Do this instead:

1. One shared builder for every row of the same shape (font, metrics, hover
   cannot drift across callers).
2. Explicit row height, font size, and line-height in pixels.
3. Line-height **taller** than ascent+descent, or `overflow_hidden` clips
   descenders. At 13px the system font is 15.31px tall; use 16px.
4. Size icons to the font (`.size(px(13.0))`), not `.small()` (that is 14px,
   `size_3p5`) or `.xsmall()` (12px, `size_3`).
5. Optically drop the icon so it sits on the letters, not the box centre.
   Less than the x-height offset so it never overshoots: **0.5px** at 13px.

```rust
const ROW_HEIGHT: f32 = 28.0;
const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 16.0; // > 15.31px ascent+descent
const ICON_DROP: f32 = 0.5;

fn row_glyph(icon: IconName, size: f32) -> Div {
    div()
        .flex_none()
        .relative()
        .top(px(ICON_DROP))
        .child(Icon::new(icon).size(px(size)))
}
```

`line_height(relative(1.0))` is not enough: it still centres on the em box,
and a 13px line box clips the 15.31px font.

Wrapping rows (markdown task lists) cannot `items_center` the marker on the
whole block. Give the checkbox a **first-line strut** of `window.line_height()`,
size the box to the ambient font (`window.text_style().font_size`), then apply
the same 0.5px optical drop. Magic `.mt(rems(0.4))` plus a rem-sized box will
not track a 13px agent transcript. Worked example:
`render_list_item_row` in `crates/zz-ui/src/widget/text/node.rs`.

## One row, three callers

If group headers, tool rows, and reasoning rows must look identical, they
share one function. Copy-pasting the same `h_flex` three times is how font
size, chevron placement, and hover diverge.

Worked example: `activity_row` in `crates/zz-ui/src/agent.rs`. Chevron sits
**after the label** (same flex children), not `justify_between`.
