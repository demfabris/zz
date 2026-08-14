//! Inert stand-ins compiled on iOS, where there is no CEF.

// Stub methods keep the real API's `self` receivers without reading them.
#![allow(clippy::unused_self, dead_code)]

pub(crate) mod controller {
    use std::sync::Arc;

    use gpui::{App, Context, EventEmitter, Task};
    use zz_browser::BrowserEvent;
    use zz_protocol::PaneId;

    /// Mirrors the real tab handle so `ControllerEvent::Browser` matches type.
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub(crate) struct TabId(pub u64);

    /// Mirrors the real event enum so the workspace subscription compiles.
    #[derive(Clone, Debug)]
    pub(crate) enum ControllerEvent {
        RuntimeReady,
        Browser {
            pane: PaneId,
            tab: TabId,
            event: BrowserEvent,
        },
        Failed(Arc<str>),
    }

    pub struct BrowserController;

    impl EventEmitter<ControllerEvent> for BrowserController {}

    impl BrowserController {
        pub fn new(_cx: &mut Context<Self>) -> Self {
            Self
        }

        pub(crate) fn active_tab(&self, _pane: PaneId) -> Option<TabId> {
            None
        }

        pub(crate) fn close_pane(&mut self, _pane: PaneId) {}

        pub(crate) fn is_shutting_down(&self) -> bool {
            false
        }

        pub(crate) fn is_shutdown_complete(&self) -> bool {
            true
        }

        pub(crate) fn log_diagnostic_snapshot(&self, _reason: &str) {}

        pub(crate) fn shutdown(&mut self, _cx: &mut Context<Self>) -> Task<bool> {
            Task::ready(true)
        }
    }

    /// Kept so `App`-taking call sites read the same on both platforms.
    pub(crate) fn is_available(_cx: &App) -> bool {
        false
    }
}

pub(crate) mod recent_pages {
    use gpui::App;

    pub fn init(_cx: &mut App) {}
}

pub(crate) mod view {
    use gpui::{App, Context, Entity, FocusHandle, Window, div, prelude::*};
    use zz_protocol::{BrowserCommand, BrowserDescriptor, PaneId};
    use zz_ui::{ActiveTheme as _, Colorize as _};

    use super::controller::BrowserController;
    use crate::{mux::client::MuxClient, window::corners::WindowCorners};

    pub fn init(_cx: &mut App) {}

    /// Placeholder for a browser pane this build cannot host.
    pub(crate) struct BrowserView {
        focus_handle: FocusHandle,
    }

    impl BrowserView {
        pub(crate) fn new(
            _pane: PaneId,
            _descriptor: &BrowserDescriptor,
            _controller: Entity<BrowserController>,
            _mux: Entity<MuxClient>,
            _window: &mut Window,
            cx: &mut Context<Self>,
        ) -> Self {
            Self {
                focus_handle: cx.focus_handle(),
            }
        }

        pub(crate) fn pane_focus_handle(&self, _cx: &App) -> FocusHandle {
            self.focus_handle.clone()
        }

        pub(crate) fn set_window_corners(
            &mut self,
            _corners: WindowCorners,
            _cx: &mut Context<Self>,
        ) {
        }

        pub(crate) fn set_visible(
            &mut self,
            _visible: bool,
            _window: &mut Window,
            _cx: &mut Context<Self>,
        ) {
        }

        pub(crate) fn apply_command(&mut self, _command: BrowserCommand, _cx: &mut Context<Self>) {}

        pub(crate) fn screenshot(
            &mut self,
            _request_id: u64,
            _path: String,
            _cx: &mut Context<Self>,
        ) {
        }

        pub(crate) fn synchronize_profile(&mut self, _profile: &str, _cx: &mut Context<Self>) {}

        pub(crate) fn synchronize_tabs(
            &mut self,
            _tabs: &[String],
            _active_tab: usize,
            _window: &mut Window,
            _cx: &mut Context<Self>,
        ) {
        }
    }

    impl Render for BrowserView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(crate::theme::app_pane_background(cx))
                .text_color(cx.theme().foreground.muted())
                .child("Browser panes are desktop-only")
        }
    }
}
