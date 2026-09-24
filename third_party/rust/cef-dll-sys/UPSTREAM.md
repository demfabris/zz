# CEF bindings with deferred Linux loading

This package preserves the `cef-dll-sys` 152.2.0+152.0.6 API and re-exports its types
from cef-rs commit `207bdd2917c038283635022ffcf7767496bcc7e6`. The workspace patches
the crates.io package to this adapter. Cargo resolves the adapter's upstream
dependency from its pinned Git source, which avoids a dependency cycle.

On Linux x86_64, the upstream `dox` feature supplies generated types without its
native `-lcef` link. The adapter resolves 194 C functions with `libloading` when
`load_library()` first runs. It validates the complete symbol table before
publishing it and keeps the library handle for the process lifetime. Calls through
the generated wrappers require a successful load. zz checks this before subprocess
dispatch or the first browser operation and reports failures through `BrowserError`.

On Linux x86_64, native CEF archive resolution and development resource copying
in `build.rs` follow the pinned upstream build script. The adapter omits native
link directives on that target.
It first looks for `libcef.so` beside the executable, then beside Cargo's `deps`
directory for tests, then in the configured or build-time CEF distribution.
The library remains loaded after the last browser closes because CEF objects,
threads, and function pointers can outlive individual sessions.

Linux ARM64 and Windows forward upstream unchanged. macOS forwards upstream
bindings and native wrapper builds, and adds the wrapper's required `libc++`
link dependency. This keeps consumers of CEF's scoped loader independent of
whether another crate happens to link the C++ runtime. Native ARM64
qualification remains outstanding: CEF reports include static TLS allocation
failures when loading Chromium with `dlopen`
([CEF issue 3803](https://github.com/chromiumembedded/cef/issues/3803)).

## Updating the pin

`src/functions.rs` and `src/exports.rs` contain generated declarations, not a copy
of the upstream binding types. `generate.py` checks the SHA-256 of the published
Linux x86_64 binding source. Each wrapper also has a compile-time function-pointer
assignment against its upstream declaration, so a signature change fails to build.

Run the generator against the `sys/src/bindings/x86_64_unknown_linux_gnu.rs` file
from the pinned checkout:

```sh
python3 third_party/rust/cef-dll-sys/generate.py /path/to/cef-rs/sys/src/bindings/x86_64_unknown_linux_gnu.rs --check
```

For an upgrade, review the new published source and update the commit, package
version, binding hash, expected function count, and CEF distribution version
together. Regenerate without `--check`, then run the adapter's loading tests,
zz-browser tests, and native browser smoke tests. Keep the ABI checks intact.

## Native probe

On 2026-09-22, an isolated Linux x86_64 executable linked only libc and loaded the
pinned CEF with both `RTLD_NOW | RTLD_LOCAL` and `RTLD_NOW | RTLD_GLOBAL`. Both runs
initialized CEF, dispatched sandboxed subprocesses, painted a 640 × 480 local
HTML page through off-screen rendering, received browser closure, and shut down
with exit status zero. The adapter uses `RTLD_LOCAL`.

The scratch source and logs are in
`target/profiles/linux-trim-20260922/cef-loader-probe/`. This probe covers the CEF
loader and readback callbacks; the GPUI integration and shared GPU texture path
still require the app's native smoke tests.
