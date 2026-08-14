//! The C ABI over [`zz_client::ClientCore`]: one handle per daemon
//! connection, a pollable wake fd for any main loop (GSource,
//! QSocketNotifier, DispatchSource), typed events, and zero-copy viewport
//! access through acquire/release. The hand-maintained contract lives in
//! `include/zz-client.h`; the C smoke test compiles and links against it, so
//! header drift fails the build.

#[cfg(unix)]
mod ffi;

#[cfg(unix)]
pub use ffi::*;
