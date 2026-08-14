# GTK4/GNOME-specific traps

Learned building `crates/zz-gtk` (GTK 4.22, libadwaita 1.9, gtk4-rs 0.11). Every
entry cost an agent real debugging time. Read this whole file before writing any
GTK shell code; read `pitfalls.md` first for the protocol-level rules.

## Input

1. **`EventControllerKey::set_im_context` is a trap.** GTK filters the event
   through the IM *before* emitting `key-pressed`, so plain letters never reach
   your handler and never reach the daemon's key tables. Call
   `IMContextExt::filter_keypress` by hand from inside `key-pressed` instead.
2. **ibus (Ubuntu GNOME default) commits asynchronously.** "IM claimed the
   press, no synchronous commit" is not "composing" — treating it that way
   drops every keystroke. Capture commits arriving inside the press, and route
   late commits as `InputMessage::Text`.
3. **`EventControllerKey` cannot swallow a key release** — its release signal
   returns `()`. Symmetric press/release control (the prefix claim, kitty
   pairing) needs `EventControllerLegacy`, which owns propagation for both.
4. **GDK has no key-autorepeat flag.** Infer a repeat from a held-keycode set
   (pair releases by hardware keycode, not keyval — modifiers can change the
   keyval between press and release) and clear the set on focus-out, or one
   lost release strands a phantom held key.
5. **GDK resolves keypad keyvals to characters** (`KP_1` → `'1'`), so keypad
   entries beyond navigation keys are unnecessary; dead keys fold to
   `Unidentified` with no text and only reach the wire through the IM commit.
6. **`GtkEntry::grab_focus` selects all text.** A prompt mirroring
   daemon-owned input must use `grab_focus_without_selecting`.

## Widgets

7. **`GtkListBox` cannot be drained with `remove`/`first_child`.** An empty
   list still answers `first_child`, so the idiomatic while-let loop spins
   forever at full CPU. Use `remove_all()` (GTK ≥ 4.12).
8. **A `PopoverMenu` parented into a `GtkListBox` breaks `remove_all()`** —
   the list refuses to remove the popover and loops, logging hundreds of MB a
   minute. Parent row context menus to the enclosing `ScrolledWindow` and map
   coordinates with `compute_point`.
9. **Never add a still-parented widget to a new container.** Clear the old
   container first; GTK reports the mistake as an endless "Tried to remove
   non-child" log storm, not an error. Corollary for route/stack composition:
   the widget a route wraps must be the widget that currently owns the
   content — after refactors, re-check who parents what.
10. **`adw::TabView::close_page` emits `::close-page`.** A handler mapping
    that signal to a daemon `kill-window` will destroy real windows every time
    the strip is rebuilt. Gate the handler on a sync flag and finish with
    `close_page_finish`.
11. **libadwaita rows parse Pango markup.** Session names, window titles, and
    buffer previews are user data — escape with `glib::markup_escape_text` or
    a window named `<b>` corrupts the row.
12. **A dialog rendering a daemon-owned overlay needs `can_close(false)`.**
    Escape must reach the daemon's chooser key table; a dialog that closes
    itself desyncs the daemon, which still believes the chooser is open.
13. **`gtk::Window::set_default_icon_name` before `app.run()` panics** ("GTK
    has not been initialized"). It belongs in `connect_startup`.

## Rendering and appearance

14. **Pango's `FontMetrics::height()` includes the family line gap.** Terminal
    cell height is ascent + descent; the line gap costs a grid row and opens
    the rows apart.
15. **Style runs beat one-layout-per-row.** Font fallback (emoji, CJK) shifts
    advances mid-row and the drift persists to end of line; runs keyed on the
    resolved `PackedStyle` re-sync at every boundary. `CellWidth::SpacerTail`/
    `SpacerHead` must break runs and emit nothing, or every glyph after a wide
    character is one column left.
16. **Don't drive terminal font size through GTK's DPI/text scaling.** The
    viewport widget caches cell metrics keyed on the appearance value; a Pango
    resolution change scales glyphs under a stale grid. Scale chrome with CSS
    and the grid through the point size the pane is handed.
17. **Follow dark/light by republishing the color scheme to the daemon** (it
    re-resolves the palette) rather than recoloring client-side — one source
    of truth, and remember the choice so reconnect dials with the current
    scheme.

## Desktop integration

18. **The `tray-icon` crate is unusable from GTK4** — its Linux backend is
    libappindicator/GTK3, unlinkable in the same process. Use ksni
    (StatusNotifierItem over D-Bus); GNOME needs the AppIndicator extension,
    so degrade gracefully. Install the tray's close-request hook before the
    shell's, or closing detaches instead of hiding.
19. **`org.gtk.Actions` over D-Bus drives a GTK client headlessly and
    legitimately.** Even with `NON_UNIQUE`, the app exports its window action
    group at `/<app-path>/window/1` under its unique bus name — activate
    actions from a test harness instead of ever synthesizing input. A
    parameterised action doubles as a real deep-link feature.
20. **Screenshots on GNOME Wayland:** the Shell's D-Bus screenshot API is
    `AccessDenied` to unsandboxed callers. Run the app under
    `GDK_BACKEND=x11` and capture with `import -window <xid>`, finding the
    xid via `xwininfo -root -tree` (xdotool/wmctrl may not exist). And
    `_NET_ACTIVE_WINDOW` from XWayland does NOT reflect compositor keyboard
    focus — never use it to justify anything, least of all input injection.
21. **Cap log capture when smoke-testing a GUI.** A widget-parenting mistake
    logs at full speed forever (hundreds of MB per minute); redirect to a file
    on a size budget or a tmpfs you truncate.
