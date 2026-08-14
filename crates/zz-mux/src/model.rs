use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use zz_protocol::{
    AgentDescriptor, AgentProvider, Axis, BrowserDescriptor, EditorDescriptor, LayoutNode,
    MAX_GUI_TEXT_BYTES, MuxSnapshot, PaneId, PaneKindSnapshot, PaneSnapshot, ServerError,
    SessionId, SessionSnapshot, SplitId, WindowId, WindowSnapshot, normalize_browser_profile_name,
};

const MIN_SPLIT_RATIO: f32 = 0.1;
const MAX_SPLIT_RATIO: f32 = 0.9;
const SYNCHRONIZE_PANES: u8 = 1 << 0;
const LAYOUT_COORDINATE_MAX: u32 = 1_000_000;
const MAX_AGENT_SESSION_ID_BYTES: usize = 16 * 1024;

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

/// Where a split drops the pane it creates: `ratio` is the new pane's share of
/// the box it lands in, `before` puts it left of or above the target,
/// `full_size` spans the whole window instead of the target's box, and
/// `detached` leaves focus where it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitPlacement {
    pub ratio: f32,
    pub before: bool,
    pub full_size: bool,
    pub detached: bool,
}

impl Default for SplitPlacement {
    fn default() -> Self {
        Self {
            ratio: 0.5,
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
    /// A BEL rang here and nobody has been back since.
    pub bell: bool,
    input_options: InputOptions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: WindowId,
    pub session: SessionId,
    pub index: u32,
    pub name: String,
    pub active_pane: PaneId,
    pub zoomed_pane: Option<PaneId>,
    pub layout: LayoutNode,
    pub panes: BTreeMap<PaneId, Pane>,
    pane_order: Vec<PaneId>,
    last_panes: Vec<PaneId>,
    last_layout: Option<LayoutPreset>,
    previous_layout: Option<Box<LayoutNode>>,
    input_options: InputOptions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub windows: Vec<WindowId>,
    pub active_window: WindowId,
    last_window: Option<WindowId>,
}

impl Session {
    #[must_use]
    pub fn last_window(&self) -> Option<WindowId> {
        self.last_window
    }

    fn activate_window(&mut self, window: WindowId) {
        if self.active_window != window {
            self.last_window = Some(self.active_window);
            self.active_window = window;
        }
    }

    fn forget_window(&mut self, window: WindowId) {
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
        }
    }
}

impl Window {
    #[must_use]
    pub fn pane_order(&self) -> &[PaneId] {
        &self.pane_order
    }
}

#[derive(Debug, Default)]
pub struct MuxState {
    generation: u64,
    next_session_id: u64,
    next_window_id: u64,
    next_pane_id: u64,
    next_split_id: u64,
    input_options: InputOptions,
    pub sessions: BTreeMap<SessionId, Session>,
    pub windows: BTreeMap<WindowId, Window>,
}

impl MuxState {
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn create_session(
        &mut self,
        name: impl Into<String>,
    ) -> Result<(SessionId, WindowId, PaneId), ServerError> {
        let name = name.into();
        if self.sessions.values().any(|session| session.name == name) {
            return Err(ServerError::InvalidCommand(format!(
                "duplicate session name: {name}"
            )));
        }
        let session_id = self.allocate_session_id();
        let window_id = self.allocate_window_id();
        let pane_id = self.allocate_pane_id();
        let pane = Pane {
            id: pane_id,
            title: "terminal".to_owned(),
            kind: PaneKind::Terminal,
            bell: false,
            input_options: InputOptions::default(),
        };
        let window = Window {
            id: window_id,
            session: session_id,
            index: 0,
            name: "0".to_owned(),
            active_pane: pane_id,
            zoomed_pane: None,
            layout: LayoutNode::Pane(pane_id),
            panes: BTreeMap::from([(pane_id, pane)]),
            pane_order: vec![pane_id],
            last_panes: Vec::new(),
            last_layout: None,
            previous_layout: None,
            input_options: InputOptions::default(),
        };
        self.windows.insert(window_id, window);
        self.sessions.insert(
            session_id,
            Session {
                id: session_id,
                name,
                windows: vec![window_id],
                active_window: window_id,
                last_window: None,
            },
        );
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
        let index = self.claim_window_index(session, index)?;
        let window_id = self.allocate_window_id();
        let pane_id = self.allocate_pane_id();
        let pane = Pane {
            id: pane_id,
            title: pane_title(&kind),
            kind,
            bell: false,
            input_options: InputOptions::default(),
        };
        let window = Window {
            id: window_id,
            session,
            index,
            name: name.unwrap_or_else(|| index.to_string()),
            active_pane: pane_id,
            zoomed_pane: None,
            layout: LayoutNode::Pane(pane_id),
            panes: BTreeMap::from([(pane_id, pane)]),
            pane_order: vec![pane_id],
            last_panes: Vec::new(),
            last_layout: None,
            previous_layout: None,
            input_options: InputOptions::default(),
        };
        self.windows.insert(window_id, window);
        let session_state = self
            .sessions
            .get_mut(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        session_state.windows.push(window_id);
        if activate {
            session_state.activate_window(window_id);
        }
        self.sort_session_windows(session);
        self.bump_generation();
        Ok((window_id, pane_id))
    }

    /// The index a new window takes in `session`: the requested one when it is
    /// free, otherwise the lowest free index.
    fn claim_window_index(
        &self,
        session: SessionId,
        index: Option<u32>,
    ) -> Result<u32, ServerError> {
        let Some(index) = index else {
            return self.next_window_index(session);
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
        if !placement.ratio.is_finite()
            || !(MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(&placement.ratio)
        {
            return Err(ServerError::InvalidCommand(format!(
                "new pane ratio must be between {MIN_SPLIT_RATIO} and {MAX_SPLIT_RATIO}"
            )));
        }
        let window_id = self
            .window_for_pane(target)
            .ok_or_else(|| ServerError::MissingTarget(target.to_string()))?;
        let pane_id = self.allocate_pane_id();
        let split_id = self.allocate_split_id();
        let window = self.windows.get_mut(&window_id).expect("window exists");
        if !insert_existing_pane(
            &mut window.layout,
            target,
            pane_id,
            split_id,
            axis,
            placement.ratio,
            placement.before,
            placement.full_size,
        ) {
            return Err(ServerError::MissingTarget(target.to_string()));
        }
        window.panes.insert(
            pane_id,
            Pane {
                id: pane_id,
                title: pane_title(&kind),
                kind,
                bell: false,
                input_options: InputOptions::default(),
            },
        );
        insert_pane_order(&mut window.pane_order, pane_id, target, placement.before);
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
        if self.windows[&window_id].panes.len() == 1 {
            return self.kill_window(window_id);
        }
        let window = self.windows.get_mut(&window_id).expect("window exists");
        if !remove_leaf(&mut window.layout, pane) {
            return Err(ServerError::MissingTarget(pane.to_string()));
        }
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
        let session = self
            .sessions
            .get_mut(&removed.session)
            .expect("window session exists");
        session.forget_window(window);
        if session.windows.is_empty() {
            self.sessions.remove(&removed.session);
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
            .get_mut(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        if !session_state.windows.contains(&window) {
            return Err(ServerError::MissingTarget(window.to_string()));
        }
        session_state.activate_window(window);
        self.bump_generation();
        Ok(())
    }

    pub fn select_pane(&mut self, pane: PaneId) -> Result<(), ServerError> {
        self.select_pane_with_zoom(pane, false)
    }

    pub fn select_pane_with_zoom(
        &mut self,
        pane: PaneId,
        preserve_zoom: bool,
    ) -> Result<(), ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let session_id = self.windows[&window_id].session;
        let window = self.windows.get_mut(&window_id).expect("window exists");
        let pane_changed = activate_window_pane(window, pane, preserve_zoom);
        let session = self.sessions.get_mut(&session_id).expect("session exists");
        let window_changed = session.active_window != window_id;
        session.activate_window(window_id);
        if pane_changed || window_changed {
            self.bump_generation();
        }
        Ok(())
    }

    pub fn toggle_zoom(&mut self, pane: PaneId) -> Result<(), ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let window = self.windows.get_mut(&window_id).expect("window exists");
        if window.panes.len() <= 1 {
            return Ok(());
        }
        if window.zoomed_pane.is_some() {
            window.zoomed_pane = None;
        } else {
            activate_window_pane(window, pane, false);
            window.zoomed_pane = Some(pane);
        }
        self.bump_generation();
        Ok(())
    }

    /// Move `pane`'s resize boundary by `cells` terminal cells, positive toward
    /// the right or bottom. `window_extent` is the window's cell count along
    /// `axis`; without one, a unit falls back to 5% of the adjusted split.
    pub fn resize_pane(
        &mut self,
        pane: PaneId,
        axis: Axis,
        cells: f32,
        window_extent: Option<f32>,
    ) -> Result<(), ServerError> {
        if !cells.is_finite() {
            return Err(ServerError::InvalidCommand(
                "pane resize adjustment must be finite".to_owned(),
            ));
        }
        let (window_id, boundary) = self.resize_boundary_for(pane, axis)?;
        let delta = match window_extent {
            Some(extent) if extent >= 1.0 => cells / extent / boundary.container.max(f32::EPSILON),
            _ => cells * 0.05,
        };
        self.apply_resize(window_id, boundary.split, boundary.ratio + delta);
        Ok(())
    }

    /// Give `pane` `fraction` of the window along `axis`, the way
    /// `resize-pane -x` and `-y` do. The tree keeps proportions, so callers
    /// convert cells to a fraction with the geometry they were handed.
    pub fn resize_pane_to(
        &mut self,
        pane: PaneId,
        axis: Axis,
        fraction: f32,
    ) -> Result<(), ServerError> {
        if !fraction.is_finite() || fraction <= 0.0 || fraction > 1.0 {
            return Err(ServerError::InvalidCommand(
                "pane size must be a positive share of the window".to_owned(),
            ));
        }
        let (window_id, boundary) = self.resize_boundary_for(pane, axis)?;
        let layout = &self.windows[&window_id].layout;
        let current = pane_axis_fraction(layout, pane, axis)
            .filter(|current| *current > 0.0)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let scale = fraction / current;
        let ratio = if boundary.target_first {
            boundary.ratio * scale
        } else {
            1.0 - (1.0 - boundary.ratio) * scale
        };
        self.apply_resize(window_id, boundary.split, ratio);
        Ok(())
    }

    fn resize_boundary_for(
        &self,
        pane: PaneId,
        axis: Axis,
    ) -> Result<(WindowId, ResizeBoundary), ServerError> {
        let window_id = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        let boundary = resize_boundary(&self.windows[&window_id].layout, pane, axis, 1.0)
            .ok_or_else(|| {
                ServerError::InvalidCommand(format!(
                    "pane {pane} has no resizable split on the requested axis"
                ))
            })?;
        Ok((window_id, boundary))
    }

    fn apply_resize(&mut self, window_id: WindowId, split: SplitId, ratio: f32) {
        let window = self.windows.get_mut(&window_id).expect("window exists");
        set_split_ratio(&mut window.layout, split, ratio).expect("located split still exists");
        window.zoomed_pane = None;
        self.bump_generation();
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
        let changed = set_split_ratio(&mut window.layout, split, ratio)
            .ok_or_else(|| ServerError::MissingTarget(split.to_string()))?;
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
    ) -> Result<(), ServerError> {
        let panes = self
            .windows
            .get(&window)
            .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?
            .pane_order
            .clone();
        debug_assert!(!panes.is_empty(), "validated windows are never empty");
        let split_count = panes.len().saturating_sub(1);
        let split_ids = (0..split_count)
            .map(|_| self.allocate_split_id())
            .collect::<Vec<_>>();
        let mut split_ids = split_ids.into_iter();
        let layout = build_preset_layout(&panes, preset, &mut split_ids);
        debug_assert!(
            split_ids.next().is_none(),
            "preset consumes one split ID per edge"
        );

        let window = self.windows.get_mut(&window).expect("window was resolved");
        let previous = std::mem::replace(&mut window.layout, layout);
        window.previous_layout = Some(Box::new(previous));
        window.last_layout = Some(preset);
        window.zoomed_pane = None;
        self.bump_generation();
        Ok(())
    }

    pub fn cycle_layout(
        &mut self,
        window: WindowId,
        offset: isize,
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
        self.select_layout(window, preset)?;
        Ok(preset)
    }

    pub fn restore_previous_layout(&mut self, window: WindowId) -> Result<(), ServerError> {
        let (pane_order, split_count) = {
            let window_state = self
                .windows
                .get(&window)
                .ok_or_else(|| ServerError::MissingTarget(window.to_string()))?;
            let previous = window_state.previous_layout.as_deref().ok_or_else(|| {
                ServerError::InvalidCommand(format!("window {window} has no previous layout"))
            })?;
            if layout_pane_count(previous) != window_state.pane_order.len()
                || !valid_split_ratios(previous)
            {
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
        let mut panes = pane_order.into_iter();
        replace_layout_panes_in_order(&mut restored, &mut panes);
        debug_assert!(
            panes.next().is_none(),
            "restored layout consumes every ordered pane"
        );
        let mut split_ids = split_ids.into_iter();
        replace_split_ids(&mut restored, &mut split_ids);
        debug_assert!(
            split_ids.next().is_none(),
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
        spread_first_uneven_ancestor(&mut window.layout, pane);
        window.previous_layout = Some(Box::new(previous));
        window.zoomed_pane = None;
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

    pub fn resolve_session(
        &self,
        target: Option<&str>,
        current: Option<SessionId>,
    ) -> Result<SessionId, ServerError> {
        let Some(target) = target else {
            return current
                .filter(|session| self.sessions.contains_key(session))
                .or_else(|| self.sessions.keys().next().copied())
                .ok_or_else(|| ServerError::MissingTarget("current session".to_owned()));
        };
        if target.starts_with('$') {
            let id = target
                .parse::<SessionId>()
                .map_err(|_| ServerError::InvalidTarget(target.to_owned()))?;
            return self
                .sessions
                .contains_key(&id)
                .then_some(id)
                .ok_or_else(|| ServerError::MissingTarget(target.to_owned()));
        }
        if let Some(session) = self
            .sessions
            .values()
            .find(|session| session.name == target)
            .map(|session| session.id)
        {
            return Ok(session);
        }
        let mut prefixed = self
            .sessions
            .values()
            .filter(|session| session.name.starts_with(target))
            .collect::<Vec<_>>();
        match prefixed.as_slice() {
            [] => Err(ServerError::MissingTarget(target.to_owned())),
            [session] => Ok(session.id),
            _ => {
                prefixed.sort_unstable_by(|left, right| left.name.cmp(&right.name));
                let names = prefixed
                    .iter()
                    .map(|session| session.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(ServerError::AmbiguousTarget(format!(
                    "{target} matches {names}"
                )))
            }
        }
    }

    pub fn resolve_window(
        &self,
        target: Option<&str>,
        current_session: Option<SessionId>,
        current_window: Option<WindowId>,
    ) -> Result<WindowId, ServerError> {
        let Some(target) = target else {
            if let Some(window) = current_window.filter(|window| self.windows.contains_key(window))
            {
                return Ok(window);
            }
            let session = self.resolve_session(None, current_session)?;
            return self
                .sessions
                .get(&session)
                .map(|session| session.active_window)
                .ok_or_else(|| ServerError::MissingTarget(session.to_string()));
        };
        if target.starts_with('@') {
            let id = target
                .parse::<WindowId>()
                .map_err(|_| ServerError::InvalidTarget(target.to_owned()))?;
            return self
                .windows
                .contains_key(&id)
                .then_some(id)
                .ok_or_else(|| ServerError::MissingTarget(target.to_owned()));
        }
        let (session_target, window_target) = target
            .split_once(':')
            .map_or((None, target), |(session, window)| (Some(session), window));
        let current_session = current_session
            .filter(|session| self.sessions.contains_key(session))
            .or_else(|| {
                current_window
                    .and_then(|window| self.windows.get(&window).map(|window| window.session))
            });
        let session = match session_target {
            Some("") | None => self.resolve_session(None, current_session)?,
            Some(session) => self.resolve_session(Some(session), current_session)?,
        };
        self.resolve_window_in_session(window_target, session, target)
    }

    fn resolve_window_in_session(
        &self,
        target: &str,
        session: SessionId,
        original_target: &str,
    ) -> Result<WindowId, ServerError> {
        let state = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        if target.starts_with('@') {
            let id = target
                .parse::<WindowId>()
                .map_err(|_| ServerError::InvalidTarget(original_target.to_owned()))?;
            return self
                .windows
                .get(&id)
                .is_some_and(|window| window.session == session)
                .then_some(id)
                .ok_or_else(|| ServerError::MissingTarget(original_target.to_owned()));
        }
        if let Ok(index) = target.parse::<u32>() {
            return unique_match(
                original_target,
                state.windows.iter().copied().filter(|window| {
                    self.windows
                        .get(window)
                        .is_some_and(|window| window.index == index)
                }),
            );
        }
        unique_match(
            original_target,
            state.windows.iter().copied().filter(|window| {
                self.windows
                    .get(window)
                    .is_some_and(|window| window.name == target)
            }),
        )
    }

    pub fn resolve_pane(
        &self,
        target: Option<&str>,
        current_window: Option<WindowId>,
        current_pane: Option<PaneId>,
    ) -> Result<PaneId, ServerError> {
        let Some(target) = target else {
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
                .ok_or_else(|| ServerError::MissingTarget("current pane".to_owned()))?;
            return Ok(window.active_pane);
        };
        if target.starts_with('%') {
            let id = target
                .parse::<PaneId>()
                .map_err(|_| ServerError::InvalidTarget(target.to_owned()))?;
            return self
                .window_for_pane(id)
                .map(|_| id)
                .ok_or_else(|| ServerError::MissingTarget(target.to_owned()));
        }

        let current_window = current_pane
            .and_then(|pane| self.window_for_pane(pane))
            .or_else(|| current_window.filter(|window| self.windows.contains_key(window)));
        let current_session = current_window
            .and_then(|window| self.windows.get(&window).map(|window| window.session));
        if let Ok(index) = target.parse::<u32>() {
            let window = self.resolve_window(None, current_session, current_window)?;
            return self.resolve_pane_index(target, window, index);
        }

        let Some((window_target, pane_index)) = target.rsplit_once('.') else {
            return Err(ServerError::InvalidTarget(target.to_owned()));
        };
        let index = pane_index
            .parse::<u32>()
            .map_err(|_| ServerError::InvalidTarget(target.to_owned()))?;
        let window = if window_target.is_empty() {
            self.resolve_window(None, current_session, current_window)?
        } else {
            self.resolve_window(Some(window_target), current_session, current_window)?
        };
        self.resolve_pane_index(target, window, index)
    }

    fn resolve_pane_index(
        &self,
        target: &str,
        window: WindowId,
        index: u32,
    ) -> Result<PaneId, ServerError> {
        let index =
            usize::try_from(index).map_err(|_| ServerError::MissingTarget(target.to_owned()))?;
        self.windows
            .get(&window)
            .and_then(|window| window.pane_order().get(index))
            .copied()
            .ok_or_else(|| ServerError::MissingTarget(target.to_owned()))
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
        let mut rects = Vec::with_capacity(window.panes.len());
        collect_pane_rects(
            &window.layout,
            PaneRect {
                left: 0,
                top: 0,
                right: LAYOUT_COORDINATE_MAX,
                bottom: LAYOUT_COORDINATE_MAX,
            },
            &mut rects,
        );
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
        remap_layout_panes(&mut window.layout, &replacements);
        window.pane_order = next_order;
        let next_active = replacements[&active];
        activate_window_pane(window, next_active, false);
        window.zoomed_pane = (preserve_zoom && was_zoomed).then_some(next_active);
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
            let window = self.windows.get_mut(&source_window).expect("window exists");
            let was_zoomed = window.zoomed_pane.is_some();
            swap_layout_panes(&mut window.layout, source, target);
            swap_pane_order(&mut window.pane_order, source, target);
            let next_active = if detached {
                if window.active_pane == source {
                    target
                } else if window.active_pane == target {
                    source
                } else {
                    window.active_pane
                }
            } else {
                target
            };
            activate_window_pane(window, next_active, false);
            window.zoomed_pane = (preserve_zoom && was_zoomed).then_some(window.active_pane);
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
        replace_layout_pane(&mut source_state.layout, source, target);
        replace_layout_pane(&mut target_state.layout, target, source);
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
        activate_relocated_window_pane(&mut source_state, next_source_active, source);
        activate_relocated_window_pane(target_state, next_target_active, target);
        source_state.zoomed_pane =
            (preserve_zoom && source_was_zoomed).then_some(source_state.active_pane);
        target_state.zoomed_pane =
            (preserve_zoom && target_was_zoomed).then_some(target_state.active_pane);
        self.windows.insert(source_window, source_state);
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
        let source_window = self
            .window_for_pane(pane)
            .ok_or_else(|| ServerError::MissingTarget(pane.to_string()))?;
        if !self.sessions.contains_key(&destination_session) {
            return Err(ServerError::MissingTarget(destination_session.to_string()));
        }
        if self.windows[&source_window].panes.len() == 1 {
            return Err(ServerError::InvalidCommand(
                "cannot break the only pane in a window".to_owned(),
            ));
        }
        let index = self.claim_window_index(destination_session, destination_index)?;
        let window_id = self.allocate_window_id();
        let source = self
            .windows
            .get_mut(&source_window)
            .expect("source window exists");
        if !remove_leaf(&mut source.layout, pane) {
            return Err(ServerError::MissingTarget(pane.to_string()));
        }
        let pane_state = source
            .panes
            .remove(&pane)
            .expect("source window contains pane");
        repair_window_after_pane_removal(source, pane);
        let window_name = name.unwrap_or_else(|| pane_state.title.clone());
        self.windows.insert(
            window_id,
            Window {
                id: window_id,
                session: destination_session,
                index,
                name: window_name,
                active_pane: pane,
                zoomed_pane: None,
                layout: LayoutNode::Pane(pane),
                panes: BTreeMap::from([(pane, pane_state)]),
                pane_order: vec![pane],
                last_panes: Vec::new(),
                last_layout: None,
                previous_layout: None,
                input_options: InputOptions::default(),
            },
        );
        let destination = self
            .sessions
            .get_mut(&destination_session)
            .expect("destination session exists");
        destination.windows.push(window_id);
        if !detached {
            destination.activate_window(window_id);
        }
        self.sort_session_windows(destination_session);
        self.bump_generation();
        Ok(window_id)
    }

    pub fn join_pane(
        &mut self,
        source: PaneId,
        target: PaneId,
        axis: Axis,
        pane_ratio: f32,
        before: bool,
        full_size: bool,
        detached: bool,
    ) -> Result<(), ServerError> {
        if source == target {
            return Err(ServerError::InvalidCommand(
                "source and target panes must be different".to_owned(),
            ));
        }
        if !pane_ratio.is_finite() || !(MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(&pane_ratio) {
            return Err(ServerError::InvalidCommand(format!(
                "joined pane ratio must be between {MIN_SPLIT_RATIO} and {MAX_SPLIT_RATIO}"
            )));
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
        let split = self.allocate_split_id();

        if source_window == target_window {
            let window = self.windows.get_mut(&source_window).expect("window exists");
            if !remove_leaf(&mut window.layout, source) {
                return Err(ServerError::MissingTarget(source.to_string()));
            }
            let inserted = insert_existing_pane(
                &mut window.layout,
                target,
                source,
                split,
                axis,
                pane_ratio,
                before,
                full_size,
            );
            debug_assert!(inserted, "target remains after removing a distinct source");
            window.pane_order.retain(|pane| *pane != source);
            insert_pane_order(&mut window.pane_order, source, target, before);
            window.zoomed_pane = None;
            if !detached {
                activate_window_pane(window, source, false);
            }
            normalize_window_history(window);
            self.bump_generation();
            return Ok(());
        }

        let mut source_state = self
            .windows
            .remove(&source_window)
            .expect("source window exists");
        let source_will_close = source_state.panes.len() == 1;
        if !source_will_close {
            let removed = remove_leaf(&mut source_state.layout, source);
            debug_assert!(removed, "source layout contains source pane");
        }
        let pane_state = source_state
            .panes
            .remove(&source)
            .expect("source window contains source pane");
        if !source_will_close {
            repair_window_after_pane_removal(&mut source_state, source);
        }

        let target_state = self
            .windows
            .get_mut(&target_window)
            .expect("target window exists");
        let inserted = insert_existing_pane(
            &mut target_state.layout,
            target,
            source,
            split,
            axis,
            pane_ratio,
            before,
            full_size,
        );
        debug_assert!(inserted, "target window contains target pane");
        target_state.panes.insert(source, pane_state);
        insert_pane_order(&mut target_state.pane_order, source, target, before);
        target_state.zoomed_pane = None;
        if !detached {
            activate_window_pane(target_state, source, false);
        }
        normalize_window_history(target_state);

        if source_will_close {
            self.sessions
                .get_mut(&source_session)
                .expect("source session exists")
                .forget_window(source_window);
        } else {
            self.windows.insert(source_window, source_state);
        }
        if !detached {
            self.sessions
                .get_mut(&target_session)
                .expect("target session exists")
                .activate_window(target_window);
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
        WindowSnapshot {
            id: window.id,
            index: window.index,
            name: window.name.clone(),
            active_pane: window.active_pane,
            zoomed_pane: window.zoomed_pane,
            layout: window.layout.clone(),
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
                        },
                    )
                })
                .collect(),
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
            let mut layout_panes = Vec::new();
            window.layout.panes(&mut layout_panes);
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
            if !valid_split_ratios(&window.layout) {
                return Err(format!("window {window_id} has an invalid split ratio"));
            }
            let mut layout_splits = Vec::new();
            window.layout.splits(&mut layout_splits);
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

    fn next_window_index(&self, session: SessionId) -> Result<u32, ServerError> {
        let session = self
            .sessions
            .get(&session)
            .ok_or_else(|| ServerError::MissingTarget(session.to_string()))?;
        let used = session
            .windows
            .iter()
            .map(|window| self.windows[window].index)
            .collect::<BTreeSet<_>>();
        Ok((0..=u32::MAX)
            .find(|index| !used.contains(index))
            .unwrap_or(u32::MAX))
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

    fn bump_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
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

fn unique_match<T: Copy>(
    target: &str,
    candidates: impl IntoIterator<Item = T>,
) -> Result<T, ServerError> {
    let mut candidates = candidates.into_iter();
    let first = candidates
        .next()
        .ok_or_else(|| ServerError::MissingTarget(target.to_owned()))?;
    if candidates.next().is_some() {
        Err(ServerError::AmbiguousTarget(target.to_owned()))
    } else {
        Ok(first)
    }
}

fn build_preset_layout(
    panes: &[PaneId],
    preset: LayoutPreset,
    split_ids: &mut impl Iterator<Item = SplitId>,
) -> LayoutNode {
    debug_assert!(!panes.is_empty());
    if panes.len() == 1 {
        return LayoutNode::Pane(panes[0]);
    }
    match preset {
        LayoutPreset::EvenHorizontal => combine_equal_nodes(
            panes.iter().copied().map(LayoutNode::Pane).collect(),
            Axis::Horizontal,
            split_ids,
        ),
        LayoutPreset::EvenVertical => combine_equal_nodes(
            panes.iter().copied().map(LayoutNode::Pane).collect(),
            Axis::Vertical,
            split_ids,
        ),
        LayoutPreset::MainHorizontal => {
            build_main_layout(panes, Axis::Vertical, Axis::Horizontal, false, split_ids)
        }
        LayoutPreset::MainHorizontalMirrored => {
            build_main_layout(panes, Axis::Vertical, Axis::Horizontal, true, split_ids)
        }
        LayoutPreset::MainVertical => {
            build_main_layout(panes, Axis::Horizontal, Axis::Vertical, false, split_ids)
        }
        LayoutPreset::MainVerticalMirrored => {
            build_main_layout(panes, Axis::Horizontal, Axis::Vertical, true, split_ids)
        }
        LayoutPreset::Tiled => build_tiled_layout(panes, split_ids),
    }
}

fn build_main_layout(
    panes: &[PaneId],
    root_axis: Axis,
    secondary_axis: Axis,
    mirrored: bool,
    split_ids: &mut impl Iterator<Item = SplitId>,
) -> LayoutNode {
    let split = split_ids.next().expect("multi-pane preset has a split ID");
    let main = LayoutNode::Pane(panes[0]);
    let secondary = combine_equal_nodes(
        panes[1..].iter().copied().map(LayoutNode::Pane).collect(),
        secondary_axis,
        split_ids,
    );
    let (first, second) = if mirrored {
        (secondary, main)
    } else {
        (main, secondary)
    };
    LayoutNode::Split {
        id: split,
        axis: root_axis,
        ratio: 0.5,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn build_tiled_layout(
    panes: &[PaneId],
    split_ids: &mut impl Iterator<Item = SplitId>,
) -> LayoutNode {
    let mut rows = 1_usize;
    let mut columns = 1_usize;
    while rows.saturating_mul(columns) < panes.len() {
        rows = rows.saturating_add(1);
        if rows.saturating_mul(columns) < panes.len() {
            columns = columns.saturating_add(1);
        }
    }
    let rows = panes
        .chunks(columns)
        .map(|row| {
            combine_equal_nodes(
                row.iter().copied().map(LayoutNode::Pane).collect(),
                Axis::Horizontal,
                split_ids,
            )
        })
        .collect();
    combine_equal_nodes(rows, Axis::Vertical, split_ids)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "pane counts become bounded normalized f32 split ratios; the balanced tree keeps usable precision at realistic process limits"
)]
fn combine_equal_nodes(
    mut nodes: Vec<LayoutNode>,
    axis: Axis,
    split_ids: &mut impl Iterator<Item = SplitId>,
) -> LayoutNode {
    debug_assert!(!nodes.is_empty());
    if nodes.len() == 1 {
        return nodes.pop().expect("one layout node");
    }
    let count = nodes.len();
    let midpoint = count / 2;
    let second = nodes.split_off(midpoint);
    let split = split_ids.next().expect("multi-node layout has a split ID");
    let first = combine_equal_nodes(nodes, axis, split_ids);
    let second = combine_equal_nodes(second, axis, split_ids);
    LayoutNode::Split {
        id: split,
        axis,
        ratio: midpoint as f32 / count as f32,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn layout_pane_count(layout: &LayoutNode) -> usize {
    match layout {
        LayoutNode::Pane(_) => 1,
        LayoutNode::Split { first, second, .. } => {
            layout_pane_count(first).saturating_add(layout_pane_count(second))
        }
    }
}

fn replace_layout_panes_in_order(
    layout: &mut LayoutNode,
    panes: &mut impl Iterator<Item = PaneId>,
) {
    match layout {
        LayoutNode::Pane(pane) => {
            *pane = panes.next().expect("layout leaf has an ordered pane");
        }
        LayoutNode::Split { first, second, .. } => {
            replace_layout_panes_in_order(first, panes);
            replace_layout_panes_in_order(second, panes);
        }
    }
}

fn spread_first_uneven_ancestor(node: &mut LayoutNode, pane: PaneId) -> bool {
    let LayoutNode::Split {
        ratio,
        first,
        second,
        ..
    } = node
    else {
        return false;
    };
    let branch = if first.contains(pane) {
        first
    } else if second.contains(pane) {
        second
    } else {
        return false;
    };
    if spread_first_uneven_ancestor(branch, pane) {
        return true;
    }
    if (*ratio - 0.5).abs() <= f32::EPSILON {
        false
    } else {
        *ratio = 0.5;
        true
    }
}

fn replace_split_ids(node: &mut LayoutNode, split_ids: &mut impl Iterator<Item = SplitId>) {
    if let LayoutNode::Split {
        id, first, second, ..
    } = node
    {
        *id = split_ids.next().expect("layout split has a replacement ID");
        replace_split_ids(first, split_ids);
        replace_split_ids(second, split_ids);
    }
}

/// The layout `swap-pane -s source -t target` produces, without touching state.
/// A client renders a drop optimistically through this same transform.
#[must_use]
pub fn swapped_layout(layout: &LayoutNode, source: PaneId, target: PaneId) -> LayoutNode {
    let mut layout = layout.clone();
    swap_layout_panes(&mut layout, source, target);
    layout
}

/// The layout `join-pane` produces, without touching state. `None` when the
/// panes are the same or either leaf is missing.
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
    if !remove_leaf(&mut layout, source) {
        return None;
    }
    insert_existing_pane(
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

fn insert_existing_pane(
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
            insert_existing_pane(first, target, pane, split, axis, pane_ratio, before, false)
                || insert_existing_pane(
                    second, target, pane, split, axis, pane_ratio, before, false,
                )
        }
    }
}

fn remove_leaf(node: &mut LayoutNode, target: PaneId) -> bool {
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
    remove_leaf(first, target) || remove_leaf(second, target)
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
    window.pane_order.retain(|candidate| *candidate != pane);
    window.last_panes.retain(|candidate| *candidate != pane);
    if window.active_pane == pane {
        let next = window
            .last_panes
            .first()
            .copied()
            .unwrap_or_else(|| first_pane(&window.layout));
        window.active_pane = next;
    }
    normalize_window_history(window);
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

fn insert_pane_order(order: &mut Vec<PaneId>, pane: PaneId, target: PaneId, before: bool) {
    debug_assert!(!order.contains(&pane));
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

fn activate_relocated_window_pane(window: &mut Window, pane: PaneId, outgoing: PaneId) {
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
}

fn swap_layout_panes(node: &mut LayoutNode, source: PaneId, target: PaneId) {
    match node {
        LayoutNode::Pane(pane) if *pane == source => *pane = target,
        LayoutNode::Pane(pane) if *pane == target => *pane = source,
        LayoutNode::Pane(_) => {}
        LayoutNode::Split { first, second, .. } => {
            swap_layout_panes(first, source, target);
            swap_layout_panes(second, source, target);
        }
    }
}

fn remap_layout_panes(node: &mut LayoutNode, replacements: &BTreeMap<PaneId, PaneId>) {
    match node {
        LayoutNode::Pane(pane) => {
            *pane = replacements[pane];
        }
        LayoutNode::Split { first, second, .. } => {
            remap_layout_panes(first, replacements);
            remap_layout_panes(second, replacements);
        }
    }
}

fn replace_layout_pane(node: &mut LayoutNode, source: PaneId, target: PaneId) -> bool {
    match node {
        LayoutNode::Pane(pane) if *pane == source => {
            *pane = target;
            true
        }
        LayoutNode::Pane(_) => false,
        LayoutNode::Split { first, second, .. } => {
            replace_layout_pane(first, source, target)
                || replace_layout_pane(second, source, target)
        }
    }
}

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
fn split_coordinate(start: u32, end: u32, ratio: f32) -> u32 {
    let extent = end.saturating_sub(start);
    if extent <= 1 {
        return start.saturating_add(extent);
    }
    let offset = (f64::from(extent) * f64::from(ratio)).round() as u32;
    start + offset.clamp(1, extent - 1)
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

fn first_pane(node: &LayoutNode) -> PaneId {
    match node {
        LayoutNode::Pane(pane) => *pane,
        LayoutNode::Split { first, .. } => first_pane(first),
    }
}

fn valid_split_ratios(node: &LayoutNode) -> bool {
    match node {
        LayoutNode::Pane(_) => true,
        LayoutNode::Split {
            ratio,
            first,
            second,
            ..
        } => {
            ratio.is_finite()
                && (MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(ratio)
                && valid_split_ratios(first)
                && valid_split_ratios(second)
        }
    }
}

/// The split a resize moves for one pane: tmux moves the boundary on the far
/// side of the pane (right or bottom), and only falls back to the near boundary
/// when the pane already ends at the window edge.
#[derive(Clone, Copy)]
struct ResizeBoundary {
    split: SplitId,
    ratio: f32,
    /// The window fraction the split's box covers along the resize axis.
    container: f32,
    /// The target sits in the split's first child, so the boundary is the one
    /// on its far side and a positive delta grows it.
    target_first: bool,
}

pub(crate) fn pane_axis_fraction(node: &LayoutNode, target: PaneId, axis: Axis) -> Option<f32> {
    match node {
        LayoutNode::Pane(pane) => (*pane == target).then_some(1.0),
        LayoutNode::Split {
            axis: split_axis,
            ratio,
            first,
            second,
            ..
        } => {
            let on_axis = *split_axis == axis;
            if let Some(fraction) = pane_axis_fraction(first, target, axis) {
                Some(if on_axis { fraction * *ratio } else { fraction })
            } else {
                pane_axis_fraction(second, target, axis).map(|fraction| {
                    if on_axis {
                        fraction * (1.0 - *ratio)
                    } else {
                        fraction
                    }
                })
            }
        }
    }
}

fn resize_boundary(
    node: &LayoutNode,
    target: PaneId,
    axis: Axis,
    container: f32,
) -> Option<ResizeBoundary> {
    let LayoutNode::Split {
        id,
        axis: split_axis,
        ratio,
        first,
        second,
    } = node
    else {
        return None;
    };
    let on_axis = *split_axis == axis;
    let (child, target_first) = if first.contains(target) {
        (first, true)
    } else if second.contains(target) {
        (second, false)
    } else {
        return None;
    };
    let share = if target_first { *ratio } else { 1.0 - *ratio };
    let child_container = if on_axis {
        container * share
    } else {
        container
    };
    let deeper = resize_boundary(child, target, axis, child_container.max(f32::EPSILON));
    let here = on_axis.then_some(ResizeBoundary {
        split: *id,
        ratio: *ratio,
        container,
        target_first,
    });
    match (deeper, here) {
        (Some(deeper), Some(here)) => Some(if here.target_first && !deeper.target_first {
            here
        } else {
            deeper
        }),
        (Some(deeper), None) => Some(deeper),
        (None, here) => here,
    }
}

fn set_split_ratio(node: &mut LayoutNode, target: SplitId, requested: f32) -> Option<bool> {
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
    if *id == target {
        let requested = requested.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO);
        let changed = ratio.to_bits() != requested.to_bits();
        *ratio = requested;
        return Some(changed);
    }
    set_split_ratio(first, target, requested).or_else(|| set_split_ratio(second, target, requested))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split_ratio(node: &LayoutNode, split: SplitId) -> Option<f32> {
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
            .or_else(|| split_ratio(first, split))
            .or_else(|| split_ratio(second, split))
    }

    fn layout_panes(node: &LayoutNode) -> Vec<PaneId> {
        let mut panes = Vec::new();
        node.panes(&mut panes);
        panes
    }

    fn same_layout_geometry(left: &LayoutNode, right: &LayoutNode) -> bool {
        match (left, right) {
            (LayoutNode::Pane(left), LayoutNode::Pane(right)) => left == right,
            (
                LayoutNode::Split {
                    axis: left_axis,
                    ratio: left_ratio,
                    first: left_first,
                    second: left_second,
                    ..
                },
                LayoutNode::Split {
                    axis: right_axis,
                    ratio: right_ratio,
                    first: right_first,
                    second: right_second,
                    ..
                },
            ) => {
                left_axis == right_axis
                    && left_ratio.to_bits() == right_ratio.to_bits()
                    && same_layout_geometry(left_first, right_first)
                    && same_layout_geometry(left_second, right_second)
            }
            _ => false,
        }
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
    fn remove_leaf_moves_the_promoted_subtree_without_reallocating_descendants() {
        let target = PaneId(0);
        for target_first in [true, false] {
            let survivor = LayoutNode::Split {
                id: SplitId(7),
                axis: Axis::Vertical,
                ratio: 0.375,
                first: Box::new(LayoutNode::Pane(PaneId(1))),
                second: Box::new(LayoutNode::Pane(PaneId(2))),
            };
            let preserved_first = match &survivor {
                LayoutNode::Split { first, .. } => std::ptr::from_ref(first.as_ref()),
                LayoutNode::Pane(_) => unreachable!("fixture is a split"),
            };
            let (first, second) = if target_first {
                (Box::new(LayoutNode::Pane(target)), Box::new(survivor))
            } else {
                (Box::new(survivor), Box::new(LayoutNode::Pane(target)))
            };
            let mut layout = LayoutNode::Split {
                id: SplitId(8),
                axis: Axis::Horizontal,
                ratio: 0.5,
                first,
                second,
            };

            assert!(remove_leaf(&mut layout, target));
            let LayoutNode::Split {
                id,
                axis,
                ratio,
                first,
                ..
            } = &layout
            else {
                panic!("surviving split was not promoted")
            };
            assert_eq!(
                (*id, *axis, ratio.to_bits()),
                (SplitId(7), Axis::Vertical, 0.375_f32.to_bits())
            );
            assert!(std::ptr::eq(preserved_first, first.as_ref()));
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
    fn session_names_resolve_by_exact_match_then_unique_prefix() {
        let mut state = MuxState::default();
        let (work, ..) = state.create_session("work").unwrap();
        let (workshop, ..) = state.create_session("workshop").unwrap();
        let (other, ..) = state.create_session("other").unwrap();

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

        let ambiguous = state.resolve_session(Some("wor"), None).unwrap_err();
        assert!(
            matches!(&ambiguous, ServerError::AmbiguousTarget(message)
                if message == "wor matches work, workshop"),
            "{ambiguous:?}"
        );
        let missing = state.resolve_session(Some("nope"), None).unwrap_err();
        assert!(matches!(missing, ServerError::MissingTarget(target) if target == "nope"));
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
        assert!(matches!(
            state.resolve_pane(Some("a:b"), Some(second_window), Some(second_pane)),
            Err(ServerError::InvalidTarget(target)) if target == "a:b"
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
        assert_eq!(split_ratio(layout, SplitId(0)), Some(0.5));
        assert_eq!(split_ratio(layout, SplitId(1)), Some(0.5));
        assert_eq!(state.snapshot().sessions[0].windows[0].layout, *layout);

        state
            .resize_pane(second, Axis::Horizontal, 1.0, None)
            .unwrap();
        let layout = &state.windows[&window].layout;
        assert_eq!(split_ratio(layout, SplitId(0)), Some(0.5));
        assert_eq!(split_ratio(layout, SplitId(1)), Some(0.55));

        assert!(state.resize_split(window, SplitId(0), 0.72).unwrap());
        assert!(!state.resize_split(window, SplitId(0), 0.72).unwrap());
        let layout = &state.windows[&window].layout;
        assert_eq!(split_ratio(layout, SplitId(0)), Some(0.72));
        assert_eq!(split_ratio(layout, SplitId(1)), Some(0.55));

        state.kill_pane(third).unwrap();
        state
            .split_pane(first, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        assert!(state.windows[&window].layout.contains_split(SplitId(2)));
        assert!(!state.windows[&window].layout.contains_split(SplitId(1)));
        assert!(matches!(
            state.resize_pane(first, Axis::Horizontal, f32::NAN, None),
            Err(ServerError::InvalidCommand(message)) if message.contains("finite")
        ));
        assert!(state.validate().is_ok());
    }

    #[test]
    fn named_layouts_rebuild_only_the_tree_and_keep_equal_geometry_balanced() {
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
        let mut retired_splits = Vec::new();
        state.windows[&window].layout.splits(&mut retired_splits);
        state.toggle_zoom(target).unwrap();

        state
            .select_layout(window, LayoutPreset::EvenHorizontal)
            .unwrap();
        let arranged = &state.windows[&window];
        assert_eq!(arranged.panes, panes);
        assert_eq!(arranged.zoomed_pane, None);
        assert_eq!(arranged.last_layout, Some(LayoutPreset::EvenHorizontal));
        let mut new_splits = Vec::new();
        arranged.layout.splits(&mut new_splits);
        assert_eq!(new_splits.len(), panes.len() - 1);
        assert!(
            new_splits
                .iter()
                .all(|split| !retired_splits.contains(split))
        );
        let mut rects = Vec::new();
        collect_pane_rects(
            &arranged.layout,
            PaneRect {
                left: 0,
                top: 0,
                right: LAYOUT_COORDINATE_MAX,
                bottom: LAYOUT_COORDINATE_MAX,
            },
            &mut rects,
        );
        let widths = rects
            .iter()
            .map(|(_, rect)| rect.right - rect.left)
            .collect::<Vec<_>>();
        assert!(
            widths.iter().max().unwrap() - widths.iter().min().unwrap() <= 1,
            "balanced binary splits must still give every pane equal width"
        );
        assert!(
            rects
                .iter()
                .all(|(_, rect)| rect.top == 0 && rect.bottom == LAYOUT_COORDINATE_MAX)
        );

        state
            .select_layout(window, LayoutPreset::MainHorizontal)
            .unwrap();
        assert!(matches!(
            &state.windows[&window].layout,
            LayoutNode::Split {
                axis: Axis::Vertical,
                ratio,
                first: main,
                ..
            } if (*ratio - 0.5).abs() < f32::EPSILON
                && matches!(main.as_ref(), LayoutNode::Pane(pane) if *pane == first)
        ));
        state
            .select_layout(window, LayoutPreset::MainVerticalMirrored)
            .unwrap();
        assert!(matches!(
            &state.windows[&window].layout,
            LayoutNode::Split {
                axis: Axis::Horizontal,
                ratio,
                second: main,
                ..
            } if (*ratio - 0.5).abs() < f32::EPSILON
                && matches!(main.as_ref(), LayoutNode::Pane(pane) if *pane == first)
        ));

        state.swap_panes(first, target, true, false).unwrap();
        let reordered = state.windows[&window].pane_order.clone();
        state.restore_previous_layout(window).unwrap();
        assert_eq!(layout_panes(&state.windows[&window].layout), reordered);
        assert!(matches!(
            &state.windows[&window].layout,
            LayoutNode::Split {
                axis: Axis::Vertical,
                first: main,
                ..
            } if matches!(main.as_ref(), LayoutNode::Pane(pane) if *pane == target)
        ));

        state.select_layout(window, LayoutPreset::Tiled).unwrap();
        let mut tiled = Vec::new();
        collect_pane_rects(
            &state.windows[&window].layout,
            PaneRect {
                left: 0,
                top: 0,
                right: LAYOUT_COORDINATE_MAX,
                bottom: LAYOUT_COORDINATE_MAX,
            },
            &mut tiled,
        );
        assert_eq!(tiled.len(), 5);
        assert_eq!(tiled[4].1.right - tiled[4].1.left, LAYOUT_COORDINATE_MAX);
        assert_eq!(
            tiled
                .iter()
                .map(|(_, rect)| rect.top)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        assert!(state.validate().is_ok());
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
        let mut retired_splits = Vec::new();
        original.splits(&mut retired_splits);

        assert_eq!(
            state.cycle_layout(window, 1).unwrap(),
            LayoutPreset::EvenHorizontal
        );
        let even = state.windows[&window].layout.clone();
        even.splits(&mut retired_splits);
        assert_ne!(even, original);
        state.restore_previous_layout(window).unwrap();
        assert!(same_layout_geometry(
            &state.windows[&window].layout,
            &original
        ));
        let mut restored_splits = Vec::new();
        state.windows[&window].layout.splits(&mut restored_splits);
        assert!(
            restored_splits
                .iter()
                .all(|split| !retired_splits.contains(split))
        );
        retired_splits.extend(restored_splits);
        state.restore_previous_layout(window).unwrap();
        assert!(same_layout_geometry(&state.windows[&window].layout, &even));
        let mut restored_splits = Vec::new();
        state.windows[&window].layout.splits(&mut restored_splits);
        assert!(
            restored_splits
                .iter()
                .all(|split| !retired_splits.contains(split))
        );

        state
            .resize_pane(first, Axis::Horizontal, -4.0, None)
            .unwrap();
        state.spread_layout(first).unwrap();
        let mut rects = Vec::new();
        collect_pane_rects(
            &state.windows[&window].layout,
            PaneRect {
                left: 0,
                top: 0,
                right: LAYOUT_COORDINATE_MAX,
                bottom: LAYOUT_COORDINATE_MAX,
            },
            &mut rects,
        );
        assert_eq!(rects[0].1.right, LAYOUT_COORDINATE_MAX / 2);

        state.select_layout(window, LayoutPreset::Tiled).unwrap();
        assert_eq!(
            state.cycle_layout(window, 1).unwrap(),
            LayoutPreset::EvenHorizontal
        );
        assert_eq!(state.cycle_layout(window, -1).unwrap(), LayoutPreset::Tiled);
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
    fn pane_swaps_preserve_layout_identity_active_slots_and_cross_window_state() {
        let mut state = MuxState::default();
        let (session, window, first) = state.create_session("work").unwrap();
        let second = state
            .split_pane(first, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        let third = state
            .split_pane(second, Axis::Vertical, PaneKind::Terminal)
            .unwrap();
        let mut split_ids = Vec::new();
        state.windows[&window].layout.splits(&mut split_ids);

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
        let mut current_split_ids = Vec::new();
        state.windows[&window].layout.splits(&mut current_split_ids);
        assert_eq!(current_split_ids, split_ids, "split IDs remain stable");

        state.swap_panes(third, second, true, false).unwrap();
        assert_eq!(
            layout_panes(&state.windows[&window].layout),
            [first, second, third]
        );
        assert_eq!(state.windows[&window].pane_order, [first, second, third]);
        assert_eq!(
            state.windows[&window].active_pane, second,
            "detached swaps preserve the active layout slot"
        );
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
        assert_eq!(state.windows[&other_window].layout, LayoutNode::Pane(third));
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
            .select_layout(window, LayoutPreset::MainHorizontalMirrored)
            .unwrap();
        state.select_pane(third).unwrap();
        state.toggle_zoom(third).unwrap();

        let panes = state.windows[&window].panes.clone();
        let order = state.windows[&window].pane_order.clone();
        assert_eq!(order, [first, fourth, second, third]);
        let layout = state.windows[&window].layout.clone();
        let layout_order = layout_panes(&layout);
        let mut split_ids = Vec::new();
        layout.splits(&mut split_ids);

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
        let mut rotated_split_ids = Vec::new();
        state.windows[&window].layout.splits(&mut rotated_split_ids);
        assert_eq!(rotated_split_ids, split_ids);
        assert_eq!(state.windows[&window].panes, panes);

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
            state.windows[&broken_window].layout,
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
            .join_pane(browser, first, Axis::Horizontal, 0.3, true, false, false)
            .unwrap();
        assert!(!state.windows.contains_key(&broken_window));
        assert_eq!(
            layout_panes(&state.windows[&original_window].layout),
            [browser, first, third]
        );
        assert_eq!(
            state.windows[&original_window].pane_order,
            [browser, first, third]
        );
        let LayoutNode::Split { first: left, .. } = &state.windows[&original_window].layout else {
            panic!("original root remains a split");
        };
        assert!(matches!(
            left.as_ref(),
            LayoutNode::Split { id, ratio, .. }
                if *id == next_split && (*ratio - 0.3).abs() < f32::EPSILON
        ));
        assert_eq!(state.windows[&original_window].active_pane, browser);

        let second_break = state
            .break_pane(browser, session, None, None, true)
            .unwrap();
        state
            .join_pane(browser, third, Axis::Vertical, 0.25, false, true, true)
            .unwrap();
        assert!(!state.windows.contains_key(&second_break));
        let LayoutNode::Split {
            axis,
            ratio,
            second,
            ..
        } = &state.windows[&original_window].layout
        else {
            panic!("full-size join wraps the destination layout");
        };
        assert_eq!(*axis, Axis::Vertical);
        assert!((*ratio - 0.75).abs() < f32::EPSILON);
        assert_eq!(second.as_ref(), &LayoutNode::Pane(browser));
        assert_eq!(
            state.windows[&original_window].pane_order,
            [first, third, browser]
        );
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
            .join_pane(moving, target, Axis::Vertical, 0.5, false, false, false)
            .unwrap();
        assert!(!state.windows.contains_key(&moving_window));
        assert!(state.sessions.contains_key(&source_session));
        assert_eq!(state.window_for_pane(moving), Some(target_window));
        assert_eq!(state.windows[&target_window].session, target_session);
        assert_eq!(state.windows[&target_window].pane_order, [target, moving]);
        state
            .select_layout(target_window, LayoutPreset::MainHorizontal)
            .unwrap();
        assert!(matches!(
            &state.windows[&target_window].layout,
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
                0.5,
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
        for (axis, before, expected) in [
            (Axis::Horizontal, true, [3, 1, 2]),
            (Axis::Horizontal, false, [1, 3, 2]),
            (Axis::Vertical, true, [3, 1, 2]),
            (Axis::Vertical, false, [1, 3, 2]),
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
            let expected = expected.map(|index| panes[index - 1]);
            state.select_pane(second).unwrap();

            let predicted = joined_layout(
                &state.windows[&window].layout,
                third,
                first,
                SplitId(u64::MAX),
                axis,
                0.5,
                before,
            )
            .expect("both leaves are in the window");
            state
                .join_pane(third, first, axis, 0.5, before, false, true)
                .unwrap();

            let layout = &state.windows[&window].layout;
            assert_eq!(layout_panes(layout), expected);
            assert!(same_layout_geometry(layout, &predicted));
            assert_eq!(state.windows[&window].pane_order, expected);
            assert_eq!(state.windows[&window].active_pane, second);
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
            &state.windows[&window].layout,
            second,
            first,
            SplitId(u64::MAX),
            Axis::Vertical,
            0.5,
            true,
        )
        .expect("both leaves are in the window");
        state
            .join_pane(second, first, Axis::Vertical, 0.5, true, false, true)
            .unwrap();

        let layout = &state.windows[&window].layout;
        assert!(matches!(
            layout,
            LayoutNode::Split { axis: Axis::Vertical, first: top, second: bottom, .. }
                if top.as_ref() == &LayoutNode::Pane(second)
                    && bottom.as_ref() == &LayoutNode::Pane(first)
        ));
        assert!(same_layout_geometry(layout, &predicted));
        assert_eq!(
            joined_layout(
                layout,
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

        let predicted = swapped_layout(&state.windows[&window].layout, first, third);
        state.swap_panes(first, third, true, false).unwrap();

        assert_eq!(state.windows[&window].layout, predicted);
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

        state
            .resize_pane(first, Axis::Horizontal, 1.0, None)
            .unwrap();
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
}
