//! Inert stand-ins compiled when the `editor-pane` feature is off.

// Stub methods keep the real API's `self` receivers without reading them.
#![allow(clippy::unused_self)]

use gpui::{App, Context, Window, div, prelude::*};
use zz_protocol::{EditorDescriptor, PaneId};
use zz_ui::{ActiveTheme as _, Colorize as _};

use crate::mux::client::MuxClient;

pub fn init(_cx: &mut App) {}

/// Placeholder for an editor pane this build cannot host.
pub(crate) struct EditorView {
    focus_handle: gpui::FocusHandle,
}

impl EditorView {
    pub(crate) fn new(
        _pane: PaneId,
        _descriptor: &EditorDescriptor,
        _mux: gpui::Entity<MuxClient>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    pub(crate) fn focus(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) fn synchronize_descriptor(
        &mut self,
        _descriptor: &EditorDescriptor,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    pub(crate) fn set_window_corners(
        &mut self,
        _corners: crate::window::corners::WindowCorners,
        _cx: &mut Context<Self>,
    ) {
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(crate::theme::app_pane_background(cx))
            .text_color(cx.theme().foreground.muted())
            .child("Editor panes are not included in this build")
    }
}
