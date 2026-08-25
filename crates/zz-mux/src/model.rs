use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use zz_protocol::{
    AgentDescriptor, AgentProvider, Axis, BrowserDescriptor, EditorDescriptor, LayoutNode,
    MAX_GUI_TEXT_BYTES, MuxSnapshot, PaneId, PaneKindSnapshot, PaneSnapshot, ServerError,
    SessionId, SessionSnapshot, SplitId, WindowId, WindowSnapshot, normalize_browser_profile_name,
};

use crate::{
    PresetOptions,
    layout::{CellLayout, LayoutError, SplitSize},
};

pub(crate) const DEFAULT_WINDOW_EXTENT: (u16, u16) = (80, 24);
const SYNCHRONIZE_PANES: u8 = 1 << 0;
const AUTOMATIC_RENAME: u8 = 1 << 1;
const AGGRESSIVE_RESIZE: u8 = 1 << 2;
const LAYOUT_COORDINATE_MAX: u32 = 1_000_000;
const MAX_AGENT_SESSION_ID_BYTES: usize = 16 * 1024;
const MAX_WINDOW_INDEX: u32 = i32::MAX.cast_unsigned();

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutPreset {
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainHorizontalMirrored,
    MainVertical,
    MainVerticalMirrored,
    Tiled,
}

impl LayoutPreset {
    pub const ALL: [Self; 7] = [
        Self::EvenHorizontal,
        Self::EvenVertical,
        Self::MainHorizontal,
        Self::MainHorizontalMirrored,
        Self::MainVertical,
        Self::MainVerticalMirrored,
        Self::Tiled,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::EvenHorizontal => "even-horizontal",
            Self::EvenVertical => "even-vertical",
            Self::MainHorizontal => "main-horizontal",
            Self::MainHorizontalMirrored => "main-horizontal-mirrored",
            Self::MainVertical => "main-vertical",
            Self::MainVerticalMirrored => "main-vertical-mirrored",
            Self::Tiled => "tiled",
        }
    }

    #[must_use]
    const fn at_offset(self, offset: isize) -> Self {
        let current = self as isize;
        let count = 7_isize;
        Self::ALL[(current + offset).rem_euclid(count) as usize]
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PaneRect {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct InputOptions {
    values: u8,
    overrides: u8,
}

impl InputOptions {
    const fn synchronize_panes(self) -> Option<bool> {
        if self.overrides & SYNCHRONIZE_PANES == 0 {
            None
        } else {
            Some(self.values & SYNCHRONIZE_PANES != 0)
        }
    }

    fn set_synchronize_panes(&mut self, value: Option<bool>) {
        if let Some(value) = value {
            self.overrides |= SYNCHRONIZE_PANES;
            if value {
                self.values |= SYNCHRONIZE_PANES;
            } else {
                self.values &= !SYNCHRONIZE_PANES;
            }
        } else {
            self.values &= !SYNCHRONIZE_PANES;
            self.overrides &= !SYNCHRONIZE_PANES;
        }
    }

    const fn automatic_rename(self) -> Option<bool> {
        if self.overrides & AUTOMATIC_RENAME == 0 {
            None
        } else {
            Some(self.values & AUTOMATIC_RENAME != 0)
        }
    }

    fn set_automatic_rename(&mut self, value: Option<bool>) {
        if let Some(value) = value {
            self.overrides |= AUTOMATIC_RENAME;
            if value {
                self.values |= AUTOMATIC_RENAME;
            } else {
                self.values &= !AUTOMATIC_RENAME;
            }
        } else {
            self.values &= !AUTOMATIC_RENAME;
            self.overrides &= !AUTOMATIC_RENAME;
        }
    }

    const fn aggressive_resize(self) -> Option<bool> {
        if self.overrides & AGGRESSIVE_RESIZE == 0 {
            None
        } else {
            Some(self.values & AGGRESSIVE_RESIZE != 0)
        }
    }

    fn set_aggressive_resize(&mut self, value: Option<bool>) {
        if let Some(value) = value {
            self.overrides |= AGGRESSIVE_RESIZE;
            if value {
                self.values |= AGGRESSIVE_RESIZE;
            } else {
                self.values &= !AGGRESSIVE_RESIZE;
            }
        } else {
            self.values &= !AGGRESSIVE_RESIZE;
            self.overrides &= !AGGRESSIVE_RESIZE;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Picker { inherit_cwd_from: Option<PaneId> },
    Terminal,
    Browser(BrowserDescriptor),
    Agent(AgentDescriptor),
    Editor(EditorDescriptor),
}

impl PaneKind {
    fn snapshot(&self) -> PaneKindSnapshot {
        match self {
            Self::Picker { .. } => PaneKindSnapshot::Picker,
            Self::Terminal => PaneKindSnapshot::Terminal,
            Self::Browser(browser) => PaneKindSnapshot::Browser(browser.clone()),
            Self::Agent(agent) => PaneKindSnapshot::Agent(agent.clone()),
            Self::Editor(editor) => PaneKindSnapshot::Editor(editor.clone()),
        }
    }
}

/// Where a split drops the pane it creates: `size` is the requested size of
/// the new pane, `before` puts it left of or above the target,
/// `full_size` spans the whole window instead of the target's box, and
/// `detached` leaves focus where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitPlacement {
    pub size: SplitSize,
    pub before: bool,
    pub full_size: bool,
    pub detached: bool,
}

impl Default for SplitPlacement {
    fn default() -> Self {
        Self {
            size: SplitSize::Default,
            before: false,
            full_size: false,
            detached: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: PaneId,
    pub title: String,
    pub kind: PaneKind,
    pub active_point: u64,
    /// A BEL rang here and nobody has been back since.
    pub bell: bool,
    pub dead: bool,
    pub dead_status: Option<u32>,
    pub dead_time: Option<u64>,
    pub(crate) input_off: bool,
    pub(crate) empty: bool,
    input_options: InputOptions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: WindowId,
    pub session: SessionId,
    pub index: u32,
    pub name: String,
    pub created: u64,
    pub activity: u64,
    /// The pin's `WINLINK_ACTIVITY`: output landed here and nobody has been
    /// back since. Server-side only; formats and labels carry it to clients.
    pub activity_flag: bool,
    /// The pin's `WINLINK_SILENCE`: the monitor-silence deadline expired and
    /// nobody has been back since.
    pub silence_flag: bool,
    pub active_pane: PaneId,
    pub zoomed_pane: Option<PaneId>,
    pub layout: CellLayout,
    pub panes: BTreeMap<PaneId, Pane>,
    pane_order: Vec<PaneId>,
    last_panes: Vec<PaneId>,
    last_layout: Option<LayoutPreset>,
    previous_layout: Option<Box<CellLayout>>,
    pub(crate) last_extent_probe: Option<(PaneId, u16, u16)>,
    input_options: InputOptions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub created: Option<i64>,
    pub sort_created: u64,
    pub sort_activity: u64,
    pub windows: Vec<WindowId>,
    pub active_window: WindowId,
    last_window: Option<WindowId>,
}

#[derive(Clone, Copy)]
struct WindowTargetResolution {
    session: SessionId,
    window: Option<WindowId>,
    index: Option<u32>,
}

#[derive(Clone, Copy)]
struct WindowInSessionResolution {
    window: Option<WindowId>,
    index: u32,
}

impl Session {
    #[must_use]
    pub fn last_window(&self) -> Option<WindowId> {
        self.last_window
    }

    fn activate_window(&mut self, window: WindowId) -> bool {
        if self.active_window == window {
            false
        } else {
            self.last_window = Some(self.active_window);
            self.active_window = window;
            true
        }
    }

    fn forget_window(&mut self, window: WindowId) -> Option<WindowId> {
        self.windows.retain(|candidate| *candidate != window);
        if self.last_window == Some(window) {
            self.last_window = None;
        }
        if self.active_window == window && !self.windows.is_empty() {
            self.active_window = self
                .last_window
                .take()
                .filter(|candidate| self.windows.contains(candidate))
                .unwrap_or(self.windows[0]);
            Some(self.active_window)
        } else {
            None
        }
    }
}

impl Window {
    /// The cell size a pane presents to formats and status lines: the full
    /// window extent while the pane is zoomed (tmux swaps in a one-leaf
    /// layout during zoom), otherwise its tree allocation.
    #[must_use]
    pub fn displayed_pane_geometry(&self, pane: PaneId) -> Option<(u16, u16)> {
        if self.zoomed_pane == Some(pane) {
            return Some(self.layout.extent());
        }
        let geometry = self.layout.pane_geometry(pane)?;
        Some((geometry.sx, geometry.sy))
    }

    #[must_use]
    pub fn pane_order(&self) -> &[PaneId] {
        &self.pane_order
    }

    pub(crate) fn last_pane(&self) -> Option<PaneId> {
        self.last_panes.first().copied()
    }
}

#[derive(Debug, Default)]
pub struct MuxState {
    generation: u64,
    next_session_id: u64,
    next_window_id: u64,
    next_pane_id: u64,
    next_split_id: u64,
    next_sort_point: u64,
    last_active_session: Option<SessionId>,
    input_options: InputOptions,
    pub sessions: BTreeMap<SessionId, Session>,
    pub windows: BTreeMap<WindowId, Window>,
}

impl MuxState {
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn next_session_id(&self) -> SessionId {
        SessionId(self.next_session_id)
    }

    pub fn create_session(
        &mut self,
        name: impl Into<String>,
    ) -> Result<(SessionId, WindowId, PaneId), ServerError> {
        self.create_session_with_extent(name, DEFAULT_WINDOW_EXTENT)
    }

    pub(crate) fn create_session_with_extent(
        &mut self,
        name: impl Into<String>,
        extent: (u16, u16),
    ) -> Result<(SessionId, WindowId, PaneId), ServerError> {
        self.create_session_with_extent_at(name, extent, 0)
    }

    pub(crate) fn create_session_with_extent_at(
        &mut self,
        name: impl Into<String>,
        extent: (u16, u16),
        index: u32,
    ) -> Result<(SessionId, WindowId, PaneId), ServerError> {
        let name = name.into();
        if self.sessions.values().any(|session| session.name == name) {
            return Err(ServerError::InvalidCommand(format!(
                "duplicate session: {name}"
            )));
        }
        let session_id = self.allocate_session_id();
        let window_id = self.allocate_window_id();
        let pane_id = self.allocate_pane_id();
        let created = self.allocate_sort_point();
        let active_point = self.allocate_sort_point();
        let pane = Pane {
            id: pane_id,
            title: "terminal".to_owned(),
            kind: PaneKind::Terminal,
            active_point,
            bell: false,
            dead: false,
            dead_status: None,
            dead_time: None,
            input_off: false,
            empty: false,
            input_options: InputOptions::default(),
        };
        let window = Window {
            id: window_id,
            session: session_id,
            index,
            name: index.to_string(),
            created,
            activity: created,
            activity_flag: false,
            silence_flag: false,
            active_pane: pane_id,
            zoomed_pane: None,
            layout: CellLayout::new(pane_id, extent.0, extent.1),
            panes: BTreeMap::from([(pane_id, pane)]),
            pane_order: vec![pane_id],
            last_panes: Vec::new(),
            last_layout: None,
            previous_layout: None,
            last_extent_probe: None,
            input_options: InputOptions::default(),
        };
        self.windows.insert(window_id, window);
        self.sessions.insert(
            session_id,
            Session {
                id: session_id,
                name,
                created: None,
                sort_created: created,
                sort_activity: created,
                windows: vec![window_id],
                active_window: window_id,
                last_window: None,
            },
        );
        self.last_active_session = Some(session_id);
        self.bump_generation();
        Ok((session_id, window_id, pane_id))
    }

    pub fn rename_session(
        &mut self,
        session: SessionId,
        name: impl Into<String>,
    ) -> Result<(), ServerError> {
        let name = name.into();
        let current = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        if current.name == name {
            return Ok(());
        }
        if self
            .sessions
            .values()
            .any(|candidate| candidate.name == name)
        {
            return Err(ServerError::InvalidCommand(format!(
                "duplicate session: {name}"
            )));
        }
        self.sessions
            .get_mut(&session)
            .expect("session target was resolved")
            .name = name;
        self.bump_generation();
        Ok(())
    }

    pub fn create_window(
        &mut self,
        session: SessionId,
        name: Option<String>,
        kind: PaneKind,
    ) -> Result<(WindowId, PaneId), ServerError> {
        self.create_window_at(session, None, name, kind, true)
    }

    pub fn create_window_at(
        &mut self,
        session: SessionId,
        index: Option<u32>,
        name: Option<String>,
        kind: PaneKind,
        activate: bool,
    ) -> Result<(WindowId, PaneId), ServerError> {
        self.create_window_at_with_base_index(session, index, name, kind, activate, 0)
    }

    pub(crate) fn create_window_at_with_base_index(
        &mut self,
        session: SessionId,
        index: Option<u32>,
        name: Option<String>,
        kind: PaneKind,
        activate: bool,
        base_index: u32,
    ) -> Result<(WindowId, PaneId), ServerError> {
        let extent = self
            .session_active_window_extent(session)
            .unwrap_or(DEFAULT_WINDOW_EXTENT);
        let index = self.claim_window_index(session, index, base_index)?;
        let window_id = self.allocate_window_id();
        let pane_id = self.allocate_pane_id();
        let created = self.allocate_sort_point();
        let active_point = self.allocate_sort_point();
        let pane = Pane {
            id: pane_id,
            title: pane_title(&kind),
            kind,
            active_point,
            bell: false,
            dead: false,
            dead_status: None,
            dead_time: None,
            input_off: false,
            empty: false,
            input_options: InputOptions::default(),
        };
        let window = Window {
            id: window_id,
            session,
            index,
            name: name.unwrap_or_else(|| index.to_string()),
            created,
            activity: created,
            activity_flag: false,
            silence_flag: false,
            active_pane: pane_id,
            zoomed_pane: None,
            layout: CellLayout::new(pane_id, extent.0, extent.1),
            panes: BTreeMap::from([(pane_id, pane)]),
            pane_order: vec![pane_id],
            last_panes: Vec::new(),
            last_layout: None,
            previous_layout: None,
            last_extent_probe: None,
            input_options: InputOptions::default(),
        };
        self.windows.insert(window_id, window);
        self.sessions
            .get_mut(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?
            .windows
            .push(window_id);
        if activate {
            self.activate_window(session, window_id);
        }
        self.sort_session_windows(session);
        self.bump_generation();
        Ok((window_id, pane_id))
    }

    fn session_active_window_extent(&self, session: SessionId) -> Option<(u16, u16)> {
        let window = self.sessions.get(&session)?.active_window;
        Some(self.windows.get(&window)?.layout.extent())
    }

    /// The index a new window takes in `session`: the requested one when it is
    /// free, otherwise the lowest free index.
    fn claim_window_index(
        &self,
        session: SessionId,
        index: Option<u32>,
        base_index: u32,
    ) -> Result<u32, ServerError> {
        let Some(index) = index else {
            return self.next_window_index(session, base_index);
        };
        if self.window_at_index(session, index).is_some() {
            return Err(ServerError::InvalidCommand(format!(
                "index in use: {index}"
            )));
        }
        if self.sessions.contains_key(&session) {
            Ok(index)
        } else {
            Err(ServerError::MissingTarget(session.to_string()))
        }
    }

    #[must_use]
    pub fn window_at_index(&self, session: SessionId, index: u32) -> Option<WindowId> {
        self.window_matching(session, |window| window.index == index)
    }

    #[must_use]
    pub fn window_named(&self, session: SessionId, name: &str) -> Option<WindowId> {
        self.window_matching(session, |window| window.name == name)
    }

    fn window_matching(
        &self,
        session: SessionId,
        predicate: impl Fn(&Window) -> bool,
    ) -> Option<WindowId> {
        self.sessions
            .get(&session)?
            .windows
            .iter()
            .copied()
            .find(|window| self.windows.get(window).is_some_and(&predicate))
    }

    /// Frees `index` by moving the contiguous run of windows starting there up
    /// one slot, tmux's `winlink_shuffle_up`.
    pub fn shift_windows_up(&mut self, session: SessionId, index: u32) -> Result<(), ServerError> {
        let state = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        let used = state
            .windows
            .iter()
            .filter_map(|window| self.windows.get(window).map(|window| window.index))
            .collect::<BTreeSet<_>>();
        let mut free = index;
        while used.contains(&free) {
            free = free
                .checked_add(1)
                .ok_or_else(|| ServerError::InvalidCommand("no free window index".to_owned()))?;
        }
        let moved = state
            .windows
            .iter()
            .copied()
            .filter(|window| {
                self.windows
                    .get(window)
                    .is_some_and(|window| (index..free).contains(&window.index))
            })
            .collect::<Vec<_>>();
        if moved.is_empty() {
            return Ok(());
        }
        for window in moved {
            self.windows
                .get_mut(&window)
                .expect("shifted window exists")
                .index += 1;
        }
        self.sort_session_windows(session);
        self.bump_generation();
        Ok(())
    }

    pub fn set_window_index(&mut self, window: WindowId, index: u32) -> Result<(), ServerError> {
        let session = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
            .session;
        if self
            .window_at_index(session, index)
            .is_some_and(|occupant| occupant != window)
        {
            return Err(ServerError::InvalidCommand(format!(
                "index in use: {index}"
            )));
        }
        self.windows
            .get_mut(&window)
            .expect("window target was resolved")
            .index = index;
        self.sort_session_windows(session);
        self.bump_generation();
        Ok(())
    }

    pub fn move_window(
        &mut self,
        source: WindowId,
        destination_session: SessionId,
        destination_index: u32,
        kill: bool,
        select: bool,
    ) -> Result<Vec<PaneId>, ServerError> {
        let source_session = self
            .windows
            .get(&source)
            .ok_or_else(|| ServerError::MissingTarget(source.to_string()))?
            .session;
        let source_index = self.windows[&source].index;
        let source_was_active = self.sessions[&source_session].active_window == source;
        if !self.sessions.contains_key(&destination_session) {
            return Err(ServerError::MissingTarget(destination_session.to_string()));
        }
        let occupant = self.window_at_index(destination_session, destination_index);
        if occupant == Some(source) {
            return Err(ServerError::InvalidCommand(format!(
                "same index: {destination_index}"
            )));
        }
        if occupant.is_some() && !kill {
            return Err(ServerError::InvalidCommand(format!(
                "index in use: {destination_index}"
            )));
        }

        let force_select = occupant
            .is_some_and(|occupant| self.sessions[&destination_session].active_window == occupant);
        let mut removed_panes = Vec::new();
        if let Some(occupant) = occupant {
            let removed = self
                .windows
                .remove(&occupant)
                .expect("destination occupant exists");
            removed_panes.extend(removed.pane_order);
            self.sessions
                .get_mut(&destination_session)
                .expect("destination session exists")
                .forget_window(occupant);
        }

        let source_fallback = if source_was_active {
            let session = &self.sessions[&source_session];
            session
                .last_window()
                .filter(|window| *window != source && session.windows.contains(window))
                .or_else(|| {
                    let candidates = session
                        .windows
                        .iter()
                        .copied()
                        .filter_map(|window| {
                            if window == source {
                                (source_session == destination_session)
                                    .then_some((window, destination_index))
                            } else {
                                Some((window, self.windows[&window].index))
                            }
                        })
                        .collect::<Vec<_>>();
                    candidates
                        .iter()
                        .filter(|(_, index)| *index < source_index)
                        .max_by_key(|(_, index)| *index)
                        .or_else(|| candidates.iter().max_by_key(|(_, index)| *index))
                        .map(|(window, _)| *window)
                })
        } else {
            None
        };
        if source_session != destination_session {
            let source_state = self
                .sessions
                .get_mut(&source_session)
                .expect("source session exists");
            source_state.forget_window(source);
            if let Some(fallback) = source_fallback {
                source_state.active_window = fallback;
                source_state.last_window = None;
            }
        }
        {
            let window = self.windows.get_mut(&source).expect("source window exists");
            window.session = destination_session;
            window.index = destination_index;
        }
        if source_session != destination_session {
            self.sessions
                .get_mut(&destination_session)
                .expect("destination session exists")
                .windows
                .push(source);
        }

        let destination_was_empty = self.sessions[&destination_session].windows.len() == 1
            && self.sessions[&destination_session].windows[0] == source;
        if destination_was_empty {
            let destination = self
                .sessions
                .get_mut(&destination_session)
                .expect("destination session exists");
            destination.active_window = source;
            destination.last_window = None;
            self.force_activate_window(destination_session, source);
        } else if select || force_select {
            self.force_activate_window(destination_session, source);
        } else if source_session == destination_session
            && source_was_active
            && let Some(fallback) = source_fallback
        {
            let session = self
                .sessions
                .get_mut(&source_session)
                .expect("source session exists");
            session.active_window = fallback;
            session.last_window = None;
            self.touch_window_activity(fallback);
            self.clear_window_alerts(fallback);
        }

        if source_session != destination_session {
            let source_empty = self.sessions[&source_session].windows.is_empty();
            if source_empty {
                self.sessions.remove(&source_session);
            } else if let Some(fallback) = source_fallback {
                self.touch_window_activity(fallback);
                self.clear_window_alerts(fallback);
            }
        }
        self.sort_session_windows(destination_session);
        if source_session != destination_session && self.sessions.contains_key(&source_session) {
            self.sort_session_windows(source_session);
        }
        self.bump_generation();
        Ok(removed_panes)
    }

    pub fn swap_windows(
        &mut self,
        source: WindowId,
        target: WindowId,
        select_destination: bool,
    ) -> Result<(), ServerError> {
        if source == target {
            return Ok(());
        }
        let source_state = self
            .windows
            .get(&source)
            .ok_or_else(|| ServerError::MissingTarget(source.to_string()))?;
        let target_state = self
            .windows
            .get(&target)
            .ok_or_else(|| ServerError::MissingTarget(target.to_string()))?;
        let source_session = source_state.session;
        let source_index = source_state.index;
        let target_session = target_state.session;
        let target_index = target_state.index;

        if source_session == target_session {
            let session = self
                .sessions
                .get_mut(&source_session)
                .expect("window session exists");
            session.active_window = swap_window_id(session.active_window, source, target);
            session.last_window = session
                .last_window
                .map(|window| swap_window_id(window, source, target));
        } else {
            let source_state = self
                .sessions
                .get_mut(&source_session)
                .expect("source session exists");
            for window in &mut source_state.windows {
                if *window == source {
                    *window = target;
                }
            }
            if source_state.active_window == source {
                source_state.active_window = target;
            }
            if source_state.last_window == Some(source) {
                source_state.last_window = Some(target);
            }

            let target_state = self
                .sessions
                .get_mut(&target_session)
                .expect("target session exists");
            for window in &mut target_state.windows {
                if *window == target {
                    *window = source;
                }
            }
            if target_state.active_window == target {
                target_state.active_window = source;
            }
            if target_state.last_window == Some(target) {
                target_state.last_window = Some(source);
            }
        }

        {
            let source_state = self.windows.get_mut(&source).expect("source window exists");
            source_state.session = target_session;
            source_state.index = target_index;
        }
        {
            let target_state = self.windows.get_mut(&target).expect("target window exists");
            target_state.session = source_session;
            target_state.index = source_index;
        }
        self.sort_session_windows(source_session);
        if source_session != target_session {
            self.sort_session_windows(target_session);
        }
        if select_destination {
            self.activate_window(target_session, source);
            if source_session != target_session {
                self.activate_window(source_session, target);
            }
        }
        self.bump_generation();
        Ok(())
    }

    pub fn renumber_windows(
        &mut self,
        session: SessionId,
        base_index: u32,
    ) -> Result<(), ServerError> {
        let mut windows = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?
            .windows
            .clone();
        windows.sort_by_key(|window| self.windows.get(window).map(|window| window.index));
        validate_window_index_run(base_index, windows.len())?;

        let mut assignments = Vec::with_capacity(windows.len());
        for (position, window) in windows.iter().copied().enumerate() {
            let index = base_index
                .checked_add(u32::try_from(position).expect("validated window count fits u32"))
                .expect("validated window index run fits u32");
            assignments.push((window, index));
        }

        let changed = assignments
            .iter()
            .any(|(window, index)| self.windows[window].index != *index);
        for (window, index) in assignments {
            self.windows
                .get_mut(&window)
                .expect("renumbered window exists")
                .index = index;
        }
        self.sessions
            .get_mut(&session)
            .expect("renumbered session exists")
            .windows = windows;
        if changed {
            self.bump_generation();
        }
        Ok(())
    }

    fn sort_session_windows(&mut self, session: SessionId) {
        let Some(state) = self.sessions.get(&session) else {
            return;
        };
        let mut windows = state.windows.clone();
        windows.sort_by_key(|window| self.windows.get(window).map(|window| window.index));
        self.sessions
            .get_mut(&session)
            .expect("session was just read")
            .windows = windows;
    }

    pub fn rename_window(
        &mut self,
        window: WindowId,
        name: impl Into<String>,
    ) -> Result<(), ServerError> {
        let name = name.into();
        let window = self
            .windows
            .get_mut(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        if window.name != name {
            window.name = name;
            self.bump_generation();
        }
        Ok(())
    }

    pub fn split_pane(
        &mut self,
        target: PaneId,
        axis: Axis,
        kind: PaneKind,
    ) -> Result<PaneId, ServerError> {
        self.split_pane_with(target, axis, kind, SplitPlacement::default())
    }

    pub fn split_pane_with(
        &mut self,
        target: PaneId,
        axis: Axis,
        kind: PaneKind,
        placement: SplitPlacement,
    ) -> Result<PaneId, ServerError> {
        let window_id = self
            .window_for_pane(target)
            .ok_or_else(|| ServerError::MissingTarget(target.to_string()))?;
        let pane_id = PaneId(self.next_pane_id);
        let active_point = if placement.detached {
            0
        } else {
            self.allocate_sort_point()
        };
        let next_pane_id = &mut self.next_pane_id;
        let next_split_id = &mut self.next_split_id;
        let mut ids = || {
            let id = SplitId(*next_split_id);
            *next_split_id = (*next_split_id).saturating_add(1);
            id
        };
        let window = self.windows.get_mut(&window_id).expect("window exists");
        window
            .layout
            .split(
                target,
                axis,
                placement.size,
                placement.before,
                placement.full_size,
                pane_id,
                &mut ids,
            )
            .map_err(|error| split_layout_error(error, target))?;
        *next_pane_id = (*next_pane_id).saturating_add(1);
        window.panes.insert(
            pane_id,
            Pane {
                id: pane_id,
                title: pane_title(&kind),
                kind,
                active_point,
                bell: false,
                dead: false,
                dead_status: None,
                dead_time: None,
                input_off: false,
                empty: false,
                input_options: InputOptions::default(),
            },
        );
        insert_pane_order(
            &mut window.pane_order,
            pane_id,
            target,
            placement.before,
            placement.full_size,
        );
        if !placement.detached {
            activate_window_pane(window, pane_id, false);
        }
        self.bump_generation();
        Ok(pane_id)
    }

    pub fn kill_pane(&mut self, pane: PaneId) -> Result<Vec<PaneId>, ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let removed = self
            .windows
            .get_mut(&window_id)
            .expect("window exists")
            .layout
            .remove(pane);
        match removed {
            Ok(()) => {}
            Err(LayoutError::LastPane) => return self.kill_window(window_id),
            Err(error) => return Err(pane_layout_error(error, pane)),
        }
        let window = self.windows.get_mut(&window_id).expect("window exists");
        window.panes.remove(&pane);
        repair_window_after_pane_removal(window, pane);
        self.bump_generation();
        Ok(vec![pane])
    }

    pub fn kill_window(&mut self, window: WindowId) -> Result<Vec<PaneId>, ServerError> {
        let removed = self
            .windows
            .remove(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        let removed_panes = removed.pane_order.clone();
        let (activated, session_empty) = {
            let session = self
                .sessions
                .get_mut(&removed.session)
                .expect("window session exists");
            let activated = session.forget_window(window);
            (activated, session.windows.is_empty())
        };
        if session_empty {
            self.sessions.remove(&removed.session);
        } else if let Some(window) = activated {
            self.touch_window_activity(window);
            self.clear_window_alerts(window);
        }
        self.bump_generation();
        Ok(removed_panes)
    }

    pub fn kill_session(&mut self, session: SessionId) -> Result<Vec<PaneId>, ServerError> {
        let session = self
            .sessions
            .remove(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        let mut panes = Vec::new();
        for window in session.windows {
            if let Some(window) = self.windows.remove(&window) {
                panes.extend(window.pane_order);
            }
        }
        self.bump_generation();
        Ok(panes)
    }

    pub fn select_window(
        &mut self,
        session: SessionId,
        window: WindowId,
    ) -> Result<(), ServerError> {
        let session_state = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        if !session_state.windows.contains(&window) {
            return Err(ServerError::MissingTarget(window.to_string()));
        }
        self.activate_window(session, window);
        self.bump_generation();
        Ok(())
    }

    pub fn validate_renumber_capacity(
        &self,
        session: SessionId,
        base_index: u32,
        removed_windows: usize,
    ) -> Result<(), ServerError> {
        let count = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?
            .windows
            .len()
            .checked_sub(removed_windows)
            .ok_or_else(|| ServerError::Internal("invalid removed window count".to_owned()))?;
        validate_window_index_run(base_index, count)
    }

    fn activate_window(&mut self, session: SessionId, window: WindowId) -> bool {
        let changed = self
            .sessions
            .get_mut(&session)
            .expect("session exists")
            .activate_window(window);
        if changed {
            self.touch_window_activity(window);
            self.clear_window_alerts(window);
        }
        changed
    }

    fn force_activate_window(&mut self, session: SessionId, window: WindowId) {
        self.sessions
            .get_mut(&session)
            .expect("session exists")
            .activate_window(window);
        self.touch_window_activity(window);
        self.clear_window_alerts(window);
    }

    fn touch_window_activity(&mut self, window: WindowId) {
        let activity = self.allocate_sort_point();
        self.windows
            .get_mut(&window)
            .expect("active window exists")
            .activity = activity;
    }

    fn clear_window_alerts(&mut self, window: WindowId) {
        let panes = self.windows[&window].pane_order.clone();
        for pane in panes {
            self.set_pane_bell(pane, false);
        }
        self.set_window_activity_flag(window, false);
        self.set_window_silence_flag(window, false);
    }

    /// Set or clear a window's pin-`WINLINK_ACTIVITY` flag, reporting whether
    /// it moved. A window that already left is not an error.
    pub fn set_window_activity_flag(&mut self, window: WindowId, raised: bool) -> bool {
        let Some(window_state) = self.windows.get_mut(&window) else {
            return false;
        };
        if window_state.activity_flag == raised {
            return false;
        }
        window_state.activity_flag = raised;
        self.bump_generation();
        true
    }

    /// Set or clear a window's pin-`WINLINK_SILENCE` flag, reporting whether
    /// it moved. A window that already left is not an error.
    pub fn set_window_silence_flag(&mut self, window: WindowId, raised: bool) -> bool {
        let Some(window_state) = self.windows.get_mut(&window) else {
            return false;
        };
        if window_state.silence_flag == raised {
            return false;
        }
        window_state.silence_flag = raised;
        self.bump_generation();
        true
    }

    pub fn select_pane(&mut self, pane: PaneId) -> Result<(), ServerError> {
        self.select_pane_with_zoom(pane, false).map(|_| ())
    }

    pub fn select_pane_with_zoom(
        &mut self,
        pane: PaneId,
        preserve_zoom: bool,
    ) -> Result<bool, ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let active_point = self.allocate_sort_point();
        let window = self.windows.get_mut(&window_id).expect("window exists");
        if window.active_pane == pane && window.zoomed_pane.is_some() && !preserve_zoom {
            window.zoomed_pane = None;
            self.bump_generation();
            return Ok(false);
        }
        let pane_changed = activate_window_pane(window, pane, preserve_zoom);
        if pane_changed {
            window
                .panes
                .get_mut(&pane)
                .expect("selected pane exists")
                .active_point = active_point;
            self.bump_generation();
        }
        Ok(pane_changed)
    }

    pub fn toggle_zoom(&mut self, pane: PaneId) -> Result<(), ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let active_point = self.allocate_sort_point();
        let window = self.windows.get_mut(&window_id).expect("window exists");
        if window.panes.len() <= 1 {
            return Ok(());
        }
        if window.zoomed_pane.is_some() {
            window.zoomed_pane = None;
        } else {
            if activate_window_pane(window, pane, false) {
                window
                    .panes
                    .get_mut(&pane)
                    .expect("selected pane exists")
                    .active_point = active_point;
            }
            window.zoomed_pane = Some(pane);
        }
        self.bump_generation();
        Ok(())
    }

    pub fn touch_window_activity_for_pane(&mut self, pane: PaneId) {
        let Some(window) = self.window_for_pane(pane) else {
            return;
        };
        self.touch_window_activity(window);
    }

    fn touch_pane_active_point(&mut self, pane: PaneId) {
        let Some(window) = self.window_for_pane(pane) else {
            return;
        };
        let active_point = self.allocate_sort_point();
        if let Some(pane) = self
            .windows
            .get_mut(&window)
            .and_then(|window| window.panes.get_mut(&pane))
        {
            pane.active_point = active_point;
        }
    }

    /// Move `pane`'s resize boundary by terminal cells, positive toward the
    /// right or bottom.
    pub fn resize_pane(
        &mut self,
        pane: PaneId,
        axis: Axis,
        delta_cells: i32,
    ) -> Result<(), ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let window = self.windows.get_mut(&window_id).expect("window exists");
        window
            .layout
            .resize_pane(pane, axis, delta_cells)
            .map_err(|error| pane_layout_error(error, pane))?;
        window.zoomed_pane = None;
        self.bump_generation();
        Ok(())
    }

    /// Give `pane` an absolute cell size along `axis`.
    pub fn resize_pane_to(
        &mut self,
        pane: PaneId,
        axis: Axis,
        cells: u16,
    ) -> Result<(), ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let window = self.windows.get_mut(&window_id).expect("window exists");
        window
            .layout
            .resize_pane_to(pane, axis, cells)
            .map_err(|error| pane_layout_error(error, pane))?;
        window.zoomed_pane = None;
        self.bump_generation();
        Ok(())
    }

    pub(crate) fn resize_window(
        &mut self,
        window: WindowId,
        columns: u16,
        rows: u16,
    ) -> Result<(), ServerError> {
        let window = self
            .windows
            .get_mut(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        let before = window.layout.extent();
        window.layout.resize(columns, rows);
        window.last_extent_probe = None;
        if window.layout.extent() != before {
            self.bump_generation();
        }
        Ok(())
    }

    /// Sets one exact split ratio and returns whether the layout changed.
    pub fn resize_split(
        &mut self,
        window: WindowId,
        split: SplitId,
        ratio: f32,
    ) -> Result<bool, ServerError> {
        if !ratio.is_finite() {
            return Err(ServerError::InvalidCommand(
                "split ratio must be finite".to_owned(),
            ));
        }
        let window = self
            .windows
            .get_mut(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        let changed = window
            .layout
            .set_divider_ratio(split, ratio)
            .map_err(|error| divider_layout_error(error, split))?;
        if changed {
            window.zoomed_pane = None;
            self.bump_generation();
        }
        Ok(changed)
    }

    pub fn select_layout(
        &mut self,
        window: WindowId,
        preset: LayoutPreset,
        options: &PresetOptions,
    ) -> Result<(), ServerError> {
        let panes = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
            .pane_order
            .clone();
        debug_assert!(!panes.is_empty(), "validated windows are never empty");
        let split_ids = (0..panes.len().saturating_sub(1))
            .map(|_| self.allocate_split_id())
            .collect::<Vec<_>>();
        let mut split_ids = split_ids.into_iter();
        let mut ids = || split_ids.next().expect("preset has one split ID per edge");

        let window = self.windows.get_mut(&window).expect("window was resolved");
        let previous = window.layout.clone();
        window
            .layout
            .apply_preset(preset, &panes, options, &mut ids);
        let split_ids_exhausted = split_ids.next().is_none();
        debug_assert!(split_ids_exhausted, "preset consumes one split ID per edge");
        window.previous_layout = Some(Box::new(previous));
        window.last_layout = Some(preset);
        self.bump_generation();
        Ok(())
    }

    pub fn select_layout_string(
        &mut self,
        window: WindowId,
        layout: &str,
    ) -> Result<(), ServerError> {
        let panes = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
            .pane_order
            .clone();
        let mut parsed = CellLayout::parse(layout)
            .map_err(|error| ServerError::InvalidCommand(format!("{}: {layout}", error.cause())))?;
        let cells = parsed.pane_count();
        if panes.len() > cells {
            return Err(ServerError::InvalidCommand(format!(
                "have {} panes but need {cells}: {layout}",
                panes.len()
            )));
        }
        while parsed.pane_count() > panes.len() {
            parsed.trim_bottom_right();
        }
        let split_ids = (0..panes.len().saturating_sub(1))
            .map(|_| self.allocate_split_id())
            .collect::<Vec<_>>();
        let mut split_ids = split_ids.into_iter();
        let mut ids = || {
            split_ids
                .next()
                .expect("parsed layout has one split ID per edge")
        };
        let next = parsed.into_layout(&panes, &mut ids);
        let split_ids_exhausted = split_ids.next().is_none();
        debug_assert!(
            split_ids_exhausted,
            "parsed layout consumes one fresh ID per split"
        );

        let window = self.windows.get_mut(&window).expect("window was resolved");
        let previous = std::mem::replace(&mut window.layout, next);
        window.previous_layout = Some(Box::new(previous));
        window.last_extent_probe = None;
        self.bump_generation();
        Ok(())
    }

    pub fn cycle_layout(
        &mut self,
        window: WindowId,
        offset: isize,
        options: &PresetOptions,
    ) -> Result<LayoutPreset, ServerError> {
        let last = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
            .last_layout;
        let preset = match last {
            Some(last) => last.at_offset(offset),
            None if offset < 0 => LayoutPreset::Tiled,
            None => LayoutPreset::EvenHorizontal,
        };
        self.select_layout(window, preset, options)?;
        Ok(preset)
    }

    pub fn restore_previous_layout(&mut self, window: WindowId) -> Result<(), ServerError> {
        let has_previous = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
            .previous_layout
            .is_some();
        if !has_previous {
            let window = self.windows.get_mut(&window).expect("window was resolved");
            window.previous_layout = Some(Box::new(window.layout.clone()));
            return Ok(());
        }
        let (pane_order, split_count) = {
            let window_state = self
                .windows
                .get(&window)
                .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
            let previous = window_state
                .previous_layout
                .as_deref()
                .expect("previous layout was checked");
            if previous.pane_count() != window_state.pane_order.len() {
                return Err(ServerError::InvalidCommand(format!(
                    "window {window} previous layout no longer matches its panes"
                )));
            }
            (
                window_state.pane_order.clone(),
                window_state.pane_order.len().saturating_sub(1),
            )
        };
        let split_ids = (0..split_count)
            .map(|_| self.allocate_split_id())
            .collect::<Vec<_>>();

        let window = self.windows.get_mut(&window).expect("window was resolved");
        let mut restored = *window
            .previous_layout
            .take()
            .expect("previous layout was validated");
        let replaced = restored.replace_panes_in_order(&pane_order);
        debug_assert!(replaced);
        let mut split_ids = split_ids.into_iter();
        let mut ids = || {
            split_ids
                .next()
                .expect("restored layout has one ID per edge")
        };
        restored.refresh_divider_ids(&mut ids);
        let split_ids_exhausted = split_ids.next().is_none();
        debug_assert!(
            split_ids_exhausted,
            "restored layout consumes one fresh ID per split"
        );
        let current = std::mem::replace(&mut window.layout, restored);
        window.previous_layout = Some(Box::new(current));
        window.zoomed_pane = None;
        self.bump_generation();
        Ok(())
    }

    pub fn spread_layout(&mut self, pane: PaneId) -> Result<(), ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let window = self
            .windows
            .get_mut(&window_id)
            .expect("pane window exists");
        let previous = window.layout.clone();
        window
            .layout
            .spread(pane)
            .map_err(|error| pane_layout_error(error, pane))?;
        window.previous_layout = Some(Box::new(previous));
        self.bump_generation();
        Ok(())
    }

    pub fn last_layout(&self, window: WindowId) -> Result<Option<LayoutPreset>, ServerError> {
        self.windows
            .get(&window)
            .map(|window| window.last_layout)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))
    }

    pub fn update_pane_title(
        &mut self,
        pane: PaneId,
        title: impl Into<String>,
    ) -> Result<bool, ServerError> {
        let title = title.into();
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if pane_state.title == title {
            return Ok(false);
        }
        pane_state.title = title;
        self.bump_generation();
        Ok(true)
    }

    /// Set or clear a pane's pending bell, reporting whether it moved. A pane
    /// that already left is not an error: nothing changed either way.
    pub fn set_pane_bell(&mut self, pane: PaneId, bell: bool) -> bool {
        let Some(pane_state) = self.pane_mut(pane) else {
            return false;
        };
        if pane_state.bell == bell {
            return false;
        }
        pane_state.bell = bell;
        self.bump_generation();
        true
    }

    /// Replace a pending picker with its selected runtime kind, keeping the pane
    /// ID and layout position. Returns the terminal to inherit a directory from:
    /// the donor captured at split time if still live, otherwise a fresh resolve.
    pub fn materialize_pane(
        &mut self,
        pane: PaneId,
        kind: PaneKind,
    ) -> Result<Option<PaneId>, ServerError> {
        if matches!(kind, PaneKind::Picker { .. }) {
            return Err(ServerError::InvalidCommand(
                "a pane picker cannot materialize another picker".to_owned(),
            ));
        }
        let inherit_cwd_from = match &self
            .pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?
            .kind
        {
            PaneKind::Picker { inherit_cwd_from } => *inherit_cwd_from,
            _ => {
                return Err(ServerError::InvalidCommand(format!(
                    "pane {pane} is not awaiting a type selection"
                )));
            }
        };
        let inherit_cwd_from = inherit_cwd_from
            .filter(|donor| self.cwd_donor(*donor) == Some(*donor))
            .or_else(|| self.cwd_donor(pane));
        let title = pane_title(&kind);
        let pane_state = self
            .pane_mut(pane)
            .expect("the validated picker pane still exists");
        pane_state.title = title;
        pane_state.kind = kind;
        self.bump_generation();
        Ok(inherit_cwd_from)
    }

    pub fn update_browser_url(&mut self, pane: PaneId, url: String) -> Result<(), ServerError> {
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let PaneKind::Browser(browser) = &mut pane_state.kind else {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not a browser"
            )));
        };
        let index = browser.active_tab.min(browser.tabs.len().saturating_sub(1));
        match browser.tabs.get_mut(index) {
            Some(active) if *active == url => return Ok(()),
            Some(active) => *active = url,
            None => browser.tabs.push(url),
        }
        self.bump_generation();
        Ok(())
    }

    pub fn update_browser_tabs(
        &mut self,
        pane: PaneId,
        tabs: Vec<String>,
        active_tab: usize,
    ) -> Result<(), ServerError> {
        if tabs.is_empty() {
            return Err(ServerError::InvalidCommand(
                "a browser pane needs at least one tab".to_owned(),
            ));
        }
        if active_tab >= tabs.len() {
            return Err(ServerError::InvalidCommand(format!(
                "active tab {active_tab} is out of range for {} tabs",
                tabs.len()
            )));
        }
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let PaneKind::Browser(browser) = &mut pane_state.kind else {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not a browser"
            )));
        };
        if browser.tabs == tabs && browser.active_tab == active_tab {
            return Ok(());
        }
        browser.tabs = tabs;
        browser.active_tab = active_tab;
        self.bump_generation();
        Ok(())
    }

    pub fn update_agent_cwd(
        &mut self,
        pane: PaneId,
        cwd: Option<PathBuf>,
    ) -> Result<(), ServerError> {
        if cwd.as_ref().is_some_and(|cwd| {
            !cwd.is_absolute() || cwd.as_os_str().as_encoded_bytes().len() > MAX_GUI_TEXT_BYTES
        }) {
            return Err(ServerError::InvalidCommand(
                "agent working directory must be absolute and stay inside the wire limit"
                    .to_owned(),
            ));
        }
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let PaneKind::Agent(agent) = &mut pane_state.kind else {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not an agent"
            )));
        };
        if agent.cwd == cwd {
            return Ok(());
        }
        agent.cwd = cwd;
        self.bump_generation();
        Ok(())
    }

    pub fn update_agent_session(
        &mut self,
        pane: PaneId,
        session_id: String,
        cwd: Option<PathBuf>,
    ) -> Result<(), ServerError> {
        if session_id.is_empty()
            || session_id.len() > MAX_AGENT_SESSION_ID_BYTES
            || session_id.chars().any(char::is_control)
        {
            return Err(ServerError::InvalidCommand(format!(
                "agent session ID must be 1..={MAX_AGENT_SESSION_ID_BYTES} non-control bytes"
            )));
        }
        if cwd.as_ref().is_some_and(|cwd| {
            !cwd.is_absolute() || cwd.as_os_str().as_encoded_bytes().len() > MAX_GUI_TEXT_BYTES
        }) {
            return Err(ServerError::InvalidCommand(
                "agent working directory must be absolute and stay inside the wire limit"
                    .to_owned(),
            ));
        }
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let PaneKind::Agent(agent) = &mut pane_state.kind else {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not an agent"
            )));
        };
        let cwd_changed = cwd
            .as_ref()
            .is_some_and(|cwd| agent.cwd.as_ref() != Some(cwd));
        if agent.session_id.as_deref() == Some(&session_id) && !cwd_changed {
            return Ok(());
        }
        agent.session_id = Some(session_id);
        if let Some(cwd) = cwd {
            agent.cwd = Some(cwd);
        }
        self.bump_generation();
        Ok(())
    }

    pub fn update_agent_provider(
        &mut self,
        pane: PaneId,
        provider: AgentProvider,
    ) -> Result<bool, ServerError> {
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let PaneKind::Agent(agent) = &mut pane_state.kind else {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not an agent"
            )));
        };
        if agent.provider == provider {
            return Ok(false);
        }
        agent.provider = provider;
        agent.session_id = None;
        self.bump_generation();
        Ok(true)
    }

    pub fn update_editor_cwd(&mut self, pane: PaneId, cwd: String) -> Result<(), ServerError> {
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let PaneKind::Editor(editor) = &mut pane_state.kind else {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not an editor"
            )));
        };
        let next = EditorDescriptor {
            path: editor.path.clone(),
            cwd,
        };
        next.validate()
            .map_err(|error| ServerError::InvalidCommand(error.to_string()))?;
        if *editor == next {
            return Ok(());
        }
        *editor = next;
        self.bump_generation();
        Ok(())
    }

    pub fn update_editor_path(
        &mut self,
        pane: PaneId,
        path: Option<String>,
    ) -> Result<(), ServerError> {
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let PaneKind::Editor(editor) = &mut pane_state.kind else {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not an editor"
            )));
        };
        let next = EditorDescriptor {
            path,
            cwd: editor.cwd.clone(),
        };
        next.validate()
            .map_err(|error| ServerError::InvalidCommand(error.to_string()))?;
        if *editor == next {
            return Ok(());
        }
        *editor = next;
        self.bump_generation();
        Ok(())
    }

    pub fn update_browser_profile(
        &mut self,
        pane: PaneId,
        profile: &str,
    ) -> Result<(), ServerError> {
        let profile = normalize_browser_profile_name(profile)
            .map_err(|error| ServerError::InvalidCommand(error.to_string()))?;
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let PaneKind::Browser(browser) = &mut pane_state.kind else {
            return Err(ServerError::InvalidCommand(format!(
                "pane {pane} is not a browser"
            )));
        };
        if browser.profile == profile {
            return Ok(());
        }
        browser.profile = profile;
        browser.tabs = vec![browser.url().to_owned()];
        browser.active_tab = 0;
        self.bump_generation();
        Ok(())
    }

    #[must_use]
    pub fn global_synchronize_panes(&self) -> bool {
        self.input_options.synchronize_panes().unwrap_or(false)
    }

    #[must_use]
    pub fn global_automatic_rename(&self) -> bool {
        self.input_options.automatic_rename().unwrap_or(true)
    }

    pub fn set_global_automatic_rename(&mut self, value: Option<bool>) {
        if self.input_options.automatic_rename() != value {
            self.input_options.set_automatic_rename(value);
            self.bump_generation();
        }
    }

    #[must_use]
    pub fn global_aggressive_resize(&self) -> bool {
        self.input_options.aggressive_resize().unwrap_or(false)
    }

    pub fn set_global_aggressive_resize(&mut self, value: Option<bool>) {
        if self.input_options.aggressive_resize() != value {
            self.input_options.set_aggressive_resize(value);
            self.bump_generation();
        }
    }

    pub fn window_aggressive_resize(&self, window: WindowId) -> Result<bool, ServerError> {
        let window = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        Ok(window
            .input_options
            .aggressive_resize()
            .unwrap_or_else(|| self.global_aggressive_resize()))
    }

    pub fn window_aggressive_resize_override(
        &self,
        window: WindowId,
    ) -> Result<Option<bool>, ServerError> {
        self.windows
            .get(&window)
            .map(|window| window.input_options.aggressive_resize())
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))
    }

    pub fn set_window_aggressive_resize(
        &mut self,
        window: WindowId,
        value: Option<bool>,
    ) -> Result<(), ServerError> {
        let window = self
            .windows
            .get_mut(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        if window.input_options.aggressive_resize() != value {
            window.input_options.set_aggressive_resize(value);
            self.bump_generation();
        }
        Ok(())
    }

    pub fn window_automatic_rename(&self, window: WindowId) -> Result<bool, ServerError> {
        let window = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        Ok(window
            .input_options
            .automatic_rename()
            .unwrap_or_else(|| self.global_automatic_rename()))
    }

    pub fn window_automatic_rename_override(
        &self,
        window: WindowId,
    ) -> Result<Option<bool>, ServerError> {
        self.windows
            .get(&window)
            .map(|window| window.input_options.automatic_rename())
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))
    }

    pub fn set_window_automatic_rename(
        &mut self,
        window: WindowId,
        value: Option<bool>,
    ) -> Result<(), ServerError> {
        let window = self
            .windows
            .get_mut(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        if window.input_options.automatic_rename() != value {
            window.input_options.set_automatic_rename(value);
            self.bump_generation();
        }
        Ok(())
    }

    pub fn set_global_synchronize_panes(&mut self, value: bool) {
        if self.global_synchronize_panes() != value {
            self.input_options.set_synchronize_panes(Some(value));
            self.bump_generation();
        }
    }

    pub fn window_synchronize_panes(&self, window: WindowId) -> Result<bool, ServerError> {
        let window = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        Ok(window
            .input_options
            .synchronize_panes()
            .unwrap_or_else(|| self.global_synchronize_panes()))
    }

    pub fn window_synchronize_override(
        &self,
        window: WindowId,
    ) -> Result<Option<bool>, ServerError> {
        self.windows
            .get(&window)
            .map(|window| window.input_options.synchronize_panes())
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))
    }

    pub fn set_window_synchronize_panes(
        &mut self,
        window: WindowId,
        value: Option<bool>,
    ) -> Result<(), ServerError> {
        let window = self
            .windows
            .get_mut(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        if window.input_options.synchronize_panes() != value {
            window.input_options.set_synchronize_panes(value);
            self.bump_generation();
        }
        Ok(())
    }

    pub fn pane_synchronize_panes(&self, pane: PaneId) -> Result<bool, ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let window = &self.windows[&window_id];
        let pane = &window.panes[&pane];
        Ok(pane
            .input_options
            .synchronize_panes()
            .unwrap_or(self.window_synchronize_panes(window_id)?))
    }

    pub fn pane_synchronize_override(&self, pane: PaneId) -> Result<Option<bool>, ServerError> {
        self.pane(pane)
            .map(|pane| pane.input_options.synchronize_panes())
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))
    }

    pub fn set_pane_synchronize_panes(
        &mut self,
        pane: PaneId,
        value: Option<bool>,
    ) -> Result<(), ServerError> {
        let pane_state = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if pane_state.input_options.synchronize_panes() != value {
            pane_state.input_options.set_synchronize_panes(value);
            self.bump_generation();
        }
        Ok(())
    }

    pub fn clear_pane_synchronize_overrides(
        &mut self,
        window: WindowId,
    ) -> Result<(), ServerError> {
        let window = self
            .windows
            .get_mut(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        let mut changed = false;
        for pane in window.panes.values_mut() {
            if pane.input_options.synchronize_panes().is_some() {
                pane.input_options.set_synchronize_panes(None);
                changed = true;
            }
        }
        if changed {
            self.bump_generation();
        }
        Ok(())
    }

    pub fn synchronized_input_targets(&self, source: PaneId) -> Result<Vec<PaneId>, ServerError> {
        let window_id = self
            .window_for_pane(source)
            .ok_or_else(|| ServerError::MissingTarget(source.to_string()))?;
        let window = &self.windows[&window_id];
        let synchronized = window
            .input_options
            .synchronize_panes()
            .unwrap_or_else(|| self.global_synchronize_panes());
        let source_synchronized = window.panes[&source]
            .input_options
            .synchronize_panes()
            .unwrap_or(synchronized);
        if !source_synchronized {
            return Ok(vec![source]);
        }
        Ok(window
            .panes
            .iter()
            .filter_map(|(id, pane)| {
                pane.input_options
                    .synchronize_panes()
                    .unwrap_or(synchronized)
                    .then_some(*id)
            })
            .collect())
    }

    /// The last-resort context every recovery path falls back to. A session
    /// whose `active_window` dangles does not end the search.
    #[must_use]
    pub fn default_context(&self) -> Option<(SessionId, WindowId, PaneId)> {
        self.sessions.values().find_map(|session| {
            let window = self.windows.get(&session.active_window)?;
            Some((session.id, window.id, window.active_pane))
        })
    }

    /// Sessions in tmux's listing order: the pin keeps them in an RB tree
    /// keyed by strcmp on the name, so every session enumeration a script can
    /// observe is name-sorted, not creation-sorted.
    #[must_use]
    pub fn sessions_by_name(&self) -> Vec<&Session> {
        let mut sessions = self.sessions.values().collect::<Vec<_>>();
        sessions.sort_by(|left, right| left.name.as_bytes().cmp(right.name.as_bytes()));
        sessions
    }

    #[must_use]
    pub fn most_recent_context(&self) -> Option<(SessionId, WindowId, PaneId)> {
        self.last_active_session
            .and_then(|session| self.sessions.get(&session))
            .and_then(|session| {
                let window = self.windows.get(&session.active_window)?;
                Some((session.id, window.id, window.active_pane))
            })
            .or_else(|| self.default_context())
    }

    pub(crate) fn mark_session_active(&mut self, session: SessionId) {
        if self.sessions.contains_key(&session) {
            self.touch_session_activity(session);
            self.last_active_session = Some(session);
        }
    }

    pub(crate) fn touch_session_activity(&mut self, session: SessionId) {
        if self.sessions.contains_key(&session) {
            let activity = self.allocate_sort_point();
            self.sessions
                .get_mut(&session)
                .expect("active session exists")
                .sort_activity = activity;
        }
    }

    pub fn resolve_session(
        &self,
        target: Option<&str>,
        current: Option<SessionId>,
    ) -> Result<SessionId, ServerError> {
        let Some(target) = target.filter(|target| !target.is_empty()) else {
            return current
                .filter(|session| self.sessions.contains_key(session))
                .or_else(|| self.sessions.keys().next().copied())
                .ok_or_else(|| ServerError::SessionNotFound("current session".to_owned()));
        };
        let (target, exact) = target
            .strip_prefix('=')
            .map_or((target, false), |target| (target, true));
        if target.starts_with('$') {
            let id = target
                .parse::<SessionId>()
                .map_err(|_| ServerError::SessionNotFound(target.to_owned()))?;
            return self
                .sessions
                .contains_key(&id)
                .then_some(id)
                .ok_or_else(|| ServerError::SessionNotFound(target.to_owned()));
        }
        if let Some(session) = self
            .sessions
            .values()
            .find(|session| session.name == target)
            .map(|session| session.id)
        {
            return Ok(session);
        }
        if exact {
            return Err(ServerError::SessionNotFound(target.to_owned()));
        }
        match unique_candidate(
            self.sessions
                .values()
                .filter(|session| session.name.starts_with(target))
                .map(|session| session.id),
        ) {
            Ok(Some(session)) => return Ok(session),
            Err(()) => return Err(ServerError::SessionNotFound(target.to_owned())),
            Ok(None) => {}
        }
        match unique_candidate(
            self.sessions
                .values()
                .filter(|session| fnmatch(target, &session.name))
                .map(|session| session.id),
        ) {
            Ok(Some(session)) => Ok(session),
            Ok(None) | Err(()) => Err(ServerError::SessionNotFound(target.to_owned())),
        }
    }

    pub fn resolve_window(
        &self,
        target: Option<&str>,
        current_session: Option<SessionId>,
        current_window: Option<WindowId>,
    ) -> Result<WindowId, ServerError> {
        self.resolve_window_with_pane_index(
            target,
            current_session,
            current_window,
            &|window, index| {
                let index = usize::try_from(index).ok()?;
                self.windows.get(&window)?.pane_order().get(index).copied()
            },
        )
    }

    pub(crate) fn resolve_window_with_pane_index(
        &self,
        target: Option<&str>,
        current_session: Option<SessionId>,
        current_window: Option<WindowId>,
        pane_at_index: &impl Fn(WindowId, u32) -> Option<PaneId>,
    ) -> Result<WindowId, ServerError> {
        if let Some(target) = target
            && target.starts_with('%')
        {
            let pane =
                self.resolve_pane_with_index(Some(target), current_window, None, pane_at_index)?;
            return self
                .window_for_pane(pane)
                .ok_or_else(|| ServerError::PaneNotFound(target.to_owned()));
        }
        match self.resolve_window_core(target, current_session, current_window) {
            Ok(window) => Ok(window),
            Err(error) => {
                let Some(target) = target else {
                    return Err(error);
                };
                let window_target = target.split_once(':').map_or(target, |(_, window)| window);
                if !window_target.contains('.') {
                    return Err(error);
                }
                let pane = self.resolve_pane_with_index(
                    Some(target),
                    current_window,
                    None,
                    pane_at_index,
                )?;
                self.window_for_pane(pane)
                    .ok_or_else(|| ServerError::PaneNotFound(target.to_owned()))
            }
        }
    }

    fn resolve_window_core(
        &self,
        target: Option<&str>,
        current_session: Option<SessionId>,
        current_window: Option<WindowId>,
    ) -> Result<WindowId, ServerError> {
        self.resolve_window_target_core(target, current_session, current_window, false)?
            .window
            .ok_or_else(|| ServerError::WindowNotFound(target.unwrap_or_default().to_owned()))
    }

    pub(crate) fn resolve_window_index_target(
        &self,
        target: Option<&str>,
        current_session: Option<SessionId>,
        current_window: Option<WindowId>,
    ) -> Result<(SessionId, Option<u32>), ServerError> {
        let resolved =
            self.resolve_window_target_core(target, current_session, current_window, true)?;
        Ok((resolved.session, resolved.index))
    }

    fn resolve_window_target_core(
        &self,
        target: Option<&str>,
        current_session: Option<SessionId>,
        current_window: Option<WindowId>,
        index_mode: bool,
    ) -> Result<WindowTargetResolution, ServerError> {
        let current_session = current_session
            .filter(|session| self.sessions.contains_key(session))
            .or_else(|| {
                current_window
                    .and_then(|window| self.windows.get(&window).map(|window| window.session))
            });
        let Some(target) = target.filter(|target| !target.is_empty()) else {
            let window = current_window.filter(|window| self.windows.contains_key(window));
            let session = window
                .map(|window| self.windows[&window].session)
                .map_or_else(|| self.resolve_session(None, current_session), Ok)?;
            let window = window.unwrap_or(self.sessions[&session].active_window);
            return Ok(WindowTargetResolution {
                session,
                window: Some(window),
                index: (!index_mode).then_some(self.windows[&window].index),
            });
        };
        if target.starts_with('$') && !target.contains([':', '.']) {
            let session = self.resolve_session(Some(target), current_session)?;
            return Ok(WindowTargetResolution {
                session,
                window: Some(self.sessions[&session].active_window),
                index: None,
            });
        }
        if target.starts_with('@') {
            let window = target
                .parse::<WindowId>()
                .ok()
                .filter(|window| self.windows.contains_key(window))
                .ok_or_else(|| ServerError::WindowNotFound(target.to_owned()))?;
            let state = &self.windows[&window];
            return Ok(WindowTargetResolution {
                session: state.session,
                window: Some(window),
                index: Some(state.index),
            });
        }
        let (session_target, window_target) = target
            .split_once(':')
            .map_or((None, target), |(session, window)| (Some(session), window));
        let session = match session_target {
            Some("") | None => self.resolve_session(None, current_session)?,
            Some(session) => self.resolve_session(Some(session), current_session)?,
        };
        if window_target.is_empty() {
            return Ok(WindowTargetResolution {
                session,
                window: Some(self.sessions[&session].active_window),
                index: None,
            });
        }
        let window_error = match self.resolve_window_in_session(window_target, session, index_mode)
        {
            Ok(window) => {
                return Ok(WindowTargetResolution {
                    session,
                    window: window.window,
                    index: Some(window.index),
                });
            }
            Err(error) => error,
        };
        if session_target.is_some() {
            return Err(window_error);
        }
        let (fallback, _) = normalize_window_target(window_target);
        if let Ok(session) = self.resolve_session(Some(fallback), current_session) {
            return Ok(WindowTargetResolution {
                session,
                window: Some(self.sessions[&session].active_window),
                index: None,
            });
        }
        Err(window_error)
    }

    fn resolve_window_in_session(
        &self,
        target: &str,
        session: SessionId,
        index_mode: bool,
    ) -> Result<WindowInSessionResolution, ServerError> {
        let state = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::SessionNotFound(session.to_string()))?;
        let (target, exact) = normalize_window_target(target);
        let not_found = || ServerError::WindowNotFound(target.to_owned());
        if target.starts_with('@') {
            let window = target
                .parse::<WindowId>()
                .ok()
                .filter(|window| {
                    self.windows
                        .get(window)
                        .is_some_and(|window| window.session == session)
                })
                .ok_or_else(not_found)?;
            return Ok(WindowInSessionResolution {
                window: Some(window),
                index: self.windows[&window].index,
            });
        }
        if !exact && let Some((forward, offset)) = parse_offset(target) {
            if index_mode {
                let current = self.windows[&state.active_window].index;
                let index = if forward {
                    current.checked_add(offset)
                } else {
                    current.checked_sub(offset)
                }
                .filter(|index| *index <= MAX_WINDOW_INDEX)
                .ok_or_else(not_found)?;
                return Ok(WindowInSessionResolution {
                    window: self.window_at_index(session, index),
                    index,
                });
            }
            let current = state
                .windows
                .iter()
                .position(|window| *window == state.active_window)
                .ok_or_else(not_found)?;
            let count = state.windows.len();
            if count == 0 {
                return Err(not_found());
            }
            let offset = usize::try_from(offset).map_err(|_| not_found())? % count;
            let position = if forward {
                (current + offset) % count
            } else {
                (current + count - offset) % count
            };
            let window = state.windows[position];
            return Ok(WindowInSessionResolution {
                window: Some(window),
                index: self.windows[&window].index,
            });
        }
        if !exact {
            let special = match target {
                "!" => state.last_window(),
                "^" => state.windows.first().copied(),
                "$" => state.windows.last().copied(),
                _ => None,
            };
            if matches!(target, "!" | "^" | "$") {
                let window = special.ok_or_else(not_found)?;
                return Ok(WindowInSessionResolution {
                    window: Some(window),
                    index: self.windows[&window].index,
                });
            }
        }
        if let Ok(index) = target.parse::<u32>()
            && index <= MAX_WINDOW_INDEX
        {
            let window = self.window_at_index(session, index);
            if window.is_some() || index_mode {
                return Ok(WindowInSessionResolution { window, index });
            }
        }
        match unique_candidate(state.windows.iter().copied().filter(|window| {
            self.windows
                .get(window)
                .is_some_and(|window| window.name == target)
        })) {
            Ok(Some(window)) => {
                return Ok(WindowInSessionResolution {
                    window: Some(window),
                    index: self.windows[&window].index,
                });
            }
            Err(()) => return Err(not_found()),
            Ok(None) => {}
        }
        if exact {
            return Err(not_found());
        }
        match unique_candidate(state.windows.iter().copied().filter(|window| {
            self.windows
                .get(window)
                .is_some_and(|window| window.name.starts_with(target))
        })) {
            Ok(Some(window)) => {
                return Ok(WindowInSessionResolution {
                    window: Some(window),
                    index: self.windows[&window].index,
                });
            }
            Err(()) => return Err(not_found()),
            Ok(None) => {}
        }
        match unique_candidate(state.windows.iter().copied().filter(|window| {
            self.windows
                .get(window)
                .is_some_and(|window| fnmatch(target, &window.name))
        })) {
            Ok(Some(window)) => Ok(WindowInSessionResolution {
                window: Some(window),
                index: self.windows[&window].index,
            }),
            Ok(None) | Err(()) => Err(not_found()),
        }
    }

    pub fn resolve_pane(
        &self,
        target: Option<&str>,
        current_window: Option<WindowId>,
        current_pane: Option<PaneId>,
    ) -> Result<PaneId, ServerError> {
        self.resolve_pane_with_index(target, current_window, current_pane, &|window, index| {
            let index = usize::try_from(index).ok()?;
            self.windows.get(&window)?.pane_order().get(index).copied()
        })
    }

    pub(crate) fn resolve_pane_with_index(
        &self,
        target: Option<&str>,
        current_window: Option<WindowId>,
        current_pane: Option<PaneId>,
        pane_at_index: &impl Fn(WindowId, u32) -> Option<PaneId>,
    ) -> Result<PaneId, ServerError> {
        let Some(target) = target.filter(|target| !target.is_empty()) else {
            if let Some(pane) = current_pane
                && self.window_for_pane(pane).is_some()
            {
                return Ok(pane);
            }
            let window = current_window
                .and_then(|window| self.windows.get(&window))
                .or_else(|| {
                    self.default_context()
                        .and_then(|(_, window, _)| self.windows.get(&window))
                })
                .ok_or_else(|| ServerError::PaneNotFound("current pane".to_owned()))?;
            return Ok(window.active_pane);
        };
        if target.starts_with('%') {
            let id = target
                .parse::<PaneId>()
                .map_err(|_| ServerError::PaneNotFound(target.to_owned()))?;
            return self
                .window_for_pane(id)
                .map(|_| id)
                .ok_or_else(|| ServerError::PaneNotFound(target.to_owned()));
        }

        let current_window = current_pane
            .and_then(|pane| self.window_for_pane(pane))
            .or_else(|| current_window.filter(|window| self.windows.contains_key(window)));
        let current_session = current_window
            .and_then(|window| self.windows.get(&window).map(|window| window.session));
        let pane_target = normalize_pane_target(target);
        let pane_error = ServerError::PaneNotFound(pane_target.to_owned());

        let Some((window_target, pane_target)) = target.split_once('.') else {
            if !target.contains(':') {
                let window = self.resolve_window_core(None, current_session, current_window)?;
                if let Ok(pane) = self.resolve_pane_in_window(target, window, pane_at_index) {
                    return Ok(pane);
                }
            }
            let window =
                match self.resolve_window_core(Some(target), current_session, current_window) {
                    Ok(window) => window,
                    Err(_) if !target.contains(':') => return Err(pane_error),
                    Err(error) => return Err(error),
                };
            return self
                .windows
                .get(&window)
                .map(|window| window.active_pane)
                .ok_or_else(|| ServerError::WindowNotFound(target.to_owned()));
        };
        let window = if window_target.is_empty() {
            self.resolve_window_core(None, current_session, current_window)?
        } else {
            self.resolve_window_core(Some(window_target), current_session, current_window)?
        };
        if pane_target.is_empty() {
            return Ok(self.windows[&window].active_pane);
        }
        self.resolve_pane_in_window(pane_target, window, pane_at_index)
    }

    fn resolve_pane_in_window(
        &self,
        target: &str,
        window: WindowId,
        pane_at_index: &impl Fn(WindowId, u32) -> Option<PaneId>,
    ) -> Result<PaneId, ServerError> {
        let target = normalize_pane_target(target);
        let not_found = || ServerError::PaneNotFound(target.to_owned());
        let state = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::WindowNotFound(window.to_string()))?;
        if target.starts_with('%') {
            let pane = target.parse::<PaneId>().map_err(|_| not_found())?;
            return state
                .panes
                .contains_key(&pane)
                .then_some(pane)
                .ok_or_else(not_found);
        }
        if target == "!" {
            return state.last_panes.first().copied().ok_or_else(not_found);
        }
        if let Some((forward, offset)) = parse_offset(target) {
            let current = state
                .pane_order
                .iter()
                .position(|pane| *pane == state.active_pane)
                .ok_or_else(not_found)?;
            let count = state.pane_order.len();
            if count == 0 {
                return Err(not_found());
            }
            let offset = usize::try_from(offset).map_err(|_| not_found())? % count;
            let position = if forward {
                (current + offset) % count
            } else {
                (current + count - offset) % count
            };
            return Ok(state.pane_order[position]);
        }
        let index = target.parse::<u32>().map_err(|_| not_found())?;
        pane_at_index(window, index).ok_or_else(not_found)
    }

    #[must_use]
    pub fn pane(&self, pane: PaneId) -> Option<&Pane> {
        self.windows
            .values()
            .find_map(|window| window.panes.get(&pane))
    }

    pub fn pane_mut(&mut self, pane: PaneId) -> Option<&mut Pane> {
        self.windows
            .values_mut()
            .find_map(|window| window.panes.get_mut(&pane))
    }

    pub fn mark_pane_dead(
        &mut self,
        pane: PaneId,
        status: Option<u32>,
    ) -> Result<bool, ServerError> {
        let pane = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if pane.dead && pane.dead_status == status && !pane.empty {
            return Ok(false);
        }
        pane.dead = true;
        pane.dead_status = status;
        pane.empty = false;
        self.bump_generation();
        Ok(true)
    }

    pub fn revive_pane(&mut self, pane: PaneId) -> Result<bool, ServerError> {
        let pane = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if !pane.dead && pane.dead_status.is_none() && pane.dead_time.is_none() && !pane.empty {
            return Ok(false);
        }
        pane.dead = false;
        pane.dead_status = None;
        pane.dead_time = None;
        pane.empty = false;
        self.bump_generation();
        Ok(true)
    }

    pub fn mark_pane_empty(&mut self, pane: PaneId) -> Result<bool, ServerError> {
        let pane = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if !pane.dead && pane.dead_status.is_none() && pane.dead_time.is_none() && pane.empty {
            return Ok(false);
        }
        pane.dead = false;
        pane.dead_status = None;
        pane.dead_time = None;
        pane.empty = true;
        self.bump_generation();
        Ok(true)
    }

    #[must_use]
    pub fn window_for_pane(&self, pane: PaneId) -> Option<WindowId> {
        self.windows
            .iter()
            .find_map(|(id, window)| window.panes.contains_key(&pane).then_some(*id))
    }

    /// The pane a new pane copies its working directory from: `target` itself
    /// when it is a terminal, otherwise the window's last focused terminal,
    /// otherwise the first in layout order.
    #[must_use]
    pub fn cwd_donor(&self, target: PaneId) -> Option<PaneId> {
        let window = &self.windows[&self.window_for_pane(target)?];
        let is_terminal = |pane: &PaneId| {
            matches!(
                window.panes.get(pane).map(|pane| &pane.kind),
                Some(PaneKind::Terminal)
            )
        };
        if is_terminal(&target) {
            return Some(target);
        }
        std::iter::once(&window.active_pane)
            .chain(&window.last_panes)
            .chain(&window.pane_order)
            .find(|pane| **pane != target && is_terminal(pane))
            .copied()
    }

    /// The Agent pane a routed payload (`send-last-output`) lands in: the active
    /// pane when it is an agent, otherwise focus history, otherwise layout order.
    #[must_use]
    pub fn recent_agent_pane(&self, origin: PaneId) -> Option<PaneId> {
        let window = &self.windows[&self.window_for_pane(origin)?];
        let is_agent = |pane: &PaneId| {
            matches!(
                window.panes.get(pane).map(|pane| &pane.kind),
                Some(PaneKind::Agent(_))
            )
        };
        std::iter::once(&window.active_pane)
            .chain(&window.last_panes)
            .chain(&window.pane_order)
            .find(|pane| is_agent(pane))
            .copied()
    }

    pub fn pane_in_direction(
        &self,
        pane: PaneId,
        direction: PaneDirection,
    ) -> Result<Option<PaneId>, ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let window = &self.windows[&window_id];
        let (window_columns, window_rows) = window.layout.extent();
        let rects = window
            .layout
            .panes_in_order()
            .into_iter()
            .map(|pane| {
                let geometry = window
                    .layout
                    .pane_geometry(pane)
                    .expect("layout order contains a pane geometry");
                let right = geometry
                    .xoff
                    .saturating_add(geometry.sx)
                    .saturating_add(1)
                    .min(window_columns);
                let bottom = geometry
                    .yoff
                    .saturating_add(geometry.sy)
                    .saturating_add(1)
                    .min(window_rows);
                (
                    pane,
                    PaneRect {
                        left: normalize_cell_coordinate(geometry.xoff, window_columns),
                        top: normalize_cell_coordinate(geometry.yoff, window_rows),
                        right: normalize_cell_coordinate(right, window_columns),
                        bottom: normalize_cell_coordinate(bottom, window_rows),
                    },
                )
            })
            .collect::<Vec<_>>();
        let current = rects
            .iter()
            .find_map(|(candidate, rect)| (*candidate == pane).then_some(*rect))
            .expect("validated layout contains every window pane");
        let candidates = directional_candidates(&rects, pane, current, direction);
        for recent in &window.last_panes {
            if candidates.contains(recent) {
                return Ok(Some(*recent));
            }
        }
        Ok(candidates.first().copied())
    }

    pub fn previous_pane(&self, pane: PaneId) -> Result<PaneId, ServerError> {
        self.pane_at_order_offset(pane, -1)
    }

    pub fn next_pane(&self, pane: PaneId) -> Result<PaneId, ServerError> {
        self.pane_at_order_offset(pane, 1)
    }

    fn pane_at_order_offset(&self, pane: PaneId, offset: isize) -> Result<PaneId, ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let window = &self.windows[&window_id];
        let index = window
            .pane_order
            .iter()
            .position(|candidate| *candidate == pane)
            .expect("validated pane order contains every window pane");
        let len = isize::try_from(window.pane_order.len()).expect("pane count fits isize");
        let next =
            (isize::try_from(index).expect("pane index fits isize") + offset).rem_euclid(len);
        Ok(window.pane_order[usize::try_from(next).expect("wrapped pane index is nonnegative")])
    }

    pub fn rotate_window(
        &mut self,
        window: WindowId,
        reverse: bool,
        preserve_zoom: bool,
    ) -> Result<PaneId, ServerError> {
        let window = self
            .windows
            .get_mut(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        let active = window.active_pane;
        let was_zoomed = window.zoomed_pane.is_some();
        if window.pane_order.len() <= 1 {
            if was_zoomed && !preserve_zoom {
                window.zoomed_pane = None;
                self.bump_generation();
            }
            return Ok(active);
        }

        let previous_order = window.pane_order.clone();
        let mut next_order = previous_order.clone();
        if reverse {
            next_order.rotate_right(1);
        } else {
            next_order.rotate_left(1);
        }
        let replacements = previous_order
            .into_iter()
            .zip(next_order.iter().copied())
            .collect::<BTreeMap<_, _>>();
        window.layout.remap(&replacements);
        window.pane_order = next_order;
        let next_active = replacements[&active];
        let pane_changed = activate_window_pane(window, next_active, false);
        window.zoomed_pane = (preserve_zoom && was_zoomed).then_some(next_active);
        if pane_changed {
            self.touch_pane_active_point(next_active);
        }
        self.bump_generation();
        Ok(next_active)
    }

    pub fn swap_panes(
        &mut self,
        source: PaneId,
        target: PaneId,
        detached: bool,
        preserve_zoom: bool,
    ) -> Result<(), ServerError> {
        let source_window = self
            .window_for_pane(source)
            .ok_or_else(|| ServerError::MissingTarget(source.to_string()))?;
        let target_window = self
            .window_for_pane(target)
            .ok_or_else(|| ServerError::MissingTarget(target.to_string()))?;

        if source == target {
            let window = self.windows.get_mut(&source_window).expect("window exists");
            if window.zoomed_pane.is_some() && !preserve_zoom {
                window.zoomed_pane = None;
                self.bump_generation();
            }
            return Ok(());
        }

        if source_window == target_window {
            let was_zoomed = {
                let window = self.windows.get_mut(&source_window).expect("window exists");
                let was_zoomed = window.zoomed_pane.is_some();
                let swapped = window.layout.swap(source, target);
                debug_assert!(swapped);
                swap_pane_order(&mut window.pane_order, source, target);
                was_zoomed
            };
            if detached {
                if self.windows[&source_window].active_pane == source {
                    let changed = activate_window_pane(
                        self.windows.get_mut(&source_window).expect("window exists"),
                        target,
                        false,
                    );
                    if changed {
                        self.touch_pane_active_point(target);
                    }
                }
                if self.windows[&source_window].active_pane == target {
                    let changed = activate_window_pane(
                        self.windows.get_mut(&source_window).expect("window exists"),
                        source,
                        false,
                    );
                    if changed {
                        self.touch_pane_active_point(source);
                    }
                }
            } else {
                let changed = activate_window_pane(
                    self.windows.get_mut(&source_window).expect("window exists"),
                    target,
                    false,
                );
                if changed {
                    self.touch_pane_active_point(target);
                }
            }
            let active = self.windows[&source_window].active_pane;
            self.windows
                .get_mut(&source_window)
                .expect("window exists")
                .zoomed_pane = (preserve_zoom && was_zoomed).then_some(active);
            self.bump_generation();
            return Ok(());
        }

        let source_was_zoomed = self.windows[&source_window].zoomed_pane.is_some();
        let target_was_zoomed = self.windows[&target_window].zoomed_pane.is_some();
        let source_active = self.windows[&source_window].active_pane;
        let target_active = self.windows[&target_window].active_pane;

        let mut source_state = self
            .windows
            .remove(&source_window)
            .expect("source window exists");
        let target_state = self
            .windows
            .get_mut(&target_window)
            .expect("target window exists");
        let source_replaced = source_state.layout.replace(source, target);
        debug_assert!(source_replaced);
        let target_replaced = target_state.layout.replace(target, source);
        debug_assert!(target_replaced);
        let source_pane = source_state
            .panes
            .remove(&source)
            .expect("source window contains source pane");
        let target_pane = target_state
            .panes
            .remove(&target)
            .expect("target window contains target pane");
        source_state.panes.insert(target, target_pane);
        target_state.panes.insert(source, source_pane);
        replace_pane_order(&mut source_state.pane_order, source, target);
        replace_pane_order(&mut target_state.pane_order, target, source);

        let next_source_active = if detached && source_active != source {
            source_active
        } else {
            target
        };
        let next_target_active = if detached && target_active != target {
            target_active
        } else {
            source
        };
        let source_changed =
            activate_relocated_window_pane(&mut source_state, next_source_active, source);
        let target_changed =
            activate_relocated_window_pane(target_state, next_target_active, target);
        source_state.zoomed_pane =
            (preserve_zoom && source_was_zoomed).then_some(source_state.active_pane);
        target_state.zoomed_pane =
            (preserve_zoom && target_was_zoomed).then_some(target_state.active_pane);
        self.windows.insert(source_window, source_state);
        if source_changed {
            self.touch_pane_active_point(next_source_active);
        }
        if target_changed {
            self.touch_pane_active_point(next_target_active);
        }
        self.bump_generation();
        Ok(())
    }

    pub fn break_pane(
        &mut self,
        pane: PaneId,
        destination_session: SessionId,
        destination_index: Option<u32>,
        name: Option<String>,
        detached: bool,
    ) -> Result<WindowId, ServerError> {
        self.break_pane_with_base_index(
            pane,
            destination_session,
            destination_index,
            name,
            detached,
            0,
        )
    }

    pub(crate) fn break_pane_with_base_index(
        &mut self,
        pane: PaneId,
        destination_session: SessionId,
        destination_index: Option<u32>,
        name: Option<String>,
        detached: bool,
        base_index: u32,
    ) -> Result<WindowId, ServerError> {
        let source_window = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if !self.sessions.contains_key(&destination_session) {
            return Err(ServerError::MissingTarget(destination_session.to_string()));
        }
        let source_session = self.windows[&source_window].session;
        let index = self.claim_window_index(destination_session, destination_index, base_index)?;
        if self.windows[&source_window].panes.len() == 1 {
            self.move_window(source_window, destination_session, index, false, !detached)?;
            if let Some(name) = name {
                self.windows
                    .get_mut(&source_window)
                    .expect("moved source window exists")
                    .name = name;
            }
            return Ok(source_window);
        }
        let inherited_extent = self
            .session_active_window_extent(destination_session)
            .unwrap_or(DEFAULT_WINDOW_EXTENT);
        let window_id = self.allocate_window_id();
        let created = self.allocate_sort_point();
        let mut source = self
            .windows
            .remove(&source_window)
            .expect("source window exists");
        let source_will_close = match source.layout.remove(pane) {
            Ok(()) => false,
            Err(LayoutError::LastPane) => true,
            Err(error) => {
                self.windows.insert(source_window, source);
                return Err(pane_layout_error(error, pane));
            }
        };
        let pane_state = source
            .panes
            .remove(&pane)
            .expect("source window contains pane");
        let activated = if source_will_close {
            self.sessions
                .get_mut(&source_session)
                .expect("source session exists")
                .forget_window(source_window)
        } else {
            repair_window_after_pane_removal(&mut source, pane);
            self.windows.insert(source_window, source);
            None
        };
        let window_name = name.unwrap_or_else(|| pane_state.title.clone());
        self.windows.insert(
            window_id,
            Window {
                id: window_id,
                session: destination_session,
                index,
                name: window_name,
                created,
                activity: created,
                activity_flag: false,
                silence_flag: false,
                active_pane: pane,
                zoomed_pane: None,
                layout: CellLayout::new(pane, inherited_extent.0, inherited_extent.1),
                panes: BTreeMap::from([(pane, pane_state)]),
                pane_order: vec![pane],
                last_panes: Vec::new(),
                last_layout: None,
                previous_layout: None,
                last_extent_probe: None,
                input_options: InputOptions::default(),
            },
        );
        let destination_was_empty = {
            let destination = self
                .sessions
                .get_mut(&destination_session)
                .expect("destination session exists");
            let was_empty = destination.windows.is_empty();
            destination.windows.push(window_id);
            if was_empty {
                destination.active_window = window_id;
                destination.last_window = None;
            }
            was_empty
        };
        if !destination_was_empty && !detached {
            self.activate_window(destination_session, window_id);
        }
        if let Some(window) = activated {
            self.touch_window_activity(window);
            self.clear_window_alerts(window);
        }
        if source_session != destination_session
            && self.sessions[&source_session].windows.is_empty()
        {
            self.sessions.remove(&source_session);
        }
        self.sort_session_windows(destination_session);
        self.bump_generation();
        Ok(window_id)
    }

    /// `before` changes layout placement, while pane order stays after the target to match tmux's by-value flag bug.
    pub fn join_pane(
        &mut self,
        source: PaneId,
        target: PaneId,
        axis: Axis,
        size: SplitSize,
        before: bool,
        full_size: bool,
        detached: bool,
    ) -> Result<(), ServerError> {
        if source == target {
            return Err(ServerError::InvalidCommand(
                "source and target panes must be different".to_owned(),
            ));
        }
        let source_window = self
            .window_for_pane(source)
            .ok_or_else(|| ServerError::MissingTarget(source.to_string()))?;
        let target_window = self
            .window_for_pane(target)
            .ok_or_else(|| ServerError::MissingTarget(target.to_string()))?;
        let source_session = self.windows[&source_window].session;
        let target_session = self.windows[&target_window].session;
        if source_window != target_window
            && self.windows[&source_window].panes.len() == 1
            && source_session != target_session
            && self.sessions[&source_session].windows.len() == 1
        {
            return Err(ServerError::InvalidCommand(
                "cannot move the last window out of a session".to_owned(),
            ));
        }
        if source_window == target_window {
            let next_split_id = &mut self.next_split_id;
            let mut ids = || {
                let id = SplitId(*next_split_id);
                *next_split_id = (*next_split_id).saturating_add(1);
                id
            };
            let window = self.windows.get_mut(&source_window).expect("window exists");
            let original_layout = window.layout.clone();
            window
                .layout
                .remove(source)
                .map_err(|error| pane_layout_error(error, source))?;
            if let Err(error) = window
                .layout
                .split(target, axis, size, before, full_size, source, &mut ids)
            {
                window.layout = original_layout;
                return Err(split_layout_error(error, target));
            }
            lose_window_pane(window, source);
            window.pane_order.retain(|pane| *pane != source);
            insert_pane_order(&mut window.pane_order, source, target, false, false);
            window.zoomed_pane = None;
            let pane_changed = !detached && activate_window_pane(window, source, false);
            normalize_window_history(window);
            if pane_changed {
                self.touch_pane_active_point(source);
            }
            self.bump_generation();
            return Ok(());
        }

        let mut source_state = self
            .windows
            .remove(&source_window)
            .expect("source window exists");
        let source_backup = source_state.clone();
        let source_will_close = match source_state.layout.remove(source) {
            Ok(()) => false,
            Err(LayoutError::LastPane) => true,
            Err(error) => {
                self.windows.insert(source_window, source_state);
                return Err(pane_layout_error(error, source));
            }
        };
        let pane_state = source_state
            .panes
            .remove(&source)
            .expect("source window contains source pane");
        if !source_will_close {
            repair_window_after_pane_removal(&mut source_state, source);
        }

        let next_split_id = &mut self.next_split_id;
        let mut ids = || {
            let id = SplitId(*next_split_id);
            *next_split_id = (*next_split_id).saturating_add(1);
            id
        };
        let split_result = self
            .windows
            .get_mut(&target_window)
            .expect("target window exists")
            .layout
            .split(target, axis, size, before, full_size, source, &mut ids);
        if let Err(error) = split_result {
            self.windows.insert(source_window, source_backup);
            return Err(split_layout_error(error, target));
        }
        let target_state = self
            .windows
            .get_mut(&target_window)
            .expect("target window exists");
        target_state.panes.insert(source, pane_state);
        insert_pane_order(&mut target_state.pane_order, source, target, false, false);
        target_state.zoomed_pane = None;
        let target_changed = !detached && activate_window_pane(target_state, source, false);
        normalize_window_history(target_state);
        if target_changed {
            self.touch_pane_active_point(source);
        }

        if !detached {
            self.activate_window(target_session, target_window);
        }

        let activated = if source_will_close {
            self.sessions
                .get_mut(&source_session)
                .expect("source session exists")
                .forget_window(source_window)
        } else {
            self.windows.insert(source_window, source_state);
            None
        };
        if let Some(window) = activated {
            self.touch_window_activity(window);
            self.clear_window_alerts(window);
        }
        self.bump_generation();
        Ok(())
    }

    pub fn last_window(&self, session: SessionId) -> Result<WindowId, ServerError> {
        let session = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        session
            .last_window()
            .filter(|window| session.windows.contains(window))
            .ok_or_else(|| ServerError::InvalidCommand("no last window".to_owned()))
    }

    pub fn last_pane(&self, window: WindowId) -> Result<PaneId, ServerError> {
        let window = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
        if let Some(pane) = window.last_panes.first() {
            return Ok(*pane);
        }
        if window.panes.len() == 2 {
            return window
                .panes
                .keys()
                .find(|pane| **pane != window.active_pane)
                .copied()
                .ok_or_else(|| ServerError::InvalidCommand("no last pane".to_owned()));
        }
        Err(ServerError::InvalidCommand("no last pane".to_owned()))
    }

    pub(crate) fn pane_input_off(&self, pane: PaneId) -> Result<bool, ServerError> {
        self.pane(pane)
            .map(|target| target.input_off)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))
    }

    pub(crate) fn set_pane_input_off(
        &mut self,
        pane: PaneId,
        input_off: bool,
    ) -> Result<bool, ServerError> {
        let target = self
            .pane_mut(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if target.input_off == input_off {
            return Ok(false);
        }
        target.input_off = input_off;
        self.bump_generation();
        Ok(true)
    }

    #[must_use]
    pub fn snapshot(&self) -> MuxSnapshot {
        let sessions = self
            .sessions
            .values()
            .map(|session| SessionSnapshot {
                id: session.id,
                name: session.name.clone(),
                active_window: session.active_window,
                windows: session
                    .windows
                    .iter()
                    .filter_map(|id| self.windows.get(id))
                    .map(|window| self.window_snapshot(window))
                    .collect(),
                viewers: Vec::new(),
            })
            .collect();
        MuxSnapshot {
            generation: self.generation,
            sessions,
            focused_window: None,
        }
    }

    fn window_snapshot(&self, window: &Window) -> WindowSnapshot {
        let synchronized = window
            .input_options
            .synchronize_panes()
            .unwrap_or_else(|| self.global_synchronize_panes());
        let (width, height) = window.layout.extent();
        let layout_dump = window.layout.dump();
        let visible_layout_dump = window.zoomed_pane.map_or_else(
            || layout_dump.clone(),
            |pane| CellLayout::new(pane, width, height).dump(),
        );
        WindowSnapshot {
            id: window.id,
            index: window.index,
            name: window.name.clone(),
            automatic_rename: window
                .input_options
                .automatic_rename()
                .unwrap_or_else(|| self.global_automatic_rename()),
            active_pane: window.active_pane,
            zoomed_pane: window.zoomed_pane,
            layout: window.layout.project(),
            panes: window
                .panes
                .iter()
                .map(|(id, pane)| {
                    (
                        *id,
                        PaneSnapshot {
                            id: pane.id,
                            title: pane.title.clone(),
                            kind: pane.kind.snapshot(),
                            synchronized_input: pane
                                .input_options
                                .synchronize_panes()
                                .unwrap_or(synchronized),
                            bell: pane.bell,
                            dead: pane.dead,
                            dead_status: pane.dead_status,
                            border_colour: None,
                            active_border_colour: None,
                        },
                    )
                })
                .collect(),
            layout_dump,
            visible_layout_dump,
            status_label: String::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut all_panes = BTreeSet::new();
        let mut all_splits = BTreeSet::new();
        for (window_id, window) in &self.windows {
            let session = self
                .sessions
                .get(&window.session)
                .ok_or_else(|| format!("window {window_id} has missing session"))?;
            if !session.windows.contains(window_id) {
                return Err(format!("session does not contain window {window_id}"));
            }
            if let Err(error) = window.layout.validate() {
                return Err(format!("window {window_id} has an invalid layout: {error}"));
            }
            let layout_panes = window.layout.panes_in_order();
            let layout_set = layout_panes.iter().copied().collect::<BTreeSet<_>>();
            let pane_set = window.panes.keys().copied().collect::<BTreeSet<_>>();
            if layout_set != pane_set || layout_panes.len() != layout_set.len() {
                return Err(format!("window {window_id} layout does not match panes"));
            }
            let pane_order = window.pane_order.iter().copied().collect::<BTreeSet<_>>();
            if pane_order != pane_set || window.pane_order.len() != pane_order.len() {
                return Err(format!(
                    "window {window_id} pane order does not match panes"
                ));
            }
            let mut layout_splits = Vec::new();
            window.layout.project().splits(&mut layout_splits);
            for split in layout_splits {
                if !all_splits.insert(split) {
                    return Err(format!("split {split} occurs more than once"));
                }
            }
            if !pane_set.contains(&window.active_pane) {
                return Err(format!("window {window_id} active pane is missing"));
            }
            let history = window.last_panes.iter().copied().collect::<BTreeSet<_>>();
            if history.len() != window.last_panes.len()
                || history.contains(&window.active_pane)
                || !history.is_subset(&pane_set)
            {
                return Err(format!("window {window_id} pane history is invalid"));
            }
            if window
                .zoomed_pane
                .is_some_and(|pane| pane != window.active_pane || !pane_set.contains(&pane))
            {
                return Err(format!("window {window_id} zoomed pane is invalid"));
            }
            for pane in pane_set {
                if !all_panes.insert(pane) {
                    return Err(format!("pane {pane} occurs in multiple windows"));
                }
            }
        }
        for session in self.sessions.values() {
            if session.windows.is_empty() || !session.windows.contains(&session.active_window) {
                return Err(format!("session {} has invalid active window", session.id));
            }
        }
        Ok(())
    }

    pub(crate) fn next_window_index(
        &self,
        session: SessionId,
        base_index: u32,
    ) -> Result<u32, ServerError> {
        if base_index > MAX_WINDOW_INDEX {
            return Err(ServerError::InvalidCommand(
                "no free window index".to_owned(),
            ));
        }
        let session = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        let used = session
            .windows
            .iter()
            .map(|window| self.windows[window].index)
            .collect::<BTreeSet<_>>();
        (base_index..=MAX_WINDOW_INDEX)
            .chain(0..base_index)
            .find(|index| !used.contains(index))
            .ok_or_else(|| ServerError::InvalidCommand("no free window index".to_owned()))
    }

    fn allocate_session_id(&mut self) -> SessionId {
        let id = SessionId(self.next_session_id);
        self.next_session_id = self.next_session_id.saturating_add(1);
        id
    }

    fn allocate_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id = self.next_window_id.saturating_add(1);
        id
    }

    fn allocate_pane_id(&mut self) -> PaneId {
        let id = PaneId(self.next_pane_id);
        self.next_pane_id = self.next_pane_id.saturating_add(1);
        id
    }

    fn allocate_split_id(&mut self) -> SplitId {
        let id = SplitId(self.next_split_id);
        self.next_split_id = self.next_split_id.saturating_add(1);
        id
    }

    fn allocate_sort_point(&mut self) -> u64 {
        let point = self.next_sort_point;
        self.next_sort_point = self.next_sort_point.saturating_add(1);
        point
    }

    pub(crate) fn bump_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }
}

fn swap_window_id(value: WindowId, source: WindowId, target: WindowId) -> WindowId {
    if value == source {
        target
    } else if value == target {
        source
    } else {
        value
    }
}

fn pane_title(kind: &PaneKind) -> String {
    match kind {
        PaneKind::Picker { .. } => "new pane".to_owned(),
        PaneKind::Terminal => "terminal".to_owned(),
        PaneKind::Browser(browser) if browser.url() == "about:blank" => "browser".to_owned(),
        PaneKind::Browser(browser) => browser.url().to_owned(),
        PaneKind::Agent(_) => "agent".to_owned(),
        PaneKind::Editor(_) => "editor".to_owned(),
    }
}

fn split_layout_error(error: LayoutError, target: PaneId) -> ServerError {
    match error {
        LayoutError::NoSpace => ServerError::InvalidCommand("no space for a new pane".to_owned()),
        LayoutError::UnknownPane => ServerError::MissingTarget(target.to_string()),
        LayoutError::LastPane | LayoutError::UnknownDivider => {
            ServerError::Internal(format!("unexpected split layout error: {error:?}"))
        }
    }
}

fn pane_layout_error(error: LayoutError, pane: PaneId) -> ServerError {
    match error {
        LayoutError::UnknownPane => ServerError::MissingTarget(pane.to_string()),
        LayoutError::LastPane | LayoutError::NoSpace | LayoutError::UnknownDivider => {
            ServerError::Internal(format!("unexpected pane layout error: {error:?}"))
        }
    }
}

fn divider_layout_error(error: LayoutError, split: SplitId) -> ServerError {
    match error {
        LayoutError::UnknownDivider => ServerError::MissingTarget(split.to_string()),
        LayoutError::LastPane | LayoutError::NoSpace | LayoutError::UnknownPane => {
            ServerError::Internal(format!("unexpected divider layout error: {error:?}"))
        }
    }
}

fn unique_candidate<T: Copy>(candidates: impl IntoIterator<Item = T>) -> Result<Option<T>, ()> {
    let mut candidates = candidates.into_iter();
    let first = candidates.next();
    if first.is_some() && candidates.next().is_some() {
        return Err(());
    }
    Ok(first)
}

fn validate_window_index_run(base_index: u32, count: usize) -> Result<(), ServerError> {
    let last_offset = u32::try_from(count.saturating_sub(1))
        .ok()
        .filter(|offset| {
            base_index
                .checked_add(*offset)
                .is_some_and(|last| last <= MAX_WINDOW_INDEX)
        });
    if base_index > MAX_WINDOW_INDEX || (count != 0 && last_offset.is_none()) {
        return Err(ServerError::InvalidCommand(
            "no free window index".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_window_target(target: &str) -> (&str, bool) {
    let (target, exact) = target
        .strip_prefix('=')
        .map_or((target, false), |target| (target, true));
    let target = match target {
        "{start}" => "^",
        "{last}" => "!",
        "{end}" => "$",
        "{next}" => "+",
        "{previous}" => "-",
        _ => target,
    };
    (target, exact)
}

fn normalize_pane_target(target: &str) -> &str {
    match target {
        "{last}" => "!",
        "{next}" => "+",
        "{previous}" => "-",
        "{top}" => "top",
        "{bottom}" => "bottom",
        "{left}" => "left",
        "{right}" => "right",
        "{top-left}" => "top-left",
        "{top-right}" => "top-right",
        "{bottom-left}" => "bottom-left",
        "{bottom-right}" => "bottom-right",
        _ => target,
    }
}

fn parse_offset(target: &str) -> Option<(bool, u32)> {
    let (forward, offset) = match target.as_bytes().first() {
        Some(b'+') => (true, &target[1..]),
        Some(b'-') => (false, &target[1..]),
        _ => return None,
    };
    let offset = if offset.is_empty() {
        1
    } else {
        offset.parse::<u32>().ok()?
    };
    (1..=MAX_WINDOW_INDEX)
        .contains(&offset)
        .then_some((forward, offset))
}

#[derive(Clone)]
enum GlobToken {
    AnySequence,
    AnyCharacter,
    Literal(char),
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

#[derive(Clone, Copy)]
struct GlobClassCharacter {
    value: char,
    escaped: bool,
}

fn fnmatch(pattern: &str, value: &str) -> bool {
    let Some(tokens) = glob_tokens(pattern) else {
        return false;
    };
    let value = value.chars().collect::<Vec<_>>();
    let mut matched = vec![false; value.len() + 1];
    matched[0] = true;
    for token in tokens {
        let mut next = vec![false; value.len() + 1];
        match token {
            GlobToken::AnySequence => {
                next[0] = matched[0];
                for index in 1..=value.len() {
                    next[index] = matched[index] || next[index - 1];
                }
            }
            GlobToken::AnyCharacter => {
                next[1..].copy_from_slice(&matched[..value.len()]);
            }
            GlobToken::Literal(expected) => {
                for (index, character) in value.iter().copied().enumerate() {
                    next[index + 1] = matched[index] && character == expected;
                }
            }
            GlobToken::Class { negated, ranges } => {
                for (index, character) in value.iter().copied().enumerate() {
                    let contains = ranges
                        .iter()
                        .any(|(start, end)| *start <= character && character <= *end);
                    next[index + 1] = matched[index] && contains != negated;
                }
            }
        }
        matched = next;
    }
    matched[value.len()]
}

fn glob_tokens(pattern: &str) -> Option<Vec<GlobToken>> {
    let characters = pattern.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        match characters[index] {
            '*' => {
                if !matches!(tokens.last(), Some(GlobToken::AnySequence)) {
                    tokens.push(GlobToken::AnySequence);
                }
                index += 1;
            }
            '?' => {
                tokens.push(GlobToken::AnyCharacter);
                index += 1;
            }
            '\\' => {
                index += 1;
                let character = characters.get(index).copied()?;
                tokens.push(GlobToken::Literal(character));
                index += 1;
            }
            '[' => {
                if let Some((token, next)) = glob_class(&characters, index + 1) {
                    tokens.push(token);
                    index = next;
                } else {
                    tokens.push(GlobToken::Literal('['));
                    index += 1;
                }
            }
            character => {
                tokens.push(GlobToken::Literal(character));
                index += 1;
            }
        }
    }
    Some(tokens)
}

fn glob_class(characters: &[char], mut index: usize) -> Option<(GlobToken, usize)> {
    let negated = matches!(characters.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let mut members: Vec<GlobClassCharacter> = Vec::new();
    while index < characters.len() {
        let character = characters[index];
        if character == ']' && !members.is_empty() {
            let mut ranges = Vec::new();
            let mut member = 0;
            while member < members.len() {
                if member + 2 < members.len()
                    && members[member + 1].value == '-'
                    && !members[member + 1].escaped
                {
                    ranges.push((members[member].value, members[member + 2].value));
                    member += 3;
                } else {
                    ranges.push((members[member].value, members[member].value));
                    member += 1;
                }
            }
            return Some((GlobToken::Class { negated, ranges }, index + 1));
        }
        if character == '\\' {
            index += 1;
            members.push(GlobClassCharacter {
                value: characters.get(index).copied()?,
                escaped: true,
            });
        } else {
            members.push(GlobClassCharacter {
                value: character,
                escaped: false,
            });
        }
        index += 1;
    }
    None
}

/// Pure client-side predictor for `swap-pane` on a wire [`LayoutNode`].
/// A client renders a drop optimistically through this same transform.
#[must_use]
pub fn swapped_layout(layout: &LayoutNode, source: PaneId, target: PaneId) -> LayoutNode {
    let mut layout = layout.clone();
    predict_swap_layout_panes(&mut layout, source, target);
    layout
}

/// Pure client-side predictor for `join-pane` on a wire [`LayoutNode`].
/// Its ratio-tree surgery approximates the engine's cell-derived result.
/// Returns `None` when the panes are the same or either leaf is missing.
#[must_use]
pub fn joined_layout(
    layout: &LayoutNode,
    source: PaneId,
    target: PaneId,
    split: SplitId,
    axis: Axis,
    pane_ratio: f32,
    before: bool,
) -> Option<LayoutNode> {
    if source == target {
        return None;
    }
    let mut layout = layout.clone();
    if !predict_remove_layout_leaf(&mut layout, source) {
        return None;
    }
    predict_insert_layout_pane(
        &mut layout,
        target,
        source,
        split,
        axis,
        pane_ratio,
        before,
        false,
    )
    .then_some(layout)
}

fn predict_insert_layout_pane(
    node: &mut LayoutNode,
    target: PaneId,
    pane: PaneId,
    split: SplitId,
    axis: Axis,
    pane_ratio: f32,
    before: bool,
    full_size: bool,
) -> bool {
    if full_size {
        if !node.contains(target) {
            return false;
        }
        let existing = std::mem::replace(node, LayoutNode::Pane(pane));
        let (ratio, first, second) = if before {
            (
                pane_ratio,
                Box::new(LayoutNode::Pane(pane)),
                Box::new(existing),
            )
        } else {
            (
                1.0 - pane_ratio,
                Box::new(existing),
                Box::new(LayoutNode::Pane(pane)),
            )
        };
        *node = LayoutNode::Split {
            id: split,
            axis,
            ratio,
            first,
            second,
        };
        return true;
    }
    match node {
        LayoutNode::Pane(candidate) if *candidate == target => {
            let existing = LayoutNode::Pane(target);
            let (ratio, first, second) = if before {
                (
                    pane_ratio,
                    Box::new(LayoutNode::Pane(pane)),
                    Box::new(existing),
                )
            } else {
                (
                    1.0 - pane_ratio,
                    Box::new(existing),
                    Box::new(LayoutNode::Pane(pane)),
                )
            };
            *node = LayoutNode::Split {
                id: split,
                axis,
                ratio,
                first,
                second,
            };
            true
        }
        LayoutNode::Pane(_) => false,
        LayoutNode::Split { first, second, .. } => {
            predict_insert_layout_pane(first, target, pane, split, axis, pane_ratio, before, false)
                || predict_insert_layout_pane(
                    second, target, pane, split, axis, pane_ratio, before, false,
                )
        }
    }
}

fn predict_remove_layout_leaf(node: &mut LayoutNode, target: PaneId) -> bool {
    let promote_second = match node {
        LayoutNode::Pane(_) => return false,
        LayoutNode::Split { first, .. } if matches!(first.as_ref(), LayoutNode::Pane(pane) if *pane == target) => {
            Some(true)
        }
        LayoutNode::Split { second, .. } if matches!(second.as_ref(), LayoutNode::Pane(pane) if *pane == target) => {
            Some(false)
        }
        LayoutNode::Split { .. } => None,
    };

    if let Some(promote_second) = promote_second {
        let removed = std::mem::replace(node, LayoutNode::Pane(target));
        let LayoutNode::Split { first, second, .. } = removed else {
            unreachable!("only split nodes can promote a sibling")
        };
        *node = if promote_second { *second } else { *first };
        return true;
    }

    let LayoutNode::Split { first, second, .. } = node else {
        unreachable!("pane nodes return before recursive removal")
    };
    predict_remove_layout_leaf(first, target) || predict_remove_layout_leaf(second, target)
}

fn activate_window_pane(window: &mut Window, pane: PaneId, preserve_zoom: bool) -> bool {
    if window.active_pane == pane {
        return false;
    }
    debug_assert!(window.panes.contains_key(&pane));
    let previous = window.active_pane;
    let was_zoomed = window.zoomed_pane.is_some();
    window
        .last_panes
        .retain(|candidate| *candidate != pane && *candidate != previous);
    window.last_panes.insert(0, previous);
    window
        .last_panes
        .truncate(window.panes.len().saturating_sub(1));
    window.active_pane = pane;
    window.zoomed_pane = (preserve_zoom && was_zoomed).then_some(pane);
    true
}

fn repair_window_after_pane_removal(window: &mut Window, pane: PaneId) {
    window.zoomed_pane = None;
    lose_window_pane(window, pane);
    window.pane_order.retain(|candidate| *candidate != pane);
    normalize_window_history(window);
}

fn lose_window_pane(window: &mut Window, pane: PaneId) {
    window.last_panes.retain(|candidate| *candidate != pane);
    if window.active_pane != pane {
        return;
    }
    let position = window
        .pane_order
        .iter()
        .position(|candidate| *candidate == pane)
        .expect("relocated pane belongs to the window");
    let next = window
        .last_panes
        .first()
        .copied()
        .or_else(|| {
            position
                .checked_sub(1)
                .and_then(|position| window.pane_order.get(position).copied())
        })
        .or_else(|| window.pane_order.get(position + 1).copied())
        .expect("losing a pane leaves another pane");
    window.active_pane = next;
    window.last_panes.retain(|candidate| *candidate != next);
}

fn normalize_window_history(window: &mut Window) {
    let active = window.active_pane;
    let panes = &window.panes;
    window
        .last_panes
        .retain(|candidate| *candidate != active && panes.contains_key(candidate));
    window
        .last_panes
        .truncate(window.panes.len().saturating_sub(1));
}

fn insert_pane_order(
    order: &mut Vec<PaneId>,
    pane: PaneId,
    target: PaneId,
    before: bool,
    full_size: bool,
) {
    debug_assert!(!order.contains(&pane));
    if full_size {
        if before {
            order.insert(0, pane);
        } else {
            order.push(pane);
        }
        return;
    }
    let target = order
        .iter()
        .position(|candidate| *candidate == target)
        .expect("validated pane order contains the target");
    order.insert(if before { target } else { target + 1 }, pane);
}

fn replace_pane_order(order: &mut [PaneId], pane: PaneId, replacement: PaneId) {
    let slot = order
        .iter_mut()
        .find(|candidate| **candidate == pane)
        .expect("validated pane order contains the replaced pane");
    *slot = replacement;
}

fn swap_pane_order(order: &mut [PaneId], first: PaneId, second: PaneId) {
    for pane in order {
        if *pane == first {
            *pane = second;
        } else if *pane == second {
            *pane = first;
        }
    }
}

fn activate_relocated_window_pane(window: &mut Window, pane: PaneId, outgoing: PaneId) -> bool {
    let previous = window.active_pane;
    window.last_panes.retain(|candidate| {
        *candidate != outgoing && *candidate != pane && window.panes.contains_key(candidate)
    });
    if previous != outgoing && previous != pane && window.panes.contains_key(&previous) {
        window.last_panes.retain(|candidate| *candidate != previous);
        window.last_panes.insert(0, previous);
    }
    window
        .last_panes
        .truncate(window.panes.len().saturating_sub(1));
    window.active_pane = pane;
    previous != pane
}

fn predict_swap_layout_panes(node: &mut LayoutNode, source: PaneId, target: PaneId) {
    match node {
        LayoutNode::Pane(pane) if *pane == source => *pane = target,
        LayoutNode::Pane(pane) if *pane == target => *pane = source,
        LayoutNode::Pane(_) => {}
        LayoutNode::Split { first, second, .. } => {
            predict_swap_layout_panes(first, source, target);
            predict_swap_layout_panes(second, source, target);
        }
    }
}

#[cfg(test)]
fn collect_pane_rects(node: &LayoutNode, bounds: PaneRect, output: &mut Vec<(PaneId, PaneRect)>) {
    match node {
        LayoutNode::Pane(pane) => output.push((*pane, bounds)),
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
            ..
        } => match axis {
            Axis::Horizontal => {
                let split = split_coordinate(bounds.left, bounds.right, *ratio);
                collect_pane_rects(
                    first,
                    PaneRect {
                        right: split,
                        ..bounds
                    },
                    output,
                );
                collect_pane_rects(
                    second,
                    PaneRect {
                        left: split,
                        ..bounds
                    },
                    output,
                );
            }
            Axis::Vertical => {
                let split = split_coordinate(bounds.top, bounds.bottom, *ratio);
                collect_pane_rects(
                    first,
                    PaneRect {
                        bottom: split,
                        ..bounds
                    },
                    output,
                );
                collect_pane_rects(
                    second,
                    PaneRect {
                        top: split,
                        ..bounds
                    },
                    output,
                );
            }
        },
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated split ratios map a bounded one-million-unit logical extent to u32"
)]
#[cfg(test)]
fn split_coordinate(start: u32, end: u32, ratio: f32) -> u32 {
    let extent = end.saturating_sub(start);
    if extent <= 1 {
        return start.saturating_add(extent);
    }
    let offset = (f64::from(extent) * f64::from(ratio)).round() as u32;
    start + offset.clamp(1, extent - 1)
}

fn normalize_cell_coordinate(coordinate: u16, extent: u16) -> u32 {
    if extent == 0 {
        return 0;
    }
    let numerator = u64::from(coordinate) * u64::from(LAYOUT_COORDINATE_MAX);
    let rounded = (numerator + u64::from(extent) / 2) / u64::from(extent);
    u32::try_from(rounded).unwrap_or(LAYOUT_COORDINATE_MAX)
}

fn directional_candidates(
    rects: &[(PaneId, PaneRect)],
    pane: PaneId,
    current: PaneRect,
    direction: PaneDirection,
) -> Vec<PaneId> {
    rects
        .iter()
        .filter_map(|(candidate, rect)| {
            if *candidate == pane {
                return None;
            }
            let adjacent = match direction {
                PaneDirection::Left => {
                    let edge = if current.left == 0 {
                        LAYOUT_COORDINATE_MAX
                    } else {
                        current.left
                    };
                    rect.right == edge
                        && ranges_overlap(current.top, current.bottom, rect.top, rect.bottom)
                }
                PaneDirection::Right => {
                    let edge = if current.right == LAYOUT_COORDINATE_MAX {
                        0
                    } else {
                        current.right
                    };
                    rect.left == edge
                        && ranges_overlap(current.top, current.bottom, rect.top, rect.bottom)
                }
                PaneDirection::Up => {
                    let edge = if current.top == 0 {
                        LAYOUT_COORDINATE_MAX
                    } else {
                        current.top
                    };
                    rect.bottom == edge
                        && ranges_overlap(current.left, current.right, rect.left, rect.right)
                }
                PaneDirection::Down => {
                    let edge = if current.bottom == LAYOUT_COORDINATE_MAX {
                        0
                    } else {
                        current.bottom
                    };
                    rect.top == edge
                        && ranges_overlap(current.left, current.right, rect.left, rect.right)
                }
            };
            adjacent.then_some(*candidate)
        })
        .collect()
}

fn ranges_overlap(first_start: u32, first_end: u32, second_start: u32, second_end: u32) -> bool {
    first_start.max(second_start) < first_end.min(second_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split_ratio(layout: &CellLayout, split: SplitId) -> Option<f32> {
        fn find(node: &LayoutNode, split: SplitId) -> Option<f32> {
            let LayoutNode::Split {
                id,
                ratio,
                first,
                second,
                ..
            } = node
            else {
                return None;
            };
            (*id == split)
                .then_some(*ratio)
                .or_else(|| find(first, split))
                .or_else(|| find(second, split))
        }

        find(&layout.project(), split)
    }

    fn wire_layout_panes(node: &LayoutNode) -> Vec<PaneId> {
        let mut panes = Vec::new();
        node.panes(&mut panes);
        panes
    }

    fn layout_splits(layout: &CellLayout) -> Vec<SplitId> {
        let mut splits = Vec::new();
        layout.project().splits(&mut splits);
        splits
    }

    fn layout_panes(layout: &CellLayout) -> Vec<PaneId> {
        layout.panes_in_order()
    }

    fn pane_size(layout: &CellLayout, pane: PaneId) -> (u16, u16) {
        let geometry = layout.pane_geometry(pane).expect("layout contains pane");
        (geometry.sx, geometry.sy)
    }

    fn same_layout_geometry(left: &CellLayout, right: &CellLayout) -> bool {
        left.dump() == right.dump()
    }

    fn same_projected_geometry(left: &CellLayout, right: &LayoutNode) -> bool {
        fn axes(node: &LayoutNode, output: &mut Vec<Axis>) {
            let LayoutNode::Split {
                axis,
                first,
                second,
                ..
            } = node
            else {
                return;
            };
            output.push(*axis);
            axes(first, output);
            axes(second, output);
        }

        let projected = left.project();
        let mut projected_axes = Vec::new();
        let mut predicted_axes = Vec::new();
        axes(&projected, &mut projected_axes);
        axes(right, &mut predicted_axes);
        let bounds = PaneRect {
            left: 0,
            top: 0,
            right: LAYOUT_COORDINATE_MAX,
            bottom: LAYOUT_COORDINATE_MAX,
        };
        let mut projected_rects = Vec::new();
        let mut predicted_rects = Vec::new();
        collect_pane_rects(&projected, bounds, &mut projected_rects);
        collect_pane_rects(right, bounds, &mut predicted_rects);
        let tolerance = LAYOUT_COORDINATE_MAX * 3 / 100;
        wire_layout_panes(&projected) == wire_layout_panes(right)
            && projected_axes == predicted_axes
            && projected_rects.len() == predicted_rects.len()
            && projected_rects.iter().all(|(pane, projected)| {
                predicted_rects.iter().any(|(candidate, predicted)| {
                    pane == candidate
                        && projected.left.abs_diff(predicted.left) <= tolerance
                        && projected.top.abs_diff(predicted.top) <= tolerance
                        && projected.right.abs_diff(predicted.right) <= tolerance
                        && projected.bottom.abs_diff(predicted.bottom) <= tolerance
                })
            })
    }

    #[test]
    fn recent_agent_pane_follows_focus_history_then_layout_order() {
        let mut state = MuxState::default();
        let (_, _, terminal) = state.create_session("main").unwrap();
        assert_eq!(state.recent_agent_pane(terminal), None);

        let first = state
            .split_pane(
                terminal,
                Axis::Horizontal,
                PaneKind::Agent(AgentDescriptor::default()),
            )
            .unwrap();
        let second = state
            .split_pane(
                terminal,
                Axis::Vertical,
                PaneKind::Agent(AgentDescriptor::default()),
            )
            .unwrap();

        assert_eq!(state.recent_agent_pane(terminal), Some(second));

        state.select_pane(first).unwrap();
        state.select_pane(terminal).unwrap();
        assert_eq!(state.recent_agent_pane(terminal), Some(first));

        let browser = state
            .split_pane(
                terminal,
                Axis::Horizontal,
                PaneKind::Browser(BrowserDescriptor::single(
                    "https://example.com".to_owned(),
                    "default".to_owned(),
                )),
            )
            .unwrap();
        assert_ne!(state.recent_agent_pane(terminal), Some(browser));
    }

    #[test]
    fn pane_titles_change_snapshot_generation_without_overwriting_static_window_names() {
        let mut state = MuxState::default();
        let (_, window, terminal) = state.create_session("main").unwrap();
        let browser = state
            .split_pane(
                terminal,
                Axis::Horizontal,
                PaneKind::Browser(BrowserDescriptor::single(
                    "https://example.com".to_owned(),
                    "default".to_owned(),
                )),
            )
            .unwrap();
        state.rename_window(window, "manually named").unwrap();

        let generation = state.generation();
        assert!(state.update_pane_title(terminal, "~/src/zz").unwrap());
        assert_eq!(state.generation(), generation + 1);
        assert_eq!(state.windows[&window].name, "manually named");
        assert_eq!(state.windows[&window].panes[&terminal].title, "~/src/zz");
        assert!(!state.update_pane_title(terminal, "~/src/zz").unwrap());
        assert_eq!(state.generation(), generation + 1);

        assert!(state.update_pane_title(browser, "Example Domain").unwrap());
        state
            .update_browser_url(browser, "https://example.com".to_owned())
            .unwrap();
        assert_eq!(
            state.windows[&window].panes[&browser].title, "Example Domain",
            "a duplicate address event must not replace a newer document title"
        );
        state
            .update_browser_url(browser, "https://example.org".to_owned())
            .unwrap();
        assert_eq!(
            state.windows[&window].panes[&browser].title, "Example Domain",
            "URL persistence and displayed pane titles have separate ownership"
        );
        assert!(matches!(
            &state.windows[&window].panes[&browser].kind,
            PaneKind::Browser(browser) if browser.url() == "https://example.org"
        ));

        let generation = state.generation();
        state.update_browser_profile(browser, " Work ").unwrap();
        assert_eq!(state.generation(), generation + 1);
        assert!(matches!(
            &state.windows[&window].panes[&browser].kind,
            PaneKind::Browser(browser) if browser.profile == "Work"
        ));
        state.update_browser_profile(browser, "zz-default").unwrap();
        assert!(matches!(
            &state.windows[&window].panes[&browser].kind,
            PaneKind::Browser(browser) if browser.profile == "default"
        ));
    }

    #[test]
    fn cwd_donors_fall_back_to_the_last_focused_terminal() {
        let mut state = MuxState::default();
        let (_, _, first) = state.create_session("main").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let browser = state
            .split_pane(
                second,
                Axis::Vertical,
                PaneKind::Browser(BrowserDescriptor::single(
                    "about:blank".to_owned(),
                    "default".to_owned(),
                )),
            )
            .unwrap();

        assert_eq!(state.cwd_donor(first), Some(first));
        assert_eq!(state.cwd_donor(second), Some(second));
        assert_eq!(
            state.cwd_donor(browser),
            Some(second),
            "a pane without a working directory borrows the last focused terminal's"
        );

        state.select_pane(first).unwrap();
        state.select_pane(browser).unwrap();
        assert_eq!(state.cwd_donor(browser), Some(first));

        let picker = state
            .split_pane(
                browser,
                Axis::Horizontal,
                PaneKind::Picker {
                    inherit_cwd_from: None,
                },
            )
            .unwrap();
        assert_eq!(
            state.materialize_pane(picker, PaneKind::Agent(AgentDescriptor::default())),
            Ok(Some(first)),
            "a picker split with no donor still opens on the last focused terminal"
        );

        state.kill_pane(first).unwrap();
        state.kill_pane(second).unwrap();
        assert_eq!(
            state.cwd_donor(browser),
            None,
            "a window with no terminal left has nothing to inherit"
        );
    }

    #[test]
    fn split_remove_and_ids_preserve_invariants() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("main").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let third = state
            .split_pane(
                second,
                Axis::Vertical,
                PaneKind::Browser(BrowserDescriptor::single(
                    "https://example.com".to_owned(),
                    "default".to_owned(),
                )),
            )
            .unwrap();
        assert_eq!((first, second, third), (PaneId(0), PaneId(1), PaneId(2)));
        assert!(state.validate().is_ok());
        state.kill_pane(second).unwrap();
        assert!(state.validate().is_ok());
        assert!(state.windows[&window].panes.contains_key(&third));
        let fourth = state
            .split_pane(first, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        assert_eq!(fourth, PaneId(3));
    }

    #[test]
    fn pane_removal_fallback_preserves_the_replacement_active_point() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let third = state
            .split_pane(second, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        state.select_pane(first).unwrap();
        let replacement_point = state.windows[&window].panes[&third].active_point;

        state.kill_pane(first).unwrap();

        assert_eq!(state.windows[&window].active_pane, third);
        assert_eq!(
            state.windows[&window].panes[&third].active_point,
            replacement_point
        );
        assert!(state.validate().is_ok());
    }

    #[test]
    fn failed_split_keeps_the_cell_tree_and_allocators_unchanged() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("main").unwrap();
        let narrow = state
            .split_pane_with(
                first,
                Axis::Vertical,
                PaneKind::Terminal,
                SplitPlacement {
                    size: SplitSize::Cells(1),
                    ..SplitPlacement::default()
                },
            )
            .unwrap();
        let layout = state.windows[&window].layout.clone();
        let next_pane_id = state.next_pane_id;
        let next_split_id = state.next_split_id;
        let generation = state.generation();

        assert_eq!(
            state.split_pane(narrow, Axis::Vertical, PaneKind::Terminal),
            Err(ServerError::InvalidCommand(
                "no space for a new pane".to_owned()
            ))
        );
        assert_eq!(state.windows[&window].layout, layout);
        assert_eq!(state.windows[&window].panes.len(), 2);
        assert_eq!(state.next_pane_id, next_pane_id);
        assert_eq!(state.next_split_id, next_split_id);
        assert_eq!(state.generation(), generation);
        let next = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        assert_eq!(next, PaneId(next_pane_id));
        assert!(state.validate().is_ok());
    }

    #[test]
    fn remove_promotes_the_surviving_cell_subtree_without_reallocating_descendants() {
        for target_first in [true, false] {
            let mut state = MuxState::default();
            let (_, window, first) = state.create_session("work").unwrap();
            let second = state
                .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
                .unwrap();
            let (target, survivor, nested) = if target_first {
                let nested = state
                    .split_pane(second, Axis::Vertical, PaneKind::Terminal)
                    .unwrap();
                (first, second, nested)
            } else {
                let nested = state
                    .split_pane(first, Axis::Vertical, PaneKind::Terminal)
                    .unwrap();
                (second, first, nested)
            };
            assert_eq!(
                layout_splits(&state.windows[&window].layout),
                [SplitId(0), SplitId(1)]
            );

            state.kill_pane(target).unwrap();

            let layout = &state.windows[&window].layout;
            assert_eq!(layout_splits(layout), [SplitId(1)]);
            assert_eq!(pane_size(layout, survivor), (80, 12));
            assert_eq!(pane_size(layout, nested), (80, 11));
            assert!(state.validate().is_ok());
        }
    }

    #[test]
    fn stable_targets_and_names_resolve() {
        let mut state = MuxState::default();
        let (session, window, pane) = state.create_session("work").unwrap();
        assert_eq!(state.resolve_session(Some("work"), None).unwrap(), session);
        assert_eq!(
            state
                .resolve_window(Some("0"), Some(session), None)
                .unwrap(),
            window
        );
        assert_eq!(state.resolve_pane(Some("%0"), None, None).unwrap(), pane);
    }

    #[test]
    fn fnmatch_uses_tmux_flags_zero_metacharacters() {
        assert!(fnmatch("w[oa]rk", "work"));
        assert!(fnmatch("w[!x]rk", "work"));
        assert!(fnmatch("file?", "file1"));
        assert!(fnmatch("**/name", "path/to/name"));
        assert!(fnmatch(r"literal\*", "literal*"));
        assert!(!fnmatch("w[a-c]rk", "work"));
        assert!(!fnmatch("file?", "file10"));
    }

    #[test]
    fn session_targets_follow_id_exact_prefix_and_fnmatch_passes() {
        let mut state = MuxState::default();
        let (work, ..) = state.create_session("work").unwrap();
        let (workshop, ..) = state.create_session("workshop").unwrap();
        let (other, ..) = state.create_session("other").unwrap();
        let (slash, ..) = state.create_session("path/name").unwrap();

        assert_eq!(state.resolve_session(Some(""), Some(work)).unwrap(), work);
        assert_eq!(
            state
                .resolve_session(Some(&work.to_string()), Some(other))
                .unwrap(),
            work
        );
        assert_eq!(
            state
                .resolve_session(Some(&format!("={work}")), Some(other))
                .unwrap(),
            work
        );
        assert_eq!(
            state.resolve_session(Some("work"), None).unwrap(),
            work,
            "an exact name wins over the longer session it prefixes"
        );
        assert_eq!(
            state.resolve_session(Some("workshop"), None).unwrap(),
            workshop
        );
        assert_eq!(
            state.resolve_session(Some("works"), None).unwrap(),
            workshop
        );
        assert_eq!(state.resolve_session(Some("o"), None).unwrap(), other);
        assert_eq!(state.resolve_session(Some("oth?r"), None).unwrap(), other);
        assert_eq!(state.resolve_session(Some("path/*"), None).unwrap(), slash);

        let ambiguous = state.resolve_session(Some("wor"), None).unwrap_err();
        assert!(
            matches!(&ambiguous, ServerError::SessionNotFound(target) if target == "wor"),
            "{ambiguous:?}"
        );
        let ambiguous = state.resolve_session(Some("work*"), None).unwrap_err();
        assert!(matches!(ambiguous, ServerError::SessionNotFound(target) if target == "work*"));
        let exact_only = state.resolve_session(Some("=works"), None).unwrap_err();
        assert!(matches!(exact_only, ServerError::SessionNotFound(target) if target == "works"));
        let missing = state.resolve_session(Some("nope"), None).unwrap_err();
        assert!(matches!(missing, ServerError::SessionNotFound(target) if target == "nope"));
        let malformed = state.resolve_session(Some("$nope"), None).unwrap_err();
        assert!(matches!(malformed, ServerError::SessionNotFound(target) if target == "$nope"));
    }

    #[test]
    fn window_targets_follow_tokens_indexes_names_prefixes_and_fnmatch() {
        let mut state = MuxState::default();
        let (session, first, _) = state.create_session("work").unwrap();
        state.rename_window(first, "root").unwrap();
        let (alpha, _) = state
            .create_window(session, Some("alpha".to_owned()), PaneKind::Terminal)
            .unwrap();
        let (alpine, _) = state
            .create_window(session, Some("alpine".to_owned()), PaneKind::Terminal)
            .unwrap();
        let (caret, _) = state
            .create_window(session, Some("^".to_owned()), PaneKind::Terminal)
            .unwrap();
        state.set_window_index(caret, 9).unwrap();
        state.set_window_index(alpine, 7).unwrap();
        state.set_window_index(alpha, 3).unwrap();
        state.select_window(session, first).unwrap();

        for target in ["^", "{start}"] {
            assert_eq!(
                state
                    .resolve_window(Some(target), Some(session), Some(first))
                    .unwrap(),
                first
            );
        }
        for target in ["work:$", "{end}", "!", "{last}"] {
            assert_eq!(
                state
                    .resolve_window(Some(target), Some(session), Some(first))
                    .unwrap(),
                caret
            );
        }
        for (target, expected) in [
            ("+", alpha),
            ("{next}", alpha),
            ("+2", alpine),
            ("-", caret),
            ("{previous}", caret),
            ("-2", alpine),
            ("7", alpine),
            ("alph", alpha),
            ("a*ha", alpha),
            ("=^", caret),
            ("={start}", caret),
        ] {
            assert_eq!(
                state
                    .resolve_window(Some(target), Some(session), Some(first))
                    .unwrap(),
                expected,
                "{target}"
            );
        }
        assert_eq!(
            state
                .resolve_window(Some(&caret.to_string()), Some(session), Some(first))
                .unwrap(),
            caret
        );
        assert_eq!(
            state
                .resolve_window_index_target(Some("+5"), Some(session), Some(first))
                .unwrap(),
            (session, Some(5))
        );
        assert_eq!(
            state
                .resolve_window_index_target(Some("alph"), Some(session), Some(first))
                .unwrap(),
            (session, Some(3))
        );

        for target in ["alp", "a*"] {
            assert!(matches!(
                state.resolve_window(Some(target), Some(session), Some(first)),
                Err(ServerError::WindowNotFound(component)) if component == target
            ));
        }
        assert!(matches!(
            state.resolve_window(Some("work:=missing"), Some(session), Some(first)),
            Err(ServerError::WindowNotFound(component)) if component == "missing"
        ));

        let (fallback, fallback_window, _) = state.create_session("fallback").unwrap();
        assert_eq!(
            state
                .resolve_window(Some("=fallback"), Some(session), Some(first))
                .unwrap(),
            fallback_window
        );
        assert_eq!(
            state
                .resolve_window(Some(&fallback.to_string()), Some(session), Some(first))
                .unwrap(),
            fallback_window
        );

        let (lonely, lonely_window, _) = state.create_session("lonely").unwrap();
        assert!(matches!(
            state.resolve_window(Some("lonely:{last}"), Some(lonely), Some(lonely_window)),
            Err(ServerError::WindowNotFound(component)) if component == "!"
        ));
    }

    #[test]
    fn compound_window_targets_resolve_within_the_named_session() {
        let mut state = MuxState::default();
        let (named_session, named_window, _) = state.create_session("a").unwrap();
        state.rename_window(named_window, "b").unwrap();
        let (current_session, current_window, _) = state.create_session("current").unwrap();
        state.rename_window(current_window, "a:b").unwrap();

        assert_eq!(
            state
                .resolve_window(Some("a:b"), Some(current_session), Some(current_window),)
                .unwrap(),
            named_window,
            "the first colon selects session a instead of the current window named a:b"
        );
        assert_eq!(
            state
                .resolve_window(
                    Some(&format!("{named_session}:0")),
                    Some(current_session),
                    Some(current_window),
                )
                .unwrap(),
            named_window
        );
        assert_eq!(
            state
                .resolve_window(
                    Some(&format!("a:{named_window}")),
                    Some(current_session),
                    Some(current_window),
                )
                .unwrap(),
            named_window
        );
        assert_eq!(
            state
                .resolve_window(Some(":a:b"), None, Some(current_window))
                .unwrap(),
            current_window,
            "an empty session component keeps the current session"
        );
    }

    #[test]
    fn pane_indexes_and_compound_targets_resolve_without_weakening_absolute_ids() {
        let mut state = MuxState::default();
        let (session, first_window, first_pane) = state.create_session("a").unwrap();
        state.rename_window(first_window, "shell").unwrap();
        let (second_window, second_pane) = state
            .create_window(session, Some("b".to_owned()), PaneKind::Terminal)
            .unwrap();
        let third_pane = state
            .split_pane(second_pane, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let (_, _, absolute_pane) = state.create_session("other").unwrap();
        assert_eq!(absolute_pane, PaneId(3));

        for target in [".0", "0"] {
            assert_eq!(
                state
                    .resolve_pane(Some(target), Some(second_window), Some(second_pane))
                    .unwrap(),
                second_pane
            );
        }
        assert_eq!(
            state
                .resolve_pane(Some(".1"), Some(second_window), Some(second_pane))
                .unwrap(),
            third_pane
        );
        for target in ["a:b.1", ":b.1"] {
            assert_eq!(
                state
                    .resolve_pane(Some(target), Some(second_window), Some(second_pane))
                    .unwrap(),
                third_pane
            );
        }
        for target in ["a:0.0", "a:shell.0"] {
            assert_eq!(
                state
                    .resolve_pane(Some(target), Some(second_window), Some(second_pane))
                    .unwrap(),
                first_pane
            );
        }
        assert_eq!(
            state
                .resolve_pane(Some("%3"), Some(second_window), Some(second_pane))
                .unwrap(),
            absolute_pane,
            "absolute pane ids retain priority over index parsing"
        );
        for target in ["a", "b", "a:b"] {
            assert_eq!(
                state
                    .resolve_pane(Some(target), Some(second_window), Some(second_pane))
                    .unwrap(),
                third_pane
            );
        }
        for target in ["a:0", "a:shell"] {
            assert_eq!(
                state
                    .resolve_pane(Some(target), Some(second_window), Some(second_pane))
                    .unwrap(),
                first_pane
            );
        }
        assert!(matches!(
            state.resolve_pane(Some("a:b.9"), Some(second_window), Some(second_pane)),
            Err(ServerError::PaneNotFound(target)) if target == "9"
        ));
    }

    #[test]
    fn window_targets_accept_pane_forms_like_tmux() {
        let mut state = MuxState::default();
        let (session, first_window, _first_pane) = state.create_session("a").unwrap();
        state.rename_window(first_window, "shell").unwrap();
        let (second_window, second_pane) = state
            .create_window(session, Some("b".to_owned()), PaneKind::Terminal)
            .unwrap();
        for target in ["a:shell.0", "a:0.0", "0.0"] {
            assert_eq!(
                state
                    .resolve_window(Some(target), Some(session), Some(first_window))
                    .unwrap(),
                first_window
            );
        }
        assert_eq!(
            state
                .resolve_window(Some(&second_pane.to_string()), Some(session), None)
                .unwrap(),
            second_window
        );
        assert!(matches!(
            state.resolve_window(Some("a:shell.9"), Some(session), Some(first_window)),
            Err(ServerError::PaneNotFound(target)) if target == "9"
        ));
    }

    #[test]
    fn deeply_segmented_targets_do_not_recurse_between_resolvers() {
        let mut state = MuxState::default();
        let (session, window, pane) = state.create_session("work").unwrap();
        let target = format!("{}1", "1.".repeat(700));

        assert!(matches!(
            state.resolve_window(Some(&target), Some(session), Some(window)),
            Err(ServerError::WindowNotFound(_))
        ));
        assert!(matches!(
            state.resolve_pane(Some(&target), Some(window), Some(pane)),
            Err(ServerError::WindowNotFound(_))
        ));
    }

    #[test]
    fn split_ids_are_stable_and_exact_resize_targets_nested_boundaries() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let third = state
            .split_pane(second, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();

        let layout = &state.windows[&window].layout;
        assert_eq!(
            [
                pane_size(layout, first),
                pane_size(layout, second),
                pane_size(layout, third)
            ],
            [(40, 24), (19, 24), (19, 24)]
        );
        assert!((split_ratio(layout, SplitId(0)).unwrap() - 40.0 / 79.0).abs() < 1e-6);
        assert_eq!(split_ratio(layout, SplitId(1)), Some(0.5));
        assert_eq!(
            state.snapshot().sessions[0].windows[0].layout,
            layout.project()
        );

        state.resize_pane(second, Axis::Horizontal, 1).unwrap();
        let layout = &state.windows[&window].layout;
        assert_eq!(
            [
                pane_size(layout, first),
                pane_size(layout, second),
                pane_size(layout, third)
            ],
            [(40, 24), (20, 24), (18, 24)]
        );
        assert!((split_ratio(layout, SplitId(0)).unwrap() - 40.0 / 79.0).abs() < 1e-6);
        assert!((split_ratio(layout, SplitId(1)).unwrap() - 20.0 / 38.0).abs() < 1e-6);

        assert!(state.resize_split(window, SplitId(0), 0.72).unwrap());
        assert!(!state.resize_split(window, SplitId(0), 0.72).unwrap());
        let layout = &state.windows[&window].layout;
        assert_eq!(
            [
                pane_size(layout, first),
                pane_size(layout, second),
                pane_size(layout, third)
            ],
            [(57, 24), (3, 24), (18, 24)]
        );
        assert!((split_ratio(layout, SplitId(0)).unwrap() - 57.0 / 79.0).abs() < 1e-6);
        assert!((split_ratio(layout, SplitId(1)).unwrap() - 3.0 / 21.0).abs() < 1e-6);
        assert!(matches!(
            state.resize_split(window, SplitId(0), f32::NAN),
            Err(ServerError::InvalidCommand(message)) if message.contains("finite")
        ));

        state.kill_pane(third).unwrap();
        state
            .split_pane(first, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let splits = layout_splits(&state.windows[&window].layout);
        assert!(splits.contains(&SplitId(2)));
        assert!(!splits.contains(&SplitId(1)));
        assert!(state.validate().is_ok());
    }

    #[test]
    fn named_layouts_rebuild_only_the_cells_with_tmux_geometry() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let mut target = first;
        for index in 1..5 {
            target = state
                .split_pane(
                    target,
                    Axis::Horizontal,
                    if index == 2 {
                        PaneKind::Browser(BrowserDescriptor::single(
                            "https://layout.example".to_owned(),
                            "default".to_owned(),
                        ))
                    } else {
                        PaneKind::Terminal
                    },
                )
                .unwrap();
        }
        let panes = state.windows[&window].panes.clone();
        let pane_order = state.windows[&window].pane_order.clone();
        let mut retired_splits = layout_splits(&state.windows[&window].layout);
        for (preset, expected) in [
            (
                LayoutPreset::EvenHorizontal,
                [(16, 24), (15, 24), (15, 24), (15, 24), (15, 24)],
            ),
            (
                LayoutPreset::EvenVertical,
                [(80, 4), (80, 4), (80, 4), (80, 4), (80, 4)],
            ),
            (
                LayoutPreset::MainHorizontal,
                [(80, 22), (20, 1), (19, 1), (19, 1), (19, 1)],
            ),
            (
                LayoutPreset::MainHorizontalMirrored,
                [(80, 22), (20, 1), (19, 1), (19, 1), (19, 1)],
            ),
            (
                LayoutPreset::MainVertical,
                [(78, 24), (1, 6), (1, 5), (1, 5), (1, 5)],
            ),
            (
                LayoutPreset::MainVerticalMirrored,
                [(78, 24), (1, 6), (1, 5), (1, 5), (1, 5)],
            ),
            (
                LayoutPreset::Tiled,
                [(39, 7), (40, 7), (39, 7), (40, 7), (80, 8)],
            ),
        ] {
            state
                .select_layout(window, preset, &PresetOptions::default())
                .unwrap();
            let arranged = &state.windows[&window];
            assert_eq!(arranged.panes, panes);
            assert_eq!(arranged.zoomed_pane, None);
            assert_eq!(arranged.last_layout, Some(preset));
            assert_eq!(
                pane_order
                    .iter()
                    .map(|pane| pane_size(&arranged.layout, *pane))
                    .collect::<Vec<_>>(),
                expected.to_vec(),
                "{}",
                preset.name()
            );
            let new_splits = layout_splits(&arranged.layout);
            assert_eq!(new_splits.len(), panes.len() - 1);
            assert!(
                new_splits
                    .iter()
                    .all(|split| !retired_splits.contains(split))
            );
            retired_splits.extend(new_splits);
        }

        state
            .select_layout(
                window,
                LayoutPreset::MainHorizontal,
                &PresetOptions::default(),
            )
            .unwrap();
        state
            .select_layout(
                window,
                LayoutPreset::MainVerticalMirrored,
                &PresetOptions::default(),
            )
            .unwrap();

        state.swap_panes(first, target, true, false).unwrap();
        let reordered = state.windows[&window].pane_order.clone();
        state.restore_previous_layout(window).unwrap();
        assert_eq!(layout_panes(&state.windows[&window].layout), reordered);
        let projected = state.windows[&window].layout.project();
        assert!(matches!(
            &projected,
            LayoutNode::Split {
                axis: Axis::Vertical,
                first: main,
                ..
            } if matches!(main.as_ref(), LayoutNode::Pane(pane) if *pane == target)
        ));

        let tiled_order = state.windows[&window].pane_order.clone();
        state
            .select_layout(window, LayoutPreset::Tiled, &PresetOptions::default())
            .unwrap();
        assert_eq!(
            tiled_order
                .iter()
                .map(|pane| pane_size(&state.windows[&window].layout, *pane))
                .collect::<Vec<_>>(),
            [(39, 7), (40, 7), (39, 7), (40, 7), (80, 8)].to_vec()
        );
        assert!(state.validate().is_ok());
    }

    #[test]
    fn serialized_layouts_adopt_extent_assign_pane_order_and_trim_bottom_right() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let third = state
            .split_pane(second, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let original = state.windows[&window].layout.clone();
        let retired = layout_splits(&original);
        let input = "b78d,120x30,0,0{50x30,0,0,9,69x30,51,0[69x14,51,0,8,69x15,51,15,7]}";

        state.select_layout_string(window, input).unwrap();
        let applied = state.windows[&window].layout.clone();
        assert_eq!(applied.extent(), (120, 30));
        assert_eq!(applied.panes_in_order(), [first, second, third]);
        assert_eq!(pane_size(&applied, first), (50, 30));
        assert_eq!(pane_size(&applied, second), (69, 14));
        assert_eq!(pane_size(&applied, third), (69, 15));
        assert!(
            layout_splits(&applied)
                .iter()
                .all(|split| !retired.contains(split))
        );

        state.restore_previous_layout(window).unwrap();
        assert!(same_layout_geometry(
            &state.windows[&window].layout,
            &original
        ));
        state.restore_previous_layout(window).unwrap();
        assert!(same_layout_geometry(
            &state.windows[&window].layout,
            &applied
        ));

        state.kill_pane(second).unwrap();
        state
            .select_layout_string(
                window,
                "e7f0,100x20,0,0[100x9,0,0{49x9,0,0,50,50x9,50,0,51},100x10,0,10,52]",
            )
            .unwrap();
        let trimmed = &state.windows[&window].layout;
        assert_eq!(trimmed.extent(), (100, 20));
        assert_eq!(trimmed.panes_in_order(), [first, third]);
        assert_eq!(pane_size(trimmed, first), (49, 20));
        assert_eq!(pane_size(trimmed, third), (50, 20));
        assert!(state.validate().is_ok());
    }

    #[test]
    fn serialized_layout_have_need_error_is_atomic() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let before = state.windows[&window].layout.clone();
        let generation = state.generation();
        let next_split_id = state.next_split_id;
        let input = "b25d,80x24,0,0,0";

        assert_eq!(
            state.select_layout_string(window, input),
            Err(ServerError::InvalidCommand(format!(
                "have 2 panes but need 1: {input}"
            )))
        );
        assert_eq!(state.windows[&window].layout, before);
        assert_eq!(state.generation(), generation);
        assert_eq!(state.next_split_id, next_split_id);
    }

    #[test]
    fn layout_cycle_restore_and_spread_are_reversible_and_atomic() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let third = state
            .split_pane(second, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let original = state.windows[&window].layout.clone();
        let mut retired_splits = layout_splits(&original);

        assert_eq!(
            state
                .cycle_layout(window, 1, &PresetOptions::default())
                .unwrap(),
            LayoutPreset::EvenHorizontal
        );
        let even = state.windows[&window].layout.clone();
        retired_splits.extend(layout_splits(&even));
        assert_ne!(even, original);
        state.restore_previous_layout(window).unwrap();
        assert!(same_layout_geometry(
            &state.windows[&window].layout,
            &original
        ));
        let restored_splits = layout_splits(&state.windows[&window].layout);
        assert!(
            restored_splits
                .iter()
                .all(|split| !retired_splits.contains(split))
        );
        retired_splits.extend(restored_splits);
        state.restore_previous_layout(window).unwrap();
        assert!(same_layout_geometry(&state.windows[&window].layout, &even));
        let restored_splits = layout_splits(&state.windows[&window].layout);
        assert!(
            restored_splits
                .iter()
                .all(|split| !retired_splits.contains(split))
        );

        state.resize_pane(first, Axis::Horizontal, -4).unwrap();
        state.spread_layout(first).unwrap();
        let spread = &state.windows[&window];
        assert_eq!(
            spread
                .pane_order
                .iter()
                .map(|pane| pane_size(&spread.layout, *pane))
                .collect::<Vec<_>>(),
            [(26, 24), (26, 24), (26, 24)]
        );

        state
            .select_layout(window, LayoutPreset::Tiled, &PresetOptions::default())
            .unwrap();
        assert_eq!(
            state
                .cycle_layout(window, 1, &PresetOptions::default())
                .unwrap(),
            LayoutPreset::EvenHorizontal
        );
        assert_eq!(
            state
                .cycle_layout(window, -1, &PresetOptions::default())
                .unwrap(),
            LayoutPreset::Tiled
        );
        let fourth = state
            .split_pane(third, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let before_failed_restore = state.windows[&window].layout.clone();
        let generation = state.generation();
        assert!(matches!(
            state.restore_previous_layout(window),
            Err(ServerError::InvalidCommand(_))
        ));
        assert_eq!(state.windows[&window].layout, before_failed_restore);
        assert_eq!(state.generation(), generation);
        assert!(state.windows[&window].panes.contains_key(&fourth));
        assert!(state.validate().is_ok());
    }

    #[test]
    fn directional_navigation_uses_geometry_wrapping_and_mru_tie_breaking() {
        let mut state = MuxState::default();
        let (_, window, left) = state.create_session("work").unwrap();
        let right_top = state
            .split_pane(left, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let right_bottom = state
            .split_pane(right_top, Axis::Vertical, PaneKind::Terminal)
            .unwrap();

        assert_eq!(
            state
                .pane_in_direction(right_top, PaneDirection::Down)
                .unwrap(),
            Some(right_bottom)
        );
        assert_eq!(
            state
                .pane_in_direction(right_bottom, PaneDirection::Up)
                .unwrap(),
            Some(right_top)
        );
        assert_eq!(
            state
                .pane_in_direction(right_bottom, PaneDirection::Right)
                .unwrap(),
            Some(left)
        );
        assert_eq!(
            state.pane_in_direction(left, PaneDirection::Up).unwrap(),
            None
        );

        state.select_pane(left).unwrap();
        assert_eq!(
            state.pane_in_direction(left, PaneDirection::Right).unwrap(),
            Some(right_bottom),
            "the most recently active adjacent pane wins"
        );
        state.select_pane(right_top).unwrap();
        state.select_pane(left).unwrap();
        assert_eq!(
            state.pane_in_direction(left, PaneDirection::Right).unwrap(),
            Some(right_top)
        );

        state.resize_split(window, SplitId(0), 0.72).unwrap();
        state.resize_split(window, SplitId(1), 0.35).unwrap();
        assert_eq!(
            state
                .pane_in_direction(right_top, PaneDirection::Left)
                .unwrap(),
            Some(left)
        );
        assert!(state.validate().is_ok());
    }

    #[test]
    fn kill_and_move_window_touch_only_newly_selected_windows() {
        let mut state = MuxState::default();
        let (source_session, fallback, _) = state.create_session("source").unwrap();
        let (current, _) = state
            .create_window(source_session, None, PaneKind::Terminal)
            .unwrap();
        let fallback_activity = state.windows[&fallback].activity;

        state.kill_window(current).unwrap();

        assert_eq!(state.sessions[&source_session].active_window, fallback);
        assert!(state.windows[&fallback].activity > fallback_activity);

        let (inactive, _) = state
            .create_window_at(source_session, None, None, PaneKind::Terminal, false)
            .unwrap();
        let fallback_activity = state.windows[&fallback].activity;
        state.kill_window(inactive).unwrap();
        assert_eq!(state.windows[&fallback].activity, fallback_activity);

        let (moving, _) = state
            .create_window(source_session, None, PaneKind::Terminal)
            .unwrap();
        let fallback_activity = state.windows[&fallback].activity;
        state
            .move_window(moving, source_session, 2, false, false)
            .unwrap();
        assert_eq!(state.sessions[&source_session].active_window, fallback);
        assert!(state.windows[&fallback].activity > fallback_activity);

        let moving_activity = state.windows[&moving].activity;
        state
            .move_window(moving, source_session, 3, false, true)
            .unwrap();
        assert_eq!(state.sessions[&source_session].active_window, moving);
        assert!(state.windows[&moving].activity > moving_activity);

        let moving_activity = state.windows[&moving].activity;
        state
            .move_window(moving, source_session, 4, false, true)
            .unwrap();
        assert!(state.windows[&moving].activity > moving_activity);

        let (destination_session, _, _) = state.create_session("destination").unwrap();
        let moving_activity = state.windows[&moving].activity;
        let fallback_activity = state.windows[&fallback].activity;
        state
            .move_window(moving, destination_session, 1, false, true)
            .unwrap();
        assert_eq!(state.sessions[&destination_session].active_window, moving);
        assert_eq!(state.sessions[&source_session].active_window, fallback);
        assert!(state.windows[&moving].activity > moving_activity);
        assert!(state.windows[&fallback].activity > fallback_activity);
        assert!(state.windows[&fallback].activity > state.windows[&moving].activity);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn swap_window_updates_activity_only_when_destination_selection_changes() {
        let mut state = MuxState::default();
        let (session, source, _) = state.create_session("work").unwrap();
        let (target, _) = state
            .create_window_at(session, None, None, PaneKind::Terminal, false)
            .unwrap();
        let source_activity = state.windows[&source].activity;
        let target_activity = state.windows[&target].activity;
        let next_sort_point = state.next_sort_point;

        state.swap_windows(source, target, false).unwrap();

        assert_eq!(state.sessions[&session].active_window, target);
        assert_eq!(state.windows[&source].activity, source_activity);
        assert_eq!(state.windows[&target].activity, target_activity);
        assert_eq!(state.next_sort_point, next_sort_point);

        state.select_window(session, source).unwrap();
        let source_activity = state.windows[&source].activity;
        let target_activity = state.windows[&target].activity;
        let next_sort_point = state.next_sort_point;
        state.swap_windows(source, target, true).unwrap();

        assert_eq!(state.sessions[&session].active_window, source);
        assert!(state.windows[&source].activity > source_activity);
        assert_eq!(state.windows[&target].activity, target_activity);
        assert_eq!(state.next_sort_point, next_sort_point + 1);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn pane_swaps_preserve_layout_identity_active_slots_and_cross_window_state() {
        let mut state = MuxState::default();
        let (session, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let third = state
            .split_pane(second, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let split_ids = layout_splits(&state.windows[&window].layout);

        assert_eq!(state.previous_pane(first).unwrap(), third);
        assert_eq!(state.next_pane(third).unwrap(), first);
        state.toggle_zoom(third).unwrap();
        state.swap_panes(second, third, false, true).unwrap();
        assert_eq!(
            layout_panes(&state.windows[&window].layout),
            [first, third, second]
        );
        assert_eq!(state.windows[&window].pane_order, [first, third, second]);
        assert_eq!(state.windows[&window].active_pane, third);
        assert_eq!(state.windows[&window].zoomed_pane, Some(third));
        let current_split_ids = layout_splits(&state.windows[&window].layout);
        assert_eq!(current_split_ids, split_ids, "split IDs remain stable");

        let second_point = state.windows[&window].panes[&second].active_point;
        let third_point = state.windows[&window].panes[&third].active_point;
        state.swap_panes(third, second, true, false).unwrap();
        assert_eq!(
            layout_panes(&state.windows[&window].layout),
            [first, second, third]
        );
        assert_eq!(state.windows[&window].pane_order, [first, second, third]);
        assert_eq!(
            state.windows[&window].active_pane, third,
            "tmux reselects the source after briefly selecting the target"
        );
        let second_point_after = state.windows[&window].panes[&second].active_point;
        let third_point_after = state.windows[&window].panes[&third].active_point;
        assert!(second_point_after > second_point);
        assert!(third_point_after > third_point);
        assert!(third_point_after > second_point_after);
        assert_eq!(state.windows[&window].zoomed_pane, None);

        let (other_window, other) = state
            .create_window(session, Some("other".to_owned()), PaneKind::Terminal)
            .unwrap();
        state.swap_panes(third, other, false, false).unwrap();
        assert!(state.windows[&window].panes.contains_key(&other));
        assert!(!state.windows[&window].panes.contains_key(&third));
        assert!(state.windows[&other_window].panes.contains_key(&third));
        assert_eq!(state.windows[&window].active_pane, other);
        assert_eq!(state.windows[&other_window].active_pane, third);
        assert_eq!(layout_panes(&state.windows[&window].layout)[2], other);
        assert_eq!(
            state.windows[&other_window].layout.project(),
            LayoutNode::Pane(third)
        );
        assert_eq!(state.windows[&window].pane_order, [first, second, other]);
        assert_eq!(state.windows[&other_window].pane_order, [third]);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn window_rotation_moves_surfaces_through_stable_layout_slots_and_preserves_zoom() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(
                first,
                Axis::Horizontal,
                PaneKind::Browser(BrowserDescriptor::single(
                    "https://rotate.example".to_owned(),
                    "default".to_owned(),
                )),
            )
            .unwrap();
        let third = state
            .split_pane(second, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let fourth = state
            .split_pane(first, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        state
            .select_layout(
                window,
                LayoutPreset::MainHorizontalMirrored,
                &PresetOptions::default(),
            )
            .unwrap();
        state.select_pane(third).unwrap();
        state.toggle_zoom(third).unwrap();

        let panes = state.windows[&window].panes.clone();
        let order = state.windows[&window].pane_order.clone();
        assert_eq!(order, [first, fourth, second, third]);
        let layout = state.windows[&window].layout.clone();
        let layout_order = layout_panes(&layout);
        let split_ids = layout_splits(&layout);

        let rotated = state.rotate_window(window, false, true).unwrap();
        assert_eq!(rotated, first);
        assert_eq!(state.windows[&window].active_pane, first);
        assert_eq!(state.windows[&window].zoomed_pane, Some(first));
        assert_eq!(
            state.windows[&window].pane_order,
            [fourth, second, third, first]
        );
        let replacements = BTreeMap::from([
            (first, fourth),
            (fourth, second),
            (second, third),
            (third, first),
        ]);
        assert_eq!(
            layout_panes(&state.windows[&window].layout),
            layout_order
                .iter()
                .map(|pane| replacements[pane])
                .collect::<Vec<_>>()
        );
        let rotated_split_ids = layout_splits(&state.windows[&window].layout);
        assert_eq!(rotated_split_ids, split_ids);
        assert!(state.windows[&window].panes[&first].active_point > panes[&first].active_point);
        for pane in [second, third, fourth] {
            assert_eq!(state.windows[&window].panes[&pane], panes[&pane]);
        }

        let restored = state.rotate_window(window, true, true).unwrap();
        assert_eq!(restored, third);
        assert_eq!(state.windows[&window].pane_order, order);
        assert_eq!(state.windows[&window].layout, layout);
        assert_eq!(state.windows[&window].active_pane, third);
        assert_eq!(state.windows[&window].zoomed_pane, Some(third));

        state.rotate_window(window, false, false).unwrap();
        assert_eq!(state.windows[&window].zoomed_pane, None);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn break_and_join_follow_tmux_window_activity_transitions() {
        let mut state = MuxState::default();
        let (session, original_window, first) = state.create_session("work").unwrap();
        let moving = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();

        let next_sort_point = state.next_sort_point;
        let broken_window = state
            .break_pane(moving, session, None, None, false)
            .unwrap();
        assert_eq!(state.next_sort_point, next_sort_point + 2);
        assert!(
            state.windows[&broken_window].activity > state.windows[&broken_window].created,
            "selecting the newly created window updates its activity"
        );
        assert_eq!(state.sessions[&session].active_window, broken_window);

        let target_activity = state.windows[&original_window].activity;
        let next_sort_point = state.next_sort_point;
        state
            .join_pane(
                moving,
                first,
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                false,
            )
            .unwrap();
        assert_eq!(state.next_sort_point, next_sort_point + 2);
        assert!(!state.windows.contains_key(&broken_window));
        assert_eq!(state.sessions[&session].active_window, original_window);
        assert!(state.windows[&original_window].activity > target_activity);

        let next_sort_point = state.next_sort_point;
        let detached_window = state.break_pane(moving, session, None, None, true).unwrap();
        assert_eq!(state.next_sort_point, next_sort_point + 1);
        assert_eq!(
            state.windows[&detached_window].activity,
            state.windows[&detached_window].created
        );
        state.select_window(session, detached_window).unwrap();
        let target_activity = state.windows[&original_window].activity;
        let moving_point = state.windows[&detached_window].panes[&moving].active_point;
        let next_sort_point = state.next_sort_point;

        state
            .join_pane(
                moving,
                first,
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                true,
            )
            .unwrap();

        assert_eq!(state.next_sort_point, next_sort_point + 1);
        assert!(!state.windows.contains_key(&detached_window));
        assert_eq!(state.sessions[&session].active_window, original_window);
        assert_eq!(state.windows[&original_window].active_pane, first);
        assert_eq!(
            state.windows[&original_window].panes[&moving].active_point,
            moving_point
        );
        assert!(state.windows[&original_window].activity > target_activity);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn break_and_join_reparent_existing_panes_without_reusing_layout_ids() {
        let mut state = MuxState::default();
        let (session, original_window, first) = state.create_session("work").unwrap();
        let browser = state
            .split_pane(
                first,
                Axis::Horizontal,
                PaneKind::Browser(BrowserDescriptor::single(
                    "https://example.com".to_owned(),
                    "default".to_owned(),
                )),
            )
            .unwrap();
        let third = state
            .split_pane(browser, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let next_split = SplitId(state.next_split_id);

        let broken_window = state
            .break_pane(browser, session, None, Some("web".to_owned()), false)
            .unwrap();
        assert_eq!(
            state.windows[&broken_window].layout.project(),
            LayoutNode::Pane(browser)
        );
        assert_eq!(state.windows[&broken_window].name, "web");
        assert!(matches!(
            state.windows[&broken_window].panes[&browser].kind,
            PaneKind::Browser(_)
        ));
        assert_eq!(state.sessions[&session].active_window, broken_window);
        assert_eq!(
            layout_panes(&state.windows[&original_window].layout),
            [first, third]
        );
        assert_eq!(state.windows[&original_window].pane_order, [first, third]);
        assert_eq!(state.windows[&broken_window].pane_order, [browser]);

        state
            .join_pane(
                browser,
                first,
                Axis::Horizontal,
                SplitSize::Percent(30),
                true,
                false,
                false,
            )
            .unwrap();
        assert!(!state.windows.contains_key(&broken_window));
        assert_eq!(
            layout_panes(&state.windows[&original_window].layout),
            [browser, first, third]
        );
        assert_eq!(
            state.windows[&original_window].pane_order,
            [first, browser, third]
        );
        assert_eq!(
            [browser, first, third]
                .map(|pane| { pane_size(&state.windows[&original_window].layout, pane) }),
            [(12, 24), (27, 24), (39, 24)]
        );
        assert!(layout_splits(&state.windows[&original_window].layout).contains(&next_split));
        assert_eq!(state.windows[&original_window].active_pane, browser);

        let second_break = state
            .break_pane(browser, session, None, None, true)
            .unwrap();
        state
            .join_pane(
                browser,
                third,
                Axis::Vertical,
                SplitSize::Percent(25),
                false,
                true,
                true,
            )
            .unwrap();
        assert!(!state.windows.contains_key(&second_break));
        assert_eq!(
            [first, third, browser]
                .map(|pane| { pane_size(&state.windows[&original_window].layout, pane) }),
            [(40, 17), (39, 17), (80, 6)]
        );
        assert!(matches!(
            state.windows[&original_window].layout.project(),
            LayoutNode::Split {
                axis: Axis::Vertical,
                second,
                ..
            } if second.as_ref() == &LayoutNode::Pane(browser)
        ));
        assert_eq!(
            state.windows[&original_window].pane_order,
            [first, third, browser]
        );
        assert!(state.validate().is_ok());
    }

    #[test]
    fn breaking_a_last_pane_moves_its_window_and_retires_the_source_session() {
        let mut state = MuxState::default();
        let (source_session, source_window, source) = state.create_session("source").unwrap();
        let (destination_session, destination_window, destination) =
            state.create_session("destination").unwrap();
        state
            .rename_window(source_window, "kept")
            .expect("rename source window");
        let source_layout = state.windows[&source_window].layout.clone();

        let broken = state
            .break_pane(source, destination_session, None, None, true)
            .unwrap();

        assert_eq!(broken, source_window);
        assert!(!state.sessions.contains_key(&source_session));
        assert_eq!(state.window_for_pane(source), Some(source_window));
        assert_eq!(state.window_for_pane(destination), Some(destination_window));
        assert_eq!(state.windows[&source_window].name, "kept");
        assert_eq!(state.windows[&source_window].layout, source_layout);
        assert_eq!(state.windows[&source_window].session, destination_session);
        assert_eq!(
            state.sessions[&destination_session].active_window,
            destination_window
        );
        assert!(
            state.sessions[&destination_session]
                .windows
                .contains(&source_window)
        );
        assert!(state.validate().is_ok());
    }

    #[test]
    fn failed_cross_window_join_restores_the_source_and_target_layouts() {
        let mut state = MuxState::default();
        let (session, target_window, target) = state.create_session("work").unwrap();
        let (source_window, source) = state
            .create_window(session, Some("source".to_owned()), PaneKind::Terminal)
            .unwrap();
        state
            .windows
            .get_mut(&target_window)
            .unwrap()
            .layout
            .resize(80, 1);
        let target_layout = state.windows[&target_window].layout.clone();
        let source_layout = state.windows[&source_window].layout.clone();
        let next_split_id = state.next_split_id;
        let generation = state.generation();

        assert_eq!(
            state.join_pane(
                source,
                target,
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                false,
            ),
            Err(ServerError::InvalidCommand(
                "no space for a new pane".to_owned()
            ))
        );
        assert_eq!(state.windows[&target_window].layout, target_layout);
        assert_eq!(state.windows[&source_window].layout, source_layout);
        assert!(state.windows[&source_window].panes.contains_key(&source));
        assert!(!state.windows[&target_window].panes.contains_key(&source));
        assert_eq!(state.next_split_id, next_split_id);
        assert_eq!(state.generation(), generation);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn cross_session_join_keeps_the_source_session_nonempty() {
        let mut state = MuxState::default();
        let (source_session, _, source_base) = state.create_session("source").unwrap();
        let moving = state
            .split_pane(source_base, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let moving_window = state
            .break_pane(moving, source_session, None, None, true)
            .unwrap();
        let (target_session, target_window, target) = state.create_session("target").unwrap();

        state
            .join_pane(
                moving,
                target,
                Axis::Vertical,
                SplitSize::Default,
                false,
                false,
                false,
            )
            .unwrap();
        assert!(!state.windows.contains_key(&moving_window));
        assert!(state.sessions.contains_key(&source_session));
        assert_eq!(state.window_for_pane(moving), Some(target_window));
        assert_eq!(state.windows[&target_window].session, target_session);
        assert_eq!(state.windows[&target_window].pane_order, [target, moving]);
        state
            .select_layout(
                target_window,
                LayoutPreset::MainHorizontal,
                &PresetOptions::default(),
            )
            .unwrap();
        assert!(matches!(
            state.windows[&target_window].layout.project(),
            LayoutNode::Split {
                first,
                ..
            } if matches!(first.as_ref(), LayoutNode::Pane(pane) if *pane == target)
        ));

        let (last_session, _, last_pane) = state.create_session("last").unwrap();
        assert!(matches!(
            state.join_pane(
                last_pane,
                target,
                Axis::Horizontal,
                SplitSize::Default,
                false,
                false,
                false,
            ),
            Err(ServerError::InvalidCommand(message)) if message.contains("last window")
        ));
        assert!(state.sessions.contains_key(&last_session));
        assert!(state.validate().is_ok());
    }

    #[test]
    fn same_window_join_reparents_a_pane_beside_the_target() {
        for (axis, before, expected_layout, expected_order) in [
            (Axis::Horizontal, true, [3, 1, 2], [1, 3, 2]),
            (Axis::Horizontal, false, [1, 3, 2], [1, 3, 2]),
            (Axis::Vertical, true, [3, 1, 2], [1, 3, 2]),
            (Axis::Vertical, false, [1, 3, 2], [1, 3, 2]),
        ] {
            let mut state = MuxState::default();
            let (_, window, first) = state.create_session("work").unwrap();
            let second = state
                .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
                .unwrap();
            let third = state
                .split_pane(second, Axis::Vertical, PaneKind::Terminal)
                .unwrap();
            let panes = [first, second, third];
            let expected_layout = expected_layout.map(|index| panes[index - 1]);
            let expected_order = expected_order.map(|index| panes[index - 1]);
            state.select_pane(second).unwrap();

            let predicted = joined_layout(
                &state.windows[&window].layout.project(),
                third,
                first,
                SplitId(u64::MAX),
                axis,
                0.5,
                before,
            )
            .expect("both leaves are in the window");
            state
                .join_pane(third, first, axis, SplitSize::Default, before, false, true)
                .unwrap();

            let layout = &state.windows[&window].layout;
            assert_eq!(layout_panes(layout), expected_layout);
            assert!(same_projected_geometry(layout, &predicted));
            assert_eq!(state.windows[&window].pane_order, expected_order);
            assert_eq!(state.windows[&window].active_pane, second);
            assert!(state.validate().is_ok());
        }
    }

    #[test]
    fn same_window_join_matches_tmux_active_pane_transitions() {
        for detached in [true, false] {
            let mut state = MuxState::default();
            let (_, window, first) = state.create_session("work").unwrap();
            let second = state
                .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
                .unwrap();
            let source = state
                .split_pane(second, Axis::Vertical, PaneKind::Terminal)
                .unwrap();
            state.select_pane(second).unwrap();
            state.select_pane(source).unwrap();
            let source_point = state.windows[&window].panes[&source].active_point;
            let fallback_point = state.windows[&window].panes[&second].active_point;
            let next_sort_point = state.next_sort_point;

            state
                .join_pane(
                    source,
                    first,
                    Axis::Horizontal,
                    SplitSize::Default,
                    false,
                    false,
                    detached,
                )
                .unwrap();

            let expected_active = if detached { second } else { source };
            assert_eq!(state.windows[&window].active_pane, expected_active);
            assert_eq!(
                state.windows[&window].panes[&second].active_point, fallback_point,
                "losing the active source uses the fallback without selecting it"
            );
            if detached {
                assert_eq!(
                    state.windows[&window].panes[&source].active_point,
                    source_point
                );
                assert_eq!(state.next_sort_point, next_sort_point);
            } else {
                assert!(state.windows[&window].panes[&source].active_point > source_point);
                assert_eq!(state.next_sort_point, next_sort_point + 1);
            }
            assert!(state.validate().is_ok());
        }
    }

    #[test]
    fn joining_the_last_sibling_splits_the_promoted_root() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();

        let predicted = joined_layout(
            &state.windows[&window].layout.project(),
            second,
            first,
            SplitId(u64::MAX),
            Axis::Vertical,
            0.5,
            true,
        )
        .expect("both leaves are in the window");
        state
            .join_pane(
                second,
                first,
                Axis::Vertical,
                SplitSize::Default,
                true,
                false,
                true,
            )
            .unwrap();

        let layout = &state.windows[&window].layout;
        assert!(matches!(
            layout.project(),
            LayoutNode::Split { axis: Axis::Vertical, first: top, second: bottom, .. }
                if top.as_ref() == &LayoutNode::Pane(second)
                    && bottom.as_ref() == &LayoutNode::Pane(first)
        ));
        assert!(same_projected_geometry(layout, &predicted));
        assert_eq!(state.windows[&window].pane_order, [first, second]);
        assert_eq!(
            joined_layout(
                &layout.project(),
                first,
                first,
                SplitId(u64::MAX),
                Axis::Vertical,
                0.5,
                true
            ),
            None
        );
        assert!(state.validate().is_ok());
    }

    #[test]
    fn swapped_layout_matches_the_swap_mutation() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let third = state
            .split_pane(second, Axis::Vertical, PaneKind::Terminal)
            .unwrap();

        let predicted = swapped_layout(&state.windows[&window].layout.project(), first, third);
        state.swap_panes(first, third, true, false).unwrap();

        assert_eq!(state.windows[&window].layout.project(), predicted);
        assert_eq!(
            layout_panes(&state.windows[&window].layout),
            [third, second, first]
        );
        assert!(state.validate().is_ok());
    }

    #[test]
    fn zoom_tracks_the_active_pane_and_layout_mutations_unzoom() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        state.toggle_zoom(first).unwrap();
        assert_eq!(state.windows[&window].zoomed_pane, None);

        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        state.toggle_zoom(first).unwrap();
        assert_eq!(state.windows[&window].active_pane, first);
        assert_eq!(state.windows[&window].zoomed_pane, Some(first));

        state.select_pane(second).unwrap();
        assert_eq!(state.windows[&window].zoomed_pane, None);
        state.toggle_zoom(second).unwrap();
        state.select_pane_with_zoom(first, true).unwrap();
        assert_eq!(state.windows[&window].zoomed_pane, Some(first));

        state.resize_pane(first, Axis::Horizontal, 1).unwrap();
        assert_eq!(state.windows[&window].zoomed_pane, None);
        state.toggle_zoom(first).unwrap();
        state
            .split_pane(first, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        assert_eq!(state.windows[&window].zoomed_pane, None);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn synchronized_input_resolves_global_window_and_pane_inheritance() {
        let mut state = MuxState::default();
        let (_, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();

        assert_eq!(state.synchronized_input_targets(first).unwrap(), [first]);
        state.set_global_synchronize_panes(true);
        assert_eq!(
            state.synchronized_input_targets(first).unwrap(),
            [first, second]
        );

        state
            .set_pane_synchronize_panes(second, Some(false))
            .unwrap();
        assert_eq!(state.synchronized_input_targets(first).unwrap(), [first]);
        assert_eq!(state.synchronized_input_targets(second).unwrap(), [second]);

        state.clear_pane_synchronize_overrides(window).unwrap();
        state
            .set_window_synchronize_panes(window, Some(false))
            .unwrap();
        state
            .set_pane_synchronize_panes(second, Some(true))
            .unwrap();
        assert_eq!(state.synchronized_input_targets(first).unwrap(), [first]);
        assert_eq!(state.synchronized_input_targets(second).unwrap(), [second]);

        state.set_window_synchronize_panes(window, None).unwrap();
        let panes = &state.snapshot().sessions[0].windows[0].panes;
        assert!(panes[&first].synchronized_input);
        assert!(panes[&second].synchronized_input);
        assert_eq!(
            state.synchronized_input_targets(second).unwrap(),
            [first, second]
        );
    }

    #[test]
    fn pane_bells_ride_the_snapshot_and_only_move_on_a_transition() {
        let mut state = MuxState::default();
        let (_, _, pane) = state.create_session("work").unwrap();
        let bell = |state: &MuxState| state.snapshot().sessions[0].windows[0].panes[&pane].bell;
        assert!(!bell(&state));

        let generation = state.generation();
        assert!(state.set_pane_bell(pane, true));
        assert!(bell(&state));
        assert!(state.generation() > generation);

        let generation = state.generation();
        assert!(!state.set_pane_bell(pane, true));
        assert_eq!(state.generation(), generation);

        assert!(state.set_pane_bell(pane, false));
        assert!(!bell(&state));
        assert!(!state.set_pane_bell(pane, false));

        assert!(!state.set_pane_bell(PaneId(9999), false));
    }

    #[test]
    fn window_alert_flags_move_on_transition_and_clear_on_activation() {
        let mut state = MuxState::default();
        let (session, first, _) = state.create_session("work").unwrap();
        let (second, _) = state
            .create_window_at(session, None, None, PaneKind::Terminal, false)
            .unwrap();

        let generation = state.generation();
        assert!(state.set_window_activity_flag(second, true));
        assert!(state.set_window_silence_flag(second, true));
        assert!(state.generation() > generation);
        assert!(state.windows[&second].activity_flag);
        assert!(state.windows[&second].silence_flag);

        let generation = state.generation();
        assert!(!state.set_window_activity_flag(second, true));
        assert!(!state.set_window_silence_flag(second, true));
        assert_eq!(state.generation(), generation);

        assert!(state.activate_window(session, second));
        assert!(!state.windows[&second].activity_flag);
        assert!(!state.windows[&second].silence_flag);

        assert!(!state.set_window_activity_flag(WindowId(9999), true));
        assert!(!state.set_window_silence_flag(WindowId(9999), true));
        let _ = first;
    }
}
