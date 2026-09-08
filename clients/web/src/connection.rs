#![cfg_attr(not(target_family = "wasm"), allow(dead_code))]

use std::{collections::HashMap, sync::Arc};

use gpui::{Context, EventEmitter};
use zz_client::agent_completion::AgentCommand;
use zz_client::{ClientCore, CoreEvent, Outbound};
use zz_protocol::{
    CommandInvocation, CommandRequest, CommandResponse, InputMessage, PaneId, ProtocolMessage,
    ServerError, SessionId,
};

const MAX_AGENT_HISTORY_BYTES: usize = 32 * 1024 * 1024;
const MAX_AGENT_HISTORY_ITEMS: usize = 10_000;

#[derive(Default)]
struct AgentCursor {
    last_seq: u64,
    replay_pending: bool,
    bytes: usize,
    history_supported: bool,
    session_load_supported: bool,
    session_delete_supported: bool,
    commands: Arc<[AgentCommand]>,
    command_session_reset: bool,
}

#[derive(serde::Deserialize)]
struct AgentEnvelope<'a> {
    seq: u64,
    #[serde(borrow)]
    item: &'a str,
    #[serde(default)]
    restoring: bool,
    #[serde(default)]
    capabilities: AgentCapabilities,
    #[serde(default)]
    update: Option<AgentCommandUpdate>,
    #[serde(default)]
    replay: Vec<AgentCommandUpdate>,
}

#[derive(serde::Deserialize)]
struct AgentCommandUpdate {
    #[serde(default, rename = "sessionUpdate")]
    kind: serde_json::Value,
    #[serde(default, rename = "availableCommands")]
    commands: serde_json::Value,
}

#[derive(Default, serde::Deserialize)]
struct AgentCapabilities {
    #[serde(default)]
    list: bool,
    #[serde(default)]
    load: bool,
    #[serde(default)]
    delete: bool,
}

impl AgentCursor {
    fn apply(
        &mut self,
        journal: &mut Vec<(u64, Vec<u8>)>,
        first_seq: u64,
        items: &[Vec<u8>],
    ) -> (Option<u64>, bool) {
        let mut accepted = false;
        let mut invalid = false;
        for (offset, bytes) in items.iter().enumerate() {
            let positional = first_seq.saturating_add(offset as u64);
            let Ok(item) = serde_json::from_slice::<AgentEnvelope<'_>>(bytes) else {
                if positional > self.last_seq.saturating_add(1) {
                    let request = (!self.replay_pending).then_some(self.last_seq);
                    self.replay_pending = true;
                    return (request, true);
                }
                self.last_seq = self.last_seq.max(positional);
                invalid = true;
                continue;
            };
            if item.seq <= self.last_seq {
                continue;
            }
            if item.item == "sessionReset" && item.restoring {
                self.last_seq = item.seq.saturating_sub(1);
            }
            if item.seq > self.last_seq.saturating_add(1) {
                let request = (!self.replay_pending).then_some(self.last_seq);
                self.replay_pending = true;
                return (request, invalid);
            }
            if item.item == "ready" {
                self.history_supported = item.capabilities.list;
                self.session_load_supported = item.capabilities.load;
                self.session_delete_supported = item.capabilities.delete;
            }
            if item.item == "sessionReset" {
                journal.clear();
                self.bytes = 0;
                self.commands = Arc::from([]);
                self.command_session_reset = true;
            } else if item.item == "sessionReady" {
                self.command_session_reset = false;
            } else if item.item == "sessionSwitched" {
                if !self.command_session_reset {
                    self.commands = Arc::from([]);
                }
                self.command_session_reset = false;
            }
            for update in item.update.iter().chain(&item.replay) {
                if update.kind.as_str() == Some("available_commands_update") {
                    self.commands = update
                        .commands
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(zz_protocol::MAX_AGENT_AVAILABLE_COMMANDS)
                        .filter_map(|command| {
                            Some(AgentCommand {
                                name: command.get("name")?.as_str()?.to_owned(),
                                description: command
                                    .get("description")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default()
                                    .to_owned(),
                                input_hint: command
                                    .pointer("/input/hint")
                                    .and_then(serde_json::Value::as_str)
                                    .map(ToOwned::to_owned),
                            })
                        })
                        .collect::<Vec<_>>()
                        .into();
                }
            }
            journal.push((item.seq, bytes.clone()));
            self.bytes = self.bytes.saturating_add(bytes.len());
            self.last_seq = item.seq;
            accepted = true;
        }
        if accepted {
            self.replay_pending = false;
        }
        (None, invalid)
    }

    fn trim(
        &mut self,
        journal: &mut Vec<(u64, Vec<u8>)>,
        byte_limit: usize,
        item_limit: usize,
    ) -> bool {
        let mut remove = 0;
        while remove < journal.len()
            && (self.bytes > byte_limit || journal.len() - remove > item_limit)
        {
            self.bytes = self.bytes.saturating_sub(journal[remove].1.len());
            remove += 1;
        }
        journal.drain(..remove);
        remove > 0
    }
}

pub struct Connection {
    pub core: ClientCore,
    pub status: String,
    pub connected: bool,
    pub agent_events: HashMap<PaneId, Vec<(u64, Vec<u8>)>>,
    agent_cursors: HashMap<PaneId, AgentCursor>,
    terminal_images: crate::terminal_images::TerminalImages,
    request_id: u64,
    remembered_session: Option<SessionId>,
    attaching: bool,
    retry_default: bool,
    focused: bool,
    #[cfg(target_family = "wasm")]
    socket: Option<browser::Socket>,
    #[cfg(target_family = "wasm")]
    reader: Option<gpui::Task<()>>,
    #[cfg(target_family = "wasm")]
    retry: Option<gpui::Task<()>>,
}

impl EventEmitter<CoreEvent> for Connection {}

impl Connection {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            core: ClientCore::new(),
            status: "Connecting…".into(),
            connected: false,
            agent_events: HashMap::new(),
            agent_cursors: HashMap::new(),
            terminal_images: crate::terminal_images::TerminalImages::default(),
            request_id: 1,
            remembered_session: None,
            attaching: false,
            retry_default: false,
            focused: true,
            #[cfg(target_family = "wasm")]
            socket: None,
            #[cfg(target_family = "wasm")]
            reader: None,
            #[cfg(target_family = "wasm")]
            retry: None,
        }
    }

    pub fn agent_history_supported(&self, pane: PaneId) -> bool {
        self.agent_cursors
            .get(&pane)
            .is_some_and(|cursor| cursor.history_supported)
    }

    pub fn agent_session_load_supported(&self, pane: PaneId) -> bool {
        self.agent_cursors
            .get(&pane)
            .is_some_and(|cursor| cursor.session_load_supported)
    }

    pub fn agent_session_delete_supported(&self, pane: PaneId) -> bool {
        self.agent_cursors
            .get(&pane)
            .is_some_and(|cursor| cursor.session_delete_supported)
    }

    pub fn agent_commands(&self, pane: PaneId) -> Arc<[AgentCommand]> {
        self.agent_cursors
            .get(&pane)
            .map_or_else(|| Arc::from([]), |cursor| cursor.commands.clone())
    }

    pub fn terminal_images(&self, pane: PaneId) -> Option<&crate::terminal_images::PaneImages> {
        self.terminal_images.pane(pane)
    }

    pub fn take_retired_terminal_images(&mut self) -> Vec<Arc<gpui::RenderImage>> {
        self.terminal_images.take_retired()
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.reconnect(cx);
    }

    pub fn client_instance_id(&self) -> Option<zz_protocol::ClientInstanceId> {
        #[cfg(target_family = "wasm")]
        {
            self.core.hello_received().then(browser::instance_id)
        }
        #[cfg(not(target_family = "wasm"))]
        {
            None
        }
    }

    pub fn reconnect(&mut self, cx: &mut Context<Self>) {
        self.connected = false;
        self.attaching = false;
        self.retry_default = false;
        self.status = "Connecting…".into();
        #[cfg(target_family = "wasm")]
        {
            self.retry = None;
            self.reader = None;
            self.socket = None;
            match browser::Socket::connect() {
                Ok((socket, receiver)) => {
                    self.socket = Some(socket);
                    self.reader = Some(cx.spawn(async move |entity, cx| {
                        while let Some(event) = receiver.recv().await {
                            if !entity
                                .update(cx, |this, cx| this.socket_event(event, cx))
                                .unwrap_or(false)
                            {
                                return;
                            }
                        }
                        let reason = receiver.close_reason();
                        let _ = entity.update(cx, |this, cx| this.disconnected(reason, cx));
                    }));
                    self.retry = Some(cx.spawn(async move |entity, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_secs(15))
                            .await;
                        let _ = entity.update(cx, |this, cx| {
                            this.disconnected("The daemon did not answer. Retrying…".into(), cx);
                        });
                    }));
                }
                Err(error) => self.disconnected(error, cx),
            }
        }
        #[cfg(not(target_family = "wasm"))]
        {
            self.status = "Open the browser client through zz-web to connect.".into();
        }
        cx.notify();
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn send(&mut self, message: ProtocolMessage, cx: &mut Context<Self>) {
        #[cfg(target_family = "wasm")]
        {
            if let Some(socket) = &self.socket {
                if let Err(error) = socket.send(&message) {
                    self.disconnected(error, cx);
                }
            } else {
                self.status = "Disconnected. This action was not sent.".into();
                cx.notify();
            }
        }
        #[cfg(not(target_family = "wasm"))]
        let _ = (message, cx);
    }

    pub fn command(&mut self, command: &str, args: Vec<String>, cx: &mut Context<Self>) {
        let request_id = self.request_id;
        self.request_id = self.request_id.wrapping_add(1).max(1);
        self.send(
            ProtocolMessage::CommandRequest(CommandRequest {
                request_id,
                command: CommandInvocation::new(command, args),
                prepared: false,
            }),
            cx,
        );
    }

    pub fn attach(&mut self, session: SessionId, cx: &mut Context<Self>) {
        self.remembered_session = Some(session);
        self.attach_target(session.to_string(), cx);
    }

    fn attach_target(&mut self, session: String, cx: &mut Context<Self>) {
        self.attaching = true;
        self.send(ProtocolMessage::Attach { session }, cx);
    }

    pub fn input(&mut self, _: PaneId, input: InputMessage, cx: &mut Context<Self>) {
        self.send(ProtocolMessage::Input(input), cx);
    }

    pub fn set_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.focused != focused {
            self.focused = focused;
            if self.core.attached_session().is_some() && !self.attaching {
                self.send(
                    ProtocolMessage::Input(InputMessage::ClientFocus { focused }),
                    cx,
                );
            }
        }
    }

    fn receive(&mut self, message: ProtocolMessage, cx: &mut Context<Self>) {
        self.core.handle_message(message);
        while let Some(Outbound::RequestFull(pane)) = self.core.poll_outbound() {
            self.send(ProtocolMessage::RequestFull { pane }, cx);
        }
        while let Some(event) = self.core.poll_event() {
            self.terminal_images.apply(&event);
            match &event {
                CoreEvent::HelloReceived => {
                    self.connected = true;
                    #[cfg(target_family = "wasm")]
                    {
                        self.retry = None;
                    }
                    self.agent_events.clear();
                    self.agent_cursors.clear();
                    self.attach_target(
                        self.remembered_session
                            .map_or_else(String::new, |id| id.to_string()),
                        cx,
                    );
                }
                CoreEvent::Attached { session } => {
                    self.attaching = false;
                    self.retry_default = false;
                    self.remembered_session = Some(*session);
                    self.status = "Connected".into();
                    self.agent_events.clear();
                    self.agent_cursors.clear();
                    self.send(
                        ProtocolMessage::Input(InputMessage::ClientFocus {
                            focused: self.focused,
                        }),
                        cx,
                    );
                }
                CoreEvent::CommandResponse(CommandResponse::Error {
                    request_id, error, ..
                }) => {
                    if *request_id == 0 && self.attaching {
                        self.attaching = false;
                        if matches!(error, ServerError::MissingTarget(_)) && !self.retry_default {
                            self.retry_default = true;
                            self.remembered_session = None;
                            self.attach_target(String::new(), cx);
                        } else {
                            self.status = error.to_string();
                        }
                    } else {
                        self.status = error.to_string();
                    }
                }
                CoreEvent::ClientMessage { text, .. } => self.status.clone_from(text),
                CoreEvent::Clipboard { text, .. } => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                }
                CoreEvent::OpenUri { uri, .. }
                    if uri.starts_with("https://")
                        || uri.starts_with("http://")
                        || uri.starts_with("mailto:") =>
                {
                    cx.open_url(uri);
                }
                CoreEvent::AgentStateChanged { pane, .. } => {
                    if !self.agent_cursors.contains_key(pane) {
                        self.request_agent_replay(*pane, cx);
                    }
                }
                CoreEvent::AgentUpdates {
                    pane,
                    first_seq,
                    items,
                } => {
                    let cursor = self.agent_cursors.entry(*pane).or_default();
                    let journal = self.agent_events.entry(*pane).or_default();
                    let (replay, invalid) = cursor.apply(journal, *first_seq, items);
                    let pane_trimmed =
                        cursor.trim(journal, MAX_AGENT_HISTORY_BYTES, MAX_AGENT_HISTORY_ITEMS);
                    let history_trimmed = self.trim_agent_history();
                    if pane_trimmed || history_trimmed {
                        self.status =
                            "Showing recent agent history to limit browser memory.".into();
                    }
                    if invalid {
                        self.status = "Some agent updates could not be read.".into();
                    }
                    if let Some(from_seq) = replay {
                        self.send(
                            ProtocolMessage::AgentReplay {
                                pane: *pane,
                                from_seq,
                            },
                            cx,
                        );
                    }
                }
                CoreEvent::AgentLagged { pane, .. } => {
                    self.request_agent_replay(*pane, cx);
                }
                CoreEvent::PaneRemoved { pane } => {
                    self.agent_events.remove(pane);
                    self.agent_cursors.remove(pane);
                }
                CoreEvent::AgentCommand { request_id, .. } => self.send(
                    ProtocolMessage::GuiResponse(zz_protocol::GuiResponse::Error {
                        request_id: *request_id,
                        message: "This desktop action is unavailable in the browser client.".into(),
                    }),
                    cx,
                ),
                CoreEvent::Detached { .. } => {
                    self.status = "Detached".into();
                    if self.core.last_detach_was_session_destroyed() {
                        self.remembered_session = None;
                        self.attach_target(String::new(), cx);
                    }
                }
                CoreEvent::ServerStopping => self.status = "Daemon stopped".into(),
                _ => {}
            }
            cx.emit(event);
        }
        cx.notify();
    }

    fn request_agent_replay(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let cursor = self.agent_cursors.entry(pane).or_default();
        cursor.replay_pending = true;
        let from_seq = cursor.last_seq;
        self.send(ProtocolMessage::AgentReplay { pane, from_seq }, cx);
    }

    fn trim_agent_history(&mut self) -> bool {
        let mut bytes: usize = self.agent_cursors.values().map(|cursor| cursor.bytes).sum();
        let mut trimmed = false;
        while bytes > MAX_AGENT_HISTORY_BYTES {
            let Some((&pane, _)) = self
                .agent_cursors
                .iter()
                .max_by_key(|(_, cursor)| cursor.bytes)
            else {
                break;
            };
            let cursor = self
                .agent_cursors
                .get_mut(&pane)
                .expect("agent cursor exists");
            let Some(journal) = self.agent_events.get_mut(&pane) else {
                break;
            };
            let old_bytes = cursor.bytes;
            let limit = old_bytes.saturating_sub(bytes - MAX_AGENT_HISTORY_BYTES);
            trimmed |= cursor.trim(journal, limit, MAX_AGENT_HISTORY_ITEMS);
            bytes -= old_bytes - cursor.bytes;
        }
        trimmed
    }

    #[cfg(target_family = "wasm")]
    fn disconnected(&mut self, reason: String, cx: &mut Context<Self>) {
        self.connected = false;
        self.attaching = false;
        self.status = reason;
        self.socket = None;
        self.reader = None;
        self.retry = Some(cx.spawn(async move |entity, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            let _ = entity.update(cx, Self::reconnect);
        }));
        cx.notify();
    }

    #[cfg(target_family = "wasm")]
    fn socket_event(&mut self, event: inbox::SocketEvent, cx: &mut Context<Self>) -> bool {
        match event {
            inbox::SocketEvent::Opened => {
                self.focused = web_sys::window()
                    .and_then(|window| window.document())
                    .is_some_and(|document| document.has_focus().unwrap_or(false));
                self.send(ProtocolMessage::ClientHello(browser::hello()), cx);
            }
            inbox::SocketEvent::Frame(bytes) => match zz_protocol::decode_protocol_frame(&bytes) {
                Ok(message) => self.receive(message, cx),
                Err(error) => {
                    self.disconnected(format!("Connection rejected: {error}. Rebuild zz-web and the browser client together."), cx);
                    return false;
                }
            },
            inbox::SocketEvent::Focus(focused) => self.set_focused(focused, cx),
        }
        cx.notify();
        self.socket.is_some()
    }
}

#[cfg(any(target_family = "wasm", test))]
mod inbox {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[derive(Debug, PartialEq)]
    pub enum SocketEvent {
        Opened,
        Frame(Vec<u8>),
        Focus(bool),
    }

    struct State {
        bytes: Cell<usize>,
        byte_limit: usize,
        reason: RefCell<Option<String>>,
    }

    #[derive(Clone)]
    pub struct Sender {
        sender: async_channel::Sender<SocketEvent>,
        state: Rc<State>,
    }

    pub struct Receiver {
        receiver: async_channel::Receiver<SocketEvent>,
        state: Rc<State>,
    }

    pub fn channel(capacity: usize, byte_limit: usize) -> (Sender, Receiver) {
        let (sender, receiver) = async_channel::bounded(capacity);
        let state = Rc::new(State {
            bytes: Cell::new(0),
            byte_limit,
            reason: RefCell::new(None),
        });
        (
            Sender {
                sender,
                state: state.clone(),
            },
            Receiver { receiver, state },
        )
    }

    impl Sender {
        fn accepts(&self, bytes: usize) -> bool {
            if self.sender.is_closed() {
                return false;
            }
            if self.sender.is_full()
                || bytes > self.state.byte_limit.saturating_sub(self.state.bytes.get())
            {
                self.close(
                    "Too much pending output. Reconnecting to refresh the workspace.".into(),
                );
                return false;
            }
            true
        }

        pub fn frame(&self, length: usize, copy: impl FnOnce() -> Vec<u8>) -> bool {
            self.accepts(length) && self.push(SocketEvent::Frame(copy()))
        }

        pub fn push(&self, event: SocketEvent) -> bool {
            let bytes = match &event {
                SocketEvent::Frame(bytes) => bytes.len(),
                _ => 0,
            };
            if !self.accepts(bytes) {
                return false;
            }
            if self.sender.try_send(event).is_ok() {
                self.state.bytes.set(self.state.bytes.get() + bytes);
                true
            } else {
                self.close(
                    "Too much pending output. Reconnecting to refresh the workspace.".into(),
                );
                false
            }
        }

        pub fn close(&self, reason: String) {
            self.state.reason.borrow_mut().get_or_insert(reason);
            self.sender.close();
        }
    }

    impl Receiver {
        pub async fn recv(&self) -> Option<SocketEvent> {
            if self.state.reason.borrow().is_some() {
                return None;
            }
            let event = self.receiver.recv().await.ok()?;
            if let SocketEvent::Frame(bytes) = &event {
                self.state
                    .bytes
                    .set(self.state.bytes.get().saturating_sub(bytes.len()));
            }
            Some(event)
        }

        pub fn close_reason(&self) -> String {
            self.state
                .reason
                .borrow()
                .clone()
                .unwrap_or_else(|| "Connection lost. Retrying…".into())
        }
    }
}

#[cfg(target_family = "wasm")]
mod browser {
    use wasm_bindgen::{JsCast, closure::Closure};
    use zz_protocol::{
        ClientHello, ClientInstanceId, ClientKind, PROTOCOL_VERSION, ProtocolMessage,
    };

    use super::inbox::{self, Receiver, Sender, SocketEvent};

    const CONNECTION_FAILED: &str =
        "Cannot connect to zz. Start the daemon and check the gateway socket. Retrying…";

    pub struct Socket {
        ws: web_sys::WebSocket,
        sender: Sender,
        open: Closure<dyn FnMut(web_sys::Event)>,
        message: Closure<dyn FnMut(web_sys::MessageEvent)>,
        close: Closure<dyn FnMut(web_sys::CloseEvent)>,
        error: Closure<dyn FnMut(web_sys::Event)>,
        focus: Closure<dyn FnMut(web_sys::Event)>,
        blur: Closure<dyn FnMut(web_sys::Event)>,
    }

    impl Socket {
        pub fn connect() -> Result<(Self, Receiver), String> {
            let window = web_sys::window().ok_or("Browser window unavailable")?;
            let location = window.location();
            let scheme = if location.protocol().unwrap_or_default() == "https:" {
                "wss"
            } else {
                "ws"
            };
            let url = format!(
                "{scheme}://{}/ws",
                location.host().map_err(|_| "Missing gateway host")?
            );
            let ws = web_sys::WebSocket::new(&url).map_err(|_| "Could not open WebSocket")?;
            ws.set_binary_type(web_sys::BinaryType::Arraybuffer);
            let (sender, receiver) = inbox::channel(64, zz_protocol::MAX_ENCODED_FRAME_BYTES);
            let tx = sender.clone();
            let open_socket = ws.clone();
            let open = Closure::new(move |_: web_sys::Event| {
                if !tx.push(SocketEvent::Opened) {
                    let _ = open_socket.close_with_code_and_reason(4008, "Client cannot keep up");
                }
            });
            ws.set_onopen(Some(open.as_ref().unchecked_ref()));
            let tx = sender.clone();
            let overflow_socket = ws.clone();
            let message = Closure::new(move |event: web_sys::MessageEvent| {
                let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() else {
                    tx.close("Unexpected gateway message. Reconnecting…".into());
                    let _ =
                        overflow_socket.close_with_code_and_reason(4002, "Binary frames required");
                    return;
                };
                let length = buffer.byte_length() as usize;
                if length > zz_protocol::MAX_ENCODED_FRAME_BYTES {
                    tx.close("Gateway frame too large. Reconnecting…".into());
                    let _ = overflow_socket.close_with_code_and_reason(4009, "Frame too large");
                } else if !tx.frame(length, || js_sys::Uint8Array::new(&buffer).to_vec()) {
                    let _ =
                        overflow_socket.close_with_code_and_reason(4008, "Client cannot keep up");
                }
            });
            ws.set_onmessage(Some(message.as_ref().unchecked_ref()));
            let tx = sender.clone();
            let close = Closure::new(move |event: web_sys::CloseEvent| {
                let reason = if event.reason().is_empty() {
                    "Connection lost. Retrying…".into()
                } else {
                    event.reason()
                };
                tx.close(reason);
            });
            ws.set_onclose(Some(close.as_ref().unchecked_ref()));
            let tx = sender.clone();
            let error = Closure::new(move |_: web_sys::Event| tx.close(CONNECTION_FAILED.into()));
            ws.set_onerror(Some(error.as_ref().unchecked_ref()));
            let focus = focus_callback(sender.clone(), ws.clone(), true);
            let blur = focus_callback(sender.clone(), ws.clone(), false);
            let socket = Self {
                ws,
                sender,
                open,
                message,
                close,
                error,
                focus,
                blur,
            };
            window
                .add_event_listener_with_callback("focus", socket.focus.as_ref().unchecked_ref())
                .map_err(|_| "Cannot watch browser focus")?;
            window
                .add_event_listener_with_callback("blur", socket.blur.as_ref().unchecked_ref())
                .map_err(|_| "Cannot watch browser focus")?;
            Ok((socket, receiver))
        }

        pub fn send(&self, message: &ProtocolMessage) -> Result<(), String> {
            if self.ws.ready_state() != web_sys::WebSocket::OPEN {
                return Err("Connection closed. This action was not sent. Reconnecting…".into());
            }
            let frame =
                zz_protocol::encode_protocol_message(message).map_err(|error| error.to_string())?;
            if (self.ws.buffered_amount() as usize).saturating_add(frame.len()) > 4 * 1024 * 1024 {
                return Err(
                    "Connection overloaded. This action was not sent. Reconnecting…".into(),
                );
            }
            self.ws
                .send_with_u8_array(&frame)
                .map_err(|_| "Could not send this action to the daemon. Reconnecting…".into())
        }
    }

    impl Drop for Socket {
        fn drop(&mut self) {
            self.ws.set_onopen(None);
            self.ws.set_onmessage(None);
            self.ws.set_onclose(None);
            self.ws.set_onerror(None);
            if let Some(window) = web_sys::window() {
                let _ = window.remove_event_listener_with_callback(
                    "focus",
                    self.focus.as_ref().unchecked_ref(),
                );
                let _ = window.remove_event_listener_with_callback(
                    "blur",
                    self.blur.as_ref().unchecked_ref(),
                );
            }
            let _ = self.ws.close();
            self.sender.close("Connection closed. Retrying…".into());
            let _ = (&self.open, &self.message, &self.close, &self.error);
        }
    }

    fn focus_callback(
        sender: Sender,
        socket: web_sys::WebSocket,
        focused: bool,
    ) -> Closure<dyn FnMut(web_sys::Event)> {
        Closure::new(move |_: web_sys::Event| {
            if !sender.push(SocketEvent::Focus(focused)) {
                let _ = socket.close_with_code_and_reason(4008, "Client cannot keep up");
            }
        })
    }

    thread_local! {
        static INSTANCE_ID: ClientInstanceId = ClientInstanceId(js_sys::Math::random().to_bits().max(1));
    }

    pub fn instance_id() -> ClientInstanceId {
        INSTANCE_ID.with(|id| *id)
    }

    pub fn hello() -> ClientHello {
        ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_instance_id: instance_id(),
            kind: ClientKind::Interactive,
            device_name: Some("Browser".into()),
            capabilities: vec![
                ClientHello::CLIENT_TERMINAL_CAPABILITY.into(),
                ClientHello::CLIENT_UTF8_CAPABILITY.into(),
            ],
            color_scheme: None,
            origin: None,
            working_directory: None,
            environment: Vec::new(),
            process_id: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    use super::{
        AgentCursor,
        inbox::{self, SocketEvent},
    };

    fn blob(seq: u64) -> Vec<u8> {
        format!(r#"{{"seq":{seq},"item":"promptAccepted","turn_id":1}}"#).into_bytes()
    }

    fn reset(seq: u64, restoring: bool) -> Vec<u8> {
        format!(r#"{{"seq":{seq},"item":"sessionReset","restoring":{restoring}}}"#).into_bytes()
    }

    #[test]
    fn agent_commands_survive_history_trimming_and_follow_session_replays() {
        let catalog = |name| {
            serde_json::json!({
                "sessionUpdate":"available_commands_update",
                "availableCommands":[{"name":name,"description":"Review changes","input":{"hint":"branch or files"}}]
            })
        };
        let message = |seq, update| {
            serde_json::to_vec(&serde_json::json!({"seq":seq,"item":"update","update":update}))
                .unwrap()
        };
        let mut cursor = AgentCursor::default();
        let mut journal = Vec::new();
        let initial = message(1, catalog("review"));
        assert_eq!(
            cursor.apply(&mut journal, 1, std::slice::from_ref(&initial)),
            (None, false)
        );
        assert_eq!(cursor.commands[0].name, "review");
        assert_eq!(
            cursor.commands[0].input_hint.as_deref(),
            Some("branch or files")
        );
        assert!(cursor.trim(&mut journal, 0, 0));
        assert!(journal.is_empty());
        assert_eq!(cursor.commands[0].name, "review");
        cursor.apply(&mut journal, 2, &[reset(2, true)]);
        assert!(cursor.commands.is_empty());
        cursor.apply(&mut journal, 3, &[message(3, catalog("explain"))]);
        cursor.apply(
            &mut journal,
            4,
            &[br#"{"seq":4,"item":"sessionSwitched","replay":[]}"#.to_vec()],
        );
        assert_eq!(cursor.commands[0].name, "explain");
        cursor.apply(&mut journal, 1, &[initial]);
        assert_eq!(cursor.commands[0].name, "explain");
        let switched = serde_json::to_vec(
            &serde_json::json!({"seq":5,"item":"sessionSwitched","replay":[catalog("test")]}),
        )
        .unwrap();
        cursor.apply(&mut journal, 5, &[switched]);
        assert_eq!(cursor.commands.len(), 1);
        assert_eq!(cursor.commands[0].name, "test");
        cursor.apply(&mut journal, 6, &[message(6, serde_json::json!({"sessionUpdate":"available_commands_update","availableCommands":[]}))]);
        assert!(cursor.commands.is_empty());
    }

    fn ready<T>(future: impl Future<Output = T>) -> T {
        let mut future = pin!(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("expected a ready queue operation"),
        }
    }

    #[test]
    fn agent_replay_accepts_sequence_one_and_deduplicates_overlap() {
        let mut cursor = AgentCursor::default();
        let mut journal = Vec::new();
        assert_eq!(
            cursor.apply(&mut journal, 1, &[blob(1), blob(2)]),
            (None, false)
        );
        assert_eq!(
            cursor.apply(&mut journal, 1, &[blob(1), blob(2), blob(3)]),
            (None, false)
        );
        assert_eq!(
            journal.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(cursor.last_seq, 3);
        assert_eq!(
            cursor.bytes,
            journal.iter().map(|(_, bytes)| bytes.len()).sum::<usize>()
        );
    }

    #[test]
    fn agent_gap_requests_one_inclusive_replay_until_it_closes() {
        let mut cursor = AgentCursor::default();
        let mut journal = Vec::new();
        cursor.apply(&mut journal, 1, &[blob(1)]);
        assert_eq!(cursor.apply(&mut journal, 4, &[blob(4)]), (Some(1), false));
        assert_eq!(cursor.apply(&mut journal, 5, &[blob(5)]), (None, false));
        assert_eq!(cursor.apply(&mut journal, 1, &[blob(1)]), (None, false));
        assert!(cursor.replay_pending);
        assert_eq!(
            cursor.apply(&mut journal, 1, &[blob(1), blob(2), blob(3), blob(4)]),
            (None, false)
        );
        assert!(!cursor.replay_pending);
        assert_eq!(cursor.last_seq, 4);
        assert_eq!(cursor.apply(&mut journal, 7, &[blob(7)]), (Some(4), false));
    }

    #[test]
    fn evicted_agent_history_accepts_a_restoring_reset_at_a_new_sequence() {
        let mut cursor = AgentCursor::default();
        let mut journal = Vec::new();
        cursor.apply(&mut journal, 1, &[blob(1), blob(2)]);
        assert_eq!(
            cursor.apply(&mut journal, 40, &[reset(40, true), blob(41)]),
            (None, false)
        );
        assert_eq!(
            journal.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            [40, 41]
        );
        assert_eq!(cursor.last_seq, 41);
        assert!(!cursor.replay_pending);
        assert_eq!(
            cursor.bytes,
            journal.iter().map(|(_, bytes)| bytes.len()).sum::<usize>()
        );
        assert_eq!(
            cursor.apply(&mut journal, 80, &[reset(80, false)]),
            (Some(41), false)
        );
    }

    #[test]
    fn session_switch_completion_keeps_streamed_history() {
        let mut cursor = AgentCursor::default();
        let mut journal = Vec::new();
        let switched = br#"{"seq":3,"item":"sessionSwitched","replay":[]}"#.to_vec();
        cursor.apply(&mut journal, 1, &[reset(1, true), blob(2), switched]);
        assert_eq!(
            journal.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[test]
    fn agent_capabilities_survive_session_reset_and_transcript_eviction() {
        let mut cursor = AgentCursor::default();
        let mut journal = Vec::new();
        assert!(!cursor.history_supported);
        assert!(!cursor.session_load_supported);
        assert!(!cursor.session_delete_supported);
        let ready =
            br#"{"seq":1,"item":"ready","capabilities":{"list":true,"load":true,"delete":true}}"#
                .to_vec();
        assert_eq!(
            cursor.apply(&mut journal, 1, &[ready.clone(), reset(2, false), blob(3)]),
            (None, false)
        );
        assert!(cursor.history_supported);
        assert!(cursor.session_load_supported);
        assert!(cursor.session_delete_supported);
        assert_eq!(journal[0].0, 2);
        cursor.trim(&mut journal, 0, 0);
        assert!(cursor.history_supported);
        assert!(cursor.session_load_supported);
        assert!(cursor.session_delete_supported);
        assert!(journal.is_empty());
        let unsupported = br#"{"seq":4,"item":"ready","capabilities":{"list":false}}"#.to_vec();
        cursor.apply(&mut journal, 4, &[unsupported]);
        cursor.apply(&mut journal, 1, &[ready]);
        assert!(!cursor.history_supported);
        assert!(!cursor.session_load_supported);
        assert!(!cursor.session_delete_supported);
    }

    #[test]
    fn agent_history_limits_bytes_and_items_without_rewinding_replay() {
        let mut cursor = AgentCursor::default();
        let mut journal = Vec::new();
        cursor.apply(&mut journal, 1, &[blob(1), blob(2), blob(3)]);
        let limit = blob(2).len() + blob(3).len();
        assert!(cursor.trim(&mut journal, limit, 10));
        assert_eq!(
            journal.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(cursor.bytes, limit);
        assert_eq!(cursor.last_seq, 3);
        assert!(cursor.trim(&mut journal, limit, 1));
        assert_eq!(journal[0].0, 3);
        assert_eq!(
            cursor.apply(&mut journal, 3, &[blob(3), blob(4)]),
            (None, false)
        );
        assert_eq!(
            journal.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            [3, 4]
        );
    }

    #[test]
    fn invalid_agent_items_do_not_skip_an_unseen_gap() {
        let mut cursor = AgentCursor::default();
        let mut journal = Vec::new();
        assert_eq!(
            cursor.apply(&mut journal, 3, &[b"invalid".to_vec()]),
            (Some(0), true)
        );
        assert_eq!(cursor.last_seq, 0);
        assert_eq!(
            cursor.apply(&mut journal, 1, &[b"invalid".to_vec(), blob(2)]),
            (None, true)
        );
        assert_eq!(cursor.last_seq, 2);
        assert_eq!(journal[0].0, 2);
    }

    #[test]
    fn queue_releases_byte_budget_when_frames_are_consumed() {
        let (sender, receiver) = inbox::channel(4, 10);
        assert!(sender.push(SocketEvent::Opened));
        assert!(sender.frame(6, || vec![0; 6]));
        assert!(sender.frame(4, || vec![1; 4]));
        assert_eq!(ready(receiver.recv()), Some(SocketEvent::Opened));
        assert_eq!(ready(receiver.recv()), Some(SocketEvent::Frame(vec![0; 6])));
        assert!(sender.frame(6, || vec![2; 6]));
        assert_eq!(ready(receiver.recv()), Some(SocketEvent::Frame(vec![1; 4])));
        assert_eq!(ready(receiver.recv()), Some(SocketEvent::Frame(vec![2; 6])));
        assert!(sender.push(SocketEvent::Focus(false)));
        assert_eq!(ready(receiver.recv()), Some(SocketEvent::Focus(false)));
    }

    #[test]
    fn queue_overflow_closes_without_copying_the_extra_frame() {
        let (sender, receiver) = inbox::channel(4, 10);
        assert!(sender.frame(8, || vec![0; 8]));
        assert!(!sender.frame(3, || panic!("overflow must be rejected before copying")));
        assert_eq!(ready(receiver.recv()), None);
        assert!(receiver.close_reason().contains("Too much pending output"));
        sender.close("Later close event".into());
        assert!(receiver.close_reason().contains("Too much pending output"));
    }

    #[test]
    fn a_full_event_queue_cannot_lose_its_close_reason() {
        let (sender, receiver) = inbox::channel(1, 10);
        assert!(sender.push(SocketEvent::Opened));
        sender.close("Daemon stopped".into());
        assert_eq!(ready(receiver.recv()), None);
        assert_eq!(receiver.close_reason(), "Daemon stopped");
        assert!(!sender.push(SocketEvent::Focus(true)));
    }

    #[test]
    fn event_capacity_overflow_also_closes_the_connection() {
        let (sender, receiver) = inbox::channel(1, 10);
        assert!(sender.push(SocketEvent::Focus(true)));
        assert!(!sender.frame(8, || panic!("a full queue must not copy another frame")));
        assert_eq!(ready(receiver.recv()), None);
        assert!(receiver.close_reason().contains("Too much pending output"));
    }
}
