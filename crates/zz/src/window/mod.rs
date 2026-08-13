pub(crate) mod background;
pub(crate) mod corners;
pub(crate) mod drag;
#[cfg(target_os = "linux")]
pub(crate) mod frame;
/// Window geometry persistence. An iPad window has no bounds to restore.
#[cfg(not(target_os = "ios"))]
pub(crate) mod state;
pub(crate) mod toast;
