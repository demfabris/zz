use std::{
    collections::{BTreeMap, HashMap, btree_map},
    ops::Deref,
    sync::{Arc, Weak},
};

use zz_protocol::{PaneId, SessionId, WindowId};

use crate::{
    layout::CellLayout,
    model::{MuxState, Session, Window},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionImage {
    pub name: String,
    pub active_window: WindowId,
    pub windows: Vec<WindowId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneImage {
    pub id: PaneId,
    pub title: String,
    pub bell: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowImage {
    pub session: SessionId,
    pub name: String,
    pub active_pane: PaneId,
    pub zoomed_pane: Option<PaneId>,
    pub layout: CellLayout,
    pub extent: (u16, u16),
    pub panes: Vec<PaneImage>,
}

#[derive(Debug)]
enum Change {
    Session(SessionId, Option<SessionImage>),
    Window(WindowId, Option<WindowImage>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Entity {
    Session(SessionId),
    Window(WindowId),
}

type Positions = HashMap<Entity, u64, foldhash::fast::FixedState>;

#[derive(Debug, Default)]
pub struct ChangeJournal {
    base: u64,
    entries: Vec<Change>,
    positions: Positions,
    open: Vec<(u64, Weak<()>)>,
    removals: u64,
}

#[derive(Clone, Debug)]
pub struct ChangeWindow {
    mark: u64,
    _alive: Arc<()>,
}

#[derive(Debug, Default)]
pub struct JournalChanges<'a> {
    pub sessions: BTreeMap<SessionId, Option<&'a SessionImage>>,
    pub windows: BTreeMap<WindowId, Option<&'a WindowImage>>,
}

impl ChangeJournal {
    pub(crate) fn note_removal(&mut self) {
        self.removals = self.removals.wrapping_add(1);
    }

    fn prune(&mut self) {
        self.open.retain(|(_, alive)| alive.strong_count() > 0);
        if self.open.is_empty() && !self.entries.is_empty() {
            self.base += self.entries.len() as u64;
            self.entries.clear();
            self.positions.clear();
        }
    }

    fn recording_floor(&mut self) -> Option<u64> {
        self.prune();
        self.open.iter().map(|(mark, _)| *mark).max()
    }

    fn wants(&mut self, entity: Entity) -> bool {
        let Some(floor) = self.recording_floor() else {
            return false;
        };
        self.positions
            .get(&entity)
            .is_none_or(|position| *position < floor)
    }

    fn push(&mut self, entity: Entity, change: Change) {
        self.positions
            .insert(entity, self.base + self.entries.len() as u64);
        self.entries.push(change);
    }

    fn open(&mut self) -> ChangeWindow {
        self.prune();
        let mark = self.base + self.entries.len() as u64;
        let alive = Arc::new(());
        self.open.push((mark, Arc::downgrade(&alive)));
        ChangeWindow {
            mark,
            _alive: alive,
        }
    }

    fn changes(&self, window: &ChangeWindow) -> JournalChanges<'_> {
        let start = usize::try_from(window.mark.saturating_sub(self.base)).unwrap_or(usize::MAX);
        let mut changes = JournalChanges::default();
        for change in self.entries.iter().skip(start) {
            match change {
                Change::Session(id, image) => {
                    changes.sessions.entry(*id).or_insert(image.as_ref());
                }
                Change::Window(id, image) => {
                    changes.windows.entry(*id).or_insert(image.as_ref());
                }
            }
        }
        changes
    }
}

trait Journaled: Sized {
    type Key: Copy + Ord;

    fn entity(key: Self::Key) -> Entity;

    fn change(key: Self::Key, value: Option<&Self>) -> Change;
}

impl Journaled for Session {
    type Key = SessionId;

    fn entity(key: SessionId) -> Entity {
        Entity::Session(key)
    }

    fn change(key: SessionId, value: Option<&Self>) -> Change {
        Change::Session(
            key,
            value.map(|session| SessionImage {
                name: session.name.clone(),
                active_window: session.active_window,
                windows: session.windows.clone(),
            }),
        )
    }
}

impl Journaled for Window {
    type Key = WindowId;

    fn entity(key: WindowId) -> Entity {
        Entity::Window(key)
    }

    fn change(key: WindowId, value: Option<&Self>) -> Change {
        Change::Window(
            key,
            value.map(|window| WindowImage {
                session: window.session,
                name: window.name.clone(),
                active_pane: window.active_pane,
                zoomed_pane: window.zoomed_pane,
                layout: window.layout.clone(),
                extent: window.layout.extent(),
                panes: window
                    .panes
                    .values()
                    .map(|pane| PaneImage {
                        id: pane.id,
                        title: pane.title.clone(),
                        bell: pane.bell,
                    })
                    .collect(),
            }),
        )
    }
}

#[derive(Debug)]
pub struct Tracked<K, V> {
    map: BTreeMap<K, V>,
}

impl<K, V> Default for Tracked<K, V> {
    fn default() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }
}

impl<K, V> Deref for Tracked<K, V> {
    type Target = BTreeMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<'a, K, V> IntoIterator for &'a Tracked<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = btree_map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

macro_rules! tracked_methods {
    ($key:ty, $value:ty) => {
        impl Tracked<$key, $value> {
            fn record(&self, journal: &mut ChangeJournal, key: $key) {
                let entity = <$value as Journaled>::entity(key);
                if journal.wants(entity) {
                    journal.push(
                        entity,
                        <$value as Journaled>::change(key, self.map.get(&key)),
                    );
                }
            }

            pub(crate) fn get_mut(
                &mut self,
                journal: &mut ChangeJournal,
                key: &$key,
            ) -> Option<&mut $value> {
                if self.map.contains_key(key) {
                    self.record(journal, *key);
                }
                self.map.get_mut(key)
            }

            pub(crate) fn insert(
                &mut self,
                journal: &mut ChangeJournal,
                key: $key,
                value: $value,
            ) -> Option<$value> {
                self.record(journal, key);
                self.map.insert(key, value)
            }

            pub(crate) fn remove(
                &mut self,
                journal: &mut ChangeJournal,
                key: &$key,
            ) -> Option<$value> {
                if self.map.contains_key(key) {
                    self.record(journal, *key);
                    journal.note_removal();
                }
                self.map.remove(key)
            }
        }
    };
}

tracked_methods!(SessionId, Session);
tracked_methods!(WindowId, Window);

impl MuxState {
    pub fn open_change_window(&mut self) -> ChangeWindow {
        self.journal.open()
    }

    #[must_use]
    pub fn changes_since(&self, window: &ChangeWindow) -> JournalChanges<'_> {
        self.journal.changes(window)
    }

    #[must_use]
    pub fn journal_len(&self) -> usize {
        self.journal.entries.len()
    }

    #[must_use]
    pub fn removals(&self) -> u64 {
        self.journal.removals
    }

    pub fn session_mut(&mut self, session: SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(&mut self.journal, &session)
    }

    pub fn window_mut(&mut self, window: WindowId) -> Option<&mut Window> {
        self.windows.get_mut(&mut self.journal, &window)
    }

    pub fn remove_session(&mut self, session: SessionId) -> Option<Session> {
        self.sessions.remove(&mut self.journal, &session)
    }
}
