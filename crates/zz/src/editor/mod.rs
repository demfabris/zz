//! Editor panes, behind the `editor-pane` cargo feature. [`stub`] stands in
//! when the feature is off.

#[cfg(feature = "editor-pane")]
mod view;

#[cfg(feature = "editor-pane")]
pub(crate) use view::EditorView;
#[cfg(feature = "editor-pane")]
pub use view::init;

#[cfg(not(feature = "editor-pane"))]
mod stub;
#[cfg(not(feature = "editor-pane"))]
pub(crate) use stub::EditorView;
#[cfg(not(feature = "editor-pane"))]
pub use stub::init;
