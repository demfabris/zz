//! Inert stand-ins compiled when the `agent-pane` feature is off.

// Stub methods keep the real API's `self` receivers without reading them.
#![allow(clippy::unused_self)]

use std::sync::Arc;

use gpui::{App, Context, EventEmitter, Task, Window, div, prelude::*};
use zz_protocol::{AgentDescriptor, AgentProvider, PaneId};
use zz_ui::{ActiveTheme as _, Colorize as _};

use crate::{config::AgentConfig, mux::client::MuxClient};

/// Mirrors the real event enum so the workspace subscription match compiles.
/// Never constructed: the stub controller emits nothing.
#[allow(dead_code)]
pub(crate) enum AgentControllerEvent {
    Pane {
        pane: PaneId,
    },
    Provider {
        pane: PaneId,
        provider: AgentProvider,
    },
    Session {
        pane: PaneId,
        session_id: Arc<str>,
        cwd: std::path::PathBuf,
    },
    Title {
        pane: PaneId,
        title: Arc<str>,
    },
}

/// Mirrors the real fleet rollup; always the zero partition here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentAttention {
    pub(crate) waiting: usize,
    pub(crate) failed: usize,
    pub(crate) running: usize,
    pub(crate) waiting_pane: Option<PaneId>,
    pub(crate) failed_pane: Option<PaneId>,
}

impl AgentAttention {
    pub(crate) fn is_quiet(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AgentPreferences;

impl AgentPreferences {
    pub fn load_persistent() -> Self {
        Self
    }
}

pub struct AgentController;

impl EventEmitter<AgentControllerEvent> for AgentController {}

impl AgentController {
    // Only tests construct a bare controller: view tests that need a sidebar.
    #[cfg(test)]
    pub(crate) fn new(_config: AgentConfig) -> Self {
        Self
    }

    pub fn with_preferences(
        _config: AgentConfig,
        _preferences: AgentPreferences,
        _socket: Option<String>,
    ) -> Self {
        Self
    }

    pub(crate) fn ensure_pane(
        &mut self,
        _pane: PaneId,
        _descriptor: &AgentDescriptor,
        _cx: &mut Context<Self>,
    ) {
    }

    pub(crate) fn synchronize_config(&mut self, _config: AgentConfig, _cx: &mut Context<Self>) {}

    pub(crate) fn set_session_name(&mut self, _session: Option<String>) {}

    pub(crate) fn retain_panes(&mut self, _retained: &std::collections::BTreeSet<PaneId>) {}

    pub(crate) fn attention(&self) -> AgentAttention {
        AgentAttention::default()
    }

    pub(crate) fn append_composer(&mut self, _pane: PaneId, _text: &str, _cx: &mut Context<Self>) {}

    pub(crate) fn prompt(
        &mut self,
        _pane: PaneId,
        _text: &str,
        _images: Vec<Arc<gpui::Image>>,
        _cx: &mut Context<Self>,
    ) -> Result<(), Arc<str>> {
        Err("agent panes are not included in this build".into())
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        false
    }

    pub(crate) fn is_shutdown_complete(&self) -> bool {
        true
    }

    pub(crate) fn shutdown(&mut self, _cx: &mut Context<Self>) -> Task<bool> {
        Task::ready(true)
    }
}

pub fn warm_agent_adapter_cache(_config: &AgentConfig) {}

/// Placeholder for an agent pane this build cannot host.
pub(crate) struct AgentView {
    focus_handle: gpui::FocusHandle,
}

impl AgentView {
    pub(crate) fn new(
        _pane: PaneId,
        _descriptor: &AgentDescriptor,
        _controller: gpui::Entity<AgentController>,
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

    pub(crate) fn set_visible(&mut self, _visible: bool, _cx: &mut Context<Self>) {}

    pub(crate) fn set_window_corners(
        &mut self,
        _corners: crate::window::corners::WindowCorners,
        _cx: &mut Context<Self>,
    ) {
    }
}

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground.muted())
            .child("Agent panes are not included in this build")
    }
}
