# libghostty-vt

Safe Rust API over `libghostty-vt-sys`.

Handle types (`Terminal`, `RenderState`, `KeyEncoder`, etc.) are `!Send + !Sync` by design. Callers should drive all operations from a single thread.

`Terminal::clone_screen` returns an owned `ScreenSnapshot` of the active screen,
including history. A snapshot keeps its content after the source changes or drops,
and callers can move it to a worker thread. Its read references borrow the snapshot;
resize, scroll, compression and color updates require a mutable snapshot reference.
The snapshot API keeps the owned terminal and tracking handles private.

`GridRow` resolves a row once and checks column bounds before returning a cell
reference. Native getters also check the dimensions of the page that owns the row.
The sys crate pins the Ghostty fork that supplies the screen clone C API.
