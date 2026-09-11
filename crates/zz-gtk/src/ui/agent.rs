mod attachments;
mod markup;
mod sessions;
mod transcript;

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
    rc::Rc,
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gtk::{glib, prelude::*};
use serde_json::Value;
use zz_protocol::{
    AgentCommand, AgentConnectionPhase, AgentImage, AgentPaneWire, GuiResponse,
    MAX_AGENT_PERMISSION_BYTES, MAX_AGENT_PROMPT_BYTES, PaneId,
};

use crate::engine::{Engine, HostId};
use transcript::{Block, Kind, Transcript};

struct TranscriptRow {
    root: gtk::Widget,
    title: gtk::Label,
    text: gtk::Label,
    shown: Block,
}

pub struct AgentPane {
    root: gtk::Box,
    engine: Arc<Engine>,
    pane: PaneId,
    host: HostId,
    title: gtk::Label,
    status: gtk::Label,
    error: gtk::Label,
    transcript: RefCell<Transcript>,
    rows: RefCell<HashMap<String, TranscriptRow>>,
    messages: gtk::Box,
    scroll: gtk::ScrolledWindow,
    empty: gtk::Label,
    earlier: gtk::Label,
    composer: gtk::TextView,
    send: gtk::Button,
    cancel: gtk::Button,
    unqueue: gtk::Button,
    permission: gtk::Box,
    settings: gtk::Box,
    history: sessions::SessionHistory,
    attach: gtk::Button,
    clear_attachments: gtk::Button,
    attachment_names: RefCell<Vec<String>>,
    images_supported: Cell<bool>,
    attachments: RefCell<Vec<AgentImage>>,
    attachment_label: gtk::Label,
    state: RefCell<Option<AgentPaneWire>>,
    connected: Cell<bool>,
    replay_pending: Cell<bool>,
    restored_prompts: RefCell<VecDeque<u64>>,
}

impl AgentPane {
    pub fn new(engine: Arc<Engine>, pane: PaneId) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("view");
        let header = gtk::Box::builder()
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(16)
            .margin_end(16)
            .build();
        let title = gtk::Label::builder()
            .label("Agent")
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        title.add_css_class("heading");
        let status = gtk::Label::new(Some("Connecting…"));
        status.add_css_class("dim-label");
        let options = gtk::MenuButton::builder()
            .icon_name("emblem-system-symbolic")
            .tooltip_text("Agent settings")
            .build();
        options.add_css_class("flat");
        let settings = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        let popover = gtk::Popover::new();
        popover.set_child(Some(&settings));
        options.set_popover(Some(&popover));
        let history = sessions::SessionHistory::new();
        header.append(&title);
        header.append(&status);
        header.append(&history.button);
        header.append(&options);
        root.append(&header);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let messages = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();
        let earlier = gtk::Label::builder()
            .label("Showing recent conversation")
            .wrap(true)
            .visible(false)
            .build();
        earlier.add_css_class("dim-label");
        messages.append(&earlier);
        let empty = gtk::Label::builder()
            .label("Start a conversation")
            .vexpand(true)
            .valign(gtk::Align::Center)
            .build();
        empty.add_css_class("title-2");
        empty.add_css_class("dim-label");
        messages.append(&empty);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&messages)
            .build();
        root.append(&scroll);

        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .margin_start(16)
            .margin_end(16)
            .margin_bottom(16)
            .margin_top(12)
            .build();
        let error = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .visible(false)
            .build();
        error.add_css_class("error");
        footer.append(&error);
        let permission = gtk::Box::new(gtk::Orientation::Vertical, 8);
        permission.set_visible(false);
        footer.append(&permission);
        let composer = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(12)
            .bottom_margin(12)
            .left_margin(12)
            .right_margin(12)
            .accepts_tab(false)
            .build();
        composer.update_property(&[gtk::accessible::Property::Label("Message the Agent")]);
        let input_scroll = gtk::ScrolledWindow::builder()
            .min_content_height(80)
            .max_content_height(200)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&composer)
            .build();
        input_scroll.add_css_class("card");
        footer.append(&input_scroll);
        let attachment_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .visible(false)
            .build();
        attachment_label.add_css_class("dim-label");
        footer.append(&attachment_label);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let attach = gtk::Button::builder()
            .icon_name("mail-attachment-symbolic")
            .tooltip_text("Attach images or text files")
            .build();
        attach.add_css_class("flat");
        let clear_attachments = gtk::Button::with_label("Remove images");
        clear_attachments.add_css_class("flat");
        clear_attachments.set_visible(false);
        let unqueue = gtk::Button::with_label("Edit queued messages");
        unqueue.set_visible(false);
        unqueue.add_css_class("flat");
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        let cancel = gtk::Button::with_label("Stop");
        cancel.set_visible(false);
        let send = gtk::Button::with_label("Send");
        send.add_css_class("suggested-action");
        actions.append(&attach);
        actions.append(&clear_attachments);
        actions.append(&unqueue);
        actions.append(&spacer);
        actions.append(&cancel);
        actions.append(&send);
        footer.append(&actions);
        root.append(&footer);

        let view = Rc::new(Self {
            root,
            host: engine.active_host(),
            engine,
            pane,
            title,
            status,
            error,
            transcript: RefCell::default(),
            rows: RefCell::default(),
            messages,
            scroll,
            empty,
            earlier,
            composer,
            send,
            cancel,
            unqueue,
            permission,
            settings,
            history,
            attach,
            clear_attachments,
            attachment_names: RefCell::default(),
            images_supported: Cell::new(false),
            attachments: RefCell::default(),
            attachment_label,
            state: RefCell::default(),
            connected: Cell::new(true),
            replay_pending: Cell::new(false),
            restored_prompts: RefCell::default(),
        });
        view.connect();
        view.refresh();
        view.request_replay();
        view
    }

    fn connect(self: &Rc<Self>) {
        self.history.connect(self);
        let weak = Rc::downgrade(self);
        self.attach.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.choose_attachments();
            }
        });
        let weak = Rc::downgrade(self);
        self.clear_attachments.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.attachments.borrow_mut().clear();
                view.attachment_names.borrow_mut().clear();
                view.update_actions();
            }
        });
        let weak = Rc::downgrade(self);
        self.send.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade().filter(|view| view.available()) {
                view.submit();
            }
        });
        let weak = Rc::downgrade(self);
        self.cancel.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade().filter(|view| view.available()) {
                view.engine.agent_cancel(view.pane);
            }
        });
        let weak = Rc::downgrade(self);
        self.unqueue.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade().filter(|view| view.available()) {
                view.engine.agent_unqueue(view.pane);
            }
        });
        let weak = Rc::downgrade(self);
        self.composer.buffer().connect_changed(move |_| {
            if let Some(view) = weak.upgrade().filter(|view| view.available()) {
                view.update_actions();
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_enter(move |_| {
            if let Some(view) = weak.upgrade().filter(|view| view.available()) {
                view.engine.select_pane(view.pane);
            }
        });
        self.root.add_controller(focus);
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub fn focus(&self) {
        self.composer.grab_focus();
    }

    pub fn park(&self) {
        self.connected.set(false);
        self.replay_pending.set(false);
        self.history.disconnected();
    }

    pub fn resume(self: &Rc<Self>) {
        if self.engine.active_host() == self.host {
            self.set_connected(true);
        }
    }

    pub fn set_connected(self: &Rc<Self>, connected: bool) {
        let previous = self.connected.replace(connected);
        if !connected {
            self.history.disconnected();
            self.replay_pending.set(false);
        }
        if connected && !previous {
            self.transcript.borrow_mut().prepare_replay();
            self.render_transcript();
            self.request_replay();
        }
        self.refresh();
    }

    pub fn request_replay(&self) {
        if !self.available() || self.replay_pending.get() {
            return;
        }
        let cursor = self.transcript.borrow().cursor;
        if self.engine.agent_replay(self.pane, cursor) {
            self.replay_pending.set(true);
        }
    }

    pub fn lagged(&self) {
        self.replay_pending.set(false);
        self.request_replay();
    }

    fn available(&self) -> bool {
        self.connected.get() && self.engine.active_host() == self.host
    }

    pub fn apply_updates(self: &Rc<Self>, first_seq: u64, items: &[Vec<u8>]) {
        let result = self.transcript.borrow_mut().apply(first_seq, items);
        self.replay_pending.set(false);
        if let Some(capabilities) = result.capabilities {
            self.images_supported
                .set(capabilities["images"].as_bool().unwrap_or(false));
            self.history.capabilities(&capabilities);
            self.update_actions();
        }
        for restore in result.restores {
            self.restore_prompts(&restore);
        }
        self.render_transcript();
        if result.needs_replay {
            self.request_replay();
        }
    }

    pub fn refresh(self: &Rc<Self>) {
        let next = self.engine.agent_state(self.pane);
        let session_changed = next
            .as_ref()
            .and_then(|state| state.session_id.as_deref())
            .is_some_and(|session| self.transcript.borrow_mut().set_session(session));
        if session_changed {
            self.render_transcript();
        }
        let changed = *self.state.borrow() != next;
        if changed {
            self.state.replace(next);
        }
        let state = self.state.borrow();
        self.title.set_text(
            state
                .as_ref()
                .and_then(|s| s.title.as_deref())
                .unwrap_or("Agent"),
        );
        let status = if self.connected.get() {
            match state.as_ref().map(|s| &s.phase) {
                Some(AgentConnectionPhase::Ready) => "Ready",
                Some(AgentConnectionPhase::Running) => "Working…",
                Some(AgentConnectionPhase::AwaitingPermission) => "Needs approval",
                Some(AgentConnectionPhase::Failed { .. }) => "Failed",
                _ => "Connecting…",
            }
        } else {
            "Reconnecting…"
        };
        self.status.set_text(status);
        if let Some(state) = state.as_ref() {
            let error = state.error.as_deref().or(match &state.phase {
                AgentConnectionPhase::Failed { message } => Some(message.as_str()),
                _ => None,
            });
            if let Some(error) = error {
                self.show_error(error);
            } else if changed {
                self.error.set_visible(false);
            }
            if changed {
                self.render_permission(state);
                self.render_settings(state);
            }
        }
        self.permission.set_sensitive(self.connected.get());
        self.settings.set_sensitive(self.connected.get());
        drop(state);
        self.update_actions();
    }

    fn update_actions(&self) {
        let state = self.state.borrow();
        let ready = state.as_ref().is_some_and(|s| {
            matches!(
                s.phase,
                AgentConnectionPhase::Ready
                    | AgentConnectionPhase::Running
                    | AgentConnectionPhase::AwaitingPermission
            )
        });
        let running = state.as_ref().is_some_and(|s| {
            matches!(
                s.phase,
                AgentConnectionPhase::Running | AgentConnectionPhase::AwaitingPermission
            )
        });
        let queued = state.as_ref().map_or(0, |s| s.queued_prompts);
        self.send.set_label(if running { "Queue" } else { "Send" });
        let text = self.draft();
        self.send.set_sensitive(
            self.connected.get()
                && ready
                && (!text.trim().is_empty() || !self.attachments.borrow().is_empty())
                && self.prompt_bytes() <= MAX_AGENT_PROMPT_BYTES,
        );
        self.cancel.set_visible(running);
        self.cancel.set_sensitive(self.connected.get());
        self.unqueue.set_visible(queued > 0);
        self.unqueue.set_sensitive(self.connected.get());
        self.unqueue
            .set_label(&format!("Edit queued messages ({queued})"));
        self.attach.set_sensitive(self.available());
        self.history.update(self);
        let images = self.attachments.borrow().len();
        self.clear_attachments.set_visible(images > 0);
        self.attachment_label.set_visible(images > 0);
        self.attachment_label
            .set_text(&self.attachment_names.borrow().join(", "));
    }

    fn draft(&self) -> String {
        let buffer = self.composer.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string()
    }

    fn submit(&self) {
        if !self.available() {
            return;
        }
        let text = self.draft();
        if self.prompt_bytes() > MAX_AGENT_PROMPT_BYTES {
            self.show_error("This message is too long to send.");
            return;
        }
        if text.trim().is_empty() && self.attachments.borrow().is_empty() {
            return;
        }
        let images = self.attachments.borrow().clone();
        let image_count = images.len();
        let ready = self
            .state
            .borrow()
            .as_ref()
            .is_some_and(|state| matches!(state.phase, AgentConnectionPhase::Ready));
        if self.engine.agent_prompt(self.pane, text.clone(), images) {
            if ready {
                self.transcript.borrow_mut().local_echo(&text, image_count);
                self.render_transcript();
            }
            self.attachments.borrow_mut().clear();
            self.attachment_names.borrow_mut().clear();
            self.composer.buffer().set_text("");
            self.error.set_visible(false);
            self.update_actions();
        } else {
            self.show_error("The message could not be sent. Your draft is still here.");
        }
    }

    pub fn handle_command(&self, request_id: u64, command: AgentCommand) {
        if !self.available() {
            return;
        }
        let result = match command {
            AgentCommand::ComposerAppend { text } => {
                let mut draft = self.draft();
                if !draft.is_empty() && !text.is_empty() {
                    draft.push('\n');
                }
                draft.push_str(&text);
                if draft.len() > MAX_AGENT_PROMPT_BYTES {
                    Err("The composer draft is too long.")
                } else {
                    self.composer.buffer().set_text(&draft);
                    Ok(())
                }
            }
            AgentCommand::Prompt { text } => {
                if !self
                    .state
                    .borrow()
                    .as_ref()
                    .is_some_and(|s| matches!(s.phase, AgentConnectionPhase::Ready))
                {
                    Err("The Agent is not ready for a new prompt.")
                } else if self
                    .engine
                    .agent_prompt(self.pane, text.clone(), Vec::new())
                {
                    self.transcript.borrow_mut().local_echo(&text, 0);
                    self.render_transcript();
                    Ok(())
                } else {
                    Err("The Agent prompt could not be sent.")
                }
            }
        };
        self.engine.respond_gui(match result {
            Ok(()) => GuiResponse::Success {
                request_id,
                output: String::new(),
            },
            Err(message) => GuiResponse::Error {
                request_id,
                message: message.into(),
            },
        });
    }

    fn render_transcript(&self) {
        let adjustment = self.scroll.vadjustment();
        let at_bottom = adjustment.value() + adjustment.page_size() >= adjustment.upper() - 48.0;
        let transcript = self.transcript.borrow();
        self.empty.set_visible(transcript.blocks.is_empty());
        self.earlier.set_visible(transcript.trimmed);
        let mut rows = self.rows.borrow_mut();
        rows.retain(|id, row| {
            if transcript.blocks.iter().any(|b| &b.id == id) {
                true
            } else {
                self.messages.remove(&row.root);
                false
            }
        });
        let mut previous: gtk::Widget = self.empty.clone().upcast();
        for block in &transcript.blocks {
            let row = rows.entry(block.id.clone()).or_insert_with(|| {
                let row = transcript_row(block);
                self.messages.append(&row.root);
                row
            });
            if row.shown != *block {
                row.title.set_text(&block.title);
                set_block_text(&row.text, block);
                row.shown.clone_from(block);
            }
            if row.root.prev_sibling().as_ref() != Some(&previous) {
                self.messages
                    .reorder_child_after(&row.root, Some(&previous));
            }
            previous = row.root.clone();
        }
        if at_bottom {
            let weak = self.scroll.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(scroll) = weak.upgrade() {
                    let adjustment = scroll.vadjustment();
                    adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(0.0));
                }
            });
        }
    }

    fn render_permission(self: &Rc<Self>, state: &AgentPaneWire) {
        clear_box(&self.permission);
        let Some(permission) = &state.pending_permission else {
            self.permission.set_visible(false);
            return;
        };
        self.permission.set_visible(true);
        let payload = serde_json::from_str::<Value>(&permission.payload).unwrap_or_default();
        let title = payload["toolCall"]["title"]
            .as_str()
            .unwrap_or("Allow this action?");
        let label = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .build();
        label.add_css_class("heading");
        self.permission.append(&label);
        let text = permission_details(&payload["toolCall"]);
        if !text.is_empty() {
            let details = gtk::Label::builder()
                .label(&text)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .selectable(true)
                .build();
            details.add_css_class("monospace");
            let scroll = gtk::ScrolledWindow::builder()
                .max_content_height(180)
                .propagate_natural_height(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&details)
                .build();
            self.permission.append(&scroll);
        }
        let actions = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(false)
            .column_spacing(8)
            .row_spacing(6)
            .build();
        for option in payload["options"].as_array().into_iter().flatten() {
            let Some(id) = option["optionId"].as_str() else {
                continue;
            };
            let name = option["name"].as_str().unwrap_or(id);
            let button = gtk::Button::with_label(name);
            let weak = Rc::downgrade(self);
            let id = id.to_owned();
            let request = permission.request_id;
            button.connect_clicked(move |_| {
                if let Some(view) = weak.upgrade().filter(|view| view.available())
                    && view
                        .engine
                        .agent_respond_permission(view.pane, request, Some(id.clone()))
                {
                    view.permission.set_sensitive(false);
                }
            });
            actions.insert(&button, -1);
        }
        let cancel = gtk::Button::with_label("Cancel request");
        let weak = Rc::downgrade(self);
        let request = permission.request_id;
        cancel.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade().filter(|view| view.available())
                && view
                    .engine
                    .agent_respond_permission(view.pane, request, None)
            {
                view.permission.set_sensitive(false);
            }
        });
        actions.insert(&cancel, -1);
        self.permission.append(&actions);
    }

    fn render_settings(self: &Rc<Self>, state: &AgentPaneWire) {
        clear_box(&self.settings);
        let modes = serde_json::from_str::<Value>(&state.modes).unwrap_or_default();
        if let Some(values) = modes["availableModes"].as_array() {
            let choices: Vec<_> = values
                .iter()
                .filter_map(|v| {
                    Some((v["id"].as_str()?.to_owned(), v["name"].as_str()?.to_owned()))
                })
                .collect();
            let weak = Rc::downgrade(self);
            self.setting_choices(
                "Mode",
                &choices,
                modes["currentModeId"].as_str(),
                move |id| {
                    if let Some(view) = weak.upgrade().filter(|view| view.available()) {
                        view.engine.agent_set_mode(view.pane, id);
                    }
                },
            );
        }
        let config = serde_json::from_str::<Value>(&state.config_options).unwrap_or_default();
        for option in config.as_array().into_iter().flatten() {
            let Some(id) = option["id"].as_str() else {
                continue;
            };
            let mut choices = Vec::new();
            for value in option["options"].as_array().into_iter().flatten() {
                if let Some(group) = value["options"].as_array() {
                    for value in group {
                        push_choice(value, &mut choices);
                    }
                } else {
                    push_choice(value, &mut choices);
                }
            }
            if choices.is_empty() {
                continue;
            }
            let weak = Rc::downgrade(self);
            let id = id.to_owned();
            self.setting_choices(
                option["name"].as_str().unwrap_or(&id),
                &choices,
                option["currentValue"].as_str(),
                {
                    let id = id.clone();
                    move |value| {
                        if let Some(view) = weak.upgrade().filter(|view| view.available()) {
                            view.engine
                                .agent_set_config_option(view.pane, id.clone(), value);
                        }
                    }
                },
            );
        }
        if matches!(
            state.phase,
            AgentConnectionPhase::Starting | AgentConnectionPhase::Failed { .. }
        ) {
            let auth = serde_json::from_str::<Value>(&state.auth_methods).unwrap_or_default();
            for method in auth.as_array().into_iter().flatten() {
                let Some(id) = method["id"].as_str() else {
                    continue;
                };
                let button = gtk::Button::with_label(method["name"].as_str().unwrap_or("Sign in"));
                let id = id.to_owned();
                let weak = Rc::downgrade(self);
                button.connect_clicked(move |_| {
                    if let Some(view) = weak.upgrade().filter(|view| view.available()) {
                        view.engine.agent_authenticate(view.pane, id.clone());
                    }
                });
                self.settings.append(&button);
            }
        }
        if self.settings.first_child().is_none() {
            self.settings
                .append(&gtk::Label::new(Some("No Agent settings available")));
        }
    }

    fn setting_choices(
        &self,
        name: &str,
        choices: &[(String, String)],
        selected: Option<&str>,
        on_change: impl Fn(String) + 'static,
    ) {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
        row.append(&gtk::Label::builder().label(name).xalign(0.0).build());
        let labels: Vec<_> = choices.iter().map(|(_, label)| label.as_str()).collect();
        let dropdown = gtk::DropDown::from_strings(&labels);
        dropdown.set_selected(
            choices
                .iter()
                .position(|(id, _)| Some(id.as_str()) == selected)
                .map_or(gtk::INVALID_LIST_POSITION, |index| index as u32),
        );
        let choices = choices.to_vec();
        dropdown.connect_selected_notify(move |dropdown| {
            if let Some((id, _)) = choices.get(dropdown.selected() as usize) {
                on_change(id.clone());
            }
        });
        row.append(&dropdown);
        self.settings.append(&row);
    }

    fn restore_prompts(&self, item: &Value) {
        let reclaim = item["reclaim_id"].as_u64();
        let restore_key = reclaim.map(|id| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            id.hash(&mut hasher);
            self.state
                .borrow()
                .as_ref()
                .and_then(|state| state.session_id.as_ref())
                .hash(&mut hasher);
            item["prompts"].to_string().hash(&mut hasher);
            hasher.finish()
        });
        if restore_key.is_some_and(|key| self.restored_prompts.borrow().contains(&key)) {
            if let Some(id) = reclaim {
                self.engine.agent_acknowledge_prompt_restore(self.pane, id);
            }
            return;
        }
        let own = self.engine.client_instance_id().map(|id| id.0);
        let mut text = String::new();
        let mut images = Vec::new();
        for prompt in item["prompts"].as_array().into_iter().flatten() {
            let owner = prompt["owner"].as_u64().unwrap_or(0);
            if owner != 0 && Some(owner) != own {
                continue;
            }
            let Some(decoded) = prompt["text"]
                .as_str()
                .and_then(|value| BASE64.decode(value).ok())
                .and_then(|bytes| String::from_utf8(bytes).ok())
            else {
                self.show_error("A queued message could not be restored.");
                return;
            };
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&decoded);
            for image in prompt["images"].as_array().into_iter().flatten() {
                let Some(data) = image["data"]
                    .as_str()
                    .and_then(|value| BASE64.decode(value).ok())
                else {
                    self.show_error("An attachment could not be restored.");
                    return;
                };
                images.push(AgentImage {
                    format: image["format"].as_str().unwrap_or("image/png").into(),
                    data,
                });
            }
        }
        if text.is_empty() && images.is_empty() {
            return;
        }
        let draft = self.draft();
        if !draft.is_empty() {
            text.push('\n');
            text.push_str(&draft);
        }
        self.attachment_names
            .borrow_mut()
            .extend(std::iter::repeat_n("Restored image".into(), images.len()));
        self.attachments.borrow_mut().extend(images);
        self.composer.buffer().set_text(&text);
        if let Some(id) = reclaim {
            if let Some(key) = restore_key {
                let mut restored = self.restored_prompts.borrow_mut();
                restored.push_back(key);
                while restored.len() > 64 {
                    restored.pop_front();
                }
            }
            self.engine.agent_acknowledge_prompt_restore(self.pane, id);
        }
        self.update_actions();
    }

    fn show_error(&self, text: &str) {
        self.error.set_text(text);
        self.error.set_visible(true);
    }
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn push_choice(value: &Value, choices: &mut Vec<(String, String)>) {
    if let (Some(id), Some(name)) = (value["value"].as_str(), value["name"].as_str()) {
        choices.push((id.into(), name.into()));
    }
}

fn transcript_row(block: &Block) -> TranscriptRow {
    let title = gtk::Label::builder()
        .label(&block.title)
        .xalign(0.0)
        .wrap(true)
        .build();
    title.add_css_class("caption-heading");
    let text = gtk::Label::builder()
        .label(&block.text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .hexpand(true)
        .build();
    set_block_text(&text, block);
    let container = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let root: gtk::Widget = if matches!(block.kind, Kind::Thought | Kind::Tool) {
        text.add_css_class("monospace");
        let expander = gtk::Expander::builder()
            .label_widget(&title)
            .child(&text)
            .build();
        expander.upcast()
    } else {
        container.append(&title);
        container.append(&text);
        if block.kind == Kind::User {
            container.add_css_class("card");
            container.set_margin_start(24);
        }
        if block.kind == Kind::Notice {
            text.add_css_class("error");
        }
        container.upcast()
    };
    TranscriptRow {
        root,
        title,
        text,
        shown: block.clone(),
    }
}

fn set_block_text(label: &gtk::Label, block: &Block) {
    if matches!(block.kind, Kind::Agent | Kind::Thought) {
        label.set_markup(&markup::render(&block.text));
    } else {
        label.set_text(&block.text);
    }
}

fn permission_details(tool: &Value) -> String {
    let content = tool["content"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| transcript::content_text(&item["content"]))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let mut text = if content.is_empty() {
        match &tool["rawInput"] {
            Value::Null => String::new(),
            Value::String(text) => text.clone(),
            value => {
                let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
                if pretty.len() <= MAX_AGENT_PERMISSION_BYTES {
                    pretty
                } else {
                    value.to_string()
                }
            }
        }
    } else {
        content
    };
    if text.len() > MAX_AGENT_PERMISSION_BYTES {
        text.truncate(text.floor_char_boundary(MAX_AGENT_PERMISSION_BYTES - 40));
        text.push_str("\n[Additional details omitted]");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn permission_details_fall_back_to_the_requested_arguments() {
        let details = permission_details(
            &json!({"title":"Run command","content":[],"rawInput":{"command":"cargo test","cwd":"/srv/project"}}),
        );
        assert!(details.contains("cargo test"));
        assert!(details.contains("/srv/project"));
        assert_eq!(
            permission_details(&json!({"rawInput":"git status"})),
            "git status"
        );
        assert_eq!(permission_details(&json!({"title":"Permission"})), "");
    }

    #[test]
    fn permission_details_prefer_display_content_and_bound_utf8() {
        let details = permission_details(
            &json!({"content":[{"type":"content","content":{"type":"text","text":"Read README.md"}}],"rawInput":{"path":"README.md"}}),
        );
        assert_eq!(details, "Read README.md");
        let details =
            permission_details(&json!({"rawInput":"🦀".repeat(MAX_AGENT_PERMISSION_BYTES)}));
        assert!(details.len() <= MAX_AGENT_PERMISSION_BYTES);
        assert!(details.ends_with("[Additional details omitted]"));
    }
}
