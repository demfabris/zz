use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gtk::prelude::*;
use serde_json::Value;
use zz_protocol::{
    AgentConnectionPhase, AgentSessionOpKind, MAX_AGENT_OPTION_BYTES,
    MAX_AGENT_SESSION_DIRECTORIES, MAX_AGENT_SESSION_ID_BYTES, MAX_GUI_TEXT_BYTES,
    PaneKindSnapshot,
};

use super::{AgentPane, clear_box};

const MAX_SESSIONS: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Session {
    id: String,
    title: String,
    cwd: PathBuf,
    additional_directories: Vec<PathBuf>,
}

impl Session {
    fn parse(value: &Value) -> Option<Self> {
        let id = value["sessionId"].as_str()?.to_owned();
        let cwd = PathBuf::from(value["cwd"].as_str()?);
        let directories = value["additionalDirectories"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let valid_directory = |path: &std::path::Path| {
            !path.as_os_str().is_empty()
                && path.as_os_str().as_encoded_bytes().len() <= MAX_GUI_TEXT_BYTES
        };
        if id.is_empty()
            || id.len() > MAX_AGENT_SESSION_ID_BYTES
            || id.chars().any(char::is_control)
            || !valid_directory(&cwd)
            || directories.len() > MAX_AGENT_SESSION_DIRECTORIES
            || directories.iter().any(|path| !valid_directory(path))
            || value["title"].as_str().is_some_and(|title| {
                title.len() > MAX_AGENT_OPTION_BYTES || title.chars().any(char::is_control)
            })
        {
            return None;
        }
        let title = value["title"]
            .as_str()
            .filter(|title| !title.trim().is_empty())
            .map_or_else(
                || {
                    cwd.file_name()
                        .map_or_else(|| id.clone(), |name| name.to_string_lossy().into_owned())
                },
                str::to_owned,
            );
        Some(Self {
            id,
            title,
            cwd,
            additional_directories: directories,
        })
    }
}

pub(super) struct SessionHistory {
    pub button: gtk::MenuButton,
    popover: gtk::Popover,
    list: gtk::Box,
    status: gtk::Label,
    all_projects: gtk::CheckButton,
    more: gtk::Button,
    refresh: gtk::Button,
    new_session: gtk::Button,
    sessions: RefCell<Vec<Session>>,
    next_cursor: RefCell<Option<String>>,
    loading: Cell<bool>,
    can_list: Cell<bool>,
    can_load: Cell<bool>,
}

impl SessionHistory {
    pub fn new() -> Self {
        let button = gtk::MenuButton::builder()
            .icon_name("document-open-recent-symbolic")
            .tooltip_text("Agent conversations")
            .build();
        button.add_css_class("flat");
        let popover = gtk::Popover::new();
        button.set_popover(Some(&popover));
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .width_request(300)
            .build();
        let heading = gtk::Label::builder()
            .label("Conversations")
            .xalign(0.0)
            .build();
        heading.add_css_class("heading");
        content.append(&heading);
        let new_session = gtk::Button::with_label("New conversation");
        content.append(&new_session);
        let filters = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let all_projects = gtk::CheckButton::with_label("All projects");
        all_projects.set_hexpand(true);
        let refresh = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh conversations")
            .build();
        refresh.add_css_class("flat");
        filters.append(&all_projects);
        filters.append(&refresh);
        content.append(&filters);
        let status = gtk::Label::builder()
            .label("Connecting…")
            .xalign(0.0)
            .wrap(true)
            .build();
        status.add_css_class("dim-label");
        content.append(&status);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(400)
            .propagate_natural_height(true)
            .build();
        content.append(&scroll);
        let more = gtk::Button::with_label("Load more");
        more.set_visible(false);
        content.append(&more);
        popover.set_child(Some(&content));
        Self {
            button,
            popover,
            list,
            status,
            all_projects,
            more,
            refresh,
            new_session,
            sessions: RefCell::default(),
            next_cursor: RefCell::default(),
            loading: Cell::new(false),
            can_list: Cell::new(false),
            can_load: Cell::new(false),
        }
    }

    pub fn connect(&self, owner: &Rc<AgentPane>) {
        let weak = Rc::downgrade(owner);
        self.popover.connect_show(move |_| {
            if let Some(owner) = weak.upgrade() {
                owner.history.request(&owner, false);
            }
        });
        let weak = Rc::downgrade(owner);
        self.refresh.connect_clicked(move |_| {
            if let Some(owner) = weak.upgrade() {
                owner.history.request(&owner, false);
            }
        });
        let weak = Rc::downgrade(owner);
        self.all_projects.connect_toggled(move |_| {
            if let Some(owner) = weak.upgrade() {
                owner.history.request(&owner, false);
            }
        });
        let weak = Rc::downgrade(owner);
        self.more.connect_clicked(move |_| {
            if let Some(owner) = weak.upgrade() {
                owner.history.request(&owner, true);
            }
        });
        let weak = Rc::downgrade(owner);
        self.new_session.connect_clicked(move |_| {
            let Some(owner)=weak.upgrade().filter(|owner|owner.available()) else{return;};
            if !owner.can_change_session() {owner.show_error("Finish the current turn and send or clear your draft before switching conversations.");return;}
            let Some(cwd)=owner.agent_cwd() else {owner.show_error("The Agent has not reported its working directory yet.");return;};
            if owner.engine.agent_session_op(owner.pane,AgentSessionOpKind::New{cwd}) {owner.history.popover.popdown();}
        });
    }

    pub fn capabilities(&self, value: &Value) {
        self.can_list.set(value["list"].as_bool().unwrap_or(false));
        self.can_load.set(value["load"].as_bool().unwrap_or(false));
        self.all_projects.set_visible(self.can_list.get());
        self.refresh.set_visible(self.can_list.get());
        if !self.can_list.get() {
            self.status
                .set_text("This Agent does not provide conversation history.");
        }
    }

    pub fn update(&self, owner: &AgentPane) {
        self.button.set_sensitive(owner.available());
        self.new_session
            .set_sensitive(owner.can_change_session() && owner.agent_cwd().is_some());
        self.all_projects.set_sensitive(!self.loading.get());
        self.refresh.set_sensitive(!self.loading.get());
        self.more.set_sensitive(!self.loading.get());
        self.list
            .set_sensitive(owner.can_change_session() && self.can_load.get());
    }

    fn request(&self, owner: &Rc<AgentPane>, append: bool) {
        if !owner.available() || !self.can_list.get() || self.loading.get() {
            return;
        }
        let cursor = if append {
            let Some(cursor) = self.next_cursor.borrow().clone() else {
                return;
            };
            Some(cursor)
        } else {
            None
        };
        let cwd = if self.all_projects.is_active() {
            None
        } else {
            owner.agent_cwd()
        };
        if owner.engine.agent_session_op(
            owner.pane,
            AgentSessionOpKind::List {
                cwd,
                cursor,
                replace: !append,
            },
        ) {
            self.loading.set(true);
            self.status.set_text("Loading conversations…");
            self.update(owner);
        } else {
            self.status.set_text("Conversations could not be loaded.");
        }
    }

    pub fn result(&self, owner: &Rc<AgentPane>, result: &str) {
        self.loading.set(false);
        let parsed = serde_json::from_str::<Value>(result);
        let Ok(value) = parsed else {
            self.status
                .set_text("The conversation list could not be read.");
            self.update(owner);
            return;
        };
        if value["item"] == "sessionListFailed" {
            self.status.set_text(
                value["message"]
                    .as_str()
                    .unwrap_or("Conversations could not be loaded."),
            );
            self.update(owner);
            return;
        }
        if value["item"] != "sessionsListed" {
            self.update(owner);
            return;
        }
        let mut sessions = self.sessions.borrow_mut();
        if value["replace"].as_bool().unwrap_or(true) {
            sessions.clear();
        }
        for value in value["sessions"].as_array().into_iter().flatten() {
            if let Some(session) = Session::parse(value) {
                if let Some(previous) = sessions
                    .iter_mut()
                    .find(|previous| previous.id == session.id)
                {
                    *previous = session;
                } else if sessions.len() < MAX_SESSIONS {
                    sessions.push(session);
                }
            }
        }
        self.next_cursor
            .replace(value["next_cursor"].as_str().map(str::to_owned));
        self.more
            .set_visible(self.next_cursor.borrow().is_some() && sessions.len() < MAX_SESSIONS);
        self.status.set_text(if sessions.is_empty() {
            "No saved conversations"
        } else if sessions.len() == MAX_SESSIONS {
            "Showing the 500 most recent conversations"
        } else {
            ""
        });
        clear_box(&self.list);
        let current = owner
            .state
            .borrow()
            .as_ref()
            .and_then(|state| state.session_id.clone());
        for session in sessions.iter() {
            let row = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(4)
                .margin_top(6)
                .margin_bottom(6)
                .build();
            let label = gtk::Label::builder()
                .label(&session.title)
                .xalign(0.0)
                .wrap(true)
                .build();
            if Some(&session.id) == current.as_ref() {
                label.add_css_class("heading");
            }
            row.append(&label);
            let directory = gtk::Label::builder()
                .label(session.cwd.to_string_lossy())
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Start)
                .build();
            directory.add_css_class("dim-label");
            directory.add_css_class("caption");
            row.append(&directory);
            let button = gtk::Button::builder().child(&row).build();
            button.add_css_class("flat");
            let session = session.clone();
            let weak = Rc::downgrade(owner);
            button.connect_clicked(move |_| {
                let Some(owner)=weak.upgrade().filter(|owner|owner.available()) else{return;};
                if !owner.can_change_session() {owner.show_error("Finish the current turn and send or clear your draft before switching conversations.");return;}
                let snapshot=owner.engine.snapshot();
                let provider=snapshot.sessions.iter().flat_map(|s|&s.windows).find_map(|window|window.panes.get(&owner.pane)).and_then(|pane|match &pane.kind {PaneKindSnapshot::Agent(agent)=>Some(agent.provider),_=>None});
                let already_open=snapshot.sessions.iter().flat_map(|s|&s.windows).flat_map(|w|w.panes.values()).any(|pane|pane.id!=owner.pane && matches!(&pane.kind,PaneKindSnapshot::Agent(agent) if Some(agent.provider)==provider && agent.session_id.as_ref()==Some(&session.id)));
                if already_open {owner.show_error("That conversation is already open in another Agent pane.");return;}
                if owner.engine.agent_session_op(owner.pane,AgentSessionOpKind::Switch{session_id:session.id.clone(),cwd:session.cwd.clone(),additional_directories:session.additional_directories.clone()}) {owner.history.popover.popdown();}
            });
            self.list.append(&button);
        }
        self.update(owner);
    }

    pub fn disconnected(&self) {
        self.loading.set(false);
        self.popover.popdown();
    }
}

impl AgentPane {
    pub fn handle_sessions(self: &Rc<Self>, _request_id: u64, result: &str) {
        self.history.result(self, result);
    }

    fn agent_cwd(&self) -> Option<PathBuf> {
        self.engine
            .snapshot()
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .find_map(|window| window.panes.get(&self.pane))
            .and_then(|pane| match &pane.kind {
                PaneKindSnapshot::Agent(agent) => agent.cwd.clone(),
                _ => None,
            })
    }

    fn can_change_session(&self) -> bool {
        self.available()
            && self.state.borrow().as_ref().is_some_and(|state| {
                matches!(state.phase, AgentConnectionPhase::Ready) && state.queued_prompts == 0
            })
            && self.draft().trim().is_empty()
            && self.attachments.borrow().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sessions_preserve_daemon_paths_and_additional_directories() {
        let session=Session::parse(&json!({"sessionId":"abc","cwd":"/srv/project","additionalDirectories":["/srv/shared"],"title":"Fix tests"})).unwrap();
        assert_eq!(session.cwd, PathBuf::from("/srv/project"));
        assert_eq!(
            session.additional_directories,
            vec![PathBuf::from("/srv/shared")]
        );
        assert_eq!(session.title, "Fix tests");
    }

    #[test]
    fn invalid_restore_metadata_is_not_selectable() {
        assert!(Session::parse(&json!({"sessionId":"abc","cwd":""})).is_none());
        assert!(Session::parse(&json!({"sessionId":"","cwd":"/srv"})).is_none());
        assert!(
            Session::parse(&json!({"sessionId":"abc","cwd":"/srv","additionalDirectories":[""]}))
                .is_none()
        );
    }
}
